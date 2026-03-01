use super::*;
use std::collections::HashMap;

/// Helper: build a minimal ko_table for testing.
fn test_ko_table() -> HashMap<char, Vec<u8>> {
    let mut t = HashMap::new();
    t.insert('\u{C774}', vec![0x20]); // 이
    t.insert('\u{C544}', vec![0x21]); // 아
    t.insert('\u{B974}', vec![0x22]); // 르
    t.insert('\u{B2E4}', vec![0x23]); // 다
    t.insert('\u{AC00}', vec![0x29]); // 가
    t.insert('\u{D558}', vec![0x2A]); // 하
    t.insert('\u{D638}', vec![0xFB, 0x00]); // 호
    t.insert('\u{C880}', vec![0xFB, 0x01]); // 좀
    t
}

// ── parse_hex_string tests ──────────────────────────────────

#[test]
fn lookup_parse_hex_single_byte() {
    let bytes = parse_hex_string("30").unwrap();
    assert_eq!(bytes, vec![0x30]);
}

#[test]
fn lookup_parse_hex_multi_bytes() {
    let bytes = parse_hex_string("FB 67 30 53").unwrap();
    assert_eq!(bytes, vec![0xFB, 0x67, 0x30, 0x53]);
}

#[test]
fn lookup_parse_hex_lowercase() {
    let bytes = parse_hex_string("fb 0a").unwrap();
    assert_eq!(bytes, vec![0xFB, 0x0A]);
}

#[test]
fn lookup_parse_hex_empty() {
    let result = parse_hex_string("");
    assert!(result.is_err());
}

#[test]
fn lookup_parse_hex_invalid() {
    let result = parse_hex_string("ZZ");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("ZZ"));
}

// ── bytes_to_jp tests ───────────────────────────────────────

#[test]
fn lookup_bytes_to_jp_hiragana() {
    let result = bytes_to_jp(&[0x2E, 0x30, 0x32]);
    assert_eq!(result, "\u{3042}\u{3044}\u{3046}"); // あいう
}

#[test]
fn lookup_bytes_to_jp_fb_prefix() {
    let result = bytes_to_jp(&[0xFB, 0x00]);
    assert_eq!(result, "\u{624B}"); // 手
}

#[test]
fn lookup_bytes_to_jp_control_codes() {
    let result = bytes_to_jp(&[0x2E, 0xF9, 0x30, 0xFF]);
    assert_eq!(result, "\u{3042}{NL}\u{3044}{END}");
}

#[test]
fn lookup_bytes_to_jp_space() {
    let result = bytes_to_jp(&[0x00]);
    assert_eq!(result, " ");
}

#[test]
fn lookup_bytes_to_jp_unknown() {
    // 0x12 has no JP mapping (between special glyphs and alphabet)
    let result = bytes_to_jp(&[0x12]);
    assert_eq!(result, "[12]");
}

// ── bytes_to_ko tests ───────────────────────────────────────

#[test]
fn lookup_bytes_to_ko_single() {
    let ko = test_ko_table();
    let result = bytes_to_ko(&[0x20], &ko);
    assert_eq!(result, "\u{C774}"); // 이
}

#[test]
fn lookup_bytes_to_ko_fb_prefix() {
    let ko = test_ko_table();
    let result = bytes_to_ko(&[0xFB, 0x00], &ko);
    assert_eq!(result, "\u{D638}"); // 호
}

#[test]
fn lookup_bytes_to_ko_fixed() {
    let ko = test_ko_table();
    let result = bytes_to_ko(&[0x01], &ko); // digit 0
    assert_eq!(result, "0");
}

#[test]
fn lookup_bytes_to_ko_control() {
    let ko = test_ko_table();
    let result = bytes_to_ko(&[0xF9, 0xFF], &ko);
    assert_eq!(result, "{NL}{END}");
}

// ── jp_to_bytes tests ───────────────────────────────────────

#[test]
fn lookup_jp_to_bytes_hiragana() {
    let result = jp_to_bytes("\u{3042}\u{3044}"); // あい
    assert_eq!(result, vec![0x2E, 0x30]);
}

#[test]
fn lookup_jp_to_bytes_fb_kanji() {
    let result = jp_to_bytes("\u{624B}"); // 手
    assert_eq!(result, vec![0xFB, 0x00]);
}

#[test]
fn lookup_jp_to_bytes_unknown_skipped() {
    let result = jp_to_bytes("X\u{3042}"); // X is not in JP table, skipped
                                           // X is not in JP table -> skipped
                                           // Actually \u{FF38} = Ｘ is at 0x2A... but plain ASCII 'X' is not
    assert_eq!(result, vec![0x2E]);
}

// ── ko_to_bytes tests ───────────────────────────────────────

#[test]
fn lookup_ko_to_bytes_hangul() {
    let ko = test_ko_table();
    let result = ko_to_bytes("\u{C774}\u{AC00}", &ko); // 이가
    assert_eq!(result, vec![0x20, 0x29]);
}

#[test]
fn lookup_ko_to_bytes_fb() {
    let ko = test_ko_table();
    let result = ko_to_bytes("\u{D638}", &ko); // 호
    assert_eq!(result, vec![0xFB, 0x00]);
}

#[test]
fn lookup_ko_to_bytes_fixed() {
    let ko = test_ko_table();
    let result = ko_to_bytes("!", &ko);
    assert_eq!(result, vec![0x0B]);
}

#[test]
fn lookup_ko_to_bytes_digit() {
    let ko = test_ko_table();
    let result = ko_to_bytes("5", &ko);
    assert_eq!(result, vec![0x06]);
}

// ── Round-trip tests ────────────────────────────────────────

#[test]
fn lookup_roundtrip_jp_bytes_jp() {
    // JP text -> bytes -> JP text should match for simple chars
    let original = "\u{3042}\u{3044}\u{3046}"; // あいう
    let bytes = jp_to_bytes(original);
    let decoded = bytes_to_jp(&bytes);
    assert_eq!(decoded, original);
}

#[test]
fn lookup_roundtrip_ko_bytes_ko() {
    let ko = test_ko_table();
    let original = "\u{C774}\u{AC00}"; // 이가
    let bytes = ko_to_bytes(original, &ko);
    let decoded = bytes_to_ko(&bytes, &ko);
    assert_eq!(decoded, original);
}

// ── Fixed decode table test ─────────────────────────────────

#[test]
fn lookup_fixed_decode_coverage() {
    let fixed = build_fixed_decode();
    assert_eq!(fixed[&0x00], ' ');
    assert_eq!(fixed[&0x01], '0');
    assert_eq!(fixed[&0x0A], '9');
    assert_eq!(fixed[&0x0B], '!');
    assert_eq!(fixed[&0x0D], '.');
    assert_eq!(fixed[&0x0E], '?');
    assert_eq!(fixed[&0x14], '-');
    assert_eq!(fixed[&0x16], '[');
    assert_eq!(fixed[&0x17], ']');
}
