//! Hidden build-plugin Component Model host.
//!
//! This file is included only by `jetpack-bin`. The compiler-linked crates
//! exchange bounded request/response bytes and never link Wasmtime (I6).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use wasmtime::component::{Component, Linker, Val};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};

const BUILD_EXPORT: &str = "build";
const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_FUEL: u64 = 10_000_000;
const MAX_MEMORY_BYTES: usize = 16 * 1024 * 1024;
const MAX_TABLE_ELEMENTS: u32 = 10_000;
const TIMEOUT_MS: u64 = 2_000;

struct HostState {
    limits: StoreLimits,
}

fn engine() -> Result<Engine, String> {
    let mut config = Config::new();
    config.consume_fuel(true);
    config.epoch_interruption(true);
    Engine::new(&config).map_err(|error| format!("build-plugin engine: {error}"))
}

fn limits() -> StoreLimits {
    StoreLimitsBuilder::new()
        .memory_size(MAX_MEMORY_BYTES)
        .table_elements(MAX_TABLE_ELEMENTS)
        .instances(1)
        .memories(1)
        .tables(1)
        .build()
}

fn guest_name(path: &str) -> &str {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
}

/// Instantiate one packaged build component, call its pure `build` export,
/// and drop the store before returning. The linker has no imports: any guest
/// capability request fails at instantiation rather than escaping the sandbox.
pub fn run(path: &str, component_bytes: &[u8], request: &[u8]) -> Result<Vec<u8>, String> {
    if request.len() > MAX_REQUEST_BYTES {
        return Err(format!(
            "build-plugin request exceeds {MAX_REQUEST_BYTES} bytes"
        ));
    }
    let engine = engine()?;
    let component = Component::new(&engine, component_bytes)
        .map_err(|error| format!("couldn't load build plugin {}: {error}", guest_name(path)))?;
    let linker: Linker<HostState> = Linker::new(&engine);
    let mut store = Store::new(&engine, HostState { limits: limits() });
    store.limiter(|state| &mut state.limits);
    store.set_epoch_deadline(1_000_000_000);
    store.epoch_deadline_trap();
    store
        .set_fuel(MAX_FUEL)
        .map_err(|error| format!("build-plugin fuel setup failed: {error}"))?;
    let instance = linker.instantiate(&mut store, &component).map_err(|error| {
        format!(
            "build plugin {} couldn't be instantiated (the build world has no host imports): {error}",
            guest_name(path)
        )
    })?;
    let func = instance
        .get_func(&mut store, BUILD_EXPORT)
        .ok_or_else(|| format!("build plugin {} has no exported {BUILD_EXPORT}", guest_name(path)))?;

    store.set_epoch_deadline(1);
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_flag = Arc::clone(&cancel);
    let timer_engine = engine.clone();
    let timer = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_millis(TIMEOUT_MS);
        while Instant::now() < deadline {
            if cancel_flag.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        if !cancel_flag.load(Ordering::Relaxed) {
            timer_engine.increment_epoch();
        }
    });
    let args = [Val::List(request.iter().copied().map(Val::U8).collect())];
    let mut results = [Val::List(Vec::new())];
    let call_result = func.call(&mut store, &args, &mut results);
    cancel.store(true, Ordering::Relaxed);
    let _ = timer.join();
    store.set_epoch_deadline(1_000_000_000);
    call_result.map_err(|error| format!("calling build plugin {BUILD_EXPORT} trapped: {error}"))?;
    func.post_return(&mut store)
        .map_err(|error| format!("build plugin post-return failed: {error}"))?;
    let Val::List(values) = &results[0] else {
        return Err(format!("build plugin {BUILD_EXPORT} must return list<u8>"));
    };
    if values.len() > MAX_RESPONSE_BYTES {
        return Err(format!(
            "build plugin response exceeds {MAX_RESPONSE_BYTES} bytes"
        ));
    }
    values
        .iter()
        .map(|value| match value {
            Val::U8(byte) => Ok(*byte),
            _ => Err(format!("build plugin {BUILD_EXPORT} returned a non-byte list")),
        })
        .collect()
}
