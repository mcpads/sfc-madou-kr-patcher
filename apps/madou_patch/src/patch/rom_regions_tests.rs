use super::*;
use crate::rom::lorom_to_pc;

#[test]
fn test_no_collision_disjoint() {
    let mut t = RomRegionTracker::new();
    t.register(0x1000, 0x100, "region_a");
    t.register(0x1100, 0x100, "region_b");
    t.register(0x2000, 0x200, "region_c");
    assert!(t.check().is_ok());
}

#[test]
fn test_collision_detected() {
    let mut t = RomRegionTracker::new();
    t.register(0x1000, 0x200, "region_a"); // [0x1000..0x1200)
    t.register(0x1100, 0x200, "region_b"); // [0x1100..0x1300)
    let result = t.check();
    assert!(result.is_err());
    let msg = result.unwrap_err();
    assert!(msg.contains("region_a"), "should mention region_a: {}", msg);
    assert!(msg.contains("region_b"), "should mention region_b: {}", msg);
    assert!(
        msg.contains("256 bytes"),
        "should show overlap size: {}",
        msg
    );
}

#[test]
fn test_adjacent_no_collision() {
    let mut t = RomRegionTracker::new();
    t.register(0x1000, 0x100, "region_a"); // [0x1000..0x1100)
    t.register(0x1100, 0x100, "region_b"); // [0x1100..0x1200)
    assert!(t.check().is_ok());
}

#[test]
fn test_zero_length_ignored() {
    let mut t = RomRegionTracker::new();
    t.register(0x1000, 0, "empty_region");
    t.register(0x1000, 0x100, "real_region");
    assert!(t.check().is_ok());
}

#[test]
fn test_snes_conversion() {
    let mut t = RomRegionTracker::new();
    t.register_snes(0x0F, 0x8000, 0x800, "font:fixed_encode");
    // Verify the converted PC matches lorom_to_pc
    let expected_pc = lorom_to_pc(0x0F, 0x8000);
    assert_eq!(expected_pc, 0x78000);
    // Non-overlapping region
    t.register_snes(0x0F, 0x8800, 0x100, "font:single_byte");
    assert!(t.check().is_ok());
}

#[test]
fn test_full_containment_detected() {
    let mut t = RomRegionTracker::new();
    t.register(0x1000, 0x400, "outer"); // [0x1000..0x1400)
    t.register(0x1100, 0x100, "inner"); // [0x1100..0x1200)
    let result = t.check();
    assert!(result.is_err());
    let msg = result.unwrap_err();
    assert!(msg.contains("outer"));
    assert!(msg.contains("inner"));
}

#[test]
fn test_single_region_ok() {
    let mut t = RomRegionTracker::new();
    t.register(0x1000, 0x100, "only_one");
    assert!(t.check().is_ok());
}

#[test]
fn test_empty_tracker_ok() {
    let t = RomRegionTracker::new();
    assert!(t.check().is_ok());
}

#[test]
fn test_multiple_collisions_reported() {
    let mut t = RomRegionTracker::new();
    t.register(0x1000, 0x200, "a"); // [0x1000..0x1200)
    t.register(0x1100, 0x200, "b"); // [0x1100..0x1300)
    t.register(0x1200, 0x200, "c"); // [0x1200..0x1400)
    let result = t.check();
    assert!(result.is_err());
    let msg = result.unwrap_err();
    // a↔b collision and b↔c collision
    assert!(msg.contains("2"), "should report 2 collisions: {}", msg);
}

// ── check_untracked_writes tests ─────────────────────────────────

#[test]
fn test_untracked_writes_clean() {
    let mut t = RomRegionTracker::new();
    t.register(0x10, 4, "region_a");
    let original = vec![0x00u8; 32];
    let mut patched = original.clone();
    patched[0x10] = 0xFF;
    patched[0x11] = 0xAA;
    assert!(t.check_untracked_writes(&original, &patched).is_ok());
}

#[test]
fn test_untracked_writes_detected() {
    let mut t = RomRegionTracker::new();
    t.register(0x10, 4, "region_a");
    let original = vec![0x00u8; 32];
    let mut patched = original.clone();
    // Write inside registered region (OK)
    patched[0x10] = 0xFF;
    // Write outside registered region (UNTRACKED)
    patched[0x00] = 0xAB;
    let result = t.check_untracked_writes(&original, &patched);
    assert!(result.is_err());
    let msg = result.unwrap_err();
    assert!(
        msg.contains("UNTRACKED"),
        "should report untracked: {}",
        msg
    );
    assert!(msg.contains("0x000000"), "should show offset: {}", msg);
}

#[test]
fn test_untracked_writes_no_changes() {
    let t = RomRegionTracker::new();
    let rom = vec![0x00u8; 32];
    assert!(t.check_untracked_writes(&rom, &rom).is_ok());
}

#[test]
fn test_untracked_writes_multiple_runs() {
    let mut t = RomRegionTracker::new();
    t.register(0x08, 4, "middle");
    let original = vec![0x00u8; 32];
    let mut patched = original.clone();
    // Two untracked regions: [0x00..0x02) and [0x10..0x12)
    patched[0x00] = 0xFF;
    patched[0x01] = 0xFF;
    patched[0x10] = 0xAA;
    patched[0x11] = 0xBB;
    // One tracked write (OK)
    patched[0x09] = 0xCC;
    let result = t.check_untracked_writes(&original, &patched);
    assert!(result.is_err());
    let msg = result.unwrap_err();
    assert!(msg.contains("2 region(s)"), "should report 2 runs: {}", msg);
}
