//! End-to-end tests for engine-verb process dispatch (D-JPK-DISPATCH1=B,
//! card c9jetpackgates, Epoch 4 Phase A1).
//!
//! `jet env` execs the `jetpack` binary by name instead of linking it
//! in-process. To exercise the "engine missing" / "version skew" / exit-code
//! / `--json` paths without disturbing the real co-built `jetpack` binary,
//! each test copies the compiled `jet` binary into an isolated scratch
//! directory (so `EngineDispatch::find_engine_binary`'s "next to the running
//! exe" check finds nothing) and points `PATH` at a scratch dir the test
//! controls.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::Duration;

mod common;
use common::Scratch;

fn real_jet() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

/// Copy the real `jet` binary into an isolated directory with no `jetpack`
/// alongside it, so engine dispatch must fall through to `PATH`.
fn isolated_jet(dir: &Scratch) -> PathBuf {
    let dest = dir.join("jet");
    fs::copy(real_jet(), &dest).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = fs::metadata(&dest).unwrap().permissions();
        perm.set_mode(0o755);
        fs::set_permissions(&dest, perm).unwrap();
    }
    dest
}

#[cfg(unix)]
fn write_script(path: &PathBuf, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut perm = fs::metadata(path).unwrap().permissions();
    perm.set_mode(0o755);
    fs::set_permissions(path, perm).unwrap();
}

fn output_with_retry(cmd: &mut Command) -> Output {
    let mut last = None;
    for attempt in 0..8 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(20 * attempt));
        }
        match cmd.output() {
            Ok(out) => return out,
            Err(e) if e.kind() == io::ErrorKind::ExecutableFileBusy => last = Some(e),
            Err(e) => panic!("engine dispatch command failed: {e}"),
        }
    }
    panic!("engine dispatch command stayed busy: {}", last.unwrap());
}

#[cfg(unix)]
#[test]
fn missing_engine_binary_is_e1228() {
    let jet_dir = Scratch::new("missing-jet");
    let jet_bin = isolated_jet(&jet_dir);
    let empty_path_dir = Scratch::new("missing-path");

    let out = output_with_retry(
        Command::new(&jet_bin)
            .arg("env")
            .env("PATH", &empty_path_dir.path),
    );

    assert_eq!(out.status.code(), Some(1), "engine-missing is USER_ERROR");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1228"),
        "expected E1228 in stderr: {stderr}"
    );
    assert!(
        stderr.contains("jetpack"),
        "names the missing engine: {stderr}"
    );
}

#[cfg(unix)]
#[test]
fn version_skew_is_e1227() {
    let jet_dir = Scratch::new("skew-jet");
    let jet_bin = isolated_jet(&jet_dir);
    let path_dir = Scratch::new("skew-path");
    let fake_jetpack = path_dir.join("jetpack");
    // Answers the handshake with a version that can never equal jet's real
    // `CARGO_PKG_VERSION`.
    write_script(
        &fake_jetpack,
        r#"if [ "$1" = "--engine-protocol" ]; then echo "0.0.0-skew-test"; exit 0; fi
exit 0"#,
    );

    let out = output_with_retry(
        Command::new(&jet_bin)
            .arg("env")
            .env("PATH", &path_dir.path),
    );

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1227"),
        "expected E1227 in stderr: {stderr}"
    );
    assert!(
        stderr.contains("0.0.0-skew-test"),
        "names the mismatched version: {stderr}"
    );
}

#[cfg(unix)]
#[test]
fn engine_too_old_for_handshake_is_e1227() {
    // An engine binary that doesn't understand `--engine-protocol` at all
    // (predates this gate) must not be treated as compatible.
    let jet_dir = Scratch::new("oldengine-jet");
    let jet_bin = isolated_jet(&jet_dir);
    let path_dir = Scratch::new("oldengine-path");
    let fake_jetpack = path_dir.join("jetpack");
    write_script(
        &fake_jetpack,
        "echo \"jetpack: unknown flag $1\" >&2\nexit 2",
    );

    let out = output_with_retry(
        Command::new(&jet_bin)
            .arg("env")
            .env("PATH", &path_dir.path),
    );

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E1227"),
        "expected E1227 in stderr: {stderr}"
    );
}

#[cfg(unix)]
#[test]
fn exit_code_propagates_from_engine() {
    let jet_bin_version = env!("CARGO_PKG_VERSION");
    let jet_dir = Scratch::new("exitcode-jet");
    let jet_bin = isolated_jet(&jet_dir);
    let path_dir = Scratch::new("exitcode-path");
    let fake_jetpack = path_dir.join("jetpack");
    write_script(
        &fake_jetpack,
        &format!(
            r#"if [ "$1" = "--engine-protocol" ]; then echo "{jet_bin_version}"; exit 0; fi
exit 37"#
        ),
    );

    let out = output_with_retry(
        Command::new(&jet_bin)
            .arg("env")
            .env("PATH", &path_dir.path),
    );

    assert_eq!(
        out.status.code(),
        Some(37),
        "jet must return the engine's exit code verbatim"
    );
}

#[cfg(unix)]
#[test]
fn json_flag_forwarded_verbatim_to_engine() {
    let jet_bin_version = env!("CARGO_PKG_VERSION");
    let jet_dir = Scratch::new("json-jet");
    let jet_bin = isolated_jet(&jet_dir);
    let path_dir = Scratch::new("json-path");
    let fake_jetpack = path_dir.join("jetpack");
    let log = path_dir.join("argv.log");
    write_script(
        &fake_jetpack,
        &format!(
            r#"if [ "$1" = "--engine-protocol" ]; then echo "{jet_bin_version}"; exit 0; fi
echo "$@" > "{log}"
exit 0"#,
            log = log.display()
        ),
    );

    let out = output_with_retry(
        Command::new(&jet_bin)
            .arg("env")
            .arg("--json")
            .env("PATH", &path_dir.path),
    );

    assert_eq!(out.status.code(), Some(0));
    let logged = fs::read_to_string(&log).unwrap_or_default();
    assert!(
        logged.contains("--json"),
        "--json must reach the engine verbatim, got argv: {logged}"
    );
}

/// Regression: engine-verb dispatch must not disturb the existing typo
/// suggestion path (E2101) for a near-miss of a known command like `env`.
#[test]
fn typo_of_engine_verb_still_suggests_it() {
    let out = output_with_retry(Command::new(real_jet()).arg("envx"));
    assert_eq!(out.status.code(), Some(2), "unknown command is USAGE");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("E2101"),
        "expected E2101 in stderr: {stderr}"
    );
    assert!(
        stderr.contains("env"),
        "did-you-mean should suggest `env`: {stderr}"
    );
}

/// The real, co-built `jetpack` binary must satisfy its own handshake (a
/// same-toolchain `jet env` never hits E1227/E1228). This doesn't drive an
/// actual shell (no TTY, no manifest) so `jetpack enter` fails for its own
/// reasons — the point is that dispatch gets past the handshake cleanly.
#[test]
fn real_engine_pair_passes_handshake() {
    let out = output_with_retry(
        Command::new(real_jet())
            .arg("env")
            .env("NO_COLOR", "1")
            .stdin(std::process::Stdio::null()),
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("E1227") && !stderr.contains("E1228"),
        "the co-built jet/jetpack pair must pass their own handshake: {stderr}"
    );
}
