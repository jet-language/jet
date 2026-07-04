//! U17 — consuming a realized `library` package with `use` (D-LIB-USE A).
//!
//! Once a `library` package (U10) is realized — its source staged in the shared
//! hangar by the `core` provider — it is brought into code with the ordinary
//! `use <pkg>` module form (S16). One import concept covers files, modules, and
//! library packages. An `executable` package goes on PATH, not `use`.
//!
//! These tests realize a library entirely offline (the `core` provider with a
//! `path:` source — no Nix, no network), then drive the compiled `jet` binary
//! the way a user would, with `JETPACK_ROOT` pointed at a throwaway hangar so
//! the compiler's loader finds the staged source.

use jet::Jetpack::Provider::{self, Ctx};
use jet::Jetpack::RefSpec::{classify_in, ProviderKind, SourceTable};
use jet::Jetpack::Store::{self, Roots};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

/// A throwaway directory under the system temp dir, removed on drop.
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
            "jet-libuse-{tag}-{nanos}-{:?}",
            std::thread::current().id()
        ));
        fs::create_dir_all(&path).unwrap();
        Scratch { path }
    }
    fn join(&self, p: &str) -> PathBuf {
        self.path.join(p)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}

/// Realize a `core` package from a local `path:` source repo into `hangar`,
/// recording it in the store exactly as `jetpack build` would. Offline; no Nix.
fn realize_into_hangar(roots: &Roots, repo: &Path, pkg: &str) -> Store::StoreEntry {
    let store_dir = roots.hangar_dir();
    fs::create_dir_all(&store_dir).unwrap();
    let upstream = format!("path:{}", repo.to_string_lossy());
    let table = SourceTable::from_decls([("mine".to_string(), upstream, ProviderKind::Core)]);
    let spec = classify_in(&format!("mine:{pkg}"), &table).unwrap();
    let ctx = Ctx {
        fixtures: None,
        store_dir: &store_dir,
        offline: true,
    };
    let r = Provider::realize(&spec, &table, &ctx).expect("library realizes offline");
    Store::record(
        roots,
        &r.name,
        &r.version,
        &r.reference,
        &r.out,
        &r.bin,
        &r.rlib,
        &r.envelope,
    )
    .expect("records into hangar")
}

/// `use jsonutil;` resolves a realized library and `jsonutil.parse(...)` works.
#[test]
fn realized_library_is_consumed_with_use() {
    let s = Scratch::new("ok");
    let hangar_root = s.join("hangar-root");
    fs::create_dir_all(&hangar_root).unwrap();
    let roots = Roots {
        root: hangar_root.clone(),
        dev_mode: true,
    };

    // Producer repo: a `library` package `jsonutil`. The package's identity is
    // its `module jsonutil` declaration (U10 discovery); its consumable code is
    // top-level `pub fn` in the module file (S16 module form).
    let producer = s.join("jsonutil-src");
    write(
        &producer.join("pkg.jet"),
        "payload: { name: \"jsonutil\", version: \"0.1.0\" }\npackages: { jsonutil: library }\n",
    );
    write(
        &producer.join("jsonutil.jet"),
        "module jsonutil { }\npub fn parse(raw: String) -> Int {\n    return 42;\n}\n",
    );

    let entry = realize_into_hangar(&roots, &producer, "jsonutil");
    assert!(
        entry.bin.is_empty(),
        "a library stages no PATH bin: {entry:?}"
    );

    // Consumer project: declares the dependency, then `use`s the library. The
    // dep is a remote (git) ref — its source isn't on disk as a path dep, so the
    // resolver must find the realized package through the shared hangar (U17),
    // exercising the new extra search root rather than the M12.1 path-dep path.
    let consumer = s.join("app");
    write(
        &consumer.join("pkg.jet"),
        "payload: { name: \"app\", version: \"0.1.0\" }\ndeps: { jsonutil: github@acme/jsonutil/abc123 }\n",
    );
    write(
        &consumer.join("main.jet"),
        "use jsonutil;\nfn run() {\n    print(jsonutil.parse(\"x\"));\n}\n",
    );
    fs::create_dir_all(consumer.join("build")).unwrap();

    let out = Command::new(jet_bin())
        .args(["run", "main.jet"])
        .current_dir(&consumer)
        .env("JETPACK_ROOT", &hangar_root)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "jet run failed\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(stdout.contains("42"), "stdout: {stdout}\nstderr: {stderr}");
}

/// `use <exe>;` on a realized executable package is a teaching error (E0982).
#[test]
fn executable_is_not_importable() {
    let s = Scratch::new("exe");
    let hangar_root = s.join("hangar-root");
    fs::create_dir_all(&hangar_root).unwrap();
    let roots = Roots {
        root: hangar_root.clone(),
        dev_mode: true,
    };

    // Producer repo: an `executable` package `deploy` with a prebuilt bin/.
    let producer = s.join("deploy-src");
    write(
        &producer.join("pkg.jet"),
        "payload: { name: \"deploy\", version: \"0.1.0\" }\npackages: { deploy: executable }\n",
    );
    write(&producer.join("deploy.jet"), "module deploy { }\n");
    write(&producer.join("bin/deploy"), "#!/bin/sh\necho deploying\n");

    let entry = realize_into_hangar(&roots, &producer, "deploy");
    assert!(
        !entry.bin.is_empty(),
        "an executable stages a PATH bin: {entry:?}"
    );

    let consumer = s.join("app");
    write(
        &consumer.join("pkg.jet"),
        "payload: { name: \"app\", version: \"0.1.0\" }\ndeps: { deploy: github@acme/deploy/abc123 }\n",
    );
    write(
        &consumer.join("main.jet"),
        "use deploy;\nfn run() {\n    print(\"hi\");\n}\n",
    );
    fs::create_dir_all(consumer.join("build")).unwrap();

    let out = Command::new(jet_bin())
        .args(["run", "main.jet"])
        .current_dir(&consumer)
        .env("JETPACK_ROOT", &hangar_root)
        .output()
        .unwrap();
    assert!(!out.status.success(), "use of an executable must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    // Pin the exact product copy (I4: the message IS the UX).
    assert!(stderr.contains("E0982"), "stderr: {stderr}");
    assert!(
        stderr.contains("`deploy` is an executable package, so it can't be used with `use`"),
        "what line: {stderr}"
    );
    assert!(
        stderr.contains("an `executable` package installs a binary on your PATH"),
        "why line: {stderr}"
    );
    assert!(
        stderr.contains("change `deploy` to `library` in `pkg.jet`"),
        "fix line: {stderr}"
    );
}

/// `use <lib>;` on a declared-but-unrealized library points at `jetpack build`
/// (E0983).
#[test]
fn unrealized_library_points_at_build() {
    let s = Scratch::new("unrealized");
    let hangar_root = s.join("hangar-root");
    fs::create_dir_all(hangar_root.join("hangar")).unwrap();

    // The consumer declares a `jsonutil` dependency, but it was never realized
    // into the hangar — so the hangar has no `jsonutil` entry, and (being a git
    // dep) its source isn't on disk either.
    let consumer = s.join("app");
    write(
        &consumer.join("pkg.jet"),
        "payload: { name: \"app\", version: \"0.1.0\" }\ndeps: { jsonutil: github@acme/jsonutil/abc123 }\n",
    );
    write(
        &consumer.join("main.jet"),
        "use jsonutil;\nfn run() {\n    print(jsonutil.parse(\"x\"));\n}\n",
    );
    fs::create_dir_all(consumer.join("build")).unwrap();

    let out = Command::new(jet_bin())
        .args(["run", "main.jet"])
        .current_dir(&consumer)
        .env("JETPACK_ROOT", &hangar_root)
        .output()
        .unwrap();
    assert!(!out.status.success(), "an unrealized library must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    // Pin the exact product copy (I4: the message IS the UX).
    assert!(stderr.contains("E0983"), "stderr: {stderr}");
    assert!(
        stderr.contains("the library package `jsonutil` hasn't been built yet"),
        "what line: {stderr}"
    );
    assert!(
        stderr.contains("its source staged in the shared store (hangar)"),
        "why line: {stderr}"
    );
    assert!(
        stderr.contains("run `jetpack build` to realize `jsonutil`"),
        "fix line: {stderr}"
    );
}
