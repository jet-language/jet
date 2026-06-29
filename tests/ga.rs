//! E2-M17 GA checklist — asserts that Epoch 2 exit criteria still hold at the
//! compiler level. Showcase programs were retired from `examples/`; milestone
//! coverage now lives in `examples/features/` (I5 golden tests).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// ── 1. Every diagnostic code has a jet explain entry ──────────────────────

/// Mirrors the check in cli.rs `every_registered_code_has_an_explain_entry`.
#[test]
fn ga_every_diagnostic_has_explain() {
    let md = fs::read_to_string(root().join("docs/spec/diagnostics.md")).expect("diagnostics.md");
    let index = jet::Explain::index();

    let mut missing = Vec::new();
    for line in md.lines() {
        let line = line.trim();
        if !line.starts_with("| E") && !line.starts_with("| L") {
            continue;
        }
        let first = line
            .trim_matches('|')
            .split('|')
            .next()
            .unwrap_or("")
            .trim();
        if is_code(first) && !index.contains_key(first) {
            missing.push(first.to_string());
        }
    }
    assert!(
        missing.is_empty(),
        "M17 GA gate: these diagnostic codes lack a `jet explain` entry:\n  {}",
        missing.join(", ")
    );
}

fn is_code(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 5 && (b[0] == b'E' || b[0] == b'L') && b[1..].iter().all(|c| c.is_ascii_digit())
}

// ── 2. Milestone feature examples are front-end clean ─────────────────────

/// D-GA1=B milestone coverage now lives under `examples/features/`.
#[test]
fn ga_milestone_features_front_end_clean() {
    let features: &[(&str, &str)] = &[
        ("47_library.jet", "library authoring"),
        ("48_lowlevel.jet", "expert low-level tier"),
        ("57_http_server.jet", "HTTP service"),
        ("61_freestanding.jet", "freestanding smoke"),
    ];

    let features_dir = root().join("examples/features");
    for (file, desc) in features {
        let path = features_dir.join(file);
        assert!(
            path.is_file(),
            "M17 GA gate: feature example missing: {}",
            path.display()
        );
        let src =
            fs::read_to_string(&path).unwrap_or_else(|_| panic!("cannot read {}", path.display()));
        let result = jet::compile_with_path(&src, path.to_str().unwrap());
        assert!(
            result.is_ok(),
            "M17 GA gate: '{}' failed front end:\n{:?}",
            desc,
            result.err()
        );
    }
}

// ── 3. Hard size budgets (D-GA2=B) ────────────────────────────────────────

#[test]
fn ga_feature_size_budgets() {
    let jet = jet_bin();
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc || !jet.exists() {
        eprintln!("note: skipping GA size budgets (need jet + rustc)");
        return;
    }

    let budgets: &[(&str, u64)] = &[
        ("47_library.jet", 4_194_304),
        ("48_lowlevel.jet", 4_194_304),
        ("61_freestanding.jet", 4_194_304),
    ];

    let features_dir = root().join("examples/features");
    let build_dir = std::env::temp_dir().join(format!("jet_ga_budgets_{}", std::process::id()));
    fs::create_dir_all(build_dir.join("build")).unwrap();

    for (file, max_bytes) in budgets {
        let src = features_dir.join(file);
        let stem = Path::new(file).file_stem().unwrap().to_string_lossy();
        let bin = build_dir.join("build").join(stem.as_ref());

        let out = Command::new(&jet)
            .args(["build", "--small", src.to_str().unwrap()])
            .current_dir(&build_dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "GA size gate: `--small` build of {} failed:\n{}",
            file,
            String::from_utf8_lossy(&out.stderr)
        );

        let size = fs::metadata(&bin).map(|m| m.len()).unwrap_or(0);
        assert!(
            size <= *max_bytes && size > 0,
            "GA size gate: {} --small binary is {} bytes (limit {})",
            file,
            size,
            max_bytes
        );
    }

    let _ = fs::remove_dir_all(&build_dir);
}
