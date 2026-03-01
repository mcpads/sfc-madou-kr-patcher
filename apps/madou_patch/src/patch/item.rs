//! Item name table patch for KO localization.
//!
//! The inventory system reads item names from a fixed-width table at
//! $2B:$FD8B — 18 entries x 10 bytes each. The renderer indexes by
//! `item_id * 10`, so the stride must be preserved exactly.
//!
//! The original KO translation used {NL}/{SEP} delimiters which broke
//! the fixed-width structure, causing the inventory renderer to read
//! bytes at wrong offsets ("もうび" garbage).
//!
//! This module encodes each KO item name into exactly 10 bytes,
//! padding with $00 (space) if shorter, truncating at character
//! boundary if longer.

use crate::encoding::ko;
use crate::patch::tracked_rom::TrackedRom;
use std::collections::HashMap;

// ── Constants ────────────────────────────────────────────────────────

/// PC offset of the item name table: lorom_to_pc(0x2B, 0xFD8B).
const ITEM_NAME_TABLE_PC: usize = 0x15FD8B;

/// Number of items in the table.
const ITEM_COUNT: usize = 18;

/// Byte stride per item name entry.
const ITEM_NAME_STRIDE: usize = 10;

/// KO item names, indexed 0-17.
/// Order matches the JP original at $2B:$FD8B.
const KO_ITEM_NAMES: [&str; ITEM_COUNT] = [
    "락교",         // 0: らっきょ
    "후쿠신즈케",   // 1: ふくしんづけ
    "카레라이스",   // 2: カレーライス
    "마도주",       // 3: 魔導酒
    "모모모주",     // 4: ももも酒
    "용의 고기",    // 5: 竜の肉
    "마도수정",     // 6: 魔導水晶
    "뇌천초",       // 7: のうてん草
    "각력초",       // 8: きゃくりょく草
    "눈알초",       // 9: めんたま草
    "탈리스만",     // 10: タリスマン
    "거북이심장",   // 11: カメのしんぞう
    "여행의행복",   // 12: 旅のしぁわせ
    "마오리가의약", // 13: マオリガの薬
    "전갈맨의지갑", // 14: さそりまんのさいふ
    "어둠의 꽃",    // 15: 闇の花
    "만드레이크잎", // 16: マンドレイクの葉
    "빛의 물방울",  // 17: 光のしずく
];

// ── Truncation helper ───────────────────────────────────────────────

/// Truncate encoded bytes at a character boundary, respecting multi-byte prefixes.
fn truncate_at_char_boundary(bytes: &[u8], max_len: usize) -> Vec<u8> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let char_len = match bytes[i] {
            0xFB | 0xF0 | 0xF1 | 0xFA => 2,
            _ => 1,
        };
        if result.len() + char_len > max_len {
            break;
        }
        for j in 0..char_len {
            if i + j < bytes.len() {
                result.push(bytes[i + j]);
            }
        }
        i += char_len;
    }
    result
}

// ── Main entry point ─────────────────────────────────────────────────

/// Patch the item name table at $2B:$FD8B with KO names.
///
/// Each name is encoded via `ko::encode_simple()`, truncated to
/// ITEM_NAME_STRIDE bytes at character boundary, and padded with $00.
///
/// Returns the number of items patched.
pub fn patch_item_name_table(
    rom: &mut TrackedRom,
    ko_table: &HashMap<char, Vec<u8>>,
) -> Result<usize, String> {
    let table_size = ITEM_COUNT * ITEM_NAME_STRIDE;
    let table_end = ITEM_NAME_TABLE_PC + table_size;
    if table_end > rom.len() {
        return Err(format!(
            "Item name table end 0x{:X} exceeds ROM size 0x{:X}",
            table_end,
            rom.len()
        ));
    }

    // Build the full table data in memory, then write once
    let mut table_data = vec![0x00u8; table_size];

    let mut patched = 0;

    for (i, &name) in KO_ITEM_NAMES.iter().enumerate() {
        let encoded = ko::encode_simple(name, ko_table)
            .map_err(|e| format!("Item #{} '{}' encoding error: {}", i, name, e))?;

        let fitted = if encoded.len() > ITEM_NAME_STRIDE {
            let truncated = truncate_at_char_boundary(&encoded, ITEM_NAME_STRIDE);
            println!(
                "  Item #{} '{}': truncated from {} to {} bytes",
                i,
                name,
                encoded.len(),
                truncated.len()
            );
            truncated
        } else {
            encoded
        };

        let offset = i * ITEM_NAME_STRIDE;
        table_data[offset..offset + fitted.len()].copy_from_slice(&fitted);

        patched += 1;
    }

    rom.write(ITEM_NAME_TABLE_PC, &table_data, "item:name_table");

    Ok(patched)
}

/// Collect all unique KO characters from item names.
pub fn all_ko_chars() -> Vec<char> {
    let mut chars: Vec<char> = KO_ITEM_NAMES
        .iter()
        .flat_map(|name| name.chars())
        .filter(|ch| !ch.is_whitespace())
        .collect();
    chars.sort_unstable();
    chars.dedup();
    chars
}

#[cfg(test)]
#[path = "item_tests.rs"]
mod tests;
