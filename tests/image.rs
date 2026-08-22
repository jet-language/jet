//! U14 `.Oci` container images (D-JPK-IMAGE1=A, card c9jetpackgates, Epoch 4).
//!
//! `image.<name> { kind: .Oci, from: packages.<name> }` builds a deterministic,
//! native OCI layout from an already-built package binary. The same record with
//! `from: env.<name>` projects a verified Hangar shell output — no Docker or
//! second OCI model (I6). Explicit registry pushes use the host curl adapter;
//! `.Iso` disk images (the original U14 shape) ride
//! the jetos installer tier (Phase D, owner-gated, untouched by this card).
//!
//! Covers:
//!   * field-check capture through the library `evaluate_env` (kind inference,
//!     `expose`/`env_vars`/`files`/`base`) — most of this lives in
//!     `crates/jet-driver/src/Jetpack/ModuleEval/mod.rs`'s own unit tests;
//!     this file adds the committed-fixture and cross-file-manifest angles;
//!   * a library-kind (or undeclared) `from:` package is rejected (E1267);
//!   * the `jetpack image <name>` engine verb: builds a real OCI layout,
//!     reproducibly, from a scratch project; a `.Iso` image gets a friendly
//!     "not yet" message; `--push` copies a validated local layout or uses the
//!     explicit OCI Distribution HTTP(S) path, while unqualified registry
//!     names remain the honest E1268 gate.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

mod common;
use common::{jetpack_bin, Scratch};
use jet_env_model::ModuleEval::evaluate_env;
use jetpack::Store;

fn jetpack() -> Command {
    Command::new(jetpack_bin())
}

/// A minimal project: `package.jet` declaring `<pkg_kind>` for `app`, `env.jet`
/// declaring `image.server { from: packages.app, … }`, and — when `built` is
/// true — a fake executable already staged at `build/app` (the `jet build`
/// output convention `jet image` reads from).
fn write_project(dir: &Path, pkg_kind: &str, built: bool) {
    fs::write(
        dir.join("package.jet"),
        format!("name: \"demo\"\nversion: \"0.1.0\"\npackages: {{ app: {pkg_kind} }}\n"),
    )
    .unwrap();
    fs::write(
        dir.join("env.jet"),
        "module image.server {\n    kind: .Oci\n    from: packages.app\n    expose: [8080]\n    env_vars: [\"RUST_LOG\": \"info\"]\n}\n",
    )
    .unwrap();
    if built {
        fs::create_dir_all(dir.join("build")).unwrap();
        fs::write(dir.join("build").join("app"), b"#!/bin/sh\necho hi\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let bin = dir.join("build").join("app");
            let mut perm = fs::metadata(&bin).unwrap().permissions();
            perm.set_mode(0o755);
            fs::set_permissions(&bin, perm).unwrap();
        }
    }
}

fn ingest_executable(root: &Path, name: &str, reference: &str, binary: &str) {
    ingest_executable_for_platform(
        root,
        name,
        reference,
        binary,
        &jetpack::Envelope::host_platform(),
    );
}

fn ingest_executable_for_platform(
    root: &Path,
    name: &str,
    reference: &str,
    binary: &str,
    platform: &str,
) {
    let source = root.join(format!("source-{name}"));
    fs::create_dir_all(source.join("bin")).unwrap();
    fs::write(source.join("bin").join(binary), b"#!/bin/sh\necho image\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = source.join("bin").join(binary);
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }
    let roots = Store::Roots {
        root: root.to_path_buf(),
        dev_mode: false,
    };
    Store::ingest_tree(
        &roots,
        &Store::IngestRequest {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            reference: reference.to_string(),
            cache_identity: Store::CacheIdentity {
                source_fingerprint: format!("sha256-source-{name}"),
                recipe_fingerprint: format!("sha256-recipe-{name}"),
                policy_fingerprint: "policy=image-test".to_string(),
                platform: platform.to_string(),
            },
            references: Vec::new(),
            outputs: std::collections::BTreeMap::from([("out".to_string(), source)]),
            signature: String::new(),
            provenance: "image-test".to_string(),
            platform_artifact_kind: String::new(),
        },
    )
    .unwrap();
}

fn image_blob_containing(root: &Path, needle: &[u8]) -> Vec<u8> {
    fs::read_dir(root.join("blobs/sha256"))
        .unwrap()
        .map(|entry| fs::read(entry.unwrap().path()).unwrap())
        .find(|bytes| bytes.windows(needle.len()).any(|window| window == needle))
        .unwrap_or_else(|| panic!("image has no blob containing {:?}", needle))
}

fn image_layer(root: &Path) -> Vec<u8> {
    fs::read_dir(root.join("blobs/sha256"))
        .unwrap()
        .map(|entry| fs::read(entry.unwrap().path()).unwrap())
        .find(|bytes| bytes.len() >= 263 && &bytes[257..263] == b"ustar\0")
        .expect("image has no ustar layer blob")
}

fn registry_test_server(
    listener: TcpListener,
    expected_requests: usize,
) -> Vec<(String, String, Vec<u8>)> {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut requests = Vec::new();
    while requests.len() < expected_requests && Instant::now() < deadline {
        let Ok((mut stream, _)) = listener.accept() else {
            thread::sleep(Duration::from_millis(5));
            continue;
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        let header_end = loop {
            let read = stream.read(&mut buffer).unwrap();
            if read == 0 {
                break None;
            }
            request.extend_from_slice(&buffer[..read]);
            if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                break Some(index + 4);
            }
            assert!(
                request.len() < 128 * 1024,
                "registry test request headers are unbounded"
            );
        };
        let Some(header_end) = header_end else {
            continue;
        };
        let headers = &request[..header_end];
        let header_text = String::from_utf8_lossy(headers);
        let mut lines = header_text.lines();
        let request_line = lines.next().unwrap_or_default();
        let mut request_parts = request_line.split_whitespace();
        let method = request_parts.next().unwrap_or_default().to_string();
        let path = request_parts.next().unwrap_or_default().to_string();
        let content_length = lines
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        while request.len() < header_end + content_length {
            let read = stream.read(&mut buffer).unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
        }
        let body = request[header_end..request.len().min(header_end + content_length)].to_vec();
        requests.push((method.clone(), path.clone(), body));

        let (status, extra_headers) = match method.as_str() {
            "HEAD" => (404, String::new()),
            "POST" => (202, "Location: /test-upload\r\n".to_string()),
            "PUT" if path.contains("/blobs/uploads/") || path.starts_with("/test-upload") => {
                (201, String::new())
            }
            "PUT" if path.contains("/manifests/") => (201, String::new()),
            _ => (404, String::new()),
        };
        let response = format!(
            "HTTP/1.1 {status} Test\r\n{extra_headers}Content-Length: 0\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(response.as_bytes()).unwrap();
    }
    requests
}

// ── field-check / cross-check (modeval) ─────────────────────────────────────

/// I5: the committed typed-image fixture (the `.Oci` shape) is the executable
/// spec — it parses, field-checks, and cross-checks clean against its own
/// `package.jet`, capturing every `.Oci`-only field.
#[test]
fn committed_oci_image_example_field_checks_clean() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jetpack-typed/image.jet");
    let src = fs::read_to_string(&path).unwrap();
    let dir = path.parent().unwrap();
    let plan = evaluate_env(&src, dir).unwrap();
    assert_eq!(plan.images.len(), 1);
    let image = &plan.images[0];
    assert_eq!(image.name, "server");
    assert_eq!(image.from, "server");
    assert_eq!(image.expose, vec![8080, 8443]);
    assert_eq!(
        image.env_vars,
        vec![("RUST_LOG".to_string(), "info".to_string())]
    );
    assert_eq!(image.files, vec!["config/app.toml".to_string()]);
}

#[test]
fn committed_environment_image_example_field_checks_clean() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/jetpack-typed/environment_image.jet");
    let src = fs::read_to_string(&path).unwrap();
    let plan = evaluate_env(&src, path.parent().unwrap()).unwrap();
    assert_eq!(plan.images.len(), 1);
    let image = &plan.images[0];
    assert!(image.from_environment);
    assert_eq!(image.from, "dev");
    assert_eq!(image.target.as_deref(), Some("linux.arm64"));
    assert_eq!(image.user, Some(10001));
    assert_eq!(image.entrypoint.as_deref(), Some("/bin/sh"));
    assert_eq!(image.health.as_deref(), Some("test -x /bin/sh"));
    assert_eq!(image.expose, vec![8080]);
    assert_eq!(image.files, vec!["config/app.toml".to_string()]);
}

/// A `from: packages.<name>` naming a `library`-kind package is rejected
/// (E1267 `oci-from-non-executable`) — the specific test D-JPK-IMAGE1 calls
/// out: a library has no binary to containerize.
#[test]
fn oci_from_library_source_is_rejected() {
    let scratch = Scratch::new("library-rejected");
    write_project(&scratch.path, "library", false);
    let src = fs::read_to_string(scratch.path.join("env.jet")).unwrap();
    let err = evaluate_env(&src, &scratch.path).unwrap_err();
    assert_eq!(err.code, "E1267");
    let rendered = jet::Diagnostics::render_all("env.jet", &src, std::slice::from_ref(&err));
    assert!(
        rendered.contains("non-executable package `app`"),
        "{rendered}"
    );
    assert!(rendered.contains("declared `library`"), "{rendered}");
}

/// `kind: .Docker` (not `.Oci`/`.Iso`) is E1266 — see the fuller mismatch/shape
/// coverage in `crates/jet-driver/src/Jetpack/ModuleEval/mod.rs`'s own unit
/// tests; this assertion exists so `tests/diagnostics_coverage.rs` (I4: every
/// code needs a snapshot/assertion under `tests/`) sees E1266 too.
#[test]
fn image_unknown_kind_is_e1266() {
    let src = "module image.server { kind: .Docker, from: packages.app }";
    let err = evaluate_env(src, &std::env::temp_dir()).unwrap_err();
    assert_eq!(err.code, "E1266");
}

/// `expose:` written as a non-list value is E1269 — same I4 coverage note as
/// `image_unknown_kind_is_e1266` above.
#[test]
fn oci_field_wrong_shape_is_e1269() {
    let scratch = Scratch::new("field-shape");
    write_project(&scratch.path, "executable", false);
    let src = "module image.server { from: packages.app, expose: \"8080\" }";
    let err = evaluate_env(src, &scratch.path).unwrap_err();
    assert_eq!(err.code, "E1269");
}

#[test]
fn oci_expose_invalid_port_is_e1269() {
    let scratch = Scratch::new("invalid-port");
    write_project(&scratch.path, "executable", false);
    let src = "module image.server { from: packages.app, expose: [0, 70000] }";
    let err = evaluate_env(src, &scratch.path).unwrap_err();
    assert_eq!(err.code, "E1269");
}

// ── `jetpack image` engine verb ─────────────────────────────────────────────

/// `jetpack image <name>` on a valid `.Oci` image with an already-built
/// binary produces a real, on-disk OCI layout (not a stub) and exits 0.
#[test]
fn image_builds_real_oci_layout() {
    let scratch = Scratch::new("build-ok");
    write_project(&scratch.path, "executable", true);
    let out = jetpack()
        .arg("image")
        .arg("server")
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "build should succeed: {stderr}");
    assert!(stderr.contains("built image `server`"), "{stderr}");

    let out_dir = scratch.path.join(".jet").join("images").join("server");
    assert!(out_dir.join("oci-layout").is_file());
    assert!(out_dir.join("index.json").is_file());
    let index = fs::read_to_string(out_dir.join("index.json")).unwrap();
    assert!(index.contains("application/vnd.oci.image.manifest.v1+json"));
}

/// Reproducibility at the CLI level (not just the unit-tested builder):
/// building the same project twice, into fresh output dirs, yields the same
/// manifest digest byte-for-byte.
#[test]
fn image_build_is_reproducible_end_to_end() {
    let scratch = Scratch::new("reproducible");
    write_project(&scratch.path, "executable", true);
    let digest_of = || {
        let out = jetpack()
            .arg("image")
            .arg("server")
            .current_dir(&scratch.path)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0));
        let index = fs::read_to_string(scratch.path.join(".jet/images/server/index.json")).unwrap();
        index
    };
    let a = digest_of();
    fs::remove_dir_all(scratch.path.join(".jet/images")).unwrap();
    let b = digest_of();
    assert_eq!(a, b, "same project must build byte-identical index.json");
}

#[test]
fn image_push_copies_a_real_layout_and_refuses_conflicts() {
    let project = Scratch::new("copy-layout-project");
    let destination_root = Scratch::new("copy-layout-destination");
    write_project(&project.path, "executable", true);
    let destination = destination_root.path.join("published");
    let destination_ref = format!("file://{}", destination.display());

    let out = jetpack()
        .args(["image", "server", "--push"])
        .arg(&destination_ref)
        .current_dir(&project.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "copy should succeed: {stderr}");
    let source = project.path.join(".jet/images/server");
    assert_eq!(
        fs::read(source.join("index.json")).unwrap(),
        fs::read(destination.join("index.json")).unwrap()
    );
    assert_eq!(
        fs::read(source.join("oci-layout")).unwrap(),
        fs::read(destination.join("oci-layout")).unwrap()
    );
    let mut source_blobs = fs::read_dir(source.join("blobs/sha256"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    let mut destination_blobs = fs::read_dir(destination.join("blobs/sha256"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    source_blobs.sort();
    destination_blobs.sort();
    assert_eq!(source_blobs, destination_blobs);

    fs::write(destination.join("index.json"), b"conflicting layout").unwrap();
    let out = jetpack()
        .args(["image", "server", "--push"])
        .arg(&destination_ref)
        .current_dir(&project.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "conflicts must fail: {stderr}");
    assert!(stderr.contains("byte-identical"), "{stderr}");
    assert_eq!(
        fs::read(destination.join("index.json")).unwrap(),
        b"conflicting layout"
    );
}

#[test]
fn image_push_uses_the_real_oci_distribution_path() {
    let project = Scratch::new("registry-project");
    write_project(&project.path, "executable", true);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || registry_test_server(listener, 7));
    let reference = format!("http://127.0.0.1:{port}/demo/server:1");

    let out = jetpack()
        .args(["image", "server", "--push"])
        .arg(&reference)
        .current_dir(&project.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    let requests = server.join().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "registry push should succeed: {stderr}"
    );
    assert_eq!(requests.len(), 7, "requests: {requests:?}");
    assert_eq!(
        requests
            .iter()
            .filter(|(method, _, _)| method == "HEAD")
            .count(),
        2
    );
    assert_eq!(
        requests
            .iter()
            .filter(|(method, _, _)| method == "POST")
            .count(),
        2
    );
    assert_eq!(
        requests
            .iter()
            .filter(|(method, path, _)| method == "PUT" && path.starts_with("/test-upload"))
            .count(),
        2
    );
    let manifest = requests
        .iter()
        .find(|(method, path, _)| method == "PUT" && path.contains("/manifests/1"))
        .expect("manifest PUT");
    assert!(manifest
        .2
        .windows(b"schemaVersion".len())
        .any(|window| window == b"schemaVersion"));
    assert!(stderr.contains("published"), "{stderr}");
}

#[test]
fn image_registry_missing_tool_fails_before_claiming_publish() {
    let project = Scratch::new("registry-missing-tool");
    let empty_path = Scratch::new("registry-empty-path");
    write_project(&project.path, "executable", true);
    let out = jetpack()
        .args([
            "image",
            "server",
            "--push",
            "http://127.0.0.1:9/demo/server:1",
        ])
        .current_dir(&project.path)
        .env("NO_COLOR", "1")
        .env("PATH", &empty_path.path)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr.contains("OCI registry adapter could not start"),
        "{stderr}"
    );
    assert!(stderr.contains("couldn't publish image"), "{stderr}");
}

/// Environment images consume the realized Hangar bin projection. A project
/// `build/` directory is intentionally absent, so the old scratch-file path
/// would fail this production-store check.
#[test]
fn environment_image_uses_realized_package_output() {
    let project = Scratch::new("environment-store");
    let root = Scratch::new("environment-store-root");
    fs::write(
        project.path.join("env.jet"),
        "module env.dev { packages: [\"bash@nixpkgs\"] }\nmodule image.server { from: env.dev }\n",
    )
    .unwrap();
    ingest_executable(&root.path, "bash", "bash@nixpkgs", "bash");

    let out = jetpack()
        .arg("image")
        .arg("server")
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "image should use Hangar output: {stderr}"
    );
    let report =
        fs::read_to_string(project.path.join(".jet/images/server/projection.json")).unwrap();
    assert!(
        report.contains("package:bash@nixpkgs"),
        "projection: {report}"
    );
    assert!(!project.path.join("build").exists());
    let image = project.path.join(".jet/images/server");
    let layer = image_layer(&image);
    assert!(layer
        .windows(b"bin/sh\0".len())
        .any(|window| window == b"bin/sh\0"));
    assert!(!image_blob_containing(&image, br#""User":"10001""#).is_empty());
    let plan = fs::read_to_string(image.join("plan.json")).unwrap();
    assert!(plan.contains("\"source\":\"env:dev\""), "plan: {plan}");
    assert!(
        plan.contains("\"entrypoint\":[\"/bin/sh\"]"),
        "plan: {plan}"
    );
    assert!(plan.contains("\"platform\":\"linux.x64\""), "plan: {plan}");
    assert!(plan.contains("\"user\":10001"), "plan: {plan}");
    assert!(plan.contains("\"expose\":[]"), "plan: {plan}");
    assert!(plan.contains("\"healthcheck\":false"), "plan: {plan}");
    assert!(plan.contains("\"services\":[]"), "plan: {plan}");
    assert!(plan.contains("\"content\":\"Hangar\""), "plan: {plan}");
    assert!(plan.contains("\"cache\":\"Hangar\""), "plan: {plan}");
    assert!(plan.contains("\"archive\":\"Hangar\""), "plan: {plan}");
    assert!(plan.contains("\"signing\":\"Hangar\""), "plan: {plan}");
    assert!(
        plan.contains("\"provenance\":\"Hangar+.jet/lock\""),
        "plan: {plan}"
    );
    assert!(plan.contains("\"inputs\":\".jet/lock\""), "plan: {plan}");
    assert!(plan.contains("\"platforms\":\".jet/lock\""), "plan: {plan}");
    assert!(plan.contains("\"publish\":\"Hangar\""), "plan: {plan}");
    assert!(
        plan.contains("\"remote\":\"D-JPK-REMOTE1\""),
        "plan: {plan}"
    );
    let dossier = fs::read_to_string(image.join("dossier.json")).unwrap();
    assert!(
        dossier.contains("runtime-mount-or-reference-only"),
        "dossier: {dossier}"
    );
}

#[test]
fn environment_image_projects_explicit_extra_file_reproducibly() {
    let project = Scratch::new("environment-extra-file");
    let root = Scratch::new("environment-extra-file-root");
    fs::create_dir_all(project.path.join("config")).unwrap();
    fs::write(
        project.path.join("env.jet"),
        "module env.dev { packages: [\"bash@nixpkgs\"] }\nmodule image.server { from: env.dev, files: [\"config/app.toml\"] }\n",
    )
    .unwrap();
    fs::write(project.path.join("config/app.toml"), b"port = 8080\n").unwrap();
    ingest_executable(&root.path, "bash", "bash@nixpkgs", "bash");

    let build = || {
        let out = jetpack()
            .args(["image", "server"])
            .current_dir(&project.path)
            .env("JETPACK_ROOT", &root.path)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        fs::read_to_string(project.path.join(".jet/images/server/index.json")).unwrap()
    };

    let first = build();
    let image = project.path.join(".jet/images/server");
    let report = fs::read_to_string(image.join("projection.json")).unwrap();
    assert!(
        report.contains("file:config/app.toml"),
        "projection: {report}"
    );
    let plan = fs::read_to_string(image.join("plan.json")).unwrap();
    assert!(plan.contains("config/app.toml"), "plan: {plan}");
    assert!(image_layer(&image)
        .windows(b"port = 8080\n".len())
        .any(|window| window == b"port = 8080\n"));

    fs::remove_dir_all(project.path.join(".jet/images")).unwrap();
    let second = build();
    assert_eq!(
        first, second,
        "extra-file projection must preserve image identity"
    );
}

#[test]
fn environment_image_rejects_public_managed_extra_file() {
    let project = Scratch::new("environment-managed-file");
    let root = Scratch::new("environment-managed-file-root");
    fs::create_dir_all(project.path.join("config")).unwrap();
    fs::write(
        project.path.join("env.jet"),
        r#"module env.dev {
    packages: ["bash@nixpkgs"]
    files: ["config/generated.txt": File{ content: "managed\n", mode: .Copy }]
}
module image.server { from: env.dev, files: ["config/generated.txt"] }
"#,
    )
    .unwrap();
    fs::write(project.path.join("config/generated.txt"), b"project\n").unwrap();
    ingest_executable(&root.path, "bash", "bash@nixpkgs", "bash");

    let out = jetpack()
        .args(["image", "server"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("E1336"), "stderr: {stderr}");
    assert!(
        stderr.contains("managed environment files"),
        "stderr: {stderr}"
    );
    let report =
        fs::read_to_string(project.path.join(".jet/images/server/projection.json")).unwrap();
    assert!(
        report.contains("file:config/generated.txt"),
        "projection: {report}"
    );
    assert!(report.contains("\"rejected\""), "projection: {report}");
    assert!(!project.path.join(".jet/images/server/blobs").exists());
}

#[test]
fn environment_image_rejects_extra_file_path_conflict() {
    let project = Scratch::new("environment-file-conflict");
    let root = Scratch::new("environment-file-conflict-root");
    fs::create_dir_all(project.path.join("bin")).unwrap();
    fs::write(
        project.path.join("env.jet"),
        "module env.dev { packages: [\"bash@nixpkgs\"] }\nmodule image.server { from: env.dev, files: [\"bin/sh\"] }\n",
    )
    .unwrap();
    fs::write(project.path.join("bin/sh"), b"project shell\n").unwrap();
    ingest_executable(&root.path, "bash", "bash@nixpkgs", "bash");

    let out = jetpack()
        .args(["image", "server"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("E1336"), "stderr: {stderr}");
    assert!(stderr.contains("conflicts"), "stderr: {stderr}");
    let report =
        fs::read_to_string(project.path.join(".jet/images/server/projection.json")).unwrap();
    assert!(report.contains("file:bin/sh"), "projection: {report}");
    assert!(report.contains("\"rejected\""), "projection: {report}");
    assert!(!project.path.join(".jet/images/server/blobs").exists());
}

#[test]
fn environment_image_projects_named_environment_not_default() {
    let project = Scratch::new("environment-selection");
    let root = Scratch::new("environment-selection-root");
    fs::write(
        project.path.join("env.jet"),
        "module env.dev { packages: [\"bash@nixpkgs\"] }\nmodule env.full { packages: [\"sh@nixpkgs\"] }\nmodule image.server { from: env.full, target: linux.arm64 }\n",
    )
    .unwrap();
    ingest_executable(&root.path, "bash", "bash@nixpkgs", "bash");
    ingest_executable_for_platform(&root.path, "sh", "sh@nixpkgs", "sh", "aarch64-linux");

    let out = jetpack()
        .args(["image", "server"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report =
        fs::read_to_string(project.path.join(".jet/images/server/projection.json")).unwrap();
    assert!(
        report.contains("package:sh@nixpkgs"),
        "projection: {report}"
    );
    assert!(
        !report.contains("package:bash@nixpkgs"),
        "projection: {report}"
    );
    assert!(
        report.contains("platform:linux.arm64"),
        "projection: {report}"
    );
    assert!(!image_blob_containing(
        &project.path.join(".jet/images/server"),
        br#""architecture":"arm64""#,
    )
    .is_empty());
}

#[test]
fn environment_image_requires_a_realized_shell_package() {
    let project = Scratch::new("environment-no-shell");
    let root = Scratch::new("environment-no-shell-root");
    fs::write(
        project.path.join("env.jet"),
        "module env.dev { packages: [\"tool@nixpkgs\"] }\nmodule image.server { from: env.dev }\n",
    )
    .unwrap();
    ingest_executable(&root.path, "tool", "tool@nixpkgs", "tool");

    let out = jetpack()
        .args(["image", "server"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr.contains("E1336"), "stderr: {stderr}");
    assert!(stderr.contains("no shell"), "stderr: {stderr}");
}

#[test]
fn environment_image_rejects_hangar_platform_mismatch() {
    let project = Scratch::new("environment-platform-mismatch");
    let root = Scratch::new("environment-platform-mismatch-root");
    fs::write(
        project.path.join("env.jet"),
        "module env.dev { packages: [\"bash@nixpkgs\"] }\nmodule image.server { from: env.dev, target: linux.arm64 }\n",
    )
    .unwrap();
    ingest_executable_for_platform(&root.path, "bash", "bash@nixpkgs", "bash", "x86_64-linux");

    let out = jetpack()
        .args(["image", "server"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("E1336"), "stderr: {stderr}");
    assert!(stderr.contains("platform"), "stderr: {stderr}");
    let report =
        fs::read_to_string(project.path.join(".jet/images/server/projection.json")).unwrap();
    assert!(report.contains("\"rejected\""), "projection: {report}");
}

#[test]
fn environment_image_rejects_an_unsupported_integration_host() {
    let project = Scratch::new("environment-unsupported-host");
    fs::write(
        project.path.join("env.jet"),
        "module env.dev { imports: [env.platform.apple()] }\nmodule image.server { from: env.dev }\n",
    )
    .unwrap();
    let out = jetpack()
        .args(["image", "server"])
        .current_dir(&project.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(2),
        "unsupported hosts must fail: {stderr}"
    );
    assert!(stderr.contains("E1333"), "{stderr}");
    assert!(stderr.contains("not supported on target"), "{stderr}");
    let report =
        fs::read_to_string(project.path.join(".jet/images/server/projection.json")).unwrap();
    assert!(
        report.contains("integration:apple:host"),
        "projection: {report}"
    );
}

#[test]
fn environment_image_omits_cloud_and_vault_facts_without_secret_names() {
    let project = Scratch::new("environment-secret-integrations");
    let root = Scratch::new("environment-secret-integrations-root");
    fs::write(
        project.path.join("env.jet"),
        r#"module env.dev {
    packages: ["bash@nixpkgs"]
    imports: [
        env.cloud.credentials([aws_production]),
        env.security.vault([database_password])
    ]
}
module image.server { from: env.dev }
"#,
    )
    .unwrap();
    ingest_executable(&root.path, "bash", "bash@nixpkgs", "bash");

    let out = jetpack()
        .args(["image", "server"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "image should project the environment successfully: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report =
        fs::read_to_string(project.path.join(".jet/images/server/projection.json")).unwrap();
    assert!(report.contains("integration:cloud-credentials:activation"));
    assert!(report.contains("integration:cloud-credentials:task:credential-store-check"));
    assert!(report.contains("integration:cloud-credentials:provider:credential-store"));
    assert!(report.contains("integration:cloud-credentials:grant:credential.read"));
    assert!(report.contains("integration:vault:activation"));
    assert!(report.contains("integration:vault:task:vault-check"));
    assert!(report.contains("integration:vault:provider:vault"));
    assert!(report.contains("integration:vault:grant:vault.read"));
    assert!(report.contains("integration:cloud-credentials:secret-refs=1"));
    assert!(report.contains("integration:vault:secret-refs=1"));
    assert!(
        !report.contains("aws_production"),
        "secret name leaked: {report}"
    );
    assert!(
        !report.contains("database_password"),
        "secret name leaked: {report}"
    );
    let image = project.path.join(".jet/images/server");
    for blob in fs::read_dir(image.join("blobs/sha256")).unwrap() {
        let bytes = fs::read(blob.unwrap().path()).unwrap();
        assert!(!bytes
            .windows("aws_production".len())
            .any(|window| window == b"aws_production"));
        assert!(!bytes
            .windows("database_password".len())
            .any(|window| window == b"database_password"));
    }
}

#[test]
fn environment_image_rejects_secret_extra_file() {
    let project = Scratch::new("environment-secret-file");
    let root = Scratch::new("environment-secret-file-root");
    fs::write(
        project.path.join("env.jet"),
        r#"module env.dev {
    packages: ["bash@nixpkgs"]
    dotenv: [Dotenv{ file: ".env", allow: ["TOKEN"], secrets: ["TOKEN"] }]
}
module image.server { from: env.dev, files: [".env"] }
"#,
    )
    .unwrap();
    fs::write(project.path.join(".env"), b"TOKEN=super-secret-value\n").unwrap();
    ingest_executable(&root.path, "bash", "bash@nixpkgs", "bash");

    let out = jetpack()
        .args(["image", "server"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("E1336"), "stderr: {stderr}");
    assert!(
        !stderr.contains("super-secret-value"),
        "secret leaked: {stderr}"
    );
    let report =
        fs::read_to_string(project.path.join(".jet/images/server/projection.json")).unwrap();
    assert!(report.contains("file:.env"), "projection: {report}");
    assert!(
        !report.contains("super-secret-value"),
        "secret leaked: {report}"
    );
    if let Ok(blobs) = fs::read_dir(project.path.join(".jet/images/server/blobs/sha256")) {
        for blob in blobs {
            let bytes = fs::read(blob.unwrap().path()).unwrap();
            assert!(!bytes
                .windows(b"super-secret-value".len())
                .any(|window| { window == b"super-secret-value" }));
        }
    }
}

#[test]
fn environment_image_rejects_project_escape_layer_path() {
    let project = Scratch::new("environment-layer-path");
    let root = Scratch::new("environment-layer-path-root");
    fs::write(
        project.path.join("env.jet"),
        "module env.dev { packages: [\"bash@nixpkgs\"] }\nmodule image.server { from: env.dev, files: [\"../outside\"] }\n",
    )
    .unwrap();
    ingest_executable(&root.path, "bash", "bash@nixpkgs", "bash");

    let out = jetpack()
        .args(["image", "server"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(2),
        "unsafe layer path must fail: {stderr}"
    );
    assert!(
        stderr.contains("safe project-relative paths"),
        "stderr: {stderr}"
    );
    let report =
        fs::read_to_string(project.path.join(".jet/images/server/projection.json")).unwrap();
    assert!(report.contains("file:../outside"), "projection: {report}");
    assert!(report.contains("\"rejected\""), "projection: {report}");
}

#[test]
fn environment_image_rejects_unredactable_cloud_secret() {
    let project = Scratch::new("environment-cloud-secret-loss");
    fs::write(
        project.path.join("env.jet"),
        r#"module env.dev {
    imports: [env.cloud.credentials("super_secret_value")]
}
module image.server { from: env.dev }
"#,
    )
    .unwrap();

    let out = jetpack()
        .args(["image", "server"])
        .current_dir(&project.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E1335"), "stderr: {stderr}");
    assert!(
        !stderr.contains("super_secret_value"),
        "secret input leaked: {stderr}"
    );
}

#[test]
fn environment_image_requires_explicit_service_selection() {
    let project = Scratch::new("environment-service");
    fs::write(
        project.path.join("env.jet"),
        "module env.dev { services: { api: { run: [\"true\"] } } }\nmodule image.server { from: env.dev }\n",
    )
    .unwrap();
    let out = jetpack()
        .args(["image", "server"])
        .current_dir(&project.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(2),
        "service projection must fail: {stderr}"
    );
    let diagnostic = stderr
        .split("\n\n")
        .find(|block| block.contains("Error [E1336]"))
        .map(|block| {
            block
                .lines()
                .filter(|line| !line.is_empty())
                .map(str::trim)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .expect("E1336 diagnostic block");
    assert_eq!(
        diagnostic,
        include_str!("cli/image_service_projection_e1336.txt").trim()
    );
    let report = fs::read_to_string(project.path.join(".jet/images/server/projection.json"))
        .expect("rejected image projection report");
    assert!(
        report.contains("\"rejected\":[\"services\"]"),
        "projection: {report}"
    );
}

#[test]
fn environment_image_projects_supervised_services() {
    let project = Scratch::new("environment-service-image");
    fs::write(
        project.path.join("env.jet"),
        "module env.dev {\n    packages: [\"bash@nixpkgs\", \"db@nixpkgs\", \"sleep@nixpkgs\", \"worker@nixpkgs\"]\n    services: { db: { enable: true, run: [\"db\"] }, api: { enable: true, ports: [8080], after: [\"db\"], run: [\"worker\", \"--port\", \"8080\"] } }\n}\nmodule image.server { from: env.dev, services: [\"api\"] }\n",
    )
    .unwrap();
    ingest_executable(&project.path, "bash", "bash@nixpkgs", "bash");
    ingest_executable(&project.path, "db", "db@nixpkgs", "db");
    ingest_executable(&project.path, "sleep", "sleep@nixpkgs", "sleep");
    ingest_executable(&project.path, "worker", "worker@nixpkgs", "worker");

    let out = jetpack()
        .args(["image", "server"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &project.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "supervised image must build: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let layer = image_layer(&project.path.join(".jet/images/server"));
    assert!(
        layer
            .windows(b"jet/supervise".len())
            .any(|window| window == b"jet/supervise"),
        "supervisor path is not in the OCI layer"
    );
    let worker_cmd = b"start '/usr/local/bin/worker' '--port' '8080'";
    assert!(
        layer
            .windows(worker_cmd.len())
            .any(|window| window == worker_cmd),
        "service command is not in the generated supervisor"
    );
    let db_cmd = b"start '/usr/local/bin/db'";
    let db_start = layer
        .windows(db_cmd.len())
        .position(|window| window == db_cmd)
        .expect("dependency command");
    let worker_prefix = b"start '/usr/local/bin/worker'";
    let worker_start = layer
        .windows(worker_prefix.len())
        .position(|window| window == worker_prefix)
        .expect("dependent command");
    assert!(db_start < worker_start, "dependency must start first");
    let report =
        fs::read_to_string(project.path.join(".jet/images/server/projection.json")).unwrap();
    assert!(
        report.contains("services:jet/supervise"),
        "projection: {report}"
    );
    assert!(report.contains("service:api"), "projection: {report}");
    let plan = fs::read_to_string(project.path.join(".jet/images/server/plan.json")).unwrap();
    assert!(
        plan.contains("\"entrypoint\":[\"/jet/supervise\"]"),
        "plan: {plan}"
    );
    assert!(
        plan.contains("\"services\":[\"api\",\"db\"]"),
        "plan: {plan}"
    );
}

#[test]
fn environment_image_rejects_a_service_without_projected_executable() {
    let project = Scratch::new("environment-service-missing-tool");
    let root = Scratch::new("environment-service-missing-tool-root");
    fs::write(
        project.path.join("env.jet"),
        "module env.dev {\n    packages: [\"bash@nixpkgs\"]\n    services: { api: { enable: true, run: [\"worker\"] } }\n}\nmodule image.server { from: env.dev, services: [\"api\"] }\n",
    )
    .unwrap();
    ingest_executable(&root.path, "bash", "bash@nixpkgs", "bash");

    let out = jetpack()
        .args(["image", "server"])
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "missing service tool: {stderr}");
    assert!(stderr.contains("E1336"), "stderr: {stderr}");
    assert!(
        stderr.contains("not a projected package"),
        "stderr: {stderr}"
    );
    let report =
        fs::read_to_string(project.path.join(".jet/images/server/projection.json")).unwrap();
    assert!(
        report.contains("\"rejected\":[\"services\"]"),
        "projection: {report}"
    );
}

/// `jetpack image <name>` when the package hasn't been built yet (no
/// `build/<name>`) reports an honest "not built yet" message — it never
/// fabricates a binary or an empty image.
#[test]
fn image_of_unbuilt_package_is_honest() {
    let scratch = Scratch::new("unbuilt");
    write_project(&scratch.path, "executable", false);
    let out = jetpack()
        .arg("image")
        .arg("server")
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr.contains("isn't built yet"), "{stderr}");
    assert!(!scratch.path.join(".jet/images").exists());
}

/// An unqualified registry-looking `--push <ref>` remains an E1268 gate. Jet
/// requires an explicit HTTP(S) transport or a local `file://` OCI layout, even
/// when the image would otherwise build cleanly.
#[test]
fn image_push_is_gated_e1268() {
    let scratch = Scratch::new("push-gated");
    write_project(&scratch.path, "executable", true);
    let out = jetpack()
        .arg("image")
        .arg("server")
        .arg("--push")
        .arg("ghcr.io/acme/server:1.0")
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr.contains("E1268"), "expected E1268: {stderr}");
    assert!(stderr.contains("TLS"), "{stderr}");
    // The rejected remote name never silently builds or pushes anything.
    assert!(!scratch.path.join(".jet/images").exists());
}

/// Naming an image that doesn't exist lists the declared ones (mirrors
/// `jet push`'s friendly-unknown-name behavior, tests/fleet.rs).
#[test]
fn image_unknown_name_is_friendly() {
    let scratch = Scratch::new("unknown-name");
    write_project(&scratch.path, "executable", true);
    let out = jetpack()
        .arg("image")
        .arg("ghost")
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr.contains("no image `ghost`"), "{stderr}");
    assert!(stderr.contains("server"), "lists declared images: {stderr}");
}

/// A `.Iso` disk image named through `jetpack image` gets a friendly message
/// pointing at the (owner-gated, untouched) jetos installer tier — `jet
/// image` never silently no-ops or mis-treats it as a container.
#[test]
fn image_of_iso_kind_is_friendly_not_built() {
    let scratch = Scratch::new("iso-not-built");
    fs::write(
        scratch.path.join("env.jet"),
        "module system.web { target: linux.x64 }\nmodule image.installer { from: system.web }\n",
    )
    .unwrap();
    let out = jetpack()
        .arg("image")
        .arg("installer")
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr.contains("disk image, not a container"), "{stderr}");
    assert!(stderr.contains("Phase D"), "{stderr}");
}
