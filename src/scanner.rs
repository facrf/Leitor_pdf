use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Component, Path, PathBuf},
    thread,
    time::{Duration, Instant, UNIX_EPOCH},
};

use encoding_rs::WINDOWS_1252;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;
use zip::ZipArchive;

use crate::error::{AppError, AppResult};

pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "pdf", "epub", "mobi", "azw", "azw3", "cbz", "txt", "md", "html", "htm", "fb2",
];

/// Perfil de custo da indexacao. O perfil economico evita abrir o conteudo dos
/// livros; os demais limitam o ciclo de trabalho do unico worker de varredura.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanProfile {
    #[default]
    Economical,
    Balanced,
    Complete,
}

impl ScanProfile {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "economical" => Some(Self::Economical),
            "balanced" => Some(Self::Balanced),
            "complete" => Some(Self::Complete),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Economical => "economical",
            Self::Balanced => "balanced",
            Self::Complete => "complete",
        }
    }

    fn extracts_metadata(self) -> bool {
        !matches!(self, Self::Economical)
    }

    fn target_duty_cycle(self) -> u32 {
        match self {
            Self::Economical => 35,
            Self::Balanced => 65,
            Self::Complete => 100,
        }
    }

    fn minimum_pause(self) -> Duration {
        match self {
            Self::Economical => Duration::from_millis(12),
            Self::Balanced => Duration::from_millis(4),
            Self::Complete => Duration::ZERO,
        }
    }
}

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
    pub content_hash: Option<String>,
    pub unchanged: bool,
    pub metadata_scanned: bool,
}

#[derive(Debug, Clone)]
pub struct KnownBook {
    pub title: String,
    pub author: Option<String>,
    pub size: i64,
    pub modified_at: i64,
    pub page_count: Option<i64>,
    pub content_hash: Option<String>,
    pub metadata_scanned: bool,
}

/// Retrato serializavel do trabalho de varredura consultado pela interface.
#[derive(Debug, Clone, Serialize)]
pub struct ScanStatus {
    pub running: bool,
    pub phase: String,
    pub profile: String,
    pub total: usize,
    pub processed: usize,
    pub percent: f64,
    pub found: usize,
    pub added: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub skipped: usize,
    pub current_file: Option<String>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

impl ScanStatus {
    pub fn idle() -> Self {
        Self {
            running: false,
            phase: "idle".into(),
            profile: ScanProfile::default().as_str().into(),
            total: 0,
            processed: 0,
            percent: 0.0,
            found: 0,
            added: 0,
            updated: 0,
            unchanged: 0,
            skipped: 0,
            current_file: None,
            warnings: Vec::new(),
            error: None,
            started_at: None,
            finished_at: None,
        }
    }
}

#[derive(Debug)]
pub enum ScanEvent {
    Discovered {
        total: usize,
        skipped: usize,
    },
    Indexed {
        total: usize,
        processed: usize,
        found: usize,
        unchanged: usize,
        skipped: usize,
        current_file: String,
    },
}

/// Cataloga os formatos reconhecidos e informa cada avanco ao chamador.
pub fn scan<F>(
    root: &Path,
    profile: ScanProfile,
    known_books: &HashMap<String, KnownBook>,
    mut progress: F,
) -> AppResult<(Vec<ScannedBook>, usize, Vec<String>)>
where
    F: FnMut(ScanEvent),
{
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

    let mut candidates = Vec::new();
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
        if SUPPORTED_EXTENSIONS.contains(&extension.as_str()) {
            candidates.push((entry.into_path(), extension));
        }
    }

    let total = candidates.len();
    progress(ScanEvent::Discovered { total, skipped });
    let mut books = Vec::with_capacity(total);
    let mut unchanged = 0;
    for (index, (path, extension)) in candidates.into_iter().enumerate() {
        let work_started = Instant::now();
        match scan_file(
            &canonical_root,
            &path,
            &extension,
            profile.extracts_metadata(),
            known_books,
        ) {
            Ok(book) => {
                if book.unchanged {
                    unchanged += 1;
                }
                books.push(book);
            }
            Err(error) => {
                skipped += 1;
                warnings.push(format!("{}: {error}", path.display()));
            }
        }
        progress(ScanEvent::Indexed {
            total,
            processed: index + 1,
            found: books.len(),
            unchanged,
            skipped,
            current_file: path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("arquivo")
                .to_string(),
        });
        throttle(profile, work_started.elapsed());
    }
    warnings.truncate(20);
    Ok((books, skipped, warnings))
}

fn throttle(profile: ScanProfile, work_time: Duration) {
    let target = profile.target_duty_cycle();
    if target >= 100 {
        return;
    }
    let proportional = work_time.mul_f64(f64::from(100 - target) / f64::from(target));
    thread::sleep(proportional.max(profile.minimum_pause()));
}

fn scan_file(
    root: &Path,
    path: &Path,
    extension: &str,
    extract_metadata: bool,
    known_books: &HashMap<String, KnownBook>,
) -> AppResult<ScannedBook> {
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
    let fallback_title = filename_title(path, &filename);
    let modified_at = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_nanos()).ok())
        .unwrap_or_default();
    let size = metadata.len() as i64;
    let needs_metadata = extract_metadata && matches!(extension, "pdf" | "epub");
    if let Some(known) = known_books.get(&relative_path) {
        if known.size == size
            && known.modified_at == modified_at
            && !metadata_looks_corrupt(&known.title)
            && (!needs_metadata || known.metadata_scanned)
        {
            let content_hash = match &known.content_hash {
                Some(value) => Some(value.clone()),
                None => Some(partial_content_hash(path, metadata.len())?),
            };
            return Ok(ScannedBook {
                title: known.title.clone(),
                author: known.author.clone(),
                filename,
                relative_path,
                format: extension.to_string(),
                size,
                modified_at,
                page_count: known.page_count,
                content_hash,
                unchanged: true,
                metadata_scanned: known.metadata_scanned,
            });
        }
    }
    let (title, author, page_count) = if extract_metadata {
        match extension {
            "pdf" => pdf_metadata(path, &fallback_title),
            "epub" => epub_metadata(path, &fallback_title),
            _ => (fallback_title.clone(), None, None),
        }
    } else {
        (fallback_title.clone(), None, None)
    };
    let title = sanitize_metadata_text(&title).unwrap_or(fallback_title);
    let author = author.and_then(|value| sanitize_metadata_text(&value));
    let content_hash = Some(partial_content_hash(path, metadata.len())?);
    Ok(ScannedBook {
        title,
        author,
        filename,
        relative_path,
        format: extension.to_string(),
        size,
        modified_at,
        page_count,
        content_hash,
        unchanged: false,
        metadata_scanned: needs_metadata,
    })
}

/// SHA-256 do tamanho + primeiros e ultimos 64 KiB. Evita ler o arquivo todo
/// apenas para localizar candidatos a duplicata.
fn partial_content_hash(path: &Path, size: u64) -> AppResult<String> {
    const SAMPLE_SIZE: usize = 64 * 1024;
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    hasher.update(size.to_le_bytes());
    let mut buffer = vec![0_u8; SAMPLE_SIZE.min(size as usize)];
    if !buffer.is_empty() {
        file.read_exact(&mut buffer)?;
        hasher.update(&buffer);
    }
    if size > SAMPLE_SIZE as u64 {
        let tail_size = SAMPLE_SIZE.min(size as usize);
        file.seek(SeekFrom::End(-(tail_size as i64)))?;
        buffer.resize(tail_size, 0);
        file.read_exact(&mut buffer)?;
        hasher.update(&buffer);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn filename_title(path: &Path, filename: &str) -> String {
    let value = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(filename)
        .replace(['_', '-'], " ");
    let cleaned = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() {
        filename.to_string()
    } else {
        cleaned
    }
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
    let title = pdf_string(info.get(b"Title").ok()).unwrap_or_else(|| fallback.to_string());
    let author = pdf_string(info.get(b"Author").ok());
    (title, author, pages)
}

fn pdf_string(object: Option<&lopdf::Object>) -> Option<String> {
    let bytes = object?.as_str().ok()?;
    let decoded = decode_pdf_bytes(bytes)?;
    sanitize_metadata_text(&decoded)
}

fn decode_pdf_bytes(bytes: &[u8]) -> Option<String> {
    if bytes.starts_with(&[0xfe, 0xff]) {
        return decode_utf16(&bytes[2..], true);
    }
    if bytes.starts_with(&[0xff, 0xfe]) {
        return decode_utf16(&bytes[2..], false);
    }
    if bytes.len() >= 4 && bytes.len().is_multiple_of(2) {
        let zero_even = bytes.iter().step_by(2).filter(|byte| **byte == 0).count();
        let zero_odd = bytes
            .iter()
            .skip(1)
            .step_by(2)
            .filter(|byte| **byte == 0)
            .count();
        if zero_even > bytes.len() / 6 {
            return decode_utf16(bytes, true);
        }
        if zero_odd > bytes.len() / 6 {
            return decode_utf16(bytes, false);
        }
    }
    if let Ok(value) = std::str::from_utf8(bytes) {
        return Some(value.to_string());
    }
    let (value, _, _) = WINDOWS_1252.decode(bytes);
    Some(value.into_owned())
}

fn decode_utf16(bytes: &[u8], big_endian: bool) -> Option<String> {
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let units = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|chunk| {
            if big_endian {
                u16::from_be_bytes([chunk[0], chunk[1]])
            } else {
                u16::from_le_bytes([chunk[0], chunk[1]])
            }
        })
        .collect::<Vec<_>>();
    String::from_utf16(&units).ok()
}

/// Remove caracteres de controle e rejeita metadados binarios/mojibake.
pub fn sanitize_metadata_text(value: &str) -> Option<String> {
    let cleaned = value
        .trim_matches(['\0', '\u{feff}'])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if cleaned.is_empty() || cleaned.chars().count() > 500 {
        return None;
    }
    let mut useful = 0_usize;
    let mut suspicious = 0_usize;
    let mut total = 0_usize;
    for character in cleaned.chars() {
        total += 1;
        if character == '\u{fffd}' || (character.is_control() && !character.is_whitespace()) {
            return None;
        }
        if character.is_alphanumeric() {
            useful += 1;
        } else if !(character.is_whitespace()
            || ".,:;!?()[]{}'\"/\\&@#+–—-_|ºª°%".contains(character))
        {
            suspicious += 1;
        }
    }
    if useful == 0 || suspicious * 4 > total.max(1) {
        None
    } else {
        Some(cleaned)
    }
}

/// Detecta titulos antigos corrompidos para que uma nova varredura possa repara-los.
pub fn metadata_looks_corrupt(value: &str) -> bool {
    sanitize_metadata_text(value).is_none()
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
            .and_then(sanitize_metadata_text)
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
    use std::time::SystemTime;

    fn fixture_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("relogio do sistema")
            .as_nanos();
        std::env::current_dir()
            .expect("diretorio do projeto")
            .join("tests")
            .join(format!("runtime-data-scanner-{label}-{nonce}"))
    }

    #[test]
    fn rejects_parent_traversal() {
        assert!(path_to_safe_relative(Path::new("../segredo.pdf")).is_err());
        assert!(path_to_safe_relative(Path::new("livros/ok.pdf")).is_ok());
    }

    #[test]
    fn decodes_utf16_pdf_metadata() {
        let bytes = [0xfe, 0xff, 0x00, 0x4f, 0x00, 0x20, 0x6d, 0x77];
        assert_eq!(decode_pdf_bytes(&bytes).as_deref(), Some("O 海"));
    }

    #[test]
    fn rejects_replacement_character_and_binary_titles() {
        assert!(sanitize_metadata_text("��_����Z�����>ϳ�6�").is_none());
        assert!(sanitize_metadata_text("\u{1}\u{2}\u{3}").is_none());
        assert_eq!(
            sanitize_metadata_text("  Grande   Sertão: Veredas  ").as_deref(),
            Some("Grande Sertão: Veredas")
        );
    }

    #[test]
    fn reuses_unchanged_book_without_reopening_metadata() {
        let root = fixture_dir("incremental");
        std::fs::create_dir_all(&root).expect("cria fixture");
        std::fs::write(root.join("Livro de Teste.txt"), "conteudo local").expect("grava livro");
        let (first, _, _) = scan(&root, ScanProfile::Complete, &HashMap::new(), |_| {})
            .expect("primeira varredura");
        let original = first.first().expect("livro indexado");
        let known = HashMap::from([(
            original.relative_path.clone(),
            KnownBook {
                title: "Titulo corrigido manualmente".into(),
                author: Some("FACRF".into()),
                size: original.size,
                modified_at: original.modified_at,
                page_count: original.page_count,
                content_hash: original.content_hash.clone(),
                metadata_scanned: original.metadata_scanned,
            },
        )]);
        let (second, _, _) =
            scan(&root, ScanProfile::Complete, &known, |_| {}).expect("segunda varredura");
        assert!(second[0].unchanged);
        assert_eq!(second[0].title, "Titulo corrigido manualmente");
        assert_eq!(second[0].content_hash, original.content_hash);
        std::fs::remove_dir_all(root).expect("remove fixture");
    }

    /// Reproduz o volume informado (2.287 livros) sem fazer parte do ciclo curto
    /// do Docker. Execute com `cargo test indexes_2287_files -- --ignored`.
    #[test]
    #[ignore = "teste de volume explicito"]
    fn indexes_2287_files_with_monotonic_progress() {
        let root = fixture_dir("volume-2287");
        std::fs::create_dir_all(&root).expect("cria fixture");
        for index in 0..2_287 {
            std::fs::write(root.join(format!("livro-{index:04}.txt")), b"fixture")
                .expect("grava fixture");
        }
        let mut observations = Vec::new();
        let (books, skipped, warnings) =
            scan(&root, ScanProfile::Complete, &HashMap::new(), |event| {
                if let ScanEvent::Indexed {
                    total, processed, ..
                } = event
                {
                    observations.push((processed, total));
                }
            })
            .expect("varredura de volume");
        assert_eq!(books.len(), 2_287);
        assert_eq!(skipped, 0);
        assert!(warnings.is_empty());
        assert_eq!(observations.last(), Some(&(2_287, 2_287)));
        assert!(observations.windows(2).all(|pair| pair[0].0 < pair[1].0));
        std::fs::remove_dir_all(root).expect("remove fixture");
    }
}
