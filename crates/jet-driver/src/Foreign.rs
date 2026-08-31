//! D-FFI-UNIFY1 shared binder registry.
//!
//! This is the narrow dispatch spine all foreign-language binders hang from.
//! C, Python, JS, and Octave are active namespace binders; rust/swift are registered ratified
//! mounts so later cards add binder depth without inventing a second namespace
//! model. The shipped S50 `extern rust` block stays on its existing path until
//! the `rust.*` binder migrates it under D-FFI-UNIFY1.

use crate::Diagnostics::Diagnostic;
use crate::Syntax;
pub use crate::AST::{
    BinderCapability, BinderCapabilityReport, BinderDescriptor, BinderRuntime, BinderStatus,
    BinderSurface, BindingStubKind, ForeignProvider, ForeignSafety, ForeignScalar, ForeignStubFile,
    FOREIGN_BINDERS as BINDERS,
};
use crate::AST::{
    ForeignAbiContract, ForeignLanguage, ForeignNamespace, ImportDecl, Item, LoadedModule,
    ProgramBundle,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

fn all_imports(module: &LoadedModule) -> impl Iterator<Item = &ImportDecl> {
    let mut seen = HashSet::new();
    crate::AST::walk_imports(module)
        .into_iter()
        .filter_map(move |(_, import)| seen.insert(import.span).then_some(import))
}

pub fn binder_for(language: ForeignLanguage) -> Option<&'static BinderDescriptor> {
    crate::AST::binder_descriptor(language)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignTarget {
    Native,
    Web,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignHost {
    DirectCAbi,
    ClangCppShim,
    BrowserJsEngine,
    NativeJsWasmComponent,
    SupervisedPythonSidecar,
    SwiftCAbiBridge,
    GoCArchive,
    EmbeddedJvm,
    EmbeddedDotNet,
    EmbeddedTcl,
    EmbeddedLua,
    FortranIsoCBinding,
    GnuCobolCAbi,
    AdaGnatCAbi,
    FreePascalCdecl,
    DartHostFfi,
    SupervisedPowerShell,
    SupervisedPerl,
    SupervisedRuby,
    SupervisedPhpPool,
    SupervisedR,
    SupervisedOctave,
    WindowsComAutomation,
    LegacyRustExtern,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignRoutePlan {
    pub namespace: ForeignNamespace,
    pub descriptor: BinderDescriptor,
    pub host: ForeignHost,
    pub binding_cache: PathBuf,
    pub type_stub: Option<PathBuf>,
    pub provenance: PathBuf,
    pub abi_contract: ForeignAbiContract,
}

pub fn binding_cache_dir(project_root: &Path, language: ForeignLanguage) -> PathBuf {
    project_root
        .join(Syntax::SOURCE_ROOT_DIR)
        .join(language.bindings_subdir())
}

pub fn binding_cache_file(project_root: &Path, language: ForeignLanguage, lib: &str) -> PathBuf {
    let extension = binder_for(language)
        .map(|descriptor| descriptor.cache_extension)
        .unwrap_or(Syntax::FILE_EXT);
    binding_cache_dir(project_root, language).join(format!("{lib}.{extension}"))
}

pub fn type_stub_file(
    project_root: &Path,
    language: ForeignLanguage,
    lib: &str,
) -> Option<PathBuf> {
    let descriptor = binder_for(language)?;
    match descriptor.type_stub_file {
        ForeignStubFile::None => None,
        ForeignStubFile::Suffix(extension) => {
            Some(binding_cache_dir(project_root, language).join(format!("{lib}.{extension}")))
        }
        ForeignStubFile::StemSuffix(suffix) => {
            Some(binding_cache_dir(project_root, language).join(format!("{lib}{suffix}")))
        }
    }
}

pub fn provenance_file(project_root: &Path, language: ForeignLanguage, lib: &str) -> PathBuf {
    binding_cache_dir(project_root, language).join(format!("{lib}.provenance"))
}

pub fn host_for(language: ForeignLanguage, target: ForeignTarget) -> ForeignHost {
    match language {
        ForeignLanguage::C => ForeignHost::DirectCAbi,
        ForeignLanguage::Cpp => ForeignHost::ClangCppShim,
        ForeignLanguage::Rust => ForeignHost::LegacyRustExtern,
        ForeignLanguage::Py => ForeignHost::SupervisedPythonSidecar,
        ForeignLanguage::JS => match target {
            ForeignTarget::Native => ForeignHost::NativeJsWasmComponent,
            ForeignTarget::Web => ForeignHost::BrowserJsEngine,
        },
        ForeignLanguage::Swift => ForeignHost::SwiftCAbiBridge,
        ForeignLanguage::Go => ForeignHost::GoCArchive,
        ForeignLanguage::Java => ForeignHost::EmbeddedJvm,
        ForeignLanguage::DotNet => ForeignHost::EmbeddedDotNet,
        ForeignLanguage::Tcl => ForeignHost::EmbeddedTcl,
        ForeignLanguage::Lua => ForeignHost::EmbeddedLua,
        ForeignLanguage::Fortran => ForeignHost::FortranIsoCBinding,
        ForeignLanguage::Cobol => ForeignHost::GnuCobolCAbi,
        ForeignLanguage::Ada => ForeignHost::AdaGnatCAbi,
        ForeignLanguage::Pascal => ForeignHost::FreePascalCdecl,
        ForeignLanguage::Dart => ForeignHost::DartHostFfi,
        ForeignLanguage::PowerShell => ForeignHost::SupervisedPowerShell,
        ForeignLanguage::Perl => ForeignHost::SupervisedPerl,
        ForeignLanguage::Ruby => ForeignHost::SupervisedRuby,
        ForeignLanguage::Php => ForeignHost::SupervisedPhpPool,
        ForeignLanguage::R => ForeignHost::SupervisedR,
        ForeignLanguage::Octave => ForeignHost::SupervisedOctave,
        ForeignLanguage::Com => ForeignHost::WindowsComAutomation,
    }
}

pub fn route_plan(
    project_root: &Path,
    namespace: ForeignNamespace,
    target: ForeignTarget,
) -> Option<ForeignRoutePlan> {
    let descriptor = *binder_for(namespace.language)?;
    Some(ForeignRoutePlan {
        host: host_for(namespace.language, target),
        binding_cache: binding_cache_file(project_root, namespace.language, &namespace.lib),
        type_stub: type_stub_file(project_root, namespace.language, &namespace.lib),
        provenance: provenance_file(project_root, namespace.language, &namespace.lib),
        descriptor,
        namespace,
        abi_contract: descriptor.contract,
    })
}

/// Capability report used by inspection and package/provider integrations.
/// Keeping it derived from the route descriptor makes cache provenance and
/// runtime dispatch agree on one ABI version.
pub fn capability_report(language: ForeignLanguage) -> Option<ForeignAbiContract> {
    binder_for(language).map(|descriptor| descriptor.contract)
}

/// Stable inspection output derived directly from the canonical binder table.
/// It reports structure and support claims, not host-tool presence: a missing
/// provisioned tool is a binding error, never a false capability downgrade.
pub fn capability_report_text() -> String {
    use std::fmt::Write as _;

    let mut output = String::from(
        "foreign capabilities\nschema=jet-ffi-capability-report-v1\nlanguage status runtime stub contract effect provider capabilities\n",
    );
    for descriptor in BINDERS {
        let report = descriptor.capability_report();
        let capabilities = report
            .capabilities
            .iter()
            .map(|capability| format!("{capability:?}"))
            .collect::<Vec<_>>()
            .join(",");
        let _ = writeln!(
            output,
            "{} {:?} {:?} {:?} {} {} {:?} {}",
            report.language.root(),
            report.status,
            descriptor.runtime,
            descriptor.stub_kind,
            contract_name(report.contract),
            report.effect_root,
            report.provider,
            capabilities,
        );
    }
    output
}

pub fn capability_report_json() -> String {
    let rows = BINDERS
        .iter()
        .map(|descriptor| {
            let report = descriptor.capability_report();
            let capabilities = report
                .capabilities
                .iter()
                .map(|capability| json_quote(&format!("{capability:?}")))
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{\"language\":{},\"status\":{},\"runtime\":{},\"stub\":{},\"contract\":{},\"effect\":{},\"provider\":{},\"capabilities\":[{}]}}",
                json_quote(report.language.root()),
                json_quote(&format!("{:?}", report.status)),
                json_quote(&format!("{:?}", descriptor.runtime)),
                json_quote(&format!("{:?}", descriptor.stub_kind)),
                json_quote(contract_name(report.contract)),
                json_quote(report.effect_root),
                json_quote(&format!("{:?}", report.provider)),
                capabilities,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let payload = format!("{{\"languages\":[{rows}]}}");
    jet_foundation::Report::render_status_json(
        "ok",
        true,
        "inspect.ffi",
        &format!(",\"ffi\":{payload}"),
    )
}

fn contract_name(contract: ForeignAbiContract) -> &'static str {
    if contract == ForeignAbiContract::C {
        "C"
    } else if contract == ForeignAbiContract::CXX {
        "CXX"
    } else if contract == ForeignAbiContract::NATIVE {
        "NATIVE"
    } else {
        "MESSAGE"
    }
}

fn json_quote(value: &str) -> String {
    let mut output = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

pub fn is_active_namespace_import(
    imp: &ImportDecl,
) -> Result<bool, crate::AST::ForeignImportError> {
    imp.foreign_imports().map(|imports| {
        imports.into_iter().any(|(ns, _)| {
            binder_for(ns.language)
                .map(|binder| {
                    binder.surface == BinderSurface::Namespace
                        && binder.status == BinderStatus::Active
                })
                .unwrap_or(false)
        })
    })
}

/// D-FFI-UNIFY1 / D-FFI-JS1: active non-C namespace imports are backed by
/// generated binding modules under `.jet/bindings/<lang>/<lib>.jet`.
///
/// C keeps its richer merge/link path in `CFFI::assemble`; this pass handles
/// active binders whose cache is already plain Jet source. Missing caches still
/// get an empty synthetic module so unused imports type-check and real symbol
/// use fails as a normal missing member until `jet inspect bind <lang>` materializes it.
#[derive(Debug, Clone)]
pub struct ForeignDiagnostic {
    pub file: String,
    pub source: String,
    pub diagnostic: Diagnostic,
}

pub fn assemble_active_namespaces(bundle: &mut ProgramBundle) -> Result<(), Vec<Diagnostic>> {
    assemble_active_namespaces_with_provenance(bundle).map_err(|diagnostics| {
        diagnostics
            .into_iter()
            .map(|entry| entry.diagnostic)
            .collect()
    })
}

pub fn assemble_active_namespaces_with_provenance(
    bundle: &mut ProgramBundle,
) -> Result<(), Vec<ForeignDiagnostic>> {
    let mut surfaces: HashMap<(ForeignLanguage, String), usize> = HashMap::new();
    let user_module_count = bundle.modules.len();

    for idx in 0..user_module_count {
        let imports: Vec<_> = all_imports(&bundle.modules[idx]).cloned().collect();
        for imp in &imports {
            let foreign = match imp.foreign_imports() {
                Ok(foreign) => foreign,
                Err(error) => {
                    return Err(vec![ForeignDiagnostic {
                        file: bundle.modules[idx].display.clone(),
                        source: bundle.modules[idx].source.clone(),
                        diagnostic: error.diagnostic(),
                    }]);
                }
            };
            for (ns, _) in foreign {
                if ns.language == ForeignLanguage::C {
                    continue;
                }
                let Some(descriptor) = binder_for(ns.language) else {
                    continue;
                };
                if descriptor.surface != BinderSurface::Namespace
                    || descriptor.status != BinderStatus::Active
                {
                    continue;
                }
                if ns.language == ForeignLanguage::Com && !cfg!(target_os = "windows") {
                    return Err(vec![ForeignDiagnostic {
                        file: bundle.modules[idx].display.clone(),
                        source: bundle.modules[idx].source.clone(),
                        diagnostic: Diagnostic::error(
                            "E3260",
                            "`com.*` needs a Windows host".to_string(),
                            "COM automation depends on Windows apartments, the registry, and IDispatch"
                                .to_string(),
                            "build and run this module on a Windows host; use a non-COM boundary for other targets"
                                .to_string(),
                            Some(imp.span),
                        ),
                    }]);
                }

                let key = (ns.language, ns.lib.clone());
                let target_idx = if let Some(idx) = surfaces.get(&key).copied() {
                    idx
                } else {
                    let idx = match materialize_namespace(bundle, ns.language, &ns.lib) {
                        Ok(idx) => idx,
                        Err(diagnostics) => {
                            let path =
                                binding_cache_file(&bundle.project_root, ns.language, &ns.lib);
                            let source = std::fs::read_to_string(&path).unwrap_or_default();
                            return Err(diagnostics
                                .into_iter()
                                .map(|diagnostic| ForeignDiagnostic {
                                    file: path.display().to_string(),
                                    source: source.clone(),
                                    diagnostic,
                                })
                                .collect());
                        }
                    };
                    surfaces.insert(key, idx);
                    idx
                };
                bundle
                    .name_ledger
                    .record_import_target(idx, imp.span, target_idx);
            }
        }
    }
    Ok(())
}

fn materialize_namespace(
    bundle: &mut ProgramBundle,
    language: ForeignLanguage,
    lib: &str,
) -> Result<usize, Vec<Diagnostic>> {
    let cache_path = binding_cache_file(&bundle.project_root, language, lib);
    let alias = synthetic_alias(language, lib);
    let Some(descriptor) = binder_for(language) else {
        return Err(vec![Diagnostic::error(
            "E3208",
            format!(
                "no foreign binder descriptor is registered for `{}`",
                language.root()
            ),
            "generated foreign stubs require a registered typed ABI descriptor".to_string(),
            "register the language binder before importing its namespace".to_string(),
            None,
        )]);
    };
    let module_idx = bundle.modules.len();

    if cache_path.is_file() {
        let source = match std::fs::read_to_string(&cache_path) {
            Ok(source) => source,
            Err(_) => String::new(),
        };
        if !source.is_empty() {
            if matches!(language, ForeignLanguage::Cobol | ForeignLanguage::Com)
                && descriptor_stamp(&source).is_none()
            {
                return Err(vec![Diagnostic::error(
                    "E3208",
                    format!(
                        "generated `{}` binding has no ABI descriptor",
                        language.root()
                    ),
                    format!(
                        "the generated stub must record the checked {} ABI contract",
                        language.root()
                    ),
                    format!(
                        "regenerate the binding with `jet inspect bind {}`",
                        language.root()
                    ),
                    None,
                )]);
            }
            if let Some(actual) = descriptor_stamp(&source) {
                if actual != descriptor.stamp() {
                    return Err(vec![Diagnostic::error(
                        "E3208",
                        format!(
                            "generated `{}` binding has a stale descriptor",
                            language.root()
                        ),
                        "the generated stub and binder must use the same foreign ABI descriptor"
                            .to_string(),
                        "regenerate the binding with `jet inspect bind`".to_string(),
                        None,
                    )]);
                }
            }
            let (tokens, lex_diags) = crate::Lexer::lex_generated(&source);
            if !lex_diags.is_empty() {
                return Err(lex_diags);
            }
            let mut program = crate::Parser::parse(&tokens)?;
            if language == ForeignLanguage::Cpp {
                mark_cpp_callback_abi(&mut program.items);
            }
            bundle.modules.push(LoadedModule {
                path: cache_path,
                display: format!("{}.{}", language.root(), lib),
                source,
                alias,
                imports: std::mem::take(&mut program.imports),
                items: std::mem::take(&mut program.items),
                script_body: std::mem::take(&mut program.script_body),
                block_spans: std::mem::take(&mut program.block_spans),
                web_target_ceiling: program.web_target_ceiling,
                pub_file: program.pub_file,
                no_prelude: program.no_prelude,
                default_target: program.default_target,
                html_path: program.html_path.clone(),
                policy_declarations: program.policy_declarations.clone(),
                user_policy_declarations: program.user_policy_declarations.clone(),
                rule_facts: std::mem::take(&mut program.rule_facts),
            });
            return Ok(module_idx);
        }
    }

    Err(vec![Diagnostic::error(
        "E3208",
        format!(
            "foreign binding cache for `{}.{}` is missing or empty",
            language.root(),
            lib
        ),
        "an active foreign namespace needs a generated typed binding before use".to_string(),
        "run the language binder or realize the foreign package before importing it".to_string(),
        None,
    )])
}

fn mark_cpp_callback_abi(items: &mut [Item]) {
    for item in items {
        let Item::Func(function) = item else { continue };
        for param in &mut function.params {
            if matches!(param.ty, crate::AST::Type::Fn { .. }) {
                param.ty = crate::AST::Type::Tagged {
                    marker: crate::AST::TagMarker::Internal(
                        crate::AST::InternalTag::CppCallbackAbi,
                    ),
                    inner: Box::new(param.ty.clone()),
                };
            }
        }
    }
}

fn synthetic_alias(language: ForeignLanguage, lib: &str) -> String {
    format!("__{}_{}", language.root(), lib)
}

fn descriptor_stamp(source: &str) -> Option<&str> {
    source
        .lines()
        .find_map(|line| line.strip_prefix("// jet-ffi-descriptor="))
}
