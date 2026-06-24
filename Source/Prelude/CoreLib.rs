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

    #[derive(Clone, Debug, PartialEq)]
    pub struct DirEntry {
        pub name: String,
        pub path: String,
        pub is_dir: bool,
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
    impl super::JetShow for DirEntry {
        fn jet_show(&self) -> String {
            format!("DirEntry {{ name: {:?}, path: {:?}, is_dir: {} }}", self.name, self.path, self.is_dir)
        }
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

    // ── core.encoding: format-agnostic value tree (D-SERDE2 = A) ───────────────
    // The one tree every format adapter speaks. The built-in `#[Codable]` derive
    // (D-ENC1) lowers `encode`/`decode` to walks over this; each adapter turns it
    // into / parses it from wire text. Distinct from the dynamic `Json` enum:
    // `DataTree` preserves field order (ordered `Object`) and keeps Int vs Float.
    #[derive(Clone, Debug, PartialEq)]
    pub enum DataTree {
        Null,
        Bool(bool),
        Int(i64),
        Float(f64),
        Text(String),
        Bytes(Vec<u8>),
        Array(Vec<DataTree>),
        Object(Vec<(String, DataTree)>),
    }

    // D-SERDE2 = A: the decode-side error carries a field path (`order.items[2]`)
    // and a plain reason. Encode is infallible, so no `EncodeError` is minted (I8).
    #[derive(Clone, Debug, PartialEq)]
    pub struct DecodeError {
        pub path: String,
        pub reason: String,
    }

    impl DecodeError {
        pub fn new(reason: impl Into<String>) -> DecodeError {
            DecodeError { path: String::new(), reason: reason.into() }
        }
        // Prefix a child error with the field/index segment it occurred under.
        pub fn under(seg: &str, mut e: DecodeError) -> DecodeError {
            e.path = if e.path.is_empty() {
                seg.to_string()
            } else if e.path.starts_with('[') {
                format!("{}{}", seg, e.path)
            } else {
                format!("{}.{}", seg, e.path)
            };
            e
        }
    }

    impl super::JetShow for DataTree {
        fn jet_show(&self) -> String { render_datatree_json(self, false, 0) }
    }
    impl super::JetShow for DecodeError {
        fn jet_show(&self) -> String {
            if self.path.is_empty() {
                self.reason.clone()
            } else {
                format!("at `{}`: {}", self.path, self.reason)
            }
        }
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
                    eprintln!("panic: a task panicked");
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

    // Render a DataTree as JSON, preserving Object field order. Int prints with no
    // decimal (`5`), Float keeps its decimal (`5.0`); Bytes render as a number array.
    pub fn render_datatree_json(t: &DataTree, pretty: bool, depth: usize) -> String {
        match t {
            DataTree::Null => "null".to_string(),
            DataTree::Bool(b) => b.to_string(),
            DataTree::Int(n) => format!("{}", n),
            DataTree::Float(f) => format!("{:?}", f),
            DataTree::Text(s) => quote_json(s),
            DataTree::Bytes(bs) => {
                let parts: Vec<String> = bs.iter().map(|b| b.to_string()).collect();
                format!("[{}]", parts.join(","))
            }
            DataTree::Array(items) => {
                if items.is_empty() {
                    return "[]".to_string();
                }
                if !pretty {
                    let parts: Vec<String> =
                        items.iter().map(|x| render_datatree_json(x, false, depth)).collect();
                    return format!("[{}]", parts.join(","));
                }
                let pad = "  ".repeat(depth + 1);
                let end = "  ".repeat(depth);
                let parts: Vec<String> = items
                    .iter()
                    .map(|x| format!("{}{}", pad, render_datatree_json(x, true, depth + 1)))
                    .collect();
                format!("[\n{}\n{}]", parts.join(",\n"), end)
            }
            DataTree::Object(entries) => {
                if entries.is_empty() {
                    return "{}".to_string();
                }
                if !pretty {
                    let parts: Vec<String> = entries
                        .iter()
                        .map(|(k, v)| format!("{}:{}", quote_json(k), render_datatree_json(v, false, depth)))
                        .collect();
                    return format!("{{{}}}", parts.join(","));
                }
                let pad = "  ".repeat(depth + 1);
                let end = "  ".repeat(depth);
                let parts: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| format!("{}{}: {}", pad, quote_json(k), render_datatree_json(v, true, depth + 1)))
                    .collect();
                format!("{{\n{}\n{}}}", parts.join(",\n"), end)
            }
        }
    }

    // Json (dynamic, BTreeMap-keyed) → DataTree. Numbers that are integral collapse
    // to `Int`, so a round-trip through JSON keeps `5` an Int.
    pub fn datatree_from_json(j: &Json) -> DataTree {
        match j {
            Json::Null => DataTree::Null,
            Json::Boolean(b) => DataTree::Bool(*b),
            Json::Number(n) => {
                if n.fract() == 0.0 && n.is_finite() && *n >= i64::MIN as f64 && *n <= i64::MAX as f64 {
                    DataTree::Int(*n as i64)
                } else {
                    DataTree::Float(*n)
                }
            }
            Json::Text(s) => DataTree::Text(s.clone()),
            Json::Array(items) => DataTree::Array(items.iter().map(datatree_from_json).collect()),
            Json::Object(m) => {
                DataTree::Object(m.iter().map(|(k, v)| (k.clone(), datatree_from_json(v))).collect())
            }
        }
    }

    // A short kind name for decode error messages.
    pub fn datatree_kind(t: &DataTree) -> &'static str {
        match t {
            DataTree::Null => "null",
            DataTree::Bool(_) => "Bool",
            DataTree::Int(_) => "Int",
            DataTree::Float(_) => "Float",
            DataTree::Text(_) => "Text",
            DataTree::Bytes(_) => "Bytes",
            DataTree::Array(_) => "a list",
            DataTree::Object(_) => "an object",
        }
    }

    // Look up a key in an ordered Object.
    pub fn datatree_get<'a>(t: &'a DataTree, key: &str) -> Option<&'a DataTree> {
        match t {
            DataTree::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
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
// D-LSDIR1=A: returns DirEntry values with name, full path, and is_dir flag.
fn jet_std_fs_list_dir(path: &String) -> Result<Vec<jet_std::DirEntry>, jet_std::IoError> {
    let mut out = Vec::new();
    let rd = std::fs::read_dir(path).map_err(|e| jet_std::io_error(path, e))?;
    for entry in rd {
        let entry = entry.map_err(|e| jet_std::io_error(path, e))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let full_path = std::path::Path::new(path.as_str())
            .join(&name)
            .to_string_lossy()
            .to_string();
        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        out.push(jet_std::DirEntry { name, path: full_path, is_dir });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
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

// D-STDIN1=A: streaming line-by-line stdin.
struct JetStdinReader {
    inner: std::io::BufReader<std::io::Stdin>,
}
fn jet_std_io_stdin() -> JetStdinReader {
    JetStdinReader { inner: std::io::BufReader::new(std::io::stdin()) }
}
fn jet_std_io_stdin_read_line(r: &mut JetStdinReader) -> Result<Option<String>, jet_std::IoError> {
    use std::io::BufRead;
    let mut line = String::new();
    match r.inner.read_line(&mut line) {
        Ok(0) => Ok(None),
        Ok(_) => {
            while line.ends_with('\n') || line.ends_with('\r') { line.pop(); }
            Ok(Some(line))
        }
        Err(e) => Err(jet_std::IoError::Other { message: e.to_string() }),
    }
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
// D-FLOATW1 (ratified 2026-06-22): F32 variants — sqrt(F32)->F32, pow(F32,F32)->F32 etc.
// F32 is a real precision choice, not just storage; no silent widening to f64 (I3).
fn jet_std_math_sqrt_f32(x: f32) -> f32 { x.sqrt() }
fn jet_std_math_pow_f32(a: f32, b: f32) -> f32 { a.powf(b) }
fn jet_std_math_floor_f32(x: f32) -> f32 { x.floor() }
fn jet_std_math_ceil_f32(x: f32) -> f32 { x.ceil() }

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

// D-JSON1-decode + D-JSON3: lenient JSON decode with coercion surfacing.
// Parses `text`, then walks the result. Any JSON string that looks like a
// number or boolean is coerced to that type; one log line is emitted per
// coercion naming the field and the from→to types.
fn jet_std_json_decode_lenient(text: &String) -> Result<jet_std::Json, jet_std::JsonError> {
    let parsed = jet_std::parse_json(text)?;
    Ok(jet_std_json_coerce_walk(&parsed, ""))
}

fn jet_std_json_coerce_walk(value: &jet_std::Json, path: &str) -> jet_std::Json {
    match value {
        jet_std::Json::Text(s) => {
            // try bool first (exact match only)
            if s == "true" {
                jet_std_json_emit_coerce(path, "string", "boolean");
                return jet_std::Json::Boolean(true);
            }
            if s == "false" {
                jet_std_json_emit_coerce(path, "string", "boolean");
                return jet_std::Json::Boolean(false);
            }
            // try number (must parse as valid f64 and round-trip cleanly)
            if let Ok(n) = s.parse::<f64>() {
                if n.is_finite() {
                    jet_std_json_emit_coerce(path, "string", "number");
                    return jet_std::Json::Number(n);
                }
            }
            value.clone()
        }
        jet_std::Json::Object(entries) => {
            let mut out = std::collections::BTreeMap::new();
            for (k, v) in entries {
                let child_path = if path.is_empty() {
                    format!("{}", k)
                } else {
                    format!("{}.{}", path, k)
                };
                out.insert(k.clone(), jet_std_json_coerce_walk(v, &child_path));
            }
            jet_std::Json::Object(out)
        }
        jet_std::Json::Array(items) => {
            let coerced: Vec<jet_std::Json> = items.iter().enumerate().map(|(i, v)| {
                let child_path = if path.is_empty() {
                    format!("[{}]", i)
                } else {
                    format!("{}[{}]", path, i)
                };
                jet_std_json_coerce_walk(v, &child_path)
            }).collect();
            jet_std::Json::Array(coerced)
        }
        // Null, Boolean, Number — already the right type, no coercion.
        other => other.clone(),
    }
}

fn jet_std_json_emit_coerce(path: &str, from: &str, to: &str) {
    let field_label = if path.is_empty() { "<root>" } else { path };
    let msg = format!("json coerce: field \"{}\" {} \u{2192} {}", field_label, from, to);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    eprintln!("{{\"level\":\"info\",\"body\":\"{}\",\"ts\":{}}}", msg, ts);
}

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

// ── core.encoding: Encode / Decode traits + blanket impls (D-SERDE1/2/4) ──────
// The built-in `#[Codable]`/`#[Encode]`/`#[Decode]` derive (D-ENC1) lowers to
// these traits. `jet_encode`/`jet_decode` are codegen-internal method names the
// user never types (they write the verbs `encode`/`decode` only in a hand-impl,
// D-SERDE2 — a later increment). Pure safe std Rust, no proc-macros (I1/I6).
#[allow(non_camel_case_types)]
pub trait user_Encode {
    fn jet_encode(&self) -> jet_std::DataTree;
}
#[allow(non_camel_case_types)]
pub trait user_Decode: Sized {
    fn jet_decode(tree: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError>;
}

impl user_Encode for i64 {
    fn jet_encode(&self) -> jet_std::DataTree { jet_std::DataTree::Int(*self) }
}
impl user_Encode for f64 {
    fn jet_encode(&self) -> jet_std::DataTree { jet_std::DataTree::Float(*self) }
}
impl user_Encode for bool {
    fn jet_encode(&self) -> jet_std::DataTree { jet_std::DataTree::Bool(*self) }
}
impl user_Encode for String {
    fn jet_encode(&self) -> jet_std::DataTree { jet_std::DataTree::Text(self.clone()) }
}
impl user_Encode for char {
    fn jet_encode(&self) -> jet_std::DataTree { jet_std::DataTree::Text(self.to_string()) }
}
impl<T: user_Encode> user_Encode for Vec<T> {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Array(self.iter().map(|x| x.jet_encode()).collect())
    }
}
impl<T: user_Encode> user_Encode for Option<T> {
    fn jet_encode(&self) -> jet_std::DataTree {
        match self {
            Some(x) => x.jet_encode(),
            None => jet_std::DataTree::Null,
        }
    }
}
impl<V: user_Encode> user_Encode for std::collections::BTreeMap<String, V> {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Object(self.iter().map(|(k, v)| (k.clone(), v.jet_encode())).collect())
    }
}

impl user_Decode for i64 {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        match t {
            jet_std::DataTree::Int(n) => Ok(*n),
            jet_std::DataTree::Float(f) if f.fract() == 0.0 => Ok(*f as i64),
            jet_std::DataTree::Text(s) => s.trim().parse::<i64>().map_err(|_| {
                jet_std::DecodeError::new(format!("expected Int, found text {:?}", s))
            }),
            other => Err(jet_std::DecodeError::new(format!(
                "expected Int, found {}", jet_std::datatree_kind(other)
            ))),
        }
    }
}
impl user_Decode for f64 {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        match t {
            jet_std::DataTree::Float(f) => Ok(*f),
            jet_std::DataTree::Int(n) => Ok(*n as f64),
            jet_std::DataTree::Text(s) => s.trim().parse::<f64>().map_err(|_| {
                jet_std::DecodeError::new(format!("expected Float, found text {:?}", s))
            }),
            other => Err(jet_std::DecodeError::new(format!(
                "expected Float, found {}", jet_std::datatree_kind(other)
            ))),
        }
    }
}
impl user_Decode for bool {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        match t {
            jet_std::DataTree::Bool(b) => Ok(*b),
            jet_std::DataTree::Text(s) => match s.trim() {
                "true" => Ok(true),
                "false" => Ok(false),
                _ => Err(jet_std::DecodeError::new(format!("expected Bool, found text {:?}", s))),
            },
            other => Err(jet_std::DecodeError::new(format!(
                "expected Bool, found {}", jet_std::datatree_kind(other)
            ))),
        }
    }
}
impl user_Decode for String {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        match t {
            jet_std::DataTree::Text(s) => Ok(s.clone()),
            jet_std::DataTree::Int(n) => Ok(n.to_string()),
            jet_std::DataTree::Float(f) => Ok(format!("{:?}", f)),
            jet_std::DataTree::Bool(b) => Ok(b.to_string()),
            other => Err(jet_std::DecodeError::new(format!(
                "expected Text, found {}", jet_std::datatree_kind(other)
            ))),
        }
    }
}
impl user_Decode for char {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        let s = String::jet_decode(t)?;
        let mut it = s.chars();
        match (it.next(), it.next()) {
            (Some(c), None) => Ok(c),
            _ => Err(jet_std::DecodeError::new(format!("expected a single Char, found {:?}", s))),
        }
    }
}
impl<T: user_Decode> user_Decode for Vec<T> {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        match t {
            jet_std::DataTree::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for (i, item) in items.iter().enumerate() {
                    out.push(T::jet_decode(item).map_err(|e| jet_std::DecodeError::under(&format!("[{}]", i), e))?);
                }
                Ok(out)
            }
            other => Err(jet_std::DecodeError::new(format!(
                "expected a list, found {}", jet_std::datatree_kind(other)
            ))),
        }
    }
}
impl<T: user_Decode> user_Decode for Option<T> {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        match t {
            jet_std::DataTree::Null => Ok(None),
            other => Ok(Some(T::jet_decode(other)?)),
        }
    }
}
impl<V: user_Decode> user_Decode for std::collections::BTreeMap<String, V> {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, jet_std::DecodeError> {
        match t {
            jet_std::DataTree::Object(entries) => {
                let mut out = std::collections::BTreeMap::new();
                for (k, v) in entries {
                    out.insert(k.clone(), V::jet_decode(v).map_err(|e| jet_std::DecodeError::under(k, e))?);
                }
                Ok(out)
            }
            other => Err(jet_std::DecodeError::new(format!(
                "expected an object, found {}", jet_std::datatree_kind(other)
            ))),
        }
    }
}

// ── core.encoding: typed format verbs over Encode/Decode (D-ENC1, D-SERDE6) ────
// `to_string`/`to_string_pretty` (D-JSONVERB1) and the typed `decode<T>` route
// every format through the one DataTree model.
fn jet_enc_json_to_string<T: user_Encode>(v: &T) -> String {
    jet_std::render_datatree_json(&v.jet_encode(), false, 0)
}
fn jet_enc_json_to_string_pretty<T: user_Encode>(v: &T) -> String {
    jet_std::render_datatree_json(&v.jet_encode(), true, 0)
}
fn jet_enc_json_decode<T: user_Decode>(text: &String) -> Result<T, jet_std::DecodeError> {
    let j = jet_std::parse_json(text)
        .map_err(|e| jet_std::DecodeError::new(format!("invalid JSON (line {}): {}", e.line, e.message)))?;
    T::jet_decode(&jet_std::datatree_from_json(&j))
}

// CSV typed decode: header row maps columns to fields by name; each data row
// becomes a DataTree::Object of Text cells, then decodes to `T`. A short row or a
// per-row decode failure is a typed `DecodeError` naming the 1-based row.
fn jet_enc_csv_decode<T: user_Decode>(text: &String) -> Result<Vec<T>, jet_std::DecodeError> {
    let rows = jet_ring_csv_parse(text).map_err(jet_std::DecodeError::new)?;
    let mut it = rows.into_iter();
    let Some(header) = it.next() else { return Ok(Vec::new()); };
    let mut out = Vec::new();
    for (i, row) in it.enumerate() {
        let obj: Vec<(String, jet_std::DataTree)> = header
            .iter()
            .enumerate()
            .map(|(c, name)| {
                let cell = row.get(c).cloned().unwrap_or_default();
                (name.clone(), jet_std::DataTree::Text(cell))
            })
            .collect();
        let tree = jet_std::DataTree::Object(obj);
        out.push(T::jet_decode(&tree).map_err(|e| jet_std::DecodeError::under(&format!("row {}", i + 1), e))?);
    }
    Ok(out)
}

// CSV typed encode: `[T]` → header row (field names from the first row's Object)
// + one record per element. Requires every element to encode to a flat Object.
fn jet_enc_csv_to_string<T: user_Encode>(values: &Vec<T>) -> String {
    let trees: Vec<jet_std::DataTree> = values.iter().map(|v| v.jet_encode()).collect();
    let mut header: Vec<String> = Vec::new();
    if let Some(jet_std::DataTree::Object(entries)) = trees.first() {
        header = entries.iter().map(|(k, _)| k.clone()).collect();
    }
    let mut rows: Vec<Vec<String>> = Vec::new();
    rows.push(header.clone());
    for tree in &trees {
        let mut record = Vec::with_capacity(header.len());
        for key in &header {
            let cell = match jet_std::datatree_get(tree, key) {
                Some(jet_std::DataTree::Text(s)) => s.clone(),
                Some(jet_std::DataTree::Int(n)) => n.to_string(),
                Some(jet_std::DataTree::Float(f)) => format!("{:?}", f),
                Some(jet_std::DataTree::Bool(b)) => b.to_string(),
                Some(jet_std::DataTree::Null) | None => String::new(),
                Some(other) => jet_std::render_datatree_json(other, false, 0),
            };
            record.push(cell);
        }
        rows.push(record);
    }
    jet_ring_csv_render(&rows)
}

// TOML / YAML typed decode: parse flat scalars into a DataTree::Object of Text,
// then decode to `T`. TOML/YAML typed encode renders a flat Object as key/value.
fn jet_enc_toml_decode<T: user_Decode>(text: &String) -> Result<T, jet_std::DecodeError> {
    let map = jet_ring_toml_parse(text).map_err(jet_std::DecodeError::new)?;
    T::jet_decode(&jet_std::DataTree::Object(
        map.into_iter().map(|(k, v)| (k, jet_std::DataTree::Text(v))).collect(),
    ))
}
fn jet_enc_yaml_decode<T: user_Decode>(text: &String) -> Result<T, jet_std::DecodeError> {
    let map = jet_ring_yaml_parse(text).map_err(jet_std::DecodeError::new)?;
    T::jet_decode(&jet_std::DataTree::Object(
        map.into_iter().map(|(k, v)| (k, jet_std::DataTree::Text(v))).collect(),
    ))
}
fn jet_enc_flat_object_strings<T: user_Encode>(v: &T) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    if let jet_std::DataTree::Object(entries) = v.jet_encode() {
        for (k, val) in entries {
            let s = match val {
                jet_std::DataTree::Text(s) => s,
                jet_std::DataTree::Int(n) => n.to_string(),
                jet_std::DataTree::Float(f) => format!("{:?}", f),
                jet_std::DataTree::Bool(b) => b.to_string(),
                jet_std::DataTree::Null => String::new(),
                other => jet_std::render_datatree_json(&other, false, 0),
            };
            out.insert(k, s);
        }
    }
    out
}
fn jet_enc_toml_to_string<T: user_Encode>(v: &T) -> String {
    jet_ring_toml_render(&jet_enc_flat_object_strings(v))
}
fn jet_enc_yaml_to_string<T: user_Encode>(v: &T) -> String {
    jet_ring_yaml_render(&jet_enc_flat_object_strings(v))
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
}

fn jet_ring_log_set_level(level: &String) {
    let n: u8 = match level.as_str() {
        "debug" => 0, "info" => 1, "warn" => 2, "error" => 3, _ => 1,
    };
    JET_LOG_LEVEL.with(|l| l.set(n));
}

fn jet_ring_log_set_trace_id(id: &String) {
    JET_LOG_TRACE_ID.with(|t| *t.borrow_mut() = id.clone());
}

// D-LOGFMT1=A: explicit format override.
fn jet_ring_log_setup(format: &String) {
    let n: u8 = match format.as_str() { "json" => 1, "text" => 2, _ => 0 };
    JET_LOG_FORMAT.with(|f| f.set(n));
}

fn jet_log_format_active() -> u8 {
    let explicit = JET_LOG_FORMAT.with(|f| f.get());
    if explicit != 0 {
        return explicit;
    }
    // Auto-detect: text if stderr is a terminal, JSON otherwise.
    use std::io::IsTerminal;
    if std::io::stderr().is_terminal() { 2 } else { 1 }
}

fn jet_log_json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"'  => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c    => out.push(c),
        }
    }
    out
}

fn jet_log_emit_json(level: &str, msg: &str, ts: i64) {
    let trace = JET_LOG_TRACE_ID.with(|t| t.borrow().clone());
    if trace.is_empty() {
        eprintln!(
            "{{\"level\":\"{}\",\"body\":\"{}\",\"ts\":{}}}",
            level,
            jet_log_json_escape(msg),
            ts,
        );
    } else {
        eprintln!(
            "{{\"level\":\"{}\",\"body\":\"{}\",\"trace_id\":\"{}\",\"ts\":{}}}",
            level,
            jet_log_json_escape(msg),
            jet_log_json_escape(&trace),
            ts,
        );
    }
}

fn jet_log_emit_text(level: &str, msg: &str, ts: i64) {
    let secs = ts / 1000;
    let (y, mo, d, h, mi, s) = unix_to_ymdhms(secs);
    let level_tag = match level {
        "debug" => "DEBUG", "info" => "INFO", "warn" => "WARN", "error" => "ERROR", _ => level,
    };
    let trace = JET_LOG_TRACE_ID.with(|t| t.borrow().clone());
    if trace.is_empty() {
        eprintln!(
            "[{}] {:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z | {}",
            level_tag, y, mo, d, h, mi, s, msg
        );
    } else {
        eprintln!(
            "[{}] {:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z trace={} | {}",
            level_tag, y, mo, d, h, mi, s, trace, msg
        );
    }
}

fn jet_log_emit(level: &str, msg: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    if jet_log_format_active() == 2 {
        jet_log_emit_text(level, msg, ts);
    } else {
        jet_log_emit_json(level, msg, ts);
    }
}

fn jet_ring_log_debug(msg: &String) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 0 {
        jet_log_emit("debug", msg);
    }
}
fn jet_ring_log_info(msg: &String) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 1 {
        jet_log_emit("info", msg);
    }
}
fn jet_ring_log_warn(msg: &String) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 2 {
        jet_log_emit("warn", msg);
    }
}
fn jet_ring_log_error(msg: &String) {
    if JET_LOG_LEVEL.with(|l| l.get()) <= 3 {
        jet_log_emit("error", msg);
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

// ── E2-M10: networking (core.net + jet.http) ─────────────────────────────────
// All networking uses std::net only — zero external crates in the prelude (I6).
// TLS (D-NET1) is delivered as the `jet.tls` FFI package and is not included here.

pub struct JetTcpListener {
    inner: std::net::TcpListener,
}

pub struct JetTcpStream {
    inner: std::net::TcpStream,
}

#[derive(Clone)]
pub struct JetHttpRequest {
    pub method: String,
    pub path: String,
    pub body: String,
    pub headers: std::collections::BTreeMap<String, String>,
    pub params: std::collections::BTreeMap<String, String>,
}

#[derive(Clone)]
pub struct JetHttpResponse {
    pub status: String,
    pub body: String,
    pub headers: std::collections::BTreeMap<String, String>,
}

// D-ROUTE1=A: HTTP router — registration + :param dispatch.
#[derive(Clone)]
enum RouteSegment {
    Static(String),
    Param(String),
}

type JetHttpHandler = Box<dyn Fn(JetHttpRequest) -> JetHttpResponse + Send + Sync>;

struct JetHttpRoute {
    method: String,
    segments: Vec<RouteSegment>,
    handler: JetHttpHandler,
}

pub struct JetHttpRouter {
    routes: Vec<JetHttpRoute>,
}

impl JetShow for JetTcpListener {
    fn jet_show(&self) -> String { format!("TcpListener({})", self.inner.local_addr().map(|a| a.to_string()).unwrap_or_default()) }
}
impl JetShow for JetTcpStream {
    fn jet_show(&self) -> String { format!("TcpStream({})", self.inner.peer_addr().map(|a| a.to_string()).unwrap_or_default()) }
}
impl JetShow for JetHttpRequest {
    fn jet_show(&self) -> String { format!("{} {}", self.method, self.path) }
}
impl JetShow for JetHttpResponse {
    fn jet_show(&self) -> String { format!("HTTP {}", self.status) }
}
impl JetShow for JetHttpRouter {
    fn jet_show(&self) -> String { format!("HttpRouter({} routes)", self.routes.len()) }
}

fn jet_net_tcp_listen(addr: &String) -> Result<JetTcpListener, String> {
    std::net::TcpListener::bind(addr.as_str())
        .map(|l| JetTcpListener { inner: l })
        .map_err(|e| format!("bind on `{}` failed: {}", addr, e))
}

fn jet_net_tcp_accept(listener: &JetTcpListener) -> Result<JetTcpStream, String> {
    listener.inner.accept()
        .map(|(s, _)| JetTcpStream { inner: s })
        .map_err(|e| format!("accept failed: {}", e))
}

fn jet_net_tcp_connect(addr: &String) -> Result<JetTcpStream, String> {
    std::net::TcpStream::connect(addr.as_str())
        .map(|s| JetTcpStream { inner: s })
        .map_err(|e| format!("connect to `{}` failed: {}", addr, e))
}

fn jet_net_tcp_read(stream: &mut JetTcpStream) -> Result<String, String> {
    use std::io::Read;
    let mut buf = [0u8; 8192];
    match stream.inner.read(&mut buf) {
        Ok(0) => Ok(String::new()),
        Ok(n) => String::from_utf8(buf[..n].to_vec())
            .map_err(|e| format!("tcp read: invalid UTF-8: {}", e)),
        Err(e) => Err(format!("tcp read failed: {}", e)),
    }
}

fn jet_net_tcp_write(stream: &mut JetTcpStream, data: &String) -> Result<(), String> {
    use std::io::Write;
    stream.inner.write_all(data.as_bytes())
        .map_err(|e| format!("tcp write failed: {}", e))
}

fn jet_net_tcp_local_addr(stream: &JetTcpStream) -> String {
    stream.inner.local_addr().map(|a| a.to_string()).unwrap_or_default()
}

fn jet_net_tcp_peer_addr(stream: &JetTcpStream) -> String {
    stream.inner.peer_addr().map(|a| a.to_string()).unwrap_or_default()
}

fn jet_net_listener_local_addr(listener: &JetTcpListener) -> String {
    listener.inner.local_addr().map(|a| a.to_string()).unwrap_or_default()
}

fn jet_net_set_timeout(stream: &mut JetTcpStream, ms: i64) {
    let dur = std::time::Duration::from_millis(ms as u64);
    let _ = stream.inner.set_read_timeout(Some(dur));
    let _ = stream.inner.set_write_timeout(Some(dur));
}

/// Send a well-formed HTTP/1.1 response on a TcpStream and close it.
/// Handles CRLF line endings internally so Jet code doesn't need `\r`.
fn jet_net_tcp_reply(mut stream: JetTcpStream, status: &String, body: &String) {
    use std::io::Write;
    let response = format!(
        "HTTP/1.1 {}\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n{}",
        status, body.len(), body
    );
    let _ = stream.inner.write_all(response.as_bytes());
}

// ── HTTP/1.1 client (minimal, over std::net::TcpStream) ──────────────────────

fn jet_http_get(url: &String) -> Result<JetHttpResponse, String> {
    jet_http_request(url, "GET", &[], "")
}

fn jet_http_post(url: &String, body: &String) -> Result<JetHttpResponse, String> {
    jet_http_request(url, "POST", &[], body.as_str())
}

fn jet_http_request(url: &str, method: &str, extra_headers: &[(&str, &str)], body: &str) -> Result<JetHttpResponse, String> {
    use std::io::{Read, Write};
    // Parse URL: http://host[:port]/path
    let url_str = url;
    let (host_port, path) = if let Some(rest) = url_str.strip_prefix("http://") {
        let slash = rest.find('/').unwrap_or(rest.len());
        let hp = &rest[..slash];
        let p = if slash < rest.len() { &rest[slash..] } else { "/" };
        (hp.to_string(), p.to_string())
    } else if let Some(rest) = url_str.strip_prefix("https://") {
        return Err("HTTPS requires the `jet.tls` package; this is plain HTTP. Add `jet.tls` to your pkg.jet to enable HTTPS.".to_string());
        // Keep the variable to silence unused warning in case we extend later.
        #[allow(unreachable_code)]
        { (rest.to_string(), "/".to_string()) }
    } else {
        return Err(format!("URL must start with http:// — got `{}`", url));
    };
    // Default port 80 if not specified.
    let addr = if host_port.contains(':') {
        host_port.clone()
    } else {
        format!("{}:80", host_port)
    };
    let host = host_port.split(':').next().unwrap_or(&host_port);
    let mut stream = std::net::TcpStream::connect(&addr)
        .map_err(|e| format!("connect to `{}` failed: {}", addr, e))?;
    // Build HTTP/1.1 request.
    let content_len = body.len();
    let mut req = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Jet/1.0\r\nConnection: close\r\n",
        method, path, host
    );
    if !body.is_empty() {
        req.push_str(&format!("Content-Length: {}\r\n", content_len));
    }
    for (k, v) in extra_headers {
        req.push_str(&format!("{}: {}\r\n", k, v));
    }
    req.push_str("\r\n");
    if !body.is_empty() {
        req.push_str(body);
    }
    stream.write_all(req.as_bytes())
        .map_err(|e| format!("http write failed: {}", e))?;
    // Read response.
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)
        .map_err(|e| format!("http read failed: {}", e))?;
    let text = String::from_utf8_lossy(&raw).into_owned();
    // Parse status line + headers + body.
    let sep = text.find("\r\n\r\n").unwrap_or(text.len());
    let header_part = &text[..sep];
    let body_part = if sep + 4 <= text.len() { text[sep + 4..].to_string() } else { String::new() };
    let mut lines = header_part.lines();
    let status_line = lines.next().unwrap_or("HTTP/1.1 200 OK");
    let status = status_line.splitn(2, ' ').nth(1).unwrap_or("200 OK").to_string();
    let mut headers = std::collections::BTreeMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(": ") {
            headers.insert(k.to_lowercase(), v.to_string());
        }
    }
    Ok(JetHttpResponse { status, body: body_part, headers })
}

// ── HTTP/1.1 server (blocking, one thread per connection) ────────────────────
// note: `jet serve` uses one task per connection. This is excellent for internal
//       services and tools at hundreds of concurrent connections. For very high
//       connection counts, Jet is not the right tool yet — see docs/services.md.

fn jet_http_serve<F>(addr: &String, handler: F)
where
    F: Fn(JetHttpRequest) -> JetHttpResponse + Send + Sync + 'static,
{
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind(addr.as_str())
        .unwrap_or_else(|e| { eprintln!("E2801: bind on `{}` failed: {}", addr, e); std::process::exit(1); });
    let handler = std::sync::Arc::new(handler);
    loop {
        let (mut stream, _peer) = match listener.accept() {
            Ok(x) => x,
            Err(e) => { eprintln!("E2801: accept failed: {}", e); continue; }
        };
        let h = handler.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 16384];
            let n = stream.read(&mut buf).unwrap_or(0);
            let raw = String::from_utf8_lossy(&buf[..n]).into_owned();
            let req = jet_http_parse_request(&raw);
            let resp = h(req);
            let response_text = jet_http_format_response(&resp);
            let _ = stream.write_all(response_text.as_bytes());
        });
    }
}

fn jet_http_parse_request(raw: &str) -> JetHttpRequest {
    let sep = raw.find("\r\n\r\n").unwrap_or(raw.len());
    let header_part = &raw[..sep];
    let body = if sep + 4 <= raw.len() { raw[sep + 4..].to_string() } else { String::new() };
    let mut lines = header_part.lines();
    let request_line = lines.next().unwrap_or("GET / HTTP/1.1");
    let mut parts = request_line.splitn(3, ' ');
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let mut headers = std::collections::BTreeMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(": ") {
            headers.insert(k.to_lowercase(), v.to_string());
        }
    }
    JetHttpRequest { method, path, body, headers, params: std::collections::BTreeMap::new() }
}

fn jet_http_format_response(resp: &JetHttpResponse) -> String {
    let mut out = format!("HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n", resp.status, resp.body.len());
    for (k, v) in &resp.headers {
        out.push_str(&format!("{}: {}\r\n", k, v));
    }
    out.push_str("\r\n");
    out.push_str(&resp.body);
    out
}

// D-ROUTE1=A: router runtime ──────────────────────────────────────────────────

fn jet_http_router_new() -> JetHttpRouter {
    JetHttpRouter { routes: Vec::new() }
}

fn jet_http_router_parse_pattern(pattern: &str) -> Vec<RouteSegment> {
    pattern.split('/').filter_map(|seg| {
        if seg.is_empty() { return None; }
        if let Some(name) = seg.strip_prefix(':') {
            Some(RouteSegment::Param(name.to_string()))
        } else {
            Some(RouteSegment::Static(seg.to_string()))
        }
    }).collect()
}

fn jet_http_router_register(router: &mut JetHttpRouter, method: String, pattern: String, handler: JetHttpHandler) {
    // E2804 (runtime): duplicate method+pattern panics at registration time.
    let segs = jet_http_router_parse_pattern(&pattern);
    let is_dup = router.routes.iter().any(|r| {
        r.method == method && r.segments.len() == segs.len() && r.segments.iter().zip(segs.iter()).all(|(a, b)| {
            match (a, b) {
                (RouteSegment::Static(x), RouteSegment::Static(y)) => x == y,
                (RouteSegment::Param(_), RouteSegment::Param(_)) => true,
                _ => false,
            }
        })
    });
    if is_dup {
        panic!("E2804: duplicate route `{} {}`", method, pattern);
    }
    router.routes.push(JetHttpRoute {
        method,
        segments: segs,
        handler,
    });
}

/// Count static segments in a route (for precedence: more statics win).
fn route_static_count(segs: &[RouteSegment]) -> usize {
    segs.iter().filter(|s| matches!(s, RouteSegment::Static(_))).count()
}

fn jet_http_router_dispatch(router: &JetHttpRouter, req: JetHttpRequest) -> JetHttpResponse {
    let path_segs: Vec<&str> = req.path.split('/').filter(|s| !s.is_empty()).collect();
    // Collect matching routes with their static count (for precedence).
    let mut candidates: Vec<(usize, usize)> = Vec::new(); // (route_idx, static_count)
    for (i, route) in router.routes.iter().enumerate() {
        if route.segments.len() != path_segs.len() { continue; }
        let mut ok = true;
        for (rseg, pseg) in route.segments.iter().zip(path_segs.iter()) {
            if let RouteSegment::Static(s) = rseg {
                if s != pseg { ok = false; break; }
            }
        }
        if ok { candidates.push((i, route_static_count(&route.segments))); }
    }
    if candidates.is_empty() {
        return JetHttpResponse {
            status: "404 Not Found".to_string(),
            body: "404 not found".to_string(),
            headers: std::collections::BTreeMap::new(),
        };
    }
    // Pick highest static-count match with the right method; otherwise 405.
    candidates.sort_by(|a, b| b.1.cmp(&a.1));
    let method_match = candidates.iter().find(|(i, _)| router.routes[*i].method == req.method);
    let Some((route_idx, _)) = method_match.copied() else {
        return JetHttpResponse {
            status: "405 Method Not Allowed".to_string(),
            body: "405 method not allowed".to_string(),
            headers: std::collections::BTreeMap::new(),
        };
    };
    let route = &router.routes[route_idx];
    let mut params = std::collections::BTreeMap::new();
    for (rseg, pseg) in route.segments.iter().zip(path_segs.iter()) {
        if let RouteSegment::Param(name) = rseg {
            params.insert(name.clone(), pseg.to_string());
        }
    }
    let mut req2 = req;
    req2.params = params;
    (route.handler)(req2)
}

fn jet_http_serve_router(addr: &String, router: JetHttpRouter) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind(addr.as_str())
        .unwrap_or_else(|e| { eprintln!("E2801: bind on `{}` failed: {}", addr, e); std::process::exit(1); });
    let router = std::sync::Arc::new(router);
    loop {
        let (mut stream, _peer) = match listener.accept() {
            Ok(x) => x,
            Err(e) => { eprintln!("E2801: accept failed: {}", e); continue; }
        };
        let r = router.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 16384];
            let n = stream.read(&mut buf).unwrap_or(0);
            let raw = String::from_utf8_lossy(&buf[..n]).into_owned();
            let req = jet_http_parse_request(&raw);
            let resp = jet_http_router_dispatch(&r, req);
            let response_text = jet_http_format_response(&resp);
            let _ = stream.write_all(response_text.as_bytes());
        });
    }
}

fn jet_http_request_param(req: &JetHttpRequest, name: &String) -> Option<String> {
    req.params.get(name.as_str()).cloned()
}

// ── D-ARGS1: declarative CLI arg parsing (ratified 2026-06-22) ───────────────
// The builder accumulates a spec; `jet_args_parse` runs it against an argv
// list, producing `ParsedArgs` or an error string (never exits — the caller
// decides what to print and how to exit, which keeps the API testable).
//
// Design: builder methods take the spec BY VALUE and return a new one —
// ownership-safe, no aliasing, works with both immutable (@=) and mutable
// (:=) bindings in Jet. The parse result is cloneable.
//
// `--help` is recognized but NOT parsed out of argv here; the caller tests
// `parsed.flag("help")` if they want to handle it. The auto-generated help
// text is available via `spec.help()` and `spec.help_auto()`.

/// A single entry in the spec.
#[derive(Clone)]
enum JetArgKind {
    /// Boolean flag: `--name` sets it to true.
    Flag { name: String, help: String },
    /// Value option: `--name VALUE` captures VALUE.
    Option { name: String, help: String, meta: String },
    /// Positional argument (in declaration order).
    Positional { name: String, help: String },
}

/// The builder. All methods consume self and return a new spec (builder pattern).
#[derive(Clone)]
struct JetArgsSpec {
    entries: Vec<JetArgKind>,
    prog: String,
}

/// The parse result.
#[derive(Clone)]
struct JetParsedArgs {
    flags: std::collections::HashMap<String, bool>,
    options: std::collections::HashMap<String, String>,
    positionals: Vec<String>,
}

impl JetArgsSpec {
    /// Render the generated --help text.
    fn help(&self) -> String {
        let mut s = String::new();
        // usage line
        let prog = if self.prog.is_empty() { "program".to_string() } else { self.prog.clone() };
        let has_opts = self.entries.iter().any(|e| matches!(e, JetArgKind::Flag { .. } | JetArgKind::Option { .. }));
        let positionals: Vec<&JetArgKind> = self.entries.iter().filter(|e| matches!(e, JetArgKind::Positional { .. })).collect();
        s.push_str("Usage: ");
        s.push_str(&prog);
        if has_opts { s.push_str(" [options]"); }
        for p in &positionals {
            if let JetArgKind::Positional { name, .. } = p {
                s.push(' ');
                s.push_str(name);
            }
        }
        s.push('\n');
        // flags and options
        let flags_opts: Vec<&JetArgKind> = self.entries.iter().filter(|e| !matches!(e, JetArgKind::Positional { .. })).collect();
        if !flags_opts.is_empty() {
            s.push('\n');
            s.push_str("Options:\n");
            for e in flags_opts {
                match e {
                    JetArgKind::Flag { name, help } => {
                        s.push_str(&format!("  --{:<20} {}\n", name, help));
                    }
                    JetArgKind::Option { name, help, meta } => {
                        s.push_str(&format!("  --{} {:<16} {}\n", name, meta, help));
                    }
                    _ => {}
                }
            }
        }
        // positionals
        if !positionals.is_empty() {
            s.push('\n');
            s.push_str("Arguments:\n");
            for p in positionals {
                if let JetArgKind::Positional { name, help } = p {
                    s.push_str(&format!("  {:<22} {}\n", name, help));
                }
            }
        }
        s
    }
}

fn jet_args_spec() -> JetArgsSpec {
    // argv[0] is the program name — capture it from env at spec-creation time.
    let prog = std::env::args().next().unwrap_or_default();
    JetArgsSpec { entries: Vec::new(), prog }
}

fn jet_args_flag(mut spec: JetArgsSpec, name: &String, help: &String) -> JetArgsSpec {
    spec.entries.push(JetArgKind::Flag { name: name.clone(), help: help.clone() });
    spec
}

fn jet_args_option(mut spec: JetArgsSpec, name: &String, help: &String, meta: &String) -> JetArgsSpec {
    spec.entries.push(JetArgKind::Option { name: name.clone(), help: help.clone(), meta: meta.clone() });
    spec
}

fn jet_args_positional(mut spec: JetArgsSpec, name: &String, help: &String) -> JetArgsSpec {
    spec.entries.push(JetArgKind::Positional { name: name.clone(), help: help.clone() });
    spec
}

/// Parse argv against the spec. Returns `Err(message)` on unknown flags/options
/// or missing required positionals. `argv[0]` (the program name) is skipped.
fn jet_args_parse(spec: &JetArgsSpec, argv: &Vec<String>) -> Result<JetParsedArgs, String> {
    let mut flags: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    let mut options: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut positionals: Vec<String> = Vec::new();

    // Seed all flags as false (so .flag("name") returns false when absent).
    for e in &spec.entries {
        if let JetArgKind::Flag { name, .. } = e {
            flags.insert(name.clone(), false);
        }
    }

    // Build fast lookup sets.
    let flag_names: std::collections::HashSet<String> = spec.entries.iter()
        .filter_map(|e| if let JetArgKind::Flag { name, .. } = e { Some(name.clone()) } else { None })
        .collect();
    let option_names: std::collections::HashSet<String> = spec.entries.iter()
        .filter_map(|e| if let JetArgKind::Option { name, .. } = e { Some(name.clone()) } else { None })
        .collect();

    let mut i = 1usize; // skip argv[0]
    while i < argv.len() {
        let arg = &argv[i];
        if arg == "--" {
            i += 1;
            // Everything after `--` is positional.
            while i < argv.len() {
                positionals.push(argv[i].clone());
                i += 1;
            }
            break;
        }
        if let Some(rest) = arg.strip_prefix("--") {
            // Try `--name=value` form.
            if let Some(eq) = rest.find('=') {
                let name = &rest[..eq];
                let val = &rest[eq + 1..];
                if option_names.contains(name) {
                    options.insert(name.to_string(), val.to_string());
                } else if flag_names.contains(name) {
                    return Err(format!("--{} is a flag; it takes no value (got `={}`)\n\n{}", name, val, spec.help()));
                } else {
                    return Err(format!("unknown option `--{}`\n\n{}", name, spec.help()));
                }
            } else if flag_names.contains(rest) {
                flags.insert(rest.to_string(), true);
            } else if option_names.contains(rest) {
                i += 1;
                if i >= argv.len() {
                    return Err(format!("`--{}` requires a value\n\n{}", rest, spec.help()));
                }
                options.insert(rest.to_string(), argv[i].clone());
            } else {
                return Err(format!("unknown option `--{}`\n\n{}", rest, spec.help()));
            }
        } else {
            positionals.push(arg.clone());
        }
        i += 1;
    }

    // Check required positionals.
    let required_count = spec.entries.iter().filter(|e| matches!(e, JetArgKind::Positional { .. })).count();
    if positionals.len() < required_count {
        let missing: Vec<&str> = spec.entries.iter()
            .filter_map(|e| if let JetArgKind::Positional { name, .. } = e { Some(name.as_str()) } else { None })
            .skip(positionals.len())
            .collect();
        return Err(format!("missing required argument{}: {}\n\n{}", if missing.len() == 1 { "" } else { "s" }, missing.join(", "), spec.help()));
    }

    Ok(JetParsedArgs { flags, options, positionals })
}

fn jet_parsed_flag(parsed: &JetParsedArgs, name: &String) -> bool {
    *parsed.flags.get(name.as_str()).unwrap_or(&false)
}

fn jet_parsed_option(parsed: &JetParsedArgs, name: &String) -> Option<String> {
    parsed.options.get(name.as_str()).cloned()
}

fn jet_parsed_positional(parsed: &JetParsedArgs, idx: i64) -> Option<String> {
    if idx < 0 { return None; }
    parsed.positionals.get(idx as usize).cloned()
}

impl JetShow for JetArgsSpec {
    fn jet_show(&self) -> String { format!("ArgsSpec({})", self.entries.len()) }
}
impl JetShow for JetParsedArgs {
    fn jet_show(&self) -> String { format!("ParsedArgs(flags={}, options={}, positionals={})", self.flags.len(), self.options.len(), self.positionals.len()) }
}
