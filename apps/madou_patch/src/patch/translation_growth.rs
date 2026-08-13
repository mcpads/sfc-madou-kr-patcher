//! Compare translation sources and report entries whose rendered surface grew.
//!
//! This audit deliberately separates objective source growth from display
//! overflow. An entry is only called an overflow when its concrete consumer
//! profile is known; all other growth remains a review candidate.

use crate::encoding::ko;
use crate::rom::SnesAddr;
use crate::text::control;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const CELL_WIDTH_TILES: usize = 2;
const LINE_WIDTH_TILES: usize = 20;

#[derive(Debug, Serialize)]
pub struct GrowthAudit {
    pub schema_version: u8,
    pub baseline_dir: String,
    pub current_dir: String,
    pub summary: GrowthSummary,
    pub candidates: Vec<GrowthCandidate>,
}

#[derive(Debug, Serialize)]
pub struct GrowthSummary {
    pub baseline_entries: usize,
    pub current_entries: usize,
    pub changed_entries: usize,
    pub new_entries: usize,
    pub removed_entries: usize,
    pub growth_candidates: usize,
    pub crossed_common_10_cell_line: usize,
    pub confirmed_fits: usize,
    pub confirmed_new_overflows: usize,
    pub confirmed_existing_overflows: usize,
}

#[derive(Debug, Serialize)]
pub struct GrowthCandidate {
    pub entry_id: String,
    pub source: String,
    pub address: String,
    pub category: String,
    pub previous_ko: String,
    pub current_ko: String,
    pub previous: SurfaceMetrics,
    pub current: SurfaceMetrics,
    pub visible_cell_delta: isize,
    pub max_explicit_line_delta: isize,
    /// A prioritization signal only. It is not an overflow verdict unless the
    /// concrete consumer profile is also known.
    pub crossed_common_10_cell_line: bool,
    pub impact: GrowthImpact,
    pub ambiguity: GrowthAmbiguity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmed_consumer_profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GrowthImpact {
    Candidate,
    ConfirmedFits,
    ConfirmedNewOverflow,
    ConfirmedExistingOverflow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GrowthAmbiguity {
    ConsumerNotMapped,
    Low,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SurfaceMetrics {
    pub visible_cells: usize,
    pub max_explicit_line_cells: usize,
    pub segments: Vec<Vec<usize>>,
}

#[derive(Debug)]
struct TranslationSurface {
    source: String,
    address: String,
    category: String,
    ko: String,
}

pub fn build_growth_audit(baseline_dir: &Path, current_dir: &Path) -> Result<GrowthAudit, String> {
    let baseline = load_translation_surfaces(baseline_dir)?;
    let current = load_translation_surfaces(current_dir)?;
    let jp_table = ko::build_jp_encode_table();

    let new_entries = current
        .keys()
        .filter(|key| !baseline.contains_key(*key))
        .count();
    let removed_entries = baseline
        .keys()
        .filter(|key| !current.contains_key(*key))
        .count();
    let changed_entries = current
        .iter()
        .filter(|(key, entry)| baseline.get(*key).is_none_or(|old| old.ko != entry.ko))
        .count()
        + removed_entries;

    let mut candidates = Vec::new();
    for (entry_id, current_entry) in &current {
        let previous_ko = baseline
            .get(entry_id)
            .map(|entry| entry.ko.as_str())
            .unwrap_or("");
        if previous_ko == current_entry.ko {
            continue;
        }

        let previous = surface_metrics(previous_ko, &jp_table)?;
        let current_metrics = surface_metrics(&current_entry.ko, &jp_table)?;
        if !surface_grew(&previous, &current_metrics) {
            continue;
        }

        let profile = parse_snes_address(&current_entry.address)
            .and_then(|addr| control::fixed_text_box_profile(addr.bank, addr.addr));
        let previous_overflow =
            profile.is_some_and(|profile| fixed_profile_overflow(&previous, profile));
        let current_overflow =
            profile.is_some_and(|profile| fixed_profile_overflow(&current_metrics, profile));

        let (impact, ambiguity, confirmed_consumer_profile) = if current_overflow {
            let impact = if previous_overflow {
                GrowthImpact::ConfirmedExistingOverflow
            } else {
                GrowthImpact::ConfirmedNewOverflow
            };
            (
                impact,
                GrowthAmbiguity::Low,
                profile.map(|profile| {
                    format!(
                        "fixed_rows_{}x{}",
                        profile.max_width_tiles / CELL_WIDTH_TILES,
                        profile.max_lines
                    )
                }),
            )
        } else if let Some(profile) = profile {
            (
                GrowthImpact::ConfirmedFits,
                GrowthAmbiguity::Low,
                Some(format!(
                    "fixed_rows_{}x{}",
                    profile.max_width_tiles / CELL_WIDTH_TILES,
                    profile.max_lines
                )),
            )
        } else {
            (
                GrowthImpact::Candidate,
                GrowthAmbiguity::ConsumerNotMapped,
                None,
            )
        };

        candidates.push(GrowthCandidate {
            entry_id: entry_id.clone(),
            source: current_entry.source.clone(),
            address: current_entry.address.clone(),
            category: current_entry.category.clone(),
            previous_ko: previous_ko.to_owned(),
            current_ko: current_entry.ko.clone(),
            visible_cell_delta: current_metrics.visible_cells as isize
                - previous.visible_cells as isize,
            max_explicit_line_delta: current_metrics.max_explicit_line_cells as isize
                - previous.max_explicit_line_cells as isize,
            crossed_common_10_cell_line: previous.max_explicit_line_cells
                <= LINE_WIDTH_TILES / CELL_WIDTH_TILES
                && current_metrics.max_explicit_line_cells > LINE_WIDTH_TILES / CELL_WIDTH_TILES,
            previous,
            current: current_metrics,
            impact,
            ambiguity,
            confirmed_consumer_profile,
        });
    }

    candidates.sort_by(|left, right| left.entry_id.cmp(&right.entry_id));
    let confirmed_new_overflows = candidates
        .iter()
        .filter(|candidate| candidate.impact == GrowthImpact::ConfirmedNewOverflow)
        .count();
    let confirmed_existing_overflows = candidates
        .iter()
        .filter(|candidate| candidate.impact == GrowthImpact::ConfirmedExistingOverflow)
        .count();
    let confirmed_fits = candidates
        .iter()
        .filter(|candidate| candidate.impact == GrowthImpact::ConfirmedFits)
        .count();
    let crossed_common_10_cell_line = candidates
        .iter()
        .filter(|candidate| candidate.crossed_common_10_cell_line)
        .count();

    Ok(GrowthAudit {
        schema_version: 1,
        baseline_dir: baseline_dir.display().to_string(),
        current_dir: current_dir.display().to_string(),
        summary: GrowthSummary {
            baseline_entries: baseline.len(),
            current_entries: current.len(),
            changed_entries,
            new_entries,
            removed_entries,
            growth_candidates: candidates.len(),
            crossed_common_10_cell_line,
            confirmed_fits,
            confirmed_new_overflows,
            confirmed_existing_overflows,
        },
        candidates,
    })
}

pub fn write_growth_audit(
    baseline_dir: &Path,
    current_dir: &Path,
    output_path: &Path,
) -> Result<GrowthSummary, String> {
    let audit = build_growth_audit(baseline_dir, current_dir)?;
    if let Some(parent) = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create '{}': {}", parent.display(), e))?;
    }
    let json = serde_json::to_string_pretty(&audit)
        .map_err(|e| format!("Failed to serialize translation growth audit: {}", e))?;
    std::fs::write(output_path, format!("{}\n", json))
        .map_err(|e| format!("Failed to write '{}': {}", output_path.display(), e))?;
    Ok(audit.summary)
}

fn load_translation_surfaces(dir: &Path) -> Result<BTreeMap<String, TranslationSurface>, String> {
    let mut files = sorted_json_files(dir)?;
    files.retain(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.starts_with("bank_")
                    || name == "encyclopedia.json"
                    || name == "code_patches.json"
            })
    });

    let mut result = BTreeMap::new();
    for path in files {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read '{}': {}", path.display(), e))?;
        let value: Value = serde_json::from_str(&content)
            .map_err(|e| format!("JSON parse error in '{}': {}", path.display(), e))?;
        let entries = value
            .get("entries")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("Missing entries array in '{}'", path.display()))?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();

        for (index, entry) in entries.iter().enumerate() {
            let ko = entry
                .get("ko")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{}[{}]: Missing ko string", path.display(), index))?;
            let (entry_id, address, category) = surface_identity(file_name, entry, index)?;
            let surface = TranslationSurface {
                source: file_name.to_owned(),
                address,
                category,
                ko: ko.to_owned(),
            };
            if result.insert(entry_id.clone(), surface).is_some() {
                return Err(format!(
                    "Duplicate translation entry identity: {}",
                    entry_id
                ));
            }
        }
    }

    Ok(result)
}

fn sorted_json_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = std::fs::read_dir(dir)
        .map_err(|e| format!("Failed to read '{}': {}", dir.display(), e))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

fn surface_identity(
    file_name: &str,
    entry: &Value,
    index: usize,
) -> Result<(String, String, String), String> {
    if file_name.starts_with("bank_") {
        let address = required_string(entry, "addr", file_name, index)?;
        let category = optional_string(entry, "category");
        return Ok((format!("bank:{}", address), address, category));
    }

    if file_name == "encyclopedia.json" {
        let id = entry
            .get("id")
            .map(value_identity)
            .ok_or_else(|| format!("{}[{}]: Missing id", file_name, index))?;
        let entry_type = required_string(entry, "type", file_name, index)?;
        let address = optional_string(entry, "addr");
        return Ok((
            format!("encyclopedia:{}:{}", id, entry_type),
            address,
            entry_type,
        ));
    }

    if file_name == "code_patches.json" {
        let id = required_string(entry, "id", file_name, index)?;
        let address = optional_string(entry, "pc_addr");
        return Ok((
            format!("code_patch:{}", id),
            address,
            "code_patch".to_owned(),
        ));
    }

    Err(format!("Unsupported translation source: {}", file_name))
}

fn required_string(
    entry: &Value,
    field: &str,
    file_name: &str,
    index: usize,
) -> Result<String, String> {
    entry
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("{}[{}]: Missing {} string", file_name, index, field))
}

fn optional_string(entry: &Value, field: &str) -> String {
    entry
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn value_identity(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}

fn surface_metrics(
    text: &str,
    jp_table: &std::collections::HashMap<char, Vec<u8>>,
) -> Result<SurfaceMetrics, String> {
    let chars = text.chars().collect::<Vec<_>>();
    let mut segments = vec![vec![0usize]];
    let mut after_page = false;
    let mut index = 0;

    while index < chars.len() {
        if chars[index] == '{' {
            let close = chars[index..]
                .iter()
                .position(|ch| *ch == '}')
                .ok_or_else(|| format!("Unclosed control tag in translation: {}", text))?;
            let tag = chars[index + 1..index + close].iter().collect::<String>();
            match tag.as_str() {
                "NL" => segments
                    .last_mut()
                    .ok_or_else(|| "Translation surface has no segment".to_owned())?
                    .push(0),
                "PAGE" => {
                    segments.push(vec![0]);
                    after_page = true;
                }
                "SEP" => {
                    segments.push(vec![0]);
                    after_page = false;
                }
                "CHOICE" => after_page = false,
                _ if tag.starts_with("BOX:") => {
                    if !is_empty_initial_segment(&segments) {
                        segments.push(vec![0]);
                    }
                    after_page = false;
                }
                _ if tag.starts_with("RAW:") => {}
                "F9" => segments
                    .last_mut()
                    .ok_or_else(|| "Translation surface has no segment".to_owned())?
                    .push(0),
                "FC" | "FE" | "FF" => {
                    if !is_empty_initial_segment(&segments) {
                        segments.push(vec![0]);
                    }
                    after_page = false;
                }
                _ if is_hex_byte_tag(&tag) => {}
                _ => return Err(format!("Unknown control tag {{{}}} in translation", tag)),
            }
            index += close + 1;
            continue;
        }

        let ch = chars[index];
        if after_page {
            after_page = false;
            if jp_table.contains_key(&ch) {
                index += 1;
                continue;
            }
        }
        if ch != '\n' && ch != '\r' {
            let line = segments
                .last_mut()
                .ok_or_else(|| "Translation surface has no segment".to_owned())?
                .last_mut()
                .ok_or_else(|| "Translation surface has no line".to_owned())?;
            *line += 1;
        }
        index += 1;
    }

    let visible_cells = segments.iter().flatten().sum();
    let max_explicit_line_cells = segments.iter().flatten().copied().max().unwrap_or(0);
    Ok(SurfaceMetrics {
        visible_cells,
        max_explicit_line_cells,
        segments,
    })
}

fn is_hex_byte_tag(tag: &str) -> bool {
    tag.len() == 2 && tag.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_empty_initial_segment(segments: &[Vec<usize>]) -> bool {
    segments.len() == 1 && segments[0].len() == 1 && segments[0][0] == 0
}

fn surface_grew(previous: &SurfaceMetrics, current: &SurfaceMetrics) -> bool {
    current.visible_cells > previous.visible_cells
        || current
            .segments
            .iter()
            .flatten()
            .zip(previous.segments.iter().flatten())
            .any(|(new, old)| new > old)
        || current.segments.iter().flatten().count() > previous.segments.iter().flatten().count()
}

fn fixed_profile_overflow(metrics: &SurfaceMetrics, profile: control::TextBoxProfile) -> bool {
    metrics.segments.iter().any(|segment| {
        segment.len() > profile.max_lines
            || segment
                .iter()
                .any(|cells| cells * CELL_WIDTH_TILES > profile.max_width_tiles)
    })
}

fn parse_snes_address(address: &str) -> Option<SnesAddr> {
    if address.starts_with('$') {
        SnesAddr::parse(address)
    } else {
        None
    }
}

#[cfg(test)]
#[path = "translation_growth_tests.rs"]
mod tests;
