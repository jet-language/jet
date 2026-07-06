//! Build recipe substrate + sandbox (D-JPK-ADAPTER1=A safety contract).
//!
//! A `BuildRecipe` turns a staged source tree into a realized package under a
//! confined, auditable build. This is the *internal* substrate the ad-hoc
//! adapter surface (`Recipe.*`, card #176 / D-JPK-ADAPTNAME1) will sit on — the
//! constructor spellings are owner ballot surface and are **not** hardcoded
//! here; callers build `BuildStep`s directly.
//!
//! The safety contract (D-JPK-ADAPTER1=A), enforced structurally:
//! - **network** is denied except a locked `fetch(url, sha256:)` — a fetch with
//!   no locked hash is ungranted ambient network (`E1236`);
//! - **outputs** install only under the package output root — a step targeting
//!   a path outside it escapes confinement (`E1237`);
//! - **build tools** are realized `Pkg` deps, never host `/usr/bin` — an `exec`
//!   naming a tool that is not a realized dep is refused (`E1238`);
//! - every `fetch`/`exec` records an **effect entry** so the build's provenance
//!   is a diff in `.jet/lock`;
//! - a **locked fetch** caches by content hash and is offline-satisfiable on a
//!   re-build (D-JPK-OFFLINE1).
//!
//! std-only (I6): the default transport reads `file://` sources; a remote
//! transport is injected by a caller that already holds network capability, so
//! the compiler seam stays zero-external-crate.

use crate::Diagnostics::Diagnostic;
use crate::SHA256;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One step of a build recipe. Names are internal; the user-facing spellings are
/// D-JPK-ADAPTNAME1 (card #176).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildStep {
    /// A locked network fetch. `sha256` must be present; an empty hash is
    /// ungranted ambient network (`E1236`).
    Fetch { url: String, sha256: String },
    /// Run a build tool. `tool` must be the name of a realized `Pkg` dep in the
    /// `BuildContext.tools` map — never resolved from host PATH (`E1238`).
    Exec { tool: String, args: Vec<String> },
    /// Copy `src` (relative to the source dir) to `dest` under the output root.
    /// `dest` must resolve inside the output root (`E1237`).
    Install { src: String, dest: String },
    /// Copy a whole directory tree relative to the source dir into `dest`
    /// under the output root. Used by `Recipe.copy()`.
    InstallTree { src: String, dest: String },
}

/// A build recipe over a staged source tree.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BuildRecipe {
    pub steps: Vec<BuildStep>,
}

impl BuildRecipe {
    /// A stable content hash of the recipe, used by the trust gate.
    pub fn recipe_hash(&self) -> String {
        let mut data = Vec::new();
        for step in &self.steps {
            match step {
                BuildStep::Fetch { url, sha256 } => {
                    data.extend_from_slice(b"fetch\0");
                    data.extend_from_slice(url.as_bytes());
                    data.push(0);
                    data.extend_from_slice(sha256.as_bytes());
                }
                BuildStep::Exec { tool, args } => {
                    data.extend_from_slice(b"exec\0");
                    data.extend_from_slice(tool.as_bytes());
                    for a in args {
                        data.push(0);
                        data.extend_from_slice(a.as_bytes());
                    }
                }
                BuildStep::Install { src, dest } => {
                    data.extend_from_slice(b"install\0");
                    data.extend_from_slice(src.as_bytes());
                    data.push(0);
                    data.extend_from_slice(dest.as_bytes());
                }
                BuildStep::InstallTree { src, dest } => {
                    data.extend_from_slice(b"install-tree\0");
                    data.extend_from_slice(src.as_bytes());
                    data.push(0);
                    data.extend_from_slice(dest.as_bytes());
                }
            }
            data.push(b'\n');
        }
        format!("sha256-{}", SHA256::sha256_hex(&data))
    }
}

/// A locked source fetch recorded for `.jet/lock` provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchRecord {
    pub url: String,
    pub sha256: String,
}

/// What a recipe run needs, and where its confinement boundaries are.
pub struct BuildContext<'a> {
    /// The staged source tree (read-only by contract).
    pub source_dir: &'a Path,
    /// The single writable install target. Every `install` dest resolves here.
    pub output_root: &'a Path,
    /// Realized `Pkg` build-tool deps: tool name → executable path. An `exec`
    /// step may only name a tool present here (`E1238`).
    pub tools: HashMap<String, PathBuf>,
    /// Where locked fetches cache their bytes (keyed by sha256), so a re-build
    /// is offline-satisfiable.
    pub fetch_cache: &'a Path,
    /// `--offline`: no would-be network fetch may touch the wire.
    pub offline: bool,
}

/// A remote transport a caller injects when it holds network capability. Given a
/// URL, returns the bytes. The compiler seam never supplies one for remote
/// schemes (I6); `file://` is handled by the default reader before this is
/// consulted.
pub type Transport<'a> = &'a dyn Fn(&str) -> Result<Vec<u8>, String>;

/// The provenance a recipe run produced: locked fetches + the effect vocabulary
/// the build exercised. Both flow into `.jet/lock` (D-JPK-ADAPTER1 / D-EFFBUDGET1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunReport {
    pub fetches: Vec<FetchRecord>,
    /// Sorted, de-duplicated effect names (`net.fetch`, `exec:<tool>`, `write`).
    pub effects: Vec<String>,
}

impl RunReport {
    fn add_effect(&mut self, e: &str) {
        if !self.effects.iter().any(|x| x == e) {
            self.effects.push(e.to_string());
            self.effects.sort();
        }
    }
}

/// Validate a recipe against the safety contract without running it — the read
/// path `jet audit` uses (D-BUILDSCOPE1: audit never executes). Returns the
/// first violation as a diagnostic.
pub fn validate(recipe: &BuildRecipe, ctx: &BuildContext) -> Result<(), Diagnostic> {
    for step in &recipe.steps {
        match step {
            BuildStep::Fetch { url, sha256 } => {
                if sha256.trim().is_empty() {
                    return Err(e1236(url));
                }
            }
            BuildStep::Exec { tool, .. } => {
                if !ctx.tools.contains_key(tool) {
                    return Err(e1238(tool));
                }
            }
            BuildStep::Install { dest, .. } | BuildStep::InstallTree { dest, .. } => {
                confined_dest(ctx.output_root, dest)?;
            }
        }
    }
    Ok(())
}

/// Run a recipe under the sandbox. Validates first (so a violation never gets to
/// execute), then performs each step. Returns the build provenance on success.
pub fn run(
    recipe: &BuildRecipe,
    ctx: &BuildContext,
    transport: Option<Transport>,
) -> Result<RunReport, Diagnostic> {
    validate(recipe, ctx)?;
    std::fs::create_dir_all(ctx.output_root).ok();
    let mut report = RunReport::default();
    for step in &recipe.steps {
        match step {
            BuildStep::Fetch { url, sha256 } => {
                do_fetch(url, sha256, ctx, transport, &mut report)?;
            }
            BuildStep::Exec { tool, args } => {
                do_exec(tool, args, ctx, &mut report)?;
            }
            BuildStep::Install { src, dest } => {
                let target = confined_dest(ctx.output_root, dest)?;
                let from = ctx.source_dir.join(src);
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                std::fs::copy(&from, &target).map_err(|e| {
                    Diagnostic::error(
                        "E1237",
                        format!("build step could not install `{src}`"),
                        format!(
                            "copying `{}` into the output root failed: {e}",
                            from.display()
                        ),
                        "make sure the source file exists in the staged tree.".to_string(),
                        None,
                    )
                })?;
                report.add_effect("write");
            }
            BuildStep::InstallTree { src, dest } => {
                let target = confined_dest(ctx.output_root, dest)?;
                let from = ctx.source_dir.join(src);
                copy_tree(&from, &target).map_err(|e| {
                    Diagnostic::error(
                        "E1237",
                        format!("build step could not install `{src}`"),
                        format!(
                            "copying `{}` into the output root failed: {e}",
                            from.display()
                        ),
                        "make sure the source directory exists in the staged tree.".to_string(),
                        None,
                    )
                })?;
                report.add_effect("write");
            }
        }
    }
    Ok(report)
}

/// Run with U27 step logging. Used by adapter/core build paths that need
/// `jet logs`, `jet explain`, and preserved failed scratch.
pub fn run_logged(
    recipe: &BuildRecipe,
    ctx: &BuildContext,
    transport: Option<Transport>,
    attempt: &mut super::BuildDebug::Attempt,
) -> Result<RunReport, Diagnostic> {
    validate(recipe, ctx)?;
    std::fs::create_dir_all(ctx.output_root).ok();
    let mut report = RunReport::default();
    let total = recipe.steps.len();
    for (idx, step) in recipe.steps.iter().enumerate() {
        let index = idx + 1;
        let result = match step {
            BuildStep::Fetch { url, sha256 } => do_fetch(url, sha256, ctx, transport, &mut report),
            BuildStep::Exec { tool, args } => do_exec_logged(tool, args, ctx, &mut report),
            BuildStep::Install { src, dest } => {
                let target = confined_dest(ctx.output_root, dest);
                match target {
                    Ok(target) => {
                        let from = ctx.source_dir.join(src);
                        if let Some(parent) = target.parent() {
                            std::fs::create_dir_all(parent).ok();
                        }
                        std::fs::copy(&from, &target)
                            .map(|_| report.add_effect("write"))
                            .map_err(|e| {
                                Diagnostic::error(
                                    "E1237",
                                    format!("build step could not install `{src}`"),
                                    format!(
                                        "copying `{}` into the output root failed: {e}",
                                        from.display()
                                    ),
                                    "make sure the source file exists in the staged tree."
                                        .to_string(),
                                    None,
                                )
                            })
                    }
                    Err(d) => Err(d),
                }
            }
            BuildStep::InstallTree { src, dest } => {
                let target = confined_dest(ctx.output_root, dest);
                match target {
                    Ok(target) => {
                        let from = ctx.source_dir.join(src);
                        copy_tree(&from, &target)
                            .map(|_| report.add_effect("write"))
                            .map_err(|e| {
                                Diagnostic::error(
                                    "E1237",
                                    format!("build step could not install `{src}`"),
                                    format!(
                                        "copying `{}` into the output root failed: {e}",
                                        from.display()
                                    ),
                                    "make sure the source directory exists in the staged tree."
                                        .to_string(),
                                    None,
                                )
                            })
                    }
                    Err(d) => Err(d),
                }
            }
        };
        match result {
            Ok(()) => attempt.push_step(step_log(step, index, total, ctx, "ok", "", "")),
            Err(d) => {
                attempt.push_step(step_log(
                    step,
                    index,
                    total,
                    ctx,
                    "failed",
                    "",
                    &format!("{}: {}\n{}\n", d.code, d.what, d.why),
                ));
                return Err(d);
            }
        }
    }
    attempt.mark_ok();
    Ok(report)
}

fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&from)?.permissions().mode();
                std::fs::set_permissions(&to, std::fs::Permissions::from_mode(mode))?;
            }
        }
    }
    Ok(())
}

/// Resolve `dest` under `output_root`, rejecting any escape (`..`, absolute
/// path, or a symlink-free normalized path that leaves the root) with `E1237`.
fn confined_dest(output_root: &Path, dest: &str) -> Result<PathBuf, Diagnostic> {
    if dest.contains("..") || Path::new(dest).is_absolute() {
        return Err(e1237(dest));
    }
    let joined = output_root.join(dest);
    let normalized = normalize(&joined);
    if !normalized.starts_with(&normalize(output_root)) {
        return Err(e1237(dest));
    }
    Ok(normalized)
}

/// Lexically normalize a path (no filesystem access): collapse `.` and `..`.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        use std::path::Component::*;
        match comp {
            ParentDir => {
                out.pop();
            }
            CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn do_fetch(
    url: &str,
    sha256: &str,
    ctx: &BuildContext,
    transport: Option<Transport>,
    report: &mut RunReport,
) -> Result<(), Diagnostic> {
    // A locked fetch must carry a hash (checked in validate too, defensively).
    if sha256.trim().is_empty() {
        return Err(e1236(url));
    }
    std::fs::create_dir_all(ctx.fetch_cache).ok();
    let cached = ctx.fetch_cache.join(sha256);
    if cached.is_file() {
        // Offline-satisfiable: the locked source is already cached, no network.
        report.fetches.push(FetchRecord {
            url: url.to_string(),
            sha256: sha256.to_string(),
        });
        report.add_effect("net.fetch");
        return Ok(());
    }
    // Not cached: acquire the bytes. `file://` is std-only and offline-safe.
    let bytes = if let Some(path) = url.strip_prefix("file://") {
        std::fs::read(path).map_err(|e| e1236_fetch(url, &e.to_string()))?
    } else if ctx.offline {
        // A network fetch under `--offline` with no cache hit is ungranted.
        return Err(e1236_offline(url));
    } else if let Some(t) = transport {
        t(url).map_err(|e| e1236_fetch(url, &e))?
    } else {
        // No transport injected for a remote scheme: the compiler seam holds no
        // network capability (I6). The caller must vendor or mirror the source.
        return Err(e1236_no_transport(url));
    };
    // Verify the locked hash before the bytes are ever used.
    let got = SHA256::sha256_hex(&bytes);
    if got != sha256 {
        return Err(e1236_mismatch(url, sha256, &got));
    }
    std::fs::write(&cached, &bytes).map_err(|e| e1236_fetch(url, &e.to_string()))?;
    report.fetches.push(FetchRecord {
        url: url.to_string(),
        sha256: sha256.to_string(),
    });
    report.add_effect("net.fetch");
    Ok(())
}

fn do_exec(
    tool: &str,
    args: &[String],
    ctx: &BuildContext,
    report: &mut RunReport,
) -> Result<(), Diagnostic> {
    let exe = ctx.tools.get(tool).ok_or_else(|| e1238(tool))?;
    let status = std::process::Command::new(exe)
        .args(args)
        .current_dir(ctx.source_dir)
        // Confine writes: run in the staged tree; the install root is the only
        // sanctioned output surface. A hostile tool can still misbehave — the
        // OS-level sandbox is D-JPK-NODAEMON1's unprivileged jail (U28); this
        // seam enforces the *structural* contract.
        .env("JET_BUILD_OUTPUT", ctx.output_root)
        .status()
        .map_err(|e| {
            Diagnostic::error(
                "E1238",
                format!("build tool `{tool}` failed to run"),
                format!(
                    "the realized tool at {} could not be executed: {e}",
                    exe.display()
                ),
                "make sure the tool dependency realized correctly.".to_string(),
                None,
            )
        })?;
    if !status.success() {
        return Err(Diagnostic::error(
            "E1238",
            format!("build tool `{tool}` exited with an error"),
            format!("`{tool} {}` returned a non-zero status.", args.join(" ")),
            "check the build recipe and the tool's arguments.".to_string(),
            None,
        ));
    }
    report.add_effect(&format!("exec:{tool}"));
    Ok(())
}

fn do_exec_logged(
    tool: &str,
    args: &[String],
    ctx: &BuildContext,
    report: &mut RunReport,
) -> Result<(), Diagnostic> {
    let exe = ctx.tools.get(tool).ok_or_else(|| e1238(tool))?;
    let out = std::process::Command::new(exe)
        .args(args)
        .current_dir(ctx.source_dir)
        .env("JET_BUILD_OUTPUT", ctx.output_root)
        .output()
        .map_err(|e| {
            Diagnostic::error(
                "E1238",
                format!("build tool `{tool}` failed to run"),
                format!(
                    "the realized tool at {} could not be executed: {e}",
                    exe.display()
                ),
                "make sure the tool dependency realized correctly.".to_string(),
                None,
            )
        })?;
    if !out.status.success() {
        return Err(Diagnostic::error(
            "E1238",
            format!("build tool `{tool}` exited with an error"),
            format!(
                "`{tool} {}` returned a non-zero status.\nstdout:\n{}\nstderr:\n{}",
                args.join(" "),
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
            "check the build recipe and the tool's arguments.".to_string(),
            None,
        ));
    }
    report.add_effect(&format!("exec:{tool}"));
    Ok(())
}

fn step_log(
    step: &BuildStep,
    index: usize,
    total: usize,
    ctx: &BuildContext,
    status: &str,
    stdout: &str,
    stderr: &str,
) -> super::BuildDebug::StepLog {
    super::BuildDebug::StepLog {
        index,
        total,
        name: step_name(step).to_string(),
        command: step_command(step),
        cwd: ctx.source_dir.to_string_lossy().into_owned(),
        status: status.to_string(),
        stdout: stdout.to_string(),
        stderr: stderr.to_string(),
    }
}

fn step_name(step: &BuildStep) -> &'static str {
    match step {
        BuildStep::Fetch { .. } => "fetch",
        BuildStep::Exec { .. } => "exec",
        BuildStep::Install { .. } => "install",
        BuildStep::InstallTree { .. } => "install-tree",
    }
}

fn step_command(step: &BuildStep) -> String {
    match step {
        BuildStep::Fetch { url, sha256 } => format!("fetch {url} sha256:{sha256}"),
        BuildStep::Exec { tool, args } => format!("{tool} {}", args.join(" ")).trim().to_string(),
        BuildStep::Install { src, dest } => format!("install {src} {dest}"),
        BuildStep::InstallTree { src, dest } => format!("install-tree {src} {dest}"),
    }
}

// ── U19 trust gate (internal substrate) ──────────────────────────────────────
// The interactive first-build approval UX is card #176 (U19); here we keep the
// durable marker so a recipe's first build is distinguishable from a re-build.

/// Record trust for a recipe hash. Returns `true` when this is the **first**
/// build (the hash was newly trusted), `false` when it was already trusted.
/// The trust file is a newline-separated list of accepted recipe hashes under
/// the project's `.jet/` managed folder.
pub fn trust_first_build(recipe_hash: &str, trust_file: &Path) -> bool {
    let existing = std::fs::read_to_string(trust_file).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == recipe_hash) {
        return false;
    }
    if let Some(parent) = trust_file.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(recipe_hash);
    updated.push('\n');
    let _ = std::fs::write(trust_file, updated);
    true
}

// ── Diagnostics ───────────────────────────────────────────────────────────────

/// E1236 — a build step reached the network without a locked `fetch(url, sha256:)`.
pub fn e1236(url: &str) -> Diagnostic {
    Diagnostic::error(
        "E1236",
        "a build step tried to reach the network without a locked fetch".to_string(),
        format!(
            "during a build, network access is denied except a locked `fetch(url, sha256:)`. \
             The fetch of `{url}` carries no `sha256:`, so its result can't be pinned and the \
             build would not be reproducible."
        ),
        "add the source hash: `fetch(\"…\", sha256: \"…\")`, or vendor the source with `jet vendor`."
            .to_string(),
        None,
    )
}

fn e1236_offline(url: &str) -> Diagnostic {
    Diagnostic::error(
        "E1236",
        "a build step needs the network but the build is offline".to_string(),
        format!("`--offline` forbids any network fetch; `{url}` is not in the local fetch cache."),
        "run once online to populate the cache, or `jet vendor` the source and rebuild."
            .to_string(),
        None,
    )
}

fn e1236_no_transport(url: &str) -> Diagnostic {
    Diagnostic::error(
        "E1236",
        "a build step needs a remote source with no transport available".to_string(),
        format!(
            "`{url}` is a remote URL; the build seam holds no network capability by itself \
             (zero-external-crate compiler)."
        ),
        "provide a `file://` mirror, or `jet vendor` the source so the build is offline."
            .to_string(),
        None,
    )
}

fn e1236_fetch(url: &str, reason: &str) -> Diagnostic {
    Diagnostic::error(
        "E1236",
        "a locked build fetch failed".to_string(),
        format!("fetching `{url}` failed: {reason}"),
        "check the URL and the locked hash, or vendor the source.".to_string(),
        None,
    )
}

fn e1236_mismatch(url: &str, want: &str, got: &str) -> Diagnostic {
    Diagnostic::error(
        "E1236",
        "a locked build fetch did not match its hash".to_string(),
        format!("`{url}` was fetched but its sha256 was `{got}`, not the locked `{want}`."),
        "the source changed upstream or was tampered with; update the locked hash deliberately."
            .to_string(),
        None,
    )
}

/// E1237 — a build step wrote outside the package output root.
pub fn e1237(dest: &str) -> Diagnostic {
    Diagnostic::error(
        "E1237",
        format!("a build step tried to write outside the output root: `{dest}`"),
        "a build may only install files under its own package output root. Writing elsewhere \
         would let a build mutate the machine or other packages."
            .to_string(),
        "install into a path under the output root (no `..`, no absolute paths).".to_string(),
        None,
    )
}

/// E1238 — a recipe named a build tool that is not a realized `Pkg` dep.
pub fn e1238(tool: &str) -> Diagnostic {
    Diagnostic::error(
        "E1238",
        format!("build tool `{tool}` is not a realized dependency"),
        "build tools must be realized `Pkg` dependencies of the package, so the build is \
         reproducible. A build never falls through to host `/usr/bin`."
            .to_string(),
        format!(
            "add `{tool}` as a build dependency in `pkg.jet` so it is realized into the hangar."
        ),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "recipe-{tag}-{}-{:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn ctx_at<'a>(
        base: &'a Path,
        src: &'a Path,
        out: &'a Path,
        cache: &'a Path,
    ) -> BuildContext<'a> {
        let _ = base;
        BuildContext {
            source_dir: src,
            output_root: out,
            tools: HashMap::new(),
            fetch_cache: cache,
            offline: false,
        }
    }

    #[test]
    fn build_denies_ambient_network() {
        // A fetch with no locked sha256 is ungranted ambient network → E1236.
        let base = scratch("net");
        let src = base.join("src");
        let out = base.join("out");
        let cache = base.join("cache");
        std::fs::create_dir_all(&src).unwrap();
        let ctx = ctx_at(&base, &src, &out, &cache);
        let recipe = BuildRecipe {
            steps: vec![BuildStep::Fetch {
                url: "https://example.invalid/src.tar".to_string(),
                sha256: String::new(),
            }],
        };
        let err = run(&recipe, &ctx, None).unwrap_err();
        assert_eq!(err.code, "E1236");
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn build_output_confined() {
        // An install targeting a path outside the output root → E1237.
        let base = scratch("confine");
        let src = base.join("src");
        let out = base.join("out");
        let cache = base.join("cache");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("f"), "hi").unwrap();
        let ctx = ctx_at(&base, &src, &out, &cache);
        let recipe = BuildRecipe {
            steps: vec![BuildStep::Install {
                src: "f".to_string(),
                dest: "../escape".to_string(),
            }],
        };
        let err = run(&recipe, &ctx, None).unwrap_err();
        assert_eq!(err.code, "E1237");
        // A confined install succeeds and lands under the output root.
        let ok = BuildRecipe {
            steps: vec![BuildStep::Install {
                src: "f".to_string(),
                dest: "bin/f".to_string(),
            }],
        };
        run(&ok, &ctx, None).unwrap();
        assert!(out.join("bin/f").is_file());
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn build_tool_not_a_dep() {
        // An exec naming a tool that is not a realized dep → E1238.
        let base = scratch("tool");
        let src = base.join("src");
        let out = base.join("out");
        let cache = base.join("cache");
        std::fs::create_dir_all(&src).unwrap();
        let ctx = ctx_at(&base, &src, &out, &cache);
        let recipe = BuildRecipe {
            steps: vec![BuildStep::Exec {
                tool: "gcc".to_string(),
                args: vec![],
            }],
        };
        let err = run(&recipe, &ctx, None).unwrap_err();
        assert_eq!(err.code, "E1238");
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn locked_fetch_roundtrips() {
        // A locked file:// fetch caches by hash, records provenance, and is
        // offline-satisfiable on a second run even after the source vanishes.
        let base = scratch("fetch");
        let src = base.join("src");
        let out = base.join("out");
        let cache = base.join("cache");
        std::fs::create_dir_all(&src).unwrap();
        let source_file = base.join("upstream.tar");
        let payload = b"the source bytes";
        std::fs::write(&source_file, payload).unwrap();
        let sha = SHA256::sha256_hex(payload);
        let url = format!("file://{}", source_file.to_string_lossy());

        let recipe = BuildRecipe {
            steps: vec![BuildStep::Fetch {
                url: url.clone(),
                sha256: sha.clone(),
            }],
        };

        // First build: online, populates the cache and records the fetch.
        let ctx = ctx_at(&base, &src, &out, &cache);
        let report = run(&recipe, &ctx, None).unwrap();
        assert_eq!(report.fetches.len(), 1);
        assert_eq!(report.fetches[0].sha256, sha);
        assert!(report.effects.iter().any(|e| e == "net.fetch"));
        assert!(
            cache.join(&sha).is_file(),
            "locked source must be cached by hash"
        );

        // Now the upstream source disappears and we go offline — the re-build is
        // still satisfiable from the cache, no network.
        std::fs::remove_file(&source_file).unwrap();
        let offline_ctx = BuildContext {
            source_dir: &src,
            output_root: &out,
            tools: HashMap::new(),
            fetch_cache: &cache,
            offline: true,
        };
        let report2 = run(&recipe, &offline_ctx, None).unwrap();
        assert_eq!(report2.fetches[0].sha256, sha);
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn offline_uncached_fetch_is_ungranted() {
        // Offline with no cache hit and a remote scheme → E1236.
        let base = scratch("offline");
        let src = base.join("src");
        let out = base.join("out");
        let cache = base.join("cache");
        std::fs::create_dir_all(&src).unwrap();
        let ctx = BuildContext {
            source_dir: &src,
            output_root: &out,
            tools: HashMap::new(),
            fetch_cache: &cache,
            offline: true,
        };
        let recipe = BuildRecipe {
            steps: vec![BuildStep::Fetch {
                url: "https://example.invalid/x.tar".to_string(),
                sha256: "abc123".to_string(),
            }],
        };
        assert_eq!(run(&recipe, &ctx, None).unwrap_err().code, "E1236");
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn trust_gate_marks_first_build_once() {
        let base = scratch("trust");
        let trust = base.join(".jet/trust");
        let recipe = BuildRecipe {
            steps: vec![BuildStep::Install {
                src: "a".to_string(),
                dest: "a".to_string(),
            }],
        };
        let h = recipe.recipe_hash();
        assert!(
            trust_first_build(&h, &trust),
            "first build is newly trusted"
        );
        assert!(
            !trust_first_build(&h, &trust),
            "second build is already trusted"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn validate_is_pure_read_no_exec() {
        // `jet audit` uses validate(): it flags violations without running any
        // step. A recipe with an exec of a missing tool validates to E1238 and
        // never spawns a process.
        let base = scratch("audit");
        let src = base.join("src");
        let out = base.join("out");
        let cache = base.join("cache");
        std::fs::create_dir_all(&src).unwrap();
        let ctx = ctx_at(&base, &src, &out, &cache);
        let recipe = BuildRecipe {
            steps: vec![BuildStep::Exec {
                tool: "definitely-not-a-real-tool".to_string(),
                args: vec!["--boom".to_string()],
            }],
        };
        assert_eq!(validate(&recipe, &ctx).unwrap_err().code, "E1238");
        std::fs::remove_dir_all(&base).ok();
    }
}
