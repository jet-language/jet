//! Optional host callbacks for whole-program interpreter deopt (`jet run`).
//!
//! Cranelift hosts for `jet.db` / `jet.crypto` live in `jet-jit` (rusqlite +
//! bridge crypto). Pure comptime / REPL leave this unset so those modules stay
//! unsupported or REPL-native-denied. `jet-jit` installs hooks only around
//! `TirBridge::run_bundle` for runtime-tier deopt.

use std::cell::Cell;

use crate::AST::CtValue;
use crate::Diagnostics::{Diagnostic, Span};

pub type AmbientCoreCall =
    fn(&str, &str, Vec<CtValue>, Span) -> Option<Result<CtValue, Diagnostic>>;
pub type AmbientHandle =
    fn(&str, &mut CtValue, &mut [CtValue], Span) -> Option<Result<CtValue, Diagnostic>>;

thread_local! {
    static CORE_CALL: Cell<Option<AmbientCoreCall>> = const { Cell::new(None) };
    static HANDLE: Cell<Option<AmbientHandle>> = const { Cell::new(None) };
}

/// Install ambient hooks for the duration of `body`, then clear them.
pub fn with_ambient<R>(
    core_call: Option<AmbientCoreCall>,
    handle: Option<AmbientHandle>,
    body: impl FnOnce() -> R,
) -> R {
    CORE_CALL.with(|slot| slot.set(core_call));
    HANDLE.with(|slot| slot.set(handle));
    let out = body();
    CORE_CALL.with(|slot| slot.set(None));
    HANDLE.with(|slot| slot.set(None));
    out
}

pub fn try_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    CORE_CALL.with(|slot| slot.get()).and_then(|hook| hook(module, method, args, span))
}

pub fn try_handle(
    op: &str,
    recv: &mut CtValue,
    args: &mut [CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    HANDLE.with(|slot| slot.get()).and_then(|hook| hook(op, recv, args, span))
}
