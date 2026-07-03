//! Block-body parsers for the `pkg.jet` manifest: `payload:`, `deps:`,
//! `packages:`, `build:`.

use super::Helpers::{key_value_entries, top_level_commas, unquote};
use super::{
    ApiMode, BuildOptimize, BuildProfileDef, Dep, DepSource, ManifestError, PackageEntry,
    PackageMeta, Target,
};
use crate::Jetpack::RefSpec;
use crate::Syntax;
use std::collections::HashSet;

pub(super) fn parse_package(body: &str) -> Result<PackageMeta, ManifestError> {
    let mut meta = PackageMeta::default();
    let mut have_name = false;
    let mut have_version = false;
    for (key, value) in key_value_entries(body) {
        let v = unquote(&value);
        match key.as_str() {
            "name" => {
                meta.name = v;
                have_name = true;
            }
            "version" => {
                meta.version = v;
                have_version = true;
            }
            "edition" => meta.edition = Some(v),
            "license" => meta.license = Some(v),
            "description" => meta.description = Some(v),
            "repository" => meta.repository = Some(v),
            "jet" => meta.jet_constraint = Some(v),
            "target" => meta.target = Some(v),
            "layer" => {
                let raw = value.trim().trim_matches('"');
                meta.layer = Some(crate::Syntax::RuntimeLayer::parse_manifest(raw).ok_or_else(
                    || ManifestError::BadLayer {
                        value: raw.to_string(),
                    },
                )?);
            }
            // Unknown keys are tolerated for forward-compat; the wired loader
            // will turn unknown keys into an E-coded diagnostic.
            _ => {}
        }
    }
    if !have_name {
        return Err(ManifestError::MissingField("name"));
    }
    if !have_version {
        return Err(ManifestError::MissingField("version"));
    }
    Ok(meta)
}

pub(super) fn parse_deps(body: &str) -> Result<Vec<Dep>, ManifestError> {
    let mut deps = Vec::new();
    for (name, value) in key_value_entries(body) {
        let trimmed = value.trim();
        let source = if trimmed.starts_with('"') {
            DepSource::Version(unquote(trimmed))
        } else if let Some(inner) = trimmed.strip_prefix('{') {
            let inner = inner.strip_suffix('}').unwrap_or(inner);
            parse_git_dep(&name, inner)?
        } else if let Some(target) = parse_c_lib_ref(trimmed) {
            // S59/D-CFFI2: a native C-library link dep — `c@system` /
            // `c@"vendor/path"`. Detected before the generic provider-ref branch
            // (which only knows nixpkgs/github/path and would reject `c`).
            DepSource::CLib { target }
        } else if trimmed.contains(Syntax::REF_PROVIDER_AT) {
            match RefSpec::classify_provider_ref(trimmed) {
                Ok(r) => DepSource::Provider {
                    provider: r.provider,
                    target: r.target,
                },
                Err(err) => return Err(ManifestError::BadDepRef { name, err }),
            }
        } else {
            return Err(ManifestError::BadDepValue {
                name,
                value: trimmed.to_string(),
            });
        };
        deps.push(Dep { name, source });
    }
    Ok(deps)
}

/// Detect a native C-library link ref (S59/D-CFFI2): `c@system` or
/// `c@"vendor/path"`. The provider half (before `@`) must be exactly
/// `Syntax::DEP_PROVIDER_C`; the target half is unquoted. Returns the target
/// when matched, else `None` (so the caller falls through to the generic
/// provider-ref branch).
fn parse_c_lib_ref(value: &str) -> Option<String> {
    let (provider, target) = value.split_once(Syntax::REF_PROVIDER_AT)?;
    if provider.trim() != Syntax::DEP_PROVIDER_C {
        return None;
    }
    Some(unquote(target))
}

/// Parse an inline git dependency's body (the text inside `{ … }`):
/// `git: "<url>", tag/branch/rev: "<value>"` — exactly one selector (D-JPK23).
fn parse_git_dep(name: &str, body: &str) -> Result<DepSource, ManifestError> {
    let mut url = None;
    let mut tag = None;
    let mut branch = None;
    let mut rev = None;
    for (key, value) in key_value_entries(body) {
        let v = unquote(&value);
        match key.as_str() {
            "git" => url = Some(v),
            "tag" => tag = Some(v),
            "branch" => branch = Some(v),
            "rev" => rev = Some(v),
            _ => {}
        }
    }
    let Some(url) = url else {
        return Err(ManifestError::BadGitDep {
            name: name.to_string(),
            reason: "missing `git` field",
        });
    };
    let selector = match (tag, branch, rev) {
        (Some(t), None, None) => crate::Manifest::GitSelector::Tag(t),
        (None, Some(b), None) => crate::Manifest::GitSelector::Branch(b),
        (None, None, Some(r)) => crate::Manifest::GitSelector::Rev(r),
        (None, None, None) => {
            return Err(ManifestError::BadGitDep {
                name: name.to_string(),
                reason: "must have exactly one of `tag`, `branch`, `rev`",
            });
        }
        _ => {
            return Err(ManifestError::BadGitDep {
                name: name.to_string(),
                reason: "must have exactly one of `tag`, `branch`, `rev`",
            });
        }
    };
    Ok(DepSource::Git { url, selector })
}

pub(super) fn parse_packages(body: &str) -> Result<Vec<PackageEntry>, ManifestError> {
    let mut packages = Vec::new();
    for entry in top_level_commas(body) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        // D-TGT1: a package declares `targets:`, not `kind:`. A bare `name` leaves
        // targets inferred from the module's `fn run` (D-ILE1); `name: <target>`
        // is the single-target shorthand (D-TGT3); `name: { targets: [ … ] }` lists
        // them.
        let (name, targets, api) = match entry.split_once(':') {
            Some((k, v)) => {
                let name = k.trim().to_string();
                let value = v.trim();
                if let Some(inner) = value.strip_prefix('{') {
                    let inner = inner
                        .trim_end()
                        .strip_suffix('}')
                        .unwrap_or(inner.trim_end());
                    let (targets, api) = parse_package_entry_block(&name, inner)?;
                    (name, targets, api)
                } else {
                    let (target, api) = parse_target(&name, value)?;
                    (name, vec![target], api)
                }
            }
            None => (entry.to_string(), Vec::new(), ApiMode::Inferred),
        };
        packages.push(PackageEntry { name, targets, api });
    }
    Ok(packages)
}

/// Parse one target keyword and validate its optional config block
/// (`executable { entry: "…" }`; D-TGT3/D-TGT4/D-CAP4). The keyword must be a
/// shipped target; the block may carry only `entry:`/`name:`/`api:`, and `api:`
/// must be `stable`/`explicit`.
fn parse_target(name: &str, value: &str) -> Result<(Target, ApiMode), ManifestError> {
    let (keyword, block) = match value.split_once('{') {
        Some((kw, rest)) => {
            let body = rest.trim_end().strip_suffix('}').unwrap_or(rest.trim_end());
            (kw.trim(), Some(body))
        }
        None => (value.trim(), None),
    };
    let kind = match keyword {
        k if k == Syntax::TARGET_LIBRARY => Target::Library,
        k if k == Syntax::TARGET_EXECUTABLE => Target::Executable,
        k if k == Syntax::TARGET_TEST => Target::Test,
        k if k == Syntax::TARGET_EXAMPLE => Target::Example,
        // c80 / D-TGT2: `benchmark` now has a backend — routes `jet bench` at
        // the entry via the existing `#Bench`/`compile_benches_with_path` engine.
        k if k == Syntax::TARGET_BENCHMARK => Target::Benchmark,
        k if Syntax::TARGET_RESERVED.contains(&k) => {
            return Err(ManifestError::ReservedTarget {
                name: name.to_string(),
                value: k.to_string(),
            });
        }
        other => {
            return Err(ManifestError::BadTarget {
                name: name.to_string(),
                value: other.to_string(),
            });
        }
    };
    let api = match block {
        Some(body) => validate_target_block(name, body)?,
        None => ApiMode::Inferred,
    };
    // D-CAP5: only library targets emit capability metadata; `api:` on a non-library
    // target is meaningless and is ignored (never frozen).
    let api = if kind == Target::Library {
        api
    } else {
        ApiMode::Inferred
    };
    Ok((kind, api))
}

/// A target block (`{ entry: …, name: …, api: … }`) accepts only those three
/// fields, and `api:` only `stable`/`explicit` (D-TGT3/D-TGT4/D-CAP4).
fn validate_target_block(name: &str, body: &str) -> Result<ApiMode, ManifestError> {
    let mut api = ApiMode::Inferred;
    for (key, value) in key_value_entries(body) {
        if key == Syntax::TARGET_FIELD_ENTRY || key == Syntax::TARGET_FIELD_NAME {
            // entry/name are free-form strings consumed by the build pipeline.
        } else if key == Syntax::TARGET_FIELD_API {
            let v = unquote(&value);
            api = if v == Syntax::API_MODE_STABLE {
                ApiMode::Stable
            } else if v == Syntax::API_MODE_EXPLICIT {
                ApiMode::Explicit
            } else {
                return Err(ManifestError::BadTargetField {
                    name: name.to_string(),
                    detail: format!(
                        "`{}` must be `{}` or `{}`, not `{}`",
                        Syntax::TARGET_FIELD_API,
                        Syntax::API_MODE_STABLE,
                        Syntax::API_MODE_EXPLICIT,
                        v,
                    ),
                });
            };
        } else {
            return Err(ManifestError::BadTargetField {
                name: name.to_string(),
                detail: format!(
                    "unknown target field `{key}` (allowed: `{}`, `{}`, `{}`)",
                    Syntax::TARGET_FIELD_ENTRY,
                    Syntax::TARGET_FIELD_NAME,
                    Syntax::TARGET_FIELD_API,
                ),
            });
        }
    }
    Ok(api)
}

fn parse_package_entry_block(
    name: &str,
    body: &str,
) -> Result<(Vec<Target>, ApiMode), ManifestError> {
    for (key, value) in key_value_entries(body) {
        if key == Syntax::PACKAGE_FIELD_KIND_REMOVED {
            // D-TGT1: `kind:` was removed in favor of `targets:`.
            return Err(ManifestError::KindFieldRemoved {
                name: name.to_string(),
            });
        }
        if key == Syntax::PACKAGE_FIELD_TARGETS {
            return parse_targets_list(name, value.trim());
        }
    }
    // No `targets:` field — kind is inferred at realize time (D-ILE1). Other
    // fields (version/bin) are accepted and ignored for now.
    Ok((Vec::new(), ApiMode::Inferred))
}

/// D-BUILDPROFILE1: parse the `build: { … }` body. Each entry is either:
///   `name: Build.{ optimize: <level>, … }`  (full dot-ctor form)
///   `name: { optimize: <level>, … }`        (shorthand — Build type is inferred)
/// Both forms extract profile fields. Unknown fields inside `Build.{ … }` are
/// rejected so typos surface at parse time.
pub fn parse_build(body: &str) -> Result<Vec<BuildProfileDef>, ManifestError> {
    let mut profiles = Vec::new();
    let mut seen = HashSet::new();
    for (name, value) in key_value_entries(body) {
        if !seen.insert(name.clone()) {
            return Err(ManifestError::BadBuildProfile {
                name: name.clone(),
                reason: "duplicate profile name in `build { }`",
            });
        }
        let value = value.trim();
        // Strip optional `Build.` prefix (D-BUILDPROFILE1: `Build.{ … }` form).
        let inner_block = if let Some(rest) = value.strip_prefix(Syntax::BUILD_CTOR) {
            // `Build.{ … }` or `Build{ … }` (dot optional in the manifest surface).
            let rest = rest.trim_start_matches('.').trim();
            rest.strip_prefix('{')
                .and_then(|s| s.strip_suffix('}'))
                .map(|s| s.trim())
        } else if let Some(s) = value.strip_prefix('{') {
            // bare `{ … }` (Build type inferred from context in pkg.jet).
            s.strip_suffix('}').map(|s| s.trim())
        } else {
            return Err(ManifestError::BadBuildProfile {
                name: name.clone(),
                reason: "expected `Build.{ optimize: none|basic|full, … }` or `{ optimize: none|basic|full, … }`",
            });
        };
        let inner = inner_block.unwrap_or("");
        let mut optimize_val: Option<String> = None;
        let mut debug_info = false;
        let mut small = false;
        let mut panic = None;
        let mut features = Vec::new();
        let mut env = Vec::new();
        for (key, val) in key_value_entries(inner) {
            if key == Syntax::BUILD_FIELD_OPTIMIZE {
                optimize_val = Some(unquote(&val));
            } else if key == Syntax::BUILD_FIELD_DEBUG_INFO {
                debug_info = parse_bool(&name, &val)?;
            } else if key == Syntax::BUILD_FIELD_SMALL {
                small = parse_bool(&name, &val)?;
            } else if key == Syntax::BUILD_FIELD_PANIC {
                let val = unquote(&val);
                panic = Some(match val.as_str() {
                    s if s == Syntax::BUILD_PANIC_ABORT => super::BuildPanic::Abort,
                    s if s == Syntax::BUILD_PANIC_UNWIND => super::BuildPanic::Unwind,
                    _ => {
                        return Err(ManifestError::BadBuildProfile {
                            name: name.clone(),
                            reason: "unknown `panic:` value — use `abort` or `unwind`",
                        });
                    }
                });
            } else if key == Syntax::BUILD_FIELD_FEATURES {
                features = parse_string_list(&val).map_err(|_| ManifestError::BadBuildProfile {
                    name: name.clone(),
                    reason: "expected `features: [ \"name\", … ]`",
                })?;
            } else if key == Syntax::BUILD_FIELD_ENV {
                let body = val
                    .strip_prefix('{')
                    .and_then(|s| s.strip_suffix('}'))
                    .map(|s| s.trim())
                    .ok_or_else(|| ManifestError::BadBuildProfile {
                        name: name.clone(),
                        reason: "expected `env: { KEY: \"value\", … }`",
                    })?;
                for (k, v) in key_value_entries(body) {
                    env.push((k, unquote(&v)));
                }
            } else {
                return Err(ManifestError::BadBuildProfile {
                    name: name.clone(),
                    reason: "unknown field in `Build.{ … }` — allowed: optimize, debug_info, small, panic, features, env",
                });
            }
        }
        let optimize = match optimize_val.as_deref() {
            Some(s) if s == Syntax::BUILD_OPTIMIZE_NONE => BuildOptimize::None,
            Some(s) if s == Syntax::BUILD_OPTIMIZE_BASIC => BuildOptimize::Basic,
            Some(s) if s == Syntax::BUILD_OPTIMIZE_FULL => BuildOptimize::Full,
            Some(other) => {
                return Err(ManifestError::BadBuildProfile {
                    name: name.clone(),
                    reason: if other.is_empty() {
                        "missing `optimize:` field in `Build.{ … }` value"
                    } else {
                        "unknown optimize level — use `none`, `basic`, or `full`"
                    },
                });
            }
            None => {
                return Err(ManifestError::BadBuildProfile {
                    name: name.clone(),
                    reason: "missing `optimize:` field in `Build.{ … }` value",
                });
            }
        };
        profiles.push(BuildProfileDef {
            name,
            optimize,
            debug_info,
            small,
            panic,
            features,
            env,
        });
    }
    Ok(profiles)
}

/// D-EFFBUDGET1: parse the `effects: { allow: […], deny: […] }` body. Either
/// field may be omitted; an unknown field or an effect name outside the
/// closed D-EFF4 vocabulary is E1221 (`ManifestError::BadEffectsBlock`).
pub(super) fn parse_effects(
    body: &str,
) -> Result<(Option<Vec<String>>, Option<Vec<String>>), ManifestError> {
    let mut allow = None;
    let mut deny = None;
    for (key, value) in key_value_entries(body) {
        if key == Syntax::EFFECTS_FIELD_ALLOW {
            allow = Some(parse_effect_list(
                Syntax::EFFECTS_FIELD_ALLOW,
                value.trim(),
            )?);
        } else if key == Syntax::EFFECTS_FIELD_DENY {
            deny = Some(parse_effect_list(Syntax::EFFECTS_FIELD_DENY, value.trim())?);
        } else {
            return Err(ManifestError::BadEffectsBlock {
                detail: format!(
                    "unknown field `{key}` in `{}: {{ … }}` — allowed: `{}`, `{}`",
                    Syntax::MANIFEST_BLOCK_EFFECTS,
                    Syntax::EFFECTS_FIELD_ALLOW,
                    Syntax::EFFECTS_FIELD_DENY,
                ),
            });
        }
    }
    Ok((allow, deny))
}

/// D-EFFBUDGET1: parse the `grants: { "dep": [Effect], … }` body — the audited
/// per-dependency escape from the `effects:` budget.
pub(super) fn parse_grants(body: &str) -> Result<Vec<(String, Vec<String>)>, ManifestError> {
    let mut out = Vec::new();
    for (key, value) in key_value_entries(body) {
        let dep = unquote(&key);
        let effects = parse_effect_list(&dep, value.trim())?;
        out.push((dep, effects));
    }
    Ok(out)
}

/// A `[ Db, Net ]`-shaped list of effect names, each validated against the
/// closed D-EFF4 vocabulary (`crate::Sema::Effects::Effect`).
fn parse_effect_list(field: &str, value: &str) -> Result<Vec<String>, ManifestError> {
    let names = parse_string_list(value).map_err(|_| ManifestError::BadEffectsBlock {
        detail: format!("`{field}:` must be a list like `[Db, Net]`"),
    })?;
    for name in &names {
        if crate::Sema::Effect::parse(name).is_none() {
            return Err(ManifestError::BadEffectsBlock {
                detail: format!("`{name}` isn't a known effect (see docs/spec — the ten-effect D-EFF4 vocabulary)"),
            });
        }
    }
    Ok(names)
}

fn parse_bool(profile: &str, value: &str) -> Result<bool, ManifestError> {
    match value.trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(ManifestError::BadBuildProfile {
            name: profile.to_string(),
            reason: "expected `true` or `false`",
        }),
    }
}

fn parse_string_list(value: &str) -> Result<Vec<String>, ()> {
    let inner = value
        .trim()
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or(())?;
    let mut out = Vec::new();
    for entry in top_level_commas(inner) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        out.push(unquote(entry));
    }
    Ok(out)
}

/// Parse a `[ library, executable { entry: "…" }, … ]` target list. The package's
/// `api:` mode is the mode of its `library` target (D-CAP4/D-CAP5).
fn parse_targets_list(name: &str, value: &str) -> Result<(Vec<Target>, ApiMode), ManifestError> {
    let inner = value
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(value);
    let mut targets = Vec::new();
    let mut api = ApiMode::Inferred;
    for entry in top_level_commas(inner) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let (target, mode) = parse_target(name, entry)?;
        if mode.freezes() {
            api = mode;
        }
        targets.push(target);
    }
    Ok((targets, api))
}
