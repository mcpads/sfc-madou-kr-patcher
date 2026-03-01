//! Text stream parsers: FF-terminated and FC-split modes.
//!
//! Matches the logic in scripts/extract_jp_text.py.

use crate::encoding::jp;

/// Extracted string with its SNES address and raw bytes.
#[derive(Debug, Clone)]
pub struct RawString {
    pub snes_addr: u16,
    pub data: Vec<u8>,
}

/// Check if a byte is a recognized text character (for ratio filter).
fn is_text_byte(b: u8, table: &[Option<char>; 256]) -> bool {
    table[b as usize].is_some() || b == 0xFB || b == 0xFC
}

/// Noise filter: detect data table entries in banks like $2D.
/// Matches _is_data_table_entry from Python.
fn is_data_table_entry(raw: &[u8]) -> bool {
    if raw.len() < 4 {
        return false;
    }

    let has_fc = raw.contains(&0xFC);

    // Pattern 1: starts with 01 3E
    if raw[0] == 0x01 && raw[1] == 0x3E {
        return true;
    }

    // Pattern 2: starts with 7F 00 00
    if raw[0] == 0x7F && raw.len() >= 3 && raw[1] == 0x00 && raw[2] == 0x00 {
        return true;
    }

    // Pattern 3: no FC + high space/digit ratio
    if !has_fc {
        let mut space_count = 0usize;
        let mut digit_count = 0usize;
        let mut total = 0usize;
        let mut i = 0;
        while i < raw.len() {
            let b = raw[i];
            if b == 0xFF {
                break;
            } else if b == 0xFB {
                i += 2;
                total += 1;
                continue;
            } else if b == 0x00 {
                space_count += 1;
                total += 1;
            } else if (0x01..=0x09).contains(&b) {
                digit_count += 1;
                total += 1;
            } else {
                total += 1;
            }
            i += 1;
        }
        if total > 10 && (space_count + digit_count) * 100 > total * 35 {
            return true;
        }
    }

    // Pattern 4: starts with 7F without FC, long
    if raw[0] == 0x7F && !has_fc && raw.len() > 20 {
        return true;
    }

    // Pattern 5: very long blobs
    if raw.len() > 500 {
        return true;
    }

    false
}

/// Extract text strings from ROM data for a given bank range.
///
/// `data` is the full ROM. `bank`, `start_addr`, `end_addr` define the SNES range.
/// If `fc_split` is true, FC bytes are text-box separators.
/// If `filter_noise` is true, apply data-table rejection heuristic.
pub fn extract_strings(
    rom: &[u8],
    bank: u8,
    start_addr: u16,
    end_addr: u16,
    fc_split: bool,
    filter_noise: bool,
) -> Vec<RawString> {
    let table = jp::build_decode_table();
    let pc_start = crate::rom::lorom_to_pc(bank, start_addr);
    let pc_end = crate::rom::lorom_to_pc(bank, end_addr);

    if pc_end > rom.len() || pc_start >= pc_end {
        return Vec::new();
    }

    let data = &rom[pc_start..pc_end];
    let mut strings = Vec::new();
    let mut i = 0;

    if fc_split {
        while i < data.len() {
            if data[i] == 0xFF {
                i += 1;
                continue;
            }

            let start = i;
            let mut raw = Vec::new();
            let mut text_chars = 0usize;

            while i < data.len() {
                let b = data[i];
                raw.push(b);
                if b == 0xFF {
                    i += 1;
                    break;
                }
                if is_text_byte(b, &table) {
                    text_chars += 1;
                }
                i += 1;
            }

            // Filter: >55% recognized bytes
            if raw.len() >= 4 && text_chars * 100 > raw.len() * 55 {
                if filter_noise && is_data_table_entry(&raw) {
                    continue;
                }
                strings.push(RawString {
                    snes_addr: start_addr.wrapping_add(start as u16),
                    data: raw,
                });
            }
        }
    } else {
        while i < data.len() {
            if data[i] == 0xFF {
                i += 1;
                continue;
            }

            let start = i;
            let mut raw = Vec::new();
            let mut text_chars = 0usize;

            while i < data.len() {
                let b = data[i];
                raw.push(b);
                if b == 0xFF {
                    i += 1;
                    break;
                }
                if table[b as usize].is_some() || b == 0xFB {
                    text_chars += 1;
                }
                i += 1;
            }

            // Filter: >50% recognized bytes, >= 3 bytes
            if raw.len() >= 3 && text_chars * 100 > raw.len() * 50 {
                strings.push(RawString {
                    snes_addr: start_addr.wrapping_add(start as u16),
                    data: raw,
                });
            }
        }
    }

    strings
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
