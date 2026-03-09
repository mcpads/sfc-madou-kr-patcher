/// Korean encoding table loader (runtime TSV loading) and KO text encoder.
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use super::jp;

/// Load Korean encoding table from TSV file.
/// Format: CHAR\tUNICODE\tBYTES\tTILE_INDEX
/// Returns char → bytes mapping.
pub fn load_ko_encoding(path: &Path) -> Result<HashMap<char, Vec<u8>>, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read '{}': {}", path.display(), e))?;
    let mut table = HashMap::new();

    for line in content.lines().skip(1) {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 3 {
            let ch = parts[0].chars().next();
            if let Some(ch) = ch {
                let bytes: Result<Vec<u8>, _> = parts[2]
                    .split_whitespace()
                    .map(|h| u8::from_str_radix(h, 16))
                    .collect();
                if let Ok(bytes) = bytes {
                    table.insert(ch, bytes);
                }
            }
        }
    }
    Ok(table)
}

/// Build reverse lookup: bytes → char.
pub fn build_ko_decode_table(encode: &HashMap<char, Vec<u8>>) -> HashMap<Vec<u8>, char> {
    encode
        .iter()
        .map(|(&ch, bytes)| (bytes.clone(), ch))
        .collect()
}

// ── Constants ────────────────────────────────────────────────────

/// Blank tile that writes zeros to VRAM (unlike space $00 which skips).
const BLANK_RENDER: u8 = 0x18;

// ── FIXED_ENCODE mapping ─────────────────────────────────────────

/// Build the fixed-position encoding map (tiles $00-$1F).
/// These byte positions have dedicated font tiles, not Korean glyphs.
pub fn build_fixed_encode_map() -> HashMap<char, u8> {
    let mut m = HashMap::new();

    // Space -> BLANK_RENDER ($18)
    m.insert(' ', BLANK_RENDER);
    m.insert('\u{3000}', BLANK_RENDER); // fullwidth space

    // Digits 0-9 -> $01-$0A
    for i in 0u8..10 {
        m.insert((b'0' + i) as char, i + 1);
        m.insert(char::from_u32(0xFF10 + i as u32).unwrap(), i + 1); // fullwidth digits
    }

    // Punctuation
    m.insert('!', 0x0B);
    m.insert('\u{FF01}', 0x0B); // fullwidth !
    m.insert('~', 0x0C);
    m.insert('\u{FF5E}', 0x0C); // fullwidth ~
    m.insert('.', 0x0D);
    m.insert('?', 0x0E);
    m.insert('\u{FF1F}', 0x0E); // fullwidth ?
    m.insert('"', 0x0F);

    // Arrows
    m.insert('\u{2192}', 0x10); // →
    m.insert('\u{2191}', 0x11); // ↑
    m.insert('\u{2190}', 0x12); // ←
    m.insert('\u{2193}', 0x13); // ↓

    // Hyphen/dash variants
    m.insert('-', 0x14);
    m.insert('\u{30FC}', 0x14); // katakana long vowel ー
    m.insert('\u{2212}', 0x14); // minus sign −

    // Comma
    m.insert(',', 0x15);
    m.insert('\u{FF0C}', 0x15); // fullwidth comma ，

    // Brackets
    m.insert('[', 0x16);
    m.insert('\u{3010}', 0x16); // 【
    m.insert(']', 0x17);
    m.insert('\u{3011}', 0x17); // 】

    // Japanese quotation marks
    m.insert('\u{300C}', 0x19); // 「
    m.insert('\u{300D}', 0x1A); // 」

    m
}

// ── Fullwidth normalization ──────────────────────────────────────

/// Normalize select fullwidth Latin chars to halfwidth equivalents.
/// Only maps the specific chars that appear in the game text.
pub fn normalize_fullwidth(ch: char) -> char {
    match ch {
        '\u{FF21}' => 'A', // Ａ
        '\u{FF22}' => 'B', // Ｂ
        '\u{FF24}' => 'D', // Ｄ
        '\u{FF2A}' => 'J', // Ｊ
        '\u{FF34}' => 'T', // Ｔ
        '\u{FF41}' => 'a', // ａ
        '\u{FF42}' => 'b', // ｂ
        '\u{FF44}' => 'd', // ｄ
        '\u{FF4A}' => 'j', // ｊ
        '\u{FF52}' => 'r', // ｒ
        '\u{FF54}' => 't', // ｔ
        _ => ch,
    }
}

// ── Speaker name → ID mapping ────────────────────────────────────

/// Map speaker name to FC+byte speaker ID.
fn speaker_id(name: &str) -> Option<u8> {
    match name {
        "\u{30A2}\u{30EB}\u{30EB}" => Some(0x00), // アルル
        "\u{8A71}\u{8005}1" => Some(0x01),        // 話者1
        "\u{8A71}\u{8005}2" => Some(0x02),        // 話者2
        "NPC" => Some(0x03),
        _ => None,
    }
}

// ── JP encode table (for branch markers after {PAGE}) ────────────

/// Build char→bytes reverse lookup for JP encoding.
/// Used for branch marker bytes that appear after {PAGE} tags.
pub fn build_jp_encode_table() -> HashMap<char, Vec<u8>> {
    let mut table = HashMap::new();
    for &(byte, ch) in jp::SINGLE_BYTE_TABLE {
        table.insert(ch, vec![byte]);
    }
    for &(idx, ch) in jp::FB_PREFIX_TABLE {
        // Don't overwrite single-byte entries with FB-prefix
        table.entry(ch).or_insert_with(|| vec![0xFB, idx]);
    }
    table
}

// ── Main encoding function ───────────────────────────────────────

/// Encode a KO text string to game byte sequence.
///
/// Handles control tags ({NL}, {PAGE}, {BOX}, {SEP}, {CHOICE}, {RAW}),
/// branch marker preservation (JP chars after {PAGE}),
/// and Korean/fixed encoding.
///
/// VRAM tile clearing is handled by the engine-level clear_and_dispatch
/// hook — no text-level padding is needed.
///
/// Does NOT append FF terminator -- use `encode_ko_string_ff` for that.
pub fn encode_ko_string(text: &str, ko_table: &HashMap<char, Vec<u8>>) -> Result<Vec<u8>, String> {
    let fixed = build_fixed_encode_map();
    let jp_table = build_jp_encode_table();

    let mut result = Vec::new();
    let mut after_page = false;

    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        // Check for control tags: {TAG} or {TAG:arg}
        if chars[i] == '{' {
            if let Some(close) = chars[i..].iter().position(|&c| c == '}') {
                let tag_content: String = chars[i + 1..i + close].iter().collect();

                if let Some(speaker) = tag_content.strip_prefix("BOX:") {
                    if let Some(id) = speaker_id(speaker) {
                        result.push(0xFC);
                        result.push(id);
                    } else {
                        return Err(format!("Unknown speaker: {}", speaker));
                    }
                    after_page = false;
                    i += close + 1;
                    continue;
                } else if tag_content == "NL" {
                    result.push(0xF9);
                    i += close + 1;
                    continue;
                } else if tag_content == "PAGE" {
                    result.push(0xF8);
                    after_page = true;
                    i += close + 1;
                    continue;
                } else if tag_content == "SEP" {
                    result.push(0xFE);
                    after_page = false;
                    i += close + 1;
                    continue;
                } else if tag_content == "CHOICE" {
                    result.push(0xFD);
                    after_page = false;
                    i += close + 1;
                    continue;
                } else if let Some(hex) = tag_content.strip_prefix("RAW:") {
                    let byte = u8::from_str_radix(hex, 16)
                        .map_err(|_| format!("Invalid RAW hex: {}", hex))?;
                    result.push(byte);
                    // RAW bytes don't count as rendered characters
                    i += close + 1;
                    continue;
                }
                // Not a recognized tag -- fall through to char encoding
            }
        }

        let ch = chars[i];

        // After {PAGE}: next char may be a JP branch marker
        if after_page {
            if let Some(bytes) = jp_table.get(&ch) {
                result.extend_from_slice(bytes);
                // Branch marker is not a rendered character
                after_page = false;
                i += 1;
                continue;
            }
        }

        after_page = false;

        // Fullwidth Latin -> halfwidth normalization
        let ch = normalize_fullwidth(ch);

        // Normalize katakana middle dot (U+30FB) to KO middle dot (U+00B7)
        let ch = if ch == '\u{30FB}' { '\u{00B7}' } else { ch };

        // Fixed encoding (space, digits, punctuation, arrows, brackets)
        if let Some(&byte) = fixed.get(&ch) {
            result.push(byte);
        }
        // Korean encoding (Hangul syllables + Latin via ko_table)
        else if let Some(bytes) = ko_table.get(&ch) {
            result.extend_from_slice(bytes);
        }
        // Note: U+30FB (katakana middle dot) is normalized to U+00B7 above,
        // so it gets encoded via ko_table like any other KO glyph.
        // JP character fallback
        else if let Some(bytes) = jp_table.get(&ch) {
            result.extend_from_slice(bytes);
        }
        // Skip literal newlines
        else if ch == '\n' {
            // skip
        }
        // Unknown character
        else {
            return Err(format!("Unencodable char: '{}' U+{:04X}", ch, ch as u32));
        }

        i += 1;
    }

    Ok(result)
}

/// Encode a KO text string with FF terminator appended.
pub fn encode_ko_string_ff(
    text: &str,
    ko_table: &HashMap<char, Vec<u8>>,
) -> Result<Vec<u8>, String> {
    let mut bytes = encode_ko_string(text, ko_table)?;
    bytes.push(0xFF);
    Ok(bytes)
}

// ── Simple encoding for encyclopedia descriptions ────────────────

/// Simple encoder for encyclopedia descriptions, item names, and code patches.
/// No control tags, no line padding.
/// Space maps to 0x18 (BLANK_RENDER), which writes zeros to VRAM.
/// Note: 0x00 was previously used but it acts as a string terminator in the
/// game's item name renderer, causing names like "빛의 물방울" to be truncated.
pub fn encode_simple(text: &str, ko_table: &HashMap<char, Vec<u8>>) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        let ch = normalize_fullwidth(ch);
        match ch {
            '{' => {
                // Hex escape: {FE} → byte $FE, {F9} → byte $F9, etc.
                let hex: String = chars.by_ref().take_while(|&c| c != '}').collect();
                let byte = u8::from_str_radix(&hex, 16)
                    .map_err(|_| format!("Invalid hex escape: {{{}}}", hex))?;
                result.push(byte);
            }
            '\n' => result.push(0xF9),
            ' ' => result.push(BLANK_RENDER),
            '!' | '\u{FF01}' => result.push(0x0B),
            '?' | '\u{FF1F}' => result.push(0x0E),
            '.' => result.push(0x0D),
            '~' | '\u{FF5E}' => result.push(0x0C),
            ',' | '\u{FF0C}' => result.push(0x15),
            '0'..='9' => result.push(ch as u8 - b'0' + 1),
            _ => {
                if let Some(bytes) = ko_table.get(&ch) {
                    result.extend_from_slice(bytes);
                } else {
                    return Err(format!("Unencodable char: '{}' U+{:04X}", ch, ch as u32));
                }
            }
        }
    }

    Ok(result)
}

/// Simple encoder with FF terminator appended.
#[allow(dead_code)]
pub fn encode_simple_ff(text: &str, ko_table: &HashMap<char, Vec<u8>>) -> Result<Vec<u8>, String> {
    let mut bytes = encode_simple(text, ko_table)?;
    bytes.push(0xFF);
    Ok(bytes)
}

#[cfg(test)]
#[path = "ko_tests.rs"]
mod tests;
