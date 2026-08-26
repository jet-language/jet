//! Content-addressed native runtime rlib cache.
//!
//! Codegen keeps emitting one complete Rust program for inspection and I1/I2
//! audits. Native builders extract its marked Prelude/runtime and Core blocks,
//! compile that mutually dependent closure once, then link the user program.

use crate::SHA256::sha256_hex;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// v7: the fixed Prelude/runtime and selected Core closure are separate crates.
// Core depends on runtime, so a Core-only change does not rebuild the fixed
// runtime object.
const CACHE_SCHEMA: &[u8] = b"jet-runtime-core-rlib-v7";
const RUNTIME_CRATE_NAME: &str = "jet_runtime";
const CORE_CRATE_NAME: &str = "jet_runtime_core";
const RUNTIME_CRATE_PREFIX: &str = "#![allow(warnings)]\n";
const CORE_CRATE_PREFIX: &str =
    "#![allow(warnings)]\nextern crate jet_runtime;\nuse jet_runtime::*;\n";
const BEGIN: &str = crate::Codegen::CACHED_RUNTIME_BEGIN;
const END: &str = crate::Codegen::CACHED_RUNTIME_END;
const CORE_BEGIN: &str = crate::Codegen::CACHED_CORE_BEGIN;
const CORE_END: &str = crate::Codegen::CACHED_CORE_END;
const DIGEST_LEN: usize = 64;

/// Maximum logical size of the shared runtime rlib cache. Writes evict the
/// oldest published entries until this 512 MiB bound holds.
pub const RUNTIME_CACHE_LIMIT_BYTES: u64 = 512 * 1024 * 1024;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub enum Error {
    Tool(String),
    Cache(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Tool(message) | Error::Cache(message) => formatter.write_str(message),
        }
    }
}

pub struct PreparedRuntime {
    rust: String,
    runtime_rlib: Option<PathBuf>,
    core_rlib: Option<PathBuf>,
    cache_hit: bool,
    _runtime_locks: Vec<BuildLock>,
}

impl PreparedRuntime {
    pub fn inline(rust: &str) -> Self {
        Self {
            rust: rust.to_string(),
            runtime_rlib: None,
            core_rlib: None,
            cache_hit: false,
            _runtime_locks: Vec::new(),
        }
    }

    pub fn rust(&self) -> &str {
        &self.rust
    }

    pub fn cache_hit(&self) -> bool {
        self.cache_hit
    }

    /// True when the program crate links the cached runtime rlib instead of
    /// carrying the runtime source itself. A caller that can retry uses this to
    /// tell a split-only rejection apart from a genuine codegen bug.
    pub fn is_split(&self) -> bool {
        self.runtime_rlib.is_some()
    }

    pub fn add_rustc_args(&self, command: &mut Command) {
        if let Some(rlib) = &self.runtime_rlib {
            command
                .arg("--extern")
                .arg(format!("{RUNTIME_CRATE_NAME}={}", rlib.display()));
        }
        if let Some(rlib) = &self.core_rlib {
            command
                .arg("--extern")
                .arg(format!("{CORE_CRATE_NAME}={}", rlib.display()));
        }
    }
}

/// Prepare the generated program for one native rustc invocation.
///
/// `rustc_flags` and `rustc_env` must match the user-crate invocation. Compile
/// flags are applied to the runtime compile and included byte-for-byte in its
/// key; final-link-only flags remain on the user-crate invocation.
pub fn prepare(
    rustc: &OsStr,
    generated: &str,
    rustc_flags: &[OsString],
    rustc_env: &[(OsString, OsString)],
) -> Result<PreparedRuntime, Error> {
    prepare_at(&cache_root(), rustc, generated, rustc_flags, rustc_env)
}

/// The directory holding cached runtime rlibs. Public so a provider that
/// spawns a child compiler can hand the child the same toolchain cache.
///
/// Toolchain-scoped on purpose, and deliberately NOT derived from
/// `JET_CACHE_DIR`: the key contains no project data at all (runtime source,
/// exported source, rustc identity, flags, env), so a project-scoped root just
/// guarantees a cold runtime compile per project. That is now strictly worse
/// than not caching — building the runtime crate and then the thin program
/// costs more than the one monolith it replaces, and it is only ever repaid by
/// the next program that shares the key. Anything that genuinely needs its own
/// runtime artifacts (the golden suite, a child compiler probe) sets
/// `JET_RUNTIME_CACHE_DIR` explicitly.
///
/// The cache is bounded by [`RUNTIME_CACHE_LIMIT_BYTES`]. A successful write
/// prunes the oldest published entry first (FIFO by artifact publication time),
/// while build locks held by live preparations pin entries until their linked
/// program build drops the returned [`PreparedRuntime`].
pub fn cache_root() -> PathBuf {
    if let Ok(path) = std::env::var("JET_RUNTIME_CACHE_DIR") {
        return PathBuf::from(path);
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".cache")
        .join("jet")
        .join("runtime")
}

/// Logical bytes occupied by regular files below [`cache_root`]. Symlinks are
/// ignored; the value is the same byte count used by cache pruning and doctor.
pub fn cache_footprint() -> u64 {
    directory_size(&cache_root())
}

/// `JET_RUNTIME_CACHE_STATS=1` reports one line per prepared program on stderr:
/// `hit` links a cached rlib, `store` compiled one, `inline` fell back to the
/// monolith. Measurement-only, off by default — the numbers behind an
/// "AOT stopped rebuilding the runtime" claim have to come from somewhere.
fn prepare_at(
    root: &Path,
    rustc: &OsStr,
    generated: &str,
    rustc_flags: &[OsString],
    rustc_env: &[(OsString, OsString)],
) -> Result<PreparedRuntime, Error> {
    let prepared = prepare_at_uncounted(root, rustc, generated, rustc_flags, rustc_env)?;
    if std::env::var_os("JET_RUNTIME_CACHE_STATS").is_some() {
        let outcome = match (&prepared.runtime_rlib, prepared.cache_hit) {
            (None, _) => "inline".to_string(),
            (Some(rlib), hit) => format!(
                "{} key={}",
                if hit { "hit" } else { "store" },
                rlib.parent()
                    .and_then(Path::file_name)
                    .and_then(OsStr::to_str)
                    .unwrap_or("?")
            ),
        };
        eprintln!(
            "jet-runtime-cache {outcome} program_bytes={} generated_bytes={}",
            prepared.rust.len(),
            generated.len()
        );
    }
    Ok(prepared)
}

fn prepare_at_uncounted(
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
    let Some(split) = split else {
        return Ok(PreparedRuntime::inline(generated));
    };
    let rustc_version = rustc_identity(rustc, rustc_env)?;
    let compile_flags = runtime_compile_flags(rustc_flags);
    let exported_runtime = export_runtime_source(&split.runtime);
    let runtime_key = cache_key(
        RUNTIME_CRATE_NAME,
        &split.runtime,
        &exported_runtime,
        RUNTIME_CRATE_PREFIX,
        None,
        rustc,
        &rustc_version,
        &compile_flags,
        rustc_env,
    );
    let Some(runtime) = compile_artifact(
        root,
        &runtime_key,
        RUNTIME_CRATE_NAME,
        &exported_runtime,
        RUNTIME_CRATE_PREFIX,
        None,
        rustc,
        &compile_flags,
        rustc_env,
    )?
    else {
        return Ok(PreparedRuntime::inline(generated));
    };
    let runtime_cache_hit = runtime.cache_hit;
    let runtime_path = runtime.path.clone();
    let mut locks = vec![runtime.lock];
    let (core_rlib, core_cache_hit) = if let Some(core) = split.core {
        let exported_core = export_runtime_source(&core);
        let core_key = cache_key(
            CORE_CRATE_NAME,
            &core,
            &exported_core,
            CORE_CRATE_PREFIX,
            Some(&runtime_key),
            rustc,
            &rustc_version,
            &compile_flags,
            rustc_env,
        );
        let Some(core) = compile_artifact(
            root,
            &core_key,
            CORE_CRATE_NAME,
            &exported_core,
            CORE_CRATE_PREFIX,
            Some((RUNTIME_CRATE_NAME, &runtime_path)),
            rustc,
            &compile_flags,
            rustc_env,
        )?
        else {
            return Ok(PreparedRuntime::inline(generated));
        };
        locks.push(core.lock);
        (Some(core.path), core.cache_hit)
    } else {
        (None, true)
    };
    Ok(PreparedRuntime {
        rust: split.program,
        runtime_rlib: Some(runtime_path),
        core_rlib,
        cache_hit: runtime_cache_hit && core_cache_hit,
        _runtime_locks: locks,
    })
}

/// Linker selection and link arguments affect the final user crate, not the
/// reusable runtime/Core closure. Keep them on the final rustc invocation.
fn runtime_compile_flags(flags: &[OsString]) -> Vec<OsString> {
    let mut compile_flags = Vec::with_capacity(flags.len());
    let mut index = 0;
    while index < flags.len() {
        let flag = &flags[index];
        if flag.as_os_str() == OsStr::new("-C") {
            if let Some(value) = flags.get(index + 1) {
                if is_link_only_flag(value) {
                    index += 2;
                    continue;
                }
            }
        } else if is_link_only_flag(flag) {
            index += 1;
            continue;
        }
        compile_flags.push(flag.clone());
        index += 1;
    }
    compile_flags
}

fn is_link_only_flag(flag: &OsStr) -> bool {
    let flag = flag.to_string_lossy();
    flag.starts_with("linker=")
        || flag.starts_with("link-arg=")
        || flag.starts_with("-Clinker=")
        || flag.starts_with("-Clink-arg=")
}

struct SplitGenerated {
    runtime: String,
    core: Option<String>,
    program: String,
}

struct Artifact {
    path: PathBuf,
    cache_hit: bool,
    lock: BuildLock,
}

fn compile_artifact(
    root: &Path,
    key: &str,
    crate_name: &str,
    exported: &str,
    crate_prefix: &str,
    dependency: Option<(&str, &Path)>,
    rustc: &OsStr,
    rustc_flags: &[OsString],
    rustc_env: &[(OsString, OsString)],
) -> Result<Option<Artifact>, Error> {
    let entry = root.join(key);
    safe_dir(root, &entry)?;
    fs::create_dir_all(&entry)
        .map_err(|error| Error::Cache(format!("could not create {}: {error}", entry.display())))?;
    let rlib = entry.join(format!("lib{crate_name}.rlib"));
    let lock = BuildLock::acquire(&entry)?;
    if verified_artifact(&rlib) {
        prune_cache(root)?;
        return Ok(Some(Artifact {
            path: rlib,
            cache_hit: true,
            lock,
        }));
    }

    // The lock serializes writers; the private directory keeps a slow compile
    // safe if another process eventually reaps a stale lock.
    let temporary_id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let staging = entry.join(format!(".build.{}.{temporary_id}", std::process::id()));
    fs::create_dir_all(&staging).map_err(|error| {
        Error::Cache(format!("could not create {}: {error}", staging.display()))
    })?;
    let source = staging.join("runtime.rs");
    let staged_rlib = staging.join(format!("lib{crate_name}.rlib"));
    fs::write(&source, format!("{crate_prefix}{exported}")).map_err(|error| {
        let _ = fs::remove_dir_all(&staging);
        Error::Cache(format!("could not write {}: {error}", source.display()))
    })?;

    let mut command = Command::new(rustc);
    command
        .args([
            "--edition",
            "2021",
            "--crate-name",
            crate_name,
            "--crate-type",
            "rlib",
        ])
        .args(rustc_flags);
    if let Some((dependency_name, dependency_path)) = dependency {
        command
            .arg("--extern")
            .arg(format!("{dependency_name}={}", dependency_path.display()));
    }
    if let Ok(staging_prefix) = fs::canonicalize(&staging) {
        command
            .arg("--remap-path-prefix")
            .arg(format!("{}=jet-runtime-build", staging_prefix.display()));
    }
    command.arg(&source).arg("-o").arg(&staged_rlib);
    for (name, value) in rustc_env {
        command.env(name, value);
    }
    let output = command.output().map_err(|error| {
        let _ = fs::remove_dir_all(&staging);
        Error::Tool(format!("could not run rustc for cached runtime: {error}"))
    })?;
    let _ = fs::remove_file(&source);
    if !output.status.success() {
        #[cfg(test)]
        eprintln!(
            "cached {crate_name} rejection:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let _ = fs::remove_dir_all(&staging);
        // A cache-only rustc rejection must never replace a valid inline build.
        return Ok(None);
    }

    let bytes = fs::read(&staged_rlib).map_err(|error| {
        let _ = fs::remove_dir_all(&staging);
        Error::Cache(format!("could not read {}: {error}", staged_rlib.display()))
    })?;
    let digest = sha256_hex(&bytes);
    let _ = fs::remove_file(&rlib);
    fs::rename(&staged_rlib, &rlib).map_err(|error| {
        let _ = fs::remove_dir_all(&staging);
        Error::Cache(format!("could not publish {}: {error}", rlib.display()))
    })?;
    let _ = fs::remove_dir_all(&staging);
    publish(
        &entry.join("artifact.sha256"),
        format!("{digest}\n").as_bytes(),
    )?;
    publish(
        &entry.join("runtime.rs"),
        format!("{crate_prefix}{exported}").as_bytes(),
    )?;
    prune_cache(root)?;
    Ok(Some(Artifact {
        path: rlib,
        cache_hit: false,
        lock,
    }))
}

fn split_generated(generated: &str) -> Result<Option<SplitGenerated>, Error> {
    let Some(begin) = generated.find(BEGIN) else {
        return Ok(None);
    };
    if generated.matches(BEGIN).count() != 1 || generated.matches(END).count() != 1 {
        return Err(Error::Cache(
            "generated Rust has an invalid runtime marker pair".to_string(),
        ));
    }
    let runtime_start = begin + BEGIN.len();
    let relative_end = generated[runtime_start..].find(END).ok_or_else(|| {
        Error::Cache("generated Rust has an unterminated runtime block".to_string())
    })?;
    let runtime_end = runtime_start + relative_end;
    let after_runtime = runtime_end + END.len();
    let runtime = generated[runtime_start..runtime_end].to_string();
    let core_begin = generated.find(CORE_BEGIN);
    if core_begin.is_none() && generated.matches(CORE_END).count() != 0 {
        return Err(Error::Cache(
            "generated Rust has an invalid core marker pair".to_string(),
        ));
    }
    if generated.matches(CORE_BEGIN).count() > 1 || generated.matches(CORE_END).count() > 1 {
        return Err(Error::Cache(
            "generated Rust has an invalid core marker pair".to_string(),
        ));
    }
    let core = if let Some(core_begin) = core_begin {
        if core_begin < after_runtime {
            return Err(Error::Cache(
                "generated Rust has nested runtime/core markers".to_string(),
            ));
        }
        let core_start = core_begin + CORE_BEGIN.len();
        let core_end = core_start
            + generated[core_start..].find(CORE_END).ok_or_else(|| {
                Error::Cache("generated Rust has an unterminated core block".to_string())
            })?;
        Some((
            generated[core_start..core_end].to_string(),
            core_end + CORE_END.len(),
        ))
    } else {
        None
    };
    let mut program = String::with_capacity(generated.len() - runtime.len() + 96);
    program.push_str(&generated[..begin]);
    program.push_str("extern crate jet_runtime;\nuse jet_runtime::*;\n");
    match &core {
        Some((_, core_after)) => {
            let core_begin = core_begin.expect("core marker present");
            program.push_str("extern crate jet_runtime_core;\nuse jet_runtime_core::*;\n");
            program.push_str(&generated[after_runtime..core_begin]);
            program.push_str(&generated[*core_after..]);
        }
        None => program.push_str(&generated[after_runtime..]),
    }
    Ok(Some(SplitGenerated {
        runtime,
        core: core.map(|(source, _)| source),
        program,
    }))
}

fn cache_key(
    crate_name: &str,
    source: &str,
    exported_source: &str,
    crate_prefix: &str,
    dependency_key: Option<&str>,
    rustc: &OsStr,
    rustc_version: &str,
    rustc_flags: &[OsString],
    rustc_env: &[(OsString, OsString)],
) -> String {
    cache_key_with_schema(
        CACHE_SCHEMA,
        crate_name,
        source,
        exported_source,
        crate_prefix,
        dependency_key,
        rustc,
        rustc_version,
        rustc_flags,
        rustc_env,
    )
}

fn cache_key_with_schema(
    schema: &[u8],
    crate_name: &str,
    source: &str,
    exported_source: &str,
    crate_prefix: &str,
    dependency_key: Option<&str>,
    rustc: &OsStr,
    rustc_version: &str,
    rustc_flags: &[OsString],
    rustc_env: &[(OsString, OsString)],
) -> String {
    let mut data = Vec::new();
    push_bytes(&mut data, schema);
    push_bytes(&mut data, crate_name.as_bytes());
    push_bytes(&mut data, source.as_bytes());
    push_bytes(&mut data, crate_prefix.as_bytes());
    push_bytes(&mut data, exported_source.as_bytes());
    push_bytes(&mut data, dependency_key.unwrap_or_default().as_bytes());
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

fn rustc_identity(rustc: &OsStr, rustc_env: &[(OsString, OsString)]) -> Result<String, Error> {
    static IDENTITIES: OnceLock<Mutex<HashMap<Vec<u8>, String>>> = OnceLock::new();
    let mut identity_key = Vec::new();
    push_bytes(&mut identity_key, &os_bytes(rustc));
    push_bytes(&mut identity_key, &(rustc_env.len() as u64).to_be_bytes());
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
        return Err(Error::Cache(format!(
            "invalid cache path {}",
            path.display()
        )));
    };
    let name = path
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("artifact");
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
        fs::rename(&temporary, path)
            .map_err(|error| Error::Cache(format!("could not publish {}: {error}", path.display())))
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn directory_size(path: &Path) -> u64 {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    if metadata.file_type().is_file() {
        return metadata.len();
    }
    if !metadata.is_dir() {
        return 0;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| directory_size(&entry.path()))
        .fold(0, |total, size| total.saturating_add(size))
}

fn is_cache_entry(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| {
            name.len() == DIGEST_LEN
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
}

/// Oldest published artifact first. The digest record is written once after a
/// successful publish, so cache hits do not reorder victims or make pruning
/// depend on directory-lock churn.
fn entry_age(path: &Path) -> SystemTime {
    [
        "artifact.sha256",
        "libjet_runtime.rlib",
        "libjet_runtime_core.rlib",
    ]
        .iter()
        .filter_map(|name| fs::symlink_metadata(path.join(name)).ok())
        .filter_map(|metadata| metadata.modified().ok())
        .min()
        .unwrap_or(UNIX_EPOCH)
}

fn prune_cache(root: &Path) -> Result<(), Error> {
    let mut footprint = directory_size(root);
    if footprint <= RUNTIME_CACHE_LIMIT_BYTES {
        return Ok(());
    }
    let Ok(entries) = fs::read_dir(root) else {
        return Ok(());
    };
    let mut candidates = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false)
                && is_cache_entry(&entry.path())
        })
        .map(|entry| {
            let path = entry.path();
            let key = entry.file_name().to_string_lossy().into_owned();
            (entry_age(&path), key, path)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));

    for (_, _, entry) in candidates {
        if footprint <= RUNTIME_CACHE_LIMIT_BYTES {
            break;
        }
        // A live preparation owns this lock from cache lookup/store through
        // its user's rustc link. Never remove an entry that lock acquisition
        // says is in use.
        let Some(_lock) = BuildLock::try_acquire(&entry)? else {
            continue;
        };
        let size = directory_size(&entry);
        match fs::remove_dir_all(&entry) {
            Ok(()) => footprint = footprint.saturating_sub(size),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(Error::Cache(format!(
                    "could not evict runtime cache entry {}: {error}",
                    entry.display()
                )))
            }
        }
    }
    Ok(())
}

struct BuildLock {
    path: PathBuf,
}

impl BuildLock {
    fn try_acquire(entry: &Path) -> Result<Option<Self>, Error> {
        let path = entry.join(".build-lock");
        match fs::create_dir(&path) {
            Ok(()) => Ok(Some(Self { path })),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(Error::Cache(format!(
                "could not lock {}: {error}",
                entry.display()
            ))),
        }
    }

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
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    fs::create_dir_all(entry).map_err(|create_error| {
                        Error::Cache(format!(
                            "could not recreate {} after eviction: {create_error}",
                            entry.display()
                        ))
                    })?;
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
        let scope = scopes
            .last()
            .map(|(scope, _)| *scope)
            .unwrap_or(Scope::Other);
        let direct = scopes.last().is_some_and(|(_, level)| *level == depth);
        // `pending` is non-empty only while an item header is still open — a
        // multi-line generic parameter list or `where` clause. Those rows are
        // continuations, not items: `pub fn jet_fixed_list_concat<\n T: Clone,\n
        // const LEFT: usize,` would otherwise become `pub const LEFT: usize,`
        // inside the angle brackets and the runtime crate would not even parse.
        let rewritten = if direct && pending.is_empty() {
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
        Scope::Module => starts_exportable_item(code) || starts_restricted_reexport(code),
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

/// A `pub(crate) use …;` at module scope IS part of the runtime boundary: it is
/// how the Core prelude publishes a fragment it keeps inside a private module
/// (`mod jet_sync { … } pub(crate) use jet_sync::*;` — `core.sync` CRDTs, the
/// `core.db` row policy, `app.sync`; `mod jet_crypto_entropy` likewise). Left
/// alone, the split runtime rlib kept those names crate-private and the user
/// crate could not see a single one, so rustc rejected generated code that the
/// monolith accepts. A *bare* `use` is a private import, not a boundary, and
/// stays private so no `std` path leaks into the program's glob namespace.
fn starts_restricted_reexport(code: &str) -> bool {
    restricted_visibility_rest(code).is_some_and(|rest| rest.starts_with("use "))
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
    [
        "fn ", "struct ", "enum ", "union ", "trait ", "type ", "const ", "static ", "mod ",
    ]
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
    [
        "fn ",
        "const ",
        "type ",
        "unsafe fn ",
        "async fn ",
        "const fn ",
    ]
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
        && name
            .bytes()
            .all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
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
            RUNTIME_CRATE_NAME,
            runtime,
            &export_runtime_source(runtime),
            RUNTIME_CRATE_PREFIX,
            None,
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
                &[(
                    OsString::from("RUSTFLAGS"),
                    OsString::from("-Ctarget-cpu=native")
                )]
            )
        );
    }

    #[test]
    fn hostile_cache_invalidation_matrix() {
        let runtime = "fn x() {}";
        let exported = export_runtime_source(runtime);
        let base = cache_key_with_schema(
            CACHE_SCHEMA,
            RUNTIME_CRATE_NAME,
            runtime,
            &exported,
            RUNTIME_CRATE_PREFIX,
            None,
            OsStr::new("rustc"),
            "rustc 1",
            &[],
            &[],
        );
        let cases = [
            (
                "schema",
                cache_key_with_schema(
                    b"jet-runtime-rlib-v6",
                    RUNTIME_CRATE_NAME,
                    runtime,
                    &exported,
                    RUNTIME_CRATE_PREFIX,
                    None,
                    OsStr::new("rustc"),
                    "rustc 1",
                    &[],
                    &[],
                ),
            ),
            (
                "compiler path",
                cache_key_with_schema(
                    CACHE_SCHEMA,
                    RUNTIME_CRATE_NAME,
                    runtime,
                    &exported,
                    RUNTIME_CRATE_PREFIX,
                    None,
                    OsStr::new("other-rustc"),
                    "rustc 1",
                    &[],
                    &[],
                ),
            ),
            (
                "compiler version",
                cache_key_with_schema(
                    CACHE_SCHEMA,
                    RUNTIME_CRATE_NAME,
                    runtime,
                    &exported,
                    RUNTIME_CRATE_PREFIX,
                    None,
                    OsStr::new("rustc"),
                    "rustc 2",
                    &[],
                    &[],
                ),
            ),
            (
                "target",
                test_key(
                    runtime,
                    OsStr::new("rustc"),
                    "rustc 1",
                    &[OsString::from("--target"), OsString::from("wasm32")],
                    &[],
                ),
            ),
            (
                "profile",
                test_key(
                    runtime,
                    OsStr::new("rustc"),
                    "rustc 1",
                    &[OsString::from("-C"), OsString::from("opt-level=3")],
                    &[],
                ),
            ),
            (
                "backend",
                test_key(
                    runtime,
                    OsStr::new("rustc"),
                    "rustc 1",
                    &[OsString::from("-C"), OsString::from("target-cpu=native")],
                    &[],
                ),
            ),
            (
                "flags",
                test_key(
                    runtime,
                    OsStr::new("rustc"),
                    "rustc 1",
                    &[OsString::from("-C"), OsString::from("panic=abort")],
                    &[],
                ),
            ),
            (
                "environment flags",
                test_key(
                    runtime,
                    OsStr::new("rustc"),
                    "rustc 1",
                    &[],
                    &[(
                        OsString::from("RUSTFLAGS"),
                        OsString::from("-Ctarget-cpu=native"),
                    )],
                ),
            ),
            (
                "generated code",
                cache_key_with_schema(
                    CACHE_SCHEMA,
                    RUNTIME_CRATE_NAME,
                    runtime,
                    "pub fn generated_code_changed() {}",
                    RUNTIME_CRATE_PREFIX,
                    None,
                    OsStr::new("rustc"),
                    "rustc 1",
                    &[],
                    &[],
                ),
            ),
        ];
        for (input, key) in cases {
            assert_ne!(base, key, "{input} must invalidate runtime artifact");
        }
        let linker_flags = [
            OsString::from("-C"),
            OsString::from("linker=ld-hostile"),
            OsString::from("-C"),
            OsString::from("link-arg=-fuse-ld=lld"),
        ];
        assert_eq!(
            base,
            cache_key_with_schema(
                CACHE_SCHEMA,
                RUNTIME_CRATE_NAME,
                runtime,
                &exported,
                RUNTIME_CRATE_PREFIX,
                None,
                OsStr::new("rustc"),
                "rustc 1",
                &runtime_compile_flags(&linker_flags),
                &[],
            ),
            "linker-only inputs must not invalidate runtime work"
        );

        let runtime_with_extra_source = cache_key_with_schema(
            CACHE_SCHEMA,
            RUNTIME_CRATE_NAME,
            "fn runtime() {}fn core() {}",
            "pub fn runtime() {}pub fn core() {}",
            RUNTIME_CRATE_PREFIX,
            None,
            OsStr::new("rustc"),
            "rustc 1",
            &[],
            &[],
        );
        let changed_core = cache_key_with_schema(
            CACHE_SCHEMA,
            RUNTIME_CRATE_NAME,
            "fn runtime() {}fn core_changed() {}",
            "pub fn runtime() {}pub fn core_changed() {}",
            RUNTIME_CRATE_PREFIX,
            None,
            OsStr::new("rustc"),
            "rustc 1",
            &[],
            &[],
        );
        assert_ne!(
            runtime_with_extra_source, changed_core,
            "source changes must invalidate the runtime artifact"
        );
    }

    #[test]
    fn split_keeps_real_user_program_compile() {
        let generated = format!(
            "#![allow(warnings)]\n{BEGIN}fn runtime() {{}}\n{END}fn main() {{ runtime(); }}\n"
        );
        let split = split_generated(&generated).unwrap().unwrap();
        assert_eq!(split.runtime, "fn runtime() {}\n");
        assert!(split.core.is_none());
        assert!(split.program.contains("extern crate jet_runtime;"));
        assert!(split.program.contains("fn main() { runtime(); }"));
        assert!(!split.program.contains("fn runtime()"));
    }

    #[test]
    fn split_extracts_core_for_a_separate_runtime_closure() {
        let generated = format!(
            "#![allow(warnings)]\n{BEGIN}fn runtime() {{}}\n{END}{CORE_BEGIN}fn core() {{}}\n{CORE_END}fn main() {{ core(); }}\n"
        );
        let split = split_generated(&generated).unwrap().unwrap();
        assert_eq!(split.runtime, "fn runtime() {}\n");
        assert_eq!(split.core.as_deref(), Some("fn core() {}\n"));
        assert!(split.program.contains("extern crate jet_runtime;"));
        assert!(split.program.contains("extern crate jet_runtime_core;"));
        assert!(split.program.contains("use jet_runtime_core::*;"));
        assert!(split.program.contains("fn main() { core(); }"));
        assert!(!split.program.contains("fn runtime()"));
        assert!(!split.program.contains("fn core()"));
    }

    #[test]
    fn split_keeps_header_before_cached_runtime_import() {
        let generated = format!(
            "#![allow(warnings)]\nconst __JET_PACKAGE_EDITION: u16 = 2027;\nextern crate helper;\n{BEGIN}pub trait __jet_Display {{}}\n{END}use __jet_Display;\n"
        );
        let program = split_generated(&generated).unwrap().unwrap().program;
        let edition = program.find("__JET_PACKAGE_EDITION").unwrap();
        let helper = program.find("extern crate helper;").unwrap();
        let runtime = program.find("extern crate jet_runtime;").unwrap();
        let import = program.find("use jet_runtime::*;").unwrap();
        let tail = program.find("use __jet_Display;").unwrap();
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

    /// `Prelude/Core/FixedList.rs` declares `jet_fixed_list_concat` with its
    /// const-generic parameters on their own lines. Those rows start with
    /// `const ` at module depth, so the exporter used to write
    /// `pub const LEFT: usize,` between the angle brackets and rustc rejected
    /// the runtime crate before it read a single item.
    #[test]
    fn export_does_not_promote_multiline_generic_parameter_lists() {
        let source = r#"pub fn concat<
    T: Clone,
    const LEFT: usize,
    const RIGHT: usize,
>(
    left: &[T; LEFT],
) -> [T; LEFT] {
    left.clone()
}
"#;
        let exported = export_runtime_source(source);
        assert!(!exported.contains("pub const LEFT"));
        assert!(!exported.contains("pub const RIGHT"));
        assert!(exported.contains("    const LEFT: usize,"));
        assert!(exported.contains("pub fn concat<"));
    }

    /// A `where` clause on its own lines is the same continuation case, and its
    /// rows can start with `type `/`const ` too.
    #[test]
    fn export_does_not_promote_where_clause_rows() {
        let source = r#"pub fn run<T>(value: T) -> T
where
    T: Clone,
{
    value
}
"#;
        let exported = export_runtime_source(source);
        assert!(exported.contains("pub fn run<T>(value: T) -> T\nwhere\n    T: Clone,\n{"));
    }

    /// The `core.sync` prelude publishes its CRDT/row-policy fragment as
    /// `mod jet_sync { … }` plus one crate-root `pub(crate) use jet_sync::*;`.
    /// The exporter has to widen that re-export too, or the split runtime rlib
    /// hides every `jet_sync_*` / `jet_db_policy_*` / `jet_app_sync` name from
    /// the user crate and rustc rejects generated code the monolith accepts.
    /// A bare `use` stays private so no `std` path joins the program's globs.
    #[test]
    fn export_promotes_restricted_reexports_but_not_private_imports() {
        let source = r#"mod jet_sync {
use std::collections::HashMap;
pub(crate) fn jet_sync_text_new() -> i64 { 0 }
}
pub(crate) use jet_sync::*;
use std::fmt::Debug;
"#;
        let exported = export_runtime_source(source);
        assert!(exported.contains("pub mod jet_sync {"));
        assert!(exported.contains("pub fn jet_sync_text_new()"));
        assert!(exported.contains("pub use jet_sync::*;"));
        assert!(exported.contains("\nuse std::fmt::Debug;"));
        assert!(!exported.contains("pub use std::fmt::Debug;"));
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
        assert!(!first.cache_hit());
        drop(first);
        let second = prepare_at(&root.join("cache"), rustc.as_os_str(), &one, &[], &[]).unwrap();
        assert!(second.cache_hit());
        assert_eq!(fs::read(&count).unwrap(), b"x");
        let runtime_rlib = second.runtime_rlib.as_ref().unwrap().clone();
        drop(second);

        // The user program is not part of the reusable stdlib object. A
        // program edit with the same runtime must stay on the warm rlib.
        let program_changed =
            format!("{BEGIN}fn runtime() {{}}\n{END}fn main() {{ println!(\"changed\"); }}\n");
        let program_hit = prepare_at(
            &root.join("cache"),
            rustc.as_os_str(),
            &program_changed,
            &[],
            &[],
        )
        .unwrap();
        assert!(program_hit.cache_hit());
        assert_eq!(fs::read(&count).unwrap(), b"x");
        drop(program_hit);

        fs::write(runtime_rlib, b"corrupt!").unwrap();
        let repaired = prepare_at(&root.join("cache"), rustc.as_os_str(), &one, &[], &[]).unwrap();
        assert!(!repaired.cache_hit());
        assert_eq!(fs::read(&count).unwrap(), b"xx");

        let two = format!("{BEGIN}fn runtime_changed() {{}}\n{END}fn main() {{}}\n");
        drop(repaired);
        prepare_at(&root.join("cache"), rustc.as_os_str(), &two, &[], &[]).unwrap();
        assert_eq!(fs::read(&count).unwrap(), b"xxx");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn hostile_cache_invalidation_matrix_reuses_unaffected_separate_closures() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "jet-runtime-cache-linker-inputs-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(&root).unwrap();
        let rustc = root.join("rustc-fake");
        let count = root.join("count");
        fs::write(
            &rustc,
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"-vV\" ]; then echo 'rustc 1.99.0 fake'; exit 0; fi\nout=''\nwhile [ \"$#\" -gt 0 ]; do if [ \"$1\" = \"-o\" ]; then shift; out=\"$1\"; fi; shift; done\nprintf artifact > \"$out\"\nprintf x >> '{}'\n",
                count.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&rustc).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&rustc, permissions).unwrap();

        let generated = format!(
            "{BEGIN}fn runtime() {{}}\n{END}{CORE_BEGIN}fn core() {{}}\n{CORE_END}fn main() {{}}\n"
        );
        let first = prepare_at(
            &root.join("cache"),
            rustc.as_os_str(),
            &generated,
            &[],
            &[],
        )
        .unwrap();
        assert!(!first.cache_hit());
        drop(first);
        assert_eq!(
            fs::read(&count).unwrap().len(),
            2,
            "runtime and Core need separate cold builds"
        );

        let linker_changed = prepare_at(
            &root.join("cache"),
            rustc.as_os_str(),
            &generated,
            &[
                OsString::from("-C"),
                OsString::from("linker=other-linker"),
                OsString::from("-C"),
                OsString::from("link-arg=-fuse-ld=lld"),
            ],
            &[],
        )
        .unwrap();
        assert!(
            linker_changed.cache_hit(),
            "linker-only changes must not rebuild the runtime/Core closure"
        );
        drop(linker_changed);
        assert_eq!(
            fs::read(&count).unwrap().len(),
            2,
            "linker-only change must affect final link work only"
        );

        let program_changed = generated.replace("fn main() {}", "fn main() { println!(\"changed\"); }");
        let program_hit = prepare_at(
            &root.join("cache"),
            rustc.as_os_str(),
            &program_changed,
            &[],
            &[],
        )
        .unwrap();
        assert!(
            program_hit.cache_hit(),
            "program/generated-code changes must reuse the runtime/Core closure"
        );
        drop(program_hit);
        assert_eq!(fs::read(&count).unwrap().len(), 2);

        let core_changed = generated.replace("fn core() {}", "fn core_changed() {}");
        let core_miss = prepare_at(
            &root.join("cache"),
            rustc.as_os_str(),
            &core_changed,
            &[],
            &[],
        )
        .unwrap();
        assert!(!core_miss.cache_hit());
        drop(core_miss);
        assert_eq!(
            fs::read(&count).unwrap().len(),
            3,
            "Core-only change must rebuild only the Core closure"
        );

        let runtime_changed = generated.replace("fn runtime() {}", "fn runtime_changed() {}");
        let runtime_miss = prepare_at(
            &root.join("cache"),
            rustc.as_os_str(),
            &runtime_changed,
            &[],
            &[],
        )
        .unwrap();
        assert!(!runtime_miss.cache_hit());
        drop(runtime_miss);
        assert_eq!(
            fs::read(&count).unwrap().len(),
            5,
            "runtime change must rebuild runtime and its dependent Core closure"
        );

        let compile_changed = prepare_at(
            &root.join("cache"),
            rustc.as_os_str(),
            &generated,
            &[OsString::from("-C"), OsString::from("opt-level=3")],
            &[],
        )
        .unwrap();
        assert!(!compile_changed.cache_hit());
        drop(compile_changed);
        assert_eq!(
            fs::read(&count).unwrap().len(),
            7,
            "compile flag change must rebuild both closures"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn pruning_skips_entry_pinned_by_live_build() {
        let root = std::env::temp_dir().join(format!(
            "jet-runtime-cache-live-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let cache = root.join("cache");
        let entry = cache.join(format!("{:064x}", 1));
        fs::create_dir_all(&entry).unwrap();
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(entry.join("libjet_runtime.rlib"))
            .unwrap();
        file.set_len(RUNTIME_CACHE_LIMIT_BYTES + 1).unwrap();
        drop(file);
        let lock = BuildLock::acquire(&entry).unwrap();
        prune_cache(&cache).unwrap();
        assert!(entry.is_dir(), "live build entry must not be evicted");
        drop(lock);
        prune_cache(&cache).unwrap();
        assert!(
            !entry.exists(),
            "unlocked oversized entry should be evicted"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn bounded_write_evicts_old_entries_and_keeps_build_successful() {
        let root = std::env::temp_dir().join(format!(
            "jet-runtime-cache-bound-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let cache = root.join("cache");
        fs::create_dir_all(&cache).unwrap();
        for key in [format!("{:064x}", 1), format!("{:064x}", 2)] {
            let entry = cache.join(key);
            fs::create_dir_all(&entry).unwrap();
            let rlib = entry.join("libjet_runtime.rlib");
            let file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(rlib)
                .unwrap();
            file.set_len(RUNTIME_CACHE_LIMIT_BYTES / 2 + 1).unwrap();
            fs::write(entry.join("artifact.sha256"), b"seed\n").unwrap();
        }
        let before = directory_size(&cache);

        let generated = format!("{BEGIN}fn runtime() {{}}\n{END}fn main() {{}}\n");
        let prepared = prepare_at(&cache, OsStr::new("rustc"), &generated, &[], &[]).unwrap();
        let after = directory_size(&cache);
        assert!(
            prepared.is_split(),
            "bounded cache build must still prepare a split runtime"
        );
        assert!(after < before, "pruning must reduce cache footprint");
        assert!(
            after <= RUNTIME_CACHE_LIMIT_BYTES,
            "cache must stay under its bound"
        );
        assert!(prepared.runtime_rlib.as_ref().unwrap().is_file());

        let source = root.join("main.rs");
        let binary = root.join("main");
        fs::write(&source, prepared.rust()).unwrap();
        let mut command = Command::new("rustc");
        command
            .args(["--edition", "2021"])
            .arg(&source)
            .arg("-o")
            .arg(&binary);
        prepared.add_rustc_args(&mut command);
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "user build must succeed after eviction: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        drop(prepared);
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
        let prepared =
            prepare_at(&root.join("cache"), rustc.as_os_str(), &generated, &[], &[]).unwrap();
        assert_eq!(prepared.rust(), generated);
        assert!(!prepared.cache_hit());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn branches_core_closure_compiles_as_cached_rlib() {
        if Command::new("rustc").arg("-vV").output().is_err() {
            return;
        }
        let entry = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples/features/basics/branches.jet");
        let generated = crate::compile_with_path("", entry.to_str().unwrap())
            .expect("branches should reach codegen")
            .rust;
        let root = std::env::temp_dir().join(format!(
            "jet-runtime-cache-branches-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let prepared =
            prepare_at(&root.join("cache"), OsStr::new("rustc"), &generated, &[], &[])
                .expect("runtime preparation");
        assert!(prepared.is_split(), "Core-bearing builds must use cached rlibs");
        let source = root.join("main.rs");
        let binary = root.join("main");
        fs::write(&source, prepared.rust()).unwrap();
        let mut rustc = Command::new("rustc");
        rustc
            .args(["--edition", "2021"])
            .arg(&source)
            .arg("-o")
            .arg(&binary);
        prepared.add_rustc_args(&mut rustc);
        let output = rustc.output().unwrap();
        assert!(
            output.status.success(),
            "thin branches crate must compile:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        drop(prepared);
        let warm = prepare_at(
            &root.join("cache"),
            OsStr::new("rustc"),
            &generated,
            &[],
            &[],
        )
        .unwrap();
        assert!(warm.cache_hit(), "second preparation must reuse the closure rlib");
        drop(warm);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn debug_generated_runtime_cache_rejection() {
        let Some(source) = std::env::var_os("JET_RUNTIME_DEBUG_SOURCE") else {
            return;
        };
        let generated = fs::read_to_string(source).unwrap();
        let root = std::env::temp_dir().join(format!(
            "jet-runtime-cache-debug-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let prepared = prepare_at(&root.join("cache"), OsStr::new("rustc"), &generated, &[], &[])
            .unwrap();
        assert!(prepared.is_split());
        drop(prepared);
        let _ = fs::remove_dir_all(root);
    }
}
