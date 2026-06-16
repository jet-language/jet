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
use std::path::Path;

use crate::ast::{Contribution, Expr, Func, Item, ModuleDecl, Namespace};
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
    let (toks, lex_diags) = crate::lexer::lex(src);
    if let Some(d) = lex_diags.into_iter().next() {
        return Err(d);
    }
    let program = crate::parser::parse(&toks).map_err(|mut diags| {
        diags
            .pop()
            .unwrap_or_else(|| Diagnostic::error("E0000", "parse failed".into(), String::new(), String::new(), None))
    })?;
    evaluate_modules(&program.items, src, base_dir)
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
    let (toks, lex_diags) = crate::lexer::lex(src);
    if let Some(d) = lex_diags.into_iter().next() {
        return Err(d);
    }
    let program = crate::parser::parse(&toks).map_err(|mut diags| {
        diags.pop().unwrap_or_else(|| {
            Diagnostic::error("E0000", "parse failed".into(), String::new(), String::new(), None)
        })
    })?;

    let table = build_source_table(&program.items, src)?;

    let modules = evaluate_modules(&program.items, src, base_dir)?;
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

/// Merge every enabled module's `sources:` block into one `(name → upstream)`
/// table (U5: same name + different ref conflicts, E0967). Each `provider@target`
/// ref (U6) is translated to the colon/flake upstream the providers realize
/// (`github:owner/repo/rev`, `path:./local`, `nixpkgs:channel`).
fn build_source_table(items: &[Item], src: &str) -> Result<SourceTable, Diagnostic> {
    let mut maps: Vec<BTreeMap<String, String>> = Vec::new();
    for item in items {
        let Item::Module(m) = item else { continue };
        if m.disabled {
            continue;
        }
        let mut map = BTreeMap::new();
        for s in &m.sources {
            let ref_text = src[s.ref_span.start..s.ref_span.end].trim();
            let pref = refspec::classify_provider_ref(ref_text)
                .map_err(|_| bad_source_ref(ref_text, s.ref_span))?;
            let upstream = format!(
                "{}{}{}",
                pref.provider.label(),
                syntax::REF_SEPARATOR,
                pref.target
            );
            map.insert(s.name.clone(), upstream);
        }
        maps.push(map);
    }
    let merged = merge::merge_sources(&maps).map_err(|e| merge_error_to_diagnostic(&e))?;
    Ok(SourceTable::from_decls(
        merged
            .into_iter()
            .map(|(name, upstream)| (name, upstream, ProviderKind::Nix)),
    ))
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

fn bad_source_ref(ref_text: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0968",
        format!("`{ref_text}` isn't a `provider@target` source ref"),
        "a named source resolves to an upstream written as `provider@target` (U6) — `github@owner/repo/rev`, `path@../local`, `nixpkgs@channel`".to_string(),
        "write the ref as `provider@target`, e.g. `github@NixOS/nixpkgs/nixos-24.05`".to_string(),
        Some(span),
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

    #[test]
    fn evaluate_env_builds_plan_from_typed_surface() {
        let src = r#"
module dev {
    sources: { default: github@NixOS/nixpkgs/nixos-24.05 }
    imports: find("./modules")
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
}
