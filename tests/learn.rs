//! `jet learn` acceptance: packaged katas stay executable and teach from the
//! diagnostic produced by the current compiler.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const EXERCISES: &[(&str, &str, &str)] = &[
    (
        "01_unknown_function.jet",
        include_str!("../examples/learn/first_arc/01_unknown_function.solution.jet"),
        "E0102",
    ),
    (
        "02_type_mismatch.jet",
        include_str!("../examples/learn/first_arc/02_type_mismatch.solution.jet"),
        "E0112",
    ),
    (
        "03_entry_body.jet",
        include_str!("../examples/learn/first_arc/03_entry_body.solution.jet"),
        "E0621",
    ),
];

fn jet(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(args)
        .current_dir(cwd)
        .env("NO_COLOR", "1")
        .output()
        .unwrap_or_else(|error| panic!("jet {args:?} should start: {error}"))
}

fn combined(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn first_arc_is_offline_with_live_diagnostic_hints() {
    let scratch = common::Scratch::new("learn-first-arc");

    let first = jet(&["learn", "--watch=off"], &scratch.path);
    let first_text = combined(&first);
    assert!(!first.status.success(), "broken kata unexpectedly passed:\n{first_text}");
    assert!(first_text.contains("E0102"), "live diagnostic missing:\n{first_text}");
    assert!(first_text.contains("What this means:"), "live hint missing:\n{first_text}");
    assert!(first_text.contains("jet explain E0102"), "explain route missing:\n{first_text}");

    for (index, &(file, solution, _)) in EXERCISES.iter().enumerate() {
        let kata = scratch.path.join(".jet/learn/first-arc").join(file);
        assert!(kata.is_file(), "learn did not materialize {file}");
        fs::write(&kata, solution).unwrap();

        let next = jet(&["learn", "--watch=off"], &scratch.path);
        let next_text = combined(&next);
        if let Some(&(next_file, _, next_diagnostic)) = EXERCISES.get(index + 1) {
            assert!(!next.status.success(), "next kata unexpectedly passed:\n{next_text}");
            assert!(next_text.contains(next_file), "next kata missing:\n{next_text}");
            assert!(next_text.contains(next_diagnostic), "next diagnostic missing:\n{next_text}");
        } else {
            assert!(next.status.success(), "completed arc failed:\n{next_text}");
            assert!(next_text.contains("First arc complete"), "completion missing:\n{next_text}");
        }
    }

    let checked = jet(&["learn", "--check"], &scratch.path);
    let checked_text = combined(&checked);
    assert!(checked.status.success(), "curriculum check failed:\n{checked_text}");
    assert!(checked_text.contains("first arc"), "curriculum result missing:\n{checked_text}");
}

#[test]
fn first_arc_runs_in_the_dev_watch_loop() {
    let scratch = common::Scratch::new("learn-first-arc-watch");
    let mut child = Command::new(env!("CARGO_BIN_EXE_jet"))
        .arg("learn")
        .current_dir(&scratch.path)
        .env("NO_COLOR", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("jet learn watch should start");

    for &(file, solution, _) in EXERCISES {
        let kata = scratch.path.join(".jet/learn/first-arc").join(file);
        wait_for_file(&kata);
        fs::write(kata, solution).unwrap();
    }

    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "watch arc failed: {status:?}");
            break;
        }
        assert!(Instant::now() < deadline, "watch arc did not complete");
        thread::sleep(Duration::from_millis(50));
    }
}

fn wait_for_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !path.is_file() {
        assert!(Instant::now() < deadline, "learn did not materialize {}", path.display());
        thread::sleep(Duration::from_millis(50));
    }
}
