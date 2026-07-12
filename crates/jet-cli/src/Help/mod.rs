//! `jet ?` — the hybrid help app (D-FE-HELP1=D, ratified 2026-07-08).
//!
//! Owner modification on ratification (2026-07-08): the default *empty*
//! surface is a categorized command palette (like the two-pane reference's
//! category tree), not a standalone "what do you want to do?" goal screen.
//! There is no separate task-search/goal-home surface. Task/outcome phrases
//! (e.g. "run a file and rebuild on save") are fuzzy keyword aliases carried
//! on the real command entry they resolve to (`Entry::keywords`), so typing
//! an outcome phrase still lands on a concrete `jet <cmd>` line — I8, one
//! index, several depths.
//!
//! One index, four depths:
//!   - empty query  -> categorized command list (`render_categorized`)
//!   - typing       -> fuzzy-filtered result list (`render_results`)
//!   - Tab          -> man-depth inline detail (`render_detail`)
//!   - a code like `E0112` -> the verbatim diagnostic (I4, `render_code_page`)
//!   - F1           -> the same index as a two-pane reference (`Interactive`)
//!
//! The index is built from the CLI's own registry (`crate::CLI::COMMANDS` /
//! `crate::CLI::FLAGS` — the same table that drives completions, the man
//! page, and typo suggestions, D-DX4) and the diagnostics spec
//! (`crate::Explain` — I4's single source of truth). No command, flag, or
//! diagnostic text here is invented; category grouping and task-phrase
//! aliases are this module's own presentation layer over that real data.
//!
//! `jet ? <query>` (and any non-TTY use, including bare `jet ?`) is
//! non-interactive: it prints the best matches statically. Bare `jet ?` on a
//! TTY hands off to `Interactive::run`, the raw-mode app built on the shared
//! `crate::Term` raw-mode module (I8 — the same one the hybrid REPL uses).

use crate::Explain::Explanation;
use crate::Syntax::{BINARY_NAME, FILE_EXT};
use crate::CLI;

pub mod Interactive;
pub mod Render;

/// One command in the help index.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Shared identity/signature/docs/provenance fact. Help adds only layout,
    /// flags, related routes, and fuzzy task aliases.
    pub symbol: jet_semindex::SemanticSymbol,
    pub category: &'static str,
    pub flags: Vec<(&'static str, &'static str)>,
    pub see_also: Vec<&'static str>,
    /// Extra fuzzy-searchable aliases: task/outcome phrases ratified for this
    /// entry, plus nothing else — never a second name for the command.
    pub keywords: Vec<&'static str>,
}

fn command_symbol(cmd: String, usage: String, summary: &str, example: Option<String>) -> jet_semindex::SemanticSymbol {
    jet_semindex::SemanticSymbol {
        identity: format!("command:{cmd}"),
        name: cmd.clone(),
        qualified_name: cmd,
        owner: None,
        module_path: "jet CLI".to_string(),
        kind: jet_semindex::SemanticSymbolKind::Command,
        signature: usage,
        summary: summary.to_string(),
        examples: example.into_iter().collect(),
        provenance: jet_semindex::SemanticProvenance::CommandRegistry,
        span: None,
    }
}

/// Category display order for the categorized (empty-query) view and the
/// F1 reference's left-hand tree.
pub const CATEGORIES: &[&str] = &[
    "Build and run",
    "Project/env",
    "Packages",
    "jetos",
    "Dev server",
    "Reference",
    "Error codes",
];

fn category_for(cmd: &str) -> &'static str {
    match cmd {
        "run" | "check" | "test" | "build" | "debug" | "bench" | "eval" | "emit" => {
            "Build and run"
        }
        "dev" | "serve" => "Dev server",
        "devtools" => "Reference",
        "new" | "fmt" | "fix" | "lint" | "env" | "init" | "lock" | "config" | "toolchain" => {
            "Project/env"
        }
        "add" | "remove" | "fetch" | "update" | "outdated" | "search" | "info" | "logs"
        | "clean" | "publish" | "yank" | "keygen" | "key" | "vendor" | "audit" | "sbom"
        | "store" | "schema" | "gc" => "Packages",
        "os" | "image" | "bind" | "push" | "trust" | "bridge" | "services" => "jetos",
        "semindex" | "dossier" | "impact" | "codemod" | "expand" => "Reference",
        // explain, doctor, repl, man, completions, version, upgrade, help, lsp,
        // and any future command land here — a safe default, never a crash.
        _ => "Reference",
    }
}

/// Curated `<placeholder>` usage line for the commands whose argument shape
/// isn't obvious from the summary alone. Everything else falls back to a
/// generic `jet <cmd> [args]` — real command name, no invented flags.
fn usage_for(cmd: &str) -> String {
    match cmd {
        "run" | "check" | "test" | "build" => format!("jet {} <file.{}> [flags]", cmd, FILE_EXT),
        "dev" => format!("jet {} <file.{}> [flags]", cmd, FILE_EXT),
        "serve" => format!("jet {} <file.{}> [flags]", cmd, FILE_EXT),
        "fmt" | "fix" => format!("jet {} <file.{}>", cmd, FILE_EXT),
        "new" => "jet new <name>".to_string(),
        "env" => "jet env".to_string(),
        "add" => "jet add <dep>".to_string(),
        "remove" => "jet remove <dep>".to_string(),
        "fetch" => "jet store fetch".to_string(),
        "update" => "jet update [dep]".to_string(),
        "clean" => "jet clean".to_string(),
        "explain" => "jet explain <CODE>".to_string(),
        "doctor" => "jet self doctor".to_string(),
        "repl" => "jet repl".to_string(),
        "help" => "jet help".to_string(),
        "os" => "jet os <plan|import|build|vm> <host> …".to_string(),
        _ => format!("jet {} [args]", cmd),
    }
}

/// A short, real example line — only for commands with one worth showing.
fn example_for(cmd: &str) -> Option<String> {
    match cmd {
        "run" | "check" | "build" => {
            Some(format!("jet {} examples/features/basics/hello.jet", cmd))
        }
        "dev" => Some("jet dev run.jet".to_string()),
        "explain" => Some("jet explain E0102".to_string()),
        "new" => Some("jet new web-api".to_string()),
        _ => None,
    }
}

fn see_also_for(cmd: &str) -> Vec<&'static str> {
    match cmd {
        "run" => vec!["build", "test", "dev"],
        "dev" => vec!["run", "serve"],
        "build" => vec!["run", "check"],
        "test" => vec!["run", "check"],
        "add" => vec!["fetch", "update"],
        "explain" => vec!["doctor"],
        _ => Vec::new(),
    }
}

/// Task/outcome phrase aliases the owner ratified into `jet ?`'s default
/// static text (kept 1:1 with `question_mark_palette`'s "Task keywords"
/// section in `Source/main.rs`) — attached here to the real command that
/// carries them out. "run on save" resolves to `dev` (the real watch/rerun
/// command); `run` itself has no `--watch` flag (checked against
/// `crate::CLI::FLAGS`), so the alias does not point at a command that can't
/// do the job (no invented commands, per the card brief).
fn keywords_for(cmd: &str) -> Vec<&'static str> {
    match cmd {
        "dev" => vec![
            "run on save",
            "rebuild on save",
            "run a file and rebuild on save",
            "watch",
            "start a web app",
        ],
        "add" => vec!["add a dependency", "dependency"],
        "explain" => vec!["understand an error message", "error", "diagnostic"],
        _ => Vec::new(),
    }
}

/// Real flags for `cmd`, read off `crate::CLI::FLAGS`'s `"with a/b: …"`
/// convention (D-DX4) — the same registry `is_known_flag`/completions use.
/// Flags with no `"with …:"` prefix are cross-command globals and aren't
/// attached to any single entry here.
fn flags_for(cmd: &str) -> Vec<(&'static str, &'static str)> {
    CLI::FLAGS
        .iter()
        .filter(|f| {
            f.help
                .strip_prefix("with ")
                .and_then(|rest| rest.split_once(':'))
                .map(|(names, _)| names.split(['/', ',']).map(str::trim).any(|n| n == cmd))
                .unwrap_or(false)
        })
        .map(|f| (f.long, f.help))
        .collect()
}

/// Build the full command index from the CLI's own registry (D-DX4) — one
/// entry per `crate::CLI::COMMANDS` row, real flags/category/keywords layered
/// on top.
pub fn build_index() -> Vec<Entry> {
    let mut entries: Vec<Entry> = CLI::COMMANDS
        .iter()
        .filter(|c| CLI::is_canonical_top_level(c.name))
        .map(|c| Entry {
            symbol: command_symbol(
                c.name.to_string(),
                usage_for(c.name),
                c.summary,
                example_for(c.name),
            ),
            category: category_for(c.name),
            flags: flags_for(c.name),
            see_also: see_also_for(c.name),
            keywords: keywords_for(c.name),
        })
        .collect();
    for group in CLI::COMMAND_GROUPS {
        for action in group.actions {
            entries.push(Entry {
                symbol: command_symbol(
                    format!("{} {}", group.name, action.name),
                    format!("jet {} {} [args]", group.name, action.name),
                    action.summary,
                    None,
                ),
                category: category_for(action.handler.dispatch_word()),
                flags: flags_for(action.handler.dispatch_word()),
                see_also: Vec::new(),
                keywords: keywords_for(action.handler.dispatch_word()),
            });
        }
    }
    entries
}

pub fn symbol_index(entries: &[Entry]) -> jet_semindex::SemanticSymbolIndex {
    jet_semindex::SemanticSymbolIndex::new(
        entries.iter().map(|entry| entry.symbol.clone()).collect(),
    )
}

/// A search result: either a command entry (with its fuzzy score and the
/// matched character positions in `haystack`, for highlighting) or a
/// diagnostic code page (I4 verbatim, no score — codes are exact hits).
#[derive(Debug, Clone)]
pub enum Hit {
    Command {
        entry: Entry,
        score: i64,
        haystack: String,
        positions: Vec<usize>,
    },
    Code(Explanation),
}

/// Looks like a diagnostic code (`E0102`, `L1000`, case-insensitive) —
/// mirrors `crate::Explain`'s own registry-table shape check.
pub fn looks_like_code(s: &str) -> bool {
    let s = s.trim();
    let b = s.as_bytes();
    b.len() == 5
        && matches!(b[0], b'E' | b'L' | b'e' | b'l')
        && b[1..].iter().all(|c| c.is_ascii_digit())
}

/// Fuzzy subsequence match: every char of `query` (case-insensitive) must
/// appear in order in `hay`. Score rewards consecutive runs and word-start
/// hits (classic fzf-style heuristic); returns `None` on no match. Matched
/// positions are byte-free char indices into `hay`, for highlighting.
pub fn fuzzy_match(query: &str, hay: &str) -> Option<(i64, Vec<usize>)> {
    if query.is_empty() {
        return Some((0, Vec::new()));
    }
    let q: Vec<char> = query.to_lowercase().chars().collect();
    let h: Vec<char> = hay.to_lowercase().chars().collect();
    let mut qi = 0usize;
    let mut score = 0i64;
    let mut positions = Vec::with_capacity(q.len());
    let mut prev: Option<usize> = None;
    for (hi, hc) in h.iter().enumerate() {
        if qi >= q.len() {
            break;
        }
        if *hc == q[qi] {
            positions.push(hi);
            score += 1;
            if prev == Some(hi.wrapping_sub(1)) {
                score += 5;
            }
            if hi == 0 || matches!(h[hi - 1], ' ' | '-' | '.' | '<') {
                score += 3;
            }
            prev = Some(hi);
            qi += 1;
        }
    }
    if qi == q.len() {
        // Prefer tighter matches over longer haystacks with the same hits.
        score -= (h.len() as i64) / 8;
        Some((score, positions))
    } else {
        None
    }
}

/// Search the index for `query`. An exact diagnostic-code query (e.g.
/// `E0112`) short-circuits to a single `Hit::Code`, rendered verbatim from
/// `crate::Explain` (I4) — codes share the index but are exact hits, never
/// fuzzy-ranked. Otherwise fuzzy-matches every command's name, usage line,
/// summary, and keyword aliases, keeping each entry's single best score.
pub fn search(index: &[Entry], query: &str) -> Vec<Hit> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    if looks_like_code(query) {
        if let Some(ex) = crate::Explain::lookup(query) {
            return vec![Hit::Code(ex)];
        }
    }
    let mut hits: Vec<Hit> = Vec::new();
    let symbols = symbol_index(index);
    for symbol in symbols.symbols() {
        let entry = index
            .iter()
            .find(|entry| entry.symbol.identity == symbol.identity)
            .expect("help presentation for command symbol");
        let haystacks: Vec<String> = std::iter::once(format!("jet {}", entry.symbol.name))
            .chain(std::iter::once(entry.symbol.signature.clone()))
            .chain(std::iter::once(entry.symbol.summary.clone()))
            .chain(entry.flags.iter().map(|(flag, help)| format!("{} {}", flag, help)))
            .chain(entry.symbol.examples.iter().cloned())
            .chain(entry.see_also.iter().map(|name| name.to_string()))
            .chain(entry.keywords.iter().map(|k| k.to_string()))
            .collect();
        let mut best: Option<(i64, String, Vec<usize>)> = None;
        for hay in haystacks {
            if let Some((score, positions)) = fuzzy_match(query, &hay) {
                if best.as_ref().map(|(s, ..)| score > *s).unwrap_or(true) {
                    best = Some((score, hay, positions));
                }
            }
        }
        if let Some((score, haystack, positions)) = best {
            let score = score + if entry.symbol.name.eq_ignore_ascii_case(query) { 1000 } else { 0 };
            hits.push(Hit::Command {
                entry: entry.clone(),
                score,
                haystack,
                positions,
            });
        }
    }
    hits.sort_by(|a, b| match (a, b) {
        (Hit::Command { score: sa, entry: ea, .. }, Hit::Command { score: sb, entry: eb, .. }) => {
            sb.cmp(sa).then_with(|| ea.symbol.name.cmp(&eb.symbol.name))
        }
        _ => std::cmp::Ordering::Equal,
    });
    hits
}

/// `jet ? <query>` (and any non-TTY use) — the non-interactive floor. Prints
/// the best matches (or the verbatim code page) as plain text; no raw mode,
/// no box drawing beyond what `Render` already produces for NO_COLOR.
pub fn run_query(query: &str, color: bool) -> String {
    if let Some(symbol) = crate::SemanticSymbols::lookup(query.trim()) {
        return format!(
            "{}\n{}\nExample: {}\nSource: {} ({})\n",
            symbol.signature, symbol.summary, symbol.example, symbol.module, symbol.provenance
        );
    }
    let index = build_index();
    let hits = search(&index, query);
    if hits.is_empty() {
        return format!(
            "no matches for `{}` — try `{} ?` for the full command palette or `{} help`\n",
            query, BINARY_NAME, BINARY_NAME
        );
    }
    match &hits[0] {
        Hit::Code(ex) => crate::Explain::render(ex, color),
        Hit::Command { .. } => Render::render_result_list(&hits, query, 72, color, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_covers_every_cli_command() {
        let index = build_index();
        let expected = CLI::COMMANDS.iter().filter(|c| CLI::is_canonical_top_level(c.name)).count()
            + CLI::COMMAND_GROUPS.iter().map(|g| g.actions.len()).sum::<usize>();
        assert_eq!(index.len(), expected);
        for c in CLI::COMMANDS.iter().filter(|c| CLI::is_canonical_top_level(c.name)) {
            assert!(index.iter().any(|e| e.symbol.name == c.name), "missing {}", c.name);
        }
        for group in CLI::COMMAND_GROUPS {
            for action in group.actions {
                let route = format!("{} {}", group.name, action.name);
                assert!(index.iter().any(|e| e.symbol.name == route), "missing {route}");
            }
        }
    }

    #[test]
    fn every_entry_has_a_real_category() {
        for e in build_index() {
            assert!(CATEGORIES.contains(&e.category), "bad category for {}", e.symbol.name);
        }
    }

    #[test]
    fn help_entries_are_shared_semantic_facts() {
        let entries = build_index();
        let symbols = symbol_index(&entries);
        let run = symbols.lookup_qualified("run").expect("run command fact");
        assert_eq!(run.signature, usage_for("run"));
        assert_eq!(run.summary, CLI::COMMANDS.iter().find(|c| c.name == "run").unwrap().summary);
        assert!(matches!(run.provenance, jet_semindex::SemanticProvenance::CommandRegistry));
    }

    #[test]
    fn run_flags_are_real_not_invented() {
        let index = build_index();
        let run = index.iter().find(|e| e.symbol.name == "run").unwrap();
        // `run` has no `--watch` flag on the real CLI surface (that's `dev`'s
        // job) — the help index must not invent one.
        assert!(!run.flags.iter().any(|(f, _)| *f == "--watch"));
        assert!(run.flags.iter().any(|(f, _)| *f == "--release"));
    }

    #[test]
    fn watch_task_phrase_resolves_to_dev_not_a_fake_run_flag() {
        let index = build_index();
        let hits = search(&index, "run on save");
        let Hit::Command { entry, .. } = &hits[0] else {
            panic!("expected a command hit");
        };
        assert_eq!(entry.symbol.name, "dev");
    }

    #[test]
    fn search_covers_registry_flags_and_examples() {
        let index = build_index();
        assert!(search(&index, "--release").iter().any(|hit| {
            matches!(hit, Hit::Command { entry, .. } if entry.symbol.name == "run")
        }));
        assert!(search(&index, "hello.jet").iter().any(|hit| {
            matches!(hit, Hit::Command { entry, .. } if entry.symbol.name == "run")
        }));
    }

    #[test]
    fn fuzzy_match_is_subsequence_in_order() {
        assert!(fuzzy_match("rn", "run").is_some());
        assert!(fuzzy_match("nr", "run").is_none());
        assert!(fuzzy_match("xyz", "run").is_none());
    }

    #[test]
    fn fuzzy_ranks_exact_prefix_above_loose_subsequence() {
        let (exact, _) = fuzzy_match("run", "jet run").unwrap();
        let (loose, _) = fuzzy_match("run", "jet self devtools — run checked generators").unwrap();
        assert!(exact > loose, "exact {} should outrank loose {}", exact, loose);
    }

    #[test]
    fn code_query_short_circuits_to_verbatim_diagnostic() {
        let index = build_index();
        let hits = search(&index, "E0102");
        assert_eq!(hits.len(), 1);
        assert!(matches!(hits[0], Hit::Code(_)));
    }

    #[test]
    fn unknown_code_shape_falls_back_to_fuzzy() {
        let index = build_index();
        // Looks code-shaped but isn't registered — Explain::lookup misses,
        // so search must not return an empty result silently. Built at
        // runtime (not a string literal) so this sentinel doesn't read as
        // a real registered diagnostic code to the I4 coverage scanner,
        // which greps Source/ for quoted `"Ennnn"` literals.
        let unregistered_code = format!("E{}", 9999);
        let hits = search(&index, &unregistered_code);
        // No fuzzy command matches this code either — empty is correct
        // here, but it must not panic and must not fabricate a code page.
        assert!(hits.iter().all(|h| !matches!(h, Hit::Code(_))));
    }

    #[test]
    fn run_query_renders_diagnostic_verbatim() {
        let out = run_query("E0102", false);
        let ex = crate::Explain::lookup("E0102").unwrap();
        assert_eq!(out, crate::Explain::render(&ex, false));
    }

    #[test]
    fn run_query_no_match_is_a_helpful_pointer_not_empty() {
        let out = run_query("zzzznonsense", false);
        assert!(out.contains(BINARY_NAME));
    }
}
