//! Choice highlight width fix (Issue V).
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

use crate::patch::asm::{compile_fixed_machine_code, ExecutionMode, Inst, MachineCode};
use crate::patch::tracked_rom::{Expect, TrackedRom};

/// Full line highlight width: 10 chars × 2 tiles × 2 bytes = 40 = $28.
const FULL_LINE_SIZE: u8 = 0x28;

/// Top-size patch: replaces `LDA dp$1E; LSR; LSR; AND #$3C; PHA; STA dp$10`
/// (9 bytes) with `LDA #$28; NOP×4; PHA; STA dp$10`.
const TOP_SIZE_ORIGINAL: [u8; 9] = [0xA5, 0x1E, 0x4A, 0x4A, 0x29, 0x3C, 0x48, 0x85, 0x10];

#[cfg(test)]
fn top_size_patch() -> [u8; 9] {
    compile_top_size_patch(0xDE7B)
        .bytes()
        .try_into()
        .expect("top patch has checked size")
}

fn compile_top_size_patch(addr: u16) -> MachineCode {
    compile_fixed_machine_code::<9>(
        vec![
            Inst::LdaImm8(FULL_LINE_SIZE),
            Inst::Nop,
            Inst::Nop,
            Inst::Nop,
            Inst::Nop,
            Inst::Pha,
            Inst::StaDp(0x10),
        ],
        0x01,
        addr,
        ExecutionMode::M8X16,
    )
    .expect("top choice-highlight patch assembly failed")
}

/// Bottom-size patch: replaces `PLA; AND #$0F; ASL; ASL; PHA; STA dp$10`
/// (8 bytes) with `PLA; LDA #$28; NOP×2; PHA; STA dp$10`.
const BOTTOM_SIZE_ORIGINAL: [u8; 8] = [0x68, 0x29, 0x0F, 0x0A, 0x0A, 0x48, 0x85, 0x10];

#[cfg(test)]
fn bottom_size_patch() -> [u8; 8] {
    compile_bottom_size_patch(0xDEC3)
        .bytes()
        .try_into()
        .expect("bottom patch has checked size")
}

fn compile_bottom_size_patch(addr: u16) -> MachineCode {
    compile_fixed_machine_code::<8>(
        vec![
            Inst::Pla,
            Inst::LdaImm8(FULL_LINE_SIZE),
            Inst::Nop,
            Inst::Nop,
            Inst::Pha,
            Inst::StaDp(0x10),
        ],
        0x01,
        addr,
        ExecutionMode::M8X16,
    )
    .expect("bottom choice-highlight patch assembly failed")
}

/// Apply choice highlight width patches to the ROM.
pub fn apply_choice_highlight_fix(rom: &mut TrackedRom) -> Result<(), String> {
    println!("\n--- Patching choice highlight width (Issue V) ---");

    let patches: [(u16, &[u8], bool, &str); 4] = [
        (0xDE7B, &TOP_SIZE_ORIGINAL, true, "top_size func1"),
        (0xDEC3, &BOTTOM_SIZE_ORIGINAL, false, "bottom_size func1"),
        (0xDF0E, &TOP_SIZE_ORIGINAL, true, "top_size func2"),
        (0xDF56, &BOTTOM_SIZE_ORIGINAL, false, "bottom_size func2"),
    ];

    for &(addr, original, is_top, desc) in &patches {
        let replacement = if is_top {
            compile_top_size_patch(addr)
        } else {
            compile_bottom_size_patch(addr)
        };
        rom.write_machine_code_expect(
            &replacement,
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

    println!(
        "  Highlight width: ${:02X} bytes (full line)",
        FULL_LINE_SIZE
    );
    Ok(())
}

#[cfg(test)]
#[path = "choice_highlight_tests.rs"]
mod tests;
