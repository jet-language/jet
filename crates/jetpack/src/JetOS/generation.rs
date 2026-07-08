fn build_generation(
    theme: &Theme,
    plan: &EnvPlan,
    system: &SystemPlan,
    flags: &OsFlags,
) -> Option<Generation> {
    let roots = Store::resolve();
    let dir = generation_dir(system, flags.name.as_deref());
    fs::create_dir_all(dir.join("packages")).ok()?;
    let name_w = system
        .packages
        .iter()
        .map(|p| p.name.len())
        .max()
        .unwrap_or(1);
    let mut realized = Vec::new();
    for pkg in &system.packages {
        let raw = if pkg.source.is_empty() {
            pkg.name.clone()
        } else {
            format!("{}:{}", pkg.source, pkg.name)
        };
        let spec = match RefSpec::classify_in(&raw, &plan.table) {
            Ok(spec) => spec,
            Err(err) => {
                super::Output::ref_error(theme, &err);
                return None;
            }
        };
        let entry = match realize_ref(theme, &roots, flags, &plan.table, &spec, name_w) {
            Some(entry) => entry,
            None => return None,
        };
        realized.push(entry);
    }
    let boot = boot_profile(system);
    if boot.kernel == "CachyOS"
        && !realized
            .iter()
            .any(|entry| entry.name == CACHYOS_KERNEL_PACKAGE)
    {
        let Some(raw) = first_party_package_ref(&plan.table, CACHYOS_KERNEL_PACKAGE) else {
            theme.error_coded(
                "E1280",
                "jetos CachyOS kernel package is missing",
                "D-JOS-KERNELSRC1=A: `.CachyOS` resolves to a first-party `cachyos-kernel` package with boot artifacts and provenance.",
                "declare a first-party source that provides `cachyos-kernel`, or select a different ratified kernel.",
            );
            return None;
        };
        let spec = match RefSpec::classify_in(&raw, &plan.table) {
            Ok(spec) => spec,
            Err(err) => {
                super::Output::ref_error(theme, &err);
                return None;
            }
        };
        let entry = match try_realize_ref(
            theme,
            &roots,
            flags,
            &plan.table,
            &spec,
            name_w.max(CACHYOS_KERNEL_PACKAGE.len()),
        ) {
            Ok(entry) => entry,
            Err(_) => {
                theme.error_coded(
                    "E1280",
                    "jetos CachyOS kernel package is missing",
                    "D-JOS-KERNELSRC1=A: `.CachyOS` resolves to a first-party `cachyos-kernel` package with boot artifacts and provenance.",
                    "declare a first-party source that provides `cachyos-kernel`, or select a different ratified kernel.",
                );
                return None;
            }
        };
        realized.push(entry);
    }
    if boot.init == "/sbin/init"
        && !realized
            .iter()
            .any(|entry| entry.name == SYSTEMD_INIT_PACKAGE)
    {
        let Some(raw) = first_party_package_ref(&plan.table, SYSTEMD_INIT_PACKAGE) else {
            theme.error_coded(
                "E1281",
                "jetos systemd init package is missing",
                "D-JPK-OSINIT1=A: the default jetos init path is systemd, so the generation needs a first-party `systemd` package with bootable init artifacts.",
                "declare a first-party source that provides `systemd`, or select a ratified init override.",
            );
            return None;
        };
        let spec = match RefSpec::classify_in(&raw, &plan.table) {
            Ok(spec) => spec,
            Err(err) => {
                super::Output::ref_error(theme, &err);
                return None;
            }
        };
        let entry = match try_realize_ref(
            theme,
            &roots,
            flags,
            &plan.table,
            &spec,
            name_w.max(SYSTEMD_INIT_PACKAGE.len()),
        ) {
            Ok(entry) => entry,
            Err(_) => {
                theme.error_coded(
                    "E1281",
                    "jetos systemd init package is missing",
                    "D-JPK-OSINIT1=A: the default jetos init path is systemd, so the generation needs a first-party `systemd` package with bootable init artifacts.",
                    "declare a first-party source that provides `systemd`, or select a ratified init override.",
                );
                return None;
            }
        };
        realized.push(entry);
    }
    for package in desktop_default_required_packages(system) {
        if realized.iter().any(|entry| entry.name == *package) {
            continue;
        }
        let Some(raw) = first_party_package_ref(&plan.table, package) else {
            theme.error_coded(
                "E1288",
                "jetos GNOME desktop package is missing",
                "D-JOS-DESKTOP1=A: the default jetos desktop profile needs first-party GNOME session packages in the system closure.",
                "declare first-party packages for gdm, gnome-session, and gnome-shell, or select a ratified non-GNOME desktop profile.",
            );
            return None;
        };
        let spec = match RefSpec::classify_in(&raw, &plan.table) {
            Ok(spec) => spec,
            Err(err) => {
                super::Output::ref_error(theme, &err);
                return None;
            }
        };
        let entry = match try_realize_ref(
            theme,
            &roots,
            flags,
            &plan.table,
            &spec,
            name_w.max(package.len()),
        ) {
            Ok(entry) => entry,
            Err(_) => {
                theme.error_coded(
                    "E1288",
                    "jetos GNOME desktop package is missing",
                    "D-JOS-DESKTOP1=A: the default jetos desktop profile needs first-party GNOME session packages in the system closure.",
                    "declare first-party packages for gdm, gnome-session, and gnome-shell, or select a ratified non-GNOME desktop profile.",
                );
                return None;
            }
        };
        realized.push(entry);
    }
    if !run_kernel_bootstrap_builder(theme, &boot, &realized) {
        return None;
    }
    if !validate_boot_payloads(theme, &boot, &realized) {
        return None;
    }
    if write_generation_files(&dir, system, &realized, plan).is_err() {
        theme.error(
            "could not write the jetos generation",
            &format!("writing `{}` failed.", dir.display()),
            "check permissions on the Jetpack root, or set JETPACK_ROOT.",
        );
        return None;
    }
    let gen = Generation {
        name: dir.file_name()?.to_string_lossy().into_owned(),
        host: system.name.clone(),
        path: dir,
        created_at: now_secs(),
    };
    if append_generation(&gen).is_err() {
        theme.error(
            "could not record the jetos generation",
            "writing the generation ledger failed.",
            "check permissions on the Jetpack root, or set JETPACK_ROOT.",
        );
        return None;
    }
    Some(gen)
}
