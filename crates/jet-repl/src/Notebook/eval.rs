//! Evaluate one notebook cell against a live REPL [`Session`].

use crate::{
    classify, collect_moved_names, e1802, normalize_repl_input, rebuild_funcs, type_check_item,
    type_check_stmts, update_core_imports_from_ledger, InputKind, Session, ReplTurnStatus,
};
use crate::Comptime::{self, CtValue, DevSink, REPL_FUEL_BUDGET};
use crate::AST::{Func, StructDef};
use crate::Diagnostics::{Diagnostic, Span};
use std::collections::{HashMap, HashSet};
use std::path::Path;

struct TrackingAuthorizer<'a> {
    inner: &'a mut dyn Comptime::ReplAuthorizer,
    observed: bool,
}

impl Comptime::ReplAuthorizer for TrackingAuthorizer<'_> {
    fn preflight(&mut self, request: &Comptime::ReplEffectRequest, span: Span) -> Result<(), Diagnostic> {
        self.inner.preflight(request, span)
    }
    fn authorize(&mut self, request: &Comptime::ReplEffectRequest, span: Span) -> Result<(), Diagnostic> {
        let result = self.inner.authorize(request, span);
        if result.is_ok() {
            self.observed = true;
        }
        result
    }
    fn fs_read(&mut self, path: &str) -> std::io::Result<Vec<u8>> {
        self.inner.fs_read(path)
    }
    fn fs_write(&mut self, path: &str, bytes: &[u8], append: bool) -> std::io::Result<()> {
        self.inner.fs_write(path, bytes, append)
    }
    fn fs_exists(&mut self, path: &str) -> std::io::Result<bool> {
        self.inner.fs_exists(path)
    }
    fn fs_is_dir(&mut self, path: &str) -> std::io::Result<bool> {
        self.inner.fs_is_dir(path)
    }
    fn fs_create_dir(&mut self, path: &str) -> std::io::Result<()> {
        self.inner.fs_create_dir(path)
    }
    fn fs_remove(&mut self, path: &str) -> std::io::Result<()> {
        self.inner.fs_remove(path)
    }
    fn verified_root(&mut self) -> std::io::Result<std::fs::File> {
        self.inner.verified_root()
    }
    fn read_input(&mut self, prompt: &str) -> std::io::Result<String> {
        self.inner.read_input(prompt)
    }
    fn reset_session(&mut self) {
        self.inner.reset_session()
    }
}

#[derive(Clone, Debug)]
pub struct EvalResult {
    pub text: String,
    pub status: ReplTurnStatus,
    pub had_effect: bool,
    pub quit: bool,
    pub value: Option<CtValue>,
}

/// Run one input after the notebook has supplied its complete declaration set.
pub(crate) fn evaluate_step_with_items(
    session: &mut Session,
    input: &str,
    base_dir: &Path,
    authorizer: &mut dyn Comptime::ReplAuthorizer,
    notebook_items_preloaded: bool,
) -> EvalResult {
    jet_driver::boot_tir_eval();
    let normalized = normalize_repl_input(input);
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return EvalResult {
            text: String::new(),
            status: ReplTurnStatus::Ok,
            had_effect: false,
            quit: false,
            value: None,
        };
    }

    session.step += 1;
    let mut out = String::new();

    let kind = match classify(trimmed, session.step) {
        Ok(k) => k,
        Err(ds) => {
            for d in &ds {
                out.push_str(&format!("error [{}]: {}\n", d.code, d.what));
            }
            session.record_turn(
                trimmed,
                ReplTurnStatus::Error,
                ds.iter()
                    .map(|d| format!("{}: {}", d.code, d.what))
                    .collect::<Vec<_>>()
                    .join("; "),
            );
            return EvalResult {
                text: out,
                status: ReplTurnStatus::Error,
                had_effect: false,
                quit: false,
                value: None,
            };
        }
    };

    match kind {
        InputKind::Empty => EvalResult {
            text: out,
            status: ReplTurnStatus::Ok,
            had_effect: false,
            quit: false,
            value: None,
        },

        InputKind::Meta(cmd, _) => {
            if matches!(cmd.as_str(), "quit" | "q" | "exit") {
                out.push_str("bye\n");
                return EvalResult {
                    text: out,
                    status: ReplTurnStatus::Ok,
                    had_effect: false,
                    quit: true,
                    value: None,
                };
            }
            out.push_str(&format!(
                "error: notebook cells do not run meta-command `:{cmd}`\n"
            ));
            session.record_turn(trimmed, ReplTurnStatus::Error, format!("meta:{cmd}"));
            EvalResult {
                text: out,
                status: ReplTurnStatus::Error,
                had_effect: false,
                quit: false,
                value: None,
            }
        }

        InputKind::Reject(feature) => {
            let d = e1802(&feature);
            out.push_str(&format!("error [E1802]: {}\n", d.what));
            session.record_turn(trimmed, ReplTurnStatus::Error, "E1802".to_string());
            EvalResult {
                text: out,
                status: ReplTurnStatus::Error,
                had_effect: false,
                quit: false,
                value: None,
            }
        }

        InputKind::Item(src) => {
            let bundle = match type_check_item(
                session,
                if notebook_items_preloaded { "" } else { &src },
            ) {
                Ok(bundle) => bundle,
                Err(errors) => {
                    for d in &errors {
                        out.push_str(&format!("error [{}]: {}\n", d.code, d.what));
                    }
                    session.record_turn(
                        trimmed,
                        ReplTurnStatus::Error,
                        errors
                            .iter()
                            .map(|d| format!("{}: {}", d.code, d.what))
                            .collect::<Vec<_>>()
                            .join("; "),
                    );
                    return EvalResult {
                        text: out,
                        status: ReplTurnStatus::Error,
                        had_effect: false,
                        quit: false,
                        value: None,
                    };
                }
            };
            if !notebook_items_preloaded {
                session.item_srcs.push(src);
            }
            rebuild_funcs(session);
            update_core_imports_from_ledger(&bundle, &mut session.core_imports);
            out.push_str("ok\n");
            session.record_turn(trimmed, ReplTurnStatus::Ok, "ok".to_string());
            session.remember_success(trimmed);
            EvalResult {
                text: out,
                status: ReplTurnStatus::Ok,
                had_effect: false,
                quit: false,
                value: None,
            }
        }

        InputKind::Import(src) => {
            session.import_srcs.push(src.clone());
            let bundle = match type_check_item(session, "") {
                Ok(bundle) => bundle,
                Err(errors) => {
                    session.import_srcs.pop();
                    for d in &errors {
                        out.push_str(&format!("error [{}]: {}\n", d.code, d.what));
                    }
                    session.record_turn(
                        trimmed,
                        ReplTurnStatus::Error,
                        errors
                            .iter()
                            .map(|d| format!("{}: {}", d.code, d.what))
                            .collect::<Vec<_>>()
                            .join("; "),
                    );
                    return EvalResult {
                        text: out,
                        status: ReplTurnStatus::Error,
                        had_effect: false,
                        quit: false,
                        value: None,
                    };
                }
            };
            rebuild_funcs(session);
            update_core_imports_from_ledger(&bundle, &mut session.core_imports);
            out.push_str("ok\n");
            session.record_turn(trimmed, ReplTurnStatus::Ok, "ok".to_string());
            session.remember_success(trimmed);
            EvalResult {
                text: out,
                status: ReplTurnStatus::Ok,
                had_effect: false,
                quit: false,
                value: None,
            }
        }

        InputKind::Stmts(stmts, suppress, _check_src) => {
            let checked_stmts = match type_check_stmts(session, &stmts, session.step) {
                Ok(s) => s,
                Err(errors) => {
                    for d in &errors {
                        out.push_str(&format!("error [{}]: {}\n", d.code, d.what));
                    }
                    session.record_turn(
                        trimmed,
                        ReplTurnStatus::Error,
                        errors
                            .iter()
                            .map(|d| format!("{}: {}", d.code, d.what))
                            .collect::<Vec<_>>()
                            .join("; "),
                    );
                    return EvalResult {
                        text: out,
                        status: ReplTurnStatus::Error,
                        had_effect: false,
                        quit: false,
                        value: None,
                    };
                }
            };

            let session_binding_names: HashSet<String> = session.scope.keys().cloned().collect();
            let newly_moved = collect_moved_names(&stmts, &session_binding_names, &session.scope);
            let before_keys: HashSet<String> = session.scope.keys().cloned().collect();
            let funcs: HashMap<String, &Func> =
                session.func_defs.iter().map(|(k, v)| (k.clone(), v)).collect();
            let structs: HashMap<String, &StructDef> =
                session.struct_defs.iter().map(|(k, v)| (k.clone(), v)).collect();

            let mut trial_scope = session.scope.clone();
            let mut sink = DevSink::new();
            let mut tracking = TrackingAuthorizer {
                inner: authorizer,
                observed: false,
            };
            let result = if Comptime::repl_interruptible_turn_active() {
                Comptime::run_repl_step_interruptible(
                    &stmts,
                    &funcs,
                    base_dir,
                    &mut sink,
                    &mut trial_scope,
                    REPL_FUEL_BUDGET,
                    suppress,
                    &session.core_imports,
                    &structs,
                    &session.binding_types,
                    &mut tracking,
                )
            } else {
                Comptime::run_repl_step(
                    &stmts,
                    &funcs,
                    base_dir,
                    &mut sink,
                    &mut trial_scope,
                    REPL_FUEL_BUDGET,
                    suppress,
                    &session.core_imports,
                    &structs,
                    &session.binding_types,
                    &mut tracking,
                )
                .map_err(Comptime::ReplStepError::Diagnostic)
            };

            match result {
                Ok(echo_val) => {
                    for name in &newly_moved {
                        trial_scope.remove(name);
                    }
                    session.scope = trial_scope;
                    session.moved_names.extend(newly_moved);
                    let raw = trimmed.trim_end_matches(';').trim().to_string();
                    if !raw.is_empty() && !raw.starts_with("__repl_echo__") {
                        session.stmt_srcs.push(format!("{raw};"));
                    }
                    let new_names: Vec<String> = session
                        .scope
                        .keys()
                        .filter(|k| !before_keys.contains(*k) && *k != "__repl_echo__")
                        .cloned()
                        .collect();
                    session.record_stmts(&checked_stmts);

                    let mut summary = String::new();
                    if !sink.stdout.is_empty() {
                        out.push_str(&sink.stdout);
                        summary.push_str(&sink.stdout);
                    }
                    if !sink.stderr.is_empty() {
                        out.push_str(&sink.stderr);
                        summary.push_str(&sink.stderr);
                    }
                    let result_value = echo_val.clone();
                    if let Some(v) = echo_val {
                        if !matches!(v, CtValue::Unit) {
                            let shown = crate::display_value(&v);
                            out.push_str(&shown);
                            out.push('\n');
                            summary.push_str(&shown);
                        }
                    }
                    let had_effect =
                        tracking.observed || !sink.stdout.is_empty() || !sink.stderr.is_empty();
                    let bound_name = match new_names.as_slice() {
                        [only] => Some(only.clone()),
                        _ => None,
                    };
                    session.record_turn_ex(
                        trimmed,
                        ReplTurnStatus::Ok,
                        summary,
                        had_effect,
                        bound_name,
                    );
                    session.remember_success(trimmed);
                    EvalResult {
                        text: out,
                        status: ReplTurnStatus::Ok,
                        had_effect,
                        quit: false,
                        value: result_value,
                    }
                }
                Err(Comptime::ReplStepError::Interrupted) => {
                    let had_effect =
                        tracking.observed || !sink.stdout.is_empty() || !sink.stderr.is_empty();
                    out.push_str("Interrupted. External effects already performed were not rolled back.\n");
                    session.record_turn_ex(
                        trimmed,
                        ReplTurnStatus::Interrupted,
                        "Interrupted".to_string(),
                        had_effect,
                        None,
                    );
                    EvalResult {
                        text: out,
                        status: ReplTurnStatus::Interrupted,
                        had_effect,
                        quit: false,
                        value: None,
                    }
                }
                Err(Comptime::ReplStepError::Diagnostic(d)) => {
                    out.push_str(&format!("error [{}]: {}\n", d.code, d.what));
                    session.record_turn(
                        trimmed,
                        ReplTurnStatus::Error,
                        format!("{}: {}", d.code, d.what),
                    );
                    EvalResult {
                        text: out,
                        status: ReplTurnStatus::Error,
                        had_effect: false,
                        quit: false,
                        value: None,
                    }
                }
            }
        }
    }
}
