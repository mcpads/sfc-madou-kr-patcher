//! Shop screen OAM sprite localization .
//!
//! Replaces JP OAM text sprites on the shop screen:
//! - "うる" speech bubble → "판매" (2 × 8x8 4bpp tiles at $50A0)
//! - "クッキー" → "쿠키" + blank (3 × 8x8 4bpp tiles at $5170)
//! - "うりきれ" sold-out → "판매완료" at tiles $0A/$1A/$1B/$0E
//!
//! All sprites use Palette 7, fg_color = $01.
//!
//! ## Sold-out tile $0A — no conflict
//!
//! JP "うりきれ" first char uses tile $0A, shared with speech "うる".
//! "판매완료" starts with "판" — same char as speech "판매" — so tile $0A
//! content is naturally shared. No game code patch needed.
//!
//! ## Hook Architecture
//!
//! The shop screen's OBJ CHR loading follows a two-phase process:
//!
//! 1. **Phase 1** ($0294D8): Table-driven LZ decompress loop fills
//!    WRAM $7F regions, then 4 DMA blocks copy data to VRAM:
//!    - Block 2: $7F:$D000 (size $3000) → VRAM $4800-$5FFF
//!      (includes speech tiles at $50A0/$50B0 and cookie at $5170-$5190)
//!
//! 2. **Phase 2** ($0281AB at $8ADA): JSL $009440 decompresses nameplate
//!    data to $7F:$A000, then DMA copies to VRAM $5800-$5FFF
//!    (overwrites part of Phase 1 for the nameplate area only).
//!
//! Since speech/cookie/sold-out tiles (VRAM $50A0-$51BF) are loaded in
//! Phase 1 and NOT overwritten by Phase 2, we hook Phase 2's JSL $009440
//! at $02:$8ADA and use **ROM→VRAM direct DMA** (during force blank) to
//! overlay KO tiles after Phase 1 completes.
//!
//! The screen is still in force blank during initialization, so direct
//! VRAM writes via DMA Channel 6 are safe.

use crate::font_gen;
use crate::patch::asm::{assemble, Inst};
use crate::patch::hook_common::JSL_LZ_BYTES;
use crate::patch::tracked_rom::{Expect, TrackedRom};
use crate::rom::lorom_to_pc;

// ── Bank / address constants ─────────────────────────────────────

/// ROM bank for tile data + hook code (shared with equip_oam in Bank $25).
const DATA_BANK: u8 = 0x25;

/// Start SNES address within Bank $25 (after equip_oam: $C8A2 + 1550 = $CEB0 → $CF00).
const DATA_ADDR: u16 = 0xCF00;

/// Total tile data: speech 64B + sold-out4 32B + cookie 96B + sold-out23 64B = 256B.
const TILE_DATA_SIZE: usize = 256;

// ── Hook site ────────────────────────────────────────────────────

/// JSL $009440 at $02:$8ADA (Phase 2 nameplate decompress).
/// We intercept this to inject VRAM DMA for speech/cookie tiles.
const HOOK_SITE_PC: usize = 0x10ADA; // $02:$8ADA

/// Nameplate LZ source identification (common across ALL shop types).
/// dp$0B=$29 (bank), dp$0C=$FB (addr lo), dp$0D=$93 (addr hi)
/// → LZ source $29:$93FB. OBJ tile source varies per shop type
/// (e.g. $26:$EEB1, $26:$6Bxx, $27:$B1xx, $28:$2Exx), so we
/// identify the shop by the constant nameplate call instead.
const NAMEPLATE_DP0B: u8 = 0x29;
const NAMEPLATE_DP0C: u8 = 0xFB;
const NAMEPLATE_DP0D: u8 = 0x93;

/// WRAM scratch byte for 2-stage shop detection.
/// Stage 1 (nameplate call): set to $01.
/// Stage 2 (next call = OBJ tiles): detected, DMA overlay, clear to $00.
const SHOP_FLAG_WRAM: u16 = 0x1F60;

// ── Palette ──────────────────────────────────────────────────────

/// Foreground (stroke) color index within Palette 7 for speech text.
const SPEECH_FG: u8 = 1;

/// Foreground (stroke) color index within Palette 7 for cookie text.
/// Foreground stroke color = $01.
const COOKIE_FG: u8 = 1;

/// Foreground (stroke) color index within Palette 7 for sold-out text.
const SOLDOUT_FG: u8 = 1;

// ── KO characters ────────────────────────────────────────────────

/// "판매" speech bubble — replaces "うる" (2 × 8x8, Palette 7).
const SPEECH_CHARS: &[char] = &['판', '매'];

/// "쿠키" cookie — replaces "クッキー" (2 × 8x8 + 1 blank, Palette 7).
const COOKIE_CHARS: &[char] = &['쿠', '키'];

/// "판매완료" sold-out — replaces "うりきれ" (4 × 8x8, Palette 7).
/// First char "판" shares tile $0A with speech — no conflict.
/// Remaining "매완료" go to tiles $1A/$1B/$0E.
const SOLDOUT_CHARS: &[char] = &['매', '완', '료'];

// ── VRAM addresses ───────────────────────────────────────────────
//
// Phase 1 DMA: $7F:$D000 (size $3000) → VRAM $4800-$5FFF.
// Speech/cookie tiles are within this range.

/// VRAM word address for 1st speech tile (판).
const SPEECH_VRAM: u16 = 0x50A0;

/// Speech DMA size: 2 tiles × 32B = 64B.
const SPEECH_DMA_SIZE: u16 = 64;

/// VRAM word address for sold-out 4th char 료 (tile $0E).
const SOLDOUT4_VRAM: u16 = 0x50E0;

/// Sold-out 4th char DMA size: 1 tile × 32B = 32B.
const SOLDOUT4_DMA_SIZE: u16 = 32;

/// VRAM word address for 1st cookie tile (blank).
const COOKIE_VRAM: u16 = 0x5170;

/// Cookie DMA size: 3 tiles × 32B = 96B.
const COOKIE_DMA_SIZE: u16 = 96;

/// VRAM word address for sold-out tiles 2+3: 매($1A) + 완($1B).
const SOLDOUT23_VRAM: u16 = 0x51A0;

/// Sold-out 2+3 DMA size: 2 tiles × 32B = 64B.
const SOLDOUT23_DMA_SIZE: u16 = 64;

// ── Tile data generation ─────────────────────────────────────────

/// Build KO tile data: speech (64B) + 료 (32B) + cookie (96B) + 매완 (64B) = 256B.
///
/// ROM layout:
/// 1. 판+매 (speech, 64B) → DMA to $50A0
/// 2. 료 (sold-out 4th, 32B) → DMA to $50E0
/// 3. blank+쿠+키 (cookie, 96B) → DMA to $5170
/// 4. 매+완 (sold-out 2+3, 64B) → DMA to $51A0
fn build_shop_tile_data(ttf_data: &[u8], ttf_size: f32) -> Result<Vec<u8>, String> {
    let speech_tiles =
        font_gen::render_oam_8x8_4bpp_tiles(ttf_data, ttf_size, SPEECH_CHARS, SPEECH_FG, 0)?;
    let cookie_tiles =
        font_gen::render_oam_8x8_4bpp_tiles(ttf_data, ttf_size, COOKIE_CHARS, COOKIE_FG, 0)?;
    let soldout_tiles =
        font_gen::render_oam_8x8_4bpp_tiles(ttf_data, ttf_size, SOLDOUT_CHARS, SOLDOUT_FG, 0)?;

    let mut data = Vec::with_capacity(TILE_DATA_SIZE);

    // Speech: 판(32B) + 매(32B) = 64B
    for tile in &speech_tiles {
        data.extend_from_slice(tile);
    }

    // Sold-out 4th char: 료(32B) → DMA to VRAM $50E0 (tile $0E)
    data.extend_from_slice(&soldout_tiles[2]); // 료

    // Cookie: blank(32B) + 쿠(32B) + 키(32B) = 96B
    data.extend_from_slice(&[0u8; 32]); // blank 1st tile
    for tile in &cookie_tiles {
        data.extend_from_slice(tile);
    }

    // Sold-out 2+3: 매(32B) + 완(32B) = 64B → DMA to VRAM $51A0
    data.extend_from_slice(&soldout_tiles[0]); // 매
    data.extend_from_slice(&soldout_tiles[1]); // 완

    debug_assert_eq!(data.len(), TILE_DATA_SIZE);
    Ok(data)
}

// ── Hook code generation ─────────────────────────────────────────

/// Build hook ASM: 2-stage WRAM flag guard for shop-agnostic OBJ tile overlay.
///
/// Uses DMA Channel 6 to copy KO tile data directly from ROM to VRAM.
/// The screen is in force blank during shop initialization, so VRAM
/// writes are safe.
///
/// ## 2-Stage Guard
///
/// Shop entry fires the hook at $02:$8ADA at least twice:
/// 1. **Nameplate** (dp$0B=$29, $0C=$FB, $0D=$93) — same across all shops
/// 2. **OBJ tiles** (dp values vary per shop type) — overwrites VRAM $50A0/$5170
///
/// KO DMA must happen AFTER the OBJ tile call (stage 2), but the OBJ tile
/// dp values vary per shop. Solution: use WRAM $1F60 as a flag.
///
/// - Stage 1: nameplate dp match → set flag, decompress, return
/// - Stage 2: flag set → clear flag, decompress, DMA overlay, return
/// - Otherwise: just decompress and return
fn build_shop_hook_code(data_bank: u8, data_base: u16) -> Result<Vec<u8>, String> {
    use Inst::*;

    let speech_addr = data_base;
    let soldout4_addr = data_base + SPEECH_DMA_SIZE;
    let cookie_addr = soldout4_addr + SOLDOUT4_DMA_SIZE;
    let soldout23_addr = cookie_addr + COOKIE_DMA_SIZE;

    // NOTE: just_decompress is placed BEFORE the DMA section to keep
    // the BNE("do_dma") branch within ±127 byte range. With 4 DMA
    // transfers the code exceeds BEQ/BNE range if just_decompress
    // is at the end.
    let program = vec![
        // ── Stage 1: Check if nameplate call (dp$0B=$29, $0C=$FB, $0D=$93) ──
        // Caller state: M=1 (8-bit A) — no REP needed for byte comparisons
        LdaDp(0x0B),
        CmpImm8(NAMEPLATE_DP0B),
        Bne("check_flag"),
        LdaDp(0x0C),
        CmpImm8(NAMEPLATE_DP0C),
        Bne("check_flag"),
        LdaDp(0x0D),
        CmpImm8(NAMEPLATE_DP0D),
        Bne("check_flag"),
        // Nameplate match → set WRAM flag, decompress, return
        LdaImm8(0x01),
        StaAbs(SHOP_FLAG_WRAM),
        Jsl(0x009440),
        Rtl,
        // ── Stage 2: Check WRAM flag for OBJ tile call ──
        Label("check_flag"),
        LdaAbs(SHOP_FLAG_WRAM),
        Bne("do_dma"),
        // ── Non-shop path: just decompress and return ──
        Jsl(0x009440),
        Rtl,
        // ── DMA overlay path ──
        Label("do_dma"),
        StzAbs(SHOP_FLAG_WRAM), // Clear flag
        Jsl(0x009440),          // Decompress OBJ tiles (varies per shop)
        // Now overlay KO tiles via ROM→VRAM DMA
        Php,
        Phb,
        Sep(0x20), // 8-bit A
        // Force blank + VRAM increment mode
        LdaImm8(0x80),
        StaAbs(0x2100), // Force blank (safety — likely already blanked)
        LdaImm8(0x80),
        StaAbs(0x2115), // VRAM auto-increment by word
        // DMA Ch6 common setup
        LdaImm8(0x01),
        StaAbs(0x4360), // Mode 1 (2-reg write: $2118/$2119)
        LdaImm8(0x18),
        StaAbs(0x4361), // B-bus = $2118 (VRAM data low)
        LdaImm8(data_bank),
        StaAbs(0x4364), // A-bus bank = ROM tile data bank
        // --- DMA 1: Speech (판+매, 64B) → VRAM $50A0 ---
        Rep(0x20), // 16-bit A
        LdaImm16(SPEECH_VRAM),
        StaAbs(0x2116), // VRAM word address
        LdaImm16(speech_addr),
        StaAbs(0x4362), // A-bus source address
        LdaImm16(SPEECH_DMA_SIZE),
        StaAbs(0x4365), // Transfer size
        Sep(0x20),      // 8-bit A
        StzAbs(0x420C), // Disable HDMA (prevent bus conflicts)
        LdaImm8(0x40),  // Channel 6 = bit 6
        StaAbs(0x420B), // Trigger DMA
        // --- DMA 2: Sold-out 료 (32B) → VRAM $50E0 ---
        Rep(0x20), // 16-bit A
        LdaImm16(SOLDOUT4_VRAM),
        StaAbs(0x2116), // VRAM word address
        LdaImm16(soldout4_addr),
        StaAbs(0x4362), // A-bus source address
        LdaImm16(SOLDOUT4_DMA_SIZE),
        StaAbs(0x4365), // Transfer size
        Sep(0x20),      // 8-bit A
        LdaImm8(0x40),  // Channel 6 = bit 6
        StaAbs(0x420B), // Trigger DMA
        // --- DMA 3: Cookie (blank+쿠+키, 96B) → VRAM $5170 ---
        Rep(0x20), // 16-bit A
        LdaImm16(COOKIE_VRAM),
        StaAbs(0x2116), // VRAM word address
        LdaImm16(cookie_addr),
        StaAbs(0x4362), // A-bus source address
        LdaImm16(COOKIE_DMA_SIZE),
        StaAbs(0x4365), // Transfer size
        Sep(0x20),      // 8-bit A
        LdaImm8(0x40),  // Channel 6 = bit 6
        StaAbs(0x420B), // Trigger DMA
        // --- DMA 4: Sold-out 매+완 (64B) → VRAM $51A0 ---
        Rep(0x20), // 16-bit A
        LdaImm16(SOLDOUT23_VRAM),
        StaAbs(0x2116), // VRAM word address
        LdaImm16(soldout23_addr),
        StaAbs(0x4362), // A-bus source address
        LdaImm16(SOLDOUT23_DMA_SIZE),
        StaAbs(0x4365), // Transfer size
        Sep(0x20),      // 8-bit A
        LdaImm8(0x40),  // Channel 6 = bit 6
        StaAbs(0x420B), // Trigger DMA
        // Restore state and return
        Plb,
        Plp,
        Rtl,
    ];

    assemble(&program)
}

// ── Public API ───────────────────────────────────────────────────

/// Apply shop OAM sprite hook .
///
/// Replaces speech bubble ("うる" → "판매"), cookie ("クッキー" → "쿠키"),
/// and sold-out ("うりきれ" → "판매완료") using ROM→VRAM direct DMA.
/// Sold-out first char "판" naturally shares tile $0A with speech.
pub fn apply_shop_oam_hook(
    rom: &mut TrackedRom,
    ttf_data: &[u8],
    ttf_size: f32,
) -> Result<(), String> {
    let tile_data = build_shop_tile_data(ttf_data, ttf_size)?;
    let hook_code = build_shop_hook_code(DATA_BANK, DATA_ADDR)?;

    // Write tile data + hook code to Bank $25
    let data_pc = lorom_to_pc(DATA_BANK, DATA_ADDR);
    let total_size = tile_data.len() + hook_code.len();
    {
        let mut r = rom.region_expect(
            data_pc,
            total_size,
            "shop_oam:data+code",
            &Expect::FreeSpace(0xFF),
        );
        r.copy_at(0, &tile_data);
        r.copy_at(tile_data.len(), &hook_code);
    }

    // Patch JSL $009440 site to jump to our hook
    let code_snes = DATA_ADDR + tile_data.len() as u16;
    let hook_long = (DATA_BANK as u32) << 16 | code_snes as u32;
    let jsl = [
        0x22u8,
        hook_long as u8,
        (hook_long >> 8) as u8,
        (hook_long >> 16) as u8,
    ];

    rom.write_expect(
        HOOK_SITE_PC,
        &jsl,
        "shop_oam:hook",
        &Expect::Bytes(&JSL_LZ_BYTES),
    );

    println!(
        "  Shop OAM : {}B tiles + {}B code @ ${:02X}:${:04X}-${:04X}",
        tile_data.len(),
        hook_code.len(),
        DATA_BANK,
        DATA_ADDR,
        DATA_ADDR + total_size as u16 - 1,
    );
    println!(
        "    Hook: $02:$8ADA → ${:02X}:${:04X} (ROM→VRAM DMA Ch6 ×4)",
        DATA_BANK, code_snes,
    );
    println!("    Speech (판매): VRAM ${:04X}", SPEECH_VRAM);
    println!("    Sold-out 료: VRAM ${:04X} (tile $0E)", SOLDOUT4_VRAM);
    println!("    Cookie (쿠키): VRAM ${:04X}", COOKIE_VRAM);
    println!("    Sold-out 매완: VRAM ${:04X}", SOLDOUT23_VRAM);
    println!("    Sold-out 1st '판' = speech tile $0A (shared, no conflict)");

    Ok(())
}

#[cfg(test)]
#[path = "shop_oam_tests.rs"]
mod tests;
