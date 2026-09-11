// ── core.log ───────────────────────────────────────────────────────────────────
// E2-M12 D-OBS3: structured JSON logs (OTel-aligned field names).
// Each log record is a JSON object on stderr:
//   {"level":"info","body":"...","ts":<unix-ms>}
// When a trace_id is set (log.set_trace_id), it appears as "trace_id":"...".
// Log level: 0=debug, 1=info, 2=warn, 3=error, 4=critical, 5=fatal. Default is info (1).
// D-LOGFMT1=A: format 0=auto (TTY→text, else JSON), 1=json, 2=text.
const JET_LOG_TELEMETRY_SOURCE: &str = "core.log";
const JET_LOG_TELEMETRY_FILE: &str = "Prelude/CoreLib/Top/Log.rs";
const JET_LOG_TELEMETRY_TEXT_BYTES: usize = 512;
const JET_LOG_TELEMETRY_MAX_FIELDS: usize = 16;
const JET_LOG_TELEMETRY_MAX_METRICS: usize = 64;
const JET_LOG_TELEMETRY_MAX_LINKED_LOGS: usize = 32;
const JET_LOG_TELEMETRY_SLOW_WORK_NS: u64 = 100_000_000;

#[derive(Clone)]
struct JetLogSpanObservation {
    id: i64,
    name: String,
    parent_id: Option<i64>,
    trace_id: Option<String>,
    start_ns: u64,
    linked_logs: Vec<u64>,
}

struct JetLogMetricState {
    name: String,
    next_index: u64,
}

static JET_LOG_TELEMETRY_NEXT_LOG_INDEX: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);
static JET_LOG_TELEMETRY_NEXT_SLOW_INDEX: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);
static JET_LOG_TELEMETRY_METRICS: std::sync::LazyLock<
    std::sync::Mutex<Vec<JetLogMetricState>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(Vec::new()));

thread_local! {
    static JET_LOG_LEVEL: std::cell::Cell<u8> = std::cell::Cell::new(1);
    static JET_LOG_DISABLED: std::cell::Cell<bool> = std::cell::Cell::new(false);
    static JET_LOG_FORMAT: std::cell::Cell<u8> = std::cell::Cell::new(0);
    static JET_LOG_SINK_PATH: std::cell::RefCell<String> = std::cell::RefCell::new(String::new());
    static JET_LOG_SPANS: std::cell::RefCell<Vec<jet_std::LogSpan>> = std::cell::RefCell::new(Vec::new());
    static JET_LOG_SPAN_OBSERVATIONS: std::cell::RefCell<Vec<JetLogSpanObservation>> =
        std::cell::RefCell::new(Vec::new());
    static JET_LOG_SAMPLE_EVERY: std::cell::Cell<i64> = std::cell::Cell::new(1);
    static JET_LOG_SAMPLE_COUNT: std::cell::Cell<i64> = std::cell::Cell::new(0);
    static JET_LOG_NEXT_SPAN: std::cell::Cell<i64> = std::cell::Cell::new(1);
}

fn jet_log_level_rank(level: &str) -> Option<u8> {
    match level {
        "debug" => Some(0),
        "info" => Some(1),
        "warn" | "warning" => Some(2),
        "error" => Some(3),
        "critical" => Some(4),
        "fatal" => Some(5),
        _ => None,
    }
}

fn jet_ring_log_set_level(level: &String) {
    let n: u8 = jet_log_level_rank(level).unwrap_or(1);
    JET_LOG_LEVEL.with(|l| l.set(n));
}

fn jet_ring_log_disable() {
    JET_LOG_DISABLED.with(|d| d.set(true));
}

fn jet_ring_log_flush() {
    let _ = std::io::Write::flush(&mut std::io::stderr());
    JET_LOG_SINK_PATH.with(|path| {
        let path = path.borrow();
        if path.is_empty() {
            return;
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path.as_str())
        {
            let _ = std::io::Write::flush(&mut file);
        }
    });
}

fn jet_ring_log_enabled(level: &String) -> bool {
    if JET_LOG_DISABLED.with(|d| d.get()) {
        return false;
    }
    let Some(rank) = jet_log_level_rank(level) else {
        return false;
    };
    JET_LOG_LEVEL.with(|l| l.get() <= rank)
}


// D-LOGFMT1=A: explicit format override.
fn jet_ring_log_setup(format: &String) {
    let n: u8 = match format.as_str() {
        "json" => 1,
        "text" => 2,
        _ => 0,
    };
    JET_LOG_FORMAT.with(|f| f.set(n));
}

fn jet_ring_log_set_sink(kind: &String, path: &String) {
    let n: u8 = match kind.as_str() {
        "jsonl" | "json" => 1,
        "text" => 2,
        _ => 1,
    };
    JET_LOG_FORMAT.with(|f| f.set(n));
    JET_LOG_SINK_PATH.with(|p| *p.borrow_mut() = path.clone());
}

fn jet_ring_log_otlp_file(path: &String) {
    jet_ring_log_set_sink(&"jsonl".to_string(), path);
}

fn jet_ring_log_sample_every(n: i64) {
    JET_LOG_SAMPLE_EVERY.with(|s| s.set(n.max(1)));
    JET_LOG_SAMPLE_COUNT.with(|c| c.set(0));
}

fn jet_ring_log_field(key: &String, value: &String) -> jet_std::LogField {
    jet_std::LogField {
        key: key.clone(),
        value: value.clone(),
        kind: "string".to_string(),
        redacted: false,
    }
}

fn jet_ring_log_int(key: &String, value: i64) -> jet_std::LogField {
    jet_std::LogField {
        key: key.clone(),
        value: value.to_string(),
        kind: "int".to_string(),
        redacted: false,
    }
}

fn jet_ring_log_float(key: &String, value: f64) -> jet_std::LogField {
    jet_std::LogField {
        key: key.clone(),
        value: value.to_string(),
        kind: "float".to_string(),
        redacted: false,
    }
}

fn jet_ring_log_bool(key: &String, value: bool) -> jet_std::LogField {
    jet_std::LogField {
        key: key.clone(),
        value: value.to_string(),
        kind: "bool".to_string(),
        redacted: false,
    }
}

fn jet_ring_log_redact(key: &String) -> jet_std::LogField {
    jet_std::LogField {
        key: key.clone(),
        value: "[redacted]".to_string(),
        kind: "redacted".to_string(),
        redacted: true,
    }
}

fn jet_ring_log_counter(name: &String, value: i64) -> jet_std::LogField {
    jet_log_publish_metric(name, value);
    jet_std::LogField {
        key: format!("metric.counter.{}", name),
        value: value.to_string(),
        kind: "counter".to_string(),
        redacted: false,
    }
}

fn jet_ring_log_span(name: &String) -> jet_std::LogSpan {
    let id = JET_LOG_NEXT_SPAN.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    jet_std::LogSpan {
        id,
        name: name.clone(),
    }
}

fn jet_ring_log_enter(span: &jet_std::LogSpan) {
    JET_LOG_SPANS.with(|s| s.borrow_mut().push(span.clone()));

    let parent_id =
        JET_LOG_SPAN_OBSERVATIONS.with(|spans| spans.borrow().last().map(|state| state.id));
    let trace_id = JET_LOG_TRACE_ID.with(|trace| {
        let trace = jet_log_telemetry_text(&trace.borrow());
        (!trace.is_empty()).then_some(trace)
    });
    let observation = JetLogSpanObservation {
        id: span.id,
        name: jet_log_telemetry_text(&span.name),
        parent_id,
        trace_id,
        start_ns: jet_log_telemetry_monotonic_now_ns(),
        linked_logs: Vec::new(),
    };
    JET_LOG_SPAN_OBSERVATIONS.with(|spans| {
        let mut spans = spans.borrow_mut();
        if spans.len() < JET_LOG_TELEMETRY_MAX_LINKED_LOGS {
            spans.push(observation);
        }
    });
}

fn jet_ring_log_close(span: &jet_std::LogSpan) {
    JET_LOG_SPANS.with(|s| {
        let mut spans = s.borrow_mut();
        if let Some(pos) = spans.iter().rposition(|x| x.id == span.id) {
            spans.remove(pos);
        }
    });
    let observation = JET_LOG_SPAN_OBSERVATIONS.with(|spans| {
        let mut spans = spans.borrow_mut();
        spans
            .iter()
            .rposition(|state| state.id == span.id)
            .map(|pos| spans.remove(pos))
    });
    if let Some(observation) = observation {
        jet_log_publish_span(&observation, jet_log_telemetry_monotonic_now_ns());
    }
}

fn jet_log_telemetry_text(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_control() {
            continue;
        }
        let width = ch.len_utf8();
        if out.len().saturating_add(width) > JET_LOG_TELEMETRY_TEXT_BYTES {
            break;
        }
        out.push(ch);
    }
    out
}

fn jet_log_telemetry_json_string(value: &str) -> String {
    let max_len = JET_LOG_TELEMETRY_TEXT_BYTES.saturating_add(2);
    let mut out = String::with_capacity(value.len().min(JET_LOG_TELEMETRY_TEXT_BYTES) + 2);
    out.push('"');
    for ch in value.chars() {
        if ch.is_control() {
            let escaped = format!("\\u{:04x}", ch as u32);
            if out.len().saturating_add(escaped.len()).saturating_add(1) > max_len {
                break;
            }
            out.push_str(&escaped);
            continue;
        }
        let fragment = match ch {
            '"' => "\\\"",
            '\\' => "\\\\",
            '\n' => "\\n",
            '\r' => "\\r",
            '\t' => "\\t",
            '\u{08}' => "\\b",
            '\u{0c}' => "\\f",
            ch => {
                if out.len().saturating_add(ch.len_utf8()).saturating_add(1) > max_len {
                    break;
                }
                out.push(ch);
                continue;
            }
        };
        if out.len().saturating_add(fragment.len()).saturating_add(1) > max_len {
            break;
        }
        out.push_str(fragment);
    }
    out.push('"');
    out
}

fn jet_log_telemetry_context_json(
    trace_id: Option<&str>,
    span_id: Option<&str>,
) -> String {
    match (trace_id, span_id) {
        (Some(trace_id), Some(span_id)) => format!(
            ",\"trace_id\":{},\"span_id\":{}",
            jet_log_telemetry_json_string(trace_id),
            jet_log_telemetry_json_string(span_id)
        ),
        _ => String::new(),
    }
}

fn jet_log_telemetry_wall_now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

fn jet_log_telemetry_monotonic_now_ns() -> u64 {
    static EPOCH: std::sync::LazyLock<std::time::Instant> =
        std::sync::LazyLock::new(std::time::Instant::now);
    EPOCH
        .elapsed()
        .as_nanos()
        .min(u64::MAX as u128) as u64
}

fn jet_log_current_span() -> Option<JetLogSpanObservation> {
    JET_LOG_SPAN_OBSERVATIONS.with(|spans| spans.borrow().last().cloned())
}

fn jet_log_metric_slot(name: &str) -> Option<(bool, u64)> {
    let mut metrics = match JET_LOG_TELEMETRY_METRICS.lock() {
        Ok(metrics) => metrics,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(metric) = metrics.iter_mut().find(|metric| metric.name == name) {
        let index = metric.next_index;
        metric.next_index = metric.next_index.saturating_add(1).max(1);
        return Some((false, index));
    }
    if metrics.len() >= JET_LOG_TELEMETRY_MAX_METRICS {
        return None;
    }
    metrics.push(JetLogMetricState {
        name: name.to_string(),
        next_index: 2,
    });
    Some((true, 1))
}

fn jet_log_publish_metric(name: &String, value: i64) {
    let name = jet_log_telemetry_text(name);
    if name.is_empty() {
        return;
    }
    let Some((first, index)) = jet_log_metric_slot(&name) else {
        return;
    };
    let timestamp_ns = jet_log_telemetry_wall_now_ns();
    if first {
        let fields = format!(
            "{{\"name\":{},\"kind\":\"counter\",\"unit\":\"count\",\"description\":\"core.log counter\",\"source_id\":{},\"source_file\":{},\"source_line\":179,\"source_column\":1,\"source_function\":\"jet_ring_log_counter\"}}",
            jet_log_telemetry_json_string(&name),
            jet_log_telemetry_json_string(JET_LOG_TELEMETRY_SOURCE),
            jet_log_telemetry_json_string(JET_LOG_TELEMETRY_FILE),
        );
        if let Ok(event) = JetDevtoolsEvent::from_parts(
            0,
            JET_LOG_TELEMETRY_SOURCE,
            "Metric",
            &name,
            fields,
        ) {
            jet_devtools_publish_event(event);
        }
    }
    let span = jet_log_current_span();
    let span_id = span.as_ref().map(|span| span.id.to_string());
    let trace_id = span.as_ref().and_then(|span| span.trace_id.as_deref());
    let fields = format!(
        "{{\"index\":{},\"timestamp_ns\":{},\"instrument\":{},\"source_id\":{},\"source_file\":{},\"source_line\":179,\"source_column\":1,\"source_function\":\"jet_ring_log_counter\",\"tags\":[]{} }}",
        index,
        timestamp_ns,
        jet_log_telemetry_json_string(&name),
        jet_log_telemetry_json_string(JET_LOG_TELEMETRY_SOURCE),
        jet_log_telemetry_json_string(JET_LOG_TELEMETRY_FILE),
        jet_log_telemetry_context_json(trace_id, span_id.as_deref()),
    );
    let payload = format!(
        "{{\"value\":{}}}",
        value,
    );
    if let Ok(event) = JetDevtoolsEvent::from_parts_with_payload(
        timestamp_ns / 1_000_000,
        JET_LOG_TELEMETRY_SOURCE,
        "Metric",
        &name,
        fields,
        Some(payload),
    ) {
        jet_devtools_publish_event(event);
    }
}

fn jet_log_telemetry_log_fields(fields: &[jet_std::LogField]) -> String {
    let mut metadata: Vec<(String, bool)> = Vec::new();
    for field in fields.iter().take(JET_LOG_TELEMETRY_MAX_FIELDS) {
        let key = jet_log_telemetry_text(&field.key);
        if key.is_empty() {
            continue;
        }
        if let Some((_, redacted)) = metadata.iter_mut().find(|(existing, _)| existing == &key) {
            *redacted |= field.redacted;
        } else {
            metadata.push((key, field.redacted));
        }
    }
    metadata.sort_by(|left, right| left.0.cmp(&right.0));
    let fields = metadata
        .into_iter()
        .map(|(key, redacted)| {
            if redacted {
                format!(
                    "{{\"key\":{},\"redacted\":true}}",
                    jet_log_telemetry_json_string(&key)
                )
            } else {
                format!("{{\"key\":{}}}", jet_log_telemetry_json_string(&key))
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{}]", fields)
}
fn jet_log_telemetry_payload_fields(fields: &[jet_std::LogField]) -> String {
    let values = fields
        .iter()
        .take(JET_LOG_TELEMETRY_MAX_FIELDS)
        .filter_map(|field| {
            let key = jet_log_telemetry_text(&field.key);
            if key.is_empty() || field.redacted {
                return None;
            }
            let value = match field.kind.as_str() {
                "int" | "counter" => field.value.parse::<i64>().ok().map(|value| value.to_string()),
                "float" => field
                    .value
                    .parse::<f64>()
                    .ok()
                    .filter(|value| value.is_finite())
                    .map(|value| value.to_string()),
                "bool" if matches!(field.value.as_str(), "true" | "false") => {
                    Some(field.value.clone())
                }
                _ => Some(jet_log_telemetry_json_string(&field.value)),
            }?;
            Some(format!(
                "{{\"key\":{},\"value\":{}}}",
                jet_log_telemetry_json_string(&key),
                value,
            ))
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{}]", values)
}


fn jet_log_publish_record(
    level: &str,
    message: &str,
    timestamp_ns: u64,
    fields: &[jet_std::LogField],
    index: u64,
    span: Option<&JetLogSpanObservation>,
) -> bool {
    let span_id = span.map(|span| span.id.to_string());
    let trace_id = span.and_then(|span| span.trace_id.as_deref());
    let telemetry_fields = format!(
        "{{\"index\":{},\"timestamp_ns\":{},\"level\":{},\"source_id\":{},\"source_file\":{},\"source_line\":724,\"source_column\":1,\"source_function\":\"jet_log_emit\",\"fields\":{}{} }}",
        index,
        timestamp_ns,
        jet_log_telemetry_json_string(level),
        jet_log_telemetry_json_string(JET_LOG_TELEMETRY_SOURCE),
        jet_log_telemetry_json_string(JET_LOG_TELEMETRY_FILE),
        jet_log_telemetry_log_fields(fields),
        jet_log_telemetry_context_json(trace_id, span_id.as_deref()),
    );
    let payload = format!(
        "{{\"body\":{},\"fields\":{}}}",
        jet_log_telemetry_json_string(message),
        jet_log_telemetry_payload_fields(fields),
    );
    let Ok(event) = JetDevtoolsEvent::from_parts_with_payload(
        timestamp_ns / 1_000_000,
        JET_LOG_TELEMETRY_SOURCE,
        "Log",
        &index.to_string(),
        telemetry_fields,
        Some(payload),
    ) else {
        return false;
    };
    jet_devtools_publish_event(event);
    true
}

fn jet_log_note_active_span(
    span_id: i64,
    log_index: u64,
) -> Option<JetLogSpanObservation> {
    JET_LOG_SPAN_OBSERVATIONS.with(|spans| {
        let mut spans = spans.borrow_mut();
        let state = spans.last_mut()?;
        if state.id != span_id {
            return None;
        }
        if state.linked_logs.len() < JET_LOG_TELEMETRY_MAX_LINKED_LOGS {
            state.linked_logs.push(log_index);
        }
        Some(state.clone())
    })
}

fn jet_log_publish_link(
    span: &JetLogSpanObservation,
    level: &str,
    timestamp_ns: u64,
    log_index: u64,
) {
    let Some(trace_id) = span.trace_id.as_deref() else {
        return;
    };
    let fields = format!(
        "{{\"span_index\":{},\"log_index\":{},\"trace_id\":{},\"span_id\":{},\"span_source_id\":{},\"log_source_id\":{},\"timestamp_ns\":{},\"level\":{}}}",
        span.id.max(0),
        log_index,
        jet_log_telemetry_json_string(trace_id),
        jet_log_telemetry_json_string(&span.id.to_string()),
        jet_log_telemetry_json_string(JET_LOG_TELEMETRY_SOURCE),
        jet_log_telemetry_json_string(JET_LOG_TELEMETRY_SOURCE),
        timestamp_ns,
        jet_log_telemetry_json_string(level),
    );
    if let Ok(event) = JetDevtoolsEvent::from_parts(
        timestamp_ns / 1_000_000,
        JET_LOG_TELEMETRY_SOURCE,
        "TraceLink",
        &format!("{}:{}", span.id.max(0), log_index),
        fields,
    ) {
        jet_devtools_publish_event(event);
    }
}

fn jet_log_publish_span(span: &JetLogSpanObservation, end_ns: u64) {
    let Some(trace_id) = span.trace_id.as_deref() else {
        return;
    };
    let span_index = span.id.max(0) as u64;
    let end_ns = end_ns.max(span.start_ns);
    let duration_ns = end_ns.saturating_sub(span.start_ns);
    let linked_logs = span
        .linked_logs
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let span_id = span.id.to_string();
    let parent_span_id = span.parent_id.map(|parent| parent.to_string());
    let parent_field = parent_span_id
        .as_deref()
        .map(jet_log_telemetry_json_string)
        .unwrap_or_else(|| "null".to_string());
    let fields = format!(
        "{{\"index\":{},\"name\":{},\"kind\":\"internal\",\"parent_span_id\":{},\"start_ns\":{},\"status\":\"ok\",\"trace_id\":{},\"span_id\":{},\"source_id\":{},\"source_file\":{},\"source_line\":201,\"source_column\":1,\"source_function\":\"jet_ring_log_enter\",\"linked_log_indexes\":[{}],\"end_ns\":{},\"duration_ns\":{}}}",
        span_index,
        jet_log_telemetry_json_string(&span.name),
        parent_field,
        span.start_ns,
        jet_log_telemetry_json_string(trace_id),
        jet_log_telemetry_json_string(&span_id),
        jet_log_telemetry_json_string(JET_LOG_TELEMETRY_SOURCE),
        jet_log_telemetry_json_string(JET_LOG_TELEMETRY_FILE),
        linked_logs,
        end_ns,
        duration_ns,
    );
    if let Ok(event) = JetDevtoolsEvent::from_parts(
        span.start_ns / 1_000_000,
        JET_LOG_TELEMETRY_SOURCE,
        "Trace",
        &span_id,
        fields,
    ) {
        jet_devtools_publish_event(event);
    }
    if duration_ns < JET_LOG_TELEMETRY_SLOW_WORK_NS {
        return;
    }
    let slow_index = JET_LOG_TELEMETRY_NEXT_SLOW_INDEX
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let slow_fields = format!(
        "{{\"index\":{},\"operation\":{},\"threshold_ns\":{},\"duration_ns\":{},\"start_ns\":{},\"end_ns\":{},\"trace_id\":{},\"span_id\":{},\"source_id\":{},\"source_file\":{},\"source_line\":201,\"source_column\":1,\"source_function\":\"jet_ring_log_enter\"}}",
        slow_index,
        jet_log_telemetry_json_string(&span.name),
        JET_LOG_TELEMETRY_SLOW_WORK_NS,
        duration_ns,
        span.start_ns,
        end_ns,
        jet_log_telemetry_json_string(trace_id),
        jet_log_telemetry_json_string(&span_id),
        jet_log_telemetry_json_string(JET_LOG_TELEMETRY_SOURCE),
        jet_log_telemetry_json_string(JET_LOG_TELEMETRY_FILE),
    );
    if let Ok(event) = JetDevtoolsEvent::from_parts(
        end_ns / 1_000_000,
        JET_LOG_TELEMETRY_SOURCE,
        "SlowWork",
        &slow_index.to_string(),
        slow_fields,
    ) {
        jet_devtools_publish_event(event);
    }
}

fn jet_log_format_active() -> u8 {
    let explicit = JET_LOG_FORMAT.with(|f| f.get());
    if explicit != 0 {
        return explicit;
    }
    // Auto-detect: text if stderr is a terminal, JSON otherwise.
    use std::io::IsTerminal;
    if std::io::stderr().is_terminal() {
        2
    } else {
        1
    }
}

fn jet_log_json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

fn jet_log_fields_json(fields: &[jet_std::LogField]) -> String {
    let mut out = String::new();
    for field in fields {
        out.push_str(",\"");
        out.push_str(&jet_log_json_escape(&field.key));
        out.push_str("\":");
        if field.kind == "int"
            || field.kind == "float"
            || field.kind == "bool"
            || field.kind == "counter"
        {
            out.push_str(&field.value);
        } else {
            out.push('"');
            out.push_str(&jet_log_json_escape(&field.value));
            out.push('"');
        }
    }
    out
}

fn jet_log_spans_json() -> String {
    JET_LOG_SPANS.with(|s| {
        let spans = s.borrow();
        if spans.is_empty() {
            return String::new();
        }
        let names = spans
            .iter()
            .map(|span| format!("\"{}\"", jet_log_json_escape(&span.name)))
            .collect::<Vec<_>>()
            .join(",");
        format!(",\"spans\":[{}]", names)
    })
}

fn jet_log_write(line: &str) {
    let path = JET_LOG_SINK_PATH.with(|p| p.borrow().clone());
    if path.is_empty() {
        jet_log_write_line(line);
    } else if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        use std::io::Write;
        let _ = writeln!(f, "{}", line);
    }
}

fn jet_log_emit_json(level: &str, msg: &str, ts: i64, fields: &[jet_std::LogField]) {
    let trace = JET_LOG_TRACE_ID.with(|t| t.borrow().clone());
    let fields_json = jet_log_fields_json(fields);
    let spans_json = jet_log_spans_json();
    let line = if trace.is_empty() {
        format!(
            "{{\"level\":\"{}\",\"body\":\"{}\",\"ts\":{}{}{} }}",
            level,
            jet_log_json_escape(msg),
            ts,
            fields_json,
            spans_json
        )
    } else {
        format!(
            "{{\"level\":\"{}\",\"body\":\"{}\",\"trace_id\":\"{}\",\"ts\":{}{}{} }}",
            level,
            jet_log_json_escape(msg),
            jet_log_json_escape(&trace),
            ts,
            fields_json,
            spans_json
        )
    };
    jet_log_write(&line.replace(" }", "}"));
}

fn jet_log_emit_text(level: &str, msg: &str, ts: i64, fields: &[jet_std::LogField]) {
    let secs = ts / 1000;
    let (y, mo, d, h, mi, s) = unix_to_ymdhms(secs);
    let level_tag = match level {
        "debug" => "DEBUG",
        "info" => "INFO",
        "warn" => "WARN",
        "error" => "ERROR",
        "critical" => "CRITICAL",
        "fatal" => "FATAL",
        _ => level,
    };
    let trace = JET_LOG_TRACE_ID.with(|t| t.borrow().clone());
    let field_text = if fields.is_empty() {
        String::new()
    } else {
        format!(
            " {}",
            fields
                .iter()
                .map(|f| format!("{}={}", f.key, f.value))
                .collect::<Vec<_>>()
                .join(" ")
        )
    };
    let line = if trace.is_empty() {
        format!(
            "[{}] {:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z | {}{}",
            level_tag, y, mo, d, h, mi, s, msg, field_text
        )
    } else {
        format!(
            "[{}] {:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z trace={} | {}{}",
            level_tag, y, mo, d, h, mi, s, trace, msg, field_text
        )
    };
    jet_log_write(&line);
}

fn jet_log_emit(level: &str, msg: &str, fields: &[jet_std::LogField]) {
    if JET_LOG_DISABLED.with(|d| d.get()) {
        return;
    }
    let keep = JET_LOG_SAMPLE_EVERY.with(|every| {
        JET_LOG_SAMPLE_COUNT.with(|count| {
            let next = count.get() + 1;
            count.set(next);
            every.get() <= 1 || (next - 1) % every.get() == 0
        })
    });
    if !keep {
        return;
    }
    let timestamp_ns = jet_log_telemetry_wall_now_ns();
    let ts = (timestamp_ns / 1_000_000) as i64;
    let index = JET_LOG_TELEMETRY_NEXT_LOG_INDEX
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let span = jet_log_current_span();
    if jet_log_publish_record(level, msg, timestamp_ns, fields, index, span.as_ref()) {
        if let Some(span) = span.as_ref() {
            if let Some(updated) = jet_log_note_active_span(span.id, index) {
                jet_log_publish_link(&updated, level, timestamp_ns, index);
            }
        }
    }
    if jet_log_format_active() == 2 {
        jet_log_emit_text(level, msg, ts, fields);
    } else {
        jet_log_emit_json(level, msg, ts, fields);
    }
}

fn jet_ring_log_debug(msg: &String) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 0 {
        jet_log_emit("debug", msg, &[]);
    }
}
fn jet_ring_log_info(msg: &String) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 1 {
        jet_log_emit("info", msg, &[]);
    }
}
fn jet_ring_log_warn(msg: &String) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 2 {
        jet_log_emit("warn", msg, &[]);
    }
}
fn jet_ring_log_error(msg: &String) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 3 {
        jet_log_emit("error", msg, &[]);
    }
}
fn jet_ring_log_critical(msg: &String) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 4 {
        jet_log_emit("critical", msg, &[]);
    }
}
fn jet_ring_log_fatal(msg: &String) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 5 {
        jet_log_emit("fatal", msg, &[]);
    }
    jet_ring_log_flush();
    jet_log_process_exit(1);
}

fn jet_ring_log_debug_fields(msg: &String, fields: &Vec<jet_std::LogField>) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 0 {
        jet_log_emit("debug", msg, fields);
    }
}
fn jet_ring_log_info_fields(msg: &String, fields: &Vec<jet_std::LogField>) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 1 {
        jet_log_emit("info", msg, fields);
    }
}
fn jet_ring_log_warn_fields(msg: &String, fields: &Vec<jet_std::LogField>) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 2 {
        jet_log_emit("warn", msg, fields);
    }
}
fn jet_ring_log_error_fields(msg: &String, fields: &Vec<jet_std::LogField>) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 3 {
        jet_log_emit("error", msg, fields);
    }
}

fn unix_to_ymdhms(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    let mut days = secs / 86400;
    let time_of_day = (secs % 86400).unsigned_abs();
    let h = (time_of_day / 3600) as u32;
    let mi = ((time_of_day % 3600) / 60) as u32;
    let s = (time_of_day % 60) as u32;
    let mut year: i32 = 1970;
    loop {
        let dy = if is_leap(year) { 366 } else { 365 };
        if days < dy {
            break;
        }
        days -= dy;
        year += 1;
    }
    let month_days: [i64; 12] = if is_leap(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month: u32 = 1;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    (year, month, (days + 1) as u32, h, mi, s)
}

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}
