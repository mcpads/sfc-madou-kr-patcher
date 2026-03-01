//! JSON-based translation file loading.
//!
//! Loads chunked `bank_{id}_{NN}.json` files, `encyclopedia.json`,
//! and `code_patches.json` as replacements for the legacy TSV format.

use crate::encoding::ko;
use crate::patch::encyclopedia::EncyclopediaData;
use crate::patch::translation::TranslationEntry;
use crate::rom::SnesAddr;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

// ── Bank translation JSON ────────────────────────────────────────

#[derive(Deserialize)]
pub struct BankTranslationFile {
    #[allow(dead_code)]
    pub bank: String,
    pub entries: Vec<BankJsonEntry>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct BankJsonEntry {
    pub addr: String,
    pub jp: String,
    pub ko: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub notes: String,
}

/// Find all `bank_{id}_*.json` files in `dir`, sorted by filename.
pub fn glob_bank_json(dir: &Path, bank_id: &str) -> Vec<std::path::PathBuf> {
    let prefix = format!("bank_{}_", bank_id);
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension().map(|e| e == "json").unwrap_or(false)
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with(&prefix))
                    .unwrap_or(false)
        })
        .collect();
    files.sort();
    files
}

/// Load all JSON chunks for a bank and encode KO text.
pub fn load_bank_json_chunks(
    dir: &Path,
    bank_id: &str,
    ko_table: &HashMap<char, Vec<u8>>,
    _fc_split: bool,
) -> Result<Vec<TranslationEntry>, String> {
    let files = glob_bank_json(dir, bank_id);
    if files.is_empty() {
        return Err(format!(
            "No bank_{}_*.json files found in {}",
            bank_id,
            dir.display()
        ));
    }

    let mut entries = Vec::new();
    let mut seen_addrs = std::collections::HashSet::new();

    for file_path in &files {
        let content = std::fs::read_to_string(file_path)
            .map_err(|e| format!("Failed to read '{}': {}", file_path.display(), e))?;
        let file: BankTranslationFile = serde_json::from_str(&content)
            .map_err(|e| format!("JSON parse error in '{}': {}", file_path.display(), e))?;

        for (i, entry) in file.entries.iter().enumerate() {
            if entry.ko.is_empty() {
                continue; // skip untranslated
            }

            let addr = SnesAddr::parse(&entry.addr).ok_or_else(|| {
                format!(
                    "{}[{}]: Invalid SNES address: {}",
                    file_path.display(),
                    i,
                    entry.addr
                )
            })?;

            if !seen_addrs.insert((addr.bank, addr.addr)) {
                return Err(format!(
                    "{}[{}]: Duplicate address {}",
                    file_path.display(),
                    i,
                    entry.addr
                ));
            }

            let encoded = ko::encode_ko_string_ff(&entry.ko, ko_table).map_err(|e| {
                format!(
                    "{}[{}]: Encoding error at {}: {}",
                    file_path.display(),
                    i,
                    entry.addr,
                    e
                )
            })?;

            entries.push(TranslationEntry { addr, encoded });
        }
    }

    // Sort by address for deterministic output (matching TSV order)
    entries.sort_by_key(|e| (e.addr.bank, e.addr.addr));

    Ok(entries)
}

// ── Encyclopedia JSON ────────────────────────────────────────────

#[derive(Deserialize)]
pub struct EncyclopediaJsonFile {
    pub entries: Vec<EncJsonEntry>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct EncJsonEntry {
    pub id: usize,
    #[serde(default)]
    pub addr: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    pub loc_idx: u8,
    pub ko: String,
    #[serde(default)]
    pub jp: String,
    #[serde(default)]
    pub notes: String,
}

const MONSTER_COUNT: usize = 36;

/// Load encyclopedia.json and encode names/descriptions.
pub fn load_encyclopedia_json(
    path: &Path,
    ko_table: &HashMap<char, Vec<u8>>,
) -> Result<EncyclopediaData, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read '{}': {}", path.display(), e))?;
    let file: EncyclopediaJsonFile = serde_json::from_str(&content)
        .map_err(|e| format!("JSON parse error in '{}': {}", path.display(), e))?;

    let mut names: Vec<Option<Vec<u8>>> = vec![None; MONSTER_COUNT];
    let mut descs: Vec<Option<Vec<u8>>> = vec![None; MONSTER_COUNT];

    for entry in &file.entries {
        if entry.id >= MONSTER_COUNT {
            return Err(format!(
                "Monster ID {} out of range (max {})",
                entry.id,
                MONSTER_COUNT - 1
            ));
        }

        match entry.entry_type.as_str() {
            "name" => {
                let mut encoded = ko::encode_simple(&entry.ko, ko_table)
                    .map_err(|e| format!("Monster #{} name encoding error: {}", entry.id, e))?;
                encoded.push(0xFF);
                names[entry.id] = Some(encoded);
            }
            "desc" => {
                // JSON preserves actual newlines — no \\n replacement needed
                let text_encoded = ko::encode_simple(&entry.ko, ko_table)
                    .map_err(|e| format!("Monster #{} desc encoding error: {}", entry.id, e))?;
                let mut encoded = vec![entry.loc_idx];
                encoded.extend_from_slice(&text_encoded);
                encoded.push(0xFF);
                descs[entry.id] = Some(encoded);
            }
            _ => {
                return Err(format!(
                    "Unknown entry type '{}' for monster #{}",
                    entry.entry_type, entry.id
                ))
            }
        }
    }

    // Verify all entries present
    let mut missing = Vec::new();
    for i in 0..MONSTER_COUNT {
        if names[i].is_none() {
            missing.push(format!("name #{}", i));
        }
        if descs[i].is_none() {
            missing.push(format!("desc #{}", i));
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "Missing encyclopedia entries: {}",
            missing.join(", ")
        ));
    }

    let final_names: Vec<Vec<u8>> = names.into_iter().map(|n| n.unwrap()).collect();

    // Compute battle names (same logic as TSV loader in encyclopedia.rs)
    let battle_names = crate::patch::encyclopedia::compute_battle_names(&final_names);

    Ok(EncyclopediaData {
        names: final_names,
        battle_names,
        descs: descs.into_iter().map(|d| d.unwrap()).collect(),
    })
}

// ── Code patches JSON ────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CodePatchesJsonFile {
    pub entries: Vec<CodePatchJsonEntry>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct CodePatchJsonEntry {
    pub id: String,
    pub pc_addr: String,
    pub slot_size: usize,
    #[serde(default)]
    pub prefix_bytes: String,
    pub ko: String,
    #[serde(default)]
    pub notes: String,
}

/// A parsed code patch entry (shared between TSV and JSON loaders).
pub struct CodePatchEntry {
    pub id: String,
    pub pc_addr: usize,
    pub slot_size: usize,
    pub prefix: Vec<u8>,
    pub ko_text: String,
}

/// Load code_patches.json into CodePatchEntry list.
pub fn load_code_patches_json(path: &Path) -> Result<Vec<CodePatchEntry>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read '{}': {}", path.display(), e))?;
    let file: CodePatchesJsonFile = serde_json::from_str(&content)
        .map_err(|e| format!("JSON parse error in '{}': {}", path.display(), e))?;

    let mut entries = Vec::new();
    for entry in &file.entries {
        let pc_addr = usize::from_str_radix(entry.pc_addr.trim_start_matches("0x"), 16)
            .map_err(|_| format!("Invalid PC_ADDR '{}' for '{}'", entry.pc_addr, entry.id))?;

        let prefix: Vec<u8> = if entry.prefix_bytes.trim().is_empty() {
            Vec::new()
        } else {
            entry
                .prefix_bytes
                .split_whitespace()
                .map(|h| {
                    u8::from_str_radix(h, 16)
                        .map_err(|_| format!("Invalid prefix hex '{}' for '{}'", h, entry.id))
                })
                .collect::<Result<Vec<u8>, String>>()?
        };

        entries.push(CodePatchEntry {
            id: entry.id.clone(),
            pc_addr,
            slot_size: entry.slot_size,
            prefix,
            ko_text: entry.ko.clone(),
        });
    }

    Ok(entries)
}

// ── Charset collection ──────────────────────────────────────────

/// Characters already in FIXED_ENCODE ($00-$1F) — excluded from charset.
/// Source of truth: `font_gen::FIXED_CHARS`.
pub fn is_fixed_encode_char(ch: char) -> bool {
    matches!(
        ch,
        '0'..='9'
            | '!'
            | '~'
            | '.'
            | '?'
            | '"'
            | '\u{2192}'
            | '\u{2191}'
            | '\u{2190}'
            | '\u{2193}'
            | '-'
            | ','
            | '['
            | ']'
            | '\u{300C}'
            | '\u{300D}'
    )
}

/// Check if a character should be included in the charset.
/// Only Hangul syllables and ASCII printable (non-FIXED_ENCODE) pass.
/// Japanese hiragana/katakana, CJK ideographs, fullwidth chars, and
/// other non-renderable characters are excluded — they use the game's
/// built-in JP encoding table as control identifiers.
fn is_charset_char(ch: char) -> bool {
    if ch.is_control() || ch == ' ' {
        return false;
    }
    if is_fixed_encode_char(ch) {
        return false;
    }
    // Hangul syllables (가-힣)
    if ('\u{AC00}'..='\u{D7AF}').contains(&ch) {
        return true;
    }
    // ASCII printable (letters, etc. — digits/punctuation already excluded above)
    if ch.is_ascii_graphic() {
        return true;
    }
    // Middle dot (·) — used as separator in Korean text
    if ch == '\u{00B7}' {
        return true;
    }
    false
}

/// Strip `{TAG}` control tags from text.
/// Tags like `{SEP}`, `{NL}`, `{BOX:アルル}`, `{PAGE}` are removed
/// so their internal characters aren't collected into the charset.
fn strip_control_tags(text: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '{' => in_tag = true,
            '}' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}

/// Scan all translation JSON files in `dir` and collect unique characters
/// with frequency counts. Control tags are stripped, and only Hangul
/// syllables + ASCII graphic characters are included.
pub fn collect_charset_from_translations(dir: &Path) -> Result<HashMap<char, usize>, String> {
    let mut freq: HashMap<char, usize> = HashMap::new();

    let mut count_text = |text: &str| {
        let stripped = strip_control_tags(text);
        for ch in stripped.chars() {
            if is_charset_char(ch) {
                *freq.entry(ch).or_insert(0) += 1;
            }
        }
    };

    // 1. bank_*_*.json files
    let bank_files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("Failed to read dir '{}': {}", dir.display(), e))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension().map(|e| e == "json").unwrap_or(false)
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("bank_"))
                    .unwrap_or(false)
        })
        .collect();

    for file_path in &bank_files {
        let content = std::fs::read_to_string(file_path)
            .map_err(|e| format!("Failed to read '{}': {}", file_path.display(), e))?;
        let file: BankTranslationFile = serde_json::from_str(&content)
            .map_err(|e| format!("JSON parse error in '{}': {}", file_path.display(), e))?;
        for entry in &file.entries {
            if !entry.ko.is_empty() {
                count_text(&entry.ko);
            }
        }
    }

    // 2. encyclopedia.json
    let enc_path = dir.join("encyclopedia.json");
    if enc_path.exists() {
        let content = std::fs::read_to_string(&enc_path)
            .map_err(|e| format!("Failed to read '{}': {}", enc_path.display(), e))?;
        let file: EncyclopediaJsonFile = serde_json::from_str(&content)
            .map_err(|e| format!("JSON parse error in '{}': {}", enc_path.display(), e))?;
        for entry in &file.entries {
            if !entry.ko.is_empty() {
                count_text(&entry.ko);
            }
        }
    }

    // 3. code_patches.json
    let cp_path = dir.join("code_patches.json");
    if cp_path.exists() {
        let content = std::fs::read_to_string(&cp_path)
            .map_err(|e| format!("Failed to read '{}': {}", cp_path.display(), e))?;
        let file: CodePatchesJsonFile = serde_json::from_str(&content)
            .map_err(|e| format!("JSON parse error in '{}': {}", cp_path.display(), e))?;
        for entry in &file.entries {
            if !entry.ko.is_empty() {
                count_text(&entry.ko);
            }
        }
    }

    Ok(freq)
}

#[cfg(test)]
#[path = "translation_json_tests.rs"]
mod tests;
