//! D-WEBAPP1=D / D-WEBAUTHOR1=D: sema-known application graph facts.

mod common;

use std::fs;
use std::process::Command;

fn check_facts(entry: &str) -> jet::Sema::SemIndexEffectFacts {
    let (diags, _, facts) = jet::Driver::check_file_with_effect_facts(entry, None, false);
    assert!(
        !diags
            .iter()
            .any(|d| d.severity == jet::Diagnostics::Severity::Error),
        "{diags:#?}"
    );
    facts
}

fn codes(entry: &str) -> Vec<String> {
    jet::Driver::check_file_with_effect_facts(entry, None, false)
        .0
        .iter()
        .map(|d| d.code.clone())
        .collect()
}

#[test]
fn example_app_hello_graph_records_policy_and_modes() {
    let path = format!(
        "{}/examples/features/web/app_hello.jet",
        env!("CARGO_MANIFEST_DIR")
    );
    let facts = check_facts(&path);
    let graph = facts.web_app.expect("fn run() App graph");
    assert!(graph.shared_tir);
    assert_eq!(graph.hydration, "dev-overlay");
    assert_eq!(graph.routes.len(), 1);
    assert_eq!(graph.routes[0].path, "/");
    assert_eq!(graph.routes[0].handler, "home");
    assert_eq!(graph.routes[0].render.as_str(), "csr");
    assert_eq!(graph.actions.len(), 1);
    assert_eq!(graph.actions[0].name, "save");
    assert_eq!(graph.policy.security, vec!["csp".to_string()]);
    assert_eq!(graph.policy.assets, vec!["public".to_string()]);
    assert_eq!(graph.policy.a11y, vec!["wcag".to_string()]);
    assert_eq!(graph.policy.adapters, vec!["node".to_string()]);
}

#[test]
fn routes_from_expands_convention_files() {
    let root = common::unique_tmp("jet_app_routes_from");
    fs::create_dir_all(root.join("routes/about")).unwrap();
    fs::write(
        root.join("app.jet"),
        r#"use core.web as web
fn about_page() => WebPage { return web.page("About", "us") }
fn run() => App { return web.app().routes(from: "routes").ssr() }
"#,
    )
    .unwrap();
    fs::write(
        root.join("routes/index.jet"),
        r#"fn page() {}
"#,
    )
    .unwrap();
    fs::write(
        root.join("routes/about/page.jet"),
        r#"fn page() {}
"#,
    )
    .unwrap();
    let facts = check_facts(root.join("app.jet").to_str().unwrap());
    let graph = facts.web_app.unwrap();
    let paths: Vec<_> = graph.routes.iter().map(|r| r.path.as_str()).collect();
    assert!(paths.contains(&"/"));
    assert!(paths.contains(&"/about"));
    assert!(graph.routes.iter().all(|r| r.render.as_str() == "ssr"));
}

#[test]
fn collision_and_stray_and_dynamic_diagnose() {
    let root = common::unique_tmp("jet_app_diags");
    fs::create_dir_all(root.join("routes")).unwrap();
    fs::write(root.join("routes/index.jet"), "fn page() {}\n").unwrap();
    fs::write(root.join("routes/stray.jet"), "fn helper() {}\n").unwrap();
    fs::write(
        root.join("collision.jet"),
        r#"use core.web as web
#Target(Web)
fn home() => WebPage { return web.page("h", "b") }
fn run() => App { return web.app().route("/", home).routes(from: "routes").csr() }
"#,
    )
    .unwrap();
    let collision = codes(root.join("collision.jet").to_str().unwrap());
    assert!(collision.contains(&"E2807".to_string()));

    fs::write(
        root.join("stray.jet"),
        r#"use core.web as web
#Target(Web)
fn run() => App { return web.app().routes(from: "routes").csr() }
"#,
    )
    .unwrap();
    let stray = codes(root.join("stray.jet").to_str().unwrap());
    assert!(stray.contains(&"E2806".to_string()));

    fs::write(
        root.join("dynamic.jet"),
        r#"use core.web as web
#Target(Web)
fn pick() => Int { return 1 }
fn run() => App {
    return web.app().route("/", pick()).csr()
}
"#,
    )
    .unwrap();
    let dynamic = codes(root.join("dynamic.jet").to_str().unwrap());
    assert!(dynamic.contains(&"E2810".to_string()));
}

#[test]
fn render_modes_mount_island_and_shared_tir() {
    let root = common::unique_tmp("jet_app_modes");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("app.jet"),
        r#"use core.web as web
#Target(Web)
fn home() => WebPage { return web.page("H", "b") }
fn dash() => WebPage { return web.page("D", "b") }
fn plugins(prefix: String) {}
fn run() => App {
    return web.app()
        .route("/", home)
        .ssg()
        .route("/dash", dash)
        .stream()
        .island()
        .mount("/plugins", plugins, "Net", "csrf")
        .security("csp")
        .cache("revalidate")
        .split("dash")
        .hydration_release()
}
"#,
    )
    .unwrap();
    let facts = check_facts(root.join("app.jet").to_str().unwrap());
    let graph = facts.web_app.unwrap();
    assert!(graph.shared_tir);
    assert_eq!(graph.hydration, "release-keep-server");
    assert_eq!(graph.mounts.len(), 1);
    assert_eq!(graph.mounts[0].prefix, "/plugins");
    assert_eq!(graph.mounts[0].effects, vec!["Net".to_string()]);
    assert_eq!(graph.mounts[0].security, vec!["csrf".to_string()]);
    assert!(graph.policy.cache.contains(&"revalidate".to_string()));
    assert!(graph.policy.split.contains(&"dash".to_string()));
    let modes: Vec<_> = graph.routes.iter().map(|r| r.render.as_str()).collect();
    assert!(modes.contains(&"ssg") || modes.contains(&"stream") || modes.contains(&"island"));
}

#[test]
fn expand_web_lens_and_explain_web_graph_json() {
    let path = format!(
        "{}/examples/features/web/app_hello.jet",
        env!("CARGO_MANIFEST_DIR")
    );
    let jet = env!("CARGO_BIN_EXE_jet");
    let expand = Command::new(jet)
        .args([
            "inspect",
            "expand",
            "--facts",
            "web",
            &path,
        ])
        .output()
        .unwrap();
    assert!(expand.status.success(), "{}", String::from_utf8_lossy(&expand.stderr));
    let stdout = String::from_utf8_lossy(&expand.stdout);
    assert!(stdout.contains("web —"));
    assert!(stdout.contains("hydration: dev-overlay (shared TIR: true)"));
    assert!(stdout.contains("  / [csr] -> home"));

    let explain = Command::new(&jet)
        .args(["explain", "--web-graph", &path, "--json"])
        .output()
        .unwrap();
    assert!(explain.status.success(), "{}", String::from_utf8_lossy(&explain.stderr));
    let json = String::from_utf8_lossy(&explain.stdout);
    assert!(json.contains("\"shared_tir\": true"));
    assert!(json.contains("\"hydration\": \"dev-overlay\""));
    assert!(json.contains("\"render\": \"csr\""));
}
