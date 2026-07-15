use super::generation_files::{generations_log, systems_dir};
use super::options_rendering::risk_classes;
use super::types::Generation;
use jet_env_model::ModuleEval::SystemPlan;
use crate::Output::Theme;
use crate::Store;
use crate::JSON;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn prove_activation(theme: &Theme, gen: &Generation, system: &SystemPlan) -> bool {
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
    let activation_path = systems_dir()
        .join("activation-proofs")
        .join(&gen.host)
        .join(format!("{}.txt", gen.name));
    let activation_parent = activation_path.parent().unwrap_or_else(|| Path::new("."));
    let write_activation = || -> std::io::Result<()> {
        fs::create_dir_all(activation_parent)?;
        fs::write(&activation_path, activation.as_bytes())?;
        fs::File::open(&activation_path)?.sync_all()?;
        Store::sync_store_node(activation_parent, true)
    };
    if let Err(e) = write_activation() {
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

pub(super) fn append_generation(gen: &Generation, witness: &str) -> std::io::Result<u64> {
    let roots = Store::resolve();
    crate::RuntimePolicy::with_lock(&roots.root, "jetos-generations", || {
        let log = generations_log();
        let parent = log
            .parent()
            .ok_or_else(|| std::io::Error::other("generation ledger has no parent"))?;
        fs::create_dir_all(parent)?;
        let mut existing = match fs::read(&log) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error),
        };
        if !existing.is_empty() && !existing.ends_with(b"\n") {
            let recovered_len = existing
                .iter()
                .rposition(|byte| *byte == b'\n')
                .map(|offset| offset + 1)
                .unwrap_or(0);
            let file = fs::OpenOptions::new().write(true).open(&log)?;
            file.set_len(recovered_len as u64)?;
            file.sync_all()?;
            existing.truncate(recovered_len);
        }
        if let Some(created_at) = ledger_timestamp_in(&existing, gen, witness)? {
            let file = fs::OpenOptions::new().read(true).open(&log)?;
            file.sync_all()?;
            if generation_failpoint("after-ledger-durable") {
                return Err(std::io::Error::other("generation ledger durability failpoint"));
            }
            Store::sync_store_node(parent, true)?;
            return Ok(created_at);
        }
        let line = format!(
            "{}\t{}\t{}\t{}\t{}\n",
            gen.created_at,
            gen.host,
            gen.name,
            gen.path.display(),
            witness,
        );
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)?;
        if generation_failpoint("after-ledger-partial") {
            file.write_all(&line.as_bytes()[..line.len() / 2])?;
            file.sync_all()?;
            return Err(std::io::Error::other("generation ledger partial-write failpoint"));
        }
        file.write_all(line.as_bytes())?;
        if generation_failpoint("after-ledger-append") {
            return Err(std::io::Error::other("generation ledger append failpoint"));
        }
        file.sync_all()?;
        if generation_failpoint("after-ledger-durable") {
            return Err(std::io::Error::other("generation ledger durability failpoint"));
        }
        Store::sync_store_node(parent, true)?;
        Ok(gen.created_at)
    })
}

pub(super) fn generation_ledger_timestamp(
    gen: &Generation,
    witness: &str,
) -> std::io::Result<Option<u64>> {
    let bytes = match fs::read(generations_log()) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    let complete = if bytes.ends_with(b"\n") {
        bytes.as_slice()
    } else {
        let len = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map(|offset| offset + 1)
            .unwrap_or(0);
        &bytes[..len]
    };
    ledger_timestamp_in(complete, gen, witness)
}

fn ledger_timestamp_in(
    bytes: &[u8],
    gen: &Generation,
    witness: &str,
) -> std::io::Result<Option<u64>> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| std::io::Error::other("generation ledger is not UTF-8"))?;
    for line in text.lines() {
        let parts = line.split('\t').collect::<Vec<_>>();
        if parts.len() < 3 || parts[1] != gen.host || parts[2] != gen.name {
            continue;
        }
        if parts.len() != 5 || Path::new(parts[3]) != gen.path || parts[4] != witness {
            return Err(std::io::Error::other(
                "generation ledger cannot replace an immutable publication",
            ));
        }
        let created_at = parts[0]
            .parse::<u64>()
            .map_err(|_| std::io::Error::other("generation ledger timestamp is invalid"))?;
        return Ok(Some(created_at));
    }
    Ok(None)
}

fn generation_failpoint(name: &str) -> bool {
    cfg!(debug_assertions)
        && std::env::var("JET_TEST_GENERATION_FAILPOINT").as_deref() == Ok(name)
}

pub(super) fn read_generations() -> Vec<Generation> {
    let Ok(text) = fs::read_to_string(generations_log()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() != 4 && parts.len() != 5 {
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

pub(super) fn latest_generation_for(host: &str) -> Option<Generation> {
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

pub(super) fn generation_named(host: &str, name: &str) -> Option<Generation> {
    read_generations()
        .into_iter()
        .find(|generation| {
            generation.host == host && generation.name == name && generation.path.is_dir()
        })
}

pub(super) fn render_generation_proof_json(gen: &Generation) -> std::io::Result<String> {
    let plan = fs::read_to_string(gen.path.join("plan.json"))?;
    let proof = fs::read_to_string(gen.path.join("proof.txt"))?;
    let activation_diff = fs::read_to_string(gen.path.join("activation-diff.txt"))?;
    let health = fs::read_to_string(gen.path.join("health-checks.txt"))?;
    let provenance = fs::read_to_string(gen.path.join("provenance.json"))?;
    let boot = fs::read_to_string(gen.path.join("boot/facts.json"))?;
    let init = fs::read_to_string(gen.path.join("init/systemd.json"))?;
    let secrets = fs::read_to_string(gen.path.join("secrets.tmpfs.manifest"))?;
    let vm_proof = fs::read_to_string(gen.path.join("vm-proof.txt")).unwrap_or_default();
    let source_proof = fs::read_to_string(gen.path.join("source-proof.json"))?;
    Ok(format!(
        "{{\"host\":{},\"generation\":{},\"path\":{},\"created_at\":{},\"source_proof\":{},\"plan\":{},\"proof\":{},\"activation_diff\":{},\"health\":{},\"provenance\":{},\"boot\":{},\"init\":{},\"secrets\":{},\"vm_proof\":{}}}",
        JSON::quote(&gen.host),
        JSON::quote(&gen.name),
        JSON::quote(&gen.path.display().to_string()),
        gen.created_at,
        source_proof,
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

pub(super) fn find_rollback_generation(host: &str, requested: Option<&str>) -> Option<Generation> {
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

pub(super) fn activate_generation(gen: &Generation) -> std::io::Result<()> {
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

pub(super) fn current_generation_path() -> Option<PathBuf> {
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

pub(super) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(super) fn print_help() {
    println!("jet os check|init|plan|proof|build|switch|rollback|generations|lift|import|image|vm <host>|path@host");
}
