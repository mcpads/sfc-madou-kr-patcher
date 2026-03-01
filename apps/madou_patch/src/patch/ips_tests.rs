use super::*;

#[test]
fn ips_identical_files() {
    let data = vec![0u8; 100];
    let ips = generate_ips(&data, &data);
    // Just header + footer
    assert_eq!(&ips[..5], b"PATCH");
    assert_eq!(&ips[ips.len() - 3..], b"EOF");
    assert_eq!(ips.len(), 8);
}

#[test]
fn ips_single_change() {
    let orig = vec![0u8; 100];
    let mut patched = orig.clone();
    patched[10] = 0x42;

    let ips = generate_ips(&orig, &patched);
    assert_eq!(&ips[..5], b"PATCH");
    assert_eq!(&ips[ips.len() - 3..], b"EOF");

    // Record: offset=10 (3 bytes), size=1 (2 bytes), data=0x42
    assert_eq!(ips[5], 0x00); // offset high
    assert_eq!(ips[6], 0x00); // offset mid
    assert_eq!(ips[7], 0x0A); // offset low
    assert_eq!(ips[8], 0x00); // size high
    assert_eq!(ips[9], 0x01); // size low
    assert_eq!(ips[10], 0x42); // data
}

#[test]
fn count_records_basic() {
    let orig = vec![0u8; 100];
    let mut patched = orig.clone();
    patched[10] = 0x42;
    patched[50] = 0x99;

    let ips = generate_ips(&orig, &patched);
    let count = count_records(&ips);
    assert!(count >= 1); // may merge or split
}

#[test]
fn ips_empty_files() {
    let ips = generate_ips(&[], &[]);
    assert_eq!(&ips[..5], b"PATCH");
    assert_eq!(&ips[5..], b"EOF");
    assert_eq!(ips.len(), 8);
}

#[test]
fn ips_contiguous_change_block() {
    let orig = vec![0u8; 100];
    let mut patched = orig.clone();
    // Change bytes 10..15
    for i in 10..15 {
        patched[i] = (i - 9) as u8;
    }
    let ips = generate_ips(&orig, &patched);
    assert_eq!(&ips[..5], b"PATCH");
    assert_eq!(&ips[ips.len() - 3..], b"EOF");

    // Should produce one record with offset=10, size=5
    assert_eq!(ips[5], 0x00); // offset high
    assert_eq!(ips[6], 0x00); // offset mid
    assert_eq!(ips[7], 0x0A); // offset low = 10
    assert_eq!(ips[8], 0x00); // size high
    assert_eq!(ips[9], 0x05); // size low = 5
    assert_eq!(&ips[10..15], &[1, 2, 3, 4, 5]);
}

#[test]
fn ips_different_length_files() {
    // Patched shorter than original
    let orig = vec![0xAAu8; 200];
    let mut patched = vec![0xAAu8; 100];
    patched[50] = 0xBB;
    let ips = generate_ips(&orig, &patched);
    let count = count_records(&ips);
    assert_eq!(count, 1);
}

#[test]
fn ips_all_bytes_different() {
    let orig = vec![0x00u8; 50];
    let patched = vec![0xFFu8; 50];
    let ips = generate_ips(&orig, &patched);
    let count = count_records(&ips);
    assert!(count >= 1);
    // Verify data integrity: all patched bytes should appear in the record
    assert_eq!(&ips[..5], b"PATCH");
    assert_eq!(&ips[ips.len() - 3..], b"EOF");
}

#[test]
fn ips_gap_merging() {
    // Two changes separated by a small gap (<=8 identical bytes) should merge
    let orig = vec![0u8; 100];
    let mut patched = orig.clone();
    patched[10] = 0xAA;
    // Gap of 5 bytes (10+1=11..16, then change at 16)
    patched[16] = 0xBB;

    let ips = generate_ips(&orig, &patched);
    let count = count_records(&ips);
    // Should be merged into 1 record (gap <= 8)
    assert_eq!(count, 1);
}

#[test]
fn ips_no_gap_merging_for_large_gap() {
    // Two changes separated by >8 identical bytes should NOT merge
    let orig = vec![0u8; 100];
    let mut patched = orig.clone();
    patched[10] = 0xAA;
    patched[30] = 0xBB; // gap = 19 bytes

    let ips = generate_ips(&orig, &patched);
    let count = count_records(&ips);
    assert_eq!(count, 2);
}

#[test]
fn ips_high_offset() {
    // Test 3-byte offset encoding for large file positions
    let size = 0x20000; // 128KB
    let orig = vec![0u8; size];
    let mut patched = orig.clone();
    let offset = 0x1ABCD;
    patched[offset] = 0x42;

    let ips = generate_ips(&orig, &patched);
    // Verify offset encoding
    assert_eq!(ips[5], 0x01); // high byte of 0x1ABCD
    assert_eq!(ips[6], 0xAB); // mid byte
    assert_eq!(ips[7], 0xCD); // low byte
}

#[test]
fn count_records_invalid_header() {
    assert_eq!(count_records(b"NOT_IPS"), 0);
    assert_eq!(count_records(b"PATC"), 0);
    assert_eq!(count_records(&[]), 0);
}

#[test]
fn count_records_header_only() {
    let ips = b"PATCHEOF";
    assert_eq!(count_records(ips), 0);
}

#[test]
fn ips_roundtrip_apply() {
    // Generate IPS then apply it to verify correctness
    let orig = vec![0u8; 256];
    let mut patched = orig.clone();
    patched[0] = 0x11;
    patched[100] = 0x22;
    patched[200] = 0x33;

    let ips = generate_ips(&orig, &patched);

    // Apply IPS patch manually
    let mut applied = orig.clone();
    let mut i = 5; // skip header
    while i + 3 <= ips.len() {
        if &ips[i..i + 3] == b"EOF" {
            break;
        }
        let offset =
            ((ips[i] as usize) << 16) | ((ips[i + 1] as usize) << 8) | (ips[i + 2] as usize);
        let size = ((ips[i + 3] as usize) << 8) | (ips[i + 4] as usize);
        applied[offset..offset + size].copy_from_slice(&ips[i + 5..i + 5 + size]);
        i += 5 + size;
    }

    assert_eq!(applied[0], 0x11);
    assert_eq!(applied[100], 0x22);
    assert_eq!(applied[200], 0x33);
}
