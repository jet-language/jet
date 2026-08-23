//! Shared identity and provenance for generated foreign bridge artifacts.
//!
//! A bridge key is a digest of a length-delimited input record.  The record is
//! deliberately built by the caller: the binder or bridge builder owns the
//! descriptor inputs, while this module owns the stable encoding and artifact
//! provenance format.

use crate::AST::{BinderCapability, BinderDescriptor, ForeignAbiContract, ForeignScalar};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const IDENTITY_SCHEMA: &str = "jet-ffi-bridge-identity-v1";
pub const PROVENANCE_SCHEMA: &str = "jet-ffi-bridge-provenance-v1";
const SCALAR_BRIDGE_SCHEMA: &str = "jet-ffi-scalar-sidecar-v1";

/// One scalar function in the common checked sidecar ABI. Language adapters
/// parse their own declaration format into this shape, then share the C
/// boundary, Jet wrapper, and provenance rules below.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScalarBridgeFunction {
    pub name: String,
    pub params: Vec<ForeignScalar>,
    pub result: ForeignScalar,
}

/// A deterministic content-addressed identity record.
#[derive(Debug, Default)]
pub struct IdentityBuilder {
    bytes: Vec<u8>,
}

impl IdentityBuilder {
    /// Start an identity record with its schema as the first field.
    pub fn new(schema: &str) -> Self {
        let mut identity = Self::default();
        identity.field("schema", schema.as_bytes());
        identity
    }

    /// Add one ordered, length-delimited input field.
    pub fn field(&mut self, name: &str, value: &[u8]) {
        self.bytes
            .extend_from_slice(&(name.len() as u64).to_le_bytes());
        self.bytes.extend_from_slice(name.as_bytes());
        self.bytes
            .extend_from_slice(&(value.len() as u64).to_le_bytes());
        self.bytes.extend_from_slice(value);
    }

    /// Finish the record as a lowercase SHA-256 digest.
    pub fn finish(self) -> String {
        crate::SHA256::sha256_hex(&self.bytes)
    }
}

/// Parsed artifact provenance. Repeated fields are retained in insertion order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub schema: String,
    pub identity: String,
    pub fields: BTreeMap<String, Vec<String>>,
}

impl Provenance {
    /// Return the first value for a field.
    pub fn value(&self, name: &str) -> Option<&str> {
        self.fields
            .get(name)
            .and_then(|values| values.first())
            .map(String::as_str)
    }
}

/// Build the identity shared by a scalar binder's bridge build and its
/// provenance sidecar. Input bytes matter: a source path alone cannot notice a
/// changed foreign implementation behind the same filename.
pub fn scalar_bridge_identity(
    descriptor: BinderDescriptor,
    lib: &str,
    abi: &str,
    runtime: &str,
    source: &Path,
    worker: &Path,
    functions: &[ScalarBridgeFunction],
) -> Result<String, String> {
    validate_scalar_bridge(abi, descriptor, functions)?;
    let expected_abi = format!("{}{lib}", descriptor.language.bridge_prefix());
    if abi != expected_abi {
        return Err("foreign bridge ABI name does not match its descriptor language".into());
    }
    scalar_bridge_identity_with_descriptor(
        descriptor.language.root(),
        lib,
        abi,
        runtime,
        &descriptor.stamp(),
        source,
        worker,
        functions,
    )
}

fn scalar_bridge_identity_with_descriptor(
    language: &str,
    lib: &str,
    abi: &str,
    runtime: &str,
    descriptor: &str,
    source: &Path,
    worker: &Path,
    functions: &[ScalarBridgeFunction],
) -> Result<String, String> {
    let source_bytes = fs::read(source).map_err(|error| {
        format!(
            "could not read {} for bridge identity: {error}",
            source.display()
        )
    })?;
    let worker_bytes = fs::read(worker).map_err(|error| {
        format!(
            "could not read {} for bridge identity: {error}",
            worker.display()
        )
    })?;
    let mut identity = IdentityBuilder::new(IDENTITY_SCHEMA);
    identity.field("bridge_schema", SCALAR_BRIDGE_SCHEMA.as_bytes());
    identity.field("language", language.as_bytes());
    identity.field("library", lib.as_bytes());
    identity.field("abi", abi.as_bytes());
    identity.field("runtime", runtime.as_bytes());
    identity.field("runtime_toolchain", tool_identity(runtime).as_bytes());
    identity.field("cc", tool_identity("cc").as_bytes());
    identity.field("ar", tool_identity("ar").as_bytes());
    identity.field("descriptor", descriptor.as_bytes());
    identity.field("source_path", source.as_os_str().as_encoded_bytes());
    identity.field("source_bytes", &source_bytes);
    identity.field("worker_path", worker.as_os_str().as_encoded_bytes());
    identity.field("worker_bytes", &worker_bytes);
    for function in functions {
        identity.field("function", function.name.as_bytes());
        for parameter in &function.params {
            identity.field("parameter", format!("{parameter:?}").as_bytes());
        }
        identity.field("result", format!("{:?}", function.result).as_bytes());
    }
    Ok(identity.finish())
}

/// Write the queryable provenance sidecar for one published bridge.
pub fn write_provenance(
    path: &Path,
    identity: &str,
    fields: &[(&str, &str)],
    artifacts: &[(String, String)],
) -> Result<(), String> {
    let mut text = format!("schema={PROVENANCE_SCHEMA}\nidentity={identity}\n");
    for (name, value) in fields {
        append_line(&mut text, name, value)?;
    }
    for (relative, digest) in artifacts {
        append_line(&mut text, &format!("artifact.{relative}"), digest)?;
    }
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&temporary, text.as_bytes())
        .map_err(|error| format!("could not stage {}: {error}", path.display()))?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("could not publish {}: {error}", path.display()));
    }
    Ok(())
}

/// Read and validate a bridge provenance sidecar.
pub fn read_provenance(path: &Path) -> Result<Provenance, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let mut schema = None;
    let mut identity = None;
    let mut fields = BTreeMap::<String, Vec<String>>::new();
    for line in text.lines() {
        let (name, value) = line
            .split_once('=')
            .ok_or_else(|| format!("malformed bridge provenance line in {}", path.display()))?;
        if name.is_empty() || value.is_empty() {
            return Err(format!(
                "empty bridge provenance field in {}",
                path.display()
            ));
        }
        match name {
            "schema" => {
                if schema.replace(value.to_string()).is_some() {
                    return Err(format!(
                        "duplicate bridge provenance schema in {}",
                        path.display()
                    ));
                }
            }
            "identity" => {
                if identity.replace(value.to_string()).is_some() {
                    return Err(format!(
                        "duplicate bridge provenance identity in {}",
                        path.display()
                    ));
                }
            }
            _ => fields
                .entry(name.to_string())
                .or_default()
                .push(value.to_string()),
        }
    }
    let schema =
        schema.ok_or_else(|| format!("bridge provenance has no schema: {}", path.display()))?;
    let identity =
        identity.ok_or_else(|| format!("bridge provenance has no identity: {}", path.display()))?;
    if schema != PROVENANCE_SCHEMA {
        return Err(format!("unsupported bridge provenance schema `{schema}`"));
    }
    Ok(Provenance {
        schema,
        identity,
        fields,
    })
}

fn append_line(text: &mut String, name: &str, value: &str) -> Result<(), String> {
    if name.is_empty()
        || value.is_empty()
        || name
            .bytes()
            .any(|byte| matches!(byte, b'\n' | b'\r' | b'='))
        || value.bytes().any(|byte| matches!(byte, b'\n' | b'\r'))
    {
        return Err("bridge provenance fields cannot contain line breaks or `=`".into());
    }
    text.push_str(name);
    text.push('=');
    text.push_str(value);
    text.push('\n');
    Ok(())
}

/// Compile the one C ABI used by supervised scalar adapters. The worker owns
/// transport and language exceptions; this bridge only marshals scalars and
/// turns every non-OK response into a checked error code.
pub fn compile_scalar_sidecar(
    cache: &Path,
    abi: &str,
    runtime: &str,
    worker: &Path,
    source: &Path,
    descriptor: BinderDescriptor,
    functions: &[ScalarBridgeFunction],
) -> Result<PathBuf, String> {
    validate_scalar_bridge(abi, descriptor, functions)?;
    let lib = scalar_bridge_library(abi, descriptor)?;
    let identity =
        scalar_bridge_identity(descriptor, lib, abi, runtime, source, worker, functions)?;
    compile_scalar_sidecar_with_identity(
        cache, &identity, abi, runtime, worker, source, descriptor, functions,
    )
}

/// Compile or reuse one content-addressed scalar sidecar. The returned path is
/// the stable projection consumed by the existing generated binding cache; the
/// actual build artifact lives below `.bridges/<identity>`.
pub fn compile_scalar_sidecar_with_identity(
    cache: &Path,
    identity: &str,
    abi: &str,
    runtime: &str,
    worker: &Path,
    source: &Path,
    descriptor: BinderDescriptor,
    functions: &[ScalarBridgeFunction],
) -> Result<PathBuf, String> {
    validate_scalar_bridge(abi, descriptor, functions)?;
    let lib = scalar_bridge_library(abi, descriptor)?;
    let expected_identity =
        scalar_bridge_identity(descriptor, lib, abi, runtime, source, worker, functions)?;
    if expected_identity != identity {
        return Err("foreign bridge identity does not match its descriptor inputs".into());
    }
    fs::create_dir_all(cache)
        .map_err(|error| format!("could not create foreign binding cache: {error}"))?;
    let store = cache.join(".bridges").join(identity);
    let cached_archive = store.join(format!("lib{abi}.a"));
    let archive = cache.join(format!("lib{abi}.a"));
    if scalar_archive_valid(&cached_archive) {
        fs::copy(&cached_archive, &archive).map_err(|error| {
            format!(
                "could not project cached foreign bridge {}: {error}",
                archive.display()
            )
        })?;
        return Ok(archive);
    }
    fs::create_dir_all(&store)
        .map_err(|error| format!("could not create scalar bridge cache: {error}"))?;
    let c_path = store.join(format!("{abi}.c"));
    let object = store.join(format!("{abi}.o"));
    let staged_archive = store.join(format!(".{abi}.a.tmp.{}", std::process::id()));
    fs::write(
        &c_path,
        render_scalar_c(abi, runtime, worker, source, descriptor, functions)?,
    )
    .map_err(|error| format!("could not write foreign bridge: {error}"))?;
    let compile = Command::new("cc")
        .args(["-std=c11", "-D_POSIX_C_SOURCE=200809L", "-fPIC", "-c"])
        .arg(&c_path)
        .arg("-o")
        .arg(&object)
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                "the provisioned cc tool was not found".to_string()
            } else {
                format!("could not start cc: {error}")
            }
        })?;
    if !compile.status.success() {
        let detail = String::from_utf8_lossy(&compile.stderr).trim().to_string();
        let _ = fs::remove_file(&c_path);
        let _ = fs::remove_file(&object);
        return Err(format!("cc rejected the foreign bridge: {detail}"));
    }
    let archive_result = Command::new("ar")
        .arg("rcs")
        .arg(&staged_archive)
        .arg(&object)
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                "the provisioned ar tool was not found".to_string()
            } else {
                format!("could not start ar: {error}")
            }
        })?;
    let _ = fs::remove_file(&c_path);
    let _ = fs::remove_file(&object);
    if !archive_result.status.success() {
        let detail = String::from_utf8_lossy(&archive_result.stderr)
            .trim()
            .to_string();
        return Err(format!("ar rejected the foreign bridge: {detail}"));
    }
    if let Err(error) = fs::rename(&staged_archive, &cached_archive) {
        let _ = fs::remove_file(&staged_archive);
        return Err(format!("could not publish cached foreign bridge: {error}"));
    }
    fs::write(
        store.join(format!("lib{abi}.sha256")),
        crate::SHA256::sha256_hex(
            &fs::read(&cached_archive)
                .map_err(|error| format!("could not read cached foreign bridge: {error}"))?,
        ),
    )
    .map_err(|error| format!("could not publish scalar bridge cache identity: {error}"))?;
    fs::copy(&cached_archive, &archive).map_err(|error| {
        format!(
            "could not project foreign bridge {}: {error}",
            archive.display()
        )
    })?;
    Ok(archive)
}

/// Publish a queryable descriptor/provenance record for a scalar adapter.
pub fn write_scalar_provenance(
    cache: &Path,
    lib: &str,
    descriptor: BinderDescriptor,
    runtime: &str,
    source: &Path,
    worker: &Path,
    archive: &Path,
) -> Result<PathBuf, String> {
    write_scalar_provenance_with_functions(
        cache,
        lib,
        descriptor,
        runtime,
        source,
        worker,
        archive,
        &[],
    )
}

/// Publish scalar provenance using the exact function list used to build the
/// bridge. Binders call this variant so the sidecar identity cannot drift from
/// the cache key.
pub fn write_scalar_provenance_with_functions(
    cache: &Path,
    lib: &str,
    descriptor_row: BinderDescriptor,
    runtime: &str,
    source: &Path,
    worker: &Path,
    archive: &Path,
    functions: &[ScalarBridgeFunction],
) -> Result<PathBuf, String> {
    let contract = descriptor_row.contract;
    let descriptor = descriptor_row.stamp();
    let calling = format!("{:?}", contract.calling_convention);
    let layout = format!("{:?}", contract.layout);
    let ownership = format!("{:?}", contract.ownership);
    let errors = format!("{:?}", contract.errors);
    let callbacks = format!("{:?}", contract.callbacks);
    let async_completion = format!("{:?}", contract.async_completion);
    let task_boundary = format!("{:?}", contract.task_boundary);
    let safety = format!("{:?}", contract.safety);
    let provider = format!("{:?}", descriptor_row.provider);
    let language = descriptor_row.language;
    let abi = format!("jet_{}_{}", language.root(), lib);
    let identity = scalar_bridge_identity(
        descriptor_row,
        lib,
        &abi,
        runtime,
        source,
        worker,
        functions,
    )?;
    let runtime_toolchain = tool_identity(runtime);
    let cc = tool_identity("cc");
    let ar = tool_identity("ar");
    let source_text = source.to_string_lossy().into_owned();
    let worker_text = worker.to_string_lossy().into_owned();
    let worker_digest = sha_file(worker)?;
    let archive_digest = sha_file(archive)?;
    let provenance = cache.join(format!("{lib}.provenance"));
    let fields = [
        ("language", language.root().to_string()),
        ("abi", abi),
        ("runtime", runtime.to_string()),
        ("transport", "supervised-scalar-sidecar".to_string()),
        ("descriptor", descriptor),
        ("calling", calling),
        ("layout", layout),
        ("ownership", ownership),
        ("errors", errors),
        ("callbacks", callbacks),
        ("async", async_completion),
        ("tasks", task_boundary),
        ("safety", safety),
        ("provider", provider),
        ("runtime-toolchain", runtime_toolchain),
        ("cc", cc),
        ("ar", ar),
        (
            "source-sha256",
            crate::SHA256::sha256_hex(
                &fs::read(source)
                    .map_err(|error| format!("could not read {}: {error}", source.display()))?,
            ),
        ),
        (
            "worker-sha256",
            crate::SHA256::sha256_hex(
                &fs::read(worker)
                    .map_err(|error| format!("could not read {}: {error}", worker.display()))?,
            ),
        ),
        ("source", source_text),
        ("worker", worker_text),
    ];
    let field_refs: Vec<(&str, &str)> = fields
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    let artifacts = [
        (
            worker
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            worker_digest,
        ),
        (
            archive
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            archive_digest,
        ),
    ];
    let artifact_refs: Vec<(String, String)> = artifacts.into_iter().collect();
    write_provenance(&provenance, &identity, &field_refs, &artifact_refs)?;
    Ok(provenance)
}

/// Render the shared safe wrapper. Raw C symbols stay private to the generated
/// module and every public call checks the adapter's contained error slot.
pub fn render_scalar_jet(
    abi: &str,
    descriptor: BinderDescriptor,
    functions: &[ScalarBridgeFunction],
) -> Result<String, String> {
    validate_scalar_bridge(abi, descriptor, functions)?;
    let effect = descriptor.effect_root;
    let contract = descriptor.stamp();
    let mut out = format!("// jet-ffi-descriptor={contract}\n#Extern module c.{abi} {{\n");
    for function in functions {
        let _ = write!(out, "    fn {}(", function.name);
        for (index, scalar) in function.params.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            let jet_type = scalar.jet_name().ok_or_else(|| {
                format!("foreign scalar `{scalar:?}` is outside the checked sidecar ABI")
            })?;
            let _ = write!(out, "arg{index}: {jet_type}");
        }
        let result = function.result.jet_name().ok_or_else(|| {
            format!(
                "foreign scalar `{:?}` is outside the checked sidecar ABI",
                function.result
            )
        })?;
        let _ = writeln!(out, ") {result} = \"{abi}_{}\"", function.name);
    }
    let _ = writeln!(out, "    fn take_error() Int = \"{abi}_take_error\"\n}}");
    let _ = writeln!(out, "use c.{abi} as abi\n");
    for function in functions {
        let _ = write!(out, "pub fn {}(", function.name);
        for (index, scalar) in function.params.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            let jet_type = scalar.jet_name().ok_or_else(|| {
                format!("foreign scalar `{scalar:?}` is outside the checked sidecar ABI")
            })?;
            let _ = write!(out, "arg{index}: {jet_type}");
        }
        let result = function.result.jet_name().ok_or_else(|| {
            format!(
                "foreign scalar `{:?}` is outside the checked sidecar ABI",
                function.result
            )
        })?;
        let _ = writeln!(out, ") {result} String! -[{effect}]> {{",);
        let _ = write!(out, "    value :: abi.{}(", function.name);
        for index in 0..function.params.len() {
            if index > 0 {
                out.push_str(", ");
            }
            let _ = write!(out, "arg{index}");
        }
        out.push_str(")\n    if abi.take_error() != 0 {\n        return Err(\"foreign call failed\")\n    }\n    return Ok(value)\n}\n\n");
    }
    Ok(out)
}

fn render_scalar_c(
    abi: &str,
    runtime: &str,
    worker: &Path,
    source: &Path,
    descriptor: BinderDescriptor,
    functions: &[ScalarBridgeFunction],
) -> Result<String, String> {
    let descriptor = descriptor.stamp();
    let worker = c_escape(&shell_quote(&worker.to_string_lossy()));
    let source = c_escape(&shell_quote(&source.to_string_lossy()));
    let runtime = c_escape(&shell_quote(runtime));
    let mut out = format!(
        "/* jet-ffi-descriptor={descriptor} */\n#include <ctype.h>\n#include <errno.h>\n#include <stdbool.h>\n#include <stdint.h>\n#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\nstatic _Thread_local int64_t jet_failed;\nstatic int jet_invoke(const char *command, char *output, size_t capacity) {{\n    jet_failed = 0;\n    FILE *pipe = popen(command, \"r\");\n    if (!pipe) {{ jet_failed = 1; return 0; }}\n    if (!fgets(output, (int)capacity, pipe)) {{ jet_failed = 2; pclose(pipe); return 0; }}\n    int status = pclose(pipe);\n    if (status != 0 || strncmp(output, \"OK \", 3) != 0) {{ jet_failed = 3; return 0; }}\n    return 1;\n}}\n"
    );
    let _ = writeln!(
        out,
        "int64_t {abi}_take_error(void) {{ int64_t value = jet_failed; jet_failed = 0; return value; }}"
    );
    for function in functions {
        let result_type =
            c_type(function.result).ok_or_else(|| unsupported_scalar(function.result))?;
        let _ = write!(out, "{result_type} {abi}_{}(", function.name);
        for (index, scalar) in function.params.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            let parameter_type = c_type(*scalar).ok_or_else(|| unsupported_scalar(*scalar))?;
            let _ = write!(out, "{parameter_type} arg{index}");
        }
        out.push_str(") {\n    char output[256];\n    char command[8192];\n    int written = snprintf(command, sizeof(command), \"");
        out.push_str(&runtime);
        out.push(' ');
        out.push_str(&worker);
        out.push(' ');
        out.push_str(&source);
        out.push(' ');
        out.push_str(&c_escape(&shell_quote(&function.name)));
        for scalar in &function.params {
            out.push_str(format_spec(*scalar).ok_or_else(|| unsupported_scalar(*scalar))?);
        }
        if function.params.is_empty() {
            out.push_str(" 2>/dev/null\");\n");
        } else {
            out.push_str(" 2>/dev/null\", ");
            for (index, scalar) in function.params.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                out.push_str(
                    &format_arg(*scalar, index).ok_or_else(|| unsupported_scalar(*scalar))?,
                );
            }
            out.push_str(");\n");
        }
        out.push_str("    if (written < 0 || (size_t)written >= sizeof(command) || !jet_invoke(command, output, sizeof(output))) return (");
        out.push_str(
            zero_value(function.result).ok_or_else(|| unsupported_scalar(function.result))?,
        );
        out.push_str(");\n    char *end = NULL;\n    errno = 0;\n    ");
        match function.result {
            ForeignScalar::Int => out.push_str(
                "long long value = strtoll(output + 3, &end, 10); if (errno || end == output + 3) { jet_failed = 4; return 0; }\n",
            ),
            ForeignScalar::Float => out.push_str(
                "double value = strtod(output + 3, &end); if (errno || end == output + 3) { jet_failed = 4; return 0; }\n",
            ),
            ForeignScalar::Bool => out.push_str(
                "bool value; if (strncmp(output + 3, \"true\", 4) == 0) { value = true; end = output + 7; } else if (strncmp(output + 3, \"false\", 5) == 0) { value = false; end = output + 8; } else { jet_failed = 4; return false; }\n",
            ),
            scalar => return Err(unsupported_scalar(scalar)),
        }
        out.push_str("    while (*end && isspace((unsigned char)*end)) end++;\n    if (*end) { jet_failed = 4; return ");
        out.push_str(
            zero_value(function.result).ok_or_else(|| unsupported_scalar(function.result))?,
        );
        out.push_str("; }\n    return value;\n}\n");
    }
    Ok(out)
}

fn c_type(scalar: ForeignScalar) -> Option<&'static str> {
    match scalar {
        ForeignScalar::Int => Some("int64_t"),
        ForeignScalar::Float => Some("double"),
        ForeignScalar::Bool => Some("bool"),
        ForeignScalar::Char | ForeignScalar::String | ForeignScalar::Unsupported => None,
    }
}

fn format_spec(scalar: ForeignScalar) -> Option<&'static str> {
    match scalar {
        ForeignScalar::Int => Some(" %lld"),
        ForeignScalar::Float => Some(" %.17g"),
        ForeignScalar::Bool => Some(" %s"),
        ForeignScalar::Char | ForeignScalar::String | ForeignScalar::Unsupported => None,
    }
}

fn format_arg(scalar: ForeignScalar, index: usize) -> Option<String> {
    match scalar {
        ForeignScalar::Int => Some(format!("(long long)arg{index}")),
        ForeignScalar::Float => Some(format!("arg{index}")),
        ForeignScalar::Bool => Some(format!("arg{index} ? \"true\" : \"false\"")),
        ForeignScalar::Char | ForeignScalar::String | ForeignScalar::Unsupported => None,
    }
}

fn zero_value(scalar: ForeignScalar) -> Option<&'static str> {
    match scalar {
        ForeignScalar::Int => Some("0"),
        ForeignScalar::Float => Some("0.0"),
        ForeignScalar::Bool => Some("false"),
        ForeignScalar::Char | ForeignScalar::String | ForeignScalar::Unsupported => None,
    }
}

fn unsupported_scalar(scalar: ForeignScalar) -> String {
    format!("foreign scalar `{scalar:?}` is outside the checked sidecar ABI")
}

fn validate_scalar_bridge(
    abi: &str,
    descriptor: BinderDescriptor,
    functions: &[ScalarBridgeFunction],
) -> Result<(), String> {
    scalar_bridge_library(abi, descriptor)?;
    if descriptor.contract != ForeignAbiContract::MESSAGE {
        return Err(format!(
            "foreign binder `{}` does not expose the checked adapter contract",
            descriptor.language.root()
        ));
    }
    for capability in [
        BinderCapability::TypedStub,
        BinderCapability::SafeWrapper,
        BinderCapability::OwnershipConversion,
        BinderCapability::LayoutValidation,
        BinderCapability::ErrorConversion,
        BinderCapability::CacheProvenance,
    ] {
        if !descriptor.capabilities.contains(&capability) {
            return Err(format!(
                "foreign binder `{}` is missing capability `{capability:?}`",
                descriptor.language.root()
            ));
        }
    }
    let mut names = BTreeMap::new();
    for function in functions {
        if !is_identifier(&function.name) {
            return Err(format!("invalid foreign function name `{}`", function.name));
        }
        if names.insert(&function.name, ()).is_some() {
            return Err(format!("duplicate foreign function `{}`", function.name));
        }
        for scalar in function
            .params
            .iter()
            .copied()
            .chain(std::iter::once(function.result))
        {
            if !matches!(
                scalar,
                ForeignScalar::Int | ForeignScalar::Float | ForeignScalar::Bool
            ) {
                return Err(format!(
                    "foreign scalar `{scalar:?}` is outside the checked sidecar ABI"
                ));
            }
        }
    }
    Ok(())
}

fn scalar_bridge_library<'a>(
    abi: &'a str,
    descriptor: BinderDescriptor,
) -> Result<&'a str, String> {
    let prefix = descriptor.language.bridge_prefix();
    let Some(lib) = abi.strip_prefix(prefix).filter(|lib| is_identifier(lib)) else {
        return Err(format!(
            "foreign bridge ABI name `{abi}` does not match `{prefix}<library>`"
        ));
    };
    Ok(lib)
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(first) if first == '_' || first.is_ascii_alphabetic())
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn scalar_archive_valid(archive: &Path) -> bool {
    let Some(store) = archive.parent() else {
        return false;
    };
    let Some(file_name) = archive.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Ok(expected) = fs::read_to_string(store.join(format!("{file_name}.sha256"))) else {
        return false;
    };
    let expected = expected.trim();
    expected.len() == 64
        && expected
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        && fs::read(archive)
            .ok()
            .is_some_and(|bytes| crate::SHA256::sha256_hex(&bytes) == expected)
}

fn shell_quote(value: &str) -> String {
    let mut out = String::from("'");
    for (index, part) in value.split('\'').enumerate() {
        if index > 0 {
            out.push_str("'\\''");
        }
        out.push_str(part);
    }
    out.push('\'');
    out
}

fn c_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(character),
        }
    }
    out
}

fn tool_identity(tool: &str) -> String {
    let Some(path) = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|directory| {
            let candidate = directory.join(tool);
            candidate.is_file().then_some(candidate)
        })
    }) else {
        return "missing".to_string();
    };
    let path = path.canonicalize().unwrap_or(path);
    let version = Command::new(&path)
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" | ")
        })
        .filter(|version| !version.is_empty())
        .unwrap_or_else(|| "unavailable".to_string());
    format!("{};version={version}", path.display())
}

fn sha_file(path: &Path) -> Result<String, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    Ok(crate::SHA256::sha256_hex(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AST::ForeignLanguage;

    #[test]
    fn identity_is_stable_and_input_sensitive() {
        let mut first = IdentityBuilder::new(IDENTITY_SCHEMA);
        first.field("descriptor", b"one");
        let mut same = IdentityBuilder::new(IDENTITY_SCHEMA);
        same.field("descriptor", b"one");
        let mut changed = IdentityBuilder::new(IDENTITY_SCHEMA);
        changed.field("descriptor", b"two");
        let first = first.finish();
        assert_eq!(first, same.finish());
        assert_ne!(first, changed.finish());
    }

    #[test]
    fn descriptor_drives_stub_and_binder_source() {
        let descriptor = *crate::AST::binder_descriptor(ForeignLanguage::Py).unwrap();
        let functions = [ScalarBridgeFunction {
            name: "probe".into(),
            params: vec![ForeignScalar::Int],
            result: ForeignScalar::Int,
        }];
        let stub = render_scalar_jet("jet_py_probe", descriptor, &functions).unwrap();
        let binder = render_scalar_c(
            "jet_py_probe",
            "python3",
            Path::new("worker.py"),
            Path::new("probe.py"),
            descriptor,
            &functions,
        )
        .unwrap();

        let mut changed = descriptor;
        changed.effect_root = "FFI.changed";
        let changed_stub = render_scalar_jet("jet_py_probe", changed, &functions).unwrap();
        let changed_binder = render_scalar_c(
            "jet_py_probe",
            "python3",
            Path::new("worker.py"),
            Path::new("probe.py"),
            changed,
            &functions,
        )
        .unwrap();

        assert!(stub.contains("-[FFI.Py]>"));
        assert!(changed_stub.contains("-[FFI.changed]>"));
        assert_ne!(stub, changed_stub);
        assert_ne!(binder, changed_binder);
        assert!(binder.contains(&descriptor.stamp()));
        assert!(changed_binder.contains(&changed.stamp()));
    }

    #[test]
    fn unsupported_scalar_fails_closed_before_rendering() {
        let descriptor = *crate::AST::binder_descriptor(ForeignLanguage::Py).unwrap();
        let functions = [ScalarBridgeFunction {
            name: "bad".into(),
            params: vec![ForeignScalar::Unsupported],
            result: ForeignScalar::Int,
        }];
        let error = render_scalar_jet("jet_py_bad", descriptor, &functions).unwrap_err();
        assert!(error.contains("outside the checked sidecar ABI"));
    }

    #[test]
    fn descriptor_language_owns_scalar_bridge_abi_prefix() {
        let descriptor = *crate::AST::binder_descriptor(ForeignLanguage::Py).unwrap();
        let functions = [ScalarBridgeFunction {
            name: "probe".into(),
            params: vec![ForeignScalar::Int],
            result: ForeignScalar::Int,
        }];
        let error = render_scalar_jet("jet_js_probe", descriptor, &functions).unwrap_err();
        assert!(error.contains("does not match `jet_py_<library>`"));
        let error = scalar_bridge_identity(
            descriptor,
            "other",
            "jet_py_probe",
            "python3",
            Path::new("missing.py"),
            Path::new("missing_worker.py"),
            &functions,
        )
        .unwrap_err();
        assert!(error.contains("does not match its descriptor language"));
    }

    #[test]
    fn scalar_bridge_cache_reuses_identical_inputs_and_misses_source_changes() {
        if !Command::new("cc")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
            || !Command::new("ar")
                .arg("--version")
                .output()
                .is_ok_and(|output| output.status.success())
        {
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "jet_scalar_bridge_cache_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("ops.py");
        let worker = root.join("ops_worker.py");
        fs::write(&source, "def probe(value): return value\n").unwrap();
        fs::write(&worker, "print('worker')\n").unwrap();
        let functions = [ScalarBridgeFunction {
            name: "probe".into(),
            params: vec![ForeignScalar::Int],
            result: ForeignScalar::Int,
        }];
        let descriptor = *crate::AST::binder_descriptor(ForeignLanguage::Py).unwrap();
        let identity = scalar_bridge_identity(
            descriptor,
            "ops",
            "jet_py_ops",
            "python3",
            &source,
            &worker,
            &functions,
        )
        .unwrap();
        let mut changed_descriptor = descriptor;
        changed_descriptor.effect_root = "FFI.changed";
        let descriptor_identity = scalar_bridge_identity(
            changed_descriptor,
            "ops",
            "jet_py_ops",
            "python3",
            &source,
            &worker,
            &functions,
        )
        .unwrap();
        assert_ne!(identity, descriptor_identity);
        let mut changed_capabilities = descriptor;
        changed_capabilities.capabilities = &[];
        let capability_error = scalar_bridge_identity(
            changed_capabilities,
            "ops",
            "jet_py_ops",
            "python3",
            &source,
            &worker,
            &functions,
        )
        .unwrap_err();
        assert!(capability_error.contains("missing capability"));
        let runtime_identity = scalar_bridge_identity(
            descriptor,
            "ops",
            "jet_py_ops",
            "python3-alt",
            &source,
            &worker,
            &functions,
        )
        .unwrap();
        assert_ne!(identity, runtime_identity);
        let worker_bytes = fs::read(&worker).unwrap();
        fs::write(&worker, b"print('changed worker')\n").unwrap();
        let worker_identity = scalar_bridge_identity(
            descriptor,
            "ops",
            "jet_py_ops",
            "python3",
            &source,
            &worker,
            &functions,
        )
        .unwrap();
        assert_ne!(identity, worker_identity);
        fs::write(&worker, worker_bytes).unwrap();
        let first = compile_scalar_sidecar_with_identity(
            &root,
            &identity,
            "jet_py_ops",
            "python3",
            &worker,
            &source,
            descriptor,
            &functions,
        )
        .unwrap();
        let store = root.join(".bridges").join(&identity);
        assert!(store.join("libjet_py_ops.a").is_file());
        fs::remove_file(&first).unwrap();
        let reused = compile_scalar_sidecar_with_identity(
            &root,
            &identity,
            "jet_py_ops",
            "python3",
            &worker,
            &source,
            descriptor,
            &functions,
        )
        .unwrap();
        assert_eq!(first, reused);
        assert!(reused.is_file());

        fs::write(&source, "def probe(value): return value + 1\n").unwrap();
        let stale = compile_scalar_sidecar_with_identity(
            &root,
            &identity,
            "jet_py_ops",
            "python3",
            &worker,
            &source,
            descriptor,
            &functions,
        )
        .unwrap_err();
        assert!(stale.contains("does not match its descriptor inputs"));
        let changed = scalar_bridge_identity(
            descriptor,
            "ops",
            "jet_py_ops",
            "python3",
            &source,
            &worker,
            &functions,
        )
        .unwrap();
        assert_ne!(identity, changed);
        compile_scalar_sidecar_with_identity(
            &root,
            &changed,
            "jet_py_ops",
            "python3",
            &worker,
            &source,
            descriptor,
            &functions,
        )
        .unwrap();
        assert!(root
            .join(".bridges")
            .join(changed)
            .join("libjet_py_ops.a")
            .is_file());
        let _ = fs::remove_dir_all(root);
    }
}
