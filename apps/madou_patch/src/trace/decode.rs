/// 65816 opcode decoding: operation enum, addressing mode, and the 256-entry opcode table.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::upper_case_acronyms)]
pub enum Operation {
    ADC,
    AND,
    ASL,
    BCC,
    BCS,
    BEQ,
    BIT,
    BMI,
    BNE,
    BPL,
    BRA,
    BRK,
    BRL,
    BVC,
    BVS,
    CLC,
    CLD,
    CLI,
    CLV,
    CMP,
    COP,
    CPX,
    CPY,
    DEC,
    DEX,
    DEY,
    EOR,
    INC,
    INX,
    INY,
    JMP,
    JML,
    JSR,
    JSL,
    LDA,
    LDX,
    LDY,
    LSR,
    MVN,
    MVP,
    NOP,
    ORA,
    PEA,
    PEI,
    PER,
    PHA,
    PHB,
    PHD,
    PHK,
    PHP,
    PHX,
    PHY,
    PLA,
    PLB,
    PLD,
    PLP,
    PLX,
    PLY,
    REP,
    ROL,
    ROR,
    RTI,
    RTL,
    RTS,
    SBC,
    SEC,
    SED,
    SEI,
    SEP,
    STA,
    STP,
    STX,
    STY,
    STZ,
    TAX,
    TAY,
    TCD,
    TCS,
    TDC,
    TSC,
    TSX,
    TXA,
    TXS,
    TXY,
    TYA,
    TYX,
    TRB,
    TSB,
    WAI,
    WDM,
    XBA,
    XCE,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddrMode {
    Implied,
    Accumulator,
    Immediate,       // size depends on M or X flag
    ImmediateByte,   // always 1 byte (REP, SEP, COP, BRK, WDM)
    DirectPage,      // dp
    DpX,             // dp,X
    DpY,             // dp,Y
    DpIndirect,      // (dp)
    DpIndirectLong,  // [dp]
    DpIndirectX,     // (dp,X)
    DpIndirectY,     // (dp),Y
    DpIndirectLongY, // [dp],Y
    Absolute,        // abs
    AbsX,            // abs,X
    AbsY,            // abs,Y
    AbsLong,         // long (3 bytes)
    AbsLongX,        // long,X
    AbsIndirect,     // (abs)
    AbsIndirectX,    // (abs,X)
    AbsIndirectLong, // [abs]
    Relative8,       // 1-byte relative branch
    Relative16,      // 2-byte relative (BRL, PER)
    StackRel,        // sr,S
    StackRelIndY,    // (sr,S),Y
    BlockMove,       // 2 bytes: dst_bank, src_bank
}

#[derive(Debug, Clone, Copy)]
pub struct OpcodeInfo {
    pub op: Operation,
    pub mode: AddrMode,
}

const fn oi(op: Operation, mode: AddrMode) -> OpcodeInfo {
    OpcodeInfo { op, mode }
}

use AddrMode::*;
use Operation::*;

pub const OPCODE_TABLE: [OpcodeInfo; 256] = [
    // $00-$0F
    oi(BRK, ImmediateByte),  // $00
    oi(ORA, DpIndirectX),    // $01
    oi(COP, ImmediateByte),  // $02
    oi(ORA, StackRel),       // $03
    oi(TSB, DirectPage),     // $04
    oi(ORA, DirectPage),     // $05
    oi(ASL, DirectPage),     // $06
    oi(ORA, DpIndirectLong), // $07
    oi(PHP, Implied),        // $08
    oi(ORA, Immediate),      // $09
    oi(ASL, Accumulator),    // $0A
    oi(PHD, Implied),        // $0B
    oi(TSB, Absolute),       // $0C
    oi(ORA, Absolute),       // $0D
    oi(ASL, Absolute),       // $0E
    oi(ORA, AbsLong),        // $0F
    // $10-$1F
    oi(BPL, Relative8),       // $10
    oi(ORA, DpIndirectY),     // $11
    oi(ORA, DpIndirect),      // $12
    oi(ORA, StackRelIndY),    // $13
    oi(TRB, DirectPage),      // $14
    oi(ORA, DpX),             // $15
    oi(ASL, DpX),             // $16
    oi(ORA, DpIndirectLongY), // $17
    oi(CLC, Implied),         // $18
    oi(ORA, AbsY),            // $19
    oi(INC, Accumulator),     // $1A
    oi(TCS, Implied),         // $1B
    oi(TRB, Absolute),        // $1C
    oi(ORA, AbsX),            // $1D
    oi(ASL, AbsX),            // $1E
    oi(ORA, AbsLongX),        // $1F
    // $20-$2F
    oi(JSR, Absolute),       // $20
    oi(AND, DpIndirectX),    // $21
    oi(JSL, AbsLong),        // $22
    oi(AND, StackRel),       // $23
    oi(BIT, DirectPage),     // $24
    oi(AND, DirectPage),     // $25
    oi(ROL, DirectPage),     // $26
    oi(AND, DpIndirectLong), // $27
    oi(PLP, Implied),        // $28
    oi(AND, Immediate),      // $29
    oi(ROL, Accumulator),    // $2A
    oi(PLD, Implied),        // $2B
    oi(BIT, Absolute),       // $2C
    oi(AND, Absolute),       // $2D
    oi(ROL, Absolute),       // $2E
    oi(AND, AbsLong),        // $2F
    // $30-$3F
    oi(BMI, Relative8),       // $30
    oi(AND, DpIndirectY),     // $31
    oi(AND, DpIndirect),      // $32
    oi(AND, StackRelIndY),    // $33
    oi(BIT, DpX),             // $34
    oi(AND, DpX),             // $35
    oi(ROL, DpX),             // $36
    oi(AND, DpIndirectLongY), // $37
    oi(SEC, Implied),         // $38
    oi(AND, AbsY),            // $39
    oi(DEC, Accumulator),     // $3A
    oi(TSC, Implied),         // $3B
    oi(BIT, AbsX),            // $3C
    oi(AND, AbsX),            // $3D
    oi(ROL, AbsX),            // $3E
    oi(AND, AbsLongX),        // $3F
    // $40-$4F
    oi(RTI, Implied),        // $40
    oi(EOR, DpIndirectX),    // $41
    oi(WDM, ImmediateByte),  // $42
    oi(EOR, StackRel),       // $43
    oi(MVP, BlockMove),      // $44
    oi(EOR, DirectPage),     // $45
    oi(LSR, DirectPage),     // $46
    oi(EOR, DpIndirectLong), // $47
    oi(PHA, Implied),        // $48
    oi(EOR, Immediate),      // $49
    oi(LSR, Accumulator),    // $4A
    oi(PHK, Implied),        // $4B
    oi(JMP, Absolute),       // $4C
    oi(EOR, Absolute),       // $4D
    oi(LSR, Absolute),       // $4E
    oi(EOR, AbsLong),        // $4F
    // $50-$5F
    oi(BVC, Relative8),       // $50
    oi(EOR, DpIndirectY),     // $51
    oi(EOR, DpIndirect),      // $52
    oi(EOR, StackRelIndY),    // $53
    oi(MVN, BlockMove),       // $54
    oi(EOR, DpX),             // $55
    oi(LSR, DpX),             // $56
    oi(EOR, DpIndirectLongY), // $57
    oi(CLI, Implied),         // $58
    oi(EOR, AbsY),            // $59
    oi(PHY, Implied),         // $5A
    oi(TCD, Implied),         // $5B
    oi(JML, AbsLong),         // $5C
    oi(EOR, AbsX),            // $5D
    oi(LSR, AbsX),            // $5E
    oi(EOR, AbsLongX),        // $5F
    // $60-$6F
    oi(RTS, Implied),        // $60
    oi(ADC, DpIndirectX),    // $61
    oi(PER, Relative16),     // $62
    oi(ADC, StackRel),       // $63
    oi(STZ, DirectPage),     // $64
    oi(ADC, DirectPage),     // $65
    oi(ROR, DirectPage),     // $66
    oi(ADC, DpIndirectLong), // $67
    oi(PLA, Implied),        // $68
    oi(ADC, Immediate),      // $69
    oi(ROR, Accumulator),    // $6A
    oi(RTL, Implied),        // $6B
    oi(JMP, AbsIndirect),    // $6C
    oi(ADC, Absolute),       // $6D
    oi(ROR, Absolute),       // $6E
    oi(ADC, AbsLong),        // $6F
    // $70-$7F
    oi(BVS, Relative8),       // $70
    oi(ADC, DpIndirectY),     // $71
    oi(ADC, DpIndirect),      // $72
    oi(ADC, StackRelIndY),    // $73
    oi(STZ, DpX),             // $74
    oi(ADC, DpX),             // $75
    oi(ROR, DpX),             // $76
    oi(ADC, DpIndirectLongY), // $77
    oi(SEI, Implied),         // $78
    oi(ADC, AbsY),            // $79
    oi(PLY, Implied),         // $7A
    oi(TDC, Implied),         // $7B
    oi(JMP, AbsIndirectX),    // $7C
    oi(ADC, AbsX),            // $7D
    oi(ROR, AbsX),            // $7E
    oi(ADC, AbsLongX),        // $7F
    // $80-$8F
    oi(BRA, Relative8),      // $80
    oi(STA, DpIndirectX),    // $81
    oi(BRL, Relative16),     // $82
    oi(STA, StackRel),       // $83
    oi(STY, DirectPage),     // $84
    oi(STA, DirectPage),     // $85
    oi(STX, DirectPage),     // $86
    oi(STA, DpIndirectLong), // $87
    oi(DEY, Implied),        // $88
    oi(BIT, Immediate),      // $89
    oi(TXA, Implied),        // $8A
    oi(PHB, Implied),        // $8B
    oi(STY, Absolute),       // $8C
    oi(STA, Absolute),       // $8D
    oi(STX, Absolute),       // $8E
    oi(STA, AbsLong),        // $8F
    // $90-$9F
    oi(BCC, Relative8),       // $90
    oi(STA, DpIndirectY),     // $91
    oi(STA, DpIndirect),      // $92
    oi(STA, StackRelIndY),    // $93
    oi(STY, DpX),             // $94
    oi(STA, DpX),             // $95
    oi(STX, DpY),             // $96
    oi(STA, DpIndirectLongY), // $97
    oi(TYA, Implied),         // $98
    oi(STA, AbsY),            // $99
    oi(TXS, Implied),         // $9A
    oi(TXY, Implied),         // $9B
    oi(STZ, Absolute),        // $9C
    oi(STA, AbsX),            // $9D
    oi(STZ, AbsX),            // $9E
    oi(STA, AbsLongX),        // $9F
    // $A0-$AF
    oi(LDY, Immediate),      // $A0
    oi(LDA, DpIndirectX),    // $A1
    oi(LDX, Immediate),      // $A2
    oi(LDA, StackRel),       // $A3
    oi(LDY, DirectPage),     // $A4
    oi(LDA, DirectPage),     // $A5
    oi(LDX, DirectPage),     // $A6
    oi(LDA, DpIndirectLong), // $A7
    oi(TAY, Implied),        // $A8
    oi(LDA, Immediate),      // $A9
    oi(TAX, Implied),        // $AA
    oi(PLB, Implied),        // $AB
    oi(LDY, Absolute),       // $AC
    oi(LDA, Absolute),       // $AD
    oi(LDX, Absolute),       // $AE
    oi(LDA, AbsLong),        // $AF
    // $B0-$BF
    oi(BCS, Relative8),       // $B0
    oi(LDA, DpIndirectY),     // $B1
    oi(LDA, DpIndirect),      // $B2
    oi(LDA, StackRelIndY),    // $B3
    oi(LDY, DpX),             // $B4
    oi(LDA, DpX),             // $B5
    oi(LDX, DpY),             // $B6
    oi(LDA, DpIndirectLongY), // $B7
    oi(CLV, Implied),         // $B8
    oi(LDA, AbsY),            // $B9
    oi(TSX, Implied),         // $BA
    oi(TYX, Implied),         // $BB
    oi(LDY, AbsX),            // $BC
    oi(LDA, AbsX),            // $BD
    oi(LDX, AbsY),            // $BE
    oi(LDA, AbsLongX),        // $BF
    // $C0-$CF
    oi(CPY, Immediate),      // $C0
    oi(CMP, DpIndirectX),    // $C1
    oi(REP, ImmediateByte),  // $C2
    oi(CMP, StackRel),       // $C3
    oi(CPY, DirectPage),     // $C4
    oi(CMP, DirectPage),     // $C5
    oi(DEC, DirectPage),     // $C6
    oi(CMP, DpIndirectLong), // $C7
    oi(INY, Implied),        // $C8
    oi(CMP, Immediate),      // $C9
    oi(DEX, Implied),        // $CA
    oi(WAI, Implied),        // $CB
    oi(CPY, Absolute),       // $CC
    oi(CMP, Absolute),       // $CD
    oi(DEC, Absolute),       // $CE
    oi(CMP, AbsLong),        // $CF
    // $D0-$DF
    oi(BNE, Relative8),       // $D0
    oi(CMP, DpIndirectY),     // $D1
    oi(CMP, DpIndirect),      // $D2
    oi(CMP, StackRelIndY),    // $D3
    oi(PEI, DirectPage),      // $D4
    oi(CMP, DpX),             // $D5
    oi(DEC, DpX),             // $D6
    oi(CMP, DpIndirectLongY), // $D7
    oi(CLD, Implied),         // $D8
    oi(CMP, AbsY),            // $D9
    oi(PHX, Implied),         // $DA
    oi(STP, Implied),         // $DB
    oi(JML, AbsIndirectLong), // $DC
    oi(CMP, AbsX),            // $DD
    oi(DEC, AbsX),            // $DE
    oi(CMP, AbsLongX),        // $DF
    // $E0-$EF
    oi(CPX, Immediate),      // $E0
    oi(SBC, DpIndirectX),    // $E1
    oi(SEP, ImmediateByte),  // $E2
    oi(SBC, StackRel),       // $E3
    oi(CPX, DirectPage),     // $E4
    oi(SBC, DirectPage),     // $E5
    oi(INC, DirectPage),     // $E6
    oi(SBC, DpIndirectLong), // $E7
    oi(INX, Implied),        // $E8
    oi(SBC, Immediate),      // $E9
    oi(NOP, Implied),        // $EA
    oi(XBA, Implied),        // $EB
    oi(CPX, Absolute),       // $EC
    oi(SBC, Absolute),       // $ED
    oi(INC, Absolute),       // $EE
    oi(SBC, AbsLong),        // $EF
    // $F0-$FF
    oi(BEQ, Relative8),       // $F0
    oi(SBC, DpIndirectY),     // $F1
    oi(SBC, DpIndirect),      // $F2
    oi(SBC, StackRelIndY),    // $F3
    oi(PEA, Absolute),        // $F4
    oi(SBC, DpX),             // $F5
    oi(INC, DpX),             // $F6
    oi(SBC, DpIndirectLongY), // $F7
    oi(SED, Implied),         // $F8
    oi(SBC, AbsY),            // $F9
    oi(PLX, Implied),         // $FA
    oi(XCE, Implied),         // $FB
    oi(JSR, AbsIndirectX),    // $FC
    oi(SBC, AbsX),            // $FD
    oi(INC, AbsX),            // $FE
    oi(SBC, AbsLongX),        // $FF
];

/// Returns the number of operand bytes for an instruction, given the addressing
/// mode and current processor flags. For `Immediate` mode, the size depends on
/// whether the operation targets an index register (X flag) or memory/accumulator
/// (M flag).
pub fn operand_size(op: Operation, mode: AddrMode, flag_m: bool, flag_x: bool) -> usize {
    match mode {
        Implied | Accumulator => 0,
        ImmediateByte => 1,
        Immediate => {
            let is_index = matches!(op, LDX | LDY | CPX | CPY);
            if is_index {
                if flag_x {
                    1
                } else {
                    2
                }
            } else if flag_m {
                1
            } else {
                2
            }
        }
        DirectPage | DpX | DpY | DpIndirect | DpIndirectLong | DpIndirectX | DpIndirectY
        | DpIndirectLongY | Relative8 | StackRel | StackRelIndY => 1,
        Absolute | AbsX | AbsY | AbsIndirect | AbsIndirectX | Relative16 | BlockMove => 2,
        AbsLong | AbsLongX | AbsIndirectLong => 3,
    }
}

#[cfg(test)]
#[path = "decode_tests.rs"]
mod tests;
