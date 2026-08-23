//! Persistent supervised Octave binder (D-FFI-OCTAVE1=A).
//!
//! The binder accepts only one-input/one-output `.m` functions.  The generated
//! Jet surface carries a rank-two `Tensor` across a bounded JSON protocol;
//! Octave remains responsible for the foreign computation, while Jet owns the
//! shape and error checks at the typed boundary.

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
    IO(String),
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source(value) | Self::IO(value) => f.write_str(value),
            Self::ToolMissing(tool) => write!(f, "the provisioned `{tool}` tool was not found"),
            Self::ToolFailed(tool, value) => {
                write!(f, "`{tool}` rejected the Octave binding input: {value}")
            }
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
    let (octave, tool_name) = match tool_path("octave-cli") {
        Some(path) => (path, "octave-cli"),
        None => (
            tool_path("octave").ok_or(BindError::ToolMissing("octave"))?,
            "octave",
        ),
    };
    run_capture(Command::new(&octave).arg("--version"), tool_name)?;
    let script = std::fs::canonicalize(path)
        .map_err(|error| BindError::IO(format!("could not resolve the Octave script: {error}")))?;
    if script.extension().and_then(|value| value.to_str()) != Some("m") {
        return Err(BindError::Source(
            "Octave bindings require a `.m` source file".into(),
        ));
    }
    let functions = parse_functions(source)?;
    std::fs::create_dir_all(cache).map_err(|error| {
        BindError::IO(format!("could not create Octave binding cache: {error}"))
    })?;
    let build = cache.join(format!(".octave-build-{lib}"));
    let _ = std::fs::remove_dir_all(&build);
    std::fs::create_dir_all(&build).map_err(|error| {
        BindError::IO(format!("could not create Octave build directory: {error}"))
    })?;

    let worker = cache.join(format!("{lib}_worker.m"));
    let worker_source = render_worker(&script, &functions);
    std::fs::write(&worker, &worker_source)
        .map_err(|error| BindError::IO(format!("could not write Octave worker: {error}")))?;
    let worker = std::fs::canonicalize(&worker)
        .map_err(|error| BindError::IO(format!("could not resolve the Octave worker: {error}")))?;

    let abi = format!("jet_octave_{lib}");
    let mut wrappers = String::new();
    for function in &functions {
        wrappers.push_str(&format!(
            "const char* {abi}_invoke_{function}(int64_t h,const char*input,int64_t deadline){{return invoke(h,\"{function}\",input,deadline);}}\n"
        ));
    }
    let bridge = crate::PowerShellBind::render_supervisor_c(
        &abi,
        &octave,
        &worker,
        &script,
        &wrappers,
        "\"--quiet\",\"--no-gui\",\"--no-window-system\",\"--no-init-file\",worker_path",
    );
    let c = build.join(format!("{abi}.c"));
    let object = build.join(format!("{abi}.o"));
    std::fs::write(&c, bridge).map_err(|error| {
        BindError::IO(format!("could not write Octave process bridge: {error}"))
    })?;
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

    let mut identity = b"jet-octave-bind-v1\0".to_vec();
    identity.extend_from_slice(source.as_bytes());
    identity.push(0);
    identity.extend_from_slice(script.to_string_lossy().as_bytes());
    identity.push(0);
    identity.extend_from_slice(octave.to_string_lossy().as_bytes());
    identity.push(0);
    identity.extend_from_slice(worker_source.as_bytes());
    let result = BindResult {
        source: render_jet(lib, &functions),
        bound: functions,
        archive,
        provenance: format!(
            "schema=jet-octave-bind-v1\nsha256={}\noctave={}\nscript={}\nworker={}\ntransport=json\norder=column-major\nshape=rank-2\nmax_sessions=32\n",
            crate::SHA256::sha256_hex(&identity),
            octave.display(),
            script.display(),
            worker.display()
        ),
    };
    let _ = std::fs::remove_dir_all(&build);
    Ok(result)
}

fn parse_functions(source: &str) -> Result<Vec<String>, BindError> {
    let mut functions = Vec::new();
    for line in source.lines() {
        let line = line.split('%').next().unwrap_or_default().trim();
        let Some(rest) = line.strip_prefix("function") else {
            continue;
        };
        if rest
            .chars()
            .next()
            .is_some_and(|value| value.is_ascii_alphanumeric() || value == '_')
        {
            continue;
        }
        let Some(open) = rest.find('(') else {
            return Err(BindError::Source(
                "Octave function declaration is missing its argument list".into(),
            ));
        };
        let Some(close) = rest[open + 1..].find(')') else {
            return Err(BindError::Source(
                "Octave function declaration has an unterminated argument list".into(),
            ));
        };
        let close = open + 1 + close;
        let head = rest[..open].trim();
        let Some(equal) = head.rfind('=') else {
            return Err(BindError::Source(
                "Octave functions must declare one matrix output with `name = function(input)`"
                    .into(),
            ));
        };
        let output = head[..equal].trim();
        let output = if let Some(inner) = output.strip_prefix('[') {
            let Some(inner) = inner.strip_suffix(']') else {
                return Err(BindError::Source(
                    "Octave multiple-output functions must use one bracketed output name".into(),
                ));
            };
            inner.trim()
        } else {
            output
        };
        if output.is_empty() || output.contains(',') || !ident(output) {
            return Err(BindError::Source(
                "Octave multiple-output functions are unsupported; expose one matrix output per binding".into(),
            ));
        }
        let name = head[equal + 1..].trim();
        if !ident(name) {
            return Err(BindError::Source(format!(
                "Octave function `{name}` cannot be projected as a Jet identifier"
            )));
        }
        if reserved(name) {
            return Err(BindError::Source(format!(
                "Octave function `{name}` uses a reserved generated binding name"
            )));
        }
        let params = rest[open + 1..close].trim();
        if params.is_empty() || params.contains(',') || params == "varargin" {
            return Err(BindError::Source(format!(
                "Octave function `{name}` must take exactly one matrix argument"
            )));
        }
        if functions.iter().any(|value| value == name) {
            return Err(BindError::Source(format!(
                "Octave function `{name}` is declared more than once"
            )));
        }
        functions.push(name.to_string());
    }
    if functions.is_empty() {
        return Err(BindError::Source(
            "no top-level Octave functions were found".into(),
        ));
    }
    Ok(functions)
}

fn render_worker(script: &Path, functions: &[String]) -> String {
    let allowed = functions
        .iter()
        .map(|value| format!("'{}'", value.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ");
    let worker = r#"input = fopen('/dev/stdin', 'rb');
output = fopen('/dev/stdout', 'wb');
script_path = __JET_SCRIPT__;
if exist('jsondecode', 'file') ~= 2 || exist('jsonencode', 'file') ~= 2
  error('Octave jsondecode/jsonencode are required by the Jet sidecar');
end
try
  evalc('source(script_path)');
catch
  addpath(fileparts(script_path));
end
allowed = {__JET_ALLOWED__};
fwrite(output, uint8([5 0 0 0]), 'uint8');
fwrite(output, uint8('READY'), 'uint8');
fflush(output);
while true
  header = fread(input, 4, '*uint8')';
  if numel(header) ~= 4
    break;
  end
  length = double(header(1)) + 256 * double(header(2)) + 65536 * double(header(3)) + 16777216 * double(header(4));
  if length < 1 || length > 1048576
    break;
  end
  payload = fread(input, length, '*uint8')';
  if numel(payload) ~= length
    break;
  end
  id = 0;
  shutdown = false;
  try
    request = jsondecode(char(payload));
    id = double(request.id);
    if strcmp(request.op, 'shutdown')
      shutdown = true;
    elseif ~strcmp(request.op, 'invoke') || ~any(strcmp(request.command, allowed))
      error('rejected Octave command');
    else
      shape = double(request.input.shape(:)');
      values = double(request.input.data(:)');
      if numel(shape) ~= 2 || any(shape < 0) || any(shape ~= fix(shape)) || numel(values) ~= shape(1) * shape(2)
        error('invalid rank-two matrix wire');
      end
      matrix = reshape(values, shape(1), shape(2));
      value = feval(request.command, matrix);
      if ~isnumeric(value) || ~isreal(value) || any(~isfinite(value(:))) || ndims(value) > 2
        error('Octave function did not return a finite real matrix');
      end
      wire = struct('shape', double(size(value)), 'data', double(value(:)'));
      response = struct('id', id, 'ok', true, 'value', wire);
    end
  catch
    response = struct('id', id, 'ok', false, 'code', 'CommandFailed', 'value', []);
  end
  if shutdown
    break;
  end
  encoded = uint8(jsonencode(response));
  if numel(encoded) > 1048576
    break;
  end
  size_bytes = uint8(mod(floor(double(numel(encoded)) ./ (256 .^ (0:3))), 256));
  id_bytes = uint8(mod(floor(id ./ (256 .^ (0:7))), 256));
  fwrite(output, size_bytes, 'uint8');
  fwrite(output, id_bytes, 'uint8');
  fwrite(output, encoded, 'uint8');
  fflush(output);
end
fclose(input);
fclose(output);
"#
    .replace("__JET_SCRIPT__", &octave_string(&script.to_string_lossy()))
    .replace("__JET_ALLOWED__", &allowed);
    worker
}

fn render_jet(lib: &str, functions: &[String]) -> String {
    let abi = format!("jet_octave_{lib}");
    let mut out = format!(
        r#"use c.{abi} as abi
use core.compute as compute
use core.encoding.json as json

#Extern module c.{abi} {{
    fn open() Int = "{abi}_open"
    fn take_error() Int = "{abi}_take_error"
    fn cancel(handle: Int) = "{abi}_cancel"
    fn close(handle: Int) = "{abi}_close"
"#
    );
    for function in functions {
        out.push_str(&format!(
            "    fn {function}(handle: Int, input: String, deadline_ms: Int) String = \"{abi}_invoke_{function}\"\n"
        ));
    }
    out.push_str(&format!(
        r#"}}

#Codable
struct TensorWire {{
    shape: [Int]
    data: [Float]
}}

pub struct Session {{
    value: Int
}}

pub enum OctaveError {{
    NotRunning
    Timeout
    Cancelled
    Protocol
    CommandFailed
    Shape
    Width
    Limit
}}

impl Session.Close {{
    fn close(^self) {{ abi.close(self.value) }}
}}

pub fn close(session: ^Session) -[FFI.Octave]> {{ abi.close(session.value) }}

pub fn open() Session OctaveError! -[FFI.Octave]> {{
    handle :: abi.open()
    if abi.take_error() != 0 -> return Err(OctaveError.NotRunning)
    return Ok(Session{{ value: handle }})
}}

pub fn cancel(session: Session) -[FFI.Octave]> {{ abi.cancel(session.value) }}

"#
    ));
    for function in functions {
        out.push_str(&format!(
            r#"pub fn {function}(session: Session, input: Tensor, deadline_ms: Int) Tensor OctaveError! -[FFI.Octave]> {{
    if compute.rank(input) != 2 -> return Err(OctaveError.Shape)
    request :: json.to_string(TensorWire{{ shape: compute.shape(input), data: compute.to_list(input) }})
    raw :: abi.{function}(session.value, request, deadline_ms)
    code :: abi.take_error()
    if code == 1 -> return Err(OctaveError.NotRunning)
    if code == 2 -> return Err(OctaveError.Timeout)
    if code == 3 -> return Err(OctaveError.Cancelled)
    if code == 5 -> return Err(OctaveError.Limit)
    if code != 0 -> return Err(OctaveError.Protocol)
    response :: json.parse(raw) ?? return Err(OctaveError.Protocol)
    succeeded :: (response.field("ok") ?? DataTree.Bool(false)).bool() ?? false
    if !succeeded -> return Err(OctaveError.CommandFailed)
    value :: response.field("value") ?? return Err(OctaveError.Protocol)
    wire :: json.decode<TensorWire>(json.to_string(value)) ?? return Err(OctaveError.Protocol)
    if wire.shape.len() != 2 -> return Err(OctaveError.Shape)
    if wire.shape[0] < 0 || wire.shape[1] < 0 -> return Err(OctaveError.Shape)
    if wire.data.len() != wire.shape[0] * wire.shape[1] -> return Err(OctaveError.Width)
    flat :: compute.from_list(wire.data) ?? return Err(OctaveError.Width)
    result :: compute.reshape(flat, wire.shape) ?? return Err(OctaveError.Shape)
    return Ok(result)
}}

"#
        ));
    }
    out
}

fn tool_path(tool: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|value| value.join(tool))
        .find(|value| value.is_file())
        .and_then(|value| std::fs::canonicalize(value).ok())
}

fn run(command: &mut Command, tool: &'static str) -> Result<(), BindError> {
    run_capture(command, tool).map(|_| ())
}

fn run_capture(command: &mut Command, tool: &'static str) -> Result<Vec<u8>, BindError> {
    const CAP: usize = 64 * 1024;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            BindError::ToolMissing(tool)
        } else {
            BindError::IO(format!("could not start `{tool}`: {error}"))
        }
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| BindError::IO(format!("could not supervise `{tool}` stdout")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| BindError::IO(format!("could not supervise `{tool}` stderr")))?;
    let out = std::thread::spawn(move || drain(stdout, CAP));
    let err = std::thread::spawn(move || drain(stderr, CAP));
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        match child
            .try_wait()
            .map_err(|error| BindError::IO(format!("could not supervise `{tool}`: {error}")))?
        {
            Some(status) => break status,
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
        .map_err(|_| BindError::IO(format!("`{tool}` stdout reader failed")))??;
    let stderr = err
        .join()
        .map_err(|_| BindError::IO(format!("`{tool}` stderr reader failed")))??;
    if status.success() {
        Ok(stdout)
    } else {
        let detail = if stderr.is_empty() { &stdout } else { &stderr };
        Err(BindError::ToolFailed(tool, summarize(detail)))
    }
}

fn drain(mut input: impl Read, limit: usize) -> Result<Vec<u8>, BindError> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = input.read(&mut buffer).map_err(|error| {
            BindError::IO(format!("could not read foreign tool output: {error}"))
        })?;
        if count == 0 {
            break;
        }
        let keep = limit.saturating_sub(output.len()).min(count);
        output.extend_from_slice(&buffer[..keep]);
    }
    Ok(output)
}

fn summarize(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(160).collect())
        .unwrap_or_else(|| "the foreign tool returned a failure status".into())
}

fn octave_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn ident(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn reserved(value: &str) -> bool {
    matches!(
        value,
        "open"
            | "take_error"
            | "cancel"
            | "close"
            | "abi"
            | "compute"
            | "json"
            | "Session"
            | "TensorWire"
            | "OctaveError"
    ) || crate::Syntax::JET_KEYWORD_LIST.contains(&value)
        || crate::Syntax::JET_TYPE_LIST.contains(&value)
}

fn require_supported_host(unix: bool) -> Result<(), BindError> {
    if unix {
        Ok(())
    } else {
        Err(BindError::Source(
            "persistent Octave bindings require a POSIX host process supervisor".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_one_matrix_argument_and_rejects_ambiguous_outputs() {
        let functions = super::parse_functions(
            "function y = scale(x)\n  y = x * 2;\nend\nfunction [z] = shift(m)\n  z = m + 1;\nend\n",
        )
        .unwrap();
        assert_eq!(functions, ["scale", "shift"]);
        let error = super::parse_functions("function [a, b] = split(x)\nend\n").unwrap_err();
        assert!(error.to_string().contains("multiple-output"));
    }

    #[test]
    fn generated_surface_keeps_tensor_shape_checks_and_current_arrows() {
        let source = super::render_jet("matrix", &["scale".into()]);
        assert!(source.contains("TensorWire{ shape: compute.shape(input)"));
        assert!(source.contains("compute.rank(input) != 2 ->"));
        assert!(source.contains("-[FFI.Octave]> {"));
        assert!(!source.contains("=>") && !source.contains("Session.{"));
    }

    #[test]
    fn worker_has_allowlist_and_bounded_matrix_wire() {
        let source =
            super::render_worker(std::path::Path::new("/work/legacy.m"), &["scale".into()]);
        assert!(source.contains("'scale'"));
        assert!(source.contains("numel(shape) ~= 2"));
        assert!(source.contains("numel(encoded) > 1048576"));
        assert!(source.contains("script_path = '/work/legacy.m'"));
    }

    #[test]
    fn non_posix_hosts_fail_instead_of_emitting_a_facade() {
        let error = super::require_supported_host(false).unwrap_err();
        assert!(error.to_string().contains("POSIX host process supervisor"));
    }
}
