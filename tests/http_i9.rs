use std::fs;

use jet::Interpreter::dev_iteration;
use jet_foundation::JitBackend::RunOutcome;

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

fn skip_if_cranelift_host_unsupported() -> bool {
    if jet_jit::cranelift_host_supported() {
        false
    } else if std::env::var("JET_REQUIRE_CRANELIFT_HOST").as_deref() == Ok("1") {
        panic!("Cranelift host required but unavailable");
    } else {
        true
    }
}

fn run(source: &str, name: &str) -> Output {
    let dir = common::unique_tmp(name);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.jet");
    fs::write(&file, source).unwrap();
    jet_jit::reset_jit_trace_for_test();
    let outcome = dev_iteration(file.to_str().unwrap(), false, false);
    let output = match outcome {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => Output {
            stdout,
            stderr,
            exit_code,
        },
        RunOutcome::Problems(diags) => panic!(
            "{name} failed: {:?}",
            diags.iter().map(|diag| &diag.code).collect::<Vec<_>>()
        ),
    };
    let _ = fs::remove_dir_all(dir);
    output
}

const RESIDENT: &str = r#"
use core.http.client as http_client
use core.http.server as http_server

#Codable
struct Reading {
    city: String
    degrees: Int
}

fn run() {
    mux :: http_server.mux()
    policy :: http_server.cors_policy(["https://app.example"]) ?? panic("cors")
    http_server.cors(mux, policy)
    http_server.static_files(mux, "/assets", "/tmp")
    response :: http_server.json(201, Reading.{city: "Reno", degrees: 32})
    decoded :: response.json<Reading>() ?? panic("response")
    request :: http_client.request("POST", "http://example.test/")
        .body("{{\"city\":\"Reno\",\"degrees\":31}}")
    request_decoded :: request.json<Reading>() ?? panic("request")
    print("{decoded.city}|{request_decoded.degrees}")
}
"#;

const FORCED_DEOPT: &str = r#"
use core.http.client as http_client
use core.http.server as http_server
use core.text as text

#Codable
struct Reading {
    city: String
    degrees: Int
}

fn any_origins() => HTTPCorsOrigins {
    return .Any
}

fn run() {
    print(text.casefold("Straße"))
    mux :: http_server.mux()
    policy :: http_server.cors_policy(["https://app.example"]) ?? panic("cors")
    http_server.cors(mux, policy)
    http_server.static_files(mux, "/assets", "/tmp")
    response :: http_server.json(201, Reading.{city: "Reno", degrees: 32})
    decoded :: response.json<Reading>() ?? panic("response")
    request :: http_client.request("POST", "http://example.test/")
        .body("{{\"city\":\"Reno\",\"degrees\":31}}")
    request_decoded :: request.json<Reading>() ?? panic("request")
    print("{decoded.city}|{request_decoded.degrees}")
    malformed :: http_client.request("POST", "http://example.test/").body("{{")
    if malformed.json<Reading>() == {
        .Ok(_) -> print("accepted")
        .Err(error) -> print(error)
        else -> print("unexpected")
    }
    if http_server.cors_policy(any_origins(), [], [], true) == {
        .Ok(_) -> print("accepted")
        .Err(error) -> print(error)
        else -> print("unexpected")
    }
}
"#;

#[test]
fn http_web_defaults_stay_resident_in_cranelift() {
    if skip_if_cranelift_host_unsupported() {
        return;
    }
    on_large_stack(|| {
        let output = run(RESIDENT, "http_i9_resident");
        assert_eq!(output.stdout, "Reno|31\n");
        assert_eq!(output.stderr, "");
        assert_eq!(output.exit_code, 0);
        assert!(jet_jit::jit_executed_for_test());
        assert!(!jet_jit::deopt_invoked_for_test());
        assert!(!jet_jit::fallback_invoked_for_test());
    });
}

#[test]
fn http_web_defaults_forced_deopt_uses_prelude_ambient() {
    on_large_stack(|| {
        let output = run(FORCED_DEOPT, "http_i9_forced_deopt");
        assert_eq!(output.stderr, "");
        assert_eq!(output.exit_code, 0);
        assert!(output.stdout.starts_with("strasse\nReno|31\nInvalidFraming\nPolicy { reason: "));
        assert!(output.stdout.contains("CORS credentials need named origins."));
        assert!(jet_jit::deopt_invoked_for_test());
        assert!(!jet_jit::fallback_invoked_for_test());
    });
}
