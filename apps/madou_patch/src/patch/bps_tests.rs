use super::*;

// ── VLI tests ───────────────────────────────────────────────────────

#[test]
fn vli_roundtrip_zero() {
    let mut buf = Vec::new();
    vli_encode(&mut buf, 0);
    let (val, consumed) = vli_decode(&buf).unwrap();
    assert_eq!(val, 0);
    assert_eq!(consumed, 1);
}

#[test]
fn vli_roundtrip_small() {
    for n in 0..128 {
        let mut buf = Vec::new();
        vli_encode(&mut buf, n);
        let (val, _) = vli_decode(&buf).unwrap();
        assert_eq!(val, n, "roundtrip failed for {n}");
    }
}

#[test]
fn vli_roundtrip_large() {
    let values = [127, 128, 255, 256, 1000, 65535, 0x100000, 0xFFFFFFFF];
    for &n in &values {
        let mut buf = Vec::new();
        vli_encode(&mut buf, n);
        let (val, _) = vli_decode(&buf).unwrap();
        assert_eq!(val, n, "roundtrip failed for {n}");
    }
}

#[test]
fn vli_single_byte_range() {
    let mut buf = Vec::new();
    vli_encode(&mut buf, 0);
    assert_eq!(buf.len(), 1);
    assert_eq!(buf[0], 0x80);

    buf.clear();
    vli_encode(&mut buf, 127);
    assert_eq!(buf.len(), 1);
    assert_eq!(buf[0], 0xFF);
}

#[test]
fn vli_two_byte_range() {
    let mut buf = Vec::new();
    vli_encode(&mut buf, 128);
    assert_eq!(buf.len(), 2);
}

#[test]
fn vli_sequential_decode() {
    let mut buf = Vec::new();
    vli_encode(&mut buf, 42);
    vli_encode(&mut buf, 1000);
    vli_encode(&mut buf, 0);

    let (v1, c1) = vli_decode(&buf).unwrap();
    assert_eq!(v1, 42);
    let (v2, c2) = vli_decode(&buf[c1..]).unwrap();
    assert_eq!(v2, 1000);
    let (v3, _) = vli_decode(&buf[c1 + c2..]).unwrap();
    assert_eq!(v3, 0);
}

// ── BPS creation tests ──────────────────────────────────────────────

#[test]
fn bps_create_identical() {
    let source = vec![0u8; 256];
    let target = source.clone();
    let patch = generate_bps(&source, &target).unwrap();
    assert_eq!(&patch[..4], b"BPS1");
    assert!(patch.len() >= 16);
}

#[test]
fn bps_create_single_byte_diff() {
    let source = vec![0u8; 16];
    let mut target = source.clone();
    target[8] = 0xFF;
    let patch = generate_bps(&source, &target).unwrap();
    assert_eq!(&patch[..4], b"BPS1");
}

#[test]
fn bps_create_roundtrip() {
    let source = b"Hello, World!".to_vec();
    let target = b"Hello, Rust!!".to_vec();
    let patch = generate_bps(&source, &target).unwrap();
    let result = apply_bps(&source, &patch).unwrap();
    assert_eq!(result, target);
}

#[test]
fn bps_create_size_change() {
    let source = vec![0u8; 100];
    let target = vec![0xFFu8; 200];
    let patch = generate_bps(&source, &target).unwrap();
    let result = apply_bps(&source, &patch).unwrap();
    assert_eq!(result, target);
}

#[test]
fn bps_create_empty_to_content() {
    let source = vec![];
    let target = b"new content".to_vec();
    let patch = generate_bps(&source, &target).unwrap();
    let result = apply_bps(&source, &patch).unwrap();
    assert_eq!(result, target);
}

// ── BPS application tests ───────────────────────────────────────────

#[test]
fn bps_apply_validates_magic() {
    let result = apply_bps(&[], b"XXXX1234567890ab");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("invalid magic"));
}

#[test]
fn bps_apply_validates_source_crc() {
    let source = b"hello".to_vec();
    let target = b"world".to_vec();
    let patch = generate_bps(&source, &target).unwrap();
    let wrong_source = b"wrong".to_vec();
    let result = apply_bps(&wrong_source, &patch);
    assert!(result.is_err());
}

#[test]
fn bps_apply_roundtrip_simple() {
    let source = vec![1, 2, 3, 4, 5, 6, 7, 8];
    let mut target = source.clone();
    target[3] = 99;
    target[7] = 88;
    let patch = generate_bps(&source, &target).unwrap();
    let result = apply_bps(&source, &patch).unwrap();
    assert_eq!(result, target);
}

#[test]
fn bps_apply_rejects_too_small() {
    let result = apply_bps(&[], &[0; 8]);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("too small"));
}

#[test]
fn bps_roundtrip_large_rom() {
    // Simulate a ROM-like scenario: large identical regions with scattered changes
    let mut source = vec![0xFFu8; 0x10000];
    for i in (0..source.len()).step_by(0x100) {
        source[i] = (i >> 8) as u8;
    }
    let mut target = source.clone();
    // Scatter 16 changes
    for i in 0..16 {
        target[i * 0x1000 + 0x42] = 0xAA;
    }
    let patch = generate_bps(&source, &target).unwrap();
    let result = apply_bps(&source, &patch).unwrap();
    assert_eq!(result, target);
    // BPS should be much smaller than the full ROM
    assert!(patch.len() < source.len() / 10);
}
