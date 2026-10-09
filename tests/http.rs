use std::fs;
use std::path::{Path, PathBuf};

use jet::Interpreter::RunOutcome;

mod common;

struct Output {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

fn on_large_stack(work: impl FnOnce() + Send) {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn_scoped(scope, work)
            .unwrap()
            .join()
            .unwrap();
    });
}

fn run_dev(path: &Path, use_interpreter: bool) -> Output {
    let shown = path.to_string_lossy().into_owned();
    match crate::common::dev_run(&shown, false, use_interpreter) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => Output {
            stdout,
            stderr,
            exit_code,
        },
        RunOutcome::Problems(diags) => {
            panic!("{} failed: {diags:?}", path.display())
        }
    }
}

fn example_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("Examples/features/net/{name}.jet"))
}

#[test]
fn evalhost_http_readiness_uses_shared_scheduler_poller() {
    let runtime = include_str!("../crates/jet-evalhost/src/net_http_rt.rs");
    let scheduler = include_str!("../crates/jet-codegen/src/Prelude/Scheduler.rs");
    let net = include_str!("../crates/jet-codegen/src/Prelude/CoreLib/Top/NetHTTP.rs");
    for symbol in [
        "jet_scheduler_io_wait",
        "jet_scheduler_tcp_listener_io_wait",
        "jet_scheduler_tcp_stream_ready_wait",
        "jet_scheduler_udp_io_wait",
    ] {
        assert!(
            runtime.contains(symbol),
            "evalhost runtime does not import {symbol}"
        );
        assert!(
            scheduler.contains(&format!("pub fn {symbol}")),
            "Prelude scheduler does not define {symbol}"
        );
        assert!(
            !runtime.contains(&format!("fn {symbol}")),
            "evalhost runtime still defines local {symbol}"
        );
    }
    #[cfg(unix)]
    assert!(
        net.contains("jet_scheduler_tcp_stream_ready_wait(stream, read, write, operation)"),
        "TCP stream waits must use the shared raw-fd poller"
    );
    assert!(!runtime.contains("fn jet_scheduler_tcp_stream_io_wait"));
    assert!(!runtime.contains("thread::sleep"));
}

fn assert_http_example_matches_all_tiers(name: &str) {
    let path = example_path(name);
    let source = fs::read_to_string(&path).unwrap();
    let expected = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("Examples/features/expected/net/{name}.out")),
    )
    .unwrap();

    let interpreted = run_dev(&path, true);
    assert_eq!(interpreted.stdout, expected, "{name} interpreter stdout");
    assert_eq!(interpreted.stderr, "", "{name} interpreter stderr");
    assert_eq!(interpreted.exit_code, 0, "{name} interpreter exit code");

    let default = run_dev(&path, false);
    assert_eq!(default.stdout, expected, "{name} default stdout");
    assert_eq!(default.stderr, "", "{name} default stderr");
    assert_eq!(default.exit_code, 0, "{name} default exit code");

    let (code, aot_stdout, aot_stderr) =
        common::build_and_run(&format!("jet_{name}"), name, &source);
    assert_eq!(code, 0, "{name} AOT exit code: {aot_stderr}");
    assert_eq!(aot_stdout, expected, "{name} AOT stdout");
    assert_eq!(aot_stderr, "", "{name} AOT stderr");
}
#[test]
fn mounted_routes_keep_declaration_order_at_the_call_site() {
    on_large_stack(|| {
        let path = example_path("http_route_markers");
        let original = fs::read_to_string(&path).unwrap();
        let grouped = original.replacen(
            "struct Jokes {",
            "marker ReadJoke = #[server.Get(\"/jokes/:id\"), Inline]\n\nstruct Jokes {",
            1,
        ).replacen("#server.Get(\"/jokes/:id\")", "#ReadJoke", 1);
        for source in [None, Some(grouped.as_str())] {
            let (diagnostics, bundle, _) = jet::Driver::check_file_with_effect_facts(
                path.to_str().unwrap(),
                source.map(|source| (path.as_path(), source)),
                false,
            );
            assert!(
                !diagnostics.iter().any(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error),
                "route example failed: {diagnostics:#?}"
            );
            let bundle = bundle.expect("checked route bundle");
            let run = bundle.modules[bundle.entry].items.iter().find_map(|item| {
                match item {
                    jet::AST::Item::Func(function) if function.name == "run" => Some(function),
                    _ => None,
                }
            }).expect("route example entry");
            let mut registrations = Vec::new();
            for statement in &run.body {
                statement.for_each_expr(|expression: &jet::AST::Expr| {
                    if let jet::AST::Expr::MethodCall { method, args, .. } = expression {
                        if matches!(method.as_str(), "get" | "post") && args.len() == 2 {
                            if let jet::AST::Expr::Str(parts, _) = &args[0].expr {
                                if let [jet::AST::StrPart::Lit(path)] = parts.as_slice() {
                                    registrations.push((method.clone(), path.clone()));
                                }
                            }
                        }
                    }
                });
            }
            assert_eq!(registrations, [
                ("get", "/before/:name"),
                ("get", "/jokes/:id"),
                ("post", "/jokes"),
                ("get", "/order/:name"),
                ("get", "/order/fixed"),
                ("get", "/order/after"),
                ("get", "/before/fixed"),
            ].map(|(method, path)| (method.to_string(), path.to_string())));
        }
    });
}

#[test]
fn http_examples_match_interpreter_default_and_aot() {
    if !common::have_rustc() {
        return;
    }

    on_large_stack(|| {
        assert_http_example_matches_all_tiers("http_readiness");
        assert_http_example_matches_all_tiers("http_client");
        assert_http_example_matches_all_tiers("http_route_markers");
        assert_http_example_matches_all_tiers("http_routes");
    });
}

#[test]
fn mounted_http_route_examples_match_all_tiers() {
    assert!(common::have_rustc(), "HTTP route AOT proof requires rustc");
    on_large_stack(|| {
        assert_http_example_matches_all_tiers("http_route_markers");
        assert_http_example_matches_all_tiers("http_routes");
    });
}

#[test]
fn auto_derived_http_inputs_keep_endpoint_schemas_on_all_tiers() {
    assert!(common::have_rustc(), "HTTP schema AOT proof requires rustc");
    on_large_stack(|| {
        let source = r#"
use core.http.server as server
use core.web as web
struct JokeID { value: Int }
struct Jokes {
    #server.Get("/mounted/:id")
    fn joke(self, id: JokeID) -> HTTPResponse HTTPError! {
        Ok(server.response(200, "joke={id.value}"))
    }
}
fn run() {
    mux :: server.mux()
    mux.get("/direct/:id", (id: JokeID) -> Ok(server.response(200, "joke={id.value}")))
    mux.mount(Jokes{})
    schema :: web.openapi(mux)
    print(schema.contains("\"value\"") && schema.contains("/mounted/") && schema.contains("/direct/"))
}
"#;
        let scratch = common::Scratch::new("http_auto_decode");
        let path = scratch.join("main.jet");
        fs::write(&path, source).unwrap();
        for interpreter in [true, false] {
            let output = run_dev(&path, interpreter);
            assert_eq!(output.stdout, "true\n", "HTTP schema tier interpreter={interpreter}");
            assert_eq!(output.stderr, "");
            assert_eq!(output.exit_code, 0);
        }
        let (code, stdout, stderr) = common::build_and_run("http_auto_decode", "http_auto_decode", source);
        assert_eq!(code, 0, "HTTP schema AOT: {stderr}");
        assert_eq!(stdout, "true\n");
        assert_eq!(stderr, "");
    });
}

#[test]
fn web_battery_mounted_routes_execute_with_shared_state() {
    if std::env::var_os("JET_HTTP_BATTERY_CHILD").is_none() {
        let scratch = common::Scratch::new("http-web-battery");
        let roots = jetpack::Store::Roots { root: scratch.join("hangar"), dev_mode: true };
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/jetpack/batteries/web");
        let table = jetpack::RefSpec::SourceTable::from_decls([(
            "battery".to_string(),
            format!("path:{}", source.display()),
            jetpack::RefSpec::ProviderKind::Core,
        )]);
        let spec = jetpack::RefSpec::classify_in("web@battery", &table).unwrap();
        let store_dir = roots.hangar_dir();
        fs::create_dir_all(&store_dir).unwrap();
        let context = jetpack::Provider::Ctx {
            fixtures: None, store_dir: &store_dir, offline: true,
            project_dir: None, nix_index: None, nix_roots: None,
            allow_local_nix_catalog: false,
        };
        jetpack::Store::realize_verified(&roots, &context,
            jetpack::Store::RealizeRequest::Package { spec: &spec, table: &table })
            .expect("realize the actual first-party web library offline");
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "web_battery_mounted_routes_execute_with_shared_state", "--nocapture", "--test-threads=1"])
            .env("JET_HTTP_BATTERY_CHILD", "1")
            .env("JETPACK_ROOT", &roots.root)
            .output().unwrap();
        assert!(output.status.success(), "web battery parity child failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        return;
    }
    on_large_stack(|| {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Examples/features/web/battery/run.jet");
        let interpreted = run_dev(&path, true);
        assert_eq!(interpreted.exit_code, 0);
        assert_eq!(interpreted.stderr, "");
        for expected in [
            "home:200\n", "redirect:303:/orders\n", "create:201\n", "<p>cache=1</p>",
            "protected-failure-http:401\n", "detail:200\n", "login:200:cookie=true\n", "protected:200\n",
        ] {
            assert!(interpreted.stdout.contains(expected), "web battery missing {expected:?}: {}", interpreted.stdout);
        }
        // The default tier is `jet run` (build, then execute). It resolves the
        // realized library through this child's JETPACK_ROOT, so it is spawned
        // here rather than through `common::dev_run`, which isolates the store.
        let scratch = common::Scratch::new("http-web-battery-run");
        let compiled = std::process::Command::new(env!("CARGO_BIN_EXE_jet"))
            .arg("run")
            .arg(&path)
            .env("JET_STORE_DIR", scratch.join("cache"))
            .env("JETPACK_ENV", "1")
            .env("NO_COLOR", "1")
            .stdin(std::process::Stdio::null())
            .output()
            .expect("spawn default `jet run`");
        assert_eq!(compiled.status.code(), Some(0), "web battery default tier: {}", String::from_utf8_lossy(&compiled.stderr));
        assert_eq!(String::from_utf8_lossy(&compiled.stderr), "");
        assert_eq!(String::from_utf8_lossy(&compiled.stdout), interpreted.stdout);
    });
}
