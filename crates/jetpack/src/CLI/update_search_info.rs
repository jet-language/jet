use super::parse::Parsed;
use super::realize::{
    channel_download_size_from_fixture, channel_sources, load_project_plan, offline_refusal,
    report_provider_error, resolve_source_channel, rewrite_channel_manifest,
};
use super::workspace_sources::{fixtures_for, project_root, workspace_root};
use crate::Output::{self, Theme};
use crate::Store::{self, ExplainLens, Roots};
use crate::{BuildDebug, Discovery, EnvFile, Lock, Overlay, SemanticLock, Syntax, WorkspaceFile};
use std::path::{Path, PathBuf};

pub(super) fn render_channel_update_row(
    theme: &Theme,
    scope: &str,
    name: &str,
    name_w: usize,
    before: &str,
    after: &str,
    package_count: usize,
    download_bytes: Option<u64>,
) {
    let after = format!(
        "{after}   {}, {}",
        package_noun(package_count),
        download_size(download_bytes),
    );
    let label = format!("{scope:<5} {name:<name_w$}");
    theme.plan_row(
        Output::PlanMark::Change,
        &label,
        label.len(),
        before,
        &after,
    );
}

#[derive(Default)]
pub(super) struct UpdateConfirmation {
    decision: Option<bool>,
}

impl UpdateConfirmation {
    pub(super) fn confirm(&mut self, theme: &Theme, assume_yes: bool) -> bool {
        *self
            .decision
            .get_or_insert_with(|| theme.confirm_apply(assume_yes))
    }
}

struct ProjectSourceUpdate {
    source: super::realize::ChannelSource,
    before: String,
    after: String,
    package_count: usize,
    download_bytes: Option<u64>,
}

pub(super) struct ProjectUpdatePlan {
    project_dir: PathBuf,
    updates: Vec<ProjectSourceUpdate>,
}

fn package_noun(count: usize) -> String {
    format!(
        "{count} {}",
        if count == 1 { "package" } else { "packages" }
    )
}

fn download_size(bytes: Option<u64>) -> String {
    bytes
        .map(|bytes| {
            Theme::render_download_line(bytes)
                .trim_start_matches("Download ")
                .to_string()
        })
        .unwrap_or_else(|| "size unknown".to_string())
}

/// `jetpack update` — select project dependency and/or user-tool scopes from
/// the current directory context, then refresh only those scopes.
pub(super) fn cmd_update(theme: &Theme, parsed: &Parsed) -> i32 {
    let mut confirmation = UpdateConfirmation::default();
    let (update_deps, update_tools) = update_scopes(parsed);
    let project_scope = project_scope_root();
    if update_deps && update_tools {
        if let Some(root) = &project_scope {
            theme.status(&format!("scope: project dependencies ({})", root.display()));
        }
        theme.status("scope: user tools");
    }
    if update_deps && update_tools {
        let project_plan = if project_scope.is_some() {
            match plan_project_update(theme, parsed) {
                Ok(plan) => plan,
                Err(code) => return code,
            }
        } else {
            None
        };
        if let Some(plan) = &project_plan {
            theme.status("Plan channel update");
            render_project_update_plan(theme, plan);
        }
        let has_project_plan = project_plan.is_some();
        let mut project_plan = project_plan;
        return super::tool::update_user_tools_with_plan(
            theme,
            parsed,
            &mut confirmation,
            has_project_plan,
            || {
                project_plan
                    .take()
                    .map_or(0, |plan| apply_project_update(theme, plan))
            },
        );
    }
    if update_tools {
        return super::tool::update_user_tools(theme, parsed, &mut confirmation);
    }
    cmd_update_project(theme, parsed, &mut confirmation)
}

fn update_scopes(parsed: &Parsed) -> (bool, bool) {
    if parsed.flags.update_deps || parsed.flags.update_tools {
        return (parsed.flags.update_deps, parsed.flags.update_tools);
    }
    match parsed.positional.first().map(String::as_str) {
        Some(Syntax::TOOL_PROFILE_NAME) => (false, true),
        Some(_) => (true, false),
        None => (true, true),
    }
}

fn project_scope_root() -> Option<PathBuf> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let root = project_root(&cwd);
    EnvFile::path_in(&root).is_file().then_some(root)
}

/// `jetpack update --deps` / `jetpack update <source>` — resolve project
/// channel source refs and move only their lock entries. Does not realize packages.
fn cmd_update_project(
    theme: &Theme,
    parsed: &Parsed,
    confirmation: &mut UpdateConfirmation,
) -> i32 {
    let plan = match plan_project_update(theme, parsed) {
        Ok(Some(plan)) => plan,
        Ok(None) => return 0,
        Err(code) => return code,
    };
    theme.status("Plan channel update");
    render_project_update_plan(theme, &plan);
    if !confirmation.confirm(theme, parsed.flags.assume_yes) {
        return 2;
    }
    apply_project_update(theme, plan)
}

pub(super) fn plan_project_update(
    theme: &Theme,
    parsed: &Parsed,
) -> Result<Option<ProjectUpdatePlan>, i32> {
    if parsed.flags.offline {
        return Err(offline_refusal(theme, "update"));
    }
    let plan = match load_project_plan(theme) {
        Ok(plan) => plan,
        Err(code) => return Err(code),
    };
    let project_dir = &plan.project_root;
    let only = parsed.positional.first().map(String::as_str);
    let sources = channel_sources(&plan.table);
    if let Some(name) = only {
        if plan
            .table
            .declared_names()
            .iter()
            .any(|declared| declared == name)
            && !sources.iter().any(|source| source.name == name)
        {
            theme.error_coded(
                "E1352",
                &format!("source `{name}` is pinned and cannot be updated"),
                "a pinned source has no moving channel policy, so Jetpack must never change its lock or manifest",
                &format!(
                    "declare `{name}` with `#latest` for manual movement or `#auto` for automatic movement"
                ),
            );
            return Err(2);
        }
    }
    let selected: Vec<_> = sources
        .into_iter()
        .filter(|s| only.is_none_or(|name| name == s.name))
        .collect();
    if selected.is_empty() {
        match only {
            Some(name) => theme.error(
                &format!("no channel source named `{name}`"),
                "only sources declared with `#latest`, `#main`, or `#vN.x` can be updated.",
                "run `jetpack outdated` to see channel sources.",
            ),
            None => theme.status("no channel sources to update."),
        }
        return if only.is_some() { Err(2) } else { Ok(None) };
    }

    let mut ok = true;
    let mut updates = Vec::new();
    for source in selected {
        match resolve_source_channel(&source, &parsed.flags) {
            Ok(exact) => {
                let before = Lock::locked_source_channel(&project_dir, &source.name)
                    .map(|lock| lock.exact)
                    .unwrap_or_else(|| "unlocked".to_string());
                let package_count = plan
                    .refs
                    .iter()
                    .filter(|spec| spec.source.label() == source.name)
                    .count();
                let download_bytes = channel_download_size_from_fixture(&source, &parsed.flags);
                updates.push(ProjectSourceUpdate {
                    source,
                    before,
                    after: exact,
                    package_count,
                    download_bytes,
                });
            }
            Err(e) => {
                report_provider_error(theme, &e);
                ok = false;
            }
        }
    }
    if !ok {
        return Err(2);
    }
    Ok(Some(ProjectUpdatePlan {
        project_dir: plan.project_root,
        updates,
    }))
}

pub(super) fn render_project_update_plan(theme: &Theme, plan: &ProjectUpdatePlan) {
    let name_w = plan
        .updates
        .iter()
        .map(|update| update.source.name.len())
        .max()
        .unwrap_or(0)
        .max(8);
    for update in &plan.updates {
        render_channel_update_row(
            theme,
            "deps",
            &update.source.name,
            name_w,
            &update.before,
            &update.after,
            update.package_count,
            update.download_bytes,
        );
    }
    let download_bytes = plan
        .updates
        .iter()
        .map(|update| update.download_bytes)
        .collect::<Option<Vec<_>>>()
        .map(|sizes| sizes.into_iter().sum());
    if let Some(bytes) = download_bytes {
        theme.download_line(bytes);
    }
}

pub(super) fn apply_project_update(theme: &Theme, plan: ProjectUpdatePlan) -> i32 {
    let ProjectUpdatePlan {
        project_dir,
        updates,
    } = plan;
    let mut ok = true;
    for update in updates {
        let source = update.source;
        if let Err(error) = rewrite_channel_manifest(&project_dir, &source, &update.after) {
            theme.error_coded(
                "E1340",
                &format!("channel `{}` could not update the manifest", source.name),
                &error,
                "fix the manifest permissions and run the command again",
            );
            ok = false;
            continue;
        }
        Lock::record_source_channel(
            &project_dir,
            Lock::LockedSourceChannel {
                name: source.name.clone(),
                channel: source.channel.as_str().to_string(),
                exact: update.after.clone(),
            },
        );
        theme.status(&format!(
            "{} {} → {}",
            theme.bold(&source.name),
            theme.gray(source.channel.as_str()),
            update.after
        ));
    }
    if ok {
        0
    } else {
        2
    }
}

/// `jetpack outdated` — read-only channel freshness report. It may query
/// metadata, but never writes `.jet/lock`.
pub(super) fn cmd_outdated(theme: &Theme, parsed: &Parsed) -> i32 {
    if parsed.flags.offline {
        return offline_refusal(theme, "outdated");
    }
    let project_dir = std::env::current_dir().unwrap_or_default();
    let plan = match load_project_plan(theme) {
        Ok(plan) => plan,
        Err(code) => return code,
    };
    let sources = channel_sources(&plan.table);
    let declarations = plan.table.declarations();
    if declarations.is_empty() {
        theme.status("no declared sources.");
        return 0;
    }
    let mut any = false;
    let mut ok = true;
    for (name, _, _) in declarations {
        let policy = plan.table.channel_policy(&name);
        let Some(source) = sources.iter().find(|source| source.name == name) else {
            theme.detail(&format!(
                "{}  {}  never moves",
                theme.bold(&name),
                theme.gray(policy.as_str())
            ));
            continue;
        };
        let locked = Lock::locked_source_channel(&project_dir, &source.name);
        let Some(locked) = locked else {
            theme.detail(&format!(
                "{}  {}  unlocked (run `jetpack update {}`)",
                theme.bold(&source.name),
                theme.gray(policy.as_str()),
                source.name
            ));
            any = true;
            continue;
        };
        match resolve_source_channel(source, &parsed.flags) {
            Ok(latest) if latest != locked.exact => {
                any = true;
                theme.detail(&format!(
                    "{}  {}  {} → {}",
                    theme.bold(&source.name),
                    theme.gray(policy.as_str()),
                    locked.exact,
                    latest
                ));
            }
            Ok(_) => theme.detail(&format!(
                "{}  {}  current",
                theme.bold(&source.name),
                theme.gray(policy.as_str())
            )),
            Err(e) => {
                report_provider_error(theme, &e);
                ok = false;
            }
        }
    }
    if ok && !any {
        theme.status("all channel sources are current.");
    }
    if ok {
        0
    } else {
        2
    }
}

/// `jet search <query>` — local/offline package discovery (U26).
pub(super) fn cmd_search(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(query) = parsed.positional.first() else {
        theme.error(
            "search needs a query",
            "`jet search` reads the local discovery index; it never fetches.",
            "write `jet search postgres`.",
        );
        return 2;
    };
    let index = match discovery_index(theme, parsed) {
        Ok(index) => index,
        Err(code) => return code,
    };
    let records = index.search(query);
    if parsed.flags.json {
        println!("{}", Discovery::search_json(&records));
        return 0;
    }
    if records.is_empty() {
        println!("no packages found for `{query}`");
        if let Some(nearest) = index.nearest(query) {
            println!("nearest: {nearest}");
        }
        return 1;
    }
    for record in records {
        println!(
            "{:<24} {:<10} {}",
            record.display_ref(),
            empty_dash(&record.version),
            record.platforms.join(", ")
        );
    }
    0
}

/// `jet info <ref>` — local/offline package metadata (U26).
pub(super) fn cmd_info(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(query) = parsed.positional.first() else {
        theme.error(
            "info needs a package ref",
            "`jet info` reads the local discovery index; it never fetches.",
            "write `jet info default.ripgrep`.",
        );
        return 2;
    };
    let index = match discovery_index(theme, parsed) {
        Ok(index) => index,
        Err(code) => return code,
    };
    let Some(record) = index.info(query) else {
        let fix = index
            .nearest(query)
            .map(|n| format!("try `jet info {n}`."))
            .unwrap_or_else(|| "run `jet search <name>` to see local matches.".to_string());
        theme.error(
            &format!("no local package info for `{query}`"),
            "`jet info` uses only the local discovery index.",
            &fix,
        );
        return 2;
    };
    if parsed.flags.json {
        println!("{}", Discovery::info_json(record));
        return 0;
    }
    println!("{}", record.display_ref());
    println!("  ref: {}", record.reference);
    println!("  version: {}", empty_dash(&record.version));
    println!("  platforms: {}", record.platforms.join(", "));
    println!("  source: {}", record.source);
    println!("  provenance: {}", record.provenance);
    println!("  tier: {}", record.tier);
    println!("  maintainer liveness: {}", record.maintainer_liveness());
    println!("  gate status: {}", record.gate_status);
    if !record.options.is_empty() {
        println!("  service options:");
        for opt in &record.options {
            println!(
                "    {:<10} default: {:<24} {}",
                opt.name,
                empty_dash(&opt.default),
                opt.docs
            );
        }
    }
    0
}

pub(super) fn cmd_explain(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(query) = parsed.positional.first() else {
        return explain_error(
            theme,
            parsed,
            "E1274",
            "explain needs a package ref",
            "`jet explain <CODE>` is handled by the main compiler; `jetpack explain` explains package refs.",
            "write `jet explain weirdctl` after a failed build.",
        );
    };
    if query.starts_with("package-overlay:") {
        return cmd_explain_overlay(theme, parsed, query);
    }
    if Syntax::lookup(query).is_some() {
        let explanation = jet_cli::Explain::lookup(query)
            .expect("Syntax::lookup and Explain::lookup must share the dictionary");
        if parsed.flags.json {
            let optional = |value: Option<&String>| {
                value
                    .map(|value| crate::JSON::quote(value))
                    .unwrap_or_else(|| "null".to_string())
            };
            println!(
                "{}",
                jet_foundation::Report::render_status_json(
                    "ok",
                    true,
                    "explain",
                    &format!(
                        ",\"code\":{},\"stage\":{},\"what\":{},\"why\":{},\"fix\":{},\"example\":{}",
                        crate::JSON::quote(&explanation.code),
                        crate::JSON::quote(&explanation.stage),
                        crate::JSON::quote(
                            explanation
                                .what
                                .as_deref()
                                .unwrap_or(explanation.meaning.as_str())
                        ),
                        optional(explanation.why.as_ref()),
                        optional(explanation.fix.as_ref()),
                        optional(explanation.example.as_ref()),
                    ),
                )
            );
        } else {
            print!("{}", jet_cli::Explain::render(&explanation, theme.color));
        }
        return 0;
    }
    if Syntax::looks_like_query(query) {
        let closest = Syntax::nearest(query)
            .map(Syntax::display)
            .unwrap_or_else(|| "a registered syntax token".to_string());
        let diagnostic = jet_foundation::Registry::diagnostic("E2106")
            .expect("E2106 is registered for unknown syntax-token explanations");
        let rendered = diagnostic.render(&[("token", query), ("closest", closest.as_str())]);
        return explain_error(
            theme,
            parsed,
            diagnostic.code,
            &rendered.what,
            &rendered.why,
            &rendered.fix,
        );
    }
    let (lens, package) = match ExplainLens::parse(query) {
        Some(lens) => {
            let Some(package) = parsed.positional.get(1) else {
                return explain_error(
                    theme,
                    parsed,
                    "E1274",
                    &format!("explain lens `{query}` needs a package ref"),
                    "causal package explanations need the lens and the package ref.",
                    &format!("write `jet explain {query} <ref>`."),
                );
            };
            (lens, package.as_str())
        }
        None => (ExplainLens::All, query.as_str()),
    };
    let roots = Store::resolve();
    match Store::explain_package(&roots, package, lens) {
        Ok(Some(explanation)) => {
            if parsed.flags.json {
                println!("{}", explanation.to_json());
            } else {
                print!("{}", explanation.text());
            }
            if explanation.reports.iter().any(|report| report.kind == "conflict") {
                2
            } else {
                0
            }
        }
        Ok(None) => {
            explain_error(
                theme,
                parsed,
                "E1274",
                &format!("no package record or build attempt for `{package}`"),
                "`jet explain` reads the Hangar Store and persisted Jetpack realization attempts; neither contains this package.",
                &format!("run `jet build {package}` first, or use `jet explain E1234` for diagnostic-code help."),
            )
        }
        Err(error) => {
            explain_error(
                theme,
                parsed,
                "E1274",
                "couldn't read package explanation",
                &error.to_string(),
                "repair the Hangar closure/lock record, then retry the explanation.",
            )
        }
    }
}

fn explain_error(
    theme: &Theme,
    parsed: &Parsed,
    code: &str,
    what: &str,
    why: &str,
    fix: &str,
) -> i32 {
    if parsed.flags.json {
        let diagnostic = jet_foundation::Diagnostics::Diagnostic::error(
            code,
            what.to_string(),
            why.to_string(),
            fix.to_string(),
            None,
        );
        print!(
            "{}",
            jet_foundation::Diagnostics::render_all_json(
                &jet_foundation::Diagnostics::ReportPath::from_process(""),
                "",
                &[diagnostic],
            )
        );
    } else {
        theme.error_coded(code, what, why, fix);
    }
    2
}

fn cmd_explain_overlay(theme: &Theme, parsed: &Parsed, query: &str) -> i32 {
    let dir = std::env::current_dir().unwrap_or_default();
    let Some(source) = WorkspaceFile::resolve_workspace_source(&dir) else {
        return explain_error(
            theme,
            parsed,
            "E1274",
            &format!("no overlay policy for `{query}`"),
            "`package-overlay:*` explanations come from reviewed `workspace.jet` overlay policy.",
            "run `jetpack override draft <ref> --patch <file>` or add an `overlay` block to `workspace.jet`.",
        );
    };
    let result = match source {
        Ok(source) => WorkspaceFile::evaluate_source(&source.source, &dir, source.role),
        Err(diagnostic) => Err(diagnostic),
    };
    let plan = match result {
        Ok(plan) => plan,
        Err(d) => {
            if parsed.flags.json {
                print!(
                    "{}",
                    crate::Diagnostics::render_all_json(
                        &crate::Diagnostics::ReportPath::from_process(Syntax::WORKSPACE_FILE),
                        "",
                        std::slice::from_ref(&d),
                    )
                );
            } else {
                eprint!(
                    "{}",
                    crate::Diagnostics::render_all(
                        Syntax::WORKSPACE_FILE,
                        "",
                        std::slice::from_ref(&d)
                    )
                );
            }
            return 2;
        }
    };
    let records =
        match Overlay::semantic_records(&plan.overlay_policy, "workspace", std::env::consts::OS) {
            Ok(records) => records,
            Err(error) => {
                return explain_error(
                    theme,
                    parsed,
                    "E0998",
                    "workspace overlay policy is malformed",
                    &error.message(),
                    "fix the conflicting overlay facts in `workspace.jet`.",
                );
            }
        };
    let lock = SemanticLock::SemanticLockFile {
        records,
        ..Default::default()
    };
    let Some(fact) = SemanticLock::explain(&lock, query) else {
        return explain_error(
            theme,
            parsed,
            "E1274",
            &format!("no overlay record for `{query}`"),
            "`workspace.jet` has overlay policy, but not that overlay/package key.",
            "query `package-overlay:<overlay>:<package>`.",
        );
    };
    if parsed.flags.json {
        let owners = fact
            .owners
            .iter()
            .map(|owner| crate::JSON::quote(owner))
            .collect::<Vec<_>>()
            .join(",");
        let contenders = fact
            .contenders
            .iter()
            .map(|contender| {
                format!(
                    "{{\"owner\":{},\"provider\":{},\"exact\":{},\"reason\":{},\"source\":{},\"channel\":{},\"policy\":{},\"recipe\":{},\"adapter\":{},\"signature\":{},\"cache_provenance\":{},\"update\":{}}}",
                    crate::JSON::quote(&contender.owner_package),
                    crate::JSON::quote(&contender.provider),
                    crate::JSON::quote(&contender.exact_output),
                    crate::JSON::quote(&contender.reason),
                    crate::JSON::quote(&contender.source_ref),
                    crate::JSON::quote(&contender.channel_input),
                    crate::JSON::quote(&contender.policy_fingerprint),
                    crate::JSON::quote(&contender.recipe_id),
                    crate::JSON::quote(&contender.adapter_id),
                    crate::JSON::quote(&contender.signature),
                    crate::JSON::quote(&contender.cache_provenance),
                    crate::JSON::quote(&contender.update_command),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{}",
            jet_foundation::Report::render_status_json(
                "ok",
                true,
                "explain",
                &format!(
                    ",\"query\":{},\"lens\":\"overlay\",\"semantic_key\":{},\"owners\":[{}],\"contenders\":[{}],\"provider\":{},\"platform\":{},\"exact\":{},\"policy\":{},\"update\":{},\"offline\":{}",
                    crate::JSON::quote(query),
                    crate::JSON::quote(&fact.semantic_key),
                    owners,
                    contenders,
                    crate::JSON::quote(&fact.provider),
                    crate::JSON::quote(&fact.platform),
                    crate::JSON::quote(&fact.exact_artifact),
                    crate::JSON::quote(&fact.policy_fingerprint),
                    crate::JSON::quote(&fact.update_command),
                    fact.offline_satisfied,
                ),
            )
        );
        return 0;
    }
    println!("{}", fact.semantic_key);
    println!("  owners: {}", fact.owners.join(", "));
    println!("  winner: {}", empty_dash(&fact.exact_artifact));
    for (index, contender) in fact.contenders.iter().enumerate() {
        println!(
            "  contender[{index}]: owner={} provider={} exact={} reason={}",
            empty_dash(&contender.owner_package),
            empty_dash(&contender.provider),
            empty_dash(&contender.exact_output),
            empty_dash(&contender.reason),
        );
    }
    println!("  provider: {}", empty_dash(&fact.provider));
    println!("  platform: {}", empty_dash(&fact.platform));
    println!("  exact: {}", empty_dash(&fact.exact_artifact));
    println!("  policy: {}", empty_dash(&fact.policy_fingerprint));
    println!("  update: {}", empty_dash(&fact.update_command));
    println!("  offline: {}", fact.offline_satisfied);
    0
}

pub(super) fn cmd_override(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(action) = parsed.positional.first().map(String::as_str) else {
        theme.error(
            "override needs an action",
            "`jetpack override` only drafts reviewed source policy; it never records hidden override state.",
            "write `jetpack override draft <ref> --patch <file>`.",
        );
        return 2;
    };
    if action != "draft" {
        theme.error(
            &format!("unknown override action `{action}`"),
            "`draft` is the only supported override action.",
            "write `jetpack override draft <ref> --patch <file>`.",
        );
        return 2;
    }
    let Some(reference) = parsed.positional.get(1) else {
        theme.error(
            "override draft needs a package ref",
            "the ref names the package whose typed workspace policy should be drafted.",
            "write `jetpack override draft foo@nixpkgs --patch patches/foo.patch`.",
        );
        return 2;
    };
    let mut overlay = "local".to_string();
    let mut patch = None::<String>;
    let mut provider = None::<String>;
    let mut channel = None::<String>;
    let mut allow_unfree = false;
    let mut i = 2usize;
    while i < parsed.positional.len() {
        match parsed.positional[i].as_str() {
            "--overlay" => {
                i += 1;
                overlay = parsed.positional.get(i).cloned().unwrap_or_default();
            }
            "--patch" => {
                i += 1;
                patch = parsed.positional.get(i).cloned();
            }
            "--provider" => {
                i += 1;
                provider = parsed.positional.get(i).cloned();
            }
            "--channel" => {
                i += 1;
                channel = parsed.positional.get(i).cloned();
            }
            "--allow-unfree" => allow_unfree = true,
            other => {
                theme.error(
                    &format!("unknown override draft flag `{other}`"),
                    "override drafts accept `--overlay`, `--provider`, `--channel`, `--patch`, and `--allow-unfree`.",
                    "write `jetpack override draft foo@nixpkgs --patch patches/foo.patch`.",
                );
                return 2;
            }
        }
        i += 1;
    }
    if overlay.trim().is_empty() {
        theme.error(
            "override draft needs a non-empty overlay name",
            "`workspace.jet` stores overrides in named overlay sets.",
            "pass `--overlay local` or another source-reviewed name.",
        );
        return 2;
    }
    let package = reference
        .rsplit_once(Syntax::REF_PROVIDER_AT)
        .map(|(package, _)| package)
        .unwrap_or(reference)
        .to_string();
    let workspace = workspace_root(&std::env::current_dir().unwrap_or_default());
    let path = match WorkspaceFile::resolve_workspace_source(&workspace) {
        Some(Ok(source)) => source.path,
        Some(Err(diagnostic)) => {
            eprint!(
                "{}",
                crate::Diagnostics::render_all(
                    &workspace.display().to_string(),
                    "",
                    std::slice::from_ref(&diagnostic),
                )
            );
            return 2;
        }
        None => workspace.join(Syntax::WORKSPACE_FILE),
    };
    let existing = std::fs::read_to_string(&path).ok();
    let next = Overlay::draft_overlay_source(
        existing.as_deref(),
        &overlay,
        &package,
        patch.as_deref(),
        provider.as_deref(),
        channel.as_deref(),
        allow_unfree,
    );
    if let Err(e) = std::fs::write(&path, next) {
        theme.error(
            "could not write workspace overlay policy",
            &format!("writing `{}` failed: {e}", path.display()),
            "check permissions and retry.",
        );
        return 2;
    }
    theme.ok(&format!("drafted overlay `{overlay}` for `{package}`"));
    theme.detail(&format!(
        "wrote reviewed source policy to {}",
        path.display()
    ));
    0
}

pub(super) fn cmd_logs(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(package) = parsed.positional.first() else {
        theme.error(
            "logs needs a package name",
            "`jet logs` prints the latest recorded build attempt for one package.",
            "write `jet logs weirdctl`.",
        );
        return 2;
    };
    let roots = Store::resolve();
    if parsed.flags.json {
        match BuildDebug::latest_json(&roots.hangar_dir(), package) {
            Ok(Some(json)) => {
                print!("{json}");
                0
            }
            Ok(None) => missing_logs(theme, package),
            Err(e) => read_logs_error(theme, &e),
        }
    } else {
        match BuildDebug::latest(&roots.hangar_dir(), package) {
            Ok(Some(attempt)) => {
                print!("{}", BuildDebug::text_logs(&attempt));
                0
            }
            Ok(None) => missing_logs(theme, package),
            Err(e) => read_logs_error(theme, &e),
        }
    }
}

fn missing_logs(theme: &Theme, package: &str) -> i32 {
    theme.error_coded(
        "E1274",
        &format!("no build log for `{package}`"),
        "`jet logs` reads persisted Jetpack realization attempts; no attempt is recorded for that package.",
        &format!("run `jet build {package}` first."),
    );
    2
}

fn read_logs_error(theme: &Theme, reason: &str) -> i32 {
    theme.error_coded(
        "E1274",
        "couldn't read build logs",
        reason,
        "check the build log directory under the Jetpack hangar.",
    );
    2
}

pub(super) fn shell_on_failed_build(theme: &Theme, roots: &Roots, package: &str) {
    let Ok(Some(attempt)) = BuildDebug::latest(&roots.hangar_dir(), package) else {
        return;
    };
    if attempt.status != "failed" || attempt.scratch_dir.is_empty() {
        return;
    }
    let Some(scratch) = verified_failed_scratch(roots, &attempt, package) else {
        theme
            .detail("failed-build shell skipped: preserved scratch is not a real Hangar directory");
        return;
    };
    let shell = std::env::var("JETPACK_SHELL_ON_FAIL")
        .ok()
        .unwrap_or_else(|| {
            std::env::var("SHELL").unwrap_or_else(|_| {
                if cfg!(windows) {
                    "cmd".to_string()
                } else {
                    "sh".to_string()
                }
            })
        });
    theme.status(&format!(
        "build failed at step {} · shell in preserved build dir {}",
        attempt.failed_step,
        scratch.display()
    ));
    let mut cmd = std::process::Command::new(&shell);
    cmd.current_dir(&scratch)
        .env("JETPACK_FAILED_SCRATCH", &scratch)
        .env("JETPACK_FAILED_STEP", attempt.failed_step.to_string())
        .env("JETPACK_FAILED_PACKAGE", package);
    let _ = cmd.status();
}

fn verified_failed_scratch(
    roots: &Roots,
    attempt: &BuildDebug::Attempt,
    package: &str,
) -> Option<PathBuf> {
    let id = &attempt.id;
    let expected_prefix = format!("{}-", BuildDebug::safe_name(package));
    if attempt.package != package
        || !id.starts_with(&expected_prefix)
        || id.is_empty()
        || matches!(id.as_str(), "." | "..")
        || BuildDebug::safe_name(id) != id.as_str()
    {
        return None;
    }
    let hangar = roots.hangar_dir();
    let hangar_metadata = std::fs::symlink_metadata(&hangar).ok()?;
    if hangar_metadata.file_type().is_symlink() || !hangar_metadata.is_dir() {
        return None;
    }
    let root = hangar.join("failed-scratch");
    let root_metadata = std::fs::symlink_metadata(&root).ok()?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return None;
    }
    let recorded = Path::new(&attempt.scratch_dir);
    if !recorded.is_absolute() {
        return None;
    }
    let recorded_metadata = std::fs::symlink_metadata(recorded).ok()?;
    if recorded_metadata.file_type().is_symlink() || !recorded_metadata.is_dir() {
        return None;
    }
    let root = std::fs::canonicalize(root).ok()?;
    let recorded = std::fs::canonicalize(recorded).ok()?;
    let expected = root.join(id);
    (recorded == expected && recorded.starts_with(&root)).then_some(recorded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(args: &[&str]) -> (bool, bool) {
        let args = args.iter().map(|arg| (*arg).to_string()).collect::<Vec<_>>();
        update_scopes(&super::super::parse::parse_args_for("update", &args))
    }

    #[test]
    fn update_scope_flags_select_only_requested_context() {
        assert_eq!(scope(&[]), (true, true));
        assert_eq!(scope(&["--deps"]), (true, false));
        assert_eq!(scope(&["--tools"]), (false, true));
        assert_eq!(scope(&["--deps", "--tools"]), (true, true));
        assert_eq!(scope(&["tools"]), (false, true));
        assert_eq!(scope(&["default"]), (true, false));
    }

    #[test]
    fn failed_shell_rejects_recorded_scratch_escape() {
        let root = std::env::temp_dir().join(format!(
            "jet-failed-shell-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let roots = Roots {
            root: root.clone(),
            dev_mode: false,
        };
        std::fs::create_dir_all(roots.hangar_dir().join("failed-scratch/pkg-1")).unwrap();
        let outside = root.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let mut attempt = BuildDebug::Attempt::new("pkg", "pkg@1", "fixture", "recipe", "source");
        attempt.id = "pkg-1".to_string();
        attempt.scratch_dir = outside.to_string_lossy().into_owned();

        assert!(verified_failed_scratch(&roots, &attempt, "pkg").is_none());
        let _ = std::fs::remove_dir_all(root);
    }
}

fn discovery_index(theme: &Theme, parsed: &Parsed) -> Result<Discovery::Index, i32> {
    let project_dir = std::env::current_dir().unwrap_or_default();
    let mut index = match Discovery::load(&project_dir) {
        Ok(Some(index)) => index,
        Ok(None) => Discovery::Index::default(),
        Err(e) => {
            theme.error(
                "local discovery index is malformed",
                &e,
                "delete `.jet/discovery/index.jsonl` and rerun `jet search` from a project with env metadata.",
            );
            return Err(2);
        }
    };

    let roots = Store::resolve();
    let store_entries = Store::list(&roots);
    Discovery::merge_store_entries(&mut index, &store_entries);
    if let Some(lock) = Lock::load(&project_dir) {
        Discovery::merge_lock(&mut index, &lock);
    }

    if EnvFile::path_in(&project_dir).exists() {
        let plan = load_project_plan(theme)?;
        let fixtures = fixtures_for(&parsed.flags);
        Discovery::merge_refs(&mut index, &plan.refs, fixtures.as_deref(), &store_entries);
        Discovery::merge_adapters(&mut index, &plan.adapters);
        if let Err(e) = Discovery::write(&project_dir, &index) {
            theme.error(
                "couldn't write local discovery index",
                &e,
                "check permissions on `.jet/discovery/`.",
            );
            return Err(2);
        }
    }

    if index.is_empty() {
        theme.error(
            "no local discovery index",
            "`jet search` and `jet info` never fetch package metadata.",
            "run from a project with env metadata, or realize packages once so hangar metadata exists.",
        );
        return Err(2);
    }
    Ok(index)
}

fn empty_dash(value: &str) -> &str {
    if value.is_empty() {
        "-"
    } else {
        value
    }
}
