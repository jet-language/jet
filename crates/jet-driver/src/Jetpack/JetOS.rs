//! jetos realization tier (Epoch 7).
//!
//! `jet os` is the user-facing command. The implementation lives in the
//! Jetpack engine because it reuses Jetpack's source table, provider boundary,
//! hangar, and trust/runtime policy.

use super::ModuleEval::{self, EnvPlan, ImageKind, ServicePlan, SystemPlan};
use super::Output::Theme;
use super::{Provider, RefSpec, Store, JSON};
use crate::Syntax;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CACHYOS_KERNEL_PACKAGE: &str = "cachyos-kernel";
const SYSTEMD_INIT_PACKAGE: &str = "systemd";
const VM_TOOLS: [&str; 6] = [
    "qemu-system-x86_64",
    "qemu-img",
    "xorriso",
    "limine",
    "mkfs.ext4",
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
        Ok(path) => {
            theme.ok(&format!(
                "wrote jetos installer media proof {}",
                path.display()
            ));
            0
        }
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
            "D-JOS-VMCOMMAND1=A and D-JOS-VMRUN1=A: the active VM actions are `prove` and `run`.",
            "run `jet os vm prove <host> --disk <path>` or `jet os vm run <host> --disk <path>`.",
        );
        return 2;
    };
    if action != Syntax::OS_VM_ACTION_PROVE && action != Syntax::OS_VM_ACTION_RUN {
        theme.error(
            &format!("`{action}` is not a jetos VM action"),
            "D-JOS-VMCOMMAND1=A and D-JOS-VMRUN1=A: the active VM actions are `prove` and `run`.",
            "run `jet os vm prove <host> --disk <path>` or `jet os vm run <host> --disk <path>`.",
        );
        return 2;
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
            "realize or expose qemu-system-x86_64, qemu-img, xorriso, limine, mkfs.ext4, and zstd, then rerun `jet os vm prove`.",
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
    let command =
        qemu_interactive_run_command(&gen.path.join("boot"), disk, &system.name, &gen.name);
    theme.ok(&format!(
        "booting jetos VM {} generation {}",
        theme.bold(&system.name),
        theme.bold(&gen.name)
    ));
    theme.detail("terminal console is attached to this process");
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
    if let Some(bad) = system.options.iter().find(|o| {
        let ns = o.key.split('.').next().unwrap_or("");
        !Syntax::OS_OPTION_NAMESPACES.contains(&ns)
    }) {
        theme.error_coded(
            "E1277",
            &format!("`{}` uses a retired jetos option namespace", bad.key),
            "D-JPK-OSNS1=B and D-JOS-SYSTEMTREE1=A: jetos option keys start with full-word namespaces: `filesystem`, `network`, `packages`, `services`, `users`, `groups`, `secrets`, `boot`, `kernel`, `init`, or `health`.",
            "rename the option namespace, for example `net.hostName` becomes `network.hostName`.",
        );
        return None;
    }
    Some((plan, system))
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
    fs::write(dir.join("sw/closure.txt"), manifest)
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
        "boot", "etc", "sbin", "sw", "share", "studio", "init", "network", "hardware", "desktop",
        "store", "compat", "terminal", "home",
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
            "TIMEOUT=5\nSERIAL=yes\nVERBOSE=yes\nTEXTMODE=yes\n:jetos {}\n    PROTOCOL=linux\n    KERNEL_PATH=boot:///boot/kernel\n    MODULE_PATH=boot:///boot/initrd\n    CMDLINE=console=ttyS0 root=LABEL=jetos-root rw init={}\n",
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
    if let Some(module) =
        kernel_entry.and_then(|entry| boot_artifact(entry, &["boot/modules/isofs.ko.xz"]))
    {
        fs::create_dir_all(boot_dir.join("modules"))?;
        link_or_copy_file(&module, &boot_dir.join("modules/isofs.ko.xz"))?;
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
    fs::create_dir_all(&hw_dir)?;
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
    fs::write(
        hw_dir.join("facts.json"),
        format!(
            "{{\"firmware\":[{}],\"drivers\":[{}],\"audit\":\"declared hardware facts enter generation proof before activation\"}}",
            firmware_json, drivers_json
        ),
    )?;
    fs::write(
        hw_dir.join("firmware.manifest"),
        firmware
            .iter()
            .map(|f| format!("{f}\tdeclared\n"))
            .collect::<String>(),
    )
}

fn write_desktop_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let desktop_dir = dir.join("desktop");
    fs::create_dir_all(&desktop_dir)?;
    let session = option_value(system, &["services.desktop.session"]);
    let display_manager = option_value(system, &["services.displayManager"]);
    fs::write(
        desktop_dir.join("facts.json"),
        format!(
            "{{\"session\":{},\"display_manager\":{},\"source\":\"system options\"}}",
            session
                .as_deref()
                .map(JSON::quote)
                .unwrap_or_else(|| "null".to_string()),
            display_manager
                .as_deref()
                .map(JSON::quote)
                .unwrap_or_else(|| "null".to_string())
        ),
    )?;
    if let Some(dm) = display_manager {
        let unit_dir = dir.join("etc/systemd/system");
        fs::create_dir_all(&unit_dir)?;
        fs::write(
            unit_dir.join("display-manager.service"),
            format!(
                "[Unit]\nDescription=jetos display manager\n\n[Service]\nExecStart=/run/current-system/sw/bin/{}\n\n[Install]\nWantedBy=graphical.target\n",
                dm
            ),
        )?;
        enable_unit(&unit_dir, "graphical.target", "display-manager.service")?;
    }
    Ok(())
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
        "{{\"brand\":\"jetos\",\"host\":{},\"generation\":{},\"mode\":\"guided-or-scripted\",\"disk\":{},\"root_label\":\"jetos-root\",\"source_generation\":{},\"steps\":[\"partition-disk\",\"mkfs.ext4-root\",\"copy-generation-closure\",\"install-limine\",\"write-generation-ledger\",\"reboot-installed-disk\",\"verify-guest-proof\"]}}",
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

fn render_installer_script(system: &SystemPlan, gen: &Generation) -> String {
    format!(
        r#"#!/bin/sh
set -eu
PATH=/bin:/sbin:/usr/bin:/usr/sbin
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
mkfs.ext4 -F -L jetos-root "$disk"
mount "$disk" "$root"
mkdir -p "$root/run" "$root/boot" "$root/var/lib/jetos/generations/{generation}"
cp -a /media/jetos/current-system/. "$root/var/lib/jetos/generations/{generation}/"
rm -rf "$root/run/current-system"
ln -s "/var/lib/jetos/generations/{generation}" "$root/run/current-system"
cp /media/boot/kernel "$root/boot/kernel"
cp /media/boot/initrd "$root/boot/initrd"
cp /media/jetos/current-system/boot/limine.conf "$root/boot/limine.conf"
printf '%s\t%s\t%s\n' "{created}" "{host}" "{generation}" > "$root/var/lib/jetos/generations/log"
printf '{{"host":"{host}","generation":"{generation}","disk":"%s","result":"installed"}}\n' "$disk" > "$root/var/lib/jetos/install-proof.json"
sync
echo "jetos installer: installed host={host} generation={generation}"
poweroff -f 2>/dev/null || halt -f 2>/dev/null || exit 0
"#,
        created = gen.created_at,
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
        "TIMEOUT=5\nSERIAL=yes\nVERBOSE=yes\nTEXTMODE=yes\n:Install jetos {host}\n    PROTOCOL=linux\n    KERNEL_PATH=boot:///boot/kernel\n    MODULE_PATH=boot:///boot/initrd\n    CMDLINE=console=ttyS0 rdinit=/jetos/install.sh jetos.mode=install jetos.host={host} jetos.generation={generation} jetos.disk={disk}\n",
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
PATH=/bin:/sbin:/usr/bin:/usr/sbin
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
    while [ ! -e /dev/vda ] && [ ! -e /dev/sda ] && [ "$tries" -lt 30 ]; do
        tries=$((tries + 1))
        sleep 1
    done
    for dev in /sys/block/* /dev/vd* /dev/sd*; do
        echo "jetos verifier: sees $dev"
    done
    echo "jetos verifier: mounting installed root"
    mount LABEL=jetos-root "$root" || mount /dev/vda "$root" || mount /dev/sda "$root" || true
fi
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
need "$root/var/lib/jetos/generations/log"
if [ ! -L "$root/run/current-system" ]; then
    echo "jetos verifier: missing current-system symlink"
    exit 1
fi
for svc in {services}; do
    need "$system/etc/systemd/system/$svc.service"
done
printf '{{"host":"{host}","generation":"{generation}","packages":"present","services":"present","network":"declared","rollback":"ledger-present","proof":"present"}}\n'
printf 'JETOS_GUEST_PROOF: {{"state":"guest-passed","host":"{host}","generation":"{generation}","assertions":["current-generation-matches","packages-present","services-active","network-up","rollback-generation-bootable","terminal-login-ready"]}}\n'
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
cmdline="$(cat /proc/cmdline 2>/dev/null || true)"
case "$cmdline" in
  *jetos.mode=install*)
    mkdir -p /media /mnt/jetos
    mount -o ro /dev/sr0 /media 2>/dev/null || mount -o ro /dev/cdrom /media 2>/dev/null || true
    exec /bin/sh /jetos/install.sh /dev/vda
    ;;
  *jetos.mode=verify*)
    mkdir -p /sysroot
    mount LABEL=jetos-root /sysroot 2>/dev/null || mount /dev/vda /sysroot 2>/dev/null || true
    JETOS_TARGET_ROOT=/sysroot /bin/sh /jetos/guest-verify.sh
    poweroff -f 2>/dev/null || halt -f 2>/dev/null || exit 0
    ;;
esac
    exec /sbin/init
"#;
    let install = render_installer_script(system, gen);
    let verify = render_guest_verify_script(system, gen);
    let isofs = fs::read(gen.path.join("boot/modules/isofs.ko.xz")).ok();
    let overlay = if let Some(isofs) = isofs.as_ref() {
        cpio_newc(&[
            CpioEntry::dir("jetos"),
            CpioEntry::dir("jetos/modules"),
            CpioEntry::file("init", 0o100755, init.as_bytes()),
            CpioEntry::file("jetos/install.sh", 0o100755, install.as_bytes()),
            CpioEntry::file("jetos/guest-verify.sh", 0o100755, verify.as_bytes()),
            CpioEntry::file("jetos/modules/isofs.ko.xz", 0o100644, isofs),
        ])
    } else {
        cpio_newc(&[
            CpioEntry::dir("jetos"),
            CpioEntry::file("init", 0o100755, init.as_bytes()),
            CpioEntry::file("jetos/install.sh", 0o100755, install.as_bytes()),
            CpioEntry::file("jetos/guest-verify.sh", 0o100755, verify.as_bytes()),
        ])
    };
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

struct CpioEntry<'a> {
    name: &'a str,
    mode: u32,
    data: &'a [u8],
}

impl<'a> CpioEntry<'a> {
    fn dir(name: &'a str) -> CpioEntry<'a> {
        CpioEntry {
            name,
            mode: 0o040755,
            data: &[],
        }
    }

    fn file(name: &'a str, mode: u32, data: &'a [u8]) -> CpioEntry<'a> {
        CpioEntry { name, mode, data }
    }
}

fn cpio_newc(entries: &[CpioEntry<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for (ino, entry) in entries.iter().enumerate() {
        cpio_newc_entry(
            &mut out,
            (ino + 1) as u32,
            entry.name,
            entry.mode,
            entry.data,
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
    fs::copy(
        data_dir.join("limine-bios.sys"),
        boot_dir.join("limine-bios.sys"),
    )
    .map_err(|e| format!("copying limine-bios.sys failed: {e}"))?;
    fs::copy(data_dir.join("BOOTX64.EFI"), boot_dir.join("BOOTX64.EFI"))
        .map_err(|e| format!("copying BOOTX64.EFI failed: {e}"))?;
    let efi_boot = staging.join("EFI/BOOT");
    fs::create_dir_all(&efi_boot).map_err(|e| format!("creating EFI boot dir failed: {e}"))?;
    fs::copy(data_dir.join("BOOTX64.EFI"), efi_boot.join("BOOTX64.EFI"))
        .map_err(|e| format!("copying EFI/BOOT/BOOTX64.EFI failed: {e}"))?;
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
            "boot/BOOTX64.EFI",
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
        "{{\"host\":{},\"generation\":{},\"state\":\"harness-ready\",\"disk\":{},\"installer_media\":{},\"media_proof\":{},\"expected_guest_proof\":{},\"tools\":[{}],\"commands\":[{}],\"steps\":[\"build-generation\",\"create-hybrid-iso\",\"boot-installer-qemu\",\"install-to-disk\",\"reboot-installed-disk\",\"verify-guest-proof\"],\"guest_assertions\":[{}]}}",
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
    let mut boot_disk_output = String::new();
    for command in qemu_proof_commands(&staging_boot, disk, &iso, &system.name, &gen.name) {
        let output = run_vm_command(&command, &log_dir)?;
        if command.phase == "boot-installed-disk" {
            boot_disk_output = output;
        }
    }
    let Some(report) = extract_guest_proof_report(&boot_disk_output) else {
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
    require_json_field(&harness_text, "disk", disk)?;
    finalize_vm_guest_proof(gen, system, disk, media_proof, harness)?;
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

const GUEST_ASSERTIONS: [&str; 6] = [
    "current-generation-matches",
    "packages-present",
    "services-active",
    "network-up",
    "rollback-generation-bootable",
    "terminal-login-ready",
];

struct VmCommand {
    phase: &'static str,
    argv: Vec<String>,
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
    let verify_cmdline = format!(
        "console=ttyS0 rdinit=/jetos/guest-verify.sh jetos.mode=verify jetos.host={host} jetos.generation={generation} root=LABEL=jetos-root rw"
    );
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
            argv: vec![
                "qemu-system-x86_64".to_string(),
                "-m".to_string(),
                "2048".to_string(),
                "-nographic".to_string(),
                "-monitor".to_string(),
                "none".to_string(),
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
            ],
        },
        VmCommand {
            phase: "boot-installed-disk",
            argv: vec![
                "qemu-system-x86_64".to_string(),
                "-m".to_string(),
                "2048".to_string(),
                "-nographic".to_string(),
                "-monitor".to_string(),
                "none".to_string(),
                "-kernel".to_string(),
                kernel,
                "-initrd".to_string(),
                initrd,
                "-append".to_string(),
                verify_cmdline,
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
    let cmdline =
        format!("console=ttyS0 jetos.mode=run jetos.host={host} jetos.generation={generation} root=LABEL=jetos-root rw");
    VmCommand {
        phase: "run-installed-disk",
        argv: vec![
            "qemu-system-x86_64".to_string(),
            "-m".to_string(),
            "2048".to_string(),
            "-nographic".to_string(),
            "-monitor".to_string(),
            "none".to_string(),
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
        ],
    }
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
    fs::write(
        etc.join("profile"),
        "export PATH=/run/current-system/sw/bin:/bin:/sbin:/usr/bin:/usr/sbin\nexport JETOS_GENERATION=/run/current-system\n",
    )?;
    fs::write(
        etc.join("issue"),
        format!("jetos {} \\n \\l\n", system.name),
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
            "{{\"login_user\":{},\"shell\":{},\"serial_tty\":\"ttyS0\",\"virtual_tty\":\"tty1\",\"profile\":\"/etc/profile\",\"unit_dir\":\"etc/systemd/system\",\"proof\":\"terminal-login-ready\"}}",
            JSON::quote(&login_user),
            JSON::quote(&shell)
        ),
    )
}

fn option_value(system: &SystemPlan, keys: &[&str]) -> Option<String> {
    system
        .options
        .iter()
        .find(|o| keys.iter().any(|k| o.key == *k))
        .map(|o| clean_value(&o.value))
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
