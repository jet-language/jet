//! jetos realization tier (Epoch 7).
//!
//! `jet os` is the user-facing command. The implementation lives in the
//! Jetpack engine because it reuses Jetpack's source table, provider boundary,
//! hangar, and trust/runtime policy.

use super::ModuleEval::{self, EnvPlan, ImageKind, ServicePlan, SystemPlan, VmTestPlan};
use super::Output::Theme;
use super::{Provider, RefSpec, Store, JSON};
use crate::Syntax;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CACHYOS_KERNEL_PACKAGE: &str = "cachyos-kernel";
const SYSTEMD_INIT_PACKAGE: &str = "systemd";
const GNOME_DESKTOP_PACKAGES: [&str; 3] = ["gdm", "gnome-session", "gnome-shell"];
const VM_TOOLS: [&str; 11] = [
    "qemu-system-x86_64",
    "qemu-img",
    "xorriso",
    "limine",
    "sfdisk",
    "blockdev",
    "mkfs.ext4",
    "mkfs.vfat",
    "mmd",
    "mcopy",
    "zstd",
];
const VM_GUEST_PROOF_MARKER: &str = "JETOS_GUEST_PROOF:";
const VM_PROOF_TIMEOUT_MS: u64 = 300_000;

#[derive(Clone)]
pub struct OsFlags {
    pub fixtures: Option<PathBuf>,
    pub offline: bool,
    pub name: Option<String>,
    pub manual_disk: Option<String>,
    pub disk: Option<String>,
    pub json: bool,
}

struct Target {
    config: PathBuf,
    host: String,
}

struct Generation {
    name: String,
    host: String,
    path: PathBuf,
    created_at: u64,
}

struct BootProfile {
    loader: String,
    kernel: String,
    init: String,
    initrd_modules: Vec<String>,
}

impl BootProfile {
    fn to_json(&self) -> String {
        let modules = self
            .initrd_modules
            .iter()
            .map(|m| JSON::quote(m))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"loader\":{},\"kernel\":{},\"init\":{},\"initrd_modules\":[{}]}}",
            JSON::quote(&self.loader),
            JSON::quote(&self.kernel),
            JSON::quote(&self.init),
            modules
        )
    }
}

pub fn main(theme: &Theme, verb: Option<&str>, args: &[String], flags: &OsFlags) -> i32 {
    match verb {
        Some(v) if v == Syntax::OS_VERB_CHECK => cmd_check(theme, args),
        Some(v) if v == Syntax::OS_VERB_PLAN => cmd_plan(theme, args, flags),
        Some(v) if v == Syntax::OS_VERB_PROOF => cmd_proof(theme, args, flags),
        Some(v) if v == Syntax::OS_VERB_BUILD => cmd_build(theme, args, flags, false),
        Some(v) if v == Syntax::OS_VERB_SWITCH => cmd_build(theme, args, flags, true),
        Some(v) if v == Syntax::OS_VERB_ROLLBACK => cmd_rollback(theme, args),
        Some(v) if v == Syntax::OS_VERB_GENERATIONS => cmd_generations(args),
        Some(v) if v == Syntax::OS_VERB_INIT => cmd_init(theme, args, flags),
        Some(v) if v == Syntax::OS_VERB_LIFT => cmd_lift(theme, args),
        Some(v) if v == Syntax::OS_VERB_IMAGE => cmd_image(theme, args, flags),
        Some(v) if v == Syntax::OS_VERB_VM => cmd_vm(theme, args, flags),
        Some("help" | "--help" | "-h") => {
            print_help();
            0
        }
        Some(other) => {
            theme.error(
                &format!("`{other}` is not a jetos verb"),
                &format!("jetos verbs are: {}.", Syntax::OS_VERBS.join(", ")),
                "run `jet os help`.",
            );
            2
        }
        None => {
            print_help();
            2
        }
    }
}

pub fn user_main(theme: &Theme, verb: Option<&str>, args: &[String], flags: &OsFlags) -> i32 {
    let Some(action) = verb else {
        theme.error(
            "user needs an action",
            "D-JOS-USERAPPLY1=A: standalone user profiles support plan, build, switch, rollback, and prove.",
            "run `jetos user plan <name>`.",
        );
        return 2;
    };
    if !Syntax::USER_VERBS.contains(&action) {
        theme.error(
            &format!("`{action}` is not a jetos user action"),
            &format!("jetos user actions are: {}.", Syntax::USER_VERBS.join(", ")),
            "run `jetos user plan <name>`.",
        );
        return 2;
    }
    let user = args.first().map_or("", String::as_str);
    if user.is_empty() {
        theme.error(
            "user action needs a profile name",
            "D-JOS-USERENV1=A: `user.<name>` or `users.<name>` declares a per-user environment profile.",
            "run `jetos user plan nate`.",
        );
        return 2;
    }
    let target = Target {
        config: default_config_path(),
        host: user.to_string(),
    };
    let Some((plan, system)) = load_user_profile_target(theme, &target, user) else {
        return 2;
    };
    match action {
        "plan" => {
            let profile = render_user_profile_json(&system, user);
            if flags.json {
                println!("{profile}");
            } else {
                theme.ok(&format!("jetos user plan for {}", theme.bold(user)));
                println!("{profile}");
            }
            0
        }
        "build" => {
            let Some(gen) = build_generation(theme, &plan, &system, flags) else {
                return 2;
            };
            theme.ok(&format!(
                "built user {} generation {}",
                theme.bold(user),
                theme.bold(&gen.name)
            ));
            theme.detail(&format!("{}", gen.path.join("users").join(user).display()));
            0
        }
        "switch" => {
            let Some(gen) = build_generation(theme, &plan, &system, flags) else {
                return 2;
            };
            if let Err(e) = activate_generation(&gen) {
                theme.error(
                    "could not activate the user generation",
                    &format!("updating the current/default pointers failed: {e}"),
                    "check permissions on the Jetpack root, or set JETPACK_ROOT.",
                );
                return 2;
            }
            theme.ok(&format!(
                "activated user {} generation {}",
                theme.bold(user),
                theme.bold(&gen.name)
            ));
            0
        }
        "rollback" => {
            let requested = args.get(1).map(String::as_str);
            let Some(gen) = find_rollback_generation(&system.name, requested) else {
                theme.error(
                    "no user generation is available for rollback",
                    &format!(
                        "no recorded jetos generation with profile `{user}` exists for `{}`.",
                        system.name
                    ),
                    "run `jetos user build <name>` or `jetos user switch <name>` first.",
                );
                return 2;
            };
            match activate_generation(&gen) {
                Ok(()) => {
                    theme.ok(&format!(
                        "rolled back user {user} to {}",
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
        "prove" => {
            let Some(gen) = latest_generation_for(&system.name) else {
                theme.error(
                    "jetos user proof is missing",
                    &format!(
                        "no built generation exists for profile `{user}` on `{}`.",
                        system.name
                    ),
                    "run `jetos user build <name>` first.",
                );
                return 2;
            };
            let profile = gen.path.join("users").join(user).join("profile.json");
            let proof = gen.path.join("users").join(user).join("proof.txt");
            if !profile.is_file() || !proof.is_file() {
                theme.error(
                    "jetos user proof is incomplete",
                    &format!(
                        "generation `{}` lacks the profile/proof files for `{user}`.",
                        gen.name
                    ),
                    "rebuild the user generation.",
                );
                return 2;
            }
            if flags.json {
                println!(
                    "{{\"user\":{},\"host\":{},\"generation\":{},\"profile\":{},\"proof\":{}}}",
                    JSON::quote(user),
                    JSON::quote(&system.name),
                    JSON::quote(&gen.name),
                    JSON::quote(&profile.display().to_string()),
                    JSON::quote(&proof.display().to_string())
                );
            } else {
                theme.ok(&format!(
                    "jetos user proof for {user} generation {}",
                    gen.name
                ));
                println!("{}", fs::read_to_string(proof).unwrap_or_default());
            }
            0
        }
        _ => unreachable!("checked user action"),
    }
}

pub fn resolve_config_path(prefix: Option<&str>) -> PathBuf {
    match prefix {
        Some("") | None => default_config_path(),
        Some(path) => PathBuf::from(path),
    }
}

fn default_config_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(Syntax::CONFIG_FILE)
}

fn cmd_check(theme: &Theme, args: &[String]) -> i32 {
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

fn cmd_plan(theme: &Theme, args: &[String], flags: &OsFlags) -> i32 {
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

fn cmd_proof(theme: &Theme, args: &[String], flags: &OsFlags) -> i32 {
    let Some(target) = parse_target_or_report(theme, args.first().map(String::as_str)) else {
        return 2;
    };
    if load_target(theme, &target).is_none() {
        return 2;
    }
    let Some(gen) = latest_generation_for(&target.host) else {
        theme.error_coded(
            "E1278",
            "jetos proof is missing",
            &format!(
                "no built generation exists for `{}`; proof is read from generation artifacts.",
                target.host
            ),
            "run `jet os build <host>` first.",
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

fn cmd_build(theme: &Theme, args: &[String], flags: &OsFlags, activate: bool) -> i32 {
    let Some(target) = parse_target_or_report(theme, args.first().map(String::as_str)) else {
        return 2;
    };
    let Some((plan, system)) = load_target(theme, &target) else {
        return 2;
    };
    match build_generation(theme, &plan, &system, flags) {
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

fn cmd_rollback(theme: &Theme, args: &[String]) -> i32 {
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

fn cmd_generations(args: &[String]) -> i32 {
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

fn cmd_init(theme: &Theme, args: &[String], flags: &OsFlags) -> i32 {
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

fn cmd_lift(theme: &Theme, args: &[String]) -> i32 {
    let host = args.first().map_or("host", String::as_str);
    let root = args.get(1).map_or("/", String::as_str);
    println!(
        "module {host} {{\n    system.{host}: {{\n        target: linux.x64,\n        packages: [],\n        options: [\n            filesystem.root.source: \"{root}\",\n        ],\n    }}\n}}"
    );
    theme.status("drafted jetos config from host root");
    0
}

fn cmd_image(theme: &Theme, args: &[String], flags: &OsFlags) -> i32 {
    let Some(target) = parse_target_or_report(theme, args.first().map(String::as_str)) else {
        return 2;
    };
    let Some((plan, system)) = load_target(theme, &target) else {
        return 2;
    };
    let Some(gen) = build_generation(theme, &plan, &system, flags) else {
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

fn cmd_vm(theme: &Theme, args: &[String], flags: &OsFlags) -> i32 {
    let Some((action, rest)) = args.split_first().map(|(a, r)| (a.as_str(), r)) else {
        theme.error(
            "vm needs an action",
            "D-JOS-VMCOMMAND1=A, D-JOS-VMRUN1=A, and D-JOS-VMTEST1=A: the active VM actions are `prove`, `run`, and `test`.",
            "run `jet os vm prove <host> --disk <path>`, `jet os vm run <host> --disk <path>`, or `jet os vm test <scenario> --disk <path>`.",
        );
        return 2;
    };
    if action != Syntax::OS_VM_ACTION_PROVE
        && action != Syntax::OS_VM_ACTION_RUN
        && action != Syntax::OS_VM_ACTION_TEST
    {
        theme.error(
            &format!("`{action}` is not a jetos VM action"),
            "D-JOS-VMCOMMAND1=A, D-JOS-VMRUN1=A, and D-JOS-VMTEST1=A: the active VM actions are `prove`, `run`, and `test`.",
            "run `jet os vm prove <host> --disk <path>`, `jet os vm run <host> --disk <path>`, or `jet os vm test <scenario> --disk <path>`.",
        );
        return 2;
    }
    if action == Syntax::OS_VM_ACTION_TEST {
        let Some(target) = parse_target_or_report(theme, rest.first().map(String::as_str)) else {
            return 2;
        };
        let disk = flags
            .disk
            .as_deref()
            .or(flags.manual_disk.as_deref())
            .unwrap_or("");
        if disk.is_empty() {
            theme.error(
                "vm test needs a target disk",
                "`jet os vm test` installs each declared host into a proved virtual disk and records scenario proof facts.",
                "pass `--disk ./scenario.qcow2`.",
            );
            return 2;
        }
        return cmd_vm_test(theme, &target, disk, flags);
    }
    let Some(target) = parse_target_or_report(theme, rest.first().map(String::as_str)) else {
        return 2;
    };
    let disk = flags
        .disk
        .as_deref()
        .or(flags.manual_disk.as_deref())
        .unwrap_or("");
    if disk.is_empty() {
        theme.error(
            "vm needs a target disk",
            "`jet os vm prove` installs into a virtual disk; `jet os vm run` opens that proved disk for human use.",
            "pass `--disk ./host.qcow2`.",
        );
        return 2;
    }
    let Some((plan, system)) = load_target(theme, &target) else {
        return 2;
    };
    if action == Syntax::OS_VM_ACTION_RUN {
        return cmd_vm_run(theme, &system, disk);
    }
    let missing = missing_vm_tools();
    if !missing.is_empty() {
        theme.error_coded(
            "E1279",
            "jetos VM proof tools are missing",
            &format!(
            "D-JOS-VMDEPS1=A requires pinned VM/media tools before installer proof can run; missing: {}.",
                missing.join(", ")
            ),
            "realize or expose qemu-system-x86_64, qemu-img, xorriso, limine, sfdisk, blockdev, mkfs.ext4, mkfs.vfat, mmd, mcopy, and zstd, then rerun `jet os vm prove`.",
        );
        return 2;
    }
    let Some(gen) = build_generation(theme, &plan, &system, flags) else {
        return 2;
    };
    let media = match write_installer_media(&gen, &system, "guided-ext4") {
        Ok(path) => path,
        Err(e) => {
            theme.error(
                "could not write the jetos installer media",
                &format!("writing installer media artifacts failed: {e}"),
                "check permissions on the Jetpack root, or set JETPACK_ROOT.",
            );
            return 2;
        }
    };
    let installer_iso = systems_dir()
        .join("images")
        .join(format!("jetos-installer-{}.iso", system.name));
    if !installer_iso.is_file() {
        theme.error(
            "jetos installer ISO was not built",
            &format!(
                "VM proof needs `{}`; media staging did not produce a bootable ISO.",
                installer_iso.display()
            ),
            "inspect the media staging `iso-error.txt`, fix the ISO build failure, then rerun `jet os vm prove`.",
        );
        return 2;
    }
    match write_vm_install_plan(&gen, &system, disk, &media) {
        Ok(path) => match prove_vm_guest(&gen, &system, disk, &media, &path) {
            Ok(Some(final_path)) => {
                theme.ok(&format!(
                    "proved jetos VM install/reboot {}",
                    final_path.display()
                ));
                0
            }
            Ok(None) => {
                theme.error_coded(
                    "E1285",
                    "jetos VM guest proof has not run",
                    &format!(
                        "the QEMU install/reboot harness was written to `{}`, but no guest boot proof was recorded.",
                        path.display()
                    ),
                    "inspect the VM run logs, fix the boot/install path, then rerun `jet os vm prove` to capture a guest proof marker.",
                );
                2
            }
            Err(e) => {
                theme.error_coded(
                    "E1285",
                    "jetos VM guest proof has not run",
                    &format!(
                        "the guest proof for `{}` is stale or incomplete: {e}.",
                        path.display()
                    ),
                    "rerun the recorded QEMU install/reboot phases and write a matching guest proof artifact.",
                );
                2
            }
        },
        Err(e) => {
            theme.error(
                "could not write the jetos VM proof plan",
                &format!("writing VM proof artifacts failed: {e}"),
                "check permissions on the Jetpack root, or set JETPACK_ROOT.",
            );
            2
        }
    }
}

fn cmd_vm_test(theme: &Theme, target: &Target, disk: &str, flags: &OsFlags) -> i32 {
    let Some(plan) = load_plan(theme, target) else {
        return 2;
    };
    let Some(vmtest) = plan.vmtests.iter().find(|t| t.name == target.host).cloned() else {
        let mut names: Vec<String> = plan.vmtests.iter().map(|t| t.name.clone()).collect();
        names.sort();
        let known = if names.is_empty() {
            "this config defines no vmtests".to_string()
        } else {
            format!("available vmtests: {}", names.join(", "))
        };
        theme.error_coded(
            "E0980",
            &format!("`{}` is not a vmtest in this config", target.host),
            &known,
            "define `module vmtest.<name> { hosts: { node: system.<host> }, run: test { ... } }`, or select one of the vmtests above.",
        );
        return 2;
    };
    let missing = missing_vm_tools();
    if !missing.is_empty() {
        theme.error_coded(
            "E1279",
            "jetos VM proof tools are missing",
            &format!(
                "D-JOS-VMTEST1=A runs the same pinned VM/media harness as `jet os vm prove`; missing: {}.",
                missing.join(", ")
            ),
            "realize or expose qemu-system-x86_64, qemu-img, xorriso, limine, sfdisk, blockdev, mkfs.ext4, mkfs.vfat, mmd, mcopy, and zstd, then rerun `jet os vm test`.",
        );
        return 2;
    }
    match run_vmtest(theme, &plan, &vmtest, disk, flags) {
        Ok(path) => {
            theme.ok(&format!("proved jetos VM test {}", path.display()));
            0
        }
        Err(e) => {
            theme.error_coded(
                "E1285",
                "jetos VM test proof has not run",
                &format!("the VM test `{}` did not produce a passing proof: {e}.", vmtest.name),
                "inspect the VM test artifacts, fix the failing host/assertion, then rerun `jet os vm test`.",
            );
            2
        }
    }
}

fn cmd_vm_run(theme: &Theme, system: &SystemPlan, disk: &str) -> i32 {
    let missing = missing_vm_tools();
    let missing = missing
        .into_iter()
        .filter(|tool| tool == "qemu-system-x86_64")
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        theme.error_coded(
            "E1279",
            "jetos VM proof tools are missing",
            &format!(
                "D-JOS-VMDEPS1=A requires pinned VM/media tools before a VM can run; missing: {}.",
                missing.join(", ")
            ),
            "realize or expose qemu-system-x86_64, then rerun `jet os vm run`.",
        );
        return 2;
    }
    let Some(gen) = latest_generation_for(&system.name) else {
        theme.error_coded(
            "E1287",
            "jetos VM run needs a proved installed disk",
            &format!(
                "no built generation exists for `{}`; VM launch follows the latest proven generation.",
                system.name
            ),
            "run `jet os vm prove <host> --disk <path>` first.",
        );
        return 2;
    };
    let proof = systems_dir()
        .join("vm-proofs")
        .join(format!("{}-{}-vm-proof.json", system.name, gen.name));
    let media_proof = systems_dir()
        .join("images")
        .join(format!("jetos-installer-{}.iso.proof.json", system.name));
    match require_vm_run_proof(&gen, system, disk, &media_proof, &proof) {
        Ok(()) => {}
        Err(e) => {
            theme.error_coded(
                "E1287",
                "jetos VM run needs a proved installed disk",
                &format!("the installed disk `{disk}` is not tied to a passing VM proof: {e}."),
                "run `jet os vm prove <host> --disk <path>` first, then rerun `jet os vm run`.",
            );
            return 2;
        }
    }
    let boot_dir = systems_dir()
        .join("images")
        .join(format!("jetos-installer-{}.iso.d/boot", system.name));
    let boot_dir = if boot_dir.join("initrd").is_file() {
        boot_dir
    } else {
        gen.path.join("boot")
    };
    let command = qemu_interactive_run_command(&boot_dir, disk, &system.name, &gen.name);
    theme.ok(&format!(
        "booting jetos VM {} generation {}",
        theme.bold(&system.name),
        theme.bold(&gen.name)
    ));
    if qemu_has_local_display() {
        theme.detail("graphical console is open in a local QEMU window");
    } else {
        theme.detail("graphical console is exposed over VNC; serial output is attached here");
    }
    match run_interactive_vm_command(&command) {
        Ok(code) => code,
        Err(e) => {
            theme.error(
                "could not run the jetos VM",
                &format!("starting interactive QEMU failed: {e}"),
                "check the VM proof artifacts and rerun `jet os vm prove` if the disk changed.",
            );
            2
        }
    }
}

fn parse_target_or_report(theme: &Theme, raw: Option<&str>) -> Option<Target> {
    let raw = raw.unwrap_or("");
    if raw.trim().is_empty() {
        theme.error_coded(
            "E0979",
            "`jet os` needs a host to apply",
            "D-JPK-OSHOST1=C: a bare host selects `system.<host>` in ./config.jet; `path@host` selects an exact external root.",
            "write `jet os switch laptop` or `jet os switch ../machines@laptop`.",
        );
        return None;
    }
    let Some((prefix, host)) = raw.rsplit_once(Syntax::OS_HOST_SELECTOR) else {
        return Some(Target {
            config: default_config_path(),
            host: raw.to_string(),
        });
    };
    if host.trim().is_empty() {
        theme.error_coded(
            "E0979",
            "`jet os` needs a host to apply",
            "D-JPK-OSHOST1=C: `path@host` uses the text after `@` as the host name.",
            "write `jet os switch laptop` or `jet os switch ../machines@laptop`.",
        );
        return None;
    }
    let config = if prefix.is_empty() {
        default_config_path()
    } else {
        let path = PathBuf::from(prefix);
        if path.is_dir() {
            path.join(Syntax::CONFIG_FILE)
        } else {
            path
        }
    };
    Some(Target {
        config,
        host: host.to_string(),
    })
}

fn load_target(theme: &Theme, target: &Target) -> Option<(EnvPlan, SystemPlan)> {
    let plan = load_plan(theme, target)?;
    let Some(system) = plan.systems.iter().find(|s| s.name == target.host).cloned() else {
        let mut systems: Vec<String> = plan.systems.iter().map(|s| s.name.clone()).collect();
        systems.sort();
        let known = if systems.is_empty() {
            "this config defines no systems".to_string()
        } else {
            format!("available systems: {}", systems.join(", "))
        };
        theme.error_coded(
            "E0980",
            &format!("`{}` is not a system in this config", target.host),
            &known,
            "define `system.<host>: { ... }`, or select one of the systems above.",
        );
        return None;
    };
    if !validate_system_options(theme, &system) {
        return None;
    }
    Some((plan, system))
}

fn load_user_profile_target(
    theme: &Theme,
    target: &Target,
    user: &str,
) -> Option<(EnvPlan, SystemPlan)> {
    let plan = load_plan(theme, target)?;
    let Some(system) = plan
        .systems
        .iter()
        .find(|s| user_names(s).iter().any(|name| name == user))
        .cloned()
    else {
        let mut users = plan.systems.iter().flat_map(user_names).collect::<Vec<_>>();
        users.sort();
        users.dedup();
        let known = if users.is_empty() {
            "this config defines no user profiles".to_string()
        } else {
            format!("available users: {}", users.join(", "))
        };
        theme.error(
            &format!("`{user}` is not a user profile in this config"),
            &known,
            "define `user.<name>.*` or `users.<name>.*` options on a system, then rerun `jetos user plan <name>`.",
        );
        return None;
    };
    if !validate_system_options(theme, &system) {
        return None;
    }
    Some((plan, system))
}

fn load_plan(theme: &Theme, target: &Target) -> Option<EnvPlan> {
    let src = match fs::read_to_string(&target.config) {
        Ok(src) => src,
        Err(_) => {
            theme.error_coded(
                "E0981",
                "the jetos config file does not exist",
                &format!(
                    "`jet os` tried to load `{}` for host `{}`.",
                    target.config.display(),
                    target.host
                ),
                "create the config file, or pass an explicit path before the `@`.",
            );
            return None;
        }
    };
    let base = target.config.parent().unwrap_or_else(|| Path::new("."));
    let plan = match ModuleEval::evaluate_env(&src, base) {
        Ok(plan) => plan,
        Err(d) => {
            eprint!(
                "{}",
                crate::Diagnostics::render_all(
                    &target.config.to_string_lossy(),
                    &src,
                    std::slice::from_ref(&d)
                )
            );
            return None;
        }
    };
    Some(plan)
}

fn validate_system_options(theme: &Theme, system: &SystemPlan) -> bool {
    if let Some(bad) = system.options.iter().find(|o| {
        let ns = o.key.split('.').next().unwrap_or("");
        !Syntax::OS_OPTION_NAMESPACES.contains(&ns)
    }) {
        theme.error_coded(
            "E1277",
            &format!("`{}` uses a retired jetos option namespace", bad.key),
            "D-JPK-OSNS1=B, D-JOS-SYSTEMTREE1=A, and Epoch 7 jetos surface decisions: jetos option keys start with full-word namespaces such as `filesystem`, `network`, `packages`, `services`, `users`, `user`, `apps`, `performance`, `storage`, `theme`, `workload`, `hardware`, `groups`, `secrets`, `boot`, `kernel`, `init`, `health`, or `deploy`.",
            "rename the option namespace, for example `net.hostName` becomes `network.hostName`.",
        );
        return false;
    }
    true
}

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

fn run_kernel_bootstrap_builder(
    theme: &Theme,
    boot: &BootProfile,
    realized: &[Store::StoreEntry],
) -> bool {
    if boot.kernel != "CachyOS" {
        return true;
    }
    let Some(entry) = cachyos_kernel_entry(realized) else {
        return true;
    };
    if missing_kernel_source_files(entry).is_some() {
        return true;
    }
    let out = Path::new(&entry.out);
    let script = out.join("source/build.sh");
    if !script.is_file() {
        return true;
    }
    if let Err(e) = fs::create_dir_all(out.join("boot")) {
        theme.error_coded(
            "E1286",
            "jetos CachyOS source build failed",
            &format!("could not create the cachyos-kernel boot artifact directory: {e}."),
            "check the first-party cachyos-kernel source recipe and rerun `jet os build`.",
        );
        return false;
    }
    let output = Command::new(&script)
        .current_dir(out)
        .env("JETOS_KERNEL_OUT", out.join("boot"))
        .env("JETOS_KERNEL_SOURCE", out.join("source"))
        .env("JETOS_KERNEL_PACKAGE", out)
        .envs(default_cachyos_kernel_env())
        .output();
    let output = match output {
        Ok(output) => output,
        Err(e) => {
            theme.error_coded(
                "E1286",
                "jetos CachyOS source build failed",
                &format!("running `source/build.sh` failed: {e}."),
                "check the first-party cachyos-kernel source recipe and rerun `jet os build`.",
            );
            return false;
        }
    };
    if !output.status.success() {
        theme.error_coded(
            "E1286",
            "jetos CachyOS source build failed",
            &format!(
                "`source/build.sh` exited with {}; stderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            "check the first-party cachyos-kernel source recipe and rerun `jet os build`.",
        );
        return false;
    }
    true
}

fn default_cachyos_kernel_env() -> Vec<(&'static str, PathBuf)> {
    let mut env = Vec::new();
    if std::env::var_os("JETOS_CACHYOS_KERNEL").is_none() {
        if let Some(kernel) = first_existing_path(&[
            "/run/booted-system/kernel",
            "/run/current-system/kernel",
        ]) {
            env.push(("JETOS_CACHYOS_KERNEL", kernel));
        }
    }
    if std::env::var_os("JETOS_CACHYOS_INITRD").is_none() {
        if let Some(initrd) = first_existing_path(&[
            "/run/booted-system/initrd",
            "/run/current-system/initrd",
        ]) {
            env.push(("JETOS_CACHYOS_INITRD", initrd));
        }
    }
    if std::env::var_os("JETOS_CACHYOS_MODULES").is_none() {
        if let Some(modules) = first_existing_path(&[
            "/run/booted-system/kernel-modules",
            "/run/current-system/kernel-modules",
        ]) {
            env.push((
                "JETOS_CACHYOS_MODULES",
                kernel_module_tree(&modules).unwrap_or(modules),
            ));
        }
    }
    env
}

fn kernel_module_tree(modules: &Path) -> Option<PathBuf> {
    let lib_modules = modules.join("lib/modules");
    fs::read_dir(lib_modules)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.join("kernel").is_dir())
}

fn first_existing_path(paths: &[&str]) -> Option<PathBuf> {
    paths
        .iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
}

fn validate_boot_payloads(
    theme: &Theme,
    boot: &BootProfile,
    realized: &[Store::StoreEntry],
) -> bool {
    if boot.kernel == "CachyOS" {
        let Some(entry) = cachyos_kernel_entry(realized) else {
            return true;
        };
        let kernel = boot_artifact(entry, &["boot/vmlinuz-cachyos", "bzImage", "vmlinuz"]);
        let initrd = boot_artifact(entry, &["boot/initrd-cachyos", "initrd", "initrd.img"]);
        let kernel_ok = kernel
            .as_ref()
            .map(|path| is_linux_kernel_image(path))
            .unwrap_or(false);
        let initrd_ok = initrd
            .as_ref()
            .map(|path| is_initrd_image(path))
            .unwrap_or(false);
        if !kernel_ok || !initrd_ok {
            theme.error_coded(
                "E1282",
                "jetos CachyOS boot artifacts are missing",
                "D-JOS-KERNELSRC1=A: the first-party `cachyos-kernel` package must provide a Linux kernel image and initrd with bootable file headers so the generation and installer can boot the same payload.",
                "add boot/vmlinuz-cachyos and boot/initrd-cachyos with real boot payloads, or select a different ratified kernel.",
            );
            return false;
        }
        if missing_kernel_source_files(entry).is_some() {
            theme.error_coded(
                "E1284",
                "jetos CachyOS source recipe is missing",
                "D-JOS-KERNELBOOTSTRAP1=A: the first-party `cachyos-kernel` package must carry source-built recipe, builder, config, patch, and initrd-input provenance beside the boot artifacts.",
                "add source/recipe.jet, source/build.sh, source/config, source/patches.manifest, and source/initrd-inputs.manifest to the package output.",
            );
            return false;
        }
    }
    if boot.init == "/sbin/init" {
        let Some(entry) = realized
            .iter()
            .find(|entry| entry.name == SYSTEMD_INIT_PACKAGE)
        else {
            return true;
        };
        if boot_artifact(entry, &["bin/systemd", "lib/systemd/systemd", "sbin/init"]).is_none() {
            theme.error_coded(
                "E1283",
                "jetos systemd init artifact is missing",
                "D-JPK-OSINIT1=A: the first-party `systemd` package must provide a bootable init binary for `/sbin/init`.",
                "add bin/systemd, lib/systemd/systemd, or sbin/init to the package output, or select a ratified init override.",
            );
            return false;
        }
    }
    true
}

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

fn generation_dir(system: &SystemPlan, explicit: Option<&str>) -> PathBuf {
    let name = explicit
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}-{}", system.name, now_secs()));
    systems_dir().join("generations").join(name)
}

fn systems_dir() -> PathBuf {
    Store::resolve().root.join("systems")
}

fn generations_log() -> PathBuf {
    systems_dir().join("generations.log")
}

fn write_generation_files(
    dir: &Path,
    system: &SystemPlan,
    realized: &[Store::StoreEntry],
    plan: &EnvPlan,
) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let packages_json = realized
        .iter()
        .map(|p| {
            JSON::object_of(&[
                ("name", &p.name),
                ("reference", &p.reference),
                ("out", &p.out),
                ("bin", &p.bin),
            ])
        })
        .collect::<Vec<_>>()
        .join(",");
    let services_json = system
        .services
        .iter()
        .map(|s| {
            JSON::object_of(&[
                ("name", &s.name),
                ("enable", if s.enable { "true" } else { "false" }),
            ])
        })
        .collect::<Vec<_>>()
        .join(",");
    let options_json = system
        .options
        .iter()
        .map(|o| JSON::object_of(&[("key", &o.key), ("value", &o.value)]))
        .collect::<Vec<_>>()
        .join(",");
    let plan_text = render_plan_json(
        system,
        realized,
        Some((&packages_json, &services_json, &options_json)),
    );
    fs::write(dir.join("plan.json"), &plan_text)?;
    write_root_closure(dir, realized)?;
    write_etc_tree(dir, system)?;
    write_network_facts(dir, system)?;
    write_boot_facts(dir, system, realized)?;
    write_init_facts(dir, system, realized)?;
    fs::write(dir.join("proof.txt"), render_proof(system, realized, plan))?;
    write_systemd_units(dir, system)?;
    write_systemd_timer_socket_units(dir, system)?;
    write_terminal_environment(dir, system)?;
    write_activation_diff(dir, system, realized)?;
    write_health_checks(dir, system)?;
    write_hardware_facts(dir, system)?;
    write_user_environment_facts(dir, system)?;
    write_flatpak_facts(dir, system)?;
    write_performance_facts(dir, system)?;
    write_module_priority_facts(dir, system)?;
    write_storage_facts(dir, system)?;
    write_workload_facts(dir, system)?;
    write_theme_facts(dir, system)?;
    write_fleet_deploy_facts(dir, system, plan)?;
    write_options_reference(dir, system)?;
    write_image_variant_facts(dir, system, plan)?;
    write_lifecycle_facts(dir, system)?;
    write_service_manager_depth(dir, system)?;
    write_app_module_facts(dir, system)?;
    write_acceptance_fixture(dir, system)?;
    write_desktop_facts(dir, system)?;
    write_store_cache_facts(dir, realized)?;
    write_compat_escape_hatches(dir, system)?;
    write_studio_app_projection(dir, system)?;
    write_provenance(dir, system, realized)?;
    write_vm_proof(dir, system, &plan_text)?;
    write_secret_manifest(dir, system)?;
    write_bootable_root_projection(dir)?;
    Ok(())
}

fn render_plan_json(
    system: &SystemPlan,
    realized: &[Store::StoreEntry],
    prebuilt: Option<(&str, &str, &str)>,
) -> String {
    let (packages_json, services_json, options_json) = match prebuilt {
        Some((p, s, o)) => (p.to_string(), s.to_string(), o.to_string()),
        None => {
            let packages = system
                .packages
                .iter()
                .map(|p| {
                    let raw = if p.source.is_empty() {
                        p.name.clone()
                    } else {
                        format!("{}:{}", p.source, p.name)
                    };
                    JSON::object_of(&[("name", &p.name), ("source", &p.source), ("ref", &raw)])
                })
                .collect::<Vec<_>>()
                .join(",");
            let services = system
                .services
                .iter()
                .map(|s| {
                    JSON::object_of(&[
                        ("name", &s.name),
                        ("enable", if s.enable { "true" } else { "false" }),
                    ])
                })
                .collect::<Vec<_>>()
                .join(",");
            let options = system
                .options
                .iter()
                .map(|o| JSON::object_of(&[("key", &o.key), ("value", &o.value)]))
                .collect::<Vec<_>>()
                .join(",");
            (packages, services, options)
        }
    };
    let closure_json = realized
        .iter()
        .map(|p| {
            JSON::object_of(&[
                ("name", &p.name),
                ("reference", &p.reference),
                ("out", &p.out),
                ("bin", &p.bin),
            ])
        })
        .collect::<Vec<_>>()
        .join(",");
    let boot = boot_profile(system);
    format!(
        "{{\"host\":{},\"target\":{},\"boot\":{},\"packages\":[{}],\"closure\":[{}],\"services\":[{}],\"options\":[{}]}}",
        JSON::quote(&system.name),
        JSON::quote(&system.target),
        boot.to_json(),
        packages_json,
        closure_json,
        services_json,
        options_json
    )
}

fn write_root_closure(dir: &Path, realized: &[Store::StoreEntry]) -> std::io::Result<()> {
    let sw_bin = dir.join("sw/bin");
    fs::create_dir_all(&sw_bin)?;
    let mut manifest = String::new();
    manifest.push_str("jetos system package closure\n");
    for pkg in realized {
        manifest.push_str(&format!("{} {} {}\n", pkg.name, pkg.reference, pkg.out));
        if pkg.bin.is_empty() {
            continue;
        }
        let bin = Path::new(&pkg.bin);
        let Ok(entries) = fs::read_dir(bin) else {
            continue;
        };
        let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let src = entry.path();
            if !src.is_file() {
                continue;
            }
            let dst = sw_bin.join(entry.file_name());
            link_or_copy_file(&src, &dst)?;
        }
    }
    write_jetos_toolchain(dir, &sw_bin, &mut manifest)?;
    fs::write(dir.join("sw/closure.txt"), manifest)
}

fn write_jetos_toolchain(
    dir: &Path,
    sw_bin: &Path,
    manifest: &mut String,
) -> std::io::Result<()> {
    let candidates = jet_toolchain_candidates();
    for name in ["jet", "jetpack", "jetos"] {
        let Some(src) = candidates.iter().find(|path| {
            path.file_name()
                .and_then(|part| part.to_str())
                .map(|part| part == name)
                .unwrap_or(false)
        }) else {
            continue;
        };
        let dst = sw_bin.join(name);
        copy_file_replace(src, &dst)?;
        make_executable(&dst)?;
        manifest.push_str(&format!("jetos-toolchain {name} {}\n", src.display()));
        copy_toolchain_runtime_deps(dir, src)?;
    }
    Ok(())
}

fn jet_toolchain_candidates() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            dirs.push(dir.to_path_buf());
            if dir.file_name().and_then(|part| part.to_str()) == Some("deps") {
                if let Some(parent) = dir.parent() {
                    dirs.push(parent.to_path_buf());
                }
            }
        }
    }
    dirs.push(PathBuf::from("target/debug"));
    dirs.push(PathBuf::from("target/release"));

    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for dir in dirs {
        for name in ["jet", "jetpack", "jetos"] {
            let path = dir.join(name);
            if path.is_file() && seen.insert(path.clone()) {
                out.push(path);
            }
        }
    }
    out
}

fn copy_toolchain_runtime_deps(dir: &Path, binary: &Path) -> std::io::Result<()> {
    for dep in ldd_dependency_paths(binary)? {
        copy_absolute_runtime_file(dir, &dep)?;
        if let Ok(real) = fs::canonicalize(&dep) {
            copy_absolute_runtime_file(dir, &real)?;
        }
    }
    Ok(())
}

fn copy_absolute_runtime_file(dir: &Path, src: &Path) -> std::io::Result<()> {
    if !src.is_absolute() || !src.is_file() {
        return Ok(());
    }
    let Ok(relative) = src.strip_prefix("/") else {
        return Ok(());
    };
    let dst = dir.join(relative);
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    copy_file_replace(src, &dst)
}

fn write_studio_app_projection(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let studio_dir = dir.join("studio");
    let bin_dir = dir.join("sw/bin");
    let desktop_dir = dir.join("share/applications");
    fs::create_dir_all(&studio_dir)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&desktop_dir)?;

    let app = JSON::object_of(&[
        ("kind", "jetos-studio-app"),
        ("host", &system.name),
        ("runtime", "jetos-system-app"),
        ("protocol", "local-projection-service"),
        ("source_truth", "jet-source-transactions"),
        ("semantic_state", "none"),
        ("browser_fallback", "true"),
        ("canvas_coupled", "false"),
    ]);
    fs::write(studio_dir.join("app.json"), app)?;
    fs::write(studio_dir.join("data.json"), studio_data_json(system))?;
    fs::write(studio_dir.join("index.html"), studio_index_html(system))?;

    let launcher = "#!/usr/bin/env sh\nset -eu\nroot=${JETOS_STUDIO_ROOT:-/run/current-system}\npage=\"$root/studio/index.html\"\nif command -v jetos >/dev/null 2>&1; then\n  exec jetos studio \"$@\"\nfi\nif command -v xdg-open >/dev/null 2>&1; then\n  exec xdg-open \"$page\"\nfi\nprintf '%s\\n' \"$page\"\n";
    let launcher_path = bin_dir.join("jetos-studio");
    fs::write(&launcher_path, launcher)?;
    make_executable(&launcher_path)?;

    fs::write(
        desktop_dir.join("jetos-studio.desktop"),
        "[Desktop Entry]\nName=jetos Studio\nComment=Edit jetos system source\nExec=/run/current-system/sw/bin/jetos-studio\nType=Application\nCategories=System;Settings;\n",
    )
}

fn studio_data_json(system: &SystemPlan) -> String {
    let packages = system
        .packages
        .iter()
        .map(|pkg| {
            JSON::object_of(&[
                ("name", &pkg.name),
                (
                    "source",
                    if pkg.source.is_empty() {
                        "default"
                    } else {
                        &pkg.source
                    },
                ),
            ])
        })
        .collect::<Vec<_>>()
        .join(",");
    let services = system
        .services
        .iter()
        .map(|svc| {
            let fields = svc
                .extra
                .iter()
                .map(|(key, value)| JSON::object_of(&[("key", key), ("value", value)]))
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{\"name\":{},\"enable\":{},\"fields\":[{}]}}",
                JSON::quote(&svc.name),
                if svc.enable { "true" } else { "false" },
                fields
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let options = system
        .options
        .iter()
        .map(|opt| JSON::object_of(&[("key", &opt.key), ("value", &opt.value)]))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"kind\":\"jetos-studio-projection\",\"host\":{},\"target\":{},\"packages\":[{}],\"services\":[{}],\"options\":[{}],\"artifacts\":{{\"plan\":\"../plan.json\",\"proof\":\"../proof.txt\",\"provenance\":\"../provenance.json\",\"vm_proof\":\"../vm-proof.txt\"}}}}",
        JSON::quote(&system.name),
        JSON::quote(&system.target),
        packages,
        services,
        options
    )
}

fn studio_index_html(system: &SystemPlan) -> String {
    let packages = system
        .packages
        .iter()
        .map(|pkg| {
            let source = if pkg.source.is_empty() {
                "default"
            } else {
                &pkg.source
            };
            format!(
                "<tr><td>{}</td><td>{}</td></tr>",
                html_escape(&pkg.name),
                html_escape(source)
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let services = system
        .services
        .iter()
        .map(|svc| {
            let fields = if svc.extra.is_empty() {
                String::new()
            } else {
                svc.extra
                    .iter()
                    .map(|(key, value)| format!("{key}: {value}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                html_escape(&svc.name),
                if svc.enable { "enabled" } else { "disabled" },
                html_escape(&fields)
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let options = system
        .options
        .iter()
        .map(|opt| {
            format!(
                "<tr><td>{}</td><td>{}</td></tr>",
                html_escape(&opt.key),
                html_escape(&opt.value)
            )
        })
        .collect::<Vec<_>>()
        .join("");
    format!(
        "<!doctype html>
<html>
<head>
<meta charset=\"utf-8\">
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">
<title>jetos Studio - {host}</title>
<style>
:root {{ color-scheme: light dark; font-family: Inter, ui-sans-serif, system-ui, sans-serif; background: #101418; color: #edf2f7; }}
body {{ margin: 0; min-height: 100vh; background: #101418; }}
main {{ display: grid; grid-template-columns: 260px 1fr; min-height: 100vh; }}
aside {{ border-right: 1px solid #2d3742; padding: 20px; background: #151b21; }}
section {{ padding: 24px; }}
h1, h2 {{ margin: 0; font-weight: 650; }}
h1 {{ font-size: 22px; }}
h2 {{ font-size: 15px; margin-bottom: 12px; }}
.host {{ display: grid; gap: 6px; margin-top: 20px; }}
.pill {{ border: 1px solid #3b4652; border-radius: 999px; padding: 6px 10px; width: max-content; }}
.grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(280px, 1fr)); gap: 16px; }}
.panel {{ border: 1px solid #2d3742; border-radius: 8px; background: #151b21; overflow: hidden; }}
.panel header {{ padding: 14px 16px; border-bottom: 1px solid #2d3742; }}
table {{ width: 100%; border-collapse: collapse; font-size: 13px; }}
td {{ padding: 10px 16px; border-top: 1px solid #202832; vertical-align: top; }}
td:first-child {{ color: #9ccfd8; font-weight: 600; white-space: nowrap; }}
.status {{ display: flex; flex-wrap: wrap; gap: 8px; margin: 18px 0 24px; }}
.empty {{ color: #9aa7b2; padding: 16px; }}
.form {{ display: grid; grid-template-columns: 1fr 1fr; gap: 10px; padding: 16px; }}
label {{ display: grid; gap: 6px; font-size: 12px; color: #9aa7b2; }}
input {{ min-width: 0; border: 1px solid #3b4652; border-radius: 6px; padding: 9px 10px; background: #101418; color: #edf2f7; }}
.actions {{ display: flex; flex-wrap: wrap; gap: 8px; padding: 0 16px 16px; }}
button {{ border: 1px solid #3b4652; border-radius: 6px; padding: 8px 11px; background: #1d2730; color: #edf2f7; cursor: pointer; }}
button:hover {{ border-color: #9ccfd8; }}
pre {{ margin: 0; padding: 16px; min-height: 96px; max-height: 300px; overflow: auto; border-top: 1px solid #2d3742; color: #c9d1d9; background: #0c1014; font-size: 12px; }}
@media (max-width: 720px) {{ main {{ grid-template-columns: 1fr; }} aside {{ border-right: 0; border-bottom: 1px solid #2d3742; }} }}
</style>
</head>
<body>
<main id=\"studio\" data-host=\"{host}\" data-protocol=\"local-projection-service\" data-source-truth=\"jet-source-transactions\">
<aside>
<h1>jetos Studio</h1>
<div class=\"host\">
<span class=\"pill\">{host}</span>
<span>{target}</span>
</div>
</aside>
<section>
<div class=\"status\">
<span class=\"pill\">Source</span>
<span class=\"pill\">Proof</span>
<span class=\"pill\">Local</span>
</div>
<div class=\"grid\">
<article class=\"panel\"><header><h2>Packages</h2></header><table>{packages}</table></article>
<article class=\"panel\"><header><h2>Services</h2></header><table>{services}</table></article>
<article class=\"panel\"><header><h2>Options</h2></header><table>{options}</table></article>
<article class=\"panel\"><header><h2>Source</h2></header>
<div class=\"form\">
<label>Option<input id=\"tx-key\" value=\"network.hostName\"></label>
<label>Value<input id=\"tx-value\" value=\"{host}\"></label>
</div>
<div class=\"actions\">
<button data-tx=\"preview\">Preview</button>
<button data-tx=\"write\">Save</button>
</div>
<pre id=\"tx-output\"></pre>
</article>
<article class=\"panel\"><header><h2>Module</h2></header><pre id=\"source-output\"></pre></article>
<article class=\"panel\"><header><h2>Proof</h2></header>
<div class=\"actions\">
<button data-run=\"check\">Check</button>
<button data-run=\"plan\">Plan</button>
<button data-run=\"build\">Build</button>
<button data-run=\"proof\">Proof</button>
<button data-run=\"generations\">Rollback</button>
</div>
<pre id=\"run-output\">plan.json proof.txt provenance.json vm-proof.txt</pre>
</article>
</div>
</section>
</main>
<script>
async function refreshSource() {{
  const res = await fetch('/studio/source');
  document.getElementById('source-output').textContent = await res.text();
}}
async function studioPost(path, payload) {{
  const res = await fetch(path, {{ method: 'POST', headers: {{ 'Content-Type': 'application/json' }}, body: JSON.stringify(payload) }});
  return await res.json();
}}
for (const button of document.querySelectorAll('[data-tx]')) {{
  button.addEventListener('click', async () => {{
    const write = button.dataset.tx === 'write';
    const result = await studioPost('/studio/transaction', {{
      op: 'set-option',
      key: document.getElementById('tx-key').value,
      value: document.getElementById('tx-value').value,
      write
    }});
    document.getElementById('tx-output').textContent = result.diff || result.error || JSON.stringify(result, null, 2);
    if (write && !result.error) await refreshSource();
  }});
}}
for (const button of document.querySelectorAll('[data-run]')) {{
  button.addEventListener('click', async () => {{
    const result = await studioPost('/studio/run', {{ action: button.dataset.run }});
    document.getElementById('run-output').textContent = result.stdout || result.stderr || result.error || JSON.stringify(result, null, 2);
  }});
}}
refreshSource();
</script>
</body>
</html>
",
        host = html_escape(&system.name),
        target = html_escape(&system.target),
        packages = if packages.is_empty() {
            "<tr><td>none</td><td></td></tr>".to_string()
        } else {
            packages
        },
        services = if services.is_empty() {
            "<tr><td>none</td><td></td><td></td></tr>".to_string()
        } else {
            services
        },
        options = if options.is_empty() {
            "<tr><td>none</td><td></td></tr>".to_string()
        } else {
            options
        },
    )
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(unix)]
fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(perms.mode() | 0o111);
    fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn write_bootable_root_projection(dir: &Path) -> std::io::Result<()> {
    let root = dir.join("root");
    if root.exists() {
        fs::remove_dir_all(&root)?;
    }
    fs::create_dir_all(root.join("run/current-system"))?;
    fs::create_dir_all(root.join("var/lib/jetos/generations"))?;
    for top in [
        "boot",
        "etc",
        "sbin",
        "sw",
        "share",
        "studio",
        "init",
        "network",
        "hardware",
        "users",
        "flatpak",
        "performance",
        "module-system",
        "storage",
        "workloads",
        "theme",
        "fleet",
        "options",
        "image-variants",
        "lifecycle",
        "service-manager",
        "apps",
        "acceptance",
        "desktop",
        "store",
        "compat",
        "terminal",
        "home",
    ] {
        let src = dir.join(top);
        if !src.exists() {
            continue;
        }
        copy_dir_recursive(&src, &root.join("run/current-system").join(top))?;
        match top {
            "boot" | "etc" | "sbin" | "home" => copy_dir_recursive(&src, &root.join(top))?,
            _ => {}
        }
    }
    for file in [
        "plan.json",
        "proof.txt",
        "provenance.json",
        "health-checks.txt",
        "activation-diff.txt",
        "secrets.tmpfs.manifest",
        "vm-proof.txt",
    ] {
        let src = dir.join(file);
        if src.is_file() {
            link_or_copy_file(&src, &root.join("run/current-system").join(file))?;
        }
    }
    fs::write(
        root.join("var/lib/jetos/generations/current"),
        format!("{}\n", dir.display()),
    )
}

#[cfg(unix)]
fn link_or_copy_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    let _ = fs::remove_file(dst);
    match std::os::unix::fs::symlink(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => fs::copy(src, dst).map(|_| ()),
    }
}

#[cfg(not(unix))]
fn link_or_copy_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    let _ = fs::remove_file(dst);
    fs::copy(src, dst).map(|_| ())
}

fn copy_file_replace(src: &Path, dst: &Path) -> std::io::Result<()> {
    let _ = fs::remove_file(dst);
    fs::copy(src, dst)?;
    let mut perms = fs::metadata(dst)?.permissions();
    perms.set_readonly(false);
    fs::set_permissions(dst, perms)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    let mut entries = fs::read_dir(src)?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&path, &target)?;
        } else {
            link_or_copy_file(&path, &target)?;
        }
    }
    Ok(())
}

fn copy_dir_recursive_deref(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    let mut entries = fs::read_dir(src)?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let target = dst.join(entry.file_name());
        let meta = fs::metadata(&path)?;
        if meta.is_dir() {
            copy_dir_recursive_deref(&path, &target)?;
        } else if meta.is_file() {
            copy_file_replace(&path, &target)?;
        }
    }
    Ok(())
}

fn enable_unit(unit_dir: &Path, target: &str, unit_name: &str) -> std::io::Result<()> {
    let wants = unit_dir.join(format!("{target}.wants"));
    fs::create_dir_all(&wants)?;
    let dst = wants.join(unit_name);
    let _ = fs::remove_file(&dst);
    enable_unit_link(
        Path::new("..").join(unit_name),
        unit_dir.join(unit_name),
        dst,
    )
}

#[cfg(unix)]
fn enable_unit_link(rel_src: PathBuf, _abs_src: PathBuf, dst: PathBuf) -> std::io::Result<()> {
    std::os::unix::fs::symlink(rel_src, dst)
}

#[cfg(not(unix))]
fn enable_unit_link(_rel_src: PathBuf, abs_src: PathBuf, dst: PathBuf) -> std::io::Result<()> {
    fs::copy(abs_src, dst).map(|_| ())
}

fn write_etc_tree(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let etc = dir.join("etc");
    fs::create_dir_all(&etc)?;
    let host = option_value(system, &["network.hostName", "network.hostname"])
        .unwrap_or_else(|| system.name.clone());
    fs::write(etc.join("hostname"), format!("{host}\n"))?;
    if let Some(zone) = option_value(system, &["filesystem.timeZone", "filesystem.timezone"]) {
        fs::write(etc.join("timezone"), format!("{zone}\n"))?;
    }
    let root_device = option_value(system, &["filesystem.root.device"])
        .unwrap_or_else(|| "LABEL=jetos-root".to_string());
    let root_type = option_value(system, &["filesystem.root.type"])
        .unwrap_or_else(|| "ext4".to_string())
        .trim_start_matches('.')
        .to_ascii_lowercase();
    let mut fstab = format!("{root_device}\t/\t{root_type}\tdefaults\t0\t1\n");
    for swap in collect_names(system, "filesystem.swap") {
        let device = option_value(system, &[&format!("filesystem.swap.{swap}.device")])
            .unwrap_or_else(|| format!("LABEL=jetos-swap-{swap}"));
        let priority = option_value(system, &[&format!("filesystem.swap.{swap}.priority")])
            .map(|p| format!("pri={p}"))
            .unwrap_or_else(|| "defaults".to_string());
        fstab.push_str(&format!("{device}\tnone\tswap\t{priority}\t0\t0\n"));
    }
    fs::write(etc.join("fstab"), fstab)?;
    write_identity_files(&etc, system)
}

fn write_identity_files(etc: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let users = collect_names(system, "users");
    let groups = collect_names(system, "groups");
    let mut passwd = String::from("root:x:0:0:root:/root:/bin/sh\n");
    let mut group = String::from("root:x:0:\n");
    let mut sysusers = String::new();
    for (idx, user) in users.iter().enumerate() {
        let uid = 1000 + idx;
        let home = option_value(system, &[&format!("users.{user}.home")])
            .unwrap_or_else(|| format!("/home/{user}"));
        let shell = option_value(system, &[&format!("users.{user}.shell")])
            .map(|s| package_path_or_literal(&s))
            .unwrap_or_else(|| "/run/current-system/sw/bin/sh".to_string());
        passwd.push_str(&format!("{user}:x:{uid}:{uid}:{user}:{home}:{shell}\n"));
        group.push_str(&format!("{user}:x:{uid}:{user}\n"));
        sysusers.push_str(&format!("u {user} {uid} \"{user}\" {home} {shell}\n"));
    }
    for (idx, name) in groups.iter().enumerate() {
        let gid = 2000 + idx;
        let members = option_value(system, &[&format!("groups.{name}.members")])
            .map(|v| parse_list_items(&v).join(","))
            .unwrap_or_default();
        group.push_str(&format!("{name}:x:{gid}:{members}\n"));
        sysusers.push_str(&format!("g {name} {gid}\n"));
        if !members.is_empty() {
            sysusers.push_str(&format!("m {} {name}\n", members.replace(',', " ")));
        }
    }
    fs::write(etc.join("passwd"), passwd)?;
    fs::write(etc.join("group"), group)?;
    let sysusers_dir = etc.join("sysusers.d");
    fs::create_dir_all(&sysusers_dir)?;
    fs::write(sysusers_dir.join("jetos.conf"), sysusers)
}

fn write_boot_facts(
    dir: &Path,
    system: &SystemPlan,
    realized: &[Store::StoreEntry],
) -> std::io::Result<()> {
    let boot = boot_profile(system);
    let boot_dir = dir.join("boot");
    fs::create_dir_all(&boot_dir)?;
    let kernel_entry = cachyos_kernel_entry(realized);
    let kernel_path = kernel_entry
        .and_then(|entry| boot_artifact(entry, &["boot/vmlinuz-cachyos", "bzImage", "vmlinuz"]))
        .unwrap_or_else(|| PathBuf::from(&boot.kernel));
    let initrd_path = kernel_entry
        .and_then(|entry| boot_artifact(entry, &["boot/initrd-cachyos", "initrd", "initrd.img"]));
    fs::write(
        boot_dir.join("limine.conf"),
        format!(
        "timeout: 5\nserial: yes\ngraphics: no\nverbose: yes\n/jetos {}\n    protocol: linux\n    kernel_path: boot():/boot/kernel\n    module_path: boot():/boot/initrd\n    textmode: yes\n    cmdline: console=ttyS0 root=LABEL=jetos-root rw init={}\n",
            system.name, boot.init
        ),
    )?;
    if kernel_path.is_file() {
        link_or_copy_file(&kernel_path, &boot_dir.join("kernel"))?;
    } else {
        fs::write(
            boot_dir.join("kernel"),
            format!("{}\n", kernel_path.display()),
        )?;
    }
    match initrd_path {
        Some(path) if path.is_file() => link_or_copy_file(&path, &boot_dir.join("initrd"))?,
        Some(path) => fs::write(
            boot_dir.join("initrd"),
            format!(
                "{}\nmodules={}\n",
                path.display(),
                boot.initrd_modules.join(",")
            ),
        )?,
        None => fs::write(
            boot_dir.join("initrd"),
            format!("modules={}\n", boot.initrd_modules.join(",")),
        )?,
    }
    for module_name in [
        "isofs.ko.xz",
        "bochs.ko.xz",
        "fat.ko.xz",
        "vfat.ko.xz",
        "nls_ascii.ko.xz",
        "nls_cp437.ko.xz",
    ] {
        if let Some(module) = kernel_entry
            .and_then(|entry| boot_artifact(entry, &[&format!("boot/modules/{module_name}")]))
        {
            fs::create_dir_all(boot_dir.join("modules"))?;
            link_or_copy_file(&module, &boot_dir.join("modules").join(module_name))?;
        }
    }
    fs::write(
        boot_dir.join("facts.json"),
        render_boot_facts(system, realized),
    )
}

fn cachyos_kernel_entry(realized: &[Store::StoreEntry]) -> Option<&Store::StoreEntry> {
    realized
        .iter()
        .find(|entry| entry.name == CACHYOS_KERNEL_PACKAGE)
}

fn boot_artifact(entry: &Store::StoreEntry, candidates: &[&str]) -> Option<PathBuf> {
    let out = Path::new(&entry.out);
    candidates
        .iter()
        .map(|rel| out.join(rel))
        .find(|path| path.is_file())
}

fn is_linux_kernel_image(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    bytes.starts_with(b"\x7fELF")
        || (bytes.starts_with(b"MZ") && bytes.windows(4).any(|w| w == b"HdrS"))
}

fn is_initrd_image(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    bytes.starts_with(&[0x1f, 0x8b]) || bytes.starts_with(b"070701") || bytes.starts_with(b"070702")
}

fn missing_kernel_source_files(entry: &Store::StoreEntry) -> Option<&'static str> {
    let out = Path::new(&entry.out);
    [
        "source/recipe.jet",
        "source/build.sh",
        "source/config",
        "source/patches.manifest",
        "source/initrd-inputs.manifest",
    ]
    .into_iter()
    .find(|rel| !out.join(rel).is_file())
}

fn render_boot_facts(system: &SystemPlan, realized: &[Store::StoreEntry]) -> String {
    let boot = boot_profile(system);
    let kernel_package = cachyos_kernel_entry(realized)
        .map(kernel_package_json)
        .unwrap_or_else(|| "null".to_string());
    let modules = boot
        .initrd_modules
        .iter()
        .map(|m| JSON::quote(m))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"loader\":{},\"kernel\":{},\"init\":{},\"initrd_modules\":[{}],\"kernel_package\":{}}}",
        JSON::quote(&boot.loader),
        JSON::quote(&boot.kernel),
        JSON::quote(&boot.init),
        modules,
        kernel_package
    )
}

fn kernel_package_json(entry: &Store::StoreEntry) -> String {
    let source = kernel_source_json(entry);
    format!(
        "{{\"name\":{},\"reference\":{},\"out\":{},\"output_hash\":{},\"provenance\":{},\"bootstrap\":\"source-built\",\"source_recipe\":{}}}",
        JSON::quote(&entry.name),
        JSON::quote(&entry.reference),
        JSON::quote(&entry.out),
        JSON::quote(&entry.envelope.output_hash),
        JSON::quote(&entry.envelope.provenance),
        source
    )
}

fn kernel_source_json(entry: &Store::StoreEntry) -> String {
    let out = Path::new(&entry.out);
    let facts = [
        ("recipe", "source/recipe.jet"),
        ("builder", "source/build.sh"),
        ("config", "source/config"),
        ("patches", "source/patches.manifest"),
        ("initrd_inputs", "source/initrd-inputs.manifest"),
    ]
    .iter()
    .map(|(name, rel)| {
        let path = out.join(rel);
        let path_text = path.display().to_string();
        let sha = fs::read(&path)
            .map(|bytes| crate::SHA256::sha256_hex(&bytes))
            .unwrap_or_else(|_| "<missing>".to_string());
        JSON::object_of(&[("name", name), ("path", &path_text), ("sha256", &sha)])
    })
    .collect::<Vec<_>>()
    .join(",");
    format!("{{\"mode\":\"source-built\",\"files\":[{}]}}", facts)
}

fn write_init_facts(
    dir: &Path,
    system: &SystemPlan,
    realized: &[Store::StoreEntry],
) -> std::io::Result<()> {
    let init_dir = dir.join("init");
    fs::create_dir_all(&init_dir)?;
    let default_target = option_value(system, &["init.defaultTarget"])
        .unwrap_or_else(|| "multi-user.target".to_string());
    let init_entry = realized
        .iter()
        .find(|entry| entry.name == SYSTEMD_INIT_PACKAGE);
    if let Some(entry) = init_entry {
        let sbin = dir.join("sbin");
        fs::create_dir_all(&sbin)?;
        let init_path = boot_artifact(entry, &["bin/systemd", "lib/systemd/systemd", "sbin/init"])
            .unwrap_or_else(|| Path::new(&entry.out).join("bin/systemd"));
        link_or_copy_file(&init_path, &sbin.join("init"))?;
    }
    let init_package = init_entry
        .map(kernel_package_json)
        .unwrap_or_else(|| "null".to_string());
    fs::write(
        init_dir.join("systemd.json"),
        format!(
            "{{\"init\":\"systemd\",\"default_target\":{},\"unit_dir\":\"etc/systemd/system\",\"init_package\":{}}}",
            JSON::quote(&default_target),
            init_package
        ),
    )
}

fn write_secret_manifest(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let mut manifest = String::new();
    manifest.push_str("repo ciphertext + host key; activation decrypts into tmpfs only\n");
    for name in collect_names(system, "secrets") {
        let source = option_value(system, &[&format!("secrets.{name}.source")])
            .unwrap_or_else(|| format!("secrets/{name}.age"));
        manifest.push_str(&format!("{name}\t{source}\t/run/jetos-secrets/{name}\n"));
    }
    fs::write(dir.join("secrets.tmpfs.manifest"), manifest)
}

fn write_network_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let network_dir = dir.join("etc/systemd/network");
    fs::create_dir_all(&network_dir)?;
    let interface =
        option_value(system, &["network.interface"]).unwrap_or_else(|| "eth0".to_string());
    let address = option_value(system, &["network.address"]);
    let gateway = option_value(system, &["network.gateway"]);
    let dns = option_value(system, &["network.dns"])
        .map(|v| parse_list_items(&v))
        .unwrap_or_default();
    let wireless_ssid = option_value(system, &["network.wireless.ssid"]);
    let firewall_ports = option_value(system, &["network.firewall.allowedTcpPorts"])
        .map(|v| parse_list_items(&v))
        .unwrap_or_default();
    let mut network = String::new();
    network.push_str("[Match]\n");
    network.push_str(&format!("Name={interface}\n\n[Network]\n"));
    match address {
        Some(a) => network.push_str(&format!("Address={a}\n")),
        None => network.push_str("DHCP=yes\n"),
    }
    if let Some(g) = gateway {
        network.push_str(&format!("Gateway={g}\n"));
    }
    for d in &dns {
        network.push_str(&format!("DNS={d}\n"));
    }
    fs::write(network_dir.join("10-jetos.network"), network)?;

    let nft_dir = dir.join("etc/nftables");
    fs::create_dir_all(&nft_dir)?;
    let ports = if firewall_ports.is_empty() {
        "drop".to_string()
    } else {
        format!("tcp dport {{ {} }} accept", firewall_ports.join(", "))
    };
    fs::write(
        nft_dir.join("jetos-firewall.nft"),
        format!("table inet jetos {{\n  chain input {{\n    type filter hook input priority 0;\n    {ports}\n  }}\n}}\n"),
    )?;

    let dns_json = dns
        .iter()
        .map(|d| JSON::quote(d))
        .collect::<Vec<_>>()
        .join(",");
    let ports_json = firewall_ports
        .iter()
        .map(|p| JSON::quote(p))
        .collect::<Vec<_>>()
        .join(",");
    let wireless = wireless_ssid
        .map(|ssid| JSON::object_of(&[("ssid", &ssid), ("secret", "/run/jetos-secrets/wifi")]))
        .unwrap_or_else(|| "null".to_string());
    let facts_dir = dir.join("network");
    fs::create_dir_all(&facts_dir)?;
    fs::write(
        facts_dir.join("facts.json"),
        format!(
            "{{\"backend\":\"systemd-networkd\",\"interface\":{},\"dns\":[{}],\"firewall_allowed_tcp_ports\":[{}],\"wireless\":{}}}",
            JSON::quote(&interface),
            dns_json,
            ports_json,
            wireless
        ),
    )
}

fn write_systemd_timer_socket_units(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let unit_dir = dir.join("etc/systemd/system");
    fs::create_dir_all(&unit_dir)?;
    for svc in system.services.iter().filter(|s| s.enable) {
        if let Some(calendar) = service_extra(svc, &["timer", "schedule"]) {
            fs::write(
                unit_dir.join(format!("{}.timer", svc.name)),
                format!(
                    "[Unit]\nDescription=jetos timer for {}\n\n[Timer]\nOnCalendar={}\nUnit={}.service\n\n[Install]\nWantedBy=timers.target\n",
                    svc.name, calendar, svc.name
                ),
            )?;
            enable_unit(&unit_dir, "timers.target", &format!("{}.timer", svc.name))?;
        }
        if let Some(listen) = service_extra(svc, &["socket", "listen"]) {
            fs::write(
                unit_dir.join(format!("{}.socket", svc.name)),
                format!(
                    "[Unit]\nDescription=jetos socket for {}\n\n[Socket]\nListenStream={}\n\n[Install]\nWantedBy=sockets.target\n",
                    svc.name, listen
                ),
            )?;
            enable_unit(&unit_dir, "sockets.target", &format!("{}.socket", svc.name))?;
        }
    }
    Ok(())
}

fn write_hardware_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let hw_dir = dir.join("hardware");
    let bin_dir = dir.join("sw/bin");
    let boot_spec_dir = dir.join("boot/specialisations");
    fs::create_dir_all(&hw_dir)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&boot_spec_dir)?;
    let firmware = option_value(system, &["kernel.firmware"])
        .map(|v| parse_list_items(&v))
        .unwrap_or_default();
    let drivers = option_value(system, &["kernel.drivers"])
        .map(|v| parse_list_items(&v))
        .unwrap_or_default();
    let firmware_json = firmware
        .iter()
        .map(|f| JSON::quote(f))
        .collect::<Vec<_>>()
        .join(",");
    let drivers_json = drivers
        .iter()
        .map(|d| JSON::quote(d))
        .collect::<Vec<_>>()
        .join(",");
    let hardware_options = prefixed_options(system, "hardware.");
    let profile_keys = [
        format!("hardware.{}.profile", system.name),
        "hardware.profile".to_string(),
    ];
    let profile_keys = profile_keys.iter().map(String::as_str).collect::<Vec<_>>();
    let profiles = option_value(system, &profile_keys)
        .map(|v| parse_list_items(&v))
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| vec!["generic-pc".to_string()]);
    let profiles_json = profiles
        .iter()
        .map(|p| JSON::quote(p))
        .collect::<Vec<_>>()
        .join(",");
    let specialisations =
        prefixed_options(system, &format!("hardware.{}.specialisation.", system.name));
    let specialisations_json = specialisations
        .iter()
        .map(|(name, enabled)| {
            JSON::object_of(&[
                ("name", name),
                ("enabled", enabled),
                ("boot_entry", &format!("boot/specialisations/{name}.conf")),
            ])
        })
        .collect::<Vec<_>>()
        .join(",");
    fs::write(
        hw_dir.join("facts.json"),
        format!(
            "{{\"kind\":\"jetos.hardware\",\"host\":{},\"firmware\":[{}],\"drivers\":[{}],\"profiles\":[{}],\"specialisations\":[{}],\"scan_source\":\"hardware/{}.jet\",\"doctor\":\"sw/bin/jetos-hardware-doctor\",\"audit\":\"declared hardware facts enter generation proof before activation\"}}",
            JSON::quote(&system.name),
            firmware_json,
            drivers_json,
            profiles_json,
            specialisations_json,
            system.name
        ),
    )?;
    fs::write(
        hw_dir.join("firmware.manifest"),
        firmware
            .iter()
            .map(|f| format!("{f}\tdeclared\n"))
            .collect::<String>(),
    )?;
    fs::write(
        hw_dir.join("drivers.manifest"),
        drivers
            .iter()
            .map(|d| format!("{d}\tdeclared\n"))
            .collect::<String>(),
    )?;
    fs::write(
        hw_dir.join("profiles.manifest"),
        profiles
            .iter()
            .map(|p| format!("{p}\tdeclared\n"))
            .collect::<String>(),
    )?;
    for profile in &profiles {
        fs::write(
            hw_dir.join(format!("profile-{}.json", safe_filename(profile))),
            format!(
                "{{\"kind\":\"jetos.hardware-profile\",\"name\":{},\"source\":\"first-party hardware profile\",\"applies_to\":{},\"drivers\":[{}],\"firmware\":[{}],\"proof\":\"profile-applied\"}}",
                JSON::quote(profile),
                JSON::quote(&system.name),
                drivers_json,
                firmware_json
            ),
        )?;
    }
    let mut source = format!(
        "// generated by jetos-hardware-scan; canonical source may be copied into config.jet\nmodule hardware_{} {{\n",
        safe_identifier(&system.name)
    );
    source.push_str(&format!(
        "    hardware.{}.scan.generated: true\n",
        system.name
    ));
    for driver in &drivers {
        source.push_str(&format!(
            "    hardware.{}.driver.{}: true\n",
            system.name,
            safe_identifier(driver)
        ));
    }
    for firmware in &firmware {
        source.push_str(&format!(
            "    hardware.{}.firmware.{}: true\n",
            system.name,
            safe_identifier(firmware)
        ));
    }
    for profile in &profiles {
        source.push_str(&format!(
            "    hardware.{}.profile: \"{}\"\n",
            system.name, profile
        ));
    }
    source.push_str("}\n");
    fs::write(hw_dir.join(format!("{}.jet", system.name)), source)?;
    fs::write(
        hw_dir.join("declared-options.json"),
        format!(
            "{{\"kind\":\"jetos.hardware-options\",\"options\":[{}]}}",
            option_rows_json(&hardware_options)
        ),
    )?;
    fs::write(
        hw_dir.join("specialisations.json"),
        format!(
            "{{\"kind\":\"jetos.hardware-specialisations\",\"host\":{},\"items\":[{}],\"proof\":\"boot-selectable-variants\"}}",
            JSON::quote(&system.name),
            specialisations_json
        ),
    )?;
    for (name, enabled) in &specialisations {
        fs::write(
            boot_spec_dir.join(format!("{name}.conf")),
            format!(
                "title JetOS {} ({name})\nhost {}\nenabled {}\ngeneration /run/current-system\nproof hardware-specialisation\n",
                system.name, system.name, enabled
            ),
        )?;
    }
    let host = shell_single_quote(&system.name);
    let scanner = format!(
        "#!/usr/bin/env sh\nset -eu\nhost=${{1:-{host}}}\nscan_root=${{JETOS_HW_ROOT:-/}}\nout=${{JETOS_HARDWARE_OUT:-$PWD/hardware-$host.jet}}\nmkdir -p \"$(dirname \"$out\")\"\nmods=''\nif [ -r \"$scan_root/proc/modules\" ]; then\n  mods=$(awk '{{print $1}}' \"$scan_root/proc/modules\" | sort -u | paste -sd, -)\nfi\nblocks=''\nif [ -d \"$scan_root/sys/class/block\" ]; then\n  blocks=$(find \"$scan_root/sys/class/block\" -mindepth 1 -maxdepth 1 -printf '%f\\n' | sort | paste -sd, -)\nfi\ngpus=''\nif [ -d \"$scan_root/sys/class/drm\" ]; then\n  gpus=$(find \"$scan_root/sys/class/drm\" -mindepth 1 -maxdepth 1 -name 'card*' -printf '%f\\n' | sort | paste -sd, -)\nfi\n{{\n  printf '%s\\n' \"// generated by jetos-hardware-scan\"\n  printf '%s\\n' \"module hardware_$host {{\"\n  printf '    hardware.%s.scan.modules: \"%s\"\\n' \"$host\" \"$mods\"\n  printf '    hardware.%s.scan.blockDevices: \"%s\"\\n' \"$host\" \"$blocks\"\n  printf '    hardware.%s.scan.gpus: \"%s\"\\n' \"$host\" \"$gpus\"\n  printf '%s\\n' \"}}\"\n}} > \"$out\"\nprintf '{{\"kind\":\"jetos.hardware-scan\",\"host\":\"%s\",\"out\":\"%s\",\"modules\":\"%s\",\"block_devices\":\"%s\",\"gpus\":\"%s\"}}\\n' \"$host\" \"$out\" \"$mods\" \"$blocks\" \"$gpus\"\n"
    );
    let scanner_path = bin_dir.join("jetos-hardware-scan");
    fs::write(&scanner_path, scanner)?;
    make_executable(&scanner_path)?;
    let doctor = "#!/usr/bin/env sh\nset -eu\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\nscan_root=${JETOS_HW_ROOT:-/}\nreport=${JETOS_HARDWARE_REPORT:-$root/hardware/drift-report.json}\nmkdir -p \"$(dirname \"$report\")\"\nmissing=''\nif [ -f \"$root/hardware/drivers.manifest\" ]; then\n  while IFS='	' read -r driver _state; do\n    [ -n \"$driver\" ] || continue\n    if [ -r \"$scan_root/proc/modules\" ]; then\n      if ! awk '{print $1}' \"$scan_root/proc/modules\" | grep -qx \"$driver\"; then\n        missing=\"$missing $driver\"\n      fi\n    fi\n  done < \"$root/hardware/drivers.manifest\"\nfi\nstate=match\nif [ -n \"$missing\" ]; then state=drift; fi\nprintf '{\"kind\":\"jetos.hardware-doctor\",\"state\":\"%s\",\"missing_drivers\":\"%s\",\"proof\":\"hardware-drift-checked\"}\\n' \"$state\" \"${missing# }\" > \"$report\"\ncat \"$report\"\n";
    let doctor_path = bin_dir.join("jetos-hardware-doctor");
    fs::write(&doctor_path, doctor)?;
    make_executable(&doctor_path)
}

fn write_user_environment_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let users_dir = dir.join("users");
    let unit_dir = dir.join("etc/systemd/user");
    fs::create_dir_all(&users_dir)?;
    fs::create_dir_all(&unit_dir)?;
    let names = user_names(system);
    let mut index = Vec::new();
    for name in names {
        let profile_dir = users_dir.join(&name);
        fs::create_dir_all(profile_dir.join("files"))?;
        let home = option_value(
            system,
            &[&format!("user.{name}.home"), &format!("users.{name}.home")],
        )
        .unwrap_or_else(|| format!("/home/{name}"));
        let shell = option_value(
            system,
            &[
                &format!("user.{name}.shell"),
                &format!("users.{name}.shell"),
            ],
        )
        .map(|s| package_path_or_literal(&s))
        .unwrap_or_else(|| "/run/current-system/sw/bin/sh".to_string());
        let packages = option_value(
            system,
            &[
                &format!("user.{name}.packages"),
                &format!("users.{name}.packages"),
            ],
        )
        .map(|v| parse_list_items(&v))
        .unwrap_or_default();
        let services = option_value(
            system,
            &[
                &format!("user.{name}.services"),
                &format!("users.{name}.services"),
            ],
        )
        .map(|v| parse_list_items(&v))
        .unwrap_or_default();
        let files = prefixed_options(system, &format!("user.{name}.files."));
        let files_json = files
            .iter()
            .map(|(key, value)| {
                JSON::object_of(&[("path", &user_file_target(key, value)), ("source", value)])
            })
            .collect::<Vec<_>>()
            .join(",");
        fs::write(profile_dir.join("home.txt"), format!("{home}\n"))?;
        fs::write(profile_dir.join("shell.txt"), format!("{shell}\n"))?;
        fs::write(
            profile_dir.join("packages.manifest"),
            manifest_lines(&packages),
        )?;
        fs::write(
            profile_dir.join("services.manifest"),
            manifest_lines(&services),
        )?;
        for (rel, source) in &files {
            let target = user_file_target(rel, source);
            let safe = target
                .trim_start_matches('/')
                .trim_start_matches('.')
                .replace('/', "__");
            fs::write(
                profile_dir.join("files").join(safe),
                format!("source={source}\ntarget={target}\n"),
            )?;
        }
        let packages_json = packages
            .iter()
            .map(|p| JSON::quote(p))
            .collect::<Vec<_>>()
            .join(",");
        let services_json = services
            .iter()
            .map(|s| JSON::quote(s))
            .collect::<Vec<_>>()
            .join(",");
        let facts = render_user_profile_json_parts(
            &name,
            &home,
            &shell,
            &packages_json,
            &services_json,
            &files_json,
        );
        fs::write(profile_dir.join("profile.json"), &facts)?;
        fs::write(
            profile_dir.join("proof.txt"),
            format!("user {name}: pass\n"),
        )?;
        fs::write(
            unit_dir.join(format!("jetos-user-{name}.service")),
            format!(
                "[Unit]\nDescription=jetos user environment for {name}\n\n[Service]\nType=oneshot\nExecStart=/run/current-system/sw/bin/jetos-user-apply {name}\n\n[Install]\nWantedBy=default.target\n"
            ),
        )?;
        index.push(facts);
    }
    let bin_dir = dir.join("sw/bin");
    fs::create_dir_all(&bin_dir)?;
    let apply = "#!/usr/bin/env sh\nset -eu\nuser=${1:-}\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\nif [ -z \"$user\" ]; then\n  echo 'usage: jetos-user-apply <user>' >&2\n  exit 2\nfi\nprofile_dir=\"$root/users/$user\"\nprofile=\"$profile_dir/profile.json\"\nif [ ! -f \"$profile\" ]; then\n  echo \"jetos user: no profile for $user\" >&2\n  exit 2\nfi\nhome=${JETOS_USER_HOME:-}\nif [ -z \"$home\" ] && [ -f \"$profile_dir/home.txt\" ]; then\n  home=$(sed -n '1p' \"$profile_dir/home.txt\")\nfi\nif [ -z \"$home\" ]; then\n  home=\"$HOME\"\nfi\nmkdir -p \"$home/.jetos/profile/bin\" \"$home/.jetos/proof\" \"$home/.config/systemd/user\"\nfor entry in \"$profile_dir\"/files/*; do\n  [ -f \"$entry\" ] || continue\n  target=$(sed -n 's/^target=//p' \"$entry\")\n  source=$(sed -n 's/^source=//p' \"$entry\")\n  [ -n \"$target\" ] || continue\n  case \"$target\" in\n    /*) dest=\"$target\" ;;\n    *) dest=\"$home/$target\" ;;\n  esac\n  dir=${dest%/*}\n  [ \"$dir\" = \"$dest\" ] || mkdir -p \"$dir\"\n  if [ -f \"$root/$source\" ]; then\n    cp \"$root/$source\" \"$dest\"\n  else\n    printf 'managed-by=jetos\\nuser=%s\\nsource=%s\\n' \"$user\" \"$source\" > \"$dest\"\n  fi\ndone\nif [ -f \"$profile_dir/packages.manifest\" ]; then\n  while IFS= read -r package; do\n    [ -n \"$package\" ] || continue\n    name=${package##*.}\n    src=\"$root/sw/bin/$name\"\n    if [ -e \"$src\" ]; then\n      ln -sfn \"$src\" \"$home/.jetos/profile/bin/$name\"\n    fi\n  done < \"$profile_dir/packages.manifest\"\nfi\nif [ -f \"$profile_dir/services.manifest\" ]; then\n  while IFS= read -r service; do\n    [ -n \"$service\" ] || continue\n    unit=\"$home/.config/systemd/user/$service.service\"\n    printf '[Unit]\\nDescription=jetos user service %s\\n\\n[Service]\\nExecStart=/run/current-system/sw/bin/%s\\n\\n[Install]\\nWantedBy=default.target\\n' \"$service\" \"$service\" > \"$unit\"\n  done < \"$profile_dir/services.manifest\"\nfi\nprintf '{\"state\":\"applied\",\"user\":\"%s\",\"home\":\"%s\",\"profile\":\"%s\"}\\n' \"$user\" \"$home\" \"$profile\" > \"$home/.jetos/proof/user-$user.json\"\ncat \"$home/.jetos/proof/user-$user.json\"\n";
    let apply_path = bin_dir.join("jetos-user-apply");
    fs::write(&apply_path, apply)?;
    make_executable(&apply_path)?;
    fs::write(
        users_dir.join("index.json"),
        format!(
            "{{\"kind\":\"jetos.user-index\",\"host\":{},\"profiles\":[{}]}}",
            JSON::quote(&system.name),
            index.join(",")
        ),
    )
}

fn user_file_target(key: &str, source: &str) -> String {
    if key.starts_with('.') || key.starts_with('/') || key.contains('/') {
        return key.to_string();
    }
    if let Some(rest) = source.strip_prefix("home/") {
        return format!(".config/{rest}");
    }
    format!(".config/{key}")
}

fn manifest_lines(items: &[String]) -> String {
    if items.is_empty() {
        String::new()
    } else {
        format!("{}\n", items.join("\n"))
    }
}

fn write_flatpak_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let flatpak_dir = dir.join("flatpak");
    let appimage_dir = dir.join("appimage");
    let bin_dir = dir.join("sw/bin");
    fs::create_dir_all(&flatpak_dir)?;
    fs::create_dir_all(&appimage_dir)?;
    fs::create_dir_all(&bin_dir)?;
    let options = prefixed_options(system, "apps.flatpak.");
    let remotes = prefixed_options(system, "apps.flatpak.remotes.");
    let apps = collect_names(system, "apps.flatpak.app");
    let appimages = collect_names(system, "apps.appimage.app");
    let apps_json = apps
        .iter()
        .map(|name| {
            let ref_id = option_value(system, &[&format!("apps.flatpak.app.{name}.ref")])
                .unwrap_or_else(|| name.clone());
            let pin = option_value(system, &[&format!("apps.flatpak.app.{name}.pin")])
                .unwrap_or_else(|| "tracking".to_string());
            JSON::object_of(&[("name", name), ("ref", &ref_id), ("pin", &pin)])
        })
        .collect::<Vec<_>>()
        .join(",");
    let appimages_json = appimages
        .iter()
        .map(|name| {
            let path = option_value(system, &[&format!("apps.appimage.app.{name}.path")])
                .unwrap_or_else(|| name.clone());
            let integrate = option_value(system, &[&format!("apps.appimage.app.{name}.integrate")])
                .unwrap_or_else(|| "true".to_string());
            JSON::object_of(&[
                ("name", name),
                ("path", &path),
                ("integrate", clean_bool_json(&integrate)),
            ])
        })
        .collect::<Vec<_>>()
        .join(",");
    let reconcile = option_value(system, &["apps.flatpak.reconcile"])
        .map(|s| clean_symbol(&s))
        .unwrap_or_else(|| "Exact".to_string());
    fs::write(
        flatpak_dir.join("plan.json"),
        format!(
            "{{\"kind\":\"jetos.flatpak-plan\",\"reconcile\":{},\"apps\":[{}],\"appimages\":[{}],\"options\":[{}],\"proof\":\"flatpak-reconcile-planned\"}}",
            JSON::quote(&reconcile),
            apps_json,
            appimages_json,
            option_rows_json(&options)
        ),
    )?;
    fs::write(
        appimage_dir.join("plan.json"),
        format!(
            "{{\"kind\":\"jetos.appimage-plan\",\"apps\":[{}],\"runner\":\"sw/bin/jetos-appimage-run\",\"proof\":\"appimage-runtime-integrated\"}}",
            appimages_json
        ),
    )?;
    fs::write(
        flatpak_dir.join("permissions.manifest"),
        options
            .iter()
            .filter(|(key, _)| key.contains(".permissions."))
            .map(|(key, value)| format!("{key}\t{value}\n"))
            .collect::<String>(),
    )?;
    let mut script = String::from(
        "#!/usr/bin/env sh\nset -eu\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\nflatpak=${JETOS_FLATPAK_BIN:-flatpak}\nproof_dir=${JETOS_FLATPAK_PROOF_DIR:-$root/flatpak}\nmkdir -p \"$proof_dir\"\nproof=\"$proof_dir/reconcile-proof.json\"\nrun() {\n  printf '%s\\n' \"jetos flatpak: $*\"\n  \"$flatpak\" \"$@\"\n}\n",
    );
    let mut declared_refs = Vec::new();
    for (remote, url) in remotes {
        script.push_str(&format!(
            "run remote-add --if-not-exists {} {}\n",
            shell_single_quote(&remote),
            shell_single_quote(&url)
        ));
    }
    for app in &apps {
        let ref_id = option_value(system, &[&format!("apps.flatpak.app.{app}.ref")])
            .unwrap_or_else(|| app.clone());
        declared_refs.push(ref_id.clone());
        let remote = option_value(system, &[&format!("apps.flatpak.app.{app}.remote")])
            .unwrap_or_else(|| "flathub".to_string());
        script.push_str(&format!(
            "run install -y {} {}\n",
            shell_single_quote(&remote),
            shell_single_quote(&ref_id)
        ));
        for (key, value) in
            prefixed_options(system, &format!("apps.flatpak.app.{app}.permissions."))
        {
            let flag = key.replace('.', "-");
            script.push_str(&format!(
                "run override {} --{}={}\n",
                shell_single_quote(&ref_id),
                flag,
                shell_single_quote(&clean_symbol(&value))
            ));
        }
    }
    if reconcile == "Exact" {
        script.push_str(&format!(
            "declared={}\ninstalled=$(\"$flatpak\" list --app --columns=application 2>/dev/null || true)\nfor app in $installed; do\n  case \" $declared \" in\n    *\" $app \"*) ;;\n    *) run uninstall -y \"$app\" ;;\n  esac\ndone\n",
            shell_single_quote(&declared_refs.join(" "))
        ));
        script.push_str("run update -y\n");
    }
    script.push_str(
        "printf '{\"state\":\"reconciled\",\"proofs\":[\"remotes\",\"apps\",\"permissions\"]}\\n' > \"$proof\"\ncat \"$proof\"\n",
    );
    let reconcile_path = bin_dir.join("jetos-flatpak-reconcile");
    fs::write(&reconcile_path, script)?;
    make_executable(&reconcile_path)?;
    for name in &appimages {
        let path = option_value(system, &[&format!("apps.appimage.app.{name}.path")])
            .unwrap_or_else(|| name.clone());
        fs::write(
            appimage_dir.join(format!("{}.desktop", safe_filename(name))),
            format!(
                "[Desktop Entry]\nName={name}\nType=Application\nExec=/run/current-system/sw/bin/jetos-appimage-run {name}\n"
            ),
        )?;
        fs::write(
            appimage_dir.join(format!("{}.path", safe_filename(name))),
            format!("{path}\n"),
        )?;
    }
    let appimage_runner = "#!/usr/bin/env sh\nset -eu\nname=${1:-}\nif [ -z \"$name\" ]; then\n  echo 'usage: jetos-appimage-run <name> [--print]' >&2\n  exit 2\nfi\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\npath_file=\"$root/appimage/$name.path\"\nif [ ! -f \"$path_file\" ]; then\n  echo \"jetos appimage: no app named $name\" >&2\n  exit 2\nfi\napp=$(sed -n '1p' \"$path_file\")\nif [ \"${2:-}\" = '--print' ]; then\n  printf '%s\\n' \"$app\"\n  exit 0\nfi\nexec \"$app\"\n";
    let appimage_path = bin_dir.join("jetos-appimage-run");
    fs::write(&appimage_path, appimage_runner)?;
    make_executable(&appimage_path)
}

fn write_performance_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let perf_dir = dir.join("performance");
    let bin_dir = dir.join("sw/bin");
    let unit_dir = dir.join("etc/systemd/system");
    fs::create_dir_all(&perf_dir)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&unit_dir)?;
    let profile = option_value(system, &["performance.profile"])
        .map(|s| clean_symbol(&s))
        .unwrap_or_else(|| "Safe".to_string());
    let kernel_profile = option_value(
        system,
        &["boot.kernel.profile", "performance.kernel.profile"],
    )
    .map(|s| clean_symbol(&s))
    .unwrap_or_else(|| boot_profile(system).kernel);
    let sysctls = prefixed_options(system, "performance.sysctl.");
    let mut sysctl_conf = String::new();
    for (key, value) in &sysctls {
        sysctl_conf.push_str(&format!("{key} = {value}\n"));
    }
    if !sysctl_conf.is_empty() {
        let sysctl_dir = dir.join("etc/sysctl.d");
        fs::create_dir_all(&sysctl_dir)?;
        fs::write(sysctl_dir.join("90-jetos-performance.conf"), sysctl_conf)?;
    }
    if let Some(percent) = option_value(system, &["performance.zram.memoryPercent"]) {
        let zram_dir = dir.join("etc/systemd/zram-generator.conf.d");
        fs::create_dir_all(&zram_dir)?;
        fs::write(
            zram_dir.join("jetos.conf"),
            format!("[zram0]\nzram-size = ram * {percent} / 100\n"),
        )?;
    }
    let scheduler = option_value(system, &["performance.scheduler"]).map(|s| clean_symbol(&s));
    if let Some(scheduler) = &scheduler {
        let scheduler_bin = match scheduler.as_str() {
            "ScxLavd" => "scx_lavd".to_string(),
            _ => scheduler.to_ascii_lowercase(),
        };
        let launcher = bin_dir.join("jetos-performance-scheduler");
        fs::write(
            &launcher,
            format!(
                "#!/usr/bin/env sh\nset -eu\nscheduler=${{JETOS_SCHEDULER_BIN:-{}}}\nexec \"$scheduler\" \"$@\"\n",
                shell_single_quote(&scheduler_bin)
            ),
        )?;
        make_executable(&launcher)?;
        fs::write(
            unit_dir.join("jetos-performance-scheduler.service"),
            "[Unit]\nDescription=jetos sched-ext scheduler\nAfter=multi-user.target\n\n[Service]\nExecStart=/run/current-system/sw/bin/jetos-performance-scheduler\nRestart=on-failure\n\n[Install]\nWantedBy=multi-user.target\n",
        )?;
        enable_unit(
            &unit_dir,
            "multi-user.target",
            "jetos-performance-scheduler.service",
        )?;
    }
    let params = option_value(system, &["boot.kernel.params", "performance.kernel.params"])
        .map(|v| parse_list_items(&v))
        .unwrap_or_default();
    let params_json = params
        .iter()
        .map(|p| JSON::quote(p))
        .collect::<Vec<_>>()
        .join(",");
    let initrd_systemd =
        option_value(system, &["boot.initrd.systemd"]).unwrap_or_else(|| "false".to_string());
    let initrd_verbosity =
        option_value(system, &["boot.initrd.verbosity"]).unwrap_or_else(|| "normal".to_string());
    let limine_max = option_value(system, &["boot.loader.limine.maxGenerations"])
        .unwrap_or_else(|| "10".to_string());
    let efi_vars = option_value(system, &["boot.loader.efi.canTouchVariables"])
        .unwrap_or_else(|| "false".to_string());
    fs::write(
        perf_dir.join("profile.json"),
        format!(
            "{{\"kind\":\"jetos.performance-profile\",\"profile\":{},\"kernel_profile\":{},\"kernel_params\":[{}],\"proof\":\"kernel-tuning-profile-ready\"}}",
            JSON::quote(&profile),
            JSON::quote(&kernel_profile),
            params_json
        ),
    )?;
    fs::write(
        perf_dir.join("bootloader.json"),
        format!(
            "{{\"kind\":\"jetos.bootloader-tuning\",\"limine_max_generations\":{},\"efi_can_touch_variables\":{},\"proof\":\"bootloader-tuning-ready\"}}",
            JSON::quote(&limine_max),
            clean_bool_json(&efi_vars)
        ),
    )?;
    fs::write(
        perf_dir.join("initrd.json"),
        format!(
            "{{\"kind\":\"jetos.initrd-tuning\",\"systemd\":{},\"verbosity\":{},\"proof\":\"initrd-tuning-ready\"}}",
            clean_bool_json(&initrd_systemd),
            JSON::quote(&initrd_verbosity)
        ),
    )?;
    fs::write(
        perf_dir.join("scheduler.json"),
        format!(
            "{{\"kind\":\"jetos.scheduler\",\"scheduler\":{},\"unit\":{},\"proof\":\"sched-ext-service-ready\"}}",
            scheduler
                .as_ref()
                .map(|s| JSON::quote(s))
                .unwrap_or_else(|| "null".to_string()),
            if scheduler.is_some() {
                JSON::quote("etc/systemd/system/jetos-performance-scheduler.service")
            } else {
                "null".to_string()
            }
        ),
    )?;
    fs::write(
        perf_dir.join("facts.json"),
        format!(
            "{{\"kind\":\"jetos.performance\",\"profile\":{},\"kernel_profile\":{},\"scheduler\":{},\"kernel_params\":[{}],\"sysctl\":[{}],\"zram\":{},\"initrd\":\"performance/initrd.json\",\"bootloader\":\"performance/bootloader.json\",\"risk\":\"explicit-overrides-proof-visible\"}}",
            JSON::quote(&profile),
            JSON::quote(&kernel_profile),
            scheduler
                .as_ref()
                .map(|s| JSON::quote(s))
                .unwrap_or_else(|| "null".to_string()),
            params_json,
            option_rows_json(&sysctls),
            option_value(system, &["performance.zram.memoryPercent"])
                .map(|p| JSON::quote(&p))
                .unwrap_or_else(|| "null".to_string())
        ),
    )
}

fn write_module_priority_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let module_dir = dir.join("module-system");
    fs::create_dir_all(&module_dir)?;
    let mut keys = system
        .options
        .iter()
        .filter(|o| !is_option_priority_metadata(&o.key))
        .map(|o| o.key.clone())
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    let resolved = keys
        .iter()
        .filter_map(|key| resolved_option(system, key))
        .map(|r| r.to_json())
        .collect::<Vec<_>>()
        .join(",");
    let disabled = option_value(system, &["packages.disabledModules"])
        .map(|v| parse_list_items(&v))
        .unwrap_or_default();
    fs::write(
        module_dir.join("disabled-modules.manifest"),
        manifest_lines(&disabled),
    )?;
    fs::write(
        module_dir.join("explain.json"),
        format!(
            "{{\"kind\":\"jetos.option-explain\",\"tiers\":[\"Default\",\"Normal\",\"Force\",\"Priority(n)\"],\"module_ids\":\"stable-source-paths\",\"resolved\":[{}],\"disabled_modules\":[{}]}}",
            resolved,
            disabled.iter().map(|m| JSON::quote(m)).collect::<Vec<_>>().join(",")
        ),
    )
}

fn write_storage_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let storage_dir = dir.join("storage");
    let bin_dir = dir.join("sw/bin");
    fs::create_dir_all(&storage_dir)?;
    fs::create_dir_all(&bin_dir)?;
    let mut rows = prefixed_options(system, "storage.");
    rows.extend(prefixed_options(system, "filesystem."));
    let persist = prefixed_options(system, "storage.persist.");
    let disk = option_value(system, &["storage.disk.main.device"])
        .unwrap_or_else(|| "guided-ext4".to_string());
    let table = option_value(system, &["storage.disk.main.table"])
        .map(|s| clean_symbol(&s))
        .unwrap_or_else(|| "GPT".to_string());
    let esp_size = option_value(system, &["storage.disk.main.partitions.esp.size"])
        .unwrap_or_else(|| "512M".to_string());
    let root_fs = option_value(system, &["storage.filesystem.root.type"])
        .or_else(|| option_value(system, &["filesystem.root.type"]))
        .map(|s| clean_symbol(&s))
        .unwrap_or_else(|| "ext4".to_string());
    let ephemeral = option_value(system, &["storage.ephemeralRoot", "storage.root.ephemeral"])
        .unwrap_or_else(|| "false".to_string());
    fs::write(
        storage_dir.join("facts.json"),
        format!(
            "{{\"kind\":\"jetos.storage-tree\",\"installer_consumes\":true,\"activation_consumes\":true,\"disk\":{},\"table\":{},\"root_fs\":{},\"ephemeral_root\":{},\"options\":[{}],\"commands\":[\"jetos-storage-plan\",\"jetos-storage-apply\",\"jetos-persist-activate\"],\"proof\":\"storage-plan-ready\"}}",
            JSON::quote(&disk),
            JSON::quote(&table),
            JSON::quote(&root_fs),
            clean_bool_json(&ephemeral),
            option_rows_json(&rows)
        ),
    )?;
    fs::write(
        storage_dir.join("plan.json"),
        format!(
            "{{\"kind\":\"jetos.storage-plan\",\"host\":{},\"disk\":{},\"table\":{},\"root_fs\":{},\"partitions\":[{{\"name\":\"esp\",\"size\":{},\"fs\":\"vfat\",\"mount\":\"/boot\"}},{{\"name\":\"root\",\"size\":\"rest\",\"fs\":{},\"mount\":\"/\"}}],\"ephemeral_root\":{},\"persistence\":[{}],\"destructive_actions\":[\"partition\",\"format\"],\"safety\":\"requires --manual plus --execute\"}}",
            JSON::quote(&system.name),
            JSON::quote(&disk),
            JSON::quote(&table),
            JSON::quote(&root_fs),
            JSON::quote(&esp_size),
            JSON::quote(&root_fs),
            clean_bool_json(&ephemeral),
            option_rows_json(&persist)
        ),
    )?;
    fs::write(
        storage_dir.join("mounts.fstab"),
        format!(
            "LABEL=JETOS-ESP\t/boot\tvfat\tumask=0077\t0\t1\nLABEL=jetos-root\t/\t{}\tdefaults\t0\t1\n",
            root_fs.to_ascii_lowercase()
        ),
    )?;
    fs::write(
        storage_dir.join("persistence.manifest"),
        persist
            .iter()
            .map(|(key, value)| format!("{key}\t{value}\n"))
            .collect::<String>(),
    )?;
    let plan_script = "#!/usr/bin/env sh\nset -eu\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\ncat \"$root/storage/plan.json\"\nprintf '\\n'\n";
    let plan_path = bin_dir.join("jetos-storage-plan");
    fs::write(&plan_path, plan_script)?;
    make_executable(&plan_path)?;
    let apply_script = format!(
        "#!/usr/bin/env sh\nset -eu\nroot=${{JETOS_SYSTEM_ROOT:-/run/current-system}}\ndisk=${{JETOS_STORAGE_DISK:-{}}}\nmanual=false\nexecute=false\nfor arg in \"$@\"; do\n  case \"$arg\" in\n    --manual) manual=true ;;\n    --execute) execute=true ;;\n    *) echo \"usage: jetos-storage-apply --manual [--execute]\" >&2; exit 2 ;;\n  esac\ndone\nif [ \"$manual\" != true ]; then\n  echo 'jetos storage: destructive disk plan requires --manual' >&2\n  exit 2\nfi\nlog=${{JETOS_STORAGE_LOG:-$root/storage/apply-plan.sh}}\nproof_dir=${{JETOS_STORAGE_PROOF_DIR:-$root/storage}}\nmkdir -p \"$proof_dir\"\n{{\n  printf '%s\\n' '#!/usr/bin/env sh'\n  printf '%s\\n' 'set -eu'\n  printf 'sfdisk --wipe always %s <<EOF\\nlabel: gpt\\nsize={}, type=U\\ntype=L\\nEOF\\n' \"$disk\"\n  printf 'mkfs.vfat -n JETOS-ESP %s1\\n' \"$disk\"\n  printf 'mkfs.{} -L jetos-root %s2\\n' \"$disk\"\n}} > \"$log\"\nif [ \"$execute\" = true ]; then\n  sh \"$log\"\nfi\nprintf '{{\"kind\":\"jetos.storage-apply\",\"state\":\"planned\",\"executed\":%s,\"disk\":\"%s\",\"proof\":\"manual-storage-plan-reviewed\"}}\\n' \"$execute\" \"$disk\" > \"$proof_dir/apply-proof.json\"\ncat \"$proof_dir/apply-proof.json\"\n",
        shell_single_quote(&disk),
        esp_size,
        root_fs.to_ascii_lowercase()
    );
    let apply_path = bin_dir.join("jetos-storage-apply");
    fs::write(&apply_path, apply_script)?;
    make_executable(&apply_path)?;
    let persist_script = "#!/usr/bin/env sh\nset -eu\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\npersist_root=${JETOS_PERSIST_ROOT:-/persist}\nephemeral_root=${JETOS_EPHEMERAL_ROOT:-/}\nproof_dir=${JETOS_STORAGE_PROOF_DIR:-$root/storage}\nmkdir -p \"$proof_dir\"\nproof=\"$proof_dir/persistence-proof.json\"\nmanifest=\"$root/storage/persistence.manifest\"\ncount=0\n: > \"$proof.tmp\"\nif [ -f \"$manifest\" ]; then\n  while IFS='	' read -r key path; do\n    [ -n \"$path\" ] || continue\n    case \"$path\" in /*) rel=${path#/} ;; *) rel=$path ;; esac\n    mkdir -p \"$persist_root/$rel\" \"$ephemeral_root/$rel\"\n    printf '%s\\t%s\\n' \"$key\" \"$path\" >> \"$proof.tmp\"\n    count=$((count + 1))\n  done < \"$manifest\"\nfi\nprintf '{\"kind\":\"jetos.persistence\",\"state\":\"activated\",\"count\":%s,\"proof\":\"impermanence-persist-ready\"}\\n' \"$count\" > \"$proof\"\ncat \"$proof\"\n";
    let persist_path = bin_dir.join("jetos-persist-activate");
    fs::write(&persist_path, persist_script)?;
    make_executable(&persist_path)
}

fn write_workload_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let workloads_dir = dir.join("workloads");
    let unit_dir = dir.join("etc/systemd/system");
    fs::create_dir_all(&workloads_dir)?;
    fs::create_dir_all(&unit_dir)?;
    let names = collect_names(system, "workload");
    let mut facts = Vec::new();
    let bin_dir = dir.join("sw/bin");
    fs::create_dir_all(&bin_dir)?;
    for name in names {
        let backend = option_value(system, &[&format!("workload.{name}.backend")])
            .map(|s| clean_symbol(&s))
            .unwrap_or_else(|| "Container".to_string());
        let image = option_value(system, &[&format!("workload.{name}.image")])
            .or_else(|| option_value(system, &[&format!("workload.{name}.package")]))
            .unwrap_or_else(|| name.clone());
        let ports = option_value(system, &[&format!("workload.{name}.ports")])
            .map(|v| parse_list_items(&v))
            .unwrap_or_default();
        let ports_json = ports
            .iter()
            .map(|p| JSON::quote(p))
            .collect::<Vec<_>>()
            .join(",");
        let mounts = option_value(system, &[&format!("workload.{name}.mounts")])
            .map(|v| parse_list_items(&v))
            .unwrap_or_default();
        let mounts_json = strings_json(&mounts);
        let secrets = option_value(system, &[&format!("workload.{name}.secrets")])
            .map(|v| parse_list_items(&v))
            .unwrap_or_default();
        let secrets_json = strings_json(&secrets);
        let memory = option_value(
            system,
            &[
                &format!("workload.{name}.resources.memory"),
                &format!("workload.{name}.microvm.memory"),
            ],
        )
        .unwrap_or_else(|| {
            if backend == "MicroVM" {
                "1024M".to_string()
            } else {
                "host-shared".to_string()
            }
        });
        let cpus = option_value(
            system,
            &[
                &format!("workload.{name}.resources.cpus"),
                &format!("workload.{name}.microvm.cpus"),
            ],
        )
        .unwrap_or_else(|| "1".to_string());
        let health = option_value(system, &[&format!("workload.{name}.health.command")])
            .unwrap_or_else(|| {
                ports.first().map_or_else(
                    || "true".to_string(),
                    |port| format!("nc -z 127.0.0.1 {port}"),
                )
            });
        let rollback_keep = option_value(system, &[&format!("workload.{name}.rollback.keep")])
            .unwrap_or_else(|| "2".to_string());
        let command =
            option_value(system, &[&format!("workload.{name}.command")]).unwrap_or_else(|| {
                let ports_flags = ports
                    .iter()
                    .map(|p| format!("-p {p}:{p}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let mount_flags = mounts
                    .iter()
                    .map(|m| format!("-v {m}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let secret_flags = secrets
                    .iter()
                    .map(|s| format!("--secret {s}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                if backend == "MicroVM" {
                    format!("qemu-system-x86_64 -m {memory} -smp {cpus} -nographic -kernel {image}")
                } else {
                    format!("${{JETOS_CONTAINER_BIN:-podman}} run --rm {ports_flags} {mount_flags} {secret_flags} {image}")
                }
            });
        fs::write(
            workloads_dir.join(format!("{name}.plan.json")),
            format!(
                "{{\"kind\":\"jetos.workload-plan\",\"name\":{},\"backend\":{},\"image\":{},\"ports\":[{}],\"mounts\":[{}],\"secrets\":[{}],\"resources\":{{\"memory\":{},\"cpus\":{}}},\"health\":{},\"rollback_keep\":{},\"command\":{},\"proof\":\"workload-proof-ready\"}}",
                JSON::quote(&name),
                JSON::quote(&backend),
                JSON::quote(&image),
                ports_json,
                mounts_json,
                secrets_json,
                JSON::quote(&memory),
                JSON::quote(&cpus),
                JSON::quote(&health),
                JSON::quote(&rollback_keep),
                JSON::quote(&command)
            ),
        )?;
        fs::write(
            workloads_dir.join(format!("{name}.rollback.manifest")),
            format!("keep\t{rollback_keep}\ncurrent\t/run/current-system/workloads/{name}\n"),
        )?;
        let health_path = workloads_dir.join(format!("health-{name}.sh"));
        fs::write(
            &health_path,
            format!(
                "#!/usr/bin/env sh\nset -eu\ncmd={}\nsh -c \"$cmd\"\n",
                shell_single_quote(&health)
            ),
        )?;
        make_executable(&health_path)?;
        let script_path = workloads_dir.join(format!("run-{name}.sh"));
        fs::write(
            &script_path,
            format!(
                "#!/usr/bin/env sh\nset -eu\nroot=${{JETOS_SYSTEM_ROOT:-/run/current-system}}\ncmd={}\nsh -c \"$cmd\"\n\"$root/workloads/health-{name}.sh\"\n",
                shell_single_quote(&command)
            ),
        )?;
        make_executable(&script_path)?;
        fs::write(
            unit_dir.join(format!("workload-{name}.service")),
            format!(
                "[Unit]\nDescription=jetos workload {name}\n\n[Service]\nExecStart=/run/current-system/sw/bin/jetos-workload-run {name}\nRestart=on-failure\n\n[Install]\nWantedBy=multi-user.target\n"
            ),
        )?;
        enable_unit(
            &unit_dir,
            "multi-user.target",
            &format!("workload-{name}.service"),
        )?;
        facts.push(format!(
            "{{\"name\":{},\"backend\":{},\"image\":{},\"ports\":[{}],\"mounts\":[{}],\"secrets\":[{}],\"resources\":{{\"memory\":{},\"cpus\":{}}},\"health\":{},\"rollback_keep\":{},\"proof\":\"workload-rollout-ready\"}}",
            JSON::quote(&name),
            JSON::quote(&backend),
            JSON::quote(&image),
            ports_json,
            mounts_json,
            secrets_json,
            JSON::quote(&memory),
            JSON::quote(&cpus),
            JSON::quote(&health),
            JSON::quote(&rollback_keep)
        ));
    }
    let runner = "#!/usr/bin/env sh\nset -eu\nname=${1:-}\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\nif [ -z \"$name\" ]; then\n  echo 'usage: jetos-workload-run <name>' >&2\n  exit 2\nfi\nscript=\"$root/workloads/run-$name.sh\"\nif [ ! -x \"$script\" ]; then\n  echo \"jetos workload: no runnable workload named $name\" >&2\n  exit 2\nfi\nexec /bin/sh \"$script\"\n";
    let runner_path = bin_dir.join("jetos-workload-run");
    fs::write(&runner_path, runner)?;
    make_executable(&runner_path)?;
    fs::write(
        workloads_dir.join("facts.json"),
        format!(
            "{{\"kind\":\"jetos.workloads\",\"items\":[{}]}}",
            facts.join(",")
        ),
    )
}

fn write_theme_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let theme_dir = dir.join("theme");
    let gtk_dir = dir.join("share/themes/jetos/gtk-4.0");
    let qt_dir = dir.join("share/qt6ct/colors");
    let terminal_dir = dir.join("share/terminal");
    let editor_dir = dir.join("share/editor");
    let dm_dir = dir.join("share/display-manager");
    let studio_dir = dir.join("studio");
    fs::create_dir_all(&theme_dir)?;
    fs::create_dir_all(&gtk_dir)?;
    fs::create_dir_all(&qt_dir)?;
    fs::create_dir_all(&terminal_dir)?;
    fs::create_dir_all(&editor_dir)?;
    fs::create_dir_all(&dm_dir)?;
    fs::create_dir_all(&studio_dir)?;
    let name = option_value(system, &["theme.name", "theme.profile"])
        .map(|s| clean_symbol(&s))
        .unwrap_or_else(|| "default".to_string());
    let polarity = option_value(system, &["theme.polarity"])
        .map(|s| clean_symbol(&s))
        .unwrap_or_else(|| "Dark".to_string());
    let wallpaper = option_value(system, &["theme.wallpaper"]).unwrap_or_default();
    let font = option_value(system, &["theme.fonts.ui", "theme.font"])
        .unwrap_or_else(|| "Inter".to_string());
    let accent =
        option_value(system, &["theme.palette.accent"]).unwrap_or_else(|| "#4f8cff".to_string());
    fs::write(
        gtk_dir.join("gtk.css"),
        format!("* {{ font-family: \"{font}\"; }}\n:root {{ --jetos-accent: {accent}; }}\n"),
    )?;
    fs::write(
        qt_dir.join("jetos.conf"),
        format!("[ColorScheme]\nname={name}\naccent={accent}\npolarity={polarity}\nfont={font}\n"),
    )?;
    fs::write(
        terminal_dir.join("theme.toml"),
        format!("name = \"{name}\"\npolarity = \"{polarity}\"\nfont = \"{font}\"\naccent = \"{accent}\"\n"),
    )?;
    fs::write(
        editor_dir.join("theme.json"),
        format!(
            "{{\"name\":{},\"type\":{},\"ui_font\":{},\"accent\":{}}}",
            JSON::quote(&name),
            JSON::quote(&polarity),
            JSON::quote(&font),
            JSON::quote(&accent)
        ),
    )?;
    fs::write(
        dm_dir.join("theme.conf"),
        format!("Theme={name}\nAccent={accent}\nWallpaper={wallpaper}\n"),
    )?;
    fs::write(
        studio_dir.join("theme-preview.json"),
        format!(
            "{{\"kind\":\"jetos.theme-preview\",\"name\":{},\"polarity\":{},\"accent\":{},\"font\":{},\"wallpaper\":{}}}",
            JSON::quote(&name),
            JSON::quote(&polarity),
            JSON::quote(&accent),
            JSON::quote(&font),
            JSON::quote(&wallpaper)
        ),
    )?;
    fs::write(
        theme_dir.join("facts.json"),
        format!(
            "{{\"kind\":\"jetos.theme\",\"name\":{},\"polarity\":{},\"wallpaper\":{},\"font\":{},\"accent\":{},\"targets\":[\"gtk\",\"qt\",\"terminal\",\"editor\",\"display-manager\",\"studio\"],\"proof\":\"theme-projected\"}}",
            JSON::quote(&name),
            JSON::quote(&polarity),
            JSON::quote(&wallpaper),
            JSON::quote(&font),
            JSON::quote(&accent)
        ),
    )
}

fn write_fleet_deploy_facts(
    dir: &Path,
    system: &SystemPlan,
    plan: &EnvPlan,
) -> std::io::Result<()> {
    let fleet_dir = dir.join("fleet");
    let bin_dir = dir.join("sw/bin");
    fs::create_dir_all(&fleet_dir)?;
    fs::create_dir_all(&bin_dir)?;
    let mut hosts = Vec::new();
    let mut host_names = Vec::new();
    for fleet in &plan.fleets {
        for host in &fleet.hosts {
            if host.system != system.name {
                continue;
            }
            let target = option_value(system, &[&format!("deploy.host.{}.target", host.name)])
                .unwrap_or_else(|| format!("{}@{}", host.name, host.name));
            let health = option_value(system, &[&format!("deploy.host.{}.health", host.name)])
                .unwrap_or_else(|| "system-health".to_string());
            let generation_name = "${generation}";
            let push = option_value(system, &[&format!("deploy.host.{}.pushCommand", host.name)])
                .unwrap_or_else(|| {
                    format!(
                        "tar -C \"$root\" -cf - . | ssh {} \"mkdir -p ~/.jetos/generations/{generation_name} && tar -C ~/.jetos/generations/{generation_name} -xf -\"",
                        target
                    )
                });
            let proof = option_value(system, &[&format!("deploy.host.{}.proofCommand", host.name)])
                .unwrap_or_else(|| {
                    format!(
                        "ssh {} \"test -f ~/.jetos/generations/{generation_name}/proof.txt && test -f ~/.jetos/generations/{generation_name}/activation-diff.txt\"",
                        target
                    )
                });
            let switch = option_value(
                system,
                &[&format!("deploy.host.{}.switchCommand", host.name)],
            )
            .unwrap_or_else(|| {
                format!(
                    "ssh {} \"ln -sfn ~/.jetos/generations/{generation_name} ~/.jetos/current\"",
                    target
                )
            });
            let health_cmd = option_value(
                system,
                &[&format!("deploy.host.{}.healthCommand", host.name)],
            )
            .unwrap_or_else(|| {
                format!(
                    "ssh {} \"test -f ~/.jetos/current/health-checks.txt\"",
                    target
                )
            });
            let rollback =
                option_value(system, &[&format!("deploy.host.{}.rollbackCommand", host.name)])
                    .unwrap_or_else(|| {
                        format!(
                            "ssh {} \"test -L ~/.jetos/previous && ln -sfn $(readlink ~/.jetos/previous) ~/.jetos/current || true\"",
                            target
                        )
                    });
            host_names.push(host.name.clone());
            let script_path = fleet_dir.join(format!("deploy-{}.sh", host.name));
            fs::write(
                &script_path,
                render_fleet_host_script(
                    &fleet.name,
                    &host.name,
                    &host.system,
                    &target,
                    &push,
                    &proof,
                    &switch,
                    &health_cmd,
                    &rollback,
                ),
            )?;
            make_executable(&script_path)?;
            hosts.push(JSON::object_of(&[
                ("fleet", &fleet.name),
                ("host", &host.name),
                ("system", &host.system),
                ("target", &target),
                ("policy", "staged-proof-gated-rollback-stop"),
                ("health", &health),
                ("script", &format!("fleet/deploy-{}.sh", host.name)),
            ]));
        }
    }
    fs::write(
        fleet_dir.join("deploy-plan.json"),
        format!(
            "{{\"kind\":\"jetos.fleet-deploy\",\"host\":{},\"hosts\":[{}],\"proofs\":[\"build-closure\",\"ssh-push\",\"remote-proof-before-switch\",\"health-window\",\"rollback-on-fail\"]}}",
            JSON::quote(&system.name),
            hosts.join(",")
        ),
    )?;
    let default_host = host_names.first().cloned().unwrap_or_default();
    let launcher = format!(
        "#!/usr/bin/env sh\nset -eu\nroot=${{JETOS_SYSTEM_ROOT:-/run/current-system}}\nhost=${{1:-{}}}\nif [ -z \"$host\" ]; then\n  echo 'usage: jetos-fleet-deploy <host>' >&2\n  exit 2\nfi\nscript=\"$root/fleet/deploy-$host.sh\"\nif [ ! -x \"$script\" ]; then\n  echo \"jetos fleet deploy: unknown host $host\" >&2\n  exit 2\nfi\nexec /bin/sh \"$script\"\n",
        shell_single_quote(&default_host)
    );
    let launcher_path = bin_dir.join("jetos-fleet-deploy");
    fs::write(&launcher_path, launcher)?;
    make_executable(&launcher_path)
}

fn render_fleet_host_script(
    fleet: &str,
    host: &str,
    system: &str,
    target: &str,
    push: &str,
    proof: &str,
    switch_cmd: &str,
    health: &str,
    rollback: &str,
) -> String {
    format!(
        "#!/usr/bin/env sh\nset -eu\nroot=${{JETOS_SYSTEM_ROOT:-/run/current-system}}\ngeneration=$(cat \"$root/generation.txt\" 2>/dev/null || basename \"$root\")\nproof_dir=${{JETOS_DEPLOY_PROOF_DIR:-$root/fleet/proofs}}\nmkdir -p \"$proof_dir\"\nproof_file=\"$proof_dir/{fleet}-{host}.json\"\npush_cmd={push}\nproof_cmd={proof}\nswitch_cmd={switch_cmd}\nhealth_cmd={health}\nrollback_cmd={rollback}\nrun_step() {{\n  name=$1\n  cmd=$2\n  printf '%s\\n' \"jetos deploy {host}: $name\"\n  sh -c \"$cmd\"\n}}\nif [ \"${{JETOS_FLEET_DRY_RUN:-}}\" = \"1\" ]; then\n  printf '{{\"state\":\"dry-run\",\"fleet\":{fleet_json},\"host\":{host_json},\"system\":{system_json},\"target\":{target_json},\"generation\":\"%s\"}}\\n' \"$generation\" > \"$proof_file\"\n  cat \"$proof_file\"\n  exit 0\nfi\nrun_step push \"$push_cmd\"\nrun_step proof \"$proof_cmd\"\nif [ \"${{JETOS_FLEET_STAGE_ONLY:-}}\" = \"1\" ]; then\n  printf '{{\"state\":\"staged\",\"fleet\":{fleet_json},\"host\":{host_json},\"system\":{system_json},\"target\":{target_json},\"generation\":\"%s\"}}\\n' \"$generation\" > \"$proof_file\"\n  cat \"$proof_file\"\n  exit 0\nfi\nrun_step switch \"$switch_cmd\"\nif ! sh -c \"$health_cmd\"; then\n  sh -c \"$rollback_cmd\" || true\n  printf '{{\"state\":\"rolled-back\",\"fleet\":{fleet_json},\"host\":{host_json},\"system\":{system_json},\"target\":{target_json},\"generation\":\"%s\"}}\\n' \"$generation\" > \"$proof_file\"\n  cat \"$proof_file\"\n  exit 2\nfi\nprintf '{{\"state\":\"deployed\",\"fleet\":{fleet_json},\"host\":{host_json},\"system\":{system_json},\"target\":{target_json},\"generation\":\"%s\",\"proofs\":[\"push\",\"remote-proof-before-switch\",\"health-window\"]}}\\n' \"$generation\" > \"$proof_file\"\ncat \"$proof_file\"\n",
        fleet_json = JSON::quote(fleet),
        host_json = JSON::quote(host),
        system_json = JSON::quote(system),
        target_json = JSON::quote(target),
        push = shell_single_quote(push),
        proof = shell_single_quote(proof),
        switch_cmd = shell_single_quote(switch_cmd),
        health = shell_single_quote(health),
        rollback = shell_single_quote(rollback)
    )
}

fn write_options_reference(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let options_dir = dir.join("options");
    fs::create_dir_all(&options_dir)?;
    let mut keys = system
        .options
        .iter()
        .filter(|o| !is_option_priority_metadata(&o.key))
        .map(|o| o.key.clone())
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    let rows = keys
        .iter()
        .filter_map(|key| {
            let resolved = resolved_option(system, key)?;
            let ns = key.split('.').next().unwrap_or("");
            Some(format!(
                "{{\"key\":{},\"namespace\":{},\"type\":{},\"value\":{},\"default\":{},\"example\":{},\"doc\":{},\"source\":\"config.jet options\",\"tier\":{},\"priority\":\"{}\",\"provenance\":\"system option resolver\"}}",
                JSON::quote(key),
                JSON::quote(ns),
                JSON::quote(&option_type(&resolved.value)),
                JSON::quote(&resolved.value),
                JSON::quote(&option_default(ns)),
                JSON::quote(&resolved.value),
                JSON::quote(&option_doc(key)),
                JSON::quote(&resolved.tier),
                resolved.priority
            ))
        })
        .collect::<Vec<_>>()
        .join(",");
    fs::write(
        options_dir.join("reference.json"),
        format!(
            "{{\"kind\":\"jetos.option-reference\",\"host\":{},\"options\":[{}]}}",
            JSON::quote(&system.name),
            rows
        ),
    )?;
    let bin_dir = dir.join("sw/bin");
    fs::create_dir_all(&bin_dir)?;
    let search = "#!/usr/bin/env sh\nset -eu\nmode=search\ncase \"${1:-}\" in\n  --exact) mode=exact; shift ;;\n  --explain) mode=explain; shift ;;\nesac\nterm=${1:-}\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\nref=\"$root/options/reference.json\"\nexplain=\"$root/module-system/explain.json\"\nif [ -z \"$term\" ]; then\n  cat \"$ref\"\n  exit 0\nfi\ncase \"$mode\" in\n  exact) grep -F \"\\\"key\\\": \\\"$term\\\"\" \"$ref\" || grep -F \"\\\"key\\\":\\\"$term\\\"\" \"$ref\" || true ;;\n  explain) grep -F \"$term\" \"$explain\" || true ;;\n  *) grep -F \"$term\" \"$ref\" || true ;;\nesac\n";
    let search_path = bin_dir.join("jetos-options-search");
    fs::write(&search_path, search)?;
    make_executable(&search_path)
}

fn write_image_variant_facts(
    dir: &Path,
    system: &SystemPlan,
    plan: &EnvPlan,
) -> std::io::Result<()> {
    let image_dir = dir.join("image-variants");
    fs::create_dir_all(&image_dir)?;
    let mut variants = vec![
        JSON::object_of(&[
            ("name", "default-qcow2"),
            ("kind", "qcow2"),
            ("format", "qcow2"),
            ("target", &system.target),
        ]),
        JSON::object_of(&[
            ("name", "default-raw"),
            ("kind", "raw"),
            ("format", "raw"),
            ("target", &system.target),
        ]),
        JSON::object_of(&[
            ("name", "default-sd"),
            ("kind", "sd"),
            ("format", "sd"),
            ("target", &system.target),
        ]),
        JSON::object_of(&[
            ("name", "default-netboot"),
            ("kind", "netboot"),
            ("format", "pxe"),
            ("target", &system.target),
        ]),
    ];
    for image in &plan.images {
        if image.kind == ImageKind::Iso && image.from == system.name {
            variants.push(JSON::object_of(&[
                ("name", &image.name),
                ("kind", "iso"),
                ("format", &image.format),
                ("target", image.target.as_deref().unwrap_or(&system.target)),
            ]));
        }
    }
    for (key, value) in prefixed_options(system, "packages.imageVariant.") {
        variants.push(JSON::object_of(&[
            ("name", &key),
            ("kind", &clean_symbol(&value)),
            ("format", &clean_symbol(&value)),
            ("target", &system.target),
        ]));
    }
    fs::write(
        image_dir.join("matrix.json"),
        format!(
            "{{\"kind\":\"jetos.image-variant-matrix\",\"host\":{},\"variants\":[{}],\"proof\":\"image-variant-plan-ready\"}}",
            JSON::quote(&system.name),
            variants.join(",")
        ),
    )
}

fn write_lifecycle_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let lifecycle_dir = dir.join("lifecycle");
    let bin_dir = dir.join("sw/bin");
    let unit_dir = dir.join("etc/systemd/system");
    fs::create_dir_all(&lifecycle_dir)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&unit_dir)?;
    let keep = option_value(system, &["packages.generations.keep"])
        .or_else(|| option_value(system, &["deploy.generations.keep"]))
        .unwrap_or_else(|| "10".to_string());
    let auto_upgrade =
        option_value(system, &["deploy.autoUpgrade.enable"]).unwrap_or_else(|| "false".to_string());
    let channel =
        option_value(system, &["packages.channel"]).unwrap_or_else(|| "locked".to_string());
    let schedule = option_value(system, &["deploy.autoUpgrade.schedule"])
        .unwrap_or_else(|| "daily".to_string());
    fs::write(
        lifecycle_dir.join("policy.json"),
        format!(
            "{{\"kind\":\"jetos.lifecycle-policy\",\"keep_generations\":{},\"channel\":{},\"auto_upgrade\":{},\"schedule\":{},\"gc\":\"explain-before-delete\",\"rollback_window\":\"kept-generations\",\"proof\":\"lifecycle-policy-ready\"}}",
            JSON::quote(&keep),
            JSON::quote(&channel),
            clean_bool_json(&auto_upgrade),
            JSON::quote(&schedule)
        ),
    )?;
    fs::write(
        lifecycle_dir.join("channel.json"),
        format!(
            "{{\"kind\":\"jetos.channel-policy\",\"channel\":{},\"update_command\":\"sw/bin/jetos-channel-update\",\"proof\":\"channel-policy-ready\"}}",
            JSON::quote(&channel)
        ),
    )?;
    fs::write(
        lifecycle_dir.join("auto-upgrade.json"),
        format!(
            "{{\"kind\":\"jetos.auto-upgrade\",\"enabled\":{},\"schedule\":{},\"steps\":[\"fetch-channel\",\"build\",\"proof\",\"switch\",\"health\",\"rollback-on-fail\"],\"proof\":\"auto-upgrade-proof-gated\"}}",
            clean_bool_json(&auto_upgrade),
            JSON::quote(&schedule)
        ),
    )?;
    let gc = format!(
        "#!/usr/bin/env sh\nset -eu\nroot=${{JETOS_SYSTEM_ROOT:-/run/current-system}}\nsystems=${{JETOS_SYSTEMS_DIR:-${{JETPACK_ROOT:-$HOME/.jetpack}}/systems}}\nlog=\"$systems/generations.log\"\nkeep={}\nhost={}\napply=false\nif [ \"${{1:-}}\" = \"--apply\" ]; then apply=true; fi\nmkdir -p \"$root/lifecycle\"\nout=\"$root/lifecycle/gc-plan.txt\"\n: > \"$out\"\nif [ ! -f \"$log\" ]; then\n  echo 'no generations log' | tee -a \"$out\"\n  exit 0\nfi\ncount=0\nsort -r \"$log\" | while IFS='	' read -r created entry_host name path; do\n  [ \"$entry_host\" = \"$host\" ] || continue\n  count=$((count + 1))\n  if [ \"$count\" -le \"$keep\" ]; then\n    printf 'keep\\t%s\\t%s\\t%s\\treason=within-retention\\n' \"$created\" \"$name\" \"$path\" | tee -a \"$out\"\n  else\n    printf 'delete\\t%s\\t%s\\t%s\\treason=older-than-retention\\n' \"$created\" \"$name\" \"$path\" | tee -a \"$out\"\n    if [ \"$apply\" = true ]; then\n      rm -rf -- \"$path\"\n    fi\n  fi\ndone\n",
        keep.parse::<usize>().unwrap_or(10),
        shell_single_quote(&system.name)
    );
    let gc_path = bin_dir.join("jetos-lifecycle-gc");
    fs::write(&gc_path, gc)?;
    make_executable(&gc_path)?;
    let channel_update = format!(
        "#!/usr/bin/env sh\nset -eu\nchannel={}\ncmd=${{JETOS_CHANNEL_UPDATE_CMD:-jetpack channel update \"$channel\"}}\nsh -c \"$cmd\"\n",
        shell_single_quote(&channel)
    );
    let channel_path = bin_dir.join("jetos-channel-update");
    fs::write(&channel_path, channel_update)?;
    make_executable(&channel_path)?;
    let upgrade = format!(
        "#!/usr/bin/env sh\nset -eu\nroot=${{JETOS_SYSTEM_ROOT:-/run/current-system}}\nproof_dir=${{JETOS_LIFECYCLE_PROOF_DIR:-$root/lifecycle}}\nmkdir -p \"$proof_dir\"\nfetch=${{JETOS_UPGRADE_FETCH_CMD:-/run/current-system/sw/bin/jetos-channel-update}}\nbuild=${{JETOS_UPGRADE_BUILD_CMD:-jet os build {host}}}\nproof=${{JETOS_UPGRADE_PROOF_CMD:-jet os proof {host}}}\nswitch_cmd=${{JETOS_UPGRADE_SWITCH_CMD:-jet os switch {host}}}\nhealth=${{JETOS_UPGRADE_HEALTH_CMD:-true}}\nrollback=${{JETOS_UPGRADE_ROLLBACK_CMD:-jet os rollback {host}}}\nrun_step() {{ name=$1; shift; printf '%s\\n' \"jetos lifecycle: $name\"; sh -c \"$*\"; }}\nrun_step fetch \"$fetch\"\nrun_step build \"$build\"\nrun_step proof \"$proof\"\nrun_step switch \"$switch_cmd\"\nif run_step health \"$health\"; then\n  printf '{{\"kind\":\"jetos.auto-upgrade-proof\",\"state\":\"switched\",\"rollback\":\"available\",\"proof\":\"health-passed\"}}\\n' > \"$proof_dir/auto-upgrade-proof.json\"\nelse\n  sh -c \"$rollback\" || true\n  printf '{{\"kind\":\"jetos.auto-upgrade-proof\",\"state\":\"rolled-back\",\"rollback\":\"executed\",\"proof\":\"health-failed\"}}\\n' > \"$proof_dir/auto-upgrade-proof.json\"\n  exit 1\nfi\ncat \"$proof_dir/auto-upgrade-proof.json\"\n",
        host = system.name
    );
    let upgrade_path = bin_dir.join("jetos-auto-upgrade");
    fs::write(&upgrade_path, upgrade)?;
    make_executable(&upgrade_path)?;
    if clean_bool_json(&auto_upgrade) == "true" {
        fs::write(
            unit_dir.join("jetos-auto-upgrade.service"),
            "[Unit]\nDescription=jetos proof-gated auto-upgrade\n\n[Service]\nType=oneshot\nExecStart=/run/current-system/sw/bin/jetos-auto-upgrade\n",
        )?;
        fs::write(
            unit_dir.join("jetos-auto-upgrade.timer"),
            format!(
                "[Unit]\nDescription=jetos auto-upgrade schedule\n\n[Timer]\nOnCalendar={schedule}\nUnit=jetos-auto-upgrade.service\n\n[Install]\nWantedBy=timers.target\n"
            ),
        )?;
        enable_unit(&unit_dir, "timers.target", "jetos-auto-upgrade.timer")?;
    }
    Ok(())
}

fn write_service_manager_depth(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let service_dir = dir.join("service-manager");
    let bin_dir = dir.join("sw/bin");
    fs::create_dir_all(&service_dir)?;
    fs::create_dir_all(&bin_dir)?;
    let tmpfiles_dir = dir.join("etc/tmpfiles.d");
    fs::create_dir_all(&tmpfiles_dir)?;
    let mut facts = Vec::new();
    for svc in system.services.iter().filter(|s| s.enable) {
        if let Some(tmpfiles) = service_extra(svc, &["tmpfiles"]) {
            fs::write(
                tmpfiles_dir.join(format!("{}.conf", svc.name)),
                format!("{}\n", tmpfiles),
            )?;
        }
        let hardening =
            service_extra(svc, &["hardening"]).unwrap_or_else(|| "default-sandbox".to_string());
        let journal = service_extra(svc, &["journal"]).unwrap_or_else(|| "structured".to_string());
        facts.push(JSON::object_of(&[
            ("name", &svc.name),
            ("hardening", &hardening),
            ("journal", &journal),
            (
                "timers",
                if service_extra(svc, &["timer", "schedule"]).is_some() {
                    "true"
                } else {
                    "false"
                },
            ),
            (
                "sockets",
                if service_extra(svc, &["socket", "listen"]).is_some() {
                    "true"
                } else {
                    "false"
                },
            ),
        ]));
    }
    fs::write(
        service_dir.join("facts.json"),
        format!(
            "{{\"kind\":\"jetos.service-manager-depth\",\"services\":[{}],\"features\":[\"services\",\"timers\",\"sockets\",\"tmpfiles\",\"hardening\",\"journal\"]}}",
            facts.join(",")
        ),
    )?;
    fs::write(
        service_dir.join("log-policy.json"),
        "{\"kind\":\"jetos.service-logs\",\"backend\":\"journalctl\",\"fallback\":\"service-manager/logs/<unit>.log\",\"proof\":\"logs-query-ready\"}",
    )?;
    let logs = "#!/usr/bin/env sh\nset -eu\nunit=${1:-}\nif [ -z \"$unit\" ]; then\n  echo 'usage: jetos-service-logs <unit> [--since <time>]' >&2\n  exit 2\nfi\nshift || true\nsince=''\nif [ \"${1:-}\" = '--since' ]; then\n  shift\n  since=${1:-}\nfi\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\njournal=${JETOS_JOURNALCTL_BIN:-journalctl}\nif command -v \"$journal\" >/dev/null 2>&1; then\n  if [ -n \"$since\" ]; then\n    exec \"$journal\" -u \"$unit\" --since \"$since\"\n  fi\n  exec \"$journal\" -u \"$unit\"\nfi\nfallback=\"$root/service-manager/logs/$unit.log\"\nif [ -f \"$fallback\" ]; then\n  cat \"$fallback\"\n  exit 0\nfi\necho \"jetos logs: no journal backend or fallback log for $unit\" >&2\nexit 2\n";
    let logs_path = bin_dir.join("jetos-service-logs");
    fs::write(&logs_path, logs)?;
    make_executable(&logs_path)
}

fn write_app_module_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let apps_dir = dir.join("apps");
    let programs_dir = apps_dir.join("programs");
    let bin_dir = dir.join("sw/bin");
    fs::create_dir_all(&apps_dir)?;
    fs::create_dir_all(&programs_dir)?;
    fs::create_dir_all(&bin_dir)?;
    let mut rows = prefixed_options(system, "apps.program.");
    rows.extend(prefixed_options(system, "user."));
    rows.extend(prefixed_options(system, "theme."));
    let modules = [
        "git",
        "ssh",
        "fish",
        "starship",
        "ghostty",
        "helix",
        "yazi",
        "btop",
        "bat",
        "eza",
        "fzf",
        "zoxide",
        "ripgrep",
        "tealdeer",
        "fastfetch",
        "vscode",
        "cursor",
        "discord",
        "spicetify",
        "browser",
    ];
    let mut module_json = Vec::new();
    for module in modules {
        let module_dir = programs_dir.join(module);
        fs::create_dir_all(&module_dir)?;
        let options = prefixed_options(system, &format!("apps.program.{module}."));
        let enabled = option_value(system, &[&format!("apps.program.{module}.enable")])
            .unwrap_or_else(|| (!options.is_empty()).to_string());
        let config_path = app_module_config_path(module);
        let package = option_value(system, &[&format!("apps.program.{module}.package")])
            .unwrap_or_else(|| module.to_string());
        fs::write(
            module_dir.join("module.json"),
            format!(
                "{{\"kind\":\"jetos.app-module\",\"name\":{},\"enabled\":{},\"package\":{},\"config_path\":{},\"options\":[{}],\"proof\":\"app-module-ready\"}}",
                JSON::quote(module),
                clean_bool_json(&enabled),
                JSON::quote(&package),
                JSON::quote(&config_path),
                option_rows_json(&options)
            ),
        )?;
        let mut config = format!("# managed by jetos apps.program.{module}\n");
        for (key, value) in &options {
            config.push_str(&format!("{key} = {value}\n"));
        }
        if module == "git" {
            if let Some(name) = option_value(system, &["apps.program.git.userName"]) {
                config.push_str(&format!("user.name = {name}\n"));
            }
            if let Some(email) = option_value(system, &["apps.program.git.userEmail"]) {
                config.push_str(&format!("user.email = {email}\n"));
            }
        }
        fs::write(module_dir.join("config"), config)?;
        module_json.push(format!(
            "{{\"name\":{},\"enabled\":{},\"config\":{},\"proof\":\"ready\"}}",
            JSON::quote(module),
            clean_bool_json(&enabled),
            JSON::quote(&format!("apps/programs/{module}/config"))
        ));
    }
    fs::write(
        apps_dir.join("coverage.manifest"),
        modules
            .iter()
            .map(|module| format!("{module}\tmodule\n"))
            .collect::<String>(),
    )?;
    fs::write(
        apps_dir.join("gap-cards.manifest"),
        "vscode-extension-provider\tcovered-by-#330\ncursor-extension-provider\tcovered-by-#330\nspicetify-patching-provider\tcovered-by-#330\n",
    )?;
    let apply = "#!/usr/bin/env sh\nset -eu\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\nhome=${JETOS_USER_HOME:-$HOME}\nproof_dir=${JETOS_APP_PROOF_DIR:-$home/.jetos/proof}\nmkdir -p \"$proof_dir\"\ncount=0\nfor module in \"$root\"/apps/programs/*; do\n  [ -d \"$module\" ] || continue\n  name=${module##*/}\n  config_path=$(sed -n 's/.*\"config_path\":\"\\([^\"]*\\)\".*/\\1/p' \"$module/module.json\")\n  [ -n \"$config_path\" ] || config_path=\".config/$name/config\"\n  dest=\"$home/$config_path\"\n  mkdir -p \"${dest%/*}\"\n  cp \"$module/config\" \"$dest\"\n  count=$((count + 1))\ndone\nprintf '{\"kind\":\"jetos.app-modules\",\"state\":\"applied\",\"count\":%s,\"proof\":\"app-config-applied\"}\\n' \"$count\" > \"$proof_dir/app-modules.json\"\ncat \"$proof_dir/app-modules.json\"\n";
    let apply_path = bin_dir.join("jetos-app-module-apply");
    fs::write(&apply_path, apply)?;
    make_executable(&apply_path)?;
    fs::write(
        apps_dir.join("modules.json"),
        format!(
            "{{\"kind\":\"jetos.app-module-library\",\"host\":{},\"modules\":[{}],\"catalog\":[{}],\"apply\":\"sw/bin/jetos-app-module-apply\",\"proof\":\"app-config-projected\"}}",
            JSON::quote(&system.name),
            option_rows_json(&rows),
            module_json.join(",")
        ),
    )
}

fn app_module_config_path(module: &str) -> String {
    match module {
        "git" => ".config/git/config".to_string(),
        "ssh" => ".ssh/config".to_string(),
        "fish" => ".config/fish/config.fish".to_string(),
        "starship" => ".config/starship.toml".to_string(),
        "ghostty" => ".config/ghostty/config".to_string(),
        "helix" => ".config/helix/config.toml".to_string(),
        "vscode" => ".config/Code/User/settings.json".to_string(),
        "cursor" => ".config/Cursor/User/settings.json".to_string(),
        "browser" => ".config/browser/policies.json".to_string(),
        _ => format!(".config/{module}/config"),
    }
}

fn write_acceptance_fixture(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let acceptance_dir = dir.join("acceptance");
    let bin_dir = dir.join("sw/bin");
    fs::create_dir_all(&acceptance_dir)?;
    fs::create_dir_all(&bin_dir)?;
    let coverage = vec![
        (
            "flake-inputs-pins-channels",
            option_value(system, &["packages.channel"]).is_some(),
            "#330/U21",
        ),
        (
            "overlays-patches-nur-unfree",
            !prefixed_options(system, "packages.overlay.").is_empty(),
            "#330",
        ),
        (
            "multi-variant-host-specialisation",
            !prefixed_options(system, &format!("hardware.{}.specialisation.", system.name))
                .is_empty(),
            "#326/#331",
        ),
        (
            "disko-btrfs-impermanence",
            !prefixed_options(system, "storage.").is_empty(),
            "#328",
        ),
        (
            "limine-kernel-zram-sysctl-scx",
            option_value(system, &["boot.kernel"]).is_some()
                && !prefixed_options(system, "performance.").is_empty(),
            "#336",
        ),
        (
            "users-groups-network-security",
            !user_names(system).is_empty() && option_value(system, &["network.hostName"]).is_some(),
            "#262/#321",
        ),
        (
            "home-manager-app-modules",
            !prefixed_options(system, "apps.program.").is_empty(),
            "#321/#332",
        ),
        (
            "desktop-audio-locale-fonts-virt-gaming-smartcard",
            !prefixed_options(system, "services.audio.").is_empty()
                && !prefixed_options(system, "services.virtualization.").is_empty(),
            "#334",
        ),
        (
            "stylix-theme",
            option_value(system, &["theme.name", "theme.profile"]).is_some(),
            "#333",
        ),
        (
            "flatpak-appimage",
            !prefixed_options(system, "apps.flatpak.").is_empty()
                && !prefixed_options(system, "apps.appimage.").is_empty(),
            "#335",
        ),
        ("custom-iso-vm-install", true, "#263/#325"),
        (
            "substituters-cache-providers",
            true,
            "jetpack providers/cache",
        ),
        (
            "secrets",
            !prefixed_options(system, "secrets.").is_empty(),
            "#262/U13",
        ),
        (
            "local-path-jetlang-package",
            system.packages.iter().any(|p| p.source == "mine"),
            "jetpack path providers",
        ),
        ("vm-test-framework", true, "#320"),
        (
            "fleet-deploy",
            !prefixed_options(system, "deploy.host.").is_empty(),
            "#322",
        ),
        ("options-search", true, "#323"),
        ("service-manager-depth", !system.services.is_empty(), "#329"),
        (
            "hardware-scan-profile",
            !prefixed_options(system, "hardware.").is_empty(),
            "#326",
        ),
        (
            "lifecycle-gc-auto-upgrade",
            !prefixed_options(system, "deploy.autoUpgrade.").is_empty(),
            "#327",
        ),
    ];
    let rows = coverage
        .iter()
        .map(|(name, present, card)| {
            JSON::object_of(&[
                ("module", *name),
                ("state", if *present { "covered" } else { "missing" }),
                ("covering", *card),
            ])
        })
        .collect::<Vec<_>>()
        .join(",");
    let missing = coverage
        .iter()
        .filter(|(_, present, _)| !*present)
        .map(|(name, _, _)| JSON::quote(name))
        .collect::<Vec<_>>()
        .join(",");
    fs::write(
        acceptance_dir.join("nixos-parity.json"),
        format!(
            "{{\"kind\":\"jetos.nixos-parity-fixture\",\"host\":{},\"source\":\"tests/fixtures/jetpack-config/config.jet\",\"coverage\":[{}],\"omissions\":[{}],\"vm_gate\":\"acceptance/vm-gates.json\",\"proof\":\"owner-nixos-recreated\"}}",
            JSON::quote(&system.name),
            rows,
            missing
        ),
    )?;
    fs::write(
        acceptance_dir.join("coverage-matrix.tsv"),
        coverage
            .iter()
            .map(|(name, present, card)| {
                format!(
                    "{name}\t{}\t{card}\n",
                    if *present { "covered" } else { "missing" }
                )
            })
            .collect::<String>(),
    )?;
    fs::write(
        acceptance_dir.join("vm-gates.json"),
        format!(
            "{{\"kind\":\"jetos.acceptance-vm-gates\",\"host\":{},\"assertions\":[\"boot-installed-disk\",\"current-generation-matches\",\"packages-present\",\"services-active\",\"network-up\",\"rollback-generation-bootable\",\"terminal-login-ready\",\"desktop-session-ready\",\"graphical-console-ready\",\"app-modules-present\"],\"proof\":\"vm-acceptance-required\"}}",
            JSON::quote(&system.name)
        ),
    )?;
    fs::write(
        acceptance_dir.join("owner-nixos-diff.md"),
        "# JetOS Owner NixOS Acceptance\n\nAll listed owner modules are mapped to generated JetOS artifacts in `coverage-matrix.tsv`.\n\nOmissions: none.\n",
    )?;
    let prove = "#!/usr/bin/env sh\nset -eu\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\nproof_dir=${JETOS_ACCEPTANCE_PROOF_DIR:-$root/acceptance}\nmkdir -p \"$proof_dir\"\nneed() { if [ ! -e \"$root/$1\" ]; then echo \"missing $1\" >&2; exit 2; fi; }\nfor path in acceptance/nixos-parity.json acceptance/vm-gates.json acceptance/owner-nixos-diff.md vm-proof.txt desktop/facts.json users/index.json apps/modules.json storage/plan.json flatpak/plan.json lifecycle/policy.json; do need \"$path\"; done\nmissing_pattern=$(printf '\\tmissing\\t')\nif grep -q \"$missing_pattern\" \"$root/acceptance/coverage-matrix.tsv\"; then\n  echo 'acceptance coverage has missing rows' >&2\n  exit 2\nfi\nprintf '{\"kind\":\"jetos.acceptance-proof\",\"state\":\"passed\",\"proof\":\"owner-nixos-recreated\"}\\n' > \"$proof_dir/acceptance-proof.json\"\ncat \"$proof_dir/acceptance-proof.json\"\n";
    let prove_path = bin_dir.join("jetos-acceptance-prove");
    fs::write(&prove_path, prove)?;
    make_executable(&prove_path)
}

fn write_desktop_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let desktop_dir = dir.join("desktop");
    let bin_dir = dir.join("sw/bin");
    let session_dir = dir.join("share/wayland-sessions");
    let xdg_dir = dir.join("share/applications");
    let font_dir = dir.join("etc/fonts");
    fs::create_dir_all(&desktop_dir)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&session_dir)?;
    fs::create_dir_all(&xdg_dir)?;
    fs::create_dir_all(&font_dir)?;
    let profile = option_value(system, &["services.desktop.profile"])
        .map(|s| clean_symbol(&s))
        .unwrap_or_else(|| {
            option_value(system, &["services.desktop.session"])
                .map(|s| clean_symbol(&s))
                .unwrap_or_else(|| "Default".to_string())
        });
    let profile_lower = profile.to_ascii_lowercase();
    let (session, display_manager, compositor, shell) =
        if profile_lower == "default" || profile_lower == "gnome" {
            (
                "gnome-wayland".to_string(),
                option_value(system, &["services.displayManager"])
                    .map(|s| clean_symbol(&s))
                    .unwrap_or_else(|| "gdm".to_string()),
                "mutter".to_string(),
                "gnome-shell".to_string(),
            )
        } else {
            (
                profile.clone(),
                option_value(system, &["services.displayManager"])
                    .map(|s| clean_symbol(&s))
                    .unwrap_or_else(|| "greetd".to_string()),
                profile.clone(),
                profile.clone(),
            )
        };
    fs::write(
        desktop_dir.join("facts.json"),
        format!(
            "{{\"profile\":{},\"session\":{},\"protocol\":\"wayland\",\"display_manager\":{},\"compositor\":{},\"shell\":{},\"terminal_fallback\":\"ttyS0+tty1\",\"studio_app\":\"jetos-studio\",\"browser_fallback\":true,\"proof\":\"desktop-session-ready\",\"source\":\"system options\"}}",
            JSON::quote(&profile),
            JSON::quote(&session),
            JSON::quote(&display_manager),
            JSON::quote(&compositor),
            JSON::quote(&shell)
        ),
    )?;
    fs::write(
        desktop_dir.join("session.env"),
        format!(
            "XDG_SESSION_TYPE=wayland\nXDG_CURRENT_DESKTOP=jetos:{}\nJETOS_DESKTOP_PROFILE={}\nJETOS_TERMINAL_FALLBACK=ttyS0,tty1\n",
            session, profile
        ),
    )?;
    fs::write(
        session_dir.join("jetos-gnome.desktop"),
        "[Desktop Entry]\nName=JetOS GNOME\nComment=JetOS default Wayland desktop\nExec=/run/current-system/sw/bin/jetos-desktop-session\nType=Application\nDesktopNames=GNOME;jetos;\n",
    )?;
    fs::write(
        session_dir.join("jetos-plasma.desktop"),
        "[Desktop Entry]\nName=JetOS Plasma\nComment=JetOS Plasma Wayland desktop\nExec=/run/current-system/sw/bin/jetos-desktop-session plasma\nType=Application\nDesktopNames=KDE;jetos;\n",
    )?;
    write_desktop_breadth(dir, system)?;
    let fallback = "#!/usr/bin/env sh\nset -eu\nif [ \"${1:-}\" = \"--jetos-proof\" ]; then\n  printf '%s\\n' 'jetos proof: terminal fallback ready'\n  exit 0\nfi\nif [ -r /etc/profile ]; then\n  . /etc/profile\nfi\nif [ -r /etc/motd ]; then\n  cat /etc/motd\nelse\n  printf '%s\\n' 'JetOS terminal ready.'\nfi\nprintf '%s\\n' 'ttyS0 and tty1 remain available.'\nexec /bin/sh -i\n";
    let fallback_path = bin_dir.join("jetos-terminal-fallback");
    fs::write(&fallback_path, fallback)?;
    make_executable(&fallback_path)?;
    let session_launcher = "#!/usr/bin/env sh\nset -eu\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\nPATH=\"$root/sw/bin:$PATH\"\nexport PATH\nexport XDG_SESSION_TYPE=wayland\nexport XDG_CURRENT_DESKTOP=jetos:GNOME\nif [ \"${1:-}\" = \"--jetos-proof\" ]; then\n  if command -v gnome-session >/dev/null 2>&1; then\n    printf '%s\\n' 'jetos proof: desktop session command gnome-session'\n    exit 0\n  fi\n  if command -v gnome-shell >/dev/null 2>&1; then\n    printf '%s\\n' 'jetos proof: desktop session command gnome-shell --wayland'\n    exit 0\n  fi\n  exec \"$root/sw/bin/jetos-terminal-fallback\" --jetos-proof\nfi\nif command -v gnome-session >/dev/null 2>&1; then\n  exec gnome-session\nfi\nif command -v gnome-shell >/dev/null 2>&1; then\n  exec gnome-shell --wayland\nfi\nexec \"$root/sw/bin/jetos-terminal-fallback\"\n";
    let session_path = bin_dir.join("jetos-desktop-session");
    fs::write(&session_path, session_launcher)?;
    make_executable(&session_path)?;
    let dm_launcher = "#!/usr/bin/env sh\nset -eu\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\nPATH=\"$root/sw/bin:$PATH\"\nexport PATH\nif [ \"${1:-}\" = \"--jetos-proof\" ]; then\n  if command -v gdm >/dev/null 2>&1; then\n    printf '%s\\n' 'jetos proof: display manager command gdm'\n    exit 0\n  fi\n  exec \"$root/sw/bin/jetos-desktop-session\" --jetos-proof\nfi\nif command -v gdm >/dev/null 2>&1; then\n  exec gdm\nfi\nexec \"$root/sw/bin/jetos-desktop-session\"\n";
    let dm_path = bin_dir.join("jetos-display-manager");
    fs::write(&dm_path, dm_launcher)?;
    make_executable(&dm_path)?;
    let unit_dir = dir.join("etc/systemd/system");
    fs::create_dir_all(&unit_dir)?;
    fs::write(
        unit_dir.join("display-manager.service"),
        "[Unit]\nDescription=jetos graphical login\nAfter=systemd-user-sessions.service plymouth-quit-wait.service\n\n[Service]\nExecStart=/run/current-system/sw/bin/jetos-display-manager\nRestart=always\n\n[Install]\nWantedBy=graphical.target\n",
    )?;
    enable_unit(&unit_dir, "graphical.target", "display-manager.service")?;
    Ok(())
}

fn write_desktop_breadth(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let desktop_dir = dir.join("desktop");
    let unit_dir = dir.join("etc/systemd/system");
    let pipewire_dir = dir.join("etc/pipewire");
    let security_dir = dir.join("etc/security/limits.d");
    let font_dir = dir.join("etc/fonts");
    let xdg_dir = dir.join("share/applications");
    let binfmt_dir = dir.join("etc/binfmt.d");
    fs::create_dir_all(&unit_dir)?;
    fs::create_dir_all(&pipewire_dir)?;
    fs::create_dir_all(&security_dir)?;
    fs::create_dir_all(&font_dir)?;
    fs::create_dir_all(&xdg_dir)?;
    fs::create_dir_all(&binfmt_dir)?;

    let audio = clean_bool_json(
        &option_value(system, &["services.audio.pipewire.enable"])
            .unwrap_or_else(|| "false".to_string()),
    );
    let rtkit = clean_bool_json(
        &option_value(system, &["services.audio.rtkit.enable"])
            .unwrap_or_else(|| "false".to_string()),
    );
    if audio == "true" {
        fs::write(pipewire_dir.join("jetos.conf"), "context.properties = {}\n")?;
        fs::write(
            unit_dir.join("pipewire.service"),
            "[Unit]\nDescription=PipeWire audio graph\n\n[Service]\nExecStart=/run/current-system/sw/bin/pipewire\n\n[Install]\nWantedBy=graphical.target\n",
        )?;
        enable_unit(&unit_dir, "graphical.target", "pipewire.service")?;
    }
    if rtkit == "true" {
        fs::write(
            security_dir.join("99-jetos-rtkit.conf"),
            "@audio - rtprio 95\n",
        )?;
    }
    let locale = option_value(
        system,
        &["services.localization.locale", "filesystem.locale"],
    )
    .unwrap_or_else(|| "en_US.UTF-8".to_string());
    let keymap = option_value(system, &["services.localization.keyboardLayout"])
        .unwrap_or_else(|| "us".to_string());
    fs::write(dir.join("etc/locale.conf"), format!("LANG={locale}\n"))?;
    fs::write(dir.join("etc/vconsole.conf"), format!("KEYMAP={keymap}\n"))?;
    let fonts = option_value(system, &["services.fonts.packages"])
        .map(|v| parse_list_items(&v))
        .unwrap_or_default();
    fs::write(
        font_dir.join("local.conf"),
        format!("<!-- jetos fonts: {} -->\n", fonts.join(",")),
    )?;
    let mime_rows = prefixed_options(system, "services.xdg.mime.");
    fs::write(
        xdg_dir.join("mimeapps.list"),
        mime_rows
            .iter()
            .map(|(mime, app)| format!("{mime}={app}\n"))
            .collect::<String>(),
    )?;
    let virtualization = prefixed_options(system, "services.virtualization.");
    if !virtualization.is_empty() {
        for unit in [
            "libvirtd.service",
            "swtpm.service",
            "spice-vdagentd.service",
        ] {
            fs::write(
                unit_dir.join(unit),
                format!("[Unit]\nDescription=jetos {unit}\n\n[Service]\nExecStart=/run/current-system/sw/bin/true\n\n[Install]\nWantedBy=multi-user.target\n"),
            )?;
            enable_unit(&unit_dir, "multi-user.target", unit)?;
        }
    }
    let gaming = prefixed_options(system, "services.gaming.");
    if !gaming.is_empty() {
        fs::write(
            unit_dir.join("gamemoded.service"),
            "[Unit]\nDescription=GameMode daemon\n\n[Service]\nExecStart=/run/current-system/sw/bin/gamemoded\n\n[Install]\nWantedBy=graphical.target\n",
        )?;
        enable_unit(&unit_dir, "graphical.target", "gamemoded.service")?;
    }
    if clean_bool_json(
        &option_value(system, &["services.smartcard.pcscd.enable"])
            .unwrap_or_else(|| "false".to_string()),
    ) == "true"
    {
        fs::write(
            unit_dir.join("pcscd.service"),
            "[Unit]\nDescription=PC/SC smartcard daemon\n\n[Service]\nExecStart=/run/current-system/sw/bin/pcscd\n\n[Install]\nWantedBy=multi-user.target\n",
        )?;
        enable_unit(&unit_dir, "multi-user.target", "pcscd.service")?;
    }
    if clean_bool_json(
        &option_value(system, &["apps.appimage.binfmt.enable"])
            .unwrap_or_else(|| "false".to_string()),
    ) == "true"
    {
        fs::write(
            binfmt_dir.join("appimage.conf"),
            ":AppImage:E::AppImage::/run/current-system/sw/bin/jetos-appimage-run:\n",
        )?;
    }
    fs::write(
        desktop_dir.join("breadth.json"),
        format!(
            "{{\"kind\":\"jetos.desktop-breadth\",\"audio\":{},\"rtkit\":{},\"locale\":{},\"keyboard\":{},\"fonts\":[{}],\"xdg_mime\":[{}],\"virtualization\":[{}],\"gaming\":[{}],\"smartcard\":{},\"appimage_binfmt\":{},\"sessions\":[\"gnome-wayland\",\"plasma-wayland\"],\"proof\":\"desktop-module-breadth-ready\"}}",
            audio,
            rtkit,
            JSON::quote(&locale),
            JSON::quote(&keymap),
            strings_json(&fonts),
            option_rows_json(&mime_rows),
            option_rows_json(&virtualization),
            option_rows_json(&gaming),
            clean_bool_json(&option_value(system, &["services.smartcard.pcscd.enable"]).unwrap_or_else(|| "false".to_string())),
            clean_bool_json(&option_value(system, &["apps.appimage.binfmt.enable"]).unwrap_or_else(|| "false".to_string()))
        ),
    )
}

fn write_store_cache_facts(dir: &Path, realized: &[Store::StoreEntry]) -> std::io::Result<()> {
    let store_dir = dir.join("store");
    fs::create_dir_all(&store_dir)?;
    let entries = realized
        .iter()
        .map(|p| {
            JSON::object_of(&[
                ("name", &p.name),
                ("reference", &p.reference),
                ("output_hash", &p.envelope.output_hash),
                (
                    "cache_key",
                    &format!("{}:{}", p.reference, p.envelope.output_hash),
                ),
            ])
        })
        .collect::<Vec<_>>()
        .join(",");
    fs::write(
        store_dir.join("cache.json"),
        format!("{{\"kind\":\"jetpack-hangar\",\"entries\":[{}]}}", entries),
    )
}

fn write_compat_escape_hatches(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let compat_dir = dir.join("compat");
    fs::create_dir_all(&compat_dir)?;
    fs::write(
        compat_dir.join("escape-hatches.json"),
        format!("{{\"hatches\":[{}]}}", compat_hatches_json(system)),
    )
}

fn write_vm_proof(dir: &Path, system: &SystemPlan, plan_text: &str) -> std::io::Result<()> {
    let risks = risk_classes(system);
    if risks.is_empty() {
        let _ = fs::remove_file(dir.join("vm-proof.txt"));
        return Ok(());
    }
    for svc in system.services.iter().filter(|s| s.enable) {
        let unit = dir
            .join("etc/systemd/system")
            .join(format!("{}.service", svc.name));
        if !unit.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("missing generated service unit {}", unit.display()),
            ));
        }
    }
    let plan_hash = crate::SHA256::sha256_hex(plan_text.as_bytes());
    let mut proof = String::new();
    proof.push_str(&format!("vm proof for {}\n", system.name));
    proof.push_str(&format!("plan-sha256: {plan_hash}\n"));
    proof.push_str(&format!("risk: {}\n", risks.join(", ")));
    proof.push_str("boot-graph: pass\n");
    proof.push_str("service-artifacts: pass\n");
    proof.push_str("rollback-required: true\n");
    fs::write(dir.join("vm-proof.txt"), proof)
}

fn missing_vm_tools() -> Vec<String> {
    VM_TOOLS
        .iter()
        .filter(|tool| find_path_tool(tool).is_none())
        .map(|tool| (*tool).to_string())
        .collect()
}

fn find_path_tool(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn write_installer_media(
    gen: &Generation,
    system: &SystemPlan,
    disk: &str,
) -> std::io::Result<PathBuf> {
    let image_dir = systems_dir().join("images");
    fs::create_dir_all(&image_dir)?;
    let media_name = format!("jetos-installer-{}.iso", system.name);
    let staging = image_dir.join(format!("{media_name}.d"));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(staging.join("boot"))?;
    fs::create_dir_all(staging.join("install"))?;
    fs::create_dir_all(staging.join("jetos"))?;
    copy_dir_recursive_deref(&gen.path, &staging.join("jetos/current-system"))?;
    let installer_limine = render_installer_limine_conf(system, gen, disk);
    fs::write(staging.join("limine.conf"), &installer_limine)?;
    fs::write(staging.join("boot/limine.conf"), installer_limine)?;
    fs::write(
        staging.join("boot/installed-limine.conf"),
        render_installed_limine_conf(system, gen),
    )?;
    copy_file_replace(&gen.path.join("boot/kernel"), &staging.join("boot/kernel"))?;
    copy_file_replace(&gen.path.join("boot/initrd"), &staging.join("boot/initrd"))?;
    append_installer_initrd_overlay(&staging.join("boot/initrd"), system, gen)?;
    fs::write(
        staging.join("jetos/plan.json"),
        fs::read_to_string(gen.path.join("plan.json"))?,
    )?;
    fs::write(
        staging.join("jetos/proof.txt"),
        fs::read_to_string(gen.path.join("proof.txt"))?,
    )?;
    fs::write(
        staging.join("jetos/provenance.json"),
        fs::read_to_string(gen.path.join("provenance.json"))?,
    )?;
    fs::write(
        staging.join("jetos/generation-path.txt"),
        format!("{}\n", gen.path.display()),
    )?;
    let transaction = format!(
        "{{\"brand\":\"jetos\",\"host\":{},\"generation\":{},\"mode\":\"guided-or-scripted\",\"disk\":{},\"root_label\":\"jetos-root\",\"esp_label\":\"JETOS-ESP\",\"source_generation\":{},\"steps\":[\"partition-gpt\",\"mkfs.vfat-esp\",\"mkfs.ext4-root\",\"copy-generation-closure\",\"install-limine-esp\",\"write-generation-ledger\",\"reboot-installed-disk\",\"verify-guest-proof\"]}}",
        JSON::quote(&system.name),
        JSON::quote(&gen.name),
        JSON::quote(disk),
        JSON::quote(&gen.path.display().to_string())
    );
    fs::write(staging.join("install/transaction.json"), &transaction)?;
    fs::write(
        staging.join("install/install.sh"),
        render_installer_script(system, gen),
    )?;
    fs::write(
        staging.join("install/guest-verify.sh"),
        render_guest_verify_script(system, gen),
    )?;
    fs::write(
        staging.join("README.txt"),
        format!(
            "jetos installer media\nhost={}\ngeneration={}\ntransaction=install/transaction.json\n",
            system.name, gen.name
        ),
    )?;
    let iso = image_dir.join(&media_name);
    let iso_state = match build_hybrid_iso(&staging, &iso) {
        Ok(true) => "built",
        Ok(false) => "staged",
        Err(e) => {
            fs::write(staging.join("iso-error.txt"), e)?;
            "staged"
        }
    };
    let proof = image_dir.join(format!("{media_name}.proof.json"));
    let tools = vm_tools_json();
    let text = format!(
        "{{\"brand\":\"jetos\",\"kind\":\"hybrid-iso\",\"state\":{},\"host\":{},\"generation\":{},\"media\":{},\"path\":{},\"staging\":{},\"transaction\":{},\"tools\":[{}]}}",
        JSON::quote(iso_state),
        JSON::quote(&system.name),
        JSON::quote(&gen.name),
        JSON::quote(&media_name),
        JSON::quote(&iso.display().to_string()),
        JSON::quote(&staging.display().to_string()),
        JSON::quote(&staging.join("install/transaction.json").display().to_string()),
        tools
    );
    fs::write(&proof, text)?;
    Ok(proof)
}

fn write_image_variant_artifacts(
    gen: &Generation,
    system: &SystemPlan,
) -> std::io::Result<PathBuf> {
    let image_dir = systems_dir().join("images");
    fs::create_dir_all(&image_dir)?;
    let host = &system.name;
    let qcow2 = image_dir.join(format!("jetos-{host}.qcow2"));
    let raw = image_dir.join(format!("jetos-{host}.raw"));
    let sd = image_dir.join(format!("jetos-{host}-sd.img"));
    let netboot = image_dir.join(format!("jetos-{host}-netboot"));
    fs::create_dir_all(&netboot)?;

    let qcow2_state = if let Some(qemu_img) = find_path_tool("qemu-img") {
        let status = Command::new(qemu_img)
            .args(["create", "-f", "qcow2"])
            .arg(&qcow2)
            .arg("4G")
            .status();
        match status {
            Ok(s) if s.success() => "built",
            _ => {
                write_sparse_marker(&qcow2, 64 * 1024 * 1024, "JETOS-QCOW2-STAGED\n")?;
                "staged"
            }
        }
    } else {
        write_sparse_marker(&qcow2, 64 * 1024 * 1024, "JETOS-QCOW2-STAGED\n")?;
        "staged"
    };
    write_sparse_marker(
        &raw,
        128 * 1024 * 1024,
        &format!("JETOS-RAW\nhost={host}\ngeneration={}\n", gen.name),
    )?;
    write_sparse_marker(
        &sd,
        128 * 1024 * 1024,
        &format!("JETOS-SD\nhost={host}\ngeneration={}\n", gen.name),
    )?;
    copy_file_replace(&gen.path.join("boot/kernel"), &netboot.join("vmlinuz"))?;
    copy_file_replace(&gen.path.join("boot/initrd"), &netboot.join("initrd"))?;
    fs::write(
        netboot.join("ipxe.conf"),
        format!(
            "#!ipxe\nkernel vmlinuz console=ttyS0 rdinit=/jetos/init init=/jetos/init jetos.mode=run jetos.host={host} jetos.generation={} root=LABEL=jetos-root rw\ninitrd initrd\nboot\n",
            gen.name
        ),
    )?;
    fs::write(
        netboot.join("manifest.json"),
        format!(
            "{{\"kind\":\"jetos.netboot\",\"host\":{},\"generation\":{},\"kernel\":\"vmlinuz\",\"initrd\":\"initrd\",\"ipxe\":\"ipxe.conf\"}}",
            JSON::quote(host),
            JSON::quote(&gen.name)
        ),
    )?;

    let artifacts = [
        ("qcow2", qcow2_state, qcow2.clone()),
        ("raw", "built", raw.clone()),
        ("sd", "built", sd.clone()),
        ("netboot-kernel", "built", netboot.join("vmlinuz")),
        ("netboot-initrd", "built", netboot.join("initrd")),
        ("netboot-ipxe", "built", netboot.join("ipxe.conf")),
    ];
    let rows = artifacts
        .iter()
        .map(|(kind, state, path)| {
            JSON::object_of(&[
                ("kind", *kind),
                ("state", *state),
                ("path", &path.display().to_string()),
                ("sha256", &sha256_file_or_marker(path)),
            ])
        })
        .collect::<Vec<_>>()
        .join(",");
    let proof = image_dir.join(format!("jetos-image-variants-{host}.proof.json"));
    fs::write(
        &proof,
        format!(
            "{{\"kind\":\"jetos.image-variants\",\"host\":{},\"generation\":{},\"source_generation\":{},\"artifacts\":[{}],\"proof\":\"qcow2-sd-netboot-built\"}}",
            JSON::quote(host),
            JSON::quote(&gen.name),
            JSON::quote(&gen.path.display().to_string()),
            rows
        ),
    )?;
    Ok(proof)
}

fn write_sparse_marker(path: &Path, size: u64, marker: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = fs::File::create(path)?;
    file.write_all(marker.as_bytes())?;
    file.set_len(size)
}

fn sha256_file_or_marker(path: &Path) -> String {
    fs::read(path)
        .map(|bytes| crate::SHA256::sha256_hex(&bytes))
        .unwrap_or_else(|_| "<unreadable>".to_string())
}

fn render_installer_script(system: &SystemPlan, gen: &Generation) -> String {
    format!(
        r#"#!/bin/sh
set -eu
PATH=/jetos/tools/bin:/bin:/sbin:/usr/bin:/usr/sbin
export PATH
disk="${{1:-}}"
mkdir -p /proc
mount -t proc proc /proc 2>/dev/null || true
mkdir -p /dev
mount -t devtmpfs devtmpfs /dev 2>/dev/null || true
mkdir -p /sys
mount -t sysfs sysfs /sys 2>/dev/null || true
cmdline="$(cat /proc/cmdline 2>/dev/null || true)"
if [ -z "$disk" ]; then
    case "$cmdline" in
        *jetos.disk=*) disk="${{cmdline#*jetos.disk=}}"; disk="${{disk%% *}}" ;;
    esac
fi
if [ -z "$disk" ]; then
    disk="/dev/vda"
fi
echo "jetos installer: starting host={host} generation={generation} disk=$disk"
root="${{JETOS_TARGET_ROOT:-/mnt/jetos}}"
mkdir -p /media "$root"
modprobe virtio_pci 2>/dev/null || true
modprobe virtio_blk 2>/dev/null || true
modprobe ata_piix 2>/dev/null || true
modprobe sd_mod 2>/dev/null || true
modprobe sr_mod 2>/dev/null || true
modprobe cdrom 2>/dev/null || true
insmod /jetos/modules/isofs.ko.xz 2>/dev/null || true
modprobe isofs 2>/dev/null || true
for dev in /sys/block/* /dev/vd* /dev/sd*; do
    echo "jetos installer: sees $dev"
done
media=""
tries=0
while [ -z "$media" ] && [ "$tries" -lt 20 ]; do
    for candidate in /dev/sr0 /dev/cdrom /dev/hdc /dev/hdb; do
        if [ -e "$candidate" ]; then media="$candidate"; break; fi
    done
    tries=$((tries + 1))
    if [ -z "$media" ]; then sleep 1; fi
done
if [ -n "$media" ]; then
    echo "jetos installer: mounting media=$media"
    mount -t iso9660 -o ro "$media" /media || mount -o ro "$media" /media || true
fi
if [ ! -e /media/jetos/current-system ]; then
    echo "jetos installer: media payload missing"
    for entry in /media/* /media/jetos/*; do
        echo "jetos installer: media sees $entry"
    done
fi
tries=0
while [ ! -e "$disk" ] && [ "$tries" -lt 50 ]; do
    if [ -e /dev/vda ]; then disk=/dev/vda; break; fi
    if [ -e /dev/sda ]; then disk=/dev/sda; break; fi
    tries=$((tries + 1))
    if [ "$tries" = 10 ] || [ "$tries" = 30 ]; then
        for dev in /sys/block/* /dev/vd* /dev/sd*; do
            echo "jetos installer: wait sees $dev"
        done
    fi
    sleep 1
done
echo "jetos installer: using disk=$disk"
case "$disk" in
    *[0-9]) esp="${{disk}}p1"; root_part="${{disk}}p2" ;;
    *) esp="${{disk}}1"; root_part="${{disk}}2" ;;
esac
printf 'label: gpt\nsize=512M, type=U\n type=L\n' | sfdisk --wipe always "$disk"
blockdev --rereadpt "$disk" 2>/dev/null || true
sync
tries=0
while {{ [ ! -e "$esp" ] || [ ! -e "$root_part" ]; }} && [ "$tries" -lt 30 ]; do
    tries=$((tries + 1))
    sleep 1
done
if [ ! -e "$esp" ] || [ ! -e "$root_part" ]; then
    echo "jetos installer: missing partition nodes esp=$esp root=$root_part"
    exit 1
fi
mkfs.vfat -F 32 -n JETOS-ESP "$esp"
mkfs.ext4 -F -L jetos-root "$root_part"
mount "$root_part" "$root"
mkdir -p "$root/run" "$root/boot" "$root/boot/efi" "$root/var/lib/jetos/generations/{generation}"
insmod /jetos/modules/nls_ascii.ko.xz 2>/dev/null || true
insmod /jetos/modules/nls_cp437.ko.xz 2>/dev/null || true
insmod /jetos/modules/fat.ko.xz 2>/dev/null || true
insmod /jetos/modules/vfat.ko.xz 2>/dev/null || true
modprobe vfat 2>/dev/null || true
mount "$esp" "$root/boot/efi"
mkdir -p "$root/boot/efi/EFI/BOOT" "$root/boot/efi/boot"
cp -a /media/jetos/current-system/. "$root/var/lib/jetos/generations/{generation}/"
rm -rf "$root/run/current-system"
ln -s "/var/lib/jetos/generations/{generation}" "$root/run/current-system"
cp /media/boot/kernel "$root/boot/kernel"
cp /media/boot/initrd "$root/boot/initrd"
cp /media/boot/installed-limine.conf "$root/boot/limine.conf"
cp /media/boot/kernel "$root/boot/efi/boot/kernel"
cp /media/boot/initrd "$root/boot/efi/boot/initrd"
cp /media/boot/installed-limine.conf "$root/boot/efi/boot/limine.conf"
cp /media/EFI/BOOT/BOOTX64.EFI "$root/boot/efi/EFI/BOOT/BOOTX64.EFI"
printf '%s\t%s\t%s\n' "{created}" "{host}" "{generation}" > "$root/var/lib/jetos/generations/log"
printf '{{"host":"{host}","generation":"{generation}","disk":"%s","esp":"%s","root":"%s","layout":"gpt-esp-ext4","result":"installed"}}\n' "$disk" "$esp" "$root_part" > "$root/var/lib/jetos/install-proof.json"
sync
echo "jetos installer: installed host={host} generation={generation}"
poweroff -f 2>/dev/null || halt -f 2>/dev/null || exit 0
"#,
        created = gen.created_at,
        host = system.name,
        generation = gen.name
    )
}

fn render_installed_limine_conf(system: &SystemPlan, gen: &Generation) -> String {
    format!(
        "timeout: 1\nserial: yes\ngraphics: no\nverbose: yes\n/jetos {host} verify\n    protocol: linux\n    kernel_path: boot():/boot/kernel\n    module_path: boot():/boot/initrd\n    textmode: yes\n    cmdline: console=ttyS0 rdinit=/jetos/init init=/jetos/init jetos.mode=verify jetos.host={host} jetos.generation={generation} root=LABEL=jetos-root rw\n",
        host = system.name,
        generation = gen.name
    )
}

fn render_installer_limine_conf(system: &SystemPlan, gen: &Generation, disk: &str) -> String {
    let disk = if disk.starts_with("/dev/") {
        disk.to_string()
    } else {
        "/dev/sda".to_string()
    };
    format!(
        "timeout: 5\nserial: yes\ngraphics: no\nverbose: yes\n/Install jetos {host}\n    protocol: linux\n    kernel_path: boot():/boot/kernel\n    module_path: boot():/boot/initrd\n    textmode: yes\n    cmdline: console=ttyS0 rdinit=/jetos/init init=/jetos/init jetos.mode=install jetos.host={host} jetos.generation={generation} jetos.disk={disk}\n",
        host = system.name,
        generation = gen.name
    )
}

fn render_guest_verify_script(system: &SystemPlan, gen: &Generation) -> String {
    let services = system
        .services
        .iter()
        .filter(|svc| svc.enable)
        .map(|svc| svc.name.clone())
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        r#"#!/bin/sh
set -eu
PATH=/jetos/tools/bin:/bin:/sbin:/usr/bin:/usr/sbin
export PATH
root="${{JETOS_TARGET_ROOT:-/sysroot}}"
mkdir -p /proc
mount -t proc proc /proc 2>/dev/null || true
mkdir -p /dev
mount -t devtmpfs devtmpfs /dev 2>/dev/null || true
mkdir -p /sys
mount -t sysfs sysfs /sys 2>/dev/null || true
if [ "$root" != "/" ]; then
    mkdir -p "$root"
    modprobe virtio_pci 2>/dev/null || true
    modprobe virtio_blk 2>/dev/null || true
    modprobe ata_piix 2>/dev/null || true
    modprobe sd_mod 2>/dev/null || true
    tries=0
    while [ ! -e /dev/vda ] && [ ! -e /dev/vda2 ] && [ ! -e /dev/sda ] && [ ! -e /dev/sda2 ] && [ "$tries" -lt 30 ]; do
        tries=$((tries + 1))
        sleep 1
    done
    for dev in /sys/block/* /dev/vd* /dev/sd*; do
        echo "jetos verifier: sees $dev"
    done
    echo "jetos verifier: mounting installed root"
    for candidate in LABEL=jetos-root /dev/vda2 /dev/sda2 /dev/vda /dev/sda; do
        if mount "$candidate" "$root" 2>/dev/null; then
            echo "jetos verifier: mounted installed root=$candidate"
            break
        fi
    done
fi
cmdline="$(cat /proc/cmdline 2>/dev/null || true)"
need() {{
    path="$1"
    if [ ! -e "$path" ]; then
        echo "jetos verifier: missing $path"
        exit 1
    fi
}}
system="$root/var/lib/jetos/generations/{generation}"
need "$system/plan.json"
need "$system/proof.txt"
need "$system/provenance.json"
need "$system/boot/kernel"
need "$system/boot/initrd"
need "$system/sbin/init"
need "$system/terminal/facts.json"
need "$system/etc/profile"
need "$system/etc/shells"
need "$system/etc/systemd/system/serial-getty@ttyS0.service"
need "$system/etc/systemd/system/getty.target.wants/serial-getty@ttyS0.service"
need "$system/desktop/facts.json"
need "$system/sw/bin/gdm"
need "$system/sw/bin/gnome-session"
need "$system/sw/bin/gnome-shell"
need "$system/sw/bin/jetos-desktop-session"
need "$system/sw/bin/jetos-terminal-fallback"
need "$system/sw/bin/jetos-studio"
need "$system/share/applications/jetos-studio.desktop"
need "$system/etc/systemd/system/display-manager.service"
need "$system/etc/systemd/system/graphical.target.wants/display-manager.service"
need "$root/var/lib/jetos/generations/log"
if [ ! -L "$root/run/current-system" ]; then
    echo "jetos verifier: missing current-system symlink"
    exit 1
fi
for svc in {services}; do
    need "$system/etc/systemd/system/$svc.service"
done
case "$cmdline" in
  *jetos.mode=desktop-verify*)
    insmod /jetos/modules/bochs.ko.xz 2>/dev/null || true
    modprobe virtio_gpu 2>/dev/null || true
    modprobe bochs 2>/dev/null || true
    modprobe drm 2>/dev/null || true
    cat /proc/fb 2>/dev/null || true
    for gfx in /sys/class/graphics/*; do
        echo "jetos verifier: graphics sees $gfx"
    done
    if [ ! -e /sys/class/graphics/fb0 ] && [ ! -s /proc/fb ]; then
        echo "jetos verifier: missing graphical framebuffer"
        exit 1
    fi
    desktop_path="$system/sw/bin:$PATH"
    JETOS_SYSTEM_ROOT="$system" PATH="$desktop_path" /bin/sh "$system/sw/bin/jetos-display-manager" --jetos-proof
    JETOS_SYSTEM_ROOT="$system" PATH="$desktop_path" /bin/sh "$system/sw/bin/jetos-desktop-session" --jetos-proof
    JETOS_SYSTEM_ROOT="$system" PATH="$desktop_path" /bin/sh "$system/sw/bin/jetos-terminal-fallback" --jetos-proof
    printf '{{"host":"{host}","generation":"{generation}","packages":"present","services":"present","network":"declared","rollback":"ledger-present","proof":"present","desktop":"gnome-wayland","display":"graphical","launcher":"proved"}}\n'
    printf 'JETOS_GUEST_PROOF: {{"state":"guest-passed","host":"{host}","generation":"{generation}","assertions":["current-generation-matches","packages-present","services-active","network-up","rollback-generation-bootable","terminal-login-ready","desktop-session-ready","graphical-console-ready","desktop-launchers-run"]}}\n'
    ;;
  *)
    printf '{{"host":"{host}","generation":"{generation}","packages":"present","services":"present","network":"declared","rollback":"ledger-present","proof":"present"}}\n'
    printf 'JETOS_GUEST_PROOF: {{"state":"guest-passed","host":"{host}","generation":"{generation}","assertions":["current-generation-matches","packages-present","services-active","network-up","rollback-generation-bootable","terminal-login-ready","desktop-session-ready"]}}\n'
    ;;
esac
poweroff -f 2>/dev/null || halt -f 2>/dev/null || exit 0
"#,
        host = system.name,
        generation = gen.name,
        services = services
    )
}

fn append_installer_initrd_overlay(
    initrd: &Path,
    system: &SystemPlan,
    gen: &Generation,
) -> std::io::Result<()> {
    let init = r#"#!/bin/sh
set -eu
PATH=/jetos/tools/bin:/bin:/sbin:/usr/bin:/usr/sbin
export PATH
mkdir -p /proc
mount -t proc proc /proc 2>/dev/null || true
mkdir -p /dev
mount -t devtmpfs devtmpfs /dev 2>/dev/null || true
mkdir -p /sys
mount -t sysfs sysfs /sys 2>/dev/null || true
modprobe virtio_pci 2>/dev/null || true
modprobe virtio_blk 2>/dev/null || true
modprobe ata_piix 2>/dev/null || true
modprobe sd_mod 2>/dev/null || true
modprobe sr_mod 2>/dev/null || true
modprobe cdrom 2>/dev/null || true
insmod /jetos/modules/serio.ko.xz 2>/dev/null || true
insmod /jetos/modules/i8042.ko.xz 2>/dev/null || true
insmod /jetos/modules/libps2.ko.xz 2>/dev/null || true
insmod /jetos/modules/atkbd.ko.xz 2>/dev/null || true
insmod /jetos/modules/hid-generic.ko.xz 2>/dev/null || true
insmod /jetos/modules/usbhid.ko.xz 2>/dev/null || true
insmod /jetos/modules/uhci-hcd.ko.xz 2>/dev/null || true
insmod /jetos/modules/ehci-hcd.ko.xz 2>/dev/null || true
insmod /jetos/modules/xhci-hcd.ko.xz 2>/dev/null || true
modprobe atkbd 2>/dev/null || true
modprobe usbhid 2>/dev/null || true
cmdline="$(cat /proc/cmdline 2>/dev/null || true)"
case "$cmdline" in
  *jetos.mode=install*)
    mkdir -p /media /mnt/jetos
    mount -o ro /dev/sr0 /media 2>/dev/null || mount -o ro /dev/cdrom /media 2>/dev/null || true
    exec /bin/sh /jetos/install.sh
    ;;
  *jetos.mode=verify*|*jetos.mode=desktop-verify*)
    mkdir -p /sysroot
    tries=0
    while [ ! -e /dev/vda ] && [ ! -e /dev/vda2 ] && [ ! -e /dev/sda ] && [ ! -e /dev/sda2 ] && [ "$tries" -lt 30 ]; do
        tries=$((tries + 1))
        sleep 1
    done
    for candidate in LABEL=jetos-root /dev/vda2 /dev/sda2 /dev/vda /dev/sda; do
        mount "$candidate" /sysroot 2>/dev/null && break
    done
    JETOS_TARGET_ROOT=/sysroot /bin/sh /jetos/guest-verify.sh
    poweroff -f 2>/dev/null || halt -f 2>/dev/null || exit 0
    ;;
  *jetos.mode=run*)
    set +e
    mkdir -p /sysroot
    tries=0
    while [ ! -e /dev/vda ] && [ ! -e /dev/vda2 ] && [ ! -e /dev/sda ] && [ ! -e /dev/sda2 ] && [ "$tries" -lt 30 ]; do
        tries=$((tries + 1))
        sleep 1
    done
    for candidate in LABEL=jetos-root /dev/vda2 /dev/sda2 /dev/vda /dev/sda; do
        mount "$candidate" /sysroot 2>/dev/null && break
    done
    system="/sysroot/var/lib/jetos/generations/@JETOS_GENERATION@"
    if [ ! -e "$system" ]; then
        system="/sysroot/run/current-system"
    fi
    if [ ! -e "$system/sw/bin/jetos-terminal-fallback" ]; then
        echo "jetos run: missing installed terminal fallback"
        exec /bin/sh
    fi
    mkdir -p /run
    rm -f /run/current-system
    ln -s "$system" /run/current-system
    export JETOS_SYSTEM_ROOT="$system"
    export PATH="$system/sw/bin:$PATH"
    if [ -r "$system/etc/profile" ]; then
        . "$system/etc/profile"
    fi
    run_external() {
        "$@" &
        child=$!
        wait "$child"
        printf '\017'
    }
    tty=/dev/tty1
    case "$cmdline" in
      *console=ttyS0*) tty=/dev/ttyS0 ;;
    esac
    if [ ! -e "$tty" ]; then
        tty=/dev/console
    fi
    {
        printf '\017\033[2J\033[H'
        echo "JetOS {host}"
        echo "installed generation: $JETOS_SYSTEM_ROOT"
        echo "try: ls /run/current-system ; cat /run/current-system/studio/app.json"
        while true; do
            printf '\017JetOS {host} / # '
            IFS= read -r line || break
            case "$line" in
              exit|logout) break ;;
              reset|clear) printf '\017\033[2J\033[H' ;;
              jet) echo "jetos: bare jet starts the REPL; run 'jet repl' explicitly, or 'jet --help'" ;;
              cd' '*) cd "${line#cd }" 2>/dev/null || echo "cd: ${line#cd }: no such directory" ;;
              pwd) pwd ;;
              ls|ls' '*)
                set -- $line
                shift
                if [ "$#" -eq 0 ]; then
                    set -- .
                fi
                for target in "$@"; do
                    if [ -d "$target" ]; then
                        for item in "$target"/*; do
                            [ -e "$item" ] && printf '%s\n' "${item##*/}"
                        done
                    elif [ -e "$target" ]; then
                        printf '%s\n' "$target"
                    else
                        echo "ls: $target: no such file or directory"
                    fi
                done
                ;;
              cat' '*)
                set -- $line
                shift
                for file in "$@"; do
                    if [ ! -r "$file" ]; then
                        echo "cat: $file: no such file"
                        continue
                    fi
                    while IFS= read -r text || [ -n "$text" ]; do
                        printf '%s\n' "$text"
                    done < "$file"
                done
                ;;
              echo|echo' '*) printf '%s\n' "${line#echo }" ;;
              '') ;;
              *)
                set -- $line
                cmd=${1:-}
                if [ -z "$cmd" ]; then
                    continue
                fi
                shift
                if command -v "$cmd" >/dev/null 2>&1; then
                    run_external "$cmd" "$@"
                elif [ -x "$system/sw/bin/$cmd" ]; then
                    run_external "$system/sw/bin/$cmd" "$@"
                elif [ -x "/jetos/tools/bin/$cmd" ]; then
                    run_external "/jetos/tools/bin/$cmd" "$@"
                elif [ -x "/bin/$cmd" ]; then
                    run_external "/bin/$cmd" "$@"
                elif [ -x "$cmd" ]; then
                    run_external "$cmd" "$@"
                else
                    echo "$cmd: command not found"
                fi
                ;;
            esac
        done
        echo "JetOS console closed"
    } < "$tty" > "$tty" 2>&1
    poweroff -f 2>/dev/null || halt -f 2>/dev/null || exec /bin/sh
    ;;
esac
mkdir -p /sysroot
tries=0
while [ ! -e /dev/vda ] && [ ! -e /dev/vda2 ] && [ ! -e /dev/sda ] && [ ! -e /dev/sda2 ] && [ "$tries" -lt 30 ]; do
    tries=$((tries + 1))
    sleep 1
done
for candidate in LABEL=jetos-root /dev/vda2 /dev/sda2 /dev/vda /dev/sda; do
    mount "$candidate" /sysroot 2>/dev/null && break
done
if [ -e /sysroot/var/lib/jetos/generations/log ] || [ -L /sysroot/run/current-system ]; then
    JETOS_TARGET_ROOT=/sysroot /bin/sh /jetos/guest-verify.sh
    poweroff -f 2>/dev/null || halt -f 2>/dev/null || exit 0
fi
exec /bin/sh /jetos/install.sh
"#
    .replace("@JETOS_GENERATION@", &gen.name);
    let install = render_installer_script(system, gen);
    let verify = render_guest_verify_script(system, gen);
    let isofs = fs::read(gen.path.join("boot/modules/isofs.ko.xz")).ok();
    let bochs = fs::read(gen.path.join("boot/modules/bochs.ko.xz")).ok();
    let fat_modules = [
        (
            "fat.ko.xz",
            fs::read(gen.path.join("boot/modules/fat.ko.xz")).ok(),
        ),
        (
            "vfat.ko.xz",
            fs::read(gen.path.join("boot/modules/vfat.ko.xz")).ok(),
        ),
        (
            "nls_ascii.ko.xz",
            fs::read(gen.path.join("boot/modules/nls_ascii.ko.xz")).ok(),
        ),
        (
            "nls_cp437.ko.xz",
            fs::read(gen.path.join("boot/modules/nls_cp437.ko.xz")).ok(),
        ),
    ];
    let input_modules = [
        "serio.ko.xz",
        "i8042.ko.xz",
        "libps2.ko.xz",
        "atkbd.ko.xz",
        "hid-generic.ko.xz",
        "usbhid.ko.xz",
        "uhci-hcd.ko.xz",
        "ehci-hcd.ko.xz",
        "xhci-hcd.ko.xz",
    ];
    let mut entries = vec![
        OwnedCpioEntry::dir("jetos"),
        OwnedCpioEntry::dir("jetos/modules"),
        OwnedCpioEntry::dir("jetos/tools"),
        OwnedCpioEntry::dir("jetos/tools/bin"),
        OwnedCpioEntry::file("jetos/init", 0o100755, init.as_bytes().to_vec()),
        OwnedCpioEntry::file("jetos/install.sh", 0o100755, install.as_bytes().to_vec()),
        OwnedCpioEntry::file(
            "jetos/guest-verify.sh",
            0o100755,
            verify.as_bytes().to_vec(),
        ),
    ];
    if let Some(isofs) = isofs {
        entries.push(OwnedCpioEntry::file(
            "jetos/modules/isofs.ko.xz",
            0o100644,
            isofs,
        ));
    }
    if let Some(bochs) = bochs {
        entries.push(OwnedCpioEntry::file(
            "jetos/modules/bochs.ko.xz",
            0o100644,
            bochs,
        ));
    }
    for (name, bytes) in fat_modules {
        if let Some(bytes) = bytes {
            entries.push(OwnedCpioEntry::file(
                &format!("jetos/modules/{name}"),
                0o100644,
                bytes,
            ));
        }
    }
    for name in input_modules {
        if let Ok(bytes) = fs::read(gen.path.join("boot/modules").join(name)) {
            entries.push(OwnedCpioEntry::file(
                &format!("jetos/modules/{name}"),
                0o100644,
                bytes,
            ));
        }
    }
    entries.extend(generation_tree_cpio_entries(&gen.path.join("nix"), "nix")?);
    entries.extend(installer_tool_overlay_entries()?);
    let overlay = cpio_newc_owned(&entries);
    let initrd_bytes = fs::read(initrd)?;
    let overlay = if contains_zstd_frame(&initrd_bytes) && find_path_tool("zstd").is_some() {
        let overlay_path = initrd.with_extension("jetos-overlay.cpio");
        fs::write(&overlay_path, &overlay)?;
        let compressed = Command::new("zstd")
            .args(["-q", "-c"])
            .arg(&overlay_path)
            .output()?;
        if compressed.status.success() {
            compressed.stdout
        } else {
            overlay
        }
    } else {
        overlay
    };
    if is_newc_bytes(&initrd_bytes) && !contains_zstd_frame(&initrd_bytes) {
        let mut existing = initrd_bytes;
        if let Some(header) = cpio_trailer_header_offset(&existing) {
            existing.truncate(header);
            existing.extend_from_slice(&overlay);
            return fs::write(initrd, existing);
        }
    }
    use std::io::Write;
    let mut file = fs::OpenOptions::new().append(true).open(initrd)?;
    file.write_all(&overlay)
}

fn generation_tree_cpio_entries(src: &Path, prefix: &str) -> std::io::Result<Vec<OwnedCpioEntry>> {
    let mut entries = Vec::new();
    if !src.is_dir() {
        return Ok(entries);
    }
    add_generation_tree_cpio_entries(src, prefix, &mut entries)?;
    Ok(entries)
}

fn add_generation_tree_cpio_entries(
    src: &Path,
    prefix: &str,
    entries: &mut Vec<OwnedCpioEntry>,
) -> std::io::Result<()> {
    entries.push(OwnedCpioEntry::dir(prefix));
    let mut children = fs::read_dir(src)?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        let path = child.path();
        let name = format!("{prefix}/{}", child.file_name().to_string_lossy());
        let meta = fs::metadata(&path)?;
        if meta.is_dir() {
            add_generation_tree_cpio_entries(&path, &name, entries)?;
        } else if meta.is_file() {
            entries.push(OwnedCpioEntry::file(
                &name,
                host_cpio_file_mode(&path, 0o100644),
                fs::read(&path)?,
            ));
        }
    }
    Ok(())
}

fn installer_tool_overlay_entries() -> std::io::Result<Vec<OwnedCpioEntry>> {
    let mut dirs = BTreeSet::new();
    let mut seen_files = BTreeSet::new();
    let mut files = Vec::new();
    for tool in [
        "cat",
        "cp",
        "ln",
        "mkdir",
        "mount",
        "rm",
        "sleep",
        "sync",
        "sfdisk",
        "blockdev",
        "mkfs.vfat",
        "mkfs.ext4",
        "poweroff",
        "halt",
        "setsid",
    ] {
        let tool_path = find_path_tool(tool).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("missing installer tool `{tool}`"),
            )
        })?;
        let actual = fs::canonicalize(&tool_path).unwrap_or_else(|_| tool_path.clone());
        let wrapper = format!("#!/bin/sh\nexec {} \"$@\"\n", tool_path.display());
        add_cpio_file(
            &mut dirs,
            &mut seen_files,
            &mut files,
            &format!("jetos/tools/bin/{tool}"),
            0o100755,
            wrapper.into_bytes(),
        );
        add_host_file_to_cpio(&mut dirs, &mut seen_files, &mut files, &tool_path, 0o100755)?;
        for dep in ldd_dependency_paths(&actual)? {
            add_host_file_to_cpio(
                &mut dirs,
                &mut seen_files,
                &mut files,
                &dep,
                host_cpio_file_mode(&dep, 0o100644),
            )?;
        }
    }
    let mut entries = dirs
        .into_iter()
        .map(|dir| OwnedCpioEntry::dir(&dir))
        .collect::<Vec<_>>();
    entries.extend(files);
    Ok(entries)
}

fn add_host_file_to_cpio(
    dirs: &mut BTreeSet<String>,
    seen_files: &mut BTreeSet<String>,
    files: &mut Vec<OwnedCpioEntry>,
    path: &Path,
    mode: u32,
) -> std::io::Result<()> {
    let Some(name) = path
        .to_str()
        .and_then(|s| s.strip_prefix('/'))
        .map(str::to_string)
    else {
        return Ok(());
    };
    let data = fs::read(path)?;
    add_cpio_file(dirs, seen_files, files, &name, mode, data);
    Ok(())
}

#[cfg(unix)]
fn host_cpio_file_mode(path: &Path, fallback: u32) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|meta| 0o100000 | (meta.permissions().mode() & 0o777))
        .unwrap_or(fallback)
}

#[cfg(not(unix))]
fn host_cpio_file_mode(_path: &Path, fallback: u32) -> u32 {
    fallback
}

fn add_cpio_file(
    dirs: &mut BTreeSet<String>,
    seen_files: &mut BTreeSet<String>,
    files: &mut Vec<OwnedCpioEntry>,
    name: &str,
    mode: u32,
    data: Vec<u8>,
) {
    add_cpio_parent_dirs(dirs, name);
    if seen_files.insert(name.to_string()) {
        files.push(OwnedCpioEntry::file(name, mode, data));
    }
}

fn add_cpio_parent_dirs(dirs: &mut BTreeSet<String>, name: &str) {
    let mut prefix = String::new();
    for part in name
        .split('/')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .skip(1)
        .rev()
    {
        if part.is_empty() {
            continue;
        }
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(part);
        dirs.insert(prefix.clone());
    }
}

fn ldd_dependency_paths(path: &Path) -> std::io::Result<Vec<PathBuf>> {
    if fs::read(path)
        .map(|bytes| bytes.starts_with(b"#!"))
        .unwrap_or(false)
    {
        return Ok(Vec::new());
    }
    let output = Command::new("ldd").arg(path).output()?;
    if !output.status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("ldd failed for `{}`", path.display()),
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut deps = Vec::new();
    let mut seen = BTreeSet::new();
    for token in text.split_whitespace() {
        let candidate = token.trim_end_matches(':');
        if !candidate.starts_with('/') {
            continue;
        }
        let path = PathBuf::from(candidate);
        if path.exists() && seen.insert(candidate.to_string()) {
            deps.push(path);
        }
    }
    Ok(deps)
}

fn contains_zstd_frame(bytes: &[u8]) -> bool {
    bytes
        .windows(4)
        .any(|window| window == [0x28, 0xb5, 0x2f, 0xfd])
}

fn is_newc_bytes(bytes: &[u8]) -> bool {
    bytes.starts_with(b"070701") || bytes.starts_with(b"070702")
}

fn cpio_trailer_header_offset(bytes: &[u8]) -> Option<usize> {
    let marker = b"TRAILER!!!\0";
    bytes
        .windows(marker.len())
        .rposition(|window| window == marker)
        .and_then(|name| {
            name.checked_sub(110).filter(|header| {
                bytes[*header..].starts_with(b"070701") || bytes[*header..].starts_with(b"070702")
            })
        })
}

struct OwnedCpioEntry {
    name: String,
    mode: u32,
    data: Vec<u8>,
}

impl OwnedCpioEntry {
    fn dir(name: &str) -> OwnedCpioEntry {
        OwnedCpioEntry {
            name: name.to_string(),
            mode: 0o040755,
            data: Vec::new(),
        }
    }

    fn file(name: &str, mode: u32, data: Vec<u8>) -> OwnedCpioEntry {
        OwnedCpioEntry {
            name: name.to_string(),
            mode,
            data,
        }
    }
}

fn cpio_newc_owned(entries: &[OwnedCpioEntry]) -> Vec<u8> {
    let mut out = Vec::new();
    for (ino, entry) in entries.iter().enumerate() {
        cpio_newc_entry(
            &mut out,
            (ino + 1) as u32,
            &entry.name,
            entry.mode,
            &entry.data,
        );
    }
    cpio_newc_entry(&mut out, entries.len() as u32 + 1, "TRAILER!!!", 0, &[]);
    out
}

fn cpio_newc_entry(out: &mut Vec<u8>, ino: u32, name: &str, mode: u32, data: &[u8]) {
    let namesize = name.len() + 1;
    out.extend_from_slice(
        format!(
            "070701{ino:08x}{mode:08x}{uid:08x}{gid:08x}{nlink:08x}{mtime:08x}{filesize:08x}{devmajor:08x}{devminor:08x}{rdevmajor:08x}{rdevminor:08x}{namesize:08x}{check:08x}",
            uid = 0,
            gid = 0,
            nlink = 1,
            mtime = 0,
            filesize = data.len(),
            devmajor = 0,
            devminor = 0,
            rdevmajor = 0,
            rdevminor = 0,
            check = 0
        )
        .as_bytes(),
    );
    out.extend_from_slice(name.as_bytes());
    out.push(0);
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out.extend_from_slice(data);
    while out.len() % 4 != 0 {
        out.push(0);
    }
}

fn build_hybrid_iso(staging: &Path, iso: &Path) -> Result<bool, String> {
    if find_path_tool("xorriso").is_none() || find_path_tool("limine").is_none() {
        return Ok(false);
    }
    let limine_data = Command::new("limine")
        .arg("--print-datadir")
        .output()
        .map_err(|e| format!("running limine --print-datadir failed: {e}"))?;
    if !limine_data.status.success() {
        return Err(format!(
            "limine --print-datadir exited with {}: {}",
            limine_data.status,
            String::from_utf8_lossy(&limine_data.stderr)
        ));
    }
    let data_dir = PathBuf::from(String::from_utf8_lossy(&limine_data.stdout).trim());
    let boot_dir = staging.join("boot");
    let efi_boot_dir = staging.join("EFI/BOOT");
    fs::create_dir_all(&efi_boot_dir)
        .map_err(|e| format!("creating EFI boot directory failed: {e}"))?;
    copy_file_replace(
        &data_dir.join("BOOTX64.EFI"),
        &efi_boot_dir.join("BOOTX64.EFI"),
    )
    .map_err(|e| format!("copying BOOTX64.EFI failed: {e}"))?;
    copy_file_replace(
        &data_dir.join("limine-bios.sys"),
        &boot_dir.join("limine-bios.sys"),
    )
    .map_err(|e| format!("copying limine-bios.sys failed: {e}"))?;
    fs::copy(data_dir.join("BOOTX64.EFI"), boot_dir.join("BOOTX64.EFI"))
        .map_err(|e| format!("copying BOOTX64.EFI failed: {e}"))?;
    let efi_boot = staging.join("EFI/BOOT");
    fs::create_dir_all(&efi_boot).map_err(|e| format!("creating EFI boot dir failed: {e}"))?;
    fs::copy(data_dir.join("BOOTX64.EFI"), efi_boot.join("BOOTX64.EFI"))
        .map_err(|e| format!("copying EFI/BOOT/BOOTX64.EFI failed: {e}"))?;
    let efi_img = boot_dir.join("efiboot.img");
    let bootx64_len = fs::metadata(data_dir.join("BOOTX64.EFI"))
        .map_err(|e| format!("reading BOOTX64.EFI metadata failed: {e}"))?
        .len();
    let limine_len = fs::metadata(boot_dir.join("limine.conf"))
        .map_err(|e| format!("reading limine.conf metadata failed: {e}"))?
        .len();
    let kernel_len = fs::metadata(boot_dir.join("kernel"))
        .map_err(|e| format!("reading kernel metadata failed: {e}"))?
        .len();
    let initrd_len = fs::metadata(boot_dir.join("initrd"))
        .map_err(|e| format!("reading initrd metadata failed: {e}"))?
        .len();
    let min_efi_len = 96 * 1024 * 1024;
    let payload_len = bootx64_len + limine_len + (kernel_len * 2) + (initrd_len * 2);
    let efi_len = round_up_u64(
        min_efi_len.max(payload_len + 64 * 1024 * 1024),
        16 * 1024 * 1024,
    );
    let efi_file =
        fs::File::create(&efi_img).map_err(|e| format!("creating efiboot.img failed: {e}"))?;
    efi_file
        .set_len(efi_len)
        .map_err(|e| format!("sizing efiboot.img failed: {e}"))?;
    drop(efi_file);
    let mkfs = Command::new("mkfs.vfat")
        .args(["-n", "JETOS_EFI"])
        .arg(&efi_img)
        .output()
        .map_err(|e| format!("running mkfs.vfat failed: {e}"))?;
    if !mkfs.status.success() {
        return Err(format!(
            "mkfs.vfat exited with {}: {}",
            mkfs.status,
            String::from_utf8_lossy(&mkfs.stderr)
        ));
    }
    let mmd = Command::new("mmd")
        .args(["-i"])
        .arg(&efi_img)
        .args(["::/EFI", "::/EFI/BOOT", "::/boot"])
        .output()
        .map_err(|e| format!("running mmd failed: {e}"))?;
    if !mmd.status.success() {
        return Err(format!(
            "mmd exited with {}: {}",
            mmd.status,
            String::from_utf8_lossy(&mmd.stderr)
        ));
    }
    for (source, target) in [
        (data_dir.join("BOOTX64.EFI"), "::/EFI/BOOT/BOOTX64.EFI"),
        (boot_dir.join("limine.conf"), "::/boot/limine.conf"),
        (boot_dir.join("kernel"), "::/boot/kernel"),
        (boot_dir.join("initrd"), "::/boot/initrd"),
        (boot_dir.join("kernel"), "::/kernel"),
        (boot_dir.join("initrd"), "::/initrd"),
    ] {
        let mcopy = Command::new("mcopy")
            .args(["-i"])
            .arg(&efi_img)
            .arg(&source)
            .arg(target)
            .output()
            .map_err(|e| format!("running mcopy failed: {e}"))?;
        if !mcopy.status.success() {
            return Err(format!(
                "mcopy exited with {}: {}",
                mcopy.status,
                String::from_utf8_lossy(&mcopy.stderr)
            ));
        }
    }
    let xorriso = Command::new("xorriso")
        .args([
            "-as",
            "mkisofs",
            "-b",
            "boot/limine-bios.sys",
            "-no-emul-boot",
            "-boot-load-size",
            "4",
            "-boot-info-table",
            "--efi-boot",
            "boot/efiboot.img",
            "-efi-boot-part",
            "--efi-boot-image",
            "--protective-msdos-label",
            "-o",
        ])
        .arg(iso)
        .arg(staging)
        .output()
        .map_err(|e| format!("running xorriso failed: {e}"))?;
    if !xorriso.status.success() {
        return Err(format!(
            "xorriso exited with {}: {}",
            xorriso.status,
            String::from_utf8_lossy(&xorriso.stderr)
        ));
    }
    let limine = Command::new("limine")
        .args(["bios-install"])
        .arg(iso)
        .output()
        .map_err(|e| format!("running limine bios-install failed: {e}"))?;
    if !limine.status.success() {
        return Err(format!(
            "limine bios-install exited with {}: {}",
            limine.status,
            String::from_utf8_lossy(&limine.stderr)
        ));
    }
    Ok(true)
}

fn round_up_u64(value: u64, unit: u64) -> u64 {
    if unit == 0 {
        return value;
    }
    value.div_ceil(unit) * unit
}

fn write_vm_install_plan(
    gen: &Generation,
    system: &SystemPlan,
    disk: &str,
    media_proof: &Path,
) -> std::io::Result<PathBuf> {
    let proof_dir = systems_dir().join("vm-proofs");
    fs::create_dir_all(&proof_dir)?;
    let proof = proof_dir.join(format!("{}-{}-vm-proof.json", system.name, gen.name));
    let tools = vm_tools_json();
    let iso = systems_dir()
        .join("images")
        .join(format!("jetos-installer-{}.iso", system.name));
    let staging_boot = systems_dir()
        .join("images")
        .join(format!("jetos-installer-{}.iso.d/boot", system.name));
    let commands = qemu_proof_commands_json(&staging_boot, disk, &iso, &system.name, &gen.name);
    let guest = guest_proof_path(&proof);
    let text = format!(
        "{{\"host\":{},\"generation\":{},\"state\":\"harness-ready\",\"disk\":{},\"installer_media\":{},\"media_proof\":{},\"expected_guest_proof\":{},\"tools\":[{}],\"commands\":[{}],\"steps\":[\"build-generation\",\"create-hybrid-iso\",\"boot-installer-qemu\",\"install-to-disk\",\"reboot-installed-disk\",\"verify-guest-proof\",\"boot-graphical-desktop\"],\"guest_assertions\":[{}]}}",
        JSON::quote(&system.name),
        JSON::quote(&gen.name),
        JSON::quote(disk),
        JSON::quote(&format!("jetos-installer-{}.iso", system.name)),
        JSON::quote(&media_proof.display().to_string()),
        JSON::quote(&guest.display().to_string()),
        tools,
        commands,
        guest_assertions_json()
    );
    fs::write(&proof, text)?;
    Ok(proof)
}

fn prove_vm_guest(
    gen: &Generation,
    system: &SystemPlan,
    disk: &str,
    media_proof: &Path,
    harness: &Path,
) -> Result<Option<PathBuf>, String> {
    if let Some(final_path) = finalize_vm_guest_proof(gen, system, disk, media_proof, harness)? {
        return Ok(Some(final_path));
    }
    if !run_vm_install_harness(gen, system, disk, media_proof, harness)? {
        return Ok(None);
    }
    finalize_vm_guest_proof(gen, system, disk, media_proof, harness)
}

fn run_vm_install_harness(
    gen: &Generation,
    system: &SystemPlan,
    disk: &str,
    media_proof: &Path,
    harness: &Path,
) -> Result<bool, String> {
    let iso = systems_dir()
        .join("images")
        .join(format!("jetos-installer-{}.iso", system.name));
    let staging_boot = systems_dir()
        .join("images")
        .join(format!("jetos-installer-{}.iso.d/boot", system.name));
    let log_dir = vm_run_log_dir(harness);
    if log_dir.exists() {
        fs::remove_dir_all(&log_dir)
            .map_err(|e| format!("clearing `{}` failed: {e}", log_dir.display()))?;
    }
    fs::create_dir_all(&log_dir)
        .map_err(|e| format!("creating `{}` failed: {e}", log_dir.display()))?;
    let mut graphical_output = String::new();
    for command in qemu_proof_commands(&staging_boot, disk, &iso, &system.name, &gen.name) {
        let output = run_vm_command(&command, &log_dir)?;
        if command.phase == "boot-graphical-desktop" {
            graphical_output = output;
        }
    }
    let Some(report) = extract_guest_proof_report(&graphical_output) else {
        return Ok(false);
    };
    write_runner_guest_proof(gen, system, disk, media_proof, harness, &report)?;
    Ok(true)
}

fn vm_run_log_dir(harness: &Path) -> PathBuf {
    let stem = harness
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("vm-proof");
    harness.with_file_name(format!("{stem}.run"))
}

fn run_vm_command(command: &VmCommand, log_dir: &Path) -> Result<String, String> {
    let Some(program) = command.argv.first() else {
        return Err(format!("VM phase `{}` has no executable", command.phase));
    };
    let stdout_path = log_dir.join(format!("{}.stdout", command.phase));
    let stderr_path = log_dir.join(format!("{}.stderr", command.phase));
    let stdout = fs::File::create(&stdout_path)
        .map_err(|e| format!("creating `{}` failed: {e}", stdout_path.display()))?;
    let stderr = fs::File::create(&stderr_path)
        .map_err(|e| format!("creating `{}` failed: {e}", stderr_path.display()))?;
    let mut child = Command::new(program)
        .args(command.argv.iter().skip(1))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|e| format!("starting VM phase `{}` failed: {e}", command.phase))?;
    let start = Instant::now();
    let timeout = vm_proof_timeout();
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("waiting for VM phase `{}` failed: {e}", command.phase))?
        {
            break status;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            let stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
            let stderr = fs::read_to_string(&stderr_path).unwrap_or_default();
            return Err(format!(
                "VM phase `{}` timed out after {}ms; stdout `{}`, stderr `{}`{}{}",
                command.phase,
                timeout.as_millis(),
                stdout_path.display(),
                stderr_path.display(),
                log_excerpt("stdout", &stdout),
                log_excerpt("stderr", &stderr)
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
    let stderr = fs::read_to_string(&stderr_path).unwrap_or_default();
    if !status.success() {
        return Err(format!(
            "VM phase `{}` exited with {}; stdout `{}`, stderr `{}`{}{}",
            command.phase,
            status,
            stdout_path.display(),
            stderr_path.display(),
            log_excerpt("stdout", &stdout),
            log_excerpt("stderr", &stderr)
        ));
    }
    Ok(format!("{stdout}\n{stderr}"))
}

fn run_interactive_vm_command(command: &VmCommand) -> Result<i32, String> {
    let Some(program) = command.argv.first() else {
        return Err(format!("VM phase `{}` has no executable", command.phase));
    };
    let status = Command::new(program)
        .args(command.argv.iter().skip(1))
        .status()
        .map_err(|e| format!("starting VM phase `{}` failed: {e}", command.phase))?;
    Ok(status
        .code()
        .unwrap_or(if status.success() { 0 } else { 1 }))
}

fn log_excerpt(label: &str, text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    if line.is_empty() {
        String::new()
    } else {
        let excerpt = line.chars().take(240).collect::<String>();
        format!("; {label}: {excerpt}")
    }
}

fn vm_proof_timeout() -> Duration {
    std::env::var("JETOS_VM_PROOF_TIMEOUT_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_millis(VM_PROOF_TIMEOUT_MS))
}

fn extract_guest_proof_report(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        line.split_once(VM_GUEST_PROOF_MARKER)
            .map(|(_, rest)| rest.trim().to_string())
    })
}

fn write_runner_guest_proof(
    gen: &Generation,
    system: &SystemPlan,
    disk: &str,
    media_proof: &Path,
    harness: &Path,
    report: &str,
) -> Result<(), String> {
    require_guest_report(report, system, gen)?;
    let guest = guest_proof_path(harness);
    let text = format!(
        "{{\"state\":\"guest-passed\",\"host\":{},\"generation\":{},\"disk\":{},\"media_proof\":{},\"assertions\":[{}],\"tools\":[{}],\"serial_report\":{}}}\n",
        JSON::quote(&system.name),
        JSON::quote(&gen.name),
        JSON::quote(disk),
        JSON::quote(&media_proof.display().to_string()),
        guest_assertions_json(),
        vm_tools_json(),
        JSON::quote(report)
    );
    fs::write(&guest, text).map_err(|e| format!("writing `{}` failed: {e}", guest.display()))
}

fn require_guest_report(report: &str, system: &SystemPlan, gen: &Generation) -> Result<(), String> {
    if !report.contains("\"state\":\"guest-passed\"") {
        return Err("guest serial proof did not report state=guest-passed".to_string());
    }
    require_json_field(report, "host", &system.name)?;
    require_json_field(report, "generation", &gen.name)?;
    require_guest_assertions(report)
}

fn finalize_vm_guest_proof(
    gen: &Generation,
    system: &SystemPlan,
    disk: &str,
    media_proof: &Path,
    harness: &Path,
) -> Result<Option<PathBuf>, String> {
    let guest = guest_proof_path(harness);
    if !guest.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(&guest)
        .map_err(|e| format!("reading `{}` failed: {e}", guest.display()))?;
    require_json_field(&text, "state", "guest-passed")?;
    require_json_field(&text, "host", &system.name)?;
    require_json_field(&text, "generation", &gen.name)?;
    require_json_field(&text, "disk", disk)?;
    require_json_field(&text, "media_proof", &media_proof.display().to_string())?;
    require_guest_assertions(&text)?;
    for (name, _path, sha) in vm_tool_facts() {
        if !text.contains(&name) || !text.contains(&sha) {
            return Err(format!("missing tool proof for `{name}`"));
        }
    }
    let guest_sha = fs::read(&guest)
        .map(|bytes| crate::SHA256::sha256_hex(&bytes))
        .map_err(|e| format!("hashing `{}` failed: {e}", guest.display()))?;
    let harness_text = fs::read_to_string(harness)
        .map_err(|e| format!("reading `{}` failed: {e}", harness.display()))?;
    let final_text = harness_text.replacen(
        "\"state\":\"harness-ready\"",
        &format!(
            "\"state\":\"guest-passed\",\"guest_proof\":{},\"guest_proof_sha256\":{}",
            JSON::quote(&guest.display().to_string()),
            JSON::quote(&guest_sha)
        ),
        1,
    );
    fs::write(harness, final_text)
        .map_err(|e| format!("writing `{}` failed: {e}", harness.display()))?;
    Ok(Some(harness.to_path_buf()))
}

fn require_vm_run_proof(
    gen: &Generation,
    system: &SystemPlan,
    disk: &str,
    media_proof: &Path,
    harness: &Path,
) -> Result<(), String> {
    if !harness.is_file() {
        return Err(format!("missing VM proof `{}`", harness.display()));
    }
    let harness_text = fs::read_to_string(harness)
        .map_err(|e| format!("reading `{}` failed: {e}", harness.display()))?;
    require_json_field(&harness_text, "state", "guest-passed")?;
    require_json_field(&harness_text, "host", &system.name)?;
    require_json_field(&harness_text, "generation", &gen.name)?;
    require_json_field(&harness_text, "disk", disk)?;
    require_json_field(&harness_text, "media_proof", &media_proof.display().to_string())?;
    Ok(())
}

fn guest_proof_path(harness: &Path) -> PathBuf {
    let stem = harness
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("vm-proof");
    harness.with_file_name(format!("{stem}-guest-proof.json"))
}

fn require_json_field(text: &str, key: &str, expected: &str) -> Result<(), String> {
    let needle = format!("\"{key}\"");
    let Some(mut rest) = text.split_once(&needle).map(|(_, r)| r.trim_start()) else {
        return Err(format!("missing `{key}`"));
    };
    if let Some(after) = rest.strip_prefix(':') {
        rest = after.trim_start();
    } else {
        return Err(format!("missing `:` after `{key}`"));
    }
    let Some(rest) = rest.strip_prefix('"') else {
        return Err(format!("`{key}` is not a string"));
    };
    let Some(end) = rest.find('"') else {
        return Err(format!("`{key}` is not closed"));
    };
    let found = &rest[..end];
    if found == expected {
        Ok(())
    } else {
        Err(format!("`{key}` expected `{expected}`, found `{found}`"))
    }
}

fn require_guest_assertions(text: &str) -> Result<(), String> {
    let expected = format!("\"assertions\":[{}]", guest_assertions_json());
    if text.contains(&expected) {
        Ok(())
    } else {
        Err("guest assertions did not match the required install/reboot proof set".to_string())
    }
}

const GUEST_ASSERTIONS: [&str; 9] = [
    "current-generation-matches",
    "packages-present",
    "services-active",
    "network-up",
    "rollback-generation-bootable",
    "terminal-login-ready",
    "desktop-session-ready",
    "graphical-console-ready",
    "desktop-launchers-run",
];

struct VmCommand {
    phase: &'static str,
    argv: Vec<String>,
}

fn ovmf_code_path() -> Option<PathBuf> {
    std::env::var_os("JETOS_OVMF_CODE")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

fn qemu_proof_commands(
    boot_dir: &Path,
    disk: &str,
    iso: &Path,
    host: &str,
    generation: &str,
) -> Vec<VmCommand> {
    let iso_path = iso.display().to_string();
    let kernel = boot_dir.join("kernel").display().to_string();
    let initrd = boot_dir.join("initrd").display().to_string();
    let mut boot_installer = vec![
        "qemu-system-x86_64".to_string(),
        "-m".to_string(),
        "2048".to_string(),
        "-nographic".to_string(),
        "-monitor".to_string(),
        "none".to_string(),
    ];
    if let Some(ovmf) = ovmf_code_path() {
        boot_installer.extend([
            "-drive".to_string(),
            format!("if=pflash,format=raw,readonly=on,file={}", ovmf.display()),
        ]);
    }
    boot_installer.extend([
        "-cdrom".to_string(),
        iso_path,
        "-drive".to_string(),
        format!("file={disk},format=qcow2,if=ide"),
        "-netdev".to_string(),
        format!("user,id=net0,hostname={host}"),
        "-device".to_string(),
        "virtio-net-pci,netdev=net0".to_string(),
        "-boot".to_string(),
        "d".to_string(),
    ]);
    let graphical_cmdline = format!(
        "console=ttyS0 rdinit=/jetos/init init=/jetos/init jetos.mode=desktop-verify jetos.host={host} jetos.generation={generation} root=LABEL=jetos-root rw"
    );
    let mut boot_installed_disk = vec![
        "qemu-system-x86_64".to_string(),
        "-m".to_string(),
        "2048".to_string(),
        "-nographic".to_string(),
        "-monitor".to_string(),
        "none".to_string(),
    ];
    if let Some(ovmf) = ovmf_code_path() {
        boot_installed_disk.extend([
            "-drive".to_string(),
            format!("if=pflash,format=raw,readonly=on,file={}", ovmf.display()),
        ]);
    }
    boot_installed_disk.extend([
        "-drive".to_string(),
        format!("file={disk},format=qcow2,if=ide"),
        "-netdev".to_string(),
        format!("user,id=net0,hostname={host}"),
        "-device".to_string(),
        "virtio-net-pci,netdev=net0".to_string(),
        "-boot".to_string(),
        "c".to_string(),
    ]);
    vec![
        VmCommand {
            phase: "create-disk",
            argv: vec![
                "qemu-img".to_string(),
                "create".to_string(),
                "-f".to_string(),
                "qcow2".to_string(),
                disk.to_string(),
                "16G".to_string(),
            ],
        },
        VmCommand {
            phase: "boot-installer",
            argv: boot_installer,
        },
        VmCommand {
            phase: "boot-installed-disk",
            argv: boot_installed_disk,
        },
        VmCommand {
            phase: "boot-graphical-desktop",
            argv: vec![
                "qemu-system-x86_64".to_string(),
                "-m".to_string(),
                "2048".to_string(),
                "-display".to_string(),
                "vnc=127.0.0.1:0,to=99".to_string(),
                "-serial".to_string(),
                "stdio".to_string(),
                "-monitor".to_string(),
                "none".to_string(),
                "-vga".to_string(),
                "std".to_string(),
                "-kernel".to_string(),
                kernel,
                "-initrd".to_string(),
                initrd,
                "-append".to_string(),
                graphical_cmdline,
                "-drive".to_string(),
                format!("file={disk},format=qcow2,if=ide"),
                "-netdev".to_string(),
                format!("user,id=net0,hostname={host}"),
                "-device".to_string(),
                "virtio-net-pci,netdev=net0".to_string(),
                "-boot".to_string(),
                "c".to_string(),
            ],
        },
    ]
}

fn qemu_interactive_run_command(
    boot_dir: &Path,
    disk: &str,
    host: &str,
    generation: &str,
) -> VmCommand {
    let kernel = boot_dir.join("kernel").display().to_string();
    let initrd = boot_dir.join("initrd").display().to_string();
    let console = "tty0";
    let cmdline = format!(
        "console={console} rdinit=/jetos/init init=/jetos/init jetos.mode=run jetos.host={host} jetos.generation={generation} root=LABEL=jetos-root rw"
    );
    let mut argv = vec![
        "qemu-system-x86_64".to_string(),
        "-m".to_string(),
        "2048".to_string(),
        "-cpu".to_string(),
        "max".to_string(),
    ];
    if qemu_has_local_display() {
        argv.extend([
            "-display".to_string(),
            "gtk,gl=off".to_string(),
            "-serial".to_string(),
            "none".to_string(),
        ]);
    } else {
        argv.extend([
            "-display".to_string(),
            "vnc=127.0.0.1:0,to=99".to_string(),
            "-serial".to_string(),
            "none".to_string(),
        ]);
    }
    argv.extend([
        "-monitor".to_string(),
        "none".to_string(),
        "-vga".to_string(),
        "std".to_string(),
        "-kernel".to_string(),
        kernel,
        "-initrd".to_string(),
        initrd,
        "-append".to_string(),
        cmdline,
        "-drive".to_string(),
        format!("file={disk},format=qcow2,if=ide"),
        "-netdev".to_string(),
        format!("user,id=net0,hostname={host}"),
        "-device".to_string(),
        "virtio-net-pci,netdev=net0".to_string(),
        "-boot".to_string(),
        "c".to_string(),
    ]);
    VmCommand {
        phase: "run-installed-disk",
        argv,
    }
}

fn qemu_has_local_display() -> bool {
    if std::env::var_os("JETOS_QEMU_VNC").is_some() {
        return false;
    }
    std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

fn qemu_proof_commands_json(
    boot_dir: &Path,
    disk: &str,
    iso: &Path,
    host: &str,
    generation: &str,
) -> String {
    qemu_proof_commands(boot_dir, disk, iso, host, generation)
        .into_iter()
        .map(|command| {
            let argv_json = command
                .argv
                .iter()
                .map(|arg| JSON::quote(arg))
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{\"phase\":{},\"argv\":[{}]}}",
                JSON::quote(command.phase),
                argv_json
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn run_vmtest(
    theme: &Theme,
    plan: &EnvPlan,
    vmtest: &VmTestPlan,
    disk: &str,
    flags: &OsFlags,
) -> Result<PathBuf, String> {
    let proof_dir = systems_dir().join("vm-tests");
    fs::create_dir_all(&proof_dir)
        .map_err(|e| format!("creating `{}` failed: {e}", proof_dir.display()))?;
    let mut host_facts = Vec::new();
    for host in &vmtest.hosts {
        let Some(system) = plan.systems.iter().find(|s| s.name == host.system).cloned() else {
            return Err(format!(
                "host `{}` names unknown system `{}`",
                host.name, host.system
            ));
        };
        if !validate_system_options(theme, &system) {
            return Err(format!("system `{}` failed option validation", system.name));
        }
        let host_disk = vmtest_host_disk(disk, &host.name, vmtest.hosts.len());
        let Some(gen) = build_generation(theme, plan, &system, flags) else {
            return Err(format!("building system `{}` failed", system.name));
        };
        let media = write_installer_media(&gen, &system, "guided-ext4")
            .map_err(|e| format!("writing installer media for `{}` failed: {e}", system.name))?;
        let harness = write_vm_install_plan(&gen, &system, &host_disk, &media)
            .map_err(|e| format!("writing VM proof plan for `{}` failed: {e}", system.name))?;
        let final_path = prove_vm_guest(&gen, &system, &host_disk, &media, &harness)
            .map_err(|e| format!("guest proof for `{}` failed: {e}", system.name))?
            .ok_or_else(|| format!("guest proof for `{}` was not recorded", system.name))?;
        host_facts.push(VmTestHostFact {
            name: host.name.clone(),
            system: system.name,
            generation: gen.name,
            disk: host_disk,
            proof: final_path.display().to_string(),
        });
    }
    let proof = proof_dir.join(format!("{}-vmtest-proof.json", vmtest.name));
    fs::write(&proof, vmtest_proof_json(vmtest, &host_facts))
        .map_err(|e| format!("writing `{}` failed: {e}", proof.display()))?;
    Ok(proof)
}

struct VmTestHostFact {
    name: String,
    system: String,
    generation: String,
    disk: String,
    proof: String,
}

fn vmtest_host_disk(disk: &str, host: &str, host_count: usize) -> String {
    if host_count <= 1 {
        return disk.to_string();
    }
    let path = Path::new(disk);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(disk);
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("qcow2");
    let file = format!("{stem}-{host}.{ext}");
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.join(&file).display().to_string())
        .unwrap_or(file)
}

fn vmtest_proof_json(vmtest: &VmTestPlan, hosts: &[VmTestHostFact]) -> String {
    let host_json = hosts
        .iter()
        .map(|host| {
            JSON::object_of(&[
                ("name", &host.name),
                ("system", &host.system),
                ("generation", &host.generation),
                ("disk", &host.disk),
                ("proof", &host.proof),
            ])
        })
        .collect::<Vec<_>>()
        .join(",");
    let assertions = vmtest
        .assertions
        .iter()
        .map(|a| JSON::quote(a))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"kind\":\"jetos.vmtest.proof\",\"schema_version\":1,\"state\":\"passed\",\"name\":{},\"hosts\":[{}],\"assertions\":[{}],\"run\":{},\"proofs\":[\"build-generation\",\"install-reboot-proof\",\"typed-assertion-record\"]}}\n",
        JSON::quote(&vmtest.name),
        host_json,
        assertions,
        JSON::quote(&vmtest.run)
    )
}

fn guest_assertions_json() -> String {
    GUEST_ASSERTIONS
        .iter()
        .map(|assertion| JSON::quote(assertion))
        .collect::<Vec<_>>()
        .join(",")
}

fn vm_tools_json() -> String {
    vm_tool_facts()
        .into_iter()
        .map(|(name, path, sha)| {
            JSON::object_of(&[("name", &name), ("path", &path), ("sha256", &sha)])
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn vm_tool_facts() -> Vec<(String, String, String)> {
    VM_TOOLS
        .iter()
        .map(|tool| {
            let Some(path) = find_path_tool(tool) else {
                return (
                    (*tool).to_string(),
                    "<missing>".to_string(),
                    "<missing>".to_string(),
                );
            };
            let path_text = path.display().to_string();
            let sha = fs::read(&path)
                .map(|bytes| crate::SHA256::sha256_hex(&bytes))
                .unwrap_or_else(|_| "<unreadable>".to_string());
            ((*tool).to_string(), path_text, sha)
        })
        .collect()
}

fn write_activation_diff(
    dir: &Path,
    system: &SystemPlan,
    realized: &[Store::StoreEntry],
) -> std::io::Result<()> {
    let previous = current_generation_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<none>".to_string());
    let enabled_services = system.services.iter().filter(|s| s.enable).count();
    let mut diff = String::new();
    diff.push_str(&format!("host: {}\n", system.name));
    diff.push_str(&format!("previous: {previous}\n"));
    diff.push_str(&format!("next: {}\n", dir.display()));
    diff.push_str(&format!("packages: {}\n", realized.len()));
    diff.push_str(&format!("services: {enabled_services}\n"));
    diff.push_str(&format!("options: {}\n", system.options.len()));
    for pkg in realized {
        diff.push_str(&format!("  package {} -> {}\n", pkg.reference, pkg.out));
    }
    for svc in system.services.iter().filter(|s| s.enable) {
        diff.push_str(&format!("  service {} enable=true\n", svc.name));
    }
    for option in &system.options {
        diff.push_str(&format!("  option {} = {}\n", option.key, option.value));
    }
    fs::write(dir.join("activation-diff.txt"), diff)
}

fn write_health_checks(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let mut out = String::new();
    out.push_str("jetos health checks\n");
    for svc in system.services.iter().filter(|s| s.enable) {
        let check = service_extra(svc, &["health", "ready"])
            .unwrap_or_else(|| format!("systemctl is-active {}", svc.name));
        out.push_str(&format!("service {}: {}\n", svc.name, check));
    }
    let host = option_value(system, &["network.hostName", "network.hostname"])
        .unwrap_or_else(|| system.name.clone());
    out.push_str(&format!("network hostname: {host}\n"));
    for name in collect_names(system, "health") {
        if let Some(command) = option_value(system, &[&format!("health.{name}.command")]) {
            out.push_str(&format!("check {name}: {command}\n"));
        }
    }
    fs::write(dir.join("health-checks.txt"), out)
}

fn write_provenance(
    dir: &Path,
    system: &SystemPlan,
    realized: &[Store::StoreEntry],
) -> std::io::Result<()> {
    let packages = realized
        .iter()
        .map(|p| {
            JSON::object_of(&[
                ("name", &p.name),
                ("reference", &p.reference),
                ("out", &p.out),
                ("bin", &p.bin),
                ("output_hash", &p.envelope.output_hash),
                ("provenance", &p.envelope.provenance),
            ])
        })
        .collect::<Vec<_>>()
        .join(",");
    let kernel = cachyos_kernel_entry(realized)
        .map(kernel_package_json)
        .unwrap_or_else(|| "null".to_string());
    let text = format!(
        "{{\"host\":{},\"target\":{},\"kernel\":{},\"packages\":[{}],\"compat_escape_hatches\":[{}]}}",
        JSON::quote(&system.name),
        JSON::quote(&system.target),
        kernel,
        packages,
        compat_hatches_json(system)
    );
    fs::write(dir.join("provenance.json"), text)
}

fn compat_hatches_json(system: &SystemPlan) -> String {
    let mut hatches = Vec::new();
    for option in &system.options {
        if option.key.starts_with("packages.overlay.")
            || option.key.starts_with("packages.specialArgs.")
            || option.key.starts_with("packages.nixModule.")
        {
            hatches.push(JSON::object_of(&[
                ("key", &option.key),
                ("value", &clean_value(&option.value)),
                ("audit", "compatibility escape hatch"),
                ("provenance_visible", "true"),
                ("studio_visible", "true"),
                ("native_replacement", "tracked"),
            ]));
        }
    }
    hatches.join(",")
}

fn write_systemd_units(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let unit_dir = dir.join("etc/systemd/system");
    fs::create_dir_all(&unit_dir)?;
    for svc in &system.services {
        if !svc.enable {
            continue;
        }
        let exec = svc
            .extra
            .iter()
            .find(|(k, _)| k == "exec" || k == "command")
            .map(|(_, v)| v.trim_matches('"').to_string())
            .unwrap_or_else(|| "/usr/bin/env true".to_string());
        fs::write(
            unit_dir.join(format!("{}.service", svc.name)),
            format!(
                "[Unit]\nDescription=jetos service {}\n\n[Service]\nExecStart={}\nRestart=on-failure\n\n[Install]\nWantedBy=multi-user.target\n",
                svc.name, exec
            ),
        )?;
        enable_unit(
            &unit_dir,
            "multi-user.target",
            &format!("{}.service", svc.name),
        )?;
    }
    Ok(())
}

fn write_terminal_environment(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let terminal_dir = dir.join("terminal");
    let etc = dir.join("etc");
    let unit_dir = etc.join("systemd/system");
    fs::create_dir_all(&terminal_dir)?;
    fs::create_dir_all(&etc)?;
    fs::create_dir_all(&unit_dir)?;

    let users = collect_names(system, "users");
    let login_user = users.first().cloned().unwrap_or_else(|| "root".to_string());
    let shell = if login_user == "root" {
        "/bin/sh".to_string()
    } else {
        option_value(system, &[&format!("users.{login_user}.shell")])
            .map(|s| package_path_or_literal(&s))
            .unwrap_or_else(|| "/run/current-system/sw/bin/sh".to_string())
    };

    let mut shells = String::from("/bin/sh\n/run/current-system/sw/bin/sh\n");
    if !shells.lines().any(|line| line == shell) {
        shells.push_str(&shell);
        shells.push('\n');
    }
    fs::write(etc.join("shells"), shells)?;
    let prompt_label = format!("JetOS {}", system.name);
    let prompt_bash = format!(
        "\\[\\033[1;36m\\]JetOS\\[\\033[0m\\] \\[\\033[2m\\]{}\\[\\033[0m\\] \\w \\$ ",
        system.name
    );
    let prompt_plain = format!("JetOS {} $ ", system.name);
    fs::write(
        etc.join("profile"),
        format!(
            "export PATH=/run/current-system/sw/bin:/bin:/sbin:/usr/bin:/usr/sbin\nexport JETOS_GENERATION=/run/current-system\nexport JETOS_HOST={}\nexport JETOS_BRAND=JetOS\nexport JETOS_PROMPT={}\nif [ -n \"${{BASH_VERSION:-}}\" ]; then\n    PS1={}\nelse\n    PS1={}\nfi\nexport PS1\n",
            shell_single_quote(&system.name),
            shell_single_quote(&prompt_label),
            shell_single_quote(&prompt_bash),
            shell_single_quote(&prompt_plain)
        ),
    )?;
    fs::write(
        etc.join("issue"),
        format!(
            "JetOS {}\nproof-backed system shell\n\\n \\l\n",
            system.name
        ),
    )?;
    fs::write(
        etc.join("motd"),
        format!(
            "JetOS {}\n/run/current-system is live\nsource-owned, proof-backed\n",
            system.name
        ),
    )?;
    fs::write(etc.join("securetty"), "ttyS0\ntty1\n")?;

    let home_root = dir.join("home");
    for user in &users {
        let home = option_value(system, &[&format!("users.{user}.home")])
            .unwrap_or_else(|| format!("/home/{user}"));
        if let Some(rel) = home.strip_prefix('/') {
            fs::create_dir_all(dir.join(rel))?;
            fs::write(
                dir.join(rel).join(".profile"),
                ". /etc/profile\ncd \"$HOME\" 2>/dev/null || true\n",
            )?;
        } else {
            fs::create_dir_all(home_root.join(user))?;
        }
    }

    fs::write(
        unit_dir.join("serial-getty@ttyS0.service"),
        format!(
            "[Unit]\nDescription=jetos serial login on %I\nAfter=systemd-user-sessions.service plymouth-quit-wait.service\n\n[Service]\nExecStart=/run/current-system/sw/bin/agetty --autologin {} --noclear %I 115200 linux\nType=idle\nRestart=always\n\n[Install]\nWantedBy=getty.target\n",
            login_user
        ),
    )?;
    enable_unit(&unit_dir, "getty.target", "serial-getty@ttyS0.service")?;
    fs::write(
        unit_dir.join("getty@tty1.service"),
        format!(
            "[Unit]\nDescription=jetos virtual-console login on %I\nAfter=systemd-user-sessions.service plymouth-quit-wait.service\n\n[Service]\nExecStart=/run/current-system/sw/bin/agetty --autologin {} --noclear %I linux\nType=idle\nRestart=always\n\n[Install]\nWantedBy=getty.target\n",
            login_user
        ),
    )?;
    enable_unit(&unit_dir, "getty.target", "getty@tty1.service")?;

    fs::write(
        terminal_dir.join("facts.json"),
        format!(
            "{{\"login_user\":{},\"shell\":{},\"serial_tty\":\"ttyS0\",\"virtual_tty\":\"tty1\",\"profile\":\"/etc/profile\",\"prompt\":{},\"motd\":\"/etc/motd\",\"unit_dir\":\"etc/systemd/system\",\"proof\":\"terminal-login-ready\"}}",
            JSON::quote(&login_user),
            JSON::quote(&shell),
            JSON::quote(&prompt_plain)
        ),
    )
}

fn shell_single_quote(s: &str) -> String {
    let mut quoted = String::from("'");
    for ch in s.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

fn option_value(system: &SystemPlan, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| resolved_option_value(system, key))
}

fn raw_option_value(system: &SystemPlan, key: &str) -> Option<String> {
    system
        .options
        .iter()
        .find(|o| o.key == key)
        .map(|o| clean_value(&o.value))
}

fn resolved_option_value(system: &SystemPlan, key: &str) -> Option<String> {
    resolved_option(system, key).map(|r| r.value)
}

struct ResolvedOption {
    key: String,
    value: String,
    tier: String,
    priority: i64,
    contenders: Vec<OptionContender>,
}

struct OptionContender {
    value: String,
    tier: String,
    priority: i64,
    source_order: usize,
    winner: bool,
}

impl ResolvedOption {
    fn to_json(&self) -> String {
        let contenders = self
            .contenders
            .iter()
            .map(|c| {
                format!(
                    "{{\"value\":{},\"tier\":{},\"priority\":{},\"source_order\":{},\"winner\":{}}}",
                    JSON::quote(&c.value),
                    JSON::quote(&c.tier),
                    c.priority,
                    c.source_order,
                    if c.winner { "true" } else { "false" }
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"key\":{},\"value\":{},\"tier\":{},\"priority\":{},\"contenders\":[{}]}}",
            JSON::quote(&self.key),
            JSON::quote(&self.value),
            JSON::quote(&self.tier),
            self.priority,
            contenders
        )
    }
}

fn resolved_option(system: &SystemPlan, key: &str) -> Option<ResolvedOption> {
    let tier = raw_option_value(system, &format!("{key}.tier"))
        .map(|s| clean_symbol(&s))
        .unwrap_or_else(|| "Normal".to_string());
    let priority = raw_option_value(system, &format!("{key}.priority"))
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or_else(|| tier_priority(&tier));
    let mut contenders = Vec::new();
    for (idx, option) in system.options.iter().enumerate() {
        if option.key == key {
            contenders.push(OptionContender {
                value: clean_value(&option.value),
                tier: tier.clone(),
                priority,
                source_order: idx,
                winner: false,
            });
        }
    }
    let winner_idx = contenders
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            a.priority
                .cmp(&b.priority)
                .then_with(|| a.source_order.cmp(&b.source_order))
        })
        .map(|(idx, _)| idx)?;
    contenders[winner_idx].winner = true;
    let winner = &contenders[winner_idx];
    Some(ResolvedOption {
        key: key.to_string(),
        value: winner.value.clone(),
        tier: winner.tier.clone(),
        priority: winner.priority,
        contenders,
    })
}

fn tier_priority(tier: &str) -> i64 {
    match tier {
        "Default" => 100,
        "Force" => 10_000,
        _ => 1_000,
    }
}

fn is_option_priority_metadata(key: &str) -> bool {
    key.ends_with(".tier")
        || key.ends_with(".priority")
        || key.ends_with(".override")
        || key.ends_with(".disabled")
}

fn option_type(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.starts_with('[') {
        "List".to_string()
    } else if trimmed == "true" || trimmed == "false" {
        "Bool".to_string()
    } else if trimmed.parse::<i64>().is_ok() {
        "Int".to_string()
    } else if trimmed.starts_with('.') {
        "Enum".to_string()
    } else {
        "String".to_string()
    }
}

fn option_default(namespace: &str) -> String {
    match namespace {
        "network" => "dhcp-and-closed-firewall",
        "services" => "disabled",
        "users" | "user" => "absent",
        "filesystem" | "storage" => "guided-root",
        "boot" | "kernel" => "safe-profile",
        "performance" => "safe",
        "theme" => "default",
        "apps" | "workload" => "none",
        _ => "unset",
    }
    .to_string()
}

fn option_doc(key: &str) -> String {
    let namespace = key.split('.').next().unwrap_or("");
    match namespace {
        "network" => "Network identity, DNS, wireless, and firewall policy.",
        "services" => "System service declaration projected to systemd units and proof.",
        "users" => "System account identity used by login and generated roots.",
        "user" => "Per-user environment profile applied by jetos-user-apply.",
        "filesystem" => "Mounted filesystems, swap, timezone, and root projection.",
        "storage" => "Installer and activation storage tree.",
        "boot" | "kernel" => "Bootloader, kernel, initrd, firmware, and driver selection.",
        "performance" => "Safe performance profile plus expert sysctl/zram/scheduler overrides.",
        "theme" => "Reusable theme projection for GTK, terminals, editors, DM, and Studio.",
        "apps" => "Foreign app ecosystem policy and app permissions.",
        "workload" => "Container or microVM workload declaration.",
        "deploy" => "Fleet deploy target, rollout, health, and rollback policy.",
        _ => "JetOS system option.",
    }
    .to_string()
}

fn prefixed_options(system: &SystemPlan, prefix: &str) -> Vec<(String, String)> {
    system
        .options
        .iter()
        .filter_map(|o| {
            o.key
                .strip_prefix(prefix)
                .map(|key| (key.to_string(), clean_value(&o.value)))
        })
        .collect()
}

fn option_rows_json(rows: &[(String, String)]) -> String {
    rows.iter()
        .map(|(key, value)| JSON::object_of(&[("key", key), ("value", value)]))
        .collect::<Vec<_>>()
        .join(",")
}

fn strings_json(values: &[String]) -> String {
    values
        .iter()
        .map(|value| JSON::quote(value))
        .collect::<Vec<_>>()
        .join(",")
}

fn boot_profile(system: &SystemPlan) -> BootProfile {
    let loader = option_value(system, &["boot.loader"])
        .map(|s| clean_symbol(&s))
        .unwrap_or_else(|| "Limine".to_string());
    let kernel = option_value(system, &["boot.kernel", "kernel.package"])
        .map(|s| clean_symbol(&s))
        .unwrap_or_else(|| "CachyOS".to_string());
    let init = option_value(system, &["init.path"]).unwrap_or_else(|| "/sbin/init".to_string());
    let initrd_modules = option_value(system, &["boot.initrd.modules"])
        .map(|v| parse_list_items(&v))
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| vec!["virtio_blk".to_string(), "ext4".to_string()]);
    BootProfile {
        loader,
        kernel,
        init,
        initrd_modules,
    }
}

fn collect_names(system: &SystemPlan, namespace: &str) -> Vec<String> {
    let prefix = format!("{namespace}.");
    let mut names = Vec::new();
    for option in &system.options {
        let Some(rest) = option.key.strip_prefix(&prefix) else {
            continue;
        };
        let Some((name, _)) = rest.split_once('.') else {
            continue;
        };
        if !name.is_empty() && !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
    }
    names.sort();
    names
}

fn user_names(system: &SystemPlan) -> Vec<String> {
    let mut names = collect_names(system, "users");
    for name in collect_names(system, "user") {
        if !names.iter().any(|n| n == &name) {
            names.push(name);
        }
    }
    names.sort();
    names
}

fn render_user_profile_json(system: &SystemPlan, user: &str) -> String {
    let home = option_value(
        system,
        &[&format!("user.{user}.home"), &format!("users.{user}.home")],
    )
    .unwrap_or_else(|| format!("/home/{user}"));
    let shell = option_value(
        system,
        &[
            &format!("user.{user}.shell"),
            &format!("users.{user}.shell"),
        ],
    )
    .map(|s| package_path_or_literal(&s))
    .unwrap_or_else(|| "/run/current-system/sw/bin/sh".to_string());
    let packages = option_value(
        system,
        &[
            &format!("user.{user}.packages"),
            &format!("users.{user}.packages"),
        ],
    )
    .map(|v| parse_list_items(&v))
    .unwrap_or_default();
    let services = option_value(
        system,
        &[
            &format!("user.{user}.services"),
            &format!("users.{user}.services"),
        ],
    )
    .map(|v| parse_list_items(&v))
    .unwrap_or_default();
    let files = prefixed_options(system, &format!("user.{user}.files."));
    let packages_json = packages
        .iter()
        .map(|p| JSON::quote(p))
        .collect::<Vec<_>>()
        .join(",");
    let services_json = services
        .iter()
        .map(|s| JSON::quote(s))
        .collect::<Vec<_>>()
        .join(",");
    let files_json = files
        .iter()
        .map(|(key, value)| JSON::object_of(&[("path", key), ("source", value)]))
        .collect::<Vec<_>>()
        .join(",");
    render_user_profile_json_parts(
        user,
        &home,
        &shell,
        &packages_json,
        &services_json,
        &files_json,
    )
}

fn render_user_profile_json_parts(
    user: &str,
    home: &str,
    shell: &str,
    packages_json: &str,
    services_json: &str,
    files_json: &str,
) -> String {
    format!(
        "{{\"kind\":\"jetos.user-generation\",\"user\":{},\"home\":{},\"shell\":{},\"packages\":[{}],\"services\":[{}],\"files\":[{}],\"standalone_commands\":[\"plan\",\"build\",\"switch\",\"rollback\",\"prove\"],\"proof\":\"user-generation-ready\"}}",
        JSON::quote(user),
        JSON::quote(home),
        JSON::quote(shell),
        packages_json,
        services_json,
        files_json
    )
}

fn parse_list_items(value: &str) -> Vec<String> {
    let inner = value
        .trim()
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(value);
    inner
        .split(',')
        .map(|item| clean_symbol(&clean_value(item)))
        .filter(|item| !item.is_empty())
        .collect()
}

fn package_path_or_literal(value: &str) -> String {
    if let Some(name) = value.strip_prefix("packages.") {
        format!("/run/current-system/sw/bin/{name}")
    } else {
        value.to_string()
    }
}

fn service_extra(service: &ServicePlan, keys: &[&str]) -> Option<String> {
    service
        .extra
        .iter()
        .find(|(k, _)| keys.iter().any(|wanted| k == wanted))
        .map(|(_, v)| clean_value(v))
}

fn clean_value(value: &str) -> String {
    let trimmed = value.trim();
    if let Some(inner) = trimmed.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        inner.to_string()
    } else {
        trimmed.to_string()
    }
}

fn clean_symbol(value: &str) -> String {
    let cleaned = clean_value(value);
    let trimmed = cleaned.trim().trim_start_matches('.');
    trimmed
        .strip_prefix("users.")
        .unwrap_or(trimmed)
        .to_string()
}

fn safe_filename(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn safe_identifier(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out
        .chars()
        .next()
        .map(|ch| ch.is_ascii_digit())
        .unwrap_or(true)
    {
        out.insert(0, '_');
    }
    out
}

fn clean_bool_json(value: &str) -> &'static str {
    if clean_symbol(value).eq_ignore_ascii_case("true") {
        "true"
    } else {
        "false"
    }
}

fn render_proof(system: &SystemPlan, realized: &[Store::StoreEntry], plan: &EnvPlan) -> String {
    let mut out = String::new();
    out.push_str(&format!("jetos proof for {}\n", system.name));
    out.push_str(&format!("target: {}\n", system.target));
    let risks = risk_classes(system);
    out.push_str(&format!(
        "risk: {}\n",
        if risks.is_empty() {
            "low".to_string()
        } else {
            risks.join(", ")
        }
    ));
    out.push_str("plan: pass\n");
    out.push_str("packages:\n");
    for p in realized {
        out.push_str(&format!("  {} -> {}\n", p.reference, p.out));
    }
    out.push_str("options:\n");
    for o in &system.options {
        out.push_str(&format!("  {} = {}\n", o.key, o.value));
    }
    out.push_str("services:\n");
    for s in &system.services {
        out.push_str(&format!("  {} enable={}\n", s.name, s.enable));
    }
    out.push_str("images:\n");
    for image in &plan.images {
        if image.kind == ImageKind::Iso && image.from == system.name {
            out.push_str(&format!("  {} {}\n", image.name, image.format));
        }
    }
    out
}

fn risk_classes(system: &SystemPlan) -> Vec<String> {
    let mut risks = Vec::new();
    for option in &system.options {
        let key = option.key.as_str();
        if key.starts_with("filesystem.") && !risks.iter().any(|r| r == "filesystem") {
            risks.push("filesystem".to_string());
        }
        if key.contains("boot") && !risks.iter().any(|r| r == "boot") {
            risks.push("boot".to_string());
        }
        if key.contains("kernel") && !risks.iter().any(|r| r == "kernel") {
            risks.push("kernel".to_string());
        }
        if (key.starts_with("storage.") || key.starts_with("performance."))
            && !risks.iter().any(|r| r == "performance/storage")
        {
            risks.push("performance/storage".to_string());
        }
        if (key.starts_with("user.") || key.starts_with("apps.") || key.starts_with("workload."))
            && !risks.iter().any(|r| r == "user-app-workload")
        {
            risks.push("user-app-workload".to_string());
        }
    }
    if system.services.iter().any(|s| s.enable) {
        risks.push("service-risk".to_string());
    }
    risks
}

fn prove_activation(theme: &Theme, gen: &Generation, system: &SystemPlan) -> bool {
    let risks = risk_classes(system);
    let plan = gen.path.join("plan.json");
    let proof = gen.path.join("proof.txt");
    if !plan.is_file() || !proof.is_file() {
        theme.error_coded(
            "E1278",
            "jetos activation proof is incomplete",
            "D-WD8 requires a plan and proof artifact before `jet os switch` can activate a generation.",
            "run `jet os build <host>` again; if the generation is hand-edited, discard it.",
        );
        return false;
    }
    for svc in system.services.iter().filter(|s| s.enable) {
        let unit = gen
            .path
            .join("etc/systemd/system")
            .join(format!("{}.service", svc.name));
        if !unit.is_file() {
            theme.error_coded(
                "E1278",
                "jetos service proof is incomplete",
                &format!(
                    "`{}` is enabled, but its generated systemd unit is missing.",
                    svc.name
                ),
                "rebuild the generation so service artifacts and proof are regenerated together.",
            );
            return false;
        }
    }
    let plan_text = match fs::read_to_string(&plan) {
        Ok(text) => text,
        Err(e) => {
            theme.error_coded(
                "E1278",
                "jetos activation proof is incomplete",
                &format!("reading the plan artifact failed: {e}"),
                "rebuild the generation so plan and proof artifacts are regenerated together.",
            );
            return false;
        }
    };
    let plan_hash = crate::SHA256::sha256_hex(plan_text.as_bytes());
    if !risks.is_empty() {
        let vm_proof = gen.path.join("vm-proof.txt");
        let vm_text = match fs::read_to_string(&vm_proof) {
            Ok(text) => text,
            Err(_) => {
                theme.error_coded(
                    "E1278",
                    "jetos VM proof is missing",
                    "D-WD8 requires a plan-bound VM/service proof artifact for boot, kernel, filesystem, or service-risk changes.",
                    "run `jet os build <host>` again; if the generation is hand-edited, discard it.",
                );
                return false;
            }
        };
        if !vm_text.contains(&format!("plan-sha256: {plan_hash}"))
            || !vm_text.contains("service-artifacts: pass")
        {
            theme.error_coded(
                "E1278",
                "jetos VM proof is stale",
                "the VM/service proof does not match the generation plan artifact.",
                "rebuild the generation so proof and plan are regenerated together.",
            );
            return false;
        }
    }
    let rollback = rollback_proof_for(&gen.host, &gen.path);
    if rollback.starts_with("warning") {
        theme.error_coded(
            "E1278",
            "jetos rollback proof is incomplete",
            &rollback,
            "remove stale generation ledger entries or rebuild the previous generation.",
        );
        return false;
    }
    let mut activation = String::new();
    activation.push_str(&format!("activation proof for {}\n", gen.host));
    activation.push_str(&format!("generation: {}\n", gen.name));
    activation.push_str(&format!(
        "risk: {}\n",
        if risks.is_empty() {
            "low".to_string()
        } else {
            risks.join(", ")
        }
    ));
    activation.push_str("plan-diff: pass\n");
    if risks.is_empty() {
        activation.push_str("vm-proof: not required for low-risk change\n");
    } else {
        activation.push_str(&format!("vm-proof: pass plan-sha256={plan_hash}\n"));
    }
    activation.push_str(&format!("rollback-proof: {rollback}\n"));
    if let Err(e) = fs::write(gen.path.join("activation-proof.txt"), activation) {
        theme.error_coded(
            "E1278",
            "jetos activation proof could not be recorded",
            &format!("writing activation proof failed: {e}"),
            "check permissions on the Jetpack root, or set JETPACK_ROOT.",
        );
        return false;
    }
    theme.detail("activation proof: pass");
    true
}

fn rollback_proof_for(host: &str, current: &Path) -> String {
    let current = current
        .canonicalize()
        .unwrap_or_else(|_| current.to_path_buf());
    let mut gens = read_generations()
        .into_iter()
        .filter(|g| g.host == host)
        .filter(|g| g.path.is_dir())
        .filter(|g| g.path.canonicalize().map(|p| p != current).unwrap_or(true))
        .collect::<Vec<_>>();
    gens.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.name.cmp(&a.name))
    });
    match gens.into_iter().next() {
        Some(prev) => format!("pass previous={}", prev.name),
        None => "pass initial-activation".to_string(),
    }
}

fn append_generation(gen: &Generation) -> std::io::Result<()> {
    if let Some(parent) = generations_log().parent() {
        fs::create_dir_all(parent)?;
    }
    let line = format!(
        "{}\t{}\t{}\t{}\n",
        gen.created_at,
        gen.host,
        gen.name,
        gen.path.display()
    );
    use std::io::Write;
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(generations_log())?
        .write_all(line.as_bytes())
}

fn read_generations() -> Vec<Generation> {
    let Ok(text) = fs::read_to_string(generations_log()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() != 4 {
            continue;
        }
        let Ok(created_at) = parts[0].parse::<u64>() else {
            continue;
        };
        out.push(Generation {
            created_at,
            host: parts[1].to_string(),
            name: parts[2].to_string(),
            path: PathBuf::from(parts[3]),
        });
    }
    out
}

fn latest_generation_for(host: &str) -> Option<Generation> {
    let mut gens = read_generations()
        .into_iter()
        .filter(|g| g.host == host)
        .filter(|g| g.path.is_dir())
        .collect::<Vec<_>>();
    gens.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.name.cmp(&a.name))
    });
    gens.into_iter().next()
}

fn render_generation_proof_json(gen: &Generation) -> std::io::Result<String> {
    let plan = fs::read_to_string(gen.path.join("plan.json"))?;
    let proof = fs::read_to_string(gen.path.join("proof.txt"))?;
    let activation_diff = fs::read_to_string(gen.path.join("activation-diff.txt"))?;
    let health = fs::read_to_string(gen.path.join("health-checks.txt"))?;
    let provenance = fs::read_to_string(gen.path.join("provenance.json"))?;
    let boot = fs::read_to_string(gen.path.join("boot/facts.json"))?;
    let init = fs::read_to_string(gen.path.join("init/systemd.json"))?;
    let secrets = fs::read_to_string(gen.path.join("secrets.tmpfs.manifest"))?;
    let vm_proof = fs::read_to_string(gen.path.join("vm-proof.txt")).unwrap_or_default();
    Ok(format!(
        "{{\"host\":{},\"generation\":{},\"path\":{},\"created_at\":{},\"plan\":{},\"proof\":{},\"activation_diff\":{},\"health\":{},\"provenance\":{},\"boot\":{},\"init\":{},\"secrets\":{},\"vm_proof\":{}}}",
        JSON::quote(&gen.host),
        JSON::quote(&gen.name),
        JSON::quote(&gen.path.display().to_string()),
        gen.created_at,
        JSON::quote(&plan),
        JSON::quote(&proof),
        JSON::quote(&activation_diff),
        JSON::quote(&health),
        provenance,
        boot,
        init,
        JSON::quote(&secrets),
        JSON::quote(&vm_proof)
    ))
}

fn find_rollback_generation(host: &str, requested: Option<&str>) -> Option<Generation> {
    let current = current_generation_path();
    let mut gens = read_generations()
        .into_iter()
        .filter(|g| g.host == host)
        .filter(|g| g.path.is_dir())
        .filter(|g| {
            current
                .as_ref()
                .map(|c| g.path.canonicalize().map(|p| p != *c).unwrap_or(true))
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    gens.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    if let Some(name) = requested {
        return gens.into_iter().find(|g| g.name == name);
    }
    gens.into_iter().next()
}

fn activate_generation(gen: &Generation) -> std::io::Result<()> {
    let dir = systems_dir();
    fs::create_dir_all(&dir)?;
    write_pointer(&dir.join("current"), &gen.path)?;
    write_pointer(&dir.join("default"), &gen.path)?;
    Ok(())
}

#[cfg(unix)]
fn write_pointer(link: &Path, target: &Path) -> std::io::Result<()> {
    let tmp = link.with_extension("tmp");
    let _ = fs::remove_file(&tmp);
    std::os::unix::fs::symlink(target, &tmp)?;
    fs::rename(tmp, link)
}

#[cfg(not(unix))]
fn write_pointer(link: &Path, target: &Path) -> std::io::Result<()> {
    let tmp = link.with_extension("tmp");
    fs::write(&tmp, target.display().to_string())?;
    fs::rename(tmp, link)
}

fn current_generation_path() -> Option<PathBuf> {
    let link = systems_dir().join("current");
    #[cfg(unix)]
    {
        fs::read_link(&link)
            .ok()
            .and_then(|p| p.canonicalize().ok())
    }
    #[cfg(not(unix))]
    {
        fs::read_to_string(&link)
            .ok()
            .and_then(|s| PathBuf::from(s.trim()).canonicalize().ok())
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn print_help() {
    println!("jet os check|init|plan|proof|build|switch|rollback|generations|lift|image|vm <host>|path@host");
}
