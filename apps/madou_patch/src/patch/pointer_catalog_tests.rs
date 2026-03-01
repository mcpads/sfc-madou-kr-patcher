use super::*;

/// Maximum valid PC offset for a 2MB SNES ROM.
const MAX_PC_2MB: usize = 0x200000;

/// All catalog entries must be within 2MB ROM range.
#[test]
fn all_catalog_pcs_within_rom_range() {
    let banks = [0x01u8, 0x1D, 0x2A, 0x2B, 0x2D, 0x34];
    for &bank in &banks {
        let pcs = get_pointer_pcs(bank);
        for &pc in &pcs {
            assert!(
                pc < MAX_PC_2MB,
                "Bank ${:02X}: PC offset ${:06X} exceeds 2MB ROM",
                bank,
                pc
            );
        }
    }
}

/// Each catalog array must have the documented count.
#[test]
fn catalog_counts_match_docs() {
    assert_eq!(BANK_02_TO_01.len(), 260);
    assert_eq!(BANK_13_TO_1D.len(), 312);
    assert_eq!(BANK_1E_TO_1D.len(), 58);
    assert_eq!(BANK_2A_TO_2A.len(), 11);
    assert_eq!(BANK_2C_TO_2A.len(), 60);
    assert_eq!(BANK_2C_TO_2B.len(), 220);
    assert_eq!(BANK_2D_TO_2A.len(), 6);
    assert_eq!(BANK_2D_TO_2B.len(), 53);
    assert_eq!(BANK_2D_TO_2D.len(), 0);
    assert_eq!(BANK_2E_TO_2A.len(), 56);
    assert_eq!(BANK_2E_TO_2B.len(), 70);
    assert_eq!(BANK_2F_TO_2A.len(), 8);
    assert_eq!(BANK_2F_TO_2B.len(), 151);
    assert_eq!(BANK_30_TO_2A.len(), 42);
    assert_eq!(BANK_30_TO_2B.len(), 191);
    assert_eq!(BANK_31_TO_2A.len(), 14);
    assert_eq!(BANK_31_TO_2D.len(), 49);
    assert_eq!(BANK_34_TO_01.len(), 17);
    assert_eq!(BANK_34_TO_34.len(), 1);
    assert_eq!(BANK_3C_TO_1D.len(), 3);
    assert_eq!(BANK_1D_TO_1D.len(), 36);
}

/// get_pointer_pcs should concatenate all sources for a given bank.
#[test]
fn get_pointer_pcs_bank_01_total() {
    let pcs = get_pointer_pcs(0x01);
    assert_eq!(pcs.len(), BANK_02_TO_01.len() + BANK_34_TO_01.len());
}

#[test]
fn get_pointer_pcs_bank_1d_total() {
    let pcs = get_pointer_pcs(0x1D);
    assert_eq!(
        pcs.len(),
        BANK_13_TO_1D.len() + BANK_1E_TO_1D.len() + BANK_3C_TO_1D.len() + BANK_1D_TO_1D.len()
    );
}

#[test]
fn get_pointer_pcs_bank_2a_total() {
    let pcs = get_pointer_pcs(0x2A);
    let expected = BANK_2A_TO_2A.len()
        + BANK_2C_TO_2A.len()
        + BANK_2D_TO_2A.len()
        + BANK_2E_TO_2A.len()
        + BANK_2F_TO_2A.len()
        + BANK_30_TO_2A.len()
        + BANK_31_TO_2A.len();
    assert_eq!(pcs.len(), expected);
}

#[test]
fn get_pointer_pcs_bank_2b_total() {
    let pcs = get_pointer_pcs(0x2B);
    let expected = BANK_2A_TO_2B.len()
        + BANK_2C_TO_2B.len()
        + BANK_2D_TO_2B.len()
        + BANK_2E_TO_2B.len()
        + BANK_2F_TO_2B.len()
        + BANK_30_TO_2B.len();
    assert_eq!(pcs.len(), expected);
}

#[test]
fn get_pointer_pcs_bank_2d_total() {
    let pcs = get_pointer_pcs(0x2D);
    assert_eq!(
        pcs.len(),
        BANK_2C_TO_2D.len() + BANK_2D_TO_2D.len() + BANK_2E_TO_2D.len() + BANK_31_TO_2D.len()
    );
}

#[test]
fn get_pointer_pcs_unknown_bank_empty() {
    assert!(get_pointer_pcs(0x00).is_empty());
    assert!(get_pointer_pcs(0xFF).is_empty());
    assert!(get_pointer_pcs(0x10).is_empty());
}

/// No duplicate PC offsets within a single catalog array.
#[test]
fn no_duplicates_in_individual_catalogs() {
    let catalogs: &[(&str, &[usize])] = &[
        ("BANK_02_TO_01", BANK_02_TO_01),
        ("BANK_13_TO_1D", BANK_13_TO_1D),
        ("BANK_1E_TO_1D", BANK_1E_TO_1D),
        ("BANK_2A_TO_2A", BANK_2A_TO_2A),
        ("BANK_2C_TO_2A", BANK_2C_TO_2A),
        ("BANK_2C_TO_2B", BANK_2C_TO_2B),
        ("BANK_2D_TO_2A", BANK_2D_TO_2A),
        ("BANK_2D_TO_2B", BANK_2D_TO_2B),
        ("BANK_2D_TO_2D", BANK_2D_TO_2D),
        ("BANK_2E_TO_2A", BANK_2E_TO_2A),
        ("BANK_2E_TO_2B", BANK_2E_TO_2B),
        ("BANK_2F_TO_2A", BANK_2F_TO_2A),
        ("BANK_2F_TO_2B", BANK_2F_TO_2B),
        ("BANK_30_TO_2A", BANK_30_TO_2A),
        ("BANK_30_TO_2B", BANK_30_TO_2B),
        ("BANK_31_TO_2A", BANK_31_TO_2A),
        ("BANK_31_TO_2D", BANK_31_TO_2D),
        ("BANK_34_TO_01", BANK_34_TO_01),
        ("BANK_34_TO_34", BANK_34_TO_34),
        ("BANK_3C_TO_1D", BANK_3C_TO_1D),
        ("BANK_1D_TO_1D", BANK_1D_TO_1D),
    ];

    for (name, catalog) in catalogs {
        let mut sorted = catalog.to_vec();
        sorted.sort();
        for w in sorted.windows(2) {
            assert!(w[0] != w[1], "{}: duplicate PC offset ${:06X}", name, w[0]);
        }
    }
}

/// Catalog entries should be sorted (ascending).
#[test]
fn catalog_entries_are_sorted() {
    let catalogs: &[(&str, &[usize])] = &[
        ("BANK_02_TO_01", BANK_02_TO_01),
        ("BANK_13_TO_1D", BANK_13_TO_1D),
        ("BANK_1E_TO_1D", BANK_1E_TO_1D),
        ("BANK_2A_TO_2A", BANK_2A_TO_2A),
        ("BANK_2C_TO_2A", BANK_2C_TO_2A),
        ("BANK_2C_TO_2B", BANK_2C_TO_2B),
        ("BANK_2D_TO_2A", BANK_2D_TO_2A),
        ("BANK_2D_TO_2B", BANK_2D_TO_2B),
        ("BANK_2D_TO_2D", BANK_2D_TO_2D),
        ("BANK_2E_TO_2A", BANK_2E_TO_2A),
        ("BANK_2E_TO_2B", BANK_2E_TO_2B),
        ("BANK_2F_TO_2A", BANK_2F_TO_2A),
        ("BANK_2F_TO_2B", BANK_2F_TO_2B),
        ("BANK_30_TO_2A", BANK_30_TO_2A),
        ("BANK_30_TO_2B", BANK_30_TO_2B),
        ("BANK_31_TO_2A", BANK_31_TO_2A),
        ("BANK_31_TO_2D", BANK_31_TO_2D),
        ("BANK_34_TO_01", BANK_34_TO_01),
        ("BANK_34_TO_34", BANK_34_TO_34),
        ("BANK_3C_TO_1D", BANK_3C_TO_1D),
        ("BANK_1D_TO_1D", BANK_1D_TO_1D),
    ];

    for (name, catalog) in catalogs {
        for w in catalog.windows(2) {
            assert!(
                w[0] < w[1],
                "{}: not sorted at ${:06X} >= ${:06X}",
                name,
                w[0],
                w[1]
            );
        }
    }
}

/// PC offsets must allow reading 3 bytes without exceeding ROM.
#[test]
fn catalog_pcs_allow_3byte_read() {
    let banks = [0x01u8, 0x1D, 0x2A, 0x2B, 0x2D, 0x34];
    for &bank in &banks {
        let pcs = get_pointer_pcs(bank);
        for &pc in &pcs {
            assert!(
                pc + 3 <= MAX_PC_2MB,
                "Bank ${:02X}: PC ${:06X} too close to ROM end for 3-byte read",
                bank,
                pc
            );
        }
    }
}

/// 2-byte table definitions have reasonable values.
#[test]
fn bank_01_2byte_table_valid() {
    let tables = get_2byte_tables(0x01);
    assert_eq!(tables.len(), 2);
    // AE21 table (map names)
    assert_eq!(tables[0].0, 0xAE21);
    assert_eq!(tables[0].1, 16);
    assert!(tables[0].0 + tables[0].1 * 2 < MAX_PC_2MB);
    // B37E table (save menu)
    assert_eq!(tables[1].0, 0xB37E);
    assert_eq!(tables[1].1, 65);
    assert!(tables[1].0 + tables[1].1 * 2 < MAX_PC_2MB);
}

#[test]
fn bank_03_2byte_table_valid() {
    let tables = get_2byte_tables(0x03);
    assert_eq!(tables.len(), 1);
    let (table_pc, count) = tables[0];
    assert_eq!(table_pc, 0x01CFC2);
    assert_eq!(count, 49);
    assert!(table_pc + count * 2 < MAX_PC_2MB);
}

#[test]
fn unknown_bank_2byte_table_empty() {
    assert!(get_2byte_tables(0x00).is_empty());
    assert!(get_2byte_tables(0x02).is_empty());
    assert!(get_2byte_tables(0xFF).is_empty());
}
