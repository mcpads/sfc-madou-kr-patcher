//! Generate a combined JP-KR review view without changing translation SSOT files.

use crate::text;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct ReviewSummary {
    pub total_entries: usize,
    pub bank_entries: usize,
    pub encyclopedia_entries: usize,
    pub code_patch_entries: usize,
    pub rom_matches: usize,
    pub derived_subentries: usize,
    pub rom_mismatches: usize,
    pub untranslated: usize,
    pub tier_a_certain: usize,
    pub tier_b_strong: usize,
    pub tier_c_context: usize,
    pub tier_d_no_signal: usize,
}

#[derive(Debug, Serialize)]
struct ReviewFile {
    schema_version: u8,
    summary: ReviewSummary,
    entries: Vec<ReviewEntry>,
}

#[derive(Debug, Serialize)]
struct ReviewEntry {
    entry_id: String,
    source: String,
    address: String,
    category: String,
    jp: String,
    ko: String,
    notes: String,
    source_check: String,
    candidate_tier: String,
    candidate_signals: Vec<CandidateSignal>,
    review_status: String,
    review_notes: String,
}

#[derive(Debug, Clone, Serialize)]
struct CandidateSignal {
    kind: String,
    certainty: String,
    ambiguity: String,
    detail: String,
}

#[derive(Deserialize)]
struct BankFile {
    bank: String,
    entries: Vec<BankEntry>,
}

#[derive(Deserialize)]
struct BankEntry {
    addr: String,
    jp: String,
    ko: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    notes: String,
}

#[derive(Deserialize)]
struct EncyclopediaFile {
    entries: Vec<EncyclopediaEntry>,
}

#[derive(Deserialize)]
struct EncyclopediaEntry {
    id: usize,
    #[serde(default)]
    addr: String,
    #[serde(rename = "type")]
    entry_type: String,
    jp: String,
    ko: String,
    #[serde(default)]
    notes: String,
}

#[derive(Deserialize)]
struct CodePatchFile {
    entries: Vec<CodePatchEntry>,
}

#[derive(Deserialize)]
struct CodePatchEntry {
    id: String,
    pc_addr: String,
    #[serde(default)]
    jp: String,
    ko: String,
    #[serde(default)]
    notes: String,
}

fn extracted_jp_by_address(rom: &[u8]) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    for config in text::control::KNOWN_BANKS {
        for entry in text::bank::extract_bank(rom, config) {
            result.insert(
                format!("${:02X}:{:04X}", entry.bank, entry.snes_addr),
                text::bank::text_to_json_convention(&entry.text),
            );
        }
    }
    result
}

fn sorted_bank_json_files(dir: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let mut files = std::fs::read_dir(dir)
        .map_err(|e| format!("Failed to read '{}': {}", dir.display(), e))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().and_then(|ext| ext.to_str()) == Some("json")
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("bank_"))
        })
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read '{}': {}", path.display(), e))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("JSON parse error in '{}': {}", path.display(), e))
}

fn strip_braced_tokens(text: &str) -> String {
    let mut result = String::new();
    let mut in_token = false;
    let mut token_after_visible_text = false;
    for ch in text.chars() {
        match ch {
            '{' if !in_token => {
                in_token = true;
                token_after_visible_text = result
                    .chars()
                    .next_back()
                    .is_some_and(|last| !last.is_whitespace());
            }
            '}' if in_token => in_token = false,
            _ if !in_token => {
                if token_after_visible_text
                    && !ch.is_whitespace()
                    && result
                        .chars()
                        .next_back()
                        .is_some_and(|last| !last.is_whitespace())
                {
                    result.push(' ');
                }
                token_after_visible_text = false;
                result.push(ch);
            }
            _ => {}
        }
    }
    result
}

fn contains_japanese_kana(text: &str) -> bool {
    strip_braced_tokens(text)
        .chars()
        .any(|ch| matches!(ch as u32, 0x3040..=0x30FF))
}

fn trailing_page_kana(text: &str) -> Option<char> {
    let page = text.rfind("{PAGE}")?;
    let tail = &text[page + "{PAGE}".len()..];
    let mut chars = tail.chars();
    let ch = chars.next()?;
    (chars.next().is_none() && matches!(ch as u32, 0x3040..=0x30FF)).then_some(ch)
}

fn count_tag(text: &str, tag: &str) -> usize {
    let exact = format!("{{{}}}", tag);
    let with_arg = format!("{{{}:", tag);
    text.match_indices(&exact).count() + text.match_indices(&with_arg).count()
}

fn visible_len(text: &str) -> usize {
    strip_braced_tokens(text)
        .chars()
        .filter(|ch| {
            ch.is_alphanumeric()
                || matches!(*ch as u32, 0x3040..=0x30FF | 0x3400..=0x9FFF | 0xAC00..=0xD7A3)
        })
        .count()
}

fn glossary_pairs(path: &Path) -> Result<Vec<(String, String)>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read '{}': {}", path.display(), e))?;
    let mut pairs = BTreeSet::new();
    for line in content.lines().filter(|line| line.starts_with('|')) {
        let columns = line
            .trim_matches('|')
            .split('|')
            .map(|cell| cell.trim().trim_matches('*'))
            .collect::<Vec<_>>();
        if columns.len() < 2
            || columns[0] == "JP"
            || columns[1].starts_with("KO")
            || columns[0].chars().count() < 4
            || columns[0].chars().all(|ch| matches!(ch, '-' | ':' | ' '))
        {
            continue;
        }
        pairs.insert((columns[0].to_string(), columns[1].to_string()));
    }
    Ok(pairs.into_iter().collect())
}

fn korean_surface_candidates(text: &str) -> Vec<(&'static str, &'static str)> {
    const SURFACES: &[(&str, &str)] = &[
        ("안돼", "안 돼"),
        ("아파보", "아파 보"),
        ("볼수", "볼 수"),
        ("갈수", "갈 수"),
        ("쉴수", "쉴 수"),
        ("읽을수", "읽을 수"),
        ("돌아갈수", "돌아갈 수"),
        ("이길수", "이길 수"),
        ("될수", "될 수"),
        ("받을수", "받을 수"),
        ("만날수", "만날 수"),
        ("들어올수", "들어올 수"),
        ("올라갈수", "올라갈 수"),
        ("용서못", "용서 못"),
        ("용서안", "용서 안"),
        ("신경쓰", "신경 쓰"),
        ("알고있", "알고 있"),
        ("보고있", "보고 있"),
        ("자고있", "자고 있"),
        ("지키고있", "지키고 있"),
        ("죽어있", "죽어 있"),
        ("하고있", "하고 있"),
        ("있을듯", "있을 듯"),
        ("안할", "안 할"),
        ("안가져", "안 가져"),
        ("안준", "안 준"),
        ("것같", "것 같"),
        ("거같", "거 같"),
        ("나봐", "나 봐"),
        ("하나봐", "하나 봐"),
        ("라는게", "라는 게"),
        ("보는게", "보는 게"),
        ("있는거", "있는 거"),
        ("말하는거", "말하는 거"),
        ("좋아하시는건", "좋아하시는 건"),
        ("더지나면", "더 지나면"),
        ("무슨일", "무슨 일"),
        ("낮잠자", "낮잠 자"),
        ("지상최강", "지상 최강"),
        ("술마", "술 마"),
        ("신비석을주", "신비석을 주"),
        ("도와주지않", "도와주지 않"),
        ("원장선생님", "원장 선생님"),
        ("전망바위산", "전망 바위산"),
        ("쌍둥이바위", "쌍둥이 바위"),
        ("반딧불알", "반딧불 알"),
        ("마왕팔찌", "마왕 팔찌"),
        ("여행의행복", "여행의 행복"),
        ("빛의구슬", "빛의 구슬"),
    ];

    SURFACES
        .iter()
        .copied()
        .filter(|(surface, _)| text.contains(surface))
        .collect()
}

fn push_signal(
    entry: &mut ReviewEntry,
    kind: &str,
    certainty: &str,
    ambiguity: &str,
    detail: String,
) {
    entry.candidate_signals.push(CandidateSignal {
        kind: kind.to_string(),
        certainty: certainty.to_string(),
        ambiguity: ambiguity.to_string(),
        detail,
    });
}

fn assign_candidate_signals(
    entries: &mut [ReviewEntry],
    glossary_path: &Path,
) -> Result<(), String> {
    let glossary = glossary_pairs(glossary_path)?;
    let mut repeated: HashMap<String, BTreeSet<String>> = HashMap::new();
    for entry in entries.iter().filter(|entry| !entry.ko.is_empty()) {
        repeated
            .entry(entry.jp.clone())
            .or_default()
            .insert(entry.ko.clone());
    }

    for entry in entries.iter_mut() {
        if matches!(
            entry.source_check.as_str(),
            "rom_mismatch" | "not_extracted"
        ) {
            push_signal(
                entry,
                "source_baseline_mismatch",
                "high",
                "low",
                format!("source_check={}", entry.source_check),
            );
        }
        if entry.ko.is_empty() {
            push_signal(
                entry,
                "empty_korean",
                "high",
                "low",
                "KO is empty; classify as untranslated text or approved non-text noise".to_string(),
            );
        }
        if let Some(ch) = trailing_page_kana(&entry.ko) {
            push_signal(
                entry,
                "unmodeled_choice_dispatch_byte",
                "high",
                "medium",
                format!(
                    "Kana '{}' remains immediately after PAGE; model it as a protected raw/script token if it is not visible text",
                    ch
                ),
            );
        } else if contains_japanese_kana(&entry.ko) {
            push_signal(
                entry,
                "jp_residue_or_raw_byte",
                "high",
                "high",
                "KO contains kana outside braced tokens; it may be visible residue or an unmodeled script byte"
                    .to_string(),
            );
        }

        let protected = ["BOX", "CHOICE"]
            .into_iter()
            .filter_map(|tag| {
                let jp = count_tag(&entry.jp, tag);
                let ko = count_tag(&entry.ko, tag);
                (jp != ko).then_some(format!("{} {}→{}", tag, jp, ko))
            })
            .collect::<Vec<_>>();
        if !protected.is_empty() && !entry.ko.is_empty() {
            push_signal(
                entry,
                "protected_control_mismatch",
                "high",
                "low",
                protected.join(", "),
            );
        }

        let layout = ["SEP", "PAGE"]
            .into_iter()
            .filter_map(|tag| {
                let jp = count_tag(&entry.jp, tag);
                let ko = count_tag(&entry.ko, tag);
                (jp != ko).then_some(format!("{} {}→{}", tag, jp, ko))
            })
            .collect::<Vec<_>>();
        if !layout.is_empty() && !entry.ko.is_empty() {
            push_signal(
                entry,
                "layout_control_reflow",
                "medium",
                "high",
                layout.join(", "),
            );
        }

        let jp_visible = strip_braced_tokens(&entry.jp);
        let ko_visible = strip_braced_tokens(&entry.ko);
        let spacing_candidates = korean_surface_candidates(&ko_visible);
        if !spacing_candidates.is_empty() {
            push_signal(
                entry,
                "korean_surface_spacing",
                "high",
                "low",
                spacing_candidates
                    .into_iter()
                    .map(|(from, to)| format!("{} → {}", from, to))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        for (jp_term, ko_term) in &glossary {
            if jp_visible.contains(jp_term) && !ko_visible.contains(ko_term) {
                push_signal(
                    entry,
                    "glossary_surface_drift",
                    "medium",
                    "medium",
                    format!("{} → expected surface {}", jp_term, ko_term),
                );
            }
        }

        if repeated
            .get(&entry.jp)
            .is_some_and(|translations| translations.len() > 1)
        {
            push_signal(
                entry,
                "same_jp_multiple_ko",
                "medium",
                "high",
                "The same JP source has multiple KO renderings; context may justify the difference"
                    .to_string(),
            );
        }

        let jp_len = visible_len(&entry.jp);
        let ko_len = visible_len(&entry.ko);
        if jp_len >= 8 && ko_len > 0 {
            let ratio = ko_len as f64 / jp_len as f64;
            if !(0.38..=1.60).contains(&ratio) {
                push_signal(
                    entry,
                    "length_ratio_outlier",
                    "low",
                    "high",
                    format!("visible KO/JP length ratio={:.2}", ratio),
                );
            }
        }

        entry.candidate_tier = if entry.candidate_signals.iter().any(|signal| {
            matches!(
                signal.kind.as_str(),
                "source_baseline_mismatch" | "empty_korean" | "protected_control_mismatch"
            )
        }) {
            "A_certain_machine_issue"
        } else if entry.candidate_signals.iter().any(|signal| {
            matches!(
                signal.kind.as_str(),
                "jp_residue_or_raw_byte"
                    | "unmodeled_choice_dispatch_byte"
                    | "korean_surface_spacing"
            )
        }) {
            "B_strong_candidate"
        } else if !entry.candidate_signals.is_empty() {
            "C_context_required"
        } else {
            "D_no_signal"
        }
        .to_string();
    }
    Ok(())
}

pub fn write_review_json(
    rom: &[u8],
    translations_dir: &Path,
    output_path: &Path,
) -> Result<ReviewSummary, String> {
    let extracted = extracted_jp_by_address(rom);
    let mut entries = Vec::new();
    let mut seen_ids = HashSet::new();
    let mut bank_entries = 0;
    let mut rom_matches = 0;
    let mut derived_subentries = 0;
    let mut rom_mismatches = 0;
    let mut untranslated = 0;

    for path in sorted_bank_json_files(translations_dir)? {
        let file: BankFile = read_json(&path)?;
        for entry in file.entries {
            let entry_id = format!("bank/{}", entry.addr);
            if !seen_ids.insert(entry_id.clone()) {
                return Err(format!("Duplicate review entry ID: {}", entry_id));
            }
            if !entry.addr.starts_with(&format!("${}:", file.bank)) {
                return Err(format!(
                    "{}: address {} does not match bank {}",
                    path.display(),
                    entry.addr,
                    file.bank
                ));
            }

            let source_check = match extracted.get(&entry.addr) {
                Some(jp) if jp == &entry.jp => {
                    rom_matches += 1;
                    "rom_exact"
                }
                Some(_) => {
                    rom_mismatches += 1;
                    "rom_mismatch"
                }
                None if entry.notes.contains("derived sub-entry") => {
                    derived_subentries += 1;
                    "derived_subentry"
                }
                None => {
                    rom_mismatches += 1;
                    "not_extracted"
                }
            };
            let review_status = if entry.ko.is_empty() {
                untranslated += 1;
                "untranslated"
            } else {
                "needs_review"
            };
            entries.push(ReviewEntry {
                entry_id,
                source: "bank_json".to_string(),
                address: entry.addr,
                category: entry.category,
                jp: entry.jp,
                ko: entry.ko,
                notes: entry.notes,
                source_check: source_check.to_string(),
                candidate_tier: String::new(),
                candidate_signals: Vec::new(),
                review_status: review_status.to_string(),
                review_notes: String::new(),
            });
            bank_entries += 1;
        }
    }

    let encyclopedia_path = translations_dir.join("encyclopedia.json");
    let encyclopedia: EncyclopediaFile = read_json(&encyclopedia_path)?;
    let encyclopedia_entries = encyclopedia.entries.len();
    for entry in encyclopedia.entries {
        let entry_id = format!("encyclopedia/{:02}/{}", entry.id, entry.entry_type);
        entries.push(ReviewEntry {
            entry_id,
            source: "encyclopedia_json".to_string(),
            address: entry.addr,
            category: format!("ENCYCLOPEDIA_{}", entry.entry_type.to_uppercase()),
            jp: entry.jp,
            ko: entry.ko,
            notes: entry.notes,
            source_check: "separate_runtime_path".to_string(),
            candidate_tier: String::new(),
            candidate_signals: Vec::new(),
            review_status: "needs_review".to_string(),
            review_notes: String::new(),
        });
    }

    let code_patch_path = translations_dir.join("code_patches.json");
    let code_patches: CodePatchFile = read_json(&code_patch_path)?;
    let code_patch_entries = code_patches.entries.len();
    for entry in code_patches.entries {
        let entry_id = format!("code_patch/{}", entry.id);
        entries.push(ReviewEntry {
            entry_id,
            source: "code_patches_json".to_string(),
            address: entry.pc_addr,
            category: "CODE_PATCH".to_string(),
            jp: entry.jp,
            ko: entry.ko,
            notes: entry.notes,
            source_check: "separate_runtime_path".to_string(),
            candidate_tier: String::new(),
            candidate_signals: Vec::new(),
            review_status: "needs_review".to_string(),
            review_notes: String::new(),
        });
    }

    assign_candidate_signals(&mut entries, &translations_dir.join("glossary.md"))?;
    let tier_a_certain = entries
        .iter()
        .filter(|entry| entry.candidate_tier == "A_certain_machine_issue")
        .count();
    let tier_b_strong = entries
        .iter()
        .filter(|entry| entry.candidate_tier == "B_strong_candidate")
        .count();
    let tier_c_context = entries
        .iter()
        .filter(|entry| entry.candidate_tier == "C_context_required")
        .count();
    let tier_d_no_signal = entries
        .iter()
        .filter(|entry| entry.candidate_tier == "D_no_signal")
        .count();
    let summary = ReviewSummary {
        total_entries: entries.len(),
        bank_entries,
        encyclopedia_entries,
        code_patch_entries,
        rom_matches,
        derived_subentries,
        rom_mismatches,
        untranslated,
        tier_a_certain,
        tier_b_strong,
        tier_c_context,
        tier_d_no_signal,
    };
    let review = ReviewFile {
        schema_version: 1,
        summary,
        entries,
    };
    let json = serde_json::to_string_pretty(&review)
        .map_err(|e| format!("Failed to serialize review JSON: {}", e))?;
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create '{}': {}", parent.display(), e))?;
    }
    std::fs::write(output_path, format!("{}\n", json))
        .map_err(|e| format!("Failed to write '{}': {}", output_path.display(), e))?;

    Ok(review.summary)
}

#[cfg(test)]
#[path = "translation_review_tests.rs"]
mod tests;
