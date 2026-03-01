use super::*;
use Inst::*;

#[test]
fn forward_branch() {
    let prog = vec![Beq("target"), Rtl, Label("target"), Rtl];
    let bytes = assemble(&prog).unwrap();
    assert_eq!(bytes, vec![0xF0, 0x01, 0x6B, 0x6B]);
}

#[test]
fn backward_branch() {
    let prog = vec![Label("loop"), LdaImm8(0x42), Beq("loop")];
    let bytes = assemble(&prog).unwrap();
    assert_eq!(bytes, vec![0xA9, 0x42, 0xF0, 0xFC]);
}

#[test]
fn undefined_label_error() {
    let prog = vec![Beq("nowhere")];
    let result = assemble(&prog);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("undefined label"));
}

#[test]
fn duplicate_label_error() {
    let prog = vec![Label("x"), Rtl, Label("x")];
    let result = assemble(&prog);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("duplicate label"));
}

#[test]
fn jsl_encoding() {
    let prog = vec![Jsl(0x1C_DA50)];
    let bytes = assemble(&prog).unwrap();
    assert_eq!(bytes, vec![0x22, 0x50, 0xDA, 0x1C]);
}

#[test]
fn jml_encoding() {
    let prog = vec![Jml(0x00_8088)];
    let bytes = assemble(&prog).unwrap();
    assert_eq!(bytes, vec![0x5C, 0x88, 0x80, 0x00]);
}

#[test]
fn bne_forward() {
    let prog = vec![Bne("skip"), LdaImm8(0x00), Label("skip"), Rtl];
    let bytes = assemble(&prog).unwrap();
    assert_eq!(bytes, vec![0xD0, 0x02, 0xA9, 0x00, 0x6B]);
}

#[test]
fn single_byte_instructions() {
    let prog = vec![Php, Sei, Nop, Plp, Pha, Pla, Cli, Rtl];
    let bytes = assemble(&prog).unwrap();
    assert_eq!(bytes, vec![0x08, 0x78, 0xEA, 0x28, 0x48, 0x68, 0x58, 0x6B]);
}

#[test]
fn lda_abs_encoding() {
    let prog = vec![LdaAbs(0x0C5B)];
    let bytes = assemble(&prog).unwrap();
    assert_eq!(bytes, vec![0xAD, 0x5B, 0x0C]);
}

#[test]
fn raw_bytes() {
    let prog = vec![RawBytes(vec![0x4C, 0x00, 0x80])]; // JMP $8000
    let bytes = assemble(&prog).unwrap();
    assert_eq!(bytes, vec![0x4C, 0x00, 0x80]);
}

#[test]
fn bmi_encoding() {
    let prog = vec![Bmi("target"), Rtl, Label("target"), Rtl];
    let bytes = assemble(&prog).unwrap();
    assert_eq!(bytes, vec![0x30, 0x01, 0x6B, 0x6B]);
}

#[test]
fn cmp_imm8_encoding() {
    let prog = vec![CmpImm8(0x29)];
    let bytes = assemble(&prog).unwrap();
    assert_eq!(bytes, vec![0xC9, 0x29]);
}

#[test]
fn new_single_byte_instructions() {
    let prog = vec![Phb, Plb, Inx, Iny, Tay, Tya];
    let bytes = assemble(&prog).unwrap();
    assert_eq!(bytes, vec![0x8B, 0xAB, 0xE8, 0xC8, 0xA8, 0x98]);
}

#[test]
fn dp_instructions() {
    let prog = vec![StaDp(0x0C), IncDp(0x0C), DecDp(0x0E)];
    let bytes = assemble(&prog).unwrap();
    assert_eq!(bytes, vec![0x85, 0x0C, 0xE6, 0x0C, 0xC6, 0x0E]);
}

#[test]
fn and_imm16_encoding() {
    let prog = vec![AndImm16(0x7FFF)];
    let bytes = assemble(&prog).unwrap();
    assert_eq!(bytes, vec![0x29, 0xFF, 0x7F]);
}

#[test]
fn bpl_bra_encoding() {
    let prog = vec![Bpl("skip"), LdaImm8(0x00), Label("skip"), Bra("skip")];
    let bytes = assemble(&prog).unwrap();
    // BPL +2, LDA #$00, BRA -2 (back to label)
    assert_eq!(bytes, vec![0x10, 0x02, 0xA9, 0x00, 0x80, 0xFE]);
}

#[test]
fn stack_and_single_byte_extended() {
    assert_eq!(
        assemble(&[Phx, Plx, DecA, AslA, Clc, Sec]).unwrap(),
        vec![0xDA, 0xFA, 0x3A, 0x0A, 0x18, 0x38]
    );
}

#[test]
fn adc_sbc_eor_encoding() {
    assert_eq!(
        assemble(&[
            AdcImm8(0x0A),
            AdcImm16(0x6400),
            SbcImm8(0x14),
            SbcImm16(0x1234),
            EorImm8(0xFF)
        ])
        .unwrap(),
        vec![0x69, 0x0A, 0x69, 0x00, 0x64, 0xE9, 0x14, 0xE9, 0x34, 0x12, 0x49, 0xFF]
    );
}

#[test]
fn xba_inca_jmp_lda_long() {
    use Inst::*;
    assert_eq!(
        assemble(&[Xba, IncA, JmpAbs(0xC747), LdaLong(0x7E17B6)]).unwrap(),
        vec![0xEB, 0x1A, 0x4C, 0x47, 0xC7, 0xAF, 0xB6, 0x17, 0x7E]
    );
}

#[test]
fn indexed_addressing_modes() {
    assert_eq!(
        assemble(&[
            LdaAbsX(0x0007),
            StaAbsY(0x000A),
            LdaAbsY(0x8000),
            StaLongX(0x7F0000)
        ])
        .unwrap(),
        vec![0xBD, 0x07, 0x00, 0x99, 0x0A, 0x00, 0xB9, 0x00, 0x80, 0x9F, 0x00, 0x00, 0x7F]
    );
}

#[test]
fn ldx_imm16_encoding() {
    assert_eq!(
        assemble(&[LdxImm16(0xDA80)]).unwrap(),
        vec![0xA2, 0x80, 0xDA]
    );
}

#[test]
fn ldy_imm16_encoding() {
    assert_eq!(
        assemble(&[LdyImm16(0xD000)]).unwrap(),
        vec![0xA0, 0x00, 0xD0]
    );
}

#[test]
fn mvn_encoding() {
    // MVN dst=$7F, src=$1C → 54 7F 1C
    assert_eq!(
        assemble(&[Mvn(0x7F, 0x1C)]).unwrap(),
        vec![0x54, 0x7F, 0x1C]
    );
}

#[test]
fn test_new_instructions_for_battle_width() {
    use super::*;
    // PHY = $5A
    let code = assemble(&[Inst::Phy]).unwrap();
    assert_eq!(code, vec![0x5A]);
    // PLY = $7A
    let code = assemble(&[Inst::Ply]).unwrap();
    assert_eq!(code, vec![0x7A]);
    // LDA [dp],Y = $B7 dp
    let code = assemble(&[Inst::LdaDpIndirectLongY(0x0B)]).unwrap();
    assert_eq!(code, vec![0xB7, 0x0B]);
    // STZ dp = $64 dp
    let code = assemble(&[Inst::StzDp(0x0E)]).unwrap();
    assert_eq!(code, vec![0x64, 0x0E]);
    // CMP dp = $C5 dp
    let code = assemble(&[Inst::CmpDp(0x0E)]).unwrap();
    assert_eq!(code, vec![0xC5, 0x0E]);
}
