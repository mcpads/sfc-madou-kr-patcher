//! TrackedRom — ROM write tracking enforced by the type system.
//!
//! All ROM writes go through `TrackedRom` methods, which automatically
//! register regions with the internal `RomRegionTracker`. Direct mutable
//! access (`rom[x] = y`) is prevented by implementing `Deref` but not `DerefMut`.

use crate::patch::rom_regions::RomRegionTracker;
use crate::rom::lorom_to_pc;
use std::ops::Deref;

/// ROM 쓰기 사전 조건.
#[derive(Clone, Debug)]
pub enum Expect<'a> {
    /// 대상 영역이 모두 지정된 바이트(보통 0xFF)여야 함
    FreeSpace(u8),
    /// 대상 영역의 원본 바이트가 정확히 일치해야 함
    Bytes(&'a [u8]),
}

/// A ROM buffer with built-in write tracking.
///
/// - `Deref<Target = [u8]>`: read access via `rom[x]`, `&rom[a..b]`, `rom.len()` etc.
/// - No `DerefMut`: `rom[x] = y` or `rom[a..b].copy_from_slice()` won't compile.
/// - All writes require a label and are automatically registered for collision detection.
pub struct TrackedRom {
    data: Vec<u8>,
    tracker: RomRegionTracker,
}

impl TrackedRom {
    /// Create a new TrackedRom wrapping raw ROM data.
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            data,
            tracker: RomRegionTracker::new(),
        }
    }

    /// Write bytes at a PC (file) offset with automatic region registration.
    pub fn write(&mut self, pc: usize, bytes: &[u8], label: &str) {
        self.tracker.register(pc, bytes.len(), label);
        self.data[pc..pc + bytes.len()].copy_from_slice(bytes);
    }

    /// Write bytes at a SNES bank:addr with automatic region registration.
    pub fn write_snes(&mut self, bank: u8, addr: u16, bytes: &[u8], label: &str) {
        let pc = lorom_to_pc(bank, addr);
        self.write(pc, bytes, label);
    }

    /// Write a single byte at a PC offset.
    pub fn write_byte(&mut self, pc: usize, value: u8, label: &str) {
        self.tracker.register(pc, 1, label);
        self.data[pc] = value;
    }

    /// Fill a region with a single value.
    pub fn fill(&mut self, pc: usize, len: usize, value: u8, label: &str) {
        self.tracker.register(pc, len, label);
        self.data[pc..pc + len].fill(value);
    }

    /// Acquire a mutable region for batch writes (registered once).
    ///
    /// While the `RomRegion` is alive, `self` cannot be used (borrow rules).
    /// Scope the region with `{ }` blocks when needed.
    pub fn region(&mut self, pc: usize, len: usize, label: &str) -> RomRegion<'_> {
        self.tracker.register(pc, len, label);
        RomRegion {
            slice: &mut self.data[pc..pc + len],
        }
    }

    // --- Expectation-checked API ---

    /// Write bytes at a PC offset, verifying a precondition first.
    pub fn write_expect(&mut self, pc: usize, bytes: &[u8], label: &str, expect: &Expect) {
        self.verify_expectation(pc, bytes.len(), label, expect);
        self.write(pc, bytes, label);
    }

    /// Write bytes at a SNES bank:addr, verifying a precondition first.
    pub fn write_snes_expect(
        &mut self,
        bank: u8,
        addr: u16,
        bytes: &[u8],
        label: &str,
        expect: &Expect,
    ) {
        let pc = lorom_to_pc(bank, addr);
        self.write_expect(pc, bytes, label, expect);
    }

    /// Fill a region with a single value, verifying a precondition first.
    #[allow(dead_code)]
    pub fn fill_expect(&mut self, pc: usize, len: usize, value: u8, label: &str, expect: &Expect) {
        self.verify_expectation(pc, len, label, expect);
        self.fill(pc, len, value, label);
    }

    /// Acquire a mutable region, verifying a precondition first.
    pub fn region_expect(
        &mut self,
        pc: usize,
        len: usize,
        label: &str,
        expect: &Expect,
    ) -> RomRegion<'_> {
        self.verify_expectation(pc, len, label, expect);
        self.region(pc, len, label)
    }

    /// Verify that ROM bytes at `[pc..pc+len)` satisfy the expectation.
    fn verify_expectation(&self, pc: usize, len: usize, label: &str, expect: &Expect) {
        match expect {
            Expect::FreeSpace(fill) => {
                let end = pc + len;
                assert!(
                    self.data[pc..end].iter().all(|&b| b == *fill),
                    "[{label}] Expected free space (0x{fill:02X}) at PC 0x{pc:X}..0x{end:X}, \
                     but found non-free bytes: {:02X?}",
                    &self.data[pc..end.min(pc + 16)],
                );
            }
            Expect::Bytes(expected) => {
                assert!(
                    expected.len() <= len,
                    "[{label}] Expect::Bytes length ({}) exceeds write length ({len})",
                    expected.len(),
                );
                let actual = &self.data[pc..pc + expected.len()];
                assert!(
                    actual == *expected,
                    "[{label}] Expected bytes {:02X?} at PC 0x{pc:X}, found {:02X?}",
                    expected,
                    actual,
                );
            }
        }
    }

    /// Check for collisions among all registered write regions.
    pub fn check(&self) -> Result<(), String> {
        self.tracker.check()
    }

    /// Check for untracked writes by comparing against original ROM data.
    pub fn check_untracked_writes(&self, original: &[u8]) -> Result<(), String> {
        self.tracker.check_untracked_writes(original, &self.data)
    }

    /// Consume the TrackedRom and return the underlying data.
    #[cfg(test)]
    pub fn into_inner(self) -> Vec<u8> {
        self.data
    }

    /// Debug: dump all registered regions.
    #[allow(dead_code)]
    pub fn dump_regions(&self) {
        self.tracker.dump();
    }
}

impl Deref for TrackedRom {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.data
    }
}

/// A borrowed mutable sub-slice of the ROM, registered once.
///
/// Use for batch writes to the same region (fill + overwrite patterns).
/// The region is already registered in the tracker — writes within
/// are free from additional registration overhead.
pub struct RomRegion<'a> {
    slice: &'a mut [u8],
}

impl<'a> RomRegion<'a> {
    /// Get a mutable reference to the region's data.
    pub fn data_mut(&mut self) -> &mut [u8] {
        self.slice
    }

    /// Copy `src` bytes at `offset` within this region.
    pub fn copy_at(&mut self, offset: usize, src: &[u8]) {
        self.slice[offset..offset + src.len()].copy_from_slice(src);
    }

    /// Read-only access to the region's data.
    #[cfg(test)]
    pub fn data(&self) -> &[u8] {
        self.slice
    }

    /// Length of the region.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.slice.len()
    }

    /// Whether the region is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.slice.is_empty()
    }
}

#[cfg(test)]
#[path = "tracked_rom_tests.rs"]
mod tests;
