//! D-BUILDNORM1=A (Tower #85): the content-addressed artifact-store key is
//! `SHA256(canonical_bytes(pre-sema AST) + profile + toolchain-salt)`. These
//! tests pin the six normalization properties of that key — the contract a
//! description alone can't enforce (I4/I5).
//!
//! The key is computed exactly as the compiler computes it for a real build:
//! load the program through `jet::Loader::load_entry_with_overlay` (lex + parse
//! + import resolution, *no sema*) and hash it with
//! `jet::CanonicalAST::ast_cache_key`.

mod common;

use jet::Syntax::RuntimeLayer;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Write `src` to a fresh temp dir under a *fixed* basename (so the module's
/// filename-derived alias is identical across variants — only the AST content
/// varies), load its pre-sema bundle, and return the cache key.
fn key_with(src: &str, profile_tag: &str, version: &str) -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir: PathBuf =
        std::env::temp_dir().join(format!("jet-buildnorm-{}-{}", std::process::id(), n));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("prog.jet");
    std::fs::write(&file, src).unwrap();
    let bundle = jet::Loader::load_entry_with_overlay(file.to_str().unwrap(), None, false)
        .unwrap_or_else(|d| panic!("test program should parse:\n{src}\n{:?}", d));
    let key =
        jet::CanonicalAST::ast_cache_key(&bundle, profile_tag, version, &bundle.build_facts);
    std::fs::remove_dir_all(&dir).ok();
    key
}

/// Re-key one parsed bundle after changing only its target dossier.
fn key_with_target_dossier(
    bundle: &jet::AST::ProgramBundle,
    mutate: impl FnOnce(&mut TargetDossier),
) -> String {
    let mut facts = bundle.build_facts.clone();
    mutate(&mut facts.target_dossier);
    jet::CanonicalAST::ast_cache_key(bundle, "default", "test-version", &facts)
}

/// Re-key one parsed bundle after changing only its target triple.
fn key_with_target_triple(bundle: &jet::AST::ProgramBundle, target_triple: &str) -> String {
    let mut facts = bundle.build_facts.clone();
    facts.target_triple = target_triple.to_string();
    jet::CanonicalAST::ast_cache_key(bundle, "default", "test-version", &facts)
}

/// The common case: default profile, a fixed version salt.
fn key(src: &str) -> String {
    key_with(src, "default", "test-version")
}
fn scratch_dir(label: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "jet-buildnorm-scriptdeps-{label}-{}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_scratch_file(root: &Path, relative: &str, contents: &[u8]) -> PathBuf {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, contents).unwrap();
    path
}

fn inline_dep(name: &str, selector: &str) -> jet::ScriptDeps::InlineDep {
    jet::ScriptDeps::InlineDep {
        name: name.to_string(),
        selector: selector.to_string(),
        span: jet::Diagnostics::Span::new(0, 0),
    }
}

fn script_cache_digest(
    source_hash: &str,
    dependencies: &[jet::ScriptDeps::Resolved],
    toolchain: &str,
    target: &str,
) -> String {
    jet::ScriptDeps::cache_identity(source_hash, dependencies, toolchain, target).digest()
}

// ── ScriptDeps cache identity and preparation guards ───────────────────────

#[test]
fn script_cache_identity_changes_for_source_dependency_toolchain_and_target() {
    let root = scratch_dir("identity");
    let script = write_scratch_file(&root, "standalone.jet", b"print(\"before\")\n");
    let dependency_file = write_scratch_file(
        &root,
        ".jet/inline-deps/textkit/1.4.2/value.jet",
        b"pub fn value() String {\n    return \"before\"\n}\n",
    );
    let dependency = inline_dep("textkit", "1.4.2");
    let resolved = jet::ScriptDeps::resolve(&dependency, &root).unwrap();
    let source_hash = jet::ScriptDeps::file_hash(&script).unwrap();
    let baseline = script_cache_digest(
        &source_hash,
        std::slice::from_ref(&resolved),
        "jet-toolchain-a",
        "x86_64-unknown-linux-gnu",
    );

    write_scratch_file(&root, "standalone.jet", b"print(\"after\")\n");
    let changed_source_hash = jet::ScriptDeps::file_hash(&script).unwrap();
    assert_ne!(
        baseline,
        script_cache_digest(
            &changed_source_hash,
            std::slice::from_ref(&resolved),
            "jet-toolchain-a",
            "x86_64-unknown-linux-gnu",
        ),
        "changed standalone source must invalidate the prepared identity"
    );

    write_scratch_file(
        &root,
        ".jet/inline-deps/textkit/1.4.2/value.jet",
        b"pub fn value() String {\n    return \"after\"\n}\n",
    );
    let changed_dependency = jet::ScriptDeps::resolve(&dependency, &root).unwrap();
    assert_ne!(
        resolved.content_hash, changed_dependency.content_hash,
        "the changed local dependency must receive a new content hash"
    );
    assert_ne!(
        baseline,
        script_cache_digest(
            &source_hash,
            std::slice::from_ref(&changed_dependency),
            "jet-toolchain-a",
            "x86_64-unknown-linux-gnu",
        ),
        "changed dependency material must invalidate the prepared identity"
    );

    assert_ne!(
        baseline,
        script_cache_digest(
            &source_hash,
            std::slice::from_ref(&resolved),
            "jet-toolchain-b",
            "x86_64-unknown-linux-gnu",
        ),
        "changed toolchain identity must invalidate the prepared identity"
    );
    assert_ne!(
        baseline,
        script_cache_digest(
            &source_hash,
            std::slice::from_ref(&resolved),
            "jet-toolchain-a",
            "aarch64-unknown-linux-gnu",
        ),
        "changed target identity must invalidate the prepared identity"
    );

    let mut relocated = resolved.clone();
    relocated.dir = PathBuf::from("/another/checkout");
    assert_eq!(
        baseline,
        script_cache_digest(
            &source_hash,
            std::slice::from_ref(&relocated),
            "jet-toolchain-a",
            "x86_64-unknown-linux-gnu",
        ),
        "moving an otherwise identical local source must not change its identity"
    );

    assert_eq!(
        fs::read(&dependency_file).unwrap(),
        b"pub fn value() String {\n    return \"after\"\n}\n"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn script_dependency_missing_material_fails_closed_without_creating_state() {
    let root = scratch_dir("missing");
    let name = format!(
        "missing_buildnorm_{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let dependency = inline_dep(&name, "1.0.0");
    let error = jet::ScriptDeps::resolve(&dependency, &root).unwrap_err();

    assert!(matches!(
        &error,
        jet::ScriptDeps::Unresolved::UnknownPackage
    ));
    assert_eq!(jet::ScriptDeps::e1253(&dependency, &error).code, "E1253");
    assert!(
        !root.join(".jet").exists(),
        "a missing dependency must not materialize a cache or lock tree"
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn script_dependency_hostile_material_fails_closed_without_creating_state() {
    use std::os::unix::fs::symlink;

    let root = scratch_dir("hostile");
    let package = root.join(".jet/inline-deps/hostile/1.0.0");
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("value.jet"), b"stable\n").unwrap();
    symlink(".", package.join("loop")).unwrap();

    let dependency = inline_dep("hostile", "1.0.0");
    let error = jet::ScriptDeps::resolve(&dependency, &root).unwrap_err();
    assert!(matches!(
        &error,
        jet::ScriptDeps::Unresolved::InvalidTree(_)
    ));
    let diagnostic = jet::ScriptDeps::e1253(&dependency, &error);
    assert_eq!(diagnostic.code, "E1253");
    assert!(
        diagnostic.why.contains("cannot be hashed safely"),
        "hostile dependency diagnostics must explain the closed failure"
    );
    assert!(!root.join("standalone.jet.lock").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn standalone_local_selector_resolves_without_preparation_side_effects() {
    let root = scratch_dir("selector");
    let script = write_scratch_file(&root, "standalone.jet", b"print(\"stable\")\n");
    write_scratch_file(
        &root,
        ".jet/inline-deps/local_only/1.4.1/value.jet",
        b"one\n",
    );
    let selected_source = write_scratch_file(
        &root,
        ".jet/inline-deps/local_only/1.4.2/value.jet",
        b"two\n",
    );
    let source_before = fs::read(&script).unwrap();
    let selected_source_before = fs::read(&selected_source).unwrap();
    let source_hash = jet::ScriptDeps::file_hash(&script).unwrap();

    let loose = inline_dep("local_only", "1.4");
    let resolved_loose = jet::ScriptDeps::resolve(&loose, &root).unwrap();
    assert_eq!(resolved_loose.resolved_version, "1.4.2");
    assert_eq!(
        resolved_loose.dir,
        selected_source.parent().unwrap().to_path_buf()
    );

    let exact = inline_dep("local_only", "1.4.2");
    let resolved_exact = jet::ScriptDeps::resolve(&exact, &root).unwrap();
    assert_eq!(
        resolved_loose.content_hash, resolved_exact.content_hash,
        "loose and exact selectors should use the same locally selected source"
    );
    assert_ne!(
        script_cache_digest(
            &source_hash,
            std::slice::from_ref(&resolved_loose),
            "jet-toolchain-a",
            "x86_64-unknown-linux-gnu",
        ),
        script_cache_digest(
            &source_hash,
            std::slice::from_ref(&resolved_exact),
            "jet-toolchain-a",
            "x86_64-unknown-linux-gnu",
        ),
        "the selector remains part of the identity even when it selects the same source"
    );

    assert_eq!(fs::read(&script).unwrap(), source_before);
    assert_eq!(fs::read(&selected_source).unwrap(), selected_source_before);
    assert!(
        !root.join("package.jet").exists(),
        "standalone preparation must not lift local dependencies into a manifest"
    );
    assert!(
        !root.join("standalone.jet.lock").exists(),
        "resolution must not write a lock sidecar"
    );
    assert!(
        !root.join(".jet/lock").exists(),
        "resolution must not create managed lock state"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn whitespace_insensitive() {
    let a = "fn add(a: Int, b: Int) {\n    print(a + b)\n}\n";
    let b = "fn  add( a : Int ,  b : Int )  {\n\n        print(  a  +  b  )\n\n}\n";
    assert_eq!(key(a), key(b), "reformatting must not change the key");
}

#[test]
fn comment_insensitive() {
    let a = "fn add(a: Int, b: Int) {\n    print(a + b)\n}\n";
    let b = "/// doc comment\n// leading comment\nfn add(a: Int, b: Int) {\n  // inline\n  print(a + b) // trailing\n}\n";
    assert_eq!(key(a), key(b), "adding comments must not change the key");
}

#[test]
fn rename_sensitive() {
    // The D-BUILDNORM1 ratified example: renaming locals changes the key.
    let a = "fn add(a: Int, b: Int) {\n    print(a + b)\n}\n";
    let b = "fn add(x: Int, y: Int) {\n    print(x + y)\n}\n";
    assert_ne!(key(a), key(b), "renaming a parameter must change the key");
}

#[test]
fn reorder_sensitive() {
    let a = "fn add(a: Int, b: Int) {\n    print(a + b)\n}\n";
    let b = "fn add(a: Int, b: Int) {\n    print(b + a)\n}\n";
    assert_ne!(key(a), key(b), "reordering operands must change the key");
}

#[test]
fn profile_sensitive() {
    let src = "fn add(a: Int, b: Int) {\n    print(a + b)\n}\n";
    let default = key_with(src, "default", "v");
    let small = key_with(src, "small", "v");
    let release = key_with(src, "release", "v");
    assert_ne!(default, small, "profiles must not share a cache entry");
    assert_ne!(default, release);
    assert_ne!(small, release);
}

#[test]
fn version_sensitive() {
    // A toolchain (or manifest) salt change must invalidate the entry — the
    // guard that a codegen change never serves a stale binary for an identical
    // AST (Tower #85 §1 step 3).
    let src = "fn add(a: Int, b: Int) {\n    print(a + b)\n}\n";
    assert_ne!(
        key_with(src, "default", "0.1.0"),
        key_with(src, "default", "0.2.0"),
        "a version-salt change must change the key"
    );
}

// ── Extra discrimination guards (belt-and-suspenders for the serializer) ──

#[test]
fn operator_sensitive() {
    let a = "fn f(a: Int, b: Int) {\n    print(a + b)\n}\n";
    let b = "fn f(a: Int, b: Int) {\n    print(a - b)\n}\n";
    assert_ne!(key(a), key(b), "a different operator must change the key");
}

#[test]
fn literal_sensitive() {
    let a = "fn f() {\n    print(1)\n}\n";
    let b = "fn f() {\n    print(2)\n}\n";
    assert_ne!(key(a), key(b), "a different literal must change the key");
}

#[test]
fn script_body_sensitive() {
    let a = "print(\"a\")\n";
    let b = "print(\"b\")\n";
    let c = "print(\"a\")\nprint(\"b\")\n";
    assert_ne!(
        key(a),
        key(b),
        "a different script statement must change the key"
    );
    assert_ne!(
        key(a),
        key(c),
        "an added script statement must change the key"
    );
}

#[test]
fn string_literal_content_sensitive() {
    // String-literal contents are part of the program and must change the key.
    // (The exact `Span {…}`-lookalike-inside-a-literal robustness of the span
    // stripper is pinned by the CanonicalAST unit test; Jet reads `{ }` in
    // strings as interpolation, so we use brace-free literals here.)
    let a = "fn f() {\n    print(\"Span start 1 end 2\")\n}\n";
    let b = "fn f() {\n    print(\"Span start 7 end 3\")\n}\n";
    assert_ne!(
        key(a),
        key(b),
        "string-literal contents must survive span-stripping"
    );
}

#[test]
fn stable_across_reloads() {
    let src = "fn add(a: Int, b: Int) {\n    print(a + b)\n}\n";
    assert_eq!(key(src), key(src), "same input must yield the same key");
    assert_eq!(key(src).len(), 64, "key is a 64-hex SHA-256 digest");
}

#[test]
fn target_dossier_inputs_invalidate_artifact_key() {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("jet-buildnorm-dossier-{}-{}", std::process::id(), n));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("prog.jet");
    let src = "fn add(a: Int, b: Int) {\n    print(a + b)\n}\n";
    std::fs::write(&file, src).unwrap();
    let bundle = jet::Loader::load_entry_with_overlay(file.to_str().unwrap(), None, false)
        .unwrap_or_else(|d| panic!("test program should parse:\n{src}\n{:?}", d));

    let baseline = key_with_target_dossier(&bundle, |_| {});
    assert_eq!(
        baseline,
        key_with_target_dossier(&bundle, |_| {}),
        "identical input must keep the artifact key stable"
    );

    let assert_changes = |name: &str, mutate: fn(&mut TargetDossier)| {
        assert_ne!(
            baseline,
            key_with_target_dossier(&bundle, mutate),
            "{name} must invalidate the artifact key"
        );
    };
    assert_changes("runtime layer", |dossier| dossier.layer = RuntimeLayer::Core);
    assert_changes("provider digest/identity", |dossier| {
        dossier.provider_identity = "target-providers-v1:sha256:provider-b".to_string()
    });
    assert_changes("Prelude closure", |dossier| {
        dossier.closure_identity = "prelude-hosted-v2:sha256:closure-b".to_string()
    });
    assert_changes("linker", |dossier| {
        dossier.linker_identity = "lld:sha256:linker-b".to_string()
    });
    assert_changes("execution tier", |dossier| dossier.tier_identity = "aot".to_string());
    assert_changes("compiler", |dossier| dossier.compiler_identity = "jet@next".to_string());
    assert_changes("environment", |dossier| {
        dossier.environment_identity = "wasi".to_string()
    });
    assert_changes("dependency graph", |dossier| {
        dossier.dependency_identity = "deps:sha256:dependency-b".to_string()
    });
    assert_ne!(
        baseline,
        key_with_target_triple(&bundle, "wasm32-unknown-unknown"),
        "target triple must invalidate the artifact key"
    );

    std::fs::remove_dir_all(&dir).ok();
}
