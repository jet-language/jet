use super::package_hangar_vendor::auto_clean_after_success;
use super::parse::{Flags, Parsed};
use super::realize::{
    apply_locked_channels, classify_or_report, load_project_plan, realize_adapter, realize_ref,
    plan_downloads, realize_ref_outcome, report_nix_bridge_required, RealizeScope, RefOutcome,
    RowStyle, RunPlan,
};
use super::services_secrets_config::find_jet_binary;
use super::workspace_sources::{
    cwd_table, load_workspace_for_source, project_root, workspace_index_required_diagnostic,
    workspace_root_snapshot_or_exit,
};
use crate::EnvHook;
use crate::MemberSelect::{self, SelectRequest};
use crate::Output::{self, Theme};
use crate::Provider;
use crate::RefSpec::{self, ProviderKind};
use crate::RuntimePolicy;
use crate::Shell::Env;
use crate::Store::{self, Roots};
use crate::Syntax;
use crate::Trust;
use crate::WorkspaceFile::WorkspaceMember;
use jet_env_model::ModuleEval;
use jet_pkg_model::Authority::AuthorityResolver;
use jet_pkg_model::WorkspacePlan::{WorkspaceSource, WorkspaceSourceRole};
use std::io::IsTerminal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeDevTool {
    definition: &'static str,
    command: &'static str,
    relative_binary: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeActivation {
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NixShellScratch {
    NotCreated,
}

/// Typed host facts for the Jet-native shell projection. The final
/// environment-variable map is only a child-process adapter; paths, markers,
/// and the no-Nix cleanup state stay typed until that boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeEnvironmentProjection {
    project_root: std::path::PathBuf,
    native_bin_dirs: Vec<std::path::PathBuf>,
    tzdir: Option<std::path::PathBuf>,
    loader_paths: Vec<std::path::PathBuf>,
    activation: NativeActivation,
    nix_shell_scratch: NixShellScratch,
}

impl NativeEnvironmentProjection {
    fn from_realized(
        project_root: &std::path::Path,
        realized: &[(String, String)],
        inherited_loader_path: Option<&str>,
    ) -> Self {
        let native_bin_dirs = native_dev_tool_paths(project_root)
            .into_iter()
            .filter_map(|(_, binary)| binary.parent().map(|path| path.to_path_buf()))
            .fold(Vec::new(), |mut dirs, directory| {
                if !dirs.iter().any(|existing| existing == &directory) {
                    dirs.push(directory);
                }
                dirs
            });
        let mut tzdir = None;
        let mut vulkan_loader = None;
        let mut raylib = None;
        for (name, output) in realized {
            let output = std::path::Path::new(output);
            match name.as_str() {
                "tzdata" => tzdir = Some(output.join("share").join("zoneinfo")),
                "vulkan-loader" => vulkan_loader = Some(output.join("lib")),
                "raylib" => raylib = Some(output.join("lib")),
                _ => {}
            }
        }
        let mut loader_paths = Vec::new();
        if let Some(path) = vulkan_loader {
            loader_paths.push(path);
        }
        if let Some(path) = raylib {
            loader_paths.push(path);
        }
        #[cfg(target_os = "linux")]
        if let Some(inherited) = inherited_loader_path.filter(|value| !value.is_empty()) {
            loader_paths.extend(
                inherited
                    .split(crate::Platform::path_separator())
                    .filter(|path| !path.is_empty())
                    .map(std::path::PathBuf::from),
            );
        }

        Self {
            project_root: project_root.to_path_buf(),
            native_bin_dirs,
            tzdir,
            loader_paths,
            activation: NativeActivation::Disabled,
            // Jetpack never creates Nix shell scratch directories. The
            // compatibility marker records that the old one-time cleanup
            // boundary is already satisfied; no hook runs.
            nix_shell_scratch: NixShellScratch::NotCreated,
        }
    }

    fn bin_dirs(&self) -> Vec<String> {
        self.native_bin_dirs
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect()
    }

    fn env_vars(&self) -> std::collections::BTreeMap<String, String> {
        let mut vars = std::collections::BTreeMap::from([(
            "JET_ROOT".to_string(),
            self.project_root.to_string_lossy().into_owned(),
        )]);
        match self.activation {
            NativeActivation::Disabled => {
                vars.insert(Syntax::ENV_DISABLE_VAR.to_string(), "1".to_string());
            }
        }
        match self.nix_shell_scratch {
            NixShellScratch::NotCreated => {
                vars.insert("JET_NIX_TMP_CLEANED".to_string(), "1".to_string());
            }
        }
        if let Some(tzdir) = &self.tzdir {
            vars.insert("TZDIR".to_string(), tzdir.to_string_lossy().into_owned());
        }
        #[cfg(target_os = "linux")]
        if !self.loader_paths.is_empty() {
            vars.insert(
                "LD_LIBRARY_PATH".to_string(),
                self.loader_paths
                    .iter()
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join(&crate::Platform::path_separator().to_string()),
            );
        }
        vars
    }
}

/// The two repo-local dev tools are native projections, not realized Nix
/// packages. Their definitions preserve the flake names while pointing at
/// the cargo-built binaries directly; argument forwarding remains the normal
/// child-process path, with no generated shell wrapper.
const NATIVE_DEV_TOOLS: [NativeDevTool; 2] = [
    NativeDevTool {
        definition: "jetDev",
        command: "jet",
        relative_binary: "target/debug/jet",
    },
    NativeDevTool {
        definition: "jetpackDev",
        command: "jetpack",
        relative_binary: "target/debug/jetpack",
    },
];

fn native_dev_tool_paths(project_root: &std::path::Path) -> Vec<(String, std::path::PathBuf)> {
    NATIVE_DEV_TOOLS
        .iter()
        .map(|tool| {
            (
                tool.command.to_string(),
                project_root.join(tool.relative_binary),
            )
        })
        .collect()
}

/// Native projection for the repository's dev-shell contract. These values
/// are derived from the selected project root and realized package outputs;
/// no shellHook, store-path literal, or external shell is involved.
fn native_environment_projection(
    project_root: &std::path::Path,
    realized: &[(String, String)],
    inherited_loader_path: Option<&str>,
) -> NativeEnvironmentProjection {
    NativeEnvironmentProjection::from_realized(project_root, realized, inherited_loader_path)
}

/// D-JPK-GRANTCMD1=A: `jet trust grant/list/explain/revoke`. Jetpack owns the
/// store; top-level `jet trust` dispatches here.
pub(super) fn cmd_trust(theme: &Theme, parsed: &Parsed) -> i32 {
    let store = Trust::store_path();
    match parsed.positional.first().map(String::as_str) {
        Some(v) if v == Syntax::TRUST_VERB_GRANT => {
            let Some(selector) = parsed.positional.get(1) else {
                theme.error(
                    "`jet trust grant` needs a grant selector",
                    "a grant selector names one package, build, env, service, image, fleet, or jetos authority.",
                    "try `jet trust grant service:postgres --scope repo`.",
                );
                return 2;
            };
            let scope = parsed
                .flags
                .trust_scope
                .as_deref()
                .unwrap_or(Syntax::TRUST_SCOPE_USER);
            let grant = match Trust::parse_grant_selector(selector, scope) {
                Ok(g) => g,
                Err(e) => {
                    theme.error(
                        "couldn't parse trust grant",
                        &e,
                        "use `--scope user` or `--scope repo`.",
                    );
                    return 2;
                }
            };
            let added = Trust::add_grant(&store, &grant);
            theme.status(&if added {
                format!(
                    "trusted {} `{}` ({})",
                    grant.authority, grant.subject, grant.scope
                )
            } else {
                format!(
                    "already trusted {} `{}` ({})",
                    grant.authority, grant.subject, grant.scope
                )
            });
            0
        }
        Some(v) if v == Syntax::TRUST_VERB_LIST => {
            let records = Trust::list_records(&store);
            if parsed.flags.json {
                println!("{}", Trust::records_json(&records));
            } else if records.is_empty() {
                theme.status("no trust grants yet.");
            } else {
                for record in records {
                    print_trust_record(theme, &record);
                }
            }
            0
        }
        Some(v) if v == Syntax::TRUST_VERB_EXPLAIN => {
            let records = Trust::list_records(&store);
            let selected = parsed.positional.get(1).map(String::as_str);
            let matches: Vec<_> = records
                .into_iter()
                .filter(|record| selected.is_none_or(|s| trust_record_matches(record, s)))
                .collect();
            if parsed.flags.json {
                println!("{}", Trust::records_json(&matches));
            } else if matches.is_empty() {
                theme.status("no matching trust grants.");
            } else {
                for record in matches {
                    print_trust_record(theme, &record);
                    match &record {
                        Trust::TrustRecord::Grant(grant) => theme.detail(&format!(
                            "exact authority: {} subject `{}`; revoke with `jet trust revoke {}`",
                            grant.authority,
                            grant.subject,
                            grant.key()
                        )),
                        Trust::TrustRecord::Hash { hash } => theme.detail(&format!(
                            "exact env/build hash grant; revoke with `jet trust revoke hash:{hash}`"
                        )),
                        Trust::TrustRecord::Pattern { pattern } => theme.detail(&format!(
                            "path pattern grant; revoke with `jet trust revoke pattern:{pattern}`"
                        )),
                        Trust::TrustRecord::Raw { line } => theme.detail(&format!(
                            "legacy/raw grant; revoke with `jet trust revoke {line}`"
                        )),
                    }
                }
            }
            0
        }
        Some(v) if v == Syntax::TRUST_VERB_REVOKE => {
            let Some(selector) = parsed.positional.get(1) else {
                theme.error(
                    "`jet trust revoke` needs a grant selector",
                    "revocation is exact: pass the subject, grant key, hash, pattern, or raw line shown by `jet trust list`.",
                    "try `jet trust revoke service:postgres.service`.",
                );
                return 2;
            };
            let removed = Trust::revoke(&store, selector);
            theme.status(&if removed {
                format!("revoked: {selector}")
            } else {
                format!("not found: {selector}")
            });
            0
        }
        _ => {
            theme.error(
                "`jet trust` needs a verb",
                &format!("the trust verbs are: {}.", Syntax::TRUST_VERBS.join(", ")),
                "try `list`, `explain`, `grant`, or `revoke`.",
            );
            2
        }
    }
}

fn print_trust_record(theme: &Theme, record: &Trust::TrustRecord) {
    match record {
        Trust::TrustRecord::Hash { hash } => theme.detail(&format!("hash     {hash}")),
        Trust::TrustRecord::Pattern { pattern } => theme.detail(&format!("pattern  {pattern}")),
        Trust::TrustRecord::Grant(grant) => theme.detail(&format!(
            "{:<7} {}  scope:{}",
            grant.authority, grant.subject, grant.scope
        )),
        Trust::TrustRecord::Raw { line } => theme.detail(&format!("raw      {line}")),
    }
}

fn trust_record_matches(record: &Trust::TrustRecord, selector: &str) -> bool {
    match record {
        Trust::TrustRecord::Hash { hash } => selector == hash || selector == format!("hash:{hash}"),
        Trust::TrustRecord::Pattern { pattern } => {
            selector == pattern || selector == format!("pattern:{pattern}")
        }
        Trust::TrustRecord::Grant(grant) => {
            selector == grant.subject
                || selector == grant.key()
                || selector == format!("{}:{}", grant.scope, grant.key())
        }
        Trust::TrustRecord::Raw { line } => selector == line,
    }
}

/// Realize every ref in `plan` and compose the shell env (PATH dirs + prompt
/// label). Returns an exit code after reporting if any ref fails to realize.
pub(super) fn compose_env(
    theme: &Theme,
    roots: &Roots,
    flags: &Flags,
    plan: &RunPlan,
) -> Result<Env, i32> {
    compose_env_scoped(theme, roots, flags, plan, RealizeScope::Project, false)
}

/// A user tool is installed into the user profile, so it must not be
/// reconciled against whatever project the shell happens to be standing in.
pub(super) fn compose_env_scoped(
    theme: &Theme,
    roots: &Roots,
    flags: &Flags,
    plan: &RunPlan,
    scope: RealizeScope,
    confirm_download: bool,
) -> Result<Env, i32> {
    if confirm_download {
        if let Err(code) = reject_unprompted_acquisition(theme, roots, flags, plan, scope) {
            return Err(code);
        }
    }
    enforce_required_sandbox_policy(theme, flags.json)?;
    if let Err(error) = validate_integration_facts(plan) {
        theme.error_coded(
            "E1335",
            "environment integration facts are not executable",
            &error,
            "use the supported integration preset and keep its typed package, host, task, and grant facts intact",
        );
        return Err(2);
    }
    let profile_bin = if scope == RealizeScope::Project {
        match super::profile::dev_shell_projection(&plan.project_root, &plan.environment) {
            Ok(profile_bin) => profile_bin,
            Err(error) => {
                theme.error_coded(
                    "E1335",
                    "the active package profile cannot project into the dev shell",
                    &error.to_string(),
                    "build and switch the named package profile, then retry the shell",
                );
                return Err(2);
            }
        }
    } else {
        None
    };
    let mut bin_dirs = profile_bin
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let mut provider_vars: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    let mut nix_vars = std::collections::BTreeMap::new();
    let mut realized_refs = Vec::new();
    let mut realized_outputs = Vec::new();
    let mut holes = Vec::new();
    let mut failed = false;
    let mut unavailable = false;
    let mut cache_leases = Vec::new();
    let name_w = name_column_width(&plan.refs);
    // Multi-package realization gets one pinned aggregate on a TTY and one
    // settled row per package. Plain output keeps only those settled rows so
    // a large package set does not become a duplicate status/row ledger.
    let total_steps = plan.refs.len() + plan.adapters.len();
    let live_mode = total_steps > 1;
    let aggregate_mode = scope == RealizeScope::Use && live_mode;
    let live_tty = live_mode && theme.color && std::io::stderr().is_terminal();
    let mut live = theme.live_region();
    let mut completed_steps = 0usize;
    for spec in plan.refs.iter() {
        if aggregate_mode {
            live.set_aggregate_status("resolving", completed_steps, total_steps);
        } else if live_tty {
            live.set_dependency_status(
                "resolving",
                completed_steps,
                total_steps,
                spec.source.label(),
                &spec.package,
                "resolving",
            );
        }
        let style = if live_mode {
            RowStyle::Silent
        } else {
            RowStyle::Ready
        };
        let live_arg = live_mode.then_some(&mut live);
        match realize_ref_outcome(
            theme,
            roots,
            flags,
            &plan.table,
            spec,
            name_w,
            style,
            live_arg,
            scope,
        ) {
            RefOutcome::Realized(entry, _state, line, lease) => {
                if live_mode {
                    live.finish(&line);
                }
                completed_steps += 1;
                // A `library` package realizes with an empty `bin` (U10) — it
                // stages source for import and contributes nothing to PATH.
                if !entry.bin.is_empty() {
                    bin_dirs.push(entry.bin);
                }
                let mut invalid_metadata = None;
                for (file, variable) in [
                    ("lua-path", "LUA_PATH"),
                    ("lua-cpath", "LUA_CPATH"),
                    ("gem-home", "GEM_HOME"),
                    ("gem-path", "GEM_PATH"),
                    ("ruby-lib", "RUBYLIB"),
                    ("perl5lib", "PERL5LIB"),
                    ("composer-autoload", "COMPOSER_AUTOLOAD"),
                ] {
                    if let Ok(value) =
                        std::fs::read_to_string(std::path::Path::new(&entry.out).join(file))
                    {
                        let value = value.trim();
                        if !value.is_empty() {
                            if let Some(value) = resolve_provider_paths(&entry.out, file, value) {
                                provider_vars
                                    .entry(variable.to_string())
                                    .or_default()
                                    .push(value);
                            } else {
                                invalid_metadata.get_or_insert(file);
                            }
                        }
                    }
                }
                if let Some(file) = invalid_metadata {
                    live.clear();
                    theme.error(
                        "couldn't compose package environment",
                        &format!(
                            "`{file}` for `{}` contains a relative path outside its verified package output.",
                            entry.reference
                        ),
                        "reinstall the package; provider metadata paths must be absolute or contain only normal relative components.",
                    );
                    failed = true;
                }
                if let Ok(producer) = Store::ProducerRecord::decode(&entry.producer_record) {
                    nix_vars.extend(Provider::nix_runtime_environment(&producer));
                }
                realized_outputs.push((entry.name.clone(), entry.out.clone()));
                realized_refs.push(entry.reference);
                cache_leases.push(lease);
            }
            RefOutcome::NeedsNix(need) => holes.push(need),
            RefOutcome::Unavailable => unavailable = true,
            RefOutcome::Failed => failed = true,
        }
    }
    for (idx, adapter) in plan.adapters.iter().enumerate() {
        live.clear();
        if total_steps > 1 {
            theme.progress_chain(
                "adapt",
                plan.refs.len() + idx + 1,
                total_steps,
                &adapter.name,
                "adapter",
            );
        }
        match realize_adapter(theme, roots, flags, adapter, &plan.table, true) {
            Some((entry, _state, lease)) => {
                if !entry.bin.is_empty() {
                    bin_dirs.push(entry.bin);
                }
                realized_refs.push(entry.reference);
                cache_leases.push(lease);
            }
            None => failed = true,
        }
    }
    if !holes.is_empty() {
        live.clear();
        report_nix_bridge_required(theme, flags, &holes, &realized_refs);
        return Err(2);
    }
    if unavailable {
        return Err(2);
    }
    if failed {
        return Err(1);
    }
    let missing_language_tools = plan
        .environment
        .language_packs
        .iter()
        .flat_map(|pack| {
            pack.required_tools.iter().filter_map(|tool| {
                let command = pack.commands.get(tool)?;
                let present = bin_dirs
                    .iter()
                    .any(|dir| std::path::Path::new(dir).join(command).is_file());
                (!present).then(|| format!("{}:{tool}", pack.name))
            })
        })
        .collect::<Vec<_>>();
    if !missing_language_tools.is_empty() {
        live.clear();
        theme.error_coded(
            "E1333",
            "a language pack tool is missing from the realized environment",
            &format!("missing required language tools: {}", missing_language_tools.join(", ")),
            "realize the pack's declared packages again, or remove the language selection until its tools are available",
        );
        return Err(2);
    }
    let inherited_loader_path = std::env::var("LD_LIBRARY_PATH").ok();
    let native_projection = (scope == RealizeScope::Project).then(|| {
        native_environment_projection(
            &plan.project_root,
            &realized_outputs,
            inherited_loader_path.as_deref(),
        )
    });
    if let Some(native_projection) = &native_projection {
        bin_dirs.extend(native_projection.bin_dirs());
    }
    let mut composed_vars: std::collections::BTreeMap<String, String> = provider_vars
        .into_iter()
        .map(|(name, values)| {
            let value = match name.as_str() {
                "LUA_PATH" | "LUA_CPATH" => format!("{};;", values.join(";")),
                "GEM_HOME" => values.into_iter().next().unwrap_or_default(),
                _ => values.join(&crate::Platform::path_separator().to_string()),
            };
            (name, value)
        })
        .collect();
    // The Nix compatibility provider contributes the fixed builder facts it
    // recorded at realization. PATH remains composed from verified packages;
    // these values only cover HOME, build scratch, store identity, and locale.
    composed_vars.extend(nix_vars);
    if let Some(preset) = &plan.environment.selected_preset {
        theme.detail(&format!("preset: {}", preset.applied.join(" -> ")));
        composed_vars.extend(preset.variables.clone());
    }
    for pack in &plan.environment.language_packs {
        composed_vars.extend(pack.variables.clone());
    }
    for dotenv in &plan.environment.lifecycle.dotenv {
        let path = &dotenv.file;
        let relative = std::path::Path::new(path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| component == std::path::Component::ParentDir)
        {
            live.clear();
            theme.error(
                "couldn't load dotenv file",
                &format!("`{path}` is not a project-relative path"),
                "keep dotenv files inside the project and remove absolute or `..` paths.",
            );
            return Err(2);
        }
        let dotenv_path = plan.project_root.join(relative);
        match read_dotenv(&dotenv_path) {
            Ok(values) => {
                for (name, value) in values {
                    if !dotenv.allow.is_empty() && !dotenv.allow.iter().any(|item| item == &name) {
                        continue;
                    }
                    composed_vars.insert(name, value);
                }
            }
            Err(error) => {
                live.clear();
                theme.error(
                    "couldn't load dotenv file",
                    &format!("{}: {error}", dotenv_path.display()),
                    "fix the dotenv path and keep each assignment in KEY=value form.",
                );
                return Err(2);
            }
        }
    }
    if let Some(relative) = &plan.environment.lifecycle.git_hooks_path {
        if plan.environment.lifecycle.unset.iter().any(|name| {
            matches!(
                name.as_str(),
                "GIT_CONFIG_COUNT" | "GIT_CONFIG_KEY_0" | "GIT_CONFIG_VALUE_0"
            )
        }) {
            live.clear();
            theme.error_coded(
                "E1333",
                "the environment Git hook configuration conflicts with `unset`",
                "Git cannot receive core.hooksPath when its configuration environment is removed",
                "remove the Git configuration names from `unset`, or remove `git_hooks_path`",
            );
            return Err(2);
        }
        match EnvHook::git_hooks_environment(&plan.project_root, relative) {
            Ok(values) => composed_vars.extend(values),
            Err(error) => {
                live.clear();
                theme.error_coded(
                    "E1333",
                    "the environment Git hook path is not usable",
                    &error,
                    "create an in-project hook directory and set `git_hooks_path` to its project-relative path",
                );
                return Err(2);
            }
        }
    }
    // Native projection is applied last so the Jet-owned root, markers, and
    // generated output paths cannot be shadowed by dotenv or preset values.
    if let Some(native_projection) = native_projection {
        composed_vars.extend(native_projection.env_vars());
    }
    // Tier 1 (D-FE-CLI1): the per-package `✓` rows above are the whole
    // report — `jet env`/`run`/`dev` hand off straight to the shell
    // threshold rule (`Shell::enter`) instead of a redundant summary line.
    Ok(Env {
        bin_dirs,
        vars: composed_vars,
        unset_vars: plan.environment.lifecycle.unset.clone(),
        refs: realized_refs,
        label: plan.label.clone(),
        prompt_path: plan.prompt_path,
        prompt_strip: plan.prompt_strip,
        cache_leases,
    })
}

/// A piped `jetpack env`/`use` command must not start realization without an
/// explicit `-y`. Existing Hangar entries are allowed through so fully cached
/// environments stay silent; every missing package is rejected before the
/// provider or Store realization boundary can acquire bytes.
fn reject_unprompted_acquisition(
    theme: &Theme,
    roots: &Roots,
    flags: &Flags,
    plan: &RunPlan,
    scope: RealizeScope,
) -> Result<(), i32> {
    let specs = plan
        .refs
        .iter()
        .filter(|spec| ref_needs_acquisition(spec, &plan.table))
        .cloned()
        .collect::<Vec<_>>();
    let mut download = plan_downloads(theme, roots, flags, &plan.table, &specs, scope)?;
    if !plan.adapters.is_empty() {
        download.packages = download.packages.saturating_add(plan.adapters.len());
        download.bytes = None;
    }
    if download.packages == 0 {
        return Ok(());
    }
    let label = match scope {
        RealizeScope::Project => plan
            .environment
            .active_environment
            .as_deref()
            .map_or_else(|| "env".to_string(), |name| format!("env.{name}")),
        RealizeScope::Use => "use".to_string(),
        RealizeScope::UserProfile => "tool".to_string(),
    };
    if theme.confirm_download(
        &label,
        download.packages,
        download.bytes,
        flags.assume_yes,
    ) {
        Ok(())
    } else {
        Err(2)
    }
}

fn ref_needs_acquisition(spec: &RefSpec::RefSpec, table: &RefSpec::SourceTable) -> bool {
    let provider = match &spec.source {
        RefSpec::Source::Named(name) => table.provider(name),
        RefSpec::Source::Releases => ProviderKind::JetPackage,
        RefSpec::Source::Cran => ProviderKind::Cran,
        RefSpec::Source::LuaRocks => ProviderKind::LuaRocks,
        RefSpec::Source::RubyGems => ProviderKind::RubyGems,
        RefSpec::Source::Cpan => ProviderKind::Cpan,
        RefSpec::Source::Packagist => ProviderKind::Packagist,
        RefSpec::Source::JetRegistry => ProviderKind::JetRegistry,
        RefSpec::Source::Npm => ProviderKind::Npm,
        RefSpec::Source::Cargo => ProviderKind::Cargo,
        RefSpec::Source::PyPI => ProviderKind::PyPI,
        RefSpec::Source::SwiftPM => ProviderKind::SwiftPM,
        RefSpec::Source::Jetpack
        | RefSpec::Source::Nixpkgs
        | RefSpec::Source::Github
        | RefSpec::Source::Path => ProviderKind::Nix,
    };
    if provider != ProviderKind::Core {
        return true;
    }
    table
        .upstream(spec.source.label())
        .is_none_or(|upstream| !upstream.starts_with("path:"))
}

pub(super) fn validate_integration_facts(plan: &RunPlan) -> Result<(), String> {
    plan.environment.integration_facts.validate()?;
    let target = std::env::var("JET_TARGET").unwrap_or_else(|_| {
        let os = if cfg!(target_os = "macos") {
            "darwin"
        } else {
            std::env::consts::OS
        };
        format!("{}-{os}", std::env::consts::ARCH)
    });
    for task in &plan.environment.integration_facts.task_facts {
        if !plan
            .environment
            .integration_facts
            .tasks
            .contains(&task.name)
        {
            return Err(format!(
                "integration task `{}` is not disclosed by its fact projection",
                task.name
            ));
        }
        for package in &task.packages {
            if !plan
                .refs
                .iter()
                .any(|reference| reference.raw == *package || reference.short_name() == *package)
            {
                return Err(format!(
                    "integration task `{}` lost package `{package}` before realization",
                    task.name
                ));
            }
        }
        for secret in &task.secrets {
            validate_task_secret_allowlist(&task.name, secret, &plan.secrets)?;
        }
        let expected_provider = match task.integration {
            ModuleEval::IntegrationKind::Android
            | ModuleEval::IntegrationKind::Apple
            | ModuleEval::IntegrationKind::Editor => "nixpkgs",
            ModuleEval::IntegrationKind::Certificates | ModuleEval::IntegrationKind::Vault => {
                "vault"
            }
            ModuleEval::IntegrationKind::CloudCredentials => "credential-store",
            ModuleEval::IntegrationKind::Hosts => "host-binding",
            ModuleEval::IntegrationKind::CodexAgent => "mcp",
        };
        if !task
            .providers
            .iter()
            .any(|provider| provider == expected_provider)
        {
            return Err(format!(
                "integration task `{}` has no executable `{expected_provider}` provider",
                task.name
            ));
        }
        if task.providers.iter().any(|provider| {
            !matches!(
                provider.as_str(),
                "nixpkgs" | "vault" | "credential-store" | "host-binding" | "mcp"
            )
        }) {
            return Err(format!(
                "integration task `{}` names an unsupported provider",
                task.name
            ));
        }
        for provider in &task.providers {
            if !plan
                .environment
                .integration_facts
                .providers
                .contains(provider)
            {
                return Err(format!(
                    "integration task `{}` lost provider `{provider}` before realization",
                    task.name
                ));
            }
        }
        for grant in &task.grants {
            if !plan.environment.integration_facts.grants.contains(grant) {
                return Err(format!(
                    "integration task `{}` lost grant `{grant}` before realization",
                    task.name
                ));
            }
        }
        for check in &task.host_checks {
            if let Err(error) = validate_integration_host_check(check, &target) {
                return Err(format!(
                    "{} integration `{}` failed: {error}",
                    task.integration.as_str(),
                    task.name
                ));
            }
        }
    }
    if plan
        .environment
        .integration_facts
        .providers
        .iter()
        .any(|provider| provider.trim().is_empty())
    {
        return Err("integration provider authority cannot be empty".to_string());
    }
    if plan
        .environment
        .integration_facts
        .host_checks
        .iter()
        .any(|check| check.trim().is_empty())
    {
        return Err("integration host checks cannot be empty".to_string());
    }
    Ok(())
}

fn validate_task_secret_allowlist(
    task_name: &str,
    secret: &str,
    declared: &[ModuleEval::SecretSpec],
) -> Result<(), String> {
    declared
        .iter()
        .any(|name| name.name == secret)
        .then_some(())
        .ok_or_else(|| {
            format!("integration task `{task_name}` lost secret `{secret}` before activation")
        })
}

fn validate_integration_host_check(check: &str, target: &str) -> Result<(), String> {
    let Some(expected) = check.strip_prefix("target:") else {
        return Ok(());
    };
    let target = target.to_ascii_lowercase();
    if expected
        .split("-or-")
        .any(|candidate| target.contains(&candidate.to_ascii_lowercase()))
    {
        Ok(())
    } else {
        Err(format!(
            "integration host check `{check}` failed for target `{target}`"
        ))
    }
}

fn read_dotenv(
    path: &std::path::Path,
) -> Result<std::collections::BTreeMap<String, String>, String> {
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let mut values = std::collections::BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            return Err(format!("line {} has no '='", index + 1));
        };
        let name = name.trim();
        let mut bytes = name.bytes();
        let valid_start = matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_'));
        if !valid_start || !bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric()) {
            return Err(format!("line {} has an invalid variable name", index + 1));
        }
        let mut value = value.trim().to_string();
        if value.len() >= 2
            && ((value.starts_with(char::from(34)) && value.ends_with(char::from(34)))
                || (value.starts_with(char::from(39)) && value.ends_with(char::from(39))))
        {
            value = value[1..value.len() - 1].to_string();
        }
        values.insert(name.to_string(), value);
    }
    Ok(values)
}

fn resolve_provider_paths(entry_out: &str, file: &str, value: &str) -> Option<String> {
    if !matches!(
        file,
        "lua-path"
            | "lua-cpath"
            | "gem-home"
            | "gem-path"
            | "ruby-lib"
            | "perl5lib"
            | "composer-autoload"
    ) {
        return Some(value.to_string());
    }
    let separator = if matches!(file, "lua-path" | "lua-cpath") {
        ';'
    } else {
        crate::Platform::path_separator()
    };
    value
        .split(separator)
        .map(|path| {
            let relative = std::path::Path::new(path);
            if relative.is_absolute() {
                return Some(path.to_string());
            }
            let mut components = relative.components();
            if components.clone().next().is_none()
                || !components.all(|component| {
                    matches!(
                        component,
                        std::path::Component::CurDir | std::path::Component::Normal(_)
                    )
                })
            {
                return None;
            }
            Some(
                std::path::Path::new(entry_out)
                    .join(relative)
                    .to_string_lossy()
                    .into_owned(),
            )
        })
        .collect::<Option<Vec<_>>>()
        .map(|paths| paths.join(&separator.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{build_sandbox_outcome, resolve_provider_paths, validate_task_secret_allowlist};
    use jet_env_model::ModuleEval;

    #[test]
    fn sandbox_claim_uses_recorded_backend_or_says_no_child_ran() {
        assert_eq!(
            build_sandbox_outcome(
                1,
                0,
                &[("non-executing".into(), "no child launched".into(),)],
            ),
            "non-executing (no child launched)"
        );
        assert_eq!(
            build_sandbox_outcome(
                1,
                0,
                &[(
                    "linux-bwrap".into(),
                    "filesystem=source-readonly,output-private-copy;process=private-pid,parent-death;network=isolated;environment=clear;devices=private-dev;privilege=no-new-privs+cap-drop-all;resources=tmpfs-64MiB".into(),
                )],
            ),
            "enforced via linux-bwrap (filesystem=source-readonly,output-private-copy;process=private-pid,parent-death;network=isolated;environment=clear;devices=private-dev;privilege=no-new-privs+cap-drop-all;resources=tmpfs-64MiB)"
        );
        assert_eq!(
            build_sandbox_outcome(1, 0, &[]),
            "sandbox receipt missing for 1 built output(s)"
        );
    }

    #[test]
    fn sandbox_claim_rejects_unknown_or_incomplete_receipts() {
        assert_eq!(
            build_sandbox_outcome(1, 0, &[("linux-userns".into(), "not-enforced".into())],),
            "sandbox receipt missing for 1 built output(s)"
        );
        assert_eq!(
            build_sandbox_outcome(
                1,
                0,
                &[("linux-bwrap".into(), "filesystem=source-readonly".into())],
            ),
            "sandbox receipt missing for 1 built output(s)"
        );
    }

    #[test]
    fn lua_metadata_paths_resolve_inside_realized_output() {
        assert_eq!(
            resolve_provider_paths(
                "/hangar/objects/sha256-output",
                "lua-path",
                "share/lua/5.4/?.lua;share/lua/5.4/?/init.lua",
            ),
            Some("/hangar/objects/sha256-output/share/lua/5.4/?.lua;/hangar/objects/sha256-output/share/lua/5.4/?/init.lua".into())
        );
    }

    #[test]
    fn relative_provider_metadata_rejects_parent_escape() {
        assert_eq!(
            resolve_provider_paths("/hangar/objects/sha256-output", "lua-path", "../outside"),
            None
        );
    }

    #[test]
    fn current_directory_provider_metadata_resolves_inside_realized_output() {
        assert_eq!(
            resolve_provider_paths("/hangar/objects/sha256-output", "gem-home", "."),
            Some("/hangar/objects/sha256-output/.".into())
        );
    }

    #[test]
    fn absolute_legacy_provider_metadata_is_preserved() {
        assert_eq!(
            resolve_provider_paths(
                "/hangar/objects/sha256-output",
                "lua-path",
                "/legacy/a/?.lua;/legacy/b/?/init.lua",
            ),
            Some("/legacy/a/?.lua;/legacy/b/?/init.lua".into())
        );
    }

    #[test]
    fn activation_denies_task_secret_missing_from_declared_list() {
        let error = validate_task_secret_allowlist(
            "vault-check",
            "database_password",
            &[ModuleEval::SecretSpec::stored("api_key")],
        )
        .expect_err("activation must deny a task secret outside its declared list");
        assert!(
            error.contains("lost secret `database_password` before activation"),
            "unexpected activation denial"
        );
    }
}

#[cfg(test)]
pub(crate) fn compose_refs_for_test(
    roots: &Roots,
    refs: Vec<RefSpec::RefSpec>,
) -> Result<Env, i32> {
    let parsed = super::parse::parse_args_for("", &[]);
    compose_env(
        &Theme::resolve(true),
        roots,
        &parsed.flags,
        &RunPlan {
            project_root: std::env::current_dir().unwrap_or_default(),
            refs,
            adapters: Vec::new(),
            table: RefSpec::SourceTable::empty(),
            label: "provider-test".into(),
            prompt_path: ModuleEval::PromptPathMode::default(),
            prompt_strip: ModuleEval::PromptStripMode::default(),
            dev_services: Vec::new(),
            secrets: Vec::new(),
            environment: ModuleEval::EnvironmentFacts::default(),
        },
    )
}

/// The ledger's name-column width for a set of refs (min 8 so a single short
/// name doesn't collapse the table).
fn name_column_width(refs: &[RefSpec::RefSpec]) -> usize {
    refs.iter()
        .map(|r| r.package.len())
        .max()
        .unwrap_or(0)
        .max(8)
}

/// D-JPK-SELECTOR1=C: turn CLI flags into a workspace selection request.
#[allow(dead_code)]
fn select_request_from_flags(flags: &Flags) -> SelectRequest {
    SelectRequest {
        packages: flags.workspace_members.clone(),
        affected: flags.affected,
        affected_since: flags.affected_since.clone(),
    }
}

#[allow(dead_code)]
fn report_select_error(theme: &Theme, d: &crate::Diagnostics::Diagnostic) -> i32 {
    theme.error_coded(&d.code, &d.what, &d.why, &d.fix);
    2
}

#[allow(dead_code)]
fn sandbox_receipt(entry: &Store::StoreEntry) -> Option<(String, String)> {
    let producer = Store::ProducerRecord::decode(&entry.producer_record).ok()?;
    let receipt = (
        producer.facts.get("build.sandbox")?.clone(),
        producer.facts.get("build.sandbox_policy")?.clone(),
    );
    crate::RuntimePolicy::sandbox_receipt_is_truthful(&receipt.0, &receipt.1).then_some(receipt)
}

#[allow(dead_code)]
fn build_sandbox_outcome(
    built: usize,
    substituted: usize,
    receipts: &[(String, String)],
) -> String {
    if built > 0 {
        let receipts = receipts
            .iter()
            .filter(|(class, policy)| {
                crate::RuntimePolicy::sandbox_receipt_is_truthful(class, policy)
            })
            .collect::<Vec<_>>();
        if receipts.len() < built {
            return format!(
                "sandbox receipt missing for {} built output(s)",
                built - receipts.len()
            );
        }
        let mut classes = std::collections::BTreeSet::new();
        let mut policies = std::collections::BTreeSet::new();
        for (class, policy) in receipts {
            classes.insert(class.as_str());
            policies.insert(policy.as_str());
        }
        if classes.iter().all(|class| *class == "non-executing") {
            return "non-executing (no child launched)".to_string();
        }
        return format!(
            "enforced via {} ({})",
            classes.into_iter().collect::<Vec<_>>().join(","),
            policies.into_iter().collect::<Vec<_>>().join(" | ")
        );
    }
    if substituted > 0 {
        "trusted substitution (no local executable launched)".to_string()
    } else {
        "verified cache only".to_string()
    }
}

/// Build (or test) each selected workspace member via the core provider.
#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum WorkspaceAction {
    Build,
    Test,
}

impl WorkspaceAction {
    fn present(self) -> &'static str {
        match self {
            Self::Build => "building",
            Self::Test => "testing",
        }
    }

    fn past(self) -> &'static str {
        match self {
            Self::Build => "built",
            Self::Test => "tested",
        }
    }
}

#[allow(dead_code)]
fn run_workspace_members(
    theme: &Theme,
    parsed: &Parsed,
    dir: &std::path::Path,
    checked_source: &(AuthorityResolver, WorkspaceSource),
    plan_members: &[WorkspaceMember],
    action: WorkspaceAction,
) -> i32 {
    let roots = Store::resolve();
    let mut ok = true;
    let mut built: Vec<WorkspaceMember> = Vec::new();
    let (resolver, source) = checked_source;
    if source.role != WorkspaceSourceRole::Index {
        return report_select_error(theme, &workspace_index_required_diagnostic());
    }
    if let Err(error) = resolver.revalidate_source(source) {
        return report_select_error(theme, &error.diagnostic());
    }
    let ordered_members =
        match MemberSelect::dependency_order_packages_checked(resolver, plan_members) {
            Ok(ordered_members) => ordered_members,
            Err(error) => {
                let diagnostic = error.diagnostic();
                return report_select_error(theme, &diagnostic);
            }
        };
    for (idx, (member, checked_package)) in ordered_members.iter().enumerate() {
        if let Err(error) = resolver.revalidate_member(&checked_package.member) {
            ok = false;
            let diagnostic = error.diagnostic();
            let _ = report_select_error(theme, &diagnostic);
            continue;
        }
        if let Err(error) = resolver.revalidate_source(source) {
            ok = false;
            let diagnostic = error.diagnostic();
            let _ = report_select_error(theme, &diagnostic);
            continue;
        }
        theme.status(&format!(
            "{} workspace member: {}",
            action.present(),
            member.name
        ));
        if ordered_members.len() > 1 {
            theme.progress_chain(
                action.present(),
                idx + 1,
                ordered_members.len(),
                &member.name,
                "workspace",
            );
        }
        let abs = checked_package.member.directory.path.clone();
        let table = RefSpec::SourceTable::from_decls([(
            member.name.clone(),
            format!("path:{}", abs.display()),
            ProviderKind::Core,
        )]);
        let raw = format!("{}@{}", member.name, member.name);
        let spec = match RefSpec::classify_in(&raw, &table) {
            Ok(s) => s,
            Err(e) => {
                Output::ref_error(theme, &e);
                ok = false;
                continue;
            }
        };
        if let Err(error) = resolver
            .revalidate_source(source)
            .and_then(|_| resolver.revalidate_member(&checked_package.member))
        {
            ok = false;
            let diagnostic = error.diagnostic();
            let _ = report_select_error(theme, &diagnostic);
            continue;
        }
        let realized = realize_ref(
            theme,
            &roots,
            &parsed.flags,
            &table,
            &spec,
            member.name.len().max(8),
        );
        let authority_valid = resolver
            .revalidate_source(source)
            .and_then(|_| resolver.revalidate_member(&checked_package.member));
        if let Err(error) = authority_valid {
            ok = false;
            let diagnostic = error.diagnostic();
            let _ = report_select_error(theme, &diagnostic);
        } else if realized.is_none() {
            ok = false;
        } else if action == WorkspaceAction::Test {
            if run_jet_tests(&checked_package.member.directory.path) {
            } else {
                ok = false;
            }
        } else {
            built.push(member.clone());
        }
    }
    if ok {
        if let Err(error) = resolver.revalidate_source(source) {
            return report_select_error(theme, &error.diagnostic());
        }
        MemberSelect::record_member_input_hashes(dir, &built);
        theme.status(&format!(
            "{} {} workspace member(s).",
            action.past(),
            ordered_members.len()
        ));
        0
    } else {
        1
    }
}

/// `jetpack env --prep [<ref>]` — realize without entering a shell.
#[allow(dead_code)]
pub(super) fn cmd_build(theme: &Theme, parsed: &Parsed) -> i32 {
    let roots = Store::resolve();
    let dir = std::env::current_dir().unwrap_or_default();
    let (workspace_dir, workspace_source) = workspace_root_snapshot_or_exit(&dir);
    if let Err(code) = enforce_required_sandbox_policy(theme, parsed.flags.json) {
        return code;
    }
    // D-WORKSPACE1=B: if a workspace declaration is present, build selected
    // members via the first-party core provider (no Nix required).
    if let Some(checked) = workspace_source.as_ref() {
        if let Some(result) = load_workspace_for_source(&workspace_dir, checked) {
            return match result {
                Err(code) => code,
                Ok(plan) => {
                    let req = select_request_from_flags(&parsed.flags);
                    let selected = match MemberSelect::select_members(&workspace_dir, &plan, &req) {
                        Ok(m) => m,
                        Err(d) => return report_select_error(theme, &d),
                    };
                    if selected.is_empty() {
                        theme.status("no workspace members matched the selection.");
                        return 0;
                    }
                    run_workspace_members(
                        theme,
                        parsed,
                        &workspace_dir,
                        checked,
                        &selected,
                        WorkspaceAction::Build,
                    )
                }
            };
        }
    }

    let mut plan = match parsed.positional.first() {
        Some(raw) => match classify_or_report(theme, raw) {
            Ok(s) => RunPlan {
                project_root: project_root(&dir),
                refs: vec![s],
                adapters: Vec::new(),
                table: cwd_table(),
                label: Syntax::JETPACK_PROMPT_LABEL.to_string(),
                prompt_path: ModuleEval::PromptPathMode::default(),
                prompt_strip: ModuleEval::PromptStripMode::default(),
                dev_services: Vec::new(),
                secrets: Vec::new(),
                environment: ModuleEval::EnvironmentFacts::default(),
            },
            Err(_) => return 2,
        },
        None => match load_project_plan(theme) {
            Ok(plan) => plan,
            Err(code) => return code,
        },
    };
    if let Err(code) = apply_locked_channels(theme, &dir, &mut plan.table, &parsed.flags) {
        return code;
    }

    let mut ok = true;
    let (mut built, mut cached, mut substituted) = (0usize, 0usize, 0usize);
    let mut realized_refs = Vec::new();
    let mut holes = Vec::new();
    let mut sandbox_receipts = Vec::new();
    let name_w = name_column_width(&plan.refs);
    let total_steps = plan.refs.len() + plan.adapters.len();
    let mut completed_steps = 0usize;
    // Tier 2 (D-FE-CLI1): a multi-package build gets the live region —
    // finished rows promote up out of a pinned `building K/N` + progress bar
    // status, which collapses to `build ready ✓` on success. A single
    // package stays the plain quiet ledger row (nothing to promote out of).
    let live_mode = total_steps > 1;
    let mut live = theme.live_region();
    for spec in &plan.refs {
        let style = if live_mode {
            live.set_dependency_status(
                "building",
                completed_steps,
                total_steps,
                spec.source.label(),
                &spec.package,
                "resolving",
            );
            RowStyle::Silent
        } else {
            RowStyle::Ledger
        };
        let live_arg = if live_mode { Some(&mut live) } else { None };
        match realize_ref_outcome(
            theme,
            &roots,
            &parsed.flags,
            &plan.table,
            spec,
            name_w,
            style,
            live_arg,
            RealizeScope::Project,
        ) {
            RefOutcome::Realized(entry, state, line, _lease) => {
                if live_mode {
                    live.finish(&line);
                }
                if state == Provider::SourceState::Built {
                    if let Some(receipt) = sandbox_receipt(&entry) {
                        sandbox_receipts.push(receipt);
                    }
                }
                realized_refs.push(entry.reference);
                completed_steps += 1;
                match state {
                    Provider::SourceState::Built | Provider::SourceState::Downloaded => built += 1,
                    Provider::SourceState::Cached => cached += 1,
                    Provider::SourceState::Substituted => substituted += 1,
                }
            }
            RefOutcome::NeedsNix(need) => holes.push(need),
            RefOutcome::Unavailable | RefOutcome::Failed => ok = false,
        }
    }
    for adapter in &plan.adapters {
        if total_steps > 1 {
            live.set_dependency_status(
                "building",
                completed_steps,
                total_steps,
                "adapter",
                &adapter.name,
                "adapting",
            );
            // Adapter realization owns its diagnostic/ledger output today;
            // erase the pinned projection before handing control to it.
            live.clear();
        }
        match realize_adapter(theme, &roots, &parsed.flags, adapter, &plan.table, false) {
            Some((entry, state, _lease)) => {
                if state == Provider::SourceState::Built {
                    if let Some(receipt) = sandbox_receipt(&entry) {
                        sandbox_receipts.push(receipt);
                    }
                }
                realized_refs.push(entry.reference);
                completed_steps += 1;
                match state {
                    Provider::SourceState::Built | Provider::SourceState::Downloaded => built += 1,
                    Provider::SourceState::Cached => cached += 1,
                    Provider::SourceState::Substituted => substituted += 1,
                }
            }
            None => ok = false,
        }
    }
    if !holes.is_empty() {
        // Erase any pinned region before the Nix-bridge diagnostic (D-FE-CLI1).
        if live_mode {
            live.clear();
        }
        report_nix_bridge_required(theme, &parsed.flags, &holes, &realized_refs);
        return 2;
    }
    if ok {
        // Tier 2 (D-FE-CLI1): collapse the live region to its one-line close
        // before the T4 source-state summary.
        if live_mode {
            live.collapse(&format!("build ready {}", theme.green("✓")));
        }
        // T4: per-run source-state summary (mirrors the D-JPK-CACHE1 example).
        theme.status(&format!(
            "built {} package(s): {} built, {} cached, {} substituted",
            plan.refs.len() + plan.adapters.len(),
            built,
            cached,
            substituted
        ));
        let outcome = build_sandbox_outcome(built, substituted, &sandbox_receipts);
        theme.detail(&format!("build sandbox outcome: {outcome}"));
        auto_clean_after_success(theme, &roots);
        0
    } else {
        // Failure path: region already cleared before each diagnostic inside
        // `realize_ref_outcome`; force one last erase so a stale bar cannot
        // survive past the process exit (D-FE-CLI1 still 8 / hybrid.html).
        if live_mode {
            live.clear();
        }
        2
    }
}

/// `jet test` — realize selected workspace members (D-JPK-SELECTOR1=C).
/// Outside a workspace, falls through to the same project-plan realize path as
/// `build` (tests ride the package after realize).
#[allow(dead_code)]
fn run_jet_tests(dir: &std::path::Path) -> bool {
    // Cargo runs integration-test binaries from `target/debug/deps`, while
    // the sibling compiler binary stays in `target/debug`. Find that binary
    // before falling back to the normal installed/PATH lookup.
    let jet = std::env::current_exe()
        .ok()
        .and_then(|exe| {
            let deps = exe.parent()?;
            (deps.file_name().and_then(|name| name.to_str()) == Some("deps")).then_some(
                deps.parent()?.join(if cfg!(windows) {
                    "jet.exe"
                } else {
                    Syntax::BINARY_NAME
                }),
            )
        })
        .filter(|candidate| candidate.is_file())
        .map(|candidate| candidate.to_string_lossy().into_owned())
        .unwrap_or_else(find_jet_binary);
    match std::process::Command::new(jet)
        .arg("test")
        .arg("--show-default")
        .arg(dir)
        .current_dir(dir)
        .status()
    {
        Ok(status) => status.success(),
        Err(error) => {
            eprintln!("jet could not run jet test: {error}");
            false
        }
    }
}

#[allow(dead_code)]
pub(super) fn cmd_test(theme: &Theme, parsed: &Parsed) -> i32 {
    let dir = std::env::current_dir().unwrap_or_default();
    let (workspace_dir, workspace_source) = workspace_root_snapshot_or_exit(&dir);
    if let Err(code) = enforce_required_sandbox_policy(theme, parsed.flags.json) {
        return code;
    }
    if let Some(checked) = workspace_source.as_ref() {
        if let Some(result) = load_workspace_for_source(&workspace_dir, checked) {
            return match result {
                Err(code) => code,
                Ok(plan) => {
                    let req = select_request_from_flags(&parsed.flags);
                    let selected = match MemberSelect::select_members(&workspace_dir, &plan, &req) {
                        Ok(m) => m,
                        Err(d) => return report_select_error(theme, &d),
                    };
                    if selected.is_empty() {
                        theme.status("no workspace members matched the selection.");
                        return 0;
                    }
                    let names: Vec<_> = selected.iter().map(|m| m.name.as_str()).collect();
                    theme.status(&format!(
                        "running {} members: {}",
                        names.len(),
                        names.join(", ")
                    ));
                    run_workspace_members(
                        theme,
                        parsed,
                        &workspace_dir,
                        checked,
                        &selected,
                        WorkspaceAction::Test,
                    )
                }
            };
        }
    }
    // Non-workspace: identical realize path to build, then run the package's
    // tests with the compiler's canonical test semantics.
    let code = cmd_build(theme, parsed);
    if code == 0 && run_jet_tests(&dir) {
        0
    } else if code != 0 {
        code
    } else {
        1
    }
}

fn enforce_required_sandbox_policy(theme: &Theme, json: bool) -> Result<(), i32> {
    if matches!(
        RuntimePolicy::read_sandbox_policy(),
        RuntimePolicy::SandboxPolicy::Require
    ) {
        RuntimePolicy::enforce_sandbox_policy(theme, json)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod native_projection_tests {
    use super::{
        native_dev_tool_paths, native_environment_projection, NativeActivation, NixShellScratch,
        NATIVE_DEV_TOOLS,
    };
    use std::path::Path;

    #[test]
    fn native_dev_tools_define_both_repo_binaries_without_shell_wrappers() {
        assert_eq!(
            NATIVE_DEV_TOOLS
                .iter()
                .map(|tool| tool.definition)
                .collect::<Vec<_>>(),
            vec!["jetDev", "jetpackDev"]
        );
        let tools = native_dev_tool_paths(Path::new("/workspace/jet"));
        assert_eq!(
            tools,
            vec![
                ("jet".to_string(), "/workspace/jet/target/debug/jet".into()),
                (
                    "jetpack".to_string(),
                    "/workspace/jet/target/debug/jetpack".into()
                ),
            ]
        );
    }

    #[test]
    fn native_projection_derives_root_timezone_and_loader_paths() {
        let projection = native_environment_projection(
            Path::new("/workspace/jet"),
            &[
                ("tzdata".into(), "/hangar/tzdata-1".into()),
                ("raylib".into(), "/hangar/raylib-1".into()),
                ("vulkan-loader".into(), "/hangar/vulkan-1".into()),
            ],
            Some("/host/lib"),
        );
        assert_eq!(projection.bin_dirs(), vec!["/workspace/jet/target/debug"]);
        let vars = projection.env_vars();
        assert_eq!(
            vars.get("JET_ROOT").map(String::as_str),
            Some("/workspace/jet")
        );
        assert_eq!(vars.get("JET_ENV_DISABLE").map(String::as_str), Some("1"));
        assert_eq!(
            vars.get("JET_NIX_TMP_CLEANED").map(String::as_str),
            Some("1")
        );
        assert_eq!(
            vars.get("TZDIR").map(String::as_str),
            Some("/hangar/tzdata-1/share/zoneinfo")
        );
        #[cfg(target_os = "linux")]
        assert_eq!(
            vars.get("LD_LIBRARY_PATH").map(String::as_str),
            Some("/hangar/vulkan-1/lib:/hangar/raylib-1/lib:/host/lib")
        );
        assert_eq!(projection.activation, NativeActivation::Disabled);
        assert_eq!(projection.nix_shell_scratch, NixShellScratch::NotCreated);
    }
}
