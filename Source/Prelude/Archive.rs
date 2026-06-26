// jet.archive runtime (D-DEP-ARCHIVE1) — gzip compress / decompress.
//
// This file is emitted verbatim into the hidden FFI bridge crate (see
// Source/FFI.rs) when a Jet program uses `jet.archive`. The compiler crate
// (`Source/`) never depends on `flate2`; it only ships this text. Owner-
// approved I6 bootstrap exception: the `flate2` crate (pure-Rust, with
// `miniz_oxide` back-end) lives inside the `jet.archive` ring package at
// `stdlibs/jet.archive/vendor/` and is built from that vendored source.
//
// D-BFS1: the vendored source is the canonical offline build path. The
// bridge-crate fallback (cargo fetching from crates.io) is used only when
// `jetpack build jet.archive` has not pre-populated the hangar.

/// Compress `data` with gzip (RFC 1952). Returns the compressed bytes.
/// Compression is always successful on valid input.
pub fn jet_archive_gzip_compress(data: &[u8]) -> Vec<u8> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    // Infallible on a Vec<u8> writer; unwrap is safe.
    let _ = enc.write_all(data);
    enc.finish().unwrap_or_default()
}

/// Decompress gzip-compressed `data`. Returns the decompressed bytes.
/// Returns an empty vec if `data` is not valid gzip.
pub fn jet_archive_gzip_decompress(data: &[u8]) -> Vec<u8> {
    use flate2::read::GzDecoder;
    use std::io::Read;
    let mut dec = GzDecoder::new(data);
    let mut out = Vec::new();
    let _ = dec.read_to_end(&mut out);
    out
}
