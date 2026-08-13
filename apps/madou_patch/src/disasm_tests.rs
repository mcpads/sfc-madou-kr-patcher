use super::*;

/// Build a minimal test ROM large enough to hold code at the given SNES address.
fn make_test_rom(snes: SnesAddr, code: &[u8]) -> Vec<u8> {
    let pc = snes.to_pc();
    let size = pc + code.len() + 16;
    let mut rom = vec![0u8; size];
    rom[pc..pc + code.len()].copy_from_slice(code);
    rom
}

#[test]
fn disasm_implied_ops() {
    // NOP, CLC, SEC, PHX
    let code = [0xEA, 0x18, 0x38, 0xDA];
    let start = SnesAddr::new(0x00, 0x8000);
    let rom = make_test_rom(start, &code);
    let lines = disassemble(&rom, start, code.len()).unwrap();

    assert_eq!(lines.len(), 4);
    assert!(lines[0].contains("NOP"));
    assert!(lines[1].contains("CLC"));
    assert!(lines[2].contains("SEC"));
    assert!(lines[3].contains("PHX"));
}

#[test]
fn disasm_lda_immediate_8bit() {
    // SEP #$20 (8-bit A), LDA #$42
    let code = [0xE2, 0x20, 0xA9, 0x42];
    let start = SnesAddr::new(0x00, 0x8000);
    let rom = make_test_rom(start, &code);
    let lines = disassemble(&rom, start, code.len()).unwrap();

    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("SEP"));
    assert!(lines[0].contains("#$20"));
    assert!(lines[1].contains("LDA"));
    assert!(lines[1].contains("#$42"));
}

#[test]
fn disasm_lda_immediate_16bit() {
    // REP #$20 (16-bit A), LDA #$1234
    let code = [0xC2, 0x20, 0xA9, 0x34, 0x12];
    let start = SnesAddr::new(0x00, 0x8000);
    let rom = make_test_rom(start, &code);
    let lines = disassemble(&rom, start, code.len()).unwrap();

    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("REP"));
    assert!(lines[0].contains("#$20"));
    assert!(lines[1].contains("LDA"));
    assert!(lines[1].contains("#$1234"));
}

#[test]
fn disasm_ldx_immediate_flag_tracking() {
    // REP #$10 (16-bit X/Y), LDX #$ABCD
    let code = [0xC2, 0x10, 0xA2, 0xCD, 0xAB];
    let start = SnesAddr::new(0x00, 0x8000);
    let rom = make_test_rom(start, &code);
    let lines = disassemble(&rom, start, code.len()).unwrap();

    assert_eq!(lines.len(), 2);
    assert!(lines[1].contains("LDX"));
    assert!(lines[1].contains("#$ABCD"));
}

#[test]
fn disasm_absolute_addressing() {
    // LDA $4200, STA $2100
    let code = [0xAD, 0x00, 0x42, 0x8D, 0x00, 0x21];
    let start = SnesAddr::new(0x00, 0x8000);
    let rom = make_test_rom(start, &code);
    let lines = disassemble(&rom, start, code.len()).unwrap();

    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("LDA"));
    assert!(lines[0].contains("$4200"));
    assert!(lines[1].contains("STA"));
    assert!(lines[1].contains("$2100"));
}

#[test]
fn disasm_branch_relative() {
    // BNE +$05 at $00:8000 => target $8007 (PC + 2 + 5)
    let code = [0xD0, 0x05];
    let start = SnesAddr::new(0x00, 0x8000);
    let rom = make_test_rom(start, &code);
    let lines = disassemble(&rom, start, code.len()).unwrap();

    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("BNE"));
    assert!(lines[0].contains("$8007"));
}

#[test]
fn disasm_branch_backward() {
    // BEQ -3 (0xFD) at $00:8010 => target $800F (PC + 2 - 3)
    let code = [0xF0, 0xFD];
    let start = SnesAddr::new(0x00, 0x8010);
    let rom = make_test_rom(start, &code);
    let lines = disassemble(&rom, start, code.len()).unwrap();

    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("BEQ"));
    assert!(lines[0].contains("$800F"));
}

#[test]
fn disasm_long_addressing() {
    // JSL $01B400
    let code = [0x22, 0x00, 0xB4, 0x01];
    let start = SnesAddr::new(0x00, 0x8000);
    let rom = make_test_rom(start, &code);
    let lines = disassemble(&rom, start, code.len()).unwrap();

    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("JSL"));
    assert!(lines[0].contains("$01B400"));
}

#[test]
fn disasm_dp_indirect() {
    // LDA ($05), LDA ($0A,X), LDA ($0B),Y
    let code = [0xB2, 0x05, 0xA1, 0x0A, 0xB1, 0x0B];
    let start = SnesAddr::new(0x00, 0x8000);
    let rom = make_test_rom(start, &code);
    let lines = disassemble(&rom, start, code.len()).unwrap();

    assert_eq!(lines.len(), 3);
    assert!(lines[0].contains("($05)"));
    assert!(lines[1].contains("($0A,X)"));
    assert!(lines[2].contains("($0B),Y"));
}

#[test]
fn disasm_block_move() {
    // MVN $7E,$00
    let code = [0x54, 0x7E, 0x00];
    let start = SnesAddr::new(0x00, 0x8000);
    let rom = make_test_rom(start, &code);
    let lines = disassemble(&rom, start, code.len()).unwrap();

    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("MVN"));
    assert!(lines[0].contains("$7E,$00"));
}

#[test]
fn disasm_rep_sep_combined() {
    // REP #$30 (16-bit A+X/Y), LDA #$1234, LDX #$5678, SEP #$30, LDA #$AB
    let code = [
        0xC2, 0x30, // REP #$30
        0xA9, 0x34, 0x12, // LDA #$1234
        0xA2, 0x78, 0x56, // LDX #$5678
        0xE2, 0x30, // SEP #$30
        0xA9, 0xAB, // LDA #$AB
    ];
    let start = SnesAddr::new(0x00, 0x8000);
    let rom = make_test_rom(start, &code);
    let lines = disassemble(&rom, start, code.len()).unwrap();

    assert_eq!(lines.len(), 5);
    assert!(lines[1].contains("#$1234")); // 16-bit LDA
    assert!(lines[2].contains("#$5678")); // 16-bit LDX
    assert!(lines[4].contains("#$AB")); // 8-bit LDA after SEP
}

#[test]
fn disasm_hex_column_format() {
    // Verify hex bytes appear correctly
    // JSR $CE9E => 20 9E CE
    let code = [0x20, 0x9E, 0xCE];
    let start = SnesAddr::new(0x00, 0x8000);
    let rom = make_test_rom(start, &code);
    let lines = disassemble(&rom, start, code.len()).unwrap();

    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("20 9E CE"));
    assert!(lines[0].contains("JSR"));
    assert!(lines[0].contains("$CE9E"));
}

#[test]
fn disasm_stack_relative() {
    // LDA $03,S  ;  LDA ($05,S),Y
    let code = [0xA3, 0x03, 0xB3, 0x05];
    let start = SnesAddr::new(0x00, 0x8000);
    let rom = make_test_rom(start, &code);
    let lines = disassemble(&rom, start, code.len()).unwrap();

    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("$03,S"));
    assert!(lines[1].contains("($05,S),Y"));
}

#[test]
fn disasm_address_column() {
    // Verify address formatting: $BB:AAAA
    let code = [0xEA]; // NOP
    let start = SnesAddr::new(0x02, 0x9000);
    let rom = make_test_rom(start, &code);
    let lines = disassemble(&rom, start, code.len()).unwrap();

    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("$02:9000"));
}

#[test]
fn disasm_accumulator_mode() {
    // ASL A, ROR A
    let code = [0x0A, 0x6A];
    let start = SnesAddr::new(0x00, 0x8000);
    let rom = make_test_rom(start, &code);
    let lines = disassemble(&rom, start, code.len()).unwrap();

    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("ASL"));
    assert!(lines[0].contains("A"));
    assert!(lines[1].contains("ROR"));
    assert!(lines[1].contains("A"));
}
