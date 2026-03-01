use super::*;

#[test]
fn deref_read_access() {
    let rom = TrackedRom::new(vec![0x10, 0x20, 0x30, 0x40]);
    assert_eq!(rom[0], 0x10);
    assert_eq!(rom[2], 0x30);
    assert_eq!(&rom[1..3], &[0x20, 0x30]);
    assert_eq!(rom.len(), 4);
}

#[test]
fn write_basic() {
    let mut rom = TrackedRom::new(vec![0u8; 16]);
    rom.write(4, &[0xAA, 0xBB, 0xCC], "test:write");
    assert_eq!(rom[4], 0xAA);
    assert_eq!(rom[5], 0xBB);
    assert_eq!(rom[6], 0xCC);
    assert_eq!(rom[3], 0x00); // untouched
    assert!(rom.check().is_ok());
}

#[test]
fn write_snes() {
    // Bank $01, addr $8000 → PC 0x8000
    let mut rom = TrackedRom::new(vec![0u8; 0x10000]);
    rom.write_snes(0x01, 0x8000, &[0xFF, 0xFE], "test:snes");
    assert_eq!(rom[0x8000], 0xFF);
    assert_eq!(rom[0x8001], 0xFE);
    assert!(rom.check().is_ok());
}

#[test]
fn write_byte_basic() {
    let mut rom = TrackedRom::new(vec![0u8; 8]);
    rom.write_byte(3, 0x42, "test:byte");
    assert_eq!(rom[3], 0x42);
    assert!(rom.check().is_ok());
}

#[test]
fn fill_basic() {
    let mut rom = TrackedRom::new(vec![0u8; 16]);
    rom.fill(2, 5, 0xDD, "test:fill");
    assert_eq!(&rom[2..7], &[0xDD; 5]);
    assert_eq!(rom[1], 0x00);
    assert_eq!(rom[7], 0x00);
    assert!(rom.check().is_ok());
}

#[test]
fn collision_detected() {
    let mut rom = TrackedRom::new(vec![0u8; 64]);
    rom.write(10, &[1, 2, 3, 4, 5], "region_a");
    rom.write(12, &[9, 8, 7], "region_b");
    let result = rom.check();
    assert!(result.is_err());
    let msg = result.unwrap_err();
    assert!(msg.contains("region_a"));
    assert!(msg.contains("region_b"));
}

#[test]
fn no_collision_disjoint() {
    let mut rom = TrackedRom::new(vec![0u8; 64]);
    rom.write(0, &[1, 2, 3], "first");
    rom.write(10, &[4, 5, 6], "second");
    assert!(rom.check().is_ok());
}

#[test]
fn region_batch_write() {
    let mut rom = TrackedRom::new(vec![0u8; 32]);
    {
        let mut r = rom.region(8, 8, "test:region");
        r.data_mut().fill(0xFF);
        r.copy_at(2, &[0xAA, 0xBB]);
    }
    assert_eq!(rom[8], 0xFF);
    assert_eq!(rom[9], 0xFF);
    assert_eq!(rom[10], 0xAA);
    assert_eq!(rom[11], 0xBB);
    assert_eq!(rom[12], 0xFF);
    assert_eq!(rom[15], 0xFF);
    assert_eq!(rom[16], 0x00); // outside region
    assert!(rom.check().is_ok());
}

#[test]
fn region_read_access() {
    let mut rom = TrackedRom::new(vec![0x10, 0x20, 0x30, 0x40, 0x50]);
    {
        let r = rom.region(1, 3, "read_test");
        assert_eq!(r.data(), &[0x20, 0x30, 0x40]);
        assert_eq!(r.len(), 3);
    }
    assert!(rom.check().is_ok());
}

#[test]
fn untracked_writes_detected() {
    let original = vec![0u8; 16];
    let mut rom = TrackedRom::new(original.clone());
    // Only write to offset 4, but manually track nothing extra
    rom.write(4, &[0xFF], "tracked");
    // No untracked writes — all changes are tracked
    assert!(rom.check_untracked_writes(&original).is_ok());
}

#[test]
fn into_inner_returns_data() {
    let mut rom = TrackedRom::new(vec![0u8; 8]);
    rom.write(0, &[0xAB, 0xCD], "test");
    let data = rom.into_inner();
    assert_eq!(data[0], 0xAB);
    assert_eq!(data[1], 0xCD);
    assert_eq!(data.len(), 8);
}

#[test]
fn zero_length_write_ignored() {
    let mut rom = TrackedRom::new(vec![0u8; 8]);
    rom.write(0, &[], "empty");
    assert!(rom.check().is_ok());
}

// --- Expectation tests ---

#[test]
fn write_expect_free_space_ok() {
    let mut rom = TrackedRom::new(vec![0xFF; 32]);
    rom.write_expect(4, &[0xAA, 0xBB], "test:free", &Expect::FreeSpace(0xFF));
    assert_eq!(rom[4], 0xAA);
    assert_eq!(rom[5], 0xBB);
}

#[test]
#[should_panic(expected = "Expected free space")]
fn write_expect_free_space_fail() {
    let mut rom = TrackedRom::new(vec![0x00; 32]);
    rom.data[8] = 0x42; // not free
    rom.write_expect(4, &[0xAA; 8], "test:free_fail", &Expect::FreeSpace(0xFF));
}

#[test]
fn write_expect_bytes_ok() {
    let mut rom = TrackedRom::new(vec![0x22, 0x40, 0x94, 0x00, 0xFF, 0xFF]);
    let original = [0x22, 0x40, 0x94, 0x00];
    rom.write_expect(
        0,
        &[0x22, 0x00, 0x80, 0x32],
        "test:bytes",
        &Expect::Bytes(&original),
    );
    assert_eq!(&rom[0..4], &[0x22, 0x00, 0x80, 0x32]);
}

#[test]
#[should_panic(expected = "Expected bytes")]
fn write_expect_bytes_fail() {
    let mut rom = TrackedRom::new(vec![0x22, 0x40, 0x94, 0x00]);
    let wrong = [0xEB, 0xA9, 0x0F];
    rom.write_expect(0, &[0x00; 3], "test:bytes_fail", &Expect::Bytes(&wrong));
}

#[test]
fn region_expect_free_space_ok() {
    let mut rom = TrackedRom::new(vec![0xFF; 64]);
    {
        let mut r = rom.region_expect(8, 16, "test:region_free", &Expect::FreeSpace(0xFF));
        r.data_mut()[0] = 0xAA;
    }
    assert_eq!(rom[8], 0xAA);
}

#[test]
fn fill_expect_free_space_ok() {
    let mut rom = TrackedRom::new(vec![0xFF; 32]);
    rom.fill_expect(4, 8, 0x00, "test:fill_free", &Expect::FreeSpace(0xFF));
    assert_eq!(&rom[4..12], &[0x00; 8]);
}
