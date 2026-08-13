use super::*;
use crate::text::control::LineWrapMode;

fn decoded_string(raw: Vec<u8>) -> DecodedString {
    DecodedString {
        bank: 0x01,
        snes_addr: 0xB640,
        raw,
        text: String::new(),
        unknowns: 0,
        char_count: 0,
    }
}

#[test]
fn fixed_rows_reject_eleventh_cell_in_second_row() {
    let mut raw = vec![0x2E, 0xF9];
    raw.extend(std::iter::repeat_n(0x2E, 11));
    raw.push(0xFF);

    let result = verify_string(
        &decoded_string(raw),
        TextBoxProfile {
            max_width_tiles: 20,
            max_lines: 2,
            wrap_mode: LineWrapMode::FixedRows,
        },
    );

    assert!(result.overflow);
    assert_eq!(result.max_line_width, 22);
}

#[test]
fn fixed_rows_accept_ten_cells_in_each_row() {
    let mut raw = vec![0x2E; 10];
    raw.push(0xF9);
    raw.extend(std::iter::repeat_n(0x2E, 10));
    raw.push(0xFF);

    let result = verify_string(
        &decoded_string(raw),
        TextBoxProfile {
            max_width_tiles: 20,
            max_lines: 2,
            wrap_mode: LineWrapMode::FixedRows,
        },
    );

    assert!(!result.overflow);
    assert_eq!(result.max_lines_per_page, 2);
    assert_eq!(result.max_line_width, 20);
}

#[test]
fn diary_fixed_row_accepts_fifteen_cells() {
    let mut raw = vec![0x2E; 15];
    raw.push(0xFF);

    let result = verify_string(
        &decoded_string(raw),
        TextBoxProfile {
            max_width_tiles: 30,
            max_lines: 5,
            wrap_mode: LineWrapMode::FixedRows,
        },
    );

    assert!(!result.overflow);
    assert_eq!(result.max_line_width, 30);
}

#[test]
fn diary_fixed_row_rejects_sixteenth_cell() {
    let mut raw = vec![0x2E; 16];
    raw.push(0xFF);

    let result = verify_string(
        &decoded_string(raw),
        TextBoxProfile {
            max_width_tiles: 30,
            max_lines: 5,
            wrap_mode: LineWrapMode::FixedRows,
        },
    );

    assert!(result.overflow);
    assert_eq!(result.max_line_width, 32);
}
