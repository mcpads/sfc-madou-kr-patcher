use super::{run_trace, StopReason, TracerConfig};
use crate::rom::lorom_to_pc;

fn write_rom_bytes(rom: &mut [u8], bank: u8, addr: u16, bytes: &[u8]) {
    let mut pc = lorom_to_pc(bank, addr);
    for b in bytes {
        rom[pc] = *b;
        pc += 1;
    }
}

fn write_rom_word(rom: &mut [u8], bank: u8, addr: u16, val: u16) {
    let pc = lorom_to_pc(bank, addr);
    let [lo, hi] = val.to_le_bytes();
    rom[pc] = lo;
    rom[pc + 1] = hi;
}

#[test]
fn emulation_mode_nmi_uses_fffa_vector() {
    let mut rom = vec![0u8; 0x80_000];

    // Reset vector -> $00:$8000
    write_rom_word(&mut rom, 0x00, 0xFFFC, 0x8000);
    // Native NMI vector intentionally invalid to ensure emulation vector is used.
    write_rom_word(&mut rom, 0x00, 0xFFEA, 0x0000);
    // Emulation NMI vector -> $00:$8010
    write_rom_word(&mut rom, 0x00, 0xFFFA, 0x8010);

    // $00:$8000: LDA #$80 ; STA $4200 ; WAI ; STP
    write_rom_bytes(
        &mut rom,
        0x00,
        0x8000,
        &[0xA9, 0x80, 0x8D, 0x00, 0x42, 0xCB, 0xDB],
    );
    // $00:$8010: STP
    write_rom_bytes(&mut rom, 0x00, 0x8010, &[0xDB]);

    let config = TracerConfig {
        max_instructions: 30_000,
        stop_on_target: false,
        inject_nmi: true,
        max_nmi: 4,
        force_loop_break: false,
        ..Default::default()
    };

    let result = run_trace(rom, &config);
    assert!(matches!(result.stop_reason, StopReason::CpuStopped));
    assert!(result
        .trace_events
        .iter()
        .any(|ev| matches!(ev, super::bus::TraceEvent::NmiInject { handler_addr, .. } if *handler_addr == 0x8010)));
    assert_eq!(result.final_pc, (0x00, 0x8011));
}
