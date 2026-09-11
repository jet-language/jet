//! D-FFI-CPP1=A: clang-AST C++ surfaces lowered to a cached C ABI shim.
//! Clang's JSONValue is the declaration source of truth. The binder never parses
//! header text, and every native input participates in provenance/cache identity.
//! A local C-linkage declaration may also select the archive as the implementation
//! carrier for the canonical C binder; that path emits no invented C++ facade.

use crate::Bindgen::BindingPlan;
use crate::ForeignBridge::{
    ForeignArtifactCoverage, ForeignBoundaryContract, ForeignBoundaryIdentity,
    ForeignEvidenceBasis, IdentityBuilder,
};
use jet_foundation::DataTree::DataTree;
use crate::JSON;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const SCHEMA: &str = "jet-cpp-bind-v3";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateInstantiation {
    pub qualified_name: String,
    pub cpp_args: Vec<String>,
    pub jet_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindOptions {
    pub lib: String,
    pub target: String,
    pub clang: PathBuf,
    pub archiver: PathBuf,
    pub include_dirs: Vec<PathBuf>,
    pub library_dirs: Vec<PathBuf>,
    pub libraries: Vec<String>,
    pub namespaces: Vec<String>,
    pub templates: Vec<TemplateInstantiation>,
}

/// Checked descriptor overlay for a C++ library.
///
/// The binder does not infer semantics from generated names. Callers provide
/// the stable overlay identity and its source identity explicitly; both are
/// carried into cache/provenance and therefore invalidate a replacement when
/// the overlay changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CppOverlay {
    pub identity: String,
    pub source: String,
}

impl Default for CppOverlay {
    fn default() -> Self {
        Self {
            identity: "none".into(),
            source: "none".into(),
        }
    }
}

impl CppOverlay {
    pub fn new(identity: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            identity: identity.into(),
            source: source.into(),
        }
    }

    fn stable_identity(&self) -> String {
        format!("identity={};source={}", self.identity, self.source)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindResult {
    pub source: String,
    pub bound: Vec<String>,
    /// Content-addressed archive. A stable projection is also materialized for
    /// ordinary `use cpp.<lib>` discovery.
    pub archive: PathBuf,
    pub provenance: String,
    /// Adaptation plans carried with this generated surface; every plan points
    /// back to the same canonical boundary digest.
    pub plans: Vec<BindingPlan>,
    /// One boundary row consumed by inspect, comparison, and mixed debugging.
    pub boundary: ForeignBoundaryContract,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError {
    Source(String),
    ToolMissing(String),
    ToolFailed(String),
    IO(String),
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source(value) | Self::IO(value) => f.write_str(value),
            Self::ToolMissing(value) => write!(f, "the selected `{value}` tool was not found"),
            Self::ToolFailed(value) => write!(f, "the C++ toolchain rejected the binding: {value}"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scalar {
    Int,
    Float,
    Bool,
    Callback,
}

impl Scalar {
    fn jet(self) -> &'static str {
        match self {
            Self::Int => "Int",
            Self::Float => "Float",
            Self::Bool => "Bool",
            Self::Callback => "fn(Int) Int -[]>",
        }
    }

    fn c(self) -> &'static str {
        match self {
            Self::Int => "int64_t",
            Self::Float => "double",
            Self::Bool => "bool",
            Self::Callback => "int64_t (*)(int64_t)",
        }
    }

    fn zero(self) -> &'static str {
        match self {
            Self::Float => "0.0",
            Self::Bool => "false",
            _ => "0",
        }
    }
}

#[derive(Clone)]
struct Param {
    name: String,
    kind: Scalar,
}

#[derive(Clone)]
struct Routine {
    cpp_name: String,
    jet_name: String,
    params: Vec<Param>,
    result: Scalar,
}

struct Class {
    name: String,
    cpp_name: String,
    ctor: Vec<Param>,
    methods: Vec<Routine>,
}

struct FunctionTemplate {
    cpp_name: String,
    type_params: Vec<String>,
    params: Vec<(String, String)>,
    result: String,
}

struct Surface {
    classes: Vec<Class>,
    functions: Vec<Routine>,
    templates: Vec<FunctionTemplate>,
}

pub fn bind(header: &Path, cache: &Path, options: &BindOptions) -> Result<BindResult, BindError> {
    bind_with_overlay(header, cache, options, &CppOverlay::default())
}

/// Bind C++ with an explicit checked overlay identity.
pub fn bind_with_overlay(
    header: &Path,
    cache: &Path,
    options: &BindOptions,
    overlay: &CppOverlay,
) -> Result<BindResult, BindError> {
    validate_options(options)?;
    if [overlay.identity.as_str(), overlay.source.as_str()]
        .into_iter()
        .any(|value| value.bytes().any(|byte| matches!(byte, b'\n' | b'\r')))
    {
        return Err(BindError::Source(
            "C++ overlay identities cannot contain line breaks".into(),
        ));
    }
    let mut resolved = options.clone();
    resolved.clang = std::fs::canonicalize(&options.clang).map_err(|e| {
        BindError::IO(format!(
            "could not resolve `{}`: {e}",
            options.clang.display()
        ))
    })?;
    resolved.archiver = std::fs::canonicalize(&options.archiver).map_err(|e| {
        BindError::IO(format!(
            "could not resolve `{}`: {e}",
            options.archiver.display()
        ))
    })?;
    let options = &resolved;
    let canonical = std::fs::canonicalize(header)
        .map_err(|e| BindError::IO(format!("could not resolve `{}`: {e}", header.display())))?;
    let header_bytes = std::fs::read(&canonical)
        .map_err(|e| BindError::IO(format!("could not read `{}`: {e}", canonical.display())))?;
    let asts = clang_asts(&canonical, options)?;
    let source_dir = canonical.parent().unwrap_or_else(|| Path::new("."));
    let mut ast_identity = Vec::new();
    let mut has_c_abi_declarations = false;
    let mut surface = Surface {
        classes: Vec::new(),
        functions: Vec::new(),
        templates: Vec::new(),
    };
    for ast in &asts {
        ast_identity.extend_from_slice(&ast.stdout);
        ast_identity.push(0);
        let parsed = JSON::parse(&String::from_utf8_lossy(&ast.stdout)).map_err(|e| {
            BindError::ToolFailed(format!("clang returned malformed AST data: {e}"))
        })?;
        has_c_abi_declarations |= has_local_c_abi_declaration(&parsed, source_dir);
        let projected = project_surface(&parsed, &canonical, &options.namespaces)?;
        surface.classes.extend(projected.classes);
        surface.functions.extend(projected.functions);
        surface.templates.extend(projected.templates);
    }
    instantiate_templates(&mut surface, &options.templates)?;
    if surface.classes.is_empty()
        && surface.functions.is_empty()
        && !has_c_abi_declarations
    {
        return Err(BindError::Source(
            "clang found no bindable public scalar declarations in the selected namespaces"
                .to_string(),
        ));
    }

    let shim = render_cpp(&canonical, &options.lib, &surface);
    let jet_without_boundary = render_jet(&options.lib, &surface);
    let clang_version = tool_version(&options.clang, "clang++")?;
    let archiver_version = tool_version(&options.archiver, "ar")?;
    let digest = binding_identity(
        &canonical,
        &header_bytes,
        &ast_identity,
        &clang_version,
        &archiver_version,
        &shim,
        &jet_without_boundary,
        options,
        &options.target,
        overlay,
    );
    let descriptor = crate::AST::binder_descriptor(crate::AST::ForeignLanguage::Cpp)
        .ok_or_else(|| BindError::Source("C++ binder descriptor is not registered".into()))?;
    let boundary_identity = ForeignBoundaryIdentity::new(
        format!(
            "header:{}:sha256-{}",
            canonical.display(),
            crate::SHA256::sha256_hex(&header_bytes)
        ),
        overlay.stable_identity(),
        format!("cpp-clang;schema={SCHEMA};descriptor={}", descriptor.stamp()),
        format!(
            "shim:sha256-{};jet:sha256-{}",
            crate::SHA256::sha256_hex(shim.as_bytes()),
            crate::SHA256::sha256_hex(jet_without_boundary.as_bytes())
        ),
        format!(
            "clang={};ar={}",
            flatten_tool_version(&clang_version),
            flatten_tool_version(&archiver_version),
        ),
        options.target.clone(),
    );
    let mut boundary =
        ForeignBoundaryContract::new(*descriptor, options.lib.clone(), boundary_identity);
    let c_abi_carrier = surface.classes.is_empty() && surface.functions.is_empty();
    if c_abi_carrier {
        boundary.ownership =
            "canonical C owner/view contract remains authoritative; no C++ handle facade".into();
        boundary.cleanup =
            "canonical C close operation owns cleanup; CppBind materializes implementation only"
                .into();
        boundary.errors =
            "native C status values remain observable through the canonical C projection".into();
        boundary.exceptions =
            "C++ implementation is compiled in the carrier; no C++ exception facade is generated"
                .into();
        boundary.callbacks = "no CppBind callback projection is added by the carrier".into();
        boundary.task_thread =
            "no CppBind task/thread projection is added by the carrier".into();
        boundary.copy_cost =
            "the carrier adds no C++ facade conversions; canonical C projection owns ABI copies"
                .into();
    } else {
        boundary.ownership =
            "constructors return owned opaque handles; methods and close consume handles".into();
        boundary.cleanup =
            "generated close and consuming methods release native slots exactly once".into();
        boundary.errors =
            "caught C++ exceptions map to checked CppError; native error slot is consumed".into();
        boundary.exceptions = "std::exception and unknown exceptions are caught at the shim".into();
        boundary.callbacks =
            "callback pointers cross only through declared scalar signatures; captured state is not inferred"
                .into();
        boundary.task_thread =
            "callback/task/thread crossing remains an explicit overlay and is not guessed from names"
                .into();
        boundary.copy_cost =
            "scalar arguments/results copy at the C ABI; classes never copy native state".into();
    }
    boundary.assumptions = vec![
        "clang JSON AST is the declaration source of truth".into(),
        "templates are instantiated only from explicit options".into(),
    ];
    if c_abi_carrier {
        boundary.assumptions.push(
            "local C-linkage declarations selected implementation-carrier projection".into(),
        );
    }
    if overlay.identity != "none" || overlay.source != "none" {
        boundary
            .assumptions
            .push("checked C++ overlay identity is supplied by the caller".into());
    }
    let store = cache.join(&digest);
    let archive = store.join(format!("libjet_cpp_{}.a", options.lib));
    std::fs::create_dir_all(&store)
        .map_err(|e| BindError::IO(format!("could not create the C++ binding cache: {e}")))?;

    if !archive.is_file() {
        build_archive(&canonical, &shim, &archive, &store, options)?;
    }
    let mut dependencies = crate::ForeignBridge::local_archive_inputs(
        &options.library_dirs,
        &options.libraries,
    )
    .into_iter()
    .map(|input| {
        format!(
            "{}:{}:{}",
            input.library,
            input.path.display(),
            input
                .bytes
                .as_deref()
                .map(crate::SHA256::sha256_hex)
                .unwrap_or_else(|| "missing".into()),
        )
    })
    .collect::<Vec<_>>();
    dependencies.extend(options.libraries.iter().map(|library| format!("library:{library}")));
    dependencies.push(format!(
        "runtime:{}",
        crate::FFI::cxx_runtime_for_target(&options.target)
    ));
    let mut compiler_flags = vec![
        "-std=c++17".into(),
        "-fPIC".into(),
        format!("-target={}", options.target),
        format!(
            "undefined-symbols={}",
            crate::FFI::undefined_symbol_flag_for_target(&options.target)
        ),
    ];
    compiler_flags.extend(
        options
            .include_dirs
            .iter()
            .map(|directory| format!("-I{}", directory.display())),
    );
    compiler_flags.extend(
        options
            .library_dirs
            .iter()
            .map(|directory| format!("-L{}", directory.display())),
    );
    compiler_flags.extend(options.libraries.iter().map(|library| format!("-l{library}")));
    let loaded_artifact = format!(
        "{}:sha256-{}",
        archive.display(),
        crate::ForeignBridge::sha_file(&archive).map_err(BindError::Source)?
    );
    let coverage = ForeignArtifactCoverage::new(
        loaded_artifact,
        options.target.clone(),
        boundary.identity.generator.clone(),
    )
    .with_transitive_dependencies(dependencies)
    .with_reachable_callbacks(Vec::<String>::new())
    .with_compiler_flags(compiler_flags);
    boundary = boundary.with_artifact_coverage(coverage);
    for obligation in crate::ForeignBridge::FOREIGN_BOUNDARY_OBLIGATIONS {
        boundary
            .set_obligation(
                (*obligation).to_string(),
                ForeignEvidenceBasis::Trusted,
                "cpp-clang-generated-shim",
                [
                    "native implementation is TRUSTED outside hardening".into(),
                    "descriptor and loaded artifact identities are exact".into(),
                ],
            )
            .map_err(BindError::Source)?;
    }
    let boundary_stamp = boundary.stamp();
    let jet = format!("// jet-ffi-boundary={boundary_stamp}\n{jet_without_boundary}");
    materialize_projection(cache, &archive, options, &boundary)?;

    let provenance = render_provenance(
        &canonical,
        &digest,
        &surface,
        options,
        &clang_version,
        &archiver_version,
        &archive,
        &cache.join(format!("{}.link", options.lib)),
        overlay,
        &boundary,
    )
    .map_err(BindError::Source)?;
    let bound = surface
        .classes
        .iter()
        .flat_map(|class| {
            std::iter::once(format!("new_{}", snake(&class.name))).chain(
                class
                    .methods
                    .iter()
                    .map(|method| format!("{}.{}", class.name, method.jet_name)),
            )
        })
        .chain(
            surface
                .functions
                .iter()
                .map(|function| function.jet_name.clone()),
        )
        .collect();
    Ok(BindResult {
        source: jet,
        bound,
        archive,
        provenance,
        plans: Vec::new(),
        boundary,
    })
}

/// Bind C++ and append only caller-selected, already-resolved adaptation
/// facades. The boundary producer remains authoritative; a stale plan is
/// rejected instead of silently regenerating against a different artifact.
pub fn bind_with_overlay_and_plans(
    header: &Path,
    cache: &Path,
    options: &BindOptions,
    overlay: &CppOverlay,
    plans: &BTreeMap<String, BindingPlan>,
) -> Result<BindResult, BindError> {
    let mut result = bind_with_overlay(header, cache, options, overlay)?;
    let boundary_digest = result.boundary.digest();
    for (name, plan) in plans {
        if name != &plan.operation.name {
            return Err(BindError::Source(format!(
                "adaptation plan key `{name}` names `{}`",
                plan.operation.name
            )));
        }
        if plan.boundary_digest != boundary_digest {
            return Err(BindError::Source(format!(
                "adaptation plan `{name}` is stale against the generated C++ boundary"
            )));
        }
    }
    for plan in plans.values() {
        result.source.push_str(&plan.render_facade());
        result.source.push('\n');
    }
    result.plans = plans.values().cloned().collect();
    Ok(result)
}

#[doc(hidden)]
pub fn cache_identity_for_test(header: &Path, options: &BindOptions, target: &str) -> String {
    let canonical = std::fs::canonicalize(header).unwrap_or_else(|_| header.to_path_buf());
    let bytes = std::fs::read(&canonical).unwrap_or_default();
    let mut resolved = options.clone();
    if let Ok(path) = std::fs::canonicalize(&options.clang) {
        resolved.clang = path;
    }
    if let Ok(path) = std::fs::canonicalize(&options.archiver) {
        resolved.archiver = path;
    }
    let options = &resolved;
    let clang_version = tool_version(&options.clang, "clang++").unwrap_or_default();
    let archiver_version = tool_version(&options.archiver, "ar").unwrap_or_default();
    binding_identity(
        &canonical,
        &bytes,
        &[],
        &clang_version,
        &archiver_version,
        "",
        "",
        options,
        target,
        &CppOverlay::default(),
    )
}

fn validate_options(options: &BindOptions) -> Result<(), BindError> {
    if !ident(&options.lib) {
        return Err(BindError::Source(format!(
            "`{}` is not a valid Jet library name",
            options.lib
        )));
    }
    if options.target.trim().is_empty() {
        return Err(BindError::Source(
            "a selected target triple is required".into(),
        ));
    }
    for (name, path) in [("clang++", &options.clang), ("ar", &options.archiver)] {
        if !path.is_absolute() || !path.is_file() {
            return Err(BindError::ToolMissing(format!(
                "{name}` at `{}`",
                path.display()
            )));
        }
    }
    for library in &options.libraries {
        if !ident(library) {
            return Err(BindError::Source(format!(
                "`{library}` is not a safe native library name"
            )));
        }
    }
    for template in &options.templates {
        if !ident(&template.jet_name)
            || !qualified_ident(&template.qualified_name)
            || template.cpp_args.iter().any(|arg| scalar(arg).is_none())
        {
            return Err(BindError::Source(format!(
                "invalid explicit template request `{}`",
                template.qualified_name
            )));
        }
    }
    Ok(())
}

fn clang_asts(header: &Path, options: &BindOptions) -> Result<Vec<Output>, BindError> {
    let filters = if options.namespaces.is_empty() {
        vec![None]
    } else {
        options
            .namespaces
            .iter()
            .map(|name| Some(name.as_str()))
            .collect()
    };
    let mut outputs = Vec::new();
    for filter in filters {
        let mut command = Command::new(&options.clang);
        command.args(["-std=c++17", "-Xclang", "-ast-dump=json"]);
        if let Some(filter) = filter {
            command
                .arg("-Xclang")
                .arg(format!("-ast-dump-filter={filter}"));
        }
        command
            .args(["-fsyntax-only", "-target"])
            .arg(&options.target);
        for dir in &options.include_dirs {
            command.arg("-I").arg(dir);
        }
        command.arg(header);
        let output = supervised(&mut command, Duration::from_secs(60), "clang++")?;
        if !output.status.success() {
            return Err(BindError::ToolFailed(launder(&output.stderr)));
        }
        outputs.push(output);
    }
    Ok(outputs)
}

fn binding_identity(
    header: &Path,
    header_bytes: &[u8],
    ast: &[u8],
    clang_version: &[u8],
    archiver_version: &[u8],
    shim: &str,
    jet: &str,
    options: &BindOptions,
    target: &str,
    overlay: &CppOverlay,
) -> String {
    let mut identity = IdentityBuilder::new(crate::ForeignBridge::IDENTITY_SCHEMA);
    identity.field("binder_schema", SCHEMA.as_bytes());
    identity.field("descriptor", cpp_descriptor_stamp().as_bytes());
    identity.field("overlay_identity", overlay.identity.as_bytes());
    identity.field("overlay_source", overlay.source.as_bytes());
    identity.field("header", header.as_os_str().as_encoded_bytes());
    identity.field("header_bytes", header_bytes);
    identity.field("ast", ast);
    identity.field("clang_version", clang_version);
    identity.field("archiver_version", archiver_version);
    identity.field("shim", shim.as_bytes());
    identity.field("jet", jet.as_bytes());
    identity.field("lib", options.lib.as_bytes());
    identity.field("selected_target", target.as_bytes());
    identity.field("command_target", options.target.as_bytes());
    identity.field("clang", options.clang.as_os_str().as_encoded_bytes());
    identity.field("archiver", options.archiver.as_os_str().as_encoded_bytes());
    identity.field(
        "proof_suffix",
        crate::FFI::proof_suffix_for_target(target).as_bytes(),
    );
    identity.field(
        "undefined_symbols",
        crate::FFI::undefined_symbol_flag_for_target(target).as_bytes(),
    );
    identity.field(
        "cxx_runtime",
        crate::FFI::cxx_runtime_for_target(target).as_bytes(),
    );
    identity.field(
        "fixed_flags",
        b"-std=c++17\0-fPIC\0-c\0-target\0-shared\0rcs",
    );
    identity.field(
        "include_dirs_count",
        &(options.include_dirs.len() as u64).to_le_bytes(),
    );
    for value in &options.include_dirs {
        identity.field("include_dir", value.as_os_str().as_encoded_bytes());
    }
    identity.field(
        "library_dirs_count",
        &(options.library_dirs.len() as u64).to_le_bytes(),
    );
    for value in &options.library_dirs {
        identity.field("library_dir", value.as_os_str().as_encoded_bytes());
    }
    identity.field(
        "libraries_count",
        &(options.libraries.len() as u64).to_le_bytes(),
    );
    for value in &options.libraries {
        identity.field("library", value.as_bytes());
    }
    identity.field(
        "namespaces_count",
        &(options.namespaces.len() as u64).to_le_bytes(),
    );
    for value in &options.namespaces {
        identity.field("namespace", value.as_bytes());
    }
    identity.field(
        "templates_count",
        &(options.templates.len() as u64).to_le_bytes(),
    );
    for template in &options.templates {
        identity.field(
            "template.qualified_name",
            template.qualified_name.as_bytes(),
        );
        identity.field(
            "template.cpp_args_count",
            &(template.cpp_args.len() as u64).to_le_bytes(),
        );
        for value in &template.cpp_args {
            identity.field("template.cpp_arg", value.as_bytes());
        }
        identity.field("template.jet_name", template.jet_name.as_bytes());
    }
    crate::ForeignBridge::add_local_archive_inputs(
        &mut identity,
        &options.library_dirs,
        &options.libraries,
    );
    identity.finish()
}

fn project_surface(
    ast: &DataTree,
    header: &Path,
    selected: &[String],
) -> Result<Surface, BindError> {
    let mut surface = Surface {
        classes: Vec::new(),
        functions: Vec::new(),
        templates: Vec::new(),
    };
    let canonical = header.to_string_lossy();
    walk_ast(
        ast,
        &canonical,
        false,
        &mut Vec::new(),
        selected,
        &mut surface,
    )?;
    disambiguate(&mut surface.functions);
    Ok(surface)
}

fn walk_ast(
    value: &DataTree,
    header: &str,
    inherited_main: bool,
    namespace: &mut Vec<String>,
    selected: &[String],
    surface: &mut Surface,
) -> Result<(), BindError> {
    let Some(map) = object(value) else {
        return Ok(());
    };
    let kind = string(map, "kind").unwrap_or("");
    let in_main = location_file(map)
        .map(|file| file == header)
        .unwrap_or(inherited_main);

    if kind == "NamespaceDecl" {
        if let Some(name) = string(map, "name").filter(|name| !name.is_empty()) {
            namespace.push(name.to_string());
            for child in children(map) {
                walk_ast(child, header, in_main, namespace, selected, surface)?;
            }
            namespace.pop();
            return Ok(());
        }
    }

    let chosen = in_main && namespace_selected(namespace, selected);
    if chosen {
        match kind {
            "CXXRecordDecl"
                if bool_field(map, "completeDefinition") && !bool_field(map, "isImplicit") =>
            {
                surface.classes.push(parse_class(map, namespace)?);
                return Ok(());
            }
            "FunctionTemplateDecl" => {
                surface.templates.push(parse_template(map, namespace)?);
                return Ok(());
            }
            "FunctionDecl" if !bool_field(map, "isImplicit") => {
                surface.functions.push(parse_routine(
                    map,
                    qualified(namespace, string(map, "name").unwrap_or("")),
                    None,
                )?);
                return Ok(());
            }
            _ => {}
        }
    }
    for child in children(map) {
        walk_ast(child, header, in_main, namespace, selected, surface)?;
    }
    Ok(())
}

fn parse_class(
    map: &[(String, DataTree)],
    namespace: &[String],
) -> Result<Class, BindError> {
    let name = string(map, "name").unwrap_or("");
    if !ident(name) {
        return Err(BindError::Source(
            "clang reported an unnamed C++ class".into(),
        ));
    }
    let mut public = string(map, "tagUsed") == Some("struct");
    let mut ctor = None;
    let mut methods = Vec::new();
    for child in children(map) {
        let Some(child) = object(child) else { continue };
        match string(child, "kind").unwrap_or("") {
            "AccessSpecDecl" => public = string(child, "access") == Some("public"),
            "CXXConstructorDecl" if public && !bool_field(child, "isImplicit") => {
                if ctor.is_some() {
                    return Err(BindError::Source(format!(
                        "class `{name}` has multiple public constructors; select one in a smaller header"
                    )));
                }
                ctor = Some(parse_params(child, &BTreeMap::new())?);
            }
            "CXXMethodDecl" if public && !bool_field(child, "isImplicit") => {
                methods.push(parse_routine(
                    child,
                    string(child, "name").unwrap_or("").to_string(),
                    None,
                )?);
            }
            _ => {}
        }
    }
    disambiguate(&mut methods);
    Ok(Class {
        name: name.to_string(),
        cpp_name: qualified(namespace, name),
        ctor: ctor.ok_or_else(|| {
            BindError::Source(format!(
                "class `{name}` needs one public scalar constructor"
            ))
        })?,
        methods,
    })
}

fn parse_template(
    map: &[(String, DataTree)],
    namespace: &[String],
) -> Result<FunctionTemplate, BindError> {
    let name = string(map, "name").unwrap_or("");
    let mut type_params = Vec::new();
    let mut function = None;
    for child in children(map) {
        let Some(child) = object(child) else { continue };
        match string(child, "kind").unwrap_or("") {
            "TemplateTypeParmDecl" => {
                if let Some(name) = string(child, "name") {
                    type_params.push(name.to_string());
                }
            }
            "FunctionDecl" => function = Some(child),
            _ => {}
        }
    }
    let function = function.ok_or_else(|| {
        BindError::Source(format!(
            "template `{name}` has no callable clang declaration"
        ))
    })?;
    let result = return_type(function)?.to_string();
    let params = raw_params(function)?;
    Ok(FunctionTemplate {
        cpp_name: qualified(namespace, name),
        type_params,
        params,
        result,
    })
}

fn instantiate_templates(
    surface: &mut Surface,
    requests: &[TemplateInstantiation],
) -> Result<(), BindError> {
    let mut names = BTreeSet::new();
    for request in requests {
        if !names.insert(request.jet_name.clone()) {
            return Err(BindError::Source(format!(
                "duplicate Jet template name `{}`",
                request.jet_name
            )));
        }
        let template = surface
            .templates
            .iter()
            .find(|template| template.cpp_name == request.qualified_name)
            .ok_or_else(|| {
                BindError::Source(format!(
                    "clang found no function template `{}` (selected templates: {})",
                    request.qualified_name,
                    surface
                        .templates
                        .iter()
                        .map(|template| template.cpp_name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?;
        if template.type_params.len() != request.cpp_args.len() {
            return Err(BindError::Source(format!(
                "template `{}` needs {} type argument(s), not {}",
                request.qualified_name,
                template.type_params.len(),
                request.cpp_args.len()
            )));
        }
        let substitutions = template
            .type_params
            .iter()
            .cloned()
            .zip(request.cpp_args.iter().cloned())
            .collect::<BTreeMap<_, _>>();
        let params = template
            .params
            .iter()
            .map(|(name, ty)| {
                Ok(Param {
                    name: name.clone(),
                    kind: scalar_substituted(ty, &substitutions).ok_or_else(|| {
                        BindError::Source(format!(
                            "template `{}` parameter `{name}` has unsupported instantiated type `{ty}`",
                            request.qualified_name
                        ))
                    })?,
                })
            })
            .collect::<Result<Vec<_>, BindError>>()?;
        let result = scalar_substituted(&template.result, &substitutions).ok_or_else(|| {
            BindError::Source(format!(
                "template `{}` has unsupported instantiated return type `{}`",
                request.qualified_name, template.result
            ))
        })?;
        surface.functions.push(Routine {
            cpp_name: format!("{}<{}>", template.cpp_name, request.cpp_args.join(",")),
            jet_name: request.jet_name.clone(),
            params,
            result,
        });
    }
    disambiguate(&mut surface.functions);
    Ok(())
}

fn parse_routine(
    map: &[(String, DataTree)],
    cpp_name: String,
    jet_name: Option<String>,
) -> Result<Routine, BindError> {
    let raw_name = string(map, "name").unwrap_or("");
    let result = scalar(return_type(map)?)
        .ok_or_else(|| BindError::Source(format!("`{cpp_name}` has an unsupported return type")))?;
    Ok(Routine {
        cpp_name,
        jet_name: jet_name
            .or_else(|| operator_name(raw_name))
            .unwrap_or_else(|| snake(raw_name)),
        params: parse_params(map, &BTreeMap::new())?,
        result,
    })
}

fn parse_params(
    map: &[(String, DataTree)],
    substitutions: &BTreeMap<String, String>,
) -> Result<Vec<Param>, BindError> {
    raw_params(map)?
        .into_iter()
        .map(|(name, ty)| {
            Ok(Param {
                name: name.clone(),
                kind: scalar_substituted(&ty, substitutions).ok_or_else(|| {
                    BindError::Source(format!(
                        "parameter `{name}` has unsupported clang type `{ty}`"
                    ))
                })?,
            })
        })
        .collect()
}

fn raw_params(map: &[(String, DataTree)]) -> Result<Vec<(String, String)>, BindError> {
    children(map)
        .iter()
        .filter_map(|value| object(value))
        .filter(|value| string(value, "kind") == Some("ParmVarDecl"))
        .enumerate()
        .map(|(index, value)| {
            Ok((
                string(value, "name")
                    .filter(|name| ident(name))
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("arg{}", index + 1)),
                qual_type(value)?.to_string(),
            ))
        })
        .collect()
}

fn return_type(map: &[(String, DataTree)]) -> Result<&str, BindError> {
    let ty = qual_type(map)?;
    ty.split_once(" (")
        .map(|(result, _)| result.trim())
        .ok_or_else(|| BindError::Source(format!("clang returned an invalid function type `{ty}`")))
}

fn qual_type(map: &[(String, DataTree)]) -> Result<&str, BindError> {
    field(map, "type")
        .and_then(object)
        .and_then(|value| string(value, "qualType"))
        .ok_or_else(|| BindError::Source("clang omitted a declaration type".into()))
}

fn scalar_substituted(raw: &str, substitutions: &BTreeMap<String, String>) -> Option<Scalar> {
    let replaced = substitutions
        .get(raw.trim())
        .map(String::as_str)
        .unwrap_or(raw);
    scalar(replaced)
}

fn scalar(raw: &str) -> Option<Scalar> {
    let compact = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact == "int64_t (*)(int64_t)" {
        return Some(Scalar::Callback);
    }
    let value = compact
        .replace("const ", "")
        .replace("volatile ", "")
        .replace(['&', '*'], "")
        .trim()
        .to_string();
    match value.as_str() {
        "int64_t" | "long long" | "long" => Some(Scalar::Int),
        "double" => Some(Scalar::Float),
        "bool" => Some(Scalar::Bool),
        _ => None,
    }
}

fn namespace_selected(namespace: &[String], selected: &[String]) -> bool {
    selected.is_empty()
        || selected
            .iter()
            .any(|choice| choice == &namespace.join("::"))
}

fn qualified(namespace: &[String], name: &str) -> String {
    if namespace.is_empty() {
        name.to_string()
    } else {
        format!("{}::{name}", namespace.join("::"))
    }
}

fn field<'a>(map: &'a [(String, DataTree)], key: &str) -> Option<&'a DataTree> {
    map.iter()
        .find_map(|(name, value)| (name == key).then_some(value))
}

fn object(value: &DataTree) -> Option<&[(String, DataTree)]> {
    match value {
        DataTree::Object(value) => Some(value),
        _ => None,
    }
}
fn string<'a>(map: &'a [(String, DataTree)], key: &str) -> Option<&'a str> {
    field(map, key).and_then(|value| match value {
        DataTree::Text(value) | DataTree::TypedText(value) => Some(value.as_str()),
        _ => None,
    })
}

fn bool_field(map: &[(String, DataTree)], key: &str) -> bool {
    matches!(field(map, key), Some(DataTree::Bool(true)))
}

fn children(map: &[(String, DataTree)]) -> &[DataTree] {
    match field(map, "inner") {
        Some(DataTree::Array(values)) => values,
        _ => &[],
    }
}

fn location_file(map: &[(String, DataTree)]) -> Option<&str> {
    field(map, "loc")
        .and_then(object)
        .and_then(|loc| string(loc, "file"))
}

/// Detect a local C-linkage declaration so a C++ header can be used as the
/// native implementation carrier for the canonical C binder. This is derived
/// from clang's AST rather than a command flag or a guessed filename; system
/// headers and unrelated C++ surfaces do not unlock the empty-surface path.
fn has_local_c_abi_declaration(value: &DataTree, source_dir: &Path) -> bool {
    let Some(map) = object(value) else {
        return false;
    };
    if string(map, "kind") == Some("FunctionDecl")
        && string(map, "languageLinkage") == Some("C")
        && location_file(map).is_some_and(|file| {
            !file.starts_with('<') && Path::new(file).starts_with(source_dir)
        })
    {
        return true;
    }
    children(map)
        .iter()
        .any(|child| has_local_c_abi_declaration(child, source_dir))
}

fn disambiguate(routines: &mut [Routine]) {
    for index in 0..routines.len() {
        if routines
            .iter()
            .enumerate()
            .any(|(other, routine)| other != index && routine.jet_name == routines[index].jet_name)
        {
            let labels = routines[index]
                .params
                .iter()
                .map(|param| param.name.as_str())
                .collect::<Vec<_>>()
                .join("_");
            routines[index].jet_name = format!(
                "{}_{}",
                routines[index].jet_name,
                if labels.is_empty() {

                    "no_args"
                } else {
                    &labels
                }
            );
        }
    }
}

fn operator_name(value: &str) -> Option<String> {
    Some(
        match value {
            "operator+" => "add",
            "operator-" => "subtract",
            "operator*" => "multiply",
            "operator/" => "divide",
            "operator==" => "equals",
            _ => return None,
        }
        .into(),
    )
}

fn build_archive(
    header: &Path,
    shim: &str,
    archive: &Path,
    store: &Path,
    options: &BindOptions,
) -> Result<(), BindError> {
    let build = store.join(".build");
    let _ = std::fs::remove_dir_all(&build);
    std::fs::create_dir_all(&build)
        .map_err(|e| BindError::IO(format!("could not create the C++ build directory: {e}")))?;
    let cpp = build.join("shim.cpp");
    let object = build.join("shim.o");
    let proof = build.join(format!(
        "proof.{}",
        crate::FFI::proof_suffix_for_target(&options.target)
    ));
    std::fs::write(&cpp, shim)
        .map_err(|e| BindError::IO(format!("could not write the C++ shim: {e}")))?;

    let mut compile = Command::new(&options.clang);
    compile
        .args(["-std=c++17", "-fPIC", "-c", "-target"])
        .arg(&options.target);
    for dir in &options.include_dirs {
        compile.arg("-I").arg(dir);
    }
    compile.arg(&cpp).arg("-o").arg(&object);
    run(&mut compile, "clang++")?;

    let mut link = Command::new(&options.clang);
    link.arg("-target").arg(&options.target).arg("-shared");
    link.arg(crate::FFI::undefined_symbol_flag_for_target(
        &options.target,
    ));
    link.arg(&object);
    add_link_inputs(&mut link, options);
    link.arg("-o").arg(&proof);
    run(&mut link, "clang++")?;

    let mut ar = Command::new(&options.archiver);
    ar.arg("rcs").arg(archive).arg(&object);
    run(&mut ar, "ar")?;
    let _ = std::fs::remove_dir_all(&build);
    let _ = header;
    Ok(())
}

fn add_link_inputs(command: &mut Command, options: &BindOptions) {
    for dir in &options.library_dirs {
        command.arg("-L").arg(dir);
    }
    for library in &options.libraries {
        command.arg("-l").arg(library);
    }
}

fn materialize_projection(
    cache: &Path,
    archive: &Path,
    options: &BindOptions,
    boundary: &ForeignBoundaryContract,
) -> Result<(), BindError> {
    std::fs::create_dir_all(cache)
        .map_err(|e| BindError::IO(format!("could not create the C++ binding directory: {e}")))?;
    std::fs::copy(archive, cache.join(format!("libjet_cpp_{}.a", options.lib)))
        .map_err(|e| BindError::IO(format!("could not materialize the C++ archive: {e}")))?;
    let mut links = String::new();
    links.push_str("target\t");
    links.push_str(&options.target);
    links.push('\n');
    links.push_str("boundary\t");
    links.push_str(&boundary.digest());
    links.push('\n');
    for dir in &options.library_dirs {
        links.push_str("L\t");
        links.push_str(&dir.to_string_lossy());
        links.push('\n');
    }
    for library in &options.libraries {
        links.push_str("l\t");
        links.push_str(library);
        links.push('\n');
    }
    links.push_str("l\t");
    links.push_str(crate::FFI::cxx_runtime_for_target(&options.target));
    links.push('\n');
    std::fs::write(cache.join(format!("{}.link", options.lib)), links)
        .map_err(|e| BindError::IO(format!("could not write C++ link provenance: {e}")))
}

fn render_provenance(
    header: &Path,
    digest: &str,
    surface: &Surface,
    options: &BindOptions,
    clang_version: &[u8],
    archiver_version: &[u8],
    archive: &Path,
    link: &Path,
    overlay: &CppOverlay,
    boundary: &ForeignBoundaryContract,
) -> Result<String, String> {

    let link = link
        .canonicalize()
        .map_err(|error| format!("could not resolve C++ link provenance: {error}"))?;
    let version = flatten_tool_version;
    let mut fields: Vec<(String, String)> = vec![
        ("language".into(), "cpp".to_string()),
        ("abi".into(), format!("jet_cpp_{}", options.lib)),
        ("transport".into(), "clang-cxx-shim".to_string()),
        ("binder-schema".into(), SCHEMA.to_string()),
        ("descriptor".into(), cpp_descriptor_stamp()),
        ("source".into(), header.to_string_lossy().into_owned()),
        (
            "source-sha256".into(),
            crate::ForeignBridge::sha_file(header)?,
        ),
        ("link".into(), link.to_string_lossy().into_owned()),
        (
            "link-sha256".into(),
            crate::ForeignBridge::sha_file(&link)?,
        ),
        ("target".into(), options.target.clone()),
        ("clang".into(), options.clang.display().to_string()),
        ("archiver".into(), options.archiver.display().to_string()),
        ("clang-version".into(), version(clang_version)),
        ("archiver-version".into(), version(archiver_version)),
        ("classes".into(), surface.classes.len().to_string()),
        ("functions".into(), surface.functions.len().to_string()),
        (
            "surface".into(),
            (if surface.classes.is_empty() && surface.functions.is_empty() {
                "c-abi-implementation-carrier"
            } else {
                "cpp-facade"
            })
            .to_string(),
        ),
        ("overlay".into(), overlay.stable_identity()),
    ];
    fields.extend(boundary.provenance_fields());
    for namespace in &options.namespaces {
        fields.push(("namespace".into(), namespace.clone()));
    }
    for dir in &options.include_dirs {
        fields.push(("include".into(), dir.display().to_string()));
    }
    for dir in &options.library_dirs {
        fields.push(("library-search".into(), dir.display().to_string()));
    }
    for library in &options.libraries {
        fields.push(("library".into(), library.clone()));
    }
    for template in &options.templates {
        fields.push((
            "template".into(),
            format!(
                "{}<{}> as {}",
                template.qualified_name,
                template.cpp_args.join(","),
                template.jet_name
            ),
        ));
    }
    for input in crate::ForeignBridge::local_archive_inputs(
        &options.library_dirs,
        &options.libraries,
    ) {
        fields.push(("linked-library".into(), input.library));
        fields.push((
            "linked-archive".into(),
            input.path.to_string_lossy().into_owned(),
        ));
        fields.push((
            "linked-archive-sha256".into(),
            input
                .bytes
                .as_deref()
                .map(crate::SHA256::sha256_hex)
                .unwrap_or_else(|| "missing".into()),
        ));
    }
    let field_refs: Vec<(&str, &str)> = fields
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let artifacts = vec![(
        archive
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        crate::ForeignBridge::sha_file(archive)?,
    )];
    crate::ForeignBridge::render_provenance(digest, &field_refs, &artifacts)
}

fn render_jet(lib: &str, surface: &Surface) -> String {
    let abi = format!("jet_cpp_{lib}");
    let descriptor = cpp_descriptor_stamp();
    let mut out = format!(
        "// jet-ffi-descriptor={descriptor}\n#Import module c.{abi} {{\n    fn take_error() Int = \"{abi}_take_error\"\n"
    );
    for class in &surface.classes {
        let name = snake(&class.name);
        out.push_str(&format!("    fn {name}_new("));
        jet_params(&mut out, &class.ctor);
        out.push_str(&format!(
            ") Int = \"{abi}_{name}_new\"\n    fn {name}_close(handle: Int) = \"{abi}_{name}_close\"\n"
        ));
        for method in &class.methods {
            out.push_str(&format!("    fn {name}_{}(handle: Int", method.jet_name));
            if !method.params.is_empty() {
                out.push_str(", ");
                jet_params(&mut out, &method.params);
            }
            out.push_str(&format!(
                ") {} = \"{abi}_{name}_{}\"\n",
                method.result.jet(),
                method.jet_name
            ));
        }
    }
    for function in &surface.functions {
        out.push_str(&format!("    fn {}(", function.jet_name));
        jet_params(&mut out, &function.params);
        out.push_str(&format!(
            ") {} = \"{abi}_{}\"\n",
            function.result.jet(),
            function.jet_name
        ));
    }
    out.push_str(&format!("}}\nuse c.{abi} as abi\n\npub enum CppError {{ Exception InvalidHandle ResourceLimit }}\n\nfn cpp_error(code: Int) CppError -[]> {{ if code == 2 {{ return CppError.InvalidHandle }} if code == 3 {{ return CppError.ResourceLimit }} return CppError.Exception }}\n\n"));
    for class in &surface.classes {
        let name = snake(&class.name);
        out.push_str(&format!(
            "#SingleUse\npub struct {} {{ value: Int }}\n\npub fn new_{name}(",
            class.name
        ));
        jet_params(&mut out, &class.ctor);
        out.push_str(&format!(
            ") {} !CppError -[FFI.Cpp]> {{\n    value :: abi.{name}_new(",
            class.name
        ));
        jet_args(&mut out, &class.ctor);
        out.push_str(&format!(
            ")\n    code :: abi.take_error()\n    if code != 0 {{ return Err(cpp_error(code)) }}\n    return Ok({}.{{ value: value }})\n}}\n\n",
            class.name
        ));
        if !class.methods.is_empty() {
            out.push_str(&format!("impl {} {{\n", class.name));
        }
        for method in &class.methods {
            out.push_str(&format!("    pub fn {}(self", method.jet_name));
            if !method.params.is_empty() {
                out.push_str(", ");
                jet_params(&mut out, &method.params);
            }
            out.push_str(&format!(
                ") {} !CppError -[FFI.Cpp]> {{\n        result_value :: abi.{name}_{}(self.value",
                method.result.jet(),
                method.jet_name
            ));
            if !method.params.is_empty() {
                out.push_str(", ");
                jet_args(&mut out, &method.params);
            }
            out.push_str(")\n        code :: abi.take_error()\n        if code != 0 { return Err(cpp_error(code)) }\n        return Ok(result_value)\n    }\n");
        }
        if !class.methods.is_empty() {
            out.push_str("}\n\n");
        }
        out.push_str(&format!("pub fn close_{name}(value: ^{}) -[FFI.Cpp]> {{\n    abi.{name}_close(value.value)\n    if abi.take_error() != 0 {{ panic(\"C++ handle close failed\") }}\n}}\n\n", class.name));
    }
    for function in &surface.functions {
        out.push_str(&format!("pub fn {}(", function.jet_name));
        jet_params(&mut out, &function.params);
        out.push_str(&format!(
            ") {} !CppError -[FFI.Cpp]> {{\n    result_value :: abi.{}(",
            function.result.jet(),
            function.jet_name
        ));
        jet_args(&mut out, &function.params);
        out.push_str(")\n    code :: abi.take_error()\n    if code != 0 { return Err(cpp_error(code)) }\n    return Ok(result_value)\n}\n\n");
    }
    out
}

fn cpp_descriptor_stamp() -> String {
    crate::AST::binder_descriptor(crate::AST::ForeignLanguage::Cpp)
        .expect("C++ binder descriptor is not registered")
        .stamp()
}

fn jet_params(out: &mut String, params: &[Param]) {
    for (index, param) in params.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&param.name);
        out.push_str(": ");
        out.push_str(param.kind.jet());
    }
}

fn jet_args(out: &mut String, params: &[Param]) {
    for (index, param) in params.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&param.name);
    }
}

fn render_cpp(header: &Path, lib: &str, surface: &Surface) -> String {
    let abi = format!("jet_cpp_{lib}");
    let mut out = format!("#include <array>\n#include <cstdint>\n#include <cstdlib>\n#include <exception>\n#include <mutex>\n#include \"{}\"\nstatic thread_local int64_t jet_cpp_error;\nextern \"C\" int64_t {abi}_take_error(){{auto value=jet_cpp_error;jet_cpp_error=0;return value;}}\n", header.display());
    for class in &surface.classes {
        let name = snake(&class.name);
        out.push_str(&format!(
            "static std::array<{}*,64> {name}_slots{{}}; static std::mutex {name}_lock;\n",
            class.cpp_name
        ));
        out.push_str(&format!("extern \"C\" int64_t {abi}_{name}_new("));
        cpp_params(&mut out, &class.ctor);
        out.push_str("){jet_cpp_error=0;try{auto* value=new ");
        out.push_str(&class.cpp_name);
        out.push('(');
        cpp_args(&mut out, &class.ctor);
        out.push_str(&format!(");std::lock_guard<std::mutex> guard({name}_lock);for(size_t i=0;i<{name}_slots.size();++i)if(!{name}_slots[i]){{{name}_slots[i]=value;return i+1;}}delete value;jet_cpp_error=3;return 0;}}catch(...){{jet_cpp_error=1;return 0;}}}}\n"));
        out.push_str(&format!("static {}* {name}_get(int64_t handle){{if(handle<1||handle>64){{jet_cpp_error=2;return nullptr;}}std::lock_guard<std::mutex> guard({name}_lock);auto* value={name}_slots[handle-1];if(!value)jet_cpp_error=2;return value;}}\n", class.cpp_name));
        out.push_str(&format!("extern \"C\" void {abi}_{name}_close(int64_t handle){{jet_cpp_error=0;try{{if(handle<1||handle>64){{jet_cpp_error=2;return;}}{}* value=nullptr;{{std::lock_guard<std::mutex> guard({name}_lock);value={name}_slots[handle-1];{name}_slots[handle-1]=nullptr;}}if(!value){{jet_cpp_error=2;return;}}delete value;}}catch(...){{jet_cpp_error=1;}}}}\n", class.cpp_name));
        for method in &class.methods {
            out.push_str(&format!(
                "extern \"C\" {} {abi}_{name}_{}(int64_t handle",
                method.result.c(),
                method.jet_name
            ));
            if !method.params.is_empty() {
                out.push_str(", ");
                cpp_params(&mut out, &method.params);
            }
            out.push_str("){jet_cpp_error=0;auto* self=");
            out.push_str(&format!(
                "{name}_get(handle);if(!self)return {};try{{return self->{}(",
                method.result.zero(),
                method.cpp_name
            ));
            cpp_args(&mut out, &method.params);
            out.push_str(&format!(
                ");}}catch(...){{jet_cpp_error=1;return {};}}}}\n",
                method.result.zero()
            ));
        }
    }
    for function in &surface.functions {
        out.push_str(&format!(
            "extern \"C\" {} {abi}_{}(",
            function.result.c(),
            function.jet_name
        ));
        cpp_params(&mut out, &function.params);
        out.push_str("){jet_cpp_error=0;try{return ");
        out.push_str(&function.cpp_name);
        out.push('(');
        cpp_args(&mut out, &function.params);
        out.push_str(&format!(
            ");}}catch(...){{jet_cpp_error=1;return {};}}}}\n",
            function.result.zero()
        ));
    }
    out
}

fn cpp_params(out: &mut String, params: &[Param]) {
    if params.is_empty() {
        out.push_str("void");
        return;
    }
    for (index, param) in params.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        if param.kind == Scalar::Callback {
            out.push_str("int64_t (*");
            out.push_str(&param.name);
            out.push_str(")(int64_t)");
        } else {
            out.push_str(param.kind.c());
            out.push(' ');
            out.push_str(&param.name);
        }
    }
}

fn cpp_args(out: &mut String, params: &[Param]) {
    for (index, param) in params.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&param.name);
    }
}

struct Output {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn supervised(command: &mut Command, timeout: Duration, tool: &str) -> Result<Output, BindError> {
    const CAP: usize = 64 * 1024 * 1024;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            BindError::ToolMissing(tool.into())
        } else {
            BindError::IO(format!("could not start `{tool}`: {error}"))
        }
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| BindError::IO("could not supervise stdout".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| BindError::IO("could not supervise stderr".into()))?;
    let out = std::thread::spawn(move || drain(stdout, CAP));
    let err = std::thread::spawn(move || drain(stderr, CAP));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child
            .try_wait()
            .map_err(|error| BindError::IO(format!("could not supervise `{tool}`: {error}")))?
        {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(BindError::ToolFailed(format!(
                    "`{tool}` exceeded its time limit"
                )));
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    Ok(Output {
        status,
        stdout: out
            .join()
            .map_err(|_| BindError::IO("stdout reader failed".into()))??,
        stderr: err
            .join()
            .map_err(|_| BindError::IO("stderr reader failed".into()))??,
    })
}

fn run(command: &mut Command, tool: &str) -> Result<(), BindError> {
    let output = supervised(command, Duration::from_secs(60), tool)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(BindError::ToolFailed(format!(
            "{tool}: {}",
            launder(&output.stderr)
        )))
    }
}

fn flatten_tool_version(bytes: &[u8]) -> String {
    let flat = String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" | ");
    if flat.is_empty() {
        "unknown".to_string()
    } else {
        flat
    }
}

fn tool_version(path: &Path, tool: &str) -> Result<Vec<u8>, BindError> {
    let output = supervised(
        Command::new(path).arg("--version"),
        Duration::from_secs(10),
        tool,
    )?;
    if !output.status.success() {
        return Err(BindError::ToolFailed(launder(&output.stderr)));
    }
    let mut identity = output.stdout;
    identity.extend_from_slice(&output.stderr);
    Ok(identity)
}

fn drain(mut input: impl Read, limit: usize) -> Result<Vec<u8>, BindError> {
    let mut out = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|error| BindError::IO(format!("could not read foreign output: {error}")))?;
        if count == 0 {
            break;
        }
        let keep = (limit - out.len()).min(count);
        out.extend_from_slice(&buffer[..keep]);
    }
    Ok(out)
}

fn launder(value: &[u8]) -> String {
    let text = String::from_utf8_lossy(value);
    text.lines()
        .map(str::trim)
        .find(|line| line.contains("error:"))
        .or_else(|| text.lines().map(str::trim).find(|line| !line.is_empty()))
        .map(|line| line.chars().take(240).collect())
        .unwrap_or_else(|| "the tool returned a failure status".into())
}

fn ident(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(ch) if ch.is_ascii_alphabetic() || ch == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn qualified_ident(value: &str) -> bool {
    value.split("::").all(ident)
}

pub(crate) fn snake(value: &str) -> String {
    let mut out = String::new();
    for (index, ch) in value.chars().enumerate() {
        if ch.is_ascii_uppercase() && index > 0 {
            out.push('_');
        }
        out.push(ch.to_ascii_lowercase());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str) -> PathBuf {
        std::env::var_os("PATH")
            .into_iter()
            .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
            .map(|dir| dir.join(name))
            .find(|path| path.is_file())
            .and_then(|path| std::fs::canonicalize(path).ok())
            .unwrap_or_else(|| panic!("{name} is required for the C++ binder test"))
    }

    #[test]
    fn bind_emits_canonical_descriptor_effect_and_provenance() {
        let root = std::env::temp_dir().join(format!("jet_cpp_bind_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let header = root.join("math.hpp");
        std::fs::write(
            &header,
            "#pragma once\n#include <cstdint>\ninline int64_t add(int64_t value) { return value + 1; }\n",
        )
        .unwrap();
        let options = BindOptions {
            lib: "math".into(),
            target: crate::FFI::host_target(),
            clang: tool("clang++"),
            archiver: tool("ar"),
            include_dirs: vec![root.clone()],
            library_dirs: vec![],
            libraries: vec![],
            namespaces: vec![],
            templates: vec![],
        };
        let cache = root.join(".jet/bindings/cpp");
        let result = bind(&header, &cache, &options).unwrap();
        assert!(result.archive.is_file());
        assert!(result
            .source
            .contains("// jet-ffi-descriptor=jet-ffi-descriptor-v1;"));
        assert!(result.source.contains("fn add(value: Int) Int ="));
        assert!(result.source.contains("Int !CppError -[FFI.Cpp]>"));
        assert!(!result.source.contains("=>"));
        assert!(result
            .provenance
            .contains("descriptor=jet-ffi-descriptor-v1;"));
        let _ = std::fs::remove_dir_all(root);
    }
}
