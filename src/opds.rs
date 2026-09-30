use std::path::{Path, PathBuf};

use serde::Serialize;
use tokio::io::AsyncWriteExt;

use crate::{
    error::{AppError, AppResult},
    metadata,
    models::BookSummary,
    scanner::SUPPORTED_EXTENSIONS,
};

#[derive(Debug, Serialize)]
pub struct ImportReport {
    pub imported: usize,
    pub skipped: usize,
    pub warnings: Vec<String>,
}

pub fn catalog_xml(books: &[BookSummary], base_url: &str) -> String {
    let updated = chrono::Utc::now().to_rfc3339();
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<feed xmlns=\"http://www.w3.org/2005/Atom\" xmlns:opds=\"http://opds-spec.org/2010/catalog\">\n<id>urn:estante-livre:catalog</id><title>Estante Livre</title><updated>{updated}</updated><link rel=\"self\" href=\"{base_url}/api/opds\" type=\"application/atom+xml;profile=opds-catalog;kind=acquisition\"/>"
    );
    for book in books {
        let author = book.author.as_deref().unwrap_or("Autor nao informado");
        let media_type = mime_guess::from_ext(&book.format)
            .first_or_octet_stream()
            .essence_str()
            .to_string();
        xml.push_str(&format!(
            "\n<entry><id>urn:estante-livre:book:{id}</id><title>{title}</title><updated>{book_updated}</updated><author><name>{author}</name></author><category term=\"{format}\"/><link rel=\"http://opds-spec.org/acquisition/open-access\" href=\"{base_url}/api/books/{id}/file?download=true\" type=\"{media_type}\"/>",
            id = book.id,
            title = escape_xml(&book.title),
            book_updated = escape_xml(&book.updated_at),
            author = escape_xml(author),
            format = escape_xml(&book.format),
        ));
        if book.has_cover {
            xml.push_str(&format!(
                "<link rel=\"http://opds-spec.org/image\" href=\"{base_url}/api/books/{}/cover\" type=\"image/jpeg\"/>",
                book.id
            ));
        }
        xml.push_str("</entry>");
    }
    xml.push_str("\n</feed>");
    xml
}

pub async fn import_catalog(
    client: &reqwest::Client,
    feed_url: &str,
    library_root: &Path,
) -> AppResult<ImportReport> {
    let parsed = metadata::validate_public_http_url(feed_url)?;
    let response = metadata::public_get(parsed.clone()).await?;
    let parsed = response.url().clone();
    if response
        .content_length()
        .is_some_and(|length| length > 5 * 1024 * 1024)
    {
        return Err(AppError::BadRequest("catalogo OPDS grande demais".into()));
    }
    let bytes = metadata::limited_body(response, 5 * 1024 * 1024).await?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| AppError::BadRequest("catalogo OPDS nao esta em UTF-8".into()))?;
    let document = roxmltree::Document::parse(text)
        .map_err(|error| AppError::BadRequest(format!("XML OPDS invalido: {error}")))?;
    let mut imported = 0;
    let mut skipped = 0;
    let mut warnings = Vec::new();
    for entry in document
        .descendants()
        .filter(|node| node.tag_name().name() == "entry")
        .take(100)
    {
        let title = entry
            .children()
            .find(|node| node.tag_name().name() == "title")
            .and_then(|node| node.text())
            .unwrap_or("Livro importado");
        let acquisition = entry.children().find(|node| {
            node.tag_name().name() == "link"
                && node
                    .attribute("rel")
                    .is_some_and(|rel| rel.contains("acquisition"))
                && node.attribute("href").is_some()
        });
        let Some(href) = acquisition.and_then(|node| node.attribute("href")) else {
            skipped += 1;
            continue;
        };
        let resource_url = match parsed.join(href) {
            Ok(value) => value,
            Err(error) => {
                skipped += 1;
                warnings.push(format!("{title}: URL invalida: {error}"));
                continue;
            }
        };
        if let Err(error) = metadata::validate_public_http_url(resource_url.as_str()) {
            skipped += 1;
            warnings.push(format!("{title}: {error}"));
            continue;
        }
        let extension = resource_url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .and_then(|name| Path::new(name).extension())
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .or_else(|| {
                acquisition
                    .and_then(|node| node.attribute("type"))
                    .and_then(extension_from_mime)
                    .map(str::to_string)
            })
            .unwrap_or_default();
        if !SUPPORTED_EXTENSIONS.contains(&extension.as_str()) {
            skipped += 1;
            warnings.push(format!("{title}: formato nao suportado"));
            continue;
        }
        let filename = format!("{}.{}", safe_filename_stem(title), extension);
        let destination = unique_destination(library_root, &filename).await?;
        match download_book(client, resource_url, &destination).await {
            Ok(()) => imported += 1,
            Err(error) => {
                skipped += 1;
                warnings.push(format!("{title}: {error}"));
            }
        }
    }
    warnings.truncate(30);
    Ok(ImportReport {
        imported,
        skipped,
        warnings,
    })
}

async fn download_book(
    _client: &reqwest::Client,
    url: url::Url,
    destination: &Path,
) -> AppResult<()> {
    let mut response = metadata::public_get(url).await?;
    if response
        .content_length()
        .is_some_and(|length| length > 1024 * 1024 * 1024)
    {
        return Err(AppError::BadRequest("livro OPDS excede 1 GiB".into()));
    }
    let mut cleanup = crate::pending_file::PendingFile::default();
    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .await?;
    cleanup.track(destination.to_path_buf());
    let mut written = 0_u64;
    while let Some(chunk) = response.chunk().await? {
        written += chunk.len() as u64;
        if written > 1024 * 1024 * 1024 {
            return Err(AppError::BadRequest("livro OPDS excede 1 GiB".into()));
        }
        output.write_all(&chunk).await?;
    }
    output.flush().await?;
    cleanup.commit();
    Ok(())
}

async fn unique_destination(root: &Path, filename: &str) -> AppResult<PathBuf> {
    let requested = root.join(filename);
    if !requested.exists() {
        return Ok(requested);
    }
    let path = Path::new(filename);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("livro");
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    for index in 2..=10_000 {
        let candidate = root.join(format!("{stem} ({index}).{extension}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(AppError::BadRequest(
        "nao foi possivel escolher nome para o livro OPDS".into(),
    ))
}

fn safe_filename_stem(value: &str) -> String {
    let cleaned = value
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, ' ' | '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    let cleaned = cleaned
        .trim_matches([' ', '.', '_'])
        .chars()
        .take(120)
        .collect::<String>();
    if cleaned.is_empty() {
        "livro".into()
    } else {
        cleaned
    }
}

fn extension_from_mime(value: &str) -> Option<&'static str> {
    match value.split(';').next()?.trim() {
        "application/pdf" => Some("pdf"),
        "application/epub+zip" => Some("epub"),
        "application/x-mobipocket-ebook" => Some("mobi"),
        "application/vnd.amazon.ebook" => Some("azw"),
        "application/vnd.comicbook+zip" => Some("cbz"),
        "text/plain" => Some("txt"),
        "text/html" => Some("html"),
        _ => None,
    }
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_opds_filenames_and_xml() {
        assert_eq!(safe_filename_stem("Livro: teste/ok"), "Livro_ teste_ok");
        assert_eq!(escape_xml("A & <B>"), "A &amp; &lt;B&gt;");
    }
}
