// core.archive runtime (D-DEP-ARCHIVE1) — gzip compress / decompress.
//
// This file is emitted verbatim into the hidden FFI bridge crate (see
// Source/FFI.rs) when a Jet program uses `core.archive`. The compiler crate
// (`Source/`) never depends on `flate2`; it only ships this text. Owner-
// approved I6 bootstrap exception: the `flate2` crate (pure-Rust, with
// `miniz_oxide` back-end) lives inside the `core.archive` ring package at
// `corelib/core.archive/vendor/` and is built from that vendored source.
//
// D-BFS1: the vendored source is the canonical offline build path. The
// bridge-crate fallback (cargo fetching from crates.io) is used only when
// `jetpack build core.archive` has not pre-populated the hangar.

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

/// Compress `data` as a zip archive with a single entry named `name`.
/// Returns the zip bytes.
pub fn jet_archive_zip_compress(name: &str, data: &[u8]) -> Vec<u8> {
    use zip::write::{FileOptions, ZipWriter};
    use std::io::Write;
    let mut buf = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut buf);
        let mut w = ZipWriter::new(cursor);
        let opts: FileOptions<()> = FileOptions::default();
        let _ = w.start_file(name, opts);
        let _ = w.write_all(data);
        let _ = w.finish();
    }
    buf
}

/// Decompress the first file in a zip archive. Returns the raw bytes.
/// Returns an empty vec on invalid input or an empty archive.
pub fn jet_archive_zip_decompress(data: &[u8]) -> Vec<u8> {
    use zip::ZipArchive;
    use std::io::Read;
    let cursor = std::io::Cursor::new(data);
    let mut archive = match ZipArchive::new(cursor) {
        Ok(a) => a,
        Err(_) => return Vec::new(),
    };
    if archive.len() == 0 {
        return Vec::new();
    }
    let mut file = match archive.by_index(0) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    let _ = file.read_to_end(&mut out);
    out
}

// ---- tar support (D-DEP-ARCHIVE1=A) ----------------------------------------

/// Read all entries from a tar archive. Returns (name, data) pairs.
fn tar_read_all(data: &[u8]) -> Vec<(String, Vec<u8>)> {
    use tar::Archive;
    use std::io::Read;
    let mut entries = Vec::new();
    if data.is_empty() {
        return entries;
    }
    let mut ar = Archive::new(data);
    let Ok(iter) = ar.entries() else { return entries };
    for entry in iter.flatten() {
        let mut e = entry;
        let name = e.path()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .unwrap_or_default();
        let mut buf = Vec::new();
        let _ = e.read_to_end(&mut buf);
        entries.push((name, buf));
    }
    entries
}

/// Write (name, data) pairs into a fresh tar archive. Returns the tar bytes.
fn tar_write_all(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    use tar::{Builder, Header};
    let mut buf = Vec::new();
    {
        let mut ar = Builder::new(&mut buf);
        for (name, data) in entries {
            let mut header = Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            let _ = ar.append_data(&mut header, name, data.as_slice());
        }
        let _ = ar.finish();
    }
    buf
}

/// Append one file entry to a tar archive. Returns the new tar bytes.
/// If `archive` is empty, a fresh archive is created.
/// If an entry with the same name already exists, it is replaced.
pub fn jet_archive_tar_add(archive: &[u8], name: &str, data: &[u8]) -> Vec<u8> {
    let mut entries = tar_read_all(archive);
    // Replace existing entry with the same name, or append.
    if let Some(pos) = entries.iter().position(|(n, _)| n == name) {
        entries[pos] = (name.to_string(), data.to_vec());
    } else {
        entries.push((name.to_string(), data.to_vec()));
    }
    tar_write_all(&entries)
}

/// Extract one file by name from a tar archive. Returns the file bytes.
/// Returns an empty vec if the entry is not found or the archive is invalid.
pub fn jet_archive_tar_get(archive: &[u8], name: &str) -> Vec<u8> {
    for (n, data) in tar_read_all(archive) {
        if n == name {
            return data;
        }
    }
    Vec::new()
}

/// List all entry names in a tar archive as a JSON array string.
/// Returns `"[]"` on an empty or invalid archive.
pub fn jet_archive_tar_names_json(archive: &[u8]) -> String {
    let entries = tar_read_all(archive);
    let mut out = String::from("[");
    for (i, (name, _)) in entries.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('"');
        for ch in name.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c => out.push(c),
            }
        }
        out.push('"');
    }
    out.push(']');
    out
}
