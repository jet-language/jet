//! Card #2197 criterion 4: one Hangar usage report spans sibling roots.

use std::fs;
use std::path::Path;

mod common;

#[path = "support/jetpack_fixtures.rs"]
mod jetpack_fixtures;
use jetpack_fixtures::{jetpack, Scratch};

#[cfg(unix)]
fn measured_bytes(path: &Path, seen: &mut std::collections::BTreeSet<(u64, u64)>) -> u64 {
    use std::os::unix::fs::MetadataExt as _;

    let metadata = fs::symlink_metadata(path).unwrap();
    if !seen.insert((metadata.dev(), metadata.ino())) {
        return 0;
    }
    let mut total = metadata.blocks() * 512;
    if metadata.is_dir() {
        for entry in fs::read_dir(path).unwrap() {
            total += measured_bytes(&entry.unwrap().path(), seen);
        }
    }
    total
}

#[cfg(unix)]
#[test]
fn hangar_du_all_reports_every_root_and_exact_physical_total() {
    let machine = Scratch::new("criterion4-machine");
    let first = machine.join("jet-omp");
    let second = machine.join("jet-omp2");
    let shared = machine.join("shared-cas");
    fs::create_dir_all(first.join("hangar/objects/one")).unwrap();
    fs::create_dir_all(second.join("hangar/objects/two")).unwrap();
    fs::create_dir_all(&shared).unwrap();
    fs::write(first.join("hangar/objects/one/payload"), vec![b'a'; 4096]).unwrap();
    fs::write(second.join("hangar/objects/two/payload"), vec![b'b'; 8192]).unwrap();
    fs::write(shared.join("shared-payload"), vec![b'c'; 2048]).unwrap();

    let output = jetpack()
        .args(["hangar", "du", "--all", "--json", "--no-color"])
        .env("JETPACK_ROOT", &first)
        .env("JETPACK_SHARED_CAS", &shared)
        .env("HOME", &machine.path)
        .env("XDG_DATA_HOME", machine.join("data"))
        .env("XDG_STATE_HOME", machine.join("state"))
        .env("XDG_CACHE_HOME", &machine.path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report = jetpack::JSON::parse(String::from_utf8_lossy(&output.stdout).trim()).unwrap();
    let roots = report.get("roots").unwrap().as_array().unwrap();
    assert!(roots.iter().any(|root| {
        root.get("root").ok().and_then(|value| value.as_str().ok()) == Some(first.to_str().unwrap())
    }));
    assert!(roots.iter().any(|root| {
        root.get("root")
            .ok()
            .and_then(|value| value.as_str().ok())
            == Some(second.to_str().unwrap())
    }));
    assert_eq!(
        report
            .get("shared_cas")
            .unwrap()
            .get("path")
            .unwrap()
            .as_str()
            .unwrap(),
        shared.to_str().unwrap()
    );

    let mut seen = std::collections::BTreeSet::new();
    let expected = measured_bytes(&shared, &mut seen)
        + measured_bytes(&first.join("hangar"), &mut seen)
        + measured_bytes(&second.join("hangar"), &mut seen);
    let actual = match report.get("total_bytes").unwrap() {
        jetpack::JSON::JSONValue::Number(value) => *value as u64,
        jetpack::JSON::JSONValue::Flt(value) => *value as u64,
        value => panic!("invalid total_bytes: {value:?}"),
    };
    assert_eq!(actual, expected);
}
