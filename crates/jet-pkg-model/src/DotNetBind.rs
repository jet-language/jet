//! Embedded .NET binding generator (D-FFI-DOTNET1=A).
//!
//! The provisioned SDK compiles the input and a reflection inspector. Generated
//! `[UnmanagedCallersOnly]` entry points retain managed objects through bounded
//! `GCHandle` ownership; a std-only C bridge embeds CoreCLR through hostfxr.

use std::io::Read;
use std::path::{Path,PathBuf};
use std::process::{Command,Stdio};
use std::time::{Duration,Instant};

#[derive(Debug,Clone,PartialEq,Eq)]
pub struct BindResult{pub source:String,pub bound:Vec<String>,pub archive:PathBuf,pub provenance:String}
#[derive(Debug,Clone,PartialEq,Eq)]
pub enum BindError{Source(String),ToolMissing(&'static str),ToolFailed(&'static str,String),Io(String)}
impl std::fmt::Display for BindError{fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{match self{Self::Source(v)|Self::Io(v)=>f.write_str(v),Self::ToolMissing(v)=>write!(f,"the provisioned `{v}` tool was not found"),Self::ToolFailed(t,v)=>write!(f,"`{t}` rejected the .NET binding input: {v}")}}}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]enum Scalar{Int,Float}
impl Scalar{fn parse(v:&str)->Option<Self>{match v{"int"=>Some(Self::Int),"float"=>Some(Self::Float),_=>None}}fn jet(self)->&'static str{match self{Self::Int=>"Int",Self::Float=>"Float"}}fn c(self)->&'static str{match self{Self::Int=>"int64_t",Self::Float=>"double"}}fn cs(self)->&'static str{match self{Self::Int=>"long",Self::Float=>"double"}}}
#[derive(Debug)]struct Method{name:String,is_static:bool,result:Scalar,params:Vec<Scalar>}
#[derive(Debug)]struct Surface{class:String,ctor:Vec<Scalar>,methods:Vec<Method>}

pub fn bind(path:&Path,source:&str,lib:&str,cache:&Path)->Result<BindResult,BindError>{
    if !ident(lib){return Err(BindError::Source(format!("`{lib}` is not a valid Jet library name")))}
    let class=path.file_stem().and_then(|v|v.to_str()).filter(|v|ident(v)).ok_or_else(||BindError::Source("C# source needs an identifier filename matching its public class".into()))?;
    std::fs::create_dir_all(cache).map_err(|e|BindError::Io(format!("could not create .NET binding cache: {e}")))?;
    let build=cache.join(format!(".dotnet-build-{lib}"));let _=std::fs::remove_dir_all(&build);std::fs::create_dir_all(&build).map_err(|e|BindError::Io(format!("could not create .NET build directory: {e}")))?;
    std::fs::write(build.join(format!("{class}.cs")),source).map_err(|e|BindError::Io(format!("could not stage C# source: {e}")))?;
    std::fs::write(build.join("Inspect.csproj"),PROJECT.replace("@OUTPUT@","Exe")).map_err(|e|BindError::Io(format!("could not write reflection project: {e}")))?;
    std::fs::write(build.join("Program.cs"),INSPECTOR).map_err(|e|BindError::Io(format!("could not write reflection inspector: {e}")))?;
    run(Command::new("dotnet").args(["build"]).arg(build.join("Inspect.csproj")).args(["-c","Release","--nologo","-v:q","--disable-build-servers"]),"dotnet")?;
    let inspected=run(Command::new("dotnet").arg(build.join("bin/Release/net8.0/Inspect.dll")).arg(class),"dotnet")?;
    let surface=parse_surface(&inspected.stdout,class)?;
    std::fs::remove_file(build.join("Program.cs")).map_err(|e|BindError::Io(format!("could not replace reflection inspector: {e}")))?;
    std::fs::write(build.join("Bridge.cs"),render_cs(lib,&surface)).map_err(|e|BindError::Io(format!("could not write managed .NET bridge: {e}")))?;
    std::fs::write(build.join("Inspect.csproj"),PROJECT.replace("@OUTPUT@","Library")).map_err(|e|BindError::Io(format!("could not write managed bridge project: {e}")))?;
    run(Command::new("dotnet").args(["build"]).arg(build.join("Inspect.csproj")).args(["-c","Release","--nologo","-v:q","--disable-build-servers","-p:AssemblyName=JetBinding"]),"dotnet")?;
    let managed=cache.join(format!("{lib}.dotnet"));let _=std::fs::remove_dir_all(&managed);std::fs::create_dir_all(&managed).map_err(|e|BindError::Io(format!("could not create managed runtime cache: {e}")))?;
    let assembly=managed.join("JetBinding.dll");std::fs::copy(build.join("bin/Release/net8.0/JetBinding.dll"),&assembly).map_err(|e|BindError::Io(format!("could not retain managed bridge assembly: {e}")))?;
    let runtime_config=managed.join("JetBinding.runtimeconfig.json");std::fs::write(&runtime_config,RUNTIME_CONFIG).map_err(|e|BindError::Io(format!("could not write .NET runtime config: {e}")))?;
    let hostfxr=find_hostfxr()?;let stem=format!("jet_cs_{lib}");let bridge=build.join(format!("{stem}.c"));let object=build.join(format!("{stem}.o"));let archive=cache.join(format!("lib{stem}.a"));
    std::fs::write(&bridge,render_c(lib,&surface,&hostfxr,&assembly,&runtime_config)).map_err(|e|BindError::Io(format!("could not write native hostfxr bridge: {e}")))?;
    run(Command::new("cc").args(["-std=c11","-fPIC","-c"]).arg(&bridge).arg("-o").arg(&object),"cc")?;let _=std::fs::remove_file(&archive);run(Command::new("ar").arg("rcs").arg(&archive).arg(&object),"ar")?;
    let jet=render_jet(lib,&surface);let mut identity=b"jet-dotnet-bind-v1\0".to_vec();identity.extend_from_slice(source.as_bytes());identity.push(0);identity.extend_from_slice(&inspected.stdout);identity.push(0);identity.extend_from_slice(hostfxr.to_string_lossy().as_bytes());let provenance=format!("schema=jet-dotnet-bind-v1\nsha256={}\nclass={}\nhostfxr={}\n",crate::SHA256::sha256_hex(&identity),surface.class,hostfxr.display());
    let bound=std::iter::once("new".into()).chain(surface.methods.iter().map(|m|m.name.clone())).collect();let _=std::fs::remove_dir_all(build);Ok(BindResult{source:jet,bound,archive,provenance})
}

const PROJECT:&str=r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>@OUTPUT@</OutputType><TargetFramework>net8.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><RestoreIgnoreFailedSources>true</RestoreIgnoreFailedSources></PropertyGroup></Project>"#;
const RUNTIME_CONFIG:&str=r#"{"runtimeOptions":{"tfm":"net8.0","framework":{"name":"Microsoft.NETCore.App","version":"8.0.0"},"rollForward":"LatestPatch"}}"#;
const INSPECTOR:&str=r#"using System.Reflection;
var wanted=args[0];var type=Assembly.GetExecutingAssembly().GetTypes().SingleOrDefault(t=>t.IsPublic&&t.Name==wanted)??throw new Exception("public class not found");
static string K(Type t)=>t==typeof(long)?"int":t==typeof(double)?"float":"unsupported";
Console.WriteLine($"CLASS\t{type.FullName}");
foreach(var c in type.GetConstructors(BindingFlags.Public|BindingFlags.Instance)){Console.Write("CTOR");foreach(var p in c.GetParameters())Console.Write($"\t{K(p.ParameterType)}");Console.WriteLine();}
foreach(var m in type.GetMethods(BindingFlags.Public|BindingFlags.Instance|BindingFlags.Static|BindingFlags.DeclaredOnly)){Console.Write($"METHOD\t{m.Name}\t{(m.IsStatic?"S":"I")}\t{K(m.ReturnType)}");foreach(var p in m.GetParameters())Console.Write($"\t{K(p.ParameterType)}");Console.WriteLine();}
"#;

fn parse_surface(bytes:&[u8],expected:&str)->Result<Surface,BindError>{let text=std::str::from_utf8(bytes).map_err(|_|BindError::Source("the .NET reflection inspector returned non-UTF-8 metadata".into()))?;let mut class=None;let mut ctor=None;let mut methods=Vec::new();let mut names=std::collections::BTreeSet::new();for line in text.lines(){let f=line.split('\t').collect::<Vec<_>>();match f.first().copied(){Some("CLASS")if f.len()==2=>class=Some(f[1].to_string()),Some("CTOR")=>{let params=parse_scalars(&f[1..],"constructor")?;if ctor.replace(params).is_some(){return Err(BindError::Source("overloaded C# constructors need an explicit overlay".into()))}},Some("METHOD")if f.len()>=4=>{if !ident(f[1]){continue}if !names.insert(f[1].to_string()){return Err(BindError::Source(format!("overloaded C# method `{}` needs an explicit overlay",f[1])))}let result=Scalar::parse(f[3]).ok_or_else(||BindError::Source(format!("C# method `{}` uses an unsupported return type; use long or double",f[1])))?;let params=parse_scalars(&f[4..],f[1])?;methods.push(Method{name:f[1].into(),is_static:f[2]=="S",result,params})},Some("METHOD")=>return Err(BindError::Source("malformed .NET reflection metadata".into())),_=>{}}}let class=class.ok_or_else(||BindError::Source(format!("public C# class `{expected}` was not discovered")))?;let ctor=ctor.ok_or_else(||BindError::Source("no supported public C# constructor was discovered".into()))?;if methods.is_empty(){return Err(BindError::Source("no supported public C# methods were discovered".into()))}Ok(Surface{class,ctor,methods})}
fn parse_scalars(values:&[&str],member:&str)->Result<Vec<Scalar>,BindError>{values.iter().map(|v|Scalar::parse(v).ok_or_else(||BindError::Source(format!("C# member `{member}` uses an unsupported parameter type; use long or double")))).collect()}

fn render_cs(lib:&str,s:&Surface)->String{
    let abi=format!("jet_cs_{lib}");
    let mut o="using System.Runtime.CompilerServices;\nusing System.Runtime.InteropServices;\nnamespace JetBinding;\npublic static class EntryPoints {\n    const int Capacity=1024;\n    static readonly object Gate=new();\n    static readonly GCHandle[] Handles=new GCHandle[Capacity];\n    static readonly uint[] Generations=new uint[Capacity];\n    [ThreadStatic] static long error;\n    static ".to_string();
    o.push_str(&s.class);
    o.push_str(" Get(long raw){var slot=(int)(raw&0xffff)-1;var generation=(uint)((ulong)raw>>16);lock(Gate){if(slot<0||slot>=Capacity||generation==0||Generations[slot]!=generation||!Handles[slot].IsAllocated)throw new InvalidOperationException();return Handles[slot].Target is ");
    o.push_str(&s.class);
    o.push_str(" value?value:throw new InvalidOperationException();}}\n    static long Store(");
    o.push_str(&s.class);
    o.push_str(" value){lock(Gate){for(var slot=0;slot<Capacity;slot++){if(Handles[slot].IsAllocated)continue;var generation=unchecked(Generations[slot]+1);if(generation==0)generation=1;Generations[slot]=generation;Handles[slot]=GCHandle.Alloc(value);return ((long)generation<<16)|(uint)(slot+1);}}throw new InsufficientMemoryException();}\n    static void Release(long raw){var slot=(int)(raw&0xffff)-1;var generation=(uint)((ulong)raw>>16);GCHandle handle;lock(Gate){if(slot<0||slot>=Capacity||generation==0||Generations[slot]!=generation||!Handles[slot].IsAllocated)throw new InvalidOperationException();handle=Handles[slot];Handles[slot]=default;}handle.Free();}\n    [UnmanagedCallersOnly(EntryPoint=\"");
    o.push_str(&abi);o.push_str("_take_error\")] public static long ");o.push_str(&abi);o.push_str("_take_error(){var v=error;error=0;return v;}\n    [UnmanagedCallersOnly(EntryPoint=\"");
    o.push_str(&abi);o.push_str("_close\")] public static void ");o.push_str(&abi);o.push_str("_close(long raw){try{Release(raw);}catch{error=2;}}\n    [UnmanagedCallersOnly(EntryPoint=\"");
    o.push_str(&abi);o.push_str("_new\")] public static long ");o.push_str(&abi);o.push_str("_new(");cs_params(&mut o,&s.ctor);o.push_str("){try{var value=new ");o.push_str(&s.class);o.push('(');args(&mut o,s.ctor.len());o.push_str(");return Store(value);}catch(InsufficientMemoryException){error=3;return 0;}catch{error=1;return 0;}}\n");
    for m in &s.methods{
        let entry=format!("{abi}_{}",m.name);
        o.push_str("    [UnmanagedCallersOnly(EntryPoint=\"");o.push_str(&entry);o.push_str("\")] public static ");o.push_str(m.result.cs());o.push(' ');o.push_str(&entry);o.push('(');
        if !m.is_static{o.push_str("long handle");if !m.params.is_empty(){o.push(',')}}
        cs_params(&mut o,&m.params);o.push_str("){try{return ");if m.is_static{o.push_str(&s.class)}else{o.push_str("Get(handle)")}o.push('.');o.push_str(&m.name);o.push('(');args(&mut o,m.params.len());o.push_str(");}catch{error=1;return default;}}\n");
    }
    o.push_str("}\n");o
}
fn cs_params(o:&mut String,p:&[Scalar]){for(i,t)in p.iter().enumerate(){if i>0{o.push(',')}o.push_str(t.cs());o.push_str(&format!(" arg{i}"))}}
fn args(o:&mut String,n:usize){for i in 0..n{if i>0{o.push(',')}o.push_str(&format!("arg{i}"))}}

fn render_jet(lib:&str,s:&Surface)->String{let abi=format!("jet_cs_{lib}");let mut o=format!("#Extern module c.{abi} {{\n    fn new(");jet_params(&mut o,&s.ctor);o.push_str(&format!(") -> Int = \"{abi}_new\"\n    fn take_error() -> Int = \"{abi}_take_error\"\n    fn close(handle: Int) = \"{abi}_close\"\n"));for m in &s.methods{o.push_str("    fn ");o.push_str(&m.name);o.push('(');if !m.is_static{o.push_str("handle: Int");if !m.params.is_empty(){o.push_str(", ")}}jet_params(&mut o,&m.params);o.push_str(") -> ");o.push_str(m.result.jet());o.push_str(&format!(" = \"{abi}_{}\"\n",m.name));}o.push_str("}\nuse c.");o.push_str(&abi);o.push_str(" as abi\n\npub struct Handle { value: Int }\npub enum DotNetError { Exception InvalidHandle ResourceLimit }\n\nfn error(code: Int) -> DotNetError { if code == 2 { return DotNetError.InvalidHandle } if code == 3 { return DotNetError.ResourceLimit } return DotNetError.Exception }\n\npub fn new(");jet_params(&mut o,&s.ctor);o.push_str(") -> Handle ? DotNetError {\n    value :: abi.new(");args(&mut o,s.ctor.len());o.push_str(")\n    code :: abi.take_error()\n    if code != 0 { return err(error(code)) }\n    return ok(Handle.{ value: value })\n}\n\npub fn close(handle: ^Handle) -> Bool ? DotNetError {\n    abi.close(handle.value)\n    code :: abi.take_error()\n    if code != 0 { return err(error(code)) }\n    return ok(true)\n}\n\n");for m in &s.methods{o.push_str("pub fn ");o.push_str(&m.name);o.push('(');if !m.is_static{o.push_str("handle: Handle");if !m.params.is_empty(){o.push_str(", ")}}jet_params(&mut o,&m.params);o.push_str(") -> ");o.push_str(m.result.jet());o.push_str(" ? DotNetError {\n    value :: abi.");o.push_str(&m.name);o.push('(');if !m.is_static{o.push_str("handle.value");if !m.params.is_empty(){o.push_str(", ")}}args(&mut o,m.params.len());o.push_str(")\n    code :: abi.take_error()\n    if code != 0 { return err(error(code)) }\n    return ok(value)\n}\n\n");}o}
fn jet_params(o:&mut String,p:&[Scalar]){for(i,t)in p.iter().enumerate(){if i>0{o.push_str(", ")}o.push_str(&format!("arg{i}: {}",t.jet()))}}

fn render_c(lib:&str,s:&Surface,hostfxr:&Path,assembly:&Path,config:&Path)->String{let abi=format!("jet_cs_{lib}");let mut wrappers=String::new();wrappers.push_str(&format!("int64_t {abi}_take_error(void){{typedef int64_t(*F)(void);F f=(F)entry(\"{abi}_take_error\");return f?f():1;}}\nvoid {abi}_close(int64_t h){{typedef void(*F)(int64_t);F f=(F)entry(\"{abi}_close\");if(f)f(h);}}\n"));wrappers.push_str(&format!("int64_t {abi}_new("));c_params(&mut wrappers,&s.ctor);wrappers.push_str("){typedef int64_t(*F)(");c_types(&mut wrappers,&s.ctor);wrappers.push_str(&format!(");F f=(F)entry(\"{abi}_new\");return f?f("));args(&mut wrappers,s.ctor.len());wrappers.push_str("):0;}\n");for m in &s.methods{wrappers.push_str(m.result.c());wrappers.push(' ');wrappers.push_str(&format!("{abi}_{}(",m.name));let types=m.params.clone();if !m.is_static{wrappers.push_str("int64_t handle");if !m.params.is_empty(){wrappers.push(',')}}c_params(&mut wrappers,&m.params);wrappers.push_str("){typedef ");wrappers.push_str(m.result.c());wrappers.push_str("(*F)(");if !m.is_static{wrappers.push_str("int64_t");if !m.params.is_empty(){wrappers.push(',')}}c_types(&mut wrappers,&types);wrappers.push_str(&format!(");F f=(F)entry(\"{abi}_{}\");return f?f(",m.name));if !m.is_static{wrappers.push_str("handle");if !m.params.is_empty(){wrappers.push(',')}}args(&mut wrappers,m.params.len());wrappers.push_str(if m.result==Scalar::Float{"):0.0;}\n"}else{"):0;}\n"});}NATIVE_C.replace("@HOSTFXR@",&c_escape(&hostfxr.to_string_lossy())).replace("@ASSEMBLY@",&c_escape(&assembly.to_string_lossy())).replace("@CONFIG@",&c_escape(&config.to_string_lossy())).replace("@TYPE@","JetBinding.EntryPoints, JetBinding").replace("@WRAPPERS@",&wrappers)}
fn c_params(o:&mut String,p:&[Scalar]){for(i,t)in p.iter().enumerate(){if i>0{o.push(',')}o.push_str(t.c());o.push_str(&format!(" arg{i}"))}}
fn c_types(o:&mut String,p:&[Scalar]){for(i,t)in p.iter().enumerate(){if i>0{o.push(',')}o.push_str(t.c())}}
const NATIVE_C:&str=r#"#include <stdint.h>
#include <pthread.h>
#include <dlfcn.h>
typedef void* hostfxr_handle;
typedef int32_t (*hostfxr_initialize_for_runtime_config_fn)(const char*,const void*,hostfxr_handle*);
typedef int32_t (*hostfxr_get_runtime_delegate_fn)(hostfxr_handle,int32_t,void**);
typedef int32_t (*hostfxr_close_fn)(hostfxr_handle);
typedef int32_t (*load_assembly_and_get_function_pointer_fn)(const char*,const char*,const char*,const char*,void*,void**);
static pthread_once_t once=PTHREAD_ONCE_INIT;static load_assembly_and_get_function_pointer_fn load;static int failed;
static void initialize(void){void*h=dlopen("@HOSTFXR@",RTLD_NOW|RTLD_LOCAL);if(!h){failed=1;return;}hostfxr_initialize_for_runtime_config_fn init=(hostfxr_initialize_for_runtime_config_fn)dlsym(h,"hostfxr_initialize_for_runtime_config");hostfxr_get_runtime_delegate_fn get=(hostfxr_get_runtime_delegate_fn)dlsym(h,"hostfxr_get_runtime_delegate");hostfxr_close_fn close=(hostfxr_close_fn)dlsym(h,"hostfxr_close");hostfxr_handle context=0;if(!init||!get||!close||init("@CONFIG@",0,&context)!=0||!context){failed=1;return;}void*delegate=0;if(get(context,5,&delegate)!=0||!delegate)failed=1;else load=(load_assembly_and_get_function_pointer_fn)delegate;close(context);}
static void*entry(const char*name){pthread_once(&once,initialize);if(failed||!load)return 0;void*fn=0;if(load("@ASSEMBLY@","@TYPE@",name,(const char*)-1,0,&fn)!=0)return 0;return fn;}
@WRAPPERS@
"#;

fn find_hostfxr()->Result<PathBuf,BindError>{let root=std::env::var_os("DOTNET_ROOT").map(PathBuf::from).or_else(||std::fs::canonicalize(which("dotnet")?).ok().and_then(|p|p.parent().map(Path::to_path_buf))).ok_or(BindError::ToolMissing("DOTNET_ROOT"))?;let dir=root.join("host/fxr");let mut versions=std::fs::read_dir(&dir).map_err(|e|BindError::Io(format!("could not inspect provisioned hostfxr: {e}")))?.filter_map(Result::ok).map(|e|e.path()).collect::<Vec<_>>();versions.sort();let name=if cfg!(target_os="macos"){"libhostfxr.dylib"}else if cfg!(target_os="windows"){"hostfxr.dll"}else{"libhostfxr.so"};versions.into_iter().rev().map(|p|p.join(name)).find(|p|p.is_file()).ok_or_else(||BindError::Source("the provisioned .NET SDK has no hostfxr runtime".into()))}
fn which(name:&str)->Option<PathBuf>{std::env::var_os("PATH")?.to_string_lossy().split(':').map(|p|Path::new(p).join(name)).find(|p|p.is_file())}
fn c_escape(v:&str)->String{v.replace('\\',"\\\\").replace('"',"\\\"")}
fn ident(v:&str)->bool{let mut c=v.chars();matches!(c.next(),Some(x)if x.is_ascii_alphabetic()||x=='_')&&c.all(|x|x.is_ascii_alphanumeric()||x=='_')}

struct Output{stdout:Vec<u8>,stderr:Vec<u8>,success:bool}
fn run(command:&mut Command,tool:&'static str)->Result<Output,BindError>{command.stdout(Stdio::piped()).stderr(Stdio::piped());let mut child=command.spawn().map_err(|e|if e.kind()==std::io::ErrorKind::NotFound{BindError::ToolMissing(tool)}else{BindError::Io(format!("could not start `{tool}`: {e}"))})?;let stdout=child.stdout.take().ok_or_else(||BindError::Io(format!("could not supervise `{tool}` stdout")))?;let stderr=child.stderr.take().ok_or_else(||BindError::Io(format!("could not supervise `{tool}` stderr")))?;let out=std::thread::spawn(move||drain(stdout));let err=std::thread::spawn(move||drain(stderr));let end=Instant::now()+Duration::from_secs(60);let status=loop{if let Some(v)=child.try_wait().map_err(|e|BindError::Io(format!("could not supervise `{tool}`: {e}")))?{break v}if Instant::now()>=end{let _=child.kill();let _=child.wait();return Err(BindError::ToolFailed(tool,"the tool exceeded the 60 second limit".into()))}std::thread::sleep(Duration::from_millis(10))};let stdout=out.join().map_err(|_|BindError::Io(format!("`{tool}` stdout reader failed")))??;let stderr=err.join().map_err(|_|BindError::Io(format!("`{tool}` stderr reader failed")))??;let result=Output{stdout,stderr,success:status.success()};if result.success{Ok(result)}else{Err(BindError::ToolFailed(tool,launder(&result.stdout,&result.stderr)))}}
fn drain(mut input:impl Read)->Result<Vec<u8>,BindError>{let mut out=Vec::new();let mut buf=[0;8192];loop{let n=input.read(&mut buf).map_err(|e|BindError::Io(format!("could not read foreign tool output: {e}")))?;if n==0{break}if out.len()<65536{out.extend_from_slice(&buf[..n.min(65536-out.len())])}}Ok(out)}
fn launder(stdout:&[u8],stderr:&[u8])->String{let text=format!("{}\n{}",String::from_utf8_lossy(stdout),String::from_utf8_lossy(stderr));if let Some(line)=text.lines().map(str::trim).find(|s|s.contains(": error CS")){if let Some((_,tail))=line.split_once(": error CS"){if let Some((_,detail))=tail.split_once(": "){let detail=detail.rsplit_once(" [").map_or(detail,|v|v.0);return detail.chars().take(160).collect()}}}text.lines().map(str::trim).find(|s|!s.is_empty()&&!s.starts_with("Determining projects")&&!s.starts_with("Restore")&&!s.starts_with("Build FAILED")&&!s.starts_with("The build failed")).map(|s|s.rsplit_once(": ").map_or(s,|x|x.1).chars().take(160).collect()).unwrap_or_else(||"the managed tool returned a failure status".into())}

#[cfg(test)]mod tests{#[test]fn reflection_schema_projects_typed_owned_surface(){let s=super::parse_surface(b"CLASS\tCounter\nCTOR\tint\nMETHOD\tadd\tI\tint\tint\nMETHOD\tscale\tS\tfloat\tfloat\n","Counter").unwrap();let jet=super::render_jet("counter",&s);assert!(jet.contains("pub struct Handle { value: Int }"));assert!(jet.contains("pub fn close(handle: ^Handle)"));assert!(jet.contains("pub fn add(handle: Handle, arg0: Int) -> Int ? DotNetError"));assert!(jet.contains("ResourceLimit"));let cs=super::render_cs("counter",&s);assert!(cs.contains("GCHandle[Capacity]"));assert!(cs.contains("Generations[slot]"));assert!(cs.contains("jet_cs_counter_add"));assert!(cs.contains("UnmanagedCallersOnly"));}}
