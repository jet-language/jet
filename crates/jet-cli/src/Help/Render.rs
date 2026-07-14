//! Pure text rendering for `jet ?` — every function here takes a width and
//! returns a `String`; nothing touches a terminal. `Interactive` redraws
//! these frames over raw mode; `mod.rs::run_query` prints one frame's worth
//! for the non-interactive floor. Kept pure so layout/highlighting/NO_COLOR
//! behavior is unit-testable without a real pty (card #360 test plan).

use crate::Explain::Explanation;
use super::{Entry, Hit};

const MIN_WIDTH: usize = 24;

fn w(width: usize) -> usize {
    width.max(MIN_WIDTH)
}

/// Visible column count of `s` (byte-agnostic; this module's inputs are all
/// plain ASCII command/flag/summary text, so `chars().count()` is exact).
fn cols(s: &str) -> usize {
    let mut escaped = false;
    s.chars()
        .filter(|&ch| {
            if escaped {
                if ch == 'm' {
                    escaped = false;
                }
                false
            } else if ch == '\x1b' {
                escaped = true;
                false
            } else {
                true
            }
        })
        .count()
}

fn pad(s: &str, width: usize) -> String {
    let c = cols(s);
    if c >= width {
        truncate_visible(s, width)
    } else {
        format!("{}{}", s, " ".repeat(width - c))
    }
}

fn truncate_visible(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let keep = width.saturating_sub(1);
    let mut out = String::new();
    let mut visible = 0usize;
    let mut escaped = false;
    for ch in s.chars() {
        if escaped {
            out.push(ch);
            if ch == 'm' {
                escaped = false;
            }
        } else if ch == '\x1b' {
            escaped = true;
            out.push(ch);
        } else if visible < keep {
            out.push(ch);
            visible += 1;
        } else {
            break;
        }
    }
    out.push('…');
    if s.contains('\x1b') {
        out.push_str("\x1b[0m");
    }
    out
}

fn paint(on: bool, sgr: &str, text: &str) -> String {
    if on {
        format!("\x1b[{sgr}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

fn top(width: usize, label: &str, color: bool) -> String {
    let width = w(width);
    let plain = format!("┌─ {} ", label);
    let fill = width.saturating_sub(cols(&plain) + 1);
    if color {
        format!(
            "{}\x1b[1;96m{}\x1b[0m{}",
            paint(true, "90", "┌─ "),
            label,
            paint(true, "90", &format!(" {}┐", "─".repeat(fill)))
        )
    } else {
        format!("{}{}┐", plain, "─".repeat(fill))
    }
}

fn mid(width: usize, color: bool) -> String {
    let width = w(width);
    paint(color, "90", &format!("├{}┤", "─".repeat(width.saturating_sub(2))))
}

fn bottom(width: usize, color: bool) -> String {
    let width = w(width);
    paint(color, "90", &format!("└{}┘", "─".repeat(width.saturating_sub(2))))
}

/// One content row. `selected` draws the NO_COLOR `>` marker; color mode uses
/// reverse-video via `selected_row`.
fn row(width: usize, text: &str, selected: bool) -> String {
    let width = w(width);
    let marker = if selected { "> " } else { "  " };
    let inner = width.saturating_sub(4);
    format!("│{}{}│", marker, pad(text, inner))
}

fn selected_row(width: usize, text: &str, selected: bool, color: bool) -> String {
    let width = w(width);
    if selected && color {
        let inner = width.saturating_sub(2);
        format!(
            "{}\x1b[48;5;24;97;1m{}\x1b[0m{}",
            paint(true, "90", "│"),
            pad(text, inner),
            paint(true, "90", "│")
        )
    } else if color {
        let inner = width.saturating_sub(4);
        format!(
            "{}  {}{}",
            paint(true, "90", "│"),
            pad(text, inner),
            paint(true, "90", "│")
        )
    } else {
        row(width, text, selected)
    }
}

/// Bracket every maximal run of matched char indices in `text` — the
/// NO_COLOR fuzzy-match emphasis (`jet [run] <file>`), per the ratified
/// mock's NO_COLOR panel.
fn bracket_matches(text: &str, positions: &[usize]) -> String {
    if positions.is_empty() {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut marked = vec![false; chars.len()];
    for &p in positions {
        if p < marked.len() {
            marked[p] = true;
        }
    }
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if marked[i] {
            out.push('[');
            while i < chars.len() && marked[i] {
                out.push(chars[i]);
                i += 1;
            }
            out.push(']');
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Same emphasis, ANSI cyan instead of brackets, for `color` mode.
fn color_matches(text: &str, positions: &[usize]) -> String {
    if positions.is_empty() {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut marked = vec![false; chars.len()];
    for &p in positions {
        if p < marked.len() {
            marked[p] = true;
        }
    }
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if marked[i] {
            out.push_str("\x1b[36m");
            while i < chars.len() && marked[i] {
                out.push(chars[i]);
                i += 1;
            }
            out.push_str("\x1b[0m");
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn emphasize(text: &str, positions: &[usize], color: bool) -> String {
    if color {
        color_matches(text, positions)
    } else {
        bracket_matches(text, positions)
    }
}

/// The default, empty-query view: commands grouped by category (owner
/// modification, 2026-07-08 — replaces the mock's standalone goal screen).
/// `selected_cmd` names the highlighted row (by command name, not row index
/// — category headers aren't selectable, so a plain "nth row" index would
/// have to know the header layout too).
pub fn render_categorized(
    index: &[Entry],
    selected_category: usize,
    expanded: bool,
    selected_cmd: Option<&str>,
    width: usize,
    color: bool,
) -> String {
    let width = w(width);
    let mut out = String::new();
    out.push_str(&top(width, "jet ? — command palette", color));
    out.push('\n');
    let hint = if color {
        paint(true, "2;37", "type to search · ↑↓ move · ⏎ prefill · ⇥ detail · F1 reference")
    } else {
        "type to search · ↑↓ move · ⏎ prefill · ⇥ detail · F1 reference".to_string()
    };
    out.push_str(&selected_row(width, &hint, false, color));
    out.push('\n');
    out.push_str(&mid(width, color));
    out.push('\n');
    for (ci, cat) in super::CATEGORIES.iter().enumerate() {
        let entries: Vec<&Entry> = index.iter().filter(|e| &e.category == cat).collect();
        if entries.is_empty() && *cat != "Error codes" {
            continue;
        }
        let is_expanded = ci == selected_category && expanded;
        let marker = if is_expanded { "▾" } else { "▸" };
        let plain = format!("{} {}", marker, cat);
        let label = if color && ci == selected_category {
            format!("\x1b[1;96m{}\x1b[0m", plain)
        } else if color {
            format!("\x1b[1;37m{}\x1b[0m", plain)
        } else {
            plain
        };
        out.push_str(&selected_row(width, &label, ci == selected_category && !is_expanded, color));
        out.push('\n');
        if is_expanded {
            if *cat == "Error codes" {
                let tip = if color {
                    paint(true, "2;37", "type an E-code, such as E0102, for verbatim help")
                } else {
                    "type an E-code, such as E0102, for verbatim help".to_string()
                };
                out.push_str(&selected_row(width, &tip, false, color));
                out.push('\n');
            }
            for e in entries {
                let line = if color {
                    let cmd_col = format!("{:<20}", e.cmd);
                    format!(
                        "{} {} {}",
                        paint(true, "1;36", "jet"),
                        paint(true, "1;37", &cmd_col),
                        paint(true, "2;37", &e.summary)
                    )
                } else {
                    format!("jet {:<20} {}", e.cmd, e.summary)
                };
                out.push_str(&selected_row(width, &line, selected_cmd == Some(e.cmd.as_str()), color));
                out.push('\n');
            }
        }
    }
    out.push_str(&bottom(width, color));
    out
}

/// Command names in categorized display order (headers excluded) — the
/// selection sequence `Interactive`'s ↑/↓ walks over.
pub fn categorized_order(index: &[Entry]) -> Vec<&str> {
    super::CATEGORIES
        .iter()
        .flat_map(|cat| index.iter().filter(move |e| &e.category == cat).map(|e| e.cmd.as_str()))
        .collect()
}

/// The fuzzy-filtered result list (typing) — also the non-interactive
/// `jet ? <query>` floor when `selected` is `None`.
pub fn render_result_list(hits: &[Hit], query: &str, width: usize, color: bool, selected: Option<usize>) -> String {
    let width = w(width);
    let mut out = String::new();
    out.push_str(&top(width, "find a command", color));
    out.push('\n');
    let query_line = if color {
        format!("{} {}", paint(true, "1;96", ">"), paint(true, "1;37", query))
    } else {
        format!("> {}", query)
    };
    out.push_str(&selected_row(width, &query_line, false, color));
    out.push('\n');
    out.push_str(&mid(width, color));
    out.push('\n');
    if hits.is_empty() {
        let empty = if color { paint(true, "2;37", "no matches") } else { "no matches".to_string() };
        out.push_str(&selected_row(width, &empty, false, color));
        out.push('\n');
    }
    for (i, hit) in hits.iter().enumerate() {
        let is_sel = selected == Some(i) || (selected.is_none() && i == 0);
        match hit {
            Hit::Command { entry, haystack, positions, .. } => {
                let display = format!("jet {}   {}", entry.cmd, entry.summary);
                // Only emphasize when the matched haystack IS the displayed
                // usage/command text (keyword-alias hits show plain — the
                // matched words aren't on screen to bracket).
                let line = if haystack.starts_with("jet ") {
                    emphasize(&display, positions, color)
                } else {
                    display
                };
                out.push_str(&selected_row(width, &line, is_sel, color));
            }
            Hit::Code(ex) => {
                out.push_str(&selected_row(width, &format!("{}   {}", ex.code, ex.meaning), is_sel, color));
            }
        }
        out.push('\n');
    }
    out.push_str(&mid(width, color));
    out.push('\n');
    if let Some(Hit::Command { entry, .. }) = hits.first() {
        if let Some(ex) = &entry.example {
            let ex_line = if color {
                format!("{} {}", paint(true, "2;37", "example"), paint(true, "37", ex))
            } else {
                format!("example  {}", ex)
            };
            out.push_str(&selected_row(width, &ex_line, false, color));
            out.push('\n');
        }
    }
    let footer = if color {
        paint(true, "2;37", "↑↓ move · ⏎ prefill shell · ⇥ detail · F1 reference")
    } else {
        "↑↓ move · ⏎ prefill shell · ⇥ detail · F1 reference".to_string()
    };
    out.push_str(&selected_row(width, &footer, false, color));
    out.push('\n');
    out.push_str(&bottom(width, color));
    out
}

/// Tab's man-depth inline expansion of one entry — usage, flags, example,
/// see-also, matching the ratified mock's "step 3".
pub fn render_detail(entry: &Entry, width: usize, color: bool) -> String {
    let width = w(width);
    let mut out = String::new();
    out.push_str(&top(width, &format!("jet {}  ⇥ collapse", entry.cmd), color));
    out.push('\n');
    out.push_str(&selected_row(width, &format!("Usage   {}", entry.usage), false, color));
    out.push('\n');
    out.push_str(&selected_row(width, entry.summary, false, color));
    out.push('\n');
    if entry.flags.is_empty() {
        out.push_str(&selected_row(width, "Flags   (none)", false, color));
        out.push('\n');
    } else {
        for (i, (flag, help)) in entry.flags.iter().enumerate() {
            let label = if i == 0 { "Flags   " } else { "        " };
            out.push_str(&selected_row(width, &format!("{}{}  {}", label, flag, help), false, color));
            out.push('\n');
        }
    }
    if let Some(ex) = &entry.example {
        out.push_str(&selected_row(width, &format!("Example {}", ex), false, color));
        out.push('\n');
    }
    if !entry.see_also.is_empty() {
        out.push_str(&selected_row(width, &format!("See also  {}", entry.see_also.join(" · ")), false, color));
        out.push('\n');
    }
    out.push_str(&mid(width, color));
    out.push('\n');
    let prefill = entry.example.clone().unwrap_or_else(|| format!("jet {}", entry.cmd));
    let footer = if color {
        format!("\x1b[2;37m⏎ prefill: {} · F1 open in reference\x1b[0m", prefill)
    } else {
        format!("prefill: {} · F1 open in reference", prefill)
    };
    out.push_str(&selected_row(width, &footer, false, color));
    out.push('\n');
    out.push_str(&bottom(width, color));
    out
}

/// Canonical diagnostic page. Byte-for-byte the same text as `jet explain`;
/// terminal viewport layout must never reconstruct or summarize it.
pub fn render_code_page(ex: &Explanation, color: bool) -> String {
    crate::Explain::render(ex, color)
}

/// Fixed-height viewport over canonical text. Long source lines wrap without
/// dropping bytes; `scroll` exposes every wrapped row without terminal scroll.
pub fn render_text_viewport(text: &str, width: usize, height: usize, scroll: usize) -> String {
    let width = w(width);
    let height = height.max(1);
    let rows: Vec<String> = wrap_text_rows(text, width)
        .into_iter()
        .map(|row| {
            let _source_break = row.hard_break;
            row.text
        })
        .collect();
    let max_start = rows.len().saturating_sub(height);
    let start = scroll.min(max_start);
    (0..height)
        .map(|offset| rows.get(start + offset).cloned().unwrap_or_else(|| " ".to_string()))
        .collect::<Vec<_>>()
        .join("\n")
}

struct WrappedRow {
    text: String,
    /// The source contained a newline after this row. Soft viewport wraps are
    /// false, letting tests reconstruct and compare the canonical byte stream.
    hard_break: bool,
}

fn wrap_text_rows(text: &str, width: usize) -> Vec<WrappedRow> {
    let width = width.max(1);
    let mut rows = Vec::new();
    for source_line in text.split_inclusive('\n') {
        let hard_break = source_line.ends_with('\n');
        let line = source_line.strip_suffix('\n').unwrap_or(source_line);
        let mut row = String::new();
        let mut visible = 0usize;
        let mut chars = line.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                // Explain color currently uses SGR. Copy the complete control
                // atom without charging terminal columns or splitting bytes.
                row.push(ch);
                while let Some(control) = chars.next() {
                    row.push(control);
                    if control == 'm' {
                        break;
                    }
                }
                continue;
            }
            if visible == width {
                rows.push(WrappedRow { text: std::mem::take(&mut row), hard_break: false });
                visible = 0;
            }
            row.push(ch);
            visible += 1;
        }
        rows.push(WrappedRow { text: row, hard_break });
    }
    rows
}

/// The F1 two-pane reference (alt-screen): a category tree on the left,
/// the selected entry's man-depth detail on the right — same index as the
/// overlay, deeper read. `left_width` splits the frame; `height` caps rows
/// (extra category/detail lines are simply not shown — `Interactive` keeps
/// the selection scrolled into view).
pub fn render_reference(
    index: &[Entry],
    selected_category: usize,
    selected_entry: Option<&Entry>,
    width: usize,
    height: usize,
    color: bool,
    query: &str,
) -> String {
    let width = w(width).max(50);
    let height = height.max(6);
    let left_width = (width / 3).max(18);
    let right_width = width - left_width - 2;

    let header = if color {
        format!("\x1b[36;1mjet ?\x1b[0m reference{}", if query.is_empty() { String::new() } else { format!(" · search: {query}") })
    } else {
        format!("jet ? reference{}", if query.is_empty() { "".to_string() } else { format!(" · search: {query}") })
    };
    let mut lines = vec![pad(&header, width)];

    let mut left_rows: Vec<String> = Vec::new();
    let mut selected_row = None;
    for (ci, cat) in super::CATEGORIES.iter().enumerate() {
        let marker = if ci == selected_category { "▾" } else { "▸" };
        left_rows.push(format!("{} {}", marker, cat));
        if ci == selected_category {
            for e in index.iter().filter(|e| &e.category == cat) {
                let sel = selected_entry.map(|s| s.cmd.as_str()) == Some(e.cmd.as_str());
                let prefix = if sel { "> " } else { "  " };
                if sel {
                    selected_row = Some(left_rows.len());
                }
                left_rows.push(format!("{}  {}", prefix, e.cmd));
            }
        }
    }

    let right_rows: Vec<String> = match selected_entry {
        Some(e) => render_detail(e, right_width, color)
            .lines()
            .map(str::to_string)
            .collect(),
        None => vec!["select a command on the left".to_string()],
    };

    let body_rows = height.saturating_sub(2);
    let left_start = selected_row
        .map(|row| row.saturating_sub(body_rows.saturating_sub(1)))
        .unwrap_or(0)
        .min(left_rows.len().saturating_sub(body_rows));
    for i in 0..body_rows {
        let l = left_rows.get(left_start + i).map(|s| pad(s, left_width)).unwrap_or_else(|| " ".repeat(left_width));
        let r = pad(right_rows.get(i).map(String::as_str).unwrap_or(""), right_width);
        lines.push(format!("│{}│{}", l, r));
    }
    lines.push(pad("↑↓ move · → into · ⏎ prefill shell · Esc back to overlay", width));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Help::build_index;

    #[test]
    fn categorized_view_lists_every_command_once() {
        let index = build_index();
        let out = render_categorized(&index, 0, false, None, 72, false);
        for e in &index {
            assert!(!out.contains(&format!("jet {}", e.cmd)), "collapsed view leaked {}", e.cmd);
        }
    }

    #[test]
    fn categorized_view_box_is_well_formed_at_fixed_width() {
        let index = build_index();
        let out = render_categorized(&index, 0, true, Some("run"), 64, false);
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines.first().unwrap().starts_with('┌'));
        assert!(lines.last().unwrap().starts_with('└'));
        for l in &lines {
            assert_eq!(cols(l), 64, "line not padded to width: {:?}", l);
        }
    }

    #[test]
    fn categorized_view_expands_only_selected_category() {
        let index = build_index();
        let out = render_categorized(&index, 0, true, Some("run"), 72, false);
        assert!(out.contains("jet run"));
        assert!(!out.contains("jet add"));
    }

    #[test]
    fn color_mode_styles_header_and_selection_without_breaking_width() {
        let index = build_index();
        let out = render_categorized(&index, 0, true, Some("run"), 64, true);
        assert!(out.contains("\x1b[1;96m"));
        assert!(out.contains("\x1b[48;5;24;97;1m"));
        for line in out.lines() {
            assert_eq!(cols(line), 64, "bad colored width: {line:?}");
        }
    }

    #[test]
    fn categorized_order_covers_every_command_no_headers() {
        let index = build_index();
        let order = categorized_order(&index);
        assert_eq!(order.len(), index.len());
        for e in &index {
            assert!(order.contains(&e.cmd.as_str()));
        }
    }

    #[test]
    fn result_list_no_color_uses_bracket_emphasis() {
        let index = build_index();
        let hits = super::super::search(&index, "run");
        let out = render_result_list(&hits, "run", 64, false, None);
        assert!(out.contains("[run]"), "expected bracketed match, got:\n{}", out);
    }

    #[test]
    fn code_page_is_verbatim_from_explain() {
        let ex = crate::Explain::lookup("E0102").unwrap();
        assert_eq!(render_code_page(&ex, false), crate::Explain::render(&ex, false));
        assert_eq!(render_code_page(&ex, true), crate::Explain::render(&ex, true));
    }

    #[test]
    fn detail_view_never_shows_invented_flags() {
        let index = build_index();
        let run = index.iter().find(|e| e.cmd == "run").unwrap();
        let out = render_detail(run, 70, false);
        assert!(!out.contains("--watch"));
    }

    #[test]
    fn reference_view_has_header_and_footer() {
        let index = build_index();
        let out = render_reference(&index, 0, index.first(), 80, 12, false, "run");
        assert!(out.starts_with("jet ? reference"));
        assert!(out.contains("search: run"));
        assert!(out.contains("Esc back to overlay"));
        assert_eq!(out.lines().count(), 12);
        assert!(out.lines().all(|line| cols(line) == 80));
    }

    #[test]
    fn short_reference_view_scrolls_selected_left_row_into_view() {
        let index = build_index();
        let category = super::super::CATEGORIES.iter().position(|c| *c == "Reference").unwrap();
        let selected = index.iter().filter(|e| e.category == "Reference").last().unwrap();
        let out = render_reference(&index, category, Some(selected), 60, 8, false, "");
        assert_eq!(out.lines().count(), 8);
        assert!(out.contains(&format!(">   {}", selected.cmd)), "selected row clipped:\n{out}");
        assert!(out.lines().all(|line| cols(line) == 60));
    }

    #[test]
    fn canonical_code_viewport_never_exceeds_requested_rows_or_columns() {
        let ex = crate::Explain::lookup("E0102").unwrap();
        let canonical = render_code_page(&ex, false);
        let out = render_text_viewport(&canonical, 50, 8, 0);
        assert_eq!(out.lines().count(), 8);
        assert!(out.lines().all(|line| cols(line) <= 50));
    }

    #[test]
    fn colored_canonical_wrap_preserves_sgr_atoms_and_source_bytes() {
        let ex = crate::Explain::lookup("E0102").unwrap();
        let canonical = render_code_page(&ex, true);
        let rows = wrap_text_rows(&canonical, 8);
        assert!(rows.iter().all(|row| cols(&row.text) <= 8));
        assert!(rows.iter().all(|row| {
            row.text
                .split('\x1b')
                .skip(1)
                .all(|suffix| suffix.starts_with("[1m") || suffix.starts_with("[0m"))
        }));
        let mut reconstructed = String::new();
        for row in rows {
            reconstructed.push_str(&row.text);
            if row.hard_break {
                reconstructed.push('\n');
            }
        }
        assert_eq!(reconstructed, canonical);
    }
}
