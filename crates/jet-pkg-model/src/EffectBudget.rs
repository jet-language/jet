//! D-EFFBUDGET1 (ratified 2026-07-01): package effect budget.
//!
//! Zero-config: every `jet build` prints a one-line summary of the effects the
//! dependency graph uses, and per-dependency effect provenance is recorded in
//! the lockfile. An `authority: .{ holds: { allow: […], deny: […] } }` block in
//! `package.jet` turns on whole-graph enforcement — the build fails naming the
//! exact dependency and offending function when a transitive dependency needs
//! an effect outside the budget. `authority.grants: { "dep": [Effect] }` is the
//! audited per-dependency escape, also recorded in the lockfile. Manifest keys
//! only — no language grammar (§0.4 DO-NOT).
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
use crate::Sema::{effect_set_has_root, Effect, EffectSet, EffectSummary};
use crate::AST::{ImportKind, Item, ProgramBundle};
use jet_foundation::Authority::{answer, parse_right, root as effect_root, Holds, Verdict};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

fn is_memory_right(name: &str) -> bool {
    name == "Mem.Rc" || crate::Sema::memory_allocation_bound(name).is_some()
}

/// One package's (root, or a dependency) aggregated effect set.
#[derive(Debug, Clone)]
pub struct PackageEffects {
    /// `"root"` for the building package itself, else the dependency name.
    pub name: String,
    pub effects: EffectSet,
    /// Function identities where a deniable Panic stop enters the graph.
    /// This is the package-budget provenance used by E1220.
    pub panic_sites: Vec<String>,
    /// Root-side source location where this dependency crosses the budget
    /// boundary. Package diagnostics render against the selected root source.
    pub boundary_span: Option<Span>,
}

#[derive(Default)]
struct PackageEffectAggregate {
    effects: EffectSet,
    panic_sites: BTreeSet<String>,
    boundary_span: Option<Span>,
}

fn dependency_boundary_span(bundle: &ProgramBundle, dependency: &str) -> Option<Span> {
    bundle
        .modules
        .get(bundle.entry)?
        .imports
        .iter()
        .find_map(|import| match &import.kind {
            ImportKind::Module(name, span) if name == dependency => Some(*span),
            ImportKind::Unqualified {
                module_alias,
                module_alias_span,
                ..
            } if module_alias == dependency => Some(*module_alias_span),
            _ => None,
        })
}

/// Attribute every function's solved effect set (`Sema::check_bundle_with_effect_facts`
/// output) to the package whose module defines it. Functions with no entry in
/// `solved` (never reached / no effects) contribute nothing.
pub fn compute_package_effects(
    bundle: &ProgramBundle,
    solved: &HashMap<String, EffectSet>,
    summaries: &HashMap<String, EffectSummary>,
) -> Vec<PackageEffects> {
    let mut by_pkg: BTreeMap<String, PackageEffectAggregate> = BTreeMap::new();

    for module in &bundle.modules {
        let owner = bundle
            .dep_roots
            .iter()
            .find(|(_, dir)| module.path.starts_with(dir))
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| "root".to_string());
        let boundary_span = if owner == "root" {
            None
        } else {
            dependency_boundary_span(bundle, &owner)
        };
        let out = by_pkg.entry(owner).or_default();
        if out.boundary_span.is_none() {
            out.boundary_span = boundary_span;
        }
        for item in &module.items {
            collect_item_effects(
                item,
                &module.alias,
                solved,
                summaries,
                &mut out.effects,
                &mut out.panic_sites,
            );
        }
    }

    by_pkg
        .into_iter()
        .map(|(name, aggregate)| PackageEffects {
            name,
            effects: aggregate.effects,
            panic_sites: aggregate.panic_sites.into_iter().collect(),
            boundary_span: aggregate.boundary_span,
        })
        .collect()
}

fn collect_effects_for_key(
    key: String,
    solved: &HashMap<String, EffectSet>,
    summaries: &HashMap<String, EffectSummary>,
    out: &mut EffectSet,
    panic_sites: &mut BTreeSet<String>,
) {
    let Some(set) = solved.get(&key) else {
        return;
    };
    out.extend(set.iter().cloned());
    if let Some(site) = panic_site(&key, summaries, &mut HashSet::new()) {
        panic_sites.insert(site);
    }
}

fn panic_site(
    key: &str,
    summaries: &HashMap<String, EffectSummary>,
    seen: &mut HashSet<String>,
) -> Option<String> {
    if !seen.insert(key.to_string()) {
        return None;
    }
    let Some(summary) = summaries.get(key) else {
        return None;
    };
    if summary.maximal
        || effect_set_has_root(&summary.direct, Effect::Panic)
        || summary.edges.contains("__jet_panic__")
    {
        return Some(key.to_string());
    }
    summary
        .edges
        .iter()
        .find_map(|callee| panic_site(callee, summaries, seen))
}

fn collect_item_effects(
    item: &Item,
    module_alias: &str,
    solved: &HashMap<String, EffectSet>,
    summaries: &HashMap<String, EffectSummary>,
    out: &mut EffectSet,
    panic_sites: &mut BTreeSet<String>,
) {
    match item {
        Item::Func(f) => {
            collect_effects_for_key(
                format!("{module_alias}::{}", crate::Sema::effect_key(None, &f.name)),
                solved,
                summaries,
                out,
                panic_sites,
            );
        }
        Item::Impl(im) => {
            for m in &im.methods {
                collect_effects_for_key(
                    format!(
                        "{module_alias}::{}",
                        crate::Sema::effect_key(Some(&im.type_name), &m.name)
                    ),
                    solved,
                    summaries,
                    out,
                    panic_sites,
                );
            }
        }
        Item::Struct(s) => {
            for m in &s.methods {
                collect_effects_for_key(
                    format!(
                        "{module_alias}::{}",
                        crate::Sema::effect_key(Some(&s.name), &m.name)
                    ),
                    solved,
                    summaries,
                    out,
                    panic_sites,
                );
            }
            for block in &s.trait_impls {
                for m in &block.methods {
                    collect_effects_for_key(
                        format!(
                            "{module_alias}::{}",
                            crate::Sema::effect_key(Some(&s.name), &m.name)
                        ),
                        solved,
                        summaries,
                        out,
                        panic_sites,
                    );
                }
            }
        }
        Item::Enum(e) => {
            for m in &e.methods {
                collect_effects_for_key(
                    format!(
                        "{module_alias}::{}",
                        crate::Sema::effect_key(Some(&e.name), &m.name)
                    ),
                    solved,
                    summaries,
                    out,
                    panic_sites,
                );
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
fn entry_effects(summaries: &HashMap<String, EffectSummary>, entry: &str) -> EffectSet {
    let mut effects = EffectSet::new();
    let mut seen = HashSet::new();
    let mut pending = vec![entry.to_string()];
    while let Some(key) = pending.pop() {
        if !seen.insert(key.clone()) {
            continue;
        }
        let Some(summary) = summaries.get(&key) else {
            continue;
        };
        effects.extend(summary.direct.iter().cloned());
        pending.extend(summary.edges.iter().cloned());
    }
    effects
}
fn program_effects(
    bundle: &ProgramBundle,
    summaries: &HashMap<String, EffectSummary>,
    default_entry: &str,
) -> EffectSet {
    bundle
        .modules
        .iter()
        .flat_map(|module| &module.items)
        .find_map(|item| match item {
            Item::Const(constant) => constant
                .resolved_output
                .as_ref()
                .filter(|output| output.selected)
                .map(|output| output.effects.iter().cloned().collect()),
            _ => None,
        })
        .unwrap_or_else(|| entry_effects(summaries, default_entry))
}

pub fn summary_line_for_program(
    bundle: &ProgramBundle,
    summaries: &HashMap<String, EffectSummary>,
    default_entry: &str,
) -> String {
    render_effect_line(&program_effects(bundle, summaries, default_entry))
}

pub fn summary_json_for_program(
    bundle: &ProgramBundle,
    summaries: &HashMap<String, EffectSummary>,
    default_entry: &str,
) -> String {
    render_effect_json(&program_effects(bundle, summaries, default_entry))
}

fn render_effect_line(effects: &EffectSet) -> String {
    if effects.is_empty() {
        "effects: none".to_string()
    } else {
        format!(
            "effects: {}",
            effects
                .iter()
                .map(|effect| effect.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn render_effect_json(effects: &EffectSet) -> String {
    let effects = effects
        .iter()
        .map(|effect| format!("\"{}\"", effect.as_str()))
        .collect::<Vec<_>>()
        .join(",");
    jet_foundation::Report::render_status_json(
        "ok",
        true,
        "build.effects",
        &format!(",\"effects\":[{effects}]"),
    )
}


/// Human build status reports effects reachable through statically known
/// calls from the selected entry. Open function values remain conservative in
/// policy enforcement, but they do not invent every ambient effect in status.
pub fn summary_line_for_entry(summaries: &HashMap<String, EffectSummary>, entry: &str) -> String {
    render_effect_line(&entry_effects(summaries, entry))
}

pub fn summary_json_for_entry(
    summaries: &HashMap<String, EffectSummary>,
    entry: &str,
) -> String {
    render_effect_json(&entry_effects(summaries, entry))
}

/// Per-package effect names, sorted, for lockfile provenance (`LockedPackage.effects`).
pub fn provenance_for(entries: &[PackageEffects], name: &str) -> Vec<String> {
    entries
        .iter()
        .find(|e| e.name == name)
        .map(|e| e.effects.iter().cloned().collect())
        .unwrap_or_default()
}

/// D-EFFBUDGET1 whole-graph enforcement: when `package.jet` declares an
/// `authority.holds` block, fail the build for any *dependency* (not root — the
/// budget names the supply chain) whose effect set has something outside
/// `allow` or inside `deny`, unless `authority.grants` covers it for that
/// dependency. Returns E1220 per offending (dependency, effect) pair.
pub fn enforce(entries: &[PackageEffects], manifest: &PackageFacts) -> Vec<Diagnostic> {
    if !manifest.effects_enabled {
        return Vec::new();
    }
    // D-EFFTREE1: allow/deny/grants entries may be ancestor roots (or leaves);
    // the substrate verdict, not exact membership, decides budget authority.
    let allow: Option<EffectSet> = manifest.authority.holds.allow.as_ref().map(|names| {
        names
            .iter()
            .filter(|name| !is_memory_right(name))
            .filter_map(|n| parse_right(n))
            .collect()
    });
    let deny: EffectSet = manifest
        .authority
        .holds
        .deny
        .as_ref()
        .map(|names| {
            names
                .iter()
                .filter(|name| !is_memory_right(name))
                .filter_map(|n| parse_right(n))
                .collect()
        })
        .unwrap_or_default();
    let grants: HashMap<&str, EffectSet> = manifest
        .authority
        .grants
        .iter()
        .map(|(dep, effects)| {
            (
                dep.as_str(),
                effects
                    .iter()
                    .filter(|name| !is_memory_right(name))
                    .filter_map(|n| parse_right(n))
                    .collect::<EffectSet>(),
            )
        })
        .collect();

    let empty = Holds::new();
    let mut diags = Vec::new();
    for pkg in entries {
        if pkg.name == "root" {
            continue;
        }
        let granted = grants.get(pkg.name.as_str());
        for effect in pkg
            .effects
            .iter()
            .filter(|effect| effect_root(effect) != "Mem")
        {
            if granted.is_some_and(|g| answer(g, &empty, effect) == Verdict::Allowed) {
                continue;
            }
            let outside_allow = allow
                .as_ref()
                .is_some_and(|a| answer(a, &empty, effect) != Verdict::Allowed);
            let inside_deny = answer(&empty, &deny, effect) == Verdict::Denied;
            if outside_allow || inside_deny {
                if effect_root(effect) == Effect::Panic.name() {
                    let site = pkg
                        .panic_sites
                        .first()
                        .map(String::as_str)
                        .unwrap_or("a reachable dependency function");
                    diags.push(e1220_panic(&pkg.name, site, pkg.boundary_span));
                } else {
                    diags.push(e1220(&pkg.name, effect));
                }
            }
        }
    }
    diags
}

/// D-EFFBUDGET1: write computed per-package effect provenance (and configured
/// `authority.grants`) into an existing lockfile's package entries in place.
/// No-op for a package name the lockfile doesn't (yet) list — `jet store fetch`
/// owns adding package entries; this only annotates ones already there.
pub fn update_lock_provenance(
    lock: &mut crate::Lock::LockFile,
    entries: &[PackageEffects],
    manifest: &PackageFacts,
) {
    lock.authority = (manifest.authority != crate::Package::PackageAuthority::default())
        .then(|| manifest.authority.clone());
    for pkg in &mut lock.packages {
        let key = if pkg.source == crate::Lock::LockSource::Root {
            "root"
        } else {
            pkg.name.as_str()
        };
        pkg.effects = provenance_for(entries, key);
        pkg.effect_grants = manifest
            .authority
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
        "an `authority.holds` budget fails the build when any dependency reaches an effect you didn't list — supply-chain review as a compile error".to_string(),
        format!(
            "add `{effect}` to `authority.holds.allow`, grant it to `{dep}` in `authority.grants`, or drop the dependency"
        ),
        None::<Span>,
    )
}

/// E1220 / D-NOPANIC1=D: package denial keeps the panic provenance visible
/// and gives the same three exits as function-scope prohibition.
pub fn e1220_panic(dep: &str, panic_site: &str, span: Option<Span>) -> Diagnostic {
    Diagnostic::error(
        "E1220",
        format!(
            "`{dep}` uses the `Panic` effect at `{panic_site}`, which this package's budget doesn't allow"
        ),
        format!(
            "the package denies stops from `{panic_site}`; a dependency that can stop cannot cross this budget boundary"
        ),
        "return a fallible result for expected failure, or add facts or a `#Pre`/refinement proof for a programmer-error stop; `Panic` is deny-only and cannot be allowed or granted".to_string(),
        span,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority_block_is_the_e1220_source() {
        let mut manifest = PackageFacts::default();
        manifest.effects_enabled = true;
        // Deliberately disagree with the retired mirror fields. The authority
        // block must decide the result.
        manifest.effects_allow = Some(vec!["Net".to_string()]);
        manifest.authority.holds.allow = Some(vec!["FS".to_string()]);
        let entries = [PackageEffects {
            name: "dep".to_string(),
            effects: EffectSet::from(["Net".to_string()]),
            panic_sites: Vec::new(),
            boundary_span: None,
        }];

        let diagnostics = enforce(&entries, &manifest);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].fix.contains("authority.holds.allow"));

        manifest.authority.grants = vec![("dep".to_string(), vec!["Net".to_string()])];
        assert!(enforce(&entries, &manifest).is_empty());
    }

    #[test]
    fn entry_summary_ignores_open_callback_possibilities() {
        let mut summaries = HashMap::new();
        summaries.insert(
            "run".to_string(),
            EffectSummary {
                direct: EffectSet::from(["IO".to_string()]),
                edges: BTreeSet::from(["callbacks::apply_twice".to_string()]),
                maximal: true,
                ..EffectSummary::default()
            },
        );
        summaries.insert(
            "callbacks::apply_twice".to_string(),
            EffectSummary {
                maximal: true,
                ..EffectSummary::default()
            },
        );
        assert_eq!(summary_line_for_entry(&summaries, "run"), "effects: IO");
        assert!(
            summary_json_for_entry(&summaries, "run")
                .contains("\"action\":\"build.effects\"")
        );
    }
}
