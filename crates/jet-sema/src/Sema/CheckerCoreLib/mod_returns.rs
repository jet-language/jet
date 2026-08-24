use super::alloc_ptrs::result_ty;
use crate::AST::Type;

/// D-LIB-CALLGRANT1=A: the resolved return of the one covered Mod method.
pub fn mod_method_return_ty(method: &str) -> Option<Type> {
    (method == "on_tick").then(|| result_ty(Type::Int, Type::String))
}
