use super::*;
use crate::patch::tracked_rom::TrackedRom;
use std::collections::HashMap;

fn test_ko_table() -> HashMap<char, Vec<u8>> {
    let mut t = HashMap::new();
    // Characters needed for code_patches.tsv test entries
    t.insert('\u{C138}', vec![0x70]); // 세
    t.insert('\u{C774}', vec![0x20]); // 이
    t.insert('\u{BE0C}', vec![0xC8]); // 브
    t.insert('\u{D560}', vec![0x65]); // 할
    t.insert('\u{B798}', vec![0x5C]); // 래
    t.insert('\u{C77C}', vec![0x6E]); // 일
    t
}

#[test]
fn parse_code_patches_tsv_basic() {
    let content = "# comment\n\
                    ID\tPC_ADDR\tSLOT_SIZE\tPREFIX_BYTES\tKO\tNOTES\n\
                    save_prompt\t0x9763\t9\t00 00\t세이브할래\ttest\n\
                    diary_label\t0x1CFBF\t3\t\t일\ttest\n";
    let entries = parse_code_patches_tsv(content).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].id, "save_prompt");
    assert_eq!(entries[0].pc_addr, 0x9763);
    assert_eq!(entries[0].slot_size, 9);
    assert_eq!(entries[0].prefix, vec![0x00, 0x00]);
    assert_eq!(entries[0].ko_text, "세이브할래");
    assert_eq!(entries[1].id, "diary_label");
    assert_eq!(entries[1].pc_addr, 0x1CFBF);
    assert_eq!(entries[1].slot_size, 3);
    assert!(entries[1].prefix.is_empty());
    assert_eq!(entries[1].ko_text, "일");
}

#[test]
fn parse_code_patches_tsv_skips_comments_and_header() {
    let content = "# this is a comment\n\
                    ID\tPC_ADDR\tSLOT_SIZE\tPREFIX_BYTES\tKO\tNOTES\n\
                    \n\
                    # another comment\n";
    let entries = parse_code_patches_tsv(content).unwrap();
    assert!(entries.is_empty());
}

#[test]
fn patch_code_strings_from_tsv_writes_correct_bytes() {
    let ko = test_ko_table();
    let mut rom = TrackedRom::new(vec![0xAA; 0x20000]);

    let dir = std::env::temp_dir().join("madou_test_code_patches");
    std::fs::create_dir_all(&dir).unwrap();
    let tsv_path = dir.join("code_patches.tsv");
    let tsv = "ID\tPC_ADDR\tSLOT_SIZE\tPREFIX_BYTES\tKO\tNOTES\n\
                save_prompt\t0x9763\t9\t00 00\t세이브할래\ttest\n\
                diary_label\t0x1CFBF\t3\t\t일\ttest\n";
    std::fs::write(&tsv_path, tsv).unwrap();

    let count = patch_code_strings_from_tsv(&mut rom, &tsv_path, &ko).unwrap();
    assert_eq!(count, 2);

    // save_prompt: prefix 00 00 + 세이브할래 (70 20 C8 65 5C) + FF FF = 9 bytes
    let save_pc = 0x9763;
    assert_eq!(
        &rom[save_pc..save_pc + 9],
        &[0x00, 0x00, 0x70, 0x20, 0xC8, 0x65, 0x5C, 0xFF, 0xFF]
    );
    // Byte before should be untouched
    assert_eq!(rom[save_pc - 1], 0xAA);
    // Byte after should be untouched
    assert_eq!(rom[save_pc + 9], 0xAA);

    // diary_label: no prefix + 일 (6E) + FF FF = 3 bytes
    let diary_pc = 0x1CFBF;
    assert_eq!(&rom[diary_pc..diary_pc + 3], &[0x6E, 0xFF, 0xFF]);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn patch_code_strings_from_tsv_rejects_overflow() {
    let ko = test_ko_table();
    let mut rom = TrackedRom::new(vec![0x00; 0x20000]);

    let dir = std::env::temp_dir().join("madou_test_code_overflow");
    std::fs::create_dir_all(&dir).unwrap();
    let tsv_path = dir.join("overflow.tsv");
    // Slot size 3, but prefix(2) + text(5) = 7 bytes > 3
    let tsv = "ID\tPC_ADDR\tSLOT_SIZE\tPREFIX_BYTES\tKO\tNOTES\n\
                too_big\t0x100\t3\t00 00\t세이브할래\ttest\n";
    std::fs::write(&tsv_path, tsv).unwrap();

    let result = patch_code_strings_from_tsv(&mut rom, &tsv_path, &ko);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("exceeds slot"));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn patch_code_byte_patches_encyclopedia() {
    let mut data = vec![0x00u8; 0x20000];

    // Plant JP encyclopedia data
    let enc_pc = 0x1B6B2;
    for i in 0..56 {
        data[enc_pc + i] = match i % 7 {
            1 | 3 | 5 => 0xC7,
            6 => 0xF9,
            _ => 0x00,
        };
    }
    data[enc_pc + 55] = 0xFF;
    data[0x1B6EA..0x1B6F0].copy_from_slice(&[0x6B, 0x38, 0x3C, 0x57, 0x7C, 0xFF]);

    let mut rom = TrackedRom::new(data);
    let count = patch_code_byte_patches(&mut rom);
    assert_eq!(count, 3); // enc_desc + enc_name + battle_suffix

    // All C7 should be replaced with 0E
    assert!(!rom[0x1B6B2..=0x1B6E9].contains(&0xC7));
    // Name: 0E x5 + FF
    assert_eq!(
        &rom[0x1B6EA..0x1B6F0],
        &[0x0E, 0x0E, 0x0E, 0x0E, 0x0E, 0xFF]
    );
    // Spaces and newlines preserved
    assert_eq!(rom[0x1B6B2], 0x00);
    assert_eq!(rom[0x1B6B8], 0xF9);
}

#[test]
fn patch_code_byte_patches_short_rom() {
    let mut rom = TrackedRom::new(vec![0x00u8; 0x100]);
    let count = patch_code_byte_patches(&mut rom);
    assert_eq!(count, 0);
}

#[test]
fn patch_removes_battle_suffix() {
    let mut data = vec![0x00u8; 0x20_0000];
    // Plant JP code: LDA #$5B / STA $0000,Y / INY
    data[0xAE89..0xAE8F].copy_from_slice(&[0xA9, 0x5B, 0x99, 0x00, 0x00, 0xC8]);
    let mut rom = TrackedRom::new(data);
    let count = patch_code_byte_patches(&mut rom);
    assert!(count >= 1);
    assert!(rom[0xAE89..0xAE8F].iter().all(|&b| b == 0xEA)); // NOP×6
}

// ── Stat level bar tests ─────────────────────────────────────

fn stat_ko_table() -> HashMap<char, Vec<u8>> {
    let mut t = HashMap::new();
    // Characters used in KO_STAT_LEVELS
    t.insert('약', vec![0xFB, 0x38]);
    t.insert('해', vec![0x3F]);
    t.insert('요', vec![0x34]);
    t.insert('아', vec![0x21]);
    t.insert('직', vec![0xFB, 0x08]);
    t.insert('강', vec![0xAE]);
    t.insert('한', vec![0x52]);
    t.insert('가', vec![0x29]);
    t.insert('조', vec![0xD8]);
    t.insert('금', vec![0xAA]);
    t.insert('꽤', vec![0xFB, 0xCF]);
    t.insert('제', vec![0x97]);
    t.insert('법', vec![0x91]);
    t.insert('나', vec![0x33]);
    t.insert('름', vec![0xFB, 0x55]);
    t.insert('매', vec![0xFB, 0x94]);
    t.insert('우', vec![0x54]);
    t.insert('엄', vec![0xFB, 0x25]);
    t.insert('청', vec![0xFB, 0x86]);
    t.insert('진', vec![0xCD]);
    t.insert('짜', vec![0xFB, 0x57]);
    t.insert('유', vec![0x9A]);
    t.insert('치', vec![0x60]);
    t.insert('원', vec![0x7A]);
    t.insert('최', vec![0xFB, 0xF1]);
    t.insert('무', vec![0x67]);
    t.insert('적', vec![0xBB]);
    t.insert('의', vec![0x28]);
    t.insert('장', vec![0x4E]);
    t
}

#[test]
fn char_to_tilemap_pair_fixed_encode() {
    let ko = stat_ko_table();
    assert_eq!(char_to_tilemap_pair(' ', &ko).unwrap(), [0x00, 0x00]);
    assert_eq!(char_to_tilemap_pair('~', &ko).unwrap(), [0x0C, 0x00]);
    assert_eq!(char_to_tilemap_pair('?', &ko).unwrap(), [0x0E, 0x00]);
    assert_eq!(char_to_tilemap_pair('!', &ko).unwrap(), [0x0B, 0x00]);
    assert_eq!(char_to_tilemap_pair('.', &ko).unwrap(), [0x0D, 0x00]);
    assert_eq!(char_to_tilemap_pair('3', &ko).unwrap(), [0x04, 0x00]);
}

#[test]
fn char_to_tilemap_pair_single_byte() {
    let ko = stat_ko_table();
    assert_eq!(char_to_tilemap_pair('해', &ko).unwrap(), [0x3F, 0x00]);
    assert_eq!(char_to_tilemap_pair('강', &ko).unwrap(), [0xAE, 0x00]);
}

#[test]
fn char_to_tilemap_pair_fb_prefix() {
    let ko = stat_ko_table();
    assert_eq!(char_to_tilemap_pair('약', &ko).unwrap(), [0x38, 0x01]);
    assert_eq!(char_to_tilemap_pair('꽤', &ko).unwrap(), [0xCF, 0x01]);
    assert_eq!(char_to_tilemap_pair('최', &ko).unwrap(), [0xF1, 0x01]);
}

#[test]
fn patch_stat_level_bar_writes_correct_data() {
    let ko = stat_ko_table();
    let mut rom = TrackedRom::new(vec![0xAA; 0x10000]);

    let count = patch_stat_level_bar(&mut rom, &ko).unwrap();
    assert_eq!(count, 12);

    let pc = STAT_BAR_PC;

    // Entry 0: 약해요~(sp)(sp)
    assert_eq!(
        &rom[pc..pc + 12],
        &[0x38, 0x01, 0x3F, 0x00, 0x34, 0x00, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x00]
    );

    // Entry 4 (나름 강해 sp): verify reordered position
    let e4 = pc + 4 * 12;
    assert_eq!(
        &rom[e4..e4 + 12],
        &[0x33, 0x00, 0x55, 0x01, 0x00, 0x00, 0xAE, 0x00, 0x3F, 0x00, 0x00, 0x00]
    );

    // Entry 10 (유치원 최강): no trailing space
    let e10 = pc + 10 * 12;
    assert_eq!(
        &rom[e10..e10 + 12],
        &[0x9A, 0x00, 0x60, 0x00, 0x7A, 0x00, 0x00, 0x00, 0xF1, 0x01, 0xAE, 0x00]
    );

    // Entry 11 (무적의 원장)
    let e11 = pc + 11 * 12;
    assert_eq!(
        &rom[e11..e11 + 12],
        &[0x67, 0x00, 0xBB, 0x00, 0x28, 0x00, 0x00, 0x00, 0x7A, 0x00, 0x4E, 0x00]
    );

    // Total size: 12 * 12 = 144 bytes, next byte untouched
    assert_eq!(rom[pc + 144], 0xAA);
}

#[test]
fn stat_levels_all_six_chars() {
    // Verify all 12 entries have exactly 6 characters
    for (i, &text) in KO_STAT_LEVELS.iter().enumerate() {
        let count = text.chars().count();
        assert_eq!(count, 6, "Entry {}: {:?} has {} chars", i, text, count);
    }
}
