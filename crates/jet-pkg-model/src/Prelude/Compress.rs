// core.archive codec runtime (D-CORE-COMPRESS1=A / D-CODECS1) — gzip/zstd streams.
//
// This file is emitted verbatim into the hidden FFI bridge crate (see
// Source/FFI.rs) when a Jet program uses `core.archive.gzip` or
// `core.archive.zstd`. The dependency-free canonical gzip encoder is emitted
// separately from `jet-foundation/src/GzipKernel.rs`; this file owns native
// gzip decoding and zstd.
//
// Decompression is fallible end-to-end: a malformed compressed stream is
// safety-critical misuse and must surface as a Jet `Result` `Err`, not a
// silent empty buffer.


/// Decompress gzip-compressed `data`. Returns an error message if `data` is
/// not a valid gzip stream.
pub fn jet_compress_gzip_decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    use flate2::read::GzDecoder;
    use std::io::Read;
    const MAX_OUTPUT: u64 = 64 * 1024 * 1024;
    let mut dec = GzDecoder::new(data);
    let mut out = Vec::new();
    dec.by_ref()
        .take(MAX_OUTPUT + 1)
        .read_to_end(&mut out)
        .map_err(|e| format!("archive.gzip.decompress: invalid gzip data: {e}"))?;
    if out.len() as u64 > MAX_OUTPUT {
        return Err("archive.gzip.decompress: output exceeds 64 MiB".to_string());
    }
    Ok(out)
}

/// Compress `data` with zstd at the default compression level.
/// Returns the compressed bytes. Compression is always successful on valid input.
pub fn jet_compress_zstd_compress(data: &[u8]) -> Vec<u8> {
    zstd::stream::encode_all(data, 0).unwrap_or_default()
}

/// Decompress zstd-compressed `data`. Returns an error message if `data` is
/// not a valid zstd frame.
pub fn jet_compress_zstd_decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Read;
    const MAX_OUTPUT: u64 = 64 * 1024 * 1024;
    let mut dec = zstd::stream::read::Decoder::new(data)
        .map_err(|e| format!("archive.zstd.decompress: invalid zstd data: {e}"))?;
    let mut out = Vec::new();
    dec.by_ref()
        .take(MAX_OUTPUT + 1)
        .read_to_end(&mut out)
        .map_err(|e| format!("archive.zstd.decompress: invalid zstd data: {e}"))?;
    if out.len() as u64 > MAX_OUTPUT {
        return Err("archive.zstd.decompress: output exceeds 64 MiB".to_string());
    }
    Ok(out)
}
