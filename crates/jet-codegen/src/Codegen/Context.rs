use super::{mangle, mangle_path, tuple_fields_plain, tuple_struct_name};
use crate::jet_generated_format as jet_format;
use crate::Diagnostics::Span;
use crate::Generics;
use crate::Syntax;
use crate::AST::FfiLink;
#[cfg(test)]
use crate::AST::Program;
use crate::AST::{
    AccessConvention, ContractClause, CtValue, EnumDef, Expr, FfiHandleFact, Func, Item,
    ProgramBundle, StructDef, Type, VariantField, VariantPayload,
};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
pub(crate) struct UnitFact {
    pub(crate) family: String,
    /// `None` for a nominal family (D-DIMENSION-OPEN1=D).
    pub(crate) dimension: Option<crate::AST::Dimension>,
    pub(crate) kind: crate::AST::QuantityKind,
    pub(crate) scale: crate::AST::UnitRatio,
    pub(crate) offset: crate::AST::UnitRatio,
    pub(crate) scale_provenance: crate::AST::UnitScaleProvenance,
}

#[derive(Clone)]
pub(crate) struct UnitLabel {
    pub(crate) symbol: String,
    pub(crate) name: String,
    pub(crate) family: String,
    pub(crate) is_base: bool,
}

fn unit_label(
    family: &crate::AST::UnitFamilyDef,
    member: &crate::AST::UnitFamilyMember,
) -> UnitLabel {
    let base = family
        .base
        .as_ref()
        .map(|base| base.0.as_str())
        .or_else(|| family.members.first().map(|member| member.name.as_str()));
    UnitLabel {
        symbol: member.name.clone(),
        name: crate::AST::UnitFamilyDef::type_name(&member.name),
        family: family.family.clone(),
        is_base: base == Some(member.name.as_str()),
    }
}

fn unit_family_member_for_type<'a>(
    family: &'a crate::AST::UnitFamilyDef,
    type_name: &str,
    kind: crate::AST::QuantityKind,
) -> Option<&'a crate::AST::UnitFamilyMember> {
    family.members.iter().find(|member| {
        let stem = crate::AST::UnitFamilyDef::type_name(&member.name);
        type_name
            == match kind {
                crate::AST::QuantityKind::Linear => stem,
                crate::AST::QuantityKind::Point => format!("{stem}Point"),
                crate::AST::QuantityKind::Delta => format!("{stem}Delta"),
            }
    })
}

fn unit_fact(
    family: &crate::AST::UnitFamilyDef,
    member: &crate::AST::UnitFamilyMember,
    dimension: Option<crate::AST::Dimension>,
    kind: crate::AST::QuantityKind,
) -> UnitFact {
    UnitFact {
        family: family.family.clone(),
        dimension,
        kind,
        scale: member.scale.clone(),
        offset: if kind == crate::AST::QuantityKind::Point {
            member.offset.clone()
        } else {
            crate::AST::UnitRatio::zero()
        },
        scale_provenance: member.scale_provenance.clone(),
    }
}


#[derive(Clone)]
pub(crate) struct ExternFn {
    pub(crate) wrapper: String,
    pub(crate) c_abi: bool,
    pub(crate) component: Option<String>,
}


/// D-OPMIX1: one checked operator method keyed by its complete
/// `(lhs, trait, method, rhs)` identity. The ordinary method tables cannot
/// represent two RHS pairs with one owner and method spelling.
#[derive(Clone)]
pub(crate) struct OperatorMethod {
    pub(crate) trait_name: String,
    pub(crate) sig: Vec<(AccessConvention, Type)>,
    pub(crate) ret: Option<Type>,
}
/// D-FOUND-RECEIPT1: declaration-owned facts appended internally to a receipt
/// Core call. Runtime tiers never infer section meaning from a value.
#[derive(Clone)]
pub(crate) struct ReceiptSectionFact {
    pub(crate) name: String,
    pub(crate) type_name: String,
    pub(crate) schema_digest: String,
}

pub(crate) struct Cx {
    /// Top-level function name -> parameter conventions+types.
    pub(crate) sigs: HashMap<String, Vec<(AccessConvention, Type)>>,
    /// D-FAIL-TIER1: callable contract clauses, kept beside the callable
    /// signature so call lowering can place `#Pre` at the caller.
    pub(crate) contract_sigs: HashMap<String, (Vec<ContractClause>, Vec<ContractClause>)>,
    /// Top-level function name -> effective function value type (M8).
    /// The return is the executable failure carrier used by calls.
    pub(crate) fn_types: HashMap<String, Type>,
    /// D-NEVER1=C: sema-proved callable names whose bodies have no returning
    /// path. Codegen consumes this fact; it never re-infers divergence.
    pub(crate) diverging_functions: HashSet<String>,
    /// Top-level function name -> source-declared function value type.
    /// Binding a named function as a value uses this source contract; call
    /// lowering uses `fn_types`' effective failure carrier instead.
    pub(crate) fn_source_types: HashMap<String, Type>,
    /// Checked function bodies retained for route-contract extraction. This
    /// is compiler metadata copied from the parsed AST, never a source scan by
    /// a runtime adapter.
    pub(crate) fn_bodies: HashMap<String, Vec<crate::AST::Stmt>>,
    /// Function name -> source parameter names for labeled compute transforms.
    pub(crate) fn_param_names: HashMap<String, Vec<String>>,
    /// `(TypeName, method)` -> parameter conventions+types (including `self`).
    pub(crate) method_sigs: HashMap<(String, String), Vec<(AccessConvention, Type)>>,
    /// Method-owned type parameters in declaration order. Owner parameters are
    /// kept separately in `struct_type_param_order`.
    pub(crate) method_type_params: HashMap<(String, String), Vec<crate::AST::TypeParam>>,
    pub(crate) method_self_convs: HashMap<(String, String), AccessConvention>,
    /// c109 Phase 6 (TIR): `(TypeName, method)` -> resolved return type (or `None`
    /// for a unit-returning method). Used by TIR lowering to give a method-call
    /// expression its total result `Type` without re-inferring in codegen.
    pub(crate) method_rets: HashMap<(String, String), Option<Type>>,
    pub(crate) consts: HashMap<String, String>,
    /// D-PERSIST-DEVSTATE1=A: source-level types for the one writable module
    /// storage class. TIR turns these names into persistent slots before any
    /// engine-specific lowering runs.
    pub(crate) persist_types: HashMap<String, Type>,
    /// The evaluated value behind each comptime const, so lowering can hand a
    /// structured literal to every engine instead of the rendered Rust text.
    pub(crate) const_values: HashMap<String, CtValue>,
    pub(crate) type_names: HashSet<String>,
    /// D-SERDE2: source-level names of locally declared imported nominals map to
    /// their canonical bundle identities while lowering the declaring module.
    /// This is separate from `local_type_names`: that set controls source lookup
    /// precedence, while this map gives TIR/JIT shape consumers the one identity
    /// registered for the imported module's own records and enums.
    pub(crate) local_type_identities: HashMap<String, String>,
    /// Nominals declared by the module being emitted. Imported canonical
    /// identities and convenience leaves also live in `type_names`; this set
    /// preserves source lookup precedence when a local shadows an import.
    pub(crate) local_type_names: HashSet<String>,
    /// D-DIST1 (c109 Phase 23): distinct-type name -> (base type, is_numeric). A
    /// distinct type renders to a `#[repr(transparent)]` newtype `__jet_<Name>(pub
    /// Base)`; the TIR reads the base type to give `.raw()` (`(recv).0`) its total
    /// result type, and `is_numeric` is informational (the arithmetic operator is
    /// chosen by `ast_operand_is_integer`, which returns `None` for a distinct).
    pub(crate) distinct_types: HashMap<String, (Type, bool)>,
    /// D-RANGETYPE1: range-constrained distinct type name -> inclusive bounds.
    pub(crate) distinct_ranges: HashMap<String, (i64, i64)>,
    /// D-SHAPE-QUANTITY1=A: the one backend registry for physical unit facts.
    /// Consulted only while lowering; facts erase from emitted Rust.
    pub(crate) unit_facts: HashMap<String, UnitFact>,
    /// D-QUANTITY-PRINT1: labels for all unit-family types.
    pub(crate) unit_labels: HashMap<String, UnitLabel>,
    /// D-TYPEALIAS1: transparent generic alias name -> (params, target).
    pub(crate) type_aliases: HashMap<String, (Vec<crate::AST::TypeParam>, Type)>,
    pub(crate) trait_names: HashSet<String>,
    pub(crate) struct_fields: HashMap<String, Vec<(String, Type)>>,
    /// Checked `#Receipt` declarations keyed by canonical type name.
    pub(crate) receipt_sections: HashMap<String, ReceiptSectionFact>,
    /// D-DX-PLUGIN1=D: checked package-wide panel/publication catalog shared
    /// by TIR and every native/backend projection.
    pub(crate) devtools_registry: jet_foundation::AST::DevtoolsRegistry,
    /// Current module's canonical package scope.
    pub(crate) devtools_package: String,
    /// Current module's canonical source-module path.
    pub(crate) devtools_module: String,
    /// D-METAREFLECT1: the registered field rows shared by comptime and
    /// runtime reflection. Layout consumers keep their own ABI map, while
    /// reflection reads this metadata-bearing model.
    pub(crate) reflection_fields: HashMap<String, Vec<jet_foundation::Reflection::ReflectionField>>,
    /// D-BOUND-EVOLVE1=A: published records carry one compiler-owned wire
    /// holder. The holder is not part of the Jet source schema.
    pub(crate) published_schemas: HashSet<String>,
    /// Structs carrying both checked serde derives needed for typed HTTP
    /// request/response schemas.
    pub(crate) codable_types: HashSet<String>,
    /// Canonical typeable paths for reflectable nominal types. This is a
    /// projection cache seeded only from the sema name ledger.
    pub(crate) reflect_paths: HashMap<String, String>,
    /// Generic parameters that occur in non-skipped serialized fields.
    pub(crate) serde_wire_params: HashMap<String, HashSet<String>>,
    pub(crate) enum_variants: HashMap<String, Vec<(String, VariantPayload)>>,
    /// variant name -> owning enum type (for pattern lowering)
    pub(crate) variant_owner: HashMap<String, String>,
    /// Recursive-type edges that need `Box<…>` in Rust (`(owner, edge_key)`).
    pub(crate) boxed_edges: HashSet<(String, String)>,
    pub(crate) cloneable: HashSet<String>,
    /// D-MIGRATE4: `migration TypeName { … }` blocks per type, in source order
    /// (the chain, oldest step first). Read by `emit_struct_migration` to lower
    /// the runtime step functions + silent decode migration chain-walker.
    pub(crate) migrations: HashMap<String, Vec<crate::AST::MigrationDecl>>,
    /// D-SOA1 / D-SOA-TIER1=A: struct names declared `#layout(columnar)`. A `[S]`
    /// of such a struct lowers to the Prelude-owned `JetColumnList<S>` over THE
    /// shared column store; `rust_type` maps the list type and the list ops route
    /// to that facade's surface.
    pub(crate) columnar: HashSet<String>,
    pub(crate) auto_printable: HashSet<String>,
    pub(crate) auto_debug: HashSet<String>,
    pub(crate) auto_equatable: HashSet<String>,
    /// D-TAG1: types whose fields are all Eq+Hash-capable (comparable minus
    /// float fields) — gates `derive(Eq, Hash)` for `Tally<T>` keys.
    pub(crate) hashable: HashSet<String>,
    pub(crate) patchable: HashSet<String>,
    /// D-FIELDPOL1: struct name -> computed field names. Sema already
    /// synthesized a `fn <field>(self) => T` getter for each on `s.methods`
    /// (`Sema::CheckerFieldPolicy`); this set is consulted at every
    /// `Expr::Field`/`LValue::Field` lowering site so a read of the field
    /// emits a call to that getter instead of a struct member access — the
    /// field simply isn't a Rust struct member (see `emit_struct`).
    pub(crate) computed_fields: HashMap<String, HashSet<String>>,
    /// D-FIELDMEMO1=A: stored computed fields and their result types. The
    /// hidden Rust member is emitted from this one semantic registry.
    pub(crate) memo_fields: HashMap<String, HashMap<String, Type>>,
    /// D-FIELDMEMO1=A: stored field -> memo getters that depend on it,
    /// including transitive computed-field dependencies.
    pub(crate) memo_dependencies: HashMap<String, HashMap<String, HashSet<String>>>,
    pub(crate) src: String,
    pub(crate) file: String,
    /// Rust module alias for this loaded source file, when it is emitted as a
    /// file module.  TIR uses this only to keep the source package's internal
    /// ABI calls distinct from calls into the package's public surface.
    pub(crate) module_alias: String,
    /// Checked namespace identity of this loaded source module
    /// (`package::path` from the name ledger).  This is the identity every
    /// TIR/MIR module reference is keyed by; `module_alias` is only a
    /// Rust-name projection and can collide across packages.
    pub(crate) module_identity: String,
    pub(crate) core_archive_source: bool,
    /// Import alias -> Rust module name (`__jet_scoring`).
    pub(crate) import_mods: HashMap<String, String>,
    /// Generated C-module functions whose wrappers return their declared C
    /// value directly. They do not use the hidden Jet `Result` carrier.
    /// Keys use the emitted module/function path (`__jet_module::function`).
    pub(crate) direct_c_functions: HashSet<String>,
    /// Opaque C handle nominal names whose ABI is a raw pointer at native
    /// boundaries. This is bundle metadata, not a source-level type heuristic.
    pub(crate) opaque_handles: HashSet<String>,
    /// Canonical cross-module nominal identity -> Rust module path. The key
    /// includes package and source-module identity; import aliases are only
    /// source lookup projections and never semantic type identities.
    pub(crate) foreign_types: HashMap<String, String>,
    /// D-MOD4: `(alias, item)` -> `(real Rust module, real fn)` for `pub use`
    /// re-exports, so `text.wrap` lowers to the module that actually defines it.
    pub(crate) reexport_calls: HashMap<(String, String), (String, String)>,
    /// `(import alias, function)` -> parameter conventions for cross-module calls.
    pub(crate) import_sigs: HashMap<(String, String), Vec<(AccessConvention, Type)>>,
    /// c109 Phase 14: `(import alias, function)` -> the function's return type, so the
    /// TIR can carry a total result type for a cross-module call (mirrors `import_sigs`).
    pub(crate) import_rets: HashMap<(String, String), Option<Type>>,
    /// Import alias -> compiler-known core module (`core.files`, `core.encoding`, ...).
    pub(crate) core_imports: HashMap<String, String>,
    /// M10 helpers proven reachable by sema.
    pub(crate) used_core: HashSet<String>,
    /// D-CABI-CALLBACK1: top-level function names sema proved are passed as a
    /// stable C callback symbol at some `#Import` call site. Emission keeps
    /// each Jet function's result carrier and gives it a raw `extern "C"`
    /// trampoline; this set identifies which adapters are needed.
    pub(crate) ffi_callback_fns: HashSet<String>,
    /// Empty at the entry module, `super::` inside generated import modules.
    pub(crate) root_prefix: String,
    /// M7: rustc crate name for the FFI bridge (`jet_ffi_…`).
    pub(crate) ffi_crate: Option<String>,
    /// M7: Jet function name -> wrapper symbol and boundary kind in the FFI crate.
    pub(crate) extern_funcs: HashMap<String, ExternFn>,
    /// D-BOUND-UNDO1=A: foreign binding name -> compensating Jet function.
    /// Lowering reads this fact; engines only marshal the resulting rollback hook.
    pub(crate) foreign_undos: HashMap<String, String>,
    /// D-MOD2: inline code module aliases in scope (alias → module name).
    pub(crate) code_modules: HashSet<String>,
    /// D-MOD3: unqualified inline-module items (name → canonical member name).
    pub(crate) unqualified_inline: HashMap<String, String>,
    /// D-MOD3: unqualified file-module items (name → (rust_mod_name, fn_name)).
    pub(crate) unqualified_file: HashMap<String, (String, String)>,
    /// D-NAME-WALK1=A: per-inline-function unqualified import scopes. The key
    /// is the emitted mangled function name (`module__function`).
    pub(crate) inline_unqualified: HashMap<String, HashMap<String, String>>,
    pub(crate) inline_unqualified_file: HashMap<String, HashMap<String, (String, String)>>,
    /// D-NAME-WALK1=A: per-inline-function Core import scopes. The key is the
    /// emitted mangled function name (`module__function`).
    pub(crate) inline_core_imports: HashMap<String, HashMap<String, String>>,
    /// D-NAME-WALK1=A / D-VERDICT-1867-1: per-inline-function foreign
    /// namespace scopes. The value is the mounted Rust module name.
    pub(crate) inline_foreign_imports: HashMap<String, HashMap<String, String>>,
    /// D-NAME-WALK1=A / D-VERDICT-1867-1: foreign call signatures scoped to
    /// the emitted inline function that declared the import. Keeping this
    /// fact local prevents two inline bodies with the same alias and method
    /// name from overwriting one another.
    pub(crate) inline_foreign_sigs:
        HashMap<String, HashMap<(String, String), Vec<(AccessConvention, Type)>>>,
    pub(crate) inline_foreign_rets: HashMap<String, HashMap<(String, String), Option<Type>>>,
    /// Signature facts for foreign namespaces re-exported by an inline
    /// module, keyed by `(inline module, exported alias, method)`.
    pub(crate) inline_foreign_reexport_sigs:
        HashMap<(String, String, String), Vec<(AccessConvention, Type)>>,
    pub(crate) inline_foreign_reexport_rets: HashMap<(String, String, String), Option<Type>>,
    /// Names from inline scopes used by the conservative TIR coverage gate.
    /// Lowering still reads the exact per-function map.
    pub(crate) inline_import_names: HashSet<String>,
    /// D-NAME-WALK1=A: inline-module pub re-exports of inline functions.
    pub(crate) inline_reexport_inline: HashMap<(String, String), String>,
    /// D-NAME-WALK1=A: inline-module pub re-exports of Core items.
    pub(crate) inline_reexport_core: HashMap<(String, String), (String, String)>,
    /// D-VERDICT-1867-1: inline-module pub re-exports of foreign namespaces.
    /// The key is `(inline module, exported namespace alias)`.
    pub(crate) inline_reexport_foreign: HashMap<(String, String), String>,
    /// S62/M9: (TypeName, method_name) pairs that come from trait impls — these
    /// are called without the `__jet_` prefix in Rust (the trait impl owns the name).
    pub(crate) trait_methods: HashSet<(String, String)>,
    /// S62/M9: the checked trait identity for each static-capable trait method.
    /// TIR carries this fact into MIR; adapters never rediscover a trait by
    /// scanning names or implementation rows.
    pub(crate) trait_method_traits: HashMap<(String, String), String>,
    /// D-OPMIX1: exact operator method facts keyed by
    /// `Traits::operator_method_identity`; unlike `method_sigs`, this table
    /// retains one entry for every RHS pair.
    pub(crate) operator_methods: HashMap<String, OperatorMethod>,
    /// Imported trait definitions Rust method lookup must bring into scope,
    /// keyed by generated module and trait name.
    pub(crate) imported_traits: HashSet<(String, String)>,
    /// D-TXN-ROLLBACK layer 2: user types that implement the `Rollback` trait.
    /// Populated in `build_cx_items` from `Item::Impl` blocks with
    /// `trait_name == Some("Rollback")` and from inline `struct { impl Rollback }`.
    pub(crate) rollback_types: HashSet<String>,
    /// D-DISPLAYDBG1: user types with an explicit `impl Type.Display`.
    pub(crate) display_types: HashSet<String>,
    /// Types with an explicit `impl Type.Debug` (hand `JetDebug` bridged in
    /// `emit_trait_impl`). Union debug arms consult this beside `auto_debug`.
    pub(crate) debug_types: HashSet<String>,
    /// D-SHAPE-RESOURCE2=A: user types with an ordinary nominal `Close` impl.
    pub(crate) close_types: HashSet<String>,
    /// D-ITER-HOOK: `loop x in coll` on types implementing `Iterable`.
    pub(crate) iterable_hooks: HashMap<String, IterableHook>,
    /// D-INDEX-HOOK: `coll[k]` on types implementing `Index`.
    pub(crate) index_hooks: HashMap<String, IndexHook>,
    /// E2-M12 D-OBS1: name of the Jet function currently being emitted, so
    /// jet_panic_rich can include the function name in the panic report.
    pub(crate) current_fn: std::cell::RefCell<String>,
    /// D-MEM-SENTRY1: module/package policy facts carried into TIR lowering.
    pub(crate) policy_declarations: Vec<crate::Policy::PolicyDeclaration>,
    /// D-MEM-GUARANTEE1: package hardening is a build-profile fact, not a
    /// source policy declaration. It enables the shared Prelude sentry in
    /// release builds after the runtime entry initializes it.
    pub(crate) package_hardened: bool,
    /// D-MEM-GUARANTEE1: this module belongs to a dependency named by the
    /// package `contain` dial, so its unsafe gates use the fenced Prelude
    /// scope. The root package itself is never marked dependency-fenced.
    pub(crate) dependency_fenced: bool,
    /// c148: struct name → its declared type-parameter names. Populated in
    /// `build_cx_items` from `StructDef.type_params`. Lets `struct_is_generic` and
    /// field-type checks recognize multi-char type params (`Kind`, `Elem`, …).
    pub(crate) struct_type_params: HashMap<String, HashSet<String>>,
    /// The same parameters in declaration order, for substituting a concrete
    /// `Struct<A, B>` receiver into its declared field types.
    pub(crate) struct_type_param_order: HashMap<String, Vec<String>>,
    /// c148: type-parameter names for the function currently being emitted. Set
    /// from `f.type_params` at the start of `emit_func` so `rust_type` and
    /// `rust_param_type` can recognize multi-char params without the single-letter
    /// heuristic. Cleared when emit returns.
    pub(crate) current_type_params: std::cell::RefCell<HashSet<String>>,
    /// c139 M4: spawn lambda bodies collected during TIR lowering (JIT order).
    pub(crate) jit_spawn_lambdas: std::cell::RefCell<Vec<crate::Codegen::TIR::TJitSpawnLambda>>,
    pub(crate) jit_spawn_sites: std::cell::RefCell<HashMap<(String, usize, usize), usize>>,
    /// Global offset for spawn sites lowered from an imported module.
    pub(crate) jit_spawn_site_base: usize,
    /// Concrete generic owner methods reached while lowering executable TIR.
    /// The key is `Owner<Args>::method`, keeping discovery deterministic.
    pub(crate) jit_method_calls:
        std::cell::RefCell<std::collections::BTreeMap<String, (Type, String, Vec<Type>)>>,
    /// Concrete argument types at calls to generic free functions. TIR uses
    /// these facts to admit one native specialization per concrete call shape.
    pub(crate) jit_generic_calls:
        std::cell::RefCell<std::collections::BTreeMap<String, Vec<Vec<Type>>>>,
    /// Functions whose typed decode depends on the canonical TIR migration
    /// plan. The resident codec has no authority to reinterpret that plan.
    pub(crate) jit_canonical_deopt: std::cell::RefCell<HashSet<String>>,
    /// Functions whose codec calls must stay on TIR if the function deopts.
    pub(crate) jit_canonical_calls: std::cell::RefCell<HashSet<String>>,
    /// Resident-only prefix for calls between functions in an imported module.
    pub(crate) jit_local_call_prefix: Option<String>,
    /// Free-function type parameter names, used to give generic call results
    /// their concrete TIR type at the call site.
    pub(crate) fn_type_params: HashMap<String, HashSet<String>>,
    /// The same free-function parameters in declaration order. HashSet is still
    /// useful for structural binding, but explicit `call<T>(…)` must map each
    /// source argument to the matching binder deterministically.
    pub(crate) fn_type_param_order: HashMap<String, Vec<String>>,
    /// D-DBG3 step 2 (dap-debugger): when true, `lower_stmts` interleaves a
    /// `TStmt::LineMarker` before every lowered statement, and emission turns each
    /// into a `// jet:line N` comment. Set ONLY by the native `jet debug` build path
    /// (`emit_bundle_dbg`); false (the default) is byte-identical to today's output,
    /// so normal builds, `jet test`, and the JIT tier never see a marker.
    pub(crate) debug_linemap: bool,
    /// D-ANY-JAI1/D-VARARGBOUND1 (c7jaiany): trait-bounded variadic function
    /// name -> (fixed param count, resolved trait-bound list). Populated once
    /// from each `Item::Func`'s trailing param; call-site lowering
    /// (`TIR/lower.rs`) reads this to route calls to the typed specialization
    /// built by `Codegen/VariadicBound.rs` instead of ordinary lowering.
    pub(crate) variadic_bound_fns: HashMap<String, (usize, Vec<String>)>,
    /// D-OSTARGET1=A (ratified 2026-07-01, c134): the native OS bucket this
    /// build is compiling for — an `impl` gated to a different `#Target(OS.*)`
    /// is skipped entirely (mirrors how `Codegen/Web.rs` filters by
    /// `WebBucket`). Defaults to the host OS; the real build pipeline
    /// (`emit_bundle_dbg`) overwrites it from the resolved `--target=<triple>`.
    pub(crate) active_os: crate::Syntax::OSTarget,
    /// Web Wasm carries unbounded `Int` in `JetWasmInt`, which is cloneable but
    /// not `Copy`. Native sema sees the same type as a scalar, so shared TIR
    /// emission uses this adapter fact when reading an exact-int field.
    pub(crate) web_wasm_noncopy_int: bool,
    /// D-ENC712: resolved package edition for encoding surface dispatch.
    pub(crate) package_edition: String,
    /// D-DX-PREVIEW1: compiler-owned build/revision facts for inline previews.
    /// Adapters receive these values from checked bundle metadata; source calls
    /// never need to expose a public metadata argument.
    pub(crate) preview_build_id: String,
    pub(crate) preview_revision: String,
    /// D-STM1=A (card #506): true while lowering the body of a `#Transact` block,
    /// so a `Shared<T>.edit(f)` inside it routes to the deferred `edit_txn` (the
    /// atomic Shared plane) instead of taking a lock immediately. Set/restored
    /// around the block body in `lower_stmt`'s `Stmt::Transact` arm.
    pub(crate) in_stm_transact: std::cell::Cell<bool>,
    /// D-STM1=A (card #506): set true when a `Shared.edit` inside the current
    /// `#Transact` body routed to `edit_txn` — i.e. the block actually touches the
    /// Shared plane and so needs the `jet_stm::begin()/commit()` scaffold emitted.
    /// Save/restored per block so each `#Transact` reports its own use.
    pub(crate) stm_touched: std::cell::Cell<bool>,
    /// D-FOUND-BOARD1: selected target profile facts are fixed before TIR lowering.
    /// Hardware call nodes copy canonical IDs from this immutable sema projection.
    pub(crate) hardware_profile: Option<jet_foundation::TargetMachine::TargetHardwareFacts>,
    pub(crate) hardware_profile_id: Option<String>,
}


/// D-ITER-HOOK: metadata for zero-copy `loop x in mytype` lowering.
#[derive(Debug, Clone)]
pub(crate) struct IterableHook {
    pub iter_type: String,
    pub item_type: Type,
    pub iter_symbol: String,
    pub next_symbol: String,
}

/// D-INDEX-HOOK: metadata for expert `mytype[k]` lowering.
#[derive(Debug, Clone)]
pub(crate) struct IndexHook {
    pub value_type: Type,
}

// D-ENC-DYN1=A+: the dynamic encoding value `Data` (+ aliases `JSON`/`TOML`/
// `YAML`/`CSV`) is the user-facing face of `jet_std::DataTree`.
pub(crate) fn is_json_type_name(name: &str) -> bool {
    Syntax::is_data_type_name(name)
}

// D-DBDRIVER1: the `DBValue` dynamic tagged SQL value — same construction
// mechanism as `Data`/`JSON`, mirrored via `jet_std::DBValue`.
pub(crate) fn is_db_value_type_name(name: &str) -> bool {
    Syntax::is_db_value_type_name(name)
}
/// Core UI runtime carriers are generated at the crate root from the shared
/// `CoreModuleExports` registry. Result aliases intentionally stay on their
/// dedicated `JetUiServiceResult<...>` projections below.
pub(crate) fn core_ui_rust_type_name(name: &str) -> Option<&str> {
    const RESULT_ALIASES: &[&str] = &[
        "UiServiceResult",
        "UiFileFilterResult",
        "UiFsGrantResult",
        "UiShortcutResult",
        "UiShortcutBindingResult",
        "UiAccessibilityResult",
        "UiFileDialogResult",
        "UiClipboardTextResult",
        "UiClipboardWriteResult",
        "UiImeResult",
        "UiDragResult",
        "UiShortcutDispatchResult",
        "UiAccessibilityNodeResult",
        "UiAccessibilityAttachResult",
        "UiAccessibilityProjectionResult",
    ];
    if RESULT_ALIASES.contains(&name) {
        return None;
    }
    jet_foundation::CoreModuleExports::core_modules()
        .iter()
        .filter(|entry| matches!(entry.module, "core.ui" | "core.ui.host"))
        .any(|entry| {
            entry
                .type_exports
                .iter()
                .any(|&(type_name, _)| type_name == name)
        })
        .then_some(name)
}

/// Prelude values emitted at the generated crate root rather than under
/// `jet_std`. Keep this separate from `core_rust_type_name`: that table's caller
/// always inserts `jet_std::`.
pub(crate) fn root_prelude_rust_type_name(name: &str) -> Option<&str> {
    match name {
        n if n == Syntax::TYPE_MEMO_STATS => Some("JetMemoStats"),
        n if n == Syntax::TYPE_AUTHORITY => Some("JetAuthority"),
        n if n == Syntax::DETERMINISTIC_WORLD_TYPE => Some("JetDeterministicWorld"),
        n if n == Syntax::TYPE_ERR => Some("JetErr"),
        "Transaction" => Some("JetTransaction"),
        "Loadable" => Some("JetLoadable"),
        n if n == Syntax::TYPE_RANGE => Some("JetRange"),
        n if n == Syntax::TYPE_ITER => Some("JetIter"),
        "Point" => Some("JetPoint"),
        "Size" => Some("JetSize"),
        "Rect" => Some("JetRect"),
        "SizeConstraint" => Some("JetSizeConstraint"),
        "UiNode" => Some("JetUiNode"),
        "FontStyle" => Some("JetFontStyle"),
        "FontFace" => Some("JetFontFace"),
        "Glyph" => Some("JetGlyph"),
        "GlyphRun" => Some("JetGlyphRun"),
        "GlyphShaper" => Some("JetGlyphShaper"),
        "UiAriaRole" => Some("JetAriaRole"),
        "InputEvent" => Some("JetInputEvent"),
        "EventResult" => Some("JetEventResult"),
        "NullBackend" => Some("JetNullBackend"),
        "TuiBackend" => Some("JetTuiBackend"),
        "GtkBackend" => Some("JetGtkBackend"),
        // D-APPROX1=A: native sketch carriers are Prelude-root values.
        "HyperLogLog" => Some("JetHyperLogLog"),
        "TDigest" => Some("JetTDigest"),
        "CountMinSketch" => Some("JetCountMinSketch"),
        "ReservoirSampler" => Some("JetReservoirSampler"),
        "JetDate" | "JetInstant" | "JetLocalTime" | "JetDateTime" | "JetPeriod"
        | "JetZone" | "JetZonedDateTime" => Some(name),
        "WebMutationStatus" => Some("JetWebMutationStatus"),
        "WebMutationState" => Some("JetWebMutationState"),
        "WebQueryStatus" => Some("JetWebQueryStatus"),
        "WebQueryNetworkMode" => Some("JetWebQueryNetworkMode"),
        "WebQueryState" => Some("JetWebQueryState"),
        "WebQuery" => Some("JetWebQuery"),
        "WebStore" => Some("JetWebStore"),
        "WebStoreTransaction" => Some("JetWebStoreTransaction"),
        "WebStorePatch" => Some("JetWebStorePatch"),
        "WebStoreInspection" => Some("JetWebStoreInspection"),
        "WebStoreEvent" => Some("JetWebStoreEvent"),
        "WebStoreSubscription" => Some("JetWebStoreSubscription"),
        "WebFormValueType" => Some("JetWebFormValueType"),
        "WebFormStatus" => Some("JetWebFormStatus"),
        "WebFormFieldState" => Some("JetWebFormFieldState"),
        "WebFormState" => Some("JetWebFormState"),
        "WebForm" => Some("JetWebForm"),
        "WebFormValidation" => Some("JetWebFormValidation"),
        "WebFormControl" => Some("JetWebFormControl"),
        "WebFormValidationTiming" => Some("JetWebFormValidationTiming"),
        "WebFormFieldSpec" => Some("JetWebFormFieldSpec"),
        "WebFormInput" => Some("JetWebFormInput"),
        "WebFormDecodedInput" => Some("JetWebFormDecodedInput"),
        "WebFormActionError" => Some("JetWebFormActionError"),
        "WebFormLifecycleStatus" => Some("JetWebFormLifecycleStatus"),
        "WebFormLifecycle" => Some("JetWebFormLifecycle"),
        "WebFormErrorState" => Some("JetWebFormErrorState"),
        "WebFormTypedAction" => Some("JetWebFormTypedAction"),
        "WebFormTyped" => Some("JetWebFormTyped"),
        "WebFormTypedValidation" => Some("JetWebFormTypedValidation"),
        "WebFormTypedSubmission" => Some("JetWebFormTypedSubmission"),
        "WebTableSortDirection" => Some("JetWebTableSortDirection"),
        "WebTableSort" => Some("JetWebTableSort"),
        "WebTableFilter" => Some("JetWebTableFilter"),
        "WebTablePageMode" => Some("JetWebTablePageMode"),
        "WebTableStatus" => Some("JetWebTableStatus"),
        "WebTableProjection" => Some("JetWebTableProjection"),
        "WebTableState" => Some("JetWebTableState"),
        "WebTableColumn" => Some("JetWebTableColumn"),
        "WebTableRow" => Some("JetWebTableRow"),
        "WebTablePage" => Some("JetWebTablePage"),
        "WebTable" => Some("JetWebTable"),
        "WebVirtualViewport" => Some("JetWebVirtualViewport"),
        "WebVirtualWindow" => Some("JetWebVirtualWindow"),
        "WebVirtualPlan" => Some("JetWebVirtualPlan"),
        "WebVirtualPlanViewport" => Some("JetWebVirtualPlanViewport"),
        "Atomic" => Some("JetAtomic"),
        "JetDataPlotMark"
        | "JetDataPlotChannel"
        | "JetDataPlotAggregate"
        | "JetDataPlotFilterOp"
        | "JetDataPlotValue"
        | "JetDataPlotScaleKind"
        | "JetDataPlotDomain"
        | "JetDataPlotLegendPosition"
        | "JetDataPlotFacetKind"
        | "JetDataPlotInteraction"
        | "JetDataPlotBackend"
        | "JetDataPlotSupport"
        | "JetDataPlotErrorKind"
        | "JetDataPlotRenderFormat"
        | "JetDataPlotField"
        | "JetDataPlotSchema"
        | "JetDataPlotSourceFacts"
        | "JetDataPlotEncoding"
        | "JetDataPlotScale"
        | "JetDataPlotAxis"
        | "JetDataPlotLegend"
        | "JetDataPlotFacet"
        | "JetDataPlotLayer"
        | "JetDataPlotAccessibility"
        | "JetDataPlotLayout"
        | "JetDataPlotCapability"
        | "JetDataPlotError"
        | "JetDataPlotPlan"
        | "JetDataPlotSelectedRow"
        | "JetDataPlotInspection"
        | "JetDataPlotRender"
        | "JetDataPlotProjection"
        | "JetDataPlotColumn"
        | "JetDataPlot"
        => Some(name),
        n if job_queue_rust_type(n).is_some() => job_queue_rust_type(n),
        _ => net_handle_rust_type(name),
    }
}

/// D-DX-QUEUE1=A: durable job queue values are flat Prelude carriers, not
/// `jet_std` values. `JobQueue` is opened with the static provider lifetime.
pub(crate) fn job_queue_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "JobQueue" => Some("JetJobQueue<'static>"),
        "JobPayload" => Some("JetJobPayload"),
        "JobResult" => Some("JetJobResult"),
        "JobError" => Some("JetJobError"),
        "JobQueueReceipt" => Some("JetJobQueueReceipt"),
        "JobQueueClaim" => Some("JetJobQueueClaim"),
        "JobQueueEvent" => Some("JetJobQueueEvent"),
        "JobQueueRecord" => Some("JetJobQueueRecord"),
        "JobQueueStatus" => Some("JetJobQueueStatus"),
        "JobQueueState" => Some("JetJobQueueState"),
        "JobQueueDeliveryPolicy" => Some("JetJobQueueDeliveryPolicy"),
        _ => None,
    }
}

/// Rust carriers for the public history-testing records. They live beside the
/// generated program in the embedded Foundation module, not in `jet_std`.
pub(crate) fn history_rust_type_name(name: &str) -> Option<&'static str> {
    match name.rsplit_once("::").or_else(|| name.rsplit_once('.')) {
        Some((prefix, leaf))
            if prefix == "core"
                || prefix.starts_with("core.")
                || prefix.starts_with("core::") =>
        {
            history_rust_type_name(leaf)
        }
        Some(_) => None,
        None => match name {
            "Count" => Some("Count"),
            "HandleId" => Some("HandleId"),
            "TaskId" => Some("TaskId"),
            "EventId" => Some("EventId"),
            "HistoryRng" => Some("HistoryRng"),
            "HistoryValue" => Some("HistoryValue"),
            "HistoryPrecondition" => Some("HistoryPrecondition"),
            "HistoryCase" => Some("HistoryCase"),
            "HistoryOperation" => Some("HistoryOperation"),
            "HistoryScheduleChoice" => Some("HistoryScheduleChoice"),
            "HistoryBounds" => Some("HistoryBounds"),
            "HistoryDistribution" => Some("HistoryDistribution"),
            "HistoryStrategy" => Some("HistoryStrategy"),
            "TypedHistoryCase" => Some("TypedHistoryCase"),
            _ => None,
        },
    }
}

pub(crate) fn core_rust_type_name(name: &str) -> Option<&'static str> {
    // MIR keeps imported core nominals qualified by their canonical module
    // (`core.time::LocalDate`), while source-facing lowering commonly carries
    // the bare leaf. Normalize only a `core` path here; arbitrary qualified
    // user/import names must not become Prelude types by leaf coincidence.
    let name = match name.rsplit_once("::").or_else(|| name.rsplit_once('.')) {
        Some((prefix, leaf))
            if prefix == "core"
                || prefix.starts_with("core.")
                || prefix.starts_with("core::") =>
        {
            leaf
        }
        Some(_) => return None,
        None => name,
    };
    match name {
        n if is_json_type_name(n) => Some("DataTree"),
        n if n == Syntax::TYPE_IO_ERROR || n == "IOError" => Some("IOError"),
        n if n == Syntax::TYPE_IO_CONTEXT => Some("IOContext"),
        n if n == Syntax::TYPE_IO_OPERATION => Some("IOOperation"),
        n if n == Syntax::TYPE_PROCESS_RESOURCE_LIMIT => Some("ProcessResourceLimit"),
        "EnvError" => Some("EnvError"),
        n if n == Syntax::TYPE_UTF8_ERROR || n == "UTF8Error" => Some("UTF8Error"),
        "ProcessResult" | "ProcessReceipt" => Some("ProcessReceipt"),
        "ProcessPlan" => Some("ProcessPlan"),
        "ProcessSpec" => Some("ProcessSpec"),
        "ProcessChild" => Some("ProcessChild"),
        // D-PROCESS1=A: the core dot-literal stream-mode enum.
        "ProcessStreamMode" => Some("ProcessStreamMode"),
        // D-PROCESS-SESSION1=A / D-PROCESS-SESSION2=D: expert terminal
        // controls on the existing process model.
        "TerminalPolicy" => Some("TerminalPolicy"),
        "TerminalSize" => Some("TerminalSize"),
        "TerminalMode" => Some("TerminalMode"),
        "TerminalSession" => Some("TerminalSession"),
        // D-TEXTWIDTH1=B: the `text.display_width` policy value + its two
        // dot-literal enum fields, plus the reject-path error struct.
        "TextWidth" => Some("TextWidth"),
        "TextWidthAmbiguous" => Some("TextWidthAmbiguous"),
        "TextWidthControls" => Some("TextWidthControls"),
        "TextError" => Some("TextError"),
        "Query" => Some("DataQuery"),
        "DataGroupedQuery" => Some("DataGroupedQuery"),
        "Group" => Some("GroupValue"),
        "DataJoin" => Some("DataJoin"),
        n if n == Syntax::TYPE_SHARED_GUARD => Some("JetSharedGuard"),
        "Signal" => Some("JetSignal"),
        "Derived" => Some("JetDerived"),
        "AsyncPolicy" => Some("JetAsyncPolicy"),
        "Event" => Some("JetEvent"),
        "AsyncEvent" => Some("JetAsyncEvent"),
        "Hook" => Some("JetHook"),
        "DecisionHook" => Some("JetDecisionHook"),
        "HookDecision" => Some("JetHookDecision"),
        "HookOutcome" => Some("JetHookOutcome"),
        "Subscription" => Some("JetSubscription"),
        "EventScope" => Some("JetEventScope"),
        "EventPolicy" => Some("JetEventPolicy"),
        "EventTrace" => Some("JetEventTrace"),
        "DispatchReport" => Some("JetDispatchReport"),
        "DispatchFailure" => Some("JetDispatchFailure"),
        "Overflow" => Some("JetEventOverflow"),
        "FailurePolicy" => Some("JetFailurePolicy"),
        "DispatchState" => Some("JetDispatchState"),
        "HookPolicy" => Some("JetHookPolicy"),
        "EventConfigError" => Some("JetEventConfigError"),
        "Stopwatch" => Some("Stopwatch"),
        "TestSuite" => Some("JetTestSuite"),
        // D-DET1: deterministic injected capability handles.
        "Clock" => Some("Clock"),
        "Rng" => Some("Rng"),
        "Fake" => Some("Fake"),
        // D-SOLVER-LIB1=A: explicit finite solver state.
        "Solver" => Some("Solver"),
        // D-SHAPE-DURATION1/D-SHAPE-DURATIONCONVERT1: checked duration values.
        "Duration" => Some("Duration"),
        "DurationUnit" => Some("DurationUnit"),
        // D-FOUND-COREAPI1 / #2853: event-time late-event policy carrier.
        "LateEventDisposition" => Some("JetLateEventDisposition"),
        "RangeError" => Some("RangeError"),
        "Instant" => Some("JetInstant"),
        "Date" | "LocalDate" => Some("JetDate"),
        "LocalTime" => Some("JetLocalTime"),
        "DateTime" => Some("JetDateTime"),
        "Period" => Some("JetPeriod"),
        "Zone" => Some("JetZone"),
        "ZonedDateTime" => Some("JetZonedDateTime"),
        "Url" => Some("JetURL"),
        "Mime" => Some("JetMIME"),
        "DataLoaderKind" => Some("DataLoaderKind"),
        "DataFreshness" => Some("DataFreshness"),
        "DataInvalidationCause" => Some("DataInvalidationCause"),
        "DataAuthority" => Some("DataAuthority"),
        "DataSourceIdentity" => Some("DataSourceIdentity"),
        "DataProvenance" => Some("DataProvenance"),
        "DataSnapshotIdentity" => Some("DataSnapshotIdentity"),
        "DataLoaderStatus" => Some("DataLoaderStatus"),
        "DataLoader" => Some("DataLoader"),
        "DataSnapshot" => Some("DataSnapshot"),
        "Regex" => Some("JetRegex"),
        "RegexFlags" => Some("RegexFlags"),
        "Match" => Some("JetRegexMatch"),
        // D-DECIMAL1 / D-NUMTYPE1=A: precise numerics.
        "Decimal" => Some("JetDecimal"),
        "Fraction" => Some("JetFraction"),
        // D-CONC-CHAN1=A / D-MEM1 S6: generic runtime handle carriers.
        n if n == Syntax::TYPE_RECEIVER => Some("JetReceiver"),
        n if n == Syntax::TYPE_SENDER => Some("JetSender"),
        "Closed" => Some("Closed"),
        "Pool" => Some("JetPool"),
        "Id" => Some("JetId"),
        n if n == Syntax::TYPE_TASK_FAILURE => Some("JetTaskFailure"),
        // D-LSDIR1=A: fs.list_dir returns [DirEntry].
        "DirEntry" => Some("DirEntry"),
        // D-FSOPS1 / D-WATCH-SCOPE1: typed filesystem and watcher values.
        "Stat" => Some("Stat"),
        "WalkEntry" => Some("WalkEntry"),
        "WatchEvent" => Some("WatchEvent"),
        // stdlib-api-laws D4: `WatchEvent`'s two closed field enums.
        "WatchDomain" => Some("WatchDomain"),
        "WatchKind" => Some("WatchKind"),
        "WatchHandle" => Some("WatchHandle"),
        "WatchSet" => Some("WatchSet"),
        // D-FFI-CALLBACK2=A: generated managed callback carriers.
        "FfiCallbackEvent" => Some("JetFfiCallbackEventValue"),
        "FfiCallbackRegistration" => Some("JetFfiCallbackRegistrationHandle"),
        "TempFile" => Some("TempFile"),
        "FileLock" => Some("FileLock"),
        "MappedFile" => Some("JetMappedFile"),
        // D-DX-QUEUE1=A: queue values are flat Prelude carriers. The MIR
        // backend handles these root names without a `jet_std` namespace.
        n if job_queue_rust_type(n).is_some() => job_queue_rust_type(n),
        // D-DATA-SURFACE1=A / D-DATA-STATUS1=A / D-DATA-PLOT1=A: core.data values.
        "DataLineOptions" => Some("DataLineOptions"),
        "DataColumn" => Some("DataColumn"),
        "DataStatus" => Some("DataStatus"),
        "DataSummary" => Some("DataSummary"),
        // D-DATAFLOW1=A: typed streaming + invalid-data policy values.
        "DataLimits" => Some("DataLimits"),
        "DataError" => Some("DataError"),
        "DataErrorKind" => Some("DataErrorKind"),
        "DataPivotCell" => Some("DataPivotCell"),
        "DataStream" => Some("DataStream"),
        // D-LOGTRACE1=A: structured logging values.
        "LogField" => Some("LogField"),
        "LogSpan" => Some("LogSpan"),
        // D-SERDE2 / D-VALIDATE-DECODE1: the value tree and accumulated typed
        // decode errors live in jet_std.
        "DataTree" => Some("DataTree"),
        "EncodingLimits" => Some("EncodingLimits"),
        "EncodingError" => Some("EncodingError"),
        "EncodingCause" => Some("EncodingCause"),
        "EncodingFormat" => Some("EncodingFormat"),
        "EncodingErrorKind" => Some("EncodingErrorKind"),
        // D-VALIDATE1: the accumulated validation error lives in jet_std too.
        "FieldError" => Some("FieldError"),
        "CBOROptions" => Some("CBOROptions"),
        "CBORError" => Some("CBORError"),
        "CBORErrorKind" => Some("CBORErrorKind"),
        "XMLLimits" => Some("XMLLimits"),
        "XMLParseOptions" => Some("XMLParseOptions"),
        "XMLRenderOptions" => Some("XMLRenderOptions"),
        "XMLEncoding" => Some("XMLEncoding"),
        "XMLLexicalPolicy" => Some("XMLLexicalPolicy"),
        "XMLCanonical" => Some("XMLCanonical"),
        "XMLCanonicalMode" => Some("XMLCanonicalMode"),
        "XMLError" => Some("XMLError"),
        "XMLReason" => Some("XMLReason"),
        "XMLEntityPolicy" => Some("XMLEntityPolicy"),
        "DataEvent" => Some("DataEvent"),
        "JSONReader" => Some("JSONReader"),
        "JSONWriter" => Some("JSONWriter"),
        "JSONLReader" => Some("JSONLReader"),
        "JSONLWriter" => Some("JSONLWriter"),
        "CSVReader" => Some("CSVReader"),
        "CSVWriter" => Some("CSVWriter"),
        "CSVRow" => Some("CSVRow"),
        "XMLReader" => Some("XMLReader"),
        "XMLWriter" => Some("XMLWriter"),
        "CBORReader" => Some("CBORReader"),
        "CBORWriter" => Some("CBORWriter"),
        // D-DBDRIVER1: the tagged SQL parameter/column value + its error type.
        "DBValue" => Some("DBValue"),
        "DBError" => Some("DBError"),
        // D-RAYLIB1=A: display-gated graphics bridge types.
        "RaylibWindow" => Some("RaylibWindow"),
        "RaylibColor" => Some("RaylibColor"),
        "RaylibSound" => Some("RaylibSound"),
        "RaylibTextureAtlas" => Some("RaylibTextureAtlas"),
        // D-TYPEDSQL-SINK1=A: `SQL` is a checked `(String, Vec<DBValue>)`
        // carrier. It remains a known core value type for subset gating; the
        // explicit Rust spelling comes from the `rust_type` arm below.
        "SQL" => Some("SQL"),
        "HTML" => Some("HTML"),
        "Sh" => Some("Sh"),
        // D-SIMD1/D-SIMD2/D-SIMD3: built-in math value types (lane + linalg
        // structs). The explicit arms keep this function's static return type
        // while Syntax owns the user-facing lane registry.
        "F32x4" => Some("F32x4"),
        "F64x2" => Some("F64x2"),
        "F32x8" => Some("F32x8"),
        "F64x4" => Some("F64x4"),
        "I8x16" => Some("I8x16"),
        "I16x8" => Some("I16x8"),
        "I32x4" => Some("I32x4"),
        "I64x2" => Some("I64x2"),
        "U8x16" => Some("U8x16"),
        "U16x8" => Some("U16x8"),
        "U32x4" => Some("U32x4"),
        "U64x2" => Some("U64x2"),
        "I8x32" => Some("I8x32"),
        "I16x16" => Some("I16x16"),
        "I32x8" => Some("I32x8"),
        "I64x4" => Some("I64x4"),
        "U8x32" => Some("U8x32"),
        "U16x16" => Some("U16x16"),
        "U32x8" => Some("U32x8"),
        "U64x4" => Some("U64x4"),
        "Vec2" => Some("Vec2"),
        "Vec3" => Some("Vec3"),
        "Vec4" => Some("Vec4"),
        "Mat3" => Some("Mat3"),
        "Mat4" => Some("Mat4"),
        // D-SPACE-GEOMETRY1=A: coordinate tags erase to compact Prelude
        // carriers; the nominal space is a sema fact, not a heap object.
        "Point2" | "Delta2" | "ScreenPoint" | "WorldPoint" | "ViewPoint"
        | "CameraPoint" | "DevicePoint" | "ScreenDelta" | "WorldDelta"
        | "ViewDelta" | "CameraDelta" | "DeviceDelta" => Some("JetCoord2"),
        "Transform" | "Transform2" => Some("JetTransform2"),
        "Ray2" => Some("JetRay2"),
        _ => None,
    }
}

/// Core email types are emitted by the `jet_email` Prelude module, not by
/// `jet_std`. Qualified Core spellings retain their `core.email` owner, while
/// compiler-owned declaration rows use the bare leaf. Keep both forms on the
/// same Rust spelling and do not let arbitrary qualified user names acquire
/// the Core email carrier by leaf coincidence.
pub(crate) fn core_email_rust_type_name(name: &str) -> Option<&'static str> {
    let leaf = match name
        .rsplit_once("::")
        .or_else(|| name.rsplit_once('.'))
    {
        Some((prefix, leaf))
            if matches!(prefix, "email" | "core.email" | "core::email") =>
        {
            leaf
        }
        Some(_) => return None,
        None => name,
    };
    match leaf {
        "Address" => Some("Address"),
        "Message" => Some("Message"),
        "Attachment" => Some("Attachment"),
        "Envelope" => Some("Envelope"),
        "SMTPSecurity" => Some("SMTPSecurity"),
        "RecipientPolicy" => Some("RecipientPolicy"),
        "RecipientReport" => Some("RecipientReport"),
        "SendReport" => Some("SendReport"),
        "EmailError" => Some("Error"),
        "Limits" => Some("Limits"),
        "SMTPAuth" => Some("SMTPAuth"),
        "TLSTrust" => Some("TLSTrust"),
        "DkimConfig" => Some("DkimConfig"),
        "SMTPConfig" => Some("SMTPConfig"),
        "Mailer" => Some("Mailer"),
        _ => None,
    }
}

pub(crate) fn core_crypto_type_name(name: &str) -> Option<&'static str> {
    match name {
        "Secret" => Some("Secret"),
        "SigningKey" => Some("SigningKey"),
        "VerifyKey" => Some("VerifyKey"),
        "X25519SecretKey" => Some("X25519SecretKey"),
        "X25519PublicKey" => Some("X25519PublicKey"),
        "SharedSecret" => Some("SharedSecret"),
        "Signature" => Some("Signature"),
        "Sealed" => Some("Sealed"),
        "WrappedKey" => Some("WrappedKey"),
        "WrappedVaultKey" => Some("WrappedVaultKey"),
        "KeyUnlock" => Some("KeyUnlock"),
        "PasswordHash" => Some("PasswordHash"),
        "Digest256" => Some("Digest256"),
        "Digest512" => Some("Digest512"),
        "Hasher" => Some("Hasher"),
        "CryptoError" => Some("CryptoError"),
        "FileCryptoError" => Some("FileCryptoError"),
        "KeyWrapError" => Some("KeyWrapError"),
        _ => None,
    }
}

pub(crate) fn core_crypto_rust_type_name(name: &str) -> Option<&'static str> {
    let name = core_crypto_type_name(name)?;
    match name {
        "Secret" => Some("Secret"),
        "SigningKey" => Some("JetSigningKey"),
        "VerifyKey" => Some("JetVerifyKey"),
        "X25519SecretKey" => Some("JetX25519SecretKey"),
        "X25519PublicKey" => Some("JetX25519PublicKey"),
        "SharedSecret" => Some("JetSharedSecret"),
        "Signature" => Some("JetSignature"),
        "Sealed" => Some("JetSealed"),
        "WrappedKey" => Some("JetWrappedKey"),
        "WrappedVaultKey" => Some("JetWrappedVaultKey"),
        "KeyUnlock" => Some("JetVaultKeyUnlock<'_>"),
        "PasswordHash" => Some("JetPasswordHash"),
        "Digest256" => Some("JetDigest256"),
        "Digest512" => Some("JetDigest512"),
        "CryptoError" => Some("JetCryptoError"),
        "FileCryptoError" => Some("JetFileCryptoError"),
        "KeyWrapError" => Some("JetVaultKeyWrapError"),
        _ => None,
    }
}

/// D-LAYOUT1 / D-LAYOUT-GATES1: `layout` runtime types live in their own
/// top-level `mod jet_layout` (NOT nested inside `mod jet_std`, unlike
/// `core_rust_type_name`'s entries) — same reason `alloc_handle_rust_type`/
/// `file_handle_rust_type`/`net_handle_rust_type` are separate functions
/// rather than folded into `core_rust_type_name` (that table's caller always
/// prepends `jet_std::`). `HVar`/`VVar`/`LengthVar` all erase to the SAME
/// runtime type (`jet_layout::LinExpr`) — the axis distinction is
/// compile-time-only (sema, GATE 1/2), nothing to represent at runtime.
pub(crate) fn layout_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "HVar" => Some("jet_layout::LinExpr"),
        "VVar" => Some("jet_layout::LinExpr"),
        "LengthVar" => Some("jet_layout::LinExpr"),
        "Constraint" => Some("jet_layout::Constraint"),
        "Layout" => Some("jet_layout::Handle"),
        _ => None,
    }
}

/// E2-M7: file handle types are top-level in the prelude (not in `jet_std`).
pub(crate) fn file_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "FileReader" => Some("JetFileReader"),
        "FileWriter" => Some("JetFileWriter"),
        // FileLines is an internal sema marker; it should never appear in emitted Rust.
        "FileLines" => Some("()"),
        // D-STDIN1=A: stdin handle types; StdinLines is an internal sema marker.
        "StdinHandle" => Some("JetStdinReader"),
        "StdinLines" => Some("()"),
        // D-PROCESS1=A: these markers project the concrete shared handles on
        // ProcessChild. Keep their native storage types in generated Rust so
        // method receivers match the process prelude helpers.
        "ProcessStdin" => Some("jet_std::ProcessStdinHandle"),
        "ProcessStdoutStream" => Some("jet_std::ProcessStdoutStreamHandle"),
        "ProcessStderrStream" => Some("jet_std::ProcessStderrStreamHandle"),
        // ProcessLines is the `.lines()` loop-only marker.
        "ProcessLines" => Some("()"),
        // D-COREIO1=A: standard stream handles.
        "Stdout" => Some("JetStdout"),
        "Stderr" => Some("JetStderr"),
        // D-PATHFS1: typed path handle.
        "Path" => Some("JetPath"),
        // D-FILESCOPE1=A: scoped filesystem handles are top-level prelude
        // values and are borrowed by scope-bound file operations.
        "FileScope" => Some("JetFileScope"),
        // D-DBDRIVER1: the SQLite connection handle wrapper.
        "DBConnection" => Some("JetDbConnection"),
        "DBScope" => Some("JetDbScope"),
        "DbPool" => Some("JetDbPool<JetDbConnection>"),
        "DbLease" => Some("JetDbLease<JetDbConnection>"),
        "DbPoolReceipt" => Some("JetDbPoolReceipt"),
        "DbPoolLifecycle" => Some("JetDbPoolLifecycle"),
        // D-DEP-WASM1=A / D-PLUGIN1=B (c81): the sandboxed WASM plugin handle.
        "Plugin" => Some("JetPlugin"),
        // D-LIB-CALLGRANT1=A: loaded libraries are opaque handles; the grant
        // is a small constructable prelude record.
        "Mod" => Some("JetMod"),
        "ModGrant" => Some("JetModGrant"),
        _ => None,
    }
}

/// D-RAYLIB1=A: raylib handle/value types are top-level prelude
/// structs, like file/net handles, not members of `mod jet_std`.
pub(crate) fn raylib_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "RaylibWindow" => Some("RaylibWindow"),
        "RaylibColor" => Some("RaylibColor"),
        "RaylibSound" => Some("RaylibSound"),
        "RaylibTextureAtlas" => Some("RaylibTextureAtlas"),
        _ => None,
    }
}

pub(crate) fn game_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "GameScene" => Some("GameScene"),
        "GameAssets" => Some("GameAssets"),
        "GameInputMap" => Some("GameInputMap"),
        "GameBackend" => Some("GameBackend"),
        "GameReplay" => Some("GameReplay"),
        "GameImage" => Some("GameImage"),
        "GameSound" => Some("GameSound"),
        "GameFrame" => Some("GameFrame"),
        "GameInputSnapshot" => Some("GameInputSnapshot"),
        _ => None,
    }
}

/// D-COMPUTE1=D: compute opaque types map to top-level prelude structs.
pub(crate) fn compute_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "Tensor" => Some("JetTensor"),
        "ComputeError" => Some("JetComputeError"),
        "ComputeDevice" => Some("JetComputeDevice"),
        "ComputeStream" => Some("JetComputeStream"),
        "VjpRun" => Some("JetComputeVjpRun"),
        "SparseTensor" => Some("JetSparseCsr"),
        _ => None,
    }
}

/// D-SERVICE1=D: service topology opaque types.
pub(crate) fn service_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "ServiceTree" => Some("JetServiceTree"),
        "ServiceWorkflow" => Some("JetWorkflowHandle"),
        "ServiceEndpoint" => Some("JetServiceEndpoint"),
        "ServiceError" => Some("JetServiceError"),
        "ServiceRestart" => Some("JetServiceRestart"),
        "ServiceDelivery" => Some("JetServiceDelivery"),
        "ServiceRuntime" => Some("JetServiceRuntime"),
        "ServiceStateStore" => Some("JetServiceStateStore"),
        "ServiceUpgradeReceipt" => Some("JetServiceUpgradeReceipt"),
        "Delivery" => Some("JetDelivery"),
        "DeliveryState" => Some("JetDeliveryState"),
        "DeliveryReceipt" => Some("JetDeliveryReceipt"),
        "DeliveryEvent" => Some("JetDeliveryEvent"),
        "TaskOutcome" => Some("JetTaskOutcome"),
        "TaskStatus" => Some("JetTaskStatus"),
        _ => None,
    }
}

/// E2-M10: networking opaque types map to top-level prelude structs.
pub(crate) fn net_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "TcpListener" => Some("JetTCPListener"),
        "TcpStream" => Some("JetTCPStream"),
        "IPAddr" => Some("JetIpAddr"),
        "SocketAddr" => Some("JetSocketAddr"),
        "UdpSocket" => Some("JetUDPSocket"),
        "UDPPacket" => Some("JetUDPPacket"),
        "DNSSrv" => Some("JetDNSSrv"),
        "UnixListener" => Some("JetUnixListener"),
        "UnixStream" => Some("JetUnixStream"),
        "TLSStream" => Some("JetTLSStream"),
        "TLSClientConfig" => Some("JetTLSClientConfig"),
        "TLSRootCertificates" => Some("JetTLSRootCertificates"),
        "TLSClientIdentity" => Some("JetTLSClientIdentity"),
        "TLSClientTrust" => Some("JetTLSTrust"),
        "TLSVersion" => Some("JetTLSVersion"),
        "TLSPeerIdentity" => Some("JetTLSPeerIdentity"),
        "TLSCertificate" => Some("JetTLSCertificate"),
        "NetError" => Some("JetNetError"),
        "NetErrorDetail" => Some("JetNetErrorDetail"),
        "NetDnsError" => Some("JetNetDnsError"),
        "NetShutdown" => Some("JetNetShutdown"),
        "NetReadyInterest" => Some("JetNetReadyInterest"),
        "NetReady" => Some("JetNetReady"),
        "HTTPRequest" => Some("JetHTTPRequest"),
        "HTTPResponse" => Some("JetHTTPResponse"),
        "HTTPBodyChunks" => Some("JetHTTPBodyChunks"),
        "HTTPClient" => Some("JetHTTPClient"),
        "HTTPProxy" => Some("JetHTTPProxy"),
        "HTTPRedirectPolicy" => Some("JetHTTPRedirectPolicy"),
        "HTTPRetryPolicy" => Some("JetHTTPRetryPolicy"),
        "HTTPCookieJar" => Some("JetHTTPCookieJar"),
        "HTTPCorsPolicy" => Some("JetHTTPCorsPolicy"),
        "HTTPCorsOrigins" => Some("JetHTTPCorsOrigins"),
        "HTTPCompressEncoding" => Some("JetHTTPCompressEncoding"),
        "HTTPRouter" => Some("JetHTTPRouter"),
        "HTTPMethod" => Some("JetHTTPMethod"),
        "HTTPStatus" => Some("JetHTTPStatus"),
        "HTTPVersion" => Some("JetHTTPVersion"),
        "HTTPHeaderName" => Some("JetHTTPHeaderName"),
        "HTTPHeaderValue" => Some("JetHTTPHeaderValue"),
        "HTTPBody" => Some("JetHTTPBody"),
        "HTTPError" => Some("JetHTTPError"),
        "HTTPOperation" => Some("JetHTTPOperation"),
        "HTTPHeaders" => Some("JetHTTPHeaders"),
        "HTTPMux" => Some("JetHTTPMux"),
        "HTTPHandler" => Some("JetHTTPHandler"),
        "HTTPServer" => Some("JetHTTPServer"),
        "HTTPShutdownReport" => Some("JetHTTPShutdownReport"),
        "HTTPServerTls" => Some("JetHTTPServerTls"),
        // D-WS1=B: WebSocket values are Prelude-owned runtime carriers.
        "WsConn" => Some("JetWsConn"),
        "WsError" => Some("JetWsError"),
        "WsMessage" => Some("JetWsMessage"),
        _ => None,
    }
}

// Re-export from Syntax so submodules (lower.rs, subset.rs) find them via `use super::*`.
pub(crate) use crate::Syntax::alloc_handle_rust_type;
pub(crate) use crate::Syntax::args_handle_rust_type;
pub(crate) use crate::Syntax::binary_text_handle_rust_type;
pub(crate) use crate::Syntax::reflect_handle_rust_type;

/// Return the leaf of a canonical nominal name. Dotted names remain accepted
/// only at source lookup boundaries for older Core spellings; foreign nominal
/// maps themselves contain `::` identities exclusively.
pub(crate) fn nominal_leaf(name: &str) -> &str {
    name.rsplit_once("::")
        .or_else(|| name.rsplit_once('.'))
        .map_or(name, |(_, leaf)| leaf)
}

impl Cx {

    pub(crate) fn persistent_local(&self, name: &str) -> Option<crate::Codegen::TIR::TLocal> {
        self.persist_types.get(name).map(|_| {
            crate::Codegen::TIR::TLocal::persistent(
                name,
                if self.module_alias.is_empty() {
                    "main"
                } else {
                    self.module_alias.as_str()
                },
                self.persist_types
                    .get(name)
                    .cloned()
                    .expect("persist type and slot must agree"),
            )
        })
    }


    pub(crate) fn foreign_type_identity(&self, alias: &str, leaf: &str) -> Option<String> {
        // A bare source name resolves to a local nominal before any imported
        // type with the same leaf. Qualified aliases still select their import.
        if alias.is_empty() && self.local_type_names.contains(leaf) {
            return None;
        }
        let rust_mod = if alias.is_empty() {
            None
        } else {
            Some(self.import_mods.get(alias)?)
        };
        let mut matches = self
            .foreign_types
            .iter()
            .filter(|(name, _)| nominal_leaf(name) == leaf)
            .filter(|(_, module)| rust_mod.is_none_or(|expected| *module == expected))
            .map(|(name, _)| name.clone());
        let identity = matches.next()?;
        matches.next().is_none().then_some(identity)
    }

    pub(crate) fn imported_type_metadata_name(&self, name: &str) -> Option<String> {
        if name.contains("::") {
            return Some(name.to_string());
        }
        if let Some((alias, leaf)) = name.split_once('.') {
            return self.foreign_type_identity(alias, leaf);
        }
        self.foreign_type_identity("", name)
    }

    /// Resolve a source-facing type spelling to the ordinary distinct-type
    /// entry. This is used only for backend representation facts; checked-text
    /// eligibility is owned by sema's ordinary trait registry.
    pub(crate) fn distinct_type_identity(&self, name: &str) -> Option<String> {
        if self.distinct_types.contains_key(name) {
            return Some(name.to_string());
        }
        let (alias, leaf) = name.split_once('.')?;
        self.foreign_type_identity(alias, leaf)
            .filter(|identity| self.distinct_types.contains_key(identity))
    }

    /// Read the ordinary lowered trait-method facts for a String-backed
    /// nominal. This keeps constructor lowering independent of any
    /// compiler-owned type whitelist.
    pub(crate) fn string_distinct_has_trait_method(&self, name: &str, method: &str) -> bool {
        let Some(identity) = self.distinct_type_identity(name) else {
            return false;
        };
        self.distinct_types
            .get(&identity)
            .is_some_and(|(base, _)| matches!(base, Type::String))
            && self.trait_methods.contains(&(identity, method.to_string()))
    }

    pub(crate) fn quantity_dimension(&self, ty: &Type) -> Option<crate::AST::Dimension> {
        ty.quantity_parts()
            .map(|(_, dimension)| dimension)
            .or_else(|| {
                let Type::Named(name) = ty else {
                    return None;
                };
                self.unit_facts
                    .get(name)
                    .or_else(|| {
                        self.imported_type_metadata_name(name)
                            .and_then(|canonical| self.unit_facts.get(&canonical))
                    })
                    .and_then(|fact| fact.dimension.clone())
            })
    }

    pub(crate) fn unit_label(&self, ty: &Type) -> Option<&UnitLabel> {
        let Type::Named(name) = ty else {
            return None;
        };
        self.unit_labels.get(name).or_else(|| {
            self.imported_type_metadata_name(name)
                .and_then(|canonical| self.unit_labels.get(&canonical))
        })
    }

    pub(crate) fn has_display_type(&self, name: &str) -> bool {
        self.display_types.contains(name)
            || self
                .imported_type_metadata_name(name)
                .is_some_and(|canonical| self.display_types.contains(&canonical))
    }



    pub(crate) fn is_distinct_type_name(&self, name: &str) -> bool {
        self.distinct_types.contains_key(name)
            || self
                .imported_type_metadata_name(name)
                .is_some_and(|canonical| self.distinct_types.contains_key(&canonical))
    }

    pub(crate) fn quantity_unit_label(
        &self,
        dimension: crate::AST::Dimension,
        style: crate::AST::UnitFormat,
    ) -> String {
        let mut numerator = Vec::new();
        let mut denominator = Vec::new();
        for (axis, exponent) in dimension.axes() {
            let family = axis.rsplit("::").next().unwrap_or(axis);
            if exponent == 0 {
                continue;
            }
            let label = self
                .unit_labels
                .values()
                .filter(|label| label.family == family && label.is_base)
                .min_by(|left, right| {
                    left.symbol
                        .cmp(&right.symbol)
                        .then_with(|| left.name.cmp(&right.name))
                })
                .map(|label| match style {
                    crate::AST::UnitFormat::Name => label.name.clone(),
                    crate::AST::UnitFormat::Symbol | crate::AST::UnitFormat::Bare => {
                        label.symbol.clone()
                    }
                })
                .or_else(|| {
                    (family == "Time").then(|| match style {
                        crate::AST::UnitFormat::Name => "Nanosecond".to_string(),
                        crate::AST::UnitFormat::Symbol | crate::AST::UnitFormat::Bare => {
                            "ns".to_string()
                        }
                    })
                })
                .unwrap_or_else(|| family.to_ascii_lowercase());
            let part = if exponent.abs() == 1 {
                label
            } else {
                format!("{label}^{}", exponent.abs())
            };
            if exponent > 0 {
                numerator.push(part);
            } else {
                denominator.push(part);
            }
        }
        let numerator = if numerator.is_empty() {
            "1".to_string()
        } else {
            numerator.join("*")
        };
        if denominator.is_empty() {
            numerator
        } else {
            format!("{numerator}/{}", denominator.join("*"))
        }
    }

    /// Resolve a Core alias in the exact function scope selected by
    /// D-NAME-WALK1=A. Inline bindings overlay the enclosing file bindings.
    pub(crate) fn core_import_module_for_function(
        &self,
        fn_name: &str,
        alias: &str,
    ) -> Option<&str> {
        self.inline_core_imports
            .get(fn_name)
            .and_then(|scope| scope.get(alias))
            .or_else(|| self.core_imports.get(alias))
            .map(String::as_str)
    }

    pub(crate) fn import_module_for_function(&self, fn_name: &str, alias: &str) -> Option<&str> {
        self.inline_foreign_imports
            .get(fn_name)
            .and_then(|scope| scope.get(alias))
            .or_else(|| self.import_mods.get(alias))
            .map(String::as_str)
    }

    pub(crate) fn import_signature_for_function(
        &self,
        fn_name: &str,
        alias: &str,
        method: &str,
    ) -> Option<Vec<(AccessConvention, Type)>> {
        self.inline_foreign_sigs
            .get(fn_name)
            .and_then(|scope| scope.get(&(alias.to_string(), method.to_string())))
            .cloned()
            .or_else(|| {
                self.import_sigs
                    .get(&(alias.to_string(), method.to_string()))
                    .cloned()
            })
    }

    pub(crate) fn import_return_for_function(
        &self,
        fn_name: &str,
        alias: &str,
        method: &str,
    ) -> Option<Option<Type>> {
        self.inline_foreign_rets
            .get(fn_name)
            .and_then(|scope| scope.get(&(alias.to_string(), method.to_string())))
            .cloned()
            .or_else(|| {
                self.import_rets
                    .get(&(alias.to_string(), method.to_string()))
                    .cloned()
            })
    }

    /// Resolve a Core alias without a function-specific scope. TIR coverage
    /// predicates have only structural AST facts, so they use the enclosing
    /// map first and then any inline body map as a conservative reachability
    /// check. Lowering always uses the exact helper above.
    pub(crate) fn any_core_import_module(&self, alias: &str) -> Option<&str> {
        self.core_imports
            .get(alias)
            .or_else(|| {
                self.inline_core_imports
                    .iter()
                    .filter_map(|(scope, imports)| imports.get(alias).map(|module| (scope, module)))
                    .min_by(|(left_scope, left_module), (right_scope, right_module)| {
                        left_scope
                            .cmp(right_scope)
                            .then_with(|| left_module.cmp(right_module))
                    })
                    .map(|(_, module)| module)
            })
            .map(String::as_str)
    }

    pub(crate) fn any_foreign_import_module(&self, alias: &str) -> Option<&str> {
        self.import_mods
            .get(alias)
            .or_else(|| {
                self.inline_foreign_imports
                    .iter()
                    .filter_map(|(scope, imports)| imports.get(alias).map(|module| (scope, module)))
                    .min_by(|(left_scope, left_module), (right_scope, right_module)| {
                        left_scope
                            .cmp(right_scope)
                            .then_with(|| left_module.cmp(right_module))
                    })
                    .map(|(_, module)| module)
            })
            .map(String::as_str)
    }

    pub(crate) fn core_qualified_rust_type_name(&self, name: &str) -> Option<&'static str> {
        let (alias, leaf) = name.split_once('.')?;
        if matches!(
            self.any_core_import_module(alias),
            Some("core.data") | Some("core.data.plot")
        ) {
            return match leaf {
                "JetDataPlotMark" => Some("JetDataPlotMark"),
                "DataLoaderKind" => Some("DataLoaderKind"),
                "DataFreshness" => Some("DataFreshness"),
                "DataInvalidationCause" => Some("DataInvalidationCause"),
                "DataAuthority" => Some("DataAuthority"),
                "DataSourceIdentity" => Some("DataSourceIdentity"),
                "DataProvenance" => Some("DataProvenance"),
                "DataSnapshotIdentity" => Some("DataSnapshotIdentity"),
                "DataLoaderStatus" => Some("DataLoaderStatus"),
                "DataLoader" => Some("DataLoader"),
                "DataSnapshot" => Some("DataSnapshot"),
                "JetDataPlotAggregate" => Some("JetDataPlotAggregate"),
                "JetDataPlotFilterOp" => Some("JetDataPlotFilterOp"),
                "JetDataPlotValue" => Some("JetDataPlotValue"),
                "JetDataPlotScaleKind" => Some("JetDataPlotScaleKind"),
                "JetDataPlotDomain" => Some("JetDataPlotDomain"),
                "JetDataPlotLegendPosition" => Some("JetDataPlotLegendPosition"),
                "JetDataPlotFacetKind" => Some("JetDataPlotFacetKind"),
                "JetDataPlotInteraction" => Some("JetDataPlotInteraction"),
                "JetDataPlotBackend" => Some("JetDataPlotBackend"),
                "JetDataPlotSupport" => Some("JetDataPlotSupport"),
                "JetDataPlotErrorKind" => Some("JetDataPlotErrorKind"),
                "JetDataPlotRenderFormat" => Some("JetDataPlotRenderFormat"),
                "JetDataPlotField" => Some("JetDataPlotField"),
                "JetDataPlotSchema" => Some("JetDataPlotSchema"),
                "JetDataPlotSourceFacts" => Some("JetDataPlotSourceFacts"),
                "JetDataPlotEncoding" => Some("JetDataPlotEncoding"),
                "JetDataPlotScale" => Some("JetDataPlotScale"),
                "JetDataPlotAxis" => Some("JetDataPlotAxis"),
                "JetDataPlotLegend" => Some("JetDataPlotLegend"),
                "JetDataPlotFacet" => Some("JetDataPlotFacet"),
                "JetDataPlotLayer" => Some("JetDataPlotLayer"),
                "JetDataPlotAccessibility" => Some("JetDataPlotAccessibility"),
                "JetDataPlotLayout" => Some("JetDataPlotLayout"),
                "JetDataPlotCapability" => Some("JetDataPlotCapability"),
                "JetDataPlotError" => Some("JetDataPlotError"),
                "JetDataPlotPlan" => Some("JetDataPlotPlan"),
                "JetDataPlotSelectedRow" => Some("JetDataPlotSelectedRow"),
                "JetDataPlotInspection" => Some("JetDataPlotInspection"),
                "JetDataPlotRender" => Some("JetDataPlotRender"),
                "JetDataPlotProjection" => Some("JetDataPlotProjection"),
                "JetDataPlotColumn" => Some("JetDataPlotColumn"),
                "JetDataPlot" => Some("JetDataPlot"),
                _ => None,
            };
        }
        match (self.any_core_import_module(alias), leaf) {
            (Some("core.crypto"), leaf) => core_crypto_type_name(leaf),
            (Some("core.auth"), "Claims") => Some("Claims"),
            (Some("core.auth"), "AuthError") => Some("AuthError"),
            (Some("core.auth"), "Session") => Some("Session"),
            (Some("core.auth"), "Auth") => Some("Auth"),
            (Some("core.sync"), "SyncText") => Some("SyncText"),
            (Some("core.sync"), "SyncCounter") => Some("SyncCounter"),
            (Some("core.sync"), "SyncMap") => Some("SyncMap"),
            (Some("core.sync"), "SyncList") => Some("SyncList"),
            (Some("core.sync"), "RowPolicy") => Some("RowPolicy"),
            (Some("core.http.client"), "Proxy") => Some("HTTPProxy"),
            (Some("core.http.client"), "RedirectPolicy") => Some("HTTPRedirectPolicy"),
            (Some("core.http.client"), "RetryPolicy") => Some("HTTPRetryPolicy"),
            (Some("core.http.client"), "CookieJar") => Some("HTTPCookieJar"),
            (Some("core.net.tls"), "TLSVersion") => Some("TLSVersion"),
            (Some("core.net.tls"), "RootCertificates") => Some("TLSRootCertificates"),
            (Some("core.net.tls"), "ClientIdentity") => Some("TLSClientIdentity"),
            (Some("core.sys"), "EnvError") => Some("EnvError"),
            (Some("core.mem"), "AllocError") => Some("AllocError"),
            (Some("core.encoding"), "DataEvent") => Some("DataEvent"),
            (Some("core.encoding"), "DataTree") => Some("DataTree"),
            (Some("core.encoding"), "EncodingLimits") => Some("EncodingLimits"),
            (Some("core.encoding"), "EncodingError") => Some("EncodingError"),
            (Some("core.encoding"), "EncodingCause") => Some("EncodingCause"),
            (Some("core.encoding"), "EncodingFormat") => Some("EncodingFormat"),
            (Some("core.encoding"), "EncodingErrorKind") => Some("EncodingErrorKind"),
            (Some("core.email"), "Address") => Some("Address"),
            (Some("core.email"), "Message") => Some("Message"),
            (Some("core.email"), "Attachment") => Some("Attachment"),
            (Some("core.email"), "Envelope") => Some("Envelope"),
            (Some("core.email"), "SMTPSecurity") => Some("SMTPSecurity"),
            (Some("core.email"), "Limits") => Some("Limits"),
            (Some("core.email"), "RecipientPolicy") => Some("RecipientPolicy"),
            (Some("core.email"), "RecipientReport") => Some("RecipientReport"),
            (Some("core.email"), "SendReport") => Some("SendReport"),
            (Some("core.email"), "EmailError") => Some("EmailError"),
            (Some("core.email"), "SMTPAuth") => Some("SMTPAuth"),
            (Some("core.email"), "TLSTrust") => Some("TLSTrust"),
            (Some("core.email"), "SMTPConfig") => Some("SMTPConfig"),
            (Some("core.email"), "DkimConfig") => Some("DkimConfig"),
            (Some("core.email"), "Mailer") => Some("Mailer"),
            (Some("core.encoding.json"), "JSONReader") => Some("JSONReader"),
            (Some("core.encoding.json"), "JSONWriter") => Some("JSONWriter"),
            (Some("core.encoding.jsonl"), "JSONLReader") => Some("JSONLReader"),
            (Some("core.encoding.jsonl"), "JSONLWriter") => Some("JSONLWriter"),
            (Some("core.encoding.csv"), "CSVReader") => Some("CSVReader"),
            (Some("core.encoding.csv"), "CSVWriter") => Some("CSVWriter"),
            (Some("core.encoding.csv"), "CSVRow") => Some("CSVRow"),
            (Some("core.encoding.xml"), "XMLReader") => Some("XMLReader"),
            (Some("core.encoding.xml"), "XMLWriter") => Some("XMLWriter"),
            (Some("core.encoding.xml"), "XMLLimits") => Some("XMLLimits"),
            (Some("core.encoding.xml"), "XMLParseOptions") => Some("XMLParseOptions"),
            (Some("core.encoding.xml"), "XMLRenderOptions") => Some("XMLRenderOptions"),
            (Some("core.encoding.xml"), "XMLEncoding") => Some("XMLEncoding"),
            (Some("core.encoding.xml"), "XMLLexicalPolicy") => Some("XMLLexicalPolicy"),
            (Some("core.encoding.xml"), "XMLCanonical") => Some("XMLCanonical"),
            (Some("core.encoding.xml"), "XMLCanonicalMode") => Some("XMLCanonicalMode"),
            (Some("core.encoding.xml"), "XMLError") => Some("XMLError"),
            (Some("core.encoding.xml"), "XMLReason") => Some("XMLReason"),
            (Some("core.encoding.xml"), "XMLEntityPolicy") => Some("XMLEntityPolicy"),
            (Some("core.encoding.cbor"), "CBORReader") => Some("CBORReader"),
            (Some("core.encoding.cbor"), "CBOROptions") => Some("CBOROptions"),
            (Some("core.encoding.cbor"), "CBORError") => Some("CBORError"),
            (Some("core.encoding.cbor"), "CBORErrorKind") => Some("CBORErrorKind"),
            (Some("core.encoding.cbor"), "CBORWriter") => Some("CBORWriter"),
            (Some("core.jobs"), "JobQueue") => Some("JobQueue"),
            (Some("core.jobs"), "JobPayload") => Some("JobPayload"),
            (Some("core.jobs"), "JobResult") => Some("JobResult"),
            (Some("core.jobs"), "JobError") => Some("JobError"),
            (Some("core.jobs"), "JobQueueReceipt") => Some("JobQueueReceipt"),
            (Some("core.jobs"), "JobQueueClaim") => Some("JobQueueClaim"),
            (Some("core.jobs"), "JobQueueEvent") => Some("JobQueueEvent"),
            (Some("core.jobs"), "JobQueueRecord") => Some("JobQueueRecord"),
            (Some("core.jobs"), "JobQueueStatus") => Some("JobQueueStatus"),
            (Some("core.jobs"), "JobQueueState") => Some("JobQueueState"),
            (Some("core.jobs"), "JobQueueDeliveryPolicy") => Some("JobQueueDeliveryPolicy"),
            _ => None,
        }
    }
    pub(crate) fn type_contains_view(&self, ty: &Type) -> bool {
        self.type_contains_view_matching(ty, false)
    }

    /// A comptime value containing one of these edges cannot use the generic
    /// `CtValue::serialize` path: serialization has no type context in which
    /// to insert the `Box::new` required by Rust's recursive layout.
    pub(crate) fn type_contains_boxed_edge(&self, ty: &Type) -> bool {
        fn contains(cx: &Cx, ty: &Type, seen: &mut HashSet<String>) -> bool {
            match ty {
                Type::Named(name) => {
                    if cx.boxed_edges.iter().any(|(owner, _)| owner == name) {
                        return true;
                    }
                    if !seen.insert(name.clone()) {
                        return false;
                    }
                    let found = cx.struct_fields.get(name).is_some_and(|fields| {
                        fields
                            .iter()
                            .any(|(_, field_ty)| contains(cx, field_ty, seen))
                    });
                    seen.remove(name);
                    found
                }
                Type::Apply { name, args } => {
                    args.iter().any(|arg| contains(cx, arg, seen))
                        || contains(cx, &Type::Named(name.clone()), seen)
                }
                Type::List(inner)
                | Type::Shared(inner)
                | Type::Option(inner)
                | Type::Tagged { inner, .. }
                | Type::InlineRange { base: inner, .. } => contains(cx, inner, seen),
                Type::Map { key, value, .. }
                | Type::Result {
                    ok: key,
                    err: value,
                } => contains(cx, key, seen) || contains(cx, value, seen),
                Type::Tuple(fields) => fields
                    .iter()
                    .any(|(_, field_ty)| contains(cx, field_ty, seen)),
                Type::FixedList { elem, .. } => contains(cx, elem, seen),
                Type::Fn { params, ret, .. } => {
                    params.iter().any(|param| contains(cx, param, seen))
                        || ret.as_deref().is_some_and(|ret| contains(cx, ret, seen))
                }
                Type::Union(members) => members.iter().any(|member| contains(cx, member, seen)),
                Type::Quantity { base, .. } => contains(cx, base, seen),
                _ => false,
            }
        }

        contains(self, ty, &mut HashSet::new())
    }

    /// `CtValue::serialize` has no expected-type context: it emits every list
    /// as `Vec` and every integer as `i64`. Keep a folded value on the typed
    /// source-lowering path when a struct or enum payload contains a shape
    /// that serialization cannot preserve.
    pub(crate) fn type_contains_typed_literal_edge(&self, ty: &Type) -> bool {
        fn payload_contains(cx: &Cx, payload: &VariantPayload, seen: &mut HashSet<String>) -> bool {
            match payload {
                VariantPayload::Unit => false,
                VariantPayload::Single(ty, _) => contains(cx, ty, seen),
                VariantPayload::Named(fields) => {
                    fields.iter().any(|field| contains(cx, &field.ty, seen))
                }
            }
        }

        fn contains(cx: &Cx, ty: &Type, seen: &mut HashSet<String>) -> bool {
            match ty {
                Type::IntN { .. } | Type::InlineRange { .. } | Type::FixedList { .. } => true,
                Type::Named(name) => {
                    if !seen.insert(name.clone()) {
                        return false;
                    }
                    let found = cx.struct_fields.get(name).is_some_and(|fields| {
                        fields
                            .iter()
                            .any(|(_, field_ty)| contains(cx, field_ty, seen))
                    }) || cx.enum_variants.get(name).is_some_and(|variants| {
                        variants
                            .iter()
                            .any(|(_, payload)| payload_contains(cx, payload, seen))
                    });
                    seen.remove(name);
                    found
                }
                Type::Apply { name, args } => {
                    args.iter().any(|arg| contains(cx, arg, seen))
                        || contains(cx, &Type::Named(name.clone()), seen)
                }
                Type::List(inner)
                | Type::Shared(inner)
                | Type::Option(inner)
                | Type::Tagged { inner, .. } => contains(cx, inner, seen),
                Type::Map { key, value, .. }
                | Type::Result {
                    ok: key,
                    err: value,
                } => contains(cx, key, seen) || contains(cx, value, seen),
                Type::Tuple(fields) => fields
                    .iter()
                    .any(|(_, field_ty)| contains(cx, field_ty, seen)),
                Type::Fn { params, ret, .. } => {
                    params.iter().any(|param| contains(cx, param, seen))
                        || ret.as_deref().is_some_and(|ret| contains(cx, ret, seen))
                }
                Type::Union(members) => members.iter().any(|member| contains(cx, member, seen)),
                Type::Quantity { base, .. } => contains(cx, base, seen),
                _ => false,
            }
        }

        contains(self, ty, &mut HashSet::new())
    }

    pub(crate) fn type_contains_mutable_view(&self, ty: &Type) -> bool {
        self.type_contains_view_matching(ty, true)
    }

    fn type_contains_view_matching(&self, ty: &Type, mutable_only: bool) -> bool {
        fn payload_contains(
            cx: &Cx,
            payload: &VariantPayload,
            seen: &mut HashSet<String>,
            mutable_only: bool,
        ) -> bool {
            match payload {
                VariantPayload::Unit => false,
                VariantPayload::Single(ty, _) => contains(cx, ty, seen, mutable_only),
                VariantPayload::Named(fields) => fields
                    .iter()
                    .any(|field| contains(cx, &field.ty, seen, mutable_only)),
            }
        }

        fn named_contains(
            cx: &Cx,
            name: &str,
            seen: &mut HashSet<String>,
            mutable_only: bool,
        ) -> bool {
            if !seen.insert(name.to_string()) {
                return false;
            }
            let found = cx.struct_fields.get(name).is_some_and(|fields| {
                fields
                    .iter()
                    .any(|(_, ty)| contains(cx, ty, seen, mutable_only))
            }) || cx.enum_variants.get(name).is_some_and(|variants| {
                variants
                    .iter()
                    .any(|(_, payload)| payload_contains(cx, payload, seen, mutable_only))
            });
            seen.remove(name);
            found
        }

        fn contains(cx: &Cx, ty: &Type, seen: &mut HashSet<String>, mutable_only: bool) -> bool {
            match ty {
                // D-PIN1=A: `Pin<T>` is a borrowed window like `View`/`ViewMut`,
                // so it needs the same hidden Rust lifetime everywhere it is
                // stored or returned. It is always a write window.
                Type::Apply { name, args }
                    if matches!(
                        name.as_str(),
                        "View" | "ViewMut" | "ComputeViewMut" | Syntax::TYPE_PIN
                    ) && args.len() == 1 =>
                {
                    !mutable_only
                        || matches!(name.as_str(), "ViewMut" | "ComputeViewMut")
                        || name == Syntax::TYPE_PIN
                }
                Type::Named(name) => named_contains(cx, name, seen, mutable_only),
                Type::Apply { name, args } => {
                    args.iter().any(|arg| contains(cx, arg, seen, mutable_only))
                        || named_contains(cx, name, seen, mutable_only)
                }
                Type::Tagged { marker, .. }
                    if matches!(
                        marker,
                        crate::AST::TagMarker::Internal(crate::AST::InternalTag::AllocatorView)
                    ) =>
                {
                    true
                }
                Type::List(inner)
                | Type::Shared(inner)
                | Type::Option(inner)
                | Type::Tagged { inner, .. } => contains(cx, inner, seen, mutable_only),
                Type::Map { key, value, .. }
                | Type::Result {
                    ok: key,
                    err: value,
                } => {
                    contains(cx, key, seen, mutable_only) || contains(cx, value, seen, mutable_only)
                }
                Type::Tuple(fields) => fields
                    .iter()
                    .any(|(_, ty)| contains(cx, ty, seen, mutable_only)),
                Type::FixedList { elem, .. } => contains(cx, elem, seen, mutable_only),
                Type::Fn { params, ret, .. } => {
                    params.iter().any(|ty| contains(cx, ty, seen, mutable_only))
                        || ret
                            .as_deref()
                            .is_some_and(|ty| contains(cx, ty, seen, mutable_only))
                }
                _ => false,
            }
        }

        contains(
            self,
            &self.expand_type_aliases(ty),
            &mut HashSet::new(),
            mutable_only,
        )
    }

    pub(crate) fn type_contains_shared_guard(&self, ty: &Type) -> bool {
        fn payload_contains(cx: &Cx, payload: &VariantPayload, seen: &mut HashSet<String>) -> bool {
            match payload {
                VariantPayload::Unit => false,
                VariantPayload::Single(ty, _) => contains(cx, ty, seen),
                VariantPayload::Named(fields) => {
                    fields.iter().any(|field| contains(cx, &field.ty, seen))
                }
            }
        }

        fn named_contains(cx: &Cx, name: &str, seen: &mut HashSet<String>) -> bool {
            if !seen.insert(name.to_string()) {
                return false;
            }
            cx.struct_fields
                .get(name)
                .is_some_and(|fields| fields.iter().any(|(_, ty)| contains(cx, ty, seen)))
                || cx.enum_variants.get(name).is_some_and(|variants| {
                    variants
                        .iter()
                        .any(|(_, payload)| payload_contains(cx, payload, seen))
                })
        }

        fn contains(cx: &Cx, ty: &Type, seen: &mut HashSet<String>) -> bool {
            match ty {
                Type::Apply { name, .. } if name == Syntax::TYPE_SHARED_GUARD => true,
                Type::Named(name) => named_contains(cx, name, seen),
                Type::Apply { name, args } => {
                    args.iter().any(|arg| contains(cx, arg, seen)) || named_contains(cx, name, seen)
                }
                Type::List(inner)
                | Type::Shared(inner)
                | Type::Option(inner)
                | Type::Tagged { inner, .. } => contains(cx, inner, seen),
                Type::Map { key, value, .. }
                | Type::Result {
                    ok: key,
                    err: value,
                } => contains(cx, key, seen) || contains(cx, value, seen),
                Type::Tuple(fields) => fields.iter().any(|(_, ty)| contains(cx, ty, seen)),
                Type::Union(members) => members.iter().any(|ty| contains(cx, ty, seen)),
                Type::FixedList { elem, .. } => contains(cx, elem, seen),
                Type::Fn { params, ret, .. } => {
                    params.iter().any(|ty| contains(cx, ty, seen))
                        || ret.as_deref().is_some_and(|ty| contains(cx, ty, seen))
                }
                _ => false,
            }
        }

        contains(self, &self.expand_type_aliases(ty), &mut HashSet::new())
    }

    /// Core crypto values are move-only and intentionally do not implement
    /// Rust `Debug` or `Clone`. Do not let a containing user record derive
    /// either backend trait; its Jet debug path already honors `#Redact`.
    pub(crate) fn type_contains_secret(&self, ty: &Type) -> bool {
        fn payload_contains(cx: &Cx, payload: &VariantPayload, seen: &mut HashSet<String>) -> bool {
            match payload {
                VariantPayload::Unit => false,
                VariantPayload::Single(ty, _) => contains(cx, ty, seen),
                VariantPayload::Named(fields) => {
                    fields.iter().any(|field| contains(cx, &field.ty, seen))
                }
            }
        }

        fn named_contains(cx: &Cx, name: &str, seen: &mut HashSet<String>) -> bool {
            if !seen.insert(name.to_string()) {
                return false;
            }
            let found = cx
                .struct_fields
                .get(name)
                .is_some_and(|fields| fields.iter().any(|(_, ty)| contains(cx, ty, seen)))
                || cx.enum_variants.get(name).is_some_and(|variants| {
                    variants
                        .iter()
                        .any(|(_, payload)| payload_contains(cx, payload, seen))
                });
            seen.remove(name);
            found
        }

        fn contains(cx: &Cx, ty: &Type, seen: &mut HashSet<String>) -> bool {
            match ty {
                Type::Tagged {
                    marker:
                        crate::AST::TagMarker::Internal(crate::AST::InternalTag::CoreCryptoNominal),
                    ..
                } => true,
                Type::Named(name) => named_contains(cx, name, seen),
                Type::Apply { name, args } => {
                    args.iter().any(|arg| contains(cx, arg, seen)) || named_contains(cx, name, seen)
                }
                Type::List(inner)
                | Type::Shared(inner)
                | Type::Option(inner)
                | Type::Tagged { inner, .. } => contains(cx, inner, seen),
                Type::Map { key, value, .. }
                | Type::Result {
                    ok: key,
                    err: value,
                } => contains(cx, key, seen) || contains(cx, value, seen),
                Type::Tuple(fields) => fields.iter().any(|(_, ty)| contains(cx, ty, seen)),
                Type::Union(members) => members.iter().any(|ty| contains(cx, ty, seen)),
                Type::FixedList { elem, .. }
                | Type::InlineRange { base: elem, .. }
                | Type::Quantity { base: elem, .. } => contains(cx, elem, seen),
                Type::Fn { params, ret, .. } => {
                    params.iter().any(|ty| contains(cx, ty, seen))
                        || ret.as_deref().is_some_and(|ty| contains(cx, ty, seen))
                }
                _ => false,
            }
        }

        contains(self, &self.expand_type_aliases(ty), &mut HashSet::new())
    }

    /// Render a type whose view-bearing leaves borrow the function's hidden
    /// owner lifetime. Wrappers stay wrappers; only references and generated
    /// aggregate types receive the lifetime argument.
    pub(crate) fn rust_type_with_view_lifetime(&self, ty: &Type) -> String {
        self.rust_type_with_view_lifetime_using(ty, &|ty| self.rust_type(ty))
    }


    fn rust_type_with_view_lifetime_using(
        &self,
        ty: &Type,
        base: &impl Fn(&Type) -> String,
    ) -> String {
        fn add_reference_lifetime(rust: String) -> String {
            if let Some(rest) = rust.strip_prefix("&mut ") {
                jet_format!("&'{jet_prefix}view mut {rest}")
            } else if let Some(rest) = rust.strip_prefix('&') {
                jet_format!("&'{jet_prefix}view {rest}")
            } else if let Some(prefix) = rust.strip_suffix("JetComputeViewMut<'_>") {
                jet_format!("{prefix}JetComputeViewMut<'{jet_prefix}view>")
            } else {
                rust
            }
        }

        fn add_type_lifetime(rust: String) -> String {
            if let Some(open) = rust.find('<') {
                jet_format!("{}<'{jet_prefix}view, {}", &rust[..open], &rust[open + 1..])
            } else {
                jet_format!("{rust}<'{jet_prefix}view>")
            }
        }

        fn definition_contains_view(cx: &Cx, name: &str) -> bool {
            cx.struct_fields
                .get(name)
                .is_some_and(|fields| fields.iter().any(|(_, ty)| cx.type_contains_view(ty)))
                || cx.enum_variants.get(name).is_some_and(|variants| {
                    variants.iter().any(|(_, payload)| match payload {
                        VariantPayload::Unit => false,
                        VariantPayload::Single(ty, _) => cx.type_contains_view(ty),
                        VariantPayload::Named(fields) => {
                            fields.iter().any(|field| cx.type_contains_view(&field.ty))
                        }
                    })
                })
        }

        fn render(cx: &Cx, ty: &Type, base: &impl Fn(&Type) -> String) -> String {
            match ty {
                Type::Apply { name, args }
                    if matches!(
                        name.as_str(),
                        "View" | "ViewMut" | "ComputeViewMut" | Syntax::TYPE_PIN
                    ) && args.len() == 1 =>
                {
                    add_reference_lifetime(base(ty))
                }
                Type::Tagged { marker, .. }
                    if matches!(
                        marker,
                        crate::AST::TagMarker::Internal(crate::AST::InternalTag::AllocatorView)
                    ) =>
                {
                    add_reference_lifetime(base(ty))
                }
                Type::List(inner) if cx.type_contains_view(inner) => {
                    format!("Vec<{}>", render(cx, inner, base))
                }
                Type::Map { key, value, .. } if cx.type_contains_view(ty) => format!(
                    "{}JetMap<{}, {}>",
                    cx.root_prefix,
                    render(cx, key, base),
                    render(cx, value, base)
                ),
                Type::Shared(inner) if cx.type_contains_view(inner) => format!(
                    "{}jet_std::JetShared<{}>",
                    cx.root_prefix,
                    render(cx, inner, base)
                ),
                Type::Option(inner) if cx.type_contains_view(inner) => {
                    format!(
                        "{0}JetOutcome<{1}, {0}JetAbsent>",
                        cx.root_prefix,
                        render(cx, inner, base)
                    )
                }
                Type::Result { ok, err } if cx.type_contains_view(ty) => {
                    format!(
                        "{}JetOutcome<{}, {}>",
                        cx.root_prefix,
                        render(cx, ok, base),
                        render(cx, err, base)
                    )
                }
                Type::Tuple(fields) if cx.type_contains_view(ty) => {
                    add_type_lifetime(tuple_struct_name(&tuple_fields_plain(fields)))
                }
                Type::FixedList { elem, len, .. } if cx.type_contains_view(elem) => {
                    format!("[{}; {len}]", render(cx, elem, base))
                }
                Type::Tagged { inner, .. } if cx.type_contains_view(inner) => {
                    render(cx, inner, base)
                }
                Type::Named(name) if definition_contains_view(cx, name) => {
                    add_type_lifetime(base(ty))
                }
                Type::Apply { name, args }
                    if definition_contains_view(cx, name)
                        || args.iter().any(|arg| cx.type_contains_view(arg)) =>
                {
                    let head = base(&Type::Named(name.clone()));
                    let args = args
                        .iter()
                        .map(|arg| render(cx, arg, base))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let life = definition_contains_view(cx, name)
                        .then(|| jet_format!("'{jet_prefix}view"))
                        .into_iter()
                        .chain((!args.is_empty()).then(|| args.clone()))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{head}<{life}>")
                }
                _ => base(ty),
            }
        }

        render(self, &self.expand_type_aliases(ty), base)
    }



    /// D-SOA1: is `name` a `#layout(columnar)` struct (local or imported)? The
    /// columnar set only carries local structs; an imported columnar struct is
    /// not tracked, so `[ImportedColumnar]` still lowers AoS — acceptable for v1
    /// (columnar lists don't cross module boundaries in the shipped examples).
    pub(crate) fn is_columnar_struct(&self, name: &str) -> bool {
        self.columnar.contains(name)
    }

    /// D-SOA-TIER1=A: if `inner` is a `#layout(columnar)` struct type, the Rust
    /// type of a `[inner]` collection — the Prelude-owned `JetColumnList<S>`
    /// over THE shared column store. `None` for any non-columnar element.
    ///
    /// There is deliberately no per-struct storage type here any more: the
    /// layout, the row bookkeeping and the gather read are Prelude source, and
    /// codegen emits only the `JetRow` marshalling adapter for `S`.
    pub(crate) fn columnar_list_type(&self, inner: &Type) -> Option<String> {
        if let Type::Named(name) = inner {
            if self.is_columnar_struct(name) {
                return Some(format!("JetColumnList<{}>", self.rust_type(inner)));
            }
        }
        None
    }

    /// D-SOA-TIER1=A: the column index of `field` on columnar struct `name` —
    /// its position in declaration order among STORED fields, which is exactly
    /// the column order the store was built with. A computed field is never a
    /// column (D-FIELDPOL1) and `struct_fields` already excludes it.
    pub(crate) fn columnar_column_index(&self, name: &str, field: &str) -> Option<usize> {
        self.struct_fields
            .get(name)?
            .iter()
            .position(|(candidate, _)| candidate == field)
    }
    /// D-DATA-PLOT1=A: derive the canonical identity shared by table schema
    /// columns and typed plot selectors through the Foundation identity kernel.
    pub(crate) fn data_plot_field_id(&self, name: &str, ty: &Type) -> String {
        let type_name = ty.name();
        jet_foundation::PreludeDataFlow::column_identity(name, &type_name)
    }

    /// D-TYPEALIAS1 / D-ALIAS-OP1=B: expand `alias Name<T> :: …` applications to their target type.
    pub(crate) fn expand_type_aliases(&self, ty: &Type) -> Type {
        match ty {
            Type::Apply { name, args } if self.type_aliases.contains_key(name) => {
                let (params, target) = self.type_aliases.get(name).unwrap();
                let subst: HashMap<String, Type> = params
                    .iter()
                    .zip(args.iter())
                    .map(|(p, a)| (p.name.clone(), a.clone()))
                    .collect();
                self.expand_type_aliases(&Generics::substitute_type(target, &subst))
            }
            Type::Apply { name, args } => Type::Apply {
                name: name.clone(),
                args: args.iter().map(|a| self.expand_type_aliases(a)).collect(),
            },
            Type::List(inner) => Type::List(Box::new(self.expand_type_aliases(inner))),
            Type::Map {
                key,
                key_span,
                value,
            } => Type::Map {
                key: Box::new(self.expand_type_aliases(key)),
                key_span: *key_span,
                value: Box::new(self.expand_type_aliases(value)),
            },
            Type::Shared(inner) => Type::Shared(Box::new(self.expand_type_aliases(inner))),
            Type::Option(inner) => Type::Option(Box::new(self.expand_type_aliases(inner))),
            Type::Result { ok, err } => Type::Result {
                ok: Box::new(self.expand_type_aliases(ok)),
                err: Box::new(self.expand_type_aliases(err)),
            },
            Type::Fn {
                params,
                ret,
                param_contract,
                effect_bound,
                call_metadata,
                return_view_provenance,
            } => Type::Fn {
                params: params.iter().map(|p| self.expand_type_aliases(p)).collect(),
                ret: ret.as_ref().map(|r| Box::new(self.expand_type_aliases(r))),
                effect_bound: effect_bound.clone(),
                param_contract: param_contract.clone(),
                call_metadata: call_metadata.clone(),
                return_view_provenance: return_view_provenance.clone(),
            },
            Type::Tuple(fields) => Type::Tuple(
                fields
                    .iter()
                    .map(|(n, t)| (n.clone(), Box::new(self.expand_type_aliases(t))))
                    .collect(),
            ),
            Type::Tagged { marker, inner } => Type::Tagged {
                marker: marker.clone(),
                inner: Box::new(self.expand_type_aliases(inner)),
            },
            Type::FixedList { elem, len } => Type::FixedList {
                elem: Box::new(self.expand_type_aliases(elem)),
                len: len.clone(),
            },
            Type::InlineRange { base, lo, hi } => Type::InlineRange {
                base: Box::new(self.expand_type_aliases(base)),
                lo: *lo,
                hi: *hi,
            },
            Type::Union(members) => crate::AST::canonicalize_union(
                members
                    .iter()
                    .map(|m| self.expand_type_aliases(m))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    /// Project source types into the checked trait-object form consumed by
    /// TIR/MIR. Explicit binders supplement the method-level scope already
    /// held by this context, so a generic name can shadow a trait name.
    pub(crate) fn canonicalize_checked_type<'a>(
        &self,
        ty: &Type,
        binders: impl IntoIterator<Item = &'a String>,
    ) -> Type {
        let mut binder_names = self.current_type_params.borrow().clone();
        for name in binders {
            if !binder_names.contains(name) {
                binder_names.insert(name.clone());
            }
        }
        let expanded = self.expand_type_aliases(ty);
        crate::Codegen::TIR::tir_to_mir_types::canonicalize_checked_trait_types(
            &expanded,
            &self.trait_names,
            &self.type_names,
            &binder_names,
        )
    }

    pub(crate) fn rust_type(&self, ty: &Type) -> String {
        let ty = self.expand_type_aliases(ty);
        if let Some((base, _)) = ty.quantity_parts() {
            return self.rust_type(base);
        }
        match &ty {
            Type::Int => "i64".to_string(),
            Type::Float => "f64".to_string(),
            Type::IntN { signed, bits } => {
                format!("{}{}", if *signed { 'i' } else { 'u' }, bits)
            }
            Type::InlineRange { base, .. } => self.rust_type(base),
            Type::Float32 => "f32".to_string(),
            Type::Bool => "bool".to_string(),
            Type::String => "String".to_string(),
            Type::Char => "char".to_string(),
            // D-SOA1 / D-SOA-TIER1=A: a `[S]` of a `#layout(columnar)` struct
            // lowers to the Prelude `JetColumnList<S>`, not `Vec<S>`.
            Type::List(inner) if self.columnar_list_type(inner).is_some() => {
                self.columnar_list_type(inner).unwrap()
            }
            Type::List(inner) => format!("Vec<{}>", self.rust_type(inner)),
            Type::Map { key, value, .. } => format!(
                "{}JetMap<{}, {}>",
                self.root_prefix,
                self.rust_type(key),
                self.rust_type(value)
            ),
            // D-MEM1 S6 (D-SHARED-API1=A): `Shared<T>` — a lock-guarded shared
            // handle (`.read(f)`/`.edit(f)` closure API). Was a plain read-only
            // `Arc<T>` before this stage (never actually constructible — no
            // `Shared.new` existed — so there is no live behavior to preserve);
            // now `Arc<RwLock<T>>`, wrapped so `.clone()` stays a cheap Arc clone.
            Type::Shared(inner) => format!(
                "{}jet_std::JetShared<{}>",
                self.root_prefix,
                self.rust_type(inner)
            ),
            // D-FAIL-CARRIER1=A: `?T` and `T !E` are two views of one carrier.
            // The optional view's report is `JetAbsent` — absence is clean.
            Type::Option(inner) => format!(
                "{0}JetOutcome<{1}, {0}JetAbsent>",
                self.root_prefix,
                self.rust_type(inner)
            ),
            Type::Result { ok, err } => {
                format!(
                    "{}JetOutcome<{}, {}>",
                    self.root_prefix,
                    self.rust_type(ok),
                    self.rust_type(err)
                )
            }
            // Items inside an imported file live in `mod __jet_<alias>`; the
            // module provides the namespace, so item names stay plain.
            // c148: also recognize multi-char type params from `current_type_params`.
            Type::Named(name)
                if (Generics::is_type_var_name(name)
                    || self.current_type_params.borrow().contains(name.as_str()))
                    && !self.type_names.contains(name) =>
            {
                name.clone()
            }
            // Sema keeps visible imported leaves in user signatures (for example
            // `Note`), while the codegen registry stores the canonical nominal
            // identity. Resolve that projection before any prelude-name fallback
            // so imported types cannot be emitted as local `__jet_Note` names.
            Type::Named(name) if self.foreign_type_identity("", name).is_some() => {
                let identity = self
                    .foreign_type_identity("", name)
                    .expect("foreign identity was checked above");
                let rust_mod = self
                    .foreign_types
                    .get(&identity)
                    .expect("foreign identity must have a Rust module");
                let leaf = nominal_leaf(&identity);
                format!("{}{}::{}", self.root_prefix, rust_mod, mangle_path(leaf))
            }
            Type::Named(name) if (name == "Unit") && !self.type_names.contains(name) => {
                "()".to_string()
            }
            Type::Named(name)
                if name == Syntax::TYPE_REMOVE_BY && !self.type_names.contains(name) =>
            {
                format!("{}JetRemoveBy", self.root_prefix)
            }
            // D-CONC-FAIL1=A: task joins carry the shared Prelude failure
            // value. Keep this builtin mapping ahead of user named types.
            Type::Named(name)
                if name == Syntax::TYPE_TASK_FAILURE && !self.type_names.contains(name) =>
            {
                format!("{}JetTaskFailure", self.root_prefix)
            }
            Type::Named(name)
                if name == Syntax::TYPE_TASKGROUP && !self.type_names.contains(name) =>
            {
                format!("{}jet_std::JetTaskGroup", self.root_prefix)
            }
            Type::Named(name)
                if name == Syntax::TYPE_CONDITION && !self.type_names.contains(name) =>
            {
                format!("{}jet_std::JetCondition", self.root_prefix)
            }
            Type::Named(name)
                if name == Syntax::TYPE_SHARED_REVISION_ERROR
                    && !self.type_names.contains(name) =>
            {
                format!("{}jet_std::JetSharedRevisionError", self.root_prefix)
            }
            Type::Named(name) if name == Syntax::TYPE_NEVER => {
                "std::convert::Infallible".to_string()
            }
            Type::Named(name) if name == Syntax::TYPE_ALLOC_ERROR => {
                format!("{}AllocError", self.root_prefix)
            }
            Type::Named(name) if name == "Claims" && !self.type_names.contains(name) => {
                format!("{}JetAuthClaims", self.root_prefix)
            }
            Type::Named(name) if name == "AuthError" && !self.type_names.contains(name) => {
                format!("{}JetAuthError", self.root_prefix)
            }
            Type::Named(name) if name == "Session" && !self.type_names.contains(name) => {
                format!("{}JetAuthSession", self.root_prefix)
            }
            Type::Named(name) if name == "Auth" && !self.type_names.contains(name) => {
                format!("{}JetAuthApp", self.root_prefix)
            }
            Type::Named(name) if name == "SyncText" && !self.type_names.contains(name) => {
                format!("{}JetSyncText", self.root_prefix)
            }
            Type::Named(name) if name == "SyncCounter" && !self.type_names.contains(name) => {
                format!("{}JetSyncCounter", self.root_prefix)
            }
            Type::Named(name) if name == "SyncMap" && !self.type_names.contains(name) => {
                format!("{}JetSyncMap", self.root_prefix)
            }
            Type::Named(name) if name == "SyncList" && !self.type_names.contains(name) => {
                format!("{}JetSyncList", self.root_prefix)
            }
            Type::Named(name) if name == "RowPolicy" && !self.type_names.contains(name) => {
                format!("{}JetRowPolicy", self.root_prefix)
            }
            Type::Named(name) if name == "Hasher" && !self.type_names.contains(name) => {
                format!("{}JetCryptoHasher", self.root_prefix)
            }
            Type::Named(name)
                if !self.type_names.contains(name)
                    && core_crypto_rust_type_name(name).is_some() =>
            {
                let ffi = self.ffi_crate.as_deref().unwrap_or("jet_ffi");
                let rust = core_crypto_rust_type_name(name).unwrap();
                format!("{ffi}::{rust}")
            }
            Type::Named(name) if matches!(name.as_str(), "KeyStatus" | "VaultError") => {
                let ffi = self.ffi_crate.as_deref().unwrap_or("jet_ffi");
                let rust = if name == "KeyStatus" {
                    "JetVaultKeyStatus"
                } else {
                    "JetVaultError"
                };
                format!("{ffi}::{rust}")
            }
            // D-PENDING1=B: Loadable<Unknown, Unknown> placeholders — Rust infers the type.
            Type::Named(name) if name == "Unknown" => "_".to_string(),
            // D-APPROX1=A: sketch types → opaque Rust structs.
            Type::Named(name) if name == "HyperLogLog" => "JetHyperLogLog".to_string(),
            Type::Named(name) if name == "TDigest" => "JetTDigest".to_string(),
            Type::Named(name) if name == "CountMinSketch" => "JetCountMinSketch".to_string(),
            Type::Named(name) if name == "ReservoirSampler" => "JetReservoirSampler".to_string(),
            // D-TIMEDEPTH1=A: civil-time types → opaque Rust structs.
            Type::Named(name) if name == "Date" => "JetDate".to_string(),
            Type::Named(name) if name == "LocalDate" => "JetDate".to_string(),
            Type::Named(name) if name == "LocalTime" => "JetLocalTime".to_string(),
            Type::Named(name) if name == "DateTime" => "JetDateTime".to_string(),
            Type::Named(name) if name == "Instant" => "JetInstant".to_string(),
            Type::Named(name) if name == "Period" => "JetPeriod".to_string(),
            Type::Named(name) if name == "Zone" => "JetZone".to_string(),
            Type::Named(name) if name == "ZonedDateTime" => "JetZonedDateTime".to_string(),
            // D-URL1=A: URL/MIME values live in the corelib prelude module.
            Type::Named(name) if name == "Url" => {
                format!("{}jet_std::JetURL", self.root_prefix)
            }
            Type::Named(name) if name == "Mime" => {
                format!("{}jet_std::JetMIME", self.root_prefix)
            }
            Type::Named(name)
                if matches!(
                    name.as_str(),
                    "Address"
                        | "Message"
                        | "Attachment"
                        | "Envelope"
                        | "SMTPSecurity"
                        | "RecipientPolicy"
                        | "RecipientReport"
                        | "SendReport"
                        | "EmailError"
                        | "Limits"
                ) && !self.type_names.contains(name) =>
            {
                let rust = match name.as_str() {
                    "Address" => "Address",
                    "Message" => "Message",
                    "Attachment" => "Attachment",
                    "Envelope" => "Envelope",
                    "SMTPSecurity" => "SMTPSecurity",
                    "RecipientPolicy" => "RecipientPolicy",
                    "RecipientReport" => "RecipientReport",
                    "SendReport" => "SendReport",
                    "Limits" => "Limits",
                    _ => "Error",
                };
                format!("{}jet_email::{rust}", self.root_prefix)
            }
            Type::Named(name) if name == "SMTPAuth" && !self.type_names.contains(name) => {
                let ffi = self.ffi_crate.as_deref().unwrap_or("jet_ffi");
                format!("{}jet_email::SMTPAuth<{}::Secret>", self.root_prefix, ffi)
            }
            Type::Named(name)
                if matches!(name.as_str(), "DkimConfig" | "SMTPConfig")
                    && !self.type_names.contains(name) =>
            {
                let ffi = self.ffi_crate.as_deref().unwrap_or("jet_ffi");
                format!("{}jet_email::{}<{}::Secret>", self.root_prefix, name, ffi)
            }
            Type::Named(name)
                if matches!(name.as_str(), "TLSTrust" | "Mailer")
                    && !self.type_names.contains(name) =>
            {
                format!("{}jet_email::{}", self.root_prefix, name)
            }
            // D-WS1=B: WebSocket types.
            Type::Named(name) if name == "WsConn" => "JetWsConn".to_string(),
            Type::Named(name) if name == "WsError" => "JetWsError".to_string(),
            Type::Named(name) if name == "WsMessage" => "JetWsMessage".to_string(),
            // D-BROWSER-AUTO1=A: native BiDi opaque types.
            Type::Named(name) if name == "Browser" => "JetBrowser".to_string(),
            Type::Named(name) if name == "BrowserContext" => "JetBrowserContext".to_string(),
            Type::Named(name) if name == "BrowserPage" => "JetBrowserPage".to_string(),
            Type::Named(name) if name == "BrowserFrame" => "JetBrowserFrame".to_string(),
            Type::Named(name) if name == "BrowserLocator" => "JetBrowserLocator".to_string(),
            Type::Named(name) if name == "BrowserIntercept" => "JetBrowserIntercept".to_string(),
            Type::Named(name) if name == "BrowserEvent" => "JetBrowserEvent".to_string(),
            Type::Named(name) if name == "BrowserTrace" => "JetBrowserTrace".to_string(),
            Type::Named(name) if name == "BrowserReceipt" => "JetBrowserReceipt".to_string(),
            Type::Named(name) if name == "BrowserPrivacy" => "JetBrowserPrivacy".to_string(),
            Type::Named(name) if name == "BrowserError" => "JetBrowserError".to_string(),
            Type::Named(name) if name == "BrowserAbilities" => "JetBrowserAbilities".to_string(),
            Type::Named(name) if name == "BrowserProfile" => "JetBrowserProfile".to_string(),
            Type::Named(name) if name == "BrowserTimeout" => "JetBrowserTimeout".to_string(),
            Type::Named(name) if name == "BrowserProtocol" => "JetBrowserProtocol".to_string(),
            // c97/D-STRPARSE1: the builtin parse error (`Int.parse`, `Float.parse`)
            // erases to a plain message — never user-constructed.
            // A user enum named `ParseError` (in `type_names`) keeps its own lowering.
            Type::Named(name) if name == "ParseError" && !self.type_names.contains(name) => {
                "String".to_string()
            }
            // D-TYPEDSQL-SINK1=A: `SQL` carries checked template text and its
            // ordered `DBValue` bindings together. `HTML` is already the fully
            // escaped text, so it is just a `String` underneath.
            Type::Named(name) if name == "SQL" => format!("{}jet_std::SQL", self.root_prefix),
            Type::Named(name) if name == "HTML" => "String".to_string(),
            Type::Named(name) if name == "Sh" => "Vec<String>".to_string(),
            // D-DEFER1: ScopeGuard is generic over F (the closure type); emit `_`
            // so Rust infers the monomorphised type from the initialiser expression.
            Type::Named(name) if name == "ScopeGuard" => "_".to_string(),
            // D-TERM1 (ratified 2026-06-22): `Key` is a top-level prelude enum.
            Type::Named(name) if name == "Key" => format!("{}JetKey", self.root_prefix),
            Type::Named(name)
                if core_ui_rust_type_name(name).is_some()
                    && !self.type_names.contains(name) =>
            {
                format!("{}Jet{}", self.root_prefix, name)
            }
            Type::Named(name)
                if name == "UiFileFilterResult" && !self.type_names.contains(name) =>
            {
                format!("{}JetUiServiceResult<{}JetUiFileFilter>", self.root_prefix, self.root_prefix)
            }
            Type::Named(name)
                if name == "UiFsGrantResult" && !self.type_names.contains(name) =>
            {
                format!("{}JetUiServiceResult<{}JetUiFsGrant>", self.root_prefix, self.root_prefix)
            }
            Type::Named(name)
                if name == "UiShortcutResult" && !self.type_names.contains(name) =>
            {
                format!("{}JetUiServiceResult<{}JetUiShortcut>", self.root_prefix, self.root_prefix)
            }
            Type::Named(name)
                if name == "UiShortcutBindingResult" && !self.type_names.contains(name) =>
            {
                format!(
                    "{}JetUiServiceResult<{}JetUiShortcutBinding>",
                    self.root_prefix, self.root_prefix
                )
            }
            Type::Named(name)
                if name == "UiAccessibilityResult" && !self.type_names.contains(name) =>
            {
                format!(
                    "{}JetUiServiceResult<{}JetUiAccessibility>",
                    self.root_prefix, self.root_prefix
                )
            }
            Type::Named(name)
                if name == "UiFileDialogResult" && !self.type_names.contains(name) =>
            {
                format!(
                    "{}JetUiServiceResult<{}JetUiFileDialogSelection>",
                    self.root_prefix, self.root_prefix
                )
            }
            Type::Named(name)
                if name == "UiClipboardTextResult" && !self.type_names.contains(name) =>
            {
                format!(
                    "{}JetUiServiceResult<{}JetUiClipboardText>",
                    self.root_prefix, self.root_prefix
                )
            }
            Type::Named(name)
                if name == "UiClipboardWriteResult" && !self.type_names.contains(name) =>
            {
                format!(
                    "{}JetUiServiceResult<{}JetUiClipboardWrite>",
                    self.root_prefix, self.root_prefix
                )
            }
            Type::Named(name) if name == "UiImeResult" && !self.type_names.contains(name) => {
                format!(
                    "{}JetUiServiceResult<Option<{}JetUiImeEvent>>",
                    self.root_prefix, self.root_prefix
                )
            }
            Type::Named(name) if name == "UiDragResult" && !self.type_names.contains(name) => {
                format!(
                    "{}JetUiServiceResult<Option<{}JetUiDragEvent>>",
                    self.root_prefix, self.root_prefix
                )
            }
            Type::Named(name)
                if name == "UiShortcutDispatchResult" && !self.type_names.contains(name) =>
            {
                format!(
                    "{}JetUiServiceResult<{}JetUiShortcutDispatch>",
                    self.root_prefix, self.root_prefix
                )
            }
            Type::Named(name)
                if name == "UiAccessibilityAttachResult" && !self.type_names.contains(name) =>
            {
                format!("{}JetUiServiceResult<()>", self.root_prefix)
            }
            Type::Named(name)
                if name == "UiAccessibilityProjectionResult"
                    && !self.type_names.contains(name) =>
            {
                format!(
                    "{}JetUiServiceResult<Option<{}JetUiAccessibilityProjection>>",
                    self.root_prefix, self.root_prefix

                )
            }
            Type::Named(name)
                if name == "UiAccessibilityNodeResult" && !self.type_names.contains(name) =>
            {
                format!("{}JetUiServiceResult<{}JetUiNode>", self.root_prefix, self.root_prefix)
            }
            // D-SPACE-GEOMETRY1=A: static coordinate-space tags erase in the
            // native carrier. Frame identity is retained by JetTransform2,
            // not by allocating a marker per point.
            Type::Named(name)
                if matches!(
                    name.as_str(),
                    "ScreenPoint"
                        | "WorldPoint"
                        | "ViewPoint"
                        | "CameraPoint"
                        | "DevicePoint"
                        | "ScreenDelta"
                        | "WorldDelta"
                        | "ViewDelta"
                        | "CameraDelta"
                        | "DeviceDelta"
                ) && !self.type_names.contains(name) =>
            {
                format!("{}JetCoord2", self.root_prefix)
            }
            Type::Named(name)
                if matches!(name.as_str(), "TransformError" | "FrameId")
                    && !self.type_names.contains(name) =>
            {
                if name == "FrameId" {
                    format!("{}i64", self.root_prefix)
                } else {
                    format!("{}JetErr", self.root_prefix)
                }
            }
            // c-devserver (owner-directed 2026-07-01): DevServer is a
            // top-level prelude struct (Prelude/DevServer.rs).
            Type::Named(name) if name == "DevServer" && !self.type_names.contains(name) => {
                format!("{}JetDevServer", self.root_prefix)
            }
            // D-WEBAPP1=D: App / WebPage opaque builder types.
            Type::Named(name) if name == "App" && !self.type_names.contains(name) => {
                format!("{}JetApp", self.root_prefix)
            }
            Type::Named(name) if name == "WebPage" && !self.type_names.contains(name) => {
                format!("{}JetWebPage", self.root_prefix)
            }
            Type::Named(name) if name == "LiveQuery" && !self.type_names.contains(name) => {
                format!("{}JetLiveQuery", self.root_prefix)
            }
            Type::Named(name) if name == Syntax::TYPE_RANGE && !self.type_names.contains(name) => {
                format!("{}JetRange", self.root_prefix)
            }
            Type::Named(name) if name == "DimensionAxis" && !self.type_names.contains(name) => {
                format!("{}JetDimensionAxis", self.root_prefix)
            }
            Type::Named(name) if name == "DimensionInfo" && !self.type_names.contains(name) => {
                format!("{}JetDimensionInfo", self.root_prefix)
            }
            Type::Named(name) if name == "StateRef" && !self.type_names.contains(name) => {
                format!("{}JetStateRef", self.root_prefix)
            }
            Type::Named(name) if name == "StateInfo" && !self.type_names.contains(name) => {
                format!("{}JetStateInfo", self.root_prefix)
            }
            Type::Named(name) if name == "EffectInfo" && !self.type_names.contains(name) => {
                format!("{}JetEffectInfo", self.root_prefix)
            }
            Type::Named(name) if name == "OriginInfo" && !self.type_names.contains(name) => {
                format!("{}JetOriginInfo", self.root_prefix)
            }
            Type::Named(name) if name == Syntax::TYPE_EFFECT && !self.type_names.contains(name) => {
                format!("{}jet_std::JetReactiveEffect", self.root_prefix)
            }
            Type::Named(name)
                if name == Syntax::TYPE_SUBSCRIPTION && !self.type_names.contains(name) =>
            {
                format!("{}jet_std::JetSubscription", self.root_prefix)
            }
            Type::Named(name)
                if name == Syntax::TYPE_EVENT_SCOPE && !self.type_names.contains(name) =>
            {
                format!("{}jet_std::JetEventScope", self.root_prefix)
            }
            Type::Named(name)
                if name == Syntax::TYPE_EVENT_POLICY && !self.type_names.contains(name) =>
            {
                format!("{}jet_std::JetEventPolicy", self.root_prefix)
            }
            Type::Named(name)
                if name == Syntax::TYPE_EVENT_TRACE && !self.type_names.contains(name) =>
            {
                format!("{}jet_std::JetEventTrace", self.root_prefix)
            }
            Type::Named(name)
                if name == Syntax::TYPE_HOOK_POLICY && !self.type_names.contains(name) =>
            {
                format!("{}jet_std::JetHookPolicy", self.root_prefix)
            }
            // E2-M7: file handle types are top-level in the prelude (not in jet_std).
            Type::Named(name) if file_handle_rust_type(name).is_some() => {
                format!(
                    "{}{}",
                    self.root_prefix,
                    file_handle_rust_type(name).unwrap()
                )
            }
            // D-RAYLIB1=A: the raylib bridge lives in the
            // top-level corelib prelude today, so generated user bindings must
            // reference `RaylibWindow`/`RaylibColor` directly.
            Type::Named(name) if raylib_handle_rust_type(name).is_some() => {
                format!(
                    "{}{}",
                    self.root_prefix,
                    raylib_handle_rust_type(name).unwrap()
                )
            }
            Type::Named(name) if game_handle_rust_type(name).is_some() => {
                format!(
                    "{}{}",
                    self.root_prefix,
                    game_handle_rust_type(name).unwrap()
                )
            }
            // E2-M10: networking opaque types are top-level in the prelude.
            Type::Named(name) if net_handle_rust_type(name).is_some() => {
                format!(
                    "{}{}",
                    self.root_prefix,
                    net_handle_rust_type(name).unwrap()
                )
            }
            // D-COMPUTE1=D: Tensor / compute error / device handles.
            Type::Named(name) if compute_handle_rust_type(name).is_some() => {
                format!(
                    "{}{}",
                    self.root_prefix,
                    compute_handle_rust_type(name).unwrap()
                )
            }
            Type::Named(name) if service_handle_rust_type(name).is_some() => {
                format!(
                    "{}{}",
                    self.root_prefix,
                    service_handle_rust_type(name).unwrap()
                )
            }
            Type::Apply { name, args }
                if matches!(name.as_str(), "Point2" | "Delta2")
                    && args.len() == 2 =>
            {
                format!("{}JetCoord2", self.root_prefix)
            }
            Type::Apply { name, args }
                if matches!(name.as_str(), "Transform" | "Transform2")
                    && (args.len() == 2 || args.len() == 3) =>
            {
                format!("{}JetTransform2", self.root_prefix)
            }
            Type::Apply { name, args }
                if name == "Ray2" && args.len() == 3 =>
            {
                format!("{}JetRay2", self.root_prefix)
            }

            Type::Apply { name, args }
                if matches!(name.as_str(), "Tensor" | "Vec" | "Matrix")
                    && (name != "Tensor" || args.len() <= 1)
                    && compute_handle_rust_type("Tensor").is_some() =>
            {
                format!("{}JetTensor", self.root_prefix)
            }
            Type::Apply { name, args } if name == "VjpRun" && args.len() == 1 => {
                format!(
                    "{}JetComputeVjpRun<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            // D-ALLOC1/D-ALLOC-C (ratified 2026-06-19): allocator opaque types.
            Type::Named(name) if alloc_handle_rust_type(name).is_some() => {
                format!(
                    "{}{}",
                    self.root_prefix,
                    alloc_handle_rust_type(name).unwrap()
                )
            }
            // D-ARGS1 (ratified 2026-06-22): ArgsSpec / ParsedArgs are top-level prelude structs.
            Type::Named(name) if args_handle_rust_type(name).is_some() => {
                format!(
                    "{}{}",
                    self.root_prefix,
                    args_handle_rust_type(name).unwrap()
                )
            }
            // D-ANY-JAI1 (c7jaiany §6): `reflect.of(x)`'s Value/Field handles. `Value`
            // and `Field` are common enough words that a user struct sharing the name
            // is likely (`examples/features/memory/zerocopy.jet` already declares its
            // own `Field`) — same guard as the layout/core-rust-type-name arms below:
            // a user type of that name always wins.
            Type::Named(name)
                if reflect_handle_rust_type(name).is_some() && !self.type_names.contains(name) =>
            {
                format!(
                    "{}{}",
                    self.root_prefix,
                    reflect_handle_rust_type(name).unwrap()
                )
            }
            // D-SHIFT1 (c7shift): `binary.Reader` / `text.Cursor` — plausible
            // user type names, same collision guard as `Value`/`Field` above.
            Type::Named(name)
                if binary_text_handle_rust_type(name).is_some()
                    && !self.type_names.contains(name) =>
            {
                format!(
                    "{}{}",
                    self.root_prefix,
                    binary_text_handle_rust_type(name).unwrap()
                )
            }
            // D-LAYOUT1 / D-LAYOUT-GATES1: `layout` runtime types are top-level
            // in their own `jet_layout` module (like the alloc/file/net handles
            // above, not nested in `jet_std`).
            Type::Named(name)
                if layout_handle_rust_type(name).is_some() && !self.type_names.contains(name) =>
            {
                format!(
                    "{}{}",
                    self.root_prefix,
                    layout_handle_rust_type(name).unwrap()
                )
            }
            // D-TYPE2-IMAG1=A: Complex lives in the shared MathLibPure root,
            // not in the JetStd namespace used by the older precise numerics.
            Type::Named(name)
                if name == Syntax::TYPE_COMPLEX && !self.type_names.contains(name) =>
            {
                format!("{}JetComplex", self.root_prefix)
            }
            Type::Named(name)
                if root_prelude_rust_type_name(name).is_some()
                    && !self.type_names.contains(name) =>
            {
                format!(
                    "{}{}",
                    self.root_prefix,
                    root_prelude_rust_type_name(name).unwrap()
                )
            }
            // A user struct/enum sharing a built-in Core type name (e.g. a user
            // `Vec3`) wins — it keeps its own `__jet_<Name>` lowering. Only fall to the
            // built-in jet_std struct when the name is NOT a user type.
            Type::Named(name)
                if core_rust_type_name(name).is_some() && !self.type_names.contains(name) =>
            {
                format!(
                    "{}jet_std::{}",
                    self.root_prefix,
                    core_rust_type_name(name).unwrap()
                )
            }
            Type::Named(name) if self.core_qualified_rust_type_name(name).is_some() => {
                let resolved = self.core_qualified_rust_type_name(name).unwrap();
                if let Some(rust) = job_queue_rust_type(resolved) {
                    return format!("{}{rust}", self.root_prefix);
                }
                if resolved == "AllocError" {
                    return format!("{}AllocError", self.root_prefix);
                }
                if resolved == "Claims"
                    || resolved == "AuthError"
                    || resolved == "Session"
                    || resolved == "Auth"
                    || resolved == "SyncText"
                    || resolved == "SyncCounter"
                    || resolved == "SyncMap"
                    || resolved == "SyncList"
                    || resolved == "RowPolicy"
                {
                    let rust = match resolved {
                        "Claims" => "JetAuthClaims",
                        "AuthError" => "JetAuthError",
                        "Session" => "JetAuthSession",
                        "Auth" => "JetAuthApp",
                        "SyncText" => "JetSyncText",
                        "SyncCounter" => "JetSyncCounter",
                        "SyncMap" => "JetSyncMap",
                        "SyncList" => "JetSyncList",
                        "RowPolicy" => "JetRowPolicy",
                        _ => resolved,
                    };
                    return format!("{}{rust}", self.root_prefix);
                }
                if resolved == "Hasher" {
                    return format!("{}JetCryptoHasher", self.root_prefix);
                }
                if let Some(rust) = core_crypto_rust_type_name(resolved) {
                    let ffi = self.ffi_crate.as_deref().unwrap_or("jet_ffi");
                    return format!("{ffi}::{rust}");
                }
                if matches!(
                    resolved,
                    "Address"
                        | "Message"
                        | "Attachment"
                        | "Envelope"
                        | "SMTPSecurity"
                        | "RecipientPolicy"
                        | "RecipientReport"
                        | "SendReport"
                        | "EmailError"
                        | "Limits"
                ) {
                    let rust = if resolved == "EmailError" {
                        "Error"
                    } else {
                        resolved
                    };
                    return format!("{}jet_email::{rust}", self.root_prefix);
                }
                if matches!(resolved, "SMTPAuth" | "DkimConfig" | "SMTPConfig") {
                    let ffi = self.ffi_crate.as_deref().unwrap_or("jet_ffi");
                    return format!(
                        "{}jet_email::{}<{}::Secret>",
                        self.root_prefix, resolved, ffi
                    );
                }
                if resolved == "TLSTrust" || resolved == "Mailer" {
                    return format!("{}jet_email::{}", self.root_prefix, resolved);
                }
                format!("{}jet_std::{}", self.root_prefix, resolved)
            }
            Type::Named(name) if self.trait_names.contains(name) => {
                format!("Box<dyn {}>", crate::Codegen::rust_trait_name(name))
            }
            Type::Named(name) if self.foreign_types.contains_key(name.as_str()) => {
                let rust_mod = &self.foreign_types[name.as_str()];
                let leaf = nominal_leaf(name);
                format!("{}{}::{}", self.root_prefix, rust_mod, mangle_path(leaf))
            }
            Type::Named(name) if name.contains('.') => {
                let (alias, leaf) = name.split_once('.').unwrap();
                match self.import_mods.get(alias) {
                    Some(rust_mod) => {
                        format!("{}{}::{}", self.root_prefix, rust_mod, mangle_path(leaf))
                    }
                    None => mangle_path(name),
                }
            }
            Type::Named(n) if n == "Expired" => "JetExpired".to_string(),
            Type::Named(name) => mangle_path(name),
            // D-CONC-FAIL1=A: every source `Task<T>` uses one AOT handle
            // representation. A spawned body's private `Result` is the
            // scheduler value, and its error is projected to `TaskFailure` by
            // the join adapter; only an already-carried TIR task keeps its
            // existing inner error type.
            // D-PLACE1=A: `Atomic<T>` is the root Prelude's one-word carrier,
            // not a JetStd member. The exact-Int marker keeps source `Int`
            // distinct from fixed-width `I64` despite their shared i64 ABI.
            Type::Apply { name, args } if name == "Atomic" && args.len() == 1 => {
                let inner = if matches!(args[0].without_user_tags(), Type::Int) {
                    "JetAtomicInt".to_string()
                } else {
                    self.rust_type(&args[0])
                };
                format!("{}JetAtomic<{inner}>", self.root_prefix)
            }
            Type::Apply { name, args } if name == "Task" && !args.is_empty() => {
                let item = self.rust_type(&args[0]);
                let carrier = if matches!(args[0].without_user_tags(), Type::Result { .. }) {
                    item
                } else {
                    format!("Result<{item}, {}JetErr>", self.root_prefix)
                };
                format!("{}jet_std::JetTask<{carrier}>", self.root_prefix)
            }
            Type::Apply { name, args }
                if matches!(
                    name.as_str(),
                    "KeyRef" | "MutationPlan" | "VaultWrite" | "Rotation" | "WrappedImportPlan"
                ) && args.len() == 1 =>
            {
                let ffi = self.ffi_crate.as_deref().unwrap_or("jet_ffi");
                let rust = match name.as_str() {
                    "KeyRef" => "JetVaultKeyRef",
                    "MutationPlan" => "JetVaultMutationPlan",
                    "VaultWrite" => "JetVaultWrite",
                    "Rotation" => "JetVaultRotation",
                    _ => "JetVaultWrappedImportPlan",
                };
                format!("{ffi}::{rust}<{}>", self.rust_type(&args[0]))
            }
            // Generic Prelude handles share their canonical Rust names with MIR.
            Type::Apply { name, args }
                if !args.is_empty()
                    && matches!(
                        core_rust_type_name(name),
                        Some("JetReceiver")
                            | Some("JetSender")
                            | Some("JetPool")
                            | Some("JetId")
                    ) =>
            {
                let rust = core_rust_type_name(name).unwrap();
                format!(
                    "{}jet_std::{rust}<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            // D-LOCALCELL1=A: local interior-mutability handles and their
            // dynamically checked projected guards.
            Type::Apply { name, args } if name == "Cell" && !args.is_empty() => {
                format!(
                    "{}jet_std::JetCell<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args } if name == "CellReadGuard" && !args.is_empty() => {
                format!(
                    "{}jet_std::JetCellReadGuard<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args } if name == "CellEditGuard" && !args.is_empty() => {
                format!(
                    "{}jet_std::JetCellEditGuard<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            // D-CONC-STREAM1=A: a generator's `Stream<T>` is the Prelude's
            // rendezvous receiver. Its owned iterator closes the receiver when
            // D-FOUND-COREAPI1 / #2853: event-time stream adapters retain the
            // shared kernel's event, keyed-view, and window carriers.
            Type::Apply { name, args }
                if name == "StreamEventTime" && args.len() == 1 =>
            {
                format!(
                    "{}jet_std::JetStream<{}jet_std::JetStreamEvent<{}>>",
                    self.root_prefix,
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args }
                if name == "KeyedStream" && args.len() == 2 =>
            {
                format!(
                    "{}jet_std::JetKeyedStream<{}, {}>",
                    self.root_prefix,
                    self.rust_type(&args[0]),
                    self.rust_type(&args[1])
                )
            }
            Type::Apply { name, args } if name == "Window" && args.len() == 2 => {
                format!(
                    "{}jet_std::JetStreamWindow<{}, {}>",
                    self.root_prefix,
                    self.rust_type(&args[0]),
                    self.rust_type(&args[1])
                )
            }
            // a consumer breaks or drops it, so a blocked producer observes the
            // same cancellation rule on every emitted program.
            Type::Apply { name, args } if name == Syntax::TYPE_STREAM && !args.is_empty() => {
                format!(
                    "{}jet_std::JetStream<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            // D-REACT1=B: reactive handle types lower to the std-only jet_std runtime.
            Type::Apply { name, args } if name == Syntax::TYPE_SIGNAL && !args.is_empty() => {
                format!(
                    "{}jet_std::JetSignal<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args }
                if (name == Syntax::TYPE_DERIVED || name == Syntax::TYPE_COMPUTED)
                    && !args.is_empty() =>
            {
                format!(
                    "{}jet_std::JetDerived<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args } if name == Syntax::TYPE_EVENT && !args.is_empty() => {
                format!(
                    "{}jet_std::JetEvent<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args } if name == Syntax::TYPE_ASYNC_EVENT && args.len() == 2 => {
                format!(
                    "{}jet_std::JetAsyncEvent<{}, {}>",
                    self.root_prefix,
                    self.rust_type(&args[0]),
                    self.rust_type(&args[1])
                )
            }
            Type::Apply { name, args }
                if name == Syntax::TYPE_DISPATCH_REPORT && !args.is_empty() =>
            {
                format!(
                    "{}jet_std::JetDispatchReport<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args }
                if name == Syntax::TYPE_DISPATCH_FAILURE && !args.is_empty() =>
            {
                format!(
                    "{}jet_std::JetDispatchFailure<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args } if name == Syntax::TYPE_HOOK && args.len() == 2 => {
                format!(
                    "{}jet_std::JetHook<{}, {}>",
                    self.root_prefix,
                    self.rust_type(&args[0]),
                    self.rust_type(&args[1])
                )
            }
            Type::Apply { name, args } if name == Syntax::TYPE_DECISION_HOOK && args.len() == 2 => {
                format!(
                    "{}jet_std::JetDecisionHook<{}, {}>",
                    self.root_prefix,
                    self.rust_type(&args[0]),
                    self.rust_type(&args[1])
                )
            }
            Type::Apply { name, args } if name == Syntax::TYPE_HOOK_DECISION && args.len() == 2 => {
                format!(
                    "{}jet_std::JetHookDecision<{}, {}>",
                    self.root_prefix,
                    self.rust_type(&args[0]),
                    self.rust_type(&args[1])
                )
            }
            Type::Apply { name, args } if name == Syntax::TYPE_HOOK_OUTCOME && args.len() == 2 => {
                format!(
                    "{}jet_std::JetHookOutcome<{}, {}>",
                    self.root_prefix,
                    self.rust_type(&args[0]),
                    self.rust_type(&args[1])
                )
            }
            // D-QUERY-RETAIN1=A: Query<T> defers a typed computation, while
            // Group<K, V> retains both nominal key and reducer value types.
            Type::Apply { name, args }
                if name == "Query"
                    && args.len() == 1
                    && !self.type_names.contains(name) =>
            {
                format!(
                    "{}jet_std::DataQuery<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args }
                if name == "Group"
                    && args.len() == 2
                    && !self.type_names.contains(name) =>
            {
                format!(
                    "{}jet_std::GroupValue<{}, {}>",
                    self.root_prefix,
                    self.rust_type(&args[0]),
                    self.rust_type(&args[1])
                )
            }
            Type::Apply { name, args }
                if name == "DataGroupedQuery"
                    && args.len() == 2
                    && !self.type_names.contains(name) =>
            {
                format!(
                    "{}jet_std::DataGroupedQuery<{}, {}>",
                    self.root_prefix,
                    self.rust_type(&args[0]),
                    self.rust_type(&args[1])
                )
            }
            Type::Apply { name, args }
                if name == "DataLoader"
                    && args.len() == 1
                    && !self.type_names.contains(name) =>
            {
                format!(
                    "{}jet_std::DataLoader<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args }
                if name == Syntax::TYPE_SHARED_SNAPSHOT
                    && args.len() == 2
                    && !self.type_names.contains(name) =>
            {
                format!(
                    "{}jet_std::JetSharedSnapshot<{}, {}>",
                    self.root_prefix,
                    self.rust_type(&args[0]),
                    self.rust_type(&args[1])
                )
            }
            Type::Apply { name, args }
                if name == "DataSnapshot"
                    && args.len() == 1
                    && !self.type_names.contains(name) =>
            {
                format!(
                    "{}jet_std::DataSnapshot<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args }
                if name == "JetDataPlotColumn"
                    && args.len() == 1
                    && !self.type_names.contains(name) =>
            {
                format!(
                    "{}JetDataPlotColumn<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args }
                if name == "JetDataPlot"
                    && args.len() == 1
                    && !self.type_names.contains(name) =>
            {
                format!(
                    "{}JetDataPlot<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            // D-DATAFLOW1=A: pull stream is one opaque Rust handle; Jet keeps T.
            Type::Apply { name, args }
                if name == "DataStream" && !args.is_empty() && !self.type_names.contains(name) =>
            {
                format!("{}jet_std::DataStream", self.root_prefix)
            }
            Type::Apply { name, args }
                if name == "DataJoin" && args.len() == 2 && !self.type_names.contains(name) =>
            {
                format!(
                    "{}jet_std::DataJoin<{}, {}>",
                    self.root_prefix,
                    self.rust_type(&args[0]),
                    self.rust_type(&args[1])
                )
            }
            // D-HONESTNUM1=A: Measurement<T> → jet_std::JetMeasurement<T>.
            Type::Apply { name, args } if name == Syntax::TYPE_MEASUREMENT && !args.is_empty() => {
                format!(
                    "{}jet_std::JetMeasurement<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args } if name == Syntax::TYPE_SHARED_GUARD && args.len() == 1 => {
                format!(
                    "{}jet_std::JetSharedGuard<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args } if name == Syntax::TYPE_SHARED_WEAK && args.len() == 1 => {
                format!(
                    "{}jet_std::JetSharedWeak<{}>",
                    self.root_prefix,
                    self.rust_type(&args[0])
                )
            }
            // D-PENDING1=B: Loadable<T,E> → JetLoadable<T,E>.
            Type::Apply { name, args } if name == "Loadable" && args.len() == 2 => {
                format!(
                    "JetLoadable<{}, {}>",
                    self.rust_type(&args[0]),
                    self.rust_type(&args[1])
                )
            }
            // D-DYNARRAY1: View<T> -> a genuine borrowed Rust slice `&[T]`. Zero-
            // copy (a slice is a plain (ptr, len) pair, no allocation) and
            // ordinary safe Rust — the lifetime is elided, valid as long as the
            // borrow stays local, exactly the shape sema's E2305 owner-outlives
            // check proves before this type is ever emitted (I2/I3: sema
            // decides, codegen just emits the reference).
            Type::Apply { name, args } if name == "View" && args.len() == 1 => {
                if matches!(&args[0], Type::Named(inner) if inner == "str") {
                    "&str".to_string()
                } else if let Type::List(inner) = &args[0] {
                    format!("&[{}]", self.rust_type(inner))
                } else {
                    format!("&[{}]", self.rust_type(&args[0]))
                }
            }
            Type::Apply { name, args } if name == "ComputeViewMut" && args.len() == 1 => {
                format!("{}JetComputeViewMut<'_>", self.root_prefix)
            }
            Type::Apply { name, args } if name == "ViewMut" && args.len() == 1 => {
                format!("&mut [{}]", self.rust_type(&args[0]))
            }
            // D-PIN1=A: `Pin<T>` is the address-stability contract, which sema
            // proves before this type is ever emitted (I3). The value itself is
            // an ordinary exclusive Rust reference to the pinned place — that is
            // exactly what "the storage does not move while the pin is live"
            // means once the proof is done, so no runtime wrapper is emitted.
            Type::Apply { name, args } if name == Syntax::TYPE_PIN && args.len() == 1 => {
                format!("&mut {}", self.rust_type(&args[0]))
            }
            // D-ITERTOOLS1=A: Iter<T> → JetIter<T> (must-use move-only lazy view).
            Type::Apply { name, args } if name == Syntax::TYPE_ITER && args.len() == 1 => {
                format!(
                    "{}{}<{}>",
                    self.root_prefix,
                    root_prelude_rust_type_name(name).expect("registered iterator carrier"),
                    self.rust_type(&args[0])
                )
            }
            // D-FOUND-VIEW1=A: `ViewIter<T>` retains the source owner while
            // yielding borrowed windows; its carrier is not an eager `Iter`.
            Type::Apply { name, args } if name == Syntax::TYPE_VIEW_ITER && args.len() == 1 => {
                format!("JetViewIter<{}>", self.rust_type(&args[0]))
            }
            // D-CORE-SECRETS1=A: generic TTL stays distinct from secret lifecycle.
            Type::Apply { name, args }
                if name == Syntax::EXPIRING_VALUE_TYPE && args.len() == 1 =>
            {
                format!("JetExpiring<{}>", self.rust_type(&args[0]))
            }
            // D-TTLVAL1=A: the one secret-lifetime wrapper.
            Type::Apply { name, args } if name == "ExpiringSecret" && args.len() == 1 => {
                format!("JetExpiringSecret<{}>", self.rust_type(&args[0]))
            }
            // D-COLLBREADTH1=A: Set<T> → HashSet<T>, Queue<T> → VecDeque<T>.
            Type::Apply { name, args } if name == "Set" && !args.is_empty() => {
                format!("std::collections::HashSet<{}>", self.rust_type(&args[0]))
            }
            Type::Apply { name, args } if name == Syntax::TYPE_RANK && !args.is_empty() => {
                format!("std::collections::BTreeSet<{}>", self.rust_type(&args[0]))
            }
            Type::Apply { name, args }
                if name == Syntax::TYPE_PRIORITY_QUEUE && !args.is_empty() =>
            {
                format!("std::collections::BinaryHeap<{}>", self.rust_type(&args[0]))
            }
            Type::Apply { name, args } if name == Syntax::TYPE_LRU && args.len() >= 2 => {
                format!(
                    "JetCache<{}, {}>",
                    self.rust_type(&args[0]),
                    self.rust_type(&args[1])
                )
            }
            // D-TAG1: Tally<T> → HashMap<T, usize>.
            Type::Apply { name, args } if name == crate::Syntax::TYPE_TALLY && !args.is_empty() => {
                format!(
                    "std::collections::HashMap<{}, usize>",
                    self.rust_type(&args[0])
                )
            }
            Type::Apply { name, args } if name == crate::Syntax::TYPE_QUEUE && !args.is_empty() => {
                format!("std::collections::VecDeque<{}>", self.rust_type(&args[0]))
            }
            // S58 (E2-M13): `Ptr<T>` lowers to a Rust raw pointer `*mut T`.
            // Memory safety is enforced in sema (the `#Unsafe` gate); codegen
            // is dumb.
            Type::Apply { name, args } if name == Syntax::TYPE_PTR && args.len() == 1 => {
                format!("*mut {}", self.rust_type(&args[0]))
            }
            Type::Apply { name, args } => {
                let head = if let Some((alias, leaf)) = name.split_once('.') {
                    self.import_mods.get(alias).map_or_else(
                        || mangle_path(name),
                        |rust_mod| {
                            format!("{}{}::{}", self.root_prefix, rust_mod, mangle_path(leaf))
                        },
                    )
                } else {
                    mangle_path(name)
                };
                if args.is_empty() {
                    head
                } else {
                    format!(
                        "{}<{args}>",
                        head,
                        args = args
                            .iter()
                            .map(|a| self.rust_type(a))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            }
            // Codegen only ever constructs/sees a singleton `TraitObject` (the
            // D-ANY-JAI1 multi-trait bound loop element never reaches codegen —
            // see the type's doc comment); join defensively rather than assume.
            Type::TraitObject(t) => format!(
                "Box<dyn {}>",
                t.iter()
                    .map(|n| crate::Codegen::rust_trait_name(n))
                    .collect::<Vec<_>>()
                    .join(" + ")
            ),
            Type::Fn {
                params,
                ret,
                return_view_provenance,
                ..
            } => self.rust_fn_trait(params, ret.as_deref(), return_view_provenance.as_ref()),
            Type::Tuple(fields) => tuple_struct_name(&tuple_fields_plain(fields)),
            // D-FIXARR1 (ratified 2026-06-22): [T#N] lowers to a real Rust stack array [T; N].
            // All size/bounds checks live in sema (I3). The Rust type is [E; N].
            Type::FixedList { elem, len, .. } => format!("[{}; {}]", self.rust_type(elem), len),
            // D-QUAL4=A: tagged types are transparent to codegen.
            Type::Tagged { marker, inner }
                if matches!(
                    marker,
                    crate::AST::TagMarker::Internal(crate::AST::InternalTag::AllocatorView)
                ) =>
            {
                format!("&mut {}", self.rust_type(inner))
            }
            Type::Tagged { inner, .. } => self.rust_type(inner),
            // D-UNIONTYPE1=A: closed structural sum → one compiler-generated enum.
            Type::Union(members) => {
                jet_foundation::Names::mangle(&crate::AST::union_enum_name(members))
            }
            // Erased by the `quantity_parts()` guard above `rust_type` returns
            // through; a runtime quantity value IS its base numeric type.
            Type::Quantity { .. } => unreachable!("quantity_parts() erased above"),
            // A compile-time measure only ever appears as a `Vec`/`Matrix`
            // shape arg, intercepted by name above before reaching the
            // generic `Type::Apply` args recursion that would call here.
            Type::Measure(_) => {
                unreachable!("type measure handled by the Vec/Matrix Apply arm above")
            }
        }
    }

    pub(crate) fn rust_fn_trait(
        &self,
        params: &[Type],
        ret: Option<&Type>,
        return_view_provenance: Option<&crate::AST::ViewProvenanceMap>,
    ) -> String {
        let thread_safe = params.len() == 1
            && matches!(&params[0], Type::Named(name) if name == "HTTPHandler")
            && matches!(ret, Some(Type::Named(name)) if name == "HTTPHandler");
        let has_view_return = ret.is_some_and(|ty| self.type_contains_view(ty));
        let owner_params: HashSet<usize> = return_view_provenance
            .map(|map| {
                map.values()
                    .flat_map(|provenance| provenance.sources.iter())
                    .filter_map(|source| match source.source {
                        crate::AST::ViewSource::Parameter(index) => Some(index),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_else(|| {
                has_view_return
                    .then(|| {
                        params
                            .iter()
                            .enumerate()
                            .filter_map(|(index, ty)| (!ty.is_scalar()).then_some(index))
                            .collect()
                    })
                    .unwrap_or_default()
            });
        let mut independent_lifetimes = Vec::new();
        // HRTB binder must not reuse the enclosing function's `'__jet___view`
        // (E0496 shadow when a view-returning fn takes a view-returning callback).
        let fn_view = jet_format!("'{jet_prefix}fn_view");
        let ps = params
            .iter()
            .enumerate()
            .map(|(index, p)| {
                if thread_safe {
                    self.rust_type(p)
                } else {
                    let rust = rust_param_type(self, AccessConvention::Read, p);
                    if owner_params.contains(&index) {
                        if let Some(rest) = rust.strip_prefix("&mut ") {
                            format!("&{fn_view} mut {rest}")
                        } else if let Some(rest) = rust.strip_prefix('&') {
                            format!("&{fn_view} {rest}")
                        } else {
                            rust
                        }
                    } else if has_view_return {
                        let lifetime = jet_format!("'{jet_prefix}arg{index}");
                        if let Some(rest) = rust.strip_prefix("&mut ") {
                            independent_lifetimes.push(lifetime.clone());
                            format!("&{lifetime} mut {rest}")
                        } else if let Some(rest) = rust.strip_prefix('&') {
                            independent_lifetimes.push(lifetime.clone());
                            format!("&{lifetime} {rest}")
                        } else {
                            rust
                        }
                    } else {
                        rust
                    }
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let r = ret
            .map(|t| {
                if has_view_return {
                    self.rust_type_with_view_lifetime(t)
                        .replace(&jet_format!("'{jet_prefix}view"), &fn_view)
                } else {
                    self.rust_type(t)
                }
            })
            .unwrap_or_else(|| "()".to_string());
        // Every source-facing function value uses one owned, cloneable slot.
        // `RefCell` is only borrowed to take/restore the callback around a
        // call (see `emit_tir_fn_value_call`); user code never runs while the
        // slot borrow is live. The inner `FnMut` is therefore an ABI detail,
        // not a second source-level closure kind: S47 already gives every
        // escaping closure owned mutable captures.
        let trait_name = "FnMut";
        if thread_safe {
            format!("std::sync::Arc<dyn Fn({ps}) -> {r} + Send + Sync>")
        } else if has_view_return {
            let lifetimes = std::iter::once(fn_view.clone())
                .chain(independent_lifetimes)
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "std::rc::Rc<std::cell::RefCell<Option<Box<dyn for<{lifetimes}> {trait_name}({ps}) -> {r}>>>>"
            )
        } else {
            format!("std::rc::Rc<std::cell::RefCell<Option<Box<dyn {trait_name}({ps}) -> {r}>>>>")
        }
    }


}


pub(crate) fn rust_param_type(cx: &Cx, convention: AccessConvention, ty: &Type) -> String {
    if let Type::Tagged { marker, inner } = ty {
        if matches!(
            marker,
            crate::AST::TagMarker::Internal(crate::AST::InternalTag::CppCallbackAbi)
        ) {
            if let Type::Fn { params, ret, .. } = inner.as_ref() {
                let params = params
                    .iter()
                    .map(|param| cx.rust_type(param))
                    .collect::<Vec<_>>()
                    .join(", ");
                let ret = ret
                    .as_deref()
                    .map(|ret| format!(" -> {}", cx.rust_type(ret)))
                    .unwrap_or_default();
                return format!("extern \"C\" fn({params}){ret}");
            }
        }
    }
    let base = cx.rust_type(ty);
    if matches!(ty, Type::Named(n) if cx.trait_names.contains(n))
        || matches!(ty, Type::TraitObject(_))
    {
        return match convention {
            AccessConvention::Read => {
                format!("&{base}")
            }
            AccessConvention::Write => format!("&mut {base}"),
            AccessConvention::Move => base,
        };
    }
    // Type parameters obey the same explicit access convention as concrete
    // types. D-NETIO-CONTRACT2 relies on `&stream: T` becoming `&mut T`;
    // forcing every generic parameter by value would move non-cloneable handles.
    if matches!(ty, Type::Named(n) if Generics::is_type_var_name(n)
        || cx.current_type_params.borrow().contains(n.as_str()))
    {
        return match convention {
            AccessConvention::Read => {
                format!("&{base}")
            }
            AccessConvention::Write => format!("&mut {base}"),
            AccessConvention::Move => base,
        };
    }
    match convention {
        AccessConvention::Read if ty.is_scalar() => base,
        AccessConvention::Read => format!("&{}", base),
        AccessConvention::Write => format!("&mut {}", base),
        AccessConvention::Move => base,
    }
}


#[cfg(test)]
pub(crate) fn build_cx(prog: &Program, src: &str, file: &str) -> Cx {
    let extern_funcs = extern_func_map(&prog.items, None, &[]);
    build_cx_items(&prog.items, src, file, None, &extern_funcs, "")
}

fn extern_func_map(
    items: &[Item],
    owner: Option<&str>,
    handles: &[FfiHandleFact],
) -> HashMap<String, ExternFn> {
    fn collect(
        items: &[Item],
        owner: Option<&str>,
        handles: &[FfiHandleFact],
        map: &mut HashMap<String, ExternFn>,
    ) {
        for item in items {
            if let Item::ExternRust(block) = item {
                for ef in &block.functions {
                    map.insert(
                        ef.name.clone(),
                        ExternFn {
                            wrapper: format!("jet_ffi_{}", ef.name),
                            c_abi: false,
                            component: None,
                        },
                    );
                }
            } else if let Item::Func(func) = item {
                if crate::Sema::guest_import_function_signature(func).is_some()
                    && crate::Sema::guest_import_bridge_compatible(func)
                {
                    let wrapper = owner
                        .map(|owner| crate::Sema::guest_import_wrapper_name(owner, &func.name))
                        .unwrap_or_else(|| format!("jet_ffi_{}", func.name));
                    map.insert(
                        func.name.clone(),
                        ExternFn {
                            wrapper,
                            c_abi: true,
                            component: None,
                        },
                    );
                } else if func.inline_foreign.is_some() {
                    map.insert(
                        func.name.clone(),
                        ExternFn {
                            wrapper: format!("jet_ffi_{}", func.name),
                            c_abi: false,
                            component: None,
                        },
                    );
                }
            } else if let Item::CModule(module) = item {
                for function in module
                    .functions
                    .iter()
                    .filter(|function| {
                        function.hidden_c_bridge_compatible_with_handles(handles)
                    })
                {
                    map.insert(
                        function.name.clone(),
                        ExternFn {
                            wrapper: format!("jet_ffi_{}", function.name),
                            c_abi: true,
                            component: Some(module.lib.clone()),
                        },
                    );
                }
            } else if let Item::CodeModule(module) = item {
                if let Some(body) = &module.body {
                    collect(body, owner, handles, map);
                }
            }
        }
    }
    let mut map = HashMap::new();
    collect(items, owner, handles, &mut map);
    map
}

fn foreign_undo_map(items: &[Item]) -> HashMap<String, String> {
    fn collect(items: &[Item], map: &mut HashMap<String, String>, prefix: Option<&str>) {
        for item in items {
            match item {
                Item::ExternRust(block) => {
                    for ef in &block.functions {
                        if let Some((inverse, _)) = &ef.undo {
                            map.insert(
                                prefix
                                    .map(|p| format!("{p}::{}", ef.name))
                                    .unwrap_or_else(|| ef.name.clone()),
                                inverse.clone(),
                            );
                        }
                    }
                }
                Item::CModule(module) => {
                    for ef in &module.functions {
                        if let Some((inverse, _)) = &ef.undo {
                            map.insert(
                                prefix
                                    .map(|p| format!("{p}::{}", ef.name))
                                    .unwrap_or_else(|| ef.name.clone()),
                                inverse.clone(),
                            );
                        }
                    }
                }
                Item::Func(func) => {
                    if let Some((inverse, _)) = &func.undo {
                        map.insert(
                            prefix
                                .map(|p| format!("{p}::{}", func.name))
                                .unwrap_or_else(|| func.name.clone()),
                            inverse.clone(),
                        );
                    }
                }
                Item::CodeModule(module) => {
                    if let Some(body) = &module.body {
                        collect(body, map, Some(&module.name));
                    }
                }
                _ => {}
            }
        }
    }
    let mut map = HashMap::new();
    collect(items, &mut map, None);
    map
}

pub(crate) fn bundle_extern_funcs(bundle: &ProgramBundle) -> HashMap<String, ExternFn> {
    let mut map = HashMap::new();
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        let module_funcs = extern_func_map(&module.items, Some(&module.alias), &bundle.cffi.handle_facts);
        for (name, wrapper) in module_funcs {
            map.insert(name.clone(), wrapper.clone());
            map.insert(format!("{}::{name}", mangle(&module.alias)), wrapper);
        }
        if module.display.starts_with("cpp.") {
            for item in &module.items {
                if let Item::Impl(def) = item {
                    for method in def.methods.iter().filter(|method| {
                        bundle
                            .name_ledger
                            .exported(module_idx, &format!("{}{}", def.type_name, method.name))
                    }) {
                        map.insert(
                            foreign_binding_method_key(&def.type_name, &method.name),
                            ExternFn {
                                wrapper: String::new(),
                                c_abi: false,
                                component: None,
                            },
                        );
                    }
                }
            }
        }
    }
    map
}

pub(crate) fn bundle_foreign_undos(
    bundle: &ProgramBundle,
    module_idx: usize,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for module in &bundle.modules {
        let module_undos = foreign_undo_map(&module.items);
        let module_prefix = mangle(&module.alias);
        for (binding, inverse) in module_undos {
            // Bundle facts are keyed by the defining foreign binding, never by
            // a bare spelling that another module can silently overwrite.
            map.insert(format!("{module_prefix}::{binding}"), inverse);
        }
    }
    // Bare calls resolve only within the module currently being lowered.
    map.extend(foreign_undo_map(&bundle.modules[module_idx].items));
    map
}

pub(crate) fn foreign_binding_method_key(owner: &str, method: &str) -> String {
    jet_foundation::Names::mangle_path(&format!("foreign_method__{owner}__{method}"))
}

/// Add local and imported unit-family facts under the names used by this module.
pub(crate) fn register_bundle_unit_metadata(
    cx: &mut Cx,
    bundle: &ProgramBundle,
    module_idx: usize,
) {
    let mut imported = Vec::new();
    for import in &bundle.modules[module_idx].imports {
        if bundle
            .name_ledger
            .effective_alias(module_idx, &import.import_alias())
            .is_none()
        {
            continue;
        }
        let Some(target) = bundle.name_ledger.import_target(module_idx, import.span) else {
            continue;
        };
        let qualifier = bundle
            .name_ledger
            .module_identity(target)
            .expect("name ledger must contain every loaded module");
        imported.push((target, Some(qualifier)));
    }
    for (target, _) in crate::Codegen::Imports::selective_nominal_targets(bundle, module_idx) {
        if imported.iter().all(|(existing, _)| *existing != target) {
            let qualifier = bundle
                .name_ledger
                .module_identity(target)
                .expect("name ledger must contain every loaded module");
            imported.push((target, Some(qualifier)));
        }
    }
    for (_, target) in bundle.modules[module_idx]
        .imports
        .iter()
        .flat_map(|import| super::Imports::foreign_list_targets(bundle, module_idx, import))
    {
        if imported.iter().all(|(existing, _)| *existing != target) {
            let qualifier = bundle
                .name_ledger
                .module_identity(target)
                .expect("name ledger must contain every loaded module");
            imported.push((target, Some(qualifier)));
        }
    }
    for (target, qualifier) in std::iter::once((module_idx, None)).chain(imported) {
        let module = &bundle.modules[target];
        for item in &module.items {
            if let Item::Distinct(definition) = item {
                if qualifier.is_some()
                    && !bundle
                        .name_ledger
                        .visible(module_idx, target, &definition.name)
                {
                    continue;
                }
                let name = qualifier.as_ref().map_or_else(
                    || definition.name.clone(),
                    |qualifier| format!("{qualifier}::{}", definition.name),
                );
                let base = qualifier.as_ref().map_or_else(
                    || definition.base.clone(),
                    |_| {
                        super::Imports::qualify_imported_call_type(
                            bundle,
                            target,
                            "",
                            &definition.base,
                        )
                    },
                );
                cx.type_names.insert(name.clone());
                cx.distinct_types.insert(
                    name,
                    (
                        base,
                        definition
                            .derives
                            .iter()
                            .any(|(derive, _)| derive == crate::Syntax::MARKER_NUMERIC),
                    ),
                );
                continue;
            }
            if let Item::UnitFamily(family) = item {
                let dimension = family.resolved_dimension.clone();
                for member in family.distinct_defs() {
                    if qualifier.is_some()
                        && !bundle.name_ledger.visible(module_idx, target, &member.name)
                    {
                        continue;
                    }
                    let name = qualifier.as_ref().map_or_else(
                        || member.name.clone(),
                        |qualifier| format!("{qualifier}::{}", member.name),
                    );
                    cx.type_names.insert(name.clone());
                    cx.distinct_types.insert(
                        name.clone(),
                        (
                            qualifier.as_ref().map_or_else(
                                || member.base.clone(),
                                |_| {
                                    super::Imports::qualify_imported_call_type(
                                        bundle,
                                        target,
                                        "",
                                        &member.base,
                                    )
                                },
                            ),
                            member
                                .derives
                                .iter()
                                .any(|(derive, _)| derive == crate::Syntax::MARKER_NUMERIC),
                        ),
                    );
                    let kind = member
                        .quantity
                        .map(|(_, kind)| kind)
                        .unwrap_or(crate::AST::QuantityKind::Linear);
                    let Some(source) = unit_family_member_for_type(family, &member.name, kind)
                    else {
                        continue;
                    };
                    cx.unit_labels
                        .insert(name.clone(), unit_label(family, source));
                    if family.base.is_some() || dimension.is_some() {
                        cx.unit_facts
                            .insert(name, unit_fact(family, source, dimension.clone(), kind));
                    }
                }
            }
        }
        if let Some(qualifier) = &qualifier {
            for item in &module.items {
                let Item::Impl(implementation) = item else {
                    continue;
                };
                let qualified = format!("{qualifier}::{}", implementation.type_name);
                if implementation.trait_name.as_deref() == Some(Syntax::TRAIT_DISPLAY)
                    && cx.unit_labels.contains_key(&qualified)
                {
                    cx.display_types.insert(qualified);
                }
            }
        }
    }
}

/// The one checked namespace identity for a loaded module: the name ledger's
/// `package::path`, or the loader display path for a module the ledger does
/// not know.  Every TIR/MIR module key (module rows, function owners, type
/// owners, item order) must derive from this same string.
pub(crate) fn module_identity(bundle: &ProgramBundle, module_idx: usize) -> String {
    bundle
        .name_ledger
        .module_identity(module_idx)
        .or_else(|| bundle.modules.get(module_idx).map(|module| module.display.clone()))
        .unwrap_or_default()
}

/// Mirror the bundle-level import maps `emit_bundle` fills before lowering.
/// `build_cx_items` alone leaves `core_imports` empty; without this, JIT
/// lowering mis-gates `use core.tasks as tasks` channel calls.
pub(crate) fn populate_cx_from_bundle(cx: &mut Cx, bundle: &ProgramBundle, module_idx: usize) {
    use super::Imports::{
        core_import_map, foreign_type_map, import_mod_map, import_ret_map, import_sig_map,
        inline_core_import_maps, inline_foreign_import_maps, inline_foreign_import_signature_maps,
        inline_foreign_reexport_maps, inline_foreign_reexport_signature_maps, inline_import_maps,
        reexport_call_map, register_foreign_enum_variants, unqualified_import_maps,
        update_cloneability_with_foreign_types,
    };
    cx.import_mods = import_mod_map(bundle, module_idx);
    cx.module_alias = bundle.modules[module_idx].alias.clone();
    cx.module_identity = module_identity(bundle, module_idx);
    // Bundle construction assigns the module identity after `build_cx_items`;
    cx.devtools_registry = bundle.devtools_registry.clone();
    if let Some(module) = bundle.name_ledger.module(module_idx) {
        cx.devtools_package = module.package.clone();
        cx.devtools_module = module.path.clone();
    } else {
        cx.devtools_package.clear();
        cx.devtools_module.clear();
    }
    cx.policy_declarations = bundle.modules[module_idx].policy_declarations.clone();
    populate_cx_module_facts(cx, bundle, module_idx);
    cx.opaque_handles = bundle
        .cffi
        .handle_facts
        .iter()
        .map(|fact| fact.jet_name.clone())
        .collect();
    cx.core_archive_source = bundle
        .modules
        .iter()
        .any(|module| module.alias == "core_archive");
    cx.foreign_types = foreign_type_map(bundle, module_idx);
    crate::Codegen::TIR::register_imported_struct_shapes(cx, bundle, module_idx);
    update_cloneability_with_foreign_types(cx, &bundle.modules[module_idx].items);
    register_foreign_enum_variants(cx, bundle, module_idx);
    cx.reexport_calls = reexport_call_map(bundle, module_idx);
    cx.import_sigs = import_sig_map(bundle, module_idx);
    cx.import_rets = import_ret_map(bundle, module_idx);
    cx.core_imports = core_import_map(bundle, module_idx);
    register_bundle_reflect_paths(cx, bundle, module_idx);
    register_core_close_types(cx);
    register_core_import_surfaces(cx);
    cx.used_core = bundle.used_core.clone();
    cx.foreign_undos = bundle_foreign_undos(bundle, module_idx);
    cx.ffi_callback_fns = bundle.ffi_callback_fns.clone();
    register_bundle_unit_metadata(cx, bundle, module_idx);
    let (uinline, ufile) = unqualified_import_maps(bundle, module_idx);
    cx.unqualified_inline = uinline;
    cx.unqualified_file = ufile;
    let (inline, file, names, reexports) = inline_import_maps(bundle, module_idx);
    cx.inline_unqualified = inline;
    cx.inline_unqualified_file = file;
    cx.inline_import_names = names;
    cx.inline_reexport_inline = reexports;
    let (inline_core, reexport_core) = inline_core_import_maps(bundle, module_idx);
    cx.inline_core_imports = inline_core;
    cx.inline_reexport_core = reexport_core;
    cx.inline_foreign_imports = inline_foreign_import_maps(bundle, module_idx);
    let (inline_foreign_sigs, inline_foreign_rets) =
        inline_foreign_import_signature_maps(bundle, module_idx);
    cx.inline_foreign_sigs = inline_foreign_sigs;
    cx.inline_foreign_rets = inline_foreign_rets;
    cx.inline_reexport_foreign = inline_foreign_reexport_maps(bundle, module_idx);
    // Keep the preview identity on the same canonical target/build facts as
    // artifact caching.  A readable tuple here would silently omit provider,
    // closure, linker, dependency, or runtime-layer changes.
    let mut preview_identity = b"jet-ui-preview-build-v1\0".to_vec();
    bundle
        .build_facts
        .append_artifact_identity_bytes(&mut preview_identity);
    for value in [
        bundle.build_facts.profile.as_str(),
        bundle.build_facts.stamp.toolchain.as_str(),
        if bundle.build_facts.stamp.dirty { "dirty" } else { "clean" },
        bundle.build_facts.package_name.as_str(),
        bundle.build_facts.package_version.as_str(),
    ] {
        preview_identity.extend_from_slice(&(value.len() as u64).to_le_bytes());
        preview_identity.extend_from_slice(value.as_bytes());
    }
    cx.preview_build_id = format!(
        "sha256-{}",
        jet_foundation::SHA256::sha256_hex(&preview_identity)
    );
    cx.preview_revision = bundle
        .build_facts
        .stamp
        .git
        .clone()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            (!bundle.build_facts.stamp.at.is_empty())
                .then(|| bundle.build_facts.stamp.at.clone())
        })
        .unwrap_or_else(|| {
            format!(
                "{}:{}",
                bundle.build_facts.package_name,
                bundle.build_facts.package_version
            )
        });
    let (inline_foreign_reexport_sigs, inline_foreign_reexport_rets) =
        inline_foreign_reexport_signature_maps(bundle, module_idx);
    cx.inline_foreign_reexport_sigs = inline_foreign_reexport_sigs;
    cx.inline_foreign_reexport_rets = inline_foreign_reexport_rets;
    cx.package_edition = bundle.edition.clone();
    cx.hardware_profile = crate::Sema::target_hardware_profile(bundle);
    cx.hardware_profile_id = crate::Sema::target_hardware_profile_id(bundle);

}

/// Carry authoritative module ownership, imported callable metadata, and
/// package-only guarantee facts into one codegen context. The name ledger owns
/// declarations because loaded dependency items may also be present in a
/// module's merged item list. Dependency ownership uses sema's longest-root rule.
pub(crate) fn populate_cx_module_facts(cx: &mut Cx, bundle: &ProgramBundle, module_idx: usize) {
    cx.direct_c_functions = bundle
        .modules
        .iter()
        .flat_map(|module| {
            module.items.iter().filter_map(|item| {
                let Item::CModule(c_module) = item else {
                    return None;
                };
                Some(
                    c_module
                        .functions
                        .iter()
                        .map(|function| format!("{}::{}", mangle(&module.alias), function.name)),
                )
            })
        })
        .flatten()
        .collect();
    cx.local_type_names
        .retain(|name| bundle.name_ledger.declaration(module_idx, name).is_some());
    register_imported_methods(cx, bundle, module_idx);
    cx.package_hardened = bundle.package_guarantees.harden;
    let Some(module) = bundle.modules.get(module_idx) else {
        cx.dependency_fenced = false;
        return;
    };
    let dependency = bundle
        .dep_roots
        .iter()
        .filter(|(_, root)| module.path.starts_with(root))
        .max_by(|(left_name, left_root), (right_name, right_root)| {
            left_root
                .components()
                .count()
                .cmp(&right_root.components().count())
                .then_with(|| left_name.cmp(right_name))
        })
        .map(|(name, _)| name);
    cx.dependency_fenced = dependency.is_some_and(|name| {
        bundle.package_guarantees.harden || bundle.package_guarantees.contain.contains(name)
    });
}

pub(crate) fn register_bundle_reflect_paths(
    cx: &mut Cx,
    bundle: &ProgramBundle,
    module_idx: usize,
) {
    let paths = bundle.name_ledger.canonical_paths(module_idx);
    for (name, path) in &paths {
        cx.reflect_paths.insert(name.clone(), path.clone());
    }
}

fn register_imported_methods(cx: &mut Cx, bundle: &ProgramBundle, module_idx: usize) {
    let mut imported: Vec<usize> = bundle.modules[module_idx]
        .imports
        .iter()
        .filter_map(|import| {
            bundle
                .name_ledger
                .effective_alias(module_idx, &import.import_alias())?;
            bundle.name_ledger.import_target(module_idx, import.span)
        })
        .collect();
    imported.extend(
        crate::Codegen::Imports::selective_nominal_targets(bundle, module_idx)
            .into_iter()
            .map(|(target, _)| target),
    );
    imported.sort_unstable();
    imported.dedup();
    imported.extend(
        bundle.modules[module_idx]
            .imports
            .iter()
            .flat_map(|import| {
                super::Imports::foreign_list_targets(bundle, module_idx, import)
                    .into_iter()
                    .map(|(_, target)| target)
            }),
    );
    imported.sort_unstable();
    imported.dedup();
    for target in imported {
        let rust_mod = crate::Codegen::mangle(&bundle.modules[target].alias);
        for item in &bundle.modules[target].items {
            let (owner, methods): (&str, Vec<(&Func, Option<&str>, bool, Option<&Type>)>) =
                match item {
                    Item::Struct(definition) => {
                        let mut methods = definition
                            .methods
                            .iter()
                            .map(|method| (method, None, false, None))
                            .collect::<Vec<_>>();
                        methods.extend(definition.trait_impls.iter().flat_map(|implementation| {
                            implementation.methods.iter().map(|method| {
                                (
                                    method,
                                    Some(implementation.trait_name.as_str()),
                                    implementation.compiler_generated,
                                    implementation.operator_rhs.as_ref(),
                                )
                            })
                        }));
                        (&definition.name, methods)
                    }
                    Item::Enum(definition) => {
                        let mut methods = definition
                            .methods
                            .iter()
                            .map(|method| (method, None, false, None))
                            .collect::<Vec<_>>();
                        methods.extend(definition.trait_impls.iter().flat_map(|implementation| {
                            implementation.methods.iter().map(|method| {
                                (
                                    method,
                                    Some(implementation.trait_name.as_str()),
                                    implementation.compiler_generated,
                                    implementation.operator_rhs.as_ref(),
                                )
                            })
                        }));
                        (&definition.name, methods)
                    }
                    Item::Impl(definition) => (
                        &definition.type_name,
                        definition
                            .methods
                            .iter()
                            .map(|method| {
                                (
                                    method,
                                    definition.trait_name.as_deref(),
                                    false,
                                    definition.operator_rhs.as_ref(),
                                )
                            })
                            .collect(),
                    ),
                    _ => continue,
                };
            let owner_identity = bundle
                .name_ledger
                .nominal_identity(target, owner)
                .expect("name ledger must contain every loaded module");
            let owner_visible = bundle.name_ledger.visible(module_idx, target, owner);
            for (method, trait_name, import_trait, operator_rhs) in methods {
                let method_visible = bundle.name_ledger.visible(
                    module_idx,
                    target,
                    &format!("{}{}", owner, method.name),
                );
                if !method_visible && !(trait_name.is_some() && owner_visible) {
                    continue;
                }
                let key = (owner_identity.clone(), method.name.clone());
                if let Some(trait_name) = trait_name {
                    cx.trait_methods.insert(key.clone());
                    cx.trait_method_traits
                        .insert(key.clone(), trait_name.to_string());
                    if import_trait {
                        cx.imported_traits
                            .insert((rust_mod.clone(), trait_name.to_string()));
                    }
                }
                if let Some(self_param) = method.params.iter().find(|p| p.name == Syntax::KW_SELF) {
                    cx.method_self_convs
                        .entry(key.clone())
                        .or_insert(self_param.convention);
                }
                let imported_sig = method_sig_params(method)
                    .into_iter()
                    .map(|(convention, ty)| {
                        (
                            convention,
                            super::Imports::qualify_imported_call_type(bundle, target, "", &ty),
                        )
                    })
                    .collect::<Vec<_>>();
                cx.method_sigs
                    .entry(key.clone())
                    .or_insert_with(|| imported_sig.clone());
                cx.contract_sigs
                    .entry(format!("{}::{}", owner, method.name))
                    .or_insert_with(|| (method.pre.clone(), method.post.clone()));
                cx.fn_param_names
                    .entry(format!("{}::{}", owner, method.name))
                    .or_insert_with(|| {
                        method
                            .params
                            .iter()
                            .map(|param| param.name.clone())
                            .collect()
                    });
                let imported_ret = method.return_type.as_ref().map(|ty| {
                    super::Imports::qualify_imported_call_type(bundle, target, "", ty)
                });
                let imported_operator_rhs = operator_rhs.map(|rhs| {
                    super::Imports::qualify_imported_call_type(bundle, target, "", rhs)
                });
                cx.method_rets
                    .entry(key)
                    .or_insert_with(|| imported_ret.clone());
                if let Some(trait_name) = trait_name {
                    register_operator_method(
                        cx,
                        &owner_identity,
                        &method.name,
                        trait_name,
                        imported_operator_rhs.as_ref(),
                        imported_sig,
                        imported_ret,
                    );
                }
            }
        }
    }
}

fn register_core_close_types(cx: &mut Cx) {
    let imports = |module: &str| cx.core_imports.values().any(|m| m == module);
    if imports("core.files") {
        cx.close_types.extend(
            ["FileReader", "FileWriter", "FileLock"]
                .into_iter()
                .map(str::to_string),
        );
    }
    if imports("core.net") {
        cx.close_types.extend(
            ["TcpStream", "UnixStream", "TLSStream"]
                .into_iter()
                .map(str::to_string),
        );
    }
    if imports(Syntax::CORE_MEM_MODULE) {
        cx.close_types.extend(
            ["Arena", "Bump", "Pool", "Fixed"]
                .into_iter()
                .map(str::to_string),
        );
    }
    if imports("core.db") {
        cx.close_types
            .extend(
                ["DBConnection", "DBScope", "DbPool", "DbLease"]
                    .into_iter()
                    .map(str::to_string),
            );
    }
}

/// Populate value-shape tables that depend on bundle-resolved Core imports.
pub(crate) fn register_core_import_surfaces(cx: &mut Cx) {
    if cx
        .core_imports
        .values()
        .any(|module| module == "core.service")
    {
        let zero = Span::new(0, 0);
        let variants = [
            "Pending",
            "Accepted",
            "Delivering",
            "Delivered",
            "DeadLettered",
            "Cancelled",
        ]
        .into_iter()
        .map(|name| (name.to_string(), VariantPayload::Unit))
        .collect::<Vec<_>>();
        for (variant, _) in &variants {
            cx.variant_owner
                .insert(variant.clone(), "DeliveryState".to_string());
        }
        cx.enum_variants
            .insert("DeliveryState".to_string(), variants);
        for name in ["Delivery", "DeliveryReceipt", "DeliveryEvent"] {
            cx.cloneable.insert(name.to_string());
        }
        let workflow_outcomes = vec![
            ("Finished".to_string(), VariantPayload::Unit),
            (
                "Panicked".to_string(),
                VariantPayload::Single(Type::String, zero),
            ),
            ("Cancelled".to_string(), VariantPayload::Unit),
            ("DeadlineBlown".to_string(), VariantPayload::Unit),
        ];
        for (variant, _) in &workflow_outcomes {
            cx.variant_owner
                .insert(variant.clone(), "TaskOutcome".to_string());
        }
        cx.enum_variants
            .insert("TaskOutcome".to_string(), workflow_outcomes);
        cx.cloneable.insert("TaskOutcome".to_string());
        let workflow_statuses = ["Running", "Paused", "CancelRequested"]
            .into_iter()
            .map(|variant| (variant.to_string(), VariantPayload::Unit))
            .collect::<Vec<_>>();
        for (variant, _) in &workflow_statuses {
            cx.variant_owner
                .insert(variant.clone(), "TaskStatus".to_string());
        }
        cx.enum_variants
            .insert("TaskStatus".to_string(), workflow_statuses);
        cx.cloneable.insert("TaskStatus".to_string());
        let error_variants = [
            "Full",
            "Ambiguous",
            "Unknown",
            "NotStarted",
            "Policy",
            "Unavailable",
            "Partitioned",
            "Revoked",
            "Stale",
            "Expired",
        ]
        .into_iter()
        .map(|name| (name.to_string(), VariantPayload::Single(Type::String, zero)))
        .collect::<Vec<_>>();
        for (variant, _) in &error_variants {
            cx.variant_owner
                .insert(variant.clone(), "ServiceError".to_string());
        }
        cx.enum_variants
            .insert("ServiceError".to_string(), error_variants);
        cx.cloneable.insert("ServiceError".to_string());
    }
    if cx.core_imports.values().any(|module| module == "core.auth") {
        let zero = Span::new(0, 0);
        let field = |name: &str, ty: Type| VariantField {
            name: name.to_string(),
            name_span: zero,
            ty,
            ty_span: zero,
        };
        let mut variants = vec![
            ("InvalidSignature".to_string(), VariantPayload::Unit),
            ("WeakKey".to_string(), VariantPayload::Unit),
            ("TokenExpired".to_string(), VariantPayload::Unit),
        ];
        variants.extend(
            [
                "MalformedToken",
                "UnsupportedToken",
                "MissingClaim",
                "DecodeError",
            ]
            .into_iter()
            .map(|name| (name.to_string(), VariantPayload::Single(Type::String, zero))),
        );
        variants.push((
            "WrongAudience".to_string(),
            VariantPayload::Named(vec![
                field("expected", Type::String),
                field("actual", Type::String),
            ]),
        ));
        variants.push((
            "WrongIssuer".to_string(),
            VariantPayload::Named(vec![
                field("expected", Type::String),
                field("actual", Type::Option(Box::new(Type::String))),
            ]),
        ));
        // Keep the existing discriminants stable; append the new unit
        // variant to the canonical AuthError surface.
        variants.push(("TokenNotYetValid".to_string(), VariantPayload::Unit));
        for (variant, _) in &variants {
            cx.variant_owner
                .insert(variant.clone(), "AuthError".to_string());
        }
        cx.enum_variants.insert("AuthError".to_string(), variants);
        cx.cloneable.insert("AuthError".to_string());
    }
    if cx
        .core_imports
        .values()
        .any(|module| module == "core.net.tls")
    {
        let zero = Span::new(0, 0);
        let versions = vec![
            ("Tls12".to_string(), VariantPayload::Unit),
            ("Tls13".to_string(), VariantPayload::Unit),
        ];
        for (variant, _) in &versions {
            cx.variant_owner
                .insert(variant.clone(), "TLSVersion".to_string());
        }
        cx.enum_variants.insert("TLSVersion".to_string(), versions);
        cx.cloneable.insert("TLSVersion".to_string());
        let roots = Type::Named("TLSRootCertificates".to_string());
        let trust = vec![
            ("System".to_string(), VariantPayload::Unit),
            (
                "SystemPlus".to_string(),
                VariantPayload::Single(roots.clone(), zero),
            ),
            (
                "CustomOnly".to_string(),
                VariantPayload::Single(roots, zero),
            ),
        ];
        for (variant, _) in &trust {
            cx.variant_owner
                .insert(variant.clone(), "TLSClientTrust".to_string());
        }
        cx.enum_variants.insert("TLSClientTrust".to_string(), trust);
        cx.cloneable.insert("TLSClientTrust".to_string());
    }
    // stdlib-api-laws D4 (#2055): `core.watcher` hands back typed `WatchEvent`
    // domain/kind values, so the two closed enums must be registered like the
    // TLS pair above — matching `.File`/`.Created` has to stay on the same
    // typed-IR path as any other covered enum instead of missing the subset.
    if cx
        .core_imports
        .values()
        .any(|module| module == Syntax::WATCHER_MODULE)
    {
        let watch_enums: [(&str, &[&str]); 2] = [
            ("WatchDomain", &["File", "Process", "Port"]),
            (
                "WatchKind",
                &["Created", "Modified", "Removed", "Error", "Exited", "Ready"],
            ),
        ];
        for (name, variants) in watch_enums {
            for variant in variants {
                cx.variant_owner
                    .insert((*variant).to_string(), name.to_string());
            }
            cx.enum_variants.insert(
                name.to_string(),
                variants
                    .iter()
                    .map(|variant| ((*variant).to_string(), VariantPayload::Unit))
                    .collect(),
            );
            cx.cloneable.insert(name.to_string());
        }
    }
    if !cx
        .core_imports
        .values()
        .any(|module| module == Syntax::CORE_EMAIL_MODULE)
    {
        return;
    }
    let zero = Span::new(0, 0);
    for (name, variants) in [
        (
            "SMTPSecurity",
            vec![
                ("StartTls".to_string(), VariantPayload::Unit),
                ("TLS".to_string(), VariantPayload::Unit),
            ],
        ),
        (
            "RecipientPolicy",
            vec![
                ("RequireAll".to_string(), VariantPayload::Unit),
                ("DeliverAccepted".to_string(), VariantPayload::Unit),
            ],
        ),
        (
            "SMTPAuth",
            vec![
                ("None".to_string(), VariantPayload::Unit),
                (
                    "Password".to_string(),
                    VariantPayload::Named(vec![
                        VariantField {
                            name: "username".to_string(),
                            name_span: zero,
                            ty: Type::String,
                            ty_span: zero,
                        },
                        VariantField {
                            name: "password".to_string(),
                            name_span: zero,
                            ty: Type::Named("Secret".to_string()),
                            ty_span: zero,
                        },
                    ]),
                ),
            ],
        ),
        (
            "TLSTrust",
            vec![
                ("System".to_string(), VariantPayload::Unit),
                (
                    "SystemPlusCa".to_string(),
                    VariantPayload::Named(vec![VariantField {
                        name: "pem".to_string(),
                        name_span: zero,
                        ty: Type::List(Box::new(Type::IntN {
                            signed: false,
                            bits: 8,
                        })),
                        ty_span: zero,
                    }]),
                ),
            ],
        ),
    ] {
        for (variant, _) in &variants {
            cx.variant_owner.insert(variant.clone(), name.to_string());
        }
        cx.enum_variants.insert(name.to_string(), variants);
        cx.cloneable.insert(name.to_string());
    }
    let error_fields = || {
        [
            ("operation", Type::String),
            ("server", Type::Option(Box::new(Type::String))),
            ("code", Type::Option(Box::new(Type::Int))),
            ("reason", Type::String),
        ]
        .into_iter()
        .map(|(field, ty)| VariantField {
            name: field.to_string(),
            name_span: zero,
            ty,
            ty_span: zero,
        })
        .collect()
    };
    let errors = [
        "Configuration",
        "DNS",
        "Connect",
        "TLS",
        "Auth",
        "Protocol",
        "Rejected",
        "Transient",
        "TimedOut",
        "Cancelled",
        "DeliveryUnknown",
    ]
    .into_iter()
    .map(|variant| (variant.to_string(), VariantPayload::Named(error_fields())))
    .collect::<Vec<_>>();
    for (variant, _) in &errors {
        cx.variant_owner
            .insert(variant.clone(), "EmailError".to_string());
    }
    cx.enum_variants.insert("EmailError".to_string(), errors);
    cx.cloneable.insert("EmailError".to_string());
    cx.struct_fields.insert(
        "Envelope".to_string(),
        vec![
            ("from".to_string(), Type::Named("Address".to_string())),
            (
                "recipients".to_string(),
                Type::List(Box::new(Type::Named("Address".to_string()))),
            ),
        ],
    );
    cx.struct_fields.insert(
        "RecipientReport".to_string(),
        vec![
            ("address".to_string(), Type::Named("Address".to_string())),
            ("accepted".to_string(), Type::Bool),
            ("code".to_string(), Type::Int),
            ("message".to_string(), Type::String),
        ],
    );
    cx.struct_fields.insert(
        "SendReport".to_string(),
        vec![
            ("server".to_string(), Type::String),
            (
                "accepted".to_string(),
                Type::List(Box::new(Type::Named("RecipientReport".to_string()))),
            ),
            (
                "rejected".to_string(),
                Type::List(Box::new(Type::Named("RecipientReport".to_string()))),
            ),
            ("response_code".to_string(), Type::Int),
            ("response".to_string(), Type::String),
            ("accepted_at".to_string(), Type::String),
        ],
    );
    cx.struct_fields.insert(
        "Limits".to_string(),
        vec![
            ("max_reply_line_bytes".to_string(), Type::Int),
            ("max_reply_lines".to_string(), Type::Int),
            ("max_capabilities".to_string(), Type::Int),
            ("max_recipients".to_string(), Type::Int),
            ("max_message_bytes".to_string(), Type::Int),
            ("max_auth_challenge_bytes".to_string(), Type::Int),
        ],
    );
    cx.struct_fields.insert(
        "SMTPConfig".to_string(),
        vec![
            ("host".to_string(), Type::String),
            ("port".to_string(), Type::Int),
            (
                "security".to_string(),
                Type::Named("SMTPSecurity".to_string()),
            ),
            ("auth".to_string(), Type::Named("SMTPAuth".to_string())),
            (
                "recipient_policy".to_string(),
                Type::Named("RecipientPolicy".to_string()),
            ),
            ("trust".to_string(), Type::Named("TLSTrust".to_string())),
            ("limits".to_string(), Type::Named("Limits".to_string())),
            (
                "dkim".to_string(),
                Type::Option(Box::new(Type::Named("DkimConfig".to_string()))),
            ),
        ],
    );
    cx.struct_fields.insert(
        "DkimConfig".to_string(),
        vec![
            ("domain".to_string(), Type::String),
            ("selector".to_string(), Type::String),
            ("private_key".to_string(), Type::Named("Secret".to_string())),
            (
                "signed_headers".to_string(),
                Type::List(Box::new(Type::String)),
            ),
        ],
    );
    cx.cloneable.extend([
        "Envelope".to_string(),
        "RecipientReport".to_string(),
        "SendReport".to_string(),
        "Limits".to_string(),
    ]);
}

pub(crate) fn memo_facts_for_struct(
    structure: &StructDef,
) -> (HashMap<String, Type>, HashMap<String, HashSet<String>>) {
    let computed: HashSet<String> = structure
        .fields
        .iter()
        .filter(|field| field.computed.is_some())
        .map(|field| field.name.clone())
        .collect();
    let mut direct = HashMap::<String, HashSet<String>>::new();
    for field in &structure.fields {
        let Some(expression) = field.computed.as_deref() else {
            continue;
        };
        let mut expression = expression.clone();
        let mut dependencies = HashSet::new();
        expression.for_each_expr_mut(|node| {
            if let Expr::Field(receiver, name, _) = node {
                if matches!(receiver.as_ref(), Expr::Ident(value, _) if value == Syntax::KW_SELF) {
                    dependencies.insert(name.clone());
                }
            }
        });
        direct.insert(field.name.clone(), dependencies);
    }
    fn depends_on(
        field: &str,
        source: &str,
        direct: &HashMap<String, HashSet<String>>,
        computed: &HashSet<String>,
        visiting: &mut HashSet<String>,
    ) -> bool {
        if !visiting.insert(field.to_string()) {
            return false;
        }
        let result = direct.get(field).is_some_and(|dependencies| {
            dependencies.iter().any(|dependency| {
                dependency == source
                    || computed.contains(dependency)
                        && depends_on(dependency, source, direct, computed, visiting)
            })
        });
        visiting.remove(field);
        result
    }

    let memo_fields = structure
        .fields
        .iter()
        .filter(|field| {
            field.computed.is_some()
                && field
                    .serde_markers
                    .iter()
                    .any(|marker| marker.name == Syntax::MARKER_MEMO)
        })
        .map(|field| (field.name.clone(), field.ty.clone()))
        .collect::<HashMap<_, _>>();
    let mut dependencies = HashMap::<String, HashSet<String>>::new();
    for source in structure
        .fields
        .iter()
        .filter(|field| field.computed.is_none())
        .map(|field| field.name.as_str())
    {
        for memo in memo_fields.keys() {

            if depends_on(memo, source, &direct, &computed, &mut HashSet::new()) {
                dependencies
                    .entry(source.to_string())
                    .or_default()
                    .insert(memo.clone());
            }
        }
    }
    (memo_fields, dependencies)
}

fn receipt_schema_digest(type_name: &str, fields: &[(String, Type)]) -> String {
    let mut schema = String::new();
    schema.push_str(type_name);
    schema.push('\0');
    for (name, ty) in fields {
        schema.push_str(name);
        schema.push(':');
        schema.push_str(&ty.name());
        schema.push(';');
    }
    jet_foundation::SHA256::sha256_hex(schema.as_bytes())
}

pub(crate) fn build_cx_items(
    items: &[Item],
    src: &str,
    file: &str,
    link: Option<&FfiLink>,
    extern_funcs: &HashMap<String, ExternFn>,
    package_edition: &str,
) -> Cx {
    let mut cx = Cx {
        sigs: HashMap::new(),
        contract_sigs: HashMap::new(),
        fn_types: HashMap::new(),
        diverging_functions: HashSet::new(),
        fn_source_types: HashMap::new(),
        fn_bodies: HashMap::new(),
        fn_param_names: HashMap::new(),
        method_sigs: HashMap::new(),
        method_type_params: HashMap::new(),
        method_self_convs: HashMap::new(),
        method_rets: HashMap::new(),
        consts: HashMap::new(),
        persist_types: HashMap::new(),
        const_values: HashMap::new(),
        type_names: HashSet::new(),
        local_type_identities: HashMap::new(),
        local_type_names: HashSet::new(),
        distinct_types: HashMap::new(),
        distinct_ranges: HashMap::new(),
        unit_facts: HashMap::new(),
        unit_labels: HashMap::new(),
        type_aliases: HashMap::new(),
        trait_names: HashSet::new(),
        struct_fields: HashMap::new(),
        reflection_fields: HashMap::new(),
        published_schemas: HashSet::new(),
        codable_types: HashSet::new(),
        reflect_paths: HashMap::new(),
        serde_wire_params: HashMap::new(),
        enum_variants: HashMap::new(),
        variant_owner: HashMap::new(),
        boxed_edges: HashSet::new(),
        cloneable: HashSet::new(),
        migrations: HashMap::new(),
        columnar: HashSet::new(),
        auto_printable: HashSet::new(),
        auto_debug: HashSet::new(),
        auto_equatable: HashSet::new(),
        hashable: HashSet::new(),
        patchable: HashSet::new(),
        computed_fields: HashMap::new(),
        memo_fields: HashMap::new(),
        memo_dependencies: HashMap::new(),
        src: src.to_string(),
        file: file.to_string(),
        module_alias: String::new(),
        module_identity: String::new(),
        core_archive_source: false,
        import_mods: HashMap::new(),
        trait_method_traits: HashMap::new(),
        operator_methods: HashMap::new(),
        direct_c_functions: HashSet::new(),
        opaque_handles: HashSet::new(),
        foreign_types: HashMap::new(),
        reexport_calls: HashMap::new(),
        import_sigs: HashMap::new(),
        import_rets: HashMap::new(),
        core_imports: HashMap::new(),
        used_core: HashSet::new(),
        ffi_callback_fns: HashSet::new(),
        root_prefix: String::new(),
        ffi_crate: link.map(|l| l.crate_name.clone()),
        extern_funcs: extern_funcs.clone(),
        foreign_undos: foreign_undo_map(items),
        code_modules: items
            .iter()
            .filter_map(|item| match item {
                Item::CodeModule(module) if module.body.is_some() => Some(module.name.clone()),
                _ => None,
            })
            .collect(),
        unqualified_inline: HashMap::new(),
        unqualified_file: HashMap::new(),
        inline_unqualified: HashMap::new(),
        inline_unqualified_file: HashMap::new(),
        inline_core_imports: HashMap::new(),
        inline_foreign_imports: HashMap::new(),
        inline_foreign_sigs: HashMap::new(),
        inline_foreign_rets: HashMap::new(),
        inline_foreign_reexport_sigs: HashMap::new(),
        inline_foreign_reexport_rets: HashMap::new(),
        inline_import_names: HashSet::new(),
        inline_reexport_inline: HashMap::new(),
        inline_reexport_core: HashMap::new(),
        receipt_sections: HashMap::new(),
        devtools_package: String::new(),
        devtools_module: String::new(),
        devtools_registry: jet_foundation::AST::DevtoolsRegistry::default(),
        inline_reexport_foreign: HashMap::new(),
        trait_methods: HashSet::new(),
        imported_traits: HashSet::new(),
        rollback_types: HashSet::new(),
        display_types: HashSet::new(),
        debug_types: HashSet::new(),
        close_types: HashSet::new(),
        iterable_hooks: HashMap::new(),
        index_hooks: HashMap::new(),
        current_fn: std::cell::RefCell::new(String::new()),
        policy_declarations: Vec::new(),
        package_hardened: false,
        dependency_fenced: false,
        struct_type_params: HashMap::new(),
        struct_type_param_order: HashMap::new(),
        current_type_params: std::cell::RefCell::new(HashSet::new()),
        jit_spawn_lambdas: std::cell::RefCell::new(Vec::new()),
        jit_spawn_sites: std::cell::RefCell::new(HashMap::new()),
        jit_spawn_site_base: 0,
        jit_method_calls: std::cell::RefCell::new(std::collections::BTreeMap::new()),
        jit_generic_calls: std::cell::RefCell::new(std::collections::BTreeMap::new()),
        jit_canonical_deopt: std::cell::RefCell::new(HashSet::new()),
        jit_canonical_calls: std::cell::RefCell::new(HashSet::new()),
        jit_local_call_prefix: None,
        fn_type_params: HashMap::new(),
        fn_type_param_order: HashMap::new(),
        debug_linemap: false,
        variadic_bound_fns: HashMap::new(),
        active_os: crate::Syntax::OSTarget::host(),
        web_wasm_noncopy_int: false,
        package_edition: package_edition.to_string(),
        preview_build_id: "unbound".to_string(),
        preview_revision: "unbound".to_string(),
        in_stm_transact: std::cell::Cell::new(false),
        stm_touched: std::cell::Cell::new(false),
        hardware_profile: None,
        hardware_profile_id: None,
    };

    let io_context = Type::Named(Syntax::TYPE_IO_CONTEXT.to_string());
    let process_resource_limit = Type::Named(Syntax::TYPE_PROCESS_RESOURCE_LIMIT.to_string());
    // D-TYPE2-TIME1=A: Duration/Instant are the core delta/point of the
    // canonical Time family. They carry unit facts for dimensional lowering,
    // but never enter the generated Float-family trait path.
    let time_dimension = Some(crate::AST::Dimension::base("core.units::Time"));
    for (name, kind) in [
        (Syntax::DURATION_TYPE, crate::AST::QuantityKind::Delta),
        (Syntax::TYPE_INSTANT, crate::AST::QuantityKind::Point),
    ] {
        cx.unit_facts.insert(
            name.to_string(),
            UnitFact {
                family: "Time".to_string(),
                dimension: time_dimension.clone(),
                kind,
                scale: crate::AST::UnitRatio::integer(1),
                offset: crate::AST::UnitRatio::zero(),
                scale_provenance: crate::AST::UnitScaleProvenance::Rational,
            },
        );
    }
    cx.struct_fields.insert(
        Syntax::TYPE_IO_CONTEXT.to_string(),
        vec![
            (
                "operation".to_string(),
                Type::Named(Syntax::TYPE_IO_OPERATION.to_string()),
            ),
            ("resource".to_string(), Type::Option(Box::new(Type::String))),
            ("os_code".to_string(), Type::Option(Box::new(Type::Int))),
            ("cause".to_string(), Type::Option(Box::new(Type::String))),
        ],
    );
    cx.cloneable.insert(Syntax::TYPE_IO_CONTEXT.to_string());
    cx.enum_variants.insert(
        Syntax::TYPE_IO_ERROR.to_string(),
        Syntax::IO_ERROR_VARIANTS
            .iter()
            .map(|name| {
                let payload = if *name == "ResourceLimit" {
                    VariantPayload::Single(process_resource_limit.clone(), Span::new(0, 0))
                } else {
                    VariantPayload::Single(io_context.clone(), Span::new(0, 0))
                };
                ((*name).to_string(), payload)
            })
            .collect(),
    );
    cx.enum_variants.insert(
        Syntax::TYPE_IO_OPERATION.to_string(),
        Syntax::IO_OPERATION_VARIANTS
            .iter()
            .map(|name| ((*name).to_string(), VariantPayload::Unit))
            .collect(),
    );
    cx.enum_variants.insert(
        Syntax::TYPE_PROCESS_RESOURCE_LIMIT.to_string(),
        Syntax::PROCESS_RESOURCE_LIMIT_VARIANTS
            .iter()
            .map(|name| ((*name).to_string(), VariantPayload::Unit))
            .collect(),
    );
    for name in Syntax::IO_ERROR_VARIANTS {
        cx.variant_owner
            .insert((*name).to_string(), Syntax::TYPE_IO_ERROR.to_string());
    }
    for name in Syntax::IO_OPERATION_VARIANTS {
        cx.variant_owner
            .insert((*name).to_string(), Syntax::TYPE_IO_OPERATION.to_string());
    }
    for name in Syntax::PROCESS_RESOURCE_LIMIT_VARIANTS {
        cx.variant_owner.insert(
            (*name).to_string(),
            Syntax::TYPE_PROCESS_RESOURCE_LIMIT.to_string(),
        );
    }
    cx.cloneable.insert(Syntax::TYPE_IO_ERROR.to_string());
    cx.cloneable.insert(Syntax::TYPE_IO_OPERATION.to_string());
    cx.cloneable
        .insert(Syntax::TYPE_PROCESS_RESOURCE_LIMIT.to_string());
    let zero = Span::new(0, 0);
    let http_operations = ["ClientConnect", "ServerBind", "ServeListener"];
    cx.enum_variants.insert(
        "HTTPOperation".to_string(),
        http_operations
            .iter()
            .map(|name| ((*name).to_string(), VariantPayload::Unit))
            .collect(),
    );
    for name in http_operations {
        cx.variant_owner
            .insert(name.to_string(), "HTTPOperation".to_string());
    }
    let http_proxy = vec![
        ("FromEnvironment".to_string(), VariantPayload::Unit),
        ("None".to_string(), VariantPayload::Unit),
        (
            "Url".to_string(),
            VariantPayload::Single(Type::String, zero),
        ),
    ];
    for (variant, _) in &http_proxy {
        cx.variant_owner
            .insert(variant.clone(), "HTTPProxy".to_string());
    }
    cx.enum_variants.insert("HTTPProxy".to_string(), http_proxy);
    cx.cloneable.insert("HTTPProxy".to_string());
    let http_redirect_policy = vec![(
        "Follow".to_string(),
        VariantPayload::Named(vec![
            VariantField {
                name: "max".to_string(),
                name_span: zero,
                ty: Type::Int,
                ty_span: zero,
            },
            VariantField {
                name: "same_origin_credentials".to_string(),
                name_span: zero,
                ty: Type::Bool,
                ty_span: zero,
            },
        ]),
    )];
    for (variant, _) in &http_redirect_policy {
        cx.variant_owner
            .insert(variant.clone(), "HTTPRedirectPolicy".to_string());
    }
    cx.enum_variants
        .insert("HTTPRedirectPolicy".to_string(), http_redirect_policy);
    cx.cloneable.insert("HTTPRedirectPolicy".to_string());
    let http_retry_policy = vec![
        ("None".to_string(), VariantPayload::Unit),
        ("Safe".to_string(), VariantPayload::Unit),
        ("Idempotent".to_string(), VariantPayload::Unit),
    ];
    for (variant, _) in &http_retry_policy {
        cx.variant_owner
            .insert(variant.clone(), "HTTPRetryPolicy".to_string());
    }
    cx.enum_variants
        .insert("HTTPRetryPolicy".to_string(), http_retry_policy);
    cx.cloneable.insert("HTTPRetryPolicy".to_string());
    let http_cookie_jar = vec![("Memory".to_string(), VariantPayload::Unit)];
    cx.variant_owner
        .insert("Memory".to_string(), "HTTPCookieJar".to_string());
    cx.enum_variants
        .insert("HTTPCookieJar".to_string(), http_cookie_jar);
    cx.cloneable.insert("HTTPCookieJar".to_string());
    let http_compress_encoding = vec![("Gzip".to_string(), VariantPayload::Unit)];
    cx.variant_owner
        .insert("Gzip".to_string(), "HTTPCompressEncoding".to_string());
    cx.enum_variants
        .insert("HTTPCompressEncoding".to_string(), http_compress_encoding);
    cx.cloneable.insert("HTTPCompressEncoding".to_string());
    cx.cloneable.insert("HTTPCorsPolicy".to_string());
    // D-HTTP-CORS1=A: `.Any` or `.List([...])` origins.
    let http_cors_origins = vec![
        ("Any".to_string(), VariantPayload::Unit),
        (
            "List".to_string(),
            VariantPayload::Single(Type::List(Box::new(Type::String)), zero),
        ),
    ];
    for (variant, _) in &http_cors_origins {
        cx.variant_owner
            .insert(variant.clone(), "HTTPCorsOrigins".to_string());
    }
    cx.enum_variants
        .insert("HTTPCorsOrigins".to_string(), http_cors_origins);
    cx.cloneable.insert("HTTPCorsOrigins".to_string());
    let mut http_errors = [
        "InvalidMethod",
        "InvalidUrl",
        "InvalidHeader",
        "InvalidStatus",
        "BodyConsumed",
        "InvalidFraming",
        "UnsupportedEncoding",
        "Cancelled",
    ]
    .into_iter()
    .map(|name| (name.to_string(), VariantPayload::Unit))
    .collect::<Vec<_>>();
    for (name, field, ty) in [
        ("BodyTooLarge", "limit", Type::Int),
        ("Resolve", "host", Type::String),
        ("Connect", "address", Type::String),
        ("TLS", "stage", Type::String),
        ("Timeout", "phase", Type::String),
        ("Proxy", "stage", Type::String),
        ("Redirect", "reason", Type::String),
        ("Protocol", "version", Type::String),
        ("IO", "operation", Type::String),
        ("Policy", "reason", Type::String),
        ("ResourceUnavailable", "resource", Type::String),
        ("Internal", "incident_id", Type::String),
        (
            "UnsupportedTarget",
            "operation",
            Type::Named("HTTPOperation".to_string()),
        ),
    ] {
        http_errors.push((
            name.to_string(),
            VariantPayload::Named(vec![VariantField {
                name: field.to_string(),
                name_span: zero,
                ty,
                ty_span: zero,
            }]),
        ));
    }
    for (name, _) in &http_errors {
        cx.variant_owner
            .insert(name.clone(), "HTTPError".to_string());
    }
    cx.enum_variants
        .insert("HTTPError".to_string(), http_errors);
    cx.cloneable.insert("HTTPError".to_string());
    cx.cloneable.insert("HTTPOperation".to_string());
    // D-WS1=B
    let mut ws_errors: Vec<(String, VariantPayload)> = [
        "InvalidUrl",
        "InvalidHandshake",
        "Protocol",
        "Timeout",
        "Closed",
        "Cancelled",
        "UnsupportedTarget",
    ]
    .into_iter()
    .map(|name| (name.to_string(), VariantPayload::Unit))
    .collect();
    ws_errors.push((
        "MessageTooLarge".to_string(),
        VariantPayload::Named(vec![VariantField {
            name: "limit".to_string(),
            name_span: zero,
            ty: Type::Int,
            ty_span: zero,
        }]),
    ));
    ws_errors.push((
        "IO".to_string(),
        VariantPayload::Named(vec![VariantField {
            name: "operation".to_string(),
            name_span: zero,
            ty: Type::String,
            ty_span: zero,
        }]),
    ));
    for (name, _) in &ws_errors {
        cx.variant_owner.insert(name.clone(), "WsError".to_string());
    }
    cx.enum_variants.insert("WsError".to_string(), ws_errors);
    cx.cloneable.insert("WsError".to_string());
    cx.cloneable.insert("WsConn".to_string());
    cx.cloneable.insert("WsMessage".to_string());
    for name in [
        "Browser",
        "BrowserContext",
        "BrowserPage",
        "BrowserFrame",
        "BrowserLocator",
        "BrowserIntercept",
        "BrowserEvent",
        "BrowserTrace",
        "BrowserReceipt",
        "BrowserPrivacy",
        "BrowserError",
        "BrowserAbilities",
        "BrowserProfile",
        "BrowserTimeout",
        "BrowserProtocol",
    ] {
        cx.cloneable.insert(name.to_string());
    }
    // Bare Ordering remains this generated module's nominal; explicit
    // canonical identities select an imported module through foreign_types.
    cx.local_type_names
        .insert(Syntax::TYPE_ORDERING.to_string());
    cx.enum_variants.insert(
        Syntax::TYPE_ORDERING.to_string(),
        ["Less", "Equal", "Greater"]
            .into_iter()
            .map(|name| (name.to_string(), VariantPayload::Unit))
            .collect(),
    );
    for name in ["Less", "Equal", "Greater"] {
        cx.variant_owner
            .insert(name.to_string(), Syntax::TYPE_ORDERING.to_string());
    }
    cx.cloneable.insert(Syntax::TYPE_ORDERING.to_string());
    cx.enum_variants.insert(
        Syntax::TYPE_REMOVE_BY.to_string(),
        ["Val", "Slot"]
            .into_iter()
            .map(|name| (name.to_string(), VariantPayload::Unit))
            .collect(),
    );
    for name in ["Val", "Slot"] {
        cx.variant_owner
            .insert(name.to_string(), Syntax::TYPE_REMOVE_BY.to_string());
    }
    cx.cloneable.insert(Syntax::TYPE_REMOVE_BY.to_string());

    // Inline foreign declarations keep their source return as the bridge ABI.
    // Ordinary Jet functions use the implicit failure carrier instead.
    fn callable_return_type(function: &Func) -> Type {
        function
            .inline_foreign
            .as_ref()
            .and_then(|_| function.return_type.clone())
            .unwrap_or_else(|| {
                if function.inline_foreign.is_some() {
                    Type::Named(Syntax::INTERNAL_UNIT_TYPE.to_string())
                } else {
                    function.effective_return_type()
                }
            })
    }

    for item in items {
        match item {
            Item::Func(f) => {
                if f.diverges {
                    cx.diverging_functions.insert(f.name.clone());
                }
                cx.contract_sigs
                    .insert(f.name.clone(), (f.pre.clone(), f.post.clone()));
                cx.fn_param_names.insert(
                    f.name.clone(),
                    f.params.iter().map(|param| param.name.clone()).collect(),
                );
                cx.fn_type_params.insert(
                    f.name.clone(),
                    f.type_params.iter().map(|param| param.name.clone()).collect(),
                );
                cx.fn_type_param_order.insert(
                    f.name.clone(),
                    f.type_params.iter().map(|param| param.name.clone()).collect(),
                );
                cx.sigs.insert(
                    f.name.clone(),
                    f.params
                        .iter()
                        .map(|p| {
                            let conv = p.convention;
                            let ty = if p.variadic {
                                Type::List(Box::new(p.ty.clone()))
                            } else {
                                p.ty.clone()
                            };
                            (conv, ty.with_effective_fn_returns())
                        })
                        .collect(),
                );
                cx.fn_types.insert(
                    f.name.clone(),
                    Type::Fn {
                        params: f
                            .params
                            .iter()
                            .map(|p| {
                                let ty = if p.variadic {
                                    Type::List(Box::new(p.ty.clone()))
                                } else {
                                    p.ty.clone()
                                };
                                ty.with_effective_fn_returns()
                            })
                            .collect(),
                        ret: Some(Box::new(callable_return_type(f))),
                        effect_bound: None,
                        param_contract: (!f.params.is_empty()).then(|| {
                            f.params
                                .iter()
                                .map(|p| (p.call_label().to_string(), p.zone))
                                .collect()
                        }),
                        call_metadata: Some(crate::AST::FunctionCallMetadata {
                            names: f.params.iter().map(|p| p.name.clone()).collect(),
                            defaults: f
                                .params
                                .iter()
                                .map(|p| p.default.as_deref().cloned())
                                .collect(),
                            variadic: f.params.iter().map(|p| p.variadic).collect(),
                            conventions: f.params.iter().map(|p| p.convention).collect(),
                            policies: f
                                .markers
                                .iter()
                                .find(|marker| marker.name == crate::Syntax::MARKER_POLICY)
                                .and_then(|marker| {
                                    crate::AST::CallablePolicyChain::parse(
                                        &marker.expr_args_owned(),
                                    )
                                    .ok()
                                })
                                .unwrap_or_default(),
                        }),
                        return_view_provenance: f.return_view_provenance.clone(),
                    },
                );
                cx.fn_bodies.insert(f.name.clone(), f.body.clone());
                let source_fn_type = cx.fn_types.get(&f.name).cloned().map(|mut ty| {
                    if let Type::Fn { ret, .. } = &mut ty {
                        *ret = f.return_type.clone().map(Box::new);
                    }
                    ty
                });
                if let Some(source_fn_type) = source_fn_type {
                    cx.fn_source_types
                        .insert(f.name.clone(), source_fn_type);
                }
                // D-ANY-JAI1/D-VARARGBOUND1 (c7jaiany): a trait-bounded variadic
                // (`...Trait` / `...[A, B]`) has no single Rust signature — record
                // it so call-site lowering routes to the per-arity function
                // `Codegen/VariadicBound.rs` synthesizes instead of a normal call.
                if let Some(last) = f.params.last() {
                    if let Some(bounds) =
                        last.variadic_trait_bounds(|n| crate::Generics::is_builtin_trait(n))
                    {
                        cx.variadic_bound_fns
                            .insert(f.name.clone(), (f.params.len() - 1, bounds));
                    }
                }
            }
            Item::Struct(s) => {
                cx.type_names.insert(s.name.clone());
                cx.local_type_names.insert(s.name.clone());
                let encode = s.derives.iter().any(|(name, _)| {
                    matches!(name.as_str(), Syntax::MARKER_CODABLE | Generics::ENCODE)
                });
                let decode = s.derives.iter().any(|(name, _)| {
                    matches!(name.as_str(), Syntax::MARKER_CODABLE | Generics::DECODE)
                });
                if encode && decode {
                    cx.codable_types.insert(s.name.clone());
                }
                if s.is_published_schema {
                    cx.published_schemas.insert(s.name.clone());
                }
                let param_names = s.type_params.iter().map(|p| p.name.as_str()).collect::<HashSet<_>>();
                let mut wire = HashSet::new();
                for field in s.fields.iter().filter(|f| f.computed.is_none()
                    && !f.serde_markers.iter().any(|m| m.name == crate::Syntax::MARKER_SKIP)) {
                    for name in crate::Generics::free_type_params(&field.ty) {
                        if param_names.contains(name.as_str()) { wire.insert(name); }
                    }
                }
                cx.serde_wire_params.insert(s.name.clone(), wire);
                if s.layout == Some(crate::AST::StructLayout::Columnar) {
                    cx.columnar.insert(s.name.clone());
                }
                cx.struct_fields.insert(
                    s.name.clone(),
                    s.reflection_fields()
                        .map(|f| (f.name.clone(), f.ty.clone()))
                        .collect(),
                );
                if let Some(marker) = s
                    .type_markers
                    .iter()
                    .find(|marker| marker.name == crate::Syntax::MARKER_RECEIPT && !marker.negated)
                {
                    if let Some(crate::AST::CtValue::Str(name)) = marker.ct.as_ref() {
                        let fields = s
                            .reflection_fields()
                            .map(|field| (field.name.clone(), field.ty.clone()))
                            .collect::<Vec<_>>();
                        cx.receipt_sections.insert(
                            s.name.clone(),
                            ReceiptSectionFact {
                                name: name.clone(),
                                type_name: s.name.clone(),
                                schema_digest: receipt_schema_digest(&s.name, &fields),
                            },
                        );
                    }
                }
                cx.reflection_fields.insert(
                    s.name.clone(),
                    jet_foundation::Reflection::fields(s),
                );
                // D-FIELDPOL1: computed field names, so TIR lowering routes a
                // read of one to a getter call instead of a member access.
                let computed: HashSet<String> = s
                    .fields
                    .iter()
                    .filter(|f| f.computed.is_some())
                    .map(|f| f.name.clone())
                    .collect();
                if !computed.is_empty() {
                    cx.computed_fields.insert(s.name.clone(), computed);
                }
                let (memo_fields, memo_dependencies) = memo_facts_for_struct(s);
                if !memo_fields.is_empty() {
                    cx.memo_fields.insert(s.name.clone(), memo_fields);
                }
                if !memo_dependencies.is_empty() {
                    cx.memo_dependencies
                        .insert(s.name.clone(), memo_dependencies);
                }
                // c148: record the declared type params so multi-char names are
                // recognized everywhere (struct_is_generic, field_type_cloneable, …).
                cx.struct_type_params.insert(
                    s.name.clone(),
                    s.type_params.iter().map(|p| p.name.clone()).collect(),
                );
                cx.struct_type_param_order.insert(
                    s.name.clone(),
                    s.type_params.iter().map(|p| p.name.clone()).collect(),
                );
            }
            Item::Enum(e) => {
                cx.type_names.insert(e.name.clone());
                cx.local_type_names.insert(e.name.clone());
                cx.enum_variants.insert(
                    e.name.clone(),
                    e.variants
                        .iter()
                        .map(|v| (v.name.clone(), v.payload.clone()))
                        .collect(),
                );
                for v in &e.variants {
                    cx.variant_owner.insert(v.name.clone(), e.name.clone());
                }
                // D-TAG1: group names resolve to their owning enum too, so a
                // group pattern (`.Fire ->`) finds its Rust type prefix.
                for g in &e.groups {
                    cx.variant_owner.insert(g.path.clone(), e.name.clone());
                }
            }
            Item::Const(c) => {
                if c.is_persist {
                    let ty = c.ty.clone().or_else(|| {
                        c.ct.as_ref().map(CtValue::jet_type).or_else(|| match &c.value {
                            Expr::Int(..) => Some(Type::Int),
                            Expr::Bool(..) => Some(Type::Bool),
                            Expr::Char(..) => Some(Type::Char),
                            Expr::Float(_, _, is_f32, _) => {
                                Some(if *is_f32 { Type::Float32 } else { Type::Float })
                            }
                            Expr::Str(..) => Some(Type::String),
                            _ => None,
                        })
                    });
                    if let Some(ty) = ty {
                        cx.persist_types.insert(c.name.clone(), ty);
                    }
                }
                if c.is_comptime && !matches!(c.rust_kind, crate::AST::RustConstKind::Static) {
                    // Inline the evaluated literal at every reference.
                    // `CtValue::serialize()` renders an empty `List([])` as a bare
                    // `vec![]` — fine when the splice site supplies a type (a `let`
                    // binding's annotation, a struct field, a typed param), but a
                    // comptime const inlines directly at a bare use site (e.g.
                    // `PATHS.join(sep)`), where nothing pins the element type and
                    // rustc rejects the untyped `vec![]` as E0282 (I2). `c.ty` (set
                    // by sema alongside `c.ct` — see `ConstDef::ty`) carries the
                    // binding's real Jet type even when the value has no elements to
                    // sample it from, so an empty list renders as a typed
                    // `Vec::<T>::new()` instead.
                    let serialized = match (c.ct.as_ref(), c.ty.as_ref()) {
                        (Some(CtValue::List(xs)), Some(Type::List(inner))) if xs.is_empty() => {
                            format!("Vec::<{}>::new()", cx.rust_type(inner))
                        }
                        (Some(v), _) => v.serialize(),
                        (None, _) => "Default::default()".to_string(),
                    };
                    cx.consts.insert(c.name.clone(), serialized);
                    if let Some(value) = c.ct.as_ref() {
                        cx.const_values.insert(c.name.clone(), value.clone());
                    }
                } else {
                    cx.consts
                        .insert(c.name.clone(), mangle(&c.name).to_uppercase());
                }
            }
            Item::ExternRust(block) => {
                for ef in &block.functions {
                    cx.sigs.insert(
                        ef.name.clone(),
                        ef.params
                            .iter()
                            .map(|p| {
                                let ty = if p.variadic {
                                    Type::List(Box::new(p.ty.clone()))
                                } else {
                                    p.ty.clone()
                                };
                                (p.convention, ty)
                            })
                            .collect(),
                    );
                    // Foreign declarations keep their source return in fn_types:
                    // the bridge wrapper has a raw ABI, not Jet's implicit Result
                    // carrier. #Import(c) functions are ordinary Jet items and differ.
                    cx.fn_types.insert(
                        ef.name.clone(),
                        Type::Fn {
                            params: ef
                                .params
                                .iter()
                                .map(|p| {
                                    if p.variadic {
                                        Type::List(Box::new(p.ty.clone()))
                                    } else {
                                        p.ty.clone()
                                    }
                                })
                                .collect(),
                            ret: ef.return_type.clone().map(Box::new),
                            effect_bound: None, return_view_provenance: None,
                            param_contract: (!ef.params.is_empty()).then(|| {
                                ef.params
                                    .iter()
                                    .map(|p| (p.call_label().to_string(), p.zone))
                                    .collect()
                            }),
                            call_metadata: Some(crate::AST::FunctionCallMetadata {
                                names: ef.params.iter().map(|p| p.name.clone()).collect(),
                                // `extern_to_sig` owns the foreign contract and
                                // deliberately has no Jet default bodies.
                                defaults: ef.params.iter().map(|_| None).collect(),
                                variadic: ef.params.iter().map(|p| p.variadic).collect(),
                                conventions: ef.params.iter().map(|p| p.convention).collect(),
                                policies: crate::AST::CallablePolicyChain::default(),
                            }),
                        },
                    );
                }
            }
            Item::CModule(cm) => {
                // S59: C boundary functions register like extern rust so that
                // cross-module call sites resolve argument conventions; their
                // foreign wrapper retains the declared raw ABI in fn_types.
                for ef in &cm.functions {
                    cx.sigs.insert(
                        ef.name.clone(),
                        ef.params
                            .iter()
                            .map(|p| {
                                let ty = if p.variadic {
                                    Type::List(Box::new(p.ty.clone()))
                                } else {
                                    p.ty.clone()
                                };
                                (p.convention, ty)
                            })
                            .collect(),
                    );
                    cx.fn_types.insert(
                        ef.name.clone(),
                        Type::Fn {
                            params: ef
                                .params
                                .iter()
                                .map(|p| {
                                    if p.variadic {
                                        Type::List(Box::new(p.ty.clone()))
                                    } else {
                                        p.ty.clone()
                                    }
                                })
                                .collect(),
                            ret: ef.return_type.clone().map(Box::new),
                            effect_bound: None, return_view_provenance: None,
                            param_contract: (!ef.params.is_empty()).then(|| {
                                ef.params
                                    .iter()
                                    .map(|p| (p.call_label().to_string(), p.zone))
                                    .collect()
                            }),
                            call_metadata: Some(crate::AST::FunctionCallMetadata {
                                names: ef.params.iter().map(|p| p.name.clone()).collect(),
                                defaults: ef.params.iter().map(|_| None).collect(),
                                variadic: ef.params.iter().map(|p| p.variadic).collect(),
                                conventions: ef.params.iter().map(|p| p.convention).collect(),
                                policies: crate::AST::CallablePolicyChain::default(),
                            }),
                        },
                    );
                }
            }
            Item::Trait(t) => {
                cx.trait_names.insert(t.name.clone());
                for m in &t.methods {
                    if let Some(self_param) = m.params.iter().find(|p| p.name == Syntax::KW_SELF) {
                        cx.method_self_convs
                            .insert((t.name.clone(), m.name.clone()), self_param.convention);
                    }
                    cx.method_sigs.insert(
                        (t.name.clone(), m.name.clone()),
                        m.params
                            .iter()
                            .filter(|p| p.name != Syntax::KW_SELF)
                            .map(|p| {
                                let ty = if p.variadic {
                                    Type::List(Box::new(p.ty.clone()))
                                } else {
                                    p.ty.clone()
                                };
                                (p.convention, ty)
                            })
                            .collect(),
                    );
                    cx.method_rets
                        .insert((t.name.clone(), m.name.clone()), m.return_type.clone());
                }
            }
            // D-QUAL2: a tag erases — it contributes no codegen names.
            Item::Tag(_) => {}
            // D-MIGRATE4: collect migration blocks per type (source order = the
            // chain, oldest step first) so `emit_struct_migration` can emit the
            // runtime step functions + chain-walker for decodable
            // `#PublishedSchema` types. Types without blocks get nothing.
            Item::Migration(m) => {
                cx.migrations
                    .entry(m.type_name.clone())
                    .or_default()
                    .push(m.clone());
            }
            Item::EffectDecl(_)
            | Item::MarkerDecl(_)
            | Item::FactDecl(_)
            | Item::Impl(_) | Item::Test(_) | Item::Module(_) | Item::ErrorConv(_)
            | Item::ProtocolDecl(_) // D-PROTO1/D-PROTO2: erases
            | Item::UserDerive(_) // D-METADERIVE1=A: erase (expanded in sema)
            | Item::TemplateLoop(_) // D-STRUCT-ONCE1=A: expanded before codegen
            | Item::GenericModule(_) // D-CONF-GENSPELL1=A: template — erases
            | Item::ModuleAlias(_) => {} // D-CONF-GENSPELL1=A: alias — erases after expansion
            Item::TypeAlias(a) => {
                cx.type_aliases.insert(
                    a.name.clone(),
                    (a.type_params.clone(), a.target.clone()),
                );
            }
            Item::Distinct(d) => {
                cx.type_names.insert(d.name.clone());
                cx.local_type_names.insert(d.name.clone());
                cx.distinct_types
                    .insert(
                        d.name.clone(),
                        (
                            d.base.clone(),
                            d.derives
                                .iter()
                                .any(|(name, _)| name == crate::Syntax::MARKER_NUMERIC),
                        ),
                    );
                if let Some((lo, hi, _)) = d.range {
                    cx.distinct_ranges.insert(d.name.clone(), (lo, hi));
                }
            }
            // D-QUAL3: each unit-family member registers as a `#Numeric` distinct
            // type erasing to `Float`.
            Item::UnitFamily(uf) => {
                let dimension = uf.resolved_dimension.clone();
                let affine = uf.base.is_some()
                    && uf
                        .members
                        .iter()
                        .any(|member| member.offset != crate::AST::UnitRatio::zero());
                for d in uf.distinct_defs() {
                    cx.type_names.insert(d.name.clone());
                    cx.local_type_names.insert(d.name.clone());
                    cx.distinct_types
                        .insert(
                            d.name.clone(),
                            (
                                d.base.clone(),
                                d.derives
                                    .iter()
                                    .any(|(name, _)| name == crate::Syntax::MARKER_NUMERIC),
                            ),
                        );
                    // Mirror sema `unit_fact`: derive Point/Delta from the type
                    // name when `quantity` is unset (affine families).
                    let kind = d
                        .quantity
                        .as_ref()
                        .map(|(_, kind)| *kind)
                        .or_else(|| {
                            if !affine {
                                return Some(crate::AST::QuantityKind::Linear);
                            }
                            uf.members.iter().find_map(|member| {
                                let stem = crate::AST::UnitFamilyDef::type_name(&member.name);
                                if d.name == format!("{stem}Point") {
                                    Some(crate::AST::QuantityKind::Point)
                                } else if d.name == format!("{stem}Delta") {
                                    Some(crate::AST::QuantityKind::Delta)
                                } else {
                                    None
                                }
                            })
                        })
                        .unwrap_or(crate::AST::QuantityKind::Linear);
                    if let Some(member) = unit_family_member_for_type(uf, &d.name, kind) {
                        cx.unit_labels
                            .insert(d.name.clone(), unit_label(uf, member));
                    }
                    if uf.base.is_some() || dimension.is_some() {
                        if let Some(member) = unit_family_member_for_type(uf, &d.name, kind) {
                            cx.unit_facts.insert(
                                d.name.clone(),
                                unit_fact(uf, member, dimension.clone(), kind),
                            );
                        }
                    }
                }
            }
            Item::CodeModule(cm) => {
                // D-MOD2: register inline module alias and add mangled function sigs.
                if let Some(body) = &cm.body {
                    cx.code_modules.insert(cm.name.clone());
                    for inner in body {
                        if let Item::Func(f) = inner {
                            let mangled = jet_foundation::Names::member_name(&cm.name, &f.name);
                            if f.diverges {
                                cx.diverging_functions.insert(mangled.clone());
                            }
                            cx.contract_sigs
                                .insert(mangled.clone(), (f.pre.clone(), f.post.clone()));
                            cx.fn_type_params.insert(
                                mangled.clone(),
                                f.type_params.iter().map(|param| param.name.clone()).collect(),
                            );
                            cx.fn_type_param_order.insert(
                                mangled.clone(),
                                f.type_params.iter().map(|param| param.name.clone()).collect(),
                            );
                            cx.sigs.insert(
                                mangled.clone(),
                                f.params
                                    .iter()
                                    .map(|p| {
                                        let ty = if p.variadic {
                                            Type::List(Box::new(p.ty.clone()))
                                        } else {
                                            p.ty.clone()
                                        };
                                        (p.convention, ty.with_effective_fn_returns())
                                    })
                                    .collect(),
                            );
                            cx.fn_types.insert(
                                mangled.clone(),
                                Type::Fn {
                                    params: f
                                        .params
                                        .iter()
                                        .map(|p| {
                                            let ty = if p.variadic {
                                                Type::List(Box::new(p.ty.clone()))
                                            } else {
                                                p.ty.clone()
                                            };
                                            ty.with_effective_fn_returns()
                                        })
                                        .collect(),
                                    ret: Some(Box::new(callable_return_type(f))),
                                    effect_bound: None,
                                    return_view_provenance: f.return_view_provenance.clone(),
                                    param_contract: (!f.params.is_empty()).then(|| {
                                        f.params
                                            .iter()
                                            .map(|p| (p.call_label().to_string(), p.zone))
                                            .collect()
                                    }),
                                    call_metadata: Some(crate::AST::FunctionCallMetadata {
                                        names: f.params.iter().map(|p| p.name.clone()).collect(),
                                        defaults: f
                                            .params
                                            .iter()
                                            .map(|p| p.default.as_deref().cloned())
                                            .collect(),
                                        variadic: f.params.iter().map(|p| p.variadic).collect(),
                                        conventions: f.params.iter().map(|p| p.convention).collect(),
                                        policies: crate::AST::CallablePolicyChain::default(),
                                    }),
                                },
                            );
                            cx.fn_param_names.insert(
                                mangled,
                                f.params.iter().map(|param| param.name.clone()).collect(),
                            );
                        }
                    }
                }
            }
        }
    }

    for item in items {
        match item {
            Item::Struct(s) => {
                cx.boxed_edges.extend(find_struct_box_edges(s, &cx));
                if type_is_cloneable_struct(s, &cx.type_names)
                    && !cx.type_contains_secret(&Type::Named(s.name.clone()))
                {
                    cx.cloneable.insert(s.name.clone());
                }
                if crate::Traits::struct_auto_derive_ok(s) {
                    for (trait_name, selected) in [
                        (Generics::PRINTABLE, &mut cx.auto_printable),
                        (Generics::DEBUG, &mut cx.auto_debug),
                        (Generics::EQUATABLE, &mut cx.auto_equatable),
                    ] {
                        if crate::Traits::auto_derive_requested(
                            &s.type_markers,
                            trait_name,
                            s.auto_derive_default,
                        ) && !items.iter().any(|item| match item {
                            Item::Impl(i) => {
                                i.type_name == s.name && i.trait_name.as_deref() == Some(trait_name)
                            }
                            _ => false,
                        }) && !s
                            .trait_impls
                            .iter()
                            .any(|block| block.trait_name == trait_name)
                        {
                            selected.insert(s.name.clone());
                        }
                    }
                }
                for m in &s.methods {
                    register_method(&mut cx, &s.name, m, None, None);
                }
                for implementation in &s.trait_impls {
                    for m in &implementation.methods {
                        register_method(
                            &mut cx,
                            &s.name,
                            m,
                            Some(implementation.trait_name.as_str()),
                            implementation.operator_rhs.as_ref(),
                        );
                    }
                }
                if s.derives.iter().any(|(t, _)| t == Syntax::MARKER_PATCHABLE) {
                    cx.patchable.insert(s.name.clone());
                    let patch = format!("{}.Patch", s.name);
                    let base_ty = Type::Named(s.name.clone());
                    let patch_ty = Type::Named(patch.clone());
                    cx.method_sigs.insert(
                        (s.name.clone(), "apply".to_string()),
                        vec![(AccessConvention::Move, patch_ty.clone())],
                    );
                    cx.method_rets
                        .insert((s.name.clone(), "apply".to_string()), Some(base_ty.clone()));
                    cx.method_sigs.insert(
                        (s.name.clone(), "diff".to_string()),
                        vec![
                            (AccessConvention::Move, base_ty.clone()),
                            (AccessConvention::Move, base_ty),
                        ],
                    );
                    cx.method_rets
                        .insert((s.name.clone(), "diff".to_string()), Some(patch_ty.clone()));
                    cx.method_sigs.insert(
                        (patch.clone(), "merge".to_string()),
                        vec![(AccessConvention::Move, patch_ty.clone())],
                    );
                    cx.method_rets
                        .insert((patch, "merge".to_string()), Some(patch_ty));
                }
            }
            Item::Enum(e) => {
                cx.boxed_edges.extend(find_enum_box_edges(e, &cx));
                if type_is_cloneable_enum(e, &cx.type_names)
                    && !cx.type_contains_secret(&Type::Named(e.name.clone()))
                {
                    cx.cloneable.insert(e.name.clone());
                }
                if crate::Traits::enum_auto_derive_ok(e) {
                    for (trait_name, selected) in [
                        (Generics::PRINTABLE, &mut cx.auto_printable),
                        (Generics::DEBUG, &mut cx.auto_debug),
                        (Generics::EQUATABLE, &mut cx.auto_equatable),
                    ] {
                        if crate::Traits::auto_derive_requested(
                            &e.type_markers,
                            trait_name,
                            e.auto_derive_default,
                        ) && !items.iter().any(|item| match item {
                            Item::Impl(i) => {
                                i.type_name == e.name && i.trait_name.as_deref() == Some(trait_name)
                            }
                            _ => false,
                        }) && !e
                            .trait_impls
                            .iter()
                            .any(|block| block.trait_name == trait_name)
                        {
                            selected.insert(e.name.clone());
                        }
                    }
                }
                for m in &e.methods {
                    register_method(&mut cx, &e.name, m, None, None);
                }
                for implementation in &e.trait_impls {
                    for m in &implementation.methods {
                        register_method(
                            &mut cx,
                            &e.name,
                            m,
                            Some(implementation.trait_name.as_str()),
                            implementation.operator_rhs.as_ref(),
                        );
                    }
                }
            }
            Item::Impl(i) => {
                for m in &i.methods {
                    register_method(
                        &mut cx,
                        &i.type_name,
                        m,
                        i.trait_name.as_deref(),
                        i.operator_rhs.as_ref(),
                    );
                }
            }
            _ => {}
        }
    }

    // D-OPMIX1: Core's typed vector/scalar operator rows use the same
    // complete identity as source operator implementations. The ordinary
    // method tables remain compatibility metadata for non-operator callers;
    // operator lowering reads `operator_methods` exclusively.
    for (owner, trait_name, method, rhs, result) in [
        ("Vec3", Syntax::TRAIT_MUL, "mul", Type::Float, Type::Named("Vec3".to_string())),
        ("Vec3", Syntax::TRAIT_DIV, "div", Type::Float, Type::Named("Vec3".to_string())),
        (
            "Float",
            Syntax::TRAIT_DIV,
            "div",
            Type::Named("Vec3".to_string()),
            Type::Named("Vec3".to_string()),
        ),
    ] {
        let sig = vec![(AccessConvention::Read, rhs.clone())];
        let key = crate::Traits::operator_method_identity(owner, trait_name, method, &rhs);
        cx.operator_methods.insert(
            key,
            OperatorMethod {
                trait_name: trait_name.to_string(),
                sig: sig.clone(),
                ret: Some(result.clone()),
            },
        );
        cx.trait_methods
            .insert((owner.to_string(), method.to_string()));
        cx.method_sigs
            .insert((owner.to_string(), method.to_string()), sig);
        cx.method_rets
            .insert((owner.to_string(), method.to_string()), Some(result));
    }

    // D-TAG1: `hashable` (unlike `comparable`) can't trust "field type is a
    // known type name" — a `Named` field is only Eq+Hash-capable if THAT type
    // is itself hashable (e.g. it has no Float fields). Fixed-point over the
    // (monotonically growing) hashable set until nothing new is added.
    let mut hashable_changed = true;
    while hashable_changed {
        hashable_changed = false;
        for item in items {
            match item {
                Item::Struct(s) if !cx.hashable.contains(&s.name) => {
                    if type_is_hashable_struct(s, &cx.hashable) {
                        cx.hashable.insert(s.name.clone());
                        hashable_changed = true;
                    }
                }
                Item::Enum(e) if !cx.hashable.contains(&e.name) => {
                    if type_is_hashable_enum(e, &cx.hashable) {
                        cx.hashable.insert(e.name.clone());
                        hashable_changed = true;
                    }
                }
                _ => {}
            }
        }
    }

    // D-TXN-ROLLBACK layer 2: collect types that implement `Rollback` so the
    // TIR lowerer can use snapshot_custom instead of snapshot for those roots.
    for item in items {
        match item {
            Item::Impl(i) if i.trait_name.as_deref() == Some(Syntax::TRAIT_ROLLBACK) => {
                cx.rollback_types.insert(i.type_name.clone());
            }
            Item::Impl(i) if i.trait_name.as_deref() == Some(Syntax::TRAIT_DISPLAY) => {
                cx.display_types.insert(i.type_name.clone());
            }
            Item::Impl(i) if i.trait_name.as_deref() == Some(Syntax::TRAIT_DEBUG) => {
                cx.debug_types.insert(i.type_name.clone());
            }
            Item::Impl(i) if i.trait_name.as_deref() == Some(Syntax::TRAIT_CLOSE) => {
                cx.close_types.insert(i.type_name.clone());
            }
            Item::Struct(s) => {
                for block in &s.trait_impls {
                    if block.trait_name == Syntax::TRAIT_ROLLBACK {
                        cx.rollback_types.insert(s.name.clone());
                    }
                    if block.trait_name == Syntax::TRAIT_DISPLAY {
                        cx.display_types.insert(s.name.clone());
                    }
                    if block.trait_name == Syntax::TRAIT_DEBUG {
                        cx.debug_types.insert(s.name.clone());
                    }
                    if block.trait_name == Syntax::TRAIT_CLOSE {
                        cx.close_types.insert(s.name.clone());
                    }
                }
            }
            Item::Enum(e) => {
                for block in &e.trait_impls {
                    if block.trait_name == Syntax::TRAIT_ROLLBACK {
                        cx.rollback_types.insert(e.name.clone());
                    }
                    if block.trait_name == Syntax::TRAIT_DISPLAY {
                        cx.display_types.insert(e.name.clone());
                    }
                    if block.trait_name == Syntax::TRAIT_DEBUG {
                        cx.debug_types.insert(e.name.clone());
                    }
                    if block.trait_name == Syntax::TRAIT_CLOSE {
                        cx.close_types.insert(e.name.clone());
                    }
                }
            }
            _ => {}
        }
    }

    collect_iter_index_hooks(&mut cx, items);
    register_core_event_enums(&mut cx);
    let auto_derives = crate::Traits::TraitRegistry::auto_derives_for_items(items);
    apply_auto_derives(&mut cx, &auto_derives);
    cx
}

pub(crate) fn apply_auto_derives(cx: &mut Cx, auto_derives: &crate::Traits::TraitRegistry) {
    cx.auto_printable = auto_derives.auto_printable.clone();
    cx.auto_debug = auto_derives.auto_debug.clone();
    cx.auto_equatable = auto_derives.auto_equatable.clone();
}

fn register_core_event_enums(cx: &mut Cx) {
    const ENUMS: &[(&str, &[&str])] = &[
        ("Overflow", &["Block", "DropNewest", "DropOldest"]),
        ("FailurePolicy", &["StopFirst", "Collect", "Log", "Ignore"]),
        ("HookPolicy", &["FirstCancelElseTransform"]),
        ("HookOutcome", &["Continue", "Cancel", "Fail"]),
        ("HookDecision", &["Continue", "Transform", "Cancel", "Fail"]),
        (
            "DispatchState",
            &[
                "Delivered",
                "HandlerFailed",
                "DroppedNewest",
                "DroppedOldest",
                "Closed",
                "Cancelled",
                "DeadlineExceeded",
            ],
        ),
    ];
    for (enum_name, variants) in ENUMS {
        cx.enum_variants
            .entry((*enum_name).to_string())
            .or_insert_with(|| {
                variants
                    .iter()
                    .map(|variant| {
                        let payload = match (*enum_name, *variant) {
                            ("HookOutcome", "Continue" | "Fail")
                            | ("HookDecision", "Transform" | "Fail") => {
                                VariantPayload::Single(
                                    Type::Named("Unknown".to_string()),
                                    Span::new(0, 0),
                                )
                            }
                            _ => VariantPayload::Unit,
                        };
                        ((*variant).to_string(), payload)
                    })
                    .collect()
            });
        for variant in *variants {
            // User/imported variants keep their owner. Pattern lowering resolves
            // the subject type first, so a same-named core variant remains exact.
            cx.variant_owner
                .entry((*variant).to_string())
                .or_insert_with(|| (*enum_name).to_string());
        }
    }
    let task_failure = vec![
        ("Cancelled".to_string(), VariantPayload::Unit),
        ("DeadlineBlown".to_string(), VariantPayload::Unit),
        (
            "Panicked".to_string(),
            VariantPayload::Single(Type::String, Span::new(0, 0)),
        ),
    ];
    cx.enum_variants
        .entry(Syntax::TYPE_TASK_FAILURE.to_string())
        .or_insert(task_failure);
    for variant in ["Cancelled", "DeadlineBlown", "Panicked"] {
        cx.variant_owner
            .entry(variant.to_string())
            .or_insert_with(|| Syntax::TYPE_TASK_FAILURE.to_string());
    }
}

fn assoc_type_impl<'a>(assoc: &'a [(String, Span, Type)], name: &str) -> Option<&'a Type> {
    assoc.iter().find(|(n, _, _)| n == name).map(|(_, _, t)| t)
}

fn trait_impl_assoc(
    items: &[Item],
    type_name: &str,
    trait_name: &str,
    assoc_name: &str,
) -> Option<Type> {
    for item in items {
        match item {
            Item::Impl(i)
                if i.type_name == type_name && i.trait_name.as_deref() == Some(trait_name) =>
            {
                return assoc_type_impl(&i.assoc_type_impls, assoc_name).cloned();
            }
            Item::Struct(s) if s.name == type_name => {
                for block in &s.trait_impls {
                    if block.trait_name == trait_name {
                        return assoc_type_impl(&block.assoc_type_impls, assoc_name).cloned();
                    }
                }
            }
            Item::Enum(e) if e.name == type_name => {
                for block in &e.trait_impls {
                    if block.trait_name == trait_name {
                        return assoc_type_impl(&block.assoc_type_impls, assoc_name).cloned();
                    }
                }
            }
            _ => {}
        }
    }
    None
}
/// Return an associated type only when its trait implementation also contains
/// the named method.  Iterable lowering must consume confirmed implementation
/// facts rather than infer backend symbols from a type name.
fn trait_impl_assoc_method(
    items: &[Item],
    type_name: &str,
    trait_name: &str,
    assoc_name: &str,
    method_name: &str,
) -> Option<Type> {
    for item in items {
        let implementation = match item {
            Item::Impl(i)
                if i.type_name == type_name && i.trait_name.as_deref() == Some(trait_name) =>
            {
                Some((&i.assoc_type_impls, &i.methods))
            }
            Item::Struct(s) if s.name == type_name => s
                .trait_impls
                .iter()
                .find(|block| block.trait_name == trait_name)
                .map(|block| (&block.assoc_type_impls, &block.methods)),
            Item::Enum(e) if e.name == type_name => e
                .trait_impls
                .iter()
                .find(|block| block.trait_name == trait_name)
                .map(|block| (&block.assoc_type_impls, &block.methods)),
            _ => None,
        };
        if let Some((assoc_type_impls, methods)) = implementation {
            if methods.iter().any(|method| method.name == method_name) {
                if let Some(assoc_type) = assoc_type_impl(assoc_type_impls, assoc_name) {
                    return Some(assoc_type.clone());
                }
            }
        }
    }
    None
}

pub(crate) fn collect_iterable_hooks(cx: &mut Cx, items: &[Item]) {
    let mut iterable_pairs: Vec<(String, String)> = Vec::new();
    for item in items {
        let coll_type = match item {
            Item::Impl(i) if i.trait_name.as_deref() == Some(Syntax::TRAIT_ITERABLE) => {
                Some(&i.type_name)
            }
            Item::Struct(s) => Some(&s.name),
            Item::Enum(e) => Some(&e.name),
            _ => None,
        };
        let Some(coll_type) = coll_type else {
            continue;
        };
        if let Some(Type::Named(iter_type)) = trait_impl_assoc_method(
            items,
            coll_type,
            Syntax::TRAIT_ITERABLE,
            "Iter",
            "iter",
        ) {
            iterable_pairs.push((coll_type.clone(), iter_type));
        }
    }

    for (coll_type, iter_type) in iterable_pairs {
        let Some(item_type) = trait_impl_assoc_method(
            items,
            &iter_type,
            Syntax::TRAIT_ITERATOR,
            "Item",
            "next",
        ) else {
            continue;
        };
        let coll_identity = crate::Codegen::TIR::canonical_enum_owner(cx, &coll_type);
        let iter_identity = crate::Codegen::TIR::canonical_enum_owner(cx, &iter_type);
        let iter_symbol = format!(
            "{coll_identity}::{}::iter",
            Syntax::TRAIT_ITERABLE
        );
        let next_symbol = format!(
            "{iter_identity}::{}::next",
            Syntax::TRAIT_ITERATOR
        );
        cx.iterable_hooks.insert(
            coll_type,
            IterableHook {
                iter_type,
                item_type,
                iter_symbol,
                next_symbol,
            },
        );
    }
}

fn collect_iter_index_hooks(cx: &mut Cx, items: &[Item]) {
    collect_iterable_hooks(cx, items);

    let mut index_types: HashSet<String> = HashSet::new();
    for item in items {
        match item {
            Item::Impl(i) if i.trait_name.as_deref() == Some(Syntax::TRAIT_INDEX) => {
                index_types.insert(i.type_name.clone());
            }
            Item::Struct(s) => {
                for block in &s.trait_impls {
                    if block.trait_name == Syntax::TRAIT_INDEX {
                        index_types.insert(s.name.clone());
                    }
                }
            }
            Item::Enum(e) => {
                for block in &e.trait_impls {
                    if block.trait_name == Syntax::TRAIT_INDEX {
                        index_types.insert(e.name.clone());
                    }
                }
            }
            _ => {}
        }
    }
    for type_name in index_types {
        let value_type = trait_impl_assoc(items, &type_name, Syntax::TRAIT_INDEX, "Value");
        if let (Some(_key_type), Some(value_type)) = (
            trait_impl_assoc(items, &type_name, Syntax::TRAIT_INDEX, "Key"),
            value_type,
        ) {
            cx.index_hooks
                .insert(type_name.clone(), IndexHook { value_type });
        }
    }
}

/// Register one method surface for sema/codegen call lookup. Nested and
/// top-level trait impls use the same table so lowering cannot lose a method
/// merely because derive generation chose a different AST container.
fn register_operator_method(
    cx: &mut Cx,
    owner: &str,
    method: &str,
    trait_name: &str,
    operator_rhs: Option<&Type>,
    sig: Vec<(AccessConvention, Type)>,
    ret: Option<Type>,
) {
    if !matches!(
        trait_name,
        Syntax::TRAIT_ADD
            | Syntax::TRAIT_SUB
            | Syntax::TRAIT_MUL
            | Syntax::TRAIT_DIV
            | Syntax::TRAIT_EQUATABLE
            | Syntax::TRAIT_COMPARABLE
    ) {
        return;
    }
    let Some(rhs) = operator_rhs else { return };
    let key = crate::Traits::operator_method_identity(owner, trait_name, method, rhs);
    cx.operator_methods.insert(
        key,
        OperatorMethod {
            trait_name: trait_name.to_string(),
            sig,
            ret,
        },
    );
}

fn register_method(
    cx: &mut Cx,
    owner: &str,
    method: &Func,
    trait_name: Option<&str>,
    operator_rhs: Option<&Type>,
) {
    let key = (owner.to_string(), method.name.clone());
    if let Some(self_param) = method
        .params
        .iter()
        .find(|p| p.name == Syntax::KW_SELF)
    {
        cx.method_self_convs
            .insert(key.clone(), self_param.convention);
    }
    let sig = method_sig_params(method);
    cx.method_sigs.insert(key.clone(), sig.clone());
    cx.method_type_params
        .insert(key.clone(), method.type_params.clone());
    cx.method_rets
        .insert(key.clone(), method.return_type.clone());
    cx.contract_sigs.insert(
        format!("{}::{}", owner, method.name),
        (method.pre.clone(), method.post.clone()),
    );
    cx.fn_param_names.insert(
        format!("{}::{}", owner, method.name),
        method
            .params
            .iter()
            .map(|param| param.name.clone())
            .collect(),
    );
    // S62: track trait-impl methods so call sites know not to mangle and retain
    // the checked trait identity for static associated calls.
    if let Some(trait_name) = trait_name {
        cx.trait_methods.insert(key.clone());
        cx.trait_method_traits
            .insert(key, trait_name.to_string());
        register_operator_method(
            cx,
            owner,
            &method.name,
            trait_name,
            operator_rhs,
            sig,
            method.return_type.clone(),
        );
    }
}


fn method_sig_params(f: &Func) -> Vec<(AccessConvention, Type)> {
    f.params
        .iter()
        .filter(|p| p.name != Syntax::KW_SELF)
        .map(|p| {
            let ty = if p.variadic {
                Type::List(Box::new(p.ty.clone()))
            } else {
                p.ty.clone()
            };
            (p.convention, ty)
        })
        .collect()
}

pub(crate) fn type_is_cloneable_struct(s: &StructDef, types: &HashSet<String>) -> bool {
    // c148: pass the struct's declared type-param names so multi-char params are
    // treated as cloneable (they carry a `T: Clone` bound in the emitted impl).
    let param_names: HashSet<String> = s.type_params.iter().map(|p| p.name.clone()).collect();
    s.fields
        .iter()
        .all(|f| field_type_cloneable(&f.ty, types, &param_names))
}

pub(crate) fn type_is_cloneable_enum(e: &EnumDef, types: &HashSet<String>) -> bool {
    // c148: pass the enum's declared type-param names.
    let param_names: HashSet<String> = e.type_params.iter().map(|p| p.name.clone()).collect();
    e.variants.iter().all(|v| match &v.payload {
        VariantPayload::Unit => true,
        VariantPayload::Single(t, _) => field_type_cloneable(t, types, &param_names),
        VariantPayload::Named(fs) => fs
            .iter()
            .all(|f| field_type_cloneable(&f.ty, types, &param_names)),
    })
}

/// Core nominal values are emitted by the Prelude rather than registered in
/// `Cx::type_names`. Most of those carriers derive `Clone`; these are the
/// stateful stream/reader handles whose single-use state intentionally does not.
fn core_type_cloneable(name: &str) -> bool {
    core_rust_type_name(name).is_some()
        && !matches!(
            name,
            "JobQueue" | "Clock"
                | "Match"
                | "DataStream"
                | "JSONReader"
                | "JSONWriter"
                | "JSONLReader"
                | "JSONLWriter"
                | "CSVReader"
                | "CSVWriter"
                | "XMLReader"
                | "XMLWriter"
                | "CBORReader"
                | "CBORWriter"
        )
}

pub(crate) fn field_type_cloneable(
    ty: &Type,
    types: &HashSet<String>,
    param_names: &HashSet<String>,
) -> bool {
    match ty {
        Type::Int | Type::Bool | Type::Float | Type::String | Type::Char => true,
        Type::IntN { .. } | Type::Float32 => true,
        Type::List(inner) | Type::Shared(inner) | Type::Option(inner) => {
            field_type_cloneable(inner, types, param_names)
        }
        Type::Map { key, value, .. } => {
            field_type_cloneable(key, types, param_names)
                && field_type_cloneable(value, types, param_names)
        }
        Type::Result { ok, err } => {
            field_type_cloneable(ok, types, param_names)
                && field_type_cloneable(err, types, param_names)
        }
        // c148: recognize both single-char heuristic and declared multi-char params.
        Type::Named(n) if Generics::is_type_var_name(n) || param_names.contains(n.as_str()) => true,
        // D-SERDE: the dynamic data surface is the Clone-backed DataTree in
        // every canonical spelling, so records containing it can satisfy the
        // generated decoder's existing result-retention path.
        Type::Named(n) if is_json_type_name(n) || n == "Tensor" => true,
        // D-TEST-WORLD1=A: the world is an Arc-backed scoped capability.
        Type::Named(n) if n == Syntax::DETERMINISTIC_WORLD_TYPE => true,
        Type::Named(n) => core_type_cloneable(n) || types.contains(n),
        // `JetTask` implements no `Clone`: a handle owns one join slot.
        // D-PIN1=A: a pin is an exclusive window, so it is no more cloneable
        // than `ViewMut` — duplicating it would hand out a second no-move claim.
        // `JetSharedSnapshot` owns a single-use publication ticket; cloning
        // it would create two winners for one revision.
        Type::Apply { name, .. }
            if name == Syntax::TYPE_SHARED_SNAPSHOT =>
        {
            false
        }
        Type::Apply { name, .. }
            if matches!(
                name.as_str(),
                "ViewMut" | "ComputeViewMut" | "Task" | Syntax::TYPE_PIN
            ) || name == Syntax::TYPE_SHARED_GUARD =>
        {
            false
        }
        Type::Apply { name, .. } if name == "View" => true,
        Type::Apply { args, .. } => args
            .iter()
            .all(|a| field_type_cloneable(a, types, param_names)),
        Type::Tuple(fields) => fields
            .iter()
            .all(|(_, t)| field_type_cloneable(t, types, param_names)),
        Type::TraitObject(_) | Type::Fn { .. } => false,
        Type::FixedList { elem, .. } => field_type_cloneable(elem, types, param_names),
        Type::Tagged {
            marker: crate::AST::TagMarker::Internal(crate::AST::InternalTag::CoreCryptoNominal),
            ..
        } => false,
        Type::Tagged { inner, .. } => field_type_cloneable(inner, types, param_names),
        Type::InlineRange { base, .. } => field_type_cloneable(base, types, param_names),
        Type::Union(members) => members
            .iter()
            .all(|m| field_type_cloneable(m, types, param_names)),
        // Runtime values carry no dimension metadata (I3): cloneable iff the
        // erased base numeric type is.
        Type::Quantity { base, .. } => field_type_cloneable(base, types, param_names),
        // Same as the retired `\0compute.dimension.N` string encoding: it
        // never matched the `Type::Named` user-type-registry lookup above
        // (only ever reached as an `Apply` arg via the fallback above it).
        Type::Measure(_) => false,
    }
}


pub(crate) fn type_is_hashable_struct(s: &StructDef, types: &HashSet<String>) -> bool {
    // D-BOUND-EVOLVE1=A: published records carry an ordered DataTree holder.
    // DataTree intentionally has no Eq/Hash contract (it can contain Float),
    // so the hidden holder must not make the source record claim those Rust
    // storage traits.
    if s.is_published_schema {
        return false;
    }
    let param_names: HashSet<String> = s.type_params.iter().map(|p| p.name.clone()).collect();
    s.fields
        .iter()
        .all(|f| field_type_hashable(&f.ty, types, &param_names))
}

pub(crate) fn type_is_hashable_enum(e: &EnumDef, types: &HashSet<String>) -> bool {
    let param_names: HashSet<String> = e.type_params.iter().map(|p| p.name.clone()).collect();
    e.variants.iter().all(|v| match &v.payload {
        VariantPayload::Unit => true,
        VariantPayload::Single(t, _) => field_type_hashable(t, types, &param_names),
        VariantPayload::Named(fs) => fs
            .iter()
            .all(|f| field_type_hashable(&f.ty, types, &param_names)),
    })
}

/// Same recursive shape as the sema hashability walk, minus `Float`/`Float32`
/// — Rust's `f64`/`f32` don't implement `Eq`/`Hash` (NaN breaks both laws).
pub(crate) fn field_type_hashable(
    ty: &Type,
    types: &HashSet<String>,
    param_names: &HashSet<String>,
) -> bool {
    match ty {
        Type::Float | Type::Float32 => false,
        Type::Int | Type::Bool | Type::String | Type::Char => true,
        Type::IntN { .. } => true,
        Type::Option(inner) => field_type_hashable(inner, types, param_names),
        Type::Result { ok, err } => {
            field_type_hashable(ok, types, param_names)
                && field_type_hashable(err, types, param_names)
        }
        Type::List(inner) => field_type_hashable(inner, types, param_names),
        Type::Named(n) if Generics::is_type_var_name(n) || param_names.contains(n.as_str()) => true,
        Type::Named(n) => types.contains(n),
        // D-TUPLE-DESTRUCT1: same opaque-handle exclusion as the backend
        // equality compatibility walk above.
        Type::Apply { name, .. }
            if matches!(
                name.as_str(),
                "Task" | "Sender" | "Receiver" | Syntax::TYPE_SHARED_GUARD
            ) =>
        {
            false
        }
        // D-MEM1 S6: same `Pool`/`Id` split as the backend equality walk above.
        Type::Apply { name, .. } if name == "Pool" => false,
        Type::Apply { name, .. } if name == "Id" => true,
        Type::Apply { args, .. } => args
            .iter()
            .all(|a| field_type_hashable(a, types, param_names)),
        Type::Tuple(fields) => fields
            .iter()
            .all(|(_, t)| field_type_hashable(t, types, param_names)),
        Type::TraitObject(_) | Type::Map { .. } | Type::Shared(_) | Type::Fn { .. } => false,
        Type::FixedList { elem, .. } => field_type_hashable(elem, types, param_names),
        Type::Tagged { inner, .. } => field_type_hashable(inner, types, param_names),
        Type::InlineRange { base, .. } => field_type_hashable(base, types, param_names),
        Type::Union(members) => members
            .iter()
            .all(|m| field_type_hashable(m, types, param_names)),
        // Runtime values carry no dimension metadata (I3): hashable iff the
        // erased base numeric type is (a `Quantity<Float, _>` is never
        // hashable, same as bare `Float`).
        Type::Quantity { base, .. } => field_type_hashable(base, types, param_names),
        // Same as the retired `\0compute.dimension.N` string encoding: it
        // never matched the `Type::Named` user-type-registry lookup above
        // (only ever reached as an `Apply` arg via the fallback above it).
        Type::Measure(_) => false,
    }
}

/// D-MAP-KEY1: the backend's key walk mirrors sema's ratified eligibility
/// rule. It only supplies the fact needed to emit the shared key adapter;
/// comparison itself remains in `Prelude/Core/MapKey.rs`.

pub(crate) fn find_struct_box_edges(s: &StructDef, cx: &Cx) -> HashSet<(String, String)> {
    let mut boxed = HashSet::new();
    for f in &s.fields {
        walk_type_edge(
            &s.name,
            &f.name,
            &f.ty,
            &mut vec![s.name.clone()],
            cx,
            &mut boxed,
        );
    }
    boxed
}

fn find_enum_box_edges(e: &EnumDef, cx: &Cx) -> HashSet<(String, String)> {
    let mut boxed = HashSet::new();
    for v in &e.variants {
        match &v.payload {
            VariantPayload::Unit => {}
            VariantPayload::Single(t, _) => walk_type_edge(
                &e.name,
                &v.name,
                t,
                &mut vec![e.name.clone()],
                cx,
                &mut boxed,
            ),
            VariantPayload::Named(fs) => {
                for f in fs {
                    let key = format!("{}.{}", v.name, f.name);
                    walk_type_edge(
                        &e.name,
                        &key,
                        &f.ty,
                        &mut vec![e.name.clone()],
                        cx,
                        &mut boxed,
                    );
                }
            }
        }
    }
    boxed
}

fn walk_type_edge(
    owner: &str,
    edge: &str,
    ty: &Type,
    stack: &mut Vec<String>,
    cx: &Cx,
    boxed: &mut HashSet<(String, String)>,
) {
    match ty {
        Type::Named(n) if cx.type_names.contains(n) => {
            if stack.iter().any(|s| s == n) {
                boxed.insert((owner.to_string(), edge.to_string()));
                return;
            }
            stack.push(n.clone());
            if let Some(fields) = cx.struct_fields.get(n) {
                for (fname, fty) in fields {
                    walk_type_edge(n, fname, fty, stack, cx, boxed);
                }
            }
            if let Some(vars) = cx.enum_variants.get(n) {
                for (vname, payload) in vars {
                    match payload {
                        VariantPayload::Unit => {}
                        VariantPayload::Single(t, _) => {
                            walk_type_edge(n, vname, t, stack, cx, boxed);
                        }
                        VariantPayload::Named(fs) => {
                            for f in fs {
                                let key = format!("{}.{}", vname, f.name);
                                walk_type_edge(n, &key, &f.ty, stack, cx, boxed);
                            }
                        }
                    }
                }
            }
            stack.pop();
        }
        // D-SHARED-CYCLE1=C: `Shared<T>` is already a sized Arc handle. Do not
        // invent recursive `Box` edges through Shared — strong Shared cycles are
        // rejected in sema (E0221); expert cycles use `Shared.Weak<T>`.
        Type::Option(inner) | Type::List(inner) => {
            walk_type_edge(owner, edge, inner, stack, cx, boxed);
        }
        Type::Map { key, value, .. }
        | Type::Result {
            ok: key,
            err: value,
        } => {
            walk_type_edge(owner, edge, key, stack, cx, boxed);
            walk_type_edge(owner, edge, value, stack, cx, boxed);
        }
        Type::Char => {}
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raylib_skeleton_types_lower_to_core_prelude_types() {
        assert_eq!(core_rust_type_name("RaylibWindow"), Some("RaylibWindow"));
        assert_eq!(core_rust_type_name("RaylibColor"), Some("RaylibColor"));
        assert_eq!(
            raylib_handle_rust_type("RaylibWindow"),
            Some("RaylibWindow")
        );
        assert_eq!(raylib_handle_rust_type("RaylibColor"), Some("RaylibColor"));
    }
    #[test]
    fn memo_stats_lowers_to_the_generated_root_record() {
        let source = "fn run() {}\n";
        let (tokens, lex_diags) = crate::Lexer::lex(source);
        assert!(lex_diags.is_empty(), "lex errors: {lex_diags:?}");
        let program = crate::Parser::parse(&tokens).expect("parse failed");
        let cx = build_cx(&program, source, "test.jet");
        assert_eq!(
            cx.rust_type(&Type::Named(Syntax::TYPE_MEMO_STATS.to_string())),
            "JetMemoStats"
        );
        assert_eq!(core_rust_type_name(Syntax::TYPE_MEMO_STATS), None);
    }

    #[test]
    fn type_alias_expansion_preserves_function_parameter_contract() {
        let source = "alias Callback<T> :: fn(*, force: T) Int\nfn run() {}\n";
        let (tokens, lex_diags) = crate::Lexer::lex(source);
        assert!(lex_diags.is_empty(), "lex errors: {lex_diags:?}");
        let program = crate::Parser::parse(&tokens).expect("parse failed");
        let cx = build_cx(&program, source, "test.jet");

        let expanded = cx.expand_type_aliases(&Type::Apply {
            name: "Callback".to_string(),
            args: vec![Type::Bool],
        });
        assert_eq!(expanded.name(), "fn(*, force: Bool) Int");
    }
}
