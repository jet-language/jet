use super::alloc_ptrs::result_ty;
use crate::AST::Type;

/// D-DEP-WASM1=A: the resolved return type of a covered `Plugin` method, read
/// from `check_plugin_method`'s authoritative match — a pure lookup for
/// codegen's TIR totality bookkeeping (mirrors `db_connection_method_return_ty`).
pub fn plugin_method_return_ty(method: &str) -> Option<Type> {
    match method {
        "call" => Some(result_ty(Type::Float, Type::String)),
        "call_int" => Some(result_ty(Type::Int, Type::String)),
        "call_bool" => Some(result_ty(Type::Bool, Type::String)),
        "call_text" => Some(result_ty(Type::String, Type::String)),
        _ => None,
    }
}
