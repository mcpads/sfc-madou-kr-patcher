use super::super::bus::MemoryBus;
use super::super::cpu::CpuState;
use super::*;

fn make_rom(data: &[u8], base_pc: usize) -> Vec<u8> {
    let mut rom = vec![0u8; 0x200000]; // 2MB
    for (i, &b) in data.iter().enumerate() {
        if base_pc + i < rom.len() {
            rom[base_pc + i] = b;
        }
    }
    rom
}

#[test]
fn test_sei_clc_xce() {
    // SEI; CLC; XCE — typical boot sequence
    let pc_offset = crate::rom::lorom_to_pc(0x00, 0x8000);
    let rom = make_rom(&[0x78, 0x18, 0xFB], pc_offset);
    let mut bus = MemoryBus::new(rom);
    let mut cpu = CpuState::reset();
    cpu.pb = 0x00;
    cpu.pc = 0x8000;
    let mut stack = Vec::new();

    // SEI
    bus.current_pb = cpu.pb;
    bus.current_pc = cpu.pc;
    execute_one(&mut cpu, &mut bus, &mut stack).unwrap();
    assert!(cpu.flag_i());
    assert_eq!(cpu.pc, 0x8001);

    // CLC
    bus.current_pb = cpu.pb;
    bus.current_pc = cpu.pc;
    execute_one(&mut cpu, &mut bus, &mut stack).unwrap();
    assert!(!cpu.flag_c());
    assert_eq!(cpu.pc, 0x8002);

    // XCE — swap carry(0) and emulation(1) → native mode
    bus.current_pb = cpu.pb;
    bus.current_pc = cpu.pc;
    execute_one(&mut cpu, &mut bus, &mut stack).unwrap();
    assert!(!cpu.emulation);
    assert!(cpu.flag_c()); // old emulation flag
    assert_eq!(cpu.pc, 0x8003);
}

#[test]
fn test_rep_sep() {
    let pc_offset = crate::rom::lorom_to_pc(0x00, 0x8000);
    // REP #$30; SEP #$20
    let rom = make_rom(&[0xC2, 0x30, 0xE2, 0x20], pc_offset);
    let mut bus = MemoryBus::new(rom);
    let mut cpu = CpuState::reset();
    cpu.pb = 0x00;
    cpu.pc = 0x8000;
    cpu.emulation = false; // native mode
    cpu.p = 0x34; // M=1, X=1, I=1
    let mut stack = Vec::new();

    // REP #$30 — clear M and X
    bus.current_pb = cpu.pb;
    bus.current_pc = cpu.pc;
    execute_one(&mut cpu, &mut bus, &mut stack).unwrap();
    assert!(!cpu.flag_m());
    assert!(!cpu.flag_x());

    // SEP #$20 — set M
    bus.current_pb = cpu.pb;
    bus.current_pc = cpu.pc;
    execute_one(&mut cpu, &mut bus, &mut stack).unwrap();
    assert!(cpu.flag_m());
    assert!(!cpu.flag_x());
}

#[test]
fn test_lda_sta_abs() {
    let pc_offset = crate::rom::lorom_to_pc(0x00, 0x8000);
    // SEP #$20; LDA #$42; STA $0010
    let rom = make_rom(&[0xE2, 0x20, 0xA9, 0x42, 0x8D, 0x10, 0x00], pc_offset);
    let mut bus = MemoryBus::new(rom);
    let mut cpu = CpuState::reset();
    cpu.pb = 0x00;
    cpu.pc = 0x8000;
    cpu.emulation = false;
    cpu.p = 0x04; // just I set
    let mut stack = Vec::new();

    // SEP #$20
    bus.current_pb = cpu.pb;
    bus.current_pc = cpu.pc;
    execute_one(&mut cpu, &mut bus, &mut stack).unwrap();

    // LDA #$42
    bus.current_pb = cpu.pb;
    bus.current_pc = cpu.pc;
    execute_one(&mut cpu, &mut bus, &mut stack).unwrap();
    assert_eq!(cpu.a8(), 0x42);

    // STA $0010
    bus.current_pb = cpu.pb;
    bus.current_pc = cpu.pc;
    execute_one(&mut cpu, &mut bus, &mut stack).unwrap();
    assert_eq!(bus.read(0, 0x0010), 0x42);
}

#[test]
fn test_jsr_rts() {
    let pc_offset = crate::rom::lorom_to_pc(0x00, 0x8000);
    // $8000: JSR $8010
    // $8003: NOP (return here)
    // ...
    // $8010: NOP; RTS
    let mut code = vec![0u8; 0x20];
    code[0] = 0x20; // JSR
    code[1] = 0x10;
    code[2] = 0x80; // $8010
    code[3] = 0xEA; // NOP (at $8003)
    code[0x10] = 0xEA; // NOP (at $8010)
    code[0x11] = 0x60; // RTS
    let rom = make_rom(&code, pc_offset);
    let mut bus = MemoryBus::new(rom);
    let mut cpu = CpuState::reset();
    cpu.pb = 0x00;
    cpu.pc = 0x8000;
    cpu.emulation = false;
    cpu.p = 0x30;
    cpu.sp = 0x01FF;
    let mut stack = Vec::new();

    // JSR $8010
    bus.current_pb = cpu.pb;
    bus.current_pc = cpu.pc;
    execute_one(&mut cpu, &mut bus, &mut stack).unwrap();
    assert_eq!(cpu.pc, 0x8010);
    assert_eq!(stack.len(), 1);

    // NOP at $8010
    bus.current_pb = cpu.pb;
    bus.current_pc = cpu.pc;
    execute_one(&mut cpu, &mut bus, &mut stack).unwrap();
    assert_eq!(cpu.pc, 0x8011);

    // RTS
    bus.current_pb = cpu.pb;
    bus.current_pc = cpu.pc;
    execute_one(&mut cpu, &mut bus, &mut stack).unwrap();
    assert_eq!(cpu.pc, 0x8003);
    assert_eq!(stack.len(), 0);
}

#[test]
fn test_branch_beq() {
    let pc_offset = crate::rom::lorom_to_pc(0x00, 0x8000);
    // SEP #$20; LDA #$00; BEQ +3; NOP; NOP; NOP (target)
    let rom = make_rom(
        &[0xE2, 0x20, 0xA9, 0x00, 0xF0, 0x02, 0xEA, 0xEA, 0xEA],
        pc_offset,
    );
    let mut bus = MemoryBus::new(rom);
    let mut cpu = CpuState::reset();
    cpu.pb = 0x00;
    cpu.pc = 0x8000;
    cpu.emulation = false;
    cpu.p = 0x04;
    let mut stack = Vec::new();

    // SEP #$20
    bus.current_pb = cpu.pb;
    bus.current_pc = cpu.pc;
    execute_one(&mut cpu, &mut bus, &mut stack).unwrap();
    // LDA #$00
    bus.current_pb = cpu.pb;
    bus.current_pc = cpu.pc;
    execute_one(&mut cpu, &mut bus, &mut stack).unwrap();
    assert!(cpu.flag_z());
    // BEQ +2 → skip 2 NOPs to $8008
    bus.current_pb = cpu.pb;
    bus.current_pc = cpu.pc;
    execute_one(&mut cpu, &mut bus, &mut stack).unwrap();
    assert_eq!(cpu.pc, 0x8008);
}
