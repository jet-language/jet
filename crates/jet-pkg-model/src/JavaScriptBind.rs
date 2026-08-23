//! Typed JavaScript declaration binder (D-FFI-JS1 / D-FFI-UNIFY1).
//!
//! A declaration file is the ABI source of truth. The binder accepts only
//! scalar TypeScript declarations and executes the matching module through a
//! supervised Node worker. Dynamic or callback-shaped declarations fail closed.

use crate::ForeignBridge::{self, ScalarBridgeFunction};
use crate::AST::{binder_descriptor, ForeignLanguage, ForeignScalar};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindResult {
    pub source: String,
    pub bound: Vec<String>,
    pub archive: PathBuf,
    pub worker: PathBuf,
    pub provenance: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError {
    Source(String),
    ToolMissing(&'static str),
    ToolFailed(&'static str, String),
    Io(String),
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source(value) | Self::Io(value) => f.write_str(value),
            Self::ToolMissing(tool) => write!(f, "the provisioned {tool} tool was not found"),
            Self::ToolFailed(tool, detail) => {
                write!(f, "{tool} rejected the JavaScript binding: {detail}")
            }
        }
    }
}

pub fn bind(
    declaration_path: &Path,
    declarations: &str,
    runtime_path: &Path,
    lib: &str,
    cache: &Path,
) -> Result<BindResult, BindError> {
    if !is_ident(lib) {
        return Err(BindError::Source(format!(
            "{lib} is not a valid Jet library name"
        )));
    }
    let functions = parse(declarations)?;
    let descriptor = *binder_descriptor(ForeignLanguage::JS)
        .ok_or_else(|| BindError::Source("JS binder descriptor is not registered".into()))?;
    let _declaration_path = declaration_path.canonicalize().map_err(|error| {
        BindError::Io(format!(
            "could not resolve TypeScript declarations: {error}"
        ))
    })?;
    let runtime_path = runtime_path
        .canonicalize()
        .map_err(|error| BindError::Io(format!("could not resolve JavaScript runtime: {error}")))?;
    check_node(&runtime_path)?;
    std::fs::create_dir_all(cache).map_err(|error| {
        BindError::Io(format!(
            "could not create JavaScript binding cache: {error}"
        ))
    })?;
    let worker = cache.join(format!("{lib}_worker.mjs"));
    let declaration_digest = crate::SHA256::sha256_hex(declarations.as_bytes());
    let worker_source = format!(
        "// jet-ffi-declaration-sha256={declaration_digest}\n{}",
        render_worker(&functions)
    );
    std::fs::write(&worker, worker_source)
        .map_err(|error| BindError::Io(format!("could not write JavaScript worker: {error}")))?;
    let worker = worker
        .canonicalize()
        .map_err(|error| BindError::Io(format!("could not resolve JavaScript worker: {error}")))?;
    let abi = format!("jet_js_{lib}");
    let identity = ForeignBridge::scalar_bridge_identity(
        descriptor,
        lib,
        &abi,
        "node",
        &runtime_path,
        &worker,
        &functions,
    )
    .map_err(BindError::Source)?;
    let archive = ForeignBridge::compile_scalar_sidecar_with_identity(
        cache,
        &identity,
        &abi,
        "node",
        &worker,
        &runtime_path,
        descriptor,
        &functions,
    )
    .map_err(BindError::Source)?;
    let provenance_path = ForeignBridge::write_scalar_provenance_with_functions(
        cache,
        lib,
        descriptor,
        "node",
        &runtime_path,
        &worker,
        &archive,
        &functions,
    )
    .map_err(BindError::Source)?;
    let provenance = std::fs::read_to_string(provenance_path)
        .map_err(|error| BindError::Io(format!("could not read binding provenance: {error}")))?;
    Ok(BindResult {
        source: ForeignBridge::render_scalar_jet(&abi, descriptor, &functions)
            .map_err(BindError::Source)?,
        bound: functions
            .into_iter()
            .map(|function| function.name)
            .collect(),
        archive,
        worker,
        provenance,
    })
}

fn check_node(path: &Path) -> Result<(), BindError> {
    let output = Command::new("node")
        .arg("--check")
        .arg(path)
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                BindError::ToolMissing("node")
            } else {
                BindError::Io(format!("could not start Node: {error}"))
            }
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(BindError::ToolFailed(
            "node",
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

fn parse(source: &str) -> Result<Vec<ScalarBridgeFunction>, BindError> {
    let mut functions = Vec::new();
    for (line_number, line) in source.lines().enumerate() {
        let line = line.trim();
        let signature = line
            .strip_prefix("export declare function ")
            .or_else(|| line.strip_prefix("export function "))
            .or_else(|| line.strip_prefix("declare function "));
        let Some(signature) = signature else {
            continue;
        };
        let open = signature.find('(').ok_or_else(|| {
            BindError::Source(format!(
                "line {}: declaration has no parameter list",
                line_number + 1
            ))
        })?;
        let name = signature[..open].trim();
        if !is_ident(name) || name == "take_error" {
            return Err(BindError::Source(format!(
                "line {}: {name} is not a bindable JavaScript function name",
                line_number + 1
            )));
        }
        let close = signature[open + 1..]
            .find(')')
            .map(|offset| open + 1 + offset)
            .ok_or_else(|| {
                BindError::Source(format!(
                    "line {}: declaration {name} has no closed parameter list",
                    line_number + 1
                ))
            })?;
        let params = parse_params(&signature[open + 1..close], line_number + 1)?;
        let tail = signature[close + 1..].trim();
        let return_type = tail
            .strip_prefix(':')
            .and_then(|value| value.strip_suffix(';'))
            .map(str::trim)
            .ok_or_else(|| {
                BindError::Source(format!(
                    "line {}: declaration {name} needs a scalar return type",
                    line_number + 1
                ))
            })?;
        let result = scalar(return_type).ok_or_else(|| {
            BindError::Source(format!(
                "line {}: JavaScript type {return_type} is outside the typed scalar ABI",
                line_number + 1
            ))
        })?;
        if functions
            .iter()
            .any(|function: &ScalarBridgeFunction| function.name == name)
        {
            return Err(BindError::Source(format!(
                "line {}: declaration {name} is repeated",
                line_number + 1
            )));
        }
        functions.push(ScalarBridgeFunction {
            name: name.to_string(),
            params,
            result,
        });
    }
    if functions.is_empty() {
        return Err(BindError::Source(
            "the TypeScript declarations have no exported typed scalar functions".into(),
        ));
    }
    Ok(functions)
}

fn parse_params(source: &str, line: usize) -> Result<Vec<ForeignScalar>, BindError> {
    if source.trim().is_empty() {
        return Ok(Vec::new());
    }
    source
        .split(',')
        .map(|param| {
            let param = param.trim();
            if param.contains('?') || param.contains('=') || param.starts_with("...") {
                return Err(BindError::Source(format!(
                    "line {line}: optional, default, and rest JavaScript parameters are not in the typed ABI"
                )));
            }
            let (name, annotation) = param.split_once(':').ok_or_else(|| {
                BindError::Source(format!(
                    "line {line}: JavaScript parameter {param} needs a scalar type"
                ))
            })?;
            if !is_ident(name.trim()) {
                return Err(BindError::Source(format!(
                    "line {line}: {} is not a bindable JavaScript parameter",
                    name.trim()
                )));
            }
            scalar(annotation.trim()).ok_or_else(|| {
                BindError::Source(format!(
                    "line {line}: JavaScript type {} is outside the typed scalar ABI",
                    annotation.trim()
                ))
            })
        })
        .collect()
}

fn scalar(name: &str) -> Option<ForeignScalar> {
    match name {
        "number" => Some(ForeignScalar::Float),
        "bigint" => Some(ForeignScalar::Int),
        "boolean" => Some(ForeignScalar::Bool),
        _ => None,
    }
}

fn render_worker(functions: &[ScalarBridgeFunction]) -> String {
    let mut out = String::from("import { pathToFileURL } from \"node:url\";\n\nconst KINDS = {\n");
    for function in functions {
        out.push_str("  ");
        out.push_str(&format!("{:?}: [", function.name));
        for (index, scalar) in function.params.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            out.push_str(&format!("{:?}", javascript_scalar(*scalar)));
        }
        out.push_str("],\n");
    }
    out.push_str("};\nconst RESULTS = {\n");
    for function in functions {
        out.push_str(&format!(
            "  {:?}: {:?},\n",
            function.name,
            javascript_scalar(function.result)
        ));
    }
    out.push_str(
        r#"};

function convert(kind, value) {
  if (kind === "int") return BigInt(value);
  if (kind === "float") {
    const number = Number(value);
    if (!Number.isFinite(number)) throw new Error("invalid number");
    return number;
  }
  if (kind === "bool") {
    if (value !== "true" && value !== "false") throw new Error("invalid bool");
    return value === "true";
  }
  throw new Error("invalid scalar");
}

function emit(value, kind) {
  if (kind === "int" && typeof value === "bigint") console.log("OK " + value.toString());
  else if (kind === "float" && typeof value === "number" && Number.isFinite(value)) console.log("OK " + String(value));
  else if (kind === "bool" && typeof value === "boolean") console.log("OK " + (value ? "true" : "false"));
  else throw new Error("foreign result does not match its declared type");
}

async function main() {
  const modulePath = process.argv[2];
  const name = process.argv[3];
  const oldLog = console.log;
  console.log = (...values) => process.stderr.write(values.join(" ") + "\n");
  let result;
  try {
    const imported = await import(pathToFileURL(modulePath).href + "?jet=1");
    const functionValue = imported[name] ?? (imported.default && imported.default[name]);
    if (typeof functionValue !== "function") throw new Error("missing export");
    const args = (KINDS[name] ?? []).map((kind, index) => convert(kind, process.argv[index + 4]));
    if (args.length !== (process.argv.length - 4)) throw new Error("wrong argument count");
    result = functionValue(...args);
  } finally {
    console.log = oldLog;
  }
  if (result && typeof result.then === "function") throw new Error("async result is not in the scalar ABI");
  emit(result, RESULTS[name]);
}

main().catch(() => process.exitCode = 2);
"#,
    );
    out
}

fn javascript_scalar(scalar: ForeignScalar) -> &'static str {
    match scalar {
        ForeignScalar::Int => "int",
        ForeignScalar::Float => "float",
        ForeignScalar::Bool => "bool",
        _ => "unsupported",
    }
}

fn is_ident(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(first) if first == '_' || first.is_ascii_alphabetic())
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_rejects_dynamic_javascript_shapes() {
        let error = parse("export function add(value?: number): number;\n").unwrap_err();
        assert!(error.to_string().contains("optional"));
    }

    #[test]
    fn renderer_uses_one_effect_row_shape() {
        let functions =
            parse("export function add(left: bigint, right: bigint): bigint;\n").unwrap();
        let descriptor = *binder_descriptor(ForeignLanguage::JS).unwrap();
        let source =
            ForeignBridge::render_scalar_jet("jet_js_math", descriptor, &functions).unwrap();
        assert!(source.contains("Int String! -[FFI]>"));
        assert!(!source.contains("=>"));
    }
}
