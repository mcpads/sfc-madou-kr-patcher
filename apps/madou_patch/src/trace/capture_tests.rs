use super::*;

#[test]
fn render_dimensions_for_full_vram() {
    let vram = vec![0u8; 0x10000];
    let cgram = [0u8; 512];
    let (w, h, rgb) = render_vram_tiles_rgb(&vram, &cgram, 0, 64).unwrap();
    assert_eq!(w, 512);
    assert_eq!(h, 256);
    assert_eq!(rgb.len(), 512 * 256 * 3);
}

#[test]
fn render_single_tile_uses_palette_color() {
    let mut vram = vec![0u8; 32];
    // Pixel (0,0) uses color index 1 (bit7 in plane0 set).
    vram[0] = 0x80;

    let mut cgram = [0u8; 512];
    // color #1 = red max (BGR555 with R=31)
    cgram[2] = 0x1F;
    cgram[3] = 0x00;

    let (w, h, rgb) = render_vram_tiles_rgb(&vram, &cgram, 0, 1).unwrap();
    assert_eq!(w, 8);
    assert_eq!(h, 8);
    assert_eq!(&rgb[0..3], &[255, 0, 0]);
}

#[test]
fn invalid_palette_rejected() {
    let vram = vec![0u8; 32];
    let cgram = [0u8; 512];
    let err = render_vram_tiles_rgb(&vram, &cgram, 8, 1).unwrap_err();
    assert!(err.contains("palette"));
}

#[test]
fn zero_columns_rejected() {
    let vram = vec![0u8; 32];
    let cgram = [0u8; 512];
    let err = render_vram_tiles_rgb(&vram, &cgram, 0, 0).unwrap_err();
    assert!(err.contains("columns"));
}
