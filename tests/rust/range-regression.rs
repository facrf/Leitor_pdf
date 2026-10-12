use super::*;
#[test]
fn distinguishes_unsatisfiable_from_unsupported_ranges() {
    assert!(is_unsatisfiable_range("bytes=10-", 10));
    assert!(is_unsatisfiable_range("bytes=12-20", 10));
    assert!(is_unsatisfiable_range("bytes=-0", 10));
    assert!(is_unsatisfiable_range("bytes=0-", 0));
    assert!(is_unsatisfiable_range("bytes=-1", 0));
    for range in [
        "bytes=9-20",
        "bytes=-20",
        "bytes=bad-",
        "bytes=10-5",
        "bytes=0-1,4-5",
        "items=20-",
    ] {
        assert!(!is_unsatisfiable_range(range, 10), "{range}");
    }
    assert_eq!(parse_range("bytes=9-20", 10), Some((9, 9)));
    assert_eq!(parse_range("bytes=-20", 10), Some((0, 9)));
}
