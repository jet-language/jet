// ── C-FFI data types ──────────────────────────────────────────────────────────

use super::{AccessConvention, ExternFn, ForeignLanguage, Type};
use std::path::{Path, PathBuf};

/// D-FFI-UNIFY1: one descriptor is the contract between a foreign schema,
/// generated Jet source, and the checked boundary. Adapters may vary their
/// transport, but they do not get a second ABI vocabulary.
pub const FOREIGN_DESCRIPTOR_SCHEMA: &str = "jet-ffi-descriptor-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinderSurface {
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
    SupervisedOctave,
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
    OctaveScript,
    ComTypeLibrary,
}

pub const FOREIGN_ABI_CONTRACT_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignCallingConvention {
    C,
    Cpp,
    Rust,
    HostMessage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignLayoutModel {
    CAbi,
    AdapterTyped,
    OpaqueHandle,
    Message,
    Native,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignOwnershipModel {
    SignatureDeclared,
    AdapterOwned,
    ByValue,
    BorrowedInputCopiedOutput,
    OwnedHandle,
    SharedHandle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignErrorModel {
    JetBoundary,
    AdapterResult,
    ErrorCode,
    TypedResult,
    PanicBoundary,
    ForeignException,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignCallbackModel {
    ReentrantThreadSafe,
    AdapterMarshalled,
    None,
    CAbi,
    HostRegistration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignAsyncModel {
    JetTask,
    AdapterMarshalled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignTaskBoundary {
    RejectCapturedState,
    AdapterMarshalled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForeignAbiContract {
    pub version: u16,
    pub calling_convention: ForeignCallingConvention,
    pub layout: ForeignLayoutModel,
    pub ownership: ForeignOwnershipModel,
    pub errors: ForeignErrorModel,
    pub callbacks: ForeignCallbackModel,
    pub async_completion: ForeignAsyncModel,
    pub task_boundary: ForeignTaskBoundary,
    pub safety: ForeignSafety,
    pub integer: ForeignScalar,
    pub floating: ForeignScalar,
    pub boolean: ForeignScalar,
    pub character: ForeignScalar,
    pub string: ForeignScalar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignSafety {
    GeneratedWrapper,
    UnsafeRaw,
}

/// Scalar projection selected by a binder descriptor. `Unsupported` is a
/// deliberate fail-closed value, not a request to invent a fallback type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignScalar {
    Int,
    Float,
    Bool,
    Char,
    String,
    Unsupported,
}

impl ForeignScalar {
    pub const fn jet_name(self) -> Option<&'static str> {
        match self {
            Self::Int => Some("Int"),
            Self::Float => Some("Float"),
            Self::Bool => Some("Bool"),
            Self::Char => Some("Char"),
            Self::String => Some("String"),
            Self::Unsupported => None,
        }
    }

    /// Rust type emitted at the checked C boundary for the matching Jet
    /// scalar. This is intentionally closed with no placeholder arm.
    pub const fn c_rust_name(self) -> Option<&'static str> {
        match self {
            Self::Int => Some("std::os::raw::c_longlong"),
            Self::Float => Some("f64"),
            Self::Bool => Some("bool"),
            Self::Char => Some("u32"),
            Self::String => Some("*const std::os::raw::c_char"),
            Self::Unsupported => None,
        }
    }
}

impl ForeignAbiContract {
    pub const fn c_abi() -> Self {
        Self {
            version: FOREIGN_ABI_CONTRACT_VERSION,
            calling_convention: ForeignCallingConvention::C,
            layout: ForeignLayoutModel::CAbi,
            ownership: ForeignOwnershipModel::SignatureDeclared,
            errors: ForeignErrorModel::JetBoundary,
            callbacks: ForeignCallbackModel::ReentrantThreadSafe,
            async_completion: ForeignAsyncModel::JetTask,
            task_boundary: ForeignTaskBoundary::RejectCapturedState,
            safety: ForeignSafety::GeneratedWrapper,
            integer: ForeignScalar::Int,
            floating: ForeignScalar::Float,
            boolean: ForeignScalar::Bool,
            character: ForeignScalar::Char,
            string: ForeignScalar::String,
        }
    }

    pub const fn adapter() -> Self {
        Self {
            version: FOREIGN_ABI_CONTRACT_VERSION,
            calling_convention: ForeignCallingConvention::HostMessage,
            layout: ForeignLayoutModel::AdapterTyped,
            ownership: ForeignOwnershipModel::AdapterOwned,
            errors: ForeignErrorModel::AdapterResult,
            callbacks: ForeignCallbackModel::AdapterMarshalled,
            async_completion: ForeignAsyncModel::AdapterMarshalled,
            task_boundary: ForeignTaskBoundary::AdapterMarshalled,
            safety: ForeignSafety::GeneratedWrapper,
            ..Self::c_abi()
        }
    }

    pub const C: Self = Self::c_abi();
    pub const CXX: Self = Self {
        calling_convention: ForeignCallingConvention::Cpp,
        layout: ForeignLayoutModel::OpaqueHandle,
        ownership: ForeignOwnershipModel::OwnedHandle,
        ..Self::c_abi()
    };
    pub const MESSAGE: Self = Self::adapter();
    pub const NATIVE: Self = Self {
        calling_convention: ForeignCallingConvention::Rust,
        layout: ForeignLayoutModel::Native,
        ..Self::adapter()
    };

    /// Map a normalized C declaration to the one descriptor-owned Jet scalar.
    /// Unsupported pointers and declarations return `None` and are skipped by
    /// the binder; no generated fake signature can reach the compiler.
    pub fn c_scalar(self, normalized: &str) -> Option<ForeignScalar> {
        if self.layout != ForeignLayoutModel::CAbi {
            return None;
        }
        if normalized.ends_with('*') {
            let base = normalized[..normalized.len() - 1].trim();
            return matches!(base, "char" | "signed char" | "unsigned char")
                .then_some(ForeignScalar::String)
                .filter(|kind| *kind != ForeignScalar::Unsupported);
        }
        let scalar = match normalized {
            "bool" | "_Bool" => ForeignScalar::Bool,
            "float" | "double" | "long double" => ForeignScalar::Float,
            "char" | "signed char" | "unsigned char" | "short" | "unsigned short" | "short int"
            | "unsigned short int" | "int" | "unsigned" | "unsigned int" | "signed"
            | "signed int" | "long" | "unsigned long" | "long int" | "unsigned long int"
            | "long long" | "unsigned long long" | "long long int" | "size_t" | "ssize_t"
            | "ptrdiff_t" | "intptr_t" | "uintptr_t" | "int8_t" | "int16_t" | "int32_t"
            | "int64_t" | "uint8_t" | "uint16_t" | "uint32_t" | "uint64_t" => ForeignScalar::Int,
            _ => ForeignScalar::Unsupported,
        };
        (scalar != ForeignScalar::Unsupported).then_some(scalar)
    }

    /// Stable text included in generated stubs, binder output, and cache
    /// provenance. A descriptor mutation therefore invalidates every product
    /// made from it instead of silently reusing an old ABI.
    pub fn stamp(self) -> String {
        format!(
            "{FOREIGN_DESCRIPTOR_SCHEMA};version={};calling={:?};layout={:?};ownership={:?};errors={:?};callbacks={:?};async={:?};tasks={:?};safety={:?};integer={:?};float={:?};bool={:?};char={:?};string={:?}",
            self.version,
            self.calling_convention,
            self.layout,
            self.ownership,
            self.errors,
            self.callbacks,
            self.async_completion,
            self.task_boundary,
            self.safety,
            self.integer,
            self.floating,
            self.boolean,
            self.character,
            self.string,
        )
    }
}

impl ForeignLanguage {
    /// Prefix used by generated bridge symbols.  Keep this beside the
    /// descriptor table so the loader cannot grow a second language map.
    pub const fn bridge_prefix(self) -> &'static str {
        match self {
            Self::C => "",
            Self::Cpp => "jet_cpp_",
            Self::Rust => "jet_rust_",
            Self::Py => "jet_py_",
            Self::JS => "jet_js_",
            Self::Swift => "jet_swift_",
            Self::Go => "jet_go_",
            Self::Java => "jet_java_",
            Self::DotNet => "jet_cs_",
            Self::Tcl => "jet_tcl_",
            Self::Lua => "jet_lua_",
            Self::Fortran => "jet_fortran_",
            Self::Cobol => "jet_cobol_",
            Self::Ada => "jet_ada_",
            Self::Pascal => "jet_pascal_",
            Self::Dart => "jet_dart_",
            Self::PowerShell => "jet_pwsh_",
            Self::Perl => "jet_perl_",
            Self::Ruby => "jet_ruby_",
            Self::Php => "jet_php_",
            Self::R => "jet_r_",
            Self::Octave => "jet_octave_",
            Self::Com => "jet_com_",
        }
    }

    pub const fn abi_contract(self) -> ForeignAbiContract {
        match self {
            Self::C
            | Self::Cpp
            | Self::Swift
            | Self::Go
            | Self::Fortran
            | Self::Cobol
            | Self::Ada
            | Self::Pascal => ForeignAbiContract::C,
            Self::Rust => ForeignAbiContract::NATIVE,
            Self::Py
            | Self::JS
            | Self::Java
            | Self::DotNet
            | Self::Tcl
            | Self::Lua
            | Self::Dart
            | Self::PowerShell
            | Self::Perl
            | Self::Ruby
            | Self::Php
            | Self::R
            | Self::Octave
            | Self::Com => ForeignAbiContract::MESSAGE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinderCapability {
    TypedStub,
    SafeWrapper,
    OwnershipConversion,
    LayoutValidation,
    ErrorConversion,
    CallbackValidation,
    CacheProvenance,
    PackageProvider,
    TargetDispatch,
    UnsafeRawEscape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignProvider {
    System,
    Cargo,
    Npm,
    PyPi,
    SwiftPm,
    Jetpack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignStubFile {
    None,
    Suffix(&'static str),
    StemSuffix(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BinderDescriptor {
    pub language: ForeignLanguage,
    pub surface: BinderSurface,
    pub status: BinderStatus,
    pub runtime: BinderRuntime,
    pub stub_kind: BindingStubKind,
    pub contract: ForeignAbiContract,
    pub capabilities: &'static [BinderCapability],
    pub effect_root: &'static str,
    pub provider: ForeignProvider,
    pub cache_extension: &'static str,
    pub type_stub_file: ForeignStubFile,
}

const C_CAPABILITIES: &[BinderCapability] = &[
    BinderCapability::TypedStub,
    BinderCapability::SafeWrapper,
    BinderCapability::OwnershipConversion,
    BinderCapability::LayoutValidation,
    BinderCapability::ErrorConversion,
    BinderCapability::CallbackValidation,
    BinderCapability::CacheProvenance,
    BinderCapability::PackageProvider,
    BinderCapability::UnsafeRawEscape,
];
const TARGET_CAPABILITIES: &[BinderCapability] = &[
    BinderCapability::TypedStub,
    BinderCapability::SafeWrapper,
    BinderCapability::OwnershipConversion,
    BinderCapability::LayoutValidation,
    BinderCapability::ErrorConversion,
    BinderCapability::CallbackValidation,
    BinderCapability::CacheProvenance,
    BinderCapability::PackageProvider,
    BinderCapability::TargetDispatch,
    BinderCapability::UnsafeRawEscape,
];
const ADAPTER_CAPABILITIES: &[BinderCapability] = &[
    BinderCapability::TypedStub,
    BinderCapability::SafeWrapper,
    BinderCapability::OwnershipConversion,
    BinderCapability::LayoutValidation,
    BinderCapability::ErrorConversion,
    BinderCapability::CacheProvenance,
    BinderCapability::PackageProvider,
    BinderCapability::UnsafeRawEscape,
];
const PLANNED_CAPABILITIES: &[BinderCapability] = &[
    BinderCapability::TypedStub,
    BinderCapability::SafeWrapper,
    BinderCapability::OwnershipConversion,
    BinderCapability::LayoutValidation,
    BinderCapability::ErrorConversion,
    BinderCapability::CallbackValidation,
    BinderCapability::CacheProvenance,
    BinderCapability::PackageProvider,
    BinderCapability::UnsafeRawEscape,
];

const fn binder(
    language: ForeignLanguage,
    runtime: BinderRuntime,
    stub_kind: BindingStubKind,
    status: BinderStatus,
    contract: ForeignAbiContract,
    capabilities: &'static [BinderCapability],
    effect_root: &'static str,
    provider: ForeignProvider,
    type_stub_file: ForeignStubFile,
) -> BinderDescriptor {
    BinderDescriptor {
        language,
        surface: BinderSurface::Namespace,
        status,
        runtime,
        stub_kind,
        contract,
        capabilities,
        effect_root,
        provider,
        cache_extension: "jet",
        type_stub_file,
    }
}

/// Canonical descriptor table. Driver routing and package binders both read
/// this table; no language adapter owns a parallel ABI contract.
pub const FOREIGN_BINDERS: &[BinderDescriptor] = &[
    binder(
        ForeignLanguage::C,
        BinderRuntime::DirectCAbi,
        BindingStubKind::CHeader,
        BinderStatus::Active,
        ForeignAbiContract::C,
        C_CAPABILITIES,
        "FFI",
        ForeignProvider::System,
        ForeignStubFile::None,
    ),
    binder(
        ForeignLanguage::Cpp,
        BinderRuntime::ClangCppShim,
        BindingStubKind::CppHeader,
        BinderStatus::Active,
        ForeignAbiContract::CXX,
        C_CAPABILITIES,
        "FFI.Cpp",
        ForeignProvider::System,
        ForeignStubFile::None,
    ),
    binder(
        ForeignLanguage::Rust,
        BinderRuntime::LegacyRustExtern,
        BindingStubKind::RustExternBlock,
        BinderStatus::Planned,
        ForeignAbiContract::NATIVE,
        PLANNED_CAPABILITIES,
        "FFI",
        ForeignProvider::Cargo,
        ForeignStubFile::None,
    ),
    binder(
        ForeignLanguage::Py,
        BinderRuntime::SupervisedPythonSidecar,
        BindingStubKind::PythonIntrospection,
        BinderStatus::Active,
        ForeignAbiContract::MESSAGE,
        ADAPTER_CAPABILITIES,
        "FFI.Py",
        ForeignProvider::PyPi,
        ForeignStubFile::None,
    ),
    binder(
        ForeignLanguage::JS,
        BinderRuntime::TargetDispatchedJs,
        BindingStubKind::TypeScriptDeclarations,
        BinderStatus::Active,
        ForeignAbiContract::MESSAGE,
        TARGET_CAPABILITIES,
        "FFI",
        ForeignProvider::Npm,
        ForeignStubFile::Suffix("d.ts"),
    ),
    binder(
        ForeignLanguage::Swift,
        BinderRuntime::SwiftCAbiBridge,
        BindingStubKind::SwiftModule,
        BinderStatus::Planned,
        ForeignAbiContract::C,
        PLANNED_CAPABILITIES,
        "FFI",
        ForeignProvider::SwiftPm,
        ForeignStubFile::None,
    ),
    binder(
        ForeignLanguage::Go,
        BinderRuntime::GoCArchive,
        BindingStubKind::GoExports,
        BinderStatus::Active,
        ForeignAbiContract::C,
        C_CAPABILITIES,
        "FFI.Go",
        ForeignProvider::System,
        ForeignStubFile::None,
    ),
    binder(
        ForeignLanguage::Java,
        BinderRuntime::EmbeddedJvm,
        BindingStubKind::JvmClass,
        BinderStatus::Active,
        ForeignAbiContract::MESSAGE,
        ADAPTER_CAPABILITIES,
        "FFI.Java",
        ForeignProvider::System,
        ForeignStubFile::None,
    ),
    binder(
        ForeignLanguage::DotNet,
        BinderRuntime::EmbeddedDotNet,
        BindingStubKind::DotNetAssembly,
        BinderStatus::Active,
        ForeignAbiContract::MESSAGE,
        ADAPTER_CAPABILITIES,
        "FFI.DotNet",
        ForeignProvider::System,
        ForeignStubFile::None,
    ),
    binder(
        ForeignLanguage::Tcl,
        BinderRuntime::EmbeddedTcl,
        BindingStubKind::TclScript,
        BinderStatus::Active,
        ForeignAbiContract::MESSAGE,
        ADAPTER_CAPABILITIES,
        "FFI.Tcl",
        ForeignProvider::System,
        ForeignStubFile::None,
    ),
    binder(
        ForeignLanguage::Lua,
        BinderRuntime::EmbeddedLua,
        BindingStubKind::LuaScript,
        BinderStatus::Active,
        ForeignAbiContract::MESSAGE,
        ADAPTER_CAPABILITIES,
        "FFI.Lua",
        ForeignProvider::System,
        ForeignStubFile::None,
    ),
    binder(
        ForeignLanguage::Fortran,
        BinderRuntime::FortranIsoCBinding,
        BindingStubKind::FortranIsoCBinding,
        BinderStatus::Active,
        ForeignAbiContract::C,
        C_CAPABILITIES,
        "FFI.Fortran",
        ForeignProvider::System,
        ForeignStubFile::None,
    ),
    binder(
        ForeignLanguage::Cobol,
        BinderRuntime::GnuCobolCAbi,
        BindingStubKind::GnuCobolCopybook,
        BinderStatus::Active,
        ForeignAbiContract::C,
        C_CAPABILITIES,
        "FFI.Cobol",
        ForeignProvider::System,
        ForeignStubFile::None,
    ),
    binder(
        ForeignLanguage::Ada,
        BinderRuntime::AdaGnatCAbi,
        BindingStubKind::AdaSpec,
        BinderStatus::Active,
        ForeignAbiContract::C,
        C_CAPABILITIES,
        "FFI.Ada",
        ForeignProvider::System,
        ForeignStubFile::None,
    ),
    binder(
        ForeignLanguage::Pascal,
        BinderRuntime::FreePascalCdecl,
        BindingStubKind::PascalSource,
        BinderStatus::Active,
        ForeignAbiContract::C,
        C_CAPABILITIES,
        "FFI.Pascal",
        ForeignProvider::System,
        ForeignStubFile::None,
    ),
    binder(
        ForeignLanguage::Dart,
        BinderRuntime::DartApiDl,
        BindingStubKind::DartContract,
        BinderStatus::Active,
        ForeignAbiContract::MESSAGE,
        ADAPTER_CAPABILITIES,
        "FFI.Dart",
        ForeignProvider::System,
        ForeignStubFile::StemSuffix("_host.dart"),
    ),
    binder(
        ForeignLanguage::PowerShell,
        BinderRuntime::SupervisedPowerShell,
        BindingStubKind::PowerShellScript,
        BinderStatus::Active,
        ForeignAbiContract::MESSAGE,
        ADAPTER_CAPABILITIES,
        "FFI.PowerShell",
        ForeignProvider::System,
        ForeignStubFile::None,
    ),
    binder(
        ForeignLanguage::Perl,
        BinderRuntime::SupervisedPerl,
        BindingStubKind::PerlScript,
        BinderStatus::Active,
        ForeignAbiContract::MESSAGE,
        ADAPTER_CAPABILITIES,
        "FFI.Perl",
        ForeignProvider::System,
        ForeignStubFile::None,
    ),
    binder(
        ForeignLanguage::Ruby,
        BinderRuntime::SupervisedRuby,
        BindingStubKind::RubyScript,
        BinderStatus::Active,
        ForeignAbiContract::MESSAGE,
        ADAPTER_CAPABILITIES,
        "FFI.Ruby",
        ForeignProvider::System,
        ForeignStubFile::None,
    ),
    binder(
        ForeignLanguage::Php,
        BinderRuntime::SupervisedPhpPool,
        BindingStubKind::PhpScript,
        BinderStatus::Active,
        ForeignAbiContract::MESSAGE,
        ADAPTER_CAPABILITIES,
        "FFI.Php",
        ForeignProvider::System,
        ForeignStubFile::None,
    ),
    binder(
        ForeignLanguage::R,
        BinderRuntime::SupervisedR,
        BindingStubKind::RScript,
        BinderStatus::Active,
        ForeignAbiContract::MESSAGE,
        ADAPTER_CAPABILITIES,
        "FFI.R",
        ForeignProvider::System,
        ForeignStubFile::None,
    ),
    binder(
        ForeignLanguage::Octave,
        BinderRuntime::SupervisedOctave,
        BindingStubKind::OctaveScript,
        BinderStatus::Active,
        ForeignAbiContract::MESSAGE,
        ADAPTER_CAPABILITIES,
        "FFI.Octave",
        ForeignProvider::System,
        ForeignStubFile::None,
    ),
    binder(
        ForeignLanguage::Com,
        BinderRuntime::WindowsComAutomation,
        BindingStubKind::ComTypeLibrary,
        BinderStatus::Active,
        ForeignAbiContract::MESSAGE,
        ADAPTER_CAPABILITIES,
        "FFI.Com",
        ForeignProvider::System,
        ForeignStubFile::None,
    ),
];

pub fn binder_descriptor(language: ForeignLanguage) -> Option<&'static BinderDescriptor> {
    FOREIGN_BINDERS
        .iter()
        .find(|descriptor| descriptor.language == language)
}

pub fn foreign_abi_contract(language: ForeignLanguage) -> ForeignAbiContract {
    binder_descriptor(language)
        .map(|descriptor| descriptor.contract)
        .unwrap_or(ForeignAbiContract::MESSAGE)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BinderCapabilityReport {
    pub schema: &'static str,
    pub language: ForeignLanguage,
    pub status: BinderStatus,
    pub contract: ForeignAbiContract,
    pub effect_root: &'static str,
    pub provider: ForeignProvider,
    pub capabilities: &'static [BinderCapability],
}

impl BinderDescriptor {
    pub const fn abi_contract(self) -> ForeignAbiContract {
        self.contract
    }

    pub fn stamp(self) -> String {
        format!(
            "{FOREIGN_DESCRIPTOR_SCHEMA};language={:?};surface={:?};status={:?};runtime={:?};stub={:?};contract={};effect={};provider={:?};cache={};type-stub={:?};capabilities={:?};bridge={}",
            self.language,
            self.surface,
            self.status,
            self.runtime,
            self.stub_kind,
            self.contract.stamp(),
            self.effect_root,
            self.provider,
            self.cache_extension,
            self.type_stub_file,
            self.capabilities,
            self.language.bridge_prefix(),
        )
    }

    pub fn capability_report(self) -> BinderCapabilityReport {
        BinderCapabilityReport {
            schema: FOREIGN_DESCRIPTOR_SCHEMA,
            language: self.language,
            status: self.status,
            contract: self.contract,
            effect_root: self.effect_root,
            provider: self.provider,
            capabilities: self.capabilities,
        }
    }
}

impl ExternFn {
    /// Whether the resident JIT and tier-0 evaluator can execute this signature
    /// through the hidden bridge.
    ///
    /// Every other valid C signature stays on CModule's direct wrapper path.
    pub fn hidden_c_bridge_compatible(&self) -> bool {
        // D-FFI-CAP1: the resident JIT/interpreter bridge is value-shaped only.
        // A capability call must report the native boundary; it must never be
        // re-encoded as a by-value adapter that could copy or retain storage.
        if self
            .params
            .iter()
            .any(|param| param.convention != AccessConvention::Read)
        {
            return false;
        }
        match self.return_type.as_ref() {
            None => {
                self.params.is_empty()
                    || matches!(self.params.as_slice(), [param] if param.ty == Type::Int)
            }
            Some(Type::Int) => {
                self.params.is_empty()
                    || matches!(self.params.as_slice(), [param] if param.ty == Type::Int)
                    || matches!(self.params.as_slice(), [param] if param.ty == Type::String)
                    || matches!(
                        self.params.as_slice(),
                        [left, right] if left.ty == Type::Int && right.ty == Type::Int
                    )
                    || matches!(
                        self.params.as_slice(),
                        [handle, code] if handle.ty == Type::Int && code.ty == Type::String
                    )
            }
            Some(Type::Float) => {
                matches!(self.params.as_slice(), [param] if param.ty == Type::Float)
                    || (self.params.len() == 6
                        && self.params.iter().all(|param| param.ty == Type::Float))
                    || matches!(
                        self.params.as_slice(),
                        [handle, code] if handle.ty == Type::Int && code.ty == Type::String
                    )
            }
            Some(Type::String) => {
                matches!(self.params.as_slice(), [param] if param.ty == Type::String)
                    || matches!(
                        self.params.as_slice(),
                        [handle, code] if handle.ty == Type::Int && code.ty == Type::String
                    )
            }
            _ => false,
        }
    }
}

/// The result of resolving one C `use` in one file.
#[derive(Debug, Clone)]
pub struct CImportLink {
    pub importing_idx: usize,
    /// `None` is a file-wide import; `Some` names the inline module whose
    /// body owns the import. Inline modules may intentionally reuse an alias,
    /// so the scope is part of the resolution key.
    pub scope: Option<String>,
    pub alias: String,
    pub target_idx: usize,
}

/// One C library that the program links against.
#[derive(Debug, Clone)]
pub struct CLib {
    pub lib: String,
    pub module_idx: usize,
}

/// Gathered C-FFI artifacts threaded into sema and codegen.
#[derive(Debug, Default, Clone)]
pub struct CFfi {
    pub import_links: Vec<CImportLink>,
    pub libs: Vec<CLib>,
}

impl CFfi {
    pub fn target_for(&self, importing_idx: usize, alias: &str) -> Option<usize> {
        self.target_for_scope(importing_idx, None, alias)
    }

    pub fn target_for_scope(
        &self,
        importing_idx: usize,
        scope: Option<&str>,
        alias: &str,
    ) -> Option<usize> {
        self.import_links
            .iter()
            .find(|l| {
                l.importing_idx == importing_idx && l.scope.as_deref() == scope && l.alias == alias
            })
            .map(|l| l.target_idx)
    }

    pub fn links_c(&self) -> bool {
        !self.libs.is_empty()
    }
}

// ── Comptime embed input ──────────────────────────────────────────────────────

/// D-CTEFFECT1 (Tier-1): one comptime embed input recorded for reproducibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComptimeInput {
    pub path: String,
    pub hash: String,
}

// ── Rust FFI link artifact ────────────────────────────────────────────────────

/// Built FFI bridge artifact paths for rustc linking (M7).
#[derive(Debug, Clone)]
pub struct FfiLink {
    pub crate_name: String,
    /// Content-addressed identity of the bridge inputs.
    pub cache_identity: String,
    /// Queryable provenance sidecar for the published bridge artifacts.
    pub provenance_path: PathBuf,
    pub rlib_path: PathBuf,
    /// Shared library with `*_cabi` trampolines for the resident Cranelift JIT.
    pub cdylib_path: PathBuf,
    /// Selected-target runtime dependencies emitted by Cargo.
    pub target_deps_dir: PathBuf,
    /// Host artifacts needed while rustc loads target metadata (notably proc macros).
    pub host_deps_dir: PathBuf,
    /// Path to the built `jet-crypto-helper` binary, present only when the
    /// bridge was built with `needs_crypto` (card c146 — package signing shells
    /// out to this helper for Ed25519 keygen/sign/verify). `None` otherwise.
    pub helper_bin_path: Option<PathBuf>,
    /// U13 (D-JPK-SECRETCRYPTO1): path to the built `jet-secrets-helper`
    /// binary, present only when the bridge was built with `needs_secrets` —
    /// `jetpack secrets set/get/recipients/keygen` shells out to this for the
    /// age-style encrypt/decrypt/keygen operations. `None` otherwise.
    pub secrets_helper_bin_path: Option<PathBuf>,
}

impl FfiLink {
    /// Cargo's dependency search paths, target artifacts first and without duplicates.
    pub fn dependency_dirs(&self) -> impl Iterator<Item = &Path> {
        std::iter::once(self.target_deps_dir.as_path()).chain(
            (self.host_deps_dir != self.target_deps_dir).then_some(self.host_deps_dir.as_path()),
        )
    }
}
