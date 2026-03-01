//! Font patching: 16x16 tiles (Bank $0F) and LZ compression utilities.
//!
//! Matches the logic in scripts/build_test_rom.py.

use crate::patch::tracked_rom::TrackedRom;
use crate::rom::lorom_to_pc;

// ── ROM layout constants ─────────────────────────────────────────

/// 16x16 font tiles (FB prefix kanji) in Bank $0F.
const FONT_16X16_BANK: u8 = 0x0F;
const FONT_16X16_START: u16 = 0xC000;
const FONT_16X16_TILE_SIZE: usize = 64;

// ── LZ compression (game-compatible format) ─────────────────────
//
// Format used by decompressor at $00:$9440:
//   $00:         end of stream
//   $01-$7F (N): literal run of N bytes (data follows)
//   $80-$FF:     back-reference
//                  length = (control & 0x7F) + 3  (range: 3-130)
//                  next byte = displacement D      (range: 0-255)
//                  source = output_pos - D - 1     (max 256 bytes back)

/// Decompress the game's LZ format from a byte slice.
#[allow(dead_code)]
pub fn decompress_lz(data: &[u8], start: usize) -> Result<(Vec<u8>, usize), String> {
    let mut output = Vec::new();
    let mut pos = start;

    loop {
        if pos >= data.len() {
            return Err("LZ: unexpected end of data".to_string());
        }
        let ctrl = data[pos];
        pos += 1;

        if ctrl == 0x00 {
            break;
        } else if ctrl <= 0x7F {
            let n = ctrl as usize;
            if pos + n > data.len() {
                return Err(format!("LZ: literal run {} exceeds data at {}", n, pos));
            }
            output.extend_from_slice(&data[pos..pos + n]);
            pos += n;
        } else {
            let length = ((ctrl & 0x7F) as usize) + 3;
            if pos >= data.len() {
                return Err("LZ: missing displacement byte".to_string());
            }
            let disp = data[pos] as usize;
            pos += 1;
            if output.len() < disp + 1 {
                return Err(format!(
                    "LZ: back-ref displacement {} exceeds output {}",
                    disp,
                    output.len()
                ));
            }
            let src = output.len() - disp - 1;
            for j in 0..length {
                output.push(output[src + j]);
            }
        }
    }

    Ok((output, pos - start))
}

/// Compress data using the game's LZ format.
#[allow(dead_code)]
pub fn compress_lz(data: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut pos = 0;

    while pos < data.len() {
        // Try to find a back-reference match
        let mut best_len = 0usize;
        let mut best_disp = 0usize;

        let search_limit = pos.min(256);
        for d in 0..search_limit {
            let src = pos - d - 1;
            let mut match_len = 0;
            while match_len < 130
                && pos + match_len < data.len()
                && data[src + match_len] == data[pos + match_len]
            {
                match_len += 1;
            }
            if match_len > best_len {
                best_len = match_len;
                best_disp = d;
            }
        }

        if best_len >= 3 {
            output.push(0x80 | ((best_len - 3) as u8));
            output.push(best_disp as u8);
            pos += best_len;
        } else {
            // Collect literal bytes (up to 127)
            let lit_start = pos;
            let mut lit_end = pos;

            while lit_end < data.len() && lit_end - lit_start < 127 {
                let out_pos = lit_end;
                let mut found_ref = false;
                let search = out_pos.min(256);
                for d in 0..search {
                    let src = out_pos - d - 1;
                    let mut ml = 0;
                    while ml < 130
                        && lit_end + ml < data.len()
                        && data[src + ml] == data[lit_end + ml]
                    {
                        ml += 1;
                    }
                    if ml >= 4 {
                        found_ref = true;
                        break;
                    }
                }
                if found_ref {
                    break;
                }
                lit_end += 1;
            }

            let lit_count = (lit_end - lit_start).max(1);
            output.push(lit_count as u8);
            output.extend_from_slice(&data[lit_start..lit_start + lit_count]);
            pos = lit_start + lit_count;
        }
    }

    output.push(0x00); // end marker
    output
}

// ── FIXED_ENCODE tiles: char $00-$1F → $0F:$8000-$87FF ──────────
//
// These 32 tiles cover chars that FIXED_ENCODE maps directly (digits,
// punctuation, arrows, blank render, etc.).  The game renderer reads
// $0F:$8000 + char_code × 64, so char $00 = $0F:$8000.

const FIXED_ENCODE_BANK: u8 = 0x0F;
const FIXED_ENCODE_START: u16 = 0x8000;
const FIXED_ENCODE_TILE_COUNT: usize = 32; // char $00-$1F
const FIXED_ENCODE_SIZE: usize = FIXED_ENCODE_TILE_COUNT * FONT_16X16_TILE_SIZE; // 2048

/// Patch the 32 FIXED_ENCODE tiles (char $00-$1F) into $0F:$8000-$87FF.
///
/// `fixed_data` must be exactly 2048 bytes (32 tiles × 64 bytes each).
pub fn patch_fixed_encode(rom: &mut TrackedRom, fixed_data: &[u8]) -> Result<usize, String> {
    if fixed_data.len() != FIXED_ENCODE_SIZE {
        return Err(format!(
            "Fixed-encode font must be {} bytes (32 tiles × 64), got {}",
            FIXED_ENCODE_SIZE,
            fixed_data.len()
        ));
    }

    let pc = lorom_to_pc(FIXED_ENCODE_BANK, FIXED_ENCODE_START);
    let end = pc + FIXED_ENCODE_SIZE;
    if end > rom.len() {
        return Err("Fixed-encode font data exceeds ROM size".to_string());
    }

    rom.write(pc, fixed_data, "font:fixed_encode");
    println!(
        "  Fixed-encode tiles: {} tiles ({} bytes) at ${:02X}:${:04X}-${:04X}",
        FIXED_ENCODE_TILE_COUNT,
        FIXED_ENCODE_SIZE,
        FIXED_ENCODE_BANK,
        FIXED_ENCODE_START,
        FIXED_ENCODE_START as usize + FIXED_ENCODE_SIZE - 1
    );

    Ok(FIXED_ENCODE_TILE_COUNT)
}

/// Single-byte font tiles: char codes $20-$EF → ROM $0F:$8800-$BBFF.
/// Game renderer reads $0F:$8000 + char_code × 64, so char $20 = offset $800.
const FONT_16X16_SINGLE_START: u16 = 0x8800; // $8000 + $20 * 64
const FONT_16X16_SINGLE_COUNT: usize = 208; // $20-$EF = 208 chars

/// Patch 16x16 font tiles into Bank $0F.
///
/// Font binary layout (tiles in charset frequency order):
///   - tiles 0-207:   single-byte chars ($20-$EF)  → $0F:$8800-$BBFF
///   - tiles 208-463:  FB-prefix chars (FB $00-$FF) → $0F:$C000-$FFFF
///
/// Game text renderer ($02:$A9A0) reads DB=$0F at $8000 + char_code × 64.
pub fn patch_16x16(rom: &mut TrackedRom, font_data: &[u8]) -> Result<usize, String> {
    let total_tiles = font_data.len() / FONT_16X16_TILE_SIZE;
    let fb_start_tile = 208;

    if fb_start_tile > total_tiles {
        return Err(format!(
            "Not enough tiles in font binary (need >=208, have {})",
            total_tiles
        ));
    }

    let mut patched = 0;

    // 1. Single-byte tiles: font binary [0..208) → $0F:$8800-$BBFF
    let single_count = total_tiles.min(FONT_16X16_SINGLE_COUNT);
    let single_size = single_count * FONT_16X16_TILE_SIZE;
    let single_data = &font_data[..single_size];

    let single_pc = lorom_to_pc(FONT_16X16_BANK, FONT_16X16_SINGLE_START);
    let single_end = single_pc + single_size;
    if single_end > rom.len() {
        return Err("Single-byte font data exceeds ROM size".to_string());
    }
    rom.write(single_pc, single_data, "font:single_byte");
    patched += single_count;
    println!(
        "  Single-byte tiles: {} tiles ({} bytes) at ${:02X}:${:04X}-${:04X}",
        single_count,
        single_size,
        FONT_16X16_BANK,
        FONT_16X16_SINGLE_START,
        FONT_16X16_SINGLE_START as usize + single_size - 1
    );

    // 2. FB-prefix tiles: font binary [208..464) → $0F:$C000-$FFFF
    //    Pre-zero blank FB slots so patch_fb_blank_remap doesn't need to
    //    re-write the same region (avoids tracker collision).
    if fb_start_tile < total_tiles {
        let fb_tile_count = (total_tiles - fb_start_tile).min(256);
        let fb_offset = fb_start_tile * FONT_16X16_TILE_SIZE;
        let fb_end = fb_offset + fb_tile_count * FONT_16X16_TILE_SIZE;
        let mut fb_data = font_data[fb_offset..fb_end].to_vec();

        // Pre-zero the 12 blank FB slots that will be remapped to F0
        for &fb_slot in FB_BLANK_SLOTS {
            let tile_offset = fb_slot as usize * FONT_16X16_TILE_SIZE;
            if tile_offset + FONT_16X16_TILE_SIZE <= fb_data.len() {
                fb_data[tile_offset..tile_offset + FONT_16X16_TILE_SIZE].fill(0);
            }
        }

        let write_pc = lorom_to_pc(FONT_16X16_BANK, FONT_16X16_START);
        let end = write_pc + fb_data.len();
        if end > rom.len() {
            return Err("FB-prefix font data exceeds ROM size".to_string());
        }
        rom.write(write_pc, &fb_data, "font:fb_prefix");
        patched += fb_tile_count;
        println!(
            "  FB-prefix tiles: {} tiles ({} bytes) at ${:02X}:${:04X}-${:04X}",
            fb_tile_count,
            fb_data.len(),
            FONT_16X16_BANK,
            FONT_16X16_START,
            FONT_16X16_START as usize + fb_data.len() - 1
        );
    }

    Ok(patched)
}

// ── FA/F0 prefix font tiles (Bank $32) ──────────────────────────

/// FA prefix tiles: ko_font.bin [464..720) → Bank $32:$8000-$BFFF
const FA_FONT_BANK: u8 = 0x32;
const FA_FONT_START: u16 = 0x8000;
const FA_TILE_OFFSET: usize = 464; // first FA tile index in ko_font.bin
const FA_TILE_COUNT: usize = 256;

/// F0 prefix tiles: ko_font.bin [720..N) → Bank $32:$C000+
const F0_FONT_START: u16 = 0xC000;
const F0_TILE_OFFSET: usize = 720; // first F0 tile index in ko_font.bin

/// Patch FA/F0 prefix font tiles into Bank $32.
///
/// Font binary layout (tiles in charset frequency order):
///   - tiles 464-719: FA-prefix chars (FA $00-$FF) → $32:$8000-$BFFF
///   - tiles 720+:    F0-prefix chars (F0 $00+)    → $32:$C000+
///
/// The engine hooks redirect the renderer to Bank $32 for page 2/3 tiles.
/// Returns the number of F0 tiles written (for dynamic address chain).
pub fn patch_fa_f0(rom: &mut TrackedRom, font_data: &[u8]) -> Result<usize, String> {
    let total_tiles = font_data.len() / FONT_16X16_TILE_SIZE;

    // FA prefix tiles
    if FA_TILE_OFFSET + FA_TILE_COUNT > total_tiles {
        return Err(format!(
            "Not enough FA tiles in font binary (need {}, have {})",
            FA_TILE_OFFSET + FA_TILE_COUNT,
            total_tiles
        ));
    }

    let fa_data_offset = FA_TILE_OFFSET * FONT_16X16_TILE_SIZE;
    let fa_size = FA_TILE_COUNT * FONT_16X16_TILE_SIZE;
    let fa_data = &font_data[fa_data_offset..fa_data_offset + fa_size];

    let fa_pc = lorom_to_pc(FA_FONT_BANK, FA_FONT_START);
    let fa_end = fa_pc + fa_size;
    if fa_end > rom.len() {
        return Err("FA font data exceeds ROM size".to_string());
    }
    rom.write(fa_pc, fa_data, "font:fa_tiles");
    println!(
        "  FA-prefix tiles: {} tiles ({} bytes) at ${:02X}:${:04X}-${:04X}",
        FA_TILE_COUNT,
        fa_size,
        FA_FONT_BANK,
        FA_FONT_START,
        FA_FONT_START as usize + fa_size - 1
    );

    // F0 prefix tiles (no cap — actual count from charset)
    let f0_count = total_tiles.saturating_sub(F0_TILE_OFFSET);

    if f0_count > 0 {
        let f0_end_addr = F0_FONT_START as usize + f0_count * FONT_16X16_TILE_SIZE;
        if f0_end_addr > 0x10000 {
            return Err(format!(
                "F0 tiles exceed Bank $32 bounds: {} tiles → end ${:04X}",
                f0_count, f0_end_addr
            ));
        }

        let f0_data_offset = F0_TILE_OFFSET * FONT_16X16_TILE_SIZE;
        let f0_size = f0_count * FONT_16X16_TILE_SIZE;
        let f0_data = &font_data[f0_data_offset..f0_data_offset + f0_size];

        let f0_pc = lorom_to_pc(FA_FONT_BANK, F0_FONT_START);
        let f0_end = f0_pc + f0_size;
        if f0_end > rom.len() {
            return Err("F0 font data exceeds ROM size".to_string());
        }
        rom.write(f0_pc, f0_data, "font:f0_tiles");
        println!(
            "  F0-prefix tiles: {} tiles ({} bytes) at ${:02X}:${:04X}-${:04X}",
            f0_count,
            f0_size,
            FA_FONT_BANK,
            F0_FONT_START,
            F0_FONT_START as usize + f0_size - 1
        );
    }

    Ok(f0_count)
}

// ── FB blank slot remap ─────────────────────────────────────────
//
// JP ROM has 12 all-zero tile slots in the FB prefix range.  The game engine
// references these indices for blank background tiles.  If our KO font fills
// them with glyphs, those glyphs bleed into the game background .
//
// Fix: zero out the 12 FB positions in Bank $0F and copy the displaced KO
// glyphs to F0 prefix positions in Bank $32 instead.

/// FB blank slots: game에서 배경 타일로 사용하는 12개 FB 인덱스.
/// 이 슬롯의 글리프는 F0 prefix로 리맵됨.
/// font_gen.rs에서도 import하여 사용 (SSOT).
pub const FB_BLANK_SLOTS: &[u8] = &[
    0xE4, 0xF4, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA, 0xFB, 0xFC, 0xFD, 0xFE, 0xFF,
];

/// Zero out 12 blank FB tile slots in Bank $0F and copy the KO glyphs
/// to F0 prefix positions in Bank $32.
///
/// `f0_count`: number of charset F0 tiles already written by `patch_fa_f0()`.
/// Remap tiles are placed at F0 indices `f0_count..f0_count+12`.
///
/// Must be called AFTER both `patch_16x16()` and `patch_fa_f0()`.
/// Returns the Bank $32 SNES address immediately after the remap region.
pub fn patch_fb_blank_remap(
    rom: &mut TrackedRom,
    font_data: &[u8],
    f0_count: usize,
) -> Result<u16, String> {
    let fb_tile_base = 208; // font binary tile index for FB $00

    // Compute dynamic remap: (fb_slot, f0_target_index)
    let remap: Vec<(u8, usize)> = FB_BLANK_SLOTS
        .iter()
        .enumerate()
        .map(|(i, &fb)| (fb, f0_count + i))
        .collect();

    let first_f0 = remap[0].1;
    let last_f0 = remap[remap.len() - 1].1;
    let remap_start = F0_FONT_START as usize + first_f0 * FONT_16X16_TILE_SIZE;
    let remap_end = F0_FONT_START as usize + (last_f0 + 1) * FONT_16X16_TILE_SIZE;

    if remap_end > 0x10000 {
        return Err(format!(
            "FB remap tiles exceed Bank $32: end ${:04X}",
            remap_end
        ));
    }

    let remap_len = (last_f0 - first_f0 + 1) * FONT_16X16_TILE_SIZE;
    let remap_pc = lorom_to_pc(FA_FONT_BANK, remap_start as u16);

    // Build all F0 remap data first, then write as one region
    let mut remap_data = vec![0u8; remap_len];

    for &(fb_slot, f0_slot) in &remap {
        // 1. FB tile in Bank $0F already zeroed by patch_16x16 (pre-zeroed before write)

        // 2. Copy glyph from font binary to F0 remap buffer
        let src_tile = fb_tile_base + fb_slot as usize;
        let src_offset = src_tile * FONT_16X16_TILE_SIZE;
        if src_offset + FONT_16X16_TILE_SIZE > font_data.len() {
            return Err(format!(
                "Font binary too small for FB tile ${:02X} (need {} bytes, have {})",
                fb_slot,
                src_offset + FONT_16X16_TILE_SIZE,
                font_data.len()
            ));
        }

        let f0_rom_addr = F0_FONT_START as usize + f0_slot * FONT_16X16_TILE_SIZE;
        let f0_pc = lorom_to_pc(FA_FONT_BANK, f0_rom_addr as u16);
        if f0_pc + FONT_16X16_TILE_SIZE > rom.len() {
            return Err(format!("F0 slot {} out of ROM bounds", f0_slot));
        }

        let buf_offset = (f0_slot - first_f0) * FONT_16X16_TILE_SIZE;
        remap_data[buf_offset..buf_offset + FONT_16X16_TILE_SIZE]
            .copy_from_slice(&font_data[src_offset..src_offset + FONT_16X16_TILE_SIZE]);
    }

    rom.write(remap_pc, &remap_data, "font:fb_remap_f0");

    println!(
        "  FB→F0 remap: {} tiles (FB blanks zeroed, glyphs moved to F0 ${:02X}-${:02X})",
        FB_BLANK_SLOTS.len(),
        first_f0,
        last_f0
    );

    Ok(remap_end as u16)
}

#[cfg(test)]
#[path = "font_tests.rs"]
mod tests;
