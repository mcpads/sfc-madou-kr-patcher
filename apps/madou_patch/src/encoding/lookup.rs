//! Encoding lookup utilities: hex bytes <-> JP char <-> KO char conversions.
//!
//! Provides bidirectional lookup between game byte codes, JP characters,
//! and KO characters sharing the same tile positions.

use std::collections::HashMap;

use super::jp;
use super::ko;

/// Names for well-known control codes.
fn control_name(byte: u8) -> Option<&'static str> {
    match byte {
        0xF8 => Some("{PAGE}"),
        0xF9 => Some("{NL}"),
        0xFA => Some("{FA:speaker}"),
        0xFB => Some("{FB prefix}"),
        0xFC => Some("{BOX}"),
        0xFD => Some("{CHOICE}"),
        0xFE => Some("{SEP}"),
        0xFF => Some("{END}"),
        _ => None,
    }
}

/// Decode a byte sequence to JP text using the JP encoding tables.
pub fn bytes_to_jp(bytes: &[u8]) -> String {
    let table = jp::build_decode_table();
    let fb_table = jp::build_fb_decode_table();
    let mut result = String::new();
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];
        match b {
            0xFF => {
                result.push_str("{END}");
                i += 1;
            }
            0xF9 => {
                result.push_str("{NL}");
                i += 1;
            }
            0xF8 => {
                result.push_str("{PAGE}");
                i += 1;
            }
            0xFC => {
                i += 1;
                if i < bytes.len() {
                    let speaker = bytes[i];
                    result.push_str(&format!("{{BOX:{:02X}}}", speaker));
                    i += 1;
                } else {
                    result.push_str("{BOX}");
                }
            }
            0xFD => {
                result.push_str("{CHOICE}");
                i += 1;
            }
            0xFE => {
                result.push_str("{SEP}");
                i += 1;
            }
            0xFB => {
                i += 1;
                if i < bytes.len() {
                    let idx = bytes[i];
                    i += 1;
                    if let Some(ch) = fb_table[idx as usize] {
                        result.push(ch);
                    } else {
                        result.push_str(&format!("[FB:{:02X}]", idx));
                    }
                }
            }
            0xFA => {
                i += 1;
                if i < bytes.len() {
                    let idx = bytes[i];
                    result.push_str(&format!("{{FA:{:02X}}}", idx));
                    i += 1;
                }
            }
            0xF1 => {
                i += 1;
                if i < bytes.len() {
                    let idx = bytes[i];
                    result.push_str(&format!("[F1:{:02X}]", idx));
                    i += 1;
                }
            }
            0xF0 => {
                i += 1;
                if i < bytes.len() {
                    let idx = bytes[i];
                    result.push_str(&format!("[F0:{:02X}]", idx));
                    i += 1;
                }
            }
            0x00 => {
                result.push(' ');
                i += 1;
            }
            _ => {
                if let Some(ch) = table[b as usize] {
                    result.push(ch);
                } else {
                    result.push_str(&format!("[{:02X}]", b));
                }
                i += 1;
            }
        }
    }
    result
}

/// Decode a byte sequence to KO text using a reverse KO encoding table.
///
/// The `ko_table` maps char -> bytes (encode direction).
/// We build the reverse (bytes -> char) internally.
pub fn bytes_to_ko(bytes: &[u8], ko_table: &HashMap<char, Vec<u8>>) -> String {
    let ko_decode = ko::build_ko_decode_table(ko_table);
    let fixed_decode = build_fixed_decode();
    let mut result = String::new();
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];
        match b {
            0xFF => {
                result.push_str("{END}");
                i += 1;
            }
            0xF9 => {
                result.push_str("{NL}");
                i += 1;
            }
            0xF8 => {
                result.push_str("{PAGE}");
                i += 1;
            }
            0xFC => {
                i += 1;
                if i < bytes.len() {
                    let speaker = bytes[i];
                    result.push_str(&format!("{{BOX:{:02X}}}", speaker));
                    i += 1;
                } else {
                    result.push_str("{BOX}");
                }
            }
            0xFD => {
                result.push_str("{CHOICE}");
                i += 1;
            }
            0xFE => {
                result.push_str("{SEP}");
                i += 1;
            }
            0xFB => {
                i += 1;
                if i < bytes.len() {
                    let idx = bytes[i];
                    i += 1;
                    let key = vec![0xFB, idx];
                    if let Some(&ch) = ko_decode.get(&key) {
                        result.push(ch);
                    } else {
                        result.push_str(&format!("[FB:{:02X}]", idx));
                    }
                }
            }
            0xFA => {
                i += 1;
                if i < bytes.len() {
                    let idx = bytes[i];
                    result.push_str(&format!("{{FA:{:02X}}}", idx));
                    i += 1;
                }
            }
            0xF1 => {
                i += 1;
                if i < bytes.len() {
                    let idx = bytes[i];
                    result.push_str(&format!("[F1:{:02X}]", idx));
                    i += 1;
                }
            }
            0xF0 => {
                i += 1;
                if i < bytes.len() {
                    let idx = bytes[i];
                    result.push_str(&format!("[F0:{:02X}]", idx));
                    i += 1;
                }
            }
            _ => {
                // Try fixed decode first
                if let Some(ch) = fixed_decode.get(&b) {
                    result.push(*ch);
                } else {
                    // Try single-byte KO decode
                    let key = vec![b];
                    if let Some(&ch) = ko_decode.get(&key) {
                        result.push(ch);
                    } else {
                        result.push_str(&format!("[{:02X}]", b));
                    }
                }
                i += 1;
            }
        }
    }
    result
}

/// Encode JP text to game bytes (using JP encoding tables).
#[allow(dead_code)]
pub fn jp_to_bytes(text: &str) -> Vec<u8> {
    let jp_encode = ko::build_jp_encode_table();
    let mut result = Vec::new();

    for ch in text.chars() {
        if let Some(bytes) = jp_encode.get(&ch) {
            result.extend_from_slice(bytes);
        }
        // Silently skip unknown chars
    }
    result
}

/// Encode KO text to game bytes (using KO encoding table).
#[allow(dead_code)]
pub fn ko_to_bytes(text: &str, ko_table: &HashMap<char, Vec<u8>>) -> Vec<u8> {
    let fixed = ko::build_fixed_encode_map();
    let mut result = Vec::new();

    for ch in text.chars() {
        if let Some(&byte) = fixed.get(&ch) {
            result.push(byte);
        } else if let Some(bytes) = ko_table.get(&ch) {
            result.extend_from_slice(bytes);
        }
        // Silently skip unknown chars
    }
    result
}

/// Parse a hex string like "FB 67 30 53" into bytes.
pub fn parse_hex_string(hex: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    for token in hex.split_whitespace() {
        let b =
            u8::from_str_radix(token, 16).map_err(|_| format!("Invalid hex byte: '{}'", token))?;
        bytes.push(b);
    }
    if bytes.is_empty() {
        return Err("Empty hex string".to_string());
    }
    Ok(bytes)
}

/// Print a detailed lookup table for each character in a byte sequence.
///
/// Shows: BYTES | JP | KO for each logical character.
pub fn print_lookup_table(bytes: &[u8], ko_table: &HashMap<char, Vec<u8>>) {
    let jp_table = jp::build_decode_table();
    let fb_table = jp::build_fb_decode_table();
    let ko_decode = ko::build_ko_decode_table(ko_table);
    let fixed_decode = build_fixed_decode();

    println!("{:<12} {:<8} {:<8} NOTE", "BYTES", "JP", "KO");
    println!("{}", "-".repeat(48));

    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];

        // Check for control codes
        if let Some(name) = control_name(b) {
            match b {
                0xFB => {
                    // FB prefix: consume two bytes
                    i += 1;
                    if i < bytes.len() {
                        let idx = bytes[i];
                        let hex = format!("FB {:02X}", idx);
                        let jp_ch = fb_table[idx as usize]
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "---".to_string());
                        let ko_key = vec![0xFB, idx];
                        let ko_ch = ko_decode
                            .get(&ko_key)
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "---".to_string());
                        println!("{:<12} {:<8} {:<8} FB prefix", hex, jp_ch, ko_ch);
                        i += 1;
                    }
                    continue;
                }
                0xFC => {
                    i += 1;
                    if i < bytes.len() {
                        let speaker = bytes[i];
                        println!(
                            "FC {:02X}        {:<8} {:<8} {{BOX:{:02X}}}",
                            speaker, "---", "---", speaker
                        );
                        i += 1;
                    } else {
                        println!("{:<12} {:<8} {:<8} {}", "FC", "---", "---", name);
                        i += 1;
                    }
                    continue;
                }
                0xFA => {
                    i += 1;
                    if i < bytes.len() {
                        let arg = bytes[i];
                        println!(
                            "{:<12} {:<8} {:<8} {{FA:{:02X}}}",
                            format!("FA {:02X}", arg),
                            "---",
                            "---",
                            arg
                        );
                        i += 1;
                    }
                    continue;
                }
                0xF0 | 0xF1 => {
                    let prefix = b;
                    i += 1;
                    if i < bytes.len() {
                        let idx = bytes[i];
                        let hex = format!("{:02X} {:02X}", prefix, idx);
                        let ko_key = vec![prefix, idx];
                        let ko_ch = ko_decode
                            .get(&ko_key)
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "---".to_string());
                        println!(
                            "{:<12} {:<8} {:<8} {:02X} prefix",
                            hex, "---", ko_ch, prefix
                        );
                        i += 1;
                    }
                    continue;
                }
                _ => {
                    // Simple control code (F8, F9, FD, FE, FF)
                    println!(
                        "{:<12} {:<8} {:<8} {}",
                        format!("{:02X}", b),
                        "---",
                        "---",
                        name
                    );
                    i += 1;
                    continue;
                }
            }
        }

        // Regular byte
        let hex = format!("{:02X}", b);
        let jp_ch = jp_table[b as usize]
            .map(|c| c.to_string())
            .unwrap_or_else(|| "---".to_string());

        let ko_ch = if let Some(ch) = fixed_decode.get(&b) {
            ch.to_string()
        } else {
            let key = vec![b];
            ko_decode
                .get(&key)
                .map(|c| c.to_string())
                .unwrap_or_else(|| "---".to_string())
        };

        let note = if b == 0x00 {
            "space"
        } else if b <= 0x1F {
            "fixed"
        } else if (0xC8..=0xF7).contains(&b) {
            "kanji/KO"
        } else {
            ""
        };

        println!("{:<12} {:<8} {:<8} {}", hex, jp_ch, ko_ch, note);
        i += 1;
    }
}

/// Look up a single JP character and show its byte encoding + KO equivalent.
pub fn lookup_jp_char(ch: char, ko_table: &HashMap<char, Vec<u8>>) {
    let jp_encode = ko::build_jp_encode_table();
    let ko_decode = ko::build_ko_decode_table(ko_table);
    let fixed_decode = build_fixed_decode();

    if let Some(bytes) = jp_encode.get(&ch) {
        let hex: Vec<String> = bytes.iter().map(|b| format!("{:02X}", b)).collect();
        let hex_str = hex.join(" ");

        // Find KO char at same byte position
        let ko_ch = if bytes.len() == 1 {
            let b = bytes[0];
            if let Some(fc) = fixed_decode.get(&b) {
                Some(*fc)
            } else {
                ko_decode.get(bytes).copied()
            }
        } else {
            ko_decode.get(bytes).copied()
        };

        let ko_str = ko_ch
            .map(|c| format!("{}", c))
            .unwrap_or_else(|| "---".to_string());

        println!("JP char:  {}", ch);
        println!("Bytes:    {}", hex_str);
        println!("KO char:  {}", ko_str);
    } else {
        println!(
            "JP char '{}' (U+{:04X}) not found in encoding table.",
            ch, ch as u32
        );
    }
}

/// Look up a single KO character and show its byte encoding + JP equivalent.
pub fn lookup_ko_char(ch: char, ko_table: &HashMap<char, Vec<u8>>) {
    let jp_table = jp::build_decode_table();
    let fb_table = jp::build_fb_decode_table();
    let fixed = ko::build_fixed_encode_map();

    // Check fixed encoding first
    if let Some(&byte) = fixed.get(&ch) {
        let jp_ch = jp_table[byte as usize];
        let jp_str = jp_ch
            .map(|c| format!("{}", c))
            .unwrap_or_else(|| "---".to_string());

        println!("KO char:  {} (fixed)", ch);
        println!("Bytes:    {:02X}", byte);
        println!("JP char:  {}", jp_str);
        return;
    }

    // Check KO encoding table
    if let Some(bytes) = ko_table.get(&ch) {
        let hex: Vec<String> = bytes.iter().map(|b| format!("{:02X}", b)).collect();
        let hex_str = hex.join(" ");

        let jp_ch = if bytes.len() == 1 {
            jp_table[bytes[0] as usize]
        } else if bytes.len() == 2 && bytes[0] == 0xFB {
            fb_table[bytes[1] as usize]
        } else {
            None
        };

        let jp_str = jp_ch
            .map(|c| format!("{}", c))
            .unwrap_or_else(|| "---".to_string());

        println!("KO char:  {}", ch);
        println!("Bytes:    {}", hex_str);
        println!("JP char:  {}", jp_str);
    } else {
        println!(
            "KO char '{}' (U+{:04X}) not found in encoding table.",
            ch, ch as u32
        );
    }
}

// ── Internal helpers ────────────────────────────────────────────

/// Build fixed-encode byte -> display char (reverse of ko::build_fixed_encode_map).
fn build_fixed_decode() -> HashMap<u8, char> {
    let mut m = HashMap::new();
    m.insert(0x00, ' ');
    for i in 0u8..10 {
        m.insert(i + 1, (b'0' + i) as char);
    }
    m.insert(0x0B, '!');
    m.insert(0x0C, '~');
    m.insert(0x0D, '.');
    m.insert(0x0E, '?');
    m.insert(0x0F, '"');
    m.insert(0x10, '\u{2192}'); // ->
    m.insert(0x11, '\u{2191}'); // up
    m.insert(0x12, '\u{2190}'); // <-
    m.insert(0x13, '\u{2193}'); // down
    m.insert(0x14, '-');
    m.insert(0x15, ',');
    m.insert(0x16, '[');
    m.insert(0x17, ']');
    m.insert(0x18, ' '); // BLANK_RENDER
    m.insert(0x19, '\u{300C}'); // 「
    m.insert(0x1A, '\u{300D}'); // 」
    m
}

#[cfg(test)]
#[path = "lookup_tests.rs"]
mod tests;
