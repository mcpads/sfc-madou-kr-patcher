//! TTF → SNES 2bpp font generation pipeline.
//!
//! Replaces Python scripts `generate_ko_font.py` and `generate_ko_fixed.py`.
//! Uses `fontdue` crate for TTF rasterization.

use std::collections::HashMap;
use std::path::Path;

use crate::patch::font::FB_BLANK_SLOTS;

// ── Constants ────────────────────────────────────────────────────

/// Characters for the 32 FIXED_ENCODE tiles ($00-$1F).
const FIXED_CHARS: [Option<char>; 32] = [
    None,             // $00: space (blank)
    Some('0'),        // $01
    Some('1'),        // $02
    Some('2'),        // $03
    Some('3'),        // $04
    Some('4'),        // $05
    Some('5'),        // $06
    Some('6'),        // $07
    Some('7'),        // $08
    Some('8'),        // $09
    Some('9'),        // $0A
    Some('!'),        // $0B
    Some('~'),        // $0C
    Some('.'),        // $0D
    Some('?'),        // $0E
    Some('"'),        // $0F
    Some('\u{2192}'), // $10: →
    Some('\u{2191}'), // $11: ↑
    Some('\u{2190}'), // $12: ←
    Some('\u{2193}'), // $13: ↓
    Some('-'),        // $14
    Some(','),        // $15
    Some('['),        // $16
    Some(']'),        // $17
    None,             // $18: BLANK_RENDER
    Some('\u{300C}'), // $19: 「
    Some('\u{300D}'), // $1A: 」
    None,             // $1B: reserved
    None,             // $1C: reserved
    None,             // $1D: reserved
    None,             // $1E: reserved
    None,             // $1F: reserved
];

/// Encoding layout constants.
const SINGLE_BYTE_START: u8 = 0x20;
const SINGLE_BYTE_COUNT: usize = 208; // $20-$EF
const FB_REGION_START: usize = 208;
const FB_REGION_COUNT: usize = 256;
const F1_REGION_START: usize = 464; // 208 + 256
const F1_REGION_COUNT: usize = 256;
const F0_REGION_START: usize = 720; // 464 + 256

// ── Public types ─────────────────────────────────────────────────

/// Result of font generation: tile data + encoding table.
pub struct FontGenResult {
    /// Sequential 16x16 2bpp tiles (chars.len() × 64 bytes).
    pub font_data: Vec<u8>,
    /// 32 fixed-encode tiles (2048 bytes).
    pub fixed_data: Vec<u8>,
    /// Character → game byte encoding.
    pub encoding: HashMap<char, Vec<u8>>,
}

// ── Main entry point ─────────────────────────────────────────────

/// Generate Korean font tiles and encoding from a TTF font.
///
/// `ttf_data`: raw TTF file bytes.
/// `ttf_size`: pixel size for rasterization (e.g. 12.0 for Galmuri).
/// `chars`: ordered charset (frequency-sorted).
pub fn generate_font(
    ttf_data: &[u8],
    ttf_size: f32,
    chars: &[char],
) -> Result<FontGenResult, String> {
    let font = fontdue::Font::from_bytes(ttf_data, fontdue::FontSettings::default())
        .map_err(|e| format!("Failed to load TTF font: {}", e))?;

    let encoding = build_encoding_table(chars);

    // Render all charset glyphs sequentially (with 1px outline)
    let mut font_data = Vec::with_capacity(chars.len() * 64);
    for &ch in chars {
        let bitmap = render_glyph_to_bitmap(&font, ch, ttf_size);
        let tile = bitmap_to_snes_2bpp_16x16_outline(&bitmap);
        font_data.extend_from_slice(&tile);
    }

    let fixed_data = generate_fixed_tiles(&font, ttf_size);

    print_font_summary(chars, &font_data, &encoding);

    Ok(FontGenResult {
        font_data,
        fixed_data,
        encoding,
    })
}

/// Print font generation summary (tile count + encoding breakdown).
fn print_font_summary(chars: &[char], font_data: &[u8], encoding: &HashMap<char, Vec<u8>>) {
    println!(
        "  Font: {} tiles ({} bytes), Fixed: 32 tiles (2048 bytes)",
        chars.len(),
        font_data.len()
    );
    let single = chars.iter().filter(|c| encoding[c].len() == 1).count();
    let multi = chars.iter().filter(|c| encoding[c].len() == 2).count();
    println!(
        "  Encoding: {} single-byte + {} two-byte = {} total",
        single,
        multi,
        chars.len()
    );
}

// ── Glyph rendering ─────────────────────────────────────────────

/// Render a character to a 16×16 1-bit bitmap using fontdue.
///
/// Uses baseline-relative positioning: the font baseline is placed at a fixed
/// row within the 16px canvas so that full-height characters are approximately
/// centered, while small symbols (period, tilde, comma) land at their correct
/// vertical positions instead of being clamped to the top.
pub fn render_glyph_to_bitmap(font: &fontdue::Font, ch: char, px: f32) -> [bool; 256] {
    let mut bitmap = [false; 256];
    let canvas_size = 16usize;

    let (metrics, raster) = font.rasterize(ch, px);

    if metrics.width == 0 || metrics.height == 0 {
        return bitmap;
    }

    let ascent = font
        .horizontal_line_metrics(px)
        .map(|m| m.ascent as i32)
        .unwrap_or(px as i32);

    // X: center horizontally (account for bearing)
    let bbox_x0 = metrics.xmin;
    let x_offset = ((canvas_size as i32 - metrics.width as i32) / 2 - bbox_x0).max(0) as usize;

    // Y: baseline-relative positioning
    // Place baseline so full-height characters (height ≈ ascent) are centered:
    //   baseline_row = (canvas + ascent) / 2
    // Then glyph top in screen coords = baseline_row - ymin - height
    // (ymin = bottom of glyph bbox in y-up coords from baseline)
    let baseline_in_canvas = (canvas_size as i32 + ascent) / 2;
    let y_offset = (baseline_in_canvas - metrics.ymin - metrics.height as i32).max(0) as usize;

    for row in 0..metrics.height {
        for col in 0..metrics.width {
            let coverage = raster[row * metrics.width + col];
            if coverage >= 128 {
                let cx = x_offset + col;
                let cy = y_offset + row;
                if cx < canvas_size && cy < canvas_size {
                    bitmap[cy * canvas_size + cx] = true;
                }
            }
        }
    }

    bitmap
}

// ── SNES 2bpp tile conversion ────────────────────────────────────

/// Convert a 16×16 1-bit bitmap to outlined SNES 2bpp format (64 bytes).
///
/// Layout: TL(16B) + TR(16B) + BL(16B) + BR(16B) quadrants.
/// Matches JP original save menu style:
///   - Color 3 (BP0=1, BP1=1): glyph interior (fill)
///   - Color 2 (BP0=0, BP1=1): 1px outline around glyph
///   - Color 0 (BP0=0, BP1=0): transparent background
pub fn bitmap_to_snes_2bpp_16x16_outline(bitmap: &[bool; 256]) -> [u8; 64] {
    // Step 1: Dilate bitmap by 1px in all 8 directions
    let mut dilated = [false; 256];
    for y in 0..16usize {
        for x in 0..16usize {
            if bitmap[y * 16 + x] {
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let ny = y as i32 + dy;
                        let nx = x as i32 + dx;
                        if (0..16).contains(&ny) && (0..16).contains(&nx) {
                            dilated[ny as usize * 16 + nx as usize] = true;
                        }
                    }
                }
            }
        }
    }

    // Step 2: Generate 2bpp tile data
    let mut data = [0u8; 64];
    let quadrants: [(usize, usize); 4] = [(0, 0), (0, 8), (8, 0), (8, 8)];

    for (qi, &(row_off, col_off)) in quadrants.iter().enumerate() {
        let base = qi * 16;
        for r in 0..8usize {
            let mut bp0: u8 = 0;
            let mut bp1: u8 = 0;
            for c in 0..8usize {
                let px = (row_off + r) * 16 + (col_off + c);
                if bitmap[px] {
                    // Color 3: interior fill (BP0=1, BP1=1)
                    bp0 |= 1 << (7 - c);
                    bp1 |= 1 << (7 - c);
                } else if dilated[px] {
                    // Color 2: outline only (BP0=0, BP1=1)
                    bp1 |= 1 << (7 - c);
                }
            }
            data[base + r * 2] = bp0;
            data[base + r * 2 + 1] = bp1;
        }
    }

    data
}

// ── 8x8 glyph rendering (menu worldmap) ─────────────────────────

/// Render a character to an 8×8 1-bit bitmap using fontdue.
///
/// Same centering logic as `render_glyph_to_bitmap` but with 8×8 canvas.
pub fn render_glyph_to_bitmap_8x8(font: &fontdue::Font, ch: char, px: f32) -> [bool; 64] {
    let mut bitmap = [false; 64];
    let canvas_size = 8usize;

    let (metrics, raster) = font.rasterize(ch, px);

    if metrics.width == 0 || metrics.height == 0 {
        return bitmap;
    }

    let ascent = font
        .horizontal_line_metrics(px)
        .map(|m| m.ascent as i32)
        .unwrap_or(px as i32);

    let bbox_x0 = metrics.xmin;
    let bbox_y0 = ascent - metrics.ymin - metrics.height as i32;

    let x_offset = ((canvas_size as i32 - metrics.width as i32) / 2 - bbox_x0).max(0) as usize;
    let y_offset = ((canvas_size as i32 - metrics.height as i32) / 2 - bbox_y0).max(0) as usize;

    for row in 0..metrics.height {
        for col in 0..metrics.width {
            let coverage = raster[row * metrics.width + col];
            if coverage >= 128 {
                let cx = x_offset + col;
                let cy = y_offset + row;
                if cx < canvas_size && cy < canvas_size {
                    bitmap[cy * canvas_size + cx] = true;
                }
            }
        }
    }

    bitmap
}

/// Convert an 8×8 1-bit bitmap to SNES 2bpp format (16 bytes).
///
/// Matches JP menu worldmap tile format: BP1 is always $FF (fully opaque).
/// Color 3 (BP0=1, BP1=1): glyph strokes (foreground)
/// Color 2 (BP0=0, BP1=1): background fill
pub fn bitmap_to_snes_2bpp_8x8_outline(bitmap: &[bool; 64]) -> [u8; 16] {
    let mut data = [0u8; 16];
    for r in 0..8usize {
        let mut bp0: u8 = 0;
        for c in 0..8usize {
            if bitmap[r * 8 + c] {
                bp0 |= 1 << (7 - c);
            }
        }
        data[r * 2] = bp0;
        data[r * 2 + 1] = 0xFF; // BP1 always $FF — opaque, matching JP tiles
    }

    data
}

/// Render menu worldmap KO glyphs as outlined 8×8 2bpp tiles.
///
/// Returns one `[u8; 16]` per character (single 8×8 tile).
pub fn render_menu_worldmap_tiles(
    ttf_data: &[u8],
    ttf_size: f32,
    chars: &[char],
) -> Result<Vec<[u8; 16]>, String> {
    let font = fontdue::Font::from_bytes(ttf_data, fontdue::FontSettings::default())
        .map_err(|e| format!("Failed to load TTF font: {}", e))?;

    let mut tiles = Vec::with_capacity(chars.len());
    for &ch in chars {
        let bitmap = render_glyph_to_bitmap_8x8(&font, ch, ttf_size);
        tiles.push(bitmap_to_snes_2bpp_8x8_outline(&bitmap));
    }
    Ok(tiles)
}

/// Convert a 16×16 1-bit bitmap to opaque SNES 2bpp format.
///
/// BP1 = $FF always (no transparent pixels). Used for options/stat/magic screens
/// where text tiles must be fully opaque to match the background.
/// Color 3 (BP0=1, BP1=1): glyph fill
/// Color 2 (BP0=0, BP1=1): background fill
/// Returns [TL, TR, BL, BR] quadrants, each 16 bytes.
#[cfg(test)]
pub fn bitmap_to_snes_2bpp_16x16_opaque(bitmap: &[bool; 256]) -> [[u8; 16]; 4] {
    let mut tiles = [[0u8; 16]; 4];
    let quadrants: [(usize, usize); 4] = [(0, 0), (0, 8), (8, 0), (8, 8)];
    for (qi, &(row_off, col_off)) in quadrants.iter().enumerate() {
        for r in 0..8usize {
            let mut bp0: u8 = 0;
            for c in 0..8usize {
                if bitmap[(row_off + r) * 16 + (col_off + c)] {
                    bp0 |= 1 << (7 - c);
                }
            }
            tiles[qi][r * 2] = bp0;
            tiles[qi][r * 2 + 1] = 0xFF; // BP1 always $FF — opaque
        }
    }
    tiles
}

/// Render 16×16 KO glyphs as outlined 2bpp tiles (Color 3=fill, Color 2=1px outline).
///
/// Returns 4 tiles per character: [TL, TR, BL, BR], each 16 bytes.
pub fn render_options_16x16_tiles(
    ttf_data: &[u8],
    ttf_size: f32,
    chars: &[char],
) -> Result<Vec<[[u8; 16]; 4]>, String> {
    let font = fontdue::Font::from_bytes(ttf_data, fontdue::FontSettings::default())
        .map_err(|e| format!("Failed to load TTF font: {}", e))?;

    let mut tiles = Vec::with_capacity(chars.len());
    for &ch in chars {
        let mut bitmap = render_glyph_to_bitmap(&font, ch, ttf_size);
        // Vertically center within the 16px cell
        let mut min_y = 16usize;
        let mut max_y = 0usize;
        for y in 0..16usize {
            for x in 0..16usize {
                if bitmap[y * 16 + x] {
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
            }
        }
        if min_y <= max_y {
            let dy = (16i32 - (min_y + max_y + 1) as i32) / 2;
            if dy != 0 {
                let mut shifted = [false; 256];
                for y in 0..16usize {
                    for x in 0..16usize {
                        if bitmap[y * 16 + x] {
                            let ny = (y as i32 + dy).clamp(0, 15) as usize;
                            shifted[ny * 16 + x] = true;
                        }
                    }
                }
                bitmap = shifted;
            }
        }
        let flat = bitmap_to_snes_2bpp_16x16_outline(&bitmap);
        let mut quads = [[0u8; 16]; 4];
        for q in 0..4 {
            quads[q].copy_from_slice(&flat[q * 16..(q + 1) * 16]);
        }
        tiles.push(quads);
    }
    Ok(tiles)
}

/// Render save menu KO glyphs as outlined 2bpp tiles (Color 3 fill + Color 2 outline).
///
/// Returns one `[u8; 64]` per character (4 × 8×8 quadrants: TL, TR, BL, BR).
pub fn render_savemenu_tiles(
    ttf_data: &[u8],
    ttf_size: f32,
    chars: &[char],
) -> Result<Vec<[u8; 64]>, String> {
    let font = fontdue::Font::from_bytes(ttf_data, fontdue::FontSettings::default())
        .map_err(|e| format!("Failed to load TTF font: {}", e))?;

    let mut tiles = Vec::with_capacity(chars.len());
    for &ch in chars {
        let bitmap = render_glyph_to_bitmap(&font, ch, ttf_size);
        tiles.push(bitmap_to_snes_2bpp_16x16_outline(&bitmap));
    }
    Ok(tiles)
}

/// Convert an 8×8 1-bit bitmap to SNES 4bpp format (32 bytes).
///
/// 4bpp layout: bytes 0-15 = BP0/BP1 interleaved, bytes 16-31 = BP2/BP3.
/// fg_color: 4-bit color index (0-15) for foreground (glyph) pixels.
/// bg_color: 4-bit color index (0-15) for background pixels (0 = transparent).
pub fn bitmap_to_snes_4bpp_8x8(bitmap: &[bool; 64], fg_color: u8, bg_color: u8) -> [u8; 32] {
    let mut tile = [0u8; 32];
    for r in 0..8usize {
        let mut bp = [0u8; 4];
        for c in 0..8usize {
            let color = if bitmap[r * 8 + c] {
                fg_color
            } else {
                bg_color
            };
            if color != 0 {
                let bit = 1 << (7 - c);
                for (p, bp_val) in bp.iter_mut().enumerate() {
                    if color & (1 << p) != 0 {
                        *bp_val |= bit;
                    }
                }
            }
        }
        tile[r * 2] = bp[0];
        tile[r * 2 + 1] = bp[1];
        tile[16 + r * 2] = bp[2];
        tile[16 + r * 2 + 1] = bp[3];
    }
    tile
}

/// Convert a 16×16 1-bit bitmap to SNES 4bpp format (single fg color, transparent bg).
///
/// Returns [TL, TR, BL, BR] quadrants, each 32 bytes.
/// 4bpp layout per tile: bytes 0-15 = BP0/BP1 interleaved, bytes 16-31 = BP2/BP3.
/// fg_color: 4-bit color index (0-15) for foreground pixels. Background = 0 (transparent).
pub fn bitmap_to_snes_4bpp_16x16(bitmap: &[bool; 256], fg_color: u8) -> [[u8; 32]; 4] {
    let mut tiles = [[0u8; 32]; 4];
    let quadrants: [(usize, usize); 4] = [(0, 0), (0, 8), (8, 0), (8, 8)];

    for (qi, &(row_off, col_off)) in quadrants.iter().enumerate() {
        for r in 0..8usize {
            let mut bp = [0u8; 4];
            for c in 0..8usize {
                if bitmap[(row_off + r) * 16 + (col_off + c)] {
                    let bit = 1 << (7 - c);
                    for (p, bp_val) in bp.iter_mut().enumerate() {
                        if fg_color & (1 << p) != 0 {
                            *bp_val |= bit;
                        }
                    }
                }
            }
            tiles[qi][r * 2] = bp[0];
            tiles[qi][r * 2 + 1] = bp[1];
            tiles[qi][16 + r * 2] = bp[2];
            tiles[qi][16 + r * 2 + 1] = bp[3];
        }
    }
    tiles
}

/// Render characters to 8×8 4bpp tiles for OAM sprites.
///
/// Returns one `[u8; 32]` per character (single 8×8 4bpp tile).
/// bg_color: 4-bit color index for background pixels (0 = transparent).
pub fn render_oam_8x8_4bpp_tiles(
    ttf_data: &[u8],
    ttf_size: f32,
    chars: &[char],
    fg_color: u8,
    bg_color: u8,
) -> Result<Vec<[u8; 32]>, String> {
    let font = fontdue::Font::from_bytes(ttf_data, fontdue::FontSettings::default())
        .map_err(|e| format!("Failed to load TTF font: {}", e))?;

    let mut tiles = Vec::with_capacity(chars.len());
    for &ch in chars {
        let bitmap = render_glyph_to_bitmap_8x8(&font, ch, ttf_size);
        tiles.push(bitmap_to_snes_4bpp_8x8(&bitmap, fg_color, bg_color));
    }
    Ok(tiles)
}

/// Render characters to 16×16 4bpp tiles for OAM sprites.
///
/// Returns [TL, TR, BL, BR] quadrants per character, each 32 bytes.
#[allow(dead_code)] // May be needed for future OAM sprite work (OAM sprite rendering)
pub fn render_oam_16x16_4bpp_tiles(
    ttf_data: &[u8],
    ttf_size: f32,
    chars: &[char],
    fg_color: u8,
) -> Result<Vec<[[u8; 32]; 4]>, String> {
    let font = fontdue::Font::from_bytes(ttf_data, fontdue::FontSettings::default())
        .map_err(|e| format!("Failed to load TTF font: {}", e))?;

    let mut tiles = Vec::with_capacity(chars.len());
    for &ch in chars {
        let bitmap = render_glyph_to_bitmap(&font, ch, ttf_size);
        tiles.push(bitmap_to_snes_4bpp_16x16(&bitmap, fg_color));
    }
    Ok(tiles)
}

/// Render character pairs to 16×16 4bpp tiles (each char in an 8×16 half).
///
/// Each pair `(left, right)` is rendered into one 16×16 sprite where:
/// - Left char occupies columns 0–7 (8×16 pixels)
/// - Right char occupies columns 8–15 (8×16 pixels)
///
/// Returns [TL, TR, BL, BR] quadrants per pair, each 32 bytes.
#[allow(dead_code)]
pub fn render_oam_16x16_pair_4bpp_tiles(
    ttf_data: &[u8],
    ttf_size: f32,
    pairs: &[(char, char)],
    fg_color: u8,
) -> Result<Vec<[[u8; 32]; 4]>, String> {
    let font = fontdue::Font::from_bytes(ttf_data, fontdue::FontSettings::default())
        .map_err(|e| format!("Failed to load TTF font: {}", e))?;

    let mut tiles = Vec::with_capacity(pairs.len());
    for &(left, right) in pairs {
        let left_bm = render_glyph_to_bitmap_8x16(&font, left, ttf_size);
        let right_bm = render_glyph_to_bitmap_8x16(&font, right, ttf_size);

        // Combine into 16×16 bitmap: left half + right half
        let mut combined = [false; 256];
        for row in 0..16 {
            for col in 0..8 {
                combined[row * 16 + col] = left_bm[row * 8 + col];
                combined[row * 16 + 8 + col] = right_bm[row * 8 + col];
            }
        }
        tiles.push(bitmap_to_snes_4bpp_16x16(&combined, fg_color));
    }
    Ok(tiles)
}

/// Render a single glyph into an 8×16 pixel bitmap (for half of a 16×16 OAM sprite).
fn render_glyph_to_bitmap_8x16(font: &fontdue::Font, ch: char, px: f32) -> [bool; 128] {
    let mut bitmap = [false; 128];
    let canvas_w = 8usize;
    let canvas_h = 16usize;

    let (metrics, raster) = font.rasterize(ch, px);

    if metrics.width == 0 || metrics.height == 0 {
        return bitmap;
    }

    let ascent = font
        .horizontal_line_metrics(px)
        .map(|m| m.ascent as i32)
        .unwrap_or(px as i32);

    // X: center horizontally in 8px width
    let bbox_x0 = metrics.xmin;
    let x_offset = ((canvas_w as i32 - metrics.width as i32) / 2 - bbox_x0).max(0) as usize;

    // Y: baseline-relative positioning (same as 16×16 full canvas)
    let baseline_in_canvas = (canvas_h as i32 + ascent) / 2;
    let y_offset = (baseline_in_canvas - metrics.ymin - metrics.height as i32).max(0) as usize;

    for row in 0..metrics.height {
        for col in 0..metrics.width {
            let coverage = raster[row * metrics.width + col];
            if coverage >= 128 {
                let cx = x_offset + col;
                let cy = y_offset + row;
                if cx < canvas_w && cy < canvas_h {
                    bitmap[cy * canvas_w + cx] = true;
                }
            }
        }
    }
    bitmap
}

/// Render characters to 16×16 1-bit bitmaps for OBJ sprite overlays.
pub fn render_obj_title_bitmaps(
    ttf_data: &[u8],
    ttf_size: f32,
    chars: &[char],
) -> Result<Vec<[bool; 256]>, String> {
    let font = fontdue::Font::from_bytes(ttf_data, fontdue::FontSettings::default())
        .map_err(|e| format!("Failed to load TTF font: {}", e))?;

    Ok(chars
        .iter()
        .map(|&ch| render_glyph_to_bitmap(&font, ch, ttf_size))
        .collect())
}

/// Convert a 16×16 1-bit bitmap to SNES 2bpp format (64 bytes).
///
/// Layout: TL(16B) + TR(16B) + BL(16B) + BR(16B) quadrants.
/// Monochrome: BP0 = BP1 = source bit → color index 3.
#[cfg(test)]
fn bitmap_to_snes_2bpp_16x16(bitmap: &[bool; 256]) -> [u8; 64] {
    let mut data = [0u8; 64];

    // Quadrant order: TL, TR, BL, BR
    let quadrants: [(usize, usize); 4] = [(0, 0), (0, 8), (8, 0), (8, 8)];

    for (qi, &(row_off, col_off)) in quadrants.iter().enumerate() {
        let base = qi * 16;
        for r in 0..8usize {
            let mut byte_val: u8 = 0;
            for c in 0..8usize {
                if bitmap[(row_off + r) * 16 + (col_off + c)] {
                    byte_val |= 1 << (7 - c);
                }
            }
            // Monochrome: BP0 = BP1 for color index 3
            data[base + r * 2] = byte_val;
            data[base + r * 2 + 1] = byte_val;
        }
    }

    data
}

// ── Encoding table ───────────────────────────────────────────────

/// Build game byte encoding for each character based on charset position.
///
/// Layout by position:
/// - [0, 208): single-byte $20-$EF
/// - [208, 464): FB $00-$FF (12 blank slots → F0 $45-$50)
/// - [464, 720): F1 $00-$FF
/// - [720, ...): F0 $00+ (charset overflow)
///
/// FB blank slots are remapped to F0 indices AFTER the charset overflow.
fn build_encoding_table(chars: &[char]) -> HashMap<char, Vec<u8>> {
    assert!(
        chars.len() <= 976,
        "Charset size {} exceeds maximum 976 (208 single + 256 FB + 256 F1 + 256 F0)",
        chars.len()
    );

    // F0 remap targets start after the charset F0 overflow count
    let charset_f0_count = chars.len().saturating_sub(F0_REGION_START);
    let fb_remap: HashMap<u8, u8> = FB_BLANK_SLOTS
        .iter()
        .enumerate()
        .map(|(i, &fb)| (fb, (charset_f0_count + i) as u8))
        .collect();
    let mut table = HashMap::with_capacity(chars.len());

    for (i, &ch) in chars.iter().enumerate() {
        let bytes = if i < SINGLE_BYTE_COUNT {
            // Phase 1: single-byte $20-$EF
            vec![SINGLE_BYTE_START + i as u8]
        } else if i < FB_REGION_START + FB_REGION_COUNT {
            // Phase 2: FB range (blank slots remapped to F0)
            let fb_slot = (i - FB_REGION_START) as u8;
            if let Some(&f0_slot) = fb_remap.get(&fb_slot) {
                vec![0xF0, f0_slot]
            } else {
                vec![0xFB, fb_slot]
            }
        } else if i < F1_REGION_START + F1_REGION_COUNT {
            // Phase 3: F1 $00-$FF
            vec![0xF1, (i - F1_REGION_START) as u8]
        } else {
            // Phase 4: F0 $00+ (charset overflow)
            vec![0xF0, (i - F0_REGION_START) as u8]
        };
        table.insert(ch, bytes);
    }

    table
}

// ── Fixed tiles ──────────────────────────────────────────────────

/// Generate 32 FIXED_ENCODE tiles ($00-$1F) for digits, punctuation, arrows.
fn generate_fixed_tiles(font: &fontdue::Font, px: f32) -> Vec<u8> {
    let mut data = vec![0u8; 32 * 64];

    for (i, ch_opt) in FIXED_CHARS.iter().enumerate() {
        if let Some(&ch) = ch_opt.as_ref() {
            let bitmap = render_glyph_to_bitmap(font, ch, px);
            let tile = bitmap_to_snes_2bpp_16x16_outline(&bitmap);
            data[i * 64..(i + 1) * 64].copy_from_slice(&tile);
        }
        // None entries stay as zeros (blank tiles)
    }

    data
}

// ── File I/O ─────────────────────────────────────────────────────

/// Load character set from a TSV file.
///
/// Format: first column is a single character, tab-separated.
/// Lines starting with `#` or blank lines are skipped.
pub fn load_charset(path: &Path) -> Result<Vec<char>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read charset '{}': {}", path.display(), e))?;

    let mut chars = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let first_field = line.split('\t').next().unwrap_or("");
        let mut ch_iter = first_field.chars();
        if let Some(ch) = ch_iter.next() {
            if ch_iter.next().is_none() {
                chars.push(ch);
            }
        }
    }

    Ok(chars)
}

/// Write encoding table as TSV (compatible with `ko::load_ko_encoding`).
pub fn write_encoding_tsv(
    path: &Path,
    encoding: &HashMap<char, Vec<u8>>,
    chars: &[char],
) -> Result<(), String> {
    let mut content = String::from("CHAR\tUNICODE\tBYTES\tTILE_INDEX\n");

    for (i, &ch) in chars.iter().enumerate() {
        if let Some(bytes) = encoding.get(&ch) {
            let hex_str: String = bytes
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect::<Vec<_>>()
                .join(" ");
            content.push_str(&format!(
                "{}\tU+{:04X}\t{}\t{}\n",
                ch, ch as u32, hex_str, i
            ));
        }
    }

    std::fs::write(path, &content)
        .map_err(|e| format!("Failed to write TSV '{}': {}", path.display(), e))
}

#[cfg(test)]
#[path = "font_gen_tests.rs"]
mod tests;
