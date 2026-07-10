struct RealizedPackage {
    entry: Store::StoreEntry,
    _lease: Option<Store::CacheLease>,
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
        Self {
            entry: realized.entry,
            _lease: realized.lease,
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
        self._lease
            .as_ref()
            .map_or_else(|| Ok(PathBuf::from(path)), |lease| lease.stable_path(path))
    }

    fn set_consumption_override(&mut self, path: PathBuf) {
        self.consumption_override = Some(path);
    }
}

fn first_party_package_ref(table: &RefSpec::SourceTable, package: &str) -> Option<String> {
    table
        .declarations()
        .into_iter()
        .find(|(_, _, via)| *via == RefSpec::ProviderKind::Core)
        .map(|(name, _, _)| format!("{name}:{package}"))
}

fn jetos_runtime_package_ref(
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

fn desktop_default_required_packages(system: &SystemPlan) -> &'static [&'static str] {
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

fn realize_ref(
    theme: &Theme,
    roots: &Store::Roots,
    flags: &OsFlags,
    table: &RefSpec::SourceTable,
    spec: &RefSpec::RefSpec,
    name_w: usize,
) -> Option<RealizedPackage> {
    theme.status(&format!("resolving {} ...", theme.bold(&spec.raw)));
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
    };
    match Store::realize_verified(
        roots,
        &ctx,
        Store::RealizeRequest::Package { spec, table },
    ) {
        Ok(realized) => {
            theme.row(
                &realized.entry.name,
                name_w,
                &realized.entry.version,
                realized.source_state.label(),
            );
            theme.detail(&theme.gray(&realized.entry.out));
            Some(RealizedPackage::from_verified(realized))
        }
        Err(Store::RealizeError::Integrity(failure)) => {
            Store::report_integrity(theme, &failure);
            None
        }
        Err(e) => {
            theme.error(
                "could not realize a jetos package",
                &format!("provider failed for `{}`: {e:?}", spec.raw),
                "check the source ref, or rerun without --offline if this source needs fetching.",
            );
            None
        }
    }
}

fn try_realize_ref(
    theme: &Theme,
    roots: &Store::Roots,
    flags: &OsFlags,
    table: &RefSpec::SourceTable,
    spec: &RefSpec::RefSpec,
    name_w: usize,
) -> Result<RealizedPackage, String> {
    theme.status(&format!("resolving {} ...", theme.bold(&spec.raw)));
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
    };
    let realized = Store::realize_verified(
        roots,
        &ctx,
        Store::RealizeRequest::Package { spec, table },
    )
    .map_err(|e| format!("verified realization failed for `{}`: {e:?}", spec.raw))?;
    theme.row(
        &realized.entry.name,
        name_w,
        &realized.entry.version,
        realized.source_state.label(),
    );
    theme.detail(&theme.gray(&realized.entry.out));
    Ok(RealizedPackage::from_verified(realized))
}
