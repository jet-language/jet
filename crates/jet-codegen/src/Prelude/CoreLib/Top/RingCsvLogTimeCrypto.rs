// ── E2-M9: First-party ring libraries ────────────────────────────────────────
// Pure-Rust, zero external crates (I6). CSV, TOML, YAML, log, time, crypto.

// ── jet.csv ───────────────────────────────────────────────────────────────────
fn jet_ring_csv_parse(text: &String) -> Result<Vec<Vec<String>>, String> {
    let mut rows = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        match csv_parse_row(line) {
            Ok(row) => rows.push(row),
            Err(msg) => {
                return Err(format!("E2701: CSV row {} — {}", line_no + 1, msg));
            }
        }
    }
    Ok(rows)
}

fn csv_parse_row(line: &str) -> Result<Vec<String>, String> {
    let mut fields = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        let field = if chars.peek() == Some(&'"') {
            chars.next(); // consume opening quote
            let mut s = String::new();
            loop {
                match chars.next() {
                    Some('"') => {
                        if chars.peek() == Some(&'"') {
                            chars.next(); // escaped quote
                            s.push('"');
                        } else {
                            break;
                        }
                    }
                    Some(c) => s.push(c),
                    None => break,
                }
            }
            s
        } else {
            let mut s = String::new();
            while let Some(&c) = chars.peek() {
                if c == ',' {
                    break;
                }
                s.push(c);
                chars.next();
            }
            s
        };
        fields.push(field);
        match chars.next() {
            Some(',') => {}
            None => break,
            Some(c) => return Err(format!("unexpected character {:?} after field", c)),
        }
    }
    Ok(fields)
}

fn jet_ring_csv_render(rows: &Vec<Vec<String>>) -> String {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|field| {
                    if field.contains(',') || field.contains('"') || field.contains('\n') {
                        format!("\"{}\"", field.replace('"', "\"\""))
                    } else {
                        field.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ── jet.log ───────────────────────────────────────────────────────────────────
// E2-M12 D-OBS3: structured JSON logs (OTel-aligned field names).
// Each log record is a JSON object on stderr:
//   {"level":"info","body":"...","ts":<unix-ms>}
// When a trace_id is set (log.set_trace_id), it appears as "trace_id":"...".
// Log level: 0=debug, 1=info, 2=warn, 3=error. Default is info (1).
// D-LOGFMT1=A: format 0=auto (TTY→text, else JSON), 1=json, 2=text.
thread_local! {
    static JET_LOG_LEVEL: std::cell::Cell<u8> = std::cell::Cell::new(1);
    static JET_LOG_TRACE_ID: std::cell::RefCell<String> = std::cell::RefCell::new(String::new());
    static JET_LOG_FORMAT: std::cell::Cell<u8> = std::cell::Cell::new(0);
    static JET_LOG_SINK_PATH: std::cell::RefCell<String> = std::cell::RefCell::new(String::new());
    static JET_LOG_SPANS: std::cell::RefCell<Vec<jet_std::LogSpan>> = std::cell::RefCell::new(Vec::new());
    static JET_LOG_SAMPLE_EVERY: std::cell::Cell<i64> = std::cell::Cell::new(1);
    static JET_LOG_SAMPLE_COUNT: std::cell::Cell<i64> = std::cell::Cell::new(0);
    static JET_LOG_NEXT_SPAN: std::cell::Cell<i64> = std::cell::Cell::new(1);
}

fn jet_ring_log_set_level(level: &String) {
    let n: u8 = match level.as_str() {
        "debug" => 0,
        "info" => 1,
        "warn" => 2,
        "error" => 3,
        _ => 1,
    };
    JET_LOG_LEVEL.with(|l| l.set(n));
}

fn jet_ring_log_set_trace_id(id: &String) {
    JET_LOG_TRACE_ID.with(|t| *t.borrow_mut() = id.clone());
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
}

fn jet_ring_log_close(span: &jet_std::LogSpan) {
    JET_LOG_SPANS.with(|s| {
        let mut spans = s.borrow_mut();
        if let Some(pos) = spans.iter().rposition(|x| x.id == span.id) {
            spans.remove(pos);
        }
    });
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
        if field.kind == "int" || field.kind == "float" || field.kind == "bool" || field.kind == "counter" {
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
        eprintln!("{}", line);
    } else if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
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
            level, jet_log_json_escape(msg), ts, fields_json, spans_json
        )
    } else {
        format!(
            "{{\"level\":\"{}\",\"body\":\"{}\",\"trace_id\":\"{}\",\"ts\":{}{}{} }}",
            level, jet_log_json_escape(msg), jet_log_json_escape(&trace), ts, fields_json, spans_json
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
        format!("[{}] {:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z | {}{}", level_tag, y, mo, d, h, mi, s, msg, field_text)
    } else {
        format!("[{}] {:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z trace={} | {}{}", level_tag, y, mo, d, h, mi, s, trace, msg, field_text)
    };
    jet_log_write(&line);
}

fn jet_log_emit(level: &str, msg: &str, fields: &[jet_std::LogField]) {
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
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
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

// ── jet.time ──────────────────────────────────────────────────────────────────
// Format a Unix millisecond timestamp using a strftime-like pattern.
// Supported tokens: %Y year, %m month, %d day, %H hour, %M minute, %S second.
fn jet_ring_time_format(millis: i64, fmt: &String) -> String {
    let secs = (millis / 1000) as i64;
    let (y, mo, d, h, mi, s) = unix_to_ymdhms(secs);
    let mut out = fmt.clone();
    out = out.replace("%Y", &format!("{:04}", y));
    out = out.replace("%m", &format!("{:02}", mo));
    out = out.replace("%d", &format!("{:02}", d));
    out = out.replace("%H", &format!("{:02}", h));
    out = out.replace("%M", &format!("{:02}", mi));
    out = out.replace("%S", &format!("{:02}", s));
    out
}

fn unix_to_ymdhms(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    // Days since epoch, treating every year as having the right leap-year logic.
    let mut days = secs / 86400;
    let time_of_day = (secs % 86400).unsigned_abs();
    let h = (time_of_day / 3600) as u32;
    let mi = ((time_of_day % 3600) / 60) as u32;
    let s = (time_of_day % 60) as u32;
    // Walk from 1970.
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

// ── jet.crypto ────────────────────────────────────────────────────────────────
// SHA-256 of a UTF-8 string, returned as a lowercase hex string.
fn jet_ring_crypto_sha256(s: &String) -> String {
    let hash = jet_sha256_raw(s.as_bytes());
    hash.iter().map(|b| format!("{:02x}", b)).collect()
}

// SHA-256 of a byte list (Vec<u8>), returned as lowercase hex.
fn jet_ring_crypto_sha256_bytes(bs: &Vec<u8>) -> String {
    let hash = jet_sha256_raw(bs.as_slice());
    hash.iter().map(|b| format!("{:02x}", b)).collect()
}

// D-TTLVAL1=A: Expiring<T> / Rotting<T> (pure std, injectable Clock).
fn jet_expiring_new<T: Clone>(value: T, ttl_ms: i64, clock_now: i64) -> JetExpiring<T> {
    JetExpiring::new(value, clock_now.saturating_add(ttl_ms))
}
fn jet_rotting_new<T: Clone + 'static>(value: T, ttl_ms: i64, clock_now: i64) -> JetRotting<T> {
    JetRotting::new(value, clock_now.saturating_add(ttl_ms))
}
fn jet_expiring_get<T: Clone>(exp: &JetExpiring<T>, now_ms: i64) -> Result<T, JetExpired> {
    exp.get(now_ms)
}
fn jet_rotting_get<T: Clone + 'static>(
    rot: &mut JetRotting<T>,
    now_ms: i64,
) -> Result<T, JetExpired> {
    rot.get(now_ms)
}

// Minimal SHA-256 (same algorithm as src/sha256.rs — duplicated here so the
// prelude doesn't need to reach into the compiler crate; I6 forbids extern deps).
fn jet_sha256_raw(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    const H0: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut state = H0;
    let bit_len = (data.len() as u64) * 8;
    let mut msg: Vec<u8> = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(block[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = [
            state[0], state[1], state[2], state[3], state[4], state[5], state[6], state[7],
        ];
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
