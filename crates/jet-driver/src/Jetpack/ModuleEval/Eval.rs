//! The env-namespace evaluation core: parse a source to a `Program`, evaluate
//! every enabled `Item::Module`, run each `env.*` field through `comptime` (or
//! the text-level Pkg-sugar parser for `packages:`), and merge the resulting
//! contributions through the §6 engine.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::Comptime;
use crate::Diagnostics::Diagnostic;
use crate::Syntax;
use crate::AST::{ContribValue, Contribution, EnvLit, Expr, Func, Item, ModuleDecl, Namespace};

use super::super::Merge::{self, EntryContribution, MergeError, MergedEntry, Scalar};
use super::DevService::evaluate_dev_service;
use super::Diagnostics::{not_a_namespace_literal, packages_not_a_list, wrong_namespace_type};
use super::System::{evaluate_fleet, evaluate_image, evaluate_system};
use super::Types::{DevServicePlan, EvaluatedModule};

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
pub(super) fn parse_program(src: &str) -> Result<crate::AST::Program, Diagnostic> {
    let (toks, lex_diags) = crate::Lexer::lex(src);
    if let Some(d) = lex_diags.into_iter().next() {
        return Err(d);
    }
    crate::Parser::parse(&toks).map_err(|mut diags| {
        diags.pop().unwrap_or_else(|| {
            Diagnostic::error(
                "E0000",
                "parse failed".into(),
                String::new(),
                String::new(),
                None,
            )
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
    let mut fleets = Vec::new();
    let mut dev_services = Vec::new();
    let mut secrets = Vec::new();
    for c in &m.contributions {
        match (&c.namespace, &c.value) {
            (Namespace::Env, ContribValue::Expr(_)) => {
                let (entry, names) = evaluate_env_contribution(c, src, base_dir, funcs)?;
                entries.push(((c.namespace, c.path.clone()), entry));
                secrets.extend(names);
            }
            (Namespace::Env, ContribValue::Env(lit)) => {
                let (entry, services, names) = evaluate_env_role(lit, src, base_dir, funcs)?;
                entries.push(((c.namespace, c.path.clone()), entry));
                dev_services.extend(services);
                secrets.extend(names);
            }
            (Namespace::System, ContribValue::System(lit)) => {
                systems.push(evaluate_system(&c.path, lit, src, base_dir, funcs)?);
            }
            (Namespace::Image, ContribValue::Image(lit)) => {
                images.push(evaluate_image(&c.path, lit, src, base_dir, funcs)?);
            }
            (Namespace::Fleet, ContribValue::Fleet(lit)) => {
                fleets.push(evaluate_fleet(&c.path, lit, src)?);
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
        fleets,
        dev_services,
        secrets,
    })
}

fn namespace_type(ns: Namespace) -> &'static str {
    match ns {
        Namespace::Env => Syntax::TYPE_ENV,
        Namespace::System => Syntax::TYPE_SYSTEM,
        Namespace::Image => Syntax::TYPE_IMAGE,
        Namespace::Fleet => Syntax::TYPE_FLEET,
    }
}

/// E3402: builtins that perform ambient I/O or network access. A sandboxed
/// package build / module-field evaluation must reach none of them. The fs/net
/// names are future-proofing for the build-from-source path; print/eprint/input
/// are reachable today, so the diagnostic is live (I4).
fn is_ambient_io_builtin(name: &str) -> bool {
    matches!(
        name,
        "read_file"
            | "write_file"
            | "fetch"
            | "http_get"
            | "env_var"
            | "print"
            | "eprint"
            | "input"
            | "read_all_input"
    )
}

/// Scan a field expression for an ambient-I/O call; the first one fires E3402.
pub(super) fn check_build_io(value: &Expr) -> Result<(), Diagnostic> {
    let mut found: Option<(String, crate::Diagnostics::Span)> = None;
    crate::Comptime::walk_calls(value, &mut |name, span| {
        if found.is_none() && is_ambient_io_builtin(name) {
            found = Some((name.to_string(), span));
        }
    });
    match found {
        Some((name, span)) => Err(crate::Sema::e3402(&name, Some(span))),
        None => Ok(()),
    }
}

fn evaluate_env_contribution(
    c: &Contribution,
    src: &str,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
) -> Result<(EntryContribution, Vec<String>), Diagnostic> {
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

    evaluate_env_fields(&fields, src, base_dir, funcs)
}

/// Shared field-loop for both `env.<name>:` producer shapes (the legacy
/// `Expr::StructLit`'s fields and the canonical role-module's `EnvLit::fields`):
/// `packages:` reuses the Pkg-sugar text slice; everything else is a pure
/// comptime scalar setting.
fn evaluate_env_fields(
    fields: &[(String, crate::Diagnostics::Span, Expr)],
    src: &str,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
) -> Result<(EntryContribution, Vec<String>), Diagnostic> {
    let mut entry = EntryContribution::default();
    let mut secrets = Vec::new();
    let extern_names = HashSet::new();
    let globals = HashMap::new();
    for (name, _span, value) in fields {
        if name == Syntax::SYSTEM_FIELD_PACKAGES {
            entry.packages.extend(extract_packages(value, src)?);
        } else if name == Syntax::ENV_FIELD_SECRETS {
            // U13: `secrets: ["name", …]` — a plain list of strings, no Pkg
            // sugar. Evaluated as an ordinary comptime expression; anything
            // that isn't a `[String]` is captured as a scalar setting instead
            // (E1242-style "wrong shape" surfaces at env-entry validation,
            // not here — this stays a pure capture step, no field-check).
            check_build_io(value)?;
            let v = Comptime::evaluate(value, funcs, &extern_names, base_dir, &globals)?;
            match names_from(&v) {
                Some(names) => secrets.extend(names),
                None => {
                    entry
                        .settings
                        .entry(name.clone())
                        .or_default()
                        .push(Scalar::normal(v.jet_show()));
                }
            }
        } else {
            check_build_io(value)?;
            let v = Comptime::evaluate(value, funcs, &extern_names, base_dir, &globals)?;
            entry
                .settings
                .entry(name.clone())
                .or_default()
                .push(Scalar::normal(v.jet_show()));
        }
    }
    Ok((entry, secrets))
}

/// U13: `[String]` → `Vec<String>`, or `None` if `v` isn't a list of strings
/// (caller falls back to capturing it as an opaque scalar setting).
fn names_from(v: &crate::Comptime::CtValue) -> Option<Vec<String>> {
    let crate::Comptime::CtValue::List(xs) = v else {
        return None;
    };
    xs.iter()
        .map(|x| match x {
            crate::Comptime::CtValue::Str(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

/// U12/D-JPK-MODBODY1=A: evaluate a canonical `env.<name>: { … }` role-module
/// body (`EnvLit`) — the same scalar/package fields as the legacy form, plus a
/// dev-supervised `services:` map, field-checked and captured as
/// `DevServicePlan`s (distinct from the jetos `system.*.services` capture).
fn evaluate_env_role(
    lit: &EnvLit,
    src: &str,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
) -> Result<(EntryContribution, Vec<DevServicePlan>, Vec<String>), Diagnostic> {
    let (entry, secrets) = evaluate_env_fields(&lit.fields, src, base_dir, funcs)?;
    let mut services = Vec::new();
    for s in &lit.services {
        services.push(evaluate_dev_service(s, base_dir, funcs)?);
    }
    Ok((entry, services, secrets))
}

/// Pull the package list out of a `packages: [ … ]` field by slicing its
/// source text and reusing the tested sugar parser (see module docs).
pub(super) fn extract_packages(value: &Expr, src: &str) -> Result<Vec<Merge::Pkg>, Diagnostic> {
    let Expr::ListLit(_, span) = value else {
        return Err(packages_not_a_list(value.span()));
    };
    let text = &src[span.start..span.end];
    let body = text
        .strip_prefix('[')
        .and_then(|t| t.strip_suffix(']'))
        .unwrap_or(text);
    Ok(Merge::parse_package_list(body))
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
        out.insert(key, Merge::merge_entry(&contribs)?);
    }
    Ok(out)
}

/// Render one merged `Pkg` as a `<source>:<package>` ref. A bare package (the
/// sugar's empty source) resolves against the conventional `default` source.
pub fn pkg_ref(pkg: &Merge::Pkg) -> String {
    let source = if pkg.source.is_empty() {
        Syntax::DEFAULT_SOURCE
    } else {
        pkg.source.as_str()
    };
    format!("{}{}{}", source, Syntax::REF_SEPARATOR, pkg.name)
}
