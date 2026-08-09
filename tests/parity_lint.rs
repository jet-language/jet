//! D-ONCE-LAW1 / D-ONCE-TIER1: every tier-parity promise has a mechanical grade.
//!
//! A promise comment is useful inventory, but it is not enforcement. This
//! guard accepts exactly one of the three mechanical links from the card:
//! `include`, `marshal`, or `guard`. A documented `gap` is accepted outside
//! the JIT and TIR-eval engines when it names the card that owns the risk.
//!
//! The scanner deliberately limits `byte-for-byte` to cross-tier promises.
//! Other uses in this corpus describe syntax, formatting, cache keys, or
//! emitted-shape tests; those are not AOT/JIT/interpreter synchronization
//! claims. Run: `scripts/agent/jet-env cargo test --test parity_lint`.

use std::fs;
use std::path::{Path, PathBuf};

const SCANNED_ROOTS: &[&str] = &["crates", "Source", "tests"];
const THIS_TEST: &str = "tests/parity_lint.rs";
const CROSS_TIER_WORDS: &[&str] = &[
    "aot",
    "comptime",
    "interpreter",
    "jit",
    "tier",
    "r12",
    "decode path",
    "ffi bridge",
    "wire format",
    "jet_std",
];

#[derive(Debug)]
struct Site {
    path: String,
    line: usize,
    grade: String,
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn collect_source_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_source_files(&path, out);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "rs" || extension == "jet")
        {
            out.push(path);
        }
    }
}

fn source_files(repo_root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for directory in SCANNED_ROOTS {
        collect_source_files(&repo_root.join(directory), &mut files);
    }
    files.sort();
    files
}

fn relative_path(repo_root: &Path, path: &Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn line_comment_start(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut in_string = false;
    let mut index = 0;
    while index + 1 < bytes.len() {
        match bytes[index] {
            b'"' => in_string = !in_string,
            b'\\' if in_string => {
                index += 2;
                continue;
            }
            b'/' if !in_string && bytes[index + 1] == b'/' => return Some(index),
            _ => {}
        }
        index += 1;
    }
    None
}

fn comment_blocks(source: &str) -> Vec<(usize, String)> {
    let mut blocks = Vec::new();
    let mut current_start = None;
    let mut current = String::new();

    let mut finish = |blocks: &mut Vec<(usize, String)>, start: &mut Option<usize>, text: &mut String| {
        if let Some(line) = start.take() {
            blocks.push((line, std::mem::take(text)));
        }
    };

    for (index, line) in source.lines().enumerate() {
        let Some(comment_start) = line_comment_start(line) else {
            finish(&mut blocks, &mut current_start, &mut current);
            continue;
        };
        if current_start.is_none() {
            current_start = Some(index + 1);
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line[comment_start + 2..].trim());
    }
    finish(&mut blocks, &mut current_start, &mut current);
    blocks
}

fn is_parity_promise(comment: &str) -> bool {
    let lower = comment.to_ascii_lowercase();
    if lower.contains("matches aot") || lower.contains("mirrors aot") {
        return true;
    }
    lower.contains("byte-for-byte")
        && CROSS_TIER_WORDS
            .iter()
            .any(|word| lower.contains(word))
}

fn parity_annotations(comment: &str) -> Vec<String> {
    comment
        .lines()
        .filter_map(|line| {
            let lower = line.to_ascii_lowercase();
            let start = lower.find("parity:")? + "parity:".len();
            Some(line[start..].trim().to_string())
        })
        .collect()
}

fn code_without_line_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| {
            line_comment_start(line)
                .map_or(line, |comment_start| &line[..comment_start])
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn include_mentions(source: &str, target: &str) -> bool {
    let suffix = target.strip_prefix("crates/").unwrap_or(target);
    let code = code_without_line_comments(source);
    (code.contains(target) || code.contains(suffix))
        && (code.contains("include!(") || code.contains("include_str!("))
}

fn is_engine_gap(path: &str) -> bool {
    path.contains("crates/jet-jit/")
        || path.contains("crates/jet-codegen/src/Codegen/TIR/eval/")
}

fn valid_card_number(token: &str) -> bool {
    let Some(number) = token.strip_prefix("card=#") else {
        return false;
    };
    !number.is_empty() && number.chars().all(|character| character.is_ascii_digit())
}

fn validate_grade(
    repo_root: &Path,
    path: &str,
    source: &str,
    comment: &str,
) -> Result<String, String> {
    let annotations = parity_annotations(comment);
    if annotations.len() != 1 {
        return Err(format!(
            "expected exactly one `parity:` grade, found {}",
            annotations.len()
        ));
    }
    let annotation = annotations[0].trim();

    if let Some(target) = annotation.strip_prefix("include path=") {
        let target = target.trim();
        if target.is_empty() {
            return Err("the include grade has no canonical source path".to_string());
        }
        if !repo_root.join(target).is_file() {
            return Err(format!("the include target does not exist: {target}"));
        }
        if !include_mentions(source, target) {
            return Err(format!(
                "the source does not include the canonical source named by the grade: {target}"
            ));
        }
        return Ok(format!("include {target}"));
    }

    if let Some(symbol) = annotation.strip_prefix("marshal symbol=") {
        let symbol = symbol.trim();
        if symbol.is_empty() || symbol.chars().any(char::is_whitespace) {
            return Err("the marshal grade must name one symbol".to_string());
        }
        if !code_without_line_comments(source).contains(symbol) {
            return Err(format!("the shared marshalling symbol is not called: {symbol}"));
        }
        return Ok(format!("marshal {symbol}"));
    }

    if let Some(target) = annotation.strip_prefix("guard ") {
        let Some((test_path, function)) = target.trim().split_once("::") else {
            return Err("the guard grade must be `guard tests/file.rs::function`".to_string());
        };
        if !test_path.starts_with("tests/") {
            return Err("the guard grade must point to a test file".to_string());
        }
        let function = function.trim();
        if function.is_empty() {
            return Err("the guard grade names no test function".to_string());
        }
        let guard_path = repo_root.join(test_path);
        let guard_source = fs::read_to_string(&guard_path)
            .map_err(|error| format!("cannot read guard {test_path}: {error}"))?;
        if !guard_source.contains(&format!("fn {function}")) {
            return Err(format!("guard function does not exist: {test_path}::{function}"));
        }
        if !guard_source.contains("assert") {
            return Err(format!("guard has no behavior assertion: {test_path}::{function}"));
        }
        return Ok(format!("guard {test_path}::{function}"));
    }

    if let Some(rest) = annotation.strip_prefix("gap ") {
        let mut fields = rest.splitn(2, " risk=");
        let card = fields.next().unwrap_or_default().trim();
        let risk = fields.next().unwrap_or_default().trim();
        if !valid_card_number(card) || risk.is_empty() {
            return Err("the gap grade must be `gap card=#N risk=<reason>`".to_string());
        }
        if is_engine_gap(path) {
            return Err(
                "jet-jit and TIR eval parity promises require include, marshal, or guard; gaps are not allowed"
                    .to_string(),
            );
        }
        return Ok(format!("gap {card}"));
    }

    Err(format!("unknown parity grade: {annotation}"))
}

fn scan(repo_root: &Path) -> (Vec<Site>, Vec<String>) {
    let mut sites = Vec::new();
    let mut errors = Vec::new();
    for path in source_files(repo_root) {
        let relative = relative_path(repo_root, &path);
        if relative == THIS_TEST {
            continue;
        }
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        for (line, comment) in comment_blocks(&source) {
            if !is_parity_promise(&comment) {
                continue;
            }
            match validate_grade(repo_root, &relative, &source, &comment) {
                Ok(grade) => sites.push(Site {
                    path: relative.clone(),
                    line,
                    grade,
                }),
                Err(error) => errors.push(format!("{relative}:{line}: {error}")),
            }
        }
    }
    (sites, errors)
}

#[test]
fn every_tier_parity_promise_has_one_mechanical_grade() {
    let (mut sites, errors) = scan(&root());
    sites.sort_by(|left, right| (&left.path, left.line).cmp(&(&right.path, right.line)));
    assert!(errors.is_empty(), "ungraded parity promises:\n  {}", errors.join("\n  "));
    assert!(!sites.is_empty(), "the parity inventory found no promise comments");
    for site in sites {
        println!("{}:{} — {}", site.path, site.line, site.grade);
    }
}

#[test]
fn new_unbacked_duplication_comment_is_rejected() {
    let source = "// hand-copy matches AOT\nfn copied() {}\n";
    let (line, comment) = comment_blocks(source)
        .into_iter()
        .find(|(_, comment)| is_parity_promise(comment))
        .expect("seeded promise comment");
    let error = validate_grade(&root(), "crates/jet-jit/src/new_copy.rs", source, &comment)
        .expect_err("a comment without a mechanical link must fail");
    assert!(
        error.contains("parity:` grade") || error.contains("parity: grade"),
        "line {line}: unexpected lint error: {error}"
    );
}

#[test]
fn include_grade_must_name_real_include_code() {
    let source = "// matches AOT\n// parity: include path=crates/jet-codegen/src/Prelude/Core/EncodingBase.rs\n";
    let (_, comment) = comment_blocks(source)
        .into_iter()
        .find(|(_, comment)| is_parity_promise(comment))
        .expect("seeded promise comment");
    let error = validate_grade(&root(), "crates/jet-jit/src/new_copy.rs", source, &comment)
        .expect_err("an include path written only in a comment must fail");
    assert!(error.contains("does not include the canonical source"), "{error}");
}

#[test]
fn jet_jit_and_tir_eval_cannot_card_a_parity_gap() {
    for path in [
        "crates/jet-jit/src/new_copy.rs",
        "crates/jet-codegen/src/Codegen/TIR/eval/new_copy.rs",
    ] {
        let source = "// matches AOT\n// parity: gap card=#1711 risk=hand-copy\n";
        let (_, comment) = comment_blocks(source)
            .into_iter()
            .find(|(_, comment)| is_parity_promise(comment))
            .expect("seeded promise comment");
        let error = validate_grade(&root(), path, source, &comment)
            .expect_err("engine parity gaps must have a mechanical link");
        assert!(error.contains("require include, marshal, or guard"), "{path}: {error}");
    }
}
