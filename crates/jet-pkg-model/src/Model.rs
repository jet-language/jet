//! Compiler/package projection for the runtime-owned model contract.
//!
//! The executable model kernel lives in `jet-rt::model`. This module owns only
//! the package-manifest projection (`PackageFacts`/`OutputPayload`) and the
//! authority-bound artifact verification used while loading a package. Keeping
//! this adapter one-way prevents generated applications from linking the
//! compiler/package crate.

pub use jet_rt::model::*;

use crate::Authority::AuthorityResolver;
use crate::Package::{OutputFact, OutputPayload, PackageFacts, PackageOutputKind};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Compiler-side constructors for the runtime-owned [`ModelPackage`].
///
/// This is a local extension trait because the package parser cannot add
/// inherent methods to the runtime crate's type. Importing this trait preserves
/// the established `ModelPackage::from_facts` spelling for compiler callers.
pub trait ModelPackageCompiler {
    fn descriptor_from_source(
        source: &str,
        origin: impl Into<String>,
        output_name: &str,
    ) -> Result<ModelDescriptor, ModelError>;

    fn from_facts(facts: &PackageFacts, output_name: &str) -> Result<ModelPackage, ModelError>;

    fn load(root: &Path, output_name: &str) -> Result<ModelPackage, ModelError>;
}

impl ModelPackageCompiler for ModelPackage {
    fn descriptor_from_source(
        source: &str,
        origin: impl Into<String>,
        output_name: &str,
    ) -> Result<ModelDescriptor, ModelError> {
        let origin = origin.into();
        let facts = PackageFacts::parse(source, origin.clone())
            .map_err(|error| ModelError::load(&origin, error.to_string()))?;
        Self::from_facts(&facts, output_name)?.descriptor()
    }

    fn from_facts(facts: &PackageFacts, output_name: &str) -> Result<ModelPackage, ModelError> {
        let output = facts.outputs.get(output_name).ok_or_else(|| ModelError::MissingOutput {
            package: facts.name.clone(),
            output: output_name.to_string(),
        })?;
        if output.kind != PackageOutputKind::Model {
            return Err(ModelError::WrongKind {
                package: facts.name.clone(),
                output: output_name.to_string(),
            });
        }
        from_output(facts, output_name, output)
    }

    fn load(root: &Path, output_name: &str) -> Result<ModelPackage, ModelError> {
        let facts = PackageFacts::load_checked(root)
            .map_err(|error| ModelError::load("<package>", error.to_string()))?
            .ok_or_else(|| ModelError::MissingPackage {
                root: root.display().to_string(),
            })?;
        let package = Self::from_facts(&facts, output_name)?;
        verify_artifacts(&package, root)?;
        Ok(package)
    }
}

fn from_output(
    facts: &PackageFacts,
    output_name: &str,
    output: &OutputFact,
) -> Result<ModelPackage, ModelError> {
    let version = required_package_field(&facts.version, &facts.name, "version")?;
    let license = required_package_field(&facts.license, &facts.name, "license")?;
    let fields = object_payload(&facts.name, output_name, &output.payload)?;
    validate_model_fields(fields, &facts.name, output_name)?;

    let signature_name = optional_string(fields, &facts.name, output_name, "name")?;
    let graph = artifact(fields, &facts.name, output_name, "graph")?;
    let weights = artifact(fields, &facts.name, output_name, "weights")?;
    let tokenizer = artifact(fields, &facts.name, output_name, "tokenizer")?;
    let adapter = optional_artifact(fields, &facts.name, output_name, "adapter")?;
    let inputs = tensor_list(fields, &facts.name, output_name, "inputs")?;
    let outputs = tensor_list(fields, &facts.name, output_name, "outputs")?;
    let provider = required_string(fields, &facts.name, output_name, "provider")?;
    let preprocessing = required_string(fields, &facts.name, output_name, "preprocessing")?;
    let pooling = required_string(fields, &facts.name, output_name, "pooling")?;
    let normalization = required_string(fields, &facts.name, output_name, "normalization")?;
    let output_meaning = required_string(fields, &facts.name, output_name, "output_meaning")?;
    let metric = required_string(fields, &facts.name, output_name, "metric")?;
    let custom_operators =
        optional_bool(fields, &facts.name, output_name, "custom_operators")?.unwrap_or(false);
    let max_context = optional_u64(fields, &facts.name, output_name, "max_context")?;
    let max_batch = optional_u64(fields, &facts.name, output_name, "max_batch")?;
    let max_buffer_bytes = optional_u64(fields, &facts.name, output_name, "max_buffer_bytes")?;

    ModelPackage::from_descriptor(ModelDescriptor {
        package: facts.name.clone(),
        output: output_name.to_string(),
        signature_name,
        package_version: version,
        license,
        graph,
        weights,
        tokenizer,
        adapter,
        preprocessing,
        pooling,
        normalization,
        output_meaning,
        metric,
        contract: ModelContract {
            inputs,
            outputs,
            provider,
            custom_operators,
            max_context,
            max_batch,
            max_buffer_bytes,
        },
    })
}

fn verify_artifacts(package: &ModelPackage, root: &Path) -> Result<(), ModelError> {
    let mut checked_paths = BTreeSet::new();
    for artifact in &package.artifacts {
        if !checked_paths.insert(artifact.path.as_str()) {
            continue;
        }
        let resolver = AuthorityResolver::open(root)
            .map_err(|error| ModelError::artifact(&package.package, &artifact.path, error.to_string()))?;
        let checked = resolver
            .checked_file(Path::new(&artifact.path))
            .map_err(|error| ModelError::artifact(&package.package, &artifact.path, error.to_string()))?;
        let actual = jet_rt::SHA256::sha256_hex(&checked.bytes);
        if actual != artifact.sha256 {
            return Err(ModelError::provenance(
                &package.package,
                format!(
                    "artifact `{}` hash is `{actual}`, expected `{}`",
                    artifact.path, artifact.sha256
                ),
            ));
        }
    }
    Ok(())
}

fn validate_model_fields(
    fields: &BTreeMap<String, OutputPayload>,
    package: &str,
    output: &str,
) -> Result<(), ModelError> {
    const ALLOWED: &[&str] = &[
        "name",
        "graph",
        "graph_sha256",
        "weights",
        "weights_sha256",
        "tokenizer",
        "tokenizer_sha256",
        "adapter",
        "adapter_sha256",
        "inputs",
        "outputs",
        "provider",
        "preprocessing",
        "pooling",
        "normalization",
        "output_meaning",
        "metric",
        "custom_operators",
        "max_context",
        "max_batch",
        "max_buffer_bytes",
    ];
    for field in fields.keys() {
        if !ALLOWED.contains(&field.as_str()) {
            return Err(ModelError::declaration(
                package,
                format!("output `{output}` has unknown model field `{field}`"),
            ));
        }
    }
    Ok(())
}

fn object_payload<'a>(
    package: &str,
    output: &str,
    value: &'a OutputPayload,
) -> Result<&'a BTreeMap<String, OutputPayload>, ModelError> {
    match value {
        OutputPayload::Object(fields) => Ok(fields),
        _ => Err(ModelError::declaration(
            package,
            format!("output `{output}` must carry an object payload"),
        )),
    }
}

fn artifact(
    fields: &BTreeMap<String, OutputPayload>,
    package: &str,
    output: &str,
    role: &str,
) -> Result<ModelArtifact, ModelError> {
    let path = required_string(fields, package, output, role)?;
    let hash = required_string(fields, package, output, &format!("{role}_sha256"))?;
    Ok(ModelArtifact { path, sha256: hash })
}

fn optional_artifact(
    fields: &BTreeMap<String, OutputPayload>,
    package: &str,
    output: &str,
    role: &str,
) -> Result<Option<ModelArtifact>, ModelError> {
    let path = fields.get(role);
    let hash = fields.get(&format!("{role}_sha256"));
    match (path, hash) {
        (None, None) => Ok(None),
        (Some(path), Some(hash)) => Ok(Some(ModelArtifact {
            path: payload_string(path, package, output, role)?,
            sha256: payload_string(hash, package, output, &format!("{role}_sha256"))?,
        })),
        _ => Err(ModelError::provenance(
            package,
            format!("optional `{role}` and `{role}_sha256` must be declared together"),
        )),
    }
}

fn tensor_list(
    fields: &BTreeMap<String, OutputPayload>,
    package: &str,
    output: &str,
    field: &str,
) -> Result<Vec<TensorSpec>, ModelError> {
    let value = fields.get(field).ok_or_else(|| {
        ModelError::declaration(package, format!("output `{output}` is missing `{field}`"))
    })?;
    let OutputPayload::Array(values) = value else {
        return Err(ModelError::declaration(
            package,
            format!("output `{output}` field `{field}` must be an array"),
        ));
    };
    if values.is_empty() {
        return Err(ModelError::declaration(
            package,
            format!("output `{output}` field `{field}` must not be empty"),
        ));
    }
    values
        .iter()
        .map(|value| tensor_spec(value, package, output, field))
        .collect()
}

fn tensor_spec(
    value: &OutputPayload,
    package: &str,
    output: &str,
    field: &str,
) -> Result<TensorSpec, ModelError> {
    let OutputPayload::Object(fields) = value else {
        return Err(ModelError::declaration(
            package,
            format!("output `{output}` field `{field}` tensor must be an object"),
        ));
    };
    for name in fields.keys() {
        if !matches!(name.as_str(), "name" | "dtype" | "shape") {
            return Err(ModelError::declaration(
                package,
                format!("output `{output}` field `{field}` has unknown tensor field `{name}`"),
            ));
        }
    }
    let name = required_string(fields, package, output, "name")?;
    let dtype_text = required_string(fields, package, output, "dtype")?;
    let dtype = TensorDType::parse(&dtype_text).ok_or_else(|| {
        ModelError::declaration(
            package,
            format!("tensor `{name}` has unsupported dtype `{dtype_text}`"),
        )
    })?;
    let shape = fields.get("shape").ok_or_else(|| {
        ModelError::declaration(
            package,
            format!("tensor `{name}` is missing a shape"),
        )
    })?;
    Ok(TensorSpec {
        name,
        dtype,
        shape: tensor_shape(shape, package, output, field)?,
    })
}

fn tensor_shape(
    value: &OutputPayload,
    package: &str,
    output: &str,
    field: &str,
) -> Result<TensorShape, ModelError> {
    let OutputPayload::Array(values) = value else {
        return Err(ModelError::declaration(
            package,
            format!("output `{output}` field `{field}` shape must be an array"),
        ));
    };
    if values.is_empty() || values.len() > 64 {
        return Err(ModelError::declaration(
            package,
            format!("output `{output}` field `{field}` has invalid rank"),
        ));
    }
    let dimensions = values
        .iter()
        .map(|value| tensor_dimension(value, package, output, field))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TensorShape { dimensions })
}

fn tensor_dimension(
    value: &OutputPayload,
    package: &str,
    output: &str,
    field: &str,
) -> Result<TensorDimension, ModelError> {
    match value {
        OutputPayload::Number(number) => number.parse::<u64>().ok().filter(|value| *value > 0).map(TensorDimension::Static).ok_or_else(|| {
            ModelError::declaration(package, format!("output `{output}` field `{field}` has an invalid static dimension"))
        }),
        OutputPayload::Object(fields) => {
            for name in fields.keys() {
                if !matches!(name.as_str(), "name" | "min" | "max") {
                    return Err(ModelError::declaration(
                        package,
                        format!("output `{output}` field `{field}` has unknown dimension field `{name}`"),
                    ));
                }
            }
            let name = required_string(fields, package, output, "name")?;
            let min = payload_u64(fields.get("min"), package, output, "min")?;
            let max = payload_u64(fields.get("max"), package, output, "max")?;
            if min == 0 || min > max {
                return Err(ModelError::declaration(
                    package,
                    format!("output `{output}` field `{field}` has invalid dynamic bounds"),
                ));
            }
            Ok(TensorDimension::Dynamic { name, min, max })
        }
        _ => Err(ModelError::declaration(
            package,
            format!("output `{output}` field `{field}` must be a number or bounded dimension"),
        )),
    }
}

fn required_package_field(
    value: &Option<String>,
    package: &str,
    field: &str,
) -> Result<String, ModelError> {
    let value = value.as_deref().map(str::trim).filter(|value| !value.is_empty()).ok_or_else(|| {
        ModelError::provenance(package, format!("package `{field}` must be pinned for model outputs"))
    })?;
    if value.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(ModelError::provenance(
            package,
            format!("package `{field}` contains a control character"),
        ));
    }
    Ok(value.to_string())
}

fn required_string(
    fields: &BTreeMap<String, OutputPayload>,
    package: &str,
    output: &str,
    field: &str,
) -> Result<String, ModelError> {
    let value = fields.get(field).ok_or_else(|| {
        ModelError::declaration(package, format!("output `{output}` is missing `{field}`"))
    })?;
    payload_string(value, package, output, field)
}

fn optional_string(
    fields: &BTreeMap<String, OutputPayload>,
    package: &str,
    output: &str,
    field: &str,
) -> Result<Option<String>, ModelError> {
    match fields.get(field) {
        None => Ok(None),
        Some(value) => Ok(Some(payload_string(value, package, output, field)?)),
    }
}

fn payload_string(
    value: &OutputPayload,
    package: &str,
    output: &str,
    field: &str,
) -> Result<String, ModelError> {
    match value {
        OutputPayload::String(value) if !value.trim().is_empty() => {
            if value.bytes().any(|byte| byte.is_ascii_control()) {
                return Err(ModelError::declaration(
                    package,
                    format!("output `{output}` field `{field}` contains a control character"),
                ));
            }
            Ok(value.clone())
        }
        _ => Err(ModelError::declaration(
            package,
            format!("output `{output}` field `{field}` must be a non-empty string"),
        )),
    }
}

fn optional_bool(
    fields: &BTreeMap<String, OutputPayload>,
    package: &str,
    output: &str,
    field: &str,
) -> Result<Option<bool>, ModelError> {
    match fields.get(field) {
        None => Ok(None),
        Some(OutputPayload::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(ModelError::declaration(
            package,
            format!("output `{output}` field `{field}` must be a boolean"),
        )),
    }
}

fn optional_u64(
    fields: &BTreeMap<String, OutputPayload>,
    package: &str,
    output: &str,
    field: &str,
) -> Result<Option<u64>, ModelError> {
    match fields.get(field) {
        None => Ok(None),
        Some(value) => Ok(Some(payload_u64(Some(value), package, output, field)?)),
    }
}

fn payload_u64(
    value: Option<&OutputPayload>,
    package: &str,
    output: &str,
    field: &str,
) -> Result<u64, ModelError> {
    let Some(OutputPayload::Number(value)) = value else {
        return Err(ModelError::declaration(
            package,
            format!("output `{output}` field `{field}` must be a non-negative integer"),
        ));
    };
    value.parse::<u64>().map_err(|_| {
        ModelError::declaration(
            package,
            format!("output `{output}` field `{field}` must be a non-negative integer"),
        )
    })
}

