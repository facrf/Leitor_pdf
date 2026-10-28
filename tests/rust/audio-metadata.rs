use std::{fs::File, io::Write, path::PathBuf};

use super::*;

fn fixture_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(format!("tests/runtime-data-audio-{}-{}", name, rand::random::<u64>()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn parses_mp3_id3v2_metadata_and_cover() {
    let dir = fixture_dir("id3v2");
    let path = dir.join("audiobook.mp3");
    let mut file = File::create(&path).unwrap();

    // Cria tag ID3v2.3 sintetica:
    // Header: "ID3", versao 3, revisao 0, flags 0, tamanho synchsafe
    let mut tag_body = Vec::new();

    // Frame TIT2:
    let title = "O Hobbit";
    let mut tit2_data = vec![0u8]; // Latin1
    tit2_data.extend_from_slice(title.as_bytes());
    tag_body.extend_from_slice(b"TIT2");
    tag_body.extend_from_slice(&(tit2_data.len() as u32).to_be_bytes());
    tag_body.extend_from_slice(&[0, 0]); // flags
    tag_body.extend_from_slice(&tit2_data);

    // Frame TPE1:
    let author = "J.R.R. Tolkien";
    let mut tpe1_data = vec![0u8]; // Latin1
    tpe1_data.extend_from_slice(author.as_bytes());
    tag_body.extend_from_slice(b"TPE1");
    tag_body.extend_from_slice(&(tpe1_data.len() as u32).to_be_bytes());
    tag_body.extend_from_slice(&[0, 0]); // flags
    tag_body.extend_from_slice(&tpe1_data);

    // Frame APIC (Capa JPEG):
    let mut apic_data = vec![0u8]; // encoding 0
    apic_data.extend_from_slice(b"image/jpeg\0"); // MIME
    apic_data.push(3); // Front cover
    apic_data.extend_from_slice(b"Cover\0"); // Descricao
    let fake_jpeg = [0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, b'J', b'F', b'I', b'F'];
    apic_data.extend_from_slice(&fake_jpeg);

    tag_body.extend_from_slice(b"APIC");
    tag_body.extend_from_slice(&(apic_data.len() as u32).to_be_bytes());
    tag_body.extend_from_slice(&[0, 0]); // flags
    tag_body.extend_from_slice(&apic_data);

    let tag_size = tag_body.len();
    let synchsafe_size = [
        ((tag_size >> 21) & 0x7F) as u8,
        ((tag_size >> 14) & 0x7F) as u8,
        ((tag_size >> 7) & 0x7F) as u8,
        (tag_size & 0x7F) as u8,
    ];

    file.write_all(b"ID3").unwrap();
    file.write_all(&[3, 0, 0]).unwrap(); // v2.3, rev 0, flags 0
    file.write_all(&synchsafe_size).unwrap();
    file.write_all(&tag_body).unwrap();
    file.write_all(b"fake audio data").unwrap();
    file.flush().unwrap();
    drop(file);

    let (t, a, _) = parse_audio_metadata(&path, "Fallback");
    assert_eq!(t, "O Hobbit");
    assert_eq!(a.as_deref(), Some("J.R.R. Tolkien"));

    let cover = extract_embedded_cover(&path);
    assert!(cover.is_some());
    let (data, ext) = cover.unwrap();
    assert_eq!(ext, "jpg");
    assert_eq!(data, fake_jpeg);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn parses_mp3_id3v1_fallback() {
    let dir = fixture_dir("id3v1");
    let path = dir.join("audiobook_v1.mp3");
    let mut file = File::create(&path).unwrap();
    let body = vec![0u8; 1000]; // fake mp3 audio
    file.write_all(&body).unwrap();

    // ID3v1 tag (128 bytes): "TAG" + title(30) + artist(30) + album(30) + year(4) + comment(30) + genre(1)
    let mut tag = vec![0u8; 128];
    tag[0..3].copy_from_slice(b"TAG");
    let title = b"Livro Antigo";
    tag[3..3 + title.len()].copy_from_slice(title);
    let artist = b"Narrador Classico";
    tag[33..33 + artist.len()].copy_from_slice(artist);

    file.write_all(&tag).unwrap();
    file.flush().unwrap();
    drop(file);

    let (t, a, _) = parse_audio_metadata(&path, "Fallback");
    assert_eq!(t, "Livro Antigo");
    assert_eq!(a.as_deref(), Some("Narrador Classico"));

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn parses_m4b_atoms_metadata_and_cover() {
    let dir = fixture_dir("m4b");
    let path = dir.join("course.m4b");
    let mut file = File::create(&path).unwrap();

    // Monta atomo moov -> udta -> meta -> ilst
    let mut ilst_body = Vec::new();

    // ©nam (Title)
    let mut nam_body = Vec::new();
    let title = "Curso de Rust";
    let mut data_body = vec![0, 0, 0, 1, 0, 0, 0, 0]; // type 1 (text), locale 0
    data_body.extend_from_slice(title.as_bytes());
    nam_body.extend_from_slice(&((data_body.len() + 8) as u32).to_be_bytes());
    nam_body.extend_from_slice(b"data");
    nam_body.extend_from_slice(&data_body);

    ilst_body.extend_from_slice(&((nam_body.len() + 8) as u32).to_be_bytes());
    ilst_body.extend_from_slice(b"\xa9nam");
    ilst_body.extend_from_slice(&nam_body);

    // ©ART (Artist)
    let mut art_body = Vec::new();
    let author = "Instrutor Rustacean";
    let mut data_body = vec![0, 0, 0, 1, 0, 0, 0, 0];
    data_body.extend_from_slice(author.as_bytes());
    art_body.extend_from_slice(&((data_body.len() + 8) as u32).to_be_bytes());
    art_body.extend_from_slice(b"data");
    art_body.extend_from_slice(&data_body);

    ilst_body.extend_from_slice(&((art_body.len() + 8) as u32).to_be_bytes());
    ilst_body.extend_from_slice(b"\xa9ART");
    ilst_body.extend_from_slice(&art_body);

    // covr (Cover PNG)
    let mut covr_body = Vec::new();
    let fake_png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut data_body = vec![0, 0, 0, 14, 0, 0, 0, 0]; // type 14 (png)
    data_body.extend_from_slice(&fake_png);
    covr_body.extend_from_slice(&((data_body.len() + 8) as u32).to_be_bytes());
    covr_body.extend_from_slice(b"data");
    covr_body.extend_from_slice(&data_body);

    ilst_body.extend_from_slice(&((covr_body.len() + 8) as u32).to_be_bytes());
    ilst_body.extend_from_slice(b"covr");
    ilst_body.extend_from_slice(&covr_body);

    // meta: [size][meta][4 bytes version/flags][ilst...]
    let mut meta_body = vec![0, 0, 0, 0]; // version/flags
    meta_body.extend_from_slice(&((ilst_body.len() + 8) as u32).to_be_bytes());
    meta_body.extend_from_slice(b"ilst");
    meta_body.extend_from_slice(&ilst_body);

    // udta: [size][udta][meta...]
    let mut udta_body = Vec::new();
    udta_body.extend_from_slice(&((meta_body.len() + 8) as u32).to_be_bytes());
    udta_body.extend_from_slice(b"meta");
    udta_body.extend_from_slice(&meta_body);

    // moov: [size][moov][udta...]
    let mut moov_body = Vec::new();
    moov_body.extend_from_slice(&((udta_body.len() + 8) as u32).to_be_bytes());
    moov_body.extend_from_slice(b"udta");
    moov_body.extend_from_slice(&udta_body);

    // Escreve ftyp e moov
    file.write_all(&[0, 0, 0, 16]).unwrap();
    file.write_all(b"ftypM4A \0\0\0\0").unwrap();

    file.write_all(&((moov_body.len() + 8) as u32).to_be_bytes()).unwrap();
    file.write_all(b"moov").unwrap();
    file.write_all(&moov_body).unwrap();
    file.flush().unwrap();
    drop(file);

    let (t, a, _) = parse_audio_metadata(&path, "Fallback");
    assert_eq!(t, "Curso de Rust");
    assert_eq!(a.as_deref(), Some("Instrutor Rustacean"));

    let cover = extract_embedded_cover(&path);
    assert!(cover.is_some());
    let (data, ext) = cover.unwrap();
    assert_eq!(ext, "png");
    assert_eq!(data, fake_png);

    let _ = std::fs::remove_dir_all(dir);
}
