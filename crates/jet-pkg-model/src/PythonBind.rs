//! Typed Python sidecar binder (D-FFI-PY1 / D-FFI-UNIFY1).
//!
//! The binder accepts only top-level scalar annotations. It emits one
//! supervised worker and the same checked C/Jet boundary used by the other
//! scalar adapters. Unsupported annotations fail before an artifact exists.

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
                write!(f, "{tool} rejected the Python binding: {detail}")
            }
        }
    }
}

pub fn bind(
    source_path: &Path,
    source: &str,
    lib: &str,
    cache: &Path,
) -> Result<BindResult, BindError> {
    if !is_ident(lib) {
        return Err(BindError::Source(format!(
            "{lib} is not a valid Jet library name"
        )));
    }
    let functions = parse(source)?;
    let source_path = source_path
        .canonicalize()
        .map_err(|error| BindError::Io(format!("could not resolve Python source: {error}")))?;
    check_python(&source_path)?;
    std::fs::create_dir_all(cache).map_err(|error| {
        BindError::Io(format!("could not create Python binding cache: {error}"))
    })?;
    let worker = cache.join(format!("{lib}_worker.py"));
    std::fs::write(&worker, render_worker(&functions))
        .map_err(|error| BindError::Io(format!("could not write Python worker: {error}")))?;
    let worker = worker
        .canonicalize()
        .map_err(|error| BindError::Io(format!("could not resolve Python worker: {error}")))?;
    let abi = format!("jet_py_{lib}");
    let archive = ForeignBridge::compile_scalar_sidecar(
        cache,
        &abi,
        "python3",
        &worker,
        &source_path,
        &functions,
    )
    .map_err(BindError::Source)?;
    let provenance_path = ForeignBridge::write_scalar_provenance(
        cache,
        lib,
        ForeignLanguage::Py,
        "python3",
        &source_path,
        &worker,
        &archive,
    )
    .map_err(BindError::Source)?;
    let provenance = std::fs::read_to_string(provenance_path)
        .map_err(|error| BindError::Io(format!("could not read binding provenance: {error}")))?;
    let stamp = binder_descriptor(ForeignLanguage::Py)
        .ok_or_else(|| BindError::Source("Python binder descriptor is not registered".into()))?
        .stamp();
    Ok(BindResult {
        source: ForeignBridge::render_scalar_jet(&abi, "FFI.Py", &stamp, &functions),
        bound: functions
            .into_iter()
            .map(|function| function.name)
            .collect(),
        archive,
        worker,
        provenance,
    })
}

fn check_python(path: &Path) -> Result<(), BindError> {
    let output = Command::new("python3")
        .args([
            "-c",
            "import pathlib,sys; compile(pathlib.Path(sys.argv[1]).read_text(), sys.argv[1], 'exec')",
        ])
        .arg(path)
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                BindError::ToolMissing("python3")
            } else {
                BindError::Io(format!("could not start Python: {error}"))
            }
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(BindError::ToolFailed(
            "python3",
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

fn parse(source: &str) -> Result<Vec<ScalarBridgeFunction>, BindError> {
    let mut functions = Vec::new();
    for (line_number, line) in source.lines().enumerate() {
        let line = line.trim_end();
        if line.starts_with("async def ") {
            return Err(BindError::Source(format!(
                "line {}: async Python functions need an async FFI adapter",
                line_number + 1
            )));
        }
        let Some(signature) = line.strip_prefix("def ") else {
            continue;
        };
        let open = signature.find('(').ok_or_else(|| {
            BindError::Source(format!(
                "line {}: Python function has no parameter list",
                line_number + 1
            ))
        })?;
        let name = signature[..open].trim();
        if !is_ident(name) || name == "take_error" {
            return Err(BindError::Source(format!(
                "line {}: {name} is not a bindable Python function name",
                line_number + 1
            )));
        }
        let close = signature[open + 1..]
            .find(')')
            .map(|offset| open + 1 + offset)
            .ok_or_else(|| {
                BindError::Source(format!(
                    "line {}: Python function {name} has no closed parameter list",
                    line_number + 1
                ))
            })?;
        let params = parse_params(&signature[open + 1..close], line_number + 1)?;
        let tail = signature[close + 1..].trim();
        let return_type = tail
            .strip_prefix("->")
            .and_then(|value| value.strip_suffix(':'))
            .map(str::trim)
            .ok_or_else(|| {
                BindError::Source(format!(
                    "line {}: Python function {name} needs a scalar return annotation",
                    line_number + 1
                ))
            })?;
        let result = scalar(return_type).ok_or_else(|| {
            BindError::Source(format!(
                "line {}: Python type {return_type} is outside the typed scalar ABI",
                line_number + 1
            ))
        })?;
        if functions
            .iter()
            .any(|function: &ScalarBridgeFunction| function.name == name)
        {
            return Err(BindError::Source(format!(
                "line {}: Python function {name} is declared more than once",
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
            "the Python source has no top-level typed scalar functions".into(),
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
            if param.contains('=') || param.starts_with('*') {
                return Err(BindError::Source(format!(
                    "line {line}: default and variadic Python parameters are not in the typed ABI"
                )));
            }
            let (name, annotation) = param.split_once(':').ok_or_else(|| {
                BindError::Source(format!(
                    "line {line}: Python parameter {param} needs a scalar annotation"
                ))
            })?;
            if !is_ident(name.trim()) {
                return Err(BindError::Source(format!(
                    "line {line}: {} is not a bindable Python parameter",
                    name.trim()
                )));
            }
            scalar(annotation.trim()).ok_or_else(|| {
                BindError::Source(format!(
                    "line {line}: Python type {} is outside the typed scalar ABI",
                    annotation.trim()
                ))
            })
        })
        .collect()
}

fn scalar(name: &str) -> Option<ForeignScalar> {
    match name {
        "int" => Some(ForeignScalar::Int),
        "float" => Some(ForeignScalar::Float),
        "bool" => Some(ForeignScalar::Bool),
        _ => None,
    }
}

fn render_worker(functions: &[ScalarBridgeFunction]) -> String {
    let mut out = String::from(
        "import contextlib\nimport importlib.util\nimport math\nimport sys\n\nKINDS = {\n",
    );
    for function in functions {
        out.push_str("    ");
        out.push_str(&format!("{:?}: [", function.name));
        for (index, scalar) in function.params.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            out.push_str(&format!("{:?}", python_scalar(*scalar)));
        }
        out.push_str("],\n");
    }
    out.push_str("}\nRESULTS = {\n");
    for function in functions {
        out.push_str(&format!(
            "    {:?}: {:?},\n",
            function.name,
            python_scalar(function.result)
        ));
    }
    out.push_str(
        r#"}

def convert(kind, value):
    if kind == "int":
        return int(value)
    if kind == "float":
        return float(value)
    if kind == "bool":
        if value not in ("true", "false"):
            raise ValueError("invalid bool")
        return value == "true"
    raise ValueError("invalid scalar")

def emit(value, kind):
    if kind == "int" and type(value) is int:
        print("OK " + str(value))
    elif kind == "float" and type(value) is float and math.isfinite(value):
        print("OK " + repr(value))
    elif kind == "bool" and type(value) is bool:
        print("OK " + ("true" if value else "false"))
    else:
        raise ValueError("foreign result does not match its declared type")

def main():
    path, name = sys.argv[1], sys.argv[2]
    spec = importlib.util.spec_from_file_location("jet_foreign_module", path)
    if spec is None or spec.loader is None:
        raise ValueError("could not load Python module")
    module = importlib.util.module_from_spec(spec)
    with contextlib.redirect_stdout(sys.stderr):
        spec.loader.exec_module(module)
        function = getattr(module, name)
        raw_args = sys.argv[3:]
        if len(raw_args) != len(KINDS[name]):
            raise ValueError("wrong argument count")
        args = [convert(kind, value) for kind, value in zip(KINDS[name], raw_args)]
        result = function(*args)
    emit(result, RESULTS[name])

try:
    main()
except BaseException:
    raise SystemExit(2)
"#,
    );
    out
}

fn python_scalar(scalar: ForeignScalar) -> &'static str {
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
    fn parser_rejects_untyped_python_boundary() {
        let error = parse("def add(left, right) -> int:\n    return left + right\n").unwrap_err();
        assert!(error.to_string().contains("needs a scalar annotation"));
    }

    #[test]
    fn renderer_uses_current_effect_arrow() {
        let functions =
            parse("def add(left: int, right: int) -> int:\n    return left + right\n").unwrap();
        let stamp = binder_descriptor(ForeignLanguage::Py).unwrap().stamp();
        let source = ForeignBridge::render_scalar_jet("jet_py_math", "FFI.Py", &stamp, &functions);
        assert!(source.contains("Int String! -[FFI.Py]>"));
        assert!(!source.contains("=>"));
    }
}
