//! Editable migration-import IR (D-WD5).
//!
//! Command spelling and TODO diagnostic codes are gated. These helpers produce
//! canonical editable files and data-only TODO facts for callers/tests.

use super::ProviderGraph::{normalize_provider_document, ProviderFamily};
use super::JSON::{self, JSONValue};
use jet_pkg_model::ProviderFacts::{ProviderFactValue, ProviderFacts};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportPlan {
    pub source_kind: String,
    pub packages: Vec<ImportedPackage>,
    pub deps: Vec<ImportedDep>,
    pub generated_files: Vec<GeneratedFile>,
    pub adapters: Vec<AdapterDraft>,
    pub ffi_stubs: Vec<FFIStub>,
    pub todos: Vec<ImportTodo>,
    pub migration: Vec<MigrationStatus>,
    /// One shared provider-fact record per imported provider reference. Losses
    /// remain on the record so generated files cannot hide an unresolved fact.
    pub provider_facts: BTreeMap<String, ProviderFacts>,
}

impl ImportPlan {
    pub fn empty(kind: impl Into<String>) -> ImportPlan {
        ImportPlan {
            source_kind: kind.into(),
            packages: Vec::new(),
            deps: Vec::new(),
            generated_files: Vec::new(),
            adapters: Vec::new(),
            ffi_stubs: Vec::new(),
            todos: Vec::new(),
            migration: Vec::new(),
            provider_facts: BTreeMap::new(),
        }
    }

    fn retain_provider_facts(&mut self, facts: ProviderFacts) {
        self.provider_facts.insert(facts.reference.clone(), facts);
    }

    pub fn emit_pkg_jet(&self) -> String {
        let name = self
            .packages
            .first()
            .map(|p| p.name.as_str())
            .unwrap_or("imported-app");
        let version = self
            .packages
            .first()
            .map(|p| p.version.as_str())
            .unwrap_or("0.1.0");
        let mut out = format!(
            "payload: {{\n    name: \"{name}\",\n    version: \"{version}\",\n    jet: \">=0.1.0\",\n    description: \"Imported from {}\",\n    license: \"\",\n    repository: \"\",\n}}\n",
            self.source_kind
        );
        out.push_str("\ndeps: {\n");
        let mut deps = self.deps.clone();
        deps.sort_by(|a, b| a.name.cmp(&b.name));
        for dep in deps {
            let Some(provider_ref) = self
                .provider_facts
                .get(&dep.provider_ref)
                .filter(|facts| facts.is_lossless())
                .map(ProviderFacts::qualified_reference)
            else {
                // A mutable foreign selector belongs in the source-linked
                // migration finding, never in generated Jet source.
                continue;
            };
            out.push_str(&format!("    {}: {},\n", dep.name, provider_ref));
        }
        out.push_str("}\n");
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedPackage {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedDep {
    pub name: String,
    pub provider_ref: String,
    pub locked_version: String,
    pub dev: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    pub path: String,
    pub contents: String,
    pub owned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterDraft {
    pub name: String,
    pub source: String,
    pub recipe: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FFIStub {
    pub path: String,
    pub symbol: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportTodo {
    pub source_path: String,
    pub message: String,
    pub suggested_surface: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationStatus {
    ForeignDependencyRetained { name: String },
    AdapterWrapped { name: String },
    FFIStubGenerated { name: String },
    NativeReplacementCandidate { name: String },
    CompatibilityProved { name: String },
    NativeReplacementActive { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportConflict {
    UserEditedGeneratedLine { path: String },
}

pub fn todo_diagnostics_need_ballot() -> bool {
    true
}

pub fn import_nix_facts(source_path: &str, facts_json: &str) -> ImportPlan {
    let mut plan = ImportPlan::empty("nix");
    let parsed = JSON::parse(facts_json).ok();
    let obj = parsed.as_ref().and_then(|j| j.as_object().ok());
    plan.packages.push(ImportedPackage {
        name: obj
            .and_then(|m| m.get("name"))
            .and_then(|v| v.as_str().ok())
            .unwrap_or("nix-import")
            .to_string(),
        version: "0.1.0".to_string(),
    });
    if let Some(JSONValue::Array(pkgs)) = obj.and_then(|m| m.get("packages")) {
        for pkg in pkgs {
            if let Ok(name) = pkg.as_str() {
                plan.deps.push(ImportedDep {
                    name: name.to_string(),
                    provider_ref: format!("{name}@nixpkgs"),
                    locked_version: String::new(),
                    dev: false,
                });
                let mut facts = ProviderFacts::for_reference("nix", &format!("{name}@nixpkgs"));
                facts.set_native_document("flake-facts.json", facts_json);
                facts.add_loss(
                    "provider.selector",
                    "imported Nix package has no exact package selector; retain the flake lock before realization",
                    source_path,
                );
                plan.retain_provider_facts(facts);
            }
        }
    }
    if obj.and_then(|m| m.get("shellHook")).is_some() {
        plan.todos.push(ImportTodo {
            source_path: source_path.to_string(),
            message: "`shellHook` needs an explicit Jetpack build/env action".to_string(),
            suggested_surface: Some("role module env.dev plus declared build action".to_string()),
        });
    }
    plan.generated_files.push(GeneratedFile {
        path: "env.jet".to_string(),
        contents: "module env.dev\n".to_string(),
        owned: true,
    });
    plan
}

pub fn import_cargo(cargo_toml: &str, cargo_lock: &str) -> ImportPlan {
    let mut plan = ImportPlan::empty("cargo");
    let source_name = toml_string(cargo_toml, "name");
    let source_version = toml_string(cargo_toml, "version");
    let name = source_name
        .clone()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "cargo-import".to_string());
    let version = source_version
        .clone()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "0.1.0".to_string());
    let root_reference = match (source_name, source_version) {
        (Some(name), Some(version)) if !name.is_empty() && !version.is_empty() => {
            format!("{name}#version={version}@cargo")
        }
        _ => format!("{name}@cargo"),
    };
    plan.packages.push(ImportedPackage {
        name: name.clone(),
        version: version.clone(),
    });
    retain_normalized_root(
        &mut plan,
        ProviderFamily::Cargo,
        &format!("Cargo.toml:\n{cargo_toml}\nCargo.lock:\n{cargo_lock}"),
        &root_reference,
    );
    for (section, dev, kind) in [
        ("[dependencies]", false, "runtime"),
        ("[dev-dependencies]", true, "dev"),
        ("[build-dependencies]", true, "build"),
    ] {
        for dep in dependency_keys(cargo_toml, section) {
            let versions = lock_versions(cargo_lock, &dep);
            let locked = (versions.len() == 1)
                .then(|| versions.first().cloned().unwrap_or_default())
                .unwrap_or_default();
            let provider_ref = if locked.is_empty() {
                format!("{dep}@cargo")
            } else {
                format!("{dep}#version={locked}@cargo")
            };
            plan.deps.push(ImportedDep {
                name: dep.clone(),
                provider_ref: provider_ref.clone(),
                locked_version: locked.clone(),
                dev,
            });
            let mut facts = ProviderFacts::for_reference("cargo", &provider_ref);
            facts.set_native_document(
                "Cargo.toml+Cargo.lock",
                &format!("Cargo.toml:\n{cargo_toml}\nCargo.lock:\n{cargo_lock}"),
            );
            facts.add_fact(
                "package.name",
                ProviderFactValue::Text(dep.clone()),
                "Cargo.toml.dependencies",
            );
            facts.add_fact(
                "package.dependency_kind",
                ProviderFactValue::Text(kind.to_string()),
                "Cargo.toml.dependencies",
            );
            if versions.len() > 1 {
                facts.add_conflict(
                    "provider.selector.version",
                    &versions[0],
                    &versions[1..].join(","),
                    "Cargo.lock",
                );
            } else if !locked.is_empty() {
                facts.set_resolved_source(&format!("cargo:{dep}@{locked}"));
                facts.add_fact(
                    "package.version",
                    ProviderFactValue::Text(locked.clone()),
                    "Cargo.lock.version",
                );
                if let Some(source) = lock_field(cargo_lock, &dep, "source") {
                    facts.add_fact(
                        "provider.source",
                        ProviderFactValue::Text(source),
                        "Cargo.lock.source",
                    );
                }
                if let Some(checksum) = lock_field(cargo_lock, &dep, "checksum") {
                    facts.add_fact(
                        "package.integrity",
                        ProviderFactValue::Text(checksum),
                        "Cargo.lock.checksum",
                    );
                }
            } else {
                facts.add_loss(
                    "provider.selector",
                    "Cargo.lock has no exact version for this dependency",
                    "Cargo.lock",
                );
                plan.todos.push(ImportTodo {
                    source_path: "Cargo.lock".to_string(),
                    message: format!("dependency `{dep}` remains unresolved until Cargo.lock supplies an exact version"),
                    suggested_surface: Some("pin the Cargo provider ref with #version=<exact>@cargo".to_string()),
                });
            }
            plan.retain_provider_facts(facts);
            plan.ffi_stubs.push(FFIStub {
                path: format!("ffi/{dep}.jet"),
                symbol: dep,
            });
        }
    }
    if cargo_toml.contains("build =") {
        plan.todos.push(ImportTodo {
            source_path: "Cargo.toml".to_string(),
            message: "Cargo build script becomes a Tier-2 legacy build action".to_string(),
            suggested_surface: Some(
                "declared build wrapper with inputs, outputs, and caps".to_string(),
            ),
        });
    }
    plan
}

pub fn import_npm(package_json: &str) -> ImportPlan {
    let mut plan = ImportPlan::empty("npm");
    let parsed = JSON::parse(package_json).ok();
    let obj = parsed.as_ref().and_then(|j| j.as_object().ok());
    let source_name = obj
        .and_then(|m| m.get("name"))
        .and_then(|v| v.as_str().ok())
        .unwrap_or_default()
        .to_string();
    let source_version = obj
        .and_then(|m| m.get("version"))
        .and_then(|v| v.as_str().ok())
        .unwrap_or_default()
        .to_string();
    let name = if source_name.is_empty() {
        "npm-import".to_string()
    } else {
        source_name.clone()
    };
    let version = if source_version.is_empty() {
        "0.1.0".to_string()
    } else {
        source_version.clone()
    };
    let root_reference = if source_name.is_empty() || source_version.is_empty() {
        format!("{name}@npm")
    } else {
        format!("{name}#version={version}@npm")
    };
    plan.packages.push(ImportedPackage {
        name: name.clone(),
        version: version.clone(),
    });
    retain_normalized_root(
        &mut plan,
        ProviderFamily::Npm,
        package_json,
        &root_reference,
    );
    if let Some(JSONValue::Object(deps)) = obj.and_then(|m| m.get("dependencies")) {
        for (name, val) in deps {
            let requested = val.as_str().ok().unwrap_or_default().to_string();
            let exact = exact_npm_version(&requested);
            let provider_ref = exact
                .as_deref()
                .map(|version| format!("{name}#version={version}@npm"))
                .unwrap_or_else(|| format!("{name}@npm"));
            plan.deps.push(ImportedDep {
                name: name.clone(),
                provider_ref: provider_ref.clone(),
                locked_version: exact.clone().unwrap_or_default(),
                dev: false,
            });
            let mut facts = ProviderFacts::for_reference("npm", &provider_ref);
            facts.set_native_document("package.json", package_json);
            facts.add_fact(
                "package.name",
                ProviderFactValue::Text(name.clone()),
                "package.json.dependencies",
            );
            if !requested.is_empty() {
                facts.add_fact(
                    "package.request",
                    ProviderFactValue::Text(requested.clone()),
                    "package.json.dependencies",
                );
            }
            if let Some(version) = exact {
                facts.set_resolved_source(&format!("npm:{name}@{version}"));
                facts.add_fact(
                    "package.version",
                    ProviderFactValue::Text(version),
                    "package.json exact selector",
                );
                plan.retain_provider_facts(facts);
            } else {
                facts.add_loss(
                    "provider.selector",
                    "npm dependency request is not an exact lock identity",
                    "package.json.dependencies",
                );
                plan.retain_provider_facts(facts);
                plan.todos.push(ImportTodo {
                    source_path: "package.json".to_string(),
                    message: format!("dependency `{name}` keeps npm request `{requested}`; resolve an exact package lock before realization"),
                    suggested_surface: Some("pin the npm provider ref with #version=<exact>@npm".to_string()),
                });
            }
        }
    }
    if let Some(JSONValue::Object(scripts)) = obj.and_then(|m| m.get("scripts")) {
        for name in scripts.keys() {
            plan.todos.push(ImportTodo {
                source_path: "package.json".to_string(),
                message: format!("npm script `{name}` becomes a declared legacy build action"),
                suggested_surface: Some("Tier-2 legacy build action".to_string()),
            });
        }
    }
    plan
}

fn retain_normalized_root(
    plan: &mut ImportPlan,
    family: ProviderFamily,
    document: &str,
    reference: &str,
) {
    let report = normalize_provider_document(family, document);
    plan.retain_provider_facts(report.shared_facts_for(reference));
}

pub fn import_python_metadata(name: &str, dynamic_fields: &[&str]) -> ImportPlan {
    let mut plan = ImportPlan::empty("pypi");
    plan.packages.push(ImportedPackage {
        name: name.to_string(),
        version: "0.1.0".to_string(),
    });
    for field in dynamic_fields {
        plan.todos.push(ImportTodo {
            source_path: "pyproject.toml".to_string(),
            message: format!("dynamic Python metadata field `{field}` must be resolved explicitly"),
            suggested_surface: None,
        });
    }
    let reference = format!("{name}@pypi");
    let mut facts = ProviderFacts::for_reference("pypi", &reference);
    facts.set_native_document("python-metadata", name);
    facts.add_loss(
        "provider.selector",
        "Python import has no exact distribution version",
        "pyproject.toml",
    );
    plan.retain_provider_facts(facts);
    plan
}

pub fn import_swiftpm(name: &str, revision: &str) -> ImportPlan {
    let mut plan = ImportPlan::empty("swiftpm");
    plan.packages.push(ImportedPackage {
        name: name.to_string(),
        version: revision.to_string(),
    });
    plan.deps.push(ImportedDep {
        name: name.to_string(),
        provider_ref: format!("{name}#revision={revision}@swiftpm"),
        locked_version: revision.to_string(),
        dev: false,
    });
    let provider_ref = format!("{name}#revision={revision}@swiftpm");
    let mut facts = ProviderFacts::for_reference("swiftpm", &provider_ref);
    facts.set_resolved_source(&format!("swiftpm:{name}@{revision}"));
    facts.set_native_document("Package.resolved", &provider_ref);
    facts.add_fact(
        "package.name",
        ProviderFactValue::Text(name.to_string()),
        "Package.resolved.identity",
    );
    facts.add_fact(
        "package.revision",
        ProviderFactValue::Text(revision.to_string()),
        "Package.resolved.revision",
    );
    plan.retain_provider_facts(facts);
    plan
}

pub fn merge_generated_file(
    existing: Option<&str>,
    generated: &GeneratedFile,
) -> Result<String, ImportConflict> {
    let Some(existing) = existing else {
        return Ok(generated.contents.clone());
    };
    if existing == generated.contents {
        return Ok(generated.contents.clone());
    }
    Err(ImportConflict::UserEditedGeneratedLine {
        path: generated.path.clone(),
    })
}

fn toml_string(raw: &str, key: &str) -> Option<String> {
    raw.lines().find_map(|line| {
        let line = line.trim();
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
    })
}

fn dependency_keys(raw: &str, section: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_section = false;
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_section = line == section;
            continue;
        }
        if in_section {
            if let Some((key, _)) = line.split_once('=') {
                out.push(key.trim().to_string());
            }
        }
    }
    out
}

fn exact_npm_version(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || ".-+".contains(character))
    {
        return None;
    }
    let core = value
        .split(|character: char| "-+".contains(character))
        .next()
        .unwrap_or_default();
    let parts = core.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    Some(value.to_string())
}

fn lock_versions(raw: &str, package: &str) -> Vec<String> {
    let mut versions = Vec::new();
    let mut in_package = false;
    let mut name = String::new();
    let mut version = String::new();
    let finish = |versions: &mut Vec<String>, name: &mut String, version: &mut String| {
        if name == package && !version.is_empty() {
            versions.push(version.clone());
        }
        name.clear();
        version.clear();
    };
    for line in raw.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            if in_package {
                finish(&mut versions, &mut name, &mut version);
            }
            in_package = true;
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            match key.trim() {
                "name" => name = value.trim().trim_matches('"').to_string(),
                "version" => version = value.trim().trim_matches('"').to_string(),
                _ => {}
            }
        }
    }
    if in_package {
        finish(&mut versions, &mut name, &mut version);
    }
    versions
}

fn lock_field(raw: &str, package: &str, field: &str) -> Option<String> {
    let mut in_package = false;
    let mut name = String::new();
    let mut value = None;
    for line in raw.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            if in_package && name == package && value.is_some() {
                return value;
            }
            in_package = true;
            name.clear();
            value = None;
            continue;
        }
        if !in_package {
            continue;
        }
        let Some((key, raw_value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "name" => name = raw_value.trim().trim_matches('"').to_string(),
            key if key == field => value = Some(raw_value.trim().trim_matches('"').to_string()),
            _ => {}
        }
    }
    (in_package && name == package).then_some(value).flatten()
}
