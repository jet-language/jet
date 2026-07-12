use super::package_hangar_vendor::auto_clean_after_success;
use super::parse::{Flags, Parsed};
use super::realize::{
    apply_locked_channels, classify_or_report, load_project_plan, realize_adapter, realize_ref,
    report_nix_bridge_required, realize_ref_outcome, RefOutcome, RowStyle, RunPlan,
};
use super::workspace_sources::{cwd_table, load_workspace};
use crate::ModuleEval;
use crate::Output::{self, Theme};
use crate::Provider;
use crate::RefSpec::{self, ProviderKind};
use crate::RuntimePolicy;
use crate::Shell::Env;
use crate::Store::{self, Roots};
use crate::Syntax;
use crate::Trust;

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
    let mut bin_dirs = Vec::new();
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
    // Tier 1 (D-FE-CLI1): the per-package `✓` rows above are the whole
    // report — `jet env`/`run`/`dev` hand off straight to the shell
    // threshold rule (`Shell::enter`) instead of a redundant summary line.
    Ok(Env {
        bin_dirs,
        refs: realized_refs,
        label: plan.label.clone(),
        prompt_path: plan.prompt_path,
        prompt_strip: plan.prompt_strip,
        cache_leases,
    })
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

/// `jetpack build [<ref>]` — realize without entering a shell.
pub(super) fn cmd_build(theme: &Theme, parsed: &Parsed) -> i32 {
    let roots = Store::resolve();
    let dir = std::env::current_dir().unwrap_or_default();
    if let Err(code) = RuntimePolicy::enforce_sandbox_policy(theme, parsed.flags.json) {
        return code;
    }

    // D-WORKSPACE1=B: if workspace.jet is present, build all workspace members
    // via the first-party core provider (no Nix required).
    if dir.join(Syntax::WORKSPACE_FILE).exists() {
        if let Some(result) = load_workspace(&dir) {
            return match result {
                Err(code) => code,
                Ok(plan) => {
                    let mut ok = true;
                    for (idx, member) in plan.members.iter().enumerate() {
                        let abs = if std::path::Path::new(&member.path).is_absolute() {
                            std::path::PathBuf::from(&member.path)
                        } else {
                            dir.join(&member.path)
                        };
                        theme.status(&format!("building workspace member: {}", member.name));
                        // Route the member through the core provider using its
                        // absolute local path as the upstream (source_repo handles
                        // "path:<abs>" → PathBuf directly, no Nix needed).
                        let table = RefSpec::SourceTable::from_decls([(
                            member.name.clone(),
                            format!("path:{}", abs.display()),
                            ProviderKind::Core,
                        )]);
                        let raw = format!("{}:{}", member.name, member.name);
                        if plan.members.len() > 1 {
                            theme.progress_chain(
                                "build",
                                idx + 1,
                                plan.members.len(),
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
                        }
                    }
                    if ok {
                        theme.status(&format!(
                            "built {} workspace member(s).",
                            plan.members.len()
                        ));
                        0
                    } else {
                        1
                    }
                    // (workspace members: state is printed per-package by realize_ref)
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
        1
    }
}
