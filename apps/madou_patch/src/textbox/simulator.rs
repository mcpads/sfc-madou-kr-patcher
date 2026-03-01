//! Text box rendering simulator — checks all strings for overflow.

use crate::encoding::codec;
use crate::text::bank::DecodedString;
use crate::textbox::layout;

/// Result of verifying a single string.
#[derive(Debug)]
pub struct VerifyResult {
    pub bank: u8,
    pub snes_addr: u16,
    #[allow(dead_code)]
    pub page_count: usize,
    pub overflow: bool,
    pub overflow_chars: usize,
    pub max_line_width: usize,
    pub max_lines_per_page: usize,
}

/// Verify a decoded string for text box overflow.
pub fn verify_string(ds: &DecodedString, box_lines: usize) -> VerifyResult {
    let tokens = codec::decode_jp(&ds.raw);
    let render = layout::render_pages_with_limit(&tokens, box_lines);

    let max_line_width = render
        .pages
        .iter()
        .flat_map(|p| p.lines.iter())
        .map(|l| l.width)
        .max()
        .unwrap_or(0);

    let max_lines = render
        .pages
        .iter()
        .map(|p| p.lines.len())
        .max()
        .unwrap_or(0);

    VerifyResult {
        bank: ds.bank,
        snes_addr: ds.snes_addr,
        page_count: render.pages.len(),
        overflow: render.overflow,
        overflow_chars: render.overflow_chars,
        max_line_width,
        max_lines_per_page: max_lines,
    }
}

/// Verify all strings and return overflow report.
pub fn verify_all(strings: &[DecodedString], box_lines: usize) -> Vec<VerifyResult> {
    strings
        .iter()
        .map(|ds| verify_string(ds, box_lines))
        .collect()
}

/// Print verification report.
#[allow(dead_code)]
pub fn print_report(results: &[VerifyResult]) {
    let total = results.len();
    let overflows: Vec<&VerifyResult> = results.iter().filter(|r| r.overflow).collect();

    println!("=== Text Box Verification Report ===");
    println!("Total strings: {}", total);
    println!("Overflow strings: {}", overflows.len());

    if !overflows.is_empty() {
        println!("\nOverflow details:");
        for r in &overflows {
            println!(
                "  ${:02X}:{:04X} — {} pages, max width: {}, max lines: {}, overflow chars: {}",
                r.bank,
                r.snes_addr,
                r.page_count,
                r.max_line_width,
                r.max_lines_per_page,
                r.overflow_chars
            );
        }
    }
}
