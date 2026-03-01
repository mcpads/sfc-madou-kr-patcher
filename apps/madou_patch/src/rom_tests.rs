use super::*;

#[test]
fn lorom_roundtrip() {
    // Bank $01, addr $B400
    let pc = lorom_to_pc(0x01, 0xB400);
    assert_eq!(pc, 0x00B400); // 0x01*0x8000 + (0xB400-0x8000)
    let addr = pc_to_lorom(pc);
    assert_eq!(addr.bank, 0x01);
    assert_eq!(addr.addr, 0xB400);
}

#[test]
fn lorom_bank_2b() {
    let pc = lorom_to_pc(0x2B, 0x9000);
    assert_eq!(pc, 0x159000);
    let back = pc_to_lorom(pc);
    assert_eq!(back.bank, 0x2B);
    assert_eq!(back.addr, 0x9000);
}

#[test]
fn lorom_bank_10_e000() {
    let pc = lorom_to_pc(0x10, 0xE000);
    assert_eq!(pc, 0x086000);
}

#[test]
fn snes_addr_parse() {
    let a = SnesAddr::parse("$02:$8ADA").unwrap();
    assert_eq!(a.bank, 0x02);
    assert_eq!(a.addr, 0x8ADA);
    assert_eq!(a.to_pc(), 0x010ADA);
}
