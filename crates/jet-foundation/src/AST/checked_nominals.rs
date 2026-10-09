//! Immutable checked declaration facts carried separately from public semantic indexes.

use super::{AccessConvention, Func, StructDef, Type};
use std::collections::HashMap;

/// Loaded source-owned Core signature projected into a comptime fragment.
#[derive(Clone, Debug)]
pub struct MirFragmentCoreSourceSignature {
    /// Generic parameter names in declaration order.
    pub type_params: Vec<String>,
    /// Checked parameter conventions and types in declaration order.
    pub params: Vec<(AccessConvention, Type)>,
    /// Declared return type, if the function returns a value.
    pub return_type: Option<Type>,
}

/// Loaded source module whose bodies a comptime fragment lowers on demand,
/// exactly as runtime lowering does for a reachable call: a source-owned Core
/// module, or a user file module a `prep` initializer reaches.
#[derive(Clone, Debug)]
pub struct MirFragmentCoreSourceModule {
    /// Loader alias of the defining module (`CoreSourceModule::alias`).
    pub alias: String,
    /// Checked module identity. Lowered bodies and the call sites that reach
    /// them share this owner in their semantic key.
    pub module_identity: String,
    /// The defining module's own `use core... as alias` table.
    pub core_imports: HashMap<String, String>,
    /// Every top-level function of the module by name, private helpers
    /// included, so a demanded body can call its same-module helpers.
    pub functions: HashMap<String, Func>,
    /// Every top-level constant of the module, so a demanded body reads its
    /// same-module tables (`C_SHA256_K[t]`) as runtime lowering does.
    pub constants: Vec<crate::AST::ConstDef>,
    /// User file module: its own `use` aliases, source alias to the imported
    /// module's `core_source_bodies` key (its loader alias). Empty for Core.
    pub module_imports: HashMap<String, String>,
    /// User file module: member-imported functions (`use c.[f]`), local name
    /// to (`core_source_bodies` key, function name). Empty for Core.
    pub imported_functions: HashMap<String, (String, String)>,
    /// User file module: declared signatures of its cross-module callees,
    /// keyed by (source alias or local name, function name).
    pub import_signatures: HashMap<(String, String), MirFragmentCoreSourceSignature>,
    /// User file module: its source type spellings (`Point`, `c.Shape`) to
    /// canonical `package::module::Type` identities. Empty for Core.
    pub nominal_identities: HashMap<String, String>,
}

/// Checked nominal facts retained for a comptime fragment.
///
/// Sema owns the source-to-canonical projection and the checked declaration
/// rows. The fragment lowerer consumes these facts without rediscovering an
/// imported owner from erased `CtValue` data.
#[derive(Clone, Debug, Default)]
pub struct MirFragmentNominalFacts {
    /// Authenticated identity of the active caller module for this projection.
    pub module_identity: String,
    /// Authenticated canonical Err declaration identity, supplied by the
    /// registered Prelude owner rather than inferred from its spelling.
    pub compiler_owned_default_err_identity: Option<String>,
    /// Source spellings (including import-qualified and selective-import
    /// names) to canonical `package::module::Type` identities.
    pub nominal_identities: HashMap<String, String>,
    /// Source import alias to the target module's loader alias.
    pub import_modules: HashMap<String, String>,
    /// Canonical nominal identity to the target module's loader alias.
    pub foreign_modules: HashMap<String, String>,
    /// Canonical imported struct identity to its checked declaration row.
    pub structs: HashMap<String, StructDef>,
    /// Checked enum declarations, including their payload shapes.
    pub enums: HashMap<String, crate::AST::EnumDef>,
    /// Loaded source signatures for imported source-owned Core members.
    pub core_source_sigs: HashMap<(String, String), MirFragmentCoreSourceSignature>,
    /// Loaded source-owned Core modules keyed by Core module path
    /// (`core.math`); the owner of every `core_source_sigs` row is here.
    /// Shared: every evaluation request clones these facts.
    pub core_source_bodies: std::sync::Arc<HashMap<String, MirFragmentCoreSourceModule>>,
    /// The evaluated module's member-imported user functions (`use b.[f]`):
    /// local name to (`core_source_bodies` key, function name).
    pub imported_functions: HashMap<String, (String, String)>,
    /// Declared signatures of the evaluated module's user-module callees,
    /// keyed by (source alias or local name, function name).
    pub import_signatures: HashMap<(String, String), MirFragmentCoreSourceSignature>,
}
