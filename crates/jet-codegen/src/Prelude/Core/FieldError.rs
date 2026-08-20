// D-VALIDATE-DECODE1=B: the one `FieldError` rendering for every tier.
//
// AOT's `impl JetShow`/`impl JetDisplay for FieldError`
// (Prelude/CoreLib/JetStd/DataTree.rs), the Cranelift host
// (`jet_jit_decode_error_show`, jet-jit/src/Encoding.rs) and the TIR evaluator
// (`Comptime/CorePureParity.rs`) all marshal the same two fields here. No
// engine re-encodes the path prefix, and `print(errs)`, `print("{errs}")` and
// a nested render cannot drift apart.

pub(crate) fn jet_field_error_kernel_show(path: &str, reason: &str) -> String {
    if path.is_empty() {
        reason.to_string()
    } else {
        format!("at `{path}`: {reason}")
    }
}
