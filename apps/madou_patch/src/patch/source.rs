//! Supported immutable source-ROM identity.

use sha2::{Digest, Sha256};

const SOURCE_LEN: usize = 0x20_0000;
const SOURCE_SHA256: [u8; 32] = [
    0x72, 0xE7, 0xFF, 0x78, 0x57, 0xF5, 0x77, 0x80, 0x9B, 0x58, 0x5F, 0x67, 0xF9, 0xC6, 0xB2, 0x21,
    0x1E, 0x66, 0x48, 0xC4, 0xF8, 0x65, 0x19, 0xFC, 0x6C, 0x66, 0xE3, 0xF4, 0xBF, 0xD4, 0x9F, 0x59,
];

/// Reject any input other than the supported headerless Japanese revision.
pub fn verify_source_rom(data: &[u8]) -> Result<(), String> {
    if data.len() != SOURCE_LEN {
        return Err(format!(
            "Unsupported source ROM size: expected {SOURCE_LEN} bytes, got {}",
            data.len()
        ));
    }

    let digest = Sha256::digest(data);
    if digest.as_slice() != SOURCE_SHA256 {
        return Err(format!(
            "Unsupported source ROM SHA-256: expected {}, got {:x}",
            hex_digest(&SOURCE_SHA256),
            digest
        ));
    }
    Ok(())
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrong_size_is_rejected() {
        assert!(verify_source_rom(&[]).is_err());
    }

    #[test]
    fn wrong_digest_is_rejected() {
        assert!(verify_source_rom(&vec![0; SOURCE_LEN]).is_err());
    }
}
