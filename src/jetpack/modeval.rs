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

use crate::ast::{Contribution, Expr, Func, Item, ModuleDecl, Namespace, StrPart};
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
    for c in &m.contributions {
        let entry = evaluate_contribution(c, src, base_dir, funcs)?;
        entries.push(((c.namespace, c.path.clone()), entry));
    }
    Ok(EvaluatedModule {
        name: m.name.clone(),
        entries,
    })
}

fn namespace_type(ns: Namespace) -> &'static str {
    match ns {
        Namespace::Env => syntax::TYPE_ENV,
        Namespace::System => syntax::TYPE_SYSTEM,
        Namespace::Image => syntax::TYPE_IMAGE,
    }
}

fn evaluate_contribution(
    c: &Contribution,
    src: &str,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
) -> Result<EntryContribution, Diagnostic> {
    let expected = namespace_type(c.namespace);
    let Expr::StructLit {
        type_name, fields, ..
    } = &c.value
    else {
        return Err(not_a_namespace_literal(expected, c.value.span()));
    };
    if type_name != expected {
        return Err(wrong_namespace_type(expected, type_name, c.value.span()));
    }

    let mut entry = EntryContribution::default();
    let extern_names = HashSet::new();
    let globals = HashMap::new();
    for (name, _span, value) in fields {
        if name == "packages" {
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
