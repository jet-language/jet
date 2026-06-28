//! The `env.jet` → `EnvPlan` driver: route the typed `module { … }` surface,
//! discover `imports: find(…)` files (U4), build the merged `(name → upstream)`
//! source table (U5/U6/U9), and fold every module's contributions into the
//! runnable plan.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::Diagnostics::Diagnostic;
use crate::Syntax;
use crate::AST::{Expr, Item, Namespace, StrPart};

use super::super::Merge;
use super::super::RefSpec::{self, ProviderKind, SourceTable};
use super::Diagnostics::{
    bad_import_directive, bad_source_ref, discovered_module_imports, find_dir_missing,
    image_from_unknown_system, merge_error_to_diagnostic,
};
use super::Eval::{evaluate_modules, merge_all, parse_program, pkg_ref};
use super::Types::{EnvPlan, ImagePlan, SystemPlan};

/// True when `src` uses the typed `module { … }` surface (U3/U8) rather than
/// the Phase-1 `pkg.*` directive surface. The CLI routes loading on this: a
/// file that parses with at least one module declaration is evaluated through
/// `evaluate_env`; everything else (including text that doesn't parse cleanly)
/// falls back to the directive scanner, which is deliberately tolerant.
pub fn is_module_surface(src: &str) -> bool {
    let (toks, diags) = crate::Lexer::lex(src);
    if !diags.is_empty() {
        return false;
    }
    match crate::Parser::parse(&toks) {
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
            if let Some(label) = entry.settings.get(Syntax::ENV_FIELD_PROMPT) {
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
    if call.name != Syntax::BUILTIN_FIND || call.args.len() != 1 {
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
        if path.extension().and_then(|e| e.to_str()) == Some(Syntax::FILE_EXT) {
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
                let pref = RefSpec::classify_provider_ref(ref_text)
                    .map_err(|_| bad_source_ref(ref_text, span))?;
                let upstream = format!(
                    "{}{}{}",
                    pref.provider.label(),
                    Syntax::REF_SEPARATOR,
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
    let merged = Merge::merge_sources(&maps).map_err(|e| merge_error_to_diagnostic(&e))?;
    Ok(SourceTable::from_decls(merged.into_iter().map(
        |(name, upstream)| {
            let via = kinds.get(&name).copied().unwrap_or_default();
            (name, upstream, via)
        },
    )))
}

/// U9: infer whether a source is realized by the first-party `core` provider or
/// the `nix` compatibility provider from its *resolved target* — no marker is
/// declared. The rule (see syntax-decisions.md U9, unified-ecosystem.md §6): a
/// target carrying a `pkg.jet` is a Jet package repo (→ `core`); otherwise it
/// is a nix flake (→ `nix`).
///
/// The probe must never clone a nixpkgs-sized repo just to classify it:
/// - `path@…` stats the directory locally (offline, free) — resolved here to a
///   concrete `Core`/`Nix`;
/// - `nixpkgs@…` is unconditionally `nix` — never probed;
/// - `github@…` is left **`Infer`**: its kind depends on whether the remote
///   repo carries a `pkg.jet`, which only a realize-time probe can answer
///   (this pure pass has no offline flag or source cache). `Provider::
///   resolve_kind` does the lightweight git peek when realization runs.
fn infer_provider_kind(pref: &RefSpec::ProviderRef, base_dir: &Path) -> ProviderKind {
    use super::super::RefSpec::Source;
    match pref.provider {
        Source::Path => {
            let target = Path::new(&pref.target);
            let dir = if target.is_absolute() {
                target.to_path_buf()
            } else {
                base_dir.join(target)
            };
            if dir.join(Syntax::PAYLOAD_FILE).is_file() {
                ProviderKind::Core
            } else {
                ProviderKind::Nix
            }
        }
        // `github@` can't be classified offline-and-free; defer to a realize-time
        // `pkg.jet` peek (U9).
        Source::Github => ProviderKind::Infer,
        // `nixpkgs@` is always the nix collection; never probed. (`Named` can't
        // appear in a `provider@target` ref.)
        _ => ProviderKind::Nix,
    }
}
