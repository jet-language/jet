// D-SERVICE-JIT1 / I9: the resident adapter enters the same CtValue bridges
// that the TIR evaluator uses. These functions marshal at the tier boundary;
// service and sync policy remains in the canonical Prelude implementations.

use jet_foundation::AST::CtValue;
use jet_foundation::Diagnostics::{Diagnostic, Span};

pub(crate) fn services_apply(
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    jet_codegen::Comptime::ServicesLite::apply(method, args, span)
}

pub(crate) fn services_runtime_apply(
    receiver: &CtValue,
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    jet_codegen::Comptime::ServicesLite::apply_runtime_method(receiver, method, args, span)
}

pub(crate) fn services_take_mut(
    value: CtValue,
) -> Result<(CtValue, CtValue), CtValue> {
    jet_codegen::Comptime::ServicesLite::take_mut_ok(value)
}

pub(crate) fn service_runtime(store: String, retention_ms: i64) -> CtValue {
    let runtime = jet_codegen::Comptime::ServicesLite::jet_services_runtime(store, retention_ms);
    CtValue::Struct {
        type_name: "ServiceRuntime".to_string(),
        fields: vec![
            ("store".to_string(), CtValue::Str(runtime.store)),
            ("retention_ms".to_string(), CtValue::Int(runtime.retention_ms)),
        ],
    }
}

pub(crate) fn sync_apply(
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    jet_codegen::Comptime::SyncLite::apply(method, args, span)
}

pub(crate) fn service_display(value: &CtValue) -> Option<String> {
    jet_codegen::Comptime::display_core_pure_value(value)
}
