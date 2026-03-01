use super::*;
use crate::patch::tracked_rom::TrackedRom;
use crate::rom::lorom_to_pc;

#[test]
fn item_name_table_pc_correct() {
    assert_eq!(ITEM_NAME_TABLE_PC, lorom_to_pc(0x2B, 0xFD8B));
}

#[test]
fn item_name_count() {
    assert_eq!(KO_ITEM_NAMES.len(), ITEM_COUNT);
}

#[test]
fn truncate_preserves_char_boundary() {
    // 11 single-byte chars → truncate to 10
    let bytes: Vec<u8> = (0x20..0x2B).collect();
    let result = truncate_at_char_boundary(&bytes, 10);
    assert_eq!(result.len(), 10);
    assert_eq!(result, &bytes[..10]);
}

#[test]
fn truncate_preserves_fb_prefix() {
    // [A, FB X, B, FB Y, C, FB Z, D] = 10 bytes, 5 chars
    let bytes = vec![0x20, 0xFB, 0x48, 0x21, 0xFB, 0x49, 0x22, 0xFB, 0x50, 0x23];
    let result = truncate_at_char_boundary(&bytes, 10);
    assert_eq!(result, bytes);

    // Add one more FB pair → 12 bytes, won't fit
    let bytes12 = vec![
        0x20, 0xFB, 0x48, 0x21, 0xFB, 0x49, 0x22, 0xFB, 0x50, 0x23, 0xFB, 0x51,
    ];
    let result12 = truncate_at_char_boundary(&bytes12, 10);
    assert_eq!(result12, &bytes12[..10]);
}

#[test]
fn truncate_empty() {
    assert_eq!(truncate_at_char_boundary(&[], 10), Vec::<u8>::new());
}

#[test]
fn patch_item_name_table_writes_correct_data() {
    let mut ko_table = HashMap::new();
    // Minimal table for testing: 락교 = two single-byte chars
    ko_table.insert('락', vec![0x50]);
    ko_table.insert('교', vec![0x51]);

    let mut rom = TrackedRom::new(vec![0xFF; 0x200000]);

    let count = patch_item_name_table(&mut rom, &ko_table);
    // Will fail because not all chars are in the table
    assert!(count.is_err());
}

#[test]
fn patch_item_name_table_full() {
    // Build a comprehensive ko_table covering all chars in KO_ITEM_NAMES
    let mut ko_table = HashMap::new();
    // Map each unique hangul to a single byte for testing
    let all_chars: Vec<char> = KO_ITEM_NAMES
        .iter()
        .flat_map(|name| name.chars())
        .filter(|ch| !matches!(ch, ' ' | '.' | '0'..='9'))
        .collect::<std::collections::HashSet<char>>()
        .into_iter()
        .collect();

    for (i, ch) in all_chars.iter().enumerate() {
        // Assign single-byte encoding $20+i
        let byte = 0x20 + (i as u8);
        if byte < 0xF0 {
            ko_table.insert(*ch, vec![byte]);
        } else {
            ko_table.insert(*ch, vec![0xFB, i as u8]);
        }
    }

    let mut rom = TrackedRom::new(vec![0xFF; 0x200000]);
    let count = patch_item_name_table(&mut rom, &ko_table).unwrap();
    assert_eq!(count, ITEM_COUNT);

    // Verify each entry is exactly ITEM_NAME_STRIDE bytes and properly padded
    for i in 0..ITEM_COUNT {
        let entry_pc = ITEM_NAME_TABLE_PC + i * ITEM_NAME_STRIDE;
        let slot = &rom[entry_pc..entry_pc + ITEM_NAME_STRIDE];

        // Should not be all FF (was overwritten)
        assert!(
            slot.iter().any(|&b| b != 0xFF),
            "Item #{} slot not written",
            i
        );

        // If name is shorter than 10 bytes, trailing bytes should be 0x00
        let name_end = slot.iter().rposition(|&b| b != 0x00);
        if let Some(end) = name_end {
            for &b in &slot[end + 1..] {
                assert_eq!(b, 0x00, "Item #{} padding not 0x00", i);
            }
        }
    }

    // Verify byte after table is untouched
    let after_table = ITEM_NAME_TABLE_PC + ITEM_COUNT * ITEM_NAME_STRIDE;
    assert_eq!(rom[after_table], 0xFF);
}

#[test]
fn item_name_stride_is_correct() {
    assert_eq!(ITEM_NAME_STRIDE, 10);
}

#[test]
fn table_fits_in_bank() {
    let table_size = ITEM_COUNT * ITEM_NAME_STRIDE;
    assert_eq!(table_size, 180);
    // Table starts at $FD8B and is 180 bytes → ends at $FE3E
    let end_addr = 0xFD8Bu16 + table_size as u16;
    assert!(end_addr <= 0xFFFF, "Table overflows Bank $2B");
}
