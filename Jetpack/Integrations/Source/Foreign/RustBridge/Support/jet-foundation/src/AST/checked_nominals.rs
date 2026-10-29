//! Original checked declaration facts shared by sema, whole-program lowering and comptime.

use super::{AccessConvention, ConstDef, EnumDef, Func, StructDef, Type};
use crate::Names::NameLedger;
use std::collections::HashMap;
use std::sync::Arc;

/// Loaded source-owned Core signature projected into a comptime fragment.
#[derive(Clone, Debug)]
pub struct MirFragmentCoreSourceSignature {
    /// Generic parameter names in declaration order.
    pub type_params: Vec<String>,
    /// Checked parameter conventions and types in declaration order.
    pub params: Vec<(AccessConvention, Type)>,
    /// Declared return type, if the function returns a value.
    pub return_type: Option<Type>,
    /// Canonical return of the callable value, including its protocol/carrier.
    pub value_return_type: Type,
}

/// Loaded source module whose checked bodies a fragment lowers on demand.
#[derive(Clone, Debug)]
pub struct MirFragmentCoreSourceModule {
    /// Loader alias of the defining module (`CoreSourceModule::alias`).
    pub alias: String,
    /// Checked owner shared by its lowered bodies and their call sites.
    pub module_identity: String,
    /// The defining module's own Core import aliases.
    pub core_imports: HashMap<String, String>,
    /// Original top-level functions, including private helpers.
    pub functions: HashMap<String, Func>,
    /// Canonical own signatures, frozen once instead of requalified per body.
    pub function_signatures: HashMap<String, MirFragmentCoreSourceSignature>,
    /// Original top-level constant declarations.
    pub constants: Vec<ConstDef>,
    /// User import alias to the original module's loader alias.
    pub module_imports: HashMap<String, String>,
    /// Member-imported function to (original loader alias, function name).
    pub imported_functions: HashMap<String, (String, String)>,
    /// Original cross-module signatures keyed by import and function name.
    pub import_signatures: HashMap<(String, String), MirFragmentCoreSourceSignature>,
    /// Source type spellings to their original canonical identities.
    pub nominal_identities: HashMap<String, String>,
}

/// Immutable original checked nominal facts, not a public semantic index.
///
/// These rows contain no owning ProgramBundle/ModuleState edge. The real
/// NameLedger shares its immutable tables; Core bodies and signatures retain
/// the existing shared modern suppliers rather than rebuilding an old graph.
#[derive(Clone, Debug, Default)]
pub struct MirFragmentNominalFacts {
    /// Original semantic owner/name index of the loaded source modules.
    pub name_ledger: NameLedger,
    /// Defining module of the evaluated fragment in that original index.
    pub module_idx: usize,
    /// Authenticated identity of this module in the same checked graph.
    pub module_identity: String,
    /// Original registered Prelude Err identity, never inferred from spelling.
    pub compiler_owned_default_err_identity: Option<String>,
    /// Canonical own signatures shared across evaluation requests.
    pub function_signatures: Arc<HashMap<String, MirFragmentCoreSourceSignature>>,
    /// Source spellings to original canonical nominal identities.
    pub nominal_identities: HashMap<String, String>,
    /// Import alias to the target's original loader alias.
    pub import_modules: HashMap<String, String>,
    /// Canonical imported nominal identity to its original loader alias.
    pub foreign_modules: HashMap<String, String>,
    /// Original checked struct rows, including private fields and defaults.
    pub structs: HashMap<String, StructDef>,
    /// Original checked enum rows, including their payload shapes.
    pub enums: HashMap<String, EnumDef>,
    /// Loaded source-owned Core signatures, keyed by import and function.
    pub core_source_sigs: HashMap<(String, String), MirFragmentCoreSourceSignature>,
    /// Original loaded Core/user rows shared across the whole check.
    pub core_source_bodies: Arc<HashMap<String, Arc<MirFragmentCoreSourceModule>>>,
    /// Member-imported user function to (original loader alias, function name).
    pub imported_functions: HashMap<String, (String, String)>,
    /// Original signatures of the evaluated module's imported callees.
    pub import_signatures: HashMap<(String, String), MirFragmentCoreSourceSignature>,
}
