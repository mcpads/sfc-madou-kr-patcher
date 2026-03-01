//! ROM write region tracker — collision detection for patch pipeline.
//!
//! Each patch module registers the ROM regions it writes to.
//! After all patches are applied, `check()` detects overlapping regions.

#[cfg(test)]
use crate::rom::lorom_to_pc;

/// A registered ROM write region.
struct RomRegion {
    start_pc: usize, // PC (file offset), inclusive
    end_pc: usize,   // PC (file offset), exclusive
    label: String,
}

/// Collision between two ROM write regions.
pub struct Collision {
    pub a_label: String,
    pub b_label: String,
    pub a_range: (usize, usize),
    pub b_range: (usize, usize),
    pub overlap_start: usize,
    pub overlap_end: usize,
}

/// Tracks ROM write regions and detects collisions.
pub struct RomRegionTracker {
    regions: Vec<RomRegion>,
}

impl RomRegionTracker {
    pub fn new() -> Self {
        Self {
            regions: Vec::new(),
        }
    }

    /// Register a ROM write region by PC (file offset).
    /// Zero-length writes are silently ignored.
    pub fn register(&mut self, start_pc: usize, len: usize, label: &str) {
        if len == 0 {
            return;
        }
        self.regions.push(RomRegion {
            start_pc,
            end_pc: start_pc + len,
            label: label.to_string(),
        });
    }

    /// Register a ROM write region by SNES bank:addr + len.
    /// Convenience wrapper around `register()` with LoROM conversion.
    #[cfg(test)]
    pub fn register_snes(&mut self, bank: u8, addr: u16, len: usize, label: &str) {
        let pc = lorom_to_pc(bank, addr);
        self.register(pc, len, label);
    }

    /// Check all registered regions for collisions.
    /// Returns Ok(()) if no collisions, Err(report) with details otherwise.
    pub fn check(&self) -> Result<(), String> {
        if self.regions.len() < 2 {
            return Ok(());
        }

        // Sort by start_pc (ties broken by end_pc descending for proper containment detection)
        let mut sorted: Vec<&RomRegion> = self.regions.iter().collect();
        sorted.sort_by(|a, b| a.start_pc.cmp(&b.start_pc).then(b.end_pc.cmp(&a.end_pc)));

        let mut collisions: Vec<Collision> = Vec::new();

        for i in 0..sorted.len() - 1 {
            // Check against all subsequent regions that could overlap
            // (since regions are sorted by start, we only need to check forward
            // until start_pc >= current end_pc)
            for j in i + 1..sorted.len() {
                if sorted[j].start_pc >= sorted[i].end_pc {
                    break;
                }
                let overlap_start = sorted[j].start_pc;
                let overlap_end = sorted[i].end_pc.min(sorted[j].end_pc);
                collisions.push(Collision {
                    a_label: sorted[i].label.clone(),
                    b_label: sorted[j].label.clone(),
                    a_range: (sorted[i].start_pc, sorted[i].end_pc),
                    b_range: (sorted[j].start_pc, sorted[j].end_pc),
                    overlap_start,
                    overlap_end,
                });
            }
        }

        if collisions.is_empty() {
            return Ok(());
        }

        let mut report = format!("ROM region collisions detected ({}):\n", collisions.len());
        for c in &collisions {
            report.push_str(&format!(
                "  COLLISION: '{}' [0x{:X}..0x{:X}) vs '{}' [0x{:X}..0x{:X}) — overlap [0x{:X}..0x{:X}) ({} bytes)\n",
                c.a_label, c.a_range.0, c.a_range.1,
                c.b_label, c.b_range.0, c.b_range.1,
                c.overlap_start, c.overlap_end,
                c.overlap_end - c.overlap_start,
            ));
        }
        Err(report)
    }

    /// Compare original and patched ROM, reporting any writes outside registered regions.
    /// This enforces the principle: no untracked writes allowed.
    pub fn check_untracked_writes(&self, original: &[u8], patched: &[u8]) -> Result<(), String> {
        let len = original.len().min(patched.len());

        // Build a sorted, merged list of covered intervals for efficient lookup
        let mut covered: Vec<(usize, usize)> = self
            .regions
            .iter()
            .map(|r| (r.start_pc, r.end_pc))
            .collect();
        covered.sort_by_key(|&(s, _)| s);

        // Merge overlapping/adjacent intervals
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for (s, e) in covered {
            if let Some(last) = merged.last_mut() {
                if s <= last.1 {
                    last.1 = last.1.max(e);
                    continue;
                }
            }
            merged.push((s, e));
        }

        // Scan for untracked changes
        let mut untracked: Vec<(usize, usize)> = Vec::new(); // (start, end) of contiguous untracked runs
        let mut run_start: Option<usize> = None;

        let is_covered = |pc: usize| -> bool {
            merged
                .binary_search_by(|&(s, e)| {
                    if pc < s {
                        std::cmp::Ordering::Greater
                    } else if pc >= e {
                        std::cmp::Ordering::Less
                    } else {
                        std::cmp::Ordering::Equal
                    }
                })
                .is_ok()
        };

        for i in 0..len {
            if original[i] != patched[i] {
                if !is_covered(i) {
                    if run_start.is_none() {
                        run_start = Some(i);
                    }
                } else if let Some(start) = run_start.take() {
                    untracked.push((start, i));
                }
            } else if let Some(start) = run_start.take() {
                untracked.push((start, i));
            }
        }
        if let Some(start) = run_start {
            untracked.push((start, len));
        }

        if untracked.is_empty() {
            return Ok(());
        }

        let total_bytes: usize = untracked.iter().map(|(s, e)| e - s).sum();
        let mut report = format!(
            "Untracked ROM writes detected ({} region(s), {} bytes total):\n",
            untracked.len(),
            total_bytes
        );
        for (s, e) in &untracked {
            report.push_str(&format!(
                "  UNTRACKED: [0x{:06X}..0x{:06X}) ({} bytes)\n",
                s,
                e,
                e - s
            ));
        }
        Err(report)
    }

    /// Print all registered regions sorted by PC offset (debug aid).
    #[allow(dead_code)]
    pub fn dump(&self) {
        let mut sorted: Vec<&RomRegion> = self.regions.iter().collect();
        sorted.sort_by_key(|r| r.start_pc);
        println!("ROM region map ({} regions):", sorted.len());
        for r in &sorted {
            println!(
                "  [0x{:06X}..0x{:06X}) {:>6} bytes  {}",
                r.start_pc,
                r.end_pc,
                r.end_pc - r.start_pc,
                r.label
            );
        }
    }
}

#[cfg(test)]
#[path = "rom_regions_tests.rs"]
mod tests;
