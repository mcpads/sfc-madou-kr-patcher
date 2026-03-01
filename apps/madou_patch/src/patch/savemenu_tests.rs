use super::*;

#[test]
fn dma_hook_assembles_and_ends_with_rtl() {
    let code = build_dma_hook((0x00, 0xC0, 0x01), 0x19, 0xD540, 4528).unwrap();
    // Must end with RTL (0x6B)
    assert_eq!(*code.last().unwrap(), 0x6B, "hook must end with RTL");
    // Reasonable size (< 128 bytes)
    assert!(code.len() < 128, "hook code {} bytes too large", code.len());
    println!("DMA hook: {} bytes", code.len());
}

#[test]
fn dma_hook_contains_dma_trigger() {
    let code = build_dma_hook((0x00, 0xC0, 0x01), 0x19, 0xD540, 0x1000).unwrap();
    // Must write 0x20 to $420B (STA $420B = 8D 0B 42)
    let trigger = [0x8D, 0x0B, 0x42];
    assert!(
        code.windows(3).any(|w| w == trigger),
        "hook must contain STA $420B"
    );
}

#[test]
fn dma_hook_contains_wram_address() {
    let code = build_dma_hook((0x00, 0xC0, 0x01), 0x19, 0xD540, 0x1000).unwrap();
    // Must write $C0 to $2182 (WRAM mid)
    let sta_2182 = [0x8D, 0x82, 0x21];
    assert!(
        code.windows(3).any(|w| w == sta_2182),
        "hook must write to $2182"
    );
}

#[test]
fn three_hooks_fit_in_code_region() {
    // All 3 hooks should fit in < 256 bytes
    let h1 = build_dma_hook(WRAM_CHR, 0x19, 0xD540, 4528).unwrap();
    let h2 = build_dma_hook(WRAM_MAIN_TM, 0x19, 0xE700, 1920).unwrap();
    let h3 = build_dma_hook(WRAM_TITLE_TM, 0x19, 0xEE80, 1152).unwrap();
    let total = h1.len() + h2.len() + h3.len();
    assert!(total < 256, "3 hooks = {} bytes, too large", total);
    println!("3 hooks total: {} bytes", total);
}

#[test]
fn tilemap_get_set_preserves_attributes() {
    // Tilemap entry: tile $0D8 with palette 3, priority, V-flip
    // = 0b1010_1100_1101_1000 = 0xACD8
    let mut tm = vec![0xD8, 0xAC]; // LE
    assert_eq!(get_tile_index(&tm, 0), 0x0D8);

    set_tile_index(&mut tm, 0, 0x116);
    // New tile $116 = 0b01_0001_0110
    // Attributes preserved: 0xAC00 | 0x0116 = 0xAD16
    assert_eq!(get_tile_index(&tm, 0), 0x116);
    let word = u16::from_le_bytes([tm[0], tm[1]]);
    assert_eq!(word & 0xFC00, 0xAC00, "attributes must be preserved");
}

#[test]
fn chr_patch_adds_conflict_tiles() {
    let max_idx = 0x10D;
    let tile_count = max_idx as usize + 1;
    let mut chr = vec![0u8; tile_count * TILE_8X8_SIZE];

    let ko_tiles: Vec<(char, [u8; 64])> = KO_CHARS
        .iter()
        .enumerate()
        .map(|(i, &ch)| {
            let mut tile = [0u8; 64];
            tile.fill((i + 1) as u8);
            (ch, tile)
        })
        .collect();

    let ct = patch_chr_tiles(&mut chr, &ko_tiles).unwrap();

    // 1 복BR + 4 제(す) + 4 제(さ) + 4 사(す) + 4 제(よ) + 1 num1TR = 18 new tiles
    let new_count = chr.len() / TILE_8X8_SIZE;
    assert_eq!(new_count, tile_count + 18);
    assert_eq!(ct.boku_br_idx, tile_count as u16);
    assert_eq!(ct.je_su[0], tile_count as u16 + 1);
    assert_eq!(ct.je_sa[0], tile_count as u16 + 5);
    assert_eq!(ct.sa_su[0], tile_count as u16 + 9);
    assert_eq!(ct.je_yo[0], tile_count as u16 + 13);
    assert_eq!(ct.num1_tr_idx, tile_count as u16 + 17);
}

#[test]
fn chr_patch_writes_primary_tiles() {
    let max_idx = 0x10D; // must cover さない bottom tiles ($108-$10D)
    let tile_count = max_idx as usize + 1;
    let mut chr = vec![0u8; tile_count * TILE_8X8_SIZE];

    // '시' uses tiles [D0, D1, E4, E5], fill pattern = 1
    let ko_tiles: Vec<(char, [u8; 64])> = KO_CHARS
        .iter()
        .enumerate()
        .map(|(i, &ch)| {
            let mut tile = [0u8; 64];
            tile.fill((i + 1) as u8);
            (ch, tile)
        })
        .collect();

    patch_chr_tiles(&mut chr, &ko_tiles).unwrap();

    // Check '시' TL tile at D0
    let si_fill = 1u8; // '시' is index 0 in KO_CHARS → fill = 1
    let d0_offset = 0xD0 * TILE_8X8_SIZE;
    assert!(
        chr[d0_offset..d0_offset + TILE_8X8_SIZE]
            .iter()
            .all(|&b| b == si_fill),
        "시 TL tile data wrong"
    );
}

#[test]
fn chr_patch_skips_e7_for_boku() {
    let max_idx = 0x10D; // must cover さない bottom tiles ($108-$10D)
    let tile_count = max_idx as usize + 1;
    let mut chr = vec![0xAAu8; tile_count * TILE_8X8_SIZE];

    let ko_tiles: Vec<(char, [u8; 64])> = KO_CHARS
        .iter()
        .enumerate()
        .map(|(i, &ch)| {
            let mut tile = [0u8; 64];
            tile.fill((i + 1) as u8);
            (ch, tile)
        })
        .collect();

    patch_chr_tiles(&mut chr, &ko_tiles).unwrap();

    // E7 should contain 작BR data (fill=2), NOT 복BR (fill=6)
    let jak_fill = 2u8; // '작' is index 1
    let e7_offset = 0xE7 * TILE_8X8_SIZE;
    // 작's BR is the 4th quadrant (bytes 48..64) → fill byte = 2
    assert_eq!(chr[e7_offset], jak_fill, "E7 should contain 작BR, not 복BR");
}

fn make_test_ct() -> ConflictTiles {
    ConflictTiles {
        boku_br_idx: 0x116,
        je_su: [0x117, 0x118, 0x119, 0x11A],
        je_sa: [0x11B, 0x11C, 0x11D, 0x11E],
        sa_su: [0x11F, 0x120, 0x121, 0x122],
        je_yo: [0x123, 0x124, 0x125, 0x126],
        num1_tr_idx: 0x127,
    }
}

#[test]
fn resolve_conflicts_replaces_e7_in_u_context() {
    let mut tm = vec![0u8; TM_WIDTH * 2 * 2];
    set_tile_index(&mut tm, 5, 0xEE);
    set_tile_index(&mut tm, 6, 0xE7);
    set_tile_index(&mut tm, 10, 0xE6);
    set_tile_index(&mut tm, 11, 0xE7);

    let ct = make_test_ct();
    let count = resolve_tilemap_conflicts(&mut tm, &ct);

    assert_eq!(count, 1);
    assert_eq!(get_tile_index(&tm, 6), 0x116); // う E7 → 복BR
    assert_eq!(get_tile_index(&tm, 11), 0xE7); // じ E7 → unchanged
}

#[test]
fn resolve_conflicts_replaces_f8_in_num1_context() {
    let mut tm = vec![0u8; TM_WIDTH * 2 * 2];
    // ル TR: left = $F7 (ル TL) → should NOT be replaced
    set_tile_index(&mut tm, 4, 0xF7);
    set_tile_index(&mut tm, 5, 0xF8);
    // Number 1 TR: left = $F9 (number 1 TL) → SHOULD be replaced
    set_tile_index(&mut tm, 8, 0xF9);
    set_tile_index(&mut tm, 9, 0xF8);

    let ct = make_test_ct();
    let count = resolve_tilemap_conflicts(&mut tm, &ct);

    assert_eq!(count, 1);
    assert_eq!(get_tile_index(&tm, 5), 0xF8); // ル TR unchanged
    assert_eq!(get_tile_index(&tm, 9), 0x127); // number 1 TR → new tile
}

#[test]
fn resolve_conflicts_replaces_su_in_ke_context() {
    let mut tm = vec![0u8; TM_WIDTH * 2 * 4];
    set_tile_index(&mut tm, 3, 0xE3);
    set_tile_index(&mut tm, 4, 0xDF);
    set_tile_index(&mut tm, 5, 0xE0);
    set_tile_index(&mut tm, TM_WIDTH + 4, 0xF1);
    set_tile_index(&mut tm, TM_WIDTH + 5, 0xF2);
    // つ+す pattern that should NOT be replaced
    set_tile_index(&mut tm, 2 * TM_WIDTH + 3, 0xDE);
    set_tile_index(&mut tm, 2 * TM_WIDTH + 4, 0xDF);

    let ct = make_test_ct();
    let count = resolve_tilemap_conflicts(&mut tm, &ct);

    assert_eq!(count, 4);
    assert_eq!(get_tile_index(&tm, 4), 0x117); // す TL → 제 TL
    assert_eq!(get_tile_index(&tm, 5), 0x118); // す TR → 제 TR
    assert_eq!(get_tile_index(&tm, TM_WIDTH + 4), 0x119); // す BL → 제 BL
    assert_eq!(get_tile_index(&tm, TM_WIDTH + 5), 0x11A); // す BR → 제 BR
    assert_eq!(get_tile_index(&tm, 2 * TM_WIDTH + 4), 0xDF); // unchanged
}

#[test]
fn resolve_conflicts_replaces_su_with_gap_before_ke() {
    let mut tm = vec![0u8; TM_WIDTH * 2 * 4];
    set_tile_index(&mut tm, 3, 0xE3);
    set_tile_index(&mut tm, 6, 0xDF);
    set_tile_index(&mut tm, 7, 0xE0);
    set_tile_index(&mut tm, TM_WIDTH + 6, 0xF1);
    set_tile_index(&mut tm, TM_WIDTH + 7, 0xF2);

    let ct = make_test_ct();
    let count = resolve_tilemap_conflicts(&mut tm, &ct);

    assert_eq!(count, 4);
    assert_eq!(get_tile_index(&tm, 6), 0x117);
    assert_eq!(get_tile_index(&tm, 7), 0x118);
    assert_eq!(get_tile_index(&tm, TM_WIDTH + 6), 0x119);
    assert_eq!(get_tile_index(&tm, TM_WIDTH + 7), 0x11A);
}

#[test]
fn resolve_conflicts_kesanai_no_cascade() {
    // けさない pattern: け at col 6-7, さ at col 12-13, な at col 18-19, い at col 24-25
    // After refactor: only さ→제 in ke context (no cascade; な/い have correct primaries)
    let mut tm = vec![0u8; TM_WIDTH * 2 * 4]; // 4 rows
                                              // Row 0 top tiles
    set_tile_index(&mut tm, 6, 0xE2); // け TL
    set_tile_index(&mut tm, 7, 0xE3); // け TR
    set_tile_index(&mut tm, 12, 0xFB); // さ TL
    set_tile_index(&mut tm, 13, 0xFC); // さ TR
    set_tile_index(&mut tm, 18, 0xFD); // な TL
    set_tile_index(&mut tm, 19, 0xFE); // な TR
    set_tile_index(&mut tm, 24, 0xFF); // い TL
    set_tile_index(&mut tm, 25, 0x100); // い TR
                                        // Row 1 bottom tiles
    set_tile_index(&mut tm, TM_WIDTH + 6, 0xF3); // け BL
    set_tile_index(&mut tm, TM_WIDTH + 7, 0xF4); // け BR
    set_tile_index(&mut tm, TM_WIDTH + 12, 0x108); // さ BL
    set_tile_index(&mut tm, TM_WIDTH + 13, 0x109); // さ BR
    set_tile_index(&mut tm, TM_WIDTH + 18, 0x10A); // な BL
    set_tile_index(&mut tm, TM_WIDTH + 19, 0x10B); // な BR
    set_tile_index(&mut tm, TM_WIDTH + 24, 0x10C); // い BL
    set_tile_index(&mut tm, TM_WIDTH + 25, 0x10D); // い BR

    let ct = make_test_ct();
    let count = resolve_tilemap_conflicts(&mut tm, &ct);

    // Only さ→제(4), な and い stay as primary (안, 해)
    assert_eq!(count, 4);
    // さ → 제 conflict tiles
    assert_eq!(get_tile_index(&tm, 12), ct.je_sa[0]);
    assert_eq!(get_tile_index(&tm, 13), ct.je_sa[1]);
    assert_eq!(get_tile_index(&tm, TM_WIDTH + 12), ct.je_sa[2]);
    assert_eq!(get_tile_index(&tm, TM_WIDTH + 13), ct.je_sa[3]);
    // な stays as primary (안)
    assert_eq!(get_tile_index(&tm, 18), 0xFD);
    assert_eq!(get_tile_index(&tm, 19), 0xFE);
    // い stays as primary (해)
    assert_eq!(get_tile_index(&tm, 24), 0xFF);
    assert_eq!(get_tile_index(&tm, 25), 0x100);
}

#[test]
fn resolve_conflicts_utsusanai_no_change() {
    // うつさない pattern: NO け nearby → さ/な/い stay as primary
    // つ followed by さ (not す) → no button copy rule trigger
    let mut tm = vec![0u8; TM_WIDTH * 2 * 4];
    // Row 0: う at col 7-8, つ at col 11-12, さ at col 15-16, な at col 19-20
    set_tile_index(&mut tm, 7, 0xDB); // う TL
    set_tile_index(&mut tm, 8, 0xDC); // う TR
    set_tile_index(&mut tm, 11, 0xDD); // つ TL
    set_tile_index(&mut tm, 12, 0xDE); // つ TR
    set_tile_index(&mut tm, 15, 0xFB); // さ TL
    set_tile_index(&mut tm, 16, 0xFC); // さ TR
    set_tile_index(&mut tm, 19, 0xFD); // な TL
    set_tile_index(&mut tm, 20, 0xFE); // な TR

    let ct = make_test_ct();
    let count = resolve_tilemap_conflicts(&mut tm, &ct);

    assert_eq!(count, 0); // No け nearby, つ not followed by す → no changes
    assert_eq!(get_tile_index(&tm, 11), 0xDD); // つ unchanged
    assert_eq!(get_tile_index(&tm, 15), 0xFB); // さ unchanged
    assert_eq!(get_tile_index(&tm, 19), 0xFD); // な unchanged
}

#[test]
fn resolve_conflicts_button_copy_blanks_tsu_remaps_su() {
    // うつす pattern in main TM (non-ke): つ at col 6-7, す at col 8-9
    let mut tm = vec![0u8; TM_WIDTH * 2 * 4]; // 4 rows
                                              // Row 0 top tiles
    set_tile_index(&mut tm, 6, 0xDD); // つ TL (TSU_TL)
    set_tile_index(&mut tm, 7, 0xDE); // つ TR
    set_tile_index(&mut tm, 8, 0xDF); // す TL (CONFLICT_SU_TL)
    set_tile_index(&mut tm, 9, 0xE0); // す TR
                                      // Row 1 bottom tiles
    set_tile_index(&mut tm, TM_WIDTH + 6, 0xEF); // つ BL
    set_tile_index(&mut tm, TM_WIDTH + 7, 0xF0); // つ BR
    set_tile_index(&mut tm, TM_WIDTH + 8, 0xF1); // す BL
    set_tile_index(&mut tm, TM_WIDTH + 9, 0xF2); // す BR

    let ct = make_test_ct();
    let count = resolve_tilemap_conflicts(&mut tm, &ct);

    // つ→blank(4) + す→sa_su(4) = 8
    assert_eq!(count, 8);
    // つ → blank (BLANK_REF)
    assert_eq!(get_tile_index(&tm, 6), BLANK_REF[0]);
    assert_eq!(get_tile_index(&tm, 7), BLANK_REF[1]);
    assert_eq!(get_tile_index(&tm, TM_WIDTH + 6), BLANK_REF[2]);
    assert_eq!(get_tile_index(&tm, TM_WIDTH + 7), BLANK_REF[3]);
    // す → sa_su (사)
    assert_eq!(get_tile_index(&tm, 8), ct.sa_su[0]);
    assert_eq!(get_tile_index(&tm, 9), ct.sa_su[1]);
    assert_eq!(get_tile_index(&tm, TM_WIDTH + 8), ct.sa_su[2]);
    assert_eq!(get_tile_index(&tm, TM_WIDTH + 9), ct.sa_su[3]);
}

#[test]
fn resolve_conflicts_title_tm_kesuyo_blank_su_je_yo() {
    // けすよ pattern in title TM: け at col 6-7, す at col 8-9, よ at col 10-11
    let mut tm = vec![0u8; TM_WIDTH * 2 * 4]; // 4 rows
                                              // Row 0 top tiles
    set_tile_index(&mut tm, 6, 0xE2); // け TL
    set_tile_index(&mut tm, 7, 0xE3); // け TR
    set_tile_index(&mut tm, 8, 0xDF); // す TL
    set_tile_index(&mut tm, 9, 0xE0); // す TR
    set_tile_index(&mut tm, 10, 0xD8); // よ TL (YO_TL)
    set_tile_index(&mut tm, 11, 0xD9); // よ TR
                                       // Row 1 bottom tiles
    set_tile_index(&mut tm, TM_WIDTH + 6, 0xF3); // け BL
    set_tile_index(&mut tm, TM_WIDTH + 7, 0xF4); // け BR
    set_tile_index(&mut tm, TM_WIDTH + 8, 0xF1); // す BL
    set_tile_index(&mut tm, TM_WIDTH + 9, 0xF2); // す BR
    set_tile_index(&mut tm, TM_WIDTH + 10, 0xEC); // よ BL
    set_tile_index(&mut tm, TM_WIDTH + 11, 0xED); // よ BR

    let ct = make_test_ct();
    let count = resolve_tilemap_conflicts(&mut tm, &ct);

    // す→blank(4) + よ→je_yo(4) = 8
    assert_eq!(count, 8);
    // す → blank (BLANK_REF)
    assert_eq!(get_tile_index(&tm, 8), BLANK_REF[0]);
    assert_eq!(get_tile_index(&tm, 9), BLANK_REF[1]);
    assert_eq!(get_tile_index(&tm, TM_WIDTH + 8), BLANK_REF[2]);
    assert_eq!(get_tile_index(&tm, TM_WIDTH + 9), BLANK_REF[3]);
    // よ → je_yo (제)
    assert_eq!(get_tile_index(&tm, 10), ct.je_yo[0]);
    assert_eq!(get_tile_index(&tm, 11), ct.je_yo[1]);
    assert_eq!(get_tile_index(&tm, TM_WIDTH + 10), ct.je_yo[2]);
    assert_eq!(get_tile_index(&tm, TM_WIDTH + 11), ct.je_yo[3]);
}

#[test]
fn resolve_conflicts_title_tm_utsusuyo_no_change() {
    // うつすよ pattern in title TM (no け): all tiles stay as primary
    let mut tm = vec![0u8; TM_WIDTH * 2 * 4]; // 4 rows
    set_tile_index(&mut tm, 4, 0xDB); // う TL
    set_tile_index(&mut tm, 6, 0xDD); // つ TL
    set_tile_index(&mut tm, 8, 0xDF); // す TL
    set_tile_index(&mut tm, 10, 0xD8); // よ TL

    let ct = make_test_ct();
    let count = resolve_tilemap_conflicts(&mut tm, &ct);

    assert_eq!(count, 0);
    assert_eq!(get_tile_index(&tm, 6), 0xDD); // つ unchanged
    assert_eq!(get_tile_index(&tm, 8), 0xDF); // す unchanged
    assert_eq!(get_tile_index(&tm, 10), 0xD8); // よ unchanged
}

#[test]
fn lookup_lz_source_reads_pointer() {
    let mut rom = vec![0x00u8; 0x60000];
    // Set pointer at idx $12 (offset 0x24 from table base)
    // Value $9401 → lorom_to_pc(0x0A, 0x9401) = 0x51401
    let ptr_pc = PTR_TABLE_PC + CHR_PTR_IDX * 2;
    rom[ptr_pc] = 0x01;
    rom[ptr_pc + 1] = 0x94;
    let pc = lookup_lz_source(&rom, CHR_PTR_IDX).unwrap();
    assert_eq!(pc, lorom_to_pc(0x0A, 0x9401));
}
