use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
};

use encoding_rs::WINDOWS_1252;
use serde::Serialize;
use zip::ZipArchive;

use crate::error::{AppError, AppResult};

#[derive(Debug, Serialize)]
pub struct EpubManifest {
    pub chapters: Vec<EpubChapter>,
}

#[derive(Debug, Serialize)]
pub struct EpubChapter {
    pub index: usize,
    pub href: String,
    pub label: String,
}

#[derive(Debug, Serialize)]
pub struct ComicManifest {
    pub pages: Vec<String>,
}

pub fn epub_manifest(path: &Path) -> AppResult<EpubManifest> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file)?;
    let container = zip_text(&mut archive, "META-INF/container.xml", 512 * 1024)?;
    let container_doc = roxmltree::Document::parse(&container)
        .map_err(|error| AppError::BadRequest(format!("container EPUB invalido: {error}")))?;
    let opf_path = container_doc
        .descendants()
        .find(|node| node.has_tag_name("rootfile"))
        .and_then(|node| node.attribute("full-path"))
        .ok_or_else(|| AppError::BadRequest("EPUB sem pacote OPF".into()))?;
    validate_archive_path(opf_path)?;
    let opf = zip_text(&mut archive, opf_path, 4 * 1024 * 1024)?;
    let document = roxmltree::Document::parse(&opf)
        .map_err(|error| AppError::BadRequest(format!("pacote EPUB invalido: {error}")))?;
    let base = Path::new(opf_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let mut manifest = HashMap::new();
    for item in document
        .descendants()
        .filter(|node| node.has_tag_name("item"))
    {
        if let (Some(id), Some(href)) = (item.attribute("id"), item.attribute("href")) {
            manifest.insert(id.to_string(), href.to_string());
        }
    }
    let mut chapters = Vec::new();
    for (index, itemref) in document
        .descendants()
        .filter(|node| node.has_tag_name("itemref"))
        .enumerate()
    {
        let Some(idref) = itemref.attribute("idref") else {
            continue;
        };
        let Some(href) = manifest.get(idref) else {
            continue;
        };
        let full_path = normalize_archive_join(base, href)?;
        chapters.push(EpubChapter {
            index,
            href: full_path,
            label: format!("Capitulo {}", index + 1),
        });
    }
    if chapters.is_empty() {
        return Err(AppError::BadRequest("EPUB sem capitulos legiveis".into()));
    }
    Ok(EpubManifest { chapters })
}

pub fn archive_resource(
    path: &Path,
    resource: &str,
    max_size: u64,
) -> AppResult<(Vec<u8>, String)> {
    validate_archive_path(resource)?;
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file)?;
    let mut entry = archive.by_name(resource)?;
    if entry.is_dir() || entry.size() > max_size {
        return Err(AppError::BadRequest(
            "recurso compactado invalido ou grande demais".into(),
        ));
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut bytes)?;
    let mime = mime_guess::from_path(resource)
        .first_or_octet_stream()
        .to_string();
    Ok((bytes, mime))
}

pub fn comic_manifest(path: &Path) -> AppResult<ComicManifest> {
    let file = File::open(path)?;
    let archive = ZipArchive::new(file)?;
    let mut pages: Vec<String> = archive
        .file_names()
        .filter(|name| {
            let extension = Path::new(name)
                .extension()
                .and_then(|value| value.to_str())
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();
            matches!(extension.as_str(), "jpg" | "jpeg" | "png" | "webp" | "gif")
                && validate_archive_path(name).is_ok()
        })
        .map(str::to_string)
        .collect();
    pages.sort_by_key(|value| natural_key(value));
    Ok(ComicManifest { pages })
}

/// Extrai o fluxo textual PalmDOC de MOBI/AZW classico. Arquivos com DRM sao recusados.
pub fn mobi_html(path: &Path) -> AppResult<Vec<u8>> {
    let data = std::fs::read(path)?;
    if data.len() < 86 {
        return Err(AppError::BadRequest("arquivo MOBI truncado".into()));
    }
    let record_count = be_u16(&data, 76)? as usize;
    if record_count < 2 || data.len() < 78 + record_count * 8 {
        return Err(AppError::BadRequest(
            "tabela de registros MOBI invalida".into(),
        ));
    }
    let mut offsets = Vec::with_capacity(record_count + 1);
    for index in 0..record_count {
        offsets.push(be_u32(&data, 78 + index * 8)? as usize);
    }
    offsets.push(data.len());
    let first = slice_record(&data, &offsets, 0)?;
    if first.len() < 16 {
        return Err(AppError::BadRequest("cabecalho PalmDOC ausente".into()));
    }
    let compression = be_u16(first, 0)?;
    let text_length = be_u32(first, 4)? as usize;
    let text_records = be_u16(first, 8)? as usize;
    let encryption = be_u16(first, 12)?;
    if encryption != 0 {
        return Err(AppError::Unsupported("MOBI com DRM/criptografia".into()));
    }
    if text_length > 128 * 1024 * 1024 || text_records >= record_count {
        return Err(AppError::BadRequest("dimensoes MOBI invalidas".into()));
    }
    let mut output = Vec::with_capacity(text_length.min(16 * 1024 * 1024));
    for index in 1..=text_records {
        let record = slice_record(&data, &offsets, index)?;
        match compression {
            1 => output.extend_from_slice(record),
            2 => output.extend_from_slice(&palmdoc_decompress(record)?),
            value => return Err(AppError::Unsupported(format!("compressao MOBI {value}"))),
        }
        if output.len() > 128 * 1024 * 1024 {
            return Err(AppError::BadRequest("texto MOBI grande demais".into()));
        }
    }
    output.truncate(text_length.min(output.len()));
    let text = match String::from_utf8(output) {
        Ok(value) => value,
        Err(error) => WINDOWS_1252.decode(error.as_bytes()).0.into_owned(),
    };
    let safe = strip_active_html(&text);
    Ok(format!(
        "<!doctype html><meta charset=\"utf-8\"><style>{}</style><main>{}</main>",
        READER_STYLE, safe
    )
    .into_bytes())
}

pub fn text_html(path: &Path, format: &str) -> AppResult<Vec<u8>> {
    let metadata = path.metadata()?;
    if metadata.len() > 64 * 1024 * 1024 {
        return Err(AppError::BadRequest(
            "arquivo de texto grande demais".into(),
        ));
    }
    let bytes = std::fs::read(path)?;
    let content = match String::from_utf8(bytes) {
        Ok(value) => value,
        Err(error) => WINDOWS_1252.decode(error.as_bytes()).0.into_owned(),
    };
    let body = if matches!(format, "html" | "htm" | "fb2") {
        strip_active_html(&content)
    } else {
        format!("<pre>{}</pre>", escape_html(&content))
    };
    Ok(format!(
        "<!doctype html><meta charset=\"utf-8\"><style>{}</style><main>{}</main>",
        READER_STYLE, body
    )
    .into_bytes())
}

fn palmdoc_decompress(input: &[u8]) -> AppResult<Vec<u8>> {
    let mut output = Vec::with_capacity(input.len() * 2);
    let mut cursor = 0;
    while cursor < input.len() {
        let byte = input[cursor];
        cursor += 1;
        match byte {
            0x00 => output.push(0),
            0x01..=0x08 => {
                let count = byte as usize;
                if cursor + count > input.len() {
                    return Err(AppError::BadRequest("registro PalmDOC truncado".into()));
                }
                output.extend_from_slice(&input[cursor..cursor + count]);
                cursor += count;
            }
            0x09..=0x7f => output.push(byte),
            0x80..=0xbf => {
                if cursor >= input.len() {
                    return Err(AppError::BadRequest("referencia PalmDOC truncada".into()));
                }
                let pair = u16::from(byte) << 8 | u16::from(input[cursor]);
                cursor += 1;
                let distance = ((pair >> 3) & 0x07ff) as usize;
                let length = ((pair & 0x0007) + 3) as usize;
                if distance == 0 || distance > output.len() {
                    return Err(AppError::BadRequest("referencia PalmDOC invalida".into()));
                }
                for _ in 0..length {
                    let value = output[output.len() - distance];
                    output.push(value);
                }
            }
            0xc0..=0xff => {
                output.push(b' ');
                output.push(byte ^ 0x80);
            }
        }
    }
    Ok(output)
}

fn strip_active_html(input: &str) -> String {
    // O iframe tambem recebe CSP sandbox. Esta remocao reduz conteudo ativo em leitores antigos.
    let mut output = input.to_string();
    for tag in ["script", "iframe", "object", "embed"] {
        loop {
            let lower = output.to_ascii_lowercase();
            let Some(start) = lower.find(&format!("<{tag}")) else {
                break;
            };
            let Some(relative_end) = lower[start..].find(&format!("</{tag}>")) else {
                output.truncate(start);
                break;
            };
            let end = start + relative_end + tag.len() + 3;
            output.replace_range(start..end, "");
        }
    }
    output
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn validate_archive_path(path: &str) -> AppResult<()> {
    let path = Path::new(path);
    if path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(AppError::BadRequest("caminho interno inseguro".into()));
    }
    Ok(())
}

fn normalize_archive_join(base: &Path, href: &str) -> AppResult<String> {
    let href = href.split('#').next().unwrap_or(href);
    // Hrefs sao URIs, enquanto nomes no ZIP sao caminhos decodificados.
    let mut bytes = Vec::with_capacity(href.len());
    let mut remaining = href.as_bytes();
    while let Some((&first, rest)) = remaining.split_first() {
        if first == b'%' {
            let hex = remaining
                .get(1..3)
                .ok_or_else(|| AppError::BadRequest("escape EPUB incompleto".into()))?;
            let hex = std::str::from_utf8(hex)
                .map_err(|_| AppError::BadRequest("escape EPUB invalido".into()))?;
            let byte = u8::from_str_radix(hex, 16)
                .map_err(|_| AppError::BadRequest("escape EPUB invalido".into()))?;
            bytes.push(byte);
            remaining = &remaining[3..];
        } else {
            bytes.push(first);
            remaining = rest;
        }
    }
    let href = String::from_utf8(bytes)
        .map_err(|_| AppError::BadRequest("caminho EPUB nao e UTF-8".into()))?;
    if href.contains(['\\', '\0']) {
        return Err(AppError::BadRequest("caminho EPUB inseguro".into()));
    }
    let mut result = PathBuf::new();
    for part in base.join(&href).components() {
        match part {
            Component::Normal(value) => result.push(value),
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() {
                    return Err(AppError::BadRequest("caminho EPUB fora do arquivo".into()));
                }
            }
            _ => return Err(AppError::BadRequest("caminho EPUB inseguro".into())),
        }
    }
    result
        .to_str()
        .map(|value| value.replace('\\', "/"))
        .ok_or_else(|| AppError::BadRequest("caminho EPUB invalido".into()))
}

fn zip_text(archive: &mut ZipArchive<File>, name: &str, limit: u64) -> AppResult<String> {
    let (bytes, _) = archive_resource_from(archive, name, limit)?;
    String::from_utf8(bytes).map_err(|_| AppError::BadRequest("XML do EPUB nao e UTF-8".into()))
}

fn archive_resource_from(
    archive: &mut ZipArchive<File>,
    name: &str,
    limit: u64,
) -> AppResult<(Vec<u8>, String)> {
    validate_archive_path(name)?;
    let mut entry = archive.by_name(name)?;
    if entry.size() > limit {
        return Err(AppError::BadRequest(
            "entrada compactada grande demais".into(),
        ));
    }
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes)?;
    Ok((
        bytes,
        mime_guess::from_path(name)
            .first_or_octet_stream()
            .to_string(),
    ))
}

fn slice_record<'a>(data: &'a [u8], offsets: &[usize], index: usize) -> AppResult<&'a [u8]> {
    let start = *offsets
        .get(index)
        .ok_or_else(|| AppError::BadRequest("registro MOBI ausente".into()))?;
    let end = *offsets
        .get(index + 1)
        .ok_or_else(|| AppError::BadRequest("registro MOBI ausente".into()))?;
    if start > end || end > data.len() {
        return Err(AppError::BadRequest("offset MOBI invalido".into()));
    }
    Ok(&data[start..end])
}

fn be_u16(data: &[u8], offset: usize) -> AppResult<u16> {
    let bytes: [u8; 2] = data
        .get(offset..offset + 2)
        .ok_or_else(|| AppError::BadRequest("arquivo truncado".into()))?
        .try_into()
        .map_err(|_| AppError::BadRequest("arquivo truncado".into()))?;
    Ok(u16::from_be_bytes(bytes))
}

fn be_u32(data: &[u8], offset: usize) -> AppResult<u32> {
    let bytes: [u8; 4] = data
        .get(offset..offset + 4)
        .ok_or_else(|| AppError::BadRequest("arquivo truncado".into()))?
        .try_into()
        .map_err(|_| AppError::BadRequest("arquivo truncado".into()))?;
    Ok(u32::from_be_bytes(bytes))
}

fn natural_key(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut digits = None;
    for character in value.to_ascii_lowercase().chars() {
        let is_digit = character.is_ascii_digit();
        if digits.is_some_and(|previous| previous != is_digit) {
            if digits == Some(true) {
                parts.push(format!("{:0>20}", current));
            } else {
                parts.push(current.clone());
            }
            current.clear();
        }
        digits = Some(is_digit);
        current.push(character);
    }
    if !current.is_empty() {
        if digits == Some(true) {
            parts.push(format!("{:0>20}", current));
        } else {
            parts.push(current);
        }
    }
    parts
}

const READER_STYLE: &str = r#"
:root{color-scheme:light dark}body{margin:0;background:#f7f2e8;color:#24231f;font:19px/1.75 Georgia,serif}
main{max-width:760px;margin:auto;padding:5vh 7vw 14vh}img{max-width:100%;height:auto}a{color:#9b4b2f}
pre{white-space:pre-wrap;overflow-wrap:anywhere;font:17px/1.7 ui-monospace,monospace}
@media(prefers-color-scheme:dark){body{background:#171815;color:#e8e2d5}a{color:#e39a76}}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decompresses_literal_palmdoc() {
        assert_eq!(palmdoc_decompress(b"ola").unwrap(), b"ola");
        assert_eq!(palmdoc_decompress(&[0xc1]).unwrap(), b" A");
    }

    #[test]
    fn blocks_archive_traversal() {
        assert!(validate_archive_path("../outside").is_err());
        assert!(validate_archive_path("OPS/chapter.xhtml").is_ok());
    }
}

#[cfg(test)]
#[path = "../tests/rust/epub-regression.rs"]
mod regression_tests;
