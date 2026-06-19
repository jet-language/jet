//! AST nodes. Grows with each milestone; keep nodes small and keep spans on
//! anything an error might need to point at.

use crate::Diagnostics::Span;
use crate::Syntax;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessConvention {
    /// Default: shared read borrow (`&T` in Rust; scalars pass by value).
    Read,
    /// Mutable borrow (`&mut T`).
    Mutate,
    /// Ownership transfer (`T` by value).
    Move,
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
        matches!(self, Type::Int | Type::Float | Type::Bool)
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
    /// M10: std helper names proven reachable by sema. Codegen emits only
    /// these helpers (SL9).
    pub used_std: std::collections::HashSet<String>,
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

#[derive(Debug)]
pub enum Item {
    Func(Func),
    Struct(StructDef),
    Enum(EnumDef),
    /// S28 (M9): `trait Name { fn sig(self) -> T; … }`.
    Trait(TraitDef),
    Impl(ImplDef),
    Const(ConstDef),
    /// S43 (M6): `test "name" { … }` — only at file top level.
    Test(TestDef),
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
}

#[derive(Debug)]
pub struct Variant {
    pub name: String,
    pub name_span: Span,
    pub payload: VariantPayload,
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
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Variant {
        variant: String,
        bindings: Vec<String>,
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
            | Pattern::Err { span, .. } => *span,
            Pattern::Absent(span) => *span,
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
    Int(i64, Span),
    Float(f64, Span),
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
    Deref(Box<Expr>, Span),
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
        args: Vec<CallArg>,
        /// Filled by sema when the method resolves to a user-defined type,
        /// so codegen can apply the parameter conventions (`&`/`&mut`).
        recv_type: Option<String>,
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
    /// S32: `value(expr)` — present optional.
    Present(Box<Expr>, Span),
    /// S32: bare `null` — absent optional.
    Absent(Span),
    /// D-TOOL2 (E2-M11): `todo` typed hole. Compiles anywhere; panics at
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
    /// S7/S80: `expr?` — propagates failure. When `via_fallible` is true, the
    /// error type implements `Fallible` and codegen must call `.to_error()`.
    Try(Box<Expr>, Span, bool /* via_fallible */),
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
            | Expr::Int(_, s)
            | Expr::Float(_, s)
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
            | Expr::Field(_, _, s)
            | Expr::OptField { span: s, .. }
            | Expr::StructLit { span: s, .. }
            | Expr::EnumLit { span: s, .. }
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
