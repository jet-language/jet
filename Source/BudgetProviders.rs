//! D-PERFBUDGET-PROVIDER1: compiler-owned measurement-provider registry.
//!
//! Registry keys, executable paths, and response files are resolved by the
//! compiler. No provider lookup consults `PATH`. Every transport feeds the
//! same binary decoder and limit checker before evidence reaches evaluation.

use jet_foundation::PerformanceBudget::{CanonicalJson, Rational};
use jet_foundation::PerformanceBudget::{Comparison, Direction, Enforcement, Evaluation, MeasurementPolicy, Percentile};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, atomic::{AtomicBool, Ordering as AtomicOrdering}};
use std::time::{Duration, Instant};

pub const MAX_SAMPLES: usize = 1_000_000;
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_SPECS: usize = 4_096;
pub const MAX_DETAIL_SCALARS: usize = 512;
const MAGIC: &[u8] = b"JETBUDGET1\n";
#[cfg(test)]static FILE_READER_DELAY_MS:AtomicU64=AtomicU64::new(0);
#[cfg(test)]static ACTIVE_FILE_READERS:AtomicU64=AtomicU64::new(0);
#[cfg(test)]static LAST_ISOLATED_GROUP:AtomicU64=AtomicU64::new(0);
#[cfg(test)]use std::sync::atomic::AtomicU64;
#[cfg(test)]struct ActiveFileReader;
#[cfg(test)]impl ActiveFileReader{fn new()->Self{ACTIVE_FILE_READERS.fetch_add(1,AtomicOrdering::SeqCst);Self}}
#[cfg(test)]impl Drop for ActiveFileReader{fn drop(&mut self){ACTIVE_FILE_READERS.fetch_sub(1,AtomicOrdering::SeqCst);}}
#[cfg(not(test))]struct ActiveFileReader;
#[cfg(not(test))]impl ActiveFileReader{fn new()->Self{Self}}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderSpec {
    pub budget_hash: String,
    pub metric: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderRequest {
    pub schema: String,
    pub version: u32,
    pub request_id: String,
    pub provider_hash: String,
    pub context_hash: String,
    pub specs: Vec<ProviderSpec>,
    pub workload: CanonicalJson,
    pub policy: CanonicalJson,
}

impl ProviderRequest {
    pub fn validate(&self) -> Result<(), ProviderFailure> {
        if self.schema != "jet.provider-request" || self.version != 1 {
            return Err(ProviderFailure::malformed("unsupported provider request schema/version"));
        }
        for (name, value) in [("request_id", &self.request_id), ("provider_hash", &self.provider_hash), ("context_hash", &self.context_hash)] {
            if !is_hex64(value) { return Err(ProviderFailure::malformed(format!("{name} is not lowercase Hex64"))); }
        }
        if self.specs.is_empty() || self.specs.len() > MAX_SPECS { return Err(ProviderFailure::malformed("provider request spec count is outside 1..=4096")); }
        let mut previous: Option<(&str, &str)> = None;
        for spec in &self.specs {
            if !is_hex64(&spec.budget_hash) || spec.metric.is_empty() { return Err(ProviderFailure::malformed("provider request has an invalid budget hash or empty metric")); }
            let key = (spec.metric.as_str(), spec.budget_hash.as_str());
            if previous.is_some_and(|prior| prior >= key) { return Err(ProviderFailure::malformed("provider request specs are not strictly ordered by metric then budget hash")); }
            previous = Some(key);
        }
        Ok(())
    }

    pub fn bytes(&self) -> Result<Vec<u8>, ProviderFailure> {
        self.validate()?;
        let specs = self.specs.iter().map(|spec| CanonicalJson::object([
            ("budget_hash".into(), CanonicalJson::String(spec.budget_hash.clone())),
            ("metric".into(), CanonicalJson::String(spec.metric.clone())),
        ]).expect("fixed keys")).collect();
        Ok(CanonicalJson::object([
            ("context_hash".into(), CanonicalJson::String(self.context_hash.clone())),
            ("policy".into(), self.policy.clone()),
            ("provider_hash".into(), CanonicalJson::String(self.provider_hash.clone())),
            ("request_id".into(), CanonicalJson::String(self.request_id.clone())),
            ("schema".into(), CanonicalJson::String(self.schema.clone())),
            ("specs".into(), CanonicalJson::Array(specs)),
            ("version".into(), CanonicalJson::Integer(self.version.to_string())),
            ("workload".into(), self.workload.clone()),
        ]).expect("fixed keys").bytes())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderEvent {
    Sample { spec: u32, metric: String, value: Rational },
    Unavailable { spec: u32, reason: String, details: Vec<(String, String)> },
    Complete { request_id: String, samples: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderEvidence { pub events: Vec<ProviderEvent> }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureClass { Unavailable, Malformed, Panic, Timeout, Execution, Incompatible, Unsupported, Unresolved }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderFailure { pub class: FailureClass, pub reason: String }
impl ProviderFailure {
    pub fn malformed(reason: impl Into<String>) -> Self { Self { class: FailureClass::Malformed, reason: reason.into() } }
    fn operation(class: FailureClass, reason: impl Into<String>) -> Self { Self { class, reason: reason.into() } }
    pub fn diagnostic(&self, budget: &str) -> ProviderDiagnostic {
        match self.class {
            FailureClass::Unavailable | FailureClass::Incompatible => ProviderDiagnostic { code: "E2906", what: format!("performance budget {budget} has no usable evidence"), why: self.reason.clone(), fix: "correct the provider evidence or bootstrap only when absent or stale evidence is eligible".into() },
            FailureClass::Unsupported => ProviderDiagnostic { code: "E2903", what: format!("performance budget {budget} is not valid"), why: self.reason.clone(), fix: "use one supported metric and provider pair".into() },
            FailureClass::Unresolved => ProviderDiagnostic { code: "E2905", what: format!("performance budget {budget} cannot resolve provider"), why: self.reason.clone(), fix: "name one registered provider identity".into() },
            _ => ProviderDiagnostic { code: "E2908", what: "performance budget operation failed".into(), why: format!("measurement provider refused the operation: {}", self.reason), fix: "correct the named provider failure and retry the operation".into() },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderDiagnostic { pub code: &'static str, pub what: String, pub why: String, pub fix: String }
impl ProviderDiagnostic { pub fn render(&self)->String{format!("# Error [{}]: {}\n\nWhat: {}\nWhy: {}\nFix: {}\n",self.code,self.what,self.what,self.why,self.fix)} }

#[derive(Clone)]
pub struct ProviderCancellation { cancelled: Arc<AtomicBool> }
impl ProviderCancellation { pub fn cancelled(&self)->bool{self.cancelled.load(AtomicOrdering::Acquire)} }
type InProcessProvider = fn(&ProviderRequest,&ProviderCancellation) -> Result<Vec<ProviderEvent>, ProviderFailure>;
#[derive(Clone)]
enum Provider { InProcess(InProcessProvider), Subprocess(PathBuf), File(PathBuf) }

#[derive(Default)]
pub struct ProviderRegistry { providers: BTreeMap<String, Provider> }
impl ProviderRegistry {
    /// Registry used by `jet budget` for compiler-owned deterministic facts.
    /// Values travel through the same typed request/stream validation as every
    /// other provider; the registry never consults PATH.
    pub fn with_compiler_facts() -> Self {
        let mut registry = Self::default();
        registry.register_in_process("CompilerFacts", compiler_facts_provider)
            .expect("fixed compiler provider identity");
        registry
    }
    pub fn with_builtins() -> Self {
        let mut registry = Self::with_compiler_facts();
        registry.register_in_process("BuildArtifact", build_artifact_provider)
            .expect("fixed build-artifact provider identity");
        registry
    }
    /// Provider runs in an isolated process group; collection terminates the
    /// entire group at the deadline without waiting on a blocked worker.
    pub fn register_in_process(&mut self, identity: impl Into<String>, provider: InProcessProvider) -> Result<(), String> { self.insert(identity.into(), Provider::InProcess(provider)) }
    pub fn register_subprocess(&mut self, identity: impl Into<String>, executable: PathBuf) -> Result<(), String> {
        if !executable.is_absolute() { return Err("provider executable must be an absolute compiler-resolved path".into()); }
        self.insert(identity.into(), Provider::Subprocess(executable))
    }
    pub fn register_file(&mut self, identity: impl Into<String>, response: PathBuf) -> Result<(), String> {
        if !response.is_absolute() { return Err("provider response must be an absolute compiler-resolved path".into()); }
        self.insert(identity.into(), Provider::File(response))
    }
    fn insert(&mut self, identity: String, provider: Provider) -> Result<(), String> {
        if identity.is_empty() { return Err("provider identity is empty".into()); }
        if self.providers.insert(identity.clone(), provider).is_some() { return Err(format!("duplicate provider identity `{identity}`")); }
        Ok(())
    }
    pub fn collect(&self, identity: &str, request: &ProviderRequest, timeout: Duration) -> Result<ProviderEvidence, ProviderFailure> {
        request.validate()?;
        let provider = self.providers.get(identity).ok_or_else(|| ProviderFailure::operation(FailureClass::Unresolved, format!("provider `{identity}` is unresolved")))?;
        let events = match provider {
            Provider::InProcess(function) => run_in_process(*function, request, timeout, identity)?,
            Provider::Subprocess(path) => decode_stream(&run_subprocess(path, &request.bytes()?, timeout)?, request)?,
            Provider::File(path) => decode_stream(&read_bounded(path, timeout)?, request)?,
        };
        validate_events(events, request)
    }
}

fn compiler_facts_provider(request: &ProviderRequest, _: &ProviderCancellation) -> Result<Vec<ProviderEvent>, ProviderFailure> {
    let CanonicalJson::Array(values) = &request.workload else {
        return Err(ProviderFailure::malformed("CompilerFacts workload is not an ordered sample array"));
    };
    if values.len() != request.specs.len() {
        return Err(ProviderFailure::malformed("CompilerFacts workload/spec count differs"));
    }
    let mut events = Vec::with_capacity(values.len() + 1);
    for (index, value) in values.iter().enumerate() {
        let CanonicalJson::Integer(value) = value else {
            return Err(ProviderFailure::malformed("CompilerFacts sample is not an integer"));
        };
        let value = Rational::parse(value, "1").map_err(ProviderFailure::malformed)?;
        events.push(ProviderEvent::Sample { spec: index as u32, metric: request.specs[index].metric.clone(), value });
    }
    events.push(ProviderEvent::Complete { request_id: request.request_id.clone(), samples: values.len() as u64 });
    Ok(events)
}

fn build_artifact_provider(request: &ProviderRequest, _: &ProviderCancellation) -> Result<Vec<ProviderEvent>, ProviderFailure> {
    let CanonicalJson::Object(workload) = &request.workload else {
        return Err(ProviderFailure::malformed("BuildArtifact workload is not an object"));
    };
    let Some(CanonicalJson::String(path)) = workload.get("path") else {
        return Err(ProviderFailure::malformed("BuildArtifact workload has no artifact path"));
    };
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err(ProviderFailure::malformed("BuildArtifact path is not compiler-resolved absolute text"));
    }
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| ProviderFailure::operation(FailureClass::Unavailable, format!("built artifact is unavailable: {error}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ProviderFailure::operation(FailureClass::Incompatible, "built artifact is not a regular file"));
    }
    let value = Rational::parse(&metadata.len().to_string(), "1").map_err(ProviderFailure::malformed)?;
    let mut events = Vec::with_capacity(request.specs.len() + 1);
    for (index, spec) in request.specs.iter().enumerate() {
        if !matches!(spec.metric.as_str(), "BinarySize" | "ArtifactSize") {
            return Err(ProviderFailure::operation(FailureClass::Unsupported, format!("BuildArtifact does not support metric `{}`", spec.metric)));
        }
        events.push(ProviderEvent::Sample { spec: index as u32, metric: spec.metric.clone(), value: value.clone() });
    }
    events.push(ProviderEvent::Complete { request_id: request.request_id.clone(), samples: request.specs.len() as u64 });
    Ok(events)
}

fn run_in_process(function: InProcessProvider, request: &ProviderRequest, timeout: Duration, identity: &str) -> Result<Vec<ProviderEvent>, ProviderFailure> {
    #[cfg(target_os="linux")]{let cancellation=ProviderCancellation{cancelled:Arc::new(AtomicBool::new(false))};let bytes=run_isolated_bytes(timeout,&format!("provider `{identity}`"),move||std::panic::catch_unwind(std::panic::AssertUnwindSafe(||function(request,&cancellation))).map_err(|_|ProviderFailure::operation(FailureClass::Panic,"in-process provider failed unexpectedly"))?.map(|events|encode_stream(&events)))?;decode_stream(&bytes,request)}
    #[cfg(not(target_os="linux"))]{let _=(function,request,timeout,identity);Err(ProviderFailure::operation(FailureClass::Execution,"bounded in-process providers are enabled only on Linux"))}
}

fn read_bounded(path: &Path, timeout: Duration) -> Result<Vec<u8>, ProviderFailure> {
    #[cfg(target_os="linux")]{read_bounded_isolated(path,timeout)}
    #[cfg(not(target_os="linux"))]{let _=(path,timeout);Err(ProviderFailure::operation(FailureClass::Execution,"isolated provider file reads are enabled only on Linux"))}
}

#[cfg(target_os="linux")]
fn read_bounded_isolated(path:&Path,timeout:Duration)->Result<Vec<u8>,ProviderFailure>{
    use std::ffi::CString;use std::os::unix::ffi::OsStrExt;
    let name=CString::new(path.as_os_str().as_bytes()).map_err(|_|ProviderFailure::operation(FailureClass::Execution,"provider response path contains NUL"))?;
    #[repr(C)]struct StatxTimestamp{sec:i64,nsec:u32,reserved:i32}#[repr(C)]struct Statx{mask:u32,blksize:u32,attributes:u64,nlink:u32,uid:u32,gid:u32,mode:u16,spare0:u16,ino:u64,size:u64,blocks:u64,attributes_mask:u64,atime:StatxTimestamp,btime:StatxTimestamp,ctime:StatxTimestamp,mtime:StatxTimestamp,rdev_major:u32,rdev_minor:u32,dev_major:u32,dev_minor:u32,mnt_id:u64,dio_mem_align:u32,dio_offset_align:u32,spare3:[u64;12]}
    const O_RDONLY:i32=0;const O_NONBLOCK:i32=0o4000;const O_CLOEXEC:i32=0o2000000;const O_NOFOLLOW:i32=0o400000;const AT_EMPTY_PATH:i32=0x1000;const STATX_TYPE:u32=1;const S_IFMT:u16=0o170000;const S_IFREG:u16=0o100000;
    extern "C"{fn open(path:*const i8,flags:i32,...)->i32;fn read(fd:i32,buffer:*mut u8,count:usize)->isize;fn close(fd:i32)->i32;fn statx(fd:i32,path:*const i8,flags:i32,mask:u32,stat:*mut Statx)->i32;#[cfg(test)]fn usleep(micros:u32)->i32;}
    run_isolated_bytes(timeout,&format!("provider response {}",path.display()),move||unsafe{#[cfg(test)]{let delay=FILE_READER_DELAY_MS.load(AtomicOrdering::Relaxed);if delay>0{usleep((delay.min(u32::MAX as u64)*1000)as u32);}}let fd=open(name.as_ptr(),O_RDONLY|O_NONBLOCK|O_CLOEXEC|O_NOFOLLOW);if fd<0{return Err(ProviderFailure::operation(FailureClass::Execution,"cannot open provider response without following links"))}let empty=b"\0";let mut info:Statx=std::mem::zeroed();if statx(fd,empty.as_ptr()as*const i8,AT_EMPTY_PATH,STATX_TYPE,&mut info)!=0||info.mode&S_IFMT!=S_IFREG{close(fd);return Err(ProviderFailure::operation(FailureClass::Execution,"provider response is not a regular file"))}let mut bytes=Vec::new();let mut buffer=[0u8;8192];loop{let count=read(fd,buffer.as_mut_ptr(),buffer.len());if count<0{close(fd);return Err(ProviderFailure::operation(FailureClass::Execution,"cannot read provider response"))}if count==0{break}bytes.extend_from_slice(&buffer[..count as usize]);if bytes.len()>MAX_BYTES{close(fd);return Err(ProviderFailure::malformed("provider stream exceeds 16 MiB"))}}close(fd);Ok(bytes)})
}

#[cfg(target_os="linux")]
fn run_isolated_bytes<F>(timeout:Duration,label:&str,work:F)->Result<Vec<u8>,ProviderFailure>where F:FnOnce()->Result<Vec<u8>,ProviderFailure>{
    const O_NONBLOCK:i32=0o4000;const F_SETFL:i32=4;const WNOHANG:i32=1;const SIGKILL:i32=9;
    extern "C"{fn pipe(fds:*mut i32)->i32;fn fork()->i32;fn close(fd:i32)->i32;fn read(fd:i32,buffer:*mut u8,count:usize)->isize;fn write(fd:i32,buffer:*const u8,count:usize)->isize;fn fcntl(fd:i32,command:i32,...)->i32;fn waitpid(pid:i32,status:*mut i32,options:i32)->i32;fn kill(pid:i32,signal:i32)->i32;fn setpgid(pid:i32,pgid:i32)->i32;fn _exit(status:i32)->!;}
    fn class_byte(class:FailureClass)->u8{match class{FailureClass::Unavailable=>0,FailureClass::Malformed=>1,FailureClass::Panic=>2,FailureClass::Timeout=>3,FailureClass::Execution=>4,FailureClass::Incompatible=>5,FailureClass::Unsupported=>6,FailureClass::Unresolved=>7}}
    fn byte_class(value:u8)->Option<FailureClass>{Some(match value{0=>FailureClass::Unavailable,1=>FailureClass::Malformed,2=>FailureClass::Panic,3=>FailureClass::Timeout,4=>FailureClass::Execution,5=>FailureClass::Incompatible,6=>FailureClass::Unsupported,7=>FailureClass::Unresolved,_=>return None})}
    let mut pipes=[-1,-1];if unsafe{pipe(pipes.as_mut_ptr())}!=0{return Err(ProviderFailure::operation(FailureClass::Execution,format!("cannot create isolated worker pipe: {}",std::io::Error::last_os_error())))}let pid=unsafe{fork()};if pid<0{unsafe{close(pipes[0]);close(pipes[1]);}return Err(ProviderFailure::operation(FailureClass::Execution,format!("cannot isolate {label}: {}",std::io::Error::last_os_error())))}
    if pid==0{unsafe{close(pipes[0]);setpgid(0,0);let worker=fork();if worker<0{close(pipes[1]);_exit(111)}if worker>0{close(pipes[1]);_exit(0)}close(2);let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).map_err(|_|ProviderFailure::operation(FailureClass::Panic,"isolated worker panicked")).and_then(|value|value);let frame=match result{Ok(bytes)=>{let mut frame=Vec::with_capacity(bytes.len()+1);frame.push(0);frame.extend_from_slice(&bytes);frame},Err(failure)=>{let mut frame=Vec::with_capacity(failure.reason.len()+2);frame.push(1);frame.push(class_byte(failure.class));frame.extend_from_slice(failure.reason.as_bytes());frame}};let mut at=0;while at<frame.len(){let sent=write(pipes[1],frame[at..].as_ptr(),frame.len()-at);if sent<=0{break}at+=sent as usize}close(pipes[1]);_exit(0)}}
    let _active=ActiveFileReader::new();unsafe{close(pipes[1]);setpgid(pid,pid);fcntl(pipes[0],F_SETFL,O_NONBLOCK);}#[cfg(test)]LAST_ISOLATED_GROUP.store(pid as u64,AtomicOrdering::SeqCst);let deadline=Instant::now()+timeout;let mut frame=Vec::new();let mut supervisor_reaped=false;loop{let mut buffer=[0u8;8192];let count=unsafe{read(pipes[0],buffer.as_mut_ptr(),buffer.len())};if count>0{frame.extend_from_slice(&buffer[..count as usize]);continue}if count==0{unsafe{close(pipes[0]);}break}if !supervisor_reaped{let mut status=0;let waited=unsafe{waitpid(pid,&mut status,WNOHANG)};if waited==pid{supervisor_reaped=true}}if Instant::now()>=deadline{unsafe{kill(-pid,SIGKILL);let mut status=0;waitpid(pid,&mut status,WNOHANG);close(pipes[0]);}return Err(ProviderFailure::operation(FailureClass::Timeout,format!("{label} timed out and its isolated process group was terminated")))}std::thread::sleep(Duration::from_millis(1));}if !supervisor_reaped{let mut status=0;unsafe{waitpid(pid,&mut status,WNOHANG);}}
    match frame.first().copied(){Some(0)=>Ok(frame[1..].to_vec()),Some(1)=>{let class=frame.get(1).copied().and_then(byte_class).ok_or_else(||ProviderFailure::operation(FailureClass::Execution,"isolated worker returned an invalid failure class"))?;let reason=String::from_utf8(frame[2..].to_vec()).map_err(|_|ProviderFailure::operation(FailureClass::Execution,"isolated worker returned non-UTF-8 failure text"))?;Err(ProviderFailure::operation(class,reason))},_=>Err(ProviderFailure::operation(FailureClass::Execution,format!("{label} exited without a result")))}
}

fn run_subprocess(path: &Path, request: &[u8], timeout: Duration) -> Result<Vec<u8>, ProviderFailure> {
    let mut command = Command::new(path);
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(unix)] { use std::os::unix::process::CommandExt; command.process_group(0); }
    let mut child = command.spawn().map_err(|error| ProviderFailure::operation(FailureClass::Execution, format!("cannot launch provider {}: {error}", path.display())))?;
    let mut stdin = child.stdin.take().ok_or_else(|| ProviderFailure::operation(FailureClass::Execution, "provider stdin was unavailable"))?; let request=request.to_vec();
    let (writer_tx,writer_rx)=mpsc::sync_channel(1);std::thread::spawn(move || {let _=writer_tx.send(stdin.write_all(&request));});let stdout=child.stdout.take().ok_or_else(|| ProviderFailure::operation(FailureClass::Execution, "provider stdout was unavailable"))?;
    let (tx,rx)=mpsc::channel();std::thread::spawn(move || { let mut bytes=Vec::new();let result=stdout.take((MAX_BYTES+1) as u64).read_to_end(&mut bytes).map(|_|bytes);let _=tx.send(result); });
    let deadline=Instant::now()+timeout;loop { match child.try_wait() { Ok(Some(status)) => { if !status.success(){terminate_group(&mut child);return Err(ProviderFailure::operation(FailureClass::Execution,format!("provider exited with {status}")));}let remaining=deadline.saturating_duration_since(Instant::now());let bytes=rx.recv_timeout(remaining).map_err(|_|{terminate_group(&mut child);ProviderFailure::operation(FailureClass::Timeout,"provider stdout did not close before deadline")})?.map_err(|e|ProviderFailure::operation(FailureClass::Execution,format!("cannot read provider stdout: {e}")))?;writer_rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).map_err(|_|{terminate_group(&mut child);ProviderFailure::operation(FailureClass::Timeout,"provider stdin did not close before deadline")})?.map_err(|e|ProviderFailure::operation(FailureClass::Execution,format!("cannot write provider stdin: {e}")))?;if bytes.len()>MAX_BYTES{return Err(ProviderFailure::malformed("provider stream exceeds 16 MiB"));}return Ok(bytes); },Ok(None) if Instant::now()<deadline=>std::thread::sleep(Duration::from_millis(2)),Ok(None)=>{terminate_group(&mut child);return Err(ProviderFailure::operation(FailureClass::Timeout,"provider timed out and was terminated"));},Err(error)=>{terminate_group(&mut child);return Err(ProviderFailure::operation(FailureClass::Execution,format!("cannot supervise provider: {error}")))} } }
}

#[doc(hidden)]
pub fn terminate_group(child: &mut std::process::Child) {
    #[cfg(unix)] { unsafe { extern "C" { fn kill(pid: i32, signal: i32) -> i32; } let _ = kill(-(child.id() as i32), 9); } }
    let _ = child.kill();
    let _ = child.wait();
}

pub fn encode_stream(events: &[ProviderEvent]) -> Vec<u8> { let mut out=MAGIC.to_vec();for event in events{match event{ProviderEvent::Sample{spec,metric,value}=>{out.push(1);put_u32(&mut out,*spec);put_text(&mut out,metric);put_text(&mut out,&value.num.to_string());put_text(&mut out,&value.den.to_string());},ProviderEvent::Unavailable{spec,reason,details}=>{out.push(2);put_u32(&mut out,*spec);put_text(&mut out,reason);put_u32(&mut out,details.len() as u32);for(k,v)in details{put_text(&mut out,k);put_text(&mut out,v);}},ProviderEvent::Complete{request_id,samples}=>{out.push(3);put_text(&mut out,request_id);out.extend_from_slice(&samples.to_be_bytes());}}}out }
fn put_u32(out:&mut Vec<u8>,value:u32){out.extend_from_slice(&value.to_be_bytes())}fn put_text(out:&mut Vec<u8>,value:&str){put_u32(out,value.len() as u32);out.extend_from_slice(value.as_bytes())}

fn decode_stream(bytes:&[u8],request:&ProviderRequest)->Result<Vec<ProviderEvent>,ProviderFailure>{if bytes.len()>MAX_BYTES{return Err(ProviderFailure::malformed("provider stream exceeds 16 MiB"));}let mut r=Reader{bytes,at:0};if r.take(MAGIC.len())?!=MAGIC{return Err(ProviderFailure::malformed("provider stream has bad magic"));}let mut events=Vec::new();while r.at<bytes.len(){let tag=r.byte()?;let event=match tag{1=>ProviderEvent::Sample{spec:r.u32()?,metric:r.text()?,value:Rational::parse(&r.text()?,&r.text()?).map_err(ProviderFailure::malformed)?},2=>{let spec=r.u32()?;let reason=r.text()?;let count=r.u32()? as usize;if count>MAX_DETAIL_SCALARS{return Err(ProviderFailure::malformed("provider unavailable detail exceeds 512 scalars"));}let mut details=Vec::with_capacity(count);for _ in 0..count{details.push((r.text()?,r.text()?));}if detail_scalars(&reason,&details)>MAX_DETAIL_SCALARS{return Err(ProviderFailure::malformed("provider unavailable detail exceeds 512 scalars"));}ProviderEvent::Unavailable{spec,reason,details}},3=>ProviderEvent::Complete{request_id:r.text()?,samples:r.u64()?},_=>return Err(ProviderFailure::malformed("provider stream has unknown event tag"))};events.push(event);}validate_events(events,request).map(|v|v.events)}
struct Reader<'a>{bytes:&'a[u8],at:usize}impl<'a>Reader<'a>{fn take(&mut self,n:usize)->Result<&'a[u8],ProviderFailure>{let end=self.at.checked_add(n).ok_or_else(||ProviderFailure::malformed("provider frame length overflow"))?;let value=self.bytes.get(self.at..end).ok_or_else(||ProviderFailure::malformed("provider stream is truncated"))?;self.at=end;Ok(value)}fn byte(&mut self)->Result<u8,ProviderFailure>{Ok(self.take(1)?[0])}fn u32(&mut self)->Result<u32,ProviderFailure>{Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))}fn u64(&mut self)->Result<u64,ProviderFailure>{Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))}fn text(&mut self)->Result<String,ProviderFailure>{let n=self.u32()? as usize;String::from_utf8(self.take(n)?.to_vec()).map_err(|_|ProviderFailure::malformed("provider text is not UTF-8"))}}

fn validate_events(events:Vec<ProviderEvent>,request:&ProviderRequest)->Result<ProviderEvidence,ProviderFailure>{if events.is_empty(){return Err(ProviderFailure::malformed("provider stream is empty"));}let mut sample_count=0usize;let mut complete=false;let mut last_spec=0u32;let mut seen=false;for(index,event)in events.iter().enumerate(){if complete{return Err(ProviderFailure::malformed("event follows final Complete"));}match event{ProviderEvent::Sample{spec,metric,..}=>{if *spec as usize>=request.specs.len()||request.specs[*spec as usize].metric!=*metric{return Err(ProviderFailure::operation(FailureClass::Incompatible,"provider sample does not match requested spec/metric"));}if seen&&*spec<last_spec{return Err(ProviderFailure::malformed("provider events are not contiguous and ordered"));}seen=true;last_spec=*spec;sample_count+=1;if sample_count>MAX_SAMPLES{return Err(ProviderFailure::malformed("provider emitted more than 1000000 samples"));}},ProviderEvent::Unavailable{spec,reason,details}=>{if *spec as usize>=request.specs.len()||reason.is_empty(){return Err(ProviderFailure::malformed("provider Unavailable has invalid spec or empty reason"));}if detail_scalars(reason,details)>MAX_DETAIL_SCALARS{return Err(ProviderFailure::malformed("provider unavailable detail exceeds 512 scalars"));}if seen&&*spec<last_spec{return Err(ProviderFailure::malformed("provider events are not contiguous and ordered"));}seen=true;last_spec=*spec;},ProviderEvent::Complete{request_id,samples}=>{if index+1!=events.len()||request_id!=&request.request_id||*samples!=sample_count as u64{return Err(ProviderFailure::malformed("provider Complete request id/count/finality mismatch"));}complete=true;}}}if !complete{return Err(ProviderFailure::malformed("provider stream has no final Complete"));}Ok(ProviderEvidence{events})}
fn detail_scalars(reason:&str,details:&[(String,String)])->usize{reason.chars().count().saturating_add(details.iter().map(|(key,value)|key.chars().count().saturating_add(value.chars().count())).sum::<usize>())}
fn is_hex64(value:&str)->bool{value.len()==64&&value.bytes().all(|b|b.is_ascii_hexdigit()&&!b.is_ascii_uppercase())}

pub fn unavailable_if_too_few(budget:&str, evidence:&ProviderEvidence, minimum:usize)->Result<(),ProviderDiagnostic>{let count=evidence.events.iter().filter(|e|matches!(e,ProviderEvent::Sample{..})).count();if let Some(ProviderEvent::Unavailable{reason,..})=evidence.events.iter().find(|e|matches!(e,ProviderEvent::Unavailable{..})){return Err(ProviderFailure::operation(FailureClass::Unavailable,reason.clone()).diagnostic(budget));}if count<minimum{return Err(ProviderFailure::operation(FailureClass::Unavailable,format!("provider returned {count} samples; policy requires {minimum}")).diagnostic(budget));}Ok(())}

#[allow(clippy::too_many_arguments)]
pub fn evaluate_provider_evidence(budget:&str,evidence_id:&str,context_key:&str,baseline_report_ids:&[String],evidence:&ProviderEvidence,spec:u32,baseline:&[Rational],percentile:Option<Percentile>,comparison:&Comparison,direction:Direction,enforcement:Enforcement,policy:Option<&MeasurementPolicy>,minimum_samples:usize)->Result<Evaluation,ProviderDiagnostic>{
    let mut samples=Vec::new();for event in &evidence.events{match event{ProviderEvent::Sample{spec:event_spec,value,..}if *event_spec==spec=>samples.push(value.clone()),ProviderEvent::Unavailable{spec:event_spec,reason,..}if *event_spec==spec=>return Err(ProviderFailure::operation(FailureClass::Unavailable,reason.clone()).diagnostic(budget)),_=>{}}}
    if samples.len()<minimum_samples{return Err(ProviderFailure::operation(FailureClass::Unavailable,format!("provider returned {} samples; policy requires {minimum_samples}",samples.len())).diagnostic(budget));}
    jet_foundation::PerformanceBudget::evaluate(evidence_id,context_key,baseline_report_ids,&samples,baseline,percentile,comparison,direction,enforcement,policy).map_err(|reason|ProviderFailure::operation(FailureClass::Execution,format!("shared evaluator rejected provider evidence: {reason}")).diagnostic(budget))
}

pub fn evaluation_diagnostic(budget:&str,evaluation:&Evaluation,direction:Direction,baseline_report_ids:&[String])->Option<ProviderDiagnostic>{
    use jet_foundation::PerformanceBudget::Evidence;match evaluation.evidence{Evidence::Pass=>None,Evidence::Unavailable=>Some(ProviderFailure::operation(FailureClass::Unavailable,"the shared evaluator found no compatible nonzero baseline evidence").diagnostic(budget)),Evidence::Regression|Evidence::Inconclusive=>{let state=if evaluation.evidence==Evidence::Regression{"regressed"}else{"is inconclusive"};let rational=|v:&Rational|format!("{}/{}",v.num,v.den);let lower=evaluation.lower95.as_ref().map(&rational).unwrap_or_else(||"none".into());let upper=evaluation.upper95.as_ref().map(&rational).unwrap_or_else(||"none".into());Some(ProviderDiagnostic{code:"E2907",what:format!("performance budget {budget} {state}"),why:format!("estimator {} with confidence [{lower}, {upper}] in {} direction did not prove the limit; baseline reports [{}]",rational(&evaluation.point),match direction{Direction::LowerIsBetter=>"lower-is-better",Direction::HigherIsBetter=>"higher-is-better"},baseline_report_ids.join(",")),fix:"improve the measured behavior, inspect the named evidence, or record an explicit exception".into()})}}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};use std::sync::Mutex;
    static PROCESS_TEST_LOCK:Mutex<()>=Mutex::new(());

    fn request() -> ProviderRequest {
        ProviderRequest { schema:"jet.provider-request".into(), version:1, request_id:"1".repeat(64), provider_hash:"2".repeat(64), context_hash:"3".repeat(64), specs:vec![ProviderSpec{budget_hash:"4".repeat(64),metric:"BenchTime".into()}], workload:CanonicalJson::Null, policy:CanonicalJson::Null }
    }
    fn valid_events(request:&ProviderRequest)->Vec<ProviderEvent>{vec![ProviderEvent::Sample{spec:0,metric:"BenchTime".into(),value:Rational::integer(42)},ProviderEvent::Complete{request_id:request.request_id.clone(),samples:1}]}
    fn panic_provider(_: &ProviderRequest,_:&ProviderCancellation)->Result<Vec<ProviderEvent>,ProviderFailure>{panic!("hostile provider panic")}
    fn unavailable_provider(request:&ProviderRequest,_:&ProviderCancellation)->Result<Vec<ProviderEvent>,ProviderFailure>{Ok(vec![ProviderEvent::Unavailable{spec:0,reason:"probe could not observe ready event".into(),details:vec![]},ProviderEvent::Complete{request_id:request.request_id.clone(),samples:0}])}
    fn temporary(name:&str)->PathBuf{static NEXT:AtomicU64=AtomicU64::new(0);std::env::temp_dir().join(format!("jet-budget-provider-{}-{name}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)))}
    #[cfg(target_os="linux")]fn assert_last_group_gone(){extern "C"{fn kill(pid:i32,signal:i32)->i32;}let group=LAST_ISOLATED_GROUP.load(Ordering::SeqCst)as i32;let deadline=Instant::now()+Duration::from_millis(100);while unsafe{kill(-group,0)}==0&&Instant::now()<deadline{std::thread::yield_now()}assert_ne!(unsafe{kill(-group,0)},0,"isolated provider process group survived timeout");}

    #[test]
    fn file_transport_round_trips_and_rejects_hostile_frames(){
        let _guard=PROCESS_TEST_LOCK.lock().unwrap_or_else(|poisoned|poisoned.into_inner());
        let req=request();let path=temporary("response");std::fs::write(&path,encode_stream(&valid_events(&req))).unwrap();let mut registry=ProviderRegistry::default();registry.register_file("fixture",path.clone()).unwrap();let evidence=registry.collect("fixture",&req,Duration::from_secs(1)).unwrap();assert_eq!(evidence.events,valid_events(&req));
        for (name,bytes) in [("bad-magic",b"NOTBUDGET\n".to_vec()),("truncated",{let mut b=MAGIC.to_vec();b.extend_from_slice(&[1,0,0]);b}),("trailing",{let mut b=encode_stream(&valid_events(&req));b.push(99);b})] { let hostile=temporary(name);std::fs::write(&hostile,bytes).unwrap();let mut registry=ProviderRegistry::default();registry.register_file("hostile",hostile.clone()).unwrap();let error=registry.collect("hostile",&req,Duration::from_secs(1)).unwrap_err();assert_eq!(error.diagnostic("api").code,"E2908");let _=std::fs::remove_file(hostile); }
        let _=std::fs::remove_file(path);
    }

    #[test]
    fn in_process_panic_and_unavailable_are_separate_diagnostic_classes(){
        let req=request();let mut registry=ProviderRegistry::default();registry.register_in_process("panic",panic_provider).unwrap();let failure=registry.collect("panic",&req,Duration::from_secs(1)).unwrap_err();let diagnostic=failure.diagnostic("api-p99");assert_eq!((diagnostic.code,diagnostic.what.as_str()),("E2908","performance budget operation failed"));
        registry.register_in_process("unavailable",unavailable_provider).unwrap();let evidence=registry.collect("unavailable",&req,Duration::from_secs(1)).unwrap();let diagnostic=unavailable_if_too_few("api-p99",&evidence,20).unwrap_err();assert_eq!((diagnostic.code,diagnostic.what.as_str()),("E2906","performance budget api-p99 has no usable evidence"));assert!(diagnostic.why.contains("ready event"));
        fn hostile(_: &ProviderRequest,_:&ProviderCancellation)->Result<Vec<ProviderEvent>,ProviderFailure>{loop{std::hint::spin_loop()}}
        let _guard=PROCESS_TEST_LOCK.lock().unwrap_or_else(|poisoned|poisoned.into_inner());registry.register_in_process("hostile",hostile).unwrap();let started=Instant::now();for _ in 0..25{let failure=registry.collect("hostile",&req,Duration::from_millis(5)).unwrap_err();assert_eq!(failure.class,FailureClass::Timeout);assert_last_group_gone();assert_eq!(ACTIVE_FILE_READERS.load(Ordering::SeqCst),0);}assert!(started.elapsed()<Duration::from_secs(1));
    }

    #[cfg(unix)]
    #[test]
    fn subprocess_uses_exact_path_and_is_bounded_and_timed(){
        let _guard=PROCESS_TEST_LOCK.lock().unwrap_or_else(|poisoned|poisoned.into_inner());
        use std::os::unix::fs::PermissionsExt;
        let req=request();let response=temporary("subprocess-response");std::fs::write(&response,encode_stream(&valid_events(&req))).unwrap();let script=temporary("provider.sh");std::fs::write(&script,format!("#!/bin/sh\ncat '{}'\n",response.display())).unwrap();let mut permissions=std::fs::metadata(&script).unwrap().permissions();permissions.set_mode(0o700);std::fs::set_permissions(&script,permissions).unwrap();let mut registry=ProviderRegistry::default();registry.register_subprocess("process",script.clone()).unwrap();assert_eq!(registry.collect("process",&req,Duration::from_secs(2)).unwrap().events,valid_events(&req));
        let sleeper=temporary("sleep.sh");std::fs::write(&sleeper,"#!/bin/sh\nsleep 5\n").unwrap();let mut permissions=std::fs::metadata(&sleeper).unwrap().permissions();permissions.set_mode(0o700);std::fs::set_permissions(&sleeper,permissions).unwrap();let mut registry=ProviderRegistry::default();registry.register_subprocess("slow",sleeper.clone()).unwrap();let started=Instant::now();let failure=registry.collect("slow",&req,Duration::from_millis(30)).unwrap_err();assert_eq!(failure.class,FailureClass::Timeout);assert!(started.elapsed()<Duration::from_secs(2));
        let descendant=temporary("descendant.sh");std::fs::write(&descendant,"#!/bin/sh\nsleep 5 &\nexit 0\n").unwrap();let mut permissions=std::fs::metadata(&descendant).unwrap().permissions();permissions.set_mode(0o700);std::fs::set_permissions(&descendant,permissions).unwrap();let mut registry=ProviderRegistry::default();registry.register_subprocess("descendant",descendant.clone()).unwrap();let started=Instant::now();let failure=registry.collect("descendant",&req,Duration::from_millis(30)).unwrap_err();assert_eq!(failure.class,FailureClass::Timeout);assert!(started.elapsed()<Duration::from_secs(1));
        for path in [response,script,sleeper,descendant]{let _=std::fs::remove_file(path);}
    }

    #[test]
    fn file_transport_rejects_symlinks_and_scalar_overflow(){
        let _guard=PROCESS_TEST_LOCK.lock().unwrap_or_else(|poisoned|poisoned.into_inner());
        let req=request();let target=temporary("target");std::fs::write(&target,encode_stream(&valid_events(&req))).unwrap();
        #[cfg(unix)]{use std::os::unix::fs::symlink;let link=temporary("link");symlink(&target,&link).unwrap();let mut registry=ProviderRegistry::default();registry.register_file("link",link.clone()).unwrap();assert!(registry.collect("link",&req,Duration::from_secs(1)).is_err());let _=std::fs::remove_file(link);}
        let events=vec![ProviderEvent::Unavailable{spec:0,reason:"x".repeat(513),details:Vec::new()},ProviderEvent::Complete{request_id:req.request_id.clone(),samples:0}];let path=temporary("detail");std::fs::write(&path,encode_stream(&events)).unwrap();let mut registry=ProviderRegistry::default();registry.register_file("detail",path.clone()).unwrap();assert!(registry.collect("detail",&req,Duration::from_secs(1)).unwrap_err().reason.contains("512 scalars"));for path in [target,path]{let _=std::fs::remove_file(path);}
    }

    #[cfg(unix)]#[test]fn repeated_timed_file_readers_are_killed_and_reaped(){let _guard=PROCESS_TEST_LOCK.lock().unwrap_or_else(|poisoned|poisoned.into_inner());let req=request();let path=temporary("delayed");std::fs::write(&path,encode_stream(&valid_events(&req))).unwrap();let mut registry=ProviderRegistry::default();registry.register_file("delayed",path.clone()).unwrap();FILE_READER_DELAY_MS.store(100,Ordering::SeqCst);let started=Instant::now();for _ in 0..25{let failure=registry.collect("delayed",&req,Duration::from_millis(5)).unwrap_err();assert_eq!(failure.class,FailureClass::Timeout);assert_last_group_gone();assert_eq!(ACTIVE_FILE_READERS.load(Ordering::SeqCst),0);}FILE_READER_DELAY_MS.store(0,Ordering::SeqCst);assert!(started.elapsed()<Duration::from_secs(1));let _=std::fs::remove_file(path);}
}
