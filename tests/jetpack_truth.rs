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
    let cards = tower_cards(root);
    let done = cards
        .iter()
        .filter(|(_, card)| card.epoch == "e4" && card.phase == "done")
        .map(|(num, _)| *num)
        .collect::<BTreeSet<_>>();
    let audited = rows.keys().copied().collect::<BTreeSet<_>>();
    assert!(
        done.is_subset(&audited),
        "every live projected Tower E4 done card must have a truth row; missing {:?}",
        done.difference(&audited).collect::<Vec<_>>()
    );

    let allowed = [
        "live",
        "compatibility-only",
        "model-only",
        "schema-only",
        "fixture-only",
    ];
    for (num, (class, evidence, boundary)) in rows {
        let card = cards.get(&num).unwrap_or_else(|| panic!("#{num}: card missing from Tower"));
        assert!(allowed.contains(&class.as_str()), "#{num}: bad class {class}");
        assert!(evidence.len() >= 8, "#{num}: evidence is not specific");
        assert!(!boundary.trim().is_empty(), "#{num}: empty completion boundary");
        if num != 418 {
            assert!(!card.log_empty, "#{num}: done/reclassified claim has no Tower evidence log");
        }
        if class != "live" {
            let successors = card_refs(&boundary);
            assert!(!successors.is_empty(), "#{num}: non-live row needs successor");
            assert!(successors.iter().any(|successor| {
                cards.get(successor).is_some_and(|card| {
                    card.phase != "done" && card.phase != "frozen"
                })
            }), "#{num}: named successors are absent, done, or frozen: {successors:?}");
        }
    }
}

fn matrix_rows(raw: &str) -> BTreeMap<u64, (String, String, String)> {
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
            rows.insert(
                num,
                (
                    cells[1].to_string(),
                    cells[2].to_string(),
                    cells[3].to_string(),
                ),
            )
                .is_none(),
            "duplicate truth row for #{num}"
        );
    }
    rows
}

struct CardState {
    epoch: String,
    phase: String,
    log_empty: bool,
}

fn tower_cards(root: &Path) -> BTreeMap<u64, CardState> {
    let output = Command::new("node")
        .args(["Tower/tower.mjs", "card", "list", "--json"])
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
        .map(|card| {
            let num = match card.get("num").unwrap() {
                jet::Jetpack::JSON::Json::Num(num) => *num as u64,
                other => panic!("card num is not numeric: {other:?}"),
            };
            let log_empty = card.get("log").unwrap().as_array().unwrap().is_empty();
            (
                num,
                CardState {
                    epoch: card.get("epoch").unwrap().as_str().unwrap_or("").to_string(),
                    phase: card.get("phase").unwrap().as_str().unwrap().to_string(),
                    log_empty,
                },
            )
        })
        .collect()
}

fn card_refs(text: &str) -> Vec<u64> {
    let bytes = text.as_bytes();
    let mut refs = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'#' {
            let start = index + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end > start {
                refs.push(text[start..end].parse().unwrap());
                index = end;
                continue;
            }
        }
        index += 1;
    }
    refs
}
