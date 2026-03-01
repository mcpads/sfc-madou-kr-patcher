use super::*;

#[test]
fn noise_filter_pattern1() {
    assert!(is_data_table_entry(&[0x01, 0x3E, 0x00, 0x00]));
}

#[test]
fn noise_filter_pattern2() {
    assert!(is_data_table_entry(&[0x7F, 0x00, 0x00, 0x00]));
}

#[test]
fn noise_filter_long_blob() {
    let long = vec![0x30; 501];
    assert!(is_data_table_entry(&long));
}

#[test]
fn noise_filter_normal_text() {
    // Normal JP text should not be filtered
    let text = [0xFC, 0x00, 0x2E, 0x30, 0x32, 0xF9, 0x2E, 0xFF];
    assert!(!is_data_table_entry(&text));
}

#[test]
fn noise_filter_short_data() {
    // Less than 4 bytes should never be filtered
    assert!(!is_data_table_entry(&[0x01, 0x3E, 0x00]));
}

#[test]
fn noise_filter_pattern4_7f_long_no_fc() {
    // Starts with 7F, no FC, > 20 bytes
    let mut data = vec![0x7F];
    data.extend_from_slice(&[0x30; 25]);
    assert!(is_data_table_entry(&data));
}

#[test]
fn noise_filter_high_space_ratio() {
    // No FC, > 10 bytes, > 35% spaces/digits
    let mut data = vec![0x00; 8]; // 8 spaces
    data.extend_from_slice(&[0x2E; 4]); // 4 text bytes
                                        // total=12, spaces=8, 8/12=66% > 35%
    assert!(is_data_table_entry(&data));
}

#[test]
fn extract_strings_ff_terminated() {
    // Create a ROM with known text at specific location
    let bank = 0x01;
    let start_addr = 0x8000u16;
    let pc_start = crate::rom::lorom_to_pc(bank, start_addr);
    let mut rom = vec![0xFF; pc_start + 100];

    // Place a valid text string: あいう + FF
    rom[pc_start] = 0x2E; // あ
    rom[pc_start + 1] = 0x30; // い
    rom[pc_start + 2] = 0x32; // う
    rom[pc_start + 3] = 0xFF; // terminator

    let strings = extract_strings(&rom, bank, start_addr, start_addr + 50, false, false);
    assert!(!strings.is_empty());
    assert_eq!(strings[0].snes_addr, start_addr);
    assert_eq!(strings[0].data, vec![0x2E, 0x30, 0x32, 0xFF]);
}

#[test]
fn extract_strings_skips_short() {
    let bank = 0x01;
    let start_addr = 0x8000u16;
    let pc_start = crate::rom::lorom_to_pc(bank, start_addr);
    let mut rom = vec![0xFF; pc_start + 100];

    // Only 2 bytes (below min 3)
    rom[pc_start] = 0x2E;
    rom[pc_start + 1] = 0xFF;

    let strings = extract_strings(&rom, bank, start_addr, start_addr + 50, false, false);
    assert!(strings.is_empty());
}

#[test]
fn extract_strings_fc_split_mode() {
    let bank = 0x01;
    let start_addr = 0x8000u16;
    let pc_start = crate::rom::lorom_to_pc(bank, start_addr);
    let mut rom = vec![0xFF; pc_start + 100];

    // FC-split text: FC 00 + あいうえ + FF
    rom[pc_start] = 0xFC;
    rom[pc_start + 1] = 0x00;
    rom[pc_start + 2] = 0x2E;
    rom[pc_start + 3] = 0x30;
    rom[pc_start + 4] = 0x32;
    rom[pc_start + 5] = 0x34; // え
    rom[pc_start + 6] = 0xFF;

    let strings = extract_strings(&rom, bank, start_addr, start_addr + 50, true, false);
    assert!(!strings.is_empty());
}

#[test]
fn extract_strings_empty_range() {
    let rom = vec![0xFF; 100];
    let strings = extract_strings(&rom, 0x01, 0x8000, 0x8000, false, false);
    assert!(strings.is_empty());
}

#[test]
fn extract_strings_out_of_bounds() {
    let rom = vec![0xFF; 100];
    let strings = extract_strings(&rom, 0x01, 0x8000, 0xFFFF, false, false);
    assert!(strings.is_empty());
}

#[test]
fn extract_strings_with_noise_filter() {
    let bank = 0x01;
    let start_addr = 0x8000u16;
    let pc_start = crate::rom::lorom_to_pc(bank, start_addr);
    let mut rom = vec![0xFF; pc_start + 600];

    // Place a very long blob (>500 bytes) — should be noise-filtered in FC mode
    for i in 0..505 {
        rom[pc_start + i] = 0x2E; // fill with valid text bytes
    }
    rom[pc_start + 505] = 0xFF;
    // Add FC so it passes the text ratio filter
    rom[pc_start] = 0xFC;
    rom[pc_start + 1] = 0x00;

    let strings = extract_strings(&rom, bank, start_addr, start_addr + 510, true, true);
    // The blob is > 500 bytes so noise filter should reject it
    assert!(strings.is_empty());
}
