//! The canonical merge engine (U5 — unified-ecosystem §6).
//!
//! One referee reconciles the typed contributions every module makes, across
//! all three tiers (`env` / `system` / `image`). The §6 table:
//!
//! | Field | Rule |
//! |---|---|
//! | `sources` | merge by key; duplicate names with **different** refs conflict |
//! | `packages` | concatenate, de-duplicate, **preserve source identity** |
//! | namespace entries | merge by key; package lists combine; scalars per below |
//! | scalar settings | one value wins by the shared contribution law |
//!
//! This is the pure data-reconciliation core (std-only, I6). It operates on the
//! typed contribution model the module parser/evaluator will populate (Chunk 3+)
//! and is independent of how those contributions are read. Conflicts are typed
//! `MergeError`s here; they become I4 diagnostics when wired into evaluation.

use std::collections::BTreeMap;

/// A package value (`Pkg`, §5) with its source identity preserved so the
/// de-duplication in §6 keeps `default.ripgrep` distinct from `unstable.ripgrep`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pkg {
    pub source: String,
    pub name: String,
}

impl Pkg {
    pub fn new(source: impl Into<String>, name: impl Into<String>) -> Pkg {
        Pkg {
            source: source.into(),
            name: name.into(),
        }
    }
}

/// Parse a `packages:` list body (the text inside `[ … ]`) into `Pkg`s,
/// expanding the type-directed sugar (U6 / D-JPK19):
///
/// - `default.ripgrep`      → `Pkg{ default, ripgrep }`
/// - `unstable.neovim`      → `Pkg{ unstable, neovim }`
/// - `"mine@hello"`         → escape-hatch string; source left empty for the
///   resolver to interpret
/// - bare `ripgrep`         → `Pkg{ "", ripgrep }` (source filled in later from
///   the default source)
///
/// std-only and lenient: empty items are skipped. The split is bracket-aware,
/// so a nested list value never splits mid-item.
pub fn parse_package_list(body: &str) -> Vec<Pkg> {
    let mut out = Vec::new();
    for item in split_top_level(body) {
        out.extend(parse_package_item(item.trim()));
    }
    out
}

fn parse_package_item(item: &str) -> Vec<Pkg> {
    if item.is_empty() {
        return Vec::new();
    }
    // Escape-hatch quoted string: keep verbatim, source unresolved.
    if item.starts_with('"') {
        let inner = item.trim_matches('"');
        if inner.is_empty() {
            return Vec::new();
        }
        return vec![Pkg::new("", inner)];
    }
    // D-SPREAD1=A: `source.[a, b, c]` → one Pkg per member.
    if let Some((source, rest)) = item.split_once('.') {
        let rest = rest.trim();
        if rest.starts_with('[') && rest.ends_with(']') {
            let inner = &rest[1..rest.len() - 1];
            return inner
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(|name| Pkg::new(source, name))
                .collect();
        }
        if !rest.is_empty() {
            return vec![Pkg::new(source, rest)];
        }
    }
    // Bare name: source resolved from the default later.
    vec![Pkg::new("", item)]
}

/// Split on commas not nested inside `()`/`[]`/`{}` (so `["a", "b"]` is one
/// item). Returns the raw (untrimmed) slices.
fn split_top_level(body: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, c) in body.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&body[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < body.len() {
        out.push(&body[start..]);
    }
    out
}

/// The explicit priority a scalar setting may carry (§6). A bare value is
/// `Normal`; `default` is the declaration fallback; `force` is a fleet pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    Normal,
    Default,
    Force,
}

/// One contribution to a scalar setting: the value plus its priority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scalar {
    pub value: String,
    pub priority: Priority,
}

impl Scalar {
    pub fn normal(value: impl Into<String>) -> Scalar {
        Scalar {
            value: value.into(),
            priority: Priority::Normal,
        }
    }
    pub fn default(value: impl Into<String>) -> Scalar {
        Scalar {
            value: value.into(),
            priority: Priority::Default,
        }
    }
    pub fn force(value: impl Into<String>) -> Scalar {
        Scalar {
            value: value.into(),
            priority: Priority::Force,
        }
    }
}

/// One namespace entry's contributions (e.g. everything modules contribute to
/// `env.dev`). Package lists combine; scalar settings reconcile by priority.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntryContribution {
    pub packages: Vec<Pkg>,
    /// Scalar settings (`services`/`options`/plain fields), keyed by setting
    /// name. Each key may receive several contributions to reconcile.
    pub settings: BTreeMap<String, Vec<Scalar>>,
}

/// A fully merged namespace entry: deduped packages + resolved scalar values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergedEntry {
    pub packages: Vec<Pkg>,
    pub settings: BTreeMap<String, String>,
}

/// Why a merge could not be reconciled (§6 conflict diagnostics).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeError {
    /// Two `sources` declarations share a name but resolve to different refs.
    SourceConflict { name: String, a: String, b: String },
    /// A scalar setting got conflicting values at one contribution layer.
    ScalarConflict { key: String, values: Vec<String> },
}

/// Merge `sources` maps by key: identical refs de-duplicate; the same name with
/// different refs is a conflict (§6).
pub fn merge_sources(
    contribs: &[BTreeMap<String, String>],
) -> Result<BTreeMap<String, String>, MergeError> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for map in contribs {
        for (name, reference) in map {
            match out.get(name) {
                Some(existing) if existing != reference => {
                    return Err(MergeError::SourceConflict {
                        name: name.clone(),
                        a: existing.clone(),
                        b: reference.clone(),
                    });
                }
                _ => {
                    out.insert(name.clone(), reference.clone());
                }
            }
        }
    }
    Ok(out)
}

/// Concatenate package lists, de-duplicate while preserving source identity, and
/// keep first-seen order (§6).
pub fn merge_packages(lists: &[Vec<Pkg>]) -> Vec<Pkg> {
    let mut out: Vec<Pkg> = Vec::new();
    for list in lists {
        for pkg in list {
            if !out.contains(pkg) {
                out.push(pkg.clone());
            }
        }
    }
    out
}

/// Resolve one scalar setting's contributions through the canonical fact law.
/// Scalar inputs carry no precedence algorithm; precedence and conflicts belong
/// to `jet_foundation::Policy::resolve`.
pub fn resolve_scalar(key: &str, contribs: &[Scalar]) -> Result<Option<String>, MergeError> {
    let declarations = contribs
        .iter()
        .enumerate()
        .map(|(index, scalar)| {
            let (layer, force) = match scalar.priority {
                Priority::Default => (jet_foundation::Policy::ContributionLayer::Declaration, false),
                Priority::Normal => (jet_foundation::Policy::ContributionLayer::OptimizationBundle, false),
                Priority::Force => (jet_foundation::Policy::ContributionLayer::Fleet, true),
            };
            let declaration = jet_foundation::Policy::FactContribution::new(
                key,
                jet_foundation::Policy::FactValue::Text(scalar.value.clone()),
                jet_foundation::Policy::SourceScope::Package,
                layer,
                format!("scalar contribution {index}"),
            );
            if force { declaration.force() } else { declaration }
        })
        .collect::<Vec<_>>();
    match jet_foundation::Policy::resolve(
        jet_foundation::Policy::FactKey::new(key),
        declarations,
    ) {
        Ok(Some(fact)) => match fact.value {
            jet_foundation::Policy::FactValue::Text(value) => Ok(Some(value)),
            _ => Ok(None),
        },
        Ok(None) => Ok(None),
        Err(jet_foundation::Policy::FactError::Conflict { first, second, .. }) => {
            let show = |value: jet_foundation::Policy::FactValue| match value {
                jet_foundation::Policy::FactValue::Text(value) => value,
                value => value.display(),
            };
            Err(MergeError::ScalarConflict {
                key: key.to_string(),
                values: vec![show(first.value), show(second.value)],
            })
        }
        Err(error) => Err(MergeError::ScalarConflict {
            key: key.to_string(),
            values: vec![error.message()],
        }),
    }
}

/// Merge several contributions to one namespace entry: packages combine+dedup,
/// scalar settings reconcile per `resolve_scalar` (§6).
pub fn merge_entry(contribs: &[EntryContribution]) -> Result<MergedEntry, MergeError> {
    let lists: Vec<Vec<Pkg>> = contribs.iter().map(|c| c.packages.clone()).collect();
    let packages = merge_packages(&lists);

    // Gather every contribution per setting key.
    let mut by_key: BTreeMap<String, Vec<Scalar>> = BTreeMap::new();
    for c in contribs {
        for (k, scalars) in &c.settings {
            by_key.entry(k.clone()).or_default().extend(scalars.clone());
        }
    }
    let mut settings = BTreeMap::new();
    for (k, scalars) in by_key {
        if let Some(v) = resolve_scalar(&k, &scalars)? {
            settings.insert(k, v);
        }
    }
    Ok(MergedEntry { packages, settings })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    // ── Pkg sugar (U6 / D-JPK19) ──

    #[test]
    fn sugar_member_spread() {
        assert_eq!(
            parse_package_list("default.[cargo, ripgrep, fd]"),
            vec![
                Pkg::new("default", "cargo"),
                Pkg::new("default", "ripgrep"),
                Pkg::new("default", "fd"),
            ]
        );
    }

    #[test]
    fn sugar_dotted_single() {
        assert_eq!(
            parse_package_list("default.ripgrep"),
            vec![Pkg::new("default", "ripgrep")]
        );
    }

    #[test]
    fn sugar_mixed_sources_in_one_list() {
        // The example from unified-ecosystem §5.
        assert_eq!(
            parse_package_list("default.ripgrep, default.fd, unstable.neovim"),
            vec![
                Pkg::new("default", "ripgrep"),
                Pkg::new("default", "fd"),
                Pkg::new("unstable", "neovim"),
            ]
        );
    }

    #[test]
    fn sugar_escape_hatch_string() {
        assert_eq!(
            parse_package_list("\"mine@hello\""),
            vec![Pkg::new("", "mine@hello")]
        );
    }

    #[test]
    fn sugar_bare_name_has_empty_source() {
        assert_eq!(parse_package_list("ripgrep"), vec![Pkg::new("", "ripgrep")]);
    }

    #[test]
    fn sugar_empty_and_whitespace_skipped() {
        assert!(parse_package_list("   ").is_empty());
        assert_eq!(
            parse_package_list("default.fd, , default.rg"),
            vec![Pkg::new("default", "fd"), Pkg::new("default", "rg")]
        );
    }

    // ── sources ──

    #[test]
    fn sources_merge_by_key_and_dedup() {
        let a = map(&[("default", "NixOS/nixpkgs/nixos-24.05@github")]);
        let b = map(&[
            ("default", "NixOS/nixpkgs/nixos-24.05@github"), // identical → dedup
            ("unstable", "NixOS/nixpkgs/nixpkgs-unstable@github"),
        ]);
        let merged = merge_sources(&[a, b]).unwrap();
        assert_eq!(merged.len(), 2);
        assert_eq!(merged["unstable"], "NixOS/nixpkgs/nixpkgs-unstable@github");
    }

    #[test]
    fn sources_conflict_on_different_refs() {
        let a = map(&[("default", "NixOS/nixpkgs/nixos-24.05@github")]);
        let b = map(&[("default", "NixOS/nixpkgs/nixos-23.11@github")]);
        assert!(matches!(
            merge_sources(&[a, b]),
            Err(MergeError::SourceConflict { .. })
        ));
    }

    // ── packages ──

    #[test]
    fn packages_concat_dedup_preserve_source_and_order() {
        let l1 = vec![Pkg::new("default", "ripgrep"), Pkg::new("default", "fd")];
        let l2 = vec![
            Pkg::new("default", "ripgrep"),  // dup → dropped
            Pkg::new("unstable", "ripgrep"), // different source → kept
            Pkg::new("default", "jq"),
        ];
        let merged = merge_packages(&[l1, l2]);
        assert_eq!(
            merged,
            vec![
                Pkg::new("default", "ripgrep"),
                Pkg::new("default", "fd"),
                Pkg::new("unstable", "ripgrep"),
                Pkg::new("default", "jq"),
            ]
        );
    }

    // ── scalars ──

    #[test]
    fn scalar_single_value() {
        assert_eq!(
            resolve_scalar("k", &[Scalar::normal("on")]).unwrap(),
            Some("on".to_string())
        );
    }

    #[test]
    fn scalar_same_value_twice_is_fine() {
        let v = resolve_scalar("k", &[Scalar::normal("on"), Scalar::normal("on")]).unwrap();
        assert_eq!(v, Some("on".to_string()));
    }

    #[test]
    fn scalar_conflict_without_priority() {
        assert!(matches!(
            resolve_scalar("k", &[Scalar::normal("on"), Scalar::normal("off")]),
            Err(MergeError::ScalarConflict { .. })
        ));
    }

    #[test]
    fn scalar_normal_overrides_default() {
        let v = resolve_scalar("k", &[Scalar::default("off"), Scalar::normal("on")]).unwrap();
        assert_eq!(v, Some("on".to_string()));
    }

    #[test]
    fn scalar_force_overrides_normal() {
        let v = resolve_scalar(
            "k",
            &[
                Scalar::normal("on"),
                Scalar::force("win"),
            ],
        )
        .unwrap();
        assert_eq!(v, Some("win".to_string()));
    }

    #[test]
    fn scalar_default_only() {
        let v = resolve_scalar("k", &[Scalar::default("base")]).unwrap();
        assert_eq!(v, Some("base".to_string()));
    }

    #[test]
    fn scalar_two_forces_conflict() {
        assert!(matches!(
            resolve_scalar("k", &[Scalar::force("a"), Scalar::force("b")]),
            Err(MergeError::ScalarConflict { .. })
        ));
    }

    #[test]
    fn scalar_none_when_no_contributions() {
        assert_eq!(resolve_scalar("k", &[]).unwrap(), None);
    }

    // ── full entry ──

    #[test]
    fn entry_combines_packages_and_resolves_settings() {
        let c1 = EntryContribution {
            packages: vec![Pkg::new("default", "ripgrep")],
            settings: BTreeMap::from([("prompt".to_string(), vec![Scalar::default("jetpack")])]),
        };
        let c2 = EntryContribution {
            packages: vec![Pkg::new("default", "fd")],
            settings: BTreeMap::from([("prompt".to_string(), vec![Scalar::normal("wordstats")])]),
        };
        let merged = merge_entry(&[c1, c2]).unwrap();
        assert_eq!(
            merged.packages,
            vec![Pkg::new("default", "ripgrep"), Pkg::new("default", "fd")]
        );
        // normal overrides the default fallback
        assert_eq!(merged.settings["prompt"], "wordstats");
    }

    #[test]
    fn entry_propagates_scalar_conflict() {
        let c1 = EntryContribution {
            packages: vec![],
            settings: BTreeMap::from([("host".to_string(), vec![Scalar::normal("a")])]),
        };
        let c2 = EntryContribution {
            packages: vec![],
            settings: BTreeMap::from([("host".to_string(), vec![Scalar::normal("b")])]),
        };
        assert!(matches!(
            merge_entry(&[c1, c2]),
            Err(MergeError::ScalarConflict { key, .. }) if key == "host"
        ));
    }
}
