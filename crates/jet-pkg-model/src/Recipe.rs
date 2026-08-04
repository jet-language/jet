//! `BuildRecipe` data model (D-JPK-ADAPTER1=A safety contract).
//!
//! A `BuildRecipe` turns a staged source tree into a realized package under a
//! confined, auditable build. This is the pure **data** half of the build
//! substrate — the struct/enum the `Build(BuildRecipe)` plan variant carries.
//! The engine (`validate`/`run`/`run_logged`, sandboxing, fetch/exec/install,
//! trust gate) stays in `jetpack`'s `Recipe.rs`, which imports `BuildRecipe`
//! from here (card #367 slice 4, data-down / engine-up).
//!
//! std-only (I6).

use crate::SHA256;

/// One step of a build recipe. Names are internal; the user-facing spellings are
/// the finite `Recipe.build(steps: […])` forms from D-JPK-BUILDRECIPE1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildStep {
    /// A locked network fetch. `sha256` must be present; an empty hash is
    /// ungranted ambient network (`E1236`).
    Fetch { url: String, sha256: String },
    /// Run a build tool. `tool` must be the name of a realized `Pkg` dep in the
    /// `BuildContext.tools` map — never resolved from host PATH (`E1238`).
    Exec { tool: String, args: Vec<String> },
    /// Copy `src` (relative to the source dir) to `dest` under the output root.
    /// `dest` must resolve inside the output root (`E1237`).
    Install { src: String, dest: String },
    /// Copy a whole directory tree relative to the source dir into `dest`
    /// under the output root. Used by `Recipe.copy()`.
    InstallTree { src: String, dest: String },
}

/// A build recipe over a staged source tree.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BuildRecipe {
    pub steps: Vec<BuildStep>,
}

impl BuildRecipe {
    /// A stable content hash of the recipe, used by the trust gate.
    pub fn recipe_hash(&self) -> String {
        let mut data = Vec::new();
        for step in &self.steps {
            match step {
                BuildStep::Fetch { url, sha256 } => {
                    data.extend_from_slice(b"fetch\0");
                    data.extend_from_slice(url.as_bytes());
                    data.push(0);
                    data.extend_from_slice(sha256.as_bytes());
                }
                BuildStep::Exec { tool, args } => {
                    data.extend_from_slice(b"exec\0");
                    data.extend_from_slice(tool.as_bytes());
                    for a in args {
                        data.push(0);
                        data.extend_from_slice(a.as_bytes());
                    }
                }
                BuildStep::Install { src, dest } => {
                    data.extend_from_slice(b"install\0");
                    data.extend_from_slice(src.as_bytes());
                    data.push(0);
                    data.extend_from_slice(dest.as_bytes());
                }
                BuildStep::InstallTree { src, dest } => {
                    data.extend_from_slice(b"install-tree\0");
                    data.extend_from_slice(src.as_bytes());
                    data.push(0);
                    data.extend_from_slice(dest.as_bytes());
                }
            }
            data.push(b'\n');
        }
        format!("sha256-{}", SHA256::sha256_hex(&data))
    }

    /// The exact authority requested by this recipe. This is deliberately
    /// derived from the finite step graph, not from a host environment or a
    /// caller-supplied label.
    pub fn declared_capabilities(&self) -> Vec<String> {
        let mut capabilities = Vec::new();
        for step in &self.steps {
            let capability = match step {
                BuildStep::Fetch { .. } => "net.fetch".to_string(),
                BuildStep::Exec { tool, .. } => format!("exec:{tool}"),
                BuildStep::Install { .. } | BuildStep::InstallTree { .. } => "fs.write".to_string(),
            };
            if !capabilities.iter().any(|existing| existing == &capability) {
                capabilities.push(capability);
            }
        }
        capabilities.sort();
        capabilities
    }

    /// Bind a build hook to every fact that can change its authority or
    /// result. The returned identity is suitable for both the action-cache key
    /// and a reviewed trust grant: package, provider/source, staged source,
    /// platform, recipe, and the complete declared capability set all
    /// participate.
    pub fn build_identity(&self, package: &str, source_digest: &str, platform: &str) -> String {
        self.build_identity_for_source(package, "", source_digest, platform)
    }

    /// Build the canonical identity used by a provider-backed hook. The
    /// compatibility-shaped [`Self::build_identity`] entry point remains for
    /// callers that do not have a provider/source label; production providers
    /// must use this source-bound form.
    pub fn build_identity_for_source(
        &self,
        package: &str,
        provider_source: &str,
        source_digest: &str,
        platform: &str,
    ) -> String {
        let capabilities = self.declared_capabilities().join(",");
        let identity = format!(
            "jet-build-hook-v2\npackage={package}\nprovider-source={provider_source}\nsource={source_digest}\nplatform={platform}\nrecipe={}\ncapabilities={capabilities}\n",
            self.recipe_hash()
        );
        format!("build-sha256:{}", SHA256::sha256_hex(identity.as_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::{BuildRecipe, BuildStep};

    #[test]
    fn hook_identity_binds_provider_source() {
        let recipe = BuildRecipe {
            steps: vec![BuildStep::InstallTree {
                src: ".".to_string(),
                dest: ".".to_string(),
            }],
        };
        let local = recipe.build_identity_for_source(
            "tool",
            "./vendor/tool",
            "sha256-source",
            "linux-x86_64",
        );
        let remote = recipe.build_identity_for_source(
            "tool",
            "github:owner/tool",
            "sha256-source",
            "linux-x86_64",
        );
        assert_ne!(local, remote);
    }

    #[test]
    fn hook_identity_binds_capability_set() {
        let copy = BuildRecipe {
            steps: vec![BuildStep::InstallTree {
                src: ".".to_string(),
                dest: ".".to_string(),
            }],
        };
        let exec = BuildRecipe {
            steps: vec![BuildStep::Exec {
                tool: "cc".to_string(),
                args: vec!["-c".to_string(), "main.c".to_string()],
            }],
        };
        let copy_id = copy.build_identity_for_source("tool", "./tool", "source", "linux-x86_64");
        let exec_id = exec.build_identity_for_source("tool", "./tool", "source", "linux-x86_64");
        assert_ne!(copy_id, exec_id);
        assert_eq!(copy.declared_capabilities(), vec!["fs.write"]);
        assert_eq!(exec.declared_capabilities(), vec!["exec:cc"]);
    }
}
