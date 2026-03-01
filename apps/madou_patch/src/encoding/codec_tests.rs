use super::*;

#[test]
fn decode_simple_hiragana() {
    // あいう = 0x2E, 0x30, 0x32, 0xFF
    let data = [0x2E, 0x30, 0x32, 0xFF];
    let tokens = decode_jp(&data);
    let s = tokens_to_string(&tokens);
    assert_eq!(s, "あいう");
}

#[test]
fn decode_with_fb_prefix() {
    // FB 00 = 手, FF
    let data = [0xFB, 0x00, 0xFF];
    let tokens = decode_jp(&data);
    let s = tokens_to_string(&tokens);
    assert_eq!(s, "手");
}

#[test]
fn decode_with_fc_textbox() {
    let data = [0xFC, 0x00, 0x2E, 0xFF];
    let tokens = decode_jp(&data);
    let s = tokens_to_string(&tokens);
    assert!(s.contains("[BOX:アルル]"));
    assert!(s.contains("あ"));
}

#[test]
fn decode_control_codes() {
    // F9=newline, F8=page, FF=end
    let data = [0x2E, 0xF9, 0x30, 0xF8, 0x32, 0xFF];
    let tokens = decode_jp(&data);
    assert_eq!(count_chars(&tokens), 3);
    assert_eq!(count_unknowns(&tokens), 0);
}

#[test]
fn decode_space_byte() {
    let data = [0x00, 0xFF];
    let tokens = decode_jp(&data);
    assert_eq!(tokens.len(), 2);
    assert!(matches!(tokens[0], Token::Control(ControlCode::Space)));
    let s = tokens_to_string(&tokens);
    assert_eq!(s, "\u{3000}"); // full-width space
}

#[test]
fn decode_fd_choice() {
    let data = [0xFD, 0xFF];
    let tokens = decode_jp(&data);
    assert!(matches!(tokens[0], Token::Control(ControlCode::Choice)));
    let s = tokens_to_string(&tokens);
    assert!(s.contains("<CHOICE>"));
}

#[test]
fn decode_fe_separator() {
    let data = [0xFE, 0xFF];
    let tokens = decode_jp(&data);
    assert!(matches!(tokens[0], Token::Control(ControlCode::Separator)));
    let s = tokens_to_string(&tokens);
    assert!(s.contains("|"));
}

#[test]
fn decode_unknown_byte() {
    // Byte 0xEE is unlikely to be in the decode table
    let data = [0xEE, 0xFF];
    let tokens = decode_jp(&data);
    // Should be either Unknown or a valid char depending on table
    assert_eq!(tokens.len(), 2);
}

#[test]
fn decode_f0_prefix() {
    let data = [0xF0, 0x10, 0xFF];
    let tokens = decode_jp(&data);
    assert_eq!(tokens.len(), 2);
    match &tokens[0] {
        Token::Char(GameChar::Prefixed(0xF0, 0x10), ch) => {
            assert_eq!(*ch, '\u{FFFD}');
        }
        _ => panic!("Expected F0 prefixed char"),
    }
}

#[test]
fn decode_f1_prefix() {
    let data = [0xF1, 0x05, 0xFF];
    let tokens = decode_jp(&data);
    assert_eq!(tokens.len(), 2);
    match &tokens[0] {
        Token::Char(GameChar::Prefixed(0xF1, 0x05), ch) => {
            assert_eq!(*ch, '\u{FFFD}');
        }
        _ => panic!("Expected F1 prefixed char"),
    }
}

#[test]
fn decode_fb_at_end_of_stream() {
    // FB with no following byte before FF
    let data = [0xFB];
    let tokens = decode_jp(&data);
    assert!(tokens.is_empty());
}

#[test]
fn decode_fc_at_end_of_stream() {
    // FC with no following speaker byte
    let data = [0xFC];
    let tokens = decode_jp(&data);
    assert_eq!(tokens.len(), 1);
    assert!(matches!(tokens[0], Token::Control(ControlCode::TextBox(0))));
}

#[test]
fn decode_empty_stream_with_ff() {
    let data = [0xFF];
    let tokens = decode_jp(&data);
    assert_eq!(tokens.len(), 1);
    assert!(matches!(tokens[0], Token::Control(ControlCode::End)));
}

#[test]
fn tokens_to_string_unknown_formats() {
    let tokens = vec![
        Token::Unknown(0xAB),
        Token::UnknownFb(0xCD),
        Token::UnknownFa(0xEF),
        Token::UnknownF0(0x12),
        Token::Control(ControlCode::End),
    ];
    let s = tokens_to_string(&tokens);
    assert!(s.contains("[AB]"));
    assert!(s.contains("[FB:CD]"));
    assert!(s.contains("[FA:EF]"));
    assert!(s.contains("[F0:12]"));
}

#[test]
fn count_unknowns_mixed() {
    let tokens = vec![
        Token::Char(GameChar::Single(0x2E), 'あ'),
        Token::Unknown(0xEE),
        Token::UnknownFb(0x00),
        Token::Control(ControlCode::End),
    ];
    assert_eq!(count_unknowns(&tokens), 2);
}

#[test]
fn count_chars_includes_unknowns() {
    let tokens = vec![
        Token::Char(GameChar::Single(0x2E), 'あ'),
        Token::Unknown(0xEE),
        Token::Control(ControlCode::Newline),
        Token::Control(ControlCode::End),
    ];
    // Char + Unknown = 2 displayable chars, Control not counted
    assert_eq!(count_chars(&tokens), 2);
}

#[test]
fn encode_with_table_basic() {
    let mut table = HashMap::new();
    table.insert('a', vec![0x2E]);
    table.insert('b', vec![0xFB, 0x00]);

    let (bytes, warnings) = encode_with_table("ab", &table);
    assert_eq!(bytes, vec![0x2E, 0xFB, 0x00]);
    assert!(warnings.is_empty());
}

#[test]
fn encode_with_table_unencodable() {
    let table = HashMap::new();
    let (bytes, warnings) = encode_with_table("x", &table);
    assert_eq!(bytes, vec![0x00]); // space fallback
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("UNENCODABLE"));
}

#[test]
fn speaker_name_known() {
    assert_eq!(speaker_name(0x00), "アルル");
    assert_eq!(speaker_name(0x03), "NPC");
}

#[test]
fn speaker_name_unknown() {
    assert_eq!(speaker_name(0xFF), "???");
}
