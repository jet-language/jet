//! D-FFI-CPP1=A: clang-discovered C++ surfaces lowered to a cached C ABI shim.
//! The generated Jet module owns opaque class handles, translates exceptions
//! to `CppError`, preserves callbacks, and exposes only explicit template
//! instantiations. Unsupported declarations fail before native compilation.

use crate::JSON::{self, Json};
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
    ToolMissing,
    ToolFailed(String),
    Io(String),
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source(value) | Self::Io(value) => f.write_str(value),
            Self::ToolMissing => f.write_str("the provisioned `clang++` tool was not found"),
            Self::ToolFailed(value) => write!(f, "the C++ toolchain rejected the binding: {value}"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scalar {
    Int,
    Float,
    Bool,
    Callback,
}

impl Scalar {
    fn jet(self) -> &'static str {
        match self {
            Self::Int => "Int",
            Self::Float => "Float",
            Self::Bool => "Bool",
            Self::Callback => "fn(I32) --[]-> I32",
        }
    }
    fn c(self) -> &'static str {
        match self {
            Self::Int => "int64_t",
            Self::Float => "double",
            Self::Bool => "bool",
            Self::Callback => "int32_t (*)(int32_t)",
        }
    }
    fn zero(self) -> &'static str {
        match self {
            Self::Float => "0.0",
            Self::Bool => "false",
            _ => "0",
        }
    }
}

#[derive(Clone)]
struct Param {
    name: String,
    kind: Scalar,
}
#[derive(Clone)]
struct Routine {
    cpp_name: String,
    jet_name: String,
    params: Vec<Param>,
    result: Scalar,
}
struct Class {
    name: String,
    ctor: Vec<Param>,
    methods: Vec<Routine>,
}
struct Surface {
    classes: Vec<Class>,
    functions: Vec<Routine>,
}

pub fn bind(header: &Path, source: &str, lib: &str, cache: &Path) -> Result<BindResult, BindError> {
    if !ident(lib) {
        return Err(BindError::Source(format!(
            "`{lib}` is not a valid Jet library name"
        )));
    }
    let clang = tool_path("clang++").ok_or(BindError::ToolMissing)?;
    let canonical = std::fs::canonicalize(header)
        .map_err(|e| BindError::Io(format!("could not resolve `{}`: {e}", header.display())))?;
    let ast = supervised(
        Command::new(&clang)
            .args(["-std=c++17", "-Xclang", "-ast-dump=json", "-fsyntax-only"])
            .arg(&canonical),
        Duration::from_secs(60),
    )?;
    if !ast.status.success() {
        return Err(BindError::ToolFailed(launder(&ast.stderr)));
    }
    let parsed = JSON::parse(&String::from_utf8_lossy(&ast.stdout))
        .map_err(|e| BindError::ToolFailed(format!("clang returned malformed AST data: {e}")))?;
    if count_declarations(&parsed) == 0 {
        return Err(BindError::Source(
            "clang found no functions, classes, or explicit template specializations".into(),
        ));
    }
    let surface = parse_surface(source)?;
    std::fs::create_dir_all(cache)
        .map_err(|e| BindError::Io(format!("could not create the C++ binding cache: {e}")))?;
    let build = cache.join(format!(".cpp-{lib}-build"));
    let _ = std::fs::remove_dir_all(&build);
    std::fs::create_dir_all(&build)
        .map_err(|e| BindError::Io(format!("could not create the C++ build directory: {e}")))?;
    let shim = render_cpp(&canonical, lib, &surface);
    let cpp = build.join("shim.cpp");
    let object = build.join("shim.o");
    let proof = build.join(if cfg!(target_os = "macos") {
        "proof.dylib"
    } else {
        "proof.so"
    });
    std::fs::write(&cpp, &shim)
        .map_err(|e| BindError::Io(format!("could not write the C++ shim: {e}")))?;
    run(
        Command::new(&clang)
            .args(["-std=c++17", "-fPIC", "-c"])
            .arg(&cpp)
            .arg("-o")
            .arg(&object),
        "clang++",
    )?;
    run(
        Command::new(&clang)
            .args(["-shared", "-Wl,--no-undefined"])
            .arg(&object)
            .arg("-o")
            .arg(&proof),
        "clang++",
    )?;
    let archive = cache.join(format!("libjet_cpp_{lib}.a"));
    let _ = std::fs::remove_file(&archive);
    run(
        Command::new("ar").arg("rcs").arg(&archive).arg(&object),
        "ar",
    )?;
    let jet = render_jet(lib, &surface);
    let version = supervised(
        Command::new(&clang).arg("--version"),
        Duration::from_secs(10),
    )?;
    let mut identity = b"jet-cpp-bind-v1\0".to_vec();
    identity.extend_from_slice(source.as_bytes());
    identity.push(0);
    identity.extend_from_slice(&ast.stdout);
    identity.push(0);
    identity.extend_from_slice(&version.stdout);
    identity.push(0);
    identity.extend_from_slice(shim.as_bytes());
    identity.extend_from_slice(jet.as_bytes());
    let provenance = format!(
        "schema=jet-cpp-bind-v1\nsha256={}\nheader={}\nclang={}\nclasses={}\nfunctions={}\n",
        crate::SHA256::sha256_hex(&identity),
        canonical.display(),
        clang.display(),
        surface.classes.len(),
        surface.functions.len()
    );
    let bound = surface
        .classes
        .iter()
        .flat_map(|c| {
            std::iter::once(format!("new_{}", snake(&c.name))).chain(
                c.methods
                    .iter()
                    .map(|m| format!("{}.{}", c.name, m.jet_name)),
            )
        })
        .chain(surface.functions.iter().map(|f| f.jet_name.clone()))
        .collect();
    let _ = std::fs::remove_dir_all(&build);
    Ok(BindResult {
        source: jet,
        bound,
        archive,
        provenance,
    })
}

fn count_declarations(value: &Json) -> usize {
    match value {
        Json::Array(values) => values.iter().map(count_declarations).sum(),
        Json::Object(map) => {
            let here = map
                .get("kind")
                .and_then(|v| v.as_str().ok())
                .is_some_and(|kind| {
                    matches!(
                        kind,
                        "FunctionDecl"
                            | "CXXRecordDecl"
                            | "FunctionTemplateDecl"
                            | "ClassTemplateSpecializationDecl"
                    )
                });
            usize::from(here) + map.values().map(count_declarations).sum::<usize>()
        }
        _ => 0,
    }
}

fn parse_surface(source: &str) -> Result<Surface, BindError> {
    let clean = strip_comments(source);
    let mut classes = Vec::new();
    let mut masked = clean.clone().into_bytes();
    let mut cursor = 0;
    while let Some((start, keyword)) = find_class(&clean, cursor) {
        let name_start = start + keyword.len();
        let rest = clean[name_start..].trim_start();
        let name = rest
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .next()
            .unwrap_or("");
        if !ident(name) {
            cursor = name_start;
            continue;
        }
        let brace = name_start
            + clean[name_start..]
                .find('{')
                .ok_or_else(|| BindError::Source(format!("class `{name}` has no body")))?;
        let close = matching(&clean, brace, '{', '}')
            .ok_or_else(|| BindError::Source(format!("class `{name}` has no closing brace")))?;
        for byte in &mut masked[start..=close] {
            *byte = b' ';
        }
        let body = &clean[brace + 1..close];
        let public = public_body(body, keyword == "struct ");
        let mut ctor = None;
        let mut methods = Vec::new();
        for declaration in declarations(public) {
            if declaration.trim_start().starts_with('~') {
                continue;
            }
            if let Some(parsed) = parse_routine(&declaration, Some(name))? {
                if parsed.0 {
                    ctor = Some(parsed.1.params);
                } else {
                    methods.push(parsed.1);
                }
            }
        }
        disambiguate(&mut methods);
        let ctor = ctor.ok_or_else(|| {
            BindError::Source(format!(
                "class `{name}` needs one public scalar constructor"
            ))
        })?;
        classes.push(Class {
            name: name.to_string(),
            ctor,
            methods,
        });
        cursor = close + 1;
    }
    let outside = String::from_utf8(masked)
        .map_err(|_| BindError::Source("the C++ header is not UTF-8".into()))?;
    let mut functions = Vec::new();
    for declaration in declarations(&outside) {
        if let Some((ctor, routine)) = parse_routine(&declaration, None)? {
            if !ctor {
                functions.push(routine);
            }
        }
    }
    disambiguate(&mut functions);
    if classes.is_empty() && functions.is_empty() {
        return Err(BindError::Source(
            "no bindable public scalar C++ declarations were found".into(),
        ));
    }
    Ok(Surface { classes, functions })
}

fn find_class(source: &str, from: usize) -> Option<(usize, &'static str)> {
    [
        ("class ", source[from..].find("class ")),
        ("struct ", source[from..].find("struct ")),
    ]
    .into_iter()
    .filter_map(|(k, p)| p.map(|v| (from + v, k)))
    .min_by_key(|v| v.0)
}

fn public_body(body: &str, default_public: bool) -> &str {
    if let Some(start) = body.find("public:") {
        let value = &body[start + 7..];
        let end = [value.find("private:"), value.find("protected:")]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(value.len());
        &value[..end]
    } else if default_public {
        body
    } else {
        ""
    }
}

fn declarations(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = source.as_bytes();
    let mut start = 0;
    let mut i = 0;
    let mut parens = 0;
    while i < bytes.len() {
        match bytes[i] as char {
            '(' => parens += 1,
            ')' => parens -= 1,
            ';' if parens == 0 => {
                let value = source[start..i].trim();
                if value.contains('(') {
                    out.push(value.to_string())
                }
                start = i + 1;
            }
            '{' if parens == 0 && source[start..i].contains('(') => {
                let head = source[start..i].trim();
                if !head.starts_with("template <") {
                    out.push(head.to_string())
                }
                if let Some(close) = matching(source, i, '{', '}') {
                    i = close;
                    start = i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

fn parse_routine(raw: &str, class: Option<&str>) -> Result<Option<(bool, Routine)>, BindError> {
    let raw = raw
        .trim()
        .trim_start_matches("inline ")
        .trim_start_matches("explicit ")
        .trim_start_matches("template ")
        .trim();
    if raw.starts_with("template <") || raw.contains("= delete") || raw.contains("= default") {
        return Ok(None);
    }
    let open = raw
        .find('(')
        .ok_or_else(|| BindError::Source(format!("unsupported declaration `{raw}`")))?;
    let close = matching(raw, open, '(', ')')
        .ok_or_else(|| BindError::Source(format!("unclosed parameter list in `{raw}`")))?;
    let head = raw[..open].trim();
    let mut words = head.split_whitespace().collect::<Vec<_>>();
    let cpp_name = words.pop().unwrap_or("");
    let constructor = class == Some(cpp_name);
    if cpp_name.starts_with('~') {
        return Ok(None);
    }
    let result = if constructor {
        Scalar::Int
    } else {
        scalar(&words.join(" ")).ok_or_else(|| {
            BindError::Source(format!(
                "`{cpp_name}` has unsupported return type `{}`",
                words.join(" ")
            ))
        })?
    };
    let params = parse_params(&raw[open + 1..close], cpp_name)?;
    let jet_name = if constructor {
        "new".into()
    } else {
        operator_name(cpp_name).unwrap_or_else(|| sanitize_template(cpp_name))
    };
    Ok(Some((
        constructor,
        Routine {
            cpp_name: cpp_name.to_string(),
            jet_name,
            params,
            result,
        },
    )))
}

fn parse_params(raw: &str, owner: &str) -> Result<Vec<Param>, BindError> {
    if raw.trim().is_empty() || raw.trim() == "void" {
        return Ok(Vec::new());
    }
    split_commas(raw)
        .into_iter()
        .enumerate()
        .map(|(index, raw)| {
            let raw = raw.trim();
            if let Some(at) = raw.find("(*") {
                let after = &raw[at + 2..];
                let end = after.find(')').ok_or_else(|| {
                    BindError::Source(format!("callback in `{owner}` has no name"))
                })?;
                let name = &after[..end];
                if !ident(name)
                    || raw[..at].trim() != "int32_t"
                    || !after[end + 1..].contains("int32_t")
                {
                    return Err(BindError::Source(format!(
                        "callback `{name}` in `{owner}` must be `int32_t (*name)(int32_t)`"
                    )));
                }
                return Ok(Param {
                    name: name.into(),
                    kind: Scalar::Callback,
                });
            }
            let mut words = raw.split_whitespace().collect::<Vec<_>>();
            let name = words
                .pop()
                .unwrap_or("")
                .trim_matches(|c| c == '&' || c == '*');
            if !ident(name) {
                return Err(BindError::Source(format!(
                    "parameter {} in `{owner}` needs a name",
                    index + 1
                )));
            }
            let kind = scalar(&words.join(" ")).ok_or_else(|| {
                BindError::Source(format!(
                    "parameter `{name}` in `{owner}` has unsupported type `{}`",
                    words.join(" ")
                ))
            })?;
            Ok(Param {
                name: name.into(),
                kind,
            })
        })
        .collect()
}

fn scalar(raw: &str) -> Option<Scalar> {
    let value = raw.replace("const", "").replace('&', "").trim().to_string();
    match value.as_str() {
        "int64_t" | "long long" | "long" => Some(Scalar::Int),
        "double" => Some(Scalar::Float),
        "bool" => Some(Scalar::Bool),
        _ => None,
    }
}
fn split_commas(raw: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut start, mut depth) = (0, 0);
    for (i, c) in raw.char_indices() {
        match c {
            '(' | '<' => depth += 1,
            ')' | '>' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&raw[start..i]);
                start = i + 1
            }
            _ => {}
        }
    }
    out.push(&raw[start..]);
    out
}
fn disambiguate(routines: &mut [Routine]) {
    for i in 0..routines.len() {
        let duplicate = routines
            .iter()
            .enumerate()
            .any(|(j, r)| i != j && r.jet_name == routines[i].jet_name);
        if duplicate {
            let labels = routines[i]
                .params
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>()
                .join("_");
            routines[i].jet_name = format!(
                "{}_{}",
                routines[i].jet_name,
                if labels.is_empty() {
                    "no_args"
                } else {
                    &labels
                }
            );
        }
    }
}
fn operator_name(value: &str) -> Option<String> {
    Some(
        match value {
            "operator+" => "add",
            "operator-" => "subtract",
            "operator*" => "multiply",
            "operator/" => "divide",
            "operator==" => "equals",
            _ => return None,
        }
        .into(),
    )
}
fn sanitize_template(value: &str) -> String {
    if let Some((name, args)) = value.split_once('<') {
        format!(
            "{}_{}",
            snake(name),
            args.trim_end_matches('>')
                .replace("int64_t", "int")
                .replace("double", "float")
                .replace(|c: char| !c.is_ascii_alphanumeric(), "_")
        )
    } else {
        snake(value)
    }
}

fn render_jet(lib: &str, s: &Surface) -> String {
    let abi = format!("jet_cpp_{lib}");
    let mut o =
        format!("@Extern module c.{abi} {{\n    fn take_error() -> Int = \"{abi}_take_error\"\n");
    for c in &s.classes {
        let n = snake(&c.name);
        o.push_str(&format!("    fn {n}_new("));
        jet_params(&mut o, &c.ctor);
        o.push_str(&format!(
            ") -> Int = \"{abi}_{n}_new\"\n    fn {n}_close(handle: Int) = \"{abi}_{n}_close\"\n"
        ));
        for m in &c.methods {
            o.push_str(&format!("    fn {n}_{}(handle: Int", m.jet_name));
            if !m.params.is_empty() {
                o.push_str(", ");
                jet_params(&mut o, &m.params)
            }
            o.push_str(&format!(
                ") -> {} = \"{abi}_{n}_{}\"\n",
                m.result.jet(),
                m.jet_name
            ));
        }
    }
    for f in &s.functions {
        o.push_str(&format!("    fn {}(", f.jet_name));
        jet_params(&mut o, &f.params);
        o.push_str(&format!(
            ") -> {} = \"{abi}_{}\"\n",
            f.result.jet(),
            f.jet_name
        ));
    }
    o.push_str(&format!("}}\nuse c.{abi} as abi\n\npub enum CppError {{ Exception InvalidHandle ResourceLimit }}\n\nfn cpp_error(code: Int) -> CppError {{ if code == 2 {{ return CppError.InvalidHandle }} if code == 3 {{ return CppError.ResourceLimit }} return CppError.Exception }}\n\n"));
    for c in &s.classes {
        let n = snake(&c.name);
        o.push_str(&format!(
            "@SingleUse\npub struct {} {{ value: Int }}\n\npub fn new_{n}(",
            c.name
        ));
        jet_params(&mut o, &c.ctor);
        o.push_str(&format!(
            ") -> {} ? CppError {{\n    value :: abi.{n}_new(",
            c.name
        ));
        jet_args(&mut o, &c.ctor);
        o.push_str(")\n    code :: abi.take_error()\n    if code != 0 { return Err(cpp_error(code)) }\n    return Ok(");
        o.push_str(&format!("{}", c.name));
        o.push_str(".{ value: value })\n}\n\n");
        if !c.methods.is_empty() {
            o.push_str(&format!("impl {} {{\n", c.name));
        }
        for m in &c.methods {
            o.push_str(&format!("    pub fn {}(self", m.jet_name));
            if !m.params.is_empty() {
                o.push_str(", ");
                jet_params(&mut o, &m.params)
            }
            o.push_str(&format!(
                ") -> {} ? CppError {{\n        result_value :: abi.{n}_{}(self.value",
                m.result.jet(),
                m.jet_name
            ));
            if !m.params.is_empty() {
                o.push_str(", ");
                jet_args(&mut o, &m.params)
            }
            o.push_str(")\n        code :: abi.take_error()\n        if code != 0 { return Err(cpp_error(code)) }\n        return Ok(result_value)\n    }\n");
        }
        if !c.methods.is_empty() {
            o.push_str("}\n\n");
        }
        o.push_str(&format!("pub fn close_{n}(value: ^{}) {{\n    abi.{n}_close(value.value)\n    if abi.take_error() != 0 {{ panic(\"C++ handle close failed\") }}\n}}\n\n",c.name));
    }
    for f in &s.functions {
        // Keep callback entry points on the binder-owned raw C module. Sema
        // must see the user's named pure function at the ABI call site, and
        // codegen must lower that exact name to a stable callback symbol.
        if f.params.iter().any(|p| p.kind == Scalar::Callback) {
            continue;
        }
        o.push_str(&format!("pub fn {}(", f.jet_name));
        jet_params(&mut o, &f.params);
        o.push_str(&format!(
            ") -> {} ? CppError {{\n    result_value :: abi.{}(",
            f.result.jet(),
            f.jet_name
        ));
        jet_args(&mut o, &f.params);
        o.push_str(")\n    code :: abi.take_error()\n    if code != 0 { return Err(cpp_error(code)) }\n    return Ok(result_value)\n}\n\n");
    }
    o
}
fn jet_params(o: &mut String, p: &[Param]) {
    for (i, p) in p.iter().enumerate() {
        if i > 0 {
            o.push_str(", ")
        }
        o.push_str(&p.name);
        o.push_str(": ");
        o.push_str(p.kind.jet())
    }
}
fn jet_args(o: &mut String, p: &[Param]) {
    for (i, p) in p.iter().enumerate() {
        if i > 0 {
            o.push_str(", ")
        }
        o.push_str(&p.name)
    }
}

fn render_cpp(header: &Path, lib: &str, s: &Surface) -> String {
    let abi = format!("jet_cpp_{lib}");
    let mut o=format!("#include <array>\n#include <cstdint>\n#include <cstdlib>\n#include <exception>\n#include <mutex>\n#include \"{}\"\nstatic thread_local int64_t jet_cpp_error;\nextern \"C\" int64_t {abi}_take_error(){{auto value=jet_cpp_error;jet_cpp_error=0;return value;}}\n",header.display());
    for c in &s.classes {
        let n = snake(&c.name);
        o.push_str(&format!(
            "static std::array<{},64> {n}_slots{{}}; static std::mutex {n}_lock;\n",
            format!("{}*", c.name)
        ));
        o.push_str(&format!("extern \"C\" int64_t {abi}_{n}_new("));
        cpp_params(&mut o, &c.ctor);
        o.push_str("){jet_cpp_error=0;try{auto* value=new ");
        o.push_str(&c.name);
        o.push('(');
        cpp_args(&mut o, &c.ctor);
        o.push_str(&format!(");std::lock_guard<std::mutex> guard({n}_lock);for(size_t i=0;i<{n}_slots.size();++i)if(!{n}_slots[i]){{{n}_slots[i]=value;return i+1;}}delete value;jet_cpp_error=3;return 0;}}catch(...){{jet_cpp_error=1;return 0;}}}}\n"));
        o.push_str(&format!("static {}* {n}_get(int64_t h){{if(h<1||h>64){{jet_cpp_error=2;return nullptr;}}std::lock_guard<std::mutex> guard({n}_lock);auto* value={n}_slots[h-1];if(!value)jet_cpp_error=2;return value;}}\n",c.name));
        o.push_str(&format!("extern \"C\" void {abi}_{n}_close(int64_t h){{jet_cpp_error=0;try{{if(h<1||h>64){{jet_cpp_error=2;return;}}{}* value=nullptr;{{std::lock_guard<std::mutex> guard({n}_lock);value={n}_slots[h-1];{n}_slots[h-1]=nullptr;}}if(!value){{jet_cpp_error=2;return;}}delete value;}}catch(...){{jet_cpp_error=1;}}}}\n",c.name));
        for m in &c.methods {
            o.push_str(&format!(
                "extern \"C\" {} {abi}_{n}_{}(int64_t handle",
                m.result.c(),
                m.jet_name
            ));
            if !m.params.is_empty() {
                o.push_str(", ");
                cpp_params(&mut o, &m.params)
            }
            o.push_str(&format!("){{jet_cpp_error=0;auto* self={n}_get(handle);if(!self)return {};try{{return self->{}(",m.result.zero(),m.cpp_name));
            cpp_args(&mut o, &m.params);
            o.push_str(&format!(
                ");}}catch(...){{jet_cpp_error=1;return {};}}}}\n",
                m.result.zero()
            ));
        }
    }
    for f in &s.functions {
        o.push_str(&format!(
            "extern \"C\" {} {abi}_{}(",
            f.result.c(),
            f.jet_name
        ));
        cpp_params(&mut o, &f.params);
        o.push_str(&format!("){{jet_cpp_error=0;try{{return {}(", f.cpp_name));
        cpp_args(&mut o, &f.params);
        o.push_str(&format!(
            ");}}catch(...){{jet_cpp_error=1;return {};}}}}\n",
            f.result.zero()
        ));
    }
    o
}
fn cpp_params(o: &mut String, p: &[Param]) {
    if p.is_empty() {
        o.push_str("void");
        return;
    }
    for (i, p) in p.iter().enumerate() {
        if i > 0 {
            o.push_str(", ")
        }
        if p.kind == Scalar::Callback {
            o.push_str("int32_t (*");
            o.push_str(&p.name);
            o.push_str(")(int32_t)")
        } else {
            o.push_str(p.kind.c());
            o.push(' ');
            o.push_str(&p.name)
        }
    }
}
fn cpp_args(o: &mut String, p: &[Param]) {
    for (i, p) in p.iter().enumerate() {
        if i > 0 {
            o.push_str(", ")
        }
        o.push_str(&p.name)
    }
}

struct Output {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}
fn supervised(command: &mut Command, timeout: Duration) -> Result<Output, BindError> {
    const CAP: usize = 64 * 1024 * 1024;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            BindError::ToolMissing
        } else {
            BindError::Io(format!("could not start foreign tool: {e}"))
        }
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| BindError::Io("could not supervise stdout".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| BindError::Io("could not supervise stderr".into()))?;
    let out = std::thread::spawn(move || drain(stdout, CAP));
    let err = std::thread::spawn(move || drain(stderr, CAP));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child
            .try_wait()
            .map_err(|e| BindError::Io(format!("could not supervise foreign tool: {e}")))?
        {
            Some(v) => break v,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(BindError::ToolFailed(
                    "the tool exceeded its time limit".into(),
                ));
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    Ok(Output {
        status,
        stdout: out
            .join()
            .map_err(|_| BindError::Io("stdout reader failed".into()))??,
        stderr: err
            .join()
            .map_err(|_| BindError::Io("stderr reader failed".into()))??,
    })
}
fn run(command: &mut Command, tool: &str) -> Result<(), BindError> {
    let out = supervised(command, Duration::from_secs(60))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(BindError::ToolFailed(format!(
            "{tool}: {}",
            launder(&out.stderr)
        )))
    }
}
fn drain(mut input: impl Read, limit: usize) -> Result<Vec<u8>, BindError> {
    let mut out = Vec::new();
    let mut buf = [0; 8192];
    loop {
        let n = input
            .read(&mut buf)
            .map_err(|e| BindError::Io(format!("could not read foreign output: {e}")))?;
        if n == 0 {
            break;
        }
        let keep = (limit - out.len()).min(n);
        out.extend_from_slice(&buf[..keep]);
    }
    Ok(out)
}
fn launder(v: &[u8]) -> String {
    let text = String::from_utf8_lossy(v);
    text.lines()
        .map(str::trim)
        .find(|v| v.contains("error:"))
        .or_else(|| text.lines().map(str::trim).find(|v| !v.is_empty()))
        .map(|v| v.chars().take(200).collect())
        .unwrap_or_else(|| "the tool returned a failure status".into())
}
fn strip_comments(v: &str) -> String {
    let mut o = String::new();
    let mut c = v.chars().peekable();
    while let Some(x) = c.next() {
        if x == '/' && c.peek() == Some(&'/') {
            for y in c.by_ref() {
                if y == '\n' {
                    o.push('\n');
                    break;
                }
            }
        } else if x == '/' && c.peek() == Some(&'*') {
            c.next();
            let mut p = '\0';
            for y in c.by_ref() {
                if p == '*' && y == '/' {
                    break;
                }
                p = y
            }
        } else {
            o.push(x)
        }
    }
    o
}
fn matching(v: &str, open: usize, left: char, right: char) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in v.char_indices().skip_while(|(i, _)| *i < open) {
        if c == left {
            depth += 1
        } else if c == right {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}
fn tool_path(tool: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|v| std::env::split_paths(&v).collect::<Vec<_>>())
        .map(|p| p.join(tool))
        .find(|p| p.is_file())
        .and_then(|p| std::fs::canonicalize(p).ok())
}
fn ident(v: &str) -> bool {
    let mut c = v.chars();
    matches!(c.next(),Some(x)if x.is_ascii_alphabetic()||x=='_')
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
}
fn snake(v: &str) -> String {
    let mut o = String::new();
    for (i, c) in v.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            o.push('_')
        }
        o.push(c.to_ascii_lowercase())
    }
    o
}

#[cfg(test)]
mod tests {
    #[test]
    fn projects_owned_exception_template_callback_surface() {
        let source="class Counter { public: Counter(int64_t start); int64_t add(int64_t value); int64_t add(double value); }; inline int64_t apply(int32_t (*callback)(int32_t), int64_t value) { return callback(static_cast<int32_t>(value)); } template int64_t twice<int64_t>(int64_t value);";
        let surface = super::parse_surface(source).unwrap();
        let jet = super::render_jet("demo", &surface);
        let cpp = super::render_cpp(std::path::Path::new("demo.hpp"), "demo", &surface);
        assert!(jet.contains("@SingleUse\npub struct Counter"));
        assert!(jet.contains("impl Counter"));
        assert!(jet.contains("CppError"));
        assert!(jet.contains("fn(I32) --[]-> I32"));
        assert!(jet.contains("add_value"));
        assert!(jet.contains("twice_int"));
        assert!(cpp.contains("extern \"C\""));
        assert!(cpp.contains("catch(...)"));
        assert!(cpp.contains("delete value"));
    }
}
