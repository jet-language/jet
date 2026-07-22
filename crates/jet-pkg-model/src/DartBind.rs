//! Dart-hosted native FFI binder (D-FFI-DART1=A).
//!
//! Dart owns the isolate. The generated host loads a native Jet plugin,
//! initializes `dart_api_dl`, and pins isolate-local callbacks for Jet calls.

use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path,PathBuf};
use std::process::{Command,Stdio};
use std::time::{Duration,Instant};

#[derive(Debug,Clone,PartialEq,Eq)]
pub struct BindResult {
    pub source:String,
    pub host_source:String,
    pub host_rust:String,
    pub bound:Vec<String>,
    pub archive:PathBuf,
    pub provenance:String,
}

#[derive(Debug,Clone,PartialEq,Eq)]
pub enum BindError { Source(String),ToolMissing(&'static str),ToolFailed(&'static str,String),Io(String) }
impl std::fmt::Display for BindError { fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result { match self { Self::Source(v)|Self::Io(v)=>f.write_str(v),Self::ToolMissing(v)=>write!(f,"the provisioned `{v}` tool was not found"),Self::ToolFailed(t,v)=>write!(f,"`{t}` rejected the Dart binding input: {v}") } } }

#[derive(Clone,Copy,PartialEq,Eq)] enum Scalar { Int,Float }
impl Scalar {
    fn jet(self)->&'static str { if self==Self::Int{"Int"}else{"Float"} }
    fn c(self)->&'static str { if self==Self::Int{"int64_t"}else{"double"} }
    fn rust(self)->&'static str { if self==Self::Int{"i64"}else{"f64"} }
    fn dart_native(self)->&'static str { if self==Self::Int{"Int64"}else{"Double"} }
    fn dart(self)->&'static str { if self==Self::Int{"int"}else{"double"} }
}
#[derive(Clone)] struct Param { name:String,kind:Scalar }
#[derive(Clone)] struct Function { name:String,jet:String,params:Vec<Param>,result:Scalar }

pub fn bind(path:&Path,source:&str,lib:&str,cache:&Path)->Result<BindResult,BindError>{
    if !ident(lib){return Err(BindError::Source(format!("`{lib}` is not a valid Jet library name")))}
    let functions=parse(source)?;
    std::fs::create_dir_all(cache).map_err(|e|BindError::Io(format!("could not create Dart binding cache: {e}")))?;
    let dart=tool_path("dart").ok_or(BindError::ToolMissing("dart"))?;
    let sdk=dart.parent().and_then(Path::parent).ok_or_else(||BindError::Source("the Dart SDK root could not be derived from the provisioned `dart` tool".into()))?;
    let header=find_named(sdk,"dart_api_dl.h")?.ok_or_else(||BindError::Source("the provisioned Dart SDK has no `dart_api_dl.h`".into()))?;
    let api_c=find_named(sdk,"dart_api_dl.c")?.ok_or_else(||BindError::Source("the provisioned Dart SDK has no `dart_api_dl.c`".into()))?;
    let build=cache.join(format!(".dart-build-{lib}"));let _=std::fs::remove_dir_all(&build);std::fs::create_dir_all(&build).map_err(|e|BindError::Io(format!("could not create Dart build directory: {e}")))?;
    let bridge_c=build.join(format!("jet_dart_{lib}.c"));let bridge_o=build.join("bridge.o");let api_o=build.join("dart_api_dl.o");
    std::fs::write(&bridge_c,render_c(lib,&functions)).map_err(|e|BindError::Io(format!("could not write Dart callback bridge: {e}")))?;
    let include=header.parent().ok_or_else(||BindError::Source("the Dart API DL header has no parent directory".into()))?;
    run(Command::new("cc").args(["-std=c11","-fPIC","-c"]).arg(&bridge_c).arg("-I").arg(include).arg("-o").arg(&bridge_o),"cc")?;
    run(Command::new("cc").args(["-std=c11","-fPIC","-c"]).arg(&api_c).arg("-I").arg(include).arg("-o").arg(&api_o),"cc")?;
    let archive=cache.join(format!("libjet_dart_{lib}.a"));let _=std::fs::remove_file(&archive);
    run(Command::new("ar").arg("rcs").arg(&archive).arg(&bridge_o).arg(&api_o),"ar")?;
    let canonical=std::fs::canonicalize(path).map_err(|e|BindError::Io(format!("could not resolve the Dart contract path: {e}")))?;
    let mut identity=b"jet-dart-bind-v1\0".to_vec();identity.extend_from_slice(source.as_bytes());identity.push(0);identity.extend_from_slice(canonical.to_string_lossy().as_bytes());identity.push(0);identity.extend_from_slice(dart.to_string_lossy().as_bytes());
    let result=BindResult{source:render_jet(lib,&functions),host_source:render_dart_host(&canonical,lib,&functions),host_rust:render_host_rust(lib,&functions),bound:functions.iter().map(|v|v.jet.clone()).collect(),archive,provenance:format!("schema=jet-dart-bind-v1\nsha256={}\ndart={}\ncontract={}\n",crate::SHA256::sha256_hex(&identity),dart.display(),canonical.display())};
    let _=std::fs::remove_dir_all(&build);Ok(result)
}

pub fn build_compute(guest_rust:&str,host_rust:&str,ffi:Option<&crate::AST::FfiLink>,clinks:&[String],lib:&str,cache:&Path)->Result<PathBuf,BindError>{
    let source=cache.join(format!("{lib}_compute.rs"));let output=cache.join(format!("libjet_dart_{lib}_compute{}",shared_ext()));
    std::fs::write(&source,format!("{guest_rust}\n{host_rust}")).map_err(|e|BindError::Io(format!("could not write the native Jet plugin source: {e}")))?;
    let mut command=Command::new("rustc");command.args(["--edition","2021","--crate-type","cdylib","-O","--crate-name"]).arg(format!("jet_dart_{lib}")).arg(&source).arg("-o").arg(&output);
    if let Some(link)=ffi { command.arg("--extern").arg(format!("{}={}",link.crate_name,link.rlib_path.display()));for deps_dir in link.dependency_dirs().filter(|dir|dir.is_dir()){command.arg("-L").arg(format!("dependency={}",deps_dir.display()));} }
    for arg in clinks {command.arg(arg);}command.arg("-L").arg(format!("native={}",cache.display())).arg("-l").arg(format!("static=jet_dart_{lib}"));
    run(&mut command,"rustc")?;if !output.is_file(){return Err(BindError::Source("rustc did not produce the native Jet compute library".into()))}Ok(output)
}

pub fn bind_compute_provenance(base:&str,compute_path:&Path,compute_source:&str)->Result<String,BindError>{
    let canonical=std::fs::canonicalize(compute_path).map_err(|e|BindError::Io(format!("could not resolve the Jet compute path: {e}")))?;
    let mut identity=b"jet-dart-compute-v1\0".to_vec();identity.extend_from_slice(base.as_bytes());identity.push(0);identity.extend_from_slice(compute_source.as_bytes());identity.push(0);identity.extend_from_slice(canonical.to_string_lossy().as_bytes());
    Ok(format!("schema=jet-dart-compute-v1\nsha256={}\ncompute={}\ncontract_provenance_sha256={}\n",crate::SHA256::sha256_hex(&identity),canonical.display(),crate::SHA256::sha256_hex(base.as_bytes())))
}

fn parse(source:&str)->Result<Vec<Function>,BindError>{
    let text=strip_comments(source);let mut functions=Vec::new();let mut rest=text.as_str();
    while let Some(at)=rest.find("@pragma") { rest=&rest[at..];let end=rest.find(')').ok_or_else(||BindError::Source("a Dart `@pragma` annotation is not closed".into()))?;let annotation=&rest[..=end];rest=&rest[end+1..];if !annotation.contains("vm:entry-point"){continue}
        let boundary=rest.find(|c|c=='{'||c=='='||c==';').ok_or_else(||BindError::Source("an entry-point declaration has no function body".into()))?;let header=rest[..boundary].trim();let open=header.find('(').ok_or_else(||BindError::Source("an entry-point declaration is not a top-level function".into()))?;let close=header.rfind(')').ok_or_else(||BindError::Source("an entry-point function parameter list is not closed".into()))?;let prefix=header[..open].split_whitespace().collect::<Vec<_>>();if prefix.len()!=2{return Err(BindError::Source("entry points must be top-level functions with an explicit scalar result type".into()))}let result=scalar(prefix[0]).ok_or_else(||BindError::Source(format!("Dart entry point `{}` uses unsupported result type `{}`",prefix[1],prefix[0])))?;let name=prefix[1].to_string();if !ident(&name){return Err(BindError::Source(format!("`{name}` is not a bindable Dart entry point")))}let jet=crate::CppBind::snake(&name);if reserved_jet_function(&jet){return Err(BindError::Source(format!("Dart entry point `{name}` projects to reserved Jet name `{jet}`")))}
        let mut params=Vec::new();for item in header[open+1..close].split(','){let item=item.trim();if item.is_empty(){continue}if item.chars().any(|c|matches!(c,'{'|'}'|'['|']')){return Err(BindError::Source(format!("Dart entry point `{name}` cannot use named or optional parameters")))}let words=item.split_whitespace().collect::<Vec<_>>();if words.len()!=2{return Err(BindError::Source(format!("Dart entry point `{name}` needs explicit positional parameter types")))}let kind=scalar(words[0]).ok_or_else(||BindError::Source(format!("Dart entry point `{name}` uses unsupported parameter type `{}`",words[0])))?;if !ident(words[1]){return Err(BindError::Source(format!("Dart entry point `{name}` has invalid parameter `{}`",words[1])))}let param=crate::CppBind::snake(words[1]);if reserved_jet_parameter(&param){return Err(BindError::Source(format!("Dart parameter `{}` on `{name}` projects to reserved Jet name `{param}`",words[1])))}if params.iter().any(|v:&Param|v.name==param){return Err(BindError::Source(format!("Dart parameter `{}` on `{name}` collides with another generated Jet parameter `{param}`",words[1])))}params.push(Param{name:param,kind});}
        if functions.iter().any(|v:&Function|v.jet==jet){return Err(BindError::Source(format!("Dart entry point `{name}` collides with another generated Jet function `{jet}`")))}functions.push(Function{name,jet,params,result});rest=&rest[boundary+1..];
    }
    if functions.is_empty(){return Err(BindError::Source("no `@pragma('vm:entry-point')` top-level `int` or `double` functions were found".into()))}Ok(functions)
}

fn render_jet(lib:&str,functions:&[Function])->String{let abi=format!("jet_dart_{lib}");let mut out=format!("@Extern module c.{abi} {{\n    fn take_error() -> Int = \"{abi}_take_error\"\n");for f in functions{out.push_str("    fn ");out.push_str(&f.jet);jet_params(&mut out,&f.params);out.push_str(" -> ");out.push_str(f.result.jet());out.push_str(" = \"");out.push_str(&format!("{abi}_call_{}\"\n",f.name));}out.push_str("}\nuse c.");out.push_str(&abi);out.push_str(" as abi\n\npub enum DartError { NotInitialized CallbackUnavailable }\n\n");for f in functions{out.push_str("pub fn ");out.push_str(&f.jet);jet_params(&mut out,&f.params);out.push_str(" -> ");out.push_str(f.result.jet());out.push_str(" ? DartError {\n    result :: abi.");out.push_str(&f.jet);call_args(&mut out,&f.params);out.push_str("\n    code :: abi.take_error()\n    if code == 1 { return Err(DartError.NotInitialized) }\n    if code == 2 { return Err(DartError.CallbackUnavailable) }\n    return Ok(result)\n}\n\n");}out}
fn jet_params(out:&mut String,params:&[Param]){out.push('(');for(i,p)in params.iter().enumerate(){if i>0{out.push_str(", ")}out.push_str(&p.name);out.push_str(": ");out.push_str(p.kind.jet())}out.push(')')}
fn call_args(out:&mut String,params:&[Param]){out.push('(');for(i,p)in params.iter().enumerate(){if i>0{out.push_str(", ")}out.push_str(&p.name)}out.push(')')}

fn render_c(lib:&str,functions:&[Function])->String{let abi=format!("jet_dart_{lib}");let mut out="#include <stdint.h>\n#include <stddef.h>\n#include \"dart_api_dl.h\"\nstatic _Thread_local int64_t jet_dart_error;\nstatic int initialized;\n".to_string();for f in functions{out.push_str("typedef ");out.push_str(f.result.c());out.push_str(" (*callback_");out.push_str(&f.name);out.push_str(")(");c_param_types(&mut out,&f.params);out.push_str(");\nstatic callback_");out.push_str(&f.name);out.push(' ');out.push_str(&format!("cb_{};\n",f.name));}out.push_str(&format!("int64_t {abi}_initialize(void* data){{if(!data)return 1;intptr_t status=Dart_InitializeApiDL(data);if(status!=0)return 1;initialized=1;return 0;}}\nint64_t {abi}_take_error(void){{int64_t value=jet_dart_error;jet_dart_error=0;return value;}}\nvoid {abi}_reset(void){{"));for f in functions{out.push_str(&format!("cb_{}=0;",f.name));}out.push_str("}\n");for f in functions{out.push_str(&format!("int64_t {abi}_register_{}(callback_{} value){{if(!initialized)return 1;if(!value)return 2;cb_{}=value;return 0;}}\n",f.name,f.name,f.name));out.push_str(f.result.c());out.push(' ');out.push_str(&format!("{abi}_call_{}(",f.name));c_params(&mut out,&f.params);out.push_str("){jet_dart_error=0;if(!initialized){jet_dart_error=1;return 0;}if(!cb_");out.push_str(&f.name);out.push_str("){jet_dart_error=2;return 0;}return cb_");out.push_str(&f.name);call_args(&mut out,&f.params);out.push_str(";}\n");}out}
fn c_param_types(out:&mut String,params:&[Param]){if params.is_empty(){out.push_str("void")}for(i,p)in params.iter().enumerate(){if i>0{out.push(',')}out.push_str(p.kind.c())}}
fn c_params(out:&mut String,params:&[Param]){if params.is_empty(){out.push_str("void")}for(i,p)in params.iter().enumerate(){if i>0{out.push(',')}out.push_str(p.kind.c());out.push(' ');out.push_str(&p.name)}}

fn render_host_rust(lib:&str,functions:&[Function])->String{let abi=format!("jet_dart_{lib}");let mut out="\nuse std::ffi::c_void;\nunsafe extern \"C\" {\n".to_string();out.push_str(&format!("fn {abi}_initialize(data: *mut c_void) -> i64;\nfn {abi}_reset();\n"));for f in functions{out.push_str(&format!("fn {abi}_register_{}(callback: extern \"C\" fn(",f.name));for(i,p)in f.params.iter().enumerate(){if i>0{out.push_str(", ")}out.push_str(p.kind.rust())}out.push_str(") -> ");out.push_str(f.result.rust());out.push_str(") -> i64;\n");}out.push_str("}\n");out.push_str(&format!("#[export_name = \"{abi}_host_initialize\"]\npub extern \"C\" fn __jet_dart_host_initialize(data: *mut c_void) -> i64 {{ unsafe {{ {abi}_initialize(data) }} }}\n#[export_name = \"{abi}_host_reset\"]\npub extern \"C\" fn __jet_dart_host_reset() {{ unsafe {{ {abi}_reset() }} }}\n"));for f in functions{out.push_str(&format!("#[export_name = \"{abi}_host_register_{}\"]\npub extern \"C\" fn __jet_dart_host_register_{}(callback: extern \"C\" fn(",f.name,f.name));for(i,p)in f.params.iter().enumerate(){if i>0{out.push_str(", ")}out.push_str(p.kind.rust())}out.push_str(") -> ");out.push_str(f.result.rust());out.push_str(") -> i64 { unsafe { ");out.push_str(&format!("{abi}_register_{}(callback)",f.name));out.push_str(" } }\n");}out}

fn render_dart_host(contract:&Path,lib:&str,functions:&[Function])->String{let abi=format!("jet_dart_{lib}");let uri=format!("file://{}",contract.to_string_lossy()).replace('\\',"/").replace('\'',"%27");let mut out=format!("// Generated by `jet inspect bind dart` (D-FFI-DART1=A).\nimport 'dart:ffi';\nimport '{uri}' as contract;\n\ntypedef _InitializeNative = Int64 Function(Pointer<Void>);\ntypedef _InitializeDart = int Function(Pointer<Void>);\nlate final DynamicLibrary jetDartLibrary;\nfinal List<NativeCallable> _jetDartCallbacks = <NativeCallable>[];\nbool _jetDartReady = false;\nbool _jetDartClosed = false;\n\nvoid initializeJetDart([String libraryPath = 'libjet_dart_{lib}_compute{}']) {{\n  if (_jetDartReady) return;\n  if (_jetDartClosed) {{ throw StateError('Dart callbacks were already closed'); }}\n  jetDartLibrary = DynamicLibrary.open(libraryPath);\n  final initialize = jetDartLibrary.lookupFunction<_InitializeNative, _InitializeDart>('{abi}_host_initialize');\n  if (initialize(NativeApi.initializeApiDLData) != 0) {{ throw StateError('Dart API DL initialization failed'); }}\n",shared_ext());for f in functions{let native_sig=dart_signature(f,true);let exceptional=if f.result==Scalar::Int{"0"}else{"0.0"};out.push_str(&format!("  final callback_{} = NativeCallable<{native_sig}>.isolateLocal(contract.{}, exceptionalReturn: {exceptional});\n  _jetDartCallbacks.add(callback_{});\n  final register_{} = jetDartLibrary.lookupFunction<Int64 Function(Pointer<NativeFunction<{native_sig}>>), int Function(Pointer<NativeFunction<{native_sig}>>)>('{abi}_host_register_{}');\n  if (register_{}(callback_{}.nativeFunction) != 0) {{ throw StateError('Dart callback registration failed: {}'); }}\n",f.name,f.name,f.name,f.name,f.name,f.name,f.name,f.name));}out.push_str(&format!("  _jetDartReady = true;\n}}\n\nvoid shutdownJetDart() {{\n  if (!_jetDartReady) return;\n  final reset = jetDartLibrary.lookupFunction<Void Function(), void Function()>('{abi}_host_reset');\n  reset();\n  for (final callback in _jetDartCallbacks) {{ callback.close(); }}\n  _jetDartCallbacks.clear();\n  _jetDartReady = false;\n  _jetDartClosed = true;\n}}\n"));out}
fn dart_signature(f:&Function,native:bool)->String{let ty=|s:Scalar|if native{s.dart_native()}else{s.dart()};let mut out=ty(f.result).to_string();out.push_str(" Function(");for(i,p)in f.params.iter().enumerate(){if i>0{out.push_str(", ")}out.push_str(ty(p.kind))}out.push(')');out}

fn find_named(root:&Path,name:&str)->Result<Option<PathBuf>,BindError>{let mut queue=VecDeque::from([(root.to_path_buf(),0usize)]);let mut seen=0;while let Some((dir,depth))=queue.pop_front(){if depth>7||seen>=4096{continue}let entries=std::fs::read_dir(&dir).map_err(|e|BindError::Io(format!("could not inspect the Dart SDK at `{}`: {e}",dir.display())))?;for entry in entries{let entry=entry.map_err(|e|BindError::Io(format!("could not inspect a Dart SDK entry: {e}")))?;seen+=1;let path=entry.path();if path.file_name().is_some_and(|v|v==name){return Ok(Some(path))}if depth<7&&entry.file_type().map_err(|e|BindError::Io(format!("could not inspect `{}`: {e}",path.display())))?.is_dir(){queue.push_back((path,depth+1));}}}Ok(None)}
fn strip_comments(source:&str)->String{let mut out=String::new();let mut chars=source.chars().peekable();let mut block=false;while let Some(c)=chars.next(){if block{if c=='*'&&chars.peek()==Some(&'/'){chars.next();block=false}continue}if c=='/'&&chars.peek()==Some(&'*'){chars.next();block=true;continue}if c=='/'&&chars.peek()==Some(&'/'){for n in chars.by_ref(){if n=='\n'{out.push('\n');break}}continue}out.push(c)}out}
fn scalar(v:&str)->Option<Scalar>{match v{"int"=>Some(Scalar::Int),"double"=>Some(Scalar::Float),_=>None}}
fn ident(v:&str)->bool{let mut chars=v.chars();matches!(chars.next(),Some(c)if c.is_ascii_alphabetic()||c=='_')&&chars.all(|c|c.is_ascii_alphanumeric()||c=='_')}
fn reserved_jet_name(v:&str)->bool{crate::Syntax::JET_KEYWORD_LIST.contains(&v)||crate::Syntax::JET_TYPE_LIST.contains(&v)}
fn reserved_jet_function(v:&str)->bool{matches!(v,"take_error"|"abi")||reserved_jet_name(v)}
fn reserved_jet_parameter(v:&str)->bool{v=="abi"||reserved_jet_name(v)}
fn tool_path(tool:&str)->Option<PathBuf>{let path=std::env::var_os("PATH")?;std::env::split_paths(&path).map(|v|v.join(tool)).find(|v|v.is_file()).and_then(|v|std::fs::canonicalize(v).ok())}
fn run(command:&mut Command,tool:&'static str)->Result<(),BindError>{const CAP:usize=64*1024;command.stdout(Stdio::piped()).stderr(Stdio::piped());let mut child=command.spawn().map_err(|e|if e.kind()==std::io::ErrorKind::NotFound{BindError::ToolMissing(tool)}else{BindError::Io(format!("could not start `{tool}`: {e}"))})?;let stdout=child.stdout.take().ok_or_else(||BindError::Io(format!("could not supervise `{tool}` stdout")))?;let stderr=child.stderr.take().ok_or_else(||BindError::Io(format!("could not supervise `{tool}` stderr")))?;let out=std::thread::spawn(move||drain(stdout,CAP));let err=std::thread::spawn(move||drain(stderr,CAP));let deadline=Instant::now()+Duration::from_secs(60);let status=loop{match child.try_wait().map_err(|e|BindError::Io(format!("could not supervise `{tool}`: {e}")))?{Some(v)=>break v,None if Instant::now()>=deadline=>{let _=child.kill();let _=child.wait();let _=out.join();let _=err.join();return Err(BindError::ToolFailed(tool,"the tool exceeded the 60 second limit".into()))},None=>std::thread::sleep(Duration::from_millis(10))}};let stdout=out.join().map_err(|_|BindError::Io(format!("`{tool}` stdout reader failed")))??;let stderr=err.join().map_err(|_|BindError::Io(format!("`{tool}` stderr reader failed")))??;if status.success(){Ok(())}else{let detail=if stderr.is_empty(){&stdout}else{&stderr};Err(BindError::ToolFailed(tool,launder(detail)))}}
fn drain(mut input:impl Read,limit:usize)->Result<Vec<u8>,BindError>{let mut out=Vec::new();let mut buf=[0u8;8192];loop{let n=input.read(&mut buf).map_err(|e|BindError::Io(format!("could not read foreign tool output: {e}")))?;if n==0{break}let keep=(limit-out.len()).min(n);out.extend_from_slice(&buf[..keep]);}Ok(out)}
fn launder(value:&[u8])->String{String::from_utf8_lossy(value).lines().map(str::trim).find(|v|!v.is_empty()).map(|v|v.chars().take(160).collect()).unwrap_or_else(||"the foreign tool returned a failure status".into())}
#[cfg(target_os="linux")]fn shared_ext()->&'static str{".so"}
#[cfg(target_os="macos")]fn shared_ext()->&'static str{".dylib"}
#[cfg(target_os="windows")]fn shared_ext()->&'static str{".dll"}

#[cfg(test)]
mod tests{
    #[test]
    fn projects_dart_names_without_changing_foreign_lookup(){
        let functions=super::parse("@pragma('vm:entry-point')\nint dartDouble(int someValue) => someValue * 2;\n").unwrap();
        assert_eq!(functions[0].name,"dartDouble");
        assert_eq!(functions[0].jet,"dart_double");
        assert_eq!(functions[0].params[0].name,"some_value");
        assert!(super::render_jet("callbacks",&functions).contains("pub fn dart_double(some_value: Int)"));
        assert!(super::render_dart_host(std::path::Path::new("callbacks.dart"),"callbacks",&functions).contains("contract.dartDouble"));
    }

    #[test]
    fn rejects_projected_dart_name_collisions_precisely(){
        let Err(error)=super::parse("@pragma('vm:entry-point')\nint dartDouble(int value) => value;\n@pragma('vm:entry-point')\nint DartDouble(int value) => value;\n")else{panic!("projected collision was accepted")};
        assert_eq!(error,super::BindError::Source("Dart entry point `DartDouble` collides with another generated Jet function `dart_double`".into()));
    }

    #[test]
    fn rejects_generated_dart_helper_and_alias_collisions(){
        for (source,message) in [
            ("@pragma('vm:entry-point')\nint takeError() => 0;\n","Dart entry point `takeError` projects to reserved Jet name `take_error`"),
            ("@pragma('vm:entry-point')\nint abi() => 0;\n","Dart entry point `abi` projects to reserved Jet name `abi`"),
            ("@pragma('vm:entry-point')\nint compute(int abi) => abi;\n","Dart parameter `abi` on `compute` projects to reserved Jet name `abi`"),
        ]{
            let Err(error)=super::parse(source)else{panic!("generated Dart name collision was accepted")};
            assert_eq!(error,super::BindError::Source(message.into()));
        }
    }
}
