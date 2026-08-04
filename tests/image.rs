//! U14 `.Oci` container images (D-JPK-IMAGE1=A, card c9jetpackgates, Epoch 4).
//!
//! `image.<name> { kind: .Oci, from: packages.<name> }` builds a deterministic,
//! native OCI layout from an already-built package binary — no Docker, no
//! external OCI tooling (I6). `.Iso` disk images (the original U14 shape) ride
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
//!     "not yet" message; `--push` is honestly gated (E1268) rather than
//!     attempting a real push.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;
use common::jetpack_bin;
use jet_env_model::ModuleEval::evaluate_env;
use jetpack::Store;

fn jetpack() -> Command {
    Command::new(jetpack_bin())
}

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
            "image-it-{tag}-{nanos}-{:?}",
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

/// A minimal project: `pkg.jet` declaring `<pkg_kind>` for `app`, `env.jet`
/// declaring `image.server { from: packages.app, … }`, and — when `built` is
/// true — a fake executable already staged at `build/app` (the `jet build`
/// output convention `jet image` reads from).
fn write_project(dir: &Path, pkg_kind: &str, built: bool) {
    fs::write(
        dir.join("pkg.jet"),
        format!(
            "payload: {{ name: \"demo\", version: \"0.1.0\" }}\npackages: {{ app: {pkg_kind} }}\n"
        ),
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
                platform: jetpack::Envelope::host_platform(),
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

// ── field-check / cross-check (modeval) ─────────────────────────────────────

/// I5: the committed typed-image fixture (the `.Oci` shape) is the executable
/// spec — it parses, field-checks, and cross-checks clean against its own
/// `pkg.jet`, capturing every `.Oci`-only field.
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
    ingest_executable(&root.path, "bash", "bash@nixpkgs@default", "bash");

    let out = jetpack()
        .arg("image")
        .arg("server")
        .current_dir(&project.path)
        .env("JETPACK_ROOT", &root.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "image should use Hangar output: {stderr}");
    let report = fs::read_to_string(project.path.join(".jet/images/server/projection.json"))
        .unwrap();
    assert!(report.contains("package:bash@nixpkgs"), "projection: {report}");
    assert!(!project.path.join("build").exists());
}

#[test]
fn environment_image_service_projection_is_e1336() {
    let project = Scratch::new("environment-service");
    fs::write(
        project.path.join("env.jet"),
        "module env.dev { services: { api: { run: [\"true\"] } } }\nmodule image.server { from: env.dev, services: [\"api\"] }\n",
    )
    .unwrap();
    let out = jetpack()
        .args(["image", "server"])
        .current_dir(&project.path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "service projection must fail: {stderr}");
    assert!(stderr.contains("E1336"), "stderr: {stderr}");
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

/// `jetpack image <name> --push <ref>` never attempts a real push — it
/// reports the honest E1268 TLS gate and exits non-zero, even when the image
/// would otherwise build cleanly.
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
    // Never silently built or pushed anything.
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
