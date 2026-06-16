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

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::ast::{Contribution, Expr, Func, Item, ModuleDecl, Namespace};
use crate::comptime;
use crate::diag::{Diagnostic, Span};
use crate::syntax;

use super::merge::{self, EntryContribution, MergeError, MergedEntry, Scalar};

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
