//! Dependency-free canonical gzip encoder shared by every execution tier.

/// Compress bytes as an RFC 1952 stream containing stored DEFLATE blocks.
///
/// Stored blocks make the output deterministic and keep the compiler seam free
/// of native compression dependencies. Every standards-compliant gzip decoder
/// accepts the result.
pub fn jet_compress_gzip_compress(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255];
    if data.is_empty() {
        out.extend_from_slice(&[1, 0, 0, 255, 255]);
    } else {
        let mut blocks = data.chunks(u16::MAX as usize).peekable();
        while let Some(block) = blocks.next() {
            out.push(u8::from(blocks.peek().is_none()));
            let len = block.len() as u16;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes());
            out.extend_from_slice(block);
        }
    }
    out.extend_from_slice(&crc32(data).to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gzip_output_is_deterministic() {
        assert_eq!(
            jet_compress_gzip_compress(b"hello"),
            [
                31, 139, 8, 0, 0, 0, 0, 0, 0, 255, 1, 5, 0, 250, 255, 104, 101, 108, 108,
                111, 134, 166, 16, 54, 5, 0, 0, 0,
            ]
        );
    }
}
