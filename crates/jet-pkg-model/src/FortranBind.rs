//! ISO_C_BINDING Fortran to Jet binding generator (D-FFI-FORTRAN1=A).
//!
//! This is deliberately a checked scalar bridge, not a general Fortran parser.
//! Every exported routine must use `bind(C, name="...")`; scalar inputs must
//! carry `value`. Unsupported declarations fail binding instead of guessing an
//! ABI (I3). The foreign compiler is a provisioned tool invoked out of process,
//! keeping compiler crates dependency-free (I6).

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindResult {
    pub source: String,
    pub bound: Vec<String>,
    pub archive: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError {
    Source(String),
    ToolMissing(&'static str),
    ToolFailed { tool: &'static str, detail: String },
    Io(String),
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindError::Source(message) | BindError::Io(message) => f.write_str(message),
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
    params: Vec<(String, Scalar)>,
    result: Scalar,
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
}

/// Discover supported routines, generate the Jet wrapper, compile the Fortran
/// object, and archive it beside the generated cache.
pub fn bind(source_path: &Path, source: &str, lib: &str, cache_dir: &Path) -> Result<BindResult, BindError> {
    if !is_ident(lib) {
        return Err(BindError::Source(format!("`{lib}` is not a valid Jet library name")));
    }
    let routines = parse_routines(source)?;
    std::fs::create_dir_all(cache_dir)
        .map_err(|e| BindError::Io(format!("could not create binding cache: {e}")))?;

    let stem = format!("jet_fortran_{lib}");
    let object = cache_dir.join(format!("{stem}.o"));
    let archive = cache_dir.join(format!("lib{stem}.a"));
    let _ = std::fs::remove_file(&object);
    let _ = std::fs::remove_file(&archive);
    run_tool(
        "gfortran",
        &["-c", "-fPIC"],
        source_path,
        &object,
    )?;
    let output = Command::new("ar")
        .arg("rcs")
        .arg(&archive)
        .arg(&object)
        .output()
        .map_err(|e| if e.kind() == std::io::ErrorKind::NotFound {
            BindError::ToolMissing("ar")
        } else {
            BindError::Io(format!("could not start `ar`: {e}"))
        })?;
    if !output.status.success() {
        return Err(BindError::ToolFailed {
            tool: "ar",
            detail: launder_tool_output(&output.stderr),
        });
    }
    let _ = std::fs::remove_file(&object);

    let source = render(lib, &routines);
    Ok(BindResult {
        source,
        bound: routines.into_iter().map(|r| r.jet_name).collect(),
        archive,
    })
}

fn run_tool(tool: &'static str, prefix: &[&str], source: &Path, object: &Path) -> Result<(), BindError> {
    let output = Command::new(tool)
        .args(prefix)
        .arg(source)
        .arg("-o")
        .arg(object)
        .output()
        .map_err(|e| if e.kind() == std::io::ErrorKind::NotFound {
            BindError::ToolMissing(tool)
        } else {
            BindError::Io(format!("could not start `{tool}`: {e}"))
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(BindError::ToolFailed {
            tool,
            detail: launder_tool_output(&output.stderr),
        })
    }
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
            let (scalar, value) = find_decl(&declarations, name).ok_or_else(|| BindError::Source(format!(
                "parameter `{name}` in `{jet_name}` needs an explicit ISO_C_BINDING declaration"
            )))?;
            if !value {
                return Err(BindError::Source(format!(
                    "scalar parameter `{name}` in `{jet_name}` must use `value` to match the C ABI"
                )));
            }
            params.push((name.clone(), scalar));
        }
        let (result, _) = find_decl(&declarations, &result_name).ok_or_else(|| BindError::Source(format!(
            "result `{result_name}` in `{jet_name}` needs an explicit ISO_C_BINDING declaration"
        )))?;
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

fn find_decl(declarations: &[String], name: &str) -> Option<(Scalar, bool)> {
    for declaration in declarations {
        let (type_part, names_part) = declaration.split_once("::")?;
        if !names_part.split(',').map(str::trim).any(|candidate| candidate.eq_ignore_ascii_case(name)) {
            continue;
        }
        let lower = type_part.to_ascii_lowercase();
        let scalar = if lower.contains("integer(c_int64_t)") {
            Scalar::Int
        } else if lower.contains("real(c_double)") {
            Scalar::Float
        } else {
            return None;
        };
        let value = lower.split(',').map(str::trim).any(|part| part == "value");
        return Some((scalar, value));
    }
    None
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

fn render(lib: &str, routines: &[Routine]) -> String {
    let abi = format!("jet_fortran_{lib}");
    let mut out = format!("#Extern module c.{abi} {{\n");
    for routine in routines {
        out.push_str("    fn ");
        out.push_str(&routine.jet_name);
        render_params(&mut out, &routine.params);
        out.push_str(" -> ");
        out.push_str(routine.result.jet());
        out.push_str(" = \"");
        out.push_str(&routine.symbol);
        out.push_str("\"\n");
    }
    out.push_str("}\nuse c.");
    out.push_str(&abi);
    out.push_str(" as abi\n\n");
    for routine in routines {
        out.push_str("pub fn ");
        out.push_str(&routine.jet_name);
        render_params(&mut out, &routine.params);
        out.push_str(" -> ");
        out.push_str(routine.result.jet());
        out.push_str(" {\n    return abi.");
        out.push_str(&routine.jet_name);
        out.push('(');
        for (index, (name, _)) in routine.params.iter().enumerate() {
            if index > 0 { out.push_str(", "); }
            out.push_str(name);
        }
        out.push_str(")\n}\n\n");
    }
    out
}

fn render_params(out: &mut String, params: &[(String, Scalar)]) {
    out.push('(');
    for (index, (name, scalar)) in params.iter().enumerate() {
        if index > 0 { out.push_str(", "); }
        out.push_str(name);
        out.push_str(": ");
        out.push_str(scalar.jet());
    }
    out.push(')');
}

fn strip_comment(line: &str) -> &str { line.split('!').next().unwrap_or(line) }

fn is_ident(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
