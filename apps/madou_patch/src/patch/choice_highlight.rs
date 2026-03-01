//! Choice highlight width fix .
//!
//! The original game highlights the focused choice line by DMA'ing tilemap
//! entries with palette 1 from ROM to VRAM.  The DMA size is derived from
//! a packed byte `dp$1E` using `(dp$1E >> 2) & $3C`, which encodes the
//! JP text width.  KO text is wider, so the highlight doesn't cover the
//! full line.
//!
//! Fix: replace the size computation with a fixed full-line width ($28 =
//! 40 bytes = 20 tilemap entries = 10 characters × 2 tiles/char).
//! The ROM highlight data already covers all 10 character positions per
//! line, so expanding the size is safe.
//!
//! Patch sites in Bank $01 (two functions, 4 patches total):
//!
//! | Site | Original bytes | Replacement | Purpose |
//! |------|---------------|-------------|---------|
//! | $DE7B | A5 1E 4A 4A 29 3C 48 85 10 | A9 28 EA EA EA EA 48 85 10 | top_size (func 1) |
//! | $DEC3 | 68 29 0F 0A 0A 48 85 10 | 68 A9 28 EA EA 48 85 10 | bottom_size (func 1) |
//! | $DF0E | A5 1E 4A 4A 29 3C 48 85 10 | A9 28 EA EA EA EA 48 85 10 | top_size (func 2) |
//! | $DF56 | 68 29 0F 0A 0A 48 85 10 | 68 A9 28 EA EA 48 85 10 | bottom_size (func 2) |

use crate::patch::tracked_rom::{Expect, TrackedRom};
use crate::rom::lorom_to_pc;

/// Full line highlight width: 10 chars × 2 tiles × 2 bytes = 40 = $28.
const FULL_LINE_SIZE: u8 = 0x28;

const NOP: u8 = 0xEA;

/// Top-size patch: replaces `LDA dp$1E; LSR; LSR; AND #$3C; PHA; STA dp$10`
/// (9 bytes) with `LDA #$28; NOP×4; PHA; STA dp$10`.
const TOP_SIZE_ORIGINAL: [u8; 9] = [0xA5, 0x1E, 0x4A, 0x4A, 0x29, 0x3C, 0x48, 0x85, 0x10];

fn top_size_patch() -> [u8; 9] {
    [
        0xA9, FULL_LINE_SIZE, // LDA #$28
        NOP, NOP, NOP, NOP, // pad
        0x48,               // PHA
        0x85, 0x10,         // STA dp$10
    ]
}

/// Bottom-size patch: replaces `PLA; AND #$0F; ASL; ASL; PHA; STA dp$10`
/// (8 bytes) with `PLA; LDA #$28; NOP×2; PHA; STA dp$10`.
const BOTTOM_SIZE_ORIGINAL: [u8; 8] = [0x68, 0x29, 0x0F, 0x0A, 0x0A, 0x48, 0x85, 0x10];

fn bottom_size_patch() -> [u8; 8] {
    [
        0x68,               // PLA  (stack balance: pop caller's value)
        0xA9, FULL_LINE_SIZE, // LDA #$28
        NOP, NOP,           // pad
        0x48,               // PHA
        0x85, 0x10,         // STA dp$10
    ]
}

/// Apply choice highlight width patches to the ROM.
pub fn apply_choice_highlight_fix(rom: &mut TrackedRom) -> Result<(), String> {
    println!("\n--- Patching choice highlight width  ---");

    let top = top_size_patch();
    let bottom = bottom_size_patch();

    let patches: [(u16, &[u8], &[u8], &str); 4] = [
        (0xDE7B, &TOP_SIZE_ORIGINAL, &top, "top_size func1"),
        (0xDEC3, &BOTTOM_SIZE_ORIGINAL, &bottom, "bottom_size func1"),
        (0xDF0E, &TOP_SIZE_ORIGINAL, &top, "top_size func2"),
        (0xDF56, &BOTTOM_SIZE_ORIGINAL, &bottom, "bottom_size func2"),
    ];

    for &(addr, original, replacement, desc) in &patches {
        let pc = lorom_to_pc(0x01, addr);
        rom.write_expect(
            pc,
            replacement,
            &format!("choice_highlight:{}", desc),
            &Expect::Bytes(original),
        );
        println!(
            "  Patched $01:${:04X}: {} ({} bytes)",
            addr,
            desc,
            replacement.len()
        );
    }

    println!("  Highlight width: ${:02X} bytes (full line)", FULL_LINE_SIZE);
    Ok(())
}

#[cfg(test)]
#[path = "choice_highlight_tests.rs"]
mod tests;
