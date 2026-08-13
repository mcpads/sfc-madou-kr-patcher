//! Final ROM changes backed by a shared Expected Write plan.
//!
//! Patch stages may read the evolving candidate, but every mutation records one
//! immutable-baseline precondition and final owner. `finish()` validates the
//! complete plan and recreates the output from the baseline in one operation.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::{Deref, Range};

use expected_write::{
    DecodedInstruction, ExpectedWrite, ImageRegion, MachineCodeCheck, MachineCodeProvenance,
    MachineCodeVerifier, MachineCodeVerifierError, RegionKind, WriteIntent, WritePlan,
};
use w65c816::{decode_bytes, encode_bytes, Instruction, WidthState};

use crate::patch::asm::MachineCode;
use crate::rom::lorom_to_pc;

/// ROM write precondition.
#[derive(Clone, Debug)]
pub enum Expect<'a> {
    /// Every baseline byte in the destination must equal this fill value.
    FreeSpace(u8),
    /// The baseline must start with these exact bytes at the destination.
    Bytes(&'a [u8]),
}

#[derive(Debug, Clone)]
enum PlannedIntent {
    Data,
    MachineCode(MachineCode),
}

impl PlannedIntent {
    fn region_kind(&self) -> RegionKind {
        match self {
            Self::Data => RegionKind::Data,
            Self::MachineCode(_) => RegionKind::MachineCode,
        }
    }
}

#[derive(Debug, Clone)]
struct PlannedWrite {
    id: String,
    label: String,
    range: Range<usize>,
    replacement: Option<Vec<u8>>,
    intent: PlannedIntent,
}

/// Readable ROM candidate whose writes are owned by one shared plan.
pub struct TrackedRom {
    baseline: Vec<u8>,
    data: Vec<u8>,
    writes: Vec<PlannedWrite>,
}

impl TrackedRom {
    /// Wrap a ROM that has already passed the project source-identity gate.
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            baseline: data.clone(),
            data,
            writes: Vec::new(),
        }
    }

    /// Record data bytes at a PC offset.
    pub fn write(&mut self, pc: usize, bytes: &[u8], label: &str) {
        self.write_with_intent(pc, bytes, label, PlannedIntent::Data);
    }

    /// Record data bytes at an SNES address.
    pub fn write_snes(&mut self, bank: u8, addr: u16, bytes: &[u8], label: &str) {
        self.write(lorom_to_pc(bank, addr), bytes, label);
    }

    /// Record one data byte at a PC offset.
    pub fn write_byte(&mut self, pc: usize, value: u8, label: &str) {
        self.write(pc, &[value], label);
    }

    /// Record a data range filled with one value.
    pub fn fill(&mut self, pc: usize, len: usize, value: u8, label: &str) {
        if len != 0 {
            self.write(pc, &vec![value; len], label);
        }
    }

    /// Open a data region that becomes one final write when dropped.
    pub fn region(&mut self, pc: usize, len: usize, label: &str) -> RomRegion<'_> {
        self.region_with_intent(pc, len, label, PlannedIntent::Data)
    }

    /// Verify a baseline precondition and record data bytes.
    pub fn write_expect(&mut self, pc: usize, bytes: &[u8], label: &str, expect: &Expect) {
        self.verify_expectation(pc, bytes.len(), label, expect);
        self.write(pc, bytes, label);
    }

    /// Verify a baseline precondition and record data bytes at an SNES address.
    pub fn write_snes_expect(
        &mut self,
        bank: u8,
        addr: u16,
        bytes: &[u8],
        label: &str,
        expect: &Expect,
    ) {
        self.write_expect(lorom_to_pc(bank, addr), bytes, label, expect);
    }

    /// Verify a baseline precondition and record typed machine code.
    pub fn write_machine_code_expect(
        &mut self,
        machine_code: &MachineCode,
        label: &str,
        expect: &Expect,
    ) {
        let pc = lorom_to_pc(machine_code.bank(), machine_code.addr());
        self.verify_expectation(pc, machine_code.len(), label, expect);
        self.write_with_intent(
            pc,
            machine_code.bytes(),
            label,
            PlannedIntent::MachineCode(machine_code.clone()),
        );
    }

    /// Verify a baseline precondition and fill a data range.
    #[allow(dead_code)]
    pub fn fill_expect(&mut self, pc: usize, len: usize, value: u8, label: &str, expect: &Expect) {
        self.verify_expectation(pc, len, label, expect);
        self.fill(pc, len, value, label);
    }

    /// Verify a baseline precondition and open one final data write.
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

    /// Validate the full plan and the evolving candidate without consuming it.
    #[cfg(test)]
    pub fn check(&self) -> Result<(), String> {
        let output = self.apply_plan()?;
        if output != self.data {
            return Err("Expected Write output differs from the patch candidate".to_string());
        }
        Ok(())
    }

    /// Validate the full plan and recreate the final image from immutable input.
    pub fn finish(self) -> Result<Vec<u8>, String> {
        let output = self.apply_plan()?;
        if output != self.data {
            return Err("Expected Write output differs from the patch candidate".to_string());
        }
        Ok(output)
    }

    /// Test helper returning only a successfully verified image.
    #[cfg(test)]
    pub fn into_inner(self) -> Vec<u8> {
        self.finish().expect("Expected Write verification failed")
    }

    /// Print the complete write ownership map.
    #[allow(dead_code)]
    pub fn dump_regions(&self) {
        let mut writes: Vec<_> = self.writes.iter().collect();
        writes.sort_by_key(|write| write.range.start);
        println!("Expected Write plan ({} writes):", writes.len());
        for write in writes {
            println!(
                "  [0x{:06X}..0x{:06X}) {:>6} bytes  {}",
                write.range.start,
                write.range.end,
                write.range.len(),
                write.label
            );
        }
    }

    fn apply_plan(&self) -> Result<Vec<u8>, String> {
        let plan = self.build_plan()?;
        let verifier = W65C816WriteVerifier::new(&self.writes);
        plan.apply(&self.baseline, Some(&verifier))
            .map_err(|error| format!("Expected Write verification failed: {error}"))
    }

    fn write_with_intent(&mut self, pc: usize, bytes: &[u8], label: &str, intent: PlannedIntent) {
        if bytes.is_empty() {
            return;
        }
        self.require_range(pc, bytes.len(), label);
        let id = self.next_write_id(label);
        self.writes.push(PlannedWrite {
            id,
            label: label.to_owned(),
            range: pc..pc + bytes.len(),
            replacement: Some(bytes.to_vec()),
            intent,
        });
        self.data[pc..pc + bytes.len()].copy_from_slice(bytes);
    }

    fn region_with_intent(
        &mut self,
        pc: usize,
        len: usize,
        label: &str,
        intent: PlannedIntent,
    ) -> RomRegion<'_> {
        self.require_range(pc, len, label);
        let replacement = if len == 0 {
            None
        } else {
            let id = self.next_write_id(label);
            self.writes.push(PlannedWrite {
                id,
                label: label.to_owned(),
                range: pc..pc + len,
                replacement: None,
                intent,
            });
            let last_index = self.writes.len() - 1;
            Some(&mut self.writes[last_index].replacement)
        };
        RomRegion {
            slice: &mut self.data[pc..pc + len],
            replacement,
        }
    }

    fn verify_expectation(&self, pc: usize, len: usize, label: &str, expect: &Expect) {
        self.require_range(pc, len, label);
        match expect {
            Expect::FreeSpace(fill) => {
                let end = pc + len;
                assert!(
                    self.baseline[pc..end].iter().all(|&byte| byte == *fill),
                    "[{label}] Expected free space (0x{fill:02X}) at PC 0x{pc:X}..0x{end:X}, but found non-free bytes: {:02X?}",
                    &self.baseline[pc..end.min(pc + 16)],
                );
            }
            Expect::Bytes(expected) => {
                assert!(
                    expected.len() <= len,
                    "[{label}] Expect::Bytes length ({}) exceeds write length ({len})",
                    expected.len(),
                );
                let actual = &self.baseline[pc..pc + expected.len()];
                assert!(
                    actual == *expected,
                    "[{label}] Expected bytes {:02X?} at PC 0x{pc:X}, found {:02X?}",
                    expected,
                    actual,
                );
            }
        }
    }

    fn require_range(&self, pc: usize, len: usize, label: &str) {
        let end = pc
            .checked_add(len)
            .unwrap_or_else(|| panic!("[{label}] ROM write address overflow: 0x{pc:X} + {len}"));
        assert!(
            end <= self.data.len(),
            "[{label}] ROM write out of range: 0x{pc:X}..0x{end:X}, ROM size 0x{:X}",
            self.data.len(),
        );
    }

    fn next_write_id(&self, label: &str) -> String {
        format!("write-{:04}-{label}", self.writes.len())
    }

    fn build_plan(&self) -> Result<WritePlan, String> {
        let mut plan = WritePlan::new();
        for write in &self.writes {
            let replacement = write
                .replacement
                .as_ref()
                .ok_or_else(|| format!("[{}] ROM region write was not completed", write.label))?;
            let intent = match &write.intent {
                PlannedIntent::Data => WriteIntent::Data,
                PlannedIntent::MachineCode(machine_code) => {
                    WriteIntent::MachineCode(MachineCodeProvenance {
                        assembly_source_id: write.id.clone(),
                        isa_profile_id: machine_code.entry_mode().profile_id().to_owned(),
                    })
                }
            };
            plan = plan
                .region(ImageRegion {
                    id: format!("region-{}", write.id),
                    range: write.range.clone(),
                    kind: write.intent.region_kind(),
                    reason: write.label.clone(),
                })
                .write(ExpectedWrite {
                    id: write.id.clone(),
                    owner: write.label.clone(),
                    purpose: write.label.clone(),
                    offset: write.range.start,
                    expected_original: self.baseline[write.range.clone()].to_vec(),
                    replacement: replacement.clone(),
                    intent,
                });
        }
        Ok(plan)
    }
}

impl Deref for TrackedRom {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.data
    }
}

/// Mutable region that freezes into one final write when dropped.
pub struct RomRegion<'a> {
    slice: &'a mut [u8],
    replacement: Option<&'a mut Option<Vec<u8>>>,
}

impl RomRegion<'_> {
    /// Return mutable access inside the already-owned region.
    pub fn data_mut(&mut self) -> &mut [u8] {
        self.slice
    }

    /// Copy bytes at an offset inside the owned region.
    pub fn copy_at(&mut self, offset: usize, src: &[u8]) {
        self.slice[offset..offset + src.len()].copy_from_slice(src);
    }

    /// Read the current region contents.
    #[cfg(test)]
    pub fn data(&self) -> &[u8] {
        self.slice
    }

    /// Return the region length.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.slice.len()
    }

    /// Return whether the region is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.slice.is_empty()
    }
}

impl Drop for RomRegion<'_> {
    fn drop(&mut self) {
        if let Some(replacement) = &mut self.replacement {
            **replacement = Some(self.slice.to_vec());
        }
    }
}

#[derive(Debug, Clone)]
struct ReassembledInstruction {
    canonical: String,
    instruction: Instruction,
    widths: WidthState,
    offset: usize,
    len: usize,
}

struct W65C816WriteVerifier<'a> {
    writes: &'a [PlannedWrite],
    decoded: RefCell<HashMap<String, Vec<ReassembledInstruction>>>,
}

impl<'a> W65C816WriteVerifier<'a> {
    fn new(writes: &'a [PlannedWrite]) -> Self {
        Self {
            writes,
            decoded: RefCell::new(HashMap::new()),
        }
    }

    fn source(&self, source_id: &str) -> Result<&MachineCode, MachineCodeVerifierError> {
        self.writes
            .iter()
            .find(|write| write.id == source_id)
            .and_then(|write| match &write.intent {
                PlannedIntent::MachineCode(machine_code) => Some(machine_code),
                PlannedIntent::Data => None,
            })
            .ok_or_else(|| {
                MachineCodeVerifierError::new(format!(
                    "unregistered W65C816 assembly source: {source_id}"
                ))
            })
    }
}

impl MachineCodeVerifier for W65C816WriteVerifier<'_> {
    fn assemble_source(
        &self,
        check: &MachineCodeCheck<'_>,
    ) -> Result<Vec<u8>, MachineCodeVerifierError> {
        self.source(&check.provenance.assembly_source_id)?
            .reassemble()
            .map(|program| program.into_bytes())
            .map_err(MachineCodeVerifierError::new)
    }

    fn disassemble(
        &self,
        check: &MachineCodeCheck<'_>,
    ) -> Result<Vec<DecodedInstruction>, MachineCodeVerifierError> {
        let source = self.source(&check.provenance.assembly_source_id)?;
        let assembled = source.reassemble().map_err(MachineCodeVerifierError::new)?;
        let states = assembled.instruction_width_states();
        let mut cursor = 0usize;
        let mut decoded = Vec::with_capacity(states.len());
        let mut typed = Vec::with_capacity(states.len());

        for (index, state) in states.iter().enumerate() {
            if state.offset != cursor {
                return Err(MachineCodeVerifierError::new(format!(
                    "W65C816 source instruction gap at 0x{cursor:X}"
                )));
            }
            let instruction = decode_bytes(&check.write.replacement[cursor..], state.widths)
                .map_err(|error| {
                    MachineCodeVerifierError::new(format!(
                        "W65C816 decode failed at 0x{cursor:X}: {error}"
                    ))
                })?;
            let canonical = format!(
                "{}:{index:04}:{:?}:{:?}",
                check.write.id, state.widths, instruction.instruction
            );
            decoded.push(DecodedInstruction {
                offset: cursor,
                len: instruction.len,
                canonical: canonical.clone(),
            });
            typed.push(ReassembledInstruction {
                canonical,
                instruction: instruction.instruction,
                widths: state.widths,
                offset: cursor,
                len: instruction.len,
            });
            cursor += instruction.len;
        }
        if cursor != check.write.replacement.len() {
            return Err(MachineCodeVerifierError::new(format!(
                "W65C816 decoded length mismatch: 0x{cursor:X} != 0x{:X}",
                check.write.replacement.len()
            )));
        }
        self.decoded
            .borrow_mut()
            .insert(check.write.id.clone(), typed);
        Ok(decoded)
    }

    fn assemble_decoded(
        &self,
        check: &MachineCodeCheck<'_>,
        instructions: &[DecodedInstruction],
    ) -> Result<Vec<u8>, MachineCodeVerifierError> {
        let decoded = self.decoded.borrow();
        let typed = decoded.get(&check.write.id).ok_or_else(|| {
            MachineCodeVerifierError::new(format!(
                "W65C816 decoded source missing for {}",
                check.write.id
            ))
        })?;
        if typed.len() != instructions.len() {
            return Err(MachineCodeVerifierError::new(
                "W65C816 decoded instruction count changed",
            ));
        }

        let mut bytes = Vec::new();
        for (expected, supplied) in typed.iter().zip(instructions) {
            if supplied.canonical != expected.canonical
                || supplied.offset != expected.offset
                || supplied.len != expected.len
            {
                return Err(MachineCodeVerifierError::new(
                    "W65C816 decoded instruction identity changed",
                ));
            }
            bytes.extend(
                encode_bytes(&expected.instruction, expected.widths)
                    .map_err(|error| MachineCodeVerifierError::new(error.to_string()))?,
            );
        }
        Ok(bytes)
    }
}

#[cfg(test)]
#[path = "tracked_rom_tests.rs"]
mod tests;
