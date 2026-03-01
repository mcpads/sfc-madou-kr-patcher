use super::*;
use std::collections::HashMap;

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
fn load_bank_json_basic() {
    let dir = std::env::temp_dir().join("madou_test_json_basic");
    std::fs::create_dir_all(&dir).unwrap();

    let json = r#"{
        "bank": "01",
        "entries": [
            {
                "addr": "$01:B400",
                "jp": "もう　ぜんぜんダメ！",
                "ko": "이제 완전히 안돼!",
                "category": "HP_STATUS",
                "notes": ""
            }
        ]
    }"#;
    std::fs::write(dir.join("bank_01_01.json"), json).unwrap();

    let ko = test_ko_table();
    let entries = load_bank_json_chunks(&dir, "01", &ko, false).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].addr, SnesAddr::new(0x01, 0xB400));
    assert_eq!(entries[0].encoded.last(), Some(&0xFF));
    assert_eq!(entries[0].encoded[0], 0x20); // 이

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn load_bank_json_multiple_chunks() {
    let dir = std::env::temp_dir().join("madou_test_json_multi");
    std::fs::create_dir_all(&dir).unwrap();

    let json1 = r#"{
        "bank": "01",
        "entries": [
            { "addr": "$01:B40C", "jp": "test", "ko": "이제" }
        ]
    }"#;
    let json2 = r#"{
        "bank": "01",
        "entries": [
            { "addr": "$01:B400", "jp": "test", "ko": "이제" }
        ]
    }"#;
    std::fs::write(dir.join("bank_01_01.json"), json1).unwrap();
    std::fs::write(dir.join("bank_01_02.json"), json2).unwrap();

    let ko = test_ko_table();
    let entries = load_bank_json_chunks(&dir, "01", &ko, false).unwrap();
    assert_eq!(entries.len(), 2);
    // Sorted by address
    assert_eq!(entries[0].addr, SnesAddr::new(0x01, 0xB400));
    assert_eq!(entries[1].addr, SnesAddr::new(0x01, 0xB40C));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn load_bank_json_skips_empty_ko() {
    let dir = std::env::temp_dir().join("madou_test_json_skip");
    std::fs::create_dir_all(&dir).unwrap();

    let json = r#"{
        "bank": "01",
        "entries": [
            { "addr": "$01:B400", "jp": "test", "ko": "이제" },
            { "addr": "$01:B40C", "jp": "test", "ko": "" }
        ]
    }"#;
    std::fs::write(dir.join("bank_01_01.json"), json).unwrap();

    let ko = test_ko_table();
    let entries = load_bank_json_chunks(&dir, "01", &ko, false).unwrap();
    assert_eq!(entries.len(), 1);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn load_bank_json_detects_duplicate_addr() {
    let dir = std::env::temp_dir().join("madou_test_json_dup");
    std::fs::create_dir_all(&dir).unwrap();

    let json1 = r#"{
        "bank": "01",
        "entries": [
            { "addr": "$01:B400", "jp": "test", "ko": "이제" }
        ]
    }"#;
    let json2 = r#"{
        "bank": "01",
        "entries": [
            { "addr": "$01:B400", "jp": "test", "ko": "이제" }
        ]
    }"#;
    std::fs::write(dir.join("bank_01_01.json"), json1).unwrap();
    std::fs::write(dir.join("bank_01_02.json"), json2).unwrap();

    let ko = test_ko_table();
    let result = load_bank_json_chunks(&dir, "01", &ko, false);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Duplicate address"));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn glob_bank_json_sorted() {
    let dir = std::env::temp_dir().join("madou_test_glob");
    std::fs::create_dir_all(&dir).unwrap();

    std::fs::write(dir.join("bank_2B_03.json"), "{}").unwrap();
    std::fs::write(dir.join("bank_2B_01.json"), "{}").unwrap();
    std::fs::write(dir.join("bank_2B_02.json"), "{}").unwrap();
    std::fs::write(dir.join("bank_01_01.json"), "{}").unwrap(); // different bank

    let files = glob_bank_json(&dir, "2B");
    assert_eq!(files.len(), 3);
    assert!(files[0]
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .contains("bank_2B_01"));
    assert!(files[1]
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .contains("bank_2B_02"));
    assert!(files[2]
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .contains("bank_2B_03"));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn load_code_patches_json_basic() {
    let dir = std::env::temp_dir().join("madou_test_json_code");
    std::fs::create_dir_all(&dir).unwrap();

    let json = r#"{
        "entries": [
            {
                "id": "save_prompt",
                "pc_addr": "0x9763",
                "slot_size": 9,
                "prefix_bytes": "00 00",
                "ko": "세이브할래",
                "notes": ""
            },
            {
                "id": "diary_label",
                "pc_addr": "0x1CFBF",
                "slot_size": 3,
                "prefix_bytes": "",
                "ko": "일",
                "notes": ""
            }
        ]
    }"#;
    let path = dir.join("code_patches.json");
    std::fs::write(&path, json).unwrap();

    let entries = load_code_patches_json(&path).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].id, "save_prompt");
    assert_eq!(entries[0].pc_addr, 0x9763);
    assert_eq!(entries[0].slot_size, 9);
    assert_eq!(entries[0].prefix, vec![0x00, 0x00]);
    assert_eq!(entries[0].ko_text, "세이브할래");
    assert_eq!(entries[1].id, "diary_label");
    assert_eq!(entries[1].pc_addr, 0x1CFBF);
    assert!(entries[1].prefix.is_empty());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn load_encyclopedia_json_with_jp_field() {
    // Verify that encyclopedia entries with jp/notes fields parse correctly
    let dir = std::env::temp_dir().join("madou_test_enc_jp");
    std::fs::create_dir_all(&dir).unwrap();

    let json = r#"{
        "entries": [
            { "id": 0, "type": "name", "loc_idx": 0, "jp": "スキヤポデス", "ko": "스키야보데스", "notes": "" },
            { "id": 0, "type": "desc", "loc_idx": 1, "jp": "", "ko": "테스트 설명", "notes": "note" }
        ]
    }"#;
    let path = dir.join("encyclopedia.json");
    std::fs::write(&path, json).unwrap();

    let parsed: EncyclopediaJsonFile =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(parsed.entries.len(), 2);
    assert_eq!(parsed.entries[0].jp, "スキヤポデス");
    assert_eq!(parsed.entries[0].notes, "");
    assert_eq!(parsed.entries[1].jp, "");
    assert_eq!(parsed.entries[1].notes, "note");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn load_bank_json_optional_fields() {
    let dir = std::env::temp_dir().join("madou_test_json_opt");
    std::fs::create_dir_all(&dir).unwrap();

    // No category or notes fields
    let json = r#"{
        "bank": "01",
        "entries": [
            { "addr": "$01:B400", "jp": "test", "ko": "이제" }
        ]
    }"#;
    std::fs::write(dir.join("bank_01_01.json"), json).unwrap();

    let ko = test_ko_table();
    let entries = load_bank_json_chunks(&dir, "01", &ko, false).unwrap();
    assert_eq!(entries.len(), 1);

    std::fs::remove_dir_all(&dir).ok();
}
