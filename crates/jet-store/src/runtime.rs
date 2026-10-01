//! Content-addressed native runtime rlib cache.
//!
//! Codegen keeps emitting one complete Rust program for inspection and I1/I2
//! audits. Native builders extract its marked Prelude/runtime and Core blocks,
//! compile that mutually dependent closure once as ONE `jet_runtime` rlib, then
//! link the user program against it. The two blocks reference each other
//! (the fixed runtime reaches `jet_std`, the scheduler world, SIMD kernels and
//! `DataTree` in the Core block, and Core reaches the runtime traits), so they
//! cannot be two crates: Rust crates cannot depend on each other cyclically.
//! The Core block is selected only by build facts (edition, OS, test harness,
//! runtime parts, devtools policy), never by user source, so the key is the
//! (runtime source, rustc identity, flags) tuple and many programs share it.
//!
//! Packaging prebuilds the default build-fact set: the `jet-runtime/`
//! directory beside the `jet` executable holds `<key>/libjet_runtime.rlib`
//! under exactly this key, so a fresh machine with an empty store links the
//! shipped rlib instead of compiling the runtime. Other keys (another rustc,
//! profile, CPU, or feature set) miss it and build on demand into the store.

use crate::{ArtifactLookup, ArtifactRestore, Lease, LeaseTarget, Store, StoreError, StoredArtifact};
use jet_foundation::SHA256::sha256_hex;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

const CACHE_SCHEMA: &[u8] = b"jet-runtime-rlib-v9";
const RUNTIME_CRATE_NAME: &str = "jet_runtime";
const RUNTIME_RLIB_FILE: &str = "libjet_runtime.rlib";
const RUNTIME_CRATE_PREFIX: &str = "#![allow(warnings)]\n";
const BEGIN: &str = "// jet:cached-runtime-begin\n";
const END: &str = "// jet:cached-runtime-end\n";
const CORE_BEGIN: &str = "// jet:cached-core-begin\n";
const CORE_END: &str = "// jet:cached-core-end\n";
/// Directory beside the `jet` executable holding prebuilt runtime rlibs as
/// `<key>/libjet_runtime.rlib`.
pub const PREBUILT_DIR_NAME: &str = "jet-runtime";
/// When set, every runtime rlib a process links is also written to this
/// directory in the `PREBUILT_DIR_NAME` layout. The packaging step sets it
/// while building a seed program to populate the shipped prebuilt directory.
pub const PREBUILD_EXPORT_ENV: &str = "JET_RUNTIME_PREBUILD_DIR";
/// Separates the `rustc -vV` identity from the host CPU description when the
/// flags select `target-cpu=native`: the key must name the CPU the rlib was
/// tuned for, or a prebuilt rlib could reach a machine lacking its features.
const NATIVE_CPU_IDENTITY: &str = "jet:target-cpu=native\n";

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub enum RuntimeError {
    Tool(String),
    Cache(StoreError),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tool(message) => formatter.write_str(message),
            Self::Cache(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for RuntimeError {}

impl From<StoreError> for RuntimeError {
    fn from(error: StoreError) -> Self {
        Self::Cache(error)
    }
}

#[must_use = "keep the runtime lease alive until its consuming rustc command exits"]
pub struct PreparedRuntime {
    rust: String,
    runtime_rlib: Option<PathBuf>,
    cache_hit: bool,
    repaired: bool,
    prebuilt: bool,
    _lease: Option<Lease>,
    _materialized_root: Option<MaterializedRoot>,
}

struct MaterializedRoot(PathBuf);

impl MaterializedRoot {
    fn new(store: &Store) -> Result<Self, RuntimeError> {
        let path = store.root().join("staging").join(format!(
            "runtime-link-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).map_err(StoreError::Io)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for MaterializedRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

impl PreparedRuntime {
    pub fn inline(rust: &str) -> Self {
        Self {
            rust: rust.to_string(),
            runtime_rlib: None,
            cache_hit: false,
            repaired: false,
            prebuilt: false,
            _lease: None,
            _materialized_root: None,
        }
    }

    pub fn rust(&self) -> &str {
        &self.rust
    }

    pub fn cache_hit(&self) -> bool {
        self.cache_hit
    }

    pub fn repaired(&self) -> bool {
        self.repaired
    }

    /// True when the runtime rlib came from the prebuilt directory shipped
    /// beside the `jet` executable rather than from the store or rustc.
    pub fn prebuilt(&self) -> bool {
        self.prebuilt
    }

    /// True when the program crate links the cached runtime rlib instead of
    /// carrying the runtime source itself.
    pub fn is_split(&self) -> bool {
        self.runtime_rlib.is_some()
    }

    pub fn add_rustc_args(&self, command: &mut Command) {
        if let Some(rlib) = &self.runtime_rlib {
            command
                .arg("--extern")
                .arg(format!("{RUNTIME_CRATE_NAME}={}", rlib.display()));
        }
    }
}

/// Prepare the generated program for one native rustc invocation.
pub fn prepare(
    store: &Store,
    rustc: &OsStr,
    generated: &str,
    rustc_flags: &[OsString],
    rustc_env: &[(OsString, OsString)],
) -> Result<PreparedRuntime, RuntimeError> {
    let split = match split_generated(generated) {
        Ok(Some(split)) => split,
        Ok(None) | Err(_) => return Ok(PreparedRuntime::inline(generated)),
    };
    let compile_flags = runtime_compile_flags(rustc_flags);
    let rustc_version = rustc_identity(rustc, rustc_env, targets_native_cpu(&compile_flags))
        .map_err(RuntimeError::Tool)?;
    let exported_runtime = export_runtime_source(&split.runtime);
    let runtime_key = cache_key(
        &split.runtime,
        &exported_runtime,
        rustc,
        &rustc_version,
        &compile_flags,
        rustc_env,
    );
    let prepared = if let Some(rlib) = prebuilt_runtime_rlib(&runtime_key) {
        PreparedRuntime {
            rust: split.program,
            runtime_rlib: Some(rlib),
            cache_hit: true,
            repaired: false,
            prebuilt: true,
            _lease: None,
            _materialized_root: None,
        }
    } else {
        let materialized_root = MaterializedRoot::new(store)?;
        let Some(runtime) = compile_artifact(
            store,
            &runtime_key,
            &exported_runtime,
            rustc,
            &compile_flags,
            rustc_env,
            materialized_root.path(),
        )?
        else {
            return Ok(PreparedRuntime::inline(generated));
        };
        PreparedRuntime {
            rust: split.program,
            runtime_rlib: Some(runtime.artifact.path),
            cache_hit: runtime.cache_hit,
            repaired: runtime.repaired,
            prebuilt: false,
            _lease: Some(runtime.lease),
            _materialized_root: Some(materialized_root),
        }
    };
    if let Some(rlib) = &prepared.runtime_rlib {
        export_prebuilt_runtime(&runtime_key, rlib)?;
    }
    Ok(prepared)
}

/// The shipped rlib for `key`: `<exe dir>/jet-runtime/<key>/libjet_runtime.rlib`.
/// The key covers the runtime source, rustc identity (host CPU included for
/// `target-cpu=native`), flags and environment, so a present file is exactly
/// the artifact rustc would produce here.
fn prebuilt_runtime_rlib(key: &str) -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let path = executable
        .parent()?
        .join(PREBUILT_DIR_NAME)
        .join(key)
        .join(RUNTIME_RLIB_FILE);
    path.is_file().then_some(path)
}

/// Packaging hook (`JET_RUNTIME_PREBUILD_DIR`): copy the linked rlib into the
/// prebuilt layout under `key`, atomically, unless that entry already exists.
fn export_prebuilt_runtime(key: &str, rlib: &Path) -> Result<(), RuntimeError> {
    let Some(dir) = std::env::var_os(PREBUILD_EXPORT_ENV).filter(|dir| !dir.is_empty()) else {
        return Ok(());
    };
    let entry = PathBuf::from(dir).join(key);
    let target = entry.join(RUNTIME_RLIB_FILE);
    if target.is_file() {
        return Ok(());
    }
    fs::create_dir_all(&entry).map_err(StoreError::Io)?;
    let staged = entry.join(format!(
        ".{RUNTIME_RLIB_FILE}.{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let copied = fs::copy(rlib, &staged).and_then(|_| fs::rename(&staged, &target));
    if let Err(error) = copied {
        let _ = fs::remove_file(&staged);
        return Err(RuntimeError::Cache(StoreError::Io(error)));
    }
    Ok(())
}

/// Whether the runtime compile flags tune code for the host CPU.
fn targets_native_cpu(flags: &[OsString]) -> bool {
    flags.iter().any(|flag| {
        let flag = flag.as_os_str();
        flag == OsStr::new("target-cpu=native") || flag == OsStr::new("-Ctarget-cpu=native")
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

struct CompiledArtifact {
    artifact: StoredArtifact,
    cache_hit: bool,
    repaired: bool,
    lease: Lease,
}

fn compile_artifact(
    store: &Store,
    key: &str,
    exported: &str,
    rustc: &OsStr,
    rustc_flags: &[OsString],
    rustc_env: &[(OsString, OsString)],
    materialized_root: &Path,
) -> Result<Option<CompiledArtifact>, RuntimeError> {
    let (repaired, artifact) = match store.lookup_artifact(key)? {
        ArtifactLookup::Hit(artifact) => (false, Some(artifact)),
        ArtifactLookup::Missing => (false, None),
        ArtifactLookup::Corrupt => (true, None),
    };
    if let Some(mut artifact) = artifact {
        let path = materialize_artifact(store, key, materialized_root)?;
        artifact.path = path;
        let lease = store.acquire_artifact_lease(&[
            LeaseTarget::Action(artifact.action),
            LeaseTarget::Blob(artifact.object),
        ])?;
        return Ok(Some(CompiledArtifact {
            artifact,
            cache_hit: true,
            repaired,
            lease,
        }));
    }

    let staging = store.root().join("staging").join(format!(
        "{RUNTIME_CRATE_NAME}-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&staging).map_err(|error| {
        RuntimeError::Cache(StoreError::Io(error))
    })?;
    let source = staging.join("runtime.rs");
    let staged_rlib = staging.join(RUNTIME_RLIB_FILE);
    fs::write(&source, format!("{RUNTIME_CRATE_PREFIX}{exported}")).map_err(|error| {
        let _ = fs::remove_dir_all(&staging);
        RuntimeError::Cache(StoreError::Io(error))
    })?;

    let mut command = Command::new(rustc);
    command
        .args([
            "--edition",
            "2021",
            "--crate-name",
            RUNTIME_CRATE_NAME,
            "--crate-type",
            "rlib",
        ])
        .args(rustc_flags)
        .arg("-C")
        .arg(format!("metadata={key}"));
    if let Ok(prefix) = fs::canonicalize(&staging) {
        command
            .arg("--remap-path-prefix")
            .arg(format!("{}=/jet/build", prefix.display()));
    }
    command.arg(&source).arg("-o").arg(&staged_rlib);
    for (name, value) in rustc_env {
        command.env(name, value);
    }
    let output = command.output().map_err(|error| {
        let _ = fs::remove_dir_all(&staging);
        RuntimeError::Tool(format!("could not run rustc for cached runtime: {error}"))
    })?;
    let _ = fs::remove_file(&source);
    if !output.status.success() {
        let _ = fs::remove_dir_all(&staging);
        return Ok(None);
    }
    match store.publish_file(key, &staged_rlib) {
        Ok(_) => {}
        Err(StoreError::Conflict { .. }) => {}
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(RuntimeError::Cache(error));
        }
    }
    let _ = fs::remove_dir_all(&staging);
    let mut artifact = match store.lookup_artifact(key)? {
        ArtifactLookup::Hit(artifact) => artifact,
        ArtifactLookup::Missing | ArtifactLookup::Corrupt => return Ok(None),
    };
    artifact.path = materialize_artifact(store, key, materialized_root)?;
    let lease = store.acquire_artifact_lease(&[
        LeaseTarget::Action(artifact.action),
        LeaseTarget::Blob(artifact.object),
    ])?;
    Ok(Some(CompiledArtifact {
        artifact,
        cache_hit: false,
        repaired,
        lease,
    }))
}
fn materialize_artifact(
    store: &Store,
    key: &str,
    root: &Path,
) -> Result<PathBuf, RuntimeError> {
    let path = root.join(RUNTIME_RLIB_FILE);
    match store.restore_file(key, &path)? {
        ArtifactRestore::Hit { .. } => Ok(path),
        ArtifactRestore::Missing | ArtifactRestore::Corrupt => Err(RuntimeError::Cache(
            StoreError::Corrupt {
                path,
                reason: "published runtime artifact disappeared before linking".to_string(),
            },
        )),
    }
}


struct SplitGenerated {
    /// The runtime block followed by the Core block: one crate, because the
    /// two blocks reference each other.
    runtime: String,
    program: String,
}
fn split_generated(generated: &str) -> Result<Option<SplitGenerated>, String> {
    let Some(begin) = generated.find(BEGIN) else {
        return Ok(None);
    };
    if generated.matches(BEGIN).count() != 1 || generated.matches(END).count() != 1 {
        return Err("generated Rust has an invalid runtime marker pair".to_string());
    }
    let runtime_start = begin + BEGIN.len();
    let relative_end = generated[runtime_start..]
        .find(END)
        .ok_or_else(|| "generated Rust has an unterminated runtime block".to_string())?;
    let runtime_end = runtime_start + relative_end;
    let after_runtime = runtime_end + END.len();
    let mut runtime = generated[runtime_start..runtime_end].to_string();
    let core_begin = generated.find(CORE_BEGIN);
    if core_begin.is_none() && generated.matches(CORE_END).count() != 0 {
        return Err("generated Rust has an invalid core marker pair".to_string());
    }
    if generated.matches(CORE_BEGIN).count() > 1 || generated.matches(CORE_END).count() > 1 {
        return Err("generated Rust has an invalid core marker pair".to_string());
    }
    let mut program = String::with_capacity(generated.len() - runtime.len() + 48);
    program.push_str(&generated[..begin]);
    program.push_str("extern crate jet_runtime;\nuse jet_runtime::*;\n");
    if let Some(core_begin) = core_begin {
        if core_begin < after_runtime {
            return Err("generated Rust has nested runtime/core markers".to_string());
        }
        let core_start = core_begin + CORE_BEGIN.len();
        let core_end = core_start
            + generated[core_start..]
                .find(CORE_END)
                .ok_or_else(|| "generated Rust has an unterminated core block".to_string())?;
        runtime.push_str(&generated[core_start..core_end]);
        program.push_str(&generated[after_runtime..core_begin]);
        program.push_str(&generated[core_end + CORE_END.len()..]);
    } else {
        program.push_str(&generated[after_runtime..]);
    }
    Ok(Some(SplitGenerated { runtime, program }))
}

fn cache_key(
    source: &str,
    exported_source: &str,
    rustc: &OsStr,
    rustc_version: &str,
    rustc_flags: &[OsString],
    rustc_env: &[(OsString, OsString)],
) -> String {
    let mut data = Vec::new();
    push_bytes(&mut data, CACHE_SCHEMA);
    push_bytes(&mut data, RUNTIME_CRATE_NAME.as_bytes());
    push_bytes(&mut data, source.as_bytes());
    push_bytes(&mut data, RUNTIME_CRATE_PREFIX.as_bytes());
    push_bytes(&mut data, exported_source.as_bytes());
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

/// The `rustc -vV` identity of `rustc` under `rustc_env`, followed, when the
/// flags tune for the host CPU, by `NATIVE_CPU_IDENTITY` and the
/// `rustc --print cfg -C target-cpu=native` description of this CPU.
fn rustc_identity(
    rustc: &OsStr,
    rustc_env: &[(OsString, OsString)],
    native_cpu: bool,
) -> Result<String, String> {
    static IDENTITIES: LazyLock<Mutex<HashMap<Vec<u8>, String>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
    let mut identity_key = Vec::new();
    push_bytes(&mut identity_key, &os_bytes(rustc));
    push_bytes(&mut identity_key, &(rustc_env.len() as u64).to_be_bytes());
    for (name, value) in rustc_env {
        push_bytes(&mut identity_key, &os_bytes(name));
        push_bytes(&mut identity_key, &os_bytes(value));
    }
    identity_key.push(u8::from(native_cpu));
    let identities = &*IDENTITIES;
    if let Some(identity) = identities.lock().unwrap().get(&identity_key).cloned() {
        return Ok(identity);
    }
    // Both queries run concurrently: the CPU description costs no wall time.
    let cpu_query = native_cpu
        .then(|| spawn_rustc_query(rustc, rustc_env, &["--print", "cfg", "-C", "target-cpu=native"]));
    let mut identity = finish_rustc_query(spawn_rustc_query(rustc, rustc_env, &["-vV"]))?;
    if let Some(cpu_query) = cpu_query {
        identity.push_str(NATIVE_CPU_IDENTITY);
        identity.push_str(&finish_rustc_query(cpu_query)?);
    }
    identities
        .lock()
        .unwrap()
        .insert(identity_key, identity.clone());
    Ok(identity)
}

struct RustcQuery {
    rendered: String,
    child: std::io::Result<std::process::Child>,
}

fn spawn_rustc_query(rustc: &OsStr, rustc_env: &[(OsString, OsString)], args: &[&str]) -> RustcQuery {
    let mut command = Command::new(rustc);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in rustc_env {
        command.env(name, value);
    }
    RustcQuery {
        rendered: args.join(" "),
        child: command.spawn(),
    }
}

fn finish_rustc_query(query: RustcQuery) -> Result<String, String> {
    let rendered = query.rendered;
    let output = query
        .child
        .and_then(std::process::Child::wait_with_output)
        .map_err(|error| format!("could not run rustc {rendered}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "rustc {rendered} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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
    /// A `macro_rules!` body: the items it generates when the runtime invokes
    /// it are part of the runtime boundary (`export_macro_line`).
    Macro,
    /// A module-level `thread_local! { … }` block: its statics are items.
    ThreadLocal,
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
    let mut macro_exported = false;

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
        let macro_level = scopes
            .iter()
            .find(|(kind, _)| *kind == Scope::Macro)
            .map(|(_, level)| *level);
        let code = masked.trim();
        let rewritten = if let Some(level) = macro_level {
            export_macro_line(line, masked, depth == level + 1)
        } else if direct && pending.is_empty() {
            export_line(line, masked, scope)
        } else {
            line.to_string()
        };
        // A root `macro_rules!` is textually scoped to the crate that defines
        // it; the program crate invokes some of them (`jet_history_callable!`),
        // so the split runtime exports every root macro by path.
        if scopes.len() == 1
            && direct
            && pending.is_empty()
            && code.starts_with("macro_rules!")
            && !macro_exported
        {
            let indent = line.len() - line.trim_start().len();
            out.push_str(&line[..indent]);
            out.push_str("#[macro_export]\n");
        }
        out.push_str(&rewritten);
        if !code.is_empty() {
            macro_exported = code == "#[macro_export]"
                || (macro_exported && code.starts_with('#'));
        }
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
        Scope::ThreadLocal => strip_visibility(code).starts_with("static "),
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

/// Inside a `macro_rules!` body an explicit restricted visibility widens at
/// any depth, and an item written directly in a transcriber (`trait $name`,
/// `fn $name`) becomes `pub`: the runtime invokes these macros to generate
/// items the program crate names. Deeper lines (impl members, statements) and
/// the arm structure itself are not item syntax and stay as written.
fn export_macro_line(line: &str, masked: &str, transcriber_item: bool) -> String {
    let indent = line.len() - line.trim_start().len();
    let code = masked.trim_start();
    if let Some(rest) = restricted_visibility_rest(&line[indent..])
        .filter(|_| restricted_visibility_rest(code).is_some())
    {
        return format!("{}pub {}", &line[..indent], rest);
    }
    if transcriber_item && !code.starts_with("pub") && starts_exportable_item(code) {
        return format!("{}pub {}", &line[..indent], &line[indent..]);
    }
    line.to_string()
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
                || code.starts_with("macro_rules!")
                || code.starts_with("thread_local!")
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
    } else if header.starts_with("macro_rules!") {
        Scope::Macro
    } else if header.starts_with("thread_local!") {
        Scope::ThreadLocal
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

    fn emitted_target_dossier_source(layer: &str, provider: &str, closure: &str) -> String {
        format!(
            "{BEGIN}// jet:target-dossier layer={layer} provider={provider} closure={closure} artifact=fixture\nfn runtime_entry() {{}}\n{END}"
        )
    }

    fn key_for_source(source: &str) -> String {
        cache_key(
            source,
            "fixed-exported-runtime",
            OsStr::new("rustc"),
            "rustc-test",
            &[],
            &[],
        )
    }

    #[test]
    fn target_dossier_source_invalidates_runtime_key() {
        let provider = "target-providers-v1:sha256:provider-a";
        let closure = "prelude-hosted-v1:sha256:closure-a";
        let base = emitted_target_dossier_source("hosted", provider, closure);
        let variants = [
            (
                "layer",
                emitted_target_dossier_source("core", provider, closure),
            ),
            (
                "provider digest/identity",
                emitted_target_dossier_source(
                    "hosted",
                    "target-providers-v1:sha256:provider-b",
                    closure,
                ),
            ),
            (
                "Prelude closure",
                emitted_target_dossier_source(
                    "hosted",
                    provider,
                    "prelude-hosted-v1:sha256:closure-b",
                ),
            ),
        ];
        let runtime_base = key_for_source(&base);
        assert_eq!(runtime_base, key_for_source(&base));
        for (field, changed) in variants {
            assert_ne!(
                runtime_base,
                key_for_source(&changed),
                "{field} source identity must invalidate the runtime rlib key"
            );
        }
    }

    /// The fixed runtime and the Core block reference each other, so the split
    /// must put both into the one `jet_runtime` crate and leave the program
    /// only user code plus that single dependency.
    #[test]
    fn split_puts_runtime_and_core_into_one_crate() {
        let generated = format!(
            "#![allow(warnings)]\n{BEGIN}fn runtime_item() {{ core_item() }}\n{END}\
             {CORE_BEGIN}fn core_item() {{}}\n{CORE_END}fn main() {{ runtime_item() }}\n"
        );
        let split = split_generated(&generated)
            .expect("valid markers")
            .expect("marked program splits");
        assert_eq!(
            split.runtime,
            "fn runtime_item() { core_item() }\nfn core_item() {}\n"
        );
        assert_eq!(
            split.program,
            "#![allow(warnings)]\nextern crate jet_runtime;\nuse jet_runtime::*;\nfn main() { runtime_item() }\n"
        );
    }

    /// The program crate names items a runtime `macro_rules!` generates (the
    /// fixed-width `jet_u64_trap_shl` kernels, `trait JetHistoryFn1`) and
    /// invokes root macros itself (`jet_history_callable!`), and reads
    /// thread-local runtime state. Trait impl members stay untouched.
    #[test]
    fn export_opens_macro_and_thread_local_runtime_items() {
        let source = "macro_rules! kernels {\n    ($name:ident, $t:ty) => {\n        pub(crate) fn $name(value: $t) -> $t { value }\n        trait Callable {}\n        impl Marker for $t {\n            fn mark(&self) {}\n        }\n    };\n}\n#[macro_export]\nmacro_rules! exported {\n    () => {};\n}\nkernels!(jet_u64_trap_shl, u64);\nthread_local! {\n    static STATE: u8 = const { 0 };\n}\n";
        assert_eq!(
            export_runtime_source(source),
            "#[macro_export]\nmacro_rules! kernels {\n    ($name:ident, $t:ty) => {\n        pub fn $name(value: $t) -> $t { value }\n        pub trait Callable {}\n        impl Marker for $t {\n            fn mark(&self) {}\n        }\n    };\n}\n#[macro_export]\nmacro_rules! exported {\n    () => {};\n}\nkernels!(jet_u64_trap_shl, u64);\nthread_local! {\n    pub static STATE: u8 = const { 0 };\n}\n"
        );
    }
}
