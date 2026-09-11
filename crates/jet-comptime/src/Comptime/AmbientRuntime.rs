//! Optional host callbacks for whole-program interpreter deopt (`jet run`).
//!
//! Cranelift hosts for `core.db` / `core.crypto` live in `jet-jit` (rusqlite +
//! bridge crypto). Pure comptime / REPL/dev entry points leave this unset so
//! those modules stay unsupported or REPL-native-denied. `jet-jit` installs
//! hooks only around `MirBridge::run_bundle` for runtime-tier deopt.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::Comptime::DevSink;
use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::ComptimeInput;
use crate::AST::{CtValue, Type};
use crate::MIR::{
    MirCoreClosureKind, MirForeign, MirPreludeCallId, MirRuntimeValue, MirSiteId,
};

pub type AmbientCoreCall = fn(
    &str,
    &str,
    Vec<CtValue>,
    Span,
    Option<Type>,
    Option<&mut DevSink>,
) -> Option<Result<CtValue, Diagnostic>>;
/// Typed bridge for closure-taking Core routes.
///
/// The evaluator supplies the checked route, closure kind, canonical MIR
/// operands, and the opaque closure value. Adapters may marshal those facts to
/// a host representation, but they must not reinterpret the source spelling.
pub type AmbientCoreClosureCall = fn(
    &str,
    &str,
    MirPreludeCallId,
    MirCoreClosureKind,
    Vec<MirRuntimeValue>,
    Option<MirRuntimeValue>,
    MirSiteId,
    &str,
    Span,
) -> Option<Result<MirRuntimeValue, Diagnostic>>;
pub type AmbientHandle =
    fn(&str, &mut CtValue, &mut [CtValue], Span) -> Option<Result<CtValue, Diagnostic>>;
pub type AmbientExternCall =
    fn(&str, Vec<CtValue>, Span, Option<Type>) -> Option<Result<CtValue, Diagnostic>>;

/// Host implementation for a checked MIR closure that must run after a
/// semantic Core adapter has retained it. The wrapper is stored in `CtOpaque`;
/// the visible closure remains a normal `CtValue::Closure`.
pub trait StandaloneClosureHost: Send + Sync {
    fn invoke(&self, args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic>;

    /// Mutable callback variant used by transaction adapters. Hosts that do
    /// not mutate arguments inherit the ordinary call result; a MIR host
    /// overrides this to return the post-call argument frame.
    fn invoke_mut(
        &self,
        args: &mut Vec<CtValue>,
        span: Span,
    ) -> Result<CtValue, Diagnostic> {
        self.invoke(args.clone(), span)
    }
    /// Stable checked identity for this callback, including its relevant
    /// captured values. Hosts must provide this; opaque callbacks cannot
    /// silently fall back to process addresses or rendered debug text.
    fn history_callback_identity(&self) -> Result<String, String>;
}

#[derive(Clone)]
pub struct AmbientStandaloneClosure(Arc<dyn StandaloneClosureHost>);

impl AmbientStandaloneClosure {
    pub fn new<T>(host: T) -> Self
    where
        T: StandaloneClosureHost + 'static,
    {
        Self(Arc::new(host))
    }

    pub fn invoke(
        &self,
        args: Vec<CtValue>,
        span: Span,
    ) -> Result<CtValue, Diagnostic> {
        self.0.invoke(args, span)
    }

    pub fn invoke_mut(
        &self,
        args: &mut Vec<CtValue>,
        span: Span,
    ) -> Result<CtValue, Diagnostic> {
        self.0.invoke_mut(args, span)
    }
    pub fn history_callback_identity(&self) -> Result<String, String> {
        self.0.history_callback_identity()
    }
}

/// Detect a MIR-backed standalone closure without changing ordinary AST
/// closure dispatch. `None` means that the value is not an ambient host
/// closure; `Some(Err(_))` preserves the host's actual callback failure.
pub fn try_ambient_standalone_closure(
    closure: &CtValue,
    args: Vec<CtValue>,
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    let CtValue::Closure(data) = closure else {
        return None;
    };
    let host = data
        .opaque
        .as_ref()?
        .downcast_ref::<AmbientStandaloneClosure>()?;
    Some(host.invoke(args, span))
}

pub fn try_ambient_standalone_closure_mut(
    closure: &CtValue,
    args: &mut Vec<CtValue>,
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    let CtValue::Closure(data) = closure else {
        return None;
    };
    let host = data
        .opaque
        .as_ref()?
        .downcast_ref::<AmbientStandaloneClosure>()?;
    Some(host.invoke_mut(args, span))
}

/// Result of a typed MIR handle operation. Opaque handles remain private to
/// the adapter and never masquerade as a canonical MIR integer.
#[derive(Debug, Clone, PartialEq)]
pub enum AmbientMirHandleResult {
    Value(MirRuntimeValue),
    Handle(i64),
}

/// Typed interpreter carrier operations that must not cross the CtValue
/// boundary. The raw token is private to the resident adapter; `args` and the
/// result remain canonical MIR values.
pub type AmbientMirHandle = fn(
    &str,
    Option<i64>,
    Vec<MirRuntimeValue>,
    Span,
) -> Option<Result<AmbientMirHandleResult, Diagnostic>>;

/// Native foreign callback for MIR execution. The selected foreign row carries
/// the checked symbol, ABI, target applicability, and link/callback identity;
/// the adapter receives only that row and runtime values.
/// Native foreign callback for canonical MIR execution. The selected foreign
/// row carries the checked symbol, ABI, target applicability, and link/callback
/// identity; the adapter receives only that row and canonical MIR values.
pub type AmbientMirExternCall = fn(
    &MirForeign,
    Vec<MirRuntimeValue>,
    Span,
) -> Option<Result<MirRuntimeValue, Diagnostic>>;
thread_local! {
    static CORE_CALL: Cell<Option<AmbientCoreCall>> = const { Cell::new(None) };
    static CORE_CLOSURE_CALL: Cell<Option<AmbientCoreClosureCall>> = const { Cell::new(None) };
    static HANDLE: Cell<Option<AmbientHandle>> = const { Cell::new(None) };
    static EXTERN_CALL: Cell<Option<AmbientExternCall>> = const { Cell::new(None) };
    static MIR_HANDLE_CALL: Cell<Option<AmbientMirHandle>> = const { Cell::new(None) };
    static MIR_EXTERN_CALL: Cell<Option<AmbientMirExternCall>> = const { Cell::new(None) };
    static PACKAGE_READ_CONTEXT: RefCell<Option<PackageReadContext>> = const { RefCell::new(None) };
}
struct AmbientHooksGuard {
    core_call: Option<AmbientCoreCall>,
    handle: Option<AmbientHandle>,
    extern_call: Option<AmbientExternCall>,
}

impl Drop for AmbientHooksGuard {
    fn drop(&mut self) {
        CORE_CALL.with(|slot| slot.set(self.core_call));
        HANDLE.with(|slot| slot.set(self.handle));
        EXTERN_CALL.with(|slot| slot.set(self.extern_call));
    }
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
pub fn with_package_read_context<R>(
    root: &Path,
    body: impl FnOnce() -> R,
) -> (R, Vec<ComptimeInput>) {
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
                if !parent
                    .inputs
                    .iter()
                    .any(|existing| existing.path == input.path)
                {
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
        if !context
            .inputs
            .iter()
            .any(|existing| existing.path == input.path)
        {
            context.inputs.push(input);
        }
    });
}

/// Install ambient hooks for the duration of `body`, then restore the hooks
/// that were active before this scope.
pub fn with_ambient<R>(
    core_call: Option<AmbientCoreCall>,
    handle: Option<AmbientHandle>,
    extern_call: Option<AmbientExternCall>,
    body: impl FnOnce() -> R,
) -> R {
    let _previous = AmbientHooksGuard {
        core_call: CORE_CALL.with(|slot| slot.replace(core_call)),
        handle: HANDLE.with(|slot| slot.replace(handle)),
        extern_call: EXTERN_CALL.with(|slot| slot.replace(extern_call)),
    };
    body()
}

struct AmbientCoreClosureGuard(Option<AmbientCoreClosureCall>);

impl Drop for AmbientCoreClosureGuard {
    fn drop(&mut self) {
        CORE_CLOSURE_CALL.with(|slot| slot.set(self.0));
    }
}

/// Install the typed closure-taking Core bridge for the duration of `body`.
pub fn with_ambient_core_closure<R>(
    core_closure_call: Option<AmbientCoreClosureCall>,
    body: impl FnOnce() -> R,
) -> R {
    let _previous =
        AmbientCoreClosureGuard(CORE_CLOSURE_CALL.with(|slot| slot.replace(core_closure_call)));
    body()
}

/// Dispatch one checked closure-taking Core route to the active adapter.
pub fn try_ambient_core_closure(
    module: &str,
    method: &str,
    call: MirPreludeCallId,
    kind: MirCoreClosureKind,
    args: Vec<MirRuntimeValue>,
    closure: Option<MirRuntimeValue>,
    site: MirSiteId,
    label: &str,
    span: Span,
) -> Option<Result<MirRuntimeValue, Diagnostic>> {
    CORE_CLOSURE_CALL.with(|slot| {
        slot.get().and_then(|hook| {
            hook(
                module, method, call, kind, args, closure, site, label, span,
            )
        })
    })
}

struct AmbientMirExternGuard(Option<AmbientMirExternCall>);

impl Drop for AmbientMirExternGuard {
    fn drop(&mut self) {
        MIR_EXTERN_CALL.with(|slot| slot.set(self.0));
    }
}

/// Install the canonical MIR foreign callback for the duration of `body`.
/// This scope is separate from the legacy CtValue callback so callers can
/// migrate without changing the established interpreter hook transport.
pub fn with_ambient_mir_extern<R>(
    mir_extern_call: Option<AmbientMirExternCall>,
    body: impl FnOnce() -> R,
) -> R {
    let _previous = AmbientMirExternGuard(MIR_EXTERN_CALL.with(|slot| slot.replace(mir_extern_call)));
    body()
}

/// Return the callback currently installed for canonical MIR foreign calls.
pub fn ambient_mir_extern_hook() -> Option<AmbientMirExternCall> {
    MIR_EXTERN_CALL.with(|slot| slot.get())
}

struct AmbientMirHandleGuard(Option<AmbientMirHandle>);

impl Drop for AmbientMirHandleGuard {
    fn drop(&mut self) {
        MIR_HANDLE_CALL.with(|slot| slot.set(self.0));
    }
}

/// Install typed MIR handle operations for the duration of `body`.
pub fn with_ambient_mir_handle<R>(
    mir_handle: Option<AmbientMirHandle>,
    body: impl FnOnce() -> R,
) -> R {
    let _previous =
        AmbientMirHandleGuard(MIR_HANDLE_CALL.with(|slot| slot.replace(mir_handle)));
    body()
}

/// Return the callback currently installed for canonical MIR handle operations.
pub fn try_ambient_mir_handle(
    operation: &str,
    handle: Option<i64>,
    args: Vec<MirRuntimeValue>,
    span: Span,
) -> Option<Result<AmbientMirHandleResult, Diagnostic>> {
    MIR_HANDLE_CALL
        .with(|slot| slot.get())
        .and_then(|hook| hook(operation, handle, args, span))
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


/// Invoke the selected canonical MIR foreign callback, if one is installed.
pub fn try_mir_extern_call(
    foreign: &MirForeign,
    args: Vec<MirRuntimeValue>,
    span: Span,
) -> Option<Result<MirRuntimeValue, Diagnostic>> {
    MIR_EXTERN_CALL
        .with(|slot| slot.get())
        .and_then(|hook| hook(foreign, args, span))
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    fn outer_core(
        _: &str,
        _: &str,
        _: Vec<CtValue>,
        _: Span,
        _: Option<Type>,
        _: Option<&mut DevSink>,
    ) -> Option<Result<CtValue, Diagnostic>> {
        None
    }

    fn inner_core(
        _: &str,
        _: &str,
        _: Vec<CtValue>,
        _: Span,
        _: Option<Type>,
        _: Option<&mut DevSink>,
    ) -> Option<Result<CtValue, Diagnostic>> {
        None
    }

    fn outer_handle(
        _: &str,
        _: &mut CtValue,
        _: &mut [CtValue],
        _: Span,
    ) -> Option<Result<CtValue, Diagnostic>> {
        None
    }

    fn outer_extern(
        _: &str,
        _: Vec<CtValue>,
        _: Span,
        _: Option<Type>,
    ) -> Option<Result<CtValue, Diagnostic>> {
        None
    }

    #[test]
    fn nested_ambient_scopes_restore_previous_hooks() {
        assert_eq!(ambient_hooks(), (None, None, None));
        let outer = (
            Some(outer_core as AmbientCoreCall),
            Some(outer_handle as AmbientHandle),
            Some(outer_extern as AmbientExternCall),
        );
        let inner = (Some(inner_core as AmbientCoreCall), None, None);

        with_ambient(
            Some(outer_core),
            Some(outer_handle),
            Some(outer_extern),
            || {
                assert_eq!(ambient_hooks(), outer);

                with_ambient(Some(inner_core), None, None, || {
                    assert_eq!(ambient_hooks(), inner);
                });
                assert_eq!(ambient_hooks(), outer);

                let unwound = catch_unwind(AssertUnwindSafe(|| {
                    with_ambient(None, Some(outer_handle), None, || {
                        panic!("ambient scope unwind");
                    });
                }));
                assert!(unwound.is_err());
                assert_eq!(ambient_hooks(), outer);
            },
        );

        assert_eq!(ambient_hooks(), (None, None, None));
    }
}
