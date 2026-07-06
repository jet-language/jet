//! E2-M3 (D-DX2 + D-BUILD1) — `jet doctor`: environment self-diagnosis.
//!
//! Jet hides a rustc dependency and a build/store cache; when those drift the
//! errors land far from the cause. `doctor` checks them up front, offline by
//! default, and offers an actionable fix for each problem. Auto-fixable
//! problems can be applied with `--fix`. Network checks (the registry) run only
//! under `--online`.
//!
//! The advisory diagnostic for rustc / native linker / cache / PATH problems is **L2101**.
//! The C-FFI section (D-BUILD1) reports pkg-config presence and hangar link
//! dirs honestly.

use std::path::PathBuf;
use std::process::Command;

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
            Check::ok("toolchain", "rustc", v)
        }
        _ => Check::problem(
            "toolchain",
            "rustc",
            "not found on PATH",
            "install Rust from https://rustup.rs, then re-run; v1 of Jet uses rustc as its hidden backend",
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
    let build = crate::BuildCache::cache_dir();
    out.push(cache_check("build cache", build));
    let ffi = ffi_cache_dir();
    out.push(cache_check("ffi cache", ffi));
    let store = crate::Store::store_dir();
    out.push(cache_check("package store", store));
    out
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
    // The LSP ships inside this same binary (`jet lsp`); there is no external
    // server to find. Report it as wired.
    Check::ok(
        "lsp",
        "language server",
        format!("built in (`{} lsp`)", crate::Syntax::BINARY_NAME),
    )
}

fn check_registry() -> Check {
    // Offline-first: we do not bundle a network client (I6 / no crates), and the
    // registry backend is M12.2. Report honestly rather than fake a probe.
    Check::note(
        "registry",
        "registry",
        "registry backend is not wired yet (M12.2); nothing to reach",
    )
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
    // Hangar link dirs: the shared C-lib store.
    let hangar = PathBuf::from(crate::Syntax::HANGAR_DIR);
    if hangar.is_dir() {
        out.push(Check::ok("c-ffi", "hangar", hangar.display().to_string()));
    } else {
        out.push(Check::note(
            "c-ffi",
            "hangar",
            format!(
                "{} not present (created when you realize a C library)",
                hangar.display()
            ),
        ));
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

/// E2-M15: check that a cross-compilation target triple is installed.
/// Reports whether `rustup target list --installed` contains the triple,
/// and whether the target's std library directory exists under the sysroot.
fn check_cross_target(triple: &str) -> Check {
    if triple == crate::Syntax::BUILD_TARGET_WEB {
        return Check::ok("cross", triple, "Jet web backend target (WASM + JS)");
    }
    // D-DEP-WASM1=A (c81): `--target=plugin` needs `wasm-tools` on PATH (to
    // lift the rustc-built core wasm module into a Component Model binary)
    // in addition to the ordinary `wasm32-unknown-unknown` rustc target this
    // function's normal path below already checks.
    if triple == crate::Syntax::TARGET_PLUGIN {
        let have_wasm_tools = Command::new("wasm-tools")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !have_wasm_tools {
            return Check::problem(
                "cross",
                triple,
                "`wasm-tools` isn't on PATH (needed to build a plugin's Component)",
                "install wasm-tools (ships in the project's `nix develop` shell), or add it to PATH",
                false,
            );
        }
        return check_cross_target("wasm32-unknown-unknown");
    }
    // Step 1: is it a known rustc target at all?
    let known = Command::new("rustc")
        .arg("--print")
        .arg("target-list")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            let list = String::from_utf8_lossy(&o.stdout);
            list.lines().any(|l| l.trim() == triple)
        })
        .unwrap_or(false);

    if !known {
        return Check::problem(
            "cross",
            triple,
            "not a recognised rustc target triple",
            &format!(
                "run `rustc --print target-list | grep {}` to search for similar names",
                triple
            ),
            false,
        );
    }

    // Step 2: is the std library installed for this target?
    let sysroot = Command::new("rustc")
        .arg("--print")
        .arg("sysroot")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    if let Some(root) = sysroot {
        let target_lib = std::path::PathBuf::from(&root)
            .join("lib")
            .join("rustlib")
            .join(triple);
        if !target_lib.exists() {
            return Check::problem(
                "cross",
                triple,
                "std library for this target is not installed",
                &format!("run `rustup target add {}` to install it", triple),
                false,
            );
        }
        Check::ok(
            "cross",
            triple,
            format!("installed ({})", target_lib.display()),
        )
    } else {
        Check::note(
            "cross",
            triple,
            "could not determine sysroot; target may or may not be installed",
        )
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
