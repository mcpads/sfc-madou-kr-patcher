//! Engine hooks for FA/F0 prefix character support.
//!
//! The original game engine only handles single-byte and FB-prefix characters.
//! Korean text uses 770 characters: 224 single + 256 FB + 256 FA + 34 F0.
//! These hooks extend the engine to recognize FA/F0 as 2-byte prefixes and
//! load their font tiles from Bank $32.
//!
//! Hook points:
//!   1. Tilemap writer ($02:$AAC0) — adds FA→page $02, F0→page $03
//!   2. Renderer ($02:$A9AA) — redirects page 2/3 tile loads to Bank $32

use crate::patch::asm::{assemble, Inst};
use crate::patch::tracked_rom::{Expect, TrackedRom};
use crate::rom::lorom_to_pc;

// ── Hook placement ────────────────────────────────────────────────
// Hooks are written to Bank $32 free space after FA/F0 font tiles.
const HOOK_BANK: u8 = 0x32;

// ── DMA parameters ──────────────────────────────────────────────
const VMAIN_WORD_INC: u8 = 0x80;
const DMA_XFER_SIZE: u8 = 0x40; // 64 bytes per DMA transfer
const DMA_ITERATIONS: u8 = 0x01; // 1 iteration per call
/// Combined dp$10/$11 value: transfer size (low) | iterations (high)
const DMA_SIZE_ITERS: u16 = ((DMA_ITERATIONS as u16) << 8) | DMA_XFER_SIZE as u16;
const VRAM_TILE_STRIDE: u16 = 0x0020;

// ── Text VRAM layout ────────────────────────────────────────────
const VRAM_TEXT_BASE: u16 = 0x6400;
const TILES_PER_LINE: u8 = 0x0A; // 10 tiles/line
const LINE2_START: u8 = TILES_PER_LINE; // slot 10
const LINE3_START: u8 = 2 * TILES_PER_LINE; // slot 20
const MAX_SLOT: u8 = 3 * TILES_PER_LINE; // slot 30

// ── Font bank offsets ───────────────────────────────────────────
const FONT_TILE_BASE: u16 = 0x8000; // Font tile data base address in ROM bank
const F0_TILE_OFFSET: u16 = 0xC000; // F0 prefix tiles at bank:$C000

// ── Original ROM patch points ─────────────────────────────────────
/// Tilemap writer: CMP #$FB / BEQ fb_path at $02:$AACB (4 bytes).
const TILEMAP_HOOK_PC: usize = lorom_to_pc(0x02, 0xAACB);
/// Renderer: TAY at $02:$A9AA (replaces TAY + loop setup + inner loop).
const RENDERER_HOOK_PC: usize = lorom_to_pc(0x02, 0xA9AA);
/// Renderer inner loop end: BNE at $02:$A9BD (fill to here with NOPs).
const RENDERER_LOOP_END_PC: usize = lorom_to_pc(0x02, 0xA9BE);

// ── SNES long addresses for JML targets ───────────────────────────
/// Original single-byte path: STA $000A,Y at $02:$AACF
const TILEMAP_SINGLE_PATH: u32 = 0x02_AACF;
/// Original FB-prefix path: INX at $02:$AAD9
const TILEMAP_FB_PATH: u32 = 0x02_AAD9;
/// Tilemap "next" entry: INY×2, INX, DEC, BNE at $02:$AAE5
const TILEMAP_NEXT: u32 = 0x02_AAE5;
/// Renderer post-loop: INC dp$0C × 2 at $02:$A9BF
const RENDERER_POST_LOOP: u32 = 0x02_A9BF;

// ── In-game dialogue engine hooks ($00:$CCxx) ────────────────────
//
// The in-game text engine at $00:$CC8B reads text directly from ROM,
// dispatches char codes $F8-$FF as control codes via a pointer table
// at $00:$CCCF (3-byte entries, indexed by char - $F8).
//
// Original behavior:
//   F0: treated as normal char (garbage tile at $0F:$BC00)
//   FA: control code index 2 → stores dp$1E to $0009,X, no rendering
//   FB: control code index 3 → sets $11D6=$4000, renders from $0F:$C000
//
// Hook strategy:
//   1. Dispatch hook at $00:$CCA3: intercept F0 before the F8 threshold
//   2. FA table entry at $00:$CCD5: redirect to our FA prefix handler
//   3. Bank check at $00:$CECD: override dp$0B from $0F to $32

/// Char dispatch hook patch point: CMP #$F8 at $00:$CCA3 (7 bytes).
const INGAME_DISPATCH_PC: usize = lorom_to_pc(0x00, 0xCCA3);
/// Original normal char path (delay check): $00:$CCAA
const INGAME_NORMAL_PATH: u32 = 0x00_CCAA;
/// Original control code dispatch: $00:$CCE7
const INGAME_CONTROL_DISPATCH: u32 = 0x00_CCE7;
/// Normal rendering entry: $00:$CE9E
const INGAME_RENDER_ENTRY: u32 = 0x00_CE9E;

/// FA control code table entry at $00:$CCD5. Kept for reference only:
/// FA is no longer used as a prefix byte (F1 replaces it to avoid control code range).
/// The original table entry is left unmodified.
#[allow(dead_code)]
const FA_TABLE_ENTRY_PC: usize = lorom_to_pc(0x00, 0xCCD5);

/// Bank override patch point: LDA #$0F / STA $0B at $00:$CECD (4 bytes).
const BANK_OVERRIDE_PC: usize = lorom_to_pc(0x00, 0xCECD);
/// Continue after bank setup: $00:$CED1
const BANK_OVERRIDE_CONTINUE: u32 = 0x00_CED1;

/// Unused RAM byte for bank override flag.
/// $1D70: zero absolute/long references in JP ROM (12-byte free block $1D70-$1D7B).
/// Previously $11D8, but the game uses that address (normal value $16).
const BANK_FLAG_ADDR: u16 = 0x1D70;
/// Text buffer offset variable (used by FB handler for $4000 offset).
const TILE_OFFSET_ADDR: u16 = 0x11D6;
/// Text position counter.
const TEXT_POS_ADDR: u16 = 0x1186;

// ── Bank $03 independent text pipeline hooks ──────────────────────
//
// Bank $03 has its own text rendering pipeline at $8B76 that bypasses
// the main engine at $CC8B. It only recognizes FB as a 2-byte prefix;
// F1/F0 are treated as normal 1-byte chars → garbage tiles from Bank $0F.
//
// Hook strategy:
//   1. Dispatch hook at $03:$8B9F: intercept F1/F0 before normal char path
//   2. Bank check at $03:$8C3C: override dp$0B from $0F to $32

/// Bank $03 dispatcher: CMP #$FC / BEQ / JMP at $03:$8B9F (7 bytes).
const BANK03_DISPATCH_PC: usize = lorom_to_pc(0x03, 0x8B9F);
/// Bank $03 bank override: LDA #$0F / STA $0B at $03:$8C3C (4 bytes).
const BANK03_BANK_OVERRIDE_PC: usize = lorom_to_pc(0x03, 0x8C3C);

/// FC page-break handler in Bank $03.
const BANK03_FC_PATH: u32 = 0x03_8BBF;
/// Normal (single-byte) char handler in Bank $03.
const BANK03_NORMAL_CHAR: u32 = 0x03_8C10;
/// FB handler's INC×2 + SEP + JSR $8C2C (shared with F1/F0).
const BANK03_FB_ADVANCE: u32 = 0x03_8C02;
/// DMA call entry: JSL $008C8D at $03:$8C40.
const BANK03_DMA_CALL: u32 = 0x03_8C40;

// ── Block 0 VRAM clear hook ($03:$8CA0) ──────────────────────────
//
// the game's dynamic clear routine at $03:$8CA0 only
// clears JP-specific tile positions using state table parameters
// ($0006,X width, $0007,X rows, $001A,X VRAM offset). The F1/F0
// hooks change tile consumption (JP 2→KR 1 per char), shifting all
// tilemap positions and leaving stale KR tiles in Block 0.
//
// This hook replaces the dynamic clear with a fixed full clear of
// rows 0-15 ($7800~$79E0), using the game's own 64-byte zero buffer
// at $03:$8E3E as DMA source. The original routine at $8CA0-$8CE3
// becomes dead code after the JML.

/// Block 0 clear hook patch point: start of dynamic clear at $03:$8CA0 (4 bytes for JML).
const BLOCK0_CLEAR_HOOK_PC: usize = lorom_to_pc(0x03, 0x8CA0);
/// Continue after hook: $03:$8CE4 (code after original clear routine).
const BLOCK0_CLEAR_CONTINUE: u32 = 0x03_8CE4;
/// DMA queue scheduler at $00:$8BF9.
const DMA_SCHEDULER: u32 = 0x00_8BF9;
/// VRAM word address for Block 0 row 0 (byte $F000).
const BLOCK0_VRAM_START: u16 = 0x7800;
/// Number of rows to clear (rows 0-15).
const BLOCK0_ROW_COUNT: u8 = 16;
/// Game's own 64-byte zero buffer in Bank $03 ROM (confirmed all $00).
const GAME_ZERO_BUF_BANK: u8 = 0x03;
const GAME_ZERO_BUF_ADDR: u16 = 0x8E3E;

/// Generate the in-game char dispatch hook + F0/F1 handler.
///
/// Replaces CMP #$F8 / BCC / JMP at $00:$CCA3 (7 bytes → JML + 3 NOP).
/// Checks for F0 and F1 prefixes before the F8 control code threshold.
/// F1 replaces FA to avoid the control code range ($F8-$FF).
///
/// For control codes >= F8:
///   FA/FB/FC → original dispatch (no VRAM clear needed)
///   F8/F9/FD/FE/FF → clear_and_dispatch (clear remaining VRAM tiles first)
///
/// Entry: 8-bit A = char code (from dp$0B).
fn build_ingame_dispatch_hook(clear_and_dispatch_addr: u32) -> Result<Vec<u8>, String> {
    use Inst::*;
    let program = vec![
        // Check F0 prefix (tiles at Bank $32:$C000)
        CmpImm8(0xF0),
        Beq("f0_setup"),
        // Check F1 prefix (tiles at Bank $32:$8000, replaces old FA)
        CmpImm8(0xF1),
        Beq("f1_setup"),
        // Check F8 threshold (original behavior)
        CmpImm8(0xF8),
        Bcs("control"),
        // Normal char path
        Jml(INGAME_NORMAL_PATH),
        // Control code dispatch (F8-FF)
        Label("control"),
        // FA/FB/FC don't end a line — dispatch directly (no VRAM clear)
        CmpImm8(0xFA),
        Beq("no_clear"),
        CmpImm8(0xFB),
        Beq("no_clear"),
        CmpImm8(0xFC),
        Beq("no_clear"),
        // F8/F9/FD/FE/FF end a line — clear remaining VRAM tiles first
        Jml(clear_and_dispatch_addr),
        Label("no_clear"),
        Jml(INGAME_CONTROL_DISPATCH),
        // F0: offset $4000 → Bank $32:$C000
        Label("f0_setup"),
        Rep(0x20),        // 16-bit A
        LdaImm16(0x4000), // offset $4000
        Bra("shared_prefix"),
        // F1: offset $0000 → Bank $32:$8000
        Label("f1_setup"),
        Rep(0x20),        // 16-bit A
        LdaImm16(0x0000), // offset $0000
        // Shared prefix handler
        Label("shared_prefix"),
        StaAbs(TILE_OFFSET_ADDR), // STA $11D6
        IncAbs(TEXT_POS_ADDR),    // INC $1186 (advance past prefix)
        Sep(0x20),                // 8-bit A
        LdaImm8(0x32),            // Bank $32
        StaAbs(BANK_FLAG_ADDR),   // STA $11D8 (bank override flag)
        LdaDp(0x1E),              // LDA dp$1E (index byte)
        Jml(INGAME_RENDER_ENTRY), // JML $00:$CE9E
    ];
    assemble(&program).map_err(|e| format!("ingame dispatch hook assembly failed: {}", e))
}

/// Generate the in-game FA prefix handler.
///
/// Replaces the FA entry in the control code table at $00:$CCD5.
/// Mimics FB handler but uses Bank $32:$8000 (offset 0).
///
/// Entry: dp$1E = index byte (pre-fetched at $00:$CC99).
fn build_ingame_fa_handler() -> Result<Vec<u8>, String> {
    use Inst::*;
    let program = vec![
        Rep(0x20),                // 16-bit A
        LdaImm16(0x0000),         // no offset (base $8000)
        StaAbs(TILE_OFFSET_ADDR), // STA $11D6
        IncAbs(TEXT_POS_ADDR),    // INC $1186 (advance past FA)
        Sep(0x20),                // 8-bit A
        LdaImm8(0x32),            // Bank $32
        StaAbs(BANK_FLAG_ADDR),   // STA $11D8 (bank override flag)
        LdaDp(0x1E),              // LDA dp$1E (index byte)
        Jml(INGAME_RENDER_ENTRY), // JML $00:$CE9E
    ];
    assemble(&program).map_err(|e| format!("ingame FA handler assembly failed: {}", e))
}

/// Generate a bank check hook (shared pattern for in-game and Bank $03).
///
/// Checks `BANK_FLAG_ADDR` for exactly $32. If matched, uses Bank $32
/// and clears the flag; otherwise falls through to default Bank $0F.
/// The only difference between call sites is the JML continue address.
fn build_bank_check_hook(continue_addr: u32, label: &str) -> Result<Vec<u8>, String> {
    use Inst::*;
    let program = vec![
        LdaAbs(BANK_FLAG_ADDR), // LDA $1D70
        CmpImm8(0x32),          // CMP #$32 — exact match only
        Beq("override"),        // if $32, use Bank $32
        // Default: use Bank $0F
        LdaImm8(0x0F),      // LDA #$0F (original)
        StaDp(0x0B),        // STA dp$0B
        Jml(continue_addr), // JML continue
        // Override: use Bank $32
        Label("override"),
        StaDp(0x0B),            // STA dp$0B (A = $32)
        StzAbs(BANK_FLAG_ADDR), // STZ $1D70 (clear flag)
        Jml(continue_addr),     // JML continue
    ];
    assemble(&program).map_err(|e| format!("{} assembly failed: {}", label, e))
}

/// Generate the in-game bank override hook.
///
/// Replaces LDA #$0F / STA $0B at $00:$CECD (exactly 4 bytes → JML).
fn build_ingame_bank_check() -> Result<Vec<u8>, String> {
    build_bank_check_hook(BANK_OVERRIDE_CONTINUE, "ingame bank check")
}

/// Generate the Bank $03 dispatch hook + F1/F0 prefix handlers.
///
/// Replaces CMP #$FC / BEQ / JMP at $03:$8B9F (7 bytes → JML + 3 NOP).
/// Routes FC to original handler, F1/F0 to inline prefix handlers,
/// and all other chars to the normal single-byte path.
///
/// F1/F0 handlers mirror the FB handler ($03:$8BEF): read dp$1E index,
/// compute tile offset with ASL×6, set $1D70 bank flag, then reuse
/// FB's INC×2 + JSR $8C2C path at $03:$8C02.
///
/// Entry: 8-bit A = char code (from dispatcher at $03:$8B93).
fn build_bank03_dispatch_hook() -> Result<Vec<u8>, String> {
    use Inst::*;
    let program = vec![
        // ── Dispatch (mirrors $03:$8B9F-$8BA5) ──
        CmpImm8(0xFC),
        Beq("fc_path"),
        CmpImm8(0xF1),
        Beq("f1_handler"),
        CmpImm8(0xF0),
        Beq("f0_handler"),
        Jml(BANK03_NORMAL_CHAR), // other chars → $03:$8C10
        Label("fc_path"),
        Jml(BANK03_FC_PATH), // FC → $03:$8BBF
        // ── F1 handler (tiles at Bank $32:$8000) ──
        Label("f1_handler"),
        LdaDp(0x1E),      // index byte (pre-fetched)
        Rep(0x20),        // 16-bit A
        AndImm16(0x00FF), // mask high byte
        AslA,
        AslA,
        AslA,
        AslA,
        AslA,
        AslA, // ASL A × 6
        Clc,
        AdcImm16(FONT_TILE_BASE), // ADC #$8000
        Bra("shared"),
        // ── F0 handler (tiles at Bank $32:$C000) ──
        Label("f0_handler"),
        LdaDp(0x1E),
        Rep(0x20),
        AndImm16(0x00FF),
        AslA,
        AslA,
        AslA,
        AslA,
        AslA,
        AslA, // ASL A × 6
        Clc,
        AdcImm16(F0_TILE_OFFSET), // ADC #$C000
        // ── Shared: store offset, set bank flag, reuse FB advance path ──
        Label("shared"),
        StaDp(0x0C),            // tile source offset
        Sep(0x20),              // 8-bit A
        LdaImm8(0x32),          // Bank $32
        StaAbs(BANK_FLAG_ADDR), // set bank override flag ($1D70)
        Rep(0x20),              // 16-bit (INC $000B,X at $8C02 needs M=0)
        Jml(BANK03_FB_ADVANCE), // → $03:$8C02 (INC×2 + JSR $8C2C)
    ];
    assemble(&program).map_err(|e| format!("bank03 dispatch hook assembly failed: {}", e))
}

/// Generate the Bank $03 bank check hook.
///
/// Replaces LDA #$0F / STA $0B at $03:$8C3C (exactly 4 bytes → JML).
/// Entry: 8-bit A, inside tile renderer subroutine ($03:$8C2C).
fn build_bank03_bank_check() -> Result<Vec<u8>, String> {
    build_bank_check_hook(BANK03_DMA_CALL, "bank03 bank check")
}

/// Generate the Block 0 VRAM clear hook .
///
/// Replaces the game's dynamic clear routine at $03:$8CA0 (JML, 4 bytes).
/// The original routine uses state table parameters to clear only JP-specific
/// positions; this hook clears 16 full rows ($7800~$79E0) unconditionally
/// using the game's own zero buffer at $03:$8E3E.
///
/// Entry: M=8, X = state table index (preserved via PHX/PLX).
/// Exit: JML $03:$8CE4 (code after original clear routine).
fn build_block0_clear_hook() -> Result<Vec<u8>, String> {
    use Inst::*;
    let program = vec![
        // === Guard: skip if game says no clear ($0007,X == 0) ===
        LdaAbsX(0x0007),
        Bne("do_clear"),
        Jml(BLOCK0_CLEAR_CONTINUE), // skip — original also does nothing
        Label("do_clear"),
        // === Save X ($008BF9 destroys X) ===
        Phx,
        // === DMA parameter setup (M=8 on entry) ===
        LdaImm8(GAME_ZERO_BUF_BANK), // LDA #$03
        StaDp(0x0B),                 // source bank = Bank $03
        LdaImm8(VMAIN_WORD_INC),
        StaDp(0x1E), // DMA mode = $80 (VMAIN word inc)
        // === 16-bit parameter setup ===
        Rep(0x20),                    // M=16
        LdaImm16(GAME_ZERO_BUF_ADDR), // $8E3E (game's zero buffer)
        StaDp(0x0C),                  // source offset
        LdaImm16(DMA_SIZE_ITERS),     // dp$10=$40(64B), dp$11=$01(1 iter)
        StaDp(0x10),
        LdaImm16(BLOCK0_VRAM_START), // $7800
        StaDp(0x0E),                 // VRAM dest = row 0
        // === Loop: clear 16 rows ($7800~$79E0) ===
        Sep(0x20),                 // M=8
        LdaImm8(BLOCK0_ROW_COUNT), // 16
        Label("loop"),
        Pha,                // save counter
        Jsl(DMA_SCHEDULER), // JSL $008BF9 (queue DMA)
        Rep(0x20),          // M=16
        LdaDp(0x0E),
        Clc,
        AdcImm16(VRAM_TILE_STRIDE), // ADC #$0020 (next row)
        StaDp(0x0E),
        Sep(0x20), // M=8
        Pla,       // restore counter
        DecA,
        Bne("loop"),
        // === Restore X + continue ===
        Plx,
        Jml(BLOCK0_CLEAR_CONTINUE), // JML $03:$8CE4
    ];
    assemble(&program).map_err(|e| format!("block0 clear hook assembly failed: {}", e))
}

/// Unused RAM byte for slot temp storage during VRAM clear.
/// $1D72: zero absolute/long references in JP ROM (within 12-byte free block $1D70-$1D7B).
const SLOT_TEMP_ADDR: u16 = 0x1D72;

/// Generate the VRAM tile clear hook for line-ending control codes.
///
/// Called before dispatching F8/F9/FD/FE/FF control codes. Clears
/// remaining VRAM tile slots on the current line (from current slot
/// to the next line boundary) using DMA zero fill.
///
/// This replaces text-level BLANK_RENDER padding: instead of padding
/// each line with $18 bytes, the engine zeroes unused VRAM tile slots
/// at the hardware level when a line ends.
///
/// IMPORTANT: The DMA setup clobbers dp registers ($0B, $0C, $0E, $10,
/// $1E) that the text engine uses for rendering state. All modified dp
/// values are saved to the stack before DMA and restored after.
///
/// Slot layout: 0-9 = line 1, 10-19 = line 2, 20-29 = line 3.
/// VRAM word address for slot S: $6400 + S * $20.
///
/// Entry: 8-bit M, A = control code (F8/F9/FD/FE/FF).
///        X = text state table index. $0007,X = current slot (0-29).
/// Exit: JML $00:$CCE7 (original control code dispatch).
fn build_clear_and_dispatch() -> Result<Vec<u8>, String> {
    use Inst::*;
    let program = vec![
        // === Save registers ===
        Pha, // save control code
        Phx, // save state table index; $008BF9 destroys X
        // === Read current slot ===
        LdaAbsX(0x0007), // LDA $0007,X
        // === Boundary check: skip if slot is at line boundary ===
        Beq("skip_exit"), // slot 0 → skip
        CmpImm8(LINE2_START),
        Beq("skip_exit"), // slot 10 → skip
        CmpImm8(LINE3_START),
        Beq("skip_exit"), // slot 20 → skip
        CmpImm8(MAX_SLOT),
        Bcs("skip_exit"), // slot >= 30 → skip
        Bra("do_clear"),  // not at boundary → clear
        // === Skip exit (nearby, within branch range) ===
        Label("skip_exit"),
        Plx,
        Pla,
        Jml(INGAME_CONTROL_DISPATCH),
        // === DMA clear path ===
        Label("do_clear"),
        // Save slot to temp RAM (dp regs will be clobbered)
        StaAbs(SLOT_TEMP_ADDR), // STA $1D72
        // Save dp registers used by DMA (text engine state)
        LdaDp(0x0B),
        Pha, // save dp$0B (bank byte)
        LdaDp(0x1E),
        Pha,       // save dp$1E (pre-fetched byte)
        Rep(0x20), // 16-bit
        LdaDp(0x0C),
        Pha, // save dp$0C-$0D (tile offset, 2 bytes)
        LdaDp(0x0E),
        Pha, // save dp$0E-$0F (renderer state, 2 bytes)
        LdaDp(0x10),
        Pha, // save dp$10-$11 (2 bytes)
        // === Compute VRAM address: $6400 + slot * $20 ===
        LdaAbs(SLOT_TEMP_ADDR), // reload slot (16-bit, high byte garbage)
        AndImm16(0x00FF),       // zero-extend
        AslA,
        AslA,
        AslA,
        AslA,
        AslA, // ASL A × 5 (= × 32)
        Clc,
        AdcImm16(VRAM_TEXT_BASE), // ADC #$6400
        StaDp(0x0E),              // VRAM dest
        // === DMA parameter setup ===
        Sep(0x20),                   // 8-bit
        LdaImm8(GAME_ZERO_BUF_BANK), // #$03
        StaDp(0x0B),                 // source bank
        LdaImm8(VMAIN_WORD_INC),
        StaDp(0x1E),                  // DMA mode = $80 (VMAIN word inc)
        Rep(0x20),                    // 16-bit
        LdaImm16(GAME_ZERO_BUF_ADDR), // $8E3E
        StaDp(0x0C),                  // source offset
        LdaImm16(DMA_SIZE_ITERS),     // dp$10=64B, dp$11=1 iter
        StaDp(0x10),
        Sep(0x20), // 8-bit
        // === Compute remaining slots: 10 - (slot MOD 10) ===
        LdaAbs(SLOT_TEMP_ADDR), // reload slot
        CmpImm8(LINE3_START),
        Bcs("sub20"),
        CmpImm8(LINE2_START),
        Bcs("sub10"),
        Bra("got_mod"), // slot 1-9: mod = slot
        Label("sub20"),
        SbcImm8(LINE3_START), // SBC #$14 (carry set from CMP)
        Bra("got_mod"),
        Label("sub10"),
        SbcImm8(LINE2_START), // SBC #$0A (carry set from CMP)
        Label("got_mod"),
        // A = slot MOD 10 (1-9, boundary cases already skipped)
        // remaining = 10 - A
        EorImm8(0xFF),
        Sec,
        AdcImm8(TILES_PER_LINE), // ADC #$0A (= 10 - mod)
        // === DMA loop: clear remaining tile slots ===
        Label("dma_loop"),
        Pha,                // save counter
        Jsl(DMA_SCHEDULER), // JSL $008BF9
        Rep(0x20),          // 16-bit
        LdaDp(0x0E),
        Clc,
        AdcImm16(VRAM_TILE_STRIDE), // ADC #$0020 (next tile slot)
        StaDp(0x0E),
        Sep(0x20), // 8-bit
        Pla,       // restore counter
        DecA,
        Bne("dma_loop"),
        // === Restore dp registers (reverse order) ===
        Rep(0x20), // 16-bit
        Pla,
        StaDp(0x10), // restore dp$10-$11
        Pla,
        StaDp(0x0E), // restore dp$0E-$0F
        Pla,
        StaDp(0x0C), // restore dp$0C-$0D
        Sep(0x20),   // 8-bit
        Pla,
        StaDp(0x1E), // restore dp$1E
        Pla,
        StaDp(0x0B), // restore dp$0B
        // === Round slot to next line boundary ===
        // Prevents repeated DMA when FE (Separator) re-reads:
        //   FE handler RTLs without advancing text pointer while waiting
        //   for player input → text engine re-reads FE next frame →
        //   our hook runs again. Without rounding, the boundary check
        //   fails every frame, causing DMA clears that starve OAM DMA.
        Plx, // restore X = state table index
        LdaAbs(SLOT_TEMP_ADDR),
        CmpImm8(LINE3_START),
        Bcs("round_l3"),
        CmpImm8(LINE2_START),
        Bcs("round_l2"),
        LdaImm8(LINE2_START), // slot 1-9 → 10
        Bra("store_rnd"),
        Label("round_l3"),
        LdaImm8(MAX_SLOT), // slot 20-29 → 30
        Bra("store_rnd"),
        Label("round_l2"),
        LdaImm8(LINE3_START), // slot 10-19 → 20
        Label("store_rnd"),
        StaAbsX(0x0007), // update slot counter
        Pla,             // restore control code
        Jml(INGAME_CONTROL_DISPATCH),
    ];
    assemble(&program).map_err(|e| format!("clear_and_dispatch assembly failed: {}", e))
}

// ── Blank tile fix ($02:$AA92) ───────────────────────────────────
//
// The save-menu tilemap initializer fills all 12 character slots
// with LDA #$01F4 (tile $F4, page $01 = FB prefix tile $F4).
// In the original JP ROM, this tile is blank (all zeros).
// In the KO font, FB $F4 has a Korean glyph → stale "혀" artifacts.
//
// Fix: change the blank tile value from $01F4 to $0000.
// Tile $00 page $00 = fixed-encode tile $00 = blank in both JP/KO.

/// Blank tile init: LDA #$01F4 at $02:$AA92 (3 bytes: A9 F4 01).
const BLANK_TILE_PC: usize = lorom_to_pc(0x02, 0xAA92);

/// Generate the tilemap writer hook.
///
/// Replaces `CMP #$FB / BEQ fb_path` at $02:$AACB.
/// Dispatches: FB→original, F1→page $02, F0→page $03, other→single.
///
/// Entry: 8-bit A = char byte. X = source buffer index, Y = dest index.
fn build_tilemap_hook() -> Result<Vec<u8>, String> {
    use Inst::*;
    let program = vec![
        // Check FB (original behavior)
        CmpImm8(0xFB),
        Beq("jmp_fb"),
        // Check F1 (replaces old FA, avoids control code range)
        CmpImm8(0xF1),
        Beq("fa"),
        // Check F0
        CmpImm8(0xF0),
        Beq("f0"),
        // Single byte — original path
        Jml(TILEMAP_SINGLE_PATH),
        // FB — original path
        Label("jmp_fb"),
        Jml(TILEMAP_FB_PATH),
        // FA prefix: read index, set page $02
        Label("fa"),
        Inx,
        LdaAbsX(0x0000), // LDA $0000,X
        StaAbsY(0x000A), // STA $000A,Y
        LdaImm8(0x02),
        Bra("store_page"),
        // F0 prefix: read index, set page $03
        Label("f0"),
        Inx,
        LdaAbsX(0x0000), // LDA $0000,X
        StaAbsY(0x000A), // STA $000A,Y
        LdaImm8(0x03),
        // Shared: store page byte and continue
        Label("store_page"),
        StaAbsY(0x000B), // STA $000B,Y
        Jml(TILEMAP_NEXT),
    ];
    assemble(&program).map_err(|e| format!("tilemap hook assembly failed: {}", e))
}

/// Generate the renderer hook.
///
/// Replaces TAY + inner loop at $02:$A9AA.
/// For page 0/1 (Y < $8000): reads tiles from Bank $0F (DB already set).
/// For page 2/3 (Y >= $8000): switches DB to $32, adjusts Y, reads tiles.
///
/// Entry: 16-bit A = tile offset (after ASL×6). X = WRAM dest offset.
/// DB = $0F.
fn build_renderer_hook() -> Result<Vec<u8>, String> {
    use Inst::*;
    let program = vec![
        // A = tile offset from ASL×6
        Tay,             // Y = tile offset
        Bmi("extended"), // Y >= $8000 → FA/F0 tiles
        // ── Normal path (page 0/1, DB=$0F) ──
        LdaImm16(0x0020),
        StaDp(0x0E), // dp$0E = loop counter (32 words)
        Label("loop_normal"),
        LdaAbsY(FONT_TILE_BASE), // LDA $8000,Y
        StaLongX(0x7F0000),      // STA $7F0000,X
        Iny,
        Iny,
        Inx,
        Inx,
        DecDp(0x0E),
        Bne("loop_normal"),
        Jml(RENDERER_POST_LOOP),
        // ── Extended path (page 2/3, switch to Bank $32) ──
        Label("extended"),
        Phb,       // save DB ($0F)
        Sep(0x20), // 8-bit A
        LdaImm8(0x32),
        Pha,
        Plb,       // DB = $32
        Rep(0x20), // 16-bit A
        Tya,
        AndImm16(0x7FFF), // strip bit 15 (Y' = Y & $7FFF)
        Tay,
        LdaImm16(0x0020),
        StaDp(0x0E),
        Label("loop_ext"),
        LdaAbsY(FONT_TILE_BASE), // LDA $8000,Y (now in Bank $32)
        StaLongX(0x7F0000),      // STA $7F0000,X
        Iny,
        Iny,
        Inx,
        Inx,
        DecDp(0x0E),
        Bne("loop_ext"),
        Plb, // restore DB = $0F
        Jml(RENDERER_POST_LOOP),
    ];
    assemble(&program).map_err(|e| format!("renderer hook assembly failed: {}", e))
}

/// Helper: build a 4-byte JML instruction for a 24-bit SNES address.
fn jml_bytes(addr: u32) -> [u8; 4] {
    [0x5C, addr as u8, (addr >> 8) as u8, (addr >> 16) as u8]
}

/// Apply engine hooks to the ROM.
///
/// Installs two sets of hooks:
///   A. Save-menu hooks ($02:$AACB tilemap, $02:$A9AA renderer)
///   B. In-game dialogue hooks ($00:$CCA3 dispatch, $00:$CCD5 FA table,
///      $00:$CECD bank override)
///
/// All hook code is placed in Bank $32 free space starting at `hook_base`.
/// Returns the SNES address immediately after the last hook byte.
pub fn apply_hooks(rom: &mut TrackedRom, hook_base: u16) -> Result<u16, String> {
    // ── Build hook code (order matters: dispatch depends on clear_dispatch addr) ──
    let tilemap_code = build_tilemap_hook()?;
    let renderer_code = build_renderer_hook()?;
    let clear_dispatch_code = build_clear_and_dispatch()?;

    // Calculate addresses for hooks built so far to determine clear_dispatch_addr
    let mut next_addr = hook_base;

    let tilemap_addr = next_addr;
    next_addr += tilemap_code.len() as u16;

    let renderer_addr = next_addr;
    next_addr += renderer_code.len() as u16;

    let clear_dispatch_addr = next_addr;
    next_addr += clear_dispatch_code.len() as u16;

    // Now build dispatch hook with the known clear_dispatch address
    let clear_dispatch_long = ((HOOK_BANK as u32) << 16) | (clear_dispatch_addr as u32);
    let dispatch_code = build_ingame_dispatch_hook(clear_dispatch_long)?;

    let dispatch_addr = next_addr;
    next_addr += dispatch_code.len() as u16;

    // Build remaining hooks (no address dependencies)
    let fa_handler_code = build_ingame_fa_handler()?;
    let fa_handler_addr = next_addr;
    next_addr += fa_handler_code.len() as u16;

    let bank_check_code = build_ingame_bank_check()?;
    let bank_check_addr = next_addr;
    next_addr += bank_check_code.len() as u16;

    let bank03_dispatch_code = build_bank03_dispatch_hook()?;
    let bank03_dispatch_addr = next_addr;
    next_addr += bank03_dispatch_code.len() as u16;

    let bank03_bank_check_code = build_bank03_bank_check()?;
    let bank03_bank_check_addr = next_addr;
    next_addr += bank03_bank_check_code.len() as u16;

    // Block 0 VRAM clear hook ($03:$8CA0)
    let block0_clear_code = build_block0_clear_hook()?;
    let block0_clear_addr = next_addr;
    next_addr += block0_clear_code.len() as u16;

    let hooks_end = next_addr;

    println!("\n--- Applying engine hooks (FA/F0 + blank tile fix + VRAM clear) ---");
    println!("  [Save-menu hooks]");
    println!(
        "    Tilemap hook: {} bytes at ${:02X}:${:04X}",
        tilemap_code.len(),
        HOOK_BANK,
        tilemap_addr
    );
    println!(
        "    Renderer hook: {} bytes at ${:02X}:${:04X}",
        renderer_code.len(),
        HOOK_BANK,
        renderer_addr
    );
    println!("  [In-game dialogue hooks]");
    println!(
        "    VRAM clear+dispatch: {} bytes at ${:02X}:${:04X}",
        clear_dispatch_code.len(),
        HOOK_BANK,
        clear_dispatch_addr
    );
    println!(
        "    Dispatch hook: {} bytes at ${:02X}:${:04X}",
        dispatch_code.len(),
        HOOK_BANK,
        dispatch_addr
    );
    println!(
        "    FA handler: {} bytes at ${:02X}:${:04X}",
        fa_handler_code.len(),
        HOOK_BANK,
        fa_handler_addr
    );
    println!(
        "    Bank check: {} bytes at ${:02X}:${:04X}",
        bank_check_code.len(),
        HOOK_BANK,
        bank_check_addr
    );
    println!("  [Bank $03 battle text hooks]");
    println!(
        "    Dispatch+handler: {} bytes at ${:02X}:${:04X}",
        bank03_dispatch_code.len(),
        HOOK_BANK,
        bank03_dispatch_addr
    );
    println!(
        "    Bank check: {} bytes at ${:02X}:${:04X}",
        bank03_bank_check_code.len(),
        HOOK_BANK,
        bank03_bank_check_addr
    );
    println!("  [Block 0 VRAM clear ($03:$8CA0)]");
    println!(
        "    Clear hook: {} bytes at ${:02X}:${:04X}",
        block0_clear_code.len(),
        HOOK_BANK,
        block0_clear_addr
    );
    println!(
        "    Zero buffer: game ROM at $03:${:04X} (64B)",
        GAME_ZERO_BUF_ADDR
    );
    println!(
        "  Hooks end: ${:02X}:${:04X} ({} bytes total)",
        HOOK_BANK,
        hooks_end,
        (hooks_end - hook_base)
    );

    // Bank boundary check
    if (hooks_end as u32) > 0xFFFF {
        return Err(format!(
            "Hook code exceeds Bank ${:02X} boundary: end=${:04X}",
            HOOK_BANK, hooks_end
        ));
    }

    // ── Write all hook code to Bank $32 ──────────────────────────
    let total_hook_bytes = (hooks_end - hook_base) as usize;
    {
        let hook_pc = lorom_to_pc(HOOK_BANK, hook_base);
        let mut region = rom.region_expect(
            hook_pc,
            total_hook_bytes,
            "engine:hook_code",
            &Expect::FreeSpace(0xFF),
        );
        let chunks: &[(u16, &[u8])] = &[
            (tilemap_addr, &tilemap_code),
            (renderer_addr, &renderer_code),
            (clear_dispatch_addr, &clear_dispatch_code),
            (dispatch_addr, &dispatch_code),
            (fa_handler_addr, &fa_handler_code),
            (bank_check_addr, &bank_check_code),
            (bank03_dispatch_addr, &bank03_dispatch_code),
            (bank03_bank_check_addr, &bank03_bank_check_code),
            (block0_clear_addr, &block0_clear_code),
        ];
        for &(addr, code) in chunks {
            let offset = (addr - hook_base) as usize;
            region.copy_at(offset, code);
        }
    }

    // ── Patch save-menu hooks ────────────────────────────────────
    // Blank tile fix: change LDA #$01F4 → LDA #$0000 at $02:$AA92
    // Original: A9 F4 01 → New: A9 00 00
    // FB $F4 tile has a Korean glyph; tile $00 page $00 is blank.
    rom.write_expect(
        BLANK_TILE_PC + 1,
        &[0x00, 0x00],
        "engine:blank_tile_fix",
        &Expect::Bytes(&[0xF4, 0x01]),
    );
    println!("  Patched $02:$AA92: LDA #$01F4 → LDA #$0000 (blank tile fix)");

    // Tilemap writer: replace CMP #$FB / BEQ at $02:$AACB with JML
    let tilemap_long = ((HOOK_BANK as u32) << 16) | (tilemap_addr as u32);
    rom.write_expect(
        TILEMAP_HOOK_PC,
        &jml_bytes(tilemap_long),
        "engine:tilemap_jml",
        &Expect::Bytes(&[0xC9, 0xFB]),
    );
    println!("  Patched $02:$AACB: JML ${:06X}", tilemap_long);

    // Renderer: replace TAY at $02:$A9AA with JML + NOP fill
    let renderer_long = ((HOOK_BANK as u32) << 16) | (renderer_addr as u32);
    {
        let total = RENDERER_LOOP_END_PC - RENDERER_HOOK_PC;
        let mut r = rom.region_expect(
            RENDERER_HOOK_PC,
            total,
            "engine:renderer_jml",
            &Expect::Bytes(&[0xA8]),
        );
        r.copy_at(0, &jml_bytes(renderer_long));
        r.data_mut()[4..].fill(0xEA);
    }
    println!(
        "  Patched $02:$A9AA: JML ${:06X} (+{} NOP fill)",
        renderer_long,
        RENDERER_LOOP_END_PC - RENDERER_HOOK_PC - 4
    );

    // ── Patch in-game dialogue hooks ─────────────────────────────
    // 1. Dispatch: replace CMP #$F8 / BCC / JMP at $00:$CCA3 (7 bytes)
    let dispatch_long = ((HOOK_BANK as u32) << 16) | (dispatch_addr as u32);
    {
        let mut r = rom.region_expect(
            INGAME_DISPATCH_PC,
            7,
            "engine:ingame_dispatch_jml",
            &Expect::Bytes(&[0xC9, 0xF8]),
        );
        r.copy_at(0, &jml_bytes(dispatch_long));
        r.data_mut()[4..].fill(0xEA);
    }
    println!("  Patched $00:$CCA3: JML ${:06X} (+3 NOP)", dispatch_long);

    // 2. FA table entry: NO LONGER PATCHED.
    // FA ($FA) is no longer used as a prefix byte. F1 ($F1) is used instead,
    // handled by the dispatch hook above (F1 < F8, so it never reaches the
    // control code table). FA reverts to its original game behavior.
    // The FA handler code is still in Bank $32 but won't be called.
    println!("  FA table entry at $00:$CCD5: left as original (FA no longer used as prefix)");

    // 3. Bank override: replace LDA #$0F / STA $0B at $00:$CECD (4 bytes)
    let bank_long = ((HOOK_BANK as u32) << 16) | (bank_check_addr as u32);
    rom.write_expect(
        BANK_OVERRIDE_PC,
        &jml_bytes(bank_long),
        "engine:bank_override_jml",
        &Expect::Bytes(&[0xA9, 0x0F, 0x85, 0x0B]),
    );
    println!("  Patched $00:$CECD: JML ${:06X}", bank_long);

    // ── Patch Bank $03 battle text hooks ───────────────────────
    // 1. Dispatch: replace CMP #$FC / BEQ / JMP at $03:$8B9F (7 bytes)
    let bank03_dispatch_long = ((HOOK_BANK as u32) << 16) | (bank03_dispatch_addr as u32);
    {
        let mut r = rom.region_expect(
            BANK03_DISPATCH_PC,
            7,
            "engine:bank03_dispatch_jml",
            &Expect::Bytes(&[0xC9, 0xFC]),
        );
        r.copy_at(0, &jml_bytes(bank03_dispatch_long));
        r.data_mut()[4..].fill(0xEA);
    }
    println!(
        "  Patched $03:$8B9F: JML ${:06X} (+3 NOP)",
        bank03_dispatch_long
    );

    // 2. Bank override: replace LDA #$0F / STA $0B at $03:$8C3C (4 bytes)
    let bank03_bank_long = ((HOOK_BANK as u32) << 16) | (bank03_bank_check_addr as u32);
    rom.write_expect(
        BANK03_BANK_OVERRIDE_PC,
        &jml_bytes(bank03_bank_long),
        "engine:bank03_bank_override_jml",
        &Expect::Bytes(&[0xA9, 0x0F, 0x85, 0x0B]),
    );
    println!("  Patched $03:$8C3C: JML ${:06X}", bank03_bank_long);

    // ── Patch Block 0 VRAM clear hook  ───────────────
    // Dynamic clear at $03:$8CA0 (4 bytes → JML, rest is dead code)
    let block0_long = ((HOOK_BANK as u32) << 16) | (block0_clear_addr as u32);
    rom.write_expect(
        BLOCK0_CLEAR_HOOK_PC,
        &jml_bytes(block0_long),
        "engine:block0_clear_jml",
        &Expect::Bytes(&[0xC2, 0x21, 0xBD, 0x1A]),
    );
    println!("  Patched $03:$8CA0: JML ${:06X}", block0_long);

    // ── Hook address map summary ──────────────────────────────────
    println!("  ┌─────────────────────────────────────────────────────────┐");
    println!(
        "  │ Engine hook address map (Bank ${:02X})                     │",
        HOOK_BANK
    );
    println!("  ├──────────────┬────────────────────────────┬────────────┤");
    println!("  │ Patch site   │ Target                     │ Size       │");
    println!("  ├──────────────┼────────────────────────────┼────────────┤");
    println!(
        "  │ $02:$AACB    │ ${:02X}:${:04X} tilemap         │ {:>3} bytes  │",
        HOOK_BANK,
        tilemap_addr,
        tilemap_code.len()
    );
    println!(
        "  │ $02:$A9AA    │ ${:02X}:${:04X} renderer        │ {:>3} bytes  │",
        HOOK_BANK,
        renderer_addr,
        renderer_code.len()
    );
    println!(
        "  │ (internal)   │ ${:02X}:${:04X} clear+dispatch  │ {:>3} bytes  │",
        HOOK_BANK,
        clear_dispatch_addr,
        clear_dispatch_code.len()
    );
    println!(
        "  │ $00:$CCA3    │ ${:02X}:${:04X} ingame dispatch │ {:>3} bytes  │",
        HOOK_BANK,
        dispatch_addr,
        dispatch_code.len()
    );
    println!(
        "  │ $00:$CECD    │ ${:02X}:${:04X} bank override   │ {:>3} bytes  │",
        HOOK_BANK,
        bank_check_addr,
        bank_check_code.len()
    );
    println!(
        "  │ $03:$8B9F    │ ${:02X}:${:04X} bank03 dispatch │ {:>3} bytes  │",
        HOOK_BANK,
        bank03_dispatch_addr,
        bank03_dispatch_code.len()
    );
    println!(
        "  │ $03:$8C3C    │ ${:02X}:${:04X} bank03 bank chk │ {:>3} bytes  │",
        HOOK_BANK,
        bank03_bank_check_addr,
        bank03_bank_check_code.len()
    );
    println!(
        "  │ $03:$8CA0    │ ${:02X}:${:04X} block0 clear    │ {:>3} bytes  │",
        HOOK_BANK,
        block0_clear_addr,
        block0_clear_code.len()
    );
    println!("  ├──────────────┴────────────────────────────┴────────────┤");
    println!(
        "  │ Total: ${:02X}:${:04X}-${:04X} ({} bytes)                    │",
        HOOK_BANK,
        hook_base,
        hooks_end,
        hooks_end - hook_base
    );
    println!("  └─────────────────────────────────────────────────────────┘");

    Ok(hooks_end)
}

#[cfg(test)]
#[path = "engine_hooks_tests.rs"]
mod tests;
