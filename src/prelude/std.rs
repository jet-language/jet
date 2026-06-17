mod jet_std {
    #[derive(Clone, Debug, PartialEq)]
    pub enum IoError {
        NotFound { path: String },
        PermissionDenied { path: String },
        Other { message: String },
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct Utf8Error {
        pub message: String,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct ProcessResult {
        pub code: i64,
        pub output: String,
        pub errors: String,
    }

    #[derive(Clone, Debug)]
    pub struct Stopwatch {
        pub start: std::time::Instant,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct JsonError {
        pub line: i64,
        pub message: String,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub enum Json {
        Null,
        Boolean(bool),
        Number(f64),
        Text(String),
        Array(Vec<Json>),
        Object(std::collections::BTreeMap<String, Json>),
    }

    impl super::JetShow for IoError {
        fn jet_show(&self) -> String { format!("{:?}", self) }
    }
    impl super::JetShow for Utf8Error {
        fn jet_show(&self) -> String { self.message.clone() }
    }
    impl super::JetShow for ProcessResult {
        fn jet_show(&self) -> String { format!("{:?}", self) }
    }
    impl super::JetShow for Stopwatch {
        fn jet_show(&self) -> String { format!("{:?}", self.start) }
    }
    impl super::JetShow for JsonError {
        fn jet_show(&self) -> String { format!("line {}: {}", self.line, self.message) }
    }
    impl super::JetShow for Json {
        fn jet_show(&self) -> String { render_json(self, false, 0) }
    }

    pub struct JetTask<T: Send + 'static> {
        handle: Option<std::thread::JoinHandle<T>>,
    }
    impl<T: Send + 'static> JetTask<T> {
        pub fn spawn<F: FnOnce() -> T + Send + 'static>(f: F) -> JetTask<T> {
            JetTask { handle: Some(std::thread::spawn(f)) }
        }
        pub fn join(mut self) -> T {
            match self.handle.take().unwrap().join() {
                Ok(v) => v,
                Err(_) => {
                    eprintln!("The program stopped: a task panicked");
                    std::process::exit(70);
                }
            }
        }
    }

    pub struct JetChannel<T> {
        recv: std::sync::mpsc::Receiver<T>,
        tx_keeper: std::sync::mpsc::Sender<T>,
    }
    impl<T: Send> JetChannel<T> {
        pub fn new() -> JetChannel<T> {
            let (tx, rx) = std::sync::mpsc::channel();
            JetChannel { recv: rx, tx_keeper: tx }
        }
        pub fn sender(&self) -> JetSender<T> {
            JetSender { tx: self.tx_keeper.clone() }
        }
        pub fn receive(&self) -> Result<T, Closed> {
            self.recv.recv().map_err(|_| Closed::Closed)
        }
    }

    pub struct JetSender<T> {
        tx: std::sync::mpsc::Sender<T>,
    }
    impl<T> JetSender<T> {
        pub fn send(&self, value: T) { let _ = self.tx.send(value); }
    }
    impl<T> Clone for JetSender<T> {
        fn clone(&self) -> Self { JetSender { tx: self.tx.clone() } }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub enum Closed { Closed }

    impl super::JetShow for Closed {
        fn jet_show(&self) -> String { "Closed".to_string() }
    }

    pub fn io_error(path: &str, e: std::io::Error) -> IoError {
        match e.kind() {
            std::io::ErrorKind::NotFound => IoError::NotFound { path: path.to_string() },
            std::io::ErrorKind::PermissionDenied => {
                IoError::PermissionDenied { path: path.to_string() }
            }
            _ => IoError::Other { message: e.to_string() },
        }
    }

    pub fn parse_json(text: &str) -> Result<Json, JsonError> {
        let mut p = JsonParser { chars: text.chars().collect(), pos: 0 };
        let v = p.value()?;
        p.ws();
        if p.pos != p.chars.len() {
            return Err(p.err("extra text after JSON value"));
        }
        Ok(v)
    }

    pub fn render_json(j: &Json, pretty: bool, depth: usize) -> String {
        match j {
            Json::Null => "null".to_string(),
            Json::Boolean(b) => b.to_string(),
            Json::Number(n) => format!("{:?}", n),
            Json::Text(s) => quote_json(s),
            Json::Array(items) => {
                if items.is_empty() {
                    return "[]".to_string();
                }
                if !pretty {
                    let parts: Vec<String> = items.iter().map(|x| render_json(x, false, depth)).collect();
                    return format!("[{}]", parts.join(","));
                }
                let pad = "  ".repeat(depth + 1);
                let end = "  ".repeat(depth);
                let parts: Vec<String> = items
                    .iter()
                    .map(|x| format!("{}{}", pad, render_json(x, true, depth + 1)))
                    .collect();
                format!("[\n{}\n{}]", parts.join(",\n"), end)
            }
            Json::Object(entries) => {
                if entries.is_empty() {
                    return "{}".to_string();
                }
                if !pretty {
                    let parts: Vec<String> = entries
                        .iter()
                        .map(|(k, v)| format!("{}:{}", quote_json(k), render_json(v, false, depth)))
                        .collect();
                    return format!("{{{}}}", parts.join(","));
                }
                let pad = "  ".repeat(depth + 1);
                let end = "  ".repeat(depth);
                let parts: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| format!("{}{}: {}", pad, quote_json(k), render_json(v, true, depth + 1)))
                    .collect();
                format!("{{\n{}\n{}}}", parts.join(",\n"), end)
            }
        }
    }

    fn quote_json(s: &str) -> String {
        let mut out = String::from("\"");
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                _ => out.push(c),
            }
        }
        out.push('"');
        out
    }

    struct JsonParser {
        chars: Vec<char>,
        pos: usize,
    }

    impl JsonParser {
        fn err(&self, msg: &str) -> JsonError {
            let line = self.chars[..self.pos.min(self.chars.len())]
                .iter()
                .filter(|c| **c == '\n')
                .count() as i64
                + 1;
            JsonError { line, message: msg.to_string() }
        }

        fn ws(&mut self) {
            while self.pos < self.chars.len() && self.chars[self.pos].is_whitespace() {
                self.pos += 1;
            }
        }

        fn value(&mut self) -> Result<Json, JsonError> {
            self.ws();
            match self.peek() {
                Some('n') => self.word("null", Json::Null),
                Some('t') => self.word("true", Json::Boolean(true)),
                Some('f') => self.word("false", Json::Boolean(false)),
                Some('"') => Ok(Json::Text(self.string()?)),
                Some('[') => self.array(),
                Some('{') => self.object(),
                Some('-') | Some('0'..='9') => self.number(),
                _ => Err(self.err("expected a JSON value")),
            }
        }

        fn peek(&self) -> Option<char> { self.chars.get(self.pos).copied() }

        fn word(&mut self, w: &str, v: Json) -> Result<Json, JsonError> {
            for ch in w.chars() {
                if self.peek() != Some(ch) {
                    return Err(self.err("expected a JSON word"));
                }
                self.pos += 1;
            }
            Ok(v)
        }

        fn string(&mut self) -> Result<String, JsonError> {
            if self.peek() != Some('"') {
                return Err(self.err("expected quoted text"));
            }
            self.pos += 1;
            let mut out = String::new();
            while let Some(c) = self.peek() {
                self.pos += 1;
                match c {
                    '"' => return Ok(out),
                    '\\' => {
                        let Some(e) = self.peek() else { return Err(self.err("unfinished escape")); };
                        self.pos += 1;
                        match e {
                            '"' => out.push('"'),
                            '\\' => out.push('\\'),
                            'n' => out.push('\n'),
                            't' => out.push('\t'),
                            other => out.push(other),
                        }
                    }
                    other => out.push(other),
                }
            }
            Err(self.err("missing closing quote"))
        }

        fn number(&mut self) -> Result<Json, JsonError> {
            let start = self.pos;
            if self.peek() == Some('-') {
                self.pos += 1;
            }
            while matches!(self.peek(), Some('0'..='9')) {
                self.pos += 1;
            }
            if self.peek() == Some('.') {
                self.pos += 1;
                while matches!(self.peek(), Some('0'..='9')) {
                    self.pos += 1;
                }
            }
            let s: String = self.chars[start..self.pos].iter().collect();
            match s.parse::<f64>() {
                Ok(n) => Ok(Json::Number(n)),
                Err(_) => Err(self.err("bad number")),
            }
        }

        fn array(&mut self) -> Result<Json, JsonError> {
            self.pos += 1;
            let mut out = Vec::new();
            loop {
                self.ws();
                if self.peek() == Some(']') {
                    self.pos += 1;
                    return Ok(Json::Array(out));
                }
                out.push(self.value()?);
                self.ws();
                match self.peek() {
                    Some(',') => self.pos += 1,
                    Some(']') => {}
                    _ => return Err(self.err("expected `,` or `]`")),
                }
            }
        }

        fn object(&mut self) -> Result<Json, JsonError> {
            self.pos += 1;
            let mut out = std::collections::BTreeMap::new();
            loop {
                self.ws();
                if self.peek() == Some('}') {
                    self.pos += 1;
                    return Ok(Json::Object(out));
                }
                let key = self.string()?;
                self.ws();
                if self.peek() != Some(':') {
                    return Err(self.err("expected `:` after object key"));
                }
                self.pos += 1;
                let value = self.value()?;
                out.insert(key, value);
                self.ws();
                match self.peek() {
                    Some(',') => self.pos += 1,
                    Some('}') => {}
                    _ => return Err(self.err("expected `,` or `}`")),
                }
            }
        }
    }
}

// ── Streaming file handles (E2-M7, D-IO2) ────────────────────────────────────
// FileReader / FileWriter are RAII: Drop closes (and flushes) them
// on every exit path — including `?` early returns and panics.
struct JetFileReader {
    inner: std::io::BufReader<std::fs::File>,
    path: String,
}
struct JetFileWriter {
    inner: std::io::BufWriter<std::fs::File>,
    path: String,
}

fn jet_std_files_open(path: &String) -> Result<JetFileReader, jet_std::IoError> {
    let f = std::fs::File::open(path).map_err(|e| jet_std::io_error(path, e))?;
    Ok(JetFileReader { inner: std::io::BufReader::new(f), path: path.clone() })
}
fn jet_std_files_create(path: &String) -> Result<JetFileWriter, jet_std::IoError> {
    let f = std::fs::File::create(path).map_err(|e| jet_std::io_error(path, e))?;
    Ok(JetFileWriter { inner: std::io::BufWriter::new(f), path: path.clone() })
}
fn jet_std_files_append(path: &String) -> Result<JetFileWriter, jet_std::IoError> {
    let f = std::fs::OpenOptions::new()
        .create(true).append(true).open(path)
        .map_err(|e| jet_std::io_error(path, e))?;
    Ok(JetFileWriter { inner: std::io::BufWriter::new(f), path: path.clone() })
}
fn jet_std_file_reader_read_line(r: &mut JetFileReader) -> Result<Option<String>, jet_std::IoError> {
    use std::io::BufRead;
    let mut line = String::new();
    match r.inner.read_line(&mut line) {
        Ok(0) => Ok(None),
        Ok(_) => {
            while line.ends_with('\n') || line.ends_with('\r') { line.pop(); }
            Ok(Some(line))
        }
        Err(e) => Err(jet_std::io_error(&r.path, e)),
    }
}
fn jet_std_file_writer_write_line(w: &mut JetFileWriter, line: &String) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    w.inner.write_all(line.as_bytes()).and_then(|_| w.inner.write_all(b"\n"))
        .map_err(|e| jet_std::io_error(&w.path, e))
}
fn jet_std_file_writer_flush(w: &mut JetFileWriter) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    w.inner.flush().map_err(|e| jet_std::io_error(&w.path, e))
}

// ── std.path helpers (D-IO1) ──────────────────────────────────────────────────
fn jet_std_path_join(base: &String, part: &String) -> String {
    let b = std::path::Path::new(base.as_str());
    b.join(part.as_str()).to_string_lossy().to_string()
}
fn jet_std_path_parent(path: &String) -> String {
    std::path::Path::new(path.as_str())
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}
fn jet_std_path_extension(path: &String) -> String {
    std::path::Path::new(path.as_str())
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default()
}
fn jet_std_path_normalize(path: &String) -> String {
    // Resolve `.` and `..` components without hitting the filesystem.
    let mut parts: Vec<&str> = Vec::new();
    let s = path.as_str();
    let absolute = s.starts_with('/');
    for seg in s.split('/') {
        match seg {
            "" | "." => {}
            ".." => { parts.pop(); }
            other => parts.push(other),
        }
    }
    let joined = parts.join("/");
    if absolute { format!("/{}", joined) } else { joined }
}

fn jet_std_fs_read(path: &String) -> Result<String, jet_std::IoError> {
    std::fs::read_to_string(path).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_read_bytes(path: &String) -> Result<Vec<u8>, jet_std::IoError> {
    std::fs::read(path).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_write(path: &String, text: &String) -> Result<(), jet_std::IoError> {
    std::fs::write(path, text).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_append(path: &String, text: &String) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| jet_std::io_error(path, e))?;
    f.write_all(text.as_bytes()).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_exists(path: &String) -> bool { std::path::Path::new(path).exists() }
fn jet_std_fs_remove(path: &String) -> Result<(), jet_std::IoError> {
    std::fs::remove_file(path).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_list_dir(path: &String) -> Result<Vec<String>, jet_std::IoError> {
    let mut out = Vec::new();
    let rd = std::fs::read_dir(path).map_err(|e| jet_std::io_error(path, e))?;
    for entry in rd {
        let entry = entry.map_err(|e| jet_std::io_error(path, e))?;
        out.push(entry.file_name().to_string_lossy().to_string());
    }
    out.sort();
    Ok(out)
}
fn jet_std_fs_create_dir(path: &String) -> Result<(), jet_std::IoError> {
    std::fs::create_dir_all(path).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_is_dir(path: &String) -> bool { std::path::Path::new(path).is_dir() }
fn jet_std_fs_copy(from: &String, to: &String) -> Result<(), jet_std::IoError> {
    std::fs::copy(from, to).map(|_| ()).map_err(|e| jet_std::io_error(from, e))
}
fn jet_std_fs_rename(from: &String, to: &String) -> Result<(), jet_std::IoError> {
    std::fs::rename(from, to).map_err(|e| jet_std::io_error(from, e))
}

fn jet_std_io_args() -> Vec<String> { std::env::args().collect() }
fn jet_std_io_input(prompt: Option<&String>) -> Result<String, jet_std::IoError> {
    use std::io::Write;
    if let Some(p) = prompt {
        print!("{}", p);
        std::io::stdout().flush().map_err(|e| jet_std::IoError::Other { message: e.to_string() })?;
    }
    let mut s = String::new();
    std::io::stdin()
        .read_line(&mut s)
        .map_err(|e| jet_std::IoError::Other { message: e.to_string() })?;
    while s.ends_with('\n') || s.ends_with('\r') {
        s.pop();
    }
    Ok(s)
}
fn jet_std_io_read_all_input() -> Result<String, jet_std::IoError> {
    use std::io::Read;
    let mut s = String::new();
    std::io::stdin()
        .read_to_string(&mut s)
        .map_err(|e| jet_std::IoError::Other { message: e.to_string() })?;
    Ok(s)
}

fn jet_std_env_get(name: &String) -> Option<String> { std::env::var(name).ok() }
fn jet_std_env_set(name: &String, value: &String) { std::env::set_var(name, value); }
fn jet_std_env_current_dir() -> Result<String, jet_std::IoError> {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| jet_std::IoError::Other { message: e.to_string() })
}
fn jet_std_env_home_dir() -> Option<String> {
    std::env::var("HOME").ok().or_else(|| std::env::var("USERPROFILE").ok())
}

fn jet_std_process_exit(code: i64) -> ! { std::process::exit(code as i32) }
fn jet_std_process_run(cmd: &Vec<String>) -> Result<jet_std::ProcessResult, jet_std::IoError> {
    if cmd.is_empty() {
        return Err(jet_std::IoError::Other { message: "process.run needs at least one command word".to_string() });
    }
    let out = std::process::Command::new(&cmd[0])
        .args(&cmd[1..])
        .output()
        .map_err(|e| jet_std::IoError::Other { message: e.to_string() })?;
    Ok(jet_std::ProcessResult {
        code: out.status.code().unwrap_or(-1) as i64,
        output: String::from_utf8_lossy(&out.stdout).to_string(),
        errors: String::from_utf8_lossy(&out.stderr).to_string(),
    })
}

fn jet_std_math_sqrt(x: f64) -> f64 { x.sqrt() }
fn jet_std_math_pow(a: f64, b: f64) -> f64 { a.powf(b) }
fn jet_std_math_floor(x: f64) -> f64 { x.floor() }
fn jet_std_math_ceil(x: f64) -> f64 { x.ceil() }
fn jet_std_math_round(x: f64) -> i64 { x.round() as i64 }

thread_local! { static JET_RNG: std::cell::Cell<u64> = std::cell::Cell::new(0x4d595df4d0f33173); }
fn jet_rng_next() -> u64 {
    JET_RNG.with(|cell| {
        let mut x = cell.get();
        x ^= x << 7;
        x ^= x >> 9;
        x = x.wrapping_mul(0x9e3779b97f4a7c15);
        cell.set(x);
        x
    })
}
fn jet_std_random_seed(n: i64) { JET_RNG.with(|cell| cell.set(n as u64)); }
fn jet_std_random_int(low: i64, high: i64) -> i64 {
    if high <= low {
        return low;
    }
    low + (jet_rng_next() % ((high - low + 1) as u64)) as i64
}
fn jet_std_random_float() -> f64 { (jet_rng_next() as f64) / (u64::MAX as f64) }
fn jet_std_random_pick<T: Clone>(xs: &Vec<T>) -> Option<T> {
    if xs.is_empty() { None } else { Some(xs[jet_std_random_int(0, xs.len() as i64 - 1) as usize].clone()) }
}
fn jet_std_random_shuffle<T>(xs: &mut Vec<T>) {
    let len = xs.len();
    for i in (1..len).rev() {
        let j = jet_std_random_int(0, i as i64) as usize;
        xs.swap(i, j);
    }
}

fn jet_std_time_now() -> i64 {
    if let Ok(s) = std::env::var("LEX_TEST_EPOCH") {
        if let Ok(n) = s.parse::<i64>() {
            return n;
        }
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
fn jet_std_time_sleep(millis: i64) {
    std::thread::sleep(std::time::Duration::from_millis(millis.max(0) as u64));
}
fn jet_std_time_start() -> jet_std::Stopwatch {
    jet_std::Stopwatch { start: std::time::Instant::now() }
}

fn jet_std_json_parse(text: &String) -> Result<jet_std::Json, jet_std::JsonError> {
    jet_std::parse_json(text)
}
fn jet_std_json_render(j: &jet_std::Json) -> String { jet_std::render_json(j, false, 0) }
fn jet_std_json_render_pretty(j: &jet_std::Json) -> String { jet_std::render_json(j, true, 0) }

fn jet_string_bytes(s: &String) -> Vec<u8> { s.as_bytes().to_vec() }
fn jet_string_from_bytes(bs: &Vec<u8>) -> Result<String, jet_std::Utf8Error> {
    String::from_utf8(bs.clone()).map_err(|e| jet_std::Utf8Error { message: e.to_string() })
}
fn jet_int_to_u8(n: i64) -> Result<u8, String> {
    if (0..=255).contains(&n) { Ok(n as u8) } else { Err("a U8 holds 0..255".to_string()) }
}
fn jet_stopwatch_elapsed_millis(sw: &jet_std::Stopwatch) -> i64 {
    sw.start.elapsed().as_millis() as i64
}

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
                if c == ',' { break; }
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

// ── jet.toml ──────────────────────────────────────────────────────────────────
// Simplified: parses flat `key = "value"` and `key = 123` TOML; skips sections
// and tables. Returns String values for all scalar types.
fn jet_ring_toml_parse(text: &String) -> Result<std::collections::BTreeMap<String, String>, String> {
    let mut map = std::collections::BTreeMap::new();
    for (line_no, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
            continue;
        }
        let Some(eq) = line.find('=') else {
            return Err(format!("E2701: TOML line {} — expected `key = value`", line_no + 1));
        };
        let key = line[..eq].trim().to_string();
        let val_raw = line[eq + 1..].trim();
        let val = if val_raw.starts_with('"') && val_raw.ends_with('"') && val_raw.len() >= 2 {
            val_raw[1..val_raw.len() - 1].replace("\\n", "\n").replace("\\t", "\t").replace("\\\"", "\"")
        } else {
            val_raw.to_string()
        };
        map.insert(key, val);
    }
    Ok(map)
}

fn jet_ring_toml_render(data: &std::collections::BTreeMap<String, String>) -> String {
    data.iter()
        .map(|(k, v)| {
            if v.parse::<f64>().is_ok() || v == "true" || v == "false" {
                format!("{} = {}", k, v)
            } else {
                format!("{} = \"{}\"", k, v.replace('"', "\\\""))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ── jet.yaml ──────────────────────────────────────────────────────────────────
// Simplified: parses flat `key: value` YAML (no nesting, no lists).
fn jet_ring_yaml_parse(text: &String) -> Result<std::collections::BTreeMap<String, String>, String> {
    let mut map = std::collections::BTreeMap::new();
    for (line_no, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(colon) = line.find(':') else {
            return Err(format!("E2701: YAML line {} — expected `key: value`", line_no + 1));
        };
        let key = line[..colon].trim().to_string();
        let val = line[colon + 1..].trim();
        let val = if (val.starts_with('"') && val.ends_with('"'))
            || (val.starts_with('\'') && val.ends_with('\''))
        {
            val[1..val.len() - 1].to_string()
        } else {
            val.to_string()
        };
        map.insert(key, val);
    }
    Ok(map)
}

fn jet_ring_yaml_render(data: &std::collections::BTreeMap<String, String>) -> String {
    data.iter()
        .map(|(k, v)| {
            if v.parse::<f64>().is_ok() || v == "true" || v == "false" {
                format!("{}: {}", k, v)
            } else {
                format!("{}: \"{}\"", k, v.replace('"', "\\\""))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ── jet.log ───────────────────────────────────────────────────────────────────
// Log level: 0=debug, 1=info, 2=warn, 3=error. Default is info (1).
thread_local! {
    static JET_LOG_LEVEL: std::cell::Cell<u8> = std::cell::Cell::new(1);
}

fn jet_ring_log_set_level(level: &String) {
    let n: u8 = match level.as_str() {
        "debug" => 0, "info" => 1, "warn" => 2, "error" => 3, _ => 1,
    };
    JET_LOG_LEVEL.with(|l| l.set(n));
}

fn jet_ring_log_debug(msg: &String) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 0 {
        eprintln!("[DEBUG] {}", msg);
    }
}
fn jet_ring_log_info(msg: &String) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 1 {
        eprintln!("[INFO] {}", msg);
    }
}
fn jet_ring_log_warn(msg: &String) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 2 {
        eprintln!("[WARN] {}", msg);
    }
}
fn jet_ring_log_error(msg: &String) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 3 {
        eprintln!("[ERROR] {}", msg);
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
        if days < dy { break; }
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
        if days < md { break; }
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

// Minimal SHA-256 (same algorithm as src/sha256.rs — duplicated here so the
// prelude doesn't need to reach into the compiler crate; I6 forbids extern deps).
fn jet_sha256_raw(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    const H0: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let mut state = H0;
    let bit_len = (data.len() as u64) * 8;
    let mut msg: Vec<u8> = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 { msg.push(0); }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 { w[i] = u32::from_be_bytes(block[i*4..i*4+4].try_into().unwrap()); }
        for i in 16..64 {
            let s0 = w[i-15].rotate_right(7) ^ w[i-15].rotate_right(18) ^ (w[i-15] >> 3);
            let s1 = w[i-2].rotate_right(17) ^ w[i-2].rotate_right(19) ^ (w[i-2] >> 10);
            w[i] = w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] =
            [state[0], state[1], state[2], state[3], state[4], state[5], state[6], state[7]];
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let temp1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            h = g; g = f; f = e; e = d.wrapping_add(temp1);
            d = c; c = b; b = a; a = temp1.wrapping_add(temp2);
        }
        state[0] = state[0].wrapping_add(a); state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c); state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e); state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g); state[7] = state[7].wrapping_add(h);
    }
    let mut out = [0u8; 32];
    for (i, &s) in state.iter().enumerate() {
        out[i*4..i*4+4].copy_from_slice(&s.to_be_bytes());
    }
    out
}
