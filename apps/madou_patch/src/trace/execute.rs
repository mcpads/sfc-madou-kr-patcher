//! 65816 instruction execution engine.
//!
//! Executes one instruction at a time, updating CPU state and memory bus.
//! Tracks call stack for DMA provenance analysis.

use super::bus::MemoryBus;
use super::cpu::{BusAccess, CpuState};
use super::decode::{AddrMode, OpcodeInfo, Operation, OPCODE_TABLE};

/// Call stack entry for tracing subroutine calls.
#[derive(Debug, Clone)]
pub struct CallFrame {
    pub bank: u8,
    pub addr: u16,
    pub is_long: bool, // JSL vs JSR
}

/// Resolved effective address for memory operations.
#[derive(Debug, Clone, Copy)]
enum EffAddr {
    /// No address (implied, accumulator)
    None,
    /// Bank:Addr for memory access
    Full(u8, u16),
}

/// Decoded instruction context passed to category handlers.
struct InsnCtx {
    info: &'static OpcodeInfo,
    flag_m: bool,
    flag_x: bool,
    op1: u8,
    imm16: u16,
    imm24: u32,
    next_pc: u16,
    eff: EffAddr,
}

impl InsnCtx {
    fn read_val(&self, bus: &MemoryBus, is_16: bool) -> u16 {
        match self.eff {
            EffAddr::None => 0,
            EffAddr::Full(bank, addr) => {
                if is_16 {
                    bus.read16(bank, addr)
                } else {
                    bus.read(bank, addr) as u16
                }
            }
        }
    }
}

/// Execute one instruction. Returns the number of bytes consumed (for PC advance).
/// The caller should update bus.current_pb/current_pc before calling this.
pub fn execute_one(
    cpu: &mut CpuState,
    bus: &mut MemoryBus,
    call_stack: &mut Vec<CallFrame>,
) -> Result<(), String> {
    let opcode = bus.read(cpu.pb, cpu.pc);
    let info: &OpcodeInfo = &OPCODE_TABLE[opcode as usize];

    let flag_m = cpu.flag_m();
    let flag_x = cpu.flag_x();
    let op_size = super::decode::operand_size(info.op, info.mode, flag_m, flag_x);

    // Read operand bytes
    let operand_addr = cpu.pc.wrapping_add(1);
    let op1 = if op_size >= 1 {
        bus.read(cpu.pb, operand_addr)
    } else {
        0
    };
    let op2 = if op_size >= 2 {
        bus.read(cpu.pb, operand_addr.wrapping_add(1))
    } else {
        0
    };
    let op3 = if op_size >= 3 {
        bus.read(cpu.pb, operand_addr.wrapping_add(2))
    } else {
        0
    };

    let imm16 = u16::from_le_bytes([op1, op2]);
    let imm24 = (op3 as u32) << 16 | (op2 as u32) << 8 | (op1 as u32);

    // Advance PC past this instruction before execution (branches/jumps override)
    let next_pc = cpu.pc.wrapping_add(1 + op_size as u16);
    // Resolve effective address
    let eff = resolve_addr(cpu, bus, info.mode, op1, imm16, imm24);

    let ctx = InsnCtx {
        info,
        flag_m,
        flag_x,
        op1,
        imm16,
        imm24,
        next_pc,
        eff,
    };

    // Dispatch to category handlers
    match info.op {
        // Flag / mode switching
        Operation::SEI
        | Operation::CLI
        | Operation::SEC
        | Operation::CLC
        | Operation::SED
        | Operation::CLD
        | Operation::CLV
        | Operation::NOP
        | Operation::WDM
        | Operation::XCE
        | Operation::REP
        | Operation::SEP => exec_flag(cpu, &ctx),

        // Load / Store
        Operation::LDA
        | Operation::LDX
        | Operation::LDY
        | Operation::STA
        | Operation::STX
        | Operation::STY
        | Operation::STZ => exec_load_store(cpu, bus, &ctx),

        // Register transfers
        Operation::TAX
        | Operation::TAY
        | Operation::TXA
        | Operation::TYA
        | Operation::TXS
        | Operation::TSX
        | Operation::TCD
        | Operation::TDC
        | Operation::TCS
        | Operation::TSC
        | Operation::TXY
        | Operation::TYX
        | Operation::XBA => exec_transfer(cpu, &ctx),

        // Comparisons
        Operation::CMP | Operation::CPX | Operation::CPY => exec_compare(cpu, bus, &ctx),

        // Branches
        Operation::BEQ
        | Operation::BNE
        | Operation::BCS
        | Operation::BCC
        | Operation::BMI
        | Operation::BPL
        | Operation::BVS
        | Operation::BVC
        | Operation::BRA
        | Operation::BRL => exec_branch(cpu, &ctx),

        // Jumps & subroutines
        Operation::JMP | Operation::JML | Operation::JSR | Operation::JSL => {
            return exec_jump(cpu, bus, call_stack, &ctx);
        }
        Operation::RTS | Operation::RTL | Operation::RTI => exec_return(cpu, bus, call_stack, &ctx),

        // ALU
        Operation::AND
        | Operation::ORA
        | Operation::EOR
        | Operation::ADC
        | Operation::SBC
        | Operation::BIT => exec_alu(cpu, bus, &ctx),

        // Shifts & rotates
        Operation::ASL | Operation::LSR | Operation::ROL | Operation::ROR => {
            exec_shift(cpu, bus, &ctx)
        }

        // INC / DEC
        Operation::INC
        | Operation::DEC
        | Operation::INX
        | Operation::INY
        | Operation::DEX
        | Operation::DEY => exec_inc_dec(cpu, bus, &ctx),

        // Stack operations
        Operation::PHA
        | Operation::PLA
        | Operation::PHX
        | Operation::PLX
        | Operation::PHY
        | Operation::PLY
        | Operation::PHP
        | Operation::PLP
        | Operation::PHB
        | Operation::PLB
        | Operation::PHD
        | Operation::PLD
        | Operation::PHK
        | Operation::PEA
        | Operation::PEI
        | Operation::PER => exec_stack(cpu, bus, &ctx),

        // TSB / TRB
        Operation::TSB | Operation::TRB => exec_test_bits(cpu, bus, &ctx),

        // Block move
        Operation::MVP | Operation::MVN => exec_block_move(cpu, bus, &ctx),

        // BRK / COP
        Operation::BRK | Operation::COP => exec_interrupt(cpu, bus, &ctx),

        // Stop / Wait
        Operation::STP | Operation::WAI => exec_stop_wait(cpu, &ctx),
    }

    Ok(())
}

// ── Flag / Mode Switching ──────────────────────────────────────────────────

fn exec_flag(cpu: &mut CpuState, ctx: &InsnCtx) {
    match ctx.info.op {
        Operation::SEI => cpu.p |= 0x04,
        Operation::CLI => cpu.p &= !0x04,
        Operation::SEC => cpu.set_c(true),
        Operation::CLC => cpu.set_c(false),
        Operation::SED => cpu.p |= 0x08,
        Operation::CLD => cpu.p &= !0x08,
        Operation::CLV => cpu.set_v(false),
        Operation::NOP | Operation::WDM => {}
        Operation::XCE => {
            let old_carry = cpu.flag_c();
            let old_emu = cpu.emulation;
            cpu.emulation = old_carry;
            cpu.set_c(old_emu);
            if cpu.emulation {
                cpu.p |= 0x30; // M=1, X=1 in emulation
                cpu.sp = (cpu.sp & 0x00FF) | 0x0100;
            }
        }
        Operation::REP => {
            cpu.p &= !ctx.op1;
            if cpu.emulation {
                cpu.p |= 0x30; // M and X always set in emulation mode
            }
        }
        Operation::SEP => {
            cpu.p |= ctx.op1;
            if cpu.flag_x() {
                cpu.x &= 0x00FF;
                cpu.y &= 0x00FF;
            }
        }
        _ => unreachable!(),
    }
    cpu.pc = ctx.next_pc;
}

// ── Load / Store ───────────────────────────────────────────────────────────

fn exec_load_store(cpu: &mut CpuState, bus: &mut MemoryBus, ctx: &InsnCtx) {
    match ctx.info.op {
        Operation::LDA => {
            if ctx.flag_m {
                let val = match ctx.info.mode {
                    AddrMode::Immediate | AddrMode::ImmediateByte => ctx.op1,
                    _ => ctx.read_val(bus, false) as u8,
                };
                cpu.set_a8(val);
                cpu.update_nz8(val);
            } else {
                let val = match ctx.info.mode {
                    AddrMode::Immediate => ctx.imm16,
                    _ => ctx.read_val(bus, true),
                };
                cpu.c = val;
                cpu.update_nz16(val);
            }
        }
        Operation::LDX => {
            if ctx.flag_x {
                let val = match ctx.info.mode {
                    AddrMode::Immediate | AddrMode::ImmediateByte => ctx.op1,
                    _ => ctx.read_val(bus, false) as u8,
                };
                cpu.x = val as u16;
                cpu.update_nz8(val);
            } else {
                let val = match ctx.info.mode {
                    AddrMode::Immediate => ctx.imm16,
                    _ => ctx.read_val(bus, true),
                };
                cpu.x = val;
                cpu.update_nz16(val);
            }
        }
        Operation::LDY => {
            if ctx.flag_x {
                let val = match ctx.info.mode {
                    AddrMode::Immediate | AddrMode::ImmediateByte => ctx.op1,
                    _ => ctx.read_val(bus, false) as u8,
                };
                cpu.y = val as u16;
                cpu.update_nz8(val);
            } else {
                let val = match ctx.info.mode {
                    AddrMode::Immediate => ctx.imm16,
                    _ => ctx.read_val(bus, true),
                };
                cpu.y = val;
                cpu.update_nz16(val);
            }
        }
        Operation::STA => {
            if let EffAddr::Full(bank, addr) = ctx.eff {
                if ctx.flag_m {
                    bus.write(bank, addr, cpu.a8());
                } else {
                    bus.write(bank, addr, cpu.c as u8);
                    bus.write(bank, addr.wrapping_add(1), (cpu.c >> 8) as u8);
                }
            }
        }
        Operation::STX => {
            if let EffAddr::Full(bank, addr) = ctx.eff {
                if ctx.flag_x {
                    bus.write(bank, addr, cpu.x as u8);
                } else {
                    bus.write(bank, addr, cpu.x as u8);
                    bus.write(bank, addr.wrapping_add(1), (cpu.x >> 8) as u8);
                }
            }
        }
        Operation::STY => {
            if let EffAddr::Full(bank, addr) = ctx.eff {
                if ctx.flag_x {
                    bus.write(bank, addr, cpu.y as u8);
                } else {
                    bus.write(bank, addr, cpu.y as u8);
                    bus.write(bank, addr.wrapping_add(1), (cpu.y >> 8) as u8);
                }
            }
        }
        Operation::STZ => {
            if let EffAddr::Full(bank, addr) = ctx.eff {
                if ctx.flag_m {
                    bus.write(bank, addr, 0);
                } else {
                    bus.write(bank, addr, 0);
                    bus.write(bank, addr.wrapping_add(1), 0);
                }
            }
        }
        _ => unreachable!(),
    }
    cpu.pc = ctx.next_pc;
}

// ── Register Transfers ─────────────────────────────────────────────────────

fn exec_transfer(cpu: &mut CpuState, ctx: &InsnCtx) {
    match ctx.info.op {
        Operation::TAX => {
            if ctx.flag_x {
                cpu.x = cpu.a8() as u16;
                cpu.update_nz8(cpu.x as u8);
            } else {
                cpu.x = cpu.c;
                cpu.update_nz16(cpu.x);
            }
        }
        Operation::TAY => {
            if ctx.flag_x {
                cpu.y = cpu.a8() as u16;
                cpu.update_nz8(cpu.y as u8);
            } else {
                cpu.y = cpu.c;
                cpu.update_nz16(cpu.y);
            }
        }
        Operation::TXA => {
            if ctx.flag_m {
                cpu.set_a8(cpu.x as u8);
                cpu.update_nz8(cpu.a8());
            } else {
                cpu.c = cpu.x;
                cpu.update_nz16(cpu.c);
            }
        }
        Operation::TYA => {
            if ctx.flag_m {
                cpu.set_a8(cpu.y as u8);
                cpu.update_nz8(cpu.a8());
            } else {
                cpu.c = cpu.y;
                cpu.update_nz16(cpu.c);
            }
        }
        Operation::TXS => {
            cpu.sp = if cpu.emulation {
                0x0100 | (cpu.x & 0xFF)
            } else {
                cpu.x
            };
        }
        Operation::TSX => {
            if ctx.flag_x {
                cpu.x = cpu.sp & 0xFF;
                cpu.update_nz8(cpu.x as u8);
            } else {
                cpu.x = cpu.sp;
                cpu.update_nz16(cpu.x);
            }
        }
        Operation::TCD => {
            cpu.dp = cpu.c;
            cpu.update_nz16(cpu.dp);
        }
        Operation::TDC => {
            cpu.c = cpu.dp;
            cpu.update_nz16(cpu.c);
        }
        Operation::TCS => {
            cpu.sp = cpu.c;
            if cpu.emulation {
                cpu.sp = (cpu.sp & 0x00FF) | 0x0100;
            }
        }
        Operation::TSC => {
            cpu.c = cpu.sp;
            cpu.update_nz16(cpu.c);
        }
        Operation::TXY => {
            cpu.y = cpu.x;
            if ctx.flag_x {
                cpu.update_nz8(cpu.y as u8);
            } else {
                cpu.update_nz16(cpu.y);
            }
        }
        Operation::TYX => {
            cpu.x = cpu.y;
            if ctx.flag_x {
                cpu.update_nz8(cpu.x as u8);
            } else {
                cpu.update_nz16(cpu.x);
            }
        }
        Operation::XBA => {
            let lo = cpu.c & 0xFF;
            let hi = (cpu.c >> 8) & 0xFF;
            cpu.c = (lo << 8) | hi;
            // N and Z based on new low byte (A)
            cpu.update_nz8(cpu.c as u8);
        }
        _ => unreachable!(),
    }
    cpu.pc = ctx.next_pc;
}

// ── Comparisons ────────────────────────────────────────────────────────────

fn exec_compare(cpu: &mut CpuState, bus: &MemoryBus, ctx: &InsnCtx) {
    match ctx.info.op {
        Operation::CMP => {
            if ctx.flag_m {
                let a = cpu.a8() as u16;
                let val = match ctx.info.mode {
                    AddrMode::Immediate | AddrMode::ImmediateByte => ctx.op1 as u16,
                    _ => ctx.read_val(bus, false),
                };
                let result = a.wrapping_sub(val);
                cpu.set_c(a >= val);
                cpu.update_nz8(result as u8);
            } else {
                let a = cpu.c as u32;
                let val = match ctx.info.mode {
                    AddrMode::Immediate => ctx.imm16 as u32,
                    _ => ctx.read_val(bus, true) as u32,
                };
                let result = a.wrapping_sub(val);
                cpu.set_c(a >= val);
                cpu.update_nz16(result as u16);
            }
        }
        Operation::CPX => {
            if ctx.flag_x {
                let x = cpu.x & 0xFF;
                let val = match ctx.info.mode {
                    AddrMode::Immediate | AddrMode::ImmediateByte => ctx.op1 as u16,
                    _ => ctx.read_val(bus, false),
                };
                let result = x.wrapping_sub(val);
                cpu.set_c(x >= val);
                cpu.update_nz8(result as u8);
            } else {
                let x = cpu.x as u32;
                let val = match ctx.info.mode {
                    AddrMode::Immediate => ctx.imm16 as u32,
                    _ => ctx.read_val(bus, true) as u32,
                };
                let result = x.wrapping_sub(val);
                cpu.set_c(x >= val);
                cpu.update_nz16(result as u16);
            }
        }
        Operation::CPY => {
            if ctx.flag_x {
                let y = cpu.y & 0xFF;
                let val = match ctx.info.mode {
                    AddrMode::Immediate | AddrMode::ImmediateByte => ctx.op1 as u16,
                    _ => ctx.read_val(bus, false),
                };
                let result = y.wrapping_sub(val);
                cpu.set_c(y >= val);
                cpu.update_nz8(result as u8);
            } else {
                let y = cpu.y as u32;
                let val = match ctx.info.mode {
                    AddrMode::Immediate => ctx.imm16 as u32,
                    _ => ctx.read_val(bus, true) as u32,
                };
                let result = y.wrapping_sub(val);
                cpu.set_c(y >= val);
                cpu.update_nz16(result as u16);
            }
        }
        _ => unreachable!(),
    }
    cpu.pc = ctx.next_pc;
}

// ── Branches ───────────────────────────────────────────────────────────────

fn exec_branch(cpu: &mut CpuState, ctx: &InsnCtx) {
    match ctx.info.op {
        Operation::BEQ => {
            cpu.pc = ctx.next_pc;
            if cpu.flag_z() {
                cpu.pc = branch_target(ctx.next_pc, ctx.op1);
            }
        }
        Operation::BNE => {
            cpu.pc = ctx.next_pc;
            if !cpu.flag_z() {
                cpu.pc = branch_target(ctx.next_pc, ctx.op1);
            }
        }
        Operation::BCS => {
            cpu.pc = ctx.next_pc;
            if cpu.flag_c() {
                cpu.pc = branch_target(ctx.next_pc, ctx.op1);
            }
        }
        Operation::BCC => {
            cpu.pc = ctx.next_pc;
            if !cpu.flag_c() {
                cpu.pc = branch_target(ctx.next_pc, ctx.op1);
            }
        }
        Operation::BMI => {
            cpu.pc = ctx.next_pc;
            if cpu.flag_n() {
                cpu.pc = branch_target(ctx.next_pc, ctx.op1);
            }
        }
        Operation::BPL => {
            cpu.pc = ctx.next_pc;
            if !cpu.flag_n() {
                cpu.pc = branch_target(ctx.next_pc, ctx.op1);
            }
        }
        Operation::BVS => {
            cpu.pc = ctx.next_pc;
            if cpu.flag_v() {
                cpu.pc = branch_target(ctx.next_pc, ctx.op1);
            }
        }
        Operation::BVC => {
            cpu.pc = ctx.next_pc;
            if !cpu.flag_v() {
                cpu.pc = branch_target(ctx.next_pc, ctx.op1);
            }
        }
        Operation::BRA => {
            cpu.pc = branch_target(ctx.next_pc, ctx.op1);
        }
        Operation::BRL => {
            let offset = ctx.imm16 as i16;
            cpu.pc = ctx.next_pc.wrapping_add(offset as u16);
        }
        _ => unreachable!(),
    }
}

// ── Jumps & Subroutines ────────────────────────────────────────────────────

fn exec_jump(
    cpu: &mut CpuState,
    bus: &mut MemoryBus,
    call_stack: &mut Vec<CallFrame>,
    ctx: &InsnCtx,
) -> Result<(), String> {
    match ctx.info.op {
        Operation::JMP => match ctx.info.mode {
            AddrMode::Absolute => {
                cpu.pc = ctx.imm16;
            }
            AddrMode::AbsIndirect => {
                let target = bus.read16(0, ctx.imm16);
                cpu.pc = target;
            }
            AddrMode::AbsIndirectX => {
                let ptr = ctx.imm16.wrapping_add(cpu.x);
                let target = bus.read16(cpu.pb, ptr);
                cpu.pc = target;
            }
            _ => {
                return Err(format!(
                    "JMP: unexpected addressing mode {:?} at ${:02X}:${:04X}",
                    ctx.info.mode, cpu.pb, cpu.pc
                ));
            }
        },
        Operation::JML => match ctx.info.mode {
            AddrMode::AbsLong => {
                cpu.pb = (ctx.imm24 >> 16) as u8;
                cpu.pc = ctx.imm24 as u16;
            }
            AddrMode::AbsIndirectLong => {
                let long_addr = bus.read24(0, ctx.imm16);
                cpu.pb = (long_addr >> 16) as u8;
                cpu.pc = long_addr as u16;
            }
            _ => {
                return Err(format!(
                    "JML: unexpected addressing mode {:?} at ${:02X}:${:04X}",
                    ctx.info.mode, cpu.pb, cpu.pc
                ));
            }
        },
        Operation::JSR => {
            let ret = ctx.next_pc.wrapping_sub(1);
            cpu.push16(bus, ret);
            call_stack.push(CallFrame {
                bank: cpu.pb,
                addr: cpu.pc,
                is_long: false,
            });
            match ctx.info.mode {
                AddrMode::Absolute => {
                    cpu.pc = ctx.imm16;
                }
                AddrMode::AbsIndirectX => {
                    let ptr = ctx.imm16.wrapping_add(cpu.x);
                    let target = bus.read16(cpu.pb, ptr);
                    cpu.pc = target;
                }
                _ => {
                    return Err(format!(
                        "JSR: unexpected addressing mode {:?} at ${:02X}:${:04X}",
                        ctx.info.mode, cpu.pb, cpu.pc
                    ));
                }
            }
        }
        Operation::JSL => {
            cpu.push8(bus, cpu.pb);
            let ret = ctx.next_pc.wrapping_sub(1);
            cpu.push16(bus, ret);
            call_stack.push(CallFrame {
                bank: cpu.pb,
                addr: cpu.pc,
                is_long: true,
            });
            cpu.pb = (ctx.imm24 >> 16) as u8;
            cpu.pc = ctx.imm24 as u16;
        }
        _ => unreachable!(),
    }
    Ok(())
}

// ── Returns ────────────────────────────────────────────────────────────────

fn exec_return(
    cpu: &mut CpuState,
    bus: &mut MemoryBus,
    call_stack: &mut Vec<CallFrame>,
    ctx: &InsnCtx,
) {
    match ctx.info.op {
        Operation::RTS => {
            let ret = cpu.pull16(bus);
            cpu.pc = ret.wrapping_add(1);
            if !call_stack.is_empty() {
                call_stack.pop();
            }
        }
        Operation::RTL => {
            let ret = cpu.pull16(bus);
            let bank = cpu.pull8(bus);
            cpu.pc = ret.wrapping_add(1);
            cpu.pb = bank;
            if !call_stack.is_empty() {
                call_stack.pop();
            }
        }
        Operation::RTI => {
            cpu.p = cpu.pull8(bus);
            cpu.pc = cpu.pull16(bus);
            if !cpu.emulation {
                cpu.pb = cpu.pull8(bus);
            }
            if !call_stack.is_empty() {
                call_stack.pop();
            }
        }
        _ => unreachable!(),
    }
}

// ── ALU (AND, ORA, EOR, ADC, SBC, BIT) ────────────────────────────────────

fn exec_alu(cpu: &mut CpuState, bus: &MemoryBus, ctx: &InsnCtx) {
    match ctx.info.op {
        Operation::AND => {
            if ctx.flag_m {
                let val = match ctx.info.mode {
                    AddrMode::Immediate | AddrMode::ImmediateByte => ctx.op1,
                    _ => ctx.read_val(bus, false) as u8,
                };
                let result = cpu.a8() & val;
                cpu.set_a8(result);
                cpu.update_nz8(result);
            } else {
                let val = match ctx.info.mode {
                    AddrMode::Immediate => ctx.imm16,
                    _ => ctx.read_val(bus, true),
                };
                cpu.c &= val;
                cpu.update_nz16(cpu.c);
            }
        }
        Operation::ORA => {
            if ctx.flag_m {
                let val = match ctx.info.mode {
                    AddrMode::Immediate | AddrMode::ImmediateByte => ctx.op1,
                    _ => ctx.read_val(bus, false) as u8,
                };
                let result = cpu.a8() | val;
                cpu.set_a8(result);
                cpu.update_nz8(result);
            } else {
                let val = match ctx.info.mode {
                    AddrMode::Immediate => ctx.imm16,
                    _ => ctx.read_val(bus, true),
                };
                cpu.c |= val;
                cpu.update_nz16(cpu.c);
            }
        }
        Operation::EOR => {
            if ctx.flag_m {
                let val = match ctx.info.mode {
                    AddrMode::Immediate | AddrMode::ImmediateByte => ctx.op1,
                    _ => ctx.read_val(bus, false) as u8,
                };
                let result = cpu.a8() ^ val;
                cpu.set_a8(result);
                cpu.update_nz8(result);
            } else {
                let val = match ctx.info.mode {
                    AddrMode::Immediate => ctx.imm16,
                    _ => ctx.read_val(bus, true),
                };
                cpu.c ^= val;
                cpu.update_nz16(cpu.c);
            }
        }
        Operation::ADC => {
            if ctx.flag_m {
                let a = cpu.a8() as u16;
                let val = match ctx.info.mode {
                    AddrMode::Immediate | AddrMode::ImmediateByte => ctx.op1 as u16,
                    _ => ctx.read_val(bus, false),
                };
                let carry = if cpu.flag_c() { 1u16 } else { 0 };
                let result = a + val + carry;
                let v = ((!(a ^ val)) & (a ^ result)) & 0x80 != 0;
                cpu.set_v(v);
                cpu.set_c(result > 0xFF);
                let r8 = result as u8;
                cpu.set_a8(r8);
                cpu.update_nz8(r8);
            } else {
                let a = cpu.c as u32;
                let val = match ctx.info.mode {
                    AddrMode::Immediate => ctx.imm16 as u32,
                    _ => ctx.read_val(bus, true) as u32,
                };
                let carry = if cpu.flag_c() { 1u32 } else { 0 };
                let result = a + val + carry;
                let v = ((!(a ^ val)) & (a ^ result)) & 0x8000 != 0;
                cpu.set_v(v);
                cpu.set_c(result > 0xFFFF);
                cpu.c = result as u16;
                cpu.update_nz16(cpu.c);
            }
        }
        Operation::SBC => {
            if ctx.flag_m {
                let a = cpu.a8() as u16;
                let val = match ctx.info.mode {
                    AddrMode::Immediate | AddrMode::ImmediateByte => ctx.op1 as u16,
                    _ => ctx.read_val(bus, false),
                };
                let borrow = if cpu.flag_c() { 0u16 } else { 1 };
                let result = a.wrapping_sub(val).wrapping_sub(borrow);
                let v = ((a ^ val) & (a ^ result)) & 0x80 != 0;
                cpu.set_v(v);
                cpu.set_c(a >= val + borrow);
                let r8 = result as u8;
                cpu.set_a8(r8);
                cpu.update_nz8(r8);
            } else {
                let a = cpu.c as u32;
                let val = match ctx.info.mode {
                    AddrMode::Immediate => ctx.imm16 as u32,
                    _ => ctx.read_val(bus, true) as u32,
                };
                let borrow = if cpu.flag_c() { 0u32 } else { 1 };
                let result = a.wrapping_sub(val).wrapping_sub(borrow);
                let v = ((a ^ val) & (a ^ result)) & 0x8000 != 0;
                cpu.set_v(v);
                cpu.set_c(a >= val + borrow);
                cpu.c = result as u16;
                cpu.update_nz16(cpu.c);
            }
        }
        Operation::BIT => {
            if ctx.info.mode == AddrMode::Immediate {
                if ctx.flag_m {
                    let result = cpu.a8() & ctx.op1;
                    cpu.set_z(result == 0);
                } else {
                    let result = cpu.c & ctx.imm16;
                    cpu.set_z(result == 0);
                }
            } else if ctx.flag_m {
                let val = ctx.read_val(bus, false) as u8;
                cpu.set_n(val & 0x80 != 0);
                cpu.set_v(val & 0x40 != 0);
                cpu.set_z(cpu.a8() & val == 0);
            } else {
                let val = ctx.read_val(bus, true);
                cpu.set_n(val & 0x8000 != 0);
                cpu.set_v(val & 0x4000 != 0);
                cpu.set_z(cpu.c & val == 0);
            }
        }
        _ => unreachable!(),
    }
    cpu.pc = ctx.next_pc;
}

// ── Shifts & Rotates ───────────────────────────────────────────────────────

fn exec_shift(cpu: &mut CpuState, bus: &mut MemoryBus, ctx: &InsnCtx) {
    match ctx.info.op {
        Operation::ASL => {
            if ctx.info.mode == AddrMode::Accumulator {
                if ctx.flag_m {
                    let val = cpu.a8();
                    cpu.set_c(val & 0x80 != 0);
                    let r = val << 1;
                    cpu.set_a8(r);
                    cpu.update_nz8(r);
                } else {
                    cpu.set_c(cpu.c & 0x8000 != 0);
                    cpu.c <<= 1;
                    cpu.update_nz16(cpu.c);
                }
            } else if let EffAddr::Full(bank, addr) = ctx.eff {
                if ctx.flag_m {
                    let val = bus.read(bank, addr);
                    cpu.set_c(val & 0x80 != 0);
                    let r = val << 1;
                    bus.write(bank, addr, r);
                    cpu.update_nz8(r);
                } else {
                    let val = bus.read16(bank, addr);
                    cpu.set_c(val & 0x8000 != 0);
                    let r = val << 1;
                    bus.write(bank, addr, r as u8);
                    bus.write(bank, addr.wrapping_add(1), (r >> 8) as u8);
                    cpu.update_nz16(r);
                }
            }
        }
        Operation::LSR => {
            if ctx.info.mode == AddrMode::Accumulator {
                if ctx.flag_m {
                    let val = cpu.a8();
                    cpu.set_c(val & 1 != 0);
                    let r = val >> 1;
                    cpu.set_a8(r);
                    cpu.update_nz8(r);
                } else {
                    cpu.set_c(cpu.c & 1 != 0);
                    cpu.c >>= 1;
                    cpu.update_nz16(cpu.c);
                }
            } else if let EffAddr::Full(bank, addr) = ctx.eff {
                if ctx.flag_m {
                    let val = bus.read(bank, addr);
                    cpu.set_c(val & 1 != 0);
                    let r = val >> 1;
                    bus.write(bank, addr, r);
                    cpu.update_nz8(r);
                } else {
                    let val = bus.read16(bank, addr);
                    cpu.set_c(val & 1 != 0);
                    let r = val >> 1;
                    bus.write(bank, addr, r as u8);
                    bus.write(bank, addr.wrapping_add(1), (r >> 8) as u8);
                    cpu.update_nz16(r);
                }
            }
        }
        Operation::ROL => {
            let old_c = cpu.flag_c();
            if ctx.info.mode == AddrMode::Accumulator {
                if ctx.flag_m {
                    let val = cpu.a8();
                    cpu.set_c(val & 0x80 != 0);
                    let r = (val << 1) | (old_c as u8);
                    cpu.set_a8(r);
                    cpu.update_nz8(r);
                } else {
                    cpu.set_c(cpu.c & 0x8000 != 0);
                    cpu.c = (cpu.c << 1) | (old_c as u16);
                    cpu.update_nz16(cpu.c);
                }
            } else if let EffAddr::Full(bank, addr) = ctx.eff {
                if ctx.flag_m {
                    let val = bus.read(bank, addr);
                    cpu.set_c(val & 0x80 != 0);
                    let r = (val << 1) | (old_c as u8);
                    bus.write(bank, addr, r);
                    cpu.update_nz8(r);
                } else {
                    let val = bus.read16(bank, addr);
                    cpu.set_c(val & 0x8000 != 0);
                    let r = (val << 1) | (old_c as u16);
                    bus.write(bank, addr, r as u8);
                    bus.write(bank, addr.wrapping_add(1), (r >> 8) as u8);
                    cpu.update_nz16(r);
                }
            }
        }
        Operation::ROR => {
            let old_c = cpu.flag_c();
            if ctx.info.mode == AddrMode::Accumulator {
                if ctx.flag_m {
                    let val = cpu.a8();
                    cpu.set_c(val & 1 != 0);
                    let r = (val >> 1) | ((old_c as u8) << 7);
                    cpu.set_a8(r);
                    cpu.update_nz8(r);
                } else {
                    cpu.set_c(cpu.c & 1 != 0);
                    cpu.c = (cpu.c >> 1) | ((old_c as u16) << 15);
                    cpu.update_nz16(cpu.c);
                }
            } else if let EffAddr::Full(bank, addr) = ctx.eff {
                if ctx.flag_m {
                    let val = bus.read(bank, addr);
                    cpu.set_c(val & 1 != 0);
                    let r = (val >> 1) | ((old_c as u8) << 7);
                    bus.write(bank, addr, r);
                    cpu.update_nz8(r);
                } else {
                    let val = bus.read16(bank, addr);
                    cpu.set_c(val & 1 != 0);
                    let r = (val >> 1) | ((old_c as u16) << 15);
                    bus.write(bank, addr, r as u8);
                    bus.write(bank, addr.wrapping_add(1), (r >> 8) as u8);
                    cpu.update_nz16(r);
                }
            }
        }
        _ => unreachable!(),
    }
    cpu.pc = ctx.next_pc;
}

// ── INC / DEC ──────────────────────────────────────────────────────────────

fn exec_inc_dec(cpu: &mut CpuState, bus: &mut MemoryBus, ctx: &InsnCtx) {
    match ctx.info.op {
        Operation::INC => {
            if ctx.info.mode == AddrMode::Accumulator {
                if ctx.flag_m {
                    let r = cpu.a8().wrapping_add(1);
                    cpu.set_a8(r);
                    cpu.update_nz8(r);
                } else {
                    cpu.c = cpu.c.wrapping_add(1);
                    cpu.update_nz16(cpu.c);
                }
            } else if let EffAddr::Full(bank, addr) = ctx.eff {
                if ctx.flag_m {
                    let val = bus.read(bank, addr).wrapping_add(1);
                    bus.write(bank, addr, val);
                    cpu.update_nz8(val);
                } else {
                    let val = bus.read16(bank, addr).wrapping_add(1);
                    bus.write(bank, addr, val as u8);
                    bus.write(bank, addr.wrapping_add(1), (val >> 8) as u8);
                    cpu.update_nz16(val);
                }
            }
        }
        Operation::DEC => {
            if ctx.info.mode == AddrMode::Accumulator {
                if ctx.flag_m {
                    let r = cpu.a8().wrapping_sub(1);
                    cpu.set_a8(r);
                    cpu.update_nz8(r);
                } else {
                    cpu.c = cpu.c.wrapping_sub(1);
                    cpu.update_nz16(cpu.c);
                }
            } else if let EffAddr::Full(bank, addr) = ctx.eff {
                if ctx.flag_m {
                    let val = bus.read(bank, addr).wrapping_sub(1);
                    bus.write(bank, addr, val);
                    cpu.update_nz8(val);
                } else {
                    let val = bus.read16(bank, addr).wrapping_sub(1);
                    bus.write(bank, addr, val as u8);
                    bus.write(bank, addr.wrapping_add(1), (val >> 8) as u8);
                    cpu.update_nz16(val);
                }
            }
        }
        Operation::INX => {
            if ctx.flag_x {
                cpu.x = (cpu.x.wrapping_add(1)) & 0xFF;
                cpu.update_nz8(cpu.x as u8);
            } else {
                cpu.x = cpu.x.wrapping_add(1);
                cpu.update_nz16(cpu.x);
            }
        }
        Operation::INY => {
            if ctx.flag_x {
                cpu.y = (cpu.y.wrapping_add(1)) & 0xFF;
                cpu.update_nz8(cpu.y as u8);
            } else {
                cpu.y = cpu.y.wrapping_add(1);
                cpu.update_nz16(cpu.y);
            }
        }
        Operation::DEX => {
            if ctx.flag_x {
                cpu.x = (cpu.x.wrapping_sub(1)) & 0xFF;
                cpu.update_nz8(cpu.x as u8);
            } else {
                cpu.x = cpu.x.wrapping_sub(1);
                cpu.update_nz16(cpu.x);
            }
        }
        Operation::DEY => {
            if ctx.flag_x {
                cpu.y = (cpu.y.wrapping_sub(1)) & 0xFF;
                cpu.update_nz8(cpu.y as u8);
            } else {
                cpu.y = cpu.y.wrapping_sub(1);
                cpu.update_nz16(cpu.y);
            }
        }
        _ => unreachable!(),
    }
    cpu.pc = ctx.next_pc;
}

// ── Stack Operations ───────────────────────────────────────────────────────

fn exec_stack(cpu: &mut CpuState, bus: &mut MemoryBus, ctx: &InsnCtx) {
    match ctx.info.op {
        Operation::PHA => {
            if ctx.flag_m {
                cpu.push8(bus, cpu.a8());
            } else {
                cpu.push16(bus, cpu.c);
            }
        }
        Operation::PLA => {
            if ctx.flag_m {
                let val = cpu.pull8(bus);
                cpu.set_a8(val);
                cpu.update_nz8(val);
            } else {
                let val = cpu.pull16(bus);
                cpu.c = val;
                cpu.update_nz16(val);
            }
        }
        Operation::PHX => {
            if ctx.flag_x {
                cpu.push8(bus, cpu.x as u8);
            } else {
                cpu.push16(bus, cpu.x);
            }
        }
        Operation::PLX => {
            if ctx.flag_x {
                let val = cpu.pull8(bus);
                cpu.x = val as u16;
                cpu.update_nz8(val);
            } else {
                let val = cpu.pull16(bus);
                cpu.x = val;
                cpu.update_nz16(val);
            }
        }
        Operation::PHY => {
            if ctx.flag_x {
                cpu.push8(bus, cpu.y as u8);
            } else {
                cpu.push16(bus, cpu.y);
            }
        }
        Operation::PLY => {
            if ctx.flag_x {
                let val = cpu.pull8(bus);
                cpu.y = val as u16;
                cpu.update_nz8(val);
            } else {
                let val = cpu.pull16(bus);
                cpu.y = val;
                cpu.update_nz16(val);
            }
        }
        Operation::PHP => {
            cpu.push8(bus, cpu.p);
        }
        Operation::PLP => {
            cpu.p = cpu.pull8(bus);
            if cpu.emulation {
                cpu.p |= 0x30;
            }
            if cpu.flag_x() {
                cpu.x &= 0xFF;
                cpu.y &= 0xFF;
            }
        }
        Operation::PHB => {
            cpu.push8(bus, cpu.db);
        }
        Operation::PLB => {
            cpu.db = cpu.pull8(bus);
            cpu.update_nz8(cpu.db);
        }
        Operation::PHD => {
            cpu.push16(bus, cpu.dp);
        }
        Operation::PLD => {
            cpu.dp = cpu.pull16(bus);
            cpu.update_nz16(cpu.dp);
        }
        Operation::PHK => {
            cpu.push8(bus, cpu.pb);
        }
        Operation::PEA => {
            cpu.push16(bus, ctx.imm16);
        }
        Operation::PEI => {
            let dp_addr = cpu.dp.wrapping_add(ctx.op1 as u16);
            let val = bus.read16(0, dp_addr);
            cpu.push16(bus, val);
        }
        Operation::PER => {
            let val = ctx.next_pc.wrapping_add(ctx.imm16);
            cpu.push16(bus, val);
        }
        _ => unreachable!(),
    }
    cpu.pc = ctx.next_pc;
}

// ── TSB / TRB ──────────────────────────────────────────────────────────────

fn exec_test_bits(cpu: &mut CpuState, bus: &mut MemoryBus, ctx: &InsnCtx) {
    match ctx.info.op {
        Operation::TSB => {
            if let EffAddr::Full(bank, addr) = ctx.eff {
                if ctx.flag_m {
                    let val = bus.read(bank, addr);
                    cpu.set_z(cpu.a8() & val == 0);
                    bus.write(bank, addr, val | cpu.a8());
                } else {
                    let val = bus.read16(bank, addr);
                    cpu.set_z(cpu.c & val == 0);
                    let r = val | cpu.c;
                    bus.write(bank, addr, r as u8);
                    bus.write(bank, addr.wrapping_add(1), (r >> 8) as u8);
                }
            }
        }
        Operation::TRB => {
            if let EffAddr::Full(bank, addr) = ctx.eff {
                if ctx.flag_m {
                    let val = bus.read(bank, addr);
                    cpu.set_z(cpu.a8() & val == 0);
                    bus.write(bank, addr, val & !cpu.a8());
                } else {
                    let val = bus.read16(bank, addr);
                    cpu.set_z(cpu.c & val == 0);
                    let r = val & !cpu.c;
                    bus.write(bank, addr, r as u8);
                    bus.write(bank, addr.wrapping_add(1), (r >> 8) as u8);
                }
            }
        }
        _ => unreachable!(),
    }
    cpu.pc = ctx.next_pc;
}

// ── Block Move ─────────────────────────────────────────────────────────────

fn exec_block_move(cpu: &mut CpuState, bus: &mut MemoryBus, ctx: &InsnCtx) {
    let op2 = (ctx.imm16 >> 8) as u8;
    match ctx.info.op {
        Operation::MVP => {
            let dst_bank = ctx.op1;
            let src_bank = op2;
            cpu.db = dst_bank;
            let src_val = bus.read(src_bank, cpu.x);
            bus.write(dst_bank, cpu.y, src_val);
            cpu.x = cpu.x.wrapping_sub(1);
            cpu.y = cpu.y.wrapping_sub(1);
            cpu.c = cpu.c.wrapping_sub(1);
            if cpu.c == 0xFFFF {
                cpu.pc = ctx.next_pc;
            }
            // else: PC stays at the MVP instruction (repeat)
        }
        Operation::MVN => {
            let dst_bank = ctx.op1;
            let src_bank = op2;
            cpu.db = dst_bank;
            let src_val = bus.read(src_bank, cpu.x);
            bus.write(dst_bank, cpu.y, src_val);
            cpu.x = cpu.x.wrapping_add(1);
            cpu.y = cpu.y.wrapping_add(1);
            cpu.c = cpu.c.wrapping_sub(1);
            if cpu.c == 0xFFFF {
                cpu.pc = ctx.next_pc;
            }
            // else: repeat
        }
        _ => unreachable!(),
    }
}

// ── BRK / COP ──────────────────────────────────────────────────────────────

fn exec_interrupt(cpu: &mut CpuState, bus: &mut MemoryBus, ctx: &InsnCtx) {
    match ctx.info.op {
        Operation::BRK => {
            if !cpu.emulation {
                cpu.push8(bus, cpu.pb);
            }
            cpu.push16(bus, ctx.next_pc);
            cpu.push8(bus, cpu.p);
            cpu.p |= 0x04; // Set I
            cpu.p &= !0x08; // Clear D
            if cpu.emulation {
                let vec = bus.read16(0, 0xFFFE);
                cpu.pc = vec;
            } else {
                cpu.pb = 0;
                let vec = bus.read16(0, 0xFFE6);
                cpu.pc = vec;
            }
        }
        Operation::COP => {
            if !cpu.emulation {
                cpu.push8(bus, cpu.pb);
            }
            cpu.push16(bus, ctx.next_pc);
            cpu.push8(bus, cpu.p);
            cpu.p |= 0x04;
            cpu.p &= !0x08;
            if cpu.emulation {
                let vec = bus.read16(0, 0xFFF4);
                cpu.pc = vec;
            } else {
                cpu.pb = 0;
                let vec = bus.read16(0, 0xFFE4);
                cpu.pc = vec;
            }
        }
        _ => unreachable!(),
    }
}

// ── Stop / Wait ────────────────────────────────────────────────────────────

fn exec_stop_wait(cpu: &mut CpuState, ctx: &InsnCtx) {
    match ctx.info.op {
        Operation::STP => {
            cpu.stopped = true;
        }
        Operation::WAI => {
            // Mark CPU as waiting for interrupt. The tracer main loop handles
            // NMI delivery or immediate pass-through.
            cpu.waiting = true;
        }
        _ => unreachable!(),
    }
    cpu.pc = ctx.next_pc;
}

// ── Address Resolution ─────────────────────────────────────────────────────

/// Compute branch target from a signed 8-bit offset.
fn branch_target(next_pc: u16, offset: u8) -> u16 {
    next_pc.wrapping_add(offset as i8 as i16 as u16)
}

/// Resolve effective address based on addressing mode.
fn resolve_addr(
    cpu: &CpuState,
    bus: &MemoryBus,
    mode: AddrMode,
    op1: u8,
    imm16: u16,
    imm24: u32,
) -> EffAddr {
    match mode {
        AddrMode::Implied
        | AddrMode::Accumulator
        | AddrMode::Relative8
        | AddrMode::Relative16
        | AddrMode::ImmediateByte
        | AddrMode::Immediate
        | AddrMode::BlockMove => EffAddr::None,

        AddrMode::DirectPage => {
            let addr = cpu.dp.wrapping_add(op1 as u16);
            EffAddr::Full(0, addr)
        }
        AddrMode::DpX => {
            let addr = cpu.dp.wrapping_add(op1 as u16).wrapping_add(cpu.x);
            EffAddr::Full(0, addr)
        }
        AddrMode::DpY => {
            let addr = cpu.dp.wrapping_add(op1 as u16).wrapping_add(cpu.y);
            EffAddr::Full(0, addr)
        }
        AddrMode::DpIndirect => {
            let dp_addr = cpu.dp.wrapping_add(op1 as u16);
            let ptr = bus.read16(0, dp_addr);
            EffAddr::Full(cpu.db, ptr)
        }
        AddrMode::DpIndirectLong => {
            let dp_addr = cpu.dp.wrapping_add(op1 as u16);
            let long = bus.read24(0, dp_addr);
            EffAddr::Full((long >> 16) as u8, long as u16)
        }
        AddrMode::DpIndirectX => {
            let dp_addr = cpu.dp.wrapping_add(op1 as u16).wrapping_add(cpu.x);
            let ptr = bus.read16(0, dp_addr);
            EffAddr::Full(cpu.db, ptr)
        }
        AddrMode::DpIndirectY => {
            let dp_addr = cpu.dp.wrapping_add(op1 as u16);
            let ptr = bus.read16(0, dp_addr);
            EffAddr::Full(cpu.db, ptr.wrapping_add(cpu.y))
        }
        AddrMode::DpIndirectLongY => {
            let dp_addr = cpu.dp.wrapping_add(op1 as u16);
            let long = bus.read24(0, dp_addr);
            let bank = (long >> 16) as u8;
            let addr = (long as u16).wrapping_add(cpu.y);
            EffAddr::Full(bank, addr)
        }
        AddrMode::Absolute => EffAddr::Full(cpu.db, imm16),
        AddrMode::AbsX => EffAddr::Full(cpu.db, imm16.wrapping_add(cpu.x)),
        AddrMode::AbsY => EffAddr::Full(cpu.db, imm16.wrapping_add(cpu.y)),
        AddrMode::AbsLong => EffAddr::Full((imm24 >> 16) as u8, imm24 as u16),
        AddrMode::AbsLongX => {
            let base = imm24 as u16;
            let bank = (imm24 >> 16) as u8;
            EffAddr::Full(bank, base.wrapping_add(cpu.x))
        }
        AddrMode::AbsIndirect => {
            // Used by JMP (abs) — resolved in JMP handler
            let ptr = bus.read16(0, imm16);
            EffAddr::Full(cpu.pb, ptr)
        }
        AddrMode::AbsIndirectX => {
            let ptr = imm16.wrapping_add(cpu.x);
            let target = bus.read16(cpu.pb, ptr);
            EffAddr::Full(cpu.pb, target)
        }
        AddrMode::AbsIndirectLong => {
            let long = bus.read24(0, imm16);
            EffAddr::Full((long >> 16) as u8, long as u16)
        }
        AddrMode::StackRel => {
            let addr = cpu.sp.wrapping_add(op1 as u16);
            EffAddr::Full(0, addr)
        }
        AddrMode::StackRelIndY => {
            let addr = cpu.sp.wrapping_add(op1 as u16);
            let ptr = bus.read16(0, addr);
            EffAddr::Full(cpu.db, ptr.wrapping_add(cpu.y))
        }
    }
}

#[cfg(test)]
#[path = "execute_tests.rs"]
mod tests;
