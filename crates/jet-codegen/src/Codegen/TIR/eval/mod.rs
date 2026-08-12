//! Canonical TIR evaluator — reference semantics (D-ONECORE1=A / #777).

mod builtins;
mod browser;
mod closure_ops;
mod compute_calls;
mod data_calls;
mod event_ops;
mod exprs;
mod handles;
mod local_cell;
mod regex_ops;
mod services_calls;
mod stmts;
mod stream;
mod webapp;

mod range_semantics {
    use jet_foundation::StructuralDebug::jet_debug_range;
    include!("../../../Prelude/Core/RangeBounds.rs");
}

mod disjoint_semantics {
    include!("../../../Prelude/Core/Disjoint.rs");

    pub(super) fn split(
        len: usize,
        mid: i64,
    ) -> Result<((usize, usize), (usize, usize)), String> {
        jet_disjoint_split_bounds(len, mid)
    }

    pub(super) fn indexes(
        len: usize,
        indices: &[i64],
    ) -> Result<Vec<(usize, usize, usize)>, String> {
        jet_disjoint_index_bounds(len, indices)
    }
}

#[allow(dead_code)]
mod interrupt_queue {
    include!("../../../Prelude/CoreLib/Top/Interrupt.rs");
}

#[allow(dead_code)]
mod uninit_semantics {
    include!("../../../Prelude/Uninit.rs");
}

#[allow(dead_code)]
mod shared_protocol {
    include!("../../../Prelude/SharedProtocol.rs");
}

use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::AST::{Expr, Func, ProgramBundle, Stmt, Type};
use crate::Codegen::mangle;
use super::Cx;
use crate::Codegen::TIR::{
    self, JitProgram, LowerEnv, TExpr, TExprKind, TFunc, TJitSpawnBody, TJitSpawnLambda, TLocal,
    TStmt,
};
use super::build_cx_items;
use crate::Comptime::{self, CtReport, CtValue, DevSink};
use crate::Diagnostics::{Diagnostic, Span};

/// Cross-tier hook: Cranelift-native functions callable from the TIR evaluator (#778).
pub type NativeCallHook = fn(&str, &[CtValue]) -> Option<Result<CtValue, Diagnostic>>;

thread_local! {
    static NATIVE_CALL_HOOK: Cell<Option<NativeCallHook>> = const { Cell::new(None) };
}

pub fn set_native_call_hook(hook: Option<NativeCallHook>) {
    NATIVE_CALL_HOOK.with(|slot| slot.set(hook));
}

pub(super) fn native_call_hook() -> Option<NativeCallHook> {
    NATIVE_CALL_HOOK.with(Cell::get)
}

static INTERPRETER_INTERRUPT_QUEUE: interrupt_queue::JetInterruptQueue =
    interrupt_queue::JetInterruptQueue::new();
static INTERPRETER_INTERRUPT_HANDLER: OnceLock<Result<(), String>> = OnceLock::new();

fn note_interpreter_interrupt() {
    INTERPRETER_INTERRUPT_QUEUE.note();
}

#[cfg(unix)]
extern "C" fn interpreter_unix_mark(_: i32) {
    note_interpreter_interrupt();
}

fn install_interpreter_interrupt_handler() -> Result<(), String> {
    #[cfg(unix)]
    {
        return interrupt_queue::jet_interrupt_install_unix_handler(interpreter_unix_mark);
    }
    #[cfg(windows)]
    {
        unsafe extern "system" fn mark(kind: u32) -> i32 {
            if kind == 0 {
                note_interpreter_interrupt();
                1
            } else {
                0
            }
        }
        return interrupt_queue::jet_interrupt_install_windows_handler(Some(mark));
    }
    #[cfg(not(any(unix, windows)))]
    {
        Err(interrupt_queue::jet_interrupt_unavailable_error().to_string())
    }
}

fn ensure_interpreter_interrupt_handler() -> Result<(), String> {
    match INTERPRETER_INTERRUPT_HANDLER.get_or_init(install_interpreter_interrupt_handler) {
        Ok(()) => Ok(()),
        Err(message) => Err(message.clone()),
    }
}

pub(super) fn raw_place_local(expr: &TExpr) -> Option<&TLocal> {
    match &expr.kind {
        TIR::TExprKind::Local(local) => Some(local),
        TIR::TExprKind::Borrow { place, .. } => raw_place_local(place),
        TIR::TExprKind::DistinctCtor { arg, .. } => raw_place_local(arg),
        _ => None,
    }
}

pub use exprs::{stable_place_address, tir_place_address_key};

pub(super) fn unsupported(what: &str, span: Span) -> Diagnostic {
    Diagnostic::e0956_unsupported(what, span)
}

/// D-CONC-FAIL1=A: the evaluator turns a child diagnostic into the same
/// normal TaskFailure enum that the shared scheduler reports. A diagnostic
/// raised by the joining parent remains a control diagnostic and is not
/// converted here.
fn task_failure_value(error: &Diagnostic) -> CtValue {
    let failure = crate::task_group::jet_task_failure_from_code(error.code.as_str(), error.what.clone());
    let (variant, args) = match failure {
        crate::task_group::JetTaskFailure::Cancelled => ("Cancelled", Vec::new()),
        crate::task_group::JetTaskFailure::DeadlineBlown => ("DeadlineBlown", Vec::new()),
        crate::task_group::JetTaskFailure::Panicked(reason) => (
            "Panicked",
            vec![(None, CtValue::Str(reason))],
        ),
    };
    CtValue::Enum {
        type_name: crate::Syntax::TYPE_TASK_FAILURE.to_string(),
        variant: variant.to_string(),
        args,
    }
}

fn task_child_panic(message: String, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0953",
        message,
        "a child task panicked".to_string(),
        String::new(),
        Some(span),
    )
}

pub(super) fn reborrow_repl_authorizer<'short, 'long: 'short>(
    authorizer: &'short mut Option<&'long mut dyn Comptime::ReplAuthorizer>,
) -> Option<&'short mut (dyn Comptime::ReplAuthorizer + 'short)> {
    match authorizer {
        Some(authorizer) => Some(&mut **authorizer),
        None => None,
    }
}

pub(super) fn progress_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

pub(super) fn progress_elapsed(started_at: f64) -> f64 {
    (progress_now() - started_at).max(0.0)
}

pub(super) fn progress_no_color() -> bool {
    std::env::var_os("NO_COLOR").is_some()
}

pub(super) fn progress_emit(
    sink: Option<&Arc<Mutex<DevSink>>>,
    text: &str,
) {
    use std::io::IsTerminal;

    let tty = std::io::stdout().is_terminal();
    if let Some(sink) = sink {
        let mut sink = sink.lock().expect("evaluator sink poisoned");
        if tty {
            sink.stdout.push('\r');
        }
        sink.stdout.push_str(text);
        if !tty {
            sink.stdout.push('\n');
        }
    }
}

pub(super) fn progress_source_has_exact_total(expr: &TExpr) -> bool {
    matches!(
        &expr.kind,
        TIR::TExprKind::BuiltinMethod {
            op: TIR::TBuiltinOp::ListLazy,
            ..
        }
    )
}

pub(super) fn progress_iter_value(items: Vec<CtValue>, known_total: bool) -> CtValue {
    CtValue::Struct {
        type_name: "__JetIter".to_string(),
        fields: vec![
            ("items".to_string(), CtValue::List(items)),
            ("known_total".to_string(), CtValue::Bool(known_total)),
        ],
    }
}

pub(super) fn progress_iter_parts(value: &CtValue) -> Option<(Vec<CtValue>, bool)> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != "__JetIter" {
        return None;
    }
    let items = fields.iter().find_map(|(name, value)| {
        (name == "items").then(|| match value {
            CtValue::List(items) => Some(items.clone()),
            _ => None,
        })
    })??;
    let known_total = fields
        .iter()
        .find_map(|(name, value)| {
            (name == "known_total").then(|| match value {
                CtValue::Bool(value) => Some(*value),
                _ => None,
            })
        })
        .flatten()
        .unwrap_or(true);
    Some((items, known_total))
}

pub(super) fn range_window(
    value: &CtValue,
    len: usize,
    span: Span,
) -> Result<(i64, i64), Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("Range window", span));
    };
    if type_name != crate::Syntax::TYPE_RANGE {
        return Err(unsupported("Range window type", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value)
    };
    let start = match field("start") {
        Some(CtValue::Int(value)) => *value,
        _ => return Err(unsupported("Range.start", span)),
    };
    let end = match field("end") {
        Some(CtValue::Int(value)) => *value,
        _ => return Err(unsupported("Range.end", span)),
    };
    let exclusive = match field("exclusive") {
        Some(CtValue::Bool(value)) => *value,
        _ => return Err(unsupported("Range.exclusive", span)),
    };
    checked_view_window(start, end, exclusive, len, span)
}

pub(super) fn checked_view_window(
    start: i64,
    end: i64,
    exclusive: bool,
    len: usize,
    span: Span,
) -> Result<(i64, i64), Diagnostic> {
    range_semantics::jet_checked_view_bounds(start, end, exclusive, len as i64)
        .map_err(|message| view_bounds_diagnostic(message, span))
}

pub(super) fn view_bounds_diagnostic(
    message: String,
    span: Span,
) -> Diagnostic {
    // Same E0953 voice as the JIT trap / comptime panic path so every tier
    // reports one code for an out-of-bounds view (I9).
    Diagnostic::error(
        "E0953",
        "your comptime code stopped the build".to_string(),
        format!("while computing this value at compile time, the program panicked: {message}"),
        "this is the sanctioned way to validate at compile time — fix the input the check rejects"
            .to_string(),
        Some(span),
    )
}

fn enter_source_nesting(depth: &mut usize, span: Span) -> Result<(), Diagnostic> {
    *depth += 1;
    if *depth <= crate::Diagnostics::MAX_SOURCE_NESTING {
        return Ok(());
    }
    let exceeded = *depth;
    *depth -= 1;
    Err(Diagnostic::source_nesting_exceeded(exceeded, span))
}

pub(super) fn range_contains(
    value: &CtValue,
    needle: &CtValue,
    span: Span,
) -> Result<bool, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("Range.contains receiver", span));
    };
    if type_name != crate::Syntax::TYPE_RANGE {
        return Err(unsupported("Range.contains receiver", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value)
    };
    let (Some(CtValue::Int(start)), Some(CtValue::Int(end)), CtValue::Int(needle)) =
        (field("start"), field("end"), needle)
    else {
        return Err(unsupported("Range.contains arguments", span));
    };
    let exclusive = matches!(field("exclusive"), Some(CtValue::Bool(true)));
    Ok(range_semantics::jet_range_contains(
        *start, *end, exclusive, *needle,
    ))
}

const UNINIT_FIXED_CARRIER: &str = "__JetUninitFixed";

pub(super) fn uninit_fixed_carrier(len: usize) -> CtValue {
    CtValue::Struct {
        type_name: UNINIT_FIXED_CARRIER.to_string(),
        fields: vec![
            (
                "values".to_string(),
                CtValue::List(vec![CtValue::Unit; len]),
            ),
            (
                "initialized".to_string(),
                CtValue::List(
                    uninit_semantics::jet_uninit_bitmap(len)
                        .into_iter()
                        .map(CtValue::Bool)
                        .collect(),
                ),
            ),
        ],
    }
}

pub(super) fn uninit_fixed_read(value: &CtValue, index: usize) -> Option<CtValue> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != UNINIT_FIXED_CARRIER {
        return None;
    }
    let CtValue::List(values) = &fields.iter().find(|(name, _)| name == "values")?.1 else {
        return None;
    };
    let CtValue::List(initialized) = &fields
        .iter()
        .find(|(name, _)| name == "initialized")?
        .1
    else {
        return None;
    };
    let bitmap = initialized
        .iter()
        .map(|value| matches!(value, CtValue::Bool(true)))
        .collect::<Vec<_>>();
    let index = uninit_semantics::jet_uninit_read(&bitmap, index).ok()?;
    values.get(index).cloned()
}

pub(super) fn uninit_fixed_materialize(value: &CtValue) -> Option<CtValue> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != UNINIT_FIXED_CARRIER {
        return None;
    }
    let CtValue::List(values) = &fields.iter().find(|(name, _)| name == "values")?.1 else {
        return None;
    };
    let CtValue::List(initialized) = &fields
        .iter()
        .find(|(name, _)| name == "initialized")?
        .1
    else {
        return None;
    };
    let bitmap = initialized
        .iter()
        .map(|value| matches!(value, CtValue::Bool(true)))
        .collect::<Vec<_>>();
    uninit_semantics::jet_uninit_all(&bitmap).ok()?;
    Some(CtValue::List(values.clone()))
}

pub(super) fn uninit_fixed_write(
    value: &mut CtValue,
    index: usize,
    replacement: CtValue,
) -> bool {
    let CtValue::Struct { type_name, fields } = value else {
        return false;
    };
    if type_name != UNINIT_FIXED_CARRIER {
        return false;
    }
    let Some(values_index) = fields.iter().position(|(name, _)| name == "values") else {
        return false;
    };
    let Some(initialized_index) = fields
        .iter()
        .position(|(name, _)| name == "initialized")
    else {
        return false;
    };
    let CtValue::List(initialized) = &mut fields[initialized_index].1 else {
        return false;
    };
    let mut bitmap = initialized
        .iter()
        .map(|value| matches!(value, CtValue::Bool(true)))
        .collect::<Vec<_>>();
    let Ok((index, _)) = uninit_semantics::jet_uninit_write(&mut bitmap, index) else {
        return false;
    };
    *initialized = bitmap.into_iter().map(CtValue::Bool).collect();
    let CtValue::List(values) = &mut fields[values_index].1 else {
        return false;
    };
    values[index] = replacement;
    true
}

/// D-MEM1 S9 / D-PIN1=A: marker for a whole-place write window (`p :: &node`,
/// `pinned :: mem.pin(&node)`). AOT and Cranelift both give the local a real
/// exclusive reference to the owner's storage, so the interpreter has to alias
/// too — storing the copied value would silently drop every edit made through
/// the window (I9). The handle carries the owner local plus the field/index
/// path, exactly like `__JetViewMut` does for range windows.
pub(super) const PLACE_MUT_TYPE: &str = "__JetPlaceMut";

pub(super) fn place_mut_handle(base: &str, path: &[ViewMutPathStep]) -> CtValue {
    CtValue::Struct {
        type_name: PLACE_MUT_TYPE.into(),
        fields: vec![
            ("base".into(), CtValue::Str(base.to_string())),
            ("path".into(), encode_view_mut_path(path)),
        ],
    }
}

pub(super) fn place_mut_parts(value: &CtValue) -> Option<(String, Vec<ViewMutPathStep>)> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != PLACE_MUT_TYPE {
        return None;
    }
    let base = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("base", CtValue::Str(base)) => Some(base.clone()),
        _ => None,
    })?;
    Some((base, parse_view_mut_path(fields)))
}

/// Read the value the handle windows into, or `None` when the owner is gone.
pub(super) fn read_place_mut(
    value: &CtValue,
    scope: &HashMap<String, CtValue>,
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    let (base, path) = place_mut_parts(value)?;
    let Some(root) = scope.get(&base) else {
        return Some(Err(unsupported("place window owner", span)));
    };
    Some(project_list_place(root, &path, span).cloned())
}

/// Write through the handle into the owner's storage.
pub(super) fn write_place_mut(
    handle: &CtValue,
    replacement: CtValue,
    scope: &mut HashMap<String, CtValue>,
    span: Span,
) -> Option<Result<(), Diagnostic>> {
    let (base, path) = place_mut_parts(handle)?;
    let Some(root) = scope.get(&base).cloned() else {
        return Some(Err(unsupported("place window owner", span)));
    };
    Some(
        replace_list_place(root, &path, replacement, span).map(|updated| {
            scope.insert(base, updated);
        }),
    )
}

/// One step from a root local to the list a `__JetViewMut` windows into.
#[derive(Clone, Debug)]
pub(super) enum ViewMutPathStep {
    Field(String),
    Index(i64),
}

/// Recover the complete owner place carried by a view expression. The caller
/// supplies index evaluation so dynamic projected indexes retain their TIR
/// provenance instead of being reduced to a bare root local.
pub(super) fn view_mut_place(
    expr: &TExpr,
    resolve_index: &mut impl FnMut(&TExpr) -> Result<i64, Diagnostic>,
) -> Result<Option<(String, Vec<ViewMutPathStep>)>, Diagnostic> {
    match &expr.kind {
        TExprKind::Local(local) => Ok(Some((local.name.clone(), Vec::new()))),
        TExprKind::Borrow { place, .. } | TExprKind::Deref(place) => {
            view_mut_place(place, resolve_index)
        }
        TExprKind::Field { recv, field, .. } => {
            let Some((base, mut path)) = view_mut_place(recv, resolve_index)? else {
                return Ok(None);
            };
            path.push(ViewMutPathStep::Field(field.clone()));
            Ok(Some((base, path)))
        }
        TExprKind::Index {
            base,
            index,
            is_map: false,
            ..
        } => {
            let Some((root, mut path)) = view_mut_place(base, resolve_index)? else {
                return Ok(None);
            };
            path.push(ViewMutPathStep::Index(resolve_index(index)?));
            Ok(Some((root, path)))
        }
        _ => Ok(None),
    }
}

pub(super) fn parse_view_mut_path(fields: &[(String, CtValue)]) -> Vec<ViewMutPathStep> {
    let Some((_, CtValue::List(steps))) = fields.iter().find(|(name, _)| name == "path") else {
        return Vec::new();
    };
    steps
        .iter()
        .filter_map(|step| {
            let CtValue::Str(s) = step else {
                return None;
            };
            if let Some(field) = s.strip_prefix("f:") {
                Some(ViewMutPathStep::Field(field.to_string()))
            } else if let Some(index) = s.strip_prefix("i:") {
                index.parse().ok().map(ViewMutPathStep::Index)
            } else {
                None
            }
        })
        .collect()
}

pub(super) fn encode_view_mut_path(path: &[ViewMutPathStep]) -> CtValue {
    CtValue::List(
        path.iter()
            .map(|step| {
                CtValue::Str(match step {
                    ViewMutPathStep::Field(field) => format!("f:{field}"),
                    ViewMutPathStep::Index(index) => format!("i:{index}"),
                })
            })
            .collect(),
    )
}

pub(super) fn view_mut_parts(
    fields: &[(String, CtValue)],
) -> Option<(String, Vec<ViewMutPathStep>, i64, i64)> {
    let mut base = None;
    let mut start = None;
    let mut end = None;
    for (name, value) in fields {
        match (name.as_str(), value) {
            ("base", CtValue::Str(s)) => base = Some(s.clone()),
            ("start", CtValue::Int(n)) => start = Some(*n),
            ("end", CtValue::Int(n)) => end = Some(*n),
            _ => {}
        }
    }
    Some((base?, parse_view_mut_path(fields), start?, end?))
}

pub(super) fn project_list_place<'a>(
    root: &'a CtValue,
    path: &[ViewMutPathStep],
    span: Span,
) -> Result<&'a CtValue, Diagnostic> {
    let mut cur = root;
    for step in path {
        cur = match (step, cur) {
            (ViewMutPathStep::Field(field), CtValue::Struct { fields, .. }) => fields
                .iter()
                .find(|(name, _)| {
                    name == field
                        || name.strip_prefix(crate::Syntax::GENERATED_NAME_PREFIX) == Some(field.as_str())
                        || name == &crate::Codegen::mangle(field)
                })
                .map(|(_, value)| value)
                .ok_or_else(|| unsupported("view-mut path field", span))?,
            (ViewMutPathStep::Index(index), CtValue::List(items)) => {
                if *index < 0 || *index as usize >= items.len() {
                    return Err(unsupported("view-mut path index", span));
                }
                &items[*index as usize]
            }
            _ => return Err(unsupported("view-mut path", span)),
        };
    }
    Ok(cur)
}

pub(super) fn replace_list_place(
    root: CtValue,
    path: &[ViewMutPathStep],
    replacement: CtValue,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    if path.is_empty() {
        return Ok(replacement);
    }
    let step = &path[0];
    let rest = &path[1..];
    match (step, root) {
        (ViewMutPathStep::Field(field), CtValue::Struct { type_name, mut fields }) => {
            let mangled = crate::Codegen::mangle(field);
            let slot = fields.iter_mut().find(|(name, _)| {
                name == field
                    || name.strip_prefix(crate::Syntax::GENERATED_NAME_PREFIX) == Some(field.as_str())
                    || name == &mangled
            });
            let Some((_, value)) = slot else {
                return Err(unsupported("view-mut path field", span));
            };
            *value = replace_list_place(value.clone(), rest, replacement, span)?;
            Ok(CtValue::Struct { type_name, fields })
        }
        (ViewMutPathStep::Index(index), CtValue::List(mut items)) => {
            if *index < 0 || *index as usize >= items.len() {
                return Err(unsupported("view-mut path index", span));
            }
            let i = *index as usize;
            items[i] = replace_list_place(items[i].clone(), rest, replacement, span)?;
            Ok(CtValue::List(items))
        }
        _ => Err(unsupported("view-mut path", span)),
    }
}

pub(super) fn view_mut_window_args(fields: &[(String, CtValue)]) -> Option<&[CtValue]> {
    if !crate::Comptime::ComputeLite::tensor_window_is_live(fields) {
        return None;
    }
    fields.iter().find_map(|(name, value)| {
        (name == "window").then(|| match value {
            CtValue::List(args) => Some(args.as_slice()),
            _ => None,
        })
    }).flatten()
}

pub(super) fn view_mut_owner_value(
    fields: &[(String, CtValue)],
    scope: &HashMap<String, CtValue>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    if !crate::Comptime::ComputeLite::tensor_window_is_live(fields) {
        return Err(unsupported("Tensor view window", span));
    }
    let (base, path, _, _) =
        view_mut_parts(fields).ok_or_else(|| unsupported("view-mut fields", span))?;
    let root = scope
        .get(&base)
        .ok_or_else(|| unsupported("view-mut owner", span))?;
    project_list_place(root, &path, span).cloned()
}

pub(super) fn store_view_mut_owner_value(
    fields: &[(String, CtValue)],
    scope: &mut HashMap<String, CtValue>,
    replacement: CtValue,
    span: Span,
) -> Result<(), Diagnostic> {
    let (base, path, _, _) =
        view_mut_parts(fields).ok_or_else(|| unsupported("view-mut fields", span))?;
    let root = scope
        .get(&base)
        .cloned()
        .ok_or_else(|| unsupported("view-mut owner", span))?;
    let updated = replace_list_place(root, &path, replacement, span)?;
    scope.insert(base, updated);
    Ok(())
}

pub(super) fn load_view_mut_owner_list(
    fields: &[(String, CtValue)],
    scope: &HashMap<String, CtValue>,
    span: Span,
) -> Result<Vec<CtValue>, Diagnostic> {
    let (base, path, _, _) =
        view_mut_parts(fields).ok_or_else(|| unsupported("view-mut fields", span))?;
    let root = scope
        .get(&base)
        .ok_or_else(|| unsupported("view-mut owner", span))?;
    let owner = project_list_place(root, &path, span)?;
    match owner {
        CtValue::List(items) => Ok(items.clone()),
        CtValue::Struct {
            type_name,
            fields: _,
        }
            if type_name == "Tensor" || type_name == "JetTensor" =>
        {
            match crate::Comptime::ComputeLite::tensor_to_list_value(
                owner,
                span,
            )? {
                CtValue::List(items) => Ok(items),
                _ => Err(unsupported("Tensor view owner data", span)),
            }
        }
        _ => Err(unsupported("view-mut owner list", span)),
    }
}

pub(super) fn store_view_mut_owner_list(
    fields: &[(String, CtValue)],
    scope: &mut HashMap<String, CtValue>,
    items: Vec<CtValue>,
    span: Span,
) -> Result<(), Diagnostic> {
    let (base, path, _, _) =
        view_mut_parts(fields).ok_or_else(|| unsupported("view-mut fields", span))?;
    let root = scope
        .get(&base)
        .cloned()
        .ok_or_else(|| unsupported("view-mut owner", span))?;
    let replacement = match project_list_place(&root, &path, span)? {
        CtValue::Struct { type_name, .. }
            if type_name == "Tensor" || type_name == "JetTensor" =>
        {
            crate::Comptime::ComputeLite::tensor_replace_data(
                project_list_place(&root, &path, span)?,
                items,
                span,
            )?
        }
        _ => CtValue::List(items),
    };
    let updated = replace_list_place(root, &path, replacement, span)?;
    scope.insert(base, updated);
    Ok(())
}

/// Resolve a `__JetViewMut { base, start, end }` handle to the inclusive window List.
pub(super) fn materialize_view_mut_window(
    fields: &[(String, CtValue)],
    scope: &HashMap<String, CtValue>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let (_, _, start, end) =
        view_mut_parts(fields).ok_or_else(|| unsupported("view-mut fields", span))?;
    let owner = view_mut_owner_value(fields, scope, span)?;
    if let Some(window) = view_mut_window_args(fields) {
        if matches!(&owner, CtValue::Struct { type_name, .. } if type_name == "Tensor" || type_name == "JetTensor") {
            return crate::Comptime::ComputeLite::tensor_view_list(&owner, window, span);
        }
    }
    let items = load_view_mut_owner_list(fields, scope, span)?;
    let (start, end_exclusive) = if end < start {
        if end.checked_add(1) != Some(start) {
            return Err(view_bounds_diagnostic(
                range_semantics::jet_view_bounds_error(
                    start,
                    end,
                    false,
                    items.len() as i64,
                ),
                span,
            ));
        }
        range_semantics::jet_checked_view_bounds(start, start, true, items.len() as i64)
            .map_err(|_| {
                view_bounds_diagnostic(
                    range_semantics::jet_view_bounds_error(
                        start,
                        end,
                        false,
                        items.len() as i64,
                    ),
                    span,
                )
            })?
    } else {
        checked_view_window(start, end, false, items.len(), span)?
    };
    Ok(CtValue::List(
        items[start as usize..end_exclusive as usize].to_vec(),
    ))
}

fn rebase_view_mut_owners(
    value: &mut CtValue,
    owners: &HashMap<String, String>,
) {
    match value {
        CtValue::Struct { type_name, fields } if type_name == "__JetViewMut" => {
            if let Some((_, CtValue::Str(base))) =
                fields.iter_mut().find(|(name, _)| name == "base")
            {
                if let Some(owner) = owners.get(base) {
                    *base = owner.clone();
                }
            }
        }
        CtValue::Struct { fields, .. } => {
            for (_, field) in fields {
                rebase_view_mut_owners(field, owners);
            }
        }
        CtValue::Enum { args, .. } => {
            for (_, arg) in args {
                rebase_view_mut_owners(arg, owners);
            }
        }
        CtValue::List(values) => {
            for value in values {
                rebase_view_mut_owners(value, owners);
            }
        }
        CtValue::Map(values) => {
            for value in values.values_mut() {
                rebase_view_mut_owners(value, owners);
            }
        }
        CtValue::Present(value) | CtValue::Failed(CtReport::Told(value)) => {
            rebase_view_mut_owners(value, owners);
        }
        _ => {}
    }
}

#[derive(Debug)]
pub(super) enum Flow {
    Normal,
    Break,
    Continue,
    BreakLabel(String),
    BreakValue(Option<String>, CtValue),
    ContinueLabel(String),
    Return(CtValue),
}

const DEV_FUEL: u64 = 1_000_000_000;
#[allow(dead_code)]
const CT_FUEL: u64 = 10_000_000;

pub(super) struct EvalCtx<'a> {
    pub(super) funcs: HashMap<String, &'a TFunc>,
    #[allow(dead_code)]
    pub(super) base_dir: PathBuf,
    pub(super) fuel: u64,
    pub(super) sink: Option<Arc<Mutex<DevSink>>>,
    #[allow(dead_code)]
    pub(super) core_imports: &'a HashMap<String, String>,
    pub(super) globals: HashMap<String, CtValue>,
    #[allow(dead_code)]
    pub(super) allow_impure: bool,
    #[allow(dead_code)]
    pub(super) impure_depth: usize,
    /// True only for an actual dev/JIT runtime execution. Comptime may permit
    /// explicit I/O, but it must never open a Browser session while compiling.
    pub(super) runtime_execution: bool,
    /// Keep calls inside a codec-sensitive named deopt on canonical TIR.
    pub(super) prefer_tir_calls: bool,
    pub(super) repl_mode: bool,
    /// Lexical REPL capabilities forwarded from the frontend. Authorization
    /// decisions remain in the shared Comptime host seam.
    pub(super) repl_grants: Vec<String>,
    pub(super) repl_authorizer: Option<&'a mut dyn Comptime::ReplAuthorizer>,
    pub(super) pending_return: Option<CtValue>,
    /// `defer close(^…)` exprs scheduled in the current eval frame (LIFO).
    pub(super) deferred_closes: Vec<&'a TExpr>,
    /// Control emitted by an inline loop expression that targets an enclosing
    /// loop. The containing statement list consumes and propagates it.
    pub(super) pending_flow: Option<Flow>,
    /// Compiler-private eager List sinks for raw comptime yielding-loop
    /// fragments. Fully checked programs rewrite these sends to `List.push`.
    pub(super) collecting_items: Vec<Vec<CtValue>>,
    pub(super) call_depth: usize,
    pub(super) source_nesting: usize,
    pub(super) current_span: Span,
    pub(super) emitted_fragments: Option<&'a mut Vec<String>>,
    pub(super) embed_inputs: Option<&'a mut Vec<crate::AST::ComptimeInput>>,
    /// `TypeName -> [(field, redact)]` for JetDebug formatting (D-DISPLAYDBG).
    pub(super) struct_fields: HashMap<String, Vec<(String, bool)>>,
    /// Canonical typeable paths for runtime reflection.
    pub(super) reflect_paths: HashMap<String, String>,
    /// `TypeName -> [(field, Type)]` for `core.data.csv` / decode on deopt.
    pub(super) struct_field_types: HashMap<String, Vec<(String, crate::AST::Type)>>,
    /// Sema-compiled published-schema plans. These contain wire keys and
    /// lowered helper names, never AST expressions or a field/type tuple codec.
    pub(super) codec_migrations: HashMap<String, TIR::TCodecMigrationPlan>,
    pub(super) distinct_bases: HashMap<String, crate::AST::Type>,
    pub(super) distinct_ranges: HashMap<String, (i64, i64)>,
    /// Current `MixedSwitch` subject for structured field conditions.
    switch_subject: Option<CtValue>,
    /// Handle-backed runtime state is shared by lexical task children. The TIR
    /// and local variable scopes remain borrowed by each evaluator context.
    runtime: Arc<Mutex<EvalRuntime<'a>>>,
    /// Thread-confined Cell values and loans. Never shared with spawned tasks.
    local_cells: local_cell::EvalLocalCells,
    shared_transactions: Vec<EvalSharedTransaction<'a>>,
    /// Spawn bodies are lowered separately because native tiers compile them as
    /// independent functions. The evaluator records each outcome behind a task
    /// handle, then observes it only at join/group boundaries.
    spawn_lambdas: &'a [TJitSpawnLambda],
    task_sender: Option<mpsc::Sender<EvalTaskJob<'a>>>,
    task_cancel: Option<Arc<AtomicBool>>,
    task_paused: Option<Arc<AtomicBool>>,
    pub(super) context_deadline: Option<i64>,
    pub(super) shield_depth: usize,
    /// Direct yield delivery keeps generator evaluation streaming: no eager
    /// collection is materialized between producer and consumer.
    yield_consumer: Option<YieldConsumer<'a>>,
    yield_scope: Option<HashMap<String, CtValue>>,
    /// `scope.guard` cleanups — run LIFO when the enclosing function returns.
    scope_guards: Vec<&'a TIR::TLambda>,
    /// Owned Shared lock leases acquired by this evaluator context.
    shared_guards: Vec<usize>,
    /// Nested `#Transact` frames for auto-snapshot + commit/rollback hooks.
    txn_stack: Vec<EvalTxnFrame<'a>>,
}

pub(super) struct EvalTxnFrame<'a> {
    pub(super) snapshots: Vec<(String, CtValue)>,
    pub(super) on_commit: Vec<&'a TIR::TLambda>,
    pub(super) on_rollback: Vec<&'a TIR::TLambda>,
}

pub(super) struct EvalSharedDelta<'a> {
    pub(super) shared_index: usize,
    pub(super) lambda: &'a TIR::TLambda,
    pub(super) captured: HashMap<String, CtValue>,
}

pub(super) struct EvalSharedTransaction<'a> {
    pub(super) transaction: shared_protocol::JetSharedTransaction,
    pub(super) deltas: Vec<EvalSharedDelta<'a>>,
}

impl<'a> EvalSharedTransaction<'a> {
    pub(super) fn new() -> Self {
        Self {
            transaction: shared_protocol::jet_shared_transaction_begin(),
            deltas: Vec::new(),
        }
    }
}

enum EvalCallable<'a> {
    Lambda {
        lambda: &'a TIR::TLambda,
        captured: HashMap<String, CtValue>,
    },
    Named(&'a str),
    ComputeTransform {
        base: CtValue,
        method: String,
        targets: Vec<i64>,
        result_ty: Type,
    },
    ComputePull {
        output: CtValue,
        anchor: CtValue,
        targets: Vec<i64>,
        gradient_ty: Type,
    },
    ComputeGrads {
        output: CtValue,
        anchor: CtValue,
        targets: Vec<i64>,
        gradient_ty: Type,
    },
}

enum EvalCallableSnapshot<'a> {
    Lambda {
        lambda: &'a TIR::TLambda,
        captured: HashMap<String, CtValue>,
    },
    Named(&'a str),
    ComputeTransform {
        base: CtValue,
        method: String,
        targets: Vec<i64>,
        result_ty: Type,
    },
    ComputePull {
        output: CtValue,
        anchor: CtValue,
        targets: Vec<i64>,
        gradient_ty: Type,
    },
    ComputeGrads {
        output: CtValue,
        anchor: CtValue,
        targets: Vec<i64>,
        gradient_ty: Type,
    },
}

#[derive(Clone)]
struct EvalWebApp {
    steps: Vec<EvalWebAppStep>,
}

#[derive(Clone)]
struct EvalWebAppStep {
    method: String,
    args: Vec<CtValue>,
}

struct EvalStream<'a> {
    func: &'a TFunc,
    args: Vec<CtValue>,
}

const TIR_SELECT_BUILDER: &str = "__JetTirSelectBuilder";
const TIR_SELECT_AFTER: &str = "__JetTirSelectAfter";

struct EvalChannel {
    channel: crate::scheduler::JetSchedulerChannel<CtValue>,
    sender: crate::scheduler::JetSchedulerSender<CtValue>,
}

struct EvalRuntime<'a> {
    callables: Vec<EvalCallable<'a>>,
    interrupt_handlers: Vec<usize>,
    streams: Vec<EvalStream<'a>>,
    shared_values: Vec<Arc<EvalSharedState>>,
    shared_guards: Vec<Arc<shared_protocol::JetSharedGuardState>>,
    shared_conditions: Vec<Arc<shared_protocol::JetConditionProtocol>>,
    clocks: Vec<i64>,
    channels: Vec<EvalChannel>,
    task_groups: Vec<Arc<crate::task_group::JetTaskGroupRuntime<usize>>>,
    tasks: Vec<Option<EvalTask>>,
    web_apps: Vec<EvalWebApp>,
    completion_order: AtomicU64,
}

struct EvalSharedState {
    value: Mutex<CtValue>,
    protocol: Arc<shared_protocol::JetSharedProtocol>,
}

impl EvalSharedState {
    fn new(value: CtValue) -> Self {
        Self {
            value: Mutex::new(value),
            protocol: shared_protocol::JetSharedProtocol::new(),
        }
    }

    fn acquire(
        self: &Arc<Self>,
        editable: bool,
        cancel: Option<&Arc<AtomicBool>>,
    ) -> Option<Arc<shared_protocol::JetSharedPermit>> {
        shared_protocol::jet_shared_acquire(&self.protocol, editable, || {
            cancel.is_some_and(|cancel| cancel.load(Ordering::Acquire))
        })
    }

    fn acquire_guard(
        self: &Arc<Self>,
        editable: bool,
        cancel: Option<&Arc<AtomicBool>>,
    ) -> Option<Arc<shared_protocol::JetSharedGuardState>> {
        shared_protocol::jet_shared_guard_acquire(&self.protocol, editable, || {
            cancel.is_some_and(|cancel| cancel.load(Ordering::Acquire))
        })
    }
}

struct EvalConditionWaiter {
    notified: Mutex<bool>,
    wake: Condvar,
    cancel: Option<Arc<AtomicBool>>,
    deadline: Option<i64>,
}

impl EvalConditionWaiter {
    fn new(cancel: Option<Arc<AtomicBool>>, deadline: Option<i64>) -> Self {
        Self {
            notified: Mutex::new(false),
            wake: Condvar::new(),
            cancel,
            deadline,
        }
    }
}

impl shared_protocol::JetConditionWaiter for EvalConditionWaiter {
    fn park(&self) -> Result<(), ()> {
        let mut notified = self
            .notified
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        while !*notified {
            if self
                .cancel
                .as_ref()
                .is_some_and(|cancel| cancel.load(Ordering::Acquire))
                || self.deadline.is_some_and(|deadline| wall_now_ms() >= deadline)
            {
                return Err(());
            }
            let (next, _) = self
                .wake
                .wait_timeout(notified, std::time::Duration::from_millis(10))
                .unwrap_or_else(|error| error.into_inner());
            notified = next;
        }
        Ok(())
    }

    fn wake(&self) {
        *self
            .notified
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = true;
        self.wake.notify_one();
    }

    fn interrupted(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(|cancel| cancel.load(Ordering::Acquire))
            || self.deadline.is_some_and(|deadline| wall_now_ms() >= deadline)
    }
}

struct EvalTask {
    completion: mpsc::Receiver<EvalTaskCompletion>,
    completion_order: Arc<OnceLock<u64>>,
    completion_wait: Arc<crate::scheduler::ParkSlot>,
    control: Arc<crate::scheduler::JetTaskControl>,
}

struct EvalTaskCompletion {
    result: Result<CtValue, Diagnostic>,
}

#[derive(Debug)]
enum EvalTaskSelectError {
    /// The parent reached its own cancellation/deadline wait policy.
    Wait(Diagnostic),
    /// A child failed while completing. This becomes `? TaskFailure` at the
    /// task surface instead of escaping as an interpreter diagnostic.
    Child(Diagnostic),
}

struct EvalTaskJob<'a> {
    lambda: &'a TJitSpawnLambda,
    captured: HashMap<String, CtValue>,
    task_sender: mpsc::Sender<EvalTaskJob<'a>>,
    context_deadline: Option<i64>,
    completion: mpsc::SyncSender<EvalTaskCompletion>,
    completion_order: Arc<OnceLock<u64>>,
    completion_wait: Arc<crate::scheduler::ParkSlot>,
    control: Arc<crate::scheduler::JetTaskControl>,
    permit: Option<crate::task_group::JetTaskGroupPermit>,
}

fn select_eval_tasks(
    tasks: Vec<EvalTask>,
    mode: crate::task_group::JetTaskSelectMode,
    span: Span,
    mut wait_check: impl FnMut() -> Result<(), Diagnostic>,
) -> Result<Vec<CtValue>, EvalTaskSelectError> {
    crate::task_group::jet_task_select(
        tasks,
        mode,
        || wait_check().map_err(EvalTaskSelectError::Wait),
        |task| task.completion_order.get().copied().map(Into::into),
        |task| match task.completion.try_recv() {
            Ok(completion) => Some(completion.result.map_err(EvalTaskSelectError::Child)),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(EvalTaskSelectError::Child(unsupported(
                    "task completion",
                    span,
                ))))
            }
        },
        |task| task.control.cancel(),
        |task| {
            let _ = task.completion.recv();
        },
    )
}

#[derive(Clone)]
struct EvalTaskConfig<'a> {
    funcs: HashMap<String, &'a TFunc>,
    base_dir: PathBuf,
    sink: Option<Arc<Mutex<DevSink>>>,
    core_imports: &'a HashMap<String, String>,
    globals: HashMap<String, CtValue>,
    allow_impure: bool,
    impure_depth: usize,
    runtime_execution: bool,
    prefer_tir_calls: bool,
    repl_mode: bool,
    repl_grants: Vec<String>,
    struct_fields: HashMap<String, Vec<(String, bool)>>,
    reflect_paths: HashMap<String, String>,
    struct_field_types: HashMap<String, Vec<(String, Type)>>,
    codec_migrations: HashMap<String, TIR::TCodecMigrationPlan>,
    distinct_bases: HashMap<String, Type>,
    distinct_ranges: HashMap<String, (i64, i64)>,
    spawn_lambdas: &'a [TJitSpawnLambda],
    runtime: Arc<Mutex<EvalRuntime<'a>>>,
}

impl EvalRuntime<'_> {
    fn new() -> Self {
        // A fresh interpreter runtime is a teardown boundary. Do not deliver
        // a SIGINT that was marked for a previous dev/restart instance.
        INTERPRETER_INTERRUPT_QUEUE.clear();
        Self {
            callables: Vec::new(),
            interrupt_handlers: Vec::new(),
            streams: Vec::new(),
            shared_values: Vec::new(),
            shared_guards: Vec::new(),
            shared_conditions: Vec::new(),
            clocks: Vec::new(),
            channels: Vec::new(),
            task_groups: Vec::new(),
            tasks: Vec::new(),
            web_apps: Vec::new(),
            completion_order: AtomicU64::new(0),
        }
    }
}

#[derive(Clone)]
struct YieldConsumer<'a> {
    var: String,
    body: &'a [TStmt],
}

impl<'a> EvalCtx<'a> {
    fn scheduler_wait<T>(
        &self,
        wait_kind: &str,
        wait: impl FnOnce() -> T,
    ) -> Result<T, Diagnostic> {
        match crate::scheduler::jet_scheduler_wait_without_unwind(wait) {
            crate::scheduler::JetSchedulerWait::Ready(value) => Ok(value),
            crate::scheduler::JetSchedulerWait::Cancelled => {
                let cancelled = crate::task_group::jet_task_cancellation();
                Err(Diagnostic::error(
                    cancelled.code,
                    cancelled.what.to_string(),
                    cancelled.why.to_string(),
                    cancelled.fix.to_string(),
                    Some(self.span()),
                ))
            }
            crate::scheduler::JetSchedulerWait::Deadline(_) => {
                let deadline = crate::task_group::jet_task_deadline(wait_kind);
                Err(Diagnostic::error(
                    "E3003",
                    deadline.what,
                    deadline.why,
                    deadline.fix,
                    Some(self.span()),
                ))
            }
            crate::scheduler::JetSchedulerWait::Panicked(message) => {
                Err(task_child_panic(message, self.span()))
            }
        }
    }

    fn task_wait_check(&self, wait_kind: &str) -> Result<(), Diagnostic> {
        self.task_wait_while_paused()?;
        let deadline = crate::task_group::jet_task_deadline_if_expired(
            self.context_deadline
                .map(|deadline| {
                    deadline.saturating_sub(crate::scheduler::jet_std_time_now())
                }),
            wait_kind,
        );
        let cancelled = self
            .task_cancel
            .as_ref()
            .is_some_and(|cancel| cancel.load(Ordering::Acquire));
        match crate::task_group::jet_task_wait_policy(deadline, cancelled, self.shield_depth > 0) {
            Ok(()) => Ok(()),
            Err(crate::task_group::JetTaskWaitInterrupt::Deadline(deadline)) => {
                Err(Diagnostic::error(
                    "E3003",
                    deadline.what,
                    deadline.why,
                    deadline.fix,
                    Some(self.span()),
                ))
            }
            Err(crate::task_group::JetTaskWaitInterrupt::Cancelled) => {
                Err(Diagnostic::task_cancelled(Some(self.span())))
            }
        }
    }

    fn task_wait_cancel_check(&self) -> Result<(), Diagnostic> {
        self.task_wait_check("task selection")
    }

    fn task_join_wait_check(&self) -> Result<(), Diagnostic> {
        self.task_wait_check("task join")
    }

    /// The evaluator twin of `JetTaskControl::wait_while_paused`: a paused task
    /// stops at its next cooperative wait point and stays there until it is
    /// resumed or cancelled. The shared scheduler owns the park, wake, and
    /// deadline behavior; this method only maps its boundary status.
    fn task_wait_while_paused(&self) -> Result<(), Diagnostic> {
        if self.task_paused.is_none() {
            return Ok(());
        }
        self.scheduler_wait(
            "task pause",
            crate::scheduler::jet_scheduler_wait_while_paused,
        )
    }

    /// D-VERDICT-1323-1 / D-COROUTINE1=A: set or clear one task's pause flag
    /// without consuming the handle — the evaluator twin of `JetTask::pause`
    /// and `JetTask::resume`.
    pub(super) fn set_task_paused_value(
        &mut self,
        value: &CtValue,
        paused: bool,
    ) -> Result<(), Diagnostic> {
        let index = Self::task_index(value)
            .ok_or_else(|| unsupported("task receiver", self.span()))?;
        if let Some(Some(task)) = self
            .runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .tasks
            .get(index)
        {
            if paused {
                task.control.pause();
            } else {
                task.control.resume();
            }
        }
        Ok(())
    }

    /// The evaluator twin of `JetTask::detach`: drop the join handle so the
    /// task runs unattached and its result is never observed.
    pub(super) fn detach_task_value(&mut self, value: &CtValue) -> Result<(), Diagnostic> {
        let index = Self::task_index(value)
            .ok_or_else(|| unsupported("task receiver", self.span()))?;
        if let Some(slot) = self
            .runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .tasks
            .get_mut(index)
        {
            let _ = slot.take();
        }
        Ok(())
    }

    fn task_config(&self) -> EvalTaskConfig<'a> {
        EvalTaskConfig {
            funcs: self.funcs.clone(),
            base_dir: self.base_dir.clone(),
            sink: self.sink.clone(),
            core_imports: self.core_imports,
            globals: self.globals.clone(),
            allow_impure: self.allow_impure,
            impure_depth: self.impure_depth,
            runtime_execution: self.runtime_execution,
            prefer_tir_calls: self.prefer_tir_calls,
            repl_mode: self.repl_mode,
            repl_grants: self.repl_grants.clone(),
            struct_fields: self.struct_fields.clone(),
            reflect_paths: self.reflect_paths.clone(),
            struct_field_types: self.struct_field_types.clone(),
            codec_migrations: self.codec_migrations.clone(),
            distinct_bases: self.distinct_bases.clone(),
            distinct_ranges: self.distinct_ranges.clone(),
            spawn_lambdas: self.spawn_lambdas,
            runtime: self.runtime.clone(),
        }
    }

    fn with_task_dispatcher<R>(&mut self, run: impl FnOnce(&mut Self) -> R) -> R {
        if self.task_sender.is_some() {
            return run(self);
        }
        std::thread::scope(|threads| {
            let (sender, receiver) = mpsc::channel();
            self.task_sender = Some(sender);
            let config = Arc::new(self.task_config());
            let dispatcher = std::thread::Builder::new()
                .name("jet-tir-task-dispatch".to_string())
                .stack_size(8 * 1024 * 1024)
                .spawn_scoped(threads, {
                    let config = config.clone();
                    move || {
                        while let Ok(job) = receiver.recv() {
                            let job_config = (*config).clone();
                            std::thread::Builder::new()
                                .name("jet-tir-task".to_string())
                                .stack_size(8 * 1024 * 1024)
                                .spawn_scoped(threads, move || {
                                    Self::run_task_job(job_config, job)
                                })
                                .expect("evaluator task worker");
                        }
                    }
                })
                .expect("evaluator task dispatcher");
            let result = run(self);
            drop(self.task_sender.take());
            dispatcher
                .join()
                .expect("evaluator task dispatcher panicked");
            result
        })
    }

    fn run_task_job(config: EvalTaskConfig<'a>, job: EvalTaskJob<'a>) {
        let _permit = job.permit;
        crate::scheduler::jet_scheduler_set_task_control(Some(job.control.clone()));
        let _deadline = job
            .context_deadline
            .map(crate::scheduler::jet_ctx_push_deadline);
        let mut ctx = EvalCtx {
            funcs: config.funcs,
            base_dir: config.base_dir,
            fuel: DEV_FUEL,
            sink: config.sink,
            core_imports: config.core_imports,
            globals: config.globals,
            allow_impure: config.allow_impure,
            impure_depth: config.impure_depth,
            runtime_execution: config.runtime_execution,
            prefer_tir_calls: config.prefer_tir_calls,
            repl_mode: config.repl_mode,
            repl_grants: config.repl_grants,
            repl_authorizer: None,
            pending_return: None,
            deferred_closes: Vec::new(),
            pending_flow: None,
            collecting_items: Vec::new(),
            call_depth: 0,
            source_nesting: 0,
            current_span: Span::new(0, 0),
            emitted_fragments: None,
            embed_inputs: None,
            struct_fields: config.struct_fields,
            reflect_paths: config.reflect_paths,
            struct_field_types: config.struct_field_types,
            codec_migrations: config.codec_migrations,
            distinct_bases: config.distinct_bases,
            distinct_ranges: config.distinct_ranges,
            switch_subject: None,
            runtime: config.runtime.clone(),
            local_cells: local_cell::EvalLocalCells::new(),
            shared_transactions: Vec::new(),
            spawn_lambdas: config.spawn_lambdas,
            task_sender: Some(job.task_sender),
            task_cancel: Some(job.control.cancelled.clone()),
            task_paused: Some(job.control.paused.clone()),
            context_deadline: job.context_deadline,
            shield_depth: 0,
            yield_consumer: None,
            yield_scope: None,
            scope_guards: Vec::new(),
            shared_guards: Vec::new(),
            txn_stack: Vec::new(),
        };
        let mut scope = job.captured;
        let result = match &job.lambda.body {
            TJitSpawnBody::Expr(expr) => ctx.eval_expr(expr, &mut scope),
            TJitSpawnBody::Block { prefix, tail } => match ctx.exec_stmts(prefix, &mut scope) {
                Ok(Flow::Return(value)) => Ok(value),
                Ok(Flow::Normal) => match tail {
                    Some(expr) => ctx.eval_expr(expr, &mut scope),
                    None => Ok(CtValue::Unit),
                },
                Ok(other) => Err(unsupported(
                    &format!("control flow {other:?} escaping spawn"),
                    ctx.span(),
                )),
                Err(error) => Err(error),
            },
            TJitSpawnBody::SharedBlock { body, tail } => {
                if *tail {
                    match body[..].split_last() {
                        Some((last, prefix)) => match ctx.exec_stmts(prefix, &mut scope) {
                            Ok(Flow::Return(value)) => Ok(value),
                            Ok(Flow::Normal) => match last {
                                TStmt::ExprStmt(expr) => ctx.eval_expr(expr, &mut scope),
                                TStmt::Return(Some(expr)) => ctx.eval_expr(expr, &mut scope),
                                _ => Ok(CtValue::Unit),
                            },
                            Ok(other) => Err(unsupported(
                                &format!("control flow {other:?} escaping shared spawn"),
                                ctx.span(),
                            )),
                            Err(error) => Err(error),
                        },
                        None => Err(unsupported("empty shared spawn body", ctx.span())),
                    }
                } else {
                    match ctx.exec_stmts(&body[..], &mut scope) {
                        Ok(Flow::Return(value)) => Ok(value),
                        Ok(Flow::Normal) => Ok(CtValue::Unit),
                        Ok(other) => Err(unsupported(
                            &format!("control flow {other:?} escaping shared spawn"),
                            ctx.span(),
                        )),
                        Err(error) => Err(error),
                    }
                }
            },
        };
        crate::scheduler::jet_scheduler_set_task_control(None);
        let order = config
            .runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .completion_order
            .fetch_add(1, Ordering::AcqRel);
        job.completion_order
            .set(order)
            .expect("task completion recorded twice");
        let _ = job.completion.send(EvalTaskCompletion { result });
        job.completion_wait.wake();
    }

    fn eval_spawn(
        &mut self,
        site: usize,
        group: Option<&'a TExpr>,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let group = match group {
            Some(group) => Some(
                Self::taskgroup_index(&self.eval_expr(group, scope)?)
                    .ok_or_else(|| unsupported("task group handle", self.span()))?,
            ),
            None => None,
        };
        let lam = self
            .spawn_lambdas
            .get(site)
            .ok_or_else(|| unsupported("spawn body", self.span()))?;
        let mut child = HashMap::new();
        for capture in &lam.captures {
            child.insert(
                capture.name.clone(),
                scope
                    .get(&capture.source)
                    .cloned()
                    .or_else(|| self.globals.get(&capture.source).cloned())
                    .unwrap_or(CtValue::Unit),
            );
        }
        let sender = self
            .task_sender
            .as_ref()
            .ok_or_else(|| unsupported("spawn outside a task group", self.span()))?;
        let (completion, receiver) = mpsc::sync_channel(1);
        let completion_order = Arc::new(OnceLock::new());
        let completion_wait = crate::scheduler::ParkSlot::new();
        let control = crate::scheduler::JetTaskControl::new();
        let group_runtime = match group {
            Some(group) => Some(
                self.runtime
                    .lock()
                    .expect("evaluator runtime poisoned")
                    .task_groups
                    .get(group)
                    .cloned()
                    .ok_or_else(|| unsupported("task group handle", self.span()))?,
            ),
            None => None,
        };
        let _deadline = group_runtime
            .as_ref()
            .and_then(|_| self.context_deadline.map(crate::scheduler::jet_ctx_push_deadline));
        let permit = match group_runtime.as_ref() {
            Some(group) => {
                let waiter = crate::scheduler::ParkSlot::new();
                group.acquire_with(waiter, |waiter| {
                    self.task_wait_check("task admission")?;
                    self.scheduler_wait("task admission", || {
                        crate::scheduler::jet_scheduler_yield("task admission", waiter, None)
                    })?;
                    Ok(())
                })?
            }
            None => None,
        };
        drop(_deadline);
        let mut runtime = self.runtime.lock().expect("evaluator runtime poisoned");
        let task = runtime.tasks.len();
        runtime.tasks.push(Some(EvalTask {
            completion: receiver,
            completion_order: completion_order.clone(),
            completion_wait: completion_wait.clone(),
            control: control.clone(),
        }));
        if let Some(group) = group_runtime {
            group.register(task);
        }
        drop(runtime);
        sender
            .send(EvalTaskJob {
                lambda: lam,
                captured: child,
                task_sender: sender.clone(),
                context_deadline: self.context_deadline,
                completion,
                completion_order,
                completion_wait,
                control,
                permit,
            })
            .map_err(|_| unsupported("closed task group", self.span()))?;
        Ok(CtValue::Struct {
            type_name: "__JetTirTask".to_string(),
            fields: vec![("index".to_string(), CtValue::Int(task as i64))],
        })
    }

    fn taskgroup_index(value: &CtValue) -> Option<usize> {
        Self::internal_index(value, "__JetTirTaskGroup")
    }

    fn task_index(value: &CtValue) -> Option<usize> {
        Self::internal_index(value, "__JetTirTask")
    }

    fn internal_index(value: &CtValue, expected: &str) -> Option<usize> {
        let CtValue::Struct { type_name, fields } = value else {
            return None;
        };
        if type_name != expected {
            return None;
        }
        fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
            ("index", CtValue::Int(index)) => usize::try_from(*index).ok(),
            _ => None,
        })
    }

    fn new_taskgroup(
        &mut self,
        limit: Option<&'a TExpr>,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let limit = match limit {
            Some(limit) => match self.eval_expr(limit, scope)? {
                // The shared Prelude owns defaulting and bound clamping for
                // every execution tier.
                CtValue::Int(value) => Some(value),
                _ => return Err(unsupported("task-group limit", self.span())),
            },
            None => None,
        };
        let mut runtime = self.runtime.lock().expect("evaluator runtime poisoned");
        let index = runtime.task_groups.len();
        runtime.task_groups.push(Arc::new(
            crate::task_group::JetTaskGroupRuntime::new_defaulted(limit),
        ));
        Ok(CtValue::Struct {
            type_name: "__JetTirTaskGroup".to_string(),
            fields: vec![("index".to_string(), CtValue::Int(index as i64))],
        })
    }

    fn select_builder_value(
        receivers: Vec<usize>,
        afters: Vec<(i64, CtValue)>,
    ) -> CtValue {
        CtValue::Struct {
            type_name: TIR_SELECT_BUILDER.to_string(),
            fields: vec![
                (
                    "receivers".to_string(),
                    CtValue::List(
                        receivers
                            .into_iter()
                            .map(|index| CtValue::Int(index as i64))
                            .collect(),
                    ),
                ),
                (
                    "afters".to_string(),
                    CtValue::List(
                        afters
                            .into_iter()
                            .map(|(ms, value)| CtValue::Struct {
                                type_name: TIR_SELECT_AFTER.to_string(),
                                fields: vec![
                                    ("ms".to_string(), CtValue::Int(ms)),
                                    ("value".to_string(), value),
                                ],
                            })
                            .collect(),
                    ),
                ),
            ],
        }
    }

    fn select_builder_parts(
        value: &CtValue,
    ) -> Option<(Vec<usize>, Vec<(i64, CtValue)>)> {
        let CtValue::Struct { type_name, fields } = value else {
            return None;
        };
        if type_name != TIR_SELECT_BUILDER {
            return None;
        }
        let receivers = fields.iter().find_map(|(name, value)| {
            (name == "receivers").then(|| match value {
                CtValue::List(values) => values
                    .iter()
                    .map(|value| match value {
                        CtValue::Int(index) => usize::try_from(*index).ok(),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>(),
                _ => None,
            })
        })??;
        let afters = fields.iter().find_map(|(name, value)| {
            (name == "afters").then(|| match value {
                CtValue::List(values) => values
                    .iter()
                    .map(|value| {
                        let CtValue::Struct { type_name, fields } = value else {
                            return None;
                        };
                        if type_name != TIR_SELECT_AFTER {
                            return None;
                        }
                        let ms = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
                            ("ms", CtValue::Int(ms)) => Some(*ms),
                            _ => None,
                        })?;
                        let payload = fields
                            .iter()
                            .find_map(|(name, value)| (name == "value").then(|| value.clone()))?;
                        Some((ms, payload))
                    })
                    .collect::<Option<Vec<_>>>(),
                _ => None,
            })
        })??;
        Some((receivers, afters))
    }

    pub(super) fn new_eval_channel(&mut self, capacity: Option<i64>) -> CtValue {
        let channel = match capacity {
            Some(capacity) => crate::scheduler::JetSchedulerChannel::bounded(capacity),
            None => crate::scheduler::JetSchedulerChannel::new(),
        };
        let sender = channel.sender();
        let mut runtime = self.runtime.lock().expect("evaluator runtime poisoned");
        let index = runtime.channels.len();
        runtime.channels.push(EvalChannel { channel, sender });
        let sender_value = CtValue::Struct {
            type_name: "Sender".to_string(),
            fields: vec![("index".to_string(), CtValue::Int(index as i64))],
        };
        let receiver_value = CtValue::Struct {
            type_name: "Receiver".to_string(),
            fields: vec![("index".to_string(), CtValue::Int(index as i64))],
        };
        CtValue::Struct {
            type_name: "tuple".to_string(),
            fields: vec![
                (crate::Codegen::mangle("sender"), sender_value),
                (crate::Codegen::mangle("receiver"), receiver_value),
            ],
        }
    }

    pub(super) fn send_eval_channel(
        &self,
        index: usize,
        value: CtValue,
    ) -> Result<(), Diagnostic> {
        self.task_wait_cancel_check()?;
        let sender = self
            .runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .channels
            .get(index)
            .map(|channel| channel.sender.clone())
            .ok_or_else(|| unsupported("channel sender", self.span()))?;
        let _deadline = self
            .context_deadline
            .map(crate::scheduler::jet_ctx_push_deadline);
        self.scheduler_wait("channel send", || sender.send(value))?;
        drop(_deadline);
        self.task_wait_cancel_check()?;
        Ok(())
    }

    pub(super) fn receive_eval_channel(
        &self,
        index: usize,
    ) -> Result<CtValue, Diagnostic> {
        self.task_wait_cancel_check()?;
        let channel = self
            .runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .channels
            .get(index)
            .map(|channel| channel.channel.clone())
            .ok_or_else(|| unsupported("channel receiver", self.span()))?;
        let _deadline = self
            .context_deadline
            .map(crate::scheduler::jet_ctx_push_deadline);
        let value = self.scheduler_wait("channel receive", || channel.receive())?;
        drop(_deadline);
        self.task_wait_cancel_check()?;
        Ok(match value {
            Some(value) => CtValue::Present(Box::new(value)),
            None => CtValue::failed(Box::new(CtValue::Enum {
                type_name: "Closed".to_string(),
                variant: "Closed".to_string(),
                args: Vec::new(),
            })),
        })
    }

    pub(super) fn close_eval_channel(&self, index: usize) -> Result<(), Diagnostic> {
        let channel = self
            .runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .channels
            .get(index)
            .map(|channel| channel.channel.clone())
            .ok_or_else(|| unsupported("channel", self.span()))?;
        channel.close();
        Ok(())
    }

    pub(super) fn new_eval_select(&self) -> CtValue {
        Self::select_builder_value(Vec::new(), Vec::new())
    }

    pub(super) fn eval_select_recv(
        &self,
        builder: CtValue,
        receiver: usize,
    ) -> Result<CtValue, Diagnostic> {
        let (mut receivers, afters) = Self::select_builder_parts(&builder)
            .ok_or_else(|| unsupported("select builder", self.span()))?;
        let valid = self
            .runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .channels
            .get(receiver)
            .is_some();
        if !valid {
            return Err(unsupported("select receiver", self.span()));
        }
        receivers.push(receiver);
        Ok(Self::select_builder_value(receivers, afters))
    }

    pub(super) fn eval_select_after(
        &self,
        builder: CtValue,
        millis: i64,
        value: CtValue,
    ) -> Result<CtValue, Diagnostic> {
        let (receivers, mut afters) = Self::select_builder_parts(&builder)
            .ok_or_else(|| unsupported("select builder", self.span()))?;
        afters.push((millis, value));
        Ok(Self::select_builder_value(receivers, afters))
    }

    pub(super) fn eval_select_wait(&self, builder: CtValue) -> Result<CtValue, Diagnostic> {
        let (receiver_ids, after_values) = Self::select_builder_parts(&builder)
            .ok_or_else(|| unsupported("select builder", self.span()))?;
        if receiver_ids.is_empty() && after_values.is_empty() {
            return Err(unsupported("empty select", self.span()));
        }
        let channels = {
            let runtime = self.runtime.lock().expect("evaluator runtime poisoned");
            receiver_ids
                .iter()
                .map(|index| {
                    runtime
                        .channels
                        .get(*index)
                        .map(|channel| channel.channel.clone())
                })
                .collect::<Option<Vec<_>>>()
        }
        .ok_or_else(|| unsupported("select receiver", self.span()))?;
        let recvs = channels
            .iter()
            .map(|channel| channel.select_inner())
            .collect();
        let timers = after_values
            .into_iter()
            .map(|(ms, value)| {
                (
                    crate::scheduler::jet_task_delay_ms_defaulted(ms),
                    Some(value),
                )
            })
            .collect();
        let _deadline = self
            .context_deadline
            .map(crate::scheduler::jet_ctx_push_deadline);
        let value = self.scheduler_wait("select wait", || {
            crate::scheduler::jet_scheduler_select_values(recvs, timers)
        })?;
        drop(_deadline);
        self.task_wait_cancel_check()?;
        Ok(value)
    }

    /// D-VERDICT-1323-1: request cancellation for one task without consuming
    /// it, the evaluator twin of `JetTask::cancel`.
    pub(super) fn cancel_task_value(&mut self, value: &CtValue) -> Result<(), Diagnostic> {
        let index = Self::task_index(value)
            .ok_or_else(|| unsupported("task receiver", self.span()))?;
        if let Some(Some(task)) = self
            .runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .tasks
            .get(index)
        {
            task.control.cancel();
        }
        Ok(())
    }

    fn take_task_entry(&mut self, value: &CtValue) -> Result<(usize, EvalTask), Diagnostic> {
        let index = Self::task_index(value)
            .ok_or_else(|| unsupported("task receiver", self.span()))?;
        let task = self
            .runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .tasks
            .get_mut(index)
            .and_then(Option::take)
            .ok_or_else(|| unsupported("task already joined", self.span()))?;
        Ok((index, task))
    }

    fn restore_task_entry(&mut self, index: usize, task: EvalTask) {
        let mut runtime = self.runtime.lock().expect("evaluator runtime poisoned");
        if let Some(slot) = runtime.tasks.get_mut(index) {
            if slot.is_none() {
                *slot = Some(task);
            }
        }
    }

    pub(super) fn take_task(&mut self, value: &CtValue) -> Result<CtValue, Diagnostic> {
        if let CtValue::Struct { type_name, fields } = value {
            if type_name == "__JetTirTask" {
                if let Some(result) = fields
                    .iter()
                    .find_map(|(name, value)| (name == "value").then(|| value.clone()))
                {
                    // The already-completed carrier still answers to the one
                    // Prelude wait policy (bd15-rev): a cancelled scope or an
                    // expired deadline refuses the join here exactly as
                    // jet_task_wait_policy does on the other tiers.
                    self.task_join_wait_check()?;
                    return Ok(CtValue::Present(Box::new(result)));
                }
            }
        }
        self.task_join_wait_check()?;
        let (index, task) = self.take_task_entry(value)?;
        let _deadline = self
            .context_deadline
            .map(crate::scheduler::jet_ctx_push_deadline);
        let result = loop {
            if let Err(error) = self.task_join_wait_check() {
                self.restore_task_entry(index, task);
                return Err(error);
            }
            match task.completion.try_recv() {
                Ok(completion) => break completion.result,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.restore_task_entry(index, task);
                    return Err(unsupported("task completion", self.span()));
                }
                Err(mpsc::TryRecvError::Empty) => {
                    if let Err(error) = self.scheduler_wait("task join", || {
                        crate::scheduler::jet_scheduler_yield(
                            "task join",
                            &task.completion_wait,
                            None,
                        )
                    }) {
                        self.restore_task_entry(index, task);
                        return Err(error);
                    }
                }
            }
        };
        drop(_deadline);
        self.task_join_wait_check()?;
        match result {
            Ok(value) => Ok(CtValue::Present(Box::new(value))),
            Err(error) => Ok(CtValue::failed(Box::new(task_failure_value(&error)))),
        }
    }

    pub(super) fn task_select(
        &mut self,
        values: &[CtValue],
        mode: crate::task_group::JetTaskSelectMode,
    ) -> Result<CtValue, Diagnostic> {
        let tasks = values
            .iter()
            .map(|value| self.take_task_entry(value).map(|(_, task)| task))
            .collect::<Result<Vec<_>, _>>()?;
        if tasks.is_empty() {
            unreachable!("sema must reject an empty task group combinator");
        }
        match select_eval_tasks(tasks, mode, self.span(), || self.task_wait_cancel_check()) {
            Ok(mut values) => {
                let value = if matches!(mode, crate::task_group::JetTaskSelectMode::All) {
                    CtValue::List(values)
                } else {
                    values.pop().expect("race/any result missing")
                };
                Ok(CtValue::Present(Box::new(value)))
            }
            Err(EvalTaskSelectError::Wait(error)) => Err(error),
            Err(EvalTaskSelectError::Child(error)) => {
                Ok(CtValue::failed(Box::new(task_failure_value(&error))))
            }
        }
    }

    fn close_taskgroup(&mut self, index: usize) -> Result<(), Diagnostic> {
        let span = self.span();
        let group = self
            .runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .task_groups
            .get(index)
            .cloned()
            .ok_or_else(|| unsupported("task group handle", span))?;
        let join_runtime = self.runtime.clone();
        let drain = move |child| {
            let task: Option<EvalTask> = join_runtime
                .lock()
                .expect("evaluator runtime poisoned")
                .tasks
                .get_mut(child)
                .and_then(|slot: &mut Option<EvalTask>| slot.take());
            if let Some(task) = task {
                let _ = task.completion.recv();
            }
        };
        if let Err(interruption) = self.task_wait_cancel_check() {
            let cancel_runtime = self.runtime.clone();
            group.close_with_cancel(
                move |child| {
                    if let Some(task) = cancel_runtime
                        .lock()
                        .expect("evaluator runtime poisoned")
                        .tasks
                        .get(*child)
                        .and_then(Option::as_ref)
                    {
                        task.control.cancel();
                    }
                },
                drain,
            );
            return Err(interruption);
        }
        group.close_with(drain);
        self.task_wait_cancel_check()
    }

    pub(crate) fn span(&self) -> Span {
        self.current_span
    }

    fn check_contracts(
        &mut self,
        contracts: &'a [TIR::TContract],
        keyword: &str,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic> {
        for contract in contracts {
            let previous_span = self.current_span;
            self.current_span = contract.span;
            let condition = match self.eval_expr(&contract.condition, scope) {
                Ok(CtValue::Bool(value)) => value,
                Ok(_) => {
                    self.current_span = previous_span;
                    return Err(unsupported("contract condition", contract.span));
                }
                Err(error) => {
                    self.current_span = previous_span;
                    return Err(error);
                }
            };
            if condition {
                self.current_span = previous_span;
                continue;
            }
            let message = match self.eval_expr(&contract.message, scope) {
                Ok(value) => value.jet_show(),
                Err(error) => {
                    self.current_span = previous_span;
                    return Err(error);
                }
            };
            self.current_span = previous_span;
            if let Some(sink) = self.sink.as_ref() {
                let mut sink = sink.lock().expect("evaluator sink poisoned");
                sink.stderr.push_str(&format!(
                    "#{} contract failed: {}\n  --> {}:{}\n",
                    keyword, message, contract.file, contract.line
                ));
                sink.exit_code = Some(70);
                return Err(Diagnostic::soft_exit(
                    "70".to_string(),
                    "runtime contract failed".to_string(),
                    Some(contract.span),
                ));
            }
            return Err(Diagnostic::error(
                "E3005",
                format!("#{keyword} contract failed: {message}"),
                "a runtime contract condition evaluated false".to_string(),
                "satisfy the contract or update it".to_string(),
                Some(contract.span),
            ));
        }
        Ok(())
    }

    pub(super) fn burn(&mut self) -> Result<(), Diagnostic> {
        if self.fuel == 0 {
            // Dev/REPL fragments use E2202; pure comptime uses E0952.
            let (code, what, why, fix) = if self.sink.is_some() || self.repl_mode {
                (
                    "E2202",
                    "this program ran too many steps without finishing".to_string(),
                    format!(
                        "`jet dev` interprets your program to give instant feedback, but it ran out of steps without finishing — this usually means a loop that never ends"
                    ),
                    "check the loop near here for a condition that never becomes false; run `jet run` to execute the real build with no step limit"
                        .to_string(),
                )
            } else {
                (
                    "E0952",
                    "compile-time evaluation ran out of steps".to_string(),
                    "a loop or recursion in this expression doesn't finish".to_string(),
                    "simplify the expression, or move the work to runtime".to_string(),
                )
            };
            return Err(Diagnostic::error(code, what, why, fix, Some(self.span())));
        }
        self.fuel -= 1;
        Ok(())
    }

    pub(super) fn enter_source_nesting(&mut self) -> Result<(), Diagnostic> {
        let span = self.span();
        enter_source_nesting(&mut self.source_nesting, span)
    }

    pub(super) fn leave_source_nesting(&mut self) {
        self.source_nesting -= 1;
    }

    pub(crate) fn run_func(
        &mut self,
        func: &'a TFunc,
        args: Vec<CtValue>,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        if self.call_depth > 64 {
            self.fuel = 0;
            self.burn()?;
            unreachable!("burn with fuel 0 always errors");
        }
        self.call_depth += 1;
        let previous_source_nesting = std::mem::replace(&mut self.source_nesting, 0);
        let previous_span = std::mem::replace(&mut self.current_span, func.source_span);
        let guard_mark = self.scope_guards.len();
        self.local_cells.enter_frame();
        for (i, (name, _, _)) in func.params.iter().enumerate() {
            let jet = name.strip_prefix(crate::Syntax::GENERATED_NAME_PREFIX).unwrap_or(name.as_str());
            let value = args.get(i).cloned().unwrap_or(CtValue::Unit);
            scope.insert(jet.to_string(), value.clone());
            if jet != name {
                scope.insert(name.clone(), value);
            }
        }
        let result = match self.check_contracts(&func.pre_contracts, "Pre", scope) {
            Ok(()) => match self.exec_stmts(&func.body, scope) {
                Ok(Flow::Return(v)) => Ok(v),
                Ok(Flow::Normal) => Ok(CtValue::Unit),
                Ok(other) => Err(unsupported(
                        &format!("control flow {other:?} escaping function"),
                        self.span(),
                    )),
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        };
        // Run scope.guard cleanups LIFO, matching Drop order in AOT/JIT.
        let guards: Vec<_> = self.scope_guards.drain(guard_mark..).rev().collect();
        let mut cleanup_result = Ok(());
        for lam in guards {
            if let Err(error) = self.eval_tlambda(lam, Vec::new(), scope) {
                cleanup_result = Err(error);
                break;
            }
        }
        let returned = result.as_ref().ok().cloned().unwrap_or(CtValue::Unit);
        self.local_cells.leave_frame(&returned);
        self.call_depth -= 1;
        self.source_nesting = previous_source_nesting;
        self.current_span = previous_span;
        let post_result = match (&result, &cleanup_result) {
            (Ok(value), Ok(())) if !func.post_contracts.is_empty() => {
                scope.insert("__jet_result".to_string(), value.clone());
                let checked = self.check_contracts(&func.post_contracts, "Post", scope);
                scope.remove("__jet_result");
                checked
            }
            _ => Ok(()),
        };
        match (result, cleanup_result, post_result) {
            (Err(error), _, _) | (Ok(_), Err(error), _) | (Ok(_), Ok(()), Err(error)) => {
                Err(error)
            }
            (Ok(value), Ok(()), Ok(())) => Ok(value),
        }
    }

    fn store_callable(&mut self, callable: EvalCallable<'a>) -> CtValue {
        let mut runtime = self.runtime.lock().expect("evaluator runtime poisoned");
        let index = runtime.callables.len() as i64;
        runtime.callables.push(callable);
        CtValue::Struct {
            type_name: "__JetTirCallable".to_string(),
            fields: vec![("index".to_string(), CtValue::Int(index))],
        }
    }

    fn callable_value(index: usize) -> CtValue {
        CtValue::Struct {
            type_name: "__JetTirCallable".to_string(),
            fields: vec![("index".to_string(), CtValue::Int(index as i64))],
        }
    }

    fn callable_index(value: &CtValue) -> Option<usize> {
        let CtValue::Struct { type_name, fields } = value else {
            return None;
        };
        if type_name != "__JetTirCallable" {
            return None;
        }
        fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
            ("index", CtValue::Int(index)) => usize::try_from(*index).ok(),
            _ => None,
        })
    }

    pub(super) fn register_interrupt_callback(
        &mut self,
        callback: &'a TExpr,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        ensure_interpreter_interrupt_handler().map_err(|message| {
            unsupported(&interrupt_queue::jet_interrupt_core_error(&message), self.span())
        })?;
        let value = self.eval_expr(callback, scope)?;
        let index = Self::callable_index(&value)
            .ok_or_else(|| {
                unsupported(
                    interrupt_queue::jet_interrupt_invalid_callback_value_error(),
                    self.span(),
                )
            })?;
        self.runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .interrupt_handlers
            .push(index);
        Ok(CtValue::Unit)
    }

    pub(super) fn dispatch_pending_interrupts(
        &mut self,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic> {
        let handlers = self
            .runtime
            .lock()
            .expect("evaluator runtime poisoned")
            .interrupt_handlers
            .clone();
        let mut deferred_panic = None;
        let mut failure = None;
        INTERPRETER_INTERRUPT_QUEUE.dispatch(&handlers, |index| {
            if failure.is_some() {
                return;
            }
            let value = Self::callable_value(*index);
            match self.call_callable(&value, Vec::new()) {
                Ok(_) => {}
                Err(error) if error.code == "SOFT_EXIT" => {
                    let panic_stop = self.sink.as_ref().is_some_and(|sink| {
                        sink.lock()
                            .expect("evaluator sink poisoned")
                            .exit_code
                            == Some(70)
                    });
                    if panic_stop {
                        deferred_panic.get_or_insert(error);
                    } else {
                        failure = Some(error);
                    }
                }
                Err(error) => failure = Some(error),
            }
        });
        if let Some(error) = failure {
            return Err(error);
        }
        if let Some(error) = deferred_panic {
            return Err(error);
        }
        let _ = scope;
        Ok(())
    }

    fn store_stream(&mut self, func: &'a TFunc, args: Vec<CtValue>) -> CtValue {
        let mut runtime = self.runtime.lock().expect("evaluator runtime poisoned");
        let index = runtime.streams.len() as i64;
        runtime.streams.push(EvalStream { func, args });
        CtValue::Struct {
            type_name: "__JetTirStream".to_string(),
            fields: vec![("index".to_string(), CtValue::Int(index))],
        }
    }

    fn stream_index(value: &CtValue) -> Option<usize> {
        let CtValue::Struct { type_name, fields } = value else {
            return None;
        };
        if type_name != "__JetTirStream" {
            return None;
        }
        fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
            ("index", CtValue::Int(index)) => usize::try_from(*index).ok(),
            _ => None,
        })
    }

    pub(super) fn call_callable(
        &mut self,
        value: &CtValue,
        args: Vec<CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let index = Self::callable_index(value)
            .ok_or_else(|| unsupported("calling this non-function value", self.span()))?;
        let target = {
            let runtime = self.runtime.lock().expect("evaluator runtime poisoned");
            match runtime.callables.get(index) {
                Some(EvalCallable::Lambda { lambda, captured }) => {
                    EvalCallableSnapshot::Lambda {
                        lambda: *lambda,
                        captured: captured.clone(),
                    }
                }
                Some(EvalCallable::Named(name)) => EvalCallableSnapshot::Named(*name),
                Some(EvalCallable::ComputeTransform {
                    base,
                    method,
                    targets,
                    result_ty,
                }) => EvalCallableSnapshot::ComputeTransform {
                    base: base.clone(),
                    method: method.clone(),
                    targets: targets.clone(),
                    result_ty: result_ty.clone(),
                },
                Some(EvalCallable::ComputePull {
                    output,
                    anchor,
                    targets,
                    gradient_ty,
                }) => EvalCallableSnapshot::ComputePull {
                    output: output.clone(),
                    anchor: anchor.clone(),
                    targets: targets.clone(),
                    gradient_ty: gradient_ty.clone(),
                },
                Some(EvalCallable::ComputeGrads {
                    output,
                    anchor,
                    targets,
                    gradient_ty,
                }) => EvalCallableSnapshot::ComputeGrads {
                    output: output.clone(),
                    anchor: anchor.clone(),
                    targets: targets.clone(),
                    gradient_ty: gradient_ty.clone(),
                },
                None => return Err(unsupported("calling an unknown function value", self.span())),
            }
        };
        match target {
            EvalCallableSnapshot::Lambda {
                lambda,
                mut captured,
            } => {
                let result = self.eval_tlambda(lambda, args, &mut captured);
                if result.is_ok() {
                    if let Some(EvalCallable::Lambda {
                        captured: stored, ..
                    }) = self
                        .runtime
                        .lock()
                        .expect("evaluator runtime poisoned")
                        .callables
                        .get_mut(index)
                    {
                        *stored = captured;
                    }
                }
                result
            }
            EvalCallableSnapshot::Named(name) => {
                let func = self
                    .funcs
                    .get(name)
                    .copied()
                    .ok_or_else(|| unsupported(&format!("callable function `{name}`"), self.span()))?;
                let mut child = HashMap::new();
                self.run_func(func, args, &mut child)
            }
            EvalCallableSnapshot::ComputeTransform {
                base,
                method,
                targets,
                result_ty,
            } => self.eval_compute_transform(&method, base, args, targets, &result_ty),
            EvalCallableSnapshot::ComputePull {
                output,
                anchor,
                targets,
                gradient_ty,
            } => self.eval_compute_pull(output, anchor, args, targets, &gradient_ty),
            EvalCallableSnapshot::ComputeGrads {
                output,
                anchor,
                targets,
                gradient_ty,
            } => self.eval_compute_grads(output, anchor, args, targets, &gradient_ty),
        }
    }

    /// Call the canonical callable slot and publish mutable captures back to
    /// the active lexical scope. The runtime slot remains authoritative for
    /// returned/escaped closures; the scope write-back is the local-variable
    /// half of the same FnMut storage contract.
    pub(super) fn call_callable_in_scope(
        &mut self,
        value: &CtValue,
        args: Vec<CtValue>,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let result = self.call_callable(value, args);
        self.sync_callable_captures(value, scope);
        result
    }

    pub(super) fn sync_callable_captures(
        &self,
        value: &CtValue,
        scope: &mut HashMap<String, CtValue>,
    ) {
        let Some(index) = Self::callable_index(value) else {
            return;
        };
        let updates = {
            let runtime = self.runtime.lock().expect("evaluator runtime poisoned");
            let Some(EvalCallable::Lambda { lambda, captured }) = runtime.callables.get(index)
            else {
                return;
            };
            lambda
                .captures
                .iter()
                .filter_map(|(source, runtime_name, _)| {
                    let updated = captured
                        .get(runtime_name)
                        .or_else(|| captured.get(source))?
                        .clone();
                    Some((source.clone(), updated))
                })
                .collect::<Vec<_>>()
        };
        for (source, updated) in updates {
            if scope.contains_key(&source) {
                scope.insert(source, updated);
            }
        }
    }
}

fn empty_cx() -> Cx {
    build_cx_items(&[], "", "<eval>", None, &HashMap::new())
}

fn seed_fragment_distinct_types(
    cx: &mut Cx,
    ranges: &HashMap<String, Option<(i64, i64)>>,
    bases: &HashMap<String, crate::AST::Type>,
) {
    for (name, base) in bases {
        cx.distinct_types
            .insert(name.clone(), (base.clone(), base.is_numeric()));
    }
    for (name, range) in ranges {
        if let Some(bounds) = range {
            cx.distinct_ranges.insert(name.clone(), *bounds);
        }
    }
}

fn seed_fragment_funcs(cx: &mut Cx, funcs: &HashMap<String, &Func>) {
    for (name, function) in funcs {
        cx.fn_param_names.insert(
            name.clone(),
            function.params.iter().map(|parameter| parameter.name.clone()).collect(),
        );
        cx.fn_type_params.insert(
            name.clone(),
            function
                .type_params
                .iter()
                .map(|parameter| parameter.name.clone())
                .collect(),
        );
        cx.fn_type_param_order.insert(
            name.clone(),
            function
                .type_params
                .iter()
                .map(|parameter| parameter.name.clone())
                .collect(),
        );
        cx.sigs.insert(
            name.clone(),
            function
                .params
                .iter()
                .map(|parameter| {
                    let ty = if parameter.variadic {
                        Type::List(Box::new(parameter.ty.clone()))
                    } else {
                        parameter.ty.clone()
                    };
                    (parameter.convention, ty)
                })
                .collect(),
        );
        cx.fn_types.insert(
            name.clone(),
            Type::Fn {
                params: function
                    .params
                    .iter()
                    .map(|parameter| {
                        if parameter.variadic {
                            Type::List(Box::new(parameter.ty.clone()))
                        } else {
                            parameter.ty.clone()
                        }
                    })
                    .collect(),
                ret: function.return_type.clone().map(Box::new),
                effect_bound: None, return_view_provenance: None,
                param_contract: (!function.params.is_empty()).then(|| {
                    function
                        .params
                        .iter()
                        .map(|parameter| (parameter.call_label().to_string(), parameter.zone))
                        .collect()
                }),
                call_metadata: Some(crate::AST::FunctionCallMetadata {
                    names: function.params.iter().map(|parameter| parameter.name.clone()).collect(),
                    defaults: function
                        .params
                        .iter()
                        .map(|parameter| parameter.default.as_deref().cloned())
                        .collect(),
                    variadic: function.params.iter().map(|parameter| parameter.variadic).collect(),
                    conventions: function
                        .params
                        .iter()
                        .map(|parameter| parameter.convention)
                        .collect(),
                }),
            },
        );
    }
}

/// Fragment evaluation normally receives the free-function table from the
/// comptime driver and a separate semantic method table. Build the same
/// lookup surface that whole-program lowering has: methods are keyed by
/// Owner::method, while free functions retain their source keys.
fn merge_fragment_funcs<'a>(
    funcs: &'a HashMap<String, &'a Func>,
    methods: &'a HashMap<(String, String), &'a Func>,
) -> HashMap<String, &'a Func> {
    let mut merged = funcs.clone();
    for ((owner, name), function) in methods {
        merged
            .entry(format!("{owner}::{name}"))
            .or_insert(*function);
    }
    merged
}

fn seed_fragment_structs(
    cx: &mut Cx,
    structs: &HashMap<String, &crate::AST::StructDef>,
    methods: &HashMap<(String, String), &Func>,
    computed_fields: &HashMap<(String, String), &Expr>,
) {
    for (name, definition) in structs {
        cx.type_names.insert(name.clone());
        cx.struct_fields.insert(
            name.clone(),
            definition
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.ty.clone()))
                .collect(),
        );
        if !definition.type_params.is_empty() {
            let params = definition
                .type_params
                .iter()
                .map(|param| param.name.clone())
                .collect::<Vec<_>>();
            cx.struct_type_params
                .insert(name.clone(), params.iter().cloned().collect());
            cx.struct_type_param_order.insert(name.clone(), params);
        }
        let computed = definition
            .fields
            .iter()
            .filter(|field| field.computed.is_some())
            .map(|field| field.name.clone())
            .collect::<std::collections::HashSet<_>>();
        if !computed.is_empty() {
            cx.computed_fields.insert(name.clone(), computed);
        }
    }
    // Preserve sema's qualified and short keys even if a fragment's StructDef
    // table is sparse.
    for ((owner, field), _) in computed_fields {
        cx.computed_fields
            .entry(owner.clone())
            .or_default()
            .insert(field.clone());
    }
    for ((owner, name), method) in methods {
        let key = (owner.clone(), name.clone());
        if let Some(self_param) = method
            .params
            .iter()
            .find(|param| param.name == crate::Syntax::KW_SELF)
        {
            cx.method_self_convs.insert(key.clone(), self_param.convention);
        }
        cx.method_sigs.insert(
            key.clone(),
            method
                .params
                .iter()
                .filter(|param| param.name != crate::Syntax::KW_SELF)
                .map(|param| {
                    let ty = if param.variadic {
                        Type::List(Box::new(param.ty.clone()))
                    } else {
                        param.ty.clone()
                    };
                    (param.convention, ty)
                })
                .collect(),
        );
        cx.method_rets.insert(key, method.return_type.clone());
    }
}

/// Lower one expression for the evaluator (comptime / REPL fragments).
pub fn lower_expr_for_eval(
    expr: &Expr,
    funcs: &HashMap<String, &Func>,
    methods: &HashMap<(String, String), &Func>,
    structs: &HashMap<String, &crate::AST::StructDef>,
    computed_fields: &HashMap<(String, String), &Expr>,
    globals: &HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    distinct_ranges: &HashMap<String, Option<(i64, i64)>>,
    distinct_bases: &HashMap<String, crate::AST::Type>,
) -> Result<TExpr, Diagnostic> {
    let mut diagnostic = None;
    crate::Comptime::walk_expr_nodes_for_validation(expr, &mut |node| {
        if diagnostic.is_none() {
            diagnostic = crate::Codegen::TIR::validate_typed_boundary_before_lowering(node);
        }
    });
    if let Some(diagnostic) = diagnostic {
        return Err(diagnostic);
    }
    let mut cx = empty_cx();
    seed_fragment_structs(&mut cx, structs, methods, computed_fields);
    seed_fragment_distinct_types(&mut cx, distinct_ranges, distinct_bases);
    seed_fragment_funcs(&mut cx, funcs);
    cx.const_values = globals.clone();
    for (name, value) in globals {
        cx.consts.insert(name.clone(), String::new());
        let _ = value;
    }
    cx.core_imports = core_imports.clone();
    let mut env = LowerEnv::new("__ct".into());
    for name in globals.keys() {
        env.bind(name, TLocal::user(name), None);
    }
    // Fragment eval: sema facts may still be incomplete (e.g. IndexKind::Unknown).
    // Try lower under catch_unwind; refuse with E0956 only when lower can't.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::Codegen::TIR::with_eval_fragment(|| TIR::lower_expr(expr, &cx, &mut env))
    })) {
        Ok(tir) => Ok(tir),
        Err(_) => Err(unsupported("this expression", Span::new(0, 0))),
    }
}

/// Lower a statement list for the evaluator.
pub fn lower_stmts_for_eval(
    stmts: &[Stmt],
    funcs: &HashMap<String, &Func>,
    methods: &HashMap<(String, String), &Func>,
    structs: &HashMap<String, &crate::AST::StructDef>,
    computed_fields: &HashMap<(String, String), &Expr>,
    globals: &HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    distinct_ranges: &HashMap<String, Option<(i64, i64)>>,
    distinct_bases: &HashMap<String, crate::AST::Type>,
) -> Result<Vec<TStmt>, Diagnostic> {
    let mut diagnostic = None;
    crate::Comptime::walk_stmt_expr_nodes_for_validation(stmts, &mut |expr| {
        if diagnostic.is_none() {
            diagnostic = crate::Codegen::TIR::validate_typed_boundary_before_lowering(expr);
        }
    });
    if let Some(diagnostic) = diagnostic {
        return Err(diagnostic);
    }
    let mut cx = empty_cx();
    seed_fragment_structs(&mut cx, structs, methods, computed_fields);
    seed_fragment_distinct_types(&mut cx, distinct_ranges, distinct_bases);
    seed_fragment_funcs(&mut cx, funcs);
    cx.const_values = globals.clone();
    for name in globals.keys() {
        cx.consts.insert(name.clone(), String::new());
    }
    cx.core_imports = core_imports.clone();
    let mut env = LowerEnv::new("__ct_block".into());
    for name in globals.keys() {
        env.bind(name, TLocal::user(name), None);
    }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::Codegen::TIR::with_eval_fragment(|| TIR::lower_stmts(stmts, &cx, &mut env))
    })) {
        Ok(tir) => Ok(tir),
        Err(_) => Err(unsupported("this statement", Span::new(0, 0))),
    }
}

pub fn lower_interp_program(bundle: &ProgramBundle) -> Option<JitProgram> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| TIR::lower_jit_program(bundle)))
    {
        Ok(program) => program,
        Err(_) => None,
    }
}

fn program_funcs(program: &JitProgram) -> HashMap<String, &TFunc> {
    program.funcs.iter().map(|f| (f.name.clone(), f)).collect()
}

fn struct_metadata_keys(
    bundle: &ProgramBundle,
    owner_idx: usize,
    type_name: &str,
) -> Vec<String> {
    let keys = if owner_idx == bundle.entry {
        vec![type_name.to_string()]
    } else {
        crate::Codegen::TIR::imported_type_owners(bundle, owner_idx)
            .into_iter()
            .map(|owner| crate::Codegen::TIR::imported_type_name(&owner, type_name))
            .collect()
    };
    keys
}

fn collect_struct_fields(bundle: &ProgramBundle) -> HashMap<String, Vec<(String, bool)>> {
    let mut out = HashMap::new();
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        for item in &module.items {
            if let crate::AST::Item::Struct(s) = item {
                let fields = s
                    .fields
                    .iter()
                    .map(|f| (f.name.clone(), f.redact))
                    .collect::<Vec<(String, bool)>>();
                for key in struct_metadata_keys(bundle, module_idx, &s.name) {
                    out.insert(key, fields.clone());
                }
            }
        }
    }
    out
}

fn collect_struct_field_types(
    bundle: &ProgramBundle,
) -> HashMap<String, Vec<(String, crate::AST::Type)>> {
    let mut out = HashMap::new();
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        for item in &module.items {
            if let crate::AST::Item::Struct(s) = item {
                let fields = s
                    .fields
                    .iter()
                    .map(|f| (f.name.clone(), f.ty.clone()))
                    .collect::<Vec<(String, crate::AST::Type)>>();
                for key in struct_metadata_keys(bundle, module_idx, &s.name) {
                    out.insert(key, fields.clone());
                }
            }
        }
    }
    insert_core_struct_field_types(&mut out);
    out
}

fn insert_core_struct_field_types(
    fields: &mut HashMap<String, Vec<(String, crate::AST::Type)>>,
) {
    fields.insert(
        crate::Syntax::TYPE_IO_CONTEXT.to_string(),
        vec![
            (
                "operation".to_string(),
                Type::Named(crate::Syntax::TYPE_IO_OPERATION.to_string()),
            ),
            (
                "resource".to_string(),
                Type::Option(Box::new(Type::String)),
            ),
            (
                "os_code".to_string(),
                Type::Option(Box::new(Type::Int)),
            ),
            (
                "cause".to_string(),
                Type::Option(Box::new(Type::String)),
            ),
        ],
    );
}

fn normalize_struct_field_types(
    structs: &HashMap<String, &crate::AST::StructDef>,
) -> HashMap<String, Vec<(String, crate::AST::Type)>> {
    let mut out = structs
        .iter()
        .map(|(name, definition)| {
            (
                name.clone(),
                definition
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.ty.clone()))
                    .collect(),
            )
        })
        .collect();
    insert_core_struct_field_types(&mut out);
    out
}

fn program_struct_field_types(
    program: &JitProgram,
) -> HashMap<String, Vec<(String, crate::AST::Type)>> {
    let mut out = program
        .struct_field_types
        .iter()
        .filter_map(|(type_name, types)| {
            let names = program.struct_fields.get(type_name)?;
            Some((
                type_name.clone(),
                names
                    .iter()
                    .zip(types)
                    .map(|(name, ty)| {
                        (
                            name.strip_prefix(crate::Syntax::GENERATED_NAME_PREFIX).unwrap_or(name).to_string(),
                            ty.clone(),
                        )
                    })
                    .collect(),
            ))
        })
        .collect();
    insert_core_struct_field_types(&mut out);
    out
}

pub fn run_program(
    program: &JitProgram,
    base_dir: &Path,
    sink: &mut DevSink,
    globals: HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    allow_impure: bool,
) -> Result<CtValue, Diagnostic> {
    run_program_with_structs(
        program,
        base_dir,
        sink,
        globals,
        core_imports,
        allow_impure,
        HashMap::new(),
        HashMap::new(),
    )
}

fn validate_kernel_proofs(program: &JitProgram) -> Result<(), Diagnostic> {
    if let Some(func) = program
        .funcs
        .iter()
        .find(|func| func.kernel_proof.is_some_and(|proof| !proof.is_complete()))
    {
        return Err(Diagnostic::error(
            "E0956",
            format!("kernel proof for `{}` is incomplete", func.name),
            "the interpreter consumes sema's complete kernel proof before execution"
                .to_string(),
            "report this as a compiler bug".to_string(),
            Some(func.source_span),
        ));
    }
    Ok(())
}

pub fn run_program_with_structs(
    program: &JitProgram,
    base_dir: &Path,
    sink: &mut DevSink,
    globals: HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    allow_impure: bool,
    struct_fields: HashMap<String, Vec<(String, bool)>>,
    struct_field_types: HashMap<String, Vec<(String, crate::AST::Type)>>,
) -> Result<CtValue, Diagnostic> {
    run_program_with_structs_at_stage(
        program,
        base_dir,
        sink,
        globals,
        core_imports,
        allow_impure,
        struct_fields,
        struct_field_types,
        Comptime::PurityStage::RunTime,
    )
}

pub fn run_program_with_structs_at_stage(
    program: &JitProgram,
    base_dir: &Path,
    sink: &mut DevSink,
    globals: HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    allow_impure: bool,
    struct_fields: HashMap<String, Vec<(String, bool)>>,
    struct_field_types: HashMap<String, Vec<(String, crate::AST::Type)>>,
    stage: Comptime::PurityStage,
) -> Result<CtValue, Diagnostic> {
    // The evaluator's exhaustive expression dispatcher is intentionally one
    // semantic spine, but its large Rust frame makes ordinary test/CLI stacks
    // too small for nested aggregate literals. Keep the public runtime seam
    // on a bounded worker stack; this changes no language semantics.
    let (ambient_core, ambient_handle) = crate::Comptime::ambient_hooks();
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("jet-tir-eval".to_string())
            .stack_size(8 * 1024 * 1024)
            .spawn_scoped(scope, move || {
                crate::Comptime::with_ambient(ambient_core, ambient_handle, || {
                    run_program_with_structs_on_stack(
                        program,
                        base_dir,
                        sink,
                        globals,
                        core_imports,
                        allow_impure,
                        struct_fields,
                        struct_field_types,
                        stage,
                    )
                })
            })
            .expect("evaluator worker");
        worker.join().expect("evaluator worker panicked")
    })
}

fn run_program_with_structs_on_stack(
    program: &JitProgram,
    base_dir: &Path,
    sink: &mut DevSink,
    globals: HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    allow_impure: bool,
    struct_fields: HashMap<String, Vec<(String, bool)>>,
    mut struct_field_types: HashMap<String, Vec<(String, crate::AST::Type)>>,
    stage: Comptime::PurityStage,
) -> Result<CtValue, Diagnostic> {
    validate_kernel_proofs(program)?;
    // Fresh EventLite stores per whole-program run (REPL / warm cache / workers).
    crate::Comptime::reset_event_lite();
    let _browser_session = browser::SessionGuard::new();
    insert_core_struct_field_types(&mut struct_field_types);
    for (name, fields) in program_struct_field_types(program) {
        struct_field_types.entry(name).or_insert(fields);
    }
    let funcs = program_funcs(program);
    let entry = funcs.get(&program.entry).copied().ok_or_else(|| {
        Diagnostic::error(
            "E2201",
            format!("entry `{}` missing from lowered TIR", program.entry),
            "the interpreter needs the selected entry function in the TIR program".to_string(),
            "report this as a compiler bug".to_string(),
            None,
        )
    })?;
    let shared_sink = Arc::new(Mutex::new(std::mem::take(sink)));
    let mut ctx = EvalCtx {
        funcs,
        base_dir: base_dir.to_path_buf(),
        fuel: DEV_FUEL,
        sink: Some(shared_sink.clone()),
        core_imports,
        globals,
        allow_impure,
        // Whole-program runtime/deopt carries RunTime explicitly.  Comptime
        // purity still uses eval_expr/eval_block with build-time defaults.
        impure_depth: if matches!(stage, Comptime::PurityStage::RunTime) && allow_impure {
            1
        } else {
            0
        },
        runtime_execution: matches!(stage, Comptime::PurityStage::RunTime),
        prefer_tir_calls: false,
        repl_mode: false,
        repl_grants: Vec::new(),
        repl_authorizer: None,
        pending_return: None,
        deferred_closes: Vec::new(),
        pending_flow: None,
        collecting_items: Vec::new(),
        call_depth: 0,
        source_nesting: 0,
        current_span: entry.source_span,
        emitted_fragments: None,
        embed_inputs: None,
        struct_fields,
        reflect_paths: program.reflect_paths.clone(),
        struct_field_types,
        codec_migrations: program.codec_migrations.clone(),
        distinct_bases: program.distinct_bases.clone(),
        distinct_ranges: program.distinct_ranges.clone(),
        switch_subject: None,
        runtime: Arc::new(Mutex::new(EvalRuntime::new())),
        local_cells: local_cell::EvalLocalCells::new(),
        shared_transactions: Vec::new(),
        spawn_lambdas: &program.spawn_lambdas,
        task_sender: None,
        task_cancel: None,
        task_paused: None,
        context_deadline: None,
        shield_depth: 0,
        yield_consumer: None,
        yield_scope: None,
        scope_guards: Vec::new(),
        shared_guards: Vec::new(),
        txn_stack: Vec::new(),
    };
    let mut scope = HashMap::new();
    let result = ctx.with_task_dispatcher(|ctx| ctx.run_func(entry, Vec::new(), &mut scope));
    *sink = std::mem::take(
        &mut *shared_sink.lock().expect("evaluator sink poisoned"),
    );
    result
}

/// Run one named function through the canonical TIR evaluator (#778 deopt).
pub fn run_named_func(
    program: &JitProgram,
    name: &str,
    args: Vec<CtValue>,
    sink: &mut DevSink,
) -> Result<CtValue, Diagnostic> {
    validate_kernel_proofs(program)?;
    let _browser_session = browser::SessionGuard::new();
    let funcs = program_funcs(program);
    let func = funcs.get(name).copied().ok_or_else(|| {
        Diagnostic::error(
            "E2201",
            format!("function `{name}` missing from lowered TIR"),
            "the deopt tier needs the named function in the TIR program".to_string(),
            "report this as a compiler bug".to_string(),
            None,
        )
    })?;
    let core_imports = HashMap::new();
    let shared_sink = Arc::new(Mutex::new(std::mem::take(sink)));
    let mut ctx = EvalCtx {
        funcs,
        base_dir: PathBuf::from("."),
        fuel: DEV_FUEL,
        sink: Some(shared_sink.clone()),
        core_imports: &core_imports,
        globals: HashMap::new(),
        allow_impure: true,
        // Runtime deopt is not comptime: open Tier-2 ambient I/O so `jet run`
        // matches AOT for env/fs/process (D-LENS-RUN2 / #778).
        // parity: guard tests/dev.rs::dev_default_matches_compiled_binary
        impure_depth: 1,
        runtime_execution: true,
        prefer_tir_calls: program.canonical_calls.contains(name),
        repl_mode: false,
        repl_grants: Vec::new(),
        repl_authorizer: None,
        pending_return: None,
        deferred_closes: Vec::new(),
        pending_flow: None,
        collecting_items: Vec::new(),
        call_depth: 0,
        source_nesting: 0,
        current_span: func.source_span,
        emitted_fragments: None,
        embed_inputs: None,
        struct_fields: HashMap::new(),
        reflect_paths: program.reflect_paths.clone(),
        struct_field_types: program_struct_field_types(program),
        codec_migrations: program.codec_migrations.clone(),
        distinct_bases: program.distinct_bases.clone(),
        distinct_ranges: program.distinct_ranges.clone(),
        switch_subject: None,
        runtime: Arc::new(Mutex::new(EvalRuntime::new())),
        local_cells: local_cell::EvalLocalCells::new(),
        shared_transactions: Vec::new(),
        spawn_lambdas: &program.spawn_lambdas,
        task_sender: None,
        task_cancel: None,
        task_paused: None,
        context_deadline: None,
        shield_depth: 0,
        yield_consumer: None,
        yield_scope: None,
        scope_guards: Vec::new(),
        shared_guards: Vec::new(),
        txn_stack: Vec::new(),
    };
    let mut scope = HashMap::new();
    let result = ctx.with_task_dispatcher(|ctx| ctx.run_func(func, args, &mut scope));
    *sink = std::mem::take(
        &mut *shared_sink.lock().expect("evaluator sink poisoned"),
    );
    result
}

static INSTALLED: OnceLock<()> = OnceLock::new();

pub fn install_comptime_bridge() {
    INSTALLED.get_or_init(|| {
        Comptime::TirBridge::install(Comptime::TirBridge::Hooks {
            run_bundle,
            run_bundle_at_stage,
            eval_expr: eval_expr_hook,
            eval_block: eval_block_hook,
        });
    });
}

fn run_bundle(
    bundle: &ProgramBundle,
    sink: &mut DevSink,
    allow_impure: bool,
) -> Result<CtValue, Diagnostic> {
    run_bundle_at_stage(bundle, sink, allow_impure, Comptime::PurityStage::RunTime)
}

fn run_bundle_at_stage(
    bundle: &ProgramBundle,
    sink: &mut DevSink,
    allow_impure: bool,
    stage: Comptime::PurityStage,
) -> Result<CtValue, Diagnostic> {
    let program = lower_interp_program(bundle).ok_or_else(|| {
        Diagnostic::error(
            "E2201",
            "`jet dev` needs a `run` function to run".to_string(),
            "`jet dev` runs a program; a library with no `run` has nothing to execute".to_string(),
            "add `fn run() { … }`, or use `jet check <file>`".to_string(),
            None,
        )
    })?;
    let mut globals = HashMap::new();
    let mut core_imports = HashMap::new();
    let persist = jet_foundation::Persist::prepare_bundle(bundle);
    for msg in &persist.messages {
        // Dev-tier only: surface reset / migration notes on stderr.
        eprintln!("{msg}");
    }
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        for item in &module.items {
            if let crate::AST::Item::Const(c) = item {
                let value = if c.is_persist {
                    persist.by_name.get(&c.name).cloned()
                } else {
                    None
                }
                .or_else(|| {
                    c.ct.clone().or_else(|| match &c.value {
                        crate::AST::Expr::Int(v, _, _, _) => Some(CtValue::Int(*v)),
                        crate::AST::Expr::Bool(v, _) => Some(CtValue::Bool(*v)),
                        _ => None,
                    })
                });
                if let Some(v) = value {
                    globals.entry(c.name.clone()).or_insert_with(|| v.clone());
                    // ConstRef sometimes carries the Rust-mangled spelling.
                    globals
                        .entry(mangle(&c.name))
                        .or_insert(v);
                }
            }
        }
        for imp in &module.imports {
            if let crate::AST::ImportKind::Unqualified { items, .. } = &imp.kind {
                for (original, alias) in items {
                    let local = crate::AST::import_item_alias(original, alias.as_deref());
                    if let Some(binding) = bundle.name_ledger.effective_alias(module_idx, local) {
                        if binding.target == "core" || binding.target.starts_with("core.") {
                            core_imports
                                .entry(local.to_string())
                                .or_insert_with(|| binding.target.clone());
                        }
                    }
                }
            } else if let Some(core_module) = imp.core_module_path() {
                let alias = imp.import_alias();
                if bundle.name_ledger.effective_alias(module_idx, &alias).is_some() {
                    core_imports.entry(alias).or_insert(core_module);
                }
            }
        }
    }
    run_program_with_structs_at_stage(
        &program,
        &bundle.project_root,
        sink,
        globals,
        &core_imports,
        allow_impure,
        collect_struct_fields(bundle),
        collect_struct_field_types(bundle),
        stage,
    )
}

fn eval_expr_hook(
    req: &mut Comptime::TirBridge::ExprEvalRequest<'_>,
) -> Result<CtValue, Diagnostic> {
    let fragment_funcs = merge_fragment_funcs(req.funcs, req.methods);
    let tir = lower_expr_for_eval(
        req.expr,
        &fragment_funcs,
        req.methods,
        req.structs,
        req.computed_fields,
        req.globals,
        req.core_imports,
        req.distinct_ranges,
        req.distinct_bases,
    )?;
    let mut cx = empty_cx();
    seed_fragment_structs(&mut cx, req.structs, req.methods, req.computed_fields);
    seed_fragment_distinct_types(&mut cx, req.distinct_ranges, req.distinct_bases);
    seed_fragment_funcs(&mut cx, &fragment_funcs);
    cx.struct_fields = normalize_struct_field_types(req.structs);
    cx.type_names.extend(req.structs.keys().cloned());
    cx.core_imports = req.core_imports.clone();
    let lowered: Vec<TFunc> = fragment_funcs
        .iter()
        .filter_map(|(name, f)| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::Codegen::TIR::with_eval_fragment(|| {
                    let mut lowered = match name.rsplit_once("::") {
                        Some((owner, method))
                            if req
                                .methods
                                .contains_key(&(owner.to_string(), method.to_string())) =>
                        {
                            TIR::lower_method(f, owner, &cx)
                        }
                        Some((owner, "encode")) => {
                            TIR::lower_trait_method(f, owner, &cx, crate::Generics::ENCODE)
                        }
                        Some((owner, "decode")) => {
                            TIR::lower_trait_method(f, owner, &cx, crate::Generics::DECODE)
                        }
                        _ => TIR::lower_func(f, &cx),
                    };
                    lowered.name = name.clone();
                    lowered
                })
            }))
            .ok()
        })
        .collect();
    let funcs: HashMap<String, &TFunc> = lowered.iter().map(|f| (f.name.clone(), f)).collect();
    let base_dir = req.base_dir.to_path_buf();
    let fuel = req.fuel;
    let core_imports = req.core_imports;
    let globals = req.globals.clone();
    let allow_impure = req.allow_impure;
    let impure_depth = req.initial_impure_depth;
    let repl_mode = req.repl_mode;
    let repl_grants = req.repl_grants.to_vec();
    let repl_authorizer = reborrow_repl_authorizer(&mut req.repl_authorizer);
    let source_span = req.expr.span();
    let mut sink_target = req.sink.take();
    let mutated_out = req.mutated.take();
    let sink = sink_target
        .as_deref_mut()
        .map(|sink| Arc::new(Mutex::new(std::mem::take(sink))));
    let emitted_fragments = req.emitted_fragments.take();
    let embed_inputs = req.embed_inputs.take();
    let mut ctx = EvalCtx {
        funcs,
        base_dir,
        fuel,
        sink,
        core_imports,
        globals: globals.clone(),
        allow_impure,
        impure_depth,
        runtime_execution: false,
        prefer_tir_calls: false,
        repl_mode,
        repl_grants,
        repl_authorizer,
        pending_return: None,
        deferred_closes: Vec::new(),
        pending_flow: None,
        collecting_items: Vec::new(),
        call_depth: 0,
        source_nesting: 0,
        current_span: source_span,
        emitted_fragments,
        embed_inputs,
        struct_fields: HashMap::new(),
        reflect_paths: HashMap::new(),
        struct_field_types: normalize_struct_field_types(req.structs),
        codec_migrations: HashMap::new(),
        distinct_bases: HashMap::new(),
        distinct_ranges: HashMap::new(),
        switch_subject: None,
        runtime: Arc::new(Mutex::new(EvalRuntime::new())),
        local_cells: local_cell::EvalLocalCells::new(),
        shared_transactions: Vec::new(),
        spawn_lambdas: &[],
        task_sender: None,
        task_cancel: None,
        task_paused: None,
        context_deadline: None,
        shield_depth: 0,
        yield_consumer: None,
        yield_scope: None,
        scope_guards: Vec::new(),
        shared_guards: Vec::new(),
        txn_stack: Vec::new(),
    };
    let mut scope = globals;
    let result = ctx.eval_expr(&tir, &mut scope);
    if let (Some(target), Some(shared)) = (sink_target, ctx.sink.as_ref()) {
        *target = std::mem::take(&mut *shared.lock().expect("evaluator sink poisoned"));
    }
    // Hand back the bindings the expression left behind: a mutating receiver
    // (`reader.read_u8()`, `cursor.skip_ws()`) advances state the next
    // statement must observe.
    if let Some(out) = mutated_out {
        *out = scope;
    }
    result
}

fn eval_block_hook(
    req: &mut Comptime::TirBridge::BlockEvalRequest<'_>,
) -> Result<Comptime::TirBridge::StmtOutcome, Diagnostic> {
    let fragment_funcs = merge_fragment_funcs(req.funcs, req.methods);
    let tir = lower_stmts_for_eval(
        req.stmts,
        &fragment_funcs,
        req.methods,
        req.structs,
        req.computed_fields,
        req.globals,
        req.core_imports,
        req.distinct_ranges,
        req.distinct_bases,
    )?;
    let mut cx = empty_cx();
    seed_fragment_structs(&mut cx, req.structs, req.methods, req.computed_fields);
    seed_fragment_distinct_types(&mut cx, req.distinct_ranges, req.distinct_bases);
    seed_fragment_funcs(&mut cx, &fragment_funcs);
    cx.core_imports = req.core_imports.clone();
    let lowered: Vec<TFunc> = fragment_funcs
        .iter()
        .filter_map(|(name, f)| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::Codegen::TIR::with_eval_fragment(|| {
                    let mut lowered = match name.rsplit_once("::") {
                        Some((owner, method))
                            if req
                                .methods
                                .contains_key(&(owner.to_string(), method.to_string())) =>
                        {
                            TIR::lower_method(f, owner, &cx)
                        }
                        _ => TIR::lower_func(f, &cx),
                    };
                    lowered.name = name.clone();
                    lowered
                })
            }))
            .ok()
        })
        .collect();
    let funcs: HashMap<String, &TFunc> = lowered.iter().map(|f| (f.name.clone(), f)).collect();
    let base_dir = req.base_dir.to_path_buf();
    let fuel = req.fuel;
    let core_imports = req.core_imports;
    let globals = req.globals.clone();
    let allow_impure = req.allow_impure;
    let impure_depth = req.impure_depth;
    let repl_mode = req.repl_mode;
    let repl_grants = req.repl_grants.to_vec();
    let repl_authorizer = reborrow_repl_authorizer(&mut req.repl_authorizer);
    let source_span = req
        .stmts
        .first()
        .map(crate::AST::Stmt::span)
        .unwrap_or_else(|| Span::new(0, 0));
    let mut sink_target = req.sink.take();
    let sink = sink_target
        .as_deref_mut()
        .map(|sink| Arc::new(Mutex::new(std::mem::take(sink))));
    let emitted_fragments = req.emitted_fragments.take();
    let embed_inputs = req.embed_inputs.take();
    let mut ctx = EvalCtx {
        funcs,
        base_dir,
        fuel,
        sink,
        core_imports,
        globals: globals.clone(),
        allow_impure,
        impure_depth,
        runtime_execution: false,
        prefer_tir_calls: false,
        repl_mode,
        repl_grants,
        repl_authorizer,
        pending_return: None,
        deferred_closes: Vec::new(),
        pending_flow: None,
        collecting_items: Vec::new(),
        call_depth: 0,
        source_nesting: 0,
        current_span: source_span,
        emitted_fragments,
        embed_inputs,
        struct_fields: HashMap::new(),
        reflect_paths: HashMap::new(),
        struct_field_types: normalize_struct_field_types(req.structs),
        codec_migrations: HashMap::new(),
        distinct_bases: HashMap::new(),
        distinct_ranges: HashMap::new(),
        switch_subject: None,
        runtime: Arc::new(Mutex::new(EvalRuntime::new())),
        local_cells: local_cell::EvalLocalCells::new(),
        shared_transactions: Vec::new(),
        spawn_lambdas: &[],
        task_sender: None,
        task_cancel: None,
        task_paused: None,
        context_deadline: None,
        shield_depth: 0,
        yield_consumer: None,
        yield_scope: None,
        scope_guards: Vec::new(),
        shared_guards: Vec::new(),
        txn_stack: Vec::new(),
    };
    let mut scope = globals;
    let outcome = match ctx.exec_stmts(&tir, &mut scope)? {
        Flow::Normal => Ok(Comptime::TirBridge::StmtOutcome::Done(scope)),
        Flow::Return(value) => Ok(Comptime::TirBridge::StmtOutcome::Returned { value, scope }),
        other => Err(unsupported(
            &format!("control flow {other:?} in statement fragment"),
            ctx.span(),
        )),
    };
    if let (Some(target), Some(shared)) = (sink_target, ctx.sink.as_ref()) {
        *target = std::mem::take(&mut *shared.lock().expect("evaluator sink poisoned"));
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::{
        mpsc, select_eval_tasks, unsupported, Arc, CtValue, EvalTask,
        EvalTaskCompletion, OnceLock, Span,
    };
    use crate::task_group::JetTaskSelectMode;
    use crate::scheduler::JetTaskControl;

    fn ready_task(
        order: u64,
        result: Result<CtValue, crate::Diagnostics::Diagnostic>,
    ) -> EvalTask {
        let (sender, completion) = mpsc::sync_channel(1);
        let completion_order = Arc::new(OnceLock::new());
        completion_order.set(order).unwrap();
        sender.send(EvalTaskCompletion { result }).unwrap();
        EvalTask {
            completion,
            completion_order,
            completion_wait: crate::scheduler::ParkSlot::new(),
            control: JetTaskControl::new(),
        }
    }

    #[test]
    fn selection_keeps_every_already_ready_completion() {
        let all = select_eval_tasks(
            vec![
                ready_task(1, Ok(CtValue::Int(10))),
                ready_task(0, Ok(CtValue::Int(20))),
            ],
            JetTaskSelectMode::All,
            Span::new(0, 0),
            || Ok(()),
        )
        .unwrap();
        assert_eq!(all, [CtValue::Int(10), CtValue::Int(20)]);

        let race = select_eval_tasks(
            vec![
                ready_task(
                    0,
                    Err(unsupported("first completion failed", Span::new(0, 0))),
                ),
                ready_task(1, Ok(CtValue::Int(22))),
            ],
            JetTaskSelectMode::Race,
            Span::new(0, 0),
            || Ok(()),
        )
        .unwrap();
        assert_eq!(race, [CtValue::Int(22)]);
    }
}
