use super::*;

#[test]
fn top_size_patch_length_matches_original() {
    assert_eq!(TOP_SIZE_ORIGINAL.len(), top_size_patch().len());
}

#[test]
fn bottom_size_patch_length_matches_original() {
    assert_eq!(BOTTOM_SIZE_ORIGINAL.len(), bottom_size_patch().len());
}

#[test]
fn top_size_patch_produces_correct_bytes() {
    let patch = top_size_patch();
    // LDA #$28
    assert_eq!(patch[0], 0xA9);
    assert_eq!(patch[1], FULL_LINE_SIZE);
    // PHA
    assert_eq!(patch[6], 0x48);
    // STA dp$10
    assert_eq!(patch[7], 0x85);
    assert_eq!(patch[8], 0x10);
}

#[test]
fn bottom_size_patch_preserves_pla_for_stack_balance() {
    let patch = bottom_size_patch();
    // PLA must be first (stack balance)
    assert_eq!(patch[0], 0x68);
    // LDA #$28
    assert_eq!(patch[1], 0xA9);
    assert_eq!(patch[2], FULL_LINE_SIZE);
    // PHA
    assert_eq!(patch[5], 0x48);
    // STA dp$10
    assert_eq!(patch[6], 0x85);
    assert_eq!(patch[7], 0x10);
}

#[test]
fn full_line_size_covers_10_characters() {
    // 10 chars × 2 tiles/char × 2 bytes/entry = 40 = $28
    assert_eq!(FULL_LINE_SIZE, 10 * 2 * 2);
}
