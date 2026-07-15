//! D-SHAPE-CLI1: the checked command-schema projection shared by codegen and
//! inspection. The entry parameter type remains source truth; consumers never
//! reconstruct shell names, requiredness, defaults, or help independently.

use crate::AST::{CtValue, Expr, Item, Marker, ProgramBundle, StrPart, StructDef, Type, VariantPayload};
use crate::Syntax;

const RECORD_MAGIC: &[u8; 8] = b"JETCMD\0\0";
pub const RECORD_VERSION: u16 = 1;
pub const ELF_SECTION: &str = ".jet_command";
pub const PE_SECTION: &str = ".jetcmd";
pub const MACH_SECTION: &str = "__jetcmd";
pub const WASM_SECTION: &str = "jet.command";
const MAX_RECORD_BYTES: usize = 1024 * 1024;
const MAX_INPUTS: usize = 4096;
const MAX_STRING_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliValueKind {
    Bool,
    Int,
    Float,
    String,
    Path,
}

impl CliValueKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CliValueKind::Bool => "Bool",
            CliValueKind::Int => "Int",
            CliValueKind::Float => "Float",
            CliValueKind::String => "String",
            CliValueKind::Path => "Path",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CliDefault {
    TypeDefault,
    Value(CtValue),
    /// Canonical display value recovered from an executable metadata record.
    Recorded(String),
}

impl CliDefault {
    pub fn display(&self) -> String {
        match self {
            CliDefault::TypeDefault => "type default".to_string(),
            CliDefault::Value(value) => value.jet_show(),
            CliDefault::Recorded(value) => value.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CliInputShape {
    Flag,
    Value {
        kind: CliValueKind,
        optional: bool,
        default: Option<CliDefault>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CliInputSchema {
    pub field: String,
    pub flag: String,
    pub help: String,
    pub metavar: Option<String>,
    pub shape: CliInputShape,
}

impl CliInputSchema {
    pub fn required(&self) -> bool {
        matches!(
            self.shape,
            CliInputShape::Value {
                optional: false,
                default: None,
                ..
            }
        )
    }

    pub fn value_kind(&self) -> CliValueKind {
        match self.shape {
            CliInputShape::Flag => CliValueKind::Bool,
            CliInputShape::Value { kind, .. } => kind,
        }
    }

    pub fn default_display(&self) -> Option<String> {
        match &self.shape {
            CliInputShape::Value { default, .. } => {
                default.as_ref().map(CliDefault::display)
            }
            CliInputShape::Flag => Some("false".to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CliCommandSchema {
    pub entry_type: String,
    pub inputs: Vec<CliInputSchema>,
    pub commands: Vec<CliSubcommandSchema>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CliSubcommandSchema {
    pub name: String,
    pub inputs: Vec<CliInputSchema>,
}

/// Checked command surface for an executable. Plain `fn run()` deliberately
/// produces an empty record so external completion can still register the
/// built-in `--help` surface without executing the program.
pub fn executable_schema(bundle: &ProgramBundle) -> CliCommandSchema {
    let items = &bundle.modules[bundle.entry].items;
    entry_schema(items).unwrap_or(CliCommandSchema {
        entry_type: String::new(),
        inputs: Vec::new(),
        commands: Vec::new(),
    })
}

/// Checked schema for a typed `fn run` in one entry module. Codegen, dossier,
/// executable metadata, and completion all consume this projection.
pub fn entry_schema(items: &[Item]) -> Option<CliCommandSchema> {
    let run_type = items.iter().find_map(|item| match item {
        Item::Func(function) if function.name == "run" && function.params.len() == 1 => {
            match &function.params[0].ty {
                Type::Named(name) => Some(name.as_str()),
                _ => None,
            }
        }
        _ => None,
    })?;
    let name = run_type;
    if let Some(structure) = items.iter().find_map(|item| match item {
        Item::Struct(structure) if structure.name == name => command_schema(structure),
        _ => None,
    }) {
        return Some(structure);
    }
    let enumeration = items.iter().find_map(|item| match item {
        Item::Enum(enumeration) if enumeration.name == name => Some(enumeration),
        _ => None,
    })?;
    let commands = enumeration.variants.iter().filter_map(|variant| {
        let VariantPayload::Single(Type::Named(payload), _) = &variant.payload else { return None };
        let structure = items.iter().find_map(|item| match item {
            Item::Struct(structure) if structure.name == *payload => Some(structure),
            _ => None,
        })?;
        let payload = command_schema(structure)?;
        Some(CliSubcommandSchema { name: variant.name.to_lowercase(), inputs: payload.inputs })
    }).collect();
    Some(CliCommandSchema { entry_type: name.to_string(), inputs: Vec::new(), commands })
}

/// Canonical, versioned JetCommandSchema record. The digest makes corruption
/// fail closed; embedding these bytes before linking binds them into the
/// executable and therefore its cache/signing identity.
pub fn encode_record(schema: &CliCommandSchema) -> Vec<u8> {
    let mut payload = Vec::new();
    put_string(&mut payload, &schema.entry_type);
    put_u32(&mut payload, schema.inputs.len() as u32);
    for input in &schema.inputs {
        encode_input(&mut payload, input);
    }
    put_u32(&mut payload, schema.commands.len() as u32);
    for command in &schema.commands {
        put_string(&mut payload, &command.name);
        put_u32(&mut payload, command.inputs.len() as u32);
        for input in &command.inputs { encode_input(&mut payload, input); }
    }
    let mut record = Vec::with_capacity(46 + payload.len());
    record.extend_from_slice(RECORD_MAGIC);
    record.extend_from_slice(&RECORD_VERSION.to_le_bytes());
    put_u32(&mut record, payload.len() as u32);
    record.extend_from_slice(&crate::SHA256::sha256(&payload));
    record.extend_from_slice(&payload);
    record
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataError {
    UnknownFormat,
    Missing,
    Duplicate,
    Malformed(&'static str),
    UnsupportedVersion(u16),
}

impl std::fmt::Display for MetadataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MetadataError::UnknownFormat => write!(f, "the file is not an ELF, PE, Mach-O, or Wasm executable"),
            MetadataError::Missing => write!(f, "the executable has no JetCommandSchema metadata"),
            MetadataError::Duplicate => write!(f, "the executable contains more than one JetCommandSchema record"),
            MetadataError::Malformed(why) => write!(f, "the JetCommandSchema record is malformed ({why})"),
            MetadataError::UnsupportedVersion(version) => write!(f, "JetCommandSchema version {version} is not supported"),
        }
    }
}

pub fn read_executable(bytes: &[u8]) -> Result<CliCommandSchema, MetadataError> {
    let sections = if bytes.starts_with(b"\x7fELF") {
        elf_sections(bytes)?
    } else if bytes.starts_with(b"MZ") {
        pe_sections(bytes)?
    } else if bytes.starts_with(b"\0asm") {
        wasm_sections(bytes)?
    } else if is_mach(bytes) {
        mach_sections(bytes)?
    } else {
        return Err(MetadataError::UnknownFormat);
    };
    if sections.is_empty() { return Err(MetadataError::Missing); }
    if sections.len() != 1 { return Err(MetadataError::Duplicate); }
    decode_record(sections[0])
}

pub fn decode_record(record: &[u8]) -> Result<CliCommandSchema, MetadataError> {
    if record.len() < 46 || &record[..8] != RECORD_MAGIC {
        return Err(MetadataError::Malformed("bad record header"));
    }
    let version = u16::from_le_bytes([record[8], record[9]]);
    if version != RECORD_VERSION { return Err(MetadataError::UnsupportedVersion(version)); }
    let len = u32::from_le_bytes(record[10..14].try_into().unwrap()) as usize;
    if len > MAX_RECORD_BYTES || 46usize.checked_add(len) != Some(record.len()) {
        return Err(MetadataError::Malformed("invalid record length"));
    }
    let payload = &record[46..];
    if crate::SHA256::sha256(payload) != record[14..46] {
        return Err(MetadataError::Malformed("digest mismatch"));
    }
    let mut cursor = Cursor::new(payload);
    let entry_type = cursor.string()?;
    let count = cursor.u32()? as usize;
    if count > MAX_INPUTS { return Err(MetadataError::Malformed("too many inputs")); }
    let mut inputs = Vec::with_capacity(count);
    for _ in 0..count {
        inputs.push(decode_input(&mut cursor)?);
    }
    let command_count = cursor.u32()? as usize;
    if command_count > MAX_INPUTS { return Err(MetadataError::Malformed("too many commands")); }
    let mut commands = Vec::with_capacity(command_count);
    for _ in 0..command_count {
        let name = cursor.string()?;
        let input_count = cursor.u32()? as usize;
        if input_count > MAX_INPUTS { return Err(MetadataError::Malformed("too many command inputs")); }
        let mut command_inputs = Vec::with_capacity(input_count);
        for _ in 0..input_count { command_inputs.push(decode_input(&mut cursor)?); }
        commands.push(CliSubcommandSchema { name, inputs: command_inputs });
    }
    if !cursor.done() { return Err(MetadataError::Malformed("trailing payload bytes")); }
    Ok(CliCommandSchema { entry_type, inputs, commands })
}

fn encode_input(payload: &mut Vec<u8>, input: &CliInputSchema) {
    put_string(payload, &input.field);
    put_string(payload, &input.flag);
    put_string(payload, &input.help);
    put_optional_string(payload, input.metavar.as_deref());
    match &input.shape {
        CliInputShape::Flag => payload.push(0),
        CliInputShape::Value { kind, optional, default } => {
            payload.push(1);
            payload.push(match kind { CliValueKind::Bool => 0, CliValueKind::Int => 1, CliValueKind::Float => 2, CliValueKind::String => 3, CliValueKind::Path => 4 });
            payload.push(u8::from(*optional));
            match default {
                None => payload.push(0),
                Some(CliDefault::TypeDefault) => payload.push(1),
                Some(value) => { payload.push(2); put_string(payload, &value.display()); }
            }
        }
    }
}

fn decode_input(cursor: &mut Cursor<'_>) -> Result<CliInputSchema, MetadataError> {
        let field = cursor.string()?;
        let flag = cursor.string()?;
        let help = cursor.string()?;
        let metavar = cursor.optional_string()?;
        let shape = match cursor.byte()? {
            0 => CliInputShape::Flag,
            1 => {
                let kind = match cursor.byte()? {
                    0 => CliValueKind::Bool, 1 => CliValueKind::Int,
                    2 => CliValueKind::Float, 3 => CliValueKind::String,
                    4 => CliValueKind::Path,
                    _ => return Err(MetadataError::Malformed("unknown input kind")),
                };
                let optional = match cursor.byte()? { 0 => false, 1 => true, _ => return Err(MetadataError::Malformed("invalid optional bit")) };
                let default = match cursor.byte()? {
                    0 => None,
                    1 => Some(CliDefault::TypeDefault),
                    2 => Some(CliDefault::Recorded(cursor.string()?)),
                    _ => return Err(MetadataError::Malformed("unknown default kind")),
                };
                CliInputShape::Value { kind, optional, default }
            }
            _ => return Err(MetadataError::Malformed("unknown input shape")),
        };
    Ok(CliInputSchema { field, flag, help, metavar, shape })
}

fn put_u32(out: &mut Vec<u8>, value: u32) { out.extend_from_slice(&value.to_le_bytes()); }
fn put_string(out: &mut Vec<u8>, value: &str) {
    put_u32(out, value.len() as u32);
    out.extend_from_slice(value.as_bytes());
}
fn put_optional_string(out: &mut Vec<u8>, value: Option<&str>) {
    out.push(u8::from(value.is_some()));
    if let Some(value) = value { put_string(out, value); }
}

struct Cursor<'a> { bytes: &'a [u8], at: usize }
impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self { Self { bytes, at: 0 } }
    fn byte(&mut self) -> Result<u8, MetadataError> {
        let value = *self.bytes.get(self.at).ok_or(MetadataError::Malformed("truncated payload"))?;
        self.at += 1;
        Ok(value)
    }
    fn u32(&mut self) -> Result<u32, MetadataError> {
        let end = self.at.checked_add(4).ok_or(MetadataError::Malformed("length overflow"))?;
        let bytes = self.bytes.get(self.at..end).ok_or(MetadataError::Malformed("truncated payload"))?;
        self.at = end;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<String, MetadataError> {
        let len = self.u32()? as usize;
        if len > MAX_STRING_BYTES { return Err(MetadataError::Malformed("string too long")); }
        let end = self.at.checked_add(len).ok_or(MetadataError::Malformed("length overflow"))?;
        let bytes = self.bytes.get(self.at..end).ok_or(MetadataError::Malformed("truncated string"))?;
        self.at = end;
        String::from_utf8(bytes.to_vec()).map_err(|_| MetadataError::Malformed("string is not UTF-8"))
    }
    fn optional_string(&mut self) -> Result<Option<String>, MetadataError> {
        match self.byte()? { 0 => Ok(None), 1 => self.string().map(Some), _ => Err(MetadataError::Malformed("invalid optional string bit")) }
    }
    fn done(&self) -> bool { self.at == self.bytes.len() }
}

fn bounds(bytes: &[u8], at: usize, len: usize) -> Result<&[u8], MetadataError> {
    let end = at.checked_add(len).ok_or(MetadataError::Malformed("section range overflow"))?;
    bytes.get(at..end).ok_or(MetadataError::Malformed("section outside file"))
}

fn read_num(bytes: &[u8], at: usize, width: usize, little: bool) -> Result<u64, MetadataError> {
    let value = bounds(bytes, at, width)?;
    Ok(match (width, little) {
        (2, true) => u16::from_le_bytes(value.try_into().unwrap()) as u64,
        (2, false) => u16::from_be_bytes(value.try_into().unwrap()) as u64,
        (4, true) => u32::from_le_bytes(value.try_into().unwrap()) as u64,
        (4, false) => u32::from_be_bytes(value.try_into().unwrap()) as u64,
        (8, true) => u64::from_le_bytes(value.try_into().unwrap()),
        (8, false) => u64::from_be_bytes(value.try_into().unwrap()),
        _ => return Err(MetadataError::Malformed("invalid integer width")),
    })
}

fn usize_num(value: u64) -> Result<usize, MetadataError> {
    usize::try_from(value).map_err(|_| MetadataError::Malformed("file offset overflow"))
}

fn c_name(bytes: &[u8]) -> Result<&str, MetadataError> {
    let end = bytes.iter().position(|byte| *byte == 0).unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[..end]).map_err(|_| MetadataError::Malformed("section name is not UTF-8"))
}

fn elf_sections(bytes: &[u8]) -> Result<Vec<&[u8]>, MetadataError> {
    let class = *bytes.get(4).ok_or(MetadataError::Malformed("truncated ELF header"))?;
    let little = match bytes.get(5) { Some(1) => true, Some(2) => false, _ => return Err(MetadataError::Malformed("invalid ELF byte order")) };
    let (header, shoff_at, shentsize_at, shnum_at, shstr_at, shoff_width, sh_offset_at, sh_size_at, value_width) = match class {
        1 => (52, 32, 46, 48, 50, 4, 16, 20, 4),
        2 => (64, 40, 58, 60, 62, 8, 24, 32, 8),
        _ => return Err(MetadataError::Malformed("unsupported ELF class")),
    };
    bounds(bytes, 0, header)?;
    let shoff = usize_num(read_num(bytes, shoff_at, shoff_width, little)?)?;
    let entsize = usize_num(read_num(bytes, shentsize_at, 2, little)?)?;
    let count = usize_num(read_num(bytes, shnum_at, 2, little)?)?;
    let names_index = usize_num(read_num(bytes, shstr_at, 2, little)?)?;
    if count == 0 || entsize < sh_size_at + value_width || names_index >= count {
        return Err(MetadataError::Malformed("invalid ELF section table"));
    }
    bounds(bytes, shoff, entsize.checked_mul(count).ok_or(MetadataError::Malformed("ELF section count overflow"))?)?;
    let section = |index: usize| -> Result<(usize, usize, usize), MetadataError> {
        let base = shoff.checked_add(index.checked_mul(entsize).ok_or(MetadataError::Malformed("ELF section offset overflow"))?).ok_or(MetadataError::Malformed("ELF section offset overflow"))?;
        let name = usize_num(read_num(bytes, base, 4, little)?)?;
        let offset = usize_num(read_num(bytes, base + sh_offset_at, value_width, little)?)?;
        let size = usize_num(read_num(bytes, base + sh_size_at, value_width, little)?)?;
        Ok((name, offset, size))
    };
    let (_, names_at, names_len) = section(names_index)?;
    let names = bounds(bytes, names_at, names_len)?;
    let mut found = Vec::new();
    for index in 0..count {
        let (name_at, offset, size) = section(index)?;
        let tail = names.get(name_at..).ok_or(MetadataError::Malformed("ELF section name outside string table"))?;
        if c_name(tail)? == ELF_SECTION { found.push(bounds(bytes, offset, size)?); }
    }
    Ok(found)
}

fn pe_sections(bytes: &[u8]) -> Result<Vec<&[u8]>, MetadataError> {
    let pe = usize_num(read_num(bytes, 0x3c, 4, true)?)?;
    if bounds(bytes, pe, 4)? != b"PE\0\0" { return Err(MetadataError::Malformed("bad PE signature")); }
    let count = usize_num(read_num(bytes, pe + 6, 2, true)?)?;
    let optional = usize_num(read_num(bytes, pe + 20, 2, true)?)?;
    let table = pe.checked_add(24).and_then(|v| v.checked_add(optional)).ok_or(MetadataError::Malformed("PE section table overflow"))?;
    bounds(bytes, table, count.checked_mul(40).ok_or(MetadataError::Malformed("PE section count overflow"))?)?;
    let mut found = Vec::new();
    for index in 0..count {
        let base = table + index * 40;
        if c_name(bounds(bytes, base, 8)?)? == PE_SECTION {
            let size = usize_num(read_num(bytes, base + 16, 4, true)?)?;
            let offset = usize_num(read_num(bytes, base + 20, 4, true)?)?;
            let raw = bounds(bytes, offset, size)?;
            // PE section raw sizes are file-aligned. The canonical record's
            // own length trims only zero alignment padding, then validates.
            if raw.len() < 14 { return Err(MetadataError::Malformed("truncated PE metadata section")); }
            let record_len = 46usize.checked_add(u32::from_le_bytes(raw[10..14].try_into().unwrap()) as usize).ok_or(MetadataError::Malformed("record length overflow"))?;
            let record = bounds(raw, 0, record_len)?;
            let trailing = &raw[record_len..];
            if trailing.starts_with(RECORD_MAGIC) { return Err(MetadataError::Duplicate); }
            if trailing.iter().any(|byte| *byte != 0) {
                return Err(MetadataError::Malformed("nonzero bytes after PE metadata record"));
            }
            found.push(record);
        }
    }
    Ok(found)
}

fn is_mach(bytes: &[u8]) -> bool {
    matches!(bytes.get(..4),
        Some([0xce, 0xfa, 0xed, 0xfe]) | Some([0xcf, 0xfa, 0xed, 0xfe]) |
        Some([0xca, 0xfe, 0xba, 0xbe]) | Some([0xca, 0xfe, 0xba, 0xbf]) |
        Some([0xbe, 0xba, 0xfe, 0xca]) | Some([0xbf, 0xba, 0xfe, 0xca]))
}

fn is_fat_mach(bytes: &[u8]) -> bool {
    matches!(bytes.get(..4),
        Some([0xca, 0xfe, 0xba, 0xbe]) | Some([0xca, 0xfe, 0xba, 0xbf]) |
        Some([0xbe, 0xba, 0xfe, 0xca]) | Some([0xbf, 0xba, 0xfe, 0xca]))
}

fn mach_sections(bytes: &[u8]) -> Result<Vec<&[u8]>, MetadataError> {
    if is_fat_mach(bytes) {
        return fat_mach_sections(bytes);
    }
    let is_64 = bytes.get(..4) == Some(&[0xcf, 0xfa, 0xed, 0xfe]);
    let header = if is_64 { 32 } else { 28 };
    bounds(bytes, 0, header)?;
    let count = usize_num(read_num(bytes, 16, 4, true)?)?;
    let mut command = header;
    let mut found = Vec::new();
    for _ in 0..count {
        let kind = read_num(bytes, command, 4, true)? as u32;
        let size = usize_num(read_num(bytes, command + 4, 4, true)?)?;
        if size < 8 { return Err(MetadataError::Malformed("invalid Mach-O load command size")); }
        bounds(bytes, command, size)?;
        let segment_kind = if is_64 { 0x19 } else { 0x1 };
        if kind == segment_kind {
            let (nsects_at, sections_at, section_size, file_offset_at, section_len_at, section_len_width): (usize, usize, usize, usize, usize, usize) = if is_64 {
                (64, 72, 80, 48, 40, 8)
            } else {
                (48, 56, 68, 40, 36, 4)
            };
            let nsects = usize_num(read_num(bytes, command + nsects_at, 4, true)?)?;
            let needed = sections_at.checked_add(nsects.checked_mul(section_size).ok_or(MetadataError::Malformed("Mach-O section count overflow"))?).ok_or(MetadataError::Malformed("Mach-O section count overflow"))?;
            if needed > size { return Err(MetadataError::Malformed("Mach-O sections outside load command")); }
            for index in 0..nsects {
                let section = command + sections_at + index * section_size;
                if c_name(bounds(bytes, section, 16)?)? == MACH_SECTION {
                    let offset = usize_num(read_num(bytes, section + file_offset_at, 4, true)?)?;
                    let len = usize_num(read_num(bytes, section + section_len_at, section_len_width, true)?)?;
                    found.push(bounds(bytes, offset, len)?);
                }
            }
        }
        command = command.checked_add(size).ok_or(MetadataError::Malformed("Mach-O load command overflow"))?;
    }
    Ok(found)
}

fn fat_mach_sections(bytes: &[u8]) -> Result<Vec<&[u8]>, MetadataError> {
    let magic = bounds(bytes, 0, 4)?;
    let little = matches!(magic, [0xbe, 0xba, 0xfe, 0xca] | [0xbf, 0xba, 0xfe, 0xca]);
    let is_64 = matches!(magic, [0xca, 0xfe, 0xba, 0xbf] | [0xbf, 0xba, 0xfe, 0xca]);
    let count = usize_num(read_num(bytes, 4, 4, little)?)?;
    if count == 0 || count > 64 { return Err(MetadataError::Malformed("invalid universal Mach-O architecture count")); }
    let entry_size = if is_64 { 32usize } else { 20usize };
    bounds(bytes, 8, count.checked_mul(entry_size).ok_or(MetadataError::Malformed("universal Mach-O table overflow"))?)?;
    let mut canonical: Option<&[u8]> = None;
    for index in 0..count {
        let arch = 8 + index * entry_size;
        let width = if is_64 { 8 } else { 4 };
        let offset = usize_num(read_num(bytes, arch + 8, width, little)?)?;
        let size = usize_num(read_num(bytes, arch + 8 + width, width, little)?)?;
        let slice = bounds(bytes, offset, size)?;
        if is_fat_mach(slice) {
            return Err(MetadataError::Malformed("nested universal Mach-O slice"));
        }
        let sections = mach_sections(slice)?;
        if sections.len() != 1 {
            return if sections.is_empty() { Err(MetadataError::Missing) } else { Err(MetadataError::Duplicate) };
        }
        match canonical {
            None => canonical = Some(sections[0]),
            Some(record) if record == sections[0] => {}
            Some(_) => return Err(MetadataError::Malformed("universal Mach-O slices disagree")),
        }
    }
    Ok(vec![canonical.unwrap()])
}

fn wasm_leb(bytes: &[u8], at: &mut usize) -> Result<usize, MetadataError> {
    let mut value = 0usize;
    for shift in (0..35).step_by(7) {
        let byte = *bytes.get(*at).ok_or(MetadataError::Malformed("truncated Wasm LEB"))?;
        *at += 1;
        value |= ((byte & 0x7f) as usize).checked_shl(shift).ok_or(MetadataError::Malformed("Wasm LEB overflow"))?;
        if byte & 0x80 == 0 { return Ok(value); }
    }
    Err(MetadataError::Malformed("Wasm LEB too long"))
}

fn wasm_sections(bytes: &[u8]) -> Result<Vec<&[u8]>, MetadataError> {
    if bounds(bytes, 0, 8)? != b"\0asm\x01\0\0\0" { return Err(MetadataError::Malformed("unsupported Wasm header")); }
    let mut at = 8;
    let mut found = Vec::new();
    while at < bytes.len() {
        let id = bytes[at];
        at += 1;
        let size = wasm_leb(bytes, &mut at)?;
        let payload = bounds(bytes, at, size)?;
        at += size;
        if id == 0 {
            let mut name_at = 0;
            let name_len = wasm_leb(payload, &mut name_at)?;
            let name = bounds(payload, name_at, name_len)?;
            name_at += name_len;
            if name == WASM_SECTION.as_bytes() { found.push(&payload[name_at..]); }
        }
    }
    Ok(found)
}

fn put_wasm_leb(out: &mut Vec<u8>, mut value: usize) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 { byte |= 0x80; }
        out.push(byte);
        if value == 0 { break; }
    }
}

/// Add the canonical Wasm custom section before the artifact is published.
pub fn embed_wasm_record(wasm: &mut Vec<u8>, record: &[u8]) -> Result<(), MetadataError> {
    wasm_sections(wasm)?;
    let mut payload = Vec::new();
    put_wasm_leb(&mut payload, WASM_SECTION.len());
    payload.extend_from_slice(WASM_SECTION.as_bytes());
    payload.extend_from_slice(record);
    wasm.push(0);
    put_wasm_leb(wasm, payload.len());
    wasm.extend_from_slice(&payload);
    Ok(())
}

impl CliCommandSchema {
    /// Candidates legal before any subcommand is selected.
    pub fn completion_words(&self) -> Vec<String> {
        let mut words = vec!["--help".to_string()];
        words.extend(self.inputs.iter().map(|input| format!("--{}", input.flag)));
        for command in &self.commands {
            words.push(command.name.clone());
        }
        words
    }
}

pub fn command_schema(structure: &StructDef) -> Option<CliCommandSchema> {
    if !structure
        .derives
        .iter()
        .any(|(name, _)| name == Syntax::CONTRACT_CLI)
    {
        return None;
    }

    let inputs = structure
        .fields
        .iter()
        .filter(|field| field.computed.is_none())
        .map(|field| {
            let flag = field.name.replace('_', "-");
            let help = marker(&field.serde_markers, Syntax::CONTRACT_DOC)
                .and_then(marker_string)
                .unwrap_or_else(|| format!("value for --{flag}"));
            let metavar = flag.replace('-', "_").to_uppercase();
            let shape = match &field.ty {
                Type::Bool => CliInputShape::Flag,
                Type::Option(inner) => CliInputShape::Value {
                    kind: scalar_kind(inner)
                        .expect("sema permits only scalar Option fields on a Cli struct"),
                    optional: true,
                    default: None,
                },
                ty => CliInputShape::Value {
                    kind: scalar_kind(ty)
                        .expect("sema permits only scalar fields on a Cli struct"),
                    optional: false,
                    default: field_default(&field.serde_markers),
                },
            };
            CliInputSchema {
                field: field.name.clone(),
                flag,
                help,
                metavar: (!matches!(shape, CliInputShape::Flag)).then_some(metavar),
                shape,
            }
        })
        .collect();

    Some(CliCommandSchema {
        entry_type: structure.name.clone(),
        inputs,
        commands: Vec::new(),
    })
}

fn scalar_kind(ty: &Type) -> Option<CliValueKind> {
    match ty {
        Type::Bool => Some(CliValueKind::Bool),
        Type::Int => Some(CliValueKind::Int),
        Type::Float => Some(CliValueKind::Float),
        Type::String => Some(CliValueKind::String),
        Type::Named(name) if name == "Path" => Some(CliValueKind::Path),
        _ => None,
    }
}

fn marker<'a>(markers: &'a [Marker], name: &str) -> Option<&'a Marker> {
    markers.iter().find(|marker| marker.name == name)
}

fn marker_string(marker: &Marker) -> Option<String> {
    match marker.args.first() {
        Some(Expr::Str(parts, _)) if parts.len() == 1 => match &parts[0] {
            StrPart::Lit(value) => Some(value.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn field_default(markers: &[Marker]) -> Option<CliDefault> {
    let marker = marker(markers, Syntax::ATTR_DEFAULT)?;
    Some(match (&marker.args[..], &marker.ct) {
        ([_, ..], Some(value)) => CliDefault::Value(value.clone()),
        _ => CliDefault::TypeDefault,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> CliCommandSchema {
        CliCommandSchema {
            entry_type: "Options".to_string(),
            inputs: vec![CliInputSchema {
                field: "output_file".to_string(),
                flag: "output-file".to_string(),
                help: "destination".to_string(),
                metavar: Some("OUTPUT_FILE".to_string()),
                shape: CliInputShape::Value {
                    kind: CliValueKind::Path,
                    optional: false,
                    default: None,
                },
            }],
            commands: Vec::new(),
        }
    }

    fn elf(record: &[u8]) -> Vec<u8> {
        let names = b"\0.shstrtab\0.jet_command\0";
        let names_at = 64usize;
        let record_at = names_at + names.len();
        let sections_at = record_at + record.len();
        let mut bytes = vec![0u8; sections_at + 3 * 64];
        bytes[..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[40..48].copy_from_slice(&(sections_at as u64).to_le_bytes());
        bytes[58..60].copy_from_slice(&64u16.to_le_bytes());
        bytes[60..62].copy_from_slice(&3u16.to_le_bytes());
        bytes[62..64].copy_from_slice(&1u16.to_le_bytes());
        bytes[names_at..record_at].copy_from_slice(names);
        bytes[record_at..sections_at].copy_from_slice(record);
        let strings = sections_at + 64;
        bytes[strings..strings + 4].copy_from_slice(&1u32.to_le_bytes());
        bytes[strings + 24..strings + 32].copy_from_slice(&(names_at as u64).to_le_bytes());
        bytes[strings + 32..strings + 40].copy_from_slice(&(names.len() as u64).to_le_bytes());
        let metadata = sections_at + 128;
        bytes[metadata..metadata + 4].copy_from_slice(&11u32.to_le_bytes());
        bytes[metadata + 24..metadata + 32].copy_from_slice(&(record_at as u64).to_le_bytes());
        bytes[metadata + 32..metadata + 40].copy_from_slice(&(record.len() as u64).to_le_bytes());
        bytes
    }

    fn pe(record: &[u8], duplicate: bool) -> Vec<u8> {
        let pe = 0x80usize;
        let count = if duplicate { 2usize } else { 1usize };
        let table = pe + 24;
        let raw_at = table + count * 40;
        let mut bytes = vec![0u8; raw_at + count * record.len()];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&(pe as u32).to_le_bytes());
        bytes[pe..pe + 4].copy_from_slice(b"PE\0\0");
        bytes[pe + 6..pe + 8].copy_from_slice(&(count as u16).to_le_bytes());
        for index in 0..count {
            let section = table + index * 40;
            bytes[section..section + 7].copy_from_slice(b".jetcmd");
            bytes[section + 16..section + 20].copy_from_slice(&(record.len() as u32).to_le_bytes());
            let offset = raw_at + index * record.len();
            bytes[section + 20..section + 24].copy_from_slice(&(offset as u32).to_le_bytes());
            bytes[offset..offset + record.len()].copy_from_slice(record);
        }
        bytes
    }

    fn pe_with_trailing(record: &[u8], trailing: &[u8]) -> Vec<u8> {
        let mut bytes = pe(record, false);
        let table = 0x80 + 24;
        bytes[table + 16..table + 20]
            .copy_from_slice(&((record.len() + trailing.len()) as u32).to_le_bytes());
        bytes.extend_from_slice(trailing);
        bytes
    }

    fn mach(record: &[u8]) -> Vec<u8> {
        let command_size = 72 + 80;
        let record_at = 32 + command_size;
        let mut bytes = vec![0u8; record_at + record.len()];
        bytes[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&(command_size as u32).to_le_bytes());
        bytes[32..36].copy_from_slice(&0x19u32.to_le_bytes());
        bytes[36..40].copy_from_slice(&(command_size as u32).to_le_bytes());
        bytes[96..100].copy_from_slice(&1u32.to_le_bytes());
        let section = 104usize;
        bytes[section..section + 8].copy_from_slice(b"__jetcmd");
        bytes[section + 40..section + 48].copy_from_slice(&(record.len() as u64).to_le_bytes());
        bytes[section + 48..section + 52].copy_from_slice(&(record_at as u32).to_le_bytes());
        bytes[record_at..].copy_from_slice(record);
        bytes
    }

    fn fat_mach(record: &[u8]) -> Vec<u8> {
        let first = mach(record);
        let second = mach(record);
        let table_end = 8 + 2 * 20;
        let second_at = table_end + first.len();
        let mut bytes = vec![0u8; second_at + second.len()];
        bytes[..4].copy_from_slice(&[0xca, 0xfe, 0xba, 0xbe]);
        bytes[4..8].copy_from_slice(&2u32.to_be_bytes());
        bytes[16..20].copy_from_slice(&(table_end as u32).to_be_bytes());
        bytes[20..24].copy_from_slice(&(first.len() as u32).to_be_bytes());
        bytes[36..40].copy_from_slice(&(second_at as u32).to_be_bytes());
        bytes[40..44].copy_from_slice(&(second.len() as u32).to_be_bytes());
        bytes[table_end..second_at].copy_from_slice(&first);
        bytes[second_at..].copy_from_slice(&second);
        bytes
    }

    fn nested_fat_mach(record: &[u8]) -> Vec<u8> {
        let inner = fat_mach(record);
        let table_end = 8 + 20;
        let mut bytes = vec![0u8; table_end + inner.len()];
        bytes[..4].copy_from_slice(&[0xca, 0xfe, 0xba, 0xbe]);
        bytes[4..8].copy_from_slice(&1u32.to_be_bytes());
        bytes[16..20].copy_from_slice(&(table_end as u32).to_be_bytes());
        bytes[20..24].copy_from_slice(&(inner.len() as u32).to_be_bytes());
        bytes[table_end..].copy_from_slice(&inner);
        bytes
    }

    #[test]
    fn canonical_record_round_trips() {
        let schema = schema();
        assert_eq!(decode_record(&encode_record(&schema)).unwrap(), schema);
    }

    #[test]
    fn reads_cross_format_section_fixtures() {
        let schema = schema();
        let record = encode_record(&schema);
        assert_eq!(read_executable(&elf(&record)).unwrap(), schema);
        assert_eq!(read_executable(&pe(&record, false)).unwrap(), schema);
        assert_eq!(read_executable(&mach(&record)).unwrap(), schema);
        assert_eq!(read_executable(&fat_mach(&record)).unwrap(), schema);
        let mut wasm = b"\0asm\x01\0\0\0".to_vec();
        embed_wasm_record(&mut wasm, &record).unwrap();
        assert_eq!(read_executable(&wasm).unwrap(), schema);
    }

    #[test]
    fn hostile_records_fail_closed() {
        let record = encode_record(&schema());
        assert_eq!(read_executable(b"not executable"), Err(MetadataError::UnknownFormat));
        assert_eq!(read_executable(b"\0asm\x01\0\0\0"), Err(MetadataError::Missing));
        assert_eq!(read_executable(&pe(&record, true)), Err(MetadataError::Duplicate));
        assert_eq!(read_executable(&pe_with_trailing(&record, &record)), Err(MetadataError::Duplicate));
        assert_eq!(read_executable(&pe_with_trailing(&record, &[0, 7])), Err(MetadataError::Malformed("nonzero bytes after PE metadata record")));
        assert_eq!(read_executable(&pe_with_trailing(&record, &[0; 16])).unwrap(), schema());
        assert_eq!(read_executable(&nested_fat_mach(&record)), Err(MetadataError::Malformed("nested universal Mach-O slice")));

        let mut unsupported = record.clone();
        unsupported[8..10].copy_from_slice(&2u16.to_le_bytes());
        assert_eq!(decode_record(&unsupported), Err(MetadataError::UnsupportedVersion(2)));
        let mut corrupt = record;
        *corrupt.last_mut().unwrap() ^= 1;
        assert_eq!(decode_record(&corrupt), Err(MetadataError::Malformed("digest mismatch")));
    }
}
