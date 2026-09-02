//! E2-M3 (D-DX2 + D-BUILD1) — `jet self doctor`: environment self-diagnosis.
//!
//! Jet hides a rustc dependency and a machine-wide artifact store; when those drift the
//! errors land far from the cause. `doctor` checks them up front, offline by
//! default, and offers an actionable fix for each problem. Auto-fixable
//! problems can be applied with `--fix`. Network checks (the registry) run only
//! under `--online`.
//!
//! The advisory diagnostic for rustc / native linker / cache / PATH problems is **L2101**.
//! The C-FFI section (D-BUILD1) reports pkg-config presence and hangar link
//! dirs honestly.

use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Exact rustc release owned by the current Jet build.
pub const RUSTC_VERSION_PIN: &str = "1.97.1";

/// Health of one checked thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// Everything is fine.
    Ok,
    /// Something is wrong; `fix` says what to do.
    Problem,
    /// Not a failure, but worth noting (e.g. an optional tool is absent).
    Note,
}

/// One line of the doctor report.
#[derive(Debug, Clone)]
pub struct Check {
    /// Section heading this check belongs to (e.g. "toolchain").
    pub section: &'static str,
    /// Short label, e.g. "rustc".
    pub label: String,
    /// Result.
    pub health: Health,
    /// Human detail (version string, path, reason).
    pub detail: String,
    /// When `health == Problem`: the imperative fix.
    pub fix: Option<String>,
    /// Whether `doctor --fix` can apply this automatically.
    pub auto_fixable: bool,
}

impl Check {
    fn ok(section: &'static str, label: impl Into<String>, detail: impl Into<String>) -> Self {
        Check {
            section,
            label: label.into(),
            health: Health::Ok,
            detail: detail.into(),
            fix: None,
            auto_fixable: false,
        }
    }
    fn note(section: &'static str, label: impl Into<String>, detail: impl Into<String>) -> Self {
        Check {
            section,
            label: label.into(),
            health: Health::Note,
            detail: detail.into(),
            fix: None,
            auto_fixable: false,
        }
    }
    fn problem(
        section: &'static str,
        label: impl Into<String>,
        detail: impl Into<String>,
        fix: impl Into<String>,
        auto_fixable: bool,
    ) -> Self {
        Check {
            section,
            label: label.into(),
            health: Health::Problem,
            detail: detail.into(),
            fix: Some(fix.into()),
            auto_fixable,
        }
    }
}

/// What to check.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Allow network checks (registry reachability).
    pub online: bool,
    /// E2-M15: check that a specific cross-compilation target is installed.
    pub cross_target: Option<String>,
}

/// Run every check and return them in report order.
pub fn run(opts: Options) -> Vec<Check> {
    let mut out = Vec::new();
    out.push(check_rustc());
    out.push(check_native_linker());
    out.extend(check_caches());
    out.push(check_path());
    out.push(check_lsp());
    if opts.online {
        out.push(check_registry());
    } else {
        out.push(Check::note(
            "registry",
            "registry",
            "skipped (offline; pass --online to check)",
        ));
    }
    out.extend(check_ffi());
    if let Some(triple) = &opts.cross_target {
        out.push(check_cross_target(triple));
    }
    out
}

/// Does any check report a real problem?
pub fn has_problem(checks: &[Check]) -> bool {
    checks.iter().any(|c| c.health == Health::Problem)
}

fn check_rustc() -> Check {
    match Command::new("rustc").arg("--version").output() {
        Ok(o) if o.status.success() => {
            let v = String::from_utf8_lossy(&o.stdout).trim().to_string();
            let actual = v
                .strip_prefix("rustc ")
                .and_then(|version| version.split_whitespace().next());
            if actual == Some(RUSTC_VERSION_PIN) {
                Check::ok(
                    "toolchain",
                    "rustc",
                    format!("{v} (Jet pin: rustc {RUSTC_VERSION_PIN})"),
                )
            } else {
                Check::problem(
                    "toolchain",
                    "rustc",
                    format!("{v} (Jet requires rustc {RUSTC_VERSION_PIN})"),
                    format!(
                        "use rustc {RUSTC_VERSION_PIN} from the project toolchain, then re-run `jet self doctor`"
                    ),
                    false,
                )
            }
        }
        _ => Check::problem(
            "toolchain",
            "rustc",
            format!("not found on PATH (Jet pin: rustc {RUSTC_VERSION_PIN})"),
            format!(
                "install Rust {RUSTC_VERSION_PIN} from https://rustup.rs, then re-run; v1 of Jet uses rustc as its hidden backend"
            ),
            false,
        ),
    }
}

fn check_native_linker() -> Check {
    if command_ok("cc", &["--version"]) {
        let detail = which("cc")
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "found".to_string());
        Check::ok("toolchain", "native linker (cc)", detail)
    } else {
        Check::problem(
            "toolchain",
            "native linker (cc)",
            "not found on PATH",
            "run from `nix develop`, or install a C toolchain that provides `cc` (`gcc`/`clang`; on Debian/Ubuntu: `build-essential`, on Arch: `base-devel`)",
            false,
        )
    }
}

fn check_caches() -> Vec<Check> {
    let mut out = Vec::new();
    out.push(artifact_store_check());
    let ffi = ffi_cache_dir();
    out.push(cache_check("ffi cache", ffi));
    let store = crate::Store::store_dir();
    out.push(cache_check("package store", store));
    out
}

fn artifact_store_check() -> Check {
    let store = match jet_store::Store::from_env() {
        Ok(store) => store,
        Err(error) => {
            return Check::problem(
                "cache",
                "artifact store",
                format!("could not open the machine-wide store: {error}"),
                "check JET_STORE_DIR and its permissions, then run `jet cache status`",
                false,
            );
        }
    };
    match store.status() {
        Ok(status) => Check::ok(
            "cache",
            "artifact store",
            format!(
                "{} (footprint {}; cap {}; reserve {}; {} entries; {} live leases)",
                status.root.display(),
                format_bytes(status.footprint_bytes),
                format_bytes(status.limit_bytes),
                format_bytes(status.reserve_bytes),
                status.entries.len(),
                status.live_leases
            ),
        ),
        Err(error) => Check::problem(
            "cache",
            "artifact store",
            format!("could not read the machine-wide store: {error}"),
            "check JET_STORE_DIR and its permissions, then run `jet cache status`",
            false,
        ),
    }
}

/// A cache dir is healthy if it exists and is writable, or doesn't exist yet
/// (it is created on demand). A path that exists but isn't a directory is a
/// problem with an auto-fix (remove it).
fn cache_check(label: &'static str, dir: PathBuf) -> Check {
    if !dir.exists() {
        return Check::ok(
            "cache",
            label,
            format!("{} (created on demand)", dir.display()),
        );
    }
    if !dir.is_dir() {
        return Check::problem(
            "cache",
            label,
            format!("{} exists but is not a directory", dir.display()),
            format!("remove `{}` so Jet can recreate it", dir.display()),
            true,
        );
    }
    Check::ok("cache", label, dir.display().to_string())
}
fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value.fract() == 0.0 || value >= 10.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// PATH sanity: is the running `jet` binary reachable as `jet` on PATH?
fn check_path() -> Check {
    let bin = crate::Syntax::BINARY_NAME;
    let on_path = which(bin).is_some();
    if on_path {
        Check::ok("path", bin, "on PATH")
    } else {
        Check::problem(
            "path",
            bin,
            format!("`{}` is not on your PATH", bin),
            format!("add the directory containing `{}` to PATH so commands and `{}-<plugin>` discovery work", bin, bin),
            false,
        )
    }
}

fn check_lsp() -> Check {
    // The LSP ships inside this same binary (`jet self lsp`); there is no external
    // server to find. Report it as wired.
    Check::ok(
        "lsp",
        "language server",
        format!("built in (`{} lsp`)", crate::Syntax::BINARY_NAME),
    )
}

fn check_registry() -> Check {
    let registry = crate::Publish::resolve_publish_registry();
    let safe_url = crate::Publish::redact_registry_url(&registry.url);
    if safe_url != registry.url {
        return Check::problem(
            "registry",
            registry.name,
            format!("{safe_url} (embedded credentials or URL parameters are not allowed)"),
            "remove credentials and query parameters from JET_REGISTRY_URL; configure the host Git credential provider instead",
            false,
        );
    }
    if registry.url.starts_with("file://") {
        let path = registry.url.trim_start_matches("file://");
        return if PathBuf::from(path).exists() {
            Check::ok(
                "registry",
                registry.name,
                format!("{safe_url} (local index reachable)"),
            )
        } else {
            Check::problem(
                "registry",
                registry.name,
                format!("{safe_url} (local index is missing)"),
                "create the configured local registry or set JET_REGISTRY_URL to a reachable index",
                false,
            )
        };
    }
    let mut git = Command::new("git");
    git.args([
        "-c",
        "credential.useHttpPath=true",
        "ls-remote",
        "--",
        &registry.url,
    ])
    .env("GIT_TERMINAL_PROMPT", "0");
    for (key, _) in std::env::vars_os() {
        let key = key.to_string_lossy();
        if key.starts_with("JET_REGISTRY_") {
            git.env_remove(&*key);
        }
    }
    match git.output() {
        Ok(output) if output.status.success() => {
            Check::ok("registry", registry.name, format!("{safe_url} (reachable)"))
        }
        Ok(_) => Check::problem(
            "registry",
            registry.name,
            format!("{safe_url} (unreachable; transport details omitted)"),
            "check JET_REGISTRY_URL, credentials, and network access, then retry",
            false,
        ),
        Err(_) => Check::problem(
            "registry",
            registry.name,
            format!("{safe_url} (git unavailable; transport details omitted)"),
            "install git or configure a local registry index",
            false,
        ),
    }
}

fn check_ffi() -> Vec<Check> {
    let mut out = Vec::new();
    // D-BUILD1: pkg-config presence (used to discover C libs not in the hangar).
    if command_ok("pkg-config", &["--version"]) {
        out.push(Check::ok("c-ffi", "pkg-config", "found"));
    } else {
        out.push(Check::problem(
            "c-ffi",
            "pkg-config",
            "not found",
            "install pkg-config (e.g. `pacman -S pkgconf` / `apt install pkg-config`); only needed if you link C libraries",
            false,
        ));
    }
    // cargo: needed to build the hidden FFI bridge crate.
    if command_ok("cargo", &["--version"]) {
        out.push(Check::ok(
            "c-ffi",
            "cargo",
            "found (builds the FFI bridge crate)",
        ));
    } else {
        out.push(Check::note(
            "c-ffi",
            "cargo",
            "not found (only needed for C FFI builds)",
        ));
    }
    // Hangar link dirs: the user-owned C-lib store.
    let hangar = jetpack::Store::resolve().hangar_dir();
    match fs::symlink_metadata(&hangar) {
        Ok(metadata) if metadata.file_type().is_symlink() => out.push(Check::problem(
            "c-ffi",
            "hangar",
            format!("{} is a symlink", hangar.display()),
            "move the symlink aside without following it, then retry `jetpack hangar path`",
            false,
        )),
        Ok(metadata) if metadata.is_dir() => {
            out.push(Check::ok("c-ffi", "hangar", hangar.display().to_string()));
        }
        Ok(_) => out.push(Check::problem(
            "c-ffi",
            "hangar",
            format!("{} is not a directory", hangar.display()),
            "move the path aside without deleting it, then retry `jetpack hangar path`",
            false,
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let legacy = jetpack::Store::legacy_user_hangar_dir();
            let retired = PathBuf::from(crate::Syntax::HANGAR_DIR);
            let source = [legacy, retired]
                .into_iter()
                .find(|path| fs::symlink_metadata(path).is_ok());
            let detail = if let Some(source) = source {
                format!(
                    "{} not present; legacy Hangar at {} awaits atomic migration",
                    hangar.display(),
                    source.display()
                )
            } else {
                format!(
                    "{} not present (created when you realize a C library)",
                    hangar.display()
                )
            };
            out.push(Check::note("c-ffi", "hangar", detail));
        }
        Err(error) => out.push(Check::problem(
            "c-ffi",
            "hangar",
            format!(
                "{} cannot be inspected ({})",
                hangar.display(),
                error.kind()
            ),
            "restore access to the resolved user Hangar path",
            false,
        )),
    }
    out
}

/// Apply the auto-fixable problems. Returns the labels that were fixed.
pub fn apply_fixes(checks: &[Check]) -> Vec<String> {
    let mut fixed = Vec::new();
    for c in checks {
        if c.health == Health::Problem && c.auto_fixable {
            // The only auto-fix today: a cache path that is a stray file.
            if c.section == "cache" {
                // detail is "<path> exists but is not a directory"
                if let Some(path) = c.detail.split(" exists").next() {
                    if std::fs::remove_file(path).is_ok() || std::fs::remove_dir_all(path).is_ok() {
                        fixed.push(c.label.clone());
                    }
                }
            }
        }
    }
    fixed
}

// ── helpers ──────────────────────────────────────────────

fn ffi_cache_dir() -> PathBuf {
    dirs_home().join(".cache").join("jet").join("ffi")
}

/// The Rust target used by Jet's backend aliases.
pub const WASM_TARGET: &str = "wasm32-unknown-unknown";

/// Resolve a Jet backend target to the Rust target whose installed component
/// the compiler will actually invoke.
pub fn target_rustc_triple(triple: &str) -> &str {
    match triple {
        crate::Syntax::BUILD_TARGET_WEB | crate::Syntax::TARGET_SANDBOX => WASM_TARGET,
        _ => triple,
    }
}

/// Why the target-component probe could not prove readiness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetComponentProbeError {
    /// The target-list command could not provide usable output.
    TargetListUnavailable,
    /// Rustc ran, but did not report the requested target.
    TargetNotKnown,
    /// The sysroot command could not provide a usable path.
    SysrootUnavailable,
    /// The target lib directory is absent, inaccessible, or empty.
    LibraryUnavailable(PathBuf),
}

impl fmt::Display for TargetComponentProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TargetListUnavailable => {
                write!(f, "could not run the rustc target metadata probe")
            }
            Self::TargetNotKnown => write!(f, "not a recognised rustc target triple"),
            Self::SysrootUnavailable => write!(f, "could not determine the rustc sysroot"),
            Self::LibraryUnavailable(path) => write!(
                f,
                "target-specific standard-library component is missing or empty ({})",
                path.display()
            ),
        }
    }
}

impl TargetComponentProbeError {
    /// Actionable remediation without exposing a failed rustc command's stderr.
    pub fn fix(&self, requested: &str) -> String {
        let triple = target_rustc_triple(requested);
        match self {
            Self::TargetNotKnown => format!(
                "run `rustc --print target-list | grep {}` to search for similar names",
                triple
            ),
            Self::TargetListUnavailable | Self::SysrootUnavailable => {
                "make the project's rustc toolchain available, then re-run the target check"
                    .to_string()
            }
            Self::LibraryUnavailable(_) => {
                format!("run `rustup target add {triple}` to install the standard library")
            }
        }
    }
}

/// Prove that the Rust component needed by a Jet target is installed.
///
/// A successful target-list query alone is not enough: rustup can leave a
/// target directory behind after an interrupted installation. The target's
/// actual `lib` directory must contain non-empty regular-file content.
pub fn probe_target_component(
    triple: &str,
) -> Result<PathBuf, TargetComponentProbeError> {
    let rust_triple = target_rustc_triple(triple);

    let target_list = Command::new("rustc")
        .args(["--print", "target-list"])
        .output()
        .map_err(|_| TargetComponentProbeError::TargetListUnavailable)?;
    if !target_list.status.success() {
        return Err(TargetComponentProbeError::TargetListUnavailable);
    }
    let known = String::from_utf8_lossy(&target_list.stdout)
        .lines()
        .any(|line| line.trim() == rust_triple);
    if !known {
        return Err(TargetComponentProbeError::TargetNotKnown);
    }

    let sysroot = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .map_err(|_| TargetComponentProbeError::SysrootUnavailable)?;
    if !sysroot.status.success() {
        return Err(TargetComponentProbeError::SysrootUnavailable);
    }
    let root = String::from_utf8_lossy(&sysroot.stdout).trim().to_string();
    if root.is_empty() {
        return Err(TargetComponentProbeError::SysrootUnavailable);
    }

    let target_lib = PathBuf::from(root)
        .join("lib")
        .join("rustlib")
        .join(rust_triple)
        .join("lib");
    let has_component = fs::read_dir(&target_lib)
        .map(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .metadata()
                    .map(|metadata| metadata.is_file() && metadata.len() > 0)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    if !has_component {
        return Err(TargetComponentProbeError::LibraryUnavailable(target_lib));
    }
    Ok(target_lib)
}

/// E2-M15: check that a cross-compilation target triple is installed.
/// Backend aliases are checked against the Rust target they invoke.
fn check_cross_target(triple: &str) -> Check {
    // D-DEP-WASM1=A (c81): `--target=sandbox` needs `wasm-tools` on PATH (to
    // lift the rustc-built core wasm module into a Component Model binary).
    // Keep this prerequisite separate from the Rust std-component probe below.
    if triple == crate::Syntax::TARGET_SANDBOX && !command_ok("wasm-tools", &["--version"]) {
        return Check::problem(
            "cross",
            triple,
            "`wasm-tools` isn't on PATH (needed to build a sandbox's Component)",
            "install wasm-tools (ships in the project's `nix develop` shell), or add it to PATH",
            false,
        );
    }

    match probe_target_component(triple) {
        Ok(target_lib) => {
            let description = if triple == crate::Syntax::BUILD_TARGET_WEB {
                format!(
                    "Jet web backend target (WASM + JS; installed ({}))",
                    target_lib.display()
                )
            } else if triple == crate::Syntax::BUILD_TARGET_WASI_SERVER {
                format!(
                    "Jet WASI Preview 2 server Component target ({})",
                    target_lib.display()
                )
            } else {
                format!("installed ({})", target_lib.display())
            };
            Check::ok("cross", triple, description)
        }
        Err(error) => Check::problem(
            "cross",
            triple,
            error.to_string(),
            error.fix(triple),
            false,
        ),
    }
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

fn command_ok(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Find `name` on PATH (std-only, no `which` crate — I6).
fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
