//! Card #2197 criterion 2: malformed Hangar objects do not stop GC.

use std::fs;

mod common;

#[path = "support/jetpack_fixtures.rs"]
mod jetpack_fixtures;
use jetpack_fixtures::{jetpack, now_secs, write_hangar_meta, Scratch};

#[test]
fn clean_quarantines_malformed_object_and_collects_other_objects() {
    let root = Scratch::new("criterion2-root");
    let shared = Scratch::new("criterion2-shared-cas");
    std::env::set_var("JETPACK_SHARED_CAS", &shared.path);

    let stale = write_hangar_meta(&root.path, "criterion2-stale", "stale", "1.0", Some(1)).0;
    let fresh = write_hangar_meta(
        &root.path,
        "criterion2-fresh",
        "fresh",
        "1.0",
        Some(now_secs()),
    )
    .0;
    let malformed = root.path.join("hangar/nix-index");
    fs::create_dir_all(&malformed).unwrap();
    fs::write(malformed.join("payload"), "metadata missing").unwrap();

    let output = jetpack()
        .args(["hangar", "clean", "--no-color", "--yes"])
        .env("JETPACK_ROOT", &root.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!stale.exists(), "valid stale sibling should be collected");
    assert!(fresh.exists(), "fresh sibling should remain");
    assert!(
        !malformed.exists(),
        "malformed object should leave object pool"
    );

    let names = fs::read_dir(root.path.join("hangar/quarantine"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(
        names
            .iter()
            .any(|name| name.contains("nix-index") && name.contains("missing-metadata")),
        "quarantine names: {names:?}"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("quarantined 1 invalid object"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
