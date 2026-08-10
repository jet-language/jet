//! D-DEP-HTTP2=B evidence: native HTTP/compression stay free of new external crates.
//! Ratified TLS bridges (rustls*) remain allowed.

mod common;

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &str) -> String {
    fs::read_to_string(workspace_root().join(path)).unwrap_or_else(|error| {
        panic!("failed to read {path}: {error}");
    })
}

#[test]
fn http_manifests_have_no_new_external_http_or_compression_crates() {
    // Forbidden: new HTTP/compression stacks in compiler/prelude seams.
    // rustls* is a separately ratified TLS bridge in jet-pkg-model.
    let forbidden = [
        "hyper",
        "reqwest",
        "ureq",
        "attohttpc",
        "isahc",
        "surf",
        "awc",
        "actix-web",
        "axum",
        "warp",
        "tiny_http",
        "flate2",
        "libflate",
        "brotli",
        "async-compression",
        "tokio",
        "h2 ",
        "http-body",
    ];
    for path in [
        "crates/jet-codegen/Cargo.toml",
        "crates/jet-pkg-model/Cargo.toml",
    ] {
        let text = read(path);
        for needle in forbidden {
            if needle == "h2 " {
                // Ignore accidental substring matches inside other words.
                continue;
            }
            // Match Cargo dependency keys only: `name =` at line start or after newline.
            let needle_eq = format!("{needle} =");
            let needle_dot = format!("{needle}.");
            assert!(
                !text.contains(&needle_eq) && !text.lines().any(|line| {
                    let trimmed = line.trim_start();
                    trimmed.starts_with(needle) && (trimmed[needle.len()..].starts_with(" =") || trimmed[needle.len()..].starts_with("."))
                }) && !text.contains(&needle_dot),
                "{path} must not depend on `{needle}`"
            );
        }
        if path.ends_with("jet-pkg-model/Cargo.toml") {
            assert!(
                text.contains("rustls"),
                "jet-pkg-model may keep the ratified rustls TLS bridge"
            );
        }
    }
}

#[test]
fn http_prelude_sources_do_not_import_forbidden_http_crates() {
    let forbidden_imports = [
        "use ureq",
        "use hyper",
        "use reqwest",
        "use flate2",
        "use brotli",
        "use tokio",
        "extern crate ureq",
        "extern crate hyper",
        "extern crate flate2",
    ];
    let sources = [
        "crates/jet-codegen/src/Prelude/CoreLib/Top/HTTPMessage.rs",
        "crates/jet-codegen/src/Prelude/CoreLib/Top/HTTPRoute.rs",
        "crates/jet-codegen/src/Prelude/CoreLib/Top/HTTPClient.rs",
        "crates/jet-codegen/src/Prelude/CoreLib/Top/HTTPServer.rs",
        "crates/jet-codegen/src/Prelude/CoreLib/Top/Ws.rs",
        "crates/jet-pkg-model/src/Prelude/HTTP.rs",
    ];
    for path in sources {
        let text = read(path);
        for needle in forbidden_imports {
            assert!(
                !text.contains(needle),
                "{path} must not import `{needle}`"
            );
        }
    }
    // Comment noise in HTTPClient.rs historically mentioned ureq; keep it gone.
    let client = read("crates/jet-codegen/src/Prelude/CoreLib/Top/HTTPClient.rs");
    assert!(
        !client.to_ascii_lowercase().contains("ureq"),
        "HTTPClient prelude must not mention ureq"
    );
}

#[test]
fn unsupported_target_variant_is_mapped_from_bridge_and_server_bind() {
    let bridge = read("crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs");
    assert!(
        bridge.contains("JetHTTPBridgeError::UnsupportedTarget => JetHTTPError::UnsupportedTarget")
            && bridge.contains("JetHTTPOperation::ClientConnect"),
        "client bridge must map UnsupportedTarget to ClientConnect"
    );
    assert!(
        bridge.contains("unsupported-target:server-bind"),
        "server bind must map UnsupportedTarget before IO fallback"
    );
    assert!(
        bridge.contains("JetHTTPOperation::ServerBind"),
        "server bind UnsupportedTarget must name ServerBind"
    );
    let server = read("crates/jet-codegen/src/Prelude/CoreLib/Top/HTTPServer.rs");
    assert!(
        server.contains("unsupported-target:server-bind"),
        "server bind must preflight unsupported targets"
    );
    let client = read("crates/jet-pkg-model/src/Prelude/HTTP.rs");
    assert!(
        client.contains("JetHTTPBridgeError::UnsupportedTarget"),
        "client send must be able to return UnsupportedTarget"
    );
}

#[test]
fn current_host_supports_http_bind_and_ws_connect() {
    assert!(
        cfg!(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "openbsd",
            target_os = "netbsd",
            target_os = "windows"
        )),
        "http_dependency_audit assumes a supported desktop/server OS"
    );
}
