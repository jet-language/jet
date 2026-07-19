//! D-FFI-UNIFY1 shared binder registry.
//!
//! This is the narrow dispatch spine all foreign-language binders hang from.
//! C and JS are active namespace binders; rust/py/swift are registered ratified
//! mounts so later cards add binder depth without inventing a second namespace
//! model. The shipped S50 `extern rust` block stays on its existing path until
//! the `rust.*` binder migrates it under D-FFI-UNIFY1.

use crate::Diagnostics::Diagnostic;
use crate::Syntax;
use crate::AST::{ForeignLanguage, ForeignNamespace, ImportDecl, Item, LoadedModule, ProgramBundle};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinderSurface {
    /// `use <lang>.<lib>` / `@Extern module <lang>.<lib>` / generated cache.
    Namespace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinderStatus {
    Active,
    Planned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinderRuntime {
    DirectCAbi,
    ClangCppShim,
    LegacyRustExtern,
    SupervisedPythonSidecar,
    TargetDispatchedJs,
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
    DartApiDl,
    SupervisedPowerShell,
    SupervisedPerl,
    SupervisedRuby,
    SupervisedPhpPool,
    SupervisedR,
    WindowsComAutomation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingStubKind {
    CHeader,
    CppHeader,
    RustExternBlock,
    PythonIntrospection,
    TypeScriptDeclarations,
    SwiftModule,
    GoExports,
    JvmClass,
    DotNetAssembly,
    TclScript,
    LuaScript,
    FortranIsoCBinding,
    GnuCobolCopybook,
    AdaSpec,
    PascalSource,
    DartContract,
    PowerShellScript,
    PerlScript,
    RubyScript,
    PhpScript,
    RScript,
    ComTypeLibrary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BinderDescriptor {
    pub language: ForeignLanguage,
    pub surface: BinderSurface,
    pub status: BinderStatus,
    pub runtime: BinderRuntime,
    pub stub_kind: BindingStubKind,
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
}

pub const BINDERS: &[BinderDescriptor] = &[
    BinderDescriptor {
        language: ForeignLanguage::C,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Active,
        runtime: BinderRuntime::DirectCAbi,
        stub_kind: BindingStubKind::CHeader,
    },
    BinderDescriptor {
        language: ForeignLanguage::Cpp,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Active,
        runtime: BinderRuntime::ClangCppShim,
        stub_kind: BindingStubKind::CppHeader,
    },
    BinderDescriptor {
        language: ForeignLanguage::Rust,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Planned,
        runtime: BinderRuntime::LegacyRustExtern,
        stub_kind: BindingStubKind::RustExternBlock,
    },
    BinderDescriptor {
        language: ForeignLanguage::Py,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Planned,
        runtime: BinderRuntime::SupervisedPythonSidecar,
        stub_kind: BindingStubKind::PythonIntrospection,
    },
    BinderDescriptor {
        language: ForeignLanguage::Js,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Active,
        runtime: BinderRuntime::TargetDispatchedJs,
        stub_kind: BindingStubKind::TypeScriptDeclarations,
    },
    BinderDescriptor {
        language: ForeignLanguage::Swift,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Planned,
        runtime: BinderRuntime::SwiftCAbiBridge,
        stub_kind: BindingStubKind::SwiftModule,
    },
    BinderDescriptor {
        language: ForeignLanguage::Go,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Active,
        runtime: BinderRuntime::GoCArchive,
        stub_kind: BindingStubKind::GoExports,
    },
    BinderDescriptor {
        language: ForeignLanguage::Java,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Active,
        runtime: BinderRuntime::EmbeddedJvm,
        stub_kind: BindingStubKind::JvmClass,
    },
    BinderDescriptor {
        language: ForeignLanguage::DotNet,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Active,
        runtime: BinderRuntime::EmbeddedDotNet,
        stub_kind: BindingStubKind::DotNetAssembly,
    },
    BinderDescriptor {
        language: ForeignLanguage::Tcl,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Active,
        runtime: BinderRuntime::EmbeddedTcl,
        stub_kind: BindingStubKind::TclScript,
    },
    BinderDescriptor {
        language: ForeignLanguage::Lua,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Active,
        runtime: BinderRuntime::EmbeddedLua,
        stub_kind: BindingStubKind::LuaScript,
    },
    BinderDescriptor {
        language: ForeignLanguage::Fortran,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Active,
        runtime: BinderRuntime::FortranIsoCBinding,
        stub_kind: BindingStubKind::FortranIsoCBinding,
    },
    BinderDescriptor {
        language: ForeignLanguage::Cobol,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Active,
        runtime: BinderRuntime::GnuCobolCAbi,
        stub_kind: BindingStubKind::GnuCobolCopybook,
    },
    BinderDescriptor {
        language: ForeignLanguage::Ada,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Active,
        runtime: BinderRuntime::AdaGnatCAbi,
        stub_kind: BindingStubKind::AdaSpec,
    },
    BinderDescriptor {
        language: ForeignLanguage::Pascal,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Active,
        runtime: BinderRuntime::FreePascalCdecl,
        stub_kind: BindingStubKind::PascalSource,
    },
    BinderDescriptor {
        language: ForeignLanguage::Dart,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Active,
        runtime: BinderRuntime::DartApiDl,
        stub_kind: BindingStubKind::DartContract,
    },
    BinderDescriptor {
        language: ForeignLanguage::PowerShell,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Active,
        runtime: BinderRuntime::SupervisedPowerShell,
        stub_kind: BindingStubKind::PowerShellScript,
    },
    BinderDescriptor {
        language: ForeignLanguage::Perl,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Active,
        runtime: BinderRuntime::SupervisedPerl,
        stub_kind: BindingStubKind::PerlScript,
    },
    BinderDescriptor {
        language: ForeignLanguage::Ruby,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Active,
        runtime: BinderRuntime::SupervisedRuby,
        stub_kind: BindingStubKind::RubyScript,
    },
    BinderDescriptor {
        language: ForeignLanguage::Php,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Active,
        runtime: BinderRuntime::SupervisedPhpPool,
        stub_kind: BindingStubKind::PhpScript,
    },
    BinderDescriptor {
        language: ForeignLanguage::R,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Active,
        runtime: BinderRuntime::SupervisedR,
        stub_kind: BindingStubKind::RScript,
    },
    BinderDescriptor {
        language: ForeignLanguage::Com,
        surface: BinderSurface::Namespace,
        status: BinderStatus::Active,
        runtime: BinderRuntime::WindowsComAutomation,
        stub_kind: BindingStubKind::ComTypeLibrary,
    },
];

pub fn binder_for(language: ForeignLanguage) -> Option<&'static BinderDescriptor> {
    BINDERS.iter().find(|b| b.language == language)
}

pub fn binding_cache_dir(project_root: &Path, language: ForeignLanguage) -> PathBuf {
    project_root
        .join(Syntax::SOURCE_ROOT_DIR)
        .join(language.bindings_subdir())
}

pub fn binding_cache_file(project_root: &Path, language: ForeignLanguage, lib: &str) -> PathBuf {
    binding_cache_dir(project_root, language).join(format!("{}.{}", lib, Syntax::FILE_EXT))
}

pub fn type_stub_file(
    project_root: &Path,
    language: ForeignLanguage,
    lib: &str,
) -> Option<PathBuf> {
    if language == ForeignLanguage::Dart {
        return Some(binding_cache_dir(project_root,language).join(format!("{lib}_host.dart")));
    }
    let ext = match language {
        ForeignLanguage::Js => "d.ts",
        _ => return None,
    };
    Some(binding_cache_dir(project_root, language).join(format!("{lib}.{ext}")))
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
        ForeignLanguage::Js => match target {
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
    })
}

pub fn is_active_namespace_import(imp: &ImportDecl) -> bool {
    let Some(ns) = imp.foreign_namespace() else {
        return false;
    };
    binder_for(ns.language)
        .map(|binder| {
            binder.surface == BinderSurface::Namespace && binder.status == BinderStatus::Active
        })
        .unwrap_or(false)
}

/// D-FFI-UNIFY1 / D-FFI-JS1: active non-C namespace imports are backed by
/// generated binding modules under `.jet/bindings/<lang>/<lib>.jet`.
///
/// C keeps its richer merge/link path in `CFFI::assemble`; this pass handles
/// active binders whose cache is already plain Jet source. Missing caches still
/// get an empty synthetic module so unused imports type-check and real symbol
/// use fails as a normal missing member until `jet inspect bind <lang>` materializes it.
pub fn assemble_active_namespaces(bundle: &mut ProgramBundle) -> Result<(), Vec<Diagnostic>> {
    let mut surfaces: HashMap<(ForeignLanguage, String), usize> = HashMap::new();
    let user_module_count = bundle.modules.len();

    for idx in 0..user_module_count {
        let imports = bundle.modules[idx].imports.clone();
        for imp in &imports {
            let Some(ns) = imp.foreign_namespace() else {
                continue;
            };
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
                return Err(vec![Diagnostic::error(
                    "E3260",
                    "`com.*` needs a Windows host".to_string(),
                    "COM automation depends on Windows apartments, the registry, and IDispatch"
                        .to_string(),
                    "build and run this module on a Windows host; use a non-COM boundary for other targets"
                        .to_string(),
                    Some(imp.span),
                )]);
            }

            let key = (ns.language, ns.lib.clone());
            let target_idx = if let Some(idx) = surfaces.get(&key).copied() {
                idx
            } else {
                let idx = materialize_namespace(bundle, ns.language, &ns.lib)?;
                surfaces.insert(key, idx);
                idx
            };
            bundle.import_targets.insert((idx, imp.span), target_idx);
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
    let module_idx = bundle.modules.len();

    if cache_path.is_file() {
        let source = match std::fs::read_to_string(&cache_path) {
            Ok(source) => source,
            Err(_) => String::new(),
        };
        if !source.is_empty() {
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
                block_spans: std::mem::take(&mut program.block_spans),
                web_target_ceiling: program.web_target_ceiling,
                pub_file: program.pub_file,
                no_prelude: program.no_prelude,
                html_path: program.html_path.clone(),
                no_alloc_policy: program.no_alloc_policy,
                policy_declarations: program.policy_declarations.clone(),
            });
            return Ok(module_idx);
        }
    }

    bundle.modules.push(LoadedModule {
        path: PathBuf::from(format!("<{}.{}>", language.root(), lib)),
        display: format!("{}.{}", language.root(), lib),
        source: String::new(),
        alias,
        imports: Vec::new(),
        items: Vec::new(),
        block_spans: Vec::new(),
        web_target_ceiling: None,
        pub_file: false,
        no_prelude: false,
        html_path: None,
        no_alloc_policy: None,
        policy_declarations: Vec::new(),
    });
    Ok(module_idx)
}

fn mark_cpp_callback_abi(items: &mut [Item]) {
    for item in items {
        let Item::Func(function) = item else { continue };
        for param in &mut function.params {
            if matches!(param.ty, crate::AST::Type::Fn { .. }) {
                param.ty = crate::AST::Type::Tagged {
                    marker: crate::AST::CPP_CALLBACK_ABI_MARKER.to_string(),
                    inner: Box::new(param.ty.clone()),
                };
            }
        }
    }
}

fn synthetic_alias(language: ForeignLanguage, lib: &str) -> String {
    format!("__{}_{}", language.root(), lib)
}
