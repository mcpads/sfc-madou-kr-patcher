use super::*;

fn test_ko_table() -> HashMap<char, Vec<u8>> {
    let mut t = HashMap::new();
    t.insert('\u{C774}', vec![0x20]); // 이
    t.insert('\u{C81C}', vec![0x97]); // 제
    t.insert('\u{C644}', vec![0xFB, 0xEB]); // 완
    t.insert('\u{C804}', vec![0x6C]); // 전
    t.insert('\u{D788}', vec![0xA0]); // 히
    t.insert('\u{C548}', vec![0x49]); // 안
    t.insert('\u{B3FC}', vec![0xFB, 0x24]); // 돼
    t.insert('\u{C138}', vec![0x70]); // 세
    t.insert('\u{BE0C}', vec![0xC8]); // 브
    t.insert('\u{D560}', vec![0x65]); // 할
    t.insert('\u{B798}', vec![0x5C]); // 래
    t.insert('\u{C77C}', vec![0x6E]); // 일
    t
}

#[test]
fn load_bank_tsv_ff_terminated() {
    let dir = std::env::temp_dir().join("madou_test_translation");
    std::fs::create_dir_all(&dir).unwrap();

    let tsv = "# Bank $01\n\
                ADDR\tCATEGORY\tJP\tKO\tNOTES\n\
                $01:B400\tHP_STATUS\tもう　ぜんぜんダメ！\t이제 완전히 안돼!\t\n";
    std::fs::write(dir.join("bank_01.tsv"), tsv).unwrap();

    let ko = test_ko_table();
    let entries = load_and_encode_bank(&dir, "01", &ko, false).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].addr, SnesAddr::new(0x01, 0xB400));
    // "이제 완전히 안돼!" = 10 chars exactly, no padding
    // 20 97 18 FB EB 6C A0 18 49 FB 24 0B FF
    assert_eq!(entries[0].encoded.last(), Some(&0xFF)); // FF terminated
    assert_eq!(entries[0].encoded[0], 0x20); // 이

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn load_bank_tsv_fc_split_has_ff() {
    let dir = std::env::temp_dir().join("madou_test_fc_split");
    std::fs::create_dir_all(&dir).unwrap();

    let tsv = "ADDR\tCATEGORY\tJP\tKO\tNOTES\n\
                $2B:8000\tSTORY\ttest\t이제\t\n";
    std::fs::write(dir.join("bank_2B.tsv"), tsv).unwrap();

    let ko = test_ko_table();
    let entries = load_and_encode_bank(&dir, "2B", &ko, true).unwrap();
    assert_eq!(entries.len(), 1);
    // FC-split banks also get FF terminator (matching original JP ROM)
    assert_eq!(entries[0].encoded.last(), Some(&0xFF));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn load_bank_tsv_skips_empty_ko() {
    let dir = std::env::temp_dir().join("madou_test_empty_ko");
    std::fs::create_dir_all(&dir).unwrap();

    let tsv = "ADDR\tCATEGORY\tJP\tKO\tNOTES\n\
                $01:B400\tSTATUS\ttest\t이제\t\n\
                $01:B40C\tSTATUS\ttest\t\t\n";
    std::fs::write(dir.join("bank_01.tsv"), tsv).unwrap();

    let ko = test_ko_table();
    let entries = load_and_encode_bank(&dir, "01", &ko, false).unwrap();
    assert_eq!(entries.len(), 1); // second entry skipped (empty KO)

    std::fs::remove_dir_all(&dir).ok();
}
