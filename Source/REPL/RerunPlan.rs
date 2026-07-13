//! D-FE-REPL-RERUN1=A (ratified 2026-07-08, option A): "replay from the
//! edited turn forward with effect confirmation". Rerunning a turn rebuilds
//! session state from that turn forward, recomputing every later turn in
//! order. Pure/binding turns replay automatically; a turn that produced an
//! observable effect the first time (`ReplTurn::had_effect`) pauses the plan
//! and needs a `y`/`N` confirmation before Jet replays it again — the REPL
//! never shows stale state as current, and it never re-fires an effect
//! silently.
//!
//! This module is pure: it only *decides* the plan and renders it as text.
//! `Interactive::cmd_rerun` (and the `:rerun` textual fallback in `mod.rs`)
//! own actually re-executing the steps and asking for confirmation.

use super::{bold, ReplTurn};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepKind {
    /// Pure or binding turn — replays without asking.
    Auto,
    /// Produced an observable effect the first time — replaying it needs a
    /// `y`/`N` confirmation (declining stale-marks the turn instead).
    ConfirmEffect,
}

#[derive(Clone, Debug)]
pub struct ReplayStep {
    pub turn_id: usize,
    /// The input to replay: the edited text for `from_id`, the turn's
    /// original recorded input for everything after it.
    pub input: String,
    pub kind: StepKind,
}

#[derive(Clone, Debug)]
pub struct ReplayPlan {
    pub from_id: usize,
    pub steps: Vec<ReplayStep>,
}

/// Build the replay plan for rerunning `from_id` forward through the end of
/// the session's recorded turns, substituting `edited_input` for `from_id`'s
/// own text (`None` = an unedited rerun of the same input).
pub fn build_replay_plan(
    turns: &[ReplTurn],
    from_id: usize,
    edited_input: Option<&str>,
) -> Result<ReplayPlan, String> {
    if !turns.iter().any(|t| t.id == from_id) {
        return Err(format!("turn #{from_id} does not exist"));
    }
    let steps = turns
        .iter()
        .filter(|t| t.id >= from_id)
        .map(|t| {
            let input = if t.id == from_id {
                edited_input.unwrap_or(&t.input).to_string()
            } else {
                t.input.clone()
            };
            let kind = if t.had_effect {
                StepKind::ConfirmEffect
            } else {
                StepKind::Auto
            };
            ReplayStep {
                turn_id: t.id,
                input,
                kind,
            }
        })
        .collect();
    Ok(ReplayPlan { from_id, steps })
}

/// Whether applying `plan` needs at least one `y`/`N` prompt.
pub fn plan_needs_confirmation(plan: &ReplayPlan) -> bool {
    plan.steps.iter().any(|s| s.kind == StepKind::ConfirmEffect)
}

/// Render the ratified plan shape:
/// ```text
/// Replay plan:
///   1 rate :: 0.08        auto
///   2 invoice_total ...   auto
///   3 write_file(...)     confirm effect
/// Apply? [y/N]
/// ```
pub fn render_replay_plan(plan: &ReplayPlan, color: bool) -> String {
    let mut out = String::from("Replay plan:\n");
    let widest_input = plan
        .steps
        .iter()
        .map(|s| s.input.chars().count())
        .max()
        .unwrap_or(0);
    for step in &plan.steps {
        let tag = match step.kind {
            StepKind::Auto => "auto",
            StepKind::ConfirmEffect => "confirm effect",
        };
        let pad = widest_input.saturating_sub(step.input.chars().count()) + 2;
        out.push_str(&format!(
            "  {} {}{}{}\n",
            step.turn_id,
            step.input,
            " ".repeat(pad),
            tag
        ));
    }
    out.push_str(&format!("Apply? [y/{}]", bold("N", color)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::REPL::ReplTurnStatus;

    fn turn(id: usize, input: &str, had_effect: bool, bound_name: Option<&str>) -> ReplTurn {
        ReplTurn {
            id,
            input: input.to_string(),
            summary: String::new(),
            status: ReplTurnStatus::Ok,
            folded: false,
            pinned: false,
            stale: false,
            had_effect,
            bound_name: bound_name.map(str::to_string),
            pending_unfold: None,
        }
    }

    #[test]
    fn plan_marks_pure_turns_auto_and_effectful_confirm() {
        let turns = vec![
            turn(1, "rate :: 0.07", false, Some("rate")),
            turn(2, "invoice_total :: subtotal * (1.0 + rate)", false, Some("invoice_total")),
            turn(3, "write_file(\"out.txt\", invoice_total)", true, None),
        ];
        let plan = build_replay_plan(&turns, 1, Some("rate :: 0.08")).expect("plan");
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].input, "rate :: 0.08");
        assert_eq!(plan.steps[0].kind, StepKind::Auto);
        assert_eq!(plan.steps[1].kind, StepKind::Auto);
        assert_eq!(plan.steps[2].kind, StepKind::ConfirmEffect);
        assert!(plan_needs_confirmation(&plan));
    }

    #[test]
    fn plan_without_effects_needs_no_confirmation() {
        let turns = vec![turn(1, "x :: 1", false, Some("x")), turn(2, "x * 2", false, None)];
        let plan = build_replay_plan(&turns, 1, None).expect("plan");
        assert!(!plan_needs_confirmation(&plan));
    }

    #[test]
    fn unknown_turn_id_errors() {
        let turns = vec![turn(1, "x :: 1", false, Some("x"))];
        assert!(build_replay_plan(&turns, 5, None).is_err());
    }

    #[test]
    fn render_matches_ratified_shape() {
        let turns = vec![
            turn(1, "rate :: 0.08", false, Some("rate")),
            turn(2, "invoice_total ...", false, None),
            turn(3, "write_file(...)", true, None),
        ];
        let plan = build_replay_plan(&turns, 1, None).expect("plan");
        let rendered = render_replay_plan(&plan, false);
        assert!(rendered.starts_with("Replay plan:\n"));
        assert!(rendered.contains("1 rate :: 0.08"));
        assert!(rendered.contains("auto"));
        assert!(rendered.contains("3 write_file(...)"));
        assert!(rendered.contains("confirm effect"));
        assert!(rendered.trim_end().ends_with("Apply? [y/N]"));
    }
}
