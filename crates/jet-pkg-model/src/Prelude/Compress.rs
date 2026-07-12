// core.compress runtime (D-CORE-COMPRESS1=A / D-CODECS1) — gzip/zstd streams.
//
// This file is emitted verbatim into the hidden FFI bridge crate (see
// Source/FFI.rs) when a Jet program uses `core.compress.gzip` or
// `core.compress.zstd`. The compiler crate (`Source/`) never depends on
// `flate2` or `zstd`; it only ships this text. Owner-approved I6 bootstrap
// exception (same approved dependency family as D-DEP-ARCHIVE1): `flate2`
// (pure-Rust, `miniz_oxide` back-end) and `zstd` (Rust binding, vendors and
// builds the C zstd source via `zstd-sys`) live inside the `core.compress`
// ring package and are built from vendored/fetched source. Native-ize
// obligation before the end of Epoch 3 (I6).
//
// Decompression is fallible end-to-end: a
// malformed compressed stream is safety-critical misuse and must surface
// as a Jet `Result` `Err`, not a silent empty buffer.

/// Compress `data` with gzip (RFC 1952) at the default compression level.
/// Returns the compressed bytes. Compression is always successful on valid input.
pub fn jet_compress_gzip_compress(data: &[u8]) -> Vec<u8> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    // Infallible on a Vec<u8> writer; unwrap is safe.
    let _ = enc.write_all(data);
    enc.finish().unwrap_or_default()
}

/// Decompress gzip-compressed `data`. Returns an error message if `data` is
/// not a valid gzip stream.
pub fn jet_compress_gzip_decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    use flate2::read::GzDecoder;
    use std::io::Read;
    let mut dec = GzDecoder::new(data);
    let mut out = Vec::new();
    dec.read_to_end(&mut out)
        .map_err(|e| format!("compress.gzip.decompress: invalid gzip data: {e}"))?;
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
    zstd::stream::decode_all(data)
        .map_err(|e| format!("compress.zstd.decompress: invalid zstd data: {e}"))
}
