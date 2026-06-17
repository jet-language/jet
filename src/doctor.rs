//! `jet doctor` — environment self-diagnosis (E2-M3, decisions D-DX2 and
//! D-BUILD1). Jet hides a rustc dependency and a C-FFI/cargo bridge; doctor
//! makes that hidden machinery legible and offers conservative auto-fixes.
//!
//! Design rules this module obeys:
//! - **Offline by default.** No process touches the network unless the caller
//!   passes `--online`/`--network`. The registry check is the only networked
//!   probe and it is skipped otherwise.
//! - **Deterministic test mode.** `--plain` redacts every volatile value
//!   (tool versions, absolute paths) and prints plain ASCII status words, so a
//!   golden transcript is byte-identical across machines / CI.
//! - **Conservative auto-fix.** `--fix` only creates a missing cache/store
//!   directory. It never touches user source or anything under the package
//!   manager's manifests. Each action is logged.
//! - **L2101** is the advisory lint code for a doctor-surfaced problem with an
//!   actionable fix (registered in docs/spec/diagnostics.md).

use std::path::{Path, PathBuf};
use std::process::Command;

/// Status of a single check line.
#[derive(Clone, Copy, PartialEq)]
enum Status {
    Ok,
    /// A hard problem that blocks normal use — doctor exits non-zero.
    Fail,
    /// An advisory (L2101): not blocking, but worth fixing.
    Warn,
    /// Not run (e.g. a network check while offline).
    Skip,
}

/// How doctor renders. Resolved once from argv + environment + TTY.
struct Render {
    /// ANSI color allowed (TTY + not NO_COLOR, etc.).
    color: bool,
    /// `--plain`: redact volatile values, ASCII status words, for golden tests.
    plain: bool,
}

impl Render {
    fn status_word(&self, s: Status) -> String {
        if self.plain || !self.color {
            // ASCII, fixed-width, scriptable.
            match s {
                Status::Ok => "[ ok ]".to_string(),
                Status::Fail => "[fail]".to_string(),
                Status::Warn => "[warn]".to_string(),
                Status::Skip => "[skip]".to_string(),
            }
        } else {
            // Glyphs + color for an attached terminal.
            match s {
                Status::Ok => "\x1b[32m✓\x1b[0m".to_string(),
                Status::Fail => "\x1b[31m✗\x1b[0m".to_string(),
                Status::Warn => "\x1b[33m⚠\x1b[0m".to_string(),
                Status::Skip => "\x1b[90m–\x1b[0m".to_string(),
            }
        }
    }

    /// Redact a tool version string in `--plain` mode so transcripts are stable.
    fn version(&self, v: &str) -> String {
        if self.plain {
            "<version>".to_string()
        } else {
            v.to_string()
        }
    }

    /// Redact an absolute path in `--plain` mode.
    fn path(&self, p: &Path) -> String {
        if self.plain {
            "<path>".to_string()
        } else {
            p.display().to_string()
        }
    }
}

/// One health check's outcome, plus the actionable fix text when not Ok.
struct Check {
    name: String,
    status: Status,
    /// The detail shown after the name (a version, a path, "skipped (offline)").
    detail: String,
    /// L2101 fix text, printed indented under a Warn/Fail line.
    fix: Option<String>,
}

impl Check {
    fn ok(name: &str, detail: String) -> Check {
        Check { name: name.to_string(), status: Status::Ok, detail, fix: None }
    }
    fn fail(name: &str, detail: String, fix: &str) -> Check {
        Check {
            name: name.to_string(),
            status: Status::Fail,
            detail,
            fix: Some(fix.to_string()),
        }
    }
    fn warn(name: &str, detail: String, fix: &str) -> Check {
        Check {
            name: name.to_string(),
            status: Status::Warn,
            detail,
            fix: Some(fix.to_string()),
        }
    }
    fn skip(name: &str, detail: String) -> Check {
        Check { name: name.to_string(), status: Status::Skip, detail, fix: None }
    }
}

/// Parsed `jet doctor` flags.
pub struct Options {
    /// `--online`/`--network`: allow the registry reachability probe.
    pub online: bool,
    /// `--fix`: apply conservative auto-fixes (create missing cache/store dir).
    pub fix: bool,
    /// `--plain`: deterministic, redacted output for golden tests.
    pub plain: bool,
    /// Whether stderr/stdout may carry ANSI color.
    pub color: bool,
}

/// Entry point. Returns the process exit code (0 healthy / advisory-only,
/// 1 if a hard problem blocks normal use).
pub fn run(opts: &Options) -> i32 {
    let r = Render { color: opts.color, plain: opts.plain };

    println!("{} doctor — checking your environment", crate::syntax::BINARY_NAME);
    println!();

    let mut sections: Vec<(&str, Vec<Check>)> = Vec::new();
    sections.push(("toolchain", check_toolchain(&r)));
    sections.push(("cache & store", check_cache_store(&r, opts.fix)));
    sections.push(("PATH", check_path(&r)));
    sections.push(("language server", check_lsp(&r)));
    sections.push(("FFI (C bridge)", check_ffi(&r)));
    sections.push(("registry", check_registry(&r, opts.online)));

    let mut any_fail = false;
    let mut any_warn = false;
    for (title, checks) in &sections {
        println!("{}:", title);
        for c in checks {
            let word = r.status_word(c.status);
            if c.detail.is_empty() {
                println!("  {} {}", word, c.name);
            } else {
                println!("  {} {} — {}", word, c.name, c.detail);
            }
            if let Some(fix) = &c.fix {
                // L2101 advisory line: the actionable fix, clearly attributed.
                println!("       L2101: {}", fix);
            }
            match c.status {
                Status::Fail => any_fail = true,
                Status::Warn => any_warn = true,
                _ => {}
            }
        }
        println!();
    }

    if any_fail {
        println!(
            "found problems that block normal use — fix the {} lines above.",
            r.status_word(Status::Fail)
        );
        1
    } else if any_warn {
        println!(
            "healthy, with advisories — see the L2101 lines above (run `{} explain L2101`).",
            crate::syntax::BINARY_NAME
        );
        0
    } else {
        println!("all healthy — your environment is ready.");
        0
    }
}

/// Probe an executable's `--version` (first line). Offline, local-only.
fn tool_version(bin: &str) -> Option<String> {
    let out = Command::new(bin).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let line = s.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        // Some tools (cc) print version to stderr.
        let e = String::from_utf8_lossy(&out.stderr);
        let l = e.lines().next().unwrap_or("").trim();
        if l.is_empty() {
            None
        } else {
            Some(l.to_string())
        }
    } else {
        Some(line.to_string())
    }
}

/// Find an executable on PATH (no version probe). Returns its resolved path.
fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return Some(candidate);
        }
        // Windows-style extension is out of scope (v1 targets unix shells).
    }
    None
}

/// 1. rustc reachable — Jet's hidden backend (docs/spec/architecture.md).
fn check_toolchain(r: &Render) -> Vec<Check> {
    match tool_version("rustc") {
        Some(v) => vec![Check::ok("rustc", r.version(&v))],
        None => vec![Check::fail(
            "rustc",
            "not found".to_string(),
            "Jet compiles through Rust. Install it from https://rustup.rs, then re-run.",
        )],
    }
}

/// 2. cache/store healthy — exist, writable, not obviously corrupt. With
///    `--fix`, create a missing dir (the only auto-fix doctor performs).
fn check_cache_store(r: &Render, fix: bool) -> Vec<Check> {
    vec![
        dir_health(r, "build cache", crate::build_cache::cache_dir(), fix),
        dir_health(r, "package store", crate::store::store_dir(), fix),
    ]
}

/// Shared dir check: present? writable? Optionally auto-create with `--fix`.
fn dir_health(r: &Render, name: &str, dir: PathBuf, fix: bool) -> Check {
    if !dir.exists() {
        if fix {
            match std::fs::create_dir_all(&dir) {
                Ok(()) => {
                    return Check::ok(
                        name,
                        format!("{} (created)", r.path(&dir)),
                    );
                }
                Err(e) => {
                    return Check::fail(
                        name,
                        format!("{} (could not create: {})", r.path(&dir), e),
                        "check the parent directory's permissions and disk space.",
                    );
                }
            }
        }
        // Missing-but-creatable is only an advisory: Jet creates it on demand.
        return Check::warn(
            name,
            format!("{} (missing)", r.path(&dir)),
            "run `jet doctor --fix` to create it now (Jet also creates it on first build).",
        );
    }
    if !dir.is_dir() {
        return Check::fail(
            name,
            format!("{} (not a directory)", r.path(&dir)),
            "remove the file at that path so Jet can create the directory.",
        );
    }
    if is_writable(&dir) {
        Check::ok(name, r.path(&dir))
    } else {
        Check::fail(
            name,
            format!("{} (not writable)", r.path(&dir)),
            "fix the directory's permissions so Jet can write build artifacts.",
        )
    }
}

/// Probe write access by creating and removing a temp marker file.
fn is_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".jet-doctor-write-{}", std::process::id()));
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// 3. PATH sane — jet itself, rustc, and a C compiler are discoverable.
fn check_path(r: &Render) -> Vec<Check> {
    let mut checks = Vec::new();

    // jet itself: resolve our own executable so a user can confirm which build
    // is on PATH.
    match std::env::current_exe() {
        Ok(p) => checks.push(Check::ok(crate::syntax::BINARY_NAME, r.path(&p))),
        Err(_) => checks.push(Check::warn(
            crate::syntax::BINARY_NAME,
            "could not resolve this executable's path".to_string(),
            "this is unusual but harmless; PATH lookups still work.",
        )),
    }

    match which("rustc") {
        Some(p) => checks.push(Check::ok("rustc on PATH", r.path(&p))),
        None => checks.push(Check::fail(
            "rustc on PATH",
            "not found".to_string(),
            "add rustc to your PATH (rustup puts it in ~/.cargo/bin).",
        )),
    }

    // A C toolchain is needed for FFI builds; absent is only an advisory.
    match cc_path() {
        Some((bin, p)) => checks.push(Check::ok(
            "C compiler on PATH",
            format!("{} ({})", bin, r.path(&p)),
        )),
        None => checks.push(Check::warn(
            "C compiler on PATH",
            "none found (cc/clang/gcc)".to_string(),
            "install a C compiler (clang or gcc) only if you build C-FFI code.",
        )),
    }

    checks
}

/// Find the first available C compiler and its resolved path.
fn cc_path() -> Option<(&'static str, PathBuf)> {
    for bin in ["cc", "clang", "gcc"] {
        if let Some(p) = which(bin) {
            return Some((bin, p));
        }
    }
    None
}

/// 4. LSP wired — reuse the existing language-server health path. We run the
///    same in-process front-end smoke test `jet lsp doctor` performs (lex →
///    parse → sema → format) so doctor reports the LSP as healthy or not.
fn check_lsp(_r: &Render) -> Vec<Check> {
    let src = "fn main() { print(\"hello\"); }\n";
    let (toks, lex_errs) = crate::lexer::lex(src);
    let parse_ok = crate::parser::parse(&toks).is_ok();
    let sema_ok = crate::lsp::check_document("doctor.jet", src).is_empty();
    let fmt_ok = crate::format_source(src).is_ok();
    let healthy = lex_errs.is_empty() && parse_ok && sema_ok && fmt_ok;
    if healthy {
        vec![Check::ok(
            "language server",
            "front-end pipeline responds (run `jet lsp doctor` for detail)".to_string(),
        )]
    } else {
        vec![Check::fail(
            "language server",
            "front-end smoke test failed".to_string(),
            "run `jet lsp doctor` to see which stage failed; this is likely a Jet bug.",
        )]
    }
}

/// 6. FFI section (D-BUILD1) — report the C-FFI/cargo-bridge toolchain so a
///    user debugging an FFI build sees the whole picture: a C compiler, cargo
///    (the bridge that builds FFI rlibs), and pkg-config (link discovery).
fn check_ffi(r: &Render) -> Vec<Check> {
    let mut checks = Vec::new();

    match cc_path() {
        Some((bin, _)) => match tool_version(bin) {
            Some(v) => checks.push(Check::ok(
                "C compiler (cc)",
                format!("{}: {}", bin, r.version(&v)),
            )),
            None => checks.push(Check::ok("C compiler (cc)", bin.to_string())),
        },
        None => checks.push(Check::warn(
            "C compiler (cc)",
            "none found".to_string(),
            "install clang or gcc to build C-FFI code (not needed for pure Jet).",
        )),
    }

    match tool_version("cargo") {
        Some(v) => checks.push(Check::ok("cargo (FFI bridge)", r.version(&v))),
        None => checks.push(Check::warn(
            "cargo (FFI bridge)",
            "not found".to_string(),
            "install cargo (ships with rustup) to build C-FFI dependency rlibs.",
        )),
    }

    match tool_version("pkg-config") {
        Some(v) => checks.push(Check::ok("pkg-config", r.version(&v))),
        None => checks.push(Check::warn(
            "pkg-config",
            "not found".to_string(),
            "install pkg-config if an FFI library resolves its link flags through it.",
        )),
    }

    checks
}

/// 5. registry reachable — networked, so skipped unless `--online` is passed.
///    Offline by default just reports the skip honestly.
fn check_registry(_r: &Render, online: bool) -> Vec<Check> {
    if !online {
        return vec![Check::skip(
            "registry",
            "skipped (offline) — pass `--online` to probe it".to_string(),
        )];
    }
    // Registry support itself ships in M12.2 (see fetch.rs E1207). Until then,
    // an `--online` probe honestly reports there is no registry endpoint to
    // reach rather than faking a network call.
    vec![Check::warn(
        "registry",
        "no registry endpoint configured".to_string(),
        "registry hosting arrives in M12.2; use `--path`/`--git` dependencies for now.",
    )]
}
