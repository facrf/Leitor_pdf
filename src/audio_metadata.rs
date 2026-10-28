use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use encoding_rs::WINDOWS_1252;

use crate::scanner::sanitize_metadata_text;

/// Extrai título, autor e duração (se disponível) de arquivos de áudio (MP3, M4B, M4A).
pub fn parse_audio_metadata(path: &Path, fallback_title: &str) -> (String, Option<String>, Option<i64>) {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    match extension.as_str() {
        "mp3" => parse_mp3_metadata(path, fallback_title),
        "m4a" | "m4b" | "mp4" => parse_mp4_metadata(path, fallback_title),
        _ => (fallback_title.to_string(), None, None),
    }
}

/// Extrai capa embutida de arquivo de áudio (APIC em ID3v2 ou covr em MP4/M4B).
pub fn extract_embedded_cover(path: &Path) -> Option<(Vec<u8>, &'static str)> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    match extension.as_str() {
        "mp3" => extract_id3_cover(path),
        "m4a" | "m4b" | "mp4" => extract_mp4_cover(path),
        _ => None,
    }
}

fn parse_mp3_metadata(path: &Path, fallback: &str) -> (String, Option<String>, Option<i64>) {
    let Ok(mut file) = File::open(path) else {
        return (fallback.to_string(), None, None);
    };

    let mut header = [0u8; 10];
    if file.read_exact(&mut header).is_ok() && &header[0..3] == b"ID3" {
        let version = header[3];
        let tag_size = synchsafe_u32(&header[6..10]) as usize;
        let read_size = tag_size.min(1024 * 1024);
        let mut buffer = vec![0u8; read_size];
        if file.read_exact(&mut buffer).is_ok() {
            let (title, author) = parse_id3_frames(&buffer, version);
            let final_title = title.and_then(|t| sanitize_metadata_text(&t)).unwrap_or_else(|| fallback.to_string());
            let final_author = author.and_then(|a| sanitize_metadata_text(&a));
            return (final_title, final_author, None);
        }
    }

    // Fallback: ID3v1 nos ultimos 128 bytes
    if let Ok(metadata) = file.metadata() {
        if metadata.len() >= 128 && file.seek(SeekFrom::End(-128)).is_ok() {
            let mut buf = [0u8; 128];
            if file.read_exact(&mut buf).is_ok() && &buf[0..3] == b"TAG" {
                let title_raw = latin1_to_string(&buf[3..33]);
                let artist_raw = latin1_to_string(&buf[33..63]);
                let final_title = sanitize_metadata_text(&title_raw).unwrap_or_else(|| fallback.to_string());
                let final_author = sanitize_metadata_text(&artist_raw);
                return (final_title, final_author, None);
            }
        }
    }

    (fallback.to_string(), None, None)
}

fn synchsafe_u32(bytes: &[u8]) -> u32 {
    ((bytes[0] as u32 & 0x7F) << 21)
        | ((bytes[1] as u32 & 0x7F) << 14)
        | ((bytes[2] as u32 & 0x7F) << 7)
        | (bytes[3] as u32 & 0x7F)
}

fn parse_id3_frames(buffer: &[u8], version: u8) -> (Option<String>, Option<String>) {
    let mut offset = 0;
    let mut title = None;
    let mut author = None;

    while offset + 10 <= buffer.len() {
        let frame_id = &buffer[offset..offset + 4];
        if frame_id == [0, 0, 0, 0] {
            break;
        }

        let frame_size = if version >= 4 {
            synchsafe_u32(&buffer[offset + 4..offset + 8]) as usize
        } else {
            u32::from_be_bytes([
                buffer[offset + 4],
                buffer[offset + 5],
                buffer[offset + 6],
                buffer[offset + 7],
            ]) as usize
        };

        if frame_size == 0 || offset + 10 + frame_size > buffer.len() {
            break;
        }

        let frame_data = &buffer[offset + 10..offset + 10 + frame_size];
        offset += 10 + frame_size;

        if frame_id == b"TIT2" && title.is_none() {
            title = decode_id3_text(frame_data);
        } else if (frame_id == b"TPE1" || (frame_id == b"TPE2" && author.is_none())) && author.is_none() {
            author = decode_id3_text(frame_data);
        }
    }

    (title, author)
}

fn decode_id3_text(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    let encoding = bytes[0];
    let payload = &bytes[1..];
    match encoding {
        0 => {
            let (text, _, _) = WINDOWS_1252.decode(payload);
            Some(text.trim_matches('\0').trim().to_string())
        }
        1 => decode_utf16_with_bom(payload),
        2 => decode_utf16_be(payload),
        3 => {
            let text = String::from_utf8_lossy(payload);
            Some(text.trim_matches('\0').trim().to_string())
        }
        _ => None,
    }
}

fn decode_utf16_with_bom(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 2 {
        return None;
    }
    let is_be = if bytes.starts_with(&[0xfe, 0xff]) {
        true
    } else if bytes.starts_with(&[0xff, 0xfe]) {
        false
    } else {
        return None;
    };
    let payload = &bytes[2..];
    if !payload.len().is_multiple_of(2) {
        return None;
    }
    let units: Vec<u16> = payload
        .as_chunks::<2>()
        .0
        .iter()
        .map(|chunk| {
            if is_be {
                u16::from_be_bytes([chunk[0], chunk[1]])
            } else {
                u16::from_le_bytes([chunk[0], chunk[1]])
            }
        })
        .collect();
    String::from_utf16(&units)
        .ok()
        .map(|s| s.trim_matches('\0').trim().to_string())
}

fn decode_utf16_be(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 2 || !bytes.len().is_multiple_of(2) {
        return None;
    }
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
        .collect();
    String::from_utf16(&units)
        .ok()
        .map(|s| s.trim_matches('\0').trim().to_string())
}

fn latin1_to_string(bytes: &[u8]) -> String {
    let (text, _, _) = WINDOWS_1252.decode(bytes);
    text.trim_matches('\0').trim().to_string()
}

fn extract_id3_cover(path: &Path) -> Option<(Vec<u8>, &'static str)> {
    let mut file = File::open(path).ok()?;
    let mut header = [0u8; 10];
    file.read_exact(&mut header).ok()?;
    if &header[0..3] != b"ID3" {
        return None;
    }
    let version = header[3];
    let tag_size = synchsafe_u32(&header[6..10]) as usize;
    let read_size = tag_size.min(8 * 1024 * 1024); // Ate 8 MiB para capas embutidas
    let mut buffer = vec![0u8; read_size];
    file.read_exact(&mut buffer).ok()?;

    let mut offset = 0;
    while offset + 10 <= buffer.len() {
        let frame_id = &buffer[offset..offset + 4];
        if frame_id == [0, 0, 0, 0] {
            break;
        }
        let frame_size = if version >= 4 {
            synchsafe_u32(&buffer[offset + 4..offset + 8]) as usize
        } else {
            u32::from_be_bytes([
                buffer[offset + 4],
                buffer[offset + 5],
                buffer[offset + 6],
                buffer[offset + 7],
            ]) as usize
        };
        if frame_size == 0 || offset + 10 + frame_size > buffer.len() {
            break;
        }
        let frame_data = &buffer[offset + 10..offset + 10 + frame_size];
        offset += 10 + frame_size;

        if frame_id == b"APIC" && frame_data.len() > 10 {
            let encoding = frame_data[0];
            let mut idx = 1;
            // Pula MIME type (string terminada em \0)
            while idx < frame_data.len() && frame_data[idx] != 0 {
                idx += 1;
            }
            idx += 1; // Pula o \0 do MIME
            if idx >= frame_data.len() {
                continue;
            }
            idx += 1; // Pula picture type (1 byte)
            // Pula descricao (terminada em \0 em Latin1/UTF-8 ou \0\0 em UTF-16)
            if encoding == 1 || encoding == 2 {
                while idx + 1 < frame_data.len() && (frame_data[idx] != 0 || frame_data[idx + 1] != 0) {
                    idx += 2;
                }
                idx += 2;
            } else {
                while idx < frame_data.len() && frame_data[idx] != 0 {
                    idx += 1;
                }
                idx += 1;
            }
            if idx < frame_data.len() {
                let img_bytes = &frame_data[idx..];
                if let Some(ext) = detect_image_format(img_bytes) {
                    return Some((img_bytes.to_vec(), ext));
                }
            }
        }
    }
    None
}

fn detect_image_format(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("png")
    } else if bytes.starts_with(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

// ---------------------------------------------------------
// Parser de caixas / atomos MP4 (M4A / M4B)
// ---------------------------------------------------------

fn parse_mp4_metadata(path: &Path, fallback: &str) -> (String, Option<String>, Option<i64>) {
    let Ok(mut file) = File::open(path) else {
        return (fallback.to_string(), None, None);
    };

    let mut title = None;
    let mut author = None;

    if let Some(ilst_data) = find_atom_payload(&mut file, &[b"moov", b"udta", b"meta", b"ilst"], 4 * 1024 * 1024) {
        let mut offset = 0;
        while offset + 8 <= ilst_data.len() {
            let atom_size = u32::from_be_bytes([
                ilst_data[offset],
                ilst_data[offset + 1],
                ilst_data[offset + 2],
                ilst_data[offset + 3],
            ]) as usize;
            if atom_size < 8 || offset + atom_size > ilst_data.len() {
                break;
            }
            let atom_type = &ilst_data[offset + 4..offset + 8];
            let atom_body = &ilst_data[offset + 8..offset + atom_size];
            offset += atom_size;

            if atom_type == b"\xa9nam" && title.is_none() {
                title = extract_mp4_text_data(atom_body);
            } else if (atom_type == b"\xa9ART" || (atom_type == b"aART" && author.is_none())) && author.is_none() {
                author = extract_mp4_text_data(atom_body);
            }
        }
    }

    let final_title = title.and_then(|t| sanitize_metadata_text(&t)).unwrap_or_else(|| fallback.to_string());
    let final_author = author.and_then(|a| sanitize_metadata_text(&a));
    (final_title, final_author, None)
}

fn extract_mp4_text_data(atom_body: &[u8]) -> Option<String> {
    if let Some(data_payload) = find_child_atom_payload(atom_body, b"data") {
        if data_payload.len() > 8 {
            let text_bytes = &data_payload[8..];
            return String::from_utf8(text_bytes.to_vec()).ok().map(|s| s.trim().to_string());
        }
    }
    None
}

fn extract_mp4_cover(path: &Path) -> Option<(Vec<u8>, &'static str)> {
    let mut file = File::open(path).ok()?;
    let ilst_data = find_atom_payload(&mut file, &[b"moov", b"udta", b"meta", b"ilst"], 8 * 1024 * 1024)?;
    let mut offset = 0;
    while offset + 8 <= ilst_data.len() {
        let atom_size = u32::from_be_bytes([
            ilst_data[offset],
            ilst_data[offset + 1],
            ilst_data[offset + 2],
            ilst_data[offset + 3],
        ]) as usize;
        if atom_size < 8 || offset + atom_size > ilst_data.len() {
            break;
        }
        let atom_type = &ilst_data[offset + 4..offset + 8];
        let atom_body = &ilst_data[offset + 8..offset + atom_size];
        offset += atom_size;

        if atom_type == b"covr" {
            if let Some(data_payload) = find_child_atom_payload(atom_body, b"data") {
                if data_payload.len() > 8 {
                    let img_bytes = &data_payload[8..];
                    if let Some(ext) = detect_image_format(img_bytes) {
                        return Some((img_bytes.to_vec(), ext));
                    }
                }
            }
        }
    }
    None
}

fn find_child_atom_payload<'a>(container: &'a [u8], target_type: &[u8; 4]) -> Option<&'a [u8]> {
    let mut offset = 0;
    while offset + 8 <= container.len() {
        let size = u32::from_be_bytes([
            container[offset],
            container[offset + 1],
            container[offset + 2],
            container[offset + 3],
        ]) as usize;
        if size < 8 || offset + size > container.len() {
            break;
        }
        let atom_type = &container[offset + 4..offset + 8];
        if atom_type == target_type {
            return Some(&container[offset + 8..offset + size]);
        }
        offset += size;
    }
    None
}

fn find_atom_payload(file: &mut File, path: &[&[u8; 4]], max_buffer_bytes: usize) -> Option<Vec<u8>> {
    if path.is_empty() {
        return None;
    }
    file.seek(SeekFrom::Start(0)).ok()?;
    let mut current_pos = 0_u64;
    let file_len = file.metadata().ok()?.len();

    let target_top = path[0];
    while current_pos + 8 <= file_len {
        file.seek(SeekFrom::Start(current_pos)).ok()?;
        let mut header = [0u8; 8];
        file.read_exact(&mut header).ok()?;
        let size = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as u64;
        let atom_type = &header[4..8];

        let actual_size = if size == 1 {
            let mut ext = [0u8; 8];
            file.read_exact(&mut ext).ok()?;
            u64::from_be_bytes(ext)
        } else if size == 0 {
            file_len - current_pos
        } else {
            size
        };

        if actual_size < 8 {
            break;
        }

        if atom_type == target_top {
            let read_len = (actual_size - 8).min(max_buffer_bytes as u64) as usize;
            let mut buffer = vec![0u8; read_len];
            file.read_exact(&mut buffer).ok()?;

            let mut current_slice = &buffer[..];
            for &segment in &path[1..] {
                // Caso especial da caixa 'meta': os primeiros 4 bytes sao version/flags
                if segment == b"ilst" && current_slice.len() >= 4 {
                    current_slice = &current_slice[4..];
                }
                current_slice = find_child_atom_payload(current_slice, segment)?;
            }
            return Some(current_slice.to_vec());
        }

        current_pos += actual_size;
    }
    None
}

#[cfg(test)]
#[path = "../tests/rust/audio-metadata.rs"]
mod regression_tests;
