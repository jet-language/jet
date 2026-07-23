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
//! | scalar settings | one value wins only by explicit priority (`default`/`force`) |
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
/// - `default.[ripgrep, fd]`→ `Pkg{ default, ripgrep }`, `Pkg{ default, fd }`
/// - `unstable.neovim`      → `Pkg{ unstable, neovim }`
/// - `"mine@hello"`         → escape-hatch string; source left empty for the
///   resolver to interpret
/// - bare `ripgrep`         → `Pkg{ "", ripgrep }` (source filled in later from
///   the default source)
///
/// std-only and lenient: empty items are skipped. The scoped form's inner commas
/// are respected (the split is bracket-aware).
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
    // Scoped list: `source.[a, b, c]`.
    if let Some(dot_bracket) = item.find(".[") {
        let source = &item[..dot_bracket];
        let rest = &item[dot_bracket + 2..];
        let inside = rest.strip_suffix(']').unwrap_or(rest);
        return inside
            .split(',')
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(|n| Pkg::new(source, n))
            .collect();
    }
    // Dotted single: `source.name`.
    if let Some((source, name)) = item.split_once('.') {
        if !name.is_empty() {
            return vec![Pkg::new(source, name)];
        }
    }
    // Bare name: source resolved from the default later.
    vec![Pkg::new("", item)]
}

/// Split on commas not nested inside `()`/`[]`/`{}` (so `source.[a, b]` is one
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
/// `Normal`; `default` is the overridable fallback; `force` overrides everything.
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
    /// A scalar setting got conflicting values at the same (highest) priority
    /// with no `force`/`default` to break the tie.
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

/// Resolve one scalar setting's contributions by priority (§6): `force`
/// overrides everything; otherwise `normal` wins over `default`; a tie at the
/// deciding priority with differing values is a conflict.
pub fn resolve_scalar(key: &str, contribs: &[Scalar]) -> Result<Option<String>, MergeError> {
    // Distinct values contributed at a given priority (order-independent).
    let pick = |want: Priority| -> Vec<&str> {
        let mut uniq: Vec<&str> = Vec::new();
        for s in contribs.iter().filter(|s| s.priority == want) {
            if !uniq.contains(&s.value.as_str()) {
                uniq.push(s.value.as_str());
            }
        }
        uniq
    };

    for level in [Priority::Force, Priority::Normal, Priority::Default] {
        let uniq = pick(level);
        match uniq.len() {
            0 => continue,
            1 => return Ok(Some(uniq[0].to_string())),
            _ => {
                return Err(MergeError::ScalarConflict {
                    key: key.to_string(),
                    values: uniq.into_iter().map(str::to_string).collect(),
                })
            }
        }
    }
    Ok(None)
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
    fn sugar_dotted_single() {
        assert_eq!(
            parse_package_list("default.ripgrep"),
            vec![Pkg::new("default", "ripgrep")]
        );
    }

    #[test]
    fn sugar_scoped_list_keeps_inner_commas() {
        assert_eq!(
            parse_package_list("default.[ripgrep, fd, jq]"),
            vec![
                Pkg::new("default", "ripgrep"),
                Pkg::new("default", "fd"),
                Pkg::new("default", "jq"),
            ]
        );
    }

    #[test]
    fn sugar_mixed_sources_in_one_list() {
        // The example from unified-ecosystem §5.
        assert_eq!(
            parse_package_list("default.[ripgrep, fd], unstable.neovim"),
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
                Scalar::normal("on2"),
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
