//! Bank-level text extraction: extract all strings from a bank and decode them.

use crate::encoding::codec;
use crate::text::control::BankConfig;
use crate::text::stream;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

/// A decoded string from a bank.
#[derive(Debug)]
pub struct DecodedString {
    pub bank: u8,
    pub snes_addr: u16,
    pub raw: Vec<u8>,
    pub text: String,
    pub unknowns: usize,
    #[allow(dead_code)]
    pub char_count: usize,
}

/// Extract and decode all strings from a bank.
pub fn extract_bank(rom: &[u8], config: &BankConfig) -> Vec<DecodedString> {
    let raw_strings = stream::extract_strings(
        rom,
        config.bank,
        config.start_addr,
        config.end_addr,
        config.fc_split,
        config.filter_noise,
    );

    raw_strings
        .into_iter()
        .map(|rs| {
            let tokens = codec::decode_jp(&rs.data);
            let text = codec::tokens_to_string(&tokens);
            let unknowns = codec::count_unknowns(&tokens);
            let char_count = codec::count_chars(&tokens);
            DecodedString {
                bank: config.bank,
                snes_addr: rs.snes_addr,
                raw: rs.data,
                text,
                unknowns,
                char_count,
            }
        })
        .collect()
}

/// Print bank extraction results.
pub fn print_bank(strings: &[DecodedString], config: &BankConfig, show_all: bool) {
    let total = strings.len();
    let fully_decoded = strings.iter().filter(|s| s.unknowns == 0).count();

    for s in strings {
        if show_all || s.unknowns <= 3 {
            let display = s.text.replace('\n', "\\n");
            let tag = if s.unknowns == 0 {
                "OK".to_string()
            } else {
                format!("{}?", s.unknowns)
            };
            println!(
                "  ${:02X}:{:04X} [{:>3}]: {}",
                s.bank, s.snes_addr, tag, display
            );
        }
    }

    println!();
    println!(
        "  --- Bank ${:02X} [{}] Statistics ---",
        config.bank, config.label
    );
    println!("  Total text strings: {}", total);
    if total > 0 {
        println!(
            "  Fully decoded: {} ({:.1}%)",
            fully_decoded,
            100.0 * fully_decoded as f64 / total as f64
        );
    }
}

/// Print bank extraction results in TSV format.
///
/// Format: `ADDR\tCATEGORY\tJP\tKO\tNOTES`
pub fn print_tsv(strings: &[DecodedString], config: &BankConfig) {
    let category = label_to_category(config.label);
    println!("ADDR\tCATEGORY\tJP\tKO\tNOTES");
    for s in strings {
        if s.unknowns > 3 {
            continue;
        }
        let display = s.text.replace('\n', "\\n");
        println!(
            "${:02X}:{:04X}\t{}\t{}\t\t",
            s.bank, s.snes_addr, category, display
        );
    }
}

/// Convert a bank label to a category name.
pub fn label_to_category(label: &str) -> &'static str {
    match label {
        "01" => "MENU",
        "01_monster" => "MONSTER_LABEL",
        "01_save" => "SAVE_LABEL",
        "01_hp" => "HP_STATUS",
        "03" => "DIARY",
        "08" => "OPENING_EVENT",
        "09" => "ORB_MOMOMO",
        "0A" => "DRAGON_GATE",
        "1D" => "BATTLE",
        "2A" => "WORLD_MAP",
        "2B" => "STORY",
        "2D" => "TUTORIAL",
        _ => "UNKNOWN",
    }
}

// ── JSON dump ───────────────────────────────────────────────────

/// Convert tokens_to_string output to JSON-convention control codes.
///
/// tokens_to_string uses: `\n`, `|`, `▽`, `<CHOICE>`, `\n[BOX:X] name`
/// JSON convention uses: `{NL}`, `{SEP}`, `{PAGE}`, `{CHOICE}`, `{BOX:name}`
fn text_to_json_convention(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\n' => {
                // Check for [BOX:...] pattern after newline
                if chars.peek() == Some(&'[') {
                    // Peek ahead to check for BOX pattern
                    let rest: String = chars.clone().collect();
                    if rest.starts_with("[BOX:") {
                        if let Some(end) = rest.find(']') {
                            // Extract the speaker ID digit
                            let box_content = &rest[5..end]; // "0", "1", "2", "3"
                            let speaker = match box_content {
                                "0" => "アルル",
                                "1" => "話者1",
                                "2" => "話者2",
                                "3" => "NPC",
                                _ => box_content,
                            };
                            result.push_str(&format!("{{BOX:{}}}", speaker));
                            // Skip past "[BOX:X] " in the iterator
                            for _ in 0..end + 1 {
                                chars.next();
                            }
                            // Skip trailing space after ]
                            if chars.peek() == Some(&' ') {
                                chars.next();
                            }
                            // Skip the speaker name text that follows
                            // tokens_to_string format: "\n[BOX:X] speaker_name"
                            // The speaker name is already consumed by the chars
                            // Actually, we need to skip "speaker_name" too
                            // Let me re-check: format is "\n[BOX:{id}] {speaker_name}"
                            // After "]" we skipped space, now skip speaker name
                            let name_to_skip = match box_content {
                                "0" => "アルル",
                                "1" => "話者1",
                                "2" => "話者2",
                                "3" => "NPC",
                                _ => "",
                            };
                            let remaining: String = chars.clone().collect();
                            if remaining.starts_with(name_to_skip) {
                                for _ in 0..name_to_skip.chars().count() {
                                    chars.next();
                                }
                            }
                            continue;
                        }
                    }
                }
                result.push_str("{NL}");
            }
            '|' => result.push_str("{SEP}"),
            '▽' => result.push_str("{PAGE}"),
            _ => result.push(ch),
        }
    }
    // Handle <CHOICE> tags
    result = result.replace("<CHOICE>", "{CHOICE}");
    result
}

/// A decoded string tagged with its category, for JSON dump.
pub struct CategorizedString {
    pub bank: u8,
    pub snes_addr: u16,
    pub text: String,
    pub category: String,
    pub unknowns: usize,
}

#[derive(Serialize)]
struct DumpJsonFile {
    bank: String,
    entries: Vec<DumpJsonEntry>,
}

#[derive(Serialize, Clone)]
struct DumpJsonEntry {
    addr: String,
    jp: String,
    ko: String,
    category: String,
    notes: String,
}

/// Dump decoded strings as chunked JSON files, grouped by bank.
///
/// Files are written as `bank_{XX}_{NN}.json` in `output_dir`.
pub fn dump_json_chunks(
    entries: Vec<CategorizedString>,
    output_dir: &Path,
    chunk_size: usize,
) -> Result<(), String> {
    // Group by bank ID string
    let mut bank_groups: BTreeMap<String, Vec<CategorizedString>> = BTreeMap::new();
    for entry in entries {
        let bank_id = format!("{:02X}", entry.bank);
        bank_groups.entry(bank_id).or_default().push(entry);
    }

    std::fs::create_dir_all(output_dir)
        .map_err(|e| format!("Failed to create dir '{}': {}", output_dir.display(), e))?;

    for (bank_id, mut group) in bank_groups {
        // Sort by address
        group.sort_by_key(|e| (e.bank, e.snes_addr));

        // Build JSON entries (filter out high-unknown entries)
        let json_entries: Vec<DumpJsonEntry> = group
            .iter()
            .filter(|e| e.unknowns <= 3)
            .map(|e| DumpJsonEntry {
                addr: format!("${:02X}:{:04X}", e.bank, e.snes_addr),
                jp: text_to_json_convention(&e.text),
                ko: String::new(),
                category: e.category.clone(),
                notes: String::new(),
            })
            .collect();

        let total = json_entries.len();

        for (chunk_idx, chunk) in json_entries.chunks(chunk_size).enumerate() {
            let file = DumpJsonFile {
                bank: bank_id.clone(),
                entries: chunk.to_vec(),
            };
            let out_path = output_dir.join(format!("bank_{}_{:02}.json", bank_id, chunk_idx + 1));
            let json = serde_json::to_string_pretty(&file)
                .map_err(|e| format!("JSON serialize: {}", e))?;
            std::fs::write(&out_path, format!("{}\n", json))
                .map_err(|e| format!("write '{}': {}", out_path.display(), e))?;
        }

        let chunks = if total > 0 {
            total.div_ceil(chunk_size)
        } else {
            0
        };
        eprintln!(
            "  bank_{} → {} entries → {} chunk(s)",
            bank_id, total, chunks
        );
    }

    Ok(())
}
