//! Encyclopedia monster name hooks for KO patch.
//!
//! The encyclopedia uses a dedicated text renderer at $03:C6EE that only
//! handles single-byte and FB-prefix characters. Monster names are stored
//! in data blocks across Banks $1D/$1E/$13 as 8x8 tile indices.
//!
//! Since the KO patch replaces 8x8 tiles with 16x16 tiles, the original
//! JP monster names render as garbage. This module:
//!
//!   1. Patches each monster's data block to store a sequential ID (0-35)
//!   2. Hooks $03:$B626 (name display) to look up KO names by ID
//!   3. Hooks $03:$C742 (C6EE renderer) to support F1/F0 prefix characters
//!   4. Writes the KO name table to Bank $03 free space

use crate::encoding::ko;
use crate::patch::asm::{assemble, Inst};
use crate::patch::tracked_rom::{Expect, TrackedRom};
use crate::rom::lorom_to_pc;
use std::collections::HashMap;
use std::path::Path;

// ── Constants ────────────────────────────────────────────────────────

/// Hook placement in Bank $03 free space (high region, after diary relocations).
const HOOK_BANK: u8 = 0x03;
/// First address reserved for encyclopedia hooks.
pub const ENC_HOOK_BASE: u16 = 0xF800;

/// Monster pointer table: Bank $1D:$8000, 36 entries × 6 bytes.
const MONSTER_TABLE_PC: usize = 0xE8000; // lorom_to_pc(0x1D, 0x8000)
const MONSTER_COUNT: usize = 36;
/// Offset within each monster data block where the name starts (00-terminated, max 10B).
const NAME_OFFSET_IN_BLOCK: usize = 0x4A;
/// Size of the name slot in each data block (bytes +$4A through +$53).
const NAME_SLOT_SIZE: usize = 10;
/// Max encoded bytes for battle name (excluding 00 terminator).
/// With the monster index moved to offset $56, the full 10-byte name slot
/// ($4A-$53) is available for the name. If a name uses all 10 bytes,
/// the 00 terminator spills into offset $54.
const BATTLE_NAME_MAX_LEN: usize = 10;

/// Offset within the data block where the monster's discovery ID is stored.
/// JP original uses 1-based values (1-36) at this offset.
/// This is separate from the name slot. The game bulk-copies the data block
/// to WRAM $1760, so this value ends up at WRAM $17B6.
/// The game's encyclopedia search at $03:$B4F9 scans the discovery table
/// ($14A8) for `selection+1`, so the values must be 1-based.
/// The encyclopedia name hook reads $17B6 and subtracts 1 to get a 0-based
/// index into the KO name table.
const INDEX_OFFSET_IN_BLOCK: usize = 0x56;

/// Fixed entry size in the KO name table (must be power of 2 for easy ASM shift).
const NAME_ENTRY_SIZE: usize = 16;

/// Description pointer offsets within each monster data block.
/// +0x5B = addr lo, +0x5C = addr hi, +0x5D = bank.
const DESC_PTR_OFFSET: usize = 0x5B;
/// Bank for KO description data (high area, after text relocation).
const DESC_TABLE_BANK: u8 = 0x33;
/// Start address for description data in Bank $33 (high area to avoid relocation conflict).
const DESC_TABLE_BASE: u16 = 0xF000;

/// C6EE char processing: $03:$C742 (PC 0x1C742).
/// Original bytes: EB A9 0F (XBA / LDA #$0F) — replaced with JMP hook (3 bytes).
const C6EE_PATCH_PC: usize = 0x1C742;
/// Address after the replaced bytes (continue original path).
const C6EE_CONTINUE_ADDR: u16 = 0xC747; // $C745 STA dp$0B is part of replaced code,
                                        // but we do it ourselves in the hook.
                                        // Actually $C745 is 85 0B = STA dp$0B (2 bytes)
                                        // After replaced 3 bytes (EB A9 0F), next is $C745.
                                        // Our hook does STA dp$0B and jumps to $C747.
/// Tile computation entry at $C755 (REP #$20 / ASL×6 / ADC #$8000).
const TILE_CALC_ADDR: u16 = 0xC755;

/// Monster name hook: $03:$B626 (PC 0x1B626).
/// Original bytes: A0 AA 17 (LDY #$17AA) — replaced with JMP hook (3 bytes).
const NAME_PATCH_PC: usize = 0x1B626;
/// JSL $03:C6EE at $03:$B62D (continue after hook sets up Y and dp$0B).
const NAME_JSL_ADDR: u16 = 0xB62D;

// ── Encyclopedia Data ──────────────────────────────────────────────

/// Pre-encoded encyclopedia data loaded from TSV.
pub struct EncyclopediaData {
    /// 36 monster names, each FF-terminated encoded bytes (for encyclopedia display table).
    pub names: Vec<Vec<u8>>,
    /// 36 battle names, raw encoded bytes without terminator (max 8 bytes each).
    /// Written to monster data blocks at +$4A for battle dialogue rendering.
    pub battle_names: Vec<Vec<u8>>,
    /// 36 monster descriptions, each [loc_index, text_bytes..., FF].
    pub descs: Vec<Vec<u8>>,
}

/// Load encyclopedia TSV and encode names/descriptions.
///
/// TSV format: ID\tTYPE\tLOC_IDX\tKO
/// TYPE is "name" or "desc".
/// Description KO text uses literal `\n` for newlines.
pub fn load_encyclopedia_tsv(
    tsv_path: &Path,
    ko_table: &HashMap<char, Vec<u8>>,
) -> Result<EncyclopediaData, String> {
    let content = std::fs::read_to_string(tsv_path)
        .map_err(|e| format!("Failed to read '{}': {}", tsv_path.display(), e))?;

    let mut names: Vec<Option<Vec<u8>>> = vec![None; MONSTER_COUNT];
    let mut descs: Vec<Option<Vec<u8>>> = vec![None; MONSTER_COUNT];

    for line in content.lines() {
        if line.starts_with('#') || line.starts_with("ID") || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 4 {
            return Err(format!("Invalid TSV line (need 4+ columns): {}", line));
        }
        let id: usize = parts[0]
            .parse()
            .map_err(|_| format!("Invalid monster ID: {}", parts[0]))?;
        if id >= MONSTER_COUNT {
            return Err(format!(
                "Monster ID {} out of range (max {})",
                id,
                MONSTER_COUNT - 1
            ));
        }
        let entry_type = parts[1];
        let loc_idx: u8 = parts[2]
            .parse()
            .map_err(|_| format!("Invalid LOC_IDX: {}", parts[2]))?;
        let ko_text = parts[3];

        match entry_type {
            "name" => {
                let mut encoded = ko::encode_simple(ko_text, ko_table)
                    .map_err(|e| format!("Monster #{} name encoding error: {}", id, e))?;
                encoded.push(0xFF); // terminator
                names[id] = Some(encoded);
            }
            "desc" => {
                // Replace literal \n with actual newline for encoding
                let text_with_newlines = ko_text.replace("\\n", "\n");
                let text_encoded = ko::encode_simple(&text_with_newlines, ko_table)
                    .map_err(|e| format!("Monster #{} desc encoding error: {}", id, e))?;
                let mut encoded = vec![loc_idx]; // location index prefix
                encoded.extend_from_slice(&text_encoded);
                encoded.push(0xFF); // terminator
                descs[id] = Some(encoded);
            }
            _ => {
                return Err(format!(
                    "Unknown entry type '{}' for monster #{}",
                    entry_type, id
                ))
            }
        }
    }

    // Verify all entries present
    let mut missing = Vec::new();
    for i in 0..MONSTER_COUNT {
        if names[i].is_none() {
            missing.push(format!("name #{}", i));
        }
        if descs[i].is_none() {
            missing.push(format!("desc #{}", i));
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "Missing encyclopedia entries: {}",
            missing.join(", ")
        ));
    }

    let final_names: Vec<Vec<u8>> = names.into_iter().map(|n| n.unwrap()).collect();
    let battle_names = compute_battle_names(&final_names);

    Ok(EncyclopediaData {
        names: final_names,
        battle_names,
        descs: descs.into_iter().map(|d| d.unwrap()).collect(),
    })
}

/// Compute battle names from FF-terminated encyclopedia names.
///
/// Each name is truncated to BATTLE_NAME_MAX_LEN at a character boundary.
pub fn compute_battle_names(names: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let mut battle_names = Vec::with_capacity(names.len());
    for (i, name) in names.iter().enumerate() {
        let raw = if name.last() == Some(&0xFF) {
            &name[..name.len() - 1]
        } else {
            &name[..]
        };
        let truncated = truncate_at_char_boundary(raw, BATTLE_NAME_MAX_LEN);
        if truncated.len() < raw.len() {
            println!(
                "  Battle name #{}: truncated from {} to {} bytes",
                i,
                raw.len(),
                truncated.len()
            );
        }
        battle_names.push(truncated);
    }
    battle_names
}

/// Truncate encoded bytes at a character boundary, respecting multi-byte prefixes.
fn truncate_at_char_boundary(bytes: &[u8], max_len: usize) -> Vec<u8> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let char_len = match bytes[i] {
            0xFB | 0xF0 | 0xF1 | 0xFA => 2,
            _ => 1,
        };
        if result.len() + char_len > max_len {
            break;
        }
        for j in 0..char_len {
            if i + j < bytes.len() {
                result.push(bytes[i + j]);
            }
        }
        i += char_len;
    }
    result
}

// ── C6EE F1/F0 Hook ──────────────────────────────────────────────────

/// Build the C6EE F1/F0 dispatch hook.
///
/// Hooks into the char processing path at $03:$C742. Checks for F1/F0
/// prefix bytes and routes them to Bank $32 tile data. Normal chars
/// fall through to the original Bank $0F path.
///
/// Entry: 8-bit A = char code (from text stream).
///        dp$0B is about to be set to the font tile bank.
///        DB = text source bank.
///        Y = current text read position.
fn build_c6ee_hook() -> Vec<u8> {
    use Inst::*;
    let program = vec![
        // Check for F1 prefix (Bank $32:$8000, page 0)
        CmpImm8(0xF1),
        Beq("f1"),
        // Check for F0 prefix (Bank $32:$C000, page 1)
        CmpImm8(0xF0),
        Beq("f0"),
        // Original path: XBA / LDA #$0F / STA dp$0B / JMP continue
        Xba,
        LdaImm8(0x0F),
        StaDp(0x0B), // dp$0B = $0F (Bank $0F)
        JmpAbs(C6EE_CONTINUE_ADDR),
        // F1: tiles at Bank $32:$8000 (page 0)
        Label("f1"),
        Xba, // XBA (save char code to A_hi)
        LdaImm8(0x32),
        StaDp(0x0B), // dp$0B = $32
        LdaImm8(0x00),
        Xba,             // XBA → A_lo = char_code($F1), A_hi = $00
        LdaAbsY(0x0000), // LDA $0000,Y (read F1 index byte)
        Iny,             // advance past index byte
        // Now A_lo = F1_index, A_hi = $00 → page 0 ($8000 + index*64)
        JmpAbs(TILE_CALC_ADDR),
        // F0: tiles at Bank $32:$C000 (page 1)
        Label("f0"),
        Xba,
        LdaImm8(0x32),
        StaDp(0x0B), // dp$0B = $32
        LdaImm8(0x00),
        Xba,             // XBA → A_lo = $F0, A_hi = $00
        LdaAbsY(0x0000), // LDA $0000,Y (read F0 index byte)
        Iny,
        // Build page 1: need A = $01:F0_index
        Xba,  // XBA → A_lo = $00, A_hi = F0_index
        IncA, // INC A → A_lo = $01
        Xba,  // XBA → A_lo = F0_index, A_hi = $01
        JmpAbs(TILE_CALC_ADDR),
    ];
    assemble(&program).expect("C6EE F1/F0 hook assembly failed")
}

// ── Monster Name Hook ─────────────────────────────────────────────────

/// Build the monster name display hook.
///
/// Replaces `LDY #$17AA` at $03:$B626 (3 bytes → JMP hook).
/// Reads the monster index from WRAM $17B6 (offset $56 in the data block,
/// placed there by our data block patching), computes an offset into the
/// KO name table, sets Y and dp$0B for C6EE to read from Bank $03 ROM,
/// then jumps to the JSL $03:C6EE call.
///
/// The value at $17B6 is 1-based (matching the JP original's discovery ID
/// convention: values 1-36). We subtract 1 to get a 0-based table index
/// before multiplying by NAME_ENTRY_SIZE.
///
/// Entry: 8-bit A/M mode. dp$17B4 was saved on stack.
fn build_name_hook(name_table_addr: u16) -> Vec<u8> {
    use Inst::*;
    // WRAM $17B6 = $1760 + $56 (INDEX_OFFSET_IN_BLOCK, separate from name slot)
    let index_addr: u16 = 0x1760 + INDEX_OFFSET_IN_BLOCK as u16; // $17B6
    let program = vec![
        // Read monster index from WRAM $17B6 (absolute long to access WRAM bank $7E)
        LdaLong(0x7E0000 | index_addr as u32),
        // Build 16-bit value: $00:ID
        Xba,           // XBA → A_hi = ID, A_lo = old
        LdaImm8(0x00), // A_lo = $00
        Xba,           // XBA → A_lo = ID, A_hi = $00
        // Convert 1-based discovery ID to 0-based table index
        DecA, // DEC A → ID = ID - 1
        // Multiply by NAME_ENTRY_SIZE (16 = ASL×4)
        Rep(0x20), // 16-bit A
        AslA,      // ASL
        AslA,      // ASL
        AslA,      // ASL
        AslA,      // ASL → A = ID * 16
        // Add name table base address
        Clc,
        AdcImm16(name_table_addr),
        Tay,       // TAY → Y = table + ID * 16
        Sep(0x20), // 8-bit A
        // Set dp$0B = $03 (Bank $03, where name table is in ROM)
        LdaImm8(HOOK_BANK),
        StaDp(0x0B),
        // Jump to JSL $03:C6EE (skip the original LDA #$00 / STA dp$0B at $B629)
        JmpAbs(NAME_JSL_ADDR),
    ];
    assemble(&program).expect("name hook assembly failed")
}

// ── Main Entry Point ──────────────────────────────────────────────────

/// Apply all encyclopedia hooks to the ROM.
///
/// Must be called AFTER text relocation (which may use Bank $03:$DA70-$F7FF).
pub fn apply_encyclopedia_hooks(
    rom: &mut TrackedRom,
    data: &EncyclopediaData,
) -> Result<usize, String> {
    if data.names.len() != MONSTER_COUNT {
        return Err(format!(
            "Expected {} monster names, got {}",
            MONSTER_COUNT,
            data.names.len()
        ));
    }
    if data.descs.len() != MONSTER_COUNT {
        return Err(format!(
            "Expected {} monster descs, got {}",
            MONSTER_COUNT,
            data.descs.len()
        ));
    }

    let mut count = 0;

    // ── 1. Build hook code ────────────────────────────────────────────
    let c6ee_code = build_c6ee_hook();

    // We need to know the name table address before building the name hook,
    // so compute all addresses first.
    let mut next_addr = ENC_HOOK_BASE;

    let c6ee_hook_addr = next_addr;
    next_addr += c6ee_code.len() as u16;

    let name_hook_addr = next_addr;
    // Build name hook with a placeholder — we'll rebuild after computing table addr
    let name_hook_placeholder = build_name_hook(0x0000);
    next_addr += name_hook_placeholder.len() as u16;

    let name_table_addr = next_addr;
    let name_table_size = (MONSTER_COUNT * NAME_ENTRY_SIZE) as u16;
    next_addr += name_table_size;

    let hooks_end = next_addr;

    // Sanity check: ensure we fit in Bank $03 ($F800-$FFFF = 2048 bytes)
    let total_size = (hooks_end as usize).saturating_sub(ENC_HOOK_BASE as usize);
    let available = 0xFFFFusize - ENC_HOOK_BASE as usize + 1;
    if total_size > available {
        return Err(format!(
            "Encyclopedia hooks exceed Bank $03 space: {} bytes > {} available",
            total_size, available
        ));
    }

    // Now rebuild name hook with the actual table address
    let name_hook_code = build_name_hook(name_table_addr);
    assert_eq!(
        name_hook_code.len(),
        name_hook_placeholder.len(),
        "name hook size changed with different table addr"
    );

    println!("\n--- Applying encyclopedia hooks ---");
    println!(
        "  C6EE F1/F0 hook: {} bytes at ${:02X}:${:04X}",
        c6ee_code.len(),
        HOOK_BANK,
        c6ee_hook_addr
    );
    println!(
        "  Name hook: {} bytes at ${:02X}:${:04X}",
        name_hook_code.len(),
        HOOK_BANK,
        name_hook_addr
    );
    println!(
        "  Name table: {} bytes at ${:02X}:${:04X} ({} entries × {} bytes)",
        name_table_size, HOOK_BANK, name_table_addr, MONSTER_COUNT, NAME_ENTRY_SIZE
    );
    println!(
        "  Total: {} bytes (${:04X}-${:04X})",
        hooks_end - ENC_HOOK_BASE,
        ENC_HOOK_BASE,
        hooks_end - 1
    );

    // ── 2. Write hook code + name table to Bank $03 free space ──────
    {
        let base_pc = lorom_to_pc(HOOK_BANK, ENC_HOOK_BASE);
        let mut region = rom.region_expect(
            base_pc,
            total_size,
            "encyclopedia:hook_code",
            &Expect::FreeSpace(0xFF),
        );

        // C6EE hook
        let c6ee_off = (c6ee_hook_addr - ENC_HOOK_BASE) as usize;
        region.copy_at(c6ee_off, &c6ee_code);

        // Name hook
        let name_hook_off = (name_hook_addr - ENC_HOOK_BASE) as usize;
        region.copy_at(name_hook_off, &name_hook_code);

        // ── 3. Write KO name table ────────────────────────────────────
        let table_off = (name_table_addr - ENC_HOOK_BASE) as usize;
        for (i, name_bytes) in data.names.iter().enumerate() {
            let entry_off = table_off + i * NAME_ENTRY_SIZE;
            if name_bytes.len() > NAME_ENTRY_SIZE {
                return Err(format!(
                    "Monster #{} name too long: {} bytes (max {})",
                    i,
                    name_bytes.len(),
                    NAME_ENTRY_SIZE
                ));
            }
            // Fill entry with FF first, then write name bytes
            region.data_mut()[entry_off..entry_off + NAME_ENTRY_SIZE].fill(0xFF);
            region.copy_at(entry_off, name_bytes);
        }
    }
    println!("  Wrote {} KO monster names to table", data.names.len());

    // ── 4. Patch C6EE renderer at $03:$C742 ──────────────────────────
    // Replace EB A9 0F (XBA / LDA #$0F) with JMP c6ee_hook_addr (3 bytes)
    rom.write_expect(
        C6EE_PATCH_PC,
        &[0x4C, c6ee_hook_addr as u8, (c6ee_hook_addr >> 8) as u8],
        "encyclopedia:c6ee_jmp",
        &Expect::Bytes(&[0xEB, 0xA9, 0x0F]),
    );
    println!(
        "  Patched $03:$C742: JMP ${:04X} (C6EE F1/F0 dispatch)",
        c6ee_hook_addr
    );
    count += 1;

    // ── 5. Patch name display at $03:$B626 ───────────────────────────
    // Replace A0 AA 17 (LDY #$17AA) with JMP name_hook_addr (3 bytes)
    rom.write_expect(
        NAME_PATCH_PC,
        &[0x4C, name_hook_addr as u8, (name_hook_addr >> 8) as u8],
        "encyclopedia:name_jmp",
        &Expect::Bytes(&[0xA0, 0xAA, 0x17]),
    );
    println!(
        "  Patched $03:$B626: JMP ${:04X} (name table lookup)",
        name_hook_addr
    );
    count += 1;

    // ── 6. Patch monster data blocks (write KO battle names + index) ──
    let patched = patch_monster_data_names(rom, &data.battle_names)?;
    println!(
        "  Patched {} monster data blocks with KO battle names + index",
        patched
    );
    count += patched;

    // ── 7. Write KO description table to Bank $33 + patch desc pointers ──
    let desc_patched = write_desc_table_and_patch_pointers(rom, &data.descs)?;
    count += desc_patched;

    Ok(count)
}

/// Write KO description table to Bank $33 and patch data block pointers.
///
/// Descriptions are written sequentially starting at DESC_TABLE_BASE.
/// Each monster data block's desc pointer (offset +0x5B/+0x5C/+0x5D)
/// is updated to point to the corresponding KO description.
fn write_desc_table_and_patch_pointers(
    rom: &mut TrackedRom,
    descs: &[Vec<u8>],
) -> Result<usize, String> {
    let mut offset = DESC_TABLE_BASE;
    let mut patched = 0;

    // Sanity check: total desc size fits
    let total_desc_bytes: usize = descs.iter().map(|d| d.len()).sum();
    let end_addr = DESC_TABLE_BASE as usize + total_desc_bytes;
    if end_addr > 0x10000 {
        return Err(format!(
            "Desc table overflows Bank $33: {} bytes from ${:04X} to ${:04X}",
            total_desc_bytes, DESC_TABLE_BASE, end_addr
        ));
    }

    // Build the full desc table in memory
    let mut desc_data = Vec::with_capacity(total_desc_bytes);
    let mut desc_addrs = Vec::with_capacity(descs.len());
    for desc in descs {
        desc_addrs.push(offset);
        desc_data.extend_from_slice(desc);
        offset += desc.len() as u16;
    }

    // Write entire desc table as one region
    rom.write_snes_expect(
        DESC_TABLE_BANK,
        DESC_TABLE_BASE,
        &desc_data,
        "encyclopedia:desc_table",
        &Expect::FreeSpace(0xFF),
    );

    for (i, &desc_addr) in desc_addrs.iter().enumerate() {
        // Patch data block pointer: offset +0x5B (addr lo/hi) + +0x5D (bank)
        let entry_pc = MONSTER_TABLE_PC + i * 6;
        if entry_pc + 3 > rom.len() {
            return Err(format!("Monster table entry {} out of ROM bounds", i));
        }
        let lo = rom[entry_pc] as u16;
        let hi = rom[entry_pc + 1] as u16;
        let bank = rom[entry_pc + 2];
        let snes_addr = (hi << 8) | lo;
        let block_pc = lorom_to_pc(bank, snes_addr);
        let ptr_pc = block_pc + DESC_PTR_OFFSET;

        if ptr_pc + 3 > rom.len() {
            return Err(format!(
                "Monster #{} desc ptr at PC 0x{:X} out of ROM bounds",
                i, ptr_pc
            ));
        }

        rom.write(
            ptr_pc,
            &[desc_addr as u8, (desc_addr >> 8) as u8, DESC_TABLE_BANK],
            "encyclopedia:desc_ptr",
        );
        patched += 1;
    }

    println!(
        "  Wrote {} KO monster descriptions ({} bytes at ${:02X}:${:04X}-${:04X})",
        patched,
        total_desc_bytes,
        DESC_TABLE_BANK,
        DESC_TABLE_BASE,
        offset - 1
    );
    println!("  Patched {} monster data block desc pointers", patched);

    Ok(patched)
}

/// Write KO battle names and monster index to each monster data block.
///
/// The monster pointer table at Bank $1D:$8000 has 36 entries × 6 bytes.
/// Each entry contains two 24-bit pointers. The first pointer points to
/// the monster's data block. At offset 0x4A in the data block, there's
/// a 00-terminated name (max 10 bytes).
///
/// Layout:
///   +$4A..$53 (10 bytes): KO battle name + 00 terminator
///     - Names ≤ 9 bytes: name + 00 fits within the 10-byte slot
///     - Names = 10 bytes: 00 terminator spills into +$54 (1 byte past slot)
///   +$56: monster discovery ID (1-36, 1-based to match JP convention)
///
/// Battle: the in-game renderer reads from WRAM $17AA, sees KO text + 00 terminator.
/// Encyclopedia: the name hook reads WRAM $17B6 (offset $56) to get the index.
fn patch_monster_data_names(
    rom: &mut TrackedRom,
    battle_names: &[Vec<u8>],
) -> Result<usize, String> {
    if battle_names.len() != MONSTER_COUNT {
        return Err(format!(
            "Expected {} battle names, got {}",
            MONSTER_COUNT,
            battle_names.len()
        ));
    }

    let mut patched = 0;

    for (i, battle) in battle_names.iter().enumerate().take(MONSTER_COUNT) {
        let entry_pc = MONSTER_TABLE_PC + i * 6;
        if entry_pc + 3 > rom.len() {
            return Err(format!("Monster table entry {} out of ROM bounds", i));
        }

        // Read 24-bit pointer (lo, hi, bank) to data block
        let lo = rom[entry_pc] as u16;
        let hi = rom[entry_pc + 1] as u16;
        let bank = rom[entry_pc + 2];
        let snes_addr = (hi << 8) | lo;

        // Convert to PC offset
        let block_pc = lorom_to_pc(bank, snes_addr);
        let name_pc = block_pc + NAME_OFFSET_IN_BLOCK;
        let index_pc = block_pc + INDEX_OFFSET_IN_BLOCK;

        if index_pc + 1 > rom.len() {
            return Err(format!(
                "Monster #{} index at PC 0x{:X} out of ROM bounds",
                i, index_pc
            ));
        }

        if battle.len() > BATTLE_NAME_MAX_LEN {
            return Err(format!(
                "Monster #{} battle name too long: {} bytes (max {})",
                i,
                battle.len(),
                BATTLE_NAME_MAX_LEN
            ));
        }

        // Build name slot: zero-fill + battle name + optional terminator
        let mut slot = vec![0x00u8; NAME_SLOT_SIZE];
        slot[..battle.len()].copy_from_slice(battle);
        rom.write(name_pc, &slot, "encyclopedia:battle_name");
        // If name uses all 10 bytes, write 00 terminator at +$54 (1 byte past slot)
        if battle.len() == NAME_SLOT_SIZE {
            rom.write_byte(
                name_pc + NAME_SLOT_SIZE,
                0x00,
                "encyclopedia:battle_name_term",
            );
        }
        // Write monster index at offset $56 (separate from name slot).
        // Uses 1-based indexing (1-36) to match the JP original's convention.
        rom.write_byte(index_pc, (i + 1) as u8, "encyclopedia:monster_index");

        patched += 1;
    }

    Ok(patched)
}

#[cfg(test)]
#[path = "encyclopedia_tests.rs"]
mod tests;
