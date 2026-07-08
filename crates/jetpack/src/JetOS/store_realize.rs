fn first_party_package_ref(table: &RefSpec::SourceTable, package: &str) -> Option<String> {
    table
        .declarations()
        .into_iter()
        .find(|(_, _, via)| *via == RefSpec::ProviderKind::Core)
        .map(|(name, _, _)| format!("{name}:{package}"))
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
) -> Option<Store::StoreEntry> {
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
    match Provider::realize(spec, table, &ctx) {
        Ok(r) => {
            theme.row(&r.name, name_w, &r.version, r.source_state.label());
            theme.detail(&theme.gray(&r.out));
            match Store::record(
                roots,
                &r.name,
                &r.version,
                &r.reference,
                &r.out,
                &r.bin,
                &r.rlib,
                &r.envelope,
            ) {
                Ok(entry) => Some(entry),
                Err(e) => {
                    theme.error(
                        "could not record the package",
                        &format!("writing to the Jetpack store failed: {e}"),
                        "check permissions on the store root, or set JETPACK_ROOT.",
                    );
                    None
                }
            }
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
) -> Result<Store::StoreEntry, String> {
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
    let r = Provider::realize(spec, table, &ctx)
        .map_err(|e| format!("provider failed for `{}`: {e:?}", spec.raw))?;
    theme.row(&r.name, name_w, &r.version, r.source_state.label());
    theme.detail(&theme.gray(&r.out));
    Store::record(
        roots,
        &r.name,
        &r.version,
        &r.reference,
        &r.out,
        &r.bin,
        &r.rlib,
        &r.envelope,
    )
    .map_err(|e| format!("writing to the Jetpack store failed: {e}"))
}
