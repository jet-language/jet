impl<'a> Checker<'a> {
        /// D-DEP-WASM1=A / D-PLUGIN1=B (c81): `(name: String, args: [T]) -> T ?
        /// String` argument elaboration shared by `.call`/`.call_int` — a plugin
        /// export name plus a homogeneous scalar argument list. v1 supports
        /// exactly two scalar shapes (`Float` via `.call`, `Int` via `.call_int`);
        /// see `Prelude/Plugin.rs` for why Bool/Text aren't wired yet.
        fn check_plugin_call_args(
            &mut self,
            name: &str,
            arg_ty: &Type,
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
            let list_ty = Type::List(Box::new(arg_ty.clone()));
            self.expect_core_arg(name, 0, &Type::String, &mut args[0]);
            self.expect_core_arg(name, 1, &list_ty, &mut args[1]);
        }
    
        /// D-DEP-WASM1=A / D-PLUGIN1=B (c81): instance methods on a `Plugin` handle
        /// (produced by `core.plugin`'s `load`). `call`/`call_int` are fallible
        /// (`? String`, naming a missing export or a param/type mismatch against
        /// the plugin's actual `.wit` signature) — the sandboxed loader never
        /// crashes the host program, it reports (I2).
        pub(crate) fn check_plugin_method(
            &mut self,
            method: &str,
            args: &mut [crate::AST::CallArg],
            span: Span,
        ) -> Option<Option<Type>> {
            match method {
                "call" => {
                    self.check_plugin_call_args("call", &Type::Float, args, span);
                    self.record_effect(Effect::Exec.name());
                    Some(Some(result_ty(Type::Float, Type::String)))
                }
                "call_int" => {
                    self.check_plugin_call_args("call_int", &Type::Int, args, span);
                    self.record_effect(Effect::Exec.name());
                    Some(Some(result_ty(Type::Int, Type::String)))
                }
                _ => None,
            }
        }
}
