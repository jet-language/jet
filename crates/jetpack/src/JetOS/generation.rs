use super::generation_files::{
    GenerationRootProof, generation_dir, sync_generation_tree, validate_generation_root_proof,
    write_generation_files, write_generation_root_proof, write_generation_source_proof,
};
use super::generations_activation::{
    append_generation, generation_ledger_timestamp, now_secs,
};
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
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static GENERATION_STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) fn build_generation(
    theme: &Theme,
    plan: &EnvPlan,
    system: &SystemPlan,
    flags: &OsFlags,
    source_config: &Path,
) -> Option<Generation> {
    let roots = Store::resolve();
    let final_dir = generation_dir(system, flags.name.as_deref());
    let generation_name = final_dir.file_name()?.to_string_lossy().into_owned();
    let published_proof = if final_dir.exists() {
        match validate_generation_root_proof(
            &final_dir,
            &system.name,
            &generation_name,
            source_config,
            &roots,
            flags,
        ) {
            Ok(proof) => Some(proof),
            Err(error) => {
                immutable_generation_error(theme, &final_dir, &error);
                return None;
            }
        }
    } else {
        None
    };
    let dir = generation_staging_dir(&final_dir);
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
    if let Err(e) = write_generation_source_proof(&dir, source_config, flags) {
        theme.error_coded(
            "E1278",
            "jetos generation source proof is incomplete",
            &format!("binding source and plan hashes failed: {e}"),
            "check generation storage permissions, then rebuild the generation.",
        );
        return None;
    }
    let root_proof = match write_generation_root_proof(
        &dir,
        &system.name,
        &generation_name,
        &realized,
        &roots,
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
    if let Err(e) = sync_generation_tree(&dir) {
        theme.error(
            "could not seal the jetos generation",
            &format!("durably syncing the complete generation failed: {e}."),
            "check generation storage integrity, then retry with the same generation name.",
        );
        return None;
    }
    let parent = final_dir.parent()?;
    if let Some(existing) = published_proof {
        let _ = fs::remove_dir_all(&dir);
        if existing != root_proof {
            immutable_generation_error(
                theme,
                &final_dir,
                &std::io::Error::other("current request has a different sealed witness"),
            );
            return None;
        }
    } else if let Err(e) = fs::rename(&dir, &final_dir) {
        if !final_dir.is_dir() {
            theme.error(
                "could not publish the jetos generation",
                &format!("atomically installing `{}` failed: {e}.", final_dir.display()),
                "check generation storage permissions, then retry with the same name.",
            );
            return None;
        }
        let _ = fs::remove_dir_all(&dir);
        let existing = match validate_generation_root_proof(
            &final_dir,
            &system.name,
            &generation_name,
            source_config,
            &roots,
            flags,
        ) {
            Ok(proof) => proof,
            Err(error) => {
                immutable_generation_error(theme, &final_dir, &error);
                return None;
            }
        };
        if existing != root_proof {
            immutable_generation_error(
                theme,
                &final_dir,
                &std::io::Error::other("concurrent publication has a different witness"),
            );
            return None;
        }
    } else if let Err(e) = Store::sync_store_node(parent, true) {
        theme.error(
            "could not publish the jetos generation",
            &format!("syncing the generation directory after atomic publication failed: {e}."),
            "check generation storage integrity, then retry with the same generation name.",
        );
        return None;
    }
    let gen = publish_generation(
        theme,
        &roots,
        &system.name,
        generation_name,
        final_dir,
        root_proof,
    )?;
    // Tier 2 close: erase any leftover pinned status and leave one ledger
    // summary, matching jetpack build's region→ledger settle (D-FE-CLI1).
    live.collapse(&format!(
        "generation ready · {} package(s) {}",
        realized.len(),
        theme.green("✓")
    ));
    Some(gen)
}

fn publish_generation(
    theme: &Theme,
    roots: &Store::Roots,
    host: &str,
    name: String,
    path: PathBuf,
    root_proof: GenerationRootProof,
) -> Option<Generation> {
    let created_at = now_secs();
    let mut gen = Generation {
        name,
        host: host.to_string(),
        path,
        created_at,
    };
    let ledger_at = match generation_ledger_timestamp(&gen, &root_proof.witness) {
        Ok(timestamp) => timestamp,
        Err(error) => {
            immutable_generation_error(theme, &gen.path, &error);
            return None;
        }
    };
    let has_hangar_targets = !root_proof.output_digests.is_empty();
    let prepared = if !has_hangar_targets {
        None
    } else {
        match Store::reconcile_external_consumer_root(
            roots,
            "jetos-generation",
            &format!("{}\0{}", gen.host, gen.name),
            &root_proof.witness,
            root_proof.output_digests,
            created_at,
        ) {
            Ok(prepared) => prepared,
            Err(e) => {
                theme.error(
                    "could not prepare the jetos generation root",
                    &format!("recording its Hangar output roots failed: {e}."),
                    "verify the Hangar, then retry the exact published generation.",
                );
                return None;
            }
        }
    };
    if prepared.is_none() && has_hangar_targets && ledger_at.is_none() {
        immutable_generation_error(
            theme,
            &gen.path,
            &std::io::Error::other("committed lifecycle root has no generation ledger entry"),
        );
        return None;
    }
    if generation_failpoint("after-root-prepare") {
        theme.error(
            "jetos generation test failpoint stopped publication",
            "the immutable generation is durable and its external-consumer root is prepared.",
            "rerun the same build without JET_TEST_GENERATION_FAILPOINT to exercise recovery.",
        );
        return None;
    }
    gen.created_at = match append_generation(&gen, &root_proof.witness) {
        Ok(created_at) => created_at,
        Err(e) => {
            theme.error(
                "could not record the jetos generation",
                &format!("writing the generation ledger failed: {e}."),
                "retry the exact published generation; partial ledger writes are recovered.",
            );
            return None;
        }
    };
    if generation_failpoint("after-ledger") {
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
    if generation_failpoint("after-commit") {
        theme.error(
            "jetos generation test failpoint stopped publication",
            "the immutable generation, ledger, and external-consumer root are committed.",
            "rerun the same build without JET_TEST_GENERATION_FAILPOINT to exercise recovery.",
        );
        return None;
    }
    Some(gen)
}

fn generation_staging_dir(final_dir: &Path) -> PathBuf {
    let sequence = GENERATION_STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let name = final_dir
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    final_dir.with_file_name(format!(
        ".{name}.{}.{}.partial",
        std::process::id(),
        sequence
    ))
}

fn generation_failpoint(name: &str) -> bool {
    cfg!(debug_assertions)
        && std::env::var("JET_TEST_GENERATION_FAILPOINT").as_deref() == Ok(name)
}

fn immutable_generation_error(theme: &Theme, dir: &Path, error: &std::io::Error) {
    theme.error_coded(
        "E1278",
        "jetos generation name is already published",
        &format!("immutable generation `{}` cannot be reused: {error}.", dir.display()),
        "choose a new generation name, or restore the exact sealed generation and retry.",
    );
}
