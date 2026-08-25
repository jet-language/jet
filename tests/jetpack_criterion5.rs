//! Card #2197 criterion 5: concurrent agent realizations share physical bytes.

#[cfg(unix)]
use std::fs;
#[cfg(unix)]
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
fn thirty_concurrent_agent_realizations_stay_near_single_footprint() {
    use std::os::unix::fs::MetadataExt as _;

    let machine = Scratch::new("criterion5-machine");
    let shared = machine.join("shared-cas");
    let source = machine.join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("payload"), vec![b'x'; 4 * 1024 * 1024]).unwrap();
    let roots = (0..30)
        .map(|index| machine.join(&format!("agent-{index}")))
        .collect::<Vec<_>>();
    let home = machine.path.clone();
    let data_home = machine.join("data");
    let state_home = machine.join("state");

    std::thread::scope(|scope| {
        let handles = roots
            .iter()
            .map(|root| {
                let root = root.clone();
                let source = source.clone();
                let shared = shared.clone();
                let home = home.clone();
                let data_home = data_home.clone();
                let state_home = state_home.clone();
                scope.spawn(move || {
                    let output = jetpack()
                        .args([
                            "hangar",
                            "ingest",
                            source.to_str().unwrap(),
                            "--name",
                            "same-package",
                            "--version",
                            "1",
                            "--ref",
                            "path:same-package",
                            "--no-color",
                        ])
                        .env("JETPACK_ROOT", &root)
                        .env("JETPACK_SHARED_CAS", &shared)
                        .env("HOME", &home)
                        .env("XDG_DATA_HOME", &data_home)
                        .env("XDG_STATE_HOME", &state_home)
                        .env("XDG_CACHE_HOME", &home)
                        .env_remove("JETPACK_FIXTURES")
                        .output()
                        .unwrap();
                    assert!(
                        output.status.success(),
                        "stdout: {}\nstderr: {}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    );
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().unwrap();
        }
    });

    let payload = fs::read_dir(roots[0].join("hangar/objects"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.is_dir())
        .unwrap()
        .join("payload");
    assert!(fs::metadata(payload).unwrap().nlink() >= 30);

    let single_bytes = {
        let mut seen = std::collections::BTreeSet::new();
        measured_bytes(&shared, &mut seen) + measured_bytes(&roots[0].join("hangar"), &mut seen)
    };
    let multi_bytes = {
        let mut seen = std::collections::BTreeSet::new();
        let mut total = measured_bytes(&shared, &mut seen);
        for root in &roots {
            total += measured_bytes(&root.join("hangar"), &mut seen);
        }
        total
    };
    assert!(
        multi_bytes <= single_bytes.saturating_mul(4),
        "30-root footprint {multi_bytes} exceeds single-root footprint {single_bytes}"
    );
}
