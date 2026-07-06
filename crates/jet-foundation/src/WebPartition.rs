//! D-WASM1=A (c123 M1): JS/WASM partition buckets and ABI-safe type checks.

use crate::Syntax;
use crate::AST::Type;

/// Compile target bucket for the web backend (D-WASM1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebBucket {
    Js,
    Wasm,
}

impl WebBucket {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            Syntax::WEB_BUCKET_JS => Some(WebBucket::Js),
            Syntax::WEB_BUCKET_WASM => Some(WebBucket::Wasm),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            WebBucket::Js => Syntax::WEB_BUCKET_JS,
            WebBucket::Wasm => Syntax::WEB_BUCKET_WASM,
        }
    }
}

/// Per-function web partition override (D-WASM1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebPartitionMarker {
    Wasm,
    Js,
    WasmExport,
}

impl WebPartitionMarker {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            Syntax::ATTR_WASM => Some(WebPartitionMarker::Wasm),
            Syntax::ATTR_JS => Some(WebPartitionMarker::Js),
            Syntax::ATTR_WASM_EXPORT => Some(WebPartitionMarker::WasmExport),
            _ => None,
        }
    }

    pub fn bucket(self) -> WebBucket {
        match self {
            WebPartitionMarker::Wasm | WebPartitionMarker::WasmExport => WebBucket::Wasm,
            WebPartitionMarker::Js => WebBucket::Js,
        }
    }

    /// The marker's source spelling (without the `#`), for re-emission by
    /// `jet fmt` — inverse of `parse`.
    pub fn name(self) -> &'static str {
        match self {
            WebPartitionMarker::Wasm => Syntax::ATTR_WASM,
            WebPartitionMarker::Js => Syntax::ATTR_JS,
            WebPartitionMarker::WasmExport => Syntax::ATTR_WASM_EXPORT,
        }
    }
}

/// D-JSBIND1=A: scalars, `String`, and homogeneous `[T]` / `[String: T]` of ABI-safe
/// element types. `Named` / `Apply` Codable structs and enums are checked in sema where
/// the bundle's type definitions are available.
pub fn is_abi_safe_type(ty: &Type) -> bool {
    match ty {
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::IntN { .. }
        | Type::Float32 => true,
        Type::Named(n) if n == "String" => true,
        Type::List(inner) | Type::Option(inner) | Type::Shared(inner) => is_abi_safe_type(inner),
        Type::FixedList { elem, .. } => is_abi_safe_type(elem),
        Type::Map { key, value } => matches!(**key, Type::String) && is_abi_safe_type(value),
        _ => false,
    }
}

/// E-WEB-CROSS-PARTITION: a function in one web bucket calls a function in another.
pub fn web_cross_partition(
    caller: &str,
    callee: &str,
    caller_bucket: WebBucket,
    callee_bucket: WebBucket,
    span: Option<crate::Diagnostics::Span>,
) -> crate::Diagnostics::Diagnostic {
    crate::Diagnostics::Diagnostic::error(
        "E-WEB-CROSS-PARTITION",
        format!(
            "`{caller}` is compiled to {} but calls `{callee}`, which lives in {}",
            caller_bucket.name(),
            callee_bucket.name(),
        ),
        "the web backend keeps DOM/view code in JS and compute in WASM; a direct call across that boundary is not allowed yet"
            .to_string(),
        format!(
            "move the call behind a generated bridge, colocate both functions in the same bucket, or adjust `#{}` / `#{}` markers",
            Syntax::ATTR_TARGET,
            Syntax::ATTR_WASM,
        ),
        span,
    )
}

/// E-WEB-TARGET-BROWSER: a function pinned to Wasm also carries the `Browser` effect.
pub fn web_target_browser(
    fn_name: &str,
    span: Option<crate::Diagnostics::Span>,
) -> crate::Diagnostics::Diagnostic {
    crate::Diagnostics::Diagnostic::error(
        "E-WEB-TARGET-BROWSER",
        format!("`{fn_name}` is pinned to Wasm but uses the `Browser` effect"),
        "the web backend keeps DOM/view code in JS and compute in WASM; a Wasm-pinned function cannot call browser APIs directly"
            .to_string(),
        format!(
            "remove the `#{}` / `#{}` pin, move browser work into a `#{}` function, or drop the browser API calls",
            Syntax::ATTR_WASM,
            Syntax::ATTR_TARGET,
            Syntax::ATTR_JS,
        ),
        span,
    )
}

/// E-WEB-ABI-TYPE: a JS/WASM boundary type is not ABI-safe.
pub fn web_abi_type(
    ty_show: &str,
    context: &str,
    span: Option<crate::Diagnostics::Span>,
) -> crate::Diagnostics::Diagnostic {
    crate::Diagnostics::Diagnostic::error(
        "E-WEB-ABI-TYPE",
        format!("`{ty_show}` cannot cross the JS/WASM boundary {context}"),
        "web exports and imports only admit ABI-safe types (scalars, `String`, `List`/`Map` of ABI-safe values, and `@[Codable]` structs/enums per D-JSBIND1)"
            .to_string(),
        "use a scalar, `String`, a `List`/`Map` of ABI-safe values, or a `@[Codable]` struct/enum whose fields are ABI-safe (D-JSBIND1)"
            .to_string(),
        span,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abi_safe_scalars() {
        assert!(is_abi_safe_type(&Type::Int));
        assert!(is_abi_safe_type(&Type::String));
        assert!(is_abi_safe_type(&Type::List(Box::new(Type::Int))));
        assert!(!is_abi_safe_type(&Type::Named("Point".to_string())));
    }
}
