//! Evaluate one notebook cell against a live REPL [`Session`].

use crate::{
    classify, echo_text, failure_text, normalize_repl_input, rebuild_funcs, run_stmts_turn,
    turn_failure_summary, type_check_item, update_core_imports_from_ledger, Executor, InputKind,
    ReplAuthorization, ReplTurnStatus, Session, TurnResult,
};
use std::path::Path;

#[derive(Clone, Debug)]
pub struct EvalResult {
    pub text: String,
    pub status: ReplTurnStatus,
    pub had_effect: bool,
    pub quit: bool,
    /// The echoed value with its type-directed JSON (rich cell output).
    pub value: Option<Executor::TurnValue>,
}

impl EvalResult {
    fn plain(text: String, status: ReplTurnStatus) -> Self {
        Self {
            text,
            status,
            had_effect: false,
            quit: false,
            value: None,
        }
    }
}

fn error_lines(diagnostics: &[crate::Diagnostics::Diagnostic]) -> String {
    failure_text(diagnostics, None)
}

/// Run one input after the notebook has supplied its complete declaration set.
pub(crate) fn evaluate_step_with_items(
    session: &mut Session,
    input: &str,
    base_dir: &Path,
    authorizer: &mut ReplAuthorization<'_>,
    notebook_items_preloaded: bool,
) -> EvalResult {
    let normalized = normalize_repl_input(input);
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return EvalResult::plain(String::new(), ReplTurnStatus::Ok);
    }

    session.step += 1;

    // Notebook declarations are file-wide (D-NOTEBOOK-SURFACE1): the kernel
    // already placed every parser-plane item cell (`is_item_input`) into the
    // session's item set before this call. Such a cell's own turn must run on
    // the item lane too — `classify`'s keyword heads only cover `fn`/`struct`/…,
    // so an `@name :: expr` or `#Test` cell would fall into the statement lane,
    // where `Checker::declare` collides with the cell's own file-wide
    // declaration (E0118 "already taken").
    let classified = if notebook_items_preloaded && crate::is_item_input(trimmed) {
        Ok(InputKind::Item(format!("{trimmed}\n")))
    } else {
        classify(trimmed, session.step)
    };
    let kind = match classified {
        Ok(k) => k,
        Err(ds) => {
            session.record_turn(trimmed, ReplTurnStatus::Error, turn_failure_summary(&ds, None));
            return EvalResult::plain(error_lines(&ds), ReplTurnStatus::Error);
        }
    };

    match kind {
        InputKind::Empty => EvalResult::plain(String::new(), ReplTurnStatus::Ok),

        InputKind::Meta(cmd, _) => {
            if matches!(cmd.as_str(), "quit" | "q" | "exit") {
                return EvalResult {
                    quit: true,
                    ..EvalResult::plain("bye\n".to_string(), ReplTurnStatus::Ok)
                };
            }
            session.record_turn(trimmed, ReplTurnStatus::Error, format!("meta:{cmd}"));
            EvalResult::plain(
                format!("error: notebook cells do not run meta-command `:{cmd}`\n"),
                ReplTurnStatus::Error,
            )
        }

        InputKind::Item(src) => {
            let bundle =
                match type_check_item(session, if notebook_items_preloaded { "" } else { &src }) {
                    Ok(bundle) => bundle,
                    Err(errors) => {
                        session.record_turn(
                            trimmed,
                            ReplTurnStatus::Error,
                            turn_failure_summary(&errors, None),
                        );
                        return EvalResult::plain(error_lines(&errors), ReplTurnStatus::Error);
                    }
                };
            if !notebook_items_preloaded {
                session.item_srcs.push(src);
            }
            rebuild_funcs(session);
            update_core_imports_from_ledger(&bundle, &mut session.core_imports);
            session.record_turn(trimmed, ReplTurnStatus::Ok, "ok".to_string());
            session.remember_success(trimmed);
            EvalResult::plain("ok\n".to_string(), ReplTurnStatus::Ok)
        }

        InputKind::Import(src) => {
            session.import_srcs.push(src);
            let bundle = match type_check_item(session, "") {
                Ok(bundle) => bundle,
                Err(errors) => {
                    session.import_srcs.pop();
                    session.record_turn(
                        trimmed,
                        ReplTurnStatus::Error,
                        turn_failure_summary(&errors, None),
                    );
                    return EvalResult::plain(error_lines(&errors), ReplTurnStatus::Error);
                }
            };
            rebuild_funcs(session);
            update_core_imports_from_ledger(&bundle, &mut session.core_imports);
            session.record_turn(trimmed, ReplTurnStatus::Ok, "ok".to_string());
            session.remember_success(trimmed);
            EvalResult::plain("ok\n".to_string(), ReplTurnStatus::Ok)
        }

        InputKind::Stmts(stmts, suppress, check_src) => match run_stmts_turn(
            session, &stmts, suppress, &check_src, trimmed, base_dir, authorizer, true,
        ) {
            TurnResult::Done {
                stdout,
                stderr,
                echo,
                bound_name,
                had_effect,
                ..
            } => {
                let mut out = format!("{stdout}{stderr}");
                let mut summary = out.clone();
                if let Some(value) = &echo {
                    let shown = echo_text(value);
                    out.push_str(&shown);
                    out.push('\n');
                    summary.push_str(&shown);
                }
                session.record_turn_ex(trimmed, ReplTurnStatus::Ok, summary, had_effect, bound_name);
                session.remember_success(trimmed);
                EvalResult {
                    text: out,
                    status: ReplTurnStatus::Ok,
                    had_effect,
                    quit: false,
                    value: echo,
                }
            }
            TurnResult::Interrupted => {
                session.record_turn_ex(
                    trimmed,
                    ReplTurnStatus::Interrupted,
                    "Interrupted".to_string(),
                    true,
                    None,
                );
                EvalResult {
                    had_effect: true,
                    ..EvalResult::plain(
                        "Interrupted. External effects already performed were not rolled back.\n"
                            .to_string(),
                        ReplTurnStatus::Interrupted,
                    )
                }
            }
            TurnResult::Failed {
                stdout,
                stderr,
                diagnostics,
                report,
            } => {
                session.record_turn(
                    trimmed,
                    ReplTurnStatus::Error,
                    turn_failure_summary(&diagnostics, report.as_deref()),
                );
                EvalResult::plain(
                    format!(
                        "{stdout}{stderr}{}",
                        failure_text(&diagnostics, report.as_deref())
                    ),
                    ReplTurnStatus::Error,
                )
            }
        },
    }
}
