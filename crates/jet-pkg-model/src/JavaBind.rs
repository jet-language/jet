//! Embedded JVM binding generator (D-FFI-JVM1=A).
//!
//! `javac` produces bytecode, `javap -s` is the typed discovery source, and a
//! std-only generated C bridge owns JNI invocation. The JVM is created lazily
//! in-process. Java objects cross as bounded global-reference handles; Jet can
//! borrow them for calls and consumes them through `close`.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindResult {
    pub source: String,
    pub bound: Vec<String>,
    pub archive: PathBuf,
    pub jvm_dir: PathBuf,
    pub provenance: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError { Source(String), ToolMissing(&'static str), ToolFailed(&'static str, String), IO(String) }

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source(v) | Self::IO(v) => f.write_str(v),
            Self::ToolMissing(v) => write!(f, "the provisioned `{v}` tool was not found"),
            Self::ToolFailed(tool, detail) => write!(f, "`{tool}` rejected the JVM binding input: {detail}"),
        }
    }
}

#[derive(Clone, Copy)]
enum Scalar { Int, Float, Void }
impl Scalar {
    fn jet(self) -> &'static str { match self { Self::Int => "Int", Self::Float => "Float", Self::Void => "Void" } }
    fn c(self) -> &'static str { match self { Self::Int => "int64_t", Self::Float => "double", Self::Void => "void" } }
    fn jni_field(self) -> &'static str { match self { Self::Int => "j", Self::Float => "d", Self::Void => "j" } }
}

struct Method { name: String, params: Vec<Scalar>, result: Scalar, is_static: bool }
struct Surface { class: String, ctor: Vec<Scalar>, methods: Vec<Method> }

pub fn bind(source_path: &Path, source: &str, lib: &str, cache: &Path) -> Result<BindResult, BindError> {
    if !ident(lib) { return Err(BindError::Source(format!("`{lib}` is not a valid Jet library name"))); }
    let class = source_path.file_stem().and_then(|v| v.to_str()).filter(|v| ident(v))
        .ok_or_else(|| BindError::Source("Java source needs an identifier filename matching its public class".into()))?;
    std::fs::create_dir_all(cache).map_err(|e| BindError::IO(format!("could not create JVM binding cache: {e}")))?;
    let classes = cache.join(format!("{lib}.classes"));
    let _ = std::fs::remove_dir_all(&classes);
    std::fs::create_dir_all(&classes).map_err(|e| BindError::IO(format!("could not create JVM class cache: {e}")))?;
    run(Command::new("javac").args(["-encoding", "UTF-8", "-d"]).arg(&classes).arg(source_path), "javac")?;
    let javap = run(Command::new("javap").args(["-s", "-public", "-classpath"]).arg(&classes).arg(class), "javap")?;
    let surface = parse_javap(class, &String::from_utf8_lossy(&javap.stdout))?;
    let java_home = std::env::var_os("JAVA_HOME").map(PathBuf::from)
        .ok_or(BindError::ToolMissing("JAVA_HOME"))?;
    let jvm_dir = java_home.join("lib/server");
    if !jvm_dir.join(jvm_name()).is_file() { return Err(BindError::Source("provisioned OpenJDK has no embedded libjvm runtime".into())); }
    let stem = format!("jet_java_{lib}");
    let bridge = cache.join(format!("{stem}.c"));
    let object = cache.join(format!("{stem}.o"));
    let archive = cache.join(format!("lib{stem}.a"));
    std::fs::write(&bridge, render_c(lib, &surface, &classes)).map_err(|e| BindError::IO(format!("could not write JNI bridge: {e}")))?;
    run(Command::new("cc").args(["-std=c11", "-fPIC", "-c", "-I"]).arg(java_home.join("include"))
        .arg("-I").arg(java_home.join("include").join(os_include())).arg(&bridge).arg("-o").arg(&object), "cc")?;
    run(Command::new("ar").arg("rcs").arg(&archive).arg(&object), "ar")?;
    let _ = std::fs::remove_file(&object);
    let _ = std::fs::remove_file(&bridge);
    let mut identity = Vec::new();
    identity.extend_from_slice(b"jet-java-bind-v1\0"); identity.extend_from_slice(source.as_bytes()); identity.push(0);
    identity.extend_from_slice(&javap.stdout); identity.push(0); identity.extend_from_slice(classes.to_string_lossy().as_bytes());
    let provenance = format!("schema=jet-java-bind-v1\nsha256={}\nclass={}\n", crate::SHA256::sha256_hex(&identity), class);
    let bound = surface.methods.iter().map(|m| m.name.clone()).chain(std::iter::once("new".into())).collect();
    Ok(BindResult { source: render_jet(lib, &surface), bound, archive, jvm_dir, provenance })
}

struct Output { stdout: Vec<u8>, stderr: Vec<u8>, success: bool }
fn run(command: &mut Command, tool: &'static str) -> Result<Output, BindError> {
    const LIMIT: usize = 64 * 1024;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { BindError::ToolMissing(tool) } else { BindError::IO(format!("could not start `{tool}`: {e}")) })?;
    let stdout = child.stdout.take().ok_or_else(|| BindError::IO(format!("could not supervise `{tool}` stdout")))?;
    let stderr = child.stderr.take().ok_or_else(|| BindError::IO(format!("could not supervise `{tool}` stderr")))?;
    let out = std::thread::spawn(move || drain(stdout, LIMIT));
    let err = std::thread::spawn(move || drain(stderr, LIMIT));
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop { match child.try_wait().map_err(|e| BindError::IO(format!("could not supervise `{tool}`: {e}")))? {
        Some(v) => break v,
        None if Instant::now() >= deadline => { let _ = child.kill(); let _ = child.wait(); let _ = out.join(); let _ = err.join(); return Err(BindError::ToolFailed(tool, "the tool exceeded the 60 second limit".into())); }
        None => std::thread::sleep(Duration::from_millis(10)),
    }};
    let stdout = out.join().map_err(|_| BindError::IO(format!("`{tool}` stdout reader failed")))??;
    let stderr = err.join().map_err(|_| BindError::IO(format!("`{tool}` stderr reader failed")))??;
    let result = Output { stdout, stderr, success: status.success() };
    if result.success { Ok(result) } else { Err(BindError::ToolFailed(tool, launder(&result.stderr))) }
}
fn drain(mut input: impl Read, limit: usize) -> Result<Vec<u8>, BindError> { let mut out=Vec::new(); let mut buf=[0u8;8192]; loop { let n=input.read(&mut buf).map_err(|e|BindError::IO(format!("could not read foreign tool output: {e}")))?; if n==0{break} let keep=(limit-out.len()).min(n); out.extend_from_slice(&buf[..keep]); } Ok(out) }
fn launder(bytes: &[u8]) -> String { String::from_utf8_lossy(bytes).lines().map(str::trim).find(|v|!v.is_empty()).map(|v|v.rsplit_once(": ").map_or(v,|(_,detail)|detail).chars().take(160).collect()).unwrap_or_else(||"the foreign tool returned a failure status".into()) }

fn parse_javap(class: &str, text: &str) -> Result<Surface, BindError> {
    let lines: Vec<&str> = text.lines().collect(); let mut ctor=None; let mut methods=Vec::new(); let mut names=std::collections::BTreeSet::new();
    for pair in lines.windows(2) { let sig=pair[0].trim(); let desc=pair[1].trim(); if !sig.starts_with("public ") || !desc.starts_with("descriptor:") { continue; }
        let descriptor=desc.trim_start_matches("descriptor:").trim();
        if sig.contains(&format!(" {class}(")) || sig.starts_with(&format!("public {class}(")) { let (params,result)=descriptor_types(descriptor)?; if !matches!(result,Scalar::Void){return Err(BindError::Source("JVM constructor descriptor must return void".into()))} if ctor.replace(params).is_some(){return Err(BindError::Source("overloaded constructors need an explicit overlay".into()))} continue; }
        let open=sig.find('(').ok_or_else(||BindError::Source("malformed javap method signature".into()))?; let before=sig[..open].trim(); let name=before.split_whitespace().last().unwrap_or_default(); if !ident(name){continue} if !names.insert(name.to_string()){return Err(BindError::Source(format!("overloaded Java method `{name}` needs an explicit overlay")))}
        let (params,result)=descriptor_types(descriptor)?;
        if matches!(result,Scalar::Void){return Err(BindError::Source(format!("void Java method `{name}` needs a typed overlay")))}
        methods.push(Method{name:name.into(),params,result,is_static:sig.contains(" static ")});
    }
    let ctor=ctor.ok_or_else(||BindError::Source("no supported public constructor was discovered".into()))?;
    if methods.is_empty(){return Err(BindError::Source("no supported public Java methods were discovered".into()))}
    Ok(Surface{class:class.into(),ctor,methods})
}
fn descriptor_types(value:&str)->Result<(Vec<Scalar>,Scalar),BindError>{ let close=value.find(')').ok_or_else(||BindError::Source("malformed JVM descriptor".into()))?; if !value.starts_with('('){return Err(BindError::Source("malformed JVM descriptor".into()))} let mut params=Vec::new(); for b in value[1..close].bytes(){params.push(match b{b'J'=>Scalar::Int,b'D'=>Scalar::Float,_=>return Err(BindError::Source(format!("unsupported JVM descriptor `{value}`; use long/double/void")))})} let result=match value.as_bytes().get(close+1){Some(b'J')=>Scalar::Int,Some(b'D')=>Scalar::Float,Some(b'V')=>Scalar::Void,_=>return Err(BindError::Source(format!("unsupported JVM descriptor `{value}`; use long/double/void")))}; Ok((params,result)) }

fn render_jet(lib:&str,s:&Surface)->String{ let abi=format!("jet_java_{lib}"); let mut o=format!("#Extern module c.{abi} {{\n    fn new("); params_jet(&mut o,&s.ctor); o.push_str(") => Int = \"");o.push_str(&format!("{abi}_new\"\n    fn take_error() => Int = \"{abi}_take_error\"\n    fn close(handle: Int) = \"{abi}_close\"\n")); for m in &s.methods {o.push_str("    fn ");o.push_str(&m.name);o.push('(');if !m.is_static{o.push_str("handle: Int");if !m.params.is_empty(){o.push_str(", ")}}params_jet(&mut o,&m.params);o.push(')');if !matches!(m.result,Scalar::Void){o.push_str(" => ");o.push_str(m.result.jet())}o.push_str(" = \"");o.push_str(&format!("{abi}_{}\"\n",m.name));} o.push_str("}\nuse c.");o.push_str(&abi);o.push_str(" as abi\n\npub struct Handle { value: Int }\npub enum JavaError { Exception }\n\npub fn new(");params_jet(&mut o,&s.ctor);o.push_str(") => Handle ? JavaError {\n    value :: abi.new(");args(&mut o,s.ctor.len(),0);o.push_str(")\n    if abi.take_error() != 0 { return Err(JavaError.Exception) }\n    return Ok(Handle.{ value: value })\n}\n\npub fn close(^handle: Handle) {}\n\nimpl Handle.Close {\n    fn close(^self) { abi.close(self.value) }\n}\n\n"); for m in &s.methods {o.push_str("pub fn ");o.push_str(&m.name);o.push('(');if !m.is_static{o.push_str("handle: Handle");if !m.params.is_empty(){o.push_str(", ")}}params_jet(&mut o,&m.params);o.push(')');if !matches!(m.result,Scalar::Void){o.push_str(" => ");o.push_str(m.result.jet());o.push_str(" ? JavaError")}o.push_str(" {\n    ");if !matches!(m.result,Scalar::Void){o.push_str("value :: ")}o.push_str("abi.");o.push_str(&m.name);o.push('(');let start=if m.is_static{0}else{o.push_str("handle.value");if !m.params.is_empty(){o.push_str(", ")}1};args(&mut o,m.params.len(),start);o.push_str(")\n    if abi.take_error() != 0 {");if matches!(m.result,Scalar::Void){o.push_str(" panic(\"Java exception\") }")}else{o.push_str(" return Err(JavaError.Exception) }\n    return Ok(value)")}o.push_str("\n}\n\n");}o }
fn params_jet(o:&mut String,p:&[Scalar]){for(i,t)in p.iter().enumerate(){if i>0{o.push_str(", ")}o.push_str(&format!("arg{i}: {}",t.jet()))}}
fn args(o:&mut String,n:usize,start:usize){for i in 0..n{if i>0{o.push_str(", ")}o.push_str(&format!("arg{}",i+start-start))}}

fn render_c(lib:&str,s:&Surface,classes:&Path)->String {
    let abi=format!("jet_java_{lib}");
    let mut o=format!(r#"#include <jni.h>
#include <stdint.h>
#include <pthread.h>
#include <stdlib.h>
#include <string.h>
static JavaVM *vm; static pthread_mutex_t lock=PTHREAD_MUTEX_INITIALIZER; static jobject handles[1024]; static _Thread_local int64_t last_error;
static void fail(JNIEnv*e){{last_error=1;if(e&&(*e)->ExceptionCheck(e))(*e)->ExceptionClear(e);}}
static void shutdown(void){{if(!vm)return;JNIEnv*e=0;if((*vm)->GetEnv(vm,(void**)&e,JNI_VERSION_1_8)==JNI_OK){{for(int i=0;i<1024;i++)if(handles[i]){{(*e)->DeleteGlobalRef(e,handles[i]);handles[i]=0;}}}}(*vm)->DestroyJavaVM(vm);vm=0;}}
static JNIEnv* env(void){{JNIEnv*e=0;pthread_mutex_lock(&lock);if(!vm){{JavaVMOption opt;JavaVMInitArgs a;char cp[]="-Djava.class.path={}";opt.optionString=cp;memset(&a,0,sizeof(a));a.version=JNI_VERSION_1_8;a.nOptions=1;a.options=&opt;a.ignoreUnrecognized=JNI_FALSE;if(JNI_CreateJavaVM(&vm,(void**)&e,&a)!=JNI_OK){{pthread_mutex_unlock(&lock);fail(0);return 0;}}atexit(shutdown);}}pthread_mutex_unlock(&lock);if((*vm)->GetEnv(vm,(void**)&e,JNI_VERSION_1_8)==JNI_EDETACHED&&(*vm)->AttachCurrentThread(vm,(void**)&e,0)!=JNI_OK){{fail(0);return 0;}}return e;}}
static jobject get_handle(JNIEnv*e,int64_t h){{if(h<1||h>1024){{fail(e);return 0;}}pthread_mutex_lock(&lock);jobject v=handles[h-1];pthread_mutex_unlock(&lock);if(!v)fail(e);return v;}}
int64_t {abi}_take_error(void){{int64_t v=last_error;last_error=0;if(vm)(*vm)->DetachCurrentThread(vm);return v;}}
void {abi}_close(int64_t h){{JNIEnv*e=env();if(!e||h<1||h>1024)return;pthread_mutex_lock(&lock);jobject v=handles[h-1];handles[h-1]=0;pthread_mutex_unlock(&lock);if(v)(*e)->DeleteGlobalRef(e,v);(*vm)->DetachCurrentThread(vm);}}
"#,c_escape(&classes.to_string_lossy()));
    o.push_str(&format!("int64_t {abi}_new("));
    params_c(&mut o,&s.ctor,0);
    o.push_str("){last_error=0;JNIEnv*e=env();if(!e)return 0;jclass c=(*e)->FindClass(e,\"");
    o.push_str(&c_escape(&s.class));
    o.push_str("\");if(!c){fail(e);return 0;}jmethodID m=(*e)->GetMethodID(e,c,\"<init>\",\"");
    o.push_str(&descriptor(&s.ctor,Scalar::Void));
    o.push_str("\");if(!m){fail(e);return 0;}");
    emit_jvalues(&mut o,&s.ctor,0);
    o.push_str("jobject local=(*e)->NewObjectA(e,c,m,a);if(!local||(*e)->ExceptionCheck(e)){fail(e);return 0;}jobject global=(*e)->NewGlobalRef(e,local);(*e)->DeleteLocalRef(e,local);if(!global){fail(e);return 0;}pthread_mutex_lock(&lock);for(int i=0;i<1024;i++){if(!handles[i]){handles[i]=global;pthread_mutex_unlock(&lock);return i+1;}}pthread_mutex_unlock(&lock);(*e)->DeleteGlobalRef(e,global);fail(e);return 0;}\n");
    for m in &s.methods {
        o.push_str(m.result.c()); o.push(' '); o.push_str(&format!("{abi}_{}(",m.name));
        let mut first=true;
        if !m.is_static { o.push_str("int64_t handle"); first=false; }
        for(i,t)in m.params.iter().enumerate(){if !first{o.push(',')}first=false;o.push_str(t.c());o.push_str(&format!(" arg{i}"))}
        o.push_str("){last_error=0;JNIEnv*e=env();if(!e)"); default_return(&mut o,m.result);
        o.push_str(&format!("jclass c=(*e)->FindClass(e,\"{}\");if(!c){{fail(e);",c_escape(&s.class))); default_return(&mut o,m.result); o.push('}');
        let target=if m.is_static{"c"}else{"obj"};
        if !m.is_static { o.push_str("jobject obj=get_handle(e,handle);if(!obj)"); default_return(&mut o,m.result); }
        o.push_str(&format!("jmethodID id=(*e)->Get{}MethodID(e,c,\"{}\",\"{}\");if(!id){{fail(e);",if m.is_static{"Static"}else{""},m.name,descriptor(&m.params,m.result))); default_return(&mut o,m.result); o.push('}');
        emit_jvalues(&mut o,&m.params,0);
        match m.result {
            Scalar::Int=>o.push_str(&format!("jlong value=(*e)->Call{}LongMethodA(e,{target},id,a);",if m.is_static{"Static"}else{""})),
            Scalar::Float=>o.push_str(&format!("jdouble value=(*e)->Call{}DoubleMethodA(e,{target},id,a);",if m.is_static{"Static"}else{""})),
            Scalar::Void=>o.push_str(&format!("(*e)->Call{}VoidMethodA(e,{target},id,a);",if m.is_static{"Static"}else{""})),
        }
        o.push_str("if((*e)->ExceptionCheck(e)){fail(e);"); default_return(&mut o,m.result); o.push('}');
        if !matches!(m.result,Scalar::Void){o.push_str("return value;")} o.push_str("}\n");
    }
    o
}
fn params_c(o:&mut String,p:&[Scalar],_:usize){for(i,t)in p.iter().enumerate(){if i>0{o.push(',')}o.push_str(t.c());o.push_str(&format!(" arg{i}"))}}
fn emit_jvalues(o:&mut String,p:&[Scalar],_:usize){o.push_str(&format!("jvalue a[{}];",p.len().max(1)));for(i,t)in p.iter().enumerate(){o.push_str(&format!("a[{i}].{}=arg{i};",t.jni_field()))}}
fn descriptor(p:&[Scalar],r:Scalar)->String{let mut o="(".to_string();for t in p{o.push(match t{Scalar::Int=>'J',Scalar::Float=>'D',Scalar::Void=>'V'})}o.push(')');o.push(match r{Scalar::Int=>'J',Scalar::Float=>'D',Scalar::Void=>'V'});o}
fn default_return(o:&mut String,t:Scalar){match t{Scalar::Int=>o.push_str("return 0;"),Scalar::Float=>o.push_str("return 0.0;"),Scalar::Void=>o.push_str("return;")}}
fn c_escape(v:&str)->String{v.replace('\\',"\\\\").replace('"',"\\\"")}
fn ident(v:&str)->bool{let mut c=v.chars();matches!(c.next(),Some(x)if x.is_ascii_alphabetic()||x=='_')&&c.all(|x|x.is_ascii_alphanumeric()||x=='_')}
#[cfg(target_os="linux")]fn os_include()->&'static str{"linux"}
#[cfg(target_os="macos")]fn os_include()->&'static str{"darwin"}
#[cfg(target_os="windows")]fn os_include()->&'static str{"win32"}
#[cfg(target_os="linux")]fn jvm_name()->&'static str{"libjvm.so"}
#[cfg(target_os="macos")]fn jvm_name()->&'static str{"libjvm.dylib"}
#[cfg(target_os="windows")]fn jvm_name()->&'static str{"jvm.dll"}
