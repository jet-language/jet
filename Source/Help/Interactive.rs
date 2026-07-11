//! The raw-mode `jet ?` event loop. Built on the shared `crate::Term`
//! raw-mode module (I8 — the same one the hybrid REPL uses); all *decisions*
//! about what a frame looks like live in `Render` (pure, unit-tested) — this
//! module only reads keys, tracks selection state, and redraws.
//!
//! Output discipline (ratified "prefilled, never runs"): every interactive
//! frame goes to **stderr**. The **only** thing this module ever writes to
//! stdout is the chosen command line at Enter — so `$(jet ?)` composes
//! cleanly, and everything else (the whole palette UI) stays out of a
//! captured pipe. After exit, the same line is echoed to stderr again as a
//! ready-to-copy line for a human reading the terminal.

use std::io::{self, IsTerminal, Write};

use crate::Term::{Key, KeyReader, RawGuard};

use super::{build_index, search, Entry, Hit};
use super::Render;

enum Mode {
    /// Empty query: categorized command list.
    Categorized,
    /// Typing: fuzzy results over the whole index.
    Results,
    /// Tab: man-depth detail for one entry, inline.
    Detail,
    /// F1: two-pane reference, alt-screen.
    Reference,
}

/// RAII: enters the terminal alt-screen buffer on construction, restores the
/// normal screen on drop (including on panic — this crate doesn't set
/// `panic = "abort"`, so unwinding still runs `Drop`), mirroring `RawGuard`.
struct AltScreen;

impl AltScreen {
    fn enter() -> Self {
        eprint!("\x1b[?1049h");
        io::stderr().flush().ok();
        AltScreen
    }
}

impl Drop for AltScreen {
    fn drop(&mut self) {
        eprint!("\x1b[?1049l");
        io::stderr().flush().ok();
    }
}

/// Redraws `frame` in place: clears the previous frame (by line count) and
/// prints the new one. All writes are `\r\n`-terminated (raw mode doesn't
/// translate `\n`) and go to stderr (see module docs). Returns the new
/// frame's line count, to feed back in as `prev_lines` next call.
fn redraw(prev_lines: usize, frame: &str) -> usize {
    if prev_lines > 0 {
        eprint!("\x1b[{}A\r\x1b[J", prev_lines);
    }
    for line in frame.lines() {
        eprint!("{}\r\n", line);
    }
    io::stderr().flush().ok();
    frame.lines().count()
}

/// `jet ?` with no query on a TTY — the full interactive app. Falls back to
/// a static categorized print (still on the real stdout floor via `mod.rs`
/// dispatch for the true non-TTY case; this fallback only covers the rare
/// "is a TTY but `stty` unavailable" edge, matching `RawGuard`'s contract).
pub fn run(color: bool) -> io::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        print!("{}", super::Render::render_categorized(&build_index(), 0, false, None, 72, color));
        println!();
        return Ok(());
    }
    let Some(_guard) = RawGuard::enable() else {
        eprint!("{}", Render::render_categorized(&build_index(), 0, false, None, 72, color));
        eprintln!();
        return Ok(());
    };

    let index = build_index();
    let (width, height) = terminal_size();
    let stdin = io::stdin();
    let mut reader = KeyReader::new(stdin.lock());

    let mut query = String::new();
    let mut mode = Mode::Categorized;
    let mut cat_selected: usize = 0; // index into `CATEGORIES`
    let mut cat_entry: Option<usize> = None; // index into entries in selected category
    let mut hits: Vec<Hit> = Vec::new();
    let mut res_selected: usize = 0;
    let mut ref_category: usize = 0;
    let mut ref_entry: Option<usize> = None; // index into `index`, filtered to ref_category
    let mut ref_query = String::new();
    let mut alt: Option<AltScreen> = None;
    let mut prev_lines = 0usize;

    let mut frame = Render::render_categorized(&index, cat_selected, cat_entry.is_some(), selected_category_cmd(&index, cat_selected, cat_entry), width, color);
    prev_lines = redraw(prev_lines, &frame);

    loop {
        match reader.read_key() {
            Key::Idle => continue,
            Key::Eof | Key::CtrlC => {
                quit_without_prefill(prev_lines);
                return Ok(());
            }
            Key::Escape => match mode {
                Mode::Detail => {
                    mode = Mode::Results;
                }
                Mode::Reference => {
                    alt = None; // Drop restores the normal screen.
                    mode = if query.is_empty() { Mode::Categorized } else { Mode::Results };
                    prev_lines = 0;
                }
                Mode::Categorized | Mode::Results => {
                    quit_without_prefill(prev_lines);
                    return Ok(());
                }
            },
            Key::F1 => {
                if !matches!(mode, Mode::Reference) {
                    alt = Some(AltScreen::enter());
                    mode = Mode::Reference;
                    ref_category = 0;
                    ref_entry = selected_category_index(&index, cat_selected, cat_entry)
                        .or_else(|| entries_in_category(&index, cat_selected).first().copied());
                    ref_query.clear();
                    prev_lines = 0;
                }
            }
            Key::Tab => match mode {
                Mode::Categorized if cat_entry.is_some() => mode = Mode::Detail,
                Mode::Results if !hits.is_empty() => mode = Mode::Detail,
                Mode::Detail => mode = if query.is_empty() { Mode::Categorized } else { Mode::Results },
                _ => {}
            },
            Key::Backspace if matches!(mode, Mode::Reference) => {
                ref_query.pop();
                apply_reference_search(&index, &ref_query, &mut ref_category, &mut ref_entry);
            }
            Key::Backspace => {
                query.pop();
                if query.is_empty() {
                    mode = Mode::Categorized;
                    hits.clear();
                } else {
                    hits = search(&index, &query);
                    res_selected = 0;
                    mode = Mode::Results;
                }
            }
            Key::Char(c) if matches!(mode, Mode::Reference) => {
                ref_query.push(c);
                apply_reference_search(&index, &ref_query, &mut ref_category, &mut ref_entry);
            }
            Key::Char('q') if query.is_empty() && matches!(mode, Mode::Categorized) => {
                quit_without_prefill(prev_lines);
                return Ok(());
            }
            Key::Char(c) => {
                query.push(c);
                hits = search(&index, &query);
                res_selected = 0;
                mode = Mode::Results;
            }
            Key::Up => move_selection(&mut mode, &mut cat_selected, &mut cat_entry, &hits, &mut res_selected, &index, &mut ref_category, &mut ref_entry, -1),
            Key::Down => move_selection(&mut mode, &mut cat_selected, &mut cat_entry, &hits, &mut res_selected, &index, &mut ref_category, &mut ref_entry, 1),
            Key::Left if matches!(mode, Mode::Categorized) => cat_entry = None,
            Key::Right if matches!(mode, Mode::Categorized) => cat_entry = Some(0),
            Key::Left if matches!(mode, Mode::Reference) => {
                ref_entry = None;
            }
            Key::Right if matches!(mode, Mode::Reference) => {
                let cat = super::CATEGORIES[ref_category];
                if let Some(e) = index.iter().find(|e| e.category == cat) {
                    ref_entry = index.iter().position(|x| x.cmd == e.cmd);
                }
            }
            Key::Enter => {
                if matches!(mode, Mode::Categorized) && cat_entry.is_none() {
                    cat_entry = Some(0);
                    frame = render_current(&mode, &index, cat_selected, cat_entry, &query, &hits, res_selected, ref_category, ref_entry, &ref_query, width, height, color);
                    prev_lines = redraw(prev_lines, &frame);
                    continue;
                }
                if matches!(mode, Mode::Categorized)
                    && selected_category_index(&index, cat_selected, cat_entry).is_none()
                {
                    continue;
                }
                let command = current_command(&mode, &index, cat_selected, cat_entry, &hits, res_selected, ref_entry);
                drop(alt);
                drop(_guard);
                if let Some(cmd) = command {
                    // Ratified "prefilled, never runs": the command is the
                    // ONLY thing this app ever writes to stdout, so
                    // `$(jet ?)` composes; the human-readable copy lands on
                    // stderr after the terminal is restored.
                    println!("{}", cmd);
                    eprintln!();
                    eprintln!("ready to copy:");
                    eprintln!("  {}", cmd);
                }
                return Ok(());
            }
            _ => {}
        }

        frame = render_current(&mode, &index, cat_selected, cat_entry, &query, &hits, res_selected, ref_category, ref_entry, &ref_query, width, height, color);
        prev_lines = redraw(prev_lines, &frame);
    }
}

fn quit_without_prefill(prev_lines: usize) {
    if prev_lines > 0 {
        eprint!("\x1b[{}A\r\x1b[J", prev_lines);
    }
    io::stderr().flush().ok();
}

#[allow(clippy::too_many_arguments)]
fn move_selection(
    mode: &mut Mode,
    cat_selected: &mut usize,
    cat_entry: &mut Option<usize>,
    hits: &[Hit],
    res_selected: &mut usize,
    index: &[Entry],
    ref_category: &mut usize,
    ref_entry: &mut Option<usize>,
    delta: i64,
) {
    match mode {
        Mode::Categorized => {
            if let Some(entry) = cat_entry {
                let count = entries_in_category(index, *cat_selected).len();
                *entry = step(*entry, count, delta);
            } else {
                *cat_selected = step(*cat_selected, super::CATEGORIES.len(), delta);
            }
        }
        Mode::Results => {
            if !hits.is_empty() {
                *res_selected = step(*res_selected, hits.len(), delta);
            }
        }
        Mode::Reference => {
            if ref_entry.is_some() {
                let cat = super::CATEGORIES[*ref_category];
                let in_cat: Vec<usize> = index
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.category == cat)
                    .map(|(i, _)| i)
                    .collect();
                if let Some(pos) = ref_entry.and_then(|cur| in_cat.iter().position(|&i| i == cur)) {
                    let next = step(pos, in_cat.len(), delta);
                    *ref_entry = Some(in_cat[next]);
                }
            } else {
                *ref_category = step(*ref_category, super::CATEGORIES.len(), delta);
            }
        }
        Mode::Detail => {}
    }
}

fn step(cur: usize, len: usize, delta: i64) -> usize {
    if len == 0 {
        return 0;
    }
    let len = len as i64;
    (((cur as i64 + delta) % len + len) % len) as usize
}

#[allow(clippy::too_many_arguments)]
fn render_current(
    mode: &Mode,
    index: &[Entry],
    cat_selected: usize,
    cat_entry: Option<usize>,
    query: &str,
    hits: &[Hit],
    res_selected: usize,
    ref_category: usize,
    ref_entry: Option<usize>,
    ref_query: &str,
    width: usize,
    height: usize,
    color: bool,
) -> String {
    match mode {
        Mode::Categorized => Render::render_categorized(index, cat_selected, cat_entry.is_some(), selected_category_cmd(index, cat_selected, cat_entry), width, color),
        Mode::Results => Render::render_result_list(hits, query, width, color, Some(res_selected)),
        Mode::Detail => {
            let entry = current_entry(cat_selected, cat_entry, hits, res_selected, index);
            match entry {
                Some(e) => Render::render_detail(e, width, color),
                None => Render::render_result_list(hits, query, width, color, Some(res_selected)),
            }
        }
        Mode::Reference => {
            if let Some(Hit::Code(ex)) = search(index, ref_query).first() {
                return format!("search: {}\n{}", ref_query, Render::render_code_page(ex, width, color));
            }
            let sel = ref_entry.map(|i| &index[i]);
            let mut rendered = Render::render_reference(index, ref_category, sel, width, height, color);
            if !ref_query.is_empty() {
                rendered = format!("search: {}\n{}", ref_query, rendered);
            }
            rendered
        }
    }
}

fn current_entry<'a>(
    cat_selected: usize,
    cat_entry: Option<usize>,
    hits: &[Hit],
    res_selected: usize,
    index: &'a [Entry],
) -> Option<&'a Entry> {
    if let Some(Hit::Command { entry, .. }) = hits.get(res_selected) {
        return index.iter().find(|e| e.cmd == entry.cmd);
    }
    selected_category_index(index, cat_selected, cat_entry).map(|i| &index[i])
}

#[allow(clippy::too_many_arguments)]
fn current_command(
    mode: &Mode,
    index: &[Entry],
    cat_selected: usize,
    cat_entry: Option<usize>,
    hits: &[Hit],
    res_selected: usize,
    ref_entry: Option<usize>,
) -> Option<String> {
    match mode {
        Mode::Reference => ref_entry.map(|i| prefill_for(&index[i])),
        Mode::Results | Mode::Detail => match hits.get(res_selected) {
            Some(Hit::Command { entry, .. }) => Some(prefill_for(entry)),
            Some(Hit::Code(_)) => None,
            None => None,
        },
        Mode::Categorized => selected_category_index(index, cat_selected, cat_entry).map(|i| prefill_for(&index[i])),
    }
}

fn entries_in_category(index: &[Entry], category: usize) -> Vec<usize> {
    let Some(category) = super::CATEGORIES.get(category) else { return Vec::new() };
    index.iter().enumerate().filter(|(_, e)| &e.category == category).map(|(i, _)| i).collect()
}

fn selected_category_index(index: &[Entry], category: usize, entry: Option<usize>) -> Option<usize> {
    entry.and_then(|entry| entries_in_category(index, category).get(entry).copied())
}

fn selected_category_cmd(index: &[Entry], category: usize, entry: Option<usize>) -> Option<&str> {
    selected_category_index(index, category, entry).map(|i| index[i].cmd)
}

fn apply_reference_search(index: &[Entry], query: &str, category: &mut usize, entry: &mut Option<usize>) {
    if query.is_empty() { return; }
    if let Some(Hit::Command { entry: found, .. }) = search(index, query).first() {
        if let Some(ci) = super::CATEGORIES.iter().position(|c| *c == found.category) {
            *category = ci;
        }
        *entry = index.iter().position(|e| e.cmd == found.cmd);
    }
}

fn terminal_size() -> (usize, usize) {
    let size = std::process::Command::new("stty")
        .arg("size")
        .stdin(std::process::Stdio::inherit())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let text = String::from_utf8_lossy(&o.stdout);
            let mut parts = text.split_whitespace();
            Some((parts.next()?.parse::<usize>().ok()?, parts.next()?.parse::<usize>().ok()?))
        });
    let (rows, cols) = size.unwrap_or((24, crate::Term::terminal_width()));
    (cols.clamp(50, 160), rows.max(8))
}

fn prefill_for(entry: &Entry) -> String {
    entry.example.clone().unwrap_or_else(|| format!("jet {}", entry.cmd))
}
