use super::*;

#[test]
fn converts_srt_to_valid_vtt() {
    let srt = "1\n00:00:01,500 --> 00:00:04,200\nOlá, bem-vindo!\n\n2\n00:00:05,000 --> 00:00:09,800\nCurso de Rust.\n";
    let vtt = srt_to_vtt(srt);
    assert!(vtt.starts_with("WEBVTT\n\n"));
    assert!(vtt.contains("00:00:01.500 --> 00:00:04.200"));
    assert!(vtt.contains("00:00:05.000 --> 00:00:09.800"));
    assert!(vtt.contains("Olá, bem-vindo!"));
}

#[test]
fn parses_language_from_filenames() {
    let (lang_pt, label_pt) = parse_subtitle_lang_and_label("01_aula.pt.srt");
    assert_eq!(lang_pt, "pt");
    assert_eq!(label_pt, "Português");

    let (lang_en, label_en) = parse_subtitle_lang_and_label("01_aula.en.vtt");
    assert_eq!(lang_en, "en");
    assert_eq!(label_en, "English");

    let (lang_es, label_es) = parse_subtitle_lang_and_label("01_aula.espanhol.srt");
    assert_eq!(lang_es, "es");
    assert_eq!(label_es, "Español");

    let (lang_default, label_default) = parse_subtitle_lang_and_label("01_aula.vtt");
    assert_eq!(lang_default, "pt");
    assert_eq!(label_default, "Legenda");
}

#[test]
fn detects_and_loads_subtitles_safely() {
    let temp_dir = std::path::PathBuf::from(format!(
        "tests/runtime-data-subtitles-{}",
        rand::random::<u64>()
    ));
    let course_dir = temp_dir.join("Curso");
    std::fs::create_dir_all(&course_dir).unwrap();

    let video_file = course_dir.join("aula1.mp4");
    std::fs::write(&video_file, b"dummy video content").unwrap();

    let srt_file = course_dir.join("aula1.pt.srt");
    std::fs::write(&srt_file, "1\n00:00:01,000 --> 00:00:03,000\nLegenda em Portugues\n").unwrap();

    let vtt_file = course_dir.join("aula1.en.vtt");
    std::fs::write(&vtt_file, "WEBVTT\n\n00:00:01.000 --> 00:00:03.000\nEnglish Subtitle\n").unwrap();

    let tracks = detect_subtitles(&temp_dir, "Curso/aula1.mp4").unwrap();
    assert_eq!(tracks.len(), 2);
    assert!(tracks.iter().any(|t| t.srclang == "pt" && t.is_default));
    assert!(tracks.iter().any(|t| t.srclang == "en" && !t.is_default));

    let loaded_vtt = load_subtitle_vtt(&temp_dir, "Curso/aula1.mp4", "aula1.pt.srt").unwrap();
    assert!(loaded_vtt.starts_with("WEBVTT\n\n"));
    assert!(loaded_vtt.contains("00:00:01.000 --> 00:00:03.000"));

    // Traversal check
    let traversal = load_subtitle_vtt(&temp_dir, "Curso/aula1.mp4", "../outside.srt");
    assert!(traversal.is_err());

    let _ = std::fs::remove_dir_all(&temp_dir);
}
