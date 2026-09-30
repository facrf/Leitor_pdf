use super::*;
use std::io::Write;

#[test]
fn encoded_epub_hrefs_resolve_real_zip_entries() {
    let root = PathBuf::from(format!("tests/runtime-data-epub-{}", rand::random::<u64>()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("book.epub");
    let mut archive = zip::ZipWriter::new(File::create(&path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    for (name, content) in [
        ("META-INF/container.xml", "<container xmlns='urn:oasis:names:tc:opendocument:xmlns:container'><rootfile full-path='OPS/package.opf'/></container>"),
        ("OPS/package.opf", "<package xmlns='http://www.idpf.org/2007/opf'><manifest><item id='c' href='cap%C3%ADtulo%20um%23.xhtml#anchor'/></manifest><spine><itemref idref='c'/></spine></package>"),
        ("OPS/capítulo um#.xhtml", "<html><body>Capitulo de teste</body></html>")
    ] { archive.start_file(name, options).unwrap(); archive.write_all(content.as_bytes()).unwrap(); }
    archive.finish().unwrap();
    let manifest = epub_manifest(&path).unwrap();
    assert_eq!(manifest.chapters[0].href, "OPS/capítulo um#.xhtml");
    let (bytes, _) = archive_resource(&path, &manifest.chapters[0].href, 1024).unwrap();
    assert!(String::from_utf8(bytes).unwrap().contains("Capitulo de teste"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn decoded_epub_paths_cannot_escape_archive_root() {
    assert_eq!(normalize_archive_join(Path::new("OPS"), "../text/a.xhtml").unwrap(), "text/a.xhtml");
    for href in ["%2e%2e/%2e%2e/outside", "%2foutside", "a%5cb.xhtml", "a%00.xhtml", "a%zz.xhtml", "a%2", "%ff.xhtml"] {
        assert!(normalize_archive_join(Path::new("OPS"), href).is_err(), "{href}");
    }
}
