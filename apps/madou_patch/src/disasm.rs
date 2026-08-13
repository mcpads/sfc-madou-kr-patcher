//! 65816 linear disassembler — decodes ROM bytes into assembly mnemonics.
//!
//! Uses the OPCODE_TABLE from trace/decode.rs and LoROM conversion from rom.rs.
//! Tracks M/X flags via REP/SEP to determine immediate operand sizes.

use crate::rom::{self, SnesAddr};
use crate::trace::decode::{operand_size, AddrMode, OpcodeInfo, Operation, OPCODE_TABLE};
use std::fmt::Write;

/// Disassemble `length` bytes of ROM starting at `start` (SNES address).
/// Returns a Vec of formatted disassembly lines.
pub fn disassemble(rom: &[u8], start: SnesAddr, length: usize) -> Result<Vec<String>, String> {
    let start_pc = start.to_pc();
    if start_pc >= rom.len() {
        return Err(format!(
            "Start address {} (PC ${:06X}) is beyond ROM size (${:06X})",
            start,
            start_pc,
            rom.len()
        ));
    }

    let end_pc = (start_pc + length).min(rom.len());
    let mut pc = start_pc;

    // Default: 8-bit A, 8-bit X/Y (common after reset/SEP #$30)
    let mut flag_m = true;
    let mut flag_x = true;

    let mut lines = Vec::new();

    while pc < end_pc {
        let snes = rom::pc_to_lorom(pc);
        let opcode = rom[pc];
        let info = &OPCODE_TABLE[opcode as usize];

        let op_size = operand_size(info.op, info.mode, flag_m, flag_x);
        let instr_size = 1 + op_size;

        // Check we have enough bytes
        if pc + instr_size > rom.len() {
            let mut line = format!("${:02X}:{:04X}  {:02X}", snes.bank, snes.addr, opcode);
            line.push_str("              ; truncated");
            lines.push(line);
            break;
        }

        let operand_bytes = &rom[pc + 1..pc + instr_size];

        // Format raw hex bytes (opcode + operands)
        let hex = format_hex_bytes(opcode, operand_bytes);

        // Format operand string
        let operand_str = format_operand(info, operand_bytes, snes, flag_m, flag_x);

        let mnemonic = format!("{:?}", info.op);
        let line = format!(
            "${:02X}:{:04X}  {:<12}{}{}",
            snes.bank,
            snes.addr,
            hex,
            mnemonic,
            if operand_str.is_empty() {
                String::new()
            } else {
                format!(" {}", operand_str)
            }
        );
        lines.push(line);

        // Track M/X flag changes via REP/SEP
        update_flags(info.op, operand_bytes, &mut flag_m, &mut flag_x);

        pc += instr_size;
    }

    Ok(lines)
}

fn format_hex_bytes(opcode: u8, operand_bytes: &[u8]) -> String {
    let mut hex = format!("{:02X}", opcode);
    for b in operand_bytes {
        write!(hex, " {:02X}", b).unwrap();
    }
    hex
}

fn format_operand(
    info: &OpcodeInfo,
    operand: &[u8],
    pc_snes: SnesAddr,
    flag_m: bool,
    flag_x: bool,
) -> String {
    let _ = (flag_m, flag_x); // used implicitly via operand length
    match info.mode {
        AddrMode::Implied => String::new(),
        AddrMode::Accumulator => "A".to_string(),
        AddrMode::Immediate | AddrMode::ImmediateByte => {
            if operand.len() == 2 {
                format!("#${:02X}{:02X}", operand[1], operand[0])
            } else {
                format!("#${:02X}", operand[0])
            }
        }
        AddrMode::DirectPage => format!("${:02X}", operand[0]),
        AddrMode::DpX => format!("${:02X},X", operand[0]),
        AddrMode::DpY => format!("${:02X},Y", operand[0]),
        AddrMode::DpIndirect => format!("(${:02X})", operand[0]),
        AddrMode::DpIndirectLong => format!("[${:02X}]", operand[0]),
        AddrMode::DpIndirectX => format!("(${:02X},X)", operand[0]),
        AddrMode::DpIndirectY => format!("(${:02X}),Y", operand[0]),
        AddrMode::DpIndirectLongY => format!("[${:02X}],Y", operand[0]),
        AddrMode::Absolute => {
            let addr = u16::from_le_bytes([operand[0], operand[1]]);
            format!("${:04X}", addr)
        }
        AddrMode::AbsX => {
            let addr = u16::from_le_bytes([operand[0], operand[1]]);
            format!("${:04X},X", addr)
        }
        AddrMode::AbsY => {
            let addr = u16::from_le_bytes([operand[0], operand[1]]);
            format!("${:04X},Y", addr)
        }
        AddrMode::AbsLong => {
            format!("${:02X}{:02X}{:02X}", operand[2], operand[1], operand[0])
        }
        AddrMode::AbsLongX => {
            format!("${:02X}{:02X}{:02X},X", operand[2], operand[1], operand[0])
        }
        AddrMode::AbsIndirect => {
            let addr = u16::from_le_bytes([operand[0], operand[1]]);
            format!("(${:04X})", addr)
        }
        AddrMode::AbsIndirectX => {
            let addr = u16::from_le_bytes([operand[0], operand[1]]);
            format!("(${:04X},X)", addr)
        }
        AddrMode::AbsIndirectLong => {
            // decode.rs returns 3 bytes for AbsIndirectLong; handle both 2/3-byte cases
            if operand.len() == 3 {
                let long_addr =
                    (operand[2] as u32) << 16 | (operand[1] as u32) << 8 | operand[0] as u32;
                format!("[${:06X}]", long_addr)
            } else {
                let addr = u16::from_le_bytes([operand[0], operand[1]]);
                format!("[${:04X}]", addr)
            }
        }
        AddrMode::Relative8 => {
            let offset = operand[0] as i8;
            // Branch target = PC after instruction + signed offset
            let instr_size = 2u16; // opcode + 1 byte operand
            let target = pc_snes
                .addr
                .wrapping_add(instr_size)
                .wrapping_add(offset as u16);
            format!("${:04X}", target)
        }
        AddrMode::Relative16 => {
            let offset = i16::from_le_bytes([operand[0], operand[1]]);
            let instr_size = 3u16; // opcode + 2 byte operand
            let target = pc_snes
                .addr
                .wrapping_add(instr_size)
                .wrapping_add(offset as u16);
            format!("${:04X}", target)
        }
        AddrMode::StackRel => format!("${:02X},S", operand[0]),
        AddrMode::StackRelIndY => format!("(${:02X},S),Y", operand[0]),
        AddrMode::BlockMove => {
            // MVP/MVN: operand[0] = dst_bank, operand[1] = src_bank
            format!("${:02X},${:02X}", operand[0], operand[1])
        }
    }
}

fn update_flags(op: Operation, operand: &[u8], flag_m: &mut bool, flag_x: &mut bool) {
    let Some(&bits) = operand.first() else {
        return;
    };

    match op {
        Operation::REP => {
            if bits & 0x20 != 0 {
                *flag_m = false; // 16-bit A
            }
            if bits & 0x10 != 0 {
                *flag_x = false; // 16-bit X/Y
            }
        }
        Operation::SEP => {
            if bits & 0x20 != 0 {
                *flag_m = true; // 8-bit A
            }
            if bits & 0x10 != 0 {
                *flag_x = true; // 8-bit X/Y
            }
        }
        _ => {}
    }
}

/// CLI entry point for the disasm command.
pub fn run_disasm(
    rom_path: &std::path::Path,
    start_str: &str,
    length: usize,
) -> Result<(), String> {
    let rom = rom::load_rom(rom_path)?;
    let start =
        SnesAddr::parse(start_str).ok_or_else(|| format!("Invalid SNES address: {}", start_str))?;

    let lines = disassemble(&rom, start, length)?;
    for line in &lines {
        println!("{}", line);
    }
    Ok(())
}

#[cfg(test)]
#[path = "disasm_tests.rs"]
mod tests;
