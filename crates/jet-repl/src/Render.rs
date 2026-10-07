//! Pure, TTY-independent rendering helpers for the hybrid REPL UX
//! (D-FE-REPL1=D). Kept separate from `Interactive`'s raw-mode event loop so
//! the visible shape of every layer — banner, turn gutter, pin rail, fold
//! marker, bindings-pane row — is unit-testable without a real pty.
//!
//! Nothing here does I/O; every function takes its inputs explicitly
//! (including terminal width) and returns a `String`.

use jet_foundation::Terminal::Theme;
use std::collections::HashSet;

use super::{bold, dim, ReplTurn, Session};
use crate::AST::Type;

/// D-FE-REPL1=D: a `List` value with more items than this auto-folds in the
/// interactive echo (`⋯ N rows folded · [Type] · unfold ⏎`) instead of
/// printing every element inline.
pub const FOLD_LIST_THRESHOLD: usize = 20;

/// Default column to split the flat scrollback from the `^B` bindings pane
/// when the caller doesn't know (or care about) the real terminal width.
pub const DEFAULT_WORKSPACE_SPLIT: usize = 32;

/// D-FE-REPL1=D banner: `Jet <ver> — Interactive REPL  (:quit, :help, ^B bindings)`.
/// Printed unconditionally (TTY and non-TTY) — only the wording changed from
/// the pre-redesign banner, not whether it prints.
pub fn render_banner(version: &str, color: bool) -> String {
    let theme = Theme::new(color);
    format!(
        "{} {} — Interactive REPL  ({}, {}, {} bindings)",
        theme.accent("Jet"),
        version,
        bold(":quit", color),
        bold(":help", color),
        bold("^B", color),
    )
}

/// Startup discovery stays mode-accurate: raw-only keys are never offered
/// when the REPL fell back to its cooked, non-TTY line loop.
pub fn render_discovery_hint(raw_mode: bool, color: bool) -> String {
    if raw_mode {
        format!(
            "Try: {} Complete · {} Cursor Docs · {} Docs · {} History · {} Pin · {} Fold · {} Rerun · {} Bindings",
            bold("Tab", color),
            bold("F1", color),
            bold("?name", color),
            bold("F3", color),
            bold("^P", color),
            bold("^F", color),
            bold("^R", color),
            bold("^B", color),
        )
    } else {
        format!(
            "Try: {} Docs · {} · Interactive Keys Require a TTY",
            bold("?name", color),
            bold(":pin/:fold/:rerun <id>", color),
        )
    }
}

/// The interactive-only prompt: a dim one-character-cost turn-number gutter
/// ahead of `user> ` (`1 user> `). The non-TTY floor keeps the old bare
/// `user> ` prompt (see `lib.rs::run_cooked`) — this is never
/// called from there.
pub fn render_prompt(turn_no: usize, color: bool) -> String {
    let theme = Theme::new(color);
    format!(
        "{} {} ",
        theme.dim(&turn_no.to_string()),
        theme.accent("user>")
    )
}

/// `name :: value` (immutable) / `name := value` (mutable) —
/// the one shared line shape used by both the pin rail and the `^B`
/// bindings pane (D-FE-REPL1=D / D-BIND-BARE1). The sigil matches the
/// binding's original spelling so the pane reads as valid Jet.
pub fn format_binding(name: &str, shown: &str, mutable: bool) -> String {
    let sigil = if mutable { ":=" } else { "::" };
    format!("{} {} {}", name, sigil, shown)
}

/// Top-level element count of a list's Display text (`[a, [b, c], "x,y"]`
/// has three), counted outside nested brackets/braces/parens and quotes.
fn list_element_count(shown: &str) -> usize {
    let inner = shown.trim();
    let Some(inner) = inner.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) else {
        return 0;
    };
    if inner.trim().is_empty() {
        return 0;
    }
    let mut depth = 0usize;
    let mut quoted = false;
    let mut count = 1;
    for ch in inner.chars() {
        match ch {
            '"' => quoted = !quoted,
            '[' | '{' | '(' if !quoted => depth += 1,
            ']' | '}' | ')' if !quoted => depth = depth.saturating_sub(1),
            ',' if !quoted && depth == 0 => count += 1,
            _ => {}
        }
    }
    count
}

/// D-FE-REPL1=D auto-fold: a list past `FOLD_LIST_THRESHOLD` items folds in
/// place instead of printing every element. Returns `(item_count, elem_type)`
/// when the value should fold; `None` for anything short enough to show
/// plainly. The checked type names the elements; the built program's Display
/// text gives the count.
pub fn fold_decision_for_value(ty: &Type, shown: &str) -> Option<(usize, String)> {
    let elem = match ty {
        Type::List(elem) | Type::FixedList { elem, .. } => elem,
        _ => return None,
    };
    let count = list_element_count(shown);
    (count > FOLD_LIST_THRESHOLD).then(|| (count, elem.name()))
}

/// `⋯ 42 rows folded · [Row] · unfold ⏎`
pub fn render_fold_marker(count: usize, elem_type: &str, color: bool) -> String {
    format!(
        "{} {} rows folded · [{}] · unfold {}",
        dim("⋯", color),
        count,
        elem_type,
        dim("⏎", color)
    )
}

/// The pinned-turn rail: a `📌` line (binding name/type/value or, absent a
/// bound name, the turn's own summary) plus a separator, rendered above the
/// active prompt every cycle. `width` is the terminal column count (falls
/// back to 64 in non-TTY/unknown-width callers).
pub fn render_pin_rail(label: &str, turn_id: usize, width: usize, color: bool) -> String {
    let theme = Theme::new(color);
    let width = width.max(20);
    let hint_plain = format!("turn {} · unpin ^P", turn_id);
    let pin = "📌";
    let head_len = 2 /* pin glyph + space */ + label.chars().count();
    let pad = width
        .saturating_sub(head_len)
        .saturating_sub(hint_plain.chars().count())
        .max(1);
    let hint = format!("turn {} · unpin {}", turn_id, bold("^P", color));
    let sep = theme.border(&"─".repeat(width));
    format!(
        "{} {}{}{}\n{}",
        theme.accent(pin),
        label,
        " ".repeat(pad),
        hint,
        sep
    )
}

/// One row of the `^B` bindings pane: `name : Type = value`, with a
/// `◂ new this step` marker on names introduced by the just-completed turn.
pub fn render_bindings_pane(session: &Session, changed: &HashSet<String>) -> Vec<String> {
    let mut names: Vec<&String> = session.scope.keys().collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let shown = &session.scope[name];
            let mut line = format_binding(name, shown, session.mutable_names.contains(name));
            if changed.contains(name) {
                line.push_str(" ◂ new this step");
            }
            line
        })
        .collect()
}

/// Join a left (session) column and a right (bindings) column into one
/// side-by-side row, padding the left column out to `split_col`. Used to
/// draw the `┌─ session ──…─┬─ bindings ─…─┐` layout one row at a time —
/// each row is independently correct even though the terminal never holds
/// the full historical left column at once (D-FE-REPL1=D hybrid: `^B` is a
/// live strip attached to the current turn, not a retroactive rewrite of
/// scrollback).
pub fn render_workspace_row(left: &str, right: &str, split_col: usize, color: bool) -> String {
    let left_len = left.chars().count();
    let pad = split_col.saturating_sub(left_len).max(1);
    format!(
        "{}{}{} {}",
        left,
        " ".repeat(pad),
        Theme::new(color).border("│"),
        right
    )
}

/// `:turns`-style one-line label for a pinned rail entry: prefers the turn's
/// bound-name live value (`name : Type = value`); falls back to the turn's
/// recorded summary text when the turn didn't bind exactly one new name.
pub fn pin_label(session: &Session, turn: &ReplTurn) -> String {
    if let Some(name) = &turn.bound_name {
        if let Some(shown) = session.scope.get(name) {
            return format_binding(name, shown, session.mutable_names.contains(name));
        }
    }
    if turn.summary.is_empty() {
        "OK".to_string()
    } else {
        turn.summary.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_matches_ratified_wording() {
        let s = render_banner("1.0.0", false);
        assert_eq!(
            s,
            "Jet 1.0.0 — Interactive REPL  (:quit, :help, ^B bindings)"
        );
    }

    #[test]
    fn discovery_hints_match_available_modes() {
        assert_eq!(
            render_discovery_hint(true, false),
            "Try: Tab complete · F1 cursor docs · ?name docs · F3 history · ^P pin · ^F fold · ^R rerun · ^B bindings"
        );
        assert_eq!(
            render_discovery_hint(false, false),
            "Try: ?name docs · :pin/:fold/:rerun <id> · interactive keys require a TTY"
        );
    }

    #[test]
    fn prompt_has_turn_gutter() {
        assert_eq!(render_prompt(1, false), "1 user> ");
        assert_eq!(render_prompt(42, false), "42 user> ");
    }

    #[test]
    fn fold_decision_triggers_past_threshold() {
        let ints = Type::List(Box::new(Type::Int));
        assert!(fold_decision_for_value(&ints, "[0, 1, 2, 3, 4]").is_none());
        let long = format!(
            "[{}]",
            (0..42).map(|n| n.to_string()).collect::<Vec<_>>().join(", ")
        );
        let (count, ty) = fold_decision_for_value(&ints, &long).expect("should fold");
        assert_eq!(count, 42);
        assert_eq!(ty, "Int");
    }

    #[test]
    fn fold_decision_uses_struct_type_name_and_nested_commas() {
        let rows = Type::List(Box::new(Type::Named("Row".to_string())));
        let shown = format!(
            "[{}]",
            (0..30)
                .map(|i| format!("Row {{ id: {i}, tag: \"a,b\" }}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let (count, ty) = fold_decision_for_value(&rows, &shown).expect("should fold");
        assert_eq!(count, 30);
        assert_eq!(ty, "Row");
    }

    #[test]
    fn fold_marker_matches_ratified_shape() {
        let s = render_fold_marker(42, "Row", false);
        assert_eq!(s, "⋯ 42 rows folded · [Row] · unfold ⏎");
    }

    #[test]
    fn pin_rail_shows_label_and_unpin_hint() {
        let s = render_pin_rail("total: Int :: 15", 3, 62, false);
        let mut lines = s.lines();
        let head = lines.next().unwrap();
        assert!(head.starts_with("📌 total: Int :: 15"));
        assert!(head.ends_with("turn 3 · unpin ^P"));
        let sep = lines.next().unwrap();
        assert!(sep.chars().all(|c| c == '─'));
    }

    #[test]
    fn format_binding_matches_workspace_shape() {
        assert_eq!(format_binding("total", "15", false), "total :: 15");
        assert_eq!(format_binding("total", "15", true), "total := 15");
    }
}
