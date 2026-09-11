// Shared core.log trace context state.
//
// The AOT prelude, resident host, and comptime evaluator include this same
// kernel so set_trace_id has one state carrier rather than tier-specific
// copies of the setter semantics.
thread_local! {
    pub(crate) static JET_LOG_TRACE_ID: std::cell::RefCell<String> =
        std::cell::RefCell::new(String::new());
}

pub(crate) fn jet_ring_log_set_trace_id(id: &str) {
    JET_LOG_TRACE_ID.with(|trace| *trace.borrow_mut() = id.to_string());
}
