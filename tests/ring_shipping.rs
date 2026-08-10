//! T3 (card #99, D-JPK-RINGSHIP1=C): ring package shipping.
//!
//! A ring library (`core.http`, `core.regex`, …) resolves from a realized
//! hangar object when the active toolchain object stages a prebuilt artifact for
//! it, and from the compiler-embedded template otherwise. One resolution path,
//! two sources, no user-visible difference. This is its own test binary so the
//! process-global `JET_TOOLCHAIN_OBJECT` env it sets never races the parallel
//! unit tests in other binaries.

mod common;

use jet::Loader::{self, RingResolution};
use jet::Syntax;
use std::path::PathBuf;

fn scratch(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "ring-ship-{tag}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn ring_module_realizes_from_hangar() {
    // No active toolchain object → the ring resolves from the embedded template
    // (rung-0 magic preserved).
    std::env::remove_var(Syntax::TOOLCHAIN_OBJECT_ENV);
    assert!(
        !Syntax::is_ring_module_staged("http"),
        "with no toolchain object, no ring is staged"
    );
    assert_eq!(
        Loader::resolve_ring_module("http"),
        RingResolution::Embedded
    );

    // A fixture toolchain object that carries prebuilt `http` and `regex`
    // artifacts → those rings become staged and resolve from the hangar object;
    // a ring it does not carry still falls back to the embedded template.
    let obj = scratch("obj");
    let ring = obj.join("ring");
    std::fs::create_dir_all(&ring).unwrap();
    std::fs::write(ring.join("http"), "prebuilt-http").unwrap();
    std::fs::write(ring.join("regex"), "prebuilt-regex").unwrap();
    std::env::set_var(Syntax::TOOLCHAIN_OBJECT_ENV, &obj);

    assert!(
        Syntax::is_ring_module_staged("http"),
        "is_ring_module_staged(\"http\") must flip true when the object stages it"
    );
    assert!(Syntax::is_ring_module_staged("regex"));
    // The loader resolves the staged artifact from the hangar object.
    match Loader::resolve_ring_module("http") {
        RingResolution::Staged(path) => {
            assert_eq!(path, ring.join("http"), "must point at the staged artifact");
        }
        other => panic!("expected Staged, got {other:?}"),
    }
    // A ring the object does not carry falls back to embedded (no skew, no error).
    assert!(!Syntax::is_ring_module_staged("db"));
    assert_eq!(Loader::resolve_ring_module("db"), RingResolution::Embedded);

    // A non-ring name is never staged.
    assert!(!Syntax::is_ring_module_staged("not_a_ring"));

    std::env::remove_var(Syntax::TOOLCHAIN_OBJECT_ENV);
    std::fs::remove_dir_all(&obj).ok();
}

#[test]
fn ring_platform_miss_diagnostic_exists() {
    // E1241 — the RINGSHIP1=B hard-error / RINGSHIP1=C informational form.
    let d = Loader::e1241_ring_platform_miss("http");
    assert_eq!(d.code, "E1241");
    assert!(d.what.contains("core.http"));
}
