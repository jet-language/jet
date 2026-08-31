//! Public read-only front-end toolkit API (D-FRONTENDAPI1=A).

use crate::Comptime::DevSink;
use crate::Diagnostics::{span_line_col, Diagnostic, Severity, Span};
use crate::Lexer::{TokKind, Token};
use crate::AST::{CtValue, Type};
use crate::{Lexer, Parser, AST};
use jet_foundation::Report::render_status_json;
use std::path::{Path, PathBuf};

use jet_driver::Authority::{AuthorityError, AuthorityResolver};
use jet_driver::{Lock, Package};
use jet_env_model::ModuleEval::{evaluate_env_with_source_loader, SourceLoader};

pub const API_VERSION: u32 = 1;
pub const SCHEMA_VERSION: u32 = 1;

/// Version 1 package-view field matrix.
///
/// `manifest` is the uncomposed declaration view and `package` is the
/// composed package-facts view. Both expose only manifest metadata that has no
/// `@build.*` spelling; current-package identity stays exclusively at
/// `@build.package.name` and `@build.package.version`. `dependencies` and
/// `outputs` use the underlying `BTreeMap` key order. `packages` and
/// `build_profiles` retain model order. Lock package and root-dependency lists
/// retain lock-model order. Profile sets and collision maps use key order;
/// profile `extends`, `packages`, and `sources` retain declaration order.
/// Optional source fields are `Option<String>` and collections are present as
/// empty lists when the model has no declarations. Every operation returns a
/// `Result` whose failure is `PackageReadError { code, file, message, cause }`;
/// the Jet carrier is `CompilerPackageError` with the same four fields. The
/// retained model records no per-field source positions, so the views expose
/// no fabricated position data.
pub const PACKAGE_MODEL_SCHEMA_VERSION: u32 = 1;

fn compiler_error_value(code: &str, message: impl Into<String>, span: Span) -> CtValue {
    ct_struct(
        "CompilerError",
        vec![
            ("code", CtValue::Str(code.to_string())),
            ("message", CtValue::Str(message.into())),
            ("span", span_value(span.into())),
        ],
    )
}

fn compiler_package_error_value(error: &PackageReadError) -> CtValue {
    ct_struct(
        "CompilerPackageError",
        vec![
            ("code", CtValue::Str(error.code.clone())),
            ("message", CtValue::Str(error.message.clone())),
            ("file", CtValue::Str(error.file.clone())),
            ("cause", CtValue::Str(error.cause.clone())),
        ],
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageReadError {
    pub code: String,
    pub message: String,
    pub file: String,
    pub cause: String,
}

impl PackageReadError {
    pub fn new(
        code: impl Into<String>,
        file: impl Into<String>,
        message: impl Into<String>,
        cause: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            file: file.into(),
            cause: cause.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyView {
    pub name: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageTargetView {
    pub name: String,
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputView {
    pub name: String,
    pub kind: String,
    pub entry: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildProfileView {
    pub name: String,
    pub optimize: String,
    pub debug_info: bool,
    pub small: bool,
    pub panic: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestView {
    pub schema_version: u32,
    pub file: String,
    pub jet: Option<String>,
    pub edition: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub repository: Option<String>,
    pub layer: Option<String>,
    pub target: Option<String>,
    pub dependencies: Vec<DependencyView>,
    pub packages: Vec<PackageTargetView>,
    pub outputs: Vec<OutputView>,
    pub build_profiles: Vec<BuildProfileView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageView {
    pub schema_version: u32,
    pub file: String,
    pub jet: Option<String>,
    pub edition: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub repository: Option<String>,
    pub layer: Option<String>,
    pub target: Option<String>,
    pub dependencies: Vec<DependencyView>,
    pub packages: Vec<PackageTargetView>,
    pub outputs: Vec<OutputView>,
    pub build_profiles: Vec<BuildProfileView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedPackageView {
    pub name: String,
    pub version: String,
    pub source_kind: String,
    pub source: Option<String>,
    pub revision: Option<String>,
    pub fingerprint: String,
    pub content_hash: Option<String>,
    pub dependencies: Vec<String>,
    pub layer: Option<String>,
    pub inferred_layer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockView {
    pub schema_version: u32,
    pub file: String,
    pub version: u32,
    pub root_dependencies: Vec<String>,
    pub packages: Vec<LockedPackageView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyValueView {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileView {
    pub name: String,
    pub extends: Vec<String>,
    pub packages: Vec<String>,
    pub collisions: Vec<KeyValueView>,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileSetView {
    pub schema_version: u32,
    pub file: String,
    pub profiles: Vec<ProfileView>,
}

fn package_read_error(
    file: impl Into<String>,
    message: impl Into<String>,
    cause: impl Into<String>,
) -> PackageReadError {
    PackageReadError::new("E0956", file, message, cause)
}

fn authority_read_error(
    file: impl Into<String>,
    message: impl Into<String>,
    cause: AuthorityError,
) -> PackageReadError {
    package_read_error(file, message, cause.to_string())
}

fn diagnostic_cause(diagnostic: &Diagnostic) -> String {
    match &diagnostic.detail {
        Some(detail) => format!("{}: {}; {}", diagnostic.code, diagnostic.what, detail),
        None => format!("{}: {}", diagnostic.code, diagnostic.what),
    }
}

fn record_checked_file(file: &jet_driver::Authority::CheckedFile) {
    let path = file
        .relative
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/");
    crate::Comptime::record_package_input(path, crate::SHA256::sha256_hex(&file.bytes));
}

fn record_checked_package_inputs(
    resolver: &AuthorityResolver,
    package: &jet_driver::Authority::CheckedPackage,
) -> Result<(), PackageReadError> {
    record_checked_file(&package.member.manifest.file);
    for path in &package.facts.resolved_config_paths {
        let file = resolver.checked_file(Path::new(path)).map_err(|cause| {
            authority_read_error(
                path.clone(),
                "could not revalidate a package configuration input",
                cause,
            )
        })?;
        record_checked_file(&file);
    }
    Ok(())
}

fn option_layer(layer: Option<crate::Syntax::RuntimeLayer>) -> Option<String> {
    layer.map(|layer| layer.as_str().to_string())
}

fn dependency_views(
    dependencies: &std::collections::BTreeMap<String, Package::DepSource>,
) -> Vec<DependencyView> {
    dependencies
        .iter()
        .map(|(name, source)| DependencyView {
            name: name.clone(),
            source: Package::dep_display_redacted(source),
        })
        .collect()
}

fn target_name(target: &Package::Target) -> &'static str {
    match target {
        Package::Target::Library => "library",
        Package::Target::Executable => "executable",
        Package::Target::Test => "test",
        Package::Target::Example => "example",
        Package::Target::Plugin { .. } => "plugin",
    }
}

fn package_target_views(packages: &[Package::PackageEntry]) -> Vec<PackageTargetView> {
    packages
        .iter()
        .map(|package| PackageTargetView {
            name: package.name.clone(),
            targets: package
                .targets
                .iter()
                .map(|target| target_name(target).to_string())
                .collect(),
        })
        .collect()
}

fn output_kind_name(kind: Package::PackageOutputKind) -> &'static str {
    match kind {
        Package::PackageOutputKind::Library => "library",
        Package::PackageOutputKind::Executable => "executable",
        Package::PackageOutputKind::Service => "service",
        Package::PackageOutputKind::Check => "check",
        Package::PackageOutputKind::Environment => "environment",
        Package::PackageOutputKind::Image => "image",
        Package::PackageOutputKind::Bundle => "bundle",
        Package::PackageOutputKind::System => "system",
        Package::PackageOutputKind::Fleet => "fleet",
    }
}

fn output_views(
    outputs: &std::collections::BTreeMap<String, Package::OutputFact>,
) -> Vec<OutputView> {
    outputs
        .values()
        .map(|output| OutputView {
            name: output.name.clone(),
            kind: output_kind_name(output.kind).to_string(),
            entry: output.entry.clone(),
        })
        .collect()
}

fn build_profile_views(profiles: &[Package::BuildProfileDef]) -> Vec<BuildProfileView> {
    profiles
        .iter()
        .map(|profile| BuildProfileView {
            name: profile.name.clone(),
            optimize: profile.optimize.as_str().to_string(),
            debug_info: profile.debug_info,
            small: profile.small,
            panic: profile.panic.map(|panic| match panic {
                Package::BuildPanic::Unwind => "unwind".to_string(),
                Package::BuildPanic::Abort => "abort".to_string(),
            }),
        })
        .collect()
}

fn manifest_view_from_facts(facts: &Package::PackageFacts) -> ManifestView {
    ManifestView {
        schema_version: PACKAGE_MODEL_SCHEMA_VERSION,
        file: crate::Syntax::PACKAGE_FILE.to_string(),
        jet: facts.jet.clone(),
        edition: facts.edition.clone(),
        description: facts.description.clone(),
        license: facts.license.clone(),
        repository: facts.repository.clone(),
        layer: option_layer(facts.layer),
        target: facts.target.clone(),
        dependencies: dependency_views(&facts.deps),
        packages: package_target_views(&facts.packages),
        outputs: output_views(&facts.outputs),
        build_profiles: build_profile_views(&facts.build_profiles),
    }
}

fn package_view_from_facts(facts: &Package::PackageFacts) -> PackageView {
    PackageView {
        schema_version: PACKAGE_MODEL_SCHEMA_VERSION,
        file: crate::Syntax::PACKAGE_FILE.to_string(),
        jet: facts.jet.clone(),
        edition: facts.edition.clone(),
        description: facts.description.clone(),
        license: facts.license.clone(),
        repository: facts.repository.clone(),
        layer: option_layer(facts.layer),
        target: facts.target.clone(),
        dependencies: dependency_views(&facts.deps),
        packages: package_target_views(&facts.packages),
        outputs: output_views(&facts.outputs),
        build_profiles: build_profile_views(&facts.build_profiles),
    }
}

fn lock_source_kind(source: &Lock::LockSource) -> &'static str {
    match source {
        Lock::LockSource::Root => "root",
        Lock::LockSource::Path(_) => "path",
        Lock::LockSource::Git { .. } => "git",
        Lock::LockSource::Nix { .. } => "nix",
        Lock::LockSource::Cran { .. } => "cran",
        Lock::LockSource::LuaRocks { .. } => "lua_rocks",
        Lock::LockSource::Registry { .. } => "registry",
        Lock::LockSource::Foreign { .. } => "foreign",
    }
}

fn lock_source_reference(source: &Lock::LockSource) -> Option<String> {
    match source {
        Lock::LockSource::Root | Lock::LockSource::Path(_) => None,
        Lock::LockSource::Git { selector, .. } => Some(selector.clone()),
        Lock::LockSource::Nix { reference, .. }
        | Lock::LockSource::Cran { reference, .. }
        | Lock::LockSource::LuaRocks { reference, .. }
        | Lock::LockSource::Registry { reference, .. }
        | Lock::LockSource::Foreign { reference, .. } => Some(reference.clone()),
    }
}

fn lock_view_from_lock(lock: &Lock::LockFile) -> LockView {
    LockView {
        schema_version: PACKAGE_MODEL_SCHEMA_VERSION,
        file: crate::Syntax::UNIFIED_LOCK_FILE.to_string(),
        version: lock.version,
        root_dependencies: lock.root_dependencies.clone(),
        packages: lock
            .packages
            .iter()
            .map(|package| LockedPackageView {
                name: package.name.clone(),
                version: package.version.clone(),
                source_kind: lock_source_kind(&package.source).to_string(),
                source: lock_source_reference(&package.source),
                revision: package.locked.as_ref().map(|revision| revision.rev.clone()),
                fingerprint: package.fingerprint.clone(),
                content_hash: package.content_hash.clone(),
                dependencies: package.dependencies.clone(),
                layer: option_layer(package.layer),
                inferred_layer: option_layer(package.inferred_layer),
            })
            .collect(),
    }
}

fn profile_view(profile: &jet_env_model::ModuleEval::PackageProfileSpec) -> ProfileView {
    ProfileView {
        name: profile.name.clone(),
        extends: profile.extends.clone(),
        packages: profile.packages.clone(),
        collisions: profile
            .collisions
            .iter()
            .map(|(key, value)| KeyValueView {
                key: key.clone(),
                value: value.clone(),
            })
            .collect(),
        sources: profile.sources.clone(),
    }
}

fn profile_set_view(
    profiles: &jet_env_model::ModuleEval::PackageProfileSet,
) -> ProfileSetView {
    ProfileSetView {
        schema_version: PACKAGE_MODEL_SCHEMA_VERSION,
        file: crate::Syntax::ENV_FILE.to_string(),
        profiles: profiles.profiles.values().map(profile_view).collect(),
    }
}

pub fn read_manifest(root: &Path) -> Result<ManifestView, PackageReadError> {
    let resolver = AuthorityResolver::open(root).map_err(|cause| {
        authority_read_error(
            crate::Syntax::PACKAGE_FILE,
            "could not open the pinned package root",
            cause,
        )
    })?;
    let manifest = resolver
        .checked_manifest(Path::new("."))
        .map_err(|cause| {
            authority_read_error(
                crate::Syntax::PACKAGE_FILE,
                "could not read the package manifest",
                cause,
            )
        })?;
    record_checked_file(&manifest.file);
    Ok(manifest_view_from_facts(&manifest.facts))
}

pub fn read_package(root: &Path) -> Result<PackageView, PackageReadError> {
    let resolver = AuthorityResolver::open(root).map_err(|cause| {
        authority_read_error(
            crate::Syntax::PACKAGE_FILE,
            "could not open the pinned package root",
            cause,
        )
    })?;
    let package = resolver.checked_root_package().map_err(|cause| {
        authority_read_error(
            crate::Syntax::PACKAGE_FILE,
            "could not compose the package facts",
            cause,
        )
    })?;
    record_checked_package_inputs(&resolver, &package)?;
    Ok(package_view_from_facts(&package.facts))
}

pub fn read_lock(root: &Path) -> Result<LockView, PackageReadError> {
    let resolver = AuthorityResolver::open(root).map_err(|cause| {
        authority_read_error(
            crate::Syntax::UNIFIED_LOCK_FILE,
            "could not open the pinned package root",
            cause,
        )
    })?;
    let file = resolver
        .checked_file(Path::new(crate::Syntax::UNIFIED_LOCK_FILE))
        .map_err(|cause| {
            authority_read_error(
                crate::Syntax::UNIFIED_LOCK_FILE,
                "could not read the package lock",
                cause,
            )
        })?;
    record_checked_file(&file);
    let text = file.text().map_err(|cause| {
        authority_read_error(
            crate::Syntax::UNIFIED_LOCK_FILE,
            "could not decode the package lock",
            cause,
        )
    })?;
    let lock = Lock::parse(&text).map_err(|cause| {
        package_read_error(
            crate::Syntax::UNIFIED_LOCK_FILE,
            "could not parse the package lock",
            cause,
        )
    })?;
    resolver
        .revalidate_file(&file)
        .map_err(|cause| {
            authority_read_error(
                crate::Syntax::UNIFIED_LOCK_FILE,
                "the package lock changed while it was being read",
                cause,
            )
        })?;
    Ok(lock_view_from_lock(&lock))
}

struct PackageSourceLoader {
    resolver: AuthorityResolver,
    checked_inputs: Vec<jet_driver::Authority::CheckedFile>,
    last_file: String,
}

impl SourceLoader for PackageSourceLoader {
    fn read_file(&mut self, relative: &Path) -> Result<String, Diagnostic> {
        self.last_file = relative
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        let file = self
            .resolver
            .checked_file(relative)
            .map_err(|cause| cause.diagnostic())?;
        record_checked_file(&file);
        let text = file.text().map_err(|cause| cause.diagnostic())?;
        self.checked_inputs.push(file);
        Ok(text)
    }

    fn list_jet_files(&mut self, relative: &Path) -> Result<Vec<PathBuf>, Diagnostic> {
        self.last_file = relative
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        let files = self
            .resolver
            .discover_files(relative, Some(crate::Syntax::FILE_EXT))
            .map_err(|cause| cause.diagnostic())?;
        for file in &files {
            record_checked_file(file);
        }
        self.checked_inputs.extend(files.iter().cloned());
        Ok(files.into_iter().map(|file| file.relative).collect())
    }

    fn package_facts(&mut self) -> Result<Option<Package::PackageFacts>, Diagnostic> {
        self.last_file = crate::Syntax::PACKAGE_FILE.to_string();
        let package = self
            .resolver
            .checked_root_package()
            .map_err(|cause| cause.diagnostic())?;
        record_checked_package_inputs(&self.resolver, &package).map_err(|error| {
            Diagnostic::error(
                error.code,
                error.message,
                error.cause,
                "restore the package inputs and try again".to_string(),
                None,
            )
        })?;
        Ok(Some(package.facts))
    }
}

pub fn read_profiles(root: &Path) -> Result<ProfileSetView, PackageReadError> {
    let resolver = AuthorityResolver::open(root).map_err(|cause| {
        authority_read_error(
            crate::Syntax::ENV_FILE,
            "could not open the pinned package root",
            cause,
        )
    })?;
    let env_file = resolver
        .checked_file(Path::new(crate::Syntax::ENV_FILE))
        .map_err(|cause| {
            authority_read_error(
                crate::Syntax::ENV_FILE,
                "could not read the profile source",
                cause,
            )
        })?;
    record_checked_file(&env_file);
    let source = env_file.text().map_err(|cause| {
        authority_read_error(
            crate::Syntax::ENV_FILE,
            "could not decode the profile source",
            cause,
        )
    })?;
    let base_dir = resolver.root().to_path_buf();
    let mut loader = PackageSourceLoader {
        resolver,
        checked_inputs: vec![env_file],
        last_file: crate::Syntax::ENV_FILE.to_string(),
    };
    let plan = match evaluate_env_with_source_loader(&source, &base_dir, &mut loader, None, None) {
        Ok(plan) => plan,
        Err(cause) => {
            return Err(package_read_error(
                loader.last_file.clone(),
                "could not evaluate the profile source",
                diagnostic_cause(&cause),
            ));
        }
    };
    for file in &loader.checked_inputs {
        let file_name = file
            .relative
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        loader
            .resolver
            .revalidate_file(file)
            .map_err(|cause| {
                authority_read_error(
                    file_name,
                    "a profile input changed while it was being read",
                    cause,
                )
            })?;
    }
    let mut profiles = jet_env_model::ModuleEval::PackageProfileSet::default();
    for profile in plan.package_profiles {
        profiles.insert_checked(profile).map_err(|cause| {
            package_read_error(
                crate::Syntax::ENV_FILE,
                "could not compose the profile set",
                cause.to_string(),
            )
        })?;
    }
    Ok(profile_set_view(&profiles))
}

fn optional_string(value: Option<&String>) -> CtValue {
    compiler_option_string(value.map(String::as_str))
}

fn dependency_value(dependency: &DependencyView) -> CtValue {
    ct_struct(
        "CompilerDependency",
        vec![
            ("name", CtValue::Str(dependency.name.clone())),
            ("source", CtValue::Str(dependency.source.clone())),
        ],
    )
}

fn package_target_value(package: &PackageTargetView) -> CtValue {
    ct_struct(
        "CompilerPackageTarget",
        vec![
            ("name", CtValue::Str(package.name.clone())),
            ("targets", compiler_string_list(package.targets.clone())),
        ],
    )
}

fn output_value(output: &OutputView) -> CtValue {
    ct_struct(
        "CompilerPackageOutput",
        vec![
            ("name", CtValue::Str(output.name.clone())),
            ("kind", CtValue::Str(output.kind.clone())),
            ("entry", optional_string(output.entry.as_ref())),
        ],
    )
}

fn build_profile_value(profile: &BuildProfileView) -> CtValue {
    ct_struct(
        "CompilerBuildProfile",
        vec![
            ("name", CtValue::Str(profile.name.clone())),
            ("optimize", CtValue::Str(profile.optimize.clone())),
            ("debug_info", CtValue::Bool(profile.debug_info)),
            ("small", CtValue::Bool(profile.small)),
            ("panic", optional_string(profile.panic.as_ref())),
        ],
    )
}

fn manifest_value(view: &ManifestView) -> CtValue {
    ct_struct(
        "CompilerManifest",
        vec![
            ("schema_version", CtValue::Int(i64::from(view.schema_version))),
            ("file", CtValue::Str(view.file.clone())),
            ("jet", optional_string(view.jet.as_ref())),
            ("edition", optional_string(view.edition.as_ref())),
            ("description", optional_string(view.description.as_ref())),
            ("license", optional_string(view.license.as_ref())),
            ("repository", optional_string(view.repository.as_ref())),
            ("layer", optional_string(view.layer.as_ref())),
            ("target", optional_string(view.target.as_ref())),
            (
                "dependencies",
                CtValue::List(view.dependencies.iter().map(dependency_value).collect()),
            ),
            (
                "packages",
                CtValue::List(view.packages.iter().map(package_target_value).collect()),
            ),
            (
                "outputs",
                CtValue::List(view.outputs.iter().map(output_value).collect()),
            ),
            (
                "build_profiles",
                CtValue::List(view.build_profiles.iter().map(build_profile_value).collect()),
            ),
        ],
    )
}

fn package_value(view: &PackageView) -> CtValue {
    ct_struct(
        "CompilerPackage",
        vec![
            ("schema_version", CtValue::Int(i64::from(view.schema_version))),
            ("file", CtValue::Str(view.file.clone())),
            ("jet", optional_string(view.jet.as_ref())),
            ("edition", optional_string(view.edition.as_ref())),
            ("description", optional_string(view.description.as_ref())),
            ("license", optional_string(view.license.as_ref())),
            ("repository", optional_string(view.repository.as_ref())),
            ("layer", optional_string(view.layer.as_ref())),
            ("target", optional_string(view.target.as_ref())),
            (
                "dependencies",
                CtValue::List(view.dependencies.iter().map(dependency_value).collect()),
            ),
            (
                "packages",
                CtValue::List(view.packages.iter().map(package_target_value).collect()),
            ),
            (
                "outputs",
                CtValue::List(view.outputs.iter().map(output_value).collect()),
            ),
            (
                "build_profiles",
                CtValue::List(view.build_profiles.iter().map(build_profile_value).collect()),
            ),
        ],
    )
}

fn locked_package_value(package: &LockedPackageView) -> CtValue {
    ct_struct(
        "CompilerLockedPackage",
        vec![
            ("name", CtValue::Str(package.name.clone())),
            ("version", CtValue::Str(package.version.clone())),
            ("source_kind", CtValue::Str(package.source_kind.clone())),
            ("source", optional_string(package.source.as_ref())),
            ("revision", optional_string(package.revision.as_ref())),
            ("fingerprint", CtValue::Str(package.fingerprint.clone())),
            ("content_hash", optional_string(package.content_hash.as_ref())),
            (
                "dependencies",
                compiler_string_list(package.dependencies.clone()),
            ),
            ("layer", optional_string(package.layer.as_ref())),
            (
                "inferred_layer",
                optional_string(package.inferred_layer.as_ref()),
            ),
        ],
    )
}

fn key_value_value(value: &KeyValueView) -> CtValue {
    ct_struct(
        "CompilerKeyValue",
        vec![
            ("key", CtValue::Str(value.key.clone())),
            ("value", CtValue::Str(value.value.clone())),
        ],
    )
}

fn profile_value(profile: &ProfileView) -> CtValue {
    ct_struct(
        "CompilerProfile",
        vec![
            ("name", CtValue::Str(profile.name.clone())),
            ("extends", compiler_string_list(profile.extends.clone())),
            ("packages", compiler_string_list(profile.packages.clone())),
            (
                "collisions",
                CtValue::List(profile.collisions.iter().map(key_value_value).collect()),
            ),
            ("sources", compiler_string_list(profile.sources.clone())),
        ],
    )
}

fn lock_value(view: &LockView) -> CtValue {
    ct_struct(
        "CompilerLock",
        vec![
            ("schema_version", CtValue::Int(i64::from(view.schema_version))),
            ("file", CtValue::Str(view.file.clone())),
            ("version", CtValue::Int(i64::from(view.version))),
            (
                "root_dependencies",
                compiler_string_list(view.root_dependencies.clone()),
            ),
            (
                "packages",
                CtValue::List(view.packages.iter().map(locked_package_value).collect()),
            ),
        ],
    )
}

fn profile_set_value(view: &ProfileSetView) -> CtValue {
    ct_struct(
        "CompilerProfileSet",
        vec![
            ("schema_version", CtValue::Int(i64::from(view.schema_version))),
            ("file", CtValue::Str(view.file.clone())),
            (
                "profiles",
                CtValue::List(view.profiles.iter().map(profile_value).collect()),
            ),
        ],
    )
}

/// D-FRONTENDAPI1=A: comptime bridge for the same read-only compiler values
/// exposed by this Rust module. The callback is installed at the compiler
/// entry seam; it deliberately declines every other Core module so the normal
/// interpreter/AOT paths remain unchanged.
pub fn eval_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    eval_core_call_with_type(module, method, args, span, None, None)
}

/// Ambient callback variant carrying the resolved return type for typed
/// absence payloads. The compiler API wrapper above stays four-argument for
/// callers that use the public read-only toolkit directly.
pub fn eval_core_call_with_type(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    _resolved_ret: Option<Type>,
    _sink: Option<&mut DevSink>,
) -> Option<Result<CtValue, Diagnostic>> {
    if module != "core.compiler" {
        return None;
    }
    if matches!(method, "manifest" | "package" | "lock" | "profiles") {
        if !args.is_empty() {
            return Some(Ok(CtValue::failed(Box::new(
                compiler_package_error_value(&PackageReadError::new(
                    "E0956",
                    "",
                    format!("`core.compiler.{method}` takes no arguments"),
                    "call the package view from `fn build` or a comptime binding without a path argument",
                )),
            ))));
        }
        let Some(root) = crate::Comptime::package_read_root() else {
            return Some(Ok(CtValue::failed(Box::new(
                compiler_package_error_value(&PackageReadError::new(
                    "E0956",
                    "",
                    format!("`core.compiler.{method}` is available only during package-aware compile-time evaluation"),
                    "call the package view from `fn build` or a comptime binding; the compiler pins the package root for you",
                )),
            ))));
        };
        let result = match method {
            "manifest" => read_manifest(&root).map(|view| manifest_value(&view)),
            "package" => read_package(&root).map(|view| package_value(&view)),
            "lock" => read_lock(&root).map(|view| lock_value(&view)),
            "profiles" => read_profiles(&root).map(|view| profile_set_value(&view)),
            _ => Err(package_read_error(
                "",
                "unknown package view operation",
                "the compiler package view dispatcher received an unsupported operation",
            )),
        };
        return Some(Ok(match result {
            Ok(value) => CtValue::Present(Box::new(value)),
            Err(error) => CtValue::failed(Box::new(compiler_package_error_value(&error))),
        }));
    }
    let source = match args.first() {
        Some(CtValue::Str(source)) if args.len() == 1 && method != "check" => source.clone(),
        Some(CtValue::Struct { type_name, fields })
            if args.len() == 1 && method == "check" && type_name == "CompilerSyntaxTree" =>
        {
            match fields
                .iter()
                .find_map(|(name, value)| {
                    (name == "source").then(|| match value {
                        CtValue::Str(source) => Some(source.clone()),
                        _ => None,
                    })
                })
                .flatten()
            {
                Some(source) => source,
                None => {
                    return Some(Ok(CtValue::failed(Box::new(compiler_error_value(
                        "E0956",
                        "`core.compiler.check` needs a parsed syntax tree with its source",
                        span,
                    )))))
                }
            }
        }
        _ => {
            let message = if method == "check" {
                "`core.compiler.check` expects one CompilerSyntaxTree".to_string()
            } else {
                format!("`core.compiler.{method}` expects one source String")
            };
            return Some(Ok(CtValue::failed(Box::new(compiler_error_value(
                "E0956", message, span,
            )))));
        }
    };
    if method == "check" {
        let Some(CtValue::Struct { fields, .. }) = args.first() else {
            unreachable!("check input was validated above")
        };
        let schema = fields
            .iter()
            .find_map(|(name, value)| (name == "schema_version").then_some(value));
        if !matches!(schema, Some(CtValue::Int(value)) if *value == i64::from(SCHEMA_VERSION)) {
            let got = schema
                .and_then(|value| match value {
                    CtValue::Int(value) => Some(*value),
                    _ => None,
                })
                .map_or_else(|| "missing".to_string(), |value| value.to_string());
            return Some(Ok(CtValue::failed(Box::new(compiler_error_value(
                "E0956",
                format!("unsupported CompilerSyntaxTree schema version {got}"),
                span,
            )))));
        }
    }
    let value = match method {
        "lex" => lexed_value(&lex_source(&source)),
        "parse" => syntax_tree_value(&parse_source(&source)),
        "check" => checked_value(&source),
        "source_map" => source_map_value(&source_map_from_generated_rust(&source)),
        _ => {
            return Some(Ok(CtValue::failed(Box::new(compiler_error_value(
                "E0956",
                format!("unknown `core.compiler` operation `{method}`"),
                span,
            )))))
        }
    };
    Some(Ok(CtValue::Present(Box::new(value))))
}

fn ct_struct(type_name: &str, fields: Vec<(&str, CtValue)>) -> CtValue {
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields: fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect(),
    }
}

fn span_value(range: TextRange) -> CtValue {
    ct_struct(
        crate::Syntax::TYPE_SOURCE_SPAN,
        vec![
            ("start", CtValue::Int(range.start as i64)),
            ("end", CtValue::Int(range.end as i64)),
        ],
    )
}

fn optional_span(range: Option<TextRange>) -> CtValue {
    range.map_or(
        CtValue::absent(Type::Named(crate::Syntax::TYPE_SOURCE_SPAN.to_string())),
        |range| CtValue::Present(Box::new(span_value(range))),
    )
}

fn diagnostic_value(diagnostic: &DiagnosticView) -> CtValue {
    ct_struct(
        "CompilerDiagnostic",
        vec![
            ("code", CtValue::Str(diagnostic.code.clone())),
            (
                "severity",
                CtValue::Str(match diagnostic.severity {
                    DiagnosticSeverity::Error => "error".to_string(),
                    DiagnosticSeverity::Lint => "lint".to_string(),
                }),
            ),
            ("message", CtValue::Str(diagnostic.message.clone())),
            ("why", CtValue::Str(diagnostic.why.clone())),
            ("fix", CtValue::Str(diagnostic.fix.clone())),
            ("span", optional_span(diagnostic.span)),
        ],
    )
}

fn lexed_value(lexed: &LexedSource) -> CtValue {
    ct_struct(
        "CompilerLexed",
        vec![
            ("schema_version", CtValue::Int(i64::from(SCHEMA_VERSION))),
            ("source", CtValue::Str(lexed.source.clone())),
            (
                "tokens",
                CtValue::List(
                    lexed
                        .tokens
                        .iter()
                        .map(|token| {
                            ct_struct(
                                "CompilerToken",
                                vec![
                                    ("kind", CtValue::Str(token.kind.to_string())),
                                    ("text", CtValue::Str(token.text.clone())),
                                    ("span", span_value(token.span)),
                                ],
                            )
                        })
                        .collect(),
                ),
            ),
            (
                "diagnostics",
                CtValue::List(lexed.diagnostics.iter().map(diagnostic_value).collect()),
            ),
        ],
    )
}

fn syntax_node_value(node: &SyntaxNode) -> CtValue {
    let kind = match node.kind {
        SyntaxNodeKind::Function => "function",
        SyntaxNodeKind::Struct => "struct",
        SyntaxNodeKind::Enum => "enum",
        SyntaxNodeKind::Trait => "trait",
        SyntaxNodeKind::Tag => "tag",
        SyntaxNodeKind::Effect => "effect",
        SyntaxNodeKind::Impl => "impl",
        SyntaxNodeKind::Const => "const",
        SyntaxNodeKind::Test => "test",
        SyntaxNodeKind::ExternRust => "extern_rust",
        SyntaxNodeKind::Module => "module",
        SyntaxNodeKind::CModule => "c_module",
        SyntaxNodeKind::CodeModule => "code_module",
        SyntaxNodeKind::ErrorConversion => "error_conversion",
        SyntaxNodeKind::Migration => "migration",
        SyntaxNodeKind::State => "state",
        SyntaxNodeKind::Protocol => "protocol",
        SyntaxNodeKind::Derive => "derive",
        SyntaxNodeKind::GenericModule => "generic_module",
        SyntaxNodeKind::ModuleAlias => "module_alias",
        SyntaxNodeKind::Distinct => "distinct",
        SyntaxNodeKind::TypeAlias => "type_alias",
        SyntaxNodeKind::UnitFamily => "unit_family",
        SyntaxNodeKind::Marker => "marker",
        SyntaxNodeKind::Fact => "fact",
        SyntaxNodeKind::TemplateLoop => "template_loop",
    };
    ct_struct(
        "CompilerNode",
        vec![
            ("kind", CtValue::Str(kind.to_string())),
            (
                "name",
                node.name
                    .clone()
                    .map_or(CtValue::absent(Type::String), |name| {
                        CtValue::Present(Box::new(CtValue::Str(name)))
                    }),
            ),
            ("span", span_value(node.span)),
        ],
    )
}

fn syntax_tree_value(tree: &SyntaxTree) -> CtValue {
    ct_struct(
        "CompilerSyntaxTree",
        vec![
            ("schema_version", CtValue::Int(i64::from(SCHEMA_VERSION))),
            ("source", CtValue::Str(tree.source.clone())),
            (
                "items",
                CtValue::List(tree.items.iter().map(syntax_node_value).collect()),
            ),
            (
                "diagnostics",
                CtValue::List(tree.diagnostics.iter().map(diagnostic_value).collect()),
            ),
        ],
    )
}

fn compiler_function_value(node: &SyntaxNode) -> CtValue {
    let name = node.name.clone().unwrap_or_default();
    ct_struct(
        "FunctionInfo",
        vec![
            ("name", CtValue::Str(name.clone())),
            ("module", CtValue::Str("core.compiler".to_string())),
            ("identity", CtValue::Str(format!("core.compiler::{name}"))),
            ("params", CtValue::List(Vec::new())),
            ("span", span_value(node.span)),
            (
                "effects",
                ct_struct("EffectInfo", vec![("values", CtValue::List(Vec::new()))]),
            ),
            ("reaches_panic", CtValue::Bool(false)),
            ("arithmetic", CtValue::List(Vec::new())),
        ],
    )
}

fn field_value(value: &CtValue, name: &str) -> Option<CtValue> {
    match value {
        CtValue::Struct { fields, .. } => fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.clone()),
        _ => None,
    }
}

fn effect_info_value(values: impl IntoIterator<Item = String>) -> CtValue {
    ct_struct(
        "EffectInfo",
        vec![(
            "values",
            CtValue::List(values.into_iter().map(CtValue::Str).collect()),
        )],
    )
}

fn compiler_option_string(value: Option<&str>) -> CtValue {
    value.map_or(CtValue::absent(Type::String), |value| {
        CtValue::Present(Box::new(CtValue::Str(value.to_string())))
    })
}

fn compiler_option_int(value: Option<usize>) -> CtValue {
    value.map_or(CtValue::absent(Type::Int), |value| {
        CtValue::Present(Box::new(CtValue::Int(value as i64)))
    })
}

fn compiler_string_list(values: impl IntoIterator<Item = String>) -> CtValue {
    CtValue::List(values.into_iter().map(CtValue::Str).collect())
}

fn compiler_semantic_span(span: jet_semindex::SourceSpan) -> CtValue {
    span_value(TextRange {
        start: span.start,
        end: span.end,
    })
}

fn compiler_arithmetic_value(value: &jet_semindex::ArithmeticOperationFact) -> CtValue {
    ct_struct(
        "CompilerArithmeticOperation",
        vec![
            ("operation", CtValue::Str(value.operation.clone())),
            ("policy", CtValue::Str(value.policy.clone())),
            ("module", CtValue::Str(value.module_path.clone())),
            (
                "operation_span",
                compiler_semantic_span(value.operation_span),
            ),
            ("scope_span", compiler_semantic_span(value.scope_span)),
        ],
    )
}

fn compiler_symbol_kind_value(kind: &jet_semindex::SymbolKind) -> CtValue {
    let (kind_name, params, ret, fields, variants, parent, mutable, ty) = match kind {
        jet_semindex::SymbolKind::Module => (
            "module",
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
        ),
        jet_semindex::SymbolKind::Function { params, ret, .. } => (
            "function",
            params
                .iter()
                .map(|(name, ty)| {
                    ct_struct(
                        "CompilerParam",
                        vec![
                            ("name", CtValue::Str(name.clone())),
                            ("ty", CtValue::Str(ty.clone())),
                        ],
                    )
                })
                .collect(),
            ret.as_deref(),
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
        ),
        jet_semindex::SymbolKind::Struct { fields } => (
            "struct",
            Vec::new(),
            None,
            fields
                .iter()
                .map(|(name, ty)| {
                    ct_struct(
                        "CompilerField",
                        vec![
                            ("name", CtValue::Str(name.clone())),
                            ("ty", CtValue::Str(ty.clone())),
                        ],
                    )
                })
                .collect(),
            Vec::new(),
            None,
            None,
            None,
        ),
        jet_semindex::SymbolKind::Enum { variants } => (
            "enum",
            Vec::new(),
            None,
            Vec::new(),
            variants.clone(),
            None,
            None,
            None,
        ),
        jet_semindex::SymbolKind::Trait => (
            "trait",
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
        ),
        jet_semindex::SymbolKind::Tag => (
            "tag",
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
        ),
        jet_semindex::SymbolKind::Type => (
            "type",
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
        ),
        jet_semindex::SymbolKind::Const => (
            "const",
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
        ),
        jet_semindex::SymbolKind::EnumVariant { parent } => (
            "enum_variant",
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            Some(parent.as_str()),
            None,
            None,
        ),
        jet_semindex::SymbolKind::Field { ty, parent } => (
            "field",
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            Some(parent.as_str()),
            None,
            Some(ty.as_str()),
        ),
        jet_semindex::SymbolKind::Local { mutable, ty } => (
            "local",
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            None,
            Some(*mutable),
            ty.as_deref(),
        ),
        jet_semindex::SymbolKind::Param { ty } => (
            "param",
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            None,
            None,
            Some(ty.as_str()),
        ),
    };
    ct_struct(
        "CompilerSymbolKind",
        vec![
            ("kind", CtValue::Str(kind_name.to_string())),
            ("params", CtValue::List(params)),
            ("ret", compiler_option_string(ret)),
            ("fields", CtValue::List(fields)),
            ("variants", compiler_string_list(variants)),
            ("parent", compiler_option_string(parent)),
            (
                "mutable",
                mutable.map_or(CtValue::absent(Type::Bool), |value| {
                    CtValue::Present(Box::new(CtValue::Bool(value)))
                }),
            ),
            ("ty", compiler_option_string(ty)),
        ],
    )
}

fn compiler_view_source_value(source: &jet_semindex::ViewSourceFact) -> CtValue {
    let (kind, index, module, name) = match source {
        jet_semindex::ViewSourceFact::Receiver => ("receiver", None, None, None),
        jet_semindex::ViewSourceFact::Parameter(index) => ("parameter", Some(*index), None, None),
        jet_semindex::ViewSourceFact::Static { module_path, name } => (
            "static",
            None,
            Some(module_path.as_str()),
            Some(name.as_str()),
        ),
    };
    ct_struct(
        "CompilerViewSource",
        vec![
            ("kind", CtValue::Str(kind.to_string())),
            ("index", compiler_option_int(index)),
            ("module", compiler_option_string(module)),
            ("name", compiler_option_string(name)),
        ],
    )
}

fn compiler_view_projection_value(projection: &jet_semindex::ViewProjectionFact) -> CtValue {
    let (kind, name) = match projection {
        jet_semindex::ViewProjectionFact::Field(name) => ("field", Some(name.as_str())),
        jet_semindex::ViewProjectionFact::Index => ("index", None),
        jet_semindex::ViewProjectionFact::Range => ("range", None),
    };
    ct_struct(
        "CompilerViewProjection",
        vec![
            ("kind", CtValue::Str(kind.to_string())),
            ("name", compiler_option_string(name)),
        ],
    )
}

fn compiler_view_provenance_value(provenance: &jet_semindex::ViewProvenanceFact) -> CtValue {
    let sources = provenance
        .sources
        .iter()
        .map(|source| {
            ct_struct(
                "CompilerViewSourcePath",
                vec![
                    ("source", compiler_view_source_value(&source.source)),
                    (
                        "projections",
                        CtValue::List(
                            source
                                .projections
                                .iter()
                                .map(compiler_view_projection_value)
                                .collect(),
                        ),
                    ),
                ],
            )
        })
        .collect();
    ct_struct(
        "CompilerViewProvenance",
        vec![
            (
                "output_path",
                compiler_string_list(provenance.output_path.clone()),
            ),
            ("sources", CtValue::List(sources)),
            ("mutable", CtValue::Bool(provenance.mutable)),
        ],
    )
}

fn compiler_definition_value(definition: &jet_semindex::SymbolDef) -> CtValue {
    ct_struct(
        "CompilerDefinition",
        vec![
            ("identity", CtValue::Str(definition.identity.clone())),
            ("name", CtValue::Str(definition.name.clone())),
            ("module", CtValue::Str(definition.module_path.clone())),
            ("span", compiler_semantic_span(definition.def_span)),
            ("kind", compiler_symbol_kind_value(&definition.kind)),
            (
                "view_provenance",
                CtValue::List(
                    definition
                        .view_provenance
                        .iter()
                        .map(compiler_view_provenance_value)
                        .collect(),
                ),
            ),
        ],
    )
}

fn compiler_anchor_value(anchor: &jet_semindex::DefinitionAnchor) -> CtValue {
    ct_struct(
        "CompilerDefinitionAnchor",
        vec![
            ("module", CtValue::Str(anchor.module_path.clone())),
            ("kind", CtValue::Str(anchor.kind.clone())),
            (
                "semantic_identity",
                compiler_option_string(anchor.semantic_identity.as_deref()),
            ),
            ("span", compiler_semantic_span(anchor.def_span)),
        ],
    )
}

fn compiler_reference_value(reference: &jet_semindex::SymbolRef) -> CtValue {
    ct_struct(
        "CompilerReference",
        vec![
            ("name", CtValue::Str(reference.name.clone())),
            ("module", CtValue::Str(reference.module_path.clone())),
            (
                "scope_identity",
                compiler_option_string(reference.scope_identity.as_deref()),
            ),
            (
                "target",
                reference.target.as_ref().map_or(
                    CtValue::absent(Type::Named("CompilerDefinitionAnchor".to_string())),
                    |target| CtValue::Present(Box::new(compiler_anchor_value(target))),
                ),
            ),
            ("span", compiler_semantic_span(reference.span)),
        ],
    )
}

fn compiler_call_value(call: &jet_semindex::CallEdge) -> CtValue {
    ct_struct(
        "CompilerCall",
        vec![
            ("caller", CtValue::Str(call.caller.clone())),
            ("callee", CtValue::Str(call.callee.clone())),
            ("module", CtValue::Str(call.module_path.clone())),
            ("span", compiler_semantic_span(call.call_span)),
        ],
    )
}

fn compiler_structural_node_value(node: &jet_semindex::StructuralNode) -> CtValue {
    ct_struct(
        "CompilerStructuralNode",
        vec![
            ("id", CtValue::Int(node.id as i64)),
            ("parent", compiler_option_int(node.parent)),
            ("slot", CtValue::Str(node.slot.clone())),
            (
                "slot_kind",
                CtValue::Str(
                    match node.slot_kind {
                        jet_semindex::StructuralSlotKind::Scalar => "scalar",
                        jet_semindex::StructuralSlotKind::List => "list",
                    }
                    .to_string(),
                ),
            ),
            ("ordinal", CtValue::Int(node.ordinal as i64)),
            ("class", CtValue::Str(node.class.clone())),
            ("shape", CtValue::Str(node.shape.clone())),
            ("module", CtValue::Str(node.module_path.clone())),
            ("span", compiler_semantic_span(node.span)),
        ],
    )
}

struct SemIndexProgramIndex<'a> {
    index: &'a jet_semindex::SemIndex,
}

impl crate::Comptime::ProgramIndexView for SemIndexProgramIndex<'_> {
    fn definitions(&self) -> Vec<CtValue> {
        self.index
            .definitions()
            .iter()
            .map(compiler_definition_value)
            .collect()
    }

    fn references(&self) -> Vec<CtValue> {
        self.index
            .references()
            .iter()
            .map(compiler_reference_value)
            .collect()
    }

    fn call_edges(&self) -> Vec<CtValue> {
        self.index
            .call_edges()
            .iter()
            .map(compiler_call_value)
            .collect()
    }

    fn structural_nodes(&self) -> Vec<CtValue> {
        self.index
            .structural_nodes()
            .iter()
            .map(compiler_structural_node_value)
            .collect()
    }
}

pub(crate) fn program_info_value(
    bundle: &AST::ProgramBundle,
    effect_facts: &crate::Sema::SemIndexEffectFacts,
) -> CtValue {
    let semantic_facts = crate::Driver::program_semantic_facts(bundle, effect_facts);
    let index = jet_semindex::from_checked(bundle, effect_facts);
    let view = SemIndexProgramIndex { index: &index };
    crate::Comptime::build_program_info_with_index(bundle, &semantic_facts, Some(&view))
}

fn compiler_effect_value(effect: &jet_semindex::EffectFact) -> CtValue {
    let provenance = effect
        .provenance
        .iter()
        .map(|origin| {
            ct_struct(
                "CompilerEffectProvenance",
                vec![
                    ("effect", CtValue::Str(origin.effect.clone())),
                    ("call_path", compiler_string_list(origin.call_path.clone())),
                    (
                        "spans",
                        CtValue::List(
                            origin
                                .spans
                                .iter()
                                .copied()
                                .map(compiler_semantic_span)
                                .collect(),
                        ),
                    ),
                ],
            )
        })
        .collect();
    ct_struct(
        "CompilerEffect",
        vec![
            ("function", CtValue::Str(effect.function.clone())),
            ("direct", compiler_string_list(effect.direct.clone())),
            ("callees", compiler_string_list(effect.callees.clone())),
            ("inferred", compiler_string_list(effect.inferred.clone())),
            ("maximal", CtValue::Bool(effect.maximal)),
            ("provenance", CtValue::List(provenance)),
        ],
    )
}

fn compiler_output_entry_value(entry: &jet_semindex::OutputEntryFact) -> CtValue {
    ct_struct(
        "CompilerOutputEntry",
        vec![
            ("identity", CtValue::Str(entry.identity.clone())),
            ("name", CtValue::Str(entry.name.clone())),
            ("module", CtValue::Str(entry.module_path.clone())),
            (
                "definition_span",
                compiler_semantic_span(entry.definition_span),
            ),
            (
                "reference_span",
                compiler_semantic_span(entry.reference_span),
            ),
            ("params", compiler_string_list(entry.params.clone())),
            (
                "return_type",
                compiler_option_string(entry.return_type.as_deref()),
            ),
            (
                "failure_contract",
                CtValue::Str(entry.failure_contract.clone()),
            ),
            ("failure_source", CtValue::Str(entry.failure_source.clone())),
            ("authority", CtValue::Str(entry.authority.clone())),
            ("effects", compiler_string_list(entry.effects.clone())),
        ],
    )
}

fn compiler_output_value(output: &jet_semindex::OutputFact) -> CtValue {
    ct_struct(
        "CompilerOutput",
        vec![
            ("binding", CtValue::Str(output.binding.clone())),
            ("kind", CtValue::Str(output.kind.clone())),
            ("name", CtValue::Str(output.name.clone())),
            ("module", CtValue::Str(output.module_path.clone())),
            ("span", compiler_semantic_span(output.span)),
            ("entry", compiler_output_entry_value(&output.entry)),
        ],
    )
}

fn compiler_semantic_index_value(index: &jet_semindex::SemIndex, source: &str) -> CtValue {
    ct_struct(
        "CompilerSemanticIndex",
        vec![
            (
                "schema_version",
                CtValue::Int(index.schema_version() as i64),
            ),
            (
                "source_digest",
                CtValue::Str(crate::SHA256::sha256_hex(source.as_bytes())),
            ),
            (
                "definitions",
                CtValue::List(
                    index
                        .definitions()
                        .iter()
                        .map(compiler_definition_value)
                        .collect(),
                ),
            ),
            (
                "references",
                CtValue::List(
                    index
                        .references()
                        .iter()
                        .map(compiler_reference_value)
                        .collect(),
                ),
            ),
            (
                "calls",
                CtValue::List(index.call_edges().iter().map(compiler_call_value).collect()),
            ),
            (
                "structural_nodes",
                CtValue::List(
                    index
                        .structural_nodes()
                        .iter()
                        .map(compiler_structural_node_value)
                        .collect(),
                ),
            ),
            (
                "effects",
                CtValue::List(index.effects().iter().map(compiler_effect_value).collect()),
            ),
            (
                "arithmetic",
                CtValue::List(
                    index
                        .arithmetic()
                        .iter()
                        .map(compiler_arithmetic_value)
                        .collect(),
                ),
            ),
            (
                "outputs",
                CtValue::List(index.outputs().iter().map(compiler_output_value).collect()),
            ),
        ],
    )
}

fn checked_value(source: &str) -> CtValue {
    let (checked_diagnostics, bundle, effect_facts) =
        crate::Driver::check_eval_with_effect_facts(source, "core.compiler.jet");
    checked_value_from_parts(source, &checked_diagnostics, bundle.as_ref(), &effect_facts)
}

fn checked_value_from_parts(
    source: &str,
    checked_diagnostics: &[Diagnostic],
    bundle: Option<&AST::ProgramBundle>,
    effect_facts: &crate::Sema::SemIndexEffectFacts,
) -> CtValue {
    let syntax = parse_source(source);
    let diagnostics = checked_diagnostics
        .iter()
        .map(diagnostic_view)
        .collect::<Vec<_>>();
    let has_errors = checked_diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error);
    let syntax_value = syntax_tree_value(&syntax);
    let (functions, effects, semantic_index) = if let Some(bundle) = bundle {
        let index = jet_semindex::from_checked(bundle, effect_facts);
        let semantic_facts = crate::Driver::program_semantic_facts(bundle, effect_facts);
        let view = SemIndexProgramIndex { index: &index };
        let program =
            crate::Comptime::build_program_info_with_index(bundle, &semantic_facts, Some(&view));
        let functions =
            field_value(&program, "functions").unwrap_or_else(|| CtValue::List(Vec::new()));
        let effects = CtValue::List(
            index
                .effects()
                .iter()
                .map(|effect| effect_info_value(effect.inferred.clone()))
                .collect(),
        );
        let semantic_index = if has_errors {
            CtValue::absent(Type::Named("CompilerSemanticIndex".to_string()))
        } else {
            CtValue::Present(Box::new(compiler_semantic_index_value(&index, source)))
        };
        (functions, effects, semantic_index)
    } else {
        let functions = syntax
            .items
            .iter()
            .filter(|node| node.kind == SyntaxNodeKind::Function)
            .map(compiler_function_value)
            .collect::<Vec<_>>();
        (
            CtValue::List(functions),
            CtValue::absent(Type::Named("CompilerSemanticIndex".to_string())),
            CtValue::absent(Type::Named("CompilerSemanticIndex".to_string())),
        )
    };
    ct_struct(
        "CompilerChecked",
        vec![
            ("schema_version", CtValue::Int(i64::from(SCHEMA_VERSION))),
            ("source", CtValue::Str(source.to_string())),
            ("syntax", syntax_value),
            (
                "diagnostics",
                CtValue::List(diagnostics.iter().map(diagnostic_value).collect()),
            ),
            ("functions", functions),
            ("effects", effects),
            ("semantic_index", semantic_index),
        ],
    )
}

fn source_map_value(map: &SourceMap) -> CtValue {
    ct_struct(
        "CompilerSourceMap",
        vec![
            ("schema_version", CtValue::Int(i64::from(SCHEMA_VERSION))),
            (
                "sources",
                CtValue::List(map.sources.iter().cloned().map(CtValue::Str).collect()),
            ),
            (
                "generated_lines",
                CtValue::List(
                    map.generated_lines
                        .iter()
                        .map(|line| {
                            ct_struct(
                                "CompilerGeneratedLine",
                                vec![
                                    ("generated_line", CtValue::Int(line.generated_line as i64)),
                                    (
                                        "source",
                                        line.source.clone().map_or(
                                            CtValue::absent(Type::String),
                                            |source| {
                                                CtValue::Present(Box::new(CtValue::Str(source)))
                                            },
                                        ),
                                    ),
                                    ("source_line", CtValue::Int(line.source_line as i64)),
                                ],
                            )
                        })
                        .collect(),
                ),
            ),
        ],
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRange {
    pub start: usize,
    pub end: usize,
}

impl From<Span> for TextRange {
    fn from(span: Span) -> Self {
        TextRange {
            start: span.start,
            end: span.end,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCol {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenView {
    pub kind: &'static str,
    pub text: String,
    pub span: TextRange,
    pub start: LineCol,
    pub end: LineCol,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Lint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticView {
    pub code: String,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub why: String,
    pub fix: String,
    pub span: Option<TextRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxNodeKind {
    Function,
    Struct,
    Enum,
    Trait,
    Tag,
    Effect,
    Impl,
    Const,
    Test,
    ExternRust,
    Module,
    CModule,
    CodeModule,
    ErrorConversion,
    Migration,
    State,
    Protocol,
    Derive,
    GenericModule,
    ModuleAlias,
    Distinct,
    TypeAlias,
    UnitFamily,
    Marker,
    Fact,
    TemplateLoop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxNode {
    pub kind: SyntaxNodeKind,
    pub name: Option<String>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxTree {
    pub api_version: u32,
    pub schema_version: u32,
    pub source: String,
    pub items: Vec<SyntaxNode>,
    pub diagnostics: Vec<DiagnosticView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexedSource {
    pub api_version: u32,
    pub schema_version: u32,
    pub source: String,
    pub tokens: Vec<TokenView>,
    pub diagnostics: Vec<DiagnosticView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMap {
    pub api_version: u32,
    pub schema_version: u32,
    pub sources: Vec<String>,
    pub generated_lines: Vec<GeneratedLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedLine {
    pub generated_line: usize,
    pub source: Option<String>,
    pub source_line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemIndexView {
    pub schema_version: u32,
    pub source_digest: String,
    pub definitions: Vec<jet_semindex::SymbolDef>,
    pub references: Vec<jet_semindex::SymbolRef>,
    pub calls: Vec<jet_semindex::CallEdge>,
    pub effects: Vec<jet_semindex::EffectFact>,
    pub arithmetic: Vec<jet_semindex::ArithmeticOperationFact>,
    pub outputs: Vec<jet_semindex::OutputFact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedFile {
    pub api_version: u32,
    pub schema_version: u32,
    pub diagnostics: Vec<DiagnosticView>,
    pub syntax: Option<SyntaxTree>,
    pub semantic_index: Option<SemIndexView>,
}

pub fn lex_source(src: &str) -> LexedSource {
    let (tokens, diagnostics) = Lexer::lex(src);
    LexedSource {
        api_version: API_VERSION,
        schema_version: SCHEMA_VERSION,
        source: src.to_string(),
        tokens: tokens.iter().map(|token| token_view(src, token)).collect(),
        diagnostics: diagnostics.iter().map(diagnostic_view).collect(),
    }
}

pub fn parse_source(src: &str) -> SyntaxTree {
    let lexed = lex_source(src);
    if !lexed.diagnostics.is_empty() {
        return SyntaxTree {
            api_version: API_VERSION,
            schema_version: SCHEMA_VERSION,
            source: src.to_string(),
            items: Vec::new(),
            diagnostics: lexed.diagnostics,
        };
    }

    let (tokens, _) = Lexer::lex(src);
    match Parser::parse_for_check_with_source(&tokens, src) {
        Ok((program, parse_teaching)) => SyntaxTree {
            api_version: API_VERSION,
            schema_version: SCHEMA_VERSION,
            source: src.to_string(),
            items: program.items.iter().map(item_node).collect(),
            diagnostics: parse_teaching.iter().map(diagnostic_view).collect(),
        },
        Err(diagnostics) => SyntaxTree {
            api_version: API_VERSION,
            schema_version: SCHEMA_VERSION,
            source: src.to_string(),
            items: Vec::new(),
            diagnostics: diagnostics.iter().map(diagnostic_view).collect(),
        },
    }
}

pub fn check_file(path: &std::path::Path) -> CheckedFile {
    let file = path.to_string_lossy();
    let (diagnostics, bundle, facts) =
        crate::Driver::check_file_with_effect_facts(&file, None, true);
    let has_errors = diagnostics.iter().any(|d| d.severity == Severity::Error);
    let source = std::fs::read_to_string(path).unwrap_or_default();
    let syntax = bundle
        .as_ref()
        .map(|bundle| bundle_syntax_tree(bundle, &source));
    let semantic_index = if has_errors {
        None
    } else {
        bundle.as_ref().map(|bundle| {
            SemIndexView::from_index(jet_semindex::from_checked(bundle, &facts), &source)
        })
    };
    CheckedFile {
        api_version: API_VERSION,
        schema_version: SCHEMA_VERSION,
        diagnostics: diagnostics.iter().map(diagnostic_view).collect(),
        syntax,
        semantic_index,
    }
}

/// Stable JSON envelope shared by the CLI mirror and callers that need to
/// persist compiler facts. This is deliberately hand-written: the compiler
/// seam has no serialization dependency, and the field order is part of the
/// schema's deterministic output.
pub const JSON_SCHEMA_VERSION: u32 = SCHEMA_VERSION;

fn compiler_json(operation: &str, payload: String) -> String {
    render_status_json(
        "ok",
        true,
        &format!("inspect.compiler.{operation}"),
        &format!(",\"compiler\":{payload}"),
    )
}

pub fn lex_source_json(source: &str) -> String {
    compiler_json(
        "lex",
        format!(
            "{{\"schema_version\":{},\"api_version\":{},\"operation\":\"lex\",\"value\":{}}}",
            JSON_SCHEMA_VERSION,
            API_VERSION,
            json_lexed(&lex_source(source)),
        ),
    )
}

pub fn parse_source_json(source: &str) -> String {
    compiler_json(
        "parse",
        format!(
            "{{\"schema_version\":{},\"api_version\":{},\"operation\":\"parse\",\"value\":{}}}",
            JSON_SCHEMA_VERSION,
            API_VERSION,
            json_syntax_tree(&parse_source(source)),
        ),
    )
}

pub fn check_file_json(path: &std::path::Path) -> String {
    let file = path.to_string_lossy();
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            return compiler_api_error_json(
                "check",
                &file,
                "E0956",
                format!("could not read compiler input: {error}"),
            )
        }
    };
    // Keep the JSON mirror byte-for-byte aligned with the typed, source-only
    // operation. The file belongs in the outer envelope; it must not change
    // the `CompilerChecked` value returned by `core.compiler.check`.
    let value = checked_value(&source).to_json();
    compiler_json("check", format!(
        "{{\"schema_version\":{},\"api_version\":{},\"operation\":\"check\",\"file\":{},\"value\":{}}}",
        JSON_SCHEMA_VERSION,
        API_VERSION,
        json_string(&file),
        value,
    ))
}

/// Serialize one compiler-operation failure at the JSON boundary. The typed
/// Jet surface uses `CompilerError`; JSON carries the same fields without
/// leaking a Rust or rustc error string as the whole payload.
pub fn compiler_api_error_json(
    operation: &str,
    file: &str,
    code: &str,
    message: impl Into<String>,
) -> String {
    render_status_json(
        "error",
        false,
        &format!("inspect.compiler.{operation}"),
        &format!(
            ",\"compiler\":{}",
            format!(
        "{{\"schema_version\":{},\"api_version\":{},\"operation\":{},\"file\":{},\"error\":{{\"code\":{},\"message\":{}}}}}",
        JSON_SCHEMA_VERSION,
        API_VERSION,
        json_string(operation),
        json_string(file),
        json_string(code),
        json_string(&message.into()),
            )
        ),
    )
}

pub fn source_map_json(rust_source: &str) -> String {
    compiler_json(
        "source_map",
        format!(
        "{{\"schema_version\":{},\"api_version\":{},\"operation\":\"source_map\",\"value\":{}}}",
        JSON_SCHEMA_VERSION,
        API_VERSION,
        json_source_map(&source_map_from_generated_rust(rust_source)),
    ),
    )
}

fn json_string(value: &str) -> String {
    format!("\"{}\"", jet_foundation::JSON::json_escape(value))
}

fn json_span(span: Option<TextRange>) -> String {
    span.map_or_else(
        || "null".to_string(),
        |range| format!("{{\"start\":{},\"end\":{}}}", range.start, range.end),
    )
}

fn json_diagnostic(diagnostic: &DiagnosticView) -> String {
    format!(
        "{{\"code\":{},\"severity\":{},\"message\":{},\"why\":{},\"fix\":{},\"span\":{}}}",
        json_string(&diagnostic.code),
        json_string(match diagnostic.severity {
            DiagnosticSeverity::Error => "error",
            DiagnosticSeverity::Lint => "lint",
        }),
        json_string(&diagnostic.message),
        json_string(&diagnostic.why),
        json_string(&diagnostic.fix),
        json_span(diagnostic.span),
    )
}

fn json_diagnostics(diagnostics: &[DiagnosticView]) -> String {
    format!(
        "[{}]",
        diagnostics
            .iter()
            .map(json_diagnostic)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn json_lexed(lexed: &LexedSource) -> String {
    let tokens = lexed
        .tokens
        .iter()
        .map(|token| {
            format!(
                "{{\"kind\":{},\"text\":{},\"span\":{{\"start\":{},\"end\":{}}},\"start\":{{\"line\":{},\"column\":{}}},\"end\":{{\"line\":{},\"column\":{}}}}}",
                json_string(token.kind),
                json_string(&token.text),
                token.span.start,
                token.span.end,
                token.start.line,
                token.start.column,
                token.end.line,
                token.end.column,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema_version\":{},\"api_version\":{},\"source\":{},\"tokens\":[{}],\"diagnostics\":{}}}",
        lexed.schema_version,
        lexed.api_version,
        json_string(&lexed.source),
        tokens,
        json_diagnostics(&lexed.diagnostics),
    )
}

fn json_syntax_tree(tree: &SyntaxTree) -> String {
    let items = tree
        .items
        .iter()
        .map(|node| {
            format!(
                "{{\"kind\":{},\"name\":{},\"span\":{{\"start\":{},\"end\":{}}}}}",
                json_string(match node.kind {
                    SyntaxNodeKind::Function => "function",
                    SyntaxNodeKind::Struct => "struct",
                    SyntaxNodeKind::Enum => "enum",
                    SyntaxNodeKind::Trait => "trait",
                    SyntaxNodeKind::Tag => "tag",
                    SyntaxNodeKind::Effect => "effect",
                    SyntaxNodeKind::Impl => "impl",
                    SyntaxNodeKind::Const => "const",
                    SyntaxNodeKind::Test => "test",
                    SyntaxNodeKind::ExternRust => "extern_rust",
                    SyntaxNodeKind::Module => "module",
                    SyntaxNodeKind::CModule => "c_module",
                    SyntaxNodeKind::CodeModule => "code_module",
                    SyntaxNodeKind::ErrorConversion => "error_conversion",
                    SyntaxNodeKind::Migration => "migration",
                    SyntaxNodeKind::State => "state",
                    SyntaxNodeKind::Protocol => "protocol",
                    SyntaxNodeKind::Derive => "derive",
                    SyntaxNodeKind::GenericModule => "generic_module",
                    SyntaxNodeKind::ModuleAlias => "module_alias",
                    SyntaxNodeKind::Distinct => "distinct",
                    SyntaxNodeKind::TypeAlias => "type_alias",
                    SyntaxNodeKind::UnitFamily => "unit_family",
                    SyntaxNodeKind::Marker => "marker",
                    SyntaxNodeKind::Fact => "fact",
                    SyntaxNodeKind::TemplateLoop => "template_loop",
                }),
                node.name
                    .as_deref()
                    .map(json_string)
                    .unwrap_or_else(|| "null".to_string()),
                node.span.start,
                node.span.end,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema_version\":{},\"api_version\":{},\"source\":{},\"items\":[{}],\"diagnostics\":{}}}",
        tree.schema_version,
        tree.api_version,
        json_string(&tree.source),
        items,
        json_diagnostics(&tree.diagnostics),
    )
}

fn json_source_map(map: &SourceMap) -> String {
    let sources = map
        .sources
        .iter()
        .map(|source| json_string(source))
        .collect::<Vec<_>>()
        .join(",");
    let lines = map
        .generated_lines
        .iter()
        .map(|line| {
            format!(
                "{{\"generated_line\":{},\"source\":{},\"source_line\":{}}}",
                line.generated_line,
                line.source
                    .as_deref()
                    .map(json_string)
                    .unwrap_or_else(|| "null".to_string()),
                line.source_line,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema_version\":{},\"api_version\":{},\"sources\":[{}],\"generated_lines\":[{}]}}",
        map.schema_version, map.api_version, sources, lines
    )
}

pub fn source_map_from_generated_rust(rust_src: &str) -> SourceMap {
    let mut sources = Vec::new();
    let mut current_source = None;
    let mut generated_lines = Vec::new();
    for (idx, line) in rust_src.lines().enumerate() {
        let generated_line = idx + 1;
        let trimmed = line.trim_start();
        if let Some(source) = trimmed.strip_prefix("// jet:source-map source=") {
            current_source = Some(source.to_string());
            if !sources.iter().any(|s| s == source) {
                sources.push(source.to_string());
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("// jet:line ") {
            if let Ok(source_line) = rest.trim().parse::<usize>() {
                generated_lines.push(GeneratedLine {
                    generated_line,
                    source: current_source.clone(),
                    source_line,
                });
            }
        }
    }
    SourceMap {
        api_version: API_VERSION,
        schema_version: SCHEMA_VERSION,
        sources,
        generated_lines,
    }
}

impl SemIndexView {
    fn from_index(index: jet_semindex::SemIndex, source: &str) -> Self {
        SemIndexView {
            schema_version: index.schema_version(),
            source_digest: crate::SHA256::sha256_hex(source.as_bytes()),
            definitions: index.definitions().to_vec(),
            references: index.references().to_vec(),
            calls: index.call_edges().to_vec(),
            effects: index.effects().to_vec(),
            arithmetic: index.arithmetic().to_vec(),
            outputs: index.outputs().to_vec(),
        }
    }
}

fn token_view(src: &str, token: &Token) -> TokenView {
    let start = line_col(src, token.span.start);
    let end = line_col(src, token.span.end);
    TokenView {
        kind: token_kind_name(&token.kind),
        text: token_text(src, token),
        span: token.span.into(),
        start,
        end,
    }
}

fn diagnostic_view(diagnostic: &Diagnostic) -> DiagnosticView {
    DiagnosticView {
        code: diagnostic.code.to_string(),
        severity: match diagnostic.severity {
            Severity::Error => DiagnosticSeverity::Error,
            Severity::Lint => DiagnosticSeverity::Lint,
        },
        message: diagnostic.what.clone(),
        why: diagnostic.why.clone(),
        fix: diagnostic.fix.clone(),
        span: diagnostic.span.map(Into::into),
    }
}

fn bundle_syntax_tree(bundle: &AST::ProgramBundle, source: &str) -> SyntaxTree {
    let mut items = Vec::new();
    for module in &bundle.modules {
        items.extend(module.items.iter().map(item_node));
    }
    SyntaxTree {
        api_version: API_VERSION,
        schema_version: SCHEMA_VERSION,
        source: source.to_string(),
        items,
        diagnostics: Vec::new(),
    }
}

fn item_node(item: &AST::Item) -> SyntaxNode {
    let (kind, name, span) = match item {
        AST::Item::Func(f) => (SyntaxNodeKind::Function, Some(f.name.clone()), f.name_span),
        AST::Item::Struct(s) => (SyntaxNodeKind::Struct, Some(s.name.clone()), s.name_span),
        AST::Item::Enum(e) => (SyntaxNodeKind::Enum, Some(e.name.clone()), e.name_span),
        AST::Item::Distinct(d) => (SyntaxNodeKind::Distinct, Some(d.name.clone()), d.name_span),
        AST::Item::TypeAlias(a) => (SyntaxNodeKind::TypeAlias, Some(a.name.clone()), a.name_span),
        AST::Item::UnitFamily(f) => (
            SyntaxNodeKind::UnitFamily,
            Some(f.family.clone()),
            f.family_span,
        ),
        AST::Item::Trait(t) => (SyntaxNodeKind::Trait, Some(t.name.clone()), t.name_span),
        AST::Item::Tag(t) => (SyntaxNodeKind::Tag, Some(t.name.clone()), t.name_span),
        AST::Item::EffectDecl(effect) => (
            SyntaxNodeKind::Effect,
            Some(effect.name.clone()),
            effect.name_span,
        ),
        AST::Item::Impl(i) => (SyntaxNodeKind::Impl, Some(i.type_name.clone()), i.type_span),
        AST::Item::Const(c) => (SyntaxNodeKind::Const, Some(c.name.clone()), c.name_span),
        AST::Item::Test(t) => (SyntaxNodeKind::Test, t.name.clone(), t.name_span),
        AST::Item::ExternRust(e) => (
            SyntaxNodeKind::ExternRust,
            Some(e.crate_spec.clone()),
            e.span,
        ),
        AST::Item::Module(m) => (SyntaxNodeKind::Module, Some(m.name.clone()), m.name_span),
        AST::Item::CModule(m) => (SyntaxNodeKind::CModule, Some(m.lib.clone()), m.path_span),
        AST::Item::CodeModule(m) => (
            SyntaxNodeKind::CodeModule,
            Some(m.name.clone()),
            m.name_span,
        ),
        AST::Item::ErrorConv(e) => (
            SyntaxNodeKind::ErrorConversion,
            Some(format!("{} -> {}", e.from_ty, e.to_ty)),
            e.from_span,
        ),
        AST::Item::Migration(m) => (
            SyntaxNodeKind::Migration,
            Some(m.type_name.clone()),
            m.type_span,
        ),
        AST::Item::ProtocolDecl(p) => (SyntaxNodeKind::Protocol, Some(p.name.clone()), p.name_span),
        AST::Item::UserDerive(d) => (
            SyntaxNodeKind::Derive,
            Some(d.trait_name.clone()),
            d.trait_span,
        ),
        AST::Item::GenericModule(m) => (
            SyntaxNodeKind::GenericModule,
            Some(m.name.clone()),
            m.name_span,
        ),
        AST::Item::ModuleAlias(m) => (
            SyntaxNodeKind::ModuleAlias,
            Some(m.name.clone()),
            m.name_span,
        ),
        AST::Item::MarkerDecl(m) => (SyntaxNodeKind::Marker, Some(m.name.clone()), m.name_span),
        AST::Item::FactDecl(f) => (SyntaxNodeKind::Fact, Some(f.name.clone()), f.name_span),
        // D-STRUCT-ONCE1=A: a root declaration loop is a nameless template
        // that sema expands; surface it by its own span.
        AST::Item::TemplateLoop(l) => (SyntaxNodeKind::TemplateLoop, None, l.span),
    };
    SyntaxNode {
        kind,
        name,
        span: span.into(),
    }
}

fn token_text(src: &str, token: &Token) -> String {
    if token.span.start <= token.span.end && token.span.end <= src.len() {
        src[token.span.start..token.span.end].to_string()
    } else {
        String::new()
    }
}

fn line_col(src: &str, offset: usize) -> LineCol {
    let (line, column) = span_line_col(src, offset);
    LineCol { line, column }
}

fn token_kind_name(kind: &TokKind) -> &'static str {
    match kind {
        TokKind::KwFn => "keyword.fn",
        TokKind::KwPub => "keyword.pub",
        TokKind::KwPriv => "keyword.priv",
        TokKind::KwIf => "keyword.if",
        TokKind::KwElse => "keyword.else",
        TokKind::KwIn => "keyword.in",
        TokKind::KwSwitch => "keyword.switch",
        TokKind::KwBreak => "keyword.break",
        TokKind::KwTrue => "literal.true",
        TokKind::KwFalse => "literal.false",
        TokKind::KwMutate => "keyword.mutate",
        TokKind::KwMove => "keyword.move",
        TokKind::KwCopy => "keyword.copy",
        TokKind::KwStruct => "keyword.struct",
        TokKind::KwEnum => "keyword.enum",
        TokKind::KwImpl => "keyword.impl",
        TokKind::KwTrait => "keyword.trait",
        TokKind::KwTag => "keyword.tag",
        TokKind::KwEffect => "keyword.effect",
        TokKind::KwDerive => "keyword.derive",
        TokKind::KwSelf => "keyword.self",
        TokKind::KwNull => "literal.null",
        TokKind::KwIt => "keyword.it",
        TokKind::KwConst => "keyword.const",
        TokKind::KwComptime => "keyword.comptime",
        TokKind::KwReturn => "keyword.return",
        TokKind::KwLoop => "keyword.loop",
        TokKind::KwYield => "keyword.yield",
        TokKind::KwUse => "keyword.use",
        TokKind::KwExtern => "keyword.extern",
        TokKind::KwModule => "keyword.module",
        TokKind::Ident(_) => "identifier",
        TokKind::Str(_) => "literal.string",
        TokKind::Int(..) => "literal.int",
        TokKind::Float(..) => "literal.float",
        TokKind::UnitNumber { .. } => "literal.unit_number",
        TokKind::Char(_) => "literal.char",
        TokKind::LParen => "punctuation.left_paren",
        TokKind::RParen => "punctuation.right_paren",
        TokKind::LBrace => "punctuation.left_brace",
        TokKind::RBrace => "punctuation.right_brace",
        TokKind::LBracket => "punctuation.left_bracket",
        TokKind::RBracket => "punctuation.right_bracket",
        TokKind::FenceOpen => "operator.fence_open",
        TokKind::FenceClose => "operator.fence_close",
        TokKind::Colon => "punctuation.colon",
        TokKind::ColonColon => "operator.bind_immutable",
        TokKind::ColonEq => "operator.bind_mutable",
        TokKind::Comma => "punctuation.comma",
        TokKind::Arrow => "operator.arrow",
        TokKind::UnifiedArrow => "operator.unified_arrow",
        TokKind::LambdaArrow => "operator.lambda_arrow",
        TokKind::Semi => "terminator",
        TokKind::Eq => "operator.assign",
        TokKind::Dot => "punctuation.dot",
        TokKind::DotDot => "operator.range",
        TokKind::DotDotLt => "operator.range_exclusive",
        TokKind::DotDotDot => "operator.spread",
        TokKind::At => "punctuation.at",
        TokKind::Question => "operator.try",
        TokKind::QuestionQuestion => "operator.fallback",
        TokKind::QuestionDot => "operator.optional_field",
        TokKind::Plus => "operator.add",
        TokKind::Minus => "operator.subtract",
        TokKind::Star => "operator.star",
        TokKind::Slash => "operator.divide",
        TokKind::SlashPercent => "operator.floor_divide",
        TokKind::Percent => "operator.modulo",
        TokKind::PercentPercent => "operator.remainder",
        TokKind::Amp => "operator.amp",
        TokKind::Pipe => "operator.alternative",
        TokKind::Caret => "operator.caret",
        TokKind::Tilde => "operator.tilde",
        TokKind::TildePipe => "operator.tilde_pipe",
        TokKind::TildePipeEq => "operator.tilde_pipe_assign",
        TokKind::TildeTilde => "operator.trait_attach",
        TokKind::Shl => "operator.shift_left",
        TokKind::Shr => "operator.shift_right",
        TokKind::AndAnd => "operator.and",
        TokKind::OrOr => "operator.or",
        TokKind::Bang => "operator.not",
        TokKind::EqEq => "operator.equal",
        TokKind::NotEq => "operator.not_equal",
        TokKind::Lt => "operator.less",
        TokKind::Gt => "operator.greater",
        TokKind::Le => "operator.less_equal",
        TokKind::Ge => "operator.greater_equal",
        TokKind::Compare => "operator.compare",
        TokKind::PlusEq => "operator.add_assign",
        TokKind::PlusPlus => "operator.increment",
        TokKind::MinusEq => "operator.subtract_assign",
        TokKind::MinusMinus => "operator.decrement",
        TokKind::StarEq => "operator.multiply_assign",
        TokKind::SlashEq => "operator.divide_assign",
        TokKind::SlashPercentEq => "operator.floor_divide_assign",
        TokKind::PercentEq => "operator.modulo_assign",
        TokKind::PercentPercentEq => "operator.remainder_assign",
        TokKind::AmpEq => "operator.amp_assign",
        TokKind::PipeEq => "operator.bit_or_assign",
        TokKind::CaretEq => "operator.caret_assign",
        TokKind::ShlEq => "operator.shift_left_assign",
        TokKind::ShrEq => "operator.shift_right_assign",
        TokKind::Hash => "punctuation.hash",
        TokKind::Dollar => "punctuation.dollar",
        TokKind::LineComment(_) => "comment.line",
        TokKind::BlockComment(_) => "comment.block",
        TokKind::Eof => "eof",
    }
}
