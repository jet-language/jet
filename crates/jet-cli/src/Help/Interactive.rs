//! The raw-mode `jet ?` event loop. Built on the shared `crate::Term`
//! raw-mode module (I8 — the same one the hybrid REPL uses); all *decisions*
//! about what a frame looks like live in `Render` (pure, unit-tested) — this
//! module only reads keys, tracks selection state, and redraws.
//!
//! Output discipline (ratified "prefilled, never runs"): every interactive
//! frame goes to **stderr**. The **only** thing this module ever writes to
//! stdout is the chosen command line at Enter — so `$(jet ?)` composes
//! cleanly, and everything else (the whole palette UI) stays out of a
//! captured pipe. Shell integration places that captured line into the next
//! editable prompt without executing it.

use std::io::{self, IsTerminal, Write};

use crate::Term::{Key, KeyReader, RawGuard};

use super::Render;
use super::{build_index, search, Entry, Hit};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Empty query: categorized command list.
    Categorized,
    /// Typing: fuzzy results over the whole index.
    Results,
    /// Tab: man-depth detail for one entry, inline.
    Detail,
    /// F1: two-pane reference, alt-screen.
    Reference,
}

#[derive(Debug)]
pub(crate) enum Action {
    Continue,
    Quit,
    EnterReference,
    LeaveReference,
    Submit(Option<String>),
}

/// Pure interactive state shared by the real raw-mode loop and the headless
/// driver. Rendering stays in `Render`; this type only owns focus, input, and
/// selection transitions.
pub(crate) struct State {
    index: Vec<Entry>,
    width: usize,
    height: usize,
    color: bool,
    mode: Mode,
    query: String,
    cat_selected: usize,
    cat_entry: Option<usize>,
    hits: Vec<Hit>,
    res_selected: usize,
    res_scroll: usize,
    ref_category: usize,
    ref_entry: Option<usize>,
    ref_query: String,
    ref_scroll: usize,
}

impl State {
    pub(crate) fn new(width: usize, height: usize, color: bool) -> Self {
        Self {
            index: build_index(),
            width: width.max(1),
            height: height.max(1),
            color,
            mode: Mode::Categorized,
            query: String::new(),
            cat_selected: 0,
            cat_entry: None,
            hits: Vec::new(),
            res_selected: 0,
            res_scroll: 0,
            ref_category: 0,
            ref_entry: None,
            ref_query: String::new(),
            ref_scroll: 0,
        }
    }

    pub(crate) fn width(&self) -> usize {
        self.width
    }

    pub(crate) fn height(&self) -> usize {
        self.height
    }

    pub(crate) fn resize(&mut self, width: usize, height: usize) {
        self.width = width.max(1);
        self.height = height.max(1);
    }

    pub(crate) fn render(&self) -> String {
        render_current(
            &self.mode,
            &self.index,
            self.cat_selected,
            self.cat_entry,
            &self.query,
            &self.hits,
            self.res_selected,
            self.res_scroll,
            self.ref_category,
            self.ref_entry,
            &self.ref_query,
            self.ref_scroll,
            self.width,
            self.height,
            self.color,
        )
    }

    pub(crate) fn apply_key(&mut self, key: Key) -> Action {
        match key {
            Key::Idle => Action::Continue,
            Key::Eof | Key::CtrlC => Action::Quit,
            Key::Escape => match self.mode {
                Mode::Detail => {
                    self.mode = if self.query.is_empty() {
                        Mode::Categorized
                    } else {
                        Mode::Results
                    };
                    self.res_scroll = 0;
                    Action::Continue
                }
                Mode::Reference => {
                    self.mode = if self.query.is_empty() {
                        Mode::Categorized
                    } else {
                        Mode::Results
                    };
                    self.ref_scroll = 0;
                    Action::LeaveReference
                }
                Mode::Categorized | Mode::Results => Action::Quit,
            },
            Key::F1 => {
                if !matches!(self.mode, Mode::Reference) {
                    self.mode = Mode::Reference;
                    self.ref_category = 0;
                    self.ref_entry = selected_category_index(
                        &self.index,
                        self.cat_selected,
                        self.cat_entry,
                    )
                    .or_else(|| entries_in_category(&self.index, self.cat_selected).first().copied());
                    self.ref_query.clear();
                    self.ref_scroll = 0;
                    Action::EnterReference
                } else {
                    Action::Continue
                }
            }
            Key::Tab => {
                match self.mode {
                    Mode::Categorized if self.cat_entry.is_some() => {
                        self.mode = Mode::Detail;
                        self.res_scroll = 0;
                    }
                    Mode::Results if !self.hits.is_empty() => {
                        self.mode = Mode::Detail;
                        self.res_scroll = 0;
                    }
                    Mode::Detail => {
                        self.mode = if self.query.is_empty() {
                            Mode::Categorized
                        } else {
                            Mode::Results
                        };
                    }
                    _ => {}
                }
                Action::Continue
            }
            Key::Backspace if matches!(self.mode, Mode::Reference) => {
                self.ref_query.pop();
                self.ref_scroll = 0;
                apply_reference_search(
                    &self.index,
                    &self.ref_query,
                    &mut self.ref_category,
                    &mut self.ref_entry,
                );
                Action::Continue
            }
            Key::Backspace => {
                self.query.pop();
                if self.query.is_empty() {
                    self.mode = Mode::Categorized;
                    self.hits.clear();
                } else {
                    self.hits = search(&self.index, &self.query);
                    self.res_selected = 0;
                    self.res_scroll = 0;
                    self.mode = Mode::Results;
                }
                Action::Continue
            }
            Key::Char(c) if matches!(self.mode, Mode::Reference) => {
                self.ref_query.push(c);
                self.ref_scroll = 0;
                apply_reference_search(
                    &self.index,
                    &self.ref_query,
                    &mut self.ref_category,
                    &mut self.ref_entry,
                );
                Action::Continue
            }
            Key::Char('q') if self.query.is_empty() && matches!(self.mode, Mode::Categorized) => {
                Action::Quit
            }
            Key::Char(c) => {
                self.query.push(c);
                self.hits = search(&self.index, &self.query);
                self.res_selected = 0;
                self.res_scroll = 0;
                self.mode = Mode::Results;
                Action::Continue
            }
            Key::Up
                if matches!(self.mode, Mode::Reference)
                    && reference_code(&self.index, &self.ref_query).is_some() =>
            {
                self.ref_scroll = self.ref_scroll.saturating_sub(1);
                Action::Continue
            }
            Key::Down
                if matches!(self.mode, Mode::Reference)
                    && reference_code(&self.index, &self.ref_query).is_some() =>
            {
                self.ref_scroll = self.ref_scroll.saturating_add(1);
                Action::Continue
            }
            Key::Up
                if matches!(self.mode, Mode::Results | Mode::Detail)
                    && result_code(&self.hits, self.res_selected).is_some() =>
            {
                self.res_scroll = self.res_scroll.saturating_sub(1);
                Action::Continue
            }
            Key::Down
                if matches!(self.mode, Mode::Results | Mode::Detail)
                    && result_code(&self.hits, self.res_selected).is_some() =>
            {
                self.res_scroll = self.res_scroll.saturating_add(1);
                Action::Continue
            }
            Key::Up if matches!(self.mode, Mode::Detail) => {
                self.res_scroll = self.res_scroll.saturating_sub(1);
                Action::Continue
            }
            Key::Down if matches!(self.mode, Mode::Detail) => {
                self.res_scroll = self.res_scroll.saturating_add(1);
                Action::Continue
            }
            Key::Up => {
                move_selection(
                    &mut self.mode,
                    &mut self.cat_selected,
                    &mut self.cat_entry,
                    &self.hits,
                    &mut self.res_selected,
                    &self.index,
                    &mut self.ref_category,
                    &mut self.ref_entry,
                    -1,
                );
                Action::Continue
            }
            Key::Down => {
                move_selection(
                    &mut self.mode,
                    &mut self.cat_selected,
                    &mut self.cat_entry,
                    &self.hits,
                    &mut self.res_selected,
                    &self.index,
                    &mut self.ref_category,
                    &mut self.ref_entry,
                    1,
                );
                Action::Continue
            }
            Key::Left if matches!(self.mode, Mode::Categorized) => {
                self.cat_entry = None;
                Action::Continue
            }
            Key::Right if matches!(self.mode, Mode::Categorized) => {
                self.cat_entry = Some(0);
                Action::Continue
            }
            Key::Left if matches!(self.mode, Mode::Reference) => {
                self.ref_entry = None;
                Action::Continue
            }
            Key::Right if matches!(self.mode, Mode::Reference) => {
                let cat = super::CATEGORIES[self.ref_category];
                if let Some(e) = self.index.iter().find(|e| e.category == cat) {
                    self.ref_entry = self
                        .index
                        .iter()
                        .position(|x| x.symbol.identity == e.symbol.identity);
                }
                Action::Continue
            }
            key @ (Key::Enter | Key::EscapeEnter) => {
                let want_example = matches!(key, Key::EscapeEnter);
                if !want_example
                    && matches!(self.mode, Mode::Categorized)
                    && self.cat_entry.is_none()
                {
                    self.cat_entry = Some(0);
                    return Action::Continue;
                }
                if matches!(self.mode, Mode::Categorized)
                    && selected_category_index(&self.index, self.cat_selected, self.cat_entry)
                        .is_none()
                {
                    return Action::Continue;
                }
                Action::Submit(current_prefill(
                    &self.mode,
                    &self.index,
                    self.cat_selected,
                    self.cat_entry,
                    &self.hits,
                    self.res_selected,
                    self.ref_entry,
                    want_example,
                ))
            }
            _ => Action::Continue,
        }
    }

    /// Apply a click through a semantic hit map.
    ///
    /// The map is derived from selection state and the renderer's documented
    /// row geometry.  It never scans the rendered frame, so ANSI/style changes
    /// cannot change which command a click selects.
    pub(crate) fn click(&mut self, column: usize, row: usize) -> Action {
        if column >= self.width || row >= self.height {
            return Action::Continue;
        }
        let Some(element) = self
            .hit_map()
            .into_iter()
            .find(|region| region.contains(column, row))
            .map(|region| region.element)
        else {
            return Action::Continue;
        };
        match element {
            ElementId::Category(category) => {
                if matches!(self.mode, Mode::Categorized) {
                    self.cat_selected = category;
                    self.cat_entry = None;
                }
            }
            ElementId::Command(command_index) => {
                if matches!(self.mode, Mode::Categorized) {
                    self.select_categorized_command(command_index);
                }
            }
            ElementId::Result(hit) => {
                if matches!(self.mode, Mode::Results) && hit < self.hits.len() {
                    self.res_selected = hit;
                }
            }
            ElementId::ReferenceCategory(category) => {
                if matches!(self.mode, Mode::Reference) {
                    self.ref_category = category;
                    self.ref_entry = None;
                }
            }
            ElementId::ReferenceCommand(command_index) => {
                if matches!(self.mode, Mode::Reference)
                    && self.index.get(command_index).is_some()
                {
                    self.ref_entry = Some(command_index);
                    self.ref_category = self.category_index(command_index).unwrap_or(self.ref_category);
                }
            }
        }
        Action::Continue
    }

    pub(crate) fn wait(&mut self) {}

    /// Return the canonical name of the target that currently owns focus.
    pub(crate) fn focus_name(&self) -> Option<String> {
        match self.mode {
            Mode::Categorized => self
                .selected_category_index()
                .map(|index| self.index[index].symbol.name.clone())
                .or_else(|| {
                    super::CATEGORIES
                        .get(self.cat_selected)
                        .map(|name| (*name).to_string())
                }),
            Mode::Results | Mode::Detail => match self.hits.get(self.res_selected) {
                Some(Hit::Command { entry, .. }) => Some(entry.symbol.name.clone()),
                Some(Hit::Code(explanation)) => Some(explanation.code.clone()),
                None => None,
            },
            Mode::Reference => self
                .ref_entry
                .and_then(|index| self.index.get(index))
                .map(|entry| entry.symbol.name.clone())
                .or_else(|| {
                    super::CATEGORIES
                        .get(self.ref_category)
                        .map(|name| (*name).to_string())
                }),
        }
    }

    /// Return the canonical semantic readiness state used by headless waits.
    ///
    /// Readiness is deliberately limited to state-owned names.  In
    /// particular, this does not accept arbitrary substrings from a rendered
    /// frame.
    pub(crate) fn ready(&self, marker: &str) -> bool {
        if marker.is_empty() {
            return false;
        }
        let mode_ready = match marker {
            "categorized" => matches!(self.mode, Mode::Categorized),
            "results" => matches!(self.mode, Mode::Results),
            "detail" => matches!(self.mode, Mode::Detail),
            "reference" => matches!(self.mode, Mode::Reference),
            _ => false,
        };
        if mode_ready {
            return true;
        }
        let Some(focus) = self.focus_name() else {
            return false;
        };
        marker == focus || marker.strip_prefix("jet ") == Some(focus.as_str())
    }

    /// Return the semantic hit target at a viewport coordinate.
    ///
    /// This is kept as a typed map rather than inferred from terminal text.
    fn hit_map(&self) -> Vec<HitRegion> {
        match self.mode {
            Mode::Categorized => self.categorized_hit_map(),
            Mode::Results => self.result_hit_map(),
            Mode::Detail => Vec::new(),
            Mode::Reference => self.reference_hit_map(),
        }
    }

    fn categorized_hit_map(&self) -> Vec<HitRegion> {
        let rows = self.categorized_elements();
        if rows.is_empty() {
            return Vec::new();
        }
        let selected_header = rows
            .iter()
            .position(|element| *element == Some(ElementId::Category(self.cat_selected)));
        let selected = if self.cat_entry.is_some() {
            self.selected_category_index()
                .and_then(|command_index| {
                    rows.iter()
                        .position(|element| *element == Some(ElementId::Command(command_index)))
                })
                .or(selected_header)
        } else {
            selected_header
        }
        .unwrap_or(0);
        let visible_rows = self.height.saturating_sub(4).max(1);
        let start = selected
            .saturating_sub(visible_rows.saturating_sub(1))
            .min(rows.len().saturating_sub(visible_rows));
        rows.into_iter()
            .enumerate()
            .skip(start)
            .take(visible_rows)
            .filter_map(|(body_row, element)| {
                element.map(|element| HitRegion::all_columns(
                    3 + body_row - start,
                    self.width,
                    element,
                ))
            })
            .collect()
    }

    fn result_hit_map(&self) -> Vec<HitRegion> {
        let Some((start, visible_hits)) = self.result_viewport() else {
            return Vec::new();
        };
        self.hits
            .iter()
            .enumerate()
            .skip(start)
            .take(visible_hits)
            .enumerate()
            .map(|(offset, (hit, _))| {
                HitRegion::all_columns(3 + offset, self.width, ElementId::Result(hit))
            })
            .collect()
    }

    fn reference_hit_map(&self) -> Vec<HitRegion> {
        let rows = self.reference_elements();
        let height = self.height.max(3);
        let body_rows = height.saturating_sub(2);
        let selected = self
            .ref_entry
            .and_then(|command_index| {
                rows.iter()
                    .position(|element| *element == ElementId::ReferenceCommand(command_index))
            });
        let left_start = selected
            .map(|row| row.saturating_sub(body_rows.saturating_sub(1)))
            .unwrap_or(0)
            .min(rows.len().saturating_sub(body_rows));
        let left_width = (self.width.max(24) / 3).max(18);
        (0..body_rows)
            .filter_map(|offset| {
                rows.get(left_start + offset).copied().map(|element| {
                    HitRegion::new(1 + offset, 0, left_width, element)
                })
            })
            .collect()
    }

    fn categorized_elements(&self) -> Vec<Option<ElementId>> {
        let mut rows = Vec::new();
        for (category, name) in super::CATEGORIES.iter().enumerate() {
            let entries: Vec<usize> = self
                .index
                .iter()
                .enumerate()
                .filter(|(_, entry)| &entry.category == name)
                .map(|(index, _)| index)
                .collect();
            if entries.is_empty() && *name != "Error Codes" {
                continue;
            }
            rows.push(Some(ElementId::Category(category)));
            if category == self.cat_selected && self.cat_entry.is_some() {
                if *name == "Error Codes" {
                    rows.push(None);
                }
                rows.extend(entries.into_iter().map(|index| Some(ElementId::Command(index))));
            }
        }
        rows
    }

    fn reference_elements(&self) -> Vec<ElementId> {
        let mut rows = Vec::new();
        for (category, name) in super::CATEGORIES.iter().enumerate() {
            rows.push(ElementId::ReferenceCategory(category));
            if category == self.ref_category {
                rows.extend(
                    self.index
                        .iter()
                        .enumerate()
                        .filter(|(_, entry)| &entry.category == name)
                        .map(|(index, _)| ElementId::ReferenceCommand(index)),
                );
            }
        }
        rows
    }

    fn result_viewport(&self) -> Option<(usize, usize)> {
        if self.hits.is_empty() || result_code(&self.hits, self.res_selected).is_some() {
            return None;
        }
        let show_footer = self.height >= 6;
        let show_lower_rule = self.height >= 7;
        let show_example = self
            .hits
            .first()
            .and_then(|hit| match hit {
                Hit::Command { entry, .. } => entry.symbol.examples.first(),
                Hit::Code(_) => None,
            })
            .is_some()
            && self.height >= 8;
        let fixed_rows =
            4 + usize::from(show_footer) + usize::from(show_lower_rule) + usize::from(show_example);
        let visible_hits = self.height.saturating_sub(fixed_rows).max(1);
        let selected = self.res_selected.min(self.hits.len().saturating_sub(1));
        let start = selected
            .saturating_sub(visible_hits.saturating_sub(1))
            .min(self.hits.len().saturating_sub(visible_hits));
        Some((start, visible_hits))
    }

    fn selected_category_index(&self) -> Option<usize> {
        selected_category_index(&self.index, self.cat_selected, self.cat_entry)
    }

    fn category_index(&self, command_index: usize) -> Option<usize> {
        let category = self.index.get(command_index)?.category;
        super::CATEGORIES
            .iter()
            .position(|candidate| *candidate == category)
    }

    fn select_categorized_command(&mut self, command_index: usize) {
        let Some(category) = self.category_index(command_index) else {
            return;
        };
        self.cat_selected = category;
        self.cat_entry = entries_in_category(&self.index, category)
            .iter()
            .position(|&index| index == command_index);
    }

    pub(crate) fn cursor(&self) -> Option<(usize, usize)> {
        let target = match self.mode {
            Mode::Categorized => self
                .selected_category_index()
                .map(ElementId::Command)
                .or(Some(ElementId::Category(self.cat_selected))),
            Mode::Results if result_code(&self.hits, self.res_selected).is_none() => {
                Some(ElementId::Result(self.res_selected))
            }
            Mode::Reference => self
                .ref_entry
                .map(ElementId::ReferenceCommand)
                .or(Some(ElementId::ReferenceCategory(self.ref_category))),
            Mode::Results | Mode::Detail => None,
        }?;
        self.hit_map()
            .into_iter()
            .find(|region| region.element == target)
            .map(|region| (region.column_start, region.row))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ElementId {
    Category(usize),
    Command(usize),
    Result(usize),
    ReferenceCategory(usize),
    ReferenceCommand(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HitRegion {
    row: usize,
    column_start: usize,
    column_end: usize,
    element: ElementId,
}

impl HitRegion {
    const fn new(row: usize, column_start: usize, column_end: usize, element: ElementId) -> Self {
        Self {
            row,
            column_start,
            column_end,
            element,
        }
    }

    const fn all_columns(row: usize, width: usize, element: ElementId) -> Self {
        Self::new(row, 0, width, element)
    }

    const fn contains(self, column: usize, row: usize) -> bool {
        self.row == row && column >= self.column_start && column < self.column_end
    }
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
fn redraw(prev_lines: usize, frame: &str, height: usize) -> usize {
    let visible_prev = prev_lines.min(height.max(1));
    if visible_prev > 0 {
        eprint!("\r");
        if visible_prev > 1 {
            eprint!("\x1b[{}A", visible_prev - 1);
        }
        eprint!("\x1b[J");
    }
    let mut lines = frame.lines().peekable();
    while let Some(line) = lines.next() {
        eprint!("{}", line);
        if lines.peek().is_some() {
            eprint!("\r\n");
        }
    }
    io::stderr().flush().ok();
    frame.lines().count()
}

/// `jet ?` with no query on a TTY — the full interactive app. Falls back to
/// a static categorized print (still on the real stdout floor via `mod.rs`
/// dispatch for the true non-TTY case; this fallback only covers the rare
/// "is a TTY but `stty` unavailable" edge, matching `RawGuard`'s contract).
pub fn run(color: bool) -> io::Result<()> {
    let shell_prefill = std::env::var_os("JET_HELP_SHELL_PREFILL").is_some();
    if !io::stdin().is_terminal()
        || (!io::stdout().is_terminal() && !(shell_prefill && io::stderr().is_terminal()))
    {
        print!(
            "{}",
            super::Render::render_categorized(&build_index(), 0, false, None, 72, color, None)
        );
        println!();
        return Ok(());
    }
    let guard = if shell_prefill {
        RawGuard::enable_with_captured_stdout()
    } else {
        RawGuard::enable()
    };
    let Some(_guard) = guard else {
        eprint!(
            "{}",
            Render::render_categorized(&build_index(), 0, false, None, 72, color, None)
        );
        eprintln!();
        return Ok(());
    };

    let (width, height) = terminal_size();
    let mut state = State::new(width, height, color);
    let stdin = io::stdin();
    let mut reader = KeyReader::new(stdin.lock());
    let mut alt: Option<AltScreen> = None;
    let mut prev_lines = redraw(0, &state.render(), state.height());

    loop {
        let key = reader.read_key();
        if matches!(key, Key::Idle) {
            let (next_width, next_height) = terminal_size();
            if (next_width, next_height) == (state.width(), state.height()) {
                continue;
            }
            state.resize(next_width, next_height);
        }
        match state.apply_key(key) {
            Action::Continue => {}
            Action::Quit => {
                quit_without_prefill(prev_lines);
                return Ok(());
            }
            Action::EnterReference => {
                alt = Some(AltScreen::enter());
                prev_lines = 0;
            }
            Action::LeaveReference => {
                alt = None;
                prev_lines = 0;
            }
            Action::Submit(command) => {
                drop(alt);
                drop(_guard);
                if let Some(cmd) = command {
                    // Prefill widgets (`jet env hook` / jetpack shell) set
                    // JET_HELP_SHELL_PREFILL and capture the sole stdout line.
                    // Piped `$(jet ?)` also needs that line. A bare interactive
                    // TTY without hooks must NOT print it — that looks like a
                    // paste and leaves the next prompt empty.
                    if shell_prefill || !io::stdout().is_terminal() {
                        println!("{}", cmd);
                    } else {
                        eprintln!("Selected: {cmd}");
                        eprintln!(
                            "not inserted into the prompt — install once, then open a new shell:"
                        );
                        eprintln!("  jet env hook bash >> ~/.bashrc");
                        eprintln!("  jet env hook zsh  >> ~/.zshrc");
                        eprintln!("  jet env hook fish | source");
                        eprintln!("then: jet ?  or  Alt-?");
                    }
                }
                return Ok(());
            }
        }

        let (next_width, next_height) = terminal_size();
        state.resize(next_width, next_height);
        let frame = state.render();
        prev_lines = redraw(prev_lines, &frame, state.height());
    }
}

fn quit_without_prefill(prev_lines: usize) {
    let visible_prev = prev_lines.min(terminal_size().1);
    if visible_prev > 0 {
        eprint!("\r");
        if visible_prev > 1 {
            eprint!("\x1b[{}A", visible_prev - 1);
        }
        eprint!("\x1b[J");
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
    res_scroll: usize,
    ref_category: usize,
    ref_entry: Option<usize>,
    ref_query: &str,
    ref_scroll: usize,
    width: usize,
    height: usize,
    color: bool,
) -> String {
    if matches!(mode, Mode::Results | Mode::Detail) {
        if let Some(ex) = result_code(hits, res_selected) {
            let canonical = Render::render_code_page(ex, color);
            return Render::render_text_viewport(&canonical, width, height, res_scroll);
        }
    }
    match mode {
        Mode::Categorized => Render::render_categorized(
            index,
            cat_selected,
            cat_entry.is_some(),
            selected_category_cmd(index, cat_selected, cat_entry),
            width,
            color,
            Some(height),
        ),
        Mode::Results => {
            Render::render_result_list(hits, query, width, color, Some(res_selected), Some(height))
        }
        Mode::Detail => {
            let entry = current_entry(cat_selected, cat_entry, hits, res_selected, index);
            match entry {
                Some(e) => Render::render_lines_viewport(
                    &Render::render_detail(e, width, color),
                    height,
                    res_scroll,
                ),
                None => Render::render_result_list(
                    hits,
                    query,
                    width,
                    color,
                    Some(res_selected),
                    Some(height),
                ),
            }
        }
        Mode::Reference => {
            if let Some(ex) = reference_code(index, ref_query) {
                let canonical = Render::render_code_page(&ex, color);
                return Render::render_text_viewport(&canonical, width, height, ref_scroll);
            }
            let sel = ref_entry.map(|i| &index[i]);
            Render::render_reference(index, ref_category, sel, width, height, color, ref_query)
        }
    }
}

fn result_code(hits: &[Hit], selected: usize) -> Option<&crate::Explain::Explanation> {
    match hits.get(selected) {
        Some(Hit::Code(ex)) => Some(ex),
        _ => None,
    }
}

fn reference_code(index: &[Entry], query: &str) -> Option<crate::Explain::Explanation> {
    match search(index, query).into_iter().next() {
        Some(Hit::Code(ex)) => Some(ex),
        _ => None,
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
        return index
            .iter()
            .find(|e| e.symbol.identity == entry.symbol.identity);
    }
    selected_category_index(index, cat_selected, cat_entry).map(|i| &index[i])
}

#[allow(clippy::too_many_arguments)]
fn current_prefill(
    mode: &Mode,
    index: &[Entry],
    cat_selected: usize,
    cat_entry: Option<usize>,
    hits: &[Hit],
    res_selected: usize,
    ref_entry: Option<usize>,
    want_example: bool,
) -> Option<String> {
    let entry = match mode {
        Mode::Reference => ref_entry.map(|i| &index[i]),
        Mode::Results | Mode::Detail => match hits.get(res_selected) {
            Some(Hit::Command { entry, .. }) => index
                .iter()
                .find(|e| e.symbol.identity == entry.symbol.identity),
            Some(Hit::Code(_)) | None => None,
        },
        Mode::Categorized => {
            selected_category_index(index, cat_selected, cat_entry).map(|i| &index[i])
        }
    }?;
    if want_example {
        prefill_example(entry).or_else(|| Some(prefill_command(entry)))
    } else {
        Some(prefill_command(entry))
    }
}

fn entries_in_category(index: &[Entry], category: usize) -> Vec<usize> {
    let Some(category) = super::CATEGORIES.get(category) else {
        return Vec::new();
    };
    index
        .iter()
        .enumerate()
        .filter(|(_, e)| &e.category == category)
        .map(|(i, _)| i)
        .collect()
}

fn selected_category_index(
    index: &[Entry],
    category: usize,
    entry: Option<usize>,
) -> Option<usize> {
    entry.and_then(|entry| entries_in_category(index, category).get(entry).copied())
}

fn selected_category_cmd(index: &[Entry], category: usize, entry: Option<usize>) -> Option<&str> {
    selected_category_index(index, category, entry).map(|i| index[i].symbol.name.as_str())
}

fn apply_reference_search(
    index: &[Entry],
    query: &str,
    category: &mut usize,
    entry: &mut Option<usize>,
) {
    if query.is_empty() {
        return;
    }
    if let Some(Hit::Command { entry: found, .. }) = search(index, query).first() {
        if let Some(ci) = super::CATEGORIES.iter().position(|c| *c == found.category) {
            *category = ci;
        }
        *entry = index
            .iter()
            .position(|e| e.symbol.identity == found.symbol.identity);
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
            Some((
                parts.next()?.parse::<usize>().ok()?,
                parts.next()?.parse::<usize>().ok()?,
            ))
        });
    let (rows, cols) = size.unwrap_or((24, crate::Term::terminal_width()));
    (cols.clamp(24, 160), rows.max(5))
}

fn prefill_command(entry: &Entry) -> String {
    format!("jet {}", entry.symbol.name)
}

fn prefill_example(entry: &Entry) -> Option<String> {
    entry.symbol.examples.first().cloned()
}
