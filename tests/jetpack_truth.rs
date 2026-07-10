//! E4-JP0 truth stop-line: every done Epoch 4 card has an explicit completion
//! boundary, and partial substrate cannot silently become a shipped claim.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

const MATRIX: &str = "docs/plans/epoch-4/truth-matrix.md";

#[test]
fn truth_matrix_covers_every_done_epoch4_card() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let raw = std::fs::read_to_string(root.join(MATRIX)).unwrap();
    let rows = matrix_rows(&raw);
    let mut done = done_epoch4_cards(root);
    done.insert(418); // Self-row exists before and after Tower closes this card.
    assert_eq!(
        rows.keys().copied().collect::<BTreeSet<_>>(),
        done,
        "truth matrix must match live projected Tower E4 done cards"
    );

    let allowed = [
        "live",
        "compatibility-only",
        "model-only",
        "schema-only",
        "fixture-only",
    ];
    for (num, (class, boundary)) in rows {
        assert!(allowed.contains(&class.as_str()), "#{num}: bad class {class}");
        assert!(!boundary.trim().is_empty(), "#{num}: empty completion boundary");
        if class != "live" {
            assert!(
                boundary.contains('#'),
                "#{num}: non-live row must name its active completion owner"
            );
        }
    }
}

fn matrix_rows(raw: &str) -> BTreeMap<u64, (String, String)> {
    let mut rows = BTreeMap::new();
    for line in raw.lines().filter(|line| line.starts_with("| #")) {
        let cells = line
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect::<Vec<_>>();
        assert_eq!(cells.len(), 4, "malformed truth row: {line}");
        let num = cells[0].trim_start_matches('#').parse::<u64>().unwrap();
        assert!(
            rows.insert(num, (cells[1].to_string(), cells[3].to_string()))
                .is_none(),
            "duplicate truth row for #{num}"
        );
    }
    rows
}

fn done_epoch4_cards(root: &Path) -> BTreeSet<u64> {
    let output = Command::new("node")
        .args([
            "Tower/tower.mjs",
            "card",
            "list",
            "--json",
            "--track",
            "epoch",
            "--epoch",
            "e4",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Tower projection failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = jet::Jetpack::JSON::parse(&String::from_utf8(output.stdout).unwrap()).unwrap();
    json.as_array()
        .unwrap()
        .iter()
        .filter(|card| card.get("phase").unwrap().as_str().unwrap() == "done")
        .map(|card| match card.get("num").unwrap() {
            jet::Jetpack::JSON::Json::Num(num) => *num as u64,
            other => panic!("card num is not numeric: {other:?}"),
        })
        .collect()
}
