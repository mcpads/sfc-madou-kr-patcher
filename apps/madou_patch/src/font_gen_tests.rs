use super::*;

// -- 2bpp conversion tests ---

#[test]
fn bitmap_to_2bpp_empty() {
    let bitmap = [false; 256];
    let tile = bitmap_to_snes_2bpp_16x16(&bitmap);
    assert!(tile.iter().all(|&b| b == 0));
}

#[test]
fn bitmap_to_2bpp_top_left_pixel() {
    let mut bitmap = [false; 256];
    bitmap[0] = true; // row 0, col 0
    let tile = bitmap_to_snes_2bpp_16x16(&bitmap);
    // TL quadrant, row 0: bit 7 set
    assert_eq!(tile[0], 0x80); // BP0
    assert_eq!(tile[1], 0x80); // BP1
                               // Rest of TL quadrant rows should be 0
    for r in 1..8 {
        assert_eq!(tile[r * 2], 0);
        assert_eq!(tile[r * 2 + 1], 0);
    }
}

#[test]
fn bitmap_to_2bpp_bottom_right_pixel() {
    let mut bitmap = [false; 256];
    bitmap[15 * 16 + 15] = true; // row 15, col 15
    let tile = bitmap_to_snes_2bpp_16x16(&bitmap);
    // BR quadrant (index 3), row 7, bit 0
    let base = 3 * 16;
    assert_eq!(tile[base + 7 * 2], 0x01);
    assert_eq!(tile[base + 7 * 2 + 1], 0x01);
}

#[test]
fn bitmap_to_2bpp_full_first_row() {
    let mut bitmap = [false; 256];
    for c in 0..16 {
        bitmap[c] = true;
    }
    let tile = bitmap_to_snes_2bpp_16x16(&bitmap);
    // TL row 0: all 8 bits
    assert_eq!(tile[0], 0xFF);
    assert_eq!(tile[1], 0xFF);
    // TR row 0: all 8 bits
    assert_eq!(tile[16], 0xFF);
    assert_eq!(tile[17], 0xFF);
}

#[test]
fn bitmap_to_2bpp_quadrant_independence() {
    // Set pixel at (4, 12) -> TR quadrant, row 4, col 4
    let mut bitmap = [false; 256];
    bitmap[4 * 16 + 12] = true;
    let tile = bitmap_to_snes_2bpp_16x16(&bitmap);
    // TR quadrant (index 1), row 4, bit (7-4)=3 -> 0x08
    let base = 1 * 16;
    assert_eq!(tile[base + 4 * 2], 0x08);
    assert_eq!(tile[base + 4 * 2 + 1], 0x08);
    // TL and BL/BR should be all zeros
    assert!(tile[0..16].iter().all(|&b| b == 0));
    assert!(tile[32..64].iter().all(|&b| b == 0));
}

// -- 4bpp 8x8 conversion tests ---

#[test]
fn bitmap_4bpp_8x8_empty() {
    let bitmap = [false; 64];
    let tile = bitmap_to_snes_4bpp_8x8(&bitmap, 0x0F, 0);
    assert!(tile.iter().all(|&b| b == 0));
}

#[test]
fn bitmap_4bpp_8x8_single_pixel_all_planes() {
    let mut bitmap = [false; 64];
    bitmap[0] = true; // row 0, col 0
    let tile = bitmap_to_snes_4bpp_8x8(&bitmap, 0x0F, 0); // fg=15 → all 4 bitplanes
                                                          // bit 7 set in all bitplanes
    assert_eq!(tile[0], 0x80); // BP0
    assert_eq!(tile[1], 0x80); // BP1
    assert_eq!(tile[16], 0x80); // BP2
    assert_eq!(tile[17], 0x80); // BP3
                                // rest should be zero
    for r in 1..8 {
        assert_eq!(tile[r * 2], 0);
        assert_eq!(tile[r * 2 + 1], 0);
        assert_eq!(tile[16 + r * 2], 0);
        assert_eq!(tile[16 + r * 2 + 1], 0);
    }
}

#[test]
fn bitmap_4bpp_8x8_fg_color_selective() {
    let mut bitmap = [false; 64];
    bitmap[0] = true; // row 0, col 0
                      // fg_color=5 (0b0101) → BP0=1, BP1=0, BP2=1, BP3=0
    let tile = bitmap_to_snes_4bpp_8x8(&bitmap, 5, 0);
    assert_eq!(tile[0], 0x80); // BP0 set
    assert_eq!(tile[1], 0x00); // BP1 clear
    assert_eq!(tile[16], 0x80); // BP2 set
    assert_eq!(tile[17], 0x00); // BP3 clear
}

#[test]
fn bitmap_4bpp_8x8_full_row() {
    let mut bitmap = [false; 64];
    for c in 0..8 {
        bitmap[c] = true;
    }
    let tile = bitmap_to_snes_4bpp_8x8(&bitmap, 0x0F, 0);
    assert_eq!(tile[0], 0xFF); // BP0 row 0 all set
    assert_eq!(tile[1], 0xFF); // BP1
    assert_eq!(tile[16], 0xFF); // BP2
    assert_eq!(tile[17], 0xFF); // BP3
}

#[test]
fn bitmap_4bpp_8x8_bottom_right_pixel() {
    let mut bitmap = [false; 64];
    bitmap[7 * 8 + 7] = true; // row 7, col 7
    let tile = bitmap_to_snes_4bpp_8x8(&bitmap, 0x0F, 0);
    assert_eq!(tile[7 * 2], 0x01); // BP0 row 7, bit 0
    assert_eq!(tile[7 * 2 + 1], 0x01); // BP1
    assert_eq!(tile[16 + 7 * 2], 0x01); // BP2
    assert_eq!(tile[16 + 7 * 2 + 1], 0x01); // BP3
}

#[test]
fn bitmap_4bpp_8x8_size() {
    let bitmap = [false; 64];
    let tile = bitmap_to_snes_4bpp_8x8(&bitmap, 1, 0);
    assert_eq!(tile.len(), 32); // 4bpp 8x8 = 32 bytes
}

#[test]
fn bitmap_4bpp_8x8_matches_16x16_quadrant() {
    // A pixel at (r,c) in the 8x8 should match the TL quadrant of a 16x16
    // with the same pixel at (r,c)
    let mut bitmap_8 = [false; 64];
    let mut bitmap_16 = [false; 256];
    bitmap_8[3 * 8 + 5] = true;
    bitmap_16[3 * 16 + 5] = true;
    let tile_8 = bitmap_to_snes_4bpp_8x8(&bitmap_8, 0x0A, 0);
    let tile_16 = bitmap_to_snes_4bpp_16x16(&bitmap_16, 0x0A);
    // TL quadrant of 16x16 should match the 8x8
    assert_eq!(tile_8, tile_16[0]);
}

// -- Encoding table tests ---

#[test]
fn encoding_single_byte_range() {
    let chars: Vec<char> = (0..208)
        .map(|i| char::from_u32(0xAC00 + i).unwrap())
        .collect();
    let table = build_encoding_table(&chars);
    assert_eq!(table[&chars[0]], vec![0x20]);
    assert_eq!(table[&chars[207]], vec![0xEF]);
}

#[test]
fn encoding_fb_range_basic() {
    let chars: Vec<char> = (0..300)
        .map(|i| char::from_u32(0xAC00 + i).unwrap())
        .collect();
    let table = build_encoding_table(&chars);
    assert_eq!(table[&chars[208]], vec![0xFB, 0x00]);
    assert_eq!(table[&chars[209]], vec![0xFB, 0x01]);
}

#[test]
fn encoding_fb_blank_remap() {
    // 789 chars → charset_f0_count = 789 - 720 = 69
    // FB blank remap indices start at 69: $E4→$45, $F4→$46, ..., $FF→$50
    let chars: Vec<char> = (0..789)
        .map(|i| char::from_u32(0xAC00 + i).unwrap())
        .collect();
    let table = build_encoding_table(&chars);

    // FB $E4 -> F0 $45 (69+0)
    assert_eq!(table[&chars[208 + 0xE4]], vec![0xF0, 0x45]);
    // FB $E5 -> stays FB $E5
    assert_eq!(table[&chars[208 + 0xE5]], vec![0xFB, 0xE5]);
    // FB $F4 -> F0 $46 (69+1)
    assert_eq!(table[&chars[208 + 0xF4]], vec![0xF0, 0x46]);
    // FB $F6 -> F0 $47 (69+2)
    assert_eq!(table[&chars[208 + 0xF6]], vec![0xF0, 0x47]);
    // FB $FF -> F0 $50 (69+11)
    assert_eq!(table[&chars[208 + 0xFF]], vec![0xF0, 0x50]);
}

#[test]
fn encoding_f1_range() {
    let chars: Vec<char> = (0..465)
        .map(|i| char::from_u32(0xAC00 + i).unwrap())
        .collect();
    let table = build_encoding_table(&chars);
    assert_eq!(table[&chars[464]], vec![0xF1, 0x00]);
}

#[test]
fn encoding_f0_range() {
    let chars: Vec<char> = (0..721)
        .map(|i| char::from_u32(0xAC00 + i).unwrap())
        .collect();
    let table = build_encoding_table(&chars);
    assert_eq!(table[&chars[720]], vec![0xF0, 0x00]);
}

#[test]
fn encoding_full_789_chars() {
    // Simulate the actual charset size
    let chars: Vec<char> = (0..789)
        .map(|i| char::from_u32(0xAC00 + i).unwrap())
        .collect();
    let table = build_encoding_table(&chars);

    // Counts by prefix
    let single_count = chars.iter().filter(|c| table[c].len() == 1).count();
    let fb_count = chars
        .iter()
        .filter(|c| table[c].len() == 2 && table[c][0] == 0xFB)
        .count();
    let f1_count = chars
        .iter()
        .filter(|c| table[c].len() == 2 && table[c][0] == 0xF1)
        .count();
    let f0_count = chars
        .iter()
        .filter(|c| table[c].len() == 2 && table[c][0] == 0xF0)
        .count();

    assert_eq!(single_count, 208);
    assert_eq!(fb_count, 244); // 256 - 12 blanks
    assert_eq!(f1_count, 256);
    assert_eq!(f0_count, 81); // 69 regular + 12 remap

    // Last char -> F0 $44 (index 788 = 720 + 68)
    assert_eq!(table[&chars[788]], vec![0xF0, 0x44]);
}

// -- Fixed tiles tests ---

#[test]
fn fixed_chars_count() {
    assert_eq!(FIXED_CHARS.len(), 32);
}

#[test]
fn fb_blank_slots_matches_font_rs() {
    // Must match FB_BLANK_SLOTS in patch/font.rs (SSOT)
    assert_eq!(FB_BLANK_SLOTS.len(), 12);
    assert_eq!(FB_BLANK_SLOTS[0], 0xE4);
    assert_eq!(FB_BLANK_SLOTS[11], 0xFF);
}

// -- Charset loading tests ---

#[test]
fn load_charset_basic() {
    let dir = std::env::temp_dir().join("madou_test_font_gen_charset");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test_charset.txt");
    std::fs::write(
        &path,
        "# header comment\n이\t1046\tU+C774\n아\t1007\tU+C544\n",
    )
    .unwrap();

    let chars = load_charset(&path).unwrap();
    assert_eq!(chars, vec!['이', '아']);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn load_charset_skips_blanks_and_comments() {
    let dir = std::env::temp_dir().join("madou_test_font_gen_blanks");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("blanks.txt");
    std::fs::write(&path, "# comment\n\n이\t100\n\n# another\n아\t200\n").unwrap();

    let chars = load_charset(&path).unwrap();
    assert_eq!(chars, vec!['이', '아']);

    std::fs::remove_dir_all(&dir).ok();
}

// -- TSV writing tests ---

#[test]
fn write_encoding_tsv_format() {
    let mut encoding = HashMap::new();
    encoding.insert('이', vec![0x20]);
    encoding.insert('아', vec![0xFB, 0x00]);
    let chars = vec!['이', '아'];

    let dir = std::env::temp_dir().join("madou_test_font_gen_tsv");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test_encoding.tsv");

    write_encoding_tsv(&path, &encoding, &chars).unwrap();

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.starts_with("CHAR\tUNICODE\tBYTES\tTILE_INDEX\n"));
    assert!(content.contains("이\tU+C774\t20\t0"));
    assert!(content.contains("아\tU+C544\tFB 00\t1"));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn write_encoding_tsv_roundtrip() {
    // Generate encoding, write TSV, load with ko::load_ko_encoding
    let chars: Vec<char> = vec!['가', '나', '다'];
    let encoding = build_encoding_table(&chars);

    let dir = std::env::temp_dir().join("madou_test_font_gen_roundtrip");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("roundtrip.tsv");

    write_encoding_tsv(&path, &encoding, &chars).unwrap();

    let loaded = crate::encoding::ko::load_ko_encoding(&path).unwrap();
    for &ch in &chars {
        assert_eq!(loaded[&ch], encoding[&ch], "Mismatch for '{}'", ch);
    }

    std::fs::remove_dir_all(&dir).ok();
}

// -- Outline tile tests (Color 3 fill + Color 2 outline) ---

#[test]
fn outline_empty_bitmap() {
    let bitmap = [false; 256];
    let tile = bitmap_to_snes_2bpp_16x16_outline(&bitmap);
    assert!(tile.iter().all(|&b| b == 0));
}

#[test]
fn outline_single_pixel_produces_fill_and_outline() {
    let mut bitmap = [false; 256];
    bitmap[0] = true; // row 0, col 0 → TL quadrant
    let tile = bitmap_to_snes_2bpp_16x16_outline(&bitmap);
    // (0,0): Color 3 (fill) → BP0=1, BP1=1 → bit 7
    // (0,1): Color 2 (outline) → BP0=0, BP1=1 → bit 6
    // TL row 0: BP0 = 0x80, BP1 = 0xC0
    assert_eq!(tile[0], 0x80); // BP0: bit 7 (fill only)
    assert_eq!(tile[1], 0xC0); // BP1: bit 7 (fill) + bit 6 (outline)
                               // TL row 1: dilation reaches (1,0) and (1,1) as outline
                               // (1,0): outline → BP0=0, BP1=1 → bit 7
                               // (1,1): outline → BP0=0, BP1=1 → bit 6
    assert_eq!(tile[2], 0x00); // BP0: no fill pixels
    assert_eq!(tile[3], 0xC0); // BP1: outline at bits 7+6
}

#[test]
fn outline_full_row_color3_fill() {
    // Full row 0 (16 pixels) → all Color 3 (fill)
    let mut bitmap = [false; 256];
    for c in 0..16 {
        bitmap[c] = true;
    }
    let tile = bitmap_to_snes_2bpp_16x16_outline(&bitmap);
    // TL row 0: all 8 pixels are fill → BP0=0xFF, BP1=0xFF (Color 3)
    assert_eq!(tile[0], 0xFF); // BP0
    assert_eq!(tile[1], 0xFF); // BP1
                               // TR row 0: same
    assert_eq!(tile[16], 0xFF);
    assert_eq!(tile[17], 0xFF);
    // TL row 1: dilation outline → BP0=0x00, BP1=0xFF (Color 2)
    assert_eq!(tile[2], 0x00);
    assert_eq!(tile[3], 0xFF);
}

#[test]
fn outline_vs_monochrome() {
    // Outline uses Color 3 for fill (same as monochrome) + Color 2 for outline (extra)
    let mut bitmap = [false; 256];
    bitmap[4 * 16 + 4] = true; // center-ish pixel at (4,4) in TL quadrant
    let outline = bitmap_to_snes_2bpp_16x16_outline(&bitmap);
    let mono = bitmap_to_snes_2bpp_16x16(&bitmap);
    // At the fill pixel, both should have BP0=BP1=bit set (Color 3)
    // TL row 4: pixel at col 4 → bit (7-4)=3 → 0x08
    assert_eq!(mono[8], 0x08); // BP0
    assert_eq!(mono[9], 0x08); // BP1
    assert_eq!(outline[8] & 0x08, 0x08); // BP0 has fill bit
    assert_eq!(outline[9] & 0x08, 0x08); // BP1 has fill bit
                                         // Outline also has neighboring pixels in BP1 (Color 2)
                                         // Col 3 and 5 should be outline → bits (7-3)=4 and (7-5)=2 → 0x14
    assert_eq!(outline[8] & 0x14, 0x00); // BP0: no outline bits
    assert_eq!(outline[9] & 0x14, 0x14); // BP1: outline at cols 3,5
}
