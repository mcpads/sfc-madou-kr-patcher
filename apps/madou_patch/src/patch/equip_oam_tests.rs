use super::*;

#[test]
fn tile_data_size_matches_dma_groups() {
    let total: u16 = DMA_GROUPS.iter().map(|&(_, _, size)| size).sum();
    assert_eq!(total as usize, TILE_DATA_SIZE);
}

#[test]
fn dma_groups_offsets_are_contiguous() {
    // Each group's offset + size should equal the next group's offset
    for pair in DMA_GROUPS.windows(2) {
        let (off, _, size) = pair[0];
        let (next_off, _, _) = pair[1];
        assert_eq!(
            off + size,
            next_off,
            "Gap between DMA groups at offset {}+{}",
            off,
            size
        );
    }
    // Last group ends at TILE_DATA_SIZE
    let (off, _, size) = DMA_GROUPS[DMA_GROUPS.len() - 1];
    assert_eq!((off + size) as usize, TILE_DATA_SIZE);
}

#[test]
fn hook_code_starts_with_jsl_009440() {
    let code = build_equip_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    assert_eq!(&code[0..4], &JSL_LZ_BYTES, "Should start with JSL $009440");
}

#[test]
fn hook_code_ends_with_plb_plp_rtl() {
    let code = build_equip_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    let n = code.len();
    assert_eq!(code[n - 3], 0xAB, "PLB");
    assert_eq!(code[n - 2], 0x28, "PLP");
    assert_eq!(code[n - 1], 0x6B, "RTL");
}

#[test]
fn hook_code_contains_five_dma_triggers() {
    let code = build_equip_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    // DMA trigger: LDA #$40; STA $420B = [A9 40 8D 0B 42]
    let trigger_count = code
        .windows(5)
        .filter(|w| w == &[0xA9, 0x40, 0x8D, 0x0B, 0x42])
        .count();
    assert_eq!(trigger_count, 5, "Expected 5 DMA triggers (Ch6)");
}

#[test]
fn hook_code_sets_force_blank() {
    let code = build_equip_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    // LDA #$80; STA $2100 = [A9 80 8D 00 21]
    let found = code
        .windows(5)
        .any(|w| w == &[0xA9, 0x80, 0x8D, 0x00, 0x21]);
    assert!(found, "Force blank (STA $2100) not found in hook code");
}

#[test]
fn hook_code_uses_dma_ch6() {
    let code = build_equip_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    // DMA Ch6 mode register: STA $4360 = [8D 60 43]
    let found = code.windows(3).any(|w| w == &[0x8D, 0x60, 0x43]);
    assert!(found, "DMA Ch6 mode register ($4360) not found");
}

#[test]
fn hook_code_sets_source_bank() {
    let code = build_equip_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    // LDA #$25; STA $4364 = [A9 25 8D 64 43]
    let found = code
        .windows(5)
        .any(|w| w == &[0xA9, DATA_BANK, 0x8D, 0x64, 0x43]);
    assert!(found, "DMA source bank ${:02X} not found", DATA_BANK);
}

#[test]
fn hook_code_contains_all_vram_addresses() {
    let code = build_equip_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    for &(_, vram, _) in &DMA_GROUPS {
        // LDA #vram (16-bit); STA $2116 = [A9 lo hi 8D 16 21]
        let lo = (vram & 0xFF) as u8;
        let hi = (vram >> 8) as u8;
        let found = code
            .windows(6)
            .any(|w| w == &[0xA9, lo, hi, 0x8D, 0x16, 0x21]);
        assert!(found, "VRAM address ${:04X} not found in hook code", vram);
    }
}

#[test]
fn hook_code_disables_hdma_once() {
    let code = build_equip_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    // STZ $420C = [9C 0C 42]
    let hdma_count = code.windows(3).filter(|w| w == &[0x9C, 0x0C, 0x42]).count();
    assert_eq!(hdma_count, 1, "HDMA disable should appear exactly once");
}

#[test]
fn hook_code_expected_size() {
    let code = build_equip_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    // Common: JSL(4) + PHP(1) + PHB(1) + SEP(2) + 5×(LDA+STA) = 33
    // DMA1: REP(2) + 3×(LDA16+STA)(18) + SEP(2) + STZ(3) + LDA+STA(5) = 30
    // DMA2-5: REP(2) + 3×(LDA16+STA)(18) + SEP(2) + LDA+STA(5) = 27 × 4 = 108
    // Tail: PLB(1) + PLP(1) + RTL(1) = 3
    // Total: 33 + 30 + 108 + 3 = 174
    assert_eq!(code.len(), 174);
}

#[test]
fn hook_site_pc_matches_lorom() {
    assert_eq!(HOOK_SITE_PC, lorom_to_pc(0x00, 0x83FF));
}

#[test]
fn name_pairs_count() {
    assert_eq!(NAME_PAIRS.len(), 10);
}

#[test]
fn soubi_ko_chars_count() {
    assert_eq!(SOUBI_KO.len(), 2, "장비 = 2 characters");
}

#[test]
fn data_fits_in_bank() {
    let code = build_equip_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    let total = TILE_DATA_SIZE + code.len();
    let end_addr = DATA_ADDR as usize + total;
    assert!(
        end_addr <= 0x10000,
        "Data+code ({} bytes) overflows Bank ${:02X}: end=${:04X}",
        total,
        DATA_BANK,
        end_addr,
    );
}

#[test]
fn data_does_not_overlap_shop_oam() {
    // shop_oam uses $25:$CF00+
    let code = build_equip_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    let equip_end = DATA_ADDR as usize + TILE_DATA_SIZE + code.len();
    assert!(
        equip_end <= 0xCF00,
        "Equip data end ${:04X} overlaps shop_oam start $CF00",
        equip_end,
    );
}

#[test]
fn dma_groups_vram_in_obj_range() {
    // OBJ tile VRAM typically resides in $4000-$5FFF for this game
    for &(_, vram, size) in &DMA_GROUPS {
        let end = vram as u32 + (size as u32) / 2; // size is bytes, VRAM is words
        assert!(
            vram >= 0x4000 && end <= 0x6000,
            "VRAM ${:04X}..${:04X} outside OBJ range ($4000..$6000)",
            vram,
            end,
        );
    }
}

#[test]
fn group3_spans_contiguous_vram() {
    // Group 3 covers ラ ラ bot ($41E0) through ロ フ bot ($43F0+$10=$4400)
    // = $41E0..$4400 = $220 words = 1088 bytes
    let (_, vram, size) = DMA_GROUPS[2];
    assert_eq!(vram, 0x41E0);
    assert_eq!(size, 1088);
    let vram_end = vram as u32 + (size as u32) / 2;
    assert_eq!(vram_end, 0x4400);
}
