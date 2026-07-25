//! #708 / D-DATA-BRIDGE1 / D-DATA-STATUS1: honest Python/R/GPU bridge facts.
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn build_and_run_env(
    dir: &PathBuf,
    name: &str,
    src: &str,
    env: &[(&str, &str)],
) -> (i32, String, String) {
    let path = dir.join(format!("{name}.jet"));
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    fs::write(&rs, &out.rust).unwrap();
    let mut rustc_cmd = Command::new("rustc");
    rustc_cmd.args([
        "--edition",
        "2021",
        rs.to_str().unwrap(),
        "-o",
        bin.to_str().unwrap(),
    ]);
    if let Some(link) = &out.ffi {
        rustc_cmd
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc_cmd
                .arg("-L")
                .arg(format!("dependency={}", deps_dir.display()));
        }
    }
    let rustc = rustc_cmd.output().unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected generated code:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let mut cmd = Command::new(&bin);
    cmd.current_dir(dir);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    (
        out.status.code().unwrap_or(0),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn rscript_on_path() -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join("Rscript").is_file())
}

const BRIDGE_SRC: &str = r#"
use core.data as data

fn show_bridge(step: String) {
    status :: data.status()
    loop row; status {
        if row.step == step {
            print("{row.step} path={row.path} copy={row.copy} ownership={row.ownership} trust={row.trust} fallback={row.fallback} replacement={row.replacement}")
        }
    }
}

fn probe_bridge(name: String) {
    asked := data.require_bridge(name)
    if asked == {
        Ok(_) -> print("{name}: available")
        Err(error) -> print("{name}: {error}")
    }
}

fn run() {
    show_bridge("py.*")
    show_bridge("r.*")
    show_bridge("gpu.*")
    probe_bridge("py")
    probe_bridge("r")
    probe_bridge("gpu")
    bad := data.require_bridge("nope")
    if bad == {
        Ok(_) -> print("nope: unexpectedly ok")
        Err(error) -> print("nope: {error}")
    }
}
"#;

#[test]
fn data_bridges_declare_costs_and_fail_closed() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping data bridges test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_data_bridges_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run_env(&dir, "data_bridges", BRIDGE_SRC, &[]);
    assert_eq!(code, 0, "data bridges program failed: {stderr}\n{stdout}");
    assert!(
        !stdout.contains("bridge-ready"),
        "must not claim bridge-ready facade:\n{stdout}"
    );
    assert!(
        stdout.contains("py.* path=unavailable copy=owned-copy ownership=python-sidecar trust=untrusted-foreign fallback=none"),
        "py status:\n{stdout}"
    );
    assert!(
        stdout.contains("r.* path=unavailable copy=owned-copy ownership=r-sidecar trust=untrusted-foreign fallback=none"),
        "r default status:\n{stdout}"
    );
    assert!(
        stdout.contains("gpu.* path=unavailable copy=device-transfer ownership=device-buffer trust=untrusted-accelerator fallback=none"),
        "gpu status:\n{stdout}"
    );
    assert!(
        stdout.contains("py: Bridge require_bridge: py.* unavailable"),
        "py fail:\n{stdout}"
    );
    assert!(
        stdout.contains("r: Bridge require_bridge: r.* unavailable"),
        "r fail by default:\n{stdout}"
    );
    assert!(
        stdout.contains("gpu: Bridge require_bridge: gpu.* unavailable"),
        "gpu fail:\n{stdout}"
    );
    assert!(
        stdout.contains("nope: InvalidArgument require_bridge"),
        "unknown provider:\n{stdout}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn data_bridges_r_available_with_opt_in_and_rscript() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping R available test (need rustc)");
        return;
    }
    if !rscript_on_path() {
        eprintln!("note: skipping R available test (no Rscript on PATH)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_data_bridges_r_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (code, stdout, stderr) = build_and_run_env(
        &dir,
        "data_bridges_r",
        BRIDGE_SRC,
        &[("JET_DATA_R_BRIDGE", "1")],
    );
    assert_eq!(code, 0, "r available program failed: {stderr}\n{stdout}");
    assert!(
        stdout.contains("r.* path=available copy=owned-copy ownership=r-sidecar"),
        "r available status:\n{stdout}"
    );
    assert!(
        stdout.contains("r: available"),
        "r require_bridge should succeed:\n{stdout}"
    );
    assert!(
        stdout.contains("py: Bridge require_bridge:"),
        "py still fails:\n{stdout}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn data_bridges_r_fails_closed_without_rscript_on_path() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: skipping data bridges PATH test (need rustc)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_data_bridges_nors_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let src = r#"
use core.data as data
fn run() {
    status :: data.status()
    loop row; status {
        if row.step == "r.*" {
            print("{row.step}:{row.path}")
        }
    }
    asked := data.require_bridge("r")
    if asked == {
        Ok(_) -> print("r:ok")
        Err(error) -> print("r:{error}")
    }
}
"#;
    let (code, stdout, stderr) = build_and_run_env(
        &dir,
        "data_bridges_nors",
        src,
        &[("PATH", ""), ("JET_DATA_R_BRIDGE", "1")],
    );
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(stdout.contains("r.*:unavailable"), "{stdout}");
    assert!(stdout.contains("r:Bridge"), "{stdout}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn dossier_data_lens_matches_status_rows() {
    let jet = env!("CARGO_BIN_EXE_jet");
    let out = Command::new(jet)
        .args(["inspect", "dossier", "data", "--json"])
        .output()
        .expect("jet inspect dossier data");
    assert!(
        out.status.success(),
        "dossier data failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\"lens\":\"data\""), "{stdout}");
    assert!(stdout.contains("\"step\":\"py.*\""), "{stdout}");
    assert!(stdout.contains("\"step\":\"r.*\""), "{stdout}");
    assert!(stdout.contains("\"step\":\"gpu.*\""), "{stdout}");
    assert!(stdout.contains("\"copy\":\"owned-copy\""), "{stdout}");
    assert!(stdout.contains("\"fallback\":\"none\""), "{stdout}");
    assert!(stdout.contains("\"path\":\"unavailable\""), "{stdout}");
    assert!(!stdout.contains("bridge-ready"), "{stdout}");
}
