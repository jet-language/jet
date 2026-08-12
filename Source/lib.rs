//! jet — compiler library.
//!
//! Pipeline: lex -> parse -> sema -> codegen (docs/spec/architecture.md).
//! The front end (everything before codegen) owns ALL user-facing
//! correctness and every diagnostic. The Rust backend is a verifier and
//! optimizer, never a source of user-facing errors.

// Source files/modules use PascalCase names (owner decision), which trips the
// non_snake_case lint at module-name level.
#![allow(non_snake_case)]
// Warnings are errors: keeps the build warning-clean (card c115).
#![deny(warnings)]

// Seam crates — re-export everything so callers use `jet::AST`, `jet::Sema`, etc.
// unchanged. Within Source/, `crate::AST` etc. resolve through these re-exports.
pub use jet_driver::{
    // Top-level re-exports from Compile module:
    bundle_uses_unsafe,
    boot_tir_eval,
    AdaBind,
    CBind,
    CppBind,
    CobolBind,
    ComBind,
    DartBind,
    DotNetBind,
    PowerShellBind,
    FortranBind,
    GoBind,
    JavaBind,
    LuaBind,
    PascalBind,
    PerlBind,
    RubyBind,
    PhpBind,
    RBind,
    TclBind,
    CanonicalAST,
    Capabilities,
    Codegen,
    Collections,
    Compile,
    CompileOutput,
    Comptime,
    Diagnostics,
    Driver,
    // Card #367 / D-PRODUCT-SPLIT1=C slice 3: the read-only package/config
    // model and its pure policy computation (effect budget, lint policy) —
    // routed through jet-driver's jet-pkg-model re-export, not the full
    // `jetpack` engine. Renamed `Store` to `PkgStore`: this crate already has
    // its own `crate::Store` (the compiler's build cache) — `PkgStore` is the
    // read-only hangar root/listing half (`jetpack::Store` still re-exports
    // the same `lock_path` etc. for the engine's own genuine store calls).
    EffectBudget,
    Foreign,
    Formatter,
    Generics,
    Lexer,
    LintPolicy,
    Loader,
    LibraryExport,
    JetLibArtifact,
    JetLibStamp,
    Lock,
    Manifest,
    Package,
    Parser,
    Policy,
    PhaseTiming,
    ScriptDeps,
    Sema,
    Store as PkgStore,
    Syntax,
    TargetMachine,
    Traits,
    AST,
    Authority,
    CFFI,
    FFI,
    SHA256,
};
pub use jet_queries as Queries;
// D-ARCH-SOURCE1=A: full debugger subsystem and stable exit policy live in
// inward workspace seams. Preserve public paths without root-owned wrappers.
pub use jet_debug as Debug;
// D-ARCH-SOURCE1=A: CLI registry, argument vocabulary, diagnostic reference,
// and hybrid help UI live in the inward jet-cli seam. Public paths remain
// `jet::CLI`, `jet::Explain`, and `jet::Help` without root wrapper modules.
pub use jet_cli::{CLI, Explain, Help};
pub use jet_canvas as CanvasUi;
pub use jet_devserver as DevServer;
pub use jet_foundation::ExitCodes;
// D-FAIL-CARRIER1=A: the one outcome carrier. It is embedded as the first
// prelude part, and it is a real module here too, so the compiler's own
// interpreters call the same functions the generated program calls.
pub use jet_foundation::Outcome;
// D-ARCH-SOURCE1=A: real REPL seam ownership. Compatibility paths remain
// `jet::REPL`, `jet::Term`, and `jet::SemanticSymbols`; implementation lives
// entirely in the workspace crate.
pub use jet_repl as REPL;
pub use jet_repl::{SemanticSymbols, Term};
pub mod BuildCache;
pub mod RunCache;
pub mod RuntimeCache;
pub mod BudgetProviders;
pub mod BudgetStore;
pub use jet_driver::BudgetView;
pub use jet_driver::ProjectParts;
pub use jet_devserver::Canvas;
pub mod Compiler;
pub mod Doctest;
pub mod Doctor;
pub mod Fetch;
pub use jet_driver::FixEngine;
pub mod Interpreter;
pub mod JitBackend;
pub mod LSP;
pub mod Publish;
pub mod Store;

use Diagnostics::Diagnostic;

fn with_compiler_stack<R: Send>(work: impl FnOnce() -> R + Send) -> R {
    // D-FRONTENDAPI1=A: install the one read-only Core compiler callback for
    // every compiler entry point. Build uses the same ambient seam, so a
    // `comptime` binding and `fn build` cannot observe different APIs.
    jet_driver::run_compiler_work(|| {
        Comptime::with_ambient(Some(Compiler::eval_core_call), None, work)
    })
}

/// Run the full front end on source text. All lex errors (then all parse
/// errors) surface in one run — M1 error recovery.
pub fn compile(src: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    compile_with_mode(src, "input.jet", Sema::CompileMode::Run)
}

pub fn compile_with_path(src: &str, file: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    let _ = src;
    compile_bundle_path(file, Sema::CompileMode::Run, None)
}

/// Like `compile_with_path`, but threads a `--target=<triple>` (or `None`)
/// through to codegen's native OS-target gating (D-OSTARGET1=A, ratified
/// 2026-07-01, c134) — an `impl` gated to a different `#Target(OS.*)` than
/// the resolved active OS is skipped entirely. `jet build`/`jet run`'s real
/// `--target=` flag is the only caller; `compile_with_path` keeps its
/// existing host-OS-default behavior unchanged for every other caller.
pub fn compile_with_target(
    src: &str,
    file: &str,
    cross_target: Option<&str>,
) -> Result<CompileOutput, Vec<Diagnostic>> {
    let _ = src;
    compile_bundle_path(file, Sema::CompileMode::Run, cross_target)
}

/// Front-end check for a file on disk (and its imports). Library modules
/// need not define `main`; use `compile_with_path` when building or running.
pub fn check_with_path(file: &str) -> Vec<Diagnostic> {
    with_compiler_stack(|| {
        if let Some(diagnostics) = check_workspace_file(file) {
            return diagnostics;
        }
        let mut queries = jet_driver::QueryService::CompilerQueries::new();
        queries.check_disk(file, true).diagnostics.as_ref().clone()
    })
}

/// Workspace sources use the workspace evaluator as their front end. Keep its
/// diagnostics in the same carrier as ordinary source checks so callers use
/// the shared renderer and JSON path.
fn check_workspace_file(file: &str) -> Option<Vec<Diagnostic>> {
    let path = absolute_source_path(file);
    let root = path.parent().unwrap_or(std::path::Path::new("."));
    let resolver = match jet_driver::Authority::AuthorityResolver::open(root) {
        Ok(resolver) => resolver,
        Err(error) if error.is_missing() => return None,
        Err(error) => return Some(vec![error.diagnostic()]),
    };
    let source = match resolver.resolve_workspace_source() {
        Ok(Some(source)) => source,
        Ok(None) => return None,
        Err(error) => return Some(vec![error.workspace_diagnostic()]),
    };
    let Some(entry_name) = path.file_name() else {
        return None;
    };
    if source.path != resolver.root().join(entry_name) {
        return None;
    }
    match jetpack::WorkspaceFile::evaluate_checked_source(&source, &resolver) {
        Ok(_) => Some(Vec::new()),
        Err(diagnostic) => Some(vec![diagnostic]),
    }
}

/// Full sema type-check for `jet eval`: runs the same pipeline as `compile`
/// but with `CompileMode::Eval` so E0122 (`run` return shape) is relaxed
/// while all other diagnostics (type errors, unknown identifiers, etc.) still
/// fire. Returns the error diagnostics, or an empty vec on success.
pub fn check_for_eval(src: &str, file: &str) -> Vec<Diagnostic> {
    with_compiler_stack(|| Driver::check_eval(src, file))
}

/// Check an already loaded bundle on the compiler-owned stack.
pub fn check_bundle(bundle: &mut AST::ProgramBundle, mode: Sema::CompileMode) -> Vec<Diagnostic> {
    with_compiler_stack(|| Sema::check_bundle(bundle, mode))
}

fn compile_bundle_path(
    file: &str,
    mode: Sema::CompileMode,
    cross_target: Option<&str>,
) -> Result<CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts(file, mode, false, false, false, cross_target)
}

/// Like `compile_with_path` but with `--freestanding` mode (E2-M15).
/// Rejects OS-dependent std APIs (E3301) and emits `panic = "abort"` hint.
pub fn compile_freestanding(file: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts(file, Sema::CompileMode::Run, true, false, false, None)
}

/// Like `compile_with_path` but with `--allow-impure` (D-CTEFFECT1).
/// Enables Tier-2 ambient comptime effects inside `#Impure` gates.
pub fn compile_allow_impure(file: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts(file, Sema::CompileMode::Run, false, true, false, None)
}

/// D-BUILDENTRY1: native `jet build` path. No root `fn build` keeps existing
/// zero-config pipeline; selected root entry evaluates and executes first.
pub fn compile_programmable_build(
    file: &str,
    grants: &[String],
) -> Result<CompileOutput, Vec<Diagnostic>> {
    compile_programmable_build_opts(file, grants, false, true, false, false, false, None)
}

pub fn compile_programmable_build_opts(
    file: &str,
    grants: &[String],
    freestanding: bool,
    allow_impure: bool,
    locked: bool,
    web_target: bool,
    plugin_target: bool,
    cross_target: Option<&str>,
) -> Result<CompileOutput, Vec<Diagnostic>> {
    compile_programmable_build_opts_with_builder(
        file,
        grants,
        freestanding,
        allow_impure,
        locked,
        web_target,
        plugin_target,
        cross_target,
        None,
    )
}

pub fn compile_programmable_build_opts_with_builder(
    file: &str,
    grants: &[String],
    freestanding: bool,
    allow_impure: bool,
    locked: bool,
    web_target: bool,
    plugin_target: bool,
    cross_target: Option<&str>,
    remote_builder: Option<&str>,
) -> Result<CompileOutput, Vec<Diagnostic>> {
    compile_programmable_build_opts_inner(
        file,
        grants,
        freestanding,
        allow_impure,
        locked,
        web_target,
        plugin_target,
        cross_target,
        false,
        remote_builder,
    )
}

/// D-BUILDGEN1 / #1040: compile a programmable build and copy the exact
/// generated Jet sources from this transaction into `build/generated/` for
/// inspection. The build still compiles the normal `.jet/generated` inputs;
/// this is only a visible export of the same materialized bytes.
pub fn compile_programmable_build_emit_generated_opts(
    file: &str,
    grants: &[String],
    freestanding: bool,
    allow_impure: bool,
    locked: bool,
    web_target: bool,
    plugin_target: bool,
    cross_target: Option<&str>,
) -> Result<CompileOutput, Vec<Diagnostic>> {
    compile_programmable_build_emit_generated_opts_with_builder(
        file,
        grants,
        freestanding,
        allow_impure,
        locked,
        web_target,
        plugin_target,
        cross_target,
        None,
    )
}

pub fn compile_programmable_build_emit_generated_opts_with_builder(
    file: &str,
    grants: &[String],
    freestanding: bool,
    allow_impure: bool,
    locked: bool,
    web_target: bool,
    plugin_target: bool,
    cross_target: Option<&str>,
    remote_builder: Option<&str>,
) -> Result<CompileOutput, Vec<Diagnostic>> {
    compile_programmable_build_opts_inner(
        file,
        grants,
        freestanding,
        allow_impure,
        locked,
        web_target,
        plugin_target,
        cross_target,
        true,
        remote_builder,
    )
}

fn compile_programmable_build_opts_inner(
    file: &str,
    grants: &[String],
    freestanding: bool,
    allow_impure: bool,
    locked: bool,
    web_target: bool,
    plugin_target: bool,
    cross_target: Option<&str>,
    emit_generated: bool,
    remote_builder: Option<&str>,
) -> Result<CompileOutput, Vec<Diagnostic>> {
    with_compiler_stack(|| {
        let remote = remote_builder
            .map(Comptime::Build::RemoteBuildBinding::load_host)
            .transpose()
            .map_err(|error| {
                vec![Diagnostic::error(
                    "E3502",
                    format!("remote builder binding could not be loaded: {error}"),
                    "remote endpoints and credentials come only from host-owned bindings".to_string(),
                    "run `jet remote list`, or bind the builder with `jet remote bind`".to_string(),
                    None,
                )]
            })?;
        if let Some(checked_workspace) = workspace_build_authority(file)? {
            return compile_workspace_build_opts(
                file,
                grants,
                freestanding,
                allow_impure,
                locked,
                web_target,
                plugin_target,
                cross_target,
                emit_generated,
                remote,
                checked_workspace,
            );
        }
        let grants = resolve_build_grants(file, grants)?;
        let grants = grants
            .iter()
            .filter_map(|grant| Comptime::Build::BuildCapability::parse(grant))
            .collect();
        let output = Driver::compile_bundle_path_build(
            file,
            Driver::BuildRunOptions {
                grants,
                policy: production_build_policy(),
                execute: true,
                allow_impure,
                inspect_only: false,
                emit_generated,
                locked,
                freestanding,
                web_target,
                plugin_target,
                cross_target: cross_target.map(str::to_string),
                remote,
            },
        )?;
        if emit_generated {
            export_generated_sources(file, &output)?;
        }
        Ok(output.compile)
    })
}

/// D-BUILDSCOPE1: workspace builds are an orchestration boundary. Member
/// entries run in dependency order with their own BuildContext and policy;
/// the workspace entry runs last with a fresh context. No member plan crosses
/// that boundary as a mutable value.
fn compile_workspace_build_opts(
    _file: &str,
    grants: &[String],
    freestanding: bool,
    allow_impure: bool,
    locked: bool,
    web_target: bool,
    plugin_target: bool,
    cross_target: Option<&str>,
    emit_generated: bool,
    remote: Option<Comptime::Build::RemoteBuildBinding>,
    checked_workspace: (
        jet_driver::Authority::AuthorityResolver,
        jetpack::WorkspaceFile::WorkspaceSource,
    ),
) -> Result<CompileOutput, Vec<Diagnostic>> {
    let (workspace_resolver, workspace_source) = checked_workspace;
    workspace_resolver
        .revalidate_source(&workspace_source)
        .map_err(|error| vec![error.diagnostic()])?;
    let workspace_path = workspace_source.path.clone();
    let plan = jetpack::WorkspaceFile::evaluate_checked_source(
        &workspace_source,
        &workspace_resolver,
    )
    .map_err(|diagnostic| vec![workspace_build_root_diagnostic(&workspace_path, &diagnostic)])?;
    let members = jetpack::MemberSelect::dependency_order_packages_checked(
        &workspace_resolver,
        &plan.members,
    )
    .map_err(|error| vec![error.diagnostic()])?;

    for (member, checked_package) in members {
        let entry = match workspace_member_entry(
            &workspace_resolver,
            &member.path,
            checked_package,
        ) {
            Ok(entry) => entry,
            Err(diagnostic) => return Err(vec![diagnostic]),
        };
        let Some(entry) = entry else {
            return Err(vec![Diagnostic::error(
                "E3501",
                format!("workspace member `{}` has no build entry", member.name),
                "workspace members run their own unit-local build entry before the workspace entry".to_string(),
                "add `run.jet` or `src/run.jet`, or name the member source explicitly".to_string(),
                None,
            )]);
        };
        entry
            .revalidate(&workspace_resolver)
            .map_err(|error| vec![error.diagnostic()])?;
        let source_file = entry.source_file();
        let entry_path = source_file.path.to_string_lossy().into_owned();
        let entry_source = source_file
            .text()
            .map_err(|error| vec![error.diagnostic()])?;
        // A dependency/member is its own authority boundary. The workspace
        // CLI grant names the workspace root; it is not inherited by every
        // member. Package-local and explicitly subject-matched workspace
        // grants still flow through `resolve_build_grants`.
        let member_grants = resolve_build_grants(&entry_path, &[])?;
        let member_grants = member_grants
            .iter()
            .filter_map(|grant| Comptime::Build::BuildCapability::parse(grant))
            .collect();
        entry
            .revalidate(&workspace_resolver)
            .map_err(|error| vec![error.diagnostic()])?;
        let output = Driver::compile_bundle_path_build_as_dependency_with_overlay(
            &entry_path,
            &source_file.path,
            &entry_source,
            Driver::BuildRunOptions {
                grants: member_grants,
                policy: production_build_policy(),
                execute: true,
                allow_impure,
                inspect_only: false,
                emit_generated: false,
                locked,
                freestanding,
                web_target,
                plugin_target,
                cross_target: cross_target.map(str::to_string),
                remote: remote.clone(),
            },
        )?;
        entry
            .revalidate(&workspace_resolver)
            .map_err(|error| vec![error.diagnostic()])?;
        if emit_generated {
            export_generated_sources(&entry_path, &output)?;
        }
    }

    let workspace_grants = resolve_build_grants(&workspace_path.to_string_lossy(), grants)?;
    let workspace_grants = workspace_grants
        .iter()
        .filter_map(|grant| Comptime::Build::BuildCapability::parse(grant))
        .collect();
    workspace_resolver
        .revalidate_source(&workspace_source)
        .map_err(|error| vec![error.diagnostic()])?;
    let output = Driver::compile_bundle_path_build_with_overlay(
        &workspace_path.to_string_lossy(),
        &workspace_source.path,
        &workspace_source.source,
        Driver::BuildRunOptions {
            grants: workspace_grants,
            policy: production_build_policy(),
            execute: true,
            allow_impure,
            inspect_only: false,
            emit_generated: false,
            locked,
            freestanding,
            web_target,
            plugin_target,
            cross_target: cross_target.map(str::to_string),
            remote,
        },
    )?;
    workspace_resolver
        .revalidate_source(&workspace_source)
        .map_err(|error| vec![error.diagnostic()])?;
    if emit_generated {
        export_generated_sources(&workspace_path.to_string_lossy(), &output)?;
    }
    Ok(output.compile)
}

fn export_generated_sources(
    file: &str,
    output: &Driver::BuildCompileOutput,
) -> Result<(), Vec<Diagnostic>> {
    let Some(build) = &output.build else {
        return Ok(());
    };
    let project_root = build_project_root(file)?;
    let export_root = project_root.join("build/generated");
    let mut exports = Vec::new();
    for generated in build.generated.iter().filter(|generated| {
        generated
            .path
            .extension()
            .and_then(|extension| extension.to_str())
            == Some(Syntax::FILE_EXT)
    }) {
        let relative = generated
            .path
            .strip_prefix(&project_root)
            .ok()
            .and_then(|path| path.strip_prefix(".jet/generated").ok())
            .ok_or_else(|| {
                vec![Diagnostic::error(
                    "E3510",
                    format!("generated module `{}` is outside the project root", generated.name),
                    "the visible export must stay inside the project that produced the generated source".to_string(),
                    "run the build from the package containing the entry file".to_string(),
                    None,
                )]
            })?;
        let destination = export_root.join(relative);
        let source = read_real_generated_file(&project_root, &generated.path).map_err(|error| {
            vec![Diagnostic::error(
                "E3510",
                format!("could not read generated module `{}`: {error}", generated.name),
                "the visible export must copy the exact source that was compiled".to_string(),
                "remove the damaged generated file and rerun the build".to_string(),
                None,
            )]
        })?;
        exports.push((generated.name.as_str(), destination, source));
    }
    let paths = exports.iter().map(|(_, path, _)| path.clone());
    let mut transaction = GeneratedExportTransaction::new(&project_root, paths).map_err(|error| {
        vec![Diagnostic::error(
            "E3510",
            format!("could not prepare generated export: {error}"),
            "the visible export must be all-or-nothing and must not alter the compiled source".to_string(),
            "fix the build/generated directory permissions and try again".to_string(),
            None,
        )]
    })?;
    for (name, destination, source) in exports {
        write_exported_generated_file(&project_root, &destination, &source).map_err(|error| {
            vec![Diagnostic::error(
                "E3510",
                format!("could not write generated module `{name}`: {error}"),
                "the visible export must not alter the compiled source".to_string(),
                "fix the build/generated directory permissions and try again".to_string(),
                None,
            )]
        })?;
    }
    transaction.commit();
    Ok(())
}

fn workspace_build_root_diagnostic(
    workspace_path: &std::path::Path,
    diagnostic: &Diagnostic,
) -> Diagnostic {
    Diagnostic::error(
        "E3503",
        format!("build policy in `{}` is malformed", workspace_path.display()),
        diagnostic.why.clone(),
        diagnostic.fix.clone(),
        None,
    )
}

struct GeneratedExportTransaction {
    files: Vec<(std::path::PathBuf, Option<Vec<u8>>)>,
    project_root: std::path::PathBuf,
    committed: bool,
}

impl GeneratedExportTransaction {
    fn new(
        project_root: &std::path::Path,
        paths: impl IntoIterator<Item = std::path::PathBuf>,
    ) -> std::io::Result<Self> {
        let mut seen = std::collections::BTreeSet::new();
        let mut files = Vec::new();
        for path in paths.into_iter().filter(|path| seen.insert(path.clone())) {
            if path_has_symlinked_component(project_root, &path) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("generated export path `{}` contains a symlink", path.display()),
                ));
            }
            let before = match std::fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        format!("generated export path `{}` is a symlink", path.display()),
                    ));
                }
                Ok(metadata) if !metadata.is_file() => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        format!("generated export path `{}` is not a file", path.display()),
                    ));
                }
                Ok(_) => Some(std::fs::read(&path)?),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            };
            files.push((path, before));
        }
        Ok(Self {
            files,
            project_root: project_root.to_path_buf(),
            committed: false,
        })
    }

    fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for GeneratedExportTransaction {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for (path, before) in self.files.iter().rev() {
            if path_has_symlinked_component(&self.project_root, path) {
                continue;
            }
            match before {
                Some(bytes) => {
                    let _ = write_exported_generated_file(&self.project_root, path, bytes);
                }
                None => {
                    if !std::fs::symlink_metadata(path)
                        .is_ok_and(|metadata| metadata.file_type().is_symlink())
                    {
                        let _ = std::fs::remove_file(path);
                    }
                }
            }
        }
    }
}

fn workspace_build_authority(
    file: &str,
) -> Result<
    Option<(
        jet_driver::Authority::AuthorityResolver,
        jetpack::WorkspaceFile::WorkspaceSource,
    )>,
    Vec<Diagnostic>,
> {
    let path = absolute_source_path(file);
    let root = path.parent().unwrap_or(std::path::Path::new("."));
    let resolver = match jet_driver::Authority::AuthorityResolver::open(root) {
        Ok(resolver) => resolver,
        Err(error) if error.is_missing() => return Ok(None),
        Err(error) => return Err(vec![error.diagnostic()]),
    };
    let Some(source) = resolver
        .resolve_workspace_source()
        .map_err(|error| vec![workspace_build_root_diagnostic(&path, &error.workspace_diagnostic())])?
    else {
        return Ok(None);
    };
    let Some(entry_name) = path.file_name() else {
        return Ok(None);
    };
    let checked_entry = resolver
        .checked_file(std::path::Path::new(entry_name))
        .map_err(|error| vec![error.diagnostic()])?;
    if checked_entry.path != source.path || !jetpack::WorkspaceFile::has_build_entry(&source.source) {
        return Ok(None);
    }
    resolver
        .revalidate_source(&source)
        .map_err(|error| vec![error.diagnostic()])?;
    if source.role != jetpack::WorkspaceFile::WorkspaceSourceRole::Index {
        return Err(vec![Diagnostic::error(
            "E1334",
            "workspace build entry is selected by an authority source".to_string(),
            "workspace build execution requires the matching source to have the Index role; an authority source cannot select a workspace entry".to_string(),
            "declare the workspace index in `workspace.jet`, or build the member entry directly".to_string(),
            None,
        )]);
    }
    Ok(Some((resolver, source)))
}

fn absolute_source_path(file: &str) -> std::path::PathBuf {
    let path = std::path::Path::new(file);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(path)
    }
}

struct CheckedBuildEntry {
    package: jet_driver::Authority::CheckedPackage,
    file: Option<jet_driver::Authority::CheckedFile>,
}

impl CheckedBuildEntry {
    fn source_file(&self) -> &jet_driver::Authority::CheckedFile {
        self.file
            .as_ref()
            .unwrap_or(&self.package.member.manifest.file)
    }

    fn revalidate(
        &self,
        workspace_resolver: &jet_driver::Authority::AuthorityResolver,
    ) -> Result<(), jet_driver::Authority::AuthorityError> {
        workspace_resolver.revalidate_member(&self.package.member)?;
        let member_resolver =
            jet_driver::Authority::AuthorityResolver::from_checked_directory(
                &self.package.member.directory,
            );
        match &self.file {
            Some(file) => member_resolver.revalidate_file(file),
            None => workspace_resolver.revalidate_file(&self.package.member.manifest.file),
        }
    }
}

fn workspace_member_entry(
    resolver: &jet_driver::Authority::AuthorityResolver,
    member: &str,
    checked_package: jet_driver::Authority::CheckedPackage,
) -> Result<Option<CheckedBuildEntry>, Diagnostic> {
    resolver
        .revalidate_member(&checked_package.member)
        .map_err(|error| error.diagnostic())?;
    let member_resolver = jet_driver::Authority::AuthorityResolver::from_checked_directory(
        &checked_package.member.directory,
    );
    if let Some(entry) = checked_package
        .facts
        .resolve_run_entry_checked(&member_resolver)
        .map_err(|error| {
        Diagnostic::error(
            "E3501",
            format!("workspace member `{member}` has an invalid typed output"),
            error,
            "repair the member's typed Package output before building the workspace".to_string(),
            None,
        )
    })? {
        return Ok(Some(CheckedBuildEntry {
            package: checked_package,
            file: Some(entry),
        }));
    }
    for candidate in [
        std::path::PathBuf::from(Syntax::DEFAULT_ENTRY_FILE),
        std::path::PathBuf::from("src").join(Syntax::DEFAULT_ENTRY_FILE),
    ] {
        match member_resolver.checked_file(&candidate) {
            Ok(file) => {
                return Ok(Some(CheckedBuildEntry {
                    package: checked_package,
                    file: Some(file),
                }))
            }
            Err(error) if error.is_missing() => {}
            Err(error) => return Err(error.diagnostic()),
        }
    }
    let named = std::path::PathBuf::from(format!(
        "{}.{}",
        checked_package.facts.name,
        Syntax::FILE_EXT
    ));
    match member_resolver.checked_file(&named) {
        Ok(file) => {
            return Ok(Some(CheckedBuildEntry {
                package: checked_package,
                file: Some(file),
            }))
        }
        Err(error) if error.is_missing() => {}
        Err(error) => return Err(error.diagnostic()),
    }
    let package_source = checked_package
        .member
        .manifest
        .file
        .text()
        .map_err(|error| error.diagnostic())?;
    if Package::build_entry_source(&package_source).is_some() {
        return Ok(Some(CheckedBuildEntry {
            package: checked_package,
            file: None,
        }));
    }
    Ok(None)
}

fn read_real_generated_file(
    project_root: &std::path::Path,
    path: &std::path::Path,
) -> std::io::Result<Vec<u8>> {
    if path_has_symlinked_component(project_root, path)
        || std::fs::symlink_metadata(path)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "generated source path contains a symlink",
        ));
    }
    std::fs::read(path)
}

fn write_exported_generated_file(
    project_root: &std::path::Path,
    path: &std::path::Path,
    bytes: &[u8],
) -> std::io::Result<()> {
    if path_has_symlinked_component(project_root, path)
        || std::fs::symlink_metadata(path)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "generated export path contains a symlink",
        ));
    }
    let Some(parent) = path.parent() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "generated export has no parent directory",
        ));
    };
    ensure_real_directory(parent)?;
    for attempt in 0..100u32 {
        let temporary = parent.join(format!(".jet-export-{}-{attempt}", std::process::id()));
        let mut file = match std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        use std::io::Write;
        if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }
        std::fs::rename(&temporary, path)?;
        return Ok(());
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate a unique generated export temporary file",
    ))
}

fn path_has_symlinked_component(root: &std::path::Path, path: &std::path::Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return true;
    };
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(part) = component else {
            return true;
        };
        current.push(part);
        if std::fs::symlink_metadata(&current)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return true;
        }
    }
    false
}

fn ensure_real_directory(path: &std::path::Path) -> std::io::Result<()> {
    let mut current = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            std::path::Component::RootDir => current.push(std::path::MAIN_SEPARATOR.to_string()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "generated export path contains a parent component",
                ));
            }
            std::path::Component::Normal(part) => current.push(part),
        }
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("generated export directory `{}` is not real", current.display()),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&current)?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn build_project_root(file: &str) -> Result<std::path::PathBuf, Vec<Diagnostic>> {
    let entry = std::path::Path::new(file);
    let absolute = if entry.is_absolute() {
        entry.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(entry)
    };
    let directory = absolute
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .to_path_buf();
    let workspace_root = Loader::find_workspace_root_checked(&directory).map_err(|diagnostic| {
        vec![workspace_build_root_diagnostic(&directory, &diagnostic)]
    })?;
    let package_root = Loader::find_manifest_root_checked(&directory).map_err(|diagnostic| {
        vec![workspace_build_root_diagnostic(&directory, &diagnostic)]
    })?;
    Ok(match (workspace_root, package_root) {
        (Some(workspace), Some(package)) if Loader::is_physically_within(&workspace, &package) => {
            package
        }
        (Some(workspace), _) => workspace,
        (None, Some(package)) => package,
        (None, None) => directory,
    })
}

fn resolve_build_grants(file: &str, cli: &[String]) -> Result<Vec<String>, Vec<Diagnostic>> {
    let mut allowed = cli.iter().cloned().collect::<std::collections::BTreeSet<_>>();
    let mut workspace_denies = std::collections::BTreeSet::new();
    let mut workspace_grants = std::collections::BTreeMap::<
        String,
        std::collections::BTreeSet<String>,
    >::new();
    let mut package_name = None;
    // Build policy is rooted at the entry's real location.  A relative entry
    // such as `src/run.jet` must still discover the package/workspace files
    // above the current directory; walking the unresolved relative path
    // silently missed them for a bare `run.jet`.
    let entry = std::path::Path::new(file);
    let absolute = if entry.is_absolute() {
        entry.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(entry)
    };
    let entry_dir = absolute
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let workspace_root = Loader::find_workspace_root_checked(entry_dir).map_err(|diagnostic| {
        vec![workspace_build_root_diagnostic(entry_dir, &diagnostic)]
    })?;
    let workspace_checked = workspace_root
        .as_deref()
        .map(|root| {
            let resolver = jet_driver::Authority::AuthorityResolver::open(root)
                .map_err(|error| vec![workspace_build_root_diagnostic(root, &error.diagnostic())])?;
            let source = resolver
                .resolve_workspace_source()
                .map_err(|error| vec![workspace_build_root_diagnostic(root, &error.workspace_diagnostic())])?
                .ok_or_else(|| {
                    vec![Diagnostic::error(
                        "E3503",
                        format!("build policy in `{}` is malformed", root.display()),
                        "the workspace root disappeared before build policy was selected".to_string(),
                        "restore the workspace declaration before running build code".to_string(),
                        None,
                    )]
                })?;
            resolver
                .revalidate_source(&source)
                .map_err(|error| vec![error.diagnostic()])?;
            Ok((resolver, source))
        })
        .transpose()?;
    let workspace_policy = workspace_checked
        .as_ref()
        .map(|(resolver, source)| load_workspace_build_policy(resolver, source))
        .transpose()?;
    let workspace_path = workspace_checked
        .as_ref()
        .map(|(_, source)| source.path.clone());
    let package_root = Loader::find_manifest_root_checked(entry_dir)
        .map_err(|diagnostic| vec![workspace_build_root_diagnostic(entry_dir, &diagnostic)])?
        .filter(|root| {
            workspace_root
                .as_ref()
                .is_none_or(|workspace| Loader::is_physically_within(workspace, root))
        });
    if package_root.is_none() {
        if let Some((_, diagnostic)) = Loader::stale_manifest_name_diagnostic(entry_dir) {
            return Err(vec![diagnostic]);
        }
    }
    if let Some(dir) = package_root.as_deref() {
        let resolver = jet_driver::Authority::AuthorityResolver::open(dir)
            .map_err(|error| vec![error.diagnostic()])?;
        let member = resolver
            .checked_member(std::path::Path::new("."))
            .map_err(package_policy_diagnostic)?;
        let package = resolver
            .complete_checked_package(member)
            .map_err(package_policy_diagnostic)?;
        package_name = Some(package.facts.name.clone());
        for effect in package.facts.build_allow {
            if let Some(capability) = Comptime::Build::BuildCapability::parse(&effect) {
                allowed.insert(capability.flag().to_string());
            }
        }
    }
    // A nested workspace owns one policy layer. Do not merge an enclosing
    // workspace's ceiling into this root; that would make an outer policy
    // silently decide the inner workspace's authority.
    if let (Some(policy), Some(workspace)) = (workspace_policy, workspace_path.as_deref()) {
        for effect in policy.build_deny {
            if let Some(capability) = Comptime::Build::BuildCapability::parse(&effect) {
                workspace_denies.insert(capability.flag().to_string());
            } else {
                return Err(vec![Diagnostic::error(
                    "E3503",
                    format!("build policy in `{}` is malformed", workspace.display()),
                    format!("unknown build effect `{effect}`"),
                    "use one of the declared build effects in `policy.deny`".to_string(),
                    None,
                )]);
            }
        }
        for (subject, effects) in policy.build_grants {
            let grants = workspace_grants.entry(subject).or_default();
            for effect in effects {
                if let Some(capability) = Comptime::Build::BuildCapability::parse(&effect) {
                    grants.insert(capability.flag().to_string());
                } else {
                    return Err(vec![Diagnostic::error(
                        "E3503",
                        format!("build policy in `{}` is malformed", workspace.display()),
                        format!("unknown build effect `{effect}`"),
                        "use one of the declared build effects in `policy.grants`".to_string(),
                        None,
                    )]);
                }
            }
        }
    }
    let package_name = match package_name {
        Some(name) => name,
        None if workspace_root.is_some() => Loader::authority_name_for_entry(&absolute)
            .map_err(|diagnostic| vec![diagnostic])?,
        None => absolute
            .file_stem()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("app")
            .to_string(),
    };
    if let Some(grants) = workspace_grants.get(&package_name) {
        allowed.extend(grants.iter().cloned());
    }
    for denied in workspace_denies { allowed.remove(&denied); }
    Ok(allowed.into_iter().collect())
}

fn load_workspace_build_policy(
    resolver: &jet_driver::Authority::AuthorityResolver,
    source: &jetpack::WorkspaceFile::WorkspaceSource,
) -> Result<jetpack::Overlay::OverlayPolicy, Vec<Diagnostic>> {
    match jetpack::WorkspaceFile::evaluate_checked_source(source, resolver) {
        Ok(plan) => Ok(plan.overlay_policy),
        Err(diagnostic) => Err(vec![workspace_build_root_diagnostic(
            &source.path,
            &diagnostic,
        )]),
    }
}

fn package_policy_diagnostic(error: jet_driver::Authority::AuthorityError) -> Vec<Diagnostic> {
    match error {
        jet_driver::Authority::AuthorityError::Invalid { path, detail }
            if detail.contains("build") || detail.contains("effect") => {
                vec![Diagnostic::error(
                    "E1221",
                    format!("invalid build policy in `{}`", path.display()),
                    detail,
                    "use `build: { allow: #(FS, Exec) }` with a valid effect tuple".to_string(),
                    None,
                )]
            }
        other => vec![other.diagnostic()],
    }
}

fn production_build_policy() -> Comptime::Build::BuildPolicy {
    let ci = std::env::var("CI")
        .ok()
        .is_some_and(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"));
    if ci {
        Comptime::Build::BuildPolicy::ci_default()
    } else {
        Comptime::Build::BuildPolicy::local_default()
    }
}

fn compile_bundle_path_opts(
    file: &str,
    mode: Sema::CompileMode,
    freestanding: bool,
    allow_impure: bool,
    web_target: bool,
    cross_target: Option<&str>,
) -> Result<CompileOutput, Vec<Diagnostic>> {
    with_compiler_stack(|| {
        Driver::compile_bundle_path_opts(
            file,
            mode,
            freestanding,
            allow_impure,
            web_target,
            cross_target,
        )
    })
}

/// Like `compile_with_path` but for `jet build --target=web` (D-WEBBACKEND1 M2).
pub fn compile_web(file: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts(file, Sema::CompileMode::Run, false, false, true, None)
}

/// Like `compile_with_path` but for `jet build --target=plugin` (D-PLUGIN1=B /
/// D-DEP-WASM1=A, c81). `CompileMode::Check` — a plugin package has no single
/// `fn run` entry point (D-ILE1: it's a library-shaped export surface, not an
/// executable), so the "no `run`" requirement (E0101, `Run`/`Eval`-only) never
/// applies here; every other check still runs in full.
pub fn compile_plugin(file: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    with_compiler_stack(|| {
        Driver::compile_bundle_path_opts_plugin(
            file,
            Sema::CompileMode::Check,
            Some(Syntax::TARGET_PLUGIN),
        )
    })
}

/// Compile a package's selected `Library` output. Library-shaped sources do
/// not need an executable `fn run`; the native build adapter consumes the
/// checked projections on `CompileOutput`.
pub fn compile_library(
    file: &str,
    output: Option<&str>,
) -> Result<CompileOutput, Vec<Diagnostic>> {
    with_compiler_stack(|| {
        Driver::compile_bundle_path_opts_library(file, Sema::CompileMode::Check, output)
    })
}

/// D-DBG3 step 2 (dap-debugger): compile for the native `jet debug` backend — a
/// normal build with `debug_linemap = true`, so the generated Rust carries the
/// `// jet:line N` table `crates/jet-debug/src/LineMap.rs` reads back.
pub fn compile_for_debug(file: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    with_compiler_stack(|| {
        Driver::compile_bundle_path_opts_dbg(
            file,
            Sema::CompileMode::Run,
            false,
            false,
            false,
            true,
            None,
        )
    })
}

/// c-devserver (owner-directed 2026-07-01): `jet dev <file>` when `file`
/// defines a top-level `fn dev()` — a normal native compile, but with `dev()`
/// swapped in as the program's real entry point instead of `run()` (see
/// `Driver::compile_bundle_path_with_entry`).
pub fn compile_with_entry(file: &str, entry_fn: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    with_compiler_stack(|| Driver::compile_bundle_path_with_entry(file, entry_fn))
}

/// Compile one explicitly addressed runnable Output.
pub fn compile_with_output(file: &str, output: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    with_compiler_stack(|| Driver::compile_bundle_path_output(file, output))
}

pub fn compile_output_with_options(
    file: &str,
    output: &str,
    freestanding: bool,
    allow_impure: bool,
    web_target: bool,
    plugin_target: bool,
    cross_target: Option<&str>,
) -> Result<CompileOutput, Vec<Diagnostic>> {
    with_compiler_stack(|| {
        Driver::compile_bundle_path_output_opts(
            file,
            output,
            freestanding,
            allow_impure,
            web_target,
            plugin_target,
            cross_target,
        )
    })
}

/// In-memory web-target compile (used by integration tests).
pub fn compile_web_with_path(src: &str, file: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    with_compiler_stack(|| {
        Driver::compile_src_with_options(
            src,
            file,
            Sema::CompileMode::Run,
            Driver::CompileSrcOptions { web_target: true },
        )
    })
}

/// Resolve native C-library link args for a built program (S59 / E2-M14),
/// surfacing E3201 when a library cannot be located via hangar or pkg-config.
/// Build/run paths call this AFTER a successful compile; codegen and front-end
/// checks do not, keeping link discovery out of semantic checking (I3).
pub fn resolve_c_links(file: &str) -> Result<Vec<String>, Vec<Diagnostic>> {
    resolve_c_links_for_target(file, None)
}

pub fn resolve_c_links_for_target(
    file: &str,
    target: Option<&str>,
) -> Result<Vec<String>, Vec<Diagnostic>> {
    with_compiler_stack(|| {
        let bundle = Loader::load_entry_with_overlay(file, None, false)?;
        if !bundle.cffi.links_c() {
            return Ok(Vec::new());
        }
        match target {
            Some(target) => {
                crate::CFFI::rustc_link_args_for_target(&bundle.cffi, &bundle.project_root, target)
            }
            None => crate::CFFI::rustc_link_args(&bundle.cffi, &bundle.project_root),
        }
    })
}

/// Compile for `jet test`: optional `main`, at least one test block required.
pub fn compile_tests_with_path(
    src: &str,
    file: &str,
) -> Result<(String, Option<FFI::FfiLink>), Vec<Diagnostic>> {
    compile_tests_with_path_cov(src, file, false)
}

/// Compile for `jet test`, with optional `jet test --coverage`
/// instrumentation. `coverage = false` produces the historical, uninstrumented
/// harness.
pub fn compile_tests_with_path_cov(
    src: &str,
    file: &str,
    coverage: bool,
) -> Result<(String, Option<FFI::FfiLink>), Vec<Diagnostic>> {
    let _ = src;
    with_compiler_stack(|| Driver::compile_tests(file, coverage))
}

/// D-TESTKIT1=A (gap #1): compile for `jet fuzz <file> [<name>]`.
pub use Driver::FuzzCompileError;

pub fn compile_fuzz_with_path(
    file: &str,
    test_name: Option<&str>,
) -> Result<(String, Option<FFI::FfiLink>), FuzzCompileError> {
    with_compiler_stack(|| Driver::compile_fuzz(file, test_name))
}

/// D-BENCH1: compile for `jet bench` when the file has `#Bench` blocks —
/// optional `main`, bodies type-checked in `Bench` mode, then lowered to the
/// timing harness.
pub fn compile_benches_with_path(
    file: &str,
) -> Result<(String, Option<FFI::FfiLink>), Vec<Diagnostic>> {
    with_compiler_stack(|| Driver::compile_benches(file))
}

/// D-BENCH1: does this entry file declare any `#Bench` blocks? `jet bench`
/// uses per-region timing when it does, and falls back to whole-program timing
/// otherwise. A load failure returns `false` so the caller surfaces the real
/// compile error on its normal path.
pub fn has_bench_blocks(file: &str) -> bool {
    with_compiler_stack(|| match Loader::load_entry_with_overlay(file, None, false) {
        Ok(bundle) => bundle
            .modules
            .iter()
            .flat_map(|module| module.items.iter())
            .any(|i| matches!(i, AST::Item::Bench(_))),
        Err(_) => false,
    })
}

/// Does the entry file declare any `#Test` block? `jet test` runs the test
/// harness when it does and skips it (running doctests only) when it doesn't, so
/// a file with only doctests is still testable. A load failure returns `true` so
/// the caller surfaces the real compile error on the normal harness path.
pub fn has_test_blocks(file: &str) -> bool {
    with_compiler_stack(|| match Loader::load_entry_with_overlay(file, None, false) {
        Ok(bundle) => bundle
            .modules
            .iter()
            .flat_map(|module| module.items.iter())
            .any(|i| matches!(i, AST::Item::Test(_))),
        Err(_) => true,
    })
}

/// D-COV1: every user function the `jet test --coverage` probes can record, as
/// `(name, 1-based line)`. Mirrors the probe set: free functions, inherent
/// methods, and trait-impl methods in the entry file (`run` is excluded — it is
/// never probed). The runner diffs the recorded hit lines against this set to
/// report per-function / per-line coverage.
pub fn coverable_functions(file: &str) -> Vec<(String, usize)> {
    with_compiler_stack(|| coverable_functions_inner(file))
}

fn coverable_functions_inner(file: &str) -> Vec<(String, usize)> {
    let bundle = match Loader::load_entry_with_overlay(file, None, false) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    let entry = &bundle.modules[bundle.entry];
    let src = &entry.source;
    let line_of = |off: usize| {
        src[..off.min(src.len())]
            .bytes()
            .filter(|&b| b == b'\n')
            .count()
            + 1
    };
    let mut out = Vec::new();
    for item in &entry.items {
        match item {
            AST::Item::Func(f) if f.name != "run" => {
                out.push((f.name.clone(), line_of(f.name_span.start)));
            }
            AST::Item::Struct(s) => {
                for m in &s.methods {
                    out.push((format!("{}.{}", s.name, m.name), line_of(m.name_span.start)));
                }
                for b in &s.trait_impls {
                    for m in &b.methods {
                        out.push((format!("{}.{}", s.name, m.name), line_of(m.name_span.start)));
                    }
                }
            }
            AST::Item::Enum(e) => {
                for m in &e.methods {
                    out.push((format!("{}.{}", e.name, m.name), line_of(m.name_span.start)));
                }
                for b in &e.trait_impls {
                    for m in &b.methods {
                        out.push((format!("{}.{}", e.name, m.name), line_of(m.name_span.start)));
                    }
                }
            }
            AST::Item::Impl(i) => {
                for m in &i.methods {
                    out.push((
                        format!("{}.{}", i.type_name, m.name),
                        line_of(m.name_span.start),
                    ));
                }
            }
            _ => {}
        }
    }
    out
}

fn compile_with_mode(
    src: &str,
    file: &str,
    mode: Sema::CompileMode,
) -> Result<CompileOutput, Vec<Diagnostic>> {
    with_compiler_stack(|| Driver::compile_src(src, file, mode))
}

/// Back-compat: compile and return only Rust (drops lints).
pub fn compile_rust(src: &str) -> Result<String, Vec<Diagnostic>> {
    compile(src).map(|o| o.rust)
}

pub use Comptime::{CtReport, CtValue};
pub use Diagnostics::render_all as render_diagnostics;
pub use Diagnostics::{render_all_colored, render_all_json, render_all_linked};
pub use Sema::check_pure_program_root;

/// Pretty-print source to canonical Jet style (M6/S44).
pub fn format_source(src: &str) -> Result<String, Vec<Diagnostic>> {
    with_compiler_stack(|| Formatter::format_source(src))
}

/// Front-end check for one document (LSP / editor integration).
pub fn check_document(path: &str, text: &str) -> Vec<Diagnostic> {
    with_compiler_stack(|| LSP::check_document(path, text))
}

/// S60 / D-PURE1 (E2-M16): evaluate a pure Jet program via the comptime
/// interpreter and return the value `main()` returns as a `CtValue`.
/// Stdout is captured but not returned; callers render with `render_pretty()`
/// (human) or `to_json()` (machine/`--json`).
///
/// Returns `Err` diagnostics (E3401/E0952/E0953) on failure.
pub fn eval_pure_program_value(src: &str, file: &str) -> Result<CtValue, Vec<Diagnostic>> {
    with_compiler_stack(|| eval_pure_program_value_inner(src, file))
}

fn eval_pure_program_value_inner(src: &str, file: &str) -> Result<CtValue, Vec<Diagnostic>> {
    use std::collections::HashMap;

    let (toks, lex_diags) = Lexer::lex(src);
    if !lex_diags.is_empty() {
        return Err(lex_diags);
    }
    let prog = Parser::parse(&toks)?;

    let func_map: HashMap<String, &AST::Func> = prog
        .items
        .iter()
        .filter_map(|item| {
            if let AST::Item::Func(f) = item {
                Some((f.name.clone(), f))
            } else {
                None
            }
        })
        .collect();

    let main_fn = func_map.get("run").ok_or_else(|| {
        vec![Diagnostics::Diagnostic::error(
            "E3401",
            "no `run` function found for `jet eval`".to_string(),
            "pure evaluation needs a `fn run() =[]=>` entry point".to_string(),
            "add `fn run() =[]=> { … }` to the program".to_string(),
            None,
        )]
    })?;

    let base_dir = std::path::Path::new(file)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let mut sink = Comptime::DevSink::new();
    let value =
        Comptime::run_main_value(main_fn, &func_map, base_dir, &mut sink).map_err(|d| vec![d])?;
    Ok(value)
}

/// S60 / D-PURE1 (E2-M16): evaluate a pure Jet program via the comptime
/// interpreter and return its output as a stable JSON string. The program's
/// `run()` function is interpreted using the comptime engine; any print calls
/// are captured; the captured output is returned as a JSON string value.
///
/// Returns `Err` diagnostics (E3401/E0952/E0953) on failure.
pub fn eval_pure_program(src: &str, file: &str) -> Result<String, Vec<Diagnostic>> {
    with_compiler_stack(|| eval_pure_program_inner(src, file))
}

fn eval_pure_program_inner(src: &str, file: &str) -> Result<String, Vec<Diagnostic>> {
    use std::collections::HashMap;

    let (toks, lex_diags) = Lexer::lex(src);
    if !lex_diags.is_empty() {
        return Err(lex_diags);
    }
    let prog = Parser::parse(&toks)?;

    // Collect functions into a map for the comptime evaluator.
    let func_map: HashMap<String, &AST::Func> = prog
        .items
        .iter()
        .filter_map(|item| {
            if let AST::Item::Func(f) = item {
                Some((f.name.clone(), f))
            } else {
                None
            }
        })
        .collect();

    let main_fn = func_map.get("run").ok_or_else(|| {
        vec![Diagnostics::Diagnostic::error(
            "E3401",
            "no `run` function found for `jet eval`".to_string(),
            "pure evaluation needs a `fn run() =[]=>` entry point".to_string(),
            "add `fn run() =[]=> { … }` to the program".to_string(),
            None,
        )]
    })?;

    // Run main() via the comptime engine with a dev sink capturing print output.
    let base_dir = std::path::Path::new(file)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let mut sink = Comptime::DevSink::new();
    let program = Comptime::ProgramInfo::empty();
    Comptime::run_main(main_fn, &func_map, base_dir, &mut sink, &program).map_err(|d| vec![d])?;
    let text = sink.stdout;
    // Render the captured output as a JSON string.
    let json = if text.trim().is_empty() {
        "null".to_string()
    } else {
        // Try to parse as a number or bool for cleaner output; otherwise quote it.
        let trimmed = text.trim();
        if trimmed == "true" || trimmed == "false" {
            trimmed.to_string()
        } else if trimmed.parse::<i64>().is_ok() || trimmed.parse::<f64>().is_ok() {
            trimmed.to_string()
        } else {
            format!("{:?}", trimmed)
        }
    };
    Ok(json)
}
