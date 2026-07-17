//! In-process Tcl binding generator (D-FFI-TCL1=A).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug,Clone,PartialEq,Eq)]
pub struct BindResult { pub source:String, pub archive:PathBuf, pub lib_dir:PathBuf, pub provenance:String }
#[derive(Debug,Clone,PartialEq,Eq)]
pub enum BindError { Source(String), ToolMissing(&'static str), ToolFailed(&'static str,String), Io(String) }
impl std::fmt::Display for BindError { fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{match self{Self::Source(v)|Self::Io(v)=>f.write_str(v),Self::ToolMissing(v)=>write!(f,"the provisioned `{v}` tool was not found"),Self::ToolFailed(t,v)=>write!(f,"`{t}` rejected the Tcl binding bridge: {v}")}} }

pub fn bind(source:&str,lib:&str,cache:&Path)->Result<BindResult,BindError>{
    if !ident(lib){return Err(BindError::Source(format!("`{lib}` is not a valid Jet library name")))}
    let root=tool_root("tclsh").ok_or(BindError::ToolMissing("tclsh"))?;
    let include=root.join("include"); let lib_dir=root.join("lib");
    if !include.join("tcl.h").is_file()||!lib_dir.join(lib_name()).is_file(){return Err(BindError::Source("the provisioned Tcl runtime has no embeddable headers or shared library".into()))}
    std::fs::create_dir_all(cache).map_err(|e|BindError::Io(format!("could not create Tcl binding cache: {e}")))?;
    let stem=format!("jet_tcl_{lib}"); let c=cache.join(format!("{stem}.c")); let obj=cache.join(format!("{stem}.o")); let archive=cache.join(format!("lib{stem}.a"));
    std::fs::write(&c,render_c(lib,source)).map_err(|e|BindError::Io(format!("could not write Tcl bridge: {e}")))?;
    run(Command::new("cc").args(["-std=c11","-fPIC","-c","-I"]).arg(&include).arg(&c).arg("-o").arg(&obj),"cc")?;
    run(Command::new("ar").arg("rcs").arg(&archive).arg(&obj),"ar")?;
    let _=std::fs::remove_file(c);let _=std::fs::remove_file(obj);
    let mut identity=b"jet-tcl-bind-v1\0".to_vec();identity.extend_from_slice(source.as_bytes());identity.push(0);identity.extend_from_slice(root.to_string_lossy().as_bytes());
    Ok(BindResult{source:render_jet(lib),archive,lib_dir,provenance:format!("schema=jet-tcl-bind-v1\nsha256={}\n",crate::SHA256::sha256_hex(&identity))})
}

fn render_jet(lib:&str)->String{let abi=format!("jet_tcl_{lib}");format!(r#"@Extern module c.{abi} {{
    fn open() -> Int = "{abi}_open"
    fn eval(handle: Int, code: String) -> String = "{abi}_eval"
    fn eval_once(code: String) -> String = "{abi}_eval_once"
    fn eval_int(handle: Int, code: String) -> Int = "{abi}_eval_int"
    fn eval_float(handle: Int, code: String) -> Float = "{abi}_eval_float"
    fn take_error() -> Int = "{abi}_take_error"
    fn close(handle: Int) = "{abi}_close"
}}
use c.{abi} as abi

pub struct Session {{ value: Int }}
pub enum TclError {{ Eval }}

pub fn open() -> Session ? TclError {{
    value :: abi.open()
    if abi.take_error() != 0 {{ return Err(TclError.Eval) }}
    return Ok(Session.{{ value: value }})
}}

pub fn eval(session: Session, code: String) -> String ? TclError {{
    value :: abi.eval(session.value, code)
    if abi.take_error() != 0 {{ return Err(TclError.Eval) }}
    return Ok(value)
}}

pub fn eval_once(code: String) -> String ? TclError {{
    value :: abi.eval_once(code)
    if abi.take_error() != 0 {{ return Err(TclError.Eval) }}
    return Ok(value)
}}

pub fn eval_int(session: Session, code: String) -> Int ? TclError {{
    value :: abi.eval_int(session.value, code)
    if abi.take_error() != 0 {{ return Err(TclError.Eval) }}
    return Ok(value)
}}

pub fn eval_float(session: Session, code: String) -> Float ? TclError {{
    value :: abi.eval_float(session.value, code)
    if abi.take_error() != 0 {{ return Err(TclError.Eval) }}
    return Ok(value)
}}

impl Session.Close {{
    fn close(^self) {{ abi.close(self.value) }}
}}
"#)}

fn render_c(lib:&str,seed:&str)->String{let abi=format!("jet_tcl_{lib}");format!(r#"#include <tcl.h>
#include <stdint.h>
#include <pthread.h>
#include <stdlib.h>
#include <string.h>
#define LIMIT 65536
typedef struct {{ Tcl_Interp *interp; pthread_t owner; }} Slot;
static Slot slots[64]; static pthread_mutex_t lock=PTHREAD_MUTEX_INITIALIZER; static pthread_once_t once=PTHREAD_ONCE_INIT; static _Thread_local int64_t failed; static _Thread_local char result[LIMIT];
static const char seed[]="{}";
static void finish(void){{for(int i=0;i<64;i++)if(slots[i].interp){{Tcl_DeleteInterp(slots[i].interp);slots[i].interp=0;}}Tcl_Finalize();}}
static void init(void){{Tcl_FindExecutable("jet");atexit(finish);}}
static Tcl_Interp* fresh(void){{pthread_once(&once,init);Tcl_Interp*i=Tcl_CreateInterp();if(!i||Tcl_Init(i)!=TCL_OK){{if(i)Tcl_DeleteInterp(i);failed=1;return 0;}}return i;}}
static const char* copy_result(Tcl_Interp*i){{int n=0;const char*s=Tcl_GetStringFromObj(Tcl_GetObjResult(i),&n);if(!s||n<0||n>=LIMIT||(int)strlen(s)!=n){{failed=1;result[0]=0;return result;}}memcpy(result,s,n);result[n]=0;return result;}}
static Tcl_Interp* get(int64_t h){{if(h<1||h>64){{failed=1;return 0;}}pthread_mutex_lock(&lock);Slot s=slots[h-1];pthread_mutex_unlock(&lock);if(!s.interp||!pthread_equal(s.owner,pthread_self())){{failed=1;return 0;}}return s.interp;}}
int64_t {abi}_take_error(void){{int64_t v=failed;failed=0;return v;}}
int64_t {abi}_open(void){{failed=0;Tcl_Interp*i=fresh();if(!i)return 0;if(Tcl_EvalEx(i,seed,-1,TCL_EVAL_DIRECT)!=TCL_OK){{Tcl_ResetResult(i);Tcl_DeleteInterp(i);failed=1;return 0;}}pthread_mutex_lock(&lock);for(int n=0;n<64;n++)if(!slots[n].interp){{slots[n].interp=i;slots[n].owner=pthread_self();pthread_mutex_unlock(&lock);return n+1;}}pthread_mutex_unlock(&lock);Tcl_DeleteInterp(i);failed=1;return 0;}}
const char* {abi}_eval(int64_t h,const char*code){{failed=0;Tcl_Interp*i=get(h);if(!i||!code)return "";if(Tcl_EvalEx(i,code,-1,TCL_EVAL_DIRECT)!=TCL_OK){{Tcl_ResetResult(i);failed=1;return "";}}return copy_result(i);}}
const char* {abi}_eval_once(const char*code){{failed=0;Tcl_Interp*i=fresh();if(!i||!code)return "";const char*out="";if(Tcl_EvalEx(i,code,-1,TCL_EVAL_DIRECT)!=TCL_OK){{Tcl_ResetResult(i);failed=1;}}else out=copy_result(i);Tcl_DeleteInterp(i);return out;}}
int64_t {abi}_eval_int(int64_t h,const char*code){{failed=0;Tcl_Interp*i=get(h);Tcl_WideInt v=0;if(!i||!code||Tcl_EvalEx(i,code,-1,TCL_EVAL_DIRECT)!=TCL_OK||Tcl_GetWideIntFromObj(i,Tcl_GetObjResult(i),&v)!=TCL_OK){{Tcl_ResetResult(i);failed=1;return 0;}}return (int64_t)v;}}
double {abi}_eval_float(int64_t h,const char*code){{failed=0;Tcl_Interp*i=get(h);double v=0;if(!i||!code||Tcl_EvalEx(i,code,-1,TCL_EVAL_DIRECT)!=TCL_OK||Tcl_GetDoubleFromObj(i,Tcl_GetObjResult(i),&v)!=TCL_OK){{Tcl_ResetResult(i);failed=1;return 0;}}return v;}}
void {abi}_close(int64_t h){{failed=0;if(h<1||h>64)return;pthread_mutex_lock(&lock);Slot s=slots[h-1];if(!s.interp||!pthread_equal(s.owner,pthread_self())){{pthread_mutex_unlock(&lock);failed=1;return;}}slots[h-1].interp=0;pthread_mutex_unlock(&lock);Tcl_DeleteInterp(s.interp);}}
"#,c_escape(seed))}

fn tool_root(tool:&str)->Option<PathBuf>{let path=std::env::var_os("PATH")?;for dir in std::env::split_paths(&path){let candidate=dir.join(tool);if candidate.is_file(){let exe=std::fs::canonicalize(candidate).ok()?;return exe.parent()?.parent().map(Path::to_path_buf)}}None}
fn run(command:&mut Command,tool:&'static str)->Result<(),BindError>{const CAP:usize=64*1024;command.stdout(Stdio::piped()).stderr(Stdio::piped());let mut child=command.spawn().map_err(|e|if e.kind()==std::io::ErrorKind::NotFound{BindError::ToolMissing(tool)}else{BindError::Io(format!("could not start `{tool}`: {e}"))})?;let stdout=child.stdout.take().ok_or_else(||BindError::Io(format!("could not supervise `{tool}` stdout")))?;let stderr=child.stderr.take().ok_or_else(||BindError::Io(format!("could not supervise `{tool}` stderr")))?;let out=std::thread::spawn(move||drain(stdout,CAP));let err=std::thread::spawn(move||drain(stderr,CAP));let deadline=Instant::now()+Duration::from_secs(60);let status=loop{match child.try_wait().map_err(|e|BindError::Io(format!("could not supervise `{tool}`: {e}")))?{Some(v)=>break v,None if Instant::now()>=deadline=>{let _=child.kill();let _=child.wait();let _=out.join();let _=err.join();return Err(BindError::ToolFailed(tool,"the tool exceeded the 60 second limit".into()))},None=>std::thread::sleep(Duration::from_millis(10))}};let _=out.join().map_err(|_|BindError::Io(format!("`{tool}` stdout reader failed")))??;let stderr=err.join().map_err(|_|BindError::Io(format!("`{tool}` stderr reader failed")))??;if status.success(){Ok(())}else{Err(BindError::ToolFailed(tool,launder(&stderr)))}}
fn drain(mut input:impl Read,limit:usize)->Result<Vec<u8>,BindError>{let mut out=Vec::new();let mut buf=[0u8;8192];loop{let n=input.read(&mut buf).map_err(|e|BindError::Io(format!("could not read foreign tool output: {e}")))?;if n==0{break}let keep=(limit-out.len()).min(n);out.extend_from_slice(&buf[..keep]);}Ok(out)}
fn launder(v:&[u8])->String{String::from_utf8_lossy(v).lines().map(str::trim).find(|v|!v.is_empty()).map(|v|v.rsplit_once(": ").map_or(v,|x|x.1).chars().take(160).collect()).unwrap_or_else(||"the foreign tool returned a failure status".into())}
fn c_escape(v:&str)->String{let mut o=String::new();for b in v.bytes(){match b{b'\\'=>o.push_str("\\\\"),b'"'=>o.push_str("\\\""),b'\n'=>o.push_str("\\n"),b'\r'=>o.push_str("\\r"),b'\t'=>o.push_str("\\t"),0x20..=0x7e=>o.push(b as char),_=>o.push_str(&format!("\\{:03o}",b))}}o}
fn ident(v:&str)->bool{let mut c=v.chars();matches!(c.next(),Some(x)if x.is_ascii_alphabetic()||x=='_')&&c.all(|x|x.is_ascii_alphanumeric()||x=='_')}
#[cfg(target_os="linux")]fn lib_name()->&'static str{"libtcl.so"}
#[cfg(target_os="macos")]fn lib_name()->&'static str{"libtcl.dylib"}
#[cfg(target_os="windows")]fn lib_name()->&'static str{"tcl86.dll"}
