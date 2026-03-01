use super::*;

#[test]
fn tile_data_size_is_256() {
    // speech: 2×32 = 64B, sold-out4: 1×32 = 32B,
    // cookie: 3×32 = 96B, sold-out23: 2×32 = 64B → total 256B
    assert_eq!(TILE_DATA_SIZE, 256);
}

#[test]
fn hook_code_contains_three_jsl_lz() {
    let code = build_shop_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    let jsl_count = code
        .windows(4)
        .filter(|w| *w == JSL_LZ_BYTES)
        .count();
    assert_eq!(jsl_count, 3, "Expected 3× JSL $009440 (nameplate + non-shop + OBJ tile)");
}

#[test]
fn hook_code_starts_with_3byte_nameplate_guard() {
    let code = build_shop_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    // 3 pairs of LDA dp + CMP imm8 + BNE, at offsets 0/6/12
    for (i, &(dp, val)) in [(0x0B, NAMEPLATE_DP0B), (0x0C, NAMEPLATE_DP0C), (0x0D, NAMEPLATE_DP0D)]
        .iter()
        .enumerate()
    {
        let off = i * 6;
        assert_eq!(code[off], 0xA5, "LDA dp at guard {}", i);
        assert_eq!(code[off + 1], dp, "dp byte at guard {}", i);
        assert_eq!(code[off + 2], 0xC9, "CMP #imm8 at guard {}", i);
        assert_eq!(code[off + 3], val, "compare value at guard {}", i);
        assert_eq!(code[off + 4], 0xD0, "BNE at guard {}", i);
    }
}

#[test]
fn hook_code_nameplate_path_sets_flag_and_decompresses() {
    let code = build_shop_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    // After 3 guard checks (18 bytes): LDA #$01, STA $1F60, JSL $009440, RTL
    assert_eq!(code[18], 0xA9, "LDA #imm8");
    assert_eq!(code[19], 0x01, "flag value $01");
    assert_eq!(code[20], 0x8D, "STA abs");
    assert_eq!(code[21], SHOP_FLAG_WRAM as u8, "WRAM lo");
    assert_eq!(code[22], (SHOP_FLAG_WRAM >> 8) as u8, "WRAM hi");
    assert_eq!(&code[23..27], &JSL_LZ_BYTES, "JSL $009440");
    assert_eq!(code[27], 0x6B, "RTL");
}

#[test]
fn hook_code_check_flag_reads_wram_and_branches() {
    let code = build_shop_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    // check_flag at offset 28: LDA $1F60, BNE do_dma
    assert_eq!(code[28], 0xAD, "LDA abs");
    assert_eq!(code[29], SHOP_FLAG_WRAM as u8, "WRAM lo");
    assert_eq!(code[30], (SHOP_FLAG_WRAM >> 8) as u8, "WRAM hi");
    assert_eq!(code[31], 0xD0, "BNE (to do_dma)");
}

#[test]
fn hook_code_non_shop_path_after_check_flag() {
    let code = build_shop_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    // Non-shop path: JSL $009440 + RTL immediately after BNE at offset 31
    // BNE is 2 bytes (offset 31-32), so non-shop starts at 33
    assert_eq!(&code[33..37], &JSL_LZ_BYTES, "JSL $009440 (non-shop)");
    assert_eq!(code[37], 0x6B, "RTL (non-shop)");
}

#[test]
fn hook_code_bne_targets_do_dma() {
    let code = build_shop_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    // BNE at offset 31, rel at offset 32
    assert_eq!(code[31], 0xD0, "BNE opcode");
    let rel = code[32] as i8;
    let target = 33isize + rel as isize;
    // do_dma starts at offset 38 (after non-shop JSL+RTL = 5 bytes)
    assert_eq!(target, 38, "BNE should target do_dma (offset 38)");
}

#[test]
fn hook_code_guard_bne_all_target_check_flag() {
    let code = build_shop_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    // 3 BNE instructions at offsets 4, 10, 16 — all target check_flag (offset 28)
    for &bne_off in &[4usize, 10, 16] {
        assert_eq!(code[bne_off], 0xD0, "BNE at offset {}", bne_off);
        let rel = code[bne_off + 1] as i8;
        let target = (bne_off as isize + 2) + rel as isize;
        assert_eq!(
            target, 28,
            "BNE at offset {} should target check_flag (28), got {}",
            bne_off, target
        );
    }
}

#[test]
fn hook_code_dma_path_clears_flag() {
    let code = build_shop_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    // do_dma at offset 38: STZ $1F60 = 9C 60 1F
    assert_eq!(code[38], 0x9C, "STZ abs");
    assert_eq!(code[39], SHOP_FLAG_WRAM as u8, "WRAM lo");
    assert_eq!(code[40], (SHOP_FLAG_WRAM >> 8) as u8, "WRAM hi");
    // Followed by JSL $009440
    assert_eq!(&code[41..45], &JSL_LZ_BYTES, "JSL $009440 (OBJ tile)");
}

#[test]
fn hook_code_dma_path_has_plb_plp_rtl() {
    let code = build_shop_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    let n = code.len();
    // DMA path ends with PLB(AB) PLP(28) RTL(6B) at the very end
    assert_eq!(code[n - 3], 0xAB, "PLB");
    assert_eq!(code[n - 2], 0x28, "PLP");
    assert_eq!(code[n - 1], 0x6B, "RTL");
}

#[test]
fn hook_code_contains_four_dma_triggers() {
    let code = build_shop_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    let trigger_count = code
        .windows(5)
        .filter(|w| w == &[0xA9, 0x40, 0x8D, 0x0B, 0x42])
        .count();
    assert_eq!(trigger_count, 4, "Expected 4 DMA triggers (Ch6): speech + 료 + cookie + 매완");
}

#[test]
fn hook_code_contains_speech_vram_addr() {
    let code = build_shop_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    let found = code
        .windows(6)
        .any(|w| w == &[0xA9, 0xA0, 0x50, 0x8D, 0x16, 0x21]);
    assert!(found, "Speech VRAM address $50A0 not found");
}

#[test]
fn hook_code_contains_soldout4_vram_addr() {
    let code = build_shop_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    let found = code
        .windows(6)
        .any(|w| w == &[0xA9, 0xE0, 0x50, 0x8D, 0x16, 0x21]);
    assert!(found, "Sold-out 료 VRAM address $50E0 not found");
}

#[test]
fn hook_code_contains_cookie_vram_addr() {
    let code = build_shop_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    let found = code
        .windows(6)
        .any(|w| w == &[0xA9, 0x70, 0x51, 0x8D, 0x16, 0x21]);
    assert!(found, "Cookie VRAM address $5170 not found");
}

#[test]
fn hook_code_contains_soldout23_vram_addr() {
    let code = build_shop_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    let found = code
        .windows(6)
        .any(|w| w == &[0xA9, 0xA0, 0x51, 0x8D, 0x16, 0x21]);
    assert!(found, "Sold-out 매완 VRAM address $51A0 not found");
}

#[test]
fn hook_code_sets_force_blank() {
    let code = build_shop_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    let found = code
        .windows(5)
        .any(|w| w == &[0xA9, 0x80, 0x8D, 0x00, 0x21]);
    assert!(found, "Force blank (STA $2100) not found");
}

#[test]
fn hook_code_uses_dma_ch6() {
    let code = build_shop_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    let found = code.windows(3).any(|w| w == &[0x8D, 0x60, 0x43]);
    assert!(found, "DMA Ch6 mode register ($4360) not found");
}

#[test]
fn hook_code_sets_source_bank() {
    let code = build_shop_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    let found = code
        .windows(5)
        .any(|w| w == &[0xA9, DATA_BANK, 0x8D, 0x64, 0x43]);
    assert!(found, "DMA source bank ${:02X} not found", DATA_BANK);
}

#[test]
fn hook_site_pc_matches_lorom() {
    assert_eq!(HOOK_SITE_PC, lorom_to_pc(0x02, 0x8ADA));
}

#[test]
fn data_does_not_overlap_equip_oam() {
    let equip_end = 0xCEB0u16;
    assert!(
        DATA_ADDR >= equip_end,
        "Shop data ${:04X} overlaps equip end ${:04X}",
        DATA_ADDR, equip_end
    );
}

#[test]
fn data_fits_in_bank() {
    let code = build_shop_hook_code(DATA_BANK, DATA_ADDR).unwrap();
    let total = TILE_DATA_SIZE + code.len();
    let end_addr = DATA_ADDR as usize + total;
    assert!(
        end_addr <= 0x10000,
        "Data+code overflows Bank ${:02X}: end=${:04X}",
        DATA_BANK, end_addr
    );
}

#[test]
fn all_vram_in_phase1_dma_range() {
    for &(vram, size, name) in &[
        (SPEECH_VRAM, SPEECH_DMA_SIZE, "Speech"),
        (SOLDOUT4_VRAM, SOLDOUT4_DMA_SIZE, "Sold-out 료"),
        (COOKIE_VRAM, COOKIE_DMA_SIZE, "Cookie"),
        (SOLDOUT23_VRAM, SOLDOUT23_DMA_SIZE, "Sold-out 매완"),
    ] {
        let end = vram + size / 2;
        assert!(
            vram >= 0x4800 && end <= 0x6000,
            "{} VRAM ${:04X}-${:04X} outside Phase 1 range ($4800-$5FFF)",
            name, vram, end,
        );
    }
}

#[test]
fn all_vram_not_overwritten_by_phase2() {
    let phase2_vram_start: u16 = 0x5800;
    for &(vram, size, name) in &[
        (SPEECH_VRAM, SPEECH_DMA_SIZE, "Speech"),
        (SOLDOUT4_VRAM, SOLDOUT4_DMA_SIZE, "Sold-out 료"),
        (COOKIE_VRAM, COOKIE_DMA_SIZE, "Cookie"),
        (SOLDOUT23_VRAM, SOLDOUT23_DMA_SIZE, "Sold-out 매완"),
    ] {
        let end = vram + size / 2;
        assert!(
            end <= phase2_vram_start,
            "{} VRAM ${:04X}-${:04X} overlaps Phase 2 DMA at ${:04X}",
            name, vram, end, phase2_vram_start,
        );
    }
}

#[test]
fn wram_flag_in_mirror_range() {
    assert!(
        SHOP_FLAG_WRAM < 0x2000,
        "WRAM flag ${:04X} outside mirror range ($0000-$1FFF)",
        SHOP_FLAG_WRAM,
    );
}

#[test]
fn soldout4_no_overlap_with_speech() {
    let speech_end = SPEECH_VRAM + SPEECH_DMA_SIZE / 2;
    assert!(
        SOLDOUT4_VRAM >= speech_end,
        "Sold-out 료 VRAM ${:04X} overlaps speech end ${:04X}",
        SOLDOUT4_VRAM, speech_end,
    );
}

#[test]
fn cookie_no_overlap_with_soldout4() {
    let soldout4_end = SOLDOUT4_VRAM + SOLDOUT4_DMA_SIZE / 2;
    assert!(
        COOKIE_VRAM >= soldout4_end,
        "Cookie VRAM ${:04X} overlaps sold-out 진 end ${:04X}",
        COOKIE_VRAM, soldout4_end,
    );
}

#[test]
fn soldout23_no_overlap_with_cookie() {
    let cookie_end = COOKIE_VRAM + COOKIE_DMA_SIZE / 2;
    assert!(
        SOLDOUT23_VRAM >= cookie_end,
        "Sold-out 매완 VRAM ${:04X} overlaps cookie end ${:04X}",
        SOLDOUT23_VRAM, cookie_end,
    );
}
