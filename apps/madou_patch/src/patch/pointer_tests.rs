use super::*;
use crate::patch::tracked_rom::TrackedRom;

fn make_rom(size: usize) -> Vec<u8> {
    vec![0x00; size]
}

#[test]
fn scan_pointers_finds_valid_pointer() {
    let mut rom = make_rom(0x10000);
    let pc = lorom_to_pc(0x01, 0x8000);
    rom[pc] = 0x00;
    rom[pc + 1] = 0x90;
    rom[pc + 2] = 0x01;
    let entries = scan_pointers(&rom, 0x01, 0x8000, 0x8010, Some(0x01));
    assert!(entries
        .iter()
        .any(|e| e.target == SnesAddr::new(0x01, 0x9000)));
}

#[test]
fn scan_pointers_filters_by_target_bank() {
    let mut rom = make_rom(0x10000);
    let pc = lorom_to_pc(0x01, 0x8000);
    rom[pc] = 0x00;
    rom[pc + 1] = 0x90;
    rom[pc + 2] = 0x02;
    let entries = scan_pointers(&rom, 0x01, 0x8000, 0x8010, Some(0x01));
    assert!(!entries
        .iter()
        .any(|e| e.pc_offset == pc && e.target.bank == 0x01));
}

#[test]
fn scan_pointers_no_target_filter_accepts_any_bank() {
    let mut rom = make_rom(0x10000);
    let pc = lorom_to_pc(0x01, 0x8000);
    rom[pc] = 0x00;
    rom[pc + 1] = 0x90;
    rom[pc + 2] = 0x05;
    let entries = scan_pointers(&rom, 0x01, 0x8000, 0x8010, None);
    assert!(entries
        .iter()
        .any(|e| e.target == SnesAddr::new(0x05, 0x9000)));
}

#[test]
fn scan_pointers_rejects_addr_below_8000() {
    let mut rom = make_rom(0x10000);
    let pc = lorom_to_pc(0x01, 0x8000);
    rom[pc] = 0x00;
    rom[pc + 1] = 0x70;
    rom[pc + 2] = 0x01;
    let entries = scan_pointers(&rom, 0x01, 0x8000, 0x8010, None);
    assert!(entries.iter().all(|e| e.target.addr >= 0x8000));
}

#[test]
fn scan_pointers_empty_on_short_rom() {
    let rom = make_rom(10);
    let entries = scan_pointers(&rom, 0x01, 0x8000, 0x8010, None);
    assert!(entries.is_empty());
}

#[test]
fn rewrite_pointers_redirects_matching_entry() {
    let mut data = make_rom(0x10000);
    let pc = 0x100;
    data[pc] = 0x00;
    data[pc + 1] = 0x90;
    data[pc + 2] = 0x01;
    let mut rom = TrackedRom::new(data);

    let entries = vec![PointerEntry {
        pc_offset: pc,
        target: SnesAddr::new(0x01, 0x9000),
    }];
    let redirects = vec![(SnesAddr::new(0x01, 0x9000), SnesAddr::new(0x32, 0xD600))];

    let count = rewrite_pointers(&mut rom, &entries, &redirects);
    assert_eq!(count, 1);
    assert_eq!(rom[pc], 0x00);
    assert_eq!(rom[pc + 1], 0xD6);
    assert_eq!(rom[pc + 2], 0x32);
}

#[test]
fn rewrite_pointers_skips_non_matching() {
    let mut data = make_rom(0x10000);
    let pc = 0x100;
    data[pc] = 0x00;
    data[pc + 1] = 0x90;
    data[pc + 2] = 0x01;
    let mut rom = TrackedRom::new(data);

    let entries = vec![PointerEntry {
        pc_offset: pc,
        target: SnesAddr::new(0x01, 0x9000),
    }];
    let redirects = vec![(SnesAddr::new(0x01, 0xA000), SnesAddr::new(0x32, 0xD600))];

    let count = rewrite_pointers(&mut rom, &entries, &redirects);
    assert_eq!(count, 0);
    assert_eq!(rom[pc + 2], 0x01);
}

#[test]
fn rewrite_2byte_exact_match() {
    let mut data = make_rom(0x20000);
    let table_pc = 0x100;
    data[table_pc] = 0x00;
    data[table_pc + 1] = 0x90;
    data[table_pc + 2] = 0x00;
    data[table_pc + 3] = 0xA0;
    let mut rom = TrackedRom::new(data);

    let redirects = vec![(SnesAddr::new(0x01, 0xA000), SnesAddr::new(0x01, 0xFE00))];

    let count = rewrite_2byte_pointer_table(&mut rom, table_pc, 2, 0x01, &redirects, false);
    assert_eq!(count, 1);
    assert_eq!(rom[table_pc + 2], 0x00);
    assert_eq!(rom[table_pc + 3], 0xFE);
    assert_eq!(rom[table_pc + 1], 0x90);
}

#[test]
fn rewrite_2byte_rejects_cross_bank_redirect() {
    let mut data = make_rom(0x20000);
    let table_pc = 0x100;
    data[table_pc] = 0x00;
    data[table_pc + 1] = 0x90;
    let mut rom = TrackedRom::new(data);

    let redirects = vec![(SnesAddr::new(0x01, 0x9000), SnesAddr::new(0x32, 0xD600))];

    let count = rewrite_2byte_pointer_table(&mut rom, table_pc, 1, 0x01, &redirects, false);
    assert_eq!(count, 0);
    assert_eq!(rom[table_pc + 1], 0x90);
}

#[test]
fn rewrite_2byte_sub_pointer_delta() {
    let mut data = make_rom(0x20000);
    let table_pc = 0x100;
    let old_addr: u16 = 0xD000;
    let sub_addr: u16 = old_addr + 2;
    data[table_pc] = sub_addr as u8;
    data[table_pc + 1] = (sub_addr >> 8) as u8;
    let mut rom = TrackedRom::new(data);

    let redirects = vec![(SnesAddr::new(0x03, old_addr), SnesAddr::new(0x03, 0xE000))];

    let count = rewrite_2byte_pointer_table(&mut rom, table_pc, 1, 0x03, &redirects, true);
    assert_eq!(count, 1);
    let new_sub = 0xE000u16 + 2;
    assert_eq!(rom[table_pc], new_sub as u8);
    assert_eq!(rom[table_pc + 1], (new_sub >> 8) as u8);
}

#[test]
fn rewrite_2byte_table_stops_at_rom_end() {
    let data = make_rom(0x105);
    let table_pc = 0x104;
    let mut rom = TrackedRom::new(data);
    let redirects = vec![(SnesAddr::new(0x01, 0x9000), SnesAddr::new(0x01, 0xFE00))];
    let count = rewrite_2byte_pointer_table(&mut rom, table_pc, 1, 0x01, &redirects, false);
    assert_eq!(count, 0);
}

#[test]
fn rewrite_at_known_pcs_rewrites_matching() {
    let mut data = make_rom(0x20000);
    let pc = 0x1000;
    data[pc] = 0x00;
    data[pc + 1] = 0x90;
    data[pc + 2] = 0x01;
    let mut rom = TrackedRom::new(data);

    let count = rewrite_at_known_pcs(
        &mut rom,
        &[pc],
        &[(SnesAddr::new(0x01, 0x9000), SnesAddr::new(0x33, 0x8000))],
    );
    assert_eq!(count, 1);
    assert_eq!(rom[pc], 0x00);
    assert_eq!(rom[pc + 1], 0x80);
    assert_eq!(rom[pc + 2], 0x33);
}

#[test]
fn rewrite_at_known_pcs_skips_non_matching() {
    let mut data = make_rom(0x20000);
    let pc = 0x1000;
    data[pc] = 0x00;
    data[pc + 1] = 0x90;
    data[pc + 2] = 0x01;
    let mut rom = TrackedRom::new(data);

    let count = rewrite_at_known_pcs(
        &mut rom,
        &[pc],
        &[(SnesAddr::new(0x01, 0xB000), SnesAddr::new(0x33, 0x8000))],
    );
    assert_eq!(count, 0);
    assert_eq!(rom[pc + 2], 0x01);
}

#[test]
fn rewrite_at_known_pcs_skips_out_of_bounds() {
    let data = make_rom(100);
    let mut rom = TrackedRom::new(data);
    let count = rewrite_at_known_pcs(
        &mut rom,
        &[98, 99, 200],
        &[(SnesAddr::new(0x00, 0x0000), SnesAddr::new(0x33, 0x8000))],
    );
    assert_eq!(count, 0);
}

#[test]
fn rewrite_at_known_pcs_handles_multiple() {
    let mut data = make_rom(0x20000);
    let pc1 = 0x1000;
    data[pc1] = 0x00;
    data[pc1 + 1] = 0x90;
    data[pc1 + 2] = 0x01;

    let pc2 = 0x2000;
    data[pc2] = 0x00;
    data[pc2 + 1] = 0xA0;
    data[pc2 + 2] = 0x01;
    let mut rom = TrackedRom::new(data);

    let count = rewrite_at_known_pcs(
        &mut rom,
        &[pc1, pc2],
        &[
            (SnesAddr::new(0x01, 0x9000), SnesAddr::new(0x32, 0xD600)),
            (SnesAddr::new(0x01, 0xA000), SnesAddr::new(0x33, 0x8100)),
        ],
    );
    assert_eq!(count, 2);
    assert_eq!(rom[pc1 + 2], 0x32);
    assert_eq!(rom[pc2 + 2], 0x33);
}

// --- Additional tests for scan_pointers ---

#[test]
fn scan_pointers_finds_multiple_in_range() {
    let mut rom = make_rom(0x10000);
    let base = lorom_to_pc(0x01, 0x8000);
    // Place two valid pointers at different positions
    // Pointer 1: bank=0x01, addr=$9000
    rom[base] = 0x00;
    rom[base + 1] = 0x90;
    rom[base + 2] = 0x01;
    // Pointer 2: bank=0x01, addr=$A000 (at offset +5)
    rom[base + 5] = 0x00;
    rom[base + 6] = 0xA0;
    rom[base + 7] = 0x01;

    let entries = scan_pointers(&rom, 0x01, 0x8000, 0x800A, Some(0x01));
    let targets: Vec<SnesAddr> = entries.iter().map(|e| e.target).collect();
    assert!(targets.contains(&SnesAddr::new(0x01, 0x9000)));
    assert!(targets.contains(&SnesAddr::new(0x01, 0xA000)));
}

#[test]
fn scan_pointers_rejects_bank_above_3f() {
    let mut rom = make_rom(0x10000);
    let pc = lorom_to_pc(0x01, 0x8000);
    // Bank $40 is invalid for LoROM
    rom[pc] = 0x00;
    rom[pc + 1] = 0x90;
    rom[pc + 2] = 0x40;
    let entries = scan_pointers(&rom, 0x01, 0x8000, 0x8010, None);
    assert!(entries.iter().all(|e| e.target.bank <= 0x3F));
}

#[test]
fn scan_pointers_pc_offset_is_correct() {
    let mut rom = make_rom(0x10000);
    let pc = lorom_to_pc(0x01, 0x8005);
    rom[pc] = 0x00;
    rom[pc + 1] = 0x90;
    rom[pc + 2] = 0x02;
    let entries = scan_pointers(&rom, 0x01, 0x8000, 0x8010, Some(0x02));
    let found = entries
        .iter()
        .find(|e| e.target == SnesAddr::new(0x02, 0x9000));
    assert!(found.is_some());
    assert_eq!(found.unwrap().pc_offset, pc);
}

#[test]
fn scan_pointers_empty_range() {
    let rom = make_rom(0x10000);
    // scan_start == scan_end should produce nothing useful
    let entries = scan_pointers(&rom, 0x01, 0x8000, 0x8000, None);
    assert!(entries.is_empty());
}

// --- Additional tests for rewrite_2byte_pointer_table ---

#[test]
fn rewrite_2byte_multiple_entries() {
    let mut data = make_rom(0x20000);
    let table_pc = 0x200;
    // Entry 0: $9000
    data[table_pc] = 0x00;
    data[table_pc + 1] = 0x90;
    // Entry 1: $A000
    data[table_pc + 2] = 0x00;
    data[table_pc + 3] = 0xA0;
    // Entry 2: $B000 (no redirect)
    data[table_pc + 4] = 0x00;
    data[table_pc + 5] = 0xB0;
    let mut rom = TrackedRom::new(data);

    let redirects = vec![
        (SnesAddr::new(0x01, 0x9000), SnesAddr::new(0x01, 0xF000)),
        (SnesAddr::new(0x01, 0xA000), SnesAddr::new(0x01, 0xF100)),
    ];

    let count = rewrite_2byte_pointer_table(&mut rom, table_pc, 3, 0x01, &redirects, false);
    assert_eq!(count, 2);
    // Entry 0 redirected
    assert_eq!(
        u16::from_le_bytes([rom[table_pc], rom[table_pc + 1]]),
        0xF000
    );
    // Entry 1 redirected
    assert_eq!(
        u16::from_le_bytes([rom[table_pc + 2], rom[table_pc + 3]]),
        0xF100
    );
    // Entry 2 unchanged
    assert_eq!(
        u16::from_le_bytes([rom[table_pc + 4], rom[table_pc + 5]]),
        0xB000
    );
}

#[test]
fn rewrite_2byte_sub_pointer_delta_max_8() {
    // Sub-pointer match allows delta up to 8
    let mut data = make_rom(0x20000);
    let table_pc = 0x100;
    let old_addr: u16 = 0xD000;

    // Delta = 8 (within range)
    data[table_pc] = (old_addr + 8) as u8;
    data[table_pc + 1] = ((old_addr + 8) >> 8) as u8;
    let mut rom = TrackedRom::new(data);

    let redirects = vec![(SnesAddr::new(0x03, old_addr), SnesAddr::new(0x03, 0xE000))];

    let count = rewrite_2byte_pointer_table(&mut rom, table_pc, 1, 0x03, &redirects, true);
    assert_eq!(count, 1);
    let expected = 0xE000u16 + 8;
    assert_eq!(
        u16::from_le_bytes([rom[table_pc], rom[table_pc + 1]]),
        expected
    );
}

#[test]
fn rewrite_2byte_sub_pointer_delta_9_rejected() {
    // Delta = 9 is outside the sub-pointer range
    let mut data = make_rom(0x20000);
    let table_pc = 0x100;
    let old_addr: u16 = 0xD000;

    data[table_pc] = (old_addr + 9) as u8;
    data[table_pc + 1] = ((old_addr + 9) >> 8) as u8;
    let mut rom = TrackedRom::new(data);

    let redirects = vec![(SnesAddr::new(0x03, old_addr), SnesAddr::new(0x03, 0xE000))];

    let count = rewrite_2byte_pointer_table(&mut rom, table_pc, 1, 0x03, &redirects, true);
    assert_eq!(count, 0);
    // Unchanged
    assert_eq!(
        u16::from_le_bytes([rom[table_pc], rom[table_pc + 1]]),
        old_addr + 9
    );
}

#[test]
fn rewrite_2byte_exact_match_takes_priority_over_sub() {
    // When a pointer matches both an exact redirect and could be a sub-pointer
    // of another, the exact match should win
    let mut data = make_rom(0x20000);
    let table_pc = 0x100;
    let addr: u16 = 0xD002;
    data[table_pc] = addr as u8;
    data[table_pc + 1] = (addr >> 8) as u8;

    let redirects = vec![
        // Sub-pointer candidate: $D000 -> $E000 (delta=2)
        (SnesAddr::new(0x03, 0xD000), SnesAddr::new(0x03, 0xE000)),
        // Exact match: $D002 -> $F000
        (SnesAddr::new(0x03, 0xD002), SnesAddr::new(0x03, 0xF000)),
    ];
    let mut rom = TrackedRom::new(data);

    let count = rewrite_2byte_pointer_table(&mut rom, table_pc, 1, 0x03, &redirects, true);
    assert_eq!(count, 1);
    // Should be $F000 (exact match), not $E002 (sub-pointer)
    assert_eq!(
        u16::from_le_bytes([rom[table_pc], rom[table_pc + 1]]),
        0xF000
    );
}

#[test]
fn rewrite_2byte_sub_pointer_cross_bank_old_rejected() {
    // Sub-pointer logic skips if old.bank != pointer_bank
    let mut data = make_rom(0x20000);
    let table_pc = 0x100;
    data[table_pc] = 0x02;
    data[table_pc + 1] = 0xD0; // $D002
    let mut rom = TrackedRom::new(data);

    let redirects = vec![(
        SnesAddr::new(0x05, 0xD000), // old bank != pointer_bank
        SnesAddr::new(0x03, 0xE000),
    )];

    let count = rewrite_2byte_pointer_table(&mut rom, table_pc, 1, 0x03, &redirects, true);
    assert_eq!(count, 0);
}

// --- Additional tests for rewrite_at_known_pcs ---

#[test]
fn rewrite_at_known_pcs_preserves_addr_bytes() {
    // Verify lo/hi addr bytes are written correctly for non-aligned addresses
    let mut data = make_rom(0x20000);
    let pc = 0x500;
    data[pc] = 0xAB;
    data[pc + 1] = 0xCD;
    data[pc + 2] = 0x02;
    let mut rom = TrackedRom::new(data);

    let count = rewrite_at_known_pcs(
        &mut rom,
        &[pc],
        &[(SnesAddr::new(0x02, 0xCDAB), SnesAddr::new(0x31, 0xC7F0))],
    );
    assert_eq!(count, 1);
    assert_eq!(rom[pc], 0xF0); // lo byte of $C7F0
    assert_eq!(rom[pc + 1], 0xC7); // hi byte of $C7F0
    assert_eq!(rom[pc + 2], 0x31);
}

#[test]
fn rewrite_at_known_pcs_empty_list() {
    let data = make_rom(0x1000);
    let mut rom = TrackedRom::new(data);
    let count = rewrite_at_known_pcs(
        &mut rom,
        &[],
        &[(SnesAddr::new(0x01, 0x9000), SnesAddr::new(0x33, 0x8000))],
    );
    assert_eq!(count, 0);
}

#[test]
fn rewrite_at_known_pcs_empty_redirects() {
    let mut data = make_rom(0x20000);
    let pc = 0x1000;
    data[pc] = 0x00;
    data[pc + 1] = 0x90;
    data[pc + 2] = 0x01;
    let mut rom = TrackedRom::new(data);

    let count = rewrite_at_known_pcs(&mut rom, &[pc], &[]);
    assert_eq!(count, 0);
    // Original values unchanged
    assert_eq!(rom[pc + 2], 0x01);
}

#[test]
fn rewrite_at_known_pcs_first_redirect_wins() {
    // If multiple redirects match, the first one in the list wins
    let mut data = make_rom(0x20000);
    let pc = 0x1000;
    data[pc] = 0x00;
    data[pc + 1] = 0x90;
    data[pc + 2] = 0x01;
    let mut rom = TrackedRom::new(data);

    let count = rewrite_at_known_pcs(
        &mut rom,
        &[pc],
        &[
            (SnesAddr::new(0x01, 0x9000), SnesAddr::new(0x32, 0xD600)),
            (SnesAddr::new(0x01, 0x9000), SnesAddr::new(0x33, 0x8000)),
        ],
    );
    assert_eq!(count, 1);
    assert_eq!(rom[pc + 2], 0x32); // first redirect wins
}

// --- Tests for rewrite_pointers ---

#[test]
fn rewrite_pointers_updates_all_three_bytes() {
    let mut data = make_rom(0x10000);
    let pc = 0x200;
    data[pc] = 0x00;
    data[pc + 1] = 0x80;
    data[pc + 2] = 0x2A;
    let mut rom = TrackedRom::new(data);

    let entries = vec![PointerEntry {
        pc_offset: pc,
        target: SnesAddr::new(0x2A, 0x8000),
    }];
    let redirects = vec![(SnesAddr::new(0x2A, 0x8000), SnesAddr::new(0x31, 0xC700))];

    rewrite_pointers(&mut rom, &entries, &redirects);
    assert_eq!(rom[pc], 0x00); // lo of $C700
    assert_eq!(rom[pc + 1], 0xC7); // hi of $C700
    assert_eq!(rom[pc + 2], 0x31);
}

#[test]
fn rewrite_pointers_handles_multiple_entries_multiple_redirects() {
    let mut data = make_rom(0x10000);
    // Entry A at pc=0x100, pointing to $01:$9000
    let pc_a = 0x100;
    data[pc_a] = 0x00;
    data[pc_a + 1] = 0x90;
    data[pc_a + 2] = 0x01;

    // Entry B at pc=0x200, pointing to $01:$A000
    let pc_b = 0x200;
    data[pc_b] = 0x00;
    data[pc_b + 1] = 0xA0;
    data[pc_b + 2] = 0x01;

    // Entry C at pc=0x300, pointing to $02:$B000 (no redirect)
    let pc_c = 0x300;
    data[pc_c] = 0x00;
    data[pc_c + 1] = 0xB0;
    data[pc_c + 2] = 0x02;

    let entries = vec![
        PointerEntry {
            pc_offset: pc_a,
            target: SnesAddr::new(0x01, 0x9000),
        },
        PointerEntry {
            pc_offset: pc_b,
            target: SnesAddr::new(0x01, 0xA000),
        },
        PointerEntry {
            pc_offset: pc_c,
            target: SnesAddr::new(0x02, 0xB000),
        },
    ];
    let redirects = vec![
        (SnesAddr::new(0x01, 0x9000), SnesAddr::new(0x32, 0xD600)),
        (SnesAddr::new(0x01, 0xA000), SnesAddr::new(0x33, 0x8100)),
    ];
    let mut rom = TrackedRom::new(data);

    let count = rewrite_pointers(&mut rom, &entries, &redirects);
    assert_eq!(count, 2);
    assert_eq!(rom[pc_a + 2], 0x32);
    assert_eq!(rom[pc_b + 2], 0x33);
    assert_eq!(rom[pc_c + 2], 0x02); // unchanged
}

// --- Tests for rewrite_scattered_2byte_ptrs ---

#[test]
fn scattered_2byte_exact_match() {
    let mut data = make_rom(0x20000);
    let pc = 0x1000;
    data[pc] = 0x00;
    data[pc + 1] = 0x90; // $9000
    let mut rom = TrackedRom::new(data);

    let redirects = vec![(SnesAddr::new(0x01, 0x9000), SnesAddr::new(0x01, 0xF000))];

    let count = rewrite_scattered_2byte_ptrs(&mut rom, &[pc], 0x01, &redirects);
    assert_eq!(count, 1);
    assert_eq!(u16::from_le_bytes([rom[pc], rom[pc + 1]]), 0xF000);
}

#[test]
fn scattered_2byte_cross_bank_rejected() {
    let mut data = make_rom(0x20000);
    let pc = 0x1000;
    data[pc] = 0x00;
    data[pc + 1] = 0x90;
    let mut rom = TrackedRom::new(data);

    let redirects = vec![(
        SnesAddr::new(0x01, 0x9000),
        SnesAddr::new(0x32, 0xD600), // different bank
    )];

    let count = rewrite_scattered_2byte_ptrs(&mut rom, &[pc], 0x01, &redirects);
    assert_eq!(count, 0);
    assert_eq!(u16::from_le_bytes([rom[pc], rom[pc + 1]]), 0x9000);
}

#[test]
fn scattered_2byte_no_match() {
    let mut data = make_rom(0x20000);
    let pc = 0x1000;
    data[pc] = 0x00;
    data[pc + 1] = 0xA0;
    let mut rom = TrackedRom::new(data);

    let redirects = vec![(SnesAddr::new(0x01, 0x9000), SnesAddr::new(0x01, 0xF000))];

    let count = rewrite_scattered_2byte_ptrs(&mut rom, &[pc], 0x01, &redirects);
    assert_eq!(count, 0);
}

#[test]
fn scattered_2byte_out_of_bounds() {
    let data = make_rom(100);
    let mut rom = TrackedRom::new(data);
    let count = rewrite_scattered_2byte_ptrs(
        &mut rom,
        &[99, 200],
        0x01,
        &[(SnesAddr::new(0x01, 0x9000), SnesAddr::new(0x01, 0xF000))],
    );
    assert_eq!(count, 0);
}

#[test]
fn scattered_2byte_multiple_pcs() {
    let mut data = make_rom(0x20000);
    let pc1 = 0x1000;
    data[pc1] = 0x00;
    data[pc1 + 1] = 0x90; // $9000
    let pc2 = 0x2000;
    data[pc2] = 0x00;
    data[pc2 + 1] = 0xA0; // $A000
    let mut rom = TrackedRom::new(data);

    let redirects = vec![
        (SnesAddr::new(0x01, 0x9000), SnesAddr::new(0x01, 0xF000)),
        (SnesAddr::new(0x01, 0xA000), SnesAddr::new(0x01, 0xF100)),
    ];

    let count = rewrite_scattered_2byte_ptrs(&mut rom, &[pc1, pc2], 0x01, &redirects);
    assert_eq!(count, 2);
    assert_eq!(u16::from_le_bytes([rom[pc1], rom[pc1 + 1]]), 0xF000);
    assert_eq!(u16::from_le_bytes([rom[pc2], rom[pc2 + 1]]), 0xF100);
}
