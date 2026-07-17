//! Persistent supervised Ruby binder (D-FFI-RUBY1=A).

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
            Self::ToolFailed(t, v) => write!(f, "`{t}` rejected the Ruby binding input: {v}"),
        }
    }
}

pub fn bind(path: &Path, source: &str, lib: &str, cache: &Path) -> Result<BindResult, BindError> {
    require_supported_host(cfg!(unix))?;
    if !ident(lib) {
        return Err(BindError::Source(format!("`{lib}` is not a valid Jet library name")));
    }
    let ruby = tool_path("ruby").ok_or(BindError::ToolMissing("ruby"))?;
    let script = std::fs::canonicalize(path)
        .map_err(|e| BindError::Io(format!("could not resolve the Ruby script: {e}")))?;
    std::fs::create_dir_all(cache)
        .map_err(|e| BindError::Io(format!("could not create Ruby binding cache: {e}")))?;
    let build = cache.join(format!(".ruby-build-{lib}"));
    let _ = std::fs::remove_dir_all(&build);
    std::fs::create_dir_all(&build)
        .map_err(|e| BindError::Io(format!("could not create Ruby build directory: {e}")))?;

    let discoverer = build.join("jet_discover.rb");
    std::fs::write(&discoverer, DISCOVERER)
        .map_err(|e| BindError::Io(format!("could not write Ruby binding inspector: {e}")))?;
    let discovered = run_capture(
        Command::new(&ruby).arg(&discoverer).arg(&script),
        "ruby",
    )?;
    let functions = parse_function_names(&discovered)?;
    let worker = cache.join(format!("{lib}_worker.rb"));
    let worker_source = render_worker(&functions);
    std::fs::write(&worker, &worker_source)
        .map_err(|e| BindError::Io(format!("could not write Ruby worker: {e}")))?;
    let worker = std::fs::canonicalize(&worker)
        .map_err(|e| BindError::Io(format!("could not resolve the Ruby worker: {e}")))?;

    let abi = format!("jet_ruby_{lib}");
    let mut wrappers = String::new();
    for name in &functions {
        wrappers.push_str(&format!(
            "const char* {abi}_invoke_{name}(int64_t h,const char*input,int64_t deadline){{return invoke(h,\"{name}\",input,deadline);}}\n"
        ));
    }
    let bridge = crate::PowerShellBind::render_supervisor_c(
        &abi,
        &ruby,
        &worker,
        &script,
        &wrappers,
        "worker_path,script_path",
    );
    let c = build.join(format!("{abi}.c"));
    let object = build.join(format!("{abi}.o"));
    std::fs::write(&c, bridge)
        .map_err(|e| BindError::Io(format!("could not write Ruby process bridge: {e}")))?;
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
    run(Command::new("ar").arg("rcs").arg(&archive).arg(&object), "ar")?;

    let mut identity = b"jet-ruby-bind-v1\0".to_vec();
    identity.extend_from_slice(source.as_bytes());
    identity.push(0);
    identity.extend_from_slice(script.to_string_lossy().as_bytes());
    identity.push(0);
    identity.extend_from_slice(ruby.to_string_lossy().as_bytes());
    identity.push(0);
    identity.extend_from_slice(worker_source.as_bytes());
    let result = BindResult {
        source: render_jet(lib, &functions),
        bound: functions.clone(),
        archive,
        provenance: format!(
            "schema=jet-ruby-bind-v1\nsha256={}\nruby={}\nscript={}\nworker={}\n",
            crate::SHA256::sha256_hex(&identity),
            ruby.display(),
            script.display(),
            worker.display()
        ),
    };
    let _ = std::fs::remove_dir_all(&build);
    Ok(result)
}

const DISCOVERER: &str = r#"require 'ripper'
source = File.binread(ARGV.fetch(0))
tree = Ripper.sexp(source)
abort 'parse error' unless tree && tree[0] == :program
tree[1].each do |statement|
  next unless statement.is_a?(Array) && statement[0] == :def
  name = statement.dig(1, 1)
  parameters = statement[2]
  parameters = parameters[1] if parameters&.first == :paren
  valid = parameters&.first == :params && parameters[1]&.length == 1 &&
    parameters[2..7].all?(&:nil?)
  abort "unsupported parameters for #{name}" unless valid
  puts name
end
"#;

fn parse_function_names(bytes: &[u8]) -> Result<Vec<String>, BindError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| BindError::Source("Ruby returned non-UTF-8 function metadata".into()))?;
    let mut out = Vec::new();
    for name in text.lines().map(str::trim).filter(|v| !v.is_empty()) {
        if !ident(name) {
            return Err(BindError::Source(format!("Ruby function `{name}` cannot be projected as a Jet identifier")));
        }
        if reserved(name) {
            return Err(BindError::Source(format!("Ruby function `{name}` uses a reserved generated binding name")));
        }
        if out.iter().any(|v| v == name) {
            return Err(BindError::Source(format!("Ruby function `{name}` is declared more than once")));
        }
        out.push(name.to_string());
    }
    if out.is_empty() {
        return Err(BindError::Source("no top-level named Ruby functions were found".into()));
    }
    Ok(out)
}

fn render_worker(functions: &[String]) -> String {
    let allowed = functions
        .iter()
        .map(|v| format!("'{v}'"))
        .collect::<Vec<_>>().join(", ");
    format!(r#"require 'json'
STDIN.binmode
STDOUT.binmode
ALLOWED = [{allowed}].to_h {{ |name| [name, true] }}.freeze
load ARGV.fetch(0)

def read_exact(length)
  value = +''
  while value.bytesize < length
    part = STDIN.read(length - value.bytesize)
    return nil unless part
    value << part
  end
  value
end

STDOUT.write([5].pack('L<'), 'READY')
STDOUT.flush
loop do
  header = read_exact(4)
  break unless header
  length = header.unpack1('L<')
  break unless length.between?(1, 1_048_576)
  payload = read_exact(length)
  break unless payload
  request = nil
  begin
    request = JSON.parse(payload)
    raise 'invalid request' unless request.is_a?(Hash)
    break if request['op'] == 'shutdown'
    command = request['command']
    raise 'rejected command' unless request['op'] == 'invoke' && ALLOWED[command]
    value = Object.new.__send__(command, request['input'])
    response = {{ 'id' => request['id'], 'ok' => true, 'value' => value }}
  rescue StandardError
    id = request.is_a?(Hash) ? (request['id'] || 0) : 0
    response = {{ 'id' => id, 'ok' => false, 'code' => 'CommandFailed', 'value' => nil }}
  end
  encoded = JSON.generate(response)
  break if encoded.bytesize > 1_048_576
  STDOUT.write([encoded.bytesize].pack('L<'), [response['id'] || 0].pack('Q<'), encoded)
  STDOUT.flush
end
"#)
}

fn render_jet(lib: &str, functions: &[String]) -> String {
    let abi = format!("jet_ruby_{lib}");
    let mut out = format!("@Extern module c.{abi} {{\n    fn open() -> Int = \"{abi}_open\"\n    fn take_error() -> Int = \"{abi}_take_error\"\n    fn cancel(handle: Int) = \"{abi}_cancel\"\n    fn close(handle: Int) = \"{abi}_close\"\n");
    for name in functions {
        out.push_str(&format!("    fn {name}(handle: Int, input: String, deadline_ms: Int) -> String = \"{abi}_invoke_{name}\"\n"));
    }
    out.push_str(&format!("}}\nuse c.{abi} as abi\nuse core.encoding.json as json\n\npub struct Session {{ value: Int }}\npub enum RubyError {{ NotRunning Timeout Cancelled Protocol CommandFailed Limit }}\n\nimpl Session.Close {{\n    fn close(^self) {{ abi.close(self.value) }}\n}}\n\npub fn open() -> Session ? RubyError {{\n    handle :: abi.open()\n    if abi.take_error() != 0 {{ return err(RubyError.NotRunning) }}\n    return ok(Session.{{ value: handle }})\n}}\n\npub fn cancel(session: Session) {{ abi.cancel(session.value) }}\n\n"));
    for name in functions {
        out.push_str(&format!("pub fn {name}(session: Session, input: DataTree, deadline_ms: Int) -> DataTree ? RubyError {{\n    raw :: abi.{name}(session.value, json.to_string(input), deadline_ms)\n    code :: abi.take_error()\n    if code == 1 {{ return err(RubyError.NotRunning) }}\n    if code == 2 {{ return err(RubyError.Timeout) }}\n    if code == 3 {{ return err(RubyError.Cancelled) }}\n    if code == 5 {{ return err(RubyError.Limit) }}\n    if code != 0 {{ return err(RubyError.Protocol) }}\n    response := json.parse(raw) ?? return err(RubyError.Protocol)\n    succeeded := (response.field(\"ok\") ?? DataTree.Bool(false)).bool() ?? false\n    if !succeeded {{ return err(RubyError.CommandFailed) }}\n    return ok(response.field(\"value\") ?? DataTree.Null)\n}}\n\n"));
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
    const CAP: usize = 64 * 1024;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| if e.kind() == std::io::ErrorKind::NotFound {
        BindError::ToolMissing(tool)
    } else {
        BindError::Io(format!("could not start `{tool}`: {e}"))
    })?;
    let stdout = child.stdout.take().ok_or_else(|| BindError::Io(format!("could not supervise `{tool}` stdout")))?;
    let stderr = child.stderr.take().ok_or_else(|| BindError::Io(format!("could not supervise `{tool}` stderr")))?;
    let out = std::thread::spawn(move || drain(stdout, CAP));
    let err = std::thread::spawn(move || drain(stderr, CAP));
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        match child.try_wait().map_err(|e| BindError::Io(format!("could not supervise `{tool}`: {e}")))? {
            Some(v) => break v,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = out.join();
                let _ = err.join();
                return Err(BindError::ToolFailed(tool, "the tool exceeded the 60 second limit".into()));
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    let stdout = out.join().map_err(|_| BindError::Io(format!("`{tool}` stdout reader failed")))??;
    let stderr = err.join().map_err(|_| BindError::Io(format!("`{tool}` stderr reader failed")))??;
    if status.success() { Ok(()) } else {
        let detail = if stderr.is_empty() { &stdout } else { &stderr };
        Err(BindError::ToolFailed(tool, launder(detail)))
    }
}

fn run_capture(command: &mut Command, tool: &'static str) -> Result<Vec<u8>, BindError> {
    const CAP: usize = 64 * 1024;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { BindError::ToolMissing(tool) } else { BindError::Io(format!("could not start `{tool}`: {e}")) })?;
    let stdout = child.stdout.take().ok_or_else(|| BindError::Io(format!("could not supervise `{tool}` stdout")))?;
    let stderr = child.stderr.take().ok_or_else(|| BindError::Io(format!("could not supervise `{tool}` stderr")))?;
    let out = std::thread::spawn(move || drain(stdout, CAP));
    let err = std::thread::spawn(move || drain(stderr, CAP));
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        match child.try_wait().map_err(|e| BindError::Io(format!("could not supervise `{tool}`: {e}")))? {
            Some(v) => break v,
            None if Instant::now() >= deadline => { let _=child.kill();let _=child.wait();let _=out.join();let _=err.join();return Err(BindError::ToolFailed(tool,"the tool exceeded the 60 second limit".into())); }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    let stdout=out.join().map_err(|_|BindError::Io(format!("`{tool}` stdout reader failed")))??;
    let stderr=err.join().map_err(|_|BindError::Io(format!("`{tool}` stderr reader failed")))??;
    if status.success(){Ok(stdout)}else{Err(BindError::ToolFailed(tool,launder(&stderr)))}
}

fn drain(mut input: impl Read, limit: usize) -> Result<Vec<u8>, BindError> {
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = input.read(&mut buf).map_err(|e| BindError::Io(format!("could not read foreign tool output: {e}")))?;
        if n == 0 { break; }
        let keep = (limit - out.len()).min(n);
        out.extend_from_slice(&buf[..keep]);
    }
    Ok(out)
}

fn launder(value: &[u8]) -> String {
    let text = String::from_utf8_lossy(value);
    if text.contains("syntax error") || text.contains("compilation aborted") {
        "the script has a Ruby parse error".into()
    } else {
        "the Ruby compiler rejected the script".into()
    }
}

fn ident(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn reserved(value: &str) -> bool {
    matches!(value, "open" | "cancel" | "close" | "Session" | "RubyError")
        || crate::Syntax::JET_KEYWORD_LIST.contains(&value)
        || crate::Syntax::JET_TYPE_LIST.contains(&value)
}

fn require_supported_host(unix: bool) -> Result<(), BindError> {
    if unix { Ok(()) } else {
        Err(BindError::Source("persistent Ruby bindings require a POSIX host process supervisor".into()))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_compiler_discovered_function_names() {
        assert_eq!(super::parse_function_names(b"Fail\nTransform\n").unwrap(), vec!["Fail", "Transform"]);
    }

    #[test]
    fn rejects_reserved_duplicate_and_non_jet_method_names() {
        assert!(super::parse_function_names(b"open\n").is_err());
        assert!(super::parse_function_names(b"call\ncall\n").is_err());
        assert!(super::parse_function_names(b"ready?\n").is_err());
    }

    #[test]
    fn non_posix_hosts_fail_instead_of_emitting_a_posix_facade() {
        assert!(super::require_supported_host(false).is_err());
    }
}
