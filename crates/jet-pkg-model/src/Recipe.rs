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
        let mut writer = CanonicalWriter::new("jet.build-recipe.v2");
        writer.usize(self.steps.len());
        for (index, step) in self.steps.iter().enumerate() {
            writer.usize(index);
            match step {
                BuildStep::Fetch { url, sha256 } => {
                    writer.str("fetch");
                    writer.str(url);
                    writer.str(sha256);
                }
                BuildStep::Exec { tool, args } => {
                    writer.str("exec");
                    writer.str(tool);
                    writer.strs(args.iter().map(String::as_str));
                }
                BuildStep::Install { src, dest } => {
                    writer.str("install");
                    writer.str(src);
                    writer.str(dest);
                }
                BuildStep::InstallTree { src, dest } => {
                    writer.str("install-tree");
                    writer.str(src);
                    writer.str(dest);
                }
            }
        }
        format!("sha256-{}", SHA256::sha256_hex(&writer.bytes))
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
        self.build_identity_for_source_with_dependencies(
            package,
            provider_source,
            source_digest,
            platform,
            &[],
        )
    }

    /// Build the approval subject for a hook whose tool dependencies are
    /// declared by the surrounding adapter plan. Dependency refs are part of
    /// authority: changing a tool's provider/source must invalidate both the
    /// cache identity and the exact trust grant, even when the recipe bytes
    /// stay unchanged.
    pub fn build_identity_for_source_with_dependencies(
        &self,
        package: &str,
        provider_source: &str,
        source_digest: &str,
        platform: &str,
        dependencies: &[String],
    ) -> String {
        let mut dependencies = dependencies.to_vec();
        dependencies.sort();
        let mut writer = CanonicalWriter::new("jet-build-hook.v3");
        writer.str(package);
        writer.str(provider_source);
        writer.str(source_digest);
        writer.str(platform);
        writer.str(&self.recipe_hash());
        writer.strs(self.declared_capabilities().iter().map(String::as_str));
        writer.strs(dependencies.iter().map(String::as_str));
        format!("build-sha256:{}", SHA256::sha256_hex(&writer.bytes))
    }
}

/// Length-frame every field so delimiters, newlines, and empty values cannot
/// make two different declared plans share one identity.
struct CanonicalWriter {
    bytes: Vec<u8>,
}

impl CanonicalWriter {
    fn new(domain: &str) -> Self {
        let mut writer = Self { bytes: Vec::new() };
        writer.str(domain);
        writer
    }

    fn usize(&mut self, value: usize) {
        self.bytes.extend_from_slice(&(value as u64).to_be_bytes());
    }

    fn str(&mut self, value: &str) {
        self.bytes
            .extend_from_slice(&(value.len() as u64).to_be_bytes());
        self.bytes.extend_from_slice(value.as_bytes());
    }

    fn strs<'a>(&mut self, values: impl IntoIterator<Item = &'a str>) {
        let values = values.into_iter().collect::<Vec<_>>();
        self.usize(values.len());
        for value in values {
            self.str(value);
        }
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

    #[test]
    fn hook_identity_binds_all_authority_facts() {
        let recipe = BuildRecipe {
            steps: vec![BuildStep::Install {
                src: "a".to_string(),
                dest: "bin/a".to_string(),
            }],
        };
        let identity = |package, source, digest, platform| {
            recipe.build_identity_for_source(package, source, digest, platform)
        };
        let base = identity("tool", "registry:stable", "source-a", "linux-x86_64");

        assert_ne!(
            base,
            identity("other", "registry:stable", "source-a", "linux-x86_64")
        );
        assert_ne!(
            base,
            identity("tool", "registry:next", "source-a", "linux-x86_64")
        );
        assert_ne!(
            base,
            identity("tool", "registry:stable", "source-b", "linux-x86_64")
        );
        assert_ne!(
            base,
            identity("tool", "registry:stable", "source-a", "darwin-arm64")
        );
        assert_ne!(
            base,
            BuildRecipe {
                steps: vec![BuildStep::InstallTree {
                    src: "a".to_string(),
                    dest: "bin/a".to_string(),
                }],
            }
            .build_identity_for_source(
                "tool",
                "registry:stable",
                "source-a",
                "linux-x86_64"
            )
        );
    }

    #[test]
    fn hook_identity_binds_declared_tool_dependencies() {
        let recipe = BuildRecipe {
            steps: vec![BuildStep::Exec {
                tool: "cc".to_string(),
                args: vec!["main.c".to_string()],
            }],
        };
        let identity = |dependencies: &[String]| {
            recipe.build_identity_for_source_with_dependencies(
                "tool",
                "./vendor/tool",
                "source",
                "linux-x86_64",
                dependencies,
            )
        };
        let stable = identity(&["cc@default".to_string()]);
        assert_ne!(stable, identity(&["cc@trusted".to_string()]));
        assert_eq!(
            stable,
            identity(&["cc@default".to_string()]),
            "repeated builds must derive one identity from the same declared facts"
        );
        assert_eq!(
            identity(&["cc@default".to_string(), "ar@default".to_string()]),
            identity(&["ar@default".to_string(), "cc@default".to_string()]),
            "dependency ordering must not make repeated builds diverge"
        );
    }

    #[test]
    fn canonical_hash_frames_steps_and_fields() {
        let first = BuildRecipe {
            steps: vec![BuildStep::Exec {
                tool: "cc\0ar".to_string(),
                args: vec!["main".to_string()],
            }],
        };
        let second = BuildRecipe {
            steps: vec![BuildStep::Exec {
                tool: "cc".to_string(),
                args: vec!["ar\0main".to_string()],
            }],
        };
        assert_ne!(
            first.recipe_hash(),
            second.recipe_hash(),
            "field boundaries must be part of recipe identity"
        );
        assert_eq!(first.recipe_hash(), first.recipe_hash());
    }

    #[test]
    fn canonical_hook_identity_frames_authority_fields() {
        let recipe = BuildRecipe::default();
        let first = recipe.build_identity_for_source_with_dependencies(
            "pkg\nsource",
            "provider",
            "digest",
            "linux",
            &["tool\nref".to_string()],
        );
        let second = recipe.build_identity_for_source_with_dependencies(
            "pkg",
            "source\nprovider",
            "digest",
            "linux",
            &["tool\nref".to_string()],
        );
        assert_ne!(first, second);
    }
}
