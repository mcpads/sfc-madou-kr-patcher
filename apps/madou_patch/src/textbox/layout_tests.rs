use super::*;
use crate::encoding::codec::{ControlCode, GameChar, Token};

#[test]
fn single_line_no_overflow() {
    let tokens: Vec<Token> = (0..10)
        .map(|i| Token::Char(GameChar::Single(0x2E + i), 'あ'))
        .chain(std::iter::once(Token::Control(ControlCode::End)))
        .collect();

    let result = render_pages(&tokens);
    assert!(!result.overflow);
    assert_eq!(result.pages.len(), 1);
}

#[test]
fn overflow_detection() {
    // 11 chars × 2 tiles = 22 tiles → wraps to 2 lines (10 + 1), no overflow
    let tokens: Vec<Token> = (0..11)
        .map(|_| Token::Char(GameChar::Single(0x2E), 'あ'))
        .chain(std::iter::once(Token::Control(ControlCode::End)))
        .collect();

    let result = render_pages(&tokens);
    assert!(!result.overflow);
    assert_eq!(result.pages[0].lines.len(), 2);
}

#[test]
fn fb_chars_are_double_width() {
    // 11 FB chars = 22 tiles = overflow on wrap
    let tokens: Vec<Token> = (0..11)
        .map(|_| Token::Char(GameChar::Prefixed(0xFB, 0x00), '手'))
        .chain(std::iter::once(Token::Control(ControlCode::End)))
        .collect();

    let result = render_pages(&tokens);
    // 10 chars (20 tiles) fit on line 1, 1 char wraps to line 2
    assert_eq!(result.pages[0].lines.len(), 2);
}

#[test]
fn fa_chars_are_double_width() {
    assert_eq!(char_width(&GameChar::Prefixed(0xFA, 0x00)), 2);
}

#[test]
fn f0_chars_are_double_width() {
    assert_eq!(char_width(&GameChar::Prefixed(0xF0, 0x00)), 2);
}

#[test]
fn f1_chars_are_double_width() {
    // All dialogue characters are 16×16 = 2 tile columns
    assert_eq!(char_width(&GameChar::Prefixed(0xF1, 0x00)), 2);
}

#[test]
fn page_break_creates_new_page() {
    let tokens = vec![
        Token::Char(GameChar::Single(0x2E), 'あ'),
        Token::Control(ControlCode::PageBreak),
        Token::Char(GameChar::Single(0x30), 'い'),
        Token::Control(ControlCode::End),
    ];

    let result = render_pages(&tokens);
    assert_eq!(result.pages.len(), 2);
    assert!(!result.overflow);
}

#[test]
fn textbox_starts_new_page() {
    let tokens = vec![
        Token::Char(GameChar::Single(0x2E), 'あ'),
        Token::Control(ControlCode::TextBox(0x00)),
        Token::Char(GameChar::Single(0x30), 'い'),
        Token::Control(ControlCode::End),
    ];

    let result = render_pages(&tokens);
    assert_eq!(result.pages.len(), 2);
}

#[test]
fn overflow_when_more_than_3_lines() {
    // 4 explicit newlines = 4 lines > 3 max
    let tokens = vec![
        Token::Char(GameChar::Single(0x2E), 'あ'),
        Token::Control(ControlCode::Newline),
        Token::Char(GameChar::Single(0x2E), 'あ'),
        Token::Control(ControlCode::Newline),
        Token::Char(GameChar::Single(0x2E), 'あ'),
        Token::Control(ControlCode::Newline),
        Token::Char(GameChar::Single(0x2E), 'あ'),
        Token::Control(ControlCode::End),
    ];

    let result = render_pages(&tokens);
    assert!(result.overflow);
}

#[test]
fn exactly_3_lines_no_overflow() {
    let tokens = vec![
        Token::Char(GameChar::Single(0x2E), 'あ'),
        Token::Control(ControlCode::Newline),
        Token::Char(GameChar::Single(0x2E), 'あ'),
        Token::Control(ControlCode::Newline),
        Token::Char(GameChar::Single(0x2E), 'あ'),
        Token::Control(ControlCode::End),
    ];

    let result = render_pages(&tokens);
    assert_eq!(result.pages.len(), 1);
    assert_eq!(result.pages[0].lines.len(), 3);
    assert!(!result.overflow);
}

#[test]
fn exactly_20_tiles_no_wrap() {
    // 10 chars × 2 tiles = 20 tiles = exactly fits
    let tokens: Vec<Token> = (0..10)
        .map(|_| Token::Char(GameChar::Single(0x2E), 'あ'))
        .chain(std::iter::once(Token::Control(ControlCode::End)))
        .collect();

    let result = render_pages(&tokens);
    assert_eq!(result.pages[0].lines.len(), 1);
    assert_eq!(result.pages[0].lines[0].width, 20);
    assert!(!result.overflow);
}

#[test]
fn space_wraps_like_char() {
    // 10 chars (20 tiles) + 1 space (2 tiles) = wrap to 2 lines
    let mut tokens: Vec<Token> = (0..10)
        .map(|_| Token::Char(GameChar::Single(0x2E), 'あ'))
        .collect();
    tokens.push(Token::Control(ControlCode::Space));
    tokens.push(Token::Control(ControlCode::End));

    let result = render_pages(&tokens);
    assert_eq!(result.pages[0].lines.len(), 2);
}

#[test]
fn empty_tokens_no_pages() {
    let tokens: Vec<Token> = vec![Token::Control(ControlCode::End)];
    let result = render_pages(&tokens);
    assert!(result.pages.is_empty());
    assert!(!result.overflow);
}

#[test]
fn unknown_tokens_have_width_2() {
    // Unknown tokens are also 16×16 = 2 tile columns each
    let tokens = vec![
        Token::Unknown(0xEE),
        Token::UnknownFb(0x00),
        Token::Control(ControlCode::End),
    ];

    let result = render_pages(&tokens);
    assert_eq!(result.pages[0].lines[0].width, 4);
}
