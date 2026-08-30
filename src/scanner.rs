use std::{
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
    time::UNIX_EPOCH,
};

use serde::Serialize;
use walkdir::WalkDir;
use zip::ZipArchive;

use crate::error::{AppError, AppResult};

pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "pdf", "epub", "mobi", "azw", "azw3", "cbz", "txt", "md", "html", "htm", "fb2",
];

#[derive(Debug, Clone)]
pub struct ScannedBook {
    pub title: String,
    pub author: Option<String>,
    pub filename: String,
    pub relative_path: String,
    pub format: String,
    pub size: i64,
    pub modified_at: i64,
    pub page_count: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ScanReport {
    pub found: usize,
    pub added: usize,
    pub updated: usize,
    pub skipped: usize,
    pub warnings: Vec<String>,
}

pub fn scan(root: &Path) -> AppResult<(Vec<ScannedBook>, usize, Vec<String>)> {
    let canonical_root = root.canonicalize().map_err(|error| {
        AppError::BadRequest(format!(
            "nao foi possivel abrir a pasta {}: {error}",
            root.display()
        ))
    })?;
    if !canonical_root.is_dir() {
        return Err(AppError::BadRequest(
            "o caminho da biblioteca nao e uma pasta".into(),
        ));
    }

    let mut books = Vec::new();
    let mut skipped = 0;
    let mut warnings = Vec::new();
    for entry in WalkDir::new(&canonical_root).follow_links(false) {
        let entry = match entry {
            Ok(value) => value,
            Err(error) => {
                skipped += 1;
                warnings.push(error.to_string());
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let extension = entry
            .path()
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if !SUPPORTED_EXTENSIONS.contains(&extension.as_str()) {
            continue;
        }
        match scan_file(&canonical_root, entry.path(), &extension) {
            Ok(book) => books.push(book),
            Err(error) => {
                skipped += 1;
                warnings.push(format!("{}: {error}", entry.path().display()));
            }
        }
    }
    warnings.truncate(20);
    Ok((books, skipped, warnings))
}

fn scan_file(root: &Path, path: &Path, extension: &str) -> AppResult<ScannedBook> {
    let metadata = path.metadata()?;
    let relative = path
        .strip_prefix(root)
        .map_err(|_| AppError::Internal("arquivo fora da biblioteca".into()))?;
    let relative_path = path_to_safe_relative(relative)?;
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| AppError::BadRequest("nome de arquivo invalido".into()))?
        .to_string();
    let fallback_title = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(&filename)
        .replace(['_', '-'], " ");
    let (title, author, page_count) = match extension {
        "pdf" => pdf_metadata(path, &fallback_title),
        "epub" => epub_metadata(path, &fallback_title),
        _ => (fallback_title, None, None),
    };
    let modified_at = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();
    Ok(ScannedBook {
        title,
        author,
        filename,
        relative_path,
        format: extension.to_string(),
        size: metadata.len() as i64,
        modified_at,
        page_count,
    })
}

/// Converte um caminho ja relativo em representacao portavel e rejeita travessia.
pub fn path_to_safe_relative(path: &Path) -> AppResult<String> {
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(AppError::BadRequest("caminho de arquivo inseguro".into()));
    }
    path.to_str()
        .map(|value| value.replace('\\', "/"))
        .ok_or_else(|| AppError::BadRequest("o caminho do arquivo nao e UTF-8".into()))
}

/// Resolve somente caminhos previamente catalogados e confirma que continuam na raiz.
pub fn resolve_book_path(root: &Path, relative: &str) -> AppResult<PathBuf> {
    let relative_path = Path::new(relative);
    path_to_safe_relative(relative_path)?;
    let canonical_root = root.canonicalize()?;
    let candidate = canonical_root.join(relative_path).canonicalize()?;
    if !candidate.starts_with(&canonical_root) || !candidate.is_file() {
        return Err(AppError::NotFound);
    }
    Ok(candidate)
}

fn pdf_metadata(path: &Path, fallback: &str) -> (String, Option<String>, Option<i64>) {
    let Ok(document) = lopdf::Document::load(path) else {
        return (fallback.to_string(), None, None);
    };
    let pages = Some(document.get_pages().len() as i64);
    let Some(info_ref) = document
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|value| value.as_reference().ok())
    else {
        return (fallback.to_string(), None, pages);
    };
    let Some(info) = document
        .get_object(info_ref)
        .ok()
        .and_then(|value| value.as_dict().ok())
    else {
        return (fallback.to_string(), None, pages);
    };
    let title = pdf_string(info.get(b"Title").ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| fallback.to_string());
    let author = pdf_string(info.get(b"Author").ok()).filter(|value| !value.trim().is_empty());
    (title, author, pages)
}

fn pdf_string(object: Option<&lopdf::Object>) -> Option<String> {
    let bytes = object?.as_str().ok()?;
    Some(
        String::from_utf8_lossy(bytes)
            .trim_matches(char::from(0))
            .to_string(),
    )
}

fn epub_metadata(path: &Path, fallback: &str) -> (String, Option<String>, Option<i64>) {
    let Ok(file) = File::open(path) else {
        return (fallback.into(), None, None);
    };
    let Ok(mut archive) = ZipArchive::new(file) else {
        return (fallback.into(), None, None);
    };
    let Ok(container) = read_zip_text(&mut archive, "META-INF/container.xml", 512 * 1024) else {
        return (fallback.into(), None, None);
    };
    let Ok(container_doc) = roxmltree::Document::parse(&container) else {
        return (fallback.into(), None, None);
    };
    let Some(opf_path) = container_doc
        .descendants()
        .find(|node| node.has_tag_name("rootfile"))
        .and_then(|node| node.attribute("full-path"))
    else {
        return (fallback.into(), None, None);
    };
    let Ok(opf) = read_zip_text(&mut archive, opf_path, 4 * 1024 * 1024) else {
        return (fallback.into(), None, None);
    };
    let Ok(document) = roxmltree::Document::parse(&opf) else {
        return (fallback.into(), None, None);
    };
    let find_text = |name: &str| {
        document
            .descendants()
            .find(|node| node.tag_name().name() == name)
            .and_then(|node| node.text())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    (
        find_text("title").unwrap_or_else(|| fallback.into()),
        find_text("creator"),
        None,
    )
}

fn read_zip_text(archive: &mut ZipArchive<File>, name: &str, max_size: u64) -> AppResult<String> {
    let mut entry = archive.by_name(name)?;
    if entry.size() > max_size {
        return Err(AppError::BadRequest("entrada EPUB grande demais".into()));
    }
    let mut content = String::new();
    entry.read_to_string(&mut content)?;
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_parent_traversal() {
        assert!(path_to_safe_relative(Path::new("../segredo.pdf")).is_err());
        assert!(path_to_safe_relative(Path::new("livros/ok.pdf")).is_ok());
    }
}
