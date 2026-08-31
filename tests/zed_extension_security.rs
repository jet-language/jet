//! Structural security checks for the Zed extension.
//!
//! Zed owns worktree trust and executes the compiled extension outside the
//! Rust integration-test harness. Keep this check beside the other extension
//! configuration checks so a future edit cannot restore worktree PATH lookup.

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn artifact_contains(artifact: &[u8], needle: &[u8]) -> bool {
    artifact.windows(needle.len()).any(|window| window == needle)
}

#[test]
fn zed_extension_keeps_hostile_worktree_path_out_of_the_server_command() {
    let root = repo_root().join("editors/zed");
    let source = fs::read_to_string(root.join("wasm-src/src/lib.rs"))
        .expect("Zed extension source must be present");
    let manifest = fs::read_to_string(root.join("extension.toml.in"))
        .expect("Zed extension manifest template must be present");
    let readme = fs::read_to_string(root.join("README.md")).expect("Zed README must be present");
    let artifact = fs::read(root.join("extension.wasm"))
        .expect("tracked Zed extension artifact must be present");

    // A hostile worktree may put `jet` earlier on the worktree PATH. The
    // extension must return the literal command identity that the manifest
    // approves; it must never ask Zed for an executable path from Worktree.
    assert!(
        source.contains(
            "command: \"jet\".to_string(),\n            args: vec![\"self\".to_string(), \"lsp\".to_string()]"
        ),
        "{source}"
    );
    assert!(!source.contains("worktree.which"), "worktree PATH lookup returned: {source}");
    assert!(!source.contains("shell_env"), "worktree environment lookup returned: {source}");
    assert!(!source.contains("root_path"), "worktree path lookup returned: {source}");
    assert!(manifest.contains("kind = \"process:exec\""), "{manifest}");
    assert!(manifest.contains("command = \"jet\""), "{manifest}");
    assert!(manifest.contains("args = [\"self\", \"lsp\"]"), "{manifest}");
    assert!(
        !artifact_contains(&artifact, b"worktree.which"),
        "tracked artifact still embeds worktree PATH lookup"
    );
    assert!(
        !artifact_contains(&artifact, b"/target/debug/jet"),
        "tracked artifact still embeds a worktree debug-binary fallback"
    );

    // The trust decision is made by Zed. Keep the operational contract
    // visible to users rather than implying that source-tree files are trust.
    assert!(readme.contains("worktree-trust gate"), "{readme}");
    assert!(readme.contains("does not call\n`Worktree::which`"), "{readme}");
}
