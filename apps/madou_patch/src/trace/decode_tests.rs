use super::*;

#[test]
fn table_has_256_entries() {
    assert_eq!(OPCODE_TABLE.len(), 256);
}

#[test]
fn specific_opcodes() {
    // BRK
    assert_eq!(OPCODE_TABLE[0x00].op, BRK);
    assert_eq!(OPCODE_TABLE[0x00].mode, ImmediateByte);

    // COP
    assert_eq!(OPCODE_TABLE[0x02].op, COP);
    assert_eq!(OPCODE_TABLE[0x02].mode, ImmediateByte);

    // JSR abs
    assert_eq!(OPCODE_TABLE[0x20].op, JSR);
    assert_eq!(OPCODE_TABLE[0x20].mode, Absolute);

    // JSL long
    assert_eq!(OPCODE_TABLE[0x22].op, JSL);
    assert_eq!(OPCODE_TABLE[0x22].mode, AbsLong);

    // JMP abs
    assert_eq!(OPCODE_TABLE[0x4C].op, JMP);
    assert_eq!(OPCODE_TABLE[0x4C].mode, Absolute);

    // JML long
    assert_eq!(OPCODE_TABLE[0x5C].op, JML);
    assert_eq!(OPCODE_TABLE[0x5C].mode, AbsLong);

    // JMP (abs)
    assert_eq!(OPCODE_TABLE[0x6C].op, JMP);
    assert_eq!(OPCODE_TABLE[0x6C].mode, AbsIndirect);

    // JMP (abs,X)
    assert_eq!(OPCODE_TABLE[0x7C].op, JMP);
    assert_eq!(OPCODE_TABLE[0x7C].mode, AbsIndirectX);

    // JML [abs]
    assert_eq!(OPCODE_TABLE[0xDC].op, JML);
    assert_eq!(OPCODE_TABLE[0xDC].mode, AbsIndirectLong);

    // JSR (abs,X)
    assert_eq!(OPCODE_TABLE[0xFC].op, JSR);
    assert_eq!(OPCODE_TABLE[0xFC].mode, AbsIndirectX);

    // RTI / RTS / RTL
    assert_eq!(OPCODE_TABLE[0x40].op, RTI);
    assert_eq!(OPCODE_TABLE[0x60].op, RTS);
    assert_eq!(OPCODE_TABLE[0x6B].op, RTL);

    // REP / SEP
    assert_eq!(OPCODE_TABLE[0xC2].op, REP);
    assert_eq!(OPCODE_TABLE[0xC2].mode, ImmediateByte);
    assert_eq!(OPCODE_TABLE[0xE2].op, SEP);
    assert_eq!(OPCODE_TABLE[0xE2].mode, ImmediateByte);

    // LDA imm
    assert_eq!(OPCODE_TABLE[0xA9].op, LDA);
    assert_eq!(OPCODE_TABLE[0xA9].mode, Immediate);

    // LDA abs
    assert_eq!(OPCODE_TABLE[0xAD].op, LDA);
    assert_eq!(OPCODE_TABLE[0xAD].mode, Absolute);

    // STA abs
    assert_eq!(OPCODE_TABLE[0x8D].op, STA);
    assert_eq!(OPCODE_TABLE[0x8D].mode, Absolute);

    // MVP / MVN
    assert_eq!(OPCODE_TABLE[0x44].op, MVP);
    assert_eq!(OPCODE_TABLE[0x44].mode, BlockMove);
    assert_eq!(OPCODE_TABLE[0x54].op, MVN);
    assert_eq!(OPCODE_TABLE[0x54].mode, BlockMove);

    // BRA / BRL
    assert_eq!(OPCODE_TABLE[0x80].op, BRA);
    assert_eq!(OPCODE_TABLE[0x80].mode, Relative8);
    assert_eq!(OPCODE_TABLE[0x82].op, BRL);
    assert_eq!(OPCODE_TABLE[0x82].mode, Relative16);

    // PER
    assert_eq!(OPCODE_TABLE[0x62].op, PER);
    assert_eq!(OPCODE_TABLE[0x62].mode, Relative16);

    // PEA
    assert_eq!(OPCODE_TABLE[0xF4].op, PEA);
    assert_eq!(OPCODE_TABLE[0xF4].mode, Absolute);

    // PEI
    assert_eq!(OPCODE_TABLE[0xD4].op, PEI);
    assert_eq!(OPCODE_TABLE[0xD4].mode, DirectPage);

    // WAI / STP
    assert_eq!(OPCODE_TABLE[0xCB].op, WAI);
    assert_eq!(OPCODE_TABLE[0xDB].op, STP);

    // WDM
    assert_eq!(OPCODE_TABLE[0x42].op, WDM);
    assert_eq!(OPCODE_TABLE[0x42].mode, ImmediateByte);

    // XBA / XCE
    assert_eq!(OPCODE_TABLE[0xEB].op, XBA);
    assert_eq!(OPCODE_TABLE[0xFB].op, XCE);

    // NOP
    assert_eq!(OPCODE_TABLE[0xEA].op, NOP);
    assert_eq!(OPCODE_TABLE[0xEA].mode, Implied);

    // TXY / TYX
    assert_eq!(OPCODE_TABLE[0x9B].op, TXY);
    assert_eq!(OPCODE_TABLE[0xBB].op, TYX);
}

#[test]
fn branch_opcodes() {
    let branches: &[(u8, Operation)] = &[
        (0x10, BPL),
        (0x30, BMI),
        (0x50, BVC),
        (0x70, BVS),
        (0x90, BCC),
        (0xB0, BCS),
        (0xD0, BNE),
        (0xF0, BEQ),
        (0x80, BRA),
    ];
    for &(code, expected_op) in branches {
        let info = &OPCODE_TABLE[code as usize];
        assert_eq!(info.op, expected_op, "opcode ${:02X}", code);
        assert_eq!(info.mode, Relative8, "opcode ${:02X}", code);
    }
}

#[test]
fn operand_size_implied() {
    assert_eq!(operand_size(NOP, Implied, true, true), 0);
    assert_eq!(operand_size(ASL, Accumulator, true, true), 0);
}

#[test]
fn operand_size_immediate_m_flag() {
    // LDA imm, M=1 (8-bit): 1 byte operand
    assert_eq!(operand_size(LDA, Immediate, true, true), 1);
    // LDA imm, M=0 (16-bit): 2 byte operand
    assert_eq!(operand_size(LDA, Immediate, false, true), 2);

    // ORA imm follows M flag too
    assert_eq!(operand_size(ORA, Immediate, true, false), 1);
    assert_eq!(operand_size(ORA, Immediate, false, false), 2);
}

#[test]
fn operand_size_immediate_x_flag() {
    // LDX imm, X=1 (8-bit): 1 byte
    assert_eq!(operand_size(LDX, Immediate, false, true), 1);
    // LDX imm, X=0 (16-bit): 2 bytes
    assert_eq!(operand_size(LDX, Immediate, false, false), 2);

    // LDY imm, X=1: 1 byte
    assert_eq!(operand_size(LDY, Immediate, true, true), 1);
    // LDY imm, X=0: 2 bytes
    assert_eq!(operand_size(LDY, Immediate, true, false), 2);

    // CPX/CPY also use X flag
    assert_eq!(operand_size(CPX, Immediate, false, true), 1);
    assert_eq!(operand_size(CPX, Immediate, false, false), 2);
    assert_eq!(operand_size(CPY, Immediate, true, true), 1);
    assert_eq!(operand_size(CPY, Immediate, true, false), 2);
}

#[test]
fn operand_size_direct_page() {
    assert_eq!(operand_size(LDA, DirectPage, true, true), 1);
    assert_eq!(operand_size(STA, DpX, true, true), 1);
    assert_eq!(operand_size(LDA, DpIndirectY, true, true), 1);
    assert_eq!(operand_size(LDA, DpIndirectLongY, true, true), 1);
}

#[test]
fn operand_size_absolute() {
    assert_eq!(operand_size(LDA, Absolute, true, true), 2);
    assert_eq!(operand_size(STA, AbsX, true, true), 2);
    assert_eq!(operand_size(LDA, AbsY, true, true), 2);
    assert_eq!(operand_size(JMP, AbsIndirect, true, true), 2);
}

#[test]
fn operand_size_long() {
    assert_eq!(operand_size(LDA, AbsLong, true, true), 3);
    assert_eq!(operand_size(LDA, AbsLongX, true, true), 3);
    assert_eq!(operand_size(JML, AbsIndirectLong, true, true), 3);
    assert_eq!(operand_size(JSL, AbsLong, true, true), 3);
}

#[test]
fn operand_size_relative() {
    assert_eq!(operand_size(BEQ, Relative8, true, true), 1);
    assert_eq!(operand_size(BRL, Relative16, true, true), 2);
    assert_eq!(operand_size(PER, Relative16, true, true), 2);
}

#[test]
fn operand_size_block_move() {
    assert_eq!(operand_size(MVP, BlockMove, true, true), 2);
    assert_eq!(operand_size(MVN, BlockMove, true, true), 2);
}

#[test]
fn operand_size_immediate_byte() {
    assert_eq!(operand_size(BRK, ImmediateByte, true, true), 1);
    assert_eq!(operand_size(COP, ImmediateByte, true, true), 1);
    assert_eq!(operand_size(REP, ImmediateByte, true, true), 1);
    assert_eq!(operand_size(SEP, ImmediateByte, true, true), 1);
    assert_eq!(operand_size(WDM, ImmediateByte, true, true), 1);
}

#[test]
fn operand_size_stack_rel() {
    assert_eq!(operand_size(LDA, StackRel, true, true), 1);
    assert_eq!(operand_size(STA, StackRelIndY, true, true), 1);
}
