//! In-process Lua binding generator (D-FFI-LUA1=A).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindResult {
    pub source: String,
    pub bound: Vec<String>,
    pub archive: PathBuf,
    pub lib_dir: PathBuf,
    pub provenance: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError {
    Source(String),
    ToolMissing(&'static str),
    ToolFailed(&'static str, String),
    IO(String),
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source(v) | Self::IO(v) => f.write_str(v),
            Self::ToolMissing(v) => write!(f, "the provisioned `{v}` tool was not found"),
            Self::ToolFailed(t, v) => write!(f, "`{t}` rejected the Lua binding input: {v}"),
        }
    }
}

pub fn bind(path: &Path, source: &str, lib: &str, cache: &Path) -> Result<BindResult, BindError> {
    if !ident(lib) {
        return Err(BindError::Source(format!("`{lib}` is not a valid Jet library name")));
    }
    let luac = tool_path("luac").ok_or(BindError::ToolMissing("luac"))?;
    if let Err(error) = run(Command::new(&luac).arg("-p").arg(path), "luac") {
        return Err(match error {
            BindError::ToolFailed(_, _) => BindError::Source("the Lua parser rejected the script".into()),
            other => other,
        });
    }
    let functions = discover(source)?;
    if functions.is_empty() {
        return Err(BindError::Source(
            "the script has no top-level `function name(input)` declarations".into(),
        ));
    }
    let root = tool_root(&luac).ok_or_else(|| BindError::Source("the provisioned Lua root could not be resolved".into()))?;
    let include = find_header_dir(&root).ok_or_else(|| BindError::Source("the provisioned Lua runtime has no `lua.h` header".into()))?;
    let lib_dir = find_library_dir(&root).ok_or_else(|| BindError::Source("the provisioned Lua runtime has no embeddable library".into()))?;
    std::fs::create_dir_all(cache).map_err(|e| BindError::IO(format!("could not create Lua binding cache: {e}")))?;
    let abi = format!("jet_lua_{lib}");
    let c = cache.join(format!("{abi}.c"));
    let object = cache.join(format!("{abi}.o"));
    let archive = cache.join(format!("lib{abi}.a"));
    std::fs::write(&c, render_c(&abi, source, &functions)).map_err(|e| BindError::IO(format!("could not write Lua bridge: {e}")))?;
    run(Command::new("cc").args(["-std=c11", "-D_POSIX_C_SOURCE=200809L", "-fPIC", "-c", "-I"]).arg(&include).arg(&c).arg("-o").arg(&object), "cc")?;
    let _ = std::fs::remove_file(&archive);
    run(Command::new("ar").arg("rcs").arg(&archive).arg(&object), "ar")?;
    let _ = std::fs::remove_file(&c);
    let _ = std::fs::remove_file(&object);
    let mut identity = b"jet-lua-bind-v2\0".to_vec();
    identity.extend_from_slice(source.as_bytes());
    identity.push(0);
    identity.extend_from_slice(root.to_string_lossy().as_bytes());
    let provenance = format!(
        "schema=jet-lua-bind-v2\nsha256={}\nruntime={}\nstate=per-session\ntransport=datatree+table-view\ntable-view=zero-copy\nhook=instructions\n",
        crate::SHA256::sha256_hex(&identity), root.display()
    );
    Ok(BindResult { source: render_jet(&abi, &functions), bound: functions, archive, lib_dir, provenance })
}

const GENERATED_FIXED_FUNCTIONS: &[(&str, &str)] = &[
    ("open", "() -> Int"),
    ("take_error", "() -> Int"),
    ("cancel", "(handle: Int)"),
    ("close", "(handle: Int)"),
    ("view_get_int", "(handle: Int, table: Int, key: String) -> Int"),
    ("view_set_int", "(handle: Int, table: Int, key: String, value: Int)"),
    ("view_release", "(handle: Int, table: Int)"),
];

fn render_jet(abi: &str, functions: &[String]) -> String {
    let mut out = format!("#Extern module c.{abi} {{\n");
    for (name, signature) in GENERATED_FIXED_FUNCTIONS {
        out.push_str(&format!("    fn {name}{signature} = \"{abi}_{name}\"\n"));
    }
    for name in functions {
        out.push_str(&format!("    fn {name}(handle: Int, input: String, deadline_ms: Int) => String = \"{abi}_invoke_{name}\"\n"));
        out.push_str(&format!("    fn {name}_view(handle: Int, deadline_ms: Int) => Int = \"{abi}_view_{name}\"\n"));
    }
    out.push_str(&format!("}}\nuse c.{abi} as abi\nuse core.encoding.json as json\n\npub struct Session {{ value: Int }}\npub struct TableView {{ session: Int, table: Int }}\npub enum LuaError {{ NotRunning Timeout Cancelled Protocol CommandFailed Limit }}\n\nimpl Session.Close {{\n    fn close(^self) {{ abi.close(self.value) }}\n}}\n\nimpl TableView.Close {{\n    fn close(^self) {{ abi.view_release(self.session, self.table) }}\n}}\n\npub fn open() => Session ? LuaError {{\n    handle :: abi.open()\n    if abi.take_error() != 0 {{ return Err(LuaError.NotRunning) }}\n    return Ok(Session.{{ value: handle }})\n}}\n\npub fn cancel(session: Session) {{ abi.cancel(session.value) }}\n\npub fn view_get_int(view: TableView, key: String) => Int ? LuaError {{\n    value :: abi.view_get_int(view.session, view.table, key)\n    code :: abi.take_error()\n    if code == 1 {{ return Err(LuaError.NotRunning) }}\n    if code != 0 {{ return Err(LuaError.Protocol) }}\n    return Ok(value)\n}}\n\npub fn view_set_int(view: TableView, key: String, value: Int) => Bool ? LuaError {{\n    abi.view_set_int(view.session, view.table, key, value)\n    code :: abi.take_error()\n    if code == 1 {{ return Err(LuaError.NotRunning) }}\n    if code != 0 {{ return Err(LuaError.Protocol) }}\n    return Ok(true)\n}}\n\n"));
    for name in functions {
        out.push_str(&format!("pub fn {name}(session: Session, input: DataTree, deadline_ms: Int) => DataTree ? LuaError {{\n    raw :: abi.{name}(session.value, json.to_string(input), deadline_ms)\n    code :: abi.take_error()\n    if code == 1 {{ return Err(LuaError.NotRunning) }}\n    if code == 2 {{ return Err(LuaError.Timeout) }}\n    if code == 3 {{ return Err(LuaError.Cancelled) }}\n    if code == 5 {{ return Err(LuaError.Limit) }}\n    if code == 4 {{ return Err(LuaError.CommandFailed) }}\n    if code != 0 {{ return Err(LuaError.Protocol) }}\n    value := json.parse(raw) ?? return Err(LuaError.Protocol)\n    return Ok(value)\n}}\n\npub fn {name}_typed<T: [Encode, Decode]>(session: Session, input: T, deadline_ms: Int) => T ? LuaError {{\n    tree := json.parse(json.to_string(input)) ?? return Err(LuaError.Protocol)\n    value := {name}(session, tree, deadline_ms)?\n    decoded := json.decode<T>(json.to_string(value)) ?? return Err(LuaError.Protocol)\n    return Ok(decoded)\n}}\n\npub fn {name}_view(session: Session, deadline_ms: Int) => TableView ? LuaError {{\n    table :: abi.{name}_view(session.value, deadline_ms)\n    code :: abi.take_error()\n    if code == 1 {{ return Err(LuaError.NotRunning) }}\n    if code == 2 {{ return Err(LuaError.Timeout) }}\n    if code == 3 {{ return Err(LuaError.Cancelled) }}\n    if code == 5 {{ return Err(LuaError.Limit) }}\n    if code == 4 {{ return Err(LuaError.CommandFailed) }}\n    if code != 0 {{ return Err(LuaError.Protocol) }}\n    return Ok(TableView.{{ session: session.value, table: table }})\n}}\n\n"));
    }
    out
}

fn render_c(abi: &str, source: &str, functions: &[String]) -> String {
    let wrappers = functions.iter().map(|name| format!("const char* {abi}_invoke_{name}(int64_t h,const char*input,int64_t deadline){{return invoke(h,\"{name}\",input,deadline);}}\nint64_t {abi}_view_{name}(int64_t h,int64_t deadline){{return view(h,\"{name}\",deadline);}}\n")).collect::<String>();
    format!(r#"#include <lua.h>
#include <lauxlib.h>
#include <lualib.h>
#include <stdint.h>
#include <stdatomic.h>
#include <pthread.h>
#include <string.h>
#include <time.h>
#define SLOTS 32
#define LIMIT (1024*1024)
typedef struct {{ lua_State *state; uint32_t generation; pthread_mutex_t use; atomic_int cancelled; int hook_reason; int64_t deadline; int reserved; }} Slot;
static Slot slots[SLOTS]; static pthread_mutex_t registry=PTHREAD_MUTEX_INITIALIZER; static pthread_once_t once=PTHREAD_ONCE_INIT;
static _Thread_local int64_t failed; static _Thread_local char output[LIMIT];
static const char codec[]="{}";
static const char program[]="{}";
static int64_t now_ms(void){{struct timespec t;if(clock_gettime(CLOCK_MONOTONIC,&t))return 0;return (int64_t)t.tv_sec*1000+t.tv_nsec/1000000;}}
static void initialize(void){{for(int n=0;n<SLOTS;n++){{pthread_mutex_init(&slots[n].use,0);slots[n].generation=1;}}}}
static void hook(lua_State*L,lua_Debug*d){{(void)d;Slot*s=*(Slot**)lua_getextraspace(L);if(atomic_load(&s->cancelled)){{s->hook_reason=3;luaL_error(L,"cancelled");}}if(s->deadline>0&&now_ms()>=s->deadline){{s->hook_reason=2;luaL_error(L,"deadline");}}}}
static int load(lua_State*L,const char*text,const char*name){{if(luaL_loadbuffer(L,text,strlen(text),name)!=LUA_OK)return 0;if(lua_pcall(L,0,0,0)!=LUA_OK)return 0;return 1;}}
static lua_State*fresh(Slot*s){{lua_State*L=luaL_newstate();if(!L)return 0;*(Slot**)lua_getextraspace(L)=s;luaL_openlibs(L);s->deadline=now_ms()+5000;s->hook_reason=0;lua_sethook(L,hook,LUA_MASKCOUNT,1000);if(!load(L,codec,"jet:codec")||!load(L,program,"jet:module")){{lua_sethook(L,0,0,0);lua_close(L);return 0;}}lua_sethook(L,0,0,0);s->deadline=0;return L;}}
static int decode(lua_State*L,const char*input){{lua_getglobal(L,"_jet_decode");lua_pushstring(L,input);return lua_pcall(L,1,1,0)==LUA_OK;}}
static int encode(lua_State*L){{lua_getglobal(L,"_jet_encode");lua_insert(L,-2);return lua_pcall(L,1,1,0)==LUA_OK;}}
static Slot*acquire(int64_t h,lua_State**out){{int index=(int)(h&63)-1;uint32_t generation=(uint32_t)((uint64_t)h>>6);if(index<0||index>=SLOTS){{failed=1;return 0;}}pthread_once(&once,initialize);pthread_mutex_lock(&registry);Slot*s=&slots[index];if(!s->state||s->generation!=generation){{pthread_mutex_unlock(&registry);failed=1;return 0;}}pthread_mutex_lock(&s->use);pthread_mutex_unlock(&registry);*out=s->state;return s;}}
static const char*invoke(int64_t h,const char*name,const char*input,int64_t deadline){{failed=0;output[0]=0;if(!input||strlen(input)>=LIMIT){{failed=5;return output;}}lua_State*L=0;Slot*s=acquire(h,&L);if(!s)return output;lua_settop(L,0);atomic_store(&s->cancelled,0);s->hook_reason=0;s->deadline=deadline>0?now_ms()+deadline:0;lua_sethook(L,hook,LUA_MASKCOUNT,1000);lua_getglobal(L,name);if(!lua_isfunction(L,-1))failed=4;else if(!decode(L,input))failed=6;else if(lua_pcall(L,1,1,0)!=LUA_OK)failed=s->hook_reason?s->hook_reason:4;else if(!encode(L))failed=6;if(failed)lua_settop(L,0);else{{size_t n=0;const char*v=lua_tolstring(L,-1,&n);if(!v||n>=LIMIT)failed=5;else{{memcpy(output,v,n);output[n]=0;}}lua_settop(L,0);}}lua_sethook(L,0,0,0);s->deadline=0;pthread_mutex_unlock(&s->use);return output;}}
static int64_t view(int64_t h,const char*name,int64_t deadline){{failed=0;lua_State*L=0;Slot*s=acquire(h,&L);if(!s)return 0;lua_settop(L,0);atomic_store(&s->cancelled,0);s->hook_reason=0;s->deadline=deadline>0?now_ms()+deadline:0;lua_sethook(L,hook,LUA_MASKCOUNT,1000);lua_getglobal(L,name);if(!lua_isfunction(L,-1))failed=4;else{{lua_pushnil(L);if(lua_pcall(L,1,1,0)!=LUA_OK)failed=s->hook_reason?s->hook_reason:4;else if(!lua_istable(L,-1))failed=6;}}int64_t reference=failed?0:luaL_ref(L,LUA_REGISTRYINDEX);lua_settop(L,0);lua_sethook(L,0,0,0);s->deadline=0;pthread_mutex_unlock(&s->use);return reference;}}
int64_t {abi}_view_get_int(int64_t h,int64_t reference,const char*key){{failed=0;if(!key){{failed=6;return 0;}}lua_State*L=0;Slot*s=acquire(h,&L);if(!s)return 0;lua_rawgeti(L,LUA_REGISTRYINDEX,(lua_Integer)reference);if(!lua_istable(L,-1))failed=6;else{{lua_pushstring(L,key);lua_rawget(L,-2);if(!lua_isinteger(L,-1))failed=6;}}int64_t value=failed?0:(int64_t)lua_tointeger(L,-1);lua_settop(L,0);pthread_mutex_unlock(&s->use);return value;}}
void {abi}_view_set_int(int64_t h,int64_t reference,const char*key,int64_t value){{failed=0;if(!key){{failed=6;return;}}lua_State*L=0;Slot*s=acquire(h,&L);if(!s)return;lua_rawgeti(L,LUA_REGISTRYINDEX,(lua_Integer)reference);if(!lua_istable(L,-1))failed=6;else{{lua_pushstring(L,key);lua_pushinteger(L,(lua_Integer)value);lua_rawset(L,-3);}}lua_settop(L,0);pthread_mutex_unlock(&s->use);}}
void {abi}_view_release(int64_t h,int64_t reference){{failed=0;lua_State*L=0;Slot*s=acquire(h,&L);if(!s)return;lua_rawgeti(L,LUA_REGISTRYINDEX,(lua_Integer)reference);if(!lua_istable(L,-1))failed=6;lua_pop(L,1);if(!failed)luaL_unref(L,LUA_REGISTRYINDEX,(int)reference);pthread_mutex_unlock(&s->use);}}
int64_t {abi}_take_error(void){{int64_t value=failed;failed=0;return value;}}
int64_t {abi}_open(void){{failed=0;pthread_once(&once,initialize);pthread_mutex_lock(&registry);int index=-1;for(int n=0;n<SLOTS;n++)if(!slots[n].state&&!slots[n].reserved){{index=n;slots[n].reserved=1;break;}}if(index<0){{pthread_mutex_unlock(&registry);failed=5;return 0;}}Slot*s=&slots[index];pthread_mutex_unlock(&registry);pthread_mutex_lock(&s->use);atomic_store(&s->cancelled,0);lua_State*L=fresh(s);if(!L){{pthread_mutex_lock(&registry);s->reserved=0;pthread_mutex_unlock(&registry);pthread_mutex_unlock(&s->use);failed=s->hook_reason?s->hook_reason:4;return 0;}}pthread_mutex_lock(&registry);s->state=L;s->reserved=0;uint32_t generation=s->generation;pthread_mutex_unlock(&registry);pthread_mutex_unlock(&s->use);return ((int64_t)generation<<6)|(index+1);}}
void {abi}_cancel(int64_t h){{int index=(int)(h&63)-1;uint32_t generation=(uint32_t)((uint64_t)h>>6);if(index<0||index>=SLOTS)return;pthread_mutex_lock(&registry);Slot*s=&slots[index];if(s->state&&s->generation==generation)atomic_store(&s->cancelled,1);pthread_mutex_unlock(&registry);}}
void {abi}_close(int64_t h){{failed=0;int index=(int)(h&63)-1;uint32_t generation=(uint32_t)((uint64_t)h>>6);if(index<0||index>=SLOTS){{failed=1;return;}}pthread_mutex_lock(&registry);Slot*s=&slots[index];if(!s->state||s->generation!=generation){{pthread_mutex_unlock(&registry);failed=1;return;}}pthread_mutex_lock(&s->use);lua_State*L=s->state;s->state=0;s->generation++;if(!s->generation)s->generation=1;pthread_mutex_unlock(&registry);lua_close(L);pthread_mutex_unlock(&s->use);}}
{}"#, c_escape(JSON_CODEC), c_escape(source), wrappers)
}

const JSON_CODEC: &str = r#"
jet = jet or {}; jet.null = jet.null or setmetatable({}, {__tostring=function() return "null" end})
local kinds=setmetatable({}, {__mode="k"})
local function decode(text)
  local at,n=1,#text
  local function ws() while at<=n and text:sub(at,at):match("%s") do at=at+1 end end
  local value
  local function string_value()
    at=at+1; local out={}
    while at<=n do local c=text:sub(at,at); at=at+1
      if c=='"' then return table.concat(out) end
      if c=='\\' then local e=text:sub(at,at); at=at+1; local simple={['"']='"',['\\']='\\',['/']='/',b='\b',f='\f',n='\n',r='\r',t='\t'}
        if simple[e] then out[#out+1]=simple[e] elseif e=='u' then local h=text:sub(at,at+3); if not h:match('^%x%x%x%x$') then error('json') end; at=at+4; local cp=tonumber(h,16)
          if cp>=0xD800 and cp<=0xDBFF and text:sub(at,at+1)=='\\u' then local l=tonumber(text:sub(at+2,at+5),16); if l and l>=0xDC00 and l<=0xDFFF then cp=0x10000+(cp-0xD800)*0x400+l-0xDC00;at=at+6 end end
          out[#out+1]=utf8.char(cp) else error('json') end
      elseif c:byte()<32 then error('json') else out[#out+1]=c end
    end; error('json')
  end
  local function array() at=at+1;ws();local out={};kinds[out]='array';if text:sub(at,at)==']' then at=at+1;return out end;while true do out[#out+1]=value();ws();local c=text:sub(at,at);at=at+1;if c==']' then return out elseif c~=',' then error('json') end;ws() end end
  local function object() at=at+1;ws();local out={};kinds[out]='object';if text:sub(at,at)=='}' then at=at+1;return out end;while true do if text:sub(at,at)~='"' then error('json') end;local k=string_value();ws();if text:sub(at,at)~=':' then error('json') end;at=at+1;ws();out[k]=value();ws();local c=text:sub(at,at);at=at+1;if c=='}' then return out elseif c~=',' then error('json') end;ws() end end
  function value() ws();local c=text:sub(at,at);if c=='"' then return string_value() elseif c=='[' then return array() elseif c=='{' then return object() elseif text:sub(at,at+3)=='true' then at=at+4;return true elseif text:sub(at,at+4)=='false' then at=at+5;return false elseif text:sub(at,at+3)=='null' then at=at+4;return jet.null else local s=text:sub(at):match('^-?%d+%.?%d*[eE]?[+-]?%d*');if not s or s=='' then error('json') end;local x=tonumber(s);if not x then error('json') end;at=at+#s;return x end end
  local out=value();ws();if at<=n then error('json') end;return out
end
local function quote(s) return '"'..s:gsub('[%z\1-\31\\"]',function(c)local m={['"']='\\"',['\\']='\\\\',['\b']='\\b',['\f']='\\f',['\n']='\\n',['\r']='\\r',['\t']='\\t'};return m[c] or string.format('\\u%04x',c:byte()) end)..'"' end
local function encode(v,seen,depth) if depth>64 then error('depth') end;local t=type(v);if v==jet.null then return 'null' elseif t=='nil' then return 'null' elseif t=='boolean' then return tostring(v) elseif t=='number' then if v~=v or v==math.huge or v==-math.huge then error('number') end;return string.format('%.17g',v) elseif t=='string' then return quote(v) elseif t~='table' then error('type') end;if seen[v] then error('cycle') end;seen[v]=true;local kind=kinds[v];if not kind then local count,max=0,0;local array=true;for k in pairs(v) do count=count+1;if type(k)~='number' or k%1~=0 or k<1 then array=false else max=math.max(max,k) end end;kind=array and max==count and 'array' or 'object' end;local out={};if kind=='array' then for i=1,#v do out[#out+1]=encode(v[i],seen,depth+1) end;seen[v]=nil;return '['..table.concat(out,',')..']' end;for k,x in pairs(v) do if type(k)~='string' then error('key') end;out[#out+1]=quote(k)..':'..encode(x,seen,depth+1) end;table.sort(out);seen[v]=nil;return '{'..table.concat(out,',')..'}' end
function _jet_decode(text) return decode(text) end
function _jet_encode(value) return encode(value,{},0) end
"#;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token { Word(String), Punct(char) }

fn discover(source: &str) -> Result<Vec<String>, BindError> {
    let tokens = lex(source)?;
    let mut functions = Vec::new();
    let mut depth = 0usize;
    let mut i = 0usize;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Word(word) if word == "function" && depth == 0 => {
                if matches!(tokens.get(i.wrapping_sub(1)),Some(Token::Word(previous))if previous=="local") { depth=1;i+=1;continue }
                let Some(Token::Word(name)) = tokens.get(i + 1) else { return Err(BindError::Source("only named top-level Lua functions can be bound".into())) };
                if !ident(name) || reserved(name) { return Err(BindError::Source(format!("`{name}` cannot be exported as a Jet function"))) }
                if tokens.get(i + 2) != Some(&Token::Punct('(')) { return Err(BindError::Source(format!("Lua function `{name}` has an unsupported declaration"))) }
                let Some(Token::Word(_parameter)) = tokens.get(i + 3) else { return Err(BindError::Source(format!("Lua function `{name}` must take one input argument"))) };
                if tokens.get(i + 4) != Some(&Token::Punct(')')) { return Err(BindError::Source(format!("Lua function `{name}` must take one input argument"))) }
                if functions.contains(name) { return Err(BindError::Source(format!("Lua function `{name}` is declared more than once"))) }
                functions.push(name.clone()); depth = 1; i += 5; continue;
            }
            Token::Word(word) if matches!(word.as_str(), "function" | "do" | "then" | "repeat") => depth += 1,
            Token::Word(word) if matches!(word.as_str(), "end" | "until") => depth = depth.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }
    for name in &functions {
        for suffix in ["typed", "view"] {
            if functions.contains(&format!("{name}_{suffix}")) {
                return Err(BindError::Source(format!("Lua function `{name}` collides with generated adapter `{name}_{suffix}`")));
            }
        }
    }
    Ok(functions)
}

fn lex(source: &str) -> Result<Vec<Token>, BindError> {
    let bytes=source.as_bytes();let mut out=Vec::new();let mut i=0;
    while i<bytes.len(){let b=bytes[i];if b.is_ascii_whitespace(){i+=1;continue}if b==b'-'&&bytes.get(i+1)==Some(&b'-'){i+=2;if bytes.get(i)==Some(&b'[')&&bytes.get(i+1)==Some(&b'['){i+=2;while i+1<bytes.len()&&(bytes[i]!=b']'||bytes[i+1]!=b']'){i+=1}if i+1>=bytes.len(){return Err(BindError::Source("unterminated Lua block comment".into()))}i+=2}else{while i<bytes.len()&&bytes[i]!=b'\n'{i+=1}}continue}if b==b'\''||b==b'"'{let q=b;i+=1;while i<bytes.len(){if bytes[i]==b'\\'{i+=2;continue}if bytes[i]==q{break}i+=1}if i>=bytes.len(){return Err(BindError::Source("unterminated Lua string".into()))}i+=1;continue}if b==b'['&&bytes.get(i+1)==Some(&b'['){i+=2;while i+1<bytes.len()&&(bytes[i]!=b']'||bytes[i+1]!=b']'){i+=1}if i+1>=bytes.len(){return Err(BindError::Source("unterminated Lua long string".into()))}i+=2;continue}if b.is_ascii_alphabetic()||b==b'_'{let start=i;i+=1;while i<bytes.len()&&(bytes[i].is_ascii_alphanumeric()||bytes[i]==b'_'){i+=1}out.push(Token::Word(source[start..i].into()));continue}if b"(),".contains(&b){out.push(Token::Punct(b as char));}i+=1}
    Ok(out)
}

fn tool_path(tool: &str) -> Option<PathBuf> { std::env::split_paths(&std::env::var_os("PATH")?).map(|p|p.join(tool)).find(|p|p.is_file()) }
fn tool_root(tool: &Path) -> Option<PathBuf> { let exe=std::fs::canonicalize(tool).ok()?;exe.parent()?.parent().map(Path::to_path_buf) }
fn find_header_dir(root:&Path)->Option<PathBuf>{[root.join("include"),root.join("include/lua5.4")].into_iter().find(|p|p.join("lua.h").is_file())}
fn find_library_dir(root:&Path)->Option<PathBuf>{[root.join("lib"),root.join("lib64")].into_iter().find(|p|p.join(lib_name()).is_file())}
fn ident(v:&str)->bool{let mut c=v.chars();matches!(c.next(),Some(x)if x.is_ascii_alphabetic()||x=='_')&&c.all(|x|x.is_ascii_alphanumeric()||x=='_')}
fn reserved(v:&str)->bool{GENERATED_FIXED_FUNCTIONS.iter().any(|(name,_)|*name==v)||matches!(v,"Session"|"TableView"|"LuaError")||crate::Syntax::JET_KEYWORD_LIST.contains(&v)||crate::Syntax::JET_TYPE_LIST.contains(&v)}
fn c_escape(v:&str)->String{let mut o=String::new();for b in v.bytes(){match b{b'\\'=>o.push_str("\\\\"),b'"'=>o.push_str("\\\""),b'\n'=>o.push_str("\\n"),b'\r'=>o.push_str("\\r"),b'\t'=>o.push_str("\\t"),0x20..=0x7e=>o.push(b as char),_=>o.push_str(&format!("\\{:03o}",b))}}o}
fn run(command:&mut Command,tool:&'static str)->Result<(),BindError>{const CAP:usize=64*1024;command.stdout(Stdio::null()).stderr(Stdio::piped());let mut child=command.spawn().map_err(|e|if e.kind()==std::io::ErrorKind::NotFound{BindError::ToolMissing(tool)}else{BindError::IO(format!("could not start `{tool}`: {e}"))})?;let stderr=child.stderr.take().ok_or_else(||BindError::IO(format!("could not supervise `{tool}` stderr")))?;let err=std::thread::spawn(move||drain(stderr,CAP));let deadline=Instant::now()+Duration::from_secs(60);let status=loop{match child.try_wait().map_err(|e|BindError::IO(format!("could not supervise `{tool}`: {e}")))?{Some(v)=>break v,None if Instant::now()>=deadline=>{let _=child.kill();let _=child.wait();let _=err.join();return Err(BindError::ToolFailed(tool,"the tool exceeded the 60 second limit".into()))},None=>std::thread::sleep(Duration::from_millis(10))}};let stderr=err.join().map_err(|_|BindError::IO(format!("`{tool}` stderr reader failed")))??;if status.success(){Ok(())}else{Err(BindError::ToolFailed(tool,launder(&stderr)))}}
fn drain(mut input:impl Read,limit:usize)->Result<Vec<u8>,BindError>{let mut out=Vec::new();let mut buf=[0u8;8192];loop{let n=input.read(&mut buf).map_err(|e|BindError::IO(format!("could not read foreign tool output: {e}")))?;if n==0{break}let keep=(limit-out.len()).min(n);out.extend_from_slice(&buf[..keep]);}Ok(out)}
fn launder(v:&[u8])->String{String::from_utf8_lossy(v).lines().map(str::trim).find(|v|!v.is_empty()).map(|v|v.rsplit_once(": ").map_or(v,|x|x.1).chars().take(160).collect()).unwrap_or_else(||"the foreign tool returned a failure status".into())}
#[cfg(target_os="linux")]fn lib_name()->&'static str{"liblua.so"}
#[cfg(target_os="macos")]fn lib_name()->&'static str{"liblua.dylib"}
#[cfg(target_os="windows")]fn lib_name()->&'static str{"lua54.dll"}

#[cfg(test)]
mod tests {
    #[test] fn discovers_only_top_level_named_functions(){let source="-- function fake(x)\nfunction transform(x) return x end\nlocal function hidden(x) return x end\n";assert_eq!(super::discover(source).unwrap(),vec!["transform"]);}
    #[test] fn rejects_wrong_arity(){assert!(super::discover("function bad(a,b) return a end").is_err());}
    #[test] fn fixed_function_descriptors_drive_emission_and_reservation(){let abi="jet_lua_test";let jet=super::render_jet(abi,&["probe".into()]);for (name,signature) in super::GENERATED_FIXED_FUNCTIONS{assert!(jet.contains(&format!("fn {name}{signature} = \"{abi}_{name}\"")),"fixed helper `{name}` descriptor not emitted");assert!(super::reserved(name),"fixed helper `{name}` descriptor not reserved");let source=format!("function {name}(input) return input end");let error=super::discover(&source).unwrap_err();assert!(error.to_string().contains(&format!("`{name}` cannot be exported")));}}
    #[test] fn codec_has_null_and_cycle_guards(){assert!(super::JSON_CODEC.contains("jet.null"));assert!(super::JSON_CODEC.contains("cycle"));}
    #[test] fn table_view_reads_live_lua_without_json(){let c=super::render_c("jet_lua_test","function values(input) return { count = 1 } end",&["values".into()]);let body=c.split("_view_get_int").nth(1).unwrap().split("_view_set_int").next().unwrap();assert!(body.contains("lua_rawget"));assert!(!body.contains("decode(")&&!body.contains("encode(")&&!body.contains("DataTree"));}
}
