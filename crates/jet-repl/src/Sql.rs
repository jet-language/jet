//! SQL-console session seam (D-DX-SQLSHELL1=A).
//!
//! This module deliberately does not parse SQL or execute a second evaluator.
//! It owns only the typed REPL session lifetime and returns the same `TurnValue`
//! plus diagnostics used by the ordinary `jet repl` path. `Source/CmdDb.rs`
//! marshals files, dot commands, and output modes around this seam.

use std::path::{Path, PathBuf};

use crate::Executor::TurnValue;
use crate::Diagnostics::Diagnostic;

/// One deterministic evaluation result from the shared REPL executor.
pub struct SqlEval {
    pub value: Option<TurnValue>,
    pub stdout: String,
    pub stderr: String,
}

/// Stateful SQL-console evaluator. Imports, declarations, and bindings live in
/// one ordinary REPL `Session`; no SQL-specific value or execution model is
/// introduced here.
pub struct SqlSession {
    session: super::Session,
    policy: super::ReplPolicy,
    base_dir: PathBuf,
}

impl SqlSession {
    /// Create a console against `base_dir`. Effects remain fail-closed unless
    /// the supplied `ReplFlags` explicitly grant them.
    pub fn new(base_dir: impl AsRef<Path>, flags: super::ReplFlags) -> Self {
        let base_dir = base_dir.as_ref().to_path_buf();
        Self {
            session: super::Session::new(),
            policy: super::ReplPolicy::new(flags, &base_dir),
            base_dir,
        }
    }

    /// Evaluate one already-complete Jet input through the ordinary REPL
    /// classifier, sema, and checked build-and-execute path.
    pub fn execute(&mut self, input: &str) -> Result<SqlEval, Vec<Diagnostic>> {
        jet_driver::boot_mir_eval();
        let normalized = super::normalize_repl_input(input);
        let trimmed = normalized.trim();
        if trimmed.is_empty() {
            return Ok(SqlEval {
                value: None,
                stdout: String::new(),
                stderr: String::new(),
            });
        }

        self.session.step += 1;
        let kind = super::classify(trimmed, self.session.step)?;
        match kind {
            super::InputKind::Empty => Ok(SqlEval {
                value: None,
                stdout: String::new(),
                stderr: String::new(),
            }),
            super::InputKind::Meta(command, _) => Err(vec![Diagnostic::error(
                "E1802",
                format!("SQL console does not accept REPL meta-command `:{command}`"),
                "dot commands are handled by `jet db` before Jet source reaches the shared evaluator"
                    .to_string(),
                "use `.help`, `.schema`, `.plan`, `.stats`, or `.quit` in the SQL console"
                    .to_string(),
                None,
            )]),
            super::InputKind::Item(_) => Err(vec![Diagnostic::error(
                "E1802",
                "SQL console accepts query expressions, not top-level declarations".to_string(),
                "declarations would change the shared session's program surface rather than query a typed source"
                    .to_string(),
                "use `define_item` only for the console's generated typed source, then submit SELECT-backed expressions"
                    .to_string(),
                None,
            )]),
            super::InputKind::Import(source) => self.execute_import(source),
            super::InputKind::Stmts(stmts, suppress, check_src) => {
                self.execute_statements(trimmed, stmts, suppress, check_src)
            }
        }
    }

    /// Add one generated typed source item to this same session. The command
    /// host uses this only for its in-memory row struct; user SQL never exposes
    /// a second declaration evaluator.
    pub fn define_item(&mut self, source: &str) -> Result<(), Vec<Diagnostic>> {
        jet_driver::boot_mir_eval();
        let bundle = super::type_check_item(&self.session, source)?;
        self.session.item_srcs.push(source.to_string());
        super::rebuild_funcs(&mut self.session);
        super::update_core_imports_from_ledger(&bundle, &mut self.session.core_imports);
        Ok(())
    }

    /// Reset the shared session and its effect grants, matching REPL reset
    /// semantics without leaving stale query bindings behind.
    pub fn reset(&mut self) {
        self.session.reset();
        self.policy.session.clear();
    }

    /// Read-only access for host projections that need session provenance.
    pub fn session(&self) -> &super::Session {
        &self.session
    }

    /// The authority root used by this console instance.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    fn execute_import(&mut self, source: String) -> Result<SqlEval, Vec<Diagnostic>> {
        self.session.import_srcs.push(source);
        let bundle = match super::type_check_item(&self.session, "") {
            Ok(bundle) => bundle,
            Err(errors) => {
                self.session.import_srcs.pop();
                return Err(errors);
            }
        };
        super::rebuild_funcs(&mut self.session);
        super::update_core_imports_from_ledger(&bundle, &mut self.session.core_imports);
        Ok(SqlEval {
            value: None,
            stdout: String::new(),
            stderr: String::new(),
        })
    }

    fn execute_statements(
        &mut self,
        input: &str,
        parsed: Vec<crate::AST::Stmt>,
        suppress: bool,
        check_src: String,
    ) -> Result<SqlEval, Vec<Diagnostic>> {
        let mut authorizer = self.policy.authorizer(None);
        let result = super::run_stmts_turn(
            &mut self.session,
            &parsed,
            suppress,
            &check_src,
            input,
            &self.base_dir,
            &mut authorizer,
            true,
        );
        match result {
            super::TurnResult::Done {
                stdout,
                stderr,
                echo,
                bound_name,
                had_effect,
                ..
            } => {
                let summary = echo.as_ref().map(super::echo_text).unwrap_or_default();
                self.session.record_turn_ex(
                    input,
                    super::ReplTurnStatus::Ok,
                    summary,
                    had_effect,
                    bound_name,
                );
                self.session.remember_success(input);
                Ok(SqlEval {
                    value: echo,
                    stdout,
                    stderr,
                })
            }
            super::TurnResult::Failed {
                mut diagnostics,
                report,
                ..
            } => {
                if let Some(report) = report {
                    diagnostics.push(Diagnostic::error(
                        "E2105",
                        report.lines().find(|line| !line.trim().is_empty())
                            .unwrap_or("the program stopped").trim().to_string(),
                        report,
                        "fix the runtime failure before running the query again".to_string(),
                        None,
                    ));
                }
                Err(diagnostics)
            }
            super::TurnResult::Interrupted => Err(vec![Diagnostic::error(
                "E2105",
                "the SQL console turn was interrupted".to_string(),
                "its child build-and-execute run was stopped".to_string(),
                "run the query again".to_string(),
                None,
            )]),
        }
    }
}
