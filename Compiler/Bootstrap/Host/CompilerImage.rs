//! Private, current-format compiler-image envelope and strict payload primitives.
//!
//! The MIR payload itself is generated from the complete Source MIR schema in
//! `RuntimeMirCodec.rs`; this module binds those bytes to their checked source,
//! artifact, entry, and full execution identity.

use crate::compiler_bootstrap_host::AuthorizedSourceSnapshot;
use jet_foundation::MIR::{
    MirArtifactId, MirExecutionIdentity, MirFunctionId, MirProgram, MIR_SCHEMA_VERSION,
};

const MAGIC: &[u8; 8] = b"JETCIMG\0";
const FORMAT_VERSION: u16 = 1;
const CHECKSUM_BYTES: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompilerImageHeader {
    pub(crate) format_version: u16,
    pub(crate) mir_schema_version: u16,
    pub(crate) source_authority_digest: [u8; 32],
    pub(crate) artifact: MirArtifactId,
    pub(crate) entry_function: MirFunctionId,
    pub(crate) identity: MirExecutionIdentity,
}

struct EncodedCompilerImageHeader {
    format_version: u16,
    mir_schema_version: u16,
    source_authority_digest: [u8; 32],
    artifact: MirArtifactId,
    entry_function: MirFunctionId,
}

pub(crate) struct RestoredCompilerImage<SourceProgram> {
    pub(crate) header: CompilerImageHeader,
    pub(crate) source_program: SourceProgram,
    pub(crate) program: MirProgram,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompilerImageError(pub(crate) String);

impl std::fmt::Display for CompilerImageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CompilerImageError {}

/// Writer used by the schema-generated, typed Source-MIR payload codec.
#[doc(hidden)]
pub(crate) struct CompilerImagePayloadWriter {
    bytes: Vec<u8>,
}

impl CompilerImagePayloadWriter {
    pub(crate) fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    pub(crate) fn write_u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub(crate) fn write_u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn write_u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn write_u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn write_raw(&mut self, value: &[u8]) {
        self.bytes.extend_from_slice(value);
    }

    pub(crate) fn write_len(&mut self, value: usize) -> Result<(), String> {
        let value = u64::try_from(value)
            .map_err(|_| "compiler-image length exceeds u64".to_string())?;
        self.write_u64(value);
        Ok(())
    }

    pub(crate) fn write_bytes(&mut self, value: &[u8]) -> Result<(), String> {
        self.write_len(value.len())?;
        self.write_raw(value);
        Ok(())
    }

    pub(crate) fn write_string(&mut self, value: &str) -> Result<(), String> {
        self.write_bytes(value.as_bytes())
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

/// Reader used by the schema-generated, typed Source-MIR payload codec.
#[doc(hidden)]
pub(crate) struct CompilerImagePayloadReader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> CompilerImagePayloadReader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, cursor: 0 }
    }

    pub(crate) fn read_u8(&mut self) -> Result<u8, String> {
        let value = *self
            .bytes
            .get(self.cursor)
            .ok_or_else(|| "truncated compiler-image MIR payload".to_string())?;
        self.cursor += 1;
        Ok(value)
    }

    pub(crate) fn read_u16(&mut self) -> Result<u16, String> {
        let bytes = self.read_raw(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    pub(crate) fn read_u32(&mut self) -> Result<u32, String> {
        let bytes = self.read_raw(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub(crate) fn read_u64(&mut self) -> Result<u64, String> {
        let bytes = self.read_raw(8)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    pub(crate) fn read_raw(&mut self, length: usize) -> Result<&'a [u8], String> {
        let end = self
            .cursor
            .checked_add(length)
            .ok_or_else(|| "compiler-image cursor overflow".to_string())?;
        let bytes = self
            .bytes
            .get(self.cursor..end)
            .ok_or_else(|| "truncated compiler-image MIR payload".to_string())?;
        self.cursor = end;
        Ok(bytes)
    }

    pub(crate) fn read_len(&mut self) -> Result<usize, String> {
        let raw = self.read_u64()?;
        let length = usize::try_from(raw)
            .map_err(|_| "compiler-image length exceeds the host address space".to_string())?;
        if length > self.bytes.len().saturating_sub(self.cursor).saturating_add(1) {
            return Err("compiler-image collection length exceeds remaining payload".to_string());
        }
        Ok(length)
    }

    pub(crate) fn read_bytes(&mut self) -> Result<&'a [u8], String> {
        let length = self.read_len()?;
        self.read_raw(length)
    }

    pub(crate) fn read_string(&mut self) -> Result<String, String> {
        String::from_utf8(self.read_bytes()?.to_vec())
            .map_err(|_| "compiler-image string is not valid UTF-8".to_string())
    }

    pub(crate) fn finish(self) -> Result<(), String> {
        if self.cursor != self.bytes.len() {
            return Err("compiler-image MIR payload has trailing bytes".to_string());
        }
        Ok(())
    }
}

/// Serialize a complete, checked compiler MIR image using a generated typed
/// MIR payload codec. User-result MIR must not be passed here.
pub(crate) fn archive_compiler_image<SourceProgram>(
    source_program: &SourceProgram,
    program: &MirProgram,
    artifact: MirArtifactId,
    entry_function: MirFunctionId,
    source_authority: &AuthorizedSourceSnapshot,
    source_to_native: impl FnOnce(&SourceProgram) -> Result<MirProgram, String>,
    encode_source: impl FnOnce(&SourceProgram) -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, CompilerImageError> {
    let identity = checked_identity(program, artifact, entry_function)?;
    let source_native = source_to_native(source_program).map_err(CompilerImageError)?;
    let source_identity = checked_identity(&source_native, artifact, entry_function)?;
    if source_identity != identity {
        return Err(CompilerImageError(
            "Source MIR compiler image does not match the supplied native MIR identity".into(),
        ));
    }
    let payload = encode_source(source_program).map_err(CompilerImageError)?;
    let header = CompilerImageHeader {
        format_version: FORMAT_VERSION,
        mir_schema_version: MIR_SCHEMA_VERSION,
        source_authority_digest: compiler_image_source_authority_digest(source_authority),
        artifact,
        entry_function,
        identity,
    };
    encode_archive(&header, &payload)
}

/// Restore one private compiler image only for the exact source authority,
/// artifact, and entry selected by the caller.
pub(crate) fn restore_compiler_image<SourceProgram>(
    bytes: &[u8],
    expected_source_authority_digest: [u8; 32],
    expected_artifact: MirArtifactId,
    expected_entry_function: MirFunctionId,
    decode_source: impl FnOnce(&[u8]) -> Result<SourceProgram, String>,
    source_to_native: impl FnOnce(&SourceProgram) -> Result<MirProgram, String>,
) -> Result<RestoredCompilerImage<SourceProgram>, CompilerImageError> {
    let (raw_header, payload, identity_bytes) = decode_archive(bytes)?;
    if raw_header.mir_schema_version != MIR_SCHEMA_VERSION {
        return Err(CompilerImageError(format!(
            "compiler-image MIR schema {} does not match current schema {}",
            raw_header.mir_schema_version, MIR_SCHEMA_VERSION
        )));
    }
    if raw_header.source_authority_digest != expected_source_authority_digest {
        return Err(CompilerImageError(
            "compiler-image source authority does not match the authorized source snapshot".into(),
        ));
    }
    if raw_header.artifact != expected_artifact {
        return Err(CompilerImageError(
            "compiler-image artifact identity does not match the requested artifact".into(),
        ));
    }
    if raw_header.entry_function != expected_entry_function {
        return Err(CompilerImageError(
            "compiler-image entry function does not match the requested entry".into(),
        ));
    }
    let source_program = decode_source(payload).map_err(CompilerImageError)?;
    let program = source_to_native(&source_program).map_err(CompilerImageError)?;
    let identity = checked_identity(&program, expected_artifact, expected_entry_function)?;
    if encode_identity(&identity)? != identity_bytes {
        return Err(CompilerImageError(
            "compiler-image execution identity does not match its MIR payload".into(),
        ));
    }
    let header = CompilerImageHeader {
        format_version: raw_header.format_version,
        mir_schema_version: raw_header.mir_schema_version,
        source_authority_digest: raw_header.source_authority_digest,
        artifact: raw_header.artifact,
        entry_function: raw_header.entry_function,
        identity,
    };
    Ok(RestoredCompilerImage {
        header,
        source_program,
        program,
    })
}

fn checked_identity(
    program: &MirProgram,
    artifact: MirArtifactId,
    entry_function: MirFunctionId,
) -> Result<MirExecutionIdentity, CompilerImageError> {
    program
        .validate()
        .map_err(|error| CompilerImageError(format!("invalid compiler-image MIR: {error}")))?;
    let artifact_plan = program
        .artifacts
        .iter()
        .find(|row| row.id == artifact)
        .ok_or_else(|| CompilerImageError("compiler-image artifact is absent from MIR".into()))?;
    if artifact_plan
        .entry
        .as_ref()
        .and_then(|entry| entry.function)
        != Some(entry_function)
    {
        return Err(CompilerImageError(
            "compiler-image entry does not match the selected artifact entry".into(),
        ));
    }
    if !program
        .functions
        .iter()
        .any(|function| function.id == entry_function)
    {
        return Err(CompilerImageError(
            "compiler-image entry function is absent from MIR".into(),
        ));
    }
    program
        .execution_identity(Some(artifact))
        .map_err(|error| CompilerImageError(format!("invalid compiler-image identity: {error}")))
}

pub(crate) fn compiler_image_source_authority_digest(
    source: &AuthorizedSourceSnapshot,
) -> [u8; 32] {
    let mut writer = CompilerImagePayloadWriter::new();
    writer
        .write_string(&source.entry_root_identity)
        .expect("source authority string length fits u64");
    writer
        .write_string(&source.entry_path)
        .expect("source authority string length fits u64");
    writer
        .write_len(source.roots.len())
        .expect("source root count fits u64");
    for root in &source.roots {
        writer
            .write_string(&root.canonical_path)
            .expect("source authority string length fits u64");
        writer
            .write_string(&root.identity)
            .expect("source authority string length fits u64");
        writer.write_u8(u8::from(root.allow_hardlinks));
        write_authorized_files(&mut writer, &root.files);
        write_authorized_files(&mut writer, &root.foreign_cache_files);
    }
    jet_foundation::SHA256::sha256(&writer.finish())
}

fn write_authorized_files(
    writer: &mut CompilerImagePayloadWriter,
    files: &[crate::compiler_bootstrap_host::AuthorizedSourceFile],
) {
    writer.write_len(files.len()).expect("source file count fits u64");
    for file in files {
        writer
            .write_string(&file.path)
            .expect("source authority string length fits u64");
        writer
            .write_string(&file.relative_path)
            .expect("source authority string length fits u64");
        writer
            .write_string(&file.identity)
            .expect("source authority string length fits u64");
        writer
            .write_len(file.source.len())
            .expect("source file length fits u64");
        writer.write_raw(&jet_foundation::SHA256::sha256(file.source.as_bytes()));
    }
}

fn encode_archive(
    header: &CompilerImageHeader,
    payload: &[u8],
) -> Result<Vec<u8>, CompilerImageError> {
    let identity = encode_identity(&header.identity)?;
    let identity_len = u64::try_from(identity.len())
        .map_err(|_| CompilerImageError("compiler-image identity is too large".into()))?;
    let payload_len = u64::try_from(payload.len())
        .map_err(|_| CompilerImageError("compiler-image MIR payload is too large".into()))?;
    let capacity = MAGIC
        .len()
        .checked_add(2 + 2 + 8 + 8 + 32 + 8 + 8 + CHECKSUM_BYTES)
        .and_then(|length| length.checked_add(identity.len()))
        .and_then(|length| length.checked_add(payload.len()))
        .ok_or_else(|| CompilerImageError("compiler-image size overflows usize".into()))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| CompilerImageError("compiler-image is too large to allocate".into()))?;
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&header.format_version.to_le_bytes());
    bytes.extend_from_slice(&header.mir_schema_version.to_le_bytes());
    bytes.extend_from_slice(&header.artifact.0.to_le_bytes());
    bytes.extend_from_slice(&header.entry_function.0.to_le_bytes());
    bytes.extend_from_slice(&header.source_authority_digest);
    bytes.extend_from_slice(&identity_len.to_le_bytes());
    bytes.extend_from_slice(&identity);
    bytes.extend_from_slice(&payload_len.to_le_bytes());
    bytes.extend_from_slice(payload);
    let checksum = jet_foundation::SHA256::sha256(&bytes);
    bytes.extend_from_slice(&checksum);
    Ok(bytes)
}

fn decode_archive(
    bytes: &[u8],
) -> Result<(EncodedCompilerImageHeader, &[u8], Vec<u8>), CompilerImageError> {
    if bytes.len() < MAGIC.len() + 2 + 2 + 8 + 8 + 32 + 8 + 8 + CHECKSUM_BYTES {
        return Err(CompilerImageError("truncated compiler-image archive".into()));
    }
    let content_end = bytes
        .len()
        .checked_sub(CHECKSUM_BYTES)
        .ok_or_else(|| CompilerImageError("truncated compiler-image checksum".into()))?;
    let expected_checksum = &bytes[content_end..];
    if jet_foundation::SHA256::sha256(&bytes[..content_end]).as_slice() != expected_checksum {
        return Err(CompilerImageError(
            "compiler-image archive checksum mismatch".into(),
        ));
    }
    let mut reader = CompilerImagePayloadReader::new(&bytes[..content_end]);
    if reader.read_raw(MAGIC.len()).map_err(CompilerImageError)? != MAGIC {
        return Err(CompilerImageError("invalid compiler-image magic".into()));
    }
    let format_version = reader.read_u16().map_err(CompilerImageError)?;
    if format_version != FORMAT_VERSION {
        return Err(CompilerImageError(format!(
            "unsupported private compiler-image format {format_version}"
        )));
    }
    let mir_schema_version = reader.read_u16().map_err(CompilerImageError)?;
    let artifact = MirArtifactId(reader.read_u64().map_err(CompilerImageError)?);
    let entry_function = MirFunctionId(reader.read_u64().map_err(CompilerImageError)?);
    if artifact.0 == 0 || entry_function.0 == 0 {
        return Err(CompilerImageError(
            "compiler-image contains a zero artifact or entry identity".into(),
        ));
    }
    let source_authority_digest: [u8; 32] = reader
        .read_raw(32)
        .map_err(CompilerImageError)?
        .try_into()
        .map_err(|_| CompilerImageError("invalid compiler-image authority digest".into()))?;
    let identity_len = reader.read_len().map_err(CompilerImageError)?;
    let identity = reader
        .read_raw(identity_len)
        .map_err(CompilerImageError)?
        .to_vec();
    let payload_len = reader.read_len().map_err(CompilerImageError)?;
    let payload = reader
        .read_raw(payload_len)
        .map_err(CompilerImageError)?;
    reader.finish().map_err(CompilerImageError)?;
    Ok((
        EncodedCompilerImageHeader {
            format_version,
            mir_schema_version,
            source_authority_digest,
            artifact,
            entry_function,
        },
        payload,
        identity,
    ))
}

fn encode_identity(identity: &MirExecutionIdentity) -> Result<Vec<u8>, CompilerImageError> {
    let mut writer = CompilerImagePayloadWriter::new();
    writer.write_u16(identity.schema_version);
    let artifact = &identity.artifact;
    writer.write_u16(artifact.schema_version);
    writer.write_u16(artifact.mir_schema_version);
    writer.write_raw(&artifact.program_digest);
    writer
        .write_string(&artifact.package_identity)
        .map_err(CompilerImageError)?;
    writer.write_u64(artifact.artifact.0);
    writer
        .write_string(&artifact.name)
        .map_err(CompilerImageError)?;
    writer
        .write_string(artifact.kind.as_str())
        .map_err(CompilerImageError)?;
    writer
        .write_string(artifact.target.as_str())
        .map_err(CompilerImageError)?;
    writer
        .write_string(artifact.mode.as_str())
        .map_err(CompilerImageError)?;
    writer
        .write_string(&artifact.provider_identity)
        .map_err(CompilerImageError)?;
    writer
        .write_string(&artifact.closure_identity)
        .map_err(CompilerImageError)?;
    writer
        .write_string(&artifact.artifact_identity)
        .map_err(CompilerImageError)?;
    let program = &artifact.program_identity;
    writer
        .write_string(&program.semantic_hash)
        .map_err(CompilerImageError)?;
    writer
        .write_string(&program.optimized_hash)
        .map_err(CompilerImageError)?;
    write_u64_values(&mut writer, &program.function_ids)?;
    write_u64_values(&mut writer, &program.core_ids)?;
    writer
        .write_len(program.target_facts.len())
        .map_err(CompilerImageError)?;
    for (key, value) in &program.target_facts {
        writer.write_string(key).map_err(CompilerImageError)?;
        writer.write_string(value).map_err(CompilerImageError)?;
    }
    writer
        .write_len(program.source_map.len())
        .map_err(CompilerImageError)?;
    for source in &program.source_map {
        writer.write_u64(source.id);
        writer.write_string(&source.path).map_err(CompilerImageError)?;
        writer
            .write_string(&source.digest)
            .map_err(CompilerImageError)?;
    }
    Ok(writer.finish())
}

fn write_u64_values(
    writer: &mut CompilerImagePayloadWriter,
    values: &[u64],
) -> Result<(), CompilerImageError> {
    writer
        .write_len(values.len())
        .map_err(CompilerImageError)?;
    for value in values {
        writer.write_u64(*value);
    }
    Ok(())
}

