//! Windows COM type-library binder (D-FFI-COM1=A).

use std::io::Read;
use std::path::{Path,PathBuf};
use std::process::{Command,Stdio};
use std::time::{Duration,Instant};

#[derive(Debug,Clone,PartialEq,Eq)]
pub enum TypeLibraryInput{File(PathBuf),Registered{guid:String,major:u16,minor:u16,lcid:u32}}
#[derive(Debug,Clone,PartialEq,Eq)]
pub struct BindResult{pub source:String,pub archive:PathBuf,pub provenance:String,pub methods:Vec<String>}
#[derive(Debug,Clone,PartialEq,Eq)]
pub enum BindError{UnsupportedHost,Source(String),ToolMissing(&'static str),ToolFailed(&'static str,String),Io(String)}
impl std::fmt::Display for BindError{fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{match self{Self::UnsupportedHost=>f.write_str("COM type libraries and IDispatch require a Windows host"),Self::Source(v)|Self::Io(v)=>f.write_str(v),Self::ToolMissing(v)=>write!(f,"the provisioned `{v}` tool was not found"),Self::ToolFailed(t,v)=>write!(f,"`{t}` could not inspect the COM type library: {v}")}}}

#[derive(Debug,Clone,PartialEq,Eq)]enum Kind{Unit,Int,Float,Bool,Text,Data,Object(String)}
impl Kind{fn parse(v:&str)->Option<Self>{Some(match v{"unit"=>Self::Unit,"int"=>Self::Int,"float"=>Self::Float,"bool"=>Self::Bool,"text"=>Self::Text,"data"=>Self::Data,_=>Self::Object(project(v.strip_prefix("object=")?).ok()?)})}fn jet(&self)->&str{match self{Self::Unit=>"Void",Self::Int=>"Int",Self::Float=>"Float",Self::Bool=>"Bool",Self::Text=>"String",Self::Data=>"DataTree",Self::Object(v)=>v}}fn c(&self)->&'static str{match self{Self::Unit=>"void",Self::Int|Self::Bool|Self::Object(_)=>"int64_t",Self::Float=>"double",Self::Text|Self::Data=>"const char*"}}}
#[derive(Debug,Clone,PartialEq,Eq)]struct Param{name:String,kind:Kind}
#[derive(Debug,Clone,PartialEq,Eq)]struct Method{interface:String,jet:String,dispid:i32,flags:u16,result:Kind,params:Vec<Param>}
#[derive(Debug,Clone,PartialEq,Eq)]struct Schema{name:String,guid:String,class_guid:String,root_interface:String,methods:Vec<Method>}

pub fn bind(input:&TypeLibraryInput,lib:&str,cache:&Path)->Result<BindResult,BindError>{
    if !cfg!(target_os="windows"){return Err(BindError::UnsupportedHost)}if !ident(lib){return Err(BindError::Source(format!("`{lib}` is not a valid Jet library name")))}
    std::fs::create_dir_all(cache).map_err(|e|BindError::Io(format!("could not create COM binding cache: {e}")))?;let build=cache.join(format!(".com-build-{lib}"));let _=std::fs::remove_dir_all(&build);std::fs::create_dir_all(&build).map_err(|e|BindError::Io(format!("could not create COM build directory: {e}")))?;
    let inspect_c=build.join("inspect.c");let inspect_exe=build.join("inspect.exe");std::fs::write(&inspect_c,DISCOVERY_C).map_err(|e|BindError::Io(format!("could not write COM type-library inspector: {e}")))?;
    run(Command::new("cc").args(["-std=c11","-municode"]).arg(&inspect_c).args(["-loleaut32","-lole32","-o"]).arg(&inspect_exe),"cc")?;
    let mut command=Command::new(&inspect_exe);match input{TypeLibraryInput::File(path)=>{let path=std::fs::canonicalize(path).map_err(|e|BindError::Io(format!("could not resolve the COM type library: {e}")))?;command.arg("file").arg(path);},TypeLibraryInput::Registered{guid,major,minor,lcid}=>{validate_guid(guid)?;command.args(["reg",guid,&major.to_string(),&minor.to_string(),&lcid.to_string()]);}}
    let metadata=run_capture(&mut command,"COM type-library inspector")?;let schema=parse_schema(&metadata)?;let bridge=build.join(format!("jet_com_{lib}.c"));let object=build.join(format!("jet_com_{lib}.o"));std::fs::write(&bridge,render_c(lib,&schema)).map_err(|e|BindError::Io(format!("could not write COM automation bridge: {e}")))?;run(Command::new("cc").args(["-std=c11","-c"]).arg(&bridge).arg("-o").arg(&object),"cc")?;
    let archive=cache.join(format!("libjet_com_{lib}.a"));let _=std::fs::remove_file(&archive);run(Command::new("ar").arg("rcs").arg(&archive).arg(&object),"ar")?;let source=render_jet(lib,&schema);let mut identity=b"jet-com-bind-v1\0".to_vec();identity.extend_from_slice(&metadata);identity.extend_from_slice(source.as_bytes());let provenance=format!("schema=jet-com-bind-v1\nsha256={}\ntype_library_name={}\ntype_library={}\nclass={}\nroot_interface={}\n",crate::SHA256::sha256_hex(&identity),schema.name,schema.guid,schema.class_guid,schema.root_interface);let methods=schema.methods.iter().map(method_name).collect();let _=std::fs::remove_dir_all(&build);Ok(BindResult{source,archive,provenance,methods})
}

fn parse_schema(bytes:&[u8])->Result<Schema,BindError>{
    let text=std::str::from_utf8(bytes).map_err(|_|BindError::Source("the COM inspector returned non-UTF-8 metadata".into()))?;let mut name=None;let mut guid=None;let mut class=None;let mut root=None;let mut methods=Vec::new();
    for line in text.lines(){let fields=line.split('\t').collect::<Vec<_>>();match fields.first().copied(){
        Some("LIB")if fields.len()==3=>{name=Some(fields[1].to_string());guid=Some(fields[2].to_string())},
        Some("CLASS")if fields.len()==3=>{class=Some(fields[1].to_string());root=Some(project(fields[2])?)},
        Some("METHOD")if fields.len()>=6=>{let interface=project(fields[1])?;let raw=fields[2];let dispid=fields[3].parse().map_err(|_|BindError::Source(format!("COM member `{raw}` has an invalid DISPID")))?;let flags:u16=fields[4].parse().map_err(|_|BindError::Source(format!("COM member `{raw}` has invalid invocation flags")))?;let mut jet=project(raw)?;if methods.iter().any(|m:&Method|m.interface.eq_ignore_ascii_case(&interface)&&m.jet.eq_ignore_ascii_case(&jet)){jet.push_str(if flags&2!=0{"_get"}else if flags&12!=0{"_set"}else{"_call"});}if methods.iter().any(|m:&Method|m.interface.eq_ignore_ascii_case(&interface)&&m.jet.eq_ignore_ascii_case(&jet)){return Err(BindError::Source(format!("COM member `{raw}` collides with generated Jet member `{interface}.{jet}`")))}let result=Kind::parse(fields[5]).ok_or_else(||BindError::Source(format!("COM member `{raw}` has unsupported result type `{}`",fields[5])))?;let mut params=Vec::new();for value in &fields[6..]{let Some((n,k))=value.split_once(':')else{return Err(BindError::Source(format!("COM member `{raw}` has malformed parameter metadata")))};params.push(Param{name:project(n)?,kind:Kind::parse(k).ok_or_else(||BindError::Source(format!("COM member `{raw}` has unsupported parameter type `{k}`")))?});}methods.push(Method{interface,jet,dispid,flags,result,params})},
        Some("SKIP")=>{},Some(tag)=>return Err(BindError::Source(format!("the COM inspector returned unknown `{tag}` metadata"))),None=>{}
    }}
    if methods.is_empty(){return Err(BindError::Source("the type library has no safely bindable IDispatch members".into()))}Ok(Schema{name:name.ok_or_else(||BindError::Source("the COM inspector omitted the library identity".into()))?,guid:guid.ok_or_else(||BindError::Source("the COM inspector omitted the library GUID".into()))?,class_guid:class.ok_or_else(||BindError::Source("the type library has no creatable COM class".into()))?,root_interface:root.ok_or_else(||BindError::Source("the COM class has no default automation interface".into()))?,methods})
}

fn method_name(m:&Method)->String{format!("{}_{}",m.interface,m.jet)}

fn interfaces(s:&Schema)->Vec<String>{
    let mut out=vec![s.root_interface.clone()];
    for m in &s.methods{
        if !out.contains(&m.interface){out.push(m.interface.clone())}
        for kind in std::iter::once(&m.result).chain(m.params.iter().map(|p|&p.kind)){
            if let Kind::Object(name)=kind{if !out.contains(name){out.push(name.clone())}}
        }
    }
    out
}

fn render_jet(lib:&str,s:&Schema)->String{
    let abi=format!("jet_com_{lib}");
    let mut o=format!("#Extern module c.{abi} {{\n    fn open() -> Int = \"{abi}_open\"\n    fn take_error() -> Int = \"{abi}_take_error\"\n    fn close(handle: Int) = \"{abi}_close\"\n    fn dynamic(handle: Int, name: String, args: String, flags: Int) -> String = \"{abi}_dynamic\"\n");
    for m in &s.methods{
        let name=method_name(m);o.push_str(&format!("    fn {name}(handle: Int"));
        for p in &m.params{o.push_str(&format!(", {}: {}",p.name,p.kind.jet()))}
        o.push(')');if m.result!=Kind::Unit{o.push_str(&format!(" -> {}",m.result.jet()))}
        o.push_str(&format!(" = \"{abi}_{name}\"\n"));
    }
    o.push_str(&format!("}}\nuse c.{abi} as abi\nuse core.encoding.json as json\n\n"));
    for interface in interfaces(s){o.push_str(&format!("pub struct {interface} {{ value: Int }}\n"))}
    o.push_str("pub enum ComError { WrongApartment InvalidHandle InvalidArgument MemberFailed TypeMismatch Limit }\n\n");
    o.push_str(&format!("pub fn open() -> {} ? ComError {{\n    value :: abi.open()\n    code :: abi.take_error()\n    if code != 0 {{ return err(error(code)) }}\n    return ok({}.{{ value: value }})\n}}\n\n",s.root_interface,s.root_interface));
    for interface in interfaces(s){
        o.push_str(&format!("pub fn close_{interface}(object: ^{interface}) -> Void ? ComError {{\n    abi.close(object.value)\n    code :: abi.take_error()\n    if code != 0 {{ return err(error(code)) }}\n    return ok(Void)\n}}\n\n"));
        o.push_str(&format!("#Unsafe(\"dynamic IDispatch has no type-library contract\") pub fn dynamic_{interface}(object: {interface}, name: String, args: [DataTree], flags: Int) -> DataTree ? ComError {{\n    raw :: abi.dynamic(object.value, name, json.to_string(args), flags)\n    code :: abi.take_error()\n    if code != 0 {{ return err(error(code)) }}\n    value := json.parse(raw) ?? return err(ComError.TypeMismatch)\n    return ok(value)\n}}\n\n"));
    }
    o.push_str("fn error(code: Int) -> ComError {\n    if code == 1 { return ComError.WrongApartment }\n    if code == 2 { return ComError.InvalidHandle }\n    if code == 3 { return ComError.InvalidArgument }\n    if code == 5 { return ComError.TypeMismatch }\n    if code == 6 { return ComError.Limit }\n    return ComError.MemberFailed\n}\n\n");
    for m in &s.methods{
        let name=method_name(m);o.push_str(&format!("pub fn {name}(object: {}",m.interface));
        for p in &m.params{o.push_str(&format!(", {}: {}",p.name,p.kind.jet()))}
        o.push_str(&format!(") -> {} ? ComError {{\n    ",m.result.jet()));
        if m.result!=Kind::Unit{o.push_str("value :: ")}o.push_str(&format!("abi.{name}(object.value"));
        for p in &m.params{o.push_str(&format!(", {}",p.name))}
        o.push_str(")\n    code :: abi.take_error()\n    if code != 0 { return err(error(code)) }\n");
        if m.result==Kind::Unit{o.push_str("    return ok(Void)\n")}else{o.push_str("    return ok(value)\n")}
        o.push_str("}\n\n");
    }
    o
}

fn render_c(lib:&str,s:&Schema)->String{let abi=format!("jet_com_{lib}");let mut wrappers=String::new();for m in &s.methods{let name=method_name(m);wrappers.push_str(m.result.c());wrappers.push(' ');wrappers.push_str(&format!("{abi}_{name}(int64_t handle"));for p in &m.params{wrappers.push_str(&format!(",{} {}",p.kind.c(),p.name))}wrappers.push_str("){failed=0;VARIANT args[");wrappers.push_str(&m.params.len().max(1).to_string());wrappers.push_str("];for(size_t i=0;i<sizeof(args)/sizeof(args[0]);i++)VariantInit(&args[i]);VARIANT result;VariantInit(&result);");for(i,p)in m.params.iter().enumerate(){let at=m.params.len()-1-i;let set=match &p.kind{Kind::Int=>format!("V_VT(&args[{at}])=VT_I8;V_I8(&args[{at}])={};",p.name),Kind::Float=>format!("V_VT(&args[{at}])=VT_R8;V_R8(&args[{at}])={};",p.name),Kind::Bool=>format!("V_VT(&args[{at}])=VT_BOOL;V_BOOL(&args[{at}])={}?VARIANT_TRUE:VARIANT_FALSE;",p.name),Kind::Text=>format!("if(!text_arg({},&args[{at}]))goto bad;",p.name),Kind::Data=>format!("if(!json_arg({},&args[{at}]))goto bad;",p.name),Kind::Object(_)=>format!("if(!object_arg({},&args[{at}]))goto bad;",p.name),Kind::Unit=>String::new()};wrappers.push_str(&set)}wrappers.push_str(&format!("if(!invoke_member(handle,{}, {},args,{},&result))goto bad;",m.dispid,m.flags,m.params.len()));let ret=match &m.result{Kind::Unit=>"VariantClear(&result);clear_args(args,sizeof(args)/sizeof(args[0]));return;",Kind::Int=>"int64_t value=variant_int(&result);VariantClear(&result);clear_args(args,sizeof(args)/sizeof(args[0]));return value;",Kind::Float=>"double value=variant_float(&result);VariantClear(&result);clear_args(args,sizeof(args)/sizeof(args[0]));return value;",Kind::Bool=>"int64_t value=variant_bool(&result);VariantClear(&result);clear_args(args,sizeof(args)/sizeof(args[0]));return value;",Kind::Text=>"const char*value=variant_text(&result);VariantClear(&result);clear_args(args,sizeof(args)/sizeof(args[0]));return value;",Kind::Data=>"const char*value=variant_json(&result);VariantClear(&result);clear_args(args,sizeof(args)/sizeof(args[0]));return value;",Kind::Object(_)=>"int64_t value=variant_object(&result);VariantClear(&result);clear_args(args,sizeof(args)/sizeof(args[0]));return value;"};wrappers.push_str(ret);wrappers.push_str("bad:VariantClear(&result);clear_args(args,sizeof(args)/sizeof(args[0]));");wrappers.push_str(match &m.result{Kind::Unit=>"return;}\n",Kind::Float=>"return 0.0;}\n",Kind::Text|Kind::Data=>"return \"\";}\n",_=>"return 0;}\n"});}RUNTIME_C.replace("@ABI@",&abi).replace("@CLASS@",&s.class_guid).replace("@WRAPPERS@",&wrappers)}

const RUNTIME_C:&str=r#"#define COBJMACROS
#include <windows.h>
#include <oleauto.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#define SLOTS 256
#define LIMIT 1048576
typedef struct { IDispatch* value; DWORD owner; uint32_t generation; } Slot;
static Slot slots[SLOTS]; static CRITICAL_SECTION lock; static INIT_ONCE once=INIT_ONCE_STATIC_INIT;
static _Thread_local int64_t failed; static _Thread_local char text[LIMIT+1];
static BOOL CALLBACK initialize(PINIT_ONCE o,PVOID p,PVOID*c){(void)o;(void)p;(void)c;InitializeCriticalSection(&lock);return TRUE;}
static void ready(void){InitOnceExecuteOnce(&once,initialize,0,0);}
static void fail_hr(HRESULT hr){if(hr==RPC_E_CHANGED_MODE)failed=1;else if(hr==E_INVALIDARG||hr==DISP_E_BADPARAMCOUNT||hr==DISP_E_PARAMNOTOPTIONAL)failed=3;else if(hr==DISP_E_TYPEMISMATCH||hr==DISP_E_OVERFLOW)failed=5;else if(hr==E_OUTOFMEMORY)failed=6;else failed=4;}
static int64_t store(IDispatch*v){if(!v){failed=5;return 0;}HRESULT hr=CoInitializeEx(0,COINIT_APARTMENTTHREADED);if(FAILED(hr)){fail_hr(hr);return 0;}ready();EnterCriticalSection(&lock);for(int i=0;i<SLOTS;i++)if(!slots[i].value){slots[i].generation++;if(!slots[i].generation)slots[i].generation=1;slots[i].value=v;IDispatch_AddRef(v);slots[i].owner=GetCurrentThreadId();int64_t h=((int64_t)slots[i].generation<<16)|(i+1);LeaveCriticalSection(&lock);return h;}LeaveCriticalSection(&lock);CoUninitialize();failed=6;return 0;}
static IDispatch* get(int64_t h){int i=(int)(h&65535)-1;uint32_t g=(uint32_t)((uint64_t)h>>16);if(i<0||i>=SLOTS){failed=2;return 0;}ready();EnterCriticalSection(&lock);IDispatch*v=slots[i].value;if(!v||slots[i].generation!=g){LeaveCriticalSection(&lock);failed=2;return 0;}if(slots[i].owner!=GetCurrentThreadId()){LeaveCriticalSection(&lock);failed=1;return 0;}IDispatch_AddRef(v);LeaveCriticalSection(&lock);return v;}
int64_t @ABI@_take_error(void){int64_t v=failed;failed=0;return v;}
int64_t @ABI@_open(void){failed=0;HRESULT hr=CoInitializeEx(0,COINIT_APARTMENTTHREADED);if(FAILED(hr)){fail_hr(hr);return 0;}CLSID clsid;hr=CLSIDFromString(L"@CLASS@",&clsid);if(FAILED(hr)){CoUninitialize();fail_hr(hr);return 0;}IDispatch*v=0;hr=CoCreateInstance(&clsid,0,CLSCTX_LOCAL_SERVER|CLSCTX_INPROC_SERVER,&IID_IDispatch,(void**)&v);if(FAILED(hr)){CoUninitialize();fail_hr(hr);return 0;}int64_t h=store(v);IDispatch_Release(v);CoUninitialize();return h;}
void @ABI@_close(int64_t h){failed=0;int i=(int)(h&65535)-1;uint32_t g=(uint32_t)((uint64_t)h>>16);ready();if(i<0||i>=SLOTS){failed=2;return;}EnterCriticalSection(&lock);if(!slots[i].value||slots[i].generation!=g){LeaveCriticalSection(&lock);failed=2;return;}if(slots[i].owner!=GetCurrentThreadId()){LeaveCriticalSection(&lock);failed=1;return;}IDispatch*v=slots[i].value;slots[i].value=0;slots[i].owner=0;LeaveCriticalSection(&lock);IDispatch_Release(v);CoUninitialize();}
static void clear_args(VARIANT*a,size_t n){for(size_t i=0;i<n;i++)VariantClear(&a[i]);}
static int invoke_member(int64_t h,DISPID id,WORD flags,VARIANT*a,UINT n,VARIANT*r){IDispatch*v=get(h);if(!v)return 0;DISPID named=DISPID_PROPERTYPUT;DISPPARAMS p={a,flags&(DISPATCH_PROPERTYPUT|DISPATCH_PROPERTYPUTREF)?&named:0,n,flags&(DISPATCH_PROPERTYPUT|DISPATCH_PROPERTYPUTREF)?1:0};EXCEPINFO ex={0};UINT bad=0;HRESULT hr=IDispatch_Invoke(v,id,&IID_NULL,LOCALE_USER_DEFAULT,flags,&p,r,&ex,&bad);IDispatch_Release(v);if(ex.bstrSource)SysFreeString(ex.bstrSource);if(ex.bstrDescription)SysFreeString(ex.bstrDescription);if(ex.bstrHelpFile)SysFreeString(ex.bstrHelpFile);if(FAILED(hr)){fail_hr(hr);return 0;}return 1;}
static BSTR utf8_bstr(const char*s){if(!s){failed=3;return 0;}int n=MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,s,-1,0,0);if(n<1){failed=3;return 0;}BSTR b=SysAllocStringLen(0,n-1);if(!b){failed=6;return 0;}if(!MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,s,-1,b,n)){SysFreeString(b);failed=3;return 0;}return b;}
static int text_arg(const char*s,VARIANT*v){BSTR b=utf8_bstr(s);if(!b)return 0;V_VT(v)=VT_BSTR;V_BSTR(v)=b;return 1;}
static int object_arg(int64_t h,VARIANT*v){IDispatch*d=get(h);if(!d)return 0;V_VT(v)=VT_DISPATCH;V_DISPATCH(v)=d;return 1;}
typedef struct{const char*p;const char*e;} Json;
static void ws(Json*j){while(j->p<j->e&&(*j->p==' '||*j->p=='\n'||*j->p=='\r'||*j->p=='\t'))j->p++;}
static int json_string(Json*j,BSTR*out){if(j->p>=j->e||*j->p++!='\"')return 0;char*buf=malloc((size_t)(j->e-j->p)+1);if(!buf){failed=6;return 0;}size_t n=0;while(j->p<j->e&&*j->p!='\"'){unsigned char c=(unsigned char)*j->p++;if(c=='\\'){if(j->p>=j->e){free(buf);return 0;}c=(unsigned char)*j->p++;if(c=='n')c='\n';else if(c=='r')c='\r';else if(c=='t')c='\t';else if(c!='\\'&&c!='\"'&&c!='/'){free(buf);return 0;}}if(c<0x20){free(buf);return 0;}buf[n++]=(char)c;}if(j->p>=j->e){free(buf);return 0;}j->p++;buf[n]=0;*out=utf8_bstr(buf);free(buf);return *out!=0;}
static int json_value(Json*j,VARIANT*v,int depth){if(depth>64){failed=6;return 0;}ws(j);if(j->p>=j->e)return 0;if(*j->p=='\"'){BSTR b=0;if(!json_string(j,&b))return 0;V_VT(v)=VT_BSTR;V_BSTR(v)=b;return 1;}if(j->e-j->p>=4&&!memcmp(j->p,"null",4)){j->p+=4;V_VT(v)=VT_NULL;return 1;}if(j->e-j->p>=4&&!memcmp(j->p,"true",4)){j->p+=4;V_VT(v)=VT_BOOL;V_BOOL(v)=VARIANT_TRUE;return 1;}if(j->e-j->p>=5&&!memcmp(j->p,"false",5)){j->p+=5;V_VT(v)=VT_BOOL;V_BOOL(v)=VARIANT_FALSE;return 1;}if(*j->p=='['){j->p++;VARIANT*items=0;size_t n=0,cap=0;ws(j);while(j->p<j->e&&*j->p!=']'){if(n==cap){size_t nc=cap?cap*2:8;VARIANT*q=realloc(items,nc*sizeof(VARIANT));if(!q){clear_args(items,n);free(items);failed=6;return 0;}items=q;cap=nc;}VariantInit(&items[n]);if(!json_value(j,&items[n],depth+1)){clear_args(items,n+1);free(items);return 0;}n++;ws(j);if(j->p<j->e&&*j->p==','){j->p++;ws(j);continue;}break;}if(j->p>=j->e||*j->p++!=']'){clear_args(items,n);free(items);return 0;}SAFEARRAYBOUND b={(ULONG)n,0};SAFEARRAY*a=SafeArrayCreate(VT_VARIANT,1,&b);if(!a){clear_args(items,n);free(items);failed=6;return 0;}for(LONG i=0;i<(LONG)n;i++)if(FAILED(SafeArrayPutElement(a,&i,&items[i]))){SafeArrayDestroy(a);clear_args(items,n);free(items);return 0;}clear_args(items,n);free(items);V_VT(v)=VT_ARRAY|VT_VARIANT;V_ARRAY(v)=a;return 1;}char*end=0;double d=strtod(j->p,&end);if(end==j->p)return 0;int integral=1;for(const char*q=j->p;q<end;q++)if(*q=='.'||*q=='e'||*q=='E')integral=0;j->p=end;if(integral){V_VT(v)=VT_I8;V_I8(v)=(LONGLONG)d;}else{V_VT(v)=VT_R8;V_R8(v)=d;}return 1;}
static int json_arg(const char*s,VARIANT*v){if(!s||strlen(s)>LIMIT){failed=6;return 0;}Json j={s,s+strlen(s)};if(!json_value(&j,v,0)){if(!failed)failed=3;return 0;}ws(&j);if(j.p!=j.e){VariantClear(v);failed=3;return 0;}return 1;}
static int change(VARIANT*in,VARTYPE vt,VARIANT*out){VariantInit(out);HRESULT hr=VariantChangeType(out,in,0,vt);if(FAILED(hr)){fail_hr(hr);return 0;}return 1;}
static int64_t variant_int(VARIANT*v){VARIANT x;if(!change(v,VT_I8,&x))return 0;int64_t n=V_I8(&x);VariantClear(&x);return n;}
static double variant_float(VARIANT*v){VARIANT x;if(!change(v,VT_R8,&x))return 0;double n=V_R8(&x);VariantClear(&x);return n;}
static int64_t variant_bool(VARIANT*v){VARIANT x;if(!change(v,VT_BOOL,&x))return 0;int64_t n=V_BOOL(&x)!=VARIANT_FALSE;VariantClear(&x);return n;}
static const char* bstr_text(BSTR b){int n=WideCharToMultiByte(CP_UTF8,WC_ERR_INVALID_CHARS,b,SysStringLen(b),0,0,0,0);if(n<0||n>LIMIT){failed=6;text[0]=0;return text;}WideCharToMultiByte(CP_UTF8,WC_ERR_INVALID_CHARS,b,SysStringLen(b),text,n,0,0);text[n]=0;return text;}
static const char* variant_text(VARIANT*v){VARIANT x;if(!change(v,VT_BSTR,&x)){text[0]=0;return text;}bstr_text(V_BSTR(&x));VariantClear(&x);return text;}
static int64_t variant_object(VARIANT*v){IDispatch*d=0;if(V_VT(v)==VT_DISPATCH)d=V_DISPATCH(v);else if(V_VT(v)==VT_UNKNOWN&&V_UNKNOWN(v))IUnknown_QueryInterface(V_UNKNOWN(v),&IID_IDispatch,(void**)&d);else{failed=5;return 0;}int64_t h=store(d);if(V_VT(v)==VT_UNKNOWN&&d)IDispatch_Release(d);return h;}
typedef struct{char*p;size_t n;} Out;
static int put(Out*o,const char*s,size_t n){if(o->n+n>LIMIT){failed=6;return 0;}memcpy(o->p+o->n,s,n);o->n+=n;return 1;}
static int encode_variant(Out*o,VARIANT*v,int depth);
static int encode_string(Out*o,BSTR b){if(!put(o,"\"",1))return 0;const char*s=bstr_text(b);if(failed)return 0;for(const unsigned char*p=(const unsigned char*)s;*p;p++){char q[2]={'\\',0};if(*p=='\"'||*p=='\\'){q[1]=(char)*p;if(!put(o,q,2))return 0;}else if(*p=='\n'){if(!put(o,"\\n",2))return 0;}else if(*p=='\r'){if(!put(o,"\\r",2))return 0;}else if(*p=='\t'){if(!put(o,"\\t",2))return 0;}else if(*p<0x20){failed=5;return 0;}else if(!put(o,(const char*)p,1))return 0;}return put(o,"\"",1);}
static int encode_array(Out*o,SAFEARRAY*a,int depth){if(SafeArrayGetDim(a)!=1){failed=5;return 0;}LONG lo=0,hi=-1;SafeArrayGetLBound(a,1,&lo);SafeArrayGetUBound(a,1,&hi);if(!put(o,"[",1))return 0;VARTYPE vt=VT_EMPTY;SafeArrayGetVartype(a,&vt);for(LONG i=lo;i<=hi;i++){if(i>lo&&!put(o,",",1))return 0;VARIANT x;VariantInit(&x);if(vt==VT_VARIANT){if(FAILED(SafeArrayGetElement(a,&i,&x))){failed=4;return 0;}}else if(vt==VT_BSTR){V_VT(&x)=VT_BSTR;if(FAILED(SafeArrayGetElement(a,&i,&V_BSTR(&x)))){failed=4;return 0;}}else if(vt==VT_I4){V_VT(&x)=VT_I4;if(FAILED(SafeArrayGetElement(a,&i,&V_I4(&x)))){failed=4;return 0;}}else if(vt==VT_I8){V_VT(&x)=VT_I8;if(FAILED(SafeArrayGetElement(a,&i,&V_I8(&x)))){failed=4;return 0;}}else if(vt==VT_R8){V_VT(&x)=VT_R8;if(FAILED(SafeArrayGetElement(a,&i,&V_R8(&x)))){failed=4;return 0;}}else if(vt==VT_BOOL){V_VT(&x)=VT_BOOL;if(FAILED(SafeArrayGetElement(a,&i,&V_BOOL(&x)))){failed=4;return 0;}}else{failed=5;return 0;}int ok=encode_variant(o,&x,depth+1);VariantClear(&x);if(!ok)return 0;}return put(o,"]",1);}
static int encode_variant(Out*o,VARIANT*v,int depth){if(depth>64){failed=6;return 0;}VARIANT x;VariantInit(&x);if(FAILED(VariantCopyInd(&x,v))){failed=5;return 0;}VARTYPE vt=V_VT(&x);int ok=0;char num[64];if(vt==VT_EMPTY||vt==VT_NULL)ok=put(o,"null",4);else if(vt==VT_BOOL)ok=put(o,V_BOOL(&x)?"true":"false",V_BOOL(&x)?4:5);else if(vt==VT_BSTR)ok=encode_string(o,V_BSTR(&x));else if(vt&VT_ARRAY)ok=encode_array(o,V_ARRAY(&x),depth);else if(vt==VT_I1||vt==VT_I2||vt==VT_I4||vt==VT_I8||vt==VT_UI1||vt==VT_UI2||vt==VT_UI4||vt==VT_UI8){VARIANT n;if(change(&x,VT_I8,&n)){int z=snprintf(num,sizeof(num),"%lld",(long long)V_I8(&n));VariantClear(&n);ok=z>0&&put(o,num,(size_t)z);}}else if(vt==VT_R4||vt==VT_R8){VARIANT n;if(change(&x,VT_R8,&n)){int z=snprintf(num,sizeof(num),"%.17g",V_R8(&n));VariantClear(&n);ok=z>0&&put(o,num,(size_t)z);}}else failed=5;VariantClear(&x);return ok;}
static const char* variant_json(VARIANT*v){Out o={text,0};if(!encode_variant(&o,v,0)){text[0]=0;return text;}text[o.n]=0;return text;}
const char* @ABI@_dynamic(int64_t h,const char*name,const char*json,int64_t flags){failed=0;text[0]=0;if(flags<1||(flags&~(DISPATCH_METHOD|DISPATCH_PROPERTYGET|DISPATCH_PROPERTYPUT|DISPATCH_PROPERTYPUTREF))){failed=3;return text;}VARIANT packed;VariantInit(&packed);if(!json_arg(json,&packed))return text;if(V_VT(&packed)!=(VT_ARRAY|VT_VARIANT)||SafeArrayGetDim(V_ARRAY(&packed))!=1){VariantClear(&packed);failed=3;return text;}LONG lo=0,hi=-1;SafeArrayGetLBound(V_ARRAY(&packed),1,&lo);SafeArrayGetUBound(V_ARRAY(&packed),1,&hi);size_t n=hi>=lo?(size_t)(hi-lo+1):0;VARIANT*args=calloc(n?n:1,sizeof(VARIANT));if(!args){VariantClear(&packed);failed=6;return text;}for(size_t i=0;i<n;i++){VariantInit(&args[i]);LONG at=hi-(LONG)i;if(FAILED(SafeArrayGetElement(V_ARRAY(&packed),&at,&args[i]))){clear_args(args,n);free(args);VariantClear(&packed);failed=4;return text;}}BSTR wide=utf8_bstr(name);IDispatch*d=get(h);DISPID id=0;HRESULT hr=(wide&&d)?IDispatch_GetIDsOfNames(d,&IID_NULL,&wide,1,LOCALE_USER_DEFAULT,&id):E_INVALIDARG;if(wide)SysFreeString(wide);if(d)IDispatch_Release(d);VARIANT result;VariantInit(&result);if(FAILED(hr)){fail_hr(hr);}else if(invoke_member(h,id,(WORD)flags,args,(UINT)n,&result)){variant_json(&result);}VariantClear(&result);clear_args(args,n);free(args);VariantClear(&packed);return text;}
@WRAPPERS@
"#;

const DISCOVERY_C:&str=r#"#define COBJMACROS
#include <windows.h>
#include <oleauto.h>
#include <stdio.h>
#include <stdlib.h>
#include <wchar.h>
static void utf8(BSTR b,char*out,size_t cap){if(!b||WideCharToMultiByte(CP_UTF8,WC_ERR_INVALID_CHARS,b,-1,out,(int)cap,0,0)<1)out[0]=0;for(char*p=out;*p;p++)if(*p=='\t'||*p=='\r'||*p=='\n')*p='_';}
static void guid_text(REFGUID g,char*out,size_t cap){wchar_t w[40];StringFromGUID2(g,w,40);WideCharToMultiByte(CP_UTF8,0,w,-1,out,(int)cap,0,0);}
static int info_name(ITypeInfo*info,char*out,size_t cap){BSTR b=0;HRESULT hr=ITypeInfo_GetDocumentation(info,MEMBERID_NIL,&b,0,0,0);if(FAILED(hr)||!b){out[0]=0;return 0;}utf8(b,out,cap);SysFreeString(b);return out[0]!=0;}
static int type_token(TYPEDESC*t,ITypeInfo*owner,char*out,size_t cap){
 VARTYPE v=t->vt&~VT_BYREF;if(v&VT_ARRAY){strcpy(out,"data");return 1;}
 switch(v){case VT_EMPTY:case VT_VOID:strcpy(out,"unit");return 1;case VT_I1:case VT_I2:case VT_I4:case VT_I8:case VT_UI1:case VT_UI2:case VT_UI4:case VT_UI8:case VT_INT:case VT_UINT:strcpy(out,"int");return 1;case VT_R4:case VT_R8:strcpy(out,"float");return 1;case VT_BOOL:strcpy(out,"bool");return 1;case VT_BSTR:strcpy(out,"text");return 1;case VT_VARIANT:case VT_SAFEARRAY:strcpy(out,"data");return 1;case VT_DISPATCH:case VT_UNKNOWN:strcpy(out,"object=Object");return 1;case VT_PTR:return type_token(t->lptdesc,owner,out,cap);case VT_USERDEFINED:{ITypeInfo*r=0;TYPEATTR*a=0;if(FAILED(ITypeInfo_GetRefTypeInfo(owner,t->hreftype,&r)))return 0;if(FAILED(ITypeInfo_GetTypeAttr(r,&a))){ITypeInfo_Release(r);return 0;}int ok=0;if(a->typekind==TKIND_ENUM){strcpy(out,"int");ok=1;}else if(a->typekind==TKIND_DISPATCH||a->typekind==TKIND_INTERFACE||a->typekind==TKIND_COCLASS){char name[512];if(info_name(r,name,sizeof(name))&&strlen(name)+8<cap){snprintf(out,cap,"object=%s",name);ok=1;}}ITypeInfo_ReleaseTypeAttr(r,a);ITypeInfo_Release(r);return ok;}default:return 0;}
}
static int default_interface(ITypeInfo*coclass,TYPEATTR*a,char*out,size_t cap){for(UINT i=0;i<a->cImplTypes;i++){INT flags=0;HREFTYPE ref=0;ITypeInfo*info=0;if(FAILED(ITypeInfo_GetImplTypeFlags(coclass,i,&flags))||!(flags&IMPLTYPEFLAG_FDEFAULT)||(flags&IMPLTYPEFLAG_FSOURCE))continue;if(FAILED(ITypeInfo_GetRefTypeOfImplType(coclass,i,&ref))||FAILED(ITypeInfo_GetRefTypeInfo(coclass,ref,&info)))continue;int ok=info_name(info,out,cap);ITypeInfo_Release(info);if(ok)return 1;}return 0;}
int wmain(int argc,wchar_t**argv){
 ITypeLib*lib=0;HRESULT hr=OleInitialize(0);if(FAILED(hr))return 2;
 if(argc>=3&&!wcscmp(argv[1],L"file"))hr=LoadTypeLibEx(argv[2],REGKIND_NONE,&lib);else if(argc==6&&!wcscmp(argv[1],L"reg")){GUID g;if(FAILED(CLSIDFromString(argv[2],&g)))hr=E_INVALIDARG;else hr=LoadRegTypeLib(&g,(WORD)wcstoul(argv[3],0,10),(WORD)wcstoul(argv[4],0,10),(LCID)wcstoul(argv[5],0,10),&lib);}else hr=E_INVALIDARG;
 if(FAILED(hr)||!lib){fputs("COMError\n",stderr);OleUninitialize();return 3;}
 TLIBATTR*la=0;if(FAILED(ITypeLib_GetLibAttr(lib,&la))){ITypeLib_Release(lib);OleUninitialize();return 4;}BSTR library=0;ITypeLib_GetDocumentation(lib,-1,&library,0,0,0);char name[512],guid[64];utf8(library,name,sizeof(name));guid_text(&la->guid,guid,sizeof(guid));printf("LIB\t%s\t%s\n",name,guid);if(library)SysFreeString(library);ITypeLib_ReleaseTLibAttr(lib,la);
 UINT count=ITypeLib_GetTypeInfoCount(lib);int found_class=0,found_method=0;
 for(UINT i=0;i<count;i++){ITypeInfo*info=0;TYPEATTR*a=0;if(FAILED(ITypeLib_GetTypeInfo(lib,i,&info))||FAILED(ITypeInfo_GetTypeAttr(info,&a))){if(info)ITypeInfo_Release(info);continue;}
  if(a->typekind==TKIND_COCLASS&&!found_class&&(a->wTypeFlags&TYPEFLAG_FCANCREATE)){char cls[64],root[512];if(default_interface(info,a,root,sizeof(root))){guid_text(&a->guid,cls,sizeof(cls));printf("CLASS\t%s\t%s\n",cls,root);found_class=1;}}
  if(a->typekind==TKIND_DISPATCH){char interface_name[512];if(!info_name(info,interface_name,sizeof(interface_name))){ITypeInfo_ReleaseTypeAttr(info,a);ITypeInfo_Release(info);continue;}for(UINT f=0;f<a->cFuncs;f++){FUNCDESC*d=0;if(FAILED(ITypeInfo_GetFuncDesc(info,f,&d)))continue;if(d->wFuncFlags&(FUNCFLAG_FHIDDEN|FUNCFLAG_FRESTRICTED)){ITypeInfo_ReleaseFuncDesc(info,d);continue;}BSTR names[65]={0};UINT got=0;ITypeInfo_GetNames(info,d->memid,names,65,&got);char member[512];utf8(got?names[0]:0,member,sizeof(member));TYPEDESC*ret=&d->elemdescFunc.tdesc;int retval=-1,bad=member[0]==0;for(UINT p=0;p<d->cParams;p++){USHORT flags=d->lprgelemdescParam[p].paramdesc.wParamFlags;if(flags&PARAMFLAG_FRETVAL){retval=(int)p;ret=&d->lprgelemdescParam[p].tdesc;}else if(flags&PARAMFLAG_FOUT)bad=1;}char result[520];if(!type_token(ret,info,result,sizeof(result)))bad=1;char kinds[64][520];for(UINT p=0;p<d->cParams;p++)if((int)p!=retval&&!type_token(&d->lprgelemdescParam[p].tdesc,info,kinds[p],sizeof(kinds[p])))bad=1;if(!bad){printf("METHOD\t%s\t%s\t%ld\t%u\t%s",interface_name,member,(long)d->memid,(unsigned)d->invkind,result);for(UINT p=0;p<d->cParams;p++)if((int)p!=retval){char param[512];if(p+1<got)utf8(names[p+1],param,sizeof(param));else snprintf(param,sizeof(param),"arg%u",p+1);printf("\t%s:%s",param,kinds[p]);}putchar('\n');found_method=1;}for(UINT n=0;n<got;n++)if(names[n])SysFreeString(names[n]);ITypeInfo_ReleaseFuncDesc(info,d);}}
  ITypeInfo_ReleaseTypeAttr(info,a);ITypeInfo_Release(info);
 }
 ITypeLib_Release(lib);OleUninitialize();return found_class&&found_method?0:5;
}
"#;

#[cfg(test)]
mod tests{
    #[test]
    fn schema_generates_typed_stub_and_real_windows_automation(){let metadata=b"LIB\tOffice Fixture\t{00000000-0000-0000-0000-000000000001}\nCLASS\t{00000000-0000-0000-0000-000000000002}\tApplication\nMETHOD\tApplication\tWorkbooks\t41\t2\tobject=Workbooks\nMETHOD\tWorkbooks\tOpen-Book\t42\t1\tobject=Workbook\tpath:text\nMETHOD\tRange\tValues\t77\t2\tdata\n";let schema=super::parse_schema(metadata).unwrap();let jet=super::render_jet("office",&schema);assert!(jet.contains("pub fn open() -> Application ? ComError"));assert!(jet.contains("pub fn Application_Workbooks(object: Application) -> Workbooks ? ComError"));assert!(jet.contains("pub fn Workbooks_Open_Book(object: Workbooks, path: String) -> Workbook ? ComError"));assert!(jet.contains("pub fn Range_Values(object: Range) -> DataTree ? ComError"));assert!(jet.contains("#Unsafe(\"dynamic IDispatch has no type-library contract\") pub fn dynamic_Application"));let c=super::render_c("office",&schema);assert!(c.contains("jet_com_office_Workbooks_Open_Book"));for needle in ["CoInitializeEx","CoCreateInstance","IDispatch_Invoke","SafeArrayGetElement","VariantChangeType","IDispatch_Release","GetCurrentThreadId"]{assert!(c.contains(needle),"missing {needle}")}}
    #[cfg(not(target_os="windows"))]
    #[test]
    fn generated_windows_sources_cross_compile_with_winegcc(){
        if std::process::Command::new("winegcc").arg("--version").output().is_err(){return}
        let metadata=b"LIB\tOffice Fixture\t{00000000-0000-0000-0000-000000000001}\nCLASS\t{00000000-0000-0000-0000-000000000002}\tApplication\nMETHOD\tApplication\tTitle\t1\t2\ttext\n";
        let schema=super::parse_schema(metadata).unwrap();let dir=std::env::temp_dir().join(format!("jet-com-cross-{}",std::process::id()));let _=std::fs::remove_dir_all(&dir);std::fs::create_dir_all(&dir).unwrap();
        let inspector=dir.join("inspect.c");let runtime=dir.join("runtime.c");std::fs::write(&inspector,super::DISCOVERY_C).unwrap();std::fs::write(&runtime,super::render_c("office",&schema)).unwrap();
        for (source,object) in [(&inspector,"inspect.o"),(&runtime,"runtime.o")]{let output=std::process::Command::new("winegcc").args(["-std=gnu11","-c"]).arg(source).arg("-o").arg(dir.join(object)).output().unwrap();assert!(output.status.success(),"winegcc failed for {}: {}",source.display(),String::from_utf8_lossy(&output.stderr));}
        let _=std::fs::remove_dir_all(dir);
    }
    #[test]
    fn non_windows_host_is_rejected_before_tool_or_file_access(){if cfg!(target_os="windows"){return}let error=super::bind(&super::TypeLibraryInput::File("missing.tlb".into()),"office",std::path::Path::new("missing-cache")).unwrap_err();assert_eq!(error,super::BindError::UnsupportedHost)}
}

fn project(v:&str)->Result<String,BindError>{let out=v.replace('-',"_");if !ident(&out)||crate::Syntax::JET_KEYWORD_LIST.contains(&out.as_str())||crate::Syntax::JET_TYPE_LIST.contains(&out.as_str()){return Err(BindError::Source(format!("COM name `{v}` cannot be projected as a Jet identifier")))}Ok(out)}
fn ident(v:&str)->bool{let mut c=v.chars();matches!(c.next(),Some(x)if x.is_ascii_alphabetic()||x=='_')&&c.all(|x|x.is_ascii_alphanumeric()||x=='_')}
fn validate_guid(v:&str)->Result<(),BindError>{let bytes=v.as_bytes();if bytes.len()!=38||bytes[0]!=b'{'||bytes[37]!=b'}'||[9,14,19,24].iter().any(|i|bytes[*i]!=b'-')||bytes[1..37].iter().enumerate().any(|(i,b)|![8,13,18,23].contains(&i)&&!b.is_ascii_hexdigit()){return Err(BindError::Source("the registered COM type-library GUID is invalid".into()))}Ok(())}

fn run_capture(command:&mut Command,tool:&'static str)->Result<Vec<u8>,BindError>{command.stdout(Stdio::piped()).stderr(Stdio::piped());let mut child=command.spawn().map_err(|e|if e.kind()==std::io::ErrorKind::NotFound{BindError::ToolMissing(tool)}else{BindError::Io(format!("could not start `{tool}`: {e}"))})?;let stdout=child.stdout.take().unwrap();let stderr=child.stderr.take().unwrap();let out=std::thread::spawn(move||drain(stdout));let err=std::thread::spawn(move||drain(stderr));let status=wait(&mut child,tool)?;let stdout=out.join().map_err(|_|BindError::Io(format!("`{tool}` stdout reader failed")))??;let stderr=err.join().map_err(|_|BindError::Io(format!("`{tool}` stderr reader failed")))??;if status.success(){Ok(stdout)}else{Err(BindError::ToolFailed(tool,launder(&stderr)))}}
fn run(command:&mut Command,tool:&'static str)->Result<(),BindError>{let _=run_capture(command,tool)?;Ok(())}
fn wait(child:&mut std::process::Child,tool:&'static str)->Result<std::process::ExitStatus,BindError>{let end=Instant::now()+Duration::from_secs(60);loop{if let Some(v)=child.try_wait().map_err(|e|BindError::Io(format!("could not supervise `{tool}`: {e}")))?{return Ok(v)}if Instant::now()>=end{let _=child.kill();let _=child.wait();return Err(BindError::ToolFailed(tool,"the tool exceeded the 60 second limit".into()))}std::thread::sleep(Duration::from_millis(10))}}
fn drain(mut input:impl Read)->Result<Vec<u8>,BindError>{let mut out=Vec::new();let mut buf=[0;8192];loop{let n=input.read(&mut buf).map_err(|e|BindError::Io(format!("could not read COM tool output: {e}")))?;if n==0{break}if out.len()<65536{out.extend_from_slice(&buf[..n.min(65536-out.len())])}}Ok(out)}
fn launder(v:&[u8])->String{String::from_utf8_lossy(v).lines().map(str::trim).find(|s|!s.is_empty()).map(|_|"the Windows COM operation failed".into()).unwrap_or_else(||"the Windows COM operation failed".into())}
