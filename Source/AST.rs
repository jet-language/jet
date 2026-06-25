//! AST nodes. Grows with each milestone; keep nodes small and keep spans on
//! anything an error might need to point at.

use crate::Diagnostics::Span;
use crate::Syntax;

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

#[derive(Debug, Clone, PartialEq, Eq)]
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
    Fn {
        params: Vec<Type>,
        ret: Option<Box<Type>>,
    },
    /// User-defined monomorphic type name.
    Named(String),
    /// S45 (M9): generic application — `Pair<Int>`, `Stack<T>`.
    Apply {
        name: String,
        args: Vec<Type>,
    },
    /// S48 (M9): trait object — dynamic dispatch with invisible boxing.
    TraitObject(String),
    /// S73 (D-SG7): named tuple `(x: Int, y: Int)` — fields stored sorted by name.
    Tuple(Vec<(String, Box<Type>)>),
    /// S76 (2026-06-16): fixed-size list `[T#N]` — a compile-time refinement of
    /// `[T]` with a statically-known length. Erases to `Vec<T>` at codegen (I3).
    FixedList { elem: Box<Type>, len: u64 },
    /// D-SG9/S42: explicit fixed-width integer. The default 64-bit *signed*
    /// integer is spelled `Int` (and equivalently `I64`) and lives in
    /// `Type::Int`, so it never appears here — `I64` canonicalises to
    /// `Type::Int` at parse time. Every other width is an `IntN`: `bits` ∈
    /// {8,16,32,64}, and `(signed: true, bits: 64)` is excluded by construction
    /// because that *is* `Int`. So `U8` = `{signed:false, bits:8}`,
    /// `U64` = `{signed:false, bits:64}`, `I32` = `{signed:true, bits:32}`.
    IntN { signed: bool, bits: u8 },
    /// D-SG9/S42: 32-bit float. The default 64-bit float is spelled `Float`
    /// (and `F64`) and lives in `Type::Float`; only `F32` is a `Float32`.
    Float32,
}

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
                64 if !signed => Some(Type::IntN { signed: false, bits: 64 }),
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
            Type::Fn { params, ret } => {
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
            Type::TraitObject(t) => format!("`{}` (a trait value)", t),
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
            Type::Fn { params, ret } => {
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
            Type::TraitObject(t) => t.clone(),
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
        }
    }

    /// Base name for struct/enum/trait references (without generic args).
    pub fn base_name(&self) -> Option<&str> {
        match self {
            Type::Named(n) => Some(n.as_str()),
            Type::Apply { name, .. } => Some(name.as_str()),
            Type::TraitObject(t) => Some(t.as_str()),
            _ => None,
        }
    }

    pub fn is_scalar(&self) -> bool {
        matches!(
            self,
            Type::Int | Type::Float | Type::Bool | Type::IntN { .. } | Type::Float32
        )
    }

    /// D-SG9: any integer type — the default `Int` or an explicit fixed width.
    pub fn is_integer(&self) -> bool {
        matches!(self, Type::Int | Type::IntN { .. })
    }

    /// D-SG9/D-FLOATW1: any float type — the default `Float` or `F32`.
    pub fn is_float(&self) -> bool {
        matches!(self, Type::Float | Type::Float32)
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
}

#[derive(Debug, Clone)]
pub enum ImportKind {
    /// Quoted path relative to this file's directory (no `.jet` suffix).
    File(String, Span),
    /// Bare module name — searched from the project root.
    Module(String, Span),
    /// D-MOD3/4: `use alias.Item` / `use alias.{A, B}` / `pub use alias.Item`
    Unqualified {
        module_alias: String,
        module_alias_span: Span,
        items: Vec<String>,
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
    pub cffi: crate::CFFI::CFfi,
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
    Remove {
        field: String,
        field_span: Span,
    },
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
    /// D-DIST1 (ratified 2026-06-19): `UserId @= distinct Int` — a distinct type
    /// declaration. `distinct`-over-`distinct` base is rejected in sema.
    Distinct(DistinctDef),
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
    /// S59 (E2-M14): `@extern module c.<lib> { … }` (user overlay) or
    /// `@bindgen module c.<lib>.__bindgen__ { … }` (compiler-generated cache).
    CModule(CModule),
    /// D-MOD1/2 (code module system): `module name;` (file declaration) or
    /// `module name { … }` (inline body). `body = None` means the items live in
    /// a separate file found by the loader. NOT a JetOS module (see `ModuleDecl`).
    CodeModule(CodeModule),
    /// D-ERR-CONV (ratified 2026-06-19): `impl Source -> Target { … }` — typed
    /// error conversion; `?` applies it when propagating Source into a Target context.
    ErrorConv(ErrorConvDef),
    /// D-MIGRATE1 (ratified 2026-06-22): `migration TypeName { rename a -> b }`
    /// block — declares field renames on a `#PublishedSchema` struct.
    Migration(MigrationDecl),
}

/// D-MOD1/2: code module — `module math;` or `module math { pub fn … }`.
#[derive(Debug)]
pub struct CodeModule {
    pub name: String,
    pub name_span: Span,
    pub is_pub: bool,
    /// None = file declaration (`module math;`), Some = inline body.
    pub body: Option<Vec<Item>>,
    pub span: Span,
}

/// S59 (E2-M14): which attribute introduced a C FFI module — the user-written
/// overlay (`@extern`) or the generated cache surface (`@bindgen`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CModuleKind {
    /// `@extern module c.<lib> { … }` — user overlay, allowed anywhere.
    Extern,
    /// `@bindgen module c.<lib>.__bindgen__ { … }` — generated, cache files only.
    Bindgen,
}

/// S59 (E2-M14): one `@extern`/`@bindgen module c.<lib>[.__bindgen__] { … }` block.
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
    Platform { os: String, arch: String, span: Span },
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
    Platform { os: String, arch: String, span: Span },
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
    pub name: String,
    pub name_span: Span,
    /// Methods erroneously written in a tag body. Always empty for a well-formed
    /// tag; each entry triggers E0732 in sema.
    pub methods: Vec<TraitMethodSig>,
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
    /// D-EFF3: `#Pure fn hash(self)` — the method declares the empty effect set
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
    pub body: Vec<Stmt>,
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
}

/// D-REPRC1 (ratified; D-REPRC1 = B): the variant of `#layout(…)` on a struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructLayout {
    /// `#layout(c)` → `#[repr(C)]` on the generated Rust struct.
    C,
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
    /// D-MIGRATE1 (ratified 2026-06-22): `#PublishedSchema` marker was present
    /// before `struct`. The span is retained for pointing at the annotation in E0910.
    pub is_published_schema: bool,
    pub published_schema_span: Option<Span>,
    /// D-LIN1 (ratified 2026-06-21): `#SingleUse` marker before `struct` — values
    /// of this type must be consumed exactly once on every path (E0140/E0141)
    /// and may not be aliased (E0142). Implies `#NoCopy`. The span points at the
    /// marker for diagnostics.
    pub is_single_use: bool,
    pub single_use_span: Option<Span>,
    /// D-REPRC1 (ratified; D-REPRC1 = B): `#layout(…)` attribute. `None` = default layout.
    pub layout: Option<StructLayout>,
    pub layout_span: Option<Span>,
    /// D-SERDE3/8: container-level serde attribute markers (`RenameAll`,
    /// `DenyUnknownFields`) attached before the `struct`. Empty when none.
    pub serde_markers: Vec<Marker>,
}

/// D-DIST1/D-DIST3: distinct type declaration — `[#Numeric] Name @= distinct Base`.
#[derive(Debug)]
pub struct DistinctDef {
    pub is_pub: bool,
    /// D-DIST3: whether `#Numeric` marker was present — enables same-type arithmetic.
    pub is_numeric: bool,
    pub name: String,
    pub name_span: Span,
    pub base: Type,
    pub base_span: Span,
    pub span: Span,
}

#[derive(Debug)]
pub struct EnumDef {
    pub is_pub: bool,
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
    /// D-SERDE3/7/8: container-level serde markers (`RenameAll`, `Tag`,
    /// `Untagged`, `DenyUnknownFields`) attached before the `enum`. Empty when none.
    pub serde_markers: Vec<Marker>,
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

#[derive(Debug)]
pub struct Field {
    /// S18: visible to other files via `import` when true.
    pub is_pub: bool,
    pub is_stored_ref: bool,
    pub stored_ref_label: Option<String>,
    pub name: String,
    pub name_span: Span,
    pub ty: Type,
    pub ty_span: Span,
    /// D-SERDE5: per-field serde markers (`Rename`/`Skip`/`Default`/`Flatten`)
    /// attached before this field. Empty when none.
    pub serde_markers: Vec<Marker>,
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
        if let PatSlot::Bind(s) = self { Some(s) } else { None }
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
    Range { lo: i64, hi: i64, span: Span },
    /// D-PATO (ratified 2026-06-19): structural or-pattern `A(x) | B(x)`.
    /// All alternatives must bind the same names at the same types (E0317).
    Or(Vec<Pattern>, Span),
}

/// S74: a single name bound by a destructuring target.
#[derive(Debug, Clone)]
pub struct BindName {
    pub name: String,
    pub span: Span,
}

/// S74: the destructuring target on the left of a `val`/`var` binding.
/// Reuses the existing bracket conventions — `Type { fields }` for structs,
/// `[ elems ]` for lists, `( a, b )` for named tuples (S73/S74).
#[derive(Debug, Clone)]
pub enum BindPattern {
    /// `val Point { x, y } = p;` — binds a subset of the struct's fields.
    Struct {
        type_name: String,
        type_span: Span,
        fields: Vec<BindName>,
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

/// S35: right-hand side of `expr or …`.
#[derive(Debug, Clone)]
pub enum OrFallback {
    Value(Box<Expr>),
    Return(Option<Box<Expr>>, Span),
    Panic { name_span: Span, args: Vec<CallArg> },
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
    pub ct: Option<crate::Comptime::CtValue>,
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
    /// S58 (E2-M13): `@unsafe { … }` audited region. `audit` carries the
    /// `@audit("…")` reason on the line above, when present (lint L3101 fires
    /// when it is `None`). `body` is the gated statements.
    Unsafe {
        audit: Option<String>,
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
    /// D-CTX1 (ratified 2026-06-22, G2): `#Context(field: value, …) { … }`.
    /// Swaps named ambient fields for the block's lexical+dynamic extent, then
    /// restores them on all exit paths (return, break, ?, panic unwind) via
    /// a RAII guard. Expert-tier; never surfaced in beginner diagnostics.
    /// v1 fields: `allocator` (allocator handle), `logger` (logger handle).
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndexKind {
    #[default]
    Unknown,
    List,
    Map,
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
    In { collection: Expr },
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
    pub ct: Option<crate::Comptime::CtValue>,
    /// D-UNINIT1 (ratified 2026-06-21, opt C): `#Uninit name: Type` — an
    /// uninitialized binding (no `:=`/`::` initializer), gated by `use core.mem`.
    /// `init` is a harmless placeholder (never evaluated); sema proves write-before-read (E0420)
    /// and codegen lowers to `MaybeUninit`. `false` for every ordinary binding.
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
}

#[derive(Debug, Default, Clone)]
pub struct CallArgFlags {
    pub implicit_clone: bool,
    pub shared_auto_clone: bool,
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

/// One piece of a string literal (S8): literal text or an interpolated
/// expression.
#[derive(Debug, Clone)]
pub enum StrPart {
    Lit(String),
    Interp(Expr),
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
    /// S29: `Type { field: expr, ... }` or `Type<Args> { ... }` or `alias.Type { ... }`.
    StructLit {
        type_name: String,
        /// S45: generic args in `Pair<Int> { … }`.
        type_args: Vec<Type>,
        /// When set, the struct type lives in the imported module `alias`.
        import_ns: Option<String>,
        /// S48: box as `Box<dyn Trait>` when coerced into a trait-object list.
        as_trait: Option<String>,
        fields: Vec<(String, Span, Expr)>,
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
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Str(_, s)
            | Expr::Int(_, s, _)
            | Expr::Float(_, s, _)
            | Expr::Bool(_, s)
            | Expr::Char(_, s)
            | Expr::ListLit(_, s)
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
            | Expr::Ok(_, s)
            | Expr::Err(_, s)
            | Expr::Try(_, s, _)
            | Expr::OrFallback { span: s, .. }
            | Expr::PatternTest { span: s, .. }
            | Expr::If { span: s, .. }
            | Expr::CallValue { span: s, .. }
            | Expr::FanOut { span: s, .. }
            | Expr::PtrFromAddr { span: s, .. } => *s,
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
