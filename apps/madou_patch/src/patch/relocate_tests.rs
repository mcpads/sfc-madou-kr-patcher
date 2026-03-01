use super::*;

/// Test bank32 start address (matches legacy MENU_RESERVE_END).
const TEST_BANK32_START: u16 = 0xF100;

#[test]
fn freespace_alloc_basic() {
    let mut fs = FreeSpace::new(TEST_BANK32_START);
    let addr = fs.alloc(100).unwrap();
    assert_eq!(addr.bank, 0x32);
    assert_eq!(addr.addr, TEST_BANK32_START);
}

#[test]
fn freespace_alloc_sequential() {
    let mut fs = FreeSpace::new(TEST_BANK32_START);
    let a1 = fs.alloc(10).unwrap();
    let a2 = fs.alloc(20).unwrap();
    assert_eq!(a1.bank, 0x32);
    assert_eq!(a1.addr, TEST_BANK32_START);
    assert_eq!(a2.bank, 0x32);
    assert_eq!(a2.addr, TEST_BANK32_START + 10);
}

#[test]
fn freespace_alloc_overflows_to_next_region() {
    let mut fs = FreeSpace::new(TEST_BANK32_START);
    let region_size = (0xFFFF - TEST_BANK32_START + 1) as usize;
    let a1 = fs.alloc(region_size).unwrap();
    assert_eq!(a1.bank, 0x32);
    let a2 = fs.alloc(10).unwrap();
    assert_eq!(a2.bank, 0x33);
    assert_eq!(a2.addr, 0x8000);
}

#[test]
fn freespace_alloc_exhaustion() {
    let mut fs = FreeSpace::new(TEST_BANK32_START);
    for _ in 0..4 {
        let _ = fs.alloc(0x8000);
    }
    let result = fs.alloc(0x8000);
    assert!(result.is_err());
}

#[test]
fn freespace_within_bank_basic() {
    let mut fs = FreeSpace::new(TEST_BANK32_START);
    let addr = fs.alloc_within_bank(0x01, 50).unwrap();
    assert_eq!(addr.bank, 0x01);
    assert_eq!(addr.addr, 0xFD79);
}

#[test]
fn freespace_within_bank_sequential() {
    let mut fs = FreeSpace::new(TEST_BANK32_START);
    let a1 = fs.alloc_within_bank(0x01, 10).unwrap();
    let a2 = fs.alloc_within_bank(0x01, 20).unwrap();
    assert_eq!(a1.addr, 0xFD79);
    assert_eq!(a2.addr, 0xFD79 + 10);
}

#[test]
fn freespace_within_bank_wrong_bank() {
    let mut fs = FreeSpace::new(TEST_BANK32_START);
    let result = fs.alloc_within_bank(0x05, 10);
    assert!(result.is_err());
}

#[test]
fn freespace_within_bank_exhaustion() {
    let mut fs = FreeSpace::new(TEST_BANK32_START);
    let _ = fs.alloc_within_bank(0x01, 646).unwrap();
    let _ = fs.alloc_within_bank(0x01, 1).unwrap();
    let result = fs.alloc_within_bank(0x01, 2);
    assert!(result.is_err());
}

#[test]
fn load_2byte_targets_basic() {
    let mut rom = vec![0x00u8; 0x200];
    let table_pc = 0x100;
    rom[table_pc] = 0x00;
    rom[table_pc + 1] = 0x90;
    rom[table_pc + 2] = 0x00;
    rom[table_pc + 3] = 0xA0;

    let targets = load_2byte_targets(&rom, 0x01, &[(table_pc, 2)]);
    assert!(targets.contains(&SnesAddr::new(0x01, 0x9000)));
    assert!(targets.contains(&SnesAddr::new(0x01, 0xA000)));
    assert_eq!(targets.len(), 2);
}

#[test]
fn load_2byte_targets_filters_below_8000() {
    let mut rom = vec![0x00u8; 0x200];
    let table_pc = 0x100;
    rom[table_pc] = 0x00;
    rom[table_pc + 1] = 0x70;

    let targets = load_2byte_targets(&rom, 0x01, &[(table_pc, 1)]);
    assert!(targets.is_empty());
}

#[test]
fn count_controls_before_basic() {
    // [FC 01] [text] [F9] [text] [FE] [FC 02] [text]
    let bytes = [0xFC, 0x01, 0x13, 0xF9, 0x14, 0xFE, 0xFC, 0x02, 0x15];
    assert_eq!(count_controls_before(&bytes, 0), (0, 0)); // before first FC
    assert_eq!(count_controls_before(&bytes, 2), (1, 0)); // right after FC 01 → 0 text gap
    assert_eq!(count_controls_before(&bytes, 3), (1, 1)); // 1 text byte after FC
    assert_eq!(count_controls_before(&bytes, 4), (2, 0)); // right after F9 → 0 gap
    assert_eq!(count_controls_before(&bytes, 5), (2, 1)); // 1 text byte after F9
    assert_eq!(count_controls_before(&bytes, 6), (3, 0)); // right after FE → 0 gap
    assert_eq!(count_controls_before(&bytes, 8), (4, 0)); // right after FC 02 → 0 gap
}

#[test]
fn count_controls_fb_is_not_control() {
    // [FB 01] [FC 02] [FB 03] [F9]
    let bytes = [0xFB, 0x01, 0xFC, 0x02, 0xFB, 0x03, 0xF9];
    // FB is character combo, not counted as control code
    assert_eq!(count_controls_before(&bytes, 2), (0, 2)); // FB consumed 2 bytes text, 0 controls
    assert_eq!(count_controls_before(&bytes, 4), (1, 0)); // after FC → 0 gap
    assert_eq!(count_controls_before(&bytes, 7), (2, 0)); // right after F9 → 0 gap

    // [FC 01] [F9] [FB 02] [text]
    let bytes2 = [0xFC, 0x01, 0xF9, 0xFB, 0x02, 0x13];
    assert_eq!(count_controls_before(&bytes2, 3), (2, 0)); // right after F9
    assert_eq!(count_controls_before(&bytes2, 5), (2, 2)); // FB combo = 2 text bytes after F9
    assert_eq!(count_controls_before(&bytes2, 6), (2, 3)); // + 1 more text byte
}

#[test]
fn find_after_nth_control_basic() {
    // [FC 01] [text] [F9] [text] [FE] [FC 02]
    let bytes = [0xFC, 0x01, 0x13, 0xF9, 0x14, 0xFE, 0xFC, 0x02];
    assert_eq!(find_after_nth_control(&bytes, 1), Some(2)); // after FC 01
    assert_eq!(find_after_nth_control(&bytes, 2), Some(4)); // after F9
    assert_eq!(find_after_nth_control(&bytes, 3), Some(6)); // after FE
    assert_eq!(find_after_nth_control(&bytes, 4), Some(8)); // after FC 02
    assert_eq!(find_after_nth_control(&bytes, 5), None);
}

#[test]
fn find_after_nth_control_with_fb() {
    // [FA 01] [FC 02] [FB 03] [F9]
    let bytes = [0xFA, 0x01, 0xFC, 0x02, 0xFB, 0x03, 0xF9];
    assert_eq!(find_after_nth_control(&bytes, 1), Some(2)); // after FA 01
    assert_eq!(find_after_nth_control(&bytes, 2), Some(4)); // after FC 02
    assert_eq!(find_after_nth_control(&bytes, 3), Some(7)); // after F9 (FB skipped)
}
