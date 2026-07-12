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

    fn consumption_path(&self, path: &str) -> std::io::Result<PathBuf> {
        if let Some(root) = &self.consumption_override {
            let relative = Path::new(path).strip_prefix(&self.entry.out).map_err(|_| {
                std::io::Error::other("package member escapes realized output")
            })?;
            return Ok(root.join(relative));
        }
        self.lease.stable_path(path)
    }

    fn set_consumption_override(&mut self, path: PathBuf) {
        self.consumption_override = Some(path);
    }

    fn original_output(&self) -> &Path {
        &self.original_out
    }

    fn original_reference(&self) -> &str {
        &self.original_reference
    }
}

pub(super) fn first_party_package_ref(table: &RefSpec::SourceTable, package: &str) -> Option<String> {
    table
        .declarations()
        .into_iter()
        .find(|(_, _, via)| *via == RefSpec::ProviderKind::Core)
        .map(|(name, _, _)| format!("{name}:{package}"))
}

pub(super) fn jetos_runtime_package_ref(
    table: &RefSpec::SourceTable,
    package: &str,
    offline: bool,
) -> Option<String> {
    if offline {
        first_party_package_ref(table, package)
    } else {
        Some(format!("nixpkgs:{package}"))
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
    flags: &OsFlags,
    table: &RefSpec::SourceTable,
    spec: &RefSpec::RefSpec,
    name_w: usize,
    mut progress: Option<(&mut super::Output::LiveRegion<'_>, usize, usize)>,
) -> Option<RealizedPackage> {
    if let Some((live, step, total)) = progress.as_mut() {
        live.set_dependency_status(
            "building system",
            *step,
            *total,
            spec.source.label(),
            &spec.package,
            "resolving",
        );
    } else {
        theme.status(&format!("resolving {} ...", theme.bold(&spec.raw)));
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
        project_dir: None,
    };
    match Store::realize_verified(
        roots,
        &ctx,
        Store::RealizeRequest::Package { spec, table },
    ) {
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
    flags: &OsFlags,
    table: &RefSpec::SourceTable,
    spec: &RefSpec::RefSpec,
    name_w: usize,
    mut progress: Option<(&mut super::Output::LiveRegion<'_>, usize, usize)>,
) -> Result<RealizedPackage, String> {
    if let Some((live, step, total)) = progress.as_mut() {
        live.set_dependency_status(
            "building system",
            *step,
            *total,
            spec.source.label(),
            &spec.package,
            "resolving",
        );
    } else {
        theme.status(&format!("resolving {} ...", theme.bold(&spec.raw)));
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
        project_dir: None,
    };
    let realized = Store::realize_verified(
        roots,
        &ctx,
        Store::RealizeRequest::Package { spec, table },
    )
    .map_err(|e| {
        if let Some((live, _, _)) = progress.as_mut() {
            live.clear();
        }
        format!("verified realization failed for `{}`: {e:?}", spec.raw)
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
