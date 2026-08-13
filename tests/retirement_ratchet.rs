//! D-ONCE-RETIRE1=C: every retirement carries an adoption ratchet that ends at
//! zero.
//!
//! `Syntax::RETIREMENTS` says what was retired. This test says how much of the
//! repository is still written the old way, and holds that number down. It
//! counts files per canonical form, prints the adoption ratio for each row, and
//! fails when a count moves away from its recorded ceiling in either direction:
//!
//! * a count **above** the ceiling means a new file was written in a retired
//!   form, so the retirement went backwards;
//! * a count **below** the ceiling means a migration landed without lowering
//!   the ceiling, so the ratchet stopped ratcheting.
//!
//! A row is finished when its ceiling is `0`, and a ceiling of `0` then holds
//! the retired form out of the repository for good.
//!
//! Diagnostic fixtures under `tests/ui` and `tests/fuzz` are not counted for
//! the content rows: a fixture must keep the retired form to prove the error
//! that refuses it.

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use jet::Syntax::{
    law_violations, rename_target, Retirement, RetirementKind, REF_PROVIDERS,
    RETIREMENTS,
};

/// Files still written in the retired form, per row, as counted today. Lower a
/// number when a migration lands; never raise one.
const CEILINGS: &[(&str, usize)] = &[
    ("entry-file", 94),
    // The two corelib archives and the seven out-of-scope engine fixtures
    // remain until their owning migration slices land.
    ("manifest-file", 2),
    ("manifest-identity", 7),
    ("lint-policy-code", 0),
    ("package-ref-order", 0),
    ("interpolation-selector-rail", 0),
    ("core-io-println", 0),
    ("core-io-sprint", 0),
    ("core-io-repr", 0),
    ("comptime-mark", 0),
    ("set-take", 0),
    ("map-replace", 0),
    ("set-replace", 0),
    ("allow-impure", 0),
    ("core-path-free-functions", 0),
    ("target-plugin", 0),
    ("core-container-queue", 0),
    ("core-container-rank", 0),
    ("core-container-tally", 0),
    ("core-container-bits", 0),
    ("core-container-bytes", 0),
    ("jet-time-now", 0),
    ("jet-time-format", 0),
];

const CONTENT_ROOTS: &[&str] = &["crates", "examples", "tests", "Source"];

/// Repository content never lives in a dot directory, a build directory, or a
/// vendored package tree. Skipping all three keeps the count stable whatever a
/// working tree happens to hold: worktrees, scratch, caches and build output
/// are not the repository.
fn is_skipped_dir(name: &str) -> bool {
    name.starts_with('.') || name.starts_with("target") || name == "node_modules"
}

fn walk(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if !is_skipped_dir(&name) {
                walk(&path, out);
            }
        } else {
            out.push(path);
        }
    }
}

/// Every file in the repository, for the rows that count file names.
fn all_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(Path::new("."), &mut out);
    out
}

/// Source and corpus files, for the rows that count what a file says. Fixture
/// trees are left out; they hold retired forms on purpose.
fn content_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in CONTENT_ROOTS {
        walk(Path::new(root), &mut out);
    }
    for entry in fs::read_dir(".").into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "jet") {
            out.push(path);
        }
    }
    out.retain(|path| {
        let text = format!("/{}", path.to_string_lossy().replace('\\', "/"));
        !text.contains("/tests/ui/") && !text.contains("/tests/fuzz/")
    });
    out
}

fn file_name(path: &Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().to_string()
}

fn read(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_string_lossy().to_string();
    (ext == "rs" || ext == "jet").then(|| fs::read_to_string(path).ok())?
}

/// A manifest written with the retired `payload: { … }` identity wrapper,
/// either as a file or as manifest text a test writes out. The opener must sit
/// at the start of a line or right after a quote, so a `payload:` field on a
/// user's own type is not counted.
fn writes_retired_identity(text: &str) -> bool {
    let bytes = text.as_bytes();
    for (index, _) in text.match_indices("payload:") {
        let opens_here = index == 0
            || matches!(bytes[index - 1], b'\n' | b'"')
            || (bytes[index - 1] == b'\\' && index >= 2 && bytes[index - 2] == b'"');
        if !opens_here {
            continue;
        }
        let rest = text[index + "payload:".len()..].trim_start();
        if rest.starts_with('{') && rest.contains("name:") {
            return true;
        }
    }
    false
}

/// The words either side of each `@` in a package ref, in source order.
fn ref_sides(text: &str) -> Vec<(String, String)> {
    let bytes = text.as_bytes();
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'-';
    let mut out = Vec::new();
    for (at, _) in text.match_indices('@') {
        let mut start = at;
        while start > 0 && word(bytes[start - 1]) {
            start -= 1;
        }
        let mut end = at + 1;
        while end < bytes.len() && word(bytes[end]) {
            end += 1;
        }
        out.push((text[start..at].to_string(), text[at + 1..end].to_string()));
    }
    out
}

/// A package ref written provider first, the order D-JPK-REF1 retired. A
/// provider name on the left is only the retired order when the right side is
/// not itself a provider: `perl@nixpkgs` is the package `perl` from `nixpkgs`,
/// written the canonical way round.
fn writes_provider_first(text: &str) -> bool {
    ref_sides(text).iter().any(|(left, right)| {
        REF_PROVIDERS.contains(&left.as_str()) && !REF_PROVIDERS.contains(&right.as_str())
    })
}

fn writes_canonical_ref(text: &str) -> bool {
    ref_sides(text)
        .iter()
        .any(|(_, right)| REF_PROVIDERS.contains(&right.as_str()))
}

/// Whether a line writes a package lint policy value, and whether its first
/// value is a retired diagnostic code. Test fixtures under `tests/ui` are
/// excluded by `content_files`; this only counts repository content that can
/// become a live package/config source.
fn lint_policy_value_is_code(line: &str) -> Option<bool> {
    let lints = line.find("lints")?;
    let deny = line[lints..].find("deny")? + lints;
    let open = line[deny..].find('[')? + deny;
    let value = line[open + 1..].trim_start();
    let token = value
        .split(|character: char| character == ',' || character == ']' || character.is_whitespace())
        .next()
        .unwrap_or("")
        .trim_matches('"');
    if token.is_empty() {
        return None;
    }
    let mut chars = token.chars();
    let code_shape = matches!(chars.next(), Some('E' | 'L' | 'W'))
        && chars.count() == 4
        && token[1..].chars().all(|character| character.is_ascii_digit());
    Some(code_shape)
}

fn lint_policy_values(text: &str) -> (bool, bool) {
    let mut retired = false;
    let mut canonical = false;
    for line in text.lines() {
        match lint_policy_value_is_code(line) {
            Some(true) => retired = true,
            Some(false) => canonical = true,
            None => {}
        }
    }
    (retired, canonical)
}

fn writes_interpolation_selector(text: &str, retired: bool) -> bool {
    let (tokens, lex_diags) = jet::Lexer::lex(text);
    if !lex_diags.is_empty() {
        return false;
    }
    fn scan(tokens: &[jet::Lexer::Token], retired: bool) -> bool {
        for token in tokens {
            let jet::Lexer::TokKind::Str(parts) = &token.kind else {
                continue;
            };
            for part in parts {
                let jet::Lexer::StrTokPart::Interp(inner) = part else {
                    continue;
                };
                for pair in inner.windows(2) {
                    let rail = if retired {
                        matches!(pair[0].kind, jet::Lexer::TokKind::Hash)
                    } else {
                        matches!(pair[0].kind, jet::Lexer::TokKind::Colon)
                    };
                    let selector = matches!(
                        &pair[1].kind,
                        jet::Lexer::TokKind::Ident(name)
                            if jet::Syntax::interpolation_selector(name).is_some()
                    );
                    if rail && selector {
                        return true;
                    }
                }
                if scan(inner, retired) {
                    return true;
                }
            }
        }
        false
    }
    scan(&tokens, retired)
}

fn has_retired_comptime_mark(tokens: &[jet::Lexer::Token]) -> bool {
    for token in tokens {
        if matches!(&token.kind, jet::Lexer::TokKind::Dollar) {
            return true;
        }
        let jet::Lexer::TokKind::Str(parts) = &token.kind else {
            continue;
        };
        for part in parts {
            let jet::Lexer::StrTokPart::Interp(inner) = part else {
                continue;
            };
            if has_retired_comptime_mark(inner) {
                return true;
            }
        }
    }
    false
}

fn tally_collection_example(path_suffix: &str, retired_form: &str, canonical_form: &str) -> (usize, usize) {
    content_files()
        .into_iter()
        .filter(|path| path.to_string_lossy().ends_with(path_suffix))
        .filter_map(|path| read(&path))
        .fold((0, 0), |(retired, canonical), text| {
            (
                retired + usize::from(text.contains(retired_form)),
                canonical + usize::from(text.contains(canonical_form)),
            )
        })
}

fn tally_print_family(retired_form: &str, canonical_form: &str) -> (usize, usize) {
    content_files()
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "jet"))
        .filter_map(|path| read(&path))
        .fold((0, 0), |(retired, canonical), text| {
            (
                retired + usize::from(text.contains(retired_form)),
                canonical + usize::from(text.contains(canonical_form)),
            )
        })
}

/// Files on the retired form and files on the canonical form, for one row.
fn tally(row: &Retirement) -> (usize, usize) {
    match row.id {
        "entry-file" | "manifest-file" => {
            let files = all_files();
            let count = |name: &str| files.iter().filter(|p| file_name(p) == name).count();
            (count(row.retired), count(row.canonical))
        }
        "manifest-identity" => {
            let mut retired = 0;
            let mut canonical = 0;
            for path in content_files() {
                let Some(text) = read(&path) else { continue };
                if writes_retired_identity(&text) {
                    retired += 1;
                } else if file_name(&path).ends_with(".jet")
                    && text.lines().any(|line| line.starts_with("name:"))
                {
                    canonical += 1;
                }
            }
            (retired, canonical)
        }
        "lint-policy-code" => {
            let mut retired = 0;
            let mut canonical = 0;
            for path in content_files() {
                let Some(text) = read(&path) else { continue };
                let (has_retired, has_canonical) = lint_policy_values(&text);
                if has_retired {
                    retired += 1;
                } else if has_canonical {
                    canonical += 1;
                }
            }
            (retired, canonical)
        }
        "package-ref-order" => {
            // Jet files only. A Rust source that quotes `github@owner/repo` is
            // the E1317 test proving the order is refused, not a file written
            // in the retired order.
            let mut retired = 0;
            let mut canonical = 0;
            for path in content_files() {
                if path.extension().is_none_or(|ext| ext != "jet") {
                    continue;
                }
                // The generated diagnostic catalog quotes the retired form in
                // E1317's teaching text. It is evidence about the retirement,
                // not package source written in the retired order.
                if path.ends_with("crates/jet-codegen/src/Prelude/Diagnostics.jet") {
                    continue;
                }
                let Some(text) = read(&path) else { continue };
                if writes_provider_first(&text) {
                    retired += 1;
                } else if writes_canonical_ref(&text) {
                    canonical += 1;
                }
            }
            (retired, canonical)
        }
        "allow-impure" => {
            let mut retired = 0;
            let mut canonical = 0;
            for path in content_files() {
                if path.ends_with("crates/jet-foundation/src/Syntax/retirements.rs") {
                    continue;
                }
                let Some(text) = read(&path) else { continue };
                if text.contains(row.retired) {
                    retired += 1;
                } else if text.contains(row.canonical) {
                    canonical += 1;
                }
            }
            (retired, canonical)
        }
        "interpolation-selector-rail" => {
            let mut retired = 0;
            let mut canonical = 0;
            for path in content_files() {
                if path.extension().is_none_or(|ext| ext != "jet") {
                    continue;
                }
                let Some(text) = read(&path) else { continue };
                let has_retired = writes_interpolation_selector(&text, true);
                if has_retired {
                    retired += 1;
                } else if writes_interpolation_selector(&text, false) {
                    canonical += 1;
                }
            }
            (retired, canonical)
        }
        "comptime-mark" => {
            let mut retired = 0;
            let mut canonical = 0;
            for path in content_files() {
                if path.extension().is_none_or(|ext| ext != "jet") {
                    continue;
                }
                let Some(text) = read(&path) else { continue };
                let (tokens, lex_diags) = jet::Lexer::lex(&text);
                if !lex_diags.is_empty() {
                    continue;
                }
                let old = has_retired_comptime_mark(&tokens);
                let current = tokens.iter().any(|token| {
                    matches!(&token.kind, jet::Lexer::TokKind::Ident(name) if name.starts_with(jet::Syntax::COMPTIME_MARK))
                });
                if old {
                    retired += 1;
                } else if current {
                    canonical += 1;
                }
            }
            (retired, canonical)
        }
        "set-take" => tally_collection_example(
            "examples/features/collections/set.jet",
            ".take(",
            ".pop(",
        ),
        "map-replace" => tally_collection_example(
            "examples/features/collections/map_surface.jet",
            ".replace(",
            ".add(",
        ),
        "set-replace" => tally_collection_example(
            "examples/features/collections/set.jet",
            ".replace(",
            ".add(",
        ),
        "core-io-println" => tally_print_family("io.println", "io.print"),
        "core-io-sprint" => tally_print_family("io.sprint", "{value}"),
        "core-io-repr" => tally_print_family("io.repr", "{value:Debug}"),
        "core-path-free-functions" => {
            let mut retired = 0;
            let mut canonical = 0;
            for path in content_files() {
                if path.extension().is_none_or(|ext| ext != "jet") {
                    continue;
                }
                let Some(text) = read(&path) else { continue };
                if text.contains("use core.path") || text.contains("core.path.") {
                    retired += 1;
                } else if text.contains("Path.from") || text.contains("Path.home") {
                    canonical += 1;
                }
            }
            (retired, canonical)
        }
        "target-plugin" => {
            let mut retired = 0;
            let mut canonical = 0;
            for path in content_files() {
                if path.extension().is_none_or(|ext| ext != "jet") {
                    continue;
                }
                let Some(text) = read(&path) else { continue };
                let (_, retired_targets) = jet::Package::rewrite_retired_targets(&text);
                if retired_targets > 0 || text.contains("target: plugin") {
                    retired += 1;
                } else if text.contains("target: sandbox")
                    || jet::Package::PackageFacts::parse(&text, &path.display().to_string())
                        .is_ok_and(|facts| {
                            facts.packages.iter().any(|package| {
                                package.targets.iter().any(|target| {
                                    matches!(target, jet::Package::Target::Plugin { .. })
                                })
                            })
                        })
                {
                    canonical += 1;
                }
            }
            (retired, canonical)
        }
        "core-container-queue"
        | "core-container-rank"
        | "core-container-tally"
        | "core-container-bits"
        | "core-container-bytes" => {
            let mut retired = 0;
            let mut canonical = 0;
            for path in content_files() {
                let Some(text) = read(&path) else { continue };
                if contains_word(&text, row.retired) {
                    retired += 1;
                } else if contains_word(&text, row.canonical) {
                    canonical += 1;
                }
            }
            (retired, canonical)
        }
        "jet-time-now" | "jet-time-format" => {
            let files = content_files();
            let count = |needle: &str| {
                files
                    .iter()
                    .filter_map(|path| read(path))
                    .filter(|text| text.contains(needle))
                    .count()
            };
            (count(row.retired), count(row.canonical))
        }
        other => panic!("no way to count row `{other}`; teach `tally` how to count it"),
    }
}

#[test]
fn the_retirement_table_obeys_its_own_law() {
    assert!(law_violations().is_empty(), "{:#?}", law_violations());
}

#[test]
fn every_retirement_carries_a_ratchet() {
    let ceilings: BTreeMap<&str, usize> = CEILINGS.iter().copied().collect();
    assert_eq!(
        ceilings.len(),
        CEILINGS.len(),
        "two ceilings claim the same retirement id"
    );
    let rows: BTreeMap<&str, &Retirement> = RETIREMENTS.iter().map(|row| (row.id, row)).collect();
    let missing: Vec<&str> = rows
        .keys()
        .filter(|id| !ceilings.contains_key(*id))
        .copied()
        .collect();
    assert!(
        missing.is_empty(),
        "these retirements ship no adoption ratchet: {missing:?}"
    );
    let orphaned: Vec<&str> = ceilings
        .keys()
        .filter(|id| !rows.contains_key(*id))
        .copied()
        .collect();
    assert!(
        orphaned.is_empty(),
        "these ratchets name no retirement: {orphaned:?}"
    );
}

#[test]
fn adoption_ratchets_toward_zero() {
    let ceilings: BTreeMap<&str, usize> = CEILINGS.iter().copied().collect();
    let mut report = String::new();
    let mut failures = Vec::new();
    for row in RETIREMENTS {
        let ceiling = ceilings[row.id];
        let (retired, canonical) = tally(row);
        let total = retired + canonical;
        let adoption = if total == 0 { 100.0 } else { canonical as f64 * 100.0 / total as f64 };
        let answer = match row.kind {
            RetirementKind::Rename => "fmt/fix rewrites",
            RetirementKind::Semantic => "refused",
        };
        report.push_str(&format!(
            "  {:<18} {:>5.1}% on `{}`  ({canonical} canonical, {retired} retired, ceiling {ceiling}, {answer}, {})\n",
            row.id, adoption, row.canonical, row.decision
        ));
        if retired > ceiling {
            failures.push(format!(
                "{}: `{}` is written in {retired} files, above the ceiling of {ceiling}. \
                 A retired form may not gain new files.",
                row.id, row.retired
            ));
        } else if retired < ceiling {
            failures.push(format!(
                "{}: `{}` is down to {retired} files. Lower the ceiling from {ceiling} to \
                 {retired} in tests/retirement_ratchet.rs so the ratchet holds the gain.",
                row.id, row.retired
            ));
        }
    }
    println!("adoption per canonical form:\n{report}");
    assert!(failures.is_empty(), "{}\n{report}", failures.join("\n"));
}

#[test]
fn a_finished_retirement_stays_finished() {
    let ceilings: BTreeMap<&str, usize> = CEILINGS.iter().copied().collect();
    for row in RETIREMENTS {
        if ceilings[row.id] == 0 {
            let (retired, _) = tally(row);
            assert_eq!(retired, 0, "`{}` came back in {retired} files", row.retired);
        }
    }
}

#[test]
fn retired_time_doors_map_to_the_one_clock_door() {
    assert_eq!(
        rename_target(concat!("jet", ".time", ".now")),
        Some("core.time.now")
    );
    assert_eq!(
        rename_target(concat!("jet", ".time", ".format")),
        Some("DateTime.format_rfc3339()")
    );
}

/// D-ONCE: `Syntax::REF_SOURCE_PROVIDERS` (`Syntax/effects_surface.rs`) is the
/// one home for "which source tokens are built-in providers" — this ratchet's
/// `REF_PROVIDERS` and `jet-pkg-model`'s `RefSpec::Source::is_builtin` both
/// read it. Fails if `is_builtin` goes back to hand-copying the `REF_SOURCE_*`
/// constants into a second list instead of calling the shared one.
#[test]
fn ref_provider_set_has_one_definition_site() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ref_spec = fs::read_to_string(root.join("crates/jet-pkg-model/src/RefSpec.rs"))
        .expect("RefSpec.rs is readable");
    let is_builtin_body = ref_spec
        .split("pub fn is_builtin(name: &str) -> bool {")
        .nth(1)
        .and_then(|tail| tail.split_once('}').map(|(body, _)| body))
        .expect("RefSpec::Source::is_builtin body");
    assert!(
        !is_builtin_body.contains("REF_SOURCE_") || is_builtin_body.contains("REF_SOURCE_PROVIDERS"),
        "RefSpec::Source::is_builtin must read Syntax::REF_SOURCE_PROVIDERS, not hand-copy \
         individual REF_SOURCE_* constants into a second list:\n{is_builtin_body}"
    );
    assert!(
        is_builtin_body.contains("REF_SOURCE_PROVIDERS"),
        "RefSpec::Source::is_builtin must call Syntax::REF_SOURCE_PROVIDERS.contains(..):\n{is_builtin_body}"
    );
}
