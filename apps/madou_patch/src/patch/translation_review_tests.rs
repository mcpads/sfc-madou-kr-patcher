use super::{
    contains_japanese_kana, count_tag, korean_surface_candidates, sorted_bank_json_files,
    strip_braced_tokens, trailing_page_kana,
};

#[test]
fn bank_file_discovery_excludes_non_bank_json() {
    let dir = std::env::temp_dir().join(format!(
        "madou_translation_review_{}_{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("bank_2B_02.json"), "{}").unwrap();
    std::fs::write(dir.join("bank_01_01.json"), "{}").unwrap();
    std::fs::write(dir.join("encyclopedia.json"), "{}").unwrap();

    let files = sorted_bank_json_files(&dir).unwrap();
    let names = files
        .iter()
        .map(|path| path.file_name().unwrap().to_str().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(names, vec!["bank_01_01.json", "bank_2B_02.json"]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn candidate_helpers_ignore_speaker_tags_but_detect_visible_kana() {
    assert_eq!(strip_braced_tokens("{BOX:アルル}안녕"), "안녕");
    assert_eq!(strip_braced_tokens("하고{NL}있었어"), "하고 있었어");
    assert!(!contains_japanese_kana("{BOX:アルル}안녕"));
    assert!(contains_japanese_kana("{BOX:アルル}안녕ケ"));
}

#[test]
fn tag_counter_distinguishes_exact_and_argument_forms() {
    assert_eq!(count_tag("{BOX:アルル}a{BOX:NPC}b", "BOX"), 2);
    assert_eq!(count_tag("{SEP}a{SEP}", "SEP"), 2);
}

#[test]
fn trailing_page_kana_detects_unmodeled_dispatch_byte() {
    assert_eq!(trailing_page_kana("선택{PAGE}ざ"), Some('ざ'));
    assert_eq!(trailing_page_kana("선택{PAGE}"), None);
    assert_eq!(trailing_page_kana("선택{PAGE}가"), None);
}

#[test]
fn korean_surface_candidates_report_current_spacing_forms() {
    assert_eq!(
        korean_surface_candidates("원장선생님을 만날수 있어"),
        vec![("만날수", "만날 수"), ("원장선생님", "원장 선생님")]
    );
    assert!(korean_surface_candidates("원장 선생님을 만날 수 있어").is_empty());
    assert!(korean_surface_candidates(&strip_braced_tokens("하고{NL}있었어")).is_empty());
}
