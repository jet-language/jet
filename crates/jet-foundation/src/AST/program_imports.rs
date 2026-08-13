use super::{CFfi, ComptimeInput, Expr, Func, Item, Marker, Stmt, Type};
use crate::{
    Diagnostics::{Diagnostic, Span},
    Syntax,
};

#[derive(Debug)]
pub struct Program {
    /// S16 (M6): `import` declarations at the top of this file.
    pub imports: Vec<ImportDecl>,
    pub items: Vec<Item>,
    /// D-ENTRY-SCRIPT1=B: top-level statements remain separate until the
    /// package seam materializes the entry file's implicit `fn run`.
    pub script_body: Vec<Stmt>,
    /// Parser-owned inner boundaries for statement blocks. Each span starts
    /// immediately after `{` and ends immediately before `}`.
    pub block_spans: Vec<Span>,
    /// D-EACH1=C: authored fenced statements retained for formatter emission.
    /// Sema and tooling consume the ordinary expanded statements in `items`.
    pub fenced_statements: Vec<FencedStatement>,
    /// D-WASM1 (c123): optional file-level web bucket ceiling (`js target;` / `wasm target;`).
    pub web_target_ceiling: Option<crate::WebPartition::WebBucket>,
    /// D-VISDEFAULT1=C / D-VISDEFAULT2=A: `#PubFile` flips default top-level export visibility.
    pub pub_file: bool,
    /// D-PRELUDEX1=A: `#NoPrelude` disables the readable Core prelude in this file.
    pub no_prelude: bool,
    /// D-WEBDEFAULT1 (ratified 2026-07-01, c134): `#Target(Web)` — this file's default CLI
    /// backend is the web target, so `jet run`/`jet dev`/`jet build` don't
    /// need `--target=web` on every invocation. `None` means the native
    /// default applies unless `pkg.jet` or an explicit `--target=` flag says
    /// otherwise. Distinct from `web_target_ceiling` (`Wasm`/`JS`, a partition
    /// ceiling *within* a web build) — `Web` here means "build for the web
    /// backend at all," a different axis, same marker family (I8).
    pub default_target: Option<String>,
    /// D-HTMLPAIR1 (ratified 2026-07-01, c134): `#HTML("path.html")` — this program's
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
    /// D-MARK-SCOPE1: source policy declarations with compiler-owned scope/target metadata.
    pub policy_declarations: Vec<crate::Policy::PolicyDeclaration>,
    /// D-MARK-STACK1: source-order applied rules for attachment sites whose
    /// semantic AST fields do not retain order. `None` targets the file;
    /// `Some(span)` targets the parsed function declaration.
    pub applied_rules: Vec<AppliedRuleApplication>,
    /// D-MARKSIG1=A: every source-order applied rule, retained unchanged for
    /// sema's shared signature-conformance pass.
    pub rule_facts: Vec<AppliedRuleApplication>,
}

#[derive(Debug, Clone)]
pub struct FencedStatement {
    pub span: Span,
    pub fences: Vec<FencedNames>,
    pub copies: usize,
}

#[derive(Debug, Clone)]
pub struct FencedNames {
    pub span: Span,
    pub names: Vec<(String, Span)>,
    pub range: Option<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct AppliedRuleApplication {
    pub marker: Marker,
    pub target: Option<Span>,
    pub site: Option<crate::Policy::RuleSite>,
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

/// The target selected by one member in a Core `.[…]` import list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreListPath {
    /// The member names another Core module, such as `encoding.json`.
    Module(String),
    /// The member names an item inside the longest known Core module prefix.
    Item { module: String, item: String },
}

/// D-CORE-USELIST1=A: the std path prefix a `use <prefix>.[…]` list walks, or
/// `None` when the prefix names something else. The prefix may be any depth, so
/// `use core.encoding.[json]` walks to `core.encoding.json` exactly as
/// `use core.[encoding.json]` does. `jet` is the retired spelling of the same
/// root and resolves to `core`.
pub fn core_list_prefix(module_alias: &str) -> Option<String> {
    let rest = module_alias
        .strip_prefix(Syntax::CORE_SHORT)
        .or_else(|| module_alias.strip_prefix("jet"))?;
    (rest.is_empty() || rest.starts_with('.'))
        .then(|| format!("{}{rest}", Syntax::CORE_SHORT))
}

/// Resolve one member path in a Core `.[…]` import list. The longest known
/// module prefix wins, so `core.[math.abs]` and `core.math.[abs]` select the
/// same Core member while `core.encoding.[json]` selects a submodule.
pub fn core_list_path(module_alias: &str, member: &str) -> Option<CoreListPath> {
    let prefix = core_list_prefix(module_alias)?;
    let full = format!("{prefix}.{member}");
    if Syntax::is_known_core_module(&full) {
        return Some(CoreListPath::Module(full));
    }

    let parts: Vec<&str> = full.split('.').collect();
    for split in (1..parts.len()).rev() {
        let module = parts[..split].join(".");
        if Syntax::is_known_core_module(&module) {
            let item = parts[split..].join(".");
            if !item.is_empty() {
                return Some(CoreListPath::Item { module, item });
            }
        }
    }
    None
}

/// The binding introduced by one member-list item. A dotted Core member uses
/// its leaf by default (`use core.[encoding.json]` binds `json`); an explicit
/// alias always wins.
pub fn member_import_local(original: &str, alias: Option<&str>) -> String {
    alias
        .map(str::to_owned)
        .unwrap_or_else(|| original.rsplit('.').next().unwrap_or(original).to_string())
}

/// One canonical import binding produced by `ImportDecl::walk_bindings`.
///
/// A single module import and every member of a `.[…]` list use this same
/// shape. The source `ImportKind` remains the parser's lossless representation;
/// this view carries the authority, scope metadata, alias, and full member path
/// needed by every consumer after parsing.
#[derive(Debug, Clone)]
pub struct ImportBinding<'a> {
    /// The path prefix. For a single module import this is the full module path;
    /// for a member list it is the list prefix before `.[…]`.
    pub module_alias: &'a str,
    /// The dotted member path, or `None` for a single module import.
    pub original: Option<&'a str>,
    /// The local binding introduced by this import.
    pub local: String,
    /// An explicit member alias, when the source supplied one.
    pub alias: Option<&'a str>,
    pub module_alias_span: Span,
    pub items_span: Option<Span>,
    pub import_span: Span,
    pub is_pub: bool,
    pub is_package_pub: bool,
}

impl ImportBinding<'_> {
    /// Reconstruct the canonical dotted path consumed by namespace resolvers.
    pub fn path(&self) -> String {
        match self.original {
            Some(original) => format!("{}.{}", self.module_alias, original),
            None => self.module_alias.to_string(),
        }
    }
}

/// A foreign-language-rooted import whose path cannot name exactly one library.
/// The error is returned before any member is resolved, so a group can never be
/// partially imported.
#[derive(Debug, Clone)]
pub struct ForeignImportError {
    pub path: String,
    pub language: ForeignLanguage,
    pub span: Span,
}

impl ForeignImportError {
    pub fn diagnostic(&self) -> Diagnostic {
        let root = self.language.root();
        Diagnostic::error(
            "E0611",
            format!("`{}` is not a foreign library namespace", self.path),
            format!(
                "foreign imports have exactly one library segment after the `{root}` language root"
            ),
            format!(
                "write `use {root}.[library]` or `use {root}.library as alias`"
            ),
            Some(self.span),
        )
    }
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

    /// If this import uses Core's canonical namespace, return its path. The
    /// loader keeps unknown `core.*` paths in this lane so sema can report the
    /// module-specific diagnostic.
    pub fn core_module_path(&self) -> Option<String> {
        let ImportKind::Module(name, _) = &self.kind else {
            return None;
        };
        (name == Syntax::CORE_SHORT
            || name == Syntax::CORE_CANONICAL
            || name == "app"
            || name.starts_with("core."))
            .then(|| name.clone())
    }

    /// Walk one canonical binding for a single import or each member of a
    /// `.[…]` import list. All consumers use this view instead of destructuring
    /// `ImportKind::Unqualified` independently.
    pub fn walk_bindings(&self) -> Vec<ImportBinding<'_>> {
        match &self.kind {
            ImportKind::Module(name, path_span) => vec![ImportBinding {
                module_alias: name,
                original: None,
                local: self.import_alias(),
                alias: (!self.alias.is_empty()).then_some(self.alias.as_str()),
                module_alias_span: *path_span,
                items_span: None,
                import_span: self.span,
                is_pub: self.is_pub,
                is_package_pub: self.is_package_pub,
            }],
            ImportKind::Unqualified {
                module_alias,
                module_alias_span,
                items,
                items_span,
                ..
            } => items
                .iter()
                .map(|(original, alias)| ImportBinding {
                    module_alias,
                    original: Some(original),
                    local: member_import_local(original, alias.as_deref()),
                    alias: alias.as_deref(),
                    module_alias_span: *module_alias_span,
                    items_span: Some(*items_span),
                    import_span: self.span,
                    is_pub: self.is_pub,
                    is_package_pub: self.is_package_pub,
                })
                .collect(),
            ImportKind::File(_, _) => Vec::new(),
        }
    }

    /// Resolve every foreign library carried by this import.
    ///
    /// The member-list form deliberately stays in `ImportKind::Unqualified`:
    /// `use c.[raylib as rl, sqlite3]` is the same parser path as an ordinary
    /// `use alias.[item as local]`.  This helper is the shared semantic seam
    /// for loaders, sema, and codegen; no foreign namespace gets a grammar of
    /// its own.
    pub fn foreign_imports(
        &self,
    ) -> Result<Vec<(ForeignNamespace, String)>, ForeignImportError> {
        if matches!(&self.kind, ImportKind::File(_, _)) {
            return Ok(Vec::new());
        }
        let mut imports = Vec::new();
        for binding in self.walk_bindings() {
            let path = binding.path();
            let Some(language) = path
                .split('.')
                .next()
                .and_then(ForeignLanguage::from_root)
            else {
                continue;
            };
            let Some(namespace) = ForeignNamespace::from_module_path(&path) else {
                return Err(ForeignImportError {
                    path,
                    language,
                    span: binding.import_span,
                });
            };
            imports.push((namespace, binding.local));
        }
        Ok(imports)
    }

    /// True when this import is any C `use` form (`use c.<lib>`, a C member
    /// list, or `use "<…>.h"`).
    pub fn is_c_import(&self) -> Result<bool, ForeignImportError> {
        match &self.kind {
            ImportKind::Module(_, _) | ImportKind::Unqualified { .. } => self
                .foreign_imports()
                .map(|imports| {
                    imports
                        .into_iter()
                        .any(|(ns, _)| ns.language == ForeignLanguage::C)
                }),
            ImportKind::File(path, _) => Ok(path.ends_with(".h")),
        }
    }
}

/// D-FFI-UNIFY1: registered foreign language roots. C and JS are active
/// namespace binders; rust/py/swift are ratified mounts whose binder depth
/// lands on later cards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ForeignLanguage {
    C,
    Cpp,
    Rust,
    Py,
    JS,
    Swift,
    Go,
    Java,
    DotNet,
    Tcl,
    Lua,
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
    pub const ALL: [ForeignLanguage; 22] = [
        ForeignLanguage::C,
        ForeignLanguage::Cpp,
        ForeignLanguage::Rust,
        ForeignLanguage::Py,
        ForeignLanguage::JS,
        ForeignLanguage::Swift,
        ForeignLanguage::Go,
        ForeignLanguage::Java,
        ForeignLanguage::DotNet,
        ForeignLanguage::Tcl,
        ForeignLanguage::Lua,
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
            Syntax::CPP_MODULE_ROOT => Some(ForeignLanguage::Cpp),
            Syntax::KW_RUST => Some(ForeignLanguage::Rust),
            Syntax::PY_MODULE_ROOT => Some(ForeignLanguage::Py),
            Syntax::JS_MODULE_ROOT => Some(ForeignLanguage::JS),
            Syntax::SWIFT_MODULE_ROOT => Some(ForeignLanguage::Swift),
            Syntax::GO_MODULE_ROOT => Some(ForeignLanguage::Go),
            Syntax::JAVA_MODULE_ROOT => Some(ForeignLanguage::Java),
            Syntax::CS_MODULE_ROOT => Some(ForeignLanguage::DotNet),
            Syntax::TCL_MODULE_ROOT => Some(ForeignLanguage::Tcl),
            Syntax::LUA_MODULE_ROOT => Some(ForeignLanguage::Lua),
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
            ForeignLanguage::Cpp => Syntax::CPP_MODULE_ROOT,
            ForeignLanguage::Rust => Syntax::KW_RUST,
            ForeignLanguage::Py => Syntax::PY_MODULE_ROOT,
            ForeignLanguage::JS => Syntax::JS_MODULE_ROOT,
            ForeignLanguage::Swift => Syntax::SWIFT_MODULE_ROOT,
            ForeignLanguage::Go => Syntax::GO_MODULE_ROOT,
            ForeignLanguage::Java => Syntax::JAVA_MODULE_ROOT,
            ForeignLanguage::DotNet => Syntax::CS_MODULE_ROOT,
            ForeignLanguage::Tcl => Syntax::TCL_MODULE_ROOT,
            ForeignLanguage::Lua => Syntax::LUA_MODULE_ROOT,
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
    /// D-MOD3/4: `use alias.Item` / `use alias.[A, B as C]` / `pub use alias.Item`
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
    /// definitions as `extern "C" fn` — never every `#Pure fn` (that leaked the
    /// purity lever into codegen and broke I3 erasure; see 14dd68a5).
    pub ffi_callback_fns: std::collections::HashSet<String>,
    /// S59 (E2-M14): C-FFI artifacts produced by `CFFI::assemble` after loading
    /// — per-file `use c.<lib>` bindings and the libraries to link against.
    pub cffi: CFfi,
    /// D-CTEFFECT1 Tier-1: embed_file/embed_bytes inputs accumulated by sema.
    /// Each entry records the path and sha256 of a file embedded at compile
    /// time. Written to `.jet/lock` by the build driver for reproducibility.
    pub comptime_inputs: Vec<ComptimeInput>,
    /// One name ledger. Loader seeds file-import edges; sema fills checked
    /// declaration, alias, visibility, path, and reference facts.
    pub name_ledger: crate::Names::NameLedger,
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
    /// `$if build.os == { … }` desugar reads it to fold the switch to
    /// the arm matching this OS, and it must equal codegen's `active_os` so the
    /// selected arm's gated `impl` is the one codegen keeps.
    pub active_os: crate::OSTarget::OSTarget,
    /// D-REL3 / card #712: resolved package edition (`"2026"`, `"2027"`, …).
    /// Single-file programs use the toolchain's newest stable edition.
    pub edition: String,
}

impl ProgramBundle {
    /// Materialize loose statements in the direct entry module as its implicit `fn run`.
    ///
    /// Invalid script shapes remain in `script_body` for sema to diagnose. In particular, an
    /// explicit `fn run` is never wrapped or replaced.
    pub fn materialize_script_entries(&mut self) {
        let entry = self.entry;
        for (module_idx, module) in self.modules.iter_mut().enumerate() {
            if module_idx != entry || module.script_body.is_empty() {
                continue;
            }

            let has_explicit_run = module
                .items
                .iter()
                .any(|item| matches!(item, Item::Func(func) if func.name == "run"));
            if has_explicit_run {
                continue;
            }

            let body = std::mem::take(&mut module.script_body);
            let span = Span::new(
                body.first().map_or(0, |stmt| stmt.span().start),
                body.last().map_or(0, |stmt| stmt.span().end),
            );
            module.items.push(Item::Func(Func::implicit_run(body, span)));
        }
    }
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
    /// D-ENTRY-SCRIPT1=B: raw top-level statements from a script file. The
    /// package seam materializes a valid direct-entry body; sema consumes any
    /// remaining body to diagnose imported scripts or an explicit `fn run` conflict.
    pub script_body: Vec<Stmt>,
    /// Checked parser-owned inner boundaries for statement blocks.
    pub block_spans: Vec<Span>,
    /// D-WASM1: optional file-level web bucket ceiling.
    pub web_target_ceiling: Option<crate::WebPartition::WebBucket>,
    /// D-VISDEFAULT1=C / D-VISDEFAULT2=A: `#PubFile` flips default top-level export visibility.
    pub pub_file: bool,
    /// D-PRELUDEX1=A: `#NoPrelude` disables the readable Core prelude in this file.
    pub no_prelude: bool,
    /// D-HTMLPAIR1 (ratified 2026-07-01, c134): `#HTML("path.html")` — this file's explicit
    /// companion host page for `--target=web` builds.
    pub html_path: Option<String>,
    /// D-MEM1/S7 (D-NOALLOC-SEM1=A): mirrors `Program::no_alloc_policy`.
    pub no_alloc_policy: Option<Span>,
    /// Mirrors `Program::policy_declarations` for sema/index/explain consumers.
    pub policy_declarations: Vec<crate::Policy::PolicyDeclaration>,
    /// Mirrors `Program::rule_facts` for sema signature conformance.
    pub rule_facts: Vec<AppliedRuleApplication>,
}

/// Walk every import-bearing scope in a loaded module. Top-level imports use
/// `None`; inline, generic, and instantiated module bodies carry their owning
/// namespace. The recursive shape is shared by CFFI, foreign binders, and
/// codegen so no tier invents a second namespace traversal.
pub fn walk_imports(module: &LoadedModule) -> Vec<(Option<&str>, &ImportDecl)> {
    fn collect_code_module_imports<'a>(
        code_module: &'a crate::AST::CodeModule,
        imports: &mut Vec<(Option<&'a str>, &'a ImportDecl)>,
    ) {
        imports.extend(
            code_module
                .imports
                .iter()
                .map(|import| (Some(code_module.name.as_str()), import)),
        );
        if let Some(body) = &code_module.body {
            for item in body {
                if let Item::CodeModule(child) = item {
                    collect_code_module_imports(child, imports);
                }
            }
        }
    }

    let mut imports = module
        .imports
        .iter()
        .map(|import| (None, import))
        .collect::<Vec<_>>();
    for item in &module.items {
        match item {
            Item::CodeModule(code_module) => {
                collect_code_module_imports(code_module, &mut imports);
            }
            Item::GenericModule(generic_module) => {
                imports.extend(
                    generic_module
                        .imports
                        .iter()
                        .map(|import| (Some(generic_module.name.as_str()), import)),
                );
                for alias in &module.items {
                    let Item::ModuleAlias(alias) = alias else {
                        continue;
                    };
                    if alias.target == generic_module.name {
                        imports.extend(
                            generic_module
                                .imports
                                .iter()
                                .map(|import| (Some(alias.name.as_str()), import)),
                        );
                    }
                }
                for item in &generic_module.body {
                    if let Item::CodeModule(code_module) = item {
                        collect_code_module_imports(code_module, &mut imports);
                    }
                }
            }
            _ => {}
        }
    }
    imports
}

/// D-ERR-CONV (ratified 2026-06-19): how `?` converts the error type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TryConvert {
    /// Error types match exactly — no conversion needed.
    None,
    /// D-FAIL-ERROR1=A: a legacy string-shaped failure becomes the default
    /// `Err` value at the propagation seam.
    DefaultErr,
    /// Declared `impl Source => Target { … }` conversion (D-ERR-CONV).
    /// Holds the mangled Rust function name emitted by codegen.
    Typed(String),
    /// D-UNIONTYPE1=A: source error is one member of the return's anonymous union.
    /// Codegen wraps with the canonical `__jet_<enum>::__jet_<tag>(e)` path.
    WidenUnion { enum_name: String, tag: String },
}

/// D-ERR-CONV (ratified 2026-06-19): `impl Source => Target { body }` — declares
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
    /// lambda); `None` falls back to an `impl Old => New` in scope (D-MIGRATE2B).
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
