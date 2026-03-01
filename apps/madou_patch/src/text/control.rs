//! Text bank configuration and control code definitions.

/// Configuration for a known text bank in the ROM.
#[derive(Debug, Clone)]
pub struct BankConfig {
    pub label: &'static str,
    pub bank: u8,
    pub start_addr: u16,
    pub end_addr: u16,
    pub description: &'static str,
    pub fc_split: bool,
    pub filter_noise: bool,
    /// Max lines per page for overflow detection (default 3, diary = 5).
    pub box_lines: usize,
}

/// All known text banks in the game.
pub const KNOWN_BANKS: &[BankConfig] = &[
    BankConfig {
        label: "01",
        bank: 0x01,
        start_addr: 0xB400,
        end_addr: 0xC588,
        description: "Menu/item/spell text",
        fc_split: false,
        filter_noise: false,
        box_lines: 3,
    },
    BankConfig {
        label: "01_monster",
        bank: 0x01,
        start_addr: 0x86DE,
        end_addr: 0x8800,
        description: "Monster strength labels",
        fc_split: false,
        filter_noise: false,
        box_lines: 3,
    },
    BankConfig {
        label: "01_save",
        bank: 0x01,
        start_addr: 0x9763,
        end_addr: 0x9780,
        description: "Save label",
        fc_split: false,
        filter_noise: false,
        box_lines: 3,
    },
    BankConfig {
        label: "01_hp",
        bank: 0x01,
        start_addr: 0xFD80,
        end_addr: 0xFFFF,
        description: "HP status + save location names",
        fc_split: false,
        filter_noise: false,
        box_lines: 3,
    },
    BankConfig {
        label: "03",
        bank: 0x03,
        start_addr: 0xD024,
        end_addr: 0xDA6F,
        description: "Diary entries (ptr table at $CFC2, 49 entries)",
        fc_split: false,
        filter_noise: false,
        box_lines: 5,
    },
    BankConfig {
        label: "08",
        bank: 0x08,
        start_addr: 0xFA50,
        end_addr: 0xFF90,
        description: "Opening/event text",
        fc_split: true,
        filter_noise: true,
        box_lines: 3,
    },
    BankConfig {
        label: "09",
        bank: 0x09,
        start_addr: 0xF470,
        end_addr: 0xFF20,
        description: "Orb/Momomo/Panoti text",
        fc_split: true,
        filter_noise: true,
        box_lines: 3,
    },
    BankConfig {
        label: "0A",
        bank: 0x0A,
        start_addr: 0xF6A0,
        end_addr: 0xFECA,
        description: "Momomo/Dragon Gate text",
        fc_split: true,
        filter_noise: true,
        box_lines: 3,
    },
    BankConfig {
        label: "1D",
        bank: 0x1D,
        start_addr: 0x8FD0,
        end_addr: 0xAB10,
        description: "Battle/monster dialogue",
        fc_split: true,
        filter_noise: false,
        box_lines: 3,
    },
    BankConfig {
        label: "2A",
        bank: 0x2A,
        start_addr: 0xBB00,
        end_addr: 0xDC40,
        description: "World map NPC/event dialogue",
        fc_split: true,
        filter_noise: false,
        box_lines: 3,
    },
    BankConfig {
        label: "2B",
        bank: 0x2B,
        start_addr: 0x8000,
        end_addr: 0xFE3F,
        description: "Main story dialogue",
        fc_split: true,
        filter_noise: false,
        box_lines: 3,
    },
    BankConfig {
        label: "2D",
        bank: 0x2D,
        start_addr: 0x8000,
        end_addr: 0xEE00,
        description: "Tutorial/extra dialogue",
        fc_split: true,
        filter_noise: true,
        box_lines: 3,
    },
];

/// Find bank config by bank number (returns first match — backward compatible).
#[allow(dead_code)]
pub fn find_bank(bank_id: u8) -> Option<&'static BankConfig> {
    KNOWN_BANKS.iter().find(|b| b.bank == bank_id)
}

/// Find bank config by label.
pub fn find_by_label(label: &str) -> Option<&'static BankConfig> {
    KNOWN_BANKS.iter().find(|b| b.label == label)
}

/// Find all bank configs matching a bank number.
pub fn find_banks_by_number(bank_id: u8) -> Vec<&'static BankConfig> {
    KNOWN_BANKS.iter().filter(|b| b.bank == bank_id).collect()
}

#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;
