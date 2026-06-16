//! Module evaluation (computed-modules arc, Stages 2–4): reduces an
//! `env.jet`/`config.jet` file's `module name { ns.path: Type { … } }`
//! contributions to typed values and feeds them through the §6 merge engine
//! (`super::merge`).
//!
//! Two evaluation paths, by field:
//! - **`packages`** reuses the already-tested text-level Pkg-sugar parser
//!   (`merge::parse_package_list`) on the field's source span. The
//!   `default.[ripgrep, fd]` grammar (U6) is static sugar, not a runtime
//!   computation, so this stays a text slice rather than re-deriving the
//!   same rules at the AST level.
//! - **Every other field** runs through `comptime::evaluate` — the M9.5
//!   pure-eval interpreter, extended (this arc) with `if … else` expression
//!   support — so a module field may hold any pure, deterministic
//!   expression, not just a literal.
//!
//! `Item::Module` is otherwise invisible to sema/codegen (a deliberate no-op,
//! see commit 2b3825e), so this module owns the only pass that gives module
//! bodies meaning.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::ast::{
    Contribution, ContribValue, Expr, Func, ImageFieldValue, ImageLit, Item, ModuleDecl, Namespace,
    ServiceEntry, StrPart, SystemFieldValue, SystemLit,
};
use crate::comptime;
use crate::diag::{Diagnostic, Span};
use crate::syntax;

use super::merge::{self, EntryContribution, MergeError, MergedEntry, Scalar};
use super::refspec::{self, ProviderKind, SourceTable};

/// One module's contributions, keyed by `(namespace, path)` so `merge_all`
/// can combine same-keyed contributions from different modules.
#[derive(Debug)]
pub struct EvaluatedModule {
    pub name: String,
    pub entries: Vec<((Namespace, String), EntryContribution)>,
    /// U11: `system.<name>:` contributions, captured for the jetos tier (gap #4
    /// realizes them; gap #5 only field-checks + captures).
    pub systems: Vec<SystemPlan>,
    /// U14: `image.<name>:` contributions, captured for the jetos tier.
    pub images: Vec<ImagePlan>,
}

/// U11: a field-checked `system.<name>: { … }` contribution, captured so the
/// jetos tier (gap #4) can realize it. Pure data — no realize logic here.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemPlan {
    /// The contribution path — the `<name>` in `system.<name>`.
    pub name: String,
    /// U13: the typed target platform, e.g. `linux.x64`.
    pub target: String,
    /// U6: the packages to install, as `Pkg`s (source-qualified).
    pub packages: Vec<merge::Pkg>,
    /// U12: the enabled/typed services, in source order.
    pub services: Vec<ServicePlan>,
    /// U13: the ordered option entries (`net.hostName: laptop`), in source order.
    pub options: Vec<OptionPlan>,
}

/// U12: one captured `Service` record under a `System`'s `services:` map.
#[derive(Debug, Clone, PartialEq)]
pub struct ServicePlan {
    /// The service name (the map key), e.g. `openssh`.
    pub name: String,
    /// The required `enable` flag (U12).
    pub enable: bool,
    /// Any further open-record fields, rendered to display strings, in source
    /// order. (e.g. `ports: [22]`.)
    pub extra: Vec<(String, String)>,
}

/// U13: one captured `options:` entry — a dotted key path and its value, rendered
/// to a display string.
#[derive(Debug, Clone, PartialEq)]
pub struct OptionPlan {
    pub key: String,
    pub value: String,
}

/// U14: a field-checked `image.<name>: { … }` contribution, captured for the
/// jetos tier. `target`/`packages`/`services`/`options` are inherited from the
/// referenced `System` at realize time (gap #4), so they are not stored here.
#[derive(Debug, Clone, PartialEq)]
pub struct ImagePlan {
    /// The contribution path — the `<name>` in `image.<name>`.
    pub name: String,
    /// U14: the source system this image is built from (`from: system.<name>`).
    pub from: String,
    /// U14: the disk-image format (`iso` default / `qcow` / `raw`).
    pub format: String,
    /// U14: an explicit cross-compile target, if any (else inherited from system).
    pub target: Option<String>,
}

/// Parse `src` and evaluate every enabled module it declares. `base_dir`
/// resolves `embed_file` inside contribution expressions, same as ordinary
/// comptime evaluation.
pub fn evaluate_source(src: &str, base_dir: &Path) -> Result<Vec<EvaluatedModule>, Diagnostic> {
    let program = parse_program(src)?;
    evaluate_modules(&program.items, src, base_dir)
}

/// Lex + parse `src` to a `Program`, collapsing any lex/parse failure to a
/// single diagnostic. The module surface is evaluated, not type-checked, here,
/// so the first error is enough to stop.
fn parse_program(src: &str) -> Result<crate::ast::Program, Diagnostic> {
    let (toks, lex_diags) = crate::lexer::lex(src);
    if let Some(d) = lex_diags.into_iter().next() {
        return Err(d);
    }
    crate::parser::parse(&toks).map_err(|mut diags| {
        diags.pop().unwrap_or_else(|| {
            Diagnostic::error("E0000", "parse failed".into(), String::new(), String::new(), None)
        })
    })
}

/// Evaluate every enabled `Item::Module` in `items` (already-parsed).
pub fn evaluate_modules(
    items: &[Item],
    src: &str,
    base_dir: &Path,
) -> Result<Vec<EvaluatedModule>, Diagnostic> {
    let funcs = collect_funcs(items);
    let mut out = Vec::new();
    for item in items {
        let Item::Module(m) = item else { continue };
        if m.disabled {
            continue;
        }
        out.push(evaluate_module(m, src, base_dir, &funcs)?);
    }
    Ok(out)
}

fn collect_funcs(items: &[Item]) -> HashMap<String, &Func> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Func(f) => Some((f.name.clone(), f)),
            _ => None,
        })
        .collect()
}

fn evaluate_module<'a>(
    m: &ModuleDecl,
    src: &str,
    base_dir: &Path,
    funcs: &HashMap<String, &'a Func>,
) -> Result<EvaluatedModule, Diagnostic> {
    let mut entries = Vec::new();
    let mut systems = Vec::new();
    let mut images = Vec::new();
    for c in &m.contributions {
        match (&c.namespace, &c.value) {
            (Namespace::Env, ContribValue::Expr(_)) => {
                let entry = evaluate_env_contribution(c, src, base_dir, funcs)?;
                entries.push(((c.namespace, c.path.clone()), entry));
            }
            (Namespace::System, ContribValue::System(lit)) => {
                systems.push(evaluate_system(&c.path, lit, src, base_dir, funcs)?);
            }
            (Namespace::Image, ContribValue::Image(lit)) => {
                images.push(evaluate_image(&c.path, lit)?);
            }
            // Namespace/value-shape mismatches can't occur: the parser pairs each
            // namespace with its dedicated value parser (see `contribution`).
            _ => unreachable!("contribution namespace/value shape mismatch"),
        }
    }
    Ok(EvaluatedModule {
        name: m.name.clone(),
        entries,
        systems,
        images,
    })
}

fn namespace_type(ns: Namespace) -> &'static str {
    match ns {
        Namespace::Env => syntax::TYPE_ENV,
        Namespace::System => syntax::TYPE_SYSTEM,
        Namespace::Image => syntax::TYPE_IMAGE,
    }
}

fn evaluate_env_contribution(
    c: &Contribution,
    src: &str,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
) -> Result<EntryContribution, Diagnostic> {
    let expected = namespace_type(c.namespace);
    let ContribValue::Expr(value) = &c.value else {
        unreachable!("evaluate_env_contribution called on a non-env contribution")
    };
    // U18: a bare `{ … }` elaborates to the namespace type; an explicit
    // `Env { … }` stays legal. A non-record value (or the wrong type name) is
    // E0966.
    let Expr::StructLit {
        type_name, fields, ..
    } = value
    else {
        return Err(not_a_namespace_literal(expected, value.span()));
    };
    if !type_name.is_empty() && type_name != expected {
        return Err(wrong_namespace_type(expected, type_name, value.span()));
    }

    let mut entry = EntryContribution::default();
    let extern_names = HashSet::new();
    let globals = HashMap::new();
    for (name, _span, value) in fields {
        if name == syntax::SYSTEM_FIELD_PACKAGES {
            entry.packages.extend(extract_packages(value, src)?);
        } else {
            let v = comptime::evaluate(value, funcs, &extern_names, base_dir, &globals)?;
            entry
                .settings
                .entry(name.clone())
                .or_default()
                .push(Scalar::normal(v.jet_show()));
        }
    }
    Ok(entry)
}

/// U11/U12/U13/U18: field-check a `system.<name>: { … }` record and capture it as
/// a `SystemPlan`. Validates that every field is one of the four known `System`
/// fields, that `target` names a known platform, that `services` records carry a
/// `Bool` `enable`, and that `options` is a list of `key: value` entries.
fn evaluate_system(
    path: &str,
    lit: &SystemLit,
    src: &str,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
) -> Result<SystemPlan, Diagnostic> {
    let mut target = None;
    let mut packages = Vec::new();
    let mut services = Vec::new();
    let mut options = Vec::new();
    for field in &lit.fields {
        match &field.value {
            SystemFieldValue::Platform { os, arch, span } => {
                target = Some(check_platform(os, arch, *span)?);
            }
            SystemFieldValue::Packages(value) => {
                packages.extend(extract_packages(value, src)?);
            }
            SystemFieldValue::Services(entries) => {
                for e in entries {
                    services.push(evaluate_service(e, base_dir, funcs)?);
                }
            }
            SystemFieldValue::Options(entries) => {
                // U13: option values are typed values / identifiers (`halcyon`,
                // `default.fish`) or free-form quoted strings — not computed
                // expressions. Capture the written value by slicing its source
                // span, the same technique `sources:`/`packages:` use.
                for e in entries {
                    let span = e.value_span;
                    options.push(OptionPlan {
                        key: e.key.clone(),
                        value: src[span.start..span.end].trim().to_string(),
                    });
                }
            }
            SystemFieldValue::Other(_) => {
                return Err(unknown_record_field(
                    syntax::TYPE_SYSTEM,
                    &field.name,
                    "`target`, `packages`, `services`, `options`",
                    field.name_span,
                ));
            }
        }
    }
    let target = target.ok_or_else(|| missing_system_target(lit.span))?;
    Ok(SystemPlan {
        name: path.to_string(),
        target,
        packages,
        services,
        options,
    })
}

/// U13: a `target`/cross-compile platform must name a known OS+arch pair.
fn check_platform(os: &str, arch: &str, span: Span) -> Result<String, Diagnostic> {
    let known_arch =
        matches!(arch, _ if arch == syntax::PLATFORM_ARCH_X64 || arch == syntax::PLATFORM_ARCH_ARM64);
    if os == syntax::PLATFORM_OS_LINUX && known_arch {
        Ok(format!("{os}.{arch}"))
    } else {
        Err(unknown_platform(os, arch, span))
    }
}

/// U12: field-check one `Service` record (open record; requires a `Bool`
/// `enable`) and capture it as a `ServicePlan`.
fn evaluate_service(
    entry: &ServiceEntry,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
) -> Result<ServicePlan, Diagnostic> {
    let mut enable = None;
    let mut extra = Vec::new();
    for (name, span, value) in &entry.fields {
        let v = comptime::evaluate(value, funcs, &HashSet::new(), base_dir, &HashMap::new())?;
        if name == syntax::SERVICE_FIELD_ENABLE {
            match v {
                comptime::CtValue::Bool(b) => enable = Some(b),
                _ => return Err(service_enable_not_bool(*span)),
            }
        } else {
            extra.push((name.clone(), v.jet_show()));
        }
    }
    let enable = enable.ok_or_else(|| service_missing_enable(&entry.name, entry.span))?;
    Ok(ServicePlan {
        name: entry.name.clone(),
        enable,
        extra,
    })
}

/// U14/U18: field-check an `image.<name>: { … }` record and capture it as an
/// `ImagePlan`. Requires `from: system.<name>`; `format` defaults to `iso` and
/// must be one of the three known formats; only `target:` may be restated (for
/// cross-compile) — every other inherited field is rejected.
fn evaluate_image(path: &str, lit: &ImageLit) -> Result<ImagePlan, Diagnostic> {
    let mut from = None;
    let mut format = None;
    let mut target = None;
    for field in &lit.fields {
        match &field.value {
            ImageFieldValue::From { system, .. } => from = Some(system.clone()),
            ImageFieldValue::Format { word, span } => format = Some(check_format(word, *span)?),
            ImageFieldValue::Platform { os, arch, span } => {
                target = Some(check_platform(os, arch, *span)?);
            }
            ImageFieldValue::Other(_) => {
                return Err(image_restated_field(&field.name, field.name_span));
            }
        }
    }
    let from = from.ok_or_else(|| image_missing_from(lit.span))?;
    Ok(ImagePlan {
        name: path.to_string(),
        from,
        format: format.unwrap_or_else(|| syntax::IMAGE_FORMAT_ISO.to_string()),
        target,
    })
}

/// U14: an image `format:` must be one of `iso` / `qcow` / `raw`.
fn check_format(word: &str, span: Span) -> Result<String, Diagnostic> {
    if word == syntax::IMAGE_FORMAT_ISO
        || word == syntax::IMAGE_FORMAT_QCOW
        || word == syntax::IMAGE_FORMAT_RAW
    {
        Ok(word.to_string())
    } else {
        Err(image_bad_format(word, span))
    }
}

/// Pull the package list out of a `packages: [ … ]` field by slicing its
/// source text and reusing the tested sugar parser (see module docs).
fn extract_packages(value: &Expr, src: &str) -> Result<Vec<merge::Pkg>, Diagnostic> {
    let Expr::ListLit(_, span) = value else {
        return Err(packages_not_a_list(value.span()));
    };
    let text = &src[span.start..span.end];
    let body = text
        .strip_prefix('[')
        .and_then(|t| t.strip_suffix(']'))
        .unwrap_or(text);
    Ok(merge::parse_package_list(body))
}

/// Merge every evaluated module's contributions through the §6 engine,
/// grouping by `(namespace, path)` first.
pub fn merge_all(
    modules: &[EvaluatedModule],
) -> Result<HashMap<(Namespace, String), MergedEntry>, MergeError> {
    let mut by_key: HashMap<(Namespace, String), Vec<EntryContribution>> = HashMap::new();
    for module in modules {
        for (key, entry) in &module.entries {
            by_key.entry(key.clone()).or_default().push(entry.clone());
        }
    }
    let mut out = HashMap::new();
    for (key, contribs) in by_key {
        out.insert(key, merge::merge_entry(&contribs)?);
    }
    Ok(out)
}

/// The runnable shape of a typed `env.jet`, ready for the CLI run/build path:
/// the named-source table, the package refs to realize (`<source>:<package>`),
/// and the prompt label. Only the `env` namespace is consulted — `system`/`image`
/// are the jetos tiers and have no meaning for `jetpack`.
#[derive(Debug)]
pub struct EnvPlan {
    pub table: SourceTable,
    pub package_refs: Vec<String>,
    pub prompt: Option<String>,
    /// U11: every captured `System` across all evaluated modules, in source order.
    /// The jetos tier (gap #4) realizes these; the dev-shell path ignores them.
    pub systems: Vec<SystemPlan>,
    /// U14: every captured `Image`, validated so each `from` names a known system.
    pub images: Vec<ImagePlan>,
}

/// True when `src` uses the typed `module { … }` surface (U3/U8) rather than
/// the Phase-1 `pkg.*` directive surface. The CLI routes loading on this: a
/// file that parses with at least one module declaration is evaluated through
/// `evaluate_env`; everything else (including text that doesn't parse cleanly)
/// falls back to the directive scanner, which is deliberately tolerant.
pub fn is_module_surface(src: &str) -> bool {
    let (toks, diags) = crate::lexer::lex(src);
    if !diags.is_empty() {
        return false;
    }
    match crate::parser::parse(&toks) {
        Ok(program) => program
            .items
            .iter()
            .any(|item| matches!(item, Item::Module(_))),
        Err(_) => false,
    }
}

/// Evaluate a typed `env.jet` (the `module name { sources:/imports:/env.X: }`
/// surface, U3/U6/U8) into an `EnvPlan`. Sources merge across modules by key
/// (U5); package sugar resolves to `<source>:<package>` refs; the `prompt`
/// scalar becomes the label. `imports: find(…)` is parsed but not yet walked
/// (U4 discovery is a separate chunk).
pub fn evaluate_env(src: &str, base_dir: &Path) -> Result<EnvPlan, Diagnostic> {
    let program = parse_program(src)?;

    // The root `env.jet` plus every file reachable through `imports: find(…)`
    // (U4). Each unit owns its source text (spans index into it) and the dir its
    // relative refs / `embed_file` resolve against.
    let mut units = vec![EvalUnit {
        items: program.items,
        src: src.to_string(),
        base_dir: base_dir.to_path_buf(),
    }];
    let discovered = discover_imports(&units[0], base_dir)?;
    units.extend(discovered);

    let table = build_source_table(&units)?;

    // Evaluate every unit's modules (each against its own source + base dir),
    // then merge all contributions through the §6 engine as one pass — so a
    // discovered module's `env.dev` packages combine with the root's, and a
    // cross-file source/scalar conflict still surfaces as E0967.
    let mut modules = Vec::new();
    for unit in &units {
        modules.extend(evaluate_modules(&unit.items, &unit.src, &unit.base_dir)?);
    }

    // U11/U14: collect every captured System/Image across all modules (source
    // order), then cross-check each image's `from:` against the known systems.
    let mut systems: Vec<SystemPlan> = Vec::new();
    let mut images: Vec<ImagePlan> = Vec::new();
    for module in &modules {
        systems.extend(module.systems.iter().cloned());
        images.extend(module.images.iter().cloned());
    }
    let system_names: Vec<String> = systems.iter().map(|s| s.name.clone()).collect();
    for image in &images {
        if !system_names.contains(&image.from) {
            return Err(image_from_unknown_system(
                &image.name,
                &image.from,
                &system_names,
            ));
        }
    }

    let merged = merge_all(&modules).map_err(|e| merge_error_to_diagnostic(&e))?;

    // Collect the `env`-namespace contributions in a deterministic order so the
    // realized package list is stable across runs (merge_all returns a HashMap).
    let mut env_keys: Vec<(Namespace, String)> = merged
        .keys()
        .filter(|(ns, _)| *ns == Namespace::Env)
        .cloned()
        .collect();
    env_keys.sort_by(|a, b| a.1.cmp(&b.1));

    let mut package_refs = Vec::new();
    let mut prompt = None;
    for key in &env_keys {
        let entry = &merged[key];
        for pkg in &entry.packages {
            package_refs.push(pkg_ref(pkg));
        }
        if prompt.is_none() {
            if let Some(label) = entry.settings.get(syntax::ENV_FIELD_PROMPT) {
                prompt = Some(label.clone());
            }
        }
    }
    Ok(EnvPlan {
        table,
        package_refs,
        prompt,
        systems,
        images,
    })
}

/// One parsed `.jet` file contributing modules: the root `env.jet` and every
/// file discovered through `imports: find(…)` (U4). Spans in `items` index into
/// this unit's own `src`; `base_dir` is the dir the file's relative refs and
/// `embed_file` calls resolve against.
struct EvalUnit {
    items: Vec<Item>,
    src: String,
    base_dir: PathBuf,
}

/// Walk every `imports: find("<dir>")` directive in the root unit's modules,
/// returning one `EvalUnit` per discovered `*.jet` file (U4 import-tree
/// discovery). Discovery is one level deep: a discovered file may not itself
/// import (the liftability law — modules contribute to the merged whole, they
/// don't import each other; violations are E0971).
fn discover_imports(root: &EvalUnit, base_dir: &Path) -> Result<Vec<EvalUnit>, Diagnostic> {
    let mut out = Vec::new();
    for item in &root.items {
        let Item::Module(m) = item else { continue };
        if m.disabled {
            continue;
        }
        for imp in &m.imports {
            let rel = find_dir_arg(imp)?;
            let dir = base_dir.join(&rel);
            for file in list_jet_files(&dir, imp)? {
                let file_src = std::fs::read_to_string(&file)
                    .map_err(|_| find_dir_missing(&dir, imp.span()))?;
                let prog = parse_program(&file_src)?;
                // Liftability law (U4): a discovered module may not import.
                for nested in &prog.items {
                    if let Item::Module(nm) = nested {
                        if !nm.imports.is_empty() {
                            return Err(discovered_module_imports(&file));
                        }
                    }
                }
                let file_base = file
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| base_dir.to_path_buf());
                out.push(EvalUnit {
                    items: prog.items,
                    src: file_src,
                    base_dir: file_base,
                });
            }
        }
    }
    Ok(out)
}

/// Extract the literal directory path from an `imports: find("dir")` directive.
/// Anything else — a non-`find` expression, the wrong arity, or a non-literal
/// (interpolated) argument — is E0969.
fn find_dir_arg(imp: &Expr) -> Result<String, Diagnostic> {
    let Expr::Call(call) = imp else {
        return Err(bad_import_directive(imp.span()));
    };
    if call.name != syntax::BUILTIN_FIND || call.args.len() != 1 {
        return Err(bad_import_directive(imp.span()));
    }
    let Expr::Str(parts, _) = &call.args[0].expr else {
        return Err(bad_import_directive(imp.span()));
    };
    let mut path = String::new();
    for part in parts {
        match part {
            StrPart::Lit(s) => path.push_str(s),
            StrPart::Interp(_) => return Err(bad_import_directive(imp.span())),
        }
    }
    Ok(path)
}

/// List the `*.jet` files directly under `dir`, sorted for determinism. A
/// missing/unreadable directory is E0970.
fn list_jet_files(dir: &Path, imp: &Expr) -> Result<Vec<PathBuf>, Diagnostic> {
    let entries = std::fs::read_dir(dir).map_err(|_| find_dir_missing(dir, imp.span()))?;
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some(syntax::FILE_EXT) {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// Merge every enabled module's `sources:` block — across the root and every
/// discovered unit — into one `(name → upstream)` table (U5: same name +
/// different ref conflicts, E0967). Each `provider@target` ref (U6) is translated
/// to the colon/flake upstream the providers realize (`github:owner/repo/rev`,
/// `path:./local`, `nixpkgs:channel`).
fn build_source_table(units: &[EvalUnit]) -> Result<SourceTable, Diagnostic> {
    let mut maps: Vec<BTreeMap<String, String>> = Vec::new();
    // U9: the provider kind is *inferred*, never declared. We record each
    // source's kind here, keyed by name, as we resolve its target. The §6 merge
    // guarantees a given name resolves to one upstream (else E0967), so the
    // probe result is consistent across units.
    let mut kinds: BTreeMap<String, ProviderKind> = BTreeMap::new();
    for (idx, unit) in units.iter().enumerate() {
        // Spans index into each unit's own source, but the CLI renders against
        // the root `env.jet`. Only the root unit (index 0) can safely carry a
        // span; a discovered file's diagnostic is span-less so it never slices
        // the wrong source.
        let is_root = idx == 0;
        for item in &unit.items {
            let Item::Module(m) = item else { continue };
            if m.disabled {
                continue;
            }
            let mut map = BTreeMap::new();
            for s in &m.sources {
                let ref_text = unit.src[s.ref_span.start..s.ref_span.end].trim();
                let span = if is_root { Some(s.ref_span) } else { None };
                let pref = refspec::classify_provider_ref(ref_text)
                    .map_err(|_| bad_source_ref(ref_text, span))?;
                let upstream = format!(
                    "{}{}{}",
                    pref.provider.label(),
                    syntax::REF_SEPARATOR,
                    pref.target
                );
                // Probe the resolved target against the *declaring file's* dir,
                // so a `path@./local` relative resolves where it was written.
                kinds.insert(s.name.clone(), infer_provider_kind(&pref, &unit.base_dir));
                map.insert(s.name.clone(), upstream);
            }
            maps.push(map);
        }
    }
    let merged = merge::merge_sources(&maps).map_err(|e| merge_error_to_diagnostic(&e))?;
    Ok(SourceTable::from_decls(merged.into_iter().map(|(name, upstream)| {
        let via = kinds.get(&name).copied().unwrap_or_default();
        (name, upstream, via)
    })))
}

/// U9: infer whether a source is realized by the first-party `core` provider or
/// the `nix` compatibility provider from its *resolved target* — no marker is
/// declared. The rule (see syntax-decisions.md U9, unified-ecosystem.md §6): a
/// target carrying a `payload.jet` is a Jet package repo (→ `core`); otherwise it
/// is a nix flake (→ `nix`).
///
/// The probe must never clone a nixpkgs-sized repo just to classify it:
/// - `path@…` stats the directory locally (offline, free) — resolved here to a
///   concrete `Core`/`Nix`;
/// - `nixpkgs@…` is unconditionally `nix` — never probed;
/// - `github@…` is left **`Infer`**: its kind depends on whether the remote
///   repo carries a `payload.jet`, which only a realize-time probe can answer
///   (this pure pass has no offline flag or source cache). `provider::
///   resolve_kind` does the lightweight git peek when realization runs.
fn infer_provider_kind(pref: &refspec::ProviderRef, base_dir: &Path) -> ProviderKind {
    use super::refspec::Source;
    match pref.provider {
        Source::Path => {
            let target = Path::new(&pref.target);
            let dir = if target.is_absolute() {
                target.to_path_buf()
            } else {
                base_dir.join(target)
            };
            if dir.join(syntax::PAYLOAD_FILE).is_file() {
                ProviderKind::Core
            } else {
                ProviderKind::Nix
            }
        }
        // `github@` can't be classified offline-and-free; defer to a realize-time
        // `payload.jet` peek (U9).
        Source::Github => ProviderKind::Infer,
        // `nixpkgs@` is always the nix collection; never probed. (`Named` can't
        // appear in a `provider@target` ref.)
        _ => ProviderKind::Nix,
    }
}

/// Render one merged `Pkg` as a `<source>:<package>` ref. A bare package (the
/// sugar's empty source) resolves against the conventional `default` source.
fn pkg_ref(pkg: &merge::Pkg) -> String {
    let source = if pkg.source.is_empty() {
        syntax::DEFAULT_SOURCE
    } else {
        pkg.source.as_str()
    };
    format!("{}{}{}", source, syntax::REF_SEPARATOR, pkg.name)
}

fn bad_source_ref(ref_text: &str, span: Option<Span>) -> Diagnostic {
    Diagnostic::error(
        "E0968",
        format!("`{ref_text}` isn't a `provider@target` source ref"),
        "a named source resolves to an upstream written as `provider@target` (U6) — `github@owner/repo/rev`, `path@../local`, `nixpkgs@channel`".to_string(),
        "write the ref as `provider@target`, e.g. `github@NixOS/nixpkgs/nixos-24.05`".to_string(),
        span,
    )
}

/// E0969: an `imports:` directive must be `find("<dir>")` with a literal path.
fn bad_import_directive(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0969",
        "an `imports:` directive must be `find(\"<dir>\")`".to_string(),
        "imports auto-discover a directory of modules (U4); the only directive is `find` with a single string-literal path, e.g. `find(\"./modules\")`".to_string(),
        "write `imports: find(\"./modules\")`".to_string(),
        Some(span),
    )
}

/// E0970: `imports: find("<dir>")` points at a directory that doesn't exist.
fn find_dir_missing(dir: &Path, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0970",
        format!("`find` can't read the directory `{}`", dir.display()),
        "`imports: find(\"<dir>\")` walks that directory for `.jet` modules (U4); it must exist relative to this file".to_string(),
        "create the directory, or fix the path so it points at your modules folder".to_string(),
        Some(span),
    )
}

/// E0971: a discovered module imports — forbidden by the liftability law (U4):
/// modules contribute to the merged whole, they don't import each other.
fn discovered_module_imports(file: &Path) -> Diagnostic {
    Diagnostic::error(
        "E0971",
        format!(
            "the discovered module `{}` has its own `imports:`",
            file.display()
        ),
        "a module found by `find(…)` may not import (the liftability law, U4) — modules only contribute to the merged whole, they never import each other; nesting `find` would make composition explode".to_string(),
        "remove the `imports:` from this module; declare all `find(…)` directives in the top-level env.jet".to_string(),
        None,
    )
}

fn not_a_namespace_literal(expected: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0966",
        format!("a module contribution must be a `{expected}` literal"),
        format!(
            "a contribution's value describes its namespace with a typed struct literal, e.g. `env.dev: {expected} {{ … }}`"
        ),
        format!("wrap the value in `{expected} {{ … }}`"),
        Some(span),
    )
}

fn wrong_namespace_type(expected: &str, got: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0966",
        format!("expected a `{expected}` literal here, found `{got}`"),
        format!("a contribution to this namespace must use the matching type `{expected}`"),
        format!("change `{got} {{ … }}` to `{expected} {{ … }}`"),
        Some(span),
    )
}

fn packages_not_a_list(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0966",
        "the `packages` field must be a list literal".to_string(),
        "`packages: [ … ]` lists the packages this contribution adds, using the Pkg sugar (U6)"
            .to_string(),
        "write `packages: [ … ]`".to_string(),
        Some(span),
    )
}

/// E0972: an unknown field on a `System` / `Image` / `Service` record (U11/U14).
fn unknown_record_field(ty: &str, field: &str, known: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0972",
        format!("`{field}` isn't a field of `{ty}`"),
        format!("a `{ty}` has a fixed set of fields: {known}"),
        format!("remove `{field}`, or use one of {known}"),
        Some(span),
    )
}

/// E0973: a `target:` (or cross-compile platform) names an unknown platform (U13).
fn unknown_platform(os: &str, arch: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0973",
        format!("`{os}.{arch}` isn't a platform Jet knows"),
        "U13: a `target` is a typed platform value, not a piece of quoted text — it must be `linux.x64` or `linux.arm64`".to_string(),
        "write `target: linux.x64` or `target: linux.arm64`".to_string(),
        Some(span),
    )
}

/// E0974: a `System` with no `target` field (U11).
fn missing_system_target(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0974",
        "this `System` has no `target`".to_string(),
        "U11: every machine names the platform it runs on with a typed `target` value".to_string(),
        "add `target: linux.x64` (or `linux.arm64`)".to_string(),
        Some(span),
    )
}

/// E0975: a `Service` record's `enable` field is not a yes/no value (U12).
fn service_enable_not_bool(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0975",
        "a service's `enable` must be `true` or `false`".to_string(),
        "U12: a `Service` turns on or off with a yes/no `enable` flag".to_string(),
        "write `enable: true` or `enable: false`".to_string(),
        Some(span),
    )
}

/// E0975: a `Service` record with no `enable` field (U12).
fn service_missing_enable(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0975",
        format!("the service `{name}` has no `enable`"),
        "U12: every `Service` says whether it is on with a required `enable` flag, then any further settings".to_string(),
        format!("add `enable: true` (or `false`) to `{name}`"),
        Some(span),
    )
}

/// E0976: an `Image` `format:` that isn't `iso` / `qcow` / `raw` (U14).
fn image_bad_format(word: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0976",
        format!("`{word}` isn't a disk-image format"),
        "U14: an image is built as one of three formats — `iso`, `qcow`, or `raw`".to_string(),
        "write `format: iso`, `format: qcow`, or `format: raw`".to_string(),
        Some(span),
    )
}

/// E0977: an `Image` with no `from:` field (U14).
fn image_missing_from(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0977",
        "this `Image` has no `from`".to_string(),
        "U14: an image is built from a system — `from: system.<name>` names which one".to_string(),
        "add `from: system.<name>`, e.g. `from: system.halcyon`".to_string(),
        Some(span),
    )
}

/// E0977: an `Image` restates a field it inherits from its system (U14).
fn image_restated_field(field: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0977",
        format!("an image doesn't restate `{field}`"),
        "U14: `packages`, `services`, and `options` are inherited from the system the image is built from — they are written once on the system, never on the image".to_string(),
        format!("remove `{field}` from the image; set it on the system instead. (Only an explicit `target:` may be restated, for cross-compiling.)"),
        Some(span),
    )
}

/// E0978: an `Image` `from:` references a system that no contribution defines (U14).
fn image_from_unknown_system(image: &str, system: &str, known: &[String]) -> Diagnostic {
    let hint = if known.is_empty() {
        "no `system.<name>:` contribution is defined".to_string()
    } else {
        format!("known systems: {}", known.join(", "))
    };
    Diagnostic::error(
        "E0978",
        format!("the image `{image}` is built from an unknown system `{system}`"),
        "U14: `from: system.<name>` must name a `System` defined by some module contribution; the image inherits that system's target, packages, services, and options".to_string(),
        format!("define `system.{system}: {{ … }}`, or point `from:` at an existing system ({hint})"),
        None,
    )
}

/// Wrap a merge conflict (§6) as a user-facing diagnostic (I4).
pub fn merge_error_to_diagnostic(err: &MergeError) -> Diagnostic {
    match err {
        MergeError::SourceConflict { name, a, b } => Diagnostic::error(
            "E0967",
            format!("source `{name}` is declared with two different refs"),
            format!("one module declares `{name}` as `{a}`, another as `{b}` — sources merge by name, so the refs must agree"),
            "make every declaration of this source agree, or rename one of them".to_string(),
            None,
        ),
        MergeError::ScalarConflict { key, values } => Diagnostic::error(
            "E0967",
            format!("`{key}` got conflicting values: {}", values.join(", ")),
            "scalar settings merge to one value; without a priority marker, modules contributing different values can't be reconciled".to_string(),
            "make every module agree on this value, or remove the conflicting contribution".to_string(),
            None,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn base_dir() -> PathBuf {
        std::env::temp_dir()
    }

    #[test]
    fn evaluates_plain_scalar_and_packages() {
        let src = r#"
module dev {
    env.dev: Env {
        packages: [default.[ripgrep, fd], unstable.neovim],
        prompt: "wordstats",
    }
}
"#;
        let modules = evaluate_source(src, &base_dir()).unwrap();
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].name, "dev");
        let (key, entry) = &modules[0].entries[0];
        assert_eq!(key.0, Namespace::Env);
        assert_eq!(key.1, "dev");
        assert_eq!(
            entry.packages,
            vec![
                merge::Pkg::new("default", "ripgrep"),
                merge::Pkg::new("default", "fd"),
                merge::Pkg::new("unstable", "neovim"),
            ]
        );
        assert_eq!(
            entry.settings.get("prompt"),
            Some(&vec![Scalar::normal("wordstats")])
        );
    }

    #[test]
    fn evaluates_computed_scalar_via_if_else() {
        let src = r#"
module dev {
    env.dev: Env {
        prompt: if 3 > 2 { "yes" } else { "no" },
    }
}
"#;
        let modules = evaluate_source(src, &base_dir()).unwrap();
        let (_, entry) = &modules[0].entries[0];
        assert_eq!(
            entry.settings.get("prompt"),
            Some(&vec![Scalar::normal("yes")])
        );
    }

    #[test]
    fn disabled_module_is_skipped() {
        let src = r#"
module _gaming {
    env.gaming: Env {
        prompt: "should not appear",
    }
}
"#;
        let modules = evaluate_source(src, &base_dir()).unwrap();
        assert!(modules.is_empty());
    }

    #[test]
    fn wrong_namespace_type_is_a_pinned_diagnostic() {
        let src = "\nmodule dev {\n    env.dev: System {\n        prompt: \"wrong type\",\n    }\n}\n";
        let err = evaluate_source(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0966");
        let rendered = crate::diag::render_all("env.jet", src, std::slice::from_ref(&err));
        assert_eq!(
            rendered,
            "Error [E0966]: expected a `Env` literal here, found `System`\n  --> env.jet:3:14\n    |\n  3 |     env.dev: System {\n    |              ^^^^^^^^\n Why: a contribution to this namespace must use the matching type `Env`\n Fix: change `System { … }` to `Env { … }`\n"
        );
    }

    /// A fresh, empty directory under the system temp dir, unique per call.
    fn fresh_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("modeval-{tag}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn evaluate_env_builds_plan_from_typed_surface() {
        let src = r#"
module dev {
    sources: { default: github@NixOS/nixpkgs/nixos-24.05 }
    env.dev: Env {
        packages: [default.[ripgrep, fd]],
        prompt: "wordstats",
    }
}
"#;
        let plan = evaluate_env(src, &base_dir()).unwrap();
        assert_eq!(plan.prompt.as_deref(), Some("wordstats"));
        assert_eq!(plan.package_refs, vec!["default:ripgrep", "default:fd"]);
        // The `provider@target` ref is translated to the colon/flake upstream the
        // provider realizes (`github:…#pkg`).
        assert_eq!(
            plan.table.upstream("default"),
            Some("github:NixOS/nixpkgs/nixos-24.05")
        );
    }

    #[test]
    fn github_source_kind_is_left_to_inference() {
        // U9: a `github@…` source can't be classified core-vs-nix at pure
        // evaluation time (it depends on a remote `payload.jet` peek), so the table
        // records `Infer`; `provider::resolve_kind` decides at realize time.
        let src = r#"
module dev {
    sources: { up: github@acme/jet-pkgs/v1 }
    env.dev: Env { packages: [up.hello] }
}
"#;
        let plan = evaluate_env(src, &base_dir()).unwrap();
        assert_eq!(plan.table.provider("up"), ProviderKind::Infer);
        assert_eq!(plan.table.upstream("up"), Some("github:acme/jet-pkgs/v1"));
    }

    #[test]
    fn nixpkgs_source_kind_stays_nix() {
        let src = r#"
module dev {
    sources: { default: nixpkgs@nixpkgs-unstable }
    env.dev: Env { packages: [default.fd] }
}
"#;
        let plan = evaluate_env(src, &base_dir()).unwrap();
        assert_eq!(plan.table.provider("default"), ProviderKind::Nix);
    }

    #[test]
    fn evaluate_env_bare_package_resolves_to_default_source() {
        let src = r#"
module dev {
    sources: { default: nixpkgs@nixpkgs-unstable }
    env.dev: Env { packages: [ripgrep] }
}
"#;
        let plan = evaluate_env(src, &base_dir()).unwrap();
        assert_eq!(plan.package_refs, vec!["default:ripgrep"]);
    }

    #[test]
    fn evaluate_env_rejects_non_provider_source_ref() {
        let src = "\nmodule dev {\n    sources: { default: nixos-24.05 }\n    env.dev: Env { packages: [default.ripgrep] }\n}\n";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0968");
        let rendered = crate::diag::render_all("env.jet", src, std::slice::from_ref(&err));
        assert_eq!(
            rendered,
            "Error [E0968]: `nixos-24.05` isn't a `provider@target` source ref\n  --> env.jet:3:25\n    |\n  3 |     sources: { default: nixos-24.05 }\n    |                         ^^^^^^^^^^^\n Why: a named source resolves to an upstream written as `provider@target` (U6) — `github@owner/repo/rev`, `path@../local`, `nixpkgs@channel`\n Fix: write the ref as `provider@target`, e.g. `github@NixOS/nixpkgs/nixos-24.05`\n"
        );
    }

    #[test]
    fn evaluate_env_conflicting_sources_are_a_merge_error() {
        let src = r#"
module a {
    sources: { default: github@NixOS/nixpkgs/nixos-24.05 }
    env.dev: Env { packages: [default.ripgrep] }
}
module b {
    sources: { default: github@NixOS/nixpkgs/nixos-23.11 }
    env.dev: Env { packages: [default.fd] }
}
"#;
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0967");
    }

    #[test]
    fn merges_packages_across_modules_and_dedupes() {
        let src = r#"
module a {
    env.dev: Env {
        packages: [default.ripgrep],
    }
}
module b {
    env.dev: Env {
        packages: [default.ripgrep, default.fd],
    }
}
"#;
        let modules = evaluate_source(src, &base_dir()).unwrap();
        let merged = merge_all(&modules).unwrap();
        let entry = merged
            .get(&(Namespace::Env, "dev".to_string()))
            .unwrap();
        assert_eq!(
            entry.packages,
            vec![
                merge::Pkg::new("default", "ripgrep"),
                merge::Pkg::new("default", "fd"),
            ]
        );
    }

    #[test]
    fn conflicting_scalar_contributions_are_a_merge_error() {
        let src = r#"
module a {
    env.dev: Env {
        prompt: "one",
    }
}
module b {
    env.dev: Env {
        prompt: "two",
    }
}
"#;
        let modules = evaluate_source(src, &base_dir()).unwrap();
        let err = merge_all(&modules).unwrap_err();
        let diag = merge_error_to_diagnostic(&err);
        assert_eq!(diag.code, "E0967");
        let rendered = crate::diag::render_all("env.jet", src, std::slice::from_ref(&diag));
        assert_eq!(
            rendered,
            "Error [E0967]: `prompt` got conflicting values: one, two\n Why: scalar settings merge to one value; without a priority marker, modules contributing different values can't be reconciled\n Fix: make every module agree on this value, or remove the conflicting contribution\n"
        );
    }

    #[test]
    fn find_discovers_modules_and_merges_their_packages() {
        // U4: a `find("./modules")` import walks the dir, parses each `.jet`, and
        // folds its modules into the same merge — the discovered `jq` joins the
        // root's `ripgrep`, reusing the root-declared `default` source.
        let dir = fresh_dir("find-discovers");
        std::fs::create_dir_all(dir.join("modules")).unwrap();
        std::fs::write(
            dir.join("modules/tools.jet"),
            "module tools { env.dev: Env { packages: [default.jq] } }",
        )
        .unwrap();
        let src = "module dev {\n    sources: { default: nixpkgs@nixpkgs-unstable }\n    imports: find(\"./modules\")\n    env.dev: Env { packages: [default.ripgrep] }\n}\n";
        let plan = evaluate_env(src, &dir).unwrap();
        assert_eq!(plan.package_refs, vec!["default:ripgrep", "default:jq"]);
        assert_eq!(plan.table.upstream("default"), Some("nixpkgs:nixpkgs-unstable"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn find_missing_directory_is_a_pinned_diagnostic() {
        let src = "\nmodule dev {\n    imports: find(\"./nope\")\n    env.dev: Env { packages: [default.ripgrep] }\n}\n";
        let dir = fresh_dir("find-missing");
        let err = evaluate_env(src, &dir).unwrap_err();
        assert_eq!(err.code, "E0970");
        // The span points at the `find(…)` call in the root file.
        let rendered = crate::diag::render_all("env.jet", src, std::slice::from_ref(&err));
        assert!(rendered.contains("Error [E0970]:"), "{rendered}");
        assert!(rendered.contains("3 |     imports: find(\"./nope\")"), "{rendered}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn non_find_import_directive_is_e0969() {
        let src = "\nmodule dev {\n    imports: gather(\"./modules\")\n    env.dev: Env { packages: [default.ripgrep] }\n}\n";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0969");
        let rendered = crate::diag::render_all("env.jet", src, std::slice::from_ref(&err));
        assert_eq!(
            rendered,
            "Error [E0969]: an `imports:` directive must be `find(\"<dir>\")`\n  --> env.jet:3:14\n    |\n  3 |     imports: gather(\"./modules\")\n    |              ^^^^^^\n Why: imports auto-discover a directory of modules (U4); the only directive is `find` with a single string-literal path, e.g. `find(\"./modules\")`\n Fix: write `imports: find(\"./modules\")`\n"
        );
    }

    // ── gap #5: System / Service / Image (U11–U14, U18) ──────────────────

    /// The brief's worked example parses, elaborates (U18 bare `{ … }`), and
    /// field-checks clean, capturing a `SystemPlan` + `ImagePlan` (not discarded).
    #[test]
    fn worked_example_captures_system_and_image() {
        let src = r#"
module halcyon {
    sources: { default: github@NixOS/nixpkgs/nixos-24.05 }
    system.halcyon: {
        target: linux.x64,
        packages: [default.[firefox, btop, ripgrep]],
        services: {
            pipewire: { enable: true },
            openssh: { enable: true, ports: [22] },
        },
        options: [
            net.hostName: halcyon,
            time.timeZone: "Europe/London",
            users.nate.shell: default.fish,
        ],
    }
}
module installer {
    image.halcyon_iso: { from: system.halcyon, format: iso }
}
"#;
        let plan = evaluate_env(src, &base_dir()).unwrap();
        assert_eq!(plan.systems.len(), 1);
        let sys = &plan.systems[0];
        assert_eq!(sys.name, "halcyon");
        assert_eq!(sys.target, "linux.x64");
        assert_eq!(
            sys.packages,
            vec![
                merge::Pkg::new("default", "firefox"),
                merge::Pkg::new("default", "btop"),
                merge::Pkg::new("default", "ripgrep"),
            ]
        );
        assert_eq!(sys.services.len(), 2);
        assert_eq!(sys.services[0].name, "pipewire");
        assert!(sys.services[0].enable);
        assert_eq!(sys.services[1].name, "openssh");
        assert_eq!(sys.services[1].extra, vec![("ports".to_string(), "[22]".to_string())]);
        assert_eq!(
            sys.options,
            vec![
                OptionPlan { key: "net.hostName".into(), value: "halcyon".into() },
                OptionPlan { key: "time.timeZone".into(), value: "\"Europe/London\"".into() },
                OptionPlan { key: "users.nate.shell".into(), value: "default.fish".into() },
            ]
        );
        assert_eq!(plan.images.len(), 1);
        assert_eq!(plan.images[0].name, "halcyon_iso");
        assert_eq!(plan.images[0].from, "halcyon");
        assert_eq!(plan.images[0].format, "iso");
        assert_eq!(plan.images[0].target, None);
    }

    /// I5: the committed `examples/jetpack-typed/system.jet` is the executable
    /// spec for the typed jetos surface — it must field-check clean and capture a
    /// system + image.
    #[test]
    fn committed_system_example_field_checks_clean() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples/jetpack-typed/system.jet");
        let src = std::fs::read_to_string(&path).unwrap();
        let dir = path.parent().unwrap();
        let plan = evaluate_env(&src, dir).unwrap();
        assert_eq!(plan.systems.len(), 1);
        assert_eq!(plan.systems[0].name, "halcyon");
        assert_eq!(plan.images.len(), 1);
        assert_eq!(plan.images[0].from, "halcyon");
    }

    /// U18: an explicit `System { … }` / `Service { … }` / `Image { … }` is still
    /// legal alongside the inferred bare form.
    #[test]
    fn explicit_type_names_still_parse() {
        let src = r#"
module m {
    system.box: System {
        target: linux.arm64,
        services: { sshd: Service { enable: false } },
    }
    image.box_iso: Image { from: system.box }
}
"#;
        let plan = evaluate_env(src, &base_dir()).unwrap();
        assert_eq!(plan.systems[0].target, "linux.arm64");
        assert!(!plan.systems[0].services[0].enable);
        assert_eq!(plan.images[0].format, "iso");
    }

    #[test]
    fn unknown_system_field_is_e0972() {
        let src = "module m { system.s: { target: linux.x64, gpu: true } }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0972");
        let rendered = crate::diag::render_all("config.jet", src, std::slice::from_ref(&err));
        assert_eq!(
            rendered,
            "Error [E0972]: `gpu` isn't a field of `System`\n  --> config.jet:1:43\n    |\n  1 | module m { system.s: { target: linux.x64, gpu: true } }\n    |                                           ^^^\n Why: a `System` has a fixed set of fields: `target`, `packages`, `services`, `options`\n Fix: remove `gpu`, or use one of `target`, `packages`, `services`, `options`\n"
        );
    }

    #[test]
    fn unknown_platform_target_is_e0973() {
        let src = "module m { system.s: { target: windows.x64 } }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0973");
        let rendered = crate::diag::render_all("config.jet", src, std::slice::from_ref(&err));
        assert!(rendered.contains("`windows.x64` isn't a platform"), "{rendered}");
    }

    #[test]
    fn system_without_target_is_e0974() {
        let src = "module m { system.s: { packages: [default.fd] } }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0974");
    }

    #[test]
    fn service_without_enable_is_e0975() {
        let src = "module m { system.s: { target: linux.x64, services: { ssh: { ports: [22] } } } }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0975");
        let rendered = crate::diag::render_all("config.jet", src, std::slice::from_ref(&err));
        assert!(rendered.contains("`ssh` has no `enable`"), "{rendered}");
    }

    #[test]
    fn service_enable_not_bool_is_e0975() {
        let src = "module m { system.s: { target: linux.x64, services: { ssh: { enable: 1 } } } }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0975");
    }

    #[test]
    fn bad_image_format_is_e0976() {
        let src = "module m { system.s: { target: linux.x64 } image.i: { from: system.s, format: dmg } }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0976");
        let rendered = crate::diag::render_all("config.jet", src, std::slice::from_ref(&err));
        assert!(rendered.contains("`dmg` isn't a disk-image format"), "{rendered}");
    }

    #[test]
    fn image_without_from_is_e0977() {
        let src = "module m { image.i: { format: iso } }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0977");
    }

    #[test]
    fn image_restating_inherited_field_is_e0977() {
        let src = "module m { system.s: { target: linux.x64 } image.i: { from: system.s, packages: [default.fd] } }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0977");
        let rendered = crate::diag::render_all("config.jet", src, std::slice::from_ref(&err));
        assert!(rendered.contains("doesn't restate `packages`"), "{rendered}");
    }

    #[test]
    fn image_cross_compile_target_is_allowed() {
        let src = "module m { system.s: { target: linux.x64 } image.i: { from: system.s, target: linux.arm64 } }";
        let plan = evaluate_env(src, &base_dir()).unwrap();
        assert_eq!(plan.images[0].target.as_deref(), Some("linux.arm64"));
    }

    #[test]
    fn image_from_unknown_system_is_e0978() {
        let src = "module m { image.i: { from: system.nope } }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0978");
        let rendered = crate::diag::render_all("config.jet", src, std::slice::from_ref(&err));
        assert!(rendered.contains("unknown system `nope`"), "{rendered}");
    }

    #[test]
    fn discovered_module_that_imports_is_e0971() {
        // Liftability law (U4): a discovered module may not itself import.
        let dir = fresh_dir("find-liftability");
        std::fs::create_dir_all(dir.join("modules")).unwrap();
        std::fs::write(
            dir.join("modules/nested.jet"),
            "module nested {\n    imports: find(\"./more\")\n    env.dev: Env { packages: [default.jq] }\n}\n",
        )
        .unwrap();
        let src = "module dev {\n    imports: find(\"./modules\")\n    env.dev: Env { packages: [default.ripgrep] }\n}\n";
        let err = evaluate_env(src, &dir).unwrap_err();
        assert_eq!(err.code, "E0971");
        let rendered = crate::diag::render_all("env.jet", src, std::slice::from_ref(&err));
        assert!(rendered.contains("Error [E0971]:"), "{rendered}");
        assert!(rendered.contains("liftability law"), "{rendered}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
