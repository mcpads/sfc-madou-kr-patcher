// 65816 CPU state and register operations for the execution tracer.

// Status register bit masks.
const FLAG_C: u8 = 0x01;
const FLAG_Z: u8 = 0x02;
const FLAG_I: u8 = 0x04;
const FLAG_X: u8 = 0x10;
const FLAG_M: u8 = 0x20;
const FLAG_V: u8 = 0x40;
const FLAG_N: u8 = 0x80;

/// Trait for CPU bus access (implemented by MemoryBus in bus.rs).
pub trait BusAccess {
    fn read(&self, bank: u8, addr: u16) -> u8;
    fn write(&mut self, bank: u8, addr: u16, val: u8);
}

/// Full 65816 CPU register state.
#[derive(Debug, Clone)]
pub struct CpuState {
    /// Accumulator (16-bit C register).
    pub c: u16,
    /// Index register X.
    pub x: u16,
    /// Index register Y.
    pub y: u16,
    /// Stack pointer.
    pub sp: u16,
    /// Direct page register.
    pub dp: u16,
    /// Data bank register.
    pub db: u8,
    /// Program bank register.
    pub pb: u8,
    /// Program counter.
    pub pc: u16,
    /// Processor status (NVMXDIZC).
    pub p: u8,
    /// Emulation mode flag.
    pub emulation: bool,
    /// CPU stopped (STP instruction).
    pub stopped: bool,
    /// CPU waiting for interrupt (WAI instruction).
    pub waiting: bool,
}

impl CpuState {
    /// Power-on reset state. Does NOT read the reset vector;
    /// the Tracer sets pc/pb after calling this.
    pub fn reset() -> Self {
        Self {
            c: 0,
            x: 0,
            y: 0,
            sp: 0x01FD,
            dp: 0,
            db: 0,
            pb: 0,
            pc: 0,
            p: FLAG_M | FLAG_X | FLAG_I, // 0x34
            emulation: true,
            stopped: false,
            waiting: false,
        }
    }

    // -- Flag accessors --

    pub fn flag_n(&self) -> bool {
        self.p & FLAG_N != 0
    }
    pub fn flag_v(&self) -> bool {
        self.p & FLAG_V != 0
    }
    pub fn flag_m(&self) -> bool {
        self.p & FLAG_M != 0
    }
    pub fn flag_x(&self) -> bool {
        self.p & FLAG_X != 0
    }
    #[allow(dead_code)]
    pub fn flag_d(&self) -> bool {
        self.p & 0x08 != 0
    }
    #[allow(dead_code)]
    pub fn flag_i(&self) -> bool {
        self.p & FLAG_I != 0
    }
    pub fn flag_z(&self) -> bool {
        self.p & FLAG_Z != 0
    }
    pub fn flag_c(&self) -> bool {
        self.p & FLAG_C != 0
    }

    // -- Flag setters --

    pub fn set_flag(&mut self, mask: u8, val: bool) {
        if val {
            self.p |= mask;
        } else {
            self.p &= !mask;
        }
    }

    pub fn set_c(&mut self, val: bool) {
        self.set_flag(FLAG_C, val);
    }
    pub fn set_z(&mut self, val: bool) {
        self.set_flag(FLAG_Z, val);
    }
    pub fn set_n(&mut self, val: bool) {
        self.set_flag(FLAG_N, val);
    }
    pub fn set_v(&mut self, val: bool) {
        self.set_flag(FLAG_V, val);
    }

    // -- N/Z update helpers --

    pub fn update_nz8(&mut self, val: u8) {
        self.set_n(val & 0x80 != 0);
        self.set_z(val == 0);
    }

    pub fn update_nz16(&mut self, val: u16) {
        self.set_n(val & 0x8000 != 0);
        self.set_z(val == 0);
    }

    // -- Accumulator helpers --

    /// Low byte of accumulator (used in 8-bit mode).
    pub fn a8(&self) -> u8 {
        self.c as u8
    }

    /// Set low byte of accumulator, preserving high byte.
    pub fn set_a8(&mut self, val: u8) {
        self.c = (self.c & 0xFF00) | val as u16;
    }

    /// Full 16-bit accumulator.
    #[allow(dead_code)]
    pub fn a16(&self) -> u16 {
        self.c
    }

    // -- Stack operations --

    pub fn push8(&mut self, bus: &mut impl BusAccess, val: u8) {
        bus.write(0x00, self.sp, val);
        self.sp = self.sp.wrapping_sub(1);
    }

    pub fn push16(&mut self, bus: &mut impl BusAccess, val: u16) {
        self.push8(bus, (val >> 8) as u8); // high byte first
        self.push8(bus, val as u8); // then low byte
    }

    pub fn pull8(&mut self, bus: &mut impl BusAccess) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        bus.read(0x00, self.sp)
    }

    pub fn pull16(&mut self, bus: &mut impl BusAccess) -> u16 {
        let lo = self.pull8(bus) as u16;
        let hi = self.pull8(bus) as u16;
        (hi << 8) | lo
    }
}

#[cfg(test)]
#[path = "cpu_tests.rs"]
mod tests;
