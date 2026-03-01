//! World map place name localization (Hook #14 equivalent).
//!
//! The game's world map displays place names as LZ-compressed 8x8 graphic tiles,
//! not through the text engine. Bank $10's map engine handler calls `JSL $009440`
//! (LZ decompressor) at $10:$B562. The A register holds the low byte of dp$0C
//! (the LZ source address within the source bank).
//!
//! This module intercepts that call to provide uncompressed KO tile data via DMA,
//! bypassing the LZ decompressor for specific tilesets that contain place names.
//!
//! ## JP LZ blocks (identified via Phase A research)
//!
//! | Condition | dp$0C low | JP source       | Decomp size | WRAM dest    |
//! |-----------|-----------|-----------------|-------------|--------------|
//! | A         | $8C       | $11:$818C       | $4000 (16K) | $7F:$0000    |
//! | B         | $3C       | $12:$D83C       | $2000 (8K)  | $7F:$0000    |
//! | C         | $74       | $11:$8774       | $0CF0 (3K)  | $7F:$0000    |
//! | D         | $CD       | $11:$9ACD idx8  | $2000 (8K)  | $7F:$2000    |
//! | E         | $F0       | $11:$85F0       | $0800 (2K)  | $7F:$F000    |
//!
//! Blocks A/B/C share WRAM $7F:$0000 (scene-dependent, not simultaneous).
//! Block D loads to $7F:$2000 alongside A/B/C.
//! Block E has only 1 tile difference between JP/EN — skip for KO.

use crate::patch::asm::{assemble, Inst};
use crate::patch::font;
use crate::patch::tracked_rom::{Expect, TrackedRom};
use crate::rom::lorom_to_pc;

pub mod consts {
    // ── Hook site 1: LZ intercept ($10:$B562) ────────────────────────
    /// $10:$B562: JSL $009440 → JSL flag_clear_hook
    /// During title: clears WRAM flag + passthrough to $009440 (JP data).
    pub const HOOK_CALL_SITE_PC: usize = 0x83562; // lorom_to_pc(0x10, 0xB562)
    /// Original LZ decompressor entry
    pub const LZ_DECOMPRESS: u32 = 0x00_9440;

    // ── Hook site 2: worldmap handler ($03:$CC56) ────────────────────
    /// $03:$CC56: STZ $1A65; PHX → JML ko_loader
    /// First worldmap frame: DMA KO blocks to WRAM, set flag.
    pub const LOADER_CALL_SITE_PC: usize = 0x1CC56; // lorom_to_pc(0x03, 0xCC56)
    /// Original 4 bytes at $03:$CC56 that get relocated into the hook.
    pub const LOADER_ORIG_BYTES: [u8; 4] = [0x9C, 0x65, 0x1A, 0xDA]; // STZ $1A65; PHX
    /// Return address: instruction after the patched 4 bytes ($03:$CC5A).
    /// JML back here after executing relocated bytes + optional DMA.
    pub const LOADER_RETURN_ADDR: u32 = 0x03_CC5A;

    // ── Hook code placement ──────────────────────────────────────────
    /// Bank $10:$D000+ is free space (~13KB from $CC0F).
    pub const HOOK_CODE_BANK: u8 = 0x10;
    /// Flag-clear hook at $D000 (~15 bytes).
    pub const HOOK_CODE_ADDR: u16 = 0xD000;
    pub const HOOK_CODE_PC: usize = 0x85000; // lorom_to_pc(0x10, 0xD000)
    /// KO loader hook at $D020 (~300 bytes). After flag-clear code.
    pub const LOADER_CODE_ADDR: u16 = 0xD020;
    pub const LOADER_CODE_PC: usize = 0x85020; // lorom_to_pc(0x10, 0xD020)

    // ── WRAM flag ────────────────────────────────────────────────────
    /// $7F:$FF00 — KO data loaded flag. 0=needs load, nonzero=loaded.
    /// High address in Bank $7F, outside game's normal variable range.
    pub const KO_LOADED_FLAG: u32 = 0x7F_FF00;

    // ── KO tile data placement ───────────────────────────────────────
    // Total decompressed data = $8D00 (36KB) > one LoROM bank (32KB).
    // Split: blocks A/B/C in Bank $0B, block D in Bank $10 after hook code.
    //
    // Bank $0B is all-FF in JP ROM (32KB free).

    /// Primary data bank: Bank $0B is fully free in JP ROM (32KB).
    pub const DATA_BANK: u8 = 0x0B;
    pub const DATA_BASE_ADDR: u16 = 0x8000;
    pub const DATA_BANK_CAPACITY: u16 = 0x8000; // 32KB

    /// Overflow data in Bank $10, after loader code.
    /// Loader code is ~300 bytes at $D020; data starts at $D200 (safe margin).
    pub const DATA_BANK2: u8 = 0x10;
    pub const DATA_BANK2_ADDR: u16 = 0xD200;
}
use consts::*;

// ── Condition parameters ─────────────────────────────────────────
/// LZ block condition: low byte of dp$0C, DMA size, WRAM destination.
struct LzCondition {
    /// Low byte of dp$0C that triggers this condition
    low_byte: u8,
    /// JP LZ source: (bank, SNES address)
    jp_source: (u8, u16),
    /// DMA transfer size in bytes
    dma_size: u16,
    /// WRAM destination: (low, mid, high) for registers $2181/$2182/$2183
    wram_dest: (u8, u8, u8),
}

/// The 5 LZ block conditions from Hook #14.
/// Block E ($F0) is excluded — only 1 tile differs, not worth hooking.
const CONDITIONS: &[LzCondition] = &[
    LzCondition {
        low_byte: 0x8C,
        jp_source: (0x11, 0x818C),
        dma_size: 0x4000,
        wram_dest: (0x00, 0x00, 0x01), // WRAM $10000 = $7F:$0000
    },
    LzCondition {
        low_byte: 0x3C,
        jp_source: (0x12, 0xD83C),
        dma_size: 0x1FC0,              // actual JP LZ decompressed size (not $2000)
        wram_dest: (0x00, 0x00, 0x01), // WRAM $10000 = $7F:$0000
    },
    LzCondition {
        low_byte: 0x74,
        jp_source: (0x11, 0x8774),
        dma_size: 0x0CF0,              // actual JP LZ decompressed size (not $0D00)
        wram_dest: (0x00, 0x00, 0x01), // WRAM $10000 = $7F:$0000
    },
    LzCondition {
        low_byte: 0xCD,
        jp_source: (0x11, 0x9ACD),
        dma_size: 0x2000,
        wram_dest: (0x00, 0x20, 0x01), // WRAM $12000 = $7F:$2000
    },
];

/// Decompress all JP LZ blocks for the world map conditions.
/// Returns a Vec of (condition_index, decompressed_data).
fn decompress_jp_blocks(rom: &[u8]) -> Result<Vec<(usize, Vec<u8>)>, String> {
    let mut results = Vec::new();
    for (i, cond) in CONDITIONS.iter().enumerate() {
        let pc = lorom_to_pc(cond.jp_source.0, cond.jp_source.1);
        let (data, _consumed) = font::decompress_lz(rom, pc)?;
        // Pad or truncate to DMA size
        let mut padded = data;
        padded.resize(cond.dma_size as usize, 0x00);
        results.push((i, padded));
    }
    Ok(results)
}

/// Build the flag-clear hook at $10:$D000 (Hook site 1: LZ intercept).
///
/// Every LZ decompressor call at $10:$B562 passes through this hook.
/// It clears the WRAM "KO loaded" flag so the next worldmap frame
/// will re-DMA the KO tile data.  Then it calls the original $009440.
///
/// This ensures that whenever the title screen (or any other scene) triggers
/// the LZ decompressor, the WRAM flag resets — so the worldmap handler will
/// re-load KO data on its next entry.
#[allow(clippy::vec_init_then_push)]
fn build_hook_code() -> Result<Vec<u8>, String> {
    use Inst::*;
    let mut program: Vec<Inst> = Vec::new();

    // Clear KO-loaded flag (WRAM $7F:FF00 = 0)
    program.push(Sep(0x20)); // 8-bit A
    program.push(LdaImm8(0x00)); // A = 0
    program.push(StaLong(KO_LOADED_FLAG)); // STA $7F:FF00
                                           // Call original LZ decompressor (passthrough all LZ loads)
    program.push(Jsl(LZ_DECOMPRESS)); // JSL $009440
    program.push(Rtl);

    assemble(&program)
}

/// Build the KO loader hook at $10:$D020 (Hook site 2: worldmap handler).
///
/// Called every frame from $03:$CC56 via JML (not JSL — no stack push).
/// First executes the relocated original bytes (STZ $1A65; PHX), then checks
/// the WRAM flag.  On the first call after title/reset (flag=0), DMAs all 4
/// KO tile blocks from ROM to WRAM, then sets the flag.  Subsequent frames
/// hit the early-return path (~25 cycles overhead).
///
/// Uses JML instead of JSL/RTL because the relocated PHX modifies the stack.
/// If we used JSL, PHX would push X on top of the return address, and RTL
/// would pop X as the return address → crash.
///
/// `data_addrs` maps condition index → (bank, SNES address) of KO data in ROM.
#[allow(clippy::vec_init_then_push)]
fn build_loader_code(data_addrs: &[(u8, u16)]) -> Result<Vec<u8>, String> {
    use Inst::*;
    let mut program: Vec<Inst> = Vec::new();

    // Relocated original bytes from $03:$CC56 (STZ $1A65; PHX)
    program.push(StzAbs(0x1A65));
    program.push(Phx);

    // Check WRAM flag — early return if already loaded
    program.push(Sep(0x20)); // 8-bit A
    program.push(LdaLong(KO_LOADED_FLAG)); // LDA $7F:FF00
    program.push(Beq("needs_load")); // flag=0 → load KO data
    program.push(Jml(LOADER_RETURN_ADDR)); // flag≠0 → jump back to $03:CC5A

    program.push(Label("needs_load"));

    // DMA all 4 KO tile blocks from ROM → WRAM
    for (i, &(src_bank, src_addr)) in data_addrs.iter().enumerate() {
        emit_dma_block(&mut program, &CONDITIONS[i], (src_bank, src_addr));
    }

    // Set loaded flag
    program.push(LdaImm8(0x01));
    program.push(StaLong(KO_LOADED_FLAG)); // STA $7F:FF00

    program.push(Jml(LOADER_RETURN_ADDR)); // jump back to $03:CC5A

    assemble(&program)
}

/// Emit DMA ch5 register setup + trigger for one LZ block (no SEP/RTL).
///
/// Caller must ensure 8-bit A mode before calling.
fn emit_dma_block(program: &mut Vec<Inst>, cond: &LzCondition, src: (u8, u16)) {
    use Inst::*;
    let (src_bank, src_addr) = src;

    // $4350 = DMA params (0x00 = A→B, byte, increment)
    program.push(LdaImm8(0x00));
    program.push(StaAbs(0x4350));
    // $4351 = B-bus address ($80 = WMDATA $2180)
    program.push(LdaImm8(0x80));
    program.push(StaAbs(0x4351));
    // WRAM address registers
    program.push(LdaImm8(cond.wram_dest.0));
    program.push(StaAbs(0x2181));
    program.push(LdaImm8(cond.wram_dest.1));
    program.push(StaAbs(0x2182));
    program.push(LdaImm8(cond.wram_dest.2));
    program.push(StaAbs(0x2183));
    // DMA source address
    program.push(LdaImm8(src_addr as u8));
    program.push(StaAbs(0x4352));
    program.push(LdaImm8((src_addr >> 8) as u8));
    program.push(StaAbs(0x4353));
    program.push(LdaImm8(src_bank));
    program.push(StaAbs(0x4354));
    // DMA transfer size
    program.push(LdaImm8(cond.dma_size as u8));
    program.push(StaAbs(0x4355));
    program.push(LdaImm8((cond.dma_size >> 8) as u8));
    program.push(StaAbs(0x4356));
    // Trigger DMA ch5
    program.push(LdaImm8(0x20)); // bit 5 = channel 5
    program.push(StaAbs(0x420B));
}

/// Lay out KO tile data across Bank $33 (primary) and Bank $10 (overflow).
///
/// Blocks are placed sequentially in Bank $33. If a block would exceed
/// Bank $33's 32KB capacity, it spills into Bank $10 after the hook code.
fn layout_data(blocks: &[(usize, Vec<u8>)]) -> Result<Vec<(u8, u16, usize)>, String> {
    let mut result = Vec::new();
    let mut bank0b_offset = 0u16;
    let mut bank10_offset = 0u16;

    for (idx, data) in blocks {
        let len = data.len() as u16;
        if bank0b_offset + len <= DATA_BANK_CAPACITY {
            let addr = DATA_BASE_ADDR + bank0b_offset;
            result.push((DATA_BANK, addr, *idx));
            bank0b_offset += len;
        } else {
            let addr = DATA_BANK2_ADDR + bank10_offset;
            result.push((DATA_BANK2, addr, *idx));
            bank10_offset += len;
        }
    }

    // Verify Bank $10 overflow doesn't exceed bank boundary
    let bank10_end = DATA_BANK2_ADDR as u32 + bank10_offset as u32;
    if bank10_end > 0x10000 {
        return Err(format!(
            "World map overflow data exceeds Bank $10: end ${:04X}",
            bank10_end
        ));
    }

    Ok(result)
}

// ── Sky worldmap KO tile injection ──────────────────────────────

/// KO texts displayed on the sky squirrel worldmap (text pointer entries 38-64).
/// Only entries that appear as sky squirrel stops are listed.
/// Entries 42 (スケトウダラJr), 48-49, 57-59 are not rendered on the sky worldmap.
const SKY_PLACE_NAMES: &[&str] = &[
    "아르르의 집",     // 38
    "개구리 늪",       // 39
    "어둠의 숲",       // 40-41
    "빛의 숲",         // 43
    "마도유치원",      // 44
    "옛날마을",        // 45
    "유적마을",        // 46
    "대마왕의 유적",   // 47
    "전망 바위산",     // 50
    "죽음의 계곡",     // 51
    "꽃밭",            // 52
    "선인의 지하동굴", // 53, 55
    "하피의 산",       // 54
    "어둠의 우물",     // 56
    "적목의 미로",     // 60
    "선인의 산",       // 61
    "용의 묘지",       // 62
    "용의 신전",       // 63
    "비의 숲",         // 64
];

/// Collect unique KO characters from sky worldmap place names, excluding spaces.
pub fn sky_ko_chars() -> Vec<char> {
    let mut chars = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for &name in SKY_PLACE_NAMES {
        for ch in name.chars() {
            if ch != ' ' && seen.insert(ch) {
                chars.push(ch);
            }
        }
    }
    chars
}

/// Inject KO 8×8 tiles into decompressed CHR block data.
///
/// Each entry is (tile_index, 16-byte 2bpp tile). The tile replaces the JP tile
/// at offset `tile_index * 16`. Out-of-bounds entries are silently skipped
/// (Block C has only ~208 tiles vs Block B's 512).
///
/// Tile indices: single-byte $20-$EF map directly; FB-prefix tiles use $100+byte.
fn inject_ko_tiles(block_data: &mut [u8], ko_tiles: &[(u16, [u8; 16])]) {
    for &(tile_idx, ref tile) in ko_tiles {
        let offset = tile_idx as usize * 16;
        if offset + 16 <= block_data.len() {
            block_data[offset..offset + 16].copy_from_slice(tile);
        }
    }
}

/// Apply world map hook + data to the ROM.
///
/// Steps:
/// 1. Decompress JP LZ blocks
/// 2. Inject KO 8×8 tiles into Block B/C CHR data
/// 3. Write uncompressed data to Bank $33
/// 4. Build and write hook code to Bank $10:$D000
/// 5. Patch JSL at $10:$B562 to jump to our hook
pub fn apply_worldmap_hook(
    rom: &mut TrackedRom,
    ko_sky_tiles: &[(u16, [u8; 16])],
) -> Result<usize, String> {
    // Step 1: Decompress JP blocks
    let mut blocks = decompress_jp_blocks(rom)?;

    // Step 2: Inject KO tiles into Block B (cond 1) and Block C (cond 2)
    if !ko_sky_tiles.is_empty() {
        for (idx, data) in &mut blocks {
            if *idx == 1 || *idx == 2 {
                inject_ko_tiles(data, ko_sky_tiles);
            }
        }
        println!(
            "  Injected {} KO 8×8 tiles into Block B/C",
            ko_sky_tiles.len()
        );
    }

    // Step 3: Layout data across Bank $33 + Bank $10 overflow
    let layout = layout_data(&blocks)?;

    let total_size: usize = blocks.iter().map(|(_, d)| d.len()).sum();

    // Step 3: Build two hook codes
    let flag_clear_code = build_hook_code()?;
    let data_addrs: Vec<(u8, u16)> = layout.iter().map(|&(b, a, _)| (b, a)).collect();
    let loader_code = build_loader_code(&data_addrs)?;

    // Verify codes fit in their reserved areas
    let flag_clear_limit = (LOADER_CODE_ADDR - HOOK_CODE_ADDR) as usize;
    if flag_clear_code.len() > flag_clear_limit {
        return Err(format!(
            "Flag-clear hook {} bytes exceeds limit {} at ${:04X}",
            flag_clear_code.len(),
            flag_clear_limit,
            HOOK_CODE_ADDR
        ));
    }
    let loader_limit = (DATA_BANK2_ADDR - LOADER_CODE_ADDR) as usize;
    if loader_code.len() > loader_limit {
        return Err(format!(
            "Loader hook {} bytes exceeds limit {} at ${:04X}",
            loader_code.len(),
            loader_limit,
            LOADER_CODE_ADDR
        ));
    }

    // Step 4: Write tile data to ROM
    for ((_, data), &(bank, addr, idx)) in blocks.iter().zip(layout.iter()) {
        rom.write_snes(bank, addr, data, &format!("worldmap:sky_data_{}", idx));
    }

    // Step 5: Write hook codes to Bank $10
    rom.write_expect(
        HOOK_CODE_PC,
        &flag_clear_code,
        "worldmap:flag_clear_hook",
        &Expect::FreeSpace(0xFF),
    );
    rom.write_expect(
        LOADER_CODE_PC,
        &loader_code,
        "worldmap:loader_hook",
        &Expect::FreeSpace(0xFF),
    );

    // Step 6: Patch JSL at $10:$B562 → flag-clear hook ($10:$D000)
    let hook1_long: u32 = (HOOK_CODE_BANK as u32) << 16 | (HOOK_CODE_ADDR as u32);
    let jsl1 = [
        0x22u8,
        hook1_long as u8,
        (hook1_long >> 8) as u8,
        (hook1_long >> 16) as u8,
    ];
    rom.write_expect(
        HOOK_CALL_SITE_PC,
        &jsl1,
        "worldmap:sky_jsl_patch",
        &Expect::Bytes(&[0x22, 0x40, 0x94, 0x00]),
    );

    // Step 7: Patch $03:$CC56 → loader hook ($10:$D020) via JML (not JSL!)
    // JML avoids pushing a return address, which is critical because the hook
    // executes a relocated PHX that would corrupt the JSL return address.
    let hook2_long: u32 = (HOOK_CODE_BANK as u32) << 16 | (LOADER_CODE_ADDR as u32);
    let jml2 = [
        0x5Cu8, // JML opcode
        hook2_long as u8,
        (hook2_long >> 8) as u8,
        (hook2_long >> 16) as u8,
    ];
    rom.write_expect(
        LOADER_CALL_SITE_PC,
        &jml2,
        "worldmap:loader_jml_patch",
        &Expect::Bytes(&LOADER_ORIG_BYTES),
    );

    // Report layout
    for &(bank, addr, idx) in &layout {
        let cond = &CONDITIONS[idx];
        println!(
            "  Block {} (${:02X}): ${:02X}:${:04X} → WRAM ${:02X}{:02X}{:02X}, {} bytes",
            idx,
            cond.low_byte,
            bank,
            addr,
            cond.wram_dest.2,
            cond.wram_dest.1,
            cond.wram_dest.0,
            cond.dma_size
        );
    }
    println!(
        "  Flag-clear hook: {} bytes at ${:02X}:${:04X}",
        flag_clear_code.len(),
        HOOK_CODE_BANK,
        HOOK_CODE_ADDR
    );
    println!(
        "  Loader hook: {} bytes at ${:02X}:${:04X}",
        loader_code.len(),
        HOOK_CODE_BANK,
        LOADER_CODE_ADDR
    );
    println!("  Total tile data: {} bytes", total_size);
    println!(
        "  Patches: JSL ${:05X}→${:06X}, JML ${:05X}→${:06X}",
        HOOK_CALL_SITE_PC, hook1_long, LOADER_CALL_SITE_PC, hook2_long
    );

    Ok(CONDITIONS.len())
}

#[cfg(test)]
#[path = "worldmap_tests.rs"]
mod tests;

// ══════════════════════════════════════════════════════════════════
// Menu worldmap place name localization
// ══════════════════════════════════════════════════════════════════
//
// The menu worldmap ($03:$C3F0) is separate from the sky squirrel worldmap
// ($10:$B562). It loads 5 LZ blocks from Bank $25 via JSL $009440.
// Only Block 1 (CHR tiles) and Block 2 (tilemap) contain text data.
//
// This module intercepts the LZ decompressor calls at $03:$C3F0 to provide
// pre-built KO CHR and tilemap data via DMA.

pub mod menu_consts {
    use crate::rom::lorom_to_pc;

    // ── Hook site ────────────────────────────────────────────────────
    /// $03:$C3F0: JSL $009440 → JSL menu_hook
    pub const HOOK_SITE_PC: usize = 0x1C3F0; // lorom_to_pc(0x03, 0xC3F0)
    pub const LZ_DECOMPRESS: u32 = 0x00_9440;

    // ── JP LZ sources ────────────────────────────────────────────────
    pub const BLOCK1_LZ: (u8, u16) = (0x25, 0xB784); // CHR tiles
    pub const BLOCK2_LZ: (u8, u16) = (0x25, 0xB9E7); // Tilemap

    // ── Tile format ──────────────────────────────────────────────────
    pub const BYTES_PER_TILE: usize = 16; // 8x8 2bpp
    pub const TM_COLS: usize = 32;
    pub const TM_ROWS: usize = 56;
    pub const TM_SIZE: usize = TM_COLS * TM_ROWS * 2; // 3584

    // ── WRAM destinations ────────────────────────────────────────────
    /// dp$11=$60 → WRAM $7F:$6000
    pub const WRAM_CHR: (u8, u8, u8) = (0x00, 0x60, 0x01);
    /// dp$11=$70 → WRAM $7F:$7000
    pub const WRAM_TM: (u8, u8, u8) = (0x00, 0x70, 0x01);

    // ── Data placement: Bank $32 after engine hooks ──────────────────
    // Engine hooks end at $D659. Menu worldmap code+data follows at $D660.
    // Code (256B) + CHR (~1KB) + TM (~3.5KB) ≈ 5KB total.
    // MENU_RESERVE_END marks the upper bound; relocate.rs FREE_REGIONS
    // must start at or after this address to avoid collision.
    pub const MENU_DATA_BANK: u8 = 0x32;
    /// Bank end for bounds checking
    pub const MENU_BANK_END: u16 = 0xFFFF;
    /// 코드→CHR 간 고정 오프셋 (256B 코드 공간)
    pub const CODE_TO_CHR_OFFSET: u16 = 0x100;

    // ── Sky tilemap ──────────────────────────────────────────────────
    /// LZ source for sky BG3 tilemap: Bank $25:$AB82
    /// 1792 bytes (32×28×2), loaded to WRAM $7F:$7000 (same as WRAM_TM).
    pub const SKY_TM_LZ: (u8, u16) = (0x25, 0xAB82);

    // ── OBJ sprite title ("월드맵") ──────────────────────────────────
    /// LZ source for OBJ CHR: Bank $25:$B10C → WRAM $7F:$4000
    pub const BLOCK3_LZ: (u8, u16) = (0x25, 0xB10C);
    /// OBJ tile data placement in Bank $10 (after sky squirrel data)
    pub const OBJ_DATA_BANK: u8 = 0x10;
    pub const OBJ_DATA_ADDR: u16 = 0xF200;
    pub const OBJ_DATA_PC: usize = lorom_to_pc(OBJ_DATA_BANK, OBJ_DATA_ADDR);
    /// OBJ patch routine placement (after bubble data)
    pub const OBJ_CODE_ADDR: u16 = 0xF600;
    pub const OBJ_CODE_PC: usize = lorom_to_pc(OBJ_DATA_BANK, OBJ_CODE_ADDR);
    pub const OBJ_TITLE_CHARS: &[char] = &['월', '드', '맵'];

    // ── OBJ bubble text ("현위치", "목적지", "이것") ─────────────────
    /// Bubble text tile data placement (after OBJ title data at $F200+640=$F480)
    pub const BUBBLE_DATA_ADDR: u16 = 0xF480;
    pub const BUBBLE_DATA_PC: usize = lorom_to_pc(OBJ_DATA_BANK, BUBBLE_DATA_ADDR);
    pub const BUBBLE_KORE_CHARS: &[char] = &['이', '것'];
    pub const BUBBLE_IMAWA_CHARS: &[char] = &['현', '위', '치'];
    pub const BUBBLE_IKISAKI_CHARS: &[char] = &['목', '적', '지'];

    // ── Frame tile KO indices ────────────────────────────────────────
    pub const KO_BLANK: u8 = 0x00;
    pub const KO_CORNER: u8 = 0x01; // JP $01 — right-top corner
    pub const KO_HBAR: u8 = 0x02; // JP $02
    pub const KO_VWALL: u8 = 0x03; // JP $03, also JP $0F via V-flip
    pub const KO_DOWNPTR: u8 = 0x04; // JP $09
    pub const KO_INNER: u8 = 0x05; // JP $19
    pub const KO_RIGHTPTR: u8 = 0x06; // JP $22 — right-pointer
    pub const KO_CORNER_L: u8 = 0x07; // JP $34 — left-top corner (separate CHR)
    pub const KO_GLYPH_BASE: u8 = 0x08;
    pub const FRAME_TILE_COUNT: usize = 8;
}

/// JP tile index → KO tile index mapping for frame tiles.
/// Text tiles (indices $04-$35 except frame tiles) are cleared to blank.
const JP_TO_KO_FRAME: &[(u8, u8)] = &[
    (0x00, menu_consts::KO_BLANK),
    (0x01, menu_consts::KO_CORNER),
    (0x02, menu_consts::KO_HBAR),
    (0x03, menu_consts::KO_VWALL),
    (0x09, menu_consts::KO_DOWNPTR),
    (0x0F, menu_consts::KO_VWALL), // V-flip (attr preserved from JP)
    (0x19, menu_consts::KO_INNER),
    (0x22, menu_consts::KO_RIGHTPTR), // right-pointer restored
    (0x34, menu_consts::KO_CORNER_L), // separate left-top corner tile
];

/// JP frame tile indices (for fast lookup).
fn is_jp_frame_tile(idx: u8) -> bool {
    matches!(
        idx,
        0x00 | 0x01 | 0x02 | 0x03 | 0x09 | 0x0F | 0x19 | 0x22 | 0x34
    )
}

/// Map a JP tile index to a KO tile index. Non-frame tiles → blank.
fn remap_tile_index(jp_idx: u8) -> u8 {
    for &(jp, ko) in JP_TO_KO_FRAME {
        if jp == jp_idx {
            return ko;
        }
    }
    menu_consts::KO_BLANK // text tiles cleared
}

/// KO translations for each text group detected in the menu worldmap tilemap.
/// 25 entries — one per group returned by `find_text_groups()` on the JP tilemap.
/// Multi-row bubble names are split across their constituent groups.
/// Empty strings leave the group positions blank (used for row-2/3 overflow
/// when the full KO name fits in an earlier group of the same bubble).
///
/// Tilemap dump reference (JP ROM, `dump_menu_tilemap_text_groups` test):
///   Screen 2 (rows 0-27): town names in speech bubbles
///   Screen 3 (rows 28-55): region names with boundary lines
const MENU_GROUP_TEXTS: &[&str] = &[
    "비의 숲",           // G0  row=5  あめのもり
    "유적 마을",         // G1  row=6  いせきむら          — 띄어쓰기
    "개구리",            // G2  row=10 かえるの (split row 1)
    "대마왕의 유적",     // G3  row=10 ぞうだいまおうのいせき — 띄어쓰기
    "연못",              // G4  row=11 いけ (split row 2)
    "옛날 마을",         // G5  row=14 むかしむら          — 띄어쓰기
    "",                  // G6  row=14 (blank — 선인의 산 merged to G8)
    "어둠의 우물",       // G7  row=14 やみのいど          — 띄어쓰기 (6ch→확장)
    "선인의 산",         // G8  row=15 3행→2행 병합 (2번째줄부터)
    "입구",              // G9  row=16 いりぐち
    "늑대 마을",         // G10 row=17 おおかみむら        — 띄어쓰기
    "마도유치원",        // G11 row=18 ようちえん          — 띄어쓰기 없음
    "스케토우다라Jr 집", // G12 row=20 すけとうだらのいえ  — 띄어쓰기
    "사탄님의",          // G13 row=20 サタンさまの (2-row bubble: row 1)
    "별장",              // G14 row=21 べっぞう (2-row bubble: row 2)
    "마도 마을",         // G15 row=23 まどうむら          — 띄어쓰기
    "아르르의 집",       // G16 row=25 アルルのいえ
    "할머니의 집",       // G17 row=25 おばあちゃんのいえ
    "",                  // G18 row=29 ゜ (standalone dakuten — skip)
    "하피의 산",         // G19 row=30 ハーピーのやま
    "죽음계곡",          // G20 row=33 しのたに (4 slots)
    "선인의 산",         // G21 row=36 せんにんの
    "빛의 숲",           // G22 row=37 ひかりのもり
    "",                  // G23 row=38 やま (blank)
    "어둠의 숲",         // G24 row=47 やみのもり
];

/// Sky worldmap framed text box placement.
/// Each entry defines a rectangular bordered text box at an exact tilemap position.
/// Multi-row JP names are consolidated; positions adjusted for visual balance.
struct SkyFrame {
    text: &'static str,
    row: usize,
    col: usize,
    /// Absolute column for ↓DOWN pointer in bottom bar (replaces one HBAR).
    /// JP original has a down pointer on every box, pointing toward the map marker below.
    down_col: Option<usize>,
}

const SKY_FRAMES: &[SkyFrame] = &[
    SkyFrame {
        text: "유적마을",
        row: 4,
        col: 23,
        down_col: Some(23),
    }, // JP (5,23)
    SkyFrame {
        text: "선인의 산",
        row: 7,
        col: 13,
        down_col: Some(16),
    }, // JP (8,16)
    SkyFrame {
        text: "옛날마을",
        row: 7,
        col: 6,
        down_col: Some(8),
    }, // JP (8,8)
    SkyFrame {
        text: "늑대마을",
        row: 12,
        col: 22,
        down_col: Some(24),
    }, // JP (13,24)
    SkyFrame {
        text: "마도유치원",
        row: 14,
        col: 2,
        down_col: Some(3),
    }, // JP (15,3)
];

/// Expected number of text groups in the JP sky tilemap (validation).
const SKY_EXPECTED_GROUPS: usize = 7;

/// OBJ title characters for menu worldmap "월드맵" sprite overlay.
pub const OBJ_TITLE_CHARS: &[char] = menu_consts::OBJ_TITLE_CHARS;

/// Bubble text characters for sky worldmap OAM speech bubbles.
pub const BUBBLE_KORE_CHARS: &[char] = menu_consts::BUBBLE_KORE_CHARS;
pub const BUBBLE_IMAWA_CHARS: &[char] = menu_consts::BUBBLE_IMAWA_CHARS;
pub const BUBBLE_IKISAKI_CHARS: &[char] = menu_consts::BUBBLE_IKISAKI_CHARS;

/// Public accessor for menu worldmap KO characters (used by builder.rs).
pub fn menu_ko_chars() -> Vec<char> {
    collect_menu_ko_chars()
}

/// Collect unique KO characters from all menu place names, in order of first appearance.
fn collect_menu_ko_chars() -> Vec<char> {
    let mut chars = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for &name in MENU_GROUP_TEXTS {
        for ch in name.chars() {
            if ch != ' ' && seen.insert(ch) {
                chars.push(ch);
            }
        }
    }
    chars
}

/// Scan the JP tilemap for contiguous groups of text tiles within each row.
/// Returns groups as Vec of position lists, sorted by (row, col).
/// A "text tile" is any tile index NOT in the frame tile set and NOT blank ($00).
fn find_text_groups(tilemap: &[u8]) -> Vec<Vec<usize>> {
    let cols = menu_consts::TM_COLS;
    let rows = tilemap.len() / 2 / cols;
    let mut groups = Vec::new();

    for r in 0..rows {
        let mut col = 0;
        while col < cols {
            let entry_idx = r * cols + col;
            let tile = tilemap[entry_idx * 2];
            if !is_jp_frame_tile(tile) && tile != 0x00 {
                let mut group = vec![entry_idx];
                col += 1;
                while col < cols {
                    let idx2 = r * cols + col;
                    let tile2 = tilemap[idx2 * 2];
                    if !is_jp_frame_tile(tile2) && tile2 != 0x00 {
                        group.push(idx2);
                        col += 1;
                    } else {
                        break;
                    }
                }
                groups.push(group);
            } else {
                col += 1;
            }
        }
    }

    groups
}

// HBAR_OVERRIDES removed — all affected positions are in bubbles redrawn by adjust_menu_frames().

/// Remap tilemap: replace JP tile indices with KO indices, clear text tiles.
///
/// `tilemap` is the decompressed Block 2 data (3584 bytes = 32×56 × 2-byte entries).
/// Each entry is (tile_index, attribute_byte).
///
/// Pass 1: Scan JP tilemap for text tile groups (before modifying).
/// Pass 2: Remap all frame tile indices JP→KO, clear all non-frame tiles to blank.
/// Pass 3: Write centered KO glyph tile indices at each text group's positions.
/// Pass 4: Override specific positions from VWALL to HBAR.
fn remap_tilemap(tilemap: &mut [u8], ko_char_indices: &std::collections::HashMap<char, u8>) {
    // Pass 1: Find text groups from JP tilemap (must happen before clearing)
    let groups = find_text_groups(tilemap);

    // Collect text group positions into a set for quick lookup
    let text_positions: std::collections::HashSet<usize> =
        groups.iter().flat_map(|g| g.iter().copied()).collect();

    // Pass 2: Remap frame tiles, clear text tiles to INNER (opaque background)
    let entry_count = tilemap.len() / 2;
    for i in 0..entry_count {
        let tile_idx = tilemap[i * 2];
        if is_jp_frame_tile(tile_idx) {
            tilemap[i * 2] = remap_tile_index(tile_idx);
        } else if text_positions.contains(&i) {
            // Text tile inside speech bubble → opaque inner fill
            tilemap[i * 2] = menu_consts::KO_INNER;
        } else {
            tilemap[i * 2] = menu_consts::KO_BLANK;
        }
    }

    // Pass 3: Write KO glyph indices at text group positions
    let name_count = groups.len().min(MENU_GROUP_TEXTS.len());
    for (gi, group) in groups.iter().enumerate().take(name_count) {
        let ko_text = MENU_GROUP_TEXTS[gi];
        let ko_chars: Vec<char> = ko_text.chars().collect();
        let text_len = ko_chars.len();
        let avail = group.len();

        // Center KO text within available positions
        let start_offset = if avail > text_len {
            (avail - text_len) / 2
        } else {
            0
        };

        for (ci, &ch) in ko_chars.iter().enumerate() {
            let pos_idx = start_offset + ci;
            if pos_idx >= avail {
                break;
            }
            let entry_idx = group[pos_idx];
            if entry_idx * 2 + 1 < tilemap.len() {
                if let Some(&glyph_idx) = ko_char_indices.get(&ch) {
                    tilemap[entry_idx * 2] = menu_consts::KO_GLYPH_BASE + glyph_idx;
                }
            }
        }
    }

    if groups.len() != MENU_GROUP_TEXTS.len() {
        println!(
            "  WARNING: found {} text groups but expected {}",
            groups.len(),
            MENU_GROUP_TEXTS.len()
        );
    }
}

/// Write a tile entry (index + attribute) at the given (row, col) position.
fn set_tile(tilemap: &mut [u8], row: usize, col: usize, tile: u8, attr: u8) {
    let idx = (row * menu_consts::TM_COLS + col) * 2;
    if idx + 1 < tilemap.len() {
        tilemap[idx] = tile;
        tilemap[idx + 1] = attr;
    }
}

/// Draw a complete speech bubble: clear old area, draw frame, fill interior, write text, place pointers.
///
/// - `clear`: (top, left, bot, right) inclusive — area to erase before drawing
/// - `frame`: (top, left, bot, right) inclusive — corners/bars/walls
/// - `texts`: (row, text) — each text row centered within inner frame width
/// - `pointers`: (row, col, tile, attr) — direction markers, override frame tiles
fn draw_bubble(
    tilemap: &mut [u8],
    ko_char_indices: &std::collections::HashMap<char, u8>,
    clear: (usize, usize, usize, usize),
    frame: (usize, usize, usize, usize),
    texts: &[(usize, &str)],
    pointers: &[(usize, usize, u8, u8)],
) {
    const ATTR: u8 = 0x20;
    const HFLIP: u8 = 0x60;
    const VFLIP: u8 = 0xA0;
    const HVFLIP: u8 = 0xE0;

    let (ct, cl, cb, cr) = clear;
    let (ft, fl, fb, fr) = frame;

    // 1. Clear old region
    for r in ct..=cb {
        for c in cl..=cr {
            set_tile(tilemap, r, c, menu_consts::KO_BLANK, ATTR);
        }
    }

    // 2. Corners
    set_tile(tilemap, ft, fl, menu_consts::KO_CORNER, ATTR);
    set_tile(tilemap, ft, fr, menu_consts::KO_CORNER, HFLIP);
    set_tile(tilemap, fb, fl, menu_consts::KO_CORNER, VFLIP);
    set_tile(tilemap, fb, fr, menu_consts::KO_CORNER, HVFLIP);

    // 3. Top/bottom bars
    for c in (fl + 1)..fr {
        set_tile(tilemap, ft, c, menu_consts::KO_HBAR, ATTR);
        set_tile(tilemap, fb, c, menu_consts::KO_HBAR, VFLIP);
    }

    // 4. Side walls
    for r in (ft + 1)..fb {
        set_tile(tilemap, r, fl, menu_consts::KO_VWALL, ATTR);
        set_tile(tilemap, r, fr, menu_consts::KO_VWALL, HFLIP);
    }

    // 5. Fill interior with INNER
    for r in (ft + 1)..fb {
        for c in (fl + 1)..fr {
            set_tile(tilemap, r, c, menu_consts::KO_INNER, ATTR);
        }
    }

    // 6. Write centered text
    let inner_width = fr - fl - 1;
    for &(row, text) in texts {
        let chars: Vec<char> = text.chars().collect();
        let text_len = chars.len();
        let start_col = fl
            + 1
            + if inner_width > text_len {
                (inner_width - text_len) / 2
            } else {
                0
            };
        for (i, &ch) in chars.iter().enumerate() {
            let col = start_col + i;
            if col >= fr {
                break;
            }
            if ch == ' ' {
                continue; // space = INNER (already filled)
            }
            if let Some(&glyph) = ko_char_indices.get(&ch) {
                set_tile(tilemap, row, col, menu_consts::KO_GLYPH_BASE + glyph, ATTR);
            }
        }
    }

    // 7. Direction pointers (override frame tiles at specific positions)
    for &(row, col, tile, attr) in pointers {
        set_tile(tilemap, row, col, tile, attr);
    }
}

/// Post-process menu tilemap: redraw speech bubbles (screen 2, rows 0-27) that need
/// resizing for KO text. Direction pointers stay at their fixed JP positions;
/// text is placed relative to them; frames wrap around the result.
///
/// Called after `remap_tilemap()` which handles the basic JP→KO index remap.
/// Bubbles not listed here keep their remap_tilemap() output as-is.
fn adjust_menu_frames(tilemap: &mut [u8], ko_char_indices: &std::collections::HashMap<char, u8>) {
    use menu_consts::{KO_DOWNPTR, KO_RIGHTPTR};

    // B0: 비의 숲 — shrink left (4→3 inner → 4 chars tight)
    //     Pointer: (6,9) ↓DOWN
    draw_bubble(
        tilemap,
        ko_char_indices,
        (4, 4, 6, 10), // clear JP area
        (4, 5, 6, 10), // new frame (left wall moved from 4→5)
        &[(5, "비의 숲")],
        &[(6, 9, KO_DOWNPTR, 0x20)],
    );

    // B1: 개구리/연못 — shrink left (4→3 inner cols)
    //     Pointer: (9,5) ↑UP
    draw_bubble(
        tilemap,
        ko_char_indices,
        (9, 1, 12, 6), // clear JP area
        (9, 2, 12, 6), // new frame (left wall moved from 1→2)
        &[(10, "개구리"), (11, "연못")],
        &[(9, 5, KO_DOWNPTR, 0xA0)],
    );

    // B2: 대마왕의 유적 — shrink from 12→7 inner cols
    //     Pointer: (11,20) ↓DOWN
    draw_bubble(
        tilemap,
        ko_char_indices,
        (9, 17, 11, 31), // clear (union of old + new)
        (9, 17, 11, 25), // new frame
        &[(10, "대마왕의 유적")],
        &[(11, 20, KO_DOWNPTR, 0x20)],
    );

    // B3: 어둠의 우물 — expand left by 1 (5→6 inner cols)
    //     Pointer: (15,28) ↓DOWN
    draw_bubble(
        tilemap,
        ko_char_indices,
        (13, 24, 15, 31), // clear (col 24 was blank in JP)
        (13, 24, 15, 31), // new frame
        &[(14, "어둠의 우물")],
        &[(15, 28, KO_DOWNPTR, 0x20)],
    );

    // B4: 선인의 산/입구 — shrink top row (top 13→14, text stays at rows 15-16)
    //     Pointer: (16,16) →RIGHT on right wall of row 16
    draw_bubble(
        tilemap,
        ko_char_indices,
        (13, 10, 17, 16), // clear old JP frame (includes row 13)
        (14, 10, 17, 16), // frame: top moved down 1 row
        &[(15, "선인의 산"), (16, "입구")],
        &[(16, 16, KO_RIGHTPTR, 0x20)],
    );

    // B5: 늑대 마을 — shrink right (6→5 inner cols)
    //     Pointer: (16,20) ↑UP
    draw_bubble(
        tilemap,
        ko_char_indices,
        (16, 19, 18, 26), // clear JP area
        (16, 19, 18, 25), // new frame (right wall moved from 26→25)
        &[(17, "늑대 마을")],
        &[(16, 20, KO_DOWNPTR, 0xA0)],
    );

    // B6: 스케토우다라Jr 집 — expand left by 1 (9→10 inner cols)
    //     Pointer: (21,17) ↓DOWN
    draw_bubble(
        tilemap,
        ko_char_indices,
        (19, 9, 21, 20), // clear (col 9 was blank in JP)
        (19, 9, 21, 20), // new frame
        &[(20, "스케토우다라Jr 집")],
        &[(21, 17, KO_DOWNPTR, 0x20)],
    );

    // B7: 사탄님의/별장 — shrink right (6→4 inner cols)
    //     Pointer: (20,24) ←LEFT (RIGHTPTR + H+V flip)
    draw_bubble(
        tilemap,
        ko_char_indices,
        (19, 24, 22, 31), // clear JP area
        (19, 24, 22, 29), // new frame (right wall moved from 31→29)
        &[(20, "사탄님의"), (21, "별장")],
        &[(20, 24, KO_RIGHTPTR, 0xE0)],
    );

    // B8: 할머니의 집 — shrink right (9→6 inner cols)
    //     Pointer: (25,19) ←LEFT (RIGHTPTR + H-flip)
    draw_bubble(
        tilemap,
        ko_char_indices,
        (24, 19, 26, 29), // clear JP area
        (24, 19, 26, 26), // new frame (right wall moved from 29→26)
        &[(25, "할머니의 집")],
        &[(25, 19, KO_RIGHTPTR, 0x60)],
    );
}

/// Remap sky worldmap tilemap ($AB82): clear JP text, draw framed KO text boxes.
///
/// The JP sky tilemap places text directly on the map (no speech bubbles).
/// KO version adds rectangular frames around each place name for readability.
///
/// Pass 1: Validate JP tilemap structure (find_text_groups).
/// Pass 2: Remap JP frame tiles, clear all text tiles to transparent.
/// Pass 3: Draw framed text boxes at positions defined by SKY_FRAMES.
fn remap_sky_tilemap(tilemap: &mut [u8], ko_char_indices: &std::collections::HashMap<char, u8>) {
    let cols = menu_consts::TM_COLS;
    let total_rows = tilemap.len() / 2 / cols;

    // Pass 1: Validate JP tilemap structure
    let groups = find_text_groups(tilemap);
    if groups.len() != SKY_EXPECTED_GROUPS {
        println!(
            "  WARNING: sky tilemap has {} text groups but expected {}",
            groups.len(),
            SKY_EXPECTED_GROUPS
        );
    }

    // Pass 2: Clear entire tilemap to transparent (remove all JP residuals)
    let entry_count = tilemap.len() / 2;
    for i in 0..entry_count {
        tilemap[i * 2] = menu_consts::KO_BLANK;
        tilemap[i * 2 + 1] = 0x20;
    }

    // Pass 3: Draw framed text boxes
    const ATTR: u8 = 0x20; // priority=1, palette=0, no flip
    const VFLIP: u8 = 0xA0; // priority=1, V-flip

    for frame in SKY_FRAMES {
        let text_chars: Vec<char> = frame.text.chars().collect();
        let text_len = text_chars.len();
        let top = frame.row - 1;
        let bot = frame.row + 1;
        let left = frame.col - 1;
        let right = frame.col + text_len;

        if bot >= total_rows || right >= cols {
            continue;
        }

        // Corners — JP menu uses KO_CORNER ($01) only, with H/V flips
        const HFLIP: u8 = 0x60;
        const HVFLIP: u8 = 0xE0;
        set_tile(tilemap, top, left, menu_consts::KO_CORNER, ATTR); // TL: no flip
        set_tile(tilemap, top, right, menu_consts::KO_CORNER, HFLIP); // TR: H-flip
        set_tile(tilemap, bot, left, menu_consts::KO_CORNER, VFLIP); // BL: V-flip
        set_tile(tilemap, bot, right, menu_consts::KO_CORNER, HVFLIP); // BR: H+V flip

        // Horizontal bars (top and bottom), with optional ↓DOWN pointer in bottom bar
        for c in (left + 1)..right {
            set_tile(tilemap, top, c, menu_consts::KO_HBAR, ATTR);
            if frame.down_col == Some(c) {
                // DOWN pointer: no flip (points down naturally)
                set_tile(tilemap, bot, c, menu_consts::KO_DOWNPTR, ATTR);
            } else {
                set_tile(tilemap, bot, c, menu_consts::KO_HBAR, VFLIP);
            }
        }

        // Side walls
        set_tile(tilemap, frame.row, left, menu_consts::KO_VWALL, ATTR);
        set_tile(tilemap, frame.row, right, menu_consts::KO_VWALL, HFLIP); // right: H-flip

        // Inner fill (opaque background), then glyph overwrite
        for c in frame.col..(frame.col + text_len) {
            set_tile(tilemap, frame.row, c, menu_consts::KO_INNER, ATTR);
        }
        for (i, &ch) in text_chars.iter().enumerate() {
            if let Some(&glyph_idx) = ko_char_indices.get(&ch) {
                set_tile(
                    tilemap,
                    frame.row,
                    frame.col + i,
                    menu_consts::KO_GLYPH_BASE + glyph_idx,
                    ATTR,
                );
            }
            // Space character: no glyph in ko_char_indices → keeps KO_INNER fill
        }
    }
}

/// Build KO CHR tile data: 7 frame tiles (from JP) + KO glyph tiles.
///
/// `jp_chr` is the decompressed Block 1 (864 bytes).
/// `ko_glyph_tiles` are the rendered 8×8 outlined tiles from font_gen.
fn build_ko_chr(jp_chr: &[u8], ko_glyph_tiles: &[[u8; 16]]) -> Vec<u8> {
    let total_tiles = menu_consts::FRAME_TILE_COUNT + ko_glyph_tiles.len();
    let mut chr = vec![0u8; total_tiles * menu_consts::BYTES_PER_TILE];

    // Copy 8 JP frame tiles at their original indices
    let jp_frame_indices: [u8; 8] = [0x00, 0x01, 0x02, 0x03, 0x09, 0x19, 0x22, 0x34];
    for (ko_idx, &jp_idx) in jp_frame_indices.iter().enumerate() {
        let src_off = jp_idx as usize * menu_consts::BYTES_PER_TILE;
        let dst_off = ko_idx * menu_consts::BYTES_PER_TILE;
        if src_off + menu_consts::BYTES_PER_TILE <= jp_chr.len() {
            chr[dst_off..dst_off + menu_consts::BYTES_PER_TILE]
                .copy_from_slice(&jp_chr[src_off..src_off + menu_consts::BYTES_PER_TILE]);
        }
    }

    // Append KO glyph tiles
    for (i, tile) in ko_glyph_tiles.iter().enumerate() {
        let dst_off = (menu_consts::FRAME_TILE_COUNT + i) * menu_consts::BYTES_PER_TILE;
        chr[dst_off..dst_off + menu_consts::BYTES_PER_TILE].copy_from_slice(tile);
    }

    chr
}

// ── OBJ sprite title helpers ─────────────────────────────────────

/// Detect the foreground color index from a 4bpp OBJ tile.
///
/// Analyzes tile $0C (first text character tile of "ワールドマップ") to find
/// the most frequent non-zero color index. Returns the detected color or
/// fallback value 6 (the known OBJ palette index from ROM analysis).
fn detect_fg_color_4bpp(obj_data: &[u8]) -> u8 {
    let tile_offset = 0x0C * 32; // 4bpp = 32 bytes per tile
    if tile_offset + 32 > obj_data.len() {
        return 6;
    }
    let tile = &obj_data[tile_offset..tile_offset + 32];

    let mut counts = [0u32; 16];
    for r in 0..8usize {
        for c in 0..8usize {
            let bit = 7 - c;
            let bp0 = (tile[r * 2] >> bit) & 1;
            let bp1 = (tile[r * 2 + 1] >> bit) & 1;
            let bp2 = (tile[16 + r * 2] >> bit) & 1;
            let bp3 = (tile[16 + r * 2 + 1] >> bit) & 1;
            let color = bp0 | (bp1 << 1) | (bp2 << 2) | (bp3 << 3);
            if color != 0 {
                counts[color as usize] += 1;
            }
        }
    }

    let max_count = counts.iter().skip(1).max().copied().unwrap_or(0);
    if max_count == 0 {
        return 6; // no non-zero pixels found, fallback to known palette index
    }
    counts
        .iter()
        .enumerate()
        .skip(1)
        .max_by_key(|(_, &c)| c)
        .map(|(i, _)| i as u8)
        .unwrap_or(6)
}

/// Build 640 bytes of OBJ tile data for the "월드맵" title overlay.
///
/// Uses a composite 80×16 canvas (5 sprites) to render ONE unified rounded
/// rectangle background across all sprites, matching the JP original's style.
///
/// Layout (6 MVN groups):
///   [0..64]:     blank top (sprite 9 top)
///   [64..192]:   월-TL,TR + 드-TL,TR (sprites 10+11 top)
///   [192..256]:  blank bot (sprite 9 bottom)
///   [256..384]:  월-BL,BR + 드-BL,BR (sprites 10+11 bottom)
///   [384..512]:  맵-TL,TR + blank-TL,TR (sprites 12+13 top)
///   [512..640]:  맵-BL,BR + blank-BL,BR (sprites 12+13 bottom)
fn build_obj_tile_data(bitmaps: &[[bool; 256]], _fg_color: u8) -> Vec<u8> {
    const BG_FILL: u8 = 0x0F;
    const BG_BORDER: u8 = 0x0A;
    const TEXT_FILL: u8 = 0x0E;
    const TEXT_BORDER: u8 = 0x0C;

    // ── 1. Composite 80×16 canvas ──────────────────────────────────
    let mut canvas = [[0u8; 80]; 16];
    let mut text_map = [[false; 80]; 16];

    // Place character bitmaps: 월 at x=16, 드 at x=32, 맵 at x=48
    // Vertically center each glyph within the 16px height.
    for (ci, x_off) in [(0usize, 16usize), (1, 32), (2, 48)] {
        // Find vertical content bounds
        let mut min_y = 16usize;
        let mut max_y = 0usize;
        for y in 0..16usize {
            for x in 0..16usize {
                if bitmaps[ci][y * 16 + x] {
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
            }
        }
        let dy = if min_y <= max_y {
            (16i32 - (min_y + max_y + 1) as i32) / 2
        } else {
            0 // empty bitmap
        };
        for y in 0..16usize {
            for x in 0..16usize {
                if bitmaps[ci][y * 16 + x] {
                    let ny = (y as i32 + dy).clamp(0, 15) as usize;
                    text_map[ny][x_off + x] = true;
                }
            }
        }
    }

    // ── 2. Rounded rectangle background ────────────────────────────
    // Base range x=12..68 (inclusive), y=1..14 with chamfered corners.
    // Inset table: extra indent from [12..68] at each row.
    //   y=0,15 → outside rect
    //   y=1,14 → indent 2 → x=[14..66]
    //   y=2,13 → indent 1 → x=[13..67]
    //   y=3..12 → full     → x=[12..68]
    const RECT_X_MIN: usize = 12;
    const RECT_X_MAX: usize = 68; // inclusive
    const INSETS: [usize; 16] = [80, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 80];

    for (y, &inset) in INSETS.iter().enumerate() {
        let x_lo = RECT_X_MIN.saturating_add(inset);
        let x_hi = RECT_X_MAX.saturating_sub(inset);
        if x_lo <= x_hi && x_hi < 80 {
            canvas[y][x_lo..=x_hi].fill(BG_FILL);
        }
    }

    // ── 3. Text border (1px outline around text pixels) ────────────
    for y in 0..16i32 {
        for x in 0..80i32 {
            if !text_map[y as usize][x as usize] && canvas[y as usize][x as usize] != 0 {
                let has_text_neighbor = (-1..=1i32).any(|dy| {
                    (-1..=1i32).any(|dx| {
                        if dx == 0 && dy == 0 {
                            return false;
                        }
                        let nx = x + dx;
                        let ny = y + dy;
                        (0..80).contains(&nx)
                            && (0..16).contains(&ny)
                            && text_map[ny as usize][nx as usize]
                    })
                });
                if has_text_neighbor {
                    canvas[y as usize][x as usize] = TEXT_BORDER;
                }
            }
        }
    }

    // ── 4. Text fill ───────────────────────────────────────────────
    for y in 0..16usize {
        for x in 0..80usize {
            if text_map[y][x] {
                canvas[y][x] = TEXT_FILL;
            }
        }
    }

    // ── 5. Background border (bg_fill adjacent to transparent) ─────
    let snapshot = canvas;
    for y in 0..16i32 {
        for x in 0..80i32 {
            if snapshot[y as usize][x as usize] == BG_FILL {
                let near_transparent = (-1..=1i32).any(|dy| {
                    (-1..=1i32).any(|dx| {
                        if dx == 0 && dy == 0 {
                            return false;
                        }
                        let nx = x + dx;
                        let ny = y + dy;
                        if !(0..80).contains(&nx) || !(0..16).contains(&ny) {
                            true // out of bounds = transparent
                        } else {
                            snapshot[ny as usize][nx as usize] == 0
                        }
                    })
                });
                if near_transparent {
                    canvas[y as usize][x as usize] = BG_BORDER;
                }
            }
        }
    }

    // ── 6. Encode 4bpp tiles in MVN group order ────────────────────
    let encode_tile = |cx: usize, cy: usize| -> [u8; 32] {
        let mut tile = [0u8; 32];
        for r in 0..8usize {
            let mut bp = [0u8; 4];
            for c in 0..8usize {
                let ci = canvas[cy + r][cx + c];
                let bit = 1 << (7 - c);
                for (p, bp_val) in bp.iter_mut().enumerate() {
                    if ci & (1 << p) != 0 {
                        *bp_val |= bit;
                    }
                }
            }
            tile[r * 2] = bp[0];
            tile[r * 2 + 1] = bp[1];
            tile[16 + r * 2] = bp[2];
            tile[16 + r * 2 + 1] = bp[3];
        }
        tile
    };

    let mut data = Vec::with_capacity(640);

    // Group 1: Sprite9(blank) top — TL,TR
    data.extend_from_slice(&encode_tile(0, 0));
    data.extend_from_slice(&encode_tile(8, 0));
    // Group 2: Sprite10(월)+Sprite11(드) top
    data.extend_from_slice(&encode_tile(16, 0));
    data.extend_from_slice(&encode_tile(24, 0));
    data.extend_from_slice(&encode_tile(32, 0));
    data.extend_from_slice(&encode_tile(40, 0));
    // Group 3: Sprite9(blank) bottom
    data.extend_from_slice(&encode_tile(0, 8));
    data.extend_from_slice(&encode_tile(8, 8));
    // Group 4: Sprite10(월)+Sprite11(드) bottom
    data.extend_from_slice(&encode_tile(16, 8));
    data.extend_from_slice(&encode_tile(24, 8));
    data.extend_from_slice(&encode_tile(32, 8));
    data.extend_from_slice(&encode_tile(40, 8));
    // Group 5: Sprite12(맵)+Sprite13(blank) top
    data.extend_from_slice(&encode_tile(48, 0));
    data.extend_from_slice(&encode_tile(56, 0));
    data.extend_from_slice(&encode_tile(64, 0));
    data.extend_from_slice(&encode_tile(72, 0));
    // Group 6: Sprite12(맵)+Sprite13(blank) bottom
    data.extend_from_slice(&encode_tile(48, 8));
    data.extend_from_slice(&encode_tile(56, 8));
    data.extend_from_slice(&encode_tile(64, 8));
    data.extend_from_slice(&encode_tile(72, 8));

    data
}

/// Build 4bpp OBJ bubble text tiles for a single speech bubble row (32×8 px).
///
/// Takes 8×8 glyph bitmaps and renders them centered on a 4-tile-wide canvas
/// with BG_FILL background, BG_BORDER on left/right columns, and TEXT_FILL glyphs.
/// Returns 128 bytes (4 tiles × 32 bytes/tile in 4bpp).
fn build_bubble_text_tiles(bitmaps_8x8: &[[bool; 64]], canvas_width_tiles: usize) -> Vec<u8> {
    const BG_FILL: u8 = 0x0F;
    const BG_BORDER: u8 = 0x0A;
    const TEXT_FILL: u8 = 0x09;

    let canvas_w = canvas_width_tiles * 8; // 32 px
    let canvas_h = 8usize;

    // 1. Fill canvas with BG_FILL
    let mut canvas = vec![vec![BG_FILL; canvas_w]; canvas_h];

    // 2. Left/right border columns
    for row in canvas.iter_mut() {
        row[0] = BG_BORDER;
        row[canvas_w - 1] = BG_BORDER;
    }

    // 3. Center-place glyph bitmaps
    let glyph_count = bitmaps_8x8.len();
    let total_glyph_width = glyph_count * 8;
    let x_start = (canvas_w.saturating_sub(total_glyph_width)) / 2;

    for (gi, bitmap) in bitmaps_8x8.iter().enumerate() {
        let gx = x_start + gi * 8;
        for y in 0..8usize {
            for x in 0..8usize {
                if bitmap[y * 8 + x] && gx + x < canvas_w {
                    canvas[y][gx + x] = TEXT_FILL;
                }
            }
        }
    }

    // 4. Encode to 4bpp tiles
    let mut data = Vec::with_capacity(canvas_width_tiles * 32);
    for ti in 0..canvas_width_tiles {
        let cx = ti * 8;
        let mut tile = [0u8; 32];
        for r in 0..8usize {
            let mut bp = [0u8; 4];
            for c in 0..8usize {
                let ci = canvas[r][cx + c];
                let bit = 1 << (7 - c);
                for (p, bp_val) in bp.iter_mut().enumerate() {
                    if ci & (1 << p) != 0 {
                        *bp_val |= bit;
                    }
                }
            }
            tile[r * 2] = bp[0];
            tile[r * 2 + 1] = bp[1];
            tile[16 + r * 2] = bp[2];
            tile[16 + r * 2 + 1] = bp[3];
        }
        data.extend_from_slice(&tile);
    }

    data
}

/// Build OBJ patch routine: decompress original → MVN overwrite KO tiles.
///
/// The routine calls JSL $009440 first (normal LZ decompression to WRAM $7F:$4000),
/// then patches specific tile offsets with pre-rendered KO 4bpp data via 8 MVN blocks
/// (6 title + 2 bubble text).
#[allow(clippy::vec_init_then_push)]
fn build_obj_patch_code(data_addr: u16, data_bank: u8) -> Result<Vec<u8>, String> {
    use Inst::*;

    let mut program: Vec<Inst> = Vec::new();

    // Call original LZ decompressor
    program.push(Jsl(menu_consts::LZ_DECOMPRESS));

    // Save processor flags — the caller at $03:$C3F4 does PLX right after return,
    // so X flag size must match the PHX at $03:$C3DB. The decompressor preserves
    // flags; our MVN section must not alter them permanently.
    program.push(Php);

    // Save DBR (MVN modifies it)
    program.push(Phb);

    // 16-bit A + XY for MVN
    program.push(Rep(0x30));

    // 8 MVN groups: (rom_offset, wram_dest, size)
    // Groups 1-6: title "월드맵" tiles (640B at data_addr+0)
    // Groups 7-8: bubble text tiles (384B at data_addr+0x280)
    let groups: &[(u16, u16, u16)] = &[
        (0x000, 0x40C0, 64),  // title: blank top
        (0x040, 0x4180, 128), // title: 월+드 top
        (0x0C0, 0x42C0, 64),  // title: blank bot
        (0x100, 0x4380, 128), // title: 월+드 bot
        (0x180, 0x4580, 128), // title: 맵+blank top
        (0x200, 0x4780, 128), // title: 맵+blank bot
        (0x280, 0x4500, 128), // bubble: これ "이것" text row
        (0x300, 0x4C00, 256), // bubble: 今はここ "현위치" + 行き先 "목적지" text rows
    ];

    for &(rom_off, wram_dest, size) in groups {
        program.push(LdxImm16(data_addr.wrapping_add(rom_off)));
        program.push(LdyImm16(wram_dest));
        program.push(LdaImm16(size - 1));
        program.push(Mvn(0x7F, data_bank));
    }

    // Restore DBR and processor flags (matching decompressor's return state)
    program.push(Plb);
    program.push(Plp);
    program.push(Rtl);

    assemble(&program)
}

/// Build menu/sky worldmap hook assembly code.
///
/// The hook site at $03:$C3F0 is a **generic LZ loading subroutine** used by
/// multiple subsystems (menu worldmap, sky worldmap, encyclopedia, etc.). We must
/// check both dp$0B (source bank) and dp$0C:$0D (LZ source address) to avoid
/// intercepting unrelated LZ loads (e.g. encyclopedia data).
///
/// - dp$0B == $25 AND dp$0C:$0D == $B784 → DMA KO CHR from ROM to WRAM $7F:$6000
/// - dp$0B == $25 AND dp$0C:$0D == $B9E7 → DMA KO menu tilemap to WRAM $7F:$7000
/// - dp$0B == $25 AND dp$0C:$0D == $AB82 → DMA KO sky tilemap to WRAM $7F:$7000
/// - dp$0B == $25 AND dp$0C:$0D == $B10C → JSL OBJ patch-after-decompress
/// - else → passthrough JSL $009440
#[allow(clippy::vec_init_then_push)]
fn build_menu_hook_code(
    chr_addr: (u8, u16),
    chr_size: u16,
    tm_addr: (u8, u16),
    tm_size: u16,
    sky_tm: Option<((u8, u16), u16)>,
    obj_routine: Option<u32>,
) -> Result<Vec<u8>, String> {
    use Inst::*;

    let mut program: Vec<Inst> = Vec::new();

    // Interleaved compare-and-handle layout: each BNE only skips one handler
    // (~45 bytes), keeping all branches within ±127 range.
    //
    // Two passthrough copies: one right after the bank check (for bank mismatch),
    // one at the end (for address mismatch after all checks). This avoids any
    // long-distance branches.

    // Entry: check dp$0B (source bank) = $25
    program.push(Sep(0x20)); // 8-bit A
    program.push(LdaDp(0x0B));
    program.push(CmpImm8(menu_consts::BLOCK1_LZ.0)); // $25
    program.push(Beq("bank_ok"));
    // Bank mismatch → passthrough (nearby copy)
    program.push(Jsl(menu_consts::LZ_DECOMPRESS));
    program.push(Rtl);

    program.push(Label("bank_ok"));
    program.push(Rep(0x20)); // 16-bit A
    program.push(LdaDp(0x0C));

    // --- CHR block: $B784 → DMA KO CHR to WRAM $7F:$6000 ---
    program.push(CmpImm16(menu_consts::BLOCK1_LZ.1));
    program.push(Bne("check_tm"));
    program.push(Sep(0x20));
    build_menu_dma_handler(&mut program, chr_addr, chr_size, menu_consts::WRAM_CHR);

    // --- Menu tilemap block: $B9E7 → DMA KO tilemap to WRAM $7F:$7000 ---
    program.push(Label("check_tm"));
    program.push(CmpImm16(menu_consts::BLOCK2_LZ.1));
    program.push(Bne("check_sky"));
    program.push(Sep(0x20));
    build_menu_dma_handler(&mut program, tm_addr, tm_size, menu_consts::WRAM_TM);

    // --- Sky tilemap block: $AB82 → DMA KO sky tilemap to WRAM $7F:$7000 ---
    program.push(Label("check_sky"));
    if let Some((addr, size)) = sky_tm {
        program.push(CmpImm16(menu_consts::SKY_TM_LZ.1));
        program.push(Bne("check_obj"));
        program.push(Sep(0x20));
        build_menu_dma_handler(&mut program, addr, size, menu_consts::WRAM_TM);
        program.push(Label("check_obj"));
    }

    // --- OBJ block: $B10C → JSL patch-after-decompress ---
    if let Some(addr) = obj_routine {
        program.push(CmpImm16(menu_consts::BLOCK3_LZ.1));
        program.push(Bne("passthrough"));
        program.push(Sep(0x20));
        program.push(Jsl(addr));
        program.push(Rtl);
    }

    // Address mismatch → passthrough (end copy)
    program.push(Label("passthrough"));
    program.push(Sep(0x20));
    program.push(Jsl(menu_consts::LZ_DECOMPRESS));
    program.push(Rtl);

    assemble(&program)
}

/// Append a DMA ch5 handler for a menu worldmap block.
fn build_menu_dma_handler(
    program: &mut Vec<Inst>,
    src: (u8, u16),
    size: u16,
    wram_dest: (u8, u8, u8),
) {
    use Inst::*;
    let (src_bank, src_addr) = src;

    // $4350 = DMA params (0x00 = A→B, byte, increment)
    program.push(LdaImm8(0x00));
    program.push(StaAbs(0x4350));
    // $4351 = B-bus address ($80 = WMDATA $2180)
    program.push(LdaImm8(0x80));
    program.push(StaAbs(0x4351));
    // WRAM address registers
    program.push(LdaImm8(wram_dest.0));
    program.push(StaAbs(0x2181));
    program.push(LdaImm8(wram_dest.1));
    program.push(StaAbs(0x2182));
    program.push(LdaImm8(wram_dest.2));
    program.push(StaAbs(0x2183));
    // DMA source address
    program.push(LdaImm8(src_addr as u8));
    program.push(StaAbs(0x4352));
    program.push(LdaImm8((src_addr >> 8) as u8));
    program.push(StaAbs(0x4353));
    program.push(LdaImm8(src_bank));
    program.push(StaAbs(0x4354));
    // DMA transfer size
    program.push(LdaImm8(size as u8));
    program.push(StaAbs(0x4355));
    program.push(LdaImm8((size >> 8) as u8));
    program.push(StaAbs(0x4356));
    // Trigger DMA ch5
    program.push(LdaImm8(0x20)); // bit 5 = channel 5
    program.push(StaAbs(0x420B));
    program.push(Rtl);
}

/// Apply menu + sky worldmap hook + KO data to the ROM.
///
/// 1. Decompress JP Block 1 (CHR), Block 2 (menu tilemap), sky tilemap ($AB82)
/// 2. Build KO CHR: 8 frame tiles + rendered KO glyph tiles
/// 3. Build KO menu tilemap: remap frame indices, write KO text
/// 4. Build KO sky tilemap: remap text tiles to KO glyphs
/// 5. Write data to Bank $32 (code + CHR + menu TM + sky TM)
/// 6. Write hook code (intercepts $B784, $B9E7, $AB82, $B10C)
/// 7. Patch JSL at $03:$C3F0
pub fn apply_menu_worldmap_hook(
    rom: &mut TrackedRom,
    ko_glyph_tiles: &[[u8; 16]],
    obj_bitmaps: Option<&[[bool; 256]]>,
    bubble_bitmaps_8x8: Option<&[Vec<[bool; 64]>]>,
    menu_code_addr: u16,
) -> Result<u16, String> {
    use menu_consts::*;

    // Step 1: Decompress JP blocks
    let block1_pc = lorom_to_pc(BLOCK1_LZ.0, BLOCK1_LZ.1);
    let (jp_chr, _) = font::decompress_lz(rom, block1_pc)?;
    let block2_pc = lorom_to_pc(BLOCK2_LZ.0, BLOCK2_LZ.1);
    let (jp_tm, _) = font::decompress_lz(rom, block2_pc)?;
    let sky_tm_pc = lorom_to_pc(SKY_TM_LZ.0, SKY_TM_LZ.1);
    let (jp_sky_tm, _) = font::decompress_lz(rom, sky_tm_pc)?;

    println!(
        "  JP CHR: {} bytes, menu TM: {} bytes, sky TM: {} bytes",
        jp_chr.len(),
        jp_tm.len(),
        jp_sky_tm.len()
    );

    // Step 2: Build KO CHR
    let ko_chr = build_ko_chr(&jp_chr, ko_glyph_tiles);

    // Step 3: Build KO menu tilemap
    let ko_chars = collect_menu_ko_chars();
    let ko_char_indices: std::collections::HashMap<char, u8> = ko_chars
        .iter()
        .enumerate()
        .map(|(i, &ch)| (ch, i as u8))
        .collect();
    let mut ko_tm = jp_tm.clone();
    ko_tm.resize(TM_SIZE, 0x00);
    remap_tilemap(&mut ko_tm, &ko_char_indices);
    adjust_menu_frames(&mut ko_tm, &ko_char_indices);

    // Step 4: Build KO sky tilemap
    let mut ko_sky_tm = jp_sky_tm.clone();
    remap_sky_tilemap(&mut ko_sky_tm, &ko_char_indices);

    // Dynamic layout: code → CHR → menu TM → sky TM (all 16-byte aligned)
    let menu_chr_addr = menu_code_addr + CODE_TO_CHR_OFFSET;
    let menu_code_pc = lorom_to_pc(MENU_DATA_BANK, menu_code_addr);
    let menu_chr_pc = lorom_to_pc(MENU_DATA_BANK, menu_chr_addr);

    let ko_chr_len = ko_chr.len();
    let menu_tm_addr = menu_chr_addr + ((ko_chr_len as u16 + 15) & !15);
    let menu_tm_pc = lorom_to_pc(MENU_DATA_BANK, menu_tm_addr);
    let sky_tm_addr = menu_tm_addr + ((ko_tm.len() as u16 + 15) & !15);
    let sky_tm_pc = lorom_to_pc(MENU_DATA_BANK, sky_tm_addr);

    // Verify data fits in bank
    let sky_tm_end = sky_tm_addr as usize + ko_sky_tm.len();
    if sky_tm_end > MENU_BANK_END as usize + 1 {
        return Err(format!(
            "Worldmap data exceeds Bank ${:02X}: end=${:04X} > ${:04X}",
            MENU_DATA_BANK, sky_tm_end, MENU_BANK_END
        ));
    }
    let chr_end_pc = menu_chr_pc + ko_chr_len;
    let sky_tm_end_pc = sky_tm_pc + ko_sky_tm.len();
    if chr_end_pc > rom.len() || sky_tm_end_pc > rom.len() {
        return Err("Worldmap data exceeds ROM bounds".to_string());
    }

    // Step 5: Write BG3 data to ROM
    rom.write(menu_chr_pc, &ko_chr, "worldmap:menu_chr");
    rom.write(menu_tm_pc, &ko_tm, "worldmap:menu_tm");
    rom.write(sky_tm_pc, &ko_sky_tm, "worldmap:sky_tm");

    // Step 5b: OBJ sprite title + bubble text patching (patch-after-decompress)
    let obj_routine_addr = if let Some(bitmaps) = obj_bitmaps {
        let obj_pc = lorom_to_pc(BLOCK3_LZ.0, BLOCK3_LZ.1);
        let (obj_data, _) = font::decompress_lz(rom, obj_pc)?;
        let fg_color = detect_fg_color_4bpp(&obj_data);

        // Title tiles (640B at OBJ_DATA_PC)
        let obj_tile_data = build_obj_tile_data(bitmaps, fg_color);
        rom.write(OBJ_DATA_PC, &obj_tile_data, "worldmap:obj_data");

        // Bubble text tiles (384B at BUBBLE_DATA_PC)
        if let Some(bubble_groups) = bubble_bitmaps_8x8 {
            // Expected order: [これ "이것", 今はここ "현위치", 行き先 "목적지"]
            let mut bubble_data = Vec::with_capacity(384);
            for group in bubble_groups {
                bubble_data.extend_from_slice(&build_bubble_text_tiles(group, 4));
            }
            rom.write(BUBBLE_DATA_PC, &bubble_data, "worldmap:bubble_data");
            println!(
                "  Bubble tiles: {} bytes at ${:02X}:${:04X}",
                bubble_data.len(),
                OBJ_DATA_BANK,
                BUBBLE_DATA_ADDR
            );
        }

        // Patch code (at OBJ_CODE_PC, after bubble data)
        let obj_code = build_obj_patch_code(OBJ_DATA_ADDR, OBJ_DATA_BANK)?;
        rom.write(OBJ_CODE_PC, &obj_code, "worldmap:obj_code");

        let long_addr = (OBJ_DATA_BANK as u32) << 16 | OBJ_CODE_ADDR as u32;
        println!(
            "  OBJ tiles: {} bytes at ${:02X}:${:04X}, fg_color={}",
            obj_tile_data.len(),
            OBJ_DATA_BANK,
            OBJ_DATA_ADDR,
            fg_color
        );
        println!(
            "  OBJ code: {} bytes at ${:02X}:${:04X}",
            obj_code.len(),
            OBJ_DATA_BANK,
            OBJ_CODE_ADDR
        );

        Some(long_addr)
    } else {
        None
    };

    // Step 6: Build and write hook code
    let sky_tm_param = Some(((MENU_DATA_BANK, sky_tm_addr), ko_sky_tm.len() as u16));
    let hook_code = build_menu_hook_code(
        (MENU_DATA_BANK, menu_chr_addr),
        ko_chr_len as u16,
        (MENU_DATA_BANK, menu_tm_addr),
        ko_tm.len() as u16,
        sky_tm_param,
        obj_routine_addr,
    )?;

    let code_space = (menu_chr_addr - menu_code_addr) as usize;
    if hook_code.len() > code_space {
        return Err(format!(
            "Hook code {} bytes exceeds space {} bytes",
            hook_code.len(),
            code_space
        ));
    }
    rom.write_expect(
        menu_code_pc,
        &hook_code,
        "worldmap:menu_hook_code",
        &Expect::FreeSpace(0xFF),
    );

    // Step 7: Patch JSL at $03:$C3F0
    let hook_long: u32 = (MENU_DATA_BANK as u32) << 16 | (menu_code_addr as u32);
    let jsl = [
        0x22u8,
        hook_long as u8,
        (hook_long >> 8) as u8,
        (hook_long >> 16) as u8,
    ];
    rom.write_expect(
        HOOK_SITE_PC,
        &jsl,
        "worldmap:menu_jsl_patch",
        &Expect::Bytes(&[0x22, 0x40, 0x94, 0x00]),
    );

    println!(
        "  Hook code: {} bytes at ${:02X}:${:04X}",
        hook_code.len(),
        MENU_DATA_BANK,
        menu_code_addr
    );
    println!(
        "  KO CHR: {} bytes ({} tiles) at ${:02X}:${:04X}",
        ko_chr_len,
        ko_chr_len / BYTES_PER_TILE,
        MENU_DATA_BANK,
        menu_chr_addr
    );
    println!(
        "  KO menu TM: {} bytes at ${:02X}:${:04X}",
        ko_tm.len(),
        MENU_DATA_BANK,
        menu_tm_addr
    );
    println!(
        "  KO sky TM: {} bytes at ${:02X}:${:04X}",
        ko_sky_tm.len(),
        MENU_DATA_BANK,
        sky_tm_addr
    );
    println!("  KO glyphs: {} unique chars", ko_chars.len());
    println!(
        "  JSL patch at PC 0x{:05X}: → ${:06X}",
        HOOK_SITE_PC, hook_long
    );

    // Return align16 of sky TM end — next available address in Bank $32
    let data_end_aligned = ((sky_tm_end as u16) + 15) & !15;
    Ok(data_end_aligned)
}
