use super::*;
#[test]
fn identical_samples_do_not_imply_identical_content() {
    let root = std::env::current_dir()
        .unwrap()
        .join(".cache/tmp")
        .join(format!("full-hash-{:032x}", rand::random::<u128>()));
    std::fs::create_dir_all(&root).unwrap();
    let mut data = vec![b'a'; 192 * 1024];
    let a = root.join("a.txt");
    let b = root.join("b.txt");
    std::fs::write(&a, &data).unwrap();
    data[96 * 1024] = b'b';
    std::fs::write(&b, &data).unwrap();
    assert_eq!(
        partial_content_hash(&a, data.len() as u64).unwrap(),
        partial_content_hash(&b, data.len() as u64).unwrap()
    );
    assert_ne!(
        full_content_hash(&a).unwrap(),
        full_content_hash(&b).unwrap()
    );
    std::fs::remove_dir_all(root).unwrap();
}
