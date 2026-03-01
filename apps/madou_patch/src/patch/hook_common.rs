//! Shared utilities for LZ intercept hooks (savemenu, options_screen, worldmap).

use crate::rom::lorom_to_pc;

/// Expected bytes at each hook site: `JSL $009440` (LZ decompressor call).
pub const JSL_LZ_BYTES: [u8; 4] = [0x22, 0x40, 0x94, 0x00];

/// Read a 16-bit pointer from an LZ pointer table and return its PC offset.
///
/// `ptr_bank` is the bank containing the pointer table data,
/// `ptr_table_pc` is the PC offset of the table start,
/// `entry_idx` is the zero-based index into the table.
pub fn lookup_lz_source(
    rom: &[u8],
    ptr_bank: u8,
    ptr_table_pc: usize,
    entry_idx: usize,
) -> Result<usize, String> {
    let ptr_pc = ptr_table_pc + entry_idx * 2;
    if ptr_pc + 2 > rom.len() {
        return Err(format!("LZ pointer entry {} out of bounds", entry_idx));
    }
    let offset = u16::from_le_bytes([rom[ptr_pc], rom[ptr_pc + 1]]);
    Ok(lorom_to_pc(ptr_bank, offset))
}

#[cfg(test)]
#[path = "hook_common_tests.rs"]
mod tests;
