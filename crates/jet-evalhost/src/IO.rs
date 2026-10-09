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
    use super::term_prelude::*;
    use crate::fault_injection::jet_fault_should_fail;
    use crate::CoreHost::{jit_env_value as jet_std_env_get, jit_env_value_raw as jet_env_value_raw};
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
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/IoLineStream.rs");
    include!("../../jet-codegen/src/Prelude/Core/Terminal8Raw.rs");

    use jet_foundation::AST::{CtValue, Type};
    fn optional(value: Option<String>) -> CtValue {
        value.map_or_else(|| CtValue::absent(Type::String), |value| CtValue::Present(Box::new(CtValue::Str(value))))
    }
    fn tuple(fields: Vec<(&str, CtValue)>) -> CtValue {
        CtValue::Struct {
            type_name: format!("({})", fields.iter().map(|(name, _)| *name).collect::<Vec<_>>().join(",")),
            fields: fields.into_iter().map(|(name, value)| (name.into(), value)).collect(),
        }
    }
    pub(crate) fn ambient_core_call(
        module: &str, method: &str, args: Vec<CtValue>,
        _span: jet_foundation::Diagnostics::Span, _resolved_ret: Option<Type>,
        _sink: Option<&mut jet_codegen::Comptime::DevSink>,
    ) -> Option<Result<CtValue, jet_foundation::Diagnostics::Diagnostic>> {
        if module != "core.term" { return None; }
        let value = match (method, args.as_slice()) {
            ("__terminal8_fault", [CtValue::Str(name)]) => CtValue::Bool(jet_terminal8_fault(name)),
            ("__terminal8_write_stdout", [CtValue::Str(text), CtValue::Bool(flush)]) => optional(jet_terminal8_write_stdout(text, *flush)),
            ("__terminal8_write_stderr", [CtValue::Str(text), CtValue::Bool(flush)]) => optional(jet_terminal8_write_stderr(text, *flush)),
            ("__terminal8_line", []) => {
                let (eof, line, cause) = jet_terminal8_line();
                tuple(vec![("eof", CtValue::Bool(eof)), ("line", CtValue::Str(line)), ("cause", optional(cause))])
            }
            ("__terminal8_read_all", []) => {
                let (text, cause) = jet_terminal8_read_all();
                tuple(vec![("text", CtValue::Str(text)), ("cause", optional(cause))])
            }
            ("__terminal8_read_until", [CtValue::Str(delimiter)]) => {
                let (text, cause) = jet_terminal8_read_until(delimiter);
                tuple(vec![("text", CtValue::Str(text)), ("cause", optional(cause))])
            }
            ("__terminal8_take", [CtValue::Int(n)]) => {
                let (bytes, cause) = jet_terminal8_take(*n);
                tuple(vec![("bytes", CtValue::Bytes(bytes)), ("cause", optional(cause))])
            }
            ("__terminal8_env", [CtValue::Str(name)]) => optional(jet_terminal8_env(name)),
            ("__terminal8_env_has", [CtValue::Str(name)]) => CtValue::Bool(jet_terminal8_env_has(name)),
            ("__terminal8_color_mode", []) => CtValue::Int(jet_terminal8_color_mode()),
            ("__terminal8_stdin_tty", []) => CtValue::Bool(jet_term_stdin_is_terminal()),
            ("__terminal8_stdout_tty", []) => CtValue::Bool(jet_term_stdout_is_terminal()),
            ("__terminal8_stderr_tty", []) => CtValue::Bool(jet_term_stderr_is_terminal()),
            ("__terminal8_machine_output", []) => CtValue::Bool(jet_term_machine_output()),
            ("__terminal8_mode_enter", [CtValue::Bool(echo)]) => CtValue::Bool(jet_term_mode_enter(*echo)),
            ("__terminal8_mode_leave", []) => { jet_term_mode_leave(); CtValue::Unit },
            ("__terminal8_stty", []) => jet_terminal8_stty().map_or_else(
                || CtValue::absent(Type::Tuple(vec![("w".into(), Box::new(Type::Int)), ("h".into(), Box::new(Type::Int))])),
                |(w, h)| CtValue::Present(Box::new(tuple(vec![("w", CtValue::Int(w)), ("h", CtValue::Int(h))]))),
            ),
            ("__terminal8_key_bytes", []) => {
                let (bytes, platform) = jet_terminal8_key_bytes();
                tuple(vec![("bytes", CtValue::Bytes(bytes)), ("platform", CtValue::Int(platform))])
            }
            _ => return None,
        };
        Some(Ok(value))
    }

}
pub(crate) use io_line_stream::ambient_core_call as terminal8_ambient_core_call;


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
