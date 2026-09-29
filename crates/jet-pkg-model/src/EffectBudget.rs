//! D-EFFBUDGET1 (ratified 2026-07-01): package effect budget.
//!
//! Zero-config: every `jet build` prints a one-line summary of the effects the
//! dependency graph uses, and per-dependency effect provenance is recorded in
//! the lockfile. An `authority: { holds: { allow: […], deny: […] } }` block
//! enforces the whole graph — package code and every dependency fail with the
//! exact package and offending function when they reach an effect outside the
//! budget. `authority.grants: { "dep": [Effect] }` is the audited per-dependency
//! escape, also recorded in the lockfile. Manifest keys only — no language
//! grammar (§0.4 DO-NOT).
//!
//! Attribution: sema already computes a whole-program per-function effect fixpoint
//! (`Sema::Effects::solve`, keyed by `Sema::effect_key`). This module first walks
//! the existing `EffectSummary` graph from the driver's selected entry, then
//! attributes only reached functions to the package (root, or a dependency)
//! whose module defines them, by matching the module's on-disk path against
//! `ProgramBundle::dep_roots` — the same name→source-root map `Loader` builds
//! for both `deps:` (path/git/provider) entries and hangar-realized
//! `use <pkg>` libraries (U17).

use crate::Diagnostics::{Diagnostic, Span};
use crate::Package::PackageFacts;
use crate::Sema::{effect_covers, effect_set_has_root, Effect, EffectSet, EffectSummary};
use crate::AST::{ImportKind, Item, ProgramBundle};
use jet_foundation::Authority::{
    answer, parse_right, root as effect_root, ApplicationAuthority, Holds, Verdict,
};
use jet_foundation::Report::{StatusEnvelope, StatusValue};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// The application-boundary view of one checked program.
///
/// Keep the four roles separate. `required_effects` is a sema fact;
/// `granted_effects` and `denied_effects` are policy facts; `authority` names
/// the source of that policy. Consumers must not infer one role from another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectProjection {
    pub required_effects: EffectSet,
    pub granted_effects: EffectSet,
    pub denied_effects: EffectSet,
    pub authority: String,
}

impl Default for EffectProjection {
    fn default() -> Self {
        let authority = ApplicationAuthority::ambient_basics();
        Self {
            required_effects: EffectSet::new(),
            granted_effects: authority.granted_effects,
            denied_effects: authority.denied_effects,
            authority: authority.authority,
        }
    }
}

impl EffectProjection {
    /// Carry this checked application decision into the execution bundle.
    /// Required effects remain the sema fact; the other fields remain the
    /// manifest or interactive policy fact.
    pub fn application_authority(&self) -> jet_foundation::Authority::ApplicationAuthority {
        jet_foundation::Authority::ApplicationAuthority {
            required_effects: self.required_effects.clone(),
            granted_effects: self.granted_effects.clone(),
            denied_effects: self.denied_effects.clone(),
            authority: self.authority.clone(),
        }
    }

    /// Required effects with no policy verdict at the application boundary.
    pub fn undecided(&self) -> EffectSet {
        self.application_authority().undecided_effects()
    }

    pub fn is_allowed(&self) -> bool {
        self.application_authority().is_allowed()
    }
}

fn policy_effects(names: impl IntoIterator<Item = String>) -> EffectSet {
    names
        .into_iter()
        .filter_map(|name| parse_right(&name))
        .collect()
}

/// Project the application policy without changing the sema effect fact.
///
/// The package manifest is optional for single-file programs. Such programs
/// receive D-AUTH-AMBIENT1's beginner basics; an explicit package manifest
/// replaces that default with its own holds.
pub fn project_application_effects(
    required_effects: &EffectSet,
    manifest: Option<&PackageFacts>,
) -> EffectProjection {
    let Some(manifest) = manifest else {
        return floor_projection(required_effects, ApplicationAuthority::ambient_basics());
    };
    // #3718: a manifest that writes no `authority.holds` keeps the beginner
    // floor; writing `holds` replaces it with exactly the written policy.
    if manifest.authority.holds.allow.is_none() && manifest.authority.holds.deny.is_none() {
        return floor_projection(required_effects, ApplicationAuthority::package_default());
    }

    let granted_effects = manifest
        .authority
        .holds
        .allow
        .clone()
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let denied_effects = manifest
        .authority
        .holds
        .deny
        .clone()
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    EffectProjection {
        required_effects: required_effects.clone(),
        granted_effects: policy_effects(granted_effects),
        denied_effects: policy_effects(denied_effects),
        authority: "package.jet authority.holds".to_string(),
    }
}

/// Compute the application-boundary projection from the exact selected entry
/// rather than the broader per-package provenance aggregate.
pub fn project_program_effects(
    bundle: &ProgramBundle,
    summaries: &HashMap<String, EffectSummary>,
    default_entry: &str,
    manifest: Option<&PackageFacts>,
) -> EffectProjection {
    project_application_effects(&program_effects(bundle, summaries, default_entry), manifest)
}

fn floor_projection(required_effects: &EffectSet, floor: ApplicationAuthority) -> EffectProjection {
    EffectProjection {
        required_effects: required_effects.clone(),
        granted_effects: floor.granted_effects,
        denied_effects: floor.denied_effects,
        authority: floor.authority,
    }
}

/// The entry file of a program run without any Package manifest. Its E1803
/// repair names the exact `--allow` invocation (D-SCRIPT-CONFIRM1=A) and
/// inserts the leading inline Package carrier (D-ECO-INLINEPACKAGE1) instead
/// of naming a `package.jet` that the program does not have.
#[derive(Debug, Clone, Copy)]
pub struct ScriptEntry<'a> {
    pub path: &'a str,
    pub source: &'a str,
    /// The `jet` command that ran the script (`run`, `build`).
    pub command: &'a str,
}

/// Structured authority failure for JSON, redirected, and non-interactive
/// invocations. The same code covers denial and missing policy; the plain
/// Why names what the program does and the structured detail keeps the role
/// rows explicit for machine consumers.
///
/// For a manifest-less `script` with undecided effects the Fix leads with
/// the exact `--allow` command, and the report carries the inline Package
/// block as a source edit. Rights widen only by a written word, so the edit
/// is `needs-review` (D-RIGHTS-DIAG1=B) and `jet fix` applies it only under
/// `--all` (D-REPORT-FIXGRADE1=D).
pub fn application_policy_diagnostic(
    projection: &EffectProjection,
    denied: &EffectSet,
    script: Option<ScriptEntry<'_>>,
) -> Diagnostic {
    let missing = projection.undecided();
    let authority = projection.application_authority();
    let mut diagnostic = authority.policy_refusal();
    match script {
        Some(entry)
            if denied.is_empty() && !missing.is_empty() && authority.is_application_default() =>
        {
            // The constructor sentence-cases the fix; it now follows "or".
            let mut rest = diagnostic.fix.chars();
            let rest = rest
                .next()
                .map(|first| first.to_lowercase().collect::<String>() + rest.as_str())
                .unwrap_or_default();
            diagnostic.fix = format!(
                "run `jet {} --allow={} {}` to allow it for this run, or {rest}",
                entry.command,
                missing.iter().map(String::as_str).collect::<Vec<_>>().join(","),
                entry.path,
            );
            let at = crate::Package::inline_package_insertion_offset(entry.source);
            diagnostic.with_source_derived_suggestion(
                Span::new(at, at),
                inline_authority_package(entry.path, &authority.inline_allow_row()),
            )
        }
        _ => diagnostic,
    }
}

/// The inline Package carrier that grants `allow` to the script at `path`.
/// The Package name is the file stem; the canonical field parser requires it.
fn inline_authority_package(path: &str, allow: &Holds) -> String {
    let name = std::path::Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| {
            !stem.is_empty()
                && stem
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        })
        .unwrap_or("script");
    let allow = allow.iter().map(String::as_str).collect::<Vec<_>>().join(", ");
    format!(
        "package {{\n    name: \"{name}\"\n    authority: {{ holds: {{ allow: [{allow}] }} }}\n}}\n\n"
    )
}

fn is_memory_right(name: &str) -> bool {
    name == "Mem.Rc" || crate::Sema::memory_allocation_bound(name).is_some()
}

/// One package's (root, or a dependency) aggregated effect set.
#[derive(Debug, Clone)]
pub struct PackageEffects {
    /// `"root"` for the building package itself, else the dependency name.
    pub name: String,
    pub effects: EffectSet,
    /// One deterministic function identity that reaches each effect.
    /// This is the package-budget provenance used by E1220.
    pub effect_sites: BTreeMap<String, String>,
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
    effect_sites: BTreeMap<String, String>,
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

/// Attribute only functions reachable from the selected program entry to the
/// package whose module defines them. Sema solves every loaded declaration so
/// that function-local checks remain complete; package budgets are narrower:
/// loaded helpers that the selected driver never calls must not become
/// application provenance. Functions with no entry in `solved` contribute
/// nothing even when a reachable summary exists.
pub fn compute_package_effects(
    bundle: &ProgramBundle,
    solved: &HashMap<String, EffectSet>,
    summaries: &HashMap<String, EffectSummary>,
) -> Vec<PackageEffects> {
    let reachable = entry_reachable_keys(bundle, summaries, "run");
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
                &reachable,
                solved,
                summaries,
                &mut out.effects,
                &mut out.effect_sites,
                &mut out.panic_sites,
            );
        }
    }

    by_pkg
        .into_iter()
        .map(|(name, aggregate)| PackageEffects {
            name,
            effects: aggregate.effects,
            effect_sites: aggregate.effect_sites,
            panic_sites: aggregate.panic_sites.into_iter().collect(),
            boundary_span: aggregate.boundary_span,
        })
        .collect()
}
fn selected_output_identity(bundle: &ProgramBundle) -> Option<(String, String)> {
    let entry_module = bundle.modules.get(bundle.entry)?;
    for item in &entry_module.items {
        let Item::Const(constant) = item else {
            continue;
        };
        let Some(output) = constant
            .resolved_output
            .as_ref()
            .filter(|output| output.selected)
        else {
            continue;
        };
        let alias = bundle.modules.get(output.module)?.alias.clone();
        return Some((alias, output.semantic_name.clone()));
    }
    None
}

/// Return the semantic roots the driver can select for this checked bundle.
/// Runnable output facts identify a module explicitly; ordinary native builds
/// use the canonical `run` wrapper (including `swap_entry_point`'s wrapper).
/// The build-entry fallback is limited to the compiler-host function so its
/// pre-runtime budget is not silently dropped.
fn entry_candidates(
    bundle: &ProgramBundle,
    summaries: &HashMap<String, EffectSummary>,
    default_entry: &str,
) -> Vec<String> {
    if let Some((alias, name)) = selected_output_identity(bundle) {
        return [format!("{alias}::{name}"), name]
            .into_iter()
            .find(|candidate| summaries.contains_key(candidate))
            .into_iter()
            .collect();
    }

    let Some(module) = bundle.modules.get(bundle.entry) else {
        return [default_entry.to_string()]
            .into_iter()
            .filter(|candidate| summaries.contains_key(candidate))
            .collect();
    };
    let mut candidates = vec![
        format!("{}::{default_entry}", module.alias),
        default_entry.to_string(),
    ];
    if default_entry == "run"
        && module.items.iter().any(|item| {
            matches!(
                item,
                Item::Func(function) if crate::Sema::is_build_entry(function)
            )
        })
    {
        candidates.push(format!("{}::build", module.alias));
        candidates.push("build".to_string());
    }
    // Prefer the qualified spelling, then a unique short spelling. The build
    // fallback is considered only when no ordinary runtime root exists.
    candidates
        .into_iter()
        .find(|candidate| summaries.contains_key(candidate))
        .into_iter()
        .collect()
}


fn entry_reachable_keys(
    bundle: &ProgramBundle,
    summaries: &HashMap<String, EffectSummary>,
    default_entry: &str,
) -> HashSet<String> {
    let mut pending = entry_candidates(bundle, summaries, default_entry);
    let mut reachable = HashSet::new();
    while let Some(key) = pending.pop() {
        let Some(summary) = summaries.get(&key) else {
            continue;
        };
        if !reachable.insert(key) {
            continue;
        }
        pending.extend(summary.edges.iter().cloned());
    }
    reachable
}

fn key_is_reachable(key: &str, reachable: &HashSet<String>) -> bool {
    reachable.contains(key)
        || key
            .rsplit_once("::")
            .is_some_and(|(_, local)| reachable.contains(local))
}


fn collect_effects_for_key(
    key: String,
    reachable: &HashSet<String>,
    solved: &HashMap<String, EffectSet>,
    summaries: &HashMap<String, EffectSummary>,
    out: &mut EffectSet,
    effect_sites: &mut BTreeMap<String, String>,
    panic_sites: &mut BTreeSet<String>,
) {
    if !key_is_reachable(&key, reachable) {
        return;
    }
    let Some(set) = solved.get(&key) else {
        return;
    };
    out.extend(set.iter().cloned());
    for effect in set {
        let site = effect_site(&key, effect, solved, summaries, &mut HashSet::new());
        effect_sites.entry(effect.clone()).or_insert(site);
    }
    if let Some(site) = panic_site(&key, summaries, &mut HashSet::new()) {
        panic_sites.insert(site);
    }
}

fn effect_site(
    key: &str,
    effect: &str,
    solved: &HashMap<String, EffectSet>,
    summaries: &HashMap<String, EffectSummary>,
    seen: &mut HashSet<String>,
) -> String {
    if !seen.insert(key.to_string()) {
        return key.to_string();
    }
    let Some(summary) = summaries.get(key) else {
        return key.to_string();
    };
    if summary.maximal
        || summary
            .direct
            .iter()
            .any(|direct| effect_covers(direct, effect))
    {
        return key.to_string();
    }
    for callee in &summary.edges {
        let reaches = solved.get(callee).is_some_and(|effects| {
            effects
                .iter()
                .any(|candidate| effect_covers(candidate, effect))
        });
        if reaches {
            return effect_site(callee, effect, solved, summaries, seen);
        }
    }
    key.to_string()
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
    reachable: &HashSet<String>,
    solved: &HashMap<String, EffectSet>,
    summaries: &HashMap<String, EffectSummary>,
    out: &mut EffectSet,
    effect_sites: &mut BTreeMap<String, String>,
    panic_sites: &mut BTreeSet<String>,
) {
    let mut collect = |key: String| {
        collect_effects_for_key(
            key,
            reachable,
            solved,
            summaries,
            out,
            effect_sites,
            panic_sites,
        );
    };
    match item {
        Item::Func(f) => {
            collect(format!(
                "{module_alias}::{}",
                crate::Sema::effect_key(None, &f.name)
            ));
        }
        Item::Impl(im) => {
            for m in &im.methods {
                collect(format!(
                    "{module_alias}::{}",
                    crate::Sema::effect_key(Some(&im.type_name), &m.name)
                ));
            }
        }
        Item::Struct(s) => {
            for m in &s.methods {
                collect(format!(
                    "{module_alias}::{}",
                    crate::Sema::effect_key(Some(&s.name), &m.name)
                ));
            }
            for block in &s.trait_impls {
                for m in &block.methods {
                    collect(format!(
                        "{module_alias}::{}",
                        crate::Sema::effect_key(Some(&s.name), &m.name)
                    ));
                }
            }
        }
        Item::Enum(e) => {
            for m in &e.methods {
                collect(format!(
                    "{module_alias}::{}",
                    crate::Sema::effect_key(Some(&e.name), &m.name)
                ));
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
    let Some(entry) = entry_candidates(bundle, summaries, default_entry)
        .into_iter()
        .next()
    else {
        return EffectSet::new();
    };
    entry_effects(summaries, &entry)
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

/// Human application-boundary summary with explicit policy roles.
pub fn summary_line_for_program_with_authority(
    bundle: &ProgramBundle,
    summaries: &HashMap<String, EffectSummary>,
    default_entry: &str,
    manifest: Option<&PackageFacts>,
) -> String {
    let projection =
        project_application_effects(&program_effects(bundle, summaries, default_entry), manifest);
    format!(
        "required effects: {}; granted effects: {}; denied effects: {}; authority: {}",
        render_effect_names(&projection.required_effects),
        render_effect_names(&projection.granted_effects),
        render_effect_names(&projection.denied_effects),
        projection.authority,
    )
}

pub fn render_effect_projection_line(projection: &EffectProjection) -> String {
    format!(
        "required effects: {}; granted effects: {}; denied effects: {}; authority: {}",
        render_effect_names(&projection.required_effects),
        render_effect_names(&projection.granted_effects),
        render_effect_names(&projection.denied_effects),
        projection.authority,
    )
}

/// JSON object used inside semantic-index, dossier, and Canvas documents.
/// The status wrapper remains owned by command renderers; consumers embed this
/// object so the four roles stay one shared projection instead of four ad-hoc
/// serializers.
pub fn render_effect_projection_object(projection: &EffectProjection) -> String {
    let required = projection
        .required_effects
        .iter()
        .map(|effect| jet_foundation::JSON::quote(effect))
        .collect::<Vec<_>>()
        .join(",");
    let granted = projection
        .granted_effects
        .iter()
        .map(|effect| jet_foundation::JSON::quote(effect))
        .collect::<Vec<_>>()
        .join(",");
    let denied = projection
        .denied_effects
        .iter()
        .map(|effect| jet_foundation::JSON::quote(effect))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"required_effects\":[{required}],\"granted_effects\":[{granted}],\"denied_effects\":[{denied}],\"authority\":{}}}",
        jet_foundation::JSON::quote(&projection.authority),
    )
}

/// Machine application-boundary summary. The legacy `effects` field remains
/// for existing consumers; the role-specific fields are the canonical view
/// for new CLI, inspect, and Canvas consumers.
pub fn summary_json_for_program_with_authority(
    bundle: &ProgramBundle,
    summaries: &HashMap<String, EffectSummary>,
    default_entry: &str,
    manifest: Option<&PackageFacts>,
) -> String {
    render_effect_projection_json(&project_application_effects(
        &program_effects(bundle, summaries, default_entry),
        manifest,
    ))
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

fn render_effect_names(effects: &EffectSet) -> String {
    if effects.is_empty() {
        "none".to_string()
    } else {
        effects
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    }
}
fn render_effect_json(effects: &EffectSet) -> String {
    let effects = StatusValue::array(
        effects
            .iter()
            .map(|effect| StatusValue::from(effect.as_str())),
    );
    StatusEnvelope::new("build.effects", true)
        .with_field("effects", effects)
        .json()
}

pub fn render_effect_projection_json(projection: &EffectProjection) -> String {
    let required = StatusValue::array(
        projection
            .required_effects
            .iter()
            .map(|effect| StatusValue::from(effect.as_str())),
    );
    let granted = StatusValue::array(
        projection
            .granted_effects
            .iter()
            .map(|effect| StatusValue::from(effect.as_str())),
    );
    let denied = StatusValue::array(
        projection
            .denied_effects
            .iter()
            .map(|effect| StatusValue::from(effect.as_str())),
    );
    StatusEnvelope::new("build.effects", true)
        .with_field("effects", required.clone())
        .with_field("required_effects", required)
        .with_field("granted_effects", granted)
        .with_field("denied_effects", denied)
        .with_field("authority", StatusValue::from(projection.authority.as_str()))
        .json()
}

/// Human build status reports effects reachable through statically known
/// calls from the selected entry. Open function values remain conservative in
/// policy enforcement, but they do not invent every ambient effect in status.
pub fn summary_line_for_entry(summaries: &HashMap<String, EffectSummary>, entry: &str) -> String {
    render_effect_line(&entry_effects(summaries, entry))
}

pub fn summary_json_for_entry(summaries: &HashMap<String, EffectSummary>, entry: &str) -> String {
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
/// `authority.holds` block, fail the build for any package (root or
/// dependency) whose effect set has something outside `allow` or inside
/// `deny`, unless `authority.grants` covers it for that dependency. Returns
/// E1220 per offending (package, effect) pair.
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
        let granted = grants.get(pkg.name.as_str());
        for effect in pkg
            .effects
            .iter()
            .filter(|effect| effect_root(effect) != "Mem")
        {
            let is_panic = effect_root(effect) == Effect::Panic.name();
            // D-PANICROOT1: Panic is deny-only. Omission from `allow` is not
            // a denial, and an audited positive grant cannot override one.
            if !is_panic
                && granted.is_some_and(|g| answer(g, &empty, effect) == Verdict::Allowed)
            {
                continue;
            }
            let outside_allow = !is_panic
                && allow
                    .as_ref()
                    .is_some_and(|a| answer(a, &empty, effect) != Verdict::Allowed);
            let inside_deny = answer(&empty, &deny, effect) == Verdict::Denied;
            if outside_allow || inside_deny {
                if is_panic {
                    let site = pkg
                        .panic_sites
                        .first()
                        .map(String::as_str)
                        .unwrap_or("a reachable dependency function");
                    diags.push(e1220_panic(&pkg.name, site, pkg.boundary_span));
                } else {
                    let site = pkg.effect_sites.get(effect).map(String::as_str);
                    diags.push(e1220_at(&pkg.name, effect, site));
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
        pkg.required_effects = provenance_for(entries, key);
        pkg.effects = pkg.required_effects.clone();
        pkg.granted_effects = manifest
            .authority
            .grants
            .iter()
            .find(|(dep, _)| dep == &pkg.name)
            .map(|(_, effects)| effects.clone())
            .unwrap_or_default();
        pkg.effect_grants = pkg.granted_effects.clone();
        pkg.denied_effects = manifest.authority.holds.deny.clone().unwrap_or_default();
        pkg.effect_authority = Some(
            if pkg.source == crate::Lock::LockSource::Root {
                "package.jet authority.holds"
            } else if pkg.granted_effects.is_empty() {
                "package.jet authority.holds"
            } else {
                "package.jet authority.grants"
            }
            .to_string(),
        );
    }
}

/// E1220: a package reaches an effect outside this package's budget.
pub fn e1220(dep: &str, effect: &str) -> Diagnostic {
    e1220_at(dep, effect, None)
}

fn e1220_at(dep: &str, effect: &str, site: Option<&str>) -> Diagnostic {
    let location = site.map_or_else(String::new, |site| format!(" function `{site}`"));
    let fix = if dep == "root" {
        format!("add `{effect}` to `authority.holds.allow` or remove the code that reaches it")
    } else {
        format!(
            "add `{effect}` to `authority.holds.allow`, grant it to `{dep}` in `authority.grants`, or drop the dependency"
        )
    };
    Diagnostic::error(
        "E1220",
        format!(
            "`{dep}`{location} uses the `{effect}` effect, which this package's budget doesn't allow"
        ),
        "an `authority.holds` budget fails the build when package code or any dependency reaches an effect you didn't list — supply-chain review as a compile error".to_string(),
        fix,
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
    fn test_module(path: &str, alias: &str, source: &str) -> crate::AST::LoadedModule {
        let (tokens, diagnostics) = crate::Lexer::lex(source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let mut program = crate::Parser::parse(&tokens).expect("test source parses");
        crate::AST::LoadedModule {
            path: std::path::PathBuf::from(path),
            display: path.to_string(),
            source: source.to_string(),
            alias: alias.to_string(),
            imports: std::mem::take(&mut program.imports),
            items: std::mem::take(&mut program.items),
            script_body: std::mem::take(&mut program.script_body),
            block_spans: std::mem::take(&mut program.block_spans),
            web_target_ceiling: program.web_target_ceiling,
            pub_file: program.pub_file,
            no_prelude: program.no_prelude,
            default_target: program.default_target,
            html_path: program.html_path,
            policy_declarations: program.policy_declarations,
            user_policy_declarations: program.user_policy_declarations,
            rule_facts: program.rule_facts,
        }
    }

    fn effect_fixture(root_calls_migrate: bool) -> (
        crate::AST::ProgramBundle,
        HashMap<String, EffectSet>,
        HashMap<String, EffectSummary>,
    ) {
        let bundle = crate::AST::ProgramBundle {
            entry: 0,
            project_root: std::path::PathBuf::from("/workspace/app"),
            modules: vec![
                test_module("/workspace/app/run.jet", "run", "fn run() {}\n"),
                test_module(
                    "/workspace/deps/email/src/crypto.jet",
                    "crypto",
                    "fn migrate_v1() {}\nfn seal() {}\nfn unused() {}\n",
                ),
            ],
            devtools_registry: crate::AST::DevtoolsRegistry::default(),
            parse_teaching: Vec::new(),
            used_core: HashSet::new(),
            ffi_callback_fns: HashSet::new(),
            cffi: crate::AST::CFfi::default(),
            comptime_inputs: Vec::new(),
            name_ledger: jet_foundation::Names::NameLedger::default(),
            layer_ceiling: None,
            inferred_layer: crate::Syntax::RuntimeLayer::Core,
            web_partitions: HashMap::new(),
            web_partition_enforced: false,
            web_partition_report: None,
            dep_roots: HashMap::from([(
                "email".to_string(),
                std::path::PathBuf::from("/workspace/deps/email"),
            )]),
            package_guarantees: Default::default(),
            program_allocator: Default::default(),
            active_os: crate::Syntax::OSTarget::host(),
            build_facts: Default::default(),
            edition: "2027".to_string(),
        };

        let mut summaries = HashMap::new();
        summaries.insert(
            "run::run".to_string(),
            EffectSummary {
                edges: root_calls_migrate
                    .then(|| BTreeSet::from(["crypto::migrate_v1".to_string()]))
                    .unwrap_or_default(),
                ..EffectSummary::default()
            },
        );
        summaries.insert(
            "crypto::migrate_v1".to_string(),
            EffectSummary {
                edges: BTreeSet::from(["crypto::seal".to_string()]),
                ..EffectSummary::default()
            },
        );
        summaries.insert(
            "crypto::seal".to_string(),
            EffectSummary {
                direct: EffectSet::from(["Rand".to_string()]),
                ..EffectSummary::default()
            },
        );
        summaries.insert(
            "crypto::unused".to_string(),
            EffectSummary {
                direct: EffectSet::from(["Rand".to_string()]),
                ..EffectSummary::default()
            },
        );

        let solved = HashMap::from([
            ("run::run".to_string(), EffectSet::new()),
            (
                "crypto::migrate_v1".to_string(),
                EffectSet::from(["Rand".to_string()]),
            ),
            (
                "crypto::seal".to_string(),
                EffectSet::from(["Rand".to_string()]),
            ),
            (
                "crypto::unused".to_string(),
                EffectSet::from(["Rand".to_string()]),
            ),
        ]);
        (bundle, solved, summaries)
    }

    fn package<'a>(entries: &'a [PackageEffects], name: &str) -> &'a PackageEffects {
        entries
            .iter()
            .find(|entry| entry.name == name)
            .expect("fixture package entry")
    }


    #[test]
    fn package_budget_ignores_unreachable_loaded_rand_helper() {
        let (bundle, solved, summaries) = effect_fixture(false);
        let entries = compute_package_effects(&bundle, &solved, &summaries);

        let email = package(&entries, "email");
        assert!(email.effects.is_empty());
        assert!(email.effect_sites.is_empty());

        let mut manifest = PackageFacts::default();
        manifest.effects_enabled = true;
        manifest.authority.holds.allow = Some(vec!["IO".to_string()]);
        assert!(enforce(&entries, &manifest).is_empty());
    }

    #[test]
    fn package_budget_keeps_reachable_rand_at_dependency_boundary() {
        let (bundle, solved, summaries) = effect_fixture(true);
        let entries = compute_package_effects(&bundle, &solved, &summaries);

        let email = package(&entries, "email");
        assert!(email.effects.contains("Rand"));
        assert_eq!(
            email.effect_sites.get("Rand").map(String::as_str),
            Some("crypto::seal")
        );

        let mut manifest = PackageFacts::default();
        manifest.effects_enabled = true;
        manifest.authority.holds.allow = Some(vec!["IO".to_string()]);
        let diagnostics = enforce(&entries, &manifest);
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.what.contains("`email`") && diagnostic.what.contains("Rand")
        }));
    }

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
            effect_sites: BTreeMap::new(),
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
    fn authority_block_covers_root_and_names_effect_site() {
        let mut manifest = PackageFacts::default();
        manifest.effects_enabled = true;
        manifest.authority.holds.allow = Some(vec!["FS".to_string()]);
        let entries = [PackageEffects {
            name: "root".to_string(),
            effects: EffectSet::from(["Net".to_string()]),
            effect_sites: BTreeMap::from([("Net".to_string(), "run::fetch".to_string())]),
            panic_sites: Vec::new(),
            boundary_span: None,
        }];

        let diagnostics = enforce(&entries, &manifest);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "E1220");
        assert!(diagnostics[0].what.contains("fetch"));
        assert!(diagnostics[0].what.contains("Net"));

        manifest.authority.holds.allow = Some(vec!["Net".to_string()]);
        assert!(enforce(&entries, &manifest).is_empty());
    }

    #[test]
    fn panic_budget_ignores_allow_omission_but_honors_denial() {
        let mut manifest = PackageFacts::default();
        manifest.effects_enabled = true;
        manifest.authority.holds.allow = Some(vec![
            "IO".to_string(),
            "GPU".to_string(),
            "Mem.Alloc".to_string(),
            "FS".to_string(),
            "Rand".to_string(),
        ]);
        let entries = [PackageEffects {
            name: "panicdep".to_string(),
            effects: EffectSet::from(["Panic".to_string()]),
            effect_sites: BTreeMap::new(),
            panic_sites: vec!["panicdep::parse_port".to_string()],
            boundary_span: Some(Span::new(4, 12)),
        }];

        assert!(enforce(&entries, &manifest).is_empty());

        manifest.authority.holds.deny = Some(vec!["Panic".to_string()]);
        let diagnostics = enforce(&entries, &manifest);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "E1220");
        assert!(diagnostics[0].what.contains("panicdep::parse_port"));
        assert_eq!(diagnostics[0].span, Some(Span::new(4, 12)));

        manifest.authority.grants = vec![("panicdep".to_string(), vec!["Panic".to_string()])];
        let diagnostics = enforce(&entries, &manifest);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "E1220");
        assert!(diagnostics[0].what.contains("panicdep::parse_port"));
        assert_eq!(diagnostics[0].span, Some(Span::new(4, 12)));
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
        assert!(summary_json_for_entry(&summaries, "run").contains("\"action\":\"build.effects\""));
    }

    #[test]
    fn application_projection_keeps_required_granted_denied_and_authority_distinct() {
        let mut manifest = PackageFacts::default();
        manifest.authority.holds.allow = Some(vec!["FS".to_string()]);
        manifest.authority.holds.deny = Some(vec!["Panic".to_string()]);
        manifest.authority.grants = vec![("netdep".to_string(), vec!["Net".to_string()])];
        let required = EffectSet::from([
            "FS.Read".to_string(),
            "Net".to_string(),
            "Panic".to_string(),
        ]);

        let projection = project_application_effects(&required, Some(&manifest));
        assert_eq!(projection.required_effects, required);
        assert!(projection.granted_effects.contains("FS"));
        assert!(!projection.granted_effects.contains("Net"));
        assert!(projection.denied_effects.contains("Panic"));
        assert_eq!(projection.authority, "package.jet authority.holds");
        assert!(projection.undecided().contains("Net"));

        let json = render_effect_projection_json(&projection);
        for field in [
            "required_effects",
            "granted_effects",
            "denied_effects",
            "authority",
        ] {
            assert!(
                json.contains(&format!("\"{field}\"")),
                "missing {field}: {json}"
            );
        }
    }

    #[test]
    fn manifestless_projection_grants_beginner_basics() {
        let required = EffectSet::from([
            "IO".to_string(),
            "Mem.Alloc".to_string(),
            "Exec".to_string(),
        ]);
        let projection = project_application_effects(&required, None);

        assert_eq!(projection.required_effects, required);
        assert_eq!(
            projection.granted_effects,
            EffectSet::from([
                "IO".to_string(),
                "Mem.Alloc".to_string(),
                "Exec".to_string(),
            ])
        );
        assert!(projection.undecided().is_empty());
        assert_eq!(projection.authority, "application default");
    }

    #[test]
    fn manifestless_projection_does_not_request_a_panic_grant() {
        let required = EffectSet::from(["IO".to_string(), "Panic".to_string()]);
        let projection = project_application_effects(&required, None);

        assert!(projection.undecided().is_empty());
        assert!(projection.is_allowed());
    }

    #[test]
    fn ambient_basics_leave_file_and_network_effects_undecided() {
        for effect in ["FS", "Net"] {
            let required = EffectSet::from([effect.to_string()]);
            let projection = project_application_effects(&required, None);

            assert_eq!(projection.undecided(), required);
            let diagnostic = application_policy_diagnostic(&projection, &EffectSet::new(), None);
            assert_eq!(diagnostic.code, "E1803");
            assert!(diagnostic.fix.contains(&format!("allow: [{effect}]")));
        }
    }

    #[test]
    fn explicit_manifest_denial_overrides_ambient_basics() {
        let mut manifest = PackageFacts::default();
        manifest.authority.holds.allow = Some(vec!["IO".to_string(), "Mem.Alloc".to_string()]);
        manifest.authority.holds.deny = Some(vec!["Exec".to_string()]);
        let required = EffectSet::from([
            "IO".to_string(),
            "Mem.Alloc".to_string(),
            "Exec".to_string(),
        ]);

        let projection = project_application_effects(&required, Some(&manifest));
        assert!(projection.granted_effects.contains("IO"));
        assert!(!projection.granted_effects.contains("Exec"));
        assert!(projection.denied_effects.contains("Exec"));
        assert!(!projection.is_allowed());
        let diagnostic =
            application_policy_diagnostic(&projection, &EffectSet::from(["Exec".to_string()]), None);
        assert!(diagnostic.what.contains("denies `Exec`"));
        assert!(diagnostic
            .fix
            .to_ascii_lowercase()
            .contains("adjust the denial"));
    }

    #[test]
    fn application_policy_diagnostic_fix_lists_the_complete_missing_set() {
        let projection = EffectProjection {
            required_effects: EffectSet::from([
                "IO".to_string(),
                "Mem.Alloc".to_string(),
                "Exec".to_string(),
            ]),
            granted_effects: EffectSet::new(),
            denied_effects: EffectSet::new(),
            authority: "package.jet authority.holds".to_string(),
        };

        let diagnostic = application_policy_diagnostic(&projection, &EffectSet::new(), None);
        assert!(diagnostic.fix.contains("allow: [Exec, IO, Mem.Alloc]"));
    }

    #[test]
    fn application_policy_diagnostic_keeps_policy_denial_separate() {
        let projection = EffectProjection {
            required_effects: EffectSet::from(["FS".to_string(), "Net".to_string()]),
            granted_effects: EffectSet::from(["FS".to_string()]),
            denied_effects: EffectSet::from(["Net".to_string(), "Panic".to_string()]),
            authority: "package.jet authority.holds".to_string(),
        };

        let diagnostic =
            application_policy_diagnostic(&projection, &EffectSet::from(["Net".to_string()]), None);
        assert_eq!(
            diagnostic.why,
            "This program uses the network, and package.jet denies that."
        );
        let detail = diagnostic.detail.as_deref().expect("E1803 facts detail");
        assert!(detail.contains("denied_effects=Net, Panic"), "{detail}");
        assert!(detail.contains("denied_required_effects=Net"), "{detail}");
        assert!(detail.contains("authority=package.jet authority.holds"), "{detail}");
    }

    /// #3718: a package.jet with no `authority.holds` keeps the beginner
    /// floor; spawning still needs a written grant.
    #[test]
    fn manifest_without_holds_keeps_the_beginner_floor() {
        let manifest = PackageFacts::default();
        let print_and_argv = EffectSet::from([
            "IO".to_string(),
            "Mem.Alloc".to_string(),
            jet_foundation::Syntax::EFFECT_LEAF_EXEC_ARGS.to_string(),
        ]);
        assert!(project_application_effects(&print_and_argv, Some(&manifest)).is_allowed());
        for effect in ["Exec", "Net", "FS.Write"] {
            let required = EffectSet::from(["IO".to_string(), effect.to_string()]);
            let projection = project_application_effects(&required, Some(&manifest));
            assert_eq!(projection.undecided(), EffectSet::from([effect.to_string()]));
            let diagnostic = application_policy_diagnostic(&projection, &EffectSet::new(), None);
            assert!(diagnostic.fix.contains("package.jet"), "{}", diagnostic.fix);
        }
        let mut explicit = PackageFacts::default();
        explicit.authority.holds.allow = Some(vec!["Net".to_string()]);
        assert!(!project_application_effects(&print_and_argv, Some(&explicit)).is_allowed());
    }

    #[test]
    fn manifestless_script_edit_inserts_a_sufficient_inline_package() {
        let source = "// tool header\nuse core.files as files\n\nfn run() {}\n";
        let required = EffectSet::from(["FS".to_string(), "IO".to_string(), "Panic".to_string()]);
        let projection = project_application_effects(&required, None);
        let diagnostic = application_policy_diagnostic(
            &projection,
            &EffectSet::new(),
            Some(ScriptEntry {
                path: "tools/gradient.jet",
                source,
                command: "run",
            }),
        );
        assert!(
            diagnostic.fix.starts_with("run `jet run --allow=FS tools/gradient.jet`"),
            "{}",
            diagnostic.fix
        );
        let edit = diagnostic.edit.clone().expect("manifest-less E1803 carries an edit");
        assert_eq!(
            diagnostic.safety,
            Some(crate::Diagnostics::FixSafety::NeedsReview)
        );
        let mut fixed = source.to_string();
        fixed.replace_range(edit.span.start..edit.span.end, &edit.new_text);
        assert!(fixed.starts_with("// tool header\npackage {\n    name: \"gradient\"\n"), "{fixed}");
        let (manifest, _) = PackageFacts::parse_inline(&fixed, "gradient.jet")
            .expect("inserted carrier parses")
            .expect("inserted carrier is leading");
        assert!(project_application_effects(&required, Some(&manifest)).is_allowed());

        let denied = application_policy_diagnostic(
            &projection,
            &EffectSet::from(["FS".to_string()]),
            Some(ScriptEntry {
                path: "gradient.jet",
                source,
                command: "run",
            }),
        );
        assert!(denied.edit.is_none(), "a denial is never repaired by a grant");
    }
}
