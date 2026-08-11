// D-LIB-CALLGRANT1=A: JIT uses the exact load/check/map Prelude source. This
// module only translates heap-level handles into a resident table slot.
const __JET_COMPILER_VERSION: &str = env!("CARGO_PKG_VERSION");
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
    if id <= 0 {
        return Err("invalid Mod handle".to_string());
    }
    LOADED.with(|loaded| {
        let loaded = loaded.borrow();
        let module = loaded
            .get((id - 1) as usize)
            .ok_or_else(|| "invalid Mod handle".to_string())?;
        jet_mod_on_tick(module, dt)
    })
}

