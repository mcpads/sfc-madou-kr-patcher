//! Korean title-screen graphics compiled from three transparent PNG components.
//!
//! The original title animation already has the required sequencing:
//!
//! 1. Mode 7 flies only the main-logo outline in.
//! 2. BG3 displays the same red main-logo outline.
//! 3. BG1's completed logo is revealed by the existing window animation.
//! 4. Nine 32×32 OBJ slots display the flower/`하나마루` component.
//! 5. Two adjacent OBJ slots display the first subtitle character.
//! 6. BG3 adds the remaining four subtitle characters at 15-frame intervals.
//!
//! This patch replaces only the graphics and the two initial tilemaps. It does
//! not replace the animation controller. The source LZ streams are rebuilt in
//! place after the world-map patch has snapshotted its shared JP graphics.

use crate::patch::font::{compress_lz, decompress_lz};
use crate::patch::tracked_rom::{Expect, TrackedRom};
use crate::rom::lorom_to_pc;
use png::{BitDepth, ColorType, Transformations};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

const SCREEN_WIDTH: usize = 256;
const SCREEN_HEIGHT: usize = 224;
const MAP_COLS: usize = 32;

const MAIN_MAX_WIDTH: usize = 224;
const MAIN_MIN_WIDTH: usize = 128;
const MAIN_MAX_HEIGHT: usize = 64;
const MAIN_TOP: usize = 52;
const BADGE_RADIUS_Y_PERCENT: usize = 17;
const FLOWER_MAX_SIZE: usize = 86;
const FLOWER_ROTATION_DEGREES_CCW: f64 = 15.0;
const BG1_PALETTE: [u16; 16] = [
    0x0000, 0x000C, 0x0000, 0x0044, 0x0044, 0x0886, 0x08C8, 0x110A, 0x118E, 0x120C, 0x2212, 0x3296,
    0x42D4, 0x42D8, 0x539C, 0x7BDE,
];
const OBJ_PALETTE_1: [u16; 16] = [
    0x0000, 0x0000, 0x2000, 0x795C, 0x001E, 0x0140, 0x0180, 0x01C0, 0x0200, 0x0240, 0x0280, 0x02C0,
    0x0300, 0x0340, 0x03C0, 0x7BDE,
];

const FLOWER_BASE_TILES: [[u8; 3]; 3] = [[64, 68, 72], [128, 132, 136], [192, 196, 200]];
const DAE_BASE_TILES: [u8; 2] = [140, 204];
const SUBTITLE_GLYPH_HEIGHT: usize = 28;
const SUBTITLE_BASE_SHEAR: usize = 4;
const SUBTITLE_YU_SHEAR: usize = 1;
const SUBTITLE_WON_SHEAR: usize = 8;
const SUBTITLE_A_SHEAR: usize = 6;
const DAE_BODY_WIDTH: usize = 25;
const DAE_TOP: usize = 18;
const REST_BODY_WIDTH: usize = 20;
const REST_YU_BODY_WIDTH: usize = 24;
const REST_EMPHASIZED_BODY_WIDTH: usize = 22;
const REST_BODY_HEIGHT: usize = 28;
const REST_EMPHASIZED_BODY_HEIGHT: usize = 30;
const REST_GLYPH_LEFTS: [usize; 4] = [0, 23, 44, 68];
const REST_BASELINE: usize = 32;
const REST_CANVAS_WIDTH: usize = 96;
const REST_CANVAS_HEIGHT: usize = 32;

// Runtime-observed final BG3 map for `幼稚園児`. The game reveals one group
// every 15 frames; the first character is an OBJ and is not in this matrix.
const SUBTITLE_TILE_MATRIX: [[u16; 12]; 4] = [
    [122, 123, 124, 125, 126, 127, 128, 129, 130, 131, 132, 133],
    [137, 138, 139, 140, 141, 142, 143, 144, 145, 146, 147, 148],
    [153, 154, 155, 156, 157, 158, 159, 160, 161, 162, 163, 164],
    [0, 168, 169, 170, 171, 172, 173, 174, 175, 176, 177, 178],
];

// Temporary edge tiles used by the first subtitle reveal frame.
const SUBTITLE_TEMP_TILES: &[u16] = &[149, 165, 179];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Pixel {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

#[derive(Clone, Debug)]
struct Image {
    width: usize,
    height: usize,
    pixels: Vec<Pixel>,
}

impl Image {
    fn transparent(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![Pixel::default(); width * height],
        }
    }

    fn get(&self, x: usize, y: usize) -> Pixel {
        self.pixels[y * self.width + x]
    }

    fn set(&mut self, x: usize, y: usize, pixel: Pixel) {
        self.pixels[y * self.width + x] = pixel;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Bounds {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

#[derive(Clone)]
struct LzSource {
    bank: u8,
    addr: u16,
    span: usize,
    decompressed_size: usize,
    label: &'static str,
}

const CHR_BG12_A: LzSource = LzSource {
    bank: 0x12,
    addr: 0xD83C,
    span: 7344,
    decompressed_size: 8128,
    label: "title:bg12_chr_a",
};
const CHR_MODE7: LzSource = LzSource {
    bank: 0x11,
    addr: 0x818C,
    span: 1124,
    decompressed_size: 16384,
    label: "title:mode7_chr",
};
const CHR_BG12_B: LzSource = LzSource {
    bank: 0x11,
    addr: 0x9ACD,
    span: 3730,
    decompressed_size: 8192,
    label: "title:bg12_obj_chr_b",
};
const CHR_BG3: LzSource = LzSource {
    bank: 0x11,
    addr: 0x8774,
    span: 2083,
    decompressed_size: 3312,
    label: "title:bg3_chr",
};
const MAP_BG1: LzSource = LzSource {
    bank: 0x12,
    addr: 0xF4EC,
    span: 406,
    decompressed_size: 2048,
    label: "title:bg1_map",
};
const MAP_BG3: LzSource = LzSource {
    bank: 0x11,
    addr: 0x85F0,
    span: 388,
    decompressed_size: 2048,
    label: "title:bg3_map",
};

/// Summary of the deterministic title-asset compilation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TitlePatchStats {
    pub main_width: usize,
    pub main_height: usize,
    pub bg1_tiles: usize,
    pub bg3_tiles: usize,
    pub subtitle_tiles: usize,
}

fn load_png(path: &Path) -> Result<Image, String> {
    let file =
        File::open(path).map_err(|e| format!("Failed to open PNG {}: {e}", path.display()))?;
    let mut decoder = png::Decoder::new(BufReader::new(file));
    decoder.set_transformations(Transformations::EXPAND | Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("Failed to read PNG {}: {e}", path.display()))?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|e| format!("Failed to decode PNG {}: {e}", path.display()))?;
    if info.bit_depth != BitDepth::Eight {
        return Err(format!(
            "PNG {} must decode to 8-bit channels, got {:?}",
            path.display(),
            info.bit_depth
        ));
    }

    let bytes = &buffer[..info.buffer_size()];
    let pixels = match info.color_type {
        ColorType::Rgba => bytes
            .chunks_exact(4)
            .map(|p| Pixel {
                r: p[0],
                g: p[1],
                b: p[2],
                a: p[3],
            })
            .collect(),
        ColorType::Rgb => bytes
            .chunks_exact(3)
            .map(|p| Pixel {
                r: p[0],
                g: p[1],
                b: p[2],
                a: 0xFF,
            })
            .collect(),
        ColorType::GrayscaleAlpha => bytes
            .chunks_exact(2)
            .map(|p| Pixel {
                r: p[0],
                g: p[0],
                b: p[0],
                a: p[1],
            })
            .collect(),
        ColorType::Grayscale => bytes
            .iter()
            .map(|&value| Pixel {
                r: value,
                g: value,
                b: value,
                a: 0xFF,
            })
            .collect(),
        ColorType::Indexed => {
            return Err(format!(
                "PNG {} remained indexed after expansion",
                path.display()
            ));
        }
    };

    Ok(Image {
        width: info.width as usize,
        height: info.height as usize,
        pixels,
    })
}

fn alpha_bounds(image: &Image) -> Result<Bounds, String> {
    let mut min_x = image.width;
    let mut min_y = image.height;
    let mut max_x = 0;
    let mut max_y = 0;
    let mut found = false;

    for y in 0..image.height {
        for x in 0..image.width {
            if image.get(x, y).a > 16 {
                found = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }

    if !found {
        return Err("PNG has no non-transparent pixels".to_string());
    }
    Ok(Bounds {
        x: min_x,
        y: min_y,
        width: max_x - min_x + 1,
        height: max_y - min_y + 1,
    })
}

fn crop(image: &Image, bounds: Bounds) -> Image {
    let mut result = Image::transparent(bounds.width, bounds.height);
    for y in 0..bounds.height {
        for x in 0..bounds.width {
            result.set(x, y, image.get(bounds.x + x, bounds.y + y));
        }
    }
    result
}

fn crop_alpha(image: &Image) -> Result<Image, String> {
    Ok(crop(image, alpha_bounds(image)?))
}

fn resize_nearest(image: &Image, width: usize, height: usize) -> Image {
    let mut result = Image::transparent(width, height);
    for y in 0..height {
        let src_y = y * image.height / height;
        for x in 0..width {
            let src_x = x * image.width / width;
            result.set(x, y, image.get(src_x, src_y));
        }
    }
    result
}

fn rotate_nearest(image: &Image, degrees_ccw: f64) -> Image {
    let radians = degrees_ccw.to_radians();
    let cos = radians.cos();
    let sin = radians.sin();
    let rotated_width = image.width;
    let rotated_height = image.height;
    let mut result = Image::transparent(rotated_width, rotated_height);
    let source_center_x = (image.width as f64 - 1.0) / 2.0;
    let source_center_y = (image.height as f64 - 1.0) / 2.0;
    let target_center_x = (rotated_width as f64 - 1.0) / 2.0;
    let target_center_y = (rotated_height as f64 - 1.0) / 2.0;

    for y in 0..rotated_height {
        let target_y = y as f64 - target_center_y;
        for x in 0..rotated_width {
            let target_x = x as f64 - target_center_x;
            let source_x = (cos * target_x - sin * target_y + source_center_x).round() as isize;
            let source_y = (sin * target_x + cos * target_y + source_center_y).round() as isize;
            if source_x >= 0
                && source_x < image.width as isize
                && source_y >= 0
                && source_y < image.height as isize
            {
                result.set(x, y, image.get(source_x as usize, source_y as usize));
            }
        }
    }
    result
}

fn fit(image: &Image, max_width: usize, max_height: usize) -> Result<Image, String> {
    let cropped = crop_alpha(image)?;
    let by_width_height = cropped.height * max_width / cropped.width;
    let (width, height) = if by_width_height <= max_height {
        (max_width, by_width_height.max(1))
    } else {
        (
            (cropped.width * max_height / cropped.height).max(1),
            max_height,
        )
    };
    Ok(resize_nearest(&cropped, width, height))
}

fn place(canvas: &mut Image, image: &Image, x: usize, y: usize) -> Result<Bounds, String> {
    if x + image.width > canvas.width || y + image.height > canvas.height {
        return Err(format!(
            "Image {}×{} at ({x},{y}) exceeds {}×{} canvas",
            image.width, image.height, canvas.width, canvas.height
        ));
    }
    for iy in 0..image.height {
        for ix in 0..image.width {
            canvas.set(x + ix, y + iy, image.get(ix, iy));
        }
    }
    Ok(Bounds {
        x,
        y,
        width: image.width,
        height: image.height,
    })
}

fn bgr15_to_rgb(value: u16) -> (i32, i32, i32) {
    (
        ((value & 0x1F) as i32) * 255 / 31,
        (((value >> 5) & 0x1F) as i32) * 255 / 31,
        (((value >> 10) & 0x1F) as i32) * 255 / 31,
    )
}

fn nearest_palette_index(pixel: Pixel, palette: &[u16]) -> u8 {
    let mut best_index = 1;
    let mut best_distance = i64::MAX;
    for (index, &color) in palette.iter().enumerate().skip(1) {
        let (r, g, b) = bgr15_to_rgb(color);
        let dr = pixel.r as i32 - r;
        let dg = pixel.g as i32 - g;
        let db = pixel.b as i32 - b;
        let distance = (dr * dr + dg * dg + db * db) as i64;
        if distance < best_distance {
            best_distance = distance;
            best_index = index as u8;
        }
    }
    best_index
}

fn quantize(image: &Image, palette: &[u16]) -> Vec<u8> {
    image
        .pixels
        .iter()
        .map(|&pixel| {
            if pixel.a <= 16 {
                0
            } else {
                nearest_palette_index(pixel, palette)
            }
        })
        .collect()
}

fn structural_layer(image: &Image, main_bounds: Bounds) -> Vec<u8> {
    let mut indexed = vec![0u8; image.width * image.height];
    let mut red = vec![false; image.width * image.height];

    for y in main_bounds.y..main_bounds.y + main_bounds.height {
        for x in main_bounds.x..main_bounds.x + main_bounds.width {
            let pixel = image.get(x, y);
            if pixel.a > 16 && nearest_palette_index(pixel, &BG1_PALETTE) == 1 {
                red[y * image.width + x] = true;
            }
        }
    }

    for y in main_bounds.y..main_bounds.y + main_bounds.height {
        for x in main_bounds.x..main_bounds.x + main_bounds.width {
            if red[y * image.width + x]
                && has_neighbor_outside_mask(&red, image.width, image.height, x, y)
            {
                indexed[y * image.width + x] = 1;
            }
        }
    }

    // The authored badges contain a gold ring, a dark-red circular field, and
    // Hangul. Clear each whole badge window so none of those pixels fly in or
    // remain on BG3. The completed BG1 logo retains all of them.
    for &x_percent in &[24usize, 42, 59, 77] {
        let center_x = main_bounds.x + main_bounds.width * x_percent / 100;
        let center_y = main_bounds.y + main_bounds.height * 89 / 100;
        let radius_x = (main_bounds.width * 6 / 100).max(5);
        let radius_y = (main_bounds.height * BADGE_RADIUS_Y_PERCENT / 100).max(5);
        let start_x = center_x.saturating_sub(radius_x).max(main_bounds.x);
        let end_x = (center_x + radius_x + 1)
            .min(main_bounds.x + main_bounds.width)
            .min(image.width);
        let start_y = center_y.saturating_sub(radius_y).max(main_bounds.y);
        let end_y = (center_y + radius_y + 1)
            .min(main_bounds.y + main_bounds.height)
            .min(image.height);

        for y in start_y..end_y {
            for x in start_x..end_x {
                indexed[y * image.width + x] = 0;
            }
        }
    }

    indexed
}

fn has_neighbor_outside_mask(
    mask: &[bool],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
) -> bool {
    for dy in -1isize..=1 {
        for dx in -1isize..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let Some(nx) = x.checked_add_signed(dx) else {
                return true;
            };
            let Some(ny) = y.checked_add_signed(dy) else {
                return true;
            };
            if nx >= width || ny >= height || !mask[ny * width + nx] {
                return true;
            }
        }
    }
    false
}

fn encode_tile_2bpp(tile: &[u8; 64]) -> Vec<u8> {
    let mut result = vec![0u8; 16];
    for y in 0..8 {
        for x in 0..8 {
            let value = tile[y * 8 + x];
            let bit = 7 - x;
            result[y * 2] |= (value & 1) << bit;
            result[y * 2 + 1] |= ((value >> 1) & 1) << bit;
        }
    }
    result
}

fn encode_tile_4bpp(tile: &[u8; 64]) -> Vec<u8> {
    let mut result = vec![0u8; 32];
    for y in 0..8 {
        for x in 0..8 {
            let value = tile[y * 8 + x];
            let bit = 7 - x;
            result[y * 2] |= (value & 1) << bit;
            result[y * 2 + 1] |= ((value >> 1) & 1) << bit;
            result[16 + y * 2] |= ((value >> 2) & 1) << bit;
            result[16 + y * 2 + 1] |= ((value >> 3) & 1) << bit;
        }
    }
    result
}

fn encode_screen_tile(indexed: &[u8], x: usize, y: usize, bpp: u8) -> Vec<u8> {
    let mut tile = [0u8; 64];
    for py in 0..8 {
        for px in 0..8 {
            tile[py * 8 + px] = indexed[(y * 8 + py) * SCREEN_WIDTH + x * 8 + px];
        }
    }
    if bpp == 2 {
        encode_tile_2bpp(&tile)
    } else {
        encode_tile_4bpp(&tile)
    }
}

fn encoded_unique_tile_count(indexed: &[u8], bpp: u8) -> usize {
    let mut unique = HashSet::new();
    for row in 0..SCREEN_HEIGHT / 8 {
        for col in 0..SCREEN_WIDTH / 8 {
            let encoded = encode_screen_tile(indexed, col, row, bpp);
            if encoded.iter().any(|&byte| byte != 0) {
                unique.insert(encoded);
            }
        }
    }
    unique.len()
}

fn clear_tile(chr: &mut [u8], tile: u16, tile_size: usize) -> Result<(), String> {
    let start = tile as usize * tile_size;
    let end = start + tile_size;
    if end > chr.len() {
        return Err(format!(
            "Tile {tile} at {start}..{end} exceeds CHR size {}",
            chr.len()
        ));
    }
    chr[start..end].fill(0);
    Ok(())
}

fn pack_layer(
    indexed: &[u8],
    bpp: u8,
    allowed_tiles: &[u16],
    chr: &mut [u8],
    tilemap: &mut [u8],
    blank_entry: u16,
) -> Result<usize, String> {
    let tile_size = if bpp == 2 { 16 } else { 32 };
    for &tile in allowed_tiles {
        clear_tile(chr, tile, tile_size)?;
    }
    for cell in tilemap.chunks_exact_mut(2) {
        cell.copy_from_slice(&blank_entry.to_le_bytes());
    }

    let mut encoded_to_tile: HashMap<Vec<u8>, u16> = HashMap::new();
    let mut next_allowed = 0;
    for row in 0..SCREEN_HEIGHT / 8 {
        for col in 0..SCREEN_WIDTH / 8 {
            let encoded = encode_screen_tile(indexed, col, row, bpp);
            if encoded.iter().all(|&byte| byte == 0) {
                continue;
            }
            let tile = if let Some(&tile) = encoded_to_tile.get(&encoded) {
                tile
            } else {
                let tile = *allowed_tiles.get(next_allowed).ok_or_else(|| {
                    format!(
                        "{}bpp layer needs more than {} unique tiles",
                        bpp,
                        allowed_tiles.len()
                    )
                })?;
                next_allowed += 1;
                let start = tile as usize * tile_size;
                chr[start..start + tile_size].copy_from_slice(&encoded);
                encoded_to_tile.insert(encoded, tile);
                tile
            };
            let entry = (blank_entry & !0x03FF) | tile;
            let map_offset = (row * MAP_COLS + col) * 2;
            tilemap[map_offset..map_offset + 2].copy_from_slice(&entry.to_le_bytes());
        }
    }
    Ok(encoded_to_tile.len())
}

fn tile_ids_from_map(tilemap: &[u8]) -> Vec<u16> {
    let blank = u16::from_le_bytes([tilemap[0], tilemap[1]]);
    let mut result: Vec<u16> = tilemap
        .chunks_exact(2)
        .map(|entry| u16::from_le_bytes([entry[0], entry[1]]))
        .filter(|&entry| entry != blank)
        .map(|entry| entry & 0x03FF)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    result.sort_unstable();
    result
}

fn subtitle_reserved_tiles() -> HashSet<u16> {
    SUBTITLE_TILE_MATRIX
        .iter()
        .flatten()
        .copied()
        .chain(SUBTITLE_TEMP_TILES.iter().copied())
        .filter(|&tile| tile != 0)
        .collect()
}

fn bg3_outline_tiles(chr_len: usize) -> Vec<u16> {
    let reserved = subtitle_reserved_tiles();
    (1..(chr_len / 16) as u16)
        .filter(|tile| !reserved.contains(tile))
        .collect()
}

fn alpha_column_runs(image: &Image) -> Vec<(usize, usize)> {
    let mut occupied = vec![false; image.width];
    for (x, slot) in occupied.iter_mut().enumerate() {
        *slot = (0..image.height).any(|y| image.get(x, y).a > 16);
    }

    let mut runs = Vec::new();
    let mut start = None;
    for (x, &is_occupied) in occupied.iter().enumerate() {
        match (start, is_occupied) {
            (None, true) => start = Some(x),
            (Some(run_start), false) => {
                runs.push((run_start, x));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(run_start) = start {
        runs.push((run_start, image.width));
    }
    runs
}

fn five_glyph_ranges(image: &Image) -> Vec<(usize, usize)> {
    let projection: Vec<usize> = (0..image.width)
        .map(|x| {
            (0..image.height)
                .filter(|&y| image.get(x, y).a > 16)
                .count()
        })
        .collect();
    let radius = (image.width / 12).max(1);
    let mut cuts = Vec::new();
    for index in 1..5 {
        let target = image.width * index / 5;
        let start = target.saturating_sub(radius).max(1);
        let end = (target + radius).min(image.width - 1);
        let cut = (start..=end)
            .min_by_key(|&x| (projection[x], x.abs_diff(target)))
            .unwrap_or(target);
        cuts.push(cut);
    }
    let mut bounds = Vec::with_capacity(5);
    let mut start = 0;
    for cut in cuts {
        bounds.push((start, cut));
        start = cut;
    }
    bounds.push((start, image.width));
    bounds
}

fn overlay(canvas: &mut Image, image: &Image, x: usize, y: usize) -> Result<(), String> {
    if x + image.width > canvas.width || y + image.height > canvas.height {
        return Err(format!(
            "Overlay {}×{} at ({x},{y}) exceeds {}×{} canvas",
            image.width, image.height, canvas.width, canvas.height
        ));
    }
    for iy in 0..image.height {
        for ix in 0..image.width {
            let source = image.get(ix, iy);
            if source.a <= 16 {
                continue;
            }
            canvas.set(x + ix, y + iy, source);
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GlyphLean {
    Backslash,
    Slash,
}

fn shear_glyph(image: &Image, lean: GlyphLean, amount: usize) -> Image {
    let mut result = Image::transparent(image.width + amount, image.height);
    let denominator = image.height.saturating_sub(1).max(1);
    for y in 0..image.height {
        let progress = (y * amount + denominator / 2) / denominator;
        let offset = match lean {
            GlyphLean::Backslash => progress,
            GlyphLean::Slash => amount - progress,
        };
        for x in 0..image.width {
            result.set(x + offset, y, image.get(x, y));
        }
    }
    result
}

fn subtitle_lean(index: usize) -> GlyphLean {
    if index.is_multiple_of(2) {
        GlyphLean::Backslash
    } else {
        GlyphLean::Slash
    }
}

fn subtitle_shear(index: usize) -> usize {
    match index {
        1 => SUBTITLE_YU_SHEAR,
        3 => SUBTITLE_WON_SHEAR,
        4 => SUBTITLE_A_SHEAR,
        _ => SUBTITLE_BASE_SHEAR,
    }
}

fn style_subtitle_glyph(
    image: &Image,
    body_width: usize,
    body_height: usize,
    index: usize,
) -> Result<Image, String> {
    let cropped = crop_alpha(image)?;
    let resized = resize_nearest(&cropped, body_width, body_height);
    Ok(shear_glyph(
        &resized,
        subtitle_lean(index),
        subtitle_shear(index),
    ))
}

fn rest_glyph_body_size(index: usize) -> (usize, usize) {
    match index {
        1 => (REST_YU_BODY_WIDTH, REST_EMPHASIZED_BODY_HEIGHT),
        3 | 4 => (REST_EMPHASIZED_BODY_WIDTH, REST_EMPHASIZED_BODY_HEIGHT),
        _ => (REST_BODY_WIDTH, REST_BODY_HEIGHT),
    }
}

fn compose_rest_subtitle(glyphs: &[Image]) -> Result<Image, String> {
    if glyphs.len() != 4 {
        return Err(format!(
            "Expected four BG3 subtitle glyphs, got {}",
            glyphs.len()
        ));
    }
    let mut canvas = Image::transparent(REST_CANVAS_WIDTH, REST_CANVAS_HEIGHT);
    for (index, glyph) in glyphs.iter().enumerate() {
        let subtitle_index = index + 1;
        let (body_width, body_height) = rest_glyph_body_size(subtitle_index);
        let styled = style_subtitle_glyph(glyph, body_width, body_height, subtitle_index)?;
        let x = REST_GLYPH_LEFTS[index];
        let y = REST_BASELINE - body_height;
        overlay(&mut canvas, &styled, x, y)?;
    }
    Ok(canvas)
}

fn subtitle_glyphs(image: &Image) -> Result<Vec<Image>, String> {
    let cropped = crop_alpha(image)?;
    let separated = alpha_column_runs(&cropped);
    let runs = if separated.len() == 5 {
        separated
    } else {
        five_glyph_ranges(&cropped)
    };

    let glyphs: Vec<Image> = runs
        .iter()
        .map(|&(start, end)| {
            let slice = crop(
                &cropped,
                Bounds {
                    x: start,
                    y: 0,
                    width: end - start,
                    height: cropped.height,
                },
            );
            crop_alpha(&slice)
        })
        .collect::<Result<_, String>>()?;

    if glyphs.len() != 5 {
        return Err(format!(
            "Expected five subtitle glyphs, got {}",
            glyphs.len()
        ));
    }
    Ok(glyphs)
}

fn write_flower_tiles(chr: &mut [u8], indexed: &[u8]) -> Result<(), String> {
    for tile_y in 0..12 {
        for tile_x in 0..12 {
            let sprite_row = tile_y / 4;
            let sprite_col = tile_x / 4;
            let local_y = tile_y % 4;
            let local_x = tile_x % 4;
            let tile = FLOWER_BASE_TILES[sprite_row][sprite_col] as usize + local_y * 16 + local_x;
            let mut pixels = [0u8; 64];
            for py in 0..8 {
                for px in 0..8 {
                    pixels[py * 8 + px] = indexed[(tile_y * 8 + py) * 96 + tile_x * 8 + px];
                }
            }
            let encoded = encode_tile_4bpp(&pixels);
            let start = tile * 32;
            if start + 32 > chr.len() {
                return Err(format!("Flower OBJ tile {tile} exceeds CHR"));
            }
            chr[start..start + 32].copy_from_slice(&encoded);
        }
    }
    Ok(())
}

fn write_dae_tiles(chr: &mut [u8], indexed: &[u8]) -> Result<(), String> {
    for tile_y in 0..8 {
        for tile_x in 0..4 {
            let sprite_row = tile_y / 4;
            let local_y = tile_y % 4;
            let tile = DAE_BASE_TILES[sprite_row] as usize + local_y * 16 + tile_x;
            let mut pixels = [0u8; 64];
            for py in 0..8 {
                for px in 0..8 {
                    pixels[py * 8 + px] = indexed[(tile_y * 8 + py) * 32 + tile_x * 8 + px];
                }
            }
            let encoded = encode_tile_4bpp(&pixels);
            let start = tile * 32;
            if start + 32 > chr.len() {
                return Err(format!("대 OBJ tile {tile} exceeds CHR"));
            }
            chr[start..start + 32].copy_from_slice(&encoded);
        }
    }
    Ok(())
}

fn write_mode7_tiles(chr: &mut [u8], indexed: &[u8]) -> Result<(), String> {
    if chr.len() != 256 * 64 || indexed.len() != 256 * 64 {
        return Err(format!(
            "Mode 7 title dimensions drifted: CHR {}, raster {}",
            chr.len(),
            indexed.len()
        ));
    }
    for tile_y in 0..8 {
        for tile_x in 0..32 {
            let tile = tile_y * 32 + tile_x;
            for pixel_y in 0..8 {
                for pixel_x in 0..8 {
                    let source = (tile_y * 8 + pixel_y) * 256 + tile_x * 8 + pixel_x;
                    let destination = tile * 64 + pixel_y * 8 + pixel_x;
                    chr[destination] = u8::from(indexed[source] != 0);
                }
            }
        }
    }
    Ok(())
}

fn write_subtitle_tiles(chr: &mut [u8], indexed: &[u8]) -> Result<usize, String> {
    let mut nonblank = 0;
    for (tile_y, row) in SUBTITLE_TILE_MATRIX.iter().enumerate() {
        for (tile_x, &tile) in row.iter().enumerate() {
            if tile == 0 {
                continue;
            }
            let mut pixels = [0u8; 64];
            for py in 0..8 {
                for px in 0..8 {
                    pixels[py * 8 + px] = indexed[(tile_y * 8 + py) * 96 + tile_x * 8 + px];
                }
            }
            let encoded = encode_tile_2bpp(&pixels);
            if encoded.iter().any(|&byte| byte != 0) {
                nonblank += 1;
            }
            let start = tile as usize * 16;
            if start + 16 > chr.len() {
                return Err(format!("Subtitle BG3 tile {tile} exceeds CHR"));
            }
            chr[start..start + 16].copy_from_slice(&encoded);
        }
    }
    for &tile in SUBTITLE_TEMP_TILES {
        clear_tile(chr, tile, 16)?;
    }
    Ok(nonblank)
}

fn load_lz(rom: &[u8], source: &LzSource) -> Result<(Vec<u8>, Vec<u8>), String> {
    let pc = lorom_to_pc(source.bank, source.addr);
    let original = rom
        .get(pc..pc + source.span)
        .ok_or_else(|| format!("{} source exceeds ROM", source.label))?
        .to_vec();
    let (data, consumed) = decompress_lz(rom, pc)?;
    if consumed != source.span {
        return Err(format!(
            "{} compressed span drifted: expected {}, got {}",
            source.label, source.span, consumed
        ));
    }
    if data.len() != source.decompressed_size {
        return Err(format!(
            "{} decompressed size drifted: expected {}, got {}",
            source.label,
            source.decompressed_size,
            data.len()
        ));
    }
    Ok((data, original))
}

fn write_lz(
    rom: &mut TrackedRom,
    source: &LzSource,
    data: &[u8],
    original: &[u8],
) -> Result<usize, String> {
    let compressed = compress_lz(data);
    let (roundtrip, consumed) = decompress_lz(&compressed, 0)?;
    if roundtrip != data || consumed != compressed.len() {
        return Err(format!("{} LZ round-trip mismatch", source.label));
    }
    if compressed.len() > source.span {
        return Err(format!(
            "{} compressed data does not fit in place: {} > {} bytes",
            source.label,
            compressed.len(),
            source.span
        ));
    }

    let mut replacement = vec![0xFF; source.span];
    replacement[..compressed.len()].copy_from_slice(&compressed);
    rom.write_expect(
        lorom_to_pc(source.bank, source.addr),
        &replacement,
        source.label,
        &Expect::Bytes(original),
    );
    Ok(compressed.len())
}

/// Compile the three transparent PNG masters and patch the original title LZ
/// streams. This must run after `worldmap::apply_worldmap_hook`, because those
/// two screens share three source streams and the world-map hook snapshots the
/// original decompressed data.
pub fn apply_title_screen(
    rom: &mut TrackedRom,
    main_path: &Path,
    hanamaru_path: &Path,
    subtitle_path: &Path,
) -> Result<TitlePatchStats, String> {
    let main_source = crop_alpha(&load_png(main_path)?)?;
    let hanamaru_source = load_png(hanamaru_path)?;
    let subtitle_source = load_png(subtitle_path)?;

    let (mut chr_mode7, chr_mode7_original) = load_lz(rom, &CHR_MODE7)?;
    let (mut chr_a, chr_a_original) = load_lz(rom, &CHR_BG12_A)?;
    let (mut chr_b, chr_b_original) = load_lz(rom, &CHR_BG12_B)?;
    let (mut chr_bg3, chr_bg3_original) = load_lz(rom, &CHR_BG3)?;
    let (mut map_bg1, map_bg1_original) = load_lz(rom, &MAP_BG1)?;
    let (mut map_bg3, map_bg3_original) = load_lz(rom, &MAP_BG3)?;

    let bg1_allowed = tile_ids_from_map(&map_bg1);
    let bg3_allowed = bg3_outline_tiles(chr_bg3.len());
    if bg1_allowed.len() != 179 {
        return Err(format!(
            "BG1 title tile budget drifted: expected 179, got {}",
            bg1_allowed.len()
        ));
    }
    if bg3_allowed.len() != 156 {
        return Err(format!(
            "BG3 outline tile budget drifted: expected 156, got {}",
            bg3_allowed.len()
        ));
    }

    let bg1_blank = u16::from_le_bytes([map_bg1[0], map_bg1[1]]);
    let bg3_blank = u16::from_le_bytes([map_bg3[0], map_bg3[1]]);
    let mut selected = None;
    for max_width in (MAIN_MIN_WIDTH..=MAIN_MAX_WIDTH).rev().step_by(8) {
        let scaled = fit(&main_source, max_width, MAIN_MAX_HEIGHT)?;
        let mut canvas = Image::transparent(SCREEN_WIDTH, SCREEN_HEIGHT);
        let x = (SCREEN_WIDTH - scaled.width) / 2;
        let bounds = place(&mut canvas, &scaled, x, MAIN_TOP)?;
        let bg1_indexed = quantize(&canvas, &BG1_PALETTE);
        let bg3_indexed = structural_layer(&canvas, bounds);
        let bg1_count = encoded_unique_tile_count(&bg1_indexed, 4);
        let bg3_count = encoded_unique_tile_count(&bg3_indexed, 2);
        if bg1_count > bg1_allowed.len() || bg3_count > bg3_allowed.len() {
            continue;
        }

        let mut chr_a_trial = chr_a.clone();
        let mut chr_bg3_trial = chr_bg3.clone();
        let mut map_bg1_trial = map_bg1.clone();
        let mut map_bg3_trial = map_bg3.clone();
        pack_layer(
            &bg1_indexed,
            4,
            &bg1_allowed,
            &mut chr_a_trial,
            &mut map_bg1_trial,
            bg1_blank,
        )?;
        pack_layer(
            &bg3_indexed,
            2,
            &bg3_allowed,
            &mut chr_bg3_trial,
            &mut map_bg3_trial,
            bg3_blank,
        )?;
        if compress_lz(&chr_a_trial).len() <= CHR_BG12_A.span
            && compress_lz(&map_bg1_trial).len() <= MAP_BG1.span
            && compress_lz(&map_bg3_trial).len() <= MAP_BG3.span
        {
            selected = Some((scaled.width, scaled.height, bg1_indexed, bg3_indexed));
            break;
        }
    }
    let (main_width, main_height, bg1_indexed, bg3_indexed) =
        selected.ok_or("Main title cannot fit the protected BG1/BG3 tile budgets")?;

    let bg1_tiles = pack_layer(
        &bg1_indexed,
        4,
        &bg1_allowed,
        &mut chr_a,
        &mut map_bg1,
        bg1_blank,
    )?;
    let bg3_tiles = pack_layer(
        &bg3_indexed,
        2,
        &bg3_allowed,
        &mut chr_bg3,
        &mut map_bg3,
        bg3_blank,
    )?;

    let mut mode7_indexed = vec![0u8; 256 * 64];
    for y in 0..main_height {
        let source_y = MAIN_TOP + y;
        for x in 0..SCREEN_WIDTH {
            mode7_indexed[y * 256 + x] = u8::from(bg3_indexed[source_y * SCREEN_WIDTH + x] != 0);
        }
    }
    write_mode7_tiles(&mut chr_mode7, &mode7_indexed)?;

    // Keep a one-pixel margin below the old 87 px bound so the rotated OBJ set
    // remains inside its fixed LZ slot.
    let flower = fit(&hanamaru_source, FLOWER_MAX_SIZE, FLOWER_MAX_SIZE)?;
    let mut flower_canvas = Image::transparent(96, 96);
    let flower_y = (96 - flower.height) / 2;
    place(&mut flower_canvas, &flower, 0, flower_y)?;
    // The alternate `대` tiles below sample this same rotated canvas, keeping
    // their cloud fragment aligned when the subtitle enters.
    let flower_canvas = rotate_nearest(&flower_canvas, FLOWER_ROTATION_DEGREES_CCW);
    let flower_indexed = quantize(&flower_canvas, &OBJ_PALETTE_1);
    write_flower_tiles(&mut chr_b, &flower_indexed)?;

    let subtitle_glyphs = subtitle_glyphs(&subtitle_source)?;

    let dae = style_subtitle_glyph(
        &subtitle_glyphs[0],
        DAE_BODY_WIDTH,
        SUBTITLE_GLYPH_HEIGHT,
        0,
    )?;
    let mut dae_canvas = Image::transparent(32, 64);
    // The original animation does not allocate two new sprites for `大`.
    // Instead it moves the flower's right-hand sprites up by eight pixels and
    // switches them to adjacent CHR tiles. Rebuild the flower fragment that
    // those alternate tiles replace, then place `대` over it.
    for y in 0..64 {
        for x in 0..32 {
            let flower_x = 64 + x;
            let flower_y = 24 + y;
            if flower_x < flower_canvas.width && flower_y < flower_canvas.height {
                dae_canvas.set(x, y, flower_canvas.get(flower_x, flower_y));
            }
        }
    }
    let dae_x = dae_canvas.width - dae.width;
    overlay(&mut dae_canvas, &dae, dae_x, DAE_TOP)?;
    let dae_indexed = quantize(&dae_canvas, &OBJ_PALETTE_1);
    write_dae_tiles(&mut chr_b, &dae_indexed)?;

    let rest_canvas = compose_rest_subtitle(&subtitle_glyphs[1..])?;
    let rest_indexed = rest_canvas
        .pixels
        .iter()
        .map(|pixel| {
            if pixel.a <= 16 {
                0
            } else {
                let luminance = (pixel.r as u16 * 3 + pixel.g as u16 * 6 + pixel.b as u16) / 10;
                if luminance >= 128 {
                    3
                } else {
                    2
                }
            }
        })
        .collect::<Vec<_>>();
    let subtitle_tiles = write_subtitle_tiles(&mut chr_bg3, &rest_indexed)?;

    let mode7_size = write_lz(rom, &CHR_MODE7, &chr_mode7, &chr_mode7_original)?;
    let chr_a_size = write_lz(rom, &CHR_BG12_A, &chr_a, &chr_a_original)?;
    let chr_b_size = write_lz(rom, &CHR_BG12_B, &chr_b, &chr_b_original)?;
    let chr_bg3_size = write_lz(rom, &CHR_BG3, &chr_bg3, &chr_bg3_original)?;
    let map_bg1_size = write_lz(rom, &MAP_BG1, &map_bg1, &map_bg1_original)?;
    let map_bg3_size = write_lz(rom, &MAP_BG3, &map_bg3, &map_bg3_original)?;

    println!(
        "  LZ sizes: Mode7 {mode7_size}/{}, BG1/2-A {chr_a_size}/{}, OBJ-B {chr_b_size}/{}, \
         BG3 {chr_bg3_size}/{}, BG1 map {map_bg1_size}/{}, BG3 map {map_bg3_size}/{}",
        CHR_MODE7.span, CHR_BG12_A.span, CHR_BG12_B.span, CHR_BG3.span, MAP_BG1.span, MAP_BG3.span,
    );

    Ok(TitlePatchStats {
        main_width,
        main_height,
        bg1_tiles,
        bg3_tiles,
        subtitle_tiles,
    })
}

#[cfg(test)]
#[path = "title_screen_tests.rs"]
mod tests;
