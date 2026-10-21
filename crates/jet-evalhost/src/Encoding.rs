//! `core.encoding.hex` / `base64` / `base32` / `csv` / `json` and `core.crypto.uuid`
//! host shims (#729). Encode mirrors `jet_std_*` in EncodingCodecs.rs; decode
//! calls `jet_foundation::base_encoding_dispatch` (no third algorithm). CSV
//! mirrors `jet_ring_csv_parse` / `jet_ring_csv_render`. JSON parse/render
//! `include!` the canonical `jet_std` parser. UUID mirrors `jet_std_uuid_*`.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.

use crate::{JetDebug, JetDisplay, JetShow};
use jet_foundation::AST::{CtReport, CtValue, Type};
use jet_foundation::Diagnostics::{Diagnostic, Span};
// The Foundation carrier's tier-side accessors are a trait defined by the
// included Prelude `Dyn.rs`; the host adapters below call them by name.


mod encoding_error_rt {
}

mod field_error_rt {
    include!("../../jet-codegen/src/Prelude/Core/FieldError.rs");
}

pub(crate) mod inline_range_rt {
    include!("../../jet-codegen/src/Prelude/Core/InlineRange.rs");
}

// D-CONFIG-ENV1 / I9: source collection is the same Prelude fragment used by
// generated AOT programs and the whole-program interpreter. This module only
// supplies the resident heap adapter below.
mod env_config_rt {
}

pub(crate) mod codec_rt {
    pub(crate) mod jet_std {
        pub(crate) type JetDecimal = jet_foundation::Numeric::CtDecimal;
    }
    include!("../../jet-codegen/src/Prelude/Core/Codec.rs");
}

/// Canonical `jet_std` JSON/Dyn runtime — adapter types, shared algorithm via include!
pub(crate) mod json_rt {
    pub(crate) use jet_foundation::Shape::ShapeProjection;
    pub use jet_foundation::Dyn::Dyn;
    // AOT places the Foundation JSON kernel at the program root; the
    // Dyn/TOML renderers name its quoting entry unqualified.
    use crate::jet_encoding_json::quote_json;

    // JSON.rs starts with `io_error_at`; provide the IO surface it names.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum IOOperation {
        Read,
        Write,
        Flush,
        Connect,
        Accept,
        Close,
        Resolve,
        Codec
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct IOContext {
        pub operation: IOOperation,
        pub resource: Option<String>,
        pub os_code: Option<i64>,
        pub cause: Option<String>
    }

    impl IOContext {
        pub fn new(
            operation: IOOperation,
            resource: Option<String>,
            os_code: Option<i64>,
            cause: Option<String>,
        ) -> Self {
            Self {
                operation,
                resource,
                os_code,
                cause
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub enum IOError {
        InvalidInput(IOContext),
        NotFound(IOContext),
        PermissionDenied(IOContext),
        TimedOut(IOContext),
        Cancelled(IOContext),
        Closed(IOContext),
        Protocol(IOContext),
        Other(IOContext),
        AlreadyExists(IOContext),
        Unsupported(IOContext)
    }

    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/WireOrder.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/DynKind.rs");
    // The one FieldError projection (I9), included beside its caller so
    // `Dyn.rs`'s `JetShow`/`JetDisplay` impls resolve it here too.
    include!("../../jet-codegen/src/Prelude/Core/FieldError.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/Dyn.rs");
    jet_dyn_decode_helpers!();
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/EncodingTypes.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/JSON.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/TOML.rs");

    // I9 marshalling adapters for the exact-`Int` carrier (D-INTBIG1), the
    // same ones the comptime evaluator host uses: evaluator `Dyn` values
    // keep `Int` in a plain `i64`, so the packed carrier collapses to identity.
    // Text still goes through the arbitrary-precision parse the AOT carrier
    // uses, and only genuinely unrepresentable values are refused.
    pub fn jet_int_from_i64(value: i64) -> i64 {
        value
    }

    pub fn jet_int_from_str(value: &str) -> Result<i64, String> {
        jet_foundation::Numeric::CtBigInt::from_str(value)?
            .try_i64()
            .ok_or_else(|| format!("exact Int `{value}` is out of range for this tier"))
    }

    pub fn jet_int_to_string(value: i64) -> String {
        value.to_string()
    }

    pub fn jet_int_to_f64(value: i64) -> f64 {
        value as f64
    }
}
/// Shared CSV/data query kernel. The resident layer only marshals heap
/// `Dyn` values; plan parsing and comparison stay in the Prelude.
pub(crate) mod data_query_rt {
    pub(crate) mod jet_std {
        pub(crate) use super::super::json_rt::Dyn;
        // `DataSchema::infer` / `DataFormat` from CommonTypes.rs, cut by
        // build.rs (`write_data_schema_std`).
        include!(concat!(env!("OUT_DIR"), "/data_schema_std.rs"));
    }
    include!("../../jet-codegen/src/Prelude/Core/LazyTablePlan.rs");
}

mod csv_query_raw {
    use super::json_rt as jet_std;
    include!("../../jet-codegen/src/Prelude/Core/CsvQueryRead.rs");
    pub(crate) mod sql {
        include!("../../jet-codegen/src/Prelude/Core/SqlQuery.rs");
    }
}

/// Canonical YAML via build.rs-stripped include (trailing prelude brace removed).
mod yaml_rt {
    use super::json_rt::Dyn;
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!(concat!(env!("OUT_DIR"), "/yaml_std.rs"));
}


// ── CSV (shared with the AOT Prelude and comptime parser) ─────────────────────

pub(crate) fn csv_parse(
    text: &str,
    delimiter: &str,
    header: bool,
    skip_blank: bool,
) -> Result<Vec<jet_foundation::CsvKernel::CsvRecord>, String> {
    let delimiter = jet_foundation::CsvKernel::delimiter(delimiter)?;
    jet_foundation::CsvKernel::parse(
        text,
        jet_foundation::CsvKernel::CsvOptions {
            delimiter,
            header,
            skip_blank
        },
    )
}

// ── UUID (marshals through the shared Prelude entropy seam) ─────────────────

// ── JSON / Dyn (core.encoding.json) ─────────────────────────────────────

// ── XML via the shared foundation kernel ────────────────────────────────────

// ── CBOR (shared foundation kernel) ─────────────────────────────────────────

fn decode_under_diag(message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0956",
        message.into(),
        "the interpreter decode error adapter rejected the checked operation".to_string(),
        "report this as a compiler bug".to_string(),
        Some(span),
    )
}

fn decode_under_ambient_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    _resolved_ret: Option<Type>,
    _sink: Option<&mut jet_codegen::Comptime::DevSink>,
) -> Option<Result<CtValue, Diagnostic>> {
    if module != "core.encoding" || method != "decode_under" {
        return None;
    }
    let mut args = args.into_iter();
    let segment = match args.next() {
        Some(CtValue::Str(segment)) => segment,
        Some(_) => {
            return Some(Err(decode_under_diag(
                "core.encoding.decode_under expects a String segment",
                span,
            )));
        }
        None => {
            return Some(Err(decode_under_diag(
                "core.encoding.decode_under received no segment",
                span,
            )));
        }
    };
    let result = match args.next() {
        Some(result) => result,
        None => {
            return Some(Err(decode_under_diag(
                "core.encoding.decode_under received no Result",
                span,
            )));
        }
    };
    if args.next().is_some() {
        return Some(Err(decode_under_diag(
            "core.encoding.decode_under received extra arguments",
            span,
        )));
    }
    let result = match result {
        CtValue::Present(value) => CtValue::Present(value),
        CtValue::Failed(CtReport::Told(error)) => CtValue::failed(Box::new(
            jet_codegen::Comptime::decode_error_under(&segment, *error),
        )),
        _ => {
            return Some(Err(decode_under_diag(
                "core.encoding.decode_under received a non-Result value",
                span,
            )));
        }
    };
    Some(Ok(result))
}

// Aliases for the exact existing D-JSON3 clock and stderr leaves. The walk
// and audit formatting stay in Core/Encoding/json.jet.
fn coerce_ambient_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    _span: Span,
    _resolved_ret: Option<Type>,
    sink: Option<&mut jet_codegen::Comptime::DevSink>,
) -> Option<Result<CtValue, Diagnostic>> {
    if module != "core.encoding" {
        return None;
    }
    let value = match (method, args.as_slice()) {
        ("__coerce_now", []) => CtValue::Int(
            crate::enc_stream::runtime::jet_raw_json_coerce_now(),
        ),
        ("__coerce_emit", [CtValue::Str(line)]) => {
            if let Some(sink) = sink {
                sink.stderr.push_str(&crate::IO::term_prelude::jet_term_print_frame(line));
            } else {
                crate::enc_stream::runtime::jet_raw_json_coerce_emit(line);
            }
            CtValue::Unit
        }
        _ => return None,
    };
    Some(Ok(value))
}

fn csv_query_ambient_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    _resolved_ret: Option<Type>,
    _sink: Option<&mut jet_codegen::Comptime::DevSink>,
) -> Option<Result<CtValue, Diagnostic>> {
    if module != "core.encoding" { return None; }
    let result = match (method, args.as_slice()) {
        ("__query_read_bounded", [CtValue::Str(path), CtValue::Int(max)]) => {
            csv_query_raw::jet_data_query_read_bounded(path, *max).map(|sample| {
                let size = i64::try_from(sample.size).map(CtValue::Int).unwrap_or_else(|_| {
                    CtValue::BigInt(jet_foundation::Numeric::CtBigInt::from_str(&sample.size.to_string())
                        .expect("metadata length is an exact unsigned integer"))
                });
                CtValue::Struct { type_name: "__QueryRead".to_string(), fields: vec![
                    ("size".to_string(), size), ("bytes".to_string(), CtValue::Bytes(sample.bytes)),
                ] }
            })
        }
        ("__query_validate_fields", [CtValue::List(fields), CtValue::Str(sql)]) => {
            let fields = fields.iter().map(|field| match field {
                CtValue::Str(field) => Ok(field.clone()),
                _ => Err(decode_under_diag("CSV query header is not [String]", span)),
            }).collect::<Result<Vec<_>, _>>();
            let fields = match fields { Ok(fields) => fields, Err(error) => return Some(Err(error)) };
            csv_query_raw::sql::jet_data_query_validate_fields_raw(&fields, sql).map(|()| CtValue::Unit)
        }
        _ => return None,
    };
    Some(Ok(match result {
        Ok(value) => CtValue::Present(Box::new(value)),
        Err(reason) => CtValue::failed(Box::new(CtValue::Str(reason))),
    }))
}

pub(crate) fn register_interpreter_ambient(
    context: &mut crate::InterpreterAmbientContext,
) {
    context.register_core_call(decode_under_ambient_core_call);
    context.register_core_call(coerce_ambient_core_call);
    context.register_core_call(csv_query_ambient_core_call);
}

