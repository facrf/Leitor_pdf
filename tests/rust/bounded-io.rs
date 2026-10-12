use super::*;
#[test]
fn detects_overflow_without_writing_extra_byte() {
    let mut input = &b"12345"[..];
    let mut output = Vec::new();
    assert!(copy_limited(&mut input, &mut output, 4).is_err());
    assert_eq!(output, b"1234");
    assert_eq!(read_limited(&mut &b"1234"[..], 4).unwrap(), b"1234");
    assert!(read_limited(&mut &b"x"[..], 0).is_err());
}
