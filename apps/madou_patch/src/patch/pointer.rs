//! Pointer table analysis and rewriting for text relocation.
//!
//! The game uses 3-byte long pointers (bank:addr_lo:addr_hi) in Bank $02
//! to reference text data. The EN patch redirects 178 pointers from
//! original banks to relocated text in Bank $31-$33.

use crate::patch::tracked_rom::TrackedRom;
use crate::rom::{lorom_to_pc, SnesAddr};

/// A pointer table entry.
#[derive(Debug, Clone)]
pub struct PointerEntry {
    /// PC offset of this pointer in the ROM.
    pub pc_offset: usize,
    /// The SNES address this pointer references.
    pub target: SnesAddr,
}

/// Scan for 3-byte long pointers in a bank range.
/// Looks for pointers that reference a given target bank.
///
/// Scans byte-by-byte (not every 3rd byte) to find all pointer occurrences
/// regardless of alignment. Each candidate is `[addr_lo, addr_hi, bank]`.
pub fn scan_pointers(
    rom: &[u8],
    scan_bank: u8,
    scan_start: u16,
    scan_end: u16,
    target_bank: Option<u8>,
) -> Vec<PointerEntry> {
    let pc_start = lorom_to_pc(scan_bank, scan_start);
    let pc_end = lorom_to_pc(scan_bank, scan_end);
    let mut entries = Vec::new();

    if pc_end + 3 > rom.len() {
        return entries;
    }

    // Scan byte-by-byte to find all 3-byte pointer patterns
    for i in pc_start..pc_end.saturating_sub(2) {
        let addr_lo = rom[i];
        let addr_hi = rom[i + 1];
        let bank = rom[i + 2];

        let addr = u16::from_le_bytes([addr_lo, addr_hi]);

        // Valid LoROM pointer: bank 0x00-0x3F, addr 0x8000-0xFFFF
        if bank <= 0x3F && addr >= 0x8000 && (target_bank.is_none() || target_bank == Some(bank)) {
            entries.push(PointerEntry {
                pc_offset: i,
                target: SnesAddr::new(bank, addr),
            });
        }
    }

    entries
}

/// Dump pointer table entries.
pub fn print_pointers(entries: &[PointerEntry]) {
    for entry in entries {
        println!("  PC ${:06X} → {}", entry.pc_offset, entry.target);
    }
    println!("  Total: {} pointers", entries.len());
}

/// Rewrite pointers to redirect text from one bank to another.
/// `redirects` maps old_target → new_target.
#[allow(dead_code)]
pub fn rewrite_pointers(
    rom: &mut TrackedRom,
    entries: &[PointerEntry],
    redirects: &[(SnesAddr, SnesAddr)],
) -> usize {
    let mut rewritten = 0;
    for entry in entries {
        for (old, new) in redirects {
            if entry.target == *old {
                let pc = entry.pc_offset;
                rom.write(
                    pc,
                    &[new.addr as u8, (new.addr >> 8) as u8, new.bank],
                    "pointer:rewrite_3byte",
                );
                rewritten += 1;
                break;
            }
        }
    }
    rewritten
}

/// Rewrite 2-byte pointer table entries (within-bank, no bank byte).
///
/// Each table entry is a 16-bit LoROM offset. The implied bank is `pointer_bank`.
/// Only rewrites entries that match an old address in `redirects` AND whose
/// new address is in the same bank.
///
/// When `allow_sub_pointers` is true, also handles sub-pointers: entries that
/// point a small offset (1-8 bytes) into a relocated string. For example,
/// Bank $03's diary table has entries pointing past the chapter-number prefix
/// byte. When the base string is relocated from old_addr to new_addr, a
/// sub-pointer at old_addr+N becomes new_addr+N.
///
/// Sub-pointer matching must be disabled for tables whose entries are
/// independent pointers (e.g., Bank $01 $B37E save location names), since
/// adjacent entries within 8 bytes would be incorrectly rewritten.
pub fn rewrite_2byte_pointer_table(
    rom: &mut TrackedRom,
    table_pc: usize,
    entry_count: usize,
    pointer_bank: u8,
    redirects: &[(SnesAddr, SnesAddr)],
    allow_sub_pointers: bool,
) -> usize {
    let mut rewritten = 0;
    for i in 0..entry_count {
        let pc = table_pc + i * 2;
        if pc + 2 > rom.len() {
            break;
        }
        let offset = u16::from_le_bytes([rom[pc], rom[pc + 1]]);
        let current = SnesAddr::new(pointer_bank, offset);

        // Try exact match first
        let mut matched = false;
        for (old, new) in redirects {
            if current == *old {
                if new.bank != pointer_bank {
                    continue;
                }
                rom.write(
                    pc,
                    &[new.addr as u8, (new.addr >> 8) as u8],
                    "pointer:rewrite_2byte",
                );
                rewritten += 1;
                matched = true;
                break;
            }
        }

        // Try sub-pointer match: current = old + small_delta
        if !matched && allow_sub_pointers {
            for (old, new) in redirects {
                if new.bank != pointer_bank || old.bank != pointer_bank {
                    continue;
                }
                if current.addr > old.addr && current.addr <= old.addr.saturating_add(8) {
                    let delta = current.addr - old.addr;
                    let new_addr = new.addr.wrapping_add(delta);
                    rom.write(
                        pc,
                        &[new_addr as u8, (new_addr >> 8) as u8],
                        "pointer:rewrite_2byte_sub",
                    );
                    rewritten += 1;
                    break;
                }
            }
        }
    }
    rewritten
}

/// Rewrite scattered 2-byte pointers at individual PC locations.
///
/// Unlike `rewrite_2byte_pointer_table` which operates on contiguous tables,
/// this handles pointers scattered in code (e.g., LDA #$XXXX immediates).
/// Each `pc` in `known_pcs` points to a 2-byte LE value (addr_lo, addr_hi).
/// Only rewrites within-bank redirects (new.bank == pointer_bank).
pub fn rewrite_scattered_2byte_ptrs(
    rom: &mut TrackedRom,
    known_pcs: &[usize],
    pointer_bank: u8,
    redirects: &[(SnesAddr, SnesAddr)],
) -> usize {
    let mut rewritten = 0;
    for &pc in known_pcs {
        if pc + 2 > rom.len() {
            continue;
        }
        let offset = u16::from_le_bytes([rom[pc], rom[pc + 1]]);
        let current = SnesAddr::new(pointer_bank, offset);
        for (old, new) in redirects {
            if current == *old && new.bank == pointer_bank {
                rom.write(
                    pc,
                    &[new.addr as u8, (new.addr >> 8) as u8],
                    "pointer:rewrite_scattered_2byte",
                );
                rewritten += 1;
                break;
            }
        }
    }
    rewritten
}

/// Rewrite 3-byte pointers at known PC locations (from EN RE catalog).
///
/// Only rewrites if the current value at `pc` matches an entry in `redirects`.
/// This is much safer than scan_pointers() which can produce false positives.
pub fn rewrite_at_known_pcs(
    rom: &mut TrackedRom,
    known_pcs: &[usize],
    redirects: &[(SnesAddr, SnesAddr)],
) -> usize {
    let mut rewritten = 0;
    for &pc in known_pcs {
        if pc + 3 > rom.len() {
            continue;
        }
        let current = SnesAddr::new(rom[pc + 2], u16::from_le_bytes([rom[pc], rom[pc + 1]]));
        for (old, new) in redirects {
            if current == *old {
                rom.write(
                    pc,
                    &[new.addr as u8, (new.addr >> 8) as u8, new.bank],
                    "pointer:rewrite_at_known",
                );
                rewritten += 1;
                break;
            }
        }
    }
    rewritten
}

#[cfg(test)]
#[path = "pointer_tests.rs"]
mod tests;
