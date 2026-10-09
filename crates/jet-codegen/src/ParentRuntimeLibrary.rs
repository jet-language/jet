//! Private compiler-host admission for the Jet-authored parent runtime.
//! This bootstrap transport dies at cutover; it supplies no runtime semantics.
use crate::NativeLibraryImage::Image;
use jet_foundation::{Dyn::Dyn, JSON, SHA256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CString;
use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

/// Compiler-private build constants from the same producer as the installed .so.
/// No runtime environment/CLI option/receipt file selects this record.
pub struct EmbeddedAdmission {
    pub library_install_relative: &'static str,
    pub expected_library_sha256: [u8; 32],
    pub expected_receipt_sha256: [u8; 32],
    pub receipt_bytes: &'static [u8],
}

#[cfg(jet_parent_runtime_admission)]
mod embedded {
    include!(concat!(env!("OUT_DIR"), "/parent-runtime-admission.rs"));
    pub(super) static RECORD: super::EmbeddedAdmission = super::EmbeddedAdmission {
        library_install_relative: LIBRARY_INSTALL_RELATIVE,
        expected_library_sha256: EXPECTED_LIBRARY_SHA256,
        expected_receipt_sha256: EXPECTED_RECEIPT_SHA256,
        receipt_bytes: RECEIPT_BYTES,
    };
}

/// None means no actual runtime artifact was supplied to the host build. The
/// Source consumer must retain its registered missing-runtime refusal then.
pub fn embedded_admission() -> Option<&'static EmbeddedAdmission> {
    #[cfg(jet_parent_runtime_admission)]
    {
        Some(&embedded::RECORD)
    }
    #[cfg(not(jet_parent_runtime_admission))]
    {
        None
    }
}

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

struct Symbol {
    name: CString,
    address: OnceLock<Result<usize, Refusal>>,
}

pub struct RuntimeLibrary {
    image: Image,
    receipt_identity: [u8; 32],
    library_identity: [u8; 32],
    exports: Vec<Symbol>,
}

impl RuntimeLibrary {
    pub fn receipt_identity(&self) -> &[u8; 32] {
        &self.receipt_identity
    }

    /// The actual library-byte SHA validated before initialization, cached once.
    pub fn library_identity(&self) -> &[u8; 32] {
        &self.library_identity
    }

    /// The returned address borrows the process-pinned runtime. Callers must
    /// marshal the declared C ABI; pinned Core getters are not runtime symbols.
    pub fn symbol(&self, name: &str) -> Result<usize, Refusal> {
        let index = self
            .exports
            .binary_search_by(|symbol| symbol.name.to_bytes().cmp(name.as_bytes()))
            .map_err(|_| Refusal::RuntimeLibraryExportSealMismatch)?;
        let symbol = &self.exports[index];
        *symbol.address.get_or_init(|| {
            self.image
                .symbol(&symbol.name)
                .map(|address| address as usize)
                .map_err(|_| Refusal::RuntimeLibraryLoadFailed)
        })
    }
}

struct Resident {
    identity: [u8; 32],
    result: Result<RuntimeLibrary, Refusal>,
}
static RESIDENT: OnceLock<Resident> = OnceLock::new();

struct Prepared {
    bytes: Vec<u8>,
    library_identity: [u8; 32],
    exports: Vec<Symbol>,
}

/// `authoritative_receipt_sha256` comes from the compiler's admitted record,
/// never the candidate's self-declared hash or a user environment variable.
/// A different runtime cannot replace a resident instance. The process-owned
/// OnceLock is never dropped, so successful initialization happens only once.
pub fn admit(
    path: &Path,
    receipt_bytes: &[u8],
    authoritative_receipt_sha256: &[u8; 32],
) -> Result<&'static RuntimeLibrary, Refusal> {
    if SHA256::sha256(receipt_bytes) != *authoritative_receipt_sha256 {
        return Err(Refusal::RuntimeLibraryReceiptHashMismatch);
    }
    if let Some(resident) = RESIDENT.get() {
        if resident.identity != *authoritative_receipt_sha256 {
            return Err(Refusal::RuntimeLibraryIdentityMismatch);
        }
        return resident.result.as_ref().map_err(|error| *error);
    }
    // Hash/file/ELF refusals are stateless. Only an actual open attempt can
    // have executed constructors and therefore pins this process's identity.
    let prepared = prepare(path, receipt_bytes)?;
    let resident = RESIDENT.get_or_init(|| Resident {
        identity: *authoritative_receipt_sha256,
        result: Image::open_sealed(&prepared.bytes)
            .map(|image| RuntimeLibrary {
                image,
                receipt_identity: *authoritative_receipt_sha256,
                library_identity: prepared.library_identity,
                exports: prepared.exports,
            })
            .map_err(|_| Refusal::RuntimeLibraryLoadFailed),
    });
    if resident.identity != *authoritative_receipt_sha256 {
        return Err(Refusal::RuntimeLibraryIdentityMismatch);
    }
    resident.result.as_ref().map_err(|error| *error)
}

fn text<'a>(value: &'a Dyn, key: &str) -> Result<&'a str, Refusal> {
    value
        .get(key)
        .and_then(Dyn::as_str)
        .map_err(|_| Refusal::RuntimeLibraryReceiptHashMismatch)
}

fn read_library(path: &Path) -> Result<Vec<u8>, Refusal> {
    let file = std::fs::File::open(path).map_err(|_| Refusal::RuntimeLibraryLoadFailed)?;
    let size = file
        .metadata()
        .map_err(|_| Refusal::RuntimeLibraryLoadFailed)?
        .len();
    if size == 0 || size > 256 * 1024 * 1024 {
        return Err(Refusal::RuntimeLibraryLoadFailed);
    }
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(256 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Refusal::RuntimeLibraryLoadFailed)?;
    if bytes.len() != size as usize {
        return Err(Refusal::RuntimeLibraryLoadFailed);
    }
    Ok(bytes)
}

fn prepare(path: &Path, receipt_bytes: &[u8]) -> Result<Prepared, Refusal> {
    let receipt = JSON::parse_json(
        std::str::from_utf8(receipt_bytes)
            .map_err(|_| Refusal::RuntimeLibraryReceiptHashMismatch)?,
    )
    .map_err(|_| Refusal::RuntimeLibraryReceiptHashMismatch)?;
    if text(&receipt, "kind")? != "jet-parent-runtime-candidate"
        || text(&receipt, "optimization")? != "O0"
    {
        return Err(Refusal::RuntimeLibraryReceiptHashMismatch);
    }
    let expected = receipt
        .get("exports")
        .and_then(Dyn::as_array)
        .map_err(|_| Refusal::RuntimeLibraryExportSealMismatch)?;
    let mut exports = BTreeSet::new();
    for row in expected {
        let name = row
            .as_str()
            .map_err(|_| Refusal::RuntimeLibraryExportSealMismatch)?;
        if !name.starts_with("jet_")
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            || !exports.insert(name.to_owned())
        {
            return Err(Refusal::RuntimeLibraryExportSealMismatch);
        }
    }
    if exports.is_empty() {
        return Err(Refusal::RuntimeLibraryExportSealMismatch);
    }
    // Primitive seed transport only: Source decodes and governs layout classes.
    // Bind the mandatory ABI rows to this same sealed symbol inventory.
    let abi = receipt
        .get("exportABI")
        .and_then(Dyn::as_array)
        .map_err(|_| Refusal::RuntimeLibraryExportSealMismatch)?;
    if abi.len() != exports.len() {
        return Err(Refusal::RuntimeLibraryExportSealMismatch);
    }
    for (row, name) in abi.iter().zip(exports.iter()) {
        if row.get("symbol").and_then(Dyn::as_str)
            .map_err(|_| Refusal::RuntimeLibraryExportSealMismatch)? != name.as_str()
        {
            return Err(Refusal::RuntimeLibraryExportSealMismatch);
        }
    }
    let bytes = read_library(path)?;
    let library_identity = SHA256::sha256(&bytes);
    let expected_library = text(&receipt, "librarySHA256")?.as_bytes();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    if expected_library.len() != 64
        || library_identity.iter().enumerate().any(|(index, byte)| {
            expected_library[index * 2] != HEX[usize::from(byte >> 4)]
                || expected_library[index * 2 + 1] != HEX[usize::from(byte & 15)]
        })
    {
        return Err(Refusal::RuntimeLibraryLibraryHashMismatch);
    }
    let elf = Elf::new(&bytes)?;
    let actual = elf.exports()?;
    if actual.len() != exports.len() || actual.keys().any(|name| !exports.contains(*name)) {
        return Err(Refusal::RuntimeLibraryExportSealMismatch);
    }
    exact_names(&receipt, "imports", &elf.imports()?)?;
    exact_names(&receipt, "needed", &elf.needed()?)?;
    let lifecycle = receipt
        .get("lifecycle")
        .map_err(|_| Refusal::RuntimeLibraryInitializerSealMismatch)?;
    for (kind, section_type) in [("init", 14), ("fini", 15)] {
        let entries = lifecycle
            .get(kind)
            .and_then(Dyn::as_array)
            .map_err(|_| Refusal::RuntimeLibraryInitializerSealMismatch)?;
        let mut names = BTreeSet::new();
        let mut addresses = Vec::with_capacity(entries.len());
        let mut last_priority = None;
        for row in entries {
            let name =
                text(row, "name").map_err(|_| Refusal::RuntimeLibraryInitializerSealMismatch)?;
            let priority = match row.get("priority") {
                Ok(Dyn::Int(value)) => value
                    .to_i64()
                    .filter(|value| (0..=65535).contains(value))
                    .ok_or(Refusal::RuntimeLibraryInitializerSealMismatch)?,
                _ => return Err(Refusal::RuntimeLibraryInitializerSealMismatch),
            };
            if last_priority.is_some_and(|(previous, previous_name)| {
                previous > priority || (previous == priority && previous_name > name)
            }) || !names.insert(name)
            {
                return Err(Refusal::RuntimeLibraryInitializerSealMismatch);
            }
            last_priority = Some((priority, name));
            addresses.push(
                *actual
                    .get(name)
                    .ok_or(Refusal::RuntimeLibraryInitializerSealMismatch)?,
            );
        }
        if kind == "init" && addresses.is_empty() {
            return Err(Refusal::RuntimeLibraryInitializerSealMismatch);
        }
        elf.lifecycle(section_type, &addresses)?;
    }
    // No symbol use and no constructor execution precede the byte/seal checks.
    Ok(Prepared {
        bytes,
        library_identity,
        exports: exports
            .into_iter()
            .map(|name| Symbol {
                name: CString::new(name).expect("validated ASCII export has no NUL"),
                address: OnceLock::new(),
            })
            .collect(),
    })
}

fn exact_names(receipt: &Dyn, key: &str, actual: &[&str]) -> Result<(), Refusal> {
    let expected = receipt
        .get(key)
        .and_then(Dyn::as_array)
        .map_err(|_| Refusal::RuntimeLibraryExportSealMismatch)?;
    if expected.len() != actual.len()
        || expected
            .iter()
            .zip(actual)
            .any(|(value, name)| value.as_str().ok() != Some(*name))
    {
        return Err(Refusal::RuntimeLibraryExportSealMismatch);
    }
    Ok(())
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
struct Elf<'a> {
    bytes: &'a [u8],
    sections: Vec<Section>,
}

impl<'a> Elf<'a> {
    fn new(bytes: &'a [u8]) -> Result<Self, Refusal> {
        let mut elf = Self {
            bytes,
            sections: Vec::new(),
        };
        if elf.span(0, 6)? != [0x7f, b'E', b'L', b'F', 2, 1]
            || elf.u16(16)? != 3
            || elf.u16(18)? != 62
            || elf.u64(24)? != 0
        {
            return Err(Refusal::RuntimeLibraryExportSealMismatch);
        }
        let start = elf.offset(40)?;
        let width = elf.u16(58)? as usize;
        let count = elf.u16(60)? as usize;
        if width != 64 || count == 0 {
            return Err(Refusal::RuntimeLibraryExportSealMismatch);
        }
        elf.span(
            start,
            count
                .checked_mul(width)
                .ok_or(Refusal::RuntimeLibraryExportSealMismatch)?,
        )?;
        for index in 0..count {
            let at = start + index * width;
            let section = Section {
                kind: elf.u32(at + 4)?,
                address: elf.u64(at + 16)?,
                offset: elf.offset(at + 24)?,
                size: elf.offset(at + 32)?,
                link: elf.u32(at + 40)? as usize,
                entry_size: elf.offset(at + 56)?,
            };
            if (elf.u64(at + 8)? & 5) == 5 {
                return Err(Refusal::RuntimeLibraryExportSealMismatch);
            }
            if section.kind != 8 {
                elf.span(section.offset, section.size)?;
            }
            elf.sections.push(section);
        }
        let start = elf.offset(32)?;
        let count = elf.u16(56)? as usize;
        if elf.u16(54)? != 56 || count == 0 {
            return Err(Refusal::RuntimeLibraryExportSealMismatch);
        }
        elf.span(
            start,
            count
                .checked_mul(56)
                .ok_or(Refusal::RuntimeLibraryExportSealMismatch)?,
        )?;
        let mut relro = false;
        for index in 0..count {
            let at = start + index * 56;
            let kind = elf.u32(at)?;
            let flags = elf.u32(at + 4)?;
            if (kind == 1 && flags & 3 == 3) || (kind == 0x6474e551 && flags & 1 != 0) {
                return Err(Refusal::RuntimeLibraryExportSealMismatch);
            }
            relro |= kind == 0x6474e552;
        }
        if !relro {
            return Err(Refusal::RuntimeLibraryExportSealMismatch);
        }
        for section in &elf.sections {
            if section.kind == 6 {
                if section.size % 16 != 0 {
                    return Err(Refusal::RuntimeLibraryExportSealMismatch);
                }
                for offset in (0..section.size).step_by(16) {
                    let tag = elf.u64(section.offset + offset)?;
                    let value = elf.u64(section.offset + offset + 8)?;
                    if tag == 22 || (tag == 30 && value & 4 != 0) {
                        return Err(Refusal::RuntimeLibraryExportSealMismatch);
                    }
                }
            }
        }
        Ok(elf)
    }
    fn span(&self, offset: usize, count: usize) -> Result<&'a [u8], Refusal> {
        self.bytes
            .get(
                offset
                    ..offset
                        .checked_add(count)
                        .ok_or(Refusal::RuntimeLibraryExportSealMismatch)?,
            )
            .ok_or(Refusal::RuntimeLibraryExportSealMismatch)
    }
    fn u16(&self, at: usize) -> Result<u16, Refusal> {
        Ok(u16::from_le_bytes(
            self.span(at, 2)?.try_into().expect("two bytes"),
        ))
    }
    fn u32(&self, at: usize) -> Result<u32, Refusal> {
        Ok(u32::from_le_bytes(
            self.span(at, 4)?.try_into().expect("four bytes"),
        ))
    }
    fn u64(&self, at: usize) -> Result<u64, Refusal> {
        Ok(u64::from_le_bytes(
            self.span(at, 8)?.try_into().expect("eight bytes"),
        ))
    }
    fn offset(&self, at: usize) -> Result<usize, Refusal> {
        usize::try_from(self.u64(at)?).map_err(|_| Refusal::RuntimeLibraryExportSealMismatch)
    }
    fn symbols(&self, table: Section) -> Result<impl Iterator<Item = usize>, Refusal> {
        if table.entry_size != 24 || table.size % 24 != 0 || table.link >= self.sections.len() {
            return Err(Refusal::RuntimeLibraryExportSealMismatch);
        }
        self.span(table.offset, table.size)?;
        Ok((0..table.size / 24).map(move |index| table.offset + index * 24))
    }
    fn symbol_name(&self, table: Section, at: usize) -> Result<&'a str, Refusal> {
        self.string_at(self.sections[table.link], self.u32(at)? as usize)
    }
    fn string_at(&self, strings: Section, offset: usize) -> Result<&'a str, Refusal> {
        let bytes = self.span(strings.offset, strings.size)?;
        let tail = bytes
            .get(offset..)
            .ok_or(Refusal::RuntimeLibraryExportSealMismatch)?;
        let end = tail
            .iter()
            .position(|byte| *byte == 0)
            .ok_or(Refusal::RuntimeLibraryExportSealMismatch)?;
        std::str::from_utf8(&tail[..end]).map_err(|_| Refusal::RuntimeLibraryExportSealMismatch)
    }
    fn exports(&self) -> Result<BTreeMap<&'a str, u64>, Refusal> {
        let table = self.dynamic_symbols()?;
        let mut result = BTreeMap::new();
        for at in self.symbols(table)? {
            let info = self.span(at + 4, 1)?[0];
            if info >> 4 == 0 || self.u16(at + 6)? == 0 {
                continue;
            }
            let name = self.symbol_name(table, at)?;
            if info != 0x12 || name.is_empty() || result.insert(name, self.u64(at + 8)?).is_some() {
                return Err(Refusal::RuntimeLibraryExportSealMismatch);
            }
        }
        Ok(result)
    }
    fn dynamic_symbols(&self) -> Result<Section, Refusal> {
        let mut tables = self.sections.iter().filter(|section| section.kind == 11);
        let table = *tables
            .next()
            .ok_or(Refusal::RuntimeLibraryExportSealMismatch)?;
        if tables.next().is_some() {
            return Err(Refusal::RuntimeLibraryExportSealMismatch);
        }
        Ok(table)
    }
    fn imports(&self) -> Result<Vec<&'a str>, Refusal> {
        let table = self.dynamic_symbols()?;
        let mut result = Vec::new();
        for at in self.symbols(table)? {
            if self.span(at + 4, 1)?[0] >> 4 == 0 || self.u16(at + 6)? != 0 {
                continue;
            }
            let name = self.symbol_name(table, at)?;
            if !name.is_empty() {
                result.push(name);
            }
        }
        result.sort_unstable();
        Ok(result)
    }
    fn needed(&self) -> Result<Vec<&'a str>, Refusal> {
        let mut offsets = Vec::new();
        let mut strings_address = None;
        for section in self.sections.iter().filter(|section| section.kind == 6) {
            for offset in (0..section.size).step_by(16) {
                let tag = self.u64(section.offset + offset)?;
                let value = self.u64(section.offset + offset + 8)?;
                if tag == 1 {
                    offsets.push(
                        usize::try_from(value)
                            .map_err(|_| Refusal::RuntimeLibraryExportSealMismatch)?,
                    );
                }
                if tag == 5 {
                    strings_address = Some(value);
                }
            }
        }
        if offsets.is_empty() {
            return Ok(Vec::new());
        }
        let strings = *self
            .sections
            .iter()
            .find(|section| section.kind == 3 && Some(section.address) == strings_address)
            .ok_or(Refusal::RuntimeLibraryExportSealMismatch)?;
        offsets
            .into_iter()
            .map(|offset| self.string_at(strings, offset))
            .collect()
    }
    fn lifecycle(&self, kind: u32, expected: &[u64]) -> Result<(), Refusal> {
        let refusal = Refusal::RuntimeLibraryInitializerSealMismatch;
        let arrays: Vec<_> = self
            .sections
            .iter()
            .filter(|section| section.kind == kind)
            .collect();
        if arrays.is_empty() && expected.is_empty() {
            return Ok(());
        }
        if arrays.len() != 1 || arrays[0].size != expected.len() * 8 {
            return Err(refusal);
        }
        let array = *arrays[0];
        for (index, expected) in expected.iter().enumerate() {
            let address = array.address.checked_add(index as u64 * 8).ok_or(refusal)?;
            let mut target = self.u64(array.offset + index * 8)?;
            let mut relocated = false;
            for relocation in self.sections.iter().filter(|section| section.kind == 4) {
                if relocation.entry_size != 24
                    || relocation.size % 24 != 0
                    || relocation.link >= self.sections.len()
                {
                    return Err(refusal);
                }
                for offset in (0..relocation.size).step_by(24) {
                    let at = relocation.offset + offset;
                    if self.u64(at)? != address {
                        continue;
                    }
                    if relocated {
                        return Err(refusal);
                    }
                    relocated = true;
                    let info = self.u64(at + 8)?;
                    let addend = self.u64(at + 16)? as i64;
                    target = match info as u32 {
                        8 if addend >= 0 => addend as u64,
                        1 => {
                            let table = self.sections[relocation.link];
                            let symbols = self.symbols(table)?;
                            let at = symbols.skip((info >> 32) as usize).next().ok_or(refusal)?;
                            self.u64(at + 8)?
                                .checked_add_signed(addend)
                                .ok_or(refusal)?
                        }
                        _ => return Err(refusal),
                    };
                }
            }
            if target != *expected {
                return Err(refusal);
            }
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
        assert_eq!(
            Refusal::RuntimeLibraryIdentityMismatch.key(),
            "RuntimeLibraryIdentityMismatch"
        );
        assert!(Elf::new(b"not ELF").is_err());
    }
    #[test]
    fn mismatched_receipt_refuses_before_opening_a_path() {
        assert!(matches!(
            admit(Path::new("/path/does/not/exist"), b"{}", &[0; 32]),
            Err(Refusal::RuntimeLibraryReceiptHashMismatch)
        ));
    }
    #[test]
    fn stateless_refusal_does_not_pin_a_failed_identity() {
        let path = Path::new("/path/does/not/exist");
        let bad = b"{}";
        assert!(matches!(
            admit(path, bad, &SHA256::sha256(bad)),
            Err(Refusal::RuntimeLibraryReceiptHashMismatch)
        ));
        let old = br#"{"kind":"jet-parent-runtime-candidate","optimization":"O0","exports":["jet_rt_alloc"]}"#;
        assert!(matches!(
            admit(path, old, &SHA256::sha256(old)),
            Err(Refusal::RuntimeLibraryExportSealMismatch)
        ));
        let good = br#"{"kind":"jet-parent-runtime-candidate","optimization":"O0","exports":["jet_rt_alloc"],"exportABI":[{"symbol":"jet_rt_alloc","parameters":[{"size":8,"align":8,"class":"Integer"},{"size":8,"align":8,"class":"Integer"}],"result":{"size":8,"align":8,"class":"Integer"}}]}"#;
        assert!(matches!(
            admit(path, good, &SHA256::sha256(good)),
            Err(Refusal::RuntimeLibraryLoadFailed)
        ));
        assert!(
            RESIDENT.get().is_none(),
            "pre-open refusals must leave runtime identity unpinned"
        );
    }
    #[test]
    fn dependency_inventory_is_exact() {
        let receipt =
            JSON::parse_json(r#"{"imports":["pthread_create"],"needed":["libc.so.6"]}"#).unwrap();
        assert!(exact_names(&receipt, "imports", &["pthread_create"]).is_ok());
        assert!(exact_names(&receipt, "needed", &["libc.so.6"]).is_ok());
        assert!(exact_names(&receipt, "imports", &[]).is_err());
        assert!(exact_names(&receipt, "needed", &["libevil.so"]).is_err());
    }
}
