use super::*;
use crate::patch::tracked_rom::TrackedRom;

/// Build test data matching the old hardcoded KO_MONSTER_NAMES[0] (스키야보데스).
fn test_name_0() -> Vec<u8> {
    vec![0x3D, 0x82, 0x35, 0x3E, 0x46, 0x3D, 0xFF]
}

/// Build minimal test encyclopedia data with 36 entries.
fn build_test_data() -> EncyclopediaData {
    let mut names = Vec::new();
    let mut battle_names = Vec::new();
    let mut descs = Vec::new();
    for i in 0..MONSTER_COUNT {
        if i == 0 {
            names.push(test_name_0());
            // Battle name: same as full name without FF terminator (6 bytes, fits in 8)
            battle_names.push(vec![0x3D, 0x82, 0x35, 0x3E, 0x46, 0x3D]);
        } else {
            // Minimal valid name: single byte + FF
            names.push(vec![0x20 + i as u8, 0xFF]);
            // Battle name: single byte (no terminator)
            battle_names.push(vec![0x20 + i as u8]);
        }
        // Minimal valid desc: [loc_index, one_byte, FF]
        descs.push(vec![i as u8 % 27, 0x21, 0xFF]);
    }
    EncyclopediaData {
        names,
        battle_names,
        descs,
    }
}

#[test]
fn c6ee_hook_assembles() {
    let code = build_c6ee_hook();
    assert!(!code.is_empty());
    assert!(code.len() < 80, "C6EE hook too large: {} bytes", code.len());
    // Should start with CMP #$F1
    assert_eq!(code[0], 0xC9);
    assert_eq!(code[1], 0xF1);
    // Should contain CMP #$F0
    let has_f0 = code.windows(2).any(|w| w == [0xC9, 0xF0]);
    assert!(has_f0, "missing CMP #$F0");
    // Should contain LDA #$32 (Bank $32)
    let has_32 = code.windows(2).any(|w| w == [0xA9, 0x32]);
    assert!(has_32, "missing LDA #$32");
    // Should contain original LDA #$0F fallback
    let has_0f = code.windows(2).any(|w| w == [0xA9, 0x0F]);
    assert!(has_0f, "missing LDA #$0F fallback");
}

#[test]
fn name_hook_assembles() {
    let code = build_name_hook(0xF840);
    assert!(!code.is_empty());
    assert!(code.len() < 40, "name hook too large: {} bytes", code.len());
    // Should contain LDA $7E:17B6 (AF B6 17 7E) — reads monster discovery ID from offset $56
    let has_lda_long = code.windows(4).any(|w| w == [0xAF, 0xB6, 0x17, 0x7E]);
    assert!(has_lda_long, "missing LDA $7E:17B6");
    // Should contain DEC A (3A) to convert 1-based ID to 0-based table index
    let has_dec = code.contains(&0x3A);
    assert!(has_dec, "missing DEC A (1-based to 0-based conversion)");
    // Should contain LDA #$03 (bank)
    let has_bank = code.windows(2).any(|w| w == [0xA9, HOOK_BANK]);
    assert!(has_bank, "missing LDA #$03");
    // Should contain REP #$20
    let has_rep = code.windows(2).any(|w| w == [0xC2, 0x20]);
    assert!(has_rep, "missing REP #$20");
    // Should contain SEP #$20
    let has_sep = code.windows(2).any(|w| w == [0xE2, 0x20]);
    assert!(has_sep, "missing SEP #$20");
}

#[test]
fn name_hook_size_stable() {
    // Verify the hook size doesn't change with different table addresses
    let code_a = build_name_hook(0xF840);
    let code_b = build_name_hook(0xFA00);
    assert_eq!(code_a.len(), code_b.len());
}

#[test]
fn test_data_names_fit_in_entry() {
    let data = build_test_data();
    for (i, name) in data.names.iter().enumerate() {
        assert!(
            name.len() <= NAME_ENTRY_SIZE,
            "Monster #{} name ({} bytes) exceeds entry size ({})",
            i,
            name.len(),
            NAME_ENTRY_SIZE
        );
        // Each name must end with FF terminator
        assert_eq!(
            *name.last().unwrap(),
            0xFF,
            "Monster #{} name missing FF terminator",
            i
        );
    }
}

#[test]
fn hooks_fit_in_reserved_space() {
    let c6ee = build_c6ee_hook();
    let name = build_name_hook(0xFA00);
    let table = MONSTER_COUNT * NAME_ENTRY_SIZE;
    let total = c6ee.len() + name.len() + table;
    let available = 0xFFFF - ENC_HOOK_BASE as usize + 1;
    assert!(
        total <= available,
        "hooks+table ({} bytes) exceed reserved space ({} bytes)",
        total,
        available
    );
}

#[test]
fn patch_monster_data_names_writes_ko_names() {
    // Create a minimal ROM with a fake monster table
    let mut raw = vec![0xFF; 0x100000];

    // Set up a simple monster pointer table at $1D:$8000 (PC 0xE8000)
    for i in 0..MONSTER_COUNT {
        let entry_pc = MONSTER_TABLE_PC + i * 6;
        let block_addr: u16 = 0xB000 + (i as u16) * 0x100;
        raw[entry_pc] = block_addr as u8;
        raw[entry_pc + 1] = (block_addr >> 8) as u8;
        raw[entry_pc + 2] = 0x1D;
    }

    let mut rom = TrackedRom::new(raw);
    let data = build_test_data();
    let patched = patch_monster_data_names(&mut rom, &data.battle_names).unwrap();
    assert_eq!(patched, MONSTER_COUNT);

    // Verify monster #0: battle name [3D 82 35 3E 46 3D] + 00 padding, index at $56
    let block_pc_0 = lorom_to_pc(0x1D, 0xB000);
    let name_pc_0 = block_pc_0 + NAME_OFFSET_IN_BLOCK;
    assert_eq!(
        &rom[name_pc_0..name_pc_0 + 6],
        &[0x3D, 0x82, 0x35, 0x3E, 0x46, 0x3D],
        "Monster #0 battle name mismatch"
    );
    assert_eq!(rom[name_pc_0 + 6], 0x00, "Monster #0 missing 00 terminator");
    assert_eq!(rom[name_pc_0 + 7], 0x00, "Monster #0 padding not zero");
    assert_eq!(rom[name_pc_0 + 8], 0x00, "Monster #0 padding not zero");
    assert_eq!(
        rom[name_pc_0 + 9],
        0x00,
        "Monster #0 last slot byte not zero"
    );
    // Index at offset $56 (1-based: monster #0 → value 1)
    let index_pc_0 = block_pc_0 + INDEX_OFFSET_IN_BLOCK;
    assert_eq!(
        rom[index_pc_0], 0x01,
        "Monster #0 discovery ID should be 1 (1-based)"
    );

    // Verify all monsters have their 1-based discovery ID at offset $56
    for i in 0..MONSTER_COUNT {
        let block_pc = lorom_to_pc(0x1D, 0xB000 + (i as u16) * 0x100);
        let name_pc = block_pc + NAME_OFFSET_IN_BLOCK;
        let index_pc = block_pc + INDEX_OFFSET_IN_BLOCK;
        assert_eq!(
            rom[index_pc],
            (i + 1) as u8,
            "Monster #{} discovery ID should be {} (1-based)",
            i,
            i + 1
        );
        // Battle name bytes should be present at start of slot
        let battle = &data.battle_names[i];
        assert_eq!(
            &rom[name_pc..name_pc + battle.len()],
            &battle[..],
            "Monster #{} battle name bytes mismatch",
            i
        );
    }
}

#[test]
fn apply_encyclopedia_hooks_integration() {
    // Create a ROM large enough for all banks
    let mut raw = vec![0xFF; 0x200000]; // 2MB

    // Set up the monster pointer table
    for i in 0..MONSTER_COUNT {
        let entry_pc = MONSTER_TABLE_PC + i * 6;
        let block_addr: u16 = 0xB000 + (i as u16) * 0x100;
        raw[entry_pc] = block_addr as u8;
        raw[entry_pc + 1] = (block_addr >> 8) as u8;
        raw[entry_pc + 2] = 0x1D;
    }

    // Plant original C6EE bytes at $03:$C742 (PC 0x1C742)
    raw[C6EE_PATCH_PC] = 0xEB; // XBA
    raw[C6EE_PATCH_PC + 1] = 0xA9; // LDA #$0F
    raw[C6EE_PATCH_PC + 2] = 0x0F;

    // Plant original name bytes at $03:$B626 (PC 0x1B626)
    raw[NAME_PATCH_PC] = 0xA0; // LDY #$17AA
    raw[NAME_PATCH_PC + 1] = 0xAA;
    raw[NAME_PATCH_PC + 2] = 0x17;

    let mut rom = TrackedRom::new(raw);
    let data = build_test_data();
    let count = apply_encyclopedia_hooks(&mut rom, &data).unwrap();
    assert!(count > 0);

    // Verify C6EE patch: should be JMP
    assert_eq!(rom[C6EE_PATCH_PC], 0x4C, "C6EE not patched with JMP");

    // Verify name patch: should be JMP
    assert_eq!(rom[NAME_PATCH_PC], 0x4C, "B626 not patched with JMP");

    // Verify name table has valid data (first entry should be 스키야보데스)
    let table_pc = lorom_to_pc(
        HOOK_BANK,
        ENC_HOOK_BASE + build_c6ee_hook().len() as u16 + build_name_hook(0).len() as u16,
    );
    assert_eq!(rom[table_pc], 0x3D); // 스 = 0x3D

    // Verify monster data blocks have KO battle names + index at offset $56
    for i in 0..MONSTER_COUNT {
        let entry_pc = MONSTER_TABLE_PC + i * 6;
        let lo = rom[entry_pc] as u16;
        let hi = rom[entry_pc + 1] as u16;
        let bank = rom[entry_pc + 2];
        let block_pc =
            ((bank as usize) & 0x7F) * 0x8000 + ((hi << 8 | lo) as usize).wrapping_sub(0x8000);
        let name_pc = block_pc + NAME_OFFSET_IN_BLOCK;
        let index_pc = block_pc + INDEX_OFFSET_IN_BLOCK;
        // Monster discovery ID at offset $56 (1-based)
        assert_eq!(
            rom[index_pc],
            (i + 1) as u8,
            "Monster #{} discovery ID should be {} (1-based)",
            i,
            i + 1
        );
        // Battle name bytes at start of slot
        let battle = &data.battle_names[i];
        assert_eq!(
            &rom[name_pc..name_pc + battle.len()],
            &battle[..],
            "Monster #{} battle name mismatch",
            i
        );
    }

    // Verify desc table written to Bank $33
    let desc_pc = lorom_to_pc(DESC_TABLE_BANK, DESC_TABLE_BASE);
    // First desc starts with location_index from test data
    assert_eq!(
        rom[desc_pc], data.descs[0][0],
        "first desc location_index mismatch"
    );
    // Should end with FF
    let desc0_end = desc_pc + data.descs[0].len() - 1;
    assert_eq!(rom[desc0_end], 0xFF, "first desc missing FF terminator");

    // Verify desc pointers patched in data blocks
    for i in 0..MONSTER_COUNT {
        let entry_pc = MONSTER_TABLE_PC + i * 6;
        let lo = rom[entry_pc] as u16;
        let hi = rom[entry_pc + 1] as u16;
        let bank = rom[entry_pc + 2];
        let block_pc =
            ((bank as usize) & 0x7F) * 0x8000 + ((hi << 8 | lo) as usize).wrapping_sub(0x8000);
        let ptr_pc = block_pc + DESC_PTR_OFFSET;
        // Bank byte should be DESC_TABLE_BANK
        assert_eq!(
            rom[ptr_pc + 2],
            DESC_TABLE_BANK,
            "Monster #{} desc bank mismatch",
            i
        );
    }
}

#[test]
fn test_descs_have_location_index_and_terminator() {
    let data = build_test_data();
    for (i, desc) in data.descs.iter().enumerate() {
        assert!(
            desc.len() >= 3,
            "Monster #{} desc too short ({} bytes)",
            i,
            desc.len()
        );
        // Last byte must be FF (terminator)
        assert_eq!(
            *desc.last().unwrap(),
            0xFF,
            "Monster #{} desc missing FF terminator",
            i
        );
        // First byte is location index (0-26 range)
        assert!(
            desc[0] <= 0x1A,
            "Monster #{} location index {} out of range",
            i,
            desc[0]
        );
    }
}

#[test]
fn desc_table_fits_in_bank_33() {
    let data = build_test_data();
    let total: usize = data.descs.iter().map(|d| d.len()).sum();
    let end = DESC_TABLE_BASE as usize + total;
    assert!(
        end <= 0x10000,
        "Desc table overflows Bank $33: {} bytes, end at ${:04X}",
        total,
        end
    );
}

#[test]
fn desc_count_matches_monster_count() {
    let data = build_test_data();
    assert_eq!(data.descs.len(), MONSTER_COUNT);
}

#[test]
fn write_desc_table_writes_all_data() {
    let mut raw = vec![0xFF; 0x200000];

    // Set up monster pointer table
    for i in 0..MONSTER_COUNT {
        let entry_pc = MONSTER_TABLE_PC + i * 6;
        let block_addr: u16 = 0xB000 + (i as u16) * 0x100;
        raw[entry_pc] = block_addr as u8;
        raw[entry_pc + 1] = (block_addr >> 8) as u8;
        raw[entry_pc + 2] = 0x1D;
    }

    let mut rom = TrackedRom::new(raw);
    let data = build_test_data();
    let patched = write_desc_table_and_patch_pointers(&mut rom, &data.descs).unwrap();
    assert_eq!(patched, MONSTER_COUNT);

    // Verify first desc data is written correctly
    let desc_pc = lorom_to_pc(DESC_TABLE_BANK, DESC_TABLE_BASE);
    assert_eq!(
        &rom[desc_pc..desc_pc + data.descs[0].len()],
        &data.descs[0][..]
    );

    // Verify second desc follows immediately
    let desc1_pc = desc_pc + data.descs[0].len();
    assert_eq!(
        &rom[desc1_pc..desc1_pc + data.descs[1].len()],
        &data.descs[1][..]
    );

    // Verify last desc pointer
    let entry_pc = MONSTER_TABLE_PC + 35 * 6;
    let lo = rom[entry_pc] as u16;
    let hi = rom[entry_pc + 1] as u16;
    let bank = rom[entry_pc + 2];
    let block_pc =
        ((bank as usize) & 0x7F) * 0x8000 + ((hi << 8 | lo) as usize).wrapping_sub(0x8000);
    let ptr_pc = block_pc + DESC_PTR_OFFSET;
    let desc_addr = rom[ptr_pc] as u16 | ((rom[ptr_pc + 1] as u16) << 8);
    // Verify the pointer points to valid KO desc data
    let last_desc_pc = lorom_to_pc(DESC_TABLE_BANK, desc_addr);
    assert_eq!(rom[last_desc_pc], data.descs[35][0]); // location index
}

#[test]
fn load_encyclopedia_tsv_parses_correctly() {
    let mut table = HashMap::new();
    table.insert('가', vec![0x29]);
    table.insert('나', vec![0x33]);

    // Write temp file
    let dir = std::env::temp_dir().join("madou_test_enc");
    std::fs::create_dir_all(&dir).unwrap();
    let tsv_path = dir.join("test_enc.tsv");

    // Build full TSV with 36 entries
    let mut full_tsv = String::from("# comment\nID\tTYPE\tLOC_IDX\tKO\n");
    for i in 0..36 {
        full_tsv.push_str(&format!("{}\tname\t0\t가\n", i));
        full_tsv.push_str(&format!("{}\tdesc\t{}\t나\n", i, i % 27));
    }
    std::fs::write(&tsv_path, &full_tsv).unwrap();

    let data = load_encyclopedia_tsv(&tsv_path, &table).unwrap();
    assert_eq!(data.names.len(), 36);
    assert_eq!(data.battle_names.len(), 36);
    assert_eq!(data.descs.len(), 36);

    // Name 0: 가 = [0x29, 0xFF]
    assert_eq!(data.names[0], vec![0x29, 0xFF]);
    // Battle name 0: 가 = [0x29] (no terminator, raw encoded bytes)
    assert_eq!(data.battle_names[0], vec![0x29]);
    // Desc 0: [loc=0, 나=0x33, FF]
    assert_eq!(data.descs[0], vec![0x00, 0x33, 0xFF]);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn load_encyclopedia_tsv_handles_newlines_in_desc() {
    let mut table = HashMap::new();
    table.insert('가', vec![0x29]);

    let dir = std::env::temp_dir().join("madou_test_enc_nl");
    std::fs::create_dir_all(&dir).unwrap();
    let tsv_path = dir.join("test_enc_nl.tsv");

    let mut tsv = String::from("ID\tTYPE\tLOC_IDX\tKO\n");
    for i in 0..36 {
        tsv.push_str(&format!("{}\tname\t0\t가\n", i));
        // Use literal \n in desc text
        tsv.push_str(&format!("{}\tdesc\t{}\t가\\n가\n", i, i % 27));
    }
    std::fs::write(&tsv_path, &tsv).unwrap();

    let data = load_encyclopedia_tsv(&tsv_path, &table).unwrap();
    assert_eq!(data.battle_names.len(), 36);
    // Desc 0: [loc=0, 가=0x29, \n=0xF9, 가=0x29, FF]
    assert_eq!(data.descs[0], vec![0x00, 0x29, 0xF9, 0x29, 0xFF]);
    // Battle name 0: [0x29]
    assert_eq!(data.battle_names[0], vec![0x29]);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn truncate_single_byte_chars() {
    // 11 single-byte chars → truncate to 10
    let bytes = vec![
        0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A,
    ];
    let result = truncate_at_char_boundary(&bytes, 10);
    assert_eq!(result, &bytes[..10]);
}

#[test]
fn truncate_preserves_fb_prefix_boundary() {
    // 9 bytes: [A, FB X, B, FB Y, C, D, E, FB Z] → fits in 10
    let bytes9 = vec![0x20, 0xFB, 0x48, 0x21, 0xFB, 0x49, 0x22, 0xFB, 0x50];
    let result9 = truncate_at_char_boundary(&bytes9, 10);
    assert_eq!(result9, bytes9);

    // 10 bytes: exactly fills slot
    let bytes10 = vec![0x20, 0xFB, 0x48, 0x21, 0xFB, 0x49, 0x22, 0xFB, 0x50, 0x23];
    let result10 = truncate_at_char_boundary(&bytes10, 10);
    assert_eq!(result10, bytes10);

    // 11 bytes: [A, FB X, B, FB Y, C, FB Z, D, FB W] → FB W doesn't fit
    let bytes11 = vec![
        0x20, 0xFB, 0x48, 0x21, 0xFB, 0x49, 0x22, 0xFB, 0x50, 0xFB, 0x51,
    ];
    let result11 = truncate_at_char_boundary(&bytes11, 10);
    // Should keep first 9 bytes (cuts before the FB at position 9)
    assert_eq!(result11, &bytes11[..9]);
}

#[test]
fn truncate_f0_f1_prefixes() {
    // F0 33, F1 96, 20, FB C8, 8E, 4A = 쌍둥이캣토시 (9 bytes) → fits in 10
    let bytes = vec![0xF0, 0x33, 0xF1, 0x96, 0x20, 0xFB, 0xC8, 0x8E, 0x4A];
    let result = truncate_at_char_boundary(&bytes, 10);
    assert_eq!(result, bytes, "쌍둥이캣토시 should fit in 10 bytes");

    // 스케토우다라Jr = 3D 7F 8E 54 23 2D F0 3F F0 41 (10 bytes) → exactly fits
    let sketoudara = vec![0x3D, 0x7F, 0x8E, 0x54, 0x23, 0x2D, 0xF0, 0x3F, 0xF0, 0x41];
    let result2 = truncate_at_char_boundary(&sketoudara, 10);
    assert_eq!(result2, sketoudara, "스케토우다라Jr should fit in 10 bytes");
}

#[test]
fn truncate_empty_and_short() {
    assert_eq!(truncate_at_char_boundary(&[], 10), Vec::<u8>::new());
    assert_eq!(truncate_at_char_boundary(&[0x20], 10), vec![0x20]);
    assert_eq!(
        truncate_at_char_boundary(&[0xFB, 0x48], 10),
        vec![0xFB, 0x48]
    );
}

#[test]
fn battle_names_fit_in_slot() {
    let data = build_test_data();
    for (i, name) in data.battle_names.iter().enumerate() {
        assert!(
            name.len() <= BATTLE_NAME_MAX_LEN,
            "Monster #{} battle name {} bytes > max {}",
            i,
            name.len(),
            BATTLE_NAME_MAX_LEN
        );
    }
}
