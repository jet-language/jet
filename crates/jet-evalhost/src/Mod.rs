// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.
#![allow(dead_code)]

// D-LIB-CALLGRANT1=A: JIT uses the exact load/check/map Prelude source. This
// module only translates heap-level handles into a resident table slot.
const __JET_COMPILER_VERSION: &str = env!("CARGO_PKG_VERSION");
include!("../../jet-codegen/src/Prelude/CoreLib/Top/SHA256Raw.rs");
include!("../../jet-codegen/src/Prelude/CoreLib/Top/Mod.rs");

use std::cell::RefCell;

thread_local! {
    static LOADED: RefCell<Vec<JetMod>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn load(path: String, read: Vec<String>) -> Result<i64, String> {
    let module = jet_mod_load(&path, &JetModGrant { read })?;
    Ok(LOADED.with(|loaded| {
        let mut loaded = loaded.borrow_mut();
        loaded.push(module);
        loaded.len() as i64
    }))
}

pub(crate) fn on_tick(id: i64, dt: i64) -> Result<i64, String> {
    call_int(id, "on_tick", &[dt])
}

pub(crate) fn call_int(id: i64, name: &str, args: &[i64]) -> Result<i64, String> {
    if id <= 0 {
        return Err("invalid Mod handle".to_string());
    }
    LOADED.with(|loaded| {
        let loaded = loaded.borrow();
        let module = loaded
            .get((id - 1) as usize)
            .ok_or_else(|| "invalid Mod handle".to_string())?;
        jet_mod_call_int(module, name, args)
    })
}

/// Release every load owned by the current JIT/interpreter invocation. The
/// integer handle is an engine carrier only; the shared Prelude `JetMod` owns
/// the native handle and staged payload and performs the actual unload in
/// `Drop`.
pub(crate) fn clear() {
    LOADED.with(|loaded| loaded.borrow_mut().clear());
}

/// Scope native module handles to one non-resident execution. Resident
/// hot-swap sessions intentionally keep their table; one-shot runs and
/// interpreter/deopt invocations install this guard so every return path
/// closes handles and removes staged payloads.
pub(crate) struct LoadScope;

impl Drop for LoadScope {
    fn drop(&mut self) {
        clear();
    }
}
