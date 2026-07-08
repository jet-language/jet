/// Classify an explicit CLI ref, accepting any named source declared in the
/// current project's env file so `jetpack run stable:ripgrep` works there, and
/// any workspace member so `jetpack run logging` / `jetpack run packages/logging`
/// resolve in a monorepo (Slice B, D-MONOREF1=A). Prints the diagnostic on failure.
fn classify_or_report(theme: &Theme, raw: &str) -> Result<RefSpec::RefSpec, RefError> {
    RefSpec::classify_with_workspace(raw, &cwd_table(), &cwd_workspace_index()).map_err(|e| {
        Output::ref_error(theme, &e);
        e
    })
}

/// Realize one ref, recording it in the store and printing progress. `table`
/// resolves named sources (D-JPK17); it is empty for direct CLI refs.
fn realize_ref(
    theme: &Theme,
    roots: &Roots,
    flags: &Flags,
    table: &RefSpec::SourceTable,
    spec: &RefSpec::RefSpec,
    name_w: usize,
) -> Option<(Store::StoreEntry, Provider::SourceState)> {
    match realize_ref_outcome(theme, roots, flags, table, spec, name_w) {
        RefOutcome::Realized(entry, state) => Some((entry, state)),
        RefOutcome::NeedsNix(need) => {
            report_nix_bridge_required(theme, flags, &[need], &[]);
            None
        }
        RefOutcome::Failed => None,
    }
}

enum RefOutcome {
    Realized(Store::StoreEntry, Provider::SourceState),
    NeedsNix(Provider::NixBridgeNeed),
    Failed,
}

fn realize_ref_outcome(
    theme: &Theme,
    roots: &Roots,
    flags: &Flags,
    table: &RefSpec::SourceTable,
    spec: &RefSpec::RefSpec,
    name_w: usize,
) -> RefOutcome {
    // A TTY gets a live spinner that the final ledger row replaces; without
    // one (piped output, NO_COLOR) the plain status line stands instead.
    let spinner = if theme.color {
        Some(theme.spinner(&format!("resolving {} …", spec.raw)))
    } else {
        theme.status(&format!("resolving {} …", theme.bold(&spec.raw)));
        None
    };
    // The provider writes store/source-cache records under the hangar (U2). The
    // store dir also seeds the U9 remote probe's source-cache lookup, so it is
    // resolved before the fixtures decision below.
    let store_dir = roots.hangar_dir();
    if let Some(entry) = Store::find_by_reference(roots, &spec.raw) {
        drop(spinner);
        theme.ok(&format!("{} {}", theme.bold(&entry.name), "cached"));
        theme.detail(&theme.gray(&entry.out));
        return RefOutcome::Realized(entry, Provider::SourceState::Cached);
    }
    if flags.offline
        && Provider::uses_nix_provider(spec, table, flags.offline, &store_dir)
        && fixtures_for(flags).is_none()
    {
        drop(spinner);
        report_provider_error(
            theme,
            &ProviderError::Offline(format!(
                "`{}` is not in the hangar and --offline forbids fetching provider output",
                spec.raw
            )),
        );
        return RefOutcome::Failed;
    }
    if !package_fixture_available(flags, spec) && !Provider::nix_on_path() {
        if let Some(need) = Provider::needs_nix_bridge(spec, table, flags.offline, &store_dir) {
            return RefOutcome::NeedsNix(need);
        }
    }
    // Fixtures are a testing/offline mechanism only. They never override real
    // resolution: a stray `JETPACK_FIXTURES` in the environment must not
    // silently force fixture mode for an ordinary online run. The provider check
    // resolves an inferred `github@…` source's kind (U9) so a `core` source is
    // not mistakenly asked for nix fixtures.
    let fixtures =
        if flags.offline && Provider::uses_nix_provider(spec, table, flags.offline, &store_dir) {
            let fx = fixtures_for(flags);
            if fx.is_none() {
                drop(spinner);
                report_provider_error(
                    theme,
                    &ProviderError::Offline(format!(
                        "`{}` is not in the hangar and --offline forbids fetching provider output",
                        spec.raw
                    )),
                );
                return RefOutcome::Failed;
            }
            fx
        } else {
            // `--fixtures <dir>` without `--offline` is still honored (explicit
            // opt-in); the bare env var alone is not.
            flags.fixtures.clone()
        };
    let ctx = Provider::Ctx {
        fixtures: fixtures.as_deref(),
        store_dir: &store_dir,
        offline: flags.offline,
    };
    let started = std::time::Instant::now();
    let result = Provider::realize(spec, table, &ctx);
    drop(spinner);
    match result {
        Ok(r) => {
            // T4 (D-JPK-CACHE1): one ledger row per package — how it was
            // satisfied, and how long a from-source build took.
            let elapsed = started.elapsed();
            let state = if r.source_state == Provider::SourceState::Built && elapsed.as_secs() >= 1
            {
                format!("built {}", Output::human_duration(elapsed))
            } else {
                r.source_state.label().to_string()
            };
            // Nix-provided packages often carry no version of their own; the
            // store path's `<hash>-<name>-<version>` basename usually does.
            let version = if r.version.is_empty() {
                version_from_out(&r.name, &r.out).unwrap_or_default()
            } else {
                r.version.clone()
            };
            theme.row(&r.name, name_w, &version, &state);
            theme.detail(&theme.gray(&r.out));
            let state = r.source_state;
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
                Ok(entry) => Some((entry, state)),
                Err(e) => {
                    theme.error(
                        "could not record the package",
                        &format!("writing to the Jetpack store failed: {e}"),
                        "check permissions on the store root, or set JETPACK_ROOT.",
                    );
                    return RefOutcome::Failed;
                }
            }
            .map_or(RefOutcome::Failed, |(entry, state)| {
                RefOutcome::Realized(entry, state)
            })
        }
        Err(e) => {
            if matches!(e, ProviderError::NixMissing) {
                if let Some(need) =
                    Provider::needs_nix_bridge(spec, table, flags.offline, &store_dir)
                {
                    return RefOutcome::NeedsNix(need);
                }
            }
            report_provider_error(theme, &e);
            RefOutcome::Failed
        }
    }
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

fn report_nix_bridge_required(
    theme: &Theme,
    flags: &Flags,
    holes: &[Provider::NixBridgeNeed],
    realized_refs: &[String],
) {
    if flags.json {
        let holes_json = holes
            .iter()
            .map(|h| super::JSON::quote(&h.reference))
            .collect::<Vec<_>>()
            .join(", ");
        let realized_json = realized_refs
            .iter()
            .map(|r| super::JSON::quote(r))
            .collect::<Vec<_>>()
            .join(", ");
        println!("{{\"code\":\"E1272\",\"realized\":[{realized_json}],\"holes\":[{holes_json}]}}");
    }
    let count = holes.len();
    let subject = if count == 1 {
        "package needs"
    } else {
        "packages need"
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
        &format!("{count} {subject} the Nix bridge, and Nix is not installed"),
        &format!("{refs} currently realize through the Nix compatibility provider on this machine."),
        &format!(
            "install Nix from the official installer, or replace the package with a native source/adapter; `jetpack add {fix_ref} --adapt` drafts one."
        ),
    );
}

fn realize_adapter(
    theme: &Theme,
    roots: &Roots,
    flags: &Flags,
    plan: &ModuleEval::AdapterPlan,
) -> Option<(Store::StoreEntry, Provider::SourceState)> {
    theme.status(&format!("adapting {} …", theme.bold(&plan.name)));
    let store_dir = roots.hangar_dir();
    let ctx = Provider::Ctx {
        fixtures: None,
        store_dir: &store_dir,
        offline: flags.offline,
    };
    match Provider::realize_adapter(plan, &ctx) {
        Ok(r) => {
            theme.ok(&format!(
                "{} {}",
                theme.bold(&r.name),
                r.source_state.label()
            ));
            theme.detail(&theme.gray(&r.out));
            let state = r.source_state;
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
                Ok(entry) => Some((entry, state)),
                Err(e) => {
                    theme.error(
                        "could not record the adapted package",
                        &format!("writing to the Jetpack store failed: {e}"),
                        "check permissions on the store root, or set JETPACK_ROOT.",
                    );
                    None
                }
            }
        }
        Err(e) => {
            report_provider_error(theme, &e);
            if flags.shell_on_fail {
                shell_on_failed_build(theme, roots, &plan.name);
            }
            None
        }
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

pub(super) fn report_provider_error(theme: &Theme, err: &ProviderError) {
    match err {
        ProviderError::NixMissing => theme.error(
            "couldn't run `nix`",
            "This package comes from the Nix provider, but `nix` isn't on your PATH.",
            "install Nix from the official installer, or use a native Jetpack source.",
        ),
        ProviderError::BuildFailed(reason) => theme.error(
            "the provider failed to build that package",
            reason,
            "check the package name, e.g. `nixpkgs:fastfetch`.",
        ),
        ProviderError::BadOutput(reason) => theme.error(
            "couldn't understand the provider's output",
            reason,
            "this is likely a Jetpack bug — please report it.",
        ),
        ProviderError::FixtureMissing(path) => theme.error(
            "no offline fixture for that ref",
            &format!("expected a fixture at {}", path.display()),
            "drop a captured `nix build --json` file there, or run online.",
        ),
        ProviderError::Unsupported(reason) => theme.error(
            "that source can't be realized yet",
            reason,
            "for now use a `nixpkgs:`/`github:` ref while the native builder lands.",
        ),
        ProviderError::CoreBuild(reason) => theme.error(
            "couldn't build that Jet package",
            reason,
            "check the package name and that its source repo has an env.jet.",
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
            "add the dependency to the source repo's `workspace.jet` `members:`, or depend on it as an external `source:package` ref.",
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
        ProviderError::Channel(reason) => theme.error_coded(
            "E1271",
            "source channel could not be resolved",
            reason,
            &format!(
                "run `jetpack update` with network or fixture metadata, then commit `{}`.",
                Syntax::UNIFIED_LOCK_FILE
            ),
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
struct RunPlan {
    refs: Vec<RefSpec::RefSpec>,
    adapters: Vec<ModuleEval::AdapterPlan>,
    table: RefSpec::SourceTable,
    label: String,
    /// U12: dev-supervised `services:` entries the typed env surface
    /// declared, empty for the Phase-1 directive surface (which predates
    /// U12). `jetpack services <verb>` and `jet dev`'s health gate are the
    /// only readers.
    dev_services: Vec<ModuleEval::DevServicePlan>,
    /// U13: every declared `secrets: ["name", …]` entry from the typed env
    /// surface. `jet env`/`jet dev` trust-gate on this and validate the names
    /// exist before entering the environment.
    secrets: Vec<String>,
}

/// Build a plan from the project `env.jet` (the no-explicit-ref path). `Err`
/// carries the exit code to return.
fn load_project_plan(theme: &Theme) -> Result<RunPlan, i32> {
    let dir = std::env::current_dir().unwrap_or_default();

    // Load jetpack.toml [sources] first so they are available as defaults.
    // If the file exists but is malformed, surface the diagnostics and bail out.
    let toml_table = match load_toml_sources(&dir) {
        Ok(t) => t,
        Err((_, code)) => return Err(code),
    };

    let Ok(src) = std::fs::read_to_string(EnvFile::path_in(&dir)) else {
        theme.error(
            "nothing to do",
            &format!(
                "no ref was given and there is no {} here.",
                Syntax::ENV_FILE
            ),
            "try `jetpack run nixpkgs:fastfetch`, or `jetpack add <ref>` first.",
        );
        return Err(2);
    };

    // Two author surfaces share one file. The typed `module { … }` surface
    // (U3/U6/U8) is evaluated through `modeval`; the Phase-1 `pkg.*` directive
    // surface stays the fallback until the typed example fully replaces it.
    if ModuleEval::is_module_surface(&src) {
        return typed_plan_with_defaults(theme, &src, &dir, toml_table);
    }

    let ef = EnvFile::parse(&src);
    let mut table = ef.source_table();
    // Fold jetpack.toml sources as defaults (env.jet inline declarations win).
    table.merge_defaults(toml_table);
    let refs = classify_all(theme, ef.refs().iter().map(String::as_str), &table)?;
    Ok(RunPlan {
        refs,
        adapters: Vec::new(),
        table,
        label: ef.prompt_label(),
        dev_services: Vec::new(),
        secrets: Vec::new(),
    })
}

/// Evaluate the typed `module { … }` env surface (U3/U6/U8) into a plan,
/// optionally seeding `jetpack.toml` [sources] as defaults. Source refs merge
/// across modules and `Pkg` sugar resolves to `<source>:<package>` refs; the
/// merged `prompt` becomes the shell label.
fn typed_plan_with_defaults(
    theme: &Theme,
    src: &str,
    dir: &Path,
    toml_defaults: RefSpec::SourceTable,
) -> Result<RunPlan, i32> {
    let plan = ModuleEval::evaluate_env(src, dir).map_err(|d| {
        eprint!(
            "{}",
            crate::Diagnostics::render_all(Syntax::ENV_FILE, src, std::slice::from_ref(&d))
        );
        2
    })?;
    let mut table = plan.table;
    table.merge_defaults(toml_defaults);
    // U12: a dev service with no explicit `init:` that matches the built-in
    // catalog implicitly depends on that catalog's package (e.g. `redis: {
    // enable: true }` needs `redis-server` on PATH) — fold its ref in
    // alongside the author's own `packages:` so it realizes the same way.
    let mut package_refs = plan.package_refs;
    for svc in &plan.dev_services {
        if svc.enable && svc.init.is_none() {
            if let Some(pkg_ref) = Services::catalog_pkg_ref(&svc.name) {
                if !package_refs.iter().any(|r| r == pkg_ref) {
                    package_refs.push(pkg_ref.to_string());
                }
            }
        }
    }
    let refs = classify_all(theme, package_refs.iter().map(String::as_str), &table)?;
    Ok(RunPlan {
        refs,
        adapters: plan.adapters,
        table,
        label: plan
            .prompt
            .unwrap_or_else(|| Syntax::JETPACK_PROMPT_LABEL.to_string()),
        dev_services: plan.dev_services,
        secrets: plan.secrets,
    })
}

/// Classify a sequence of `<source>:<package>` refs against `table`, printing
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

struct ChannelSource {
    name: String,
    base: String,
    channel: RefSpec::ChannelRef,
}

fn channel_sources(table: &RefSpec::SourceTable) -> Vec<ChannelSource> {
    table
        .declarations()
        .into_iter()
        .filter_map(|(name, upstream, _)| {
            let (base, channel) = RefSpec::split_channel_ref(&upstream);
            Some(ChannelSource {
                name,
                base: base.to_string(),
                channel: channel?,
            })
        })
        .collect()
}

/// D-JPK-CHANNEL1=A: realize-class commands use only exact lock entries.
/// Update-class commands are the only place a channel may move.
fn apply_locked_channels(
    theme: &Theme,
    project_dir: &Path,
    table: &mut RefSpec::SourceTable,
) -> Result<(), i32> {
    for source in channel_sources(table) {
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

fn resolve_source_channel(source: &ChannelSource, flags: &Flags) -> Result<String, ProviderError> {
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

fn offline_refusal(theme: &Theme, command: &str) -> i32 {
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
