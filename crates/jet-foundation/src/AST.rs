//! AST nodes. Grows with each milestone; keep nodes small and keep spans on
//! anything an error might need to point at.

use crate::Diagnostics::Span;
use crate::Syntax;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The access capability of a parameter / argument / receiver (D-CAP7/8/9/10).
///
/// Surface sigils map here: `T`→`Infer`, `~T`→`Write`, `^T`→`Move`, `&T`→`Share`,
/// `*T`→`Raw`. Inference (D-CAP8=C) resolves `Infer` to one of the concrete
/// capabilities from body usage before codegen; the parser still seeds unmarked
/// params as `Read` until the constraint solver lands (the seam is
/// `parse_access_prefix`). `Raw` is only produced by the `*` sigil inside
/// `#Unsafe` and never inferred in safe code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessConvention {
    /// Unmarked `T`: capability inferred from body usage (D-CAP8). Resolves to
    /// one of the concrete variants below; treated as `Read` until resolved.
    Infer,
    /// Shared read borrow (`&T` in Rust; scalars pass by value).
    Read,
    /// `~T`: exclusive write/edit access (mutable borrow, `&mut T`).
    Write,
    /// `^T`: ownership transfer / move (`T` by value).
    Move,
    /// `&T`: the value may escape / be retained beyond the call (share). Composes
    /// with regions/arenas (D-CAP10/c130); a safe escaping handle, not a raw pointer.
    Share,
    /// `*T`: raw unsafe pointer/address (`#Unsafe`-only, never inferred; D-CAP9).
    Raw,
}

impl AccessConvention {
    /// The D-CAP7 prefix sigil for this resolved capability, as it appears on a
    /// public type (`~T`/`^T`/`&T`/`*T`). Read — the unmarked default — and the
    /// not-yet-resolved `Infer` emit no sigil. Used by the published-API surface
    /// (c129) so the frozen signature carries the sigil the caller must honour.
    pub fn sigil(self) -> &'static str {
        match self {
            AccessConvention::Read | AccessConvention::Infer => "",
            AccessConvention::Write => "~",
            AccessConvention::Move => "^",
            AccessConvention::Share => "&",
            AccessConvention::Raw => "*",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Type {
    Int,
    Float,
    Bool,
    String,
    /// S41 (M5): Unicode scalar value.
    Char,
    List(Box<Type>),
    /// S38 (M5): keyed collection `Map<K, V>`.
    Map {
        key: Box<Type>,
        value: Box<Type>,
    },
    Shared(Box<Type>),
    /// S32: `T?` optional value.
    Option(Box<Type>),
    /// S34: `T ? E` fallible return. Internally lowered through Rust `Result<T, E>`.
    Result {
        ok: Box<Type>,
        err: Box<Type>,
    },
    /// S47 (M8): function type `fn(T1, T2) -> R` (`ret` omitted = no return value).
    ///
    /// D-EFF2 (callback param effect bound): an optional effect annotation may
    /// ride the *front* of a function type — `@Pure fn(T) -> U` (the callback
    /// must be pure) or `#(Net) fn(T) -> U` (the callback may use at most the
    /// listed effects). `effect_bound` is `None` when unannotated, `Some(empty)`
    /// for `@Pure`, and `Some([(name, span), …])` for `#(…)`. Names are validated
    /// against the effect vocabulary in sema, not the parser. The bound is a
    /// call-site obligation on whatever callback is passed (E0747) — it is **not**
    /// part of structural type identity (see the manual `PartialEq for Type`,
    /// which ignores it in the `Fn` arm), so `@Pure fn(Int)` and `fn(Int)` are the
    /// same type for assignability; the bound is an *extra* check, not a subtype.
    Fn {
        params: Vec<Type>,
        ret: Option<Box<Type>>,
        effect_bound: Option<Vec<(String, Span)>>,
    },
    /// User-defined monomorphic type name.
    Named(String),
    /// S45 (M9): generic application — `Pair<Int>`, `Stack<T>`.
    Apply {
        name: String,
        args: Vec<Type>,
    },
    /// S48 (M9): trait object — dynamic dispatch with invisible boxing.
    /// D-ANY-JAI1/D-VARARGBOUND1: a trait-bounded variadic loop element
    /// (`...[A, B]`) types its body binding as a multi-name `TraitObject` so
    /// method dispatch and interpolation can check EVERY bound trait, not
    /// just the first — codegen never constructs (or sees) more than one
    /// name here; it always synthesizes a real generic type param with all
    /// bounds instead (`Codegen/VariadicBound.rs`), so every codegen-side
    /// `TraitObject` match arm still only ever handles a singleton list.
    TraitObject(Vec<String>),
    /// S73 (D-SG7): named tuple `(x: Int, y: Int)` — fields stored sorted by name.
    Tuple(Vec<(String, Box<Type>)>),
    /// S76 (2026-06-16): fixed-size list `[T#N]` — a compile-time refinement of
    /// `[T]` with a statically-known length. Erases to `Vec<T>` at codegen (I3).
    FixedList {
        elem: Box<Type>,
        len: u64,
    },
    /// D-SG9/S42: explicit fixed-width integer. The default 64-bit *signed*
    /// integer is spelled `Int` (and equivalently `I64`) and lives in
    /// `Type::Int`, so it never appears here — `I64` canonicalises to
    /// `Type::Int` at parse time. Every other width is an `IntN`: `bits` ∈
    /// {8,16,32,64}, and `(signed: true, bits: 64)` is excluded by construction
    /// because that *is* `Int`. So `U8` = `{signed:false, bits:8}`,
    /// `U64` = `{signed:false, bits:64}`, `I32` = `{signed:true, bits:32}`.
    IntN {
        signed: bool,
        bits: u8,
    },
    /// D-SG9/S42: 32-bit float. The default 64-bit float is spelled `Float`
    /// (and `F64`) and lives in `Type::Float`; only `F32` is a `Float32`.
    Float32,
    /// D-QUAL4=A: value-tag type qualifier — `#Marker T` in signature/binding
    /// position. Transparent to type identity (the tag is a flow annotation only,
    /// not a structural difference); sema treats it as `inner` for all purposes.
    Tagged {
        marker: String,
        inner: Box<Type>,
    },
}

/// Manual structural equality (D-EFF2). Identical to a derived `PartialEq`
/// except the `Fn` arm ignores `effect_bound`: a callback effect bound is a
/// call-site obligation, not part of a function type's identity, so a
/// `@Pure fn(Int)` value is assignable wherever a `fn(Int)` is expected. The
/// bound is enforced separately at the call site (E0747).
impl PartialEq for Type {
    fn eq(&self, other: &Self) -> bool {
        use Type::*;
        match (self, other) {
            (Int, Int)
            | (Float, Float)
            | (Bool, Bool)
            | (String, String)
            | (Char, Char)
            | (Float32, Float32) => true,
            (List(a), List(b)) => a == b,
            (Map { key: k1, value: v1 }, Map { key: k2, value: v2 }) => k1 == k2 && v1 == v2,
            (Shared(a), Shared(b)) => a == b,
            (Option(a), Option(b)) => a == b,
            (Result { ok: o1, err: e1 }, Result { ok: o2, err: e2 }) => o1 == o2 && e1 == e2,
            // D-EFF2: effect_bound deliberately excluded from the comparison.
            (
                Fn {
                    params: p1,
                    ret: r1,
                    ..
                },
                Fn {
                    params: p2,
                    ret: r2,
                    ..
                },
            ) => p1 == p2 && r1 == r2,
            (Named(a), Named(b)) => a == b,
            (Apply { name: n1, args: a1 }, Apply { name: n2, args: a2 }) => n1 == n2 && a1 == a2,
            (TraitObject(a), TraitObject(b)) => a == b,
            (Tuple(a), Tuple(b)) => a == b,
            (FixedList { elem: e1, len: l1 }, FixedList { elem: e2, len: l2 }) => {
                e1 == e2 && l1 == l2
            }
            (
                IntN {
                    signed: s1,
                    bits: b1,
                },
                IntN {
                    signed: s2,
                    bits: b2,
                },
            ) => s1 == s2 && b1 == b2,
            // D-QUAL4: tagged types are transparent — identity is on the inner type.
            (Tagged { inner: a, .. }, Tagged { inner: b, .. }) => a == b,
            (Tagged { inner, .. }, other) | (other, Tagged { inner, .. }) => {
                inner.as_ref() == other
            }
            _ => false,
        }
    }
}

impl Eq for Type {}

/// D-SG9: the spelling of a fixed-width integer (`U8`, `I32`, …).
pub fn int_spelling(signed: bool, bits: u8) -> String {
    format!("{}{}", if signed { 'I' } else { 'U' }, bits)
}

/// D-SG9: parse a numeric type spelling to its `Type` — `Int`/`Float` and the
/// fixed widths, with `I64`/`F64` folding to the 64-bit defaults. `None` for any
/// non-numeric name. Inverse of `Type::name` for the numeric types.
pub fn numeric_type_from_name(name: &str) -> Option<Type> {
    match name {
        "Int" | "I64" => Some(Type::Int),
        "Float" | "F64" => Some(Type::Float),
        "F32" => Some(Type::Float32),
        _ => {
            let signed = name.starts_with('I');
            if !(signed || name.starts_with('U')) || name.len() < 2 {
                return None;
            }
            let bits: u8 = name[1..].parse().ok()?;
            match bits {
                8 | 16 | 32 => Some(Type::IntN { signed, bits }),
                64 if !signed => Some(Type::IntN {
                    signed: false,
                    bits: 64,
                }),
                _ => None,
            }
        }
    }
}

/// D-SG9: inclusive `(min, max)` value range of a fixed-width integer, used for
/// literal-fits checks. `i128` holds every Jet integer width exactly.
pub fn int_range(signed: bool, bits: u8) -> (i128, i128) {
    if signed {
        let max = (1i128 << (bits - 1)) - 1;
        (-(max + 1), max)
    } else {
        ((0i128), (1i128 << bits) - 1)
    }
}

/// S73: sort tuple fields by name so type identity ignores source order.
pub fn canonicalize_tuple_fields<T>(mut fields: Vec<(String, T)>) -> Vec<(String, T)> {
    fields.sort_by(|a, b| a.0.cmp(&b.0));
    fields
}

impl Type {
    /// Plain-words name for diagnostics (docs/spec/diagnostics.md voice: name both types).
    pub fn show(&self) -> String {
        match self {
            Type::Int => "Int (a whole number)".to_string(),
            Type::Float => "Float (a decimal number)".to_string(),
            Type::Bool => "Bool (true or false)".to_string(),
            Type::String => "String (text)".to_string(),
            Type::Char => "Char (one character)".to_string(),
            Type::List(inner) => format!("[{}]", inner.name()),
            Type::Map { key, value } => format!("[{}, {}]", key.name(), value.name()),
            Type::Shared(inner) => format!("Shared<{}>", inner.name()),
            Type::Option(inner) => format!("{}?", inner.name()),
            Type::Result { ok, err } => format!("{} ? {}", ok.name(), err.name()),
            Type::Fn { params, ret, .. } => {
                let ps = params
                    .iter()
                    .map(|p| p.name())
                    .collect::<Vec<_>>()
                    .join(", ");
                match ret {
                    Some(r) => format!("fn({}) -> {}", ps, r.name()),
                    None => format!("fn({})", ps),
                }
            }
            Type::Named(n) => format!("`{}`", n),
            // D-CAP9: the raw-pointer type shows as the canonical `*T`.
            Type::Apply { name, args } if name == crate::Syntax::TYPE_PTR && args.len() == 1 => {
                format!("`*{}`", args[0].name())
            }
            Type::Apply { name, args } => {
                let a = args.iter().map(|x| x.name()).collect::<Vec<_>>().join(", ");
                format!("`{}`<{}>", name, a)
            }
            Type::TraitObject(t) => format!("`{}` (a trait value)", t.join(" + ")),
            Type::Tuple(fields) => {
                let parts = fields
                    .iter()
                    .map(|(n, t)| format!("{}: {}", n, t.name()))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({parts})")
            }
            Type::FixedList { elem, len } => format!("[{}#{}]", elem.name(), len),
            Type::IntN { signed, bits } => {
                let (lo, hi) = int_range(*signed, *bits);
                let article = if *bits == 8 { "an" } else { "a" };
                format!(
                    "{} ({} {}-bit whole number, {} to {})",
                    int_spelling(*signed, *bits),
                    article,
                    bits,
                    lo,
                    hi
                )
            }
            Type::Float32 => "F32 (a 32-bit decimal number)".to_string(),
            Type::Tagged { marker, inner } => format!("#{} {}", marker, inner.show()),
        }
    }

    /// Bare type name, no gloss.
    pub fn name(&self) -> String {
        match self {
            Type::Int => "Int".to_string(),
            Type::Float => "Float".to_string(),
            Type::Bool => "Bool".to_string(),
            Type::String => "String".to_string(),
            Type::Char => "Char".to_string(),
            Type::List(inner) => format!("[{}]", inner.name()),
            Type::Map { key, value } => format!("[{}, {}]", key.name(), value.name()),
            Type::Shared(inner) => format!("Shared<{}>", inner.name()),
            Type::Option(inner) => format!("{}?", inner.name()),
            Type::Result { ok, err } => format!("{} ? {}", ok.name(), err.name()),
            Type::Fn { params, ret, .. } => {
                let ps = params
                    .iter()
                    .map(|p| p.name())
                    .collect::<Vec<_>>()
                    .join(", ");
                match ret {
                    Some(r) => format!("fn({}) -> {}", ps, r.name()),
                    None => format!("fn({})", ps),
                }
            }
            Type::Named(n) => n.clone(),
            // D-CAP9: the raw-pointer type names as the canonical `*T`.
            Type::Apply { name, args } if name == crate::Syntax::TYPE_PTR && args.len() == 1 => {
                format!("*{}", args[0].name())
            }
            Type::Apply { name, args } => {
                let a = args.iter().map(|x| x.name()).collect::<Vec<_>>().join(", ");
                format!("{}<{}>", name, a)
            }
            Type::TraitObject(t) => t.join(" + "),
            Type::Tuple(fields) => {
                let parts = fields
                    .iter()
                    .map(|(n, t)| format!("{}: {}", n, t.name()))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({parts})")
            }
            Type::FixedList { elem, len } => format!("[{}#{}]", elem.name(), len),
            Type::IntN { signed, bits } => int_spelling(*signed, *bits),
            Type::Float32 => "F32".to_string(),
            Type::Tagged { marker, inner } => format!("#{} {}", marker, inner.name()),
        }
    }

    /// Base name for struct/enum/trait references (without generic args).
    pub fn base_name(&self) -> Option<&str> {
        match self {
            Type::Named(n) => Some(n.as_str()),
            Type::Apply { name, .. } => Some(name.as_str()),
            Type::TraitObject(t) => t.first().map(String::as_str),
            _ => None,
        }
    }

    pub fn is_scalar(&self) -> bool {
        match self {
            Type::Tagged { inner, .. } => inner.is_scalar(),
            _ => matches!(
                self,
                Type::Int | Type::Float | Type::Bool | Type::IntN { .. } | Type::Float32
            ),
        }
    }

    /// D-SG9: any integer type — the default `Int` or an explicit fixed width.
    pub fn is_integer(&self) -> bool {
        match self {
            Type::Tagged { inner, .. } => inner.is_integer(),
            _ => matches!(self, Type::Int | Type::IntN { .. }),
        }
    }

    /// D-SG9/D-FLOATW1: any float type — the default `Float` or `F32`.
    pub fn is_float(&self) -> bool {
        match self {
            Type::Tagged { inner, .. } => inner.is_float(),
            _ => matches!(self, Type::Float | Type::Float32),
        }
    }

    /// D-SG9: any numeric type (integer or float).
    pub fn is_numeric(&self) -> bool {
        self.is_integer() || self.is_float()
    }

    pub fn unwrap_option(&self) -> Option<&Type> {
        match self {
            Type::Option(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn unwrap_result(&self) -> Option<(&Type, &Type)> {
        match self {
            Type::Result { ok, err } => Some((ok, err)),
            _ => None,
        }
    }

    pub fn is_fallible(&self) -> bool {
        matches!(self, Type::Option(_) | Type::Result { .. })
    }
}

#[derive(Debug)]
pub struct Program {
    /// S16 (M6): `import` declarations at the top of this file.
    pub imports: Vec<ImportDecl>,
    pub items: Vec<Item>,
    /// D-WASM1 (c123): optional file-level web bucket ceiling (`js target;` / `wasm target;`).
    pub web_target_ceiling: Option<crate::WebPartition::WebBucket>,
    /// D-VISDEFAULT1=C / D-VISDEFAULT2=A: `#PubFile` flips default top-level export visibility.
    pub pub_file: bool,
    /// D-WEBDEFAULT1 (open, c134): `#Target(Web)` — this file's default CLI
    /// backend is the web target, so `jet run`/`jet dev`/`jet build` don't
    /// need `--target=web` on every invocation. `None` means the native
    /// default applies unless `pkg.jet` or an explicit `--target=` flag says
    /// otherwise. Distinct from `web_target_ceiling` (`Wasm`/`Js`, a partition
    /// ceiling *within* a web build) — `Web` here means "build for the web
    /// backend at all," a different axis, same marker family (I8).
    pub default_target: Option<String>,
    /// D-HTMLPAIR1 (open, c134): `#Html("path.html")` — this program's
    /// companion host page for `--target=web` builds, explicit instead of
    /// the silent `<stem>.html` sibling-filename convention. Relative to the
    /// `.jet` source file's own directory.
    pub html_path: Option<String>,
}

/// S16: `import "path" [as alias];` or `import name [as alias];`
#[derive(Debug, Clone)]
pub struct ImportDecl {
    pub kind: ImportKind,
    pub alias: String,
    pub alias_span: Span,
    pub span: Span,
    /// D-MOD3/4: true for `pub use alias.Item` re-exports.
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) use …` — package-scoped re-export.
    pub is_package_pub: bool,
}

impl ImportDecl {
    /// The effective alias for this import: the user-given alias if present,
    /// otherwise the default derived from the import kind.
    pub fn import_alias(&self) -> String {
        if self.alias.is_empty() {
            match &self.kind {
                ImportKind::File(path, _) => {
                    path.rsplit('/').next().unwrap_or("module").to_string()
                }
                ImportKind::Module(name, _) => name.clone(),
                ImportKind::Unqualified { module_alias, .. } => module_alias.clone(),
            }
        } else {
            self.alias.clone()
        }
    }

    /// If this import refers to a compiler-known core/ring module, return its
    /// canonical path (e.g. `"core.fs"`, `"jet.http"`). Returns `None` for
    /// file/unqualified imports and unknown module names.
    pub fn core_module_path(&self) -> Option<String> {
        let ImportKind::Module(name, _) = &self.kind else {
            return None;
        };
        Syntax::normalize_core_module(name)
    }

    /// True when this import is any C `use` form (`use c.<lib>` or `use "<…>.h"`).
    pub fn is_c_import(&self) -> bool {
        match &self.kind {
            ImportKind::Module(name, _) => {
                let mut segs = name.split('.');
                if segs.next() == Some(Syntax::C_MODULE_ROOT) {
                    if let Some(lib) = segs.next() {
                        return !lib.is_empty() && segs.next().is_none();
                    }
                }
                false
            }
            ImportKind::File(path, _) => path.ends_with(".h"),
            ImportKind::Unqualified { .. } => false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ImportKind {
    /// Quoted path relative to this file's directory (no `.jet` suffix).
    File(String, Span),
    /// Bare module name — searched from the project root.
    Module(String, Span),
    /// D-MOD3/4: `use alias.Item` / `use alias.{A, B as C}` / `pub use alias.Item`
    /// D-SELIMPORT1=A: each item may carry an `as alias` — `(original, alias_if_any)`.
    Unqualified {
        module_alias: String,
        module_alias_span: Span,
        /// `(original_name, local_alias)` — alias is the local binding name.
        items: Vec<(String, Option<String>)>,
        items_span: Span,
        span: Span,
    },
}

#[derive(Debug)]
pub struct ProgramBundle {
    /// Index into `modules` for the entry file.
    pub entry: usize,
    /// Directory containing the entry file (project root until M12 `pkg.jet`).
    pub project_root: std::path::PathBuf,
    pub modules: Vec<LoadedModule>,
    /// S14 teaching diagnostics collected during a lenient parse (LSP check).
    pub parse_teaching: Vec<crate::Diagnostics::Diagnostic>,
    /// M10: Core helper names proven reachable by sema. Codegen emits only
    /// these helpers (SL9).
    pub used_core: std::collections::HashSet<String>,
    /// S59 (E2-M14): C-FFI artifacts produced by `CFFI::assemble` after loading
    /// — per-file `use c.<lib>` bindings and the libraries to link against.
    pub cffi: CFfi,
    /// D-CTEFFECT1 Tier-1: embed_file/embed_bytes inputs accumulated by sema.
    /// Each entry records the path and sha256 of a file embedded at compile
    /// time. Written to `.jet/lock` by the build driver for reproducibility.
    pub comptime_inputs: Vec<ComptimeInput>,
    /// Pre-resolved import target indices: `(from_module_idx, import_span) → to_module_idx`.
    /// Populated by `Loader::load_entry_with_overlay` after all modules are loaded.
    /// Core-module imports and C imports are absent (they have no loaded module index).
    /// Empty for single-module bundles created inline (compile_src / check_eval paths).
    pub import_targets: std::collections::HashMap<(usize, Span), usize>,
    /// D-RINGLAYER1: optional `layer:` ceiling from `pkg.jet`.
    pub layer_ceiling: Option<crate::RingLayer::RuntimeLayer>,
    /// D-RINGLAYER1: inferred minimum runtime layer for this package.
    pub inferred_layer: crate::RingLayer::RuntimeLayer,
    /// D-WASM1: resolved web bucket per mangled function key (filled by sema).
    pub web_partitions: std::collections::HashMap<String, crate::WebPartition::WebBucket>,
    /// True when compiling for web (partition checks enforced).
    pub web_partition_enforced: bool,
    /// D-WASM1: human-readable partition report (`--explain-partition`).
    pub web_partition_report: Option<String>,
    /// D-EFFBUDGET1: dependency name → its resolved on-disk source root, for
    /// both `deps:` (path/git/provider) entries and hangar-realized `use <pkg>`
    /// libraries. Lets a downstream pass (the effect budget) attribute a
    /// loaded module's path back to the dependency that owns it. Empty for a
    /// single-file program with no `pkg.jet`.
    pub dep_roots: std::collections::HashMap<String, std::path::PathBuf>,
}

#[derive(Debug)]
pub struct LoadedModule {
    pub path: std::path::PathBuf,
    /// Stable path string for diagnostics/codegen (e.g. `examples/features/21_imports/main.jet`).
    pub display: String,
    pub source: String,
    /// Namespace when this file is imported (`import … as alias`).
    pub alias: String,
    pub imports: Vec<ImportDecl>,
    pub items: Vec<Item>,
    /// D-WASM1: optional file-level web bucket ceiling.
    pub web_target_ceiling: Option<crate::WebPartition::WebBucket>,
    /// D-VISDEFAULT1=C / D-VISDEFAULT2=A: `#PubFile` flips default top-level export visibility.
    pub pub_file: bool,
    /// D-HTMLPAIR1 (open, c134): `#Html("path.html")` — this file's explicit
    /// companion host page for `--target=web` builds.
    pub html_path: Option<String>,
}

/// D-ERR-CONV (ratified 2026-06-19): how `?` converts the error type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TryConvert {
    /// Error types match exactly — no conversion needed.
    None,
    /// The source error implements `Fallible`; call `.to_error()` (D-ERR2).
    Fallible,
    /// Declared `impl Source -> Target { … }` conversion (D-ERR-CONV).
    /// Holds the mangled Rust function name emitted by codegen.
    Typed(String),
}

/// D-ERR-CONV (ratified 2026-06-19): `impl Source -> Target { body }` — declares
/// how a `Source` error becomes a `Target` error; `?` applies it automatically.
#[derive(Debug)]
pub struct ErrorConvDef {
    pub from_ty: String,
    pub from_span: Span,
    pub to_ty: String,
    pub to_span: Span,
    /// The single expression that is the conversion body.
    /// `self` in the body refers to the source error value.
    pub body: Vec<Stmt>,
    pub body_span: Span,
}

/// D-MIGRATE1 (ratified 2026-06-22): `migration TypeName { op; … }` block.
#[derive(Debug)]
pub struct MigrationDecl {
    pub type_name: String,
    pub type_span: Span,
    pub ops: Vec<MigrationOp>,
    pub span: Span,
}

/// D-MIGRATE1 / D-MIGRATE2: one operation inside a `migration { }` block.
#[derive(Debug)]
pub enum MigrationOp {
    /// D-MIGRATE1: `rename old_field -> new_field` — declares a field was renamed.
    Rename {
        from: String,
        from_span: Span,
        to: String,
        to_span: Span,
    },
    /// D-MIGRATE2A: `add f: T = default` — a new field with a default for old
    /// records. The `default` expr is the value old data is read with; sema only
    /// checks intent here (the runtime fill is the Build-tier versioning library).
    Add {
        field: String,
        field_span: Span,
        ty: Type,
        ty_span: Span,
        default: Expr,
        default_span: Span,
    },
    /// D-MIGRATE2D: `remove f` — deletes a field (verb is `remove`, not `drop`).
    Remove { field: String, field_span: Span },
    /// D-MIGRATE2E: `change f: Old -> New [via { expr }]` — a field type change.
    /// `converter` is the inline `via { … }` body (an expression, usually a
    /// lambda); `None` falls back to an `impl Old -> New` in scope (D-MIGRATE2B).
    Change {
        field: String,
        field_span: Span,
        from_ty: Type,
        from_span: Span,
        to_ty: Type,
        to_span: Span,
        converter: Option<Expr>,
        converter_span: Option<Span>,
    },
}

#[derive(Debug)]
pub enum Item {
    Func(Func),
    Struct(StructDef),
    Enum(EnumDef),
    /// D-DIST1 (ratified 2026-06-19): `UserId :: distinct Int` — a distinct type
    /// declaration. `distinct`-over-`distinct` base is rejected in sema.
    Distinct(DistinctDef),
    /// D-TYPEALIAS1 (ratified 2026-06-28): `alias Name<T> = …` — a transparent
    /// type alias for generic shortcuts. Erases at codegen (I3).
    TypeAlias(TypeAliasDef),
    /// D-QUAL3 (ratified 2026-06-24): `#UnitFamily(currency) { usd, eur, gbp }` —
    /// a unit family. Sugar: each member mints one `@Numeric` distinct type
    /// (`usd` → `Usd`) erasing to `Float`. Lowers to a `DistinctDef` per member
    /// in sema registration and codegen — it rides the D-DIST1/D-DIST3 machinery.
    UnitFamily(UnitFamilyDef),
    /// S28 (M9): `trait Name { fn sig(self) -> T; … }`.
    Trait(TraitDef),
    /// D-QUAL2 (ratified 2026-06-21): `tag Name;` or `tag Name { }` — a marker
    /// qualifier with no methods. It erases at runtime (codegen emits nothing).
    /// A method in a tag body is E0732; using a tag where dispatch is expected
    /// is E0731.
    Tag(TagDef),
    Impl(ImplDef),
    Const(ConstDef),
    /// S43 (M6): `#Test "name" { … }` — only at file top level.
    Test(TestDef),
    /// D-BENCH1 (ratified 2026-06-24): `#Bench "name" { … }` — a region
    /// benchmark, the exact sibling of `#Test`. Run by `jet bench`.
    Bench(BenchDef),
    /// S50 (M7): `extern rust "crate@version" { … }`.
    ExternRust(ExternRustBlock),
    /// U3 (unified-ecosystem §4): `module name { … }` — a named, composable
    /// declaration contributing typed values to reserved namespaces.
    Module(ModuleDecl),
    /// S59 (E2-M14): `#Extern module c.<lib> { … }` (user overlay) or
    /// `#Bindgen module c.<lib>.__bindgen__ { … }` (compiler-generated cache).
    CModule(CModule),
    /// D-MOD1/2 (code module system): `module name;` (file declaration) or
    /// `module name { … }` (inline body). `body = None` means the items live in
    /// a separate file found by the loader. NOT a JetOS module (see `ModuleDecl`).
    CodeModule(CodeModule),
    /// D-ERR-CONV (ratified 2026-06-19): `impl Source -> Target { … }` — typed
    /// error conversion; `?` applies it when propagating Source into a Target context.
    ErrorConv(ErrorConvDef),
    /// D-MIGRATE1 (ratified 2026-06-22): `migration TypeName { rename a -> b }`
    /// block — declares field renames on a `@PublishedSchema` struct.
    Migration(MigrationDecl),
    /// D-STATE-DECL (ratified 2026-06-25, option B): `state TypeName { A, B, C }` —
    /// declares the bounded set of states for a typestate machine. The set erases at
    /// runtime (pure compile-time, no discriminant). Each name in the body is a state
    /// label; `#State(S)` / `#Transition(From -> To)` markers on `TypeName::*` methods
    /// must reference names from this set (unknown state = E0151). A declared state with
    /// no outgoing `#Transition` is a dead-end warning (L0151). Declaration family sibling
    /// of `tag`/`struct`/`enum`.
    StateDecl(StateDecl),
    /// D-PROTO1 / D-PROTO2 (ratified 2026-06-27): `protocol Name { client -> server:
    /// Msg(…) }` — declares an ordered request/response exchange and expands (R11) into
    /// `#SingleUse` `.Client`/`.Server` handle types with typestate-checked send/recv
    /// methods. Erases as generated items; the declaration itself never reaches codegen.
    ProtocolDecl(ProtocolDecl),
    /// D-METADERIVE1=A: `derive T.Trait { … }` user-authored derive.
    UserDerive(DeriveDef),
    /// D-GENMOD2=A: `module Name<params> { … }` — a parameterized module template.
    /// Stores the body as-is; sema expands `ModuleAlias` references before codegen.
    GenericModule(GenericModuleDef),
    /// D-GENMOD2=A: `module Alias = Module<args>` — module instantiation alias.
    /// Expanded to a `CodeModule` by sema before registration and body-checking.
    ModuleAlias(ModuleAliasDef),
}

/// D-MOD1/2: code module — `module math;` or `module math { pub fn … }`.
#[derive(Debug)]
pub struct CodeModule {
    pub name: String,
    pub name_span: Span,
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) module …`.
    pub is_package_pub: bool,
    /// None = file declaration (`module math;`), Some = inline body.
    pub body: Option<Vec<Item>>,
    /// D-WASM1: `module name js { … }` / `module name wasm { … }` ceiling override.
    pub web_target: Option<crate::WebPartition::WebBucket>,
    pub span: Span,
}

/// D-GENMOD2=A: one parameter of a generic module — `module Lru<K: Hash, capacity: Int>`.
/// Parse-time heuristic: uppercase-starting name → type param; lowercase-starting → value param.
#[derive(Debug, Clone)]
pub enum GenericModuleParam {
    /// `K: Hash` — a type parameter. Bound is the trait/interface name (may be empty = no bound).
    TypeParam {
        name: String,
        name_span: Span,
        bound: String,
    },
    /// `capacity: Int` — a value parameter with a concrete type annotation.
    ValueParam {
        name: String,
        name_span: Span,
        ty: Type,
    },
}

impl GenericModuleParam {
    pub fn name(&self) -> &str {
        match self {
            GenericModuleParam::TypeParam { name, .. }
            | GenericModuleParam::ValueParam { name, .. } => name.as_str(),
        }
    }

    pub fn name_span(&self) -> Span {
        match self {
            GenericModuleParam::TypeParam { name_span, .. }
            | GenericModuleParam::ValueParam { name_span, .. } => *name_span,
        }
    }
}

/// D-GENMOD2=A: one argument in `module Alias = Module<String, 32>`.
#[derive(Debug, Clone)]
pub enum ModuleArg {
    /// A type argument: `String`, `Int`, `MyType`.
    Type(Type, Span),
    /// A value argument: integer/bool literal or other comptime expression.
    Value(Expr, Span),
}

/// D-GENMOD2=A: `module Name<params> { body }` — a parameterized module template.
/// Stores the body as a template. Sema expands `ModuleAlias` referencing this into
/// a `CodeModule` before the main checking pass. Never reaches codegen directly.
#[derive(Debug)]
pub struct GenericModuleDef {
    pub name: String,
    pub name_span: Span,
    pub is_pub: bool,
    pub is_package_pub: bool,
    pub params: Vec<GenericModuleParam>,
    pub body: Vec<Item>,
    pub span: Span,
}

/// D-GENMOD2=A: `module Alias = Module<args>` — module instantiation alias.
/// Expanded to a `CodeModule` by sema before registration and codegen.
#[derive(Debug)]
pub struct ModuleAliasDef {
    pub name: String,
    pub name_span: Span,
    pub is_pub: bool,
    pub is_package_pub: bool,
    pub target: String,
    pub target_span: Span,
    pub args: Vec<ModuleArg>,
    pub span: Span,
}

/// S59 (E2-M14): which attribute introduced a C FFI module — the user-written
/// overlay (`#Extern`) or the generated cache surface (`#Bindgen`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CModuleKind {
    /// `#Extern module c.<lib> { … }` — user overlay, allowed anywhere.
    Extern,
    /// `#Bindgen module c.<lib>.__bindgen__ { … }` — generated, cache files only.
    Bindgen,
}

/// S59 (E2-M14): one `#Extern`/`#Bindgen module c.<lib>[.__bindgen__] { … }` block.
#[derive(Debug, Clone)]
pub struct CModule {
    pub kind: CModuleKind,
    /// The library link key — last `c.<lib>` segment (e.g. `raylib`).
    pub lib: String,
    /// Span of the whole dotted module path (`c.raylib` / `c.raylib.__bindgen__`).
    pub path_span: Span,
    /// Foreign functions declared in the body (same shape as `extern rust`).
    pub functions: Vec<ExternFn>,
    pub span: Span,
}

/// U3 (unified-ecosystem §4): `module name { contributions… }`. Many modules
/// may share a file; a leading-`_` name disables one (not discovered/merged).
#[derive(Debug)]
pub struct ModuleDecl {
    pub name: String,
    pub name_span: Span,
    /// True when `name` begins with `_` (U3 one-character disable).
    pub disabled: bool,
    /// U8 (unified-ecosystem §2.2): named `sources:` declared inside the module
    /// body, siblings of the contributions. Merged by key across modules (U5).
    pub sources: Vec<SourceDecl>,
    /// U8: `imports: find("./modules")` import-tree directives, parsed as
    /// ordinary call expressions; the `find` walk lands with U4 discovery.
    pub imports: Vec<Expr>,
    /// D-WORKSPACE1=B: `members: <expr>` in `module workspace { … }` — the
    /// comptime expression that yields the list of member package paths. Only
    /// meaningful in `workspace.jet`; ignored in other module files.
    pub members: Vec<Expr>,
    pub contributions: Vec<Contribution>,
    pub span: Span,
}

/// U8 (unified-ecosystem §2.2): one `name: provider@target` entry in a module's
/// `sources:` block, e.g. `default: github@NixOS/nixpkgs/nixos-24.05`. The ref
/// is not a single token (it contains `@`, `/`, `-`, `.`), so the parser records
/// its source span; modeval slices the source and validates it via
/// `classify_provider_ref`.
#[derive(Debug)]
pub struct SourceDecl {
    pub name: String,
    pub name_span: Span,
    /// Span of the raw `provider@target` ref text in the source.
    pub ref_span: Span,
    pub span: Span,
}

/// U3 (unified-ecosystem §5): one typed namespace contribution inside a module,
/// e.g. `env.dev: Env { … }`. The value reuses the struct-literal expression
/// parser; the namespace and path locate it in the merged whole.
#[derive(Debug)]
pub struct Contribution {
    pub namespace: Namespace,
    pub path: String,
    pub path_span: Span,
    pub value: ContribValue,
    pub span: Span,
}

/// U11/U12/U14/U18: the value of a typed contribution. `env.<name>:` reuses the
/// ordinary expression parser (a struct literal), while `system.<name>:` and
/// `image.<name>:` parse into dedicated typed literals so the U13 `options` list
/// (`net.hostName: laptop`), the U13 typed `target` value (`linux.x64`), the U12
/// `Service` map, and U18 bare-`{ … }` records all have a home — none of which fit
/// the ordinary expression grammar.
#[derive(Debug)]
pub enum ContribValue {
    /// `env.<name>:` — any expression, typically `Env { … }` (or a bare `{ … }`,
    /// U18). modeval field-checks it.
    Expr(Expr),
    /// `system.<name>:` — a `System` record (U11).
    System(SystemLit),
    /// `image.<name>:` — an `Image` record (U14).
    Image(ImageLit),
}

impl ContribValue {
    pub fn span(&self) -> Span {
        match self {
            ContribValue::Expr(e) => e.span(),
            ContribValue::System(s) => s.span,
            ContribValue::Image(i) => i.span,
        }
    }
}

/// U11/U18: a `System { target, packages, services, options }` record. The
/// outer type name is optional (U18 inferred constructor): `explicit_type` is
/// `Some(span)` when the author wrote `System { … }`, `None` for a bare `{ … }`.
/// Field-checking (which fields are known, that `target` is a known platform, etc.)
/// lives in modeval, not the parser.
#[derive(Debug)]
pub struct SystemLit {
    pub explicit_type: Option<Span>,
    pub fields: Vec<SystemField>,
    pub span: Span,
}

/// One `name: value` field inside a `System { … }` record. The value's shape
/// depends on the field; modeval validates it against U11.
#[derive(Debug)]
pub struct SystemField {
    pub name: String,
    pub name_span: Span,
    pub value: SystemFieldValue,
    pub span: Span,
}

/// The parsed value of one `System` field (U11/U12/U13).
#[derive(Debug)]
pub enum SystemFieldValue {
    /// `target: linux.x64` — a dotted typed platform value (U13). Stores the two
    /// dotted segments (`os`, `arch`) and the whole value's span.
    Platform {
        os: String,
        arch: String,
        span: Span,
    },
    /// `packages: [ … ]` — a `ListLit` whose Pkg sugar modeval slices from source.
    Packages(Expr),
    /// `services: { name: { … }, … }` — a keyed map of bare `Service` records (U12).
    Services(Vec<ServiceEntry>),
    /// `options: [ net.hostName: laptop, … ]` — an ordered list of dotted-key /
    /// value entries (U13).
    Options(Vec<OptionEntry>),
    /// Any other field — captured as an expression so modeval can report it as an
    /// unknown `System` field with a span.
    Other(Expr),
}

/// U12: one `name: { … }` entry in a `services:` map. The record is an inferred
/// `Service` (U18); `explicit_type` is `Some(span)` if the author wrote
/// `Service { … }`. Fields are arbitrary (open record); modeval requires `enable`.
#[derive(Debug)]
pub struct ServiceEntry {
    pub name: String,
    pub name_span: Span,
    pub explicit_type: Option<Span>,
    pub fields: Vec<(String, Span, Expr)>,
    pub span: Span,
}

/// U13: one `dotted.key: value` entry in an `options:` list. `key` is the dotted
/// path text (`net.hostName`); `value` is any expression (bare identifier, dotted
/// typed value, list, or quoted free-form string).
#[derive(Debug)]
pub struct OptionEntry {
    pub key: String,
    pub key_span: Span,
    pub value: Expr,
    /// The full source span of the written value (`default.fish`), recorded
    /// directly so modeval can slice the typed value text without depending on
    /// each `Expr` variant's span covering its whole written form.
    pub value_span: Span,
    pub span: Span,
}

/// U14/U18: an `Image { from: system.<name>, format: iso }` record. `explicit_type`
/// mirrors `SystemLit`. `from`/`format`/`target` and any stray field are captured;
/// modeval validates them (U14: `from` required and references a known `System`;
/// `format` ∈ {iso, qcow, raw}; only `target:` may be restated, for cross-compile).
#[derive(Debug)]
pub struct ImageLit {
    pub explicit_type: Option<Span>,
    pub fields: Vec<ImageField>,
    pub span: Span,
}

/// One `name: value` field inside an `Image { … }` record.
#[derive(Debug)]
pub struct ImageField {
    pub name: String,
    pub name_span: Span,
    pub value: ImageFieldValue,
    pub span: Span,
}

/// The parsed value of one `Image` field (U14).
#[derive(Debug)]
pub enum ImageFieldValue {
    /// `from: system.<name>` — references a `System` by name. Stores the name and
    /// the whole value span.
    From { system: String, span: Span },
    /// `format: iso` — a bare format keyword. Stores the word and its span.
    Format { word: String, span: Span },
    /// `target: linux.x64` — an explicit cross-compile platform (U14).
    Platform {
        os: String,
        arch: String,
        span: Span,
    },
    /// Any other field — captured so modeval can reject restated inherited fields.
    Other(Expr),
}

/// U3 (unified-ecosystem §5): the reserved namespaces a module may contribute
/// to, each with a matching type (`Env`/`System`/`Image`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Namespace {
    /// `env` → `Env`: a development environment / shell.
    Env,
    /// `system` → `System`: a whole machine (jetos).
    System,
    /// `image` → `Image`: an ISO / VM / disk image (jetos).
    Image,
}

/// S45 (M9): type parameter with optional trait bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeParam {
    pub name: String,
    pub name_span: Span,
    pub bounds: Vec<String>,
}

/// S28 (M9): trait declaration — signatures only in v1.
#[derive(Debug)]
pub struct TraitDef {
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) trait …`.
    pub is_package_pub: bool,
    pub name: String,
    pub name_span: Span,
    /// D-LIB2: `type Name;` associated type declarations inside the trait body.
    pub assoc_types: Vec<(String, Span)>,
    pub methods: Vec<TraitMethodSig>,
}

/// D-QUAL2: `tag Name;` / `tag Name { }` — a marker qualifier. By the taxonomy
/// rule (methods → trait, no methods → tag) a tag carries no methods; any method
/// found in its body is reported as E0732. The body is parsed permissively (so a
/// stray method doesn't derail the parser) and validated in sema.
#[derive(Debug)]
pub struct TagDef {
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) tag …`.
    pub is_package_pub: bool,
    pub name: String,
    pub name_span: Span,
    /// Methods erroneously written in a tag body. Always empty for a well-formed
    /// tag; each entry triggers E0732 in sema.
    pub methods: Vec<TraitMethodSig>,
    pub span: Span,
}

/// D-PROTO1 / D-PROTO2: one message line inside a `protocol` block —
/// `client -> server: Hello(version: Int)`.
#[derive(Debug, Clone)]
pub enum ProtocolDirection {
    ClientToServer,
    ServerToClient,
}

/// D-PROTO1 / D-PROTO2: one ordered message in a protocol declaration.
#[derive(Debug, Clone)]
pub struct ProtocolMessage {
    pub direction: ProtocolDirection,
    pub name: String,
    pub name_span: Span,
    pub fields: Vec<(String, Type)>,
    pub span: Span,
}

/// D-PROTO1 / D-PROTO2: `protocol Name { … }` — the user-facing session-type
/// declaration. Expanded in sema into generated `#SingleUse` + typestate items (R11).
#[derive(Debug, Clone)]
pub struct ProtocolDecl {
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) protocol …`.
    pub is_package_pub: bool,
    pub name: String,
    pub name_span: Span,
    pub messages: Vec<ProtocolMessage>,
    pub span: Span,
}

/// D-STATE-DECL (ratified 2026-06-25, option B): `state TypeName { A, B, C }` —
/// a bounded compile-time state-set declaration. Each string in `states` is a valid
/// state label; `#State(X)` / `#Transition(A -> B)` markers on `TypeName::*` methods
/// must reference labels from this set. Erases in codegen (I3, no runtime discriminant).
#[derive(Debug)]
pub struct StateDecl {
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) state …`.
    pub is_package_pub: bool,
    pub type_name: String,
    pub type_name_span: Span,
    /// Declared state labels in declaration order, with their name spans for diagnostics.
    pub states: Vec<(String, Span)>,
    pub span: Span,
}

/// D-METADERIVE1=A: `derive T.Trait { … }` user-authored derive.
#[derive(Debug, Clone)]
pub struct DeriveDef {
    pub trait_name: String,
    pub trait_span: Span,
    pub type_param: String,
    pub body: Vec<Stmt>,
    pub span: Span,
}

/// S28: method signature inside a trait block (body optional per D-LIB2).
#[derive(Debug, Clone)]
pub struct TraitMethodSig {
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub is_view_return: bool,
    pub span: Span,
    /// D-LIB2: optional default body for a trait method.
    pub default_body: Option<Vec<Stmt>>,
    /// D-EFF3: `@Pure fn hash(self)` — the method declares the empty effect set
    /// as its upper bound. Every impl's inferred effects must be empty (E0742),
    /// and a dynamic-dispatch call sees the empty set.
    pub is_pure: bool,
    /// D-EFF3: `fn render(self) #(Gpu)` — an effect upper bound on the method.
    /// `None` = un-annotated (per-impl effects under static dispatch; a
    /// trait-object call under an effect ceiling is E0743). `Some(list)` is BOTH
    /// the impl obligation (inferred ⊆ bound, else E0742) AND the dispatch
    /// contract (a trait-object call's effect IS the bound).
    pub declared_effects: Option<Vec<(String, Span)>>,
}

/// S28: `impl Trait { … }` inside a struct or enum body.
#[derive(Debug)]
pub struct TraitImplBlock {
    pub trait_name: String,
    pub trait_span: Span,
    pub methods: Vec<Func>,
    /// D-LIB2: `type Name = ConcreteType;` associated type implementations.
    pub assoc_type_impls: Vec<(String, Span, Type)>,
}

/// S50: one `extern rust` block declaring foreign functions.
#[derive(Debug, Clone)]
pub struct ExternRustBlock {
    /// `"std"` or `"crate@version"`.
    pub crate_spec: String,
    pub crate_span: Span,
    pub functions: Vec<ExternFn>,
    pub span: Span,
}

/// S50: foreign function — Jet signature plus `= "rust::path"`, no body.
#[derive(Debug, Clone)]
pub struct ExternFn {
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub is_view_return: bool,
    pub rust_path: String,
    pub rust_path_span: Span,
    pub span: Span,
}

#[derive(Debug)]
pub struct TestDef {
    pub name: String,
    pub name_span: Span,
    /// D-TEST1 (ratified 2026-06-22, option B): a property test is an `#Test fn`
    /// with parameters — inputs are generated from the parameter types and a
    /// failing case is automatically shrunk. An empty `params` (the
    /// `#Test "name" { … }` block form) is a plain unit test. The two forms share
    /// one AST node; `params.is_empty()` distinguishes them.
    pub params: Vec<Param>,
    /// D-TEST1: span of the `fn name(…)` signature for diagnostics on a property
    /// test (param-type errors point here). `None` for the block form.
    pub fn_keyword_span: Option<Span>,
    pub body: Vec<Stmt>,
}

/// D-BENCH1: `#Bench "name" { … }` — identical structure to `TestDef`. The
/// body is a bare statement list timed by the generated bench harness.
#[derive(Debug)]
pub struct BenchDef {
    pub name: String,
    pub name_span: Span,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub struct Func {
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) fn …`.
    pub is_package_pub: bool,
    /// D-EXTMETH1=B: top-level `fn Type.method(...)` before parser normalization.
    /// The parser turns this into an inherent `ImplDef`; all later stages should
    /// see `None`.
    pub external_type: Option<(String, Span)>,
    pub name: String,
    pub name_span: Span,
    /// S45 (M9): `<T: Bound>` after the function name.
    pub type_params: Vec<TypeParam>,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub is_view_return: bool,
    /// S58 (E2-M13): `@unsafe` on the line before `fn` — a whole-function
    /// contract. Calling such a function requires an enclosing `@unsafe`
    /// block (else E3103).
    pub is_unsafe: bool,
    /// S60 (E2-M16): `pure fn` — impure calls inside the body are E3401.
    pub is_pure: bool,
    /// D-TAINT1 (ratified 2026-06-21): `#Sanitizer fn` — the blessed taint-strip
    /// function. Its return value is **untainted by contract** even when its
    /// inputs are tainted; this is the one place taint is cleared before a sink.
    /// Static, erased in codegen (I3).
    pub is_sanitizer: bool,
    /// D-EFF1 / D-QUAL1: a `#(Net, Db)` effect bound on the signature, between
    /// the parameter list and the return arrow. `None` = unannotated (effects
    /// inferred). `Some(list)` = a declared upper bound; the inferred set must be
    /// a subset (E0740). Names are validated in sema, not the parser. Erased in
    /// codegen (I3).
    pub declared_effects: Option<Vec<(String, Span)>>,
    /// D-EFF2 (`#(via f)` pass-through): when set, this function publishes a tight
    /// pass-through — its effect set IS whatever the callback parameter named `f`
    /// carries, rather than the conservative flow-through default. Holds the param
    /// name and the span of the `via` clause. Mutually exclusive with
    /// `declared_effects` (a `#(via f)` annotation occupies the same `#(…)` slot).
    /// Erased in codegen (I3).
    pub effect_via: Option<(String, Span)>,
    /// D-STATE1 (ratified 2026-06-22): `#State(S) fn …` — a require-state marker.
    /// `Some((state, span))` means the method's receiver must currently be in state
    /// `state`; calling it on a value in any other state is E0150. `None` =
    /// unguarded. Compile-time only, erased in codegen (I3).
    pub state_requires: Option<(String, Span)>,
    /// D-STATE1: `#Transition(From -> To) fn …` — a transition declaration. The fn
    /// consumes a value in state `from` (the wildcard `_` → `None`, an entry
    /// transition with no prior state) and produces one in state `to`. A call
    /// requires the receiver/argument be in `from` (E0150 otherwise) and advances it
    /// to `to`. The `Span` points at the marker. Erased in codegen (I3).
    pub state_transition: Option<StateTransition>,
    /// D-REACTCORE1: `#Reactive fn` — reactive effect scope; must not return a value.
    pub is_reactive: bool,
    /// D-MUSTUSE1 (c18iwxqx): `@MustUse fn` / `@MustUse` method — callers must not
    /// drop the return value as a bare expression statement (E0419).
    pub is_must_use: bool,
    pub must_use_span: Option<Span>,
    /// D-METHODMACRO1=A: `@Inline fn` / method — a soft hint (`#[inline]` in
    /// codegen); never rejected by sema.
    pub is_inline: bool,
    /// D-METHODMACRO1=A: `@InlineAlways fn` / method — a checked promise
    /// (`#[inline(always)]` in codegen). Sema rejects it (E0917/E0918/E0919)
    /// when the compiler can prove it genuinely cannot inline. Mutually
    /// exclusive with `is_inline` (E0920 if both are written).
    pub is_inline_always: bool,
    /// Span of whichever `@Inline`/`@InlineAlways` marker was written (for
    /// diagnostics); `None` when neither is present.
    pub inline_span: Option<Span>,
    /// D-WASM1: `#Wasm` / `#Js` / `#WasmExport` partition marker on the function.
    pub web_marker: Option<crate::WebPartition::WebPartitionMarker>,
    /// D-PREPOST1: `@Pre(cond, "msg")` clauses — a claim about the arguments,
    /// checked at function entry. Repeatable; empty when none.
    pub pre: Vec<ContractClause>,
    /// D-PREPOST1: `@Post(cond, "msg")` clauses — a claim about `result` (the
    /// return value), checked before each return. Repeatable; empty when none.
    pub post: Vec<ContractClause>,
    pub body: Vec<Stmt>,
}

/// D-PREPOST1: one `@Pre`/`@Post` contract clause — a pure condition plus the
/// message shown when it's violated (E3005 at runtime). `message_span` points
/// at the message string literal for diagnostics.
#[derive(Debug, Clone)]
pub struct ContractClause {
    pub cond: Expr,
    pub message: String,
    pub message_span: Span,
    pub span: Span,
}

/// D-STATE1: the parsed `#Transition(From -> To)` declaration on a function. `from`
/// is `None` for an entry transition (`_ -> To`).
#[derive(Debug, Clone)]
pub struct StateTransition {
    pub from: Option<String>,
    pub to: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub convention: AccessConvention,
    pub name: String,
    pub name_span: Span,
    pub ty: Type,
    pub ty_span: Span,
    /// S61: trailing `= expr` default value. Only trailing params may have defaults.
    pub default: Option<Box<Expr>>,
    /// D-VARIADIC1: `name: ...T` — last parameter only; `ty` is the element type.
    pub variadic: bool,
    /// D-ANY-JAI1/D-VARARGBOUND1: `name: ...[TraitA, TraitB]` — an explicit
    /// trait-bound list on a variadic parameter (heterogeneous elements, one
    /// generic type slot per call-site argument, checked+monomorphized, zero
    /// boxing). `None` for a bare `name: ...T` — that covers both the
    /// D-VARIADIC1 homogeneous-concrete-type form and the `...Trait`
    /// single-bound sugar; sema tells the two apart the same way
    /// `resolve_type_name` already does (is `T` a registered trait name?).
    /// When `Some`, `ty`/`ty_span` are an unused placeholder (`Type::Named("")`
    /// spanning the bracket list).
    pub variadic_bound_list: Option<Vec<String>>,
}

impl Param {
    /// D-ANY-JAI1/D-VARARGBOUND1: the resolved trait-bound list for a variadic
    /// parameter, or `None` when it's the plain D-VARIADIC1 homogeneous-concrete
    /// form. `is_trait_name` lets each crate plug in its own trait-name lookup
    /// (`TraitRegistry::is_trait_name` in sema, `Cx::trait_names` in codegen) —
    /// the classification rule itself lives once, here.
    pub fn variadic_trait_bounds(&self, is_trait_name: impl Fn(&str) -> bool) -> Option<Vec<String>> {
        if !self.variadic {
            return None;
        }
        if let Some(list) = &self.variadic_bound_list {
            return if list.is_empty() { None } else { Some(list.clone()) };
        }
        if let Type::Named(n) = &self.ty {
            if !n.is_empty() && is_trait_name(n) {
                return Some(vec![n.clone()]);
            }
        }
        None
    }
}

/// D-REPRC1 / D-SOA1 (ratified): the variant of `#layout(…)` on a struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructLayout {
    /// `#layout(c)` → `#[repr(C)]` on the generated Rust struct. (D-REPRC1=B)
    C,
    /// D-SOA1 / D-SOA2A=C: `#layout(columnar)` → a `[S]` collection is stored
    /// struct-of-arrays (one `Vec` per field) instead of array-of-structs. The
    /// logical `S` value and field-access syntax are unchanged; only the memory
    /// layout of the `[S]` collection differs (cache-friendly). Whole-struct only
    /// in v1 (D-SOA2B=A).
    Columnar,
}

/// D-ATTR2 / D-SERDE2–8: one `#[Name]` or `#[Name(arg, …)]` bracket marker.
/// Derive-trait markers (`Codable`/`Encode`/`Decode`/`Comparable`/…) are lifted
/// into `derives` at parse time (Codable expands to Encode+Decode); the serde
/// *attribute* markers — container `RenameAll`/`DenyUnknownFields`/`Tag`/`Untagged`
/// and field `Rename`/`Skip`/`Default`/`Flatten` — are kept raw here so sema can
/// validate them (E2407–E2412) and codegen can apply them. Args parse as
/// expressions: a string literal (`Rename("x")`), a bare word (`RenameAll(camel)`
/// → `Expr::Ident`), or any expression (`Default(8080)`).
#[derive(Debug, Clone)]
pub struct Marker {
    pub name: String,
    pub name_span: Span,
    pub args: Vec<Expr>,
    pub span: Span,
}

#[derive(Debug)]
pub struct StructDef {
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) struct …`.
    pub is_package_pub: bool,
    pub name: String,
    pub name_span: Span,
    /// S45: `<T>` after the struct name.
    pub type_params: Vec<TypeParam>,
    pub fields: Vec<Field>,
    pub methods: Vec<Func>,
    /// S28: in-type `impl Trait { … }` blocks.
    pub trait_impls: Vec<TraitImplBlock>,
    /// S55: `derive Comparable;` / `derive Serialize;` lines.
    pub derives: Vec<(String, Span)>,
    /// D-MIGRATE1 (ratified 2026-06-22): `@PublishedSchema` marker was present
    /// before `struct`. The span is retained for pointing at the annotation in E0910.
    pub is_published_schema: bool,
    pub published_schema_span: Option<Span>,
    /// D-LIN1 (ratified 2026-06-21): `#SingleUse` marker before `struct` — values
    /// of this type must be consumed exactly once on every path (E0140/E0141)
    /// and may not be aliased (E0142). Implies `#NoCopy`. The span points at the
    /// marker for diagnostics.
    pub is_single_use: bool,
    pub single_use_span: Option<Span>,
    /// D-MUSTUSE1 (c18iwxqx): `@MustUse` marker before `struct` — values of this
    /// type cannot be silently ignored as a bare expression statement (E0419).
    pub is_must_use: bool,
    pub must_use_span: Option<Span>,
    /// D-REPRC1 (ratified; D-REPRC1 = B): `#layout(…)` attribute. `None` = default layout.
    pub layout: Option<StructLayout>,
    pub layout_span: Option<Span>,
    /// D-SERDE3/8: container-level serde attribute markers (`RenameAll`,
    /// `DenyUnknownFields`) attached before the `struct`. Empty when none.
    pub serde_markers: Vec<Marker>,
    /// Round-trip fidelity (formatter): the exact `#[…]` bracket-marker list the
    /// user wrote before the type, verbatim and in source order (e.g. `Codable`,
    /// `RenameAll(camel)`). Derive-trait markers here are *also* lowered into
    /// `derives` (Codable → Encode+Decode) for sema/codegen, and serde attrs are
    /// *also* copied into `serde_markers`; this field exists only so `jet fmt`
    /// re-emits the surface the user actually typed instead of a lowered form.
    /// Empty when the type had no leading `#[…]` list.
    pub type_markers: Vec<Marker>,
}

/// D-TYPEALIAS1: `alias Name<T, E> = T ? E` — transparent generic type shortcut.
#[derive(Debug)]
pub struct TypeAliasDef {
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) alias …`.
    pub is_package_pub: bool,
    pub name: String,
    pub name_span: Span,
    pub type_params: Vec<TypeParam>,
    pub target: Type,
    pub target_span: Span,
    pub span: Span,
}

/// D-DIST1/D-DIST3: distinct type declaration — `[@Numeric] Name :: distinct Base`.
#[derive(Debug)]
pub struct DistinctDef {
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) Name :: distinct Base`.
    pub is_package_pub: bool,
    /// D-DIST3: whether `@Numeric` marker was present — enables same-type arithmetic.
    pub is_numeric: bool,
    /// D-CAPBUNDLE1: `@Comparable` was present — grants hash/sort on top of the
    /// ordering the base type already carries (D-DIST1 makes every distinct type
    /// `==`/`<`-comparable unconditionally already; that overlap is left alone
    /// per the ballot). Stacks with the other three bundles.
    pub is_comparable: bool,
    pub comparable_span: Option<Span>,
    /// D-CAPBUNDLE1: `@Printable` was present — grants `{value}` string
    /// interpolation (Display), forwarding to the base type's rendering.
    pub is_printable: bool,
    pub printable_span: Option<Span>,
    /// D-CAPBUNDLE1: `@CodableAsBase` was present — grants encode/decode via
    /// the base type's wire representation.
    pub is_codable_as_base: bool,
    pub codable_as_base_span: Option<Span>,
    pub name: String,
    pub name_span: Span,
    pub base: Type,
    pub base_span: Span,
    /// D-RANGETYPE1: an optional literal range constraint — `distinct
    /// Int(0..10)` provably holds `0..=10` (`..` inclusive, S22). `(lo, hi,
    /// span-of-the-`(lo..hi)`-clause)`.
    pub range: Option<(i64, i64, Span)>,
    pub span: Span,
}

/// D-QUAL3 (ratified 2026-06-24): unit-family declaration —
/// `#UnitFamily(currency) { usd, eur, gbp }`. Each member mints a distinct
/// `@Numeric` type erasing to `Float`. `members` carries each member's source
/// spelling (lowercase, e.g. `usd`) and span; the minted type name is the
/// PascalCase form (`Usd`).
#[derive(Debug)]
pub struct UnitFamilyDef {
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) #UnitFamily(…) { … }`.
    pub is_package_pub: bool,
    /// The family label, e.g. `currency` — documentation only; not a type name.
    pub family: String,
    pub family_span: Span,
    /// Each member's source spelling and span (e.g. `("usd", span)`).
    pub members: Vec<(String, Span)>,
    pub span: Span,
}

impl UnitFamilyDef {
    /// PascalCase the member spelling to its minted distinct-type name:
    /// `usd` → `Usd`, `m_per_s` → `MPerS`. Splits on `_`, uppercases each
    /// segment's first char. Empty/edge inputs return the input unchanged.
    pub fn type_name(member: &str) -> String {
        let mut out = String::with_capacity(member.len());
        for seg in member.split('_') {
            let mut chars = seg.chars();
            if let Some(first) = chars.next() {
                out.extend(first.to_uppercase());
                out.push_str(chars.as_str());
            }
        }
        if out.is_empty() {
            member.to_string()
        } else {
            out
        }
    }

    /// The minted `DistinctDef` for each member (`@Numeric`, base `Float`).
    /// Used by sema registration and codegen to lower the family.
    pub fn distinct_defs(&self) -> Vec<DistinctDef> {
        self.members
            .iter()
            .map(|(member, span)| DistinctDef {
                is_pub: self.is_pub,
                is_package_pub: self.is_package_pub,
                is_numeric: true,
                is_comparable: false,
                comparable_span: None,
                is_printable: false,
                printable_span: None,
                is_codable_as_base: false,
                codable_as_base_span: None,
                name: Self::type_name(member),
                name_span: *span,
                base: Type::Float,
                base_span: *span,
                range: None,
                span: *span,
            })
            .collect()
    }
}

#[derive(Debug)]
pub struct EnumDef {
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) enum …`.
    pub is_package_pub: bool,
    pub name: String,
    pub name_span: Span,
    pub type_params: Vec<TypeParam>,
    pub variants: Vec<Variant>,
    pub methods: Vec<Func>,
    pub trait_impls: Vec<TraitImplBlock>,
    pub derives: Vec<(String, Span)>,
    /// D-LIN1 (ratified 2026-06-21): `#SingleUse` marker before `enum`. See
    /// `StructDef::is_single_use`.
    pub is_single_use: bool,
    pub single_use_span: Option<Span>,
    /// D-MUSTUSE1 (c18iwxqx): `@MustUse` marker before `enum`. See
    /// `StructDef::is_must_use`.
    pub is_must_use: bool,
    pub must_use_span: Option<Span>,
    /// D-SERDE3/7/8: container-level serde markers (`RenameAll`, `Tag`,
    /// `Untagged`, `DenyUnknownFields`) attached before the `enum`. Empty when none.
    pub serde_markers: Vec<Marker>,
    /// Round-trip fidelity (formatter): the exact `#[…]` bracket-marker list the
    /// user wrote before the `enum`, verbatim and in source order. See
    /// `StructDef::type_markers`. Empty when none.
    pub type_markers: Vec<Marker>,
}

#[derive(Debug)]
pub struct Variant {
    pub name: String,
    pub name_span: Span,
    pub payload: VariantPayload,
    /// D-SERDE5: per-variant serde markers (`Rename`). Empty when none.
    pub serde_markers: Vec<Marker>,
}

#[derive(Debug, Clone)]
pub enum VariantPayload {
    Unit,
    /// S30: single-field variants use a positional type only.
    Single(Type, Span),
    /// S30: two or more payload fields are named in the declaration.
    Named(Vec<VariantField>),
}

#[derive(Debug, Clone)]
pub struct VariantField {
    pub name: String,
    pub name_span: Span,
    pub ty: Type,
    pub ty_span: Span,
}

#[derive(Debug)]
pub struct ImplDef {
    pub type_name: String,
    pub type_span: Span,
    /// S28: `impl Type: Trait` — `None` means plain `impl Type { fn … }`.
    pub trait_name: Option<String>,
    pub trait_span: Option<Span>,
    pub methods: Vec<Func>,
    /// S62: `impl Type: Trait using field_name;` — the field that supplies the
    /// delegation target. When `Some`, `methods` is empty and the compiler
    /// generates forwarding for all trait methods.
    pub delegation_field: Option<String>,
    /// D-LIB2: `type Name = ConcreteType;` in top-level impl blocks.
    pub assoc_type_impls: Vec<(String, Span, Type)>,
}

#[derive(Debug, Clone)]
pub struct Field {
    /// S18: visible to other files via `import` when true.
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) fieldname: T`.
    pub is_package_pub: bool,
    pub is_stored_ref: bool,
    pub stored_ref_label: Option<String>,
    pub name: String,
    pub name_span: Span,
    pub ty: Type,
    pub ty_span: Span,
    /// D-SERDE5: per-field serde markers (`Rename`/`Skip`/`Default`/`Flatten`)
    /// attached before this field. Empty when none.
    pub serde_markers: Vec<Marker>,
    /// D-DEBUG-REDACT: `@[Redact]` — omit/redact in auto-derived Debug output.
    pub redact: bool,
}

/// D-PATW / D-PATR (ratified 2026-06-19): a single payload slot inside a variant pattern.
/// `Active(_)` — wildcard (D-PATW); `Closing(500..599)` — range (D-PATR).
#[derive(Debug, Clone)]
pub enum PatSlot {
    /// D-PATW: `_` in payload position — ignore this field, bind nothing.
    Wildcard,
    /// Regular name binding: `Active(id)`.
    Bind(String),
    /// D-PATR: `lo..hi` range in payload slot (inclusive). Field type must be Int or Char.
    Range { lo: i64, hi: i64 },
}

impl PatSlot {
    /// Returns the binding name if this is a `Bind` slot, else `None`.
    pub fn as_bind(&self) -> Option<&str> {
        if let PatSlot::Bind(s) = self {
            Some(s)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Variant {
        variant: String,
        /// D-PATW/D-PATR: slots can be wildcards or ranges, not just names.
        bindings: Vec<PatSlot>,
        span: Span,
    },
    Present {
        binding: String,
        span: Span,
    },
    Absent(Span),
    /// S34: `ok(binding)` pattern on `T ? E`.
    Ok {
        binding: String,
        span: Span,
    },
    /// S34: `err(binding)` pattern on `T ? E`.
    Err {
        binding: String,
        span: Span,
    },
    /// D-PATR (ratified 2026-06-19): range pattern at arm-head level (`0..59 -> "F"`).
    /// Subject must be Int or Char. Open types always still require `else`.
    Range {
        lo: i64,
        hi: i64,
        span: Span,
    },
    /// D-PATO (ratified 2026-06-19): structural or-pattern `A(x) | B(x)`.
    /// All alternatives must bind the same names at the same types (E0317).
    Or(Vec<Pattern>, Span),
    /// D-DESTRUCT1: a struct-shaped dispatch arm head:
    /// `.{ kind: "page", title, .. } -> ...`.
    Struct {
        fields: Vec<StructPatField>,
        rest: Option<Span>,
        span: Span,
    },
    /// D-PARSESTR1: the same interpolation literal that formats a string can
    /// sit in pattern position — matches the fixed text and binds each
    /// `{hole}` to a name (untyped binds `String`; `{hole:Type}` binds `Type`
    /// and is a fallible parse). Always refutable (D-PARSESTR2 amendment):
    /// the literal text might not match, and a typed hole's parse can fail.
    StrMatch {
        parts: Vec<StrMatchPart>,
        span: Span,
    },
}

/// D-PARSESTR1: one piece of a string-interpolation-literal used as a
/// pattern — fixed text to match, or a hole to bind (optionally typed).
#[derive(Debug, Clone)]
pub enum StrMatchPart {
    Lit(String),
    Hole {
        name: String,
        /// `None` binds `String`; `Some(t)` binds `t` via a fallible parse
        /// from the matched substring (E0148 if unhandled by an `else`).
        ty: Option<Type>,
        span: Span,
    },
}

/// D-DESTRUCT1: one field inside a struct pattern arm head.
#[derive(Debug, Clone)]
pub enum StructPatField {
    /// `field` or `field: local` — bind the field value into the arm body.
    Bind {
        field: String,
        field_span: Span,
        local: String,
        local_span: Span,
    },
    /// `field: value` — require the field to equal this value.
    Value {
        field: String,
        field_span: Span,
        value: Box<Expr>,
    },
}

impl StructPatField {
    pub fn field_name(&self) -> &str {
        match self {
            StructPatField::Bind { field, .. } | StructPatField::Value { field, .. } => field,
        }
    }

    pub fn field_span(&self) -> Span {
        match self {
            StructPatField::Bind { field_span, .. } | StructPatField::Value { field_span, .. } => {
                *field_span
            }
        }
    }
}

/// S74: a single name bound by a destructuring target.
#[derive(Debug, Clone)]
pub struct BindName {
    pub name: String,
    pub span: Span,
    /// D-DESTRUCT1: `severity: sev` — the local binding name when the struct
    /// field is renamed. `None` means bind under the field's own name
    /// (`self.name`). Always `None` for `List`/`Tuple` patterns.
    pub rename: Option<(String, Span)>,
}

impl BindName {
    /// The name actually bound in scope: the rename if present, else the
    /// field/element name itself.
    pub fn local_name(&self) -> &str {
        self.rename
            .as_ref()
            .map(|(n, _)| n.as_str())
            .unwrap_or(&self.name)
    }
}

/// S74: the destructuring target on the left of a `val`/`var` binding.
/// Reuses the existing bracket conventions — `Type { fields }` for structs,
/// `[ elems ]` for lists, `( a, b )` for named tuples (S73/S74).
#[derive(Debug, Clone)]
pub enum BindPattern {
    /// `Point.{ x, y } :: p;` — binds a subset of the struct's fields.
    /// D-DESTRUCT1: `rest` is `Some(span)` of a trailing `..` — MANDATORY
    /// whenever `fields` doesn't name every field of the struct (E0326); a
    /// `..` on a pattern that already names every field is E0327.
    Struct {
        type_name: String,
        type_span: Span,
        fields: Vec<BindName>,
        rest: Option<Span>,
        span: Span,
    },
    /// `val [a, b] = xs;` — binds list elements by position.
    List { elems: Vec<BindName>, span: Span },
    /// `val (x, y) = p;` — binds named tuple fields in canonical (sorted) order.
    Tuple { elems: Vec<BindName>, span: Span },
}

impl BindPattern {
    pub fn span(&self) -> Span {
        match self {
            BindPattern::Struct { span, .. }
            | BindPattern::List { span, .. }
            | BindPattern::Tuple { span, .. } => *span,
        }
    }

    /// Every name this pattern brings into scope, in source order.
    pub fn names(&self) -> &[BindName] {
        match self {
            BindPattern::Struct { fields, .. } => fields,
            BindPattern::List { elems, .. } => elems,
            BindPattern::Tuple { elems, .. } => elems,
        }
    }
}

/// S35/D-ORRETURN-ERG1: right-hand side of `expr ?? …`.
#[derive(Debug, Clone)]
pub enum OrFallback {
    Value(Box<Expr>),
    Return(Option<Box<Expr>>, Span),
    Panic {
        name_span: Span,
        args: Vec<CallArg>,
    },
    /// D-ORRETURN-ERG1=B: `expr ?? break` — loop-only, sema-gated.
    Break(Span),
    /// D-ORRETURN-ERG1=B: `expr ?? continue` — loop-only, sema-gated.
    Continue(Span),
}

impl Pattern {
    pub fn span(&self) -> Span {
        match self {
            Pattern::Variant { span, .. }
            | Pattern::Present { span, .. }
            | Pattern::Ok { span, .. }
            | Pattern::Err { span, .. }
            | Pattern::Range { span, .. } => *span,
            Pattern::Absent(span) => *span,
            Pattern::Or(_, span) => *span,
            Pattern::Struct { span, .. } => *span,
            Pattern::StrMatch { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone)]
pub enum EnumLitArg {
    Positional(Expr),
    Named { label: String, expr: Expr },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstAttr {
    ForceStatic,
    ForceInline,
}

#[derive(Debug)]
pub struct ConstDef {
    pub name: String,
    pub name_span: Span,
    pub value: Expr,
    pub attrs: Vec<ConstAttr>,
    pub rust_kind: RustConstKind,
    /// S57 (M9.5): `comptime NAME = expr;` — evaluated at compile time.
    pub is_comptime: bool,
    /// Filled by sema for comptime bindings: the evaluated constant value,
    /// serialized to a Rust literal at use sites by codegen.
    pub ct: Option<CtValue>,
    /// D-PERSIST1: `@Persist` was present before this module-level binding —
    /// its value survives a `jet dev` hot reload instead of resetting
    /// (identity = module path + binding name). Inert in release builds.
    pub is_persist: bool,
    pub persist_span: Option<Span>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustConstKind {
    Const,
    Static,
}

/// One `if`/`else if`/`else` chain.
#[derive(Debug, Clone)]
pub struct IfStmt {
    pub cond: Expr,
    pub then_body: Vec<Stmt>,
    pub else_branch: Option<ElseBranch>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ElseBranch {
    ElseIf(Box<IfStmt>),
    Else(Vec<Stmt>),
}

/// One `switch` arm: a condition and a body (S24).
#[derive(Debug, Clone)]
pub struct SwitchArm {
    pub cond: Expr,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    /// A call used for its effect, e.g. `print(x);`.
    Expr(Expr),
    Val(Binding),
    /// `target = e;` (op None) or `target += e;` etc. (op Some, S17).
    Assign {
        target: LValue,
        op: Option<BinOp>,
        op_span: Span,
        value: Expr,
    },
    Return(Option<Expr>, Span),
    If(IfStmt),
    While {
        cond: Expr,
        body: Vec<Stmt>,
        span: Span,
        /// D-LABEL1: optional `@name` loop label (`@outer loop cond { }`).
        label: Option<(String, Span)>,
    },
    /// `for i in a..b` (S22) or `for x in collection` / `for k, v in map` (M5).
    For {
        var: String,
        var_span: Span,
        /// Second binding for `for key, value in map`.
        var2: Option<(String, Span)>,
        kind: ForKind,
        body: Vec<Stmt>,
        span: Span,
        /// D-LABEL1: optional `@name` loop label.
        label: Option<(String, Span)>,
    },
    Switch {
        subject: Expr,
        arms: Vec<SwitchArm>,
        else_body: Option<Vec<Stmt>>,
        span: Span,
    },
    Break(Span),
    Continue(Span),
    /// D-LABEL1: `break @name` / `continue @name` targeting a labeled loop.
    BreakLabel(String, Span),
    ContinueLabel(String, Span),
    Loop {
        body: Vec<Stmt>,
        span: Span,
        /// D-LABEL1: optional `@name` loop label (`@outer loop { }`).
        label: Option<(String, Span)>,
    },
    /// D-LOOP-SEMICOLON1=A: `loop init; cond; step { body }` — three-part counted loop.
    /// Lowers to: `{ init; loop cond { body; step } }` in codegen.
    CountedLoop {
        init: Binding,
        cond: Expr,
        step: Box<Stmt>,
        body: Vec<Stmt>,
        span: Span,
        label: Option<(String, Span)>,
    },
    /// S58 (E2-M13): `@unsafe { … }` audited region. `audit` carries the
    /// `@audit("…")` reason on the line above, when present (lint L3101 fires
    /// when it is `None`). `body` is the gated statements.
    Unsafe {
        audit: Option<String>,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-CTEFFECT1 (ratified 2026-06-25): `#Impure("reason") { … }` — the
    /// audited Tier-2 comptime effect gate. `reason` is the argument of
    /// `#Impure` itself (lint L3102 fires when it is `None`). Both this gate
    /// AND `--allow-impure` at build time are required to execute ambient
    /// comptime I/O (Fs/Env/Exec/Io). Erases to a plain block at codegen;
    /// the gate is enforced entirely in the comptime interpreter (I3).
    Impure {
        reason: Option<String>,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-REACTCORE1 (ratified 2026-06-27, opt D): `#Reactive { … }` in statement
    /// position. Lowers to a reactive effect registration at codegen.
    Reactive {
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-IGNORERET2=A (ratified 2026-06-28): `#Suppress(MustUse) { … }` — a
    /// lexical scope in which all fallible / `@MustUse` statement results are
    /// allowed to be silently dropped without `.drop("reason")`.  Erases to a
    /// plain block at codegen; the gate is enforced entirely in sema (I3).
    SuppressMustUse {
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-REGION1 (ratified 2026-06-21, opt B): explicit allocation region
    /// `region r { … }`. `name` names the region; arena `view`s allocated
    /// inside may not escape it (E0631). A lexical scope like `loop`/`#Unsafe`,
    /// emitted as a plain Rust block — the region bound is enforced entirely in
    /// sema (I3: codegen stays dumb).
    Region {
        name: String,
        name_span: Span,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-TASKSCOPE1=A / D-NURSERY1=A: `taskgroup g { … }` — a lexical scope that
    /// owns child tasks. `g.task { … }` spawns; scope exit joins/cancels children.
    /// Emitted as a plain block at codegen; lifetime is enforced in sema (I3).
    TaskGroup {
        name: String,
        name_span: Span,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-LAYOUT1 / D-LAYOUT-GATES1 (ratified 2026-06-28/29): `layout NAME { … }`
    /// — a Cassowary-style constraint block. Unlike `region`/`taskgroup`, `name`
    /// is declared in the ENCLOSING scope and outlives the block (solved values
    /// are read after the layout is defined). The parser desugars each
    /// `box.anchor` read inside `body` into a `NAME.h(box, anchor)` /
    /// `NAME.v(box, anchor)` method call before sema ever sees it, so every line
    /// is an ordinary `Stmt::Expr`/`Stmt::Bind` comparison expression checked by
    /// the general GATE-1/GATE-2 machinery — `body` carries no layout-specific
    /// AST shape of its own.
    Layout {
        name: String,
        name_span: Span,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-EFF1 / D-QUAL1: a `#Caps(Net, Db) { … }` effect-restriction region. The
    /// `caps` list is the only effects the body (and everything it transitively
    /// calls) may use; an out-of-set effect is E0741. `caps` names are validated
    /// in sema. A lexical scope emitted as a plain Rust block — the restriction
    /// is enforced entirely in sema (I3: codegen stays dumb, effects erase).
    Caps {
        caps: Vec<(String, Span)>,
        caps_span: Span,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-SCAP1 (ratified 2026-06-21, opt A): a scoped-capability grant region
    /// `#grant(Fs) { caps -> … }`. The listed effects are **authorized** inside
    /// the block via the first-class handle `binding` (here `caps`), and the
    /// capability is **revoked at scope end** by the RAII rule (S63) — the handle
    /// is bound only for the block's extent. The dual of `#Caps`: `#Caps`
    /// *restricts* a region to a set, `#grant` *authorizes* one. An effect used
    /// inside the block that the grant doesn't cover has no capability backing it
    /// (E0712); letting the handle escape (returned, stored, captured) is E0711.
    /// A lexical scope emitted as a plain Rust block — the grant/revoke is a
    /// compile-time capability fact, erased in codegen (I3).
    Grant {
        caps: Vec<(String, Span)>,
        caps_span: Span,
        /// The bound capability handle name (`caps` in `#grant(Fs) { caps -> … }`).
        binding: String,
        binding_span: Span,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-WHEN1/D-WHEN2 (ratified 2026-06-19): `comptime if <cond> { … } else { … }`.
    /// The condition is evaluated at compile time; only the selected arm is
    /// type-checked and lowered (D-WHEN2: the dropped arm is name-resolved only).
    /// `else_body` is None when no `else` clause is written (statement position
    /// only; in expression position both arms are required by the caller).
    ComptimeIf {
        cond: Expr,
        cond_span: Span,
        then_body: Vec<Stmt>,
        else_body: Option<Vec<Stmt>>,
        span: Span,
        /// Filled by sema: true if the `then` arm is selected, false if `else`.
        /// None before sema runs.
        selected_then: Option<bool>,
    },
    /// D-CTMARKER1 (ratified 2026-06-25, piece 2): `comptime { … }` — a
    /// build-time execution block. Runs at compile time via the tree-walking
    /// comptime interpreter; erases entirely (no runtime Rust emitted, I3).
    /// Pure-only in Stage A (D-CTCORE1 whitelist + E0951/E0958/E0953/E0956);
    /// effect tiers (D-CTEFFECT1) wire in c157. Bindings inside do not leak to
    /// the enclosing scope. `$name` splice (piece 1) deferred to c155.
    ComptimeBlock {
        body: Vec<Stmt>,
        span: Span,
    },

    /// D-CTX1 (ratified 2026-06-22, G2): `#Context(field: value, …) { … }`.
    /// Swaps named ambient fields for the block's lexical+dynamic extent, then
    /// restores them on all exit paths (return, break, ?, panic unwind) via
    /// a RAII guard. Expert-tier; never surfaced in beginner diagnostics.
    /// v1 fields: `allocator` (allocator handle), `logger` (logger handle),
    /// `deadline` (absolute epoch-millis Int budget).
    /// Q1 = A2: an explicit allocator arg at a call site overrides the ambient.
    /// Q2 = Cβ: restore is per-block (on guard Drop).
    ContextBlock {
        /// `(field_name, value_expr, field_span)` — one entry per `field: value`.
        fields: Vec<(String, Expr, Span)>,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-TERM1 (ratified 2026-06-22): `live { … }` — enter un-buffered/no-echo
    /// terminal input mode for the body, restore on every exit (normal, `return`,
    /// `?`, and panic) via the D-DEFER1 scope-guard mechanism.
    /// `use core.term as term` makes `term.read_key() -> Key` available.
    Live {
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-DET1 (ratified 2026-06-22): `assume_deterministic { … }` — the expert
    /// determinism-escape block. Inside a `@Pure fn`, the body's determinism
    /// rejections (E3401 impure-call / E3403 non-deterministic Core call) are
    /// **suspended** — the "I know this is deterministic" hatch. A semantic
    /// footgun, v1-legal per the card. A lexical scope emitted as a plain Rust
    /// block; the suppression is a compile-time fact, erased in codegen (I3).
    AssumeDet {
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-TXN1–D-TXN4 (ratified 2026-06-24): `#Transact(name) { … }` — a
    /// transaction block. `name` binds a user-chosen transaction handle (any
    /// lowercase ident, mirroring `region r { … }`) typed `Transaction`.
    /// Inside the block an irreversible effect (Net/Fs/Exec) that can't be rolled
    /// back is a compile error (E0746, D-TXN2) — the fix is to move it after the
    /// block or register it via `name.on_commit(() => { … })` (D-TXN3) so it runs
    /// only on a clean commit. `on_commit` lambdas are Drop-backed and run LIFO on
    /// commit, dropped on a `?`-failure/rollback. A lexical scope emitted as a
    /// plain Rust block; effects/transaction state erase (I3).
    Transact {
        /// The user-chosen handle name, or `None` for a bare `#Transact { … }` with
        /// no hooks (D-TXN4: a transaction without a handle stays legal). A name is
        /// required only to call `name.on_commit(…)`.
        name: Option<String>,
        name_span: Option<Span>,
        body: Vec<Stmt>,
        span: Span,
    },
    /// D-STREAMYIELD1: `yield expr` — hand a value to a `Stream<T>` consumer
    /// and suspend until the next pull. Legal only in a function whose return
    /// type is `Stream<T>` (E0805 otherwise); `expr: T` (E0807 otherwise).
    Yield(Expr, Span),
}

impl Stmt {
    /// The source span this statement occupies, used by the source-level
    /// debugger (D-DBG3) to resolve a Jet line for a breakpoint or `<- here`
    /// caret. For statements that carry no explicit `span` field, this falls
    /// back to the span of the expression/sub-part that anchors them.
    pub fn span(&self) -> Span {
        match self {
            Stmt::Expr(e) => e.span(),
            Stmt::Val(b) => b.name_span,
            Stmt::Assign { target, .. } => target.span(),
            Stmt::Return(_, span)
            | Stmt::Break(span)
            | Stmt::Continue(span)
            | Stmt::BreakLabel(_, span)
            | Stmt::ContinueLabel(_, span)
            | Stmt::While { span, .. }
            | Stmt::For { span, .. }
            | Stmt::Switch { span, .. }
            | Stmt::Loop { span, .. }
            | Stmt::CountedLoop { span, .. }
            | Stmt::Unsafe { span, .. }
            | Stmt::Impure { span, .. }
            | Stmt::Reactive { span, .. }
            | Stmt::SuppressMustUse { span, .. }
            | Stmt::Region { span, .. }
            | Stmt::TaskGroup { span, .. }
            | Stmt::Layout { span, .. }
            | Stmt::Caps { span, .. }
            | Stmt::Grant { span, .. }
            | Stmt::ComptimeIf { span, .. }
            | Stmt::ComptimeBlock { span, .. }
            | Stmt::ContextBlock { span, .. }
            | Stmt::Live { span, .. }
            | Stmt::AssumeDet { span, .. }
            | Stmt::Transact { span, .. } => *span,
            Stmt::Yield(_, span) => *span,
            Stmt::If(ifs) => ifs.cond.span(),
        }
    }
}

/// Assignment target: local name or indexed collection slot (M5).
#[derive(Debug, Clone)]
pub enum LValue {
    Local {
        name: String,
        name_span: Span,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        span: Span,
        /// Filled by sema (like `Expr::Index`) so codegen picks the right
        /// runtime helper for `xs[i] = v` vs `m[k] = v`.
        kind: IndexKind,
    },
    /// D-MUTSELF1: a field-assignment target `place.field = v`. The headline use is
    /// `self.field = v` inside a `mut self` method (lowers to `(*self).field = v` on
    /// the `&mut Self` receiver). `base` is the receiver expression (an `Expr`, not a
    /// nested `LValue`), `field` the member name. Sema gates the root: a field-assign
    /// rooted at a non-`mut` `self` (or any non-changeable place) is E0205.
    Field {
        base: Box<Expr>,
        field: String,
        span: Span,
    },
}

impl LValue {
    /// The source span of an assignment target (for the D-DBG3 debugger line map).
    pub fn span(&self) -> Span {
        match self {
            LValue::Local { name_span, .. } => *name_span,
            LValue::Index { span, .. } | LValue::Field { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum IndexKind {
    #[default]
    Unknown,
    List,
    Map,
    /// D-SIMD2: `v[i]` lane access on a SIMD lane type (`F32x4`/`F64x2`). Lowers to a
    /// bounds-checked lane read `{root}jet_math_<T>_lane(&v, i, file, line)`.
    Lane(String),
    /// D-INDEX-HOOK: `mytype[k]` when the type implements `Index` (+ optional `IndexMut`).
    User(String),
}

/// `for i in 1..10` vs `for x in xs` (M5).
#[derive(Debug, Clone)]
pub enum ForKind {
    /// S22 (D-SG8): `start..end` inclusive, with an optional `step n` stride.
    Range {
        start: Expr,
        end: Expr,
        step: Option<Expr>,
    },
    In {
        collection: Expr,
    },
}

#[derive(Debug, Clone)]
pub struct Binding {
    pub mutable: bool,
    pub name: String,
    pub name_span: Span,
    /// S74: when present, this binding destructures `init` instead of binding
    /// the single `name`. `name` is empty and `name_span` covers the pattern.
    pub pattern: Option<BindPattern>,
    pub ty: Option<Type>,
    pub ty_span: Option<Span>,
    pub init: Expr,
    /// S57 (M9.5): local `comptime NAME = expr;` — immutable, evaluated
    /// after ordinary type checking and emitted as literal data.
    pub is_comptime: bool,
    pub ct: Option<CtValue>,
    /// D-UNINIT-SENTINEL1 (ratified 2026-07-02, opt D; supersedes D-UNINIT1's
    /// `#Uninit name: Type` marker spelling): `name: Type := uninit` — an
    /// uninitialized binding, gated by `use core.mem`. `init` is a harmless
    /// placeholder (the `uninit` token's own span, never evaluated); sema
    /// proves write-before-read (E0420) and codegen lowers to `MaybeUninit`.
    /// `false` for every ordinary binding.
    pub uninit: bool,
    /// D-ALLOC2 (ratified 2026-06-21): set by sema when `init` is an
    /// `arena.alloc(value)` call, so this binding holds a scope-bound *view*
    /// into the arena's storage (Rust `&mut T`), not an owned `T`. Codegen
    /// binds it as a reference and dereferences reads; sema (E0631/E0632)
    /// forbids it escaping its arena's scope or outliving a `reset`/`free`.
    pub arena_view: bool,
}

#[derive(Debug, Clone)]
pub struct Call {
    pub name: String,
    pub name_span: Span,
    pub args: Vec<CallArg>,
    /// D-RANGETYPE1: sema sets this on a range-constrained distinct
    /// constructor when it appears under postfix `?`. Codegen then emits the
    /// checked constructor as a `Result`, while the ordinary constructor form
    /// still stays infallible and is rejected for runtime values.
    pub range_checked: bool,
}

#[derive(Debug, Default, Clone)]
pub struct CallArgFlags {
    pub implicit_clone: bool,
    pub shared_auto_clone: bool,
    /// D-TRAILBLOCK1: this argument is the desugared zero-parameter lambda
    /// from a trailing `{ }` block (`callee(args) { … }`). Sema reads this to
    /// give the specific E0334 teaching message instead of a generic
    /// argument-type mismatch when the parameter it lands in isn't a
    /// zero-parameter function.
    pub is_trailing_block: bool,
}

#[derive(Debug, Clone)]
pub struct CallArg {
    pub convention: AccessConvention,
    pub expr: Expr,
    pub span: Span,
    pub flags: CallArgFlags,
    /// S61: optional `name:` label at the call site. When present, sema checks
    /// that it matches the parameter name at this position.
    pub label: Option<(String, Span)>,
    /// D-VARIADIC1: `f(...xs)` — expand a list into the remaining parameter slots.
    pub spread: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
}

impl BinOp {
    pub fn is_comparison(self) -> bool {
        matches!(
            self,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge
        )
    }

    /// The user-typed spelling (for diagnostics and codegen).
    pub fn spell(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::BitAnd => "&",
            BinOp::BitOr => "|",
            BinOp::BitXor => "^",
            BinOp::Shl => "<<",
            BinOp::Shr => ">>",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Gt => ">",
            BinOp::Le => "<=",
            BinOp::Ge => ">=",
            BinOp::And => "&&",
            BinOp::Or => "||",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

/// D-INCR1: increment (`Inc`) or decrement (`Dec`) on a mutable integer lvalue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncDecOp {
    Inc,
    Dec,
}

/// D-DISPLAYDBG2: which protocol hook an interpolated value uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StrFormat {
    /// Bare `{value}` — calls `Display` (D-DISPLAY-SHAPE).
    #[default]
    Display,
    /// `{value@Debug}` — calls auto-derived or explicit `Debug`.
    Debug,
}

/// One piece of a string literal (S8): literal text or an interpolated
/// expression.
#[derive(Debug, Clone)]
pub enum StrPart {
    Lit(String),
    Interp(Box<Expr>, StrFormat),
}

/// S46 (M8): one parameter in `(x: Int) => …`.
#[derive(Debug, Clone)]
pub struct LambdaParam {
    pub name: String,
    pub name_span: Span,
    pub ty: Option<Type>,
    pub ty_span: Option<Span>,
}

/// S46: expression or block body after `=>`.
#[derive(Debug, Clone)]
pub enum LambdaBody {
    Expr(Box<Expr>),
    Block(Vec<Stmt>),
}

/// S47: filled by sema — capture/escape lowering hints for codegen.
#[derive(Debug, Clone, Default)]
pub struct LambdaMeta {
    pub escapes: bool,
    pub needs_fn_mut: bool,
    pub mut_captures: Vec<String>,
    pub cloned_captures: Vec<String>,
}

/// S46/S47 (M8): `(take names) (params) => body`.
#[derive(Debug, Clone)]
pub struct Lambda {
    pub take_names: Vec<(String, Span)>,
    pub params: Vec<LambdaParam>,
    pub body: LambdaBody,
    pub span: Span,
    pub meta: LambdaMeta,
}

#[derive(Debug, Clone)]
pub enum Expr {
    /// String literal, possibly with interpolation parts.
    Str(Vec<StrPart>, Span),
    /// D-SHIFT1 (c7shift): a pattern-literal call argument — the sole legal
    /// shape of `cursor.take_pattern("…")`'s argument. Same source syntax as
    /// `Str` (a string literal with `{hole}`/`{hole:Type}` interpolation
    /// holes) but parsed via the D-PARSESTR1 pattern engine (`StrMatchPart`)
    /// instead of ordinary `Expr::Str`, because a typed hole `{id:Int}` is
    /// not a legal interpolation value expression. Legal ONLY as a
    /// `take_pattern` call argument; sema rejects it anywhere else.
    StrMatchLit(Vec<StrMatchPart>, Span),
    /// Integer literal. The third field is the D-SG9 elaborated fixed width
    /// `(signed, bits)`, filled by sema when the literal sits in a sized-integer
    /// context; `None` means the default `Int` (i64). Codegen reads it to pick
    /// the Rust literal suffix.
    Int(i64, Span, Option<(bool, u8)>),
    /// D-FLOATW1: the bool is `true` when the literal is resolved as F32 in a
    /// typed context (e.g. `x: F32 = 1.5`). `false` = default F64/Float.
    Float(f64, Span, bool),
    Bool(bool, Span),
    /// S41: single-quoted `'a'`.
    Char(char, Span),
    /// S37: `[a, b, c]` or `[]`.
    ListLit(Vec<Expr>, Span),
    /// D-VARIADIC1: `...expr` inside a list literal — flatten the list's elements in place.
    Spread(Box<Expr>, Span),
    /// S38: `["k": v]` or `[:]`.
    MapLit(Vec<(Expr, Expr)>, Span),
    /// S39: `xs[i]` or `m[k]`.
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        span: Span,
        /// Filled by sema so codegen picks the right runtime helper.
        kind: IndexKind,
    },
    /// S40: inclusive copy slice `xs[a..b]`.
    Slice {
        base: Box<Expr>,
        start: Box<Expr>,
        end: Box<Expr>,
        span: Span,
    },
    Ident(String, Span),
    Call(Call),
    Unary(UnOp, Box<Expr>, Span),
    Binary(BinOp, Box<Expr>, Box<Expr>, Span),
    /// D-CHAINCMP1: a same-direction relational chain `0 <= sev < 10`, any
    /// length ≥ 2 pairs (`operands.len() == ops.len() + 1`). Only `<`/`<=`/
    /// `>`/`>=` chain; `==`/`!=` never appear here (they stay plain `Binary`,
    /// non-chainable). Each shared middle operand is evaluated exactly once —
    /// a lowering fact resolved by TIR (R1), not a parser/sema concern. A
    /// single relational pair stays plain `Binary` (this node only appears for
    /// chains of length ≥ 2 ops).
    CompareChain {
        operands: Vec<Expr>,
        ops: Vec<BinOp>,
        span: Span,
    },
    /// D-UNITLIT1: a numeric literal with a unit suffix — `500ms`, `12.50usd`.
    /// The lexer only carries the raw value + suffix text (imports aren't
    /// known to it); sema resolves `suffix` against an in-scope `#UnitFamily`
    /// member (PascalCased to its minted distinct-type name) and REWRITES
    /// this node in place to an ordinary distinct-type constructor call
    /// (`Ms(500.0)`) — sugar over the existing distinct-type path, not a new
    /// type or a new TIR/codegen shape (E0134 if the suffix isn't a member in
    /// scope).
    UnitLit {
        int: Option<i64>,
        float: Option<f64>,
        suffix: String,
        suffix_span: Span,
        span: Span,
    },
    /// D-CAP9: postfix `p.*` — dereference a raw pointer. Lowers to Rust `*p`;
    /// gated to `#Unsafe` (E0208). Composes with `.field` as `p.*.field`.
    Deref(Box<Expr>, Span),
    /// D-CAP9: prefix `*x` — take a raw pointer to `x` (raw-pointer-of). Legal
    /// only inside an `#Unsafe` region/fn (E0208). Lowers to `&x as *const _`
    /// inside the gated region.
    RawOf(Box<Expr>, Span),
    /// Field access: `v.field`.
    Field(Box<Expr>, String, Span),
    /// S71 (D-SG6): `base?.field` optional chaining. Yields a `T?` and
    /// short-circuits to absent when `base` is absent.
    OptField {
        base: Box<Expr>,
        member: String,
        member_span: Span,
        /// Filled by sema: true when the field type is itself optional, so
        /// codegen flattens (`and_then`) instead of wrapping (`map`).
        flatten: bool,
        span: Span,
    },
    /// Method call: `v.method(args)`.
    MethodCall {
        receiver: Box<Expr>,
        method: String,
        method_span: Span,
        /// D-SERDE6 (= C): call-site type arguments — `decode<Order>(text)`. Jet's
        /// first turbofish; empty for an ordinary call. Drives the typed encoding
        /// decoders and is available for any generic call going forward.
        type_args: Vec<Type>,
        args: Vec<CallArg>,
        /// Filled by sema when the method resolves to a user-defined type,
        /// so codegen can apply the parameter conventions (`&`/`&mut`).
        recv_type: Option<String>,
        /// Filled by sema (c109 Phase 20) with the call's resolved return type
        /// for the polymorphic core specials (`math.abs/min/max/clamp`,
        /// `random.pick/shuffle`, `io.eprint`) whose return type is arg-type
        /// dependent and not in `core_fixed_sig`. Total fact read by TIR
        /// lowering so codegen never re-infers it (I3). `None` for every other
        /// call shape (their type comes from a `cx` table or is unused).
        resolved_ret: Option<Type>,
    },
    /// D-DOTCTOR1 (ratified 2026-06-25): `Type.{ field: expr, ... }` (named) or
    /// `.{ field: expr, ... }` (inferred — type from context). Replaces the old
    /// dotless `Type { … }` form (E0320). Also: `Type<Args>.{ … }` and
    /// `alias.Type.{ … }` for generic / namespaced structs.
    StructLit {
        type_name: String,
        /// S45: generic args in `Pair<Int>.{ … }`.
        type_args: Vec<Type>,
        /// When set, the struct type lives in the imported module `alias`.
        import_ns: Option<String>,
        /// S48: box as `Box<dyn Trait>` when coerced into a trait-object list.
        as_trait: Option<String>,
        fields: Vec<(String, Span, Expr)>,
        /// `true` for the `.{ … }` inferred form (type resolved by sema from
        /// the expected-type context). `false` for the `Type.{ … }` named form.
        inferred: bool,
        span: Span,
    },
    /// S30: `Type.Variant(args)`.
    EnumLit {
        type_name: String,
        variant: String,
        args: Vec<EnumLitArg>,
        span: Span,
    },
    /// D-TAINT1 (ratified 2026-06-21): `#Tainted expr` — marks a value as
    /// untrusted at its source. A value-fact tag (D-QUAL1): it rides the value,
    /// taint spreads to anything derived from it, and a tainted value reaching a
    /// sink effect (`Db`/`Exec`/`Net`) without passing through a `#Sanitizer fn`
    /// is E0721. The tag is static and **erased in codegen** (I3) — lowering
    /// emits the inner expression unchanged, like `Expr::Present` but unwrapped.
    Tainted(Box<Expr>, Span),
    /// S32: `value(expr)` — present optional.
    Present(Box<Expr>, Span),
    /// S32: bare `null` — absent optional.
    Absent(Span),
    /// D-TOOL2 (E2-M11; D-CASING1): `#Todo` typed hole. Compiles anywhere; panics at
    /// runtime with file, line, and the expected type (filled in by sema).
    Todo {
        span: Span,
        /// The expected type, as a display string — filled by sema.
        expected_type: Option<String>,
    },
    /// D-SIMD2 (ratified 2026-06-24): a reduce-op marker `#Add`/`#Mul`/`#Min`/`#Max`,
    /// valid ONLY as the sole argument to a SIMD lane `.reduce(#Op)`. The string is
    /// the marker name (without `#`). Sema validates the marker set and that it sits
    /// in a `reduce` arg; codegen lowers it as part of the reduce call (the marker
    /// node never emits on its own).
    ReduceMarker(String, Span),
    /// S31: `subject == pattern` (stored as dedicated node for sema/codegen).
    PatternTest {
        subject: Box<Expr>,
        pattern: Pattern,
        span: Span,
    },
    /// S34: `ok(expr)` — success value for `T ? E`.
    Ok(Box<Expr>, Span),
    /// S34: `err(expr)` — failure value for `T ? E`.
    Err(Box<Expr>, Span),
    /// S7: postfix `?` — propagate a fallible value.
    /// S7/S80/D-ERR-CONV: `expr?` — propagates failure.
    /// `TryConvert` records how (if at all) the error type is converted.
    Try(Box<Expr>, Span, TryConvert),
    /// S35: `value or fallback`.
    OrFallback {
        value: Box<Expr>,
        fallback: OrFallback,
        /// Set during typechecking: `true` when the left side is `T?`.
        is_option: bool,
        span: Span,
    },
    /// S68 (D-SG2): `if` in expression position. Each branch is a block whose
    /// trailing expression (no `;`) is its value; the `else` is required and
    /// both branches share a type. `else if` nests as the else value.
    If {
        cond: Box<Expr>,
        then_body: Vec<Stmt>,
        then_value: Box<Expr>,
        else_body: Vec<Stmt>,
        else_value: Box<Expr>,
        span: Span,
    },
    /// S73 (D-SG7): `(x: 1, y: 2)` — named members only; source order preserved for fmt.
    /// `ty` is filled by sema for codegen (canonical sorted shape).
    TupleLit(Vec<(String, Expr)>, Span, Option<Type>),
    /// S46 (M8): `(params) => expr` or block body.
    Lambda(Lambda),
    /// S47: call any function-valued expression: `f(args)`.
    CallValue {
        callee: Box<Expr>,
        args: Vec<CallArg>,
        span: Span,
    },
    /// S58 (E2-M13): `mem.Ptr<T>.from_addr(addr)` — build a typed pointer from
    /// an integer address. The element type `elem` is the `<T>` argument; the
    /// result type is `Ptr<elem>`. Only legal inside an `@unsafe` region in a
    /// module that did `use core.mem` (else E3101/E3102).
    PtrFromAddr {
        /// The module alias the call came through (`mem` in the example).
        alias: String,
        alias_span: Span,
        elem: Type,
        addr: Box<Expr>,
        span: Span,
    },
    /// S75 (2026-06-16): `callee.[item0, item1, …]` — fan-out, desugars to
    /// `[callee(item0), callee(item1), …]`. Items are typed by `callee`'s
    /// parameter type (expected-type elaboration). Result type is `[T#N]` (S76).
    FanOut {
        callee: Box<Expr>,
        items: Vec<Expr>,
        span: Span,
    },
    /// D-CTMARKER1=C: `$name` — comptime splice expression. In a comptime
    /// context (derive body, `comptime {}` block, comptime binding RHS), looks
    /// up `name` in the comptime scope. Outside comptime context: E2712.
    /// Inside `emit("… $name …")` strings, `$name` is handled by
    /// `apply_dollar_splices` (string interpolation, not this AST node).
    ComptimeSplice {
        name: String,
        span: Span,
        value: Option<CtValue>,
    },
    /// D-FMTPARENS1=A: explicit author grouping parentheses `(expr)`.
    /// Transparent to type-checking and codegen; formatter always emits the parens.
    Paren(Box<Expr>, Span),
    /// D-INCR1: `++x`/`--x` (prefix) or `x++`/`x--` (postfix). Prefix returns the
    /// updated value; postfix returns the value before the update. Operand must be
    /// a mutable integer lvalue (same LHS policy as S17 compound assignment).
    IncDec {
        op: IncDecOp,
        operand: Box<Expr>,
        postfix: bool,
        span: Span,
    },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Str(_, s)
            | Expr::StrMatchLit(_, s)
            | Expr::Int(_, s, _)
            | Expr::Float(_, s, _)
            | Expr::Bool(_, s)
            | Expr::Char(_, s)
            | Expr::ListLit(_, s)
            | Expr::Spread(_, s)
            | Expr::TupleLit(_, s, _)
            | Expr::MapLit(_, s)
            | Expr::Index { span: s, .. }
            | Expr::Slice { span: s, .. }
            | Expr::Ident(_, s)
            | Expr::Unary(_, _, s)
            | Expr::Binary(_, _, _, s)
            | Expr::Deref(_, s)
            | Expr::RawOf(_, s)
            | Expr::Field(_, _, s)
            | Expr::OptField { span: s, .. }
            | Expr::StructLit { span: s, .. }
            | Expr::EnumLit { span: s, .. }
            | Expr::Tainted(_, s)
            | Expr::Present(_, s)
            | Expr::Absent(s)
            | Expr::Todo { span: s, .. }
            | Expr::ReduceMarker(_, s)
            | Expr::Ok(_, s)
            | Expr::Err(_, s)
            | Expr::Try(_, s, _)
            | Expr::OrFallback { span: s, .. }
            | Expr::PatternTest { span: s, .. }
            | Expr::If { span: s, .. }
            | Expr::CallValue { span: s, .. }
            | Expr::FanOut { span: s, .. }
            | Expr::PtrFromAddr { span: s, .. }
            | Expr::ComptimeSplice { span: s, .. }
            | Expr::CompareChain { span: s, .. }
            | Expr::UnitLit { span: s, .. }
            | Expr::IncDec { span: s, .. } => *s,
            Expr::Paren(_, s) => *s,
            Expr::Lambda(l) => l.span,
            Expr::Call(c) => c.name_span,
            Expr::MethodCall { method_span, .. } => *method_span,
        }
    }
}

impl Func {
    /// S27: first parameter named `self`.
    pub fn self_param(&self) -> Option<&Param> {
        self.params.first().filter(|p| p.name == Syntax::KW_SELF)
    }

    pub fn is_static_method(&self) -> bool {
        self.self_param().is_none()
    }
}

/// Semantic signature of a function — the compiler's internal view after
/// registration. Lives in `AST` so that `Traits`, `Codegen`, and `Sema` can
/// all depend on it without creating cycles.
#[derive(Debug, Clone)]
pub struct FuncSig {
    pub params: Vec<(AccessConvention, Type)>,
    pub return_type: Option<Type>,
    pub is_view_return: bool,
    /// S50: declared in `extern rust`, implemented by the FFI bridge.
    pub is_extern: bool,
    /// S58 (E2-M13): `@unsafe fn` — calling it requires an enclosing `@unsafe`
    /// block (E3103).
    pub is_unsafe: bool,
    /// S60 (E2-M16): `pure fn` — this function is free of ambient I/O and
    /// non-determinism. Call sites inside a `pure fn` must also be pure (E3401).
    pub is_pure: bool,
    /// D-TAINT1: `#Sanitizer fn` — its return value is untainted by contract.
    pub is_sanitizer: bool,
    /// D-MUSTUSE1 (c18iwxqx): `@MustUse fn` / method — return value cannot be
    /// silently ignored as a bare expression statement (E0419).
    pub is_must_use: bool,
    /// S61: parameter names and default-value presence, parallel to `params`.
    /// Empty for extern/built-in functions.
    pub param_info: Vec<(String, bool)>,
    /// S61: default expressions for parameters that have them, parallel to `params`.
    pub defaults: Vec<Option<Expr>>,
    /// D-VARIADIC1: parallel to `params` — true when that parameter is variadic.
    pub param_variadic: Vec<bool>,
    /// D-ANY-JAI1/D-VARARGBOUND1 (c7jaiany): the trailing variadic parameter's
    /// resolved trait-bound list (`Param::variadic_trait_bounds`), or `None` for
    /// a non-variadic function or a plain D-VARIADIC1 homogeneous-concrete-type
    /// variadic. Call-site checking (E1313) and codegen's per-arity
    /// monomorphization both key off this.
    pub variadic_bounds: Option<Vec<String>>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared data types — placed in AST so every seam crate can depend on them
// without creating cross-seam dep cycles.
// ─────────────────────────────────────────────────────────────────────────────

// ── CtValue / CtKey ──────────────────────────────────────────────────────────

/// A fully-evaluated compile-time value.
#[derive(Clone, Debug, PartialEq)]
pub enum CtValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Char(char),
    Str(String),
    /// `[U8]` byte buffer (D-CTIO1 `embed_bytes`).
    Bytes(Vec<u8>),
    List(Vec<CtValue>),
    Map(BTreeMap<CtKey, CtValue>),
    Struct {
        type_name: String,
        fields: Vec<(String, CtValue)>,
    },
    Enum {
        type_name: String,
        variant: String,
        args: Vec<(Option<String>, CtValue)>,
    },
    Some(Box<CtValue>),
    None(Type),
    ResOk(Box<CtValue>),
    ResErr(Box<CtValue>),
    Unit,
}

/// Orderable map key (S38: maps are `BTreeMap`, so keys must be `Ord`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CtKey {
    Int(i64),
    Str(String),
    Bool(bool),
    Char(char),
}

impl CtKey {
    pub fn from_value(v: CtValue) -> Option<CtKey> {
        match v {
            CtValue::Int(n) => Some(CtKey::Int(n)),
            CtValue::Str(s) => Some(CtKey::Str(s)),
            CtValue::Bool(b) => Some(CtKey::Bool(b)),
            CtValue::Char(c) => Some(CtKey::Char(c)),
            _ => None,
        }
    }
    pub fn to_value(&self) -> CtValue {
        match self {
            CtKey::Int(n) => CtValue::Int(*n),
            CtKey::Str(s) => CtValue::Str(s.clone()),
            CtKey::Bool(b) => CtValue::Bool(*b),
            CtKey::Char(c) => CtValue::Char(*c),
        }
    }
    pub(crate) fn jet_type(&self) -> Type {
        match self {
            CtKey::Int(_) => Type::Int,
            CtKey::Str(_) => Type::String,
            CtKey::Bool(_) => Type::Bool,
            CtKey::Char(_) => Type::Char,
        }
    }
    pub(crate) fn jet_show(&self) -> String {
        self.to_value().jet_show()
    }
}

impl CtValue {
    pub fn jet_type(&self) -> Type {
        match self {
            CtValue::Int(_) => Type::Int,
            CtValue::Float(_) => Type::Float,
            CtValue::Bool(_) => Type::Bool,
            CtValue::Char(_) => Type::Char,
            CtValue::Str(_) => Type::String,
            CtValue::Bytes(_) => Type::List(Box::new(Type::IntN {
                signed: false,
                bits: 8,
            })),
            CtValue::List(xs) => {
                let inner = xs.first().map(|x| x.jet_type()).unwrap_or(Type::Int);
                Type::List(Box::new(inner))
            }
            CtValue::Map(m) => {
                let (k, v) = m
                    .iter()
                    .next()
                    .map(|(k, v)| (k.jet_type(), v.jet_type()))
                    .unwrap_or((Type::String, Type::Int));
                Type::Map {
                    key: Box::new(k),
                    value: Box::new(v),
                }
            }
            CtValue::Some(inner) => Type::Option(Box::new(inner.jet_type())),
            CtValue::None(t) => Type::Option(Box::new(t.clone())),
            CtValue::ResOk(inner) => Type::Result {
                ok: Box::new(inner.jet_type()),
                err: Box::new(Type::Named("ParseError".to_string())),
            },
            CtValue::ResErr(e) => Type::Result {
                ok: Box::new(Type::Int),
                err: Box::new(e.jet_type()),
            },
            CtValue::Struct { type_name, .. } | CtValue::Enum { type_name, .. } => {
                Type::Named(type_name.clone())
            }
            CtValue::Unit => Type::Named(String::new()),
        }
    }

    pub fn jet_show(&self) -> String {
        match self {
            CtValue::Int(n) => n.to_string(),
            CtValue::Float(f) => format!("{:?}", f),
            CtValue::Bool(b) => b.to_string(),
            CtValue::Char(c) => c.to_string(),
            CtValue::Str(s) => s.clone(),
            CtValue::Bytes(bs) => {
                let parts: Vec<String> = bs.iter().map(|b| b.to_string()).collect();
                format!("[{}]", parts.join(", "))
            }
            CtValue::List(xs) => {
                let parts: Vec<String> = xs.iter().map(|x| x.jet_show()).collect();
                format!("[{}]", parts.join(", "))
            }
            CtValue::Map(m) => {
                let parts: Vec<String> = m
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k.jet_show(), v.jet_show()))
                    .collect();
                format!("[{}]", parts.join(", "))
            }
            CtValue::Some(v) => v.jet_show(),
            CtValue::None(_) => "null".to_string(),
            CtValue::ResOk(v) => v.jet_show(),
            CtValue::ResErr(_) => "err".to_string(),
            CtValue::Struct { type_name, fields } => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(n, v)| format!("{}: {}", n, v.jet_show()))
                    .collect();
                format!("{}({})", type_name, parts.join(", "))
            }
            CtValue::Enum { variant, .. } => variant.clone(),
            CtValue::Unit => String::new(),
        }
    }

    pub fn render_pretty(&self) -> String {
        let mut out = String::new();
        self.render_pretty_inner(&mut out, 0);
        out
    }

    fn render_pretty_inner(&self, out: &mut String, depth: usize) {
        let indent = "  ".repeat(depth);
        let inner_indent = "  ".repeat(depth + 1);
        match self {
            CtValue::Int(n) => out.push_str(&n.to_string()),
            CtValue::Float(f) => out.push_str(&format!("{:?}", f)),
            CtValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            CtValue::Char(c) => {
                out.push('\'');
                out.push(*c);
                out.push('\'');
            }
            CtValue::Str(s) => {
                out.push('"');
                out.push_str(s);
                out.push('"');
            }
            CtValue::Bytes(bs) => {
                let parts: Vec<String> = bs.iter().map(|b| b.to_string()).collect();
                out.push('[');
                out.push_str(&parts.join(", "));
                out.push(']');
            }
            CtValue::List(xs) => {
                if xs.is_empty() {
                    out.push_str("[]");
                } else {
                    out.push_str("[\n");
                    for item in xs {
                        out.push_str(&inner_indent);
                        item.render_pretty_inner(out, depth + 1);
                        out.push_str(",\n");
                    }
                    out.push_str(&indent);
                    out.push(']');
                }
            }
            CtValue::Map(m) => {
                if m.is_empty() {
                    out.push_str("{}");
                } else {
                    out.push_str("{\n");
                    for (k, v) in m {
                        out.push_str(&inner_indent);
                        out.push_str(&k.jet_show());
                        out.push_str(": ");
                        v.render_pretty_inner(out, depth + 1);
                        out.push_str(",\n");
                    }
                    out.push_str(&indent);
                    out.push('}');
                }
            }
            CtValue::Struct { type_name, fields } => {
                if fields.is_empty() {
                    out.push_str(type_name);
                    out.push_str(" {}");
                } else {
                    out.push_str(type_name);
                    out.push_str(" {\n");
                    for (name, value) in fields {
                        out.push_str(&inner_indent);
                        out.push_str(name);
                        out.push_str(": ");
                        value.render_pretty_inner(out, depth + 1);
                        out.push_str(",\n");
                    }
                    out.push_str(&indent);
                    out.push('}');
                }
            }
            CtValue::Enum {
                type_name,
                variant,
                args,
            } => {
                out.push_str(type_name);
                out.push_str("::");
                out.push_str(variant);
                if !args.is_empty() {
                    out.push('(');
                    let mut first = true;
                    for (label, v) in args {
                        if !first {
                            out.push_str(", ");
                        }
                        first = false;
                        if let Some(lbl) = label {
                            out.push_str(lbl);
                            out.push_str(": ");
                        }
                        v.render_pretty_inner(out, depth);
                    }
                    out.push(')');
                }
            }
            CtValue::Some(v) => {
                out.push_str("Some(");
                v.render_pretty_inner(out, depth);
                out.push(')');
            }
            CtValue::None(_) => out.push_str("None"),
            CtValue::ResOk(v) => {
                out.push_str("ok(");
                v.render_pretty_inner(out, depth);
                out.push(')');
            }
            CtValue::ResErr(e) => {
                out.push_str("err(");
                e.render_pretty_inner(out, depth);
                out.push(')');
            }
            CtValue::Unit => out.push_str("()"),
        }
    }

    pub fn to_json(&self) -> String {
        match self {
            CtValue::Int(n) => n.to_string(),
            CtValue::Float(f) => {
                let s = format!("{:?}", f);
                if s.contains('.') || s.contains('e') || s.contains('E') {
                    s
                } else {
                    format!("{}.0", s)
                }
            }
            CtValue::Bool(b) => b.to_string(),
            CtValue::Char(c) => format!("\"{}\"", c),
            CtValue::Str(s) => {
                let mut out = String::from('"');
                for ch in s.chars() {
                    match ch {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        c => out.push(c),
                    }
                }
                out.push('"');
                out
            }
            CtValue::Bytes(bs) => {
                let parts: Vec<String> = bs.iter().map(|b| b.to_string()).collect();
                format!("[{}]", parts.join(","))
            }
            CtValue::List(xs) => {
                let parts: Vec<String> = xs.iter().map(|x| x.to_json()).collect();
                format!("[{}]", parts.join(","))
            }
            CtValue::Map(m) => {
                let parts: Vec<String> = m
                    .iter()
                    .map(|(k, v)| format!("{}:{}", k.to_value().to_json(), v.to_json()))
                    .collect();
                format!("{{{}}}", parts.join(","))
            }
            CtValue::Struct { fields, .. } => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(n, v)| format!("\"{}\":{}", n, v.to_json()))
                    .collect();
                format!("{{{}}}", parts.join(","))
            }
            CtValue::Enum { variant, args, .. } => {
                if args.is_empty() {
                    format!("\"{}\"", variant)
                } else {
                    let parts: Vec<String> = args
                        .iter()
                        .map(|(label, v)| {
                            if let Some(lbl) = label {
                                format!("\"{}\":{}", lbl, v.to_json())
                            } else {
                                v.to_json()
                            }
                        })
                        .collect();
                    if args.iter().all(|(label, _)| label.is_some()) {
                        format!("{{\"{}\":{{{}}}}}", variant, parts.join(","))
                    } else {
                        format!("{{\"{}\":[{}]}}", variant, parts.join(","))
                    }
                }
            }
            CtValue::Some(v) => v.to_json(),
            CtValue::None(_) => "null".to_string(),
            CtValue::ResOk(v) => format!("{{\"ok\":{}}}", v.to_json()),
            CtValue::ResErr(e) => format!("{{\"err\":{}}}", e.to_json()),
            CtValue::Unit => "null".to_string(),
        }
    }

    pub fn serialize(&self) -> String {
        match self {
            CtValue::Int(n) => format!("{}i64", n),
            CtValue::Float(f) => format!("{:?}f64", f),
            CtValue::Bool(b) => b.to_string(),
            CtValue::Char(c) => format!("{:?}", c),
            CtValue::Str(s) => format!("{:?}.to_string()", s),
            CtValue::Bytes(bs) => {
                let parts: Vec<String> = bs.iter().map(|b| format!("{}u8", b)).collect();
                format!("vec![{}]", parts.join(", "))
            }
            CtValue::List(xs) => {
                let parts: Vec<String> = xs.iter().map(|x| x.serialize()).collect();
                format!("vec![{}]", parts.join(", "))
            }
            CtValue::Map(m) => {
                if m.is_empty() {
                    "std::collections::BTreeMap::new()".to_string()
                } else {
                    let mut s = String::from("{ let mut _m = std::collections::BTreeMap::new(); ");
                    for (k, v) in m {
                        s.push_str(&format!(
                            "_m.insert(({}), {}); ",
                            k.to_value().serialize(),
                            v.serialize()
                        ));
                    }
                    s.push_str("_m }");
                    s
                }
            }
            CtValue::Some(v) => format!("Some({})", v.serialize()),
            CtValue::None(_) => "None".to_string(),
            CtValue::ResOk(v) => format!("Ok({})", v.serialize()),
            CtValue::ResErr(e) => format!("Err({})", e.serialize()),
            CtValue::Struct { type_name, fields } => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(n, v)| format!("{}: {}", ct_mangle(n), v.serialize()))
                    .collect();
                format!("user_{} {{ {} }}", type_name, parts.join(", "))
            }
            CtValue::Enum {
                type_name,
                variant,
                args,
            } => {
                let prefix = format!("user_{}::{}", type_name, ct_mangle(variant));
                if args.is_empty() {
                    prefix
                } else if args.iter().all(|(label, _)| label.is_none()) {
                    let parts: Vec<String> = args.iter().map(|(_, v)| v.serialize()).collect();
                    format!("{}({})", prefix, parts.join(", "))
                } else {
                    let parts: Vec<String> = args
                        .iter()
                        .filter_map(|(label, v)| {
                            label
                                .as_ref()
                                .map(|name| format!("{}: {}", ct_mangle(name), v.serialize()))
                        })
                        .collect();
                    format!("{} {{ {} }}", prefix, parts.join(", "))
                }
            }
            CtValue::Unit => "()".to_string(),
        }
    }
}

fn ct_mangle(name: &str) -> String {
    if name == "main" {
        "main".to_string()
    } else {
        format!("user_{}", name)
    }
}

// ── C-FFI data types ──────────────────────────────────────────────────────────

/// The result of resolving one C `use` in one file.
#[derive(Debug, Clone)]
pub struct CImportLink {
    pub importing_idx: usize,
    pub alias: String,
    pub target_idx: usize,
}

/// One C library that the program links against.
#[derive(Debug, Clone)]
pub struct CLib {
    pub lib: String,
    pub module_idx: usize,
}

/// Gathered C-FFI artifacts threaded into sema and codegen.
#[derive(Debug, Default, Clone)]
pub struct CFfi {
    pub import_links: Vec<CImportLink>,
    pub libs: Vec<CLib>,
}

impl CFfi {
    pub fn target_for(&self, importing_idx: usize, alias: &str) -> Option<usize> {
        self.import_links
            .iter()
            .find(|l| l.importing_idx == importing_idx && l.alias == alias)
            .map(|l| l.target_idx)
    }

    pub fn links_c(&self) -> bool {
        !self.libs.is_empty()
    }
}

// ── Comptime embed input ──────────────────────────────────────────────────────

/// D-CTEFFECT1 (Tier-1): one comptime embed input recorded for reproducibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComptimeInput {
    pub path: String,
    pub hash: String,
}

// ── Rust FFI link artifact ────────────────────────────────────────────────────

/// Built FFI bridge artifact paths for rustc linking (M7).
#[derive(Debug, Clone)]
pub struct FfiLink {
    pub crate_name: String,
    pub rlib_path: PathBuf,
    pub deps_dir: PathBuf,
}
