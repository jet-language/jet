use super::generation_files::{
    generation_dir, write_generation_files, write_generation_root_proof,
    write_generation_source_proof,
};
use super::generations_activation::{append_generation, now_secs};
use super::kernel_bootstrap::{run_kernel_bootstrap_builder, validate_boot_payloads};
use super::nixos_backend::is_nixpkgs_source;
use super::options_rendering::{boot_profile, option_value};
use super::store_realize::{
    desktop_default_required_packages, first_party_package_ref, jetos_runtime_package_ref,
    realize_ref, try_realize_ref,
};
use super::types::{CACHYOS_KERNEL_PACKAGE, Generation, OsFlags, SYSTEMD_INIT_PACKAGE};
use jet_env_model::ModuleEval::{EnvPlan, SystemPlan};
use crate::Output::Theme;
use crate::RefSpec;
use crate::Store;
use std::fs;
use std::path::Path;

pub(super) fn build_generation(
    theme: &Theme,
    plan: &EnvPlan,
    system: &SystemPlan,
    flags: &OsFlags,
    source_config: &Path,
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
    // D-FE-CLI1: build the real package-edge count up front so both the TTY
    // region and plain transcript report an honest denominator. Runtime
    // packages already named explicitly are not counted twice.
    let explicit_names: std::collections::BTreeSet<&str> =
        system.packages.iter().map(|p| p.name.as_str()).collect();
    let explicit_total = system
        .packages
        .iter()
        .filter(|pkg| {
            !(flags.real_tier
                && plan
                    .table
                    .declarations()
                    .into_iter()
                    .find(|(name, _, _)| name == &pkg.source)
                    .map(|(_, _, via)| via == RefSpec::ProviderKind::Nix)
                    .unwrap_or(pkg.source == "nixpkgs"))
        })
        .count();
    let boot_for_progress = boot_profile(system);
    let implicit_systemd = boot_for_progress.init == "/sbin/init"
        && !flags.real_tier
        && !explicit_names.contains(SYSTEMD_INIT_PACKAGE);
    let implicit_desktop = if flags.real_tier {
        0
    } else {
        desktop_default_required_packages(system)
            .iter()
            .filter(|name| !explicit_names.contains(*name))
            .count()
    };
    let progress_kernel_defaulted =
        option_value(system, &["boot.kernel", "kernel.package"]).is_none();
    let progress_cachyos_source = plan
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
    let implicit_cachyos = boot_for_progress.kernel == "CachyOS"
        && !(flags.real_tier && (progress_kernel_defaulted || progress_cachyos_source))
        && !explicit_names.contains(CACHYOS_KERNEL_PACKAGE);
    let progress_total = explicit_total
        + usize::from(implicit_cachyos)
        + usize::from(implicit_systemd)
        + implicit_desktop;
    let mut progress_step = 0usize;
    let mut live = theme.live_region();
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
                crate::Output::ref_error(theme, &err);
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
        progress_step += 1;
        let entry = match realize_ref(
            theme,
            &roots,
            flags,
            &plan.table,
            &spec,
            name_w,
            Some((&mut live, progress_step - 1, progress_total)),
        ) {
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
                crate::Output::ref_error(theme, &err);
                return None;
            }
        };
        progress_step += 1;
        let entry = match try_realize_ref(
            theme,
            &roots,
            flags,
            &plan.table,
            &spec,
            name_w.max(CACHYOS_KERNEL_PACKAGE.len()),
            Some((&mut live, progress_step - 1, progress_total)),
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
                crate::Output::ref_error(theme, &err);
                return None;
            }
        };
        progress_step += 1;
        let entry = match try_realize_ref(
            theme,
            &roots,
            flags,
            &plan.table,
            &spec,
            name_w.max(SYSTEMD_INIT_PACKAGE.len()),
            Some((&mut live, progress_step - 1, progress_total)),
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
                    crate::Output::ref_error(theme, &err);
                    return None;
                }
            };
            progress_step += 1;
            let entry = match try_realize_ref(
                theme,
                &roots,
                flags,
                &plan.table,
                &spec,
                name_w.max(package.len()),
                Some((&mut live, progress_step - 1, progress_total)),
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
    if let Err(e) = write_generation_source_proof(&dir, source_config) {
        theme.error_coded(
            "E1278",
            "jetos generation source proof is incomplete",
            &format!("binding source and plan hashes failed: {e}"),
            "check generation storage permissions, then rebuild the generation.",
        );
        return None;
    }
    let generation_name = dir.file_name()?.to_string_lossy().into_owned();
    let root_proof = match write_generation_root_proof(
        &dir,
        &system.name,
        &generation_name,
        &realized,
    ) {
        Ok(proof) => proof,
        Err(e) => {
            theme.error(
                "could not seal the jetos generation",
                &format!("writing the complete files/output proof failed: {e}."),
                "rebuild the generation after checking Hangar and generation storage integrity.",
            );
            return None;
        }
    };
    let created_at = now_secs();
    let prepared = match Store::reconcile_external_consumer_root(
        &roots,
        "jetos-generation",
        &format!("{}\0{}", system.name, generation_name),
        &root_proof.witness,
        root_proof.output_digests,
        created_at,
    ) {
        Ok(prepared) => prepared,
        Err(e) => {
            theme.error(
                "could not prepare the jetos generation root",
                &format!("recording its Hangar output roots failed: {e}."),
                "verify the Hangar, then rebuild the generation; no ledger entry was committed.",
            );
            return None;
        }
    };
    if cfg!(debug_assertions)
        && std::env::var_os("JET_TEST_GENERATION_FAILPOINT").as_deref()
            == Some(std::ffi::OsStr::new("after-root-prepare"))
    {
        theme.error(
            "jetos generation test failpoint stopped publication",
            "the external-consumer root is prepared and the generation ledger is still absent.",
            "rerun the same build without JET_TEST_GENERATION_FAILPOINT to exercise recovery.",
        );
        return None;
    }
    let gen = Generation {
        name: generation_name,
        host: system.name.clone(),
        path: dir,
        created_at,
    };
    if append_generation(&gen).is_err() {
        theme.error(
            "could not record the jetos generation",
            "writing the generation ledger failed.",
            "check permissions on the Jetpack root, or set JETPACK_ROOT.",
        );
        return None;
    }
    if cfg!(debug_assertions)
        && std::env::var_os("JET_TEST_GENERATION_FAILPOINT").as_deref()
            == Some(std::ffi::OsStr::new("after-ledger"))
    {
        theme.error(
            "jetos generation test failpoint stopped publication",
            "the generation ledger is durable and the external-consumer root remains prepared.",
            "rerun the same build without JET_TEST_GENERATION_FAILPOINT to exercise recovery.",
        );
        return None;
    }
    if let Some(prepared) = prepared {
        if let Err(e) = Store::commit_external_consumer_root(&roots, &prepared, now_secs()) {
            theme.error(
                "could not commit the jetos generation root",
                &format!("the generation ledger is durable, but its Hangar root commit failed: {e}."),
                "rerun the same build; Jetpack will recover the exact prepared root and ledger entry.",
            );
            return None;
        }
    }
    // Tier 2 close: erase any leftover pinned status and leave one ledger
    // summary, matching jetpack build's region→ledger settle (D-FE-CLI1).
    live.collapse(&format!(
        "generation ready · {} package(s) {}",
        realized.len(),
        theme.green("✓")
    ));
    Some(gen)
}
