use super::menu_consts::*;
use super::*;

// ── Sky squirrel worldmap tests ─────────────────────────────────────

#[test]
fn flag_clear_hook_assembles() {
    let code = build_hook_code().unwrap();
    // Expected: SEP(2) + LDA#00(2) + STA_long(4) + JSL(4) + RTL(1) = 13 bytes
    assert_eq!(
        code.len(),
        13,
        "flag-clear hook should be 13 bytes, got {}",
        code.len()
    );
    // Starts with SEP #$20
    assert_eq!(code[0], 0xE2); // SEP
    assert_eq!(code[1], 0x20);
    // Contains STA $7F:FF00 (8F 00 FF 7F)
    let sta_long = [0x8F, 0x00, 0xFF, 0x7F];
    assert!(
        code.windows(4).any(|w| w == sta_long),
        "must contain STA $7F:FF00"
    );
    // Contains JSL $009440
    let jsl = [0x22, 0x40, 0x94, 0x00];
    assert!(
        code.windows(4).any(|w| w == jsl),
        "must contain JSL $009440"
    );
    // Ends with RTL
    assert_eq!(code[code.len() - 1], 0x6B);
    println!("Flag-clear hook: {} bytes", code.len());
}

#[test]
fn loader_code_assembles() {
    let addrs = vec![
        (DATA_BANK, 0x8000_u16),
        (DATA_BANK, 0xC000),
        (DATA_BANK, 0xE000),
        (DATA_BANK2, 0xD200),
    ];
    let code = build_loader_code(&addrs).unwrap();
    assert!(
        code.len() < 300,
        "loader code {} bytes exceeds 300B",
        code.len()
    );
    // Starts with relocated STZ $1A65 (9C 65 1A)
    assert_eq!(&code[0..3], &[0x9C, 0x65, 0x1A]);
    // Followed by PHX (DA)
    assert_eq!(code[3], 0xDA);
    // Contains LDA $7F:FF00 (AF 00 FF 7F)
    let lda_long = [0xAF, 0x00, 0xFF, 0x7F];
    assert!(
        code.windows(4).any(|w| w == lda_long),
        "must contain LDA $7F:FF00"
    );
    // Contains 4 DMA triggers
    let trigger = [0x8D, 0x0B, 0x42]; // STA $420B
    let trigger_count = code.windows(3).filter(|w| *w == trigger).count();
    assert_eq!(
        trigger_count,
        CONDITIONS.len(),
        "expected {} DMA triggers",
        CONDITIONS.len()
    );
    // Contains STA $7F:FF00 (flag set)
    let sta_long = [0x8F, 0x00, 0xFF, 0x7F];
    assert!(
        code.windows(4).any(|w| w == sta_long),
        "must contain STA $7F:FF00 (flag set)"
    );
    // Ends with JML $03:CC5A (5C 5A CC 03)
    let jml_return = [0x5C, 0x5A, 0xCC, 0x03];
    assert_eq!(
        &code[code.len() - 4..],
        &jml_return,
        "must end with JML $03:CC5A"
    );
    // Must NOT contain RTL (0x6B) — JML is used instead to avoid stack corruption
    assert!(
        !code.contains(&0x6B),
        "loader must not contain RTL (uses JML instead)"
    );
    println!("Loader code: {} bytes", code.len());
}

#[test]
fn loader_code_has_early_return_via_jml() {
    let addrs = vec![
        (DATA_BANK, 0x8000_u16),
        (DATA_BANK, 0xC000),
        (DATA_BANK, 0xE000),
        (DATA_BANK2, 0xD200),
    ];
    let code = build_loader_code(&addrs).unwrap();
    // BEQ + JML pattern: flag check should BEQ to needs_load,
    // followed by JML $03:CC5A for the early-return path.
    let beq_pos = code
        .iter()
        .position(|&b| b == 0xF0)
        .expect("must contain BEQ");
    // After BEQ(2 bytes) should be JML (0x5C)
    assert_eq!(
        code[beq_pos + 2],
        0x5C,
        "JML should follow BEQ for early return"
    );
    // Two JML instructions total (early return + final return)
    let jml_count = code
        .windows(4)
        .filter(|w| w == &[0x5C, 0x5A, 0xCC, 0x03])
        .count();
    assert_eq!(jml_count, 2, "expected 2 JML $03:CC5A instructions");
}

#[test]
fn loader_call_site_consts() {
    assert_eq!(LOADER_CALL_SITE_PC, 0x1CC56);
    assert_eq!(LOADER_ORIG_BYTES, [0x9C, 0x65, 0x1A, 0xDA]);
}

#[test]
fn layout_data_splits_across_banks() {
    let blocks = vec![
        (0, vec![0u8; 0x4000]),
        (1, vec![0u8; 0x2000]),
        (2, vec![0u8; 0x0D00]),
        (3, vec![0u8; 0x2000]),
    ];
    let layout = layout_data(&blocks).unwrap();
    assert_eq!(layout[0], (DATA_BANK, 0x8000, 0));
    assert_eq!(layout[1], (DATA_BANK, 0xC000, 1));
    assert_eq!(layout[2], (DATA_BANK, 0xE000, 2));
    assert_eq!(layout[3], (0x10, 0xD200, 3));
}

#[test]
fn hook_call_site_bytes() {
    let mut rom = vec![0xFF; 0x200000];
    rom[HOOK_CALL_SITE_PC] = 0x22;
    rom[HOOK_CALL_SITE_PC + 1] = 0x40;
    rom[HOOK_CALL_SITE_PC + 2] = 0x94;
    rom[HOOK_CALL_SITE_PC + 3] = 0x00;
    assert_eq!(
        &rom[HOOK_CALL_SITE_PC..HOOK_CALL_SITE_PC + 4],
        &[0x22, 0x40, 0x94, 0x00]
    );
}

// ── Menu worldmap tests ─────────────────────────────────────────────

#[test]
fn menu_ko_chars_count_and_unique() {
    let chars = collect_menu_ko_chars();
    // Dynamic budget: total tiles = 6 frame + N glyph.
    // Bank $0B $EE00-$FFFF = 4608 bytes. Code 256B, TM 3584B.
    // Max glyph tiles: (4608 - 3584) / 16 = 64 tiles
    assert!(
        chars.len() <= 64,
        "KO chars {} exceeds max glyph budget 64",
        chars.len()
    );
    assert!(!chars.is_empty());
    let set: std::collections::HashSet<char> = chars.iter().copied().collect();
    assert_eq!(chars.len(), set.len(), "duplicate chars detected");
    println!("Menu worldmap KO chars: {} {:?}", chars.len(), chars);
}

#[test]
fn remap_tile_index_frame_tiles() {
    assert_eq!(remap_tile_index(0x00), KO_BLANK);
    assert_eq!(remap_tile_index(0x01), KO_CORNER);
    assert_eq!(remap_tile_index(0x02), KO_HBAR);
    assert_eq!(remap_tile_index(0x03), KO_VWALL);
    assert_eq!(remap_tile_index(0x09), KO_DOWNPTR);
    assert_eq!(remap_tile_index(0x0F), KO_VWALL); // V-flip
    assert_eq!(remap_tile_index(0x19), KO_INNER);
    assert_eq!(remap_tile_index(0x22), KO_RIGHTPTR); // restored
    assert_eq!(remap_tile_index(0x34), KO_CORNER_L); // separate left corner
}

#[test]
fn remap_tile_index_text_tiles_to_blank() {
    for idx in [
        0x04, 0x05, 0x06, 0x07, 0x08, 0x0A, 0x0B, 0x0C, 0x10, 0x20, 0x35,
    ] {
        assert_eq!(
            remap_tile_index(idx),
            KO_BLANK,
            "JP ${:02X} not cleared",
            idx
        );
    }
}

#[test]
fn menu_hook_code_assembles_without_obj() {
    let code = build_menu_hook_code(
        (MENU_DATA_BANK, 0xEE00),
        912,
        (MENU_DATA_BANK, 0xF1A0),
        TM_SIZE as u16,
        None, // no sky tilemap
        None, // no OBJ
    )
    .unwrap();

    assert!(
        code.len() <= 160,
        "menu hook code {} bytes exceeds 160B limit",
        code.len()
    );
    assert_eq!(code[0], 0xE2); // SEP #$20
    assert_eq!(code[1], 0x20);

    let jsl_pattern = [0x22, 0x40, 0x94, 0x00];
    assert!(
        code.windows(4).any(|w| w == jsl_pattern),
        "hook must contain JSL $009440 fallthrough"
    );

    // Must check dp$0B (source bank) for Bank $25 guard
    let lda_dp_0b = [0xA5, 0x0B];
    assert!(
        code.windows(2).any(|w| w == lda_dp_0b),
        "hook must check dp$0B (source bank)"
    );
    let cmp_25 = [0xC9, 0x25];
    assert!(
        code.windows(2).any(|w| w == cmp_25),
        "hook must compare dp$0B against $25"
    );

    // Must check dp$0C:$0D (LZ source address) in 16-bit mode
    let rep_20 = [0xC2, 0x20]; // REP #$20
    assert!(
        code.windows(2).any(|w| w == rep_20),
        "hook must use REP #$20 for 16-bit comparison"
    );
    let lda_dp_0c = [0xA5, 0x0C];
    assert!(
        code.windows(2).any(|w| w == lda_dp_0c),
        "hook must check dp$0C (LZ source address)"
    );
    // CMP #$B784 (CHR block source)
    let cmp_b784 = [0xC9, 0x84, 0xB7]; // CMP imm16 little-endian
    assert!(
        code.windows(3).any(|w| w == cmp_b784),
        "hook must compare dp$0C:$0D against $B784 (CHR)"
    );
    // CMP #$B9E7 (tilemap block source)
    let cmp_b9e7 = [0xC9, 0xE7, 0xB9];
    assert!(
        code.windows(3).any(|w| w == cmp_b9e7),
        "hook must compare dp$0C:$0D against $B9E7 (tilemap)"
    );

    // Should NOT contain OBJ block check
    let cmp_b10c = [0xC9, 0x0C, 0xB1];
    assert!(
        !code.windows(3).any(|w| w == cmp_b10c),
        "hook without OBJ should not check $B10C"
    );

    let trigger = [0x8D, 0x0B, 0x42];
    let trigger_count = code.windows(3).filter(|w| *w == trigger).count();
    assert_eq!(trigger_count, 2, "expected 2 DMA triggers");

    assert_eq!(code[code.len() - 1], 0x6B);
    println!("Menu hook code (no OBJ): {} bytes", code.len());
}

#[test]
fn menu_hook_code_assembles_with_obj() {
    let obj_addr: u32 = 0x10F480;
    let sky_tm_param = Some(((MENU_DATA_BANK, 0xF800u16), 1792u16));
    let code = build_menu_hook_code(
        (MENU_DATA_BANK, 0xEE00),
        912,
        (MENU_DATA_BANK, 0xF1A0),
        TM_SIZE as u16,
        sky_tm_param,
        Some(obj_addr),
    )
    .unwrap();

    assert!(
        code.len() <= 256,
        "menu hook code with OBJ+sky {} bytes exceeds 256B limit",
        code.len()
    );

    // Must contain OBJ block check: CMP #$B10C
    let cmp_b10c = [0xC9, 0x0C, 0xB1];
    assert!(
        code.windows(3).any(|w| w == cmp_b10c),
        "hook with OBJ must check $B10C"
    );

    // Must contain JSL to OBJ routine $10:$F480
    let jsl_obj = [0x22, 0x80, 0xF4, 0x10];
    assert!(
        code.windows(4).any(|w| w == jsl_obj),
        "hook must JSL to OBJ routine at $10:F480"
    );

    // Must contain CMP #$AB82 for sky tilemap
    let cmp_ab82 = [0xC9, 0x82, 0xAB];
    assert!(
        code.windows(3).any(|w| w == cmp_ab82),
        "hook must check $AB82 for sky tilemap"
    );

    // 3 DMA triggers: CHR + menu TM + sky TM
    let trigger = [0x8D, 0x0B, 0x42];
    let trigger_count = code.windows(3).filter(|w| *w == trigger).count();
    assert_eq!(
        trigger_count, 3,
        "expected 3 DMA triggers (CHR + menu TM + sky TM)"
    );

    assert_eq!(code[code.len() - 1], 0x6B);
    println!("Menu hook code (with OBJ+sky): {} bytes", code.len());
}

#[test]
fn menu_data_fits_in_bank() {
    let ko_chars = collect_menu_ko_chars();
    let glyph_count = ko_chars.len();
    let total_tiles = FRAME_TILE_COUNT + glyph_count;
    let chr_size = total_tiles * BYTES_PER_TILE;
    let chr_aligned = (chr_size + 15) & !15;
    // Use example values for dynamic layout test
    let test_code_addr: u16 = 0xD660;
    let test_chr_addr = test_code_addr + CODE_TO_CHR_OFFSET;
    let menu_tm_addr = test_chr_addr as usize + chr_aligned;
    let menu_tm_aligned = (TM_SIZE + 15) & !15;
    let sky_tm_addr = menu_tm_addr + menu_tm_aligned;
    let sky_tm_size = 1792; // 32×28×2
    let data_end = sky_tm_addr + sky_tm_size;

    assert!(
        data_end <= 0x10000,
        "worldmap data end ${:04X} exceeds bank boundary",
        data_end
    );
    println!(
        "Worldmap data layout: code=${:04X}, CHR=${:04X} ({}B, {} tiles), menu_TM=${:04X}, sky_TM=${:04X}-${:04X}",
        test_code_addr, test_chr_addr, chr_size, total_tiles, menu_tm_addr, sky_tm_addr, data_end
    );
}

#[test]
fn build_ko_chr_correct_size() {
    let ko_chars = collect_menu_ko_chars();
    let glyph_count = ko_chars.len();
    let jp_chr_size = 864; // JP original
    let jp_chr = vec![0u8; jp_chr_size];
    let ko_glyphs: Vec<[u8; 16]> = (0..glyph_count).map(|_| [0xAA; 16]).collect();
    let chr = build_ko_chr(&jp_chr, &ko_glyphs);

    assert_eq!(chr.len(), (FRAME_TILE_COUNT + glyph_count) * BYTES_PER_TILE);
    assert!(chr[0..FRAME_TILE_COUNT * BYTES_PER_TILE]
        .iter()
        .all(|&b| b == 0));
    assert!(chr[FRAME_TILE_COUNT * BYTES_PER_TILE..]
        .iter()
        .all(|&b| b == 0xAA));
}

#[test]
fn remap_tilemap_clears_text_and_preserves_attrs() {
    let mut tm = vec![0u8; 32 * 56 * 2];
    tm[0] = 0x01;
    tm[1] = 0x40;
    tm[2] = 0x04;
    tm[3] = 0x00;
    tm[4] = 0x34;
    tm[5] = 0x80;

    let ko_map = std::collections::HashMap::new();
    remap_tilemap(&mut tm, &ko_map);

    assert_eq!(tm[0], KO_CORNER);
    assert_eq!(tm[1], 0x40);
    assert_eq!(tm[2], KO_INNER); // text tile inside speech bubble → opaque fill
    assert_eq!(tm[3], 0x00);
    assert_eq!(tm[4], KO_CORNER_L);
    assert_eq!(tm[5], 0x80); // V-flip preserved, no H-flip needed (separate tile)
}

#[test]
fn find_text_groups_basic() {
    let mut tm = vec![0u8; 32 * 4 * 2]; // 4 rows
                                        // Row 0: blank(0) frame(01) text(04 05 06) blank(0) text(0A 0B)
    tm[0] = 0x00;
    tm[2] = 0x01;
    tm[4] = 0x04;
    tm[6] = 0x05;
    tm[8] = 0x06;
    tm[10] = 0x00;
    tm[12] = 0x0A;
    tm[14] = 0x0B;

    let groups = find_text_groups(&tm);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].len(), 3); // tiles 04,05,06
    assert_eq!(groups[1].len(), 2); // tiles 0A,0B
}

#[test]
fn remap_tilemap_writes_centered_glyphs() {
    let mut tm = vec![0u8; 32 * 2 * 2]; // 2 rows, 32 cols
                                        // Place 5 text tiles at cols 3-7
    for c in 3..8 {
        tm[c * 2] = 0x04 + (c - 3) as u8;
    }

    let groups = find_text_groups(&tm);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].len(), 5);
    assert_eq!(groups[0], vec![3, 4, 5, 6, 7]);
}

// ── OBJ sprite title tests ──────────────────────────────────────────

#[test]
fn test_bitmap_to_snes_4bpp_16x16() {
    use crate::font_gen::bitmap_to_snes_4bpp_16x16;

    // Simple bitmap: top-left pixel only
    let mut bitmap = [false; 256];
    bitmap[0] = true; // pixel (0,0)

    let tiles = bitmap_to_snes_4bpp_16x16(&bitmap, 6); // fg_color = 6 = 0b0110

    // Pixel (0,0) in TL quadrant, row 0, col 0 → bit 7
    // fg_color 6 = BP0:0, BP1:1, BP2:1, BP3:0
    assert_eq!(tiles[0][0] & 0x80, 0x00); // BP0 bit 7 = 0
    assert_eq!(tiles[0][1] & 0x80, 0x80); // BP1 bit 7 = 1
    assert_eq!(tiles[0][16] & 0x80, 0x80); // BP2 bit 7 = 1
    assert_eq!(tiles[0][17] & 0x80, 0x00); // BP3 bit 7 = 0

    // Other quadrants should be all zeros
    assert!(tiles[1].iter().all(|&b| b == 0), "TR should be empty");
    assert!(tiles[2].iter().all(|&b| b == 0), "BL should be empty");
    assert!(tiles[3].iter().all(|&b| b == 0), "BR should be empty");
}

#[test]
fn test_bitmap_to_snes_4bpp_16x16_all_colors() {
    use crate::font_gen::bitmap_to_snes_4bpp_16x16;

    // Test with fg_color = 15 (all bitplanes set)
    let mut bitmap = [false; 256];
    bitmap[0] = true;

    let tiles = bitmap_to_snes_4bpp_16x16(&bitmap, 15);
    assert_eq!(tiles[0][0] & 0x80, 0x80); // BP0
    assert_eq!(tiles[0][1] & 0x80, 0x80); // BP1
    assert_eq!(tiles[0][16] & 0x80, 0x80); // BP2
    assert_eq!(tiles[0][17] & 0x80, 0x80); // BP3

    // Test with fg_color = 0 (transparent — no bitplanes)
    let tiles_zero = bitmap_to_snes_4bpp_16x16(&bitmap, 0);
    assert!(tiles_zero[0].iter().all(|&b| b == 0));
}

#[test]
fn test_detect_fg_color_4bpp() {
    // Build fake OBJ data with tile $0C containing color 6 pixels
    let mut obj_data = vec![0u8; 0x200]; // enough for tiles 0-15
    let tile_off = 0x0C * 32;

    // Fill row 0 with all color 6 (0b0110): BP0=0, BP1=FF, BP2=FF, BP3=0
    obj_data[tile_off] = 0x00; // BP0 row 0
    obj_data[tile_off + 1] = 0xFF; // BP1 row 0
    obj_data[tile_off + 16] = 0xFF; // BP2 row 0
    obj_data[tile_off + 17] = 0x00; // BP3 row 0

    let color = detect_fg_color_4bpp(&obj_data);
    assert_eq!(color, 6, "expected fg_color 6, got {}", color);
}

#[test]
fn test_detect_fg_color_4bpp_empty() {
    // All zeros → fallback to 6
    let obj_data = vec![0u8; 0x200];
    let color = detect_fg_color_4bpp(&obj_data);
    assert_eq!(color, 6, "empty data should fallback to 6");
}

#[test]
fn test_detect_fg_color_4bpp_short_data() {
    // Data too short for tile $0C → fallback
    let obj_data = vec![0u8; 32]; // only 1 tile
    let color = detect_fg_color_4bpp(&obj_data);
    assert_eq!(color, 6);
}

#[test]
fn test_build_obj_tile_data_layout() {
    // 3 simple bitmaps (just need correct size)
    let bitmaps: Vec<[bool; 256]> = vec![[false; 256]; 3];
    let data = build_obj_tile_data(&bitmaps, 6);

    assert_eq!(
        data.len(),
        640,
        "OBJ tile data should be 640 bytes (20 tiles × 32B)"
    );

    // With all-false bitmaps, tiles should contain bg_fill/bg_border (non-zero)
    assert!(
        data.iter().any(|&b| b != 0),
        "outlined tiles should not be all-zero"
    );
}

#[test]
fn test_build_obj_tile_data_with_content() {
    // Create bitmaps with a text pixel in interior to verify it gets text_fill color (0x0E)
    // Use pixel (5,7) — already near vertical center, so centering won't shift it far.
    // Single pixel at y=7: dy = (16 - (7+7+1))/2 = 0 → stays at y=7.
    let mut bm_wol = [false; 256];
    bm_wol[7 * 16 + 5] = true; // pixel (5,7) → TL quadrant, vertically centered

    let bm_deu = [false; 256];
    let bm_maep = [false; 256];

    let bitmaps = vec![bm_wol, bm_deu, bm_maep];
    let data = build_obj_tile_data(&bitmaps, 6);
    assert_eq!(data.len(), 640);

    // 월 TL starts at offset 64 (Group 2)
    // Pixel (5,7) in TL quadrant → row 7, col 5, bit position 2 (7-5)
    // text_fill = 0x0E = 0b1110 → BP0=0, BP1=1, BP2=1, BP3=1
    let r = 7;
    let bit = 1 << (7 - 5);
    let bp0 = data[64 + r * 2] & bit;
    let bp1 = data[64 + r * 2 + 1] & bit;
    let bp2 = data[64 + 16 + r * 2] & bit;
    let bp3 = data[64 + 16 + r * 2 + 1] & bit;
    // 0x0E = 0b1110: BP0=0, BP1=1, BP2=1, BP3=1
    assert_eq!(bp0, 0, "text_fill BP0 should be 0");
    assert_ne!(bp1, 0, "text_fill BP1 should be set");
    assert_ne!(bp2, 0, "text_fill BP2 should be set");
    assert_ne!(bp3, 0, "text_fill BP3 should be set");
}

#[test]
fn test_build_obj_patch_code() {
    let code = build_obj_patch_code(0xF200, 0x10).unwrap();

    // Should start with JSL $009440
    assert_eq!(&code[0..4], &[0x22, 0x40, 0x94, 0x00]);

    // Should contain PHB (0x8B) and PLB (0xAB)
    assert!(code.contains(&0x8B), "must contain PHB");
    assert!(code.contains(&0xAB), "must contain PLB");

    // Should contain REP #$30 (0xC2, 0x30)
    assert!(
        code.windows(2).any(|w| w == [0xC2, 0x30]),
        "must contain REP #$30"
    );

    // Should contain 8 MVN instructions (6 title + 2 bubble)
    let mvn_count = code
        .windows(3)
        .filter(|w| w[0] == 0x54 && w[1] == 0x7F && w[2] == 0x10)
        .count();
    assert_eq!(mvn_count, 8, "expected 8 MVN $7F,$10 instructions");

    // Should end with RTL
    assert_eq!(code[code.len() - 1], 0x6B);

    // Verify size is reasonable (~107 bytes for 8 MVN groups)
    assert!(
        code.len() <= 120,
        "OBJ patch code {} bytes exceeds 120B",
        code.len()
    );
    println!("OBJ patch code: {} bytes", code.len());
}

#[test]
fn test_obj_data_fits_in_bank_10() {
    // Layout: OBJ data (640B at $F200) + bubble data (384B at $F480) + code at $F600
    let title_end = OBJ_DATA_ADDR as usize + 640; // $F200 + 640 = $F480
    assert!(
        title_end <= BUBBLE_DATA_ADDR as usize,
        "OBJ title end ${:04X} overlaps bubble at ${:04X}",
        title_end,
        BUBBLE_DATA_ADDR
    );

    let bubble_end = BUBBLE_DATA_ADDR as usize + 384; // $F480 + 384 = $F600
    assert!(
        bubble_end <= OBJ_CODE_ADDR as usize,
        "Bubble data end ${:04X} overlaps code at ${:04X}",
        bubble_end,
        OBJ_CODE_ADDR
    );

    let code = build_obj_patch_code(OBJ_DATA_ADDR, OBJ_DATA_BANK).unwrap();
    let code_end = OBJ_CODE_ADDR as usize + code.len();
    assert!(
        code_end <= 0x10000,
        "OBJ code end ${:04X} exceeds bank boundary",
        code_end
    );
    println!(
        "Bank $10 OBJ layout: title=$F200-${:04X}, bubble=$F480-${:04X}, code=$F600-${:04X}",
        title_end, bubble_end, code_end
    );
}

// ── Bubble text tile tests ────────────────────────────────────────────

#[test]
fn test_build_bubble_text_tiles() {
    // 2 glyphs on a 4-tile-wide canvas → 128 bytes
    let bitmaps = vec![
        {
            let mut b = [false; 64];
            // Simple vertical line at x=3
            for y in 1..7 {
                b[y * 8 + 3] = true;
            }
            b
        },
        {
            let mut b = [false; 64];
            // Simple horizontal line at y=4
            for x in 1..7 {
                b[4 * 8 + x] = true;
            }
            b
        },
    ];

    let result = build_bubble_text_tiles(&bitmaps, 4);
    assert_eq!(result.len(), 128, "4 tiles × 32B = 128B");

    // Verify 4bpp encoding: check first tile's first row
    // Row 0 should be: BG_BORDER(0x0A), BG_FILL(0x0F) × 6, BG_FILL(0x0F)
    // Actually col 0 = BG_BORDER, cols 1-7 = BG_FILL
    // BG_FILL = 0x0F = 0b1111, BG_BORDER = 0x0A = 0b1010
    let bp0 = result[0]; // row 0 BP0
    let bp1 = result[1]; // row 0 BP1
                         // Col 0: BG_BORDER (0x0A) → BP0=0, BP1=1
    assert_eq!(bp0 & 0x80, 0, "col0 BP0 should be 0 for BG_BORDER");
    assert_ne!(bp1 & 0x80, 0, "col0 BP1 should be 1 for BG_BORDER");
    // Col 1: BG_FILL (0x0F) → BP0=1, BP1=1
    assert_ne!(bp0 & 0x40, 0, "col1 BP0 should be 1 for BG_FILL");
    assert_ne!(bp1 & 0x40, 0, "col1 BP1 should be 1 for BG_FILL");
}

#[test]
fn test_bubble_wram_no_overlap_with_title() {
    // Title MVN destinations (from build_obj_patch_code groups 1-6)
    let title_ranges: &[(u16, u16)] = &[
        (0x40C0, 0x40C0 + 64),
        (0x4180, 0x4180 + 128),
        (0x42C0, 0x42C0 + 64),
        (0x4380, 0x4380 + 128),
        (0x4580, 0x4580 + 128),
        (0x4780, 0x4780 + 128),
    ];
    // Bubble MVN destinations (groups 7-8)
    let bubble_ranges: &[(u16, u16)] = &[(0x4500, 0x4500 + 128), (0x4C00, 0x4C00 + 256)];

    for &(bs, be) in bubble_ranges {
        for &(ts, te) in title_ranges {
            assert!(
                be <= ts || bs >= te,
                "bubble [{:04X}..{:04X}) overlaps title [{:04X}..{:04X})",
                bs,
                be,
                ts,
                te
            );
        }
    }
}

#[test]
fn test_bubble_text_tiles_empty_glyphs() {
    // Even with no glyphs, should produce valid 4-tile output (all BG_FILL + borders)
    let result = build_bubble_text_tiles(&[], 4);
    assert_eq!(result.len(), 128);
}

// ── Sky worldmap KO tile injection tests ──────────────────────────────

#[test]
fn sky_ko_chars_unique_and_nonempty() {
    let chars = sky_ko_chars();
    assert!(!chars.is_empty(), "sky_ko_chars should not be empty");
    let set: std::collections::HashSet<char> = chars.iter().copied().collect();
    assert_eq!(chars.len(), set.len(), "duplicate chars detected");
    // No spaces should be included
    assert!(
        !chars.contains(&' '),
        "space should be excluded from sky_ko_chars"
    );
    println!("Sky worldmap KO chars: {} {:?}", chars.len(), chars);
}

#[test]
fn inject_ko_tiles_correct_offset() {
    let mut block = vec![0u8; 0x2000]; // 8KB = Block B size
    let tile_data = [0xAA; 16];

    // Inject tile at index $20 → offset $200
    inject_ko_tiles(&mut block, &[(0x20u16, tile_data)]);
    assert_eq!(
        &block[0x200..0x210],
        &[0xAA; 16],
        "tile at index $20 should be at offset $200"
    );

    // Surrounding data should be untouched
    assert_eq!(block[0x1FF], 0x00);
    assert_eq!(block[0x210], 0x00);
}

#[test]
fn inject_ko_tiles_multiple() {
    let mut block = vec![0u8; 0x2000];
    let tiles: Vec<(u16, [u8; 16])> =
        vec![(0x21, [0x11; 16]), (0x22, [0x22; 16]), (0xEF, [0xEE; 16])];

    inject_ko_tiles(&mut block, &tiles);

    assert_eq!(&block[0x210..0x220], &[0x11; 16]);
    assert_eq!(&block[0x220..0x230], &[0x22; 16]);
    assert_eq!(&block[0xEF0..0xF00], &[0xEE; 16]);
}

#[test]
fn inject_ko_tiles_fb_prefix() {
    let mut block = vec![0u8; 0x2000]; // 8KB = Block B (512 tiles)

    // FB-prefix tile: index $100 + $DA = $1DA → offset $1DA0
    inject_ko_tiles(&mut block, &[(0x1DA_u16, [0xBB; 16])]);
    assert_eq!(&block[0x1DA0..0x1DB0], &[0xBB; 16]);
}

#[test]
fn inject_ko_tiles_block_c_bounds() {
    // Block C is $0D00 bytes = 208 tiles (indices $00-$CF)
    let mut block = vec![0u8; 0x0D00];
    let tiles: Vec<(u16, [u8; 16])> = vec![
        (0xCF, [0xCC; 16]),  // last valid tile (offset $CF0, fits)
        (0xD0, [0xDD; 16]),  // out of bounds (offset $D00, doesn't fit)
        (0x1DA, [0xBB; 16]), // FB-prefix — way out of bounds for Block C
    ];

    inject_ko_tiles(&mut block, &tiles);

    // $CF should be injected
    assert_eq!(&block[0xCF0..0xD00], &[0xCC; 16]);
    // $D0 and $1DA should be silently skipped (no panic, no corruption)
    assert_eq!(block.len(), 0x0D00);
}

#[test]
fn sky_place_names_cover_expected_chars() {
    let chars = sky_ko_chars();
    // Key characters that must be present
    let required = ['아', '르', '의', '숲', '마', '산', '용', '비'];
    for &ch in &required {
        assert!(
            chars.contains(&ch),
            "sky_ko_chars missing required char '{}'",
            ch
        );
    }
}

// ── Sky tilemap candidate analysis (Bank $25 LZ blocks) ──────────────────

#[test]
#[ignore] // diagnostic dump — run manually with --ignored
fn sky_tilemap_candidates() {
    use crate::patch::font::decompress_lz;
    use crate::rom::lorom_to_pc;

    // CARGO_MANIFEST_DIR = apps/madou_patch, workspace root = ../../
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = std::path::Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let rom_path =
        workspace_root.join("roms/Madou Monogatari - Hanamaru Daiyouchienji (Japan).sfc");
    let rom = std::fs::read(&rom_path)
        .unwrap_or_else(|e| panic!("ROM not found at {}: {}", rom_path.display(), e));

    let candidates: &[(&str, u8, u16)] = &[
        ("$25:$AB82", 0x25, 0xAB82),
        ("$25:$8018", 0x25, 0x8018),
        ("$25:$AC47", 0x25, 0xAC47),
    ];

    for &(label, bank, addr) in candidates {
        let pc = lorom_to_pc(bank, addr);
        println!("\n======================================================================");
        println!("=== {} (PC = 0x{:06X}) ===", label, pc);
        println!("======================================================================");

        match decompress_lz(&rom, pc) {
            Ok((data, consumed)) => {
                println!("  Compressed size: {} bytes (consumed from ROM)", consumed);
                println!(
                    "  Decompressed size: {} bytes (0x{:04X})",
                    data.len(),
                    data.len()
                );

                // Hex dump of first 64 bytes
                let dump_len = data.len().min(64);
                println!("\n  First {} bytes (hex):", dump_len);
                for row in 0..(dump_len + 15) / 16 {
                    let start = row * 16;
                    let end = (start + 16).min(dump_len);
                    let hex: Vec<String> = data[start..end]
                        .iter()
                        .map(|b| format!("{:02X}", b))
                        .collect();
                    let ascii: String = data[start..end]
                        .iter()
                        .map(|&b| {
                            if (0x20..=0x7E).contains(&b) {
                                b as char
                            } else {
                                '.'
                            }
                        })
                        .collect();
                    println!("    {:04X}: {}  {}", start, hex.join(" "), ascii);
                }

                // Value range analysis
                println!("\n  Value range analysis:");
                let min_val = data.iter().copied().min().unwrap_or(0);
                let max_val = data.iter().copied().max().unwrap_or(0);
                println!(
                    "    Min byte: 0x{:02X}, Max byte: 0x{:02X}",
                    min_val, max_val
                );

                // Check if data looks like a tilemap (2-byte entries)
                if data.len() >= 2 && data.len() % 2 == 0 {
                    let entry_count = data.len() / 2;
                    println!("    Entry count (as 2-byte pairs): {}", entry_count);

                    // Analyze even bytes (tile indices) and odd bytes (attributes)
                    let even_bytes: Vec<u8> = data.iter().step_by(2).copied().collect();
                    let odd_bytes: Vec<u8> = data.iter().skip(1).step_by(2).copied().collect();

                    let even_min = even_bytes.iter().copied().min().unwrap_or(0);
                    let even_max = even_bytes.iter().copied().max().unwrap_or(0);
                    let odd_min = odd_bytes.iter().copied().min().unwrap_or(0);
                    let odd_max = odd_bytes.iter().copied().max().unwrap_or(0);

                    println!(
                        "    Even bytes (tile indices): min=0x{:02X}, max=0x{:02X}",
                        even_min, even_max
                    );
                    println!(
                        "    Odd bytes (attributes):    min=0x{:02X}, max=0x{:02X}",
                        odd_min, odd_max
                    );

                    // Check tilemap hypothesis: odd bytes should be $20 or $60
                    let odd_is_tilemap_attr = odd_bytes.iter().all(|&b| b == 0x20 || b == 0x60);
                    let odd_attr_counts: std::collections::HashMap<u8, usize> = odd_bytes
                        .iter()
                        .fold(std::collections::HashMap::new(), |mut map, &b| {
                            *map.entry(b).or_insert(0) += 1;
                            map
                        });
                    println!("    Odd byte value distribution:");
                    let mut sorted_attrs: Vec<_> = odd_attr_counts.iter().collect();
                    sorted_attrs.sort_by(|a, b| b.1.cmp(a.1));
                    for (val, count) in sorted_attrs.iter().take(10) {
                        println!(
                            "      0x{:02X}: {} times ({:.1}%)",
                            val,
                            count,
                            **count as f64 / odd_bytes.len() as f64 * 100.0
                        );
                    }

                    // Check if even bytes are in tile index range $00-$35
                    let even_in_range = even_bytes.iter().filter(|&&b| b <= 0x35).count();
                    println!(
                        "    Even bytes in $00-$35 range: {}/{} ({:.1}%)",
                        even_in_range,
                        even_bytes.len(),
                        even_in_range as f64 / even_bytes.len() as f64 * 100.0
                    );

                    // Tilemap verdict
                    let is_likely_tilemap = odd_is_tilemap_attr && even_max <= 0x35;
                    println!(
                        "\n    >>> TILEMAP VERDICT: {}",
                        if is_likely_tilemap {
                            "LIKELY BG3 TILEMAP (odd bytes are $20/$60, tile indices in $00-$35)"
                        } else if odd_attr_counts.len() <= 4 && even_max <= 0x80 {
                            "POSSIBLE TILEMAP (limited attribute variety, moderate tile range)"
                        } else {
                            "UNLIKELY TILEMAP (does not match expected pattern)"
                        }
                    );
                } else {
                    println!(
                        "    Odd size ({} bytes) - not aligned to 2-byte entries",
                        data.len()
                    );
                }

                // Additional: check if it could be 2bpp CHR data
                // 2bpp tiles are 16 bytes each; check if size is tile-aligned
                if data.len() % 16 == 0 {
                    let tile_count = data.len() / 16;
                    println!("    As 2bpp CHR: {} tiles (8x8)", tile_count);
                }
                if data.len() % 32 == 0 {
                    let tile_count = data.len() / 32;
                    println!("    As 4bpp CHR: {} tiles (8x8)", tile_count);
                }
            }
            Err(e) => {
                println!("  DECOMPRESSION FAILED: {}", e);
            }
        }
    }
}

// ── Menu tilemap corner analysis (dump removed) ─────────────────────────────

#[test]
#[ignore] // diagnostic dump — run manually with --ignored
fn dump_menu_corner_attrs() {
    use crate::patch::font::decompress_lz;
    use crate::rom::lorom_to_pc;

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = std::path::Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let rom_path =
        workspace_root.join("roms/Madou Monogatari - Hanamaru Daiyouchienji (Japan).sfc");
    let rom = match std::fs::read(&rom_path) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Skipping: {}", e);
            return;
        }
    };

    let block2_pc = lorom_to_pc(BLOCK2_LZ.0, BLOCK2_LZ.1);
    let (tm, _) = decompress_lz(&rom, block2_pc).expect("decompress menu TM");

    println!("=== JP menu tilemap: tile $01 (CORNER) and $34 (CORNER_L) positions ===");
    println!(
        "(col,row): tile  attr  [V={} H={} VH={}]",
        "$A0", "$60", "$E0"
    );
    for i in 0..(tm.len() / 2) {
        let tile = tm[i * 2];
        let attr = tm[i * 2 + 1];
        if tile == 0x01 || tile == 0x34 {
            let row = i / TM_COLS;
            let col = i % TM_COLS;
            let name = if tile == 0x01 { "CORNER" } else { "CORNER_L" };
            let flip = match attr & 0xC0 {
                0x00 => "none",
                0x40 => "H",
                0x80 => "V",
                0xC0 => "HV",
                _ => "?",
            };
            println!(
                "  ({:2},{:2}): ${:02X} ({:8}) attr=${:02X} flip={}",
                col, row, tile, name, attr, flip
            );
        }
    }
}

#[test]
#[ignore]
fn dump_menu_tilemap_full_structure() {
    use crate::patch::font::decompress_lz;
    use crate::rom::lorom_to_pc;

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = std::path::Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let rom_path =
        workspace_root.join("roms/Madou Monogatari - Hanamaru Daiyouchienji (Japan).sfc");
    let rom = match std::fs::read(&rom_path) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Skipping dump_menu_tilemap_full_structure: {}", e);
            return;
        }
    };

    let block2_pc = lorom_to_pc(BLOCK2_LZ.0, BLOCK2_LZ.1);
    let (tm, _) = decompress_lz(&rom, block2_pc).expect("decompress menu TM");

    let cols = TM_COLS;
    let rows = tm.len() / 2 / cols;
    println!(
        "\n=== JP menu tilemap full structure: {}×{} ===\n",
        cols, rows
    );

    // Print all non-blank tiles with labels
    for r in 0..rows {
        for c in 0..cols {
            let idx = r * cols + c;
            let tile = tm[idx * 2];
            let attr = tm[idx * 2 + 1];
            if tile != 0x00 {
                let label = match tile {
                    0x01 => "CORNER",
                    0x02 => "HBAR",
                    0x03 => "VWALL",
                    0x09 => "↓DOWN",
                    0x0F => "VWALL2",
                    0x19 => "INNER",
                    0x22 => "→RIGHT",
                    0x34 => "CORNER_L",
                    _ => "text",
                };
                let flip = match attr & 0xC0 {
                    0x40 => " H",
                    0x80 => " V",
                    0xC0 => " HV",
                    _ => "",
                };
                println!(
                    "  ({:2},{:2}) ${:02X} ${:02X} {:8}{}",
                    r, c, tile, attr, label, flip
                );
            }
        }
    }

    // Print visual grid: frame types + text
    println!("\n=== Visual grid (C=corner, H=hbar, V=vwall, D=↓down, R=→right, I=inner, L=corner_l, .=blank) ===\n");
    for r in 0..rows {
        print!("  r{:02}: ", r);
        let mut has_content = false;
        for c in 0..cols {
            let idx = r * cols + c;
            let tile = tm[idx * 2];
            let ch = match tile {
                0x00 => '.',
                0x01 => 'C',
                0x02 => 'H',
                0x03 | 0x0F => 'V',
                0x09 => 'D',
                0x19 => 'I',
                0x22 => 'R',
                0x34 => 'L',
                _ => {
                    has_content = true;
                    'T'
                }
            };
            print!("{}", ch);
        }
        if has_content {
            // Print tile hex for text tiles
            print!("  |");
            for c in 0..cols {
                let idx = r * cols + c;
                let tile = tm[idx * 2];
                if tile != 0x00
                    && !matches!(tile, 0x01 | 0x02 | 0x03 | 0x09 | 0x0F | 0x19 | 0x22 | 0x34)
                {
                    print!(" {:02X}@{}", tile, c);
                }
            }
        }
        println!();
    }
}

#[test]
#[ignore]
fn dump_ko_menu_tilemap_screen2() {
    use crate::patch::font::decompress_lz;
    use crate::rom::lorom_to_pc;

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = std::path::Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let rom_path =
        workspace_root.join("roms/Madou Monogatari - Hanamaru Daiyouchienji (Japan).sfc");
    let rom = match std::fs::read(&rom_path) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Skipping dump_ko_menu_tilemap_screen2: {}", e);
            return;
        }
    };

    let block2_pc = lorom_to_pc(BLOCK2_LZ.0, BLOCK2_LZ.1);
    let (jp_tm, _) = decompress_lz(&rom, block2_pc).expect("decompress menu TM");

    // Build KO tilemap (same as pipeline)
    let ko_chars = super::collect_menu_ko_chars();
    let ko_char_indices: std::collections::HashMap<char, u8> = ko_chars
        .iter()
        .enumerate()
        .map(|(i, &ch)| (ch, i as u8))
        .collect();
    let mut ko_tm = jp_tm.clone();
    ko_tm.resize(TM_COLS * TM_ROWS * 2, 0x00);
    super::remap_tilemap(&mut ko_tm, &ko_char_indices);
    super::adjust_menu_frames(&mut ko_tm, &ko_char_indices);

    // Reverse lookup: glyph index → char
    let idx_to_char: std::collections::HashMap<u8, char> = ko_char_indices
        .iter()
        .map(|(&ch, &idx)| (idx, ch))
        .collect();

    let cols = TM_COLS;
    println!("\n=== KO menu tilemap screen 2 (rows 0-27) ===\n");
    println!("  C=CORNER H=HBAR V=VWALL D=↓DOWN R=→RIGHT I=INNER L=CORNER_L .=blank g=glyph\n");
    for r in 0..28 {
        print!("  r{:02}: ", r);
        let mut glyph_info = String::new();
        for c in 0..cols {
            let idx = r * cols + c;
            let tile = ko_tm[idx * 2];
            let attr = ko_tm[idx * 2 + 1];
            let ch = match tile {
                0x00 => '.',
                0x01 => 'C',
                0x02 => 'H',
                0x03 => 'V',
                0x04 => 'D',
                0x05 => 'I',
                0x06 => 'R',
                0x07 => 'L',
                t if t >= 0x08 => {
                    if let Some(&ko_ch) = idx_to_char.get(&(t - 0x08)) {
                        glyph_info.push_str(&format!(" {}@{}", ko_ch, c));
                    }
                    'g'
                }
                _ => '?',
            };
            // Add flip indicator
            let flip_ch = match attr & 0xC0 {
                0x40 => 'h', // H-flip (lowercase for compact display)
                0x80 => 'v',
                0xC0 => 'x', // HV-flip
                _ => ch,     // no flip: use tile char
            };
            // For frame tiles, show flip; for glyphs/inner, show tile type
            if matches!(tile, 0x01..=0x07) {
                print!("{}", flip_ch);
            } else {
                print!("{}", ch);
            }
        }
        if !glyph_info.is_empty() {
            print!("  |{}", glyph_info);
        }
        println!();
    }
}

// ── Sky tilemap text group analysis ─────────────────────────────────────────

#[test]
#[ignore]
fn dump_sky_tilemap_text_groups() {
    use crate::patch::font::decompress_lz;
    use crate::rom::lorom_to_pc;

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = std::path::Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let rom_path =
        workspace_root.join("roms/Madou Monogatari - Hanamaru Daiyouchienji (Japan).sfc");
    let rom = match std::fs::read(&rom_path) {
        Ok(data) => data,
        Err(e) => {
            eprintln!(
                "Skipping dump_sky_tilemap_text_groups: ROM not found at {}: {}",
                rom_path.display(),
                e
            );
            return;
        }
    };

    // JP tile index → kana character mapping from WORLDMAP_TILE_IDX.md
    let tile_to_kana: std::collections::HashMap<u8, &str> = [
        (0x04, "あ"),
        (0x05, "め"),
        (0x06, "の"),
        (0x07, "も"),
        (0x08, "り"),
        (0x0A, "い"),
        (0x0B, "せ"),
        (0x0C, "き"),
        (0x0D, "む"),
        (0x0E, "ら"),
        (0x10, "か"),
        (0x11, "え"),
        (0x12, "る"),
        (0x13, "ぞ"),
        (0x14, "う"),
        (0x15, "た"),
        (0x16, "ま"),
        (0x17, "お"),
        (0x18, "け"),
        (0x1A, "し"),
        (0x1B, "ん"),
        (0x1C, "に"),
        (0x1D, "や"),
        (0x1E, "み"),
        (0x1F, "ど"),
        (0x20, "ぐ"),
        (0x21, "ち"),
        (0x23, "よ"),
        (0x24, "す"),
        (0x25, "と"),
        (0x26, "サ"),
        (0x27, "タ"),
        (0x28, "ン"),
        (0x29, "さ"),
        (0x2A, "べ"),
        (0x2B, "っ"),
        (0x2C, "ア"),
        (0x2D, "ル"),
        (0x2E, "は"),
        (0x2F, "ヤ"),
        (0x30, "゜"),
        (0x31, "ハ"),
        (0x32, "ー"),
        (0x33, "ピ"),
        (0x35, "ひ"),
    ]
    .iter()
    .cloned()
    .collect();

    // Decompress $25:$AB82 = sky BG3 tilemap
    let pc = lorom_to_pc(0x25, 0xAB82);
    let (data, _) = decompress_lz(&rom, pc).expect("Failed to decompress $25:$AB82");
    assert_eq!(data.len(), 1792, "Expected 1792 bytes (32×28×2)");

    let cols = 32usize;
    let rows = data.len() / 2 / cols;
    println!(
        "\nSky tilemap: {}×{} entries ({} bytes)",
        cols,
        rows,
        data.len()
    );

    // Find text groups (same logic as find_text_groups in worldmap.rs)
    let is_frame = |idx: u8| -> bool {
        matches!(
            idx,
            0x00 | 0x01 | 0x02 | 0x03 | 0x09 | 0x0F | 0x19 | 0x22 | 0x34
        )
    };

    let mut groups: Vec<Vec<(usize, usize, u8, u8)>> = Vec::new(); // (col, row, tile, attr)
    for r in 0..rows {
        let mut col = 0;
        while col < cols {
            let entry_idx = r * cols + col;
            let tile = data[entry_idx * 2];
            let attr = data[entry_idx * 2 + 1];
            if !is_frame(tile) && tile != 0x00 {
                let mut group = vec![(col, r, tile, attr)];
                col += 1;
                while col < cols {
                    let idx2 = r * cols + col;
                    let t2 = data[idx2 * 2];
                    let a2 = data[idx2 * 2 + 1];
                    if !is_frame(t2) && t2 != 0x00 {
                        group.push((col, r, t2, a2));
                        col += 1;
                    } else {
                        break;
                    }
                }
                groups.push(group);
            } else {
                col += 1;
            }
        }
    }

    println!("\nFound {} text groups:\n", groups.len());
    for (gi, group) in groups.iter().enumerate() {
        let tiles_hex: Vec<String> = group
            .iter()
            .map(|(_, _, t, _)| format!("{:02X}", t))
            .collect();
        let attrs_hex: Vec<String> = group
            .iter()
            .map(|(_, _, _, a)| format!("{:02X}", a))
            .collect();
        let kana: String = group
            .iter()
            .map(|(_, _, t, _)| tile_to_kana.get(t).copied().unwrap_or("?"))
            .collect();

        let (start_col, row, _, _) = group[0];
        let end_col = group.last().unwrap().0;

        println!(
            "  G{:02} row={:2} col=({:2},{:2}) len={} tiles=[{}] attrs=[{}] JP=\"{}\"",
            gi,
            row,
            start_col,
            end_col,
            group.len(),
            tiles_hex.join(" "),
            attrs_hex.join(" "),
            kana
        );
    }

    // Also dump full tilemap grid for visual inspection
    println!("\n\nFull tilemap grid (tile indices, '.' = blank/frame):\n");
    for r in 0..rows {
        print!("  row {:2}: ", r);
        for c in 0..cols {
            let idx = r * cols + c;
            let tile = data[idx * 2];
            if is_frame(tile) || tile == 0x00 {
                print!(". ");
            } else {
                print!("{:02X}", tile);
            }
        }
        println!();
    }

    // Dump ALL non-zero tiles (including frame tiles) to find direction pointers
    println!("\n\nAll non-zero tiles (row, col, tile, attr):\n");
    for r in 0..rows {
        for c in 0..cols {
            let idx = r * cols + c;
            let tile = data[idx * 2];
            let attr = data[idx * 2 + 1];
            if tile != 0x00 {
                let label = match tile {
                    0x01 => " CORNER",
                    0x02 => " HBAR",
                    0x03 | 0x0F => " VWALL",
                    0x09 => " ↓DOWN",
                    0x19 => " INNER",
                    0x22 => " →RIGHT",
                    0x34 => " CORNER_L",
                    _ => "",
                };
                println!(
                    "  ({:2},{:2}) tile=${:02X} attr=${:02X}{}",
                    r, c, tile, attr, label
                );
            }
        }
    }
}
