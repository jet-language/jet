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
/// D-JPK-ADAPTNAME1 (card #176).
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
    /// and a reviewed trust grant: package, staged source, platform, recipe,
    /// and the complete declared capability set all participate.
    pub fn build_identity(&self, package: &str, source_digest: &str, platform: &str) -> String {
        let capabilities = self.declared_capabilities().join(",");
        let identity = format!(
            "jet-build-hook-v1\npackage={package}\nsource={source_digest}\nplatform={platform}\nrecipe={}\ncapabilities={capabilities}\n",
            self.recipe_hash()
        );
        format!("build-sha256:{}", SHA256::sha256_hex(identity.as_bytes()))
    }
}
