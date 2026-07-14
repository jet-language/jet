use super::{CFfi, ComptimeInput, Expr, Item, Stmt, Type};
use crate::{Diagnostics::Span, Syntax};

#[derive(Debug)]
pub struct Program {
    /// S16 (M6): `import` declarations at the top of this file.
    pub imports: Vec<ImportDecl>,
    pub items: Vec<Item>,
    /// D-WASM1 (c123): optional file-level web bucket ceiling (`js target;` / `wasm target;`).
    pub web_target_ceiling: Option<crate::WebPartition::WebBucket>,
    /// D-VISDEFAULT1=C / D-VISDEFAULT2=A: `#PubFile` flips default top-level export visibility.
    pub pub_file: bool,
    /// D-PRELUDEX1=A: `#NoPrelude` disables ambient `print`/`input` in this file.
    pub no_prelude: bool,
    /// D-WEBDEFAULT1 (ratified 2026-07-01, c134): `#Target(Web)` — this file's default CLI
    /// backend is the web target, so `jet run`/`jet dev`/`jet build` don't
    /// need `--target=web` on every invocation. `None` means the native
    /// default applies unless `pkg.jet` or an explicit `--target=` flag says
    /// otherwise. Distinct from `web_target_ceiling` (`Wasm`/`Js`, a partition
    /// ceiling *within* a web build) — `Web` here means "build for the web
    /// backend at all," a different axis, same marker family (I8).
    pub default_target: Option<String>,
    /// D-HTMLPAIR1 (ratified 2026-07-01, c134): `#Html("path.html")` — this program's
    /// companion host page for `--target=web` builds, explicit instead of
    /// the silent `<stem>.html` sibling-filename convention. Relative to the
    /// `.jet` source file's own directory.
    pub html_path: Option<String>,
    /// D-MEM1/S7 / D-POLICY-WORD1: `#Policy(no_alloc)` —
    /// this file's allocation floor. `Some(span)` = the policy line's span
    /// (for a "declared twice" check); `None` = no policy. Local-only: sema
    /// checks only expressions written directly in this file's own function
    /// bodies, never calls into other modules (E0921).
    pub no_alloc_policy: Option<Span>,
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
    /// U11 (D-JPK-SCRIPTDEP1=A): the `#version` selector on `use pkg#version;` —
    /// an inline script dependency. Only ever `Some` for a single-segment
    /// `ImportKind::Module` (no dotted path); a manifest-less entry file's
    /// `Loader` pass resolves these (crate::Jetpack::ScriptDeps). `None` for
    /// every ordinary import, and ignored (kept for round-trip only) once a
    /// project has a `pkg.jet` — deps then come from the manifest.
    pub inline_version: Option<InlineVersion>,
}

/// The `#version` payload of `use pkg#version;` (U11). `text` is the exact
/// selector as written (e.g. `"1.4"`, `"1.4.2"`) — never renormalized, so
/// `jet fmt` round-trips it byte-for-byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineVersion {
    pub text: String,
    pub span: Span,
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
    /// canonical path (e.g. `"core.files"`, `"jet.http"`). Returns `None` for
    /// file/unqualified imports and unknown module names.
    pub fn core_module_path(&self) -> Option<String> {
        let ImportKind::Module(name, _) = &self.kind else {
            return None;
        };
        Syntax::normalize_core_module(name)
    }

    /// D-FFI-UNIFY1: parse a project-tier foreign namespace import,
    /// `use <lang>.<lib> as alias`.
    pub fn foreign_namespace(&self) -> Option<ForeignNamespace> {
        let ImportKind::Module(name, _) = &self.kind else {
            return None;
        };
        ForeignNamespace::from_module_path(name)
    }

    /// True when this import is any C `use` form (`use c.<lib>` or `use "<…>.h"`).
    pub fn is_c_import(&self) -> bool {
        match &self.kind {
            ImportKind::Module(_, _) => self
                .foreign_namespace()
                .map(|ns| ns.language == ForeignLanguage::C)
                .unwrap_or(false),
            ImportKind::File(path, _) => path.ends_with(".h"),
            ImportKind::Unqualified { .. } => false,
        }
    }
}

/// D-FFI-UNIFY1: registered foreign language roots. C and JS are active
/// namespace binders; rust/py/swift are ratified mounts whose binder depth
/// lands on later cards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ForeignLanguage {
    C,
    Rust,
    Py,
    Js,
    Swift,
    Go,
    Java,
    DotNet,
    Tcl,
    Fortran,
    Cobol,
    Ada,
    Pascal,
    Dart,
    PowerShell,
    Perl,
    Ruby,
    Php,
    R,
    Com,
}

impl ForeignLanguage {
    pub const ALL: [ForeignLanguage; 20] = [
        ForeignLanguage::C,
        ForeignLanguage::Rust,
        ForeignLanguage::Py,
        ForeignLanguage::Js,
        ForeignLanguage::Swift,
        ForeignLanguage::Go,
        ForeignLanguage::Java,
        ForeignLanguage::DotNet,
        ForeignLanguage::Tcl,
        ForeignLanguage::Fortran,
        ForeignLanguage::Cobol,
        ForeignLanguage::Ada,
        ForeignLanguage::Pascal,
        ForeignLanguage::Dart,
        ForeignLanguage::PowerShell,
        ForeignLanguage::Perl,
        ForeignLanguage::Ruby,
        ForeignLanguage::Php,
        ForeignLanguage::R,
        ForeignLanguage::Com,
    ];

    pub fn from_root(root: &str) -> Option<Self> {
        match root {
            Syntax::C_MODULE_ROOT => Some(ForeignLanguage::C),
            Syntax::KW_RUST => Some(ForeignLanguage::Rust),
            Syntax::PY_MODULE_ROOT => Some(ForeignLanguage::Py),
            Syntax::JS_MODULE_ROOT => Some(ForeignLanguage::Js),
            Syntax::SWIFT_MODULE_ROOT => Some(ForeignLanguage::Swift),
            Syntax::GO_MODULE_ROOT => Some(ForeignLanguage::Go),
            Syntax::JAVA_MODULE_ROOT => Some(ForeignLanguage::Java),
            Syntax::CS_MODULE_ROOT => Some(ForeignLanguage::DotNet),
            Syntax::TCL_MODULE_ROOT => Some(ForeignLanguage::Tcl),
            Syntax::FORTRAN_MODULE_ROOT => Some(ForeignLanguage::Fortran),
            Syntax::COBOL_MODULE_ROOT => Some(ForeignLanguage::Cobol),
            Syntax::ADA_MODULE_ROOT => Some(ForeignLanguage::Ada),
            Syntax::PASCAL_MODULE_ROOT => Some(ForeignLanguage::Pascal),
            Syntax::DART_MODULE_ROOT => Some(ForeignLanguage::Dart),
            Syntax::PWSH_MODULE_ROOT => Some(ForeignLanguage::PowerShell),
            Syntax::PERL_MODULE_ROOT => Some(ForeignLanguage::Perl),
            Syntax::RUBY_MODULE_ROOT => Some(ForeignLanguage::Ruby),
            Syntax::PHP_MODULE_ROOT => Some(ForeignLanguage::Php),
            Syntax::R_MODULE_ROOT => Some(ForeignLanguage::R),
            Syntax::COM_MODULE_ROOT => Some(ForeignLanguage::Com),
            _ => None,
        }
    }

    pub fn root(self) -> &'static str {
        match self {
            ForeignLanguage::C => Syntax::C_MODULE_ROOT,
            ForeignLanguage::Rust => Syntax::KW_RUST,
            ForeignLanguage::Py => Syntax::PY_MODULE_ROOT,
            ForeignLanguage::Js => Syntax::JS_MODULE_ROOT,
            ForeignLanguage::Swift => Syntax::SWIFT_MODULE_ROOT,
            ForeignLanguage::Go => Syntax::GO_MODULE_ROOT,
            ForeignLanguage::Java => Syntax::JAVA_MODULE_ROOT,
            ForeignLanguage::DotNet => Syntax::CS_MODULE_ROOT,
            ForeignLanguage::Tcl => Syntax::TCL_MODULE_ROOT,
            ForeignLanguage::Fortran => Syntax::FORTRAN_MODULE_ROOT,
            ForeignLanguage::Cobol => Syntax::COBOL_MODULE_ROOT,
            ForeignLanguage::Ada => Syntax::ADA_MODULE_ROOT,
            ForeignLanguage::Pascal => Syntax::PASCAL_MODULE_ROOT,
            ForeignLanguage::Dart => Syntax::DART_MODULE_ROOT,
            ForeignLanguage::PowerShell => Syntax::PWSH_MODULE_ROOT,
            ForeignLanguage::Perl => Syntax::PERL_MODULE_ROOT,
            ForeignLanguage::Ruby => Syntax::RUBY_MODULE_ROOT,
            ForeignLanguage::Php => Syntax::PHP_MODULE_ROOT,
            ForeignLanguage::R => Syntax::R_MODULE_ROOT,
            ForeignLanguage::Com => Syntax::COM_MODULE_ROOT,
        }
    }

    pub fn bindings_subdir(self) -> String {
        format!("{}/{}", Syntax::BINDINGS_ROOT_SUBDIR, self.root())
    }
}

/// D-FFI-UNIFY1: a mounted foreign library, `<lang>.<lib>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ForeignNamespace {
    pub language: ForeignLanguage,
    pub lib: String,
}

impl ForeignNamespace {
    pub fn from_module_path(path: &str) -> Option<Self> {
        let mut segs = path.split('.');
        let language = ForeignLanguage::from_root(segs.next()?)?;
        let lib = segs.next()?;
        if lib.is_empty() || segs.next().is_some() {
            return None;
        }
        Some(ForeignNamespace {
            language,
            lib: lib.to_string(),
        })
    }

    pub fn display(&self) -> String {
        format!("{}.{}", self.language.root(), self.lib)
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
    /// D-CABI-CALLBACK1: top-level function names sema proved are passed as a
    /// stable C callback symbol (`CallArgFlags::c_callback_symbol`) at some
    /// `#Extern` call site anywhere in the bundle. Codegen emits exactly these
    /// definitions as `extern "C" fn` — never every `@Pure fn` (that leaked the
    /// purity lever into codegen and broke I3 erasure; see 14dd68a5).
    pub ffi_callback_fns: std::collections::HashSet<String>,
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
    /// D-RINGLAYER1: optional `runtime:` ceiling from `pkg.jet`.
    pub layer_ceiling: Option<crate::RingLayer::RuntimeLayer>,
    /// D-RINGLAYER1: inferred minimum runtime profile for this package.
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
    /// D-OSTARGET2 (=B, ratified 2026-07-03): the active native OS bucket for
    /// this build — resolved from `--target=<triple>` (host OS when absent or a
    /// web/wasm pseudo-target). Seeded by the driver right after load; defaults
    /// to the host OS for every other bundle constructor (LSP, tests). Sema's
    /// `comptime if build.os == { … }` desugar reads it to fold the switch to
    /// the arm matching this OS, and it must equal codegen's `active_os` so the
    /// selected arm's gated `impl` is the one codegen keeps.
    pub active_os: crate::OsTarget::OsTarget,
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
    /// D-PRELUDEX1=A: `#NoPrelude` disables ambient `print`/`input` in this file.
    pub no_prelude: bool,
    /// D-HTMLPAIR1 (ratified 2026-07-01, c134): `#Html("path.html")` — this file's explicit
    /// companion host page for `--target=web` builds.
    pub html_path: Option<String>,
    /// D-MEM1/S7 (D-NOALLOC-SEM1=A): mirrors `Program::no_alloc_policy`.
    pub no_alloc_policy: Option<Span>,
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
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
pub struct MigrationDecl {
    pub type_name: String,
    pub type_span: Span,
    pub ops: Vec<MigrationOp>,
    pub span: Span,
}

/// D-MIGRATE1 / D-MIGRATE2: one operation inside a `migration { }` block.
#[derive(Debug, Clone)]
pub enum MigrationOp {
    /// D-MIGRATE1: `rename old_field -> new_field` — declares a field was renamed.
    Rename {
        from: String,
        from_span: Span,
        to: String,
        to_span: Span,
    },
    /// D-MIGRATE2A: `add f: T = default` — a new field with a default for old
    /// records. The `default` expr is the value old data is read with; sema
    /// checks intent, and D-MIGRATE4 lowers the default through `default_fn`
    /// for the runtime chain.
    Add {
        field: String,
        field_span: Span,
        ty: Type,
        ty_span: Span,
        default: Expr,
        default_span: Span,
        /// D-MIGRATE4: the mangled name of the synthetic zero-arg default
        /// function this `add` desugars to (`Sema::desugar_migrations`), so the
        /// runtime step function can evaluate the default. `None` until the
        /// desugar runs (and for a type with no runtime decode path).
        default_fn: Option<String>,
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
        /// D-MIGRATE4: the mangled name of the synthetic top-level converter
        /// function this `change` desugars to (`Sema::desugar_migrations`), so
        /// the runtime step function codegen can call it. `None` until the
        /// desugar runs (and for a type with no runtime decode path).
        conv_fn: Option<String>,
    },
}
