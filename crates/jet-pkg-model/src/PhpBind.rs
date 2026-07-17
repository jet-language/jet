//! Persistent supervised PHP worker-pool binder (D-FFI-PHP1=A).

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
            Self::ToolFailed(t, v) => write!(f, "`{t}` rejected the PHP binding input: {v}"),
        }
    }
}

pub fn bind(path: &Path, source: &str, lib: &str, cache: &Path) -> Result<BindResult, BindError> {
    require_supported_host(cfg!(unix))?;
    if !ident(lib) {
        return Err(BindError::Source(format!("`{lib}` is not a valid Jet library name")));
    }
    let php = tool_path("php").ok_or(BindError::ToolMissing("php"))?;
    let script = std::fs::canonicalize(path)
        .map_err(|e| BindError::Io(format!("could not resolve the PHP script: {e}")))?;
    lint(&php, &script)?;
    let functions = discover_functions(&php, &script)?;
    std::fs::create_dir_all(cache)
        .map_err(|e| BindError::Io(format!("could not create PHP binding cache: {e}")))?;
    let build = cache.join(format!(".php-build-{lib}"));
    let _ = std::fs::remove_dir_all(&build);
    std::fs::create_dir_all(&build)
        .map_err(|e| BindError::Io(format!("could not create PHP build directory: {e}")))?;

    let worker = cache.join(format!("{lib}_worker.php"));
    let worker_source = render_worker(&functions);
    std::fs::write(&worker, &worker_source)
        .map_err(|e| BindError::Io(format!("could not write PHP worker: {e}")))?;
    let worker = std::fs::canonicalize(&worker)
        .map_err(|e| BindError::Io(format!("could not resolve the PHP worker: {e}")))?;

    let abi = format!("jet_php_{lib}");
    let inner = format!("{abi}_worker");
    let base = crate::PowerShellBind::render_supervisor_c(
        &inner,
        &php,
        &worker,
        &script,
        "",
        "worker_path,script_path",
    );
    let bridge = format!("{base}\n{}", render_pool_c(&abi, &inner, &functions));
    let c = build.join(format!("{abi}.c"));
    let object = build.join(format!("{abi}.o"));
    std::fs::write(&c, bridge)
        .map_err(|e| BindError::Io(format!("could not write PHP pool bridge: {e}")))?;
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

    let mut identity = b"jet-php-bind-v1\0".to_vec();
    identity.extend_from_slice(source.as_bytes());
    identity.push(0);
    identity.extend_from_slice(script.to_string_lossy().as_bytes());
    identity.push(0);
    identity.extend_from_slice(php.to_string_lossy().as_bytes());
    identity.push(0);
    identity.extend_from_slice(worker_source.as_bytes());
    let result = BindResult {
        source: render_jet(lib, &functions),
        bound: functions,
        archive,
        provenance: format!(
            "schema=jet-php-bind-v1\nsha256={}\nphp={}\nscript={}\nworker={}\npool_workers=4\n",
            crate::SHA256::sha256_hex(&identity),
            php.display(),
            script.display(),
            worker.display()
        ),
    };
    let _ = std::fs::remove_dir_all(&build);
    Ok(result)
}

const TOKEN_ANALYZER: &str = r#"
$source = @file_get_contents($argv[1]);
if ($source === false) exit(2);
$tokens = token_get_all($source, TOKEN_PARSE);
$ignorable = static function ($token): bool {
    return is_array($token) && in_array($token[0], [T_WHITESPACE, T_COMMENT, T_DOC_COMMENT], true);
};
$ampersand = static function ($token): bool {
    if ($token === '&') return true;
    if (!is_array($token)) return false;
    return (defined('T_AMPERSAND_FOLLOWED_BY_VAR_OR_VARARG') && $token[0] === constant('T_AMPERSAND_FOLLOWED_BY_VAR_OR_VARARG'))
        || (defined('T_AMPERSAND_NOT_FOLLOWED_BY_VAR_OR_VARARG') && $token[0] === constant('T_AMPERSAND_NOT_FOLLOWED_BY_VAR_OR_VARARG'));
};
$depth = 0;
for ($i = 0, $count = count($tokens); $i < $count; $i++) {
    $token = $tokens[$i];
    if ($token === '{') { $depth++; continue; }
    if ($token === '}') { $depth = max(0, $depth - 1); continue; }
    if ($depth !== 0 || !is_array($token) || $token[0] !== T_FUNCTION) continue;
    $j = $i + 1;
    while ($j < $count && $ignorable($tokens[$j])) $j++;
    if ($j < $count && $ampersand($tokens[$j])) {
        $j++;
        while ($j < $count && $ignorable($tokens[$j])) $j++;
    }
    if ($j >= $count || !is_array($tokens[$j]) || $tokens[$j][0] !== T_STRING) continue;
    $name = $tokens[$j][1];
    $j++;
    while ($j < $count && $ignorable($tokens[$j])) $j++;
    if ($j >= $count || $tokens[$j] !== '(') continue;
    $parens = 1;
    $variables = 0;
    $unsupported = false;
    for ($j++; $j < $count && $parens > 0; $j++) {
        $param = $tokens[$j];
        if ($param === '(') { $parens++; continue; }
        if ($param === ')') { $parens--; continue; }
        if ($parens !== 1) continue;
        if (is_array($param) && $param[0] === T_VARIABLE) $variables++;
        if (is_array($param) && $param[0] === T_ELLIPSIS) $unsupported = true;
        if ($param === ',' || $param === '=' || $ampersand($param)) $unsupported = true;
    }
    if ($variables !== 1 || $unsupported) {
        echo "E\tARG\t", $name, "\n";
        exit(0);
    }
    echo "N\t", $name, "\n";
}
"#;

fn discover_functions(php: &Path, script: &Path) -> Result<Vec<String>, BindError> {
    let output = run_capture(Command::new(php).args(["-d", "auto_prepend_file=", "-d", "auto_append_file=", "-r", TOKEN_ANALYZER, "--"]).arg(script), "php")?;
    let text = std::str::from_utf8(&output).map_err(|_| BindError::Source("PHP tokenizer returned non-UTF-8 metadata".into()))?;
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("E\tARG\t") {
            return Err(BindError::Source(format!("PHP function `{name}` must take one required positional argument by value")));
        }
        let name = line.strip_prefix("N\t").ok_or_else(|| BindError::Source("PHP tokenizer returned malformed metadata".into()))?;
        if !ident(name) { return Err(BindError::Source(format!("PHP function `{name}` cannot be projected as a Jet identifier"))); }
        if reserved(name) { return Err(BindError::Source(format!("PHP function `{name}` uses a reserved generated binding name"))); }
        if out.iter().any(|v: &String| v.eq_ignore_ascii_case(name)) { return Err(BindError::Source(format!("PHP function `{name}` is declared more than once"))); }
        out.push(name.to_string());
    }
    if out.is_empty() { return Err(BindError::Source("no top-level named PHP functions were found".into())); }
    Ok(out)
}

fn render_worker(functions: &[String]) -> String {
    let allowed = functions.iter().map(|v| format!("    '{}' => true", v)).collect::<Vec<_>>().join(",\n");
    format!(r#"<?php
ini_set('display_errors', '0');
error_reporting(0);
require_once $argv[1];
$allowed = [
{allowed}
];

function jet_read_exact(int $length): ?string {{
    $value = '';
    while (strlen($value) < $length) {{
        $part = fread(STDIN, $length - strlen($value));
        if ($part === false || $part === '') return null;
        $value .= $part;
    }}
    return $value;
}}

fwrite(STDOUT, pack('V', 5) . 'READY');
fflush(STDOUT);
while (true) {{
    $header = jet_read_exact(4);
    if ($header === null) break;
    $length = unpack('Vlength', $header)['length'];
    if ($length < 1 || $length > 1048576) break;
    $payload = jet_read_exact($length);
    if ($payload === null) break;
    $request = null;
    try {{
        $request = json_decode($payload, true, 512, JSON_THROW_ON_ERROR);
        if (!is_array($request)) throw new RuntimeException('invalid request');
        if (($request['op'] ?? '') === 'shutdown') break;
        $command = $request['command'] ?? '';
        if (($request['op'] ?? '') !== 'invoke' || !isset($allowed[$command])) throw new RuntimeException('rejected command');
        $value = $command($request['input']);
        $response = ['id' => $request['id'], 'ok' => true, 'value' => $value];
    }} catch (Throwable $error) {{
        $response = ['id' => is_array($request) ? ($request['id'] ?? 0) : 0, 'ok' => false, 'code' => 'CommandFailed', 'value' => null];
    }}
    try {{ $encoded = json_encode($response, JSON_THROW_ON_ERROR); }} catch (Throwable $error) {{ break; }}
    if (strlen($encoded) > 1048576) break;
    $id = (int)($response['id'] ?? 0);
    fwrite(STDOUT, pack('V', strlen($encoded)) . pack('V2', $id & 0xffffffff, intdiv($id, 4294967296)) . $encoded);
    fflush(STDOUT);
}}
"#)
}

fn render_pool_c(abi: &str, inner: &str, functions: &[String]) -> String {
    let wrappers = functions.iter().map(|name| format!("const char* {abi}_invoke_{name}(int64_t h,const char*input,int64_t deadline){{return pool_invoke(h,\"{name}\",input,deadline);}}\n")).collect::<String>();
    format!(r#"
#define PHP_POOL_WORKERS 4
#define PHP_POOLS 8
typedef struct {{int64_t workers[PHP_POOL_WORKERS];uint32_t generation;uint32_t next;int opening;int open;int closing;int active;pthread_mutex_t mutex;pthread_cond_t idle;}} PhpPool;
static PhpPool php_pools[PHP_POOLS];
static pthread_mutex_t php_lock=PTHREAD_MUTEX_INITIALIZER;
static pthread_once_t php_once=PTHREAD_ONCE_INIT;
static _Thread_local int64_t php_failed;
static void php_init(void){{for(int i=0;i<PHP_POOLS;i++){{pthread_mutex_init(&php_pools[i].mutex,0);pthread_cond_init(&php_pools[i].idle,0);}}}}
static int php_acquire(int64_t h,int*idx,int64_t*worker){{int n=(int)(h&255)-1;uint32_t gen=(uint32_t)((uint64_t)h>>8);if(n<0||n>=PHP_POOLS)return 0;pthread_mutex_lock(&php_lock);PhpPool*p=&php_pools[n];pthread_mutex_lock(&p->mutex);int ok=p->open&&!p->closing&&p->generation==gen;if(ok){{p->active++;uint32_t selected=p->next++%PHP_POOL_WORKERS;*worker=p->workers[selected];*idx=n;}}pthread_mutex_unlock(&p->mutex);pthread_mutex_unlock(&php_lock);return ok;}}
static void php_release(int idx){{PhpPool*p=&php_pools[idx];pthread_mutex_lock(&p->mutex);p->active--;if(p->active==0)pthread_cond_broadcast(&p->idle);pthread_mutex_unlock(&p->mutex);}}
int64_t {abi}_take_error(void){{int64_t value=php_failed;php_failed=0;return value;}}
int64_t {abi}_open(void){{pthread_once(&php_once,php_init);php_failed=0;int slot=-1;uint32_t gen=0;pthread_mutex_lock(&php_lock);for(int i=0;i<PHP_POOLS;i++){{PhpPool*p=&php_pools[i];pthread_mutex_lock(&p->mutex);if(!p->opening&&!p->open&&!p->closing&&p->active==0){{p->opening=1;p->generation++;if(p->generation==0)p->generation=1;slot=i;gen=p->generation;pthread_mutex_unlock(&p->mutex);break;}}pthread_mutex_unlock(&p->mutex);}}pthread_mutex_unlock(&php_lock);if(slot<0){{php_failed=1;return 0;}}int64_t made[PHP_POOL_WORKERS]={{0}};for(int i=0;i<PHP_POOL_WORKERS;i++){{made[i]={inner}_open();if(!made[i]){{for(int j=0;j<i;j++){inner}_close(made[j]);pthread_mutex_lock(&php_pools[slot].mutex);php_pools[slot].opening=0;pthread_mutex_unlock(&php_pools[slot].mutex);php_failed=1;return 0;}}}}PhpPool*p=&php_pools[slot];pthread_mutex_lock(&p->mutex);for(int i=0;i<PHP_POOL_WORKERS;i++)p->workers[i]=made[i];p->next=0;p->opening=0;p->open=1;pthread_mutex_unlock(&p->mutex);return (int64_t)(((uint64_t)gen<<8)|(uint64_t)(slot+1));}}
static const char* pool_invoke(int64_t h,const char*command,const char*json,int64_t deadline){{php_failed=0;int idx;int64_t worker;if(!php_acquire(h,&idx,&worker)){{php_failed=1;result[0]=0;return result;}}const char*value=invoke(worker,command,json,deadline);int64_t code={inner}_take_error();if(code==1||code==2||code==3||code==4){{PhpPool*p=&php_pools[idx];pthread_mutex_lock(&p->mutex);for(int i=0;i<PHP_POOL_WORKERS;i++)if(p->workers[i]==worker&&p->open){{int64_t replacement={inner}_open();p->workers[i]=replacement;break;}}pthread_mutex_unlock(&p->mutex);}}php_failed=code;php_release(idx);return value;}}
void {abi}_cancel(int64_t h){{php_failed=0;int idx;int64_t worker;if(!php_acquire(h,&idx,&worker)){{php_failed=1;return;}}PhpPool*p=&php_pools[idx];pthread_mutex_lock(&p->mutex);int64_t copy[PHP_POOL_WORKERS];for(int i=0;i<PHP_POOL_WORKERS;i++)copy[i]=p->workers[i];for(int i=0;i<PHP_POOL_WORKERS;i++){inner}_cancel(copy[i]);for(int i=0;i<PHP_POOL_WORKERS;i++){{{inner}_close(copy[i]);p->workers[i]={inner}_open();}}pthread_mutex_unlock(&p->mutex);php_release(idx);}}
void {abi}_close(int64_t h){{php_failed=0;int n=(int)(h&255)-1;uint32_t gen=(uint32_t)((uint64_t)h>>8);if(n<0||n>=PHP_POOLS){{php_failed=1;return;}}pthread_mutex_lock(&php_lock);PhpPool*p=&php_pools[n];pthread_mutex_lock(&p->mutex);if(!p->open||p->generation!=gen){{pthread_mutex_unlock(&p->mutex);pthread_mutex_unlock(&php_lock);php_failed=1;return;}}p->closing=1;p->open=0;pthread_mutex_unlock(&php_lock);while(p->active>0)pthread_cond_wait(&p->idle,&p->mutex);int64_t copy[PHP_POOL_WORKERS];for(int i=0;i<PHP_POOL_WORKERS;i++){{copy[i]=p->workers[i];p->workers[i]=0;}}p->closing=0;pthread_mutex_unlock(&p->mutex);for(int i=0;i<PHP_POOL_WORKERS;i++){inner}_close(copy[i]);}}
{wrappers}
"#)
}

fn render_jet(lib: &str, functions: &[String]) -> String {
    let abi = format!("jet_php_{lib}");
    let mut out = format!("@Extern module c.{abi} {{\n    fn open() -> Int = \"{abi}_open\"\n    fn take_error() -> Int = \"{abi}_take_error\"\n    fn cancel(handle: Int) = \"{abi}_cancel\"\n    fn close(handle: Int) = \"{abi}_close\"\n");
    for name in functions { out.push_str(&format!("    fn {name}(handle: Int, input: String, deadline_ms: Int) -> String = \"{abi}_invoke_{name}\"\n")); }
    out.push_str(&format!("}}\nuse c.{abi} as abi\nuse core.encoding.json as json\n\npub struct PhpPool {{ value: Int }}\npub enum PhpError {{ NotRunning Timeout Cancelled Protocol CommandFailed Limit }}\n\nimpl PhpPool.Close {{\n    fn close(^self) {{ abi.close(self.value) }}\n}}\n\npub fn open() -> PhpPool ? PhpError {{\n    handle :: abi.open()\n    if abi.take_error() != 0 {{ return Err(PhpError.NotRunning) }}\n    return Ok(PhpPool.{{ value: handle }})\n}}\n\npub fn cancel(pool: PhpPool) {{ abi.cancel(pool.value) }}\n\n"));
    for name in functions {
        out.push_str(&format!("pub fn {name}(pool: PhpPool, input: DataTree, deadline_ms: Int) -> DataTree ? PhpError {{\n    raw :: abi.{name}(pool.value, json.to_string(input), deadline_ms)\n    code :: abi.take_error()\n    if code == 1 {{ return Err(PhpError.NotRunning) }}\n    if code == 2 {{ return Err(PhpError.Timeout) }}\n    if code == 3 {{ return Err(PhpError.Cancelled) }}\n    if code == 5 {{ return Err(PhpError.Limit) }}\n    if code != 0 {{ return Err(PhpError.Protocol) }}\n    response := json.parse(raw) ?? return Err(PhpError.Protocol)\n    succeeded := (response.field(\"ok\") ?? DataTree.Bool(false)).bool() ?? false\n    if !succeeded {{ return Err(PhpError.CommandFailed) }}\n    return Ok(response.field(\"value\") ?? DataTree.Null)\n}}\n\n"));
    }
    out
}

fn lint(php: &Path, script: &Path) -> Result<(), BindError> {
    let mut command = Command::new(php); command.arg("-n").arg("-l").arg(script);
    match run_capture(&mut command, "php") {
        Ok(_) => Ok(()),
        Err(BindError::ToolFailed(_, _)) => Err(BindError::ToolFailed("php", "the PHP source has a parse error".into())),
        Err(error) => Err(error),
    }
}

fn tool_path(tool: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|v| v.join(tool)).find(|v| v.is_file()).and_then(|v| std::fs::canonicalize(v).ok())
}
fn run(command: &mut Command, tool: &'static str) -> Result<(), BindError> { run_capture(command, tool).map(|_| ()) }
fn run_capture(command: &mut Command, tool: &'static str) -> Result<Vec<u8>, BindError> {
    const CAP: usize = 64 * 1024;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { BindError::ToolMissing(tool) } else { BindError::Io(format!("could not start `{tool}`: {e}")) })?;
    let stdout = child.stdout.take().ok_or_else(|| BindError::Io(format!("could not supervise `{tool}` stdout")))?;
    let stderr = child.stderr.take().ok_or_else(|| BindError::Io(format!("could not supervise `{tool}` stderr")))?;
    let out = std::thread::spawn(move || drain(stdout, CAP)); let err = std::thread::spawn(move || drain(stderr, CAP));
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop { match child.try_wait().map_err(|e| BindError::Io(format!("could not supervise `{tool}`: {e}")))? {
        Some(v) => break v,
        None if Instant::now() >= deadline => { let _=child.kill();let _=child.wait();let _=out.join();let _=err.join();return Err(BindError::ToolFailed(tool,"the tool exceeded the 60 second limit".into())); }
        None => std::thread::sleep(Duration::from_millis(10)),
    }};
    let stdout = out.join().map_err(|_| BindError::Io(format!("`{tool}` stdout reader failed")))??;
    let stderr = err.join().map_err(|_| BindError::Io(format!("`{tool}` stderr reader failed")))??;
    if status.success() { Ok(stdout) } else { let _ = stderr; Err(BindError::ToolFailed(tool, "the foreign tool returned a failure status".into())) }
}
fn drain(mut input: impl Read, limit: usize) -> Result<Vec<u8>, BindError> { let mut out=Vec::new();let mut buf=[0u8;8192];loop{let n=input.read(&mut buf).map_err(|e|BindError::Io(format!("could not read foreign tool output: {e}")))?;if n==0{break}let keep=(limit-out.len()).min(n);out.extend_from_slice(&buf[..keep]);}Ok(out) }
fn ident(v: &str) -> bool { let mut chars=v.chars();matches!(chars.next(),Some(c)if c.is_ascii_alphabetic()||c=='_')&&chars.all(|c|c.is_ascii_alphanumeric()||c=='_') }
fn reserved(v: &str) -> bool { matches!(v,"open"|"cancel"|"close"|"PhpPool"|"PhpError")||crate::Syntax::JET_KEYWORD_LIST.contains(&v)||crate::Syntax::JET_TYPE_LIST.contains(&v) }
fn require_supported_host(unix: bool) -> Result<(), BindError> { if unix { Ok(()) } else { Err(BindError::Source("persistent PHP bindings require a POSIX host process supervisor".into())) } }

#[cfg(test)]
mod tests {
    fn discover(source: &str) -> Result<Vec<String>, super::BindError> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let php = super::tool_path("php").expect("Nix dev shell provisions PHP");
        let path = std::env::temp_dir().join(format!("jet_php_tokens_{}_{}.php", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::write(&path, source).expect("write PHP token fixture");
        let result = super::discover_functions(&php, &path);
        let _ = std::fs::remove_file(path);
        result
    }

    #[test]
    fn discovery_is_static_and_ignores_every_php_string_form_and_nested_function() {
        let source = r#"<?php
// function fake($input) {}
/* function fake_block($input) {} */
# function fake_hash($input) {}
die("source bytes were executed");
$single = 'function fake_single($input) {}';
$double = "function fake_double($input) {}";
$heredoc = <<<TEXT
function fake_heredoc($input) {}
TEXT;
$nowdoc = <<<'TEXT'
function fake_nowdoc($input) {}
TEXT;
$backtick = `echo 'function fake_backtick($input) {}'`;
$anonymous = function ($input) { return $input; };
if (false) { function nested_conditional($input) { return $input; } }
function price_cart(array $input): array { return $input; }
class Hidden { function method($input) {} }
"#;
        assert_eq!(discover(source).unwrap(), vec!["price_cart"]);
    }
    #[test]
    fn discovery_rejects_by_reference_variadic_default_and_multiple_arguments() {
        for source in [
            "<?php function bad(&$input) {}",
            "<?php function bad(...$input) {}",
            "<?php function bad($input = null) {}",
            "<?php function bad($input, $other) {}",
        ] {
            let error=discover(source).unwrap_err();
            assert!(error.to_string().contains("one required positional argument by value"), "{error}");
        }
    }
    #[test]
    fn non_posix_hosts_fail_instead_of_emitting_a_posix_facade() {
        let error=super::require_supported_host(false).unwrap_err();
        assert_eq!(error,super::BindError::Source("persistent PHP bindings require a POSIX host process supervisor".into()));
    }
}
