use super::*;

fn fixture() -> PathBuf {
    let root = std::env::current_dir()
        .unwrap()
        .join(".cache/tmp")
        .join(format!("maintenance-{:032x}", rand::random::<u128>()));
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn trash_restores_without_overwriting_and_scan_skips_originals() {
    let root = fixture();
    fs::write(root.join("book.txt"), b"original").unwrap();
    let item = trash(&root, 1, "book.txt".into(), "book.txt".into()).unwrap();
    assert!(!root.join("book.txt").exists());
    assert_eq!(list_trash(&root).unwrap().len(), 1);
    let (books, _, _) = scanner::scan(
        &root,
        scanner::ScanProfile::Economical,
        &Default::default(),
        |_| {},
    )
    .unwrap();
    assert!(books.is_empty());
    fs::write(root.join("book.txt"), b"replacement").unwrap();
    assert!(restore_trash(&root, &item.token).is_err());
    assert_eq!(fs::read(root.join("book.txt")).unwrap(), b"replacement");
    fs::remove_file(root.join("book.txt")).unwrap();
    restore_trash(&root, &item.token).unwrap();
    assert_eq!(fs::read(root.join("book.txt")).unwrap(), b"original");
    assert!(list_trash(&root).unwrap().is_empty());
    assert!(restore_trash(&root, "../bad").is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cleanup_protects_referenced_recent_and_symlink_covers() {
    let root = fixture();
    for name in ["old.png", "referenced.png", "recent.png"] {
        fs::write(root.join(name), b"image").unwrap();
    }
    let old = SystemTime::now() - Duration::from_secs(8 * 86400);
    for name in ["old.png", "referenced.png"] {
        fs::File::options()
            .write(true)
            .open(root.join(name))
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(old))
            .unwrap();
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("old.png"), root.join("link.png")).unwrap();
    let items = orphan_covers(&root, &HashSet::from(["referenced.png".into()])).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].filename, "old.png");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn trash_restores_when_parent_subfolder_was_removed() {
    let root = fixture();
    let subfolder = root.join("sub").join("nested");
    fs::create_dir_all(&subfolder).unwrap();
    let file = subfolder.join("nested_book.txt");
    fs::write(&file, b"nested content").unwrap();
    let item = trash(
        &root,
        2,
        "nested_book.txt".into(),
        "sub/nested/nested_book.txt".into(),
    )
    .unwrap();
    assert!(!file.exists());
    // Remove the nested subfolder
    fs::remove_dir_all(root.join("sub")).unwrap();
    assert!(!root.join("sub").exists());
    // Restore should succeed and recreate the parent directories
    restore_trash(&root, &item.token).unwrap();
    assert_eq!(
        fs::read(root.join("sub/nested/nested_book.txt")).unwrap(),
        b"nested content"
    );
    fs::remove_dir_all(root).unwrap();
}
