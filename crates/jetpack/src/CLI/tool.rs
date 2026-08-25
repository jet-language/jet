//! D-JPK-TOOLRUN1=A: `jetpack tool run|install|list|uninstall`.
//!
//! Ephemeral `tool run` realizes a ref through every built-in provider and
//! execs its binary once (nothing stays on PATH). Persistent `tool install`
//! projects bins into `~/.jet/bin` with per-install generation metadata under
//! `~/.jet/tools/` — a minimal isolated install until the shared
//! D-JPK-PROFILE1 `jet profile` surface is the front door. A bin name that
//! collides with a project `#Job fn` is E1297 (JPK-TOOL-COLLIDE).

use super::parse::Parsed;
use super::realize::{
    channel_sources, classify_or_report, realize_ref_outcome, RealizeScope, report_nix_bridge_required,
    resolve_source_channel, RefOutcome, RowStyle, RunPlan,
};
use super::trust_env_build::compose_env_scoped;
use super::workspace_sources::cwd_table;
use super::ProfileDispatch;
use crate::Output::Theme;
use crate::RefSpec::{self, ChannelPolicy, ProviderKind};
use crate::RuntimePolicy;
use crate::Shell;
use crate::Store;
use crate::Syntax;
use crate::JSON;
use crate::SHA256;
use jet_env_model::ModuleEval;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// `jetpack tool <verb> …`
pub(super) fn cmd_tool(theme: &Theme, parsed: &Parsed) -> i32 {
    match parsed.positional.first().map(String::as_str) {
        Some(v) if v == Syntax::TOOL_VERB_RUN => tool_run(theme, parsed),
        Some(v) if v == Syntax::TOOL_VERB_INSTALL => tool_install(theme, parsed),
        Some(v) if v == Syntax::TOOL_VERB_LIST => tool_list(theme),
        Some(v) if v == Syntax::TOOL_VERB_UNINSTALL => tool_uninstall(theme, parsed),
        Some(other) => {
            theme.error(
                &format!("`{other}` is not a jetpack tool verb"),
                &format!(
                    "`jetpack tool` verbs are: {}.",
                    Syntax::TOOL_VERBS.join(", ")
                ),
                "try `jetpack tool run <ref>`, `jetpack tool install <ref>`, `jetpack tool list`, or `jetpack tool uninstall <name>`.",
            );
            2
        }
        None => {
            theme.error(
                "`jetpack tool` needs a verb",
                &format!(
                    "verbs are: {} — ephemeral run or persistent PATH install (D-JPK-TOOLRUN1).",
                    Syntax::TOOL_VERBS.join(", ")
                ),
                "try `jetpack tool run ripgrep@nixpkgs -- rg --version`.",
            );
            2
        }
    }
}

/// The standalone `~/.jet/tools` profile uses the same generation engine as
/// `jetpack tool install`, but its declaration is independent of any project
/// `env.jet`.
pub(super) fn cmd_tool_profile(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(verb) = parsed.positional.first().map(String::as_str) else {
        theme.error(
            "`jetpack profile` needs a verb",
            "the standalone user-tools profile accepts plan, build, switch, rollback, or generations",
            "try `jetpack profile switch tools`",
        );
        return 2;
    };
    if parsed.positional.get(1).map(String::as_str) != Some(Syntax::TOOL_PROFILE_NAME) {
        return 2;
    }
    match verb {
        v if v == Syntax::PROFILE_VERB_PLAN => tool_profile_plan(theme, parsed),
        v if v == Syntax::PROFILE_VERB_BUILD => tool_profile_build(theme, parsed),
        v if v == Syntax::PROFILE_VERB_SWITCH => tool_profile_switch(theme, parsed),
        v if v == Syntax::PROFILE_VERB_ROLLBACK => tool_profile_rollback(theme, parsed),
        v if v == Syntax::PROFILE_VERB_GENERATIONS => tool_profile_generations(theme, parsed),
        _ => 2,
    }
}

fn tool_profile_args(
    theme: &Theme,
    parsed: &Parsed,
    verb: &str,
    allow_generation: bool,
) -> Option<Option<u64>> {
    let valid = if allow_generation {
        parsed.positional.len() == 2 || parsed.positional.len() == 3
    } else {
        parsed.positional.len() == 2
    };
    if !valid || parsed.command.is_some() {
        theme.error(
            &format!("`jetpack profile {verb} tools` has invalid arguments"),
            "the standalone user-tools profile has no trailing command",
            if allow_generation {
                "try `jetpack profile rollback tools 2`"
            } else {
                "try `jetpack profile plan tools`"
            },
        );
        return None;
    }
    let generation = parsed
        .positional
        .get(2)
        .map(|value| value.parse::<u64>().ok().filter(|number| *number > 0));
    if allow_generation && parsed.positional.get(2).is_some() && generation.flatten().is_none() {
        theme.error(
            "tool profile rollback generation is invalid",
            "generation numbers are positive decimal integers",
            "try `jetpack profile rollback tools 2`",
        );
        return None;
    }
    Some(generation.flatten())
}

fn tool_profile_plan(theme: &Theme, parsed: &Parsed) -> i32 {
    if tool_profile_args(theme, parsed, Syntax::PROFILE_VERB_PLAN, false).is_none() {
        return 2;
    }
    let manifest = match read_tool_manifest() {
        Ok(manifest) => manifest,
        Err(error) => return report_tool_profile_error(theme, &error),
    };
    if parsed.flags.json {
        println!("{}", format_tool_manifest(&manifest));
        return 0;
    }
    theme.ok(&format!(
        "user-tools profile {}",
        theme.bold(Syntax::TOOL_PROFILE_NAME)
    ));
    theme.detail(&format!("declaration: {}", tool_manifest_path().display()));
    if manifest.tools.is_empty() {
        theme.detail("tools: none");
    } else {
        for tool in &manifest.tools {
            let policy = if tool.tier == "pinned" {
                String::new()
            } else {
                format!("{} ", tool.tier)
            };
            theme.detail(&format!("{}{} -> {}", policy, tool.name, tool.resolved));
        }
    }
    0
}

fn tool_profile_build(theme: &Theme, parsed: &Parsed) -> i32 {
    if tool_profile_args(theme, parsed, Syntax::PROFILE_VERB_BUILD, false).is_none() {
        return 2;
    }
    match build_tool_manifest_generation(theme, parsed) {
        Ok(generation) => {
            if parsed.flags.json {
                println!(
                    "{{\"profile\":{},\"generation\":{}}}",
                    JSON::quote(Syntax::TOOL_PROFILE_NAME),
                    generation
                );
            } else {
                theme.ok(&format!(
                    "user-tools profile built as generation {generation}"
                ));
            }
            0
        }
        Err(error) => report_tool_profile_error(theme, &error),
    }
}

fn tool_profile_switch(theme: &Theme, parsed: &Parsed) -> i32 {
    if tool_profile_args(theme, parsed, Syntax::PROFILE_VERB_SWITCH, false).is_none() {
        return 2;
    }
    let generation = match build_tool_manifest_generation(theme, parsed) {
        Ok(generation) => generation,
        Err(error) => return report_tool_profile_error(theme, &error),
    };
    let result = RuntimePolicy::with_lock(&tools_state_dir(), PROFILE_LOCK_SCOPE, || {
        recover_profile_state()?;
        activate_generation_locked(generation)
    });
    match result {
        Ok(()) => {
            if parsed.flags.json {
                println!(
                    "{{\"profile\":{},\"current\":{}}}",
                    JSON::quote(Syntax::TOOL_PROFILE_NAME),
                    generation
                );
            } else {
                theme.ok(&format!(
                    "user-tools profile switched to generation {generation}"
                ));
            }
            0
        }
        Err(error) => report_tool_profile_error(theme, &error),
    }
}

fn tool_profile_rollback(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(target) = tool_profile_args(theme, parsed, Syntax::PROFILE_VERB_ROLLBACK, true) else {
        return 2;
    };
    let result = RuntimePolicy::with_lock(&tools_state_dir(), PROFILE_LOCK_SCOPE, || {
        recover_profile_state()?;
        let current = current_generation_locked()?.ok_or_else(|| {
            io::Error::other("user-tools profile has no current generation to roll back")
        })?;
        let generation = match target {
            Some(generation) => generation,
            None => generation_numbers()?
                .into_iter()
                .filter(|generation| *generation < current)
                .max()
                .ok_or_else(|| {
                    io::Error::other("user-tools profile has no older generation to roll back to")
                })?,
        };
        activate_generation_locked(generation)?;
        Ok(generation)
    });
    match result {
        Ok(generation) => {
            if parsed.flags.json {
                println!(
                    "{{\"profile\":{},\"current\":{}}}",
                    JSON::quote(Syntax::TOOL_PROFILE_NAME),
                    generation
                );
            } else {
                theme.ok(&format!(
                    "user-tools profile rolled back to generation {generation}"
                ));
            }
            0
        }
        Err(error) => report_tool_profile_error(theme, &error),
    }
}

fn tool_profile_generations(theme: &Theme, parsed: &Parsed) -> i32 {
    if tool_profile_args(theme, parsed, Syntax::PROFILE_VERB_GENERATIONS, false).is_none() {
        return 2;
    }
    let result = RuntimePolicy::with_lock(&tools_state_dir(), PROFILE_LOCK_SCOPE, || {
        recover_profile_state()?;
        let current = current_generation_locked()?;
        let generations = generation_numbers()?
            .into_iter()
            .map(|generation| {
                Ok((
                    generation,
                    generation_disk_bytes(generation)?,
                    current == Some(generation),
                ))
            })
            .collect::<io::Result<Vec<_>>>()?;
        let state_bytes = profile_state_disk_bytes()?;
        let bin_bytes = profile_bin_disk_bytes()?;
        Ok((generations, state_bytes, bin_bytes))
    });
    let (generations, state_bytes, bin_bytes) = match result {
        Ok(value) => value,
        Err(error) => return report_tool_profile_error(theme, &error),
    };
    let generations_bytes = generations.iter().map(|(_, bytes, _)| *bytes).sum::<u64>();
    let profile_bytes = state_bytes.saturating_add(bin_bytes);
    if parsed.flags.json {
        let entries = generations
            .iter()
            .map(|(generation, bytes, current)| {
                format!("{{\"generation\":{generation},\"current\":{current},\"bytes\":{bytes}}}")
            })
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{{\"profile\":{},\"profile_bytes\":{profile_bytes},\"generations_bytes\":{generations_bytes},\"total_bytes\":{},\"generations\":[{}]}}",
            JSON::quote(Syntax::TOOL_PROFILE_NAME),
            profile_bytes.saturating_add(generations_bytes),
            entries
        );
        return 0;
    }
    println!("PROFILE  BYTES");
    println!("files    {profile_bytes}");
    println!("generations {generations_bytes}");
    for (generation, bytes, current) in generations.iter().rev() {
        println!(
            "generation {generation:<4} {bytes:>10}{}",
            if *current { "  current" } else { "" }
        );
    }
    println!(
        "total    {}",
        profile_bytes.saturating_add(generations_bytes)
    );
    0
}

fn report_tool_profile_error(theme: &Theme, error: &io::Error) -> i32 {
    theme.error(
        "user-tools profile failed",
        &error.to_string(),
        "repair the standalone declaration or profile state and retry",
    );
    2
}

/// Refresh the moving source pins recorded by the standalone user-tools
/// declaration. `#latest` reaches this path only through an explicit update;
/// `#auto` is also refreshed by profile realization.
pub(super) fn update_user_tools(theme: &Theme, parsed: &Parsed) -> i32 {
    if parsed.flags.offline {
        theme.error(
            "cannot update user-tools offline",
            "channel movement needs the configured source metadata or a captured fixture",
            "remove `--offline` and run `jetpack update tools` again",
        );
        return 2;
    }
    let result = RuntimePolicy::with_lock(&tools_state_dir(), PROFILE_LOCK_SCOPE, || {
        recover_profile_state()?;
        let mut manifest = read_tool_manifest()?;
        let changed = refresh_tool_manifest_sources(theme, parsed, &mut manifest, true)?;
        if changed {
            if !theme.confirm_apply(parsed.flags.assume_yes) {
                return Ok(false);
            }
            write_tool_manifest(&manifest)?;
        }
        Ok(changed)
    });
    match result {
        Ok(true) => {
            theme.ok("user-tools channel pins updated");
            0
        }
        Ok(false) => {
            theme.status("no moving user-tools channels to update");
            0
        }
        Err(error) => report_tool_profile_error(theme, &error),
    }
}

fn tool_run(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(raw) = parsed.positional.get(1) else {
        theme.error(
            "`jetpack tool run` needs a package ref",
            "ephemeral tool execution realizes one `package@source` ref and runs its binary once — nothing is left on PATH.",
            "try `jetpack tool run ripgrep@nixpkgs -- rg --version`.",
        );
        return 2;
    };
    if let Some(code) = reject_unavailable_provider(theme, raw) {
        return code;
    }
    let Ok(spec) = classify_or_report(theme, raw) else {
        return 2;
    };
    let roots = Store::resolve();
    let plan = RunPlan {
        project_root: std::env::current_dir().unwrap_or_default(),
        refs: vec![spec.clone()],
        adapters: Vec::new(),
        table: cwd_table(),
        label: Syntax::JETPACK_PROMPT_LABEL.to_string(),
        prompt_path: ModuleEval::PromptPathMode::default(),
        prompt_strip: ModuleEval::PromptStripMode::default(),
        dev_services: Vec::new(),
        secrets: Vec::new(),
        environment: ModuleEval::EnvironmentFacts::default(),
    };
    let env = match compose_env_scoped(theme, &roots, &parsed.flags, &plan, RealizeScope::UserProfile) {
        Ok(env) => env,
        Err(code) => return code,
    };
    let program = match &parsed.command {
        Some(cmd) if !cmd.is_empty() => cmd.clone(),
        _ => vec![spec.short_name().to_string()],
    };
    theme.status(&format!(
        "tool run {} -> {} (ephemeral)",
        theme.bold(&spec.raw),
        theme.bold(program.first().map(String::as_str).unwrap_or("?"))
    ));
    Shell::run_command(&env, &program)
}

fn tool_install(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(raw) = parsed.positional.get(1) else {
        theme.error(
            "`jetpack tool install` needs a package ref",
            "persistent install realizes one `package@source` ref and projects its bins onto `~/.jet/bin` as a tool generation.",
            "try `jetpack tool install ripgrep@nixpkgs`.",
        );
        return 2;
    };
    let (channel_policy, raw_ref) = RefSpec::split_channel_policy(raw);
    if let Some(code) = reject_unavailable_provider(theme, raw_ref) {
        return code;
    }
    let Ok(spec) = classify_or_report(theme, raw_ref) else {
        return 2;
    };
    let as_name = parsed.flags.as_name.as_deref();
    let project_dir = std::env::current_dir().unwrap_or_default();
    // Early collision check on the projected name (package short name or --as).
    let preview = as_name.unwrap_or(spec.short_name());
    if let Some((job, path)) = find_job_collision(&project_dir, preview) {
        report_collide(theme, preview, &job, &path, raw);
        return 2;
    }
    let roots = Store::resolve();
    let plan = RunPlan {
        project_root: project_dir.clone(),
        refs: vec![spec.clone()],
        adapters: Vec::new(),
        table: cwd_table(),
        label: Syntax::JETPACK_PROMPT_LABEL.to_string(),
        prompt_path: ModuleEval::PromptPathMode::default(),
        prompt_strip: ModuleEval::PromptStripMode::default(),
        dev_services: Vec::new(),
        secrets: Vec::new(),
        environment: ModuleEval::EnvironmentFacts::default(),
    };
    let env = match compose_env_scoped(theme, &roots, &parsed.flags, &plan, RealizeScope::UserProfile) {
        Ok(env) => env,
        Err(code) => return code,
    };
    let Some(lease) = env.cache_leases.first() else {
        theme.error(
            &format!("`{}` has no bin directory to install", spec.raw),
            "tool install projects package binaries onto PATH; this realization produced no `bin/`.",
            "pick a package that ships executables, or use `jetpack tool run` for a one-shot.",
        );
        return 2;
    };
    let receipt = match lease.profile_install_receipt() {
        Ok(receipt) => receipt,
        Err(error) => {
            theme.error(
                "couldn't pin verified tool realization",
                &error.to_string(),
                "retry realization; Jetpack will not publish an unverified profile member.",
            );
            return 2;
        }
    };
    let bins = match collect_receipt_bins(&receipt, as_name) {
        Ok(bins) if !bins.is_empty() => bins,
        Ok(_) => {
            theme.error(
                &format!("`{}` ships no executables under bin/", spec.raw),
                "tool install needs at least one executable to project onto `~/.jet/bin`.",
                "use `jetpack tool run` for a one-shot, or pick a different package.",
            );
            return 2;
        }
        Err(e) => {
            theme.error(
                "couldn't read package bin directory",
                &e,
                "check the realized package output and retry.",
            );
            return 2;
        }
    };
    for (bin_name, _) in &bins {
        if bin_name == preview {
            continue;
        }
        if let Some((job, path)) = find_job_collision(&project_dir, bin_name) {
            report_collide(theme, bin_name, &job, &path, raw);
            return 2;
        }
    }
    match project_install(theme, &roots, &spec, &receipt, lease, &bins, channel_policy) {
        Ok((gen, version)) => {
            for (name, _) in &bins {
                let link = user_bin_dir().join(name);
                let ver = if version.is_empty() {
                    String::new()
                } else {
                    format!(" {version}")
                };
                theme.status(&format!(
                    "installed {}{ver}  ->  {}   (generation \"{}\", generation {})",
                    theme.bold(spec.short_name()),
                    link.display(),
                    Syntax::TOOL_PROFILE_NAME,
                    gen
                ));
            }
            0
        }
        Err(e) => {
            theme.error(
                "tool install failed",
                &e,
                "check permissions on `~/.jet` and retry.",
            );
            2
        }
    }
}

fn tool_list(theme: &Theme) -> i32 {
    let tools = match RuntimePolicy::with_lock(&tools_state_dir(), PROFILE_LOCK_SCOPE, || {
        recover_profile_state()?;
        read_current_tools().map_err(io::Error::other)
    }) {
        Ok(t) => t,
        Err(e) => {
            theme.error(
                "couldn't read installed tools",
                &e.to_string(),
                "check `~/.jet/tools`.",
            );
            return 2;
        }
    };
    if tools.is_empty() {
        theme.status("no tools installed yet.");
        return 0;
    }
    println!("TOOL     VERSION  SOURCE  BIN");
    for t in tools {
        println!(
            "{:<8} {:<8} {:<7} {}",
            t.name,
            if t.version.is_empty() {
                "-"
            } else {
                &t.version
            },
            t.source,
            t.bins.join(",")
        );
    }
    0
}

fn tool_uninstall(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(name) = parsed.positional.get(1) else {
        theme.error(
            "`jetpack tool uninstall` needs a tool name",
            "names match `jetpack tool list` (package short name or projected bin).",
            "try `jetpack tool uninstall ripgrep`.",
        );
        return 2;
    };
    match uninstall_tool(theme, name) {
        Ok(true) => {
            theme.status(&format!("removed {name}"));
            0
        }
        Ok(false) => {
            theme.error(
                &format!("`{name}` is not an installed tool"),
                "uninstall only removes packages previously added with `jetpack tool install`.",
                "run `jetpack tool list` to see what's installed.",
            );
            2
        }
        Err(e) => {
            theme.error(
                "tool uninstall failed",
                &e,
                "check `~/.jet/tools` and retry.",
            );
            2
        }
    }
}

fn reject_unavailable_provider(theme: &Theme, raw: &str) -> Option<i32> {
    let source = raw.rsplit_once(Syntax::REF_PROVIDER_AT).map(|(_, s)| s)?;
    if !Syntax::TOOL_EXTERNAL_PROVIDERS.contains(&source) {
        return None;
    }
    theme.error_coded(
        Syntax::TOOL_DIAG_PROVIDER,
        &format!("tool provider `{source}` isn't available yet"),
        &format!(
            "D-JPK-TOOLRUN1 runs tools across providers, but `…@{source}` has no hangar realization path yet (JPK-TOOL-PROVIDER). Built-in providers that work today: nixpkgs, github, and bare local paths."
        ),
        &format!(
            "use a built-in ref (`…@nixpkgs`, `…@github`, or a bare local path), or wait for the `{source}` provider to land."
        ),
    );
    Some(2)
}

fn report_collide(theme: &Theme, bin: &str, job: &str, path: &Path, raw: &str) {
    let rel = path
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| format!("./{s}"))
        .unwrap_or_else(|| path.display().to_string());
    theme.error_coded(
        Syntax::TOOL_DIAG_COLLIDE,
        &format!("`{bin}` is already a job in {rel}"),
        &format!(
            "the project job `{job}` wins in this directory, so the global tool would be shadowed here (JPK-TOOL-COLLIDE / D-JPK-TOOLRUN1)."
        ),
        &format!(
            "install under a different bin name  ->  jetpack tool install {raw} {} <other>\n     or just run it once                  ->  jetpack tool run {raw}",
            Syntax::TOOL_FLAG_AS
        ),
    );
}

/// Scan project `.jet` sources for `#Job fn <name>` matching `bin`.
fn find_job_collision(dir: &Path, bin: &str) -> Option<(String, PathBuf)> {
    let mut files = Vec::new();
    collect_jet_files(dir, &mut files, 0);
    for path in files {
        let Ok(src) = fs::read_to_string(&path) else {
            continue;
        };
        for name in task_names_in(&src) {
            if name == bin {
                return Some((name, path));
            }
        }
    }
    None
}

fn collect_jet_files(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "build" || name == "target" || name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect_jet_files(&path, out, depth + 1);
        } else if path.extension().and_then(|e| e.to_str()) == Some(Syntax::FILE_EXT) {
            out.push(path);
        }
    }
}

fn task_names_in(src: &str) -> Vec<String> {
    let mut names = Vec::new();
    let bytes = src.as_bytes();
    let needle = b"#Job";
    let mut i = 0;
    while i + needle.len() < bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            let after = &src[i + needle.len()..];
            let trimmed = after.trim_start();
            // Optional `#Every(…)` between `#Job` and `fn`.
            let after_every = if trimmed.starts_with('#') {
                // skip one more marker + optional (…)
                let mut rest = trimmed;
                if let Some(idx) = rest.find("fn") {
                    rest = &rest[idx..];
                }
                rest
            } else {
                trimmed
            };
            let rest = after_every.trim_start();
            if let Some(rest) = rest.strip_prefix("fn") {
                let rest = rest.trim_start();
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    names.push(name);
                }
            }
            i += needle.len();
        } else {
            i += 1;
        }
    }
    names
}

fn collect_receipt_bins(
    receipt: &Store::ProfileInstallReceipt,
    as_name: Option<&str>,
) -> Result<Vec<(String, String)>, String> {
    let mut bins = receipt
        .executable_members
        .iter()
        .map(|member| {
            validate_bin_name(member)?;
            Ok((as_name.unwrap_or(member).to_string(), member.clone()))
        })
        .collect::<Result<Vec<_>, String>>()?;
    if as_name.is_some() {
        bins.truncate(1);
    }
    for (projected, _) in &bins {
        validate_bin_name(projected)?;
    }
    bins.sort();
    Ok(bins)
}

#[derive(Clone)]
struct InstalledTool {
    name: String,
    version: String,
    source: String,
    reference: String,
    bins: Vec<String>,
    members: Vec<String>,
    member_digests: Vec<String>,
    output_hash: String,
    store_root: String,
}

#[derive(Clone, Default)]
struct ToolManifest {
    sources: Vec<ManifestSource>,
    tools: Vec<ManifestTool>,
}

#[derive(Clone)]
struct ManifestSource {
    name: String,
    upstream: String,
    provider: ProviderKind,
    policy: ChannelPolicy,
    raw: String,
}

#[derive(Clone)]
struct ManifestTool {
    name: String,
    reference: String,
    resolved: String,
    tier: String,
    bins: Vec<String>,
    members: Vec<String>,
}

fn tool_policy_marker(policy: ChannelPolicy) -> &'static str {
    match policy {
        ChannelPolicy::Pinned => "pinned",
        ChannelPolicy::Manual => "#latest",
        ChannelPolicy::Automatic => "#auto",
    }
}

fn source_policy_from_marker(marker: &str) -> Result<ChannelPolicy, String> {
    match marker {
        "pinned" => Ok(ChannelPolicy::Pinned),
        "#latest" => Ok(ChannelPolicy::Manual),
        "#auto" => Ok(ChannelPolicy::Automatic),
        _ => Err(format!("unknown user-tools source policy `{marker}`")),
    }
}

fn tool_manifest_ref(raw: &str, policy: ChannelPolicy) -> String {
    match policy {
        ChannelPolicy::Pinned => raw.to_string(),
        ChannelPolicy::Manual => format!("#latest {raw}"),
        ChannelPolicy::Automatic => format!("#auto {raw}"),
    }
}

fn source_manifest_ref(upstream: &str, policy: ChannelPolicy) -> String {
    let exact = upstream
        .split_once(Syntax::REF_SEPARATOR)
        .map(|(provider, target)| format!("{target}@{provider}"))
        .unwrap_or_else(|| upstream.to_string());
    tool_manifest_ref(&exact, policy)
}

fn source_table_from_manifest(manifest: &ToolManifest) -> RefSpec::SourceTable {
    let mut table = RefSpec::SourceTable::from_decls(manifest.sources.iter().map(|source| {
        (
            source.name.clone(),
            source.upstream.clone(),
            source.provider,
        )
    }));
    for source in &manifest.sources {
        table.set_channel_metadata(&source.name, source.policy, source.raw.clone());
    }
    table
}

fn manifest_sources_from_table(table: &RefSpec::SourceTable) -> Vec<ManifestSource> {
    table
        .declarations()
        .into_iter()
        .map(|(name, upstream, provider)| {
            let policy = table.channel_policy(&name);
            let raw = source_manifest_ref(&upstream, policy);
            ManifestSource {
                name,
                upstream,
                provider,
                policy,
                raw,
            }
        })
        .collect()
}

fn read_tool_manifest() -> io::Result<ToolManifest> {
    let path = tool_manifest_path();
    if !path.is_file() {
        return Ok(ToolManifest::default());
    }
    let text = read_bounded(&path)?;
    parse_tool_manifest(&text).map_err(io::Error::other)
}

fn write_tool_manifest(manifest: &ToolManifest) -> io::Result<()> {
    fs::create_dir_all(tools_state_dir())?;
    let partial = tools_state_dir().join(TOOL_MANIFEST_PARTIAL);
    if fs::symlink_metadata(&partial).is_ok() {
        fs::remove_file(&partial)?;
    }
    write_synced(&partial, format_tool_manifest(manifest).as_bytes())?;
    finalize_profile_pointer(&partial, &tool_manifest_path())?;
    Store::sync_store_directory(&tools_state_dir())
}

fn record_tool_manifest_locked(
    tools: &[InstalledTool],
    declared_sources: &RefSpec::SourceTable,
    override_tool: Option<(&str, ChannelPolicy)>,
) -> io::Result<()> {
    let old = read_tool_manifest()?;
    let mut source_map = BTreeMap::<String, ManifestSource>::new();
    for source in old.sources.clone() {
        source_map.insert(source.name.clone(), source);
    }
    for source in manifest_sources_from_table(declared_sources) {
        source_map.insert(source.name.clone(), source);
    }

    let mut manifest_tools = Vec::with_capacity(tools.len());
    for tool in tools {
        let previous = old.tools.iter().find(|entry| {
            entry.name == tool.name
                && (entry.resolved == tool.reference || entry.reference == tool.reference)
        });
        let override_policy = override_tool
            .filter(|(reference, _)| *reference == tool.reference)
            .map(|(_, policy)| policy);
        let (reference, tier) = if let Some(policy) = override_policy {
            (
                tool_manifest_ref(&tool.reference, policy),
                tool_policy_marker(policy).to_string(),
            )
        } else if let Some(previous) = previous {
            (previous.reference.clone(), previous.tier.clone())
        } else {
            (
                tool.reference.clone(),
                tool_policy_marker(ChannelPolicy::Pinned).to_string(),
            )
        };
        manifest_tools.push(ManifestTool {
            name: tool.name.clone(),
            reference,
            resolved: tool.reference.clone(),
            tier,
            bins: tool.bins.clone(),
            members: tool.members.clone(),
        });
    }
    manifest_tools
        .sort_by(|left, right| (&left.name, &left.resolved).cmp(&(&right.name, &right.resolved)));
    let manifest = ToolManifest {
        sources: source_map.into_values().collect(),
        tools: manifest_tools,
    };
    write_tool_manifest(&manifest)
}

fn parse_tool_manifest(text: &str) -> Result<ToolManifest, String> {
    let JSON::JSONValue::Object(root) = JSON::parse(text)? else {
        return Err("user-tools manifest root is not an object".into());
    };
    expect_exact_keys(
        &root,
        &["profile", "schema", "sources", "tools"],
        "user-tools manifest",
    )?;
    if json_field_string(&root, "schema")? != TOOL_MANIFEST_SCHEMA
        || json_field_string(&root, "profile")? != Syntax::TOOL_PROFILE_NAME
    {
        return Err("user-tools manifest schema or profile identity mismatch".into());
    }

    let JSON::JSONValue::Array(source_values) = root.get("sources").unwrap() else {
        return Err("user-tools manifest sources field is not an array".into());
    };
    if source_values.len() > MAX_PROFILE_TOOLS {
        return Err("user-tools source count exceeds bound".into());
    }
    let mut sources = Vec::with_capacity(source_values.len());
    let mut source_names = BTreeSet::new();
    for value in source_values {
        let JSON::JSONValue::Object(source) = value else {
            return Err("user-tools manifest source entry is not an object".into());
        };
        expect_exact_keys(
            source,
            &["name", "policy", "provider", "raw", "upstream"],
            "user-tools manifest source",
        )?;
        let name = bounded_json_string(source, "name")?;
        if !source_names.insert(name.clone()) {
            return Err(format!("duplicate user-tools source `{name}`"));
        }
        let upstream = bounded_json_string(source, "upstream")?;
        let raw = bounded_json_string(source, "raw")?;
        let provider_name = bounded_json_string(source, "provider")?;
        let provider = manifest_provider(&provider_name)?;
        let policy = source_policy_from_marker(json_field_string(source, "policy")?)?;
        sources.push(ManifestSource {
            name,
            upstream,
            provider,
            policy,
            raw,
        });
    }
    sources.sort_by(|left, right| left.name.cmp(&right.name));

    let JSON::JSONValue::Array(tool_values) = root.get("tools").unwrap() else {
        return Err("user-tools manifest tools field is not an array".into());
    };
    if tool_values.len() > MAX_PROFILE_TOOLS {
        return Err("user-tools tool count exceeds bound".into());
    }
    let mut tools = Vec::with_capacity(tool_values.len());
    let mut tool_names = BTreeSet::new();
    let mut bins = BTreeSet::new();
    for value in tool_values {
        let JSON::JSONValue::Object(tool) = value else {
            return Err("user-tools manifest tool entry is not an object".into());
        };
        expect_exact_keys(
            tool,
            &["bins", "members", "name", "reference", "resolved", "tier"],
            "user-tools manifest tool",
        )?;
        let name = bounded_json_string(tool, "name")?;
        if name.is_empty() || !tool_names.insert(name.clone()) {
            return Err(format!("duplicate or empty user-tools tool `{name}`"));
        }
        let reference = bounded_json_string(tool, "reference")?;
        let resolved = bounded_json_string(tool, "resolved")?;
        if resolved.is_empty() {
            return Err(format!(
                "user-tools tool `{name}` has no resolved reference"
            ));
        }
        let tier = bounded_json_string(tool, "tier")?;
        if !matches!(tier.as_str(), "pinned" | "#latest" | "#auto") {
            return Err(format!(
                "user-tools tool `{name}` has unknown tier `{tier}`"
            ));
        }
        let tool_bins = json_bounded_string_array(tool, "bins", 255)?;
        let members = json_bounded_string_array(tool, "members", 255)?;
        if !tool_bins.is_empty() && tool_bins.len() != members.len() {
            return Err(format!(
                "user-tools tool `{name}` has mismatched bin/member pairs"
            ));
        }
        for bin in &tool_bins {
            validate_bin_name(bin).map_err(|error| format!("user-tools tool `{name}`: {error}"))?;
            if !bins.insert(bin.clone()) {
                return Err(format!("duplicate user-tools bin `{bin}`"));
            }
        }
        for member in &members {
            validate_bin_name(member)
                .map_err(|error| format!("user-tools tool `{name}`: {error}"))?;
        }
        tools.push(ManifestTool {
            name,
            reference,
            resolved,
            tier,
            bins: tool_bins,
            members,
        });
    }
    tools.sort_by(|left, right| (&left.name, &left.resolved).cmp(&(&right.name, &right.resolved)));
    Ok(ToolManifest { sources, tools })
}

fn manifest_provider(name: &str) -> Result<ProviderKind, String> {
    let provider = ProviderKind::parse(name);
    if provider.label() == name {
        Ok(provider)
    } else {
        Err(format!("unknown user-tools provider `{name}`"))
    }
}

fn format_tool_manifest(manifest: &ToolManifest) -> String {
    let mut sources = manifest.sources.clone();
    sources.sort_by(|left, right| left.name.cmp(&right.name));
    let mut tools = manifest.tools.clone();
    tools.sort_by(|left, right| (&left.name, &left.resolved).cmp(&(&right.name, &right.resolved)));
    let source_text = sources
        .iter()
        .map(|source| {
            format!(
                "    {{\"name\":{},\"policy\":{},\"provider\":{},\"raw\":{},\"upstream\":{}}}",
                JSON::quote(&source.name),
                JSON::quote(tool_policy_marker(source.policy)),
                JSON::quote(source.provider.label()),
                JSON::quote(&source.raw),
                JSON::quote(&source.upstream),
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    let tool_text = tools
        .iter()
        .map(|tool| {
            format!(
                "    {{\"bins\":{},\"members\":{},\"name\":{},\"reference\":{},\"resolved\":{},\"tier\":{}}}",
                json_string_array_text(&tool.bins),
                json_string_array_text(&tool.members),
                JSON::quote(&tool.name),
                JSON::quote(&tool.reference),
                JSON::quote(&tool.resolved),
                JSON::quote(&tool.tier),
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    format!(
        "{{\n  \"profile\":{},\n  \"schema\":{},\n  \"sources\": [{}],\n  \"tools\": [{}]\n}}\n",
        JSON::quote(Syntax::TOOL_PROFILE_NAME),
        JSON::quote(TOOL_MANIFEST_SCHEMA),
        if source_text.is_empty() {
            String::new()
        } else {
            format!("\n{source_text}\n  ")
        },
        if tool_text.is_empty() {
            String::new()
        } else {
            format!("\n{tool_text}\n  ")
        },
    )
}

fn json_string_array_text(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| JSON::quote(value))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn build_tool_manifest_generation(theme: &Theme, parsed: &Parsed) -> io::Result<u64> {
    RuntimePolicy::with_lock(&tools_state_dir(), PROFILE_LOCK_SCOPE, || {
        recover_profile_state()?;
        let mut manifest = read_tool_manifest()?;
        let sources_changed = refresh_tool_manifest_sources(theme, parsed, &mut manifest, false)?;
        let table = source_table_from_manifest(&manifest);
        let roots = Store::resolve();
        let name_w = manifest
            .tools
            .iter()
            .map(|tool| tool.resolved.len())
            .max()
            .unwrap_or(1);
        let mut tools = Vec::with_capacity(manifest.tools.len());
        let mut leases = Vec::with_capacity(manifest.tools.len());
        for entry in &manifest.tools {
            let (_policy, raw) = RefSpec::split_channel_policy(&entry.resolved);
            if let Some(code) = reject_unavailable_provider(theme, raw) {
                return Err(io::Error::other(format!(
                    "tool manifest entry `{}` rejected with exit code {code}",
                    entry.name
                )));
            }
            let spec = RefSpec::classify_in(raw, &table)
                .map_err(|error| io::Error::other(format!("{}: {error:?}", entry.name)))?;
            let outcome = realize_ref_outcome(
                theme,
                &roots,
                &parsed.flags,
                &table,
                &spec,
                name_w,
                RowStyle::Ledger,
                None,
                RealizeScope::UserProfile,
            );
            let (realized, lease) = match outcome {
                RefOutcome::Realized(entry, _state, _line, lease) => (entry, lease),
                RefOutcome::NeedsNix(need) => {
                    report_nix_bridge_required(theme, &parsed.flags, &[need], &[]);
                    return Err(io::Error::other(
                        "user-tools entry needs a Nix compatibility output",
                    ));
                }
                RefOutcome::Unavailable | RefOutcome::Failed => {
                    return Err(io::Error::other(format!(
                        "could not realize user-tools entry `{}`",
                        entry.name
                    )));
                }
            };
            lease.validate()?;
            let receipt = lease.profile_install_receipt()?;
            if receipt.reference != spec.raw || receipt.output_hash.is_empty() {
                return Err(io::Error::other(format!(
                    "verified user-tools receipt disagrees with `{}`",
                    entry.resolved
                )));
            }
            let bins = collect_manifest_bins(&receipt, entry)?;
            tools.push(InstalledTool {
                name: entry.name.clone(),
                version: receipt.version.clone(),
                source: spec.source.label().to_string(),
                reference: spec.raw.clone(),
                bins: bins.iter().map(|(name, _)| name.clone()).collect(),
                members: bins.iter().map(|(_, member)| member.clone()).collect(),
                member_digests: Vec::new(),
                output_hash: receipt.output_hash.clone(),
                store_root: receipt.store_root.to_string_lossy().into_owned(),
            });
            leases.push(lease);
            let _ = realized;
        }
        tools.sort_by(|left, right| {
            (&left.name, &left.reference).cmp(&(&right.name, &right.reference))
        });
        let generation = write_generation_locked(&tools, None)?;
        if sources_changed {
            write_tool_manifest(&manifest)?;
        }
        drop(leases);
        Ok(generation)
    })
}

fn collect_manifest_bins(
    receipt: &Store::ProfileInstallReceipt,
    entry: &ManifestTool,
) -> Result<Vec<(String, String)>, io::Error> {
    if entry.bins.is_empty() {
        return collect_receipt_bins(receipt, None).map_err(io::Error::other);
    }
    let members = if entry.members.is_empty() {
        entry.bins.clone()
    } else {
        entry.members.clone()
    };
    if members.len() != entry.bins.len() {
        return Err(io::Error::other(format!(
            "user-tools entry `{}` has mismatched bin/member pairs",
            entry.name
        )));
    }
    let mut seen = BTreeSet::new();
    let mut bins = Vec::with_capacity(entry.bins.len());
    for (bin, member) in entry.bins.iter().zip(members) {
        validate_bin_name(bin).map_err(io::Error::other)?;
        validate_bin_name(&member).map_err(io::Error::other)?;
        if !seen.insert(bin.clone()) {
            return Err(io::Error::other(format!(
                "duplicate user-tools bin `{bin}`"
            )));
        }
        if !receipt
            .executable_members
            .iter()
            .any(|available| available == &member)
        {
            return Err(io::Error::other(format!(
                "user-tools entry `{}` names missing executable member `{member}`",
                entry.name
            )));
        }
        bins.push((bin.clone(), member));
    }
    Ok(bins)
}

fn refresh_tool_manifest_sources(
    theme: &Theme,
    parsed: &Parsed,
    manifest: &mut ToolManifest,
    manual: bool,
) -> io::Result<bool> {
    let mut table = source_table_from_manifest(manifest);
    let sources = channel_sources(&table);
    let mut changed = false;
    for source in sources {
        let should_refresh = source.policy == ChannelPolicy::Automatic
            || (manual && source.policy == ChannelPolicy::Manual);
        if !should_refresh || (source.policy == ChannelPolicy::Automatic && parsed.flags.offline) {
            continue;
        }
        match resolve_source_channel(&source, &parsed.flags) {
            Ok(exact) => {
                if table.upstream(&source.name) != Some(exact.as_str()) {
                    table.set_upstream(&source.name, exact);
                    changed = true;
                }
            }
            Err(error) => {
                let pinned = table.upstream(&source.name).is_some_and(|upstream| {
                    RefSpec::split_channel_ref(upstream).1.is_none()
                        && upstream.contains(Syntax::REF_CHANNEL_MARKER)
                });
                if pinned && source.policy == ChannelPolicy::Automatic {
                    theme.detail(&format!(
                        "automatic source `{}` kept its last resolved pin",
                        source.name
                    ));
                    continue;
                }
                return Err(io::Error::other(format!("{error:?}")));
            }
        }
    }
    if changed {
        refresh_manifest_tool_pins(manifest, &table);
        manifest.sources = manifest_sources_from_table(&table);
    }
    Ok(changed)
}

/// A moving source keeps its visible tier marker, but the realization input
/// must become an exact package ref when the source channel advances. This
/// gives Store a new immutable reference instead of treating a changed
/// channel as a corrupted prior output.
fn refresh_manifest_tool_pins(manifest: &mut ToolManifest, table: &RefSpec::SourceTable) {
    for (name, upstream, _) in table.declarations() {
        let Some((_, selector)) = upstream.rsplit_once(Syntax::REF_CHANNEL_MARKER) else {
            continue;
        };
        if selector.is_empty()
            || matches!(selector, "latest" | "main")
            || (selector.starts_with('v') && selector.ends_with(".x"))
        {
            continue;
        }
        for tool in &mut manifest.tools {
            let (_policy, resolved) = RefSpec::split_channel_policy(&tool.resolved);
            let Some((package, source)) = resolved.rsplit_once(Syntax::REF_PROVIDER_AT) else {
                continue;
            };
            if source != name {
                continue;
            }
            let package = package
                .split_once(Syntax::REF_CHANNEL_MARKER)
                .map(|(package, _)| package)
                .unwrap_or(package);
            tool.resolved = format!(
                "{package}{}{selector}{}{name}",
                Syntax::REF_CHANNEL_MARKER,
                Syntax::REF_PROVIDER_AT
            );
        }
    }
}

fn activate_generation_locked(generation: u64) -> io::Result<()> {
    let record = read_generation_record(generation).map_err(io::Error::other)?;
    validate_generation_bins(generation, &record.tools)?;
    if !record.tools.is_empty() {
        ensure_generation_roots(generation, &record.tools, &record.witness)?;
    }
    publish_generation(generation, &record.witness, &record.tools)
}

fn current_generation_locked() -> io::Result<Option<u64>> {
    let path = current_path();
    if !path.is_file() {
        return Ok(None);
    }
    let pointer =
        ProfileDispatch::parse_current_pointer(&read_bounded(&path)?).map_err(io::Error::other)?;
    Ok((pointer.generation > 0).then_some(pointer.generation))
}

fn generation_numbers() -> io::Result<Vec<u64>> {
    if !generations_dir().is_dir() {
        return Ok(Vec::new());
    }
    let mut generations = Vec::new();
    for entry in fs::read_dir(generations_dir())? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let generation = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u64>().ok())
            .filter(|generation| *generation > 0)
            .ok_or_else(|| io::Error::other("invalid tool generation name"))?;
        generations.push(generation);
    }
    generations.sort_unstable();
    Ok(generations)
}

fn generation_disk_bytes(generation: u64) -> io::Result<u64> {
    disk_file_bytes(&generations_dir().join(generation.to_string()))
}

fn profile_state_disk_bytes() -> io::Result<u64> {
    disk_file_bytes_except(&tools_state_dir(), Some("generations"))
}

fn profile_bin_disk_bytes() -> io::Result<u64> {
    disk_file_bytes(&user_bin_dir())
}

fn disk_file_bytes(path: &Path) -> io::Result<u64> {
    disk_file_bytes_except(path, None)
}

fn disk_file_bytes_except(path: &Path, excluded: Option<&str>) -> io::Result<u64> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() {
        return Ok(0);
    }
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    if !metadata.is_dir() {
        return Ok(0);
    }
    let mut total = 0u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if excluded == Some(entry.file_name().to_string_lossy().as_ref()) {
            continue;
        }
        total = total.saturating_add(disk_file_bytes(&entry.path())?);
    }
    Ok(total)
}

const PROFILE_OWNER: &str = "user";
const PROFILE_LOCK_SCOPE: &str = "profile-user-tools";
const PROFILE_COMPLETE_FILE: &str = "complete";
const PROFILE_POINTER_PARTIAL: &str = "profile.json.partial";
const PROFILE_CURRENT_FILE: &str = "current";
const PROFILE_CURRENT_PARTIAL: &str = "current.partial";
const TOOL_MANIFEST_FILE: &str = "manifest.json";
const TOOL_MANIFEST_PARTIAL: &str = "manifest.json.partial";
const TOOL_MANIFEST_SCHEMA: &str = "jet-user-tools-v1";
const MAX_PROFILE_TOOLS: usize = 256;
const MAX_PROFILE_STRING: usize = 4096;
const PROFILE_FAILPOINT_ENV: &str = "JETPACK_INTERNAL_TEST_PROFILE_FAILPOINT";

fn user_jet_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(Syntax::CONFIG_DEFAULT_DIR)
}

fn user_bin_dir() -> PathBuf {
    user_jet_dir().join(Syntax::TOOL_BIN_DIR)
}

fn tools_state_dir() -> PathBuf {
    user_jet_dir().join(Syntax::TOOL_STATE_DIR)
}

fn generations_dir() -> PathBuf {
    tools_state_dir().join("generations")
}

fn profile_path() -> PathBuf {
    tools_state_dir().join("profile.json")
}

fn current_path() -> PathBuf {
    tools_state_dir().join(PROFILE_CURRENT_FILE)
}

fn tool_manifest_path() -> PathBuf {
    tools_state_dir().join(TOOL_MANIFEST_FILE)
}

pub(super) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn read_current_tools() -> Result<Vec<InstalledTool>, String> {
    let path = current_path();
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let pointer = ProfileDispatch::parse_current_pointer(
        &read_bounded(&path).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let (gen, pointer_witness) = (pointer.generation, pointer.witness);
    if gen == 0 {
        return Ok(Vec::new());
    }
    let record = read_generation_record(gen)?;
    if record.witness != pointer_witness {
        return Err("current pointer witness disagrees with generation".into());
    }
    Ok(record.tools)
}

fn read_generation_tools(gen: u64) -> Result<Vec<InstalledTool>, String> {
    read_generation_record(gen).map(|record| record.tools)
}

struct GenerationRecord {
    tools: Vec<InstalledTool>,
    witness: String,
}

fn read_generation_record(gen: u64) -> Result<GenerationRecord, String> {
    let path = generations_dir().join(gen.to_string()).join("meta.json");
    if !path.is_file() {
        return Err(format!("tool generation {gen} has no metadata"));
    }
    let text = read_bounded(&path).map_err(|error| error.to_string())?;
    let tools = parse_generation_meta(&text, gen)?;
    let witness = generation_record_witness(&text, gen)?;
    let complete = generations_dir()
        .join(gen.to_string())
        .join(PROFILE_COMPLETE_FILE);
    let marker = read_bounded(&complete).map_err(|error| error.to_string())?;
    if marker != format!("{witness}\n") {
        return Err(format!("tool generation {gen} witness mismatch"));
    }
    Ok(GenerationRecord { tools, witness })
}

fn project_install(
    _theme: &Theme,
    _roots: &Store::Roots,
    spec: &RefSpec::RefSpec,
    receipt: &Store::ProfileInstallReceipt,
    lease: &Store::CacheLease,
    bins: &[(String, String)],
    channel_policy: ChannelPolicy,
) -> Result<(u64, String), String> {
    if receipt.reference != spec.raw
        || receipt.package != spec.short_name()
        || receipt.output_hash.is_empty()
    {
        return Err("verified tool receipt disagrees with requested package".to_string());
    }
    let version = receipt.version.clone();
    let declared_sources = tool_manifest_source_table(spec, &version, channel_policy);
    RuntimePolicy::with_lock(&tools_state_dir(), PROFILE_LOCK_SCOPE, || {
        recover_profile_state()?;
        let mut tools = read_current_tools().map_err(io::Error::other)?;
        let short = spec.short_name().to_string();
        tools.retain(|tool| {
            tool.name != short
                && tool
                    .bins
                    .iter()
                    .all(|old| bins.iter().all(|(new, _)| new != old))
        });
        tools.push(InstalledTool {
            name: short,
            version: version.clone(),
            source: spec.source.label().to_string(),
            reference: spec.raw.clone(),
            bins: bins.iter().map(|(name, _)| name.clone()).collect(),
            members: bins.iter().map(|(_, member)| member.clone()).collect(),
            member_digests: Vec::new(),
            output_hash: receipt.output_hash.clone(),
            store_root: receipt.store_root.to_string_lossy().into_owned(),
        });
        tools.sort_by(|left, right| {
            (&left.name, &left.reference).cmp(&(&right.name, &right.reference))
        });
        lease.validate()?;
        let generation = write_generation_locked(&tools, Some((receipt, lease)))?;
        record_tool_manifest_locked(&tools, &declared_sources, Some((&spec.raw, channel_policy)))?;
        Ok(generation)
    })
    .map(|generation| (generation, version))
    .map_err(|error| error.to_string())
}

fn tool_manifest_source_table(
    spec: &RefSpec::RefSpec,
    version: &str,
    policy: ChannelPolicy,
) -> RefSpec::SourceTable {
    let mut table = cwd_table();
    if spec.source.label() == crate::Provider::native::SOURCE_NAME && !version.is_empty() {
        let upstream = format!(
            "{}#v{}",
            crate::Provider::native::UPSTREAM,
            version.trim_start_matches('v')
        );
        table.set_upstream(crate::Provider::native::SOURCE_NAME, upstream.clone());
        table.set_channel_metadata(
            crate::Provider::native::SOURCE_NAME,
            policy,
            source_manifest_ref(&upstream, policy),
        );
    }
    table
}

fn uninstall_tool(theme: &Theme, name: &str) -> Result<bool, String> {
    let _ = theme;
    RuntimePolicy::with_lock(&tools_state_dir(), PROFILE_LOCK_SCOPE, || {
        recover_profile_state()?;
        let mut tools = read_current_tools().map_err(io::Error::other)?;
        let before = tools.len();
        tools.retain(|tool| tool.name != name && tool.bins.iter().all(|bin| bin != name));
        if tools.len() == before {
            return Ok(false);
        }
        write_generation_locked(&tools, None)?;
        record_tool_manifest_locked(&tools, &RefSpec::SourceTable::empty(), None)?;
        Ok(true)
    })
    .map_err(|error| error.to_string())
}

fn write_generation_locked(
    tools: &[InstalledTool],
    live: Option<(&Store::ProfileInstallReceipt, &Store::CacheLease)>,
) -> io::Result<u64> {
    let gen = next_generation()?;
    let gen_dir = generations_dir().join(gen.to_string());
    fs::create_dir(&gen_dir)?;
    let mut generation_tools = tools.to_vec();
    materialize_generation_bins(&gen_dir, &mut generation_tools, live)?;
    let meta = format_generation_meta(gen, &generation_tools)?;
    write_synced(&gen_dir.join("meta.json"), meta.as_bytes())?;
    validate_generation_bins(gen, &generation_tools)?;
    let witness = generation_record_witness(&meta, gen).map_err(io::Error::other)?;
    write_synced(
        &gen_dir.join(PROFILE_COMPLETE_FILE),
        format!("{witness}\n").as_bytes(),
    )?;
    Store::sync_store_directory(&gen_dir)?;
    Store::sync_store_directory(&generations_dir())?;
    profile_failpoint("after-generation")?;

    if !generation_tools.is_empty() {
        ensure_generation_roots(gen, &generation_tools, &witness)?;
        profile_failpoint("after-root-commit")?;
    }
    profile_failpoint("before-pointer")?;
    publish_generation(gen, &witness, &generation_tools)?;
    profile_failpoint("after-pointer")?;
    Ok(gen)
}

fn next_generation() -> io::Result<u64> {
    fs::create_dir_all(generations_dir())?;
    let mut maximum = 0u64;
    for entry in fs::read_dir(generations_dir())? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| io::Error::other("tool generation name is not UTF-8"))?;
        let generation = name
            .parse::<u64>()
            .map_err(|_| io::Error::other(format!("invalid tool generation `{name}`")))?;
        if generation == 0 || !entry.file_type()?.is_dir() {
            return Err(io::Error::other(format!(
                "invalid tool generation `{name}`"
            )));
        }
        maximum = maximum.max(generation);
    }
    maximum
        .checked_add(1)
        .ok_or_else(|| io::Error::other("tool generation number overflow"))
}

fn recover_profile_state() -> io::Result<()> {
    fs::create_dir_all(tools_state_dir())?;
    for partial in [
        PROFILE_POINTER_PARTIAL,
        PROFILE_CURRENT_PARTIAL,
        TOOL_MANIFEST_PARTIAL,
    ] {
        let partial = tools_state_dir().join(partial);
        match fs::symlink_metadata(&partial) {
            Ok(metadata) if metadata.file_type().is_file() => fs::remove_file(&partial)?,
            Ok(_) => {
                return Err(io::Error::other(
                    "tool generation pointer partial is not a regular file",
                ))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }

    migrate_legacy_generations_locked()?;

    let requested_current = current_generation_locked().ok().flatten();
    let mut publish = 0;
    if generations_dir().is_dir() {
        let mut generations = fs::read_dir(generations_dir())?
            .map(|entry| {
                let entry = entry?;
                entry
                    .file_name()
                    .to_string_lossy()
                    .parse::<u64>()
                    .map_err(|_| io::Error::other("invalid tool generation name"))
            })
            .collect::<io::Result<Vec<_>>>()?;
        generations.sort();
        for generation in generations {
            let complete = generations_dir()
                .join(generation.to_string())
                .join(PROFILE_COMPLETE_FILE);
            if !complete.is_file() {
                continue;
            }
            let tools = read_generation_tools(generation).map_err(io::Error::other)?;
            let metadata = read_bounded(
                &generations_dir()
                    .join(generation.to_string())
                    .join("meta.json"),
            )?;
            let witness =
                generation_record_witness(&metadata, generation).map_err(io::Error::other)?;
            if read_bounded(&complete)?.trim() != witness {
                return Err(io::Error::other("tool generation witness mismatch"));
            }
            validate_generation_bins(generation, &tools)?;
            if !tools.is_empty() {
                ensure_generation_roots(generation, &tools, &witness)?;
            }
            publish = publish.max(generation);
        }
    }
    if publish != 0 {
        let selected = requested_current
            .filter(|generation| {
                generation_numbers()
                    .unwrap_or_default()
                    .contains(generation)
            })
            .unwrap_or(publish);
        let tools = read_generation_tools(selected).map_err(io::Error::other)?;
        let metadata = read_bounded(
            &generations_dir()
                .join(selected.to_string())
                .join("meta.json"),
        )?;
        publish_generation(
            selected,
            &generation_record_witness(&metadata, selected).map_err(io::Error::other)?,
            &tools,
        )?;
    }
    Ok(())
}

fn migrate_legacy_generations_locked() -> io::Result<()> {
    if !generations_dir().is_dir() {
        return Ok(());
    }
    let mut legacy = Vec::new();
    for entry in fs::read_dir(generations_dir())? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            return Err(io::Error::other("invalid tool generation entry"));
        }
        let generation = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u64>().ok())
            .filter(|generation| *generation != 0)
            .ok_or_else(|| io::Error::other("invalid tool generation name"))?;
        if !entry.path().join(PROFILE_COMPLETE_FILE).is_file()
            || !entry.path().join("meta.json").is_file()
        {
            continue;
        }
        let metadata = read_bounded(&entry.path().join("meta.json"))?;
        let parsed = JSON::parse(&metadata).map_err(io::Error::other)?;
        let JSON::JSONValue::Object(root) = parsed else {
            return Err(io::Error::other("tool metadata root is not an object"));
        };
        if !root.contains_key("schema") {
            legacy.push((generation, entry.path(), metadata));
        }
    }
    legacy.sort_by_key(|(generation, _, _)| *generation);
    for (generation, path, metadata) in legacy {
        let tools =
            parse_legacy_generation_meta(&metadata, generation).map_err(io::Error::other)?;
        let digests = tools
            .iter()
            .map(|tool| tool.output_hash.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let witness = profile_generation_witness(&metadata, &digests);
        let marker = read_bounded(&path.join(PROFILE_COMPLETE_FILE))?;
        if marker != "complete\n" && marker != format!("{witness}\n") {
            return Err(io::Error::other(format!(
                "legacy tool generation {generation} witness mismatch; remove only after auditing `{}`",
                path.display()
            )));
        }
        if !canonical_generation_matches(&tools)? {
            write_generation_locked(&tools, None)?;
        }
        let archive = tools_state_dir().join("legacy-generations");
        fs::create_dir_all(&archive)?;
        let destination = archive.join(format!("generation-{generation}"));
        if destination.exists() {
            return Err(io::Error::other(
                "legacy profile migration archive collision",
            ));
        }
        fs::rename(&path, &destination)?;
        Store::sync_store_directory(&archive)?;
        Store::sync_store_directory(&generations_dir())?;
    }
    Ok(())
}

fn canonical_generation_matches(legacy: &[InstalledTool]) -> io::Result<bool> {
    for entry in fs::read_dir(generations_dir())? {
        let entry = entry?;
        let Some(generation) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u64>().ok())
        else {
            continue;
        };
        let Ok(record) = read_generation_record(generation) else {
            continue;
        };
        if record.tools.len() == legacy.len()
            && record
                .tools
                .iter()
                .zip(legacy)
                .all(|(left, right)| tool_authority_equal(left, right))
        {
            validate_generation_bins(generation, &record.tools)?;
            return Ok(true);
        }
    }
    Ok(false)
}

fn tool_authority_equal(left: &InstalledTool, right: &InstalledTool) -> bool {
    left.name == right.name
        && left.version == right.version
        && left.source == right.source
        && left.reference == right.reference
        && left.bins == right.bins
        && left.members == right.members
        && left.output_hash == right.output_hash
        && left.store_root == right.store_root
}

fn ensure_generation_roots(
    generation: u64,
    tools: &[InstalledTool],
    witness: &str,
) -> io::Result<()> {
    let mut authorities = std::collections::BTreeMap::<String, Vec<String>>::new();
    for tool in tools {
        validate_digest(&tool.output_hash)?;
        authorities
            .entry(tool.store_root.clone())
            .or_default()
            .push(tool.output_hash.clone());
    }
    let mut prepared = Vec::new();
    for (root, targets) in authorities {
        let roots = Store::Roots {
            root: PathBuf::from(root),
            dev_mode: true,
        };
        if let Some(receipt) = Store::reconcile_profile_generation_root(
            &roots,
            PROFILE_OWNER,
            Syntax::TOOL_PROFILE_NAME,
            generation,
            witness,
            targets,
            now_secs(),
        )? {
            prepared.push((roots, receipt));
        }
    }
    profile_failpoint("after-root-prepare")?;
    let commit_count = prepared.len();
    for (index, (roots, receipt)) in prepared.into_iter().enumerate() {
        Store::commit_profile_generation_root(&roots, &receipt, now_secs())?;
        if index + 1 != commit_count {
            profile_failpoint("between-root-commits")?;
        }
    }
    Ok(())
}

fn materialize_generation_bins(
    generation_dir: &Path,
    tools: &mut [InstalledTool],
    live: Option<(&Store::ProfileInstallReceipt, &Store::CacheLease)>,
) -> io::Result<()> {
    let bin_dir = generation_dir.join("bin");
    fs::create_dir(&bin_dir)?;
    let mut projected = std::collections::BTreeSet::new();
    for tool in tools {
        if tool.bins.len() != tool.members.len() || tool.bins.is_empty() {
            return Err(io::Error::other(
                "profile tool has mismatched bin/member pairs",
            ));
        }
        let mut member_digests = Vec::with_capacity(tool.bins.len());
        for (bin, member) in tool.bins.iter().zip(&tool.members) {
            validate_bin_name(bin).map_err(io::Error::other)?;
            validate_bin_name(member).map_err(io::Error::other)?;
            let physical = ProfileDispatch::physical_bin_name(bin);
            if !projected.insert(physical.to_ascii_lowercase()) {
                return Err(io::Error::other(format!("duplicate profile bin `{bin}`")));
            }
            let destination = bin_dir.join(physical);
            let proof = if let Some((_receipt, lease)) = live.filter(|(receipt, _)| {
                receipt.reference == tool.reference
                    && receipt.output_hash == tool.output_hash
                    && receipt.store_root.to_string_lossy() == tool.store_root
            }) {
                lease.copy_profile_executable(member, &destination)?
            } else {
                let roots = Store::Roots {
                    root: PathBuf::from(&tool.store_root),
                    dev_mode: true,
                };
                Store::copy_profile_store_member(
                    &roots,
                    &tool.reference,
                    &tool.output_hash,
                    member,
                    &destination,
                )?
            };
            #[cfg(unix)]
            if proof.mode & 0o111 == 0 {
                return Err(io::Error::other("profile projection is not executable"));
            }
            member_digests.push(proof.digest);
        }
        tool.member_digests = member_digests;
    }
    Store::sync_store_directory(&bin_dir)
}

fn validated_store_member(tool: &InstalledTool, member: &str) -> io::Result<PathBuf> {
    let roots = Store::Roots {
        root: PathBuf::from(&tool.store_root),
        dev_mode: true,
    };
    let entry = Store::list_checked(&roots)?
        .into_iter()
        .find(|entry| {
            entry.reference == tool.reference && entry.envelope.output_hash == tool.output_hash
        })
        .ok_or_else(|| io::Error::other("profile StoreEntry authority is unavailable"))?;
    let source = Path::new(&entry.bin).join(member);
    let metadata = fs::symlink_metadata(&source)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(io::Error::other(
            "profile executable member is not a no-follow file",
        ));
    }
    Ok(source)
}

fn validate_generation_bins(generation: u64, tools: &[InstalledTool]) -> io::Result<()> {
    let bin_dir = generations_dir().join(generation.to_string()).join("bin");
    let expected = tools
        .iter()
        .flat_map(|tool| {
            tool.bins
                .iter()
                .map(|bin| ProfileDispatch::physical_bin_name(bin))
        })
        .collect::<std::collections::BTreeSet<_>>();
    let actual = fs::read_dir(&bin_dir)?
        .map(|entry| {
            entry?
                .file_name()
                .into_string()
                .map_err(|_| io::Error::other("profile bin is not UTF-8"))
        })
        .collect::<io::Result<std::collections::BTreeSet<_>>>()?;
    if actual != expected {
        return Err(io::Error::other("tool generation bin projection mismatch"));
    }
    for tool in tools {
        if tool.bins.len() != tool.member_digests.len() {
            return Err(io::Error::other("profile projection proof count mismatch"));
        }
        for member in &tool.members {
            let _ = validated_store_member(tool, member)?;
        }
        for (bin, digest) in tool.bins.iter().zip(&tool.member_digests) {
            let proof =
                Store::profile_file_proof(&bin_dir.join(ProfileDispatch::physical_bin_name(bin)))?;
            if &proof.digest != digest {
                return Err(io::Error::other(format!(
                    "profile projection proof mismatch for `{bin}`"
                )));
            }
            #[cfg(unix)]
            if proof.mode & 0o111 == 0 {
                return Err(io::Error::other(format!(
                    "profile projection is not executable for `{bin}`"
                )));
            }
        }
    }
    Ok(())
}

fn publish_generation(generation: u64, witness: &str, tools: &[InstalledTool]) -> io::Result<()> {
    fs::create_dir_all(user_jet_dir())?;
    fs::create_dir_all(user_bin_dir())?;
    if fs::symlink_metadata(user_bin_dir())?
        .file_type()
        .is_symlink()
    {
        return Err(io::Error::other(
            "legacy profile bin symlink requires explicit migration",
        ));
    }
    for bin in tools.iter().flat_map(|tool| &tool.bins) {
        ensure_dispatcher(bin)?;
    }
    Store::sync_store_directory(&user_bin_dir())?;

    let pointer = ProfileDispatch::format_current_pointer(&ProfileDispatch::CurrentPointer {
        generation,
        witness: witness.to_string(),
    })?;
    atomic_write_current_pointer(pointer.as_bytes())?;
    profile_failpoint("after-current-pointer")?;
    let profile = format!(
        "{{\n  \"name\": \"{}\",\n  \"current\": {},\n  \"witness\": {}\n}}\n",
        Syntax::TOOL_PROFILE_NAME,
        generation,
        json_str(witness),
    );
    atomic_write_profile_pointer(profile.as_bytes())
}

fn ensure_dispatcher(bin: &str) -> io::Result<()> {
    ProfileDispatch::install_dispatcher(&user_bin_dir(), bin).map(|_| ())
}

fn validate_bin_name(value: &str) -> Result<(), String> {
    ProfileDispatch::validate_bin_name(value).map_err(|error| error.to_string())
}

fn validate_digest(value: &str) -> io::Result<()> {
    let Some(hex) = value.strip_prefix("sha256-") else {
        return Err(io::Error::other("profile digest is not sha256"));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(io::Error::other("profile digest is not canonical"));
    }
    Ok(())
}

pub(super) fn read_bounded(path: &Path) -> io::Result<String> {
    const MAX_PROFILE_METADATA: u64 = 1024 * 1024;
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > MAX_PROFILE_METADATA {
        return Err(io::Error::other("profile metadata exceeds byte bound"));
    }
    let mut bytes = Vec::new();
    std::io::Read::take(file, MAX_PROFILE_METADATA + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PROFILE_METADATA {
        return Err(io::Error::other("profile metadata exceeds byte bound"));
    }
    String::from_utf8(bytes).map_err(|_| io::Error::other("profile metadata is not UTF-8"))
}

pub(super) fn write_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

pub(super) fn profile_generation_witness(
    metadata: &str,
    digests: &std::collections::BTreeSet<String>,
) -> String {
    let mut canonical = format!(
        "jet-profile-generation-witness-v1\nmetadata\t{}\n",
        SHA256::sha256_hex(metadata.as_bytes())
    );
    for digest in digests {
        canonical.push_str("target\t");
        canonical.push_str(digest);
        canonical.push('\n');
    }
    format!("sha256-{}", SHA256::sha256_hex(canonical.as_bytes()))
}

fn generation_record_witness(metadata: &str, generation: u64) -> Result<String, String> {
    let parsed = ProfileDispatch::parse_generation_metadata(metadata, generation)
        .map_err(|error| error.to_string())?;
    Ok(ProfileDispatch::generation_witness(metadata, &parsed))
}

fn profile_failpoint(phase: &str) -> io::Result<()> {
    if std::env::var(PROFILE_FAILPOINT_ENV).ok().as_deref() == Some(phase) {
        return Err(io::Error::other(format!(
            "profile publication failpoint `{phase}`"
        )));
    }
    Ok(())
}

fn atomic_write_profile_pointer(bytes: &[u8]) -> io::Result<()> {
    let directory = tools_state_dir();
    let partial = directory.join(PROFILE_POINTER_PARTIAL);
    write_synced(&partial, bytes)?;
    finalize_profile_pointer(&partial, &profile_path())?;
    Store::sync_store_directory(&directory)
}

fn atomic_write_current_pointer(bytes: &[u8]) -> io::Result<()> {
    let directory = tools_state_dir();
    let partial = directory.join(PROFILE_CURRENT_PARTIAL);
    write_synced(&partial, bytes)?;
    finalize_profile_pointer(&partial, &current_path())?;
    Store::sync_store_directory(&directory)
}

#[cfg(windows)]
pub(super) fn finalize_profile_pointer(partial: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, replacement: *const u16, flags: u32) -> i32;
    }
    let mut existing = partial.as_os_str().encode_wide().collect::<Vec<_>>();
    existing.push(0);
    let mut replacement = destination.as_os_str().encode_wide().collect::<Vec<_>>();
    replacement.push(0);
    if unsafe {
        MoveFileExW(
            existing.as_ptr(),
            replacement.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(windows))]
pub(super) fn finalize_profile_pointer(partial: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(partial, destination)
}

fn format_generation_meta(gen: u64, tools: &[InstalledTool]) -> io::Result<String> {
    let metadata = ProfileDispatch::GenerationMetadata {
        generation: gen,
        created_at: now_secs(),
        tools: tools
            .iter()
            .map(|tool| ProfileDispatch::GenerationTool {
                name: tool.name.clone(),
                version: tool.version.clone(),
                source: tool.source.clone(),
                reference: tool.reference.clone(),
                output_hash: tool.output_hash.clone(),
                store_root: tool.store_root.clone(),
                bins: tool.bins.clone(),
                members: tool.members.clone(),
                projection_hashes: tool.member_digests.clone(),
            })
            .collect(),
    };
    ProfileDispatch::format_generation_metadata(&metadata)
}
fn json_str(s: &str) -> String {
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    )
}

fn parse_generation_meta(
    text: &str,
    expected_generation: u64,
) -> Result<Vec<InstalledTool>, String> {
    let metadata = ProfileDispatch::parse_generation_metadata(text, expected_generation)
        .map_err(|error| error.to_string())?;
    Ok(metadata
        .tools
        .into_iter()
        .map(|tool| InstalledTool {
            name: tool.name,
            version: tool.version,
            source: tool.source,
            reference: tool.reference,
            bins: tool.bins,
            members: tool.members,
            member_digests: tool.projection_hashes,
            output_hash: tool.output_hash,
            store_root: tool.store_root,
        })
        .collect())
}
fn parse_legacy_generation_meta(
    text: &str,
    expected_generation: u64,
) -> Result<Vec<InstalledTool>, String> {
    let JSON::JSONValue::Object(root) = JSON::parse(text)? else {
        return Err("legacy profile metadata root is not an object".into());
    };
    expect_exact_keys(
        &root,
        &["created_at", "generation", "profile", "tools"],
        "legacy profile metadata",
    )?;
    if json_field_string(&root, "profile")? != Syntax::TOOL_PROFILE_NAME
        || json_field_u64(&root, "generation")? != expected_generation
    {
        return Err("legacy profile metadata identity mismatch".into());
    }
    let _ = json_field_u64(&root, "created_at")?;
    let JSON::JSONValue::Array(entries) = root.get("tools").ok_or("legacy metadata lacks tools")?
    else {
        return Err("legacy profile tools field is not an array".into());
    };
    if entries.len() > MAX_PROFILE_TOOLS {
        return Err("legacy profile tool count exceeds bound".into());
    }
    let roots = Store::resolve();
    validate_store_root(&roots.root.to_string_lossy())?;
    let store = Store::list_checked(&roots).map_err(|error| error.to_string())?;
    let mut tools = Vec::with_capacity(entries.len());
    let mut seen_bins = std::collections::BTreeSet::new();
    for entry in entries {
        let JSON::JSONValue::Object(tool) = entry else {
            return Err("legacy profile tool entry is not an object".into());
        };
        expect_exact_keys(
            tool,
            &[
                "bins",
                "name",
                "output_hash",
                "reference",
                "source",
                "targets",
                "version",
            ],
            "legacy profile tool",
        )?;
        let name = bounded_json_string(tool, "name")?;
        let version = bounded_json_string(tool, "version")?;
        let source = bounded_json_string(tool, "source")?;
        let reference = bounded_json_string(tool, "reference")?;
        let output_hash = bounded_json_string(tool, "output_hash")?;
        validate_digest(&output_hash).map_err(|error| error.to_string())?;
        let bins = json_string_array(tool, "bins")?;
        let targets = json_bounded_string_array(tool, "targets", MAX_PROFILE_STRING)?;
        if bins.is_empty() || bins.len() != targets.len() {
            return Err("legacy profile tool has mismatched bin/target pairs".into());
        }
        let authority = store
            .iter()
            .find(|entry| entry.reference == reference && entry.envelope.output_hash == output_hash)
            .ok_or("legacy profile Store authority is unavailable")?;
        let mut members = Vec::with_capacity(targets.len());
        for (bin, target) in bins.iter().zip(&targets) {
            validate_bin_name(bin)?;
            if !seen_bins.insert(bin.clone()) {
                return Err(format!("duplicate legacy profile bin `{bin}`"));
            }
            let target = Path::new(target);
            let member = target
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("legacy profile target has no UTF-8 member")?;
            validate_bin_name(member)?;
            if target != Path::new(&authority.bin).join(member) {
                return Err("legacy profile target escapes verified Store authority".into());
            }
            members.push(member.to_string());
        }
        tools.push(InstalledTool {
            name,
            version,
            source,
            reference,
            bins,
            members,
            member_digests: Vec::new(),
            output_hash,
            store_root: roots.root.to_string_lossy().into_owned(),
        });
    }
    tools
        .sort_by(|left, right| (&left.name, &left.reference).cmp(&(&right.name, &right.reference)));
    Ok(tools)
}

fn expect_exact_keys(
    object: &std::collections::BTreeMap<String, JSON::JSONValue>,
    expected: &[&str],
    label: &str,
) -> Result<(), String> {
    let actual = object.keys().map(String::as_str).collect::<Vec<_>>();
    let mut expected = expected.to_vec();
    expected.sort_unstable();
    if actual != expected {
        return Err(format!("{label} has unknown or missing fields"));
    }
    Ok(())
}

fn json_field_string<'a>(
    object: &'a std::collections::BTreeMap<String, JSON::JSONValue>,
    key: &str,
) -> Result<&'a str, String> {
    object
        .get(key)
        .ok_or_else(|| format!("missing key `{key}`"))?
        .as_str()
}

fn bounded_json_string(
    object: &std::collections::BTreeMap<String, JSON::JSONValue>,
    key: &str,
) -> Result<String, String> {
    let value = json_field_string(object, key)?;
    if value.len() > MAX_PROFILE_STRING || value.bytes().any(|byte| byte == 0) {
        return Err(format!("profile field `{key}` exceeds bounds"));
    }
    Ok(value.to_string())
}

fn json_field_u64(
    object: &std::collections::BTreeMap<String, JSON::JSONValue>,
    key: &str,
) -> Result<u64, String> {
    match object.get(key) {
        Some(JSON::JSONValue::Number(value)) if *value >= 0 => Ok(*value as u64),
        Some(JSON::JSONValue::Flt(value))
            if value.is_finite()
                && *value >= 0.0
                && value.fract() == 0.0
                && *value <= 9_007_199_254_740_991.0 =>
        {
            Ok(*value as u64)
        }
        Some(_) => Err(format!("profile field `{key}` is not an exact integer")),
        None => Err(format!("profile field `{key}` is not a number")),
    }
}

fn json_string_array(
    object: &std::collections::BTreeMap<String, JSON::JSONValue>,
    key: &str,
) -> Result<Vec<String>, String> {
    json_bounded_string_array(object, key, 255)
}

fn json_bounded_string_array(
    object: &std::collections::BTreeMap<String, JSON::JSONValue>,
    key: &str,
    max_len: usize,
) -> Result<Vec<String>, String> {
    let Some(JSON::JSONValue::Array(values)) = object.get(key) else {
        return Err(format!("profile field `{key}` is not an array"));
    };
    values
        .iter()
        .map(|value| {
            let value = value.as_str()?;
            if value.len() > max_len || value.bytes().any(|byte| byte == 0) {
                return Err(format!("profile field `{key}` exceeds bounds"));
            }
            Ok(value.to_string())
        })
        .collect()
}

fn validate_store_root(value: &str) -> Result<(), String> {
    let path = Path::new(value);
    if !path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err("profile Store authority is not an absolute normalized path".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_names_parse_plain_and_every() {
        let src = r#"
#Job fn serve() { }
#Every(5min) #Job fn lint() { }
fn run() { }
"#;
        let names = task_names_in(src);
        assert!(names.contains(&"serve".into()), "{names:?}");
        assert!(names.contains(&"lint".into()), "{names:?}");
        assert!(!names.iter().any(|n| n == "run"));
    }

    #[test]
    fn external_provider_list_covers_ballot_examples() {
        assert!(Syntax::TOOL_EXTERNAL_PROVIDERS.contains(&"npm"));
        assert!(Syntax::TOOL_EXTERNAL_PROVIDERS.contains(&"cargo"));
        assert!(Syntax::TOOL_EXTERNAL_PROVIDERS.contains(&"pypi"));
    }

    #[test]
    fn profile_witness_binds_immutable_metadata_and_sorted_digests() {
        let left = std::collections::BTreeSet::from([
            format!("sha256-{}", "b".repeat(64)),
            format!("sha256-{}", "a".repeat(64)),
        ]);
        let right = left.iter().rev().cloned().collect();
        let witness = profile_generation_witness("immutable", &left);
        assert_eq!(witness, profile_generation_witness("immutable", &right));
        assert_ne!(witness, profile_generation_witness("changed", &left));
        assert!(witness.starts_with("sha256-"));
        assert_eq!(PROFILE_LOCK_SCOPE, "profile-user-tools");
    }

    #[test]
    fn generation_metadata_roundtrips_original_store_digest() {
        let digest = format!("sha256-{}", "d".repeat(64));
        let tools = vec![InstalledTool {
            name: "tool".into(),
            version: "1".into(),
            source: "path".into(),
            reference: "./tool".into(),
            bins: vec!["tool".into()],
            members: vec!["tool".into()],
            member_digests: vec![format!("sha256-{}", "e".repeat(64))],
            output_hash: digest.clone(),
            store_root: "/store".into(),
        }];
        let metadata = format_generation_meta(7, &tools).expect("format metadata");
        let parsed = parse_generation_meta(&metadata, 7).expect("parse");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].output_hash, digest);
        assert!(metadata.contains("\"generation\": 7"));
    }

    #[test]
    fn profile_metadata_rejects_unknown_fields_and_duplicate_bins() {
        let digest = format!("sha256-{}", "d".repeat(64));
        let projection = format!("sha256-{}", "e".repeat(64));
        let tool = InstalledTool {
            name: "left".into(),
            version: "1".into(),
            source: "path".into(),
            reference: "./left".into(),
            bins: vec!["same".into()],
            members: vec!["left".into()],
            member_digests: vec![projection.clone()],
            output_hash: digest.clone(),
            store_root: "/store".into(),
        };
        let metadata =
            format_generation_meta(1, std::slice::from_ref(&tool)).expect("format metadata");
        let corrupted = metadata.replacen("{\n", "{\n  \"unknown\": true,\n", 1);
        assert!(parse_generation_meta(&corrupted, 1).is_err());

        let mut right = tool.clone();
        right.name = "right".into();
        right.reference = "./right".into();
        right.members = vec!["right".into()];
        let duplicate = ProfileDispatch::GenerationMetadata {
            generation: 1,
            created_at: 1,
            tools: [tool, right]
                .into_iter()
                .map(|tool| ProfileDispatch::GenerationTool {
                    name: tool.name,
                    version: tool.version,
                    source: tool.source,
                    reference: tool.reference,
                    output_hash: tool.output_hash,
                    store_root: tool.store_root,
                    bins: tool.bins,
                    members: tool.members,
                    projection_hashes: tool.member_digests,
                })
                .collect(),
        };
        assert!(ProfileDispatch::format_generation_metadata(&duplicate).is_err());
    }

    #[test]
    fn current_pointer_checksum_rejects_bitflips_and_truncation() {
        let witness = format!("sha256-{}", "f".repeat(64));
        let record = ProfileDispatch::CurrentPointer {
            generation: 9,
            witness: witness.clone(),
        };
        let pointer = ProfileDispatch::format_current_pointer(&record).unwrap();
        assert_eq!(
            ProfileDispatch::parse_current_pointer(&pointer).unwrap(),
            record
        );
        assert!(ProfileDispatch::parse_current_pointer(
            &pointer.replace("generation\t9", "generation\t8")
        )
        .is_err());
        assert!(ProfileDispatch::parse_current_pointer(pointer.trim_end()).is_err());
    }
}
