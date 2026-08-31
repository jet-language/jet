//! D-LIB-EXPORT1=C / D-LIB-REUSE1=B: the driver half of a native `Library`.
//!
//! The package model owns the checked manifest fields. This module resolves
//! one selected Library output, validates the public scalar boundary, records
//! the same API-freeze snapshot used by the plugin boundary, and supplies the
//! codegen projection with one checked configuration. It does not load or
//! link anything; those are the CLI/runtime adapters.

use std::collections::BTreeMap;

use crate::Diagnostics::Diagnostic;
use crate::Sema::ApiFreeze;
use crate::AST::{Item, ProgramBundle};

const SUPPORTED_BINDINGS: &[&str] = &["c", "python", "swift"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryConfig {
    pub name: String,
    pub entry: Option<String>,
    pub loadable: bool,
    pub native: bool,
    pub bindings: Vec<String>,
    pub declared_effects: crate::Sema::EffectSet,
}

fn e1341(what: impl Into<String>, why: impl Into<String>, fix: impl Into<String>) -> Diagnostic {
    Diagnostic::error("E1341", what.into(), why.into(), fix.into(), None)
}

/// Keep the closed foreign symbol namespace collision-free before Codegen
/// publishes the header or native artifact. This is the same ASCII C spelling
/// used by the native emitter and named bindings.
fn c_symbol(name: &str) -> String {
    let mut symbol = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if symbol.is_empty() || symbol.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        symbol.insert(0, '_');
    }
    symbol
}

fn package_facts(
    bundle: &ProgramBundle,
) -> Result<Option<crate::Package::PackageFacts>, Vec<Diagnostic>> {
    match crate::Package::PackageFacts::load(&bundle.project_root) {
        None => Ok(None),
        Some(Ok(facts)) => Ok(Some(facts)),
        Some(Err(error)) => Err(vec![e1341(
            "package manifest is not valid",
            error.to_string(),
            "fix `package.jet` before compiling the Library output",
        )]),
    }
}

/// Resolve the selected `Library` output. A manifest with one Library uses it
/// by default; more than one requires the existing `--output=<name>` address.
pub fn resolve_config(
    bundle: &ProgramBundle,
    explicit_output: Option<&str>,
) -> Result<LibraryConfig, Vec<Diagnostic>> {
    let Some(facts) = package_facts(bundle)? else {
        return Err(vec![e1341(
            "this build does not select a Library output",
            "native and loadable artifacts are driven by a checked `outputs: .{ … }` Library fact (D-LIB-EXPORT1=C)",
            "select one Library with `--output=<name>`, or declare exactly one Library output in package.jet",
        )]);
    };
    let library_names = facts
        .outputs
        .iter()
        .filter(|(_, output)| output.kind == crate::Package::PackageOutputKind::Library)
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    let output = match explicit_output {
        Some(name) => {
            let Some(output) = facts.outputs.get(name) else {
                return Err(vec![e1341(
                    format!("Library output `{name}` does not exist"),
                    "`--output` addresses the checked name in the package output model",
                    format!(
                        "choose a declared Library output: {}",
                        if library_names.is_empty() {
                            "none".to_string()
                        } else {
                            library_names.join(", ")
                        }
                    ),
                )]);
            };
            if output.kind != crate::Package::PackageOutputKind::Library {
                return Err(vec![e1341(
                    format!("output `{name}` is not a Library"),
                    "`jet build --lib` publishes only outputs of kind `Library`",
                    format!(
                        "choose a declared Library output: {}",
                        if library_names.is_empty() {
                            "none".to_string()
                        } else {
                            library_names.join(", ")
                        }
                    ),
                )]);
            }
            output.clone()
        }
        None => match library_names.as_slice() {
            [] => {
                return Err(vec![e1341(
                    "this build does not select a Library output",
                    "native and loadable artifacts are driven by a checked `outputs: .{ … }` Library fact (D-LIB-EXPORT1=C)",
                    "select one Library with `--output=<name>`, or declare exactly one Library output in package.jet",
                )]);
            }
            [name] => facts
                .outputs
                .get(name)
                .expect("Library name came from package outputs")
                .clone(),
            names => {
                return Err(vec![e1341(
                    "multiple Library outputs require an explicit selection",
                    "a native Library build must publish exactly one checked package output",
                    format!("run `jet build --lib --output <name>`; candidates: {}", names.join(", ")),
                )]);
            }
        },
    };
    let bindings = output.binding_languages();
    if let Some(language) = bindings
        .iter()
        .find(|language| !SUPPORTED_BINDINGS.contains(&language.as_str()))
    {
        return Err(vec![e1341(
            format!("a Library requests an unsupported `{language}` binding"),
            "D-LIB-EXPORT1=C defines one closed set of generated foreign projections; unknown names cannot be emitted safely",
            "use `c`, `python`, or `swift`, or remove the unsupported binding",
        )]);
    }
    if !bindings.is_empty() && !output.is_native() {
        return Err(vec![e1341(
            "a Library requests foreign bindings without `native: true`",
            "C, Python, and Swift bindings project the native Library boundary; a sealed Jet package has no foreign symbol surface",
            "add `native: true` to the selected Library output",
        )]);
    }
    let declared_effects = declared_effects(bundle);
    let name = output.name.clone();
    let loadable = output.is_loadable();
    let native = output.is_native();
    Ok(LibraryConfig {
        name,
        entry: output.entry.clone(),
        loadable,
        native,
        bindings,
        declared_effects,
    })
}

/// D-EFF1 / D-LIB-DYNTRUST1=A: preserve the compiler's checked Core effect
/// facts in the loadable stamp. The driver reads `used_core`, not generated
/// Rust, so policy is not re-inferred by an engine or loader.
pub fn declared_effects(bundle: &ProgramBundle) -> crate::Sema::EffectSet {
    let mut effects = crate::Sema::EffectSet::new();
    for usage in &bundle.used_core {
        let usage = usage
            .strip_prefix("__core_intrinsic::")
            .or_else(|| usage.strip_prefix("__core_source::"));
        let Some(usage) = usage else { continue };
        let Some((module, method)) = usage.split_once("::") else {
            continue;
        };
        if let Some(effect) = crate::Sema::core_effect(module, method) {
            effects.insert(effect.name().to_string());
        }
    }
    effects
}

/// E1260 remains the registered scalar-boundary diagnostic for this v1
/// projection. The wording names Library rather than the plugin backend.
pub fn validate_export_surface(bundle: &ProgramBundle) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let surface = crate::Sema::guest_export_surface(bundle);
    let mut exports = 0usize;
    let mut has_text_export = false;
    let mut symbols = BTreeMap::new();
    for export in surface {
        exports += 1;
        let Some(shape) = export.scalar else {
            diagnostics.push(e1341(
                "a Library export has an unsupported signature".to_string(),
                format!(
                    "native Library exports currently use one homogeneous `Int`, `Float`, `Bool`, or `Text` scalar shape; `#Export(c) fn {}` is outside that boundary",
                    export.name
                ),
                "use one supported scalar type for every parameter and the return type, or remove `#Export(c)`".to_string(),
            ));
            continue;
        };
        has_text_export |= shape == crate::Sema::GuestScalar::Text;
        let symbol = c_symbol(&export.name);
        if let Some(previous) = symbols.insert(symbol.clone(), export.name.clone()) {
            diagnostics.push(e1341(
                format!(
                    "Library exports `{previous}` and `{}` with the same C symbol `{symbol}`",
                    export.name
                ),
                "C, Python, and Swift projections must name one native function unambiguously; foreign names outside ASCII collapse to the same C symbol",
                "rename one exported function so every `#Export(c)` function has a unique ASCII C symbol",
            ));
        }
    }
    if has_text_export {
        if let Some(function) = symbols.get("jet_text_free") {
            diagnostics.push(e1341(
                format!(
                    "Library export `{function}` collides with generated C symbol `jet_text_free`"
                ),
                "Text results use `jet_text_free` as the one library-owned allocator release function",
                "rename the export so it does not use `jet_text_free`",
            ));
        }
    }
    if exports == 0 {
        diagnostics.push(e1341(
            "a native Library has no exported functions",
            "D-ADOPT-GUEST1=A publishes the entry module's explicit `#Export(c)` surface",
            "mark at least one supported scalar function `#Export(c)`, or use a sealed package output instead",
        ));
    }
    diagnostics
}

fn resolve_version(bundle: &ProgramBundle) -> String {
    crate::Manifest::load(&bundle.project_root)
        .and_then(|result| result.ok())
        .map(|manifest| manifest.package.version)
        .unwrap_or_else(|| "0.0.0".to_string())
}

fn snapshot_package_key(name: &str) -> String {
    format!("library__{name}")
}

fn e1257(delta: &str) -> Diagnostic {
    Diagnostic::error(
        "E1257",
        "this Library's exported interface changed incompatibly".to_string(),
        format!(
            "a Library's frozen `pub` interface is the load and foreign-call contract (D-LIB-EXPORT1=C) — {delta}"
        ),
        "restore the removed or changed export, or delete the stale `.jet/cache/api/` snapshot to re-freeze this deliberate breaking change".to_string(),
        None,
    )
}

/// D-LIB-REUSE1=B / D-ADOPT-GUEST1=A: freeze the exact exported surface before
/// a native artifact or loadable payload is emitted.
pub fn check_and_freeze_version(bundle: &ProgramBundle, name: &str) -> Result<(), Vec<Diagnostic>> {
    let package = snapshot_package_key(name);
    let mut funcs = Vec::new();
    let surface = crate::Sema::guest_export_surface(bundle);
    for export in surface {
        if export.scalar.is_none() {
            continue;
        }
        let Some(function) = bundle.modules[bundle.entry].items.iter().find_map(|item| {
            let Item::Func(function) = item else { return None };
            (function.name == export.name).then_some(function)
        }) else {
            continue;
        };
        funcs.push(ApiFreeze::FrozenFn {
            name: function.name.clone(),
            signature: ApiFreeze::fn_signature(function),
        });
    }
    funcs.sort();
    let current = ApiFreeze::ApiSnapshot {
        api_version: ApiFreeze::API_SNAPSHOT_VERSION,
        package: package.clone(),
        published_version: resolve_version(bundle),
        funcs,
    };

    if let Some(prior) = ApiFreeze::load_snapshot(&bundle.project_root, &package) {
        let mut deltas = Vec::new();
        for old_fn in &prior.funcs {
            match current
                .funcs
                .iter()
                .find(|function| function.name == old_fn.name)
            {
                None => deltas.push(format!("export `{}` was removed", old_fn.name)),
                Some(new_fn) if new_fn.signature != old_fn.signature => deltas.push(format!(
                    "export `{}` changed from `{}` to `{}`",
                    old_fn.name, old_fn.signature, new_fn.signature
                )),
                Some(_) => {}
            }
        }
        if !deltas.is_empty() {
            return Err(deltas.iter().map(|delta| e1257(delta)).collect());
        }
    }
    let _ = ApiFreeze::save_snapshot(&bundle.project_root, &current);
    Ok(())
}
