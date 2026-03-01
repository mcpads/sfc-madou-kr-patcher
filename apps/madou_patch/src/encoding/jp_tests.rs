use super::*;

#[test]
fn decode_table_has_hiragana() {
    let t = build_decode_table();
    assert_eq!(t[0x2E], Some('あ'));
    assert_eq!(t[0x7C], Some('ん'));
}

#[test]
fn fb_table_has_kanji() {
    let t = build_fb_decode_table();
    assert_eq!(t[0x00], Some('手'));
    assert_eq!(t[0x34], Some('村'));
    assert_eq!(t[0x0B], Some('虫'));
    assert_eq!(t[0x47], Some('畑'));
}

#[test]
fn single_byte_count() {
    // Python has ~157 single-byte + 40 kanji_1byte = ~197 entries
    // plus misc/ctrl, total should match
    assert!(SINGLE_BYTE_TABLE.len() >= 150);
}

#[test]
fn fb_prefix_count() {
    // OCR-verified: 244 mapped + 1 fullwidth space = 245
    assert_eq!(FB_PREFIX_TABLE.len(), 245);
}
