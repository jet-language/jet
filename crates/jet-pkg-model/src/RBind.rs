//! Persistent supervised R binder (D-FFI-R1=A).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindResult {
    pub source: String,
    pub bound: Vec<String>,
    pub archive: PathBuf,
    pub provenance: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError {
    Source(String),
    ToolMissing(&'static str),
    ToolFailed(&'static str, String),
    Io(String),
}
impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source(v) | Self::Io(v) => f.write_str(v),
            Self::ToolMissing(v) => write!(f, "the provisioned `{v}` tool was not found"),
            Self::ToolFailed(t, v) => write!(f, "`{t}` rejected the R binding input: {v}"),
        }
    }
}

pub fn bind(path: &Path, source: &str, lib: &str, cache: &Path) -> Result<BindResult, BindError> {
    require_supported_host(cfg!(unix))?;
    if !ident(lib) {
        return Err(BindError::Source(format!(
            "`{lib}` is not a valid Jet library name"
        )));
    }
    let rscript = tool_path("Rscript").ok_or(BindError::ToolMissing("Rscript"))?;
    let script = std::fs::canonicalize(path)
        .map_err(|e| BindError::Io(format!("could not resolve the R script: {e}")))?;
    std::fs::create_dir_all(cache)
        .map_err(|e| BindError::Io(format!("could not create R binding cache: {e}")))?;
    let build = cache.join(format!(".r-build-{lib}"));
    let _ = std::fs::remove_dir_all(&build);
    std::fs::create_dir_all(&build)
        .map_err(|e| BindError::Io(format!("could not create R build directory: {e}")))?;
    let discoverer = build.join("jet_discover.R");
    std::fs::write(&discoverer, DISCOVERER)
        .map_err(|e| BindError::Io(format!("could not write R binding inspector: {e}")))?;
    let metadata = run_capture(
        Command::new(&rscript)
            .args(["--vanilla"])
            .arg(&discoverer)
            .arg(&script),
        "Rscript",
    )?;
    let functions = parse_function_names(&metadata)?;
    let worker = cache.join(format!("{lib}_worker.R"));
    let worker_source = render_worker(&functions);
    std::fs::write(&worker, &worker_source)
        .map_err(|e| BindError::Io(format!("could not write R worker: {e}")))?;
    let worker = std::fs::canonicalize(&worker)
        .map_err(|e| BindError::Io(format!("could not resolve the R worker: {e}")))?;
    let abi = format!("jet_r_{lib}");
    let mut wrappers = String::new();
    for name in &functions {
        wrappers.push_str(&format!("const char* {abi}_invoke_{name}(int64_t h,const char*input,int64_t deadline){{return invoke(h,\"{name}\",input,deadline);}}\n"));
        wrappers.push_str(&format!("const char* {abi}_invoke_{name}_table(int64_t h,const char*input,int64_t deadline){{return invoke(h,\"__jet_table__{name}\",input,deadline);}}\n"));
    }
    let bridge = crate::PowerShellBind::render_supervisor_c(
        &abi,
        &rscript,
        &worker,
        &script,
        &wrappers,
        "\"--vanilla\",worker_path,script_path",
    );
    let c = build.join(format!("{abi}.c"));
    let object = build.join(format!("{abi}.o"));
    std::fs::write(&c, bridge)
        .map_err(|e| BindError::Io(format!("could not write R process bridge: {e}")))?;
    run(
        Command::new("cc")
            .args(["-std=c11", "-D_POSIX_C_SOURCE=200809L", "-fPIC", "-c"])
            .arg(&c)
            .arg("-o")
            .arg(&object),
        "cc",
    )?;
    let archive = cache.join(format!("lib{abi}.a"));
    let _ = std::fs::remove_file(&archive);
    run(
        Command::new("ar").arg("rcs").arg(&archive).arg(&object),
        "ar",
    )?;
    let mut identity = b"jet-r-bind-v1\0".to_vec();
    identity.extend_from_slice(source.as_bytes());
    identity.push(0);
    identity.extend_from_slice(script.to_string_lossy().as_bytes());
    identity.push(0);
    identity.extend_from_slice(rscript.to_string_lossy().as_bytes());
    identity.push(0);
    identity.extend_from_slice(worker_source.as_bytes());
    let result=BindResult{source:render_jet(lib,&functions),bound:functions,archive,provenance:format!("schema=jet-r-bind-v1\nsha256={}\nrscript={}\nscript={}\nworker={}\nworkers_per_session=1\nmax_sessions=32\ntransport=jsonlite\n",crate::SHA256::sha256_hex(&identity),rscript.display(),script.display(),worker.display())};
    let _ = std::fs::remove_dir_all(&build);
    Ok(result)
}

const DISCOVERER: &str = r#"args <- commandArgs(trailingOnly = TRUE)
expressions <- parse(file = args[[1]], keep.source = FALSE)
for (expression in expressions) {
  if (!is.call(expression) || !(as.character(expression[[1]]) %in% c("<-", "="))) next
  if (!is.symbol(expression[[2]]) || !is.call(expression[[3]]) || !identical(expression[[3]][[1]], as.name("function"))) next
  name <- as.character(expression[[2]])
  parameters <- expression[[3]][[2]]
  valid <- length(parameters) == 1 && names(parameters)[[1]] != "..." && identical(deparse(parameters[[1]]), "")
  if (!valid) { cat("E\tARG\t", name, "\n", sep = ""); quit(status = 0) }
  cat("N\t", name, "\n", sep = "")
}
"#;

fn parse_function_names(bytes: &[u8]) -> Result<Vec<String>, BindError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| BindError::Source("R returned non-UTF-8 function metadata".into()))?;
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("E\tARG\t") {
            return Err(BindError::Source(format!(
                "R function `{name}` must take one required positional argument"
            )));
        }
        let name = line.strip_prefix("N\t").ok_or_else(|| {
            BindError::Source("R parser returned malformed function metadata".into())
        })?;
        if !ident(name) {
            return Err(BindError::Source(format!(
                "R function `{name}` cannot be projected as a Jet identifier"
            )));
        }
        if reserved(name) {
            return Err(BindError::Source(format!(
                "R function `{name}` uses a reserved generated binding name"
            )));
        }
        if out
            .iter()
            .any(|v: &String| v == name || v == &format!("{name}_table"))
        {
            return Err(BindError::Source(format!(
                "R function `{name}` collides with a generated binding name"
            )));
        }
        if out.iter().any(|v| format!("{v}_table") == name) {
            return Err(BindError::Source(format!(
                "R function `{name}` collides with a generated table binding"
            )));
        }
        out.push(name.to_string());
    }
    if out.is_empty() {
        return Err(BindError::Source(
            "no top-level named R functions were found".into(),
        ));
    }
    Ok(out)
}

fn render_worker(functions: &[String]) -> String {
    let allowed = functions
        .iter()
        .map(|v| format!("  {v} = TRUE"))
        .collect::<Vec<_>>()
        .join(",\n");
    format!(
        r#"suppressPackageStartupMessages(library(jsonlite))
input <- file("/dev/stdin", "rb")
output <- file("/dev/stdout", "wb")
script <- new.env(parent = baseenv())
sys.source(commandArgs(trailingOnly = TRUE)[[1]], envir = script)
allowed <- c(
{allowed}
)
read_exact <- function(length) {{
  value <- raw(0)
  while (length(value) < length) {{
    part <- readBin(input, "raw", n = length - length(value))
    if (length(part) == 0) return(NULL)
    value <- c(value, part)
  }}
  value
}}
little <- function(value, width) as.raw(floor(as.numeric(value) / 256^(0:(width - 1))) %% 256)
from_wire <- function(value) {{
  if (is.list(value)) {{
    mapped <- lapply(value, from_wire)
    names(mapped) <- names(value)
    scalar <- vapply(mapped, function(item) is.atomic(item) && length(item) == 1, logical(1))
    kinds <- unique(vapply(mapped, typeof, character(1)))
    if (is.null(names(value)) && length(mapped) > 0 && all(scalar) && length(kinds) == 1) return(unlist(mapped, use.names = FALSE))
    return(mapped)
  }}
  value
}}
table_from_wire <- function(rows) {{
  if (!is.list(rows)) stop("invalid table rows")
  if (length(rows) == 0) return(data.frame())
  columns <- names(rows[[1]])
  valid_row <- function(row) is.list(row) && identical(names(row), columns)
  if (is.null(columns) || anyDuplicated(columns) || !all(vapply(rows, valid_row, logical(1)))) stop("invalid table row shape")
  frames <- lapply(rows, function(row) {{
    cells <- lapply(row, function(cell) {{
      item <- from_wire(cell)
      if (is.null(item)) return(NA)
      if (!is.atomic(item) || length(item) != 1) stop("table cells must be scalar")
      item
    }})
    as.data.frame(cells, stringsAsFactors = FALSE, optional = TRUE)
  }})
  do.call(rbind, frames)
}}
writeBin(c(little(5, 4), charToRaw("READY")), output)
flush(output)
repeat {{
  header <- read_exact(4)
  if (is.null(header)) break
  size <- sum(as.integer(header) * 256^(0:3))
  if (size < 1 || size > 1048576) break
  payload <- read_exact(size)
  if (is.null(payload)) break
  request <- NULL
  response <- tryCatch({{
    request <- fromJSON(rawToChar(payload), simplifyVector = FALSE)
    if (!is.list(request) || is.null(request$op)) stop("invalid request")
    if (identical(request$op, "shutdown")) NULL else {{
      command <- request$command
      table_mode <- startsWith(command, "__jet_table__")
      if (table_mode) command <- substring(command, 14)
      if (!identical(request$op, "invoke") || is.null(command) || is.na(allowed[[command]])) stop("rejected command")
      fn <- get(command, envir = script, inherits = FALSE)
      value <- fn(if (table_mode) table_from_wire(request$input) else from_wire(request$input))
      list(id = request$id, ok = TRUE, value = value)
    }}
  }}, error = function(error) list(id = if (is.list(request) && !is.null(request$id)) request$id else 0, ok = FALSE, code = "CommandFailed", value = NULL))
  if (is.null(response)) break
  encoded <- charToRaw(toJSON(response, auto_unbox = TRUE, null = "null", na = "null", dataframe = "rows", digits = NA))
  if (length(encoded) > 1048576) break
  writeBin(c(little(length(encoded), 4), little(response$id, 8), encoded), output)
  flush(output)
}}
"#
    )
}

fn render_jet(lib: &str, functions: &[String]) -> String {
    let abi = format!("jet_r_{lib}");
    let mut out=format!("#Extern module c.{abi} {{\n    fn open() -> Int = \"{abi}_open\"\n    fn take_error() -> Int = \"{abi}_take_error\"\n    fn cancel(handle: Int) = \"{abi}_cancel\"\n    fn close(handle: Int) = \"{abi}_close\"\n");
    for name in functions {
        out.push_str(&format!("    fn {name}(handle: Int, input: String, deadline_ms: Int) -> String = \"{abi}_invoke_{name}\"\n"));
        out.push_str(&format!("    fn {name}_table(handle: Int, input: String, deadline_ms: Int) -> String = \"{abi}_invoke_{name}_table\"\n"));
    }
    out.push_str(&format!("}}\nuse c.{abi} as abi\nuse core.encoding.json as json\nuse core.data as data\n\npub struct Session {{ value: Int }}\npub enum RError {{ NotRunning Timeout Cancelled Protocol CommandFailed Limit }}\n\npub fn open() -> Session ? RError {{\n    handle :: abi.open()\n    if abi.take_error() != 0 {{ return err(RError.NotRunning) }}\n    return ok(Session.{{ value: handle }})\n}}\n\npub fn cancel(session: Session) {{ abi.cancel(session.value) }}\npub fn close(session: ^Session) {{ abi.close(session.value) }}\n\n"));
    for name in functions {
        out.push_str(&format!("pub fn {name}(session: Session, input: DataTree, deadline_ms: Int) -> DataTree ? RError {{\n    raw :: abi.{name}(session.value, json.to_string(input), deadline_ms)\n    code :: abi.take_error()\n    if code == 1 {{ return err(RError.NotRunning) }}\n    if code == 2 {{ return err(RError.Timeout) }}\n    if code == 3 {{ return err(RError.Cancelled) }}\n    if code == 5 {{ return err(RError.Limit) }}\n    if code != 0 {{ return err(RError.Protocol) }}\n    response := json.parse(raw) ?? return err(RError.Protocol)\n    succeeded := (response.field(\"ok\") ?? DataTree.Bool(false)).bool() ?? false\n    if !succeeded {{ return err(RError.CommandFailed) }}\n    return ok(response.field(\"value\") ?? DataTree.Null)\n}}\n\n"));
        out.push_str(&format!("pub fn {name}_table<T: [Encode, Decode]>(session: Session, table: Table<T>, deadline_ms: Int) -> Table<T> ? RError {{\n    raw :: abi.{name}_table(session.value, json.to_string(data.rows(table)), deadline_ms)\n    code :: abi.take_error()\n    if code == 1 {{ return err(RError.NotRunning) }}\n    if code == 2 {{ return err(RError.Timeout) }}\n    if code == 3 {{ return err(RError.Cancelled) }}\n    if code == 5 {{ return err(RError.Limit) }}\n    if code != 0 {{ return err(RError.Protocol) }}\n    response := json.parse(raw) ?? return err(RError.Protocol)\n    succeeded := (response.field(\"ok\") ?? DataTree.Bool(false)).bool() ?? false\n    if !succeeded {{ return err(RError.CommandFailed) }}\n    value := response.field(\"value\") ?? return err(RError.Protocol)\n    rows := json.decode<[T]>(json.to_string(value)) ?? return err(RError.Protocol)\n    return ok(data.table(rows))\n}}\n\n"));
    }
    out
}

fn tool_path(tool: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|v| v.join(tool))
        .find(|v| v.is_file())
        .and_then(|v| std::fs::canonicalize(v).ok())
}
fn run(command: &mut Command, tool: &'static str) -> Result<(), BindError> {
    run_capture(command, tool).map(|_| ())
}
fn run_capture(command: &mut Command, tool: &'static str) -> Result<Vec<u8>, BindError> {
    const CAP: usize = 64 * 1024;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            BindError::ToolMissing(tool)
        } else {
            BindError::Io(format!("could not start `{tool}`: {e}"))
        }
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| BindError::Io(format!("could not supervise `{tool}` stdout")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| BindError::Io(format!("could not supervise `{tool}` stderr")))?;
    let out = std::thread::spawn(move || drain(stdout, CAP));
    let err = std::thread::spawn(move || drain(stderr, CAP));
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        match child
            .try_wait()
            .map_err(|e| BindError::Io(format!("could not supervise `{tool}`: {e}")))?
        {
            Some(v) => break v,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = out.join();
                let _ = err.join();
                return Err(BindError::ToolFailed(
                    tool,
                    "the tool exceeded the 60 second limit".into(),
                ));
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    let stdout = out
        .join()
        .map_err(|_| BindError::Io(format!("`{tool}` stdout reader failed")))??;
    let stderr = err
        .join()
        .map_err(|_| BindError::Io(format!("`{tool}` stderr reader failed")))??;
    if status.success() {
        Ok(stdout)
    } else {
        let _ = stderr;
        Err(BindError::ToolFailed(
            tool,
            "the R parser returned a failure status".into(),
        ))
    }
}
fn drain(mut input: impl Read, limit: usize) -> Result<Vec<u8>, BindError> {
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = input
            .read(&mut buf)
            .map_err(|e| BindError::Io(format!("could not read foreign tool output: {e}")))?;
        if n == 0 {
            break;
        }
        let keep = (limit - out.len()).min(n);
        out.extend_from_slice(&buf[..keep]);
    }
    Ok(out)
}
fn ident(v: &str) -> bool {
    let mut chars = v.chars();
    matches!(chars.next(),Some(c)if c.is_ascii_alphabetic()||c=='_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
fn reserved(v: &str) -> bool {
    matches!(v, "open" | "cancel" | "close" | "Session" | "RError")
        || crate::Syntax::JET_KEYWORD_LIST.contains(&v)
        || crate::Syntax::JET_TYPE_LIST.contains(&v)
}
fn require_supported_host(unix: bool) -> Result<(), BindError> {
    if unix {
        Ok(())
    } else {
        Err(BindError::Source(
            "persistent R bindings require a POSIX host process supervisor".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_metadata_and_rejects_collisions() {
        assert_eq!(
            super::parse_function_names(b"N\ttransform\n").unwrap(),
            vec!["transform"]
        );
        assert!(super::parse_function_names(b"N\topen\n").is_err());
        assert!(super::parse_function_names(b"N\tx\nN\tx_table\n").is_err());
    }
    #[test]
    fn rejects_bad_parameters() {
        assert!(super::parse_function_names(b"E\tARG\tbad\n")
            .unwrap_err()
            .to_string()
            .contains("one required positional argument"));
    }
    #[test]
    fn non_posix_hosts_fail() {
        assert!(super::require_supported_host(false).is_err());
    }
}
