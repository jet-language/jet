//! Engine-verb process dispatch (D-JPK-DISPATCH1=B, card c9jetpackgates, A1).
//!
//! `jet` never links the Jetpack engine in-process for engine verbs (`env`
//! today; `dev`/`services`/`secrets`/`image`/`push`/… as later gates ship
//! them, and `jetos` verbs once that binary exists). It execs the engine
//! binary by name — the same git/kubectl-style dispatch the D-DX5
//! `jet-<cmd>` PATH fallback already uses for third-party plugins, just
//! resolved against a known engine binary name first.
//!
//! Contract:
//!   - exit code: the engine's exit code is `jet`'s exit code, verbatim.
//!   - stdout/stderr/stdin: inherited — the child owns the terminal.
//!   - `--json` and every other flag: forwarded verbatim, never interpreted.
//!   - environment: inherited (`std::process::Command`'s default).
//!   - before running the real verb, `jet` queries `<engine> --engine-protocol`
//!     and compares the reply to its own version (E1227 on mismatch).
//!   - a missing engine binary is E1228, not a generic "command not found".

use std::path::PathBuf;
use std::process::Command;

use jet::ExitCodes;

/// Find `name` (e.g. `"jetpack"`) as an engine binary: check the directory
/// the running `jet` binary lives in first (the common case — installers and
/// `cargo build` place every `[[bin]]` in the same directory), then fall back
/// to `PATH` (D-DX5's mechanism), so a `jetpack` shadowed on `PATH` by an
/// unrelated install never wins over the one `jet` shipped with.
pub fn find_engine_binary(name: &str) -> Option<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            dirs.push(dir.to_path_buf());
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    find_engine_binary_in(dirs.into_iter(), name)
}

/// Pure search over an explicit directory list — the testable core of
/// [`find_engine_binary`].
fn find_engine_binary_in(dirs: impl Iterator<Item = PathBuf>, name: &str) -> Option<PathBuf> {
    for dir in dirs {
        let candidate = dir.join(exe_name(name));
        if candidate.is_file() && is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(windows)]
fn exe_name(name: &str) -> String {
    format!("{name}.exe")
}
#[cfg(not(windows))]
fn exe_name(name: &str) -> String {
    name.to_string()
}

#[cfg(unix)]
fn is_executable(p: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}
#[cfg(not(unix))]
fn is_executable(p: &std::path::Path) -> bool {
    p.is_file()
}

/// E1228 `engine-missing`: rendered exactly like every other `jet` teaching
/// diagnostic (docs/spec/diagnostics.md).
fn missing_engine_message(engine: &str, verb: &str) -> String {
    format!(
        "Error [E1228]: `{verb}` needs the `{engine}` engine, which isn't installed\n \
         Why: `{verb}` is a jetpack engine verb — {bin} execs `{engine}` for it rather than \
         building package-manager logic into the compiler\n \
         Fix: install the matching Jet toolchain (the `{engine}` binary ships alongside `{bin}`)\n",
        bin = jet::Syntax::BINARY_NAME,
    )
}

/// E1227 `engine-version-skew`: `jet` and the engine binary disagree on
/// protocol version. `probe_output` is the trimmed stdout of
/// `<engine> --engine-protocol`; `None` covers "the probe failed to run or
/// returned nothing usable" (an engine too old to know the handshake).
fn version_skew_message(engine: &str, jet_version: &str, engine_version: Option<&str>) -> String {
    let bin = jet::Syntax::BINARY_NAME;
    match engine_version {
        Some(ev) => format!(
            "Error [E1227]: `{bin}` {jet_version} and `{engine}` {ev} disagree\n \
             Why: `{bin}` and `{engine}` ship as one toolchain and must match exactly, or the \
             engine may not understand what `{bin}` sends it\n \
             Fix: use matching `{bin}`/`{engine}` versions — reinstall the toolchain so both \
             binaries come from the same release\n"
        ),
        None => format!(
            "Error [E1227]: `{bin}` {jet_version} and `{engine}` disagree (no protocol reply)\n \
             Why: `{bin}` and `{engine}` ship as one toolchain and must match exactly; this \
             `{engine}` didn't answer the version handshake, so it predates this protocol\n \
             Fix: use matching `{bin}`/`{engine}` versions — reinstall the toolchain so both \
             binaries come from the same release\n"
        ),
    }
}

/// Query `bin`'s protocol version via the hidden `--engine-protocol` flag.
/// `None` on any failure (missing flag support, non-zero exit, unparseable
/// output) — treated as skew by the caller, never as "compatible".
fn query_engine_protocol(bin: &PathBuf) -> Option<String> {
    let out = Command::new(bin)
        .arg(jet::Syntax::ENGINE_PROTOCOL_FLAG)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    Some(s.to_string())
}

/// Dispatch `argv` to the `engine` binary (e.g. `"jetpack"`): find it, check
/// its protocol version, then exec it with inherited stdio. Returns the exit
/// code `jet` itself should return — never panics, never prints past what the
/// diagnostics above already print.
pub fn dispatch(engine: &str, verb: &str, argv: &[String]) -> i32 {
    let Some(bin) = find_engine_binary(engine) else {
        eprint!("{}", missing_engine_message(engine, verb));
        return ExitCodes::USER_ERROR;
    };

    let jet_version = env!("CARGO_PKG_VERSION");
    match query_engine_protocol(&bin) {
        Some(ev) if ev == jet_version => {}
        Some(ev) => {
            eprint!("{}", version_skew_message(engine, jet_version, Some(&ev)));
            return ExitCodes::USER_ERROR;
        }
        None => {
            eprint!("{}", version_skew_message(engine, jet_version, None));
            return ExitCodes::USER_ERROR;
        }
    }

    match Command::new(&bin).args(argv).status() {
        Ok(status) => status.code().unwrap_or(ExitCodes::USER_ERROR),
        Err(e) => {
            eprintln!("error: couldn't run `{}`: {}", bin.display(), e);
            ExitCodes::USER_ERROR
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jet_engine_dispatch_unit_{tag}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(unix)]
    fn make_executable(p: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = fs::metadata(p).unwrap().permissions();
        perm.set_mode(0o755);
        fs::set_permissions(p, perm).unwrap();
    }

    #[test]
    fn find_engine_binary_in_finds_first_match() {
        let d1 = scratch("d1");
        let d2 = scratch("d2");
        let bin2 = d2.join("jetpack");
        fs::write(&bin2, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        make_executable(&bin2);
        let found = find_engine_binary_in(vec![d1, d2.clone()].into_iter(), "jetpack");
        assert_eq!(found, Some(bin2));
    }

    #[test]
    fn find_engine_binary_in_skips_non_executable() {
        let d1 = scratch("noexec");
        let bin1 = d1.join("jetpack");
        fs::write(&bin1, "not executable").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = fs::metadata(&bin1).unwrap().permissions();
            perm.set_mode(0o644);
            fs::set_permissions(&bin1, perm).unwrap();
        }
        #[cfg(unix)]
        assert_eq!(
            find_engine_binary_in(vec![d1].into_iter(), "jetpack"),
            None
        );
    }

    #[test]
    fn find_engine_binary_in_none_when_absent() {
        let d1 = scratch("absent");
        assert_eq!(
            find_engine_binary_in(vec![d1].into_iter(), "jetpack"),
            None
        );
    }

    #[test]
    fn missing_engine_message_names_e1228() {
        let msg = missing_engine_message("jetpack", "env");
        assert!(msg.contains("E1228"));
        assert!(msg.contains("jetpack"));
    }

    #[test]
    fn version_skew_message_names_e1227() {
        let msg = version_skew_message("jetpack", "1.0.0", Some("2.0.0"));
        assert!(msg.contains("E1227"));
        assert!(msg.contains("1.0.0"));
        assert!(msg.contains("2.0.0"));
    }
}
