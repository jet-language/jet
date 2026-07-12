use super::*;
impl<'a> Checker<'a> {
        /// D-DBDRIVER1: `(sql: String, params: [DbValue])` argument elaboration shared
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
    
        /// D-DBDRIVER1: instance methods on a `DbConnection` handle (produced by
        /// `db.open`/`db.open_memory`, mirroring `core.files`'s `open`/`create`
        /// producing a `FileReader`/`FileWriter`). The one generic driver interface:
        /// SQL text plus a separate `[DbValue]` bind list. `query`/`query_one`/
        /// `execute` are fallible (`? DbError`); `begin`/`commit`/`rollback`/`close`
        /// report plain success/failure (`Bool`) — there is nothing else to recover
        /// from a transaction control statement or a close.
        pub(crate) fn check_db_connection_method(
            &mut self,
            method: &str,
            args: &mut [crate::AST::CallArg],
            span: Span,
        ) -> Option<Option<Type>> {
            match method {
                "query" => {
                    self.check_db_sql_params_args("query", args, span);
                    self.record_effect(Effect::Db.name());
                    Some(Some(result_ty(
                        Type::List(Box::new(db_row_ty())),
                        db_error_ty(),
                    )))
                }
                "query_one" => {
                    self.check_db_sql_params_args("query_one", args, span);
                    self.record_effect(Effect::Db.name());
                    Some(Some(result_ty(
                        Type::Option(Box::new(db_row_ty())),
                        db_error_ty(),
                    )))
                }
                "execute" => {
                    self.check_db_sql_params_args("execute", args, span);
                    self.record_effect(Effect::Db.name());
                    Some(Some(result_ty(Type::Int, db_error_ty())))
                }
                "begin" | "commit" | "rollback" | "close" => {
                    if !args.is_empty() {
                        self.diags
                            .push(wrong_core_arity(method, 0, args.len(), span));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                    }
                    self.record_effect(Effect::Db.name());
                    Some(Some(Type::Bool))
                }
                _ => None,
            }
        }
}
