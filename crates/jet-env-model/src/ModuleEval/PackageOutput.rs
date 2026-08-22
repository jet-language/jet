//! Project the canonical Package output facts into the shared JetOS plan.
//!
//! Package facts are the only authority for this path. This module does not
//! resolve packages, read the store, or perform rollout work. It only lowers
//! checked System and Fleet payloads into the plan types already used by JetOS.

use std::collections::BTreeMap;
use std::fmt;

use crate::Comptime::CtValue;
use crate::Merge;
use crate::Package::{OutputFact, OutputPayload, PackageFacts, PackageOutputKind};
use crate::RefSpec::SourceTable;
use crate::AST::CtKey;

use super::Environment::{EnvironmentLifecycle, IntegrationFactProjection, LanguageExpansion};
use super::Types::{
    EnvPlan, FleetPlan, HostOverride, HostOverrideProvenance, HostOverrideValue, HostPlan,
    OptionPlan, PromptPathMode, PromptStripMode, ServicePlan, SystemPlan,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageOutputError {
    path: String,
    message: String,
}

impl PackageOutputError {
    fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for PackageOutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

impl std::error::Error for PackageOutputError {}

/// The immutable Package graph facts needed by the JetOS plan path.
#[derive(Debug, Clone, PartialEq)]
pub struct PackageOutputPlan {
    pub package_name: String,
    pub graph_identity: String,
    pub systems: Vec<SystemPlan>,
    pub fleets: Vec<FleetPlan>,
}

/// Lower all JetOS outputs in one Package. BTree-backed Package facts make the
/// result stable across repeated reads; arrays keep the author's package order.
pub fn project_package_outputs(
    facts: &PackageFacts,
) -> Result<PackageOutputPlan, PackageOutputError> {
    let mut systems = Vec::new();
    let mut system_names = BTreeMap::new();
    for (key, output) in &facts.outputs {
        if output.kind != PackageOutputKind::System {
            continue;
        }
        let path = format!("outputs.{key}");
        let system = project_system(&path, output)?;
        if system_names
            .insert(system.name.clone(), key.clone())
            .is_some()
        {
            return Err(PackageOutputError::new(
                path,
                format!("system {} is declared more than once", system.name),
            ));
        }
        systems.push(system);
    }

    let mut fleets = Vec::new();
    for (key, output) in &facts.outputs {
        if output.kind != PackageOutputKind::Fleet {
            continue;
        }
        let path = format!("outputs.{key}");
        fleets.push(project_fleet(&path, output, &system_names)?);
    }

    Ok(PackageOutputPlan {
        package_name: facts.name.clone(),
        graph_identity: facts.semantic_digest(),
        systems,
        fleets,
    })
}

/// Build an EnvPlan without asking the legacy module evaluator to parse a
/// second graph. JetOS still owns realization; this value only carries the
/// Package projection and its identity into that path.
pub fn env_plan_from_package_outputs(
    source_file: impl Into<String>,
    projection: PackageOutputPlan,
) -> EnvPlan {
    EnvPlan {
        table: SourceTable::empty(),
        source_files: vec![source_file.into()],
        graph_identity: Some(projection.graph_identity),
        environment_reads: Vec::new(),
        package_refs: Vec::new(),
        adapters: Vec::new(),
        prompt: None,
        prompt_path: PromptPathMode::default(),
        prompt_strip: PromptStripMode::default(),
        systems: projection.systems,
        images: Vec::new(),
        fleets: projection.fleets,
        vmtests: Vec::new(),
        dev_services: Vec::new(),
        secrets: Vec::new(),
        lifecycle: EnvironmentLifecycle::default(),
        presets: Vec::new(),
        languages: Vec::new(),
        selected_preset: None,
        language_expansion: LanguageExpansion::default(),
        language_packs: Vec::new(),
        language_projections: Vec::new(),
        files: Vec::new(),
        integrations: Vec::new(),
        integration_facts: IntegrationFactProjection::default(),
        package_profiles: Vec::new(),
        environment_names: Vec::new(),
        active_environment: None,
        active_environment_provenance: Vec::new(),
    }
}

fn project_system(path: &str, output: &OutputFact) -> Result<SystemPlan, PackageOutputError> {
    let fields = output_object(path, output)?;
    let name = match fields.get(crate::Syntax::OUTPUT_FIELD_NAME) {
        Some(value) => text_field(Some(value), &format!("{path}.name"))?,
        None => output.name.clone(),
    };
    if name.trim().is_empty() {
        return Err(PackageOutputError::new(
            format!("{path}.name"),
            "must not be empty",
        ));
    }
    let target_path = format!("{path}.target");
    let target = text_field(fields.get(crate::Syntax::SYSTEM_FIELD_TARGET), &target_path)?;
    if !matches!(target.as_str(), "linux.x64" | "linux.arm64") {
        return Err(PackageOutputError::new(
            target_path,
            format!("unknown target {target}; use linux.x64 or linux.arm64"),
        ));
    }
    Ok(SystemPlan {
        name,
        target,
        packages: project_packages(
            fields.get(crate::Syntax::SYSTEM_FIELD_PACKAGES),
            &format!("{path}.packages"),
        )?,
        services: project_services(
            fields.get(crate::Syntax::SYSTEM_FIELD_SERVICES),
            &format!("{path}.services"),
        )?,
        options: project_options(
            fields.get(crate::Syntax::SYSTEM_FIELD_OPTIONS),
            &format!("{path}.options"),
        )?,
    })
}

fn project_fleet(
    path: &str,
    output: &OutputFact,
    systems: &BTreeMap<String, String>,
) -> Result<FleetPlan, PackageOutputError> {
    let fields = output_object(path, output)?;
    let name = match fields.get(crate::Syntax::OUTPUT_FIELD_NAME) {
        Some(value) => text_field(Some(value), &format!("{path}.name"))?,
        None => output.name.clone(),
    };
    if name.trim().is_empty() {
        return Err(PackageOutputError::new(
            format!("{path}.name"),
            "must not be empty",
        ));
    }
    let hosts_path = format!("{path}.hosts");
    let hosts = match fields.get(crate::Syntax::FLEET_FIELD_HOSTS) {
        Some(OutputPayload::Object(hosts)) => hosts,
        Some(_) => {
            return Err(PackageOutputError::new(
                hosts_path,
                "must be an object of host names to systems",
            ))
        }
        None => return Err(PackageOutputError::new(hosts_path, "is required")),
    };
    let mut projected = Vec::new();
    for (host_name, value) in hosts {
        let host_path = format!("{path}.hosts.{host_name}");
        let (system, overrides, override_source) = project_host(&host_path, value)?;
        if !systems.contains_key(&system) {
            return Err(PackageOutputError::new(
                host_path,
                format!("unknown system {system}"),
            ));
        }
        projected.push(HostPlan {
            name: host_name.clone(),
            system,
            overrides,
            override_source,
        });
    }
    Ok(FleetPlan {
        name,
        hosts: projected,
    })
}

fn project_host(
    path: &str,
    value: &OutputPayload,
) -> Result<(String, Option<HostOverride>, Option<String>), PackageOutputError> {
    let OutputPayload::Object(fields) = value else {
        let system = text_field(Some(value), path)?;
        return Ok((normalize_system_name(&system), None, None));
    };
    let system = fields
        .get("system")
        .or_else(|| fields.get(crate::Syntax::OUTPUT_FIELD_NAME));
    let system = normalize_system_name(&text_field(system, &format!("{path}.system"))?);
    let override_fields = fields
        .iter()
        .filter(|(field, _)| field.as_str() != "system" && field.as_str() != "name")
        .collect::<Vec<_>>();
    if override_fields.is_empty() {
        return Ok((system, None, None));
    }
    let mut fields_out = Vec::new();
    let mut provenance = Vec::new();
    for (field, value) in override_fields {
        let field_path = format!("{path}.{field}");
        let projected = match field.as_str() {
            crate::Syntax::SYSTEM_FIELD_TARGET => {
                let target = text_field(Some(value), &field_path)?;
                if !matches!(target.as_str(), "linux.x64" | "linux.arm64") {
                    return Err(PackageOutputError::new(
                        field_path,
                        format!("unknown target {target}; use linux.x64 or linux.arm64"),
                    ));
                }
                HostOverrideValue::Platform(target)
            }
            crate::Syntax::SYSTEM_FIELD_PACKAGES => {
                HostOverrideValue::Packages(project_packages(Some(value), &field_path)?)
            }
            crate::Syntax::SYSTEM_FIELD_SERVICES => {
                HostOverrideValue::Services(project_services(Some(value), &field_path)?)
            }
            crate::Syntax::SYSTEM_FIELD_OPTIONS => {
                HostOverrideValue::Options(project_options(Some(value), &field_path)?)
            }
            _ => HostOverrideValue::Value(payload_to_ct_value(value)),
        };
        let source = payload_display(value);
        fields_out.push((field.clone(), projected));
        provenance.push(HostOverrideProvenance {
            field: field.clone(),
            dependencies: Vec::new(),
            pure: true,
            source,
        });
    }
    let source = payload_display(value);
    Ok((
        system,
        Some(HostOverride {
            fields: fields_out,
            source: source.clone(),
            provenance,
        }),
        Some(source),
    ))
}

fn project_packages(
    value: Option<&OutputPayload>,
    path: &str,
) -> Result<Vec<Merge::Pkg>, PackageOutputError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let OutputPayload::Array(values) = value else {
        return Err(PackageOutputError::new(
            path,
            "must be an array of package refs",
        ));
    };
    let mut packages = Vec::new();
    for (index, value) in values.iter().enumerate() {
        let item_path = format!("{path}[{index}]");
        let raw = text_field(Some(value), &item_path)?;
        let package = if let Some((name, source)) = raw.rsplit_once('@') {
            if name.trim().is_empty() || source.trim().is_empty() {
                return Err(PackageOutputError::new(
                    item_path,
                    "package refs need both a name and a source",
                ));
            }
            Merge::Pkg::new(source.trim(), name.trim())
        } else if raw.starts_with("./") || raw.starts_with("../") || raw.starts_with('/') {
            Merge::Pkg::new("", raw)
        } else {
            // Package outputs have no second source table. Bare names use the
            // canonical built-in provider so the same plan is buildable.
            Merge::Pkg::new(crate::Syntax::REF_SOURCE_NIXPKGS, raw)
        };
        if !packages.contains(&package) {
            packages.push(package);
        }
    }
    Ok(packages)
}

fn project_services(
    value: Option<&OutputPayload>,
    path: &str,
) -> Result<Vec<ServicePlan>, PackageOutputError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let OutputPayload::Object(services) = value else {
        return Err(PackageOutputError::new(
            path,
            "must be an object of services",
        ));
    };
    let mut projected = Vec::new();
    for (name, value) in services {
        let service_path = format!("{path}.{name}");
        let OutputPayload::Object(fields) = value else {
            return Err(PackageOutputError::new(
                service_path,
                "must be an object with enable: true|false",
            ));
        };
        let enable = match fields.get(crate::Syntax::SERVICE_FIELD_ENABLE) {
            Some(OutputPayload::Bool(enable)) => *enable,
            Some(_) => {
                return Err(PackageOutputError::new(
                    format!("{service_path}.enable"),
                    "must be a boolean",
                ))
            }
            None => {
                return Err(PackageOutputError::new(
                    format!("{service_path}.enable"),
                    "is required",
                ))
            }
        };
        let extra = fields
            .iter()
            .filter(|(field, _)| field.as_str() != crate::Syntax::SERVICE_FIELD_ENABLE)
            .map(|(field, value)| (field.clone(), payload_display(value)))
            .collect();
        projected.push(ServicePlan {
            name: name.clone(),
            enable,
            extra,
        });
    }
    Ok(projected)
}

fn project_options(
    value: Option<&OutputPayload>,
    path: &str,
) -> Result<Vec<OptionPlan>, PackageOutputError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let OutputPayload::Object(fields) = value else {
        return Err(PackageOutputError::new(
            path,
            "must be an object of option values",
        ));
    };
    let mut options = Vec::new();
    for (field, value) in fields {
        flatten_option(field, value, path, &mut options)?;
    }
    Ok(options)
}

fn flatten_option(
    field: &str,
    value: &OutputPayload,
    path: &str,
    options: &mut Vec<OptionPlan>,
) -> Result<(), PackageOutputError> {
    match value {
        OutputPayload::Object(fields) => {
            for (nested, value) in fields {
                flatten_option(
                    &format!("{field}.{nested}"),
                    value,
                    &format!("{path}.{field}"),
                    options,
                )?;
            }
        }
        OutputPayload::Array(_) => {
            return Err(PackageOutputError::new(
                format!("{path}.{field}"),
                "must be a scalar or nested object",
            ))
        }
        _ => options.push(OptionPlan {
            key: field.to_string(),
            value: payload_display(value),
        }),
    }
    Ok(())
}

fn output_object<'a>(
    path: &str,
    output: &'a OutputFact,
) -> Result<&'a BTreeMap<String, OutputPayload>, PackageOutputError> {
    match &output.payload {
        OutputPayload::Object(fields) => Ok(fields),
        _ => Err(PackageOutputError::new(path, "must be an output object")),
    }
}

fn text_field(value: Option<&OutputPayload>, path: &str) -> Result<String, PackageOutputError> {
    match value {
        Some(OutputPayload::String(value)) => Ok(value.clone()),
        Some(_) => Err(PackageOutputError::new(
            path,
            "must be a string or dotted name",
        )),
        None => Err(PackageOutputError::new(path, "is required")),
    }
}

fn normalize_system_name(value: &str) -> String {
    value
        .strip_prefix("system.")
        .or_else(|| value.strip_prefix("systems."))
        .unwrap_or(value)
        .to_string()
}

fn payload_to_ct_value(value: &OutputPayload) -> CtValue {
    match value {
        OutputPayload::Null => CtValue::Unit,
        OutputPayload::Bool(value) => CtValue::Bool(*value),
        OutputPayload::Number(value) => value
            .parse::<i64>()
            .map(CtValue::Int)
            .or_else(|_| {
                value
                    .parse::<f64>()
                    .map(|value| CtValue::Float(crate::AST::CtFloat::f64(value)))
            })
            .unwrap_or_else(|_| CtValue::Str(value.clone())),
        OutputPayload::String(value) => CtValue::Str(value.clone()),
        OutputPayload::Array(values) => {
            CtValue::List(values.iter().map(payload_to_ct_value).collect())
        }
        OutputPayload::Object(fields) => CtValue::Map(
            fields
                .iter()
                .map(|(key, value)| (CtKey::Str(key.clone()), payload_to_ct_value(value)))
                .collect(),
        ),
    }
}

fn payload_display(value: &OutputPayload) -> String {
    match value {
        OutputPayload::Null => "null".to_string(),
        OutputPayload::Bool(value) => value.to_string(),
        OutputPayload::Number(value) | OutputPayload::String(value) => value.clone(),
        OutputPayload::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(payload_display)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        OutputPayload::Object(fields) => format!(
            "{{{}}}",
            fields
                .iter()
                .map(|(key, value)| format!("{key}: {}", payload_display(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_system_and_fleet_outputs_with_stable_identity() {
        let facts = PackageFacts::parse(
            r#"name: "demo"
outputs: .{
    workstation: .System.{
        name: "workstation"
        target: linux.x64
        packages: [ripgrep, "fd@nixpkgs", ripgrep]
        services: .{ ssh: .{ enable: true, ports: [22] } }
        options: .{ network: .{ hostName: "workstation" } }
    }
    prod: .Fleet.{ hosts: .{ edge: "system.workstation" } }
}"#,
            "package.jet",
        )
        .unwrap();
        let first = project_package_outputs(&facts).unwrap();
        let second = project_package_outputs(&facts).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            first.systems[0].packages[0],
            Merge::Pkg::new("nixpkgs", "ripgrep")
        );
        assert_eq!(first.systems[0].options[0].key, "network.hostName");
        assert_eq!(first.fleets[0].hosts[0].system, "workstation");
        assert_eq!(first.graph_identity, facts.semantic_digest());
    }

    #[test]
    fn rejects_a_system_service_without_enable() {
        let facts = PackageFacts::parse(
            r#"name: "demo"
outputs: .{ host: .System.{ target: linux.x64, services: .{ ssh: .{} } } }"#,
            "package.jet",
        )
        .unwrap();
        let error = project_package_outputs(&facts).unwrap_err();
        assert!(error
            .to_string()
            .contains("outputs.host.services.ssh.enable"));
    }
}
