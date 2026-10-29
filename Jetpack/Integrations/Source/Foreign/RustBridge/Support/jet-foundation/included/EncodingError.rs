// D-ENCSTREAM-SURFACE1=A: the one `EncodingError` rendering for every tier.
//
// AOT's `impl JetShow for EncodingError` (Prelude/CoreLib/JetStd/CommonTypes.rs),
// the Cranelift host (`jet_jit_encoding_error_show`, jet-jit/src/Encoding.rs) and
// the TIR evaluator (`Comptime/CorePureParity.rs`) all marshal the same seven
// fields here. No engine re-encodes the "at byte" ladder.

pub(crate) fn jet_encoding_error_kernel_show(
    format_name: &str,
    kind_name: &str,
    byte_offset: i64,
    line: Option<i64>,
    column: Option<i64>,
    path: &str,
    reason: &str,
) -> String {
    let mut out = format!("{format_name} {kind_name} at byte {byte_offset}");
    if let Some(line) = line {
        out.push_str(&format!(", line {line}"));
    }
    if let Some(column) = column {
        out.push_str(&format!(", column {column}"));
    }
    if !path.is_empty() {
        out.push_str(&format!(", path {path}"));
    }
    out.push_str(&format!(": {reason}"));
    out
}
