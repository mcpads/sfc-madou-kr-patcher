//! Battle dialog box width/height dynamic hook .
//!
//! The battle script command $48 hardcodes width/height for each text box.
//! KO translations may have different character counts than JP originals.
//! This hook scans the KO text at runtime and sets width/height to match
//! the actual KO text dimensions exactly.
//!
//! Additionally, when the KO text is wider and the dialog box's right edge
//! would exceed the screen boundary, display_params is shifted left to clamp
//! to the screen edge. JP convention: `col + width×2 ≈ 29` for right-side
//! dialogs; each width unit = 2 tilemap columns (16px = 1 KO character).
//!
//! Hook site: $03:$9D84 (12 bytes: LDA/STA ×2 for slot+$06 and slot+$08)
//! Hook code: Bank $03:$FB00+ (~140 bytes)

use crate::patch::asm::{assemble, Inst};
use crate::patch::tracked_rom::{Expect, TrackedRom};
use crate::rom::lorom_to_pc;

// ── Constants ────────────────────────────────────────────────────────

const HOOK_BANK: u8 = 0x03;
/// Hook code placement in Bank $03 free space (after encyclopedia hooks).
const HOOK_BASE: u16 = 0xFB00;

/// Maximum width value the scanner will return.
/// 30 characters — well beyond any battle dialog line, prevents 8-bit overflow.
const MAX_WIDTH: u8 = 0x1E;

/// Right-edge column limit for screen-boundary clamping.
/// JP convention: `col + width×2 ≈ 29` for right-side dialogs (flag=1).
/// Tilemap has 32 columns (0-31); frame borders add ~2 columns past the text.
/// Setting limit to 30 avoids altering boxes that already fit in JP.
const RIGHT_LIMIT: u8 = 30;

/// Hook site: $03:$9D84 (PC offset).
const HOOK_SITE_PC: usize = lorom_to_pc(HOOK_BANK, 0x9D84);
/// Original bytes at hook site (LDA $0001,Y + STA $0006,X + LDA $0003,Y + STA $0008,X).
const HOOK_SITE_ORIGINAL: [u8; 12] = [
    0xB9, 0x01, 0x00, // LDA $0001,Y (width|height)
    0x9D, 0x06, 0x00, // STA $0006,X (→ slot+$06)
    0xB9, 0x03, 0x00, // LDA $0003,Y (display_params)
    0x9D, 0x08, 0x00, // STA $0008,X (→ slot+$08)
];

// ── dp register assignments ──────────────────────────────────────────
// After $00:$966C slot allocation, dp$0B-$10 are free to reuse.
const DP_PTR: u8 = 0x0B; // dp$0B-$0D: 24-bit text pointer
const DP_MAX_W: u8 = 0x0E; // dp$0E: max_width across lines
const DP_CUR_W: u8 = 0x0F; // dp$0F: current line width
const DP_HEIGHT: u8 = 0x10; // dp$10: line count

/// Build the scan hook ASM code.
///
/// Entry: M=0, X=0 (16-bit A/X/Y), X=slot base, Y=script ptr, DBR=script bank.
/// Exit:  M=0, X=0, X=slot base, Y=script ptr, slot+6 and slot+8 written. RTL.
///
/// Command $48 parameter layout (10 bytes, script bank = $13):
///   Y+0    cmd ($48)
///   Y+1    width           → slot+$06 lo
///   Y+2    height          → slot+$06 hi
///   Y+3,4  display params  → slot+$08,$09
///   Y+5    display flag    → slot+$0A
///   Y+6,7  text addr lo/hi → slot+$0B,$0C
///   Y+8    text bank       → slot+$0D
///   Y+9    extra param     → slot+$18
pub fn build_scan_hook() -> Vec<u8> {
    use Inst::*;
    let program = vec![
        // ── Save registers ──
        Phx, // save slot base (16-bit)
        // ── Set up 24-bit text pointer from Y+6/Y+7/Y+8 ──
        LdaAbsY(0x0006),   // text addr lo/hi (16-bit, DBR=script bank)
        StaDp(DP_PTR),     // dp$0B-$0C = text addr
        Sep(0x20),         // M=1, 8-bit A
        LdaAbsY(0x0008),   // text bank (8-bit)
        StaDp(DP_PTR + 2), // dp$0D = text bank
        // ── Save script pointer, init scan ──
        Phy,              // save Y = script ptr (16-bit)
        LdyImm16(0x0000), // Y = scan offset
        StzDp(DP_MAX_W),  // max_width = 0
        StzDp(DP_CUR_W),  // cur_width = 0
        LdaImm8(0x01),
        StaDp(DP_HEIGHT), // height = 1
        // ── Scan loop ──
        Label("scan"),
        LdaDpIndirectLongY(DP_PTR), // read text byte ($B7 $0B)
        // Dispatch: $00-$EF=char, $F0-$F1=prefix, $F2-$F7=char, $F8=done,
        //           $F9=NL, $FA-$FB=prefix, $FC=box ctrl (skip 2),
        //           $FD=choice (skip 1), $FE-$FF=done
        CmpImm8(0xF0),
        Bcc("char"), // $00-$EF → normal char
        CmpImm8(0xF2),
        Bcc("prefix"), // $F0-$F1 → prefix
        CmpImm8(0xF8),
        Bcc("char"), // $F2-$F7 → normal char (shouldn't occur)
        CmpImm8(0xF9),
        Bcc("done"), // $F8 → page break (terminator)
        Beq("nl"),   // $F9 → newline
        CmpImm8(0xFC),
        Bcc("prefix"), // $FA-$FB → prefix
        Beq("fc_skip"), // $FC → box/speaker control (2 bytes, skip)
        CmpImm8(0xFE),
        Bcc("fd_skip"), // $FD → choice marker (1 byte, 0 width)
        Bra("done"),    // $FE-$FF → terminators
        // ── Normal character (1 byte, 1 width unit) ──
        Label("char"),
        IncDp(DP_CUR_W),
        Iny,
        Bra("width_check"),
        // ── Prefix character (2 bytes, 1 width unit) ──
        Label("prefix"),
        IncDp(DP_CUR_W),
        Iny,
        Iny,
        // fall through to width_check
        // ── Width cap guard (prevents 8-bit overflow) ──
        Label("width_check"),
        LdaDp(DP_CUR_W),
        CmpImm8(MAX_WIDTH),
        Bcs("done"),   // cur_width >= MAX → stop scanning
        Bra("scan"),
        // ── FC box/speaker control (2 bytes, 0 width) ──
        Label("fc_skip"),
        Iny,
        Iny,
        Bra("scan"),
        // ── FD choice marker (1 byte, 0 width) ──
        Label("fd_skip"),
        Iny,
        Bra("scan"),
        // ── Newline ──
        Label("nl"),
        LdaDp(DP_CUR_W), // cur_width
        CmpDp(DP_MAX_W), // vs max_width
        Bcc("nl_skip"),  // if cur < max, skip
        StaDp(DP_MAX_W), // max_width = cur_width
        Label("nl_skip"),
        StzDp(DP_CUR_W),  // reset cur_width
        IncDp(DP_HEIGHT), // height++
        Iny,
        Bra("scan"),
        // ── End of text ──
        Label("done"),
        LdaDp(DP_CUR_W), // last line's width
        CmpDp(DP_MAX_W),
        Bcc("done_skip"),
        StaDp(DP_MAX_W),
        Label("done_skip"),
        // ── Restore Y, compose width|height, write slot+$06 ──
        Ply, // Y = script pointer (16-bit)
        // Compose result: low=width (char count), high=height
        LdaDp(DP_HEIGHT), // A = ko height (M=1, 8-bit)
        Xba,              // → B
        LdaDp(DP_MAX_W),  // A = ko width (char count)
        Rep(0x20),        // 16-bit A = (height<<8)|width
        Plx,              // restore slot base
        StaAbsX(0x0006),  // slot+$06 = width|height
        // ── Screen-boundary clamping for display_params ──
        // JP convention: col + width×2 ≈ 29 for right-side dialogs.
        // Each width unit = 2 tilemap columns (16px = 1 KO char).
        // If col + ko_width*2 > RIGHT_LIMIT, shift dp left by excess.
        // dp$0B-$0D free (text scan done), dp$0E=ko_width, dp$0F/dp$10 free.
        LdaAbsY(0x0003),   // original display_params (16-bit, M=0)
        StaDp(DP_PTR),      // dp$0B:$0C = dp_orig
        Sep(0x20),          // M=1, 8-bit A
        LdaDp(DP_PTR),      // A = dp_orig lo byte
        AndImm8(0x1F),      // column = dp_orig & 31 (tilemap 32 cols/row)
        Clc,
        AdcDp(DP_MAX_W),    // col + ko_width (max 31+30=61, no carry)
        AdcDp(DP_MAX_W),    // col + ko_width*2 = right_edge (max 91, no carry)
        CmpImm8(RIGHT_LIMIT + 1), // right_edge > RIGHT_LIMIT?
        Bcc("no_shift"),    // ≤ RIGHT_LIMIT → copy dp unchanged
        // ── Shift left by excess ──
        Sec,
        SbcImm8(RIGHT_LIMIT), // excess = right_edge - RIGHT_LIMIT
        StaDp(DP_PTR + 2),    // dp$0D = excess (lo)
        StzDp(DP_MAX_W),      // dp$0E = 0 (zero-extend to 16-bit)
        Rep(0x20),             // 16-bit A
        LdaDp(DP_PTR),        // dp_orig (16-bit)
        Sec,
        SbcDp(DP_PTR + 2),    // dp_orig - excess (16-bit subtract)
        StaAbsX(0x0008),      // slot+$08 = adjusted dp
        Rtl,
        // ── No shift needed ──
        Label("no_shift"),
        Rep(0x20),             // 16-bit A
        LdaDp(DP_PTR),        // dp_orig unchanged
        StaAbsX(0x0008),      // slot+$08
        Rtl,
    ];

    assemble(&program).expect("battle width hook assembly failed")
}

/// Build the 12-byte hook site patch (JSL + 8×NOP).
///
/// Replaces both LDA/STA pairs at `$9D84`-`$9D8F`.
/// The hook handles both slot+$06 and slot+$08 writes internally.
pub fn build_hook_site_patch(hook_addr: u16) -> Vec<u8> {
    let long_addr = (HOOK_BANK as u32) << 16 | hook_addr as u32;
    vec![
        0x22, // JSL
        long_addr as u8,
        (long_addr >> 8) as u8,
        (long_addr >> 16) as u8,
        0xEA, 0xEA, 0xEA, 0xEA, // NOP ×4
        0xEA, 0xEA, 0xEA, 0xEA, // NOP ×4
    ]
}

/// Apply the battle width/height hook to the ROM.
pub fn apply_battle_width_hook(rom: &mut TrackedRom) -> Result<(), String> {
    let hook_code = build_scan_hook();
    let hook_size = hook_code.len();

    println!("\n--- Applying battle width hook  ---");
    println!(
        "  Hook code: {} bytes at ${:02X}:${:04X}",
        hook_size, HOOK_BANK, HOOK_BASE
    );

    // Sanity: hook must fit before $FFFF
    let end_addr = HOOK_BASE as usize + hook_size;
    if end_addr > 0x10000 {
        return Err(format!(
            "Battle width hook exceeds bank boundary: ${:04X} + {} = ${:X}",
            HOOK_BASE, hook_size, end_addr
        ));
    }

    // Write hook code to Bank $03 free space
    let hook_pc = lorom_to_pc(HOOK_BANK, HOOK_BASE);
    rom.region_expect(
        hook_pc,
        hook_size,
        "battle_width:hook_code",
        &Expect::FreeSpace(0xFF),
    )
    .copy_at(0, &hook_code);

    // Patch hook site: replace LDA+STA with JSL+NOP+NOP
    let site_patch = build_hook_site_patch(HOOK_BASE);
    rom.write_expect(
        HOOK_SITE_PC,
        &site_patch,
        "battle_width:hook_site",
        &Expect::Bytes(&HOOK_SITE_ORIGINAL),
    );

    println!(
        "  Hook site: ${:02X}:${:04X} (12 bytes: JSL ${:02X}:${:04X} + NOP×8)",
        HOOK_BANK, 0x9D84, HOOK_BANK, HOOK_BASE
    );
    println!("  display_params: screen-boundary clamping (RIGHT_LIMIT={})", RIGHT_LIMIT);

    Ok(())
}

#[cfg(test)]
#[path = "battle_width_tests.rs"]
mod tests;
