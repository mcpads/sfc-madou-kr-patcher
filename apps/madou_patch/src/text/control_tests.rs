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
fn find_bank_backward_compat() {
    // find_bank returns the first match for each bank number
    let config = find_bank(0x01).unwrap();
    assert_eq!(config.label, "01");

    let config = find_bank(0x2B).unwrap();
    assert_eq!(config.label, "2B");

    assert!(find_bank(0xFF).is_none());
}
