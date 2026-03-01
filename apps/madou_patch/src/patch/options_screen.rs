//! Options / Stat / Magic screen localization .
//!
//! ## Screen Architecture (state machine at $01:$8069)
//!
//! The menu system has TWO independent screen paths with separate CHR:
//!
//! - **Stat+Magic screen** ($167A=2, states 3–6):
//!   - State 3: Entry 4 CHR ($01:$820A) + portrait ($01:$8255) → WRAM $7F:$D000
//!   - State 4: Entry 6 TM6 ($01:$8322) → stat/magic tilemap (2-page L/R scroll)
//!   - CHR DMA: WRAM $7F:$D000 → VRAM $2000, 12KB ($3000)
//!
//! - **Options screen** ($167A=3, states 9–12):
//!   - State 9: Entry 5 TM5 ($01:$8F83) → tilemap to WRAM $7F:$C800
//!   - State 10: Entry 3 CHR ($01:$8F46) → WRAM $7F:$D000
//!   - CHR DMA: WRAM $7F:$D000 → VRAM $2000, 8KB ($2000)
//!
//! Screens are **mutually exclusive**: each transition reloads CHR from scratch.
//!
//! Five JSL $009440 sites in Bank $01:
//!   $820A (Entry 4 CHR, stat+magic), $8255 (portrait),
//!   $8322 (Entry 6 TM6), $8F46 (Entry 3 CHR, options), $8F83 (Entry 5 TM5)
//!
//! ## Architecture: WRAM Direct Patching (MVN)
//!
//! Each hooked CHR decompress site: after the original LZ decompressor writes
//! tiles to WRAM $7F:$D000, the hook copies overlay data from ROM to WRAM via
//! MVN. The game's existing DMA then sends the patched WRAM to VRAM as usual.
//!
//! ## Phase 2a: Stat+Magic screen
//!
//! Partial overlay: tiles $254–$2E4 (145 tiles, 4640B 4bpp) to WRAM $DA80.
//! Palette: idx 8 (bg) / idx 10 (stroke).
//!
//! ## Phase 2b: Options screen
//!
//! Full overlay: all 125 4bpp tiles (4000B) to WRAM $D000.
//! Palette: idx 15 (bg) / idx 13 (stroke) — BP0=$FF, BP1=~glyph, BP2=BP3=$FF.
//! Uses direct tile mapping (not region-based) to handle:
//! - 2-column bottom-row shift in 16x16 text
//! - Tile sharing conflicts ($22A/$22B/$24E/$24F)
//! - TM5 remap hook at $01:$8F83 for shared tile resolution

use crate::patch::asm::{assemble, Inst};
use crate::patch::font;
use crate::patch::hook_common::{self, JSL_LZ_BYTES};
use crate::patch::tracked_rom::{Expect, TrackedRom};
use crate::rom::lorom_to_pc;

// ── LZ pointer table (Bank $08) ────────────────────────────────────
const LZ_PTR_BANK: u8 = 0x08;
const LZ_PTR_TABLE_PC: usize = 0x40000; // lorom_to_pc(0x08, 0x8000)

// ── Hook sites ──────────────────────────────────────────────────────
const CHR_HOOK_PC: usize = 0x0820A; // $01:$820A — stat+magic CHR (Entry 4)
const OPT_CHR_HOOK_PC: usize = 0x08F46; // $01:$8F46 — options CHR (Entry 3)
const OPT_TM5_HOOK_PC: usize = 0x08F83; // $01:$8F83 — options TM5 (Entry 5)

// ── Data placement (Bank $1C) ───────────────────────────────────────
const DATA_BANK: u8 = 0x1C;
const CODE_BASE_ADDR: u16 = 0xD800; // Phase 2a hook code
const TM6_HOOK_PC: usize = 0x08322; // $01:$8322 — stat+magic TM6 (Entry 6)
const TM6_CODE_ADDR: u16 = 0xEA40; // TM6 remap hook code (Bank $1C, after Phase 2a)
const OPT_CODE_ADDR: u16 = 0xEAD0; // Phase 2b CHR hook code (after TM6 hook)

// ── Tile constants ──────────────────────────────────────────────────
const TILE_SIZE: usize = 16; // 8×8 2bpp tile (BP0+BP1)
const TILE_SIZE_4BPP: usize = 32; // 8×8 4bpp tile
const TILE_BASE: u16 = 0x200; // tilemap index $200 = CHR tile 0

// Phase 2a overlay range
const OVERLAY_FIRST: u16 = 0x254;
const OVERLAY_LAST: u16 = 0x2E4;
#[cfg(test)]
const OVERLAY_TILES: usize = (OVERLAY_LAST - OVERLAY_FIRST + 1) as usize; // 145
#[cfg(test)]
const OVERLAY_SIZE: usize = OVERLAY_TILES * TILE_SIZE;
#[cfg(test)]
const OVERLAY_SIZE_4BPP: usize = OVERLAY_TILES * TILE_SIZE_4BPP; // 4640

/// WRAM offset = $D000 + (OVERLAY_FIRST - TILE_BASE) * 2 * TILE_SIZE = $DA80
const WRAM_OVERLAY_OFFSET: u16 = 0xDA80;

// Phase 2b: full CHR (125 LZ tiles + 3 extended remap targets $27D-$27F)
const OPT_CHR_TILES: usize = 128;
#[cfg(test)]
const OPT_OVERLAY_SIZE_4BPP: usize = OPT_CHR_TILES * TILE_SIZE_4BPP; // 4000
const OPT_WRAM_DEST: u16 = 0xD000;

// TM5 decompression buffer
const TM5_WRAM_BASE: u16 = 0xC800;

// Tilemap layout
const TM_COLS: usize = 32; // TM6 (stat+magic): 32 cols × 19 rows × 2 pages
const TM5_COLS: usize = 30; // TM5 (options): 30 cols × 17 rows (1020B)
const TM_PAGE_ROWS: usize = 19;

/// Hook assembly is always 23 bytes regardless of parameters. Verified by test.
const HOOK_CODE_SIZE: usize = 23;

// ── Types ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TileSize {
    S8x8,
    S16x16,
}

/// Palette encoding for 4bpp overlay tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PaletteMode {
    /// Phase 2a: stroke=idx10 (BP0=$00,BP1=glyph), bg=idx8 (BP0=$00,BP1=$00).
    /// KO tiles: BP0=$00, BP1=glyph, BP2=$00, BP3=$FF.
    StatMagic,
    /// Phase 2b body: stroke=idx13 (0x0D), bg=idx15 (0x0F).
    /// KO tiles: BP0=$FF, BP1=~glyph, BP2=$FF, BP3=$FF.
    Options,
    /// Outlined body: text=idx13(0x0D), outline=idx12(0x0C), bg=idx15(0x0F).
    /// 2bpp input: BP0=glyph, BP1=dilated.
    /// 4bpp: BP0=glyph|~dilated, BP1=~dilated, BP2=$FF, BP3=$FF.
    OutlinedBody,
    /// Outlined header top tile: rows 0,1 = border (0x0C, 0x0E),
    /// rows 2-7 = outlined (text=0x0D, outline=0x0C, bg=0x0A).
    OutlinedHeaderTop,
    /// Outlined header bottom tile: rows 0-5 = outlined,
    /// rows 6,7 = border (0x0E, 0x0C).
    OutlinedHeaderBottom,
    /// Header border top (blank): row0=0x0C, row1=0x0E, rows2-7=0x0A.
    HeaderBorderTop,
    /// Header border bottom (blank): rows0-5=0x0A, row6=0x0E, row7=0x0C.
    HeaderBorderBottom,
}

#[derive(Clone, Copy, Debug)]
struct TextRegion {
    ko: &'static str,
    size: TileSize,
    row: usize,
    col_start: usize,
    col_end: usize, // exclusive
}

/// Configuration for a screen hook (shared between Phase 2a and 2b).
struct ScreenHookConfig {
    name: &'static str,
    chr_entry_idx: usize,
    tm_entry_idx: usize,
    hook_pc: usize,
    region_groups: &'static [&'static [TextRegion]],
    overlay_first: u16,
    overlay_last: u16,
    code_addr: u16,
    wram_dest: u16,
}

/// Glyph lookup for 8×8 and 16×16 tiles (Vec-based, no HashMap).
struct GlyphSet<'a> {
    chars_8: &'a [char],
    tiles_8: &'a [[u8; TILE_SIZE]],
    chars_16: &'a [char],
    tiles_16: &'a [[[u8; TILE_SIZE]; 4]],
}

impl GlyphSet<'_> {
    fn get_8x8(&self, ch: char) -> Result<&[u8; TILE_SIZE], String> {
        self.chars_8
            .iter()
            .position(|&c| c == ch)
            .map(|i| &self.tiles_8[i])
            .ok_or_else(|| format!("Missing 8x8 glyph for '{}'", ch))
    }

    fn get_16x16(&self, ch: char) -> Result<&[[u8; TILE_SIZE]; 4], String> {
        self.chars_16
            .iter()
            .position(|&c| c == ch)
            .map(|i| &self.tiles_16[i])
            .ok_or_else(|| format!("Missing 16x16 glyph for '{}'", ch))
    }
}

// ── Region definitions (Phase 2a: Stat+Magic) ───────────────────────

/// Stat screen (TM6 left page, rows 0–18, cols 0–31).
const STAT_REGIONS: &[TextRegion] = &[
    TextRegion {
        ko: "원아증",
        size: TileSize::S16x16,
        row: 2,
        col_start: 17,
        col_end: 23,
    },
    TextRegion {
        ko: "아르르나쟈",
        size: TileSize::S8x8,
        row: 5,
        col_start: 17,
        col_end: 24,
    },
    TextRegion {
        ko: "꽃반",
        size: TileSize::S8x8,
        row: 5,
        col_start: 26,
        col_end: 30,
    },
    TextRegion {
        ko: "힘세기",
        size: TileSize::S8x8,
        row: 8,
        col_start: 17,
        col_end: 20,
    },
    TextRegion {
        ko: "방어력",
        size: TileSize::S8x8,
        row: 10,
        col_start: 17,
        col_end: 20,
    },
    TextRegion {
        ko: "빠르기",
        size: TileSize::S8x8,
        row: 12,
        col_start: 17,
        col_end: 20,
    },
];

/// Magic screen (TM6 right page, cols 32–63 in 64-wide space).
const MAGIC_REGIONS: &[TextRegion] = &[
    TextRegion {
        ko: "신비석",
        size: TileSize::S8x8,
        row: 3,
        col_start: 35,
        col_end: 38,
    },
    TextRegion {
        ko: "바요엔",
        size: TileSize::S8x8,
        row: 11,
        col_start: 34,
        col_end: 37,
    },
    TextRegion {
        ko: "리바이어",
        size: TileSize::S8x8,
        row: 13,
        col_start: 33,
        col_end: 37,
    },
    TextRegion {
        ko: "히돈",
        size: TileSize::S8x8,
        row: 15,
        col_start: 35,
        col_end: 37,
    },
    TextRegion {
        ko: "브레인담드",
        size: TileSize::S8x8,
        row: 11,
        col_start: 39,
        col_end: 44,
    },
    TextRegion {
        ko: "바요히히히",
        size: TileSize::S8x8,
        row: 13,
        col_start: 39,
        col_end: 44,
    },
    TextRegion {
        ko: "쥬겜",
        size: TileSize::S8x8,
        row: 15,
        col_start: 42,
        col_end: 44,
    },
    TextRegion {
        ko: "힐링",
        size: TileSize::S8x8,
        row: 7,
        col_start: 50,
        col_end: 52,
    },
    TextRegion {
        ko: "파이어",
        size: TileSize::S8x8,
        row: 9,
        col_start: 49,
        col_end: 52,
    },
    TextRegion {
        ko: "아이스스톰",
        size: TileSize::S8x8,
        row: 11,
        col_start: 47,
        col_end: 52,
    },
    TextRegion {
        ko: "썬더",
        size: TileSize::S8x8,
        row: 13,
        col_start: 50,
        col_end: 52,
    },
    TextRegion {
        ko: "다이아큐트",
        size: TileSize::S8x8,
        row: 15,
        col_start: 47,
        col_end: 52,
    },
];

// ── Phase 2b: Direct tile mapping (Options screen) ──────────────────

/// Unique 16×16 KO characters for options screen (index = char_idx in OPT_TILE_MAP).
const OPT_16X16_CHARS: &[char] = &[
    '옵', '션', // 0-1: title "옵션"
    '스', '테', '레', '오', // 2-5: "스테레오"
    '모', '노', // 6-7: "모노"
    '빠', '름', // 8-9: "빠름"
    '느', '림', // 10-11: "느림"
    '보', '통', // 12-13: "보통"
];

/// Unique 8×8 KO characters for options screen (index = char_idx in OPT_TILE_MAP).
const OPT_8X8_CHARS: &[char] = &[
    '사', '운', '드', '모', // 0-3: "사운드 모드"
    '글', '자', '속', '도', // 4-7: "글자속도"
    '창', '색', '상', // 8-10: "창 색상"
];

/// Direct CHR tile → content mapping for options screen.
///
/// Each entry maps a CHR tile index ($200+) to the KO content to render.
/// Tiles not listed here preserve original JP data.
///
/// - `Quad`: 16x16 quadrant — `char_idx` indexes OPT_16X16_CHARS, `quadrant`: 0=TL 1=TR 2=BL 3=BR
/// - `Glyph`: 8x8 glyph — `char_idx` indexes OPT_8X8_CHARS
/// - `Blank`: opaque background fill
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OptTile {
    Quad { char_idx: usize, quadrant: usize },
    Glyph { char_idx: usize },
    Blank,
}

const OPT_TILE_MAP: &[(u16, OptTile)] = &[
    // ── TM5 remap targets (extended CHR $27D-$27F, beyond LZ 125 tiles) ──
    (0x200, OptTile::Glyph { char_idx: 3 }), // 모 (remapped from $213(bg) at r3c8)
    (
        0x27D,
        OptTile::Quad {
            char_idx: 10,
            quadrant: 0,
        },
    ), // 느 TL (remapped from $22A at r7c22)
    (
        0x27E,
        OptTile::Quad {
            char_idx: 10,
            quadrant: 1,
        },
    ), // 느 TR (remapped from $22B at r7c23)
    (0x27F, OptTile::Glyph { char_idx: 8 }), // 창 (remapped from $21F at r9c4)
    // ── Title "옵션" (おぷしょん → 2 KO chars, centered with gap) ──
    // Top row: 2 blanks + 옵(2) + gap(1) + 션(2) + 2 blanks
    (0x209, OptTile::Blank),
    (0x20A, OptTile::Blank),
    (
        0x20B,
        OptTile::Quad {
            char_idx: 0,
            quadrant: 0,
        },
    ), // 옵 TL
    (
        0x20C,
        OptTile::Quad {
            char_idx: 0,
            quadrant: 1,
        },
    ), // 옵 TR
    (0x20D, OptTile::Blank), // gap
    (
        0x20E,
        OptTile::Quad {
            char_idx: 1,
            quadrant: 0,
        },
    ), // 션 TL
    (
        0x20F,
        OptTile::Quad {
            char_idx: 1,
            quadrant: 1,
        },
    ), // 션 TR
    (0x210, OptTile::Blank),
    (0x211, OptTile::Blank),
    // Bottom row: 2 blanks + 옵(2) + gap(1) + 션(2) + 2 blanks
    (0x214, OptTile::Blank),
    (0x215, OptTile::Blank),
    (
        0x216,
        OptTile::Quad {
            char_idx: 0,
            quadrant: 2,
        },
    ), // 옵 BL
    (
        0x217,
        OptTile::Quad {
            char_idx: 0,
            quadrant: 3,
        },
    ), // 옵 BR
    (0x218, OptTile::Blank), // gap
    (
        0x219,
        OptTile::Quad {
            char_idx: 1,
            quadrant: 2,
        },
    ), // 션 BL
    (
        0x21A,
        OptTile::Quad {
            char_idx: 1,
            quadrant: 3,
        },
    ), // 션 BR
    (0x21B, OptTile::Blank),
    (0x21C, OptTile::Blank),
    // ── "사운드 모드" 8x8 — JP: "サウンドモード" ($21E-$223) ──
    (0x21E, OptTile::Glyph { char_idx: 0 }), // 사
    (0x21F, OptTile::Glyph { char_idx: 1 }), // 운 (shared with ウインドウ; DEFER keeps 운)
    (0x220, OptTile::Glyph { char_idx: 2 }), // 드
    (0x221, OptTile::Blank),                 // blank (shared: r3c8 + r3c12)
    (0x222, OptTile::Glyph { char_idx: 2 }), // 드
    (0x223, OptTile::Blank),                 // blank
    // ── "스테레오" 16x16 — JP: "すてれお" ($224-$22B / $234-$23B) ──
    // Top row
    (
        0x224,
        OptTile::Quad {
            char_idx: 2,
            quadrant: 0,
        },
    ), // 스 TL
    (
        0x225,
        OptTile::Quad {
            char_idx: 2,
            quadrant: 1,
        },
    ), // 스 TR
    (
        0x226,
        OptTile::Quad {
            char_idx: 3,
            quadrant: 0,
        },
    ), // 테 TL
    (
        0x227,
        OptTile::Quad {
            char_idx: 3,
            quadrant: 1,
        },
    ), // 테 TR
    (
        0x228,
        OptTile::Quad {
            char_idx: 4,
            quadrant: 0,
        },
    ), // 레 TL
    (
        0x229,
        OptTile::Quad {
            char_idx: 4,
            quadrant: 1,
        },
    ), // 레 TR
    (
        0x22A,
        OptTile::Quad {
            char_idx: 5,
            quadrant: 0,
        },
    ), // 오 TL (shared with おそい r7c22; remap→$202)
    (
        0x22B,
        OptTile::Quad {
            char_idx: 5,
            quadrant: 1,
        },
    ), // 오 TR (shared with おそい r7c23; remap→$203)
    // Bottom row
    (
        0x234,
        OptTile::Quad {
            char_idx: 2,
            quadrant: 2,
        },
    ), // 스 BL
    (
        0x235,
        OptTile::Quad {
            char_idx: 2,
            quadrant: 3,
        },
    ), // 스 BR
    (
        0x236,
        OptTile::Quad {
            char_idx: 3,
            quadrant: 2,
        },
    ), // 테 BL
    (
        0x237,
        OptTile::Quad {
            char_idx: 3,
            quadrant: 3,
        },
    ), // 테 BR
    (
        0x238,
        OptTile::Quad {
            char_idx: 4,
            quadrant: 2,
        },
    ), // 레 BL
    (
        0x239,
        OptTile::Quad {
            char_idx: 4,
            quadrant: 3,
        },
    ), // 레 BR
    (
        0x23A,
        OptTile::Quad {
            char_idx: 5,
            quadrant: 2,
        },
    ), // 오 BL
    (
        0x23B,
        OptTile::Quad {
            char_idx: 5,
            quadrant: 3,
        },
    ), // 오 BR
    // ── "모노" 16x16 — JP: "ものらる" ($22C-$233 / $23C-$243) ──
    // Top row (2 chars + 4 blank)
    (
        0x22C,
        OptTile::Quad {
            char_idx: 6,
            quadrant: 0,
        },
    ), // 모 TL
    (
        0x22D,
        OptTile::Quad {
            char_idx: 6,
            quadrant: 1,
        },
    ), // 모 TR
    (
        0x22E,
        OptTile::Quad {
            char_idx: 7,
            quadrant: 0,
        },
    ), // 노 TL
    (
        0x22F,
        OptTile::Quad {
            char_idx: 7,
            quadrant: 1,
        },
    ), // 노 TR
    (0x230, OptTile::Blank),
    (0x231, OptTile::Blank),
    (0x232, OptTile::Blank),
    (0x233, OptTile::Blank),
    // Bottom row (2 chars + 4 blank)
    (
        0x23C,
        OptTile::Quad {
            char_idx: 6,
            quadrant: 2,
        },
    ), // 모 BL
    (
        0x23D,
        OptTile::Quad {
            char_idx: 6,
            quadrant: 3,
        },
    ), // 모 BR
    (
        0x23E,
        OptTile::Quad {
            char_idx: 7,
            quadrant: 2,
        },
    ), // 노 BL
    (
        0x23F,
        OptTile::Quad {
            char_idx: 7,
            quadrant: 3,
        },
    ), // 노 BR
    (0x240, OptTile::Blank),
    (0x241, OptTile::Blank),
    (0x242, OptTile::Blank),
    (0x243, OptTile::Blank),
    // ── "글자속도" 8x8 — JP: "もじのはやさ" ($244-$249) ──
    (0x244, OptTile::Glyph { char_idx: 4 }), // 글
    (0x245, OptTile::Glyph { char_idx: 5 }), // 자
    (0x246, OptTile::Glyph { char_idx: 6 }), // 속
    (0x247, OptTile::Glyph { char_idx: 7 }), // 도
    (0x248, OptTile::Blank),
    (0x249, OptTile::Blank),
    // ── "빠름" 16x16 — JP: "はやい" ($24A-$24F / $258-$25D) ──
    // Top row
    (
        0x24A,
        OptTile::Quad {
            char_idx: 8,
            quadrant: 0,
        },
    ), // 빠 TL
    (
        0x24B,
        OptTile::Quad {
            char_idx: 8,
            quadrant: 1,
        },
    ), // 빠 TR
    (
        0x24C,
        OptTile::Quad {
            char_idx: 9,
            quadrant: 0,
        },
    ), // 름 TL
    (
        0x24D,
        OptTile::Quad {
            char_idx: 9,
            quadrant: 1,
        },
    ), // 름 TR
    (0x24E, OptTile::Blank), // blank (shared with おそい; no conflict)
    (0x24F, OptTile::Blank), // blank (shared with おそい; no conflict)
    // Bottom row
    (
        0x258,
        OptTile::Quad {
            char_idx: 8,
            quadrant: 2,
        },
    ), // 빠 BL
    (
        0x259,
        OptTile::Quad {
            char_idx: 8,
            quadrant: 3,
        },
    ), // 빠 BR
    (
        0x25A,
        OptTile::Quad {
            char_idx: 9,
            quadrant: 2,
        },
    ), // 름 BL
    (
        0x25B,
        OptTile::Quad {
            char_idx: 9,
            quadrant: 3,
        },
    ), // 름 BR
    (0x25C, OptTile::Blank), // blank (shared with おそい; no conflict)
    (0x25D, OptTile::Blank), // blank (shared with おそい; no conflict)
    // ── "보통" 16x16 — JP: "ふつう" ($250-$255 / $25E-$263) ──
    // Top row
    (
        0x250,
        OptTile::Quad {
            char_idx: 12,
            quadrant: 0,
        },
    ), // 보 TL
    (
        0x251,
        OptTile::Quad {
            char_idx: 12,
            quadrant: 1,
        },
    ), // 보 TR
    (
        0x252,
        OptTile::Quad {
            char_idx: 13,
            quadrant: 0,
        },
    ), // 통 TL
    (
        0x253,
        OptTile::Quad {
            char_idx: 13,
            quadrant: 1,
        },
    ), // 통 TR
    (0x254, OptTile::Blank),
    (0x255, OptTile::Blank),
    // Bottom row
    (
        0x25E,
        OptTile::Quad {
            char_idx: 12,
            quadrant: 2,
        },
    ), // 보 BL
    (
        0x25F,
        OptTile::Quad {
            char_idx: 12,
            quadrant: 3,
        },
    ), // 보 BR
    (
        0x260,
        OptTile::Quad {
            char_idx: 13,
            quadrant: 2,
        },
    ), // 통 BL
    (
        0x261,
        OptTile::Quad {
            char_idx: 13,
            quadrant: 3,
        },
    ), // 통 BR
    (0x262, OptTile::Blank),
    (0x263, OptTile::Blank),
    // ── "느림" 16x16 — JP: "おそい" (shared tiles + $256-$257 / $264-$267) ──
    // Top row: 느TL/TR → remap $202/$203, blank → shared $24E/$24F
    (
        0x256,
        OptTile::Quad {
            char_idx: 11,
            quadrant: 0,
        },
    ), // 림 TL
    (
        0x257,
        OptTile::Quad {
            char_idx: 11,
            quadrant: 1,
        },
    ), // 림 TR
    // Bottom row
    (
        0x264,
        OptTile::Quad {
            char_idx: 10,
            quadrant: 2,
        },
    ), // 느 BL
    (
        0x265,
        OptTile::Quad {
            char_idx: 10,
            quadrant: 3,
        },
    ), // 느 BR
    (
        0x266,
        OptTile::Quad {
            char_idx: 11,
            quadrant: 2,
        },
    ), // 림 BL
    (
        0x267,
        OptTile::Quad {
            char_idx: 11,
            quadrant: 3,
        },
    ), // 림 BR
    // ── "창 색상" 8x8 — JP: "ウインドウのいろ" ($268-$26E) ──
    // r9: $21F(창→remap $271), $268(blank), $269(색), $26A(상), $26B-$26E(blank)
    (0x268, OptTile::Blank),                  // 공백
    (0x269, OptTile::Glyph { char_idx: 9 }),  // 색
    (0x26A, OptTile::Glyph { char_idx: 10 }), // 상
    (0x26B, OptTile::Blank),
    (0x26C, OptTile::Blank),
    (0x26D, OptTile::Blank),
    (0x26E, OptTile::Blank),
];

/// TM5 entries to remap: (row, col, new_tile_index).
/// Resolves tile sharing conflicts by redirecting to unused CHR tiles.
const TM5_REMAPS: &[(usize, usize, u16)] = &[
    (3, 8, 0x200),  // $213(bg) → $200 (사운드 모드 '모') — 30-col coords
    (7, 22, 0x27D), // $22A → $27D (느림 느TL)
    (7, 23, 0x27E), // $22B → $27E (느림 느TR)
    (9, 4, 0x27F),  // $21F → $27F (창 색상 '창')
];

/// Transparent tile index ($227) — all-zero BP0+BP1, color 0 = transparent.
/// BG layer behind shows through, giving the correct background appearance.
const TM6_BLANK_TILE: u16 = 0x227;

/// TM6 entries to remap: (row, col, new_tile_index).
/// Adjusts magic name positions and underlines to match KO character widths.
/// Row/col use 64-column TM6 coordinate space (right page cols 32–63).
///
/// For 리바이어 extension: tile $2D6 (freed by r15c34 remap, 1 ref only)
/// is repurposed for '리'. $2BC (skill icon at r13c37) stays untouched.
const TM6_REMAPS: &[(usize, usize, u16)] = &[
    // ── Name text patches (clear vacated columns, extend 리바이어) ──
    (11, 33, TM6_BLANK_TILE), // 바요엔: clear vacated column
    (13, 33, 0x2D6),          // 리바이어: '리' on freed tile ($2D6, was r15c34 only)
    (15, 34, TM6_BLANK_TILE), // 히돈: clear vacated column (frees $2D6 for above)
    (15, 41, TM6_BLANK_TILE), // 쥬겜: clear vacated column
    (7, 48, TM6_BLANK_TILE),  // 힐링: clear vacated column
    (7, 49, TM6_BLANK_TILE),  // 힐링: clear vacated column
    (9, 48, TM6_BLANK_TILE),  // 파이어: clear vacated column
    (13, 49, TM6_BLANK_TILE), // 썬더: clear vacated column
    // ── Underline patches (0x268=underline, TM6_BLANK_TILE=transparent) ──
    (12, 33, TM6_BLANK_TILE), // 바요엔: shrink underline
    (14, 33, 0x268),          // 리바이어: extend underline
    (16, 34, TM6_BLANK_TILE), // 히돈: shrink underline
    (16, 40, TM6_BLANK_TILE), // 쥬겜: shrink underline (JP extended to col 40)
    (16, 41, TM6_BLANK_TILE), // 쥬겜: shrink underline
    (8, 48, TM6_BLANK_TILE),  // 힐링: shrink underline
    (8, 49, TM6_BLANK_TILE),  // 힐링: shrink underline
    (10, 48, TM6_BLANK_TILE), // 파이어: shrink underline
    (14, 48, TM6_BLANK_TILE), // 썬더: shrink underline (JP extended to col 48)
    (14, 49, TM6_BLANK_TILE), // 썬더: shrink underline
];

// ── Screen hook configs (Phase 2a only) ─────────────────────────────

const STAT_MAGIC_REGION_GROUPS: &[&[TextRegion]] = &[STAT_REGIONS, MAGIC_REGIONS];

const STAT_MAGIC_CONFIG: ScreenHookConfig = ScreenHookConfig {
    name: "Stat+Magic",
    chr_entry_idx: 4, // Entry 4 CHR
    tm_entry_idx: 6,  // Entry 6 TM6
    hook_pc: CHR_HOOK_PC,
    region_groups: STAT_MAGIC_REGION_GROUPS,
    overlay_first: OVERLAY_FIRST,
    overlay_last: OVERLAY_LAST,
    code_addr: CODE_BASE_ADDR,
    wram_dest: WRAM_OVERLAY_OFFSET,
};

// ── Collect unique KO characters ───────────────────────────────────

fn collect_chars_from_regions(regions_list: &[&[TextRegion]], size: TileSize) -> Vec<char> {
    let mut chars = Vec::new();
    for &regions in regions_list {
        for region in regions {
            if region.size != size {
                continue;
            }
            for ch in region.ko.chars() {
                if !chars.contains(&ch) {
                    chars.push(ch);
                }
            }
        }
    }
    chars
}

/// Unique 8×8 KO characters for stat/magic screens.
pub fn collect_ko_chars_8x8() -> Vec<char> {
    collect_chars_from_regions(STAT_MAGIC_CONFIG.region_groups, TileSize::S8x8)
}

/// Unique 16×16 KO characters for stat/magic screens.
pub fn collect_ko_chars_16x16() -> Vec<char> {
    collect_chars_from_regions(STAT_MAGIC_CONFIG.region_groups, TileSize::S16x16)
}

/// Unique 8×8 KO characters for options screen.
pub fn collect_ko_chars_8x8_options() -> Vec<char> {
    OPT_8X8_CHARS.to_vec()
}

/// Unique 16×16 KO characters for options screen.
pub fn collect_ko_chars_16x16_options() -> Vec<char> {
    OPT_16X16_CHARS.to_vec()
}

// ── Blank tile (opaque background fill) ────────────────────────────

/// BP0=$00 (no stroke), BP1=$FF (opaque) for all 8 rows → palette color 2 (background).
const BLANK_TILE: [u8; TILE_SIZE] = {
    let mut tile = [0u8; TILE_SIZE];
    let mut r = 0;
    while r < 8 {
        tile[r * 2 + 1] = 0xFF;
        r += 1;
    }
    tile
};

// ── LZ pointer lookup (delegated to hook_common) ─────────────────

fn lookup_lz_source(rom: &[u8], entry_idx: usize) -> Result<usize, String> {
    hook_common::lookup_lz_source(rom, LZ_PTR_BANK, LZ_PTR_TABLE_PC, entry_idx)
}

// ── Tilemap entry access ───────────────────────────────────────────

/// Read tile index (10-bit) from a tilemap at (row, col) in 64-column space.
/// Works for both TM5 (options) and TM6 (stat/magic) — same layout.
fn tilemap_tile_at(tm: &[u8], row: usize, col: usize) -> u16 {
    let idx = if col < 32 {
        row * TM_COLS + col
    } else {
        TM_PAGE_ROWS * TM_COLS + row * TM_COLS + (col - 32)
    };
    let byte_off = idx * 2;
    if byte_off + 1 >= tm.len() {
        return 0;
    }
    u16::from_le_bytes([tm[byte_off], tm[byte_off + 1]]) & 0x03FF
}

/// Read full 16-bit tilemap entry (includes attribute bits) at (row, col).
fn tilemap_entry_at(tm: &[u8], row: usize, col: usize, cols: usize) -> u16 {
    let idx = row * cols + col;
    let byte_off = idx * 2;
    if byte_off + 1 >= tm.len() {
        return 0;
    }
    u16::from_le_bytes([tm[byte_off], tm[byte_off + 1]])
}

// ── CHR overlay replacement (Phase 2a: region-based) ─────────────────

/// Replace tile graphics in the 2bpp CHR overlay for text regions.
///
/// Only tiles within `overlay_first..=overlay_last` are modified.
/// `patched` is a bool slice (one per overlay tile) marking which tiles were replaced.
fn apply_regions_overlay<F: Fn(usize, usize) -> u16>(
    overlay: &mut [u8],
    regions: &[TextRegion],
    tile_at: &F,
    glyphs: &GlyphSet,
    patched: &mut [bool],
    overlay_first: u16,
    overlay_last: u16,
) -> Result<usize, String> {
    let mut replaced = 0;

    for region in regions {
        let ko_chars: Vec<char> = region.ko.chars().collect();

        match region.size {
            TileSize::S8x8 => {
                for (i, col) in (region.col_start..region.col_end).enumerate() {
                    let tile_idx = tile_at(region.row, col);
                    if !(overlay_first..=overlay_last).contains(&tile_idx) {
                        continue;
                    }
                    let rel = (tile_idx - overlay_first) as usize;
                    let off = rel * TILE_SIZE;
                    if off + TILE_SIZE > overlay.len() {
                        continue;
                    }
                    let data = if i < ko_chars.len() {
                        glyphs.get_8x8(ko_chars[i])?
                    } else {
                        &BLANK_TILE
                    };
                    overlay[off..off + TILE_SIZE].copy_from_slice(data);
                    patched[rel] = true;
                    replaced += 1;
                }
            }
            TileSize::S16x16 => {
                for row_off in 0..2usize {
                    let r = region.row + row_off;
                    let qi_base = row_off * 2;

                    for (i, col) in (region.col_start..region.col_end).enumerate() {
                        let tile_idx = tile_at(r, col);
                        if !(overlay_first..=overlay_last).contains(&tile_idx) {
                            continue;
                        }
                        let rel = (tile_idx - overlay_first) as usize;
                        let off = rel * TILE_SIZE;
                        if off + TILE_SIZE > overlay.len() {
                            continue;
                        }

                        let char_slot = i / 2;
                        let quad = qi_base + (i % 2);
                        let data = if char_slot < ko_chars.len() {
                            &glyphs.get_16x16(ko_chars[char_slot])?[quad]
                        } else {
                            &BLANK_TILE
                        };
                        overlay[off..off + TILE_SIZE].copy_from_slice(data);
                        patched[rel] = true;
                        replaced += 1;
                    }
                }
            }
        }
    }

    Ok(replaced)
}

// ── 2bpp → 4bpp interleaving ────────────────────────────────────────

/// Build 4bpp overlay from 2bpp overlay + original JP CHR data.
///
/// For **patched** (KO) tiles, the palette mode determines bitplane encoding:
/// - StatMagic: BP0=$00, BP1=glyph, BP2=$00, BP3=$FF
/// - Options: BP0=$FF, BP1=~glyph, BP2=~glyph, BP3=~glyph
/// - Header: BP0=glyph, BP1=~glyph, BP2=$00, BP3=~glyph
///
/// For **unpatched** (JP) tiles: preserve original BP0+BP1 and BP2+BP3.
fn build_4bpp_overlay(
    overlay_2bpp: &[u8],
    jp_chr: &[u8],
    patched: &[bool],
    overlay_base: usize,
    overlay_tiles: usize,
    palette: PaletteMode,
    tile_palettes: Option<&[PaletteMode]>,
) -> Vec<u8> {
    let mut out = vec![0u8; overlay_tiles * TILE_SIZE_4BPP];
    for i in 0..overlay_tiles {
        let dst = &mut out[i * TILE_SIZE_4BPP..(i + 1) * TILE_SIZE_4BPP];
        if patched[i] {
            let src = &overlay_2bpp[i * TILE_SIZE..(i + 1) * TILE_SIZE];
            let pal = tile_palettes.map_or(palette, |tp| tp[i]);
            for r in 0..8 {
                let glyph_row = src[r * 2]; // BP0 = glyph pattern from 2bpp
                match pal {
                    PaletteMode::StatMagic => {
                        dst[r * 2] = 0x00; // BP0 = $00
                        dst[r * 2 + 1] = glyph_row; // BP1 = glyph
                        dst[TILE_SIZE + r * 2] = 0x00; // BP2 = $00
                        dst[TILE_SIZE + r * 2 + 1] = 0xFF; // BP3 = $FF
                    }
                    PaletteMode::Options => {
                        dst[r * 2] = 0xFF; // BP0 = $FF
                        dst[r * 2 + 1] = !glyph_row; // BP1 = ~glyph
                        dst[TILE_SIZE + r * 2] = 0xFF; // BP2 = $FF
                        dst[TILE_SIZE + r * 2 + 1] = 0xFF; // BP3 = $FF
                    }
                    PaletteMode::OutlinedBody => {
                        // text=0x0D, outline=0x0C, bg=0x0F (idx15)
                        let dilated_row = src[r * 2 + 1];
                        dst[r * 2] = glyph_row | !dilated_row; // BP0
                        dst[r * 2 + 1] = !dilated_row; // BP1
                        dst[TILE_SIZE + r * 2] = 0xFF; // BP2
                        dst[TILE_SIZE + r * 2 + 1] = 0xFF; // BP3
                    }
                    PaletteMode::OutlinedHeaderTop => {
                        if r == 0 {
                            // 0x0C border: BP0=0, BP1=0, BP2=1, BP3=1
                            dst[r * 2] = 0x00;
                            dst[r * 2 + 1] = 0x00;
                            dst[TILE_SIZE + r * 2] = 0xFF;
                            dst[TILE_SIZE + r * 2 + 1] = 0xFF;
                        } else if r == 1 {
                            // 0x0E border: BP0=0, BP1=1, BP2=1, BP3=1
                            dst[r * 2] = 0x00;
                            dst[r * 2 + 1] = 0xFF;
                            dst[TILE_SIZE + r * 2] = 0xFF;
                            dst[TILE_SIZE + r * 2 + 1] = 0xFF;
                        } else {
                            // Normal outlined: text=0x0D, outline=0x0C, bg=0x0A
                            let dilated_row = src[r * 2 + 1];
                            dst[r * 2] = glyph_row;
                            dst[r * 2 + 1] = !dilated_row;
                            dst[TILE_SIZE + r * 2] = dilated_row;
                            dst[TILE_SIZE + r * 2 + 1] = 0xFF;
                        }
                    }
                    PaletteMode::OutlinedHeaderBottom => {
                        if r == 7 {
                            // 0x0C border
                            dst[r * 2] = 0x00;
                            dst[r * 2 + 1] = 0x00;
                            dst[TILE_SIZE + r * 2] = 0xFF;
                            dst[TILE_SIZE + r * 2 + 1] = 0xFF;
                        } else if r == 6 {
                            // 0x0E border
                            dst[r * 2] = 0x00;
                            dst[r * 2 + 1] = 0xFF;
                            dst[TILE_SIZE + r * 2] = 0xFF;
                            dst[TILE_SIZE + r * 2 + 1] = 0xFF;
                        } else {
                            // Normal outlined: text=0x0D, outline=0x0C, bg=0x0A
                            let dilated_row = src[r * 2 + 1];
                            dst[r * 2] = glyph_row;
                            dst[r * 2 + 1] = !dilated_row;
                            dst[TILE_SIZE + r * 2] = dilated_row;
                            dst[TILE_SIZE + r * 2 + 1] = 0xFF;
                        }
                    }
                    PaletteMode::HeaderBorderTop => {
                        // Fixed: row0=0x0C, row1=0x0E, rows2-7=0x0A
                        dst[r * 2] = 0x00; // BP0 = 0
                        if r == 0 {
                            // 0x0C (1100): BP1=0, BP2=1, BP3=1
                            dst[r * 2 + 1] = 0x00;
                            dst[TILE_SIZE + r * 2] = 0xFF;
                            dst[TILE_SIZE + r * 2 + 1] = 0xFF;
                        } else if r == 1 {
                            // 0x0E (1110): BP1=1, BP2=1, BP3=1
                            dst[r * 2 + 1] = 0xFF;
                            dst[TILE_SIZE + r * 2] = 0xFF;
                            dst[TILE_SIZE + r * 2 + 1] = 0xFF;
                        } else {
                            // 0x0A (1010): BP1=1, BP2=0, BP3=1
                            dst[r * 2 + 1] = 0xFF;
                            dst[TILE_SIZE + r * 2] = 0x00;
                            dst[TILE_SIZE + r * 2 + 1] = 0xFF;
                        }
                    }
                    PaletteMode::HeaderBorderBottom => {
                        // Fixed: rows0-5=0x0A, row6=0x0E, row7=0x0C
                        dst[r * 2] = 0x00; // BP0 = 0
                        if r == 7 {
                            // 0x0C (1100): BP1=0, BP2=1, BP3=1
                            dst[r * 2 + 1] = 0x00;
                            dst[TILE_SIZE + r * 2] = 0xFF;
                            dst[TILE_SIZE + r * 2 + 1] = 0xFF;
                        } else if r == 6 {
                            // 0x0E (1110): BP1=1, BP2=1, BP3=1
                            dst[r * 2 + 1] = 0xFF;
                            dst[TILE_SIZE + r * 2] = 0xFF;
                            dst[TILE_SIZE + r * 2 + 1] = 0xFF;
                        } else {
                            // 0x0A (1010): BP1=1, BP2=0, BP3=1
                            dst[r * 2 + 1] = 0xFF;
                            dst[TILE_SIZE + r * 2] = 0x00;
                            dst[TILE_SIZE + r * 2 + 1] = 0xFF;
                        }
                    }
                }
            }
        } else {
            // Unpatched: preserve original JP BP0+BP1
            dst[..TILE_SIZE].copy_from_slice(&overlay_2bpp[i * TILE_SIZE..(i + 1) * TILE_SIZE]);
            // Preserve original JP BP2+BP3
            let bp23_idx = (overlay_base + i) * 2 + 1;
            let bp23_start = bp23_idx * TILE_SIZE;
            if bp23_start + TILE_SIZE <= jp_chr.len() {
                dst[TILE_SIZE..].copy_from_slice(&jp_chr[bp23_start..bp23_start + TILE_SIZE]);
            }
        }
    }
    out
}

// ── CHR hook assembly ───────────────────────────────────────────────

/// Build a hook: JSL $009440 (original LZ decompress), then MVN overlay → WRAM.
fn build_chr_hook(
    overlay_bank: u8,
    overlay_addr: u16,
    overlay_size: u16,
    wram_dest: u16,
) -> Result<Vec<u8>, String> {
    if overlay_size == 0 {
        return Err("overlay_size must be > 0".into());
    }
    use Inst::*;
    let program = vec![
        Jsl(0x009440),
        Php,
        Phb,
        Rep(0x30),
        LdxImm16(overlay_addr),
        LdyImm16(wram_dest),
        LdaImm16(overlay_size - 1),
        Mvn(0x7F, overlay_bank),
        Plb,
        Plp,
        Rtl,
    ];
    assemble(&program)
}

// ── Tilemap remap hook assembly ────────────────────────────────────

/// Build a hook that decompresses a tilemap then patches N entries in WRAM.
///
/// Used for both TM5 (options) and TM6 (stat+magic) remap hooks.
/// Each remap entry preserves the original attribute bits (YXPCCC) and only
/// changes the 10-bit tile index.
fn build_tm_remap_hook(remap_values: &[(u16, u16)]) -> Result<Vec<u8>, String> {
    use Inst::*;
    let mut program = vec![
        Jsl(0x009440), // Original LZ decompress to $7F:$C800
        Php,
        Phb,
        Sep(0x20),     // 8-bit A
        LdaImm8(0x7F), // Data bank = $7F
        Pha,
        Plb,
        Rep(0x20), // 16-bit A
    ];
    for &(wram_addr, new_value) in remap_values {
        program.push(LdaImm16(new_value));
        program.push(StaAbs(wram_addr));
    }
    program.push(Plb);
    program.push(Plp);
    program.push(Rtl);
    assemble(&program)
}

// ── Shared screen hook logic (Phase 2a) ─────────────────────────────

/// Apply KO localization overlay for stat/magic screen (Phase 2a).
///
/// If `remapped_tm` is provided, it is used for tile lookups instead of the
/// decompressed JP tilemap.  This ensures the CHR overlay is built against the
/// same tile indices that will exist at runtime after a TM remap hook fires.
fn apply_screen_hook(
    rom: &mut TrackedRom,
    config: &ScreenHookConfig,
    tiles_8x8: &[[u8; TILE_SIZE]],
    tiles_16x16: &[[[u8; TILE_SIZE]; 4]],
    remapped_tm: Option<&[u8]>,
) -> Result<(), String> {
    // 1. Verify hook site
    if rom[config.hook_pc..config.hook_pc + 4] != JSL_LZ_BYTES {
        return Err(format!(
            "{} hook (PC 0x{:05X}): expected JSL $009440, got {:02X?}",
            config.name,
            config.hook_pc,
            &rom[config.hook_pc..config.hook_pc + 4]
        ));
    }

    // 2. Decompress JP data
    let chr_lz_pc = lookup_lz_source(rom, config.chr_entry_idx)?;
    let (jp_chr, _) = font::decompress_lz(rom, chr_lz_pc)?;
    let tm_lz_pc = lookup_lz_source(rom, config.tm_entry_idx)?;
    let (jp_tm, _) = font::decompress_lz(rom, tm_lz_pc)?;
    println!(
        "  {} CHR: {} 2bpp tiles ({}B), TM: {}B",
        config.name,
        jp_chr.len() / TILE_SIZE,
        jp_chr.len(),
        jp_tm.len(),
    );

    // 3. Collect chars and validate tile counts
    let chars_8 = collect_chars_from_regions(config.region_groups, TileSize::S8x8);
    let chars_16 = collect_chars_from_regions(config.region_groups, TileSize::S16x16);

    if tiles_8x8.len() != chars_8.len() {
        return Err(format!(
            "{}: expected {} 8x8 glyphs, got {}",
            config.name,
            chars_8.len(),
            tiles_8x8.len()
        ));
    }
    if tiles_16x16.len() != chars_16.len() {
        return Err(format!(
            "{}: expected {} 16x16 glyphs, got {}",
            config.name,
            chars_16.len(),
            tiles_16x16.len()
        ));
    }

    let glyphs = GlyphSet {
        chars_8: &chars_8,
        tiles_8: tiles_8x8,
        chars_16: &chars_16,
        tiles_16: tiles_16x16,
    };

    // 4. Compute overlay geometry
    let overlay_base = config.overlay_first.checked_sub(TILE_BASE).ok_or_else(|| {
        format!(
            "{}: overlay_first ${:03X} < TILE_BASE ${:03X}",
            config.name, config.overlay_first, TILE_BASE
        )
    })? as usize;
    let overlay_tiles = (config.overlay_last - config.overlay_first + 1) as usize;

    // 5. Build 2bpp overlay: extract BP0+BP1 from decompressed JP CHR
    let mut overlay = vec![0u8; overlay_tiles * TILE_SIZE];
    for i in 0..overlay_tiles {
        let bp01_idx = (overlay_base + i) * 2;
        let bp01_start = bp01_idx * TILE_SIZE;
        if bp01_start + TILE_SIZE <= jp_chr.len() {
            overlay[i * TILE_SIZE..(i + 1) * TILE_SIZE]
                .copy_from_slice(&jp_chr[bp01_start..bp01_start + TILE_SIZE]);
        } else {
            overlay[i * TILE_SIZE..(i + 1) * TILE_SIZE].copy_from_slice(&BLANK_TILE);
        }
    }

    // 6. Patch text regions into 2bpp overlay
    let effective_tm = remapped_tm.unwrap_or(&jp_tm);
    let mut patched = vec![false; overlay_tiles];
    let mut total_replaced = 0;
    for &regions in config.region_groups {
        total_replaced += apply_regions_overlay(
            &mut overlay,
            regions,
            &|row, col| tilemap_tile_at(effective_tm, row, col),
            &glyphs,
            &mut patched,
            config.overlay_first,
            config.overlay_last,
        )?;
    }
    let patched_count = patched.iter().filter(|&&p| p).count();
    println!(
        "  {} overlay: {} tiles patched (of {} in range)",
        config.name, total_replaced, overlay_tiles,
    );

    // 7. Build 4bpp overlay (Phase 2a always uses StatMagic palette)
    let overlay_4bpp = build_4bpp_overlay(
        &overlay,
        &jp_chr,
        &patched,
        overlay_base,
        overlay_tiles,
        PaletteMode::StatMagic,
        None,
    );
    let overlay_size_4bpp = overlay_4bpp.len();
    println!(
        "  {} 4bpp: {} KO tiles (idx 8/10), {} JP tiles (original)",
        config.name,
        patched_count,
        overlay_tiles - patched_count,
    );

    // 8. Compute ROM layout
    let overlay_rom_addr = ((config.code_addr as usize + HOOK_CODE_SIZE) + 0x0F) & !0x0F;
    let data_end = overlay_rom_addr + overlay_size_4bpp;
    if data_end > 0xFFFF {
        return Err(format!(
            "{}: data end ${:X} exceeds bank boundary",
            config.name, data_end
        ));
    }

    // 9. Build and write hook + overlay
    let overlay_size_u16 = u16::try_from(overlay_size_4bpp).map_err(|_| {
        format!(
            "{}: overlay size {} exceeds u16",
            config.name, overlay_size_4bpp
        )
    })?;
    let hook = build_chr_hook(
        DATA_BANK,
        overlay_rom_addr as u16,
        overlay_size_u16,
        config.wram_dest,
    )?;
    debug_assert_eq!(hook.len(), HOOK_CODE_SIZE);

    let hook_snes = (DATA_BANK as u32) << 16 | config.code_addr as u32;
    println!(
        "  Bank ${:02X}: code ${:04X} ({}B), overlay ${:04X} ({}B), WRAM ${:04X}, end ${:04X}",
        DATA_BANK,
        config.code_addr,
        hook.len(),
        overlay_rom_addr,
        overlay_size_4bpp,
        config.wram_dest,
        data_end,
    );

    let total_region = data_end - config.code_addr as usize;
    let hook_pc = lorom_to_pc(DATA_BANK, config.code_addr);
    {
        let mut r = rom.region_expect(
            hook_pc,
            total_region,
            &format!("options:{}", config.name),
            &Expect::FreeSpace(0xFF),
        );
        r.copy_at(0, &hook);
        let overlay_off = overlay_rom_addr - config.code_addr as usize;
        r.copy_at(overlay_off, &overlay_4bpp);
    }

    // 10. Patch JSL site
    let jsl = [
        0x22u8,
        hook_snes as u8,
        (hook_snes >> 8) as u8,
        (hook_snes >> 16) as u8,
    ];
    rom.write_expect(
        config.hook_pc,
        &jsl,
        &format!("options:{}_jsl", config.name),
        &Expect::Bytes(&JSL_LZ_BYTES),
    );
    println!(
        "  JSL: {} (PC 0x{:05X}) → ${:06X}",
        config.name, config.hook_pc, hook_snes,
    );

    Ok(())
}

// ── Public API ─────────────────────────────────────────────────────

/// Apply stat/magic screen KO localization (Phase 2a).
///
/// NOTE: Despite the name, this patches the Stat+Magic screens (Phase 2a).
/// Hooks $01:$820A: partial overlay (tiles $254–$2E4, 4640B) to WRAM $DA80.
pub fn apply_options_screen_hook(
    rom: &mut TrackedRom,
    tiles_8x8: &[[u8; TILE_SIZE]],
    tiles_16x16: &[[[u8; TILE_SIZE]; 4]],
) -> Result<(), String> {
    // 1. Decompress TM6 and apply remaps for CHR overlay consistency
    let tm_lz_pc = lookup_lz_source(rom, STAT_MAGIC_CONFIG.tm_entry_idx)?;
    let (jp_tm, _) = font::decompress_lz(rom, tm_lz_pc)?;
    let mut remapped_tm = jp_tm.clone();
    for &(row, col, new_tile) in TM6_REMAPS {
        let idx = if col < 32 {
            row * TM_COLS + col
        } else {
            TM_PAGE_ROWS * TM_COLS + row * TM_COLS + (col - 32)
        };
        let byte_off = idx * 2;
        if byte_off + 1 < remapped_tm.len() {
            let orig_entry = u16::from_le_bytes([remapped_tm[byte_off], remapped_tm[byte_off + 1]]);
            let attr_bits = orig_entry & 0xFC00;
            let new_entry = attr_bits | new_tile;
            remapped_tm[byte_off] = new_entry as u8;
            remapped_tm[byte_off + 1] = (new_entry >> 8) as u8;
        }
    }

    // 2. Apply CHR overlay with remapped tilemap
    apply_screen_hook(
        rom,
        &STAT_MAGIC_CONFIG,
        tiles_8x8,
        tiles_16x16,
        Some(&remapped_tm),
    )?;

    // 3. Build and install TM6 remap hook
    let mut tm6_remap_values = Vec::new();
    let tm6_wram_base: u16 = 0xC800; // TM6 decompresses to $7F:$C800
    for &(row, col, new_tile) in TM6_REMAPS {
        let idx = if col < 32 {
            row * TM_COLS + col
        } else {
            TM_PAGE_ROWS * TM_COLS + row * TM_COLS + (col - 32)
        };
        let orig_entry = u16::from_le_bytes([jp_tm[idx * 2], jp_tm[idx * 2 + 1]]);
        let attr_bits = orig_entry & 0xFC00;
        let new_entry = attr_bits | new_tile;
        let wram_addr = tm6_wram_base + (idx as u16) * 2;
        tm6_remap_values.push((wram_addr, new_entry));
    }

    let tm6_hook = build_tm_remap_hook(&tm6_remap_values)?;
    println!(
        "  TM6 remap hook: {} entries, {} bytes",
        TM6_REMAPS.len(),
        tm6_hook.len(),
    );

    // Verify TM6 hook site
    if rom[TM6_HOOK_PC..TM6_HOOK_PC + 4] != JSL_LZ_BYTES {
        return Err(format!(
            "TM6 hook (PC 0x{:05X}): expected JSL $009440, got {:02X?}",
            TM6_HOOK_PC,
            &rom[TM6_HOOK_PC..TM6_HOOK_PC + 4]
        ));
    }

    // Write TM6 hook to Bank $1C
    let tm6_hook_pc = lorom_to_pc(DATA_BANK, TM6_CODE_ADDR);
    rom.region_expect(
        tm6_hook_pc,
        tm6_hook.len(),
        "options:tm6_remap",
        &Expect::FreeSpace(0xFF),
    )
    .copy_at(0, &tm6_hook);

    // Patch TM6 JSL site
    let tm6_hook_snes = (DATA_BANK as u32) << 16 | TM6_CODE_ADDR as u32;
    let jsl_tm6 = [
        0x22u8,
        tm6_hook_snes as u8,
        (tm6_hook_snes >> 8) as u8,
        (tm6_hook_snes >> 16) as u8,
    ];
    rom.write_expect(
        TM6_HOOK_PC,
        &jsl_tm6,
        "options:tm6_jsl",
        &Expect::Bytes(&JSL_LZ_BYTES),
    );
    println!(
        "  JSL: TM6 (PC 0x{:05X}) → ${:06X}, end ${:04X}",
        TM6_HOOK_PC,
        tm6_hook_snes,
        TM6_CODE_ADDR as usize + tm6_hook.len(),
    );

    Ok(())
}

/// Apply options screen KO localization (Phase 2b).
///
/// Hooks $01:$8F46 (CHR) + $01:$8F83 (TM5).
/// Uses direct tile mapping with Options palette (idx 1/15).
pub fn apply_options_phase2b(
    rom: &mut TrackedRom,
    tiles_8x8: &[[u8; TILE_SIZE]],
    tiles_16x16: &[[[u8; TILE_SIZE]; 4]],
) -> Result<(), String> {
    // 1. Verify hook sites
    if rom[OPT_CHR_HOOK_PC..OPT_CHR_HOOK_PC + 4] != JSL_LZ_BYTES {
        return Err(format!(
            "Options CHR hook (PC 0x{:05X}): expected JSL $009440, got {:02X?}",
            OPT_CHR_HOOK_PC,
            &rom[OPT_CHR_HOOK_PC..OPT_CHR_HOOK_PC + 4]
        ));
    }
    if rom[OPT_TM5_HOOK_PC..OPT_TM5_HOOK_PC + 4] != JSL_LZ_BYTES {
        return Err(format!(
            "Options TM5 hook (PC 0x{:05X}): expected JSL $009440, got {:02X?}",
            OPT_TM5_HOOK_PC,
            &rom[OPT_TM5_HOOK_PC..OPT_TM5_HOOK_PC + 4]
        ));
    }

    // 2. Validate glyph counts
    if tiles_8x8.len() != OPT_8X8_CHARS.len() {
        return Err(format!(
            "Options: expected {} 8x8 glyphs, got {}",
            OPT_8X8_CHARS.len(),
            tiles_8x8.len()
        ));
    }
    if tiles_16x16.len() != OPT_16X16_CHARS.len() {
        return Err(format!(
            "Options: expected {} 16x16 glyphs, got {}",
            OPT_16X16_CHARS.len(),
            tiles_16x16.len()
        ));
    }

    // 3. Decompress JP CHR (Entry 3) and TM5 (Entry 5)
    let chr_lz_pc = lookup_lz_source(rom, 3)?;
    let (jp_chr, _) = font::decompress_lz(rom, chr_lz_pc)?;
    let tm_lz_pc = lookup_lz_source(rom, 5)?;
    let (jp_tm, _) = font::decompress_lz(rom, tm_lz_pc)?;
    println!(
        "  Options CHR: {} 2bpp tiles ({}B), TM5: {}B",
        jp_chr.len() / TILE_SIZE,
        jp_chr.len(),
        jp_tm.len(),
    );

    // 4. Build 2bpp overlay from JP CHR BP0+BP1
    let mut overlay = vec![0u8; OPT_CHR_TILES * TILE_SIZE];
    for i in 0..OPT_CHR_TILES {
        let bp01_idx = i * 2;
        let bp01_start = bp01_idx * TILE_SIZE;
        if bp01_start + TILE_SIZE <= jp_chr.len() {
            overlay[i * TILE_SIZE..(i + 1) * TILE_SIZE]
                .copy_from_slice(&jp_chr[bp01_start..bp01_start + TILE_SIZE]);
        } else {
            overlay[i * TILE_SIZE..(i + 1) * TILE_SIZE].copy_from_slice(&BLANK_TILE);
        }
    }

    // 5. Apply direct tile mapping
    let mut patched = vec![false; OPT_CHR_TILES];
    let mut replaced = 0;
    for &(tile_idx, ref content) in OPT_TILE_MAP {
        let rel = tile_idx
            .checked_sub(TILE_BASE)
            .ok_or_else(|| format!("tile ${:03X} < TILE_BASE", tile_idx))?
            as usize;
        if rel >= OPT_CHR_TILES {
            return Err(format!("tile ${:03X} outside CHR range", tile_idx));
        }
        let off = rel * TILE_SIZE;
        let data: &[u8; TILE_SIZE] = match content {
            OptTile::Quad { char_idx, quadrant } => {
                if *char_idx >= tiles_16x16.len() {
                    return Err(format!("16x16 char_idx {} out of range", char_idx));
                }
                &tiles_16x16[*char_idx][*quadrant]
            }
            OptTile::Glyph { char_idx } => {
                if *char_idx >= tiles_8x8.len() {
                    return Err(format!("8x8 char_idx {} out of range", char_idx));
                }
                &tiles_8x8[*char_idx]
            }
            OptTile::Blank => &BLANK_TILE,
        };
        overlay[off..off + TILE_SIZE].copy_from_slice(data);
        patched[rel] = true;
        replaced += 1;
    }
    let patched_count = patched.iter().filter(|&&p| p).count();
    println!(
        "  Options overlay: {} tiles patched (of {} total CHR)",
        replaced, OPT_CHR_TILES,
    );

    // 6. Build 4bpp overlay with per-tile palette
    let mut tile_palettes = vec![PaletteMode::Options; OPT_CHR_TILES];
    // Title top row: $209-$211 (rel 9-17) → HeaderBorderTop
    for p in &mut tile_palettes[9..=17] {
        *p = PaletteMode::HeaderBorderTop;
    }
    // Title bottom row: $214-$21C (rel 20-28) → HeaderBorderBottom
    for p in &mut tile_palettes[20..=28] {
        *p = PaletteMode::HeaderBorderBottom;
    }
    // 16×16 Quad tiles → Outlined with matching background + border
    for &(tile_idx, ref content) in OPT_TILE_MAP {
        if matches!(content, OptTile::Quad { .. }) {
            let rel = (tile_idx - TILE_BASE) as usize;
            if rel < OPT_CHR_TILES {
                tile_palettes[rel] = match tile_palettes[rel] {
                    PaletteMode::HeaderBorderTop => PaletteMode::OutlinedHeaderTop,
                    PaletteMode::HeaderBorderBottom => PaletteMode::OutlinedHeaderBottom,
                    _ => PaletteMode::OutlinedBody,
                };
            }
        }
    }
    let overlay_4bpp = build_4bpp_overlay(
        &overlay,
        &jp_chr,
        &patched,
        0,
        OPT_CHR_TILES,
        PaletteMode::Options,
        Some(&tile_palettes),
    );
    let overlay_size_4bpp = overlay_4bpp.len();
    let outlined_count = tile_palettes
        .iter()
        .zip(patched.iter())
        .filter(|(p, patched)| {
            **patched
                && matches!(
                    **p,
                    PaletteMode::OutlinedBody
                        | PaletteMode::OutlinedHeaderTop
                        | PaletteMode::OutlinedHeaderBottom
                )
        })
        .count();
    let border_count = tile_palettes
        .iter()
        .zip(patched.iter())
        .filter(|(p, patched)| {
            **patched
                && matches!(
                    **p,
                    PaletteMode::HeaderBorderTop | PaletteMode::HeaderBorderBottom
                )
        })
        .count();
    println!(
        "  Options 4bpp: {} body, {} border, {} outlined(0D/0C), {} JP",
        patched_count - border_count - outlined_count,
        border_count,
        outlined_count,
        OPT_CHR_TILES - patched_count,
    );

    // 7. Compute CHR hook ROM layout
    let chr_overlay_rom_addr = ((OPT_CODE_ADDR as usize + HOOK_CODE_SIZE) + 0x0F) & !0x0F;
    let chr_data_end = chr_overlay_rom_addr + overlay_size_4bpp;

    // 8. Build TM5 remap values from decompressed TM5
    let mut remap_values = Vec::new();
    for &(row, col, new_tile) in TM5_REMAPS {
        let orig_entry = tilemap_entry_at(&jp_tm, row, col, TM5_COLS);
        let attr_bits = orig_entry & 0xFC00;
        let new_entry = attr_bits | new_tile;
        let wram_addr = TM5_WRAM_BASE + ((row * TM5_COLS + col) * 2) as u16;
        remap_values.push((wram_addr, new_entry));
    }
    let tm5_hook = build_tm_remap_hook(&remap_values)?;
    let tm5_code_addr = chr_data_end as u16;
    let tm5_data_end = tm5_code_addr as usize + tm5_hook.len();

    if tm5_data_end > 0xFFFF {
        return Err(format!(
            "Options: TM5 hook end ${:X} exceeds bank boundary",
            tm5_data_end
        ));
    }

    // 9. Write CHR hook + overlay + TM5 hook to Bank $1C (single region)
    let total_region_len = tm5_data_end - OPT_CODE_ADDR as usize;
    let overlay_size_u16 = u16::try_from(overlay_size_4bpp)
        .map_err(|_| format!("Options: overlay size {} exceeds u16", overlay_size_4bpp))?;
    let chr_hook = build_chr_hook(
        DATA_BANK,
        chr_overlay_rom_addr as u16,
        overlay_size_u16,
        OPT_WRAM_DEST,
    )?;
    debug_assert_eq!(chr_hook.len(), HOOK_CODE_SIZE);

    let chr_hook_snes = (DATA_BANK as u32) << 16 | OPT_CODE_ADDR as u32;
    let tm5_hook_snes = (DATA_BANK as u32) << 16 | tm5_code_addr as u32;
    println!(
        "  Bank ${:02X}: CHR code ${:04X} ({}B), overlay ${:04X} ({}B), WRAM ${:04X}",
        DATA_BANK,
        OPT_CODE_ADDR,
        chr_hook.len(),
        chr_overlay_rom_addr,
        overlay_size_4bpp,
        OPT_WRAM_DEST,
    );
    println!(
        "  Bank ${:02X}: TM5 code ${:04X} ({}B), end ${:04X}",
        DATA_BANK,
        tm5_code_addr,
        tm5_hook.len(),
        tm5_data_end,
    );

    let base_pc = lorom_to_pc(DATA_BANK, OPT_CODE_ADDR);
    {
        let mut r = rom.region_expect(
            base_pc,
            total_region_len,
            "options:phase2b",
            &Expect::FreeSpace(0xFF),
        );
        r.copy_at(0, &chr_hook);
        r.copy_at(chr_overlay_rom_addr - OPT_CODE_ADDR as usize, &overlay_4bpp);
        r.copy_at(tm5_code_addr as usize - OPT_CODE_ADDR as usize, &tm5_hook);
    }

    // 10. Patch CHR hook site ($01:$8F46)
    let jsl_chr = [
        0x22u8,
        chr_hook_snes as u8,
        (chr_hook_snes >> 8) as u8,
        (chr_hook_snes >> 16) as u8,
    ];
    rom.write_expect(
        OPT_CHR_HOOK_PC,
        &jsl_chr,
        "options:phase2b_chr_jsl",
        &Expect::Bytes(&JSL_LZ_BYTES),
    );
    println!(
        "  JSL: Options CHR (PC 0x{:05X}) → ${:06X}",
        OPT_CHR_HOOK_PC, chr_hook_snes,
    );

    // 11. Patch TM5 hook site ($01:$8F83)
    let jsl_tm5 = [
        0x22u8,
        tm5_hook_snes as u8,
        (tm5_hook_snes >> 8) as u8,
        (tm5_hook_snes >> 16) as u8,
    ];
    rom.write_expect(
        OPT_TM5_HOOK_PC,
        &jsl_tm5,
        "options:phase2b_tm5_jsl",
        &Expect::Bytes(&JSL_LZ_BYTES),
    );
    println!(
        "  JSL: Options TM5 (PC 0x{:05X}) → ${:06X}",
        OPT_TM5_HOOK_PC, tm5_hook_snes,
    );

    Ok(())
}

#[cfg(test)]
#[path = "options_screen_tests.rs"]
mod tests;
