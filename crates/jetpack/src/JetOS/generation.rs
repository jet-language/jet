fn build_generation(
    theme: &Theme,
    plan: &EnvPlan,
    system: &SystemPlan,
    flags: &OsFlags,
) -> Option<Generation> {
    let roots = Store::resolve();
    let dir = generation_dir(system, flags.name.as_deref());
    if dir.exists() && fs::remove_dir_all(&dir).is_err() {
        theme.error(
            "could not prepare the jetos generation",
            &format!("removing stale generation `{}` failed.", dir.display()),
            "check permissions on the Jetpack root, or choose a different generation name.",
        );
        return None;
    }
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
        // Real tier: the hidden system backend realizes the whole nixpkgs
        // closure inside the disk build, so per-package realization here
        // would only duplicate the work against the registry instead of the
        // declared pin. First-party (path) packages still realize.
        if flags.real_tier && is_nixpkgs_source(&spec.source, &plan.table) {
            continue;
        }
        let entry = match realize_ref(theme, &roots, flags, &plan.table, &spec, name_w) {
            Some(entry) => entry,
            None => return None,
        };
        realized.push(entry);
    }
    let boot = boot_profile(system);
    // In the real tier the hidden system backend realizes the kernel from the
    // pinned package set, so a *defaulted* kernel needs no first-party
    // package here. An explicit `boot.kernel` still goes through the backend
    // mapping, which rejects unsupported kernels loudly (E1291).
    let kernel_defaulted =
        option_value(system, &["boot.kernel", "kernel.package"]).is_none();
    // An explicit `.CachyOS` is satisfied in the real tier by a declared
    // `nix-cachyos-kernel` flake source — the hidden backend realizes the
    // kernel from that overlay, and rejects the option loudly otherwise.
    let cachyos_source_declared = plan
        .table
        .declarations()
        .into_iter()
        .any(|(_, upstream, _)| {
            upstream
                .strip_prefix("github:")
                .and_then(|rest| rest.split('/').nth(1))
                .map(|repo| repo.eq_ignore_ascii_case("nix-cachyos-kernel"))
                .unwrap_or(false)
        });
    if boot.kernel == "CachyOS"
        && !(flags.real_tier && (kernel_defaulted || cachyos_source_declared))
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
        && !flags.real_tier
        && !realized
            .iter()
            .any(|entry| entry.name == SYSTEMD_INIT_PACKAGE)
    {
        let Some(raw) =
            jetos_runtime_package_ref(&plan.table, SYSTEMD_INIT_PACKAGE, flags.offline)
        else {
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
    // Real tier: the hidden NixOS backend realizes display-manager/session
    // packages from nixpkgs via mapped desktop options. Skip first-party
    // GNOME scaffolding here (same rule as the CachyOS kernel skip above).
    if !flags.real_tier {
        for package in desktop_default_required_packages(system) {
            if realized.iter().any(|entry| entry.name == *package) {
                continue;
            }
            let Some(raw) = jetos_runtime_package_ref(&plan.table, package, flags.offline) else {
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
    }
    if !run_kernel_bootstrap_builder(theme, &boot, &mut realized, !flags.offline, &dir) {
        return None;
    }
    if !validate_boot_payloads(theme, &boot, &realized) {
        return None;
    }
    if let Err(e) = write_generation_files(&dir, system, &realized, plan) {
        theme.error(
            "could not write the jetos generation",
            &format!("writing `{}` failed: {e}.", dir.display()),
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
