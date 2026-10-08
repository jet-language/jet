// ABI projections over existing Log.rs operations; replaced by Jet L6, dies at cutover.
pub(crate) fn jet_log_record_emit_raw(level: &String, message: &String, fields: &Vec<jet_std::LogField>) {
    jet_log_emit(level.as_str(), message.as_str(), fields.as_slice());
}
pub(crate) fn jet_log_span_allocate_raw(name: &String) -> (i64, String) {
    let span = jet_ring_log_span(name);
    (span.id, span.name)
}
