//! D-FE-REPL1=D interactive TTY event loop.
//!
//! Reads raw key events from `Terminal::KeyReader` and drives a small
//! single-line editor (ghost autosuggest from session history, prefix/member
//! completion on Tab) plus the notebook/workspace layers: a dim turn-number
//! gutter on every prompt, a `📌` pin rail redrawn above the prompt each
//! cycle, auto-folding long `List` echoes, and a `^B` bindings panel.
//!
//! `^P`/`^F`/`^R` act on the most recently completed turn and are only read
//! as commands when the input line is empty (pressed right at a fresh
//! prompt, matching the ratified mock) — mid-edit they're reserved rather
//! than risking an ambiguous interpretation of a half-typed line.
//!
//! All *decisions* here (what to fold, what a replay plan looks like, what
//! `?name` resolves to) come from `Render`/`RerunPlan`/`Docs` — this module
//! is the only place that writes raw ANSI/cursor-control bytes.

use std::collections::HashSet;
use std::io::{self, Read, Write};
use std::path::Path;

use super::Terminal::{Key, KeyReader, RawGuard};
use super::{
    apply_replay_plan, dim, execute_line, set_turn_flag, unfold_turn, Docs, RerunPlan, Render,
    EffectPrompt, PromptChoice, ReplFlags, ReplPolicy, Session,
};

/// Run the interactive raw-mode REPL loop. `_guard` is held for its
/// lifetime (RAII). Ordinary exits restore on drop; an authorized
/// `core.process.exit` restores explicitly before the interpreter terminates.
pub(crate) fn run_interactive(project_dir: Option<&str>, color: bool, mut guard: RawGuard, flags: ReplFlags) -> i32 {
    let base_dir: std::path::PathBuf = project_dir
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")));

    let mut session = Session::new();
    session.enable_persistent_history();
    let mut policy = ReplPolicy::new(flags, &base_dir);
    if let Some(dir) = project_dir {
        let mut stdout = io::stdout();
        super::load_project_items(Path::new(dir), &mut session, &mut stdout);
    }

    let stdin = io::stdin();
    let mut reader = KeyReader::new(stdin.lock());
    let mut bindings_pane_visible = false;

    loop {
        print_pin_rail(&session, color);

        let turn_no = session.turns.len() + 1;
        let prompt = Render::render_prompt(turn_no, color);
        match read_line(&mut reader, &prompt, "", &mut session, color) {
            LineOutcome::Submitted(text) => {
                let trimmed = text.trim().to_string();
                if trimmed.is_empty() {
                    continue;
                }
                let turns_before = session.turns.len();
                let mut prompt = InteractiveEffectPrompt { reader: &mut reader, guard: &mut guard };
                let mut authorizer = policy.authorizer(Some(&mut prompt));
                if execute_line(&trimmed, &mut session, &base_dir, color, true, false, &mut authorizer) {
                    break;
                }
                if bindings_pane_visible && session.turns.len() > turns_before {
                    let changed: HashSet<String> = session
                        .turns
                        .last()
                        .and_then(|t| t.bound_name.clone())
                        .into_iter()
                        .collect();
                    print_bindings_pane(&session, &changed, color);
                }
            }
            LineOutcome::Eof => {
                println!();
                break;
            }
            LineOutcome::CtrlB => {
                bindings_pane_visible = !bindings_pane_visible;
                if bindings_pane_visible {
                    print_bindings_pane(&session, &HashSet::new(), color);
                } else {
                    println!("{}", dim("bindings pane closed", color));
                }
            }
            LineOutcome::CtrlP => cmd_toggle_pin(&mut session, color),
            LineOutcome::CtrlF => cmd_toggle_fold(&mut session, color),
            LineOutcome::CtrlR => cmd_rerun(&mut reader, &mut session, &base_dir, color, &mut policy, &mut guard),
        }
    }

    0
}

struct InteractiveEffectPrompt<'a, R: Read> {
    reader: &'a mut KeyReader<R>,
    guard: &'a mut RawGuard,
}

impl<R: Read> EffectPrompt for InteractiveEffectPrompt<'_, R> {
    fn choose(&mut self, request: &crate::Comptime::ReplEffectRequest, reused: bool) -> PromptChoice {
        if reused {
            println!("Using session {}.{} authority for `{}`. [c] continue  [r] revoke", request.root, request.operation, request.resource);
            io::stdout().flush().ok();
            loop {
                match self.reader.read_key() {
                    Key::Char('c') | Key::Char('C') => { println!(); return PromptChoice::Continue; }
                    Key::Char('r') | Key::Char('R') => { println!(); return PromptChoice::Revoke; }
                    Key::Idle => continue,
                    _ => { println!(); return PromptChoice::Revoke; }
                }
            }
        }
        if request.operation == "Exit" {
            println!("Core effect Exec requests orderly REPL exit with status {}. [y/N]", request.resource);
            io::stdout().flush().ok();
            loop {
                match self.reader.read_key() {
                    Key::Char('y') | Key::Char('Y') => {
                        println!();
                        self.guard.restore_now();
                        return PromptChoice::Once;
                    }
                    Key::Idle => continue,
                    _ => { println!(); return PromptChoice::Deny; }
                }
            }
        }
        println!("Core effect {} requests runtime authority before this operation.", request.root);
        println!("  operation: {}", request.operation.to_ascii_lowercase());
        println!("  target:    {}", request.resource);
        println!("  [o] once  [s] exact tuple for this session  [d] deny");
        io::stdout().flush().ok();
        loop {
            match self.reader.read_key() {
                Key::Char('o') | Key::Char('O') => { println!(); return PromptChoice::Once; }
                Key::Char('s') | Key::Char('S') => { println!(); return PromptChoice::Session; }
                Key::Char('d') | Key::Char('D') => { println!(); return PromptChoice::Deny; }
                Key::Idle => continue,
                _ => { println!(); return PromptChoice::Deny; }
            }
        }
    }
}

// ── pin rail / bindings pane ────────────────────────────────────────────────

fn print_pin_rail(session: &Session, color: bool) {
    let width = super::Terminal::terminal_width();
    for turn in session.turns.iter().filter(|t| t.pinned) {
        let label = Render::pin_label(session, turn);
        println!("{}", Render::render_pin_rail(&label, turn.id, width, color));
    }
}

fn print_bindings_pane(session: &Session, changed: &HashSet<String>, color: bool) {
    let lines = Render::render_bindings_pane(session, changed);
    println!("{}", dim("┌─ bindings ──────────────────────────", color));
    if lines.is_empty() {
        println!("{}", dim("│ (no bindings yet)", color));
    } else {
        for l in &lines {
            println!("│ {}", l);
        }
    }
    println!("{}", dim("└──────────────────────────────────────", color));
}

fn cmd_toggle_pin(session: &mut Session, color: bool) {
    let Some(last) = session.turns.last() else {
        println!("{}", dim("no turns yet to pin", color));
        return;
    };
    let id = last.id;
    let now_pinned = !last.pinned;
    let _ = set_turn_flag(session, id, "pinned", now_pinned);
    println!(
        "{}",
        dim(
            &format!("turn {} {}", id, if now_pinned { "pinned" } else { "unpinned" }),
            color
        )
    );
}

fn cmd_toggle_fold(session: &mut Session, color: bool) {
    let Some(last) = session.turns.last() else {
        println!("{}", dim("no turns yet to fold", color));
        return;
    };
    let id = last.id;
    if last.folded {
        if let Ok(Some(full)) = unfold_turn(session, id) {
            println!("{}", full);
        }
        println!("{}", dim(&format!("turn {} unfolded", id), color));
    } else {
        let _ = set_turn_flag(session, id, "folded", true);
        println!("{}", dim(&format!("turn {} folded", id), color));
    }
}

// ── rerun (^R, D-FE-REPL-RERUN1=A) ─────────────────────────────────────────

fn cmd_rerun<R: Read>(reader: &mut KeyReader<R>, session: &mut Session, base_dir: &Path, color: bool, policy: &mut ReplPolicy, guard: &mut RawGuard) {
    let Some(last) = session.turns.last() else {
        println!("{}", dim("no turns yet to rerun", color));
        return;
    };
    let id = last.id;
    let original = last.input.clone();
    println!(
        "{}",
        dim(
            &format!("edit turn {} (Enter to keep as-is, or change it then Enter):", id),
            color
        )
    );
    let prompt = format!("{} ", dim(&format!("{}~", id), color));
    let edited = match read_line(reader, &prompt, &original, session, color) {
        LineOutcome::Submitted(text) => text,
        _ => {
            println!("{}", dim("rerun cancelled", color));
            return;
        }
    };
    let edited_input = if edited.trim().is_empty() || edited == original {
        None
    } else {
        Some(edited.as_str())
    };
    let plan = match RerunPlan::build_replay_plan(&session.turns, id, edited_input) {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("{msg}");
            return;
        }
    };
    if RerunPlan::plan_needs_confirmation(&plan) {
        // Ratified shape: `Replay plan: … Apply? [y/N]` — only shown when
        // there's actually a `y`/`N` to answer; an all-auto plan just
        // applies (see the `else` below), so it never asks a question it
        // doesn't wait for.
        println!("{}", RerunPlan::render_replay_plan(&plan, color));
        io::stdout().flush().ok();
        let confirmed = matches!(reader.read_key(), Key::Char('y') | Key::Char('Y'));
        println!();
        if !confirmed {
            println!(
                "{}",
                dim(
                    "rerun cancelled — effectful steps need confirmation; session unchanged",
                    color
                )
            );
            return;
        }
    } else {
        println!(
            "{}",
            dim(&format!("replaying {} turn(s), all auto", plan.steps.len()), color)
        );
    }
    let mut prompt = InteractiveEffectPrompt { reader, guard };
    let mut authorizer = policy.authorizer(Some(&mut prompt));
    apply_replay_plan(session, &plan, base_dir, color, &mut authorizer);
}

// ── line editor ─────────────────────────────────────────────────────────────

enum LineOutcome {
    Submitted(String),
    Eof,
    CtrlB,
    CtrlP,
    CtrlF,
    CtrlR,
}

/// Read one line with ghost autosuggest (from session history) and Tab
/// completion (session bindings/functions, or builtin members after a `.`).
/// `prefill` seeds the buffer (used by `^R`'s edit-in-place prompt); empty
/// for a normal fresh prompt.
fn read_line<R: Read>(
    reader: &mut KeyReader<R>,
    prompt: &str,
    prefill: &str,
    session: &mut Session,
    color: bool,
) -> LineOutcome {
    let mut buf: Vec<char> = prefill.chars().collect();
    let mut cursor = buf.len();
    let history: Vec<String> = session.history.entries().iter().rev().cloned().collect();

    redraw(prompt, &buf, cursor, ghost_for(&buf, &history), color);

    loop {
        match reader.read_key() {
            Key::Idle => continue,
            Key::Eof => {
                if buf.is_empty() {
                    return LineOutcome::Eof;
                }
                // A stray Ctrl-D mid-line: ignore rather than truncate input.
            }
            Key::CtrlC => {
                // Ctrl-C exits the REPL (owner directive 2026-07-09). With
                // text mid-line the first ^C clears it so a typo never
                // forces an exit; ^C at an empty prompt quits.
                if buf.is_empty() {
                    print!("^C\r\n");
                    io::stdout().flush().ok();
                    return LineOutcome::Eof;
                }
                buf.clear();
                cursor = 0;
                print!("^C\r\n");
                redraw(prompt, &buf, cursor, None, color);
            }
            Key::CtrlB if buf.is_empty() => return LineOutcome::CtrlB,
            Key::CtrlP if buf.is_empty() => return LineOutcome::CtrlP,
            Key::CtrlF if buf.is_empty() => return LineOutcome::CtrlF,
            Key::CtrlR if buf.is_empty() => return LineOutcome::CtrlR,
            Key::Enter => {
                print!("\r\n");
                io::stdout().flush().ok();
                if buf.is_empty() {
                    // D-FE-REPL1=D: "unfold ⏎" — Enter at an empty prompt
                    // right after a fold marker unfolds it in place instead
                    // of submitting an empty line.
                    if let Some(last) = session.turns.last() {
                        if last.folded && last.pending_unfold.is_some() {
                            let id = last.id;
                            if let Ok(Some(full)) = unfold_turn(session, id) {
                                println!("{}", full);
                            }
                            redraw(prompt, &buf, cursor, None, color);
                            continue;
                        }
                    }
                }
                return LineOutcome::Submitted(buf.iter().collect());
            }
            Key::Backspace => {
                if cursor > 0 {
                    cursor -= 1;
                    buf.remove(cursor);
                }
            }
            Key::Delete => {
                if cursor < buf.len() {
                    buf.remove(cursor);
                }
            }
            Key::Left => cursor = cursor.saturating_sub(1),
            Key::Right => {
                let ghost = ghost_for(&buf, &history);
                if cursor == buf.len() {
                    if let Some(g) = ghost {
                        // Accept the ghost suggestion in full.
                        buf.extend(g.chars());
                        cursor = buf.len();
                        redraw(prompt, &buf, cursor, None, color);
                        continue;
                    }
                }
                cursor = (cursor + 1).min(buf.len());
            }
            Key::End => {
                if let Some(g) = ghost_for(&buf, &history) {
                    buf.extend(g.chars());
                }
                cursor = buf.len();
            }
            Key::Home => cursor = 0,
            Key::Up => {
                if let Some(h) = history.first() {
                    buf = h.chars().collect();
                    cursor = buf.len();
                }
            }
            Key::Down => {
                buf.clear();
                cursor = 0;
            }
            Key::Tab => {
                apply_completion(&mut buf, &mut cursor, session, color);
            }
            Key::F3 => {
                print!("\r\n");
                if let Some(found) = read_history_search(reader, session, color) {
                    buf = found.chars().collect();
                    cursor = buf.len();
                }
            }
            Key::Escape | Key::Unknown | Key::F1 | Key::CtrlB | Key::CtrlP | Key::CtrlF
            | Key::CtrlR => {
                // Mid-edit control keys are reserved (see module docs); a
                // bare Escape/unrecognized byte is swallowed rather than
                // inserted as a literal character.
            }
            Key::Char(c) => {
                buf.insert(cursor, c);
                cursor += 1;
            }
        }
        redraw(prompt, &buf, cursor, ghost_for(&buf, &history), color);
    }
}

fn read_history_search<R: Read>(
    reader: &mut KeyReader<R>,
    session: &Session,
    color: bool,
) -> Option<String> {
    let prompt = "history search> ";
    let mut query = Vec::new();
    loop {
        redraw(prompt, &query, query.len(), None, color);
        match reader.read_key() {
            Key::Char(c) => query.push(c),
            Key::Backspace => {
                query.pop();
            }
            Key::Enter => {
                print!("\r\n");
                let needle: String = query.into_iter().collect();
                let found = session
                    .history
                    .search(&needle)
                    .into_iter()
                    .next()
                    .map(str::to_string);
                if found.is_none() {
                    println!("No history matches.");
                }
                return found;
            }
            Key::Escape | Key::CtrlC => {
                print!("\r\n");
                return None;
            }
            Key::Idle => continue,
            _ => {}
        }
    }
}

/// Longest suffix from the most recent matching history entry — the ghost
/// autosuggest text shown dim after the cursor.
fn ghost_for(buf: &[char], history: &[String]) -> Option<String> {
    if buf.is_empty() {
        return None;
    }
    let prefix: String = buf.iter().collect();
    history
        .iter()
        .find(|h| h.len() > prefix.len() && h.starts_with(&prefix))
        .map(|h| h[prefix.len()..].to_string())
}

/// Redraw the current line in place: `\r` + clear-to-EOL, prompt, typed
/// text, dim ghost suggestion (only when the cursor is at the end), cursor
/// repositioned back to `cursor`.
fn redraw(prompt: &str, buf: &[char], cursor: usize, ghost: Option<String>, color: bool) {
    print!("\r\x1b[K{}", prompt);
    let typed: String = buf.iter().collect();
    print!("{}", typed);
    let ghost_len = if cursor == buf.len() {
        if let Some(g) = &ghost {
            print!("{}", dim(g, color));
            g.chars().count()
        } else {
            0
        }
    } else {
        0
    };
    let back = (buf.len() - cursor) + ghost_len;
    if back > 0 {
        print!("\x1b[{}D", back);
    }
    io::stdout().flush().ok();
}

/// Tab completion: bare-identifier prefix over session bindings/functions,
/// or (after a `.`) builtin member names for the receiver's live type —
/// sourced from `Docs::BUILTIN_DOCS`, the same table `?name` reads, so the
/// completion menu and its docs never drift apart (I8).
fn apply_completion(buf: &mut Vec<char>, cursor: &mut usize, session: &Session, color: bool) {
    let text: String = buf[..*cursor].iter().collect();
    let (replace_start, candidates) = if let Some(dot) = text.rfind('.') {
        let receiver = &text[..dot];
        let partial = &text[dot + 1..];
        let is_ident = !receiver.is_empty()
            && receiver.chars().all(|c| c.is_alphanumeric() || c == '_');
        let candidates = if is_ident {
            session
                .scope
                .get(receiver)
                .map(|v| Docs::method_candidates(super::type_name(v), partial))
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        (dot + 1, candidates)
    } else {
        let start = text
            .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
            .map(|i| i + 1)
            .unwrap_or(0);
        let partial = &text[start..];
        if partial.is_empty() {
            (start, Vec::new())
        } else {
            let mut names: Vec<String> = session
                .scope
                .keys()
                .chain(session.func_defs.keys())
                .filter(|n| n.starts_with(partial))
                .cloned()
                .collect();
            names.sort();
            names.dedup();
            (start, names)
        }
    };

    if candidates.is_empty() {
        return;
    }
    if candidates.len() == 1 {
        insert_completion(buf, cursor, replace_start, &candidates[0]);
        return;
    }
    // Shell-style: complete to the longest common prefix, then list matches
    // once beneath the line (redrawn away on the next keystroke).
    let common = longest_common_prefix(&candidates);
    let already: String = buf[replace_start..*cursor].iter().collect();
    if common.len() > already.len() {
        insert_completion(buf, cursor, replace_start, &common);
    }
    print!("\r\n{}\r\n", dim(&candidates.join("   "), color));
}

fn insert_completion(buf: &mut Vec<char>, cursor: &mut usize, replace_start: usize, completion: &str) {
    buf.truncate(replace_start);
    buf.extend(completion.chars());
    *cursor = buf.len();
}

fn longest_common_prefix(items: &[String]) -> String {
    let mut items = items.iter();
    let Some(first) = items.next() else {
        return String::new();
    };
    let mut prefix: Vec<char> = first.chars().collect();
    for item in items {
        let chars: Vec<char> = item.chars().collect();
        let mut i = 0;
        while i < prefix.len() && i < chars.len() && prefix[i] == chars[i] {
            i += 1;
        }
        prefix.truncate(i);
    }
    prefix.into_iter().collect()
}
