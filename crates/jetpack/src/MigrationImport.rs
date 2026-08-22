//! Editable migration-import IR (D-WD5).
//!
//! Command spelling and TODO diagnostic codes are gated. These helpers produce
//! canonical editable files and data-only TODO facts for callers/tests.

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
            let provider_ref = self
                .provider_facts
                .get(&dep.provider_ref)
                .filter(|facts| facts.is_lossless())
                .map(ProviderFacts::qualified_reference)
                .unwrap_or(dep.provider_ref);
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
    let name = toml_string(cargo_toml, "name").unwrap_or_else(|| "cargo-import".to_string());
    let version = toml_string(cargo_toml, "version").unwrap_or_else(|| "0.1.0".to_string());
    plan.packages.push(ImportedPackage { name, version });
    for dep in dependency_keys(cargo_toml, "[dependencies]") {
        let locked = lock_version(cargo_lock, &dep);
        let provider_ref = if locked.is_empty() {
            format!("{dep}@cargo")
        } else {
            format!("{dep}#version={locked}@cargo")
        };
        plan.deps.push(ImportedDep {
            name: dep.clone(),
            provider_ref: provider_ref.clone(),
            locked_version: locked.clone(),
            dev: false,
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
        if !locked.is_empty() {
            facts.set_resolved_source(&format!("cargo:{dep}@{locked}"));
            facts.add_fact(
                "package.version",
                ProviderFactValue::Text(locked.clone()),
                "Cargo.lock.version",
            );
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
    let name = obj
        .and_then(|m| m.get("name"))
        .and_then(|v| v.as_str().ok())
        .unwrap_or("npm-import")
        .to_string();
    let version = obj
        .and_then(|m| m.get("version"))
        .and_then(|v| v.as_str().ok())
        .unwrap_or("0.1.0")
        .to_string();
    plan.packages.push(ImportedPackage { name, version });
    if let Some(JSONValue::Object(deps)) = obj.and_then(|m| m.get("dependencies")) {
        for (name, val) in deps {
            plan.deps.push(ImportedDep {
                name: name.clone(),
                provider_ref: format!("{name}@npm"),
                locked_version: val.as_str().unwrap_or("").to_string(),
                dev: false,
            });
            let provider_ref = format!("{name}@npm");
            let mut facts = ProviderFacts::for_reference("npm", &provider_ref);
            facts.set_native_document("package.json", package_json);
            facts.add_fact(
                "package.name",
                ProviderFactValue::Text(name.clone()),
                "package.json.dependencies",
            );
            let requested = val.as_str().unwrap_or("").to_string();
            if !requested.is_empty() {
                facts.add_fact(
                    "package.request",
                    ProviderFactValue::Text(requested.clone()),
                    "package.json.dependencies",
                );
            }
            facts.add_loss(
                "provider.selector",
                &format!("npm dependency range `{requested}` is not an exact lock identity"),
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

fn lock_version(raw: &str, package: &str) -> String {
    let mut in_pkg = false;
    let mut found_name = false;
    let mut version = String::new();
    for line in raw.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            if found_name {
                return version;
            }
            in_pkg = true;
            found_name = false;
            version.clear();
            continue;
        }
        if in_pkg && line.starts_with("name =") {
            found_name = line.contains(&format!("\"{package}\""));
        }
        if in_pkg && line.starts_with("version =") {
            version = line
                .split_once('=')
                .map(|(_, v)| v.trim().trim_matches('"').to_string())
                .unwrap_or_default();
        }
    }
    if found_name {
        version
    } else {
        String::new()
    }
}
