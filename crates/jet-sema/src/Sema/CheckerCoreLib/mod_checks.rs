use crate::AST::Type;
use crate::Diagnostics::Span;
use crate::Sema::Checker;
use crate::Sema::Effects::Effect;
use super::alloc_ptrs::result_ty;
use super::serde_diags::wrong_core_arity;

impl<'a> Checker<'a> {
    /// D-LIB-CALLGRANT1=A: a loaded Mod exposes one deliberately small first
    /// call surface. The host grants the load; the typed method keeps the
    /// exported scalar boundary visible to sema and every lowerer.
    pub(crate) fn check_mod_method(
        &mut self,
        method: &str,
        args: &mut [crate::AST::CallArg],
        span: Span,
    ) -> Option<Option<Type>> {
        if method != "on_tick" {
            return None;
        }
        if args.len() != 1 {
            self.diags
                .push(wrong_core_arity("Mod.on_tick", 1, args.len(), span));
            for arg in args {
                self.infer(&mut arg.expr);
            }
        } else {
            self.expect_core_arg("Mod.on_tick", 0, &Type::Int, &mut args[0]);
        }
        self.record_effect(Effect::Exec.name(), span);
        Some(Some(result_ty(Type::Int, Type::String)))
    }
}

