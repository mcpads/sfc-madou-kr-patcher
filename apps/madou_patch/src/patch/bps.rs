//! BPS patch file generation and application.
//!
//! BPS format (beat v1):
//!   "BPS1" magic (4 bytes)
//!   source_size (VLI), target_size (VLI), metadata_size (VLI)
//!   actions: VLI-encoded (type in bits 1:0, length-1 in upper bits)
//!   source_crc32 (4 LE), target_crc32 (4 LE), patch_crc32 (4 LE)

const BPS_MAGIC: &[u8; 4] = b"BPS1";

// ── VLI encoding ────────────────────────────────────────────────────

/// Encode a u64 as a BPS variable-length integer.
fn vli_encode(buf: &mut Vec<u8>, mut data: u64) {
    loop {
        let x = (data & 0x7F) as u8;
        data >>= 7;
        if data == 0 {
            buf.push(0x80 | x);
            break;
        }
        buf.push(x);
        data -= 1;
    }
}

/// Decode a BPS variable-length integer. Returns (value, bytes_consumed).
fn vli_decode(data: &[u8]) -> Result<(u64, usize), String> {
    let mut result: u64 = 0;
    let mut shift: u64 = 1;
    for (i, &byte) in data.iter().enumerate() {
        result += u64::from(byte & 0x7F) * shift;
        if byte & 0x80 != 0 {
            return Ok((result, i + 1));
        }
        shift <<= 7;
        result += shift;
    }
    Err("unexpected end of VLI data".into())
}

// ── BPS creation ────────────────────────────────────────────────────

#[derive(Debug)]
enum Action {
    SourceRead(usize),
    TargetRead(Vec<u8>),
}

fn generate_actions(source: &[u8], target: &[u8]) -> Vec<Action> {
    let mut actions: Vec<Action> = Vec::new();
    let mut pos = 0;

    while pos < target.len() {
        if pos < source.len() && source[pos] == target[pos] {
            let start = pos;
            while pos < target.len() && pos < source.len() && source[pos] == target[pos] {
                pos += 1;
            }
            actions.push(Action::SourceRead(pos - start));
        } else {
            let start = pos;
            while pos < target.len() && (pos >= source.len() || source[pos] != target[pos]) {
                pos += 1;
            }
            actions.push(Action::TargetRead(target[start..pos].to_vec()));
        }
    }

    actions
}

fn encode_action(patch: &mut Vec<u8>, action: &Action) {
    match action {
        Action::SourceRead(length) => {
            let data = (*length as u64 - 1) << 2; // action type 0 = SourceRead
            vli_encode(patch, data);
        }
        Action::TargetRead(bytes) => {
            let data = ((bytes.len() as u64 - 1) << 2) | 1;
            vli_encode(patch, data);
            patch.extend_from_slice(bytes);
        }
    }
}

/// Generate a BPS patch from original and patched ROM data.
pub fn generate_bps(original: &[u8], patched: &[u8]) -> Result<Vec<u8>, String> {
    let mut patch = Vec::new();

    // Header
    patch.extend_from_slice(BPS_MAGIC);
    vli_encode(&mut patch, original.len() as u64);
    vli_encode(&mut patch, patched.len() as u64);
    vli_encode(&mut patch, 0); // no metadata

    // Actions
    let actions = generate_actions(original, patched);
    for action in &actions {
        encode_action(&mut patch, action);
    }

    // Checksums (little-endian CRC32)
    let source_crc = crc32fast::hash(original);
    let target_crc = crc32fast::hash(patched);
    patch.extend_from_slice(&source_crc.to_le_bytes());
    patch.extend_from_slice(&target_crc.to_le_bytes());
    let patch_crc = crc32fast::hash(&patch);
    patch.extend_from_slice(&patch_crc.to_le_bytes());

    Ok(patch)
}

// ── BPS application ─────────────────────────────────────────────────

/// Apply a BPS patch to a source ROM, returning the patched result.
pub fn apply_bps(source: &[u8], patch: &[u8]) -> Result<Vec<u8>, String> {
    if patch.len() < 16 {
        return Err("patch too small".into());
    }
    if &patch[..4] != BPS_MAGIC {
        return Err("not a BPS patch (invalid magic)".into());
    }

    // Verify patch CRC
    let patch_body = &patch[..patch.len() - 4];
    let stored_patch_crc = u32::from_le_bytes(patch[patch.len() - 4..].try_into().unwrap());
    let actual_patch_crc = crc32fast::hash(patch_body);
    if stored_patch_crc != actual_patch_crc {
        return Err(format!(
            "patch CRC mismatch (stored: {stored_patch_crc:08X}, actual: {actual_patch_crc:08X})"
        ));
    }

    // Parse header
    let mut pos = 4;
    let (source_size, n) = vli_decode(&patch[pos..])?;
    pos += n;
    let (target_size, n) = vli_decode(&patch[pos..])?;
    pos += n;
    let (metadata_size, n) = vli_decode(&patch[pos..])?;
    pos += n;
    pos += metadata_size as usize;

    if source.len() as u64 != source_size {
        return Err(format!(
            "source size mismatch (expected: {source_size}, actual: {})",
            source.len()
        ));
    }

    // Verify source CRC
    let footer_start = patch.len() - 12;
    let stored_source_crc =
        u32::from_le_bytes(patch[footer_start..footer_start + 4].try_into().unwrap());
    let actual_source_crc = crc32fast::hash(source);
    if stored_source_crc != actual_source_crc {
        return Err(format!(
            "source CRC mismatch - wrong ROM? (expected: {stored_source_crc:08X}, actual: {actual_source_crc:08X})"
        ));
    }

    // Apply actions
    let mut target = vec![0u8; target_size as usize];
    let mut output_offset: usize = 0;
    let mut source_relative_offset: i64 = 0;
    let mut target_relative_offset: i64 = 0;
    let action_end = footer_start;

    while pos < action_end {
        let (data, n) = vli_decode(&patch[pos..])?;
        pos += n;

        let action = data & 3;
        let length = ((data >> 2) + 1) as usize;

        match action {
            0 => {
                // SourceRead
                for i in 0..length {
                    let src_idx = output_offset + i;
                    target[output_offset + i] =
                        if src_idx < source.len() { source[src_idx] } else { 0 };
                }
                output_offset += length;
            }
            1 => {
                // TargetRead
                if pos + length > action_end {
                    return Err(format!(
                        "TargetRead overflows patch data (pos={}, length={}, end={})",
                        pos, length, action_end
                    ));
                }
                target[output_offset..output_offset + length]
                    .copy_from_slice(&patch[pos..pos + length]);
                pos += length;
                output_offset += length;
            }
            2 => {
                // SourceCopy
                let (offset_data, n) = vli_decode(&patch[pos..])?;
                pos += n;
                let sign = if offset_data & 1 != 0 { -1i64 } else { 1i64 };
                let abs_offset = (offset_data >> 1) as i64;
                source_relative_offset += sign * abs_offset;
                if source_relative_offset < 0
                    || (source_relative_offset as usize + length) > source.len()
                {
                    return Err(format!(
                        "SourceCopy out of bounds (offset={}, length={}, source_len={})",
                        source_relative_offset, length, source.len()
                    ));
                }
                for _ in 0..length {
                    target[output_offset] = source[source_relative_offset as usize];
                    output_offset += 1;
                    source_relative_offset += 1;
                }
            }
            3 => {
                // TargetCopy
                let (offset_data, n) = vli_decode(&patch[pos..])?;
                pos += n;
                let sign = if offset_data & 1 != 0 { -1i64 } else { 1i64 };
                let abs_offset = (offset_data >> 1) as i64;
                target_relative_offset += sign * abs_offset;
                if target_relative_offset < 0
                    || target_relative_offset as usize >= target.len()
                {
                    return Err(format!(
                        "TargetCopy out of bounds (offset={}, target_len={})",
                        target_relative_offset, target.len()
                    ));
                }
                for _ in 0..length {
                    target[output_offset] = target[target_relative_offset as usize];
                    output_offset += 1;
                    target_relative_offset += 1;
                }
            }
            _ => unreachable!(),
        }
    }

    // Verify target CRC
    let stored_target_crc =
        u32::from_le_bytes(patch[footer_start + 4..footer_start + 8].try_into().unwrap());
    let actual_target_crc = crc32fast::hash(&target);
    if stored_target_crc != actual_target_crc {
        return Err(format!(
            "target CRC mismatch (expected: {stored_target_crc:08X}, actual: {actual_target_crc:08X})"
        ));
    }

    Ok(target)
}

#[cfg(test)]
#[path = "bps_tests.rs"]
mod tests;
