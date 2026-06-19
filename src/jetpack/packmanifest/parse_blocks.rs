//! Block-body parsers for the `pkg.jet` manifest: `payload:`, `deps:`,
//! `packages:`.

use super::helpers::{key_value_entries, top_level_commas, unquote};
use super::{
    Dep, DepSource, ManifestError, PackageEntry, PackageKind, PackageMeta,
};
use crate::jetpack::refspec;
use crate::syntax;

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
        } else if trimmed.contains(syntax::REF_PROVIDER_AT) {
            match refspec::classify_provider_ref(trimmed) {
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
        (Some(t), None, None) => crate::manifest::GitSelector::Tag(t),
        (None, Some(b), None) => crate::manifest::GitSelector::Branch(b),
        (None, None, Some(r)) => crate::manifest::GitSelector::Rev(r),
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
        // D-ILE1: `kind` is optional. A bare `name` lets the kind be inferred
        // from the module's `fn main`; `name: library` / `name: executable` /
        // `name: { kind: … }` states it explicitly (an explicit kind wins).
        let (name, kind) = match entry.split_once(':') {
            Some((k, v)) => {
                let name = k.trim().to_string();
                let value = v.trim();
                let kind = if value == syntax::PACKAGE_KIND_LIBRARY {
                    PackageKind::Library
                } else if value == syntax::PACKAGE_KIND_EXECUTABLE {
                    PackageKind::Executable
                } else if let Some(inner) = value.strip_prefix('{') {
                    let inner = inner.trim_end().strip_suffix('}').unwrap_or(inner.trim_end());
                    parse_package_entry_block(&name, inner)?
                } else {
                    return Err(ManifestError::BadPackageKind {
                        name,
                        value: value.to_string(),
                    });
                };
                (name, Some(kind))
            }
            None => (entry.to_string(), None),
        };
        packages.push(PackageEntry { name, kind });
    }
    Ok(packages)
}

fn parse_package_entry_block(name: &str, body: &str) -> Result<PackageKind, ManifestError> {
    for (key, value) in key_value_entries(body) {
        if key == syntax::PACKAGE_FIELD_KIND {
            let v = value.trim();
            if v == syntax::PACKAGE_KIND_LIBRARY {
                return Ok(PackageKind::Library);
            } else if v == syntax::PACKAGE_KIND_EXECUTABLE {
                return Ok(PackageKind::Executable);
            } else {
                return Err(ManifestError::BadPackageKind {
                    name: name.to_string(),
                    value: v.to_string(),
                });
            }
        }
    }
    Err(ManifestError::MalformedPackageEntry {
        name: name.to_string(),
    })
}
