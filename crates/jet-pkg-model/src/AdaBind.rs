//! GNAT C-ABI binder with checked scalar constraints (D-FFI-ADA1=A).

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path,PathBuf};
use std::process::{Command,Stdio};
use std::time::{Duration,Instant};

#[derive(Debug,Clone,PartialEq,Eq)]
pub struct BindResult { pub source:String,pub bound:Vec<String>,pub archive:PathBuf,pub runtime_dir:PathBuf,pub provenance:String }
#[derive(Debug,Clone,PartialEq,Eq)]
pub enum BindError { Source(String),ToolMissing(&'static str),ToolFailed(&'static str,String),Io(String) }
impl std::fmt::Display for BindError { fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result { match self { Self::Source(v)|Self::Io(v)=>f.write_str(v),Self::ToolMissing(v)=>write!(f,"the provisioned `{v}` tool was not found"),Self::ToolFailed(t,v)=>write!(f,"`{t}` rejected the Ada binding source: {v}") } } }

#[derive(Clone,Copy)] enum Scalar { Int,Float }
impl Scalar { fn jet(self)->&'static str { match self { Self::Int=>"Int",Self::Float=>"Float" } } fn c(self)->&'static str { match self { Self::Int=>"int64_t",Self::Float=>"double" } } }
#[derive(Clone)] struct Constraint { low:String,high:String }
struct Param { name:String,scalar:Scalar,constraint:Option<Constraint> }
struct Routine { name:String,symbol:String,params:Vec<Param>,result:Scalar }

pub fn bind(spec_path:&Path,spec:&str,lib:&str,cache:&Path)->Result<BindResult,BindError>{
    if !ident(lib){return Err(BindError::Source(format!("`{lib}` is not a valid Jet library name")))}
    if spec_path.extension().and_then(|v|v.to_str())!=Some("ads"){return Err(BindError::Source("the Ada binder requires a `.ads` package spec".into()))}
    let body_path=spec_path.with_extension("adb");
    let body=std::fs::read_to_string(&body_path).map_err(|e|BindError::Source(format!("the package body `{}` could not be read ({e})",body_path.display())))?;
    let routines=parse(spec)?;
    std::fs::create_dir_all(cache).map_err(|e|BindError::Io(format!("could not create Ada binding cache: {e}")))?;
    let stem=spec_path.file_stem().and_then(|v|v.to_str()).filter(|v|ident(v)).ok_or_else(||BindError::Source("the Ada package filename is not bindable".into()))?;
    let build=cache.join(format!(".ada-build-{lib}"));let _=std::fs::remove_dir_all(&build);std::fs::create_dir_all(&build).map_err(|e|BindError::Io(format!("could not create Ada build directory: {e}")))?;
    std::fs::write(build.join(format!("{stem}.ads")),spec).and_then(|_|std::fs::write(build.join(format!("{stem}.adb")),&body)).map_err(|e|BindError::Io(format!("could not stage Ada package: {e}")))?;
    run(Command::new("gnatmake").current_dir(&build).args(["-c","-fPIC","-gnat2022",&format!("{stem}.adb")]),"gnatmake")?;
    run(Command::new("gnatbind").current_dir(&build).args([&format!("-Ljet_ada_{lib}"),&format!("{stem}.ali")]),"gnatbind")?;
    run(Command::new("gnatmake").current_dir(&build).args(["-c","-fPIC","-gnat2022",&format!("b~{stem}.adb")]),"gnatmake")?;
    let bridge=build.join(format!("jet_ada_{lib}.c"));let bridge_obj=build.join(format!("jet_ada_{lib}.o"));
    std::fs::write(&bridge,render_c(lib,&routines)).map_err(|e|BindError::Io(format!("could not write Ada bridge: {e}")))?;
    run(Command::new("cc").args(["-std=c11","-fPIC","-c"]).arg(&bridge).arg("-o").arg(&bridge_obj),"cc")?;
    let archive=cache.join(format!("libjet_ada_{lib}.a"));let _=std::fs::remove_file(&archive);
    run(Command::new("ar").arg("rcs").arg(&archive).arg(build.join(format!("{stem}.o"))).arg(build.join(format!("b~{stem}.o"))).arg(&bridge_obj),"ar")?;
    let runtime_dir=gnat_runtime_dir()?;
    let mut identity=b"jet-ada-bind-v1\0".to_vec();identity.extend_from_slice(spec.as_bytes());identity.push(0);identity.extend_from_slice(body.as_bytes());identity.push(0);identity.extend_from_slice(runtime_dir.to_string_lossy().as_bytes());
    let source=render_jet(lib,&routines);let bound=routines.into_iter().map(|v|v.name).collect();let _=std::fs::remove_dir_all(&build);
    Ok(BindResult{source,bound,archive,runtime_dir,provenance:format!("schema=jet-ada-bind-v1\nsha256={}\n",crate::SHA256::sha256_hex(&identity))})
}

fn parse(source:&str)->Result<Vec<Routine>,BindError>{
    let text=source.lines().map(|v|v.split("--").next().unwrap_or("")).collect::<Vec<_>>().join(" ");
    let mut constraints:BTreeMap<String,(Scalar,Constraint)>=BTreeMap::new();let mut routines=Vec::new();
    for raw in statements(&text){let statement=raw.split_whitespace().collect::<Vec<_>>().join(" ");if statement.is_empty(){continue}let lower=statement.to_ascii_lowercase();
        if let Some(at)=lower.find("subtype "){let rest=&lower[at+8..];let Some((name,tail))=rest.split_once(" is ") else {continue};let Some((base,range))=tail.split_once(" range ") else {continue};let Some((low,high))=range.split_once("..") else {return Err(BindError::Source(format!("subtype `{name}` has a malformed range")))};let scalar=scalar(base).ok_or_else(||BindError::Source(format!("subtype `{name}` uses an unsupported scalar base `{base}`")))?;constraints.insert(name.trim().into(),(scalar,Constraint{low:low.trim().into(),high:high.trim().into()}));continue}
        let Some(function_at)=lower.find("function ") else {continue};if !lower.contains("convention => c")||!lower.contains("export"){continue}let header=&statement[function_at+9..];let open=header.find('(').ok_or_else(||BindError::Source("malformed exported Ada function".into()))?;let name=header[..open].trim().to_ascii_lowercase();if !ident(&name){return Err(BindError::Source(format!("`{name}` is not a bindable Ada function name")))}let close=matching_close(header,open).ok_or_else(||BindError::Source(format!("function `{name}` has no closed parameter list")))?;
        let after=&header[close+1..];let after_lower=after.to_ascii_lowercase();let return_at=after_lower.find("return ").ok_or_else(||BindError::Source(format!("function `{name}` has no return type")))?;let result_name=after[return_at+7..].split_whitespace().next().unwrap_or("").trim_matches(',');let (result,_)=resolve_type(result_name,&constraints).ok_or_else(||BindError::Source(format!("function `{name}` has unsupported return type `{result_name}`")))?;
        let symbol=quoted_after(after,"external_name =>").ok_or_else(||BindError::Source(format!("function `{name}` must declare `External_Name => \"...\"`")))?;if !ident(&symbol){return Err(BindError::Source(format!("`{symbol}` is not a bindable C symbol")))}
        let mut params=Vec::new();for group in header[open+1..close].split(';'){let Some((names,kind))=group.split_once(':') else {return Err(BindError::Source(format!("function `{name}` has a malformed parameter")))};let kind=kind.trim();if kind.to_ascii_lowercase().starts_with("out ")||kind.to_ascii_lowercase().starts_with("in out "){return Err(BindError::Source(format!("function `{name}` uses unsupported output parameters")))}let kind=kind.strip_prefix("in ").unwrap_or(kind);let (value,constraint)=resolve_type(kind,&constraints).ok_or_else(||BindError::Source(format!("function `{name}` has unsupported parameter type `{kind}`")))?;for param in names.split(','){let param=param.trim().to_ascii_lowercase();if !ident(&param){return Err(BindError::Source(format!("function `{name}` has invalid parameter `{param}`")))}params.push(Param{name:param,scalar:value,constraint:constraint.clone()})}}
        routines.push(Routine{name,symbol,params,result});
    }
    if routines.is_empty(){return Err(BindError::Source("no supported exported Ada functions were found".into()))}Ok(routines)
}

fn resolve_type(name:&str,constraints:&BTreeMap<String,(Scalar,Constraint)>)->Option<(Scalar,Option<Constraint>)>{let lower=name.trim().to_ascii_lowercase();if let Some((value,range))=constraints.get(&lower){return Some((*value,Some(range.clone())))}scalar(&lower).map(|v|(v,None))}
fn scalar(name:&str)->Option<Scalar>{match name.trim().to_ascii_lowercase().as_str(){"interfaces.c.long_long"|"long_long_integer"=>Some(Scalar::Int),"interfaces.c.double"|"long_float"=>Some(Scalar::Float),_=>None}}

fn render_jet(lib:&str,routines:&[Routine])->String{let abi=format!("jet_ada_{lib}");let mut o=format!("#Extern module c.{abi} {{\n");for r in routines{o.push_str("    fn ");o.push_str(&r.name);params_jet(&mut o,&r.params);o.push_str(" => ");o.push_str(r.result.jet());o.push_str(" = \"");o.push_str(&format!("{abi}_{}\"\n",r.name));}o.push_str("}\nuse c.");o.push_str(&abi);o.push_str(" as abi\n\npub enum AdaError { Constraint }\n\n");for r in routines{o.push_str("pub fn ");o.push_str(&r.name);params_jet(&mut o,&r.params);o.push_str(" => ");o.push_str(r.result.jet());o.push_str(" ? AdaError {\n");for p in &r.params{if let Some(c)=&p.constraint{o.push_str("    if ");o.push_str(&p.name);o.push_str(" < ");o.push_str(&jet_number(&c.low,p.scalar));o.push_str(" || ");o.push_str(&p.name);o.push_str(" > ");o.push_str(&jet_number(&c.high,p.scalar));o.push_str(" { return Err(AdaError.Constraint) }\n")}}o.push_str("    return Ok(abi.");o.push_str(&r.name);o.push('(');for (i,p) in r.params.iter().enumerate(){if i>0{o.push_str(", ")}o.push_str(&p.name)}o.push_str("))\n}\n\n");}o}
fn params_jet(out:&mut String,params:&[Param]){out.push('(');for(i,p)in params.iter().enumerate(){if i>0{out.push_str(", ")}out.push_str(&p.name);out.push_str(": ");out.push_str(p.scalar.jet())}out.push(')')}
fn jet_number(value:&str,scalar:Scalar)->String{let mut value=value.replace('_',"");if matches!(scalar,Scalar::Float)&&!value.contains('.')&&!value.contains('e')&&!value.contains('E'){value.push_str(".0")}value}
fn render_c(lib:&str,routines:&[Routine])->String{let abi=format!("jet_ada_{lib}");let mut o=format!("#include <stdint.h>\n#include <pthread.h>\n#include <stdlib.h>\nextern void {abi}init(void);\nextern void {abi}final(void);\nstatic pthread_once_t once=PTHREAD_ONCE_INIT;\nstatic void finish(void){{{abi}final();}}\nstatic void init(void){{{abi}init();atexit(finish);}}\n");for r in routines{o.push_str("extern ");o.push_str(r.result.c());o.push(' ');o.push_str(&r.symbol);params_c(&mut o,&r.params);o.push_str(";\n");o.push_str(r.result.c());o.push(' ');o.push_str(&format!("{abi}_{}",r.name));params_c(&mut o,&r.params);o.push_str("{pthread_once(&once,init);return ");o.push_str(&r.symbol);o.push('(');for(i,p)in r.params.iter().enumerate(){if i>0{o.push(',')}o.push_str(&p.name)}o.push_str(");}\n");}o}
fn params_c(out:&mut String,params:&[Param]){out.push('(');if params.is_empty(){out.push_str("void")}for(i,p)in params.iter().enumerate(){if i>0{out.push(',')}out.push_str(p.scalar.c());out.push(' ');out.push_str(&p.name)}out.push(')')}

fn gnat_runtime_dir()->Result<PathBuf,BindError>{let output=capture(Command::new("gnatls").arg("-v"),"gnatls")?;let text=String::from_utf8_lossy(&output);text.lines().map(str::trim).find(|v|v.ends_with("/adalib")&&Path::new(v).is_absolute()&&Path::new(v).join("libgnat.so").is_file()).map(PathBuf::from).ok_or_else(||BindError::Source("the provisioned GNAT runtime library directory was not found".into()))}
fn run(command:&mut Command,tool:&'static str)->Result<(),BindError>{let output=capture(command,tool)?;if output.first()==Some(&0){Ok(())}else{Err(BindError::ToolFailed(tool,launder(&output[1..])))} }
fn capture(command:&mut Command,tool:&'static str)->Result<Vec<u8>,BindError>{const CAP:usize=64*1024;command.stdout(Stdio::piped()).stderr(Stdio::piped());let mut child=command.spawn().map_err(|e|if e.kind()==std::io::ErrorKind::NotFound{BindError::ToolMissing(tool)}else{BindError::Io(format!("could not start `{tool}`: {e}"))})?;let stdout=child.stdout.take().ok_or_else(||BindError::Io(format!("could not supervise `{tool}` stdout")))?;let stderr=child.stderr.take().ok_or_else(||BindError::Io(format!("could not supervise `{tool}` stderr")))?;let out=std::thread::spawn(move||drain(stdout,CAP));let err=std::thread::spawn(move||drain(stderr,CAP));let deadline=Instant::now()+Duration::from_secs(60);let status=loop{match child.try_wait().map_err(|e|BindError::Io(format!("could not supervise `{tool}`: {e}"))){Ok(Some(v))=>break v,Ok(None)if Instant::now()>=deadline=>{let _=child.kill();let _=child.wait();let _=out.join();let _=err.join();return Err(BindError::ToolFailed(tool,"the tool exceeded the 60 second limit".into()))},Ok(None)=>std::thread::sleep(Duration::from_millis(10)),Err(e)=>return Err(e)}};let stdout=out.join().map_err(|_|BindError::Io(format!("`{tool}` stdout reader failed")))??;let stderr=err.join().map_err(|_|BindError::Io(format!("`{tool}` stderr reader failed")))??;let mut result=vec![u8::from(!status.success())];result.extend_from_slice(&stdout);result.extend_from_slice(&stderr);Ok(result)}
fn drain(mut input:impl Read,limit:usize)->Result<Vec<u8>,BindError>{let mut out=Vec::new();let mut buf=[0u8;8192];loop{let n=input.read(&mut buf).map_err(|e|BindError::Io(format!("could not read foreign tool output: {e}")))?;if n==0{break}let keep=(limit-out.len()).min(n);out.extend_from_slice(&buf[..keep]);}Ok(out)}
fn launder(v:&[u8])->String{let text=String::from_utf8_lossy(v);if text.lines().any(|v|v.contains("compilation error")){return "compilation error".into()}text.lines().map(str::trim).find(|v|!v.is_empty()).map(|v|v.rsplit_once(": ").map_or(v,|x|x.1).chars().take(160).collect()).unwrap_or_else(||"the foreign tool returned a failure status".into())}
fn quoted_after<'a>(text:&'a str,needle:&str)->Option<String>{let lower=text.to_ascii_lowercase();let at=lower.find(needle)?;let tail=text[at+needle.len()..].trim_start();let quote=tail.chars().next()?;if quote!='"'&&quote!='\''{return None}Some(tail[1..].split(quote).next()?.into())}
fn statements(text:&str)->Vec<&str>{let mut out=Vec::new();let mut start=0;let mut depth=0;for(i,c)in text.char_indices(){match c{'('=>depth+=1,')'=>depth-=1,';' if depth==0=>{out.push(&text[start..i]);start=i+1},_=>{}}}if start<text.len(){out.push(&text[start..])}out}
fn matching_close(text:&str,open:usize)->Option<usize>{let mut depth=0;for(i,c)in text.char_indices().skip_while(|(i,_)|*i<open){match c{'('=>depth+=1,')'=>{depth-=1;if depth==0{return Some(i)}},_=>{}}}None}
fn ident(v:&str)->bool{let mut c=v.chars();matches!(c.next(),Some(x)if x.is_ascii_alphabetic()||x=='_')&&c.all(|x|x.is_ascii_alphanumeric()||x=='_')}
