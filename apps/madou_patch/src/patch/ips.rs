//! IPS patch file generation.
//!
//! IPS format:
//!   "PATCH" header (5 bytes)
//!   Records: offset(3 bytes BE) + size(2 bytes BE) + data
//!   "EOF" footer (3 bytes)

/// Generate an IPS patch from original and patched ROM data.
pub fn generate_ips(original: &[u8], patched: &[u8]) -> Vec<u8> {
    let mut ips = Vec::new();

    // Header
    ips.extend_from_slice(b"PATCH");

    let len = original.len().min(patched.len());
    let mut i = 0;

    while i < len {
        // Skip identical bytes
        if original[i] == patched[i] {
            i += 1;
            continue;
        }

        // Found a difference — scan for end of changed region
        let start = i;
        while i < len && original[i] != patched[i] {
            i += 1;
            // Also merge small gaps (≤8 identical bytes) to avoid tiny records
            if i < len && original[i] == patched[i] {
                let mut gap = 0;
                let mut j = i;
                while j < len && original[j] == patched[j] && gap < 8 {
                    gap += 1;
                    j += 1;
                }
                if j < len && original[j] != patched[j] {
                    i = j; // merge the gap
                }
            }
        }

        let data = &patched[start..i];

        // Split into chunks of max 0xFFFF bytes (IPS size field is 16-bit)
        let mut offset = start;
        let mut remaining = data;
        while !remaining.is_empty() {
            let chunk_size = remaining.len().min(0xFFFF);
            let chunk = &remaining[..chunk_size];

            // Offset (3 bytes big-endian)
            ips.push(((offset >> 16) & 0xFF) as u8);
            ips.push(((offset >> 8) & 0xFF) as u8);
            ips.push((offset & 0xFF) as u8);

            // Size (2 bytes big-endian)
            ips.push(((chunk_size >> 8) & 0xFF) as u8);
            ips.push((chunk_size & 0xFF) as u8);

            // Data
            ips.extend_from_slice(chunk);

            offset += chunk_size;
            remaining = &remaining[chunk_size..];
        }
    }

    // Footer
    ips.extend_from_slice(b"EOF");

    ips
}

/// Count IPS records in a patch.
pub fn count_records(ips: &[u8]) -> usize {
    if ips.len() < 8 || &ips[..5] != b"PATCH" {
        return 0;
    }

    let mut count = 0;
    let mut i = 5;
    while i + 3 <= ips.len() {
        // Check for EOF
        if &ips[i..i + 3] == b"EOF" {
            break;
        }
        if i + 5 > ips.len() {
            break;
        }

        let size = ((ips[i + 3] as usize) << 8) | (ips[i + 4] as usize);
        if size == 0 {
            // RLE record
            i += 5 + 3; // offset(3) + size(2) + rle_size(2) + value(1)
        } else {
            i += 5 + size;
        }
        count += 1;
    }
    count
}

#[cfg(test)]
#[path = "ips_tests.rs"]
mod tests;
