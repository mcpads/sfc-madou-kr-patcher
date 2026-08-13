use super::*;

/// Simple RAM-backed bus for testing.
struct TestBus {
    ram: [u8; 0x10000],
}

impl TestBus {
    fn new() -> Self {
        Self { ram: [0; 0x10000] }
    }
}

impl BusAccess for TestBus {
    fn read(&self, _bank: u8, addr: u16) -> u8 {
        self.ram[addr as usize]
    }
    fn write(&mut self, _bank: u8, addr: u16, val: u8) {
        self.ram[addr as usize] = val;
    }
}

#[test]
fn reset_state() {
    let cpu = CpuState::reset();
    assert_eq!(cpu.c, 0);
    assert_eq!(cpu.x, 0);
    assert_eq!(cpu.y, 0);
    assert_eq!(cpu.sp, 0x01FD);
    assert_eq!(cpu.dp, 0);
    assert_eq!(cpu.db, 0);
    assert_eq!(cpu.pb, 0);
    assert_eq!(cpu.pc, 0);
    assert_eq!(cpu.p, 0x34);
    assert!(cpu.emulation);
    assert!(!cpu.stopped);
}

#[test]
fn flag_accessors() {
    let mut cpu = CpuState::reset();
    // After reset: M=1, X=1, I=1
    assert!(cpu.flag_m());
    assert!(cpu.flag_x());
    assert!(cpu.flag_i());
    assert!(!cpu.flag_n());
    assert!(!cpu.flag_v());
    assert!(!cpu.flag_d());
    assert!(!cpu.flag_z());
    assert!(!cpu.flag_c());

    cpu.set_n(true);
    assert!(cpu.flag_n());
    cpu.set_n(false);
    assert!(!cpu.flag_n());

    cpu.set_c(true);
    assert!(cpu.flag_c());
    cpu.set_v(true);
    assert!(cpu.flag_v());
    cpu.set_z(true);
    assert!(cpu.flag_z());
}

#[test]
fn update_nz8() {
    let mut cpu = CpuState::reset();
    cpu.update_nz8(0);
    assert!(cpu.flag_z());
    assert!(!cpu.flag_n());

    cpu.update_nz8(0x80);
    assert!(!cpu.flag_z());
    assert!(cpu.flag_n());

    cpu.update_nz8(0x42);
    assert!(!cpu.flag_z());
    assert!(!cpu.flag_n());
}

#[test]
fn update_nz16() {
    let mut cpu = CpuState::reset();
    cpu.update_nz16(0);
    assert!(cpu.flag_z());
    assert!(!cpu.flag_n());

    cpu.update_nz16(0x8000);
    assert!(!cpu.flag_z());
    assert!(cpu.flag_n());

    cpu.update_nz16(0x00FF);
    assert!(!cpu.flag_z());
    assert!(!cpu.flag_n());
}

#[test]
fn accumulator_helpers() {
    let mut cpu = CpuState::reset();
    cpu.c = 0xABCD;
    assert_eq!(cpu.a8(), 0xCD);
    assert_eq!(cpu.a16(), 0xABCD);

    cpu.set_a8(0x42);
    assert_eq!(cpu.c, 0xAB42);
    assert_eq!(cpu.a8(), 0x42);
    assert_eq!(cpu.a16(), 0xAB42);
}

#[test]
fn stack_push_pull_8() {
    let mut cpu = CpuState::reset();
    let mut bus = TestBus::new();
    let sp_before = cpu.sp;

    cpu.push8(&mut bus, 0xAA);
    assert_eq!(cpu.sp, sp_before.wrapping_sub(1));
    assert_eq!(bus.ram[sp_before as usize], 0xAA);

    let val = cpu.pull8(&mut bus);
    assert_eq!(val, 0xAA);
    assert_eq!(cpu.sp, sp_before);
}

#[test]
fn stack_push_pull_16() {
    let mut cpu = CpuState::reset();
    let mut bus = TestBus::new();
    let sp_before = cpu.sp;

    cpu.push16(&mut bus, 0x1234);
    assert_eq!(cpu.sp, sp_before.wrapping_sub(2));

    // High byte at higher address, low byte at lower address
    assert_eq!(bus.ram[sp_before as usize], 0x12);
    assert_eq!(bus.ram[sp_before.wrapping_sub(1) as usize], 0x34);

    let val = cpu.pull16(&mut bus);
    assert_eq!(val, 0x1234);
    assert_eq!(cpu.sp, sp_before);
}

#[test]
fn stack_roundtrip_mixed() {
    let mut cpu = CpuState::reset();
    let mut bus = TestBus::new();

    cpu.push16(&mut bus, 0xCAFE);
    cpu.push8(&mut bus, 0xBB);

    let b = cpu.pull8(&mut bus);
    let w = cpu.pull16(&mut bus);
    assert_eq!(b, 0xBB);
    assert_eq!(w, 0xCAFE);
}
