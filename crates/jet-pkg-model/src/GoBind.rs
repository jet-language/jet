//! Go c-archive to Jet binding generator (D-FFI-GO1=A).
//!
//! Exported scalar functions and `runtime/cgo.Handle`-compatible `uintptr`
//! values compile into one in-process Go runtime archive. Handles become
//! move-only Jet values, so a foreign call cannot reuse a consumed handle.
//! Unsupported signatures fail before compilation instead of guessing an ABI.

use std::path::{Path, PathBuf};
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindResult {
    pub source: String,
    pub bound: Vec<String>,
    pub archive: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError {
    Source(String),
    ToolMissing,
    ToolFailed(String),
    Io(String),
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source(message) | Self::Io(message) => f.write_str(message),
            Self::ToolMissing => f.write_str("the provisioned `go` tool was not found"),
            Self::ToolFailed(detail) => write!(f, "`go build -buildmode=c-archive` rejected the source: {detail}"),
        }
    }
}

struct Function {
    name: String,
    params: Vec<(String, Scalar)>,
    result: Option<Scalar>,
}

#[derive(Clone, Copy)]
enum Scalar { Int, Float, Handle }

impl Scalar {
    fn jet(self) -> &'static str { match self { Self::Int | Self::Handle => "Int", Self::Float => "Float" } }
}

pub fn bind(source_path: &Path, source: &str, lib: &str, cache_dir: &Path) -> Result<BindResult, BindError> {
    if !is_ident(lib) { return Err(BindError::Source(format!("`{lib}` is not a valid Jet library name"))); }
    let functions = parse_exports(source)?;
    std::fs::create_dir_all(cache_dir).map_err(|e| BindError::Io(format!("could not create binding cache: {e}")))?;
    let stem = format!("jet_go_{lib}");
    let archive = cache_dir.join(format!("lib{stem}.a"));
    let header = cache_dir.join(format!("lib{stem}.h"));
    let _ = std::fs::remove_file(&archive);
    let _ = std::fs::remove_file(&header);
    if functions.iter().any(|function| {
        function.params.iter().any(|(_, ty)| matches!(ty, Scalar::Handle))
            || matches!(function.result, Some(Scalar::Handle))
    }) && std::mem::size_of::<usize>() != 8 {
        return Err(BindError::Source("Go `uintptr` handles need a 64-bit host ABI".into()));
    }
    let output = supervised_output(Command::new("go")
        .args(["build", "-buildmode=c-archive", "-trimpath", "-o"])
        .arg(&archive)
        .arg(source_path)
        .env("CGO_ENABLED", "1"), Duration::from_secs(60))?;
    if !output.status.success() { return Err(BindError::ToolFailed(launder(&output.stderr))); }
    Ok(BindResult {
        source: render(lib, &functions),
        bound: functions.into_iter().map(|function| function.name).collect(),
        archive,
    })
}

fn parse_exports(source: &str) -> Result<Vec<Function>, BindError> {
    let lines: Vec<&str> = source.lines().collect();
    let mut functions = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let marker = line.trim();
        let Some(exported) = marker.strip_prefix("//export ").map(str::trim) else { continue };
        if !is_ident(exported) { return Err(BindError::Source(format!("`{exported}` is not a bindable Go export name"))); }
        let signature = lines.get(index + 1).map(|line| line.trim()).ok_or_else(|| BindError::Source(format!("export `{exported}` has no function declaration")))?;
        let rest = signature.strip_prefix("func ").ok_or_else(|| BindError::Source(format!("export `{exported}` must be followed by its `func` declaration")))?;
        let open = rest.find('(').ok_or_else(|| BindError::Source(format!("function `{exported}` has no parameter list")))?;
        if rest[..open].trim() != exported { return Err(BindError::Source(format!("export marker `{exported}` does not match function `{}`", rest[..open].trim()))); }
        let close = rest[open + 1..].find(')').map(|offset| open + 1 + offset).ok_or_else(|| BindError::Source(format!("function `{exported}` has no closed parameter list")))?;
        let mut params = Vec::new();
        let raw_params = rest[open + 1..close].trim();
        if !raw_params.is_empty() {
            for raw in raw_params.split(',') {
                let mut pieces = raw.split_whitespace();
                let name = pieces.next().unwrap_or_default();
                let ty = pieces.next().unwrap_or_default();
                if !is_ident(name) || pieces.next().is_some() { return Err(BindError::Source(format!("parameter `{}` in `{exported}` must use `name type`", raw.trim()))); }
                params.push((name.to_string(), scalar(ty).ok_or_else(|| BindError::Source(format!("parameter `{name}` in `{exported}` uses unsupported type `{ty}`; use `int64`, `float64`, or `uintptr`")))?));
            }
        }
        let tail = rest[close + 1..].trim();
        let result = if tail.is_empty() || tail.starts_with('{') { None } else {
            let ty = tail.split_whitespace().next().unwrap_or_default();
            Some(scalar(ty).ok_or_else(|| BindError::Source(format!("result of `{exported}` uses unsupported type `{ty}`; use `int64`, `float64`, `uintptr`, or no result")))?)
        };
        functions.push(Function { name: exported.to_string(), params, result });
    }
    if functions.is_empty() { return Err(BindError::Source("no supported `//export Name` Go functions were found".into())); }
    Ok(functions)
}

fn scalar(value: &str) -> Option<Scalar> { match value { "int64" => Some(Scalar::Int), "float64" => Some(Scalar::Float), "uintptr" => Some(Scalar::Handle), _ => None } }

fn render(lib: &str, functions: &[Function]) -> String {
    let abi = format!("jet_go_{lib}");
    let mut out = format!("@Extern module c.{abi} {{\n");
    for function in functions {
        out.push_str("    fn ");
        out.push_str(&function.name);
        render_params(&mut out, &function.params);
        if let Some(result) = function.result { out.push_str(" -> "); out.push_str(result.jet()); }
        out.push_str(" = \""); out.push_str(&function.name); out.push_str("\"\n");
    }
    out.push_str("}\nuse c."); out.push_str(&abi); out.push_str(" as abi\n\npub struct Handle { value: Int }\n\n");
    for function in functions {
        out.push_str("pub fn "); out.push_str(&function.name); render_public_params(&mut out, &function.params);
        if let Some(result) = function.result { out.push_str(" -> "); out.push_str(if matches!(result, Scalar::Handle) { "Handle" } else { result.jet() }); }
        out.push_str(" {\n    ");
        if function.result.is_some() { out.push_str("return "); }
        if matches!(function.result, Some(Scalar::Handle)) { out.push_str("Handle.{ value: "); }
        out.push_str("abi."); out.push_str(&function.name); out.push('(');
        for (index, (name, ty)) in function.params.iter().enumerate() {
            if index > 0 { out.push_str(", "); }
            out.push_str(name);
            if matches!(ty, Scalar::Handle) { out.push_str(".value"); }
        }
        out.push(')');
        if matches!(function.result, Some(Scalar::Handle)) { out.push_str(" }"); }
        out.push_str("\n}\n\n");
    }
    out
}

fn render_params(out: &mut String, params: &[(String, Scalar)]) {
    out.push('(');
    for (index, (name, ty)) in params.iter().enumerate() { if index > 0 { out.push_str(", "); } out.push_str(name); out.push_str(": "); out.push_str(ty.jet()); }
    out.push(')');
}

fn render_public_params(out: &mut String, params: &[(String, Scalar)]) {
    out.push('(');
    for (index, (name, ty)) in params.iter().enumerate() {
        if index > 0 { out.push_str(", "); }
        out.push_str(name);
        out.push_str(": ");
        out.push_str(if matches!(ty, Scalar::Handle) { "Handle" } else { ty.jet() });
    }
    out.push(')');
}

struct SupervisedOutput {
    status: std::process::ExitStatus,
    stderr: Vec<u8>,
}

fn supervised_output(command: &mut Command, timeout: Duration) -> Result<SupervisedOutput, BindError> {
    const CAPTURE_LIMIT: usize = 64 * 1024;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| if e.kind() == std::io::ErrorKind::NotFound {
        BindError::ToolMissing
    } else {
        BindError::Io(format!("could not start `go`: {e}"))
    })?;
    let stdout = child.stdout.take().ok_or_else(|| BindError::Io("could not supervise `go` stdout".into()))?;
    let stderr = child.stderr.take().ok_or_else(|| BindError::Io("could not supervise `go` stderr".into()))?;
    let stdout_reader = std::thread::spawn(move || bounded_read(stdout, CAPTURE_LIMIT));
    let stderr_reader = std::thread::spawn(move || bounded_read(stderr, CAPTURE_LIMIT));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait().map_err(|e| BindError::Io(format!("could not supervise `go`: {e}")))? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(BindError::ToolFailed("the compiler exceeded the 60 second limit".into()));
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    let _ = stdout_reader.join().map_err(|_| BindError::Io("`go` stdout reader failed".into()))??;
    let stderr = stderr_reader.join().map_err(|_| BindError::Io("`go` stderr reader failed".into()))??;
    Ok(SupervisedOutput { status, stderr })
}

fn bounded_read(mut input: impl Read, limit: usize) -> Result<Vec<u8>, BindError> {
    let mut captured = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = input.read(&mut buffer).map_err(|e| BindError::Io(format!("could not read foreign compiler output: {e}")))?;
        if count == 0 { break; }
        let keep = limit.saturating_sub(captured.len()).min(count);
        captured.extend_from_slice(&buffer[..keep]);
    }
    Ok(captured)
}

fn launder(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr).lines().map(str::trim).find(|line| !line.is_empty() && !line.starts_with('#')).map(|line| line.rsplit_once(':').map_or(line, |(_, detail)| detail.trim())).filter(|line| !line.is_empty()).map(|line| line.chars().take(160).collect()).unwrap_or_else(|| "the foreign tool returned a failure status".into())
}

fn is_ident(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
