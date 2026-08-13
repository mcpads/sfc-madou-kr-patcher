//! VRAM screenshot capture helpers for trace mode.
//!
//! This is a debug visualization of VRAM as 4bpp 8x8 tile atlas.

use std::path::Path;

fn expand_5_to_8(v: u8) -> u8 {
    (v << 3) | (v >> 2)
}

fn cgram_color(cgram: &[u8; 512], color_index: usize) -> (u8, u8, u8) {
    let i = color_index.saturating_mul(2);
    if i + 1 >= cgram.len() {
        return (0, 0, 0);
    }
    let lo = cgram[i] as u16;
    let hi = cgram[i + 1] as u16;
    let bgr15 = lo | (hi << 8);

    let r5 = (bgr15 & 0x1F) as u8;
    let g5 = ((bgr15 >> 5) & 0x1F) as u8;
    let b5 = ((bgr15 >> 10) & 0x1F) as u8;
    (expand_5_to_8(r5), expand_5_to_8(g5), expand_5_to_8(b5))
}

fn palette_is_empty(cgram: &[u8; 512], palette: u8) -> bool {
    let base = (palette as usize) * 16;
    (0..16).all(|idx| {
        let (r, g, b) = cgram_color(cgram, base + idx);
        r == 0 && g == 0 && b == 0
    })
}

/// Render VRAM to RGB tile atlas using SNES 4bpp tile layout.
pub fn render_vram_tiles_rgb(
    vram: &[u8],
    cgram: &[u8; 512],
    palette: u8,
    columns: usize,
) -> Result<(usize, usize, Vec<u8>), String> {
    if columns == 0 {
        return Err("columns must be > 0".to_string());
    }
    if palette > 7 {
        return Err("palette must be in range 0..7".to_string());
    }
    if vram.is_empty() {
        return Err("vram buffer is empty".to_string());
    }

    let tile_size = 32usize;
    let tile_count = vram.len() / tile_size;
    if tile_count == 0 {
        return Err("vram buffer too small for 4bpp tiles".to_string());
    }

    let cols = columns.min(tile_count).max(1);
    let rows = tile_count.div_ceil(cols);
    let width = cols * 8;
    let height = rows * 8;
    let mut out = vec![0u8; width * height * 3];
    let pal_base = (palette as usize) * 16;
    let grayscale = palette_is_empty(cgram, palette);

    for tile_idx in 0..tile_count {
        let tile_base = tile_idx * tile_size;
        let tile_x = (tile_idx % cols) * 8;
        let tile_y = (tile_idx / cols) * 8;
        if tile_y >= height {
            break;
        }

        for row in 0..8usize {
            let p0 = vram[tile_base + row * 2];
            let p1 = vram[tile_base + row * 2 + 1];
            let p2 = vram[tile_base + 16 + row * 2];
            let p3 = vram[tile_base + 16 + row * 2 + 1];

            for col in 0..8usize {
                let bit = 7 - col;
                let idx = ((p0 >> bit) & 1)
                    | (((p1 >> bit) & 1) << 1)
                    | (((p2 >> bit) & 1) << 2)
                    | (((p3 >> bit) & 1) << 3);
                let (r, g, b) = if grayscale {
                    let v = idx.wrapping_mul(17);
                    (v, v, v)
                } else {
                    cgram_color(cgram, pal_base + idx as usize)
                };

                let x = tile_x + col;
                let y = tile_y + row;
                let o = (y * width + x) * 3;
                out[o] = r;
                out[o + 1] = g;
                out[o + 2] = b;
            }
        }
    }

    Ok((width, height, out))
}

/// Write a trace screenshot as PPM (P6).
pub fn write_vram_tiles_ppm(
    path: &Path,
    vram: &[u8],
    cgram: &[u8; 512],
    palette: u8,
    columns: usize,
) -> Result<(), String> {
    let (width, height, rgb) = render_vram_tiles_rgb(vram, cgram, palette, columns)?;
    let mut bytes = Vec::with_capacity(24 + rgb.len());
    bytes.extend_from_slice(format!("P6\n{} {}\n255\n", width, height).as_bytes());
    bytes.extend_from_slice(&rgb);
    std::fs::write(path, bytes).map_err(|e| format!("failed to write {}: {}", path.display(), e))
}

#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;
