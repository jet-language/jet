//! Tower #438 CLI/runtime smoke for D-WEBAPP1 / D-WEBAUTHOR1.
#![allow(non_snake_case)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn jet_bin() -> PathBuf {
    repo_root().join("target/debug/jet")
}

fn run_jet(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(jet_bin())
        .current_dir(repo_root())
        .args(args)
        .output()
        .expect("spawn jet");
    (
        out.status.code().unwrap_or(1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn web_app_graph_facts_json() {
    let (code, stdout, stderr) = run_jet(&[
        "explain",
        "--web-graph",
        "--json",
        "examples/features/web/web_app.jet",
    ]);
    assert_eq!(code, 0, "stderr={stderr}\nstdout={stdout}");
    assert!(stdout.contains("\"shared_tir\": true"), "{stdout}");
    assert!(stdout.contains("\"path\": \"/\""), "{stdout}");
    assert!(stdout.contains("csp-default"), "{stdout}");
    assert!(stdout.contains("hydration"), "{stdout}");
    assert!(stdout.contains("\"prefix\": \"/api\""), "{stdout}");
}

#[test]
fn web_app_expand_facts_web() {
    let (code, stdout, stderr) = run_jet(&[
        "inspect",
        "expand",
        "--facts",
        "web",
        "examples/features/web/web_app.jet",
    ]);
    assert_eq!(code, 0, "stderr={stderr}\nstdout={stdout}");
    assert!(stdout.contains("web application graph") || stdout.contains("shared TIR"), "{stdout}");
}

#[test]
fn web_app_routes_from_exhaustive() {
    let tmp = repo_root().join("target/webapp-routes-test");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(tmp.join("routes/orders")).unwrap();
    fs::write(
        tmp.join("app.jet"),
        r#"
use core.web as web

fn app() => WebApp {
    return web.app().routes(from: "routes")
}

fn run() {}
"#,
    )
    .unwrap();
    fs::write(tmp.join("routes/index.jet"), "fn page() {}\n").unwrap();
    fs::write(tmp.join("routes/orders/[id].jet"), "fn page() {}\n").unwrap();
    fs::write(tmp.join("routes/_helper.jet"), "fn helper() {}\n").unwrap();

    let (code, stdout, stderr) = run_jet(&[
        "explain",
        "--web-graph",
        "--json",
        tmp.join("app.jet").to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "stderr={stderr}\nstdout={stdout}");
    assert!(stdout.contains("\"path\": \"/\""), "{stdout}");
    assert!(stdout.contains("/orders/:id"), "{stdout}");
    assert!(!stdout.contains("_helper"), "{stdout}");
}

#[test]
fn web_app_run_ok() {
    let (code, stdout, stderr) = run_jet(&["run", "examples/features/web/web_app.jet"]);
    assert_eq!(code, 0, "stderr={stderr}\nstdout={stdout}");
    assert!(stdout.contains("web-app-graph-ok"), "{stdout}");
}

#[test]
fn app_hello_graph() {
    let (code, stdout, stderr) = run_jet(&[
        "explain",
        "--web-graph",
        "--json",
        "examples/features/web/app_hello.jet",
    ]);
    assert_eq!(code, 0, "stderr={stderr}\nstdout={stdout}");
    assert!(stdout.contains("\"handler\": \"home\""), "{stdout}");
    assert!(stdout.contains("csp"), "{stdout}");
}
