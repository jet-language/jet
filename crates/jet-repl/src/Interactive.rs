//! D-FE-REPL1=D interactive TTY event loop.
//!
//! Reads raw key events from `Terminal::KeyReader` and drives a small
//! multiline editor (ghost autosuggest from session history, prefix/member
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

use jet_foundation::Terminal::Theme;
use std::collections::HashSet;
use std::io::{self, Read, Write};
use std::path::Path;

use super::Terminal::{Key, KeyReader, RawGuard};
use super::{
    dim, execute_line, set_turn_flag, unfold_turn, Docs, RerunPlan, Render,
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
        session.preserve_project_baseline();
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
                let interrupt = super::Terminal::EvaluationInterruptGuard::enable();
                let quit = execute_line(&trimmed, &mut session, &base_dir, color, true, false, &mut authorizer);
                let double_interrupt = interrupt.as_ref().is_some_and(|guard| guard.count() >= 2);
                drop(interrupt);
                if quit || double_interrupt {
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
            LineOutcome::CtrlP => cmd_toggle_pin(&mut reader, &mut session, color),
            LineOutcome::CtrlF => cmd_toggle_fold(&mut reader, &mut session, color),
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

    fn read_input(&mut self, prompt: &str) -> io::Result<String> {
        print!("{prompt}");
        io::stdout().flush()?;
        let mut line = String::new();
        loop {
            match self.reader.read_key() {
                Key::Char(ch) => {
                    line.push(ch);
                    print!("{ch}");
                    io::stdout().flush()?;
                }
                Key::Backspace => {
                    if line.pop().is_some() {
                        print!("\x08 \x08");
                        io::stdout().flush()?;
                    }
                }
                Key::Enter => {
                    println!();
                    return Ok(line);
                }
                Key::CtrlC => {
                    println!();
                    return Err(io::Error::new(io::ErrorKind::Interrupted, "input interrupted"));
                }
                Key::Eof => {
                    println!();
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "input closed"));
                }
                Key::Idle => {}
                _ => {}
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
    let width = super::Terminal::terminal_width();
    let split = (width / 2).clamp(20, 48);
    println!("{}", Render::render_workspace_row("┌─ session", "bindings ─┐", split, color));
    if lines.is_empty() {
        println!("{}", Render::render_workspace_row("│", "(no bindings yet)", split, color));
    } else {
        for l in &lines {
            println!("{}", Render::render_workspace_row("│", l, split, color));
        }
    }
    println!("{}", Render::render_workspace_row("└─ session", "bindings ─┘", split, color));
}

fn select_turn<R: Read>(reader: &mut KeyReader<R>, session: &mut Session, action: &str, color: bool) -> Option<usize> {
    if session.turns.is_empty() {
        println!("{}", dim(&format!("no turns yet to {action}"), color));
        return None;
    }
    println!("{}", dim(&format!("select turn to {action}:"), color));
    let text = match read_line(reader, "turn> ", "", session, color) {
        LineOutcome::Submitted(text) => text,
        _ => return None,
    };
    match text.trim().parse::<usize>() {
        Ok(id) if session.turns.iter().any(|turn| turn.id == id) => Some(id),
        _ => {
            eprintln!("turn #{} does not exist", text.trim());
            None
        }
    }
}

fn cmd_toggle_pin<R: Read>(reader: &mut KeyReader<R>, session: &mut Session, color: bool) {
    let Some(id) = select_turn(reader, session, "pin", color) else {
        return;
    };
    let Some(turn) = session.turns.iter().find(|turn| turn.id == id) else {
        println!("{}", dim("no turns yet to pin", color));
        return;
    };
    let now_pinned = !turn.pinned;
    let _ = set_turn_flag(session, id, "pinned", now_pinned);
    println!(
        "{}",
        dim(
            &format!("turn {} {}", id, if now_pinned { "pinned" } else { "unpinned" }),
            color
        )
    );
}

fn cmd_toggle_fold<R: Read>(reader: &mut KeyReader<R>, session: &mut Session, color: bool) {
    let Some(id) = select_turn(reader, session, "fold", color) else {
        return;
    };
    let folded = session.turns.iter().find(|turn| turn.id == id).is_some_and(|turn| turn.folded);
    if folded {
        if let Ok(Some(full)) = unfold_turn(session, id) {
            page_collection(reader, &full, color);
        }
        println!("{}", dim(&format!("turn {} unfolded", id), color));
    } else {
        let _ = set_turn_flag(session, id, "folded", true);
        println!("{}", dim(&format!("turn {} folded", id), color));
    }
}

// ── rerun (^R, D-FE-REPL-RERUN1=A) ─────────────────────────────────────────

fn cmd_rerun<R: Read>(reader: &mut KeyReader<R>, session: &mut Session, base_dir: &Path, color: bool, policy: &mut ReplPolicy, guard: &mut RawGuard) {
    if session.turns.is_empty() {
        println!("{}", dim("no turns yet to rerun", color));
        return;
    }
    println!("{}", dim("select turn to edit:", color));
    let id_text = match read_line(reader, "turn> ", "", session, color) {
        LineOutcome::Submitted(text) => text,
        _ => return,
    };
    let id = match id_text.trim().parse::<usize>() {
        Ok(id) if session.turns.iter().any(|turn| turn.id == id) => id,
        _ => {
            eprintln!("turn #{} does not exist", id_text.trim());
            return;
        }
    };
    let original = session.turns.iter().find(|turn| turn.id == id).unwrap().input.clone();
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
    println!("{}", RerunPlan::render_replay_plan(&plan, color));
    let mut stale_from = None;
    for step in &plan.steps {
        if step.kind != RerunPlan::StepKind::ConfirmEffect {
            continue;
        }
        println!(
            "replay effect turn {}? [y] replay  [s] skip and mark stale",
            step.turn_id
        );
        io::stdout().flush().ok();
        match reader.read_key() {
            Key::Char('y') | Key::Char('Y') => println!(),
            _ => {
                println!();
                stale_from = Some(step.turn_id);
                break;
            }
        }
    }
    let mut prompt = InteractiveEffectPrompt { reader, guard };
    let mut authorizer = policy.authorizer(Some(&mut prompt));
    super::apply_replay_plan_with_stale(
        session,
        &plan,
        stale_from,
        base_dir,
        color,
        &mut authorizer,
    );
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
    let mut display = EditorDisplay::default();

    display.redraw(prompt, &buf, cursor, ghost_for(&buf, &history), color);

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
                    display.finish();
                    print!("^C\r\n");
                    io::stdout().flush().ok();
                    return LineOutcome::Eof;
                }
                buf.clear();
                cursor = 0;
                display.finish();
                print!("^C\r\n");
                display.redraw(prompt, &buf, cursor, None, color);
            }
            Key::CtrlB if buf.is_empty() => return LineOutcome::CtrlB,
            Key::CtrlP if buf.is_empty() => return LineOutcome::CtrlP,
            Key::CtrlF if buf.is_empty() => return LineOutcome::CtrlF,
            Key::CtrlR if buf.is_empty() => return LineOutcome::CtrlR,
            Key::Enter => {
                if buf.is_empty() {
                    // D-FE-REPL1=D: "unfold ⏎" — Enter at an empty prompt
                    // right after a fold marker unfolds it in place instead
                    // of submitting an empty line.
                    if let Some(last) = session.turns.last() {
                        if last.folded && last.pending_unfold.is_some() {
                            let id = last.id;
                            display.finish();
                            if let Ok(Some(full)) = unfold_turn(session, id) {
                                page_collection(reader, &full, color);
                            }
                            display.redraw(prompt, &buf, cursor, None, color);
                            continue;
                        }
                    }
                }
                let blank_continuation = cursor == buf.len()
                    && buf.contains(&'\n')
                    && buf.last().is_some_and(|c| *c == '\n');
                let text: String = buf.iter().collect();
                if !blank_continuation && !super::raw_input_is_complete(&text) {
                    buf.insert(cursor, '\n');
                    cursor += 1;
                    display.redraw(prompt, &buf, cursor, None, color);
                    continue;
                }
                display.finish();
                return LineOutcome::Submitted(buf.iter().collect());
            }
            Key::EscapeEnter => {
                buf.insert(cursor, '\n');
                cursor += 1;
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
                        display.redraw(prompt, &buf, cursor, None, color);
                        continue;
                    }
                }
                cursor = (cursor + 1).min(buf.len());
            }
            Key::End => {
                let end = line_end(&buf, cursor);
                if end == buf.len() {
                    if let Some(g) = ghost_for(&buf, &history) {
                        buf.extend(g.chars());
                    }
                    cursor = buf.len();
                } else {
                    cursor = end;
                }
            }
            Key::Home => cursor = line_start(&buf, cursor),
            Key::Up => {
                if buf.contains(&'\n') {
                    cursor = vertical_cursor(&buf, cursor, false);
                } else if let Some(h) = history.first() {
                    buf = h.chars().collect();
                    cursor = buf.len();
                }
            }
            Key::Down => {
                if buf.contains(&'\n') {
                    cursor = vertical_cursor(&buf, cursor, true);
                } else {
                    buf.clear();
                    cursor = 0;
                }
            }
            Key::Tab => {
                if apply_completion(reader, &mut buf, &mut cursor, session, color) {
                    display = EditorDisplay::default();
                }
            }
            Key::F1 => {
                display.finish();
                print!("\r\n");
                match name_at_cursor(&buf, cursor) {
                    Some(name) => match Docs::lookup(session, &name) {
                        Some(doc) => print!("{doc}"),
                        None => println!("{}", dim(&format!("no docs for `{name}`"), color)),
                    },
                    None => {
                        println!(
                            "{}",
                            dim("move the cursor onto a name, then press F1", color)
                        );
                    }
                }
            }
            Key::F3 => {
                display.finish();
                if let Some(found) = read_history_search(reader, session, color) {
                    buf = found.chars().collect();
                    cursor = buf.len();
                }
            }
            Key::Escape | Key::Unknown | Key::CtrlB | Key::CtrlP | Key::CtrlF
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
        display.redraw(prompt, &buf, cursor, ghost_for(&buf, &history), color);
    }
}

fn read_history_search<R: Read>(
    reader: &mut KeyReader<R>,
    session: &Session,
    color: bool,
) -> Option<String> {
    let prompt = "history search> ";
    let mut query = Vec::new();
    let mut display = EditorDisplay::default();
    loop {
        display.redraw(prompt, &query, query.len(), None, color);
        match reader.read_key() {
            Key::Char(c) => query.push(c),
            Key::Backspace => {
                query.pop();
            }
            Key::Enter => {
                display.finish();
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
                display.finish();
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
    if buf.is_empty() || buf.contains(&'\n') {
        return None;
    }
    let prefix: String = buf.iter().collect();
    history
        .iter()
        .find(|h| !h.contains('\n') && h.len() > prefix.len() && h.starts_with(&prefix))
        .map(|h| h[prefix.len()..].to_string())
}

fn highlight_input(line: &str, color: bool) -> String {
    if !color {
        return line.to_string();
    }
    let mut out = String::new();
    let theme = Theme::new(color);
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '"' {
            let start = i;
            i += 1;
            while i < chars.len() {
                let escaped = i > start && chars[i - 1] == '\\';
                i += 1;
                if chars[i - 1] == '"' && !escaped { break; }
            }
            out.push_str(&theme.success(&chars[start..i].iter().collect::<String>()));
        } else if chars[i].is_ascii_digit() {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') { i += 1; }
            out.push_str(&theme.accent(&chars[start..i].iter().collect::<String>()));
        } else if chars[i].is_alphabetic() || chars[i] == '_' {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') { i += 1; }
            let word: String = chars[start..i].iter().collect();
            if matches!(word.as_str(), "fn" | "if" | "else" | "loop" | "return" | "use" | "struct" | "enum" | "match" | "true" | "false") {
                out.push_str(&theme.warn(&word));
            } else {
                out.push_str(&word);
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn collection_rows(full: &str) -> Vec<String> {
    let Some(start) = full.find('[') else { return vec![full.to_string()]; };
    let Some(relative_end) = full[start..].find("] : List") else { return vec![full.to_string()]; };
    let body = &full[start + 1..start + relative_end];
    let mut rows = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for ch in body.chars() {
        if quoted {
            current.push(ch);
            if ch == '"' && !escaped { quoted = false; }
            escaped = ch == '\\' && !escaped;
            if ch != '\\' { escaped = false; }
            continue;
        }
        match ch {
            '"' => { quoted = true; current.push(ch); }
            '[' | '{' | '(' => { depth += 1; current.push(ch); }
            ']' | '}' | ')' => { depth = depth.saturating_sub(1); current.push(ch); }
            ',' if depth == 0 => { rows.push(current.trim().to_string()); current.clear(); }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() { rows.push(current.trim().to_string()); }
    rows
}

fn page_collection<R: Read>(reader: &mut KeyReader<R>, full: &str, color: bool) {
    let rows = collection_rows(full);
    if rows.len() <= Render::FOLD_LIST_THRESHOLD {
        println!("{full}");
        return;
    }
    const PAGE: usize = 10;
    let mut start = 0usize;
    loop {
        println!("{}", dim("idx │ value", color));
        for (offset, value) in rows.iter().skip(start).take(PAGE).enumerate() {
            println!("{:>3} │ {}", start + offset, value);
        }
        println!("{}", dim(&format!("-- rows {}-{}/{} · j/k page · q close --", start + 1, (start + PAGE).min(rows.len()), rows.len()), color));
        io::stdout().flush().ok();
        match reader.read_key() {
            Key::Char('j') | Key::Down if start + PAGE < rows.len() => start += PAGE,
            Key::Char('k') | Key::Up if start >= PAGE => start -= PAGE,
            Key::Idle => continue,
            _ => break,
        }
    }
}

fn line_start(buf: &[char], cursor: usize) -> usize {
    buf[..cursor]
        .iter()
        .rposition(|c| *c == '\n')
        .map(|i| i + 1)
        .unwrap_or(0)
}

fn line_end(buf: &[char], cursor: usize) -> usize {
    buf[cursor..]
        .iter()
        .position(|c| *c == '\n')
        .map(|i| cursor + i)
        .unwrap_or(buf.len())
}

fn vertical_cursor(buf: &[char], cursor: usize, down: bool) -> usize {
    let start = line_start(buf, cursor);
    let column = cursor - start;
    if down {
        let end = line_end(buf, cursor);
        if end == buf.len() {
            return cursor;
        }
        let next_start = end + 1;
        let next_end = line_end(buf, next_start);
        next_start + column.min(next_end - next_start)
    } else {
        if start == 0 {
            return cursor;
        }
        let previous_end = start - 1;
        let previous_start = line_start(buf, previous_end);
        previous_start + column.min(previous_end - previous_start)
    }
}

/// Whole-buffer raw editor display. Repaints every logical row, then restores
/// cursor to its source position. This keeps insert/delete correct across
/// continuation lines without letting stale text survive a shorter redraw.
#[derive(Default)]
struct EditorDisplay {
    rows: usize,
    cursor_row: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct DisplayPosition {
    row: usize,
    column: usize,
    pending_wrap: bool,
}

impl DisplayPosition {
    fn write_cells(&mut self, cells: usize, terminal_width: usize) {
        if cells == 0 {
            return;
        }
        let width = terminal_width.max(1);
        if self.pending_wrap {
            self.row += 1;
            self.column = 0;
            self.pending_wrap = false;
        }
        if self.column + cells > width {
            self.row += 1;
            self.column = 0;
        }
        // One display scalar never exceeds two cells. Wide scalars wrap as
        // a unit instead of being split across the terminal edge.
        let cells = cells.min(width);
        self.column += cells;
        if self.column == width {
            self.column = width - 1;
            self.pending_wrap = true;
        }
    }

    fn newline(&mut self) {
        self.row += 1;
        self.column = 0;
        self.pending_wrap = false;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DisplayGeometry {
    rows: usize,
    cursor: DisplayPosition,
    end: DisplayPosition,
}

impl EditorDisplay {
    fn redraw(
        &mut self,
        prompt: &str,
        buf: &[char],
        cursor: usize,
        ghost: Option<String>,
        color: bool,
    ) {
        let ghost = color.then_some(ghost).flatten();
        // Refresh every redraw: SIGWINCH is not needed because raw reads
        // already wake at least every 100ms and the next key repaints using
        // the current PTY width.
        let terminal_width = super::Terminal::terminal_width();
        let continuation = dim("· ", color);
        let geometry = display_geometry(
            prompt,
            &continuation,
            buf,
            cursor,
            ghost.as_deref(),
            terminal_width,
        );
        if self.rows > 0 {
            print!("\r");
            if self.cursor_row > 0 {
                print!("\x1b[{}A", self.cursor_row);
            }
            print!("\x1b[J");
        }

        let typed: String = buf.iter().collect();
        for (row, line) in typed.split('\n').enumerate() {
            if row > 0 {
                print!("\r\n{}", continuation);
            } else {
                print!("{}", prompt);
            }
            print!("{}", highlight_input(line, color));
        }
        if cursor == buf.len() {
            if let Some(g) = ghost {
                print!("{}", dim(&g, color));
            }
        }

        if geometry.end.row > geometry.cursor.row {
            print!("\x1b[{}A", geometry.end.row - geometry.cursor.row);
        }
        print!("\r");
        if geometry.cursor.column > 0 {
            print!("\x1b[{}C", geometry.cursor.column);
        }
        io::stdout().flush().ok();
        self.rows = geometry.rows;
        self.cursor_row = geometry.cursor.row;
    }

    fn finish(&mut self) {
        if self.rows > 0 {
            let below = self.rows - 1 - self.cursor_row;
            if below > 0 {
                print!("\x1b[{}B", below);
            }
            print!("\r\n");
            io::stdout().flush().ok();
        }
        *self = Self::default();
    }
}

fn display_geometry(
    prompt: &str,
    continuation: &str,
    buf: &[char],
    cursor: usize,
    ghost: Option<&str>,
    terminal_width: usize,
) -> DisplayGeometry {
    let mut position = DisplayPosition::default();
    write_display_text(&mut position, prompt, terminal_width);
    let mut cursor_position = if cursor == 0 { Some(position) } else { None };
    for (index, ch) in buf.iter().enumerate() {
        if *ch == '\n' {
            position.newline();
            write_display_text(&mut position, continuation, terminal_width);
        } else {
            position.write_cells(
                jet_foundation::Diagnostics::display_char_width(*ch),
                terminal_width,
            );
        }
        if index + 1 == cursor {
            cursor_position = Some(position);
        }
    }
    if cursor == buf.len() {
        if let Some(ghost) = ghost {
            for ch in ghost.chars() {
                position.write_cells(
                    jet_foundation::Diagnostics::display_char_width(ch),
                    terminal_width,
                );
            }
        }
    }
    DisplayGeometry {
        rows: position.row + 1,
        cursor: cursor_position.unwrap_or(position),
        end: position,
    }
}

fn write_display_text(position: &mut DisplayPosition, text: &str, terminal_width: usize) {
    let mut escape = false;
    for ch in text.chars() {
        if escape {
            if ch.is_ascii_alphabetic() {
                escape = false;
            }
        } else if ch == '\x1b' {
            escape = true;
        } else {
            position.write_cells(
                jet_foundation::Diagnostics::display_char_width(ch),
                terminal_width,
            );
        }
    }
}

#[cfg(test)]
mod display_tests {
    use super::*;

    #[test]
    fn geometry_counts_wide_and_combining_cells_across_soft_wrap() {
        let buf: Vec<char> = "界界e\u{301}".chars().collect();
        let geometry = display_geometry("> ", "· ", &buf, 2, None, 6);
        assert_eq!(geometry.rows, 2);
        assert_eq!(geometry.cursor.row, 0);
        assert_eq!(geometry.cursor.column, 5);
        assert!(geometry.cursor.pending_wrap);
        assert_eq!(geometry.end.row, 1);
        assert_eq!(geometry.end.column, 1);
    }

    #[test]
    fn geometry_combines_explicit_newlines_and_soft_wraps() {
        let buf: Vec<char> = "abcd\n界x".chars().collect();
        let geometry = display_geometry("> ", "· ", &buf, buf.len(), None, 6);
        assert_eq!(geometry.rows, 2);
        assert_eq!(geometry.cursor.row, 1);
        assert_eq!(geometry.cursor.column, 5);
    }

    #[test]
    fn geometry_wraps_prompt_cells_at_narrow_width() {
        let geometry = display_geometry("1234", "· ", &[], 0, None, 3);
        assert_eq!(geometry.rows, 2);
        assert_eq!(geometry.cursor.row, 1);
        assert_eq!(geometry.cursor.column, 1);
    }

    #[test]
    fn completion_menu_has_textual_no_color_selection() {
        let candidates = vec![jet_semindex::SemanticSymbol {
            identity: "session:binding:alpha".to_string(),
            name: "alpha".to_string(),
            qualified_name: "alpha".to_string(),
            owner: None,
            module_path: "this session".to_string(),
            kind: jet_semindex::SemanticSymbolKind::Local,
            signature: "alpha :: <Int>".to_string(),
            summary: String::new(),
            examples: Vec::new(),
            provenance: jet_semindex::SemanticProvenance::Session,
            span: None,
            lexical_scope: None,
        }];
        let menu = completion_menu(&candidates, 0, false);
        assert!(menu.contains("> alpha"));
        assert!(!menu.contains("\x1b[7m"));
    }
}

/// Identifier (or `alias.member`) under / just before the cursor for F1 docs.
fn name_at_cursor(buf: &[char], cursor: usize) -> Option<String> {
    if buf.is_empty() {
        return None;
    }
    let mut i = if cursor < buf.len() {
        cursor
    } else {
        cursor.saturating_sub(1)
    };
    let is_name_char = |c: char| c.is_alphanumeric() || c == '_' || c == '.';
    if !is_name_char(buf[i]) {
        if i > 0 && is_name_char(buf[i - 1]) {
            i -= 1;
        } else {
            return None;
        }
    }
    let mut start = i;
    while start > 0 && is_name_char(buf[start - 1]) {
        start -= 1;
    }
    let mut end = i + 1;
    while end < buf.len() && is_name_char(buf[end]) {
        end += 1;
    }
    let name: String = buf[start..end].iter().collect();
    let name = name.trim_matches('.').to_string();
    (!name.is_empty()).then_some(name)
}

/// Tab completion over the same semantic facts as `?name`, LSP, and help.
/// Multiple matches open a selectable raw list; NO_COLOR keeps textual `>`
/// selection markers. Single matches insert immediately.
fn apply_completion<R: Read>(
    reader: &mut KeyReader<R>,
    buf: &mut Vec<char>,
    cursor: &mut usize,
    session: &Session,
    color: bool,
) -> bool {
    let text: String = buf[..*cursor].iter().collect();
    let (replace_start, candidates) = if let Some(dot) = text.rfind('.') {
        let receiver = &text[..dot];
        let partial = &text[dot + 1..];
        let is_ident = !receiver.is_empty()
            && receiver.chars().all(|c| c.is_alphanumeric() || c == '_');
        let candidates = if is_ident {
            Docs::dotted_completion_candidates(session, receiver, partial)
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
            (start, Docs::completion_candidates(session, partial, None))
        }
    };

    if candidates.is_empty() {
        return false;
    }
    if candidates.len() == 1 {
        insert_completion(buf, cursor, replace_start, &candidates[0].name);
        return false;
    }
    let names = candidates.iter().map(|symbol| symbol.name.clone()).collect::<Vec<_>>();
    let common = longest_common_prefix(&names);
    let already: String = buf[replace_start..*cursor].iter().collect();
    if common.len() > already.len() {
        insert_completion(buf, cursor, replace_start, &common);
    }
    print!("\r\n");
    if let Some(selected) = select_completion(reader, &candidates, color) {
        insert_completion(buf, cursor, replace_start, &selected.name);
    }
    true
}

fn completion_menu(candidates: &[jet_semindex::SemanticSymbol], selected: usize, color: bool) -> String {
    let theme = Theme::new(color);
    let mut out = format!(
        "{}\r\n",
        theme.accent("completion — ↑↓ move · Tab next · Enter insert · Esc close")
    );
    for (index, symbol) in candidates.iter().enumerate() {
        let marker = if index == selected { ">" } else { " " };
        let line = format!("{marker} {:<20} {}", symbol.qualified_name, symbol.signature);
        if index == selected && color {
            out.push_str(&format!("{}\r\n", theme.invert(&line)));
        } else {
            out.push_str(&line);
            out.push_str("\r\n");
        }
    }
    out
}

fn select_completion<R: Read>(
    reader: &mut KeyReader<R>,
    candidates: &[jet_semindex::SemanticSymbol],
    color: bool,
) -> Option<jet_semindex::SemanticSymbol> {
    let mut selected = 0usize;
    let rows = candidates.len() + 1;
    loop {
        print!("{}", completion_menu(candidates, selected, color));
        io::stdout().flush().ok();
        match reader.read_key() {
            Key::Up => selected = (selected + candidates.len() - 1) % candidates.len(),
            Key::Down | Key::Tab => selected = (selected + 1) % candidates.len(),
            Key::Enter => return Some(candidates[selected].clone()),
            Key::Escape | Key::CtrlC | Key::Eof => return None,
            Key::Idle => continue,
            _ => {}
        }
        print!("\x1b[{rows}A\r\x1b[J");
    }
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
        let mut index = 0;
        while index < prefix.len() && index < chars.len() && prefix[index] == chars[index] {
            index += 1;
        }
        prefix.truncate(index);
    }
    prefix.into_iter().collect()
}
