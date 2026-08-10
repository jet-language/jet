//! D-LIB-EXPORT1=C / D-LIB-REUSE1=B: the driver half of a native `Library`.
//!
//! The package model owns the checked manifest fields. This module resolves
//! one selected Library output, validates the public scalar boundary, records
//! the same API-freeze snapshot used by the plugin boundary, and supplies the
//! codegen projection with one checked configuration. It does not load or
//! link anything; those are the CLI/runtime adapters.

use crate::AST::{Item, ProgramBundle};
use crate::Diagnostics::Diagnostic;
use crate::Sema::ApiFreeze;

const SUPPORTED_BINDINGS: &[&str] = &["c", "python", "swift"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryConfig {
    pub name: String,
    pub loadable: bool,
    pub native: bool,
    pub bindings: Vec<String>,
    pub declared_effects: crate::Sema::EffectSet,
}

fn e1341(what: impl Into<String>, why: impl Into<String>, fix: impl Into<String>) -> Diagnostic {
    Diagnostic::error("E1341", what.into(), why.into(), fix.into(), None)
}

fn package_facts(bundle: &ProgramBundle) -> Option<crate::Package::PackageFacts> {
    let manifest = crate::Manifest::load(&bundle.project_root).and_then(|result| result.ok())?;
    crate::Package::PackageFacts::parse(&manifest.raw, "package.jet").ok()
}

/// Resolve the selected `Library` output. A manifest with one Library uses it
/// by default; more than one requires the existing `--output=<name>` address.
pub fn resolve_config(
    bundle: &ProgramBundle,
    explicit_output: Option<&str>,
) -> Result<LibraryConfig, Vec<Diagnostic>> {
    let output = package_facts(bundle).and_then(|facts| {
        let selected = explicit_output
            .and_then(|name| facts.outputs.get(name))
            .or_else(|| {
                let mut libraries = facts
                    .outputs
                    .values()
                    .filter(|output| output.kind == crate::Package::PackageOutputKind::Library);
                let first = libraries.next()?;
                libraries.next().is_none().then_some(first)
            })?;
        (selected.kind == crate::Package::PackageOutputKind::Library).then_some(selected.clone())
    });

    let Some(output) = output else {
        return Err(vec![e1341(
            "this build does not select a Library output",
            "native and loadable artifacts are driven by a checked `outputs: .{ … }` Library fact (D-LIB-EXPORT1=C)",
            "select one Library with `--output=<name>`, or declare exactly one Library output in package.jet",
        )]);
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
    let mut exports = 0usize;
    for item in &bundle.modules[bundle.entry].items {
        let Item::Func(function) = item else { continue };
        if !function.is_pub {
            continue;
        }
        exports += 1;
        if crate::Codegen::library_export_shape(function).is_none() {
            diagnostics.push(Diagnostic::error(
                "E1260",
                "a Library export has an unsupported signature".to_string(),
                format!(
                    "native Library exports currently use one homogeneous `Int` or `Float` scalar shape; `pub fn {}` is outside that boundary",
                    function.name
                ),
                "make every parameter and the return type all `Int` or all `Float`, or drop `pub` if this function is private to the library".to_string(),
                None,
            ));
        }
    }
    if exports == 0 {
        diagnostics.push(e1341(
            "a native Library has no exported functions",
            "D-LIB-EXPORT1=C freezes the public `pub fn` surface and emits it for the foreign caller",
            "mark at least one supported scalar function `pub`, or use a sealed package output instead",
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

/// D-LIB-REUSE1=B / D-LIB-EXPORT1=C: freeze the exact exported surface before
/// a native artifact or loadable payload is emitted.
pub fn check_and_freeze_version(
    bundle: &ProgramBundle,
    name: &str,
) -> Result<(), Vec<Diagnostic>> {
    let package = snapshot_package_key(name);
    let mut funcs = Vec::new();
    for item in &bundle.modules[bundle.entry].items {
        let Item::Func(function) = item else { continue };
        if !function.is_pub || crate::Codegen::library_export_shape(function).is_none() {
            continue;
        }
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
            match current.funcs.iter().find(|function| function.name == old_fn.name) {
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
