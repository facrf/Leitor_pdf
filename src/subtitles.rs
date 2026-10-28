use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleTrack {
    pub filename: String,
    pub label: String,
    pub srclang: String,
    pub is_default: bool,
}

/// Detecta legendas (.vtt e .srt) no mesmo diretorio do video.
pub fn detect_subtitles(library_root: &Path, relative_path: &str) -> AppResult<Vec<SubtitleTrack>> {
    let video_path = crate::scanner::resolve_book_path(library_root, relative_path)?;
    let parent_dir = match video_path.parent() {
        Some(p) if p.is_dir() => p,
        _ => return Ok(Vec::new()),
    };

    let video_stem = video_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();

    if video_stem.is_empty() {
        return Ok(Vec::new());
    }

    let mut tracks = Vec::new();
    let entries = match std::fs::read_dir(parent_dir) {
        Ok(e) => e,
        Err(_) => return Ok(Vec::new()),
    };

    let mut filenames = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let lower = filename.to_ascii_lowercase();
        if !lower.ends_with(".vtt") && !lower.ends_with(".srt") {
            continue;
        }

        let sub_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        if !sub_stem.starts_with(video_stem) && !video_stem.starts_with(sub_stem) {
            continue;
        }
        filenames.push(filename);
    }

    // Natural sort by filename
    filenames.sort_by_key(|a| crate::readers::natural_key(a));

    for filename in filenames {
        let (srclang, label) = parse_subtitle_lang_and_label(&filename);
        tracks.push(SubtitleTrack {
            filename,
            label,
            srclang,
            is_default: false,
        });
    }

    // Marca a primeira legenda de portugues como default, ou a primeira da lista se nao houver pt
    if let Some(pos) = tracks.iter().position(|t| t.srclang == "pt") {
        tracks[pos].is_default = true;
    } else if let Some(first) = tracks.first_mut() {
        first.is_default = true;
    }

    Ok(tracks)
}

pub fn parse_subtitle_lang_and_label(filename: &str) -> (String, String) {
    let lower = filename.to_ascii_lowercase();
    if lower.contains(".pt")
        || lower.contains(".por")
        || lower.contains("portugues")
        || lower.contains("portuguese")
    {
        ("pt".into(), "Português".into())
    } else if lower.contains(".en")
        || lower.contains(".eng")
        || lower.contains("ingles")
        || lower.contains("english")
    {
        ("en".into(), "English".into())
    } else if lower.contains(".es")
        || lower.contains(".spa")
        || lower.contains("espanhol")
        || lower.contains("spanish")
    {
        ("es".into(), "Español".into())
    } else if lower.contains(".fr")
        || lower.contains(".fra")
        || lower.contains("frances")
        || lower.contains("french")
    {
        ("fr".into(), "Français".into())
    } else if lower.contains(".de")
        || lower.contains(".ger")
        || lower.contains(".deu")
        || lower.contains("alemao")
    {
        ("de".into(), "Deutsch".into())
    } else {
        ("pt".into(), "Legenda".into())
    }
}

/// Le arquivo de legenda (.vtt ou .srt) e retorna string WebVTT valida.
pub fn load_subtitle_vtt(
    library_root: &Path,
    relative_path: &str,
    subtitle_filename: &str,
) -> AppResult<String> {
    if subtitle_filename.contains('/')
        || subtitle_filename.contains('\\')
        || subtitle_filename.contains("..")
    {
        return Err(AppError::BadRequest("nome de legenda invalido".into()));
    }

    let video_path = crate::scanner::resolve_book_path(library_root, relative_path)?;
    let parent = video_path.parent().ok_or(AppError::NotFound)?;
    let sub_path = parent.join(subtitle_filename);

    if !sub_path.is_file() {
        return Err(AppError::NotFound);
    }

    let meta = std::fs::metadata(&sub_path)?;
    if meta.len() > 5 * 1024 * 1024 {
        return Err(AppError::BadRequest(
            "arquivo de legenda grande demais".into(),
        ));
    }

    let raw = std::fs::read(&sub_path)?;
    let text = String::from_utf8_lossy(&raw);
    let trimmed = text.trim_start_matches('\u{feff}');

    if subtitle_filename.to_ascii_lowercase().ends_with(".srt") {
        Ok(srt_to_vtt(trimmed))
    } else {
        Ok(trimmed.to_string())
    }
}

/// Converte formato SubRip (SRT) para WebVTT compativel com navegadores.
pub fn srt_to_vtt(srt: &str) -> String {
    let mut vtt = String::with_capacity(srt.len() + 32);
    vtt.push_str("WEBVTT\n\n");
    for line in srt.lines() {
        if line.contains("-->") {
            vtt.push_str(&line.replace(',', "."));
        } else {
            vtt.push_str(line);
        }
        vtt.push('\n');
    }
    vtt
}

#[cfg(test)]
#[path = "../tests/rust/subtitles.rs"]
mod regression_tests;
