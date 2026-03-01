//! Equipment screen OAM sprite localization .
//!
//! Replaces JP OAM text sprites on the equipment/stat screen:
//! - "そうび" → "장비" (3 × 8x8 4bpp tiles, 3rd blank)
//! - 10 equipment names → KO equivalents (8x8 4bpp tiles in BL/BR of 16x16 sprites)
//!
//! All sprites use Palette 4, fg_color = $01.
//!
//! ## Hook Architecture
//!
//! The pause menu shows the equipment screen as its **default view**.
//! $167A stays $FF for equipment; only stats ($02) and options ($03)
//! change it. The initialization sequence:
//!
//! 1. **State 2** ($01:$A012): JSR $A034 does one-time WRAM→VRAM direct DMA
//!    (ALL OBJ tiles from $7F:$8000+ → VRAM OBJ region). After this, OBJ
//!    VRAM is NEVER refreshed — it retains State 2's data permanently.
//!
//! 2. **$00:$83FF**: JSL $009440 fires once during pause menu init (after
//!    State 2 OBJ DMA). This decompresses BG data for the equipment screen.
//!
//! Meanwhile, $00:$9625 runs every frame and decompresses JP OBJ tile data
//! from Bank $05 to WRAM $7F:$8xxx. But since no subsequent DMA copies this
//! to VRAM, per-frame WRAM patches (MVN) have no visible effect.
//!
//! **Solution**: Hook JSL $009440 at $00:$83FF and use **ROM→VRAM direct
//! DMA** (DMA Ch6 during force blank) to overlay KO tiles directly into
//! OBJ VRAM. This fires on every pause menu entry, covering equipment
//! (default), stats, and all other sub-screens via VRAM persistence.
//!
//! ## VRAM Layout (16x16 OBJ sprite sub-tiles)
//!
//! Each 16x16 sprite occupies 4 × 8x8 tiles in VRAM (word addresses):
//!   TL at base, TR = base+$10, BL = base+$100, BR = base+$110.
//!
//! KO uses 8x8 glyphs only in BL/BR positions; TL/TR are blank (transparent).
//!
//! | JP text | VRAM base | Type   |
//! |---------|-----------|--------|
//! | そうび  | $4110/$4120/$4130 | 8x8 × 3 |
//! | ラ ラ   | $40E0 | 16x16 |
//! | リ リ   | $4200 | 16x16 |
//! | ル ル   | $4220 | 16x16 |
//! | レ レ   | $4240 | 16x16 |
//! | ロ ロ   | $4260 | 16x16 |
//! | レ イ   | $4280 | 16x16 |
//! | ミ ホ   | $42A0 | 16x16 |
//! | ピ チ   | $42C0 | 16x16 |
//! | ロ フ   | $42E0 | 16x16 |
//! | パ ド   | $44E0 | 16x16 |

use crate::font_gen;
use crate::patch::asm::{assemble, Inst};
use crate::patch::hook_common::JSL_LZ_BYTES;
use crate::patch::tracked_rom::{Expect, TrackedRom};
use crate::rom::lorom_to_pc;

// ── Bank / address constants ─────────────────────────────────────

/// ROM bank for tile data + hook code (shared with shop_oam in Bank $25).
const DATA_BANK: u8 = 0x25;

/// Start SNES address within Bank $25 (free from $C8A2, 14KB).
const DATA_ADDR: u16 = 0xC8A2;

/// Total tile data bytes (5 DMA groups).
///   64 (ラ ラ top) + 96 (そうび) + 1088 (ラ ラ bot + mid-8 top+bot)
///   + 64 (パ ド top) + 64 (パ ド bot) = 1376
const TILE_DATA_SIZE: usize = 1376;

// ── Hook site ────────────────────────────────────────────────────

/// JSL $009440 at $00:$83FF (pause menu init, fires once per menu entry).
/// Equipment is the default pause menu view — this covers all sub-screens.
const HOOK_SITE_PC: usize = 0x03FF; // lorom_to_pc(0x00, 0x83FF)

// ── Palette ──────────────────────────────────────────────────────

/// Foreground color index within Palette 4 for そうび text.
const SOUBI_FG: u8 = 1;

/// Foreground color index within Palette 4 for equipment names.
const NAME_FG: u8 = 1;

/// Background color index within Palette 4 for all equipment OAM tiles.
const BG_COLOR: u8 = 7;

// ── KO characters ────────────────────────────────────────────────

/// "장비" — replaces "そうび" (8x8 × 3, 3rd tile blank).
const SOUBI_KO: &[char] = &['장', '비'];

/// Equipment name character pairs (KO), ordered by VRAM address.
/// Each pair occupies a 16x16 OAM sprite: TL/TR blank, BL=left char, BR=right char.
///
/// Index 0: ラ ラ ($40E0) — solo DMA group
/// Index 1-8: リ リ~ロ フ ($4200-$42E0) — contiguous DMA group
/// Index 9: パ ド ($44E0) — solo DMA group
const NAME_PAIRS: [(char, char); 10] = [
    ('라', '라'), // ラ ラ — magic ring
    ('리', '리'), // リ リ — magic ring
    ('루', '루'), // ル ル — magic ring
    ('레', '레'), // レ レ — magic ring
    ('로', '로'), // ロ ロ — magic ring
    ('레', '이'), // レ イ — magic staff
    ('미', '호'), // ミ ホ — magic staff
    ('피', '치'), // ピ チ — magic staff
    ('로', '프'), // ロ フ — magic staff
    ('파', '도'), // パ ド — magic staff
];

// ── DMA group layout ─────────────────────────────────────────────
//
// ROM data is serialized in DMA group order. Each group maps to a
// contiguous VRAM region, enabling direct ROM→VRAM DMA.
//
// 16x16 sprite sub-tile VRAM layout (word addresses):
//   TL at base, TR = base+$10, BL = base+$100, BR = base+$110
//
// Groups 1,4 are blank (TL/TR of 16x16 sprites).
// Groups 2,3,5 contain rendered KO glyphs.
//
// | # | Content              | ROM offset | VRAM dest | Bytes |
// |---|----------------------|-----------|-----------|-------|
// | 1 | ラ ラ top (TL+TR)    | 0         | $40E0     | 64    |
// | 2 | そうび 3×8x8         | 64        | $4110     | 96    |
// | 3 | ラ ラ bot + mid-8    | 160       | $41E0     | 1088  |
// | 4 | パ ド top (TL+TR)    | 1248      | $44E0     | 64    |
// | 5 | パ ド bot (BL+BR)    | 1312      | $45E0     | 64    |
//
// Group 3 detail (contiguous VRAM $41E0-$43FF):
//   ラ ラ BL+BR (64B) + リリ~ロフ TL+TR ×8 (512B blank)
//   + リリ~ロフ BL+BR ×8 (512B rendered) = 1088B

/// (rom_data_offset, vram_dest_word, dma_size_bytes)
const DMA_GROUPS: [(u16, u16, u16); 5] = [
    (0, 0x40E0, 64),     // Group 1: ラ ラ top (blank TL+TR)
    (64, 0x4110, 96),    // Group 2: そうび (장+비+blank)
    (160, 0x41E0, 1088), // Group 3: ラ ラ bot + リリ~ロフ top+bot
    (1248, 0x44E0, 64),  // Group 4: パ ド top (blank TL+TR)
    (1312, 0x45E0, 64),  // Group 5: パ ド bot (파+도)
];

// ── Tile data generation ─────────────────────────────────────────

/// Build KO tile data organized by DMA group order (1376 bytes).
///
/// All characters are rendered as 8x8 4bpp tiles with bg_color fill:
/// - そうび: 장(32B) + 비(32B) + blank(32B)
/// - Equipment names: TL/TR = blank (bg_color fill), BL/BR = 8x8 rendered glyphs
fn build_equip_tile_data(ttf_data: &[u8], ttf_size: f32) -> Result<Vec<u8>, String> {
    let soubi_tiles =
        font_gen::render_oam_8x8_4bpp_tiles(ttf_data, ttf_size, SOUBI_KO, SOUBI_FG, BG_COLOR)?;

    // Render all 20 name characters (10 pairs × 2 chars each)
    let all_name_chars: Vec<char> = NAME_PAIRS.iter().flat_map(|&(l, r)| [l, r]).collect();
    let name_tiles = font_gen::render_oam_8x8_4bpp_tiles(
        ttf_data,
        ttf_size,
        &all_name_chars,
        NAME_FG,
        BG_COLOR,
    )?;

    // Pre-generate a blank tile filled with BG_COLOR
    let blank = font_gen::bitmap_to_snes_4bpp_8x8(&[false; 64], 0, BG_COLOR);

    let mut data = Vec::with_capacity(TILE_DATA_SIZE);

    // Group 1: ラ ラ top — TL(32B) + TR(32B) = 64B (bg_color fill)
    data.extend_from_slice(&blank);
    data.extend_from_slice(&blank);

    // Group 2: そうび — 장(32B) + 비(32B) + blank(32B) = 96B
    for tile in &soubi_tiles {
        data.extend_from_slice(tile);
    }
    data.extend_from_slice(&blank); // blank 3rd tile

    // Group 3: ラ ラ bot + リリ~ロフ top + リリ~ロフ bot = 1088B
    //   3a: ラ ラ BL(라, 32B) + BR(라, 32B) = 64B
    data.extend_from_slice(&name_tiles[0]); // 라
    data.extend_from_slice(&name_tiles[1]); // 라
                                            //   3b: リリ~ロフ top — 8 × (TL + TR) = 512B (bg_color fill)
    for _ in 0..16 {
        data.extend_from_slice(&blank);
    }
    //   3c: リリ~ロフ bot — 8 × (BL + BR) = 512B
    for i in 1..9 {
        data.extend_from_slice(&name_tiles[i * 2]); // left char
        data.extend_from_slice(&name_tiles[i * 2 + 1]); // right char
    }

    // Group 4: パ ド top — TL(32B) + TR(32B) = 64B (bg_color fill)
    data.extend_from_slice(&blank);
    data.extend_from_slice(&blank);

    // Group 5: パ ド bot — BL(파, 32B) + BR(도, 32B) = 64B
    data.extend_from_slice(&name_tiles[18]); // 파
    data.extend_from_slice(&name_tiles[19]); // 도

    debug_assert_eq!(data.len(), TILE_DATA_SIZE);
    Ok(data)
}

// ── Hook code generation ─────────────────────────────────────────

/// Build hook ASM: JSL $009440 → force blank → 5× ROM→VRAM DMA → RTL.
///
/// Uses DMA Channel 6 to copy KO tile data directly from ROM to VRAM.
/// The screen is in force blank during pause menu initialization, so direct
/// VRAM writes via DMA are safe.
fn build_equip_hook_code(data_bank: u8, data_base: u16) -> Result<Vec<u8>, String> {
    use Inst::*;

    let mut program = vec![
        Jsl(0x009440), // Call original LZ decompressor
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
    ];

    for (i, &(rom_offset, vram_dest, dma_size)) in DMA_GROUPS.iter().enumerate() {
        let src_addr = data_base.wrapping_add(rom_offset);
        program.push(Rep(0x20)); // 16-bit A
        program.push(LdaImm16(vram_dest));
        program.push(StaAbs(0x2116)); // VRAM word address
        program.push(LdaImm16(src_addr));
        program.push(StaAbs(0x4362)); // A-bus source address
        program.push(LdaImm16(dma_size));
        program.push(StaAbs(0x4365)); // Transfer size
        program.push(Sep(0x20)); // 8-bit A
        if i == 0 {
            program.push(StzAbs(0x420C)); // Disable HDMA (once, prevent bus conflicts)
        }
        program.push(LdaImm8(0x40)); // Channel 6 = bit 6
        program.push(StaAbs(0x420B)); // Trigger DMA
    }

    // Restore state
    program.push(Plb);
    program.push(Plp);
    program.push(Rtl);

    assemble(&program)
}

// ── Public API ───────────────────────────────────────────────────

/// Apply equipment OAM sprite hook .
///
/// Replaces そうび (장비) and 10 equipment names using ROM→VRAM direct
/// DMA during pause menu initialization ($00:$83FF hook).
pub fn apply_equip_oam_hook(
    rom: &mut TrackedRom,
    ttf_data: &[u8],
    ttf_size: f32,
) -> Result<(), String> {
    let tile_data = build_equip_tile_data(ttf_data, ttf_size)?;
    let hook_code = build_equip_hook_code(DATA_BANK, DATA_ADDR)?;

    // Write tile data + hook code to Bank $25
    let data_pc = lorom_to_pc(DATA_BANK, DATA_ADDR);
    let total_size = tile_data.len() + hook_code.len();
    {
        let mut r = rom.region_expect(
            data_pc,
            total_size,
            "equip_oam:data+code",
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
        "equip_oam:hook",
        &Expect::Bytes(&JSL_LZ_BYTES),
    );

    println!(
        "  Equipment OAM : {}B tiles + {}B code @ ${:02X}:${:04X}-${:04X}",
        tile_data.len(),
        hook_code.len(),
        DATA_BANK,
        DATA_ADDR,
        DATA_ADDR + total_size as u16 - 1,
    );
    println!(
        "    Hook: $00:$83FF → ${:02X}:${:04X} (ROM→VRAM DMA Ch6 ×{})",
        DATA_BANK,
        code_snes,
        DMA_GROUPS.len(),
    );

    Ok(())
}

#[cfg(test)]
#[path = "equip_oam_tests.rs"]
mod tests;
