//! Persistent supervised Perl binder (D-FFI-PERL1=A).

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

#[derive(Debug, Clone, PartialEq, Eq)]
struct BoundFunction {
    perl: String,
    jet: String,
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source(v) | Self::Io(v) => f.write_str(v),
            Self::ToolMissing(v) => write!(f, "the provisioned `{v}` tool was not found"),
            Self::ToolFailed(t, v) => write!(f, "`{t}` rejected the Perl binding input: {v}"),
        }
    }
}

pub fn bind(path: &Path, source: &str, lib: &str, cache: &Path) -> Result<BindResult, BindError> {
    require_supported_host(cfg!(unix))?;
    if !ident(lib) {
        return Err(BindError::Source(format!("`{lib}` is not a valid Jet library name")));
    }
    let perl = tool_path("perl").ok_or(BindError::ToolMissing("perl"))?;
    let script = std::fs::canonicalize(path)
        .map_err(|e| BindError::Io(format!("could not resolve the Perl script: {e}")))?;
    std::fs::create_dir_all(cache)
        .map_err(|e| BindError::Io(format!("could not create Perl binding cache: {e}")))?;
    let build = cache.join(format!(".perl-build-{lib}"));
    let _ = std::fs::remove_dir_all(&build);
    std::fs::create_dir_all(&build)
        .map_err(|e| BindError::Io(format!("could not create Perl build directory: {e}")))?;

    let discoverer = build.join("JetDiscover.pm");
    std::fs::write(&discoverer, DISCOVERER)
        .map_err(|e| BindError::Io(format!("could not write Perl binding inspector: {e}")))?;
    let discovered = run_capture(
        Command::new(&perl)
            .arg(format!("-I{}", build.display()))
            .arg("-MJetDiscover")
            .arg("-c")
            .arg(&script)
            .env("JET_PERL_BIND_SOURCE", &script),
        "perl",
    )?;
    let functions = parse_function_names(&discovered)?;
    let worker = cache.join(format!("{lib}_worker.pl"));
    let worker_source = render_worker(&functions);
    std::fs::write(&worker, &worker_source)
        .map_err(|e| BindError::Io(format!("could not write Perl worker: {e}")))?;
    let worker = std::fs::canonicalize(&worker)
        .map_err(|e| BindError::Io(format!("could not resolve the Perl worker: {e}")))?;

    let abi = format!("jet_perl_{lib}");
    let mut wrappers = String::new();
    for function in &functions {
        wrappers.push_str(&format!(
            "const char* {abi}_invoke_{}(int64_t h,const char*input,int64_t deadline){{return invoke(h,\"{}\",input,deadline);}}\n",
            function.jet, function.perl
        ));
    }
    let bridge = crate::PowerShellBind::render_supervisor_c(
        &abi,
        &perl,
        &worker,
        &script,
        &wrappers,
        "worker_path,script_path",
    );
    let c = build.join(format!("{abi}.c"));
    let object = build.join(format!("{abi}.o"));
    std::fs::write(&c, bridge)
        .map_err(|e| BindError::Io(format!("could not write Perl process bridge: {e}")))?;
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

    let mut identity = b"jet-perl-bind-v1\0".to_vec();
    identity.extend_from_slice(source.as_bytes());
    identity.push(0);
    identity.extend_from_slice(script.to_string_lossy().as_bytes());
    identity.push(0);
    identity.extend_from_slice(perl.to_string_lossy().as_bytes());
    identity.push(0);
    identity.extend_from_slice(worker_source.as_bytes());
    let result = BindResult {
        source: render_jet(lib, &functions),
        bound: functions.iter().map(|function| function.jet.clone()).collect(),
        archive,
        provenance: format!(
            "schema=jet-perl-bind-v1\nsha256={}\nperl={}\nscript={}\nworker={}\n",
            crate::SHA256::sha256_hex(&identity),
            perl.display(),
            script.display(),
            worker.display()
        ),
    };
    let _ = std::fs::remove_dir_all(&build);
    Ok(result)
}

const DISCOVERER: &str = r#"package JetDiscover;
use strict;
use warnings;
use B ();
CHECK {
    no strict 'refs';
    my $target = $ENV{JET_PERL_BIND_SOURCE};
    for my $name (sort keys %main::) {
        next if $name !~ /\A[A-Za-z_][A-Za-z0-9_]*\z/;
        my $code = *{"main::$name"}{CODE};
        next if !defined($code);
        my $file = B::svref_2object($code)->GV->FILE;
        print "$name\n" if defined($file) && $file eq $target;
    }
}
1;
"#;

fn parse_function_names(bytes: &[u8]) -> Result<Vec<BoundFunction>, BindError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| BindError::Source("Perl returned non-UTF-8 function metadata".into()))?;
    let mut out = Vec::new();
    for name in text.lines().map(str::trim).filter(|v| !v.is_empty()) {
        if !ident(name) {
            return Err(BindError::Source(format!("Perl function `{name}` cannot be projected as a Jet identifier")));
        }
        let jet = snake(name);
        if reserved(&jet) {
            return Err(BindError::Source(format!("Perl function `{name}` projects to reserved Jet name `{jet}`")));
        }
        if out.iter().any(|function: &BoundFunction| function.jet == jet) {
            return Err(BindError::Source(format!("Perl function `{name}` collides with another generated Jet function `{jet}`")));
        }
        out.push(BoundFunction { perl: name.to_string(), jet });
    }
    if out.is_empty() {
        return Err(BindError::Source("no top-level named Perl functions were found".into()));
    }
    Ok(out)
}

fn render_worker(functions: &[BoundFunction]) -> String {
    let allowed = functions
        .iter()
        .map(|function| format!("    '{}' => 1", function.perl.replace('\'', "\\'")))
        .collect::<Vec<_>>()
        .join(",\n");
    format!(r#"use strict;
use warnings;
use JSON::PP ();
use IO::Handle ();
binmode(STDIN);
binmode(STDOUT);
my %allowed = (
{allowed}
);
my $script = $ARGV[0];
my $loaded = do $script;
die "load failed" if !defined($loaded);
my $json = JSON::PP->new->allow_nonref(1);

sub read_exact {{
    my ($length) = @_;
    my $value = '';
    while (length($value) < $length) {{
        my $read = read(STDIN, my $part, $length - length($value));
        return undef if !defined($read) || $read == 0;
        $value .= $part;
    }}
    return $value;
}}

print STDOUT pack('V', 5), 'READY';
STDOUT->flush();
while (1) {{
    my $header = read_exact(4);
    last if !defined($header);
    my $length = unpack('V', $header);
    last if $length < 1 || $length > 1048576;
    my $payload = read_exact($length);
    last if !defined($payload);
    my ($request, $response);
    eval {{
        $request = $json->decode($payload);
        die "invalid request" if ref($request) ne 'HASH';
        last if ($request->{{op}} // '') eq 'shutdown';
        my $command = $request->{{command}} // '';
        die "rejected command" if ($request->{{op}} // '') ne 'invoke' || !$allowed{{$command}};
        no strict 'refs';
        my $value = &{{$command}}($request->{{input}});
        $response = {{ id => $request->{{id}}, ok => JSON::PP::true, value => $value }};
    }};
    if ($@) {{
        my $id = ref($request) eq 'HASH' ? ($request->{{id}} // 0) : 0;
        $response = {{ id => $id, ok => JSON::PP::false, code => 'CommandFailed', value => undef }};
    }}
    my $encoded = eval {{ $json->canonical(1)->encode($response) }};
    last if !defined($encoded) || length($encoded) > 1048576;
    my $id = $response->{{id}} // 0;
    my $low = $id % 4294967296;
    my $high = int($id / 4294967296);
    print STDOUT pack('V', length($encoded)), pack('V2', $low, $high), $encoded;
    STDOUT->flush();
}}
"#)
}

fn render_jet(lib: &str, functions: &[BoundFunction]) -> String {
    let abi = format!("jet_perl_{lib}");
    let mut out = format!("#Extern module c.{abi} {{\n    fn open() -> Int = \"{abi}_open\"\n    fn take_error() -> Int = \"{abi}_take_error\"\n    fn cancel(handle: Int) = \"{abi}_cancel\"\n    fn close(handle: Int) = \"{abi}_close\"\n");
    for function in functions {
        let name = &function.jet;
        out.push_str(&format!("    fn {name}(handle: Int, input: String, deadline_ms: Int) -> String = \"{abi}_invoke_{name}\"\n"));
    }
    out.push_str(&format!("}}\nuse c.{abi} as abi\nuse core.encoding.json as json\n\npub struct Session {{ value: Int }}\npub enum PerlError {{ NotRunning Timeout Cancelled Protocol CommandFailed Limit }}\n\nimpl Session.Close {{\n    fn close(^self) {{ abi.close(self.value) }}\n}}\n\npub fn close(^session: Session) {{ abi.close(session.value) }}\n\npub fn open() -> Session ? PerlError {{\n    handle :: abi.open()\n    if abi.take_error() != 0 {{ return Err(PerlError.NotRunning) }}\n    return Ok(Session.{{ value: handle }})\n}}\n\npub fn cancel(session: Session) {{ abi.cancel(session.value) }}\n\n"));
    for function in functions {
        let name = &function.jet;
        out.push_str(&format!("pub fn {name}(session: Session, input: DataTree, deadline_ms: Int) -> DataTree ? PerlError {{\n    raw :: abi.{name}(session.value, json.to_string(input), deadline_ms)\n    code :: abi.take_error()\n    if code == 1 {{ return Err(PerlError.NotRunning) }}\n    if code == 2 {{ return Err(PerlError.Timeout) }}\n    if code == 3 {{ return Err(PerlError.Cancelled) }}\n    if code == 5 {{ return Err(PerlError.Limit) }}\n    if code != 0 {{ return Err(PerlError.Protocol) }}\n    response := json.parse(raw) ?? return Err(PerlError.Protocol)\n    succeeded := (response.field(\"ok\") ?? DataTree.Bool(false)).bool() ?? false\n    if !succeeded {{ return Err(PerlError.CommandFailed) }}\n    return Ok(response.field(\"value\") ?? DataTree.Null)\n}}\n\n"));
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
        "the script has a Perl parse error".into()
    } else {
        "the Perl compiler rejected the script".into()
    }
}

fn ident(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn snake(value: &str) -> String {
    let mut out = String::new();
    for (index, ch) in value.chars().enumerate() {
        if ch.is_ascii_uppercase() && index > 0 {
            out.push('_');
        }
        out.push(ch.to_ascii_lowercase());
    }
    out
}

fn reserved(value: &str) -> bool {
    matches!(value, "open" | "cancel" | "close" | "Session" | "PerlError")
        || crate::Syntax::JET_KEYWORD_LIST.contains(&value)
        || crate::Syntax::JET_TYPE_LIST.contains(&value)
}

fn require_supported_host(unix: bool) -> Result<(), BindError> {
    if unix { Ok(()) } else {
        Err(BindError::Source("persistent Perl bindings require a POSIX host process supervisor".into()))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn projects_perl_names_to_jet_casing_without_changing_foreign_lookup() {
        let functions = super::parse_function_names(b"Fail\nSleep\nTransform\n").unwrap();
        let jet = super::render_jet("ops", &functions);
        let worker = super::render_worker(&functions);
        for (foreign, projected) in [("Fail", "fail"), ("Sleep", "sleep"), ("Transform", "transform")] {
            assert!(jet.contains(&format!("pub fn {projected}(")));
            assert!(!jet.contains(&format!("pub fn {foreign}(")));
            assert!(worker.contains(&format!("'{foreign}' => 1")));
        }
    }

    #[test]
    fn non_posix_hosts_fail_instead_of_emitting_a_posix_facade() {
        assert!(super::require_supported_host(false).is_err());
    }
}
