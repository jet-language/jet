use super::options_rendering::{clean_symbol, option_value};
use super::types::{OSFlags, GNOME_DESKTOP_PACKAGES};
use crate::Output::Theme;
use crate::Provider;
use crate::RefSpec;
use crate::Store;
use crate::Trust;
use jet_env_model::ModuleEval::SystemPlan;
use std::path::{Path, PathBuf};

pub(super) struct RealizedPackage {
    entry: Store::StoreEntry,
    lease: Store::CacheLease,
    original_out: PathBuf,
    original_reference: String,
    consumption_override: Option<PathBuf>,
}

impl std::ops::Deref for RealizedPackage {
    type Target = Store::StoreEntry;

    fn deref(&self) -> &Self::Target {
        &self.entry
    }
}

impl RealizedPackage {
    fn from_verified(realized: Store::VerifiedRealization) -> Self {
        let original_out = realized.original_output().to_path_buf();
        let original_reference = realized.original_reference().to_string();
        let (entry, _source_state, lease) = realized.into_parts();
        Self {
            entry,
            lease,
            original_out,
            original_reference,
            consumption_override: None,
        }
    }

    pub(super) fn consumption_path(&self, path: &str) -> std::io::Result<PathBuf> {
        if let Some(root) = &self.consumption_override {
            let relative = Path::new(path)
                .strip_prefix(&self.entry.out)
                .map_err(|_| std::io::Error::other("package member escapes realized output"))?;
            return Ok(root.join(relative));
        }
        self.lease.stable_path(path)
    }

    pub(super) fn set_consumption_override(&mut self, path: PathBuf) {
        self.consumption_override = Some(path);
    }

    pub(super) fn original_output(&self) -> &Path {
        &self.original_out
    }

    pub(super) fn original_reference(&self) -> &str {
        &self.original_reference
    }

    /// Hangar identities only. Provider-native paths such as `/nix/store/...`
    /// remain provider roots and never enter Jetpack lifecycle target sets.
    pub(super) fn output_digests(&self, roots: &Store::Roots) -> Vec<String> {
        let objects = roots.hangar_dir().join("objects");
        let mut digests = self
            .entry
            .named_outputs
            .values()
            .filter(|digest| canonical_hangar_digest(digest) && objects.join(digest).is_dir())
            .cloned()
            .collect::<Vec<_>>();
        if digests.is_empty()
            && !self.entry.envelope.output_hash.is_empty()
            && canonical_hangar_digest(&self.entry.envelope.output_hash)
            && objects.join(&self.entry.envelope.output_hash).is_dir()
        {
            digests.push(self.entry.envelope.output_hash.clone());
        }
        digests.sort();
        digests.dedup();
        digests
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum RealizeRefError {
    Provider(String),
    BuildRejected,
}

#[derive(Debug, PartialEq, Eq)]
enum CoreBuildAuthorizationError {
    Identity(String),
    Denied,
}

/// Gate the exact Core Cargo action before Store realization can reach a
/// provider build hook. JetOS uses the same identity and trust gate as the
/// regular package realization path.
fn authorize_core_build(
    theme: &Theme,
    trust_store: &Path,
    spec: &RefSpec::RefSpec,
    table: &RefSpec::SourceTable,
    ctx: &Provider::Ctx<'_>,
    bypass: bool,
) -> Result<(), CoreBuildAuthorizationError> {
    match Provider::approval_facts(spec, table, ctx) {
        Ok(Some(identity)) => {
            crate::RuntimePolicy::warn_sandbox_fallback(theme);
            Trust::gate_build_identity(theme, trust_store, &identity, bypass)
                .map_err(|_| CoreBuildAuthorizationError::Denied)
        }
        Ok(None) => Ok(()),
        Err(reason) => Err(CoreBuildAuthorizationError::Identity(reason)),
    }
}

fn report_core_build_identity_error(theme: &Theme, reason: &str) {
    theme.error_coded(
        "E1275",
        "build sandboxing is required but unavailable",
        &format!("could not establish the exact Core Cargo build identity: {reason}"),
        "provide a trusted substitute or approved remote builder, or enable the native sandbox, then retry.",
    );
}

fn canonical_hangar_digest(digest: &str) -> bool {
    digest.len() == 71
        && digest.starts_with("sha256-")
        && digest[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(super) fn first_party_package_ref(
    table: &RefSpec::SourceTable,
    package: &str,
) -> Option<String> {
    table
        .declarations()
        .into_iter()
        .find(|(_, _, via)| *via == RefSpec::ProviderKind::Core)
        .map(|(name, _, _)| format!("{package}@{name}"))
}

pub(super) fn jetos_runtime_package_ref(
    table: &RefSpec::SourceTable,
    package: &str,
    offline: bool,
) -> Option<String> {
    if offline {
        first_party_package_ref(table, package)
    } else {
        Some(format!("{package}@nixpkgs"))
    }
}

pub(super) fn desktop_default_required_packages(system: &SystemPlan) -> &'static [&'static str] {
    let requested = option_value(
        system,
        &["services.desktop.profile", "services.desktop.session"],
    )
    .is_some()
        || option_value(system, &["services.displayManager"]).is_some()
        || option_value(system, &["init.defaultTarget"]).as_deref() == Some("graphical.target");
    if !requested {
        return &[];
    }
    let profile = option_value(system, &["services.desktop.profile"])
        .map(|s| clean_symbol(&s))
        .or_else(|| option_value(system, &["services.desktop.session"]).map(|s| clean_symbol(&s)))
        .unwrap_or_else(|| "Default".to_string());
    let profile = profile.to_ascii_lowercase();
    if profile == "default" || profile == "gnome" {
        &GNOME_DESKTOP_PACKAGES
    } else {
        &[]
    }
}

pub(super) fn realize_ref(
    theme: &Theme,
    roots: &Store::Roots,
    flags: &OSFlags,
    table: &RefSpec::SourceTable,
    spec: &RefSpec::RefSpec,
    project_dir: &Path,
    name_w: usize,
    mut progress: Option<(&mut crate::Output::LiveRegion<'_>, usize, usize)>,
) -> Option<RealizedPackage> {
    if let Some((live, step, total)) = progress.as_mut() {
        live.set_dependency_status(
            "Building System",
            *step,
            *total,
            spec.source.label(),
            &spec.package,
            "Resolving",
        );
    } else {
        theme.status(&format!("Resolving {} ...", theme.bold(&spec.raw)));
    }
    let store_dir = roots.hangar_dir();
    let fixtures =
        if flags.offline && Provider::uses_nix_provider(spec, table, flags.offline, &store_dir) {
            Provider::fixtures_from_env(flags.fixtures.clone())
        } else {
            flags.fixtures.clone()
        };
    let ctx = Provider::Ctx {
        fixtures: fixtures.as_deref(),
        store_dir: &store_dir,
        offline: flags.offline,
        project_dir: Some(project_dir),
        nix_index: None,
        nix_roots: None,
    };
    let trust_store = Trust::store_path();
    match authorize_core_build(
        theme,
        &trust_store,
        spec,
        table,
        &ctx,
        flags.trust,
    ) {
        Ok(()) => {}
        Err(CoreBuildAuthorizationError::Denied) => {
            if let Some((live, _, _)) = progress.as_mut() {
                live.clear();
            }
            return None;
        }
        Err(CoreBuildAuthorizationError::Identity(reason)) => {
            if let Some((live, _, _)) = progress.as_mut() {
                live.clear();
            }
            report_core_build_identity_error(theme, &reason);
            return None;
        }
    }
    match Store::realize_verified(roots, &ctx, Store::RealizeRequest::Package { spec, table }) {
        Ok(realized) => {
            if let Some((live, _, _)) = progress.as_mut() {
                live.clear();
            }
            let entry = realized.metadata();
            theme.row(
                &entry.name,
                name_w,
                &entry.version,
                realized.source_state().label(),
            );
            theme.detail(&theme.gray(&entry.out));
            Some(RealizedPackage::from_verified(realized))
        }
        Err(Store::RealizeError::Integrity(failure)) => {
            if let Some((live, _, _)) = progress.as_mut() {
                live.clear();
            }
            Store::report_integrity(theme, &failure);
            None
        }
        Err(e) => {
            if let Some((live, _, _)) = progress.as_mut() {
                live.clear();
            }
            theme.error(
                "could not realize a jetos package",
                &format!("provider failed for `{}`: {e:?}", spec.raw),
                "check the source ref, or rerun without --offline if this source needs fetching.",
            );
            None
        }
    }
}

pub(super) fn try_realize_ref(
    theme: &Theme,
    roots: &Store::Roots,
    flags: &OSFlags,
    table: &RefSpec::SourceTable,
    spec: &RefSpec::RefSpec,
    project_dir: &Path,
    name_w: usize,
    mut progress: Option<(&mut crate::Output::LiveRegion<'_>, usize, usize)>,
) -> Result<RealizedPackage, RealizeRefError> {
    if let Some((live, step, total)) = progress.as_mut() {
        live.set_dependency_status(
            "Building System",
            *step,
            *total,
            spec.source.label(),
            &spec.package,
            "Resolving",
        );
    } else {
        theme.status(&format!("Resolving {} ...", theme.bold(&spec.raw)));
    }
    let store_dir = roots.hangar_dir();
    let fixtures =
        if flags.offline && Provider::uses_nix_provider(spec, table, flags.offline, &store_dir) {
            Provider::fixtures_from_env(flags.fixtures.clone())
        } else {
            flags.fixtures.clone()
        };
    let ctx = Provider::Ctx {
        fixtures: fixtures.as_deref(),
        store_dir: &store_dir,
        offline: flags.offline,
        project_dir: Some(project_dir),
        nix_index: None,
        nix_roots: None,
    };
    let trust_store = Trust::store_path();
    match authorize_core_build(
        theme,
        &trust_store,
        spec,
        table,
        &ctx,
        flags.trust,
    ) {
        Ok(()) => {}
        Err(CoreBuildAuthorizationError::Denied) => {
            if let Some((live, _, _)) = progress.as_mut() {
                live.clear();
            }
            return Err(RealizeRefError::BuildRejected);
        }
        Err(CoreBuildAuthorizationError::Identity(reason)) => {
            if let Some((live, _, _)) = progress.as_mut() {
                live.clear();
            }
            report_core_build_identity_error(theme, &reason);
            return Err(RealizeRefError::BuildRejected);
        }
    }
    let realized =
        Store::realize_verified(roots, &ctx, Store::RealizeRequest::Package { spec, table })
            .map_err(|e| {
                if let Some((live, _, _)) = progress.as_mut() {
                    live.clear();
                }
                RealizeRefError::Provider(format!(
                    "verified realization failed for `{}`: {e:?}",
                    spec.raw
                ))
            })?;
    if let Some((live, _, _)) = progress.as_mut() {
        live.clear();
    }
    let entry = realized.metadata();
    theme.row(
        &entry.name,
        name_w,
        &entry.version,
        realized.source_state().label(),
    );
    theme.detail(&theme.gray(&entry.out));
    Ok(RealizedPackage::from_verified(realized))
}
