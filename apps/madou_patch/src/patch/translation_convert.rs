//! TSV → JSON translation file converter.
//!
//! Reads existing TSV files and writes chunked JSON files
//! in the same directory.

use serde::Serialize;
use std::fs;
use std::path::Path;

// ── Bank JSON output structs ─────────────────────────────────────

#[derive(Serialize)]
struct BankJsonFile {
    bank: String,
    entries: Vec<BankJsonEntry>,
}

#[derive(Serialize)]
struct BankJsonEntry {
    addr: String,
    jp: String,
    ko: String,
    category: String,
    notes: String,
}

// ── Encyclopedia JSON output ─────────────────────────────────────

#[derive(Serialize)]
struct EncyclopediaJsonFile {
    entries: Vec<EncJsonEntry>,
}

#[derive(Serialize)]
struct EncJsonEntry {
    id: usize,
    #[serde(rename = "type")]
    entry_type: String,
    loc_idx: u8,
    ko: String,
}

// ── Code patches JSON output ─────────────────────────────────────

#[derive(Serialize)]
struct CodePatchesJsonFile {
    entries: Vec<CodePatchJsonEntry>,
}

#[derive(Serialize)]
struct CodePatchJsonEntry {
    id: String,
    pc_addr: String,
    slot_size: usize,
    prefix_bytes: String,
    ko: String,
    notes: String,
}

// ── Conversion logic ─────────────────────────────────────────────

const BANK_IDS: &[&str] = &["01", "03", "1D", "2A", "2B", "2D"];

/// Convert all TSV translation files to JSON format.
pub fn convert_all(translations_dir: &Path, chunk_size: usize) -> Result<(), String> {
    // Bank TSVs
    for bank_id in BANK_IDS {
        convert_bank_tsv(translations_dir, bank_id, chunk_size)?;
    }

    // Encyclopedia
    let enc_path = translations_dir.join("encyclopedia.tsv");
    if enc_path.exists() {
        convert_encyclopedia_tsv(translations_dir)?;
    }

    // Code patches
    let code_path = translations_dir.join("code_patches.tsv");
    if code_path.exists() {
        convert_code_patches_tsv(translations_dir)?;
    }

    Ok(())
}

fn convert_bank_tsv(dir: &Path, bank_id: &str, chunk_size: usize) -> Result<(), String> {
    let tsv_path = dir.join(format!("bank_{}.tsv", bank_id));
    if !tsv_path.exists() {
        println!("  SKIP: {} (not found)", tsv_path.display());
        return Ok(());
    }

    let content = fs::read_to_string(&tsv_path)
        .map_err(|e| format!("Failed to read '{}': {}", tsv_path.display(), e))?;

    // Parse TSV into tuples
    let mut all_entries: Vec<(String, String, String, String, String)> = Vec::new();
    for line in content.lines() {
        if line.starts_with('#') || line.starts_with("ADDR") || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 4 {
            continue;
        }
        all_entries.push((
            parts[0].to_string(),
            if parts.len() > 1 {
                parts[1].to_string()
            } else {
                String::new()
            },
            if parts.len() > 2 {
                parts[2].to_string()
            } else {
                String::new()
            },
            parts[3].to_string(),
            if parts.len() > 4 {
                parts[4].to_string()
            } else {
                String::new()
            },
        ));
    }

    let total = all_entries.len();
    let chunk_count = total.div_ceil(chunk_size);
    for (chunk_idx, chunk) in all_entries.chunks(chunk_size).enumerate() {
        let file = BankJsonFile {
            bank: bank_id.to_string(),
            entries: chunk
                .iter()
                .map(|(addr, cat, jp, ko, notes)| BankJsonEntry {
                    addr: addr.clone(),
                    jp: jp.clone(),
                    ko: ko.clone(),
                    category: cat.clone(),
                    notes: notes.clone(),
                })
                .collect(),
        };

        let out_path = dir.join(format!("bank_{}_{:02}.json", bank_id, chunk_idx + 1));
        let json = serde_json::to_string_pretty(&file)
            .map_err(|e| format!("JSON serialize error: {}", e))?;
        fs::write(&out_path, &json)
            .map_err(|e| format!("Failed to write '{}': {}", out_path.display(), e))?;
    }

    println!(
        "  bank_{}.tsv → {} entries → {} chunk(s)",
        bank_id, total, chunk_count
    );

    Ok(())
}

fn convert_encyclopedia_tsv(dir: &Path) -> Result<(), String> {
    let tsv_path = dir.join("encyclopedia.tsv");
    let content = fs::read_to_string(&tsv_path)
        .map_err(|e| format!("Failed to read '{}': {}", tsv_path.display(), e))?;

    let mut entries = Vec::new();
    for line in content.lines() {
        if line.starts_with('#') || line.starts_with("ID") || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 4 {
            continue;
        }

        let id: usize = parts[0]
            .parse()
            .map_err(|_| format!("Invalid ID: {}", parts[0]))?;
        let entry_type = parts[1].to_string();
        let loc_idx: u8 = parts[2]
            .parse()
            .map_err(|_| format!("Invalid LOC_IDX: {}", parts[2]))?;
        // Convert literal \\n (two chars) to actual newline for JSON
        let ko = parts[3].replace("\\n", "\n");

        entries.push(EncJsonEntry {
            id,
            entry_type,
            loc_idx,
            ko,
        });
    }

    let out_path = dir.join("encyclopedia.json");
    let file = EncyclopediaJsonFile { entries };
    let json =
        serde_json::to_string_pretty(&file).map_err(|e| format!("JSON serialize error: {}", e))?;
    fs::write(&out_path, &json)
        .map_err(|e| format!("Failed to write '{}': {}", out_path.display(), e))?;

    println!(
        "  encyclopedia.tsv → {} entries → encyclopedia.json",
        file.entries.len()
    );

    Ok(())
}

fn convert_code_patches_tsv(dir: &Path) -> Result<(), String> {
    let tsv_path = dir.join("code_patches.tsv");
    let content = fs::read_to_string(&tsv_path)
        .map_err(|e| format!("Failed to read '{}': {}", tsv_path.display(), e))?;

    let mut entries = Vec::new();
    for line in content.lines() {
        if line.starts_with('#') || line.starts_with("ID") || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 5 {
            continue;
        }

        entries.push(CodePatchJsonEntry {
            id: parts[0].to_string(),
            pc_addr: parts[1].to_string(),
            slot_size: parts[2]
                .parse()
                .map_err(|_| format!("Invalid SLOT_SIZE: {}", parts[2]))?,
            prefix_bytes: parts[3].to_string(),
            ko: parts[4].to_string(),
            notes: if parts.len() > 5 {
                parts[5].to_string()
            } else {
                String::new()
            },
        });
    }

    let out_path = dir.join("code_patches.json");
    let file = CodePatchesJsonFile { entries };
    let json =
        serde_json::to_string_pretty(&file).map_err(|e| format!("JSON serialize error: {}", e))?;
    fs::write(&out_path, &json)
        .map_err(|e| format!("Failed to write '{}': {}", out_path.display(), e))?;

    println!(
        "  code_patches.tsv → {} entries → code_patches.json",
        file.entries.len()
    );

    Ok(())
}
