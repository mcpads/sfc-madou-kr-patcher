//! 65816 execution tracer — traces SNES boot sequence to find DMA targets.
//!
//! Runs a minimal 65816 emulator from the reset vector, recording all DMA
//! transfers to detect when/where the game loads fonts into VRAM.

pub mod bus;
pub mod capture;
pub mod cpu;
pub mod decode;
pub mod execute;
pub mod scenario;

use bus::{DmaEvent, MemoryBus, TraceEvent, VramWriteEvent};
use cpu::{BusAccess, CpuState};
use execute::CallFrame;

const JOYPAD_START: u16 = 0x0008;
const SKY_WORLDMAP_LZ_ADDRS: &[u16] = &[0x818C, 0xD83C, 0x8774, 0x9ACD];
const MENU_WORLDMAP_LZ_ADDRS: &[u16] = &[0xB784, 0xB9E7, 0xB10C];

/// Configuration for the tracer.
pub struct TracerConfig {
    /// Maximum instructions before stopping (default: 500K).
    pub max_instructions: u64,
    /// VRAM word address to watch for DMA/direct writes (default: 0x5000).
    pub target_vram: u16,
    /// Stop on first target VRAM hit (DMA or direct write) (default: true).
    pub stop_on_target: bool,
    /// Print each instruction as it executes.
    pub verbose: bool,
    /// Simulate NMI when VBlank wait loop is detected.
    pub inject_nmi: bool,
    /// Maximum number of NMI injections before giving up.
    pub max_nmi: u32,
    /// Simulate Start button press in joypad registers.
    pub start_button: bool,
    /// Simulate Start as a pulse on each injected NMI (edge-like input).
    pub start_interrupt: bool,
    /// Log each JSL $009440 call with dp$0B/dp$0C snapshot.
    pub log_lz_calls: bool,
    /// Allow Z-flag toggle as loop-break fallback (default: true for backwards compat).
    pub force_loop_break: bool,
}

impl Default for TracerConfig {
    fn default() -> Self {
        Self {
            max_instructions: 500_000,
            target_vram: 0x5000,
            stop_on_target: true,
            verbose: false,
            inject_nmi: true,
            max_nmi: 60,
            start_button: false,
            start_interrupt: false,
            log_lz_calls: false,
            force_loop_break: true,
        }
    }
}

/// A single DMA event with full context.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DmaRecord {
    pub event: DmaEvent,
    pub instruction_num: u64,
    pub call_stack: Vec<CallFrame>,
}

/// A single direct VRAM port write event with full context.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct VramWriteRecord {
    pub event: VramWriteEvent,
    pub instruction_num: u64,
    pub call_stack: Vec<CallFrame>,
}

/// Result of a trace run.
pub struct TraceResult {
    pub instructions_executed: u64,
    pub dma_records: Vec<DmaRecord>,
    pub target_hits: Vec<DmaRecord>,
    pub vram_write_records: Vec<VramWriteRecord>,
    pub target_vram_write_hits: Vec<VramWriteRecord>,
    pub final_pc: (u8, u16),
    pub stop_reason: StopReason,
    /// All trace events collected during the run.
    pub trace_events: Vec<TraceEvent>,
    /// Final VRAM shadow snapshot at trace stop.
    pub final_vram: Vec<u8>,
    /// Final CGRAM shadow snapshot at trace stop.
    pub final_cgram: [u8; 512],
}

#[derive(Debug)]
pub enum StopReason {
    TargetHit,
    MaxInstructions,
    CpuStopped,
    #[allow(dead_code)]
    Error(String),
}

#[allow(clippy::too_many_arguments)]
fn deliver_nmi(
    cpu: &mut CpuState,
    bus: &mut MemoryBus,
    all_trace_events: &mut Vec<TraceEvent>,
    nmi_count: &mut u32,
    nmi_vector: u16,
    start_interrupt: bool,
    start_pulse_remaining: &mut u32,
    start_pulse_inst: u32,
) {
    *nmi_count += 1;
    if start_interrupt {
        bus.set_joypad(JOYPAD_START);
        *start_pulse_remaining = start_pulse_inst;
        eprintln!(
            "[trace] Start pulse on NMI #{} ({} inst)",
            nmi_count, start_pulse_inst
        );
    }
    eprintln!(
        "[NMI #{}] Injecting at ${:02X}:${:04X} → handler $00:${:04X}",
        nmi_count, cpu.pb, cpu.pc, nmi_vector
    );
    let seq = bus.next_seq();
    all_trace_events.push(TraceEvent::NmiInject {
        nmi_num: *nmi_count,
        pc_bank: cpu.pb,
        pc_addr: cpu.pc,
        handler_addr: nmi_vector,
        seq,
    });
    let hash_seq = bus.next_seq();
    all_trace_events.push(TraceEvent::VramHash {
        nmi_num: *nmi_count,
        pc_bank: cpu.pb,
        pc_addr: cpu.pc,
        hash32: fnv1a32(&bus.vram),
        seq: hash_seq,
    });
    bus.nmi_latch.set(true);

    // 65816 NMI stack push order:
    // - Native: PBR, PCH, PCL, P
    // - Emulation: PCH, PCL, P
    if !cpu.emulation {
        cpu.push8(bus, cpu.pb);
    }
    cpu.push16(bus, cpu.pc);
    cpu.push8(bus, cpu.p);
    cpu.p |= 0x04; // Set I
    cpu.p &= !0x08; // Clear D
    cpu.pb = 0x00;
    cpu.pc = nmi_vector;
    cpu.waiting = false;
}

fn resolve_nmi_vector(cpu: &CpuState, bus: &MemoryBus) -> Option<u16> {
    let vec_addr = if cpu.emulation { 0xFFFA } else { 0xFFEA };
    let vec = bus.read16(0x00, vec_addr);
    if vec == 0 || vec == 0xFFFF {
        None
    } else {
        Some(vec)
    }
}

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811C9DC5;
    for b in bytes {
        hash ^= *b as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    hash
}

/// Run the tracer on a ROM.
pub fn run_trace(rom: Vec<u8>, config: &TracerConfig) -> TraceResult {
    let mut bus = MemoryBus::new(rom);
    let mut cpu = CpuState::reset();

    // Read reset vector from $00:FFFC
    let reset_lo = bus.read(0x00, 0xFFFC);
    let reset_hi = bus.read(0x00, 0xFFFD);
    cpu.pc = u16::from_le_bytes([reset_lo, reset_hi]);
    cpu.pb = 0x00;

    eprintln!("[trace] Reset vector: ${:02X}:${:04X}", cpu.pb, cpu.pc);

    // Set up joypad registers for Start simulation.
    if config.start_interrupt {
        bus.set_joypad(0x0000);
        eprintln!("[trace] Start interrupt mode enabled (pulse on injected NMI)");
    } else if config.start_button {
        bus.set_joypad(JOYPAD_START);
        eprintln!("[trace] Start button enabled (JOY1=${:04X})", JOYPAD_START);
    }

    let mut call_stack: Vec<CallFrame> = Vec::new();
    let mut all_dma: Vec<DmaRecord> = Vec::new();
    let mut target_hits: Vec<DmaRecord> = Vec::new();
    let mut all_vram_writes: Vec<VramWriteRecord> = Vec::new();
    let mut target_vram_write_hits: Vec<VramWriteRecord> = Vec::new();
    let mut all_trace_events: Vec<TraceEvent> = Vec::new();
    let mut instruction_count: u64 = 0;
    let mut stop_reason = StopReason::MaxInstructions;
    let mut start_pulse_remaining: u32 = 0;
    const START_PULSE_INST: u32 = 2048;

    // Loop detection: if PC stays within a small range for too long, inject NMI
    let mut loop_range_bank: u8 = 0;
    let mut loop_range_base: u16 = 0;
    let mut loop_range_count: u32 = 0;
    let mut nmi_count: u32 = 0;
    let mut synthetic_nmi_count: u32 = 0;
    const LOOP_RANGE: u16 = 32;
    const LOOP_THRESHOLD: u32 = 768; // ~256 iterations of a 3-instruction loop

    while instruction_count < config.max_instructions {
        // Advance coarse hardware timing (HVBJOY/APU handshake/NMI edge).
        bus.tick();

        // End Start pulse after a short window.
        if config.start_interrupt && start_pulse_remaining > 0 {
            start_pulse_remaining -= 1;
            if start_pulse_remaining == 0 {
                bus.set_joypad(0x0000);
            }
        }

        // Deliver hardware pending NMI between instructions.
        if config.inject_nmi && bus.take_pending_nmi() {
            let Some(nmi_vector) = resolve_nmi_vector(&cpu, &bus) else {
                instruction_count += 1;
                continue;
            };
            deliver_nmi(
                &mut cpu,
                &mut bus,
                &mut all_trace_events,
                &mut nmi_count,
                nmi_vector,
                config.start_interrupt,
                &mut start_pulse_remaining,
                START_PULSE_INST,
            );
            loop_range_count = 0;
            instruction_count += 1;
            continue;
        }

        // WAI state: CPU halts until an interrupt is delivered.
        if cpu.waiting {
            instruction_count += 1;
            continue;
        }

        if cpu.stopped {
            stop_reason = StopReason::CpuStopped;
            break;
        }

        let current_pb = cpu.pb;
        let current_pc = cpu.pc;
        bus.current_pb = current_pb;
        bus.current_pc = current_pc;

        // Detect JSL $009440 (LZ decompressor) call sites.
        {
            let opcode = bus.read(current_pb, current_pc);
            if opcode == 0x22 {
                let tgt_lo = bus.read(current_pb, current_pc.wrapping_add(1));
                let tgt_hi = bus.read(current_pb, current_pc.wrapping_add(2));
                let tgt_bank = bus.read(current_pb, current_pc.wrapping_add(3));
                if tgt_lo == 0x40 && tgt_hi == 0x94 && tgt_bank == 0x00 {
                    let dp_0b = bus.read(0x00, cpu.dp.wrapping_add(0x0B));
                    let dp_0c_lo = bus.read(0x00, cpu.dp.wrapping_add(0x0C));
                    let dp_0c_hi = bus.read(0x00, cpu.dp.wrapping_add(0x0D));
                    let dp_0c = u16::from_le_bytes([dp_0c_lo, dp_0c_hi]);
                    let seq = bus.next_seq();
                    all_trace_events.push(TraceEvent::LzCall {
                        pc_bank: current_pb,
                        pc_addr: current_pc,
                        dp_0b,
                        dp_0c,
                        seq,
                    });

                    let sky_guard_passed =
                        (dp_0b == 0x11 || dp_0b == 0x12) && SKY_WORLDMAP_LZ_ADDRS.contains(&dp_0c);
                    let sky_seq = bus.next_seq();
                    all_trace_events.push(TraceEvent::HookGuardCheck {
                        hook: "sky_worldmap".to_string(),
                        passed: sky_guard_passed,
                        pc_bank: current_pb,
                        pc_addr: current_pc,
                        dp_0b,
                        dp_0c,
                        seq: sky_seq,
                    });

                    let menu_guard_passed =
                        dp_0b == 0x25 && MENU_WORLDMAP_LZ_ADDRS.contains(&dp_0c);
                    let menu_seq = bus.next_seq();
                    all_trace_events.push(TraceEvent::HookGuardCheck {
                        hook: "menu_worldmap".to_string(),
                        passed: menu_guard_passed,
                        pc_bank: current_pb,
                        pc_addr: current_pc,
                        dp_0b,
                        dp_0c,
                        seq: menu_seq,
                    });

                    if config.log_lz_calls && config.verbose {
                        eprintln!(
                            "[LZ] JSL $009440 @${:02X}:${:04X} DP=${:04X} dp$0B=${:02X} dp$0C=${:04X}",
                            current_pb, current_pc, cpu.dp, dp_0b, dp_0c
                        );
                    }
                }
            }
        }

        // Verbose logging
        if config.verbose {
            let opcode = bus.read(cpu.pb, cpu.pc);
            let info = &decode::OPCODE_TABLE[opcode as usize];
            eprintln!(
                "  #{}: ${:02X}:${:04X}  {:02X} {:?} {:?}",
                instruction_count, cpu.pb, cpu.pc, opcode, info.op, info.mode
            );
        }

        // Execute one instruction
        let _old_pc = (cpu.pb, cpu.pc);
        match execute::execute_one(&mut cpu, &mut bus, &mut call_stack) {
            Ok(()) => {}
            Err(e) => {
                eprintln!(
                    "[trace] Error at ${:02X}:${:04X}: {}",
                    current_pb, current_pc, e
                );
                stop_reason = StopReason::Error(e);
                break;
            }
        }

        instruction_count += 1;

        // Drain bus-level trace events into our accumulator.
        all_trace_events.extend(bus.take_trace_events());

        // Collect DMA events
        let events = bus.take_events();
        for event in events {
            let record = DmaRecord {
                event: event.clone(),
                instruction_num: instruction_count,
                call_stack: call_stack.clone(),
            };

            // Print DMA event
            let dest_name = match event.dest_reg {
                0x18 => format!("VRAM=${:04X}", event.vram_addr),
                0x80 => "WRAM".to_string(),
                0x04 => "OAM".to_string(),
                0x22 => "CGRAM".to_string(),
                other => format!("${:02X}", other),
            };
            eprintln!(
                "[DMA] ch{} ${:02X}:${:04X} -> ${:04X} {} ({}B) @${:02X}:${:04X}",
                event.channel,
                event.src_bank,
                event.src_addr,
                0x2100 | event.dest_reg as u16,
                dest_name,
                event.size,
                event.pc_bank,
                event.pc_addr,
            );

            // Check for target hit
            if event.dest_reg == 0x18 && event.vram_addr == config.target_vram {
                eprintln!();
                eprintln!("=== VRAM ${:04X} HIT ===", config.target_vram);
                eprintln!("  PC: ${:02X}:${:04X}", event.pc_bank, event.pc_addr);
                eprintln!(
                    "  DMA ch{}: ${:02X}:${:04X} -> VRAM ${:04X} ({}B)",
                    event.channel, event.src_bank, event.src_addr, event.vram_addr, event.size,
                );
                eprintln!("  Instruction #: {}", instruction_count);
                if !call_stack.is_empty() {
                    eprintln!("  Call stack:");
                    for (i, frame) in call_stack.iter().enumerate() {
                        eprintln!(
                            "    #{}: ${:02X}:${:04X} ({})",
                            i,
                            frame.bank,
                            frame.addr,
                            if frame.is_long { "JSL" } else { "JSR" }
                        );
                    }
                }
                eprintln!();

                target_hits.push(record.clone());

                if config.stop_on_target {
                    stop_reason = StopReason::TargetHit;
                    all_dma.push(record);
                    print_summary(
                        instruction_count,
                        &all_dma,
                        &target_hits,
                        &all_vram_writes,
                        &target_vram_write_hits,
                        config,
                    );
                    return TraceResult {
                        instructions_executed: instruction_count,
                        dma_records: all_dma,
                        target_hits,
                        vram_write_records: all_vram_writes,
                        target_vram_write_hits,
                        final_pc: (cpu.pb, cpu.pc),
                        stop_reason,
                        trace_events: all_trace_events,
                        final_vram: bus.vram.clone(),
                        final_cgram: bus.cgram,
                    };
                }
            }

            all_dma.push(record);
        }

        // Collect direct VRAM port write events ($2118/$2119).
        let vram_events = bus.take_vram_writes();
        for event in vram_events {
            let record = VramWriteRecord {
                event: event.clone(),
                instruction_num: instruction_count,
                call_stack: call_stack.clone(),
            };

            if config.verbose {
                eprintln!(
                    "[VRAMW] ${:04X} <= ${:02X} via ${:04X} @${:02X}:${:04X}",
                    event.vram_addr,
                    event.value,
                    0x2100 | event.port as u16,
                    event.pc_bank,
                    event.pc_addr,
                );
            }

            if event.vram_addr == config.target_vram {
                eprintln!();
                eprintln!("=== VRAM ${:04X} DIRECT WRITE HIT ===", config.target_vram);
                eprintln!("  PC: ${:02X}:${:04X}", event.pc_bank, event.pc_addr);
                eprintln!(
                    "  Write: ${:04X} <= ${:02X} at VRAM ${:04X}",
                    0x2100 | event.port as u16,
                    event.value,
                    event.vram_addr,
                );
                eprintln!("  Instruction #: {}", instruction_count);
                if !call_stack.is_empty() {
                    eprintln!("  Call stack:");
                    for (i, frame) in call_stack.iter().enumerate() {
                        eprintln!(
                            "    #{}: ${:02X}:${:04X} ({})",
                            i,
                            frame.bank,
                            frame.addr,
                            if frame.is_long { "JSL" } else { "JSR" }
                        );
                    }
                }
                eprintln!();

                target_vram_write_hits.push(record.clone());
                if config.stop_on_target {
                    stop_reason = StopReason::TargetHit;
                    all_vram_writes.push(record);
                    print_summary(
                        instruction_count,
                        &all_dma,
                        &target_hits,
                        &all_vram_writes,
                        &target_vram_write_hits,
                        config,
                    );
                    return TraceResult {
                        instructions_executed: instruction_count,
                        dma_records: all_dma,
                        target_hits,
                        vram_write_records: all_vram_writes,
                        target_vram_write_hits,
                        final_pc: (cpu.pb, cpu.pc),
                        stop_reason,
                        trace_events: all_trace_events,
                        final_vram: bus.vram.clone(),
                        final_cgram: bus.cgram,
                    };
                }
            }

            all_vram_writes.push(record);
        }

        // Loop detection: PC stays within a small range for too long
        let in_range = cpu.pb == loop_range_bank
            && cpu.pc >= loop_range_base
            && cpu.pc < loop_range_base.saturating_add(LOOP_RANGE);

        if in_range {
            loop_range_count += 1;
        } else {
            loop_range_bank = cpu.pb;
            loop_range_base = cpu.pc.saturating_sub(LOOP_RANGE / 2);
            loop_range_count = 1;
        }

        if loop_range_count >= LOOP_THRESHOLD {
            // MVP/MVN stays at the same PC by design while moving a large block.
            // Treat it as progress, not a dead loop.
            let loop_opcode = bus.read(cpu.pb, cpu.pc);
            if loop_opcode == 0x44 || loop_opcode == 0x54 {
                loop_range_count = 0;
                continue;
            }

            // Try NMI injection: use NMI vector validity, not $4200 register
            // (the game may enable NMI via a write our tracer doesn't track perfectly)
            if config.inject_nmi && synthetic_nmi_count < config.max_nmi {
                let Some(nmi_vector) = resolve_nmi_vector(&cpu, &bus) else {
                    loop_range_count = 0;
                    continue;
                };
                synthetic_nmi_count += 1;
                deliver_nmi(
                    &mut cpu,
                    &mut bus,
                    &mut all_trace_events,
                    &mut nmi_count,
                    nmi_vector,
                    config.start_interrupt,
                    &mut start_pulse_remaining,
                    START_PULSE_INST,
                );
                loop_range_count = 0;
                continue;
            }
            // Fallback: toggle branch condition to break out (only if allowed)
            if config.force_loop_break {
                eprintln!(
                    "[trace] Loop at ${:02X}:${:04X}, forcing exit (Z toggle)",
                    cpu.pb, cpu.pc
                );
                let z = cpu.flag_z();
                cpu.set_z(!z);
                let seq = bus.next_seq();
                all_trace_events.push(TraceEvent::LoopBreak {
                    pc_bank: cpu.pb,
                    pc_addr: cpu.pc,
                    method: "z_toggle".to_string(),
                    seq,
                });
            } else {
                eprintln!(
                    "[trace] Loop at ${:02X}:${:04X}, no loop-break (--no-force-loop-break)",
                    cpu.pb, cpu.pc
                );
            }
            loop_range_count = 0;
        }

        // Progress reporting
        if instruction_count.is_multiple_of(50_000) {
            eprintln!(
                "[trace] {}K instructions, PC=${:02X}:${:04X}",
                instruction_count / 1000,
                cpu.pb,
                cpu.pc
            );
        }
    }

    print_summary(
        instruction_count,
        &all_dma,
        &target_hits,
        &all_vram_writes,
        &target_vram_write_hits,
        config,
    );

    TraceResult {
        instructions_executed: instruction_count,
        dma_records: all_dma,
        target_hits,
        vram_write_records: all_vram_writes,
        target_vram_write_hits,
        final_pc: (cpu.pb, cpu.pc),
        stop_reason,
        trace_events: all_trace_events,
        final_vram: bus.vram,
        final_cgram: bus.cgram,
    }
}

fn print_summary(
    instruction_count: u64,
    all_dma: &[DmaRecord],
    target_hits: &[DmaRecord],
    all_vram_writes: &[VramWriteRecord],
    target_vram_write_hits: &[VramWriteRecord],
    config: &TracerConfig,
) {
    eprintln!();
    eprintln!("=== Trace Summary ===");
    eprintln!("Instructions executed: {}", instruction_count);
    eprintln!("DMA transfers: {}", all_dma.len());
    eprintln!(
        "DMA target hits (VRAM ${:04X}): {}",
        config.target_vram,
        target_hits.len()
    );
    eprintln!("VRAM direct writes: {}", all_vram_writes.len());
    eprintln!(
        "Direct target hits (VRAM ${:04X}): {}",
        config.target_vram,
        target_vram_write_hits.len()
    );
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
