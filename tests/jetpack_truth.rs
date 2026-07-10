//! E4-JP0 truth stop-line: every done Epoch 4 card has an explicit completion
//! boundary, and partial substrate cannot silently become a shipped claim.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

const MATRIX: &str = "docs/plans/epoch-4/truth-matrix.md";
const AUDITED: &[u64] = &[
    3, 5, 6, 13, 85, 90, 99, 139, 179, 185, 187, 188, 190, 191, 192, 193, 194,
    195, 196, 197, 198, 199, 200, 201, 202, 203, 204, 205, 206, 207, 208, 214, 215,
    229, 231, 232, 233, 234, 242, 330, 418,
];

#[test]
fn truth_matrix_covers_every_done_epoch4_card() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let raw = std::fs::read_to_string(root.join(MATRIX)).unwrap();
    let rows = matrix_rows(&raw);
    let cards = tower_cards(&live_repo_root(root));
    let done = cards
        .iter()
        .filter(|(_, card)| card.epoch == "e4" && card.phase == "done")
        .map(|(num, _)| *num)
        .collect::<BTreeSet<_>>();
    let audited = rows.keys().copied().collect::<BTreeSet<_>>();
    let expected = AUDITED.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(audited, expected, "truth matrix audited set drifted");
    assert!(done.is_subset(&audited), "live E4 done cards missing from audit");
    for reopened in [6, 330] {
        assert_eq!(
            cards.get(&reopened).map(|card| card.phase.as_str()),
            Some("ready"),
            "#{reopened} must remain reopened until its live successor lands"
        );
    }

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
        assert!(evidence_resolves(root, &evidence), "#{num}: evidence does not resolve: {evidence}");
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

#[test]
fn provider_realization_has_one_production_callsite() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();
    collect_direct_provider_calls(&root.join("crates/jetpack/src"), &mut violations);
    assert_eq!(
        violations,
        vec!["crates/jetpack/src/Store.rs".to_string()],
        "Provider realization must remain private behind Store::realize_verified"
    );
}

fn collect_direct_provider_calls(dir: &Path, found: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_direct_provider_calls(&path, found);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            let source = std::fs::read_to_string(&path).unwrap();
            if source.contains("Provider::realize(") || source.contains("Provider::realize_adapter(") {
                found.push(
                    path.strip_prefix(Path::new(env!("CARGO_MANIFEST_DIR")))
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    found.sort();
    found.dedup();
}

fn live_repo_root(root: &Path) -> std::path::PathBuf {
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(output.status.success(), "cannot locate live Git common dir");
    let common = std::path::PathBuf::from(
        String::from_utf8(output.stdout).unwrap().trim(),
    );
    common.parent().unwrap().to_path_buf()
}

fn evidence_resolves(root: &Path, evidence: &str) -> bool {
    let paths = evidence
        .split('`')
        .enumerate()
        .filter_map(|(index, token)| (index % 2 == 1).then_some(token))
        .collect::<Vec<_>>();
    !paths.is_empty() && paths.iter().all(|path| root.join(path).exists())
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
    // #461: done cards retire into `.tower/history.json` after the walk-back
    // buffer. JP0's audited set includes those retired cards, so the truth
    // stop-line must read live + history — `card list` alone is live-only.
    let mut cards = BTreeMap::new();
    let live = Command::new("node")
        .args(["Tower/tower.mjs", "card", "list", "--json"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        live.status.success(),
        "Tower projection failed: {}",
        String::from_utf8_lossy(&live.stderr)
    );
    let live_json = jet::Jetpack::JSON::parse(&String::from_utf8(live.stdout).unwrap()).unwrap();
    for card in live_json.as_array().unwrap() {
        ingest_tower_card(&mut cards, card, /*prefer_existing=*/ false);
    }
    if let Ok(raw) = std::fs::read_to_string(root.join(".tower/history.json")) {
        let hist = jet::Jetpack::JSON::parse(&raw).unwrap();
        if let Ok(arr) = hist.get("cards").and_then(|v| v.as_array()) {
            for card in arr {
                ingest_tower_card(&mut cards, card, /*prefer_existing=*/ true);
            }
        }
    }
    cards
}

fn ingest_tower_card(
    cards: &mut BTreeMap<u64, CardState>,
    card: &jet::Jetpack::JSON::Json,
    prefer_existing: bool,
) {
    let num = match card.get("num").unwrap() {
        jet::Jetpack::JSON::Json::Num(num) => *num as u64,
        other => panic!("card num is not numeric: {other:?}"),
    };
    if prefer_existing && cards.contains_key(&num) {
        return;
    }
    let log_empty = match card.get("log") {
        Ok(log) => log.as_array().map(|a| a.is_empty()).unwrap_or(true),
        Err(_) => true,
    };
    let epoch = card
        .get("epoch")
        .ok()
        .and_then(|v| v.as_str().ok())
        .unwrap_or("")
        .to_string();
    let phase = card
        .get("phase")
        .ok()
        .and_then(|v| v.as_str().ok())
        .unwrap_or("")
        .to_string();
    cards.insert(
        num,
        CardState {
            epoch,
            phase,
            log_empty,
        },
    );
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
