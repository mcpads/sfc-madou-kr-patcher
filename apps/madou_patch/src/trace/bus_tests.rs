use super::*;

fn make_bus() -> MemoryBus {
    // 512KB ROM filled with a pattern for testing
    let mut rom = vec![0u8; 0x80000];
    // Put a known byte at LoROM $00:$8000 → PC offset 0
    rom[0] = 0xAB;
    // Put bytes at LoROM $01:$8000 → PC offset 0x8000
    rom[0x8000] = 0xCD;
    // Put bytes at LoROM $01:$8002/$8003 for read16 test
    rom[0x8002] = 0x34;
    rom[0x8003] = 0x12;
    // Put bytes at LoROM $01:$8004-$8006 for read24 test
    rom[0x8004] = 0x78;
    rom[0x8005] = 0x56;
    rom[0x8006] = 0x34;
    MemoryBus::new(rom)
}

#[test]
fn rom_read_lorom() {
    let bus = make_bus();
    assert_eq!(bus.read(0x00, 0x8000), 0xAB);
    assert_eq!(bus.read(0x01, 0x8000), 0xCD);
    // Mirror: $80:$8000 maps to same as $00:$8000
    assert_eq!(bus.read(0x80, 0x8000), 0xAB);
}

#[test]
fn rom_read_out_of_bounds() {
    // Small ROM: only 256 bytes
    let bus = MemoryBus::new(vec![0x42; 256]);
    // $02:$8000 → PC offset 0x10000, well beyond 256 bytes
    assert_eq!(bus.read(0x02, 0x8000), 0);
}

#[test]
fn read16_little_endian() {
    let bus = make_bus();
    assert_eq!(bus.read16(0x01, 0x8002), 0x1234);
}

#[test]
fn read24_little_endian() {
    let bus = make_bus();
    assert_eq!(bus.read24(0x01, 0x8004), 0x345678);
}

#[test]
fn wram_write_read() {
    let mut bus = make_bus();
    // Write via bank $00 low page
    bus.write(0x00, 0x0010, 0x42);
    assert_eq!(bus.read(0x00, 0x0010), 0x42);
    // Same address via bank $80 mirror
    assert_eq!(bus.read(0x80, 0x0010), 0x42);
}

#[test]
fn wram_bank_7e_7f() {
    let mut bus = make_bus();
    bus.write(0x7E, 0x1000, 0xAA);
    assert_eq!(bus.read(0x7E, 0x1000), 0xAA);
    // Also visible via $00:$1000 mirror (low 8K)
    bus.write(0x7E, 0x0020, 0xBB);
    assert_eq!(bus.read(0x00, 0x0020), 0xBB);

    // Bank $7F: upper 64KB of WRAM
    bus.write(0x7F, 0x0000, 0xCC);
    assert_eq!(bus.read(0x7F, 0x0000), 0xCC);
    assert_eq!(bus.wram[0x10000], 0xCC);
}

#[test]
fn vram_addr_registers() {
    let mut bus = make_bus();
    bus.write(0x00, 0x2116, 0x34);
    bus.write(0x00, 0x2117, 0x12);
    assert_eq!(bus.vram_addr, 0x1234);
}

#[test]
fn vram_incr_register() {
    let mut bus = make_bus();
    bus.write(0x00, 0x2115, 0x80);
    assert_eq!(bus.vram_incr, 0x80);
}

#[test]
fn wram_addr_registers() {
    let mut bus = make_bus();
    bus.write(0x00, 0x2181, 0xAB);
    bus.write(0x00, 0x2182, 0xCD);
    bus.write(0x00, 0x2183, 0x01);
    assert_eq!(bus.wram_addr, 0x1CDAB);
}

#[test]
fn io_read_stubs() {
    let bus = make_bus();
    // $4210: NMI latch (bit7=0 when no pending NMI, chip version=2)
    assert_eq!(bus.read(0x00, 0x4210), 0x02);
    assert_eq!(bus.read(0x00, 0x4212), 0x81);
    assert_eq!(bus.read(0x00, 0x4016), 0x00);
    assert_eq!(bus.read(0x00, 0x4017), 0x00);
    assert_eq!(bus.read(0x00, 0x2140), 0xAA);
    assert_eq!(bus.read(0x00, 0x2141), 0xBB);
}

#[test]
fn hvbjoy_changes_with_timing_ticks() {
    let mut bus = make_bus();
    let initial = bus.read(0x00, 0x4212);
    let mut changed = false;
    for _ in 0..5_000 {
        bus.tick();
        if bus.read(0x00, 0x4212) != initial {
            changed = true;
            break;
        }
    }
    assert!(changed, "$4212 should vary with coarse timing");
}

#[test]
fn frame_marker_emitted_on_vblank_rising_edge() {
    let mut bus = make_bus();
    bus.take_trace_events();

    let mut marker: Option<TraceEvent> = None;
    for _ in 0..20_000 {
        bus.tick();
        let events = bus.take_trace_events();
        if let Some(ev) = events
            .into_iter()
            .find(|ev| matches!(ev, TraceEvent::FrameMarker { .. }))
        {
            marker = Some(ev);
            break;
        }
    }

    let ev = marker.expect("expected at least one FRAME_MARKER event");
    match ev {
        TraceEvent::FrameMarker {
            frame_num,
            inidisp,
            seq,
            ..
        } => {
            assert_eq!(frame_num, 1);
            assert_eq!(inidisp, bus.inidisp);
            assert!(seq > 0);
        }
        other => panic!("expected FrameMarker, got {:?}", other),
    }
}

#[test]
fn nmi_pending_generated_from_vblank_edge() {
    let mut bus = make_bus();
    // Enable NMI via NMITIMEN bit 7.
    bus.write(0x00, 0x4200, 0x80);

    let mut pending_seen = false;
    for _ in 0..20_000 {
        bus.tick();
        if bus.take_pending_nmi() {
            pending_seen = true;
            break;
        }
    }
    assert!(pending_seen, "timing model should raise pending NMI");

    // $4210 latch should still report pending until read-cleared.
    let val = bus.read(0x00, 0x4210);
    assert_eq!(val & 0x80, 0x80);
    let val2 = bus.read(0x00, 0x4210);
    assert_eq!(val2 & 0x80, 0x00);
}

#[test]
fn nmi_is_one_shot_per_vblank() {
    let mut bus = make_bus();
    bus.write(0x00, 0x4200, 0x80);

    // Drive until first pending NMI.
    for _ in 0..20_000 {
        bus.tick();
        if bus.take_pending_nmi() {
            break;
        }
    }

    // Continue ticking through the current vblank interval.
    let mut extra_pending = 0u32;
    for _ in 0..2_000 {
        bus.tick();
        if bus.take_pending_nmi() {
            extra_pending += 1;
        }
        if bus.read(0x00, 0x4212) & 0x80 == 0 {
            break;
        }
    }
    assert_eq!(extra_pending, 0, "must not spam NMI within one vblank");
}

#[test]
fn enabling_nmi_during_vblank_fires_once() {
    let mut bus = make_bus();

    // Enter vblank with NMI disabled.
    while bus.read(0x00, 0x4212) & 0x80 == 0 {
        bus.tick();
    }

    bus.write(0x00, 0x4200, 0x80);
    assert!(
        bus.take_pending_nmi(),
        "first enable in vblank should trigger NMI"
    );

    // Toggle enable again during same vblank; should not retrigger.
    bus.write(0x00, 0x4200, 0x00);
    bus.write(0x00, 0x4200, 0x80);
    assert!(
        !bus.take_pending_nmi(),
        "re-enable within same vblank should not trigger additional NMI"
    );
}

#[test]
fn apu_ack_advances_while_idle() {
    let mut bus = make_bus();
    let before = bus.read(0x00, 0x2140);
    for _ in 0..10_000 {
        bus.tick();
    }
    let after = bus.read(0x00, 0x2140);
    assert_ne!(after, before, "idle SPC handshake should make progress");
}

#[test]
fn joypad_autoread_registers_reflect_state() {
    let mut bus = make_bus();
    // Start bit in JOY1L.
    bus.set_joypad(0x0008);
    assert_eq!(bus.read(0x00, 0x4218), 0x08);
    assert_eq!(bus.read(0x00, 0x4219), 0x00);
}

#[test]
fn joypad_serial_4016_shift_reads() {
    let mut bus = make_bus();
    // Start bit only; serial order is B,Y,Sel,Start,...
    bus.set_joypad(0x0008);

    // Latch on strobe high -> low transition.
    bus.write(0x00, 0x4016, 0x01);
    bus.write(0x00, 0x4016, 0x00);

    assert_eq!(bus.read(0x00, 0x4016), 0); // B
    assert_eq!(bus.read(0x00, 0x4016), 0); // Y
    assert_eq!(bus.read(0x00, 0x4016), 0); // Select
    assert_eq!(bus.read(0x00, 0x4016), 1); // Start
}

#[test]
fn inidisp_and_nmitimen() {
    let mut bus = make_bus();
    bus.write(0x00, 0x2100, 0x0F);
    assert_eq!(bus.inidisp, 0x0F);
    bus.write(0x00, 0x4200, 0x81);
    assert_eq!(bus.nmitimen, 0x81);
}

#[test]
fn apu_io_read_write() {
    let mut bus = make_bus();
    bus.write(0x00, 0x2140, 0x12);
    bus.write(0x00, 0x2141, 0x34);
    bus.write(0x00, 0x2142, 0x56);
    bus.write(0x00, 0x2143, 0x78);
    assert_eq!(bus.read(0x00, 0x2140), 0x12);
    assert_eq!(bus.read(0x00, 0x2141), 0x34);
    assert_eq!(bus.read(0x00, 0x2142), 0x56);
    assert_eq!(bus.read(0x00, 0x2143), 0x78);
}

#[test]
fn apu_io0_handshake_ack_model() {
    let mut bus = make_bus();

    // Initial bootstrap values
    assert_eq!(bus.read(0x00, 0x2140), 0xAA);

    // Write a command: first read returns exact value, then +1 ACK.
    bus.write(0x00, 0x2140, 0x35);
    assert_eq!(bus.read(0x00, 0x2140), 0x35);
    assert_eq!(bus.read(0x00, 0x2140), 0x36);
    assert_eq!(bus.read(0x00, 0x2140), 0x36);

    // New write resets handshake.
    bus.write(0x00, 0x2140, 0x7F);
    assert_eq!(bus.read(0x00, 0x2140), 0x7F);
    assert_eq!(bus.read(0x00, 0x2140), 0x80);
}

#[test]
fn dma_channel_registers() {
    let mut bus = make_bus();
    // Set up DMA channel 1
    bus.write(0x00, 0x4310, 0x01); // control
    bus.write(0x00, 0x4311, 0x18); // dest (VRAM)
    bus.write(0x00, 0x4312, 0x00); // src_addr low
    bus.write(0x00, 0x4313, 0x80); // src_addr high
    bus.write(0x00, 0x4314, 0x2B); // src_bank
    bus.write(0x00, 0x4315, 0x00); // size low
    bus.write(0x00, 0x4316, 0x10); // size high
    bus.write(0x00, 0x4317, 0x7E); // indirect_bank

    let ch = &bus.dma[1];
    assert_eq!(ch.control, 0x01);
    assert_eq!(ch.dest, 0x18);
    assert_eq!(ch.src_addr, 0x8000);
    assert_eq!(ch.src_bank, 0x2B);
    assert_eq!(ch.size, 0x1000);
    assert_eq!(ch.indirect_bank, 0x7E);

    // Read back via I/O
    assert_eq!(bus.read(0x00, 0x4310), 0x01);
    assert_eq!(bus.read(0x00, 0x4311), 0x18);
    assert_eq!(bus.read(0x00, 0x4312), 0x00);
    assert_eq!(bus.read(0x00, 0x4313), 0x80);
    assert_eq!(bus.read(0x00, 0x4314), 0x2B);
    assert_eq!(bus.read(0x00, 0x4315), 0x00);
    assert_eq!(bus.read(0x00, 0x4316), 0x10);
    assert_eq!(bus.read(0x00, 0x4317), 0x7E);
}

#[test]
fn dma_trigger_generates_events() {
    let mut bus = make_bus();
    bus.current_pb = 0x00;
    bus.current_pc = 0x8100;

    // Set VRAM target
    bus.write(0x00, 0x2116, 0x00);
    bus.write(0x00, 0x2117, 0x50);

    // Set up DMA channel 0
    bus.write(0x00, 0x4300, 0x01);
    bus.write(0x00, 0x4301, 0x18);
    bus.write(0x00, 0x4302, 0x00);
    bus.write(0x00, 0x4303, 0x90);
    bus.write(0x00, 0x4304, 0x01);
    bus.write(0x00, 0x4305, 0x00);
    bus.write(0x00, 0x4306, 0x08);

    // Trigger channel 0
    bus.write(0x00, 0x420B, 0x01);

    let events = bus.take_events();
    assert_eq!(events.len(), 1);
    let ev = &events[0];
    assert_eq!(ev.channel, 0);
    assert_eq!(ev.src_bank, 0x01);
    assert_eq!(ev.src_addr, 0x9000);
    assert_eq!(ev.dest_reg, 0x18);
    assert_eq!(ev.vram_addr, 0x5000);
    assert_eq!(ev.size, 0x0800);
    assert_eq!(ev.pc_bank, 0x00);
    assert_eq!(ev.pc_addr, 0x8100);

    // Events drained
    assert!(bus.take_events().is_empty());
}

#[test]
fn dma_trigger_multiple_channels() {
    let mut bus = make_bus();

    // Set up channels 0 and 2
    bus.write(0x00, 0x4301, 0x18);
    bus.write(0x00, 0x4321, 0x80);

    // Trigger channels 0 and 2 (bits 0 and 2)
    bus.write(0x00, 0x420B, 0x05);

    let events = bus.take_events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].channel, 0);
    assert_eq!(events[0].dest_reg, 0x18);
    assert_eq!(events[1].channel, 2);
    assert_eq!(events[1].dest_reg, 0x80);
}

#[test]
fn vram_direct_write_events_default_increment() {
    let mut bus = make_bus();
    bus.current_pb = 0x12;
    bus.current_pc = 0x3456;

    bus.write(0x00, 0x2116, 0x00);
    bus.write(0x00, 0x2117, 0x40); // VRAM $4000

    // Default $2115=0: increment after $2118 by +1 word
    bus.write(0x00, 0x2118, 0xAA);
    bus.write(0x00, 0x2119, 0xBB);

    let writes = bus.take_vram_writes();
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[0].port, 0x18);
    assert_eq!(writes[0].vram_addr, 0x4000);
    assert_eq!(writes[0].value, 0xAA);
    assert_eq!(writes[0].pc_bank, 0x12);
    assert_eq!(writes[0].pc_addr, 0x3456);
    assert_eq!(writes[1].port, 0x19);
    assert_eq!(writes[1].vram_addr, 0x4001);
    assert_eq!(writes[1].value, 0xBB);

    // Increment happened after low-port write only
    assert_eq!(bus.vram_addr, 0x4001);
}

#[test]
fn vram_direct_write_events_increment_after_high() {
    let mut bus = make_bus();
    bus.write(0x00, 0x2115, 0x80); // increment after high port
    bus.write(0x00, 0x2116, 0x34);
    bus.write(0x00, 0x2117, 0x12); // VRAM $1234

    bus.write(0x00, 0x2118, 0x11);
    assert_eq!(bus.vram_addr, 0x1234); // no increment yet
    bus.write(0x00, 0x2119, 0x22);
    assert_eq!(bus.vram_addr, 0x1235); // increment on high write

    let writes = bus.take_vram_writes();
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[0].vram_addr, 0x1234);
    assert_eq!(writes[1].vram_addr, 0x1234);
}

#[test]
fn high_rom_bank_mirror() {
    let bus = make_bus();
    // $C0:$8000 mirrors same as $40:$8000 via LoROM
    let val_c0 = bus.read(0xC0, 0x8000);
    let val_40 = bus.read(0x40, 0x8000);
    assert_eq!(val_c0, val_40);
}

#[test]
fn dma_trigger_generates_trace_events() {
    let mut bus = make_bus();
    bus.current_pb = 0x00;
    bus.current_pc = 0x8100;
    bus.write(0x00, 0x2116, 0x00);
    bus.write(0x00, 0x2117, 0x50);

    bus.write(0x00, 0x4300, 0x01);
    bus.write(0x00, 0x4301, 0x18);
    bus.write(0x00, 0x4302, 0x00);
    bus.write(0x00, 0x4303, 0x90);
    bus.write(0x00, 0x4304, 0x01);
    bus.write(0x00, 0x4305, 0x00);
    bus.write(0x00, 0x4306, 0x08);

    // Clear any trace events from VRAM addr setup
    bus.take_trace_events();

    bus.write(0x00, 0x420B, 0x01);

    let trace_events = bus.take_trace_events();
    // Should have DmaStart + DmaDone
    assert!(trace_events.len() >= 2);
    match &trace_events[0] {
        TraceEvent::DmaStart {
            channel,
            src_bank,
            dest_reg,
            seq,
            ..
        } => {
            assert_eq!(*channel, 0);
            assert_eq!(*src_bank, 0x01);
            assert_eq!(*dest_reg, 0x18);
            assert!(*seq > 0);
        }
        other => panic!("Expected DmaStart, got {:?}", other),
    }
    // Last event should be DmaDone
    match trace_events.last().unwrap() {
        TraceEvent::DmaDone { channel, .. } => assert_eq!(*channel, 0),
        other => panic!("Expected DmaDone, got {:?}", other),
    }
}

#[test]
fn vram_direct_write_generates_trace_event() {
    let mut bus = make_bus();
    bus.current_pb = 0x00;
    bus.current_pc = 0x9000;
    bus.write(0x00, 0x2116, 0x00);
    bus.write(0x00, 0x2117, 0x40);
    bus.take_trace_events(); // clear

    bus.write(0x00, 0x2118, 0xAA);
    let trace_events = bus.take_trace_events();
    assert_eq!(trace_events.len(), 1);
    match &trace_events[0] {
        TraceEvent::VramWrite {
            port,
            vram_addr,
            value,
            ..
        } => {
            assert_eq!(*port, 0x18);
            assert_eq!(*vram_addr, 0x4000);
            assert_eq!(*value, 0xAA);
        }
        other => panic!("Expected VramWrite, got {:?}", other),
    }
}

#[test]
fn vram_shadow_updated_on_direct_write() {
    let mut bus = make_bus();
    bus.write(0x00, 0x2115, 0x80); // increment after high port
    bus.write(0x00, 0x2116, 0x00);
    bus.write(0x00, 0x2117, 0x10); // VRAM word $1000

    bus.write(0x00, 0x2118, 0xAB); // low byte
    bus.write(0x00, 0x2119, 0xCD); // high byte

    // VRAM shadow: word $1000 = byte offset $2000
    assert_eq!(bus.vram[0x2000], 0xAB);
    assert_eq!(bus.vram[0x2001], 0xCD);
}

#[test]
fn dma_copies_bytes_to_vram_shadow() {
    let mut bus = make_bus();
    // Source: $01:$8000 = 0xCD (from make_bus)
    bus.write(0x00, 0x2115, 0x80); // increment after high
    bus.write(0x00, 0x2116, 0x00);
    bus.write(0x00, 0x2117, 0x20); // VRAM word $2000

    // DMA mode 1: 2-register (p, p+1 = $18, $19)
    bus.write(0x00, 0x4300, 0x01); // control = mode 1
    bus.write(0x00, 0x4301, 0x18); // dest = VRAM $2118
    bus.write(0x00, 0x4302, 0x00); // src lo
    bus.write(0x00, 0x4303, 0x80); // src hi
    bus.write(0x00, 0x4304, 0x01); // src bank
    bus.write(0x00, 0x4305, 0x02); // size lo = 2 bytes
    bus.write(0x00, 0x4306, 0x00); // size hi

    bus.write(0x00, 0x420B, 0x01); // trigger ch0

    // Word $2000, byte $4000: low = rom[0x8000]=0xCD, high = rom[0x8001]
    assert_eq!(bus.vram[0x4000], 0xCD);
}

#[test]
fn cgram_write_alternates_low_high() {
    let mut bus = make_bus();
    bus.write(0x00, 0x2121, 0x05); // CGRAM addr = 5
    bus.write(0x00, 0x2122, 0x12); // color 5 low
    bus.write(0x00, 0x2122, 0x34); // color 5 high
    assert_eq!(bus.cgram[10], 0x12);
    assert_eq!(bus.cgram[11], 0x34);
}

#[test]
fn oam_data_write() {
    let mut bus = make_bus();
    bus.write(0x00, 0x2102, 0x00);
    bus.write(0x00, 0x2103, 0x00);
    bus.write(0x00, 0x2104, 0x42);
    bus.write(0x00, 0x2104, 0x43);
    assert_eq!(bus.oam[0], 0x42);
    assert_eq!(bus.oam[1], 0x43);
}

#[test]
fn nmi_latch_clear_on_read() {
    let bus = make_bus();
    bus.nmi_latch.set(true);
    let val = bus.read(0x00, 0x4210);
    assert_eq!(val & 0x80, 0x80); // NMI pending
    let val2 = bus.read(0x00, 0x4210);
    assert_eq!(val2 & 0x80, 0x00); // cleared
}

#[test]
fn trace_event_json_roundtrip() {
    let event = TraceEvent::DmaStart {
        channel: 0,
        src_bank: 0x01,
        src_addr: 0x8000,
        dest_reg: 0x18,
        vram_addr: 0x5000,
        size: 0x0800,
        pc_bank: 0x00,
        pc_addr: 0x8100,
        seq: 42,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("DMA_START"));
    assert!(json.contains("\"seq\":42"));
    // Verify it's valid JSON
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["type"], "DMA_START");
}

#[test]
fn dma_wram_to_wram_copy() {
    let mut bus = make_bus();
    // Write source data to WRAM
    bus.wram[0x100] = 0xAA;
    bus.wram[0x101] = 0xBB;
    bus.wram[0x102] = 0xCC;

    // Set WRAM dest address via $2181-$2183
    bus.write(0x00, 0x2181, 0x00);
    bus.write(0x00, 0x2182, 0x10); // WRAM $1000
    bus.write(0x00, 0x2183, 0x00);

    // DMA mode 0 (1-register), dest=$80 (WRAM $2180), src=$00:$0100
    bus.write(0x00, 0x4300, 0x00); // control
    bus.write(0x00, 0x4301, 0x80); // dest = WRAM
    bus.write(0x00, 0x4302, 0x00); // src lo = $00
    bus.write(0x00, 0x4303, 0x01); // src hi = $01 → $0100
    bus.write(0x00, 0x4304, 0x00); // src bank = 0
    bus.write(0x00, 0x4305, 0x03); // size = 3
    bus.write(0x00, 0x4306, 0x00);

    bus.write(0x00, 0x420B, 0x01); // trigger

    assert_eq!(bus.wram[0x1000], 0xAA);
    assert_eq!(bus.wram[0x1001], 0xBB);
    assert_eq!(bus.wram[0x1002], 0xCC);
}
