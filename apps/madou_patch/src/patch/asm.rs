//! Placement-aware W65C816 assembly adapter for hook code generation.
//!
//! The project-local instruction vocabulary remains compact, while opcode
//! selection, M/X widths, label placement, branch ranges, and final decoding
//! are owned by `retro-typed-isa`'s complete `w65c816` profile.

use w65c816::{
    AddressingMode, AssembledProgram, Assembler, CodeLocation, CpuMode, Instruction, Mnemonic,
    Operand, Width, WidthState,
};

/// 65816 instruction (subset used by hooks).
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Inst {
    /// REP #imm — C2 xx
    Rep(u8),
    /// SEP #imm — E2 xx
    Sep(u8),
    /// LDA dp — A5 xx
    LdaDp(u8),
    /// LDA #imm8 (M=1) — A9 xx
    LdaImm8(u8),
    /// LDA #imm16 (M=0) — A9 xx xx
    LdaImm16(u16),
    /// LDA abs — AD xx xx
    LdaAbs(u16),
    /// STA dp — 85 xx
    StaDp(u8),
    /// STA abs — 8D xx xx
    StaAbs(u16),
    /// CMP #imm8 (M=1) — C9 xx
    CmpImm8(u8),
    /// CMP #imm16 (M=0) — C9 xx xx
    CmpImm16(u16),
    /// CMP dp — C5 dp
    CmpDp(u8),
    /// LDA [dp],Y — B7 dp (long indirect indexed Y)
    LdaDpIndirectLongY(u8),
    /// STZ dp — 64 dp
    StzDp(u8),
    /// AND #imm16 (M=0) — 29 xx xx
    AndImm16(u16),
    /// BEQ label — F0 rr
    Beq(&'static str),
    /// BNE label — D0 rr
    Bne(&'static str),
    /// BMI label — 30 rr
    Bmi(&'static str),
    /// BPL label — 10 rr
    Bpl(&'static str),
    /// BCS label — B0 rr (branch if carry set)
    Bcs(&'static str),
    /// BCC label — 90 rr (branch if carry clear)
    Bcc(&'static str),
    /// BRA label — 80 rr (65C816 always-branch)
    Bra(&'static str),
    /// INC dp — E6 xx
    IncDp(u8),
    /// INC abs — EE xx xx
    IncAbs(u16),
    /// STZ abs — 9C xx xx
    StzAbs(u16),
    /// DEC dp — C6 xx
    DecDp(u8),
    /// INX — E8
    Inx,
    /// INY — C8
    Iny,
    /// TAY — A8
    Tay,
    /// TYA — 98
    Tya,
    /// PHB — 8B
    Phb,
    /// PLB — AB
    Plb,
    /// JSL long — 22 xx xx xx
    Jsl(u32),
    /// JML long — 5C xx xx xx
    Jml(u32),
    /// RTL — 6B
    Rtl,
    /// PHP — 08
    Php,
    /// PLP — 28
    Plp,
    /// PHA — 48
    Pha,
    /// PLA — 68
    Pla,
    /// SEI — 78
    Sei,
    /// CLI — 58
    Cli,
    /// NOP — EA
    Nop,
    /// PHX — DA
    Phx,
    /// PLX — FA
    Plx,
    /// PHY — 5A
    Phy,
    /// PLY — 7A
    Ply,
    /// DEC A — 3A
    DecA,
    /// ASL A — 0A
    AslA,
    /// CLC — 18
    Clc,
    /// SEC — 38
    Sec,
    /// ADC #imm8 (M=1) — 69 xx
    AdcImm8(u8),
    /// ADC #imm16 (M=0) — 69 xx xx
    AdcImm16(u16),
    /// SBC #imm8 (M=1) — E9 xx
    SbcImm8(u8),
    /// SBC #imm16 (M=0) — E9 xx xx
    SbcImm16(u16),
    /// SBC dp — E5 xx
    SbcDp(u8),
    /// ADC dp — 65 xx
    AdcDp(u8),
    /// EOR #imm8 — 49 xx
    EorImm8(u8),
    /// AND #imm8 (M=1) — 29 xx
    AndImm8(u8),
    /// LDA abs,X — BD xx xx
    LdaAbsX(u16),
    /// STA abs,X — 9D xx xx
    StaAbsX(u16),
    /// STA abs,Y — 99 xx xx
    StaAbsY(u16),
    /// LDA abs,Y — B9 xx xx
    LdaAbsY(u16),
    /// STA long — 8F xx xx xx
    StaLong(u32),
    /// STA long,X — 9F xx xx xx
    StaLongX(u32),
    /// XBA — EB — Exchange B and A
    Xba,
    /// INC A — 1A — Increment Accumulator
    IncA,
    /// JMP abs — 4C xx xx
    JmpAbs(u16),
    /// LDA long — AF xx xx xx
    LdaLong(u32),
    /// LDX #imm16 (X=0) — A2 lo hi
    LdxImm16(u16),
    /// LDY #imm16 (X=0) — A0 lo hi
    LdyImm16(u16),
    /// MVN dst,src — 54 dst src (block move next)
    Mvn(u8, u8),
    /// Pseudo-instruction: label definition (0 bytes).
    Label(&'static str),
}

/// Native-mode accumulator/index widths at hook entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum ExecutionMode {
    M8X8,
    M8X16,
    M16X8,
    M16X16,
}

impl ExecutionMode {
    fn widths(self) -> WidthState {
        match self {
            Self::M8X8 => WidthState::M8_X8,
            Self::M8X16 => WidthState::M8_X16,
            Self::M16X8 => WidthState::M16_X8,
            Self::M16X16 => WidthState::M16_X16,
        }
    }

    fn cpu_mode(self) -> CpuMode {
        CpuMode::Native(self.widths())
    }

    pub(crate) fn profile_id(self) -> &'static str {
        match self {
            Self::M8X8 => "w65c816-native-m8-x8",
            Self::M8X16 => "w65c816-native-m8-x16",
            Self::M16X8 => "w65c816-native-m16-x8",
            Self::M16X16 => "w65c816-native-m16-x16",
        }
    }
}

/// Typed W65C816 machine code retaining source, placement, and entry mode.
#[derive(Debug, Clone)]
pub struct MachineCode {
    program: Vec<Inst>,
    bank: u8,
    addr: u16,
    entry_mode: ExecutionMode,
    assembled: AssembledProgram,
}

impl MachineCode {
    pub fn bytes(&self) -> &[u8] {
        self.assembled.bytes()
    }

    pub fn len(&self) -> usize {
        self.bytes().len()
    }

    pub(crate) fn bank(&self) -> u8 {
        self.bank
    }

    pub(crate) fn addr(&self) -> u16 {
        self.addr
    }

    pub(crate) fn entry_mode(&self) -> ExecutionMode {
        self.entry_mode
    }

    pub(crate) fn reassemble(&self) -> Result<AssembledProgram, String> {
        assemble_program_at(&self.program, self.bank, self.addr, self.entry_mode)
    }
}

/// Compile typed instructions while preserving their actual placement and mode.
pub fn compile_machine_code(
    program: Vec<Inst>,
    bank: u8,
    addr: u16,
    entry_mode: ExecutionMode,
) -> Result<MachineCode, String> {
    let assembled = assemble_program_at(&program, bank, addr, entry_mode)?;
    Ok(MachineCode {
        program,
        bank,
        addr,
        entry_mode,
        assembled,
    })
}

fn assemble_program_at(
    program: &[Inst],
    bank: u8,
    addr: u16,
    entry_mode: ExecutionMode,
) -> Result<AssembledProgram, String> {
    let mut assembler = Assembler::new();
    let initial_mode = entry_mode.cpu_mode();
    let mut widths = entry_mode.widths();
    let mut branch_join = false;

    for inst in program {
        if matches!(inst, Inst::Label(_)) {
            branch_join = true;
        } else if branch_join && apply_immediate_width_requirement(&mut widths, inst) {
            assembler.assume_mode(CpuMode::Native(widths));
            branch_join = false;
        }
        emit_typed(&mut assembler, inst);
        apply_status_width_change(&mut widths, inst);
        if matches!(inst, Inst::Plp) {
            widths = entry_mode.widths();
            assembler.assume_mode(initial_mode);
            branch_join = false;
        }
    }

    assembler
        .assemble(CodeLocation::new(bank, addr), initial_mode)
        .map_err(|error| {
            format!(
                "W65C816 hook assembly failed @ ${bank:02X}:${addr:04X} ({entry_mode:?}): {error}"
            )
        })
}

/// Assemble typed instructions at their actual SNES placement and entry mode.
#[cfg(test)]
pub fn assemble_at(
    program: &[Inst],
    bank: u8,
    addr: u16,
    entry_mode: ExecutionMode,
) -> Result<Vec<u8>, String> {
    compile_machine_code(program.to_vec(), bank, addr, entry_mode)
        .map(|machine_code| machine_code.bytes().to_vec())
}

/// Compile a fixed-size executable replacement and reject size drift.
pub fn compile_fixed_machine_code<const N: usize>(
    program: Vec<Inst>,
    bank: u8,
    addr: u16,
    entry_mode: ExecutionMode,
) -> Result<MachineCode, String> {
    let machine_code = compile_machine_code(program, bank, addr, entry_mode)?;
    if machine_code.len() != N {
        return Err(format!(
            "W65C816 replacement size mismatch @ ${bank:02X}:${addr:04X}: expected {N}, got {}",
            machine_code.len()
        ));
    }
    Ok(machine_code)
}

/// Compile a four-byte JSL replacement at its installation site.
pub fn compile_jsl(
    bank: u8,
    addr: u16,
    target: u32,
    entry_mode: ExecutionMode,
) -> Result<MachineCode, String> {
    compile_fixed_machine_code::<4>(vec![Inst::Jsl(target)], bank, addr, entry_mode)
}

/// Test-only synthetic placement for instruction-level checks.
#[cfg(test)]
pub fn assemble(program: &[Inst]) -> Result<Vec<u8>, String> {
    let mut outputs = Vec::new();
    let mut errors = Vec::new();
    for mode in [
        ExecutionMode::M8X8,
        ExecutionMode::M8X16,
        ExecutionMode::M16X8,
        ExecutionMode::M16X16,
    ] {
        match assemble_at(program, 0x00, 0x8000, mode) {
            Ok(bytes) => outputs.push(bytes),
            Err(error) => errors.push(error),
        }
    }
    let Some(expected) = outputs.first() else {
        return Err(errors.join("; "));
    };
    if outputs.iter().any(|bytes| bytes != expected) {
        return Err("synthetic assembly depends on entry M/X mode".to_string());
    }
    Ok(expected.clone())
}

fn apply_immediate_width_requirement(widths: &mut WidthState, inst: &Inst) -> bool {
    use Inst::*;
    match inst {
        LdaImm8(_) | CmpImm8(_) | AdcImm8(_) | SbcImm8(_) | EorImm8(_) | AndImm8(_) => {
            widths.accumulator = Width::Eight;
            true
        }
        LdaImm16(_) | CmpImm16(_) | AndImm16(_) | AdcImm16(_) | SbcImm16(_) => {
            widths.accumulator = Width::Sixteen;
            true
        }
        LdxImm16(_) | LdyImm16(_) => {
            widths.index = Width::Sixteen;
            true
        }
        _ => false,
    }
}

fn apply_status_width_change(widths: &mut WidthState, inst: &Inst) {
    let (width, mask) = match inst {
        Inst::Rep(mask) => (Width::Sixteen, *mask),
        Inst::Sep(mask) => (Width::Eight, *mask),
        _ => return,
    };
    if mask & 0x20 != 0 {
        widths.accumulator = width;
    }
    if mask & 0x10 != 0 {
        widths.index = width;
    }
}

fn emit_typed(assembler: &mut Assembler, inst: &Inst) {
    use AddressingMode as Mode;
    use Inst::*;
    use Mnemonic as Mn;

    let emit = |assembler: &mut Assembler, mnemonic, mode, operand| {
        assembler.emit(Instruction::new(mnemonic, mode, operand));
    };
    let implied = |assembler: &mut Assembler, mnemonic| {
        assembler.emit(Instruction::new(mnemonic, Mode::Implied, Operand::None));
    };
    let accumulator = |assembler: &mut Assembler, mnemonic| {
        assembler.emit(Instruction::new(mnemonic, Mode::Accumulator, Operand::None));
    };

    match inst {
        Label(name) => {
            assembler.label(*name);
        }
        Beq(label) => {
            assembler.emit_label_ref(Mn::Beq, Mode::Relative8, *label);
        }
        Bne(label) => {
            assembler.emit_label_ref(Mn::Bne, Mode::Relative8, *label);
        }
        Bmi(label) => {
            assembler.emit_label_ref(Mn::Bmi, Mode::Relative8, *label);
        }
        Bpl(label) => {
            assembler.emit_label_ref(Mn::Bpl, Mode::Relative8, *label);
        }
        Bcs(label) => {
            assembler.emit_label_ref(Mn::Bcs, Mode::Relative8, *label);
        }
        Bcc(label) => {
            assembler.emit_label_ref(Mn::Bcc, Mode::Relative8, *label);
        }
        Bra(label) => {
            assembler.emit_label_ref(Mn::Bra, Mode::Relative8, *label);
        }
        Rep(value) => emit(
            assembler,
            Mn::Rep,
            Mode::ImmediateByte,
            Operand::Byte(*value),
        ),
        Sep(value) => emit(
            assembler,
            Mn::Sep,
            Mode::ImmediateByte,
            Operand::Byte(*value),
        ),
        LdaDp(value) => emit(assembler, Mn::Lda, Mode::DirectPage, Operand::Byte(*value)),
        LdaImm8(value) => emit(assembler, Mn::Lda, Mode::Immediate, Operand::Byte(*value)),
        LdaImm16(value) => emit(assembler, Mn::Lda, Mode::Immediate, Operand::Word(*value)),
        LdaAbs(value) => emit(assembler, Mn::Lda, Mode::Absolute, Operand::Word(*value)),
        StaDp(value) => emit(assembler, Mn::Sta, Mode::DirectPage, Operand::Byte(*value)),
        StaAbs(value) => emit(assembler, Mn::Sta, Mode::Absolute, Operand::Word(*value)),
        CmpImm8(value) => emit(assembler, Mn::Cmp, Mode::Immediate, Operand::Byte(*value)),
        CmpImm16(value) => emit(assembler, Mn::Cmp, Mode::Immediate, Operand::Word(*value)),
        CmpDp(value) => emit(assembler, Mn::Cmp, Mode::DirectPage, Operand::Byte(*value)),
        LdaDpIndirectLongY(value) => emit(
            assembler,
            Mn::Lda,
            Mode::DirectPageIndirectLongIndexedY,
            Operand::Byte(*value),
        ),
        StzDp(value) => emit(assembler, Mn::Stz, Mode::DirectPage, Operand::Byte(*value)),
        AndImm16(value) => emit(assembler, Mn::And, Mode::Immediate, Operand::Word(*value)),
        IncDp(value) => emit(assembler, Mn::Inc, Mode::DirectPage, Operand::Byte(*value)),
        IncAbs(value) => emit(assembler, Mn::Inc, Mode::Absolute, Operand::Word(*value)),
        StzAbs(value) => emit(assembler, Mn::Stz, Mode::Absolute, Operand::Word(*value)),
        DecDp(value) => emit(assembler, Mn::Dec, Mode::DirectPage, Operand::Byte(*value)),
        AdcImm8(value) => emit(assembler, Mn::Adc, Mode::Immediate, Operand::Byte(*value)),
        AdcImm16(value) => emit(assembler, Mn::Adc, Mode::Immediate, Operand::Word(*value)),
        SbcImm8(value) => emit(assembler, Mn::Sbc, Mode::Immediate, Operand::Byte(*value)),
        SbcImm16(value) => emit(assembler, Mn::Sbc, Mode::Immediate, Operand::Word(*value)),
        SbcDp(value) => emit(assembler, Mn::Sbc, Mode::DirectPage, Operand::Byte(*value)),
        AdcDp(value) => emit(assembler, Mn::Adc, Mode::DirectPage, Operand::Byte(*value)),
        EorImm8(value) => emit(assembler, Mn::Eor, Mode::Immediate, Operand::Byte(*value)),
        AndImm8(value) => emit(assembler, Mn::And, Mode::Immediate, Operand::Byte(*value)),
        LdaAbsX(value) => emit(assembler, Mn::Lda, Mode::AbsoluteX, Operand::Word(*value)),
        StaAbsX(value) => emit(assembler, Mn::Sta, Mode::AbsoluteX, Operand::Word(*value)),
        StaAbsY(value) => emit(assembler, Mn::Sta, Mode::AbsoluteY, Operand::Word(*value)),
        LdaAbsY(value) => emit(assembler, Mn::Lda, Mode::AbsoluteY, Operand::Word(*value)),
        StaLong(value) => emit(
            assembler,
            Mn::Sta,
            Mode::AbsoluteLong,
            Operand::Long(*value),
        ),
        StaLongX(value) => emit(
            assembler,
            Mn::Sta,
            Mode::AbsoluteLongX,
            Operand::Long(*value),
        ),
        LdaLong(value) => emit(
            assembler,
            Mn::Lda,
            Mode::AbsoluteLong,
            Operand::Long(*value),
        ),
        Jsl(value) => emit(
            assembler,
            Mn::Jsl,
            Mode::AbsoluteLong,
            Operand::Long(*value),
        ),
        Jml(value) => emit(
            assembler,
            Mn::Jml,
            Mode::AbsoluteLong,
            Operand::Long(*value),
        ),
        JmpAbs(value) => emit(assembler, Mn::Jmp, Mode::Absolute, Operand::Word(*value)),
        LdxImm16(value) => emit(assembler, Mn::Ldx, Mode::Immediate, Operand::Word(*value)),
        LdyImm16(value) => emit(assembler, Mn::Ldy, Mode::Immediate, Operand::Word(*value)),
        Mvn(dst, src) => emit(
            assembler,
            Mn::Mvn,
            Mode::BlockMove,
            Operand::BlockMove {
                destination_bank: *dst,
                source_bank: *src,
            },
        ),
        DecA => accumulator(assembler, Mn::Dec),
        AslA => accumulator(assembler, Mn::Asl),
        IncA => accumulator(assembler, Mn::Inc),
        Inx => implied(assembler, Mn::Inx),
        Iny => implied(assembler, Mn::Iny),
        Tay => implied(assembler, Mn::Tay),
        Tya => implied(assembler, Mn::Tya),
        Phb => implied(assembler, Mn::Phb),
        Plb => implied(assembler, Mn::Plb),
        Rtl => implied(assembler, Mn::Rtl),
        Php => implied(assembler, Mn::Php),
        Plp => implied(assembler, Mn::Plp),
        Pha => implied(assembler, Mn::Pha),
        Pla => implied(assembler, Mn::Pla),
        Sei => implied(assembler, Mn::Sei),
        Cli => implied(assembler, Mn::Cli),
        Nop => implied(assembler, Mn::Nop),
        Phx => implied(assembler, Mn::Phx),
        Plx => implied(assembler, Mn::Plx),
        Phy => implied(assembler, Mn::Phy),
        Ply => implied(assembler, Mn::Ply),
        Clc => implied(assembler, Mn::Clc),
        Sec => implied(assembler, Mn::Sec),
        Xba => implied(assembler, Mn::Xba),
    };
}

#[cfg(test)]
#[path = "asm_tests.rs"]
mod tests;
