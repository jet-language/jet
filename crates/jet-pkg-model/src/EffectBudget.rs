//! D-EFFBUDGET1 (ratified 2026-07-01): package effect budget.
//!
//! Zero-config: every `jet build` prints a one-line summary of the effects the
//! dependency graph uses, and per-dependency effect provenance is recorded in
//! the lockfile. An `effects: { allow: […], deny: […] }` block in `package.jet`
//! turns on whole-graph enforcement — the build fails naming the exact
//! dependency and offending function when a transitive dependency needs an
//! effect outside the budget. `grants: { "dep": [Effect] }` is the audited
//! per-dependency escape, also recorded in the lockfile. Manifest keys only —
//! no language grammar (§0.4 DO-NOT).
//!
//! Attribution: sema already computes a whole-program per-function effect fixpoint
//! (`Sema::Effects::solve`, keyed by `Sema::effect_key`). This module attributes
//! each function's solved effect set to the package (root, or a dependency)
//! whose module defines it, by matching the module's on-disk path against
//! `ProgramBundle::dep_roots` — the same name→source-root map `Loader` builds
//! for both `deps:` (path/git/provider) entries and hangar-realized
//! `use <pkg>` libraries (U17).

use crate::Diagnostics::{Diagnostic, Span};
use crate::Package::PackageFacts;
use crate::Sema::{effect_covers, parse_effect_name, EffectSet};
use crate::AST::{Item, ProgramBundle};
use std::collections::{BTreeMap, HashMap};

/// One package's (root, or a dependency) aggregated effect set.
#[derive(Debug, Clone)]
pub struct PackageEffects {
    /// `"root"` for the building package itself, else the dependency name.
    pub name: String,
    pub effects: EffectSet,
}

/// Attribute every function's solved effect set (`Sema::check_bundle_with_effect_facts`
/// output) to the package whose module defines it. Functions with no entry in
/// `solved` (never reached / no effects) contribute nothing.
pub fn compute_package_effects(
    bundle: &ProgramBundle,
    solved: &HashMap<String, EffectSet>,
) -> Vec<PackageEffects> {
    let mut by_pkg: BTreeMap<String, EffectSet> = BTreeMap::new();

    for module in &bundle.modules {
        let owner = bundle
            .dep_roots
            .iter()
            .find(|(_, dir)| module.path.starts_with(dir))
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| "root".to_string());
        let out = by_pkg.entry(owner).or_default();
        for item in &module.items {
            collect_item_effects(item, solved, out);
        }
    }

    by_pkg
        .into_iter()
        .map(|(name, effects)| PackageEffects { name, effects })
        .collect()
}

fn collect_item_effects(item: &Item, solved: &HashMap<String, EffectSet>, out: &mut EffectSet) {
    match item {
        Item::Func(f) => {
            if let Some(set) = solved.get(&crate::Sema::effect_key(None, &f.name)) {
                out.extend(set.iter().cloned());
            }
        }
        Item::Impl(im) => {
            for m in &im.methods {
                if let Some(set) =
                    solved.get(&crate::Sema::effect_key(Some(&im.type_name), &m.name))
                {
                    out.extend(set.iter().cloned());
                }
            }
        }
        Item::Struct(s) => {
            for m in &s.methods {
                if let Some(set) = solved.get(&crate::Sema::effect_key(Some(&s.name), &m.name)) {
                    out.extend(set.iter().cloned());
                }
            }
            for block in &s.trait_impls {
                for m in &block.methods {
                    if let Some(set) = solved.get(&crate::Sema::effect_key(Some(&s.name), &m.name))
                    {
                        out.extend(set.iter().cloned());
                    }
                }
            }
        }
        Item::Enum(e) => {
            for m in &e.methods {
                if let Some(set) = solved.get(&crate::Sema::effect_key(Some(&e.name), &m.name)) {
                    out.extend(set.iter().cloned());
                }
            }
        }
        _ => {}
    }
}

/// The always-on one-line `jet build` summary (D-EFFBUDGET1: zero-config,
/// prints on every build).
pub fn summary_line(entries: &[PackageEffects]) -> String {
    let mut all: EffectSet = EffectSet::new();
    for e in entries {
        all.extend(e.effects.iter().cloned());
    }
    if all.is_empty() {
        return "effects: none".to_string();
    }
    let dep_count = entries
        .iter()
        .filter(|e| e.name != "root" && !e.effects.is_empty())
        .count();
    let names: Vec<&str> = all.iter().map(|e| e.as_str()).collect();
    if dep_count == 0 {
        format!("effects: {}", names.join(", "))
    } else {
        format!(
            "effects: {} (across root + {} dependenc{})",
            names.join(", "),
            dep_count,
            if dep_count == 1 { "y" } else { "ies" }
        )
    }
}

/// Per-package effect names, sorted, for lockfile provenance (`LockedPackage.effects`).
pub fn provenance_for(entries: &[PackageEffects], name: &str) -> Vec<String> {
    entries
        .iter()
        .find(|e| e.name == name)
        .map(|e| e.effects.iter().cloned().collect())
        .unwrap_or_default()
}

/// D-EFFBUDGET1 whole-graph enforcement: when `package.jet` declares an `effects:`
/// block, fail the build for any *dependency* (not root — the budget names the
/// supply chain) whose effect set has something outside `allow` or inside
/// `deny`, unless `grants:` covers it for that dependency. Returns E1220 per
/// offending (dependency, effect) pair.
pub fn enforce(entries: &[PackageEffects], manifest: &PackageFacts) -> Vec<Diagnostic> {
    if !manifest.effects_enabled {
        return Vec::new();
    }
    // D-EFFTREE1: allow/deny/grants entries may be ancestor roots (or leaves);
    // coverage (`effect_covers`), not exact membership, decides whether a
    // dependency's solved effect is inside the budget.
    let allow: Option<EffectSet> = manifest
        .effects_allow
        .as_ref()
        .map(|names| names.iter().filter_map(|n| parse_effect_name(n)).collect());
    let deny: EffectSet = manifest
        .effects_deny
        .as_ref()
        .map(|names| names.iter().filter_map(|n| parse_effect_name(n)).collect())
        .unwrap_or_default();
    let grants: HashMap<&str, EffectSet> = manifest
        .grants
        .iter()
        .map(|(dep, effects)| {
            (
                dep.as_str(),
                effects
                    .iter()
                    .filter_map(|n| parse_effect_name(n))
                    .collect::<EffectSet>(),
            )
        })
        .collect();

    let mut diags = Vec::new();
    for pkg in entries {
        if pkg.name == "root" {
            continue;
        }
        let granted = grants.get(pkg.name.as_str());
        for effect in &pkg.effects {
            if let Some(g) = granted {
                if g.iter().any(|b| effect_covers(b, effect)) {
                    continue;
                }
            }
            let outside_allow = allow
                .as_ref()
                .is_some_and(|a| !a.iter().any(|b| effect_covers(b, effect)));
            let inside_deny = deny.iter().any(|b| effect_covers(b, effect));
            if outside_allow || inside_deny {
                diags.push(e1220(&pkg.name, effect));
            }
        }
    }
    diags
}

/// D-EFFBUDGET1: write computed per-package effect provenance (and configured
/// `grants:`) into an existing lockfile's package entries in place. No-op for
/// a package name the lockfile doesn't (yet) list — `jet store fetch` owns adding
/// package entries; this only annotates ones already there.
pub fn update_lock_provenance(
    lock: &mut crate::Lock::LockFile,
    entries: &[PackageEffects],
    manifest: &PackageFacts,
) {
    for pkg in &mut lock.packages {
        let key = if pkg.source == crate::Lock::LockSource::Root {
            "root"
        } else {
            pkg.name.as_str()
        };
        pkg.effects = provenance_for(entries, key);
        pkg.effect_grants = manifest
            .grants
            .iter()
            .find(|(dep, _)| dep == &pkg.name)
            .map(|(_, effects)| effects.clone())
            .unwrap_or_default();
    }
}

/// E1220: a transitive dependency uses an effect outside this package's budget.
pub fn e1220(dep: &str, effect: &str) -> Diagnostic {
    Diagnostic::error(
        "E1220",
        format!(
            "`{dep}` uses the `{effect}` effect, which this package's budget doesn't allow"
        ),
        "an `effects:` budget fails the build when any dependency reaches an effect you didn't list — supply-chain review as a compile error".to_string(),
        format!(
            "add `{effect}` to `allow`, or grant it to `{dep}` in `grants:`, or drop the dependency"
        ),
        None::<Span>,
    )
}
