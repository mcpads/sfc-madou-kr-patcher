//! Patched ROM verification — checks all banks for text box overflow.

use crate::text::{bank, control};
use crate::textbox::simulator;

/// Run full verification on a ROM.
pub fn verify_rom(rom: &[u8]) {
    println!("ROM size: {} bytes", rom.len());
    println!();

    let mut total_strings = 0;
    let mut total_overflow = 0;

    for config in control::KNOWN_BANKS {
        println!(
            "--- Bank ${:02X} [{}]: {} ---",
            config.bank, config.label, config.description
        );

        let strings = bank::extract_bank(rom, config);
        let results = simulator::verify_all(&strings, config.box_lines);

        let overflow_count = results.iter().filter(|r| r.overflow).count();
        total_strings += strings.len();
        total_overflow += overflow_count;

        println!(
            "  Strings: {}, Overflow: {} (box: {}w × {}h)",
            strings.len(),
            overflow_count,
            crate::textbox::layout::BOX_WIDTH_TILES,
            config.box_lines
        );

        for r in results.iter().filter(|r| r.overflow) {
            println!(
                "    ${:02X}:{:04X} — width:{}, lines:{}, overflow:{}",
                r.bank, r.snes_addr, r.max_line_width, r.max_lines_per_page, r.overflow_chars
            );
        }
        println!();
    }

    println!("=== Summary ===");
    println!("Total strings: {}", total_strings);
    println!("Total overflow: {}", total_overflow);
}
