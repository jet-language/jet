//! Audited C byte-carrier marshalling for evaluator tiers.
//! Rust seed/verification seam: dies with the Rust evaluator at cutover.
use crate::AST::CtValue;
use crate::Diagnostics::{Diagnostic, Span};
use std::cell::Cell;
use std::collections::HashMap;

mod kernel {
    include!("../../../jet-codegen/src/Prelude/Core/ForeignBytes.rs");
}

thread_local! {
    // Borrow the EXISTING expression scope; no copied String/pointer registry.
    // This descriptor is installed only while the MIR fragment executes.
    static GLOBAL_HOMES: Cell<(*const HashMap<String, CtValue>, *const HashMap<String, CtValue>)> = const { Cell::new((std::ptr::null(), std::ptr::null())) };
}

pub fn with_physical_globals<R>(globals: &HashMap<String, CtValue>, scope: Option<&HashMap<String, CtValue>>, body: impl FnOnce() -> R) -> R {
    struct Restore((*const HashMap<String, CtValue>, *const HashMap<String, CtValue>));
    impl Drop for Restore {
        fn drop(&mut self) { GLOBAL_HOMES.with(|slot| slot.set(self.0)); }
    }
    let _restore = Restore(GLOBAL_HOMES.with(|slot| slot.replace((globals, scope.map_or(std::ptr::null(), |scope| scope)))));
    body()
}

pub fn physical_global_string_home(name: &str, visitor: impl FnOnce(&CtValue) -> Result<(u64, usize), Diagnostic>) -> Option<Result<(u64, usize), Diagnostic>> {
    GLOBAL_HOMES.with(|slot| {
        let (globals, scope) = slot.get();
        // SAFETY: with_physical_globals holds both maps borrowed for the entire
        // fragment. No mutation occurs until its writeback after this scope.
        // The visitor cannot return a Rust reference into either map.
        let value = unsafe { scope.as_ref() }.and_then(|scope| scope.get(name))
            .or_else(|| unsafe { globals.as_ref() }.and_then(|globals| globals.get(name)));
        value.map(visitor)
    })
}

pub fn bytes_call(method: &str, args: &[CtValue], span: Span) -> Option<Result<CtValue, Diagnostic>> {
    if !matches!(method, "__bytes_pack" | "__bytes_free" | "__bytes_unpack") { return None; }
    Some((|| {
        let bad = || Diagnostic::error("E2104", format!("core.mem.{method}: invalid checked C byte carrier"), "the host loan requires a bounded readable span".into(), "keep the source home live inside its audited #Unsafe region".into(), Some(span));
        if method == "__bytes_pack" {
            let [value] = args else { return Err(bad()); };
            let pointer = match value {
                CtValue::Bytes(bytes) => kernel::jet_bytes_pack(bytes),
                CtValue::List(values) => {
                    if !values.iter().all(|value| matches!(value, CtValue::Int(byte) if u8::try_from(*byte).is_ok())) {
                        return Err(bad());
                    }
                    kernel::jet_bytes_pack_iter(values.iter().map(|value| match value {
                        CtValue::Int(byte) => *byte as u8,
                        _ => unreachable!("checked byte list"),
                    }))
                }
                _ => return Err(bad()),
            };
            return Ok(CtValue::Int(pointer as i64));
        }
        let [CtValue::Int(pointer), CtValue::Int(length)] = args else { return Err(bad()); };
        let length = jet_foundation::Numeric::JetInt::from(*length);
        let word = length.to_raw();
        if method == "__bytes_free" {
            kernel::jet_bytes_free(*pointer as u64, &word);
            Ok(CtValue::Unit)
        } else {
            Ok(CtValue::Bytes(kernel::jet_bytes_unpack(*pointer as u64, &word)))
        }
    })())
}
