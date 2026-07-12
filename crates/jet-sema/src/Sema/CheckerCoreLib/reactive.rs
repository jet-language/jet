use crate::AST::Type;
use crate::Diagnostics::Span;
use crate::Sema::Checker;
use super::serde_diags::reactive_bad_value_type;
impl<'a> Checker<'a> {
        /// D-REACT1=B: a reactive `Signal<T>`/`Derived<T>` holds ordinary data that can
        /// be cloned to its dependents. Reject a function-typed value (E2913); everything
        /// else is admitted in sema (the codegen coverage gate handles the precise subset).
        pub(crate) fn reactive_value_ok(&mut self, ty: &Type, span: Span, kind: &str) -> bool {
            if matches!(ty, Type::Fn { .. }) {
                self.diags.push(reactive_bad_value_type(kind, ty, span));
                return false;
            }
            true
        }
    
}
