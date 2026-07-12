//! U15 fleets (D-JPK-FLEET1=A, card c9jetpackgates, Epoch 4 Phase B).
//!
//! A `module fleet.<name> { hosts: { <host>: system.<name>.{ … } } }` is parsed,
//! captured, and cross-checked now; ssh/closure rollout is gated on single-host
//! jetos realization (Phase D). These tests cover the two surfaces:
//!   * the modeval field-check / cross-check diagnostics (E1242/E1244/E1245),
//!     driven through the library `evaluate_env`;
//!   * the `jetpack push <fleet>` engine verb's honest gated notice (E1243),
//!     driven through the compiled `jetpack` binary.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use jetpack::ModuleEval::evaluate_env;

fn render(src: &str) -> (String, String) {
    let dir = std::env::temp_dir();
    let err = evaluate_env(src, &dir).expect_err("expected a diagnostic");
    let rendered = jet::Diagnostics::render_all("env.jet", src, std::slice::from_ref(&err));
    (err.code.to_string(), rendered)
}

#[test]
fn fleet_host_unknown_system_is_e1242() {
    let src = "module fleet.prod { hosts: { web1: system.ghost } }";
    let (code, rendered) = render(src);
    assert_eq!(code, "E1242");
    assert!(
        rendered.contains("host `web1` names an unknown system `ghost`"),
        "{rendered}"
    );
}

#[test]
fn fleet_unknown_field_is_e1244() {
    let src = "module fleet.prod { region: \"us\" }";
    let (code, _) = render(src);
    assert_eq!(code, "E1244");
}

#[test]
fn fleet_missing_hosts_is_e1245() {
    let src = "module fleet.prod { }";
    let (code, _) = render(src);
    assert_eq!(code, "E1245");
}

/// A well-formed fleet captures cleanly, cross-checks against its system, and
/// records each host's raw `.{ … }` override text.
#[test]
fn fleet_valid_captures_hosts_and_overrides() {
    let src = r#"
module system.web { target: linux.x64 }
module fleet.prod {
    hosts: {
        web1: system.web.{ region: "us-east" },
        web2: system.web,
    }
}
"#;
    let plan = evaluate_env(src, &std::env::temp_dir()).unwrap();
    assert_eq!(plan.fleets.len(), 1);
    let fleet = &plan.fleets[0];
    assert_eq!(fleet.name, "prod");
    assert_eq!(fleet.hosts.len(), 2);
    assert_eq!(fleet.hosts[0].system, "web");
    assert_eq!(
        fleet.hosts[0].overrides.as_deref(),
        Some("{ region: \"us-east\" }")
    );
    assert_eq!(fleet.hosts[1].overrides, None);
}

/// I5: the committed typed-fleet fixture is the executable spec — it parses,
/// field-checks, and cross-checks clean, capturing three hosts against the
/// `web` system.
#[test]
fn committed_fleet_example_field_checks_clean() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jetpack-typed/fleet.jet");
    let src = fs::read_to_string(&path).unwrap();
    let dir = path.parent().unwrap();
    let plan = evaluate_env(&src, dir).unwrap();
    assert_eq!(plan.systems.len(), 1);
    assert_eq!(plan.systems[0].name, "web");
    assert_eq!(plan.fleets.len(), 1);
    assert_eq!(plan.fleets[0].name, "prod");
    let hosts: Vec<&str> = plan.fleets[0]
        .hosts
        .iter()
        .map(|h| h.name.as_str())
        .collect();
    assert_eq!(hosts, vec!["web1", "web2", "web3"]);
    assert!(plan.fleets[0].hosts.iter().all(|h| h.system == "web"));
}

// ── `jetpack push` engine verb ──────────────────────────────────────────────

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "fleet-it-{tag}-{nanos}-{:?}",
            std::thread::current().id()
        ));
        fs::create_dir_all(&path).unwrap();
        Scratch { path }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write_fleet_project(dir: &std::path::Path) {
    fs::write(
        dir.join("env.jet"),
        "module system.web { target: linux.x64 }\n\
         module fleet.prod {\n    hosts: { web1: system.web.{ region: \"us-east\" } }\n}\n",
    )
    .unwrap();
}

fn jetpack() -> Command {
    Command::new(env!("CARGO_BIN_EXE_jetpack"))
}

/// `jetpack push <fleet>` on a valid fleet emits the honest E1243 gated notice
/// (deployment waits on jetos realization) and exits non-zero — it never fakes
/// a deploy.
#[test]
fn push_valid_fleet_is_gated_e1243() {
    let proj = Scratch::new("push-gated");
    write_fleet_project(&proj.path);
    let out = jetpack()
        .arg("push")
        .arg("prod")
        .current_dir(&proj.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(2),
        "gated push exits non-zero: {stderr}"
    );
    assert!(stderr.contains("E1243"), "expected E1243: {stderr}");
    assert!(stderr.contains("prod"), "names the fleet: {stderr}");
    assert!(
        stderr.contains("web1") && stderr.contains("system.web"),
        "lists the validated hosts: {stderr}"
    );
}

/// `jetpack push` naming a fleet that doesn't exist lists the declared fleets.
#[test]
fn push_unknown_fleet_is_friendly() {
    let proj = Scratch::new("push-unknown");
    write_fleet_project(&proj.path);
    let out = jetpack()
        .arg("push")
        .arg("staging")
        .current_dir(&proj.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr.contains("no fleet `staging`"), "{stderr}");
    assert!(stderr.contains("prod"), "lists declared fleets: {stderr}");
}

/// A fleet whose host references an unknown system fails the cross-check (E1242)
/// through the engine verb, before any gating.
#[test]
fn push_fleet_with_bad_host_is_e1242() {
    let proj = Scratch::new("push-bad-host");
    fs::write(
        proj.path.join("env.jet"),
        "module fleet.prod { hosts: { web1: system.ghost } }\n",
    )
    .unwrap();
    let out = jetpack()
        .arg("push")
        .arg("prod")
        .current_dir(&proj.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr.contains("E1242"),
        "cross-check fires first: {stderr}"
    );
}
