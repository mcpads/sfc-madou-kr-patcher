/// ROM loading and LoROM address conversion for SNES (HiROM not needed).
use std::fmt;
use std::fs;
use std::path::Path;

/// SNES LoROM address (bank:addr).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SnesAddr {
    pub bank: u8,
    pub addr: u16,
}

impl SnesAddr {
    pub fn new(bank: u8, addr: u16) -> Self {
        Self { bank, addr }
    }

    /// Parse "$XX:YYYY" format.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('$');
        let (bank_s, addr_s) = s.split_once(':')?;
        let bank = u8::from_str_radix(bank_s, 16).ok()?;
        let addr = u16::from_str_radix(addr_s.trim_start_matches('$'), 16).ok()?;
        Some(Self { bank, addr })
    }

    pub fn to_pc(self) -> usize {
        lorom_to_pc(self.bank, self.addr)
    }
}

impl fmt::Display for SnesAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "${:02X}:${:04X}", self.bank, self.addr)
    }
}

impl fmt::Debug for SnesAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SnesAddr(${:02X}:${:04X})", self.bank, self.addr)
    }
}

/// Convert SNES LoROM bank:addr to PC file offset.
///
/// # Panics
/// Panics if `addr < 0x8000` (LoROM addresses must be in the upper half-bank).
pub const fn lorom_to_pc(bank: u8, addr: u16) -> usize {
    assert!(addr >= 0x8000, "LoROM addr must be >= $8000");
    ((bank as usize) & 0x7F) * 0x8000 + (addr as usize).wrapping_sub(0x8000)
}

/// Convert PC file offset back to SNES LoROM bank:addr.
#[allow(dead_code)]
pub fn pc_to_lorom(pc: usize) -> SnesAddr {
    let bank = (pc / 0x8000) as u8;
    let addr = (pc % 0x8000 + 0x8000) as u16;
    SnesAddr { bank, addr }
}

/// Load ROM file into memory.
pub fn load_rom(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("Failed to read ROM '{}': {}", path.display(), e))
}

/// Print ROM info summary.
pub fn print_info(data: &[u8]) {
    println!("ROM size: {} bytes ({} KB)", data.len(), data.len() / 1024);

    // Read internal header at $00:$FFC0 (LoROM)
    let header_pc = lorom_to_pc(0x00, 0xFFC0);
    if header_pc + 32 <= data.len() {
        let title_bytes = &data[header_pc..header_pc + 21];
        let title: String = title_bytes
            .iter()
            .map(|&b| {
                if (0x20..0x7F).contains(&b) {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        println!("Internal title: {}", title.trim());

        let map_mode = data[header_pc + 21];
        let rom_type = data[header_pc + 22];
        let rom_size = data[header_pc + 23];
        let sram_size = data[header_pc + 24];
        println!(
            "Map mode: ${:02X}, Type: ${:02X}, ROM size: {} KB, SRAM: {} KB",
            map_mode,
            rom_type,
            (1 << rom_size) as u32,
            if sram_size > 0 {
                (1 << sram_size) as u32
            } else {
                0
            }
        );

        let checksum = u16::from_le_bytes([data[header_pc + 28], data[header_pc + 29]]);
        let complement = u16::from_le_bytes([data[header_pc + 30], data[header_pc + 31]]);
        println!(
            "Checksum: ${:04X}, Complement: ${:04X}",
            checksum, complement
        );
    }
}

#[cfg(test)]
#[path = "rom_tests.rs"]
mod tests;
