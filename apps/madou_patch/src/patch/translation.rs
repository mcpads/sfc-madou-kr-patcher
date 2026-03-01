//! Shared TSV loading and encoding for translation banks.
//!
//! Loads `translations/bank_XX.tsv` files and encodes Korean text
//! using the KO encoding engine, replacing the old Python pipeline
//! that produced pre-encoded .bin + .index files.

use crate::encoding::ko;
use crate::patch::translation_json;
use crate::rom::SnesAddr;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// A single translated and encoded text entry.
#[derive(Debug)]
pub struct TranslationEntry {
    pub addr: SnesAddr,
    pub encoded: Vec<u8>,
}

/// Load and encode a bank's translations (JSON-first, TSV fallback).
///
/// If `bank_{id}_*.json` files exist in the directory, loads from JSON.
/// Otherwise falls back to the legacy TSV format.
pub fn load_and_encode_bank(
    translations_dir: &Path,
    bank_id: &str,
    ko_table: &HashMap<char, Vec<u8>>,
    fc_split: bool,
) -> Result<Vec<TranslationEntry>, String> {
    let json_files = translation_json::glob_bank_json(translations_dir, bank_id);
    if !json_files.is_empty() {
        return translation_json::load_bank_json_chunks(
            translations_dir,
            bank_id,
            ko_table,
            fc_split,
        );
    }
    load_and_encode_bank_tsv(translations_dir, bank_id, ko_table, fc_split)
}

/// Load a bank translation TSV and encode all entries (legacy format).
///
/// TSV format: ADDR\tCATEGORY\tJP\tKO\tNOTES
/// - ADDR: SNES address like `$01:B400`
/// - KO: Korean text with control tags ({NL}, {PAGE}, {BOX:name}, etc.)
///
/// For FF-terminated banks: encoded bytes include FF terminator.
/// For FC-split banks: no FF terminator added.
fn load_and_encode_bank_tsv(
    translations_dir: &Path,
    bank_id: &str,
    ko_table: &HashMap<char, Vec<u8>>,
    _fc_split: bool,
) -> Result<Vec<TranslationEntry>, String> {
    let tsv_path = translations_dir.join(format!("bank_{}.tsv", bank_id));
    let content = fs::read_to_string(&tsv_path)
        .map_err(|e| format!("Failed to read '{}': {}", tsv_path.display(), e))?;

    let mut entries = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        // Skip comment lines and header
        if line.starts_with('#') || line.starts_with("ADDR") || line.trim().is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 4 {
            continue;
        }

        let addr = SnesAddr::parse(parts[0]).ok_or_else(|| {
            format!(
                "{}:{}: Invalid SNES address: {}",
                tsv_path.display(),
                line_num + 1,
                parts[0]
            )
        })?;

        let ko_text = parts[3];
        if ko_text.is_empty() {
            continue; // skip untranslated entries
        }

        // Always include FF terminator — original JP ROM uses FF even in
        // FC-split banks: [FC][speaker][text...][FF] [FC][speaker]...
        let encoded = ko::encode_ko_string_ff(ko_text, ko_table).map_err(|e| {
            format!(
                "{}:{}: Encoding error at {}: {}",
                tsv_path.display(),
                line_num + 1,
                parts[0],
                e
            )
        })?;

        entries.push(TranslationEntry { addr, encoded });
    }

    Ok(entries)
}

#[cfg(test)]
#[path = "translation_tests.rs"]
mod tests;
