//! FreePascal cdecl binder with bounded owned handles (D-FFI-PASCAL1=A).

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindResult {
    pub source: String,
    pub bound: Vec<String>,
    pub archive: PathBuf,
    pub runtime: PathBuf,
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
            Self::Source(value) | Self::IO(value) => f.write_str(value),
            Self::ToolMissing(tool) => write!(f, "the provisioned `{tool}` tool was not found"),
            Self::ToolFailed(tool, value) => {
                write!(f, "`{tool}` rejected the Pascal binding source: {value}")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scalar {
    Int,
    Float,
    Pointer,
    Void,
}

impl Scalar {
    fn jet(self) -> &'static str {
        match self {
            Self::Int | Self::Pointer => "Int",
            Self::Float => "Float",
            Self::Void => "",
        }
    }

    fn c(self) -> &'static str {
        match self {
            Self::Int => "int64_t",
            Self::Float => "double",
            Self::Pointer => "void*",
            Self::Void => "void",
        }
    }

    fn is_value(self) -> bool {
        matches!(self, Self::Int | Self::Float)
    }
}

#[derive(Clone)]
struct Param {
    name: String,
    kind: Scalar,
}

#[derive(Clone)]
struct Routine {
    name: String,
    params: Vec<Param>,
    result: Scalar,
}

struct Handle {
    group: String,
    ctor: Routine,
    close: Routine,
    methods: Vec<Routine>,
}

struct Surface {
    plain: Vec<Routine>,
    handle: Handle,
}

pub fn bind(path: &Path, source: &str, lib: &str, cache: &Path) -> Result<BindResult, BindError> {
    if !ident(lib) {
        return Err(BindError::Source(format!(
            "`{lib}` is not a valid Jet library name"
        )));
    }
    let surface = parse(source)?;
    let compiler = tool_path("fpc").ok_or(BindError::ToolMissing("fpc"))?;
    let c_compiler = tool_path("cc").ok_or(BindError::ToolMissing("cc"))?;
    let archiver = tool_path("ar").ok_or(BindError::ToolMissing("ar"))?;

    std::fs::create_dir_all(cache).map_err(|error| {
        BindError::IO(format!("could not create Pascal binding cache: {error}"))
    })?;
    let build = cache.join(format!(".pascal-build-{lib}"));
    let _ = std::fs::remove_dir_all(&build);
    std::fs::create_dir_all(&build).map_err(|error| {
        BindError::IO(format!("could not create Pascal build directory: {error}"))
    })?;

    let runtime = cache.join(format!("libjet_pascal_{lib}_runtime{}", shared_ext()));
    let _ = std::fs::remove_file(&runtime);
    let unit_dir = format!("-FU{}", build.display());
    let output_dir = format!("-FE{}", cache.display());
    let output_file = format!("-o{}", runtime.display());
    run(
        Command::new(&compiler)
            .arg("-Mobjfpc")
            .arg("-Cg")
            .arg("-O2")
            .arg(unit_dir)
            .arg(output_dir)
            .arg(output_file)
            .arg(path),
        "fpc",
    )?;
    if !runtime.is_file() {
        return Err(BindError::Source(
            "FreePascal did not produce the requested shared library".into(),
        ));
    }

    let abi = format!("jet_pascal_{lib}");
    let bridge = build.join(format!("{abi}.c"));
    let object = build.join(format!("{abi}.o"));
    std::fs::write(&bridge, render_c(lib, &surface))
        .map_err(|error| BindError::IO(format!("could not write Pascal handle bridge: {error}")))?;
    run(
        Command::new(&c_compiler)
            .args(["-std=c11", "-fPIC", "-c"])
            .arg(&bridge)
            .arg("-o")
            .arg(&object),
        "cc",
    )?;

    let archive = cache.join(format!("libjet_pascal_{lib}.a"));
    let _ = std::fs::remove_file(&archive);
    run(
        Command::new(&archiver)
            .arg("rcs")
            .arg(&archive)
            .arg(&object),
        "ar",
    )?;

    let mut identity = b"jet-pascal-bind-v1\0".to_vec();
    identity.extend_from_slice(source.as_bytes());
    identity.push(0);
    identity.extend_from_slice(compiler.to_string_lossy().as_bytes());
    let mut bound = surface
        .plain
        .iter()
        .map(|routine| routine.name.clone())
        .collect::<Vec<_>>();
    bound.push(surface.handle.ctor.name.clone());
    bound.extend(
        surface
            .handle
            .methods
            .iter()
            .map(|routine| routine.name.clone()),
    );
    bound.push(surface.handle.close.name.clone());
    bound.sort();

    let source_identity = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string();
    let provenance = format!(
        "schema=jet-pascal-bind-v1\nsource={}\nlibrary={}\nabi=cdecl\nint64=Jet.Int\ndouble=Jet.Float\nhandles=opaque-int64-generation\ncompiler={}\nexports={}\nsha256={}\n",
        source_identity,
        lib,
        compiler.display(),
        bound.join(","),
        crate::SHA256::sha256_hex(&identity)
    );
    let generated = render_jet(lib, &surface);
    let _ = std::fs::remove_dir_all(&build);
    Ok(BindResult {
        source: generated,
        bound,
        archive,
        runtime,
        provenance,
    })
}

fn parse(source: &str) -> Result<Surface, BindError> {
    let text = strip_comments(source);
    let lower = text.to_ascii_lowercase();
    if !lower.trim_start().starts_with("library ") {
        return Err(BindError::Source(
            "the Pascal binder requires a `library` source".into(),
        ));
    }
    let export_at = lower
        .rfind("exports ")
        .ok_or_else(|| BindError::Source("the Pascal library has no `exports` list".into()))?;
    let export_tail = &lower[export_at + 8..];
    let export_end = export_tail
        .find(';')
        .ok_or_else(|| BindError::Source("the Pascal `exports` list is not terminated".into()))?;
    let exports = export_tail[..export_end]
        .split(',')
        .map(|value| value.split_whitespace().next().unwrap_or(""))
        .map(str::trim)
        .filter(|value| ident(value))
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    if exports.is_empty() {
        return Err(BindError::Source(
            "the Pascal `exports` list has no bindable names".into(),
        ));
    }

    let class = class_name(&lower)
        .ok_or_else(|| BindError::Source("no Object Pascal class declaration was found".into()))?;
    let group = class.trim_start_matches('t').to_string();
    if group.is_empty() {
        return Err(BindError::Source(
            "the Pascal class name cannot form a handle name".into(),
        ));
    }

    let mut routines = Vec::new();
    let mut cursor = 0;
    while cursor < lower.len() {
        let function = lower[cursor..]
            .find("function ")
            .map(|offset| (cursor + offset, false));
        let procedure = lower[cursor..]
            .find("procedure ")
            .map(|offset| (cursor + offset, true));
        let next = match (function, procedure) {
            (Some(left), Some(right)) => Some(if left.0 < right.0 { left } else { right }),
            (Some(value), None) | (None, Some(value)) => Some(value),
            (None, None) => None,
        };
        let Some((start, is_procedure)) = next else {
            break;
        };
        let after = start + 1;
        let next_function = lower[after..]
            .find("function ")
            .map(|offset| after + offset);
        let next_procedure = lower[after..]
            .find("procedure ")
            .map(|offset| after + offset);
        let next_declaration = match (next_function, next_procedure) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (Some(value), None) | (None, Some(value)) => Some(value),
            (None, None) => None,
        };
        let Some(cdecl_relative) = lower[start..].find("cdecl;") else {
            cursor = next_declaration.unwrap_or(lower.len());
            continue;
        };
        let end = start + cdecl_relative;
        if next_declaration.is_some_and(|value| value < end) {
            cursor = next_declaration.unwrap();
            continue;
        }
        let header = &lower[start..end];
        let routine = parse_routine(header, is_procedure)?;
        if exports.contains(&routine.name)
            && !routines
                .iter()
                .any(|value: &Routine| value.name == routine.name)
        {
            routines.push(routine);
        }
        cursor = end + "cdecl;".len();
    }

    for export in &exports {
        if !routines.iter().any(|routine| routine.name == *export) {
            return Err(BindError::Source(format!(
                "exported Pascal name `{export}` has no bindable cdecl declaration"
            )));
        }
    }

    let ctor_name = format!("{group}_new");
    let close_names = [format!("{group}_free"), format!("{group}_close")];
    let ctor = routines
        .iter()
        .find(|routine| routine.name == ctor_name && routine.result == Scalar::Pointer)
        .cloned()
        .ok_or_else(|| {
            BindError::Source(format!(
                "class `{class}` needs exported cdecl `{ctor_name}` returning Pointer"
            ))
        })?;
    if ctor
        .params
        .iter()
        .any(|param| param.kind == Scalar::Pointer)
    {
        return Err(BindError::Source(format!(
            "constructor `{ctor_name}` has a pointer parameter; pointer ownership must stay in the handle"
        )));
    }
    let close = routines
        .iter()
        .find(|routine| {
            close_names.contains(&routine.name)
                && routine.result == Scalar::Void
                && routine.params.len() == 1
                && routine.params[0].kind == Scalar::Pointer
        })
        .cloned()
        .ok_or_else(|| {
            BindError::Source(format!(
                "class `{class}` needs exported cdecl `{group}_free(handle: Pointer)`"
            ))
        })?;

    let methods = routines
        .iter()
        .filter(|routine| {
            routine.name.starts_with(&format!("{group}_"))
                && routine.name != ctor.name
                && routine.name != close.name
        })
        .cloned()
        .collect::<Vec<_>>();
    for method in &methods {
        if method.params.first().map(|param| param.kind) != Some(Scalar::Pointer)
            || !method.result.is_value()
            || method
                .params
                .iter()
                .skip(1)
                .any(|param| param.kind == Scalar::Pointer)
        {
            return Err(BindError::Source(format!(
                "method `{}` must return Int or Float and use only a pointer-first handle plus scalar parameters",
                method.name
            )));
        }
    }

    let mut plain = Vec::new();
    for routine in routines {
        let is_handle_routine = routine.name == ctor.name
            || routine.name == close.name
            || methods.iter().any(|method| method.name == routine.name);
        if is_handle_routine {
            continue;
        }
        if routine.result.is_value()
            && routine
                .params
                .iter()
                .all(|param| param.kind != Scalar::Pointer)
        {
            plain.push(routine);
        } else {
            return Err(BindError::Source(format!(
                "exported cdecl `{}` has an unsupported ABI shape; use Int64 or Double scalars, or a pointer-first handle method",
                routine.name
            )));
        }
    }
    Ok(Surface {
        plain,
        handle: Handle {
            group,
            ctor,
            close,
            methods,
        },
    })
}

fn parse_routine(header: &str, is_procedure: bool) -> Result<Routine, BindError> {
    let keyword = if is_procedure {
        "procedure "
    } else {
        "function "
    };
    let rest = header
        .strip_prefix(keyword)
        .ok_or_else(|| BindError::Source("malformed cdecl routine".into()))?;
    let open = rest.find('(').ok_or_else(|| {
        BindError::Source("cdecl routine needs an explicit parameter list".into())
    })?;
    let name = rest[..open].trim().to_string();
    if !ident(&name) {
        return Err(BindError::Source(format!(
            "`{name}` is not a bindable Pascal export"
        )));
    }
    let close = matching_close(rest, open).ok_or_else(|| {
        BindError::Source(format!("routine `{name}` has no closed parameter list"))
    })?;
    let mut params = Vec::new();
    for group in rest[open + 1..close].split(';') {
        if group.trim().is_empty() {
            continue;
        }
        let Some((raw_names, raw_kind)) = group.split_once(':') else {
            return Err(BindError::Source(format!(
                "routine `{name}` has a malformed parameter"
            )));
        };
        let names = raw_names.trim();
        if names.starts_with("var ") || names.starts_with("out ") {
            return Err(BindError::Source(format!(
                "routine `{name}` uses a by-reference parameter; only cdecl value parameters are supported"
            )));
        }
        let names = names.strip_prefix("const ").unwrap_or(names);
        let kind = scalar(raw_kind).ok_or_else(|| {
            BindError::Source(format!(
                "routine `{name}` uses unsupported parameter type `{}`",
                raw_kind.trim()
            ))
        })?;
        for item in names.split(',') {
            let item = item.trim().to_string();
            if !ident(&item) {
                return Err(BindError::Source(format!(
                    "routine `{name}` has invalid parameter `{item}`"
                )));
            }
            params.push(Param { name: item, kind });
        }
    }
    let result = if is_procedure {
        Scalar::Void
    } else {
        let tail = rest[close + 1..].trim();
        let kind = tail
            .strip_prefix(':')
            .ok_or_else(|| BindError::Source(format!("function `{name}` has no result type")))?
            .split(';')
            .next()
            .unwrap_or("");
        scalar(kind).ok_or_else(|| {
            BindError::Source(format!(
                "function `{name}` uses unsupported result type `{}`",
                kind.trim()
            ))
        })?
    };
    Ok(Routine {
        name,
        params,
        result,
    })
}

fn scalar(value: &str) -> Option<Scalar> {
    match value.trim().to_ascii_lowercase().as_str() {
        "int64" => Some(Scalar::Int),
        "double" => Some(Scalar::Float),
        "pointer" => Some(Scalar::Pointer),
        _ => None,
    }
}

fn render_jet(lib: &str, surface: &Surface) -> String {
    let abi = format!("jet_pascal_{lib}");
    let ty = pascal_case(&surface.handle.group);
    let mut output = format!(
        "use c.{abi} as abi\n\n#Extern module c.{abi} {{\n    fn take_error() Int = \"{abi}_take_error\"\n"
    );
    for routine in &surface.plain {
        raw_jet(&mut output, &abi, routine, &routine.name);
    }
    raw_jet(
        &mut output,
        &abi,
        &surface.handle.ctor,
        &surface.handle.ctor.name,
    );
    for routine in &surface.handle.methods {
        raw_jet(&mut output, &abi, routine, &routine.name);
    }
    raw_jet(
        &mut output,
        &abi,
        &surface.handle.close,
        &format!("{}_close", surface.handle.group),
    );
    output.push_str(
        "}\n\npub enum PascalError {\n    InvalidHandle\n    Foreign\n    ResourceLimit\n}\n\n",
    );
    output.push_str("pub struct ");
    output.push_str(&ty);
    output.push_str(" {\n    value: Int\n}\n\n");
    output.push_str("fn error(code: Int) PascalError -> {\n");
    output.push_str("    if code == 1 -> return PascalError.InvalidHandle\n");
    output.push_str("    if code == 2 -> return PascalError.ResourceLimit\n");
    output.push_str("    return PascalError.Foreign\n}\n\n");

    for routine in &surface.plain {
        output.push_str("pub fn ");
        output.push_str(&routine.name);
        params_jet(&mut output, &routine.params, 0);
        output.push(' ');
        output.push_str(routine.result.jet());
        output.push_str(" -> {\n    return abi.");
        output.push_str(&routine.name);
        call_args(&mut output, &routine.params, 0, None);
        output.push_str("\n}\n\n");
    }

    output.push_str("pub fn ");
    output.push_str(&surface.handle.ctor.name);
    params_jet(&mut output, &surface.handle.ctor.params, 0);
    output.push(' ');
    output.push_str(&ty);
    output.push_str(" PascalError! -> {\n    raw_handle :: abi.");
    output.push_str(&surface.handle.ctor.name);
    call_args(&mut output, &surface.handle.ctor.params, 0, None);
    output.push_str(
        "\n    code :: abi.take_error()\n    if code != 0 -> return Err(error(code))\n    return Ok(",
    );
    output.push_str(&ty);
    output.push_str("{value: raw_handle})\n}\n\n");

    for routine in &surface.handle.methods {
        output.push_str("pub fn ");
        output.push_str(&routine.name);
        output.push_str("(handle: ");
        output.push_str(&ty);
        for param in routine.params.iter().skip(1) {
            output.push_str(", ");
            output.push_str(&param.name);
            output.push_str(": ");
            output.push_str(param.kind.jet());
        }
        output.push_str(") ");
        output.push_str(routine.result.jet());
        output.push_str(" PascalError! -> {\n    result_value :: abi.");
        output.push_str(&routine.name);
        call_args(&mut output, &routine.params, 1, Some("handle.value"));
        output.push_str(
            "\n    code :: abi.take_error()\n    if code != 0 -> return Err(error(code))\n    return Ok(result_value)\n}\n\n",
        );
    }

    output.push_str("pub fn close(^handle: ");
    output.push_str(&ty);
    output.push_str(") {}\n\nimpl ");
    output.push_str(&ty);
    output.push_str(".Close {\n    fn close(^self) {\n        abi.");
    output.push_str(&format!("{}_close", surface.handle.group));
    output.push_str("(self.value)\n        if abi.take_error() != 0 -> panic(\"invalid Pascal handle\")\n    }\n}\n");
    output
}

fn raw_jet(output: &mut String, abi: &str, routine: &Routine, name: &str) {
    output.push_str("    fn ");
    output.push_str(name);
    params_jet(output, &routine.params, 0);
    if routine.result != Scalar::Void {
        output.push(' ');
        output.push_str(routine.result.jet());
    }
    output.push_str(" = \"");
    output.push_str(&format!("{abi}_{name}\"\n"));
}

fn params_jet(output: &mut String, params: &[Param], skip: usize) {
    output.push('(');
    for (index, param) in params.iter().skip(skip).enumerate() {
        if index > 0 {
            output.push_str(", ");
        }
        output.push_str(&param.name);
        output.push_str(": ");
        output.push_str(param.kind.jet());
    }
    output.push(')');
}

fn call_args(output: &mut String, params: &[Param], skip: usize, first: Option<&str>) {
    output.push('(');
    let mut any = false;
    if let Some(value) = first {
        output.push_str(value);
        any = true;
    }
    for param in params.iter().skip(skip) {
        if any {
            output.push_str(", ");
        }
        output.push_str(&param.name);
        any = true;
    }
    output.push(')');
}

fn render_c(lib: &str, surface: &Surface) -> String {
    const SLOT_COUNT: usize = 64;
    const SLOT_BITS: usize = 7;
    let abi = format!("jet_pascal_{lib}");
    let handle = &surface.handle;
    let mut output = format!(
        "#include <stdint.h>\n#include <pthread.h>\n#include <stdlib.h>\n#define JET_PASCAL_SLOTS {SLOT_COUNT}\n#define JET_PASCAL_SLOT_BITS {SLOT_BITS}\n#define JET_PASCAL_SLOT_MASK ((uint64_t)((1ULL << JET_PASCAL_SLOT_BITS) - 1))\nstatic void* slots[JET_PASCAL_SLOTS];\nstatic uint32_t generations[JET_PASCAL_SLOTS];\nstatic pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;\nstatic pthread_once_t once = PTHREAD_ONCE_INIT;\nstatic _Thread_local int64_t failed;\n"
    );
    for routine in &surface.plain {
        raw_c_decl(&mut output, routine);
    }
    raw_c_decl(&mut output, &handle.ctor);
    for routine in &handle.methods {
        raw_c_decl(&mut output, routine);
    }
    raw_c_decl(&mut output, &handle.close);
    output.push_str("static int decode(int64_t handle, unsigned* slot, uint32_t* generation) {\n");
    output.push_str("    if (handle <= 0) return 0;\n");
    output.push_str("    uint64_t bits = (uint64_t)handle;\n");
    output.push_str("    uint64_t encoded_slot = bits & JET_PASCAL_SLOT_MASK;\n");
    output.push_str("    uint64_t encoded_generation = bits >> JET_PASCAL_SLOT_BITS;\n");
    output.push_str("    if (encoded_slot == 0 || encoded_slot > JET_PASCAL_SLOTS || encoded_generation == 0 || encoded_generation > UINT32_MAX) return 0;\n");
    output.push_str("    *slot = (unsigned)(encoded_slot - 1);\n    *generation = (uint32_t)encoded_generation;\n    return 1;\n}\n");
    output.push_str("static void finish(void) {\n    for (unsigned i = 0; i < JET_PASCAL_SLOTS; ++i) {\n        pthread_mutex_lock(&lock);\n        void* value = slots[i];\n        slots[i] = 0;\n        pthread_mutex_unlock(&lock);\n        if (value) ");
    output.push_str(&handle.close.name);
    output.push_str("(value);\n    }\n}\nstatic void init(void) { atexit(finish); }\n");
    output.push_str("static void* get(int64_t handle) {\n    unsigned slot; uint32_t generation;\n    if (!decode(handle, &slot, &generation)) { failed = 1; return 0; }\n    pthread_mutex_lock(&lock);\n    void* value = slots[slot];\n    int valid = value && generations[slot] == generation;\n    pthread_mutex_unlock(&lock);\n    if (!valid) { failed = 1; return 0; }\n    return value;\n}\n");
    output.push_str(&format!(
        "int64_t {abi}_take_error(void) {{ int64_t value = failed; failed = 0; return value; }}\n"
    ));
    for routine in &surface.plain {
        wrapper_c(&mut output, &abi, routine, &routine.name, None);
    }
    output.push_str(&format!("int64_t {abi}_{}(", handle.ctor.name));
    c_params(&mut output, &handle.ctor.params, 0);
    output.push_str(
        ") {\n    failed = 0;\n    pthread_once(&once, init);\n    void* jet_pascal_owned = ",
    );
    output.push_str(&handle.ctor.name);
    c_args(&mut output, &handle.ctor.params, 0, None);
    output.push_str(";\n    if (!jet_pascal_owned) { failed = 2; return 0; }\n    pthread_mutex_lock(&lock);\n    for (unsigned i = 0; i < JET_PASCAL_SLOTS; ++i) {\n        if (!slots[i]) {\n            uint32_t generation = generations[i] + 1;\n            if (!generation) generation = 1;\n            generations[i] = generation;\n            slots[i] = jet_pascal_owned;\n            int64_t handle = ((int64_t)generation << JET_PASCAL_SLOT_BITS) | (int64_t)(i + 1);\n            pthread_mutex_unlock(&lock);\n            return handle;\n        }\n    }\n    pthread_mutex_unlock(&lock);\n    ");
    output.push_str(&handle.close.name);
    output.push_str("(jet_pascal_owned); failed = 2; return 0;\n}\n");
    for routine in &handle.methods {
        wrapper_c(
            &mut output,
            &abi,
            routine,
            &routine.name,
            Some("jet_pascal_borrowed"),
        );
    }
    output.push_str(&format!(
        "void {abi}_{}_close(int64_t handle) {{\n    failed = 0;\n    unsigned slot; uint32_t generation;\n    if (!decode(handle, &slot, &generation)) {{ failed = 1; return; }}\n    pthread_mutex_lock(&lock);\n    void* value = slots[slot];\n    int valid = value && generations[slot] == generation;\n    if (valid) slots[slot] = 0;\n    pthread_mutex_unlock(&lock);\n    if (!valid) {{ failed = 1; return; }}\n    ",
        handle.group
    ));
    output.push_str(&handle.close.name);
    output.push_str("(value);\n}\n");
    output
}

fn raw_c_decl(output: &mut String, routine: &Routine) {
    output.push_str("extern ");
    output.push_str(routine.result.c());
    output.push(' ');
    output.push_str(&routine.name);
    output.push('(');
    c_params(output, &routine.params, 0);
    output.push_str(");\n");
}

fn wrapper_c(output: &mut String, abi: &str, routine: &Routine, name: &str, handle: Option<&str>) {
    output.push_str(routine.result.c());
    output.push(' ');
    output.push_str(&format!("{abi}_{name}("));
    if handle.is_some() {
        output.push_str("int64_t jet_pascal_handle");
        if routine.params.len() > 1 {
            output.push(',');
        }
        c_params(output, &routine.params, 1);
    } else {
        c_params(output, &routine.params, 0);
    }
    output.push_str("){ failed = 0; ");
    if let Some(value) = handle {
        output.push_str("void*");
        output.push_str(value);
        output.push_str(" = get(jet_pascal_handle); if (!");
        output.push_str(value);
        output.push_str(") return 0; ");
    }
    output.push_str("return ");
    output.push_str(&routine.name);
    c_args(
        output,
        &routine.params,
        usize::from(handle.is_some()),
        handle,
    );
    output.push_str("; }\n");
}

fn c_params(output: &mut String, params: &[Param], skip: usize) {
    if params.len() == skip {
        output.push_str("void");
    }
    for (index, param) in params.iter().skip(skip).enumerate() {
        if index > 0 {
            output.push(',');
        }
        output.push_str(param.kind.c());
        output.push(' ');
        output.push_str(&param.name);
    }
}

fn c_args(output: &mut String, params: &[Param], skip: usize, first: Option<&str>) {
    output.push('(');
    let mut any = false;
    if let Some(value) = first {
        output.push_str(value);
        any = true;
    }
    for param in params.iter().skip(skip) {
        if any {
            output.push(',');
        }
        output.push_str(&param.name);
        any = true;
    }
    output.push(')');
}

fn strip_comments(source: &str) -> String {
    let mut output = String::new();
    let mut chars = source.chars().peekable();
    let mut brace = false;
    let mut paren = false;
    while let Some(character) = chars.next() {
        if brace {
            if character == '}' {
                brace = false;
            }
            continue;
        }
        if paren {
            if character == '*' && chars.peek() == Some(&')') {
                chars.next();
                paren = false;
            }
            continue;
        }
        if character == '{' {
            brace = true;
            continue;
        }
        if character == '(' && chars.peek() == Some(&'*') {
            chars.next();
            paren = true;
            continue;
        }
        if character == '/' && chars.peek() == Some(&'/') {
            for next in chars.by_ref() {
                if next == '\n' {
                    output.push('\n');
                    break;
                }
            }
            continue;
        }
        output.push(character);
    }
    output
}

fn class_name(lower: &str) -> Option<String> {
    let at = lower.find("= class")?;
    lower[..at]
        .split_whitespace()
        .last()
        .filter(|value| ident(value))
        .map(str::to_string)
}

fn matching_close(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0;
    for (index, character) in text.char_indices().skip_while(|(index, _)| *index < open) {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn tool_path(tool: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(tool))
        .find(|candidate| candidate.is_file())
        .and_then(|candidate| std::fs::canonicalize(candidate).ok())
}

fn run(command: &mut Command, tool: &'static str) -> Result<(), BindError> {
    const CAP: usize = 64 * 1024;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            BindError::ToolMissing(tool)
        } else {
            BindError::IO(format!("could not start `{tool}`: {error}"))
        }
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| BindError::IO(format!("could not supervise `{tool}` stdout")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| BindError::IO(format!("could not supervise `{tool}` stderr")))?;
    let output = std::thread::spawn(move || drain(stdout, CAP));
    let errors = std::thread::spawn(move || drain(stderr, CAP));
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        match child
            .try_wait()
            .map_err(|error| BindError::IO(format!("could not supervise `{tool}`: {error}")))?
        {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = output.join();
                let _ = errors.join();
                return Err(BindError::ToolFailed(
                    tool,
                    "the tool exceeded the 60 second limit".into(),
                ));
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    let stdout = output
        .join()
        .map_err(|_| BindError::IO(format!("`{tool}` stdout reader failed")))??;
    let stderr = errors
        .join()
        .map_err(|_| BindError::IO(format!("`{tool}` stderr reader failed")))??;
    if status.success() {
        Ok(())
    } else {
        let detail = if stderr.is_empty() { &stdout } else { &stderr };
        Err(BindError::ToolFailed(tool, launder(detail)))
    }
}

fn drain(mut input: impl Read, limit: usize) -> Result<Vec<u8>, BindError> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let size = input.read(&mut buffer).map_err(|error| {
            BindError::IO(format!("could not read foreign tool output: {error}"))
        })?;
        if size == 0 {
            break;
        }
        let keep = (limit - output.len()).min(size);
        output.extend_from_slice(&buffer[..keep]);
    }
    Ok(output)
}

fn launder(value: &[u8]) -> String {
    let text = String::from_utf8_lossy(value);
    if text.lines().any(|line| line.contains("Fatal:")) {
        return "compilation failed".into();
    }
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(160).collect())
        .unwrap_or_else(|| "the foreign tool returned a failure status".into())
}

fn pascal_case(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(character) => character.to_ascii_uppercase().to_string() + chars.as_str(),
        None => "Handle".into(),
    }
}

fn ident(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(
        chars.next(),
        Some(character) if character.is_ascii_alphabetic() || character == '_'
    ) && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

#[cfg(target_os = "linux")]
fn shared_ext() -> &'static str {
    ".so"
}

#[cfg(target_os = "macos")]
fn shared_ext() -> &'static str {
    ".dylib"
}

#[cfg(target_os = "windows")]
fn shared_ext() -> &'static str {
    ".dll"
}
