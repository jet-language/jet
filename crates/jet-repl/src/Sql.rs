//! SQL-console session seam (D-DX-SQLSHELL1=A).
//!
//! This module deliberately does not parse SQL or execute a second evaluator.
//! It owns only the typed REPL session lifetime and returns the same `CtValue`
//! plus diagnostics used by the ordinary `jet repl` path. `Source/CmdDb.rs`
//! marshals files, dot commands, and output modes around this seam.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::Comptime::{CtValue, DevSink, REPL_FUEL_BUDGET};
use crate::Diagnostics::Diagnostic;
use crate::AST::{Func, StructDef};

/// One deterministic evaluation result from the shared REPL interpreter.
pub struct SqlEval {
    pub value: Option<CtValue>,
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
    /// classifier, sema, and comptime tree-walker.
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
            super::InputKind::Reject(feature) => Err(vec![super::e1802(&feature)]),
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

    /// Drop all generated row declarations and query bindings.
    pub fn clear_items(&mut self) {
        self.session.item_srcs.clear();
        self.session.func_defs.clear();
        self.session.struct_defs.clear();
        self.session.scope.clear();
        self.session.sema_stmts.clear();
        self.session.binding_types.clear();
        self.session.moved_names.clear();
        self.session.stmt_srcs.clear();
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
        let (checked_stmts, checked_core_imports) =
            super::type_check_stmts(&self.session, &parsed, self.session.step, &check_src)?;
        let session_binding_names: HashSet<String> = self.session.scope.keys().cloned().collect();
        let newly_moved = super::collect_moved_names(
            &parsed,
            &session_binding_names,
            &self.session.scope,
        );
        let executable = super::repl_executable_stmts(checked_stmts.clone());
        let mut core_imports = self.session.core_imports.clone();
        core_imports.extend(checked_core_imports);
        let funcs: HashMap<String, &Func> = self
            .session
            .func_defs
            .iter()
            .map(|(name, function)| (name.clone(), function))
            .collect();
        let structs: HashMap<String, &StructDef> = self
            .session
            .struct_defs
            .iter()
            .map(|(name, structure)| (name.clone(), structure))
            .collect();
        let mut sink = DevSink::new();
        let mut authorizer = self.policy.authorizer(None);
        let mut trial_scope = self.session.scope.clone();
        let result = crate::Comptime::run_repl_step(
            &executable,
            &funcs,
            &self.base_dir,
            &mut sink,
            &mut trial_scope,
            REPL_FUEL_BUDGET,
            suppress,
            &core_imports,
            &structs,
            &self.session.binding_types,
            &mut authorizer,
        );
        match result {
            Ok(value) => {
                for name in &newly_moved {
                    trial_scope.remove(name);
                }
                self.session.scope = trial_scope;
                self.session.moved_names.extend(newly_moved);
                let raw = input.trim_end_matches(';').trim();
                if !raw.is_empty() && !raw.starts_with("__repl_echo__") {
                    self.session.stmt_srcs.push(format!("{raw};"));
                }
                self.session.record_stmts(&checked_stmts);
                let summary = value
                    .as_ref()
                    .filter(|value| !matches!(**value, CtValue::Unit))
                    .map(super::display_value)
                    .unwrap_or_default();
                self.session.record_turn_ex(
                    input,
                    super::ReplTurnStatus::Ok,
                    summary,
                    !sink.stdout.is_empty() || !sink.stderr.is_empty(),
                    None,
                );
                self.session.remember_success(input);
                Ok(SqlEval {
                    value,
                    stdout: sink.stdout,
                    stderr: sink.stderr,
                })
            }
            Err(diagnostic) => {
                let diagnostic = super::restore_move_diagnostic(diagnostic, &self.session.moved_names);
                Err(vec![if diagnostic.code == "E2202" {
                    super::e1801(REPL_FUEL_BUDGET)
                } else {
                    diagnostic
                }])
            }
        }
    }
}
