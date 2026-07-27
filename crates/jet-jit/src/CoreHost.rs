//! Host shims for `core.os`, `jet.log`, `core.math`, `core.files`,
//! `core.path`, `core.env`, and `core.process` CoreCalls (#729). Behavior
//! mirrors AOT helpers in the CoreLib prelude (`jet_std_os_*`, `jet_ring_log_*`,
//! `jet_std_math_*`, `jet_std_fs_*`, `jet_std_path_*`, `jet_std_env_*`,
//! `jet_std_process_*`) — thin std wrappers, not a third algorithm.

use super::Concurrency;
use std::cell::{Cell, RefCell};

// ── core.os (mirrors jet_std_os_* in FsIoEnvOsTesting.rs) ────────────────────

extern "C" fn jet_jit_os_name() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(std::env::consts::OS.to_string()))
}

extern "C" fn jet_jit_os_family() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(std::env::consts::FAMILY.to_string()))
}

extern "C" fn jet_jit_os_arch() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(std::env::consts::ARCH.to_string()))
}

extern "C" fn jet_jit_os_cpu_count() -> i64 {
    std::thread::available_parallelism()
        .map(|n| n.get() as i64)
        .unwrap_or(1)
}

extern "C" fn jet_jit_os_temp_dir() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .alloc_string(std::env::temp_dir().to_string_lossy().to_string())
    })
}

extern "C" fn jet_jit_os_executable() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        // Under with_program_args, argv[0] is the entry .jet (or binary); that is
        // the program identity watchers/process should re-exec (#1219).
        let path = crate::program_args()
            .into_iter()
            .next()
            .filter(|p| !p.is_empty())
            .or_else(|| {
                std::env::current_exe()
                    .ok()
                    .map(|p| p.to_string_lossy().to_string())
            })
            .unwrap_or_default();
        rt.heap.alloc_string(path)
    })
}

extern "C" fn jet_jit_os_pid() -> i64 {
    std::process::id() as i64
}

extern "C" fn jet_jit_os_hostname() -> i64 {
    let host = std::env::var("HOSTNAME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| "localhost".to_string());
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(host))
}

// ── jet.log (mirrors jet_ring_log_* in RingCsvLogTimeCrypto.rs) ───────────────
// Level: 0=debug, 1=info, 2=warn, 3=error. Format: 0=auto, 1=json, 2=text.

thread_local! {
    static JIT_LOG_LEVEL: Cell<u8> = const { Cell::new(1) };
    static JIT_LOG_FORMAT: Cell<u8> = const { Cell::new(0) };
    static JIT_LOG_TRACE_ID: RefCell<String> = const { RefCell::new(String::new()) };
    static JIT_LOG_SPANS: RefCell<Vec<(i64, String)>> = const { RefCell::new(Vec::new()) };
    static JIT_LOG_NEXT_SPAN: Cell<i64> = const { Cell::new(1) };
}

struct JitLogField {
    key: String,
    value: String,
    kind: String,
}

fn jit_log_set_level_str(level: &str) {
    let n: u8 = match level {
        "debug" => 0,
        "info" => 1,
        "warn" => 2,
        "error" => 3,
        _ => 1,
    };
    JIT_LOG_LEVEL.with(|l| l.set(n));
}

fn jit_log_setup_str(format: &str) {
    let n: u8 = match format {
        "json" => 1,
        "text" => 2,
        _ => 0,
    };
    JIT_LOG_FORMAT.with(|f| f.set(n));
}

fn jit_log_format_active() -> u8 {
    let explicit = JIT_LOG_FORMAT.with(|f| f.get());
    if explicit != 0 {
        return explicit;
    }
    use std::io::IsTerminal;
    if std::io::stderr().is_terminal() {
        2
    } else {
        1
    }
}

fn jit_log_json_escape(s: &str) -> String {
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

/// Mirrors AOT `unix_to_ymdhms` in RingCsvLogTimeCrypto.rs.
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

fn clone_heap_string(id: i64) -> String {
    Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(id).unwrap_or_default())
}

fn result_ok_bits(bits: u64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.results.push(super::JitResultValue { ok: true, bits });
        rt.results.len() as i64
    })
}

fn result_err_msg(msg: &str) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let sid = rt.heap.alloc_string(msg.to_string());
        rt.results.push(super::JitResultValue {
            ok: false,
            bits: sid as u64,
        });
        rt.results.len() as i64
    })
}

fn jit_log_emit(level: &str, msg: &str, fields: &[JitLogField]) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let line = if jit_log_format_active() == 2 {
        let secs = ts / 1000;
        let (y, mo, d, h, mi, s) = unix_to_ymdhms(secs);
        let level_tag = match level {
            "debug" => "DEBUG",
            "info" => "INFO",
            "warn" => "WARN",
            "error" => "ERROR",
            _ => level,
        };
        let mut line = format!("[{level_tag}] {y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z | {msg}");
        for field in fields {
            line.push_str(&format!(" {}={}", field.key, field.value));
        }
        line
    } else {
        let mut fields_json = String::new();
        for field in fields {
            fields_json.push_str(",\"");
            fields_json.push_str(&jit_log_json_escape(&field.key));
            fields_json.push_str("\":");
            if matches!(field.kind.as_str(), "int" | "float" | "bool" | "counter") {
                fields_json.push_str(&field.value);
            } else {
                fields_json.push('"');
                fields_json.push_str(&jit_log_json_escape(&field.value));
                fields_json.push('"');
            }
        }
        let spans_json = JIT_LOG_SPANS.with(|s| {
            let spans = s.borrow();
            if spans.is_empty() {
                String::new()
            } else {
                let names = spans
                    .iter()
                    .map(|(_, name)| format!("\"{}\"", jit_log_json_escape(name)))
                    .collect::<Vec<_>>()
                    .join(",");
                format!(",\"spans\":[{names}]")
            }
        });
        let trace = JIT_LOG_TRACE_ID.with(|t| t.borrow().clone());
        if trace.is_empty() {
            format!(
                "{{\"level\":\"{}\",\"body\":\"{}\",\"ts\":{}{}{}}}",
                level,
                jit_log_json_escape(msg),
                ts,
                fields_json,
                spans_json
            )
        } else {
            format!(
                "{{\"level\":\"{}\",\"body\":\"{}\",\"trace_id\":\"{}\",\"ts\":{}{}{}}}",
                level,
                jit_log_json_escape(msg),
                jit_log_json_escape(&trace),
                ts,
                fields_json,
                spans_json
            )
        }
    };
    Concurrency::with_runtime_mut(|rt| {
        rt.stderr.push_str(&line);
        rt.stderr.push('\n');
    });
}

extern "C" fn jet_jit_log_set_level(msg: i64) {
    jit_log_set_level_str(&clone_heap_string(msg));
}

extern "C" fn jet_jit_log_setup(msg: i64) {
    jit_log_setup_str(&clone_heap_string(msg));
}

extern "C" fn jet_jit_log_debug(msg: i64) {
    if JIT_LOG_LEVEL.with(|l| l.get()) <= 0 {
        jit_log_emit("debug", &clone_heap_string(msg), &[]);
    }
}

extern "C" fn jet_jit_log_info(msg: i64) {
    if JIT_LOG_LEVEL.with(|l| l.get()) <= 1 {
        jit_log_emit("info", &clone_heap_string(msg), &[]);
    }
}

extern "C" fn jet_jit_log_warn(msg: i64) {
    if JIT_LOG_LEVEL.with(|l| l.get()) <= 2 {
        jit_log_emit("warn", &clone_heap_string(msg), &[]);
    }
}

extern "C" fn jet_jit_log_error(msg: i64) {
    if JIT_LOG_LEVEL.with(|l| l.get()) <= 3 {
        jit_log_emit("error", &clone_heap_string(msg), &[]);
    }
}

extern "C" fn jet_jit_log_set_trace_id(msg: i64) {
    let id = clone_heap_string(msg);
    JIT_LOG_TRACE_ID.with(|t| *t.borrow_mut() = id);
}

fn alloc_log_field(key: String, value: String, kind: &str, redacted: bool) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let rec = rt.heap.alloc_record(4);
        let k = rt.heap.alloc_string(key);
        let v = rt.heap.alloc_string(value);
        let kd = rt.heap.alloc_string(kind.to_string());
        let _ = rt.heap.record_set_string(rec, 0, k);
        let _ = rt.heap.record_set_string(rec, 1, v);
        let _ = rt.heap.record_set_string(rec, 2, kd);
        let _ = rt.heap.record_set_bool(rec, 3, redacted);
        rec
    })
}

extern "C" fn jet_jit_log_field(key: i64, value: i64) -> i64 {
    alloc_log_field(clone_heap_string(key), clone_heap_string(value), "string", false)
}

extern "C" fn jet_jit_log_int_field(key: i64, value: i64) -> i64 {
    alloc_log_field(clone_heap_string(key), value.to_string(), "int", false)
}

extern "C" fn jet_jit_log_bool_field(key: i64, value: i8) -> i64 {
    alloc_log_field(
        clone_heap_string(key),
        if value != 0 { "true" } else { "false" }.to_string(),
        "bool",
        false,
    )
}

extern "C" fn jet_jit_log_counter(name: i64, value: i64) -> i64 {
    alloc_log_field(
        format!("metric.counter.{}", clone_heap_string(name)),
        value.to_string(),
        "counter",
        false,
    )
}

extern "C" fn jet_jit_log_span(name: i64) -> i64 {
    let id = JIT_LOG_NEXT_SPAN.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    let name_s = clone_heap_string(name);
    Concurrency::with_runtime_mut(|rt| {
        let rec = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_int(rec, 0, id);
        let sid = rt.heap.alloc_string(name_s);
        let _ = rt.heap.record_set_string(rec, 1, sid);
        rec
    })
}

extern "C" fn jet_jit_log_enter(span: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let id = rt.heap.record_get_int(span, 0).unwrap_or(0);
        let name = rt
            .heap
            .record_get_string(span, 1)
            .and_then(|sid| rt.heap.clone_string(sid))
            .unwrap_or_default();
        JIT_LOG_SPANS.with(|s| s.borrow_mut().push((id, name)));
    });
}

extern "C" fn jet_jit_log_close(span: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let id = rt.heap.record_get_int(span, 0).unwrap_or(0);
        JIT_LOG_SPANS.with(|s| {
            let mut spans = s.borrow_mut();
            if let Some(pos) = spans.iter().rposition(|(sid, _)| *sid == id) {
                spans.remove(pos);
            }
        });
    });
}

fn read_log_fields(list: i64) -> Vec<JitLogField> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            let rec = rt.heap.list_get_int(list, i).unwrap_or(0);
            let key = rt
                .heap
                .record_get_string(rec, 0)
                .and_then(|sid| rt.heap.clone_string(sid))
                .unwrap_or_default();
            let value = rt
                .heap
                .record_get_string(rec, 1)
                .and_then(|sid| rt.heap.clone_string(sid))
                .unwrap_or_default();
            let kind = rt
                .heap
                .record_get_string(rec, 2)
                .and_then(|sid| rt.heap.clone_string(sid))
                .unwrap_or_else(|| "string".to_string());
            out.push(JitLogField { key, value, kind });
        }
        out
    })
}

extern "C" fn jet_jit_log_info_fields(msg: i64, fields: i64) {
    if JIT_LOG_LEVEL.with(|l| l.get()) <= 1 {
        let fs = read_log_fields(fields);
        jit_log_emit("info", &clone_heap_string(msg), &fs);
    }
}

// ── core.files / core.path (mirrors jet_std_fs_* / jet_std_path_*) ───────────

extern "C" fn jet_jit_fs_exists(path: i64) -> i8 {
    let p = clone_heap_string(path);
    i8::from(std::path::Path::new(&p).exists())
}

extern "C" fn jet_jit_fs_read(path: i64) -> i64 {
    let p = clone_heap_string(path);
    match std::fs::read_to_string(&p) {
        Ok(text) => {
            let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text));
            result_ok_bits(sid as u64)
        }
        Err(e) => result_err_msg(&format!("read {p}: {e}")),
    }
}

extern "C" fn jet_jit_fs_write(path: i64, text: i64) -> i64 {
    let p = clone_heap_string(path);
    let t = clone_heap_string(text);
    match std::fs::write(&p, t) {
        Ok(()) => result_ok_bits(0),
        Err(e) => result_err_msg(&format!("write {p}: {e}")),
    }
}

extern "C" fn jet_jit_fs_create_dir(path: i64) -> i64 {
    let p = clone_heap_string(path);
    match std::fs::create_dir_all(&p) {
        Ok(()) => result_ok_bits(0),
        Err(e) => result_err_msg(&format!("create_dir {p}: {e}")),
    }
}

extern "C" fn jet_jit_path_join(base: i64, part: i64) -> i64 {
    let b = clone_heap_string(base);
    let p = clone_heap_string(part);
    let joined = std::path::Path::new(&b)
        .join(p)
        .to_string_lossy()
        .to_string();
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(joined))
}

/// String-form `core.path.parent` / `.extension` / `.normalize` (D-IO1 helpers).
extern "C" fn jet_jit_path_parent_str(path: i64) -> i64 {
    let s = clone_heap_string(path);
    let out = std::path::Path::new(&s)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(out))
}

extern "C" fn jet_jit_path_extension_str(path: i64) -> i64 {
    let s = clone_heap_string(path);
    let out = std::path::Path::new(&s)
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(out))
}

extern "C" fn jet_jit_path_normalize_str(path: i64) -> i64 {
    let s = clone_heap_string(path);
    let source = std::path::Path::new(&s);
    let rooted = source.has_root();
    let mut normalized = std::path::PathBuf::new();
    let mut normal_depth = 0usize;
    for component in source.components() {
        match component {
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir => normalized.push(component.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir if normal_depth > 0 => {
                normalized.pop();
                normal_depth -= 1;
            }
            std::path::Component::ParentDir if !rooted => normalized.push(".."),
            std::path::Component::ParentDir => {}
            std::path::Component::Normal(part) => {
                normalized.push(part);
                normal_depth += 1;
            }
        }
    }
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .alloc_string(normalized.to_string_lossy().into_owned())
    })
}

extern "C" fn jet_jit_path_write_atomic(rec: i64, bytes: i64) -> i64 {
    let p = path_string_from_record(rec);
    let path_id = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(p));
    jet_jit_fs_write_atomic(path_id, bytes)
}

extern "C" fn jet_jit_path_from(path: i64) -> i64 {
    path_record(clone_heap_string(path))
}

extern "C" fn jet_jit_path_join_handle(rec: i64, part: i64) -> i64 {
    let base = path_string_from_record(rec);
    let p = clone_heap_string(part);
    path_record(
        std::path::Path::new(&base)
            .join(p)
            .to_string_lossy()
            .to_string(),
    )
}

extern "C" fn jet_jit_path_parent(rec: i64) -> i64 {
    let s = path_string_from_record(rec);
    match std::path::Path::new(&s).parent() {
        None => 0,
        Some(par) => path_record(par.to_string_lossy().to_string()).wrapping_add(1),
    }
}

extern "C" fn jet_jit_path_extension(rec: i64) -> i64 {
    let s = path_string_from_record(rec);
    option_string_bits(
        std::path::Path::new(&s)
            .extension()
            .map(|e| e.to_string_lossy().to_string()),
    )
}

extern "C" fn jet_jit_path_stem(rec: i64) -> i64 {
    let s = path_string_from_record(rec);
    option_string_bits(
        std::path::Path::new(&s)
            .file_stem()
            .map(|e| e.to_string_lossy().to_string()),
    )
}

extern "C" fn jet_jit_path_to_string(rec: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(path_string_from_record(rec)))
}

extern "C" fn jet_jit_path_walk(rec: i64) -> i64 {
    let root_s = path_string_from_record(rec);
    let root = std::path::PathBuf::from(root_s);
    let mut result_paths = Vec::new();
    let mut stack = vec![root];
    let mut visited = std::collections::HashSet::new();
    while let Some(dir) = stack.pop() {
        let canonical = match std::fs::canonicalize(&dir) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !visited.insert(canonical) {
            continue;
        }
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for entry in rd {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            result_paths.push(path.to_string_lossy().to_string());
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    let list = Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for path in result_paths {
            let rec = rt.heap.alloc_record(1);
            let sid = rt.heap.alloc_string(path);
            let _ = rt.heap.record_set_string(rec, 0, sid);
            let _ = rt.heap.list_push_int(list, rec);
        }
        list
    });
    result_ok_bits(list as u64)
}

extern "C" fn jet_jit_fs_list_dir(path: i64) -> i64 {
    let p = clone_heap_string(path);
    let rd = match std::fs::read_dir(&p) {
        Ok(rd) => rd,
        Err(e) => return result_err_msg(&format!("list_dir {p}: {e}")),
    };
    let mut entries = Vec::new();
    for entry in rd {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => return result_err_msg(&format!("list_dir {p}: {e}")),
        };
        let name = entry.file_name().to_string_lossy().to_string();
        let full_path = std::path::Path::new(&p)
            .join(&name)
            .to_string_lossy()
            .to_string();
        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        entries.push((name, full_path, is_dir));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for (name, full_path, is_dir) in entries {
            let rec = rt.heap.alloc_record(3);
            let n = rt.heap.alloc_string(name);
            let fp = rt.heap.alloc_string(full_path);
            let _ = rt.heap.record_set_string(rec, 0, n);
            let _ = rt.heap.record_set_string(rec, 1, fp);
            let _ = rt.heap.record_set_bool(rec, 2, is_dir);
            let _ = rt.heap.list_push_int(list, rec);
        }
        rt.results.push(super::JitResultValue {
            ok: true,
            bits: list as u64,
        });
        rt.results.len() as i64
    })
}

fn clone_heap_bytes(list: i64) -> Vec<u8> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            out.push(rt.heap.list_get_int(list, i).unwrap_or(0) as u8);
        }
        out
    })
}

fn alloc_byte_list(bytes: &[u8]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for &b in bytes {
            let _ = rt.heap.list_push_int(list, b as i64);
        }
        list
    })
}

fn system_time_ms(t: std::time::SystemTime) -> Option<i64> {
    t.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
}

fn glob_match(pattern: &str, text: &str) -> bool {
    fn inner(p: &[u8], t: &[u8]) -> bool {
        if p.is_empty() {
            return t.is_empty();
        }
        match p[0] {
            b'*' => inner(&p[1..], t) || (!t.is_empty() && inner(p, &t[1..])),
            b'?' => !t.is_empty() && inner(&p[1..], &t[1..]),
            c => !t.is_empty() && c == t[0] && inner(&p[1..], &t[1..]),
        }
    }
    inner(pattern.as_bytes(), text.as_bytes())
}

fn walk_entries(root: &std::path::Path) -> Result<Vec<(String, String, bool, i64)>, String> {
    let mut out = Vec::new();
    fn walk_dir(
        root: &std::path::Path,
        dir: &std::path::Path,
        depth: i64,
        out: &mut Vec<(String, String, bool, i64)>,
    ) -> Result<(), String> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            entries.push(entry.map_err(|e| e.to_string())?);
        }
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let p = entry.path();
            let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            let relative = p
                .strip_prefix(root)
                .unwrap_or(&p)
                .to_string_lossy()
                .to_string();
            out.push((p.to_string_lossy().to_string(), relative, is_dir, depth));
            if is_dir {
                walk_dir(root, &p, depth + 1, out)?;
            }
        }
        Ok(())
    }
    walk_dir(root, root, 0, &mut out)?;
    Ok(out)
}

fn path_record(path: String) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let rec = rt.heap.alloc_record(1);
        let sid = rt.heap.alloc_string(path);
        let _ = rt.heap.record_set_string(rec, 0, sid);
        rec
    })
}

fn path_string_from_record(rec: i64) -> String {
    Concurrency::with_runtime_mut(|rt| {
        let sid = rt.heap.record_get_string(rec, 0).unwrap_or(0);
        rt.heap.clone_string(sid).unwrap_or_default()
    })
}

fn option_string_bits(s: Option<String>) -> i64 {
    match s {
        None => 0,
        Some(v) => {
            let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(v));
            sid.wrapping_add(1)
        }
    }
}

fn env_validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("empty name".into());
    }
    if name.contains('\0') {
        return Err("name contains NUL".into());
    }
    Ok(())
}

fn env_validate_value(value: &str) -> Result<(), String> {
    if value.contains('\0') {
        return Err("value contains NUL".into());
    }
    Ok(())
}

fn jet_temp_path(prefix: &str) -> String {
    let clean: String = prefix
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir()
        .join(format!("{}_{}_{}", clean, std::process::id(), nanos))
        .to_string_lossy()
        .to_string()
}

extern "C" fn jet_jit_fs_remove(path: i64) -> i64 {
    let p = clone_heap_string(path);
    let res = std::fs::remove_file(&p).or_else(|_| std::fs::remove_dir(&p));
    match res {
        Ok(()) => result_ok_bits(0),
        Err(e) => result_err_msg(&format!("remove {p}: {e}")),
    }
}

extern "C" fn jet_jit_fs_remove_all(path: i64) -> i64 {
    let p = clone_heap_string(path);
    let path = std::path::Path::new(&p);
    let res = if path.is_dir() {
        std::fs::remove_dir_all(&p)
    } else {
        std::fs::remove_file(&p)
    };
    match res {
        Ok(()) => result_ok_bits(0),
        Err(e) => result_err_msg(&format!("remove_all {p}: {e}")),
    }
}

extern "C" fn jet_jit_fs_stat(path: i64) -> i64 {
    let p = clone_heap_string(path);
    let meta = match std::fs::symlink_metadata(&p) {
        Ok(m) => m,
        Err(e) => return result_err_msg(&format!("stat {p}: {e}")),
    };
    let ft = meta.file_type();
    let kind = if ft.is_symlink() {
        "symlink"
    } else if ft.is_dir() {
        "dir"
    } else if ft.is_file() {
        "file"
    } else {
        "other"
    };
    let rec = Concurrency::with_runtime_mut(|rt| {
        let rec = rt.heap.alloc_record(8);
        let _ = rt.heap.record_set_int(rec, 0, meta.len() as i64);
        let _ = rt
            .heap
            .record_set_int(rec, 1, meta.modified().ok().and_then(system_time_ms).unwrap_or(0));
        let _ = rt
            .heap
            .record_set_int(rec, 2, meta.created().ok().and_then(system_time_ms).unwrap_or(0));
        let _ = rt
            .heap
            .record_set_bool(rec, 3, meta.permissions().readonly());
        let _ = rt.heap.record_set_bool(rec, 4, ft.is_file());
        let _ = rt.heap.record_set_bool(rec, 5, ft.is_dir());
        let _ = rt.heap.record_set_bool(rec, 6, ft.is_symlink());
        let kid = rt.heap.alloc_string(kind.to_string());
        let _ = rt.heap.record_set_string(rec, 7, kid);
        rec
    });
    result_ok_bits(rec as u64)
}

extern "C" fn jet_jit_fs_read_at(path: i64, offset: i64, len: i64) -> i64 {
    use std::io::{Read, Seek, SeekFrom};
    let p = clone_heap_string(path);
    let mut f = match std::fs::File::open(&p) {
        Ok(f) => f,
        Err(e) => return result_err_msg(&format!("read_at {p}: {e}")),
    };
    if let Err(e) = f.seek(SeekFrom::Start(offset.max(0) as u64)) {
        return result_err_msg(&format!("read_at {p}: {e}"));
    }
    let mut buf = vec![0u8; len.max(0) as usize];
    let n = match f.read(&mut buf) {
        Ok(n) => n,
        Err(e) => return result_err_msg(&format!("read_at {p}: {e}")),
    };
    buf.truncate(n);
    result_ok_bits(alloc_byte_list(&buf) as u64)
}

extern "C" fn jet_jit_fs_write_at(path: i64, offset: i64, bytes: i64) -> i64 {
    use std::io::{Seek, SeekFrom, Write};
    let p = clone_heap_string(path);
    let data = clone_heap_bytes(bytes);
    let mut f = match std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&p)
    {
        Ok(f) => f,
        Err(e) => return result_err_msg(&format!("write_at {p}: {e}")),
    };
    if let Err(e) = f.seek(SeekFrom::Start(offset.max(0) as u64)) {
        return result_err_msg(&format!("write_at {p}: {e}"));
    }
    match f.write_all(&data) {
        Ok(()) => result_ok_bits(0),
        Err(e) => result_err_msg(&format!("write_at {p}: {e}")),
    }
}

extern "C" fn jet_jit_fs_fsync(path: i64) -> i64 {
    let p = clone_heap_string(path);
    match std::fs::OpenOptions::new()
        .read(true)
        .open(&p)
        .and_then(|f| f.sync_all())
    {
        Ok(()) => result_ok_bits(0),
        Err(e) => result_err_msg(&format!("fsync {p}: {e}")),
    }
}

extern "C" fn jet_jit_fs_write_atomic(path: i64, bytes: i64) -> i64 {
    let p = clone_heap_string(path);
    let data = clone_heap_bytes(bytes);
    let path = std::path::Path::new(&p);
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        Some(_) => std::path::Path::new("."),
        None => return result_err_msg(&format!("write_atomic {p}: path has no parent")),
    };
    let tmp = parent.join(format!(
        ".jet_atomic_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    if let Err(e) = std::fs::write(&tmp, &data) {
        return result_err_msg(&format!("write_atomic {p}: {e}"));
    }
    match std::fs::rename(&tmp, path) {
        Ok(()) => result_ok_bits(0),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            result_err_msg(&format!("write_atomic {p}: {e}"))
        }
    }
}

extern "C" fn jet_jit_fs_walk(path: i64) -> i64 {
    let p = clone_heap_string(path);
    let entries = match walk_entries(std::path::Path::new(&p)) {
        Ok(e) => e,
        Err(e) => return result_err_msg(&format!("walk {p}: {e}")),
    };
    let list = Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for (path, relative, is_dir, depth) in entries {
            let rec = rt.heap.alloc_record(4);
            let ps = rt.heap.alloc_string(path);
            let rs = rt.heap.alloc_string(relative);
            let _ = rt.heap.record_set_string(rec, 0, ps);
            let _ = rt.heap.record_set_string(rec, 1, rs);
            let _ = rt.heap.record_set_bool(rec, 2, is_dir);
            let _ = rt.heap.record_set_int(rec, 3, depth);
            let _ = rt.heap.list_push_int(list, rec);
        }
        list
    });
    result_ok_bits(list as u64)
}

extern "C" fn jet_jit_fs_glob(pattern: i64) -> i64 {
    let pat = clone_heap_string(pattern);
    let split = pat.find(['*', '?']).unwrap_or(pat.len());
    let base = pat[..split]
        .rsplit_once(std::path::MAIN_SEPARATOR)
        .map(|(dir, _)| if dir.is_empty() { "." } else { dir })
        .unwrap_or(".");
    let entries = match walk_entries(std::path::Path::new(base)) {
        Ok(e) => e,
        Err(e) => return result_err_msg(&format!("glob {pat}: {e}")),
    };
    let mut matches: Vec<String> = entries
        .into_iter()
        .map(|(path, _, _, _)| path)
        .filter(|path| glob_match(&pat, path))
        .collect();
    matches.sort();
    let list = Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for path in matches {
            let sid = rt.heap.alloc_string(path);
            let _ = rt.heap.list_push_int(list, sid);
        }
        list
    });
    result_ok_bits(list as u64)
}

extern "C" fn jet_jit_fs_symlink(from: i64, to: i64) -> i64 {
    let src = clone_heap_string(from);
    let dst = clone_heap_string(to);
    #[cfg(unix)]
    let res = std::os::unix::fs::symlink(&src, &dst);
    #[cfg(windows)]
    let res = {
        let meta = std::fs::metadata(&src);
        match meta {
            Ok(m) if m.is_dir() => std::os::windows::fs::symlink_dir(&src, &dst),
            _ => std::os::windows::fs::symlink_file(&src, &dst),
        }
    };
    match res {
        Ok(()) => result_ok_bits(0),
        Err(e) => result_err_msg(&format!("symlink {dst}: {e}")),
    }
}

extern "C" fn jet_jit_fs_read_link(path: i64) -> i64 {
    let p = clone_heap_string(path);
    match std::fs::read_link(&p) {
        Ok(target) => {
            let sid = Concurrency::with_runtime_mut(|rt| {
                rt.heap
                    .alloc_string(target.to_string_lossy().to_string())
            });
            result_ok_bits(sid as u64)
        }
        Err(e) => result_err_msg(&format!("read_link {p}: {e}")),
    }
}

extern "C" fn jet_jit_fs_hard_link(from: i64, to: i64) -> i64 {
    let src = clone_heap_string(from);
    let dst = clone_heap_string(to);
    match std::fs::hard_link(&src, &dst) {
        Ok(()) => result_ok_bits(0),
        Err(e) => result_err_msg(&format!("hard_link {dst}: {e}")),
    }
}

extern "C" fn jet_jit_fs_canonicalize(path: i64) -> i64 {
    let p = clone_heap_string(path);
    match std::fs::canonicalize(&p) {
        Ok(abs) => {
            let sid = Concurrency::with_runtime_mut(|rt| {
                rt.heap.alloc_string(abs.to_string_lossy().to_string())
            });
            result_ok_bits(sid as u64)
        }
        Err(e) => result_err_msg(&format!("canonicalize {p}: {e}")),
    }
}

extern "C" fn jet_jit_fs_absolute(path: i64) -> i64 {
    let p = clone_heap_string(path);
    let path = std::path::Path::new(&p);
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(path),
            Err(e) => return result_err_msg(&format!("absolute {p}: {e}")),
        }
    };
    let sid = Concurrency::with_runtime_mut(|rt| {
        rt.heap.alloc_string(abs.to_string_lossy().to_string())
    });
    result_ok_bits(sid as u64)
}

extern "C" fn jet_jit_fs_copy_dir(from: i64, to: i64) -> i64 {
    let src = clone_heap_string(from);
    let dst = clone_heap_string(to);
    fn copy_tree(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
        std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
        for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            let ft = entry.file_type().map_err(|e| e.to_string())?;
            if ft.is_dir() {
                copy_tree(&src_path, &dst_path)?;
            } else if ft.is_file() {
                std::fs::copy(&src_path, &dst_path).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    match copy_tree(std::path::Path::new(&src), std::path::Path::new(&dst)) {
        Ok(()) => result_ok_bits(0),
        Err(e) => result_err_msg(&format!("copy_dir {src}: {e}")),
    }
}

extern "C" fn jet_jit_fs_temp_dir(prefix: i64) -> i64 {
    let pref = clone_heap_string(prefix);
    let path = jet_temp_path(&pref);
    match std::fs::create_dir(&path) {
        Ok(()) => result_ok_bits(path_record(path) as u64),
        Err(e) => result_err_msg(&format!("temp_dir {path}: {e}")),
    }
}

extern "C" fn jet_jit_fs_temp_file(prefix: i64) -> i64 {
    let pref = clone_heap_string(prefix);
    let path = jet_temp_path(&pref);
    match std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
    {
        Ok(_) => result_ok_bits(path_record(path) as u64),
        Err(e) => result_err_msg(&format!("temp_file {path}: {e}")),
    }
}

extern "C" fn jet_jit_fs_lock(path: i64) -> i64 {
    let p = clone_heap_string(path);
    match std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&p)
    {
        Ok(_) => result_ok_bits(path_record(p) as u64),
        Err(e) => result_err_msg(&format!("lock {p}: {e}")),
    }
}

// ── core.math (mirrors jet_std_math_* / f64 methods in Process.rs emit) ───────

extern "C" fn jet_jit_math_sin(x: f64) -> f64 {
    x.sin()
}
extern "C" fn jet_jit_math_cos(x: f64) -> f64 {
    x.cos()
}
extern "C" fn jet_jit_math_exp(x: f64) -> f64 {
    x.exp()
}
extern "C" fn jet_jit_math_atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}
extern "C" fn jet_jit_math_hypot(a: f64, b: f64) -> f64 {
    a.hypot(b)
}
extern "C" fn jet_jit_math_lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}
extern "C" fn jet_jit_math_degrees(x: f64) -> f64 {
    x.to_degrees()
}
extern "C" fn jet_jit_math_radians(x: f64) -> f64 {
    x.to_radians()
}
extern "C" fn jet_jit_math_is_finite(x: f64) -> i8 {
    i8::from(x.is_finite())
}
extern "C" fn jet_jit_math_sign(x: f64) -> i64 {
    if x > 0.0 {
        1
    } else if x < 0.0 {
        -1
    } else {
        0
    }
}

/// Packed Option<i64> ABI: `0` = None, else `bits.wrapping_add(1)`.
extern "C" fn jet_jit_math_checked_add(a: i64, b: i64) -> i64 {
    match a.checked_add(b) {
        Some(v) => v.wrapping_add(1),
        None => 0,
    }
}

extern "C" fn jet_jit_math_saturating_add(a: i64, b: i64) -> i64 {
    a.saturating_add(b)
}
extern "C" fn jet_jit_math_wrapping_add(a: i64, b: i64) -> i64 {
    a.wrapping_add(b)
}

/// Mirrors `jet_std_math_int_pow`.
extern "C" fn jet_jit_math_int_pow(base: i64, exp: i64) -> i64 {
    if exp < 0 {
        return 0;
    }
    base.saturating_pow(exp as u32)
}

/// Mirrors `jet_std_math_gcd`.
extern "C" fn jet_jit_math_gcd(mut a: i64, mut b: i64) -> i64 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}

/// Mirrors `jet_std_math_lcm`.
extern "C" fn jet_jit_math_lcm(a: i64, b: i64) -> i64 {
    if a == 0 || b == 0 {
        0
    } else {
        (a / jet_jit_math_gcd(a, b)).saturating_mul(b).abs()
    }
}

extern "C" fn jet_jit_math_sqrt(x: f64) -> f64 {
    x.sqrt()
}
extern "C" fn jet_jit_math_sqrt_f32(x: f64) -> f64 {
    ((x as f32).sqrt()) as f64
}
extern "C" fn jet_jit_math_pow(base: f64, exp: f64) -> f64 {
    base.powf(exp)
}
extern "C" fn jet_jit_math_pow_f32(base: f64, exp: f64) -> f64 {
    ((base as f32).powf(exp as f32)) as f64
}
extern "C" fn jet_jit_math_floor(x: f64) -> f64 {
    x.floor()
}
extern "C" fn jet_jit_math_floor_f32(x: f64) -> f64 {
    ((x as f32).floor()) as f64
}
extern "C" fn jet_jit_math_ceil(x: f64) -> f64 {
    x.ceil()
}
extern "C" fn jet_jit_math_ceil_f32(x: f64) -> f64 {
    ((x as f32).ceil()) as f64
}

// ── core.env / core.process (mirrors jet_std_env_get / jet_std_process_exit) ─

/// Option ABI: `0` = None, else string-handle+1 (same as list_get_opt).
extern "C" fn jet_jit_env_get(name: i64) -> i64 {
    let key = clone_heap_string(name);
    match std::env::var(&key) {
        Ok(v) => {
            let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(v));
            sid.wrapping_add(1)
        }
        Err(_) => 0,
    }
}

extern "C" fn jet_jit_env_set(name: i64, value: i64) -> i64 {
    let key = clone_heap_string(name);
    let val = clone_heap_string(value);
    if let Err(e) = env_validate_name(&key) {
        return result_err_msg(&format!("env set: {e}"));
    }
    if let Err(e) = env_validate_value(&val) {
        return result_err_msg(&format!("env set: {e}"));
    }
    std::env::set_var(key, val);
    result_ok_bits(0)
}

extern "C" fn jet_jit_env_unset(name: i64) -> i64 {
    let key = clone_heap_string(name);
    if let Err(e) = env_validate_name(&key) {
        return result_err_msg(&format!("env unset: {e}"));
    }
    let existed = std::env::var_os(&key).is_some();
    std::env::remove_var(&key);
    result_ok_bits(u64::from(existed))
}

extern "C" fn jet_jit_env_vars() -> i64 {
    let mut names = Vec::new();
    for (name, value) in std::env::vars_os() {
        let Some(decoded) = name.to_str() else {
            return result_err_msg("environment is not Unicode");
        };
        if value.to_str().is_none() {
            return result_err_msg("environment is not Unicode");
        }
        names.push(decoded.to_string());
    }
    names.sort();
    let list = Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for name in names {
            let sid = rt.heap.alloc_string(name);
            let _ = rt.heap.list_push_int(list, sid);
        }
        list
    });
    result_ok_bits(list as u64)
}

extern "C" fn jet_jit_io_input(has_prompt: i8, prompt: i64) -> i64 {
    use std::io::Write;
    if has_prompt != 0 {
        let p = clone_heap_string(prompt);
        print!("{p}");
        if let Err(e) = std::io::stdout().flush() {
            return result_err_msg(&format!("flush stdout: {e}"));
        }
    }
    let mut s = String::new();
    if let Err(e) = std::io::stdin().read_line(&mut s) {
        return result_err_msg(&format!("read stdin: {e}"));
    }
    while s.ends_with('\n') || s.ends_with('\r') {
        s.pop();
    }
    let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s));
    result_ok_bits(sid as u64)
}

extern "C" fn jet_jit_process_exit(code: i64) {
    // Soft exit: set the code + trap so `resident_invoke` returns `Ran` with
    // that exit status. Never `std::process::exit` — that would kill the
    // resident/test process (three-way battery, `jet serve`, …).
    Concurrency::with_runtime_mut(|rt| {
        rt.exit_code = Some(code as i32);
        rt.set_trap("__jet_process_exit__");
    });
}

pub(crate) struct CoreHostFns {
    pub os_name: cranelift_module::FuncId,
    pub os_family: cranelift_module::FuncId,
    pub os_arch: cranelift_module::FuncId,
    pub os_cpu_count: cranelift_module::FuncId,
    pub os_temp_dir: cranelift_module::FuncId,
    pub os_executable: cranelift_module::FuncId,
    pub os_pid: cranelift_module::FuncId,
    pub os_hostname: cranelift_module::FuncId,
    pub log_set_level: cranelift_module::FuncId,
    pub log_setup: cranelift_module::FuncId,
    pub log_debug: cranelift_module::FuncId,
    pub log_info: cranelift_module::FuncId,
    pub log_warn: cranelift_module::FuncId,
    pub log_error: cranelift_module::FuncId,
    pub log_set_trace_id: cranelift_module::FuncId,
    pub log_field: cranelift_module::FuncId,
    pub log_int_field: cranelift_module::FuncId,
    pub log_bool_field: cranelift_module::FuncId,
    pub log_counter: cranelift_module::FuncId,
    pub log_span: cranelift_module::FuncId,
    pub log_enter: cranelift_module::FuncId,
    pub log_close: cranelift_module::FuncId,
    pub log_info_fields: cranelift_module::FuncId,
    pub fs_exists: cranelift_module::FuncId,
    pub fs_read: cranelift_module::FuncId,
    pub fs_write: cranelift_module::FuncId,
    pub fs_create_dir: cranelift_module::FuncId,
    pub fs_list_dir: cranelift_module::FuncId,
    pub fs_remove_all: cranelift_module::FuncId,
    pub fs_remove: cranelift_module::FuncId,
    pub fs_stat: cranelift_module::FuncId,
    pub fs_read_at: cranelift_module::FuncId,
    pub fs_write_at: cranelift_module::FuncId,
    pub fs_fsync: cranelift_module::FuncId,
    pub fs_write_atomic: cranelift_module::FuncId,
    pub fs_walk: cranelift_module::FuncId,
    pub fs_glob: cranelift_module::FuncId,
    pub fs_symlink: cranelift_module::FuncId,
    pub fs_read_link: cranelift_module::FuncId,
    pub fs_hard_link: cranelift_module::FuncId,
    pub fs_canonicalize: cranelift_module::FuncId,
    pub fs_absolute: cranelift_module::FuncId,
    pub fs_copy_dir: cranelift_module::FuncId,
    pub fs_temp_dir: cranelift_module::FuncId,
    pub fs_temp_file: cranelift_module::FuncId,
    pub fs_lock: cranelift_module::FuncId,
    pub path_join: cranelift_module::FuncId,
    pub path_parent_str: cranelift_module::FuncId,
    pub path_extension_str: cranelift_module::FuncId,
    pub path_normalize_str: cranelift_module::FuncId,
    pub path_from: cranelift_module::FuncId,
    pub path_write_atomic: cranelift_module::FuncId,
    pub path_join_handle: cranelift_module::FuncId,
    pub path_parent: cranelift_module::FuncId,
    pub path_extension: cranelift_module::FuncId,
    pub path_stem: cranelift_module::FuncId,
    pub path_to_string: cranelift_module::FuncId,
    pub path_walk: cranelift_module::FuncId,
    pub math_sin: cranelift_module::FuncId,
    pub math_cos: cranelift_module::FuncId,
    pub math_exp: cranelift_module::FuncId,
    pub math_atan2: cranelift_module::FuncId,
    pub math_hypot: cranelift_module::FuncId,
    pub math_lerp: cranelift_module::FuncId,
    pub math_degrees: cranelift_module::FuncId,
    pub math_radians: cranelift_module::FuncId,
    pub math_is_finite: cranelift_module::FuncId,
    pub math_sign: cranelift_module::FuncId,
    pub math_checked_add: cranelift_module::FuncId,
    pub math_saturating_add: cranelift_module::FuncId,
    pub math_wrapping_add: cranelift_module::FuncId,
    pub math_int_pow: cranelift_module::FuncId,
    pub math_gcd: cranelift_module::FuncId,
    pub math_lcm: cranelift_module::FuncId,
    pub math_sqrt: cranelift_module::FuncId,
    pub math_sqrt_f32: cranelift_module::FuncId,
    pub math_pow: cranelift_module::FuncId,
    pub math_pow_f32: cranelift_module::FuncId,
    pub math_floor: cranelift_module::FuncId,
    pub math_floor_f32: cranelift_module::FuncId,
    pub math_ceil: cranelift_module::FuncId,
    pub math_ceil_f32: cranelift_module::FuncId,
    pub env_get: cranelift_module::FuncId,
    pub env_set: cranelift_module::FuncId,
    pub env_unset: cranelift_module::FuncId,
    pub env_vars: cranelift_module::FuncId,
    pub io_input: cranelift_module::FuncId,
    pub process_exit: cranelift_module::FuncId,
}

pub(crate) fn register_core_host_symbols(builder: &mut cranelift_jit::JITBuilder) {
    builder.symbol("jet_jit_os_name", jet_jit_os_name as *const u8);
    builder.symbol("jet_jit_os_family", jet_jit_os_family as *const u8);
    builder.symbol("jet_jit_os_arch", jet_jit_os_arch as *const u8);
    builder.symbol("jet_jit_os_cpu_count", jet_jit_os_cpu_count as *const u8);
    builder.symbol("jet_jit_os_temp_dir", jet_jit_os_temp_dir as *const u8);
    builder.symbol("jet_jit_os_executable", jet_jit_os_executable as *const u8);
    builder.symbol("jet_jit_os_pid", jet_jit_os_pid as *const u8);
    builder.symbol("jet_jit_os_hostname", jet_jit_os_hostname as *const u8);
    builder.symbol("jet_jit_log_set_level", jet_jit_log_set_level as *const u8);
    builder.symbol("jet_jit_log_setup", jet_jit_log_setup as *const u8);
    builder.symbol("jet_jit_log_debug", jet_jit_log_debug as *const u8);
    builder.symbol("jet_jit_log_info", jet_jit_log_info as *const u8);
    builder.symbol("jet_jit_log_warn", jet_jit_log_warn as *const u8);
    builder.symbol("jet_jit_log_error", jet_jit_log_error as *const u8);
    builder.symbol(
        "jet_jit_log_set_trace_id",
        jet_jit_log_set_trace_id as *const u8,
    );
    builder.symbol("jet_jit_log_field", jet_jit_log_field as *const u8);
    builder.symbol("jet_jit_log_int_field", jet_jit_log_int_field as *const u8);
    builder.symbol("jet_jit_log_bool_field", jet_jit_log_bool_field as *const u8);
    builder.symbol("jet_jit_log_counter", jet_jit_log_counter as *const u8);
    builder.symbol("jet_jit_log_span", jet_jit_log_span as *const u8);
    builder.symbol("jet_jit_log_enter", jet_jit_log_enter as *const u8);
    builder.symbol("jet_jit_log_close", jet_jit_log_close as *const u8);
    builder.symbol(
        "jet_jit_log_info_fields",
        jet_jit_log_info_fields as *const u8,
    );
    builder.symbol("jet_jit_fs_exists", jet_jit_fs_exists as *const u8);
    builder.symbol("jet_jit_fs_read", jet_jit_fs_read as *const u8);
    builder.symbol("jet_jit_fs_write", jet_jit_fs_write as *const u8);
    builder.symbol("jet_jit_fs_create_dir", jet_jit_fs_create_dir as *const u8);
    builder.symbol("jet_jit_fs_list_dir", jet_jit_fs_list_dir as *const u8);
    builder.symbol("jet_jit_fs_remove_all", jet_jit_fs_remove_all as *const u8);
    builder.symbol("jet_jit_fs_remove", jet_jit_fs_remove as *const u8);
    builder.symbol("jet_jit_fs_stat", jet_jit_fs_stat as *const u8);
    builder.symbol("jet_jit_fs_read_at", jet_jit_fs_read_at as *const u8);
    builder.symbol("jet_jit_fs_write_at", jet_jit_fs_write_at as *const u8);
    builder.symbol("jet_jit_fs_fsync", jet_jit_fs_fsync as *const u8);
    builder.symbol("jet_jit_fs_write_atomic", jet_jit_fs_write_atomic as *const u8);
    builder.symbol("jet_jit_fs_walk", jet_jit_fs_walk as *const u8);
    builder.symbol("jet_jit_fs_glob", jet_jit_fs_glob as *const u8);
    builder.symbol("jet_jit_fs_symlink", jet_jit_fs_symlink as *const u8);
    builder.symbol("jet_jit_fs_read_link", jet_jit_fs_read_link as *const u8);
    builder.symbol("jet_jit_fs_hard_link", jet_jit_fs_hard_link as *const u8);
    builder.symbol("jet_jit_fs_canonicalize", jet_jit_fs_canonicalize as *const u8);
    builder.symbol("jet_jit_fs_absolute", jet_jit_fs_absolute as *const u8);
    builder.symbol("jet_jit_fs_copy_dir", jet_jit_fs_copy_dir as *const u8);
    builder.symbol("jet_jit_fs_temp_dir", jet_jit_fs_temp_dir as *const u8);
    builder.symbol("jet_jit_fs_temp_file", jet_jit_fs_temp_file as *const u8);
    builder.symbol("jet_jit_fs_lock", jet_jit_fs_lock as *const u8);
    builder.symbol("jet_jit_path_join", jet_jit_path_join as *const u8);
    builder.symbol("jet_jit_path_parent_str", jet_jit_path_parent_str as *const u8);
    builder.symbol("jet_jit_path_extension_str", jet_jit_path_extension_str as *const u8);
    builder.symbol("jet_jit_path_normalize_str", jet_jit_path_normalize_str as *const u8);
    builder.symbol("jet_jit_path_from", jet_jit_path_from as *const u8);
    builder.symbol("jet_jit_path_write_atomic", jet_jit_path_write_atomic as *const u8);
    builder.symbol("jet_jit_path_join_handle", jet_jit_path_join_handle as *const u8);
    builder.symbol("jet_jit_path_parent", jet_jit_path_parent as *const u8);
    builder.symbol("jet_jit_path_extension", jet_jit_path_extension as *const u8);
    builder.symbol("jet_jit_path_stem", jet_jit_path_stem as *const u8);
    builder.symbol("jet_jit_path_to_string", jet_jit_path_to_string as *const u8);
    builder.symbol("jet_jit_path_walk", jet_jit_path_walk as *const u8);
    builder.symbol("jet_jit_math_sin", jet_jit_math_sin as *const u8);
    builder.symbol("jet_jit_math_cos", jet_jit_math_cos as *const u8);
    builder.symbol("jet_jit_math_exp", jet_jit_math_exp as *const u8);
    builder.symbol("jet_jit_math_atan2", jet_jit_math_atan2 as *const u8);
    builder.symbol("jet_jit_math_hypot", jet_jit_math_hypot as *const u8);
    builder.symbol("jet_jit_math_lerp", jet_jit_math_lerp as *const u8);
    builder.symbol("jet_jit_math_degrees", jet_jit_math_degrees as *const u8);
    builder.symbol("jet_jit_math_radians", jet_jit_math_radians as *const u8);
    builder.symbol("jet_jit_math_is_finite", jet_jit_math_is_finite as *const u8);
    builder.symbol("jet_jit_math_sign", jet_jit_math_sign as *const u8);
    builder.symbol("jet_jit_math_checked_add", jet_jit_math_checked_add as *const u8);
    builder.symbol(
        "jet_jit_math_saturating_add",
        jet_jit_math_saturating_add as *const u8,
    );
    builder.symbol(
        "jet_jit_math_wrapping_add",
        jet_jit_math_wrapping_add as *const u8,
    );
    builder.symbol("jet_jit_math_int_pow", jet_jit_math_int_pow as *const u8);
    builder.symbol("jet_jit_math_gcd", jet_jit_math_gcd as *const u8);
    builder.symbol("jet_jit_math_lcm", jet_jit_math_lcm as *const u8);
    builder.symbol("jet_jit_math_sqrt", jet_jit_math_sqrt as *const u8);
    builder.symbol("jet_jit_math_sqrt_f32", jet_jit_math_sqrt_f32 as *const u8);
    builder.symbol("jet_jit_math_pow", jet_jit_math_pow as *const u8);
    builder.symbol("jet_jit_math_pow_f32", jet_jit_math_pow_f32 as *const u8);
    builder.symbol("jet_jit_math_floor", jet_jit_math_floor as *const u8);
    builder.symbol("jet_jit_math_floor_f32", jet_jit_math_floor_f32 as *const u8);
    builder.symbol("jet_jit_math_ceil", jet_jit_math_ceil as *const u8);
    builder.symbol("jet_jit_math_ceil_f32", jet_jit_math_ceil_f32 as *const u8);
    builder.symbol("jet_jit_env_get", jet_jit_env_get as *const u8);
    builder.symbol("jet_jit_env_set", jet_jit_env_set as *const u8);
    builder.symbol("jet_jit_env_unset", jet_jit_env_unset as *const u8);
    builder.symbol("jet_jit_env_vars", jet_jit_env_vars as *const u8);
    builder.symbol("jet_jit_io_input", jet_jit_io_input as *const u8);
    builder.symbol("jet_jit_process_exit", jet_jit_process_exit as *const u8);
}

pub(crate) fn declare_core_host_fns(
    module: &mut cranelift_jit::JITModule,
) -> Result<CoreHostFns, String> {
    use cranelift_codegen::ir::{types, AbiParam, Signature};
    use cranelift_module::{Linkage, Module};

    let cc = module.target_config().default_call_conv;
    let mut sig_str = Signature::new(cc);
    sig_str.returns.push(AbiParam::new(types::I64));
    let mut sig_i64 = Signature::new(cc);
    sig_i64.returns.push(AbiParam::new(types::I64));
    let mut sig_void_str = Signature::new(cc);
    sig_void_str.params.push(AbiParam::new(types::I64));
    let mut sig_str_str_str = Signature::new(cc);
    sig_str_str_str.params.push(AbiParam::new(types::I64));
    sig_str_str_str.params.push(AbiParam::new(types::I64));
    sig_str_str_str.returns.push(AbiParam::new(types::I64));
    let mut sig_str_i64_str = Signature::new(cc);
    sig_str_i64_str.params.push(AbiParam::new(types::I64));
    sig_str_i64_str.params.push(AbiParam::new(types::I64));
    sig_str_i64_str.returns.push(AbiParam::new(types::I64));
    let mut sig_str_i8_str = Signature::new(cc);
    sig_str_i8_str.params.push(AbiParam::new(types::I64));
    sig_str_i8_str.params.push(AbiParam::new(types::I8));
    sig_str_i8_str.returns.push(AbiParam::new(types::I64));
    let mut sig_void_i64 = Signature::new(cc);
    sig_void_i64.params.push(AbiParam::new(types::I64));
    let mut sig_void_i64_i64 = Signature::new(cc);
    sig_void_i64_i64.params.push(AbiParam::new(types::I64));
    sig_void_i64_i64.params.push(AbiParam::new(types::I64));
    let mut sig_unary_i64 = Signature::new(cc);
    sig_unary_i64.params.push(AbiParam::new(types::I64));
    sig_unary_i64.returns.push(AbiParam::new(types::I64));
    let mut sig_i64_i8 = Signature::new(cc);
    sig_i64_i8.params.push(AbiParam::new(types::I64));
    sig_i64_i8.returns.push(AbiParam::new(types::I8));
    let mut sig_f64_f64 = Signature::new(cc);
    sig_f64_f64.params.push(AbiParam::new(types::F64));
    sig_f64_f64.returns.push(AbiParam::new(types::F64));
    let mut sig_f64_f64_f64 = Signature::new(cc);
    sig_f64_f64_f64.params.push(AbiParam::new(types::F64));
    sig_f64_f64_f64.params.push(AbiParam::new(types::F64));
    sig_f64_f64_f64.returns.push(AbiParam::new(types::F64));
    let mut sig_lerp = Signature::new(cc);
    sig_lerp.params.push(AbiParam::new(types::F64));
    sig_lerp.params.push(AbiParam::new(types::F64));
    sig_lerp.params.push(AbiParam::new(types::F64));
    sig_lerp.returns.push(AbiParam::new(types::F64));
    let mut sig_f64_i8 = Signature::new(cc);
    sig_f64_i8.params.push(AbiParam::new(types::F64));
    sig_f64_i8.returns.push(AbiParam::new(types::I8));
    let mut sig_f64_i64 = Signature::new(cc);
    sig_f64_i64.params.push(AbiParam::new(types::F64));
    sig_f64_i64.returns.push(AbiParam::new(types::I64));
    let mut sig_i64_i64_i64 = Signature::new(cc);
    sig_i64_i64_i64.params.push(AbiParam::new(types::I64));
    sig_i64_i64_i64.params.push(AbiParam::new(types::I64));
    sig_i64_i64_i64.returns.push(AbiParam::new(types::I64));
    let mut sig_i64_i64_i64_i64 = Signature::new(cc);
    sig_i64_i64_i64_i64.params.push(AbiParam::new(types::I64));
    sig_i64_i64_i64_i64.params.push(AbiParam::new(types::I64));
    sig_i64_i64_i64_i64.params.push(AbiParam::new(types::I64));
    sig_i64_i64_i64_i64.returns.push(AbiParam::new(types::I64));

    let mut sig_i8_i64_i64 = Signature::new(cc);
    sig_i8_i64_i64.params.push(AbiParam::new(types::I8));
    sig_i8_i64_i64.params.push(AbiParam::new(types::I64));
    sig_i8_i64_i64.returns.push(AbiParam::new(types::I64));

    let mut import = |name: &str, sig: &Signature| -> Result<cranelift_module::FuncId, String> {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };

    Ok(CoreHostFns {
        os_name: import("jet_jit_os_name", &sig_str)?,
        os_family: import("jet_jit_os_family", &sig_str)?,
        os_arch: import("jet_jit_os_arch", &sig_str)?,
        os_cpu_count: import("jet_jit_os_cpu_count", &sig_i64)?,
        os_temp_dir: import("jet_jit_os_temp_dir", &sig_str)?,
        os_executable: import("jet_jit_os_executable", &sig_str)?,
        os_pid: import("jet_jit_os_pid", &sig_i64)?,
        os_hostname: import("jet_jit_os_hostname", &sig_str)?,
        log_set_level: import("jet_jit_log_set_level", &sig_void_str)?,
        log_setup: import("jet_jit_log_setup", &sig_void_str)?,
        log_debug: import("jet_jit_log_debug", &sig_void_str)?,
        log_info: import("jet_jit_log_info", &sig_void_str)?,
        log_warn: import("jet_jit_log_warn", &sig_void_str)?,
        log_error: import("jet_jit_log_error", &sig_void_str)?,
        log_set_trace_id: import("jet_jit_log_set_trace_id", &sig_void_str)?,
        log_field: import("jet_jit_log_field", &sig_str_str_str)?,
        log_int_field: import("jet_jit_log_int_field", &sig_str_i64_str)?,
        log_bool_field: import("jet_jit_log_bool_field", &sig_str_i8_str)?,
        log_counter: import("jet_jit_log_counter", &sig_str_i64_str)?,
        log_span: import("jet_jit_log_span", &sig_unary_i64)?,
        log_enter: import("jet_jit_log_enter", &sig_void_i64)?,
        log_close: import("jet_jit_log_close", &sig_void_i64)?,
        log_info_fields: import("jet_jit_log_info_fields", &sig_void_i64_i64)?,
        fs_exists: import("jet_jit_fs_exists", &sig_i64_i8)?,
        fs_read: import("jet_jit_fs_read", &sig_unary_i64)?,
        fs_write: import("jet_jit_fs_write", &sig_i64_i64_i64)?,
        fs_create_dir: import("jet_jit_fs_create_dir", &sig_unary_i64)?,
        fs_list_dir: import("jet_jit_fs_list_dir", &sig_unary_i64)?,
        fs_remove_all: import("jet_jit_fs_remove_all", &sig_unary_i64)?,
        fs_remove: import("jet_jit_fs_remove", &sig_unary_i64)?,
        fs_stat: import("jet_jit_fs_stat", &sig_unary_i64)?,
        fs_read_at: import("jet_jit_fs_read_at", &sig_i64_i64_i64_i64)?,
        fs_write_at: import("jet_jit_fs_write_at", &sig_i64_i64_i64_i64)?,
        fs_fsync: import("jet_jit_fs_fsync", &sig_unary_i64)?,
        fs_write_atomic: import("jet_jit_fs_write_atomic", &sig_i64_i64_i64)?,
        fs_walk: import("jet_jit_fs_walk", &sig_unary_i64)?,
        fs_glob: import("jet_jit_fs_glob", &sig_unary_i64)?,
        fs_symlink: import("jet_jit_fs_symlink", &sig_i64_i64_i64)?,
        fs_read_link: import("jet_jit_fs_read_link", &sig_unary_i64)?,
        fs_hard_link: import("jet_jit_fs_hard_link", &sig_i64_i64_i64)?,
        fs_canonicalize: import("jet_jit_fs_canonicalize", &sig_unary_i64)?,
        fs_absolute: import("jet_jit_fs_absolute", &sig_unary_i64)?,
        fs_copy_dir: import("jet_jit_fs_copy_dir", &sig_i64_i64_i64)?,
        fs_temp_dir: import("jet_jit_fs_temp_dir", &sig_unary_i64)?,
        fs_temp_file: import("jet_jit_fs_temp_file", &sig_unary_i64)?,
        fs_lock: import("jet_jit_fs_lock", &sig_unary_i64)?,
        path_join: import("jet_jit_path_join", &sig_i64_i64_i64)?,
        path_parent_str: import("jet_jit_path_parent_str", &sig_unary_i64)?,
        path_extension_str: import("jet_jit_path_extension_str", &sig_unary_i64)?,
        path_normalize_str: import("jet_jit_path_normalize_str", &sig_unary_i64)?,
        path_from: import("jet_jit_path_from", &sig_unary_i64)?,
        path_write_atomic: import("jet_jit_path_write_atomic", &sig_i64_i64_i64)?,
        path_join_handle: import("jet_jit_path_join_handle", &sig_i64_i64_i64)?,
        path_parent: import("jet_jit_path_parent", &sig_unary_i64)?,
        path_extension: import("jet_jit_path_extension", &sig_unary_i64)?,
        path_stem: import("jet_jit_path_stem", &sig_unary_i64)?,
        path_to_string: import("jet_jit_path_to_string", &sig_unary_i64)?,
        path_walk: import("jet_jit_path_walk", &sig_unary_i64)?,
        math_sin: import("jet_jit_math_sin", &sig_f64_f64)?,
        math_cos: import("jet_jit_math_cos", &sig_f64_f64)?,
        math_exp: import("jet_jit_math_exp", &sig_f64_f64)?,
        math_atan2: import("jet_jit_math_atan2", &sig_f64_f64_f64)?,
        math_hypot: import("jet_jit_math_hypot", &sig_f64_f64_f64)?,
        math_lerp: import("jet_jit_math_lerp", &sig_lerp)?,
        math_degrees: import("jet_jit_math_degrees", &sig_f64_f64)?,
        math_radians: import("jet_jit_math_radians", &sig_f64_f64)?,
        math_is_finite: import("jet_jit_math_is_finite", &sig_f64_i8)?,
        math_sign: import("jet_jit_math_sign", &sig_f64_i64)?,
        math_checked_add: import("jet_jit_math_checked_add", &sig_i64_i64_i64)?,
        math_saturating_add: import("jet_jit_math_saturating_add", &sig_i64_i64_i64)?,
        math_wrapping_add: import("jet_jit_math_wrapping_add", &sig_i64_i64_i64)?,
        math_int_pow: import("jet_jit_math_int_pow", &sig_i64_i64_i64)?,
        math_gcd: import("jet_jit_math_gcd", &sig_i64_i64_i64)?,
        math_lcm: import("jet_jit_math_lcm", &sig_i64_i64_i64)?,
        math_sqrt: import("jet_jit_math_sqrt", &sig_f64_f64)?,
        math_sqrt_f32: import("jet_jit_math_sqrt_f32", &sig_f64_f64)?,
        math_pow: import("jet_jit_math_pow", &sig_f64_f64_f64)?,
        math_pow_f32: import("jet_jit_math_pow_f32", &sig_f64_f64_f64)?,
        math_floor: import("jet_jit_math_floor", &sig_f64_f64)?,
        math_floor_f32: import("jet_jit_math_floor_f32", &sig_f64_f64)?,
        math_ceil: import("jet_jit_math_ceil", &sig_f64_f64)?,
        math_ceil_f32: import("jet_jit_math_ceil_f32", &sig_f64_f64)?,
        env_get: import("jet_jit_env_get", &sig_unary_i64)?,
        env_set: import("jet_jit_env_set", &sig_i64_i64_i64)?,
        env_unset: import("jet_jit_env_unset", &sig_unary_i64)?,
        env_vars: import("jet_jit_env_vars", &sig_i64)?,
        io_input: import("jet_jit_io_input", &sig_i8_i64_i64)?,
        process_exit: import("jet_jit_process_exit", &sig_void_i64)?,
    })
}
