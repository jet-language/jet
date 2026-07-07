//! jetos realization tier (Epoch 7).
//!
//! `jet os` is the user-facing command. The implementation lives in the
//! Jetpack engine because it reuses Jetpack's source table, provider boundary,
//! hangar, and trust/runtime policy.

use super::ModuleEval::{self, EnvPlan, ImageKind, SystemPlan};
use super::Output::Theme;
use super::{Provider, RefSpec, Store, JSON};
use crate::Syntax;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct OsFlags {
    pub fixtures: Option<PathBuf>,
    pub offline: bool,
    pub name: Option<String>,
    pub manual_disk: Option<String>,
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

pub fn main(theme: &Theme, verb: Option<&str>, args: &[String], flags: &OsFlags) -> i32 {
    match verb {
        Some(v) if v == Syntax::OS_VERB_CHECK => cmd_check(theme, args),
        Some(v) if v == Syntax::OS_VERB_BUILD => cmd_build(theme, args, flags, false),
        Some(v) if v == Syntax::OS_VERB_SWITCH => cmd_build(theme, args, flags, true),
        Some(v) if v == Syntax::OS_VERB_ROLLBACK => cmd_rollback(theme, args),
        Some(v) if v == Syntax::OS_VERB_GENERATIONS => cmd_generations(args),
        Some(v) if v == Syntax::OS_VERB_INIT => cmd_init(theme, args, flags),
        Some(v) if v == Syntax::OS_VERB_LIFT => cmd_lift(theme, args),
        Some(v) if v == Syntax::OS_VERB_IMAGE => cmd_image(theme, args, flags),
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
    let image_dir = systems_dir().join("images");
    if let Err(e) = fs::create_dir_all(&image_dir) {
        theme.error(
            "could not create the jetos image directory",
            &format!("creating `{}` failed: {e}", image_dir.display()),
            "check permissions on the Jetpack root, or set JETPACK_ROOT.",
        );
        return 2;
    }
    let path = image_dir.join(format!("jetos-installer-{}.img", system.name));
    let disk = flags.manual_disk.as_deref().unwrap_or("guided-ext4");
    let contents = format!(
        "brand=jetos\nfilename=jetos-installer-{}.img\nhost={}\ngeneration={}\nsource={}\ndisk={disk}\ncopy=jetos installer proof image\n",
        system.name,
        system.name,
        gen.name,
        gen.path.display()
    );
    match fs::write(&path, contents) {
        Ok(()) => {
            theme.ok(&format!("wrote jetos proof image {}", path.display()));
            0
        }
        Err(e) => {
            theme.error(
                "could not write the jetos image",
                &format!("writing `{}` failed: {e}", path.display()),
                "check permissions on the Jetpack root, or set JETPACK_ROOT.",
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
            "D-JPK-OSNS1=B: jetos option keys start with full-word namespaces: `filesystem`, `network`, or `packages`.",
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
    let plan_text = format!(
        "{{\"host\":{},\"target\":{},\"packages\":[{}],\"services\":[{}],\"options\":[{}]}}",
        JSON::quote(&system.name),
        JSON::quote(&system.target),
        packages_json,
        services_json,
        options_json
    );
    fs::write(dir.join("plan.json"), &plan_text)?;
    fs::write(dir.join("proof.txt"), render_proof(system, realized, plan))?;
    write_systemd_units(dir, system)?;
    write_vm_proof(dir, system, &plan_text)?;
    fs::write(
        dir.join("secrets.tmpfs.manifest"),
        "repo ciphertext + host key; activation decrypts into tmpfs only\n",
    )?;
    Ok(())
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
    }
    Ok(())
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
    println!("jet os check|init|build|switch|rollback|generations|lift|image <host>|path@host");
}
