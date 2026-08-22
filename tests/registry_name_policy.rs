//! Card #1912: publish rejects registry-name typosquats before mutation.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "jet_registry_name_policy_{label}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn git(cwd: &Path, args: &[&str]) -> Output {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|error| panic!("git {args:?} failed to start: {error}"));
    assert!(
        output.status.success(),
        "git {args:?} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn seed_existing_name(registry: &Path) {
    Command::new("git")
        .args(["init", "--bare", registry.to_str().unwrap()])
        .status()
        .unwrap();
    Command::new("git")
        .args([
            "--git-dir",
            registry.to_str().unwrap(),
            "symbolic-ref",
            "HEAD",
            "refs/heads/main",
        ])
        .status()
        .unwrap();
    let work = registry.with_extension(format!("seed-{}", std::process::id()));
    let url = format!("file://{}", registry.to_str().unwrap());
    git(Path::new("."), &["clone", &url, work.to_str().unwrap()]);

    let entry = jet::Publish::IndexEntry {
        name: "librewolf".to_string(),
        version: "1.0.0".to_string(),
        content_hash: "sha256-existing".to_string(),
        fingerprint: "sha256-existing-fingerprint".to_string(),
        yanked: false,
        tier: jet::Publish::RegistryTier::Core,
        gate_status: jet::Publish::GateStatus::core_reviewed(),
        public_key: String::new(),
        signature: String::new(),
    };
    let index = work.join("index/librewolf/librewolf.jsonl");
    fs::create_dir_all(index.parent().unwrap()).unwrap();
    fs::write(&index, format!("{}\n", entry.to_jsonl())).unwrap();
    git(&work, &["config", "user.email", "test@jet.test"]);
    git(&work, &["config", "user.name", "Jet Test"]);
    git(&work, &["add", "."]);
    git(&work, &["commit", "-m", "seed package name"]);
    git(&work, &["push", "origin", "HEAD:main"]);
    fs::remove_dir_all(work).unwrap();
}

fn init_project(project: &Path) {
    fs::create_dir_all(project).unwrap();
    fs::write(
        project.join("package.jet"),
        "name: \"librewolf-fixed-bin\"\nversion: \"1.0.0\"\njet: \">=0.1.0\"\ndescription: \"\"\nlicense: \"MIT\"\nrepository: \"\"\n",
    )
    .unwrap();
    fs::write(
        project.join("run.jet"),
        "#Test(\"smoke\") { expect(1 == 1) }\nfn run() { print(\"hello\"); }\n",
    )
    .unwrap();
    git(project, &["init", "-b", "main"]);
    git(project, &["config", "user.email", "test@jet.test"]);
    git(project, &["config", "user.name", "Jet Test"]);
    git(project, &["add", "."]);
    git(project, &["commit", "-m", "initial package"]);
}

#[test]
fn hostile_lookalike_publish_is_rejected_with_teaching_error() {
    let root = scratch("publish");
    let registry = root.join("registry.git");
    let project = root.join("project");
    let cache = root.join("registry-cache");
    let store = root.join("store");
    let keys = root.join("keys");
    seed_existing_name(&registry);
    init_project(&project);

    let url = format!("file://{}", registry.to_str().unwrap());
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["registry", "publish", "--no-sign", "--quiet"])
        .current_dir(&project)
        .env("JET_REGISTRY_URL", &url)
        .env("JET_REGISTRY_CACHE_DIR", &cache)
        .env("JET_STORE_DIR", &store)
        .env("JET_KEYS_DIR", &keys)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!output.status.success(), "hostile name must not publish");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let snapshot = include_str!("cli/registry_name_policy.txt");
    assert_eq!(stderr, snapshot, "publish diagnostic changed");
    assert!(!stderr.contains("registry source artifact"));
    assert!(!stderr.contains("index entry"));

    let _ = fs::remove_dir_all(root);
}
