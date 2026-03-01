/// Decode/encode functions for game text byte sequences.
use std::collections::HashMap;

use super::jp;

/// A game character — either single-byte or prefixed (e.g., FB+XX).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameChar {
    Single(u8),
    Prefixed(u8, u8),
}

/// Control codes used by the text engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlCode {
    Space,       // 0x00
    Newline,     // 0xF9
    PageBreak,   // 0xF8
    TextBox(u8), // 0xFC + speaker_id
    Choice,      // 0xFD
    Separator,   // 0xFE
    End,         // 0xFF
}

/// Decoded token from a byte stream.
#[derive(Debug, Clone)]
pub enum Token {
    Char(GameChar, char),
    Control(ControlCode),
    Unknown(u8),
    UnknownFb(u8),
    #[allow(dead_code)]
    UnknownFa(u8),
    #[allow(dead_code)]
    UnknownF0(u8),
}

/// Speaker names for FC+byte text box markers.
pub fn speaker_name(id: u8) -> &'static str {
    match id {
        0x00 => "アルル",
        0x01 => "話者1",
        0x02 => "話者2",
        0x03 => "NPC",
        _ => "???",
    }
}

/// Decode a raw byte stream into tokens using JP encoding.
pub fn decode_jp(data: &[u8]) -> Vec<Token> {
    let table = jp::build_decode_table();
    let fb_table = jp::build_fb_decode_table();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < data.len() {
        let b = data[i];
        match b {
            0xFF => {
                tokens.push(Token::Control(ControlCode::End));
                break;
            }
            0xF9 => {
                tokens.push(Token::Control(ControlCode::Newline));
                i += 1;
            }
            0xF8 => {
                tokens.push(Token::Control(ControlCode::PageBreak));
                i += 1;
            }
            0xFC => {
                i += 1;
                let speaker = if i < data.len() {
                    let s = data[i];
                    i += 1;
                    s
                } else {
                    0
                };
                tokens.push(Token::Control(ControlCode::TextBox(speaker)));
            }
            0xFD => {
                tokens.push(Token::Control(ControlCode::Choice));
                i += 1;
            }
            0xFE => {
                tokens.push(Token::Control(ControlCode::Separator));
                i += 1;
            }
            0xFB => {
                i += 1;
                if i < data.len() {
                    let idx = data[i];
                    i += 1;
                    if let Some(ch) = fb_table[idx as usize] {
                        tokens.push(Token::Char(GameChar::Prefixed(0xFB, idx), ch));
                    } else {
                        tokens.push(Token::UnknownFb(idx));
                    }
                }
            }
            0xF1 => {
                i += 1;
                if i < data.len() {
                    let idx = data[i];
                    i += 1;
                    tokens.push(Token::Char(GameChar::Prefixed(0xF1, idx), '\u{FFFD}'));
                }
            }
            0xF0 => {
                i += 1;
                if i < data.len() {
                    let idx = data[i];
                    i += 1;
                    tokens.push(Token::Char(GameChar::Prefixed(0xF0, idx), '\u{FFFD}'));
                }
            }
            0x00 => {
                tokens.push(Token::Control(ControlCode::Space));
                i += 1;
            }
            _ => {
                if let Some(ch) = table[b as usize] {
                    tokens.push(Token::Char(GameChar::Single(b), ch));
                } else {
                    tokens.push(Token::Unknown(b));
                }
                i += 1;
            }
        }
    }
    tokens
}

/// Format decoded tokens as a display string (matching Python output).
pub fn tokens_to_string(tokens: &[Token]) -> String {
    let mut result = String::new();
    for token in tokens {
        match token {
            Token::Char(_, ch) => result.push(*ch),
            Token::Control(ctrl) => match ctrl {
                ControlCode::Space => result.push('\u{3000}'),
                ControlCode::Newline => result.push('\n'),
                ControlCode::PageBreak => result.push('▽'),
                ControlCode::TextBox(id) => {
                    result.push_str(&format!("\n[BOX:{}] ", speaker_name(*id)));
                }
                ControlCode::Choice => result.push_str("<CHOICE>"),
                ControlCode::Separator => result.push('|'),
                ControlCode::End => {}
            },
            Token::Unknown(b) => result.push_str(&format!("[{:02X}]", b)),
            Token::UnknownFb(b) => result.push_str(&format!("[FB:{:02X}]", b)),
            Token::UnknownFa(b) => result.push_str(&format!("[FA:{:02X}]", b)),
            Token::UnknownF0(b) => result.push_str(&format!("[F0:{:02X}]", b)),
        }
    }
    result
}

/// Count unknown tokens in a token sequence.
pub fn count_unknowns(tokens: &[Token]) -> usize {
    tokens
        .iter()
        .filter(|t| {
            matches!(
                t,
                Token::Unknown(_) | Token::UnknownFb(_) | Token::UnknownFa(_) | Token::UnknownF0(_)
            )
        })
        .count()
}

/// Count displayable characters (excluding control codes).
pub fn count_chars(tokens: &[Token]) -> usize {
    tokens
        .iter()
        .filter(|t| {
            matches!(
                t,
                Token::Char(_, _)
                    | Token::Unknown(_)
                    | Token::UnknownFb(_)
                    | Token::UnknownFa(_)
                    | Token::UnknownF0(_)
            )
        })
        .count()
}

/// Encode a unicode string to game bytes using a char→bytes mapping.
#[allow(dead_code)]
pub fn encode_with_table(text: &str, table: &HashMap<char, Vec<u8>>) -> (Vec<u8>, Vec<String>) {
    let mut result = Vec::new();
    let mut warnings = Vec::new();

    for ch in text.chars() {
        if let Some(bytes) = table.get(&ch) {
            result.extend_from_slice(bytes);
        } else {
            warnings.push(format!("UNENCODABLE: '{}' U+{:04X}", ch, ch as u32));
            result.push(0x00); // space fallback
        }
    }

    (result, warnings)
}

#[cfg(test)]
#[path = "codec_tests.rs"]
mod tests;
