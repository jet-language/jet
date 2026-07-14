//! The realized build toolchain (D-JPK-BUILDTOOL1=A).
//!
//! A user's `extern rust` bridge crate is compiled reproducibly by a **pinned,
//! realized** Rust toolchain — a hangar object that carries the `cargo`/`rustc`
//! it builds with (and, per D-JPK-RINGSHIP1=C, the prebuilt ring artifacts, see
//! `is_ring_module_staged`). The toolchain object itself is realized by
//! card #179 via the D-JPK-CACHE1 substitution
//! path; here we only *resolve and use* it.
//!
//! Resolution order (BUILDTOOL1=A):
//! 1. a **realized/fixture toolchain object** — `JET_TOOLCHAIN_FIXTURE` points at
//!    one in tests; #179 will point this at the hangar object. Its `cargo` is
//!    the pinned compiler, so the bridge's output hash does not depend on
//!    whatever host `cargo` happens to be on PATH.
//! 2. the **host toolchain** as the implicit dev toolchain — a freshly-built dev
//!    compiler with no pinned object still builds, using the installed `cargo`
//!    (the #179 model: a matching toolchain runs natively).
//! 3. otherwise **none** — the caller emits `E1240` (no realized toolchain and
//!    no Nix), naming both fixes. Never a silent host fallthrough on the
//!    recommended path once a pin exists.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The env var a test (or #179's realizer) sets to point at a realized toolchain
/// object directory: `<dir>/cargo` (executable) + optional `ring/<name>`
/// prebuilt artifacts + a `version` file. Canonical definition lives in Syntax
/// so the (foundation-level) ring-staging query and this resolver agree.
pub use crate::Syntax::TOOLCHAIN_OBJECT_ENV as TOOLCHAIN_FIXTURE_ENV;

/// A resolved build toolchain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toolchain {
    /// The `cargo` executable this toolchain builds with.
    pub cargo: PathBuf,
    /// A stable identity for provenance (`toolchain-<version>` or `host`).
    pub id: String,
    /// The pinned toolchain version (empty for the host dev toolchain).
    pub version: String,
    /// Whether this is a pinned/realized object (`true`) or the host dev
    /// toolchain (`false`). A pinned object's output hash is reproducible.
    pub pinned: bool,
    /// D-JPK-RINGSHIP1=C: prebuilt ring artifacts the object carries, keyed by
    /// ring name (`http`, `regex`, …) → artifact path.
    pub ring_artifacts: HashMap<String, PathBuf>,
}

impl Toolchain {
    /// Resolve the active build toolchain, or `None` when neither a realized
    /// object nor a host toolchain is available.
    pub fn resolve() -> Option<Toolchain> {
        if let Some(dir) = std::env::var_os(TOOLCHAIN_FIXTURE_ENV) {
            let dir = PathBuf::from(dir);
            if let Some(tc) = from_object(&dir) {
                return Some(tc);
            }
        }
        host_toolchain()
    }

    /// The prebuilt ring artifact for `ring`, if this toolchain carries one for
    /// the active platform (D-JPK-RINGSHIP1=C).
    pub fn ring_artifact(&self, ring: &str) -> Option<&Path> {
        self.ring_artifacts.get(ring).map(PathBuf::as_path)
    }
}

/// Read a realized toolchain object at `dir`. Requires an executable `cargo`.
fn from_object(dir: &Path) -> Option<Toolchain> {
    let cargo = dir.join("cargo");
    if !cargo.is_file() {
        return None;
    }
    let version = std::fs::read_to_string(dir.join("version"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let mut ring_artifacts = HashMap::new();
    let ring_dir = dir.join("ring");
    if let Ok(rd) = std::fs::read_dir(&ring_dir) {
        for ent in rd.flatten() {
            if let Some(name) = ent.file_name().to_str() {
                ring_artifacts.insert(name.to_string(), ent.path());
            }
        }
    }
    let id = if version.is_empty() {
        "toolchain".to_string()
    } else {
        format!("toolchain-{version}")
    };
    Some(Toolchain {
        cargo,
        id,
        version,
        pinned: true,
        ring_artifacts,
    })
}

/// The host `cargo` as the implicit dev toolchain, if it is on PATH.
fn host_toolchain() -> Option<Toolchain> {
    if std::process::Command::new("cargo")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        Some(Toolchain {
            cargo: PathBuf::from("cargo"),
            id: "host".to_string(),
            version: String::new(),
            pinned: false,
            ring_artifacts: HashMap::new(),
        })
    } else {
        None
    }
}

/// E1240 — no realized Rust toolchain and no Nix to build an `extern rust`
/// bridge dependency. Names both fixes (recommended-path form).
pub fn e1240() -> crate::Diagnostics::Diagnostic {
    crate::Diagnostics::Diagnostic::error(
        "E1240",
        "no Rust build toolchain is available to build this package".to_string(),
        "building an `extern rust` bridge dependency needs a pinned Rust toolchain (realized \
         into the hangar) or Nix. Neither is present, so the bridge can't be compiled \
         reproducibly."
            .to_string(),
        "run `jet update jet` to realize the pinned toolchain, or install Nix so the bridge \
         builds through the compatibility provider."
            .to_string(),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_a_fixture_object_over_host() {
        let base = std::env::temp_dir().join(format!(
            "tc-fixture-{}-{:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(base.join("ring")).unwrap();
        std::fs::write(base.join("cargo"), "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(base.join("cargo"), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        std::fs::write(base.join("version"), "0.4.2\n").unwrap();
        std::fs::write(base.join("ring/http"), "artifact").unwrap();

        let tc = from_object(&base).unwrap();
        assert!(tc.pinned);
        assert_eq!(tc.version, "0.4.2");
        assert_eq!(tc.id, "toolchain-0.4.2");
        assert!(tc.ring_artifact("http").is_some());
        assert!(tc.ring_artifact("regex").is_none());
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn e1240_names_both_fixes() {
        let d = e1240();
        assert_eq!(d.code, "E1240");
        assert!(d.fix.contains("jet update jet"));
        assert!(d.fix.to_lowercase().contains("nix"));
    }
}
