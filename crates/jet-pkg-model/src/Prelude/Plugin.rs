// core.plugin runtime (D-DEP-WASM1=A, c81) — sandboxed WASM Component Model
// plugin loader, via wasmtime's dynamic component API.
//
// This file is emitted verbatim into the hidden FFI bridge crate (see
// Source/FFI.rs) when a Jet program uses `core.plugin`. The compiler crate
// (`Source/`) never depends on `wasmtime`; it only ships this text. Owner-
// approved I6 bootstrap exception (D-DEP-WASM1=A): wasmtime + the Component
// Model is the plugin sandbox engine, runtime-side only.
//
// Safety model (D-PLUGIN1=B): the linker registers *zero* host imports, so a
// plugin component that declares any import fails to instantiate — a plugin
// can only export pure computation over Int/Float/Bool/Text, never reach the
// host filesystem, network, clock, or process. This is the whole safety
// boundary; there is no `#Unsafe` gate anywhere in this file or the generated
// call sites (I1).
//
// Handles are u64 keys into a thread-local HashMap, mirroring `DB.rs`. Handle
// 0 is the error sentinel (never a live plugin instance).
//
// Wire protocol (mirrors `DB.rs`'s tagged-length encoding — byte-exact,
// nothing to escape): a scalar value is `I<len>:<int>` (Jet `Int`),
// `F<len>:<float>` (Jet `Float`), `B<len>:<true|false>` (Jet `Bool`), or
// `T<len>:<utf8>` (Jet `Text`). A
// call result or error is `O:<value>` / `E:<message>`. Helper names are
// prefixed `plugin_` so they never collide with `DB.rs`'s identically-shaped
// `encode_tagged`/`read_tagged` helpers when both runtimes are concatenated
// into the same bridge crate.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Component as PathComponent, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use wasmtime::component::{Component, Linker, Type, Val};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder, Trap};

const PLUGIN_MAX_FUEL: u64 = 10_000_000;
const PLUGIN_MAX_MEMORY_BYTES: usize = 16 * 1024 * 1024;
const PLUGIN_MAX_TABLE_ELEMENTS: u32 = 10_000;
const PLUGIN_TIMEOUT_MS: u64 = 2_000;
const PLUGIN_MAX_WIRE_BYTES: usize = 16 * 1024 * 1024;
const PLUGIN_MAX_PARAMS: usize = 1024;

struct PluginHostState {
    limits: StoreLimits,
}

struct PluginInstance {
    engine: Engine,
    store: Store<PluginHostState>,
    instance: wasmtime::component::Instance,
    authority: String,
}

thread_local! {
    static PLUGINS: RefCell<HashMap<u64, PluginInstance>> = RefCell::new(HashMap::new());
}

static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

fn plugin_engine() -> Result<Engine, String> {
    let mut config = Config::new();
    config.consume_fuel(true);
    config.epoch_interruption(true);
    Engine::new(&config).map_err(|error| format!("plugin engine: {error}"))
}

fn plugin_limits() -> StoreLimits {
    StoreLimitsBuilder::new()
        .memory_size(PLUGIN_MAX_MEMORY_BYTES)
        .table_elements(PLUGIN_MAX_TABLE_ELEMENTS as _)
        .instances(1)
        .memories(1)
        .tables(1)
        .build()
}

fn plugin_relative_candidate(
    path: &Path,
    root_text: &str,
) -> Result<Option<PathBuf>, String> {
    let explicit_root = root_text != "repo";
    if path.is_absolute() {
        if !explicit_root {
            return Ok(None);
        }
        let root = Path::new(root_text);
        if !root.is_absolute() {
            return Err(
                "an absolute plugin path requires an absolute explicit FS.Read root".to_string(),
            );
        }
        let Some(relative) = path.strip_prefix(root).ok() else {
            return Ok(None);
        };
        if relative.as_os_str().is_empty() {
            return Err("plugin path names the FS.Read root, not a file".to_string());
        }
        if relative.components().any(|component| {
            matches!(
                component,
                PathComponent::ParentDir | PathComponent::RootDir | PathComponent::Prefix(_)
            )
        }) {
            return Err("plugin path escapes its explicit FS.Read root".to_string());
        }
        return Ok(Some(relative.to_path_buf()));
    }
    if path.components().any(|component| {
        matches!(
            component,
            PathComponent::ParentDir | PathComponent::RootDir | PathComponent::Prefix(_)
        )
    }) {
        return Err("plugin path must be relative and must not contain `..`".to_string());
    }
    Ok(Some(path.to_path_buf()))
}

fn plugin_read_module(path: &str, authority: &str) -> Result<Vec<u8>, String> {
    let path = Path::new(path);
    if authority.is_empty() {
        return jet_foundation::SHA256::read_file_nofollow(
            path,
            jet_foundation::SHA256::MAX_TREE_FILE_BYTES,
        )
        .map_err(|error| format!("couldn't read plugin `{}`: {error}", path.display()));
    }

    let mut saw_read_grant = false;
    let mut matched_root = false;
    let mut last_error = None;
    for right in authority.split('\n') {
        let Some(root_text) = right.strip_prefix("FS.Read:") else {
            continue;
        };
        saw_read_grant = true;
        if root_text.is_empty() {
            return Err("plugin FS.Read authority names an empty root".to_string());
        }
        let root = if root_text == "repo" {
            Path::new(".")
        } else {
            Path::new(root_text)
        };
        let Some(relative) = plugin_relative_candidate(path, root_text)? else {
            continue;
        };
        matched_root = true;
        match jet_foundation::SHA256::read_file_nofollow_at_root(
            root,
            &relative,
            jet_foundation::SHA256::MAX_TREE_FILE_BYTES,
        ) {
            Ok(bytes) => return Ok(bytes),
            Err(error) => {
                last_error = Some(format!(
                    "couldn't read plugin `{}`: {error}",
                    path.display()
                ));
            }
        }
    }
    if !saw_read_grant {
        return Err("plugin load requires an FS.Read authority".to_string());
    }
    if !matched_root {
        return Err(format!(
            "plugin path `{}` is outside the granted FS.Read roots",
            path.display()
        ));
    }
    Err(last_error.unwrap_or_else(|| {
        format!(
            "couldn't read plugin `{}` from the granted FS.Read roots",
            path.display()
        )
    }))
}

/// Load a plugin `.wasm` Component Model module from `path`. Returns
/// `"O:<handle>"` (decimal, > 0) on success, or `"E:<message>"` naming why the
/// load failed — a missing file, a module that isn't a valid component, or a
/// component that declares a host import (denied by construction: the linker
/// registers none). Never panics — every wasmtime error is caught and
/// rendered as a plain message (I2: no raw loader crash reaches the host
/// program).
pub fn jet_plugin_load(path: &str, authority: &str) -> String {
    let component_bytes = match plugin_read_module(path, authority) {
        Ok(bytes) => bytes,
        Err(error) => return format!("E:{error}"),
    };
    let engine = match plugin_engine() {
        Ok(engine) => engine,
        Err(error) => return format!("E:{error}"),
    };
    let component = match Component::new(&engine, &component_bytes) {
        Ok(c) => c,
        Err(e) => return format!("E:couldn't load plugin `{path}`: {e}"),
    };
    // D-PLUGIN1=B: deny-by-default capabilities — an empty linker means any
    // plugin that imports a host function fails to instantiate here, with a
    // clean message naming the missing import, not a panic.
    let linker: Linker<PluginHostState> = Linker::new(&engine);
    let mut store = Store::new(
        &engine,
        PluginHostState {
            limits: plugin_limits(),
        },
    );
    store.limiter(|state| &mut state.limits);
    store.set_epoch_deadline(1_000_000_000);
    store.epoch_deadline_trap();
    if let Err(error) = store.set_fuel(PLUGIN_MAX_FUEL) {
        return format!("E:plugin fuel setup failed: {error}");
    }
    let instance = match linker.instantiate(&mut store, &component) {
        Ok(i) => i,
        Err(e) => {
            return format!(
                "E:plugin `{path}` couldn't be instantiated (it may require a host ability, which sandboxed plugins never get): {e}"
            );
        }
    };
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    PLUGINS.with(|m| {
        m.borrow_mut()
            .insert(handle, PluginInstance {
                engine,
                store,
                instance,
                authority: authority.to_string(),
            })
    });
    format!("O:{handle}")
}

/// Close a loaded plugin. Returns `true` if the handle was valid, `false`
/// otherwise (already closed, or never opened).
pub fn jet_plugin_close(handle: u64) -> bool {
    PLUGINS.with(|m| m.borrow_mut().remove(&handle).is_some())
}

/// Call exported function `name` on `handle` with the wire-encoded
/// `[PluginValue]` argument list `params_wire`. Returns `"O:"` + the
/// wire-encoded `PluginValue` result, or `"E:"` + a plain message — a missing
/// export, a param-count/type mismatch against the plugin's actual `.wit`
/// signature, or a trap during the call. Every path is a `Result`; nothing
/// here can panic the host program (I2).
pub fn jet_plugin_call(handle: u64, name: &str, params_wire: &str) -> String {
    PLUGINS.with(|m| {
        let mut map = m.borrow_mut();
        let Some(plugin) = map.get_mut(&handle) else {
            return "E:no plugin loaded for this handle".to_string();
        };
        if params_wire.len() > PLUGIN_MAX_WIRE_BYTES {
            return "E:plugin call arguments exceed the 16 MiB resource budget".to_string();
        }
        let Some(func) = plugin.instance.get_func(&mut plugin.store, name) else {
            return format!("E:plugin has no exported function `{name}`");
        };
        let want_params = func.params(&plugin.store);
        let decoded = plugin_decode_params(params_wire);
        let args = match decoded {
            Ok(args) => args,
            Err(PluginParamDecodeError::TooMany { .. }) => {
                // Preserve the established arity diagnostic while keeping the
                // rejection typed internally; this must never become an empty
                // argument list that can satisfy a zero-parameter export.
                return format!(
                    "E:`{name}` expects {} argument(s), got 0",
                    want_params.len()
                );
            }
            Err(error) => {
                return format!("E:plugin call arguments rejected: {error}");
            }
        };
        if args.len() != want_params.len() {
            return format!(
                "E:`{name}` expects {} argument(s), got {}",
                want_params.len(),
                args.len()
            );
        }
        let mut call_args = Vec::with_capacity(args.len());
        for (i, (arg, ty)) in args.iter().zip(want_params.iter()).enumerate() {
            match plugin_to_val(arg, ty) {
                Some(v) => call_args.push(v),
                None => {
                    return format!(
                        "E:argument {} to `{name}` doesn't match the plugin's declared type ({})",
                        i + 1,
                        plugin_type_name(ty)
                    );
                }
            }
        }
        if let Err(error) = plugin.store.set_fuel(PLUGIN_MAX_FUEL) {
            return format!("E:plugin fuel setup failed: {error}");
        }
        plugin.store.set_epoch_deadline(1);
        plugin.store.epoch_deadline_trap();
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancelled_timer = std::sync::Arc::clone(&cancelled);
        let timed_out = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let timed_out_timer = std::sync::Arc::clone(&timed_out);
        let timer_engine = plugin.engine.clone();
        let timer = std::thread::spawn(move || {
            let deadline = std::time::Instant::now()
                + std::time::Duration::from_millis(PLUGIN_TIMEOUT_MS);
            while std::time::Instant::now() < deadline {
                if cancelled_timer.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            if !cancelled_timer.load(Ordering::Relaxed) {
                timed_out_timer.store(true, Ordering::Relaxed);
                timer_engine.increment_epoch();
            }
        });
        let want_results = func.results(&plugin.store);
        if want_results.len() != 1 {
            cancelled.store(true, Ordering::Relaxed);
            let _ = timer.join();
            plugin.store.set_epoch_deadline(1_000_000_000);
            return format!(
                "E:`{name}` returns {} values — v1 plugin calls support exactly one return value",
                want_results.len()
            );
        }
        let mut results = vec![plugin_zero_val(&want_results[0])];
        let call_result = func.call(&mut plugin.store, &call_args, &mut results);
        cancelled.store(true, Ordering::Relaxed);
        let _ = timer.join();
        plugin.store.set_epoch_deadline(1_000_000_000);
        if let Err(error) = call_result {
            let fuel_exhausted = matches!(plugin.store.get_fuel(), Ok(0));
            let deadline_elapsed = timed_out.load(Ordering::Relaxed);
            return plugin_call_trap_error(name, error, fuel_exhausted, deadline_elapsed);
        }
        // Component Model contract: `post_return` must run after every call
        // before the instance can be called again.
        let _ = func.post_return(&mut plugin.store);
        match plugin_from_val(&results[0]) {
            Some(wire) if wire.len() <= PLUGIN_MAX_WIRE_BYTES => format!("O:{wire}"),
            Some(_) => "E:plugin result exceeds the 16 MiB resource budget".to_string(),
            None => format!(
                "E:`{name}`'s return type isn't supported yet (v1 plugin calls support Int/Float/Bool/Text only)"
            ),
        }
    })
}

fn plugin_call_trap_error(
    name: &str,
    error: wasmtime::Error,
    fuel_exhausted: bool,
    deadline_elapsed: bool,
) -> String {
    let limit_trap = error
        .downcast_ref::<Trap>()
        .is_some_and(|trap| matches!(*trap, Trap::OutOfFuel | Trap::Interrupt));
    if fuel_exhausted || deadline_elapsed || limit_trap {
        return format!(
            "E:calling `{name}` trapped: plugin execution limit reached (fuel/epoch interrupt)"
        );
    }
    format!("E:calling `{name}` trapped: {error}")
}

/// Component Model scalar types accepted by the plugin export validator and
/// checked again at the dynamic host boundary.
fn plugin_type_name(ty: &Type) -> &'static str {
    match ty {
        Type::S64 => "Int",
        Type::Float64 => "Float",
        Type::Bool => "Bool",
        Type::String => "Text",
        _ => "unsupported",
    }
}

fn plugin_to_val(tagged: &(char, String), ty: &Type) -> Option<Val> {
    let (tag, payload) = tagged;
    match (tag, ty) {
        ('I', Type::S64) => payload.parse::<i64>().ok().map(Val::S64),
        ('F', Type::Float64) => payload.parse::<f64>().ok().map(Val::Float64),
        ('B', Type::Bool) => match payload.as_str() {
            "true" => Some(Val::Bool(true)),
            "false" => Some(Val::Bool(false)),
            _ => None,
        },
        ('T', Type::String) => Some(Val::String(payload.clone())),
        _ => None,
    }
}

fn plugin_zero_val(ty: &Type) -> Val {
    match ty {
        Type::S64 => Val::S64(0),
        Type::Float64 => Val::Float64(0.0),
        Type::Bool => Val::Bool(false),
        Type::String => Val::String(String::new()),
        _ => Val::S64(0),
    }
}

fn plugin_from_val(v: &Val) -> Option<String> {
    match v {
        Val::S64(n) => Some(plugin_encode_tagged('I', &n.to_string())),
        Val::Float64(f) => Some(plugin_encode_tagged('F', &f.to_string())),
        Val::Bool(value) => Some(plugin_encode_tagged('B', if *value { "true" } else { "false" })),
        Val::String(value) => Some(plugin_encode_tagged('T', value)),
        _ => None,
    }
}

// ── wire encoding: tagged, length-prefixed, byte-exact (mirrors DB.rs) ──────

fn plugin_encode_tagged(tag: char, payload: &str) -> String {
    format!("{tag}{}:{payload}", payload.len())
}

fn plugin_read_tagged(bytes: &[u8], pos: &mut usize) -> Option<(char, String)> {
    let tag = *bytes.get(*pos)? as char;
    *pos += 1;
    let len_start = *pos;
    while *bytes.get(*pos)? != b':' {
        *pos += 1;
    }
    let len: usize = std::str::from_utf8(&bytes[len_start..*pos]).ok()?.parse().ok()?;
    *pos += 1; // skip ':'
    let payload_end = (*pos).checked_add(len)?;
    let payload = std::str::from_utf8(bytes.get(*pos..payload_end)?).ok()?.to_string();
    *pos = payload_end;
    Some((tag, payload))
}

#[derive(Debug)]
enum PluginParamDecodeError {
    Malformed,
    TooMany { count: usize },
}

impl std::fmt::Display for PluginParamDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed => f.write_str("malformed plugin call argument frame"),
            Self::TooMany { count } => write!(
                f,
                "plugin call argument frame contains {count} parameters; maximum is {PLUGIN_MAX_PARAMS}"
            ),
        }
    }
}

/// Decode a count-prefixed tagged-value list (the same shape `DB.rs` uses for
/// bind params): `"<count>:<tag><len>:<payload>…"`. Every byte must belong to
/// one declared value; malformed input is a rejection, never an empty list.
fn plugin_decode_params(
    wire: &str,
) -> Result<Vec<(char, String)>, PluginParamDecodeError> {
    let bytes = wire.as_bytes();
    let Some(colon) = bytes.iter().position(|byte| *byte == b':') else {
        return Err(PluginParamDecodeError::Malformed);
    };
    let count = std::str::from_utf8(&bytes[..colon])
        .ok()
        .and_then(|text| text.parse::<usize>().ok())
        .ok_or(PluginParamDecodeError::Malformed)?;
    if count > PLUGIN_MAX_PARAMS {
        return Err(PluginParamDecodeError::TooMany { count });
    }
    let mut pos = colon + 1;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let pair =
            plugin_read_tagged(bytes, &mut pos).ok_or(PluginParamDecodeError::Malformed)?;
        out.push(pair);
    }
    if pos != bytes.len() {
        return Err(PluginParamDecodeError::Malformed);
    }
    Ok(out)
}
