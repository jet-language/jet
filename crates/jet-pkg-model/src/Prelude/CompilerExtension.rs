// jet compiler-extension host (D-DX5-HOOK1=A, Tower #549) — sandboxed WASM
// Component Model loader for post-sema typed analysis components.
//
// Ownership: lives in jet-pkg-model next to the application `core.plugin`
// runtime (`Prelude/Plugin.rs`). Reuses the same wasmtime Component Model
// substrate and crate pin (`WASMTIME_CRATE_SPEC` / D-DEP-WASM1=A). This is
// NOT application `target: plugin` and NOT PATH `jet-*` helpers (I8).
//
// World: `compiler-extension-v1` (see `crate::CompilerExtension::wit_world`).
// Application plugins use the fixed world `jetplugin` instead.
//
// Safety model: the linker registers *zero* host imports, so a component that
// declares any import fails to instantiate. V1 components only export pure
// `analyze(snapshot: list<u8>) -> list<u8>`; the host owns snapshot schema,
// response validation, diagnostics, and semantic authority (I2/I3).
//
// Resource limits (must match `CompilerExtension::ResourceLimits::v1_defaults`):
// fuel 10_000_000, memory 16 MiB, table 10_000 elements. Wall-clock
// `timeout_ms` (2000) is declared on the snapshot; epoch interruption can
// enforce it on the analyze path without changing this loader contract.
//
// Handles are u64 keys into a thread-local HashMap (same shape as Plugin.rs).
// Handle 0 is the error sentinel. Helper names are prefixed
// `jet_compiler_extension_` so they never collide with `jet_plugin_*` when
// both runtimes are present in one process.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use wasmtime::component::{Component, Linker, Val};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};

/// Fixed WIT world name — must match `CompilerExtension::WORLD_NAME`.
const COMPILER_EXTENSION_WORLD: &str = "compiler-extension-v1";

/// Required export — must match `CompilerExtension::ANALYZE_EXPORT`.
const ANALYZE_EXPORT: &str = "analyze";

/// Mirror of `ResourceLimits::v1_defaults().max_fuel`.
const V1_MAX_FUEL: u64 = 10_000_000;
/// Mirror of `ResourceLimits::v1_defaults().max_memory_bytes`.
const V1_MAX_MEMORY_BYTES: usize = 16 * 1024 * 1024; // 16777216
/// Mirror of `ResourceLimits::v1_defaults().max_table_elements`.
const V1_MAX_TABLE_ELEMENTS: usize = 10_000;

struct ExtensionHostState {
    limits: StoreLimits,
}

struct CompilerExtensionInstance {
    store: Store<ExtensionHostState>,
    instance: wasmtime::component::Instance,
}

thread_local! {
    static EXTENSIONS: RefCell<HashMap<u64, CompilerExtensionInstance>> =
        RefCell::new(HashMap::new());
}

static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

fn v1_store_limits() -> StoreLimits {
    StoreLimitsBuilder::new()
        .memory_size(V1_MAX_MEMORY_BYTES)
        .table_elements(V1_MAX_TABLE_ELEMENTS)
        .instances(1)
        .memories(1)
        .tables(1)
        .build()
}

fn v1_engine() -> Result<Engine, String> {
    let mut config = Config::new();
    config.consume_fuel(true);
    Engine::new(&config).map_err(|e| format!("compiler-extension engine: {e}"))
}

/// Load a compiler-extension `.wasm` Component Model module from `path`.
/// Returns `"O:<handle>"` on success, or `"E:<message>"` on failure.
/// Never panics — every wasmtime error becomes a plain message (I2).
pub fn jet_compiler_extension_load(path: &str) -> String {
    let engine = match v1_engine() {
        Ok(e) => e,
        Err(e) => return format!("E:{e}"),
    };
    let component = match Component::from_file(&engine, path) {
        Ok(c) => c,
        Err(e) => {
            return format!("E:couldn't load compiler-extension `{path}`: {e}");
        }
    };
    // Deny-by-default: empty linker — any host import fails instantiate.
    let linker: Linker<ExtensionHostState> = Linker::new(&engine);
    let mut store = Store::new(
        &engine,
        ExtensionHostState {
            limits: v1_store_limits(),
        },
    );
    store.limiter(|state| &mut state.limits);
    if let Err(e) = store.set_fuel(V1_MAX_FUEL) {
        return format!("E:compiler-extension fuel setup failed: {e}");
    }
    let instance = match linker.instantiate(&mut store, &component) {
        Ok(i) => i,
        Err(e) => {
            return format!(
                "E:compiler-extension `{path}` couldn't be instantiated \
                 (world `{COMPILER_EXTENSION_WORLD}` components get no host \
                 imports): {e}"
            );
        }
    };
    if instance.get_func(&mut store, ANALYZE_EXPORT).is_none() {
        return format!(
            "E:compiler-extension `{path}` has no exported `{ANALYZE_EXPORT}` \
             (world `{COMPILER_EXTENSION_WORLD}` requires it; this is not an \
             application `jetplugin` target)"
        );
    }
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    EXTENSIONS.with(|m| {
        m.borrow_mut()
            .insert(handle, CompilerExtensionInstance { store, instance })
    });
    format!("O:{handle}")
}

/// Close a loaded compiler-extension. Returns `true` if the handle was live.
/// Closing drops guest memory — uncommitted staged host results stay host-side
/// and must be rolled back by the session owner separately.
pub fn jet_compiler_extension_close(handle: u64) -> bool {
    EXTENSIONS.with(|m| m.borrow_mut().remove(&handle).is_some())
}

/// Call `analyze` with raw snapshot bytes. Returns `"O:"` + response bytes
/// encoded as a length-prefixed payload, or `"E:"` + a plain message.
/// V1 list<u8> ABI uses the Component Model `Val::List` path; every failure
/// is caught so a trapped guest cannot crash the compiler host (I2).
/// Fuel is reset to the v1 budget before each call.
pub fn jet_compiler_extension_analyze(handle: u64, snapshot: &[u8]) -> String {
    EXTENSIONS.with(|m| {
        let mut map = m.borrow_mut();
        let Some(ext) = map.get_mut(&handle) else {
            return "E:no compiler-extension loaded for this handle".to_string();
        };
        if let Err(e) = ext.store.set_fuel(V1_MAX_FUEL) {
            return format!("E:compiler-extension fuel reset failed: {e}");
        }
        let Some(func) = ext.instance.get_func(&mut ext.store, ANALYZE_EXPORT) else {
            return format!("E:compiler-extension has no exported `{ANALYZE_EXPORT}`");
        };
        let args = [Val::List(
            snapshot.iter().copied().map(Val::U8).collect(),
        )];
        let mut results = [Val::List(Vec::new())];
        if let Err(e) = func.call(&mut ext.store, &args, &mut results) {
            return format!("E:calling `{ANALYZE_EXPORT}` trapped: {e}");
        }
        let _ = func.post_return(&mut ext.store);
        match &results[0] {
            Val::List(vals) => {
                let mut bytes = Vec::with_capacity(vals.len());
                for v in vals {
                    let Val::U8(b) = v else {
                        return format!(
                            "E:`{ANALYZE_EXPORT}` returned a non-byte list element"
                        );
                    };
                    bytes.push(*b);
                }
                format!("O:{}", cex_encode_bytes(&bytes))
            }
            _ => format!("E:`{ANALYZE_EXPORT}` must return list<u8>"),
        }
    })
}

fn cex_encode_bytes(bytes: &[u8]) -> String {
    // Length-prefixed hex — byte-exact, no escaping, mirrors other bridge wires.
    let mut out = format!("{}:", bytes.len());
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}
