use super::*;

#[test]
fn known_banks_count() {
    assert_eq!(KNOWN_BANKS.len(), 12);
}

#[test]
fn labels_are_unique() {
    let mut labels: Vec<&str> = KNOWN_BANKS.iter().map(|b| b.label).collect();
    labels.sort();
    labels.dedup();
    assert_eq!(labels.len(), KNOWN_BANKS.len());
}

#[test]
fn find_by_label_works() {
    let config = find_by_label("01_monster").unwrap();
    assert_eq!(config.bank, 0x01);
    assert_eq!(config.start_addr, 0x86DE);

    let config = find_by_label("2B").unwrap();
    assert_eq!(config.bank, 0x2B);

    assert!(find_by_label("nonexistent").is_none());
}

#[test]
fn find_banks_by_number_returns_all() {
    let bank01 = find_banks_by_number(0x01);
    assert_eq!(bank01.len(), 4);
    for b in &bank01 {
        assert_eq!(b.bank, 0x01);
    }
}

#[test]
fn diary_range_includes_final_character_and_terminator() {
    let config = find_by_label("03").unwrap();

    assert_eq!(config.end_addr, 0xDA71);
}

#[test]
fn find_bank_backward_compat() {
    // find_bank returns the first match for each bank number
    let config = find_bank(0x01).unwrap();
    assert_eq!(config.label, "01");

    let config = find_bank(0x2B).unwrap();
    assert_eq!(config.label, "2B");

    assert!(find_bank(0xFF).is_none());
}

#[test]
fn menu_descriptions_use_fixed_two_row_profile() {
    let config = find_by_label("01").unwrap();
    let profile = text_box_profile(config, MENU_COMMAND_DESCRIPTION_START);

    assert_eq!(profile.max_width_tiles, 20);
    assert_eq!(profile.max_lines, 2);
    assert_eq!(profile.wrap_mode, LineWrapMode::FixedRows);

    let last_profile = text_box_profile(config, MENU_COMMAND_DESCRIPTION_END - 1);
    assert_eq!(last_profile, profile);
}

#[test]
fn diary_uses_fixed_fifteen_cell_rows() {
    let config = find_by_label("03").unwrap();
    let profile = text_box_profile(config, DIARY_TEXT_START);

    assert_eq!(profile.max_width_tiles, 30);
    assert_eq!(profile.max_lines, 5);
    assert_eq!(profile.wrap_mode, LineWrapMode::FixedRows);

    let last_profile = text_box_profile(config, DIARY_TEXT_END - 1);
    assert_eq!(last_profile, profile);
}

#[test]
fn adjacent_bank_01_text_keeps_automatic_wrapping() {
    let config = find_by_label("01").unwrap();

    assert_eq!(
        text_box_profile(config, MENU_COMMAND_DESCRIPTION_START - 1).wrap_mode,
        LineWrapMode::Automatic
    );
    assert_eq!(
        text_box_profile(config, MENU_COMMAND_DESCRIPTION_END).wrap_mode,
        LineWrapMode::Automatic
    );
}
