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
// included Prelude `DataTree.rs`; the host adapters below call them by name.


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

/// Canonical `jet_std` JSON/DataTree runtime — adapter types, shared algorithm via include!
pub(crate) mod json_rt {
    pub(crate) use jet_foundation::Shape::ShapeProjection;
    pub use jet_foundation::DataTree::DataTree;
    // AOT places the Foundation JSON kernel at the program root; the
    // DataTree/TOML renderers name its quoting entry unqualified.
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
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/DataTreeKind.rs");
    // The one FieldError projection (I9), included beside its caller so
    // `DataTree.rs`'s `JetShow`/`JetDisplay` impls resolve it here too.
    include!("../../jet-codegen/src/Prelude/Core/FieldError.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/DataTree.rs");
    jet_datatree_decode_helpers!();
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/EncodingTypes.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/JSON.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/TOML.rs");

    // I9 marshalling adapters for the exact-`Int` carrier (D-INTBIG1), the
    // same ones the comptime evaluator host uses: evaluator `DataTree` values
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

    pub fn parse_datatree_typed_ordered(text: &str) -> Result<DataTree, EncodingError> {
        parse_json_typed_datatree(text)
    }

}
/// Shared CSV/data query kernel. The resident layer only marshals heap
/// `DataTree` values; plan parsing and comparison stay in the Prelude.
pub(crate) mod data_query_rt {
    pub(crate) mod jet_std {
        pub(crate) use super::super::json_rt::DataTree;
        // `DataSchema::infer` / `DataFormat` from CommonTypes.rs, cut by
        // build.rs (`write_data_schema_std`).
        include!(concat!(env!("OUT_DIR"), "/data_schema_std.rs"));
    }
    include!("../../jet-codegen/src/Prelude/Core/LazyTablePlan.rs");
}

/// Canonical YAML via build.rs-stripped include (trailing prelude brace removed).
mod yaml_rt {
    use super::json_rt::DataTree;
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

// ── JSON / DataTree (core.encoding.json) ─────────────────────────────────────

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

pub(crate) fn register_interpreter_ambient(
    context: &mut crate::InterpreterAmbientContext,
) {
    context.register_core_call(decode_under_ambient_core_call);
}

