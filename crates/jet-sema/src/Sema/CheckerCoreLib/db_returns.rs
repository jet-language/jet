use super::*;
/// D-DBDRIVER1: the resolved return type of a covered `DbConnection` method, read
/// from `check_db_connection_method`'s authoritative match (arity/diagnostics
/// already ran in sema; this is a pure lookup for codegen's TIR totality
/// bookkeeping, mirroring `handle_method_return_ty`'s other sources).
pub fn db_connection_method_return_ty(method: &str) -> Option<Type> {
    match method {
        "query" => Some(result_ty(Type::List(Box::new(db_row_ty())), db_error_ty())),
        "query_one" => Some(result_ty(
            Type::Option(Box::new(db_row_ty())),
            db_error_ty(),
        )),
        "execute" => Some(result_ty(Type::Int, db_error_ty())),
        "begin" | "commit" | "rollback" | "close" => Some(Type::Bool),
        _ => None,
    }
}
