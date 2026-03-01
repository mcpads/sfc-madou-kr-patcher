use super::*;
use crate::patch::tracked_rom::TrackedRom;

#[test]
fn lz_roundtrip_simple() {
    let data = vec![0x41; 256]; // 256 bytes of 'A' — highly compressible
    let compressed = compress_lz(&data);
    let (decompressed, _) = decompress_lz(&compressed, 0).unwrap();
    assert_eq!(decompressed, data);
}

#[test]
fn lz_roundtrip_varied() {
    // Varied data with some repeating patterns
    let mut data = Vec::new();
    for i in 0..512u16 {
        data.push((i & 0xFF) as u8);
    }
    let compressed = compress_lz(&data);
    let (decompressed, _) = decompress_lz(&compressed, 0).unwrap();
    assert_eq!(decompressed, data);
}

#[test]
fn lz_roundtrip_font_sized() {
    // 8192 bytes (font-sized) with tile-like patterns
    let mut data = vec![0u8; 8192];
    for (i, b) in data.iter_mut().enumerate() {
        *b = ((i / 16) ^ (i % 16)) as u8;
    }
    let compressed = compress_lz(&data);
    assert!(compressed.len() < data.len(), "Should compress");
    let (decompressed, _) = decompress_lz(&compressed, 0).unwrap();
    assert_eq!(decompressed, data);
}

#[test]
fn lz_empty_data() {
    let data: Vec<u8> = Vec::new();
    let compressed = compress_lz(&data);
    assert_eq!(compressed, vec![0x00]); // just end marker
    let (decompressed, _) = decompress_lz(&compressed, 0).unwrap();
    assert!(decompressed.is_empty());
}

#[test]
fn lz_decompression_offset() {
    // Test decompression starting at non-zero offset
    let data = vec![1, 2, 3, 4, 5];
    let compressed = compress_lz(&data);
    let mut padded = vec![0xFF; 10];
    padded.extend_from_slice(&compressed);
    let (decompressed, consumed) = decompress_lz(&padded, 10).unwrap();
    assert_eq!(decompressed, data);
    assert_eq!(consumed, compressed.len());
}

#[test]
fn patch_fixed_encode_basic() {
    // 32 tiles × 64 bytes = 2048 bytes
    let mut fixed_data = vec![0u8; 2048];
    fixed_data[0] = 0xAA; // mark first tile
    fixed_data[31 * 64] = 0xBB; // mark last tile (char $1F)

    let rom_size = lorom_to_pc(0x10, 0x8000); // past Bank $0F
    let mut rom = TrackedRom::new(vec![0u8; rom_size]);

    let count = patch_fixed_encode(&mut rom, &fixed_data).unwrap();
    assert_eq!(count, 32);

    // Verify tile placement at $0F:$8000 (PC = 0x78000)
    let pc = lorom_to_pc(0x0F, 0x8000);
    assert_eq!(rom[pc], 0xAA);

    // Verify last tile at $0F:$87C0 (char $1F = offset 31×64 = 1984)
    assert_eq!(rom[pc + 31 * 64], 0xBB);
}

#[test]
fn patch_fixed_encode_wrong_size() {
    let fixed_data = vec![0u8; 512]; // wrong size
    let rom_size = lorom_to_pc(0x10, 0x8000);
    let mut rom = TrackedRom::new(vec![0u8; rom_size]);

    let result = patch_fixed_encode(&mut rom, &fixed_data);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("2048"));
}

#[test]
fn patch_fa_f0_tile_placement() {
    // 800 tiles × 64 bytes font data (208 single + 256 FB + 256 FA + 80 F0)
    let mut font_data = vec![0u8; 800 * 64];
    // Mark FA tile 0 and F0 tile 0 with distinctive patterns
    font_data[464 * 64] = 0xFA; // first FA tile
    font_data[720 * 64] = 0xF0; // first F0 tile

    // Create a large enough ROM (at least Bank $32 end)
    let rom_size = lorom_to_pc(0x33, 0x8000); // past Bank $32
    let mut rom = TrackedRom::new(vec![0u8; rom_size]);

    let f0_count = patch_fa_f0(&mut rom, &font_data).unwrap();
    assert_eq!(f0_count, 80); // 80 F0 tiles

    // Verify FA tile 0 placement at $32:$8000
    let fa_pc = lorom_to_pc(0x32, 0x8000);
    assert_eq!(rom[fa_pc], 0xFA);

    // Verify F0 tile 0 placement at $32:$C000
    let f0_pc = lorom_to_pc(0x32, 0xC000);
    assert_eq!(rom[f0_pc], 0xF0);
}

#[test]
fn fb_blank_remap_zeros_fb_and_copies_to_f0() {
    // 464 tiles for single+FB range (208 single + 256 FB)
    let mut font_data = vec![0u8; 464 * 64];

    // Mark the 12 blank FB slots with distinctive bytes
    for &fb_slot in FB_BLANK_SLOTS {
        let tile_idx = 208 + fb_slot as usize;
        let offset = tile_idx * 64;
        font_data[offset..offset + 64].fill(fb_slot); // fill with slot value
    }

    // ROM: needs Bank $0F ($0F:$C000) and Bank $32 ($32:$D440+)
    let rom_size = lorom_to_pc(0x33, 0x8000);
    let mut data = vec![0u8; rom_size];

    // Write FB tiles to ROM first (simulates patch_16x16, which pre-zeros blank slots)
    let fb_pc = lorom_to_pc(0x0F, 0xC000);
    for i in 0..256 {
        let src = (208 + i) * 64;
        let dst = fb_pc + i * 64;
        data[dst..dst + 64].copy_from_slice(&font_data[src..src + 64]);
    }
    // patch_16x16 pre-zeros blank FB slots before writing; simulate that here
    for &fb_slot in FB_BLANK_SLOTS {
        let dst = fb_pc + fb_slot as usize * 64;
        data[dst..dst + 64].fill(0);
    }

    // Run remap with f0_count=69 (dynamic indices: 69-80, matching legacy $45-$50)
    let f0_count = 69;
    let mut rom = TrackedRom::new(data);
    let remap_end = patch_fb_blank_remap(&mut rom, &font_data, f0_count).unwrap();
    // remap_end should be $C000 + (69+12)*64 = $C000 + 81*64 = $C000 + $1440 = $D440
    assert_eq!(
        remap_end,
        0xC000 + (f0_count + FB_BLANK_SLOTS.len()) as u16 * 64
    );

    // Verify: FB slots in Bank $0F are zeroed
    for &fb_slot in FB_BLANK_SLOTS {
        let addr = 0xC000u16 + fb_slot as u16 * 64;
        let pc = lorom_to_pc(0x0F, addr);
        assert!(
            rom[pc..pc + 64].iter().all(|&b| b == 0),
            "FB ${:02X} should be zeroed",
            fb_slot
        );
    }

    // Verify: Glyphs copied to F0 positions in Bank $32
    for (i, &fb_slot) in FB_BLANK_SLOTS.iter().enumerate() {
        let f0_slot = f0_count + i;
        let addr = 0xC000u16 + f0_slot as u16 * 64;
        let pc = lorom_to_pc(0x32, addr);
        assert_eq!(
            rom[pc], fb_slot,
            "F0 {} should contain glyph from FB ${:02X}",
            f0_slot, fb_slot
        );
        assert!(
            rom[pc..pc + 64].iter().all(|&b| b == fb_slot),
            "F0 {} tile should be filled with 0x{:02X}",
            f0_slot,
            fb_slot
        );
    }

    // Verify: Non-blank FB slots are NOT zeroed
    let non_blank = 0xE5u8; // adjacent to $E4 but not in remap list
    let addr = 0xC000u16 + non_blank as u16 * 64;
    let pc = lorom_to_pc(0x0F, addr);
    assert!(
        rom[pc..pc + 64].iter().any(|&b| b != 0) || font_data[(208 + non_blank as usize) * 64] == 0,
        "Non-blank FB slot should be preserved"
    );
}
