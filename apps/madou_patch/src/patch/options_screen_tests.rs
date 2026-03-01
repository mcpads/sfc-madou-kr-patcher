use super::*;

// ── Helper ──────────────────────────────────────────────────────────

/// Create a test tile with BP0=`bp0_val`, BP1=$FF for all 8 rows.
fn make_test_tile(bp0_val: u8) -> [u8; TILE_SIZE] {
    let mut tile = [0u8; TILE_SIZE];
    for r in 0..8 {
        tile[r * 2] = bp0_val;
        tile[r * 2 + 1] = 0xFF;
    }
    tile
}

// ── Char collection tests ───────────────────────────────────────────

#[test]
fn ko_chars_8x8_unique_and_bounded() {
    let chars = collect_ko_chars_8x8();
    assert!(!chars.is_empty());
    assert!(
        chars.len() == chars.iter().collect::<std::collections::HashSet<_>>().len(),
        "duplicate 8x8 chars"
    );
    assert!(chars.len() <= 80, "too many 8x8 chars: {}", chars.len());
    println!("8x8 KO chars: {} {:?}", chars.len(), chars);
}

#[test]
fn ko_chars_16x16_unique_and_bounded() {
    let chars = collect_ko_chars_16x16();
    assert!(!chars.is_empty());
    assert!(
        chars.len() == chars.iter().collect::<std::collections::HashSet<_>>().len(),
        "duplicate 16x16 chars"
    );
    assert!(chars.len() <= 30, "too many 16x16 chars: {}", chars.len());
    println!("16x16 KO chars: {} {:?}", chars.len(), chars);
}

#[test]
fn opt_ko_chars_8x8_unique_and_bounded() {
    let chars = collect_ko_chars_8x8_options();
    assert!(!chars.is_empty());
    assert!(
        chars.len() == chars.iter().collect::<std::collections::HashSet<_>>().len(),
        "duplicate 8x8 options chars"
    );
    assert_eq!(chars.len(), OPT_8X8_CHARS.len());
    assert!(
        chars.len() <= 20,
        "too many 8x8 options chars: {}",
        chars.len()
    );
    println!("Options 8x8 KO chars: {} {:?}", chars.len(), chars);
}

#[test]
fn opt_ko_chars_16x16_unique_and_bounded() {
    let chars = collect_ko_chars_16x16_options();
    assert!(!chars.is_empty());
    assert!(
        chars.len() == chars.iter().collect::<std::collections::HashSet<_>>().len(),
        "duplicate 16x16 options chars"
    );
    assert_eq!(chars.len(), OPT_16X16_CHARS.len());
    assert!(
        chars.len() <= 20,
        "too many 16x16 options chars: {}",
        chars.len()
    );
    println!("Options 16x16 KO chars: {} {:?}", chars.len(), chars);
}

// ── Hook site and constant tests ────────────────────────────────────

#[test]
fn hook_site_correct() {
    assert_eq!(CHR_HOOK_PC, lorom_to_pc(0x01, 0x820A));
}

#[test]
fn opt_hook_site_correct() {
    assert_eq!(OPT_CHR_HOOK_PC, lorom_to_pc(0x01, 0x8F46));
}

#[test]
fn opt_tm5_hook_site_correct() {
    assert_eq!(OPT_TM5_HOOK_PC, lorom_to_pc(0x01, 0x8F83));
}

#[test]
fn lz_ptr_table_pc_correct() {
    assert_eq!(LZ_PTR_TABLE_PC, lorom_to_pc(0x08, 0x8000));
}

#[test]
fn overlay_constants_consistent() {
    assert_eq!(OVERLAY_TILES, 145);
    assert_eq!(OVERLAY_SIZE, 145 * 16);
    assert_eq!(OVERLAY_SIZE_4BPP, 145 * 32);
    assert_eq!(WRAM_OVERLAY_OFFSET, 0xDA80);
    assert!(OVERLAY_FIRST >= TILE_BASE);
    assert!(OVERLAY_LAST >= OVERLAY_FIRST);
}

#[test]
fn opt_overlay_size() {
    assert_eq!(OPT_CHR_TILES, 128);
    assert_eq!(OPT_OVERLAY_SIZE_4BPP, 128 * 32);
    assert_eq!(OPT_OVERLAY_SIZE_4BPP, 4096);
    assert_eq!(OPT_WRAM_DEST, 0xD000);
}

#[test]
fn wram_overlay_offset_calculation() {
    let base = (OVERLAY_FIRST - TILE_BASE) as usize;
    let expected = 0xD000u16 + (base * 2 * TILE_SIZE) as u16;
    assert_eq!(WRAM_OVERLAY_OFFSET, expected);
    let end = WRAM_OVERLAY_OFFSET as usize + OVERLAY_SIZE_4BPP;
    assert!(
        end <= 0x10000,
        "Overlay end ${:X} exceeds WRAM DMA range",
        end
    );
}

#[test]
fn hook_code_size_stable() {
    let hook = build_chr_hook(0, 0, 1, 0).unwrap();
    assert_eq!(hook.len(), HOOK_CODE_SIZE, "HOOK_CODE_SIZE mismatch");
}

// ── Hook assembly tests ─────────────────────────────────────────────

#[test]
fn chr_hook_assembles() {
    let hook = build_chr_hook(DATA_BANK, 0xD830, 0x1220, WRAM_OVERLAY_OFFSET).unwrap();
    assert_eq!(
        &hook[0..4],
        &[0x22, 0x40, 0x94, 0x00],
        "must start with JSL $009440"
    );
    assert_eq!(hook[hook.len() - 1], 0x6B, "must end with RTL");
    assert!(
        hook.windows(3).any(|w| w == [0x54, 0x7F, DATA_BANK]),
        "hook must contain MVN $7F, ${:02X}",
        DATA_BANK
    );
    assert!(hook.contains(&0x8B), "hook must contain PHB");
    assert!(hook.contains(&0xAB), "hook must contain PLB");
    assert!(
        hook.windows(2).any(|w| w == [0xC2, 0x30]),
        "hook must contain REP #$30"
    );
}

#[test]
fn opt_chr_hook_assembles() {
    let hook = build_chr_hook(
        DATA_BANK,
        0xEA60,
        OPT_OVERLAY_SIZE_4BPP as u16,
        OPT_WRAM_DEST,
    )
    .unwrap();
    assert_eq!(&hook[0..4], &[0x22, 0x40, 0x94, 0x00]);
    assert_eq!(hook[hook.len() - 1], 0x6B);
    assert!(hook.windows(3).any(|w| w == [0x54, 0x7F, DATA_BANK]));
    assert!(
        hook.windows(3).any(|w| w == [0xA0, 0x00, 0xD0]),
        "LDY #$D000"
    );
}

#[test]
fn build_chr_hook_rejects_zero_size() {
    let err = build_chr_hook(DATA_BANK, 0xD830, 0, WRAM_OVERLAY_OFFSET);
    assert!(err.is_err(), "overlay_size=0 should be rejected");
}

#[test]
fn tm5_hook_assembles() {
    let remap_values = vec![
        (0xC8F0, 0x3200u16),
        (0xC9D0, 0x327D),
        (0xC9D2, 0x327E),
        (0xC908, 0x327F),
    ];
    let hook = build_tm_remap_hook(&remap_values).unwrap();
    // Must start with JSL $009440
    assert_eq!(&hook[0..4], &[0x22, 0x40, 0x94, 0x00]);
    // Must end with RTL
    assert_eq!(hook[hook.len() - 1], 0x6B);
    // Must contain PHB/PLB pair
    assert!(hook.contains(&0x8B), "must contain PHB");
    assert!(hook.contains(&0xAB), "must contain PLB");
    // Must contain SEP #$20 and REP #$20
    assert!(
        hook.windows(2).any(|w| w == [0xE2, 0x20]),
        "must contain SEP #$20"
    );
    assert!(
        hook.windows(2).any(|w| w == [0xC2, 0x20]),
        "must contain REP #$20"
    );
    // Must contain LDA #$7F for data bank setup
    assert!(
        hook.windows(2).any(|w| w == [0xA9, 0x7F]),
        "must contain LDA #$7F"
    );
    // Must contain 4 STA abs instructions (8D xx xx)
    let sta_count = hook
        .windows(1)
        .enumerate()
        .filter(|(i, _)| *i > 4 && hook[*i] == 0x8D)
        .count();
    assert!(
        sta_count >= 4,
        "must contain at least 4 STA abs: found {}",
        sta_count
    );
    println!("TM5 hook: {} bytes", hook.len());
}

// ── Blank tile format ───────────────────────────────────────────────

#[test]
fn blank_tile_format() {
    let blank = BLANK_TILE;
    for r in 0..8 {
        assert_eq!(blank[r * 2], 0x00, "BP0 should be 0");
        assert_eq!(blank[r * 2 + 1], 0xFF, "BP1 should be $FF");
    }
}

// ── Region validation ───────────────────────────────────────────────

#[test]
fn all_regions_have_valid_ko_text() {
    for (name, regions) in [("STAT", STAT_REGIONS), ("MAGIC", MAGIC_REGIONS)] {
        for region in regions {
            let ko_len = region.ko.chars().count();
            let tile_slots = match region.size {
                TileSize::S8x8 => region.col_end - region.col_start,
                TileSize::S16x16 => (region.col_end - region.col_start) / 2,
            };
            assert!(
                ko_len <= tile_slots,
                "{}: '{}' has {} KO chars but only {} slots",
                name,
                region.ko,
                ko_len,
                tile_slots
            );
        }
    }
}

// ── Direct tile mapping validation ──────────────────────────────────

#[test]
fn opt_tile_map_indices_in_range() {
    for &(tile_idx, _) in OPT_TILE_MAP {
        let rel = tile_idx
            .checked_sub(TILE_BASE)
            .unwrap_or_else(|| panic!("tile ${:03X} < TILE_BASE ${:03X}", tile_idx, TILE_BASE))
            as usize;
        assert!(
            rel < OPT_CHR_TILES,
            "tile ${:03X} (rel {}) >= CHR count {}",
            tile_idx,
            rel,
            OPT_CHR_TILES
        );
    }
}

#[test]
fn opt_tile_map_no_duplicate_tiles() {
    let mut seen = std::collections::HashSet::new();
    for &(tile_idx, _) in OPT_TILE_MAP {
        assert!(
            seen.insert(tile_idx),
            "duplicate tile ${:03X} in OPT_TILE_MAP",
            tile_idx
        );
    }
}

#[test]
fn opt_tile_map_char_indices_valid() {
    for &(tile_idx, ref content) in OPT_TILE_MAP {
        match content {
            OptTile::Quad { char_idx, quadrant } => {
                assert!(
                    *char_idx < OPT_16X16_CHARS.len(),
                    "tile ${:03X}: 16x16 char_idx {} >= {}",
                    tile_idx,
                    char_idx,
                    OPT_16X16_CHARS.len()
                );
                assert!(
                    *quadrant < 4,
                    "tile ${:03X}: quadrant {} >= 4",
                    tile_idx,
                    quadrant
                );
            }
            OptTile::Glyph { char_idx } => {
                assert!(
                    *char_idx < OPT_8X8_CHARS.len(),
                    "tile ${:03X}: 8x8 char_idx {} >= {}",
                    tile_idx,
                    char_idx,
                    OPT_8X8_CHARS.len()
                );
            }
            OptTile::Blank => {}
        }
    }
}

#[test]
fn opt_tile_map_all_16x16_chars_used() {
    let mut used = vec![false; OPT_16X16_CHARS.len()];
    for &(_, ref content) in OPT_TILE_MAP {
        if let OptTile::Quad { char_idx, .. } = content {
            used[*char_idx] = true;
        }
    }
    for (i, &is_used) in used.iter().enumerate() {
        assert!(
            is_used,
            "16x16 char '{}' (idx {}) not used in OPT_TILE_MAP",
            OPT_16X16_CHARS[i], i
        );
    }
}

#[test]
fn opt_tile_map_all_8x8_chars_used() {
    let mut used = vec![false; OPT_8X8_CHARS.len()];
    for &(_, ref content) in OPT_TILE_MAP {
        if let OptTile::Glyph { char_idx } = content {
            used[*char_idx] = true;
        }
    }
    for (i, &is_used) in used.iter().enumerate() {
        assert!(
            is_used,
            "8x8 char '{}' (idx {}) not used in OPT_TILE_MAP",
            OPT_8X8_CHARS[i], i
        );
    }
}

#[test]
fn opt_tile_map_16x16_quads_complete() {
    // Each 16x16 char must have all 4 quads (TL, TR, BL, BR)
    let mut quads: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    for &(_, ref content) in OPT_TILE_MAP {
        if let OptTile::Quad { char_idx, quadrant } = content {
            quads.entry(*char_idx).or_default().push(*quadrant);
        }
    }
    for (ci, qs) in &quads {
        let mut sorted = qs.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted,
            vec![0, 1, 2, 3],
            "16x16 char '{}' (idx {}): expected quads [0,1,2,3], got {:?}",
            OPT_16X16_CHARS[*ci],
            ci,
            sorted
        );
    }
}

// ── TM6 remap validation ───────────────────────────────────────────

#[test]
fn tm6_hook_site_correct() {
    assert_eq!(TM6_HOOK_PC, lorom_to_pc(0x01, 0x8322));
}

#[test]
fn tm6_remaps_count() {
    assert_eq!(TM6_REMAPS.len(), 18, "expected 18 TM6 remap entries");
}

#[test]
fn tm6_remaps_offsets_valid() {
    // TM6 is 2 pages × 19 rows × 32 cols × 2 bytes = 2432 bytes
    let tm6_size = 2 * TM_PAGE_ROWS * TM_COLS * 2;
    for &(row, col, new_tile) in TM6_REMAPS {
        let idx = if col < 32 {
            row * TM_COLS + col
        } else {
            TM_PAGE_ROWS * TM_COLS + row * TM_COLS + (col - 32)
        };
        let offset = idx * 2;
        assert!(
            offset + 2 <= tm6_size,
            "remap ({},{}) offset {} out of TM6 (max {})",
            row,
            col,
            offset,
            tm6_size,
        );
        assert!(
            new_tile >= TILE_BASE
                || new_tile == TM6_BLANK_TILE
                || new_tile == 0x268
                || new_tile == 0x2D6,
            "unexpected tile ${:03X}",
            new_tile
        );
    }
}

#[test]
fn tm6_remap_tiles_in_overlay_range() {
    // All remap target tiles should be within the stat+magic overlay range
    for &(_, _, new_tile) in TM6_REMAPS {
        assert!(
            (OVERLAY_FIRST..=OVERLAY_LAST).contains(&new_tile) || new_tile == TM6_BLANK_TILE, // transparent tile ($227) is below overlay range
            "TM6 remap tile ${:03X} outside expected range",
            new_tile
        );
    }
}

#[test]
fn tm6_remap_hook_assembles() {
    // Build a representative TM6 remap hook
    let remap_values: Vec<(u16, u16)> = TM6_REMAPS
        .iter()
        .enumerate()
        .map(|(i, &(row, col, new_tile))| {
            let idx = if col < 32 {
                row * TM_COLS + col
            } else {
                TM_PAGE_ROWS * TM_COLS + row * TM_COLS + (col - 32)
            };
            let wram_addr = 0xC800u16 + (idx as u16) * 2;
            // Use dummy attr bits for test
            let new_entry = (0x2000u16 * (i as u16 % 2)) | new_tile;
            (wram_addr, new_entry)
        })
        .collect();
    let hook = build_tm_remap_hook(&remap_values).unwrap();

    // Must start with JSL $009440
    assert_eq!(&hook[0..4], &[0x22, 0x40, 0x94, 0x00]);
    // Must end with RTL
    assert_eq!(hook[hook.len() - 1], 0x6B);
    // Must contain 16 STA abs instructions
    let sta_count = hook
        .windows(1)
        .enumerate()
        .filter(|(i, _)| *i > 4 && hook[*i] == 0x8D)
        .count();
    assert!(
        sta_count >= 16,
        "must contain at least 16 STA abs: found {}",
        sta_count
    );
    // Expected size: 17 (preamble) + 16 * 6 (LDA+STA pairs) = 113 bytes
    let expected_size = 17 + TM6_REMAPS.len() * 6;
    assert_eq!(
        hook.len(),
        expected_size,
        "TM6 hook size mismatch: expected {}, got {}",
        expected_size,
        hook.len()
    );
    println!("TM6 remap hook: {} bytes", hook.len());
}

#[test]
fn tm6_code_does_not_overlap_phase2a() {
    let phase2a_end = {
        let oa = ((CODE_BASE_ADDR as usize + HOOK_CODE_SIZE) + 0x0F) & !0x0F;
        oa + OVERLAY_SIZE_4BPP
    };
    assert!(
        TM6_CODE_ADDR as usize >= phase2a_end,
        "TM6 code ${:04X} overlaps Phase 2a end ${:04X}",
        TM6_CODE_ADDR,
        phase2a_end,
    );
}

#[test]
fn tm6_code_does_not_overlap_phase2b() {
    // TM6 hook max size: 17 + 16*6 = 113 bytes
    let tm6_hook_end = TM6_CODE_ADDR as usize + 17 + TM6_REMAPS.len() * 6;
    assert!(
        OPT_CODE_ADDR as usize >= tm6_hook_end,
        "Phase 2b code ${:04X} overlaps TM6 hook end ${:04X}",
        OPT_CODE_ADDR,
        tm6_hook_end,
    );
}

// ── TM5 remap validation ───────────────────────────────────────────

#[test]
fn tm5_remaps_offsets_valid() {
    for &(row, col, new_tile) in TM5_REMAPS {
        let offset = (row * TM5_COLS + col) * 2;
        // TM5 is 1020 bytes (17 rows × 30 cols × 2)
        assert!(
            offset + 2 <= 1020,
            "remap ({},{}) offset {} out of TM5",
            row,
            col,
            offset
        );
        assert!(
            new_tile >= TILE_BASE,
            "new tile ${:03X} < TILE_BASE",
            new_tile
        );
        let rel = (new_tile - TILE_BASE) as usize;
        assert!(
            rel < OPT_CHR_TILES,
            "new tile ${:03X} outside CHR range",
            new_tile
        );
    }
}

#[test]
fn tm5_remap_targets_are_in_tile_map() {
    // Every remap target tile must have a mapping in OPT_TILE_MAP
    for &(_, _, new_tile) in TM5_REMAPS {
        assert!(
            OPT_TILE_MAP.iter().any(|&(t, _)| t == new_tile),
            "remap target ${:03X} not in OPT_TILE_MAP",
            new_tile
        );
    }
}

// ── Tilemap access ──────────────────────────────────────────────────

#[test]
fn tilemap_tile_index_calculations() {
    assert_eq!(tilemap_tile_at(&[0x27, 0x02], 0, 0), 0x227);

    let mut tm = vec![0u8; 2432];

    // Left page (row=5, col=17)
    let left_idx = 5 * 32 + 17;
    tm[left_idx * 2] = 0x6A;
    tm[left_idx * 2 + 1] = 0x02;
    assert_eq!(tilemap_tile_at(&tm, 5, 17), 0x26A);

    // Right page (row=3, col=35)
    let right_idx = 19 * 32 + 3 * 32 + (35 - 32);
    tm[right_idx * 2] = 0x64;
    tm[right_idx * 2 + 1] = 0x02;
    assert_eq!(tilemap_tile_at(&tm, 3, 35), 0x264);
}

#[test]
fn tilemap_entry_preserves_attributes() {
    let mut tm = vec![0u8; 1024];
    // Entry at (7,8): tile=$22A with palette 6 → attr byte = 00_0_110_10 = $1A
    let idx = (7 * 32 + 8) * 2;
    tm[idx] = 0x2A; // low byte: tile bits 0-7
    tm[idx + 1] = 0x1A; // high byte: YXPCCC=001100, tile bits 8-9=10
    let entry = tilemap_entry_at(&tm, 7, 8, 32);
    assert_eq!(entry, 0x1A2A);
    assert_eq!(entry & 0x03FF, 0x22A, "tile index");
    assert_eq!(entry & 0xFC00, 0x1800, "attribute bits");
}

// ── Data placement ──────────────────────────────────────────────────

#[test]
fn data_placement_within_bank() {
    let overlay_addr = ((CODE_BASE_ADDR as usize + HOOK_CODE_SIZE) + 0x0F) & !0x0F;
    let data_end = overlay_addr + OVERLAY_SIZE_4BPP;
    assert!(
        data_end <= 0xFFFF,
        "Data end ${:X} exceeds bank boundary",
        data_end
    );
    println!(
        "Phase 2a: hook=${:04X} ({}B), overlay=${:04X} ({}B), end=${:04X}",
        CODE_BASE_ADDR, HOOK_CODE_SIZE, overlay_addr, OVERLAY_SIZE_4BPP, data_end,
    );
}

#[test]
fn opt_data_placement_within_bank() {
    let overlay_addr = ((OPT_CODE_ADDR as usize + HOOK_CODE_SIZE) + 0x0F) & !0x0F;
    let chr_data_end = overlay_addr + OPT_OVERLAY_SIZE_4BPP;

    // Must not overlap Phase 2a
    let phase2a_end = {
        let oa = ((CODE_BASE_ADDR as usize + HOOK_CODE_SIZE) + 0x0F) & !0x0F;
        oa + OVERLAY_SIZE_4BPP
    };
    assert!(
        OPT_CODE_ADDR as usize >= phase2a_end,
        "Phase 2b code ${:04X} overlaps Phase 2a end ${:04X}",
        OPT_CODE_ADDR,
        phase2a_end,
    );

    // TM5 hook follows CHR overlay
    let tm5_hook = build_tm_remap_hook(&[
        (0xC8F0, 0x3200),
        (0xC9D0, 0x327D),
        (0xC9D2, 0x327E),
        (0xC908, 0x327F),
    ])
    .unwrap();
    let tm5_code_addr = chr_data_end;
    let tm5_end = tm5_code_addr + tm5_hook.len();
    assert!(
        tm5_end <= 0xFFFF,
        "TM5 hook end ${:X} exceeds bank boundary",
        tm5_end
    );

    println!(
        "Phase 2b: CHR hook=${:04X} ({}B), overlay=${:04X} ({}B), TM5 hook=${:04X} ({}B), end=${:04X}",
        OPT_CODE_ADDR, HOOK_CODE_SIZE, overlay_addr, OPT_OVERLAY_SIZE_4BPP,
        tm5_code_addr, tm5_hook.len(), tm5_end,
    );
}

// ── Core logic: apply_regions_overlay ────────────────────────────────

#[test]
fn apply_regions_overlay_patches_8x8() {
    let overlay_first = 0x200u16;
    let overlay_last = 0x203u16;
    let overlay_tiles = 4;

    // Initial overlay: blank tiles
    let blank = BLANK_TILE;
    let mut overlay = vec![0u8; overlay_tiles * TILE_SIZE];
    for i in 0..overlay_tiles {
        overlay[i * TILE_SIZE..(i + 1) * TILE_SIZE].copy_from_slice(&blank);
    }

    // Test glyphs
    let glyph_a = make_test_tile(0xAA);
    let glyph_b = make_test_tile(0x55);

    let regions = [TextRegion {
        ko: "가나",
        size: TileSize::S8x8,
        row: 0,
        col_start: 0,
        col_end: 2,
    }];

    // Tilemap: (0,0)→$200, (0,1)→$201
    let tile_at = |_row: usize, col: usize| -> u16 { 0x200 + col as u16 };
    let glyphs = GlyphSet {
        chars_8: &['가', '나'],
        tiles_8: &[glyph_a, glyph_b],
        chars_16: &[],
        tiles_16: &[],
    };

    let mut patched = vec![false; overlay_tiles];
    let count = apply_regions_overlay(
        &mut overlay,
        &regions,
        &tile_at,
        &glyphs,
        &mut patched,
        overlay_first,
        overlay_last,
    )
    .unwrap();

    assert_eq!(count, 2);
    assert!(patched[0]);
    assert!(patched[1]);
    assert!(!patched[2]);

    // Tile 0 should have glyph_a pattern
    for r in 0..8 {
        assert_eq!(overlay[r * 2], 0xAA, "tile 0 BP0 row {}", r);
    }
    // Tile 1 should have glyph_b pattern
    for r in 0..8 {
        assert_eq!(overlay[TILE_SIZE + r * 2], 0x55, "tile 1 BP0 row {}", r);
    }
}

#[test]
fn apply_regions_overlay_patches_16x16() {
    let overlay_first = 0x200u16;
    let overlay_last = 0x203u16;
    let overlay_tiles = 4;

    let blank = BLANK_TILE;
    let mut overlay = vec![0u8; overlay_tiles * TILE_SIZE];
    for i in 0..overlay_tiles {
        overlay[i * TILE_SIZE..(i + 1) * TILE_SIZE].copy_from_slice(&blank);
    }

    // 16x16 glyph for '가': 4 distinct quads
    let glyph_16: [[u8; TILE_SIZE]; 4] = [
        make_test_tile(0x11), // TL (quad 0)
        make_test_tile(0x22), // TR (quad 1)
        make_test_tile(0x33), // BL (quad 2)
        make_test_tile(0x44), // BR (quad 3)
    ];

    let regions = [TextRegion {
        ko: "가",
        size: TileSize::S16x16,
        row: 0,
        col_start: 0,
        col_end: 2,
    }];

    // Tilemap: (0,0)→$200, (0,1)→$201, (1,0)→$202, (1,1)→$203
    let tile_at = |row: usize, col: usize| -> u16 { 0x200 + (row * 2 + col) as u16 };
    let glyphs = GlyphSet {
        chars_8: &[],
        tiles_8: &[],
        chars_16: &['가'],
        tiles_16: &[glyph_16],
    };

    let mut patched = vec![false; overlay_tiles];
    let count = apply_regions_overlay(
        &mut overlay,
        &regions,
        &tile_at,
        &glyphs,
        &mut patched,
        overlay_first,
        overlay_last,
    )
    .unwrap();

    assert_eq!(count, 4); // 2 rows × 2 cols
    assert!(patched.iter().all(|&p| p));

    // Verify each quad was placed in the correct tile
    for r in 0..8 {
        assert_eq!(overlay[0 * TILE_SIZE + r * 2], 0x11, "TL row {}", r);
        assert_eq!(overlay[1 * TILE_SIZE + r * 2], 0x22, "TR row {}", r);
        assert_eq!(overlay[2 * TILE_SIZE + r * 2], 0x33, "BL row {}", r);
        assert_eq!(overlay[3 * TILE_SIZE + r * 2], 0x44, "BR row {}", r);
    }
}

#[test]
fn apply_regions_overlay_skips_out_of_range() {
    let overlay_first = 0x200u16;
    let overlay_last = 0x201u16;
    let overlay_tiles = 2;

    let blank = BLANK_TILE;
    let mut overlay = vec![0u8; overlay_tiles * TILE_SIZE];
    for i in 0..overlay_tiles {
        overlay[i * TILE_SIZE..(i + 1) * TILE_SIZE].copy_from_slice(&blank);
    }

    let regions = [TextRegion {
        ko: "가나",
        size: TileSize::S8x8,
        row: 0,
        col_start: 0,
        col_end: 2,
    }];

    // Tile (0,1) is outside overlay range
    let tile_at = |_row: usize, col: usize| -> u16 {
        if col == 0 {
            0x200
        } else {
            0x300
        } // $300 outside $200-$201
    };
    let glyphs = GlyphSet {
        chars_8: &['가', '나'],
        tiles_8: &[make_test_tile(0xAA), make_test_tile(0x55)],
        chars_16: &[],
        tiles_16: &[],
    };

    let mut patched = vec![false; overlay_tiles];
    let count = apply_regions_overlay(
        &mut overlay,
        &regions,
        &tile_at,
        &glyphs,
        &mut patched,
        overlay_first,
        overlay_last,
    )
    .unwrap();

    assert_eq!(count, 1, "only tile in range should be patched");
    assert!(patched[0]);
    assert!(!patched[1]);
}

// ── Core logic: build_4bpp_overlay ──────────────────────────────────

#[test]
fn build_4bpp_ko_tile_stat_magic() {
    let mut overlay_2bpp = vec![0u8; TILE_SIZE];
    for r in 0..8 {
        overlay_2bpp[r * 2] = 0xAA; // BP0 = glyph
        overlay_2bpp[r * 2 + 1] = 0xFF; // BP1 = $FF
    }

    let patched = [true];
    let result = build_4bpp_overlay(
        &overlay_2bpp,
        &[],
        &patched,
        0,
        1,
        PaletteMode::StatMagic,
        None,
    );

    assert_eq!(result.len(), TILE_SIZE_4BPP);
    for r in 0..8 {
        assert_eq!(result[r * 2], 0x00, "BP0 row {}", r);
        assert_eq!(result[r * 2 + 1], 0xAA, "BP1 row {}", r);
        assert_eq!(result[TILE_SIZE + r * 2], 0x00, "BP2 row {}", r);
        assert_eq!(result[TILE_SIZE + r * 2 + 1], 0xFF, "BP3 row {}", r);
    }
}

#[test]
fn build_4bpp_ko_tile_options() {
    let mut overlay_2bpp = vec![0u8; TILE_SIZE];
    for r in 0..8 {
        overlay_2bpp[r * 2] = 0xAA; // BP0 = glyph pattern
        overlay_2bpp[r * 2 + 1] = 0xFF; // BP1 = $FF
    }

    let patched = [true];
    let result = build_4bpp_overlay(
        &overlay_2bpp,
        &[],
        &patched,
        0,
        1,
        PaletteMode::Options,
        None,
    );

    assert_eq!(result.len(), TILE_SIZE_4BPP);
    // Options palette: text=0x0D, bg=0x0F. BP0=$FF, BP1=~glyph, BP2=$FF, BP3=$FF
    for r in 0..8 {
        assert_eq!(result[r * 2], 0xFF, "BP0 row {} should be $FF", r);
        assert_eq!(result[r * 2 + 1], !0xAA, "BP1 row {} should be ~glyph", r);
        assert_eq!(
            result[TILE_SIZE + r * 2],
            0xFF,
            "BP2 row {} should be $FF",
            r
        );
        assert_eq!(
            result[TILE_SIZE + r * 2 + 1],
            0xFF,
            "BP3 row {} should be $FF",
            r
        );
    }
}

#[test]
fn build_4bpp_options_blank_is_all_ff() {
    // Blank tile: glyph_row = $00
    let blank = BLANK_TILE; // BP0=$00, BP1=$FF
    let overlay_2bpp = blank.to_vec();

    let patched = [true];
    let result = build_4bpp_overlay(
        &overlay_2bpp,
        &[],
        &patched,
        0,
        1,
        PaletteMode::Options,
        None,
    );

    // All pixels should be palette 15 (all bitplanes = 1)
    for r in 0..8 {
        assert_eq!(result[r * 2], 0xFF, "BP0 row {}", r);
        assert_eq!(result[r * 2 + 1], 0xFF, "BP1 row {}", r); // ~$00 = $FF
        assert_eq!(result[TILE_SIZE + r * 2], 0xFF, "BP2 row {}", r);
        assert_eq!(result[TILE_SIZE + r * 2 + 1], 0xFF, "BP3 row {}", r);
    }
}

#[test]
fn build_4bpp_options_stroke_is_idx1() {
    // Full stroke tile: glyph_row = $FF
    let mut overlay_2bpp = vec![0u8; TILE_SIZE];
    for r in 0..8 {
        overlay_2bpp[r * 2] = 0xFF; // all stroke
        overlay_2bpp[r * 2 + 1] = 0xFF;
    }

    let patched = [true];
    let result = build_4bpp_overlay(
        &overlay_2bpp,
        &[],
        &patched,
        0,
        1,
        PaletteMode::Options,
        None,
    );

    // Stroke = idx 13 = 0x0D = 0b1101: BP0=1, BP1=0, BP2=1, BP3=1
    for r in 0..8 {
        assert_eq!(result[r * 2], 0xFF, "BP0 row {}", r); // 1
        assert_eq!(result[r * 2 + 1], 0x00, "BP1 row {}", r); // ~$FF = 0
        assert_eq!(result[TILE_SIZE + r * 2], 0xFF, "BP2 row {}", r); // 1
        assert_eq!(result[TILE_SIZE + r * 2 + 1], 0xFF, "BP3 row {}", r); // 1
    }
}

#[test]
fn build_4bpp_jp_tile_preserves_original() {
    // JP CHR: 2 2bpp tiles (BP0+BP1 at tile 0, BP2+BP3 at tile 1)
    let mut jp_chr = vec![0u8; 2 * TILE_SIZE];
    for r in 0..8 {
        jp_chr[r * 2] = 0xCC; // BP0 of 4bpp tile 0
        jp_chr[r * 2 + 1] = 0xDD; // BP1 of 4bpp tile 0
        jp_chr[TILE_SIZE + r * 2] = 0xEE; // BP2 of 4bpp tile 0
        jp_chr[TILE_SIZE + r * 2 + 1] = 0xFF; // BP3 of 4bpp tile 0
    }

    // 2bpp overlay = BP0+BP1 from tile 0
    let overlay_2bpp = jp_chr[..TILE_SIZE].to_vec();

    let patched = [false];
    // PaletteMode doesn't matter for unpatched tiles
    let result = build_4bpp_overlay(
        &overlay_2bpp,
        &jp_chr,
        &patched,
        0,
        1,
        PaletteMode::Options,
        None,
    );

    assert_eq!(result.len(), TILE_SIZE_4BPP);
    for r in 0..8 {
        assert_eq!(result[r * 2], 0xCC, "BP0 row {}", r);
        assert_eq!(result[r * 2 + 1], 0xDD, "BP1 row {}", r);
        assert_eq!(result[TILE_SIZE + r * 2], 0xEE, "BP2 row {}", r);
        assert_eq!(result[TILE_SIZE + r * 2 + 1], 0xFF, "BP3 row {}", r);
    }
}

// ── HeaderBorderTop palette encoding ─────────────────────────────────

#[test]
fn build_4bpp_header_border_top() {
    let overlay_2bpp = BLANK_TILE.to_vec();
    let patched = [true];
    let result = build_4bpp_overlay(
        &overlay_2bpp,
        &[],
        &patched,
        0,
        1,
        PaletteMode::HeaderBorderTop,
        None,
    );

    assert_eq!(result.len(), TILE_SIZE_4BPP);
    // Row 0: 0x0C (1100) → BP0=0, BP1=0, BP2=1, BP3=1
    assert_eq!(result[0], 0x00, "row0 BP0");
    assert_eq!(result[1], 0x00, "row0 BP1");
    assert_eq!(result[TILE_SIZE], 0xFF, "row0 BP2");
    assert_eq!(result[TILE_SIZE + 1], 0xFF, "row0 BP3");
    // Row 1: 0x0E (1110) → BP0=0, BP1=1, BP2=1, BP3=1
    assert_eq!(result[2], 0x00, "row1 BP0");
    assert_eq!(result[3], 0xFF, "row1 BP1");
    assert_eq!(result[TILE_SIZE + 2], 0xFF, "row1 BP2");
    assert_eq!(result[TILE_SIZE + 3], 0xFF, "row1 BP3");
    // Rows 2-7: 0x0A (1010) → BP0=0, BP1=1, BP2=0, BP3=1
    for r in 2..8 {
        assert_eq!(result[r * 2], 0x00, "row{} BP0", r);
        assert_eq!(result[r * 2 + 1], 0xFF, "row{} BP1", r);
        assert_eq!(result[TILE_SIZE + r * 2], 0x00, "row{} BP2", r);
        assert_eq!(result[TILE_SIZE + r * 2 + 1], 0xFF, "row{} BP3", r);
    }
}

#[test]
fn build_4bpp_header_border_bottom() {
    let overlay_2bpp = BLANK_TILE.to_vec();
    let patched = [true];
    let result = build_4bpp_overlay(
        &overlay_2bpp,
        &[],
        &patched,
        0,
        1,
        PaletteMode::HeaderBorderBottom,
        None,
    );

    assert_eq!(result.len(), TILE_SIZE_4BPP);
    // Rows 0-5: 0x0A
    for r in 0..6 {
        assert_eq!(result[r * 2], 0x00, "row{} BP0", r);
        assert_eq!(result[r * 2 + 1], 0xFF, "row{} BP1", r);
        assert_eq!(result[TILE_SIZE + r * 2], 0x00, "row{} BP2", r);
        assert_eq!(result[TILE_SIZE + r * 2 + 1], 0xFF, "row{} BP3", r);
    }
    // Row 6: 0x0E
    assert_eq!(result[12], 0x00, "row6 BP0");
    assert_eq!(result[13], 0xFF, "row6 BP1");
    assert_eq!(result[TILE_SIZE + 12], 0xFF, "row6 BP2");
    assert_eq!(result[TILE_SIZE + 13], 0xFF, "row6 BP3");
    // Row 7: 0x0C
    assert_eq!(result[14], 0x00, "row7 BP0");
    assert_eq!(result[15], 0x00, "row7 BP1");
    assert_eq!(result[TILE_SIZE + 14], 0xFF, "row7 BP2");
    assert_eq!(result[TILE_SIZE + 15], 0xFF, "row7 BP3");
}

// ── Per-tile palette encoding ────────────────────────────────────────

#[test]
fn build_4bpp_per_tile_palette() {
    // 2 tiles: tile 0 = HeaderBorderTop, tile 1 = Options mode
    let mut overlay_2bpp = vec![0u8; 2 * TILE_SIZE];
    for r in 0..8 {
        overlay_2bpp[r * 2] = 0xAA;
        overlay_2bpp[r * 2 + 1] = 0xFF;
        overlay_2bpp[TILE_SIZE + r * 2] = 0xBB;
        overlay_2bpp[TILE_SIZE + r * 2 + 1] = 0xFF;
    }

    let patched = [true, true];
    let tile_palettes = [PaletteMode::HeaderBorderTop, PaletteMode::Options];
    let result = build_4bpp_overlay(
        &overlay_2bpp,
        &[],
        &patched,
        0,
        2,
        PaletteMode::Options,
        Some(&tile_palettes),
    );

    assert_eq!(result.len(), 2 * TILE_SIZE_4BPP);

    // Tile 0: HeaderBorderTop — row 0 = 0x0C, row 1 = 0x0E, rest = 0x0A
    assert_eq!(result[0], 0x00, "tile0 row0 BP0");
    assert_eq!(result[1], 0x00, "tile0 row0 BP1");
    assert_eq!(result[TILE_SIZE], 0xFF, "tile0 row0 BP2");
    assert_eq!(result[TILE_SIZE + 1], 0xFF, "tile0 row0 BP3");

    // Tile 1: Options — BP0=$FF, BP1=~glyph, BP2=$FF, BP3=$FF
    let t1 = TILE_SIZE_4BPP;
    for r in 0..8 {
        assert_eq!(result[t1 + r * 2], 0xFF, "tile1 BP0 row {}", r);
        assert_eq!(result[t1 + r * 2 + 1], !0xBB, "tile1 BP1 row {}", r);
        assert_eq!(result[t1 + TILE_SIZE + r * 2], 0xFF, "tile1 BP2 row {}", r);
        assert_eq!(
            result[t1 + TILE_SIZE + r * 2 + 1],
            0xFF,
            "tile1 BP3 row {}",
            r
        );
    }
}

// ── lookup_lz_source tests ──────────────────────────────────────────

#[test]
fn lookup_lz_source_error_on_short_rom() {
    let rom = vec![0u8; 4]; // too small for any entry
    let result = lookup_lz_source(&rom, 10);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("out of bounds"));
}

#[test]
fn lookup_lz_source_valid() {
    // Build a ROM large enough to hold the pointer table
    let mut rom = vec![0u8; LZ_PTR_TABLE_PC + 20];
    // Entry 3: pointer = $9000
    rom[LZ_PTR_TABLE_PC + 6] = 0x00;
    rom[LZ_PTR_TABLE_PC + 7] = 0x90;
    let result = lookup_lz_source(&rom, 3).unwrap();
    assert_eq!(result, lorom_to_pc(LZ_PTR_BANK, 0x9000));
}

// ── Config consistency ──────────────────────────────────────────────

#[test]
fn screen_configs_consistent() {
    // Phase 2a
    assert_eq!(STAT_MAGIC_CONFIG.hook_pc, lorom_to_pc(0x01, 0x820A));
    assert_eq!(STAT_MAGIC_CONFIG.overlay_first, OVERLAY_FIRST);
    assert_eq!(STAT_MAGIC_CONFIG.overlay_last, OVERLAY_LAST);
    assert_eq!(STAT_MAGIC_CONFIG.region_groups.len(), 2);

    // Phase 2b: separate constants, no ScreenHookConfig
    assert_eq!(OPT_CHR_HOOK_PC, lorom_to_pc(0x01, 0x8F46));
    assert_eq!(OPT_TM5_HOOK_PC, lorom_to_pc(0x01, 0x8F83));
    assert_eq!(OPT_CHR_TILES, 128);
    assert_eq!(OPT_WRAM_DEST, 0xD000);
    assert_eq!(TM5_WRAM_BASE, 0xC800);

    // TM6 hook site
    assert_eq!(TM6_HOOK_PC, lorom_to_pc(0x01, 0x8322));

    // No overlap between Phase 2a, TM6, and Phase 2b in Bank $1C
    let phase2a_end = {
        let oa = ((STAT_MAGIC_CONFIG.code_addr as usize + HOOK_CODE_SIZE) + 0x0F) & !0x0F;
        oa + OVERLAY_SIZE_4BPP
    };
    assert!(
        TM6_CODE_ADDR as usize >= phase2a_end,
        "TM6 overlaps Phase 2a"
    );
    let tm6_hook_end = TM6_CODE_ADDR as usize + 17 + TM6_REMAPS.len() * 6;
    assert!(
        OPT_CODE_ADDR as usize >= tm6_hook_end,
        "Phase 2b overlaps TM6"
    );
}
