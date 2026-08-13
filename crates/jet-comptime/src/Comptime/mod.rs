//! M9.5 — Comptime v1 (CTFE). A tree-walking interpreter over the typed
//! AST that evaluates a pure, deterministic Jet subset at compile time and
//! bakes the answer into the binary. See the comptime section of docs/spec/spec.md.
//!
//! One law (S26): comptime computes *values* only — it never creates,
//! parameterizes, or selects a type, and never affects dispatch.
//!
//! Diagnostics: E3401 impurity (with call path — shared code with the
//! run-time `=[]=>` check, D-META-EFFECT1 c3) · E0952 fuel exhausted ·
//! E0953 comptime panic (user message verbatim, overflow, divide-by-zero) ·
//! E0955 embed_file errors · E0956 construct not yet supported at comptime.
//!
//! Semantics are bit-for-bit identical to the compiled runtime (the
//! differential battery in tests/comptime_diff.rs is the enforcement):
//! i64 `Int`, IEEE f64 `Float` (S21 display via `{:?}`), char-counted
//! `String` (S41), and `BTreeMap` ordering (S38).

pub mod Build;
/// Host/builtin surface shared by the canonical TIR evaluator (#777) and
/// any remaining policy wrappers (purity). Public so `jet-codegen`'s TIR
/// eval can dispatch without a second builtin table.
pub mod Builtins;
/// Shared collection CtValue ops for TirBridge (#722 / #777).
pub mod CollectionEval;
mod AmbientRuntime;
mod ArgsLite;
mod EventLite;
mod CryptoLite;
mod ArchiveLite;
mod ZstdEntropy;
pub mod ComputeLite;
pub mod ServicesLite;
pub mod AppLite;
pub mod AuthLite;
pub mod SyncLite;
mod DataPipeline;
mod Diagnostics;
mod EncodingLite;
pub mod EmailAdapter;
mod Interpreter;
mod JSONInterp;
pub mod MathLayout;
mod Methods;
mod Purity;
mod Reflect;
mod RegexLite;
mod TextLite;
mod TypedDecode;
mod UrlLite;
pub mod TirBridge;

#[allow(dead_code)]
mod typed_text_kernel {
    include!("../../../jet-codegen/src/Prelude/TypedText.rs");
}

pub use AmbientRuntime::{
    ambient_hooks, try_core_call as try_ambient_core_call,
    try_core_call_typed as try_ambient_core_call_typed, try_handle as try_ambient_handle,
    with_ambient,
};
pub use ArgsLite::{core_args_spec, eval_handle as eval_args_handle};
pub use EventLite::{
    core_event_async_result, core_event_decision_hook, core_event_hook, core_event_new,
    core_event_policy_sync, core_event_scope, core_event_with_policy,
    eval_method as eval_event_method, reset as reset_event_lite,
};

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::Diagnostics::Diagnostic;
use crate::AST::{EnumDef, Expr, Func, StructDef, Type};
use crate::Syntax;

pub use Interpreter::{DebugHook, DevSink, ReplAuthorizer, ReplEffectRequest, REPL_FUEL_BUDGET, with_runtime_argv};
pub use Methods::{
    apply_core_call, apply_core_call_with_type, apply_data_line_call, apply_impure_core_call,
    apply_impure_core_call_with_type,
    apply_repl_authorized_core_call, apply_repl_authorized_core_call_with_type,
    display_core_pure_value, eval_regex_replace_all_with,
};
pub use Methods::{apply_seeded_rng_method, apply_seeded_rng_method_with_type};
#[doc(hidden)]
pub use Methods::{
    eval_build_time_io, eval_net_fetch, is_tier2_core_call, vault_comptime_denied,
};

/// D-DATAFLOW1=A / #778: TIR deopt path for `core.data.csv` reuses the same
/// EncodingLite CSV splitter as comptime typed decode (no second codec).
pub fn runtime_csv_parse(text: &str) -> Result<Vec<Vec<String>>, String> {
    EncodingLite::csv_parse(text)
}

/// D-BOUND-HEAD1: typed URL heads validate at comptime with the same URL
/// kernel that backs the runtime string constructor.
pub fn validate_url_literal(value: &str) -> Result<(), String> {
    UrlLite::parse(value).map(|_| ())
}

/// D-BOUND-HEAD1: validate URL literal components without flattening holes
/// into a string that can change the URL grammar.
pub fn validate_typed_url_literal(literals: &[String]) -> Result<(), String> {
    UrlLite::validate_typed_url_literal(literals)
}

pub fn validate_typed_path_literal(literals: &[String]) -> Result<(), String> {
    let literal_refs = literals.iter().map(String::as_str).collect::<Vec<_>>();
    typed_text_kernel::jet_validate_typed_path_literal(&literal_refs)
}

pub fn validate_typed_boundary_literal(
    kind: Syntax::TypedHeadKind,
    literals: &[String],
) -> Result<(), String> {
    match kind {
        Syntax::TypedHeadKind::URL => validate_typed_url_literal(literals),
        Syntax::TypedHeadKind::Path => validate_typed_path_literal(literals),
        Syntax::TypedHeadKind::DateTime => {
            validate_datetime_literal(&literals.iter().map(String::as_str).collect::<String>())
        }
        _ => Err("typed head is not a checked boundary head".to_string()),
    }
}

/// D-BOUND-HEAD1: typed holes use the same pure display projection as nested
/// runtime values. There is no CtValue debug fallback: sema must admit only
/// values for which this canonical renderer succeeds.
pub fn render_typed_hole(value: &crate::AST::CtValue) -> Option<String> {
    display_core_pure_value(value)
}

pub fn render_typed_holes(
    values: &[crate::AST::CtValue],
    span: crate::Diagnostics::Span,
) -> Result<Vec<String>, Diagnostic> {
    values
        .iter()
        .map(render_typed_hole)
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| {
            Diagnostic::error(
                "E0112",
                "a typed literal hole has no canonical JetShow renderer".to_string(),
                "typed literal holes use one display contract in every execution tier".to_string(),
                "use a sema-admitted printable value or convert it to String before the hole".to_string(),
                Some(span),
            )
        })
}

/// D-BOUND-HEAD1: typed DateTime heads validate through the shared Prelude
/// parser; runtime string constructors remain unchanged.
pub fn validate_datetime_literal(value: &str) -> Result<(), String> {
    Methods::validate_datetime_literal(value)
}

/// D-BOUND-HEAD1: validate raw heads at the typed boundary, then evaluate a
/// sema-rewritten head through the one shared interpolation law and existing
/// pure Core parsers. Runtime string constructors stay on their existing paths.
pub fn evaluate_typed_head(
    kind: Syntax::TypedHeadKind,
    literals: &[String],
    holes: &[CtValue],
    span: crate::Diagnostics::Span,
) -> Result<CtValue, Diagnostic> {
    let name = kind.source_name();
    if !kind.is_boundary() {
        return Err(Diagnostic::error(
            "E0956",
            format!("typed head `{name}` can't run at compile time yet"),
            "the canonical TIR evaluator only accepts the ratified URL, Path, and DateTime heads".to_string(),
            "use one of `URL.{\"…\"}`, `Path.{\"…\"}`, or `DateTime.{\"…\"}`".to_string(),
            Some(span),
        ));
    }
    if kind.forbids_holes() && !holes.is_empty() {
        return Err(Diagnostic::error(
            "E0155",
            "a `DateTime` literal cannot contain interpolation".to_string(),
            "DateTime values are checked as complete RFC3339 literals before the program runs".to_string(),
            "write a complete `DateTime.{\"…\"}` literal, or parse a runtime String explicitly".to_string(),
            Some(span),
        ));
    }
    if let Err(reason) = validate_typed_boundary_literal(kind, literals) {
        return Err(Diagnostic::error(
            "E0155",
            format!("this `{name}` literal is invalid"),
            reason,
            format!(
                "fix the literal, or parse a runtime String with the ordinary `{name}` constructor"
            ),
            Some(span),
        ));
    }
    let literal_refs = literals.iter().map(String::as_str).collect::<Vec<_>>();
    let shown_holes = render_typed_holes(holes, span)?;
    match kind {
        Syntax::TypedHeadKind::URL => {
            let url = UrlLite::typed_url_literal(literals, &shown_holes);
            Ok(Methods::url_parts_to_ct(&url))
        }
        Syntax::TypedHeadKind::Path => Ok(CtValue::Struct {
            type_name: Syntax::TYPE_PATH.to_string(),
            fields: vec![(
                "inner".to_string(),
                CtValue::Str(typed_text_kernel::jet_typed_path_interpolate(
                    &literal_refs,
                    &shown_holes,
                )),
            )],
        }),
        Syntax::TypedHeadKind::DateTime => {
            let text = typed_text_kernel::jet_typed_datetime_interpolate(
                &literal_refs,
                &shown_holes,
            );
            let parsed = Methods::evaluate_typed_datetime_literal(&text, span)?;
            match parsed {
                CtValue::Present(value) => Ok(*value),
                failure => Err(Diagnostic::error(
                    "E0956",
                    format!("{name} typed head produced an invalid value"),
                    match render_typed_hole(&failure) {
                        Some(text) => format!(
                            "the typed head parser returned `{text}` after sema validation"
                        ),
                        None => "the typed head parser returned an unrenderable value after sema validation".to_string(),
                    },
                    "keep the literal skeleton valid and let holes supply only head-safe values".to_string(),
                    Some(span),
                )),
            }
        }
        _ => unreachable!("boundary evaluation validated its descriptor"),
    }
}

/// D-DATA-STATUS1 / #708: the same rows `data.status()` returns, from the one
/// Prelude kernel — used by `jet inspect dossier data`.
pub fn data_status_rows() -> Vec<(String, String, String, String, String, String, String)> {
    Methods::data_status_rows()
}
pub use Methods::apply_dollar_splices;
pub use Purity::{
    check_build_time_io, walk_calls, walk_identifiers, walk_purity_expr, walk_purity_stmts,
    walk_purity_stmts_from, walk_expr_nodes_for_validation,
    walk_stmt_expr_nodes_for_validation, PurityStage,
};
pub use Reflect::{
    build_attribution_info, build_distinct_type_info, build_distinct_type_info_with_path,
    build_enum_layout_info, build_maturity_info, build_movedness_info, build_program_info,
    build_registered_fact_info, build_registered_fact_infos, build_sendability_info,
    build_struct_layout_info, build_struct_type_info, build_struct_type_info_with_path,
    build_struct_type_info_with_states, build_track_origin_info,
    build_unit_scale_provenance_info, build_view_provenance_info, ProgramSemanticFacts,
};
pub use crate::AST::{CtReport, CtValue};

static REPL_INTERRUPT_COUNT: AtomicUsize = AtomicUsize::new(0);
static REPL_RUNTIME_CALL_ACTIVE: AtomicBool = AtomicBool::new(false);
static REPL_INTERRUPTIBLE_TURN_ACTIVE: AtomicBool = AtomicBool::new(false);
static REPL_INTERRUPT_WARNING_EMITTED: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
unsafe extern "C" {
    fn write(fd: i32, bytes: *const u8, len: usize) -> isize;
}

/// Reset per-turn interrupt state immediately before raw REPL evaluation.
pub fn begin_repl_interruptible_turn() {
    REPL_INTERRUPTIBLE_TURN_ACTIVE.store(true, Ordering::SeqCst);
    REPL_RUNTIME_CALL_ACTIVE.store(false, Ordering::SeqCst);
    REPL_INTERRUPT_WARNING_EMITTED.store(false, Ordering::SeqCst);
    REPL_INTERRUPT_COUNT.store(0, Ordering::SeqCst);
}

pub fn end_repl_interruptible_turn() {
    REPL_INTERRUPTIBLE_TURN_ACTIVE.store(false, Ordering::SeqCst);
    REPL_RUNTIME_CALL_ACTIVE.store(false, Ordering::SeqCst);
}

pub fn repl_interruptible_turn_active() -> bool {
    REPL_INTERRUPTIBLE_TURN_ACTIVE.load(Ordering::SeqCst)
}

/// Async-signal-safe Ctrl-C note used by the raw REPL signal handlers.
pub fn note_repl_interrupt() {
    REPL_INTERRUPT_COUNT.fetch_add(1, Ordering::SeqCst);
}

pub fn repl_interrupt_count() -> usize {
    REPL_INTERRUPT_COUNT.load(Ordering::SeqCst)
}

pub fn repl_runtime_call_active() -> bool {
    REPL_RUNTIME_CALL_ACTIVE.load(Ordering::SeqCst)
}

/// Async-signal-safe notice for a Ctrl-C received inside a host runtime call.
pub fn warn_repl_runtime_call_stopping() {
    if !repl_runtime_call_active() || REPL_INTERRUPT_WARNING_EMITTED.swap(true, Ordering::SeqCst) {
        return;
    }
    #[cfg(unix)]
    {
        const MESSAGE: &[u8] = b"\r\nwarning: interrupt requested; waiting for active external I/O to stop\r\n";
        unsafe {
            write(2, MESSAGE.as_ptr(), MESSAGE.len());
        }
    }
}

struct ReplRuntimeCallGuard(bool);

impl ReplRuntimeCallGuard {
    fn new(active: bool) -> Self {
        if active {
            REPL_RUNTIME_CALL_ACTIVE.store(true, Ordering::SeqCst);
        }
        Self(active)
    }
}

impl Drop for ReplRuntimeCallGuard {
    fn drop(&mut self) {
        if self.0 {
            REPL_RUNTIME_CALL_ACTIVE.store(false, Ordering::SeqCst);
            if !std::thread::panicking() && repl_interrupt_count() > 0 {
                std::panic::resume_unwind(Box::new(ReplInterrupted));
            }
        }
    }
}

#[derive(Debug)]
struct ReplInterrupted;

#[derive(Debug)]
pub enum ReplStepError {
    Diagnostic(Diagnostic),
    Interrupted,
}

use Interpreter::{Interp, DEV_FUEL_BUDGET, FUEL_BUDGET};
use Purity::check_purity;

#[derive(Debug, Clone)]
pub struct ProgramBuildEvaluation {
    pub plan: Build::BuildPlan,
    pub comptime_inputs: Vec<crate::AST::ComptimeInput>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Run selected root `fn build` through same interpreter used by comptime and
/// dev. Sema has already checked whole bundle; this stage constructs graph
/// values only, never type-checks by execution.
pub fn run_build_entry(
    build: &Func,
    funcs: &HashMap<String, &Func>,
    base_dir: &Path,
    program: &ProgramInfo,
    program_value: CtValue,
    package: &str,
    allow_impure: bool,
) -> Result<ProgramBuildEvaluation, Diagnostic> {
    run_build_entry_with_policy(
        build,
        funcs,
        base_dir,
        program,
        program_value,
        package,
        allow_impure,
        Build::BuildPolicy::local_default(),
    )
}

/// Build entry with an explicit policy snapshot. The legacy wrapper above is
/// kept for direct Rust consumers; the production driver uses this seam so
/// policy cannot be bypassed by the comptime bridge.
pub fn run_build_entry_with_policy(
    build: &Func,
    funcs: &HashMap<String, &Func>,
    base_dir: &Path,
    program: &ProgramInfo,
    program_value: CtValue,
    package: &str,
    allow_impure: bool,
    policy: Build::BuildPolicy,
) -> Result<ProgramBuildEvaluation, Diagnostic> {
    let context = Build::begin_program_build_with_policy_at(
        package,
        program_value,
        policy,
        base_dir,
    );
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel: DEV_FUEL_BUDGET,
        sink: None,
        core_imports: &program.core_imports,
        debugger: None,
        depth: 0,
        cur_func: "build".to_string(),
        impure_depth: 0,
        allow_impure,
        repl_mode: false,
        repl_grants: Vec::new(),
        repl_authorizer: None,
        repl_interruptible: false,
        embed_inputs: Vec::new(),
        emitted_fragments: Vec::new(),
        binding_types: HashMap::new(),
        globals: &program.globals,
        methods: &program.methods,
        structs: &program.structs,
        computed_fields: &program.computed_fields,
        distinct_ranges: &program.distinct_ranges,
        distinct_bases: &program.distinct_bases,
        migrations: &program.migrations,
        list_write_windows: HashMap::new(),
    };
    let mut frame = HashMap::new();
    frame.insert(build.params[0].name.clone(), context.clone());
    let returned = match interp.call_func("build", build, frame) {
        Ok(value) => value,
        Err(error) => {
            Build::abort_program_build(&context);
            return Err(error);
        }
    };
    if let CtValue::Failed(CtReport::Told(error)) = &returned {
        Build::abort_program_build(&context);
        let detail = error.jet_show();
        let (code, what, why, fix) = if let Some(detail) = detail.strip_prefix("E3511: ") {
            (
                "E3511",
                detail.to_string(),
                "generated source must reach a bounded deterministic order, not loop until quiescent".to_string(),
                "break the dependency between these generators or give each generated module one owner".to_string(),
            )
        } else {
            (
                "E3502",
                format!("`fn build` returned an error: {detail}"),
                "the selected root build entry must finish graph construction before any action runs".to_string(),
                "fix the failing build operation; use `jet inspect explain-build` to inspect completed graph nodes".to_string(),
            )
        };
        return Err(Diagnostic::error(
            code,
            what,
            why,
            fix,
            Some(build.name_span),
        ));
    }
    let (plan, diagnostics) = Build::finish_program_build(&context, &returned)?;
    Ok(ProgramBuildEvaluation {
        plan,
        comptime_inputs: interp.embed_inputs,
        diagnostics,
    })
}

// An empty core_imports map for paths that don't have `use` declarations.
static EMPTY_IMPORTS: std::sync::OnceLock<HashMap<String, String>> = std::sync::OnceLock::new();
fn empty_imports() -> &'static HashMap<String, String> {
    EMPTY_IMPORTS.get_or_init(HashMap::new)
}

// c139: empty registries for evaluation contexts that don't thread the
// whole-program info a `jet dev` run collects (comptime bindings, `derive`
// bodies, the REPL) — user-method dispatch, computed fields, and
// distinct-type constructors reached from those contexts still surface their
// existing E0956 rather than silently no-op-ing.
static EMPTY_GLOBALS: std::sync::OnceLock<HashMap<String, CtValue>> = std::sync::OnceLock::new();
fn empty_globals() -> &'static HashMap<String, CtValue> {
    EMPTY_GLOBALS.get_or_init(HashMap::new)
}
static EMPTY_METHODS: std::sync::OnceLock<HashMap<(String, String), &'static Func>> =
    std::sync::OnceLock::new();
fn empty_methods() -> &'static HashMap<(String, String), &'static Func> {
    EMPTY_METHODS.get_or_init(HashMap::new)
}
static EMPTY_STRUCTS: std::sync::OnceLock<HashMap<String, &'static StructDef>> =
    std::sync::OnceLock::new();
fn empty_structs() -> &'static HashMap<String, &'static StructDef> {
    EMPTY_STRUCTS.get_or_init(HashMap::new)
}

/// TIR core-call bridge for schema-aware CBOR encoding. `CtValue` erases
/// `[U8]` into an integer list, so the root type and normalized field schema
/// must cross the evaluator seam with the value.
pub fn cbor_encode_typed_for_tir(
    value: &CtValue,
    root_ty: &Type,
    struct_fields: &HashMap<String, Vec<(String, Type)>>,
    canonical: bool,
) -> Result<Vec<u8>, String> {
    EncodingLite::cbor_encode_typed(value, Some(root_ty), struct_fields, canonical)
}

/// TIR/JIT bridge for whole-value CBOR encoding. Keep the wire encoder in the
/// same comptime-reachable Prelude implementation used by interpreter calls.
pub fn cbor_encode_for_tir(
    value: &CtValue,
    canonical: bool,
) -> Result<Vec<u8>, CtValue> {
    if canonical {
        EncodingLite::cbor_encode_canonical(value)
    } else {
        EncodingLite::cbor_encode(value)
    }
    .map_err(EncodingLite::cbor_error_value)
}

pub fn render_datatree_for_tir(value: &CtValue) -> String {
    JSONInterp::render_json_pretty(value, false, 0)
}

pub fn render_datatree_pretty_for_tir(value: &CtValue) -> String {
    JSONInterp::render_json_pretty(value, true, 0)
}

/// TIR/JIT bridge for the canonical whole-value CBOR parser.
///
/// Keep options validation, limits, deterministic-form checks, and error
/// construction in the same implementation used by comptime evaluation.
pub fn cbor_parse_for_tir(
    bytes: &[u8],
    options: Option<&CtValue>,
    allow_bytes: bool,
) -> Result<CtValue, CtValue> {
    let options =
        EncodingLite::cbor_options(options).map_err(EncodingLite::cbor_error_value)?;
    EncodingLite::cbor_decode(bytes, &options, allow_bytes)
        .map_err(EncodingLite::cbor_error_value)
}

/// TIR/JIT bridge for the text codecs' parse-failure wording. `codec` is the
/// name as the Prelude writes it — `JSON`, `TOML`, `YAML` — so a typed decode
/// reports `invalid JSON (line 3): …` on every tier from one implementation.
pub fn codec_parse_error_for_tir(codec: &str, error: CtValue) -> CtValue {
    TypedDecode::json_parse_err_to_decode(codec, error)
}

/// Convert a parser failure to the typed decoder's shared `[FieldError]`
/// contract. CBOR's parser keeps byte offsets and `$` paths; typed decode
/// exposes those details in the field-error reason and uses source paths.
pub fn cbor_decode_source_error_for_tir(error: CtValue) -> CtValue {
    let CtValue::Struct { fields, .. } = &error else {
        return TypedDecode::decode_error(error.jet_show());
    };
    let text = |name: &str| {
        fields.iter().find_map(|(field, value)| {
            (field == name).then_some(value).and_then(|value| match value {
                CtValue::Str(value) => Some(value.as_str()),
                _ => None,
            })
        })
    };
    let kind = fields
        .iter()
        .find_map(|(field, value)| {
            (field == "kind").then_some(value).and_then(|value| match value {
                CtValue::Enum { variant, .. } => Some(variant.as_str()),
                _ => None,
            })
        })
        .unwrap_or("Unsupported");
    let offset = fields
        .iter()
        .find_map(|(field, value)| {
            (field == "byte_offset").then_some(value).and_then(|value| match value {
                CtValue::Int(offset) => Some(*offset),
                _ => None,
            })
        })
        .unwrap_or(0);
    let raw_path = text("path").unwrap_or("$");
    let path = if raw_path == "$" {
        String::new()
    } else if let Some(path) = raw_path.strip_prefix("$.") {
        path.to_string()
    } else {
        raw_path.strip_prefix('$').unwrap_or(raw_path).to_string()
    };
    let reason = text("reason").unwrap_or("CBOR decode failed");
    TypedDecode::decode_error_at(path, format!("CBOR {kind} at byte {offset}: {reason}"))
}

/// TIR core-call bridge for generic CBOR decode. TIR retains the resolved
/// `Result<T, [FieldError]>` type, so use its `T` with the shared typed decoder
/// instead of returning the parser's internal `DataTree` representation.
pub fn cbor_decode_typed_for_tir(
    bytes: &[u8],
    options: Option<&CtValue>,
    root_ty: &Type,
) -> CtValue {
    let options = match EncodingLite::cbor_options(options) {
        Ok(options) => options,
        Err(error) => {
            return CtValue::failed(Box::new(cbor_decode_source_error_for_tir(
                EncodingLite::cbor_error_value(error),
            )));
        }
    };
    let tree = match EncodingLite::cbor_decode(bytes, &options, true) {
        Ok(tree) => tree,
        Err(error) => {
            return CtValue::failed(Box::new(cbor_decode_source_error_for_tir(
                EncodingLite::cbor_error_value(error),
            )));
        }
    };
    match TypedDecode::typed_decode_builtin_value(root_ty, &tree) {
        Some(Ok(value)) => CtValue::Present(Box::new(value)),
        Some(Err(error)) => CtValue::failed(Box::new(error)),
        None => CtValue::failed(Box::new(TypedDecode::decode_error(format!(
            "comptime can't decode `{}` yet",
            root_ty.name()
        )))),
    }
}

/// TIR static-call bridge for shared EncodingLimits / XML safe constructors.
pub fn xml_safe_static_for_tir(path: &str, method: &str) -> Option<CtValue> {
    if method != "safe" {
        return None;
    }
    match path {
        "jet_std::EncodingLimits" => Some(EncodingLite::encoding_limits_safe_value()),
        "jet_std::XMLLimits" => Some(EncodingLite::xml_safe_limits_value()),
        "jet_std::XMLParseOptions" => Some(EncodingLite::xml_safe_options_value()),
        "jet_std::XMLRenderOptions" => Some(EncodingLite::xml_safe_render_options_value()),
        _ => None,
    }
}

/// TIR static-call bridge for the shared Email limits constructor.
pub fn email_safe_static_for_tir(path: &str, method: &str) -> Option<CtValue> {
    (method == "safe" && path == "jet_email::Limits")
        .then(EmailAdapter::limits_safe_value)
}
static EMPTY_COMPUTED: std::sync::OnceLock<HashMap<(String, String), &'static Expr>> =
    std::sync::OnceLock::new();
fn empty_computed() -> &'static HashMap<(String, String), &'static Expr> {
    EMPTY_COMPUTED.get_or_init(HashMap::new)
}
static EMPTY_DISTINCT: std::sync::OnceLock<HashMap<String, Option<(i64, i64)>>> =
    std::sync::OnceLock::new();
fn empty_distinct() -> &'static HashMap<String, Option<(i64, i64)>> {
    EMPTY_DISTINCT.get_or_init(HashMap::new)
}
static EMPTY_DISTINCT_BASES: std::sync::OnceLock<HashMap<String, Type>> =
    std::sync::OnceLock::new();
fn empty_distinct_bases() -> &'static HashMap<String, Type> {
    EMPTY_DISTINCT_BASES.get_or_init(HashMap::new)
}
static EMPTY_MIGRATIONS: std::sync::OnceLock<
    HashMap<String, Vec<&'static crate::AST::MigrationDecl>>,
> = std::sync::OnceLock::new();
fn empty_migrations() -> &'static HashMap<String, Vec<&'static crate::AST::MigrationDecl>> {
    EMPTY_MIGRATIONS.get_or_init(HashMap::new)
}

// --- public entry ---------------------------------------------------------

/// Type-check happens elsewhere (every function body goes through sema);
/// this checks purity then evaluates `init` to a constant value.
pub fn evaluate(
    init: &crate::AST::Expr,
    funcs: &HashMap<String, &Func>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
    globals: &HashMap<String, CtValue>,
) -> Result<CtValue, Diagnostic> {
    evaluate_with_imports(
        init,
        funcs,
        extern_names,
        base_dir,
        globals,
        &HashMap::new(),
    )
}

/// Like `evaluate` but with module aliases for effect-approved Core calls.
pub fn evaluate_with_imports(
    init: &crate::AST::Expr,
    funcs: &HashMap<String, &Func>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
    globals: &HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
) -> Result<CtValue, Diagnostic> {
    evaluate_with_imports_opts(
        init,
        funcs,
        extern_names,
        base_dir,
        globals,
        core_imports,
        false,
        0,
    )
}

/// Like `evaluate_with_imports` but with `allow_impure` and `initial_impure_depth`
/// for D-CTEFFECT1. When called from inside a sema `#Impure` block, pass
/// `initial_impure_depth: 1` (and `allow_impure: true`) so the interpreter
/// starts with the gate already open for Tier-2 calls.
pub fn evaluate_with_imports_opts(
    init: &crate::AST::Expr,
    funcs: &HashMap<String, &Func>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
    globals: &HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    allow_impure: bool,
    initial_impure_depth: usize,
) -> Result<CtValue, Diagnostic> {
    // Only run the purity check when there is no active #Impure gate (i.e.
    // the expression is not nested inside a `#Impure` block at sema time).
    // When initial_impure_depth > 0, the gate is active — skip check_purity
    // so that Tier-2 calls fire E3411 ("gate present, flag absent") instead
    // of the bare E3401 impurity gate, giving a better fix message.
    if initial_impure_depth == 0 {
        check_purity(init, funcs, extern_names)?;
    }
    TirBridge::eval_expr(&mut TirBridge::ExprEvalRequest {
        expr: init,
        funcs,
        methods: empty_methods(),
        extern_names,
        base_dir,
        globals,
        core_imports,
        allow_impure,
        initial_impure_depth,
        structs: &HashMap::new(),
        computed_fields: empty_computed(),
        distinct_ranges: empty_distinct(),
        distinct_bases: empty_distinct_bases(),
        fuel: FUEL_BUDGET,
        sink: None,
        repl_mode: false,
        repl_grants: &[],
        repl_authorizer: None,
        emitted_fragments: None,
        embed_inputs: None,
        mutated: None,
    })
}

/// Like [`evaluate_with_imports_opts`] but also returns the Tier-1 embed
/// inputs accumulated during evaluation (D-CTEFFECT1). The caller (sema
/// Checker) drains these into `CompileOutput.comptime_inputs`.
pub fn evaluate_with_imports_opts_collecting(
    init: &crate::AST::Expr,
    funcs: &HashMap<String, &Func>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
    globals: &HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    allow_impure: bool,
    initial_impure_depth: usize,
    mutated: Option<&mut HashMap<String, CtValue>>,
) -> Result<(CtValue, Vec<crate::AST::ComptimeInput>), Diagnostic> {
    evaluate_with_imports_opts_collecting_structs(
        init,
        funcs,
        extern_names,
        base_dir,
        globals,
        core_imports,
        allow_impure,
        initial_impure_depth,
        empty_structs(),
        mutated,
    )
}

/// Whole-item comptime evaluation variant. Codable encoding needs declared
/// field types so `[U8]` retains byte-string identity after CtValue erasure.
pub fn evaluate_with_imports_opts_collecting_structs<'a>(
    init: &crate::AST::Expr,
    funcs: &HashMap<String, &'a Func>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
    globals: &HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    allow_impure: bool,
    initial_impure_depth: usize,
    structs: &HashMap<String, &'a StructDef>,
    mutated: Option<&mut HashMap<String, CtValue>>,
) -> Result<(CtValue, Vec<crate::AST::ComptimeInput>), Diagnostic> {
    if initial_impure_depth == 0 {
        check_purity(init, funcs, extern_names)?;
    }
    let mut embed_inputs = Vec::new();
    let val = TirBridge::eval_expr(&mut TirBridge::ExprEvalRequest {
        expr: init,
        funcs,
        methods: empty_methods(),
        extern_names,
        base_dir,
        globals,
        core_imports,
        allow_impure,
        initial_impure_depth,
        structs,
        computed_fields: empty_computed(),
        distinct_ranges: empty_distinct(),
        distinct_bases: empty_distinct_bases(),
        fuel: FUEL_BUDGET,
        sink: None,
        repl_mode: false,
        repl_grants: &[],
        repl_authorizer: None,
        emitted_fragments: None,
        embed_inputs: Some(&mut embed_inputs),
        mutated,
    })?;
    Ok((val, embed_inputs))
}

/// Whole-program dev interpretation (E2-M4 `jet dev`). Runs `main`'s body
/// with a buffered stdout/stderr sink, reusing the exact same evaluator the
/// M9.5 comptime path uses — there is no second interpreter. Output bytes are
/// produced via `CtValue::jet_show()` + `\n`, identical to the compiled
/// program (the differential battery in `tests/dev.rs` enforces this, I2).
///
/// The caller (src/interp.rs) is responsible for the E2201 boundary scan
/// (FFI/tasks/`#Unsafe`); this function simply runs and may itself return
/// E0956 (`unsupported`) when it reaches a construct the evaluator can't run,
/// or E2202 when the fuel budget is exhausted.
/// c139: everything the dev interpreter needs beyond the flat `funcs` map to
/// run whole programs at parity with the real build — pre-evaluated
/// top-level `const`/`comptime` bindings, user-method dispatch (`impl`/
/// in-struct methods and D-MOD2 code-module namespaced calls), D-FIELDPOL1
/// computed fields, and D-RANGETYPE1/D-DIST1 distinct-type constructors.
/// Built once per `jet dev` run by `Source/Interpreter.rs::collect_program_info`.
pub struct ProgramInfo<'a> {
    pub globals: HashMap<String, CtValue>,
    pub methods: HashMap<(String, String), &'a Func>,
    pub structs: HashMap<String, &'a StructDef>,
    pub enums: HashMap<String, &'a EnumDef>,
    pub computed_fields: HashMap<(String, String), &'a Expr>,
    pub distinct_ranges: HashMap<String, Option<(i64, i64)>>,
    pub distinct_bases: HashMap<String, Type>,
    pub core_imports: HashMap<String, String>,
    /// Card #392 pass 5: `TypeName -> migration { }` blocks (source order) for
    /// `decode_traced<T>`'s runtime chain-walker (see `Interp::migrations`).
    pub migrations: HashMap<String, Vec<&'a crate::AST::MigrationDecl>>,
}

impl<'a> ProgramInfo<'a> {
    pub fn empty() -> Self {
        ProgramInfo {
            globals: HashMap::new(),
            methods: HashMap::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
            computed_fields: HashMap::new(),
            distinct_ranges: HashMap::new(),
            distinct_bases: HashMap::new(),
            core_imports: HashMap::new(),
            migrations: HashMap::new(),
        }
    }
}

pub fn run_main(
    main: &Func,
    funcs: &HashMap<String, &Func>,
    base_dir: &Path,
    sink: &mut DevSink,
    _program: &ProgramInfo,
) -> Result<CtValue, Diagnostic> {
    // #777: AST tree-walker entry retired — same TirBridge path as REPL/debug.
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel: DEV_FUEL_BUDGET,
        sink: Some(sink),
        core_imports: empty_imports(),
        debugger: None,
        depth: 0,
        cur_func: "main".to_string(),
        impure_depth: 0,
        allow_impure: true,
        repl_mode: false,
        repl_grants: Vec::new(),
        repl_authorizer: None,
        repl_interruptible: false,
        embed_inputs: Vec::new(),
        emitted_fragments: Vec::new(),
        binding_types: HashMap::new(),
        globals: empty_globals(),
        methods: empty_methods(),
        structs: empty_structs(),
        computed_fields: empty_computed(),
        distinct_ranges: empty_distinct(),
        distinct_bases: empty_distinct_bases(),
        migrations: empty_migrations(),
        list_write_windows: HashMap::new(),
    };
    let mut scope = HashMap::new();
    match interp.exec_block(&main.body, &mut scope)? {
        Interpreter::Flow::Return(v) => Ok(v),
        _ => Ok(CtValue::Unit),
    }
}

/// D-DBG3: whole-program interpretation under the source-level debugger.
/// Identical to [`run_main`] (same evaluator, same buffered sink, same I2
/// bytes) except a [`DebugHook`] is attached: the driver is notified before
/// every statement and may pause to run its `(jet)` prompt. The driver shows
/// only Jet lines/locals — it never sees generated Rust. Returns the same
/// E2202 (fuel) / E0956 (unsupported) stops, plus any abort the driver raises
/// (e.g. the user typed `quit`, surfaced as E2204).
pub fn run_main_debug(
    main: &Func,
    funcs: &HashMap<String, &Func>,
    base_dir: &Path,
    sink: &mut DevSink,
    debugger: &mut dyn DebugHook,
) -> Result<(), Diagnostic> {
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel: DEV_FUEL_BUDGET,
        sink: Some(sink),
        core_imports: empty_imports(),
        debugger: Some(debugger),
        depth: 0,
        cur_func: "main".to_string(),
        impure_depth: 0,
        allow_impure: false,
        repl_mode: false,
        repl_grants: Vec::new(),
        repl_authorizer: None,
        repl_interruptible: false,
        embed_inputs: Vec::new(),
        emitted_fragments: Vec::new(),
        binding_types: HashMap::new(),
        globals: empty_globals(),
        methods: empty_methods(),
        structs: empty_structs(),
        computed_fields: empty_computed(),
        distinct_ranges: empty_distinct(),
        distinct_bases: empty_distinct_bases(),
        migrations: empty_migrations(),
        list_write_windows: HashMap::new(),
    };
    let mut scope = HashMap::new();
    // TirBridge `exec_block` evaluates a whole block without per-statement
    // `DebugHook` callbacks. Drive top-level body statements through
    // `exec_stmt` so `jet debug` / Canvas debug sessions can stop, step, and
    // hit breakpoints (D-DBG3). Nested blocks inside a single statement still
    // use TirBridge today; call/loop body stepping is a follow-on.
    for stmt in &main.body {
        match interp.exec_stmt(stmt, &mut scope)? {
            Interpreter::Flow::Normal => {}
            Interpreter::Flow::Return(_) => return Ok(()),
            Interpreter::Flow::Break
            | Interpreter::Flow::Continue
            | Interpreter::Flow::BreakLabel(_)
            | Interpreter::Flow::ContinueLabel(_) => {}
        }
    }
    Ok(())
}

/// `jet eval --pure` variant: runs `main()` and returns its return value as a
/// `CtValue` instead of buffering stdout. Used when the caller wants to render
/// the value (pretty or JSON) rather than capture print output. Any print
/// calls are still captured but discarded; the return value is authoritative.
pub fn run_main_value(
    main: &Func,
    funcs: &HashMap<String, &Func>,
    base_dir: &Path,
    sink: &mut DevSink,
) -> Result<CtValue, Diagnostic> {
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel: DEV_FUEL_BUDGET,
        sink: Some(sink),
        core_imports: empty_imports(),
        debugger: None,
        depth: 0,
        cur_func: "main".to_string(),
        impure_depth: 0,
        allow_impure: false,
        repl_mode: false,
        repl_grants: Vec::new(),
        repl_authorizer: None,
        repl_interruptible: false,
        embed_inputs: Vec::new(),
        emitted_fragments: Vec::new(),
        binding_types: HashMap::new(),
        globals: empty_globals(),
        methods: empty_methods(),
        structs: empty_structs(),
        computed_fields: empty_computed(),
        distinct_ranges: empty_distinct(),
        distinct_bases: empty_distinct_bases(),
        migrations: empty_migrations(),
        list_write_windows: HashMap::new(),
    };
    let mut scope = HashMap::new();
    match interp.exec_block(&main.body, &mut scope)? {
        Interpreter::Flow::Return(v) => Ok(v),
        _ => Ok(CtValue::Unit),
    }
}

/// REPL variant of `run_main`: uses a caller-supplied fuel cap so the REPL
/// can enforce D-REPL-FUEL without patching DEV_FUEL_BUDGET. Returns the
/// same E2202 (dev fuel stop) or E0956 (unsupported) errors; the REPL
/// intercepts E2202 and upgrades it to E1801 with REPL-specific wording.
pub fn run_main_with_fuel(
    main: &Func,
    funcs: &HashMap<String, &Func>,
    base_dir: &Path,
    sink: &mut DevSink,
    fuel: u64,
) -> Result<(), Diagnostic> {
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel,
        sink: Some(sink),
        core_imports: empty_imports(),
        debugger: None,
        depth: 0,
        cur_func: "main".to_string(),
        impure_depth: 0,
        allow_impure: false,
        repl_mode: false,
        repl_grants: Vec::new(),
        repl_authorizer: None,
        repl_interruptible: false,
        embed_inputs: Vec::new(),
        emitted_fragments: Vec::new(),
        binding_types: HashMap::new(),
        globals: empty_globals(),
        methods: empty_methods(),
        structs: empty_structs(),
        computed_fields: empty_computed(),
        distinct_ranges: empty_distinct(),
        distinct_bases: empty_distinct_bases(),
        migrations: empty_migrations(),
        list_write_windows: HashMap::new(),
    };
    let mut scope = HashMap::new();
    interp.exec_block(&main.body, &mut scope)?;
    Ok(())
}

/// REPL `:run` transcript path: like `run_main_with_fuel` but with the REPL
/// sandbox (Tier-2 I/O, accumulated `core_imports`) so materialized sessions
/// replay the same semantics as interactive inputs.
pub fn run_repl_main_with_fuel(
    main: &Func,
    funcs: &HashMap<String, &Func>,
    base_dir: &Path,
    sink: &mut DevSink,
    fuel: u64,
    core_imports: &HashMap<String, String>,
) -> Result<(), Diagnostic> {
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel,
        sink: Some(sink),
        core_imports,
        debugger: None,
        depth: 0,
        cur_func: "main".to_string(),
        impure_depth: 1,
        allow_impure: true,
        repl_mode: true,
        repl_grants: Vec::new(),
        repl_authorizer: None,
        repl_interruptible: false,
        embed_inputs: Vec::new(),
        emitted_fragments: Vec::new(),
        binding_types: HashMap::new(),
        globals: empty_globals(),
        methods: empty_methods(),
        structs: empty_structs(),
        computed_fields: empty_computed(),
        distinct_ranges: empty_distinct(),
        distinct_bases: empty_distinct_bases(),
        migrations: empty_migrations(),
        list_write_windows: HashMap::new(),
    };
    let mut scope = HashMap::new();
    interp.exec_block(&main.body, &mut scope)?;
    Ok(())
}

/// REPL per-input step (E2-M18). Executes `stmts` inside a running REPL
/// session. Differs from `run_main_with_fuel` in two ways:
///
/// 1. The scope is passed in *and mutated* — accumulated bindings survive
///    across inputs (D-REPL7: one accumulating module).
/// 2. If the last statement is a bare `Stmt::Expr` (not `Stmt::Val`), the
///    evaluated value is returned so the caller can display
///    `x: T = v` (D-REPL16=B).
///
/// The `suppress` flag implements `;` at end of input — the caller strips the
/// trailing `;` to detect a bare expression and passes `suppress = false`; a
/// statement ending in `;` passes `suppress = true`.
///
/// D-META-EFFECT1: `core_imports` maps alias → Core module path (e.g. `"math"`
/// → `"core.math"`) from the session's accumulated `use` declarations, so
/// effect-approved implemented Core calls (e.g. `math.sqrt(16.0)`) execute
/// inline instead of raising E0956. Pass `&HashMap::new()` when no imports
/// are active.
pub fn run_repl_step(
    stmts: &[crate::AST::Stmt],
    funcs: &HashMap<String, &Func>,
    base_dir: &Path,
    sink: &mut DevSink,
    scope: &mut HashMap<String, CtValue>,
    fuel: u64,
    suppress: bool,
    core_imports: &HashMap<String, String>,
    structs: &HashMap<String, &StructDef>,
    binding_types: &HashMap<String, Type>,
    authorizer: &mut dyn ReplAuthorizer,
) -> Result<Option<CtValue>, Diagnostic> {
    match run_repl_step_inner(
        stmts,
        funcs,
        base_dir,
        sink,
        scope,
        fuel,
        suppress,
        core_imports,
        structs,
        binding_types,
        authorizer,
        false,
    ) {
        Ok(value) => Ok(value),
        Err(ReplStepError::Diagnostic(d)) => Err(d),
        Err(ReplStepError::Interrupted) => unreachable!("non-interruptible REPL step interrupted"),
    }
}

/// Raw interactive variant. Ctrl-C is polled at every interpreter burn and
/// returned as control flow, not rendered as a compiler diagnostic.
pub fn run_repl_step_interruptible(
    stmts: &[crate::AST::Stmt],
    funcs: &HashMap<String, &Func>,
    base_dir: &Path,
    sink: &mut DevSink,
    scope: &mut HashMap<String, CtValue>,
    fuel: u64,
    suppress: bool,
    core_imports: &HashMap<String, String>,
    structs: &HashMap<String, &StructDef>,
    binding_types: &HashMap<String, Type>,
    authorizer: &mut dyn ReplAuthorizer,
) -> Result<Option<CtValue>, ReplStepError> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_repl_step_inner(
            stmts,
            funcs,
            base_dir,
            sink,
            scope,
            fuel,
            suppress,
            core_imports,
            structs,
            binding_types,
            authorizer,
            true,
        )
    }));
    match result {
        Ok(result) => result,
        Err(payload) if payload.is::<ReplInterrupted>() => Err(ReplStepError::Interrupted),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

fn run_repl_step_inner(
    stmts: &[crate::AST::Stmt],
    funcs: &HashMap<String, &Func>,
    base_dir: &Path,
    sink: &mut DevSink,
    scope: &mut HashMap<String, CtValue>,
    fuel: u64,
    suppress: bool,
    core_imports: &HashMap<String, String>,
    structs: &HashMap<String, &StructDef>,
    binding_types: &HashMap<String, Type>,
    authorizer: &mut dyn ReplAuthorizer,
    interruptible: bool,
) -> Result<Option<CtValue>, ReplStepError> {
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel,
        sink: Some(sink),
        core_imports,
        debugger: None,
        depth: 0,
        cur_func: "main".to_string(),
        // D-REPLCOREEFFECT1=A: only a lexical `#Grant` opens this depth.
        impure_depth: 0,
        allow_impure: true,
        repl_mode: true,
        repl_grants: Vec::new(),
        repl_authorizer: Some(authorizer),
        repl_interruptible: interruptible,
        embed_inputs: Vec::new(),
        emitted_fragments: Vec::new(),
        // Fresh Interp per turn — seed prior binding types so empty
        // `data.table`/`data.schema` can still read `Table<T>` / `[T]`.
        binding_types: binding_types.clone(),
        globals: empty_globals(),
        methods: empty_methods(),
        structs,
        computed_fields: empty_computed(),
        distinct_ranges: empty_distinct(),
        distinct_bases: empty_distinct_bases(),
        migrations: empty_migrations(),
        list_write_windows: HashMap::new(),
    };
    // Split: run all statements except the last; then handle the last specially
    // if it is a bare expression (for display) and not suppressed.
    let (last, head) = match stmts.split_last() {
        Some(pair) => pair,
        None => return Ok(None),
    };
    interp.exec_block(head, scope).map_err(ReplStepError::Diagnostic)?;
    // Determine if the last statement should produce an echo value.
    // Case 1: `Stmt::Val` named `__repl_echo__` — the sentinel that `classify`
    //   injects for bare-expression inputs (e.g. `1 + 2` → `__repl_echo__ :: 1 + 2`).
    //   Evaluate but don't add to the persistent scope.
    // Case 2: bare `Stmt::Expr` — retained for forward-compat.
    let echo_bare = !suppress && matches!(last, crate::AST::Stmt::Expr(_));
    match last {
        crate::AST::Stmt::Val(b) if !suppress && b.name == "__repl_echo__" => {
            let v = interp.eval(&b.init, scope).map_err(ReplStepError::Diagnostic)?;
            Ok(Some(v))
        }
        crate::AST::Stmt::Val(b) => {
            let v = interp.eval(&b.init, scope).map_err(ReplStepError::Diagnostic)?;
            if let Some(pat) = &b.pattern {
                interp.bind_pattern(pat, v, scope).map_err(ReplStepError::Diagnostic)?;
            } else {
                scope.insert(b.name.clone(), v);
            }
            Ok(None)
        }
        crate::AST::Stmt::Expr(e) if echo_bare => {
            let v = interp.eval(e, scope).map_err(ReplStepError::Diagnostic)?;
            Ok(Some(v))
        }
        other => {
            interp.exec_stmt(other, scope).map_err(ReplStepError::Diagnostic)?;
            Ok(None)
        }
    }
}

/// D-META-STAGE1=B (formerly D-CTMARKER1, ratified 2026-06-25, piece 2): run a `$ { … }` block at
/// build time. Purity-checked (E3401) then tree-walked with fuel cap (E0952).
/// Pure path only (Stage A); effect tiers wire in c157 (D-CTEFFECT1).
pub fn run_block_with_imports(
    stmts: &[crate::AST::Stmt],
    funcs: &HashMap<String, Func>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
    globals: &HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
) -> Result<HashMap<String, CtValue>, Diagnostic> {
    let refs: HashMap<String, &Func> = funcs.iter().map(|(n, f)| (n.clone(), f)).collect();
    Purity::check_purity_stmts(stmts, &refs, extern_names)?;
    match TirBridge::eval_block(&mut TirBridge::BlockEvalRequest {
        stmts,
        funcs: &refs,
        methods: empty_methods(),
        extern_names,
        base_dir,
        globals,
        core_imports,
        structs: &HashMap::new(),
        computed_fields: empty_computed(),
        distinct_ranges: empty_distinct(),
        distinct_bases: empty_distinct_bases(),
        fuel: FUEL_BUDGET,
        sink: None,
        repl_mode: false,
        repl_grants: &[],
        repl_authorizer: None,
        allow_impure: false,
        impure_depth: 0,
        emitted_fragments: None,
        embed_inputs: None,
    })? {
        TirBridge::StmtOutcome::Done(scope) => Ok(scope),
        TirBridge::StmtOutcome::Returned { scope, .. } => Ok(scope),
    }
}

/// Owned-function variant used while sema is mutating function bodies for
/// local `comptime` bindings. The cloned function map is a snapshot of the
/// already-parsed program; the interpreter still sees ordinary Jet AST.
pub fn evaluate_owned(
    init: &crate::AST::Expr,
    funcs: &HashMap<String, Func>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
    globals: &HashMap<String, CtValue>,
) -> Result<CtValue, Diagnostic> {
    evaluate_owned_with_imports(
        init,
        funcs,
        extern_names,
        base_dir,
        globals,
        &HashMap::new(),
    )
}

/// Like `evaluate_owned` but with module aliases for effect-approved Core calls.
pub fn evaluate_owned_with_imports(
    init: &crate::AST::Expr,
    funcs: &HashMap<String, Func>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
    globals: &HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
) -> Result<CtValue, Diagnostic> {
    evaluate_owned_with_imports_opts(
        init,
        funcs,
        extern_names,
        base_dir,
        globals,
        core_imports,
        false,
        0,
    )
}

/// Like `evaluate_owned_with_imports` but with D-CTEFFECT1 `allow_impure` flag
/// and `initial_impure_depth`. Pass `initial_impure_depth: 1` when evaluating a
/// comptime binding inside a `#Impure` block so the interpreter starts with the
/// gate already open for Tier-2 calls.
pub fn evaluate_owned_with_imports_opts(
    init: &crate::AST::Expr,
    funcs: &HashMap<String, Func>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
    globals: &HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    allow_impure: bool,
    initial_impure_depth: usize,
) -> Result<CtValue, Diagnostic> {
    let reachable = Purity::reachable_owned_funcs(init, funcs);
    let refs: HashMap<String, &Func> =
        reachable.iter().map(|(name, function)| (name.clone(), function)).collect();
    evaluate_with_imports_opts(
        init,
        &refs,
        extern_names,
        base_dir,
        globals,
        core_imports,
        allow_impure,
        initial_impure_depth,
    )
}

/// Like [`evaluate_owned_with_imports_opts`] but also returns Tier-1 embed
/// inputs (D-CTEFFECT1). Used by the sema Checker to collect embed hashes.
pub fn evaluate_owned_with_imports_opts_collecting(
    init: &crate::AST::Expr,
    funcs: &HashMap<String, Func>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
    globals: &HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    allow_impure: bool,
    initial_impure_depth: usize,
    mutated: Option<&mut HashMap<String, CtValue>>,
) -> Result<(CtValue, Vec<crate::AST::ComptimeInput>), Diagnostic> {
    let reachable = Purity::reachable_owned_funcs(init, funcs);
    let refs: HashMap<String, &Func> =
        reachable.iter().map(|(name, function)| (name.clone(), function)).collect();
    evaluate_with_imports_opts_collecting(
        init,
        &refs,
        extern_names,
        base_dir,
        globals,
        core_imports,
        allow_impure,
        initial_impure_depth,
        mutated,
    )
}

/// D-METADERIVE1=A: evaluate the body of a user-authored `derive T.Trait { … }`
/// block in a comptime scope where `type_param` is bound to `type_info`.
/// Returns the source fragments emitted by `emit(…)` calls (D-CTCODEGEN1=A).
pub fn evaluate_derive_body(
    body: &[crate::AST::Stmt],
    type_param: &str,
    type_info: CtValue,
    funcs: &HashMap<String, &Func>,
    base_dir: &Path,
) -> Result<Vec<String>, Diagnostic> {
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel: FUEL_BUDGET,
        sink: None,
        core_imports: empty_imports(),
        debugger: None,
        depth: 0,
        cur_func: "derive".to_string(),
        impure_depth: 0,
        allow_impure: false,
        repl_mode: false,
        repl_grants: Vec::new(),
        repl_authorizer: None,
        repl_interruptible: false,
        embed_inputs: Vec::new(),
        emitted_fragments: Vec::new(),
        binding_types: HashMap::new(),
        globals: empty_globals(),
        methods: empty_methods(),
        structs: empty_structs(),
        computed_fields: empty_computed(),
        distinct_ranges: empty_distinct(),
        distinct_bases: empty_distinct_bases(),
        migrations: empty_migrations(),
        list_write_windows: HashMap::new(),
    };
    let mut scope = HashMap::new();
    scope.insert(type_param.to_string(), type_info);
    interp.exec_block(body, &mut scope)?;
    Ok(interp.emitted_fragments)
}

#[cfg(test)]
mod typed_head_tests {
    use super::{validate_datetime_literal, validate_url_literal};

    #[test]
    fn boundary_heads_use_the_canonical_validation_kernels() {
        assert!(validate_url_literal("https://api.example.com/v2/jet-hole").is_ok());
        assert!(validate_url_literal("https:").is_err());
        assert!(validate_datetime_literal("2026-08-07T12:00:00Z").is_ok());
        assert!(validate_datetime_literal("2026-08-07T12:00:00").is_err());
    }
}
