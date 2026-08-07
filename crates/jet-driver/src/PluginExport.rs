//! D-PLUGIN1=B / D-DEP-WASM1=A / D-PLUGIN-EXPORT1=A / D-PLUGIN-VERSION1=A
//! (c81): the driver-layer half of `target: plugin` — resolving the manifest
//! `export:` name, validating the exported `pub fn` surface (v1: `Int`/`Float`
//! scalars only), and the ApiFreeze-based version handshake.
//!
//! Re-grounding note (this card): D-PLUGIN-EXPORT1/D-PLUGIN-VERSION1's ratified
//! text names the retired D-CAP4 `api: stable` freeze machinery as the version
//! contract. That machinery (`CapabilityFreeze.rs`, the manifest `api:` field)
//! was deleted in the D-MEM1/S2 migration — the ratified *intent* (freeze the
//! exported interface, diff for compatibility, reject incompatible changes) is
//! unchanged, so this builds on `Sema::ApiFreeze`'s still-live pub-metadata
//! semver-snapshot mechanism instead: the same `ApiSnapshot`/`fn_signature`
//! machinery a normal library uses (E1218/E2601), just keyed under a
//! `plugin__` namespace so a plugin's frozen interface never collides with an
//! ordinary library package's frozen API in the same project.

use crate::Diagnostics::Diagnostic;
use crate::Sema::ApiFreeze;
use crate::AST::{Item, ProgramBundle};

/// The manifest `export:` field, or the payload/package name, or (no manifest
/// at all) the entry file's stem — in that priority order (D-PLUGIN-EXPORT1=A:
/// "defaults to the package name when omitted").
pub fn resolve_export_name(bundle: &ProgramBundle) -> String {
    if let Some(Ok(mf)) = crate::Manifest::load(&bundle.project_root) {
        if let Ok(facts) = crate::Package::PackageFacts::parse(&mf.raw, "package.jet") {
            for pkg in &facts.packages {
                for t in &pkg.targets {
                    if let crate::Package::Target::Plugin { export } = t {
                        return export.clone().unwrap_or_else(|| pkg.name.clone());
                    }
                }
            }
        }
        return mf.package.name.clone();
    }
    bundle.modules[bundle.entry]
        .path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("plugin")
        .to_string()
}

/// The published-version identity used for the frozen snapshot header — the
/// manifest's `payload.version`, or `"0.0.0"` with no manifest (single-file
/// plugins have no version to track; the interface diff still works, it just
/// always shows the same header).
fn resolve_version(bundle: &ProgramBundle) -> String {
    crate::Manifest::load(&bundle.project_root)
        .and_then(|r| r.ok())
        .map(|mf| mf.package.version)
        .unwrap_or_else(|| "0.0.0".to_string())
}

/// E1260: a plugin's exported `pub fn` isn't Int/Float-only (v1 scope; see
/// `Codegen::Plugin` module doc for why — Bool needs no new work, Text needs
/// the Component Model's memory-based string ABI, a real follow-on).
fn e1260(detail: &str) -> Diagnostic {
    Diagnostic::error(
        "E1260",
        "a plugin's exported function has an unsupported signature".to_string(),
        detail.to_string(),
        "every parameter and the return type must be all `Int` or all `Float` (v1 plugin scope) — narrow the signature, or drop `pub` if this function isn't meant to be called across the plugin boundary".to_string(),
        None,
    )
}

/// Validate the entry module's `pub fn` surface for a `target: plugin` build.
/// Every `pub fn` must be exportable (`Codegen::plugin_export_shape`); a
/// non-conforming one is E1260, not a silent skip (I3/I4 — codegen's own skip
/// is a defensive fallback, this is the real enforcement point).
pub fn validate_export_surface(bundle: &ProgramBundle) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for item in &bundle.modules[bundle.entry].items {
        let Item::Func(f) = item else { continue };
        if !f.is_pub {
            continue;
        }
        if crate::Codegen::plugin_export_shape(f).is_none() {
            diags.push(e1260(&format!(
                "`pub fn {}` isn't all-`Int` or all-`Float` across its parameters and return type",
                f.name
            )));
        }
    }
    diags
}

/// E1257: the plugin's exported interface changed incompatibly since the last
/// build (D-PLUGIN-VERSION1=A). `delta` names what changed — a removed export
/// or a changed signature; adding a new export is always compatible.
fn e1257(delta: &str) -> Diagnostic {
    Diagnostic::error(
        "E1257",
        "this plugin's exported interface changed incompatibly".to_string(),
        format!(
            "a plugin's frozen exported interface is the load-time contract (D-PLUGIN-VERSION1) — {delta}"
        ),
        "restore the removed/changed export, or accept this as an intentional breaking change (delete the stale snapshot under `.jet/cache/api/` to re-freeze)".to_string(),
        None,
    )
}

/// The frozen-snapshot package identity for plugin `export_name` — namespaced
/// so it never collides with an ordinary library package's frozen API in the
/// same project's `.jet/cache/api/` directory.
fn snapshot_package_key(export_name: &str) -> String {
    format!("plugin__{export_name}")
}

/// D-PLUGIN-VERSION1=A: freeze/diff the plugin's exported interface using
/// `Sema::ApiFreeze`'s existing pub-metadata snapshot mechanism (re-grounded
/// from the retired D-CAP4 system — see module doc). Builds the current
/// snapshot from exactly the functions `Codegen::emit_plugin` actually
/// exported, diffs it against the prior frozen snapshot (if any), and — when
/// compatible (no prior snapshot, or nothing removed/changed) — saves the new
/// snapshot as the fresh baseline. Returns `Err` diagnostics on an
/// incompatible change; never touches disk when it does.
pub fn check_and_freeze_version(
    bundle: &ProgramBundle,
    export_name: &str,
) -> Result<(), Vec<Diagnostic>> {
    let package = snapshot_package_key(export_name);
    let version = resolve_version(bundle);
    let mut funcs = Vec::new();
    for item in &bundle.modules[bundle.entry].items {
        let Item::Func(f) = item else { continue };
        if !f.is_pub || crate::Codegen::plugin_export_shape(f).is_none() {
            continue;
        }
        funcs.push(ApiFreeze::FrozenFn {
            name: f.name.clone(),
            signature: ApiFreeze::fn_signature(f),
        });
    }
    funcs.sort();
    let current = ApiFreeze::ApiSnapshot {
        api_version: ApiFreeze::API_SNAPSHOT_VERSION,
        package: package.clone(),
        published_version: version,
        funcs,
    };

    if let Some(prior) = ApiFreeze::load_snapshot(&bundle.project_root, &package) {
        let mut deltas = Vec::new();
        for old_fn in &prior.funcs {
            match current.funcs.iter().find(|f| f.name == old_fn.name) {
                None => deltas.push(format!("export `{}` was removed", old_fn.name)),
                Some(new_fn) if new_fn.signature != old_fn.signature => deltas.push(format!(
                    "export `{}` changed from `{}` to `{}`",
                    old_fn.name, old_fn.signature, new_fn.signature
                )),
                Some(_) => {}
            }
        }
        if !deltas.is_empty() {
            return Err(deltas.iter().map(|d| e1257(d)).collect());
        }
    }

    let _ = ApiFreeze::save_snapshot(&bundle.project_root, &current);
    Ok(())
}
