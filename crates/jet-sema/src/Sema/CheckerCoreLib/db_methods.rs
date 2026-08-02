use crate::AST::Type;
use crate::Diagnostics::Span;
use crate::Sema::Checker;
use crate::Sema::Effects::Effect;
use crate::Syntax;
use super::alloc_ptrs::{db_error_ty, db_row_ty, result_ty};
use super::serde_diags::wrong_core_arity;

/// D-EFFDBREAD1=A: the `DB.Read` effect leaf a database read call proves. Unlike
/// the general rule that Core calls stay tagged with a bare root (D-EFFTREE1),
/// `core.db`'s own closed method table is precise enough to infer read/write
/// leaves — the same shape as rustc special-casing a small list of known
/// intrinsics. This is what lets a `#(DB.Read)` query function actually check,
/// the read-footprint proof a live query (D-LIVEQUERY1) rides on. A hidden write
/// inside such a function is then caught by the existing E0740 check with no new
/// diagnostic code.
fn db_read() -> String {
    format!("{}.Read", Effect::DB.name())
}

/// D-EFFDBREAD1=A: the `DB.Write` effect leaf a database write call proves
/// (`execute` — arbitrary DDL/DML). Sibling of `DB.Read`; a `#(DB.Read)` bound
/// does not cover it (E0740), which is exactly how a write hiding inside a
/// declared read-only query is rejected.
fn db_write() -> String {
    format!("{}.Write", Effect::DB.name())
}
impl<'a> Checker<'a> {
        /// D-DBDRIVER1: `(sql: String, params: [DBValue])` argument elaboration shared
        /// by `.query`/`.query_one`/`.execute` — SQL text plus a separate bind list,
        /// never a raw execute(sql) escape (the ratified build plan is explicit that
        /// a generic `execute_raw(sql)` must not exist).
        fn check_db_sql_params_args(
            &mut self,
            name: &str,
            args: &mut [crate::AST::CallArg],
            span: Span,
        ) {
            if args.len() != 2 {
                self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return;
            }
            let params_ty = Type::List(Box::new(Type::Named(Syntax::TYPE_DB_VALUE.to_string())));
            self.expect_core_arg(name, 0, &Type::String, &mut args[0]);
            self.expect_core_arg(name, 1, &params_ty, &mut args[1]);
        }
    
        /// D-DBPOLICY-BIND1: an unscoped connection can establish a typed policy
        /// scope and control a transaction, but row reads/writes are only exposed
        /// on the returned `DBScope`. This keeps a connection from bypassing the
        /// policy simply by retaining the original handle.
        pub(crate) fn check_db_connection_method(
            &mut self,
            method: &str,
            args: &mut [crate::AST::CallArg],
            span: Span,
        ) -> Option<Option<Type>> {
            match method {
                "with_policy" => {
                    if args.len() != 2 {
                        self.diags.push(wrong_core_arity("with_policy", 2, args.len(), span));
                        for arg in args.iter_mut() {
                            self.infer(&mut arg.expr);
                        }
                    } else {
                        self.expect_core_arg(
                            "with_policy",
                            0,
                            &Type::Named("RowPolicy".to_string()),
                            &mut args[0],
                        );
                        self.expect_core_arg("with_policy", 1, &Type::String, &mut args[1]);
                    }
                    self.record_effect(Effect::DB.name(), span);
                    Some(Some(Type::Named("DBScope".to_string())))
                }
                "begin" | "commit" | "rollback" | "close" => {
                    if !args.is_empty() {
                        self.diags
                            .push(wrong_core_arity(method, 0, args.len(), span));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                    }
                    // D-EFFDBREAD1=A: transaction-control and close calls neither
                    // read nor write rows themselves, so they keep the plain `DB`
                    // root (an ancestor of both leaves).
                    self.record_effect(Effect::DB.name(), span);
                    Some(Some(Type::Bool))
                }
                _ => None,
            }
        }

        pub(crate) fn check_db_scope_method(
            &mut self,
            method: &str,
            args: &mut [crate::AST::CallArg],
            span: Span,
        ) -> Option<Option<Type>> {
            match method {
                "query" => {
                    self.check_db_sql_params_args("query", args, span);
                    self.record_effect(&db_read(), span);
                    Some(Some(result_ty(
                        Type::List(Box::new(db_row_ty())),
                        db_error_ty(),
                    )))
                }
                "query_one" => {
                    self.check_db_sql_params_args("query_one", args, span);
                    self.record_effect(&db_read(), span);
                    Some(Some(result_ty(
                        Type::Option(Box::new(db_row_ty())),
                        db_error_ty(),
                    )))
                }
                "execute" => {
                    self.check_db_sql_params_args("execute", args, span);
                    self.record_effect(&db_write(), span);
                    Some(Some(result_ty(Type::Int, db_error_ty())))
                }
                "live" => {
                    self.check_db_sql_params_args("live", args, span);
                    self.record_effect(&db_read(), span);
                    Some(Some(result_ty(Type::Named("LiveQuery".to_string()), db_error_ty())))
                }
                "begin" | "commit" | "rollback" | "close" => {
                    if !args.is_empty() {
                        self.diags.push(wrong_core_arity(method, 0, args.len(), span));
                        for arg in args.iter_mut() {
                            self.infer(&mut arg.expr);
                        }
                    }
                    self.record_effect(Effect::DB.name(), span);
                    Some(Some(Type::Bool))
                }
                _ => None,
            }
        }
}
