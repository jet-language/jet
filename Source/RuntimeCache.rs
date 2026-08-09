//! Content-addressed native runtime rlib cache.
//!
//! Codegen keeps emitting one complete Rust program for inspection and I1/I2
//! audits. Native builders split its marked, canonical runtime block, compile
//! that dependency once, then compile and link the user program normally.

use crate::SHA256::sha256_hex;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const CACHE_SCHEMA: &[u8] = b"jet-runtime-rlib-v1";
const CRATE_NAME: &str = "jet_runtime";
const RUNTIME_CRATE_PREFIX: &str = "#![allow(warnings)]\n";
const BEGIN: &str = crate::Codegen::CACHED_RUNTIME_BEGIN;
const END: &str = crate::Codegen::CACHED_RUNTIME_END;
const DIGEST_LEN: usize = 64;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub enum Error {
    Tool(String),
    Cache(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Tool(message) | Error::Cache(message) => {
                formatter.write_str(message)
            }
        }
    }
}

pub struct PreparedRuntime {
    rust: String,
    rlib: Option<PathBuf>,
    cache_hit: bool,
}

impl PreparedRuntime {
    pub fn inline(rust: &str) -> Self {
        Self {
            rust: rust.to_string(),
            rlib: None,
            cache_hit: false,
        }
    }

    pub fn rust(&self) -> &str {
        &self.rust
    }

    pub fn cache_hit(&self) -> bool {
        self.cache_hit
    }

    pub fn add_rustc_args(&self, command: &mut Command) {
        if let Some(rlib) = &self.rlib {
            command
                .arg("--extern")
                .arg(format!("{CRATE_NAME}={}", rlib.display()));
        }
    }
}

/// Prepare the generated program for one native rustc invocation.
///
/// `rustc_flags` and `rustc_env` must match the user-crate invocation. They
/// are applied to the runtime compile and included byte-for-byte in its key.
pub fn prepare(
    rustc: &OsStr,
    generated: &str,
    rustc_flags: &[OsString],
    rustc_env: &[(OsString, OsString)],
) -> Result<PreparedRuntime, Error> {
    prepare_at(
        &cache_root(),
        rustc,
        generated,
        rustc_flags,
        rustc_env,
    )
}

fn cache_root() -> PathBuf {
    if let Ok(path) = std::env::var("JET_RUNTIME_CACHE_DIR") {
        return PathBuf::from(path);
    }
    if let Ok(path) = std::env::var("JET_CACHE_DIR") {
        return PathBuf::from(path).join("runtime");
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".cache").join("jet").join("runtime")
}

fn prepare_at(
    root: &Path,
    rustc: &OsStr,
    generated: &str,
    rustc_flags: &[OsString],
    rustc_env: &[(OsString, OsString)],
) -> Result<PreparedRuntime, Error> {
    // Cache is an optimization; malformed boundaries must keep original Rust.
    let split = match split_generated(generated) {
        Ok(split) => split,
        Err(Error::Cache(_)) => return Ok(PreparedRuntime::inline(generated)),
        Err(error) => return Err(error),
    };
    let Some((runtime, program)) = split else {
        return Ok(PreparedRuntime::inline(generated));
    };
    let exported = export_runtime_source(&runtime);
    let rustc_version = rustc_identity(rustc, rustc_env)?;
    let key = cache_key(
        &runtime,
        &exported,
        rustc,
        &rustc_version,
        rustc_flags,
        rustc_env,
    );
    let entry = root.join(&key);
    safe_dir(root, &entry)?;
    fs::create_dir_all(&entry)
        .map_err(|error| Error::Cache(format!("could not create {}: {error}", entry.display())))?;
    let rlib = entry.join(format!("lib{CRATE_NAME}.rlib"));
    if verified_artifact(&rlib) {
        return Ok(PreparedRuntime {
            rust: program,
            rlib: Some(rlib),
            cache_hit: true,
        });
    }

    let _lock = BuildLock::acquire(&entry)?;
    if verified_artifact(&rlib) {
        return Ok(PreparedRuntime {
            rust: program,
            rlib: Some(rlib),
            cache_hit: true,
        });
    }

    let temporary_id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let source = entry.join(format!(
        ".runtime.{}.{temporary_id}.rs",
        std::process::id()
    ));
    let staged_rlib = entry.join(format!(
        ".lib{CRATE_NAME}.{}.{temporary_id}.rlib",
        std::process::id()
    ));
    fs::write(&source, format!("{RUNTIME_CRATE_PREFIX}{exported}"))
        .map_err(|error| Error::Cache(format!("could not write {}: {error}", source.display())))?;

    let mut command = Command::new(rustc);
    command
        .args(["--edition", "2021", "--crate-name", CRATE_NAME, "--crate-type", "rlib"])
        .args(rustc_flags)
        .arg(&source)
        .arg("-o")
        .arg(&staged_rlib);
    for (name, value) in rustc_env {
        command.env(name, value);
    }
    let output = command.output().map_err(|error| {
        let _ = fs::remove_file(&source);
        let _ = fs::remove_file(&staged_rlib);
        Error::Tool(format!("could not run rustc for cached runtime: {error}"))
    })?;
    let _ = fs::remove_file(&source);
    if !output.status.success() {
        let _ = fs::remove_file(&staged_rlib);
        // A cache-only rustc rejection must never replace a valid inline build.
        return Ok(PreparedRuntime::inline(generated));
    }

    let bytes = fs::read(&staged_rlib).map_err(|error| {
        Error::Cache(format!("could not read {}: {error}", staged_rlib.display()))
    })?;
    let digest = sha256_hex(&bytes);
    let _ = fs::remove_file(&rlib);
    fs::rename(&staged_rlib, &rlib)
        .map_err(|error| Error::Cache(format!("could not publish {}: {error}", rlib.display())))?;
    publish(&entry.join("artifact.sha256"), format!("{digest}\n").as_bytes())?;
    publish(
        &entry.join("runtime.rs"),
        format!("{RUNTIME_CRATE_PREFIX}{exported}").as_bytes(),
    )?;
    Ok(PreparedRuntime {
        rust: program,
        rlib: Some(rlib),
        cache_hit: false,
    })
}

fn split_generated(generated: &str) -> Result<Option<(String, String)>, Error> {
    let Some(begin) = generated.find(BEGIN) else {
        return Ok(None);
    };
    if generated.matches(BEGIN).count() != 1 || generated.matches(END).count() != 1 {
        return Err(Error::Cache(
            "generated Rust has an invalid runtime marker pair".to_string(),
        ));
    }
    let runtime_start = begin + BEGIN.len();
    let relative_end = generated[runtime_start..]
        .find(END)
        .ok_or_else(|| Error::Cache("generated Rust has an unterminated runtime block".to_string()))?;
    let runtime_end = runtime_start + relative_end;
    let after = runtime_end + END.len();
    let runtime = generated[runtime_start..runtime_end].to_string();
    let mut program = String::with_capacity(generated.len() - runtime.len() + 64);
    program.push_str(&generated[..begin]);
    program.push_str("extern crate jet_runtime;\nuse jet_runtime::*;\n");
    program.push_str(&generated[after..]);
    Ok(Some((runtime, program)))
}

fn cache_key(
    runtime: &str,
    exported_runtime: &str,
    rustc: &OsStr,
    rustc_version: &str,
    rustc_flags: &[OsString],
    rustc_env: &[(OsString, OsString)],
) -> String {
    let mut data = Vec::new();
    push_bytes(&mut data, CACHE_SCHEMA);
    push_bytes(&mut data, runtime.as_bytes());
    push_bytes(&mut data, RUNTIME_CRATE_PREFIX.as_bytes());
    push_bytes(&mut data, exported_runtime.as_bytes());
    push_bytes(&mut data, &os_bytes(rustc));
    push_bytes(&mut data, rustc_version.as_bytes());
    push_bytes(&mut data, b"flags");
    push_bytes(&mut data, &(rustc_flags.len() as u64).to_be_bytes());
    for flag in rustc_flags {
        push_bytes(&mut data, &os_bytes(flag));
    }
    push_bytes(&mut data, b"env");
    push_bytes(&mut data, &(rustc_env.len() as u64).to_be_bytes());
    for (name, value) in rustc_env {
        push_bytes(&mut data, &os_bytes(name));
        push_bytes(&mut data, &os_bytes(value));
    }
    sha256_hex(&data)
}

fn push_bytes(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&(value.len() as u64).to_be_bytes());
    out.extend_from_slice(value);
}

#[cfg(unix)]
fn os_bytes(value: &OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    value.as_bytes().to_vec()
}

#[cfg(windows)]
fn os_bytes(value: &OsStr) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    value
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>()
}

#[cfg(not(any(unix, windows)))]
fn os_bytes(value: &OsStr) -> Vec<u8> {
    value.to_string_lossy().as_bytes().to_vec()
}

fn rustc_identity(
    rustc: &OsStr,
    rustc_env: &[(OsString, OsString)],
) -> Result<String, Error> {
    static IDENTITIES: OnceLock<Mutex<HashMap<Vec<u8>, String>>> = OnceLock::new();
    let mut identity_key = Vec::new();
    push_bytes(&mut identity_key, &os_bytes(rustc));
    push_bytes(
        &mut identity_key,
        &(rustc_env.len() as u64).to_be_bytes(),
    );
    for (name, value) in rustc_env {
        push_bytes(&mut identity_key, &os_bytes(name));
        push_bytes(&mut identity_key, &os_bytes(value));
    }
    let identities = IDENTITIES.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(identity) = identities.lock().unwrap().get(&identity_key).cloned() {
        return Ok(identity);
    }
    let mut command = Command::new(rustc);
    command.arg("-vV");
    for (name, value) in rustc_env {
        command.env(name, value);
    }
    let output = command
        .output()
        .map_err(|error| Error::Tool(format!("could not run rustc -vV: {error}")))?;
    if !output.status.success() {
        return Err(Error::Tool(format!(
            "rustc -vV failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let identity = String::from_utf8_lossy(&output.stdout).into_owned();
    identities
        .lock()
        .unwrap()
        .insert(identity_key, identity.clone());
    Ok(identity)
}

fn safe_dir(root: &Path, entry: &Path) -> Result<(), Error> {
    for path in [root, entry] {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(Error::Cache(format!(
                    "unsafe runtime-cache directory {}",
                    path.display()
                )))
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(Error::Cache(format!(
                    "could not inspect {}: {error}",
                    path.display()
                )))
            }
        }
    }
    Ok(())
}

fn regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_file())
        .unwrap_or(false)
}

fn verified_artifact(path: &Path) -> bool {
    if !regular_file(path) {
        return false;
    }
    let Some(entry) = path.parent() else {
        return false;
    };
    let digest = entry.join("artifact.sha256");
    if !regular_file(&digest) {
        return false;
    }
    let Ok(record) = fs::read(&digest) else {
        return false;
    };
    if record.len() != DIGEST_LEN + 1 || record[DIGEST_LEN] != b'\n' {
        return false;
    }
    let Ok(expected) = std::str::from_utf8(&record[..DIGEST_LEN]) else {
        return false;
    };
    let valid_digest = expected
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase());
    valid_digest
        && fs::read(path)
            .map(|bytes| sha256_hex(&bytes) == expected)
            .unwrap_or(false)
}

fn publish(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let Some(parent) = path.parent() else {
        return Err(Error::Cache(format!("invalid cache path {}", path.display())));
    };
    let name = path.file_name().and_then(OsStr::to_str).unwrap_or("artifact");
    let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".{name}.{}.{id}.tmp", std::process::id()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| {
                Error::Cache(format!("could not stage {}: {error}", path.display()))
            })?;
        file.write_all(bytes).map_err(|error| {
            Error::Cache(format!("could not stage {}: {error}", path.display()))
        })?;
        file.sync_all().map_err(|error| {
            Error::Cache(format!("could not flush {}: {error}", path.display()))
        })?;
        fs::rename(&temporary, path).map_err(|error| {
            Error::Cache(format!("could not publish {}: {error}", path.display()))
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

struct BuildLock {
    path: PathBuf,
}

impl BuildLock {
    fn acquire(entry: &Path) -> Result<Self, Error> {
        let path = entry.join(".build-lock");
        loop {
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let stale = fs::metadata(&path)
                        .and_then(|metadata| metadata.modified())
                        .and_then(|modified| {
                            modified.elapsed().map_err(|error| {
                                std::io::Error::new(std::io::ErrorKind::Other, error)
                            })
                        })
                        .map(|age| age > Duration::from_secs(300))
                        .unwrap_or(false);
                    if stale {
                        let _ = fs::remove_dir(&path);
                        continue;
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(error) => {
                    return Err(Error::Cache(format!(
                        "could not lock {}: {error}",
                        entry.display()
                    )))
                }
            }
        }
    }
}

impl Drop for BuildLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.path);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Module,
    Struct,
    Enum,
    Trait,
    InherentImpl,
    TraitImpl,
    Function,
    Other,
}

/// Make the generated runtime boundary callable from its dependent user crate.
/// Prelude files remain canonical and unchanged; visibility changes exist only
/// in the cached build artifact.
fn export_runtime_source(source: &str) -> String {
    let mask = rust_code_mask(source);
    let source_lines = source.split_inclusive('\n').collect::<Vec<_>>();
    let mask_lines = mask.split_inclusive('\n').collect::<Vec<_>>();
    let mut out = String::with_capacity(source.len() + source.len() / 40);
    let mut scopes = vec![(Scope::Module, 0usize)];
    let mut depth = 0usize;
    let mut pending = String::new();

    for (line, masked) in source_lines.into_iter().zip(mask_lines) {
        while scopes.last().is_some_and(|(_, level)| *level > depth) {
            scopes.pop();
        }
        let scope = scopes.last().map(|(scope, _)| *scope).unwrap_or(Scope::Other);
        let direct = scopes.last().is_some_and(|(_, level)| *level == depth);
        let rewritten = if direct {
            export_line(line, masked, scope)
        } else {
            line.to_string()
        };
        out.push_str(&rewritten);

        let code = masked.trim();
        if direct && pending.is_empty() && starts_item_header(code, scope) {
            pending.push_str(code);
            pending.push(' ');
        } else if direct && !pending.is_empty() {
            pending.push_str(code);
            pending.push(' ');
        }

        for byte in masked.bytes() {
            match byte {
                b'{' => {
                    let kind = if direct && !pending.is_empty() {
                        scope_for_header(&pending)
                    } else {
                        Scope::Other
                    };
                    depth += 1;
                    scopes.push((kind, depth));
                    pending.clear();
                }
                b'}' => {
                    if depth > 0 {
                        depth -= 1;
                    }
                    while scopes.last().is_some_and(|(_, level)| *level > depth) {
                        scopes.pop();
                    }
                    pending.clear();
                }
                b';' if direct => pending.clear(),
                _ => {}
            }
        }
    }
    out
}

fn export_line(line: &str, masked: &str, scope: Scope) -> String {
    let code = masked.trim_start();
    if code.is_empty() || code.starts_with('#') {
        return line.to_string();
    }
    let should_export = match scope {
        Scope::Module => starts_exportable_item(code),
        Scope::Struct => looks_like_struct_field(code),
        Scope::InherentImpl => starts_impl_member(code),
        _ => false,
    };
    if !should_export {
        return line.to_string();
    }
    let indent = line.len() - line.trim_start().len();
    let body = &line[indent..];
    let exported = if let Some(rest) = restricted_visibility_rest(body) {
        format!("{}pub {}", &line[..indent], rest)
    } else if body.starts_with("pub ") {
        line.to_string()
    } else {
        format!("{}pub {}", &line[..indent], body)
    };
    if scope == Scope::Module && is_tuple_struct(code) {
        export_tuple_field(exported)
    } else {
        exported
    }
}

fn is_tuple_struct(code: &str) -> bool {
    let code = strip_visibility(code);
    code.starts_with("struct ") && code.contains('(') && !code.contains('{')
}

fn export_tuple_field(mut line: String) -> String {
    let Some(struct_at) = line.find("struct ") else {
        return line;
    };
    let Some(relative_open) = line[struct_at + "struct ".len()..].find('(') else {
        return line;
    };
    let open = struct_at + "struct ".len() + relative_open;
    if !line[open + 1..].starts_with("pub ") {
        line.insert_str(open + 1, "pub ");
    }
    line
}

fn restricted_visibility_rest(value: &str) -> Option<&str> {
    let rest = value.strip_prefix("pub(")?;
    let close = rest.find(')')?;
    rest.get(close + 1..)?.strip_prefix(' ')
}

fn strip_visibility(code: &str) -> &str {
    if let Some(rest) = code.strip_prefix("pub ") {
        return rest;
    }
    if let Some(rest) = code.strip_prefix("pub(") {
        if let Some(close) = rest.find(')') {
            return rest[close + 1..].trim_start();
        }
    }
    code
}

fn starts_exportable_item(code: &str) -> bool {
    let code = strip_visibility(code);
    ["fn ", "struct ", "enum ", "union ", "trait ", "type ", "const ", "static ", "mod "]
        .iter()
        .any(|prefix| code.starts_with(prefix))
        || ["unsafe fn ", "async fn ", "const fn ", "async unsafe fn "]
            .iter()
            .any(|prefix| code.starts_with(prefix))
        || ((code.starts_with("unsafe extern ") || code.starts_with("extern "))
            && code.contains(" fn "))
}

fn starts_impl_member(code: &str) -> bool {
    let code = strip_visibility(code);
    ["fn ", "const ", "type ", "unsafe fn ", "async fn ", "const fn "]
        .iter()
        .any(|prefix| code.starts_with(prefix))
}

fn looks_like_struct_field(code: &str) -> bool {
    let code = strip_visibility(code);
    let Some(colon) = code.find(':') else {
        return false;
    };
    if code[..colon].ends_with(':') || code.as_bytes().get(colon + 1) == Some(&b':') {
        return false;
    }
    let name = code[..colon].trim();
    !name.is_empty()
        && !name.chars().any(char::is_whitespace)
        && name.bytes().all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

fn starts_item_header(code: &str, scope: Scope) -> bool {
    match scope {
        Scope::Module => {
            let code = strip_visibility(code);
            starts_exportable_item(code)
                || code.starts_with("impl")
                || code.starts_with("unsafe impl")
                || code.starts_with("extern ")
        }
        Scope::InherentImpl | Scope::TraitImpl | Scope::Trait => starts_impl_member(code),
        _ => false,
    }
}

fn scope_for_header(header: &str) -> Scope {
    let header = strip_visibility(header.trim_start());
    if header.starts_with("struct ") || header.starts_with("union ") {
        Scope::Struct
    } else if header.starts_with("enum ") {
        Scope::Enum
    } else if header.starts_with("trait ") {
        Scope::Trait
    } else if header.starts_with("impl") || header.starts_with("unsafe impl") {
        if header.contains(" for ") {
            Scope::TraitImpl
        } else {
            Scope::InherentImpl
        }
    } else if header.starts_with("mod ") {
        Scope::Module
    } else if header.contains("fn ") {
        Scope::Function
    } else {
        Scope::Other
    }
}

fn rust_code_mask(source: &str) -> String {
    #[derive(Clone, Copy)]
    enum State {
        Code,
        LineComment,
        BlockComment(usize),
        String(bool),
        RawString(usize),
        Char(bool),
    }

    let bytes = source.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut state = State::Code;
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        match state {
            State::Code => {
                if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
                    out.extend_from_slice(b"  ");
                    index += 2;
                    state = State::LineComment;
                } else if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
                    out.extend_from_slice(b"  ");
                    index += 2;
                    state = State::BlockComment(1);
                } else if let Some((prefix_len, hashes)) = raw_string_start(&bytes[index..]) {
                    out.extend(std::iter::repeat(b' ').take(prefix_len));
                    index += prefix_len;
                    state = State::RawString(hashes);
                } else if byte == b'"' || (byte == b'b' && bytes.get(index + 1) == Some(&b'"')) {
                    let len = if byte == b'b' { 2 } else { 1 };
                    out.extend(std::iter::repeat(b' ').take(len));
                    index += len;
                    state = State::String(false);
                } else if byte == b'\'' && char_literal_end(&bytes[index..]).is_some() {
                    out.push(b' ');
                    index += 1;
                    state = State::Char(false);
                } else {
                    out.push(byte);
                    index += 1;
                }
            }
            State::LineComment => {
                if byte == b'\n' {
                    out.push(byte);
                    state = State::Code;
                } else {
                    out.push(b' ');
                }
                index += 1;
            }
            State::BlockComment(depth) => {
                if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
                    out.extend_from_slice(b"  ");
                    index += 2;
                    state = State::BlockComment(depth + 1);
                } else if byte == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    out.extend_from_slice(b"  ");
                    index += 2;
                    state = if depth == 1 {
                        State::Code
                    } else {
                        State::BlockComment(depth - 1)
                    };
                } else {
                    out.push(if byte == b'\n' { b'\n' } else { b' ' });
                    index += 1;
                }
            }
            State::String(escaped) => {
                out.push(if byte == b'\n' { b'\n' } else { b' ' });
                index += 1;
                if escaped {
                    state = State::String(false);
                } else if byte == b'\\' {
                    state = State::String(true);
                } else if byte == b'"' {
                    state = State::Code;
                }
            }
            State::RawString(hashes) => {
                let closes = byte == b'"'
                    && bytes
                        .get(index + 1..index + 1 + hashes)
                        .is_some_and(|suffix| suffix.iter().all(|byte| *byte == b'#'));
                out.push(if byte == b'\n' { b'\n' } else { b' ' });
                index += 1;
                if closes {
                    for _ in 0..hashes {
                        out.push(b' ');
                        index += 1;
                    }
                    state = State::Code;
                }
            }
            State::Char(escaped) => {
                out.push(b' ');
                index += 1;
                if escaped {
                    state = State::Char(false);
                } else if byte == b'\\' {
                    state = State::Char(true);
                } else if byte == b'\'' {
                    state = State::Code;
                }
            }
        }
    }
    String::from_utf8(out).expect("Rust source mask preserves UTF-8 bytes")
}

fn raw_string_start(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut index = match bytes.first() {
        Some(b'r') => 1,
        Some(b'b') if bytes.get(1) == Some(&b'r') => 2,
        _ => return None,
    };
    let start = index;
    while bytes.get(index) == Some(&b'#') {
        index += 1;
    }
    (bytes.get(index) == Some(&b'"')).then_some((index + 1, index - start))
}

fn char_literal_end(bytes: &[u8]) -> Option<usize> {
    if bytes.first() != Some(&b'\'') {
        return None;
    }
    if bytes.get(1) != Some(&b'\\') {
        let text = std::str::from_utf8(bytes.get(1..)?).ok()?;
        let character = text.chars().next()?;
        let end = 1 + character.len_utf8();
        return (bytes.get(end) == Some(&b'\'')).then_some(end);
    }

    let mut escaped = false;
    for (index, byte) in bytes.iter().copied().enumerate().skip(1) {
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b'\'' {
            return Some(index);
        } else if byte == b'\n' {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key(
        runtime: &str,
        rustc: &OsStr,
        rustc_version: &str,
        flags: &[OsString],
        environment: &[(OsString, OsString)],
    ) -> String {
        cache_key(
            runtime,
            &export_runtime_source(runtime),
            rustc,
            rustc_version,
            flags,
            environment,
        )
    }

    #[test]
    fn key_covers_runtime_rustc_flags_and_environment() {
        let base = test_key("fn x() {}", OsStr::new("rustc"), "rustc 1", &[], &[]);
        assert_ne!(
            base,
            test_key("fn y() {}", OsStr::new("rustc"), "rustc 1", &[], &[])
        );
        assert_ne!(
            base,
            test_key("fn x() {}", OsStr::new("rustc"), "rustc 2", &[], &[])
        );
        assert_ne!(
            base,
            test_key("fn x() {}", OsStr::new("other-rustc"), "rustc 1", &[], &[])
        );
        assert_ne!(
            test_key(
                "fn x() {}",
                OsStr::new("rustc"),
                "rustc 1",
                &[OsString::from("-C"), OsString::from("opt-level=3")],
                &[]
            ),
            test_key(
                "fn x() {}",
                OsStr::new("rustc"),
                "rustc 1",
                &[OsString::from("opt-level=3"), OsString::from("-C")],
                &[]
            )
        );
        assert_ne!(
            base,
            test_key(
                "fn x() {}",
                OsStr::new("rustc"),
                "rustc 1",
                &[OsString::from("-O")],
                &[]
            )
        );
        assert_ne!(
            base,
            test_key(
                "fn x() {}",
                OsStr::new("rustc"),
                "rustc 1",
                &[],
                &[(OsString::from("RUSTFLAGS"), OsString::from("-Ctarget-cpu=native"))]
            )
        );
    }

    #[test]
    fn split_keeps_real_user_program_compile() {
        let generated = format!(
            "#![allow(warnings)]\n{BEGIN}fn runtime() {{}}\n{END}fn main() {{ runtime(); }}\n"
        );
        let (runtime, program) = split_generated(&generated).unwrap().unwrap();
        assert_eq!(runtime, "fn runtime() {}\n");
        assert!(program.contains("extern crate jet_runtime;"));
        assert!(program.contains("fn main() { runtime(); }"));
        assert!(!program.contains("fn runtime()"));
    }

    #[test]
    fn split_keeps_header_before_cached_runtime_import() {
        let generated = format!(
            "#![allow(warnings)]\nconst __JET_PACKAGE_EDITION: u16 = 2027;\nextern crate helper;\n{BEGIN}pub trait user_Display {{}}\n{END}use user_Display;\n"
        );
        let (_, program) = split_generated(&generated).unwrap().unwrap();
        let edition = program.find("__JET_PACKAGE_EDITION").unwrap();
        let helper = program.find("extern crate helper;").unwrap();
        let runtime = program.find("extern crate jet_runtime;").unwrap();
        let import = program.find("use jet_runtime::*;").unwrap();
        let tail = program.find("use user_Display;").unwrap();
        assert!(edition < helper && helper < runtime && runtime < import && import < tail);
    }

    #[test]
    fn malformed_runtime_markers_fall_back_to_inline() {
        let generated = format!("{BEGIN}fn runtime() {{}}\n");
        let prepared = prepare_at(
            Path::new("unused-cache-root"),
            OsStr::new("rustc-not-called"),
            &generated,
            &[],
            &[],
        )
        .unwrap();
        assert_eq!(prepared.rust(), generated);
        assert!(!prepared.cache_hit());
    }

    #[test]
    fn export_changes_visibility_without_touching_literals_or_trait_impls() {
        let source = r#"struct Tuple(i64);
struct Value {
    field: i64,
}
impl Value {
    fn new() -> Self { Self { field: 1 } }
}
impl Clone for Value {
    fn clone(&self) -> Self { Self { field: self.field } }
}
fn braces() -> &'static str { "{private}" }
"#;
        let exported = export_runtime_source(source);
        assert!(exported.contains("pub struct Tuple(pub i64);"));
        assert!(exported.contains("pub struct Value"));
        assert!(exported.contains("    pub field: i64"));
        assert!(exported.contains("    pub fn new()"));
        assert!(exported.contains("impl Clone for Value {\n    fn clone"));
        assert!(exported.contains("pub fn braces()"));
        assert!(exported.contains("\"{private}\""));
    }

    #[test]
    fn export_does_not_promote_multiline_generic_type_continuations() {
        let source = r#"struct Protocol {
    waiters: std::sync::Mutex<
        std::collections::VecDeque<u8>,
    >,
}
"#;
        let exported = export_runtime_source(source);
        assert!(exported.contains("    pub waiters: std::sync::Mutex<"));
        assert!(exported.contains("\n        std::collections::VecDeque<u8>,"));
        assert!(!exported.contains("\n        pub std::collections::VecDeque"));
    }

    #[cfg(unix)]
    #[test]
    fn cold_build_then_warm_hit_and_source_invalidation() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "jet-runtime-cache-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(&root).unwrap();
        let rustc = root.join("rustc-fake");
        let count = root.join("count");
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = \"-vV\" ]; then echo 'rustc 1.99.0 fake'; exit 0; fi\nout=''\nwhile [ \"$#\" -gt 0 ]; do if [ \"$1\" = \"-o\" ]; then shift; out=\"$1\"; fi; shift; done\nprintf artifact > \"$out\"\nprintf x >> '{}'\n",
            count.display()
        );
        fs::write(&rustc, script).unwrap();
        let mut permissions = fs::metadata(&rustc).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&rustc, permissions).unwrap();

        let one = format!("{BEGIN}fn runtime() {{}}\n{END}fn main() {{}}\n");
        let first = prepare_at(&root.join("cache"), rustc.as_os_str(), &one, &[], &[]).unwrap();
        let second = prepare_at(&root.join("cache"), rustc.as_os_str(), &one, &[], &[]).unwrap();
        assert!(!first.cache_hit());
        assert!(second.cache_hit());
        assert_eq!(fs::read(&count).unwrap(), b"x");

        fs::write(second.rlib.as_ref().unwrap(), b"corrupt!").unwrap();
        let repaired = prepare_at(&root.join("cache"), rustc.as_os_str(), &one, &[], &[]).unwrap();
        assert!(!repaired.cache_hit());
        assert_eq!(fs::read(&count).unwrap(), b"xx");

        let two = format!("{BEGIN}fn runtime_changed() {{}}\n{END}fn main() {{}}\n");
        prepare_at(&root.join("cache"), rustc.as_os_str(), &two, &[], &[]).unwrap();
        assert_eq!(fs::read(&count).unwrap(), b"xxx");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn rejected_cached_runtime_falls_back_to_inline_program() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "jet-runtime-cache-rejected-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(&root).unwrap();
        let rustc = root.join("rustc-rejecting");
        fs::write(
            &rustc,
            "#!/bin/sh\nif [ \"$1\" = \"-vV\" ]; then exit 0; fi\nexit 1\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&rustc).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&rustc, permissions).unwrap();

        let generated = format!("prefix\n{BEGIN}fn runtime() {{}}\n{END}suffix\n");
        let prepared = prepare_at(&root.join("cache"), rustc.as_os_str(), &generated, &[], &[])
            .unwrap();
        assert_eq!(prepared.rust(), generated);
        assert!(!prepared.cache_hit());
        let _ = fs::remove_dir_all(root);
    }
}
