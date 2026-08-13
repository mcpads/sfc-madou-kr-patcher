use super::*;

fn temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "madou_translation_growth_{}_{}_{}",
        label,
        std::process::id(),
        std::thread::current().name().unwrap_or("unnamed")
    ))
}

fn write_bank(dir: &Path, entries: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("bank_01_01.json"),
        format!(r#"{{"bank":"01","entries":[{}]}}"#, entries),
    )
    .unwrap();
}

#[test]
fn metrics_preserve_control_boundaries_and_ignore_branch_marker() {
    let jp_table = ko::build_jp_encode_table();
    let metrics = surface_metrics("{BOX:NPC}가 나{NL}다{SEP}라{PAGE}ざ", &jp_table).unwrap();

    assert_eq!(metrics.visible_cells, 5);
    assert_eq!(metrics.max_explicit_line_cells, 3);
    assert_eq!(metrics.segments, vec![vec![3, 1], vec![1], vec![0]]);
}

#[test]
fn metrics_accept_code_patch_hex_bytes_and_preserve_line_boundaries() {
    let jp_table = ko::build_jp_encode_table();
    let metrics =
        surface_metrics("{FC}{00}아르르{16}선생님{F9}안녕하세요{17}{FE}", &jp_table).unwrap();

    assert_eq!(metrics.visible_cells, 11);
    assert_eq!(metrics.max_explicit_line_cells, 6);
    assert_eq!(metrics.segments, vec![vec![6, 5], vec![0]]);
}

#[test]
fn audit_marks_only_confirmed_consumer_as_overflow() {
    let baseline = temp_dir("confirmed_baseline");
    let current = temp_dir("confirmed_current");
    write_bank(
        &baseline,
        r#"{"addr":"$01:B640","jp":"","ko":"이{NL}이이이이이이이이이이"}"#,
    );
    write_bank(
        &current,
        r#"{"addr":"$01:B640","jp":"","ko":"이{NL}이이이이이이이이이이이"}"#,
    );

    let audit = build_growth_audit(&baseline, &current).unwrap();

    assert_eq!(audit.summary.growth_candidates, 1);
    assert_eq!(audit.summary.crossed_common_10_cell_line, 1);
    assert_eq!(audit.summary.confirmed_fits, 0);
    assert_eq!(audit.summary.confirmed_new_overflows, 1);
    assert!(audit.candidates[0].crossed_common_10_cell_line);
    assert_eq!(
        audit.candidates[0].impact,
        GrowthImpact::ConfirmedNewOverflow
    );
    assert_eq!(audit.candidates[0].ambiguity, GrowthAmbiguity::Low);
    assert_eq!(
        audit.candidates[0].confirmed_consumer_profile.as_deref(),
        Some("fixed_rows_10x2")
    );

    std::fs::remove_dir_all(baseline).ok();
    std::fs::remove_dir_all(current).ok();
}

#[test]
fn audit_marks_diary_growth_inside_fifteen_cells_as_confirmed_fit() {
    let baseline = temp_dir("diary_fit_baseline");
    let current = temp_dir("diary_fit_current");
    write_bank(
        &baseline,
        r#"{"addr":"$03:D024","jp":"","ko":"이이이이이이이이이이"}"#,
    );
    write_bank(
        &current,
        r#"{"addr":"$03:D024","jp":"","ko":"이이이이이이이이이이이이이이이"}"#,
    );

    let audit = build_growth_audit(&baseline, &current).unwrap();

    assert_eq!(audit.summary.growth_candidates, 1);
    assert_eq!(audit.summary.confirmed_fits, 1);
    assert_eq!(audit.summary.confirmed_new_overflows, 0);
    assert_eq!(audit.candidates[0].impact, GrowthImpact::ConfirmedFits);
    assert_eq!(audit.candidates[0].ambiguity, GrowthAmbiguity::Low);
    assert_eq!(
        audit.candidates[0].confirmed_consumer_profile.as_deref(),
        Some("fixed_rows_15x5")
    );

    std::fs::remove_dir_all(baseline).ok();
    std::fs::remove_dir_all(current).ok();
}

#[test]
fn audit_keeps_unmapped_consumer_growth_as_candidate() {
    let baseline = temp_dir("candidate_baseline");
    let current = temp_dir("candidate_current");
    write_bank(
        &baseline,
        r#"{"addr":"$01:BABC","jp":"","ko":"이이이이이이이이이이"}"#,
    );
    write_bank(
        &current,
        r#"{"addr":"$01:BABC","jp":"","ko":"이이이이이이이이이이이"}"#,
    );

    let audit = build_growth_audit(&baseline, &current).unwrap();

    assert_eq!(audit.summary.growth_candidates, 1);
    assert_eq!(audit.summary.crossed_common_10_cell_line, 1);
    assert_eq!(audit.summary.confirmed_fits, 0);
    assert_eq!(audit.summary.confirmed_new_overflows, 0);
    assert_eq!(audit.candidates[0].impact, GrowthImpact::Candidate);
    assert_eq!(
        audit.candidates[0].ambiguity,
        GrowthAmbiguity::ConsumerNotMapped
    );
    assert!(audit.candidates[0].confirmed_consumer_profile.is_none());

    std::fs::remove_dir_all(baseline).ok();
    std::fs::remove_dir_all(current).ok();
}

#[test]
fn audit_includes_encyclopedia_and_code_patch_sources() {
    let baseline = temp_dir("other_baseline");
    let current = temp_dir("other_current");
    std::fs::create_dir_all(&baseline).unwrap();
    std::fs::create_dir_all(&current).unwrap();
    std::fs::write(
        baseline.join("encyclopedia.json"),
        r#"{"entries":[{"id":0,"type":"desc","addr":"$31:BDDF","ko":"짧아"}]}"#,
    )
    .unwrap();
    std::fs::write(
        current.join("encyclopedia.json"),
        r#"{"entries":[{"id":0,"type":"desc","addr":"$31:BDDF","ko":"조금 길어"}]}"#,
    )
    .unwrap();
    std::fs::write(
        baseline.join("code_patches.json"),
        r#"{"entries":[{"id":"save_prompt","pc_addr":"0x9763","ko":"저장"}]}"#,
    )
    .unwrap();
    std::fs::write(
        current.join("code_patches.json"),
        r#"{"entries":[{"id":"save_prompt","pc_addr":"0x9763","ko":"저장할래"}]}"#,
    )
    .unwrap();

    let audit = build_growth_audit(&baseline, &current).unwrap();

    assert_eq!(audit.summary.growth_candidates, 2);
    assert!(audit
        .candidates
        .iter()
        .any(|candidate| candidate.entry_id == "encyclopedia:0:desc"));
    assert!(audit
        .candidates
        .iter()
        .any(|candidate| candidate.entry_id == "code_patch:save_prompt"));

    std::fs::remove_dir_all(baseline).ok();
    std::fs::remove_dir_all(current).ok();
}
