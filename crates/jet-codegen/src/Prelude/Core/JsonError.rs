// D-JSON-ERROR-SHOW1: one JSON parser error rendering for every tier.
//
// AOT's `JSONError` implementation, the Cranelift host, and the canonical
// TIR evaluator all marshal the same two fields here. The host adapters own
// only heap access; they do not restate this text.
pub(crate) fn jet_json_error_kernel_show(line: i64, message: &str) -> String {
    format!("line {line}: {message}")
}
