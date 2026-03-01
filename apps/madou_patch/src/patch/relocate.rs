//! Text relocation engine for overflow strings.
//!
//! When Korean text is longer than the original Japanese text,
//! the overflow strings are relocated to free ROM banks ($31-$33).
//! Pointer tables are updated to reference the new locations.

use crate::patch::tracked_rom::{Expect, TrackedRom};
use crate::patch::translation;
use crate::rom::SnesAddr;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Build free space regions for relocated text.
/// Bank $32 start is dynamic — computed from the pipeline address chain.
///
/// IMPORTANT: Regions must NOT overlap with:
///   - Encyclopedia hooks at $33:$F000+ (encyclopedia.rs)
///   - Sky worldmap LZ data at $0B:$8000-$ECFF (worldmap.rs)
fn build_free_regions(bank32_start: u16) -> Vec<(u8, u16, u16)> {
    vec![
        (0x32, bank32_start, 0xFFFF),
        (0x33, 0x8000, 0xEFFF), // ~28KB (reserve $F000+ for encyclopedia)
        (0x31, 0xC700, 0xFFFF), // ~14KB (JP data at $8000-$C6FB)
        (0x25, 0xC8A2, 0xFFFF), // ~14KB (trailing free space in JP ROM)
        (0x19, 0xF300, 0xFFFF), // ~3.3KB (savemenu data at $D460-$F2FF)
    ]
}

/// Bank configs for chain-based relocation.
/// Bank 01: fc_split=true (chain boundaries needed for pointer relocation).
const RELOCATE_TEXT_BANKS: &[(&str, bool)] = &[
    ("01", true),
    ("03", false),
    ("1D", true),
    ("2A", true),
    ("2B", true),
    ("2D", true),
];

/// Count control codes (FC/FE/F9/FD) in `bytes[0..offset]` and the number of
/// text bytes between the last control code and `offset`.
/// Returns (control_count, text_bytes_after_last_control).
fn count_controls_before(bytes: &[u8], offset: usize) -> (usize, usize) {
    let mut count = 0;
    let mut last_control_end = 0;
    let mut i = 0;
    while i < offset && i < bytes.len() {
        match bytes[i] {
            0xFC | 0xFA | 0xF0 | 0xF1 => {
                count += 1;
                i += if i + 1 < bytes.len() { 2 } else { 1 };
                last_control_end = i;
            }
            0xF9 | 0xFD | 0xFE => {
                count += 1;
                i += 1;
                last_control_end = i;
            }
            0xFB => {
                // FB is a character combo prefix, not a control code
                i += if i + 1 < bytes.len() { 2 } else { 1 };
            }
            0xFF => break,
            _ => {
                i += 1;
            }
        }
    }
    (count, offset.saturating_sub(last_control_end))
}

/// Find the byte position right AFTER the Nth control code (1-indexed) in `bytes`.
/// Returns None if fewer than `n` control codes exist.
fn find_after_nth_control(bytes: &[u8], n: usize) -> Option<usize> {
    let mut count = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            0xFC | 0xFA | 0xF0 | 0xF1 => {
                count += 1;
                i += if i + 1 < bytes.len() { 2 } else { 1 };
                if count == n {
                    return Some(i);
                }
            }
            0xF9 | 0xFD | 0xFE => {
                count += 1;
                i += 1;
                if count == n {
                    return Some(i);
                }
            }
            0xFB => {
                i += if i + 1 < bytes.len() { 2 } else { 1 };
            }
            0xFF => break,
            _ => {
                i += 1;
            }
        }
    }
    None
}

/// A string's fit analysis result.
struct FitResult {
    #[allow(dead_code)]
    index: usize,
    addr: SnesAddr,
    pc: usize,
    orig_len: usize,
    encoded: Vec<u8>,
    fits: bool,
}

/// Stats from relocation.
pub struct RelocateStats {
    pub banks_processed: usize,
    pub inplace: usize,
    pub relocated: usize,
    pub skipped: usize,
}

/// Within-bank free space for 2-byte pointer targets.
/// Strings referenced by 2-byte pointers MUST stay in the same bank.
const WITHIN_BANK_FREE: &[(u8, u16, u16)] = &[
    (0x01, 0xFD79, 0xFFFF), // Bank $01: 647 bytes (세이브 메뉴)
    (0x03, 0xDA70, 0xF7FF), // Bank $03: 7,568 bytes (일기 diary; $F800+ reserved for encyclopedia)
];

/// Free space allocator.
struct FreeSpace {
    regions: Vec<(u8, u16, u16)>, // (bank, next_addr, end_addr)
    /// Within-bank regions for 2-byte pointer targets.
    within_bank: Vec<(u8, u16, u16)>,
}

impl FreeSpace {
    fn new(bank32_start: u16) -> Self {
        Self {
            regions: build_free_regions(bank32_start),
            within_bank: WITHIN_BANK_FREE.to_vec(),
        }
    }

    /// Allocate `size` bytes, returning the SNES address.
    fn alloc(&mut self, size: usize) -> Result<SnesAddr, String> {
        for region in &mut self.regions {
            let available = (region.2 as usize + 1).saturating_sub(region.1 as usize);
            if available >= size {
                let addr = SnesAddr::new(region.0, region.1);
                // Cap at 0xFFFF to avoid u16 overflow when region fills exactly
                region.1 = (region.1 as usize + size).min(0xFFFF) as u16;
                return Ok(addr);
            }
        }
        Err(format!(
            "No free space for {} bytes in relocation regions",
            size
        ))
    }

    /// Allocate from within-bank free space (for 2-byte pointer targets).
    fn alloc_within_bank(&mut self, bank: u8, size: usize) -> Result<SnesAddr, String> {
        for region in &mut self.within_bank {
            if region.0 == bank {
                let available = (region.2 as usize + 1).saturating_sub(region.1 as usize);
                if available >= size {
                    let addr = SnesAddr::new(region.0, region.1);
                    region.1 = (region.1 as usize + size).min(0xFFFF) as u16;
                    return Ok(addr);
                }
            }
        }
        Err(format!(
            "No free space in Bank ${:02X} for {} bytes",
            bank, size
        ))
    }

    /// Reclaim freed text data space as within-bank free space.
    /// Called after cross-bank relocations free up original string locations.
    fn reclaim_within_bank(&mut self, bank: u8, start: u16, size: usize) {
        if size == 0 {
            return;
        }
        let end = (start as usize + size - 1).min(0xFFFF) as u16;
        self.within_bank.push((bank, start, end));
    }
}

/// Load 2-byte pointer table target addresses from ROM.
/// Returns a set of SnesAddr values that the table references.
fn load_2byte_targets(rom: &[u8], bank: u8, tables: &[(usize, usize)]) -> HashSet<SnesAddr> {
    let mut targets = HashSet::new();
    for &(table_pc, entry_count) in tables {
        for i in 0..entry_count {
            let pc = table_pc + i * 2;
            if pc + 2 <= rom.len() {
                let offset = u16::from_le_bytes([rom[pc], rom[pc + 1]]);
                if offset >= 0x8000 {
                    targets.insert(SnesAddr::new(bank, offset));
                }
            }
        }
    }
    targets
}

/// Count FC (BOX) tags in `bytes[0..offset]`.
fn count_fc_before(bytes: &[u8], offset: usize) -> usize {
    let mut count = 0;
    let mut i = 0;
    while i < offset && i < bytes.len() {
        if bytes[i] == 0xFC {
            count += 1;
            i += if i + 1 < bytes.len() { 2 } else { 1 };
        } else if matches!(bytes[i], 0xFA | 0xFB | 0xF0 | 0xF1) {
            i += if i + 1 < bytes.len() { 2 } else { 1 };
        } else {
            i += 1;
        }
    }
    count
}

/// Find the byte position of the Nth FC tag (0-indexed) in `bytes`.
fn find_nth_fc(bytes: &[u8], n: usize) -> Option<usize> {
    let mut count = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0xFC {
            if count == n {
                return Some(i);
            }
            count += 1;
            i += if i + 1 < bytes.len() { 2 } else { 1 };
        } else if matches!(bytes[i], 0xFA | 0xFB | 0xF0 | 0xF1) {
            i += if i + 1 < bytes.len() { 2 } else { 1 };
        } else {
            i += 1;
        }
    }
    None
}

/// Compute offset mapping for a phantom within a chain.
/// Returns (chain_entry_index, ko_offset_in_entry, tag_index, cumulative_ko_before).
///
/// Two strategies tried in order:
/// 1. **FC counting**: If phantom starts with FC (BOX tag), count FC tags in JP
///    before the phantom and find the matching FC tag in KO. Works for phantoms
///    at FC boundaries regardless of preceding text bytes.
/// 2. **Control-code boundary**: If phantom is immediately after ANY control code
///    (F9/FE/FC/FA/F0/F1), count all controls before it and find the matching
///    position in KO. Works for non-FC phantoms at control boundaries.
fn compute_phantom_offset(
    rom: &[u8],
    chain: &[usize],
    results: &[FitResult],
    phantom_addr: &SnesAddr,
    chain_start_addr: u16,
    chain_end_addr: u16,
) -> Option<(usize, usize, usize, usize)> {
    if phantom_addr.addr < chain_start_addr || phantom_addr.addr >= chain_end_addr {
        return None;
    }
    let ppc = phantom_addr.to_pc();
    if ppc >= rom.len() {
        return None;
    }
    let phantom_chain_offset = (phantom_addr.addr - chain_start_addr) as usize;
    let mut cum_jp = 0usize;
    let mut cum_ko = 0usize;
    for (ci, &entry_idx) in chain.iter().enumerate() {
        let entry = &results[entry_idx];
        if phantom_chain_offset >= cum_jp && phantom_chain_offset < cum_jp + entry.orig_len {
            let offset_in_entry = phantom_chain_offset - cum_jp;
            let jp_bytes = &rom[entry.pc..entry.pc + entry.orig_len];

            // Strategy 1: Phantom starts with FC → count FC tags
            if rom[ppc] == 0xFC {
                let fc_index = count_fc_before(jp_bytes, offset_in_entry);
                if let Some(ko_off) = find_nth_fc(&entry.encoded, fc_index) {
                    return Some((ci, ko_off, fc_index, cum_ko));
                }
            }

            // Strategy 2: Phantom right after a control code (text_gap == 0)
            let (ctrl_count, text_gap) = count_controls_before(jp_bytes, offset_in_entry);
            if text_gap == 0 && ctrl_count > 0 {
                if let Some(ko_off) = find_after_nth_control(&entry.encoded, ctrl_count) {
                    return Some((ci, ko_off, ctrl_count, cum_ko));
                }
            }

            return None;
        }
        cum_jp += entry.orig_len;
        cum_ko += entry.encoded.len();
    }
    None
}

/// Scan ROM for a single FF-terminated entry starting at `pc`.
/// Returns the length including the FF terminator.
fn scan_ff_entry_len(rom: &[u8], pc: usize) -> usize {
    let mut end = pc;
    while end < rom.len() && rom[end] != 0xFF {
        if matches!(rom[end], 0xFA | 0xFB | 0xFC | 0xF0 | 0xF1) && end + 1 < rom.len() {
            end += 2;
        } else {
            end += 1;
        }
    }
    end += 1; // include FF
    end - pc
}

/// Analyze fit for all strings in a bank.
///
/// For FC-split banks, also detects untranslated entries between consecutive
/// translation entries. These "gap" entries (e.g., branch continuation markers)
/// are included as passthrough with their original ROM bytes to preserve chain
/// structure during repack.
fn analyze_fit(
    rom: &[u8],
    bank_id: &str,
    translations_dir: &Path,
    ko_table: &HashMap<char, Vec<u8>>,
    fc_split: bool,
) -> Result<Vec<FitResult>, String> {
    let entries = translation::load_and_encode_bank(translations_dir, bank_id, ko_table, fc_split)?;

    let pc_list: Vec<usize> = entries.iter().map(|e| e.addr.to_pc()).collect();
    let mut results = Vec::new();

    for (i, entry) in entries.iter().enumerate() {
        let pc = pc_list[i];

        // For FC-split banks with a next entry: scan ROM for gap entries
        // between this entry's address and the next translation entry.
        if fc_split && i + 1 < entries.len() {
            let next_pc = pc_list[i + 1];

            // Scan the first FF-terminated entry at this position.
            // Cap at distance to next translation entry: multiple entries
            // can share a single FF-terminated block (separated by FC/FE).
            let ff_len = scan_ff_entry_len(rom, pc);
            let actual_len = ff_len.min(next_pc - pc);
            let fits = entry.encoded.len() <= actual_len;

            results.push(FitResult {
                index: results.len(),
                addr: entry.addr,
                pc,
                orig_len: actual_len,
                encoded: entry.encoded.clone(),
                fits,
            });

            // Check for untranslated gap entries between this entry and the next
            let mut gap_pc = pc + ff_len;
            while gap_pc < next_pc {
                let gap_len = scan_ff_entry_len(rom, gap_pc);
                let gap_snes_addr = pc_to_snes(gap_pc, entry.addr.bank);
                let gap_bytes = rom[gap_pc..gap_pc + gap_len].to_vec();

                results.push(FitResult {
                    index: results.len(),
                    addr: gap_snes_addr,
                    pc: gap_pc,
                    orig_len: gap_len,
                    encoded: gap_bytes, // passthrough: original ROM bytes
                    fits: true,
                });

                gap_pc += gap_len;
            }
        } else {
            // Last entry or non-FC-split: scan to FF
            let orig_len = scan_ff_entry_len(rom, pc);
            let fits = entry.encoded.len() <= orig_len;

            results.push(FitResult {
                index: results.len(),
                addr: entry.addr,
                pc,
                orig_len,
                encoded: entry.encoded.clone(),
                fits,
            });
        }
    }

    Ok(results)
}

/// Convert a PC offset back to a SNES address for the given bank.
fn pc_to_snes(pc: usize, bank: u8) -> SnesAddr {
    let bank_pc_base = (bank as usize & 0x7F) * 0x8000;
    let offset_in_bank = pc - bank_pc_base;
    SnesAddr::new(bank, (offset_in_bank + 0x8000) as u16)
}

/// Relocate all overflow strings across all text banks.
/// `bank32_free_start`: first available SNES address in Bank $32 (from pipeline chain).
pub fn relocate_all(
    rom: &mut TrackedRom,
    bank_ids: &[&str],
    translations_dir: &Path,
    ko_table: &HashMap<char, Vec<u8>>,
    bank32_free_start: u16,
) -> Result<RelocateStats, String> {
    let verbose = std::env::var("RELOCATE_VERBOSE").is_ok();

    let mut stats = RelocateStats {
        banks_processed: 0,
        inplace: 0,
        relocated: 0,
        skipped: 0,
    };
    let mut free = FreeSpace::new(bank32_free_start);

    // Pre-check: verify free regions are actually empty in the ROM
    let free_regions = build_free_regions(bank32_free_start);
    if verbose {
        println!("\n--- Free region pre-check ---");
        for &(bank, start, end) in &free_regions {
            let pc_start = crate::rom::lorom_to_pc(bank, start);
            let pc_end = crate::rom::lorom_to_pc(bank, end);
            if pc_end >= rom.len() {
                println!(
                    "  Bank ${:02X}:${:04X}-${:04X}: OUT OF BOUNDS (ROM {} bytes, need PC 0x{:X})",
                    bank,
                    start,
                    end,
                    rom.len(),
                    pc_end
                );
                continue;
            }
            let nonzero = rom[pc_start..=pc_end]
                .iter()
                .filter(|&&b| b != 0x00 && b != 0xFF)
                .count();
            println!(
                "  Bank ${:02X}:${:04X}-${:04X} (PC 0x{:06X}-0x{:06X}): {} non-zero/FF bytes",
                bank, start, end, pc_start, pc_end, nonzero
            );
        }
    }

    for bank_id in bank_ids {
        let fc_split = RELOCATE_TEXT_BANKS
            .iter()
            .find(|(id, _)| id == bank_id)
            .map(|(_, fc)| *fc)
            .unwrap_or(false);

        println!("\n--- Relocating text: Bank ${} ---", bank_id);

        let results = analyze_fit(rom, bank_id, translations_dir, ko_table, fc_split)
            .map_err(|e| format!("Bank ${}: {}", bank_id, e))?;

        let overflow_count = results.iter().filter(|r| !r.fits).count();
        let total = results.len();
        println!(
            "  Total: {}, Fits: {}, Overflow: {}",
            total,
            total - overflow_count,
            overflow_count
        );

        let bank_num = u8::from_str_radix(bank_id, 16).map_err(|_| "bad bank id".to_string())?;

        // Load 2-byte pointer targets to determine allocation strategy
        let twobyte_tables = crate::patch::pointer_catalog::get_2byte_tables(bank_num);
        let code_embedded = crate::patch::pointer_catalog::get_code_embedded_ptrs(bank_num);
        let mut twobyte_targets = if !twobyte_tables.is_empty() {
            load_2byte_targets(rom, bank_num, twobyte_tables)
        } else {
            HashSet::new()
        };
        for &pc in code_embedded {
            if pc + 2 <= rom.len() {
                let offset = u16::from_le_bytes([rom[pc], rom[pc + 1]]);
                if offset >= 0x8000 {
                    twobyte_targets.insert(SnesAddr::new(bank_num, offset));
                }
            }
        }

        // Collect pointer redirects for this bank
        let mut redirects: Vec<(SnesAddr, SnesAddr)> = Vec::new();

        let fill = if fc_split { 0x00 } else { 0xFF };

        if fc_split {
            // ── Chain-based relocation for FC-split banks ──
            //
            // FC-split banks use sequential dialogue chains: the game has one
            // pointer to the first dialogue line and walks past FF terminators
            // to find subsequent lines. Entries without a catalog pointer are
            // part of such chains and MUST stay contiguous with the chain head.
            //
            // Algorithm:
            // 1. Build set of addresses that have pointer catalog coverage
            // 2. Group consecutive entries into chains (new chain at each
            //    cataloged address)
            // 3. Process each chain atomically:
            //    a. All fit → write each in place
            //    b. Total KO ≤ total orig → repack in place
            //    c. Total KO > total orig → relocate entire chain

            let known_pcs = crate::patch::pointer_catalog::get_pointer_pcs(bank_num);
            let mut catalog_targets: HashSet<SnesAddr> = known_pcs
                .iter()
                .filter_map(|&pc| {
                    if pc + 3 > rom.len() {
                        return None;
                    }
                    Some(SnesAddr::new(
                        rom[pc + 2],
                        u16::from_le_bytes([rom[pc], rom[pc + 1]]),
                    ))
                })
                .collect();

            // Also include 2-byte table targets (e.g., Bank $01 map/save tables)
            for &(table_pc, entry_count) in twobyte_tables {
                for e in 0..entry_count {
                    let ptr_pc = table_pc + e * 2;
                    if ptr_pc + 2 <= rom.len() {
                        let offset = u16::from_le_bytes([rom[ptr_pc], rom[ptr_pc + 1]]);
                        if offset >= 0x8000 {
                            catalog_targets.insert(SnesAddr::new(bank_num, offset));
                        }
                    }
                }
            }

            // Also include code-embedded pointer targets (e.g., Bank $01 LDA #$XXXX)
            for &pc in code_embedded {
                if pc + 2 <= rom.len() {
                    let offset = u16::from_le_bytes([rom[pc], rom[pc + 1]]);
                    if offset >= 0x8000 {
                        catalog_targets.insert(SnesAddr::new(bank_num, offset));
                    }
                }
            }

            // Identify chains: each entry with catalog coverage starts a new chain
            let mut chains: Vec<Vec<usize>> = Vec::new();
            for (i, result) in results.iter().enumerate() {
                if catalog_targets.contains(&result.addr) {
                    chains.push(vec![i]);
                } else if let Some(last_chain) = chains.last_mut() {
                    last_chain.push(i);
                } else {
                    // Entry before any cataloged address — create orphan chain
                    chains.push(vec![i]);
                }
            }

            // Identify phantom entries: catalog targets in this bank not in TSV results.
            // These are addresses that game pointers reference but we don't have translations for.
            // When chains containing them are relocated and zeroed, we must preserve their data.
            let result_addrs: HashSet<SnesAddr> = results.iter().map(|r| r.addr).collect();
            let phantoms_in_bank: Vec<SnesAddr> = catalog_targets
                .iter()
                .filter(|t| t.bank == bank_num && !result_addrs.contains(t))
                .cloned()
                .collect();
            if verbose && !phantoms_in_bank.is_empty() {
                println!(
                    "  Phantom entries (catalog targets not in TSV): {}",
                    phantoms_in_bank.len()
                );
                for p in &phantoms_in_bank {
                    println!("    {}", p);
                }
            }

            let mut chains_relocated = 0usize;
            let mut chains_repacked = 0usize;
            let mut alloc_first: Option<SnesAddr> = None;
            let mut alloc_last: Option<SnesAddr> = None;
            let mut total_alloc_bytes: usize = 0;

            for (chain_idx, chain) in chains.iter().enumerate() {
                let total_orig: usize = chain.iter().map(|&i| results[i].orig_len).sum();
                let total_ko: usize = chain.iter().map(|&i| results[i].encoded.len()).sum();
                let all_fit = chain.iter().all(|&i| results[i].fits);

                if verbose && !all_fit {
                    let head = &results[chain[0]];
                    let decision = if total_ko <= total_orig {
                        "B:repack"
                    } else {
                        "C:relocate"
                    };
                    println!(
                        "    Chain {:>3}: head={} entries={} orig={} ko={} → {}",
                        chain_idx,
                        head.addr,
                        chain.len(),
                        total_orig,
                        total_ko,
                        decision
                    );
                }

                // Compute chain address range (needed for phantom checks in all cases)
                let chain_start_addr = results[chain[0]].addr.addr;
                let chain_end_addr = chain_start_addr.wrapping_add(total_orig as u16);

                if all_fit {
                    // Case A: all entries fit individually → write each in place
                    // Pre-compute phantom redirects (JP bytes still readable from ROM)
                    for phantom_addr in &phantoms_in_bank {
                        if let Some((ci, ko_off, fc_idx, _cum_ko)) = compute_phantom_offset(
                            rom,
                            chain,
                            &results,
                            phantom_addr,
                            chain_start_addr,
                            chain_end_addr,
                        ) {
                            let entry_addr = results[chain[ci]].addr.addr;
                            let new_phantom =
                                SnesAddr::new(bank_num, entry_addr.wrapping_add(ko_off as u16));
                            if new_phantom != *phantom_addr {
                                redirects.push((*phantom_addr, new_phantom));
                                if verbose {
                                    println!(
                                        "      Phantom {} → {} (FC index {}, in-place Case A)",
                                        phantom_addr, new_phantom, fc_idx
                                    );
                                }
                            }
                        }
                    }
                    {
                        let chain_pc = results[chain[0]].pc;
                        let mut rgn = rom.region(chain_pc, total_orig, "relocate:inplace");
                        let mut off = 0;
                        for &i in chain {
                            let e = &results[i];
                            rgn.copy_at(off, &e.encoded);
                            if e.encoded.len() < e.orig_len {
                                rgn.data_mut()[off + e.encoded.len()..off + e.orig_len].fill(fill);
                            }
                            off += e.orig_len;
                            stats.inplace += 1;
                        }
                    }
                } else if total_ko <= total_orig {
                    // Case B: total KO fits in total original space → repack in place
                    // Pre-compute phantom redirects (JP bytes still readable from ROM)
                    for phantom_addr in &phantoms_in_bank {
                        if let Some((_ci, ko_off, fc_idx, cum_ko)) = compute_phantom_offset(
                            rom,
                            chain,
                            &results,
                            phantom_addr,
                            chain_start_addr,
                            chain_end_addr,
                        ) {
                            let total_ko_off = cum_ko + ko_off;
                            let new_phantom = SnesAddr::new(
                                bank_num,
                                chain_start_addr.wrapping_add(total_ko_off as u16),
                            );
                            if new_phantom != *phantom_addr {
                                redirects.push((*phantom_addr, new_phantom));
                                if verbose {
                                    println!(
                                        "      Phantom {} → {} (FC index {}, repack Case B)",
                                        phantom_addr, new_phantom, fc_idx
                                    );
                                }
                            }
                        }
                    }
                    {
                        let chain_pc = results[chain[0]].pc;
                        let mut rgn = rom.region(chain_pc, total_orig, "relocate:repack");
                        let mut offset = 0;
                        for &i in chain {
                            rgn.copy_at(offset, &results[i].encoded);
                            offset += results[i].encoded.len();
                        }
                        rgn.data_mut()[offset..].fill(fill);
                    }
                    stats.inplace += chain.len();
                    chains_repacked += 1;
                } else {
                    // Case C: total KO exceeds original space → relocate entire chain
                    // If any entry in the chain is a 2-byte table target, it must stay
                    // within the same bank (2-byte pointers can't cross banks).
                    let needs_within_bank = chain
                        .iter()
                        .any(|&i| twobyte_targets.contains(&results[i].addr));
                    let new_addr = if needs_within_bank {
                        free.alloc_within_bank(bank_num, total_ko)?
                    } else {
                        free.alloc(total_ko)?
                    };
                    let new_pc = new_addr.to_pc();
                    {
                        let mut rgn = rom.region_expect(
                            new_pc,
                            total_ko,
                            "relocate:new",
                            &Expect::FreeSpace(0xFF),
                        );
                        let mut offset = 0;
                        for &i in chain {
                            rgn.copy_at(offset, &results[i].encoded);
                            offset += results[i].encoded.len();
                        }
                    }
                    // Zero original chain space, redirecting text phantoms
                    // and preserving non-text phantom data.
                    let chain_pc = results[chain[0]].pc;

                    // Compute phantom redirects BEFORE zeroing (need original JP bytes)
                    let mut phantom_preserves: Vec<(usize, Vec<u8>)> = Vec::new();
                    for phantom_addr in &phantoms_in_bank {
                        if phantom_addr.addr < chain_start_addr
                            || phantom_addr.addr >= chain_end_addr
                        {
                            continue;
                        }
                        // Try FC-based redirect for text phantoms
                        if let Some((_ci, ko_off, fc_idx, cum_ko)) = compute_phantom_offset(
                            rom,
                            chain,
                            &results,
                            phantom_addr,
                            chain_start_addr,
                            chain_end_addr,
                        ) {
                            let total_ko_offset = cum_ko + ko_off;
                            let phantom_new = SnesAddr::new(
                                new_addr.bank,
                                new_addr.addr.wrapping_add(total_ko_offset as u16),
                            );
                            redirects.push((*phantom_addr, phantom_new));
                            if verbose {
                                println!(
                                    "      Phantom {} → {} (FC index {}, relocate Case C)",
                                    phantom_addr, phantom_new, fc_idx
                                );
                            }
                        } else {
                            // Non-text or unmappable phantom: preserve data at old location
                            let ppc = phantom_addr.to_pc();
                            let chain_end_pc = chain_pc + total_orig;
                            let mut end = ppc;
                            while end < rom.len().min(chain_end_pc) && rom[end] != 0xFF {
                                if matches!(rom[end], 0xFA | 0xFB | 0xFC | 0xF0 | 0xF1)
                                    && end + 1 < rom.len()
                                {
                                    end += 2;
                                } else {
                                    end += 1;
                                }
                            }
                            end = (end + 1).min(chain_end_pc);
                            if end > ppc {
                                phantom_preserves.push((ppc, rom[ppc..end].to_vec()));
                            }
                        }
                    }

                    {
                        let mut rgn = rom.region(chain_pc, total_orig, "relocate:zero");
                        rgn.data_mut().fill(fill);
                        for (ppc, data) in &phantom_preserves {
                            rgn.copy_at(*ppc - chain_pc, data);
                        }
                    }
                    // Redirect ALL entries in the chain (head + mid-chain catalog targets)
                    let mut redir_offset = 0usize;
                    for &i in chain {
                        let entry_new_addr = SnesAddr::new(
                            new_addr.bank,
                            new_addr.addr.wrapping_add(redir_offset as u16),
                        );
                        redirects.push((results[i].addr, entry_new_addr));
                        redir_offset += results[i].encoded.len();
                    }
                    free.reclaim_within_bank(bank_num, results[chain[0]].addr.addr, total_orig);
                    stats.relocated += chain.len();
                    chains_relocated += 1;
                    total_alloc_bytes += total_ko;
                    if alloc_first.is_none() {
                        alloc_first = Some(new_addr);
                    }
                    alloc_last = Some(SnesAddr::new(
                        new_addr.bank,
                        (new_addr.addr as usize + total_ko - 1).min(0xFFFF) as u16,
                    ));
                }
            }

            if chains_repacked > 0 || chains_relocated > 0 {
                println!(
                    "  Chains: {} total, {} repacked in place, {} relocated",
                    chains.len(),
                    chains_repacked,
                    chains_relocated
                );
            }
            if total_alloc_bytes > 0 {
                println!(
                    "  Alloc: {} bytes, range {}-{}",
                    total_alloc_bytes,
                    alloc_first.map_or("?".to_string(), |a| format!("{}", a)),
                    alloc_last.map_or("?".to_string(), |a| format!("{}", a)),
                );
            }
        } else {
            // ── Per-entry relocation for non-FC-split banks ──
            let mut within_bank_indices: Vec<usize> = Vec::new();
            for (idx, result) in results.iter().enumerate() {
                let ko_data = &result.encoded;

                if result.fits {
                    let pc = result.pc;
                    {
                        let mut rgn = rom.region(pc, result.orig_len, "relocate:inplace");
                        rgn.copy_at(0, ko_data);
                        if ko_data.len() < result.orig_len {
                            rgn.data_mut()[ko_data.len()..].fill(fill);
                        }
                    }
                    stats.inplace += 1;
                } else if twobyte_targets.contains(&result.addr) {
                    within_bank_indices.push(idx);
                } else {
                    let new_addr = free.alloc(ko_data.len())?;
                    let new_pc = new_addr.to_pc();
                    rom.write_expect(
                        new_pc,
                        ko_data,
                        "relocate:cross_bank",
                        &Expect::FreeSpace(0xFF),
                    );
                    rom.fill(result.pc, result.orig_len, fill, "relocate:cross_bank_zero");
                    free.reclaim_within_bank(bank_num, result.addr.addr, result.orig_len);

                    redirects.push((result.addr, new_addr));
                    stats.relocated += 1;
                }
            }

            // Pass 2: Within-bank overflow relocations (non-FC-split only)
            for &idx in &within_bank_indices {
                let result = &results[idx];
                let ko_data = &result.encoded;
                let new_addr = free.alloc_within_bank(bank_num, ko_data.len())?;
                let new_pc = new_addr.to_pc();
                rom.write_expect(
                    new_pc,
                    ko_data,
                    "relocate:within_bank",
                    &Expect::FreeSpace(0xFF),
                );
                rom.fill(
                    result.pc,
                    result.orig_len,
                    fill,
                    "relocate:within_bank_zero",
                );

                redirects.push((result.addr, new_addr));
                stats.relocated += 1;
            }
        }

        // Apply pointer redirects using EN RE precision catalog
        if !redirects.is_empty() {
            let known_pcs = crate::patch::pointer_catalog::get_pointer_pcs(bank_num);
            if known_pcs.is_empty() && twobyte_targets.is_empty() && code_embedded.is_empty() {
                println!(
                    "  WARNING: No pointer catalog entries for bank ${:02X}, {} redirects unapplied",
                    bank_num,
                    redirects.len()
                );
            } else {
                // Pre-check: identify redirects without pointer catalog coverage
                let catalog_targets: HashSet<SnesAddr> = known_pcs
                    .iter()
                    .filter_map(|&pc| {
                        if pc + 3 > rom.len() {
                            return None;
                        }
                        Some(SnesAddr::new(
                            rom[pc + 2],
                            u16::from_le_bytes([rom[pc], rom[pc + 1]]),
                        ))
                    })
                    .collect();

                // Also include code-embedded pointer targets
                let mut all_covered = catalog_targets;
                all_covered.extend(twobyte_targets.iter().cloned());
                for &pc in code_embedded {
                    if pc + 2 <= rom.len() {
                        let offset = u16::from_le_bytes([rom[pc], rom[pc + 1]]);
                        if offset >= 0x8000 {
                            all_covered.insert(SnesAddr::new(bank_num, offset));
                        }
                    }
                }

                let uncovered: Vec<_> = redirects
                    .iter()
                    .filter(|(old, _)| !all_covered.contains(old))
                    .collect();

                if !uncovered.is_empty() {
                    println!(
                        "  WARNING: {} redirects without pointer catalog coverage:",
                        uncovered.len()
                    );
                    for (old, _new) in &uncovered {
                        println!(
                            "    ${:02X}:${:04X} — pointer will NOT be updated!",
                            old.bank, old.addr
                        );
                    }
                }

                let rewritten_3b = if !known_pcs.is_empty() {
                    crate::patch::pointer::rewrite_at_known_pcs(rom, &known_pcs, &redirects)
                } else {
                    0
                };

                // Sub-pointers only for Bank $03 diary table (chapter-number prefix offsets)
                let allow_sub_ptrs = bank_num == 0x03;
                let mut rewritten_2b = 0;
                if !twobyte_tables.is_empty() {
                    for &(table_pc, entry_count) in twobyte_tables {
                        rewritten_2b += crate::patch::pointer::rewrite_2byte_pointer_table(
                            rom,
                            table_pc,
                            entry_count,
                            bank_num,
                            &redirects,
                            allow_sub_ptrs,
                        );
                    }
                }

                let mut rewritten_ce = 0;
                if !code_embedded.is_empty() {
                    rewritten_ce = crate::patch::pointer::rewrite_scattered_2byte_ptrs(
                        rom,
                        code_embedded,
                        bank_num,
                        &redirects,
                    );
                }

                println!(
                    "  Pointers: {} redirects, {} rewritten (3-byte: {}, 2-byte: {}, code: {}, catalog: {} known)",
                    redirects.len(),
                    rewritten_3b + rewritten_2b + rewritten_ce,
                    rewritten_3b,
                    rewritten_2b,
                    rewritten_ce,
                    known_pcs.len()
                );
            }
        }

        // Post-relocation pointer integrity verification (FC-split banks)
        if fc_split {
            let verify_pcs = crate::patch::pointer_catalog::get_pointer_pcs(bank_num);
            let mut bad_count = 0;
            for &pc in &verify_pcs {
                if pc + 3 > rom.len() {
                    continue;
                }
                let target = SnesAddr::new(rom[pc + 2], u16::from_le_bytes([rom[pc], rom[pc + 1]]));
                let target_pc = target.to_pc();
                if target_pc + 16 > rom.len() {
                    continue;
                }

                // Check 1: target starts with 8+ zero bytes (zeroed original location)
                let first8 = &rom[target_pc..target_pc + 8];
                if first8.iter().all(|&b| b == 0x00) {
                    println!(
                        "    BAD-ZERO: PC 0x{:06X} → {} (PC 0x{:06X}): target is zeroed!",
                        pc, target, target_pc
                    );
                    bad_count += 1;
                    continue;
                }

                // Check 2: no FF terminator within 1000 bytes
                let scan_limit = 1000.min(rom.len() - target_pc);
                let mut found_ff = false;
                let mut j = 0;
                while j < scan_limit {
                    let b = rom[target_pc + j];
                    if b == 0xFF {
                        found_ff = true;
                        break;
                    }
                    // Skip 2-byte prefix arguments
                    if matches!(b, 0xFA | 0xFB | 0xFC | 0xF0 | 0xF1) && j + 1 < scan_limit {
                        j += 2;
                    } else {
                        j += 1;
                    }
                }
                if !found_ff {
                    let preview: Vec<u8> =
                        rom[target_pc..std::cmp::min(target_pc + 16, rom.len())].to_vec();
                    println!(
                        "    BAD-NOFF: PC 0x{:06X} → {} (PC 0x{:06X}): no FF in {}B, preview={:02X?}",
                        pc, target, target_pc, scan_limit, preview
                    );
                    bad_count += 1;
                }
            }
            if bad_count > 0 {
                println!(
                    "  VERIFY: {} / {} pointers have integrity issues!",
                    bad_count,
                    verify_pcs.len()
                );
            } else {
                println!("  VERIFY: all {} catalog pointers OK", verify_pcs.len());
            }
        }

        // Print free space state after each bank
        if verbose {
            println!("  Free space state:");
            for (i, &(bank, next, end)) in free.regions.iter().enumerate() {
                let used = if i < free_regions.len() {
                    (next as usize).saturating_sub(free_regions[i].1 as usize)
                } else {
                    0
                };
                let avail = (end as usize + 1).saturating_sub(next as usize);
                println!(
                    "    [{i}] Bank ${bank:02X}: next=${next:04X} end=${end:04X} (used={used}, avail={avail})"
                );
            }
        }

        stats.banks_processed += 1;
    }

    Ok(stats)
}

#[cfg(test)]
#[path = "relocate_tests.rs"]
mod tests;
