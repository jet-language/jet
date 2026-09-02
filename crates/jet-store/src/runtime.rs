//! Content-addressed native runtime rlib cache.
//!
//! Codegen keeps emitting one complete Rust program for inspection and I1/I2
//! audits. Native builders extract its marked Prelude/runtime and Core blocks,
//! compile that mutually dependent closure once, then link the user program.

use crate::{ArtifactLookup, ArtifactRestore, Lease, LeaseTarget, Store, StoreError, StoredArtifact};
use jet_foundation::SHA256::sha256_hex;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

const CACHE_SCHEMA: &[u8] = b"jet-runtime-core-rlib-v8";
const RUNTIME_CRATE_NAME: &str = "jet_runtime";
const CORE_CRATE_NAME: &str = "jet_runtime_core";
const RUNTIME_CRATE_PREFIX: &str = "#![allow(warnings)]\n";
const CORE_CRATE_PREFIX: &str =
    "#![allow(warnings)]\nextern crate jet_runtime;\nuse jet_runtime::*;\n";
const BEGIN: &str = "// jet:cached-runtime-begin\n";
const END: &str = "// jet:cached-runtime-end\n";
const CORE_BEGIN: &str = "// jet:cached-core-begin\n";
const CORE_END: &str = "// jet:cached-core-end\n";

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
    runtime: Option<StoredArtifact>,
    core: Option<StoredArtifact>,
    cache_hit: bool,
    repaired: bool,
    _leases: Vec<Lease>,
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
            runtime: None,
            core: None,
            cache_hit: false,
            repaired: false,
            _leases: Vec::new(),
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

    /// True when the program crate links the cached runtime rlib instead of
    /// carrying the runtime source itself.
    pub fn is_split(&self) -> bool {
        self.runtime.is_some()
    }

    pub fn add_rustc_args(&self, command: &mut Command) {
        if let Some(runtime) = &self.runtime {
            command
                .arg("--extern")
                .arg(format!("{RUNTIME_CRATE_NAME}={}", runtime.path.display()));
        }
        if let Some(core) = &self.core {
            command
                .arg("--extern")
                .arg(format!("{CORE_CRATE_NAME}={}", core.path.display()));
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
    let materialized_root = MaterializedRoot::new(store)?;
    let rustc_version =
        rustc_identity(rustc, rustc_env).map_err(|error| RuntimeError::Tool(error.to_string()))?;
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
        store,
        &runtime_key,
        RUNTIME_CRATE_NAME,
        &exported_runtime,
        RUNTIME_CRATE_PREFIX,
        None,
        rustc,
        &compile_flags,
        rustc_env,
        materialized_root.path(),
    )?
    else {
        return Ok(PreparedRuntime::inline(generated));
    };
    let runtime_path = runtime.artifact.path.clone();
    let mut leases = vec![runtime.lease];
    let mut repaired = runtime.repaired;
    let (core, core_hit) = if let Some(core_source) = split.core {
        let exported_core = export_runtime_source(&core_source);
        let core_key = cache_key(
            CORE_CRATE_NAME,
            &core_source,
            &exported_core,
            CORE_CRATE_PREFIX,
            Some(&runtime_key),
            rustc,
            &rustc_version,
            &compile_flags,
            rustc_env,
        );
        let Some(core) = compile_artifact(
            store,
            &core_key,
            CORE_CRATE_NAME,
            &exported_core,
            CORE_CRATE_PREFIX,
            Some((RUNTIME_CRATE_NAME, &runtime_path)),
            rustc,
            &compile_flags,
            rustc_env,
            materialized_root.path(),
        )?
        else {
            return Ok(PreparedRuntime::inline(generated));
        };
        repaired |= core.repaired;
        leases.push(core.lease);
        (Some(core.artifact), core.cache_hit)
    } else {
        (None, true)
    };
    Ok(PreparedRuntime {
        rust: split.program,
        runtime: Some(runtime.artifact),
        core,
        cache_hit: runtime.cache_hit && core_hit,
        repaired,
        _leases: leases,
        _materialized_root: Some(materialized_root),
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
    crate_name: &str,
    exported: &str,
    crate_prefix: &str,
    dependency: Option<(&str, &Path)>,
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
        let path = materialize_artifact(store, key, crate_name, materialized_root)?;
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
        "{}-{}-{}",
        crate_name,
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&staging).map_err(|error| {
        RuntimeError::Cache(StoreError::Io(error))
    })?;
    let source = staging.join("runtime.rs");
    let staged_rlib = staging.join(format!("lib{crate_name}.rlib"));
    fs::write(&source, format!("{crate_prefix}{exported}")).map_err(|error| {
        let _ = fs::remove_dir_all(&staging);
        RuntimeError::Cache(StoreError::Io(error))
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
        .args(rustc_flags)
        .arg("-C")
        .arg(format!("metadata={key}"));
    if let Ok(prefix) = fs::canonicalize(&staging) {
        command
            .arg("--remap-path-prefix")
            .arg(format!("{}=/jet/build", prefix.display()));
    }
    if let Some((dependency_name, dependency_path)) = dependency {
        command
            .arg("--extern")
            .arg(format!("{dependency_name}={}", dependency_path.display()));
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
    artifact.path = materialize_artifact(store, key, crate_name, materialized_root)?;
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
    crate_name: &str,
    root: &Path,
) -> Result<PathBuf, RuntimeError> {
    let path = root.join(format!("lib{crate_name}.rlib"));
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
    runtime: String,
    core: Option<String>,
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
    let runtime = generated[runtime_start..runtime_end].to_string();
    let core_begin = generated.find(CORE_BEGIN);
    if core_begin.is_none() && generated.matches(CORE_END).count() != 0 {
        return Err("generated Rust has an invalid core marker pair".to_string());
    }
    if generated.matches(CORE_BEGIN).count() > 1 || generated.matches(CORE_END).count() > 1 {
        return Err("generated Rust has an invalid core marker pair".to_string());
    }
    let core = if let Some(core_begin) = core_begin {
        if core_begin < after_runtime {
            return Err("generated Rust has nested runtime/core markers".to_string());
        }
        let core_start = core_begin + CORE_BEGIN.len();
        let core_end = core_start
            + generated[core_start..]
                .find(CORE_END)
                .ok_or_else(|| "generated Rust has an unterminated core block".to_string())?;
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

fn rustc_identity(rustc: &OsStr, rustc_env: &[(OsString, OsString)]) -> Result<String, String> {
    static IDENTITIES: LazyLock<Mutex<HashMap<Vec<u8>, String>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
    let mut identity_key = Vec::new();
    push_bytes(&mut identity_key, &os_bytes(rustc));
    push_bytes(&mut identity_key, &(rustc_env.len() as u64).to_be_bytes());
    for (name, value) in rustc_env {
        push_bytes(&mut identity_key, &os_bytes(name));
        push_bytes(&mut identity_key, &os_bytes(value));
    }
    let identities = &*IDENTITIES;
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
        .map_err(|error| format!("could not run rustc -vV: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "rustc -vV failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let identity = String::from_utf8_lossy(&output.stdout).into_owned();
    identities
        .lock()
        .unwrap()
        .insert(identity_key, identity.clone());
    Ok(identity)
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

