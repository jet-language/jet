//! `core.term` stdout/stderr/stdin + terminal hosts (#1219). TTY writes use the
//! real terminal; non-TTY writes use the resident capture buffers so
//! ProgramOutput matches AOT under the process harness.
//! parity: include path=crates/jet-codegen/src/Prelude/CoreLib/Top/IoLineStream.rs

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.

pub(crate) mod term_prelude {
    include!("../../jet-codegen/src/Prelude/Term.rs");
}

mod human_output_semantics {
    include!("../../jet-codegen/src/Prelude/Core/HumanOutput.rs");
}

// #1480: literal Prelude source for line/byte stdin primitives. The extern "C"
// wrappers below only marshal jit heap i64 handles to/from Rust values and
// call these included functions — no logic is re-encoded here (I9). The
// nested `jet_std` mirrors only the IOError shape these functions construct
// via `.other(...)`; it carries no behavior of its own.
mod io_line_stream {
    

    

    mod jet_std {
        #[derive(Debug)]
        pub enum IOOperation {
            Read,
            Write,
            Flush,
        }
        #[derive(Debug)]
        pub struct IOContext {
            pub operation: IOOperation,
            pub resource: Option<String>,
            pub os_code: Option<i64>,
            pub cause: Option<String>,
        }
        #[derive(Debug)]
        pub enum IOError {
            Other(IOContext),
        }
        impl IOError {
            pub fn other(
                operation: IOOperation,
                resource: Option<String>,
                cause: impl ToString,
            ) -> Self {
                Self::Other(IOContext {
                    operation,
                    resource,
                    os_code: None,
                    cause: Some(cause.to_string()),
                })
            }
        }
    }

    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;

}


/// Reserved source leaves share the Prelude's physical stream operations.
/// Checkpoints, line trimming, and the selected flush flag belong to Core.
pub(crate) fn ambient_core_call(
    module: &str,
    method: &str,
    args: Vec<jet_foundation::AST::CtValue>,
    span: jet_foundation::Diagnostics::Span,
    _resolved_ret: Option<jet_foundation::AST::Type>,
    _sink: Option<&mut jet_codegen::Comptime::DevSink>,
) -> Option<Result<jet_foundation::AST::CtValue, jet_foundation::Diagnostics::Diagnostic>> {
    use jet_foundation::AST::{CtValue, Type};
    use jet_foundation::MIR::MirRuntimeValue;
    if module != "core.term" {
        return None;
    }
    let cause = match (method, args.as_slice()) {
        ("__stdio_write", [CtValue::Str(text), CtValue::Bool(newline), CtValue::Bool(stderr), CtValue::Bool(flush)]) =>
            term_prelude::jet_term_stdio_write_raw(text, *newline, *stderr, *flush),
        ("__stdio_bytes", [CtValue::Bytes(bytes), CtValue::Bool(stderr), CtValue::Bool(flush)]) =>
            term_prelude::jet_term_stdio_bytes_raw(bytes, *stderr, *flush),
        ("__stdio_bytes", [CtValue::List(values), CtValue::Bool(stderr), CtValue::Bool(flush)]) => {
            let bytes = values.iter().map(|value| match value {
                CtValue::Int(byte) => u8::try_from(*byte).ok(),
                _ => None,
            }).collect::<Option<Vec<_>>>()?;
            term_prelude::jet_term_stdio_bytes_raw(&bytes, *stderr, *flush)
        }
        ("__stdio_flush", [CtValue::Bool(stderr)]) =>
            term_prelude::jet_term_stdio_flush_raw(*stderr),
        ("__stdio_fault", [CtValue::Str(selector)]) =>
            return Some(Ok(CtValue::Bool(crate::fault_injection::jet_fault_should_fail(selector)))),
        ("__stdout_terminal", []) =>
            return Some(Ok(CtValue::Bool(term_prelude::jet_term_stdout_is_terminal()))),
        ("__stderr_terminal", []) =>
            return Some(Ok(CtValue::Bool(term_prelude::jet_term_stderr_is_terminal()))),
        ("__stdin_line", []) => {
            let value = match crate::enc_stream::jet_stdin_line_raw() {
                Ok(line) => {
                    let value = line.map_or_else(
                        || MirRuntimeValue::Absent { element: jet_foundation::MIR::MirType::from_kind(jet_foundation::MIR::MirTypeKind::String) },
                        |line| MirRuntimeValue::Present(Box::new(MirRuntimeValue::String(line))),
                    );
                    MirRuntimeValue::Present(Box::new(value))
                }
                Err(error) => MirRuntimeValue::FailedTold(Box::new(crate::Process::mir_io_error(error))),
            };
            return Some(jet_codegen::Comptime::MirBridge::mir_to_ct_value(value, span));
        }
        _ => return None,
    };
    Some(Ok(cause.map_or_else(
        || CtValue::absent(Type::String),
        |cause| CtValue::Present(Box::new(CtValue::Str(cause))),
    )))
}
