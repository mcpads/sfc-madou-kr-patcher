use super::*;

#[test]
fn tilemap_hook_assembles() {
    let code = build_tilemap_hook().unwrap();
    assert!(!code.is_empty());
    assert!(
        code.len() < 64,
        "tilemap hook too large: {} bytes",
        code.len()
    );
    // First bytes: CMP #$FB
    assert_eq!(code[0], 0xC9);
    assert_eq!(code[1], 0xFB);
}

#[test]
fn renderer_hook_assembles() {
    let code = build_renderer_hook().unwrap();
    assert!(!code.is_empty());
    assert!(
        code.len() < 80,
        "renderer hook too large: {} bytes",
        code.len()
    );
    // First byte: TAY
    assert_eq!(code[0], 0xA8);
}

#[test]
fn all_hooks_fit_within_reasonable_size() {
    let t = build_tilemap_hook().unwrap();
    let r = build_renderer_hook().unwrap();
    let cd = build_clear_and_dispatch().unwrap();
    let d = build_ingame_dispatch_hook(0x32_D480).unwrap(); // dummy addr
    let f = build_ingame_fa_handler().unwrap();
    let b = build_ingame_bank_check().unwrap();
    let b3d = build_bank03_dispatch_hook().unwrap();
    let b3b = build_bank03_bank_check().unwrap();
    let b0 = build_block0_clear_hook().unwrap();
    let total = t.len()
        + r.len()
        + cd.len()
        + d.len()
        + f.len()
        + b.len()
        + b3d.len()
        + b3b.len()
        + b0.len();
    // Total hooks must fit in <700 bytes (reasonable upper bound)
    assert!(
        total < 700,
        "hooks total ({} bytes) exceeds 700B limit",
        total,
    );
}

#[test]
fn tilemap_hook_contains_f1_f0_checks() {
    let code = build_tilemap_hook().unwrap();
    let has_f1 = code.windows(2).any(|w| w == [0xC9, 0xF1]);
    let has_f0 = code.windows(2).any(|w| w == [0xC9, 0xF0]);
    assert!(has_f1, "missing CMP #$F1");
    assert!(has_f0, "missing CMP #$F0");
}

#[test]
fn renderer_hook_contains_bank_switch() {
    let code = build_renderer_hook().unwrap();
    assert!(code.contains(&0x8B), "missing PHB");
    assert!(code.contains(&0xAB), "missing PLB");
    let has_bank = code.windows(2).any(|w| w == [0xA9, 0x32]);
    assert!(has_bank, "missing LDA #$32");
}

#[test]
fn ingame_dispatch_hook_assembles() {
    let code = build_ingame_dispatch_hook(0x32_D480).unwrap(); // dummy clear_and_dispatch addr
    assert!(!code.is_empty());
    assert!(
        code.len() < 80,
        "dispatch hook too large: {} bytes",
        code.len()
    );
    // Should start with CMP #$F0
    assert_eq!(code[0], 0xC9);
    assert_eq!(code[1], 0xF0);
    // Should contain CMP #$F1 (replaces FA)
    let has_f1 = code.windows(2).any(|w| w == [0xC9, 0xF1]);
    assert!(has_f1, "missing CMP #$F1");
    // Should contain CMP #$F8
    let has_f8 = code.windows(2).any(|w| w == [0xC9, 0xF8]);
    assert!(has_f8, "missing CMP #$F8");
    // Should contain FA/FB/FC filtering (no-clear path)
    let has_fa = code.windows(2).any(|w| w == [0xC9, 0xFA]);
    let has_fb = code.windows(2).any(|w| w == [0xC9, 0xFB]);
    let has_fc = code.windows(2).any(|w| w == [0xC9, 0xFC]);
    assert!(has_fa, "missing CMP #$FA");
    assert!(has_fb, "missing CMP #$FB");
    assert!(has_fc, "missing CMP #$FC");
    // Should contain JML to clear_and_dispatch (0x32D480 = 5C 80 D4 32)
    let has_clear_jml = code.windows(4).any(|w| w == [0x5C, 0x80, 0xD4, 0x32]);
    assert!(has_clear_jml, "missing JML to clear_and_dispatch");
}

#[test]
fn clear_and_dispatch_assembles() {
    let code = build_clear_and_dispatch().unwrap();
    assert!(!code.is_empty());
    assert!(
        code.len() < 200,
        "clear_and_dispatch too large: {} bytes",
        code.len()
    );
    // First byte: PHA ($48)
    assert_eq!(code[0], 0x48, "should start with PHA");
    // Contains PHX ($DA) and PLX ($FA) for X preservation
    assert!(code.contains(&0xDA), "missing PHX");
    assert!(code.contains(&0xFA), "missing PLX");
    // Contains LDA $0007,X ($BD 07 00) — read slot counter
    let has_slot_read = code.windows(3).any(|w| w == [0xBD, 0x07, 0x00]);
    assert!(has_slot_read, "missing LDA $0007,X");
    // Contains STA $0007,X ($9D 07 00) — slot rounding write-back
    let has_slot_write = code.windows(3).any(|w| w == [0x9D, 0x07, 0x00]);
    assert!(has_slot_write, "missing STA $0007,X for slot rounding");
    // Contains JSL $008BF9 (DMA scheduler)
    let has_jsl = code.windows(4).any(|w| w == [0x22, 0xF9, 0x8B, 0x00]);
    assert!(has_jsl, "missing JSL $008BF9");
    // Contains boundary check CMP #$0A, CMP #$14, CMP #$1E
    let has_0a = code.windows(2).any(|w| w == [0xC9, 0x0A]);
    let has_14 = code.windows(2).any(|w| w == [0xC9, 0x14]);
    let has_1e = code.windows(2).any(|w| w == [0xC9, 0x1E]);
    assert!(has_0a, "missing CMP #$0A boundary check");
    assert!(has_14, "missing CMP #$14 boundary check");
    assert!(has_1e, "missing CMP #$1E boundary check");
    // Contains VRAM base $6400 in ADC
    let has_vram_base = code.windows(3).any(|w| w == [0x69, 0x00, 0x64]);
    assert!(has_vram_base, "missing ADC #$6400");
    // Contains game zero buffer $8E3E
    let has_zero_buf = code.windows(3).any(|w| w == [0xA9, 0x3E, 0x8E]);
    assert!(has_zero_buf, "missing LDA #$8E3E");
    // Contains JML to INGAME_CONTROL_DISPATCH ($00:$CCE7)
    let has_dispatch_jml = code.windows(4).any(|w| w == [0x5C, 0xE7, 0xCC, 0x00]);
    assert!(has_dispatch_jml, "missing JML $00:$CCE7");
}

#[test]
fn ingame_fa_handler_assembles() {
    let code = build_ingame_fa_handler().unwrap();
    assert!(!code.is_empty());
    assert!(
        code.len() < 30,
        "FA handler too large: {} bytes",
        code.len()
    );
    // Should contain LDA #$32 (bank override)
    let has_bank = code.windows(2).any(|w| w == [0xA9, 0x32]);
    assert!(has_bank, "missing LDA #$32");
    // Should contain LDA dp$1E
    let has_lda_1e = code.windows(2).any(|w| w == [0xA5, 0x1E]);
    assert!(has_lda_1e, "missing LDA dp$1E");
}

#[test]
fn ingame_bank_check_assembles() {
    let code = build_ingame_bank_check().unwrap();
    assert!(!code.is_empty());
    assert!(
        code.len() < 30,
        "bank check too large: {} bytes",
        code.len()
    );
    // Should contain CMP #$32 (exact match check)
    let has_cmp_32 = code.windows(2).any(|w| w == [0xC9, 0x32]);
    assert!(has_cmp_32, "missing CMP #$32");
    // Should contain LDA #$0F (default bank)
    let has_0f = code.windows(2).any(|w| w == [0xA9, 0x0F]);
    assert!(has_0f, "missing LDA #$0F default");
    // Should contain STZ $1D70 (clear flag)
    let has_stz = code.windows(3).any(|w| w == [0x9C, 0x70, 0x1D]);
    assert!(has_stz, "missing STZ $1D70");
}

#[test]
fn bank03_dispatch_hook_assembles() {
    let code = build_bank03_dispatch_hook().unwrap();
    assert!(!code.is_empty());
    assert!(
        code.len() < 80,
        "bank03 dispatch hook too large: {} bytes",
        code.len()
    );
    // Should start with CMP #$FC
    assert_eq!(code[0], 0xC9);
    assert_eq!(code[1], 0xFC);
    // Contains CMP #$F1 and CMP #$F0
    let has_f1 = code.windows(2).any(|w| w == [0xC9, 0xF1]);
    let has_f0 = code.windows(2).any(|w| w == [0xC9, 0xF0]);
    assert!(has_f1, "missing CMP #$F1");
    assert!(has_f0, "missing CMP #$F0");
}

#[test]
fn bank03_handler_contains_bank_setup() {
    let code = build_bank03_dispatch_hook().unwrap();
    // Contains LDA dp$1E (index byte read)
    let has_dp1e = code.windows(2).any(|w| w == [0xA5, 0x1E]);
    assert!(has_dp1e, "missing LDA dp$1E");
    // Contains LDA #$32 (bank override)
    let has_bank = code.windows(2).any(|w| w == [0xA9, 0x32]);
    assert!(has_bank, "missing LDA #$32");
    // Contains STA $1D70 (bank flag)
    let has_sta = code.windows(3).any(|w| w == [0x8D, 0x70, 0x1D]);
    assert!(has_sta, "missing STA $1D70");
    // Contains ADC #$8000 (F1 base) and ADC #$C000 (F0 base)
    let has_f1_base = code.windows(3).any(|w| w == [0x69, 0x00, 0x80]);
    let has_f0_base = code.windows(3).any(|w| w == [0x69, 0x00, 0xC0]);
    assert!(has_f1_base, "missing ADC #$8000 (F1 base)");
    assert!(has_f0_base, "missing ADC #$C000 (F0 base)");
}

#[test]
fn block0_clear_hook_assembles() {
    let code = build_block0_clear_hook().unwrap();
    assert!(!code.is_empty());
    assert!(
        code.len() < 70,
        "block0 clear hook too large: {} bytes",
        code.len()
    );
    // Contains JSL $008BF9 (DMA scheduler)
    let has_jsl = code.windows(4).any(|w| w == [0x22, 0xF9, 0x8B, 0x00]);
    assert!(has_jsl, "missing JSL $008BF9");
    // Contains PHX ($DA) and PLX ($FA) for X preservation
    assert!(code.contains(&0xDA), "missing PHX");
    assert!(code.contains(&0xFA), "missing PLX");
    // Contains VRAM start address $7800
    let has_vram = code.windows(3).any(|w| w == [0xA9, 0x00, 0x78]);
    assert!(has_vram, "missing LDA #$7800");
    // Contains game zero buffer address $8E3E
    let has_zero_buf = code.windows(3).any(|w| w == [0xA9, 0x3E, 0x8E]);
    assert!(has_zero_buf, "missing LDA #$8E3E");
    // Contains source bank $03
    let has_bank03 = code.windows(2).any(|w| w == [0xA9, 0x03]);
    assert!(has_bank03, "missing LDA #$03");
    // Should NOT contain displaced STZ $1188 (no longer hooking $CC21)
    let has_stz_1188 = code.windows(3).any(|w| w == [0x9C, 0x88, 0x11]);
    assert!(!has_stz_1188, "should not contain STZ $1188");
}

#[test]
fn bank03_bank_check_assembles() {
    let code = build_bank03_bank_check().unwrap();
    assert!(!code.is_empty());
    assert!(
        code.len() < 30,
        "bank03 bank check too large: {} bytes",
        code.len()
    );
    // Contains CMP #$32
    let has_cmp = code.windows(2).any(|w| w == [0xC9, 0x32]);
    assert!(has_cmp, "missing CMP #$32");
    // Contains LDA #$0F (default bank)
    let has_0f = code.windows(2).any(|w| w == [0xA9, 0x0F]);
    assert!(has_0f, "missing LDA #$0F");
    // Contains STZ $1D70 (clear flag)
    let has_stz = code.windows(3).any(|w| w == [0x9C, 0x70, 0x1D]);
    assert!(has_stz, "missing STZ $1D70");
}
