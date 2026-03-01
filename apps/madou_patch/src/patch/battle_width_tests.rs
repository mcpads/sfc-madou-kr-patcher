use super::*;

#[test]
fn test_build_scan_hook_assembles() {
    let code = build_scan_hook();
    // Hook: scan (~90B) + screen-boundary clamping (~50B), not empty
    assert!(!code.is_empty(), "hook code should not be empty");
    assert!(
        code.len() < 200,
        "hook code too large: {} bytes",
        code.len()
    );
    // First byte should be PHX ($DA)
    assert_eq!(code[0], 0xDA, "hook should start with PHX");
    // Last byte should be RTL ($6B)
    assert_eq!(code[code.len() - 1], 0x6B, "hook should end with RTL");
}

#[test]
fn test_build_hook_site_patch_12_bytes() {
    let patch = build_hook_site_patch(0xFB00);
    // JSL $03:FB00 = 22 00 FB 03, then NOP ×8
    assert_eq!(patch.len(), 12);
    assert_eq!(patch[0], 0x22, "should start with JSL");
    assert_eq!(patch[1..4], [0x00, 0xFB, 0x03]);
    for i in 4..12 {
        assert_eq!(patch[i], 0xEA, "byte {} should be NOP", i);
    }
}

#[test]
fn test_hook_site_original_12_bytes() {
    assert_eq!(HOOK_SITE_ORIGINAL.len(), 12);
    // LDA $0001,Y + STA $0006,X + LDA $0003,Y + STA $0008,X
    assert_eq!(&HOOK_SITE_ORIGINAL[0..3], &[0xB9, 0x01, 0x00]);
    assert_eq!(&HOOK_SITE_ORIGINAL[3..6], &[0x9D, 0x06, 0x00]);
    assert_eq!(&HOOK_SITE_ORIGINAL[6..9], &[0xB9, 0x03, 0x00]);
    assert_eq!(&HOOK_SITE_ORIGINAL[9..12], &[0x9D, 0x08, 0x00]);
}

#[test]
fn test_hook_writes_both_slot6_and_slot8() {
    let code = build_scan_hook();
    // STA $0006,X (9D 06 00)
    let sta_slot6 = [0x9D, 0x06, 0x00];
    assert!(
        code.windows(3).any(|w| w == sta_slot6),
        "hook must write slot+$06"
    );
    // STA $0008,X (9D 08 00) — 2 paths (shift + no_shift)
    let sta_slot8 = [0x9D, 0x08, 0x00];
    let count = code.windows(3).filter(|w| *w == sta_slot8).count();
    assert!(
        count >= 2,
        "hook must write slot+$08 in both paths (found {})",
        count
    );
}

#[test]
fn test_hook_reads_display_params_from_script() {
    let code = build_scan_hook();
    // LDA $0003,Y (B9 03 00) for reading display_params (16-bit)
    let lda_y3 = [0xB9, 0x03, 0x00];
    assert!(
        code.windows(3).any(|w| w == lda_y3),
        "hook must read display_params from script"
    );
}

#[test]
fn test_hook_has_two_rtl() {
    let code = build_scan_hook();
    // 2 exit paths (shift + no_shift) each end with RTL ($6B)
    let rtl_count = code.iter().filter(|&&b| b == 0x6B).count();
    assert_eq!(
        rtl_count, 2,
        "hook must have exactly 2 RTL instructions (found {})",
        rtl_count
    );
}

#[test]
fn test_hook_no_asl_x4() {
    let code = build_scan_hook();
    // Old approach used ASL A ×4 (0A 0A 0A 0A) — should NOT be present
    let asl_x4 = [0x0A, 0x0A, 0x0A, 0x0A];
    assert!(
        !code.windows(4).any(|w| w == asl_x4),
        "hook must NOT use ASL×4 (old pixel-based approach)"
    );
}

#[test]
fn test_hook_no_width_x2_conversion() {
    let code = build_scan_hook();
    // Width is returned as character count. No ASL A (×2) on max_width before slot+$06.
    // The ×2 multiplication happens only for column calculation, not for width itself.
    // Verify: LDA dp$0E, ASL A pattern (A5 0E 0A) should NOT appear before STA $0006,X.
    // Find the STA $0006,X instruction position
    let sta_slot6 = [0x9D, 0x06, 0x00];
    if let Some(pos) = code.windows(3).position(|w| w == sta_slot6) {
        // Check the ~10 bytes before STA $0006,X for LDA dp$0E + ASL A
        let lda_max_w_asl = [0xA5, DP_MAX_W, 0x0A];
        let start = pos.saturating_sub(10);
        let preceding = &code[start..pos];
        assert!(
            !preceding.windows(3).any(|w| w == lda_max_w_asl),
            "width must NOT be multiplied by 2 before writing slot+$06"
        );
    }
}

#[test]
fn test_hook_fd_choice_not_terminator() {
    let code = build_scan_hook();
    // $FD (ControlCode::Choice) should be skipped, not treated as terminator.
    // The dispatch should have CMP #$FE (C9 FE) before the final BRA to done.
    let cmp_fe = [0xC9, 0xFE];
    assert!(
        code.windows(2).any(|w| w == cmp_fe),
        "hook must compare against $FE to distinguish $FD from terminators"
    );
}

#[test]
fn test_hook_screen_boundary_clamping() {
    let code = build_scan_hook();
    // Screen-boundary clamping: AND #$1F to extract column from display_params
    let and_1f = [0x29, 0x1F]; // AND #$1F (8-bit)
    assert!(
        code.windows(2).any(|w| w == and_1f),
        "hook must AND #$1F to extract tilemap column"
    );
    // CMP #(RIGHT_LIMIT+1) to check if right_edge exceeds limit
    let cmp_limit = [0xC9, RIGHT_LIMIT + 1];
    assert!(
        code.windows(2).any(|w| w == cmp_limit),
        "hook must compare right_edge against RIGHT_LIMIT+1"
    );
    // SBC #RIGHT_LIMIT to compute excess
    let sbc_limit = [0xE9, RIGHT_LIMIT];
    assert!(
        code.windows(2).any(|w| w == sbc_limit),
        "hook must SBC #RIGHT_LIMIT to compute excess"
    );
    // SBC dp$0D (16-bit) for dp_orig - excess
    let sbc_dp = [0xE5, DP_PTR + 2];
    assert!(
        code.windows(2).any(|w| w == sbc_dp),
        "hook must SBC dp$0D for 16-bit position adjustment"
    );
}

#[test]
fn test_hook_double_adc_for_width_x2_in_clamping() {
    let code = build_scan_hook();
    // Column calculation uses two consecutive ADC dp$0E to compute col + ko_width*2
    // ADC dp$0E = opcode $65, operand $0E
    let adc_dp_maxw = [0x65, DP_MAX_W];
    let positions: Vec<usize> = code
        .windows(2)
        .enumerate()
        .filter(|(_, w)| *w == adc_dp_maxw)
        .map(|(i, _)| i)
        .collect();
    assert!(
        positions.len() >= 2,
        "hook must have at least 2× ADC dp$0E for col + ko_width*2 (found {})",
        positions.len()
    );
    // The two ADCs should be consecutive (2 bytes apart)
    if positions.len() >= 2 {
        assert_eq!(
            positions[1] - positions[0],
            2,
            "the two ADC dp$0E should be consecutive"
        );
    }
}
