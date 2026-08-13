// SNES LoROM memory bus for the 65816 execution tracer.
// Implements enough I/O to trace through game code (DMA, VRAM, WRAM).

use super::cpu::BusAccess;
use crate::rom::lorom_to_pc;
use serde::Serialize;
use std::cell::Cell;

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811C9DC5;
    for b in bytes {
        hash ^= *b as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    hash
}

/// Recorded DMA transfer event.
#[derive(Debug, Clone, Serialize)]
pub struct DmaEvent {
    pub channel: u8,
    pub src_bank: u8,
    pub src_addr: u16,
    pub dest_reg: u8,
    pub vram_addr: u16,
    pub size: u16,
    pub pc_bank: u8,
    pub pc_addr: u16,
}

/// Recorded direct VRAM port write event ($2118/$2119).
#[derive(Debug, Clone, Serialize)]
pub struct VramWriteEvent {
    /// VRAM data port: $18 ($2118, low byte) or $19 ($2119, high byte)
    pub port: u8,
    /// VRAM word address at write time.
    pub vram_addr: u16,
    /// Written byte value.
    pub value: u8,
    pub pc_bank: u8,
    pub pc_addr: u16,
}

/// Unified trace event for JSON export (FR-01).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum TraceEvent {
    #[serde(rename = "LZ_CALL")]
    LzCall {
        pc_bank: u8,
        pc_addr: u16,
        dp_0b: u8,
        dp_0c: u16,
        seq: u64,
    },
    #[serde(rename = "HOOK_GUARD_CHECK")]
    HookGuardCheck {
        hook: String,
        passed: bool,
        pc_bank: u8,
        pc_addr: u16,
        dp_0b: u8,
        dp_0c: u16,
        seq: u64,
    },
    #[serde(rename = "DMA_START")]
    DmaStart {
        channel: u8,
        src_bank: u8,
        src_addr: u16,
        dest_reg: u8,
        vram_addr: u16,
        size: u16,
        pc_bank: u8,
        pc_addr: u16,
        seq: u64,
    },
    #[serde(rename = "DMA_DONE")]
    DmaDone { channel: u8, seq: u64 },
    #[serde(rename = "VRAM_WRITE")]
    VramWrite {
        port: u8,
        vram_addr: u16,
        value: u8,
        pc_bank: u8,
        pc_addr: u16,
        seq: u64,
    },
    #[serde(rename = "NMI_INJECT")]
    NmiInject {
        nmi_num: u32,
        pc_bank: u8,
        pc_addr: u16,
        handler_addr: u16,
        seq: u64,
    },
    #[serde(rename = "VRAM_HASH")]
    VramHash {
        nmi_num: u32,
        pc_bank: u8,
        pc_addr: u16,
        hash32: u32,
        seq: u64,
    },
    #[serde(rename = "FRAME_MARKER")]
    FrameMarker {
        frame_num: u32,
        inidisp: u8,
        vram_hash32: u32,
        cgram_hash32: u32,
        oam_hash32: u32,
        seq: u64,
    },
    #[serde(rename = "LOOP_BREAK")]
    LoopBreak {
        pc_bank: u8,
        pc_addr: u16,
        method: String,
        seq: u64,
    },
}

impl TraceEvent {
    pub fn seq(&self) -> u64 {
        match self {
            TraceEvent::LzCall { seq, .. } => *seq,
            TraceEvent::HookGuardCheck { seq, .. } => *seq,
            TraceEvent::DmaStart { seq, .. } => *seq,
            TraceEvent::DmaDone { seq, .. } => *seq,
            TraceEvent::VramWrite { seq, .. } => *seq,
            TraceEvent::NmiInject { seq, .. } => *seq,
            TraceEvent::VramHash { seq, .. } => *seq,
            TraceEvent::FrameMarker { seq, .. } => *seq,
            TraceEvent::LoopBreak { seq, .. } => *seq,
        }
    }
}

/// State of a single SNES DMA channel ($43x0-$43xB).
#[derive(Debug, Clone, Default)]
pub struct DmaChannel {
    pub control: u8,
    pub dest: u8,
    pub src_addr: u16,
    pub src_bank: u8,
    pub size: u16,
    pub indirect_bank: u8,
}

/// SNES LoROM memory bus with I/O stubs for tracing.
pub struct MemoryBus {
    rom: Vec<u8>,
    pub wram: Vec<u8>,
    /// 64KB VRAM shadow buffer (32K words).
    pub vram: Vec<u8>,
    /// 512-byte CGRAM shadow buffer.
    pub cgram: [u8; 512],
    /// 544-byte OAM shadow buffer (512 + 32 high table).
    pub oam: [u8; 544],
    cgram_addr: u16,
    cgram_flipflop: bool,
    oam_addr: u16,
    pub vram_addr: u16,
    pub vram_incr: u8,
    pub wram_addr: u32,
    pub dma: [DmaChannel; 8],
    pub inidisp: u8,
    pub nmitimen: u8,
    /// APU I/O ports ($2140-$2143) from CPU side.
    pub apu_io: [u8; 4],
    /// Simple handshake model for $2140 readback/ack.
    apu_io0_ack: Cell<u8>,
    apu_io0_first_read_pending: Cell<bool>,
    /// Idle ticks for synthetic SPC handshake progress.
    apu_idle_ticks: u32,
    pub events: Vec<DmaEvent>,
    pub vram_writes: Vec<VramWriteEvent>,
    /// Unified trace event log for JSON export.
    pub trace_events: Vec<TraceEvent>,
    /// Monotonic event sequence counter.
    pub event_seq: u64,
    pub current_pb: u8,
    pub current_pc: u16,
    /// Joypad 1 data (16-bit: JOY1L=$4218 | JOY1H=$4219<<8)
    pub joypad1: u16,
    /// Serial joypad state for manual polling via $4016.
    joypad1_shift: Cell<u16>,
    joypad_strobe: Cell<bool>,
    /// NMI latch ($4210 bit 7), cleared on read.
    pub nmi_latch: Cell<bool>,
    /// Pending NMI request for CPU delivery between instructions.
    pending_nmi: bool,
    /// Monotonic timing tick for coarse HVBJOY/NMI modeling.
    timing_tick: u64,
    /// Cached $4212 (HVBJOY) bits.
    hvbjoy: u8,
    /// Previous vblank level for rising-edge detection.
    vblank_prev: bool,
    /// One-shot NMI already emitted for the current vblank interval.
    nmi_issued_this_vblank: bool,
    /// 1-based frame count at each vblank rising edge.
    frame_counter: u32,
}

impl MemoryBus {
    const FRAME_TICKS: u32 = 8_192;
    const VBLANK_TICKS: u32 = 1_024;
    const HBLANK_PERIOD_TICKS: u32 = 64;
    const HBLANK_WIDTH_TICKS: u32 = 8;
    const AUTO_JOYPAD_BUSY_TICKS: u32 = 32;
    const APU_IDLE_ACK_PERIOD: u32 = 2_048;

    pub fn new(rom: Vec<u8>) -> Self {
        Self {
            rom,
            wram: vec![0u8; 0x20000], // 128KB
            vram: vec![0u8; 0x10000], // 64KB (32K words)
            cgram: [0u8; 512],
            oam: [0u8; 544],
            cgram_addr: 0,
            cgram_flipflop: false,
            oam_addr: 0,
            vram_addr: 0,
            vram_incr: 0,
            wram_addr: 0,
            dma: Default::default(),
            inidisp: 0,
            nmitimen: 0,
            apu_io: [0xAA, 0xBB, 0x00, 0x00],
            apu_io0_ack: Cell::new(0xAA),
            apu_io0_first_read_pending: Cell::new(false),
            apu_idle_ticks: 0,
            events: Vec::new(),
            vram_writes: Vec::new(),
            trace_events: Vec::new(),
            event_seq: 0,
            current_pb: 0,
            current_pc: 0,
            joypad1: 0,
            joypad1_shift: Cell::new(0),
            joypad_strobe: Cell::new(false),
            nmi_latch: Cell::new(false),
            pending_nmi: false,
            timing_tick: 0,
            // Keep compatibility with prior stub default: vblank+auto-joy busy set.
            hvbjoy: 0x81,
            vblank_prev: true,
            nmi_issued_this_vblank: false,
            frame_counter: 0,
        }
    }

    /// Set joypad 1 button state (16-bit: JOY1L | JOY1H<<8).
    /// JOY1L bits: B Y Select Start Up Down Left Right.
    /// JOY1H bits: A X L R ....
    pub fn set_joypad(&mut self, val: u16) {
        self.joypad1 = val;
        if self.joypad_strobe.get() {
            self.joypad1_shift.set(val);
        }
    }

    /// Advance coarse PPU/APU timing by one tracer tick.
    pub fn tick(&mut self) {
        self.timing_tick = self.timing_tick.wrapping_add(1);

        let frame_pos = (self.timing_tick % Self::FRAME_TICKS as u64) as u32;
        let vblank_start = Self::FRAME_TICKS - Self::VBLANK_TICKS;
        let vblank = frame_pos >= vblank_start;
        let hblank = (frame_pos % Self::HBLANK_PERIOD_TICKS)
            >= (Self::HBLANK_PERIOD_TICKS - Self::HBLANK_WIDTH_TICKS);
        let auto_joy_busy = vblank && frame_pos < vblank_start + Self::AUTO_JOYPAD_BUSY_TICKS;

        self.hvbjoy = (if vblank { 0x80 } else { 0x00 })
            | (if hblank { 0x40 } else { 0x00 })
            | (if auto_joy_busy { 0x01 } else { 0x00 });

        if !self.vblank_prev && vblank {
            self.frame_counter = self.frame_counter.wrapping_add(1);
            let seq = self.next_seq();
            self.trace_events.push(TraceEvent::FrameMarker {
                frame_num: self.frame_counter,
                inidisp: self.inidisp,
                vram_hash32: fnv1a32(&self.vram),
                cgram_hash32: fnv1a32(&self.cgram),
                oam_hash32: fnv1a32(&self.oam),
                seq,
            });
        }

        if !self.vblank_prev
            && vblank
            && (self.nmitimen & 0x80) != 0
            && !self.nmi_issued_this_vblank
        {
            self.pending_nmi = true;
            self.nmi_latch.set(true);
            self.nmi_issued_this_vblank = true;
        }
        if !vblank {
            self.nmi_issued_this_vblank = false;
        }
        self.vblank_prev = vblank;

        // If CPU/SPC mailbox is idle, slowly advance the ACK byte so polling
        // loops waiting on $2140 can make forward progress in trace mode.
        if self.apu_io0_first_read_pending.get() {
            self.apu_idle_ticks = 0;
        } else {
            self.apu_idle_ticks = self.apu_idle_ticks.wrapping_add(1);
            if self.apu_idle_ticks >= Self::APU_IDLE_ACK_PERIOD {
                self.apu_idle_ticks = 0;
                self.apu_io0_ack.set(self.apu_io0_ack.get().wrapping_add(1));
            }
        }
    }

    /// Consume pending NMI request generated by timing model.
    pub fn take_pending_nmi(&mut self) -> bool {
        let pending = self.pending_nmi;
        self.pending_nmi = false;
        pending
    }

    /// Read a little-endian 16-bit word.
    pub fn read16(&self, bank: u8, addr: u16) -> u16 {
        let lo = self.read(bank, addr) as u16;
        let hi = self.read(bank, addr.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }

    /// Read a 24-bit value (for long address pointers).
    pub fn read24(&self, bank: u8, addr: u16) -> u32 {
        let lo = self.read(bank, addr) as u32;
        let mid = self.read(bank, addr.wrapping_add(1)) as u32;
        let hi = self.read(bank, addr.wrapping_add(2)) as u32;
        (hi << 16) | (mid << 8) | lo
    }

    /// Drain and return all accumulated DMA events.
    pub fn take_events(&mut self) -> Vec<DmaEvent> {
        std::mem::take(&mut self.events)
    }

    /// Drain and return all accumulated direct VRAM write events.
    pub fn take_vram_writes(&mut self) -> Vec<VramWriteEvent> {
        std::mem::take(&mut self.vram_writes)
    }

    /// Drain and return all accumulated unified trace events.
    pub fn take_trace_events(&mut self) -> Vec<TraceEvent> {
        std::mem::take(&mut self.trace_events)
    }

    /// Get next monotonic sequence number.
    pub fn next_seq(&mut self) -> u64 {
        self.event_seq += 1;
        self.event_seq
    }

    fn vram_increment_step(&self) -> u16 {
        match self.vram_incr & 0x03 {
            0 => 1,
            1 => 32,
            _ => 128,
        }
    }

    fn vram_increment_after_high(&self) -> bool {
        self.vram_incr & 0x80 != 0
    }

    fn maybe_increment_vram_addr(&mut self, wrote_high_port: bool) {
        let increment_now = if self.vram_increment_after_high() {
            wrote_high_port
        } else {
            !wrote_high_port
        };
        if increment_now {
            self.vram_addr = self.vram_addr.wrapping_add(self.vram_increment_step());
        }
    }

    fn rom_read(&self, bank: u8, addr: u16) -> u8 {
        if addr < 0x8000 {
            // LoROM banks $00-$3F/$80-$BF only map $8000-$FFFF to ROM.
            // Callers may pass lower addresses as a fallback; return 0.
            return 0;
        }
        let pc = lorom_to_pc(bank, addr);
        if pc < self.rom.len() {
            self.rom[pc]
        } else {
            0
        }
    }

    fn io_read(&self, addr: u16) -> u8 {
        match addr {
            0x4210 => {
                let pending = self.nmi_latch.get();
                self.nmi_latch.set(false);
                if pending {
                    0x82
                } else {
                    0x02
                }
            }
            0x4212 => self.hvbjoy,
            0x4016 => {
                if self.joypad_strobe.get() {
                    (self.joypad1 & 0x0001) as u8
                } else {
                    let shift = self.joypad1_shift.get();
                    let bit = (shift & 0x0001) as u8;
                    self.joypad1_shift.set((shift >> 1) | 0x8000);
                    bit
                }
            }
            0x4017 => 0x00,
            0x2140 => {
                if self.apu_io0_first_read_pending.get() {
                    self.apu_io0_first_read_pending.set(false);
                    self.apu_io[0]
                } else {
                    self.apu_io0_ack.get()
                }
            }
            0x2141..=0x2143 => self.apu_io[(addr - 0x2140) as usize],
            // Auto-joypad read results
            0x4218 => self.joypad1 as u8,        // JOY1L
            0x4219 => (self.joypad1 >> 8) as u8, // JOY1H
            0x421A..=0x421F => 0x00,             // JOY2-4
            0x4300..=0x437F => {
                let ch = ((addr - 0x4300) >> 4) as usize;
                let reg = (addr & 0x0F) as usize;
                if ch < 8 {
                    self.dma_read(ch, reg)
                } else {
                    0
                }
            }
            _ => 0x00,
        }
    }

    fn dma_read(&self, ch: usize, reg: usize) -> u8 {
        let d = &self.dma[ch];
        match reg {
            0 => d.control,
            1 => d.dest,
            2 => d.src_addr as u8,
            3 => (d.src_addr >> 8) as u8,
            4 => d.src_bank,
            5 => d.size as u8,
            6 => (d.size >> 8) as u8,
            7 => d.indirect_bank,
            _ => 0,
        }
    }

    fn io_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x2100 => self.inidisp = val,
            // $2102/$2103: OAM address
            0x2102 => self.oam_addr = (self.oam_addr & 0xFF00) | val as u16,
            0x2103 => {
                self.oam_addr = (self.oam_addr & 0x00FF) | (((val as u16) & 0x01) << 8);
            }
            // $2104: OAM data write
            0x2104 => {
                let idx = self.oam_addr as usize;
                if idx < self.oam.len() {
                    self.oam[idx] = val;
                }
                self.oam_addr = self.oam_addr.wrapping_add(1);
            }
            0x2115 => self.vram_incr = val,
            0x2116 => self.vram_addr = (self.vram_addr & 0xFF00) | val as u16,
            0x2117 => self.vram_addr = (self.vram_addr & 0x00FF) | ((val as u16) << 8),
            0x2118 => {
                // Write low byte to VRAM shadow
                let byte_addr = (self.vram_addr as usize) * 2;
                if byte_addr < self.vram.len() {
                    self.vram[byte_addr] = val;
                }
                let seq = self.next_seq();
                self.vram_writes.push(VramWriteEvent {
                    port: 0x18,
                    vram_addr: self.vram_addr,
                    value: val,
                    pc_bank: self.current_pb,
                    pc_addr: self.current_pc,
                });
                self.trace_events.push(TraceEvent::VramWrite {
                    port: 0x18,
                    vram_addr: self.vram_addr,
                    value: val,
                    pc_bank: self.current_pb,
                    pc_addr: self.current_pc,
                    seq,
                });
                self.maybe_increment_vram_addr(false);
            }
            0x2119 => {
                // Write high byte to VRAM shadow
                let byte_addr = (self.vram_addr as usize) * 2 + 1;
                if byte_addr < self.vram.len() {
                    self.vram[byte_addr] = val;
                }
                let seq = self.next_seq();
                self.vram_writes.push(VramWriteEvent {
                    port: 0x19,
                    vram_addr: self.vram_addr,
                    value: val,
                    pc_bank: self.current_pb,
                    pc_addr: self.current_pc,
                });
                self.trace_events.push(TraceEvent::VramWrite {
                    port: 0x19,
                    vram_addr: self.vram_addr,
                    value: val,
                    pc_bank: self.current_pb,
                    pc_addr: self.current_pc,
                    seq,
                });
                self.maybe_increment_vram_addr(true);
            }
            // $2121: CGRAM address
            0x2121 => {
                self.cgram_addr = val as u16;
                self.cgram_flipflop = false;
            }
            // $2122: CGRAM data write (alternates low/high byte)
            0x2122 => {
                let idx = (self.cgram_addr as usize) * 2 + (self.cgram_flipflop as usize);
                if idx < self.cgram.len() {
                    self.cgram[idx] = val;
                }
                if self.cgram_flipflop {
                    self.cgram_addr = (self.cgram_addr + 1) & 0xFF;
                }
                self.cgram_flipflop = !self.cgram_flipflop;
            }
            0x2140 => {
                self.apu_io[0] = val;
                self.apu_io0_ack.set(val.wrapping_add(1));
                self.apu_io0_first_read_pending.set(true);
                self.apu_idle_ticks = 0;
            }
            0x2141..=0x2143 => {
                self.apu_io[(addr - 0x2140) as usize] = val;
            }
            0x2180 => {
                // WRAM data write through $2180
                let wa = self.wram_addr as usize;
                if wa < self.wram.len() {
                    self.wram[wa] = val;
                }
                self.wram_addr = (self.wram_addr + 1) & 0x1FFFF;
            }
            0x2181 => self.wram_addr = (self.wram_addr & 0x1FF00) | val as u32,
            0x2182 => self.wram_addr = (self.wram_addr & 0x100FF) | ((val as u32) << 8),
            0x2183 => self.wram_addr = (self.wram_addr & 0x0FFFF) | (((val as u32) & 1) << 16),
            0x4016 => {
                let prev_strobe = self.joypad_strobe.get();
                let new_strobe = val & 1 != 0;
                self.joypad_strobe.set(new_strobe);
                if new_strobe || prev_strobe {
                    self.joypad1_shift.set(self.joypad1);
                }
            }
            0x4200 => {
                let prev = self.nmitimen;
                self.nmitimen = val;
                // Enabling NMI during vblank triggers an immediate pending NMI.
                if (prev & 0x80) == 0
                    && (val & 0x80) != 0
                    && (self.hvbjoy & 0x80) != 0
                    && !self.nmi_issued_this_vblank
                {
                    self.pending_nmi = true;
                    self.nmi_latch.set(true);
                    self.nmi_issued_this_vblank = true;
                }
            }
            0x420B => self.trigger_dma(val),
            0x4300..=0x437F => {
                let ch = ((addr - 0x4300) >> 4) as usize;
                let reg = (addr & 0x0F) as usize;
                if ch < 8 {
                    self.dma_write(ch, reg, val);
                }
            }
            _ => {} // silently ignore
        }
    }

    fn dma_write(&mut self, ch: usize, reg: usize, val: u8) {
        let d = &mut self.dma[ch];
        match reg {
            0 => d.control = val,
            1 => d.dest = val,
            2 => d.src_addr = (d.src_addr & 0xFF00) | val as u16,
            3 => d.src_addr = (d.src_addr & 0x00FF) | ((val as u16) << 8),
            4 => d.src_bank = val,
            5 => d.size = (d.size & 0xFF00) | val as u16,
            6 => d.size = (d.size & 0x00FF) | ((val as u16) << 8),
            7 => d.indirect_bank = val,
            _ => {}
        }
    }

    fn trigger_dma(&mut self, enable_mask: u8) {
        for i in 0..8u8 {
            if enable_mask & (1 << i) == 0 {
                continue;
            }
            // Copy DMA channel fields to avoid borrow conflict.
            let src_bank = self.dma[i as usize].src_bank;
            let mut src_addr = self.dma[i as usize].src_addr;
            let dest_reg = self.dma[i as usize].dest;
            let control = self.dma[i as usize].control;
            let raw_size = self.dma[i as usize].size;
            let size = if raw_size == 0 {
                0x10000u32
            } else {
                raw_size as u32
            };

            let seq = self.next_seq();
            self.events.push(DmaEvent {
                channel: i,
                src_bank,
                src_addr,
                dest_reg,
                vram_addr: self.vram_addr,
                size: raw_size,
                pc_bank: self.current_pb,
                pc_addr: self.current_pc,
            });
            self.trace_events.push(TraceEvent::DmaStart {
                channel: i,
                src_bank,
                src_addr,
                dest_reg,
                vram_addr: self.vram_addr,
                size: raw_size,
                pc_bank: self.current_pb,
                pc_addr: self.current_pc,
                seq,
            });

            // Perform actual byte copy based on transfer mode.
            let transfer_mode = control & 0x07;
            let direction = control & 0x80 != 0; // true = PPU→CPU (read from PPU)
            let addr_mode = control & 0x08 != 0; // true = fixed source address

            if direction {
                // PPU→CPU: not commonly needed for trace, skip actual copy.
            } else {
                // CPU→PPU: perform actual memory copy.
                for byte_idx in 0..size {
                    let val = self.read_for_dma(src_bank, src_addr);

                    // Determine which B-bus register to write based on transfer mode.
                    let reg_offset = match transfer_mode {
                        0 => 0u8,                             // 1-register (p)
                        1 => (byte_idx & 1) as u8,            // 2-register (p, p+1)
                        2 | 6 => 0,                           // 1-register write-twice
                        3 | 7 => ((byte_idx >> 1) & 1) as u8, // 2-register write-twice
                        4 => (byte_idx & 3) as u8,            // 4-register (p..p+3)
                        5 => {
                            let sub = byte_idx & 3;
                            match sub {
                                0 => 0,
                                1 => 1,
                                2 => 0,
                                _ => 1,
                            } // (p,p+1,p,p+1)
                        }
                        _ => 0,
                    };
                    let target_reg = dest_reg.wrapping_add(reg_offset);
                    self.dma_write_to_ppu(target_reg, val);

                    if !addr_mode {
                        src_addr = src_addr.wrapping_add(1);
                    }
                }
            }

            let done_seq = self.next_seq();
            self.trace_events.push(TraceEvent::DmaDone {
                channel: i,
                seq: done_seq,
            });
        }
    }

    /// Read a byte for DMA source (ROM or WRAM).
    fn read_for_dma(&self, bank: u8, addr: u16) -> u8 {
        match bank {
            0x00..=0x3F | 0x80..=0xBF => match addr {
                0x0000..=0x1FFF => self.wram[addr as usize],
                0x6000..=0x7FFF => 0,
                _ => self.rom_read(bank, addr),
            },
            0x7E => self.wram[addr as usize],
            0x7F => self.wram[0x10000 + addr as usize],
            0x40..=0x6F | 0xC0..=0xFF => self.rom_read(bank, addr),
            _ => 0,
        }
    }

    /// Write a byte to a PPU B-bus register during DMA.
    fn dma_write_to_ppu(&mut self, reg: u8, val: u8) {
        match reg {
            0x04 => {
                // OAM data
                let idx = self.oam_addr as usize;
                if idx < self.oam.len() {
                    self.oam[idx] = val;
                }
                self.oam_addr = self.oam_addr.wrapping_add(1);
            }
            0x18 => {
                // VRAM low byte
                let byte_addr = (self.vram_addr as usize) * 2;
                if byte_addr < self.vram.len() {
                    self.vram[byte_addr] = val;
                }
                self.maybe_increment_vram_addr(false);
            }
            0x19 => {
                // VRAM high byte
                let byte_addr = (self.vram_addr as usize) * 2 + 1;
                if byte_addr < self.vram.len() {
                    self.vram[byte_addr] = val;
                }
                self.maybe_increment_vram_addr(true);
            }
            0x22 => {
                // CGRAM data
                let idx = (self.cgram_addr as usize) * 2 + (self.cgram_flipflop as usize);
                if idx < self.cgram.len() {
                    self.cgram[idx] = val;
                }
                if self.cgram_flipflop {
                    self.cgram_addr = (self.cgram_addr + 1) & 0xFF;
                }
                self.cgram_flipflop = !self.cgram_flipflop;
            }
            0x80 => {
                // WRAM via $2180
                let wa = self.wram_addr as usize;
                if wa < self.wram.len() {
                    self.wram[wa] = val;
                }
                self.wram_addr = (self.wram_addr + 1) & 0x1FFFF;
            }
            _ => {} // other registers: silently ignore
        }
    }
}

impl BusAccess for MemoryBus {
    fn read(&self, bank: u8, addr: u16) -> u8 {
        match bank {
            0x00..=0x3F | 0x80..=0xBF => match addr {
                0x0000..=0x1FFF => self.wram[addr as usize],
                0x2000..=0x5FFF => self.io_read(addr),
                0x6000..=0x7FFF => 0,
                _ => self.rom_read(bank, addr),
            },
            0x7E => self.wram[addr as usize],
            0x7F => self.wram[0x10000 + addr as usize],
            0x40..=0x6F | 0xC0..=0xFF => self.rom_read(bank, addr),
            _ => 0,
        }
    }

    fn write(&mut self, bank: u8, addr: u16, val: u8) {
        match bank {
            0x00..=0x3F | 0x80..=0xBF => match addr {
                0x0000..=0x1FFF => self.wram[addr as usize] = val,
                0x2000..=0x5FFF => self.io_write(addr, val),
                _ => {}
            },
            0x7E => self.wram[addr as usize] = val,
            0x7F => self.wram[0x10000 + addr as usize] = val,
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "bus_tests.rs"]
mod tests;
