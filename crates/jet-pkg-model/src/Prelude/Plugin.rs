// jet.plugin runtime (D-DEP-WASM1=A, c81) — sandboxed WASM Component Model
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
// boundary; there is no `@Unsafe` gate anywhere in this file or the generated
// call sites (I1).
//
// Handles are u64 keys into a thread-local HashMap, mirroring `Db.rs`. Handle
// 0 is the error sentinel (never a live plugin instance).
//
// Wire protocol (mirrors `Db.rs`'s tagged-length encoding — byte-exact,
// nothing to escape): a scalar value is `I<len>:<int>` (Jet `Int`) or
// `F<len>:<float>` (Jet `Float`) — v1's supported plugin-call scalar types. A
// call result or error is `O:<value>` / `E:<message>`. Helper names are
// prefixed `plugin_` so they never collide with `Db.rs`'s identically-shaped
// `encode_tagged`/`read_tagged` helpers when both runtimes are concatenated
// into the same bridge crate.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use wasmtime::component::{Component, Linker, Type, Val};
use wasmtime::{Engine, Store};

struct PluginInstance {
    store: Store<()>,
    instance: wasmtime::component::Instance,
}

thread_local! {
    static PLUGINS: RefCell<HashMap<u64, PluginInstance>> = RefCell::new(HashMap::new());
}

static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

/// Load a plugin `.wasm` Component Model module from `path`. Returns
/// `"O:<handle>"` (decimal, > 0) on success, or `"E:<message>"` naming why the
/// load failed — a missing file, a module that isn't a valid component, or a
/// component that declares a host import (denied by construction: the linker
/// registers none). Never panics — every wasmtime error is caught and
/// rendered as a plain message (I2: no raw loader crash reaches the host
/// program).
pub fn jet_plugin_load(path: &str) -> String {
    let engine = Engine::default();
    let component = match Component::from_file(&engine, path) {
        Ok(c) => c,
        Err(e) => return format!("E:couldn't load plugin `{path}`: {e}"),
    };
    // D-PLUGIN1=B: deny-by-default capabilities — an empty linker means any
    // plugin that imports a host function fails to instantiate here, with a
    // clean message naming the missing import, not a panic.
    let linker: Linker<()> = Linker::new(&engine);
    let mut store = Store::new(&engine, ());
    let instance = match linker.instantiate(&mut store, &component) {
        Ok(i) => i,
        Err(e) => {
            return format!(
                "E:plugin `{path}` couldn't be instantiated (it may require a host capability, which sandboxed plugins never get): {e}"
            );
        }
    };
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    PLUGINS.with(|m| {
        m.borrow_mut()
            .insert(handle, PluginInstance { store, instance })
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
        let Some(func) = plugin.instance.get_func(&mut plugin.store, name) else {
            return format!("E:plugin has no exported function `{name}`");
        };
        let want_params = func.params(&plugin.store);
        let args = plugin_decode_params(params_wire);
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
        let want_results = func.results(&plugin.store);
        if want_results.len() != 1 {
            return format!(
                "E:`{name}` returns {} values — v1 plugin calls support exactly one return value",
                want_results.len()
            );
        }
        let mut results = vec![plugin_zero_val(&want_results[0])];
        if let Err(e) = func.call(&mut plugin.store, &call_args, &mut results) {
            return format!("E:calling `{name}` trapped: {e}");
        }
        // Component Model contract: `post_return` must run after every call
        // before the instance can be called again.
        let _ = func.post_return(&mut plugin.store);
        match plugin_from_val(&results[0]) {
            Some(wire) => format!("O:{wire}"),
            None => format!(
                "E:`{name}`'s return type isn't supported yet (v1 plugin calls support Int/Float/Bool/Text only)"
            ),
        }
    })
}

/// v1 (c81) supports Int/Float scalar plugin calls only — a plugin export
/// must be all-`Int` or all-`Float` in its params and return (checked at the
/// plugin's own build time; see `ApiFreeze`-based export validation in the
/// driver). Bool/Text aren't wired yet: Bool is a trivial scalar to add later,
/// but Text needs the Component Model's memory-based string ABI (a
/// `cabi_realloc` export this hand-rolled scalar guest ABI doesn't have yet) —
/// a real follow-on increment, not a stub inside this one.
fn plugin_type_name(ty: &Type) -> &'static str {
    match ty {
        Type::S64 => "Int",
        Type::Float64 => "Float",
        _ => "unsupported",
    }
}

fn plugin_to_val(tagged: &(char, String), ty: &Type) -> Option<Val> {
    let (tag, payload) = tagged;
    match (tag, ty) {
        ('I', Type::S64) => payload.parse::<i64>().ok().map(Val::S64),
        ('F', Type::Float64) => payload.parse::<f64>().ok().map(Val::Float64),
        _ => None,
    }
}

fn plugin_zero_val(ty: &Type) -> Val {
    match ty {
        Type::S64 => Val::S64(0),
        Type::Float64 => Val::Float64(0.0),
        _ => Val::S64(0),
    }
}

fn plugin_from_val(v: &Val) -> Option<String> {
    match v {
        Val::S64(n) => Some(plugin_encode_tagged('I', &n.to_string())),
        Val::Float64(f) => Some(plugin_encode_tagged('F', &f.to_string())),
        _ => None,
    }
}

// ── wire encoding: tagged, length-prefixed, byte-exact (mirrors Db.rs) ──────

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
    let payload = std::str::from_utf8(bytes.get(*pos..*pos + len)?).ok()?.to_string();
    *pos += len;
    Some((tag, payload))
}

/// Decode a count-prefixed tagged-value list (the same shape `Db.rs` uses for
/// bind params): `"<count>:<tag><len>:<payload>…"`.
fn plugin_decode_params(wire: &str) -> Vec<(char, String)> {
    let bytes = wire.as_bytes();
    let Some(colon) = bytes.iter().position(|b| *b == b':') else { return Vec::new() };
    let Ok(count) = std::str::from_utf8(&bytes[..colon]).unwrap_or("0").parse::<usize>() else {
        return Vec::new();
    };
    let mut pos = colon + 1;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let Some(pair) = plugin_read_tagged(bytes, &mut pos) else { break };
        out.push(pair);
    }
    out
}
