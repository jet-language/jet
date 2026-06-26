//! c129 (D-CAP4/D-CAP6/D-CAP8 — ratified): sema drift pass for a frozen public
//! capability API.
//!
//! D-CAP8 resolves every unmarked `Infer` parameter to a concrete capability from
//! body usage. At a `library { api: stable | explicit }` boundary that resolved
//! signature is frozen into durable interface metadata (`.jet/cache/api/<pkg>.api`,
//! written by `jet publish` — see `Publish::ApiFreeze`).
//!
//! This pass runs after `Capability::resolve_capabilities`. For each frozen
//! snapshot it finds, it diffs the current resolved signature of every public
//! function against the frozen one. A param that was frozen as read (no sigil) but
//! now resolves to `~`/`^`/`&` — or any other sigil change — is a **breaking
//! change** to the public contract (D-CAP8: "a later read → `~`/`^`/`&` drift is a
//! breaking-change error, not a silent flip"). That is **E0912**.
//!
//! The freeze gate lives on the write side: a `.api` snapshot exists only because
//! `jet publish` wrote it for an `api: stable|explicit` target (D-CAP6 — inference
//! is the default and never freezes). So this pass is mode-agnostic: a frozen
//! snapshot present ⇒ enforce; none ⇒ nothing to diff (first release / inferred
//! library). This mirrors the D-MIGRATE1 `#PublishedSchema` discipline exactly.
//!
//! I3: all checking here; codegen sees nothing of the freeze.

use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::ApiFreeze::{self, fn_signature};
use crate::AST::Item;
use std::collections::HashMap;
use std::path::Path;

/// Run the capability-freeze drift pass over a module's items. `project_root` is
/// the root of the Jet project (the dir containing `.jet/`). Returns any E0912
/// diagnostics. No-op when no frozen `.api` snapshot exists for the project.
pub fn check_capability_freeze(items: &[Item], project_root: &Path) -> Vec<Diagnostic> {
    let snaps = ApiFreeze::load_all_snapshots(project_root);
    if snaps.is_empty() {
        return Vec::new();
    }

    // Frozen public surface, keyed by function name → (signature, package, version).
    // The public surface is global per project (v1), matching how the SemVer API
    // extractor reads the entry module; if two packages froze the same fn name the
    // last wins — a benign over-approximation that only widens what is checked.
    let mut frozen: HashMap<String, (String, String, String)> = HashMap::new();
    for s in &snaps {
        for f in &s.funcs {
            frozen.insert(
                f.name.clone(),
                (f.signature.clone(), s.package.clone(), s.published_version.clone()),
            );
        }
    }

    let mut diags = Vec::new();
    check_items(items, &frozen, &mut diags);
    diags
}

/// Diff each public function against the frozen contract, descending into inline
/// `module { … }` bodies (the public surface mirror of
/// `Publish::ApiFreeze::snapshot_from_items`).
fn check_items(
    items: &[Item],
    frozen: &HashMap<String, (String, String, String)>,
    diags: &mut Vec<Diagnostic>,
) {
    for item in items {
        match item {
            Item::Func(f) if f.is_pub => {
                let Some((frozen_sig, package, version)) = frozen.get(&f.name) else {
                    continue; // not in the frozen contract (new fn — additive, fine)
                };
                let current_sig = fn_signature(f);
                if &current_sig == frozen_sig {
                    continue; // unchanged contract
                }
                diags.push(e0912_drift(
                    &f.name,
                    frozen_sig,
                    &current_sig,
                    package,
                    version,
                    f.name_span,
                ));
            }
            Item::CodeModule(m) => {
                if let Some(body) = &m.body {
                    check_items(body, frozen, diags);
                }
            }
            _ => {}
        }
    }
}

/// E0912: a frozen public capability signature drifted since it was published.
fn e0912_drift(
    name: &str,
    frozen_sig: &str,
    current_sig: &str,
    package: &str,
    version: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic::error(
        "E0912",
        format!(
            "the public capability signature of `{}` changed since `{}` froze it at version `{}`",
            name, package, version
        ),
        format!(
            "an `api: stable`/`api: explicit` library freezes each public function's resolved \
             capabilities into its contract. A param that callers could pass by read now demands \
             a stronger capability (`~` edit / `^` take / `&` share), which silently breaks every \
             caller.\n   | was: {}\n   | now: {}",
            frozen_sig, current_sig,
        ),
        format!(
            "if the new capability is intended, this is a breaking change — bump the major \
             version and re-run `jet publish` to re-freeze the contract. Otherwise restore the \
             original signature of `{}` (or add the capability sigil to the published api so the \
             freeze records it).",
            name,
        ),
        Some(span),
    )
}

// ──────────────────────────────────────────────
// Unit tests — diff against a temp snapshot via JET_API_CACHE_DIR.
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AST::{AccessConvention, Func, Param, Type};
    use std::sync::Mutex;

    // The pass reads a process-global env var; serialize tests that set it.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn zero() -> Span {
        Span::new(0, 0)
    }

    fn param(name: &str, conv: AccessConvention, ty: Type) -> Param {
        Param {
            convention: conv,
            name: name.to_string(),
            name_span: zero(),
            ty,
            ty_span: zero(),
            default: None,
        }
    }

    fn pub_fn(name: &str, params: Vec<Param>) -> Item {
        Item::Func(Func {
            is_pub: true,
            name: name.to_string(),
            name_span: zero(),
            type_params: vec![],
            params,
            return_type: None,
            is_view_return: false,
            is_unsafe: false,
            is_pure: false,
            is_sanitizer: false,
            declared_effects: None,
            effect_via: None,
            state_requires: None,
            state_transition: None,
            body: vec![],
        })
    }

    /// Run the pass with one frozen snapshot written into a temp cache dir.
    fn run_with_frozen(frozen_fns: &[&str], items: &[Item]) -> Vec<&'static str> {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!(
            "jet_api_freeze_unit_{}_{}",
            std::process::id(),
            frozen_fns.len() + items.len(),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut text = String::from("api_version = 1\npackage = pkg\npublished_version = 1.0.0\n");
        for f in frozen_fns {
            text.push_str(f);
            text.push('\n');
        }
        std::fs::write(dir.join("pkg.api"), text).unwrap();
        std::env::set_var("JET_API_CACHE_DIR", &dir);
        let diags = check_capability_freeze(items, std::path::Path::new("."));
        std::env::remove_var("JET_API_CACHE_DIR");
        std::fs::remove_dir_all(&dir).ok();
        diags.iter().map(|d| d.code).collect()
    }

    #[test]
    fn unchanged_signature_is_clean() {
        let items = [pub_fn("scale", vec![param("v", AccessConvention::Read, Type::Named("Vec3".into()))])];
        assert!(run_with_frozen(&["fn scale(v: Vec3)"], &items).is_empty());
    }

    #[test]
    fn read_to_write_drift_is_e0912() {
        // Frozen as read (no sigil); now resolves to ~ (write). Breaking.
        let items = [pub_fn("scale", vec![param("v", AccessConvention::Write, Type::Named("Vec3".into()))])];
        assert_eq!(run_with_frozen(&["fn scale(v: Vec3)"], &items), vec!["E0912"]);
    }

    #[test]
    fn read_to_take_drift_is_e0912() {
        let items = [pub_fn("consume", vec![param("v", AccessConvention::Move, Type::Named("Vec3".into()))])];
        assert_eq!(run_with_frozen(&["fn consume(v: Vec3)"], &items), vec!["E0912"]);
    }

    #[test]
    fn read_to_share_drift_is_e0912() {
        let items = [pub_fn("keep", vec![param("v", AccessConvention::Share, Type::Named("Vec3".into()))])];
        assert_eq!(run_with_frozen(&["fn keep(v: Vec3)"], &items), vec!["E0912"]);
    }

    #[test]
    fn new_pub_fn_not_in_contract_is_clean() {
        // Additive: a brand-new public fn is not a break.
        let items = [pub_fn("brandnew", vec![param("v", AccessConvention::Write, Type::Named("Vec3".into()))])];
        assert!(run_with_frozen(&["fn scale(v: Vec3)"], &items).is_empty());
    }

    #[test]
    fn no_frozen_snapshot_is_clean() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("jet_api_freeze_empty_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("JET_API_CACHE_DIR", &dir);
        let items = [pub_fn("scale", vec![param("v", AccessConvention::Write, Type::Named("Vec3".into()))])];
        let diags = check_capability_freeze(&items, std::path::Path::new("."));
        std::env::remove_var("JET_API_CACHE_DIR");
        std::fs::remove_dir_all(&dir).ok();
        assert!(diags.is_empty());
    }
}
