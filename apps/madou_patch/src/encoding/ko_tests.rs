use super::*;
use std::path::Path;

#[test]
fn empty_path_returns_error() {
    let result = load_ko_encoding(Path::new("/nonexistent/path.tsv"));
    assert!(result.is_err());
}

/// Helper: build a minimal ko_table for testing.
fn test_ko_table() -> HashMap<char, Vec<u8>> {
    let mut t = HashMap::new();
    t.insert('\u{C774}', vec![0x20]); // 이
    t.insert('\u{C81C}', vec![0x97]); // 제
    t.insert('\u{C644}', vec![0xFB, 0xEB]); // 완
    t.insert('\u{C804}', vec![0x6C]); // 전
    t.insert('\u{D788}', vec![0xA0]); // 히
    t.insert('\u{C548}', vec![0x49]); // 안
    t.insert('\u{B3FC}', vec![0xFB, 0x24]); // 돼
    t.insert('\u{D55C}', vec![0x30]); // 한
    t.insert('\u{AE00}', vec![0x31]); // 글
    t.insert('\u{D14C}', vec![0x32]); // 테
    t.insert('\u{C2A4}', vec![0x33]); // 스
    t.insert('\u{D2B8}', vec![0x34]); // 트
    t.insert('\u{AC00}', vec![0x35]); // 가
    t.insert('\u{B098}', vec![0x36]); // 나
    t.insert('\u{B2E4}', vec![0x37]); // 다
    t.insert('\u{B77C}', vec![0x38]); // 라
    t.insert('\u{B9C8}', vec![0x39]); // 마
    t.insert('\u{BC14}', vec![0x3A]); // 바
    t.insert('\u{C0AC}', vec![0x3B]); // 사
    t.insert('\u{C544}', vec![0x3C]); // 아
    t.insert('A', vec![0xFB, 0x40]); // A via ko_table
    t.insert('B', vec![0xFB, 0x41]); // B via ko_table
    t.insert('\u{00B7}', vec![0xF0, 0x4B]); // · (middle dot) via ko_table
    t
}

// -- FIXED_ENCODE tests ---

#[test]
fn fixed_encode_space() {
    let m = build_fixed_encode_map();
    assert_eq!(m[&' '], BLANK_RENDER);
    assert_eq!(m[&'\u{3000}'], BLANK_RENDER);
}

#[test]
fn fixed_encode_digits() {
    let m = build_fixed_encode_map();
    assert_eq!(m[&'0'], 0x01);
    assert_eq!(m[&'9'], 0x0A);
    assert_eq!(m[&'\u{FF10}'], 0x01); // fullwidth 0
    assert_eq!(m[&'\u{FF19}'], 0x0A); // fullwidth 9
}

#[test]
fn fixed_encode_punctuation() {
    let m = build_fixed_encode_map();
    assert_eq!(m[&'!'], 0x0B);
    assert_eq!(m[&'\u{FF01}'], 0x0B);
    assert_eq!(m[&'~'], 0x0C);
    assert_eq!(m[&'.'], 0x0D);
    assert_eq!(m[&'?'], 0x0E);
    assert_eq!(m[&'\u{FF1F}'], 0x0E);
    assert_eq!(m[&'"'], 0x0F);
}

#[test]
fn fixed_encode_arrows() {
    let m = build_fixed_encode_map();
    assert_eq!(m[&'\u{2192}'], 0x10); // ->
    assert_eq!(m[&'\u{2191}'], 0x11); // up
    assert_eq!(m[&'\u{2190}'], 0x12); // <-
    assert_eq!(m[&'\u{2193}'], 0x13); // down
}

#[test]
fn fixed_encode_hyphen_variants() {
    let m = build_fixed_encode_map();
    assert_eq!(m[&'-'], 0x14);
    assert_eq!(m[&'\u{30FC}'], 0x14); // katakana long vowel
    assert_eq!(m[&'\u{2212}'], 0x14); // minus sign
}

#[test]
fn fixed_encode_brackets() {
    let m = build_fixed_encode_map();
    assert_eq!(m[&'['], 0x16);
    assert_eq!(m[&'\u{3010}'], 0x16); // left black lenticular bracket
    assert_eq!(m[&']'], 0x17);
    assert_eq!(m[&'\u{3011}'], 0x17); // right black lenticular bracket
    assert_eq!(m[&'\u{300C}'], 0x19); // left corner bracket
    assert_eq!(m[&'\u{300D}'], 0x1A); // right corner bracket
}

// -- Fullwidth normalization tests ---

#[test]
fn normalize_fullwidth_latin() {
    assert_eq!(normalize_fullwidth('\u{FF21}'), 'A');
    assert_eq!(normalize_fullwidth('\u{FF22}'), 'B');
    assert_eq!(normalize_fullwidth('\u{FF52}'), 'r');
    assert_eq!(normalize_fullwidth('\u{FF54}'), 't');
}

#[test]
fn normalize_fullwidth_passthrough() {
    assert_eq!(normalize_fullwidth('X'), 'X');
    assert_eq!(normalize_fullwidth('\u{D55C}'), '\u{D55C}');
}

// -- JP encode table tests ---

#[test]
fn jp_encode_table_katakana() {
    let jt = build_jp_encode_table();
    assert_eq!(jt[&'\u{30B1}'], vec![0x8A]); // ケ
    assert_eq!(jt[&'\u{30A2}'], vec![0x7E]); // ア
}

#[test]
fn jp_encode_table_hiragana() {
    let jt = build_jp_encode_table();
    assert_eq!(jt[&'\u{3042}'], vec![0x2E]); // あ
    assert_eq!(jt[&'\u{3093}'], vec![0x7C]); // ん
}

#[test]
fn jp_encode_table_fb_kanji() {
    let jt = build_jp_encode_table();
    assert_eq!(jt[&'\u{624B}'], vec![0xFB, 0x00]); // 手
}

// -- encode_ko_string tests ---

#[test]
fn encode_basic_korean() {
    let ko = test_ko_table();
    let result = encode_ko_string("\u{C774}\u{C81C}", &ko).unwrap();
    // No padding — VRAM clearing is handled by engine hook
    assert_eq!(result, vec![0x20, 0x97]); // 이, 제
}

#[test]
fn encode_ten_chars_no_padding() {
    let ko = test_ko_table();
    // "이제 완전히 안돼!" -> exactly 10 rendered chars
    let text = "\u{C774}\u{C81C} \u{C644}\u{C804}\u{D788} \u{C548}\u{B3FC}!";
    let result = encode_ko_string(text, &ko).unwrap();
    let expected: Vec<u8> = vec![
        0x20, 0x97, 0x18, 0xFB, 0xEB, 0x6C, 0xA0, 0x18, 0x49, 0xFB, 0x24, 0x0B,
    ];
    assert_eq!(result, expected);
}

#[test]
fn encode_with_nl() {
    let ko = test_ko_table();
    let text = "\u{D55C}\u{AE00}\u{C774}{NL}\u{D14C}\u{C2A4}\u{D2B8}";
    let result = encode_ko_string(text, &ko).unwrap();
    // No padding — engine hook clears remaining VRAM tiles
    assert_eq!(result, vec![0x30, 0x31, 0x20, 0xF9, 0x32, 0x33, 0x34]); // 한글이{NL}테스트
}

#[test]
fn encode_box_tag() {
    let ko = test_ko_table();
    let text = "{BOX:\u{30A2}\u{30EB}\u{30EB}}\u{D55C}\u{AE00}";
    let result = encode_ko_string(text, &ko).unwrap();
    assert_eq!(result, vec![0xFC, 0x00, 0x30, 0x31]); // FC + speaker + 한글
}

#[test]
fn encode_page_with_branch_marker() {
    let ko = test_ko_table();
    let text = "\u{D55C}\u{AE00}{PAGE}\u{30B1}\u{D14C}\u{C2A4}\u{D2B8}";
    let result = encode_ko_string(text, &ko).unwrap();
    assert_eq!(result, vec![0x30, 0x31, 0xF8, 0x8A, 0x32, 0x33, 0x34]); // 한글{PAGE}ケ테스트
}

#[test]
fn encode_sep_tag() {
    let ko = test_ko_table();
    let text = "\u{D55C}{SEP}\u{AE00}";
    let result = encode_ko_string(text, &ko).unwrap();
    assert_eq!(result, vec![0x30, 0xFE, 0x31]); // 한{SEP}글
}

#[test]
fn encode_choice_tag() {
    let ko = test_ko_table();
    let text = "\u{D55C}{CHOICE}\u{AE00}";
    let result = encode_ko_string(text, &ko).unwrap();
    assert_eq!(result, vec![0x30, 0xFD, 0x31]); // 한{CHOICE}글
}

#[test]
fn encode_raw_tag() {
    let ko = test_ko_table();
    let text = "{RAW:AB}\u{D55C}";
    let result = encode_ko_string(text, &ko).unwrap();
    assert_eq!(result, vec![0xAB, 0x30]); // raw byte + 한
}

#[test]
fn encode_fullwidth_normalization() {
    let ko = test_ko_table();
    let text = "\u{FF21}"; // Ａ -> A
    let result = encode_ko_string(text, &ko).unwrap();
    assert_eq!(result, vec![0xFB, 0x40]); // A via ko_table
}

#[test]
fn encode_newline_skipped() {
    let ko = test_ko_table();
    let text = "\u{D55C}\n\u{AE00}";
    let result = encode_ko_string(text, &ko).unwrap();
    assert_eq!(result, vec![0x30, 0x31]); // 한글 (newline skipped)
}

#[test]
fn encode_unknown_char_error() {
    let ko = test_ko_table();
    let result = encode_ko_string("\u{1234}", &ko);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Unencodable"));
}

#[test]
fn encode_unknown_speaker_error() {
    let ko = test_ko_table();
    let result = encode_ko_string("{BOX:UNKNOWN}", &ko);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Unknown speaker"));
}

#[test]
fn encode_digits() {
    let ko = test_ko_table();
    let text = "0123456789";
    let result = encode_ko_string(text, &ko).unwrap();
    let expected: Vec<u8> = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A];
    assert_eq!(result, expected); // exactly 10 chars, no padding
}

#[test]
fn encode_empty_string() {
    let ko = test_ko_table();
    let result = encode_ko_string("", &ko).unwrap();
    assert!(result.is_empty());
}

#[test]
fn encode_nl_at_start() {
    let ko = test_ko_table();
    // {NL} with 0 line chars -> no padding before NL
    let text = "{NL}\u{D55C}";
    let result = encode_ko_string(text, &ko).unwrap();
    assert_eq!(result[0], 0xF9); // NL (no padding)
    assert_eq!(result[1], 0x30); // 한
}

// -- FF terminator tests ---

#[test]
fn encode_ff_terminator() {
    let ko = test_ko_table();
    let result = encode_ko_string_ff("\u{D55C}", &ko).unwrap();
    assert_eq!(*result.last().unwrap(), 0xFF);
}

#[test]
fn encode_ff_empty() {
    let ko = test_ko_table();
    let result = encode_ko_string_ff("", &ko).unwrap();
    assert_eq!(result, vec![0xFF]);
}

// -- encode_simple tests ---

#[test]
fn encode_simple_basic() {
    let ko = test_ko_table();
    let result = encode_simple("\u{D55C}\u{AE00}", &ko).unwrap();
    assert_eq!(result, vec![0x30, 0x31]); // No padding
}

#[test]
fn encode_simple_space_is_blank_render() {
    let ko = test_ko_table();
    let result = encode_simple(" ", &ko).unwrap();
    assert_eq!(result, vec![0x18]); // BLANK_RENDER — writes zeros to VRAM
}

#[test]
fn encode_simple_newline_is_f9() {
    let ko = test_ko_table();
    let result = encode_simple("\u{D55C}\n\u{AE00}", &ko).unwrap();
    assert_eq!(result, vec![0x30, 0xF9, 0x31]);
}

#[test]
fn encode_simple_dot() {
    let ko = test_ko_table();
    let result = encode_simple(".", &ko).unwrap();
    assert_eq!(result, vec![0x0D]);
}

#[test]
fn encode_simple_digits() {
    let ko = test_ko_table();
    let result = encode_simple("123", &ko).unwrap();
    assert_eq!(result, vec![0x02, 0x03, 0x04]);
}

#[test]
fn encode_simple_ff_terminator() {
    let ko = test_ko_table();
    let result = encode_simple_ff("\u{D55C}", &ko).unwrap();
    assert_eq!(result, vec![0x30, 0xFF]);
}

#[test]
fn encode_simple_unknown_error() {
    let ko = test_ko_table();
    let result = encode_simple("\u{2603}", &ko); // snowman, not in table
    assert!(result.is_err());
}

#[test]
fn encode_simple_punctuation() {
    let ko = test_ko_table();
    assert_eq!(encode_simple("!", &ko).unwrap(), vec![0x0B]);
    assert_eq!(encode_simple("?", &ko).unwrap(), vec![0x0E]);
    assert_eq!(encode_simple("~", &ko).unwrap(), vec![0x0C]);
    assert_eq!(encode_simple(",", &ko).unwrap(), vec![0x15]);
}

#[test]
fn encode_simple_hex_escape() {
    let ko = test_ko_table();
    // {FE} → $FE, {F9} → $F9
    assert_eq!(encode_simple("{FE}", &ko).unwrap(), vec![0xFE]);
    assert_eq!(encode_simple("{F9}", &ko).unwrap(), vec![0xF9]);
    // Mixed: text + hex escape
    assert_eq!(encode_simple("!{FE}", &ko).unwrap(), vec![0x0B, 0xFE]);
    // Invalid hex
    assert!(encode_simple("{ZZ}", &ko).is_err());
}

// -- Middle dot test ---

#[test]
fn encode_middle_dot() {
    let ko = test_ko_table();
    // U+30FB (katakana middle dot) normalizes to U+00B7, uses ko_table
    let result = encode_ko_string("\u{30FB}", &ko).unwrap();
    assert_eq!(result, vec![0xF0, 0x4B]);
}

#[test]
fn encode_interpunct() {
    let ko = test_ko_table();
    // U+00B7 (middle dot) uses ko_table directly
    let result = encode_ko_string("\u{00B7}", &ko).unwrap();
    assert_eq!(result, vec![0xF0, 0x4B]);
}

// -- Page without branch marker ---

#[test]
fn encode_page_without_branch_marker() {
    let ko = test_ko_table();
    let text = "\u{D55C}{PAGE}\u{AE00}";
    let result = encode_ko_string(text, &ko).unwrap();
    assert_eq!(result, vec![0x30, 0xF8, 0x31]); // 한{PAGE}글
}

// -- BOX resets after_page ---

#[test]
fn encode_box_resets_after_page() {
    let ko = test_ko_table();
    let text = "{BOX:\u{30A2}\u{30EB}\u{30EB}}\u{D55C}";
    let result = encode_ko_string(text, &ko).unwrap();
    assert_eq!(result[0], 0xFC);
    assert_eq!(result[1], 0x00);
    assert_eq!(result[2], 0x30); // normal encoding
}

// -- Legacy compatibility tests ---

#[test]
fn encode_special_chars_simple() {
    let table = HashMap::new();
    let result = encode_simple(" \n.0129", &table).unwrap();
    assert_eq!(result, vec![0x18, 0xF9, 0x0D, 0x01, 0x02, 0x03, 0x0A]);
}

#[test]
fn encode_with_table_simple() {
    let mut table = HashMap::new();
    table.insert('\u{AC00}', vec![0x29]); // 가
    table.insert('\u{B098}', vec![0x33]); // 나
    let result = encode_simple("\u{AC00}\u{B098}", &table).unwrap();
    assert_eq!(result, vec![0x29, 0x33]);
}

#[test]
fn encode_missing_char_returns_error_simple() {
    let table = HashMap::new();
    let result = encode_simple("\u{AC00}", &table); // 가
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("U+AC00"));
}

#[test]
fn encode_multibyte_sequence_simple() {
    let mut table = HashMap::new();
    table.insert('\u{CD08}', vec![0xFB, 0x09]); // 초
    let result = encode_simple("\u{CD08}", &table).unwrap();
    assert_eq!(result, vec![0xFB, 0x09]);
}
