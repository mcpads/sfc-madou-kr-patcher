//! 65816 ASM builder — mini assembler for hook code generation.
//!
//! Two-pass assembler with label-based branch resolution.
//! Only supports the instruction subset used by ROM hooks.

use std::collections::HashMap;

/// 65816 instruction (subset used by hooks).
#[derive(Debug, Clone)]
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
    /// Raw bytes — variable size, for inline data or unsupported opcodes.
    RawBytes(Vec<u8>),
    /// Pseudo-instruction: label definition (0 bytes).
    Label(&'static str),
}

/// Instruction byte size (Label is 0).
#[allow(dead_code)]
fn inst_size(inst: &Inst) -> usize {
    match inst {
        Inst::Rep(_) | Inst::Sep(_) => 2,
        Inst::LdaDp(_) | Inst::LdaImm8(_) | Inst::StaDp(_) | Inst::CmpImm8(_) | Inst::CmpDp(_) => 2,
        Inst::LdaDpIndirectLongY(_) | Inst::StzDp(_) => 2,
        Inst::AdcImm8(_) | Inst::SbcImm8(_) | Inst::SbcDp(_) | Inst::AdcDp(_) | Inst::EorImm8(_) | Inst::AndImm8(_) => 2,
        Inst::IncDp(_) | Inst::DecDp(_) => 2,
        Inst::Beq(_)
        | Inst::Bne(_)
        | Inst::Bmi(_)
        | Inst::Bpl(_)
        | Inst::Bcs(_)
        | Inst::Bcc(_)
        | Inst::Bra(_) => 2,
        Inst::LdaImm16(_) | Inst::LdaAbs(_) | Inst::CmpImm16(_) | Inst::AndImm16(_) => 3,
        Inst::AdcImm16(_) | Inst::SbcImm16(_) => 3,
        Inst::StaAbs(_) | Inst::IncAbs(_) | Inst::StzAbs(_) => 3,
        Inst::LdaAbsX(_) | Inst::StaAbsX(_) | Inst::StaAbsY(_) | Inst::LdaAbsY(_) => 3,
        Inst::LdxImm16(_) | Inst::LdyImm16(_) | Inst::Mvn(_, _) => 3,
        Inst::JmpAbs(_) => 3,
        Inst::Jsl(_) | Inst::Jml(_) | Inst::StaLong(_) | Inst::StaLongX(_) | Inst::LdaLong(_) => 4,
        Inst::Xba | Inst::IncA => 1,
        Inst::Rtl | Inst::Php | Inst::Plp | Inst::Pha | Inst::Pla => 1,
        Inst::Phb | Inst::Plb => 1,
        Inst::Inx | Inst::Iny | Inst::Tay | Inst::Tya => 1,
        Inst::Phx
        | Inst::Plx
        | Inst::Phy
        | Inst::Ply
        | Inst::DecA
        | Inst::AslA
        | Inst::Clc
        | Inst::Sec => 1,
        Inst::Sei | Inst::Cli | Inst::Nop => 1,
        Inst::RawBytes(data) => data.len(),
        Inst::Label(_) => 0,
    }
}

/// Assemble a sequence of instructions into bytes.
///
/// Two-pass: first pass collects label offsets, second pass emits bytes.
/// Branch targets are resolved automatically.
#[allow(dead_code)]
pub fn assemble(program: &[Inst]) -> Result<Vec<u8>, String> {
    // Pass 1: collect label offsets
    let mut labels: HashMap<&str, usize> = HashMap::new();
    let mut offset = 0usize;
    for inst in program {
        if let Inst::Label(name) = inst {
            if labels.contains_key(name) {
                return Err(format!("duplicate label: \"{}\"", name));
            }
            labels.insert(name, offset);
        }
        offset += inst_size(inst);
    }

    // Pass 2: emit bytes
    let mut out = Vec::with_capacity(offset);
    let mut pc = 0usize;
    for inst in program {
        match inst {
            Inst::Rep(v) => {
                out.push(0xC2);
                out.push(*v);
            }
            Inst::Sep(v) => {
                out.push(0xE2);
                out.push(*v);
            }
            Inst::LdaDp(v) => {
                out.push(0xA5);
                out.push(*v);
            }
            Inst::LdaImm8(v) => {
                out.push(0xA9);
                out.push(*v);
            }
            Inst::LdaImm16(v) => {
                out.push(0xA9);
                out.push(*v as u8);
                out.push((*v >> 8) as u8);
            }
            Inst::StaDp(v) => {
                out.push(0x85);
                out.push(*v);
            }
            Inst::CmpImm8(v) => {
                out.push(0xC9);
                out.push(*v);
            }
            Inst::CmpDp(dp) => {
                out.push(0xC5);
                out.push(*dp);
            }
            Inst::LdaDpIndirectLongY(dp) => {
                out.push(0xB7);
                out.push(*dp);
            }
            Inst::StzDp(dp) => {
                out.push(0x64);
                out.push(*dp);
            }
            Inst::CmpImm16(v) => {
                out.push(0xC9);
                out.push(*v as u8);
                out.push((*v >> 8) as u8);
            }
            Inst::AndImm16(v) => {
                out.push(0x29);
                out.push(*v as u8);
                out.push((*v >> 8) as u8);
            }
            Inst::LdaAbs(addr) => {
                out.push(0xAD);
                out.push(*addr as u8);
                out.push((*addr >> 8) as u8);
            }
            Inst::IncDp(v) => {
                out.push(0xE6);
                out.push(*v);
            }
            Inst::DecDp(v) => {
                out.push(0xC6);
                out.push(*v);
            }
            Inst::Beq(label)
            | Inst::Bne(label)
            | Inst::Bmi(label)
            | Inst::Bpl(label)
            | Inst::Bcs(label)
            | Inst::Bcc(label)
            | Inst::Bra(label) => {
                let target = labels
                    .get(label)
                    .ok_or_else(|| format!("undefined label: \"{}\"", label))?;
                let next_pc = pc + 2;
                let rel = (*target as isize) - (next_pc as isize);
                if !(-128..=127).contains(&rel) {
                    return Err(format!(
                        "branch to \"{}\" out of range: {} (must be -128..127)",
                        label, rel
                    ));
                }
                let opcode = match inst {
                    Inst::Beq(_) => 0xF0,
                    Inst::Bne(_) => 0xD0,
                    Inst::Bmi(_) => 0x30,
                    Inst::Bpl(_) => 0x10,
                    Inst::Bcs(_) => 0xB0,
                    Inst::Bcc(_) => 0x90,
                    Inst::Bra(_) => 0x80,
                    _ => unreachable!(),
                };
                out.push(opcode);
                out.push(rel as u8);
            }
            Inst::StaAbs(addr) => {
                out.push(0x8D);
                out.push(*addr as u8);
                out.push((*addr >> 8) as u8);
            }
            Inst::IncAbs(addr) => {
                out.push(0xEE);
                out.push(*addr as u8);
                out.push((*addr >> 8) as u8);
            }
            Inst::StzAbs(addr) => {
                out.push(0x9C);
                out.push(*addr as u8);
                out.push((*addr >> 8) as u8);
            }
            Inst::Jsl(addr) => {
                out.push(0x22);
                out.push(*addr as u8);
                out.push((*addr >> 8) as u8);
                out.push((*addr >> 16) as u8);
            }
            Inst::Jml(addr) => {
                out.push(0x5C);
                out.push(*addr as u8);
                out.push((*addr >> 8) as u8);
                out.push((*addr >> 16) as u8);
            }
            Inst::Rtl => out.push(0x6B),
            Inst::Php => out.push(0x08),
            Inst::Plp => out.push(0x28),
            Inst::Pha => out.push(0x48),
            Inst::Pla => out.push(0x68),
            Inst::Phb => out.push(0x8B),
            Inst::Plb => out.push(0xAB),
            Inst::Inx => out.push(0xE8),
            Inst::Iny => out.push(0xC8),
            Inst::Tay => out.push(0xA8),
            Inst::Tya => out.push(0x98),
            Inst::Phx => out.push(0xDA),
            Inst::Plx => out.push(0xFA),
            Inst::Phy => out.push(0x5A),
            Inst::Ply => out.push(0x7A),
            Inst::DecA => out.push(0x3A),
            Inst::AslA => out.push(0x0A),
            Inst::Clc => out.push(0x18),
            Inst::Sec => out.push(0x38),
            Inst::AdcImm8(v) => {
                out.push(0x69);
                out.push(*v);
            }
            Inst::AdcImm16(v) => {
                out.push(0x69);
                out.push(*v as u8);
                out.push((*v >> 8) as u8);
            }
            Inst::SbcImm8(v) => {
                out.push(0xE9);
                out.push(*v);
            }
            Inst::SbcImm16(v) => {
                out.push(0xE9);
                out.push(*v as u8);
                out.push((*v >> 8) as u8);
            }
            Inst::SbcDp(dp) => {
                out.push(0xE5);
                out.push(*dp);
            }
            Inst::AdcDp(dp) => {
                out.push(0x65);
                out.push(*dp);
            }
            Inst::EorImm8(v) => {
                out.push(0x49);
                out.push(*v);
            }
            Inst::AndImm8(v) => {
                out.push(0x29);
                out.push(*v);
            }
            Inst::LdaAbsX(addr) => {
                out.push(0xBD);
                out.push(*addr as u8);
                out.push((*addr >> 8) as u8);
            }
            Inst::StaAbsX(addr) => {
                out.push(0x9D);
                out.push(*addr as u8);
                out.push((*addr >> 8) as u8);
            }
            Inst::StaAbsY(addr) => {
                out.push(0x99);
                out.push(*addr as u8);
                out.push((*addr >> 8) as u8);
            }
            Inst::LdaAbsY(addr) => {
                out.push(0xB9);
                out.push(*addr as u8);
                out.push((*addr >> 8) as u8);
            }
            Inst::StaLong(addr) => {
                out.push(0x8F);
                out.push(*addr as u8);
                out.push((*addr >> 8) as u8);
                out.push((*addr >> 16) as u8);
            }
            Inst::StaLongX(addr) => {
                out.push(0x9F);
                out.push(*addr as u8);
                out.push((*addr >> 8) as u8);
                out.push((*addr >> 16) as u8);
            }
            Inst::Xba => out.push(0xEB),
            Inst::IncA => out.push(0x1A),
            Inst::JmpAbs(addr) => {
                out.push(0x4C);
                out.push(*addr as u8);
                out.push((*addr >> 8) as u8);
            }
            Inst::LdaLong(addr) => {
                out.push(0xAF);
                out.push(*addr as u8);
                out.push((*addr >> 8) as u8);
                out.push((*addr >> 16) as u8);
            }
            Inst::LdxImm16(v) => {
                out.push(0xA2);
                out.push(*v as u8);
                out.push((*v >> 8) as u8);
            }
            Inst::LdyImm16(v) => {
                out.push(0xA0);
                out.push(*v as u8);
                out.push((*v >> 8) as u8);
            }
            Inst::Mvn(dst, src) => {
                out.push(0x54);
                out.push(*dst);
                out.push(*src);
            }
            Inst::Sei => out.push(0x78),
            Inst::Cli => out.push(0x58),
            Inst::Nop => out.push(0xEA),
            Inst::RawBytes(data) => out.extend_from_slice(data),
            Inst::Label(_) => {}
        }
        pc += inst_size(inst);
    }

    Ok(out)
}

#[cfg(test)]
#[path = "asm_tests.rs"]
mod tests;
