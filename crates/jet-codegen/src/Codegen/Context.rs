use super::*;
use crate::Diagnostics::Span;
use crate::Generics;
use crate::Syntax;
use crate::AST::FfiLink;
use crate::AST::{
    AccessConvention, EnumDef, Func, Item, Program, ProgramBundle, StructDef, Type, VariantPayload,
};
use std::collections::{HashMap, HashSet};
pub(crate) struct Cx {
    /// Top-level function name -> parameter conventions+types.
    pub(crate) sigs: HashMap<String, Vec<(AccessConvention, Type)>>,
    /// Top-level function name -> function value type (M8).
    pub(crate) fn_types: HashMap<String, Type>,
    /// `(TypeName, method)` -> parameter conventions+types (including `self`).
    pub(crate) method_sigs: HashMap<(String, String), Vec<(AccessConvention, Type)>>,
    /// c109 Phase 6 (TIR): `(TypeName, method)` -> resolved return type (or `None`
    /// for a unit-returning method). Used by TIR lowering to give a method-call
    /// expression its total result `Type` without re-inferring in codegen.
    pub(crate) method_rets: HashMap<(String, String), Option<Type>>,
    pub(crate) consts: HashMap<String, String>,
    pub(crate) type_names: HashSet<String>,
    /// D-DIST1 (c109 Phase 23): distinct-type name -> (base type, is_numeric). A
    /// distinct type renders to a `#[repr(transparent)]` newtype `user_<Name>(pub
    /// Base)`; the TIR reads the base type to give `.raw()` (`(recv).0`) its total
    /// result type, and `is_numeric` is informational (the arithmetic operator is
    /// chosen by `ast_operand_is_integer`, which returns `None` for a distinct).
    pub(crate) distinct_types: HashMap<String, (Type, bool)>,
    /// D-TYPEALIAS1: transparent generic alias name -> (params, target).
    pub(crate) type_aliases: HashMap<String, (Vec<crate::AST::TypeParam>, Type)>,
    pub(crate) trait_names: HashSet<String>,
    pub(crate) struct_fields: HashMap<String, Vec<(String, Type)>>,
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
    /// D-TAG1: types whose fields are all Eq+Hash-capable (comparable minus
    /// float fields) — gates `derive(Eq, Hash)` for `Bag<T>` keys.
    pub(crate) hashable: HashSet<String>,
    /// S55: explicit `derive Comparable;` → PartialOrd in Rust.
    pub(crate) partial_ord: HashSet<String>,
    pub(crate) patchable: HashSet<String>,
    /// D-FIELDPOL1: struct name -> computed field names. Sema already
    /// synthesized a `fn <field>(self) -> T` getter for each on `s.methods`
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
    /// c148: type-parameter names for the function currently being emitted. Set
    /// from `f.type_params` at the start of `emit_func` so `rust_type` and
    /// `rust_param_type` can recognize multi-char params without the single-letter
    /// heuristic. Cleared when emit returns.
    pub(crate) current_type_params: std::cell::RefCell<HashSet<String>>,
    /// c139 M4: spawn lambda bodies collected during TIR lowering (JIT order).
    pub(crate) jit_spawn_lambdas: std::cell::RefCell<Vec<crate::Codegen::TIR::TJitSpawnLambda>>,
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
    /// build is compiling for — an `impl` gated to a different `#Target(Os.*)`
    /// is skipped entirely (mirrors how `Codegen/Web.rs` filters by
    /// `WebBucket`). Defaults to the host OS; the real build pipeline
    /// (`emit_bundle_dbg`) overwrites it from the resolved `--target=<triple>`.
    pub(crate) active_os: crate::Syntax::OsTarget,
}

pub(crate) const MOD_USE: &str = "use super::{JetShow, JetDisplay, JetDebug, JetArith, jet_panic, jet_panic_rich, jet_trace_err, jet_index_vec, jet_unpack_vec, jet_slice_vec, jet_index_map, jet_map_insert, jet_list_remove, jet_char_len, jet_string_split, jet_string_lines, jet_string_after, jet_string_before, jet_string_slice, jet_list_map, jet_list_map_mut, jet_list_filter, jet_list_each, jet_list_each_ref, jet_list_each_mut, jet_list_find, jet_list_any, jet_list_all, jet_list_sort_by, jet_list_reduce, jet_map_each, jet_list_take, jet_list_skip, jet_list_step_by, jet_list_dedup, jet_list_chunks, jet_list_windows, jet_list_sum, jet_list_product, jet_list_flatten, jet_list_intersperse, jet_list_count_by, jet_list_take_while, jet_list_skip_while, jet_list_flat_map, jet_list_scan, jet_list_fold, jet_list_position, jet_list_min_by, jet_list_max_by, jet_list_group_by, jet_list_partition};\n\n";

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

// D-ENC-DYN1=A+: the dynamic encoding value `Data` (+ aliases `Json`/`Toml`/
// `Yaml`/`Csv`) is the user-facing face of `jet_std::DataTree`.
pub(crate) fn is_json_type_name(name: &str) -> bool {
    Syntax::is_data_type_name(name)
}

// D-DBDRIVER1: the `DbValue` dynamic tagged SQL value — same construction
// mechanism as `Data`/`Json`, mirrored via `jet_std::DbValue`.
pub(crate) fn is_db_value_type_name(name: &str) -> bool {
    Syntax::is_db_value_type_name(name)
}

pub(crate) fn core_rust_type_name(name: &str) -> Option<&'static str> {
    match name {
        n if is_json_type_name(n) => Some("DataTree"),
        n if n == Syntax::TYPE_JSON_ERROR || n == "JsonError" => Some("JsonError"),
        n if n == Syntax::TYPE_IO_ERROR || n == "IoError" => Some("IoError"),
        n if n == Syntax::TYPE_UTF8_ERROR || n == "Utf8Error" => Some("Utf8Error"),
        "ProcessResult" => Some("ProcessResult"),
        "ProcessSpec" => Some("ProcessSpec"),
        "ProcessChild" => Some("ProcessChild"),
        "Stopwatch" => Some("Stopwatch"),
        // D-DET1: deterministic injected capability handles.
        "Clock" => Some("Clock"),
        "Rng" => Some("Rng"),
        // D-SOLVER-LIB1=A: explicit finite solver state.
        "Solver" => Some("Solver"),
        // D-DET-CAPAPI: deterministic `Duration` value.
        "Duration" => Some("Duration"),
        "Instant" => Some("JetInstant"),
        "Date" | "LocalDate" => Some("JetDate"),
        "LocalTime" => Some("JetLocalTime"),
        "DateTime" => Some("JetDateTime"),
        "Period" => Some("JetPeriod"),
        "Zone" => Some("JetZone"),
        "ZonedDateTime" => Some("JetZonedDateTime"),
        "Url" => Some("JetUrl"),
        "Mime" => Some("JetMime"),
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
        "DataStatus" => Some("DataStatus"),
        "DataSummary" => Some("DataSummary"),
        // D-LOGTRACE1=A: structured logging values.
        "LogField" => Some("LogField"),
        "LogSpan" => Some("LogSpan"),
        // D-SERDE2: the format-agnostic value tree + typed-decode error live in jet_std.
        "DataTree" => Some("DataTree"),
        "DecodeError" => Some("DecodeError"),
        // D-MIGRATE3=A: decode-time migration transparency's plain status struct
        // (the generic `DecodeResult<T>` has its own `rust_type` arm below, since
        // this table only covers non-generic names).
        "MigrationStatus" => Some("MigrationStatus"),
        // D-DBDRIVER1: the tagged SQL parameter/column value + its error type.
        "DbValue" => Some("DbValue"),
        "DbError" => Some("DbError"),
        // D-RAYLIB1=A: display-gated graphics bridge types.
        "RaylibWindow" => Some("RaylibWindow"),
        "RaylibColor" => Some("RaylibColor"),
        // D-TYPEDTEXT1=D: `Sql`/`Html` — this table's `.is_some()` is only a
        // "known core value type" gate for the TIR subset check; the actual Rust
        // spelling for these two comes from the earlier explicit `rust_type` arms
        // (`(String, Vec<String>)` / `String`), not this placeholder.
        "Sql" => Some("Sql"),
        "Html" => Some("Html"),
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
        "LayoutHandle" => Some("jet_layout::Handle"),
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
        // D-COREIO1=A: standard stream handles.
        "Stdout" => Some("JetStdout"),
        "Stderr" => Some("JetStderr"),
        // D-PATHFS1: typed path handle.
        "Path" => Some("JetPath"),
        // D-DBDRIVER1: the SQLite connection handle wrapper.
        "DbConnection" => Some("JetDbConnection"),
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
        _ => None,
    }
}

pub(crate) fn game_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "GameScene" => Some("GameScene"),
        "GameAssets" => Some("GameAssets"),
        "GameInputMap" => Some("GameInputMap"),
        "GameBudgetsSlot" => Some("GameBudgetsSlot"),
        "GameBudgets" => Some("GameBudgets"),
        "GameBackend" => Some("GameBackend"),
        "GameReplay" => Some("GameReplay"),
        "GameImage" => Some("GameImage"),
        "GameSound" => Some("GameSound"),
        "GameFrame" => Some("GameFrame"),
        "GameInputSnapshot" => Some("GameInputSnapshot"),
        _ => None,
    }
}

/// E2-M10: networking opaque types map to top-level prelude structs.
pub(crate) fn net_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "TcpListener" => Some("JetTcpListener"),
        "TcpStream" => Some("JetTcpStream"),
        "IpAddr" => Some("JetIpAddr"),
        "SocketAddr" => Some("JetSocketAddr"),
        "UdpSocket" => Some("JetUdpSocket"),
        "UdpPacket" => Some("JetUdpPacket"),
        "DnsSrv" => Some("JetDnsSrv"),
        "UnixListener" => Some("JetUnixListener"),
        "UnixStream" => Some("JetUnixStream"),
        "TlsStream" => Some("JetTlsStream"),
        "HttpRequest" => Some("JetHttpRequest"),
        "HttpResponse" => Some("JetHttpResponse"),
        "HttpRouter" => Some("JetHttpRouter"),
        _ => None,
    }
}

// Re-export from Syntax so submodules (lower.rs, subset.rs) find them via `use super::*`.
pub(crate) use crate::Syntax::alloc_handle_rust_type;
pub(crate) use crate::Syntax::args_handle_rust_type;
pub(crate) use crate::Syntax::binary_text_handle_rust_type;
pub(crate) use crate::Syntax::reflect_handle_rust_type;

impl Cx {
    pub(crate) fn field_rust_type(&self, owner: &str, edge: &str, ty: &Type) -> String {
        let base = self.rust_type(ty);
        if self
            .boxed_edges
            .contains(&(owner.to_string(), edge.to_string()))
        {
            format!("Box<{}>", base)
        } else {
            base
        }
    }

    pub(crate) fn struct_field_rust(&self, s: &StructDef, edge: &str, ty: &Type) -> String {
        let base = match ty {
            Type::Named(n) if s.type_params.iter().any(|p| p.name == *n) => n.clone(),
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
            Type::Map { key, value } => Type::Map {
                key: Box::new(self.expand_type_aliases(key)),
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
            } => Type::Fn {
                params: params.iter().map(|p| self.expand_type_aliases(p)).collect(),
                ret: ret.as_ref().map(|r| Box::new(self.expand_type_aliases(r))),
                effect_bound: effect_bound.clone(),
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
            Type::FixedList { elem, len } => Type::FixedList {
                elem: Box::new(self.expand_type_aliases(elem)),
                len: *len,
            },
            other => other.clone(),
        }
    }

    pub(crate) fn rust_type(&self, ty: &Type) -> String {
        let ty = self.expand_type_aliases(ty);
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
            Type::Map { key, value } => format!(
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
            Type::Named(name) if name == "Unit" || name == "Void" => "()".to_string(),
            Type::Named(name) if name == "Error" => "String".to_string(),
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
                format!("{}jet_std::JetUrl", self.root_prefix)
            }
            Type::Named(name) if name == "Mime" => {
                format!("{}jet_std::JetMime", self.root_prefix)
            }
            // D-NETDEP1=A / D-HTTPLIB1=A: HTTP types → opaque Rust structs.
            Type::Named(name) if name == "HttpClientReq" => "JetHttpClientReq".to_string(),
            Type::Named(name) if name == "HttpClientResp" => "JetHttpClientResp".to_string(),
            Type::Named(name) if name == "HttpMux" => "JetHttpMux".to_string(),
            Type::Named(name) if name == "HttpSrvReq" => "JetHttpSrvReq".to_string(),
            Type::Named(name) if name == "HttpSrvResp" => "JetHttpSrvResp".to_string(),
            Type::Named(name) if name == "HttpServerTls" => "JetHttpServerTls".to_string(),
            // c97/D-STRPARSE1: the builtin parse error (`Int.parse`, `Float.parse`,
            // `String.to_int`) erases to a plain message — never user-constructed.
            // A user enum named `ParseError` (in `type_names`) keeps its own lowering.
            Type::Named(name) if name == "ParseError" && !self.type_names.contains(name) => {
                "String".to_string()
            }
            // D-TYPEDTEXT1=D: `Sql` is a checked (template, bound params) pair — the
            // params never re-enter the template text. `Html` is already the fully
            // escaped text, so it's just a `String` underneath.
            Type::Named(name) if name == "Sql" => "(String, Vec<String>)".to_string(),
            Type::Named(name) if name == "Html" => "String".to_string(),
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
            Type::Named(name) if self.trait_names.contains(name) => {
                format!("Box<dyn {}>", Generics::user_trait_rust(name))
            }
            Type::Named(name) if self.foreign_types.contains_key(name.as_str()) => {
                let rust_mod = &self.foreign_types[name.as_str()];
                format!("{}{}::user_{name}", self.root_prefix, rust_mod)
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
            // D-STREAMYIELD1: a generator's `Stream<T>` is a rendezvous-channel
            // receiver — `Receiver<T>` already implements `IntoIterator<Item = T>`,
            // which is exactly `loop x in stream { }`'s pull-one-block-until-ready
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
            Type::Apply { name, args } if name == Syntax::TYPE_HOOK && args.len() == 2 => {
                format!(
                    "{}jet_std::JetHook<{}, {}>",
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
            // D-HONESTNUM1=A: Measurement<T> → jet_std::JetMeasurement<T>.
            Type::Apply { name, args } if name == Syntax::TYPE_MEASUREMENT && !args.is_empty() => {
                format!(
                    "{}jet_std::JetMeasurement<{}>",
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
                format!("&[{}]", self.rust_type(&args[0]))
            }
            // D-TTLVAL1=A: Expiring<T> / Rotting<T>.
            Type::Apply { name, args } if name == "Expiring" && args.len() == 1 => {
                format!("JetExpiring<{}>", self.rust_type(&args[0]))
            }
            Type::Apply { name, args } if name == "Rotting" && args.len() == 1 => {
                format!("JetRotting<{}>", self.rust_type(&args[0]))
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
                    "JetLru<{}, {}>",
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
            // D-OPTGC1: `Gc<T>` lowers to the traced handle in the vetted prelude.
            Type::Apply { name, args } if name == Syntax::GC_TYPE && args.len() == 1 => {
                format!("jet_gc::Gc<{}>", self.rust_type(&args[0]))
            }
            Type::Apply { name, args } => {
                if args.is_empty() {
                    user_type_rust(name)
                } else {
                    format!(
                        "{}<{args}>",
                        user_type_rust(name),
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
            Type::Fn { params, ret, .. } => self.rust_fn_trait(params, ret.as_deref(), false),
            Type::Tuple(fields) => tuple_struct_name(&tuple_fields_plain(fields)),
            // D-FIXARR1 (ratified 2026-06-22): [T#N] lowers to a real Rust stack array [T; N].
            // All size/bounds checks live in sema (I3). The Rust type is [E; N].
            Type::FixedList { elem, len } => format!("[{}; {}]", self.rust_type(elem), len),
            // D-QUAL4=A: tagged types are transparent to codegen.
            Type::Tagged { inner, .. } => self.rust_type(inner),
        }
    }

    pub(crate) fn rust_fn_trait(
        &self,
        params: &[Type],
        ret: Option<&Type>,
        mut_capture: bool,
    ) -> String {
        let ps = params
            .iter()
            .map(|p| self.rust_type(p))
            .collect::<Vec<_>>()
            .join(", ");
        let r = ret
            .map(|t| self.rust_type(t))
            .unwrap_or_else(|| "()".to_string());
        let trait_name = if mut_capture { "FnMut" } else { "Fn" };
        format!("Box<dyn {}({}) -> {}>", trait_name, ps, r)
    }

    pub(crate) fn mangle_name(&self, name: &str) -> String {
        mangle(name)
    }

    pub(crate) fn type_prefix(&self, type_name: &str) -> String {
        user_type_rust(type_name)
    }
}

pub(crate) fn rust_param_type(cx: &Cx, convention: AccessConvention, ty: &Type) -> String {
    let base = cx.rust_type(ty);
    if matches!(ty, Type::Named(n) if cx.trait_names.contains(n))
        || matches!(ty, Type::TraitObject(_))
    {
        return match convention {
            // D-CAP9: `Share`/`Raw` aren't produced yet (specialized when their
            // phases land); both follow `Read`.
            AccessConvention::Read | AccessConvention::Share | AccessConvention::Raw => {
                format!("&{base}")
            }
            AccessConvention::Write => format!("&mut {base}"),
            AccessConvention::Move => base,
        };
    }
    // c148: type-var params are by-value — single-char heuristic + current_type_params.
    if matches!(ty, Type::Named(n) if Generics::is_type_var_name(n)
        || cx.current_type_params.borrow().contains(n.as_str()))
    {
        return base;
    }
    if matches!(ty, Type::Fn { .. }) {
        return base;
    }
    match convention {
        // D-CAP9: Share/Raw follow Read until their phases specialize them.
        AccessConvention::Read | AccessConvention::Share | AccessConvention::Raw
            if ty.is_scalar() =>
        {
            base
        }
        AccessConvention::Read | AccessConvention::Share | AccessConvention::Raw => {
            format!("&{}", base)
        }
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
    let mut map = HashMap::new();
    for item in items {
        if let Item::ExternRust(block) = item {
            for ef in &block.functions {
                map.insert(ef.name.clone(), format!("jet_ffi_{}", ef.name));
            }
        }
    }
    map
}

pub(crate) fn bundle_extern_funcs(bundle: &ProgramBundle) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for module in &bundle.modules {
        map.extend(extern_func_map(&module.items));
    }
    map
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
    cx.used_core = bundle.used_core.clone();
    let (uinline, ufile) = unqualified_import_maps(bundle, module_idx);
    cx.unqualified_inline = uinline;
    cx.unqualified_file = ufile;
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
        method_rets: HashMap::new(),
        consts: HashMap::new(),
        type_names: HashSet::new(),
        distinct_types: HashMap::new(),
        type_aliases: HashMap::new(),
        trait_names: HashSet::new(),
        struct_fields: HashMap::new(),
        enum_variants: HashMap::new(),
        variant_owner: HashMap::new(),
        boxed_edges: HashSet::new(),
        cloneable: HashSet::new(),
        migrations: HashMap::new(),
        columnar: HashSet::new(),
        comparable: HashSet::new(),
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
        root_prefix: String::new(),
        ffi_crate: link.map(|l| l.crate_name.clone()),
        extern_funcs: extern_funcs.clone(),
        code_modules: HashSet::new(),
        unqualified_inline: HashMap::new(),
        unqualified_file: HashMap::new(),
        trait_methods: HashSet::new(),
        rollback_types: HashSet::new(),
        display_types: HashSet::new(),
        iterable_hooks: HashMap::new(),
        index_hooks: HashMap::new(),
        current_fn: std::cell::RefCell::new(String::new()),
        struct_type_params: HashMap::new(),
        current_type_params: std::cell::RefCell::new(HashSet::new()),
        jit_spawn_lambdas: std::cell::RefCell::new(Vec::new()),
        variadic_bound_fns: HashMap::new(),
        needed_variadic_arities: std::cell::RefCell::new(std::collections::BTreeMap::new()),
        active_os: crate::Syntax::OsTarget::host(),
    };

    for item in items {
        match item {
            Item::Func(f) => {
                let type_params: HashSet<String> =
                    f.type_params.iter().map(|p| p.name.clone()).collect();
                cx.sigs.insert(
                    f.name.clone(),
                    f.params
                        .iter()
                        .map(|p| {
                            let conv = if matches!(&p.ty, Type::Named(n) if type_params.contains(n))
                            {
                                AccessConvention::Move
                            } else {
                                p.convention
                            };
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
                if c.is_comptime {
                    // Inline the evaluated literal at every reference.
                    let serialized =
                        c.ct.as_ref()
                            .map(|v| v.serialize())
                            .unwrap_or_else(|| "Default::default()".to_string());
                    cx.consts.insert(c.name.clone(), serialized);
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
                }
            }
            Item::Trait(t) => {
                cx.trait_names.insert(t.name.clone());
            }
            // D-QUAL2: a tag erases — it contributes no codegen names.
            Item::Tag(_) => {}
            // D-MIGRATE4: collect migration blocks per type (source order = the
            // chain, oldest step first) so `emit_struct_migration` can emit the
            // runtime step functions + chain-walker for decodable
            // `@PublishedSchema` types. Types without blocks get nothing.
            Item::Migration(m) => {
                cx.migrations
                    .entry(m.type_name.clone())
                    .or_default()
                    .push(m.clone());
            }
            Item::Impl(_) | Item::Test(_) | Item::Bench(_) | Item::Module(_) | Item::ErrorConv(_)
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
            }
            // D-QUAL3: each unit-family member registers as a `@Numeric` distinct
            // type erasing to `Float`.
            Item::UnitFamily(uf) => {
                for d in uf.distinct_defs() {
                    cx.type_names.insert(d.name.clone());
                    cx.distinct_types
                        .insert(d.name.clone(), (d.base.clone(), d.is_numeric));
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
                                    effect_bound: None,
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
                for (t, _) in &s.derives {
                    if t == Generics::COMPARABLE {
                        cx.partial_ord.insert(s.name.clone());
                        cx.comparable.insert(s.name.clone());
                    }
                }
                for m in &s.methods {
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
                for m in &e.methods {
                    cx.method_sigs
                        .insert((e.name.clone(), m.name.clone()), method_sig_params(m));
                    cx.method_rets
                        .insert((e.name.clone(), m.name.clone()), m.return_type.clone());
                }
            }
            Item::Impl(i) => {
                for m in &i.methods {
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
            Item::Struct(s) => {
                for block in &s.trait_impls {
                    if block.trait_name == Syntax::TRAIT_ROLLBACK {
                        cx.rollback_types.insert(s.name.clone());
                    }
                    if block.trait_name == Syntax::TRAIT_DISPLAY {
                        cx.display_types.insert(s.name.clone());
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
                }
            }
            _ => {}
        }
    }

    collect_iter_index_hooks(&mut cx, items);

    cx
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

fn field_type_cloneable(ty: &Type, types: &HashSet<String>, param_names: &HashSet<String>) -> bool {
    match ty {
        Type::Int | Type::Bool | Type::Float | Type::String | Type::Char => true,
        Type::IntN { .. } | Type::Float32 => true,
        Type::List(inner) | Type::Shared(inner) | Type::Option(inner) => {
            field_type_cloneable(inner, types, param_names)
        }
        Type::Map { key, value } => {
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
        Type::Apply { args, .. } => args
            .iter()
            .all(|a| field_type_cloneable(a, types, param_names)),
        Type::Tuple(fields) => fields
            .iter()
            .all(|(_, t)| field_type_cloneable(t, types, param_names)),
        Type::TraitObject(_) | Type::Fn { .. } => false,
        Type::FixedList { elem, .. } => field_type_cloneable(elem, types, param_names),
        Type::Tagged { inner, .. } => field_type_cloneable(inner, types, param_names),
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
        Type::Apply { name, .. } if matches!(name.as_str(), "Task" | "Sender" | "Receiver") => {
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
        Type::Apply { name, .. } if matches!(name.as_str(), "Task" | "Sender" | "Receiver") => {
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
        Type::Option(inner) | Type::List(inner) | Type::Shared(inner) => {
            walk_type_edge(owner, edge, inner, stack, cx, boxed);
        }
        Type::Map { key, value } => {
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
