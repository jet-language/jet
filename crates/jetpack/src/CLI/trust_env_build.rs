use super::package_hangar_vendor::auto_clean_after_success;
use super::parse::{Flags, Parsed};
use super::realize::{
    apply_locked_channels, classify_or_report, load_project_plan, realize_adapter, realize_ref,
    report_nix_bridge_required, realize_ref_outcome, RefOutcome, RowStyle, RunPlan,
};
use super::workspace_sources::{cwd_table, load_workspace};
use crate::MemberSelect::{self, SelectRequest};
use jet_env_model::ModuleEval;
use crate::Output::{self, Theme};
use crate::Provider;
use crate::RefSpec::{self, ProviderKind};
use crate::RuntimePolicy;
use crate::Shell::Env;
use crate::Store::{self, Roots};
use crate::Syntax;
use crate::Trust;
use crate::WorkspaceFile::WorkspaceMember;

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
pub(super) fn compose_env(theme: &Theme, roots: &Roots, flags: &Flags, plan: &RunPlan) -> Result<Env, i32> {
    RuntimePolicy::enforce_sandbox_policy(theme, flags.json)?;
    if let Err(error) = validate_integration_facts(plan) {
        theme.error_coded(
            "E1335",
            "environment integration facts are not executable",
            &error,
            "use the supported integration preset and keep its typed package, host, task, and grant facts intact",
        );
        return Err(2);
    }
    let mut bin_dirs = Vec::new();
    let mut provider_vars: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
    let mut realized_refs = Vec::new();
    let mut holes = Vec::new();
    let mut failed = false;
    let mut cache_leases = Vec::new();
    let name_w = name_column_width(&plan.refs);
    // Tier 1 (D-FE-CLI1): `jet env`/`run`/`dev` composing a project's
    // packages is the trivial-op case — one `✓ name version` row per
    // package, no state/duration column.
    let total_steps = plan.refs.len() + plan.adapters.len();
    for spec in plan.refs.iter() {
        match realize_ref_outcome(theme, roots, flags, &plan.table, spec, name_w, RowStyle::Ready, None) {
            RefOutcome::Realized(entry, _state, _line, lease) => {
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
                    if let Ok(value) = std::fs::read_to_string(std::path::Path::new(&entry.out).join(file)) {
                        let value = value.trim();
                        if !value.is_empty() {
                            if let Some(value) = resolve_provider_paths(&entry.out, file, value) {
                                provider_vars.entry(variable.to_string()).or_default().push(value);
                            } else {
                                invalid_metadata.get_or_insert(file);
                            }
                        }
                    }
                }
                if let Some(file) = invalid_metadata {
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
                realized_refs.push(entry.reference);
                cache_leases.push(lease);
            }
            RefOutcome::NeedsNix(need) => holes.push(need),
            RefOutcome::Failed => failed = true,
        }
    }
    for (idx, adapter) in plan.adapters.iter().enumerate() {
        if total_steps > 1 {
            theme.progress_chain(
                "adapt",
                plan.refs.len() + idx + 1,
                total_steps,
                &adapter.name,
                "adapter",
            );
        }
        match realize_adapter(theme, roots, flags, adapter, true) {
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
        report_nix_bridge_required(theme, flags, &holes, &realized_refs);
        return Err(2);
    }
    if failed {
        return Err(1);
    }
    let mut composed_vars: std::collections::BTreeMap<String, String> =
        provider_vars
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
    if let Some(profile) = &plan.environment.selected_profile {
        theme.detail(&format!("profile: {}", profile.applied.join(" -> ")));
        composed_vars.extend(profile.variables.clone());
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
            theme.error(
                "couldn't load dotenv file",
                &format!("`{path}` is not a project-relative path"),
                "keep dotenv files inside the project and remove absolute or `..` paths.",
            );
            return Err(2);
        }
        let dotenv_path = std::env::current_dir().unwrap_or_default().join(relative);
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
                theme.error(
                    "couldn't load dotenv file",
                    &format!("{}: {error}", dotenv_path.display()),
                    "fix the dotenv path and keep each assignment in KEY=value form.",
                );
                return Err(2);
            }
        }
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

fn validate_integration_facts(plan: &RunPlan) -> Result<(), String> {
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
        if !plan.environment.integration_facts.tasks.contains(&task.name) {
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
            if !plan.secrets.iter().any(|declared| declared == secret) {
                return Err(format!(
                    "integration task `{}` lost secret `{secret}` before activation",
                    task.name
                ));
            }
        }
        let expected_provider = match task.integration {
            ModuleEval::IntegrationKind::Android
            | ModuleEval::IntegrationKind::Apple
            | ModuleEval::IntegrationKind::Editor => "nixpkgs",
            ModuleEval::IntegrationKind::Certificates | ModuleEval::IntegrationKind::Vault => "vault",
            ModuleEval::IntegrationKind::CloudCredentials => "credential-store",
            ModuleEval::IntegrationKind::Hosts => "host-binding",
            ModuleEval::IntegrationKind::CodexAgent => "mcp",
        };
        if !task.providers.iter().any(|provider| provider == expected_provider) {
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
            if !plan.environment.integration_facts.providers.contains(provider) {
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
            validate_integration_host_check(check, &target)?;
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
    use super::resolve_provider_paths;

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
}

#[cfg(test)]
pub(crate) fn compose_refs_for_test(roots: &Roots, refs: Vec<RefSpec::RefSpec>) -> Result<Env, i32> {
    let parsed = super::parse::parse_args_for("", &[]);
    compose_env(
        &Theme::resolve(true),
        roots,
        &parsed.flags,
        &RunPlan {
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
fn select_request_from_flags(flags: &Flags) -> SelectRequest {
    SelectRequest {
        packages: flags.workspace_members.clone(),
        affected: flags.affected,
        affected_since: flags.affected_since.clone(),
    }
}

fn report_select_error(theme: &Theme, d: &crate::Diagnostics::Diagnostic) -> i32 {
    theme.error_coded(&d.code, &d.what, &d.why, &d.fix);
    2
}

/// Build (or test) each selected workspace member via the core provider.
fn run_workspace_members(
    theme: &Theme,
    parsed: &Parsed,
    dir: &std::path::Path,
    plan_members: &[WorkspaceMember],
    action: &str,
) -> i32 {
    let roots = Store::resolve();
    let mut ok = true;
    let mut built: Vec<WorkspaceMember> = Vec::new();
    let ordered_members = MemberSelect::dependency_order(dir, plan_members);
    for (idx, member) in ordered_members.iter().enumerate() {
        let abs = if std::path::Path::new(&member.path).is_absolute() {
            std::path::PathBuf::from(&member.path)
        } else {
            dir.join(&member.path)
        };
        theme.status(&format!("{action} workspace member: {}", member.name));
        let table = RefSpec::SourceTable::from_decls([(
            member.name.clone(),
            format!("path:{}", abs.display()),
            ProviderKind::Core,
        )]);
        let raw = format!("{}@{}", member.name, member.name);
        if ordered_members.len() > 1 {
            theme.progress_chain(
                action,
                idx + 1,
                ordered_members.len(),
                &member.name,
                "workspace",
            );
        }
        let spec = match RefSpec::classify_in(&raw, &table) {
            Ok(s) => s,
            Err(e) => {
                Output::ref_error(theme, &e);
                ok = false;
                continue;
            }
        };
        if realize_ref(
            theme,
            &roots,
            &parsed.flags,
            &table,
            &spec,
            member.name.len().max(8),
        )
        .is_none()
        {
            ok = false;
        } else {
            built.push(member.clone());
        }
    }
    if ok {
        MemberSelect::record_member_input_hashes(dir, &built);
        theme.status(&format!(
            "{action} {} workspace member(s).",
            ordered_members.len()
        ));
        0
    } else {
        1
    }
}

/// `jetpack build [<ref>]` — realize without entering a shell.
pub(super) fn cmd_build(theme: &Theme, parsed: &Parsed) -> i32 {
    let roots = Store::resolve();
    let dir = std::env::current_dir().unwrap_or_default();
    if let Err(code) = RuntimePolicy::enforce_sandbox_policy(theme, parsed.flags.json) {
        return code;
    }

    // D-WORKSPACE1=B: if workspace.jet is present, build selected workspace
    // members via the first-party core provider (no Nix required).
    if dir.join(Syntax::WORKSPACE_FILE).exists() {
        if let Some(result) = load_workspace(&dir) {
            return match result {
                Err(code) => code,
                Ok(plan) => {
                    let req = select_request_from_flags(&parsed.flags);
                    let selected = match MemberSelect::select_members(&dir, &plan, &req) {
                        Ok(m) => m,
                        Err(d) => return report_select_error(theme, &d),
                    };
                    if selected.is_empty() {
                        theme.status("no workspace members matched the selection.");
                        return 0;
                    }
                    run_workspace_members(theme, parsed, &dir, &selected, "building")
                }
            };
        }
    }

    let mut plan = match parsed.positional.first() {
        Some(raw) => match classify_or_report(theme, raw) {
            Ok(s) => RunPlan {
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
    if let Err(code) = apply_locked_channels(theme, &dir, &mut plan.table) {
        return code;
    }

    let mut ok = true;
    let (mut built, mut cached, mut substituted) = (0usize, 0usize, 0usize);
    let mut realized_refs = Vec::new();
    let mut holes = Vec::new();
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
        ) {
            RefOutcome::Realized(entry, state, line, _lease) => {
                if live_mode {
                    live.finish(&line);
                }
                realized_refs.push(entry.reference);
                completed_steps += 1;
                match state {
                    Provider::SourceState::Built => built += 1,
                    Provider::SourceState::Cached => cached += 1,
                    Provider::SourceState::Substituted => substituted += 1,
                }
            }
            RefOutcome::NeedsNix(need) => holes.push(need),
            RefOutcome::Failed => ok = false,
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
        match realize_adapter(theme, &roots, &parsed.flags, adapter, false) {
            Some((entry, state, _lease)) => {
                realized_refs.push(entry.reference);
                completed_steps += 1;
                match state {
                    Provider::SourceState::Built => built += 1,
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
        auto_clean_after_success(theme, &roots);
        0
    } else {
        // Failure path: region already cleared before each diagnostic inside
        // `realize_ref_outcome`; force one last erase so a stale bar cannot
        // survive past the process exit (D-FE-CLI1 still 8 / hybrid.html).
        if live_mode {
            live.clear();
        }
        1
    }
}

/// `jetpack test` — realize selected workspace members (D-JPK-SELECTOR1=C).
/// Outside a workspace, falls through to the same project-plan realize path as
/// `build` (tests ride the package after realize).
pub(super) fn cmd_test(theme: &Theme, parsed: &Parsed) -> i32 {
    let dir = std::env::current_dir().unwrap_or_default();
    if let Err(code) = RuntimePolicy::enforce_sandbox_policy(theme, parsed.flags.json) {
        return code;
    }
    if dir.join(Syntax::WORKSPACE_FILE).exists() {
        if let Some(result) = load_workspace(&dir) {
            return match result {
                Err(code) => code,
                Ok(plan) => {
                    let req = select_request_from_flags(&parsed.flags);
                    let selected = match MemberSelect::select_members(&dir, &plan, &req) {
                        Ok(m) => m,
                        Err(d) => return report_select_error(theme, &d),
                    };
                    if selected.is_empty() {
                        theme.status("no workspace members matched the selection.");
                        return 0;
                    }
                    let names: Vec<_> = selected.iter().map(|m| m.name.as_str()).collect();
                    theme.status(&format!("running {} members: {}", names.len(), names.join(", ")));
                    run_workspace_members(theme, parsed, &dir, &selected, "testing")
                }
            };
        }
    }
    // Non-workspace: identical realize path to build.
    cmd_build(theme, parsed)
}
