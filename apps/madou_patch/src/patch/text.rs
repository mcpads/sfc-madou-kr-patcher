//! In-place text replacement in ROM.
//!
//! Loads translations directly from TSV files and encodes Korean text
//! using the KO encoding engine. Replaces strings in-place at their
//! original ROM addresses.

use crate::patch::tracked_rom::TrackedRom;
use crate::patch::translation;
use std::collections::HashMap;
use std::path::Path;

/// Stats from a text patching operation.
#[derive(Debug, Default)]
pub struct PatchStats {
    pub total: usize,
    pub replaced: usize,
    pub truncated: usize,
    pub skipped: usize,
}

/// Replace text strings in-place at their original ROM addresses.
///
/// For FF-terminated banks: scans to next FF for boundary.
/// For FC-split banks: uses consecutive entry addresses for boundary detection.
pub fn patch_inplace(
    rom: &mut TrackedRom,
    bank_id: &str,
    translations_dir: &Path,
    ko_table: &HashMap<char, Vec<u8>>,
    fc_split: bool,
) -> Result<PatchStats, String> {
    let entries = translation::load_and_encode_bank(translations_dir, bank_id, ko_table, fc_split)?;

    // Pre-compute PC addresses
    let pc_list: Vec<usize> = entries.iter().map(|e| e.addr.to_pc()).collect();

    let mut stats = PatchStats::default();

    for (i, entry) in entries.iter().enumerate() {
        stats.total += 1;
        let pc = pc_list[i];

        let orig_len = if fc_split && i + 1 < entries.len() {
            pc_list[i + 1] - pc
        } else {
            // Scan to FF
            let mut end = pc;
            while end < rom.len() && rom[end] != 0xFF {
                if matches!(rom[end], 0xFA | 0xFB | 0xFC | 0xF0 | 0xF1) && end + 1 < rom.len() {
                    end += 2;
                } else {
                    end += 1;
                }
            }
            end += 1; // include FF
            end - pc
        };

        if orig_len == 0 {
            stats.skipped += 1;
            continue;
        }

        let ko_data = &entry.encoded;

        let label = "text:inplace";
        if ko_data.len() <= orig_len {
            {
                let mut r = rom.region(pc, orig_len, label);
                r.copy_at(0, ko_data);
                let remaining = orig_len - ko_data.len();
                if remaining > 0 {
                    // Always fill with 0xFF (end marker) so the game engine stops
                    // reading at the text's FF terminator. Previous 0x00 fill for
                    // FC-split banks caused trailing spaces → oversized dialogue boxes.
                    r.data_mut()[ko_data.len()..].fill(0xFF);
                }
            }
            stats.replaced += 1;
        } else {
            // Truncate: fit as much Korean data as possible in the original slot.
            let mut cut = if fc_split { orig_len } else { orig_len - 1 };

            // Prevent prefix mid-cut: if the last byte would be a multi-byte
            // prefix (FB/FA/F0/F1), its pair byte gets cut off.
            if cut > 0 && matches!(ko_data[cut - 1], 0xFA | 0xFB | 0xF0 | 0xF1) {
                cut -= 1;
            }

            {
                let mut r = rom.region(pc, orig_len, label);
                r.copy_at(0, &ko_data[..cut]);
                r.data_mut()[cut..].fill(0xFF);
            }
            stats.truncated += 1;
        }
    }

    Ok(stats)
}
