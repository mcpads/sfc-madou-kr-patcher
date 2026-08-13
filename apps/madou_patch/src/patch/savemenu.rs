//! Save menu UI localization (Issue E).
//!
//! The save menu displays screen titles (はじめるよ, うつすよ, けすよ) and button
//! labels as LZ-compressed 8×8 tile graphics, not through the text engine.
//!
//! This module intercepts three `JSL $009440` (LZ decompressor) calls in Bank $02
//! with DMA hooks that load pre-patched uncompressed KO data from Bank $19.
//!
//! ## Data flow
//!
//! ```text
//! Bank $0A pointer table → LZ source (3 blocks)
//!     ├── CHR tiles (8×8 2bpp)
//!     ├── Main tilemap (32×30, slots/buttons)
//!     └── Title tilemap (32×N, screen titles)
//!            │
//!            ▼  decompress_lz() at patch time
//!            │
//!     Modify JP data:
//!     ├── CHR: replace JP tile data with KO glyphs + conflict tiles
//!     ├── Tilemaps: update conflict tile indices
//!            │
//!            ▼  Write to Bank $19 + DMA hook code
//!            │
//!     Bank $02 JSL sites → redirect to DMA hooks
//! ```

use crate::font_gen;
use crate::patch::asm::{compile_jsl, compile_machine_code, ExecutionMode, Inst, MachineCode};
use crate::patch::font;
use crate::patch::hook_common::{self, JSL_LZ_BYTES};
use crate::patch::tracked_rom::{Expect, TrackedRom};
use crate::rom::{lorom_to_pc, pc_to_lorom};

// ── Hook sites (Bank $02, `JSL $009440`) ───────────────────────────

/// (PC offset, description) for each hook site.
const HOOK_SITES: &[(usize, &str)] = &[
    (0x011FA4, "CHR tiles"),     // $02:$9FA4
    (0x011FE0, "Main tilemap"),  // $02:$9FE0
    (0x01204C, "Title tilemap"), // $02:$A04C
];

// ── LZ pointer table (Bank $0A) ────────────────────────────────────

const PTR_TABLE_BANK: u8 = 0x0A;
const PTR_TABLE_PC: usize = 0x50000; // lorom_to_pc(0x0A, 0x8000)
const CHR_PTR_IDX: usize = 0x12;
const MAIN_TM_PTR_IDX: usize = 0x13;
const TITLE_TM_PTR_IDX: usize = 0x16;

// ── Data placement (Bank $19) ──────────────────────────────────────

const DATA_BANK: u8 = 0x19;
const CODE_BASE_ADDR: u16 = 0xD460;

// ── WRAM destinations: (lo, mid, hi) for $2181/$2182/$2183 ────────

const WRAM_CHR: (u8, u8, u8) = (0x00, 0xC0, 0x01); // $7F:$C000
const WRAM_MAIN_TM: (u8, u8, u8) = (0x00, 0xF8, 0x01); // $7F:$F800
const WRAM_TITLE_TM: (u8, u8, u8) = (0x00, 0x18, 0x01); // $7F:$1800

// ── Tile constants ─────────────────────────────────────────────────

const TM_WIDTH: usize = 32;
const TILE_8X8_SIZE: usize = 16; // 8×8 2bpp = 16 bytes

// ── KO character tile mappings ─────────────────────────────────────
//
// Each 16×16 character uses 4 CHR tile indices: [TL, TR, BL, BR].
// JP tile index assignments come from HITL-verified JP ROM analysis.
//
// Title 1: はじめるよ → 시작할게요
// Title 2: うつすよ   → 복사해요
// Title 3: けすよ     → 삭 제
// Buttons: うつす → 복 사, けす → 삭제
// Cancel (うつさない): さ→blank, な→안, い→해 = "복사 안해" ✓
// Cancel (けさない):  さ→제*, な→안, い→해 = "삭제안해" ✓  (* = conflict tiles)
// Slots:   アルル1/2/3 → 아르르1/2/3 — CHR only, tiles shared across 3 slots

/// Primary mappings: KO glyph replaces JP tile data in-place.
/// A char may appear multiple times (e.g., '해' for す and な tiles).
const PRIMARY_CHARS: &[(char, [u16; 4])] = &[
    ('시', [0xD0, 0xD1, 0xE4, 0xE5]), // は
    ('작', [0xD2, 0xD3, 0xE6, 0xE7]), // じ (E7 primary = 작BR)
    ('할', [0xD4, 0xD5, 0xE8, 0xE9]), // め
    ('게', [0xD6, 0xD7, 0xEA, 0xEB]), // る → 게
    ('요', [0xD8, 0xD9, 0xEC, 0xED]), // よ (shared by title 1+2; title 3 uses 제 via conflict)
    ('복', [0xDB, 0xDC, 0xEE, 0xE7]), // う (E7 conflict: 복BR → new tile)
    ('사', [0xDD, 0xDE, 0xEF, 0xF0]), // つ
    ('해', [0xDF, 0xE0, 0xF1, 0xF2]), // す (primary; けす uses 제 via conflict)
    ('삭', [0xE2, 0xE3, 0xF3, 0xF4]), // け → 삭
    // さない cancel buttons
    ('안', [0xFD, 0xFE, 0x10A, 0x10B]),  // な → 안
    ('해', [0xFF, 0x100, 0x10C, 0x10D]), // い → 해 (same glyph, different tiles)
    // アルル1/2/3 → 아르르1/2/3 (CHR only, tiles shared across 3 slots)
    ('아', [0xF5, 0xF6, 0x102, 0x103]), // ア → 아
    ('르', [0xF7, 0xF8, 0x104, 0x105]), // ル → 르 ($F8 conflict: num 1 TR → new tile)
];

/// Tile indices for さ (blank in うつさない cancel: "복사 안해").
const BLANK_TILES: &[[u16; 4]] = &[
    [0xFB, 0xFC, 0x108, 0x109], // さ → blank
];

/// All unique KO characters needed (primary + conflict).
pub const KO_CHARS: &[char] = &[
    '시', '작', '할', '게', '요', '복', '사', '해', '삭', '제', '안', '아', '르',
];

// ── Conflict detection constants ─────────────────────────────────

// E7: shared by じBR(→작) and うBR(→복). Left neighbor determines context.
const CONFLICT_E7_TILE: u16 = 0xE7;
const CONFLICT_E7_U_CONTEXT: u16 = 0xEE; // う BL tile → left of E7 in う block

// す tiles: shared by title2(→해) and title3/button(→제).
const CONFLICT_SU_TL: u16 = 0xDF;

// さない tiles: さ→blank (primary), け context: さ→제 (conflict).
const CONFLICT_SA_TL: u16 = 0xFB;

// け tile indices — if found nearby to the left, triggers け context conflict.
const KE_TILES: [u16; 4] = [0xE2, 0xE3, 0xF3, 0xF4];
// Search range: how many tiles to the left to look for け context.
const CONTEXT_SEARCH_RANGE: usize = 12;
// Forward search range: how far right to look for tiles after a context trigger.
const FORWARD_SEARCH_RANGE: usize = 12;

// つ TL for button copy context detection (main TM: うつす → "복 사")
const TSU_TL: u16 = 0xDD;
// よ TL for title ke-context detection (title TM: けすよ → "삭 제")
const YO_TL: u16 = 0xD8;
// Blanked tile indices (さ tiles, zeroed out) for blank positions
const BLANK_REF: [u16; 4] = [0xFB, 0xFC, 0x108, 0x109];

// $F8: shared by ル TR(→르) and number 1 TR. After patching 르, number 1 TR is broken.
const CONFLICT_F8_TILE: u16 = 0xF8;
const CONFLICT_F8_NUM1_CONTEXT: u16 = 0xF9; // Number 1 TL: left neighbor of $F8 in "1" block

/// All conflict tile info returned by `patch_chr_tiles`.
struct ConflictTiles {
    boku_br_idx: u16,
    je_su: [u16; 4],  // す→제 in main TM けす context
    je_sa: [u16; 4],  // さ→제 in けさない context
    sa_su: [u16; 4],  // す→사 in main TM button copy context (うつす → "복 사")
    je_yo: [u16; 4],  // よ→제 in title TM けすよ context ("삭 제")
    num1_tr_idx: u16, // original $F8 data for number 1 TR (overwritten by 르)
}

// ── LZ pointer lookup (delegated to hook_common) ─────────────────

fn lookup_lz_source(rom: &[u8], idx: usize) -> Result<usize, String> {
    hook_common::lookup_lz_source(rom, PTR_TABLE_BANK, PTR_TABLE_PC, idx)
}

// ── Tilemap helpers ────────────────────────────────────────────────

/// Read 10-bit tile index from a 16-bit tilemap entry.
fn get_tile_index(tilemap: &[u8], entry_idx: usize) -> u16 {
    let off = entry_idx * 2;
    u16::from_le_bytes([tilemap[off], tilemap[off + 1]]) & 0x03FF
}

/// Write 10-bit tile index, preserving upper 6 attribute bits.
fn set_tile_index(tilemap: &mut [u8], entry_idx: usize, new_tile: u16) {
    let off = entry_idx * 2;
    let old = u16::from_le_bytes([tilemap[off], tilemap[off + 1]]);
    let new_word = (old & 0xFC00) | (new_tile & 0x03FF);
    tilemap[off] = new_word as u8;
    tilemap[off + 1] = (new_word >> 8) as u8;
}

// ── Diagnostic dump ────────────────────────────────────────────────

/// Dump all occurrences of save menu character tiles in a tilemap.
fn dump_tilemap_chars(label: &str, tilemap: &[u8]) {
    let entry_count = tilemap.len() / 2;
    let rows = entry_count / TM_WIDTH;

    // Tiles of interest: all JP character tiles from PRIMARY_CHARS
    let char_tiles: std::collections::HashSet<u16> = PRIMARY_CHARS
        .iter()
        .flat_map(|(_, tiles)| tiles.iter().copied())
        .collect();

    println!("  [DIAG] {} ({}×{}):", label, TM_WIDTH, rows);
    for r in 0..rows {
        let mut row_hits: Vec<String> = Vec::new();
        for c in 0..TM_WIDTH {
            let idx = r * TM_WIDTH + c;
            let tile = get_tile_index(tilemap, idx);
            if char_tiles.contains(&tile) {
                row_hits.push(format!("c{:02}=${:03X}", c, tile));
            }
        }
        if !row_hits.is_empty() {
            println!("    row {:02}: {}", r, row_hits.join(" "));
        }
    }
}

// ── CHR tile patching ──────────────────────────────────────────────

/// Replace JP tile data in CHR with KO glyph data and append conflict tiles.
fn patch_chr_tiles(
    chr: &mut Vec<u8>,
    ko_tiles: &[(char, [u8; 64])],
) -> Result<ConflictTiles, String> {
    let tile_map: std::collections::HashMap<char, &[u8; 64]> =
        ko_tiles.iter().map(|(ch, data)| (*ch, data)).collect();

    // Save original $F8 data before Phase 1 overwrites it (shared with number 1 TR)
    let f8_offset = CONFLICT_F8_TILE as usize * TILE_8X8_SIZE;
    let mut orig_f8 = [0u8; TILE_8X8_SIZE];
    if f8_offset + TILE_8X8_SIZE <= chr.len() {
        orig_f8.copy_from_slice(&chr[f8_offset..f8_offset + TILE_8X8_SIZE]);
    }

    // Phase 1: Write primary tile data to existing CHR slots
    for &(ko_ch, ref jp_tiles) in PRIMARY_CHARS {
        let tile_data = tile_map
            .get(&ko_ch)
            .ok_or_else(|| format!("Missing KO glyph for '{}'", ko_ch))?;

        let quadrants: [&[u8]; 4] = [
            &tile_data[0..16],
            &tile_data[16..32],
            &tile_data[32..48],
            &tile_data[48..64],
        ];

        for (qi, &tile_idx) in jp_tiles.iter().enumerate() {
            // Skip E7 for 복 — 작BR is primary owner; 복BR goes to new tile
            if ko_ch == '복' && tile_idx == CONFLICT_E7_TILE {
                continue;
            }

            let offset = tile_idx as usize * TILE_8X8_SIZE;
            if offset + TILE_8X8_SIZE > chr.len() {
                return Err(format!(
                    "CHR tile ${:03X} out of bounds for '{}'",
                    tile_idx, ko_ch
                ));
            }
            chr[offset..offset + TILE_8X8_SIZE].copy_from_slice(quadrants[qi]);
        }
    }

    // Phase 1b: Clear blank tiles (い from さない → 안해)
    for blank in BLANK_TILES {
        for &tile_idx in blank {
            let offset = tile_idx as usize * TILE_8X8_SIZE;
            if offset + TILE_8X8_SIZE <= chr.len() {
                chr[offset..offset + TILE_8X8_SIZE].fill(0x00);
            }
        }
    }

    // Phase 2: Append new tiles for conflicts
    let next_idx = (chr.len() / TILE_8X8_SIZE) as u16;

    // 복BR (new tile replacing shared E7 in う context)
    let boku_tile = tile_map.get(&'복').ok_or("Missing '복' glyph")?;
    let boku_br_idx = next_idx;
    chr.extend_from_slice(&boku_tile[48..64]); // BR quadrant

    /// Append 4 quadrant tiles for a glyph, returning [TL,TR,BL,BR] indices.
    fn append_glyph(chr_buf: &mut Vec<u8>, glyph: &[u8; 64], base: u16) -> [u16; 4] {
        chr_buf.extend_from_slice(&glyph[0..16]);
        chr_buf.extend_from_slice(&glyph[16..32]);
        chr_buf.extend_from_slice(&glyph[32..48]);
        chr_buf.extend_from_slice(&glyph[48..64]);
        [base, base + 1, base + 2, base + 3]
    }

    let je_tile = tile_map.get(&'제').ok_or("Missing '제' glyph")?;
    let sa_tile = tile_map.get(&'사').ok_or("Missing '사' glyph")?;

    // Conflict 1: す→제 in main TM けす context (4 tiles)
    let mut idx = next_idx + 1;
    let je_su = append_glyph(chr, je_tile, idx);
    idx += 4;

    // Conflict 2: さ→제 in けさない context (4 tiles)
    let je_sa = append_glyph(chr, je_tile, idx);
    idx += 4;

    // Conflict 3: す→사 in main TM button copy context (4 tiles)
    let sa_su = append_glyph(chr, sa_tile, idx);
    idx += 4;

    // Conflict 4: よ→제 in title TM けすよ context (4 tiles)
    let je_yo = append_glyph(chr, je_tile, idx);

    // Conflict 5: number 1 TR (original $F8 data, overwritten by 르 TR)
    let num1_tr_idx = (chr.len() / TILE_8X8_SIZE) as u16;
    chr.extend_from_slice(&orig_f8);

    Ok(ConflictTiles {
        boku_br_idx,
        je_su,
        je_sa,
        sa_su,
        je_yo,
        num1_tr_idx,
    })
}

// ── Tilemap conflict resolution ────────────────────────────────────

/// Scan tilemap and replace conflict tile indices with new tile indices.
///
/// Pattern-based conflicts (works for both main and title tilemaps):
/// 1. E7 tile in う context (left neighbor = EE) → 복BR new tile
/// 2. F8 tile in number 1 context (left neighbor = F9) → original num1 TR
/// 3. す (DF) in け context + よ ahead → blank す + よ→제 (title: "삭 제")
///    す (DF) in け context, no よ → す→제 (button: "삭제")
/// 4. さ (FB) in け context → 제
/// 5. つ (DD) + す in non-け context, no よ after す → blank つ + す→사 (button: "복 사")
///    つ (DD) + す + よ after す → no change (title: "복사해요")
fn resolve_tilemap_conflicts(tilemap: &mut [u8], ct: &ConflictTiles) -> usize {
    let entry_count = tilemap.len() / 2;
    let rows = entry_count / TM_WIDTH;
    let mut replaced = 0;

    /// Check if け tiles exist within `range` columns to the left of `col`.
    fn has_ke_context(tilemap: &[u8], row_base: usize, col: usize) -> bool {
        let search_start = col.saturating_sub(CONTEXT_SEARCH_RANGE);
        (search_start..col).rev().any(|c| {
            let t = get_tile_index(tilemap, row_base + c);
            KE_TILES.contains(&t)
        })
    }

    /// Replace a 2×2 tile block (TL at `i`) with new indices.
    fn replace_block(
        tilemap: &mut [u8],
        i: usize,
        col: usize,
        row: usize,
        rows: usize,
        new: [u16; 4],
    ) -> usize {
        set_tile_index(tilemap, i, new[0]);
        if col + 1 < TM_WIDTH {
            set_tile_index(tilemap, i + 1, new[1]);
        }
        if row + 1 < rows {
            set_tile_index(tilemap, i + TM_WIDTH, new[2]);
            if col + 1 < TM_WIDTH {
                set_tile_index(tilemap, i + TM_WIDTH + 1, new[3]);
            }
        }
        4
    }

    /// Find a tile TL in the same row, searching forward from `start_col`.
    fn find_tile_forward(
        tilemap: &[u8],
        row_base: usize,
        start_col: usize,
        target_tl: u16,
    ) -> Option<usize> {
        let end = (start_col + FORWARD_SEARCH_RANGE).min(TM_WIDTH);
        (start_col..end).find(|&c| get_tile_index(tilemap, row_base + c) == target_tl)
    }

    for i in 0..entry_count {
        let tile = get_tile_index(tilemap, i);
        let col = i % TM_WIDTH;
        let row = i / TM_WIDTH;
        let row_base = row * TM_WIDTH;

        // Conflict 1: E7 in う context (left = EE) → 복BR
        if tile == CONFLICT_E7_TILE && col > 0 {
            let left = get_tile_index(tilemap, i - 1);
            if left == CONFLICT_E7_U_CONTEXT {
                set_tile_index(tilemap, i, ct.boku_br_idx);
                replaced += 1;
            }
        }

        // Conflict 2: $F8 in number 1 context (left = $F9) → original num1 TR tile
        if tile == CONFLICT_F8_TILE && col > 0 {
            let left = get_tile_index(tilemap, i - 1);
            if left == CONFLICT_F8_NUM1_CONTEXT {
                set_tile_index(tilemap, i, ct.num1_tr_idx);
                replaced += 1;
            }
        }

        // Conflict 3: す TL (DF) in け context — pattern matching
        if tile == CONFLICT_SU_TL && has_ke_context(tilemap, row_base, col) {
            if let Some(yo_col) = find_tile_forward(tilemap, row_base, col + 2, YO_TL) {
                // けすよ pattern (title: "삭 제"): blank す + remap よ→제
                replaced += replace_block(tilemap, i, col, row, rows, BLANK_REF);
                replaced += replace_block(tilemap, row_base + yo_col, yo_col, row, rows, ct.je_yo);
            } else {
                // けす pattern (button: "삭제"): す→제
                replaced += replace_block(tilemap, i, col, row, rows, ct.je_su);
            }
        }

        // Conflict 4: さ TL (FB) in け context → 제 (no cascade)
        if tile == CONFLICT_SA_TL && has_ke_context(tilemap, row_base, col) {
            replaced += replace_block(tilemap, i, col, row, rows, ct.je_sa);
        }

        // Conflict 5: つ TL (DD) in non-け context + す ahead (no よ after す)
        // Button copy pattern: うつす → "복 사"
        // Does NOT fire for うつすよ (title 2: "복사해요")
        if tile == TSU_TL && !has_ke_context(tilemap, row_base, col) {
            if let Some(su_col) = find_tile_forward(tilemap, row_base, col + 2, CONFLICT_SU_TL) {
                if !has_ke_context(tilemap, row_base, su_col) {
                    let has_yo = find_tile_forward(tilemap, row_base, su_col + 2, YO_TL).is_some();
                    if !has_yo {
                        replaced += replace_block(tilemap, i, col, row, rows, BLANK_REF);
                        replaced +=
                            replace_block(tilemap, row_base + su_col, su_col, row, rows, ct.sa_su);
                    }
                }
            }
        }
    }

    replaced
}

// ── DMA hook code ──────────────────────────────────────────────────

/// Assemble a single DMA hook: ROM → WRAM via DMA channel 5.
fn build_dma_hook_at(
    code_addr: u16,
    wram: (u8, u8, u8),
    src_bank: u8,
    src_addr: u16,
    size: u16,
) -> Result<MachineCode, String> {
    use Inst::*;
    let program = vec![
        Sep(0x20), // 8-bit A
        // DMA ch5 params
        LdaImm8(0x00),
        StaAbs(0x4350), // transfer mode
        LdaImm8(0x80),
        StaAbs(0x4351), // B-bus $2180 (WRAM data)
        // WRAM address
        LdaImm8(wram.0),
        StaAbs(0x2181), // lo
        LdaImm8(wram.1),
        StaAbs(0x2182), // mid
        LdaImm8(wram.2),
        StaAbs(0x2183), // hi
        // ROM source
        LdaImm8(src_addr as u8),
        StaAbs(0x4352), // lo
        LdaImm8((src_addr >> 8) as u8),
        StaAbs(0x4353), // hi
        LdaImm8(src_bank),
        StaAbs(0x4354), // bank
        // Transfer size
        LdaImm8(size as u8),
        StaAbs(0x4355), // lo
        LdaImm8((size >> 8) as u8),
        StaAbs(0x4356), // hi
        // Trigger DMA ch5
        LdaImm8(0x20),
        StaAbs(0x420B),
        Rtl,
    ];
    compile_machine_code(program, DATA_BANK, code_addr, ExecutionMode::M8X16)
}

#[cfg(test)]
fn build_dma_hook(
    wram: (u8, u8, u8),
    src_bank: u8,
    src_addr: u16,
    size: u16,
) -> Result<Vec<u8>, String> {
    build_dma_hook_at(CODE_BASE_ADDR, wram, src_bank, src_addr, size)
        .map(|code| code.bytes().to_vec())
}

// ── Internal helpers ──────────────────────────────────────────────

/// Verify all hook sites contain the expected `JSL $009440` bytes.
fn verify_hook_sites(rom: &[u8]) -> Result<(), String> {
    for &(pc, desc) in HOOK_SITES {
        if rom[pc..pc + 4] != JSL_LZ_BYTES {
            return Err(format!(
                "Save menu hook site '{}' (PC 0x{:05X}): expected JSL $009440, found {:02X?}",
                desc,
                pc,
                &rom[pc..pc + 4]
            ));
        }
    }
    Ok(())
}

/// Decompressed JP save menu data: (CHR tiles, main tilemap, title tilemap).
type JpBlocks = (Vec<u8>, Vec<u8>, Vec<u8>);

/// Decompress JP LZ data for all 3 blocks (CHR, main tilemap, title tilemap).
fn decompress_jp_data(rom: &[u8]) -> Result<JpBlocks, String> {
    let chr_lz_pc = lookup_lz_source(rom, CHR_PTR_IDX)?;
    let main_tm_lz_pc = lookup_lz_source(rom, MAIN_TM_PTR_IDX)?;
    let title_tm_lz_pc = lookup_lz_source(rom, TITLE_TM_PTR_IDX)?;

    let (chr_data, _) = font::decompress_lz(rom, chr_lz_pc)?;
    let (main_tm_data, _) = font::decompress_lz(rom, main_tm_lz_pc)?;
    let (title_tm_data, _) = font::decompress_lz(rom, title_tm_lz_pc)?;

    println!(
        "  JP data: CHR {} tiles ({}B), Main TM {}B, Title TM {}B",
        chr_data.len() / TILE_8X8_SIZE,
        chr_data.len(),
        main_tm_data.len(),
        title_tm_data.len()
    );
    dump_tilemap_chars("Title TM", &title_tm_data);
    dump_tilemap_chars("Main TM", &main_tm_data);

    Ok((chr_data, main_tm_data, title_tm_data))
}

/// Patch CHR and tilemaps with KO glyphs, then write to Bank $19.
fn patch_and_write(
    rom: &mut TrackedRom,
    mut chr_data: Vec<u8>,
    mut main_tm_data: Vec<u8>,
    mut title_tm_data: Vec<u8>,
    ko_tiles: &[(char, [u8; 64])],
) -> Result<(), String> {
    let orig_chr_tiles = chr_data.len() / TILE_8X8_SIZE;

    let ct = patch_chr_tiles(&mut chr_data, ko_tiles)?;
    let new_chr_tiles = chr_data.len() / TILE_8X8_SIZE;
    println!(
        "  CHR: {} → {} tiles (+{} conflict)",
        orig_chr_tiles,
        new_chr_tiles,
        new_chr_tiles - orig_chr_tiles,
    );

    let title_fixes = resolve_tilemap_conflicts(&mut title_tm_data, &ct);
    let main_fixes = resolve_tilemap_conflicts(&mut main_tm_data, &ct);
    println!(
        "  Tilemap fixes: {} title + {} main entries",
        title_fixes, main_fixes
    );

    write_bank19_data(rom, &chr_data, &main_tm_data, &title_tm_data)
}

// ── Public API ─────────────────────────────────────────────────────

/// Apply save menu KO localization hooks to the ROM (renders internally via fontdue).
///
/// Prefer `apply_savemenu_hook_with_tiles` for pre-rendered tiles (PIL or fontdue).
#[allow(dead_code)]
pub fn apply_savemenu_hook(
    rom: &mut TrackedRom,
    ttf_data: &[u8],
    ttf_size: f32,
) -> Result<(), String> {
    verify_hook_sites(rom)?;
    let (chr_data, main_tm_data, title_tm_data) = decompress_jp_data(rom)?;

    let ko_tile_data = font_gen::render_savemenu_tiles(ttf_data, ttf_size, KO_CHARS)?;
    let ko_tiles: Vec<(char, [u8; 64])> = KO_CHARS.iter().copied().zip(ko_tile_data).collect();
    println!("  Rendered {} KO glyphs (Color 2)", ko_tiles.len());

    patch_and_write(rom, chr_data, main_tm_data, title_tm_data, &ko_tiles)
}

/// Apply save menu hook with pre-rendered KO tiles (PIL or fontdue).
pub fn apply_savemenu_hook_with_tiles(
    rom: &mut TrackedRom,
    ko_tile_data: &[[u8; 64]],
) -> Result<(), String> {
    verify_hook_sites(rom)?;
    let (chr_data, main_tm_data, title_tm_data) = decompress_jp_data(rom)?;

    let ko_tiles: Vec<(char, [u8; 64])> = KO_CHARS
        .iter()
        .copied()
        .zip(ko_tile_data.iter().copied())
        .collect();
    println!(
        "  Using {} pre-rendered KO glyphs (Color 2)",
        ko_tiles.len()
    );

    patch_and_write(rom, chr_data, main_tm_data, title_tm_data, &ko_tiles)
}

/// Write modified CHR + tilemap data to Bank $19 and patch JSL sites.
///
/// Shared by both `apply_savemenu_hook` and `apply_savemenu_hook_with_tiles`.
fn write_bank19_data(
    rom: &mut TrackedRom,
    chr_data: &[u8],
    main_tm_data: &[u8],
    title_tm_data: &[u8],
) -> Result<(), String> {
    let hooks_info = [
        (WRAM_CHR, chr_data, "CHR"),
        (WRAM_MAIN_TM, main_tm_data, "Main TM"),
        (WRAM_TITLE_TM, title_tm_data, "Title TM"),
    ];

    // First pass: compute data addresses
    let mut hook_codes: Vec<MachineCode> = Vec::new();
    let mut code_offset = CODE_BASE_ADDR;
    for _ in &hooks_info {
        let dummy = build_dma_hook_at(code_offset, (0, 0, 0), 0, 0, 0)?;
        let hook_len = dummy.len() as u16;
        code_offset += hook_len;
        hook_codes.push(dummy);
    }
    let data_start = (code_offset + 0x0F) & !0x0F;

    let mut data_addr = data_start;
    let mut data_addrs: Vec<(u8, u16, u16)> = Vec::new();
    for (_, data, _) in &hooks_info {
        let size = data.len() as u16;
        data_addrs.push((DATA_BANK, data_addr, size));
        data_addr += size;
    }

    let data_end = data_addr;
    if data_end < data_start {
        return Err("Save menu data wraps past bank boundary".to_string());
    }
    let total_size = (data_end - CODE_BASE_ADDR) as usize;
    let base_pc = lorom_to_pc(DATA_BANK, CODE_BASE_ADDR);
    if base_pc + total_size > rom.len() {
        return Err(format!(
            "Save menu data at PC 0x{:05X}+{} exceeds ROM bounds",
            base_pc, total_size
        ));
    }
    println!(
        "  Bank $19 layout: code ${:04X}-${:04X}, data ${:04X}-${:04X} ({} bytes total)",
        CODE_BASE_ADDR,
        data_start - 1,
        data_start,
        data_end - 1,
        total_size
    );

    // Second pass: build actual hook code
    hook_codes.clear();
    let mut code_addr = CODE_BASE_ADDR;
    let mut hook_addrs: Vec<u32> = Vec::new();
    for (i, (wram, _, _)) in hooks_info.iter().enumerate() {
        let (src_bank, src_addr, size) = data_addrs[i];
        let hook = build_dma_hook_at(code_addr, *wram, src_bank, src_addr, size)?;
        hook_addrs.push((DATA_BANK as u32) << 16 | code_addr as u32);
        code_addr += hook.len() as u16;
        hook_codes.push(hook);
    }

    // Write typed hooks and their payloads as separate owners.
    for (index, hook) in hook_codes.iter().enumerate() {
        rom.write_machine_code_expect(
            hook,
            &format!("savemenu:dma_hook_{index}"),
            &Expect::FreeSpace(0xFF),
        );
    }
    for (i, (_, data, desc)) in hooks_info.iter().enumerate() {
        let (_, addr, _) = data_addrs[i];
        let off = (addr - CODE_BASE_ADDR) as usize;
        rom.write_snes_expect(
            DATA_BANK,
            addr,
            data,
            &format!("savemenu:data_{i}"),
            &Expect::FreeSpace(0xFF),
        );
        println!(
            "  {} → ${:02X}:${:04X} (PC 0x{:05X}, {} bytes)",
            desc,
            DATA_BANK,
            addr,
            base_pc + off,
            data.len()
        );
    }

    // Patch Bank $02 JSL sites
    for (i, &(site_pc, desc)) in HOOK_SITES.iter().enumerate() {
        let target = hook_addrs[i];
        let site = pc_to_lorom(site_pc);
        let jsl = compile_jsl(site.bank, site.addr, target, ExecutionMode::M8X16)?;
        rom.write_machine_code_expect(&jsl, "savemenu:jsl_patch", &Expect::Bytes(&JSL_LZ_BYTES));
        println!(
            "  JSL patch: {} (PC 0x{:05X}) → ${:06X}",
            desc, site_pc, target
        );
    }

    Ok(())
}

#[cfg(test)]
#[path = "savemenu_tests.rs"]
mod tests;
