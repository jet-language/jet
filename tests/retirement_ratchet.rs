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
    law_violations, rename_target, Retirement, RetirementKind, JETPACK_TOML, REF_PROVIDERS,
    RETIREMENTS,
};

/// Files still written in the retired form, per row, as counted today. Lower a
/// number when a migration lands; never raise one.
const CEILINGS: &[(&str, usize)] = &[
    ("effect-flat-ffi-go", 0),
    ("effect-flat-ffi-java", 0),
    ("effect-flat-ffi-dotnet", 0),
    ("effect-flat-ffi-fortran", 0),
    ("effect-flat-ffi-cobol", 0),
    ("effect-flat-ffi-tcl", 0),
    ("effect-flat-ffi-lua", 0),
    ("effect-flat-ffi-ada", 0),
    ("effect-flat-ffi-pascal", 0),
    ("effect-flat-ffi-dart", 0),
    ("effect-flat-ffi-powershell", 0),
    ("effect-flat-ffi-perl", 0),
    ("effect-flat-ffi-ruby", 0),
    ("effect-flat-ffi-php", 0),
    ("effect-flat-ffi-r", 0),
    ("effect-flat-ffi-com", 0),
    ("effect-flat-ffi-cpp", 0),
    ("effect-flat-ffi-py", 0),
    ("effect-flat-ffi-octave", 0),
    ("entry-file", 0),
    // The two corelib archives and the seven out-of-scope engine fixtures
    // remain until their owning migration slices land.
    ("manifest-file", 2),
    ("jetpack-file", 0),
    ("manifest-identity", 7),
    ("lint-policy-code", 0),
    ("auto-derive-policy", 0),
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
    ("scope-marker-grant", 0),
    ("core-path-free-functions", 0),
    ("core-namespace-io", 0),
    ("core-namespace-path", 0),
    ("core-namespace-time-date", 0),
    ("core-namespace-time-datetime", 0),
    ("core-namespace-text-unicode", 0),
    ("core-namespace-fmt", 0),
    ("core-namespace-random", 0),
    ("core-namespace-env", 0),
    ("core-namespace-os", 0),
    ("core-namespace-tls", 0),
    ("core-namespace-ws", 0),
    ("core-namespace-url", 0),
    ("core-namespace-mime", 0),
    ("core-namespace-uuid", 0),
    ("core-namespace-vault", 0),
    ("core-namespace-raylib", 0),
    ("core-namespace-browser", 0),
    ("core-namespace-solve", 0),
    ("core-namespace-sketch", 0),
    ("core-namespace-sketch-hll", 0),
    ("core-namespace-sketch-tdigest", 0),
    ("core-namespace-sketch-reservoir", 0),
    ("core-namespace-sketch-cms", 0),
    ("core-namespace-compress", 0),
    ("core-namespace-compress-gzip", 0),
    ("core-namespace-compress-zstd", 0),
    ("core-namespace-measurement", 0),
    ("core-namespace-mem-alloc", 0),
    ("core-namespace-scope", 0),
    ("core-namespace-lang", 0),
    ("core-namespace-binary", 0),
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
fn is_skipped_dir(path: &Path) -> bool {
    path.components().any(|component| {
        let std::path::Component::Normal(name) = component else {
            return false;
        };
        let name = name.to_string_lossy();
        name.starts_with('.')
            || name.starts_with("target")
            || name == "build"
            || name == "node_modules"
    })
}

fn walk(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        // Ratchets count repository content, not external trees linked into it.
        if file_type.is_symlink() {
            continue;
        }
        if path.is_dir() {
            let relative = path
                .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                .unwrap_or(&path);
            if !is_skipped_dir(relative) {
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
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    walk(root, &mut out);
    out
}

/// Source and corpus files, for the rows that count what a file says. Fixture
/// trees are left out; they hold retired forms on purpose.
fn content_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for content_root in CONTENT_ROOTS {
        walk(&repo_root.join(content_root), &mut out);
    }
    for entry in fs::read_dir(repo_root).into_iter().flatten().flatten() {
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

const FAILURE_SURFACE_ROOTS: &[&str] = &[
    "crates",
    "examples",
    "tests",
    "Source",
    "corelib",
    "docs",
    "editors",
    "tools",
];

const FAILURE_SURFACE_EXTENSIONS: &[&str] = &[
    "jet", "md", "rs", "js", "json", "scm", "toml", "txt", "yaml", "yml",
];

/// D-FAILURE-FOUNDATION1=A: retired failure syntax has no active-source
/// allowance. Diagnostic fixtures and decision history are skipped by the
/// explicit allowlist below; every other detected spelling must be zero.
const FAILURE_RETIREMENT_CEILING: usize = 0;

/// The failure retirement covers source-shaped documentation and generated
/// `.jet` trees too. Unlike the older adoption rows, this walk keeps hidden
/// `.jet` directories and skips only external/build state.
fn failure_surface_skip_dir(path: &Path) -> bool {
    path.components().any(|component| {
        let std::path::Component::Normal(name) = component else {
            return false;
        };
        let name = name.to_string_lossy();
        name == ".git"
            || name == ".claude"
            || name == ".agent-worktrees"
            || name == ".opencode"
            || name == "node_modules"
            || name == "build"
            || name.starts_with("target")
    })
}

fn walk_failure_surface(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        if path.is_dir() {
            let relative = path
                .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                .unwrap_or(&path);
            if !failure_surface_skip_dir(relative) {
                walk_failure_surface(&path, out);
            }
        } else if path.extension().is_some_and(|extension| {
            FAILURE_SURFACE_EXTENSIONS
                .iter()
                .any(|allowed| extension == *allowed)
        }) {
            out.push(path);
        }
    }
}

fn failure_surface_files() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    for relative in FAILURE_SURFACE_ROOTS {
        walk_failure_surface(&root.join(relative), &mut out);
    }
    if let Ok(entries) = fs::read_dir(root) {
        out.extend(entries.flatten().filter_map(|entry| {
            let path = entry.path();
            (path.is_file()
                && (path.extension().is_some_and(|extension| extension == "jet")
                    || file_name(&path) == "llms.text"))
            .then_some(path)
        }));
    }
    let readme = root.join("README.md");
    if readme.is_file() {
        out.push(readme);
    }
    out
}

/// These are the only places where old failure spellings remain useful as
/// evidence: retirement history, refusal/teaching fixtures, and the catalog
/// that emits those diagnostics. Live `.jet` source and current documentation
/// stay outside this list.
fn failure_surface_allowlisted(path: &str) -> bool {
    is_authority_history(path)
        || is_authority_diagnostic_fixture(path)
        || path == "crates/jet-codegen/src/Prelude/Diagnostics.jet"
        // These two files contain source snippets whose only purpose is to
        // exercise retired-form recovery diagnostics and formatter fixes.
        || path == "crates/jet-parser/src/Parser/mod.rs"
        || path == "tests/fmt.rs"
        // This file also constructs retired forms as detector test inputs.
        || path == "tests/retirement_ratchet.rs"
}

struct FailureSourceFragment {
    first_line: usize,
    source: String,
}

fn failure_fence(line: &str) -> Option<(u8, usize, &str)> {
    let trimmed = line.trim_start();
    let marker = *trimmed.as_bytes().first()?;
    if marker != b'`' && marker != b'~' {
        return None;
    }
    let length = trimmed.bytes().take_while(|byte| *byte == marker).count();
    (length >= 3).then_some((marker, length, &trimmed[length..]))
}

fn failure_inline_fragments(line: &str) -> Vec<String> {
    let bytes = line.as_bytes();
    let mut fragments = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'`' {
            cursor += 1;
            continue;
        }
        let length = bytes[cursor..]
            .iter()
            .take_while(|byte| **byte == b'`')
            .count();
        let content_start = cursor + length;
        let mut close = content_start;
        while close < bytes.len() {
            if bytes[close] != b'`' {
                close += 1;
                continue;
            }
            let close_length = bytes[close..]
                .iter()
                .take_while(|byte| **byte == b'`')
                .count();
            if close_length == length {
                fragments.push(line[content_start..close].to_string());
                cursor = close + close_length;
                break;
            }
            close += close_length;
        }
        if close >= bytes.len() {
            break;
        }
    }
    fragments
}

fn failure_looks_like_jet(source: &str) -> bool {
    [
        "fn ",
        "alias ",
        "struct ",
        "pub fn ",
        "->",
        "-[",
        "fallible",
        "failure",
        "optional",
    ]
    .iter()
    .any(|marker| source.contains(marker))
}

fn failure_embedded_fragments(text: &str) -> Vec<FailureSourceFragment> {
    let bytes = text.as_bytes();
    let mut fragments = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == b'r' {
            let mut quote = cursor + 1;
            while quote < bytes.len() && bytes[quote] == b'#' {
                quote += 1;
            }
            if quote < bytes.len() && bytes[quote] == b'"' {
                let hashes = &text[cursor + 1..quote];
                let close = format!("\"{hashes}");
                let content_start = quote + 1;
                let Some(close_offset) = text[content_start..].find(&close) else {
                    cursor += 1;
                    continue;
                };
                let content_end = content_start + close_offset;
                let source = &text[content_start..content_end];
                if failure_looks_like_jet(source) {
                    fragments.push(FailureSourceFragment {
                        first_line: text[..cursor]
                            .bytes()
                            .filter(|byte| *byte == b'\n')
                            .count()
                            + 1,
                        source: source.to_string(),
                    });
                }
                cursor = content_end + close.len();
                continue;
            }
        }
        if bytes[cursor] == b'"' {
            let content_start = cursor + 1;
            let mut end = content_start;
            while end < bytes.len() {
                if bytes[end] == b'\\' {
                    end = end.saturating_add(2);
                } else if bytes[end] == b'"' {
                    break;
                } else {
                    end += 1;
                }
            }
            if end < bytes.len() {
                let source = &text[content_start..end];
                if failure_looks_like_jet(source) {
                    fragments.push(FailureSourceFragment {
                        first_line: text[..cursor]
                            .bytes()
                            .filter(|byte| *byte == b'\n')
                            .count()
                            + 1,
                        source: source.to_string(),
                    });
                }
                cursor = end + 1;
                continue;
            }
        }
        cursor += 1;
    }
    fragments
}

fn failure_source_fragments(path: &Path, text: &str) -> Vec<FailureSourceFragment> {
    if path.extension().is_some_and(|extension| extension == "jet") {
        return vec![FailureSourceFragment {
            first_line: 1,
            source: text.to_string(),
        }];
    }

    if relative_path(path).starts_with("editors/tree-sitter/test/corpus/") {
        let source = text.split_once("\n---").map_or(text, |(source, _)| source);
        return vec![FailureSourceFragment {
            first_line: 1,
            source: source.to_string(),
        }];
    }

    let markdown = path.extension().is_some_and(|extension| extension == "md")
        || file_name(path) == "llms.text";
    if !markdown {
        return failure_embedded_fragments(text);
    }

    let mut fragments = Vec::new();
    let mut fence: Option<(u8, usize, usize, bool, String)> = None;
    for (index, line) in text.lines().enumerate() {
        if let Some((open_marker, open_length, _, active, source)) = fence.as_mut() {
            if failure_fence(line).is_some_and(|(marker, length, rest)| {
                marker == *open_marker && length >= *open_length && rest.trim().is_empty()
            }) {
                let (_, _, first_line, active, source) = fence.take().expect("fence exists");
                if active {
                    fragments.push(FailureSourceFragment { first_line, source });
                }
            } else if *active {
                source.push_str(line);
                source.push('\n');
            }
        } else if let Some((marker, length, info)) = failure_fence(line) {
            let language = info.split_whitespace().next().unwrap_or("");
            fence = Some((
                marker,
                length,
                index + 2,
                matches!(language, "jet" | "Jet" | "jetlang"),
                String::new(),
            ));
        } else {
            for source in failure_inline_fragments(line) {
                fragments.push(FailureSourceFragment {
                    first_line: index + 1,
                    source,
                });
            }
        }
    }
    if let Some((_, _, first_line, active, source)) = fence {
        if active {
            fragments.push(FailureSourceFragment { first_line, source });
        }
    }
    fragments
}

fn collect_failure_tokens<'a>(
    tokens: &'a [jet::Lexer::Token],
    out: &mut Vec<&'a jet::Lexer::Token>,
) {
    for token in tokens {
        match &token.kind {
            jet::Lexer::TokKind::LineComment(_) | jet::Lexer::TokKind::BlockComment(_) => {}
            jet::Lexer::TokKind::Str(parts) => {
                out.push(token);
                for part in parts {
                    if let jet::Lexer::StrTokPart::Interp(inner) = part {
                        collect_failure_tokens(inner, out);
                    }
                }
            }
            jet::Lexer::TokKind::Eof => {}
            _ => out.push(token),
        }
    }
}

fn failure_is_type_atom(token: &jet::Lexer::Token) -> bool {
    matches!(
        &token.kind,
        jet::Lexer::TokKind::Ident(_)
            | jet::Lexer::TokKind::RParen
            | jet::Lexer::TokKind::RBracket
            | jet::Lexer::TokKind::Gt
            | jet::Lexer::TokKind::Shr
    )
}

fn failure_is_contract_position(token: &jet::Lexer::Token) -> bool {
    failure_is_type_atom(token)
        || matches!(
            &token.kind,
            jet::Lexer::TokKind::Colon
                | jet::Lexer::TokKind::ColonColon
                | jet::Lexer::TokKind::RParen
        )
}

fn failure_is_expression_end(token: &jet::Lexer::Token) -> bool {
    matches!(
        &token.kind,
        jet::Lexer::TokKind::Ident(_)
            | jet::Lexer::TokKind::Str(_)
            | jet::Lexer::TokKind::Int(..)
            | jet::Lexer::TokKind::Float(..)
            | jet::Lexer::TokKind::UnitNumber { .. }
            | jet::Lexer::TokKind::Char(_)
            | jet::Lexer::TokKind::KwTrue
            | jet::Lexer::TokKind::KwFalse
            | jet::Lexer::TokKind::KwNull
            | jet::Lexer::TokKind::KwIt
            | jet::Lexer::TokKind::KwSelf
            | jet::Lexer::TokKind::RParen
            | jet::Lexer::TokKind::RBracket
            | jet::Lexer::TokKind::RBrace
            | jet::Lexer::TokKind::PlusPlus
            | jet::Lexer::TokKind::MinusMinus
    )
}

fn failure_is_type_name(token: &jet::Lexer::Token) -> bool {
    matches!(
        &token.kind,
        jet::Lexer::TokKind::Ident(name)
            if name.chars().next().is_some_and(|character| character.is_ascii_uppercase())
    )
}

fn failure_is_adjacent(left: &jet::Lexer::Token, right: &jet::Lexer::Token) -> bool {
    left.span.end == right.span.start
}

fn failure_is_result_arm_separator(
    tokens: &[&jet::Lexer::Token],
    index: usize,
) -> bool {
    let Some(previous) = index.checked_sub(1).and_then(|index| tokens.get(index)) else {
        return false;
    };
    let Some(next) = tokens.get(index + 1) else {
        return false;
    };
    let Some(after_next) = tokens.get(index + 2) else {
        return false;
    };
    matches!(
        (&previous.kind, &next.kind, &after_next.kind),
        (
            jet::Lexer::TokKind::Ident(previous),
            jet::Lexer::TokKind::Ident(next),
            jet::Lexer::TokKind::UnifiedArrow,
        ) if previous.starts_with(|character: char| character.is_ascii_lowercase() || character == '_')
            && next.starts_with(|character: char| character.is_ascii_lowercase() || character == '_')
    )
}

fn failure_is_infix_contract(tokens: &[&jet::Lexer::Token], index: usize) -> bool {
    let Some(previous) = index.checked_sub(1).and_then(|index| tokens.get(index)) else {
        return false;
    };
    let Some(next) = tokens.get(index + 1) else {
        return false;
    };
    if !failure_is_type_atom(previous)
        || !(failure_is_type_atom(next)
            || matches!(&next.kind, jet::Lexer::TokKind::LParen))
    {
        return false;
    }
    if failure_is_result_arm_separator(tokens, index) {
        return false;
    }
    failure_is_type_name(previous) || failure_is_type_name(next)
}

fn failure_is_optional_prefix(tokens: &[&jet::Lexer::Token], index: usize) -> bool {
    let Some(question) = tokens.get(index) else {
        return false;
    };
    let Some(next) = tokens.get(index + 1) else {
        return false;
    };
    let type_head = failure_is_type_name(next)
        || matches!(
            &next.kind,
            jet::Lexer::TokKind::LParen
                | jet::Lexer::TokKind::LBracket
                | jet::Lexer::TokKind::Star
                | jet::Lexer::TokKind::KwFn
        );
    type_head && failure_is_adjacent(question, next)
}

fn failure_syntax_hits(source: &str) -> Vec<(usize, &'static str)> {
    let (lexed, _) = jet::Lexer::lex(source);
    let mut tokens = Vec::new();
    collect_failure_tokens(&lexed, &mut tokens);
    tokens.sort_by_key(|token| token.span.start);
    let line = |offset| source[..offset].bytes().filter(|byte| *byte == b'\n').count() + 1;
    let mut hits = Vec::new();

    for (index, token) in tokens.iter().enumerate() {
        match &token.kind {
            jet::Lexer::TokKind::Bang => {
                let marker_negation = index >= 2
                    && matches!(
                        &tokens[index - 2].kind,
                        jet::Lexer::TokKind::Hash
                    );
                let canonical_prefix = tokens.get(index + 1).is_some_and(|next| {
                    failure_is_adjacent(token, next)
                        && matches!(
                            &next.kind,
                            jet::Lexer::TokKind::Ident(_)
                                | jet::Lexer::TokKind::LParen
                                | jet::Lexer::TokKind::LBracket
                                | jet::Lexer::TokKind::Star
                                | jet::Lexer::TokKind::KwFn
                        )
                });
                if marker_negation
                    || canonical_prefix
                    || failure_is_result_arm_separator(&tokens, index)
                {
                    continue;
                }
                if index
                    .checked_sub(1)
                    .and_then(|index| tokens.get(index))
                    .is_some_and(|token| failure_is_contract_position(token))
                {
                    hits.push((line(token.span.start), "retired failure contract"));
                }
            }
            jet::Lexer::TokKind::Question => {
                let contextual = matches!(
                    tokens.get(index + 1).map(|token| &token.kind),
                    Some(jet::Lexer::TokKind::LParen)
                );
                let result_handler = tokens
                    .get(index + 1)
                    .and_then(|next| tokens.get(index + 2).map(|after| (next, after)))
                    .is_some_and(|(next, after)| {
                        matches!(
                            (&next.kind, &after.kind),
                            (jet::Lexer::TokKind::Ident(name), jet::Lexer::TokKind::UnifiedArrow)
                                if name.starts_with(|character: char| character.is_ascii_lowercase() || character == '_')
                        )
                    });
                let expression_end = index
                    .checked_sub(1)
                    .and_then(|index| tokens.get(index))
                    .is_some_and(|token| failure_is_expression_end(*token));
                if contextual
                    || result_handler
                    || failure_is_optional_prefix(&tokens, index)
                {
                    continue;
                }
                if failure_is_infix_contract(&tokens, index) || expression_end {
                    hits.push((line(token.span.start), "retired failure propagation/type"));
                }
            }
            _ => {}
        }
    }
    hits
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
}

fn read(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_string_lossy().to_string();
    (ext == "rs" || ext == "jet").then(|| fs::read_to_string(path).ok())?
}

fn contains_word(text: &str, word: &str) -> bool {
    let is_word = |character: Option<char>| {
        character.is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
    };
    text.match_indices(word).any(|(index, _)| {
        !is_word(text[..index].chars().next_back())
            && !is_word(text[index + word.len()..].chars().next())
    })
}

const RETIRED_AUTHORITY_WORDS: &[&str] = &[
    concat!("Abil", "ity"),
    concat!("Abil", "ities"),
    concat!("Capabil", "ity"),
    concat!("Cap", "s"),
];

const RETIRED_AUTHORITY_MARKERS: &[&str] = &[
    concat!("#", "Abil", "ities"),
    concat!("#", "Cap", "s"),
    concat!("#", "Grant"),
    concat!("#", "grant"),
];

const RETIRED_AUTHORITY_IDENTIFIERS: &[&str] = &[
    concat!("KW_", "CAPS"),
    concat!("TYPE_", "ABILITIES"),
    concat!("CAP_", "HANDLE_TYPE"),
    concat!("Stmt::", "Cap", "s"),
];

fn relative_path(path: &Path) -> String {
    path.strip_prefix(env!("CARGO_MANIFEST_DIR"))
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn is_authority_history(path: &str) -> bool {
    path == "docs/spec/syntax-decisions.md"
        || path == "docs/agents/agent-memory.md"
        || path == "docs/reference/prior-art.md"
        || path.starts_with("docs/archive/")
        || path.starts_with("docs/audits/")
        || path.starts_with("docs/plans/")
        || path.starts_with("docs/proposals/")
        || path.starts_with("docs/research/")
}

fn is_authority_diagnostic_fixture(path: &str) -> bool {
    path.starts_with("tests/ui/")
        || path.starts_with("tests/fuzz/")
        || path == "tests/syntax_reconciliation.rs"
        || path == "scripts/notebook-test/acceptance.mjs"
        || path == "docs/spec/diagnostics.md"
        || path == "docs/spec/diagnostic-rows.md"
}

fn is_authority_generated_diagnostic_row(path: &str, line: &str) -> bool {
    if path != "llms.text" {
        return false;
    }
    let mut fields = line.split('\t');
    fields.next() == Some("retired") || fields.next() == Some("retired")
}

fn is_authority_diagnostic_producer(path: &str, line: &str) -> bool {
    if path == "crates/jet-codegen/src/Prelude/Markers.jet" {
        return line.trim_start().starts_with("marker ")
            && (line.contains(concat!("marker ", "Abil", "ities"))
                || line.contains(concat!("marker ", "Cap", "s")));
    }
    if path == "crates/jet-sema/src/Sema/CheckerCoreLib/core_types.rs" {
        return line.trim_start().starts_with(concat!("\"", "Abil"))
            || line.trim_start().starts_with(concat!("\"", "Cap"));
    }
    if path == "crates/jet-codegen/src/Prelude/Diagnostics.jet" {
        return line.starts_with("diagnostic\tE0077\t") && line.contains(concat!("#", "Grant"));
    }
    if path == "crates/jet-parser/src/Parser/Statements/control.rs" {
        return line.contains(concat!("the `#", "Grant` scope marker is retired"));
    }
    if path == "crates/jet-foundation/src/Syntax/retirements.rs" {
        return line.contains(concat!("retired: \"#", "Grant\""));
    }
    if path == "crates/jet-parser/src/Parser/mod.rs" {
        return line.contains(concat!("#", "Grant"));
    }
    false
}

fn is_unrelated_authority_word(path: &str, line: &str) -> bool {
    if matches!(
        path,
        "crates/jet-pkg-model/src/CompilerExtension.rs"
            | "crates/jet-driver/src/CompilerExtensionHook.rs"
            | "crates/jetpack-bin/tests/compiler_extension_e2e.rs"
    ) {
        return true;
    }
    if path.starts_with("docs/reference/surfaces/")
        || path == "docs/reference/core-surface-ledger.json"
    {
        return true;
    }
    matches!(
        (path, line),
        ("crates/jet-canvas/src/js/inspector-connections.js", line)
            if line.contains(concat!("Capabil", "ity mismatch"))
    ) || matches!(
        (path, line),
        ("crates/jet-devserver/src/WebHost.rs", line)
            if line.contains(concat!("Abil", "ity check"))
    ) || matches!(
        (path, line),
        ("crates/jet-driver/src/Driver/mod.rs", line)
            if line.contains(concat!("Abil", "ity grants"))
    ) || matches!(
        (path, line),
        ("crates/jet-driver/src/Foreign.rs", line)
            if line.contains(concat!("Capabil", "ity report"))
    ) || matches!(
        (path, line),
        ("crates/jet-pkg-model/src/FFI.rs", line)
            if line.contains(concat!("Capabil", "ity-bearing"))
    ) || matches!(
        (path, line),
        ("crates/jet-repl/src/Notebook/trust.rs", line)
            if line.contains(concat!("Abil", "ity widget"))
    ) || matches!(
        (path, line),
        ("crates/jet-sema/src/Sema/mod.rs", line)
            if line.contains(concat!("Abil", "ity")) && line.contains("derive rows")
    ) || matches!(
        (path, line),
        ("crates/jetpack/src/Recipe.rs", line)
            if line.contains(concat!("Capabil", "ity-limited"))
    ) || matches!(
        (path, line),
        ("examples/features/io/app_config.jet", line)
            if line
                .trim_start()
                .starts_with(concat!("// ", "Abil", "ity"))
    ) || matches!(
        (path, line),
        ("examples/features/net/browser_cdp.jet", line)
            if line.contains(concat!("Abil", "ity-gated protocol"))
    ) || matches!(
        (path, line),
        ("scripts/canvas-test/scenario.mjs", line)
            if line.contains(concat!("Capabil", "ity mismatch"))
    ) || matches!(
        (path, line),
        ("docs/spec/spec.md", line)
            if line.contains(concat!("Capabil", "ity parameters"))
    )
}

fn authority_retired_words_in_line(line: &str) -> Vec<&'static str> {
    let mut hits = Vec::new();
    for word in RETIRED_AUTHORITY_WORDS {
        if contains_word(line, word) {
            hits.push(*word);
        }
    }
    for marker in RETIRED_AUTHORITY_MARKERS {
        if line.contains(marker) {
            hits.push(*marker);
        }
    }
    for identifier in RETIRED_AUTHORITY_IDENTIFIERS {
        if line.contains(identifier) {
            hits.push(*identifier);
        }
    }
    hits
}

#[test]
fn retirement_walk_skips_outputs_and_nested_worktrees_but_keeps_live_source_visible() {
    assert!(is_skipped_dir(Path::new("build")));
    assert!(is_skipped_dir(Path::new(
        ".claude/worktrees/worker/examples"
    )));
    assert!(is_skipped_dir(Path::new(
        ".agent-worktrees/worker/examples"
    )));
    assert!(!is_skipped_dir(Path::new("examples")));
    let live_type = concat!("De", "que");
    let live_source = format!("let queue: {live_type}<Int> = {live_type}.new()");
    assert!(contains_word(&live_source, live_type));
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

/// Whether a line writes a package lint policy value or source-level lint
/// allowance, and whether its first value is a retired diagnostic code. Test
/// fixtures under `tests/ui` are excluded by `content_files`; this only counts
/// repository content that can become live package/config/source input.
fn lint_policy_value_is_code(line: &str) -> Option<bool> {
    let value = if let Some(lints) = line.find("lints") {
        let deny = line[lints..].find("deny")? + lints;
        let open = line[deny..].find('[')? + deny;
        &line[open + 1..]
    } else if let Some(allow) = line.find("#allow(") {
        &line[allow + "#allow(".len()..]
    } else {
        return None;
    }
    .trim_start();
    let token = value
        .split(|character: char| {
            character == ',' || character == ']' || character == ')' || character.is_whitespace()
        })
        .next()
        .unwrap_or("")
        .trim_matches('"');
    if token.is_empty() {
        return None;
    }
    let mut chars = token.chars();
    let code_shape = matches!(chars.next(), Some('E' | 'L' | 'W'))
        && chars.count() == 4
        && token[1..]
            .chars()
            .all(|character| character.is_ascii_digit());
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

fn tally_collection_example(
    path_suffix: &str,
    retired_form: &str,
    canonical_form: &str,
) -> (usize, usize) {
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

/// Detect an effect spelling in a source surface, rather than counting a
/// language name in binder code or ordinary prose.
fn writes_effect_spelling(text: &str, spelling: &str) -> bool {
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = line
            .strip_prefix("effect ")
            .and_then(|tail| tail.split_whitespace().next())
        {
            if name == spelling {
                return true;
            }
        }
        for marker in [":[", "=[", "#(", "allow: [", "deny: ["] {
            let mut rest = line;
            while let Some(start) = rest.find(marker) {
                let values = &rest[start + marker.len()..];
                let Some(end) = values.find(']') else { break };
                if values[..end]
                    .split(',')
                    .map(str::trim)
                    .map(|value| value.strip_prefix('!').unwrap_or(value).trim())
                    .any(|value| value == spelling)
                {
                    return true;
                }
                rest = &values[end + 1..];
            }
        }
    }
    false
}

/// Files on the retired form and files on the canonical form, for one row.
fn tally(row: &Retirement) -> (usize, usize) {
    match row.id {
        "entry-file" | "manifest-file" => {
            let files = all_files();
            let count = |name: &str| files.iter().filter(|p| file_name(p) == name).count();
            (count(row.retired), count(row.canonical))
        }
        "jetpack-file" => {
            let files = all_files();
            let retired = files
                .iter()
                .filter(|path| file_name(path) == JETPACK_TOML)
                .count();
            let canonical = files
                .iter()
                .filter(|path| matches!(file_name(path).as_str(), "package.jet" | "env.jet"))
                .count();
            (retired, canonical)
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
        "auto-derive-policy" => {
            let mut retired = 0;
            let mut canonical = 0;
            for path in content_files() {
                if path.extension().is_none_or(|ext| ext != "jet") {
                    continue;
                }
                let Some(text) = read(&path) else { continue };
                if text.contains("policy: .{ auto_derive") {
                    retired += 1;
                } else if text.contains("policy: .{ lints: .{ deny: [auto_derive]") {
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
        "scope-marker-grant" => {
            let mut retired = 0;
            let mut canonical = 0;
            for path in content_files() {
                if path.ends_with("crates/jet-codegen/src/Prelude/Diagnostics.jet")
                    || path.ends_with("crates/jet-foundation/src/Syntax/retirements.rs")
                    || path.ends_with("crates/jet-parser/src/Parser/Statements/control.rs")
                    || path.ends_with("crates/jet-parser/src/Parser/mod.rs")
                    || path.ends_with("tests/retirement_ratchet.rs")
                {
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
        "set-take" => {
            tally_collection_example("examples/features/collections/set.jet", ".take(", ".pop(")
        }
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
        id if id.starts_with("core-namespace-") => {
            let mut retired = 0;
            let mut canonical = 0;
            for path in content_files() {
                if path.extension().is_none_or(|ext| ext != "jet") {
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
        id if id.starts_with("effect-flat-ffi-") => {
            let mut retired = 0;
            let mut canonical = 0;
            for path in content_files() {
                if path.ends_with("tests/effect_roots.rs") {
                    continue;
                }
                let Some(text) = read(&path) else { continue };
                if writes_effect_spelling(&text, row.retired) {
                    retired += 1;
                } else if writes_effect_spelling(&text, row.canonical) {
                    canonical += 1;
                }
            }
            (retired, canonical)
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
        let adoption = if total == 0 {
            100.0
        } else {
            canonical as f64 * 100.0 / total as f64
        };
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
fn failure_syntax_detector_accepts_current_forms_only() {
    let current = concat!(
        "fn maybe() ?Int -> None\n",
        "fn typed() Int !IOError -> 1\n",
        "fn union() Int !(DbError | TimeoutError) -> 1\n",
        "fn unit() !IOError {}\n",
        "struct Holder { value: !IOError }\n",
        "fn context() Int !IOError -> read()?(\"loading\")\n",
        "fn handled() Int !IOError -> value ? ok -> ok ! failure -> 0\n",
        "if !ready -> print(\"ready\")\n",
    );
    assert!(
        failure_syntax_hits(current).is_empty(),
        "current failure syntax was misclassified: {:?}",
        failure_syntax_hits(current)
    );

    let retired = [
        concat!("fn suffix() Int Error", "!\n"),
        concat!("fn infix_bang() Int ", "!", " Error -> 1\n"),
        concat!("fn infix_question() Int ", "?", " Error -> 1\n"),
        concat!("fn suffix_option() Int", "? -> None\n"),
        concat!("fn bare() ", "!", " {}\n"),
        concat!("alias bare :: ", "!\n"),
        concat!("fn propagated() Int !Error -> read()", "?\n"),
        concat!("fn literal_propagation() Bool -> ", "true", "?\n"),
        concat!("fn string_propagation() String -> ", "\"value\"", "?\n"),
        concat!("fn null_propagation() Null -> ", "null", "?\n"),
    ];
    for source in retired {
        assert!(
            !failure_syntax_hits(source).is_empty(),
            "retired failure syntax was missed: {source:?}"
        );
    }
}

#[test]
fn failure_surface_has_no_active_retired_spelling() {
    let mut offenders = Vec::new();
    for path in failure_surface_files() {
        let relative = relative_path(&path);
        if failure_surface_allowlisted(&relative) {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for fragment in failure_source_fragments(&path, &text) {
            for (line, spelling) in failure_syntax_hits(&fragment.source) {
                offenders.push(format!(
                    "{relative}:{}: {spelling}",
                    fragment.first_line + line.saturating_sub(1)
                ));
            }
        }
    }
    assert_eq!(
        offenders.len(),
        FAILURE_RETIREMENT_CEILING,
        "retired failure syntax escaped the diagnostic/history allowlist:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn retired_authority_vocabulary_stays_in_fixtures_history_or_unrelated_english() {
    let mut offenders = Vec::new();
    for path in all_files() {
        let relative = relative_path(&path);
        if relative.starts_with("plugins/tower/") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for (line_number, line) in text.lines().enumerate() {
            let hits = authority_retired_words_in_line(line);
            if hits.is_empty()
                || is_authority_history(&relative)
                || is_authority_diagnostic_fixture(&relative)
                || is_authority_generated_diagnostic_row(&relative, line)
                || is_authority_diagnostic_producer(&relative, line)
                || is_unrelated_authority_word(&relative, line)
            {
                continue;
            }
            offenders.push(format!(
                "{relative}:{}: {}",
                line_number + 1,
                hits.join(", ")
            ));
        }
    }
    assert!(
        offenders.is_empty(),
        "retired authority vocabulary escaped its fixture/history boundary:\n{}",
        offenders.join("\n")
    );
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
        !is_builtin_body.contains("REF_SOURCE_")
            || is_builtin_body.contains("REF_SOURCE_PROVIDERS"),
        "RefSpec::Source::is_builtin must read Syntax::REF_SOURCE_PROVIDERS, not hand-copy \
         individual REF_SOURCE_* constants into a second list:\n{is_builtin_body}"
    );
    assert!(
        is_builtin_body.contains("REF_SOURCE_PROVIDERS"),
        "RefSpec::Source::is_builtin must call Syntax::REF_SOURCE_PROVIDERS.contains(..):\n{is_builtin_body}"
    );
}
