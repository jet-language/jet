//! Persistent PowerShell 7 object-pipeline binder (D-FFI-PWSH1=A).

use std::io::Read;
use std::path::{Path,PathBuf};
use std::process::{Command,Stdio};
use std::time::{Duration,Instant};

#[derive(Debug,Clone,PartialEq,Eq)]
pub struct BindResult { pub source:String,pub bound:Vec<String>,pub archive:PathBuf,pub provenance:String }
#[derive(Debug,Clone,PartialEq,Eq)]
pub enum BindError { Source(String),ToolMissing(&'static str),ToolFailed(&'static str,String),IO(String) }
impl std::fmt::Display for BindError { fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result { match self { Self::Source(v)|Self::IO(v)=>f.write_str(v),Self::ToolMissing(v)=>write!(f,"the provisioned `{v}` tool was not found"),Self::ToolFailed(t,v)=>write!(f,"`{t}` rejected the PowerShell binding input: {v}") } } }
#[derive(Clone)]struct BoundFunction{pwsh:String,jet:String}

pub fn bind(path:&Path,source:&str,lib:&str,cache:&Path)->Result<BindResult,BindError>{
    require_supported_host(cfg!(unix))?;
    if !ident(lib){return Err(BindError::Source(format!("`{lib}` is not a valid Jet library name")))}
    let pwsh=tool_path("pwsh").ok_or(BindError::ToolMissing("pwsh"))?;let script=std::fs::canonicalize(path).map_err(|e|BindError::IO(format!("could not resolve the PowerShell script: {e}")))?;
    std::fs::create_dir_all(cache).map_err(|e|BindError::IO(format!("could not create PowerShell binding cache: {e}")))?;let build=cache.join(format!(".pwsh-build-{lib}"));let _=std::fs::remove_dir_all(&build);std::fs::create_dir_all(&build).map_err(|e|BindError::IO(format!("could not create PowerShell build directory: {e}")))?;
    let validator=build.join("validate.ps1");std::fs::write(&validator,"$tokens = $null\n$errors = $null\n$ast = [System.Management.Automation.Language.Parser]::ParseFile($args[0], [ref]$tokens, [ref]$errors)\nif ($errors.Count -ne 0) { [Console]::Error.WriteLine('ParserError'); exit 2 }\n$ast.EndBlock.Statements | Where-Object { $_ -is [System.Management.Automation.Language.FunctionDefinitionAst] } | ForEach-Object { [Console]::Out.WriteLine($_.Name) }\n").map_err(|e|BindError::IO(format!("could not write PowerShell validator: {e}")))?;
    let discovered=run_capture(Command::new(&pwsh).args(["-NoLogo","-NoProfile","-NonInteractive","-File"]).arg(&validator).arg(&script),"pwsh")?;let functions=parse_function_names(&discovered)?;
    let worker=cache.join(format!("{lib}_worker.ps1"));std::fs::write(&worker,render_worker(&functions)).map_err(|e|BindError::IO(format!("could not write PowerShell worker: {e}")))?;let worker=std::fs::canonicalize(&worker).map_err(|e|BindError::IO(format!("could not resolve the PowerShell worker: {e}")))?;
    let c=build.join(format!("jet_pwsh_{lib}.c"));let object=build.join(format!("jet_pwsh_{lib}.o"));std::fs::write(&c,render_c(lib,&pwsh,&worker,&script,&functions)).map_err(|e|BindError::IO(format!("could not write PowerShell process bridge: {e}")))?;
    run(Command::new("cc").args(["-std=c11","-D_POSIX_C_SOURCE=200809L","-fPIC","-c"]).arg(&c).arg("-o").arg(&object),"cc")?;let archive=cache.join(format!("libjet_pwsh_{lib}.a"));let _=std::fs::remove_file(&archive);run(Command::new("ar").arg("rcs").arg(&archive).arg(&object),"ar")?;
    let mut identity=b"jet-pwsh-bind-v1\0".to_vec();identity.extend_from_slice(source.as_bytes());identity.push(0);identity.extend_from_slice(script.to_string_lossy().as_bytes());identity.push(0);identity.extend_from_slice(pwsh.to_string_lossy().as_bytes());identity.push(0);identity.extend_from_slice(render_worker(&functions).as_bytes());
    let result=BindResult{source:render_jet(lib,&functions),bound:functions.iter().map(|v|v.jet.clone()).collect(),archive,provenance:format!("schema=jet-pwsh-bind-v1\nsha256={}\npwsh={}\nscript={}\nworker={}\n",crate::SHA256::sha256_hex(&identity),pwsh.display(),script.display(),worker.display())};let _=std::fs::remove_dir_all(&build);Ok(result)
}

fn parse_function_names(bytes:&[u8])->Result<Vec<BoundFunction>,BindError>{let text=std::str::from_utf8(bytes).map_err(|_|BindError::Source("PowerShell returned non-UTF-8 function metadata".into()))?;let mut out=Vec::new();for line in text.lines(){let name=line.trim();if name.is_empty(){continue}if !powershell_ident(name){return Err(BindError::Source(format!("PowerShell function `{name}` cannot be projected as a Jet identifier")))}let jet=name.split('-').map(crate::CppBind::snake).collect::<Vec<_>>().join("_");if reserved_jet_function(&jet){return Err(BindError::Source(format!("PowerShell function `{name}` projects to reserved Jet name `{jet}`")))}if out.iter().any(|v:&BoundFunction|v.jet.eq_ignore_ascii_case(&jet)){return Err(BindError::Source(format!("PowerShell function `{name}` collides with another generated Jet function `{jet}`")))}out.push(BoundFunction{pwsh:name.to_string(),jet});}if out.is_empty(){return Err(BindError::Source("no top-level PowerShell functions were found".into()))}Ok(out)}

fn render_worker(functions:&[BoundFunction])->String{let allowed=functions.iter().map(|v|format!("'{}'",v.pwsh.replace('\'',"''"))).collect::<Vec<_>>().join(", ");format!(r#"$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$inputStream = [Console]::OpenStandardInput()
$outputStream = [Console]::OpenStandardOutput()
$utf8 = [System.Text.UTF8Encoding]::new($false)
$allowed = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
@({allowed}) | ForEach-Object {{ [void]$allowed.Add($_) }}
$null = . $args[0] 2>$null 3>$null 4>$null 5>$null 6>$null
$ready = $utf8.GetBytes('READY')
$outputStream.Write([BitConverter]::GetBytes([int]$ready.Length), 0, 4)
$outputStream.Write($ready, 0, $ready.Length)
$outputStream.Flush()

function Read-Exact([byte[]]$buffer, [int]$count) {{
  $offset = 0
  while ($offset -lt $count) {{
    $read = $inputStream.Read($buffer, $offset, $count - $offset)
    if ($read -eq 0) {{ return $false }}
    $offset += $read
  }}
  return $true
}}

while ($true) {{
  $header = [byte[]]::new(4)
  if (-not (Read-Exact $header 4)) {{ break }}
  $length = [BitConverter]::ToInt32($header, 0)
  if ($length -lt 1 -or $length -gt 1048576) {{ break }}
  $payload = [byte[]]::new($length)
  if (-not (Read-Exact $payload $length)) {{ break }}
  $request = $null
  try {{
    $request = $utf8.GetString($payload) | ConvertFrom-JSON -AsHashtable -Depth 64 2>$null 3>$null 4>$null 5>$null 6>$null
    if ($request.op -eq 'shutdown') {{ break }}
    if ($request.op -ne 'invoke' -or -not $allowed.Contains([string]$request.command)) {{ throw 'rejected command identity' }}
    $value = & ([string]$request.command) $request.input 2>$null 3>$null 4>$null 5>$null 6>$null
    $response = [ordered]@{{ id = $request.id; ok = $true; value = $value }}
  }} catch {{
    $response = [ordered]@{{ id = if ($null -ne $request) {{ $request.id }} else {{ 0 }}; ok = $false; code = 'CommandFailed'; value = $null }}
  }}
  $json = $response | ConvertTo-JSON -Depth 64 -Compress 2>$null 3>$null 4>$null 5>$null 6>$null
  $bytes = $utf8.GetBytes($json)
  if ($bytes.Length -gt 1048576) {{ break }}
  $outputStream.Write([BitConverter]::GetBytes([int]$bytes.Length), 0, 4)
  $outputStream.Write([BitConverter]::GetBytes([int64]$response.id), 0, 8)
  $outputStream.Write($bytes, 0, $bytes.Length)
  $outputStream.Flush()
}}
"#)}

fn render_jet(lib:&str,functions:&[BoundFunction])->String{let abi=format!("jet_pwsh_{lib}");let mut out=format!("#Extern module c.{abi} {{\n    fn open() => Int = \"{abi}_open\"\n    fn take_error() => Int = \"{abi}_take_error\"\n    fn cancel(handle: Int) = \"{abi}_cancel\"\n    fn close(handle: Int) = \"{abi}_close\"\n");for f in functions{out.push_str(&format!("    fn {}(handle: Int, input: String, deadline_ms: Int) => String = \"{abi}_invoke_{}\"\n",f.jet,f.jet));}out.push_str(&format!("}}\nuse c.{abi} as abi\nuse core.encoding.json as json\n\npub struct Session {{ value: Int }}\npub enum PowerShellError {{ NotRunning Timeout Cancelled Protocol CommandFailed Limit }}\n\nimpl Session.Close {{\n    fn close(^self) {{ abi.close(self.value) }}\n}}\n\npub fn close(^session: Session) {{}}\n\npub fn open() => Session ? PowerShellError {{\n    handle :: abi.open()\n    if abi.take_error() != 0 {{ return Err(PowerShellError.NotRunning) }}\n    return Ok(Session.{{ value: handle }})\n}}\n\npub fn cancel(session: Session) {{ abi.cancel(session.value) }}\n\n"));for f in functions{out.push_str(&format!("pub fn {}(session: Session, input: DataTree, deadline_ms: Int) => DataTree ? PowerShellError {{\n    raw :: abi.{}(session.value, json.to_string(input), deadline_ms)\n    code :: abi.take_error()\n    if code == 1 {{ return Err(PowerShellError.NotRunning) }}\n    if code == 2 {{ return Err(PowerShellError.Timeout) }}\n    if code == 3 {{ return Err(PowerShellError.Cancelled) }}\n    if code == 5 {{ return Err(PowerShellError.Limit) }}\n    if code != 0 {{ return Err(PowerShellError.Protocol) }}\n    response := json.parse(raw) ?? return Err(PowerShellError.Protocol)\n    succeeded := (response.field(\"ok\") ?? DataTree.Bool(false)).bool() ?? false\n    if !succeeded {{ return Err(PowerShellError.CommandFailed) }}\n    return Ok(response.field(\"value\") ?? DataTree.Null)\n}}\n\n",f.jet,f.jet));}out}

fn render_c(lib:&str,pwsh:&Path,worker:&Path,script:&Path,functions:&[BoundFunction])->String{let abi=format!("jet_pwsh_{lib}");let mut wrappers=String::new();for f in functions{wrappers.push_str(&format!("const char* {abi}_invoke_{}(int64_t h,const char*input,int64_t deadline){{return invoke(h,\"{}\",input,deadline);}}\n",f.jet,f.pwsh));}render_supervisor_c(&abi,pwsh,worker,script,&wrappers,"\"-NoLogo\",\"-NoProfile\",\"-NonInteractive\",\"-File\",worker_path,script_path")}

/// Shared bounded process/wire supervisor. Language binders still own worker
/// semantics; this only centralizes the audited framing, deadline, handle,
/// cancellation, and child-reaping machinery.
pub(crate) fn render_supervisor_c(abi:&str,executable:&Path,worker:&Path,script:&Path,wrappers:&str,exec_args:&str)->String{render_supervisor_c_with_temp(abi,executable,worker,script,wrappers,exec_args,None)}

/// Variant for workers needing a private scratch directory. The supervisor
/// creates it after fork, exports it as `JET_BIND_TEMP`, and removes its one
/// declared artifact on every reap path, including timeout and cancellation.
pub(crate) fn render_supervisor_c_with_temp(abi:&str,executable:&Path,worker:&Path,script:&Path,wrappers:&str,exec_args:&str,temp_prefix:Option<&str>)->String{let temp_prefix=temp_prefix.unwrap_or("");format!(r#"#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#define LIMIT 1048576
#define SLOTS 32
typedef struct {{pid_t pid;int input;int output;uint32_t generation;int cancelled;pthread_mutex_t io;}} Slot;
static Slot slots[SLOTS];static pthread_mutex_t lock=PTHREAD_MUTEX_INITIALIZER;static pthread_once_t once=PTHREAD_ONCE_INIT;static _Thread_local int64_t failed;static _Thread_local char result[LIMIT+1];static uint64_t request_id;
static const char executable_path[]="{}";static const char worker_path[]="{}";static const char script_path[]="{}";static const char temp_prefix[]="{}";
static int64_t now_ms(void){{struct timespec t;if(clock_gettime(CLOCK_MONOTONIC,&t)!=0)return 0;return (int64_t)t.tv_sec*1000+t.tv_nsec/1000000;}}
static void init(void){{for(int i=0;i<SLOTS;i++)pthread_mutex_init(&slots[i].io,0);}}
static int wait_fd(int fd,short events,int64_t end){{for(;;){{int64_t left=end-now_ms();if(left<=0)return 0;struct pollfd p={{fd,events,0}};int n=poll(&p,1,left>2147483647?2147483647:(int)left);if(n>0)return (p.revents&events)!=0?1:-1;if(n==0)return 0;if(errno!=EINTR)return -1;}}}}
static ssize_t pipe_write(int fd,const void*b,size_t n){{sigset_t block,old,pending;sigemptyset(&block);sigaddset(&block,SIGPIPE);if(pthread_sigmask(SIG_BLOCK,&block,&old)!=0){{errno=EIO;return -1;}}sigpending(&pending);int had=sigismember(&pending,SIGPIPE);ssize_t wrote=write(fd,b,n);int saved=errno;if(wrote<0&&saved==EPIPE&&!had){{struct timespec zero={{0,0}};while(sigtimedwait(&block,0,&zero)<0&&errno==EINTR){{}}}}pthread_sigmask(SIG_SETMASK,&old,0);errno=saved;return wrote;}}
static int write_all(int fd,const unsigned char*b,size_t n,int64_t end){{size_t at=0;while(at<n){{int ready=wait_fd(fd,POLLOUT,end);if(ready<=0)return ready;ssize_t wrote=pipe_write(fd,b+at,n-at);if(wrote>0)at+=(size_t)wrote;else if(wrote==0||(wrote<0&&errno!=EINTR))return -1;}}return 1;}}
static int read_all(int fd,unsigned char*b,size_t n,int64_t end){{size_t at=0;while(at<n){{int ready=wait_fd(fd,POLLIN,end);if(ready<=0)return ready;ssize_t got=read(fd,b+at,n-at);if(got>0)at+=(size_t)got;else if(got==0)return -1;else if(errno!=EINTR)return -1;}}return 1;}}
static void temp_path(pid_t pid,char*out,size_t cap){{if(temp_prefix[0])snprintf(out,cap,"%s%ld",temp_prefix,(long)pid);else out[0]=0;}}
static void cleanup_temp(pid_t pid){{char dir[256],file[272];temp_path(pid,dir,sizeof(dir));if(!dir[0])return;snprintf(file,sizeof(file),"%s/plot.svg",dir);unlink(file);rmdir(dir);}}
static void reap(pid_t pid){{if(pid>0){{if(kill(-pid,SIGKILL)<0&&errno==ESRCH)kill(pid,SIGKILL);while(waitpid(pid,0,0)<0&&errno==EINTR){{}}cleanup_temp(pid);}}}}
static void clear_pid(pid_t pid){{pthread_mutex_lock(&lock);for(int i=0;i<SLOTS;i++)if(slots[i].pid==pid){{if(slots[i].input>=0)close(slots[i].input);if(slots[i].output>=0)close(slots[i].output);slots[i].pid=0;slots[i].input=-1;slots[i].output=-1;slots[i].cancelled=0;}}pthread_mutex_unlock(&lock);}}
static void finish(void){{for(int i=0;i<SLOTS;i++){{pthread_mutex_lock(&lock);pid_t pid=slots[i].pid;pthread_mutex_unlock(&lock);if(pid>0){{reap(pid);clear_pid(pid);}}}}}}
static void finish_once(void){{init();atexit(finish);}}
static int spawn_worker(pid_t*pid,int*input,int*output){{int to[2],from[2];if(pipe(to))return 0;if(pipe(from)){{close(to[0]);close(to[1]);return 0;}}pid_t child=fork();if(child<0){{close(to[0]);close(to[1]);close(from[0]);close(from[1]);return 0;}}if(child==0){{setpgid(0,0);if(temp_prefix[0]){{char dir[256];temp_path(getpid(),dir,sizeof(dir));if(!dir[0]||mkdir(dir,0700)!=0||setenv("JET_BIND_TEMP",dir,1)!=0)_exit(127);}}dup2(to[0],0);dup2(from[1],1);int sink=open("/dev/null",O_WRONLY);if(sink>=0)dup2(sink,2);close(to[0]);close(to[1]);close(from[0]);close(from[1]);execl(executable_path,executable_path,{exec_args},(char*)0);_exit(127);}}setpgid(child,child);close(to[0]);close(from[1]);int64_t end=now_ms()+5000;unsigned char header[4],ready[5];int ok=read_all(from[0],header,4,end);if(ok>0){{if(header[0]==5&&header[1]==0&&header[2]==0&&header[3]==0)ok=read_all(from[0],ready,5,end);else ok=-1;}}if(ok<=0||memcmp(ready,"READY",5)!=0){{close(to[1]);close(from[0]);reap(child);return 0;}}*pid=child;*input=to[1];*output=from[0];return 1;}}
static int snapshot(int64_t h,int*index,pid_t*pid,int*input,int*output){{int idx=(int)(h&255)-1;uint32_t gen=(uint32_t)((uint64_t)h>>8);if(idx<0||idx>=SLOTS)return 0;pthread_mutex_lock(&lock);Slot*s=&slots[idx];int ok=s->pid>0&&s->generation==gen;if(ok){{*index=idx;*pid=s->pid;*input=s->input;*output=s->output;}}pthread_mutex_unlock(&lock);return ok;}}
int64_t {abi}_take_error(void){{int64_t value=failed;failed=0;return value;}}
int64_t {abi}_open(void){{pthread_once(&once,finish_once);failed=0;pid_t pid;int input,output;if(!spawn_worker(&pid,&input,&output)){{failed=1;return 0;}}pthread_mutex_lock(&lock);for(int i=0;i<SLOTS;i++)if(slots[i].pid==0){{slots[i].generation++;if(slots[i].generation==0)slots[i].generation=1;slots[i].pid=pid;slots[i].input=input;slots[i].output=output;uint64_t h=((uint64_t)slots[i].generation<<8)|(uint64_t)(i+1);pthread_mutex_unlock(&lock);return (int64_t)h;}}pthread_mutex_unlock(&lock);reap(pid);close(input);close(output);failed=1;return 0;}}
static const char* invoke(int64_t h,const char*command,const char*json,int64_t deadline){{failed=0;result[0]=0;if(!json){{failed=4;return result;}}size_t json_n=strlen(json);if(json_n>LIMIT-256){{failed=5;return result;}}if(deadline<1||deadline>300000){{failed=2;return result;}}int idx,input,output;pid_t pid;if(!snapshot(h,&idx,&pid,&input,&output)){{failed=1;return result;}}pthread_mutex_lock(&slots[idx].io);if(!snapshot(h,&idx,&pid,&input,&output)){{pthread_mutex_unlock(&slots[idx].io);failed=1;return result;}}uint64_t id=__atomic_add_fetch(&request_id,1,__ATOMIC_RELAXED);char*request=malloc(json_n+256);if(!request){{pthread_mutex_unlock(&slots[idx].io);failed=5;return result;}}int n=snprintf(request,json_n+256,"{{\"id\":%llu,\"op\":\"invoke\",\"command\":\"%s\",\"input\":%s}}",(unsigned long long)id,command,json);if(n<1||(size_t)n>LIMIT){{free(request);pthread_mutex_unlock(&slots[idx].io);failed=5;return result;}}unsigned char header[4]={{(unsigned char)n,(unsigned char)(n>>8),(unsigned char)(n>>16),(unsigned char)(n>>24)}};int64_t end=now_ms()+deadline;int io=write_all(input,header,4,end);if(io>0)io=write_all(input,(unsigned char*)request,(size_t)n,end);free(request);unsigned char response_header[4],response_id[8];if(io>0)io=read_all(output,response_header,4,end);uint32_t size=0;if(io>0){{size=(uint32_t)response_header[0]|((uint32_t)response_header[1]<<8)|((uint32_t)response_header[2]<<16)|((uint32_t)response_header[3]<<24);if(size>LIMIT)io=-2;}}if(io>0)io=read_all(output,response_id,8,end);if(io>0){{uint64_t received=0;for(int b=0;b<8;b++)received|=(uint64_t)response_id[b]<<(8*b);if(received!=id)io=-3;}}if(io>0)io=read_all(output,(unsigned char*)result,size,end);if(io>0)result[size]=0;if(io<=0){{pthread_mutex_lock(&lock);int cancelled=slots[idx].pid==pid&&slots[idx].cancelled;pthread_mutex_unlock(&lock);reap(pid);clear_pid(pid);failed=cancelled?3:(io==0?2:(io==-2?5:4));result[0]=0;}}pthread_mutex_unlock(&slots[idx].io);return result;}}
void {abi}_cancel(int64_t h){{failed=0;int idx,input,output;pid_t pid;if(!snapshot(h,&idx,&pid,&input,&output)){{failed=1;return;}}pthread_mutex_lock(&lock);if(slots[idx].pid==pid)slots[idx].cancelled=1;pthread_mutex_unlock(&lock);if(kill(-pid,SIGKILL)<0&&errno==ESRCH)kill(pid,SIGKILL);}}
void {abi}_close(int64_t h){{failed=0;int idx,input,output;pid_t pid;if(!snapshot(h,&idx,&pid,&input,&output)){{failed=1;return;}}pthread_mutex_lock(&slots[idx].io);if(snapshot(h,&idx,&pid,&input,&output)){{unsigned char frame[21]={{17,0,0,0,'{{','\"','o','p','\"',':','\"','s','h','u','t','d','o','w','n','\"','}}'}};int64_t end=now_ms()+250;write_all(input,frame,sizeof(frame),end);reap(pid);clear_pid(pid);}}pthread_mutex_unlock(&slots[idx].io);}}
{wrappers}"#,c_escape(&executable.to_string_lossy()),c_escape(&worker.to_string_lossy()),c_escape(&script.to_string_lossy()),c_escape(temp_prefix))}

fn tool_path(tool:&str)->Option<PathBuf>{let path=std::env::var_os("PATH")?;std::env::split_paths(&path).map(|v|v.join(tool)).find(|v|v.is_file()).and_then(|v|std::fs::canonicalize(v).ok())}
fn run_capture(command:&mut Command,tool:&'static str)->Result<Vec<u8>,BindError>{const CAP:usize=64*1024;command.stdout(Stdio::piped()).stderr(Stdio::piped());let mut child=command.spawn().map_err(|e|if e.kind()==std::io::ErrorKind::NotFound{BindError::ToolMissing(tool)}else{BindError::IO(format!("could not start `{tool}`: {e}"))})?;let stdout=child.stdout.take().ok_or_else(||BindError::IO(format!("could not supervise `{tool}` stdout")))?;let stderr=child.stderr.take().ok_or_else(||BindError::IO(format!("could not supervise `{tool}` stderr")))?;let out=std::thread::spawn(move||drain(stdout,CAP));let err=std::thread::spawn(move||drain(stderr,CAP));let deadline=Instant::now()+Duration::from_secs(60);let status=loop{match child.try_wait().map_err(|e|BindError::IO(format!("could not supervise `{tool}`: {e}")))?{Some(v)=>break v,None if Instant::now()>=deadline=>{let _=child.kill();let _=child.wait();let _=out.join();let _=err.join();return Err(BindError::ToolFailed(tool,"the tool exceeded the 60 second limit".into()))},None=>std::thread::sleep(Duration::from_millis(10))}};let stdout=out.join().map_err(|_|BindError::IO(format!("`{tool}` stdout reader failed")))??;let stderr=err.join().map_err(|_|BindError::IO(format!("`{tool}` stderr reader failed")))??;if status.success(){Ok(stdout)}else{Err(BindError::ToolFailed(tool,launder(&stderr)))}}
fn run(command:&mut Command,tool:&'static str)->Result<(),BindError>{const CAP:usize=64*1024;command.stdout(Stdio::piped()).stderr(Stdio::piped());let mut child=command.spawn().map_err(|e|if e.kind()==std::io::ErrorKind::NotFound{BindError::ToolMissing(tool)}else{BindError::IO(format!("could not start `{tool}`: {e}"))})?;let stdout=child.stdout.take().ok_or_else(||BindError::IO(format!("could not supervise `{tool}` stdout")))?;let stderr=child.stderr.take().ok_or_else(||BindError::IO(format!("could not supervise `{tool}` stderr")))?;let out=std::thread::spawn(move||drain(stdout,CAP));let err=std::thread::spawn(move||drain(stderr,CAP));let deadline=Instant::now()+Duration::from_secs(60);let status=loop{match child.try_wait().map_err(|e|BindError::IO(format!("could not supervise `{tool}`: {e}")))?{Some(v)=>break v,None if Instant::now()>=deadline=>{let _=child.kill();let _=child.wait();let _=out.join();let _=err.join();return Err(BindError::ToolFailed(tool,"the tool exceeded the 60 second limit".into()))},None=>std::thread::sleep(Duration::from_millis(10))}};let stdout=out.join().map_err(|_|BindError::IO(format!("`{tool}` stdout reader failed")))??;let stderr=err.join().map_err(|_|BindError::IO(format!("`{tool}` stderr reader failed")))??;if status.success(){Ok(())}else{let detail=if stderr.is_empty(){&stdout}else{&stderr};Err(BindError::ToolFailed(tool,launder(detail)))}}
fn drain(mut input:impl Read,limit:usize)->Result<Vec<u8>,BindError>{let mut out=Vec::new();let mut buf=[0u8;8192];loop{let n=input.read(&mut buf).map_err(|e|BindError::IO(format!("could not read foreign tool output: {e}")))?;if n==0{break}let keep=(limit-out.len()).min(n);out.extend_from_slice(&buf[..keep]);}Ok(out)}
fn launder(v:&[u8])->String{let text=String::from_utf8_lossy(v);if text.contains("ParserError"){return "the script has a PowerShell parse error".into()}text.lines().map(str::trim).find(|v|!v.is_empty()).map(|v|v.chars().take(160).collect()).unwrap_or_else(||"the foreign tool returned a failure status".into())}
fn c_escape(v:&str)->String{let mut out=String::new();for b in v.bytes(){match b{b'\\'=>out.push_str("\\\\"),b'"'=>out.push_str("\\\""),b'\n'=>out.push_str("\\n"),b'\r'=>out.push_str("\\r"),b'\t'=>out.push_str("\\t"),0x20..=0x7e=>out.push(b as char),_=>out.push_str(&format!("\\{:03o}",b))}}out}
fn ident(v:&str)->bool{let mut chars=v.chars();matches!(chars.next(),Some(c)if c.is_ascii_alphabetic()||c=='_')&&chars.all(|c|c.is_ascii_alphanumeric()||c=='_')}
fn powershell_ident(v:&str)->bool{let mut chars=v.chars();matches!(chars.next(),Some(c)if c.is_ascii_alphabetic()||c=='_')&&chars.all(|c|c.is_ascii_alphanumeric()||c=='_'||c=='-')}
fn reserved_jet_function(v:&str)->bool{matches!(v,"open"|"take_error"|"cancel"|"close"|"abi"|"Session"|"PowerShellError")||crate::Syntax::JET_KEYWORD_LIST.contains(&v)||crate::Syntax::JET_TYPE_LIST.contains(&v)}
fn require_supported_host(unix:bool)->Result<(),BindError>{if unix{Ok(())}else{Err(BindError::Source("persistent PowerShell bindings require a POSIX host process supervisor".into()))}}

#[cfg(test)]
mod tests{
    #[test]
    fn projects_powershell_names_without_changing_foreign_lookup(){
        let functions=super::parse_function_names(b"Get-Stateful\nFail\n").unwrap();
        assert_eq!(functions.iter().map(|v|v.jet.as_str()).collect::<Vec<_>>(),["get_stateful","fail"]);
        let jet=super::render_jet("ops",&functions);
        let worker=super::render_worker(&functions);
        assert!(jet.contains("pub fn get_stateful("));
        assert!(worker.contains("'Get-Stateful'"));
    }

    #[test]
    fn rejects_generated_powershell_helper_and_alias_collisions(){
        for (name,jet) in [("Take-Error","take_error"),("ABI","abi")]{
            let Err(error)=super::parse_function_names(name.as_bytes())else{panic!("generated PowerShell name collision was accepted")};
            assert_eq!(error,super::BindError::Source(format!("PowerShell function `{name}` projects to reserved Jet name `{jet}`")));
        }
    }

    #[test]
    fn non_posix_hosts_fail_instead_of_emitting_a_posix_facade(){let error=super::require_supported_host(false).unwrap_err();assert_eq!(error,super::BindError::Source("persistent PowerShell bindings require a POSIX host process supervisor".into()));}
}
