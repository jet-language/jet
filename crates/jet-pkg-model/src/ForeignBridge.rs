//! Shared identity and provenance for generated foreign bridge artifacts.
//!
//! A bridge key is a digest of a length-delimited input record.  The record is
//! deliberately built by the caller: the binder or bridge builder owns the
//! descriptor inputs, while this module owns the stable encoding and artifact
//! provenance format.

use crate::AST::{binder_descriptor, ForeignLanguage, ForeignScalar};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const IDENTITY_SCHEMA: &str = "jet-ffi-bridge-identity-v1";
pub const PROVENANCE_SCHEMA: &str = "jet-ffi-bridge-provenance-v1";

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
    functions: &[ScalarBridgeFunction],
) -> Result<PathBuf, String> {
    fs::create_dir_all(cache)
        .map_err(|error| format!("could not create foreign binding cache: {error}"))?;
    let c_path = cache.join(format!("{abi}.c"));
    let object = cache.join(format!("{abi}.o"));
    let archive = cache.join(format!("lib{abi}.a"));
    let _ = fs::remove_file(&archive);
    fs::write(
        &c_path,
        render_scalar_c(abi, runtime, worker, source, functions),
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
        .arg(&archive)
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
    Ok(archive)
}

/// Publish a queryable descriptor/provenance record for a scalar adapter.
pub fn write_scalar_provenance(
    cache: &Path,
    lib: &str,
    language: ForeignLanguage,
    runtime: &str,
    source: &Path,
    worker: &Path,
    archive: &Path,
) -> Result<PathBuf, String> {
    let descriptor_row = binder_descriptor(language)
        .ok_or_else(|| format!("no foreign binder descriptor for `{}`", language.root()))?;
    let contract = descriptor_row.contract;
    let descriptor = contract.stamp();
    let calling = format!("{:?}", contract.calling_convention);
    let layout = format!("{:?}", contract.layout);
    let ownership = format!("{:?}", contract.ownership);
    let errors = format!("{:?}", contract.errors);
    let callbacks = format!("{:?}", contract.callbacks);
    let async_completion = format!("{:?}", contract.async_completion);
    let task_boundary = format!("{:?}", contract.task_boundary);
    let safety = format!("{:?}", contract.safety);
    let provider = format!("{:?}", descriptor_row.provider);
    let cc = tool_identity("cc");
    let ar = tool_identity("ar");
    let source_text = source.to_string_lossy().into_owned();
    let worker_text = worker.to_string_lossy().into_owned();
    let mut identity = IdentityBuilder::new(IDENTITY_SCHEMA);
    identity.field("language", language.root().as_bytes());
    identity.field("library", lib.as_bytes());
    identity.field("runtime", runtime.as_bytes());
    identity.field("descriptor", descriptor.as_bytes());
    identity.field("calling", calling.as_bytes());
    identity.field("layout", layout.as_bytes());
    identity.field("ownership", ownership.as_bytes());
    identity.field("errors", errors.as_bytes());
    identity.field("callbacks", callbacks.as_bytes());
    identity.field("async", async_completion.as_bytes());
    identity.field("tasks", task_boundary.as_bytes());
    identity.field("safety", safety.as_bytes());
    identity.field("provider", provider.as_bytes());
    identity.field("cc", cc.as_bytes());
    identity.field("ar", ar.as_bytes());
    identity.field("source", source_text.as_bytes());
    identity.field("worker", worker_text.as_bytes());
    let identity = identity.finish();
    let worker_digest = sha_file(worker)?;
    let archive_digest = sha_file(archive)?;
    let provenance = cache.join(format!("{lib}.provenance"));
    let fields = [
        ("language", language.root().to_string()),
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
        ("cc", cc),
        ("ar", ar),
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
    effect: &str,
    contract: &str,
    functions: &[ScalarBridgeFunction],
) -> String {
    let mut out = format!("// jet-ffi-descriptor={contract}\n#Extern module c.{abi} {{\n");
    for function in functions {
        let _ = write!(out, "    fn {}(", function.name);
        for (index, scalar) in function.params.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            let _ = write!(out, "arg{index}: {}", scalar.jet_name().unwrap_or("Int"));
        }
        let _ = writeln!(
            out,
            ") {} = \"{abi}_{}\"",
            function.result.jet_name().unwrap_or("Int"),
            function.name
        );
    }
    let _ = writeln!(out, "    fn take_error() Int = \"{abi}_take_error\"\n}}");
    let _ = writeln!(out, "use c.{abi} as abi\n");
    for function in functions {
        let _ = write!(out, "pub fn {}(", function.name);
        for (index, scalar) in function.params.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            let _ = write!(out, "arg{index}: {}", scalar.jet_name().unwrap_or("Int"));
        }
        let _ = writeln!(
            out,
            ") {} String! -[{effect}]> {{",
            function.result.jet_name().unwrap_or("Int")
        );
        let _ = write!(out, "    value :: abi.{}(", function.name);
        for index in 0..function.params.len() {
            if index > 0 {
                out.push_str(", ");
            }
            let _ = write!(out, "arg{index}");
        }
        out.push_str(")\n    if abi.take_error() != 0 {\n        return Err(\"foreign call failed\")\n    }\n    return Ok(value)\n}\n\n");
    }
    out
}

fn render_scalar_c(
    abi: &str,
    runtime: &str,
    worker: &Path,
    source: &Path,
    functions: &[ScalarBridgeFunction],
) -> String {
    let worker = c_escape(&shell_quote(&worker.to_string_lossy()));
    let source = c_escape(&shell_quote(&source.to_string_lossy()));
    let mut out = String::from(
        "#include <ctype.h>\n#include <errno.h>\n#include <stdbool.h>\n#include <stdint.h>\n#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\nstatic _Thread_local int64_t jet_failed;\nstatic int jet_invoke(const char *command, char *output, size_t capacity) {\n    jet_failed = 0;\n    FILE *pipe = popen(command, \"r\");\n    if (!pipe) { jet_failed = 1; return 0; }\n    if (!fgets(output, (int)capacity, pipe)) { jet_failed = 2; pclose(pipe); return 0; }\n    int status = pclose(pipe);\n    if (status != 0 || strncmp(output, \"OK \", 3) != 0) { jet_failed = 3; return 0; }\n    return 1;\n}\n",
    );
    let _ = writeln!(
        out,
        "int64_t {abi}_take_error(void) {{ int64_t value = jet_failed; jet_failed = 0; return value; }}"
    );
    for function in functions {
        let _ = write!(out, "{} {abi}_{}(", c_type(function.result), function.name);
        for (index, scalar) in function.params.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            let _ = write!(out, "{} arg{index}", c_type(*scalar));
        }
        out.push_str(") {\n    char output[256];\n    char command[8192];\n    int written = snprintf(command, sizeof(command), \"");
        out.push_str(runtime);
        out.push(' ');
        out.push_str(&worker);
        out.push(' ');
        out.push_str(&source);
        out.push(' ');
        out.push_str(&c_escape(&shell_quote(&function.name)));
        for scalar in &function.params {
            out.push_str(format_spec(*scalar));
        }
        if function.params.is_empty() {
            out.push_str(" 2>/dev/null\");\n");
        } else {
            out.push_str(" 2>/dev/null\", ");
            for (index, scalar) in function.params.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format_arg(*scalar, index));
            }
            out.push_str(");\n");
        }
        out.push_str("    if (written < 0 || (size_t)written >= sizeof(command) || !jet_invoke(command, output, sizeof(output))) return (");
        out.push_str(zero_value(function.result));
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
            _ => out.push_str("jet_failed = 4; return 0;\n"),
        }
        out.push_str("    while (*end && isspace((unsigned char)*end)) end++;\n    if (*end) { jet_failed = 4; return ");
        out.push_str(zero_value(function.result));
        out.push_str("; }\n    return value;\n}\n");
    }
    out
}

fn c_type(scalar: ForeignScalar) -> &'static str {
    match scalar {
        ForeignScalar::Int => "int64_t",
        ForeignScalar::Float => "double",
        ForeignScalar::Bool => "bool",
        _ => "int64_t",
    }
}

fn format_spec(scalar: ForeignScalar) -> &'static str {
    match scalar {
        ForeignScalar::Int => " %lld",
        ForeignScalar::Float => " %.17g",
        ForeignScalar::Bool => " %d",
        _ => " %d",
    }
}

fn format_arg(scalar: ForeignScalar, index: usize) -> String {
    match scalar {
        ForeignScalar::Int => format!("(long long)arg{index}"),
        ForeignScalar::Float => format!("arg{index}"),
        ForeignScalar::Bool => format!("(int)(arg{index} ? 1 : 0)"),
        _ => "0".to_string(),
    }
}

fn zero_value(scalar: ForeignScalar) -> &'static str {
    match scalar {
        ForeignScalar::Float => "0.0",
        ForeignScalar::Bool => "false",
        _ => "0",
    }
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
    path.canonicalize().unwrap_or(path).display().to_string()
}

fn sha_file(path: &Path) -> Result<String, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    Ok(crate::SHA256::sha256_hex(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
