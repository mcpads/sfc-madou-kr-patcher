use super::*;

#[test]
fn lookup_lz_source_error_on_short_rom() {
    let rom = vec![0u8; 4];
    let result = lookup_lz_source(&rom, 0x08, 0x40000, 10);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("out of bounds"));
}

#[test]
fn lookup_lz_source_valid() {
    let table_pc = 0x100;
    let mut rom = vec![0u8; table_pc + 20];
    // Entry 3: pointer = $9000
    rom[table_pc + 6] = 0x00;
    rom[table_pc + 7] = 0x90;
    let result = lookup_lz_source(&rom, 0x08, table_pc, 3).unwrap();
    assert_eq!(result, lorom_to_pc(0x08, 0x9000));
}
