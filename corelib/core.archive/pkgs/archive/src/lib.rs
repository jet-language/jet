// core.archive ring package implementation (D-CORE-COMPRESS1=A, D-BFS1).
//
// This is the canonical Rust source for the core.archive module.
// During `jetpack build core.archive`, CoreProvider::realize() compiles this
// crate to an rlib and caches it in the hangar (D-BFS1).
//
// `jet build` / `jet run` embeds this exact file in the hidden FFI bridge via
// crates/jet-pkg-model/src/FFI.rs. Stream codecs live only in core.compress.

use std::io::{Read, Write};

/// ABI version for the package-owned archive kernel. The Jet module source and
/// this implementation are one cache identity; neither is a compiler template.
pub const JET_CORE_ARCHIVE_ABI_VERSION: &str = "core.archive.abi.v1";

pub fn jet_archive_zip_compress(name: &str, data: &[u8]) -> Vec<u8> {
    use zip::write::{FileOptions, ZipWriter};
    let mut buf = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut buf);
        let mut writer = ZipWriter::new(cursor);
        let options: FileOptions<()> = FileOptions::default();
        if writer.start_file(name, options).is_err() {
            return Vec::new();
        }
        if writer.write_all(data).is_err() {
            return Vec::new();
        }
        if writer.finish().is_err() {
            return Vec::new();
        }
    }
    buf
}

pub fn jet_archive_zip_decompress(data: &[u8]) -> Vec<u8> {
    use zip::ZipArchive;
    let cursor = std::io::Cursor::new(data);
    let mut archive = match ZipArchive::new(cursor) {
        Ok(archive) => archive,
        Err(_) => return Vec::new(),
    };
    if archive.is_empty() {
        return Vec::new();
    }
    let mut file = match archive.by_index(0) {
        Ok(file) => file,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    if file.read_to_end(&mut out).is_err() {
        return Vec::new();
    }
    out
}

fn tar_read_all(data: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut entries = Vec::new();
    if data.is_empty() {
        return entries;
    }
    let mut archive = tar::Archive::new(data);
    let Ok(iter) = archive.entries() else {
        return entries;
    };
    for entry in iter {
        let Ok(mut entry) = entry else {
            return Vec::new();
        };
        let name = match entry
            .path()
            .ok()
            .and_then(|path| path.to_str().map(str::to_string))
        {
            Some(name) => name,
            None => return Vec::new(),
        };
        let mut buf = Vec::new();
        if entry.read_to_end(&mut buf).is_err() {
            return Vec::new();
        }
        entries.push((name, buf));
    }
    entries
}

fn tar_write_all(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut archive = tar::Builder::new(&mut buf);
        for (name, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            if archive
                .append_data(&mut header, name, data.as_slice())
                .is_err()
            {
                return Vec::new();
            }
        }
        if archive.finish().is_err() {
            return Vec::new();
        }
    }
    buf
}

pub fn jet_archive_tar_add(archive: &[u8], name: &str, data: &[u8]) -> Vec<u8> {
    let mut entries = tar_read_all(archive);
    if let Some(index) = entries.iter().position(|(entry_name, _)| entry_name == name) {
        entries[index] = (name.to_string(), data.to_vec());
    } else {
        entries.push((name.to_string(), data.to_vec()));
    }
    tar_write_all(&entries)
}

pub fn jet_archive_tar_get(archive: &[u8], name: &str) -> Vec<u8> {
    tar_read_all(archive)
        .into_iter()
        .find_map(|(entry_name, data)| (entry_name == name).then_some(data))
        .unwrap_or_default()
}

pub fn jet_archive_tar_names_json(archive: &[u8]) -> String {
    let mut out = String::from("[");
    for (index, (name, _)) in tar_read_all(archive).iter().enumerate() {
        if index > 0 {
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
                ch => out.push(ch),
            }
        }
        out.push('"');
    }
    out.push(']');
    out
}
