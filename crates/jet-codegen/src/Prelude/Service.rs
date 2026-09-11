// D-SERVICE-JIT1 / I9: the resident adapter enters the same CtValue bridges
// that the TIR evaluator uses. These functions marshal at the tier boundary;
// service and sync policy remains in the canonical Prelude implementations.

use jet_foundation::AST::CtValue;
use jet_foundation::Diagnostics::{Diagnostic, Span};
pub(crate) type JetServiceEndpoint =
    jet_codegen::Comptime::ServicesLite::JetServiceEndpoint;

fn workflow_wait(nanos: i64) -> jet_codegen::Comptime::ServicesLite::JetServiceWorkflowWait<()> {
    match jet_codegen::scheduler::jet_scheduler_wait_without_unwind(|| {
        jet_codegen::scheduler::jet_std_time_sleep_duration_ns(nanos)
    }) {
        jet_codegen::scheduler::JetSchedulerWait::Ready(()) => {
            jet_codegen::Comptime::ServicesLite::JetServiceWorkflowWait::Ready(())
        }
        jet_codegen::scheduler::JetSchedulerWait::Cancelled => {
            jet_codegen::Comptime::ServicesLite::JetServiceWorkflowWait::Cancelled
        }
        jet_codegen::scheduler::JetSchedulerWait::Deadline(reason) => {
            jet_codegen::Comptime::ServicesLite::JetServiceWorkflowWait::Deadline(reason)
        }
        jet_codegen::scheduler::JetSchedulerWait::Panicked(reason) => {
            jet_codegen::Comptime::ServicesLite::JetServiceWorkflowWait::Panicked(reason)
        }
    }
}

pub(crate) fn services_apply(
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    jet_codegen::Comptime::ServicesLite::with_workflow_wait(workflow_wait, || {
        jet_codegen::Comptime::ServicesLite::apply(method, args, span)
    })
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

pub(crate) fn service_show(value: &CtValue) -> Option<String> {
    jet_codegen::Comptime::ServicesLite::service_show_value(value)
        .or_else(|| jet_codegen::Comptime::SyncLite::sync_show_value(value))
}

pub(crate) fn service_display(value: &CtValue) -> Option<String> {
    service_show(value).or_else(|| jet_codegen::Comptime::display_core_pure_value(value))
}

use jet_foundation::MIR::MirRuntimeValue;

pub(crate) fn services_apply_runtime(
    method: &str,
    args: &[MirRuntimeValue],
    span: Span,
) -> Result<MirRuntimeValue, Diagnostic> {
    jet_codegen::Comptime::ServicesLite::with_workflow_wait(workflow_wait, || {
        jet_codegen::Comptime::ServicesLite::apply_runtime(method, args, span)
    })
}

pub(crate) fn services_start_runtime<F>(
    receiver: &MirRuntimeValue,
    span: Span,
    dispatch: F,
) -> Result<MirRuntimeValue, Diagnostic>
where
    F: FnMut(
        &str,
        &JetServiceEndpoint,
    ) -> Result<(), jet_codegen::Comptime::ServicesLite::JetServiceError>,
{
    jet_codegen::Comptime::ServicesLite::with_workflow_wait(workflow_wait, || {
        jet_codegen::Comptime::ServicesLite::apply_runtime_start_with_dispatcher(
            receiver, span, dispatch,
        )
    })
}

pub(crate) fn services_runtime_apply_runtime(
    receiver: &MirRuntimeValue,
    method: &str,
    args: &[MirRuntimeValue],
    span: Span,
) -> Result<MirRuntimeValue, Diagnostic> {
    jet_codegen::Comptime::ServicesLite::apply_runtime_method_mir(receiver, method, args, span)
}

pub(crate) fn services_take_mut_runtime(
    value: MirRuntimeValue,
) -> Result<(MirRuntimeValue, MirRuntimeValue), MirRuntimeValue> {
    jet_codegen::Comptime::ServicesLite::take_mut_runtime(value)
}

pub(crate) fn service_runtime_runtime(store: String, retention_ms: i64) -> MirRuntimeValue {
    jet_codegen::Comptime::ServicesLite::service_runtime_value(store, retention_ms)
}

pub(crate) fn sync_apply_runtime(
    method: &str,
    args: &[MirRuntimeValue],
    span: Span,
) -> Result<MirRuntimeValue, Diagnostic> {
    let args = args
        .iter()
        .map(jet_codegen::Comptime::ServicesLite::runtime_to_ct_value)
        .collect::<Result<Vec<_>, _>>()?;
    jet_codegen::Comptime::SyncLite::apply(method, &args, span)
        .and_then(|value| jet_codegen::Comptime::ServicesLite::ct_to_runtime_value(&value))
}

pub(crate) fn service_endpoint_runtime(
    value: &MirRuntimeValue,
) -> Option<jet_codegen::Comptime::ServicesLite::JetServiceEndpoint> {
    jet_codegen::Comptime::ServicesLite::service_endpoint_runtime(value)
}

pub(crate) fn jet_services_execution_scope(
    endpoint: &JetServiceEndpoint,
) -> Result<
    jet_codegen::Comptime::ServicesLite::JetServiceExecutionScope,
    jet_codegen::Comptime::ServicesLite::JetServiceError,
> {
    jet_codegen::Comptime::ServicesLite::jet_services_execution_scope(endpoint)
}

pub(crate) fn service_display_runtime(value: &MirRuntimeValue) -> Option<String> {
    if let Some(rendered) =
        jet_codegen::Comptime::ServicesLite::service_display_value_runtime(value)
    {
        return Some(rendered);
    }
    let value = jet_codegen::Comptime::ServicesLite::runtime_to_ct_value(value).ok()?;
    jet_codegen::Comptime::SyncLite::sync_show_value(&value)
        .or_else(|| jet_codegen::Comptime::display_core_pure_value(&value))
}
