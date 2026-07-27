//! ISO_C_BINDING Fortran to Jet binding generator (D-FFI-FORTRAN1=A).
//!
//! Every exported routine must use `bind(C, name="...")`; scalar inputs carry
//! `value`, while fixed-shape arrays carry `intent(in)`. Array extents and
//! Fortran's column-major order become binding facts, and generated wrappers
//! reject a wrong flat-list length before entering foreign code. Unsupported
//! declarations fail binding instead of guessing an ABI (I3).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindResult {
    pub source: String,
    pub bound: Vec<String>,
    pub archive: PathBuf,
    pub layouts: Vec<ArrayLayoutFact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrayLayoutFact {
    pub routine: String,
    pub parameter: String,
    pub extents: Vec<usize>,
    pub order: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError {
    Source(String),
    ToolMissing(&'static str),
    ToolFailed { tool: &'static str, detail: String },
    IO(String),
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindError::Source(message) | BindError::IO(message) => f.write_str(message),
            BindError::ToolMissing(tool) => write!(f, "the provisioned `{tool}` tool was not found"),
            BindError::ToolFailed { tool, detail } => {
                write!(f, "`{tool}` rejected the ISO_C_BINDING source: {detail}")
            }
        }
    }
}

struct Routine {
    jet_name: String,
    symbol: String,
    params: Vec<(String, Param)>,
    result: Scalar,
}

impl Routine {
    fn has_arrays(&self) -> bool {
        self.params
            .iter()
            .any(|(_, param)| matches!(param, Param::Array { .. }))
    }
}

#[derive(Clone)]
enum Param {
    Scalar(Scalar),
    Array { scalar: Scalar, extents: Vec<usize> },
}

impl Param {
    fn jet(&self) -> String {
        match self {
            Self::Scalar(scalar) => scalar.jet().to_string(),
            Self::Array { scalar, .. } => format!("[{}]", scalar.jet()),
        }
    }
}

#[derive(Clone, Copy)]
enum Scalar {
    Int,
    Float,
}

impl Scalar {
    fn jet(self) -> &'static str {
        match self {
            Scalar::Int => "Int",
            Scalar::Float => "Float",
        }
    }

    fn fortran(self) -> &'static str {
        match self {
            Scalar::Int => "integer(c_int64_t)",
            Scalar::Float => "real(c_double)",
        }
    }
}

/// Discover supported routines, generate the Jet wrapper, compile the Fortran
/// object, and archive it beside the generated cache.
pub fn bind(source_path: &Path, source: &str, lib: &str, cache_dir: &Path) -> Result<BindResult, BindError> {
    if !is_ident(lib) {
        return Err(BindError::Source(format!("`{lib}` is not a valid Jet library name")));
    }
    let routines = parse_routines(source)?;
    let has_arrays = routines.iter().any(Routine::has_arrays);
    let module = if has_arrays {
        Some(parse_module_name(source).ok_or_else(|| {
            BindError::Source("array bindings need a named Fortran module".to_string())
        })?)
    } else {
        None
    };
    std::fs::create_dir_all(cache_dir)
        .map_err(|e| BindError::IO(format!("could not create binding cache: {e}")))?;

    let stem = format!("jet_fortran_{lib}");
    let object = cache_dir.join(format!("{stem}.o"));
    let bridge_source = cache_dir.join(format!("{stem}_bridge.f90"));
    let bridge_object = cache_dir.join(format!("{stem}_bridge.o"));
    let archive = cache_dir.join(format!("lib{stem}.a"));
    let _ = std::fs::remove_file(&object);
    let _ = std::fs::remove_file(&bridge_object);
    let _ = std::fs::remove_file(&archive);
    compile_fortran(source_path, &object, cache_dir)?;
    let mut archive_inputs = vec![object.clone()];
    if has_arrays {
        std::fs::write(
            &bridge_source,
            render_bridge(module.as_deref().unwrap_or_default(), lib, &routines),
        )
            .map_err(|e| BindError::IO(format!("could not write the Fortran array bridge: {e}")))?;
        compile_fortran(&bridge_source, &bridge_object, cache_dir)?;
        archive_inputs.push(bridge_object.clone());
    }
    let mut archive_command = Command::new("ar");
    archive_command
        .arg("rcs")
        .arg(&archive);
    archive_command.args(&archive_inputs);
    let output = supervised_output(&mut archive_command, "ar")?;
    if !output.status.success() {
        return Err(BindError::ToolFailed {
            tool: "ar",
            detail: launder_tool_output(&output.stderr),
        });
    }
    let _ = std::fs::remove_file(&object);
    let _ = std::fs::remove_file(&bridge_object);
    let _ = std::fs::remove_file(&bridge_source);

    let source = render(lib, &routines);
    let layouts = routines.iter().flat_map(|routine| {
        routine.params.iter().filter_map(|(parameter, param)| match param {
            Param::Array { extents, .. } => Some(ArrayLayoutFact {
                routine: routine.jet_name.clone(),
                parameter: parameter.clone(),
                extents: extents.clone(),
                order: "column-major",
            }),
            Param::Scalar(_) => None,
        })
    }).collect();
    Ok(BindResult {
        source,
        bound: routines.into_iter().map(|r| r.jet_name).collect(),
        archive,
        layouts,
    })
}

fn compile_fortran(source: &Path, object: &Path, module_dir: &Path) -> Result<(), BindError> {
    let output = supervised_output(Command::new("gfortran")
        .args(["-c", "-fPIC", "-ffree-line-length-none"])
        .arg("-J")
        .arg(module_dir)
        .arg("-I")
        .arg(module_dir)
        .arg(source)
        .arg("-o")
        .arg(object), "gfortran")?;
    if output.status.success() {
        Ok(())
    } else {
        Err(BindError::ToolFailed {
            tool: "gfortran",
            detail: launder_tool_output(&output.stderr),
        })
    }
}

struct ToolOutput {
    status: std::process::ExitStatus,
    stderr: Vec<u8>,
}

fn supervised_output(command: &mut Command, tool: &'static str) -> Result<ToolOutput, BindError> {
    const CAPTURE_LIMIT: usize = 64 * 1024;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| if e.kind() == std::io::ErrorKind::NotFound {
        BindError::ToolMissing(tool)
    } else {
        BindError::IO(format!("could not start `{tool}`: {e}"))
    })?;
    let stdout = child.stdout.take().ok_or_else(|| BindError::IO(format!("could not supervise `{tool}` stdout")))?;
    let stderr = child.stderr.take().ok_or_else(|| BindError::IO(format!("could not supervise `{tool}` stderr")))?;
    let out = std::thread::spawn(move || bounded_read(stdout, CAPTURE_LIMIT));
    let err = std::thread::spawn(move || bounded_read(stderr, CAPTURE_LIMIT));
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        match child.try_wait().map_err(|e| BindError::IO(format!("could not supervise `{tool}`: {e}")))? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = out.join();
                let _ = err.join();
                return Err(BindError::ToolFailed { tool, detail: "the tool exceeded the 60 second limit".into() });
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    let _ = out.join().map_err(|_| BindError::IO(format!("`{tool}` stdout reader failed")))??;
    let stderr = err.join().map_err(|_| BindError::IO(format!("`{tool}` stderr reader failed")))??;
    Ok(ToolOutput { status, stderr })
}

fn bounded_read(mut input: impl Read, limit: usize) -> Result<Vec<u8>, BindError> {
    let mut captured = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = input.read(&mut buffer).map_err(|e| BindError::IO(format!("could not read foreign tool output: {e}")))?;
        if count == 0 { break; }
        let keep = limit.saturating_sub(captured.len()).min(count);
        captured.extend_from_slice(&buffer[..keep]);
    }
    Ok(captured)
}

fn launder_tool_output(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    text.lines()
        .find_map(|line| line.split_once("Error:").map(|(_, detail)| detail.trim()))
        .filter(|detail| !detail.is_empty())
        .map(|detail| detail.chars().take(160).collect())
        .unwrap_or_else(|| "the foreign tool returned a failure status".to_string())
}

fn parse_routines(source: &str) -> Result<Vec<Routine>, BindError> {
    let lines: Vec<&str> = source.lines().collect();
    let mut routines = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = strip_comment(lines[i]).trim();
        let lower = line.to_ascii_lowercase();
        let Some(function_at) = lower.find("function ") else {
            i += 1;
            continue;
        };
        if !lower.contains("bind") || !lower.contains("(c") {
            i += 1;
            continue;
        }
        let header = &line[function_at + "function ".len()..];
        let open = header.find('(').ok_or_else(|| BindError::Source("malformed Fortran function header".into()))?;
        let jet_name = header[..open].trim().to_string();
        if !is_ident(&jet_name) {
            return Err(BindError::Source(format!("`{jet_name}` is not a bindable routine name")));
        }
        let close = header[open + 1..].find(')').map(|n| open + 1 + n)
            .ok_or_else(|| BindError::Source(format!("function `{jet_name}` has no closed parameter list")))?;
        let names: Vec<String> = header[open + 1..close]
            .split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect();
        let symbol = parse_bind_name(header).ok_or_else(|| BindError::Source(format!(
            "function `{jet_name}` must declare `bind(C, name=\"...\")`"
        )))?;
        let result_name = parse_result_name(header).unwrap_or_else(|| jet_name.clone());
        let mut declarations = Vec::new();
        i += 1;
        while i < lines.len() {
            let body = strip_comment(lines[i]).trim();
            let body_lower = body.to_ascii_lowercase();
            if body_lower.starts_with("end function") {
                break;
            }
            if body.contains("::") {
                declarations.push(body.to_string());
            }
            i += 1;
        }
        let mut params = Vec::new();
        for name in &names {
            let declaration = find_decl(&declarations, name).ok_or_else(|| BindError::Source(format!(
                "parameter `{name}` in `{jet_name}` needs an explicit ISO_C_BINDING declaration"
            )))?;
            let param = match declaration.extents {
                Some(extents) => {
                    if declaration.value {
                        return Err(BindError::Source(format!("array parameter `{name}` in `{jet_name}` cannot use `value`")));
                    }
                    if !declaration.intent_in {
                        return Err(BindError::Source(format!("array parameter `{name}` in `{jet_name}` must declare `intent(in)`")));
                    }
                    Param::Array { scalar: declaration.scalar, extents }
                }
                None => {
                    if !declaration.value {
                        return Err(BindError::Source(format!(
                            "scalar parameter `{name}` in `{jet_name}` must use `value` to match the C ABI"
                        )));
                    }
                    Param::Scalar(declaration.scalar)
                }
            };
            params.push((name.clone(), param));
        }
        let result_decl = find_decl(&declarations, &result_name).ok_or_else(|| BindError::Source(format!(
            "result `{result_name}` in `{jet_name}` needs an explicit ISO_C_BINDING declaration"
        )))?;
        if result_decl.extents.is_some() {
            return Err(BindError::Source(format!("result `{result_name}` in `{jet_name}` must be a scalar")));
        }
        let result = result_decl.scalar;
        routines.push(Routine { jet_name, symbol, params, result });
        i += 1;
    }
    if routines.is_empty() {
        return Err(BindError::Source(
            "no supported `function ... bind(C, name=\"...\")` routines were found".into(),
        ));
    }
    Ok(routines)
}

struct Declaration {
    scalar: Scalar,
    value: bool,
    intent_in: bool,
    extents: Option<Vec<usize>>,
}

fn find_decl(declarations: &[String], name: &str) -> Option<Declaration> {
    for declaration in declarations {
        let (type_part, names_part) = declaration.split_once("::")?;
        let entity = split_top_level(names_part).into_iter().find(|candidate| {
            candidate.split('(').next().unwrap_or(candidate).trim().eq_ignore_ascii_case(name)
        });
        let Some(entity) = entity else {
            continue;
        };
        let lower = type_part.to_ascii_lowercase();
        let scalar = if lower.contains("integer(c_int64_t)") {
            Scalar::Int
        } else if lower.contains("real(c_double)") {
            Scalar::Float
        } else {
            return None;
        };
        let value = lower.split(',').map(str::trim).any(|part| part == "value");
        let intent_in = lower.split(',').map(str::trim).any(|part| part == "intent(in)");
        let dims = entity.split_once('(').and_then(|(_, tail)| tail.rsplit_once(')').map(|(inside, _)| inside))
            .or_else(|| parse_dimension_attr(&lower));
        let extents = match dims {
            Some(raw) => {
                let mut extents = Vec::new();
                for part in raw.split(',').map(str::trim) {
                    let extent = part.parse::<usize>().ok().filter(|n| *n > 0)?;
                    extents.push(extent);
                }
                if extents.is_empty() { return None; }
                Some(extents)
            }
            None => None,
        };
        return Some(Declaration { scalar, value, intent_in, extents });
    }
    None
}

fn parse_dimension_attr(type_part: &str) -> Option<&str> {
    let at = type_part.find("dimension(")? + "dimension(".len();
    type_part[at..].split(')').next()
}

fn split_top_level(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, ch) in value.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => { parts.push(value[start..index].trim()); start = index + 1; }
            _ => {}
        }
    }
    parts.push(value[start..].trim());
    parts
}

fn parse_bind_name(header: &str) -> Option<String> {
    let lower = header.to_ascii_lowercase();
    let bind_at = lower.find("bind")?;
    let name_at = bind_at + lower[bind_at..].find("name")?;
    let after = header[name_at + 4..].trim_start();
    let after = after.strip_prefix('=')?.trim_start();
    let quote = after.chars().next()?;
    if quote != '\'' && quote != '"' { return None; }
    let value = after[quote.len_utf8()..].split(quote).next()?;
    is_ident(value).then(|| value.to_string())
}

fn parse_result_name(header: &str) -> Option<String> {
    let lower = header.to_ascii_lowercase();
    let at = lower.find("result")?;
    let after = header[at + "result".len()..].trim_start();
    let inside = after.strip_prefix('(')?.split(')').next()?.trim();
    is_ident(inside).then(|| inside.to_string())
}

fn parse_module_name(source: &str) -> Option<String> {
    source.lines().find_map(|line| {
        let line = strip_comment(line).trim();
        let lower = line.to_ascii_lowercase();
        let rest = lower.strip_prefix("module ")?.trim();
        (!rest.starts_with("procedure") && is_ident(rest)).then(|| rest.to_string())
    })
}

fn render(lib: &str, routines: &[Routine]) -> String {
    let abi = format!("jet_fortran_{lib}");
    let mut out = format!("#Bindgen module c.{abi}.__bindgen__ {{\n");
    for routine in routines {
        for (name, param) in &routine.params {
            if let Param::Array { extents, .. } = param {
                out.push_str("    // fortran-layout ");
                out.push_str(&routine.jet_name);
                out.push('.');
                out.push_str(name);
                out.push_str(": column-major ");
                out.push_str(&extents.iter().map(usize::to_string).collect::<Vec<_>>().join("x"));
                out.push('\n');
            }
        }
        out.push_str("    fn ");
        out.push_str(&routine.jet_name);
        render_abi_params(&mut out, &routine.params);
        out.push_str(" -> ");
        out.push_str(routine.result.jet());
        out.push_str(" = \"");
        if routine.has_arrays() {
            out.push_str(&format!("jet_fortran_{lib}_{}", routine.jet_name));
        } else {
            out.push_str(&routine.symbol);
        }
        out.push_str("\"\n");
    }
    out.push_str("}\nuse c.");
    out.push_str(&abi);
    out.push_str(" as abi\n\n");
    for routine in routines {
        out.push_str("pub fn ");
        out.push_str(&routine.jet_name);
        render_params(&mut out, &routine.params);
        out.push_str(" =[Fortran]=> ");
        out.push_str(routine.result.jet());
        out.push_str(" {\n");
        for (name, param) in &routine.params {
            if let Param::Array { extents, .. } = param {
                let len: usize = extents.iter().product();
                out.push_str("    if "); out.push_str(name); out.push_str(".len() != "); out.push_str(&len.to_string());
                out.push_str(" { panic(\""); out.push_str(name); out.push_str(" must contain exactly "); out.push_str(&len.to_string());
                out.push_str(" column-major values\") }\n");
            }
        }
        out.push_str("    return abi.");
        out.push_str(&routine.jet_name);
        out.push('(');
        let mut first = true;
        for (name, param) in &routine.params {
            match param {
                Param::Scalar(_) => push_call_arg(&mut out, &mut first, name),
                Param::Array { extents, .. } => {
                    for index in 0..extents.iter().product() {
                        push_call_arg(&mut out, &mut first, &format!("{name}[{index}]"));
                    }
                }
            }
        }
        out.push_str(")\n}\n\n");
    }
    out
}

fn push_call_arg(out: &mut String, first: &mut bool, value: &str) {
    if !*first {
        out.push_str(", ");
    }
    *first = false;
    out.push_str(value);
}

fn render_abi_params(out: &mut String, params: &[(String, Param)]) {
    out.push('(');
    let mut first = true;
    for (name, param) in params {
        match param {
            Param::Scalar(scalar) => {
                if !first { out.push_str(", "); }
                first = false;
                out.push_str(name);
                out.push_str(": ");
                out.push_str(scalar.jet());
            }
            Param::Array { scalar, extents } => {
                for index in 0..extents.iter().product() {
                    if !first { out.push_str(", "); }
                    first = false;
                    out.push_str(&format!("{name}_{index}: {}", scalar.jet()));
                }
            }
        }
    }
    out.push(')');
}

fn render_bridge(module: &str, lib: &str, routines: &[Routine]) -> String {
    let mut out = format!("module jet_fortran_{lib}_bridge\n  use iso_c_binding\ncontains\n");
    for routine in routines.iter().filter(|routine| routine.has_arrays()) {
        let bridge = format!("jet_bridge_{}", routine.jet_name);
        let symbol = format!("jet_fortran_{lib}_{}", routine.jet_name);
        let mut args = Vec::new();
        for (name, param) in &routine.params {
            match param {
                Param::Scalar(_) => args.push(name.clone()),
                Param::Array { extents, .. } => {
                    for index in 0..extents.iter().product() {
                        args.push(format!("{name}_{index}"));
                    }
                }
            }
        }
        out.push_str(&format!(
            "  function {bridge}({}) result(jet_result) bind(C, name=\"{symbol}\")\n",
            args.join(", ")
        ));
        out.push_str(&format!(
            "    use {module}, only: jet_original => {}\n",
            routine.jet_name
        ));
        for (name, param) in &routine.params {
            match param {
                Param::Scalar(scalar) => out.push_str(&format!(
                    "    {}, value :: {name}\n",
                    scalar.fortran()
                )),
                Param::Array { scalar, extents } => {
                    let len: usize = extents.iter().product();
                    let expanded = (0..len)
                        .map(|index| format!("{name}_{index}"))
                        .collect::<Vec<_>>();
                    out.push_str(&format!(
                        "    {}, value :: {}\n",
                        scalar.fortran(),
                        expanded.join(", ")
                    ));
                    out.push_str(&format!(
                        "    {} :: {name}({})\n",
                        scalar.fortran(),
                        extents.iter().map(usize::to_string).collect::<Vec<_>>().join(",")
                    ));
                }
            }
        }
        out.push_str(&format!("    {} :: jet_result\n", routine.result.fortran()));
        for (name, param) in &routine.params {
            if let Param::Array { extents, .. } = param {
                for index in 0..extents.iter().product() {
                    let mut remaining = index;
                    let coordinates = extents
                        .iter()
                        .map(|extent| {
                            let coordinate = remaining % extent + 1;
                            remaining /= extent;
                            coordinate.to_string()
                        })
                        .collect::<Vec<_>>();
                    out.push_str(&format!(
                        "    {name}({}) = {name}_{index}\n",
                        coordinates.join(",")
                    ));
                }
            }
        }
        out.push_str("    jet_result = jet_original(");
        out.push_str(
            &routine
                .params
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push_str(")\n");
        out.push_str(&format!("  end function {bridge}\n"));
    }
    out.push_str(&format!("end module jet_fortran_{lib}_bridge\n"));
    out
}

fn render_params(out: &mut String, params: &[(String, Param)]) {
    out.push('(');
    for (index, (name, scalar)) in params.iter().enumerate() {
        if index > 0 { out.push_str(", "); }
        out.push_str(name);
        out.push_str(": ");
        out.push_str(&scalar.jet());
    }
    out.push(')');
}

fn strip_comment(line: &str) -> &str { line.split('!').next().unwrap_or(line) }

fn is_ident(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
