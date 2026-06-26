// jet.archive ring package implementation (D-DEP-ARCHIVE1=A, D-BFS1).
//
// This is the canonical Rust source for the jet.archive module.
// During `jetpack build jet.archive`, CoreProvider::realize() compiles this
// crate to an rlib and caches it in the hangar (D-BFS1).
//
// For `jet build` / `jet run`, the same functions are also available through
// the hidden FFI bridge (Source/FFI.rs → Source/Prelude/Archive.rs), which
// uses the same `flate2` crate fetched from crates.io (or the vendor/ dir when
// the ring package is pre-built in the hangar). Both code paths produce
// identical behaviour — the bridge functions delegate to these.

use flate2::write::GzEncoder;
use flate2::read::GzDecoder;
use flate2::Compression;
use std::io::{Read, Write};

/// Compress `data` with gzip (RFC 1952). Returns the compressed bytes.
pub fn jet_archive_gzip_compress(data: &[u8]) -> Vec<u8> {
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    let _ = enc.write_all(data);
    enc.finish().unwrap_or_default()
}

/// Decompress gzip-compressed `data`. Returns the decompressed bytes.
/// Returns an empty vec if `data` is not valid gzip.
pub fn jet_archive_gzip_decompress(data: &[u8]) -> Vec<u8> {
    let mut dec = GzDecoder::new(data);
    let mut out = Vec::new();
    let _ = dec.read_to_end(&mut out);
    out
}
