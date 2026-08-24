mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn scratch(tag: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "jet-production-receipt-{tag}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn receipt(root: &Path) -> String {
    let reports = root.join(".jet/reports");
    let entries = fs::read_dir(&reports).unwrap();
    let path = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("receipt"))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("no production receipt in {}", reports.display()));
    fs::read_to_string(path).unwrap()
}

fn hex(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn production_failure_receipt_is_canonical_redacted_and_tier_stable() {
    let source_secret = "PRODUCTION_RECEIPT_SOURCE_MARKER_755";
    let input_secret = "PRODUCTION_RECEIPT_INPUT_MARKER_755";
    let environment_secret = "PRODUCTION_RECEIPT_ENV_MARKER_755";
    let source = format!("// {source_secret}\nfn run() {{ panic(\"{input_secret}\") }}\n");
    let jet = PathBuf::from(env!("CARGO_BIN_EXE_jet"));
    let mut modes = vec![vec!["run", "--interpret", "main.jet"]];
    modes.push(vec!["dev", "main.jet", "--watch=off"]);
    modes.push(vec!["dev", "main.jet", "--interpret", "--watch=off"]);
    if jet_jit::cranelift_host_supported() {
        modes.push(vec!["run", "main.jet"]);
    }
    modes.push(vec!["run", "--release", "main.jet"]);

    let mut receipts = Vec::new();
    for (index, mode) in modes.iter().enumerate() {
        let root = scratch(&format!("tier-{index}"));
        fs::write(root.join("main.jet"), &source).unwrap();
        let output = Command::new(&jet)
            .args(mode)
            .arg("--")
            .arg(input_secret)
            .env("JET_PRODUCTION_USER_DATA", environment_secret)
            .env("JET_CACHE_DIR", root.join("cache"))
            .current_dir(&root)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(70),
            "{} failed:\n{}",
            mode.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        let text = receipt(&root);
        assert!(text.starts_with("jet-development-receipt-v1\n"));
        assert!(text.contains("closure\t\t"));
        assert!(text.contains("input\t"));
        assert!(text.contains(&format!("failure-path\t{}\t{}", hex("code"), hex("E3001"))));
        assert!(text.contains(&format!(
            "failure-path\t{}\t{}",
            hex("file"),
            hex("main.jet")
        )));
        assert!(text.contains(&format!("failure-path\t{}\t{}", hex("line"), hex("2"))));
        assert!(text.contains(&format!(
            "failure-path\t{}\t{}",
            hex("function"),
            hex("run")
        )));
        for forbidden in [source_secret, input_secret, environment_secret] {
            assert!(!text.contains(forbidden), "receipt leaked `{forbidden}`");
        }
        receipts.push(text);
        let _ = fs::remove_dir_all(root);
    }

    assert!(receipts.windows(2).all(|pair| pair[0] == pair[1]));
}
