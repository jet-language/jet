use super::parse::Flags;
use super::update_search_info::shell_on_failed_build;
use super::workspace_sources::{
    cwd_table, cwd_workspace_index, fixtures_for, project_root, reject_retired_jetpack_toml,
};
use crate::EnvFile;
use crate::Lock;
use crate::NixIndex::NixIndexClient;
use crate::Output::{self, Theme};
use crate::Provider::{self, ProviderError};
use crate::RefSpec::{self, RefError};
use crate::RuntimePolicy;
use crate::Services;
use crate::Store::{self, Roots};
use crate::Syntax;
use crate::Trust;
use jet_env_model::ModuleEval;
use std::path::{Path, PathBuf};

/// Classify an explicit CLI ref, accepting any named source declared in the
/// current project's env file so `jetpack use ripgrep@stable` works there, and
/// any workspace member so `jetpack use logging` / `jetpack use packages/logging`
/// resolve in a monorepo (Slice B, D-MONOREF1=A). Prints the diagnostic on failure.
pub(super) fn classify_or_report(theme: &Theme, raw: &str) -> Result<RefSpec::RefSpec, RefError> {
    let table = cwd_table();
    let index = cwd_workspace_index();
    match RefSpec::classify_with_workspace(raw, &table, &index) {
        Ok(spec) => Ok(spec),
        Err(RefError::MissingSeparator(_) | RefError::UnknownMember { .. })
            if !raw.contains(Syntax::REF_PROVIDER_AT) && !RefSpec::is_bare_path(raw) =>
        {
            RefSpec::classify_in(&RefSpec::with_default_source(raw), &table).map_err(|e| {
                Output::ref_error(theme, &e);
                e
            })
        }
        Err(e) => {
            Output::ref_error(theme, &e);
            Err(e)
        }
    }
}

fn current_project_dir() -> Option<std::path::PathBuf> {
    let dir = std::env::current_dir().ok()?;
    // The current directory is the project lock context for realization
    // commands, even before the first project file exists. The filesystem
    // root is the one deliberate no-project context used by the CLI tests and
    // must not receive a `/.jet/lock`.
    if dir.parent().is_some() {
        Some(dir)
    } else {
        None
    }
}

/// Assemble the acquisition plan before provider realization. Nix narinfo is
/// read here for its signed closure sizes; payload admission remains behind
/// the caller's single confirmation gate.
pub(super) fn plan_downloads(
    theme: &Theme,
    roots: &Roots,
    flags: &Flags,
    table: &RefSpec::SourceTable,
    specs: &[RefSpec::RefSpec],
    scope: RealizeScope,
) -> Result<Provider::DownloadPlan, i32> {
    let store_dir = roots.hangar_dir();
    let project_dir = match scope {
        RealizeScope::Project => current_project_dir(),
        RealizeScope::UserProfile | RealizeScope::Use => None,
    };
    let fixtures = if flags.offline {
        fixtures_for(flags)
    } else {
        flags.fixtures.clone()
    };
    let mut pending = Vec::new();

    for spec in specs {
        let uses_nix = Provider::uses_nix_provider(spec, table, flags.offline, &store_dir);
        if uses_nix && fixtures.is_some() {
            continue;
        }
        if uses_nix
            && Provider::needs_nix_bridge(
                spec,
                table,
                flags.offline,
                &store_dir,
                project_dir.as_deref(),
            )
            .is_some()
        {
            continue;
        }
        // Prompt planning is a routing pass, not the cache authority. A
        // persisted user-profile candidate is enough to keep the fully
        // cached entry path out of closure verification; the subsequent Store
        // realization still proves the complete closure before reuse.
        if matches!(scope, RealizeScope::Use | RealizeScope::UserProfile)
            && Store::find_by_reference_read_only(roots, &spec.raw).is_some_and(|entry| {
                !uses_nix || nix_catalog_cache_entry_matches(&entry, flags.local_nix_catalog.is_some())
            })
        {
            continue;
        }
        let probe = Provider::Ctx {
            fixtures: fixtures.as_deref(),
            store_dir: &store_dir,
            offline: flags.offline,
            project_dir: project_dir.as_deref(),
            nix_index: None,
            nix_roots: Some(roots),
        };
        // Do not resolve the closure just to decide whether a prompt is
        // needed. The identity candidate avoids that work on a warm path;
        // `realize_verified` remains the final integrity gate.
        let expectation = Provider::cache_expectation(spec, table, &probe);
        let cached = expectation.as_ref().is_some_and(|expectation| {
            Store::cache_candidate_matches(roots, &spec.raw, expectation)
                && (!uses_nix
                    || nix_catalog_cache_matches(roots, &spec.raw, flags.local_nix_catalog.is_some()))
        });
        // A non-Nix project ref may have a fully verified Hangar closure but
        // no provider metadata left to derive a fresh expectation offline.
        // The realization path re-checks that recorded identity and closure;
        // this cheap candidate check only keeps the prompt path silent.
        let recorded_project_candidate = scope == RealizeScope::Project
            && !uses_nix
            && expectation.is_none()
            && Store::find_by_reference_read_only(roots, &spec.raw).is_some();
        if !cached && !recorded_project_candidate {
            pending.push(spec.clone());
        }
    }

    if pending.is_empty() {
        return Ok(Provider::DownloadPlan::default());
    }

    let uses_nix = pending
        .iter()
        .any(|spec| Provider::uses_nix_provider(spec, table, flags.offline, &store_dir));
    let nix_index_client = if uses_nix && fixtures.is_none() {
        Some(
            match flags.local_nix_catalog.as_deref() {
                Some(catalog) => NixIndexClient::from_local_catalog(catalog, flags.offline),
                None => NixIndexClient::from_roots_with_mode(roots, flags.offline),
            }
                .map_err(ProviderError::NixIndex),
        )
    } else {
        None
    };
    let nix_index_client = match nix_index_client {
        Some(Ok(client)) => Some(client),
        Some(Err(error)) => {
            report_provider_error(theme, &error);
            return Err(2);
        }
        None => None,
    };
    let ctx = Provider::Ctx {
        fixtures: fixtures.as_deref(),
        store_dir: &store_dir,
        offline: flags.offline,
        project_dir: project_dir.as_deref(),
        nix_index: nix_index_client.as_ref(),
        nix_roots: Some(roots),
    };
    match Provider::plan_downloads(&pending, table, &ctx) {
        Ok(plan) => Ok(plan),
        Err(error) => {
            report_provider_error(theme, &error);
            Err(2)
        }
    }
}

/// Resolve the nearest directory that owns an `env.jet` or `workspace.jet`.
///
/// Environment and workspace commands are allowed from a project
/// subdirectory. Keep the root explicit so planning, trust, dotenv
/// composition, managed state, and explicit refs use the same project
/// identity.
pub(super) fn project_env_root(start: &Path) -> PathBuf {
    project_root(start)
}

/// Realize one ref, recording it in the store and printing progress. `table`
/// resolves named sources (D-JPK17); it is empty for direct CLI refs.
#[allow(dead_code)]
pub(super) fn realize_ref(
    theme: &Theme,
    roots: &Roots,
    flags: &Flags,
    table: &RefSpec::SourceTable,
    spec: &RefSpec::RefSpec,
    name_w: usize,
) -> Option<(Store::StoreEntry, Provider::SourceState)> {
    match realize_ref_outcome(
        theme,
        roots,
        flags,
        table,
        spec,
        name_w,
        RowStyle::Ledger,
        None,
        RealizeScope::Project,
    ) {
        RefOutcome::Realized(entry, state, _line, _lease) => Some((entry, state)),
        RefOutcome::NeedsNix(need) => {
            report_nix_bridge_required(theme, flags, &[need], &[]);
            None
        }
        RefOutcome::Unavailable | RefOutcome::Failed => None,
    }
}

/// How `realize_ref_outcome` reports a successful realization (D-FE-CLI1).
/// `Ledger`/`Ready` print internally, matching every call site before this
/// card; `Silent` prints nothing and hands the caller the row text instead,
/// so a tier-2 live region (`Theme::live_region`) can promote it in place of
/// a plain `eprintln!`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RowStyle {
    /// The state-column ledger row (`✓ name version state`) — D-JPK-CACHE1.
    Ledger,
    /// The tier-1 trivial-op row (`✓ name version`, no state/detail).
    Ready,
    /// No internal print; `RefOutcome::Realized`'s third field carries the
    /// same text `Ledger` would have printed, for the caller to promote.
    Silent,
}

pub(super) enum RefOutcome {
    /// The realized entry, its source state, and the row text `Ledger`
    /// style would print (computed regardless of style, since it's cheap
    /// and `Silent` callers need it).
    Realized(
        Store::StoreEntry,
        Provider::SourceState,
        String,
        Store::CacheLease,
    ),
    NeedsNix(Provider::NixBridgeNeed),
    /// The package cannot be provided on this host and its coded diagnostic
    /// has already been printed. The user must act, so callers exit 2.
    Unavailable,
    Failed,
}

/// Whose ledger a realization belongs to. A user tool is installed into the
/// user profile and is not a package of whatever project the shell happens to
/// be standing in, so it must never be reconciled against that project's lock.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RealizeScope {
    Project,
    UserProfile,
    Use,
}

fn clear_live_region(live: &mut Option<&mut Output::LiveRegion>) {
    if let Some(live) = live.as_deref_mut() {
        live.clear();
    }
}

pub(super) fn realize_ref_outcome(
    theme: &Theme,
    roots: &Roots,
    flags: &Flags,
    table: &RefSpec::SourceTable,
    spec: &RefSpec::RefSpec,
    name_w: usize,
    style: RowStyle,
    mut live: Option<&mut Output::LiveRegion>,
    scope: RealizeScope,
) -> RefOutcome {
    // Tier 2 (D-FE-CLI1 acceptance: "erase live regions before diagnostics").
    // The caller draws the live region's status lines (`building K/N …`) once
    // per item, before calling this function; nothing else redraws them
    // during the call, so clear up front and again at each diagnostic boundary
    // to guarantee every print this call makes lands on a clean line instead
    // of over stale progress-bar text. A `Silent` caller owns the aggregate
    // line and the progress sink redraws it while this call runs; clear only
    // the spinner/ledger path up front and keep the aggregate line pinned.
    if style != RowStyle::Silent {
        clear_live_region(&mut live);
    }
    // A TTY gets a live spinner that the final ledger row replaces; without
    // one (piped output, NO_COLOR) the plain status line stands instead.
    // `Silent` (tier-2 live region) draws its own status instead.
    let mut spinner = if style == RowStyle::Silent {
        None
    } else if theme.color {
        Some(theme.spinner(&format!("resolving {} …", spec.raw)))
    } else {
        theme.status(&format!("resolving {} …", theme.bold(&spec.raw)));
        None
    };
    // The provider writes store/source-cache records under the hangar (U2). The
    // store dir also seeds the U9 remote probe's source-cache lookup, so it is
    // resolved before the fixtures decision below.
    let store_dir = roots.hangar_dir();
    let project_dir = match scope {
        RealizeScope::Project => current_project_dir(),
        RealizeScope::UserProfile | RealizeScope::Use => None,
    };
    let uses_nix = Provider::uses_nix_provider(spec, table, flags.offline, &store_dir);
    let recorded_project_candidate = scope == RealizeScope::Project
        && !uses_nix
        && Store::find_by_reference_read_only(roots, &spec.raw).is_some();
    let project_metadata_missing = if recorded_project_candidate {
        let fixtures = if flags.offline {
            fixtures_for(flags)
        } else {
            flags.fixtures.clone()
        };
        let probe = Provider::Ctx {
            fixtures: fixtures.as_deref(),
            store_dir: &store_dir,
            offline: flags.offline,
            project_dir: project_dir.as_deref(),
            nix_index: None,
            nix_roots: Some(roots),
        };
        Provider::cache_expectation(spec, table, &probe).is_none()
    } else {
        false
    };
    let recorded_reuse = if scope == RealizeScope::Use || project_metadata_missing {
        match Store::find_verified_user_profile_by_reference(roots, &spec.raw) {
            Ok(reuse) => reuse,
            Err(error) => {
                drop(spinner);
                clear_live_region(&mut live);
                report_realize_error(theme, &Store::RealizeError::Store(error));
                return RefOutcome::Failed;
            }
        }
    } else {
        None
    };
    let recorded_reuse = recorded_reuse.filter(|realized| {
        !uses_nix
            || nix_catalog_cache_entry_matches(
                realized.metadata(),
                flags.local_nix_catalog.is_some(),
            )
    });
    // A Nix ref may reuse a Hangar copy only when the committed lock identity
    // and the complete closure both verify. A missing transitive object must
    // reach the indexed provider so it can report the exact missing logical
    // path instead of becoming a vague integrity failure. This preflight only
    // checks the persisted identity; Store realization performs the complete
    // closure proof exactly once on the cached path.
    // A verified imported object is a Jetpack result in every mode. Probe it
    // before the Nix-bridge diagnostic so an online run/build can reuse the
    // locked package without rediscovering or invoking Nix.
    let cache_candidate_ok = recorded_reuse.is_some()
        || (uses_nix && fixtures_for(flags).is_none() && {
            let probe = Provider::Ctx {
                fixtures: None,
                store_dir: &store_dir,
                offline: flags.offline,
                project_dir: project_dir.as_deref(),
                nix_index: None,
                nix_roots: None,
            };
            Provider::cache_expectation(spec, table, &probe)
                .is_some_and(|expectation| {
                    Store::cache_candidate_matches(roots, &spec.raw, &expectation)
                        && (!uses_nix
                            || nix_catalog_cache_matches(
                                roots,
                                &spec.raw,
                                flags.local_nix_catalog.is_some(),
                            ))
                })
        });
    let indexed_nix = flags.offline
        && uses_nix
        && fixtures_for(flags).is_none()
        && Provider::needs_nix_bridge(
            spec,
            table,
            flags.offline,
            &store_dir,
            project_dir.as_deref(),
        )
        .is_none();
    // A reference the project lock already pins was realized here before, so a
    // failure now is a damaged or incomplete closure, not an unknown package.
    // Let it reach the indexed provider, which names the exact missing logical
    // path (E1350), instead of answering with the generic "not in the hangar".
    let locked_pin = project_dir
        .as_deref()
        .and_then(|project| Lock::nix_realization(project, &spec.raw))
        .is_some();
    if flags.offline
        && uses_nix
        && fixtures_for(flags).is_none()
        && !cache_candidate_ok
        && !indexed_nix
        && !locked_pin
    {
        drop(spinner);
        clear_live_region(&mut live);
        report_provider_error(
            theme,
            &ProviderError::Offline(format!(
                "`{}` is not in the hangar and --offline forbids fetching provider output",
                spec.raw
            )),
        );
        return RefOutcome::Failed;
    }
    if uses_nix && !package_fixture_available(flags, spec) && !cache_candidate_ok {
        if let Some(need) = Provider::needs_nix_bridge(
            spec,
            table,
            flags.offline,
            &store_dir,
            project_dir.as_deref(),
        ) {
            return RefOutcome::NeedsNix(need);
        }
    }
    // Fixtures are a testing/offline mechanism only. They never override real
    // resolution: a stray `JETPACK_FIXTURES` in the environment must not
    // silently force fixture mode for an ordinary online run. The provider check
    // resolves an inferred `…@github` source's kind (U9) so a `core` source is
    // not mistakenly asked for nix fixtures.
    let fixtures = if flags.offline && uses_nix {
        let fx = fixtures_for(flags);
        if fx.is_none() && !cache_candidate_ok && !indexed_nix && !locked_pin {
            drop(spinner);
            clear_live_region(&mut live);
            report_provider_error(
                theme,
                &ProviderError::Offline(format!(
                    "`{}` is not in the hangar and --offline forbids fetching provider output",
                    spec.raw
                )),
            );
            return RefOutcome::Failed;
        }
        // `fx` may be `None` here when the hangar copy is served from the
        // lock-verified cache, or when an indexed or already-pinned reference
        // goes to the provider so it can report the exact missing object.
        fx
    } else {
        // `--fixtures <dir>` without `--offline` is still honored (explicit
        // opt-in); the bare env var alone is not.
        flags.fixtures.clone()
    };
    let nix_index_client = if uses_nix && fixtures.is_none() && !cache_candidate_ok {
        Some(
            match flags.local_nix_catalog.as_deref() {
                Some(catalog) => NixIndexClient::from_local_catalog(catalog, flags.offline),
                None => NixIndexClient::from_roots_with_mode(roots, flags.offline),
            }
                .map_err(ProviderError::NixIndex),
        )
    } else {
        None
    };
    let nix_index_client = match nix_index_client {
        Some(Ok(client)) => Some(client),
        Some(Err(error)) => {
            drop(spinner);
            clear_live_region(&mut live);
            report_provider_error(theme, &error);
            return RefOutcome::Failed;
        }
        None => None,
    };
    let ctx = Provider::Ctx {
        fixtures: fixtures.as_deref(),
        store_dir: &store_dir,
        offline: flags.offline,
        project_dir: project_dir.as_deref(),
        nix_index: nix_index_client.as_ref(),
        nix_roots: Some(roots),
    };
    // D-JPK-BUILDSCRIPT1: a Core Cargo action is an upstream executable hook,
    // even when its package manifest is locally reviewed. The exact staged
    // source/recipe/capability identity must be approved outside the project
    // metadata before Store realization can reach the provider.
    match Provider::core_build_identity(spec, table, &ctx) {
        Ok(Some(identity)) => {
            drop(spinner.take());
            clear_live_region(&mut live);
            RuntimePolicy::warn_sandbox_fallback(theme);
            if Trust::gate_build_identity(theme, &Trust::store_path(), &identity, flags.trust)
                .is_err()
            {
                return RefOutcome::Failed;
            }
        }
        Ok(None) => {}
        Err(reason) => {
            drop(spinner);
            clear_live_region(&mut live);
            report_provider_error(
                theme,
                &ProviderError::SandboxUnavailable(format!(
                    "could not establish the exact Core Cargo build identity: {reason}"
                )),
            );
            return RefOutcome::Failed;
        }
    }
    let started = std::time::Instant::now();
    let progress = live.as_deref().map(|live| live.progress_handle());
    let realize = || match recorded_reuse {
        Some(realized) => Ok(realized),
        None => Store::realize_verified(roots, &ctx, Store::RealizeRequest::Package { spec, table }),
    };
    let result = match progress {
        Some(progress) => Store::with_progress(progress, realize),
        None => realize(),
    };
    drop(spinner);
    match result {
        Ok(realized) => {
            let (mut entry, source_state, lease) = realized.into_parts();
            if style == RowStyle::Ready {
                if !entry.bin.is_empty() {
                    let path = match lease.stable_path(&entry.bin) {
                        Ok(path) => path,
                        Err(error) => {
                            let failure = lease.consumption_failure(&error);
                            clear_live_region(&mut live);
                            Store::report_integrity(theme, &failure);
                            return RefOutcome::Failed;
                        }
                    };
                    entry.bin = path.to_string_lossy().into_owned();
                }
                if !entry.rlib.is_empty() {
                    let path = match lease.stable_path(&entry.rlib) {
                        Ok(path) => path,
                        Err(error) => {
                            let failure = lease.consumption_failure(&error);
                            clear_live_region(&mut live);
                            Store::report_integrity(theme, &failure);
                            return RefOutcome::Failed;
                        }
                    };
                    entry.rlib = path.to_string_lossy().into_owned();
                }
            }
            // T4 (D-JPK-CACHE1): one ledger row per package — how it was
            // satisfied, and how long a from-source build took.
            let elapsed = started.elapsed();
            let state = if source_state == Provider::SourceState::Built && elapsed.as_secs() >= 1 {
                format!("built {}", Output::human_duration(elapsed))
            } else {
                source_state.label().to_string()
            };
            // Nix-provided packages often carry no version of their own; the
            // store path's `<hash>-<name>-<version>` basename usually does.
            let version = if entry.version.is_empty() {
                version_from_out(&entry.name, &entry.out).unwrap_or_default()
            } else {
                entry.version.clone()
            };
            let catalog_status = nix_catalog_status(&entry);
            let line = theme.render_row(&entry.name, name_w, &version, &state);
            let line = catalog_status
                .as_ref()
                .map(|status| format!("{line} [{status}]") )
                .unwrap_or(line);
            match style {
                RowStyle::Ledger => {
                    theme.row(&entry.name, name_w, &version, &state);
                    theme.detail(&theme.gray(&entry.out));
                    if let Some(status) = catalog_status.as_deref() {
                        theme.detail(status);
                    }
                }
                RowStyle::Ready => {
                    theme.ready_row(&entry.name, name_w, &version);
                    if let Some(status) = catalog_status.as_deref() {
                        theme.detail(status);
                    }
                }
                RowStyle::Silent => {}
            }
            RefOutcome::Realized(entry, source_state, line, lease)
        }
        Err(Store::RealizeError::Provider(e)) => {
            // A package the user asked for cannot be provided here, and the
            // coded diagnostic already told them what to do. That is the same
            // class as a missing Nix bridge, so it exits 2 rather than 1.
            let unavailable = matches!(
                e,
                ProviderError::NixCache(_) | ProviderError::NixIndex(_) | ProviderError::Offline(_)
            );
            clear_live_region(&mut live);
            report_provider_error(theme, &e);
            if unavailable {
                RefOutcome::Unavailable
            } else {
                RefOutcome::Failed
            }
        }
        Err(error) => {
            clear_live_region(&mut live);
            report_realize_error(theme, &error);
            RefOutcome::Failed
        }
    }
}

fn nix_catalog_cache_entry_matches(entry: &Store::StoreEntry, local: bool) -> bool {
    let expected = if local {
        "local-unofficial"
    } else {
        "official-signed"
    };
    Store::ProducerRecord::decode(&entry.producer_record)
        .ok()
        .filter(|producer| producer.provider == "nix")
        .and_then(|producer| producer.facts.get("nix.index.tier").cloned())
        .is_some_and(|tier| tier == expected)
}

fn nix_catalog_cache_matches(roots: &Roots, reference: &str, local: bool) -> bool {
    Store::find_by_reference_read_only(roots, reference)
        .is_some_and(|entry| nix_catalog_cache_entry_matches(&entry, local))
}

fn nix_catalog_status(entry: &Store::StoreEntry) -> Option<String> {
    let producer = Store::ProducerRecord::decode(&entry.producer_record).ok()?;
    if producer.provider != "nix" {
        return None;
    }
    let tier = producer.facts.get("nix.index.tier")?;
    let trust = producer
        .facts
        .get("nix.index.trust")
        .map(String::as_str)
        .unwrap_or("unknown");
    let chain = producer
        .facts
        .get("nix.index.signature-chain")
        .map(String::as_str)
        .unwrap_or("unknown");
    Some(if tier == "local-unofficial" {
        format!(
            "catalog: {tier} ({trust}; signature chain {chain}; name-to-store-path mapping is unverified; Nix cache bytes remain signature-verified)"
        )
    } else {
        format!("catalog: {tier} ({trust}; signature chain {chain})")
    })
}

fn package_fixture_available(flags: &Flags, spec: &RefSpec::RefSpec) -> bool {
    if flags.offline {
        fixtures_for(flags)
            .map(|dir| dir.join(Provider::fixture_name(spec)).is_file())
            .unwrap_or(false)
    } else {
        flags
            .fixtures
            .as_ref()
            .map(|dir| dir.join(Provider::fixture_name(spec)).is_file())
            .unwrap_or(false)
    }
}

pub(super) fn report_nix_bridge_required(
    theme: &Theme,
    flags: &Flags,
    holes: &[Provider::NixBridgeNeed],
    realized_refs: &[String],
) {
    if flags.json {
        let holes_json = holes
            .iter()
            .map(|h| crate::JSON::quote(&h.reference))
            .collect::<Vec<_>>()
            .join(", ");
        let realized_json = realized_refs
            .iter()
            .map(|r| crate::JSON::quote(r))
            .collect::<Vec<_>>()
            .join(", ");
        println!("{{\"code\":\"E1272\",\"realized\":[{realized_json}],\"holes\":[{holes_json}]}}");
    }
    let count = holes.len();
    let subject = if count == 1 {
        "package lacks"
    } else {
        "packages lack"
    };
    let refs = holes
        .iter()
        .map(|h| format!("`{}`", h.reference))
        .collect::<Vec<_>>()
        .join(", ");
    let fix_ref = holes
        .first()
        .map(|h| h.reference.as_str())
        .unwrap_or("<ref>");
    theme.error_coded(
        "E1272",
        &format!("{count} {subject} a supported Nix compatibility output"),
        &format!(
            "{refs} need a pinned compatibility output. Jetpack does not invoke an installed Nix executable for package realization."
        ),
        &format!(
            "provide a pinned fixture or verified Hangar output, or replace the package with a native source/adapter; `jetpack add {fix_ref} --adapt` drafts one."
        ),
    );
}

pub(super) fn realize_adapter(
    theme: &Theme,
    roots: &Roots,
    flags: &Flags,
    plan: &ModuleEval::AdapterPlan,
    table: &RefSpec::SourceTable,
    consume: bool,
) -> Option<(Store::StoreEntry, Provider::SourceState, Store::CacheLease)> {
    theme.status(&format!("adapting {} …", theme.bold(&plan.name)));
    let store_dir = roots.hangar_dir();
    let project_dir = current_project_dir();
    let fixtures = if flags.offline {
        fixtures_for(flags)
    } else {
        flags.fixtures.clone()
    };
    let ctx = Provider::Ctx {
        fixtures: fixtures.as_deref(),
        store_dir: &store_dir,
        offline: flags.offline,
        project_dir: project_dir.as_deref(),
        nix_index: None,
        nix_roots: None,
    };
    let expectation = match Provider::adapter_cache_expectation(plan, table, &ctx) {
        Ok(expectation) => expectation,
        Err(error) => {
            report_provider_error(theme, &error);
            return None;
        }
    };
    if let ModuleEval::AdapterRecipe::Build(recipe) = &plan.recipe {
        RuntimePolicy::warn_sandbox_fallback(theme);
        let identity = Provider::adapter_action_identity(
            plan,
            recipe,
            &expectation.identity.source_fingerprint,
            &expectation.identity.platform,
            table,
        );
        if Trust::gate_build_identity(theme, &Trust::store_path(), &identity, flags.trust).is_err()
        {
            return None;
        }
    }
    match Store::realize_verified(
        roots,
        &ctx,
        Store::RealizeRequest::Adapter {
            plan,
            table,
            expectation: &expectation,
        },
    ) {
        Ok(realized) => {
            let (mut entry, source_state, lease) = realized.into_parts();
            if consume {
                if !entry.bin.is_empty() {
                    let path = match lease.stable_path(&entry.bin) {
                        Ok(path) => path,
                        Err(error) => {
                            let failure = lease.consumption_failure(&error);
                            Store::report_integrity(theme, &failure);
                            return None;
                        }
                    };
                    entry.bin = path.to_string_lossy().into_owned();
                }
            }
            theme.ok(&format!(
                "{} {}",
                theme.bold(&entry.name),
                source_state.label()
            ));
            theme.detail(&theme.gray(&entry.out));
            Some((entry, source_state, lease))
        }
        Err(Store::RealizeError::Provider(error)) => {
            report_provider_error(theme, &error);
            if flags.shell_on_fail {
                shell_on_failed_build(theme, roots, &plan.name);
            }
            None
        }
        Err(error) => {
            report_realize_error(theme, &error);
            None
        }
    }
}

fn report_realize_error(theme: &Theme, error: &Store::RealizeError) {
    match error {
        Store::RealizeError::Integrity(failure) => Store::report_integrity(theme, failure),
        Store::RealizeError::Store(error) => {
            let detail = error.to_string();
            if detail.starts_with("unreproducible action") {
                theme.error_coded(
                    "E1315",
                    "unreproducible action rejected",
                    &format!("independent producer results disagreed: {detail}"),
                    "inspect `private/unreproducible/<action-key>.json`, fix the nondeterminism, and run a fresh build.",
                )
            } else {
                theme.error_coded(
                    "E1315",
                    "hangar ingest failed",
                    &format!("the verified realization transaction failed: {detail}"),
                    "check permissions on the store root, or set JETPACK_ROOT.",
                )
            }
        }
        Store::RealizeError::Provider(error) => report_provider_error(theme, error),
    }
}

/// Best-effort version out of a store-path basename like
/// `<hash>-ripgrep-14.1.0`: everything after `<name>-`, if it starts with a
/// digit (so `my-tool-src` never masquerades as a version).
fn version_from_out(name: &str, out: &str) -> Option<String> {
    let base = out.rsplit('/').next()?;
    let idx = base.find(&format!("-{name}-"))?;
    let version = &base[idx + name.len() + 2..];
    if version.starts_with(|c: char| c.is_ascii_digit()) {
        Some(version.to_string())
    } else {
        None
    }
}

pub(crate) fn report_provider_error(theme: &Theme, err: &ProviderError) {
    match err {
        ProviderError::BuildFailed(reason) => theme.error(
            "the provider failed to build that package",
            reason,
            "check the package name, e.g. `fastfetch@nixpkgs`.",
        ),
        ProviderError::BadOutput(reason) => theme.error(
            "couldn't understand the provider's output",
            reason,
            "this is likely a Jetpack bug — please report it.",
        ),
        ProviderError::Ingest(reason) => theme.error_coded(
            "E1315",
            "hangar ingest aborted",
            reason,
            "re-run ingest against a stable output, or quarantine and rebuild it from a trusted source.",
        ),
        ProviderError::FixtureMissing(path) => theme.error(
            "no offline fixture for that ref",
            &format!("expected a fixture at {}", path.display()),
            "provide a pinned compatibility fixture or verified Hangar output; Jetpack has no installed-Nix fallback.",
        ),
        ProviderError::Unsupported(reason) => theme.error(
            "that source can't be realized yet",
            reason,
            "provide a pinned compatibility output or use a supported native source.",
        ),
        ProviderError::ForeignProjection(reason) => theme.error_coded(
            "E1256",
            "couldn't project the foreign package or environment",
            reason,
            "provide the pinned provider artifact and generated `.jet/bindings/<language>/<library>.jet`, use supported literal devShell fields, or declare the environment in `env.*`.",
        ),
        ProviderError::CoreBuild(reason) => theme.error(
            "couldn't build that Jet package",
            reason,
            "check the package name and that its source repo has an env.jet.",
        ),
        ProviderError::Cran(reason) => theme.error(
            "couldn't realize that CRAN package",
            reason,
            "check the exact CRAN package ref and the configured CRAN authority.",
        ),
        ProviderError::LuaRocks(reason) => theme.error(
            "could not realize the LuaRocks package",
            reason,
            "use an exact `<name>#version=<version>@luarocks` ref and verify the repository metadata and source hash",
        ),
        ProviderError::Registry(registry, reason) => theme.error(
            &format!("could not realize the {registry} package"),
            reason,
            "use an exact `…@ruby`, `…@perl`, or `…@php` ref and verify registry metadata and source hashes.",
        ),
        // E1232: sparse subtree fetch + full-clone fallback both failed.
        ProviderError::MonorepoFetch(reason) => theme.error_coded(
            "E1232",
            "couldn't fetch that monorepo source",
            reason,
            "check the source URL and revision, and that the network/provider is reachable.",
        ),
        // E1233: an in-repo dependency is outside the source's workspace index.
        ProviderError::MemberOutsideWorkspace(reason) => theme.error_coded(
            "E1233",
            "an in-repo dependency is outside the workspace",
            reason,
            "add the dependency to the source repo's `workspace.jet` `members:`, or depend on it as an external `package@source` ref.",
        ),
        ProviderError::Adapter(reason) => theme.error_coded(
            "E1270",
            "adapter package could not be realized",
            reason,
            "check the `Pkg.adapt(...)` source and recipe.",
        ),
        ProviderError::BuildDebug(reason) => theme.error_coded(
            "E1273",
            "package build failed at a logged step",
            reason,
            "run `jet logs <pkg>` for full output, or rerun with `--shell-on-fail`.",
        ),
        ProviderError::SandboxUnavailable(reason) => theme.error_coded(
            "E1275",
            "build sandboxing is required but unavailable",
            reason,
            "provide a trusted substitute or approved remote builder, or enable the native sandbox, then retry.",
        ),
        ProviderError::Channel(reason) => theme.error_coded(
            "E1271",
            "source channel could not be resolved",
            reason,
            &format!(
                "run `jetpack update` with network or fixture metadata, then commit `{}`.",
                Syntax::UNIFIED_LOCK_FILE
            ),
        ),
        ProviderError::NixIndex(error) => {
            let code = match error.code() {
                1349 => "E1349",
                1276 => "E1276",
                _ => "E1348",
            };
            theme.error_coded(
                code,
                "signed nixpkgs index could not resolve that package",
                &error.to_string(),
                "refresh the signed index, use a covered locked nixpkgs attr, or drop `--offline` when a refresh is required.",
            )
        }
        ProviderError::NixCache(reason) => theme.error_coded(
            "E1350",
            "native Nix cache closure could not be admitted",
            reason,
            "repair the signed cache response or restore network access, then retry the realization.",
        ),
        ProviderError::Offline(reason) => theme.error_coded(
            "E1276",
            "--offline forbids network access",
            reason,
            "drop `--offline` for this command, or realize/fetch the needed object before going offline.",
        ),
    }
}

/// The refs to realize, the table that resolves their named sources, and the
/// prompt label for the resulting shell.
pub(super) struct RunPlan {
    /// Canonical project root for project-relative lifecycle files. Keeping
    /// this beside the typed facts prevents nested hook commands from
    /// resolving `.env` against the caller's subdirectory.
    pub(super) project_root: std::path::PathBuf,
    pub(super) refs: Vec<RefSpec::RefSpec>,
    pub(super) adapters: Vec<ModuleEval::AdapterPlan>,
    pub(super) table: RefSpec::SourceTable,
    pub(super) label: String,
    pub(super) prompt_path: ModuleEval::PromptPathMode,
    pub(super) prompt_strip: ModuleEval::PromptStripMode,
    /// U12: dev-supervised `services:` entries the typed env surface
    /// declared, empty for the Phase-1 directive surface (which predates
    /// U12). `jetpack services <verb>` and `jet dev`'s health gate are the
    /// only readers.
    pub(super) dev_services: Vec<ModuleEval::DevServicePlan>,
    /// D-JPK-SECRETMETA1=B / D-JPK-SECRETCOMPOSE1=D: the selected typed
    /// `secrets:` map. Trust sees the redacted declaration identity; activation
    /// checks policy and source presence before entering the environment.
    pub(super) secrets: Vec<ModuleEval::SecretSpec>,
    /// Typed lifecycle, preset, and language-pack facts shared by activation,
    /// lifecycle hooks, and service commands.
    pub(super) environment: ModuleEval::EnvironmentFacts,
}

/// Build a plan from the project `env.jet` (the no-explicit-ref path). `Err`
/// carries the exit code to return.
pub(super) fn load_project_plan(theme: &Theme) -> Result<RunPlan, i32> {
    load_project_plan_with_selections(theme, None, None)
}

pub(super) fn load_project_plan_with_selections(
    theme: &Theme,
    requested_preset: Option<&str>,
    requested_environment: Option<&str>,
) -> Result<RunPlan, i32> {
    let cwd = std::env::current_dir().unwrap_or_default();
    if let Err(code) = reject_retired_jetpack_toml(&cwd) {
        return Err(code);
    }
    let dir = project_env_root(&cwd);

    let Ok(src) = std::fs::read_to_string(EnvFile::path_in(&dir)) else {
        theme.error(
            "nothing to do",
            &format!(
                "no ref was given and there is no {} here.",
                Syntax::ENV_FILE
            ),
            "try `jet run fastfetch@nixpkgs`, or `jet add <ref>` first.",
        );
        return Err(2);
    };

    // Two author surfaces share one file. The typed `module { … }` surface
    // (U3/U6/U8) is evaluated through `modeval`; the Phase-1 `pkg.*` directive
    // surface stays the fallback until the typed example fully replaces it.
    if ModuleEval::is_module_surface(&src) {
        return typed_plan(theme, &src, &dir, requested_preset, requested_environment);
    }

    if let Some(name) = requested_environment {
        theme.error_coded(
            "E1337",
            &format!("environment module `{name}` is not declared"),
            "the explicit selector applies to typed `env.<name>` modules",
            "select a declared `env.<name>` module with `--env <name>`, or omit `--env`",
        );
        return Err(2);
    }

    let ef = EnvFile::parse(&src);
    let table = ef.source_table();
    let refs = classify_all(theme, ef.refs().iter().map(String::as_str), &table)?;
    Ok(RunPlan {
        project_root: dir.clone(),
        refs,
        adapters: Vec::new(),
        table,
        label: ef.prompt_label(),
        prompt_path: ModuleEval::PromptPathMode::default(),
        prompt_strip: ModuleEval::PromptStripMode::default(),
        dev_services: Vec::new(),
        secrets: Vec::new(),
        environment: ModuleEval::EnvironmentFacts::default(),
    })
}

/// Evaluate the typed `module { … }` env surface (U3/U6/U8) into a plan.
/// Source refs merge across modules and `Pkg` sugar resolves to
/// `package@source` refs; the merged `prompt` becomes the shell label.
fn typed_plan(
    theme: &Theme,
    src: &str,
    dir: &Path,
    requested_preset: Option<&str>,
    requested_environment: Option<&str>,
) -> Result<RunPlan, i32> {
    let plan =
        ModuleEval::evaluate_env_with_selections(src, dir, requested_preset, requested_environment)
            .map_err(|d| {
                eprint!(
                    "{}",
                    crate::Diagnostics::render_all(Syntax::ENV_FILE, src, std::slice::from_ref(&d))
                );
                2
            })?;
    let table = plan.table;
    // U12: a dev service with no explicit `run:` that matches the built-in
    // catalog implicitly depends on that catalog's package (e.g. `redis: {
    // enable: true }` needs `redis-server` on PATH) — fold its ref in
    // alongside the author's own `packages:` so it realizes the same way.
    let mut package_refs = plan.package_refs;
    let selected_preset = plan.selected_preset;
    if let Some(formatter) = &plan.lifecycle.formatter {
        if !package_refs
            .iter()
            .any(|existing| existing == &formatter.package)
        {
            package_refs.push(formatter.package.clone());
        }
    }
    // `evaluate_env_with_selections` already expanded the typed selections. Keep
    // that exact graph fact through realization; re-expanding here could make
    // planning, trust, and activation disagree if the catalog changes.
    let language_expansion = plan.language_expansion;
    if let Some(preset) = &selected_preset {
        for package in &preset.packages {
            if !package_refs.iter().any(|existing| existing == package) {
                package_refs.push(package.clone());
            }
        }
    }
    for package in &language_expansion.packages {
        if !package_refs.iter().any(|existing| existing == package) {
            package_refs.push(package.clone());
        }
    }
    for integration in &plan.integrations {
        for package in &integration.packages {
            if !package_refs.iter().any(|existing| existing == package) {
                package_refs.push(package.clone());
            }
        }
    }
    for svc in &plan.dev_services {
        if svc.enable && svc.run.is_none() {
            if let Some(pkg_ref) = Services::catalog_pkg_ref(&svc.name) {
                if !package_refs.iter().any(|r| r == pkg_ref) {
                    package_refs.push(pkg_ref.to_string());
                }
            }
        }
    }
    let refs = classify_all(theme, package_refs.iter().map(String::as_str), &table)?;
    Ok(RunPlan {
        project_root: dir.to_path_buf(),
        refs,
        adapters: plan.adapters,
        table,
        label: plan
            .prompt
            .unwrap_or_else(|| Syntax::JETPACK_PROMPT_LABEL.to_string()),
        prompt_path: plan.prompt_path,
        prompt_strip: plan.prompt_strip,
        dev_services: plan.dev_services.clone(),
        secrets: plan.secrets,
        environment: ModuleEval::EnvironmentFacts {
            environment_names: plan.environment_names,
            active_environment: plan.active_environment,
            active_environment_provenance: plan.active_environment_provenance,
            source_files: plan.source_files,
            environment_reads: plan.environment_reads,
            dev_services: plan.dev_services,
            lifecycle: plan.lifecycle,
            presets: plan.presets,
            languages: language_expansion.selections.clone(),
            selected_preset,
            language_expansion: language_expansion.clone(),
            language_packs: language_expansion.packs.clone(),
            language_projections: language_expansion.projections.clone(),
            files: plan.files,
            integrations: plan.integrations,
            integration_facts: plan.integration_facts,
            package_profiles: plan.package_profiles,
        },
    })
}

/// Classify a sequence of `package@source` refs against `table`, printing
/// the first failure and returning exit code 2.
fn classify_all<'a>(
    theme: &Theme,
    raws: impl Iterator<Item = &'a str>,
    table: &RefSpec::SourceTable,
) -> Result<Vec<RefSpec::RefSpec>, i32> {
    let mut refs = Vec::new();
    for raw in raws {
        match RefSpec::classify_in(raw, table) {
            Ok(s) => refs.push(s),
            Err(e) => {
                Output::ref_error(theme, &e);
                return Err(2);
            }
        }
    }
    Ok(refs)
}

pub(super) struct ChannelSource {
    pub(super) name: String,
    base: String,
    pub(super) channel: RefSpec::ChannelRef,
    pub(super) policy: RefSpec::ChannelPolicy,
    pub(super) raw: String,
}

pub(super) fn channel_sources(table: &RefSpec::SourceTable) -> Vec<ChannelSource> {
    table
        .declarations()
        .into_iter()
        .filter_map(|(name, upstream, _)| {
            let (base, channel) = RefSpec::split_channel_ref(&upstream);
            let policy = table.channel_policy(&name);
            let base = if policy.moves() {
                base.split_once(Syntax::REF_CHANNEL_MARKER)
                    .map(|(base, _)| base)
                    .unwrap_or(base)
            } else {
                base
            };
            let raw = table.source_ref(&name).unwrap_or(base).to_string();
            Some(ChannelSource {
                name,
                base: base.to_string(),
                channel: channel
                    .or_else(|| policy.moves().then_some(RefSpec::ChannelRef::Latest))?,
                policy,
                raw,
            })
        })
        .collect()
}

/// D-CHANNEL-AUTO1=A: refresh automatic channels before applying the exact
/// lock. Manual channels still move only in `jetpack update`; pinned sources
/// never enter this loop.
pub(super) fn apply_locked_channels(
    theme: &Theme,
    project_dir: &Path,
    table: &mut RefSpec::SourceTable,
    flags: &Flags,
) -> Result<(), i32> {
    for source in channel_sources(table) {
        if source.policy == RefSpec::ChannelPolicy::Automatic && !flags.offline {
            match resolve_source_channel(&source, flags) {
                Ok(exact) => {
                    let changed = Lock::locked_source_channel(project_dir, &source.name)
                        .is_none_or(|lock| lock.exact != exact);
                    if changed {
                        if let Err(error) = rewrite_channel_manifest(project_dir, &source, &exact) {
                            theme.error_coded(
                                "E1340",
                                &format!(
                                    "automatic channel `{}` could not update the manifest",
                                    source.name
                                ),
                                &error,
                                "fix the manifest permissions and run the command again",
                            );
                            return Err(2);
                        }
                        Lock::record_source_channel(
                            project_dir,
                            Lock::LockedSourceChannel {
                                name: source.name.clone(),
                                channel: source.channel.as_str().to_string(),
                                exact: exact.clone(),
                            },
                        );
                    }
                }
                Err(error) if Lock::locked_source_channel(project_dir, &source.name).is_none() => {
                    report_provider_error(theme, &error);
                    return Err(2);
                }
                Err(_) => {}
            }
        }
        let Some(lock) = Lock::locked_source_channel(project_dir, &source.name) else {
            report_unlocked_channel(theme, &source.name, source.channel.as_str());
            return Err(2);
        };
        if lock.channel != source.channel.as_str() {
            report_unlocked_channel(theme, &source.name, source.channel.as_str());
            return Err(2);
        }
        table.set_upstream(&source.name, lock.exact);
    }
    Ok(())
}

/// Write a resolved moving source back to the same declarative reference. The
/// policy marker remains visible, while the exact selector becomes explicit.
pub(super) fn rewrite_channel_manifest(
    project_dir: &Path,
    source: &ChannelSource,
    exact: &str,
) -> Result<(), String> {
    let path = EnvFile::path_in(project_dir);
    let current = std::fs::read_to_string(&path)
        .map_err(|error| format!("could not read `{}`: {error}", path.display()))?;
    let Some(old) = (!source.raw.is_empty()).then_some(source.raw.as_str()) else {
        return Err(format!("source `{}` has no manifest spelling", source.name));
    };
    let resolved = manifest_channel_ref(exact, source.policy);
    if old == resolved {
        return Ok(());
    }
    let next = current.replace(old, &resolved);
    if next == current {
        return Err(format!(
            "source `{}` was not found in `{}`",
            source.name,
            path.display()
        ));
    }
    std::fs::write(&path, next)
        .map_err(|error| format!("could not write `{}`: {error}", path.display()))
}

fn manifest_channel_ref(exact: &str, policy: RefSpec::ChannelPolicy) -> String {
    let exact = exact
        .split_once(Syntax::REF_SEPARATOR)
        .map(|(provider, target)| format!("{target}@{provider}"))
        .unwrap_or_else(|| exact.to_string());
    match policy {
        RefSpec::ChannelPolicy::Pinned => exact,
        RefSpec::ChannelPolicy::Manual => {
            format!("{exact}{}{}", Syntax::REF_CHANNEL_MARKER, Syntax::REF_CHANNEL_LATEST)
        }
        RefSpec::ChannelPolicy::Automatic => {
            format!("{exact}{}{}", Syntax::REF_CHANNEL_MARKER, Syntax::REF_CHANNEL_AUTO)
        }
    }
}

fn report_unlocked_channel(theme: &Theme, name: &str, channel: &str) {
    theme.error_coded(
        "E1271",
        &format!("source `{name}` tracks `{channel}` but is not locked"),
        &format!(
            "channel refs resolve only during `jetpack update`; build/run/env read the exact source recorded in `{}`.",
            Syntax::UNIFIED_LOCK_FILE
        ),
        &format!(
            "run `jetpack update {name}` and commit `{}`.",
            Syntax::UNIFIED_LOCK_FILE
        ),
    );
}

pub(super) fn resolve_source_channel(
    source: &ChannelSource,
    flags: &Flags,
) -> Result<String, ProviderError> {
    if let Some(exact) = resolve_channel_from_fixture(source, flags) {
        return Ok(exact);
    }
    if flags.offline || ci_mode() {
        return Err(ProviderError::Channel(format!(
            "source `{}` tracks `{}` but no exact lock entry exists",
            source.name,
            source.channel.as_str()
        )));
    }
    resolve_channel_with_git(source)
}

fn resolve_channel_from_fixture(source: &ChannelSource, flags: &Flags) -> Option<String> {
    let dir = fixtures_for(flags)?;
    let raw = std::fs::read_to_string(dir.join("channels.txt")).ok()?;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() >= 3 && cols[0] == source.base && cols[1] == source.channel.as_str() {
            return Some(exact_upstream(&source.base, cols[2]));
        }
    }
    None
}

pub(super) fn channel_download_size_from_fixture(
    source: &ChannelSource,
    flags: &Flags,
) -> Option<u64> {
    let dir = fixtures_for(flags)?;
    let raw = std::fs::read_to_string(dir.join("channels.txt")).ok()?;
    raw.lines().find_map(|line| {
        let cols = line.split_whitespace().collect::<Vec<_>>();
        (cols.len() >= 4 && cols[0] == source.base && cols[1] == source.channel.as_str())
            .then(|| cols[3].parse().ok())
            .flatten()
    })
}

fn exact_upstream(base: &str, exact: &str) -> String {
    if exact.contains(Syntax::REF_SEPARATOR) {
        exact.to_string()
    } else {
        format!("{base}#{exact}")
    }
}

fn ci_mode() -> bool {
    std::env::var_os("CI").is_some_and(|v| !v.is_empty())
}

fn resolve_channel_with_git(source: &ChannelSource) -> Result<String, ProviderError> {
    let Some(rest) = source.base.strip_prefix("github:") else {
        return Err(ProviderError::Channel(format!(
            "source `{}` uses `{}`; only GitHub source channels can be resolved without fixture metadata today",
            source.name, source.base
        )));
    };
    let mut parts = rest.split('/');
    let owner = parts.next().unwrap_or_default();
    let repo = parts.next().unwrap_or_default();
    if owner.is_empty() || repo.is_empty() {
        return Err(ProviderError::Channel(format!(
            "`{}` is not a GitHub repo source",
            source.base
        )));
    }
    let url = format!("https://github.com/{owner}/{repo}.git");
    match &source.channel {
        RefSpec::ChannelRef::Main => {
            let out = git_ls_remote(&url, "refs/heads/main")?;
            let rev = out
                .split_whitespace()
                .next()
                .ok_or_else(|| ProviderError::Channel("no `main` head found".to_string()))?;
            Ok(format!("{}#{}", source.base, rev))
        }
        RefSpec::ChannelRef::Latest => {
            let tags = git_ls_remote(&url, "refs/tags/*")?;
            let tag = newest_tag(tags.lines(), None).ok_or_else(|| {
                ProviderError::Channel(format!("no release tags found for `{}`", source.base))
            })?;
            Ok(format!("{}#{}", source.base, tag))
        }
        RefSpec::ChannelRef::SemverMask(mask) => {
            let tags = git_ls_remote(&url, "refs/tags/*")?;
            let tag = newest_tag(tags.lines(), Some(mask)).ok_or_else(|| {
                ProviderError::Channel(format!("no tags match `{mask}` for `{}`", source.base))
            })?;
            Ok(format!("{}#{}", source.base, tag))
        }
    }
}

fn git_ls_remote(url: &str, pattern: &str) -> Result<String, ProviderError> {
    if std::env::var_os("JETPACK_DENY_NETWORK").is_some_and(|v| !v.is_empty()) {
        return Err(ProviderError::Offline(
            "network disabled by JETPACK_DENY_NETWORK while refreshing source channels".to_string(),
        ));
    }
    let out = std::process::Command::new("git")
        .args(["ls-remote", "--refs", url, pattern])
        .output()
        .map_err(|e| ProviderError::Channel(format!("could not run `git ls-remote`: {e}")))?;
    if !out.status.success() {
        let reason = String::from_utf8_lossy(&out.stderr)
            .trim()
            .lines()
            .last()
            .unwrap_or("git ls-remote failed")
            .to_string();
        return Err(ProviderError::Channel(reason));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub(super) fn offline_refusal(theme: &Theme, command: &str) -> i32 {
    report_provider_error(
        theme,
        &ProviderError::Offline(format!(
            "`jetpack {command}` is a network-class command and cannot run under --offline"
        )),
    );
    2
}

fn newest_tag<'a>(lines: impl Iterator<Item = &'a str>, mask: Option<&str>) -> Option<String> {
    let major = mask
        .and_then(|m| m.strip_prefix('v'))
        .and_then(|m| m.strip_suffix(".x"))
        .and_then(|m| m.parse::<u64>().ok());
    let mut best: Option<(Vec<u64>, String)> = None;
    for line in lines {
        let Some(tag) = line.split("refs/tags/").nth(1) else {
            continue;
        };
        let tag = tag.trim();
        let Some(nums) = semver_nums(tag) else {
            continue;
        };
        if major.is_some_and(|m| nums.first().copied() != Some(m)) {
            continue;
        }
        let wins = best
            .as_ref()
            .is_none_or(|(old, _)| nums.as_slice() > old.as_slice());
        if wins {
            best = Some((nums, tag.to_string()));
        }
    }
    best.map(|(_, tag)| tag)
}

fn semver_nums(tag: &str) -> Option<Vec<u64>> {
    let rest = tag.strip_prefix('v').unwrap_or(tag);
    let mut out = Vec::new();
    for part in rest.split('.') {
        let digits = part
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>();
        if digits.is_empty() {
            return None;
        }
        out.push(digits.parse().ok()?);
    }
    Some(out)
}
