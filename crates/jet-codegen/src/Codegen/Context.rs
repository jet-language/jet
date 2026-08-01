use super::*;
use crate::Diagnostics::Span;
use crate::Generics;
use crate::Syntax;
use crate::AST::FfiLink;
use crate::AST::{
    AccessConvention, CtValue, EnumDef, Func, Item, Program, ProgramBundle, StructDef, Type,
    VariantField, VariantPayload,
};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
pub(crate) struct UnitFact {
    pub(crate) family: String,
    /// `None` for a nominal family (D-DIMENSION-OPEN1=D).
    pub(crate) dimension: Option<crate::AST::Dimension>,
    pub(crate) kind: crate::AST::QuantityKind,
    pub(crate) scale: crate::AST::UnitRatio,
    pub(crate) offset: crate::AST::UnitRatio,
}

#[derive(Clone)]
pub(crate) struct UnitLabel {
    pub(crate) symbol: String,
    pub(crate) name: String,
    pub(crate) family: String,
    pub(crate) is_base: bool,
}

fn unit_label(
    family: &crate::AST::UnitFamilyDef,
    member: &crate::AST::UnitFamilyMember,
) -> UnitLabel {
    let base = family
        .base
        .as_ref()
        .map(|base| base.0.as_str())
        .or_else(|| family.members.first().map(|member| member.name.as_str()));
    UnitLabel {
        symbol: member.name.clone(),
        name: crate::AST::UnitFamilyDef::type_name(&member.name),
        family: family.family.clone(),
        is_base: base == Some(member.name.as_str()),
    }
}

fn unit_family_member_for_type<'a>(
    family: &'a crate::AST::UnitFamilyDef,
    type_name: &str,
    kind: crate::AST::QuantityKind,
) -> Option<&'a crate::AST::UnitFamilyMember> {
    family.members.iter().find(|member| {
        let stem = crate::AST::UnitFamilyDef::type_name(&member.name);
        type_name
            == match kind {
                crate::AST::QuantityKind::Linear => stem,
                crate::AST::QuantityKind::Point => format!("{stem}Point"),
                crate::AST::QuantityKind::Delta => format!("{stem}Delta"),
            }
    })
}

fn unit_fact(
    family: &crate::AST::UnitFamilyDef,
    member: &crate::AST::UnitFamilyMember,
    dimension: Option<crate::AST::Dimension>,
    kind: crate::AST::QuantityKind,
) -> UnitFact {
    UnitFact {
        family: family.family.clone(),
        dimension,
        kind,
        scale: member.scale.clone(),
        offset: if kind == crate::AST::QuantityKind::Point {
            member.offset.clone()
        } else {
            crate::AST::UnitRatio::zero()
        },
    }
}

pub(crate) struct Cx {
    /// Top-level function name -> parameter conventions+types.
    pub(crate) sigs: HashMap<String, Vec<(AccessConvention, Type)>>,
    /// Top-level function name -> function value type (M8).
    pub(crate) fn_types: HashMap<String, Type>,
    /// `(TypeName, method)` -> parameter conventions+types (including `self`).
    pub(crate) method_sigs: HashMap<(String, String), Vec<(AccessConvention, Type)>>,
    pub(crate) method_self_convs: HashMap<(String, String), AccessConvention>,
    /// c109 Phase 6 (TIR): `(TypeName, method)` -> resolved return type (or `None`
    /// for a unit-returning method). Used by TIR lowering to give a method-call
    /// expression its total result `Type` without re-inferring in codegen.
    pub(crate) method_rets: HashMap<(String, String), Option<Type>>,
    pub(crate) consts: HashMap<String, String>,
    /// The evaluated value behind each comptime const, so lowering can hand a
    /// structured literal to every engine instead of the rendered Rust text.
    pub(crate) const_values: HashMap<String, CtValue>,
    pub(crate) type_names: HashSet<String>,
    /// D-DIST1 (c109 Phase 23): distinct-type name -> (base type, is_numeric). A
    /// distinct type renders to a `#[repr(transparent)]` newtype `user_<Name>(pub
    /// Base)`; the TIR reads the base type to give `.raw()` (`(recv).0`) its total
    /// result type, and `is_numeric` is informational (the arithmetic operator is
    /// chosen by `ast_operand_is_integer`, which returns `None` for a distinct).
    pub(crate) distinct_types: HashMap<String, (Type, bool)>,
    /// D-RANGETYPE1: range-constrained distinct type name -> inclusive bounds.
    pub(crate) distinct_ranges: HashMap<String, (i64, i64)>,
    /// D-SHAPE-QUANTITY1=A: the one backend registry for physical unit facts.
    /// Consulted only while lowering; facts erase from emitted Rust.
    pub(crate) unit_facts: HashMap<String, UnitFact>,
    /// D-QUANTITY-PRINT1: labels for all unit-family types.
    pub(crate) unit_labels: HashMap<String, UnitLabel>,
    /// D-TYPEALIAS1: transparent generic alias name -> (params, target).
    pub(crate) type_aliases: HashMap<String, (Vec<crate::AST::TypeParam>, Type)>,
    pub(crate) trait_names: HashSet<String>,
    pub(crate) struct_fields: HashMap<String, Vec<(String, Type)>>,
    /// Generic parameters that occur in non-skipped serialized fields.
    pub(crate) serde_wire_params: HashMap<String, HashSet<String>>,
    pub(crate) enum_variants: HashMap<String, Vec<(String, VariantPayload)>>,
    /// variant name -> owning enum type (for pattern lowering)
    pub(crate) variant_owner: HashMap<String, String>,
    /// Recursive-type edges that need `Box<…>` in Rust (`(owner, edge_key)`).
    pub(crate) boxed_edges: HashSet<(String, String)>,
    pub(crate) cloneable: HashSet<String>,
    /// D-MIGRATE4: `migration TypeName { … }` blocks per type, in source order
    /// (the chain, oldest step first). Read by `emit_struct_migration` to lower
    /// the runtime step functions + `jet_decode_traced` chain-walker.
    pub(crate) migrations: HashMap<String, Vec<crate::AST::MigrationDecl>>,
    /// D-SOA1: struct names declared `#layout(columnar)`. A `[S]` of such a
    /// struct lowers to the generated `user_<S>_columns` struct-of-arrays type;
    /// `rust_type` maps the list type and the list ops route to its inherent API.
    pub(crate) columnar: HashSet<String>,
    pub(crate) comparable: HashSet<String>,
    pub(crate) auto_printable: HashSet<String>,
    pub(crate) auto_debug: HashSet<String>,
    pub(crate) auto_equatable: HashSet<String>,
    /// D-TAG1: types whose fields are all Eq+Hash-capable (comparable minus
    /// float fields) — gates `derive(Eq, Hash)` for `Bag<T>` keys.
    pub(crate) hashable: HashSet<String>,
    /// S55: explicit `derive Comparable;` → PartialOrd in Rust.
    pub(crate) partial_ord: HashSet<String>,
    pub(crate) patchable: HashSet<String>,
    /// D-FIELDPOL1: struct name -> computed field names. Sema already
    /// synthesized a `fn <field>(self) => T` getter for each on `s.methods`
    /// (`Sema::CheckerFieldPolicy`); this set is consulted at every
    /// `Expr::Field`/`LValue::Field` lowering site so a read of the field
    /// emits a call to that getter instead of a struct member access — the
    /// field simply isn't a Rust struct member (see `emit_struct`).
    pub(crate) computed_fields: HashMap<String, HashSet<String>>,
    pub(crate) src: String,
    pub(crate) file: String,
    /// When true, `require`/`require_eq` unwind instead of exiting (test bodies).
    pub(crate) test_mode: bool,
    /// D-COV1: `jet test --coverage`. When true, every emitted user function head
    /// gets a `jet_cov(line)` probe and the harness carries the coverage recorder
    /// + dump. Never set in normal builds, so codegen output is byte-identical
    /// (golden tests never touch this path).
    pub(crate) coverage: bool,
    /// Import alias -> Rust module name (`user_scoring`).
    pub(crate) import_mods: HashMap<String, String>,
    /// Cross-module pub type name -> Rust module path (e.g. `Note` -> `user_note`).
    pub(crate) foreign_types: HashMap<String, String>,
    /// D-MOD4: `(alias, item)` -> `(real Rust module, real fn)` for `pub use`
    /// re-exports, so `text.wrap` lowers to the module that actually defines it.
    pub(crate) reexport_calls: HashMap<(String, String), (String, String)>,
    /// `(import alias, function)` -> parameter conventions for cross-module calls.
    pub(crate) import_sigs: HashMap<(String, String), Vec<(AccessConvention, Type)>>,
    /// c109 Phase 14: `(import alias, function)` -> the function's return type, so the
    /// TIR can carry a total result type for a cross-module call (mirrors `import_sigs`).
    pub(crate) import_rets: HashMap<(String, String), Option<Type>>,
    /// Import alias -> compiler-known core module (`core.files`, `core.json`, ...).
    pub(crate) core_imports: HashMap<String, String>,
    /// M10 helpers proven reachable by sema.
    pub(crate) used_core: HashSet<String>,
    /// D-CABI-CALLBACK1: top-level function names sema proved are passed as a
    /// stable C callback symbol at some `#Extern` call site. Emission must give
    /// exactly these functions `extern "C" fn` — never every `#Pure fn` (that
    /// leaked the purity lever into codegen and broke I3 erasure; 14dd68a5).
    pub(crate) ffi_callback_fns: HashSet<String>,
    /// Empty at the entry module, `super::` inside generated import modules.
    pub(crate) root_prefix: String,
    /// M7: rustc crate name for the FFI bridge (`jet_ffi_…`).
    pub(crate) ffi_crate: Option<String>,
    /// M7: Jet function name -> wrapper symbol in the FFI crate.
    pub(crate) extern_funcs: HashMap<String, String>,
    /// D-MOD2: inline code module aliases in scope (alias → module name).
    pub(crate) code_modules: HashSet<String>,
    /// D-MOD3: unqualified inline-module items (name → "alias__method").
    pub(crate) unqualified_inline: HashMap<String, String>,
    /// D-MOD3: unqualified file-module items (name → (rust_mod_name, fn_name)).
    pub(crate) unqualified_file: HashMap<String, (String, String)>,
    /// S62/M9: (TypeName, method_name) pairs that come from trait impls — these
    /// are called without the `user_` prefix in Rust (the trait impl owns the name).
    pub(crate) trait_methods: HashSet<(String, String)>,
    /// D-TXN-ROLLBACK layer 2: user types that implement the `Rollback` trait.
    /// Populated in `build_cx_items` from `Item::Impl` blocks with
    /// `trait_name == Some("Rollback")` and from inline `struct { impl Rollback }`.
    pub(crate) rollback_types: HashSet<String>,
    /// D-DISPLAYDBG1: user types with an explicit `impl Type.Display`.
    pub(crate) display_types: HashSet<String>,
    /// D-SHAPE-RESOURCE2=A: user types with an ordinary nominal `Close` impl.
    pub(crate) close_types: HashSet<String>,
    /// D-ITER-HOOK: `for x in coll` on types implementing `Iterable`.
    pub(crate) iterable_hooks: HashMap<String, IterableHook>,
    /// D-INDEX-HOOK: `coll[k]` on types implementing `Index`.
    pub(crate) index_hooks: HashMap<String, IndexHook>,
    /// E2-M12 D-OBS1: name of the Jet function currently being emitted, so
    /// jet_panic_rich can include the function name in the panic report.
    pub(crate) current_fn: std::cell::RefCell<String>,
    /// c148: struct name → its declared type-parameter names. Populated in
    /// `build_cx_items` from `StructDef.type_params`. Lets `struct_is_generic` and
    /// field-type checks recognize multi-char type params (`Kind`, `Elem`, …).
    pub(crate) struct_type_params: HashMap<String, HashSet<String>>,
    /// The same parameters in declaration order, for substituting a concrete
    /// `Struct<A, B>` receiver into its declared field types.
    pub(crate) struct_type_param_order: HashMap<String, Vec<String>>,
    /// c148: type-parameter names for the function currently being emitted. Set
    /// from `f.type_params` at the start of `emit_func` so `rust_type` and
    /// `rust_param_type` can recognize multi-char params without the single-letter
    /// heuristic. Cleared when emit returns.
    pub(crate) current_type_params: std::cell::RefCell<HashSet<String>>,
    /// c139 M4: spawn lambda bodies collected during TIR lowering (JIT order).
    pub(crate) jit_spawn_lambdas: std::cell::RefCell<Vec<crate::Codegen::TIR::TJitSpawnLambda>>,
    /// Global offset for spawn sites lowered from an imported module.
    pub(crate) jit_spawn_site_base: usize,
    /// Concrete generic owner methods reached while lowering executable TIR.
    /// The key is `Owner<Args>::method`, keeping discovery deterministic.
    pub(crate) jit_method_calls:
        std::cell::RefCell<std::collections::BTreeMap<String, (Type, String)>>,
    /// Concrete argument types at calls to generic free functions. TIR uses
    /// these facts to admit one native specialization per concrete call shape.
    pub(crate) jit_generic_calls:
        std::cell::RefCell<std::collections::BTreeMap<String, Vec<Vec<Type>>>>,
    /// Functions whose typed decode depends on the canonical TIR migration
    /// plan. The resident codec has no authority to reinterpret that plan.
    pub(crate) jit_canonical_deopt: std::cell::RefCell<HashSet<String>>,
    /// Functions whose codec calls must stay on TIR if the function deopts.
    pub(crate) jit_canonical_calls: std::cell::RefCell<HashSet<String>>,
    /// Resident-only prefix for calls between functions in an imported module.
    pub(crate) jit_local_call_prefix: Option<String>,
    /// Free-function type parameter names, used to give generic call results
    /// their concrete TIR type at the call site.
    pub(crate) fn_type_params: HashMap<String, HashSet<String>>,
    /// D-DBG3 step 2 (dap-debugger): when true, `lower_stmts` interleaves a
    /// `TStmt::LineMarker` before every lowered statement, and emission turns each
    /// into a `// jet:line N` comment. Set ONLY by the native `jet debug` build path
    /// (`emit_bundle_dbg`); false (the default) is byte-identical to today's output,
    /// so normal builds, `jet test`, and the JIT tier never see a marker.
    pub(crate) debug_linemap: bool,
    /// D-ANY-JAI1/D-VARARGBOUND1 (c7jaiany): trait-bounded variadic function
    /// name -> (fixed param count, resolved trait-bound list). Populated once
    /// from each `Item::Func`'s trailing param; call-site lowering
    /// (`TIR/lower.rs`) reads this to route to the per-arity monomorphized
    /// function `Codegen/VariadicBound.rs` synthesizes instead of the normal
    /// single Rust function a plain generic gets.
    pub(crate) variadic_bound_fns: HashMap<String, (usize, Vec<String>)>,
    /// Arities actually called, discovered while lowering ordinary function
    /// bodies — the one traversal already guaranteed to visit every call site,
    /// so this can never miss one the way a separate scan could. Drained after
    /// the main function-emission pass to emit exactly the specializations
    /// that are needed (`Codegen/VariadicBound.rs`). `BTreeMap`/`BTreeSet` (not
    /// `Hash*`) so the emission order is deterministic — golden output must be
    /// byte-stable across runs.
    pub(crate) needed_variadic_arities:
        std::cell::RefCell<std::collections::BTreeMap<String, std::collections::BTreeSet<usize>>>,
    /// D-OSTARGET1=A (ratified 2026-07-01, c134): the native OS bucket this
    /// build is compiling for — an `impl` gated to a different `#Target(OS.*)`
    /// is skipped entirely (mirrors how `Codegen/Web.rs` filters by
    /// `WebBucket`). Defaults to the host OS; the real build pipeline
    /// (`emit_bundle_dbg`) overwrites it from the resolved `--target=<triple>`.
    pub(crate) active_os: crate::Syntax::OSTarget,
    /// D-ENC712: resolved package edition for encoding surface dispatch.
    pub(crate) package_edition: String,
    /// D-STM1=A (card #506): true while lowering the body of a `#Transact` block,
    /// so a `Shared<T>.edit(f)` inside it routes to the deferred `edit_txn` (the
    /// atomic Shared plane) instead of taking a lock immediately. Set/restored
    /// around the block body in `lower_stmt`'s `Stmt::Transact` arm.
    pub(crate) in_stm_transact: std::cell::Cell<bool>,
    /// D-STM1=A (card #506): set true when a `Shared.edit` inside the current
    /// `#Transact` body routed to `edit_txn` — i.e. the block actually touches the
    /// Shared plane and so needs the `jet_stm::begin()/commit()` scaffold emitted.
    /// Save/restored per block so each `#Transact` reports its own use.
    pub(crate) stm_touched: std::cell::Cell<bool>,
}

pub(crate) const MOD_USE: &str = "use super::{JetShow, JetDisplay, JetDebug, JetArith, jet_panic, jet_panic_rich, jet_trace_err, jet_index_vec, jet_index_vec_mut, jet_views_mut_new, jet_views_mut_range_new, jet_split_write, jet_get_disjoint_write, jet_edit_disjoint, jet_unpack_vec, jet_slice_vec, jet_index_map, jet_map_insert, jet_map_merge, jet_map_merge_with, jet_list_remove, jet_char_len, jet_string_split, jet_string_lines, jet_string_after, jet_string_before, jet_string_slice, jet_list_map, jet_list_map_mut, jet_list_filter, jet_list_each, jet_list_each_ref, jet_list_each_mut, jet_list_find, jet_list_any, jet_list_all, jet_list_sort_by, jet_list_reduce, jet_map_each, jet_list_take, jet_list_skip, jet_list_step_by, jet_list_dedup, jet_list_chunks, jet_list_windows, jet_list_sum, jet_list_product, jet_list_flatten, jet_list_intersperse, jet_list_count_by, jet_list_take_while, jet_list_skip_while, jet_list_flat_map, jet_list_scan, jet_list_fold, jet_list_position, jet_list_min_by, jet_list_max_by, jet_list_group_by, jet_list_partition, jet_list_para_map, jet_list_para_filter, jet_list_para_partition, jet_list_para_fold, JetIter, jet_iter_from_vec, jet_iter_string_split, jet_iter_take, jet_iter_skip, jet_iter_step_by, jet_iter_dedup, jet_iter_chunks, jet_iter_windows, jet_iter_map, jet_iter_map_mut, jet_iter_filter, jet_iter_take_while, jet_iter_skip_while, jet_iter_flat_map, jet_iter_filter_map, jet_iter_scan, jet_iter_flatten, jet_iter_intersperse, jet_iter_enumerate, jet_iter_indexes, jet_iter_zip};\n\n";

/// D-ITER-HOOK: metadata for zero-copy `for x in mytype` lowering.
#[derive(Debug, Clone)]
pub(crate) struct IterableHook {
    pub iter_type: String,
    pub item_type: Type,
}

/// D-INDEX-HOOK: metadata for expert `mytype[k]` lowering.
#[derive(Debug, Clone)]
pub(crate) struct IndexHook {
    pub value_type: Type,
}

// D-ENC-DYN1=A+: the dynamic encoding value `Data` (+ aliases `JSON`/`TOML`/
// `YAML`/`CSV`) is the user-facing face of `jet_std::DataTree`.
pub(crate) fn is_json_type_name(name: &str) -> bool {
    Syntax::is_data_type_name(name)
}

// D-DBDRIVER1: the `DBValue` dynamic tagged SQL value — same construction
// mechanism as `Data`/`JSON`, mirrored via `jet_std::DBValue`.
pub(crate) fn is_db_value_type_name(name: &str) -> bool {
    Syntax::is_db_value_type_name(name)
}

pub(crate) fn core_rust_type_name(name: &str) -> Option<&'static str> {
    match name {
        n if is_json_type_name(n) => Some("DataTree"),
        n if n == Syntax::TYPE_JSON_ERROR || n == "JSONError" => Some("JSONError"),
        n if n == Syntax::TYPE_IO_ERROR || n == "IOError" => Some("IOError"),
        n if n == Syntax::TYPE_IO_CONTEXT => Some("IOContext"),
        n if n == Syntax::TYPE_IO_OPERATION => Some("IOOperation"),
        "EnvError" => Some("EnvError"),
        n if n == Syntax::TYPE_UTF8_ERROR || n == "UTF8Error" => Some("UTF8Error"),
        "ProcessResult" => Some("ProcessResult"),
        "ProcessSpec" => Some("ProcessSpec"),
        "ProcessChild" => Some("ProcessChild"),
        // D-PROCESS1=A: the core dot-literal stream-mode enum.
        "ProcessStreamMode" => Some("ProcessStreamMode"),
        // D-PROCESS-SESSION1=A / D-PROCESS-SESSION2=D: expert terminal
        // controls on the existing process model.
        "TerminalPolicy" => Some("TerminalPolicy"),
        "TerminalSize" => Some("TerminalSize"),
        "TerminalMode" => Some("TerminalMode"),
        "TerminalSession" => Some("TerminalSession"),
        // D-TEXTWIDTH1=B: the `text.display_width` policy value + its two
        // dot-literal enum fields, plus the reject-path error struct.
        "TextWidth" => Some("TextWidth"),
        "TextWidthAmbiguous" => Some("TextWidthAmbiguous"),
        "TextWidthControls" => Some("TextWidthControls"),
        "TextError" => Some("TextError"),
        "AsyncPolicy" => Some("JetAsyncPolicy"),
        "Overflow" => Some("JetEventOverflow"),
        "FailurePolicy" => Some("JetFailurePolicy"),
        "DispatchState" => Some("JetDispatchState"),
        "HookPolicy" => Some("JetHookPolicy"),
        "EventConfigError" => Some("JetEventConfigError"),
        "Stopwatch" => Some("Stopwatch"),
        // D-DET1: deterministic injected capability handles.
        "Clock" => Some("Clock"),
        "Rng" => Some("Rng"),
        // D-SOLVER-LIB1=A: explicit finite solver state.
        "Solver" => Some("Solver"),
        // D-SHAPE-DURATION1/D-SHAPE-DURATIONCONVERT1: checked duration values.
        "Duration" => Some("Duration"),
        "DurationUnit" => Some("DurationUnit"),
        "RangeError" => Some("RangeError"),
        "Instant" => Some("JetInstant"),
        "Date" | "LocalDate" => Some("JetDate"),
        "LocalTime" => Some("JetLocalTime"),
        "DateTime" => Some("JetDateTime"),
        "Period" => Some("JetPeriod"),
        "Zone" => Some("JetZone"),
        "ZonedDateTime" => Some("JetZonedDateTime"),
        "Url" => Some("JetURL"),
        "Mime" => Some("JetMIME"),
        "Regex" => Some("JetRegex"),
        "RegexFlags" => Some("RegexFlags"),
        "Match" => Some("JetRegexMatch"),
        // D-BIGINT1 / D-DECIMAL1: precise numerics.
        "BigInt" => Some("JetBigInt"),
        "Decimal" => Some("JetDecimal"),
        "Closed" => Some("Closed"),
        // D-LSDIR1=A: fs.list_dir returns [DirEntry].
        "DirEntry" => Some("DirEntry"),
        // D-FSOPS1 / D-WATCH-SCOPE1: typed filesystem and watcher values.
        "Stat" => Some("Stat"),
        "WalkEntry" => Some("WalkEntry"),
        "WatchEvent" => Some("WatchEvent"),
        "WatchHandle" => Some("WatchHandle"),
        "WatchSet" => Some("WatchSet"),
        "TempDir" => Some("TempDir"),
        "TempFile" => Some("TempFile"),
        "FileLock" => Some("FileLock"),
        // D-DATA-SURFACE1=A / D-DATA-STATUS1=A: core.data summary/status values.
        "DataGroup" => Some("DataGroup"),
        "DataColumn" => Some("DataColumn"),
        "DataStatus" => Some("DataStatus"),
        "DataSummary" => Some("DataSummary"),
        // D-DATAFLOW1=A: typed streaming + invalid-data policy values.
        "DataLimits" => Some("DataLimits"),
        "DataError" => Some("DataError"),
        "DataErrorKind" => Some("DataErrorKind"),
        "DataPivotCell" => Some("DataPivotCell"),
        "DataStream" => Some("DataStream"),
        // D-LOGTRACE1=A: structured logging values.
        "LogField" => Some("LogField"),
        "LogSpan" => Some("LogSpan"),
        // D-SERDE2: the format-agnostic value tree + typed-decode error live in jet_std.
        "DataTree" => Some("DataTree"),
        "DecodeError" => Some("DecodeError"),
        // D-VALIDATE1: the accumulated validation error lives in jet_std too.
        "FieldError" => Some("FieldError"),
        "EncodingLimits" => Some("EncodingLimits"),
        "EncodingError" => Some("EncodingError"),
        "EncodingCause" => Some("EncodingCause"),
        "EncodingFormat" => Some("EncodingFormat"),
        "EncodingErrorKind" => Some("EncodingErrorKind"),
        "CBOROptions" => Some("CBOROptions"),
        "CBORError" => Some("CBORError"),
        "CBORErrorKind" => Some("CBORErrorKind"),
        "XMLLimits" => Some("XMLLimits"),
        "XMLParseOptions" => Some("XMLParseOptions"),
        "XMLRenderOptions" => Some("XMLRenderOptions"),
        "XMLEncoding" => Some("XMLEncoding"),
        "XMLLexicalPolicy" => Some("XMLLexicalPolicy"),
        "XMLCanonical" => Some("XMLCanonical"),
        "XMLCanonicalMode" => Some("XMLCanonicalMode"),
        "XMLError" => Some("XMLError"),
        "XMLReason" => Some("XMLReason"),
        "XMLEntityPolicy" => Some("XMLEntityPolicy"),
        "DataEvent" => Some("DataEvent"),
        "JSONReader" => Some("JSONReader"),
        "JSONWriter" => Some("JSONWriter"),
        "JSONLReader" => Some("JSONLReader"),
        "JSONLWriter" => Some("JSONLWriter"),
        "CSVReader" => Some("CSVReader"),
        "CSVWriter" => Some("CSVWriter"),
        "XMLReader" => Some("XMLReader"),
        "XMLWriter" => Some("XMLWriter"),
        "CBORReader" => Some("CBORReader"),
        "CBORWriter" => Some("CBORWriter"),
        // D-MIGRATE3=A: decode-time migration transparency's plain status struct
        // (the generic `DecodeResult<T>` has its own `rust_type` arm below, since
        // this table only covers non-generic names).
        "MigrationStatus" => Some("MigrationStatus"),
        // D-DBDRIVER1: the tagged SQL parameter/column value + its error type.
        "DBValue" => Some("DBValue"),
        "DBError" => Some("DBError"),
        // D-RAYLIB1=A: display-gated graphics bridge types.
        "RaylibWindow" => Some("RaylibWindow"),
        "RaylibColor" => Some("RaylibColor"),
        "RaylibSound" => Some("RaylibSound"),
        // D-TYPEDTEXT1=D: `SQL`/`HTML` — this table's `.is_some()` is only a
        // "known core value type" gate for the TIR subset check; the actual Rust
        // spelling for these two comes from the earlier explicit `rust_type` arms
        // (`(String, Vec<String>)` / `String`), not this placeholder.
        "SQL" => Some("SQL"),
        "HTML" => Some("HTML"),
        "Sh" => Some("Sh"),
        // D-SIMD2 / D-LINALG1: built-in math value types (lane + linalg structs).
        "F32x4" => Some("F32x4"),
        "F64x2" => Some("F64x2"),
        "Vec2" => Some("Vec2"),
        "Vec3" => Some("Vec3"),
        "Vec4" => Some("Vec4"),
        "Mat3" => Some("Mat3"),
        "Mat4" => Some("Mat4"),
        _ => None,
    }
}

pub(crate) fn core_crypto_type_name(name: &str) -> Option<&'static str> {
    match name {
        "Secret" => Some("Secret"),
        "SigningKey" => Some("SigningKey"),
        "VerifyKey" => Some("VerifyKey"),
        "X25519SecretKey" => Some("X25519SecretKey"),
        "X25519PublicKey" => Some("X25519PublicKey"),
        "SharedSecret" => Some("SharedSecret"),
        "Signature" => Some("Signature"),
        "Sealed" => Some("Sealed"),
        "WrappedKey" => Some("WrappedKey"),
        "WrappedVaultKey" => Some("WrappedVaultKey"),
        "KeyUnlock" => Some("KeyUnlock"),
        "PasswordHash" => Some("PasswordHash"),
        "Digest256" => Some("Digest256"),
        "Digest512" => Some("Digest512"),
        "CryptoError" => Some("CryptoError"),
        "FileCryptoError" => Some("FileCryptoError"),
        "KeyWrapError" => Some("KeyWrapError"),
        _ => None,
    }
}

pub(crate) fn core_crypto_rust_type_name(name: &str) -> Option<&'static str> {
    let name = core_crypto_type_name(name)?;
    match name {
        "Secret" => Some("Secret"),
        "SigningKey" => Some("JetSigningKey"),
        "VerifyKey" => Some("JetVerifyKey"),
        "X25519SecretKey" => Some("JetX25519SecretKey"),
        "X25519PublicKey" => Some("JetX25519PublicKey"),
        "SharedSecret" => Some("JetSharedSecret"),
        "Signature" => Some("JetSignature"),
        "Sealed" => Some("JetSealed"),
        "WrappedKey" => Some("JetWrappedKey"),
        "WrappedVaultKey" => Some("JetWrappedVaultKey"),
        "KeyUnlock" => Some("JetVaultKeyUnlock<'_>"),
        "PasswordHash" => Some("JetPasswordHash"),
        "Digest256" => Some("JetDigest256"),
        "Digest512" => Some("JetDigest512"),
        "CryptoError" => Some("JetCryptoError"),
        "FileCryptoError" => Some("JetFileCryptoError"),
        "KeyWrapError" => Some("JetVaultKeyWrapError"),
        _ => None,
    }
}

/// D-LAYOUT1 / D-LAYOUT-GATES1: `layout` runtime types live in their own
/// top-level `mod jet_layout` (NOT nested inside `mod jet_std`, unlike
/// `core_rust_type_name`'s entries) — same reason `alloc_handle_rust_type`/
/// `file_handle_rust_type`/`net_handle_rust_type` are separate functions
/// rather than folded into `core_rust_type_name` (that table's caller always
/// prepends `jet_std::`). `HVar`/`VVar`/`LengthVar` all erase to the SAME
/// runtime type (`jet_layout::LinExpr`) — the axis distinction is
/// compile-time-only (sema, GATE 1/2), nothing to represent at runtime.
pub(crate) fn layout_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "HVar" => Some("jet_layout::LinExpr"),
        "VVar" => Some("jet_layout::LinExpr"),
        "LengthVar" => Some("jet_layout::LinExpr"),
        "Constraint" => Some("jet_layout::Constraint"),
        "Layout" => Some("jet_layout::Handle"),
        _ => None,
    }
}

/// E2-M7: file handle types are top-level in the prelude (not in `jet_std`).
pub(crate) fn file_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "FileReader" => Some("JetFileReader"),
        "FileWriter" => Some("JetFileWriter"),
        // FileLines is an internal sema marker; it should never appear in emitted Rust.
        "FileLines" => Some("()"),
        // D-STDIN1=A: stdin handle types; StdinLines is an internal sema marker.
        "StdinHandle" => Some("JetStdinReader"),
        "StdinLines" => Some("()"),
        // D-PROCESS1=A: `child.stdin`/`.stdout`/`.stderr` handle markers — the real
        // Rust value comes straight off the `ProcessChild` struct field (see
        // `core_struct_field_rust_name`); these Jet-level types never appear as a
        // standalone Rust type. `ProcessLines` is the `.lines()` loop-only marker.
        "ProcessStdin" => Some("()"),
        "ProcessStdoutStream" => Some("()"),
        "ProcessStderrStream" => Some("()"),
        "ProcessLines" => Some("()"),
        // D-COREIO1=A: standard stream handles.
        "Stdout" => Some("JetStdout"),
        "Stderr" => Some("JetStderr"),
        // D-PATHFS1: typed path handle.
        "Path" => Some("JetPath"),
        // D-DBDRIVER1: the SQLite connection handle wrapper.
        "DBConnection" => Some("JetDbConnection"),
        // D-DEP-WASM1=A / D-PLUGIN1=B (c81): the sandboxed WASM plugin handle.
        "Plugin" => Some("JetPlugin"),
        _ => None,
    }
}

/// D-RAYLIB1=A: raylib handle/value types are top-level prelude
/// structs, like file/net handles, not members of `mod jet_std`.
pub(crate) fn raylib_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "RaylibWindow" => Some("RaylibWindow"),
        "RaylibColor" => Some("RaylibColor"),
        "RaylibSound" => Some("RaylibSound"),
        _ => None,
    }
}

pub(crate) fn game_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "GameScene" => Some("GameScene"),
        "GameAssets" => Some("GameAssets"),
        "GameInputMap" => Some("GameInputMap"),
        "GameBackend" => Some("GameBackend"),
        "GameReplay" => Some("GameReplay"),
        "GameImage" => Some("GameImage"),
        "GameSound" => Some("GameSound"),
        "GameFrame" => Some("GameFrame"),
        "GameInputSnapshot" => Some("GameInputSnapshot"),
        _ => None,
    }
}

/// D-COMPUTE1=D: compute opaque types map to top-level prelude structs.
pub(crate) fn compute_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "Tensor" => Some("JetTensor"),
        "ComputeError" => Some("JetComputeError"),
        "ComputeDevice" => Some("JetComputeDevice"),
        "ComputeStream" => Some("JetComputeStream"),
        "GradTriple" => Some("JetComputeGradTriple"),
        "SparseTensor" => Some("JetSparseCsr"),
        _ => None,
    }
}

/// D-SERVICE1=D: service topology opaque types.
pub(crate) fn service_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "ServiceTree" => Some("JetServiceTree"),
        "ServiceEndpoint" => Some("JetServiceEndpoint"),
        "ServiceError" => Some("JetServiceError"),
        "ServiceRestart" => Some("JetServiceRestart"),
        "ServiceDelivery" => Some("JetServiceDelivery"),
        _ => None,
    }
}

/// E2-M10: networking opaque types map to top-level prelude structs.
pub(crate) fn net_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "TcpListener" => Some("JetTCPListener"),
        "TcpStream" => Some("JetTCPStream"),
        "IPAddr" => Some("JetIpAddr"),
        "SocketAddr" => Some("JetSocketAddr"),
        "UdpSocket" => Some("JetUDPSocket"),
        "UDPPacket" => Some("JetUDPPacket"),
        "DNSSrv" => Some("JetDNSSrv"),
        "UnixListener" => Some("JetUnixListener"),
        "UnixStream" => Some("JetUnixStream"),
        "TLSStream" => Some("JetTLSStream"),
        "TLSClientConfig" => Some("JetTLSClientConfig"),
        "TLSRootCertificates" => Some("JetTLSRootCertificates"),
        "TLSClientIdentity" => Some("JetTLSClientIdentity"),
        "TLSClientTrust" => Some("JetTLSTrust"),
        "TLSVersion" => Some("JetTLSVersion"),
        "TLSPeerIdentity" => Some("JetTLSPeerIdentity"),
        "TLSCertificate" => Some("JetTLSCertificate"),
        "NetError" => Some("JetNetError"),
        "NetErrorDetail" => Some("JetNetErrorDetail"),
        "NetDnsError" => Some("JetNetDnsError"),
        "NetShutdown" => Some("JetNetShutdown"),
        "NetReadyInterest" => Some("JetNetReadyInterest"),
        "NetReady" => Some("JetNetReady"),
        "HTTPRequest" => Some("JetHTTPRequest"),
        "HTTPResponse" => Some("JetHTTPResponse"),
        "HTTPClient" => Some("JetHTTPClient"),
        "HTTPProxy" => Some("JetHTTPProxy"),
        "HTTPRedirectPolicy" => Some("JetHTTPRedirectPolicy"),
        "HTTPRetryPolicy" => Some("JetHTTPRetryPolicy"),
        "HTTPCookieJar" => Some("JetHTTPCookieJar"),
        "HTTPCorsPolicy" => Some("JetHTTPCorsPolicy"),
        "HTTPCorsOrigins" => Some("JetHTTPCorsOrigins"),
        "HTTPCompressEncoding" => Some("JetHTTPCompressEncoding"),
        "HTTPRouter" => Some("JetHTTPRouter"),
        _ => None,
    }
}

// Re-export from Syntax so submodules (lower.rs, subset.rs) find them via `use super::*`.
pub(crate) use crate::Syntax::alloc_handle_rust_type;
pub(crate) use crate::Syntax::args_handle_rust_type;
pub(crate) use crate::Syntax::binary_text_handle_rust_type;
pub(crate) use crate::Syntax::reflect_handle_rust_type;

impl Cx {
    fn imported_type_metadata_name(&self, name: &str) -> Option<String> {
        let (qualifier, leaf) = name.split_once('.')?;
        let module = self.import_mods.get(qualifier)?.strip_prefix("user_")?;
        Some(format!("{module}.{leaf}"))
    }

    pub(crate) fn quantity_dimension(&self, ty: &Type) -> Option<crate::AST::Dimension> {
        ty.quantity_parts()
            .map(|(_, dimension)| dimension)
            .or_else(|| {
                let Type::Named(name) = ty else {
                    return None;
                };
                self.unit_facts
                    .get(name)
                    .or_else(|| {
                        self.imported_type_metadata_name(name)
                            .and_then(|canonical| self.unit_facts.get(&canonical))
                    })
                    .and_then(|fact| fact.dimension.clone())
            })
    }

    pub(crate) fn unit_label(&self, ty: &Type) -> Option<&UnitLabel> {
        let Type::Named(name) = ty else {
            return None;
        };
        self.unit_labels.get(name).or_else(|| {
            self.imported_type_metadata_name(name)
                .and_then(|canonical| self.unit_labels.get(&canonical))
        })
    }

    pub(crate) fn has_display_type(&self, name: &str) -> bool {
        self.display_types.contains(name)
            || self
                .imported_type_metadata_name(name)
                .is_some_and(|canonical| self.display_types.contains(&canonical))
    }

    pub(crate) fn is_distinct_type_name(&self, name: &str) -> bool {
        self.distinct_types.contains_key(name)
            || self
                .imported_type_metadata_name(name)
                .is_some_and(|canonical| self.distinct_types.contains_key(&canonical))
    }

    pub(crate) fn quantity_unit_label(
        &self,
        dimension: crate::AST::Dimension,
        style: crate::AST::UnitFormat,
    ) -> String {
        let mut numerator = Vec::new();
        let mut denominator = Vec::new();
        for (axis, exponent) in dimension.axes() {
            let family = axis.rsplit("::").next().unwrap_or(axis);
            if exponent == 0 {
                continue;
            }
            let label = self
                .unit_labels
                .values()
                .filter(|label| label.family == family && label.is_base)
                .min_by(|left, right| left.symbol.cmp(&right.symbol))
                .map(|label| match style {
                    crate::AST::UnitFormat::Name => label.name.clone(),
                    crate::AST::UnitFormat::Symbol | crate::AST::UnitFormat::Bare => {
                        label.symbol.clone()
                    }
                })
                .unwrap_or_else(|| family.to_ascii_lowercase());
            let part = if exponent.abs() == 1 {
                label
            } else {
                format!("{label}^{}", exponent.abs())
            };
            if exponent > 0 {
                numerator.push(part);
            } else {
                denominator.push(part);
            }
        }
        let numerator = if numerator.is_empty() {
            "1".to_string()
        } else {
            numerator.join("*")
        };
        if denominator.is_empty() {
            numerator
        } else {
            format!("{numerator}/{}", denominator.join("*"))
        }
    }

    pub(crate) fn core_qualified_rust_type_name(&self, name: &str) -> Option<&'static str> {
        let (alias, leaf) = name.split_once('.')?;
        match (self.core_imports.get(alias).map(String::as_str), leaf) {
            (Some("core.crypto" | "jet.crypto"), leaf) => core_crypto_type_name(leaf),
            (Some("core.auth"), "Claims") => Some("Claims"),
            (Some("core.auth"), "AuthError") => Some("AuthError"),
            (Some("core.http.client"), "Proxy") => Some("HTTPProxy"),
            (Some("core.http.client"), "RedirectPolicy") => Some("HTTPRedirectPolicy"),
            (Some("core.http.client"), "RetryPolicy") => Some("HTTPRetryPolicy"),
            (Some("core.http.client"), "CookieJar") => Some("HTTPCookieJar"),
            (Some("core.tls"), "TLSVersion") => Some("TLSVersion"),
            (Some("core.tls"), "RootCertificates") => Some("TLSRootCertificates"),
            (Some("core.tls"), "ClientIdentity") => Some("TLSClientIdentity"),
            (Some("core.env"), "EnvError") => Some("EnvError"),
            (Some("core.encoding"), "DataTree") => Some("DataTree"),
            (Some("core.encoding"), "EncodingLimits") => Some("EncodingLimits"),
            (Some("core.encoding"), "EncodingError") => Some("EncodingError"),
            (Some("core.encoding"), "EncodingCause") => Some("EncodingCause"),
            (Some("core.encoding"), "EncodingFormat") => Some("EncodingFormat"),
            (Some("core.encoding"), "EncodingErrorKind") => Some("EncodingErrorKind"),
            (Some("core.encoding"), "DataEvent") => Some("DataEvent"),
            (Some("core.email"), "Address") => Some("Address"),
            (Some("core.email"), "Message") => Some("Message"),
            (Some("core.email"), "Attachment") => Some("Attachment"),
            (Some("core.email"), "Envelope") => Some("Envelope"),
            (Some("core.email"), "SMTPSecurity") => Some("SMTPSecurity"),
            (Some("core.email"), "Limits") => Some("Limits"),
            (Some("core.email"), "RecipientPolicy") => Some("RecipientPolicy"),
            (Some("core.email"), "RecipientReport") => Some("RecipientReport"),
            (Some("core.email"), "SendReport") => Some("SendReport"),
            (Some("core.email"), "EmailError") => Some("EmailError"),
            (Some("core.email"), "SMTPAuth") => Some("SMTPAuth"),
            (Some("core.email"), "TLSTrust") => Some("TLSTrust"),
            (Some("core.email"), "SMTPConfig") => Some("SMTPConfig"),
            (Some("core.email"), "DkimConfig") => Some("DkimConfig"),
            (Some("core.email"), "Mailer") => Some("Mailer"),
            (Some("core.encoding.json"), "JSONReader") => Some("JSONReader"),
            (Some("core.encoding.json"), "JSONWriter") => Some("JSONWriter"),
            (Some("core.encoding.jsonl"), "JSONLReader") => Some("JSONLReader"),
            (Some("core.encoding.jsonl"), "JSONLWriter") => Some("JSONLWriter"),
            (Some("core.encoding.csv"), "CSVReader") => Some("CSVReader"),
            (Some("core.encoding.csv"), "CSVWriter") => Some("CSVWriter"),
            (Some("core.encoding.xml"), "XMLReader") => Some("XMLReader"),
            (Some("core.encoding.xml"), "XMLWriter") => Some("XMLWriter"),
            (Some("core.encoding.xml"), "XMLLimits") => Some("XMLLimits"),
            (Some("core.encoding.xml"), "XMLParseOptions") => Some("XMLParseOptions"),
            (Some("core.encoding.xml"), "XMLRenderOptions") => Some("XMLRenderOptions"),
            (Some("core.encoding.xml"), "XMLEncoding") => Some("XMLEncoding"),
            (Some("core.encoding.xml"), "XMLLexicalPolicy") => Some("XMLLexicalPolicy"),
            (Some("core.encoding.xml"), "XMLCanonical") => Some("XMLCanonical"),
            (Some("core.encoding.xml"), "XMLCanonicalMode") => Some("XMLCanonicalMode"),
            (Some("core.encoding.xml"), "XMLError") => Some("XMLError"),
            (Some("core.encoding.xml"), "XMLReason") => Some("XMLReason"),
            (Some("core.encoding.xml"), "XMLEntityPolicy") => Some("XMLEntityPolicy"),
            (Some("core.encoding.cbor"), "CBORReader") => Some("CBORReader"),
            (Some("core.encoding.cbor"), "CBOROptions") => Some("CBOROptions"),
            (Some("core.encoding.cbor"), "CBORError") => Some("CBORError"),
            (Some("core.encoding.cbor"), "CBORErrorKind") => Some("CBORErrorKind"),
            (Some("core.encoding.cbor"), "CBORWriter") => Some("CBORWriter"),
            _ => None,
        }
    }
    pub(crate) fn type_contains_view(&self, ty: &Type) -> bool {
        self.type_contains_view_matching(ty, false)
    }

    pub(crate) fn type_contains_mutable_view(&self, ty: &Type) -> bool {
        self.type_contains_view_matching(ty, true)
    }

    fn type_contains_view_matching(&self, ty: &Type, mutable_only: bool) -> bool {
        fn payload_contains(
            cx: &Cx,
            payload: &VariantPayload,
            seen: &mut HashSet<String>,
            mutable_only: bool,
        ) -> bool {
            match payload {
                VariantPayload::Unit => false,
                VariantPayload::Single(ty, _) => contains(cx, ty, seen, mutable_only),
                VariantPayload::Named(fields) => {
                    fields
                        .iter()
                        .any(|field| contains(cx, &field.ty, seen, mutable_only))
                }
            }
        }

        fn named_contains(
            cx: &Cx,
            name: &str,
            seen: &mut HashSet<String>,
            mutable_only: bool,
        ) -> bool {
            if !seen.insert(name.to_string()) {
                return false;
            }
            let found = cx.struct_fields.get(name).is_some_and(|fields| {
                fields
                    .iter()
                    .any(|(_, ty)| contains(cx, ty, seen, mutable_only))
            }) || cx.enum_variants.get(name).is_some_and(|variants| {
                variants
                    .iter()
                    .any(|(_, payload)| payload_contains(cx, payload, seen, mutable_only))
            });
            seen.remove(name);
            found
        }

        fn contains(
            cx: &Cx,
            ty: &Type,
            seen: &mut HashSet<String>,
            mutable_only: bool,
        ) -> bool {
            match ty {
                // D-PIN1=A: `Pin<T>` is a borrowed window like `View`/`ViewMut`,
                // so it needs the same hidden Rust lifetime everywhere it is
                // stored or returned. It is always a write window.
                Type::Apply { name, args }
                    if matches!(name.as_str(), "View" | "ViewMut" | Syntax::TYPE_PIN)
                        && args.len() == 1 =>
                {
                    !mutable_only || name == "ViewMut" || name == Syntax::TYPE_PIN
                }
                Type::Named(name) => named_contains(cx, name, seen, mutable_only),
                Type::Apply { name, args } => {
                    args.iter()
                        .any(|arg| contains(cx, arg, seen, mutable_only))
                        || named_contains(cx, name, seen, mutable_only)
                }
                Type::List(inner)
                | Type::Shared(inner)
                | Type::Option(inner)
                | Type::Tagged { inner, .. } => contains(cx, inner, seen, mutable_only),
                Type::Map { key, value, .. } | Type::Result { ok: key, err: value } => {
                    contains(cx, key, seen, mutable_only)
                        || contains(cx, value, seen, mutable_only)
                }
                Type::Tuple(fields) => fields
                    .iter()
                    .any(|(_, ty)| contains(cx, ty, seen, mutable_only)),
                Type::FixedList { elem, .. } => contains(cx, elem, seen, mutable_only),
                Type::Fn { params, ret, .. } => {
                    params
                        .iter()
                        .any(|ty| contains(cx, ty, seen, mutable_only))
                        || ret
                            .as_deref()
                            .is_some_and(|ty| contains(cx, ty, seen, mutable_only))
                }
                _ => false,
            }
        }

        contains(
            self,
            &self.expand_type_aliases(ty),
            &mut HashSet::new(),
            mutable_only,
        )
    }

    pub(crate) fn type_contains_shared_guard(&self, ty: &Type) -> bool {
        fn payload_contains(
            cx: &Cx,
            payload: &VariantPayload,
            seen: &mut HashSet<String>,
        ) -> bool {
            match payload {
                VariantPayload::Unit => false,
                VariantPayload::Single(ty, _) => contains(cx, ty, seen),
                VariantPayload::Named(fields) => {
                    fields.iter().any(|field| contains(cx, &field.ty, seen))
                }
            }
        }

        fn named_contains(cx: &Cx, name: &str, seen: &mut HashSet<String>) -> bool {
            if !seen.insert(name.to_string()) {
                return false;
            }
            cx.struct_fields
                .get(name)
                .is_some_and(|fields| fields.iter().any(|(_, ty)| contains(cx, ty, seen)))
                || cx.enum_variants.get(name).is_some_and(|variants| {
                    variants
                        .iter()
                        .any(|(_, payload)| payload_contains(cx, payload, seen))
                })
        }

        fn contains(cx: &Cx, ty: &Type, seen: &mut HashSet<String>) -> bool {
            match ty {
                Type::Apply { name, .. } if name == Syntax::TYPE_SHARED_GUARD => true,
                Type::Named(name) => named_contains(cx, name, seen),
                Type::Apply { name, args } => {
                    args.iter().any(|arg| contains(cx, arg, seen))
                        || named_contains(cx, name, seen)
                }
                Type::List(inner)
                | Type::Shared(inner)
                | Type::Option(inner)
                | Type::Tagged { inner, .. } => contains(cx, inner, seen),
                Type::Map { key, value, .. } | Type::Result { ok: key, err: value } => {
                    contains(cx, key, seen) || contains(cx, value, seen)
                }
                Type::Tuple(fields) => fields.iter().any(|(_, ty)| contains(cx, ty, seen)),
                Type::Union(members) => members.iter().any(|ty| contains(cx, ty, seen)),
                Type::FixedList { elem, .. } => contains(cx, elem, seen),
                Type::Fn { params, ret, .. } => {
                    params.iter().any(|ty| contains(cx, ty, seen))
                        || ret.as_deref().is_some_and(|ty| contains(cx, ty, seen))
                }
                _ => false,
            }
        }

        contains(self, &self.expand_type_aliases(ty), &mut HashSet::new())
    }

    /// Render a type whose view-bearing leaves borrow the function's hidden
    /// owner lifetime. Wrappers stay wrappers; only references and generated
    /// aggregate types receive the lifetime argument.
    pub(crate) fn rust_type_with_view_lifetime(&self, ty: &Type) -> String {
        self.rust_type_with_view_lifetime_using(ty, &|ty| self.rust_type(ty))
    }

    pub(crate) fn rust_type_with_view_lifetime_assoc(
        &self,
        ty: &Type,
        assoc: &HashSet<String>,
    ) -> String {
        self.rust_type_with_view_lifetime_using(ty, &|ty| {
            crate::Traits::rust_type_name_assoc(ty, assoc)
        })
    }

    fn rust_type_with_view_lifetime_using(
        &self,
        ty: &Type,
        base: &impl Fn(&Type) -> String,
    ) -> String {
        fn add_reference_lifetime(rust: String) -> String {
            if let Some(rest) = rust.strip_prefix("&mut ") {
                format!("&'__jet_view mut {rest}")
            } else if let Some(rest) = rust.strip_prefix('&') {
                format!("&'__jet_view {rest}")
            } else {
                rust
            }
        }

        fn add_type_lifetime(rust: String) -> String {
            if let Some(open) = rust.find('<') {
                format!("{}<'__jet_view, {}", &rust[..open], &rust[open + 1..])
            } else {
                format!("{rust}<'__jet_view>")
            }
        }

        fn definition_contains_view(cx: &Cx, name: &str) -> bool {
            cx.struct_fields
                .get(name)
                .is_some_and(|fields| fields.iter().any(|(_, ty)| cx.type_contains_view(ty)))
                || cx.enum_variants.get(name).is_some_and(|variants| {
                    variants.iter().any(|(_, payload)| match payload {
                        VariantPayload::Unit => false,
                        VariantPayload::Single(ty, _) => cx.type_contains_view(ty),
                        VariantPayload::Named(fields) => {
                            fields.iter().any(|field| cx.type_contains_view(&field.ty))
                        }
                    })
                })
        }

        fn render(cx: &Cx, ty: &Type, base: &impl Fn(&Type) -> String) -> String {
            match ty {
                Type::Apply { name, args }
                    if matches!(name.as_str(), "View" | "ViewMut" | Syntax::TYPE_PIN)
                        && args.len() == 1 =>
                {
                    add_reference_lifetime(base(ty))
                }
                Type::List(inner) if cx.type_contains_view(inner) => {
                    format!("Vec<{}>", render(cx, inner, base))
                }
                Type::Map { key, value, .. } if cx.type_contains_view(ty) => format!(
                    "std::collections::BTreeMap<{}, {}>",
                    render(cx, key, base),
                    render(cx, value, base)
                ),
                Type::Shared(inner) if cx.type_contains_view(inner) => format!(
                    "{}jet_std::JetShared<{}>",
                    cx.root_prefix,
                    render(cx, inner, base)
                ),
                Type::Option(inner) if cx.type_contains_view(inner) => {
                    format!("Option<{}>", render(cx, inner, base))
                }
                Type::Result { ok, err } if cx.type_contains_view(ty) => {
                    format!("Result<{}, {}>", render(cx, ok, base), render(cx, err, base))
                }
                Type::Tuple(fields) if cx.type_contains_view(ty) => {
                    add_type_lifetime(tuple_struct_name(&tuple_fields_plain(fields)))
                }
                Type::FixedList { elem, len, .. } if cx.type_contains_view(elem) => {
                    format!("[{}; {len}]", render(cx, elem, base))
                }
                Type::Tagged { inner, .. } if cx.type_contains_view(inner) => {
                    render(cx, inner, base)
                }
                Type::Named(name) if definition_contains_view(cx, name) => {
                    add_type_lifetime(base(ty))
                }
                Type::Apply { name, args }
                    if definition_contains_view(cx, name)
                        || args.iter().any(|arg| cx.type_contains_view(arg)) =>
                {
                    let head = base(&Type::Named(name.clone()));
                    let args = args
                        .iter()
                        .map(|arg| render(cx, arg, base))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let life = definition_contains_view(cx, name)
                        .then_some("'__jet_view")
                        .into_iter()
                        .chain((!args.is_empty()).then_some(args.as_str()))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{head}<{life}>")
                }
                _ => base(ty),
            }
        }

        render(self, &self.expand_type_aliases(ty), base)
    }

    pub(crate) fn struct_field_rust_with_view_lifetime(
        &self,
        s: &StructDef,
        edge: &str,
        ty: &Type,
    ) -> String {
        let base = match ty {
            Type::Named(n) if s.type_params.iter().any(|p| p.name == *n) => n.clone(),
            _ if self.type_contains_view(ty) => self.rust_type_with_view_lifetime(ty),
            _ => self.rust_type(ty),
        };
        if self
            .boxed_edges
            .contains(&(s.name.clone(), edge.to_string()))
        {
            format!("Box<{base}>")
        } else {
            base
        }
    }

    pub(crate) fn enum_field_rust_with_view_lifetime(
        &self,
        owner: &str,
        edge: &str,
        ty: &Type,
    ) -> String {
        let base = if self.type_contains_view(ty) {
            self.rust_type_with_view_lifetime(ty)
        } else {
            self.rust_type(ty)
        };
        if self
            .boxed_edges
            .contains(&(owner.to_string(), edge.to_string()))
        {
            format!("Box<{base}>")
        } else {
            base
        }
    }

    /// D-SOA1: is `name` a `#layout(columnar)` struct (local or imported)? The
    /// columnar set only carries local structs; an imported columnar struct is
    /// not tracked, so `[ImportedColumnar]` still lowers AoS — acceptable for v1
    /// (columnar lists don't cross module boundaries in the shipped examples).
    pub(crate) fn is_columnar_struct(&self, name: &str) -> bool {
        self.columnar.contains(name)
    }

    /// D-SOA1: if `inner` is a `#layout(columnar)` struct type, the Rust path of
    /// its generated struct-of-arrays type (`user_<S>_columns`, module-prefixed
    /// like the struct itself). `None` for any non-columnar element.
    pub(crate) fn columnar_list_type(&self, inner: &Type) -> Option<String> {
        if let Type::Named(name) = inner {
            if self.is_columnar_struct(name) {
                return Some(if self.foreign_types.contains_key(name.as_str()) {
                    let rust_mod = &self.foreign_types[name.as_str()];
                    format!("{}{}::user_{name}_columns", self.root_prefix, rust_mod)
                } else {
                    format!("user_{name}_columns")
                });
            }
        }
        None
    }

    /// D-TYPEALIAS1: expand `alias Name<T> = …` applications to their target type.
    pub(crate) fn expand_type_aliases(&self, ty: &Type) -> Type {
        match ty {
            Type::Apply { name, args } if self.type_aliases.contains_key(name) => {
                let (params, target) = self.type_aliases.get(name).unwrap();
                let subst: HashMap<String, Type> = params
                    .iter()
                    .zip(args.iter())
                    .map(|(p, a)| (p.name.clone(), a.clone()))
                    .collect();
                self.expand_type_aliases(&Generics::substitute_type(target, &subst))
            }
            Type::Apply { name, args } => Type::Apply {
                name: name.clone(),
                args: args.iter().map(|a| self.expand_type_aliases(a)).collect(),
            },
            Type::List(inner) => Type::List(Box::new(self.expand_type_aliases(inner))),
            Type::Map {
                key,
                key_span,
                value,
            } => Type::Map {
                key: Box::new(self.expand_type_aliases(key)),
                key_span: *key_span,
                value: Box::new(self.expand_type_aliases(value)),
            },
            Type::Shared(inner) => Type::Shared(Box::new(self.expand_type_aliases(inner))),
            Type::Option(inner) => Type::Option(Box::new(self.expand_type_aliases(inner))),
            Type::Result { ok, err } => Type::Result {
                ok: Box::new(self.expand_type_aliases(ok)),
                err: Box::new(self.expand_type_aliases(err)),
            },
            Type::Fn {
                params,
                ret,
                effect_bound,
                return_view_provenance,
            } => Type::Fn {
                params: params.iter().map(|p| self.expand_type_aliases(p)).collect(),
                ret: ret.as_ref().map(|r| Box::new(self.expand_type_aliases(r))),
                effect_bound: effect_bound.clone(),
                return_view_provenance: return_view_provenance.clone(),
            },
            Type::Tuple(fields) => Type::Tuple(
                fields
                    .iter()
                    .map(|(n, t)| (n.clone(), Box::new(self.expand_type_aliases(t))))
                    .collect(),
            ),
            Type::Tagged { marker, inner } => Type::Tagged {
                marker: marker.clone(),
                inner: Box::new(self.expand_type_aliases(inner)),
            },
            Type::FixedList { elem, len, .. } => Type::FixedList {
                elem: Box::new(self.expand_type_aliases(elem)),
                len: *len,
                len_symbol: None,
            },
            Type::Union(members) => crate::AST::canonicalize_union(
                members.iter().map(|m| self.expand_type_aliases(m)).collect(),
            ),
            other => other.clone(),
        }
    }

    pub(crate) fn rust_type(&self, ty: &Type) -> String {
        let ty = self.expand_type_aliases(ty);
        if let Some((base, _)) = ty.quantity_parts() {
            return self.rust_type(base);
        }
        match &ty {
            Type::Int => "i64".to_string(),
            Type::Float => "f64".to_string(),
            Type::IntN { signed, bits } => {
                format!("{}{}", if *signed { 'i' } else { 'u' }, bits)
            }
            Type::Float32 => "f32".to_string(),
            Type::Bool => "bool".to_string(),
            Type::String => "String".to_string(),
            Type::Char => "char".to_string(),
            // D-SOA1: a `[S]` of a `#layout(columnar)` struct lowers to the
            // generated struct-of-arrays type `user_<S>_columns`, not `Vec<S>`.
            Type::List(inner) if self.columnar_list_type(inner).is_some() => {
                self.columnar_list_type(inner).unwrap()
            }
            Type::List(inner) => format!("Vec<{}>", self.rust_type(inner)),
            Type::Map { key, value, .. } => format!(
                "std::collections::BTreeMap<{}, {}>",
                self.rust_type(key),
                self.rust_type(value)
            ),
            // D-MEM1 S6 (D-SHARED-API1=A): `Shared<T>` — a lock-guarded shared
            // handle (`.read(f)`/`.edit(f)` closure API). Was a plain read-only
            // `Arc<T>` before this stage (never actually constructible — no
            // `Shared.new` existed — so there is no live behavior to preserve);
            // now `Arc<RwLock<T>>`, wrapped so `.clone()` stays a cheap Arc clone.
            Type::Shared(inner) => format!(
                "{}jet_std::JetShared<{}>",
                self.root_prefix,
                self.rust_type(inner)
            ),
            Type::Option(inner) => format!("Option<{}>", self.rust_type(inner)),
            Type::Result { ok, err } => {
                format!("Result<{}, {}>", self.rust_type(ok), self.rust_type(err))
            }
            // Items inside an imported file live in `mod user_<alias>`; the
            // module provides the namespace, so item names stay plain.
            // c148: also recognize multi-char type params from `current_type_params`.
            Type::Named(name)
                if (Generics::is_type_var_name(name)
                    || self.current_type_params.borrow().contains(name.as_str()))
                    && !self.type_names.contains(name) =>
            {
                name.clone()
            }
            Type::Named(name)
                if (name == "Unit" || name == "Void") && !self.type_names.contains(name) =>
            {
                "()".to_string()
            }
            // D-TASKGROUP-PARAM1=A: helpers receive the lexical group's real
            // internal collector. The surface remains second-class.
            Type::Named(name)
                if name == Syntax::TYPE_TASKGROUP && !self.type_names.contains(name) =>
            {
                format!("{}jet_std::JetTaskGroup", self.root_prefix)
            }
            Type::Named(name)
                if name == Syntax::TYPE_CONDITION && !self.type_names.contains(name) =>
            {
                format!("{}jet_std::JetCondition", self.root_prefix)
            }
            Type::Named(name) if name == "Error" => "String".to_string(),
            Type::Named(name) if name == "Claims" && !self.type_names.contains(name) => {
                format!("{}JetAuthClaims", self.root_prefix)
            }
            Type::Named(name) if name == "AuthError" && !self.type_names.contains(name) => {
                format!("{}JetAuthError", self.root_prefix)
            }
            Type::Named(name)
                if !self.type_names.contains(name)
                    && core_crypto_rust_type_name(name).is_some() =>
            {
                let ffi = self.ffi_crate.as_deref().unwrap_or("jet_ffi");
                let rust = core_crypto_rust_type_name(name).unwrap();
                format!("{ffi}::{rust}")
            }
            Type::Named(name) if matches!(name.as_str(), "KeyStatus" | "VaultError") => {
                let ffi = self.ffi_crate.as_deref().unwrap_or("jet_ffi");
                let rust = if name == "KeyStatus" { "JetVaultKeyStatus" } else { "JetVaultError" };
                format!("{ffi}::{rust}")
            }
            // D-PENDING1=B: Loadable<Unknown, Unknown> placeholders — Rust infers the type.
            Type::Named(name) if name == "Unknown" => "_".to_string(),
            // D-APPROX1=A: sketch types → opaque Rust structs.
            Type::Named(name) if name == "HyperLogLog" => "JetHyperLogLog".to_string(),
            Type::Named(name) if name == "TDigest" => "JetTDigest".to_string(),
            Type::Named(name) if name == "CountMinSketch" => "JetCountMinSketch".to_string(),
            Type::Named(name) if name == "ReservoirSampler" => "JetReservoirSampler".to_string(),
            // D-TIMEDEPTH1=A: civil-time types → opaque Rust structs.
            Type::Named(name) if name == "Date" => "JetDate".to_string(),
            Type::Named(name) if name == "LocalDate" => "JetDate".to_string(),
            Type::Named(name) if name == "LocalTime" => "JetLocalTime".to_string(),
            Type::Named(name) if name == "DateTime" => "JetDateTime".to_string(),
            Type::Named(name) if name == "Instant" => "JetInstant".to_string(),
            Type::Named(name) if name == "Period" => "JetPeriod".to_string(),
            Type::Named(name) if name == "Zone" => "JetZone".to_string(),
            Type::Named(name) if name == "ZonedDateTime" => "JetZonedDateTime".to_string(),
            // D-URL1=A: URL/MIME values live in the corelib prelude module.
            Type::Named(name) if name == "Url" => {
                format!("{}jet_std::JetURL", self.root_prefix)
            }
            Type::Named(name) if name == "Mime" => {
                format!("{}jet_std::JetMIME", self.root_prefix)
            }
            Type::Named(name) if matches!(name.as_str(),
                "Address" | "Message" | "Attachment" | "Envelope" | "SMTPSecurity"
                | "RecipientPolicy" | "RecipientReport" | "SendReport" | "EmailError" | "Limits"
            ) && !self.type_names.contains(name) => {
                let rust = match name.as_str() {
                    "Address" => "Address", "Message" => "Message", "Attachment" => "Attachment",
                    "Envelope" => "Envelope", "SMTPSecurity" => "SMTPSecurity",
                    "RecipientPolicy" => "RecipientPolicy", "RecipientReport" => "RecipientReport",
                    "SendReport" => "SendReport", "Limits" => "Limits", _ => "Error",
                };
                format!("{}jet_email::{rust}", self.root_prefix)
            }
            Type::Named(name) if name == "SMTPAuth" && !self.type_names.contains(name) => {
                let ffi = self.ffi_crate.as_deref().unwrap_or("jet_ffi");
                format!("{}jet_email::SMTPAuth<{}::Secret>", self.root_prefix, ffi)
            }
            Type::Named(name)
                if matches!(name.as_str(), "DkimConfig" | "SMTPConfig")
                    && !self.type_names.contains(name) => {
                let ffi = self.ffi_crate.as_deref().unwrap_or("jet_ffi");
                format!("{}jet_email::{}<{}::Secret>", self.root_prefix, name, ffi)
            }
            Type::Named(name)
                if matches!(name.as_str(), "TLSTrust" | "Mailer")
                    && !self.type_names.contains(name) => {
                format!("{}jet_email::{}", self.root_prefix, name)
            }
            // D-NETDEP1=A / D-HTTPLIB1=A: HTTP types → opaque Rust structs.
            Type::Named(name) if name == "HTTPRequest" => "JetHTTPRequest".to_string(),
            Type::Named(name) if name == "HTTPResponse" => "JetHTTPResponse".to_string(),
            Type::Named(name) if name == "HTTPClient" => "JetHTTPClient".to_string(),
            Type::Named(name) if name == "HTTPProxy" => "JetHTTPProxy".to_string(),
            Type::Named(name) if name == "HTTPRedirectPolicy" => "JetHTTPRedirectPolicy".to_string(),
            Type::Named(name) if name == "HTTPRetryPolicy" => "JetHTTPRetryPolicy".to_string(),
            Type::Named(name) if name == "HTTPCookieJar" => "JetHTTPCookieJar".to_string(),
            Type::Named(name) if name == "HTTPCorsPolicy" => "JetHTTPCorsPolicy".to_string(),
            Type::Named(name) if name == "HTTPCorsOrigins" => "JetHTTPCorsOrigins".to_string(),
            Type::Named(name) if name == "HTTPCompressEncoding" => "JetHTTPCompressEncoding".to_string(),
            Type::Named(name) if name == "HTTPMethod" => "JetHTTPMethod".to_string(),
            Type::Named(name) if name == "HTTPStatus" => "JetHTTPStatus".to_string(),
            Type::Named(name) if name == "HTTPVersion" => "JetHTTPVersion".to_string(),
            Type::Named(name) if name == "HTTPHeaderName" => "JetHTTPHeaderName".to_string(),
            Type::Named(name) if name == "HTTPHeaderValue" => "JetHTTPHeaderValue".to_string(),
            Type::Named(name) if name == "HTTPBody" => "JetHTTPBody".to_string(),
            Type::Named(name) if name == "HTTPBodyChunks" => "JetHTTPBodyChunks".to_string(),
            Type::Named(name) if name == "HTTPError" => "JetHTTPError".to_string(),
            Type::Named(name) if name == "HTTPOperation" => "JetHTTPOperation".to_string(),
            Type::Named(name) if name == "HTTPHeaders" => "JetHTTPHeaders".to_string(),
            Type::Named(name) if name == "HTTPMux" => "JetHTTPMux".to_string(),
            Type::Named(name) if name == "HTTPHandler" => "JetHTTPHandler".to_string(),
            Type::Named(name) if name == "HTTPServer" => "JetHTTPServer".to_string(),
            Type::Named(name) if name == "HTTPShutdownReport" => "JetHTTPShutdownReport".to_string(),
            Type::Named(name) if name == "HTTPRequest" => "JetHTTPRequest".to_string(),
            Type::Named(name) if name == "HTTPResponse" => "JetHTTPResponse".to_string(),
            Type::Named(name) if name == "HTTPServerTls" => "JetHTTPServerTls".to_string(),
            // D-WS1=B: WebSocket types.
            Type::Named(name) if name == "WsConn" => "JetWsConn".to_string(),
            Type::Named(name) if name == "WsError" => "JetWsError".to_string(),
            Type::Named(name) if name == "WsMessage" => "JetWsMessage".to_string(),
            // D-BROWSER-AUTO1=A: native BiDi opaque types.
            Type::Named(name) if name == "Browser" => "JetBrowser".to_string(),
            Type::Named(name) if name == "BrowserContext" => "JetBrowserContext".to_string(),
            Type::Named(name) if name == "BrowserPage" => "JetBrowserPage".to_string(),
            Type::Named(name) if name == "BrowserFrame" => "JetBrowserFrame".to_string(),
            Type::Named(name) if name == "BrowserLocator" => "JetBrowserLocator".to_string(),
            Type::Named(name) if name == "BrowserIntercept" => "JetBrowserIntercept".to_string(),
            Type::Named(name) if name == "BrowserEvent" => "JetBrowserEvent".to_string(),
            Type::Named(name) if name == "BrowserTrace" => "JetBrowserTrace".to_string(),
            Type::Named(name) if name == "BrowserReceipt" => "JetBrowserReceipt".to_string(),
            Type::Named(name) if name == "BrowserPrivacy" => "JetBrowserPrivacy".to_string(),
            Type::Named(name) if name == "BrowserError" => "JetBrowserError".to_string(),
            Type::Named(name) if name == "BrowserCapabilities" => {
                "JetBrowserCapabilities".to_string()
            }
            Type::Named(name) if name == "BrowserProfile" => "JetBrowserProfile".to_string(),
            Type::Named(name) if name == "BrowserTimeout" => "JetBrowserTimeout".to_string(),
            Type::Named(name) if name == "BrowserProtocol" => "JetBrowserProtocol".to_string(),
            // c97/D-STRPARSE1: the builtin parse error (`Int.parse`, `Float.parse`)
            // erases to a plain message — never user-constructed.
            // A user enum named `ParseError` (in `type_names`) keeps its own lowering.
            Type::Named(name) if name == "ParseError" && !self.type_names.contains(name) => {
                "String".to_string()
            }
            // D-TYPEDTEXT1=D: `SQL` is a checked (template, bound params) pair — the
            // params never re-enter the template text. `HTML` is already the fully
            // escaped text, so it's just a `String` underneath.
            Type::Named(name) if name == "SQL" => "(String, Vec<String>)".to_string(),
            Type::Named(name) if name == "HTML" => "String".to_string(),
            Type::Named(name) if name == "Sh" => "Vec<String>".to_string(),
            // D-DEFER1: ScopeGuard is generic over F (the closure type); emit `_`
            // so Rust infers the monomorphised type from the initialiser expression.
            Type::Named(name) if name == "ScopeGuard" => "_".to_string(),
            // D-TERM1 (ratified 2026-06-22): `Key` is a top-level prelude enum.
            Type::Named(name) if name == "Key" => format!("{}JetKey", self.root_prefix),
            // D-RENDERTGT2=A (c133 M1): UI geometry/event/backend types. User structs
            // named Point/Rect/Size (common in examples) keep `user_<Name>` lowering.
            Type::Named(name) if name == "Point" && !self.type_names.contains(name) => {
                format!("{}JetPoint", self.root_prefix)
            }
            Type::Named(name) if name == "Size" && !self.type_names.contains(name) => {
                format!("{}JetSize", self.root_prefix)
            }
            Type::Named(name) if name == "Rect" && !self.type_names.contains(name) => {
                format!("{}JetRect", self.root_prefix)
            }
            Type::Named(name) if name == "SizeConstraint" && !self.type_names.contains(name) => {
                format!("{}JetSizeConstraint", self.root_prefix)
            }
            Type::Named(name) if name == "UiNode" && !self.type_names.contains(name) => {
                format!("{}JetUiNode", self.root_prefix)
            }
            Type::Named(name) if name == "InputEvent" && !self.type_names.contains(name) => {
                format!("{}JetInputEvent", self.root_prefix)
            }
            Type::Named(name) if name == "EventResult" && !self.type_names.contains(name) => {
                format!("{}JetEventResult", self.root_prefix)
            }
            Type::Named(name) if name == "NullBackend" && !self.type_names.contains(name) => {
                format!("{}JetNullBackend", self.root_prefix)
            }
            Type::Named(name) if name == "TuiBackend" && !self.type_names.contains(name) => {
                format!("{}JetTuiBackend", self.root_prefix)
            }
            // D-UIDEVSHELL1=A (c134 Phase 8): native Linux GTK4 backend — a
            // top-level prelude struct re-exported from `mod jet_gtk`.
            Type::Named(name) if name == "GtkBackend" && !self.type_names.contains(name) => {
                format!("{}JetGtkBackend", self.root_prefix)
            }
            // c-devserver (owner-directed 2026-07-01): DevServer is a
            // top-level prelude struct (Prelude/DevServer.rs).
            Type::Named(name) if name == "DevServer" && !self.type_names.contains(name) => {
                format!("{}JetDevServer", self.root_prefix)
            }
            // D-WEBAPP1=D: WebApp / WebPage opaque builder types.
            Type::Named(name) if name == "WebApp" && !self.type_names.contains(name) => {
                format!("{}JetWebApp", self.root_prefix)
            }
            Type::Named(name) if name == "WebPage" && !self.type_names.contains(name) => {
                format!("{}JetWebPage", self.root_prefix)
            }
            Type::Named(name)
                if name == Syntax::TYPE_RANGE && !self.type_names.contains(name) =>
            {
                format!("{}JetRange", self.root_prefix)
            }
            Type::Named(name)
                if name == Syntax::TYPE_EFFECT && !self.type_names.contains(name) =>
            {
                format!("{}jet_std::JetReactiveEffect", self.root_prefix)
            }
            Type::Named(name)
                if name == Syntax::TYPE_SUBSCRIPTION && !self.type_names.contains(name) =>
            {
                format!("{}jet_std::JetSubscription", self.root_prefix)
            }
            Type::Named(name)
                if name == Syntax::TYPE_EVENT_SCOPE && !self.type_names.contains(name) =>
            {
                format!("{}jet_std::JetEventScope", self.root_prefix)
            }
            Type::Named(name)
                if name == Syntax::TYPE_EVENT_POLICY && !self.type_names.contains(name) =>
            {
                format!("{}jet_std::JetEventPolicy", self.root_prefix)
            }
            Type::Named(name)
                if name == Syntax::TYPE_EVENT_TRACE && !self.type_names.contains(name) =>
            {
                format!("{}jet_std::JetEventTrace", self.root_prefix)
            }
            Type::Named(name)
                if name == Syntax::TYPE_HOOK_POLICY && !self.type_names.contains(name) =>
            {
                format!("{}jet_std::JetHookPolicy", self.root_prefix)
            }
            // E2-M7: file handle types are top-level in the prelude (not in jet_std).
            Type::Named(name) if file_handle_rust_type(name).is_some() => {
                format!(
                    "{}{}",
                    self.root_prefix,
                    file_handle_rust_type(name).unwrap()
                )
            }
            // D-RAYLIB1=A: the raylib bridge lives in the
            // top-level corelib prelude today, so generated user bindings must
            // reference `RaylibWindow`/`RaylibColor` directly.
            Type::Named(name) if raylib_handle_rust_type(name).is_some() => {
                format!(
                    "{}{}",
                    self.root_prefix,
                    raylib_handle_rust_type(name).unwrap()
                )
            }
            Type::Named(name) if game_handle_rust_type(name).is_some() => {
                format!(
                    "{}{}",
                    self.root_prefix,
                    game_handle_rust_type(name).unwrap()
                )
            }
            // E2-M10: networking opaque types are top-level in the prelude.
            Type::Named(name) if net_handle_rust_type(name).is_some() => {
                format!(
                    "{}{}",
                    self.root_prefix,
                    net_handle_rust_type(name).unwrap()
                )
            }
            // D-COMPUTE1=D: Tensor / compute error / device handles.
            Type::Named(name) if compute_handle_rust_type(name).is_some() => {
                format!(
                    "{}{}",
                    self.root_prefix,
                    compute_handle_rust_type(name).unwrap()
                )
            }
            Type::Named(name) if service_handle_rust_type(name).is_some() => {
                format!(
                    "{}{}",
                    self.root_prefix,
                    service_handle_rust_type(name).unwrap()
                )
            }
            Type::Apply { name, args }
                if name == "Tensor" && args.len() <= 1 && compute_handle_rust_type(name).is_some() =>
            {
                format!("{}JetTensor", self.root_prefix)
            }
            // D-ALLOC1/D-ALLOC-C (ratified 2026-06-19): allocator opaque types.
            Type::Named(name) if alloc_handle_rust_type(name).is_some() => {
                format!(
                    "{}{}",
                    self.root_prefix,
                    alloc_handle_rust_type(name).unwrap()
                )
            }
            // D-ARGS1 (ratified 2026-06-22): ArgsSpec / ParsedArgs are top-level prelude structs.
            Type::Named(name) if args_handle_rust_type(name).is_some() => {
                format!(
                    "{}{}",
                    self.root_prefix,
                    args_handle_rust_type(name).unwrap()
                )
            }
            // D-ANY-JAI1 (c7jaiany §6): `reflect.of(x)`'s Value/Field handles. `Value`
            // and `Field` are common enough words that a user struct sharing the name
            // is likely (`examples/features/memory/zerocopy.jet` already declares its
            // own `Field`) — same guard as the layout/core-rust-type-name arms below:
            // a user type of that name always wins.
            Type::Named(name)
                if reflect_handle_rust_type(name).is_some() && !self.type_names.contains(name) =>
            {
                format!(
                    "{}{}",
                    self.root_prefix,
                    reflect_handle_rust_type(name).unwrap()
                )
            }
            // D-SHIFT1 (c7shift): `binary.Reader` / `text.Cursor` — plausible
            // user type names, same collision guard as `Value`/`Field` above.
            Type::Named(name)
                if binary_text_handle_rust_type(name).is_some()
                    && !self.type_names.contains(name) =>
            {
                format!(
                    "{}{}",
                    self.root_prefix,
                    binary_text_handle_rust_type(name).unwrap()
                )
            }
            // D-LAYOUT1 / D-LAYOUT-GATES1: `layout` runtime types are top-level
            // in their own `jet_layout` module (like the alloc/file/net handles
            // above, not nested in `jet_std`).
            Type::Named(name)
                if layout_handle_rust_type(name).is_some() && !self.type_names.contains(name) =>
            {
                format!(
                    "{}{}",
                    self.root_prefix,
                    layout_handle_rust_type(name).unwrap()
                )
            }
            // A user struct/enum sharing a built-in Core type name (e.g. a user
            // `Vec3`) wins — it keeps its own `user_<Name>` lowering. Only fall to the
            // built-in jet_std struct when the name is NOT a user type.
            Type::Named(name)
                if core_rust_type_name(name).is_some() && !self.type_names.contains(name) =>
            {
                format!(
                    "{}jet_std::{}",
                    self.root_prefix,
                    core_rust_type_name(name).unwrap()
                )
            }
            Type::Named(name) if self.core_qualified_rust_type_name(name).is_some() => {
                let resolved = self.core_qualified_rust_type_name(name).unwrap();
                if resolved == "Claims" || resolved == "AuthError" {
                    let rust = if resolved == "Claims" { "JetAuthClaims" } else { "JetAuthError" };
                    return format!("{}{rust}", self.root_prefix);
                }
                if let Some(rust) = core_crypto_rust_type_name(resolved) {
                    let ffi = self.ffi_crate.as_deref().unwrap_or("jet_ffi");
                    return format!("{ffi}::{rust}");
                }
                if matches!(resolved,
                    "Address" | "Message" | "Attachment" | "Envelope" | "SMTPSecurity"
                    | "RecipientPolicy" | "RecipientReport" | "SendReport" | "EmailError" | "Limits"
                ) {
                    let rust = if resolved == "EmailError" { "Error" } else { resolved };
                    return format!("{}jet_email::{rust}", self.root_prefix);
                }
                if matches!(resolved, "SMTPAuth" | "DkimConfig" | "SMTPConfig") {
                    let ffi = self.ffi_crate.as_deref().unwrap_or("jet_ffi");
                    return format!("{}jet_email::{}<{}::Secret>", self.root_prefix, resolved, ffi);
                }
                if resolved == "TLSTrust" || resolved == "Mailer" {
                    return format!("{}jet_email::{}", self.root_prefix, resolved);
                }
                format!(
                    "{}jet_std::{}",
                    self.root_prefix,
                    resolved
                )
            }
            Type::Named(name) if self.trait_names.contains(name) => {
                format!("Box<dyn {}>", Generics::user_trait_rust(name))
            }
            Type::Named(name) if self.foreign_types.contains_key(name.as_str()) => {
                let rust_mod = &self.foreign_types[name.as_str()];
                let leaf = name.rsplit_once('.').map_or(name.as_str(), |(_, leaf)| leaf);
                format!("{}{}::{}", self.root_prefix, rust_mod, user_type_rust(leaf))
            }
            Type::Named(name) if name.contains('.') => {
                let (alias, leaf) = name.split_once('.').unwrap();
                match self.import_mods.get(alias) {
                    Some(rust_mod) => format!(
                        "{}{}::{}",
                        self.root_prefix,
                        rust_mod,
                        user_type_rust(leaf)
                    ),
                    None => user_type_rust(name),
                }
            }
            Type::Named(n) if n == "Expired" => "JetExpired".to_string(),
            Type::Named(name) => user_type_rust(name),
            Type::Apply { name, args } if name == "Task" && !args.is_empty() => {
                format!(
                    "{}jet_std::JetTask<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args }
                if matches!(name.as_str(), "KeyRef" | "MutationPlan" | "VaultWrite" | "Rotation" | "WrappedImportPlan")
                    && args.len() == 1 =>
            {
                let ffi = self.ffi_crate.as_deref().unwrap_or("jet_ffi");
                let rust = match name.as_str() {
                    "KeyRef" => "JetVaultKeyRef", "MutationPlan" => "JetVaultMutationPlan",
                    "VaultWrite" => "JetVaultWrite", "Rotation" => "JetVaultRotation",
                    _ => "JetVaultWrappedImportPlan",
                };
                format!("{ffi}::{rust}<{}>", self.rust_type(&args[0]))
            }
            Type::Apply { name, args } if name == "Receiver" && !args.is_empty() => {
                format!(
                    "{}jet_std::JetReceiver<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args } if name == "Sender" && !args.is_empty() => {
                format!(
                    "{}jet_std::JetSender<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            // D-MEM1 S6 (D-POOLID-API1=A): `Pool<T>`/`Id<T>` — the generational
            // arena and its lightweight index+generation handle.
            Type::Apply { name, args } if name == "Pool" && !args.is_empty() => {
                format!(
                    "{}jet_std::JetPool<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args } if name == "Id" && !args.is_empty() => {
                format!(
                    "{}jet_std::JetId<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            // D-LOCALCELL1=A: local interior-mutability handles and their
            // dynamically checked projected guards.
            Type::Apply { name, args } if name == "Cell" && !args.is_empty() => {
                format!(
                    "{}jet_std::JetCell<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args }
                if name == "CellReadGuard" && !args.is_empty() =>
            {
                format!(
                    "{}jet_std::JetCellReadGuard<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args }
                if name == "CellEditGuard" && !args.is_empty() =>
            {
                format!(
                    "{}jet_std::JetCellEditGuard<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            // D-STREAMYIELD1: a generator's `Stream<T>` is a rendezvous-channel
            // receiver — `Receiver<T>` already implements `IntoIterator<Item = T>`,
            // which is exactly `loop x; stream { }`'s pull-one-block-until-ready
            // shape (no coroutine machinery needed).
            Type::Apply { name, args } if name == "Stream" && !args.is_empty() => {
                format!("std::sync::mpsc::Receiver<{}>", self.rust_type(&args[0]))
            }
            // D-REACT1=B: reactive handle types lower to the std-only jet_std runtime.
            Type::Apply { name, args } if name == Syntax::TYPE_SIGNAL && !args.is_empty() => {
                format!(
                    "{}jet_std::JetSignal<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args }
                if (name == Syntax::TYPE_DERIVED || name == Syntax::TYPE_COMPUTED)
                    && !args.is_empty() =>
            {
                format!(
                    "{}jet_std::JetDerived<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args } if name == Syntax::TYPE_EVENT && !args.is_empty() => {
                format!(
                    "{}jet_std::JetEvent<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args } if name == Syntax::TYPE_ASYNC_EVENT && args.len() == 2 => {
                format!(
                    "{}jet_std::JetAsyncEvent<{}, {}>",
                    self.root_prefix,
                    self.rust_type(&args[0]),
                    self.rust_type(&args[1])
                )
            }
            Type::Apply { name, args } if name == Syntax::TYPE_DISPATCH_REPORT && !args.is_empty() => {
                format!(
                    "{}jet_std::JetDispatchReport<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args } if name == Syntax::TYPE_DISPATCH_FAILURE && !args.is_empty() => {
                format!(
                    "{}jet_std::JetDispatchFailure<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args } if name == Syntax::TYPE_HOOK && args.len() == 2 => {
                format!(
                    "{}jet_std::JetHook<{}, {}>",
                    self.root_prefix,
                    self.rust_type(&args[0]),
                    self.rust_type(&args[1])
                )
            }
            Type::Apply { name, args } if name == Syntax::TYPE_DECISION_HOOK && args.len() == 2 => {
                format!(
                    "{}jet_std::JetDecisionHook<{}, {}>",
                    self.root_prefix,
                    self.rust_type(&args[0]),
                    self.rust_type(&args[1])
                )
            }
            Type::Apply { name, args } if name == Syntax::TYPE_HOOK_DECISION && args.len() == 2 => {
                format!(
                    "{}jet_std::JetHookDecision<{}, {}>",
                    self.root_prefix,
                    self.rust_type(&args[0]),
                    self.rust_type(&args[1])
                )
            }
            Type::Apply { name, args } if name == Syntax::TYPE_HOOK_OUTCOME && args.len() == 2 => {
                format!(
                    "{}jet_std::JetHookOutcome<{}, {}>",
                    self.root_prefix,
                    self.rust_type(&args[0]),
                    self.rust_type(&args[1])
                )
            }
            // D-MIGRATE3=A: `DecodeResult<T>` — `decode_traced<T>`'s return-shape
            // wrapper (`T` is already `[T]`/`Vec<T>` for CSV by the time sema
            // builds this `Type::Apply`, so this arm needs no per-codec case).
            // User-type-wins (D-SHIFT1 precedent): a user struct named
            // `DecodeResult` shadows the core one — falls through to the plain
            // user-generic arm below instead.
            Type::Apply { name, args }
                if name == "DecodeResult"
                    && !args.is_empty()
                    && !self.type_names.contains(name) =>
            {
                format!(
                    "{}jet_std::DecodeResult<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            // D-DATAFRAME1=A: core.data typed containers, backed by std-only
            // prelude values. User types with the same names still win.
            Type::Apply { name, args }
                if name == "Table" && !args.is_empty() && !self.type_names.contains(name) =>
            {
                format!(
                    "{}jet_std::DataTable<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args }
                if name == "Series" && !args.is_empty() && !self.type_names.contains(name) =>
            {
                format!(
                    "{}jet_std::DataSeries<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args }
                if name == "LazyFrame" && !args.is_empty() && !self.type_names.contains(name) =>
            {
                format!(
                    "{}jet_std::DataLazyFrame<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            // D-DATAFLOW1=A: pull stream is one opaque Rust handle; Jet keeps T.
            Type::Apply { name, args }
                if name == "DataStream" && !args.is_empty() && !self.type_names.contains(name) =>
            {
                format!("{}jet_std::DataStream", self.root_prefix)
            }
            Type::Apply { name, args }
                if name == "DataJoin" && args.len() == 2 && !self.type_names.contains(name) =>
            {
                format!(
                    "{}jet_std::DataJoin<{}, {}>",
                    self.root_prefix,
                    self.rust_type(&args[0]),
                    self.rust_type(&args[1])
                )
            }
            // D-HONESTNUM1=A: Measurement<T> → jet_std::JetMeasurement<T>.
            Type::Apply { name, args } if name == Syntax::TYPE_MEASUREMENT && !args.is_empty() => {
                format!(
                    "{}jet_std::JetMeasurement<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args }
                if name == Syntax::TYPE_SHARED_GUARD && args.len() == 1 =>
            {
                format!(
                    "{}jet_std::JetSharedGuard<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args }
                if name == Syntax::TYPE_SHARED_WEAK && args.len() == 1 =>
            {
                format!(
                    "{}jet_std::JetSharedWeak<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            // D-PENDING1=B: Loadable<T,E> → JetLoadable<T,E>.
            Type::Apply { name, args } if name == "Loadable" && args.len() == 2 => {
                format!(
                    "JetLoadable<{}, {}>",
                    self.rust_type(&args[0]),
                    self.rust_type(&args[1])
                )
            }
            // D-DYNARRAY1: View<T> -> a genuine borrowed Rust slice `&[T]`. Zero-
            // copy (a slice is a plain (ptr, len) pair, no allocation) and
            // ordinary safe Rust — the lifetime is elided, valid as long as the
            // borrow stays local, exactly the shape sema's E2305 owner-outlives
            // check proves before this type is ever emitted (I2/I3: sema
            // decides, codegen just emits the reference).
            Type::Apply { name, args } if name == "View" && args.len() == 1 => {
                if matches!(&args[0], Type::Named(inner) if inner == "str") {
                    "&str".to_string()
                } else {
                    format!("&[{}]", self.rust_type(&args[0]))
                }
            }
            Type::Apply { name, args } if name == "ViewMut" && args.len() == 1 => {
                format!("&mut [{}]", self.rust_type(&args[0]))
            }
            // D-PIN1=A: `Pin<T>` is the address-stability contract, which sema
            // proves before this type is ever emitted (I3). The value itself is
            // an ordinary exclusive Rust reference to the pinned place — that is
            // exactly what "the storage does not move while the pin is live"
            // means once the proof is done, so no runtime wrapper is emitted.
            Type::Apply { name, args } if name == Syntax::TYPE_PIN && args.len() == 1 => {
                format!("&mut {}", self.rust_type(&args[0]))
            }
            // D-ITERTOOLS1=A: Iter<T> → JetIter<T> (must-use move-only lazy view).
            Type::Apply { name, args }
                if name == Syntax::TYPE_ITER && args.len() == 1 =>
            {
                format!("JetIter<{}>", self.rust_type(&args[0]))
            }
            // D-CORE-SECRETS1=A: generic TTL stays distinct from secret lifecycle.
            Type::Apply { name, args }
                if name == Syntax::EXPIRING_VALUE_TYPE && args.len() == 1 =>
            {
                format!("JetExpiring<{}>", self.rust_type(&args[0]))
            }
            // D-TTLVAL1=A: the one secret-lifetime wrapper.
            Type::Apply { name, args } if name == "ExpiringSecret" && args.len() == 1 => {
                format!("JetExpiringSecret<{}>", self.rust_type(&args[0]))
            }
            // D-COLLBREADTH1=A: Set<T> → HashSet<T>, Deque<T> → VecDeque<T>.
            Type::Apply { name, args } if name == "Set" && !args.is_empty() => {
                format!("std::collections::HashSet<{}>", self.rust_type(&args[0]))
            }
            Type::Apply { name, args } if name == Syntax::TYPE_SORTED_SET && !args.is_empty() => {
                format!("std::collections::BTreeSet<{}>", self.rust_type(&args[0]))
            }
            Type::Apply { name, args }
                if name == Syntax::TYPE_PRIORITY_QUEUE && !args.is_empty() =>
            {
                format!("std::collections::BinaryHeap<{}>", self.rust_type(&args[0]))
            }
            Type::Apply { name, args } if name == Syntax::TYPE_LRU && args.len() >= 2 => {
                format!(
                    "JetCache<{}, {}>",
                    self.rust_type(&args[0]),
                    self.rust_type(&args[1])
                )
            }
            // D-TAG1: Bag<T> → HashMap<T, usize>.
            Type::Apply { name, args } if name == "Bag" && !args.is_empty() => {
                format!(
                    "std::collections::HashMap<{}, usize>",
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args } if name == "Deque" && !args.is_empty() => {
                format!("std::collections::VecDeque<{}>", self.rust_type(&args[0]))
            }
            // S58 (E2-M13): `Ptr<T>` lowers to a Rust raw pointer `*mut T`.
            // Memory safety is enforced in sema (the `#Unsafe` gate); codegen
            // is dumb.
            Type::Apply { name, args } if name == Syntax::TYPE_PTR && args.len() == 1 => {
                format!("*mut {}", self.rust_type(&args[0]))
            }
            Type::Apply { name, args } => {
                let head = if let Some((alias, leaf)) = name.split_once('.') {
                    self.import_mods.get(alias).map_or_else(
                        || user_type_rust(name),
                        |rust_mod| {
                            format!(
                                "{}{}::{}",
                                self.root_prefix,
                                rust_mod,
                                user_type_rust(leaf)
                            )
                        },
                    )
                } else {
                    user_type_rust(name)
                };
                if args.is_empty() {
                    head
                } else {
                    format!(
                        "{}<{args}>",
                        head,
                        args = args
                            .iter()
                            .map(|a| self.rust_type(a))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            }
            // Codegen only ever constructs/sees a singleton `TraitObject` (the
            // D-ANY-JAI1 multi-trait bound loop element never reaches codegen —
            // see the type's doc comment); join defensively rather than assume.
            Type::TraitObject(t) => format!(
                "Box<dyn {}>",
                t.iter()
                    .map(|n| Generics::user_trait_rust(n))
                    .collect::<Vec<_>>()
                    .join(" + ")
            ),
            Type::Fn {
                params,
                ret,
                return_view_provenance,
                ..
            } => self.rust_fn_trait(
                params,
                ret.as_deref(),
                return_view_provenance.as_ref(),
                false,
            ),
            Type::Tuple(fields) => tuple_struct_name(&tuple_fields_plain(fields)),
            // D-FIXARR1 (ratified 2026-06-22): [T#N] lowers to a real Rust stack array [T; N].
            // All size/bounds checks live in sema (I3). The Rust type is [E; N].
            Type::FixedList { elem, len, .. } => format!("[{}; {}]", self.rust_type(elem), len),
            // D-QUAL4=A: tagged types are transparent to codegen.
            Type::Tagged { inner, .. } => self.rust_type(inner),
            // D-UNIONTYPE1=A: closed structural sum → one compiler-generated enum.
            Type::Union(members) => {
                format!("user_{}", crate::AST::union_enum_name(members))
            }
        }
    }

    pub(crate) fn rust_fn_trait(
        &self,
        params: &[Type],
        ret: Option<&Type>,
        return_view_provenance: Option<&crate::AST::ViewProvenanceMap>,
        mut_capture: bool,
    ) -> String {
        let thread_safe = params.len() == 1
            && matches!(&params[0], Type::Named(name) if name == "HTTPHandler")
            && matches!(ret, Some(Type::Named(name)) if name == "HTTPHandler");
        let has_view_return = ret.is_some_and(|ty| self.type_contains_view(ty));
        let owner_params: HashSet<usize> = return_view_provenance
            .map(|map| {
                map.values()
                    .flat_map(|provenance| provenance.sources.iter())
                    .filter_map(|source| match source.source {
                        crate::AST::ViewSource::Parameter(index) => Some(index),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_else(|| {
                has_view_return
                    .then(|| {
                        params
                            .iter()
                            .enumerate()
                            .filter_map(|(index, ty)| (!ty.is_scalar()).then_some(index))
                            .collect()
                    })
                    .unwrap_or_default()
            });
        let mut independent_lifetimes = Vec::new();
        // HRTB binder must not reuse the enclosing function's `'__jet_view`
        // (E0496 shadow when a view-returning fn takes a view-returning callback).
        const FN_VIEW: &str = "'__jet_fn_view";
        let ps = params
            .iter()
            .enumerate()
            .map(|(index, p)| {
                if thread_safe {
                    self.rust_type(p)
                } else {
                    let rust = rust_param_type(self, AccessConvention::Read, p);
                    if owner_params.contains(&index) {
                        if let Some(rest) = rust.strip_prefix("&mut ") {
                            format!("&{FN_VIEW} mut {rest}")
                        } else if let Some(rest) = rust.strip_prefix('&') {
                            format!("&{FN_VIEW} {rest}")
                        } else {
                            rust
                        }
                    } else if has_view_return {
                        let lifetime = format!("'__jet_arg{index}");
                        if let Some(rest) = rust.strip_prefix("&mut ") {
                            independent_lifetimes.push(lifetime.clone());
                            format!("&{lifetime} mut {rest}")
                        } else if let Some(rest) = rust.strip_prefix('&') {
                            independent_lifetimes.push(lifetime.clone());
                            format!("&{lifetime} {rest}")
                        } else {
                            rust
                        }
                    } else {
                        rust
                    }
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let r = ret
            .map(|t| {
                if has_view_return {
                    self.rust_type_with_view_lifetime(t)
                        .replace("'__jet_view", FN_VIEW)
                } else {
                    self.rust_type(t)
                }
            })
            .unwrap_or_else(|| "()".to_string());
        let trait_name = if mut_capture { "FnMut" } else { "Fn" };
        // D-SPREAD/#1357: `Fn` values use Rc/Arc so collection `.cloned()` works.
        // `FnMut` stays `Box` (shared Rc cannot call_mut).
        if thread_safe {
            format!("std::sync::Arc<dyn {trait_name}({ps}) -> {r} + Send + Sync>")
        } else if mut_capture {
            if has_view_return {
                let lifetimes = std::iter::once(FN_VIEW.to_string())
                    .chain(independent_lifetimes)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("Box<dyn for<{lifetimes}> {trait_name}({ps}) -> {r}>")
            } else {
                format!("Box<dyn {trait_name}({ps}) -> {r}>")
            }
        } else if has_view_return {
            let lifetimes = std::iter::once(FN_VIEW.to_string())
                .chain(independent_lifetimes)
                .collect::<Vec<_>>()
                .join(", ");
            format!("std::rc::Rc<dyn for<{lifetimes}> {trait_name}({ps}) -> {r}>")
        } else {
            format!("std::rc::Rc<dyn {trait_name}({ps}) -> {r}>")
        }
    }

    pub(crate) fn mangle_name(&self, name: &str) -> String {
        mangle(name)
    }

    pub(crate) fn type_prefix(&self, type_name: &str) -> String {
        user_type_rust(type_name)
    }
}

pub(crate) fn rust_param_type(cx: &Cx, convention: AccessConvention, ty: &Type) -> String {
    if let Type::Tagged { marker, inner } = ty {
        if marker == crate::AST::CPP_CALLBACK_ABI_MARKER {
            if let Type::Fn { params, ret, .. } = inner.as_ref() {
                let params = params
                    .iter()
                    .map(|param| cx.rust_type(param))
                    .collect::<Vec<_>>()
                    .join(", ");
                let ret = ret
                    .as_deref()
                    .map(|ret| format!(" -> {}", cx.rust_type(ret)))
                    .unwrap_or_default();
                return format!("extern \"C\" fn({params}){ret}");
            }
        }
    }
    let base = cx.rust_type(ty);
    if matches!(ty, Type::Named(n) if cx.trait_names.contains(n))
        || matches!(ty, Type::TraitObject(_))
    {
        return match convention {
            AccessConvention::Read => {
                format!("&{base}")
            }
            AccessConvention::Write => format!("&mut {base}"),
            AccessConvention::Move => base,
        };
    }
    // Type parameters obey the same explicit access convention as concrete
    // types. D-NETIO-CONTRACT2 relies on `&stream: T` becoming `&mut T`;
    // forcing every generic parameter by value would move non-cloneable handles.
    if matches!(ty, Type::Named(n) if Generics::is_type_var_name(n)
        || cx.current_type_params.borrow().contains(n.as_str()))
    {
        return match convention {
            AccessConvention::Read => {
                format!("&{base}")
            }
            AccessConvention::Write => format!("&mut {base}"),
            AccessConvention::Move => base,
        };
    }
    match convention {
        AccessConvention::Read if ty.is_scalar() => {
            base
        }
        AccessConvention::Read => format!("&{}", base),
        AccessConvention::Write => format!("&mut {}", base),
        AccessConvention::Move => base,
    }
}

pub(crate) fn rust_return_type(cx: &Cx, ty: &Type) -> String {
    cx.rust_type(ty)
}

pub(crate) fn build_cx(prog: &Program, src: &str, file: &str) -> Cx {
    let extern_funcs = extern_func_map(&prog.items);
    build_cx_items(&prog.items, src, file, None, &extern_funcs)
}

fn extern_func_map(items: &[Item]) -> HashMap<String, String> {
    fn collect(items: &[Item], map: &mut HashMap<String, String>) {
        for item in items {
            if let Item::ExternRust(block) = item {
                for ef in &block.functions {
                    map.insert(ef.name.clone(), format!("jet_ffi_{}", ef.name));
                }
            } else if let Item::Func(func) = item {
                if func.inline_foreign.is_some() {
                    map.insert(func.name.clone(), format!("jet_ffi_{}", func.name));
                }
            } else if let Item::CModule(module) = item {
                for function in module
                    .functions
                    .iter()
                    .filter(|function| function.hidden_c_bridge_compatible())
                {
                    map.insert(
                        function.name.clone(),
                        format!("jet_ffi_{}", function.name),
                    );
                }
            } else if let Item::CodeModule(module) = item {
                if let Some(body) = &module.body {
                    collect(body, map);
                }
            }
        }
    }
    let mut map = HashMap::new();
    collect(items, &mut map);
    map
}

pub(crate) fn bundle_extern_funcs(bundle: &ProgramBundle) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for module in &bundle.modules {
        let module_funcs = extern_func_map(&module.items);
        for (name, wrapper) in module_funcs {
            map.insert(name.clone(), wrapper.clone());
            map.insert(format!("user_{}::{name}", module.alias), wrapper);
        }
        if module.display.starts_with("cpp.") {
            for item in &module.items {
                if let Item::Impl(def) = item {
                    for method in def.methods.iter().filter(|method| method.is_pub) {
                        map.insert(
                            foreign_binding_method_key(&def.type_name, &method.name),
                            String::new(),
                        );
                    }
                }
            }
        }
    }
    map
}

pub(crate) fn foreign_binding_method_key(owner: &str, method: &str) -> String {
    format!("__jet_foreign_method__{owner}__{method}")
}

/// Add local and imported unit-family facts under the names used by this module.
pub(crate) fn register_bundle_unit_metadata(
    cx: &mut Cx,
    bundle: &ProgramBundle,
    module_idx: usize,
) {
    let imported = bundle.modules[module_idx]
        .imports
        .iter()
        .filter_map(|import| {
            bundle
                .import_targets
                .get(&(module_idx, import.span))
                .copied()
                .map(|target| {
                    let qualifier = bundle.modules[target].alias.clone();
                    (target, Some(qualifier))
                })
        });
    for (target, qualifier) in std::iter::once((module_idx, None)).chain(imported) {
        let module = &bundle.modules[target];
        for item in &module.items {
            if let Item::UnitFamily(family) = item {
                if qualifier.is_some() && !family.is_pub {
                    continue;
                }
                let dimension = family.resolved_dimension.clone();
                for member in family.distinct_defs() {
                    let name = qualifier.as_ref().map_or_else(
                        || member.name.clone(),
                        |qualifier| format!("{qualifier}.{}", member.name),
                    );
                    cx.type_names.insert(name.clone());
                    cx.distinct_types
                        .insert(name.clone(), (member.base.clone(), member.is_numeric));
                    let kind = member
                        .quantity
                        .map(|(_, kind)| kind)
                        .unwrap_or(crate::AST::QuantityKind::Linear);
                    let Some(source) =
                        unit_family_member_for_type(family, &member.name, kind)
                    else {
                        continue;
                    };
                    cx.unit_labels
                        .insert(name.clone(), unit_label(family, source));
                    if family.base.is_some() || dimension.is_some() {
                        cx.unit_facts
                            .insert(name, unit_fact(family, source, dimension.clone(), kind));
                    }
                }
            }
        }
        if let Some(qualifier) = &qualifier {
            for item in &module.items {
                let Item::Impl(implementation) = item else {
                    continue;
                };
                let qualified = format!("{qualifier}.{}", implementation.type_name);
                if implementation.trait_name.as_deref() == Some(Syntax::TRAIT_DISPLAY)
                    && cx.unit_labels.contains_key(&qualified)
                {
                    cx.display_types.insert(qualified);
                }
            }
        }
    }
}

/// Mirror the bundle-level import maps `emit_bundle` fills before lowering.
/// `build_cx_items` alone leaves `core_imports` empty; without this, JIT
/// lowering mis-gates `use core.tasks as tasks` spawn/channel calls.
pub(crate) fn populate_cx_from_bundle(cx: &mut Cx, bundle: &ProgramBundle, module_idx: usize) {
    use super::Imports::{
        core_import_map, foreign_type_map, import_mod_map, import_ret_map, import_sig_map,
        reexport_call_map, unqualified_import_maps,
    };
    cx.import_mods = import_mod_map(bundle, module_idx);
    cx.foreign_types = foreign_type_map(bundle, module_idx);
    cx.reexport_calls = reexport_call_map(bundle, module_idx);
    cx.import_sigs = import_sig_map(bundle, module_idx);
    cx.import_rets = import_ret_map(bundle, module_idx);
    cx.core_imports = core_import_map(bundle, module_idx);
    register_core_close_types(cx);
    register_core_import_surfaces(cx);
    cx.used_core = bundle.used_core.clone();
    cx.ffi_callback_fns = bundle.ffi_callback_fns.clone();
    register_bundle_unit_metadata(cx, bundle, module_idx);
    register_imported_methods(cx, bundle, module_idx);
    let (uinline, ufile) = unqualified_import_maps(bundle, module_idx);
    cx.unqualified_inline = uinline;
    cx.unqualified_file = ufile;
    cx.package_edition = bundle.edition.clone();
}

fn register_imported_methods(cx: &mut Cx, bundle: &ProgramBundle, module_idx: usize) {
    let imported = bundle.modules[module_idx]
        .imports
        .iter()
        .filter_map(|import| bundle.import_targets.get(&(module_idx, import.span)).copied());
    for target in imported {
        for item in &bundle.modules[target].items {
            let (owner, methods) = match item {
                Item::Struct(def) => (&def.name, &def.methods),
                Item::Enum(def) => (&def.name, &def.methods),
                Item::Impl(def) => (&def.type_name, &def.methods),
                _ => continue,
            };
            for method in methods.iter().filter(|method| method.is_pub) {
                let key = (owner.clone(), method.name.clone());
                if let Some(self_param) = method.params.iter().find(|p| p.name == Syntax::KW_SELF)
                {
                    cx.method_self_convs
                        .entry(key.clone())
                        .or_insert(self_param.convention);
                }
                cx.method_sigs
                    .entry(key.clone())
                    .or_insert_with(|| method_sig_params(method));
                cx.method_rets
                    .entry(key)
                    .or_insert_with(|| method.return_type.clone());
            }
        }
    }
}

fn register_core_close_types(cx: &mut Cx) {
    let imports = |module: &str| cx.core_imports.values().any(|m| m == module);
    if imports("core.files") {
        cx.close_types.extend(
            ["FileReader", "FileWriter", "FileLock"]
                .into_iter()
                .map(str::to_string),
        );
    }
    if imports("core.net") {
        cx.close_types.extend(
            ["TcpStream", "UnixStream", "TLSStream"]
                .into_iter()
                .map(str::to_string),
        );
    }
    if imports(Syntax::CORE_MEM_MODULE) || imports(Syntax::CORE_MEM_ALLOC_MODULE) {
        cx.close_types.extend(
            ["Arena", "Bump", "Pool", "Fixed"]
                .into_iter()
                .map(str::to_string),
        );
    }
    if imports("jet.db") {
        cx.close_types.insert("DBConnection".to_string());
    }
}

/// Populate value-shape tables that depend on bundle-resolved Core imports.
pub(crate) fn register_core_import_surfaces(cx: &mut Cx) {
    if cx.core_imports.values().any(|module| module == "core.auth") {
        let zero = Span::new(0, 0);
        let field = |name: &str, ty: Type| VariantField {
            name: name.to_string(),
            name_span: zero,
            ty,
            ty_span: zero,
        };
        let mut variants = vec![
            ("InvalidSignature".to_string(), VariantPayload::Unit),
            ("WeakKey".to_string(), VariantPayload::Unit),
            ("TokenExpired".to_string(), VariantPayload::Unit),
        ];
        variants.extend(
            ["MalformedToken", "UnsupportedToken", "MissingClaim", "DecodeError"]
                .into_iter()
                .map(|name| (name.to_string(), VariantPayload::Single(Type::String, zero))),
        );
        variants.push((
            "WrongAudience".to_string(),
            VariantPayload::Named(vec![
                field("expected", Type::String),
                field("actual", Type::String),
            ]),
        ));
        variants.push((
            "WrongIssuer".to_string(),
            VariantPayload::Named(vec![
                field("expected", Type::String),
                field("actual", Type::Option(Box::new(Type::String))),
            ]),
        ));
        for (variant, _) in &variants {
            cx.variant_owner.insert(variant.clone(), "AuthError".to_string());
        }
        cx.enum_variants.insert("AuthError".to_string(), variants);
        cx.cloneable.insert("AuthError".to_string());
    }
    if cx.core_imports.values().any(|module| module == "core.tls") {
        let zero = Span::new(0, 0);
        let versions = vec![
            ("Tls12".to_string(), VariantPayload::Unit),
            ("Tls13".to_string(), VariantPayload::Unit),
        ];
        for (variant, _) in &versions { cx.variant_owner.insert(variant.clone(), "TLSVersion".to_string()); }
        cx.enum_variants.insert("TLSVersion".to_string(), versions);
        cx.cloneable.insert("TLSVersion".to_string());
        let roots = Type::Named("TLSRootCertificates".to_string());
        let trust = vec![
            ("System".to_string(), VariantPayload::Unit),
            ("SystemPlus".to_string(), VariantPayload::Single(roots.clone(), zero)),
            ("CustomOnly".to_string(), VariantPayload::Single(roots, zero)),
        ];
        for (variant, _) in &trust { cx.variant_owner.insert(variant.clone(), "TLSClientTrust".to_string()); }
        cx.enum_variants.insert("TLSClientTrust".to_string(), trust);
        cx.cloneable.insert("TLSClientTrust".to_string());
    }
    if !cx.core_imports.values().any(|module| module == Syntax::CORE_EMAIL_MODULE) {
        return;
    }
    let zero = Span::new(0, 0);
    for (name, variants) in [
        ("SMTPSecurity", vec![("StartTls".to_string(), VariantPayload::Unit), ("TLS".to_string(), VariantPayload::Unit)]),
        ("RecipientPolicy", vec![("RequireAll".to_string(), VariantPayload::Unit), ("DeliverAccepted".to_string(), VariantPayload::Unit)]),
        ("SMTPAuth", vec![
            ("None".to_string(), VariantPayload::Unit),
            ("Password".to_string(), VariantPayload::Named(vec![
                VariantField { name: "username".to_string(), name_span: zero, ty: Type::String, ty_span: zero },
                VariantField { name: "password".to_string(), name_span: zero, ty: Type::Named("Secret".to_string()), ty_span: zero },
            ])),
        ]),
        ("TLSTrust", vec![
            ("System".to_string(), VariantPayload::Unit),
            ("SystemPlusCa".to_string(), VariantPayload::Named(vec![
                VariantField { name: "pem".to_string(), name_span: zero,
                    ty: Type::List(Box::new(Type::IntN { signed: false, bits: 8 })), ty_span: zero },
            ])),
        ]),
    ] {
        for (variant, _) in &variants { cx.variant_owner.insert(variant.clone(), name.to_string()); }
        cx.enum_variants.insert(name.to_string(), variants);
        cx.cloneable.insert(name.to_string());
    }
    let error_fields = || [
        ("operation", Type::String),
        ("server", Type::Option(Box::new(Type::String))),
        ("code", Type::Option(Box::new(Type::Int))),
        ("reason", Type::String),
    ].into_iter().map(|(field, ty)| VariantField {
        name: field.to_string(), name_span: zero, ty, ty_span: zero,
    }).collect();
    let errors = [
        "Configuration", "DNS", "Connect", "TLS", "Auth", "Protocol", "Rejected",
        "Transient", "TimedOut", "Cancelled", "DeliveryUnknown",
    ].into_iter().map(|variant| (variant.to_string(), VariantPayload::Named(error_fields()))).collect::<Vec<_>>();
    for (variant, _) in &errors { cx.variant_owner.insert(variant.clone(), "EmailError".to_string()); }
    cx.enum_variants.insert("EmailError".to_string(), errors);
    cx.cloneable.insert("EmailError".to_string());
    cx.struct_fields.insert("Envelope".to_string(), vec![
        ("from".to_string(), Type::Named("Address".to_string())),
        ("recipients".to_string(), Type::List(Box::new(Type::Named("Address".to_string())))),
    ]);
    cx.struct_fields.insert("RecipientReport".to_string(), vec![
        ("address".to_string(), Type::Named("Address".to_string())),
        ("accepted".to_string(), Type::Bool), ("code".to_string(), Type::Int),
        ("message".to_string(), Type::String),
    ]);
    cx.struct_fields.insert("SendReport".to_string(), vec![
        ("server".to_string(), Type::String),
        ("accepted".to_string(), Type::List(Box::new(Type::Named("RecipientReport".to_string())))),
        ("rejected".to_string(), Type::List(Box::new(Type::Named("RecipientReport".to_string())))),
        ("response_code".to_string(), Type::Int), ("response".to_string(), Type::String),
        ("accepted_at".to_string(), Type::String),
    ]);
    cx.struct_fields.insert("Limits".to_string(), vec![
        ("max_reply_line_bytes".to_string(), Type::Int),
        ("max_reply_lines".to_string(), Type::Int),
        ("max_capabilities".to_string(), Type::Int),
        ("max_recipients".to_string(), Type::Int),
        ("max_message_bytes".to_string(), Type::Int),
        ("max_auth_challenge_bytes".to_string(), Type::Int),
    ]);
    cx.struct_fields.insert("SMTPConfig".to_string(), vec![
        ("host".to_string(), Type::String), ("port".to_string(), Type::Int),
        ("security".to_string(), Type::Named("SMTPSecurity".to_string())),
        ("auth".to_string(), Type::Named("SMTPAuth".to_string())),
        ("recipient_policy".to_string(), Type::Named("RecipientPolicy".to_string())),
        ("trust".to_string(), Type::Named("TLSTrust".to_string())),
        ("limits".to_string(), Type::Named("Limits".to_string())),
        ("dkim".to_string(), Type::Option(Box::new(Type::Named("DkimConfig".to_string())))),
    ]);
    cx.struct_fields.insert("DkimConfig".to_string(), vec![
        ("domain".to_string(), Type::String),
        ("selector".to_string(), Type::String),
        ("private_key".to_string(), Type::Named("Secret".to_string())),
        ("signed_headers".to_string(), Type::List(Box::new(Type::String))),
    ]);
    cx.cloneable.extend(["Envelope".to_string(), "RecipientReport".to_string(), "SendReport".to_string(), "Limits".to_string()]);
}

pub(crate) fn build_cx_items(
    items: &[Item],
    src: &str,
    file: &str,
    link: Option<&FfiLink>,
    extern_funcs: &HashMap<String, String>,
) -> Cx {
    let mut cx = Cx {
        sigs: HashMap::new(),
        fn_types: HashMap::new(),
        method_sigs: HashMap::new(),
        method_self_convs: HashMap::new(),
        method_rets: HashMap::new(),
        consts: HashMap::new(),
        const_values: HashMap::new(),
        type_names: HashSet::new(),
        distinct_types: HashMap::new(),
        distinct_ranges: HashMap::new(),
        unit_facts: HashMap::new(),
        unit_labels: HashMap::new(),
        type_aliases: HashMap::new(),
        trait_names: HashSet::new(),
        struct_fields: HashMap::new(),
        serde_wire_params: HashMap::new(),
        enum_variants: HashMap::new(),
        variant_owner: HashMap::new(),
        boxed_edges: HashSet::new(),
        cloneable: HashSet::new(),
        migrations: HashMap::new(),
        columnar: HashSet::new(),
        comparable: HashSet::new(),
        auto_printable: HashSet::new(),
        auto_debug: HashSet::new(),
        auto_equatable: HashSet::new(),
        hashable: HashSet::new(),
        partial_ord: HashSet::new(),
        patchable: HashSet::new(),
        computed_fields: HashMap::new(),
        src: src.to_string(),
        file: file.to_string(),
        test_mode: false,
        coverage: false,
        debug_linemap: false,
        import_mods: HashMap::new(),
        foreign_types: HashMap::new(),
        reexport_calls: HashMap::new(),
        import_sigs: HashMap::new(),
        import_rets: HashMap::new(),
        core_imports: HashMap::new(),
        used_core: HashSet::new(),
        ffi_callback_fns: HashSet::new(),
        root_prefix: String::new(),
        ffi_crate: link.map(|l| l.crate_name.clone()),
        extern_funcs: extern_funcs.clone(),
        code_modules: HashSet::new(),
        unqualified_inline: HashMap::new(),
        unqualified_file: HashMap::new(),
        trait_methods: HashSet::new(),
        rollback_types: HashSet::new(),
        display_types: HashSet::new(),
        close_types: HashSet::new(),
        iterable_hooks: HashMap::new(),
        index_hooks: HashMap::new(),
        current_fn: std::cell::RefCell::new(String::new()),
        struct_type_params: HashMap::new(),
        struct_type_param_order: HashMap::new(),
        current_type_params: std::cell::RefCell::new(HashSet::new()),
        jit_spawn_lambdas: std::cell::RefCell::new(Vec::new()),
        jit_spawn_site_base: 0,
        jit_method_calls: std::cell::RefCell::new(std::collections::BTreeMap::new()),
        jit_generic_calls: std::cell::RefCell::new(std::collections::BTreeMap::new()),
        jit_canonical_deopt: std::cell::RefCell::new(HashSet::new()),
        jit_canonical_calls: std::cell::RefCell::new(HashSet::new()),
        jit_local_call_prefix: None,
        fn_type_params: HashMap::new(),
        variadic_bound_fns: HashMap::new(),
        needed_variadic_arities: std::cell::RefCell::new(std::collections::BTreeMap::new()),
        active_os: crate::Syntax::OSTarget::host(),
        package_edition: "2027".to_string(),
        in_stm_transact: std::cell::Cell::new(false),
        stm_touched: std::cell::Cell::new(false),
    };

    let io_context = Type::Named(Syntax::TYPE_IO_CONTEXT.to_string());
    cx.struct_fields.insert(Syntax::TYPE_IO_CONTEXT.to_string(), vec![
        ("operation".to_string(), Type::Named(Syntax::TYPE_IO_OPERATION.to_string())),
        ("resource".to_string(), Type::Option(Box::new(Type::String))),
        ("os_code".to_string(), Type::Option(Box::new(Type::Int))),
        ("cause".to_string(), Type::Option(Box::new(Type::String))),
    ]);
    cx.cloneable.insert(Syntax::TYPE_IO_CONTEXT.to_string());
    cx.enum_variants.insert(Syntax::TYPE_IO_ERROR.to_string(), Syntax::IO_ERROR_VARIANTS.iter().map(|name| ((*name).to_string(), VariantPayload::Single(io_context.clone(), Span::new(0, 0)))).collect());
    cx.enum_variants.insert(Syntax::TYPE_IO_OPERATION.to_string(), Syntax::IO_OPERATION_VARIANTS.iter().map(|name| ((*name).to_string(), VariantPayload::Unit)).collect());
    for name in Syntax::IO_ERROR_VARIANTS { cx.variant_owner.insert((*name).to_string(), Syntax::TYPE_IO_ERROR.to_string()); }
    for name in Syntax::IO_OPERATION_VARIANTS { cx.variant_owner.insert((*name).to_string(), Syntax::TYPE_IO_OPERATION.to_string()); }
    cx.cloneable.insert(Syntax::TYPE_IO_ERROR.to_string());
    cx.cloneable.insert(Syntax::TYPE_IO_OPERATION.to_string());
    let zero = Span::new(0, 0);
    let http_operations = ["ClientConnect", "ServerBind", "ServeListener"];
    cx.enum_variants.insert(
        "HTTPOperation".to_string(),
        http_operations
            .iter()
            .map(|name| ((*name).to_string(), VariantPayload::Unit))
            .collect(),
    );
    for name in http_operations {
        cx.variant_owner
            .insert(name.to_string(), "HTTPOperation".to_string());
    }
    let http_proxy = vec![
        ("FromEnvironment".to_string(), VariantPayload::Unit),
        ("None".to_string(), VariantPayload::Unit),
        ("Url".to_string(), VariantPayload::Single(Type::String, zero)),
    ];
    for (variant, _) in &http_proxy {
        cx.variant_owner
            .insert(variant.clone(), "HTTPProxy".to_string());
    }
    cx.enum_variants.insert("HTTPProxy".to_string(), http_proxy);
    cx.cloneable.insert("HTTPProxy".to_string());
    let http_redirect_policy = vec![(
        "Follow".to_string(),
        VariantPayload::Named(vec![
            VariantField {
                name: "max".to_string(),
                name_span: zero,
                ty: Type::Int,
                ty_span: zero,
            },
            VariantField {
                name: "same_origin_credentials".to_string(),
                name_span: zero,
                ty: Type::Bool,
                ty_span: zero,
            },
        ]),
    )];
    for (variant, _) in &http_redirect_policy {
        cx.variant_owner
            .insert(variant.clone(), "HTTPRedirectPolicy".to_string());
    }
    cx.enum_variants
        .insert("HTTPRedirectPolicy".to_string(), http_redirect_policy);
    cx.cloneable.insert("HTTPRedirectPolicy".to_string());
    let http_retry_policy = vec![
        ("None".to_string(), VariantPayload::Unit),
        ("Safe".to_string(), VariantPayload::Unit),
        ("Idempotent".to_string(), VariantPayload::Unit),
    ];
    for (variant, _) in &http_retry_policy {
        cx.variant_owner
            .insert(variant.clone(), "HTTPRetryPolicy".to_string());
    }
    cx.enum_variants
        .insert("HTTPRetryPolicy".to_string(), http_retry_policy);
    cx.cloneable.insert("HTTPRetryPolicy".to_string());
    let http_cookie_jar = vec![("Memory".to_string(), VariantPayload::Unit)];
    cx.variant_owner
        .insert("Memory".to_string(), "HTTPCookieJar".to_string());
    cx.enum_variants
        .insert("HTTPCookieJar".to_string(), http_cookie_jar);
    cx.cloneable.insert("HTTPCookieJar".to_string());
    let http_compress_encoding = vec![("Gzip".to_string(), VariantPayload::Unit)];
    cx.variant_owner
        .insert("Gzip".to_string(), "HTTPCompressEncoding".to_string());
    cx.enum_variants
        .insert("HTTPCompressEncoding".to_string(), http_compress_encoding);
    cx.cloneable.insert("HTTPCompressEncoding".to_string());
    cx.cloneable.insert("HTTPCorsPolicy".to_string());
    // D-HTTP-CORS1=A: `.Any` or `.List([...])` origins.
    let http_cors_origins = vec![
        ("Any".to_string(), VariantPayload::Unit),
        (
            "List".to_string(),
            VariantPayload::Single(Type::List(Box::new(Type::String)), zero),
        ),
    ];
    for (variant, _) in &http_cors_origins {
        cx.variant_owner
            .insert(variant.clone(), "HTTPCorsOrigins".to_string());
    }
    cx.enum_variants
        .insert("HTTPCorsOrigins".to_string(), http_cors_origins);
    cx.cloneable.insert("HTTPCorsOrigins".to_string());
    let mut http_errors = [
        "InvalidMethod",
        "InvalidUrl",
        "InvalidHeader",
        "InvalidStatus",
        "BodyConsumed",
        "InvalidFraming",
        "UnsupportedEncoding",
        "Cancelled",
    ]
    .into_iter()
    .map(|name| (name.to_string(), VariantPayload::Unit))
    .collect::<Vec<_>>();
    for (name, field, ty) in [
        ("BodyTooLarge", "limit", Type::Int),
        ("Resolve", "host", Type::String),
        ("Connect", "address", Type::String),
        ("TLS", "stage", Type::String),
        ("Timeout", "phase", Type::String),
        ("Proxy", "stage", Type::String),
        ("Redirect", "reason", Type::String),
        ("Protocol", "version", Type::String),
        ("IO", "operation", Type::String),
        ("Policy", "reason", Type::String),
        ("ResourceUnavailable", "resource", Type::String),
        ("Internal", "incident_id", Type::String),
        (
            "UnsupportedTarget",
            "operation",
            Type::Named("HTTPOperation".to_string()),
        ),
    ] {
        http_errors.push((
            name.to_string(),
            VariantPayload::Named(vec![VariantField {
                name: field.to_string(),
                name_span: zero,
                ty,
                ty_span: zero,
            }]),
        ));
    }
    for (name, _) in &http_errors {
        cx.variant_owner
            .insert(name.clone(), "HTTPError".to_string());
    }
    cx.enum_variants
        .insert("HTTPError".to_string(), http_errors);
    cx.cloneable.insert("HTTPError".to_string());
    cx.cloneable.insert("HTTPOperation".to_string());
    // D-WS1=B
    let mut ws_errors: Vec<(String, VariantPayload)> = [
        "InvalidUrl",
        "InvalidHandshake",
        "Protocol",
        "Timeout",
        "Closed",
        "Cancelled",
        "UnsupportedTarget",
    ]
    .into_iter()
    .map(|name| (name.to_string(), VariantPayload::Unit))
    .collect();
    ws_errors.push((
        "MessageTooLarge".to_string(),
        VariantPayload::Named(vec![VariantField {
            name: "limit".to_string(),
            name_span: zero,
            ty: Type::Int,
            ty_span: zero,
        }]),
    ));
    ws_errors.push((
        "IO".to_string(),
        VariantPayload::Named(vec![VariantField {
            name: "operation".to_string(),
            name_span: zero,
            ty: Type::String,
            ty_span: zero,
        }]),
    ));
    for (name, _) in &ws_errors {
        cx.variant_owner.insert(name.clone(), "WsError".to_string());
    }
    cx.enum_variants.insert("WsError".to_string(), ws_errors);
    cx.cloneable.insert("WsError".to_string());
    cx.cloneable.insert("WsConn".to_string());
    cx.cloneable.insert("WsMessage".to_string());
    for name in [
        "Browser",
        "BrowserContext",
        "BrowserPage",
        "BrowserFrame",
        "BrowserLocator",
        "BrowserIntercept",
        "BrowserEvent",
        "BrowserTrace",
        "BrowserReceipt",
        "BrowserPrivacy",
        "BrowserError",
        "BrowserCapabilities",
        "BrowserProfile",
        "BrowserTimeout",
        "BrowserProtocol",
    ] {
        cx.cloneable.insert(name.to_string());
    }
    cx.enum_variants.insert(
        Syntax::TYPE_ORDERING.to_string(),
        ["Less", "Equal", "Greater"]
            .into_iter()
            .map(|name| (name.to_string(), VariantPayload::Unit))
            .collect(),
    );
    for name in ["Less", "Equal", "Greater"] {
        cx.variant_owner
            .insert(name.to_string(), Syntax::TYPE_ORDERING.to_string());
    }
    cx.cloneable.insert(Syntax::TYPE_ORDERING.to_string());

    for item in items {
        match item {
            Item::Func(f) => {
                cx.fn_type_params.insert(
                    f.name.clone(),
                    f.type_params.iter().map(|param| param.name.clone()).collect(),
                );
                cx.sigs.insert(
                    f.name.clone(),
                    f.params
                        .iter()
                        .map(|p| {
                            let conv = p.convention;
                            let ty = if p.variadic {
                                Type::List(Box::new(p.ty.clone()))
                            } else {
                                p.ty.clone()
                            };
                            (conv, ty)
                        })
                        .collect(),
                );
                cx.fn_types.insert(
                    f.name.clone(),
                    Type::Fn {
                        params: f
                            .params
                            .iter()
                            .map(|p| {
                                if p.variadic {
                                    Type::List(Box::new(p.ty.clone()))
                                } else {
                                    p.ty.clone()
                                }
                            })
                            .collect(),
                        ret: f.return_type.clone().map(Box::new),
                        effect_bound: None,
                        return_view_provenance: f.return_view_provenance.clone(),
                    },
                );
                // D-ANY-JAI1/D-VARARGBOUND1 (c7jaiany): a trait-bounded variadic
                // (`...Trait` / `...[A, B]`) has no single Rust signature — record
                // it so call-site lowering routes to the per-arity function
                // `Codegen/VariadicBound.rs` synthesizes instead of a normal call.
                if let Some(last) = f.params.last() {
                    if let Some(bounds) =
                        last.variadic_trait_bounds(|n| crate::Generics::is_builtin_trait(n))
                    {
                        cx.variadic_bound_fns
                            .insert(f.name.clone(), (f.params.len() - 1, bounds));
                    }
                }
            }
            Item::Struct(s) => {
                cx.type_names.insert(s.name.clone());
                let param_names = s.type_params.iter().map(|p| p.name.as_str()).collect::<HashSet<_>>();
                let mut wire = HashSet::new();
                for field in s.fields.iter().filter(|f| f.computed.is_none()
                    && !f.serde_markers.iter().any(|m| m.name == crate::Syntax::ATTR_SKIP)) {
                    for name in crate::Generics::free_type_params(&field.ty) {
                        if param_names.contains(name.as_str()) { wire.insert(name); }
                    }
                }
                cx.serde_wire_params.insert(s.name.clone(), wire);
                if s.layout == Some(crate::AST::StructLayout::Columnar) {
                    cx.columnar.insert(s.name.clone());
                }
                cx.struct_fields.insert(
                    s.name.clone(),
                    s.fields
                        .iter()
                        .map(|f| (f.name.clone(), f.ty.clone()))
                        .collect(),
                );
                // D-FIELDPOL1: computed field names, so TIR lowering routes a
                // read of one to a getter call instead of a member access.
                let computed: HashSet<String> = s
                    .fields
                    .iter()
                    .filter(|f| f.computed.is_some())
                    .map(|f| f.name.clone())
                    .collect();
                if !computed.is_empty() {
                    cx.computed_fields.insert(s.name.clone(), computed);
                }
                // c148: record the declared type params so multi-char names are
                // recognized everywhere (struct_is_generic, field_type_cloneable, …).
                cx.struct_type_params.insert(
                    s.name.clone(),
                    s.type_params.iter().map(|p| p.name.clone()).collect(),
                );
                cx.struct_type_param_order.insert(
                    s.name.clone(),
                    s.type_params.iter().map(|p| p.name.clone()).collect(),
                );
            }
            Item::Enum(e) => {
                cx.type_names.insert(e.name.clone());
                cx.enum_variants.insert(
                    e.name.clone(),
                    e.variants
                        .iter()
                        .map(|v| (v.name.clone(), v.payload.clone()))
                        .collect(),
                );
                for v in &e.variants {
                    cx.variant_owner.insert(v.name.clone(), e.name.clone());
                }
                // D-TAG1: group names resolve to their owning enum too, so a
                // group pattern (`.Fire ->`) finds its Rust type prefix.
                for g in &e.groups {
                    cx.variant_owner.insert(g.path.clone(), e.name.clone());
                }
            }
            Item::Const(c) => {
                let force_static = c.attrs.contains(&crate::AST::ConstAttr::ForceStatic);
                if c.is_comptime && !force_static {
                    // Inline the evaluated literal at every reference.
                    // `CtValue::serialize()` renders an empty `List([])` as a bare
                    // `vec![]` — fine when the splice site supplies a type (a `let`
                    // binding's annotation, a struct field, a typed param), but a
                    // comptime const inlines directly at a bare use site (e.g.
                    // `PATHS.join(sep)`), where nothing pins the element type and
                    // rustc rejects the untyped `vec![]` as E0282 (I2). `c.ty` (set
                    // by sema alongside `c.ct` — see `ConstDef::ty`) carries the
                    // binding's real Jet type even when the value has no elements to
                    // sample it from, so an empty list renders as a typed
                    // `Vec::<T>::new()` instead.
                    let serialized = match (c.ct.as_ref(), c.ty.as_ref()) {
                        (Some(CtValue::List(xs)), Some(Type::List(inner))) if xs.is_empty() => {
                            format!("Vec::<{}>::new()", cx.rust_type(inner))
                        }
                        (Some(v), _) => v.serialize(),
                        (None, _) => "Default::default()".to_string(),
                    };
                    cx.consts.insert(c.name.clone(), serialized);
                    if let Some(value) = c.ct.as_ref() {
                        cx.const_values.insert(c.name.clone(), value.clone());
                    }
                } else {
                    cx.consts
                        .insert(c.name.clone(), mangle(&c.name).to_uppercase());
                }
            }
            Item::ExternRust(block) => {
                for ef in &block.functions {
                    cx.sigs.insert(
                        ef.name.clone(),
                        ef.params
                            .iter()
                            .map(|p| (p.convention, p.ty.clone()))
                            .collect(),
                    );
                    // JIT print/use sites need the real return type (AOT emits the
                    // Rust wrapper directly). Without this, TIR ExternCall falls
                    // back to Unit and `print(extern(...))` becomes a no-op.
                    cx.fn_types.insert(
                        ef.name.clone(),
                        Type::Fn {
                            params: ef.params.iter().map(|p| p.ty.clone()).collect(),
                            ret: ef.return_type.clone().map(Box::new),
                            effect_bound: None, return_view_provenance: None,
                        },
                    );
                }
            }
            Item::CModule(cm) => {
                // S59: C boundary functions register like extern rust so that
                // cross-module call sites resolve argument conventions.
                for ef in &cm.functions {
                    cx.sigs.insert(
                        ef.name.clone(),
                        ef.params
                            .iter()
                            .map(|p| (p.convention, p.ty.clone()))
                            .collect(),
                    );
                    cx.fn_types.insert(
                        ef.name.clone(),
                        Type::Fn {
                            params: ef.params.iter().map(|p| p.ty.clone()).collect(),
                            ret: ef.return_type.clone().map(Box::new),
                            effect_bound: None, return_view_provenance: None,
                        },
                    );
                }
            }
            Item::Trait(t) => {
                cx.trait_names.insert(t.name.clone());
                for m in &t.methods {
                    if let Some(self_param) = m.params.iter().find(|p| p.name == Syntax::KW_SELF) {
                        cx.method_self_convs
                            .insert((t.name.clone(), m.name.clone()), self_param.convention);
                    }
                    cx.method_sigs.insert(
                        (t.name.clone(), m.name.clone()),
                        m.params
                            .iter()
                            .filter(|p| p.name != Syntax::KW_SELF)
                            .map(|p| (p.convention, p.ty.clone()))
                            .collect(),
                    );
                    cx.method_rets
                        .insert((t.name.clone(), m.name.clone()), m.return_type.clone());
                }
            }
            // D-QUAL2: a tag erases — it contributes no codegen names.
            Item::Tag(_) => {}
            // D-MIGRATE4: collect migration blocks per type (source order = the
            // chain, oldest step first) so `emit_struct_migration` can emit the
            // runtime step functions + chain-walker for decodable
            // `#PublishedSchema` types. Types without blocks get nothing.
            Item::Migration(m) => {
                cx.migrations
                    .entry(m.type_name.clone())
                    .or_default()
                    .push(m.clone());
            }
            Item::EffectDecl(_)
            | Item::Impl(_) | Item::Test(_) | Item::Bench(_) | Item::Module(_) | Item::ErrorConv(_)
            | Item::StateDecl(_) // D-STATE-DECL: erases
            | Item::ProtocolDecl(_) // D-PROTO1/D-PROTO2: erases
            | Item::UserDerive(_) // D-METADERIVE1=A: erase (expanded in sema)
            | Item::GenericModule(_) // D-GENMOD2=A: template — erases
            | Item::ModuleAlias(_) => {} // D-GENMOD2=A: alias — erases after expansion
            Item::TypeAlias(a) => {
                cx.type_aliases.insert(
                    a.name.clone(),
                    (a.type_params.clone(), a.target.clone()),
                );
            }
            Item::Distinct(d) => {
                cx.type_names.insert(d.name.clone());
                cx.distinct_types
                    .insert(d.name.clone(), (d.base.clone(), d.is_numeric));
                if let Some((lo, hi, _)) = d.range {
                    cx.distinct_ranges.insert(d.name.clone(), (lo, hi));
                }
            }
            // D-QUAL3: each unit-family member registers as a `#Numeric` distinct
            // type erasing to `Float`.
            Item::UnitFamily(uf) => {
                let dimension = uf.resolved_dimension.clone();
                let affine = uf.base.is_some()
                    && uf
                        .members
                        .iter()
                        .any(|member| member.offset != crate::AST::UnitRatio::zero());
                for d in uf.distinct_defs() {
                    cx.type_names.insert(d.name.clone());
                    cx.distinct_types
                        .insert(d.name.clone(), (d.base.clone(), d.is_numeric));
                    // Mirror sema `unit_fact`: derive Point/Delta from the type
                    // name when `quantity` is unset (affine families).
                    let kind = d
                        .quantity
                        .as_ref()
                        .map(|(_, kind)| *kind)
                        .or_else(|| {
                            if !affine {
                                return Some(crate::AST::QuantityKind::Linear);
                            }
                            uf.members.iter().find_map(|member| {
                                let stem = crate::AST::UnitFamilyDef::type_name(&member.name);
                                if d.name == format!("{stem}Point") {
                                    Some(crate::AST::QuantityKind::Point)
                                } else if d.name == format!("{stem}Delta") {
                                    Some(crate::AST::QuantityKind::Delta)
                                } else {
                                    None
                                }
                            })
                        })
                        .unwrap_or(crate::AST::QuantityKind::Linear);
                    if let Some(member) = unit_family_member_for_type(uf, &d.name, kind) {
                        cx.unit_labels
                            .insert(d.name.clone(), unit_label(uf, member));
                    }
                    if uf.base.is_some() || dimension.is_some() {
                        if let Some(member) = unit_family_member_for_type(uf, &d.name, kind) {
                            cx.unit_facts.insert(
                                d.name.clone(),
                                unit_fact(uf, member, dimension.clone(), kind),
                            );
                        }
                    }
                }
            }
            Item::CodeModule(cm) => {
                // D-MOD2: register inline module alias and add mangled function sigs.
                if let Some(body) = &cm.body {
                    cx.code_modules.insert(cm.name.clone());
                    for inner in body {
                        if let Item::Func(f) = inner {
                            let mangled = format!("{}__{}", cm.name, f.name);
                            cx.sigs.insert(
                                mangled.clone(),
                                f.params.iter().map(|p| (p.convention, p.ty.clone())).collect(),
                            );
                            cx.fn_types.insert(
                                mangled,
                                Type::Fn {
                                    params: f.params.iter().map(|p| p.ty.clone()).collect(),
                                    ret: f.return_type.clone().map(Box::new),
                                    effect_bound: None, return_view_provenance: None,
                                },
                            );
                        }
                    }
                }
            }
        }
    }

    for item in items {
        match item {
            Item::Struct(s) => {
                cx.boxed_edges.extend(find_struct_box_edges(s, &cx));
                if type_is_cloneable_struct(s, &cx.type_names) {
                    cx.cloneable.insert(s.name.clone());
                }
                if type_is_comparable_struct(s, &cx.type_names) {
                    cx.comparable.insert(s.name.clone());
                }
                if crate::Traits::struct_auto_derive_ok(s) {
                    for (trait_name, selected) in [
                        (Generics::PRINTABLE, &mut cx.auto_printable),
                        (Generics::DEBUG, &mut cx.auto_debug),
                        (Generics::EQUATABLE, &mut cx.auto_equatable),
                    ] {
                        if crate::Traits::auto_derive_requested(
                            &s.type_markers,
                            trait_name,
                            s.auto_derive_default,
                        ) && !items.iter().any(|item| match item {
                            Item::Impl(i) => {
                                i.type_name == s.name
                                    && i.trait_name.as_deref() == Some(trait_name)
                            }
                            _ => false,
                        }) && !s
                            .trait_impls
                            .iter()
                            .any(|block| block.trait_name == trait_name)
                        {
                            selected.insert(s.name.clone());
                        }
                    }
                }
                for (t, _) in &s.derives {
                    if t == Generics::COMPARABLE {
                        cx.partial_ord.insert(s.name.clone());
                        cx.comparable.insert(s.name.clone());
                    }
                }
                for m in &s.methods {
                    if let Some(self_param) = m.params.iter().find(|p| p.name == Syntax::KW_SELF) {
                        cx.method_self_convs
                            .insert((s.name.clone(), m.name.clone()), self_param.convention);
                    }
                    cx.method_sigs
                        .insert((s.name.clone(), m.name.clone()), method_sig_params(m));
                    cx.method_rets
                        .insert((s.name.clone(), m.name.clone()), m.return_type.clone());
                }
                if s.derives
                    .iter()
                    .any(|(t, _)| t == Syntax::CONTRACT_PATCHABLE)
                {
                    cx.patchable.insert(s.name.clone());
                    let patch = format!("{}.Patch", s.name);
                    let base_ty = Type::Named(s.name.clone());
                    let patch_ty = Type::Named(patch.clone());
                    cx.method_sigs.insert(
                        (s.name.clone(), "apply".to_string()),
                        vec![(AccessConvention::Move, patch_ty.clone())],
                    );
                    cx.method_rets
                        .insert((s.name.clone(), "apply".to_string()), Some(base_ty.clone()));
                    cx.method_sigs.insert(
                        (s.name.clone(), "diff".to_string()),
                        vec![
                            (AccessConvention::Move, base_ty.clone()),
                            (AccessConvention::Move, base_ty),
                        ],
                    );
                    cx.method_rets
                        .insert((s.name.clone(), "diff".to_string()), Some(patch_ty.clone()));
                    cx.method_sigs.insert(
                        (patch.clone(), "merge".to_string()),
                        vec![(AccessConvention::Move, patch_ty.clone())],
                    );
                    cx.method_rets
                        .insert((patch, "merge".to_string()), Some(patch_ty));
                }
            }
            Item::Enum(e) => {
                cx.boxed_edges.extend(find_enum_box_edges(e, &cx));
                if type_is_cloneable_enum(e, &cx.type_names) {
                    cx.cloneable.insert(e.name.clone());
                }
                if type_is_comparable_enum(e, &cx.type_names) {
                    cx.comparable.insert(e.name.clone());
                }
                if crate::Traits::enum_auto_derive_ok(e) {
                    for (trait_name, selected) in [
                        (Generics::PRINTABLE, &mut cx.auto_printable),
                        (Generics::DEBUG, &mut cx.auto_debug),
                        (Generics::EQUATABLE, &mut cx.auto_equatable),
                    ] {
                        if crate::Traits::auto_derive_requested(
                            &e.type_markers,
                            trait_name,
                            e.auto_derive_default,
                        ) && !items.iter().any(|item| match item {
                            Item::Impl(i) => {
                                i.type_name == e.name
                                    && i.trait_name.as_deref() == Some(trait_name)
                            }
                            _ => false,
                        }) && !e
                            .trait_impls
                            .iter()
                            .any(|block| block.trait_name == trait_name)
                        {
                            selected.insert(e.name.clone());
                        }
                    }
                }
                if e.derives
                    .iter()
                    .any(|(trait_name, _)| trait_name == Generics::COMPARABLE)
                {
                    cx.partial_ord.insert(e.name.clone());
                    cx.comparable.insert(e.name.clone());
                }
                for m in &e.methods {
                    if let Some(self_param) = m.params.iter().find(|p| p.name == Syntax::KW_SELF) {
                        cx.method_self_convs
                            .insert((e.name.clone(), m.name.clone()), self_param.convention);
                    }
                    cx.method_sigs
                        .insert((e.name.clone(), m.name.clone()), method_sig_params(m));
                    cx.method_rets
                        .insert((e.name.clone(), m.name.clone()), m.return_type.clone());
                }
            }
            Item::Impl(i) => {
                for m in &i.methods {
                    if let Some(self_param) = m.params.iter().find(|p| p.name == Syntax::KW_SELF) {
                        cx.method_self_convs.insert(
                            (i.type_name.clone(), m.name.clone()),
                            self_param.convention,
                        );
                    }
                    cx.method_sigs
                        .insert((i.type_name.clone(), m.name.clone()), method_sig_params(m));
                    cx.method_rets
                        .insert((i.type_name.clone(), m.name.clone()), m.return_type.clone());
                    // S62: track trait-impl methods so call sites know not to mangle.
                    if i.trait_name.is_some() {
                        cx.trait_methods
                            .insert((i.type_name.clone(), m.name.clone()));
                    }
                }
            }
            _ => {}
        }
    }

    // D-TAG1: `hashable` (unlike `comparable`) can't trust "field type is a
    // known type name" — a `Named` field is only Eq+Hash-capable if THAT type
    // is itself hashable (e.g. it has no Float fields). Fixed-point over the
    // (monotonically growing) hashable set until nothing new is added.
    let mut hashable_changed = true;
    while hashable_changed {
        hashable_changed = false;
        for item in items {
            match item {
                Item::Struct(s) if !cx.hashable.contains(&s.name) => {
                    if type_is_hashable_struct(s, &cx.hashable) {
                        cx.hashable.insert(s.name.clone());
                        hashable_changed = true;
                    }
                }
                Item::Enum(e) if !cx.hashable.contains(&e.name) => {
                    if type_is_hashable_enum(e, &cx.hashable) {
                        cx.hashable.insert(e.name.clone());
                        hashable_changed = true;
                    }
                }
                _ => {}
            }
        }
    }

    // D-TXN-ROLLBACK layer 2: collect types that implement `Rollback` so the
    // TIR lowerer can use snapshot_custom instead of snapshot for those roots.
    for item in items {
        match item {
            Item::Impl(i) if i.trait_name.as_deref() == Some(Syntax::TRAIT_ROLLBACK) => {
                cx.rollback_types.insert(i.type_name.clone());
            }
            Item::Impl(i) if i.trait_name.as_deref() == Some(Syntax::TRAIT_DISPLAY) => {
                cx.display_types.insert(i.type_name.clone());
            }
            Item::Impl(i) if i.trait_name.as_deref() == Some(Syntax::TRAIT_CLOSE) => {
                cx.close_types.insert(i.type_name.clone());
            }
            Item::Struct(s) => {
                for block in &s.trait_impls {
                    if block.trait_name == Syntax::TRAIT_ROLLBACK {
                        cx.rollback_types.insert(s.name.clone());
                    }
                    if block.trait_name == Syntax::TRAIT_DISPLAY {
                        cx.display_types.insert(s.name.clone());
                    }
                    if block.trait_name == Syntax::TRAIT_CLOSE {
                        cx.close_types.insert(s.name.clone());
                    }
                }
            }
            Item::Enum(e) => {
                for block in &e.trait_impls {
                    if block.trait_name == Syntax::TRAIT_ROLLBACK {
                        cx.rollback_types.insert(e.name.clone());
                    }
                    if block.trait_name == Syntax::TRAIT_DISPLAY {
                        cx.display_types.insert(e.name.clone());
                    }
                    if block.trait_name == Syntax::TRAIT_CLOSE {
                        cx.close_types.insert(e.name.clone());
                    }
                }
            }
            _ => {}
        }
    }

    collect_iter_index_hooks(&mut cx, items);
    register_core_event_enums(&mut cx);
    let auto_derives = crate::Traits::TraitRegistry::auto_derives_for_items(items);
    apply_auto_derives(&mut cx, &auto_derives);

    cx
}

pub(crate) fn apply_auto_derives(
    cx: &mut Cx,
    auto_derives: &crate::Traits::TraitRegistry,
) {
    cx.auto_printable = auto_derives.auto_printable.clone();
    cx.auto_debug = auto_derives.auto_debug.clone();
    cx.auto_equatable = auto_derives.auto_equatable.clone();
}

fn register_core_event_enums(cx: &mut Cx) {
    const ENUMS: &[(&str, &[&str])] = &[
        ("Overflow", &["Block", "DropNewest", "DropOldest"]),
        ("FailurePolicy", &["StopFirst", "Collect", "Log", "Ignore"]),
        ("HookPolicy", &["FirstCancelElseTransform"]),
        ("HookOutcome", &["Continue", "Cancel", "Fail"]),
        ("HookDecision", &["Continue", "Transform", "Cancel", "Fail"]),
        (
            "DispatchState",
            &[
                "Delivered",
                "HandlerFailed",
                "DroppedNewest",
                "DroppedOldest",
                "Closed",
                "Cancelled",
                "DeadlineExceeded",
            ],
        ),
    ];
    for (enum_name, variants) in ENUMS {
        cx.enum_variants.entry((*enum_name).to_string()).or_insert_with(|| {
            variants
                .iter()
                .map(|variant| ((*variant).to_string(), VariantPayload::Unit))
                .collect()
        });
        for variant in *variants {
            // User/imported variants keep their owner. Pattern lowering resolves
            // the subject type first, so a same-named core variant remains exact.
            cx.variant_owner
                .entry((*variant).to_string())
                .or_insert_with(|| (*enum_name).to_string());
        }
    }
}

fn assoc_type_impl<'a>(assoc: &'a [(String, Span, Type)], name: &str) -> Option<&'a Type> {
    assoc.iter().find(|(n, _, _)| n == name).map(|(_, _, t)| t)
}

fn trait_impl_assoc(
    items: &[Item],
    type_name: &str,
    trait_name: &str,
    assoc_name: &str,
) -> Option<Type> {
    for item in items {
        match item {
            Item::Impl(i)
                if i.type_name == type_name && i.trait_name.as_deref() == Some(trait_name) =>
            {
                return assoc_type_impl(&i.assoc_type_impls, assoc_name).cloned();
            }
            Item::Struct(s) if s.name == type_name => {
                for block in &s.trait_impls {
                    if block.trait_name == trait_name {
                        return assoc_type_impl(&block.assoc_type_impls, assoc_name).cloned();
                    }
                }
            }
            Item::Enum(e) if e.name == type_name => {
                for block in &e.trait_impls {
                    if block.trait_name == trait_name {
                        return assoc_type_impl(&block.assoc_type_impls, assoc_name).cloned();
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn collect_iter_index_hooks(cx: &mut Cx, items: &[Item]) {
    let mut iterable_pairs: Vec<(String, String)> = Vec::new();
    for item in items {
        match item {
            Item::Impl(i) if i.trait_name.as_deref() == Some(Syntax::TRAIT_ITERABLE) => {
                if let Some(Type::Named(iter_name)) =
                    trait_impl_assoc(items, &i.type_name, Syntax::TRAIT_ITERABLE, "Iter")
                {
                    iterable_pairs.push((i.type_name.clone(), iter_name));
                }
            }
            Item::Struct(s) => {
                if trait_impl_assoc(items, &s.name, Syntax::TRAIT_ITERABLE, "Iter").is_some() {
                    if let Some(Type::Named(iter_name)) =
                        trait_impl_assoc(items, &s.name, Syntax::TRAIT_ITERABLE, "Iter")
                    {
                        iterable_pairs.push((s.name.clone(), iter_name));
                    }
                }
            }
            Item::Enum(e) => {
                if trait_impl_assoc(items, &e.name, Syntax::TRAIT_ITERABLE, "Iter").is_some() {
                    if let Some(Type::Named(iter_name)) =
                        trait_impl_assoc(items, &e.name, Syntax::TRAIT_ITERABLE, "Iter")
                    {
                        iterable_pairs.push((e.name.clone(), iter_name));
                    }
                }
            }
            _ => {}
        }
    }
    for (coll_type, iter_type) in iterable_pairs {
        if let Some(item_type) = trait_impl_assoc(items, &iter_type, Syntax::TRAIT_ITERATOR, "Item")
        {
            cx.iterable_hooks.insert(
                coll_type,
                IterableHook {
                    iter_type,
                    item_type,
                },
            );
        }
    }

    let mut index_types: HashSet<String> = HashSet::new();
    for item in items {
        match item {
            Item::Impl(i) if i.trait_name.as_deref() == Some(Syntax::TRAIT_INDEX) => {
                index_types.insert(i.type_name.clone());
            }
            Item::Struct(s) => {
                for block in &s.trait_impls {
                    if block.trait_name == Syntax::TRAIT_INDEX {
                        index_types.insert(s.name.clone());
                    }
                }
            }
            Item::Enum(e) => {
                for block in &e.trait_impls {
                    if block.trait_name == Syntax::TRAIT_INDEX {
                        index_types.insert(e.name.clone());
                    }
                }
            }
            _ => {}
        }
    }
    for type_name in index_types {
        let value_type = trait_impl_assoc(items, &type_name, Syntax::TRAIT_INDEX, "Value");
        if let (Some(_key_type), Some(value_type)) = (
            trait_impl_assoc(items, &type_name, Syntax::TRAIT_INDEX, "Key"),
            value_type,
        ) {
            cx.index_hooks
                .insert(type_name.clone(), IndexHook { value_type });
        }
    }
}

/// Parameter conventions for a method, excluding `self` — call-site args
/// align positionally with this list (the receiver is emitted separately).
fn method_sig_params(f: &Func) -> Vec<(AccessConvention, Type)> {
    f.params
        .iter()
        .filter(|p| p.name != Syntax::KW_SELF)
        .map(|p| (p.convention, p.ty.clone()))
        .collect()
}

pub(crate) fn type_is_cloneable_struct(s: &StructDef, types: &HashSet<String>) -> bool {
    // c148: pass the struct's declared type-param names so multi-char params are
    // treated as cloneable (they carry a `T: Clone` bound in the emitted impl).
    let param_names: HashSet<String> = s.type_params.iter().map(|p| p.name.clone()).collect();
    s.fields
        .iter()
        .all(|f| field_type_cloneable(&f.ty, types, &param_names))
}

pub(crate) fn type_is_cloneable_enum(e: &EnumDef, types: &HashSet<String>) -> bool {
    // c148: pass the enum's declared type-param names.
    let param_names: HashSet<String> = e.type_params.iter().map(|p| p.name.clone()).collect();
    e.variants.iter().all(|v| match &v.payload {
        VariantPayload::Unit => true,
        VariantPayload::Single(t, _) => field_type_cloneable(t, types, &param_names),
        VariantPayload::Named(fs) => fs
            .iter()
            .all(|f| field_type_cloneable(&f.ty, types, &param_names)),
    })
}

pub(crate) fn field_type_cloneable(
    ty: &Type,
    types: &HashSet<String>,
    param_names: &HashSet<String>,
) -> bool {
    match ty {
        Type::Int | Type::Bool | Type::Float | Type::String | Type::Char => true,
        Type::IntN { .. } | Type::Float32 => true,
        Type::List(inner) | Type::Shared(inner) | Type::Option(inner) => {
            field_type_cloneable(inner, types, param_names)
        }
        Type::Map { key, value, .. } => {
            field_type_cloneable(key, types, param_names)
                && field_type_cloneable(value, types, param_names)
        }
        Type::Result { ok, err } => {
            field_type_cloneable(ok, types, param_names)
                && field_type_cloneable(err, types, param_names)
        }
        // c148: recognize both single-char heuristic and declared multi-char params.
        Type::Named(n) if Generics::is_type_var_name(n) || param_names.contains(n.as_str()) => true,
        Type::Named(n) => types.contains(n),
        // `JetTask` implements no `Clone`: a handle owns one join slot.
        // D-PIN1=A: a pin is an exclusive window, so it is no more cloneable
        // than `ViewMut` — duplicating it would hand out a second no-move claim.
        Type::Apply { name, .. }
            if matches!(name.as_str(), "ViewMut" | "Task" | Syntax::TYPE_PIN)
                || name == Syntax::TYPE_SHARED_GUARD =>
        {
            false
        }
        Type::Apply { name, .. } if name == "View" => true,
        Type::Apply { args, .. } => args
            .iter()
            .all(|a| field_type_cloneable(a, types, param_names)),
        Type::Tuple(fields) => fields
            .iter()
            .all(|(_, t)| field_type_cloneable(t, types, param_names)),
        Type::TraitObject(_) | Type::Fn { .. } => false,
        Type::FixedList { elem, .. } => field_type_cloneable(elem, types, param_names),
        Type::Tagged { inner, .. } => field_type_cloneable(inner, types, param_names),
        Type::Union(members) => members
            .iter()
            .all(|m| field_type_cloneable(m, types, param_names)),
    }
}

pub(crate) fn type_is_comparable_struct(s: &StructDef, types: &HashSet<String>) -> bool {
    // c148: pass the struct's declared type-param names.
    let param_names: HashSet<String> = s.type_params.iter().map(|p| p.name.clone()).collect();
    s.fields
        .iter()
        .all(|f| field_type_comparable(&f.ty, types, &param_names))
}

pub(crate) fn type_is_comparable_enum(e: &EnumDef, types: &HashSet<String>) -> bool {
    // c148: pass the enum's declared type-param names.
    let param_names: HashSet<String> = e.type_params.iter().map(|p| p.name.clone()).collect();
    e.variants.iter().all(|v| match &v.payload {
        VariantPayload::Unit => true,
        VariantPayload::Single(t, _) => field_type_comparable(t, types, &param_names),
        VariantPayload::Named(fs) => fs
            .iter()
            .all(|f| field_type_comparable(&f.ty, types, &param_names)),
    })
}

pub(crate) fn field_type_comparable(
    ty: &Type,
    types: &HashSet<String>,
    param_names: &HashSet<String>,
) -> bool {
    match ty {
        Type::Int | Type::Bool | Type::Float | Type::String | Type::Char => true,
        Type::IntN { .. } | Type::Float32 => true,
        Type::Option(inner) => field_type_comparable(inner, types, param_names),
        Type::Result { ok, err } => {
            field_type_comparable(ok, types, param_names)
                && field_type_comparable(err, types, param_names)
        }
        Type::List(inner) => field_type_comparable(inner, types, param_names),
        // c148: recognize both single-char heuristic and declared multi-char params.
        Type::Named(n) if Generics::is_type_var_name(n) || param_names.contains(n.as_str()) => true,
        Type::Named(n) => types.contains(n),
        // D-TUPLE-DESTRUCT1: `Task<T>`/`Sender<T>`/`Receiver<T>` wrap an opaque
        // runtime handle (`JetTask`/`JetSender`/`JetReceiver`) — none implement
        // `PartialEq`, regardless of whether their element type `T` does. Only
        // surfaces once one of these lands as a tuple field (`tasks.channel<T>()`'s
        // `(Sender<T>, Receiver<T>)`); every other `Type::Apply` (Set/Bag/Deque/…)
        // is still checked structurally through its args below.
        Type::Apply { name, .. }
            if matches!(
                name.as_str(),
                "Task" | "Sender" | "Receiver" | Syntax::TYPE_SHARED_GUARD
            ) =>
        {
            false
        }
        // D-MEM1 S6: `Pool<T>` is a live arena handle (`JetPool`), never comparable
        // regardless of `T`. `Id<T>` is plain index+generation data — ALWAYS
        // comparable regardless of `T` (it never touches `T` at runtime), so it
        // must NOT fall through to the generic "comparable iff every arg is" arm
        // below (that would wrongly require `T: PartialEq`).
        Type::Apply { name, .. } if name == "Pool" => false,
        Type::Apply { name, .. } if name == "Id" => true,
        Type::Apply { args, .. } => args
            .iter()
            .all(|a| field_type_comparable(a, types, param_names)),
        Type::Tuple(fields) => fields
            .iter()
            .all(|(_, t)| field_type_comparable(t, types, param_names)),
        Type::TraitObject(_) | Type::Map { .. } | Type::Shared(_) | Type::Fn { .. } => false,
        Type::FixedList { elem, .. } => field_type_comparable(elem, types, param_names),
        Type::Tagged { inner, .. } => field_type_comparable(inner, types, param_names),
        Type::Union(members) => members
            .iter()
            .all(|m| field_type_comparable(m, types, param_names)),
    }
}

pub(crate) fn type_is_hashable_struct(s: &StructDef, types: &HashSet<String>) -> bool {
    let param_names: HashSet<String> = s.type_params.iter().map(|p| p.name.clone()).collect();
    s.fields
        .iter()
        .all(|f| field_type_hashable(&f.ty, types, &param_names))
}

pub(crate) fn type_is_hashable_enum(e: &EnumDef, types: &HashSet<String>) -> bool {
    let param_names: HashSet<String> = e.type_params.iter().map(|p| p.name.clone()).collect();
    e.variants.iter().all(|v| match &v.payload {
        VariantPayload::Unit => true,
        VariantPayload::Single(t, _) => field_type_hashable(t, types, &param_names),
        VariantPayload::Named(fs) => fs
            .iter()
            .all(|f| field_type_hashable(&f.ty, types, &param_names)),
    })
}

/// Same shape as `field_type_comparable`, minus `Float`/`Float32` — Rust's
/// `f64`/`f32` don't implement `Eq`/`Hash` (NaN breaks both laws).
pub(crate) fn field_type_hashable(
    ty: &Type,
    types: &HashSet<String>,
    param_names: &HashSet<String>,
) -> bool {
    match ty {
        Type::Float | Type::Float32 => false,
        Type::Int | Type::Bool | Type::String | Type::Char => true,
        Type::IntN { .. } => true,
        Type::Option(inner) => field_type_hashable(inner, types, param_names),
        Type::Result { ok, err } => {
            field_type_hashable(ok, types, param_names)
                && field_type_hashable(err, types, param_names)
        }
        Type::List(inner) => field_type_hashable(inner, types, param_names),
        Type::Named(n) if Generics::is_type_var_name(n) || param_names.contains(n.as_str()) => true,
        Type::Named(n) => types.contains(n),
        // D-TUPLE-DESTRUCT1: same opaque-handle exclusion as `field_type_comparable`.
        Type::Apply { name, .. }
            if matches!(
                name.as_str(),
                "Task" | "Sender" | "Receiver" | Syntax::TYPE_SHARED_GUARD
            ) =>
        {
            false
        }
        // D-MEM1 S6: same `Pool`/`Id` split as `field_type_comparable` above.
        Type::Apply { name, .. } if name == "Pool" => false,
        Type::Apply { name, .. } if name == "Id" => true,
        Type::Apply { args, .. } => args
            .iter()
            .all(|a| field_type_hashable(a, types, param_names)),
        Type::Tuple(fields) => fields
            .iter()
            .all(|(_, t)| field_type_hashable(t, types, param_names)),
        Type::TraitObject(_) | Type::Map { .. } | Type::Shared(_) | Type::Fn { .. } => false,
        Type::FixedList { elem, .. } => field_type_hashable(elem, types, param_names),
        Type::Tagged { inner, .. } => field_type_hashable(inner, types, param_names),
        Type::Union(members) => members
            .iter()
            .all(|m| field_type_hashable(m, types, param_names)),
    }
}

fn find_struct_box_edges(s: &StructDef, cx: &Cx) -> HashSet<(String, String)> {
    let mut boxed = HashSet::new();
    for f in &s.fields {
        walk_type_edge(
            &s.name,
            &f.name,
            &f.ty,
            &mut vec![s.name.clone()],
            cx,
            &mut boxed,
        );
    }
    boxed
}

fn find_enum_box_edges(e: &EnumDef, cx: &Cx) -> HashSet<(String, String)> {
    let mut boxed = HashSet::new();
    for v in &e.variants {
        match &v.payload {
            VariantPayload::Unit => {}
            VariantPayload::Single(t, _) => walk_type_edge(
                &e.name,
                &v.name,
                t,
                &mut vec![e.name.clone()],
                cx,
                &mut boxed,
            ),
            VariantPayload::Named(fs) => {
                for f in fs {
                    let key = format!("{}.{}", v.name, f.name);
                    walk_type_edge(
                        &e.name,
                        &key,
                        &f.ty,
                        &mut vec![e.name.clone()],
                        cx,
                        &mut boxed,
                    );
                }
            }
        }
    }
    boxed
}

fn walk_type_edge(
    owner: &str,
    edge: &str,
    ty: &Type,
    stack: &mut Vec<String>,
    cx: &Cx,
    boxed: &mut HashSet<(String, String)>,
) {
    match ty {
        Type::Named(n) if cx.type_names.contains(n) => {
            if stack.iter().any(|s| s == n) {
                boxed.insert((owner.to_string(), edge.to_string()));
                return;
            }
            stack.push(n.clone());
            if let Some(fields) = cx.struct_fields.get(n) {
                for (fname, fty) in fields {
                    walk_type_edge(n, fname, fty, stack, cx, boxed);
                }
            }
            if let Some(vars) = cx.enum_variants.get(n) {
                for (vname, payload) in vars {
                    match payload {
                        VariantPayload::Unit => {}
                        VariantPayload::Single(t, _) => {
                            walk_type_edge(n, vname, t, stack, cx, boxed);
                        }
                        VariantPayload::Named(fs) => {
                            for f in fs {
                                let key = format!("{}.{}", vname, f.name);
                                walk_type_edge(n, &key, &f.ty, stack, cx, boxed);
                            }
                        }
                    }
                }
            }
            stack.pop();
        }
        // D-SHARED-CYCLE1=C: `Shared<T>` is already a sized Arc handle. Do not
        // invent recursive `Box` edges through Shared — strong Shared cycles are
        // rejected in sema (E0221); expert cycles use `Shared.Weak<T>`.
        Type::Option(inner) | Type::List(inner) => {
            walk_type_edge(owner, edge, inner, stack, cx, boxed);
        }
        Type::Map { key, value, .. } => {
            walk_type_edge(owner, edge, key, stack, cx, boxed);
            walk_type_edge(owner, edge, value, stack, cx, boxed);
        }
        Type::Char => {}
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raylib_skeleton_types_lower_to_core_prelude_types() {
        assert_eq!(core_rust_type_name("RaylibWindow"), Some("RaylibWindow"));
        assert_eq!(core_rust_type_name("RaylibColor"), Some("RaylibColor"));
        assert_eq!(
            raylib_handle_rust_type("RaylibWindow"),
            Some("RaylibWindow")
        );
        assert_eq!(raylib_handle_rust_type("RaylibColor"), Some("RaylibColor"));
    }
}
