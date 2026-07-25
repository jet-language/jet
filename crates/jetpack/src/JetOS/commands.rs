use super::entry::default_config_path;
use super::generation::build_generation;
use super::generation_files::{
    diff_packages, dir_size_bytes, generation_ordinal, read_generation_packages,
    render_plan_json,
};
use super::generations_activation::{
    activate_generation, find_rollback_generation, generation_named, latest_generation_for,
    prove_activation, read_generations, render_generation_proof_json,
};
use super::installer_media::{write_image_variant_artifacts, write_installer_media};
use super::load_validate::{load_target, parse_target_or_report};
use super::nixos_backend::cmd_migrate_compare_nixos;
use super::nixos_import::cmd_import;
use super::types::OsFlags;
use crate::Output::Theme;
use crate::Syntax;
use std::fs;
use std::path::Path;

pub(super) fn cmd_check(theme: &Theme, args: &[String]) -> i32 {
    let Some(target) = parse_target_or_report(theme, args.first().map(String::as_str)) else {
        return 2;
    };
    match load_target(theme, &target) {
        Some((_, system)) => {
            theme.ok(&format!(
                "jetos {} checked ({})",
                theme.bold(&system.name),
                system.target
            ));
            0
        }
        None => 2,
    }
}

pub(super) fn cmd_plan(theme: &Theme, args: &[String], flags: &OsFlags) -> i32 {
    let Some(target) = parse_target_or_report(theme, args.first().map(String::as_str)) else {
        return 2;
    };
    let Some((_, system)) = load_target(theme, &target) else {
        return 2;
    };
    let plan = render_plan_json(&system, &[], None);
    if flags.json {
        println!("{plan}");
    } else {
        theme.ok(&format!("jetos plan for {}", theme.bold(&system.name)));
        println!("{plan}");
    }
    0
}

pub(super) fn cmd_proof(theme: &Theme, args: &[String], flags: &OsFlags) -> i32 {
    let Some(target) = parse_target_or_report(theme, args.first().map(String::as_str)) else {
        return 2;
    };
    if load_target(theme, &target).is_none() {
        return 2;
    }
    let generation = flags
        .name
        .as_deref()
        .and_then(|name| generation_named(&target.host, name))
        .or_else(|| {
            if flags.name.is_none() {
                latest_generation_for(&target.host)
            } else {
                None
            }
        });
    let Some(gen) = generation else {
        theme.error_coded(
            "E1278",
            "jetos proof is missing",
            &format!(
                "no built generation exists for `{}`; proof is read from generation artifacts.",
                target.host
            ),
                "run `jet os build <host>` first, using the same `--name` when selecting an exact generation.",
        );
        return 2;
    };
    match render_generation_proof_json(&gen) {
        Ok(proof) => {
            if flags.json {
                println!("{proof}");
            } else {
                theme.ok(&format!(
                    "jetos proof for {} generation {}",
                    theme.bold(&gen.host),
                    theme.bold(&gen.name)
                ));
                println!("{proof}");
            }
            0
        }
        Err(e) => {
            theme.error_coded(
                "E1278",
                "jetos proof is incomplete",
                &format!("reading proof artifacts for `{}` failed: {e}", gen.name),
                "run `jet os build <host>` again so plan, proof, provenance, health, and rollback facts are regenerated.",
            );
            2
        }
    }
}

pub(super) fn cmd_build(theme: &Theme, args: &[String], flags: &OsFlags, activate: bool) -> i32 {
    let Some(target) = parse_target_or_report(theme, args.first().map(String::as_str)) else {
        return 2;
    };
    let Some((plan, system)) = load_target(theme, &target) else {
        return 2;
    };
    if activate {
        if let Some(existing) = flags
            .name
            .as_deref()
            .and_then(|name| generation_named(&system.name, name))
        {
            if !prove_activation(theme, &existing, &system) {
                return 2;
            }
            return match activate_generation(&existing) {
                Ok(()) => {
                    theme.ok(&format!(
                        "jetos generation {} activated",
                        theme.bold(&existing.name)
                    ));
                    0
                }
                Err(error) => {
                    theme.error(
                        "could not activate the generation",
                        &format!("updating the current/default pointers failed: {error}"),
                        "check permissions on the Jetpack root, or set JETPACK_ROOT.",
                    );
                    2
                }
            };
        }
    }
    // Tier 3 (D-FE-CLI1): `switch` is the mutation — it diffs the outgoing
    // generation against the one this build produces and gates on Apply?
    // before activating. `jetos build` never activates, so it never touches
    // the running system and needs no gate (and no diff work).
    let outgoing_ordinal = generation_ordinal(&system.name);
    let outgoing_packages = if activate {
        latest_generation_for(&system.name)
            .map(|g| read_generation_packages(&g.path))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    match build_generation(theme, &plan, &system, flags, &target.config) {
        Some(gen) => {
            theme.ok(&format!(
                "jetos generation {} built for {}",
                theme.bold(&gen.name),
                theme.bold(&system.name)
            ));
            theme.detail(&gen.path.display().to_string());
            if activate {
                if !prove_activation(theme, &gen, &system) {
                    return 2;
                }
                let diff = diff_packages(&outgoing_packages, &read_generation_packages(&gen.path));
                if !diff.is_empty() {
                    let incoming_ordinal = outgoing_ordinal + 1;
                    theme.plan_gen_header(outgoing_ordinal, incoming_ordinal);
                    let name_w = diff.iter().map(|d| d.name.len()).max().unwrap_or(0).max(8);
                    for row in &diff {
                        theme.plan_row(row.mark, &row.name, name_w, &row.from, &row.to);
                    }
                    let download_bytes: u64 = diff
                        .iter()
                        .filter_map(|d| d.out.as_deref())
                        .map(|out| dir_size_bytes(Path::new(out)))
                        .sum();
                    if download_bytes > 0 {
                        theme.download_line(download_bytes);
                    }
                    if !theme.confirm_apply(flags.assume_yes) {
                        // Plan-only: the generation stays built and recorded
                        // (rollback-visible via `jet os generations`), but
                        // the running system is untouched.
                        return 0;
                    }
                    theme.applied_header(incoming_ordinal);
                    for row in &diff {
                        if !matches!(row.mark, crate::Output::PlanMark::Remove) {
                            theme.ready_row(&row.name, name_w, &row.to);
                        }
                    }
                }
                match activate_generation(&gen) {
                    Ok(()) => theme.ok(&format!(
                        "jetos generation {} activated",
                        theme.bold(&gen.name)
                    )),
                    Err(e) => {
                        theme.error(
                            "could not activate the generation",
                            &format!("updating the current/default pointers failed: {e}"),
                            "check permissions on the Jetpack root, or set JETPACK_ROOT.",
                        );
                        return 2;
                    }
                }
            }
            0
        }
        None => 2,
    }
}

pub(super) fn cmd_rollback(theme: &Theme, args: &[String]) -> i32 {
    let host = args.first().map_or("", String::as_str);
    if host.is_empty() {
        theme.error(
            "rollback needs a host",
            "`jet os rollback` activates a recorded generation for one host.",
            "run `jet os generations`, then `jet os rollback <host> [<name>]`.",
        );
        return 2;
    }
    let requested = args.get(1).map(String::as_str);
    let Some(gen) = find_rollback_generation(host, requested) else {
        theme.error(
            "no generation is available for rollback",
            &format!("no recorded jetos generation matches host `{host}`."),
            "run `jet os build <host>` or `jet os generations`.",
        );
        return 2;
    };
    match activate_generation(&gen) {
        Ok(()) => {
            theme.ok(&format!(
                "rolled back {} to {}",
                host,
                theme.bold(&gen.name)
            ));
            0
        }
        Err(e) => {
            theme.error(
                "could not activate the rollback generation",
                &format!("updating the current/default pointers failed: {e}"),
                "check permissions on the Jetpack root, or set JETPACK_ROOT.",
            );
            2
        }
    }
}

pub(super) fn cmd_generations(args: &[String]) -> i32 {
    let host = args.first().map(String::as_str);
    let mut gens = read_generations();
    if let Some(host) = host {
        gens.retain(|g| g.host == host);
    }
    gens.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.name.cmp(&a.name))
    });
    for gen in gens {
        println!(
            "{}\t{}\t{}\t{}",
            gen.created_at,
            gen.host,
            gen.name,
            gen.path.display()
        );
    }
    0
}

pub(super) fn cmd_init(theme: &Theme, args: &[String], flags: &OsFlags) -> i32 {
    let host = args.first().map_or("host", String::as_str);
    let path = default_config_path();
    if path.exists() {
        theme.error(
            "config.jet already exists",
            "`jet os init` never overwrites a repo's OS config.",
            "edit config.jet, or move it aside and run init again.",
        );
        return 2;
    }
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            theme.error(
                "could not create the jetos config directory",
                &format!("creating `{}` failed: {e}", parent.display()),
                "check permissions, or create the directory yourself.",
            );
            return 2;
        }
    }
    let disk = flags
        .manual_disk
        .as_deref()
        .map(|d| {
            format!(
                "            filesystem.root.device: \"{}\",\n",
                d.replace('"', "\\\"")
            )
        })
        .unwrap_or_else(|| "            filesystem.layout: \"guided-ext4\",\n".to_string());
    let contents = format!(
        "module {host} {{\n    system.{host}: {{\n        target: linux.x64,\n        packages: [],\n        services: {{\n            systemd: {{ enable: true, exec: \"/usr/bin/env true\" }},\n        }},\n        options: [\n{disk}            network.hostName: {host},\n            packages.base: true,\n        ],\n    }}\n}}\n"
    );
    match fs::write(&path, contents) {
        Ok(()) => {
            theme.ok(&format!("wrote jetos config {}", path.display()));
            theme.detail("generated systemd-ready system skeleton");
            0
        }
        Err(e) => {
            theme.error(
                "could not write the jetos config",
                &format!("writing `{}` failed: {e}", path.display()),
                "check permissions, or pass a writable HOME.",
            );
            2
        }
    }
}

pub(super) fn cmd_lift(theme: &Theme, args: &[String], flags: &OsFlags) -> i32 {
    let host = args.first().map_or("host", String::as_str);
    let root = args.get(1).map_or("/", String::as_str);
    let import_args = vec![
        root.to_string(),
        Syntax::OS_IMPORT_FLAG_HOST.to_string(),
        host.to_string(),
        Syntax::OS_IMPORT_FLAG_FACTS_ONLY.to_string(),
    ];
    cmd_import(theme, &import_args, flags)
}

pub(super) fn cmd_migrate(theme: &Theme, args: &[String], flags: &OsFlags) -> i32 {
    if !invoked_by_root_jet() {
        theme.error(
            "NixOS comparison is available only through root `jet`",
            "D-JOS-MIGRATIONVERB1=A authorizes only `jet os migrate compare-nixos`; direct engine front doors cannot reach the migration backend.",
            "run `jet os migrate compare-nixos <host> --out <dir>`.",
        );
        return 2;
    }
    let Some((action, rest)) = args.split_first() else {
        theme.error(
            "migration needs an action",
            "D-JOS-MIGRATIONVERB1=A permits only an explicit NixOS comparison migration.",
            "run `jet os migrate compare-nixos <host> --out <dir>`.",
        );
        return 2;
    };
    if action != Syntax::OS_MIGRATION_COMPARE_NIXOS {
        theme.error(
            &format!("`{action}` is not a jetos migration action"),
            "The only migration action is `compare-nixos`.",
            "run `jet os migrate compare-nixos <host> --out <dir>`.",
        );
        return 2;
    }
    let Some(target) = parse_target_or_report(theme, rest.first().map(String::as_str)) else {
        return 2;
    };
    let Some(out_index) = rest
        .iter()
        .position(|arg| arg == Syntax::OS_MIGRATION_FLAG_OUT)
    else {
        theme.error(
            "NixOS comparison needs an output directory",
            "`--out <dir>` receives the proved NixOS image, boot proof, guest fact, and receipt.",
            "run `jet os migrate compare-nixos <host> --out <dir>`.",
        );
        return 2;
    };
    let Some(out) = rest.get(out_index + 1).filter(|value| !value.starts_with('-')) else {
        theme.error(
            "NixOS comparison output directory is missing",
            "`--out` must be followed by one directory.",
            "run `jet os migrate compare-nixos <host> --out <dir>`.",
        );
        return 2;
    };
    if out_index != 1 || rest.len() != 3 {
        theme.error(
            "NixOS comparison has unsupported arguments",
            "The migration action accepts one host and one `--out <dir>` argument.",
            "run `jet os migrate compare-nixos <host> --out <dir>`.",
        );
        return 2;
    }
    let Some((plan, system)) = load_target(theme, &target) else {
        return 2;
    };
    cmd_migrate_compare_nixos(theme, &plan.table, &system, Path::new(out), flags)
}

#[cfg(target_os = "linux")]
fn invoked_by_root_jet() -> bool {
    let Ok(parent) = std::env::var(Syntax::ROOT_ENGINE_DISPATCH_PID_ENV) else {
        return false;
    };
    let Ok(parent) = parent.parse::<u32>() else {
        return false;
    };
    let Ok(status) = fs::read_to_string("/proc/self/status") else {
        return false;
    };
    let Some(actual_parent) = status
        .lines()
        .find_map(|line| line.strip_prefix("PPid:"))
        .and_then(|value| value.trim().parse::<u32>().ok())
    else {
        return false;
    };
    if parent != actual_parent {
        return false;
    }
    fs::read_link(format!("/proc/{parent}/exe"))
        .ok()
        .and_then(|path| path.file_stem().map(|name| name == Syntax::BINARY_NAME))
        .unwrap_or(false)
}

#[cfg(not(target_os = "linux"))]
fn invoked_by_root_jet() -> bool {
    false
}

pub(super) fn cmd_image(theme: &Theme, args: &[String], flags: &OsFlags) -> i32 {
    let Some(target) = parse_target_or_report(theme, args.first().map(String::as_str)) else {
        return 2;
    };
    let Some((plan, system)) = load_target(theme, &target) else {
        return 2;
    };
    let Some(gen) = build_generation(theme, &plan, &system, flags, &target.config) else {
        return 2;
    };
    let disk = flags.manual_disk.as_deref().unwrap_or("guided-ext4");
    match write_installer_media(&gen, &system, disk) {
        Ok(path) => match write_image_variant_artifacts(&gen, &system) {
            Ok(variant_proof) => {
                theme.detail(&format!(
                    "wrote image variant proof {}",
                    variant_proof.display()
                ));
                theme.ok(&format!(
                    "wrote jetos installer media proof {}",
                    path.display()
                ));
                0
            }
            Err(e) => {
                theme.error(
                    "could not write jetos image variants",
                    &format!("writing image variant artifacts failed: {e}"),
                    "check permissions on the Jetpack root, or set JETPACK_ROOT.",
                );
                2
            }
        },
        Err(e) => {
            theme.error(
                "could not write the jetos installer media",
                &format!("writing installer media artifacts failed: {e}"),
                "check permissions on the Jetpack root, or set JETPACK_ROOT.",
            );
            2
        }
    }
}
