use super::activation_provenance::{
    write_activation_diff, write_health_checks, write_provenance, write_systemd_units,
    write_terminal_environment,
};
use super::desktop_store_vm::{
    write_acceptance_fixture, write_compat_escape_hatches, write_desktop_facts,
    write_store_cache_facts, write_vm_proof,
};
use super::etc_boot_facts::{write_boot_facts, write_etc_tree};
use super::generations_activation::{now_secs, read_generations};
use super::initrd_overlay::ldd_dependency_paths;
use super::module_storage_workload::{
    write_module_priority_facts, write_storage_facts, write_workload_facts,
};
use super::options_rendering::{boot_profile, render_proof};
use super::root_projection::{copy_file_replace, write_bootable_root_projection};
use super::store_realize::RealizedPackage;
use super::types::OsFlags;
use super::studio_projection::{make_executable, write_studio_app_projection};
use super::system_facts::{
    write_hardware_facts, write_init_facts, write_network_facts, write_secret_manifest,
    write_systemd_timer_socket_units,
};
use super::theme_fleet_lifecycle::{
    write_app_module_facts, write_fleet_deploy_facts, write_image_variant_facts,
    write_lifecycle_facts, write_options_reference, write_service_manager_depth,
    write_theme_facts,
};
use super::user_flatpak_perf::{
    write_flatpak_facts, write_performance_facts, write_user_environment_facts,
};
use jet_env_model::AST::{Expr, Item, StrPart};
use jet_env_model::ModuleEval::{EnvPlan, SystemPlan};
use jet_env_model::{Lexer, Parser, Syntax};
use crate::Store;
use crate::JSON;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct GenerationRootProof {
    pub(super) witness: String,
    pub(super) output_digests: Vec<String>,
}

pub(super) fn generation_dir(system: &SystemPlan, explicit: Option<&str>) -> PathBuf {
    let name = explicit
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}-{}", system.name, now_secs()));
    systems_dir().join("generations").join(name)
}

pub(super) fn systems_dir() -> PathBuf {
    Store::resolve().root.join("systems")
}

pub(super) fn generations_log() -> PathBuf {
    systems_dir().join("generations.log")
}

pub(super) fn write_generation_files(
    dir: &Path,
    published_dir: &Path,
    system: &SystemPlan,
    realized: &[RealizedPackage],
    plan: &EnvPlan,
) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let packages_json = realized
        .iter()
        .map(|p| {
            JSON::object_of(&[
                ("name", &p.name),
                ("version", &p.version),
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
    write_root_closure(dir, realized).map_err(|error| {
        std::io::Error::new(
            error.kind(),
            format!("writing the owned runtime closure failed: {error}"),
        )
    })?;
    generation_stage("etc tree", write_etc_tree(dir, system))?;
    generation_stage("network facts", write_network_facts(dir, system))?;
    generation_stage("boot facts", write_boot_facts(dir, system, realized))?;
    generation_stage("init facts", write_init_facts(dir, system, realized))?;
    fs::write(dir.join("proof.txt"), render_proof(system, realized, plan))?;
    generation_stage("systemd units", write_systemd_units(dir, system))?;
    generation_stage(
        "systemd timer/socket units",
        write_systemd_timer_socket_units(dir, system),
    )?;
    generation_stage("terminal environment", write_terminal_environment(dir, system))?;
    generation_stage(
        "activation diff",
        write_activation_diff(dir, published_dir, system, realized),
    )?;
    generation_stage("health checks", write_health_checks(dir, system))?;
    generation_stage("hardware facts", write_hardware_facts(dir, system))?;
    generation_stage(
        "user environment facts",
        write_user_environment_facts(dir, system),
    )?;
    generation_stage("flatpak facts", write_flatpak_facts(dir, system))?;
    generation_stage("performance facts", write_performance_facts(dir, system))?;
    generation_stage(
        "module priority facts",
        write_module_priority_facts(dir, system),
    )?;
    generation_stage("storage facts", write_storage_facts(dir, system))?;
    generation_stage("workload facts", write_workload_facts(dir, system))?;
    generation_stage("theme facts", write_theme_facts(dir, system))?;
    generation_stage("fleet deploy facts", write_fleet_deploy_facts(dir, system, plan))?;
    generation_stage("options reference", write_options_reference(dir, system))?;
    generation_stage(
        "image variant facts",
        write_image_variant_facts(dir, system, plan),
    )?;
    generation_stage("lifecycle facts", write_lifecycle_facts(dir, system))?;
    generation_stage(
        "service manager facts",
        write_service_manager_depth(dir, system),
    )?;
    generation_stage("app module facts", write_app_module_facts(dir, system))?;
    generation_stage("acceptance fixture", write_acceptance_fixture(dir, system))?;
    generation_stage("desktop facts", write_desktop_facts(dir, system))?;
    generation_stage("store cache facts", write_store_cache_facts(dir, realized))?;
    generation_stage(
        "compatibility facts",
        write_compat_escape_hatches(dir, system),
    )?;
    generation_stage("provenance", write_provenance(dir, system, realized))?;
    generation_stage("VM proof", write_vm_proof(dir, system, &plan_text))?;
    generation_stage(
        "Studio projection",
        write_studio_app_projection(dir, published_dir, system),
    )?;
    generation_stage("secret manifest", write_secret_manifest(dir, system))?;
    generation_stage(
        "bootable root projection",
        write_bootable_root_projection(dir, published_dir),
    )?;
    Ok(())
}

fn generation_stage(stage: &str, result: std::io::Result<()>) -> std::io::Result<()> {
    result.map_err(|error| {
        std::io::Error::new(
            error.kind(),
            format!("writing generation {stage} failed: {error}"),
        )
    })
}

const EVALUATOR_SEMANTICS: &str = "jet-env-model.module-eval.v1";

pub(super) fn write_generation_source_proof(
    dir: &Path,
    config: &Path,
    flags: &OsFlags,
) -> std::io::Result<()> {
    let source = fs::read(config)?;
    let source_closure = generation_source_closure(config)?;
    let plan = fs::read(dir.join("plan.json"))?;
    let input_plan = std::env::var("JETOS_STUDIO_INPUT_PLAN_SHA256").unwrap_or_default();
    let proof = format!(
        "{{\"kind\":\"jetos.generation-source-proof\",\"source_sha256\":{},\"source_closure_sha256\":{},\"input_plan_sha256\":{},\"plan_sha256\":{},\"real_tier\":{},\"evaluator_semantics\":{}}}",
        JSON::quote(&crate::SHA256::sha256_hex(&source)),
        JSON::quote(&crate::SHA256::sha256_hex(&source_closure)),
        JSON::quote(&input_plan),
        JSON::quote(&crate::SHA256::sha256_hex(&plan)),
        if flags.real_tier { "true" } else { "false" },
        JSON::quote(EVALUATOR_SEMANTICS),
    );
    fs::write(dir.join("source-proof.json"), proof)
}

/// Seal complete generation inputs before its durable ledger transaction.
/// The manifest excludes only its own two proof files, avoiding self-hashing;
/// every other directory, file, symlink target, mode, and byte digest is bound.
pub(super) fn write_generation_root_proof(
    dir: &Path,
    host: &str,
    name: &str,
    realized: &[RealizedPackage],
    roots: &Store::Roots,
) -> std::io::Result<GenerationRootProof> {
    let manifest = generation_files_manifest(dir)?;
    fs::write(dir.join("generation-files.proof"), &manifest)?;
    let source_proof = fs::read(dir.join("source-proof.json"))?;
    let plan = fs::read(dir.join("plan.json"))?;
    let mut output_digests = realized
        .iter()
        .flat_map(|package| package.output_digests(roots))
        .collect::<Vec<_>>();
    output_digests.sort();
    output_digests.dedup();
    let witness = generation_root_witness(
        host,
        name,
        &source_proof,
        &plan,
        &manifest,
        &output_digests,
    );
    write_generation_root_metadata(
        dir,
        host,
        name,
        &witness,
        &source_proof,
        &plan,
        &manifest,
        &output_digests,
    )?;
    Ok(GenerationRootProof {
        witness,
        output_digests,
    })
}

pub(super) fn validate_generation_root_proof(
    dir: &Path,
    host: &str,
    name: &str,
    source_config: &Path,
    roots: &Store::Roots,
    flags: &OsFlags,
) -> std::io::Result<GenerationRootProof> {
    let root_metadata = fs::symlink_metadata(dir)?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return Err(invalid_generation("published generation root is not an owned directory"));
    }
    let stored_manifest = fs::read(dir.join("generation-files.proof"))?;
    let manifest = generation_files_manifest(dir)?;
    if manifest != stored_manifest {
        return Err(invalid_generation("generation files proof does not match the sealed tree"));
    }
    let source_proof = fs::read(dir.join("source-proof.json"))?;
    let plan = fs::read(dir.join("plan.json"))?;
    validate_source_proof(&source_proof, &plan, source_config, flags)?;
    let root_text = fs::read_to_string(dir.join("generation-root.json"))?;
    let root = JSON::parse(&root_text).map_err(invalid_generation)?;
    for (key, expected) in [
        ("kind", "jetos.generation-root.v1"),
        ("host", host),
        ("generation", name),
    ] {
        if json_string(&root, key)? != expected {
            return Err(invalid_generation("generation root identity does not match its path"));
        }
    }
    for (key, expected) in [
        ("source_proof_sha256", crate::SHA256::sha256_hex(&source_proof)),
        ("plan_sha256", crate::SHA256::sha256_hex(&plan)),
        ("files_proof_sha256", crate::SHA256::sha256_hex(&manifest)),
    ] {
        if json_string(&root, key)? != expected {
            return Err(invalid_generation("generation root hash does not match its durable proof"));
        }
    }
    let mut output_digests = root
        .get("output_digests")
        .map_err(invalid_generation)?
        .as_array()
        .map_err(invalid_generation)?
        .iter()
        .map(|value| value.as_str().map(str::to_string).map_err(invalid_generation))
        .collect::<std::io::Result<Vec<_>>>()?;
    let original = output_digests.clone();
    output_digests.sort();
    output_digests.dedup();
    if output_digests != original {
        return Err(invalid_generation("generation Hangar targets are not canonical and sorted"));
    }
    for digest in &output_digests {
        validate_hangar_digest(roots, digest)?;
    }
    let witness = generation_root_witness(
        host,
        name,
        &source_proof,
        &plan,
        &manifest,
        &output_digests,
    );
    if json_string(&root, "witness")? != witness {
        return Err(invalid_generation("generation root witness does not match its proofs"));
    }
    Ok(GenerationRootProof {
        witness,
        output_digests,
    })
}

fn generation_root_witness(
    host: &str,
    name: &str,
    source_proof: &[u8],
    plan: &[u8],
    manifest: &[u8],
    output_digests: &[String],
) -> String {
    let mut canonical = Vec::new();
    frame(&mut canonical, b"jetos-generation-root-v1");
    frame(&mut canonical, host.as_bytes());
    frame(&mut canonical, name.as_bytes());
    frame(&mut canonical, source_proof);
    frame(&mut canonical, plan);
    frame(&mut canonical, manifest);
    for digest in output_digests {
        frame(&mut canonical, digest.as_bytes());
    }
    format!("sha256-{}", crate::SHA256::sha256_hex(&canonical))
}

fn write_generation_root_metadata(
    dir: &Path,
    host: &str,
    name: &str,
    witness: &str,
    source_proof: &[u8],
    plan: &[u8],
    manifest: &[u8],
    output_digests: &[String],
) -> std::io::Result<()> {
    let digests = output_digests
        .iter()
        .map(|digest| JSON::quote(digest))
        .collect::<Vec<_>>()
        .join(",");
    let root = format!(
        "{{\"kind\":\"jetos.generation-root.v1\",\"host\":{},\"generation\":{},\"witness\":{},\"source_proof_sha256\":{},\"plan_sha256\":{},\"files_proof_sha256\":{},\"output_digests\":[{}]}}",
        JSON::quote(host),
        JSON::quote(name),
        JSON::quote(witness),
        JSON::quote(&crate::SHA256::sha256_hex(&source_proof)),
        JSON::quote(&crate::SHA256::sha256_hex(&plan)),
        JSON::quote(&crate::SHA256::sha256_hex(&manifest)),
        digests,
    );
    fs::write(dir.join("generation-root.json"), root)
}

fn validate_source_proof(
    source_proof: &[u8],
    plan: &[u8],
    source_config: &Path,
    flags: &OsFlags,
) -> std::io::Result<()> {
    let proof_text = std::str::from_utf8(source_proof).map_err(invalid_generation)?;
    let proof = JSON::parse(proof_text).map_err(invalid_generation)?;
    let source = fs::read(source_config)?;
    let source_closure = generation_source_closure(source_config)?;
    let expected_input = std::env::var("JETOS_STUDIO_INPUT_PLAN_SHA256").unwrap_or_default();
    for (key, expected) in [
        ("kind", "jetos.generation-source-proof".to_string()),
        ("source_sha256", crate::SHA256::sha256_hex(&source)),
        (
            "source_closure_sha256",
            crate::SHA256::sha256_hex(&source_closure),
        ),
        ("input_plan_sha256", expected_input),
        ("plan_sha256", crate::SHA256::sha256_hex(plan)),
        ("evaluator_semantics", EVALUATOR_SEMANTICS.to_string()),
    ] {
        if json_string(&proof, key)? != expected {
            return Err(invalid_generation("generation source proof does not match current input"));
        }
    }
    let real_tier = match proof.get("real_tier").map_err(invalid_generation)? {
        JSON::Json::Bool(value) => *value,
        _ => return Err(invalid_generation("generation source tier is not boolean")),
    };
    if real_tier != flags.real_tier {
        return Err(invalid_generation("generation source proof uses a different realization tier"));
    }
    Ok(())
}

fn generation_source_closure(config: &Path) -> std::io::Result<Vec<u8>> {
    let source = fs::read_to_string(config)?;
    let (tokens, diagnostics) = Lexer::lex(&source);
    if !diagnostics.is_empty() {
        return Err(invalid_generation("generation source closure does not lex"));
    }
    let program = Parser::parse(&tokens).map_err(|_| invalid_generation("generation source closure does not parse"))?;
    let source_base = std::env::var_os("JETOS_STUDIO_SOURCE_BASE").map(PathBuf::from);
    let base = source_base
        .as_deref()
        .unwrap_or_else(|| config.parent().unwrap_or_else(|| Path::new(".")));
    let mut files = vec![(PathBuf::from("<root>"), source.into_bytes())];
    for item in &program.items {
        let Item::Module(module) = item else {
            continue;
        };
        if !module.is_auto_discovered() {
            continue;
        }
        for import in &module.imports {
            let rel = import_find_directory(import)?;
            let directory = base.join(rel);
            let mut entries = fs::read_dir(&directory)?.collect::<Result<Vec<_>, _>>()?;
            entries.sort_by_key(|entry| path_bytes(&entry.path()));
            for entry in entries {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some(Syntax::FILE_EXT) {
                    continue;
                }
                let relative = path
                    .strip_prefix(base)
                    .map_err(|_| invalid_generation("imported source escaped its evaluation root"))?
                    .to_path_buf();
                files.push((relative, fs::read(path)?));
            }
        }
    }
    files.sort_by(|a, b| path_bytes(&a.0).cmp(&path_bytes(&b.0)));
    let mut canonical = Vec::new();
    frame(&mut canonical, b"jetos-source-closure-v1");
    for (path, bytes) in files {
        frame(&mut canonical, &path_bytes(&path));
        frame(&mut canonical, &bytes);
    }
    Ok(canonical)
}

fn import_find_directory(import: &Expr) -> std::io::Result<String> {
    let Expr::Call(call) = import else {
        return Err(invalid_generation("generation import is not find(...)"));
    };
    if call.name != Syntax::BUILTIN_FIND || call.args.len() != 1 {
        return Err(invalid_generation("generation import is not find(...)"));
    }
    let Expr::Str(parts, _) = &call.args[0].expr else {
        return Err(invalid_generation("generation import path is not literal"));
    };
    let mut path = String::new();
    for part in parts {
        match part {
            StrPart::Lit(value) => path.push_str(value),
            StrPart::Interp(..) => {
                return Err(invalid_generation("generation import path is interpolated"));
            }
        }
    }
    Ok(path)
}

fn json_string(value: &JSON::Json, key: &str) -> std::io::Result<String> {
    value
        .get(key)
        .map_err(invalid_generation)?
        .as_str()
        .map(str::to_string)
        .map_err(invalid_generation)
}

fn validate_hangar_digest(roots: &Store::Roots, digest: &str) -> std::io::Result<()> {
    if digest.len() != 71
        || !digest.starts_with("sha256-")
        || !digest[7..].bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(invalid_generation("generation contains a non-canonical Hangar digest"));
    }
    let object_root = roots.hangar_dir().join("objects");
    let object = object_root.join(digest);
    let metadata = fs::symlink_metadata(&object)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid_generation("generation Hangar target is not an owned object"));
    }
    let canonical_root = fs::canonicalize(&object_root)?;
    let canonical_object = fs::canonicalize(&object)?;
    if !canonical_object.starts_with(&canonical_root) || !canonical_object.is_dir() {
        return Err(invalid_generation("generation Hangar target is not an owned object"));
    }
    Ok(())
}

fn invalid_generation(error: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
}

pub(super) fn sync_generation_tree(root: &Path) -> std::io::Result<()> {
    let mut entries = fs::read_dir(root)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| path_bytes(&entry.path()));
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_dir() {
            sync_generation_tree(&path)?;
        } else if metadata.is_file() {
            Store::sync_store_node(&path, false)?;
        }
    }
    Store::sync_store_node(root, true)
}

fn frame(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    out.extend_from_slice(bytes);
}

fn generation_files_manifest(root: &Path) -> std::io::Result<Vec<u8>> {
    let mut paths = Vec::new();
    collect_generation_paths(root, root, &mut paths)?;
    paths.sort_by(|a, b| path_bytes(a).cmp(&path_bytes(b)));
    let mut out = Vec::new();
    for relative in paths {
        if relative == Path::new("generation-files.proof")
            || relative == Path::new("generation-root.json")
        {
            continue;
        }
        let path = root.join(&relative);
        let metadata = fs::symlink_metadata(&path)?;
        let kind = if metadata.file_type().is_symlink() {
            b'l'
        } else if metadata.is_dir() {
            b'd'
        } else if metadata.is_file() {
            b'f'
        } else {
            return Err(std::io::Error::other(
                "generation contains an unsupported filesystem object",
            ));
        };
        out.push(kind);
        frame(&mut out, &path_bytes(&relative));
        frame(&mut out, &generation_mode(&metadata).to_be_bytes());
        if kind == b'f' {
            frame(
                &mut out,
                crate::SHA256::sha256_hex(&fs::read(&path)?).as_bytes(),
            );
        } else if kind == b'l' {
            frame(&mut out, &path_bytes(&fs::read_link(&path)?));
        } else {
            frame(&mut out, &[]);
        }
    }
    Ok(out)
}

fn collect_generation_paths(
    root: &Path,
    dir: &Path,
    out: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| path_bytes(&entry.path()));
    for entry in entries {
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| std::io::Error::other("generation member escaped its root"))?
            .to_path_buf();
        out.push(relative);
        if fs::symlink_metadata(&path)?.is_dir() {
            collect_generation_paths(root, &path, out)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(windows)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(unix)]
fn generation_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    metadata.mode()
}

#[cfg(windows)]
fn generation_mode(metadata: &fs::Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}

#[cfg(test)]
mod request_proof_tests {
    use super::*;

    fn flags(real_tier: bool) -> OsFlags {
        OsFlags {
            fixtures: None,
            offline: true,
            name: Some("same-name".to_string()),
            manual_disk: None,
            disk: None,
            json: false,
            assume_yes: false,
            host: None,
            real_tier,
        }
    }

    #[test]
    fn source_proof_rejects_same_name_across_realization_tiers() {
        let root = std::env::temp_dir().join(format!(
            "jetos-request-proof-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let config = root.join("config.jet");
        fs::write(&config, "module empty {}\n").unwrap();
        fs::write(root.join("plan.json"), "{}\n").unwrap();
        write_generation_source_proof(&root, &config, &flags(false)).unwrap();
        let proof = fs::read(root.join("source-proof.json")).unwrap();
        let plan = fs::read(root.join("plan.json")).unwrap();
        validate_source_proof(&proof, &plan, &config, &flags(false)).unwrap();
        assert!(validate_source_proof(&proof, &plan, &config, &flags(true)).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}

pub(super) fn render_plan_json(
    system: &SystemPlan,
    realized: &[RealizedPackage],
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
                ("version", &p.version),
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

/// One realized package as recorded in a generation's `plan.json` — the
/// facts `jetos switch`'s tier-3 diff needs (D-FE-CLI1). `version` is empty
/// for generations written before the field existed; those packages never
/// produce a `~` change row (there is nothing honest to compare against),
/// only `+`/`-` if the name itself appears or disappears.
pub(super) struct PackageSnapshot {
    name: String,
    version: String,
    out: String,
}

/// Read the realized package set out of a generation directory's
/// `plan.json`. Missing/unreadable/malformed files read as no packages
/// (the diff then reads as "everything added", which is honest for a
/// first-ever generation).
pub(super) fn read_generation_packages(dir: &Path) -> Vec<PackageSnapshot> {
    let Ok(text) = fs::read_to_string(dir.join("plan.json")) else {
        return Vec::new();
    };
    let Ok(root) = JSON::parse(&text) else {
        return Vec::new();
    };
    let Ok(packages) = root.get("packages") else {
        return Vec::new();
    };
    let Ok(packages) = packages.as_array() else {
        return Vec::new();
    };
    packages
        .iter()
        .filter_map(|p| {
            let obj = p.as_object().ok()?;
            let name = obj.get("name")?.as_str().ok()?.to_string();
            let version = obj
                .get("version")
                .and_then(|v| v.as_str().ok())
                .unwrap_or("")
                .to_string();
            let out = obj
                .get("out")
                .and_then(|v| v.as_str().ok())
                .unwrap_or("")
                .to_string();
            Some(PackageSnapshot { name, version, out })
        })
        .collect()
}

/// One row of a `jetos switch` plan: a package added, changed (version
/// bump), or removed between the outgoing and incoming generation.
pub(super) struct PackageDiff {
    pub(super) name: String,
    pub(super) mark: crate::Output::PlanMark,
    pub(super) from: String,
    pub(super) to: String,
    /// The incoming store path, for `Download` size accounting. `None` for
    /// removed packages (freeing space downloads nothing).
    pub(super) out: Option<String>,
}

/// Diff two generations' package snapshots by name. Sorted by name so the
/// plan reads as a stable table run to run.
pub(super) fn diff_packages(old: &[PackageSnapshot], new: &[PackageSnapshot]) -> Vec<PackageDiff> {
    let mut rows = Vec::new();
    for pkg in new {
        match old.iter().find(|o| o.name == pkg.name) {
            None => rows.push(PackageDiff {
                name: pkg.name.clone(),
                mark: crate::Output::PlanMark::Add,
                from: "—".to_string(),
                to: pkg.version.clone(),
                out: Some(pkg.out.clone()),
            }),
            Some(old_pkg) if old_pkg.version != pkg.version => rows.push(PackageDiff {
                name: pkg.name.clone(),
                mark: crate::Output::PlanMark::Change,
                from: old_pkg.version.clone(),
                to: pkg.version.clone(),
                out: Some(pkg.out.clone()),
            }),
            _ => {}
        }
    }
    for pkg in old {
        if !new.iter().any(|n| n.name == pkg.name) {
            rows.push(PackageDiff {
                name: pkg.name.clone(),
                mark: crate::Output::PlanMark::Remove,
                from: pkg.version.clone(),
                to: "—".to_string(),
                out: None,
            });
        }
    }
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    rows
}

/// How many generations already exist for `host` — the `N` in `jetos
/// switch`'s `Plan  gen N → N+1` header (D-FE-CLI1). A real, derived count
/// from the generation ledger, not a fabricated number; call before the new
/// generation is built/recorded so it reads as the outgoing generation's
/// ordinal.
pub(super) fn generation_ordinal(host: &str) -> usize {
    read_generations()
        .into_iter()
        .filter(|g| g.host == host)
        .count()
}

/// Recursive on-disk size of a store path, in bytes. Pure std (I6): no
/// external `du`. Symlinks (a package's `out` is often one) are followed by
/// re-measuring their target; ordinary files are summed directly.
pub(super) fn dir_size_bytes(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_symlink() {
        return fs::canonicalize(path)
            .map(|p| dir_size_bytes(&p))
            .unwrap_or(0);
    }
    if meta.is_file() {
        return meta.len();
    }
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| dir_size_bytes(&entry.path()))
        .sum()
}

fn write_root_closure(dir: &Path, realized: &[RealizedPackage]) -> std::io::Result<()> {
    let sw_bin = dir.join("sw/bin");
    fs::create_dir_all(&sw_bin)?;
    let mut manifest = String::new();
    manifest.push_str("jetos system package closure\n");
    let mut projected_runtime = BTreeSet::new();
    for pkg in realized {
        manifest.push_str(&format!("{} {} {}\n", pkg.name, pkg.reference, pkg.out));
        project_runtime_closure(dir, pkg, &mut manifest, &mut projected_runtime)?;
        project_profile_dirs(dir, pkg)?;
        if pkg.bin.is_empty() {
            continue;
        }
        let bin = pkg.consumption_path(&pkg.bin)?;
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
            copy_file_replace(&src, &dst)?;
        }
    }
    write_jetos_toolchain(dir, &sw_bin, &mut manifest)?;
    rewrite_generation_store_symlinks(dir, dir)?;
    fs::write(dir.join("sw/closure.txt"), manifest)
}

fn project_profile_dirs(dir: &Path, pkg: &RealizedPackage) -> std::io::Result<()> {
    let out = pkg.consumption_path(&pkg.out)?;
    if !out.is_dir() {
        return Ok(());
    }
    let sw = dir.join("sw");
    for top in ["bin", "sbin", "lib", "libexec", "share", "etc"] {
        let src = out.join(top);
        if src.is_dir() {
            copy_profile_tree(&src, &sw.join(top))?;
        }
    }
    Ok(())
}

pub(super) fn copy_profile_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    if skip_runtime_payload(src) {
        return Ok(());
    }
    let meta = fs::symlink_metadata(src)?;
    // A symlink that resolves to a directory is treated as a directory:
    // replicating it verbatim would alias a read-only /nix/store dir into
    // the profile, and the next package merging into that subtree would
    // write through the link into the store (EROFS).
    let is_dir_like = meta.is_dir()
        || (meta.file_type().is_symlink()
            && fs::metadata(src).map(|m| m.is_dir()).unwrap_or(false));
    if is_dir_like {
        ensure_profile_dir(dst)?;
        let mut entries = fs::read_dir(src)?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            copy_profile_tree(&entry.path(), &dst.join(entry.file_name()))?;
        }
    } else if !dst.exists() {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        if meta.file_type().is_symlink() {
            copy_runtime_symlink(src, dst)?;
        } else if meta.is_file() {
            copy_file_replace(src, dst)?;
        }
    }
    Ok(())
}

/// Make `dst` a real, writable directory. If an earlier package left a
/// directory symlink here, materialize its contents first so this package
/// can merge alongside them.
fn ensure_profile_dir(dst: &Path) -> std::io::Result<()> {
    if let Ok(meta) = fs::symlink_metadata(dst) {
        if meta.file_type().is_symlink() {
            let target = fs::canonicalize(dst)?;
            fs::remove_file(dst)?;
            fs::create_dir_all(dst)?;
            let mut entries = fs::read_dir(&target)?
                .filter_map(Result::ok)
                .collect::<Vec<_>>();
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                copy_profile_tree(&entry.path(), &dst.join(entry.file_name()))?;
            }
            return Ok(());
        }
    }
    fs::create_dir_all(dst)
}

fn project_runtime_closure(
    dir: &Path,
    pkg: &RealizedPackage,
    manifest: &mut String,
    projected_runtime: &mut BTreeSet<String>,
) -> std::io::Result<()> {
    let out = pkg.original_output();
    if !out.starts_with("/nix/store") && !pkg.original_reference().starts_with("nix") {
        return Ok(());
    }
    for path in nix_store_closure_paths(&out)? {
        if !path.starts_with("/nix/store") {
            return Err(std::io::Error::other(format!(
                "nix-store -qR returned non-store path `{}` for `{}`",
                path.display(),
                out.display()
            )));
        }
        let key = path.to_string_lossy().into_owned();
        if !projected_runtime.insert(key.clone()) {
            continue;
        }
        copy_nix_store_path(dir, &path)?;
        manifest.push_str(&format!("jetos-adapter-closure {} {}\n", pkg.name, key));
    }
    Ok(())
}

fn nix_store_closure_paths(out: &Path) -> std::io::Result<Vec<PathBuf>> {
    nix_store_closure_paths_with(Path::new("nix-store"), out)
}

fn nix_store_closure_paths_with(command: &Path, out: &Path) -> std::io::Result<Vec<PathBuf>> {
    let output = Command::new(command).args(["-qR"]).arg(out).output();
    let output = match output {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![out.to_path_buf()]),
        Err(e) => return Err(e),
    };
    if !output.status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "nix-store -qR failed for `{}`: {}",
                out.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    let mut paths = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| PathBuf::from(line.trim()))
        .filter(|path| !path.as_os_str().is_empty())
        .collect::<Vec<_>>();
    if !paths.iter().any(|path| path == out) {
        paths.push(out.to_path_buf());
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn copy_nix_store_path(dir: &Path, src: &Path) -> std::io::Result<()> {
    let rel = src.strip_prefix("/").map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("nix closure path is not absolute: `{}`", src.display()),
        )
    })?;
    copy_nix_closure_tree(src, &dir.join(rel))
}

fn copy_nix_closure_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(src)?;
    if meta.is_dir() {
        fs::create_dir_all(dst)?;
        let mut entries = fs::read_dir(src)?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            copy_nix_closure_tree(&entry.path(), &dst.join(entry.file_name()))?;
        }
    } else if meta.file_type().is_symlink() {
        copy_runtime_symlink(src, dst)?;
    } else if meta.is_file() {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        copy_file_replace(src, dst)?;
    }
    Ok(())
}

pub(super) fn copy_runtime_file_filtered(src: &Path, dst: &Path) -> std::io::Result<()> {
    let bytes = fs::read(src)?;
    if let Ok(text) = std::str::from_utf8(&bytes) {
        if has_foreign_os_bytes(text.as_bytes()) && text.contains("nix-snowflake") {
            let sanitized = text
                .lines()
                .map(|line| {
                    if line.trim_start().starts_with("logo=") {
                        "logo='/run/current-system/share/icons/hicolor/scalable/apps/jetos-logo.svg'"
                    } else {
                        line
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            fs::write(dst, format!("{sanitized}\n"))?;
            return Ok(());
        }
    }
    if let Some(sanitized) = sanitize_runtime_branding_bytes(&bytes) {
        fs::write(dst, sanitized)?;
        return Ok(());
    }
    copy_file_replace(src, dst)
}

fn rewrite_generation_store_symlinks(root: &Path, path: &Path) -> std::io::Result<()> {
    rewrite_store_symlinks(root, path, Path::new("/nix/store"))
}

fn rewrite_store_symlinks(root: &Path, path: &Path, store: &Path) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        let target = fs::read_link(path)?;
        let relative_target = target.strip_prefix("/").map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "store symlink target must be absolute: `{}`",
                    target.display()
                ),
            )
        })?;
        if target.starts_with(store) {
            let owned = root.join(relative_target);
            let parent = path
                .parent()
                .ok_or_else(|| std::io::Error::other("generation symlink has no parent"))?;
            let rewritten = relative_path(parent, &owned)?;
            fs::remove_file(path)?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(rewritten, path)?;
            #[cfg(not(unix))]
            {
                let _ = rewritten;
                return Err(std::io::Error::other("store symlink rewriting needs Unix symlinks"));
            }
        }
        return Ok(());
    }
    if meta.is_dir() {
        let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            rewrite_store_symlinks(root, &entry.path(), store)?;
        }
    }
    Ok(())
}

pub(super) fn relative_path(from: &Path, to: &Path) -> std::io::Result<PathBuf> {
    let from = from.components().collect::<Vec<_>>();
    let to = to.components().collect::<Vec<_>>();
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| left == right)
        .count();
    if common == 0 {
        return Err(std::io::Error::other("cannot relativize generation store path"));
    }
    let mut path = PathBuf::new();
    for _ in common..from.len() {
        path.push("..");
    }
    for component in &to[common..] {
        path.push(component.as_os_str());
    }
    Ok(path)
}

pub(super) fn sanitize_runtime_branding_file(path: &Path) -> std::io::Result<()> {
    if fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Ok(());
    }
    let bytes = fs::read(path)?;
    if let Some(sanitized) = sanitize_runtime_branding_bytes(&bytes) {
        fs::write(path, sanitized)?;
    }
    Ok(())
}

pub(super) fn sanitize_runtime_branding_bytes(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut out = bytes.to_vec();
    let mut changed = false;
    for (from, to) in [
        (b"NixOS".as_slice(), b"JetOS".as_slice()),
        (b"NIXOS".as_slice(), b"JETOS".as_slice()),
        (b"nixos.org".as_slice(), b"jetos.dev".as_slice()),
        (b"nixos".as_slice(), b"jetos".as_slice()),
    ] {
        changed |= replace_bytes_in_place(&mut out, from, to);
    }
    changed.then_some(out)
}

fn replace_bytes_in_place(bytes: &mut [u8], from: &[u8], to: &[u8]) -> bool {
    if from.len() != to.len() || from.is_empty() {
        return false;
    }
    let mut changed = false;
    let mut idx = 0;
    while idx + from.len() <= bytes.len() {
        if &bytes[idx..idx + from.len()] == from {
            bytes[idx..idx + to.len()].copy_from_slice(to);
            changed = true;
            idx += from.len();
        } else {
            idx += 1;
        }
    }
    changed
}

fn has_foreign_os_bytes(bytes: &[u8]) -> bool {
    bytes
        .windows(5)
        .any(|window| window == [b'n', b'i', b'x', b'o', b's'])
}

fn skip_runtime_payload(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|part| part.to_str()) else {
        return false;
    };
    name.ends_with(".nix") || name.ends_with(".drv")
}

#[cfg(unix)]
pub(super) fn copy_runtime_symlink(src: &Path, dst: &Path) -> std::io::Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    let _ = fs::remove_file(dst);
    let target = fs::read_link(src)?;
    std::os::unix::fs::symlink(target, dst)
}

#[cfg(not(unix))]
pub(super) fn copy_runtime_symlink(src: &Path, dst: &Path) -> std::io::Result<()> {
    copy_file_replace(src, dst)
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
        copy_file_replace(src, &dst).map_err(|e| {
            std::io::Error::new(
                e.kind(),
                format!(
                    "copying jetos tool `{}` to `{}` failed: {e}",
                    src.display(),
                    dst.display()
                ),
            )
        })?;
        sanitize_runtime_branding_file(&dst)?;
        make_executable(&dst).map_err(|e| {
            std::io::Error::new(
                e.kind(),
                format!("marking jetos tool `{}` executable failed: {e}", dst.display()),
            )
        })?;
        manifest.push_str(&format!("jetos-toolchain {name} {}\n", src.display()));
        copy_toolchain_runtime_deps(dir, src).map_err(|e| {
            std::io::Error::new(
                e.kind(),
                format!(
                    "copying runtime dependencies for jetos tool `{}` failed: {e}",
                    src.display()
                ),
            )
        })?;
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
    copy_file_replace(src, &dst).map_err(|e| {
        std::io::Error::new(
            e.kind(),
            format!(
                "copying runtime file `{}` to `{}` failed: {e}",
                src.display(),
                dst.display()
            ),
        )
    })
}

#[cfg(all(test, target_os = "linux"))]
mod generation_closure_tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt as _};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "jetos-owned-closure-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn relative_store_symlink_target_returns_invalid_data() {
        let guard = Scratch::new();
        let generation = guard.0.join("generation");
        fs::create_dir_all(&generation).unwrap();
        let link = generation.join("tool");
        let target = Path::new("nix/store/aaaa-package/bin/tool");
        symlink(target, &link).unwrap();

        let error = rewrite_store_symlinks(&generation, &link, Path::new("/nix/store"))
            .unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "store symlink target must be absolute: `nix/store/aaaa-package/bin/tool`"
        );
        assert_eq!(fs::read_link(&link).unwrap(), target);
    }

    #[test]
    fn absolute_non_store_symlink_target_is_unchanged() {
        let guard = Scratch::new();
        let generation = guard.0.join("generation");
        fs::create_dir_all(&generation).unwrap();
        let link = generation.join("tool");
        let target = Path::new("/opt/jet/bin/tool");
        symlink(target, &link).unwrap();

        rewrite_store_symlinks(&generation, &link, Path::new("/nix/store")).unwrap();

        assert_eq!(fs::read_link(&link).unwrap(), target);
    }

    #[test]
    fn generation_owns_full_closure_after_original_and_lease_are_removed() {
        let guard = Scratch::new();
        let store = guard.0.join("fake-nix/store");
        let package = store.join("aaaa-package");
        let dependency = store.join("bbbb-dependency");
        fs::create_dir_all(package.join("bin")).unwrap();
        fs::create_dir_all(dependency.join("bin")).unwrap();
        let package_tool = package.join("bin/tool-real");
        fs::write(&package_tool, "#!/bin/sh\nprintf closure-owned").unwrap();
        fs::set_permissions(&package_tool, fs::Permissions::from_mode(0o555)).unwrap();
        fs::write(dependency.join("bin/dependency-data"), "required closure member").unwrap();

        let helper = guard.0.join("nix-store");
        fs::write(
            &helper,
            format!(
                "#!/bin/sh\nprintf '%s\\n' '{}' '{}'\n",
                package.display(),
                dependency.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o555)).unwrap();
        let closure = nix_store_closure_paths_with(&helper, &package).unwrap();
        assert_eq!(closure, vec![package.clone(), dependency.clone()]);

        let roots = Store::Roots {
            root: guard.0.join("jetpack-root"),
            dev_mode: true,
        };
        let envelope = crate::Envelope::Envelope::for_output(
            &package.to_string_lossy(),
            "nixpkgs:package",
            "nix",
        );
        let identity = Store::CacheIdentity {
            source_fingerprint: "source".to_string(),
            recipe_fingerprint: "recipe".to_string(),
            policy_fingerprint: "policy".to_string(),
            platform: crate::Envelope::host_platform(),
        };
        let entry = Store::record_verified(
            &roots,
            "package",
            "1",
            "nixpkgs:package",
            &package.to_string_lossy(),
            "",
            "",
            &envelope,
            &identity,
        )
        .unwrap();
        let expectation = Store::CacheExpectation {
            identity,
            owned_output: Some(package.clone()),
            allow_unsigned_local: true,
        };
        let proof = Store::verify_cache_entry(&roots, &entry, "nixpkgs:package", &expectation);
        assert!(proof.trusted(), "{proof:?}");
        let lease = Store::find_verified_by_reference(&roots, "nixpkgs:package", &expectation)
            .unwrap()
            .unwrap();
        symlink(&package_tool, package.join("bin/tool")).unwrap();

        let generation = guard.0.join("generation");
        for member in &closure {
            let destination = generation.join(member.strip_prefix("/").unwrap());
            copy_nix_closure_tree(member, &destination).unwrap();
        }
        rewrite_store_symlinks(&generation, &generation, &store).unwrap();
        drop(lease);
        fs::remove_dir_all(&store).unwrap();

        let owned_tool = generation.join(package.strip_prefix("/").unwrap()).join("bin/tool");
        assert!(generation
            .join(dependency.strip_prefix("/").unwrap())
            .join("bin/dependency-data")
            .is_file());
        let output = Command::new(owned_tool).output().unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "closure-owned");
    }
}
