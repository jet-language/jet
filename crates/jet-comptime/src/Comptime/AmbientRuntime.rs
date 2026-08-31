//! Optional host callbacks for whole-program interpreter deopt (`jet run`).
//!
//! Cranelift hosts for `core.db` / `core.crypto` live in `jet-jit` (rusqlite +
//! bridge crypto). Pure comptime / REPL leave this unset so those modules stay
//! unsupported or REPL-native-denied. `jet-jit` installs hooks only around
//! `TirBridge::run_bundle` for runtime-tier deopt.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use crate::Comptime::DevSink;
use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{CtValue, Type};
use crate::AST::ComptimeInput;

pub type AmbientCoreCall = fn(
    &str,
    &str,
    Vec<CtValue>,
    Span,
    Option<Type>,
    Option<&mut DevSink>,
) -> Option<Result<CtValue, Diagnostic>>;
pub type AmbientHandle =
    fn(&str, &mut CtValue, &mut [CtValue], Span) -> Option<Result<CtValue, Diagnostic>>;
pub type AmbientExternCall =
    fn(&str, Vec<CtValue>, Span, Option<Type>) -> Option<Result<CtValue, Diagnostic>>;

thread_local! {
    static CORE_CALL: Cell<Option<AmbientCoreCall>> = const { Cell::new(None) };
    static HANDLE: Cell<Option<AmbientHandle>> = const { Cell::new(None) };
    static EXTERN_CALL: Cell<Option<AmbientExternCall>> = const { Cell::new(None) };
    static PACKAGE_READ_CONTEXT: RefCell<Option<PackageReadContext>> = const { RefCell::new(None) };
}

#[derive(Debug, Default)]
struct PackageReadContext {
    root: PathBuf,
    inputs: Vec<ComptimeInput>,
}

/// Run compile-time work with the package root used by the public package
/// views. The context is deliberately separate from the ambient Core callback:
/// the callback stays a function pointer, while package reads need the
/// selected build root and must append their hashes to the existing input
/// provenance stream.
pub fn with_package_read_context<R>(root: &Path, body: impl FnOnce() -> R) -> (R, Vec<ComptimeInput>) {
    let previous = PACKAGE_READ_CONTEXT.with(|slot| {
        slot.replace(Some(PackageReadContext {
            root: root.to_path_buf(),
            inputs: Vec::new(),
        }))
    });
    let result = body();
    let inputs = match PACKAGE_READ_CONTEXT.with(|slot| slot.replace(previous)) {
        Some(current) => current.inputs,
        None => Vec::new(),
    };
    PACKAGE_READ_CONTEXT.with(|slot| {
        if let Some(parent) = slot.borrow_mut().as_mut() {
            for input in &inputs {
                if !parent.inputs.iter().any(|existing| existing.path == input.path) {
                    parent.inputs.push(input.clone());
                }
            }
        }
    });
    (result, inputs)
}

/// Return the pinned package root for a compile-time package-view call.
pub fn package_read_root() -> Option<PathBuf> {
    PACKAGE_READ_CONTEXT.with(|slot| slot.borrow().as_ref().map(|context| context.root.clone()))
}

/// Record one authority-checked file in the existing compile-time input
/// stream. Repeated reads of one path keep the first hash; a changed handle
/// is rejected by the authority resolver before this function is called.
pub fn record_package_input(path: impl Into<String>, hash: impl Into<String>) {
    PACKAGE_READ_CONTEXT.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(context) = slot.as_mut() else {
            return;
        };
        let input = ComptimeInput {
            path: path.into(),
            hash: hash.into(),
        };
        if !context.inputs.iter().any(|existing| existing.path == input.path) {
            context.inputs.push(input);
        }
    });
}

/// Install ambient hooks for the duration of `body`, then clear them.
pub fn with_ambient<R>(
    core_call: Option<AmbientCoreCall>,
    handle: Option<AmbientHandle>,
    extern_call: Option<AmbientExternCall>,
    body: impl FnOnce() -> R,
) -> R {
    CORE_CALL.with(|slot| slot.set(core_call));
    HANDLE.with(|slot| slot.set(handle));
    EXTERN_CALL.with(|slot| slot.set(extern_call));
    let out = body();
    CORE_CALL.with(|slot| slot.set(None));
    HANDLE.with(|slot| slot.set(None));
    EXTERN_CALL.with(|slot| slot.set(None));
    out
}

/// Copy the current callbacks into a worker thread before evaluating a
/// runtime fragment. The callbacks are function pointers, so this preserves
/// the ambient authority without sharing mutable host state.
pub fn ambient_hooks() -> (
    Option<AmbientCoreCall>,
    Option<AmbientHandle>,
    Option<AmbientExternCall>,
) {
    (
        CORE_CALL.with(|slot| slot.get()),
        HANDLE.with(|slot| slot.get()),
        EXTERN_CALL.with(|slot| slot.get()),
    )
}

pub fn try_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    try_core_call_typed(module, method, args, span, None)
}

pub fn try_core_call_typed(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    resolved_ret: Option<Type>,
) -> Option<Result<CtValue, Diagnostic>> {
    try_core_call_typed_with_sink(module, method, args, span, resolved_ret, None)
}

pub fn try_core_call_typed_with_sink(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    resolved_ret: Option<Type>,
    sink: Option<&mut DevSink>,
) -> Option<Result<CtValue, Diagnostic>> {
    CORE_CALL
        .with(|slot| slot.get())
        .and_then(|hook| hook(module, method, args, span, resolved_ret, sink))
}

pub fn try_handle(
    op: &str,
    recv: &mut CtValue,
    args: &mut [CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    HANDLE
        .with(|slot| slot.get())
        .and_then(|hook| hook(op, recv, args, span))
}

pub fn try_extern_call(
    wrapper: &str,
    args: Vec<CtValue>,
    span: Span,
    resolved_ret: Option<Type>,
) -> Option<Result<CtValue, Diagnostic>> {
    EXTERN_CALL
        .with(|slot| slot.get())
        .and_then(|hook| hook(wrapper, args, span, resolved_ret))
}
