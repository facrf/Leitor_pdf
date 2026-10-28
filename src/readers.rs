use std::{
    collections::HashMap,
    fs::File,
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
    let bytes = crate::bounded_io::read_limited(&mut entry, max_size)?;
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
            2 => palmdoc_decompress_append(record, &mut output)?,
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

fn palmdoc_decompress_append(input: &[u8], output: &mut Vec<u8>) -> AppResult<()> {
    let record_start = output.len();
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
                if distance == 0 || distance > (output.len() - record_start) {
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
    Ok(())
}

#[cfg(test)]
fn palmdoc_decompress(input: &[u8]) -> AppResult<Vec<u8>> {
    let mut output = Vec::with_capacity(input.len() * 2);
    palmdoc_decompress_append(input, &mut output)?;
    Ok(output)
}

fn strip_active_html(input: &str) -> String {
    // O iframe tambem recebe CSP sandbox. Esta remocao reduz conteudo ativo em leitores antigos.
    let mut output = input.to_string();
    for tag in ["script", "iframe", "object", "embed"] {
        let open_tag = format!("<{tag}");
        let close_tag = format!("</{tag}>");
        let mut search_from = 0;
        while search_from < output.len() {
            let lower = output[search_from..].to_ascii_lowercase();
            let Some(rel_start) = lower.find(&open_tag) else {
                break;
            };
            let start = search_from + rel_start;
            let after_tag = start + open_tag.len();
            let delim = output.as_bytes().get(after_tag).copied();
            if !matches!(delim, Some(b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/')) {
                search_from = after_tag;
                continue;
            }
            if tag == "embed" {
                if let Some(rel_gt) = lower[rel_start..].find('>') {
                    let end = start + rel_gt + 1;
                    output.replace_range(start..end, "");
                    search_from = start;
                } else {
                    output.truncate(start);
                    break;
                }
            } else {
                let rel_gt = lower[rel_start..].find('>');
                let is_self_closing = rel_gt.is_some_and(|gt| {
                    let tag_slice = lower[rel_start..rel_start + gt].trim_end();
                    tag_slice.ends_with('/')
                });
                if is_self_closing {
                    let end = start + rel_gt.unwrap() + 1;
                    output.replace_range(start..end, "");
                    search_from = start;
                    continue;
                }
                if let Some(rel_close) = lower[rel_start..].find(&close_tag) {
                    let end = start + rel_close + close_tag.len();
                    output.replace_range(start..end, "");
                    search_from = start;
                } else {
                    output.truncate(start);
                    break;
                }
            }
        }
    }
    output
}

fn escape_html(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            _ => output.push(ch),
        }
    }
    output
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
    let bytes = crate::bounded_io::read_limited(&mut entry, limit)?;
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

pub(crate) fn natural_key(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut digits = None;
    for character in value.chars().map(|c| c.to_ascii_lowercase()) {
        let is_digit = character.is_ascii_digit();
        if digits.is_some_and(|previous| previous != is_digit) {
            if digits == Some(true) {
                parts.push(format!("{:0>20}", current));
            } else {
                parts.push(std::mem::take(&mut current));
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

/// Remove tags HTML e scripts/estilos, retornando o texto limpo com espacos normalizados.
pub fn strip_html_tags_to_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_script_or_style = false;
    let mut tag_buf = String::new();
    let mut chars = html.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '<' {
            in_tag = true;
            tag_buf.clear();
            while let Some(&next) = chars.peek() {
                if next == '>' || next.is_whitespace() {
                    break;
                }
                tag_buf.push(next.to_ascii_lowercase());
                chars.next();
            }
            if tag_buf == "script" || tag_buf == "style" {
                in_script_or_style = true;
            } else if tag_buf == "/script" || tag_buf == "/style" {
                in_script_or_style = false;
            }
            continue;
        } else if ch == '>' {
            in_tag = false;
            text.push(' ');
            continue;
        }
        if !in_tag && !in_script_or_style {
            text.push(ch);
        }
    }
    let unescaped = text
        .replace("&nbsp;", " ")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&#39;", "'");
    normalize_whitespace(&unescaped)
}

/// Normaliza espacos em branco consecutivos em um unico espaco.
pub fn normalize_whitespace(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut last_was_space = true;
    for ch in input.chars() {
        if ch.is_whitespace() {
            if !last_was_space {
                output.push(' ');
                last_was_space = true;
            }
        } else {
            output.push(ch);
            last_was_space = false;
        }
    }
    output.trim().to_string()
}

fn truncate_to_char_boundary(s: &mut String, max_bytes: usize) {
    if s.len() > max_bytes {
        let mut end = max_bytes;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
}

pub fn is_tesseract_available() -> bool {
    std::process::Command::new("tesseract")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn is_timeout_available() -> bool {
    std::process::Command::new("timeout")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn extract_pdf_ocr_sample(path: &Path, max_bytes: usize, timeout_secs: u64) -> Option<String> {
    if !is_tesseract_available() {
        return None;
    }
    let tmp_base = std::env::var("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(".cache/tmp"));
    let temp_dir = tmp_base.join(format!("estante-ocr-{}", rand::random::<u64>()));
    if std::fs::create_dir_all(&temp_dir).is_err() {
        return None;
    }
    let temp_prefix = temp_dir.join("page");

    let timeout_str = timeout_secs.to_string();
    let has_timeout = is_timeout_available() && timeout_secs > 0;

    let mut ppm_cmd = if has_timeout {
        let mut cmd = std::process::Command::new("timeout");
        cmd.arg(&timeout_str);
        cmd.arg("pdftoppm");
        cmd
    } else {
        std::process::Command::new("pdftoppm")
    };

    let pdftoppm_status = ppm_cmd
        .arg("-png")
        .arg("-r")
        .arg("150")
        .arg("-f")
        .arg("1")
        .arg("-l")
        .arg("2")
        .arg(path)
        .arg(&temp_prefix)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    let mut text = String::new();
    if pdftoppm_status.map(|s| s.success()).unwrap_or(false) {
        if let Ok(entries) = std::fs::read_dir(&temp_dir) {
            let mut pngs: Vec<_> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|ext| ext == "png"))
                .collect();
            pngs.sort();
            for png in pngs {
                let mut cmd = if has_timeout {
                    let mut c = std::process::Command::new("timeout");
                    c.arg(&timeout_str);
                    c.arg("tesseract");
                    c
                } else {
                    std::process::Command::new("tesseract")
                };
                cmd.arg(&png).arg("stdout").arg("-l").arg("por+eng");
                let ocr_output = cmd.output().or_else(|_| {
                    let mut fallback = if has_timeout {
                        let mut c = std::process::Command::new("timeout");
                        c.arg(&timeout_str);
                        c.arg("tesseract");
                        c
                    } else {
                        std::process::Command::new("tesseract")
                    };
                    fallback.arg(&png).arg("stdout").output()
                });
                if let Ok(output) = ocr_output {
                    if output.status.success() {
                        if let Ok(page_text) = String::from_utf8(output.stdout) {
                            let normalized = normalize_whitespace(&page_text);
                            if !normalized.is_empty() {
                                text.push_str(&normalized);
                                text.push(' ');
                            }
                        }
                    }
                }
                if text.len() >= max_bytes {
                    break;
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(&temp_dir);
    truncate_to_char_boundary(&mut text, max_bytes);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn extract_pdf_text_sample_with_options(
    path: &Path,
    max_bytes: usize,
    ocr_enabled: bool,
    ocr_timeout_secs: u64,
) -> Option<String> {
    if let Ok(doc) = lopdf::Document::load(path) {
        let pages = doc.get_pages();
        let mut page_numbers: Vec<u32> = pages.keys().copied().collect();
        page_numbers.sort_unstable();
        let mut text = String::new();
        for page_num in page_numbers.into_iter().take(50) {
            if let Ok(page_text) = doc.extract_text(&[page_num]) {
                let normalized = normalize_whitespace(&page_text);
                if !normalized.is_empty() {
                    text.push_str(&normalized);
                    text.push(' ');
                }
                if text.len() >= max_bytes {
                    break;
                }
            }
        }
        truncate_to_char_boundary(&mut text, max_bytes);
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    // Fallback gracioso para OCR em PDFs escaneados / baseados em imagem
    if ocr_enabled {
        extract_pdf_ocr_sample(path, max_bytes, ocr_timeout_secs)
    } else {
        None
    }
}

fn extract_epub_text_sample(path: &Path, max_bytes: usize) -> Option<String> {
    let manifest = epub_manifest(path).ok()?;
    let mut combined = String::new();
    for chapter in manifest.chapters.iter().take(40) {
        if let Ok((bytes, _)) = archive_resource(path, &chapter.href, 512 * 1024) {
            let html = match String::from_utf8(bytes) {
                Ok(s) => s,
                Err(e) => WINDOWS_1252.decode(e.as_bytes()).0.into_owned(),
            };
            let text = strip_html_tags_to_text(&html);
            if !text.is_empty() {
                combined.push_str(&text);
                combined.push(' ');
            }
            if combined.len() >= max_bytes {
                break;
            }
        }
    }
    truncate_to_char_boundary(&mut combined, max_bytes);
    let trimmed = combined.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn mobi_text_sample(path: &Path, max_bytes: usize) -> Option<String> {
    let data = std::fs::read(path).ok()?;
    if data.len() < 86 {
        return None;
    }
    let record_count = be_u16(&data, 76).ok()? as usize;
    if record_count < 2 || data.len() < 78 + record_count * 8 {
        return None;
    }
    let mut offsets = Vec::with_capacity(record_count + 1);
    for index in 0..record_count {
        offsets.push(be_u32(&data, 78 + index * 8).ok()? as usize);
    }
    offsets.push(data.len());
    let first = slice_record(&data, &offsets, 0).ok()?;
    if first.len() < 16 {
        return None;
    }
    let compression = be_u16(first, 0).ok()?;
    let text_length = be_u32(first, 4).ok()? as usize;
    let text_records = be_u16(first, 8).ok()? as usize;
    let encryption = be_u16(first, 12).ok()?;
    if encryption != 0 || text_records >= record_count {
        return None;
    }
    let mut output = Vec::with_capacity(max_bytes.min(text_length));
    for index in 1..=text_records {
        let record = slice_record(&data, &offsets, index).ok()?;
        match compression {
            1 => output.extend_from_slice(record),
            2 => {
                let _ = palmdoc_decompress_append(record, &mut output);
            }
            _ => return None,
        }
        if output.len() >= max_bytes {
            break;
        }
    }
    if output.len() > max_bytes {
        output.truncate(max_bytes);
    }
    let text = match String::from_utf8(output) {
        Ok(value) => value,
        Err(error) => WINDOWS_1252.decode(error.as_bytes()).0.into_owned(),
    };
    let cleaned = strip_html_tags_to_text(&text);
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn extract_plain_or_html_sample(path: &Path, format: &str, max_bytes: usize) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let limit = if matches!(format, "html" | "htm" | "fb2") {
        (max_bytes * 3) as u64
    } else {
        max_bytes as u64
    };
    let bytes = crate::bounded_io::read_limited(&mut file, limit).ok()?;
    let content = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => WINDOWS_1252.decode(e.as_bytes()).0.into_owned(),
    };
    let mut text = if matches!(format, "html" | "htm" | "fb2") {
        strip_html_tags_to_text(&content)
    } else {
        normalize_whitespace(&content)
    };
    truncate_to_char_boundary(&mut text, max_bytes);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Extrai uma amostra de texto representativa (ate max_bytes) para indexacao no SQLite FTS5.
#[allow(dead_code)]
pub fn extract_text_sample(path: &Path, format: &str, max_bytes: usize) -> Option<String> {
    extract_text_sample_with_options(path, format, max_bytes, true, 15)
}

/// Extrai uma amostra de texto com controle explicito de OCR e timeout.
pub fn extract_text_sample_with_options(
    path: &Path,
    format: &str,
    max_bytes: usize,
    ocr_enabled: bool,
    ocr_timeout_secs: u64,
) -> Option<String> {
    match format {
        "txt" | "md" | "html" | "htm" | "fb2" => {
            extract_plain_or_html_sample(path, format, max_bytes)
        }
        "epub" => extract_epub_text_sample(path, max_bytes),
        "mobi" | "azw" | "azw3" => mobi_text_sample(path, max_bytes),
        "pdf" => {
            extract_pdf_text_sample_with_options(path, max_bytes, ocr_enabled, ocr_timeout_secs)
        }
        _ => None,
    }
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

    #[test]
    fn strips_active_html_without_truncating_void_elements() {
        let input = "Capítulo 1: <embed src='audio.mp3'> Continuação do texto.";
        assert_eq!(
            strip_active_html(input),
            "Capítulo 1:  Continuação do texto."
        );
        let script_test = "A <script>alert(1)</script> B <script src='x.js'/> C";
        assert_eq!(strip_active_html(script_test), "A  B  C");
        let safe_text = "Uma <scripture>passagem bíblica</scripture> não é script.";
        assert_eq!(strip_active_html(safe_text), safe_text);
    }

    #[test]
    fn strips_html_tags_and_scripts_for_text_search() {
        let input = "<p>Parágrafo 1.</p><script>console.log('ignorar');</script><style>.css{}</style><p>Parágrafo 2 &amp; texto &nbsp; limpo.</p>";
        assert_eq!(
            strip_html_tags_to_text(input),
            "Parágrafo 1. Parágrafo 2 & texto limpo."
        );
    }

    #[test]
    fn ocr_sample_returns_gracefully_when_unavailable_or_non_pdf() {
        let path = Path::new("tests/fixtures/nao-existe.pdf");
        assert_eq!(extract_pdf_ocr_sample(path, 1024, 5), None);
    }
}

#[cfg(test)]
#[path = "../tests/rust/epub-regression.rs"]
mod regression_tests;
