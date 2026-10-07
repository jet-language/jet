//! Private compiler-host admission for the Jet-authored parent runtime.
//! This bootstrap transport dies at cutover; it supplies no runtime semantics.
use crate::NativeLibraryImage::Image;
use jet_foundation::{DataTree::DataTree, JSON, SHA256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CString;
use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    RuntimeLibraryReceiptHashMismatch,
    RuntimeLibraryLibraryHashMismatch,
    RuntimeLibraryExportSealMismatch,
    RuntimeLibraryInitializerSealMismatch,
    RuntimeLibraryIdentityMismatch,
    RuntimeLibraryLoadFailed,
}

impl Refusal {
    /// Stable named refusal key for the host's registered E0956 adapter failure.
    pub fn key(self) -> &'static str {
        match self {
            Self::RuntimeLibraryReceiptHashMismatch => "RuntimeLibraryReceiptHashMismatch",
            Self::RuntimeLibraryLibraryHashMismatch => "RuntimeLibraryLibraryHashMismatch",
            Self::RuntimeLibraryExportSealMismatch => "RuntimeLibraryExportSealMismatch",
            Self::RuntimeLibraryInitializerSealMismatch => "RuntimeLibraryInitializerSealMismatch",
            Self::RuntimeLibraryIdentityMismatch => "RuntimeLibraryIdentityMismatch",
            Self::RuntimeLibraryLoadFailed => "RuntimeLibraryLoadFailed",
        }
    }
}

pub struct RuntimeLibrary {
    image: Image,
    receipt_identity: [u8; 32],
    exports: BTreeSet<String>,
}

impl RuntimeLibrary {
    pub fn receipt_identity(&self) -> &[u8; 32] { &self.receipt_identity }

    /// The returned address borrows the process-pinned runtime. Callers must
    /// marshal the declared C ABI; pinned Core getters are not runtime symbols.
    pub fn symbol(&self, name: &str) -> Result<usize, Refusal> {
        if !self.exports.contains(name) { return Err(Refusal::RuntimeLibraryExportSealMismatch); }
        let name = CString::new(name).map_err(|_| Refusal::RuntimeLibraryExportSealMismatch)?;
        self.image.symbol(&name).map(|address| address as usize).map_err(|_| Refusal::RuntimeLibraryLoadFailed)
    }
}

struct Resident {
    identity: [u8; 32],
    result: Result<RuntimeLibrary, Refusal>,
}
static RESIDENT: OnceLock<Resident> = OnceLock::new();

/// `authoritative_receipt_sha256` comes from the compiler's admitted record,
/// never the candidate's self-declared hash or a user environment variable.
/// A different runtime cannot replace a resident instance. The process-owned
/// OnceLock is never dropped, so successful initialization happens only once.
pub fn admit(path: &Path, receipt_bytes: &[u8], authoritative_receipt_sha256: &[u8; 32]) -> Result<&'static RuntimeLibrary, Refusal> {
    if SHA256::sha256(receipt_bytes) != *authoritative_receipt_sha256 {
        return Err(Refusal::RuntimeLibraryReceiptHashMismatch);
    }
    let resident = RESIDENT.get_or_init(|| Resident {
        identity: *authoritative_receipt_sha256,
        result: load(path, receipt_bytes, *authoritative_receipt_sha256),
    });
    if resident.identity != *authoritative_receipt_sha256 { return Err(Refusal::RuntimeLibraryIdentityMismatch); }
    resident.result.as_ref().map_err(|error| *error)
}

fn text<'a>(value: &'a DataTree, key: &str) -> Result<&'a str, Refusal> {
    value.get(key).and_then(DataTree::as_str).map_err(|_| Refusal::RuntimeLibraryReceiptHashMismatch)
}

fn read_library(path: &Path) -> Result<Vec<u8>, Refusal> {
    let file = std::fs::File::open(path).map_err(|_| Refusal::RuntimeLibraryLoadFailed)?;
    let size = file.metadata().map_err(|_| Refusal::RuntimeLibraryLoadFailed)?.len();
    if size == 0 || size > 256 * 1024 * 1024 { return Err(Refusal::RuntimeLibraryLoadFailed); }
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(256 * 1024 * 1024 + 1).read_to_end(&mut bytes).map_err(|_| Refusal::RuntimeLibraryLoadFailed)?;
    if bytes.len() != size as usize { return Err(Refusal::RuntimeLibraryLoadFailed); }
    Ok(bytes)
}

fn load(path: &Path, receipt_bytes: &[u8], identity: [u8; 32]) -> Result<RuntimeLibrary, Refusal> {
    let receipt = JSON::parse_json(std::str::from_utf8(receipt_bytes).map_err(|_| Refusal::RuntimeLibraryReceiptHashMismatch)?)
        .map_err(|_| Refusal::RuntimeLibraryReceiptHashMismatch)?;
    if text(&receipt, "kind")? != "jet-parent-runtime-candidate" || text(&receipt, "optimization")? != "O0" {
        return Err(Refusal::RuntimeLibraryReceiptHashMismatch);
    }
    let expected = receipt.get("exports").and_then(DataTree::as_array).map_err(|_| Refusal::RuntimeLibraryExportSealMismatch)?;
    let mut exports = BTreeSet::new();
    for row in expected {
        let name = row.as_str().map_err(|_| Refusal::RuntimeLibraryExportSealMismatch)?;
        if !name.starts_with("jet_") || !name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') || !exports.insert(name.to_owned()) {
            return Err(Refusal::RuntimeLibraryExportSealMismatch);
        }
    }
    if exports.is_empty() { return Err(Refusal::RuntimeLibraryExportSealMismatch); }
    let bytes = read_library(path)?;
    if SHA256::sha256_hex(&bytes) != text(&receipt, "librarySHA256")? { return Err(Refusal::RuntimeLibraryLibraryHashMismatch); }
    let elf = Elf::new(&bytes)?;
    let actual = elf.exports()?;
    if actual.len() != exports.len() || actual.keys().any(|name| !exports.contains(*name)) { return Err(Refusal::RuntimeLibraryExportSealMismatch); }
    let lifecycle = receipt.get("lifecycle").map_err(|_| Refusal::RuntimeLibraryInitializerSealMismatch)?;
    for (kind, section_type) in [("init", 14), ("fini", 15)] {
        let entries = lifecycle.get(kind).and_then(DataTree::as_array).map_err(|_| Refusal::RuntimeLibraryInitializerSealMismatch)?;
        let mut names = BTreeSet::new();
        let mut addresses = Vec::with_capacity(entries.len());
        let mut last_priority = None;
        for row in entries {
            let name = text(row, "name").map_err(|_| Refusal::RuntimeLibraryInitializerSealMismatch)?;
            let priority = match row.get("priority") {
                Ok(DataTree::Int(value)) if (0..=65535).contains(value) => *value,
                _ => return Err(Refusal::RuntimeLibraryInitializerSealMismatch),
            };
            if last_priority.is_some_and(|previous| previous > priority) || !names.insert(name) {
                return Err(Refusal::RuntimeLibraryInitializerSealMismatch);
            }
            last_priority = Some(priority);
            addresses.push(*actual.get(name).ok_or(Refusal::RuntimeLibraryInitializerSealMismatch)?);
        }
        if kind == "init" && addresses.is_empty() { return Err(Refusal::RuntimeLibraryInitializerSealMismatch); }
        elf.lifecycle(section_type, &addresses)?;
    }
    // No symbol use and no constructor execution precede the byte/seal checks.
    let image = Image::open_sealed(&bytes).map_err(|_| Refusal::RuntimeLibraryLoadFailed)?;
    Ok(RuntimeLibrary { image, receipt_identity: identity, exports })
}

#[derive(Clone, Copy)]
struct Section {
    kind: u32,
    address: u64,
    offset: usize,
    size: usize,
    link: usize,
    entry_size: usize,
}
struct Elf<'a> { bytes: &'a [u8], sections: Vec<Section> }

impl<'a> Elf<'a> {
    fn new(bytes: &'a [u8]) -> Result<Self, Refusal> {
        let mut elf = Self { bytes, sections: Vec::new() };
        if elf.span(0, 6)? != [0x7f, b'E', b'L', b'F', 2, 1] || elf.u16(16)? != 3 || elf.u16(18)? != 62 || elf.u64(24)? != 0 {
            return Err(Refusal::RuntimeLibraryExportSealMismatch);
        }
        let start = elf.offset(40)?;
        let width = elf.u16(58)? as usize;
        let count = elf.u16(60)? as usize;
        if width != 64 || count == 0 { return Err(Refusal::RuntimeLibraryExportSealMismatch); }
        elf.span(start, count.checked_mul(width).ok_or(Refusal::RuntimeLibraryExportSealMismatch)?)?;
        for index in 0..count {
            let at = start + index * width;
            let section = Section { kind: elf.u32(at + 4)?, address: elf.u64(at + 16)?, offset: elf.offset(at + 24)?, size: elf.offset(at + 32)?, link: elf.u32(at + 40)? as usize, entry_size: elf.offset(at + 56)? };
            if (elf.u64(at + 8)? & 5) == 5 { return Err(Refusal::RuntimeLibraryExportSealMismatch); }
            if section.kind != 8 { elf.span(section.offset, section.size)?; }
            elf.sections.push(section);
        }
        let start = elf.offset(32)?;
        let count = elf.u16(56)? as usize;
        if elf.u16(54)? != 56 || count == 0 { return Err(Refusal::RuntimeLibraryExportSealMismatch); }
        elf.span(start, count.checked_mul(56).ok_or(Refusal::RuntimeLibraryExportSealMismatch)?)?;
        let mut relro = false;
        for index in 0..count {
            let at = start + index * 56;
            let kind = elf.u32(at)?;
            let flags = elf.u32(at + 4)?;
            if (kind == 1 && flags & 3 == 3) || (kind == 0x6474e551 && flags & 1 != 0) { return Err(Refusal::RuntimeLibraryExportSealMismatch); }
            relro |= kind == 0x6474e552;
        }
        if !relro { return Err(Refusal::RuntimeLibraryExportSealMismatch); }
        for section in &elf.sections {
            if section.kind == 6 {
                if section.size % 16 != 0 { return Err(Refusal::RuntimeLibraryExportSealMismatch); }
                for offset in (0..section.size).step_by(16) {
                    let tag = elf.u64(section.offset + offset)?;
                    let value = elf.u64(section.offset + offset + 8)?;
                    if tag == 22 || (tag == 30 && value & 4 != 0) { return Err(Refusal::RuntimeLibraryExportSealMismatch); }
                }
            }
        }
        Ok(elf)
    }
    fn span(&self, offset: usize, count: usize) -> Result<&'a [u8], Refusal> {
        self.bytes.get(offset..offset.checked_add(count).ok_or(Refusal::RuntimeLibraryExportSealMismatch)?).ok_or(Refusal::RuntimeLibraryExportSealMismatch)
    }
    fn u16(&self, at: usize) -> Result<u16, Refusal> { Ok(u16::from_le_bytes(self.span(at, 2)?.try_into().expect("two bytes"))) }
    fn u32(&self, at: usize) -> Result<u32, Refusal> { Ok(u32::from_le_bytes(self.span(at, 4)?.try_into().expect("four bytes"))) }
    fn u64(&self, at: usize) -> Result<u64, Refusal> { Ok(u64::from_le_bytes(self.span(at, 8)?.try_into().expect("eight bytes"))) }
    fn offset(&self, at: usize) -> Result<usize, Refusal> { usize::try_from(self.u64(at)?).map_err(|_| Refusal::RuntimeLibraryExportSealMismatch) }
    fn symbols(&self, table: Section) -> Result<impl Iterator<Item = usize>, Refusal> {
        if table.entry_size != 24 || table.size % 24 != 0 || table.link >= self.sections.len() { return Err(Refusal::RuntimeLibraryExportSealMismatch); }
        self.span(table.offset, table.size)?;
        Ok((0..table.size / 24).map(move |index| table.offset + index * 24))
    }
    fn symbol_name(&self, table: Section, at: usize) -> Result<&'a str, Refusal> {
        let strings = self.sections[table.link];
        let bytes = self.span(strings.offset, strings.size)?;
        let tail = bytes.get(self.u32(at)? as usize..).ok_or(Refusal::RuntimeLibraryExportSealMismatch)?;
        let end = tail.iter().position(|byte| *byte == 0).ok_or(Refusal::RuntimeLibraryExportSealMismatch)?;
        std::str::from_utf8(&tail[..end]).map_err(|_| Refusal::RuntimeLibraryExportSealMismatch)
    }
    fn exports(&self) -> Result<BTreeMap<&'a str, u64>, Refusal> {
        let tables: Vec<_> = self.sections.iter().filter(|section| section.kind == 11).collect();
        if tables.len() != 1 { return Err(Refusal::RuntimeLibraryExportSealMismatch); }
        let table = *tables[0];
        let mut result = BTreeMap::new();
        for at in self.symbols(table)? {
            let info = self.span(at + 4, 1)?[0];
            if info >> 4 == 0 || self.u16(at + 6)? == 0 { continue; }
            let name = self.symbol_name(table, at)?;
            if info != 0x12 || name.is_empty() || result.insert(name, self.u64(at + 8)?).is_some() { return Err(Refusal::RuntimeLibraryExportSealMismatch); }
        }
        Ok(result)
    }
    fn lifecycle(&self, kind: u32, expected: &[u64]) -> Result<(), Refusal> {
        let refusal = Refusal::RuntimeLibraryInitializerSealMismatch;
        let arrays: Vec<_> = self.sections.iter().filter(|section| section.kind == kind).collect();
        if arrays.is_empty() && expected.is_empty() { return Ok(()); }
        if arrays.len() != 1 || arrays[0].size != expected.len() * 8 { return Err(refusal); }
        let array = *arrays[0];
        for (index, expected) in expected.iter().enumerate() {
            let address = array.address.checked_add(index as u64 * 8).ok_or(refusal)?;
            let mut target = self.u64(array.offset + index * 8)?;
            let mut relocated = false;
            for relocation in self.sections.iter().filter(|section| section.kind == 4) {
                if relocation.entry_size != 24 || relocation.size % 24 != 0 || relocation.link >= self.sections.len() { return Err(refusal); }
                for offset in (0..relocation.size).step_by(24) {
                    let at = relocation.offset + offset;
                    if self.u64(at)? != address { continue; }
                    if relocated { return Err(refusal); }
                    relocated = true;
                    let info = self.u64(at + 8)?;
                    let addend = self.u64(at + 16)? as i64;
                    target = match info as u32 {
                        8 if addend >= 0 => addend as u64,
                        1 => {
                            let table = self.sections[relocation.link];
                            let symbols = self.symbols(table)?;
                            let at = symbols.skip((info >> 32) as usize).next().ok_or(refusal)?;
                            self.u64(at + 8)?.checked_add_signed(addend).ok_or(refusal)?
                        }
                        _ => return Err(refusal),
                    };
                }
            }
            if target != *expected { return Err(refusal); }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refusal_keys_are_stable_and_library_is_send_sync() {
        fn send_sync<T: Send + Sync>() {}
        send_sync::<RuntimeLibrary>();
        assert_eq!(Refusal::RuntimeLibraryIdentityMismatch.key(), "RuntimeLibraryIdentityMismatch");
        assert!(Elf::new(b"not ELF").is_err());
    }
    #[test]
    fn mismatched_receipt_refuses_before_opening_a_path() {
        assert!(matches!(admit(Path::new("/path/does/not/exist"), b"{}", &[0; 32]), Err(Refusal::RuntimeLibraryReceiptHashMismatch)));
    }
}
