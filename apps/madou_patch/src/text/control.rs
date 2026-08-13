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
    /// Width of one rendered row in 8×8 tile columns.
    pub box_width_tiles: usize,
}

/// Line handling used by a concrete text consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineWrapMode {
    /// Dialogue-style renderer: advance to the next row when the line is full.
    Automatic,
    /// Fixed-row renderer: writing past the row width corrupts an existing row.
    FixedRows,
}

/// Text-box limits for one concrete consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextBoxProfile {
    pub max_width_tiles: usize,
    pub max_lines: usize,
    pub wrap_mode: LineWrapMode,
}

/// Main-menu command descriptions. The consumer has two fixed 10-cell rows;
/// it does not allocate a third row when the second row exceeds 10 cells.
pub const MENU_COMMAND_DESCRIPTION_START: u16 = 0xB605;
pub const MENU_COMMAND_DESCRIPTION_END: u16 = 0xB6B8;

/// Diary body text. Its dedicated renderer starts at tile column 1, advances
/// two tile columns per glyph, and only changes rows on an explicit newline.
pub const DIARY_TEXT_START: u16 = 0xD024;
pub const DIARY_TEXT_END: u16 = 0xDA71;

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
        box_width_tiles: 20,
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
        box_width_tiles: 20,
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
        box_width_tiles: 20,
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
        box_width_tiles: 20,
    },
    BankConfig {
        label: "03",
        bank: 0x03,
        start_addr: 0xD024,
        // Half-open end. The final diary entry has its last character at
        // $DA6F and terminator at $DA70.
        end_addr: 0xDA71,
        description: "Diary entries (ptr table at $CFC2, 49 entries)",
        fc_split: false,
        filter_noise: false,
        box_lines: 5,
        box_width_tiles: 30,
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
        box_width_tiles: 20,
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
        box_width_tiles: 20,
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
        box_width_tiles: 20,
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
        box_width_tiles: 20,
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
        box_width_tiles: 20,
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
        box_width_tiles: 20,
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
        box_width_tiles: 20,
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

/// Return a fixed-layout profile when an address is consumed by a renderer
/// that cannot safely use dialogue-style automatic wrapping.
pub fn fixed_text_box_profile(bank: u8, snes_addr: u16) -> Option<TextBoxProfile> {
    if bank == 0x03 && (DIARY_TEXT_START..DIARY_TEXT_END).contains(&snes_addr) {
        return Some(TextBoxProfile {
            max_width_tiles: 30,
            max_lines: 5,
            wrap_mode: LineWrapMode::FixedRows,
        });
    }

    if bank == 0x01
        && (MENU_COMMAND_DESCRIPTION_START..MENU_COMMAND_DESCRIPTION_END).contains(&snes_addr)
    {
        return Some(TextBoxProfile {
            max_width_tiles: 20,
            max_lines: 2,
            wrap_mode: LineWrapMode::FixedRows,
        });
    }

    None
}

/// Resolve the layout profile used to verify one extracted string.
pub fn text_box_profile(config: &BankConfig, snes_addr: u16) -> TextBoxProfile {
    fixed_text_box_profile(config.bank, snes_addr).unwrap_or(TextBoxProfile {
        max_width_tiles: config.box_width_tiles,
        max_lines: config.box_lines,
        wrap_mode: LineWrapMode::Automatic,
    })
}

#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;
