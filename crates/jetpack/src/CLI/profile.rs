//! D-JPK-PROFILE1=D: source-backed package-generation planning and lifecycle.
//!
//! The source resolver owns package identity and provider facts. This module
//! only realizes those facts, records an immutable generation, and publishes a
//! checked pointer to one generation.

use super::parse::Parsed;
use super::realize::{project_env_root, realize_ref_outcome, RefOutcome, RowStyle};
use crate::Output::Theme;
use crate::{EnvFile, RefSpec, Store, Syntax, JSON, SHA256};
use jet_env_model::ModuleEval;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub(super) fn cmd_profile(theme: &Theme, parsed: &Parsed) -> i32 {
    match parsed.positional.first().map(String::as_str) {
        Some(v) if v == Syntax::PROFILE_VERB_PLAN => profile_plan(theme, parsed),
        Some(v) if v == Syntax::PROFILE_VERB_BUILD => profile_build(theme, parsed),
        Some(v) if v == Syntax::PROFILE_VERB_SWITCH => profile_switch(theme, parsed),
        Some(v) if v == Syntax::PROFILE_VERB_ROLLBACK => profile_rollback(theme, parsed),
        Some(v) if v == Syntax::PROFILE_VERB_GENERATIONS => profile_generations(theme, parsed),
        Some(other) => {
            theme.error(
                &format!("`{other}` is not a package-generation verb"),
                &format!(
                    "`jet profile` verbs are: {}.",
                    Syntax::PROFILE_VERBS.join(", ")
                ),
                "try `jet profile plan <name>`.",
            );
            2
        }
        None => {
            theme.error(
                "`jet profile` needs a verb",
                &format!("verbs are: {}.", Syntax::PROFILE_VERBS.join(", ")),
                "try `jet profile plan <name>`.",
            );
            2
        }
    }
}

fn profile_plan(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(name) = parsed.positional.get(1) else {
        theme.error(
            "`jet profile plan` needs a generation name",
            "planning resolves one source-backed `profile.<name>` declaration and its parents",
            "try `jet profile plan dev`.",
        );
        return 2;
    };
    if parsed.positional.len() != 2 || parsed.command.is_some() {
        theme.error(
            "`jet profile plan` accepts one generation name",
            "planning is read-only and has no trailing command",
            "run `jet profile plan <name> --json` for machine-readable output",
        );
        return 2;
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let project_dir = project_env_root(&cwd);
    let path = EnvFile::path_in(&project_dir);
    let src = match std::fs::read_to_string(&path) {
        Ok(src) => src,
        Err(error) => {
            theme.error(
                &format!("couldn't read {}", path.display()),
                &error.to_string(),
                "create an env.jet with a `module profile.<name> { … }` declaration",
            );
            return 2;
        }
    };
    let plan = match ModuleEval::evaluate_package_profile(&src, &project_dir, name) {
        Ok(plan) => plan,
        Err(diagnostic) => {
            eprint!(
                "{}",
                crate::Diagnostics::render_all(
                    Syntax::ENV_FILE,
                    &src,
                    std::slice::from_ref(&diagnostic)
                )
            );
            return 2;
        }
    };
    if parsed.flags.json {
        let provider_facts = plan
            .provider_facts
            .iter()
            .map(|(raw, facts)| format!("{}:{}", JSON::quote(raw), facts.to_json()))
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{{\"name\":{},\"fingerprint\":{},\"selected_profiles\":[{}],\"applied\":[{}],\"sources\":[{}],\"packages\":[{}],\"collisions\":{{{}}},\"provider_facts\":{{{}}}}}",
            JSON::quote(&plan.name),
            JSON::quote(&plan.fingerprint),
            quote_strings(&plan.selected_profiles),
            quote_strings(&plan.applied),
            quote_strings(&plan.sources),
            plan.packages
                .iter()
                .map(|package| {
                    format!(
                        "{{\"raw\":{},\"target\":{},\"source\":{},\"upstream\":{},\"provider\":{},\"channel\":{},\"declared_by\":[{}],\"provider_facts\":{}}}",
                        JSON::quote(&package.raw),
                        JSON::quote(&package.target),
                        JSON::quote(&package.source),
                        package
                            .upstream
                            .as_deref()
                            .map(JSON::quote)
                            .unwrap_or_else(|| "null".to_string()),
                        JSON::quote(&package.provider),
                        package
                            .channel
                            .as_deref()
                            .map(JSON::quote)
                            .unwrap_or_else(|| "null".to_string()),
                        quote_strings(&package.declared_by),
                        package.provider_facts.to_json(),
                    )
                })
                .collect::<Vec<_>>()
                .join(","),
            plan.collisions
                .iter()
                .map(|(path, provider)| {
                    format!("{}:{}", JSON::quote(path), JSON::quote(provider))
                })
                .collect::<Vec<_>>()
                .join(","),
            provider_facts,
        );
        return 0;
    }
    theme.ok(&format!(
        "package generation {} planned",
        theme.bold(&plan.name)
    ));
    theme.detail(&format!("fingerprint: {}", plan.fingerprint));
    theme.detail(&format!("applied: {}", plan.applied.join(" -> ")));
    if !plan.sources.is_empty() {
        theme.detail(&format!("declared by: {}", plan.sources.join(", ")));
    }
    for package in &plan.packages {
        let channel = package
            .channel
            .as_deref()
            .map(|value| format!("#{value}"))
            .unwrap_or_default();
        let upstream = package
            .upstream
            .as_deref()
            .map(|value| format!(" -> {value}"))
            .unwrap_or_default();
        theme.detail(&format!(
            "package {}  [{}{} via {}]  ({})",
            package.raw,
            package.source,
            channel,
            package.provider,
            package.declared_by.join(", ")
        ));
        if !upstream.is_empty() {
            theme.detail(&format!("  source{upstream}"));
        }
        for line in package.provider_facts.explain_lines() {
            theme.detail(&format!("  fact {line}"));
        }
    }
    for (path, provider) in &plan.collisions {
        theme.detail(&format!("collision {path} <- {provider}"));
    }
    0
}

fn quote_strings(values: &[String]) -> String {
    values
        .iter()
        .map(|value| JSON::quote(value))
        .collect::<Vec<_>>()
        .join(",")
}

const PROFILE_STATE_DIR: &str = "profiles";
const PROFILE_GENERATIONS_DIR: &str = "generations";
const PROFILE_META_FILE: &str = "meta.json";
const PROFILE_ROOTFS_DIR: &str = "root";
const PROFILE_COMPLETE_FILE: &str = "complete";
const PROFILE_CURRENT_FILE: &str = "current";
const PROFILE_CURRENT_PARTIAL: &str = "current.partial";
const PROFILE_SCHEMA: &str = "jet-package-generation-v1";
const PROFILE_POINTER_SCHEMA: &str = "jet-package-generation-pointer-v1";
const MAX_PROFILE_NODES: usize = 100_000;

fn profile_build(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(name) = one_profile_name(theme, parsed, Syntax::PROFILE_VERB_BUILD) else {
        return 2;
    };
    let Some((project_dir, _source, plan)) = load_profile_plan(theme, &name) else {
        return 2;
    };
    match build_generation(theme, parsed, &project_dir, &plan) {
        Ok(generation) => {
            if parsed.flags.json {
                println!(
                    "{{\"profile\":{},\"generation\":{},\"fingerprint\":{}}}",
                    JSON::quote(&plan.name),
                    generation,
                    JSON::quote(&plan.fingerprint),
                );
            } else {
                theme.ok(&format!(
                    "package generation {} built as {}",
                    theme.bold(&plan.name),
                    generation
                ));
            }
            0
        }
        Err(error) => report_generation_error(theme, &error),
    }
}

fn profile_switch(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(name) = one_profile_name(theme, parsed, Syntax::PROFILE_VERB_SWITCH) else {
        return 2;
    };
    let Some((project_dir, _source, plan)) = load_profile_plan(theme, &name) else {
        return 2;
    };
    let state = profile_state_dir(&project_dir, &plan.name);
    let generation = match read_generations(&state) {
        Ok(records) => records
            .iter()
            .rev()
            .find(|record| record.fingerprint == plan.fingerprint)
            .map(|record| record.generation),
        Err(error) => return report_generation_error(theme, &error),
    };
    let generation = match generation {
        Some(generation) => generation,
        None => match build_generation(theme, parsed, &project_dir, &plan) {
            Ok(generation) => generation,
            Err(error) => return report_generation_error(theme, &error),
        },
    };
    match activate_generation(
        &project_dir,
        &plan.name,
        generation,
        Some(&plan.fingerprint),
    ) {
        Ok(record) => {
            if parsed.flags.json {
                println!(
                    "{{\"profile\":{},\"current\":{},\"witness\":{}}}",
                    JSON::quote(&plan.name),
                    record.generation,
                    JSON::quote(&record.witness),
                );
            } else {
                theme.ok(&format!(
                    "package generation {} switched to {}",
                    theme.bold(&plan.name),
                    record.generation
                ));
            }
            0
        }
        Err(error) => report_generation_error(theme, &error),
    }
}

fn profile_rollback(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(name) = parsed.positional.get(1).cloned() else {
        theme.error(
            "`jet profile rollback` needs a generation name",
            "rollback activates an older immutable generation from the same profile history",
            "try `jet profile rollback dev` or `jet profile rollback dev 3`",
        );
        return 2;
    };
    if (parsed.positional.len() != 2 && parsed.positional.len() != 3) || parsed.command.is_some() {
        theme.error(
            "`jet profile rollback` accepts a name and optional generation",
            "without a number it selects the generation immediately before the current one",
            "try `jet profile rollback dev 3`",
        );
        return 2;
    }
    let target = match parsed.positional.get(2) {
        Some(value) => match value.parse::<u64>() {
            Ok(value) if value > 0 => Some(value),
            _ => {
                theme.error(
                    "profile rollback generation is invalid",
                    "generation numbers are positive decimal integers",
                    "try `jet profile rollback dev 3`",
                );
                return 2;
            }
        },
        None => None,
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    let project_dir = project_env_root(&cwd);
    let state = profile_state_dir(&project_dir, &name);
    let records = match read_generations(&state) {
        Ok(records) => records,
        Err(error) => return report_generation_error(theme, &error),
    };
    let current = match read_current_pointer(&state) {
        Ok(Some(pointer)) => Some(pointer.generation),
        Ok(None) => None,
        Err(error) => return report_generation_error(theme, &error),
    };
    let generation = match target {
        Some(target) => target,
        None => {
            let Some(current) = current else {
                return report_generation_error(
                    theme,
                    &io::Error::other("profile has no current generation to roll back"),
                );
            };
            let Some(previous) = records
                .iter()
                .filter(|record| record.generation < current)
                .max_by_key(|record| record.generation)
            else {
                return report_generation_error(
                    theme,
                    &io::Error::other("profile has no older generation to roll back to"),
                );
            };
            previous.generation
        }
    };
    match activate_generation(&project_dir, &name, generation, None) {
        Ok(record) => {
            if parsed.flags.json {
                println!(
                    "{{\"profile\":{},\"current\":{},\"witness\":{}}}",
                    JSON::quote(&name),
                    record.generation,
                    JSON::quote(&record.witness),
                );
            } else {
                theme.ok(&format!(
                    "package generation {} rolled back to {}",
                    theme.bold(&name),
                    record.generation
                ));
            }
            0
        }
        Err(error) => report_generation_error(theme, &error),
    }
}

fn profile_generations(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(name) = one_profile_name(theme, parsed, Syntax::PROFILE_VERB_GENERATIONS) else {
        return 2;
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    let project_dir = project_env_root(&cwd);
    let state = profile_state_dir(&project_dir, &name);
    let records = match read_generations(&state) {
        Ok(records) => records,
        Err(error) => return report_generation_error(theme, &error),
    };
    let current = match read_current_pointer(&state) {
        Ok(pointer) => pointer.map(|pointer| pointer.generation),
        Err(error) => return report_generation_error(theme, &error),
    };
    if parsed.flags.json {
        let generations = records
            .iter()
            .map(|record| {
                format!(
                    "{{\"generation\":{},\"fingerprint\":{},\"witness\":{},\"lock\":{}}}",
                    record.generation,
                    JSON::quote(&record.fingerprint),
                    JSON::quote(&record.witness),
                    record.metadata.trim(),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{{\"profile\":{},\"current\":{},\"generations\":[{}]}}",
            JSON::quote(&name),
            current
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_string()),
            generations,
        );
    } else if records.is_empty() {
        theme.detail(&format!(
            "package generation {} has no built generations",
            name
        ));
    } else {
        for record in records.iter().rev() {
            let marker = if current == Some(record.generation) {
                " current"
            } else {
                ""
            };
            theme.detail(&format!(
                "generation {}{}  {}",
                record.generation, marker, record.fingerprint
            ));
        }
    }
    0
}

fn one_profile_name(theme: &Theme, parsed: &Parsed, verb: &str) -> Option<String> {
    let Some(name) = parsed.positional.get(1) else {
        theme.error(
            &format!("`jet profile {verb}` needs a generation name"),
            "the name selects one source-backed `profile.<name>` declaration",
            &format!("try `jet profile {verb} dev`"),
        );
        return None;
    };
    if parsed.positional.len() != 2 || parsed.command.is_some() {
        theme.error(
            &format!("`jet profile {verb}` accepts one generation name"),
            "profile lifecycle commands do not accept a trailing command",
            &format!("try `jet profile {verb} {name}`"),
        );
        return None;
    }
    if !valid_profile_name(name) {
        theme.error(
            "profile generation name is unsafe",
            "generation names are single path components",
            "use a name such as `dev` or `release`",
        );
        return None;
    }
    Some(name.clone())
}

fn valid_profile_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
}

fn load_profile_plan(
    theme: &Theme,
    name: &str,
) -> Option<(
    PathBuf,
    String,
    jet_env_model::ModuleEval::PackageProfilePlan,
)> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let project_dir = project_env_root(&cwd);
    let path = EnvFile::path_in(&project_dir);
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => {
            theme.error(
                &format!("couldn't read {}", path.display()),
                &error.to_string(),
                "create an env.jet with a `module profile.<name> { … }` declaration",
            );
            return None;
        }
    };
    let plan = match ModuleEval::evaluate_package_profile(&source, &project_dir, name) {
        Ok(plan) => plan,
        Err(diagnostic) => {
            eprint!(
                "{}",
                crate::Diagnostics::render_all(
                    Syntax::ENV_FILE,
                    &source,
                    std::slice::from_ref(&diagnostic),
                )
            );
            return None;
        }
    };
    Some((project_dir, source, plan))
}

struct RealizedProfilePackage {
    fact: jet_env_model::ModuleEval::PackageProfileFact,
    entry: Store::StoreEntry,
    lease: Store::CacheLease,
    nodes: Vec<OutputNode>,
}

#[derive(Clone)]
struct OutputNode {
    package: String,
    path: String,
    kind: OutputNodeKind,
    digest: String,
    source: PathBuf,
    link_target: Option<PathBuf>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum OutputNodeKind {
    File,
    Directory,
    Symlink,
}

impl OutputNodeKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
            Self::Symlink => "symlink",
        }
    }
}

struct GenerationRecord {
    generation: u64,
    fingerprint: String,
    witness: String,
    metadata: String,
}

struct CurrentPointer {
    generation: u64,
    witness: String,
}

fn build_generation(
    theme: &Theme,
    parsed: &Parsed,
    project_dir: &Path,
    plan: &jet_env_model::ModuleEval::PackageProfilePlan,
) -> io::Result<u64> {
    let state = profile_state_dir(project_dir, &plan.name);
    let generations = generations_dir(&state);
    fs::create_dir_all(&generations)?;
    let previous = read_generations(&state)?;
    let generation = next_generation(&state)?;
    let generation_dir = generations.join(generation.to_string());
    let rootfs = generation_dir.join(PROFILE_ROOTFS_DIR);

    let roots = Store::resolve();
    let table = super::workspace_sources::cwd_table();
    let name_width = plan
        .packages
        .iter()
        .map(|package| package.target.len())
        .max()
        .unwrap_or(1);
    let mut realized = Vec::new();
    for package in &plan.packages {
        let spec = realization_spec(package, &table)?;
        let outcome = realize_ref_outcome(
            theme,
            &roots,
            &parsed.flags,
            &table,
            &spec,
            name_width,
            RowStyle::Ledger,
            None,
        );
        let (entry, lease) = match outcome {
            RefOutcome::Realized(entry, _state, _line, lease) => (entry, lease),
            RefOutcome::NeedsNix(need) => {
                super::realize::report_nix_bridge_required(theme, &parsed.flags, &[need], &[]);
                return Err(io::Error::other(
                    "profile package needs the provider bridge before it can be generated",
                ));
            }
            RefOutcome::Failed => {
                return Err(io::Error::other("profile package realization failed"));
            }
        };
        lease.validate()?;
        let output_hash = entry.envelope.output_hash.clone();
        if !valid_digest(&output_hash) {
            return Err(io::Error::other(format!(
                "E1335: realized `{}` has no canonical output digest",
                package.raw
            )));
        }
        let nodes = collect_output_nodes(lease.original_output(), &package.raw)?;
        realized.push(RealizedProfilePackage {
            fact: package.clone(),
            entry,
            lease,
            nodes,
        });
    }

    let mut groups = BTreeMap::<String, Vec<OutputNode>>::new();
    for package in &realized {
        for node in &package.nodes {
            groups
                .entry(node.path.clone())
                .or_default()
                .push(node.clone());
        }
    }
    let selected = choose_output_nodes(plan, &groups, &previous)?;
    fs::create_dir(&generation_dir)?;
    fs::create_dir(&rootfs)?;
    copy_projection(&rootfs, &selected)?;
    for package in &realized {
        package.lease.validate()?;
    }

    let metadata = format_generation_metadata(generation, plan, &realized, &groups, &roots);
    let target_hashes = realized
        .iter()
        .map(|package| package.entry.envelope.output_hash.clone())
        .collect::<BTreeSet<_>>();
    let witness = super::tool::profile_generation_witness(&metadata, &target_hashes);
    super::tool::write_synced(&generation_dir.join(PROFILE_META_FILE), metadata.as_bytes())?;

    let owner = profile_owner(project_dir);
    if !target_hashes.is_empty() {
        let prepared = Store::prepare_profile_generation_root(
            &roots,
            &owner,
            &plan.name,
            generation,
            &witness,
            target_hashes.iter().cloned().collect(),
            super::tool::now_secs(),
        )?;
        Store::commit_profile_generation_root(&roots, &prepared, super::tool::now_secs())?;
    }
    super::tool::write_synced(
        &generation_dir.join(PROFILE_COMPLETE_FILE),
        format!("{witness}\n").as_bytes(),
    )?;
    Store::sync_store_directory(&generation_dir)?;
    Store::sync_store_directory(&generations)?;
    Ok(generation)
}

fn realization_spec(
    package: &jet_env_model::ModuleEval::PackageProfileFact,
    table: &RefSpec::SourceTable,
) -> io::Result<RefSpec::RefSpec> {
    package
        .provider_facts
        .validate()
        .map_err(|error| io::Error::other(format!("E1335: {error}")))?;
    let target = match &package.channel {
        Some(channel) => format!("{}#{}", package.target, channel),
        None => package.target.clone(),
    };
    let raw = if package.source == Syntax::REF_SOURCE_PATH {
        target
    } else {
        let source = if package.source == Syntax::DEFAULT_SOURCE {
            Syntax::REF_SOURCE_NIXPKGS
        } else {
            &package.source
        };
        format!("{target}{}{source}", Syntax::REF_PROVIDER_AT)
    };
    let spec = RefSpec::classify_in(&raw, table)
        .map_err(|error| io::Error::other(format!("E1335: unsupported profile ref: {error:?}")))?;
    Ok(spec)
}

fn choose_output_nodes(
    plan: &jet_env_model::ModuleEval::PackageProfilePlan,
    groups: &BTreeMap<String, Vec<OutputNode>>,
    previous: &[GenerationRecord],
) -> io::Result<BTreeMap<String, OutputNode>> {
    for (path, provider) in &plan.collisions {
        let Some(nodes) = groups.get(path) else {
            return Err(io::Error::other(format!(
                "E1335: collision selection for `{path}` names `{provider}`, but no realized contender provides that path"
            )));
        };
        if !nodes.iter().any(|node| node.package == *provider) {
            return Err(io::Error::other(format!(
                "E1335: collision selection for `{path}` names `{provider}`, but contenders are {}",
                contender_text(nodes)
            )));
        }
    }
    let mut selected = BTreeMap::new();
    for (path, nodes) in groups {
        let kinds = nodes.iter().map(|node| node.kind).collect::<BTreeSet<_>>();
        if kinds.len() != 1 {
            return Err(io::Error::other(format!(
                "E1335: collision at `{path}` has a file/directory or symlink-target type mismatch; contenders: {}",
                contender_text(nodes)
            )));
        }
        let digests = nodes
            .iter()
            .filter(|node| node.kind != OutputNodeKind::Directory)
            .map(|node| node.digest.as_str())
            .collect::<BTreeSet<_>>();
        let provider = plan.collisions.get(path);
        if digests.len() > 1 && provider.is_none() {
            return Err(io::Error::other(format!(
                "E1335: unresolved package collision at `{path}`; contenders: {}",
                contender_text(nodes)
            )));
        }
        if let Some(provider) = provider {
            ensure_collision_selection_current(previous, path, provider, nodes)?;
        }
        let chosen = provider
            .and_then(|provider| nodes.iter().find(|node| node.package == *provider))
            .unwrap_or(&nodes[0]);
        selected.insert(path.clone(), chosen.clone());
    }
    Ok(selected)
}

fn ensure_collision_selection_current(
    previous: &[GenerationRecord],
    path: &str,
    provider: &str,
    nodes: &[OutputNode],
) -> io::Result<()> {
    let Some(record) = previous.last() else {
        return Ok(());
    };
    let Some((Some(previous_provider), previous_contenders)) =
        collision_record(&record.metadata, path)?
    else {
        return Ok(());
    };
    if previous_provider != provider {
        return Ok(());
    }
    let mut current_contenders = nodes
        .iter()
        .map(|node| {
            (
                node.package.clone(),
                node.kind.as_str().to_string(),
                node.digest.clone(),
            )
        })
        .collect::<Vec<_>>();
    current_contenders.sort();
    if current_contenders != previous_contenders {
        return Err(io::Error::other(format!(
            "E1335: stale collision selection at `{path}` for `{provider}`; contender facts changed: {}; re-review `collisions` before rebuilding",
            contender_text(nodes)
        )));
    }
    Ok(())
}

fn collision_record(
    metadata: &str,
    path: &str,
) -> io::Result<Option<(Option<String>, Vec<(String, String, String)>)>> {
    let value = JSON::parse(metadata).map_err(io::Error::other)?;
    let object = value.as_object().map_err(io::Error::other)?;
    let Some(collisions) = object.get("collisions") else {
        return Ok(None);
    };
    let collisions = collisions.as_object().map_err(io::Error::other)?;
    let Some(record) = collisions.get(path) else {
        return Ok(None);
    };
    let record = record.as_object().map_err(io::Error::other)?;
    let selected = match record.get("selected") {
        Some(crate::JSON::JSONValue::Null) | None => None,
        Some(value) => Some(value.as_str().map_err(io::Error::other)?.to_string()),
    };
    let contenders = record
        .get("contenders")
        .ok_or_else(|| io::Error::other("profile collision record lacks contenders"))?
        .as_array()
        .map_err(io::Error::other)?;
    let mut contenders = contenders
        .iter()
        .map(|value| {
            let contender = value.as_object().map_err(io::Error::other)?;
            Ok((
                contender
                    .get("provider")
                    .ok_or_else(|| io::Error::other("profile collision contender lacks provider"))?
                    .as_str()
                    .map_err(io::Error::other)?
                    .to_string(),
                contender
                    .get("kind")
                    .ok_or_else(|| io::Error::other("profile collision contender lacks kind"))?
                    .as_str()
                    .map_err(io::Error::other)?
                    .to_string(),
                contender
                    .get("digest")
                    .ok_or_else(|| io::Error::other("profile collision contender lacks digest"))?
                    .as_str()
                    .map_err(io::Error::other)?
                    .to_string(),
            ))
        })
        .collect::<io::Result<Vec<_>>>()?;
    contenders.sort();
    Ok(Some((selected, contenders)))
}

fn contender_text(nodes: &[OutputNode]) -> String {
    nodes
        .iter()
        .map(|node| format!("{} [{} {}]", node.package, node.kind.as_str(), node.digest))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_generation_metadata(
    generation: u64,
    plan: &jet_env_model::ModuleEval::PackageProfilePlan,
    realized: &[RealizedProfilePackage],
    groups: &BTreeMap<String, Vec<OutputNode>>,
    roots: &Store::Roots,
) -> String {
    let packages = realized
        .iter()
        .map(|package| {
            let channel = package
                .fact
                .channel
                .as_deref()
                .map(|value| JSON::quote(value))
                .unwrap_or_else(|| "null".to_string());
            let upstream = package
                .fact
                .upstream
                .as_deref()
                .map(JSON::quote)
                .unwrap_or_else(|| "null".to_string());
            format!(
                "{{\"raw\":{},\"target\":{},\"source\":{},\"upstream\":{},\"provider\":{},\"channel\":{},\"declared_by\":[{}],\"provider_facts\":{},\"output_hash\":{},\"store_root\":{}}}",
                JSON::quote(&package.fact.raw),
                JSON::quote(&package.fact.target),
                JSON::quote(&package.fact.source),
                upstream,
                JSON::quote(&package.fact.provider),
                channel,
                quote_strings(&package.fact.declared_by),
                package.fact.provider_facts.to_json(),
                JSON::quote(&package.entry.envelope.output_hash),
                JSON::quote(&roots.root.to_string_lossy()),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let collisions = groups
        .iter()
        .filter(|(path, nodes)| nodes.len() > 1 || plan.collisions.contains_key(*path))
        .map(|(path, nodes)| {
            let contenders = nodes
                .iter()
                .map(|node| {
                    format!(
                        "{{\"provider\":{},\"kind\":{},\"digest\":{}}}",
                        JSON::quote(&node.package),
                        JSON::quote(node.kind.as_str()),
                        JSON::quote(&node.digest),
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            let selected = plan
                .collisions
                .get(path)
                .map(|value| JSON::quote(value))
                .unwrap_or_else(|| "null".to_string());
            format!(
                "{}:{{\"selected\":{},\"contenders\":[{}]}}",
                JSON::quote(path),
                selected,
                contenders,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":{},\"profile\":{},\"generation\":{},\"fingerprint\":{},\"selected_profiles\":[{}],\"applied\":[{}],\"sources\":[{}],\"collisions\":{{{}}},\"packages\":[{}]}}\n",
        JSON::quote(PROFILE_SCHEMA),
        JSON::quote(&plan.name),
        generation,
        JSON::quote(&plan.fingerprint),
        quote_strings(&plan.selected_profiles),
        quote_strings(&plan.applied),
        quote_strings(&plan.sources),
        collisions,
        packages,
    )
}

fn collect_output_nodes(root: &Path, package: &str) -> io::Result<Vec<OutputNode>> {
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(io::Error::other(format!(
            "E1335: realized `{package}` output is not a real directory"
        )));
    }
    let mut nodes = Vec::new();
    collect_output_nodes_inner(root, root, package, &mut nodes)?;
    Ok(nodes)
}

fn collect_output_nodes_inner(
    root: &Path,
    current: &Path,
    package: &str,
    nodes: &mut Vec<OutputNode>,
) -> io::Result<()> {
    let mut children = fs::read_dir(current)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<io::Result<Vec<_>>>()?;
    children.sort();
    for child in children {
        if nodes.len() >= MAX_PROFILE_NODES {
            return Err(io::Error::other(
                "profile output contains too many filesystem nodes",
            ));
        }
        let relative = child
            .strip_prefix(root)
            .map_err(|_| io::Error::other("profile output path escaped its root"))?;
        let path = relative
            .to_str()
            .ok_or_else(|| io::Error::other("profile output path is not UTF-8"))?
            .replace(std::path::MAIN_SEPARATOR, "/");
        if path.is_empty() || path.split('/').any(|part| part == ".." || part.is_empty()) {
            return Err(io::Error::other(
                "profile output contains an unsafe relative path",
            ));
        }
        let metadata = fs::symlink_metadata(&child)?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            let target = fs::read_link(&child)?;
            let digest = format!(
                "sha256-{}",
                SHA256::sha256_hex(format!("symlink:{}", target.to_string_lossy()).as_bytes())
            );
            nodes.push(OutputNode {
                package: package.to_string(),
                path,
                kind: OutputNodeKind::Symlink,
                digest,
                source: child,
                link_target: Some(target),
            });
        } else if metadata.is_dir() {
            nodes.push(OutputNode {
                package: package.to_string(),
                path,
                kind: OutputNodeKind::Directory,
                digest: String::new(),
                source: child.clone(),
                link_target: None,
            });
            collect_output_nodes_inner(root, &child, package, nodes)?;
        } else if metadata.is_file() {
            nodes.push(OutputNode {
                package: package.to_string(),
                path,
                kind: OutputNodeKind::File,
                digest: format!("sha256-{}", SHA256::sha256_file_hex(&child)?),
                source: child,
                link_target: None,
            });
        } else {
            return Err(io::Error::other(format!(
                "E1335: profile output contains unsupported node `{path}`"
            )));
        }
    }
    Ok(())
}

fn copy_projection(root: &Path, selected: &BTreeMap<String, OutputNode>) -> io::Result<()> {
    for (relative, node) in selected {
        let destination = safe_projection_path(root, relative)?;
        match node.kind {
            OutputNodeKind::Directory => {
                if let Ok(metadata) = fs::symlink_metadata(&destination) {
                    if metadata.file_type().is_symlink() || !metadata.is_dir() {
                        return Err(io::Error::other(format!(
                            "profile projection type changed at `{relative}`"
                        )));
                    }
                } else {
                    fs::create_dir_all(&destination)?;
                }
            }
            OutputNodeKind::File => {
                ensure_projection_parent(&destination)?;
                copy_projection_file(&node.source, &destination, &node.digest)?;
            }
            OutputNodeKind::Symlink => {
                ensure_projection_parent(&destination)?;
                #[cfg(unix)]
                std::os::unix::fs::symlink(
                    node.link_target
                        .as_ref()
                        .ok_or_else(|| io::Error::other("profile symlink lost its target"))?,
                    &destination,
                )?;
                #[cfg(windows)]
                return Err(io::Error::other(
                    "profile generation cannot materialize symlinks on this platform",
                ));
            }
        }
    }
    Store::sync_store_directory(root)
}

fn safe_projection_path(root: &Path, relative: &str) -> io::Result<PathBuf> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(io::Error::other("profile projection path is unsafe"));
    }
    Ok(root.join(path))
}

fn ensure_projection_parent(path: &Path) -> io::Result<()> {
    let Some(parent) = path.parent() else {
        return Err(io::Error::other("profile projection has no parent"));
    };
    fs::create_dir_all(parent)
}

fn copy_projection_file(source: &Path, destination: &Path, expected: &str) -> io::Result<()> {
    let source_metadata = fs::symlink_metadata(source)?;
    if source_metadata.file_type().is_symlink() || !source_metadata.is_file() {
        return Err(io::Error::other(
            "profile projection source is not a regular file",
        ));
    }
    fs::copy(source, destination)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(
            destination,
            fs::Permissions::from_mode(source_metadata.permissions().mode() & 0o777),
        )?;
    }
    let actual = format!("sha256-{}", SHA256::sha256_file_hex(destination)?);
    if actual != expected {
        return Err(io::Error::other(
            "profile projection source changed while copying",
        ));
    }
    Ok(())
}

fn profile_state_dir(project_dir: &Path, name: &str) -> PathBuf {
    Store::managed_dir(project_dir)
        .join(PROFILE_STATE_DIR)
        .join(name)
}

fn generations_dir(state: &Path) -> PathBuf {
    state.join(PROFILE_GENERATIONS_DIR)
}

fn profile_owner(project_dir: &Path) -> String {
    let digest = SHA256::sha256_hex(project_dir.to_string_lossy().as_bytes());
    format!("project-{}", &digest[..16])
}

fn next_generation(state: &Path) -> io::Result<u64> {
    let directory = generations_dir(state);
    fs::create_dir_all(&directory)?;
    let mut maximum = 0;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() || !file_type.is_dir() {
            continue;
        }
        if let Some(generation) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u64>().ok())
        {
            maximum = maximum.max(generation);
        }
    }
    maximum
        .checked_add(1)
        .ok_or_else(|| io::Error::other("profile generation number overflow"))
}

fn read_generations(state: &Path) -> io::Result<Vec<GenerationRecord>> {
    let directory = generations_dir(state);
    let metadata = match fs::symlink_metadata(&directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::other(
            "profile generations path is not a real directory",
        ));
    }
    let mut numbers = fs::read_dir(&directory)?
        .map(|entry| {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_symlink() || !file_type.is_dir() {
                return Ok(None);
            }
            Ok(entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u64>().ok()))
        })
        .collect::<io::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    numbers.sort_unstable();
    let mut records = Vec::new();
    for generation in numbers {
        let complete = directory
            .join(generation.to_string())
            .join(PROFILE_COMPLETE_FILE);
        match fs::symlink_metadata(&complete) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(io::Error::other(
                    "profile generation completion marker is not a regular file",
                ));
            }
            Err(error) => return Err(error),
            Ok(_) => records.push(read_generation_record(state, generation)?),
        }
    }
    Ok(records)
}

fn read_generation_record(state: &Path, generation: u64) -> io::Result<GenerationRecord> {
    let directory = generations_dir(state).join(generation.to_string());
    let metadata_path = directory.join(PROFILE_META_FILE);
    let metadata = super::tool::read_bounded(&metadata_path)?;
    let value = JSON::parse(&metadata).map_err(io::Error::other)?;
    let object = value.as_object().map_err(io::Error::other)?;
    let schema = object
        .get("schema")
        .ok_or_else(|| io::Error::other("profile generation metadata lacks schema"))?
        .as_str()
        .map_err(io::Error::other)?;
    if schema != PROFILE_SCHEMA {
        return Err(io::Error::other(
            "profile generation metadata schema is unsupported",
        ));
    }
    let actual_generation = object
        .get("generation")
        .and_then(|value| match value {
            crate::JSON::JSONValue::Number(value) => Some(*value as u64),
            _ => None,
        })
        .ok_or_else(|| io::Error::other("profile generation metadata lacks generation"))?;
    if actual_generation != generation {
        return Err(io::Error::other(
            "profile generation metadata number disagrees with its path",
        ));
    }
    let fingerprint = object
        .get("fingerprint")
        .ok_or_else(|| io::Error::other("profile generation metadata lacks fingerprint"))?
        .as_str()
        .map_err(io::Error::other)?
        .to_string();
    if fingerprint.is_empty() {
        return Err(io::Error::other("profile generation fingerprint is empty"));
    }
    if !valid_digest(&fingerprint) {
        return Err(io::Error::other(
            "profile generation fingerprint is not canonical",
        ));
    }
    let packages = object
        .get("packages")
        .ok_or_else(|| io::Error::other("profile generation metadata lacks packages"))?
        .as_array()
        .map_err(io::Error::other)?;
    let mut targets = BTreeSet::new();
    for package in packages {
        let package = package.as_object().map_err(io::Error::other)?;
        let output_hash = package
            .get("output_hash")
            .ok_or_else(|| io::Error::other("profile generation package lacks output hash"))?
            .as_str()
            .map_err(io::Error::other)?;
        if !valid_digest(output_hash) {
            return Err(io::Error::other(
                "profile generation package has invalid output hash",
            ));
        }
        let facts = package
            .get("provider_facts")
            .ok_or_else(|| io::Error::other("profile generation package lacks provider facts"))?;
        let facts = facts.as_object().map_err(io::Error::other)?;
        for key in [
            "schema",
            "provider",
            "reference",
            "selector",
            "facts",
            "losses",
            "conflicts",
        ] {
            if !facts.contains_key(key) {
                return Err(io::Error::other(format!(
                    "profile generation provider facts lack `{key}`"
                )));
            }
        }
        targets.insert(output_hash.to_string());
    }
    let marker = super::tool::read_bounded(&directory.join(PROFILE_COMPLETE_FILE))?;
    let marker = marker.trim();
    if !valid_digest(marker) {
        return Err(io::Error::other(
            "profile generation completion witness is invalid",
        ));
    }
    let expected = super::tool::profile_generation_witness(&metadata, &targets);
    if marker != expected {
        return Err(io::Error::other(
            "profile generation completion witness mismatches metadata",
        ));
    }
    let root = directory.join(PROFILE_ROOTFS_DIR);
    let root_metadata = fs::symlink_metadata(root)?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(io::Error::other(
            "profile generation root is not a real directory",
        ));
    }
    Ok(GenerationRecord {
        generation,
        fingerprint,
        witness: marker.to_string(),
        metadata,
    })
}

fn read_current_pointer(state: &Path) -> io::Result<Option<CurrentPointer>> {
    let path = state.join(PROFILE_CURRENT_FILE);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(io::Error::other(
            "profile current pointer is not a regular file",
        ));
    }
    let text = super::tool::read_bounded(&path)?;
    let value = JSON::parse(&text).map_err(io::Error::other)?;
    let object = value.as_object().map_err(io::Error::other)?;
    let schema = object
        .get("schema")
        .ok_or_else(|| io::Error::other("profile current pointer lacks schema"))?
        .as_str()
        .map_err(io::Error::other)?;
    if schema != PROFILE_POINTER_SCHEMA {
        return Err(io::Error::other(
            "profile current pointer schema is unsupported",
        ));
    }
    let generation = object
        .get("generation")
        .and_then(|value| match value {
            crate::JSON::JSONValue::Number(value) => Some(*value as u64),
            _ => None,
        })
        .ok_or_else(|| io::Error::other("profile current pointer lacks generation"))?;
    let witness = object
        .get("witness")
        .ok_or_else(|| io::Error::other("profile current pointer lacks witness"))?
        .as_str()
        .map_err(io::Error::other)?
        .to_string();
    if !valid_digest(&witness) {
        return Err(io::Error::other(
            "profile current pointer witness is invalid",
        ));
    }
    Ok(Some(CurrentPointer {
        generation,
        witness,
    }))
}

fn activate_generation(
    project_dir: &Path,
    profile: &str,
    generation: u64,
    expected_fingerprint: Option<&str>,
) -> io::Result<GenerationRecord> {
    let state = profile_state_dir(project_dir, profile);
    let record = read_generation_record(&state, generation)?;
    if let Some(expected) = expected_fingerprint {
        if record.fingerprint != expected {
            return Err(io::Error::other(
                "profile generation is stale for the current source plan; rebuild before switching",
            ));
        }
    }
    fs::create_dir_all(&state)?;
    let pointer = format!(
        "{{\"schema\":{},\"generation\":{},\"witness\":{}}}\n",
        JSON::quote(PROFILE_POINTER_SCHEMA),
        generation,
        JSON::quote(&record.witness),
    );
    let partial = state.join(PROFILE_CURRENT_PARTIAL);
    if fs::symlink_metadata(&partial).is_ok() {
        fs::remove_file(&partial)?;
    }
    super::tool::write_synced(&partial, pointer.as_bytes())?;
    fs::rename(&partial, state.join(PROFILE_CURRENT_FILE))?;
    Store::sync_store_directory(&state)?;
    if let Some(current) = read_current_pointer(&state)? {
        if current.generation != generation || current.witness != record.witness {
            return Err(io::Error::other(
                "profile current pointer failed post-write verification",
            ));
        }
    }
    Ok(record)
}

fn report_generation_error(theme: &Theme, error: &io::Error) -> i32 {
    let message = error.to_string();
    if let Some(message) = message.strip_prefix("E1335: ") {
        theme.error_coded(
            "E1335",
            message,
            "package-generation provider facts and exact path collisions must remain explicit",
            "pin the provider or add an exact collision selection, then rebuild",
        );
    } else {
        theme.error(
            "package generation failed",
            &message,
            "repair the reported source, provider, or generation state and retry",
        );
    }
    2
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256-") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
