//! c129 (D-CAP4/D-CAP6/D-CAP8) — integration tests for the public-capability
//! freeze: manifest `api:` mode, durable interface metadata (`.api` snapshot),
//! drift detection (E0912), and capability signatures in the package pin/hash.

use std::sync::Mutex;

// The freeze drift pass reads a process-global env var; serialize tests that set it.
static ENV_LOCK: Mutex<()> = Mutex::new(());

// ── manifest api: mode (D-CAP4 / D-CAP6) ──────────────────────────────────

#[test]
fn manifest_api_mode_inferred_by_default() {
    use jet::Jetpack::PackageManifest;
    let mf = PackageManifest::parse(
        "payload: { name: \"p\", version: \"1\" }\npackages: { lib: { targets: [library] } }",
    )
    .expect("parses");
    assert_eq!(mf.packages[0].api, PackageManifest::ApiMode::Inferred);
    assert!(
        !mf.packages[0].api.freezes(),
        "inference never freezes (D-CAP6)"
    );
}

#[test]
fn manifest_api_mode_stable_and_explicit_freeze() {
    use jet::Jetpack::PackageManifest::{self, ApiMode};
    let stable = PackageManifest::parse(
        "payload: { name: \"p\", version: \"1\" }\npackages: { lib: { targets: [library { api: stable }] } }",
    )
    .expect("parses");
    assert_eq!(stable.packages[0].api, ApiMode::Stable);
    assert!(stable.packages[0].api.freezes());

    let explicit = PackageManifest::parse(
        "payload: { name: \"p\", version: \"1\" }\npackages: { lib: { targets: [library { api: explicit }] } }",
    )
    .expect("parses");
    assert_eq!(explicit.packages[0].api, ApiMode::Explicit);
    assert!(explicit.packages[0].api.freezes());
}

#[test]
fn manifest_api_mode_ignored_on_non_library() {
    // D-CAP5: only a library target emits capability metadata.
    use jet::Jetpack::PackageManifest::{self, ApiMode};
    let mf = PackageManifest::parse(
        "payload: { name: \"p\", version: \"1\" }\npackages: { app: executable { api: stable } }",
    )
    .expect("parses");
    assert_eq!(mf.packages[0].api, ApiMode::Inferred);
}

// ── durable interface metadata (ApiSnapshot) ──────────────────────────────

#[test]
fn snapshot_freezes_resolved_sigils_round_trip() {
    use jet::Diagnostics::Span;
    use jet::Publish::ApiFreeze;
    use jet::AST::{AccessConvention, Func, Item, Param, Type};

    let z = Span::new(0, 0);
    let param = |name: &str, c: AccessConvention, ty: Type| Param {
        convention: c,
        name: name.to_string(),
        name_span: z,
        ty,
        ty_span: z,
        default: None,
        variadic: false,
        variadic_bound_list: None,
    };
    let func = |name: &str, is_pub: bool, params: Vec<Param>| {
        Item::Func(Func {
            is_pub,
            is_package_pub: false,
            external_type: None,
            name: name.to_string(),
            name_span: z,
            type_params: vec![],
            params,
            return_type: None,
            is_view_return: false,
            is_unsafe: false,
            is_pure: false,
            is_sanitizer: false,
            is_reactive: false,
            declared_effects: None,
            pre: vec![],
            post: vec![],
            effect_via: None,
            state_requires: None,
            state_transition: None,
            web_marker: None,
            is_must_use: false,
            must_use_span: None,
            is_inline: false,
            is_inline_always: false,
            inline_span: None,
            body: vec![],
        })
    };

    let items = vec![
        func(
            "scale",
            true,
            vec![param(
                "v",
                AccessConvention::Write,
                Type::Named("Vec3".into()),
            )],
        ),
        func("internal", false, vec![]), // private — excluded
    ];
    let snap = ApiFreeze::snapshot_from_items(&items, "vecmath", "1.0.0");
    assert_eq!(
        snap.funcs.len(),
        1,
        "private fns are not part of the contract"
    );
    assert_eq!(snap.funcs[0].signature, "fn scale(v: &Vec3)");

    let text = snap.write();
    let parsed = ApiFreeze::ApiSnapshot::parse(&text).expect("round trips");
    assert_eq!(parsed, snap);
}

// ── capability signatures in the package pin/hash ─────────────────────────

#[test]
fn fingerprint_folds_in_capability_digest() {
    use jet::Diagnostics::Span;
    use jet::Publish::ApiFreeze;
    use jet::AST::{AccessConvention, Func, Item, Param, Type};

    let z = Span::new(0, 0);
    let mk = |conv: AccessConvention| {
        let items = vec![Item::Func(Func {
            is_pub: true,
            is_package_pub: false,
            external_type: None,
            name: "scale".into(),
            name_span: z,
            type_params: vec![],
            params: vec![Param {
                convention: conv,
                name: "v".into(),
                name_span: z,
                ty: Type::Named("Vec3".into()),
                ty_span: z,
                default: None,
                variadic: false,
                variadic_bound_list: None,
            }],
            return_type: None,
            is_view_return: false,
            is_unsafe: false,
            is_pure: false,
            is_sanitizer: false,
            is_reactive: false,
            declared_effects: None,
            pre: vec![],
            post: vec![],
            effect_via: None,
            state_requires: None,
            state_transition: None,
            web_marker: None,
            is_must_use: false,
            must_use_span: None,
            is_inline: false,
            is_inline_always: false,
            inline_span: None,
            body: vec![],
        })];
        ApiFreeze::snapshot_from_items(&items, "vecmath", "1.0.0").capability_digest()
    };
    let read_digest = mk(AccessConvention::Read);
    let write_digest = mk(AccessConvention::Write);

    let fp_read = jet::Lock::compute_fingerprint("sha256-tree", &[], &read_digest);
    let fp_write = jet::Lock::compute_fingerprint("sha256-tree", &[], &write_digest);
    assert_ne!(
        fp_read, fp_write,
        "a read → & capability change must shift the package pin"
    );
}

// ── drift detection (E0912) end-to-end through compile ────────────────────

const DRIFT_SRC: &str = r#"
struct Vec3 { x: Float, y: Float, z: Float }

pub fn scale(v: Vec3, factor: Float) {
    v.x = v.x * factor
}

fn run() {
    p := Vec3.{ x: 1.0, y: 2.0, z: 3.0 }
    scale(&p, 2.0)
    print("{p.x}")
}
"#;

const STABLE_SRC: &str = r#"
struct Vec3 { x: Float, y: Float, z: Float }

pub fn length_sq(v: Vec3) -> Float {
    return v.x * v.x + v.y * v.y + v.z * v.z
}

fn run() {
    p := Vec3.{ x: 1.0, y: 2.0, z: 3.0 }
    print("{length_sq(p)}")
}
"#;

/// Install a frozen `.api` snapshot in a temp cache dir, compile `src`, and
/// return the diagnostic codes.
fn compile_with_frozen(frozen: &str, src: &str) -> Vec<String> {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!(
        "jet_cap_freeze_it_{}_{}",
        std::process::id(),
        frozen.len() + src.len()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("vecmath.api"), frozen).unwrap();
    std::env::set_var("JET_API_CACHE_DIR", &dir);
    let result = jet::compile(src);
    std::env::remove_var("JET_API_CACHE_DIR");
    std::fs::remove_dir_all(&dir).ok();
    match result {
        Ok(_) => vec![],
        Err(diags) => diags.iter().map(|d| d.code.to_string()).collect(),
    }
}

const FROZEN_SCALE_READ: &str =
    "api_version = 1\npackage = vecmath\npublished_version = 1.0.0\nfn scale(v: Vec3, factor: Float)\n";

#[test]
fn read_to_write_drift_against_frozen_is_e0912() {
    // Frozen contract has `scale(v: Vec3)` (read); the source now mutates `v`,
    // so D-CAP8 resolves it to `&Vec3` — a breaking drift.
    let codes = compile_with_frozen(FROZEN_SCALE_READ, DRIFT_SRC);
    assert!(
        codes.iter().any(|c| c == "E0912"),
        "expected E0912 for read → & drift, got {:?}",
        codes
    );
}

#[test]
fn matching_signature_against_frozen_is_clean() {
    // Frozen `length_sq(v: Vec3)` matches the read-only source signature.
    let frozen =
        "api_version = 1\npackage = vecmath\npublished_version = 1.0.0\nfn length_sq(v: Vec3) -> Float\n";
    let codes = compile_with_frozen(frozen, STABLE_SRC);
    assert!(
        !codes.iter().any(|c| c == "E0912"),
        "an unchanged contract must not drift, got {:?}",
        codes
    );
}

#[test]
fn no_frozen_contract_means_no_drift_check() {
    // A drifting source with no frozen snapshot present (inferred-default library /
    // first release) compiles clean — inference still guarantees safety (D-CAP6).
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("jet_cap_freeze_empty_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("JET_API_CACHE_DIR", &dir);
    let result = jet::compile(DRIFT_SRC);
    std::env::remove_var("JET_API_CACHE_DIR");
    std::fs::remove_dir_all(&dir).ok();
    let codes: Vec<String> = match result {
        Ok(_) => vec![],
        Err(diags) => diags.iter().map(|d| d.code.to_string()).collect(),
    };
    assert!(
        !codes.iter().any(|c| c == "E0912"),
        "no frozen snapshot ⇒ no drift check, got {:?}",
        codes
    );
}
