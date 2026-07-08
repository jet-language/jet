//! Convert a parsed `PackManifest` into the compiler's `Manifest::Manifest`
//! (E1206), and the `jet new` template generator.

use super::{DepSource, PackManifest};
use crate::Diagnostics::Diagnostic;
use crate::Jetpack::RefSpec::Source;

/// Convert a parsed `PackManifest` into the compiler's `Manifest::Manifest`
/// — the type `loader.rs`/`fetch.rs`/`lock.rs` operate on. `raw` is the
/// original `pkg.jet` text (kept for comment-preserving `jet add`/`remove`
/// edits, mirroring the old `jet.toml` `Manifest::raw`).
pub fn to_manifest(pm: &PackManifest, raw: &str) -> Result<crate::Manifest::Manifest, Diagnostic> {
    use crate::Manifest::{DepSpec, GitSelector, Manifest, PackageMeta as MPackageMeta};
    use std::collections::BTreeMap;

    let mut dependencies = BTreeMap::new();
    for dep in &pm.deps {
        let spec = match &dep.source {
            // S59/D-CFFI2: a native C-library link dep is not a Jet package —
            // it is resolved by Source/CFFI.rs into linker flags, never realized
            // as source or written to the package lock. Skip it here.
            DepSource::CLib { .. } => continue,
            DepSource::Version(v) => DepSpec::Registry(v.clone()),
            DepSource::Git { url, selector } => DepSpec::Git {
                url: url.clone(),
                selector: match selector {
                    GitSelector::Tag(t) => GitSelector::Tag(t.clone()),
                    GitSelector::Branch(b) => GitSelector::Branch(b.clone()),
                    GitSelector::Rev(r) => GitSelector::Rev(r.clone()),
                },
            },
            DepSource::Provider {
                provider: Source::Path,
                target,
            } => DepSpec::Path {
                path: target.clone(),
            },
            DepSource::Provider {
                provider: Source::Github,
                target,
            } => {
                let Some((owner_repo, rev)) = target.rsplit_once('/') else {
                    return Err(bad_dep_shape(
                        &dep.name,
                        "a `github@owner/repo/rev` dependency needs a pinned rev as its last segment; use the inline `{ git: \"...\", branch/tag: \"...\" }` form to track a moving branch or tag",
                    ));
                };
                DepSpec::Git {
                    url: format!("https://github.com/{owner_repo}"),
                    selector: GitSelector::Rev(rev.to_string()),
                }
            }
            DepSource::Provider { provider, .. } => {
                return Err(bad_dep_shape(
                    &dep.name,
                    &format!(
                        "`{}` is not a valid source for a Jet library dependency — use `path@`, `github@`, or an inline git struct",
                        provider.label()
                    ),
                ));
            }
        };
        dependencies.insert(dep.name.clone(), spec);
    }

    Ok(Manifest {
        package: MPackageMeta {
            name: pm.package.name.clone(),
            version: pm.package.version.clone(),
            edition: pm.package.edition.clone(),
            jet_constraint: pm.package.jet_constraint.clone(),
            description: pm.package.description.clone(),
            license: pm.package.license.clone(),
            repository: pm.package.repository.clone(),
            layer: pm.package.layer,
        },
        dependencies,
        dependencies_rust: BTreeMap::new(),
        raw: raw.to_string(),
    })
}

fn bad_dep_shape(name: &str, why: &str) -> Diagnostic {
    Diagnostic::error(
        "E1206",
        format!("dependency `{name}` has an invalid shape"),
        why.to_string(),
        "see docs/spec/syntax-decisions.md D-JPK23 for the dependency ref forms".to_string(),
        None,
    )
}

/// Generate a `pkg.jet` template for `jet new`.
pub fn new_template(name: &str, annotated: bool) -> String {
    let ver = crate::Manifest::COMPILER_VERSION;
    if annotated {
        format!(
            r#"payload: {{
    name:    "{name}",
    version: "0.1.0",
    jet:     ">={ver}",
    description: "",
    license: "MIT OR Apache-2.0",
    repository: "",
}}

// Jet package dependencies:
// deps: {{
//     helpers:  path@../helpers,
//     parsekit: {{ git: "https://github.com/acme/parsekit", tag: "v0.4.1" }},
// }}
"#
        )
    } else {
        format!(
            r#"payload: {{
    name:    "{name}",
    version: "0.1.0",
    jet:     ">={ver}",
    description: "",
    license: "MIT OR Apache-2.0",
    repository: "",
}}

deps: {{
}}
"#
        )
    }
}
