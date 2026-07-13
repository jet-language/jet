use super::{
    AccessConvention, ConstDef, CtValue, ErrorConvDef, Expr, MetaAttr, MigrationDecl, Stmt, Type,
};
use crate::Diagnostics::Span;

#[derive(Debug, Clone)]
pub enum Item {
    Func(Func),
    Struct(StructDef),
    Enum(EnumDef),
    /// D-DIST1 (ratified 2026-06-19): `UserId :: distinct Int` — a distinct type
    /// declaration. `distinct`-over-`distinct` base is rejected in sema.
    Distinct(DistinctDef),
    /// D-TYPEALIAS1 (ratified 2026-06-28): `alias Name<T> = …` — a transparent
    /// type alias for generic shortcuts. Erases at codegen (I3).
    TypeAlias(TypeAliasDef),
    /// D-QUAL3 (ratified 2026-06-24): `#UnitFamily(currency) { usd, eur, gbp }` —
    /// a unit family. Sugar: each member mints one `@Numeric` distinct type
    /// (`usd` → `Usd`) erasing to `Float`. Lowers to a `DistinctDef` per member
    /// in sema registration and codegen — it rides the D-DIST1/D-DIST3 machinery.
    UnitFamily(UnitFamilyDef),
    /// S28 (M9): `trait Name { fn sig(self) -> T; … }`.
    Trait(TraitDef),
    /// D-QUAL2 (ratified 2026-06-21): `tag Name;` or `tag Name { }` — a marker
    /// qualifier with no methods. It erases at runtime (codegen emits nothing).
    /// A method in a tag body is E0732; using a tag where dispatch is expected
    /// is E0731.
    Tag(TagDef),
    Impl(ImplDef),
    Const(ConstDef),
    /// S43 (M6): `#Test "name" { … }` — only at file top level.
    Test(TestDef),
    /// D-BENCH1/D-BENCH-MARKER1=A: `#Bench("name") { … }` — a region
    /// benchmark, the exact sibling of `#Test`. Run by `jet bench`.
    Bench(BenchDef),
    /// S50 (M7): `extern rust "crate@version" { … }`.
    ExternRust(ExternRustBlock),
    /// U3 (unified-ecosystem §4): `module name { … }` — a named, composable
    /// declaration contributing typed values to reserved namespaces.
    Module(ModuleDecl),
    /// S59 (E2-M14): `#Extern module c.<lib> { … }` (user overlay) or
    /// `#Bindgen module c.<lib>.__bindgen__ { … }` (compiler-generated cache).
    CModule(CModule),
    /// D-MOD1/2 (code module system): `module name;` (file declaration) or
    /// `module name { … }` (inline body). `body = None` means the items live in
    /// a separate file found by the loader. NOT a JetOS module (see `ModuleDecl`).
    CodeModule(CodeModule),
    /// D-ERR-CONV (ratified 2026-06-19): `impl Source -> Target { … }` — typed
    /// error conversion; `?` applies it when propagating Source into a Target context.
    ErrorConv(ErrorConvDef),
    /// D-MIGRATE1 (ratified 2026-06-22): `migration TypeName { rename a -> b }`
    /// block — declares field renames on a `@PublishedSchema` struct.
    Migration(MigrationDecl),
    /// D-STATE-DECL (ratified 2026-06-25, option B): `state TypeName { A, B, C }` —
    /// declares the bounded set of states for a typestate machine. The set erases at
    /// runtime (pure compile-time, no discriminant). Each name in the body is a state
    /// label; `#State(S)` / `#Transition(From -> To)` markers on `TypeName::*` methods
    /// must reference names from this set (unknown state = E0151). A declared state with
    /// no outgoing `#Transition` is a dead-end warning (L0151). Declaration family sibling
    /// of `tag`/`struct`/`enum`.
    StateDecl(StateDecl),
    /// D-PROTO1 / D-PROTO2 (ratified 2026-06-27): `protocol Name { client -> server:
    /// Msg(…) }` — declares an ordered request/response exchange and expands (R11) into
    /// `#SingleUse` `.Client`/`.Server` handle types with typestate-checked send/recv
    /// methods. Erases as generated items; the declaration itself never reaches codegen.
    ProtocolDecl(ProtocolDecl),
    /// D-METADERIVE1=A: `derive T.Trait { … }` user-authored derive.
    UserDerive(DeriveDef),
    /// D-GENMOD2=A: `module Name<params> { … }` — a parameterized module template.
    /// Stores the body as-is; sema expands `ModuleAlias` references before codegen.
    GenericModule(GenericModuleDef),
    /// D-GENMOD2=A: `module Alias = Module<args>` — module instantiation alias.
    /// Expanded to a `CodeModule` by sema before registration and body-checking.
    ModuleAlias(ModuleAliasDef),
}

/// D-MOD1/2: code module — `module math;` or `module math { pub fn … }`.
#[derive(Debug, Clone)]
pub struct CodeModule {
    pub name: String,
    pub name_span: Span,
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) module …`.
    pub is_package_pub: bool,
    /// None = file declaration (`module math;`), Some = inline body.
    pub body: Option<Vec<Item>>,
    /// D-WASM1: `module name js { … }` / `module name wasm { … }` ceiling override.
    pub web_target: Option<crate::WebPartition::WebBucket>,
    /// #91: applicative generic-module identity. Ordinary code modules carry
    /// `None`; instantiated modules carry the collision-checkable full key and
    /// its stable content fingerprint through lowering and tooling.
    pub instance_identity: Option<ModuleInstanceIdentity>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleInstanceIdentity {
    pub full_key: Vec<u8>,
    pub fingerprint: String,
    pub definition_id: String,
    pub argument_keys: Vec<Vec<u8>>,
    pub template_span: Span,
    pub applications: Vec<ModuleInstanceApplication>,
}

/// One source spelling that applies a generic-module instance. Identity and
/// source ownership travel with the span so tooling never has to recover
/// either from an alias name (which is neither unique nor semantic).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleInstanceApplication {
    pub name: String,
    pub source_module: String,
    pub semantic_identity: String,
    pub span: Span,
}

/// D-GENMOD2=A: one parameter of a generic module — `module Lru<K: Hash, capacity: Int>`.
/// Sema resolves annotated slot kind in the template definition scope. Casing
/// never decides semantics (D-GENMOD-VALUE1).
#[derive(Debug, Clone)]
pub enum GenericModuleParam {
    /// `K` — an unbounded type parameter.
    Bare {
        name: String,
        name_span: Span,
    },
    /// `K: Hash` or `capacity: Int`; sema resolves Hash as a bound and Int as
    /// a concrete Tier-0 value type.
    Annotated {
        name: String,
        name_span: Span,
        annotation: Type,
    },
}

impl GenericModuleParam {
    pub fn name(&self) -> &str {
        match self {
            GenericModuleParam::Bare { name, .. }
            | GenericModuleParam::Annotated { name, .. } => name.as_str(),
        }
    }

    pub fn name_span(&self) -> Span {
        match self {
            GenericModuleParam::Bare { name_span, .. }
            | GenericModuleParam::Annotated { name_span, .. } => *name_span,
        }
    }
}

/// D-GENMOD2=A: one argument in `module Alias = Module<String, 32>`.
#[derive(Debug, Clone)]
pub enum ModuleArg {
    /// A type argument: `String`, `Int`, `MyType`.
    Type(Type, Span),
    /// A value argument: integer/bool literal or other comptime expression.
    Value(Expr, Span),
}

/// D-GENMOD2=A: `module Name<params> { body }` — a parameterized module template.
/// Stores the body as a template. Sema expands `ModuleAlias` referencing this into
/// a `CodeModule` before the main checking pass. Never reaches codegen directly.
#[derive(Debug, Clone)]
pub struct GenericModuleDef {
    pub name: String,
    pub name_span: Span,
    pub is_pub: bool,
    pub is_package_pub: bool,
    pub params: Vec<GenericModuleParam>,
    pub body: Vec<Item>,
    pub span: Span,
}

/// D-GENMOD2=A: `module Alias = Module<args>` — module instantiation alias.
/// Expanded to a `CodeModule` by sema before registration and codegen.
#[derive(Debug, Clone)]
pub struct ModuleAliasDef {
    pub name: String,
    pub name_span: Span,
    pub is_pub: bool,
    pub is_package_pub: bool,
    pub target: String,
    pub target_span: Span,
    pub args: Vec<ModuleArg>,
    pub span: Span,
}

/// S59 (E2-M14): which attribute introduced a C FFI module — the user-written
/// overlay (`#Extern`) or the generated cache surface (`#Bindgen`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CModuleKind {
    /// `#Extern module c.<lib> { … }` — user overlay, allowed anywhere.
    Extern,
    /// `#Bindgen module c.<lib>.__bindgen__ { … }` — generated, cache files only.
    Bindgen,
}

/// S59 (E2-M14): one `#Extern`/`#Bindgen module c.<lib>[.__bindgen__] { … }` block.
#[derive(Debug, Clone)]
pub struct CModule {
    pub kind: CModuleKind,
    /// The library link key — last `c.<lib>` segment (e.g. `raylib`).
    pub lib: String,
    /// Span of the whole dotted module path (`c.raylib` / `c.raylib.__bindgen__`).
    pub path_span: Span,
    /// Foreign functions declared in the body (same shape as `extern rust`).
    pub functions: Vec<ExternFn>,
    pub span: Span,
}

/// U3 (unified-ecosystem §4): `module name { contributions… }`. Many modules
/// may share a file; a leading-`_` name disables one (not discovered/merged).
#[derive(Debug, Clone)]
pub struct ModuleDecl {
    pub name: String,
    pub name_span: Span,
    /// True when `name` begins with `_` (U3 one-character disable).
    pub disabled: bool,
    /// U8 (unified-ecosystem §2.2): named `sources:` declared inside the module
    /// body, siblings of the contributions. Merged by key across modules (U5).
    pub sources: Vec<SourceDecl>,
    /// U8: `imports: find("./modules")` import-tree directives, parsed as
    /// ordinary call expressions; the `find` walk lands with U4 discovery.
    pub imports: Vec<Expr>,
    /// D-WORKSPACE1=B: `members: <expr>` in `module workspace { … }` — the
    /// comptime expression that yields the list of member package paths. Only
    /// meaningful in `workspace.jet`; ignored in other module files.
    pub members: Vec<Expr>,
    pub contributions: Vec<Contribution>,
    pub span: Span,
}

/// U8 (unified-ecosystem §2.2): one `name: provider@target` entry in a module's
/// `sources:` block, e.g. `default: github@NixOS/nixpkgs/nixos-24.05`. The ref
/// is not a single token (it contains `@`, `/`, `-`, `.`), so the parser records
/// its source span; modeval slices the source and validates it via
/// `classify_provider_ref`.
#[derive(Debug, Clone)]
pub struct SourceDecl {
    pub name: String,
    pub name_span: Span,
    /// Span of the raw `provider@target` ref text in the source.
    pub ref_span: Span,
    pub span: Span,
}

/// U3 (unified-ecosystem §5): one typed namespace contribution inside a module,
/// e.g. `env.dev: Env { … }`. The value reuses the struct-literal expression
/// parser; the namespace and path locate it in the merged whole.
#[derive(Debug, Clone)]
pub struct Contribution {
    pub namespace: Namespace,
    pub path: String,
    pub path_span: Span,
    pub value: ContribValue,
    pub span: Span,
}

/// U11/U12/U14/U18: the value of a typed contribution. `env.<name>:` reuses the
/// ordinary expression parser (a struct literal), while `system.<name>:` and
/// `image.<name>:` parse into dedicated typed literals so the U13 `options` list
/// (`network.hostName: laptop`), the U13 typed `target` value (`linux.x64`), the U12
/// `Service` map, and U18 bare-`{ … }` records all have a home — none of which fit
/// the ordinary expression grammar.
#[derive(Debug, Clone)]
pub enum ContribValue {
    /// `env.<name>:` — any expression, typically `Env { … }` (or a bare `{ … }`,
    /// U18). modeval field-checks it. Only the legacy contribution form
    /// (`module dev { env.dev: Env.{ … } }`, teaches E1229) still produces
    /// this; it has no dev `services:` support (U12) — see `Env` below.
    Expr(Expr),
    /// `env.<name>:` in the canonical role-module form (`module env.<name> {
    /// … }`, D-JPK-MODBODY1=A) — bare `field: expr` pairs plus, distinct from
    /// jetos `system.*.services` (U11), a dev-supervised `services: { … }`
    /// map (U12), reusing the exact same `services_map()` grammar as
    /// `System`.
    Env(EnvLit),
    /// `system.<name>:` — a `System` record (U11).
    System(SystemLit),
    /// `image.<name>:` — an `Image` record (U14).
    Image(ImageLit),
    /// `fleet.<name>:` — a `Fleet` record (U15).
    Fleet(FleetLit),
    /// `vmtest.<name>:` — a VM scenario record (D-JOS-VMTEST1).
    VmTest(VmTestLit),
    /// D-PERFBUDGET-GRAMMAR1=A: `module perf.<role> { budgets: [...] }`.
    Perf(PerfLit),
}

impl ContribValue {
    pub fn span(&self) -> Span {
        match self {
            ContribValue::Expr(e) => e.span(),
            ContribValue::Env(e) => e.span,
            ContribValue::System(s) => s.span,
            ContribValue::Image(i) => i.span,
            ContribValue::Fleet(f) => f.span,
            ContribValue::VmTest(v) => v.span,
            ContribValue::Perf(p) => p.span,
        }
    }
}

/// One performance-policy role. The parser owns the declaration boundary and
/// exact source spans; sema elaborates the captured list into BudgetSpec facts.
#[derive(Debug, Clone)]
pub struct PerfLit {
    /// Typed declarations captured at the configuration boundary. Field values
    /// remain ordinary Jet expressions so unit/enum/record nodes keep their
    /// canonical AST identity and exact spans.
    pub budgets: Vec<BudgetDecl>,
    pub budgets_span: Span,
    pub list_span: Span,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct BudgetDecl {
    pub fields: Vec<BudgetField>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct BudgetField {
    pub name: String,
    pub name_span: Span,
    pub value: Expr,
    pub span: Span,
}

/// U12/D-JPK-MODBODY1=A: an `env.<name>: { … }` role-module body — bare
/// `field: expr` settings (packages/prompt/…, modeval field-checks each) plus
/// a dev-supervised `services: { name: { … }, … }` map, captured with the
/// same `ServiceEntry` grammar `System.services` uses (U12's `Service` stays
/// one open record either way — only the downstream capture/interpretation
/// differs between the jetos and dev planes, never the grammar).
#[derive(Debug, Clone)]
pub struct EnvLit {
    pub fields: Vec<(String, Span, Expr)>,
    pub services: Vec<ServiceEntry>,
    pub span: Span,
}

/// U11/U18: a `System { target, packages, services, options }` record. The
/// outer type name is optional (U18 inferred constructor): `explicit_type` is
/// `Some(span)` when the author wrote `System { … }`, `None` for a bare `{ … }`.
/// Field-checking (which fields are known, that `target` is a known platform, etc.)
/// lives in modeval, not the parser.
#[derive(Debug, Clone)]
pub struct SystemLit {
    pub explicit_type: Option<Span>,
    pub fields: Vec<SystemField>,
    pub span: Span,
}

/// One `name: value` field inside a `System { … }` record. The value's shape
/// depends on the field; modeval validates it against U11.
#[derive(Debug, Clone)]
pub struct SystemField {
    pub name: String,
    pub name_span: Span,
    pub value: SystemFieldValue,
    pub span: Span,
}

/// The parsed value of one `System` field (U11/U12/U13).
#[derive(Debug, Clone)]
pub enum SystemFieldValue {
    /// `target: linux.x64` — a dotted typed platform value (U13). Stores the two
    /// dotted segments (`os`, `arch`) and the whole value's span.
    Platform {
        os: String,
        arch: String,
        span: Span,
    },
    /// `packages: [ … ]` — a `ListLit` whose Pkg sugar modeval slices from source.
    Packages(Expr),
    /// `services: { name: { … }, … }` — a keyed map of bare `Service` records (U12).
    Services(Vec<ServiceEntry>),
    /// `options: [ network.hostName: laptop, … ]` — an ordered list of dotted-key /
    /// value entries (U13).
    Options(Vec<OptionEntry>),
    /// Any other field — captured as an expression so modeval can report it as an
    /// unknown `System` field with a span.
    Other(Expr),
}

/// U12: one `name: { … }` entry in a `services:` map. The record is an inferred
/// `Service` (U18); `explicit_type` is `Some(span)` if the author wrote
/// `Service { … }`. Fields are arbitrary (open record); modeval requires `enable`.
#[derive(Debug, Clone)]
pub struct ServiceEntry {
    pub name: String,
    pub name_span: Span,
    pub explicit_type: Option<Span>,
    pub fields: Vec<(String, Span, Expr)>,
    pub span: Span,
}

/// U13: one `dotted.key: value` entry in an `options:` list. `key` is the dotted
/// path text (`network.hostName`); `value` is any expression (bare identifier, dotted
/// typed value, list, or quoted free-form string).
#[derive(Debug, Clone)]
pub struct OptionEntry {
    pub key: String,
    pub key_span: Span,
    pub value: Expr,
    /// The full source span of the written value (`default.fish`), recorded
    /// directly so modeval can slice the typed value text without depending on
    /// each `Expr` variant's span covering its whole written form.
    pub value_span: Span,
    pub span: Span,
}

/// U14/U18: an `Image { from: system.<name>, format: iso }` record. `explicit_type`
/// mirrors `SystemLit`. `from`/`format`/`target` and any stray field are captured;
/// modeval validates them (U14: `from` required and references a known `System`;
/// `format` ∈ {iso, qcow, raw}; only `target:` may be restated, for cross-compile).
#[derive(Debug, Clone)]
pub struct ImageLit {
    pub explicit_type: Option<Span>,
    pub fields: Vec<ImageField>,
    pub span: Span,
}

/// One `name: value` field inside an `Image { … }` record.
#[derive(Debug, Clone)]
pub struct ImageField {
    pub name: String,
    pub name_span: Span,
    pub value: ImageFieldValue,
    pub span: Span,
}

/// D-JPK-IMAGE1 (=A, ratified 2026-07-01): what an `Image`'s `from:` names —
/// a `System` (the `.Iso`/`.Qcow`/`.Raw` disk-image tier, U14 original shape) or
/// a `Package` (the `.Oci` container tier, same card). One `from:` keyword,
/// two referent namespaces; `kind:` (explicit or inferred from which one is
/// written) picks the interpretation.
#[derive(Debug, Clone)]
pub enum ImageFromRef {
    System(String),
    Package(String),
}

/// The parsed value of one `Image` field (U14/D-JPK-IMAGE1).
#[derive(Debug, Clone)]
pub enum ImageFieldValue {
    /// `from: system.<name>` or `from: packages.<name>` — stores which one and
    /// the whole value span.
    From { source: ImageFromRef, span: Span },
    /// `format: iso` — a bare format keyword. Stores the word and its span.
    Format { word: String, span: Span },
    /// `target: linux.x64` — an explicit cross-compile platform (U14).
    Platform {
        os: String,
        arch: String,
        span: Span,
    },
    /// Any other field (`kind`/`expose`/`env_vars`/`files`/`base`, D-JPK-IMAGE1,
    /// or a genuinely unknown one) — captured as a raw `Expr` so modeval can
    /// dispatch on the field name (`Comptime::evaluate` for the OCI fields) or
    /// reject a restated/unknown one.
    Other(Expr),
}

/// U15: a `Fleet { hosts: { <host>: system.<name>.{ … } } }` record. Mirrors
/// `SystemLit`/`ImageLit`. Field-checking (the one field is `hosts`; every host
/// value references a known `System`) lives in modeval.
#[derive(Debug, Clone)]
pub struct FleetLit {
    pub explicit_type: Option<Span>,
    pub fields: Vec<FleetField>,
    pub span: Span,
}

/// One `name: value` field inside a `Fleet { … }` record.
#[derive(Debug, Clone)]
pub struct FleetField {
    pub name: String,
    pub name_span: Span,
    pub value: FleetFieldValue,
    pub span: Span,
}

/// The parsed value of one `Fleet` field (U15).
#[derive(Debug, Clone)]
pub enum FleetFieldValue {
    /// `hosts: { web1: system.<name>.{ … }, … }` — a keyed map of host
    /// definitions, each referencing a `System` with optional copy-with-update
    /// overrides captured as raw source text.
    Hosts(Vec<HostEntry>),
    /// Any other field — captured so modeval can report it as an unknown
    /// `Fleet` field with a span.
    Other(Expr),
}

/// U15: one `<host>: system.<name>.{ overrides }` entry in a `hosts:` map.
/// `overrides` is the raw source text of the `.{ … }` copy-with-update tail
/// (captured, not semantically parsed, until fleet realization in Phase D).
#[derive(Debug, Clone)]
pub struct HostEntry {
    pub name: String,
    pub name_span: Span,
    /// The referenced `System`'s role name (the `<name>` in `system.<name>`).
    pub system: String,
    pub system_span: Span,
    /// Source span of the `.{ … }` copy-with-update override record, if written
    /// (`None` for a bare ref). Sliced by modeval; not semantically parsed until
    /// fleet realization (Phase D).
    pub overrides: Option<Span>,
    pub span: Span,
}

/// D-JOS-VMTEST1: a `VmTest { hosts, run }` record. The `hosts:` map uses the
/// same host-to-system reference grammar as `Fleet`; `run:` captures the typed
/// test body span so the jetos VM-test runner can validate/replay proof facts.
#[derive(Debug, Clone)]
pub struct VmTestLit {
    pub explicit_type: Option<Span>,
    pub fields: Vec<VmTestField>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct VmTestField {
    pub name: String,
    pub name_span: Span,
    pub value: VmTestFieldValue,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum VmTestFieldValue {
    Hosts(Vec<HostEntry>),
    Run { span: Span },
    Other(Expr),
}

/// U3 (unified-ecosystem §5): the reserved namespaces a module may contribute
/// to, each with a matching type (`Env`/`System`/`Image`/`Fleet`/`VmTest`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Namespace {
    /// `env` → `Env`: a development environment / shell.
    Env,
    /// `system` → `System`: a whole machine (jetos).
    System,
    /// `image` → `Image`: an ISO / VM / disk image (jetos).
    Image,
    /// `fleet` → `Fleet`: a map of hosts to `System` refs (U15).
    Fleet,
    /// `vmtest` → `VmTest`: a VM scenario over `System` refs (D-JOS-VMTEST1).
    VmTest,
    /// `perf` → typed performance-policy declarations.
    Perf,
}

/// S45 (M9): type parameter with optional trait bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeParam {
    pub name: String,
    pub name_span: Span,
    pub bounds: Vec<String>,
}

/// S28 (M9): trait declaration — signatures only in v1.
#[derive(Debug, Clone)]
pub struct TraitDef {
    pub span: Span,
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) trait …`.
    pub is_package_pub: bool,
    pub name: String,
    pub name_span: Span,
    /// D-LIB2: `type Name;` associated type declarations inside the trait body.
    pub assoc_types: Vec<(String, Span)>,
    pub methods: Vec<TraitMethodSig>,
}

/// D-QUAL2: `tag Name;` / `tag Name { }` — a marker qualifier. By the taxonomy
/// rule (methods → trait, no methods → tag) a tag carries no methods; any method
/// found in its body is reported as E0732. The body is parsed permissively (so a
/// stray method doesn't derail the parser) and validated in sema.
#[derive(Debug, Clone)]
pub struct TagDef {
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) tag …`.
    pub is_package_pub: bool,
    pub name: String,
    pub name_span: Span,
    /// Methods erroneously written in a tag body. Always empty for a well-formed
    /// tag; each entry triggers E0732 in sema.
    pub methods: Vec<TraitMethodSig>,
    pub span: Span,
}

/// D-PROTO1 / D-PROTO2: one message line inside a `protocol` block —
/// `client -> server: Hello(version: Int)`.
#[derive(Debug, Clone)]
pub enum ProtocolDirection {
    ClientToServer,
    ServerToClient,
}

/// D-PROTO1 / D-PROTO2: one ordered message in a protocol declaration.
#[derive(Debug, Clone)]
pub struct ProtocolMessage {
    pub direction: ProtocolDirection,
    pub name: String,
    pub name_span: Span,
    pub fields: Vec<(String, Type)>,
    pub span: Span,
}

/// D-PROTO1 / D-PROTO2: `protocol Name { … }` — the user-facing session-type
/// declaration. Expanded in sema into generated `#SingleUse` + typestate items (R11).
#[derive(Debug, Clone)]
pub struct ProtocolDecl {
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) protocol …`.
    pub is_package_pub: bool,
    pub name: String,
    pub name_span: Span,
    pub messages: Vec<ProtocolMessage>,
    pub span: Span,
}

/// D-STATE-DECL (ratified 2026-06-25, option B): `state TypeName { A, B, C }` —
/// a bounded compile-time state-set declaration. Each string in `states` is a valid
/// state label; `#State(X)` / `#Transition(A -> B)` markers on `TypeName::*` methods
/// must reference labels from this set. Erases in codegen (I3, no runtime discriminant).
#[derive(Debug, Clone)]
pub struct StateDecl {
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) state …`.
    pub is_package_pub: bool,
    pub type_name: String,
    pub type_name_span: Span,
    /// Declared state labels in declaration order, with their name spans for diagnostics.
    pub states: Vec<(String, Span)>,
    pub span: Span,
}

/// D-METADERIVE1=A: `derive T.Trait { … }` user-authored derive.
#[derive(Debug, Clone)]
pub struct DeriveDef {
    pub trait_name: String,
    pub trait_span: Span,
    pub type_param: String,
    pub body: Vec<Stmt>,
    pub span: Span,
}

/// S28: method signature inside a trait block (body optional per D-LIB2).
#[derive(Debug, Clone)]
pub struct TraitMethodSig {
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub span: Span,
    /// D-LIB2: optional default body for a trait method.
    pub default_body: Option<Vec<Stmt>>,
    /// D-EFF3: `@Pure fn hash(self)` — the method declares the empty effect set
    /// as its upper bound. Every impl's inferred effects must be empty (E0742),
    /// and a dynamic-dispatch call sees the empty set.
    pub is_pure: bool,
    /// D-EFF3: `fn render(self) #(Gpu)` — an effect upper bound on the method.
    /// `None` = un-annotated (per-impl effects under static dispatch; a
    /// trait-object call under an effect ceiling is E0743). `Some(list)` is BOTH
    /// the impl obligation (inferred ⊆ bound, else E0742) AND the dispatch
    /// contract (a trait-object call's effect IS the bound).
    pub declared_effects: Option<Vec<(String, Span)>>,
}

/// S28: `impl Trait { … }` inside a struct or enum body.
#[derive(Debug, Clone)]
pub struct TraitImplBlock {
    pub trait_name: String,
    pub trait_span: Span,
    pub methods: Vec<Func>,
    /// D-LIB2: `type Name = ConcreteType;` associated type implementations.
    pub assoc_type_impls: Vec<(String, Span, Type)>,
}

/// S50: one `extern rust` block declaring foreign functions.
#[derive(Debug, Clone)]
pub struct ExternRustBlock {
    /// `"std"` or `"crate@version"`.
    pub crate_spec: String,
    pub crate_span: Span,
    pub functions: Vec<ExternFn>,
    pub span: Span,
}

/// S50: foreign function — Jet signature plus `= "rust::path"`, no body.
#[derive(Debug, Clone)]
pub struct ExternFn {
    /// D-CABI-PLATFORM1=A: explicit per-function native ABI. `None` means C.
    pub abi: Option<(String, Span)>,
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub return_type_span: Option<Span>,
    pub rust_path: String,
    pub rust_path_span: Span,
    /// Compiler-owned effect root for a generated foreign binding. User-written
    /// extern declarations leave this unset and retain maximal foreign effects.
    pub effect_root: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TestDef {
    pub span: Span,
    pub name: String,
    pub name_span: Span,
    /// D-TEST1 (ratified 2026-06-22, option B): a property test is an `#Test fn`
    /// with parameters — inputs are generated from the parameter types and a
    /// failing case is automatically shrunk. An empty `params` (the
    /// `#Test "name" { … }` block form) is a plain unit test. The two forms share
    /// one AST node; `params.is_empty()` distinguishes them.
    pub params: Vec<Param>,
    /// D-TEST1: span of the `fn name(…)` signature for diagnostics on a property
    /// test (param-type errors point here). `None` for the block form.
    pub fn_keyword_span: Option<Span>,
    pub body: Vec<Stmt>,
}

/// D-BENCH1/D-BENCH-MARKER1=A: `#Bench("name") { … }` — identical structure to `TestDef`. The
/// body is a bare statement list timed by the generated bench harness.
#[derive(Debug, Clone)]
pub struct BenchDef {
    pub span: Span,
    pub name: String,
    pub name_span: Span,
    pub body: Vec<Stmt>,
}

/// D-MARK-META1=B: doc-only API maturity value on a function. Parsed from
/// `#Meta(maturity: .…)` and
/// formatter-preserved; zero sema/codegen effect (no call-site propagation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaturityTag {
    Experimental,
    Tested,
    Hardened,
}

impl MaturityTag {
    pub fn as_str(self) -> &'static str {
        match self {
            MaturityTag::Experimental => crate::Syntax::ATTR_EXPERIMENTAL,
            MaturityTag::Tested => crate::Syntax::ATTR_TESTED,
            MaturityTag::Hardened => crate::Syntax::ATTR_HARDENED,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Func {
    /// Exact parser-owned declaration boundary, including `fn` through body.
    pub span: Span,
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) fn …`.
    pub is_package_pub: bool,
    /// D-EXTMETH1=B: top-level `fn Type.method(...)` before parser normalization.
    /// The parser turns this into an inherent `ImplDef`; all later stages should
    /// see `None`.
    pub external_type: Option<(String, Span)>,
    pub name: String,
    pub name_span: Span,
    /// D-CANVASMETA1=B: `#Meta(...)` facts for Canvas/tooling. Checked by sema;
    /// ignored by codegen.
    pub meta: Option<MetaAttr>,
    /// S45 (M9): `<T: Bound>` after the function name.
    pub type_params: Vec<TypeParam>,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub return_type_span: Option<Span>,
    /// S58 (E2-M13): `#Unsafe` on the line before `fn` — a whole-function
    /// contract. Calling such a function requires an enclosing `#Unsafe`
    /// block (else E3103). D-UNSAFE-REASON1=B: the reason is optional but
    /// missing it emits L3101.
    pub is_unsafe: bool,
    pub unsafe_reason: Option<String>,
    pub unsafe_span: Option<Span>,
    /// S60 (E2-M16): `pure fn` — impure calls inside the body are E3401.
    pub is_pure: bool,
    /// D-TAINT1 (ratified 2026-06-21): `#Sanitizer fn` — the blessed taint-strip
    /// function. Its return value is **untainted by contract** even when its
    /// inputs are tainted; this is the one place taint is cleared before a sink.
    /// Static, erased in codegen (I3).
    pub is_sanitizer: bool,
    /// D-EFF1 / D-QUAL1: a `#(Net, Db)` effect bound on the signature, between
    /// the parameter list and the return arrow. `None` = unannotated (effects
    /// inferred). `Some(list)` = a declared upper bound; the inferred set must be
    /// a subset (E0740). Names are validated in sema, not the parser. Erased in
    /// codegen (I3).
    pub declared_effects: Option<Vec<(String, Span)>>,
    /// D-EFF2 (`#(via f)` pass-through): when set, this function publishes a tight
    /// pass-through — its effect set IS whatever the callback parameter named `f`
    /// carries, rather than the conservative flow-through default. Holds the param
    /// name and the span of the `via` clause. Mutually exclusive with
    /// `declared_effects` (a `#(via f)` annotation occupies the same `#(…)` slot).
    /// Erased in codegen (I3).
    pub effect_via: Option<(String, Span)>,
    /// D-STATE1 (ratified 2026-06-22): `#State(S) fn …` — a require-state marker.
    /// `Some((state, span))` means the method's receiver must currently be in state
    /// `state`; calling it on a value in any other state is E0150. `None` =
    /// unguarded. Compile-time only, erased in codegen (I3).
    pub state_requires: Option<(String, Span)>,
    /// D-STATE1: `#Transition(From -> To) fn …` — a transition declaration. The fn
    /// consumes a value in state `from` (the wildcard `_` → `None`, an entry
    /// transition with no prior state) and produces one in state `to`. A call
    /// requires the receiver/argument be in `from` (E0150 otherwise) and advances it
    /// to `to`. The `Span` points at the marker. Erased in codegen (I3).
    pub state_transition: Option<StateTransition>,
    /// D-REACTCORE1: `#Reactive fn` — reactive effect scope; must not return a value.
    pub is_reactive: bool,
    /// D-REPLAY1: `#Replayable fn` — the reachable effect set must not include
    /// ambient Time/Rand/Net/Io unless routed through deterministic capabilities.
    pub is_replayable: bool,
    pub replayable_span: Option<Span>,
    /// D-JPK-TASKRUN1 / D-SCHEDULE1 (card #505): `#Task fn` — a top-level
    /// function jetpack can invoke by name (`jetpack run <name>`). Top-level
    /// only (E0925 elsewhere). Erased in codegen (I3) — an ordinary fn.
    pub is_task: bool,
    pub task_span: Option<Span>,
    /// D-SCHEDULE1 (ratified 2026-07-11, card #505): `#Every(...)` — a
    /// declarative schedule on a `#Task fn`. `None` means unscheduled (a
    /// plain task, invoked manually only). Legal only alongside `is_task`
    /// (E0925 otherwise). Compile-checked (E0926 on a bad argument), then
    /// carried as metadata for `jet dev`/service-runtime/jetos consumers —
    /// erased in codegen (I3), never a runtime value the generated fn sees.
    pub every: Option<EveryMarker>,
    /// D-MUSTUSE1 (c18iwxqx): `@MustUse fn` / `@MustUse` method — callers must not
    /// drop the return value as a bare expression statement (E0419).
    pub is_must_use: bool,
    pub must_use_span: Option<Span>,
    /// D-MARK-META1=B: `#Meta(maturity: .…)` — documentation
    /// stability tag. Stored for fmt/docs/IDE; erased for sema and codegen.
    pub maturity: Option<MaturityTag>,
    pub maturity_span: Option<Span>,
    /// D-METHODMACRO1=A: `@Inline fn` / method — a soft hint (`#[inline]` in
    /// codegen); never rejected by sema.
    pub is_inline: bool,
    /// D-METHODMACRO1=A: `@InlineAlways fn` / method — a checked promise
    /// (`#[inline(always)]` in codegen). Sema rejects it (E0917/E0918/E0919)
    /// when the compiler can prove it genuinely cannot inline. Mutually
    /// exclusive with `is_inline` (E0920 if both are written).
    pub is_inline_always: bool,
    /// Span of whichever `@Inline`/`@InlineAlways` marker was written (for
    /// diagnostics); `None` when neither is present.
    pub inline_span: Option<Span>,
    /// D-WASM1: `#Wasm` / `#Js` / `#WasmExport` partition marker on the function.
    pub web_marker: Option<crate::WebPartition::WebPartitionMarker>,
    /// D-PREPOST1: `@Pre(cond, "msg")` clauses — a claim about the arguments,
    /// checked at function entry. Repeatable; empty when none.
    pub pre: Vec<ContractClause>,
    /// D-PREPOST1: `@Post(cond, "msg")` clauses — a claim about `result` (the
    /// return value), checked before each return. Repeatable; empty when none.
    pub post: Vec<ContractClause>,
    /// D-FFI-INLINE1=A (ratified 2026-07-11, card #501): `#FFI(<lang>) fn` inline
    /// foreign tier. `None` = an ordinary Jet function (`body` holds its
    /// statements). `Some` = the function's body is one string of foreign source
    /// the per-language binder compiles; `body` is empty and the Jet signature is
    /// the checked contract sema enforces at every call site.
    pub inline_foreign: Option<InlineForeign>,
    pub body: Vec<Stmt>,
}

/// D-FFI-INLINE1=A (card #501): the inline foreign tier payload on a
/// `#FFI(<lang>) fn`. `lang` is the raw language name written in `#FFI(<lang>)`
/// (validated in sema, not the parser — same convention as effect names);
/// `source` is the single `"""…"""` string body of foreign source.
#[derive(Debug, Clone)]
pub struct InlineForeign {
    /// The language name inside `#FFI(<lang>)`, e.g. `c`, `cpp`, `asm`.
    pub lang: String,
    /// Span of the language name, for diagnostics.
    pub lang_span: Span,
    /// Span of the `#FFI(...)` marker as a whole.
    pub marker_span: Span,
    /// The verbatim foreign source from the `"""…"""` body.
    pub source: String,
    /// Span of the foreign-source string literal.
    pub source_span: Span,
}

/// D-PREPOST1: one `@Pre`/`@Post` contract clause — a pure condition plus the
/// message shown when it's violated (E3005 at runtime). `message_span` points
/// at the message string literal for diagnostics.
#[derive(Debug, Clone)]
pub struct ContractClause {
    pub cond: Expr,
    pub message: String,
    pub message_span: Span,
    pub span: Span,
}

/// D-STATE1: the parsed `#Transition(From -> To)` declaration on a function. `from`
/// is `None` for an entry transition (`_ -> To`).
#[derive(Debug, Clone)]
pub struct StateTransition {
    pub from: Option<String>,
    pub to: String,
    pub span: Span,
}

/// D-SCHEDULE1 (ratified 2026-07-11, card #505): the raw `#Every(…)`
/// argument as the parser saw it — a duration literal (`#Every(5min)`) or a
/// quoted daily wall-clock time (`#Every("03:00")`). Sema resolves this
/// (`Syntax::resolve_every_schedule`) into a checked `EverySchedule`,
/// pushing E0926 on a bad value; codegen never reads it (I3, erased).
#[derive(Debug, Clone)]
pub enum EveryArg {
    /// `#Every(5min)` — same raw pieces as `Expr::UnitLit`, minus the
    /// `#UnitFamily` scoping (a schedule duration is a fixed, closed
    /// vocabulary — `Syntax::schedule_duration_suffix_nanos`).
    Duration {
        int: Option<i64>,
        float: Option<f64>,
        suffix: String,
        suffix_span: Span,
    },
    /// `#Every("03:00")` — the plain string content (no interpolation).
    WallClock { text: String, text_span: Span },
}

/// D-SCHEDULE1: the whole `#Every(…)` marker — its raw argument plus the
/// span of the marker itself (diagnostics point here by default).
#[derive(Debug, Clone)]
pub struct EveryMarker {
    pub arg: EveryArg,
    pub span: Span,
}

/// D-SCHEDULE1: a resolved, checked `#Every(…)` schedule — what
/// `Syntax::resolve_every_schedule` produces from a valid `EveryArg`. One
/// value; every consumer (`jet dev`, the service runtime, a jetos timer
/// projection) derives from the same `EveryArg` instead of re-parsing it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EverySchedule {
    /// Re-run every `nanos` nanoseconds since the task last ran.
    Interval { nanos: u128 },
    /// Re-run once daily at this local 24h wall-clock time.
    DailyAt { hour: u8, minute: u8 },
}

/// D-SCHEDULE1: why `resolve_every_schedule` rejected an `EveryArg` — sema
/// turns each into the matching E0926 What/Why/Fix row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EveryScheduleError {
    /// The duration suffix isn't in the closed schedule vocabulary
    /// (`ns`/`us`/`ms`/`s`/`min`).
    UnknownDurationUnit,
    /// The duration is zero or negative — a schedule must advance time.
    NonPositiveDuration,
    /// The wall-clock string isn't exactly `HH:MM` (two digits, `:`, two
    /// digits).
    BadWallClockFormat,
    /// `HH` is outside `00`..=`23`.
    HourOutOfRange,
    /// `MM` is outside `00`..=`59`.
    MinuteOutOfRange,
}

impl EveryArg {
    /// D-SCHEDULE1: resolve this raw `#Every(…)` argument into a checked
    /// schedule. Sema calls this once to decide E0926; a runtime consumer
    /// (`jet dev`, …) calls it again to get the identical answer — one
    /// function, nothing cached to drift between the two callers.
    pub fn resolve(&self) -> Result<EverySchedule, EveryScheduleError> {
        match self {
            EveryArg::Duration { int, float, suffix, .. } => {
                let Some(unit_nanos) = crate::Syntax::schedule_duration_suffix_nanos(suffix)
                else {
                    return Err(EveryScheduleError::UnknownDurationUnit);
                };
                let value = float.unwrap_or_else(|| int.unwrap_or(0) as f64);
                if value <= 0.0 {
                    return Err(EveryScheduleError::NonPositiveDuration);
                }
                let nanos = (value * unit_nanos as f64).round() as u128;
                if nanos == 0 {
                    return Err(EveryScheduleError::NonPositiveDuration);
                }
                Ok(EverySchedule::Interval { nanos })
            }
            EveryArg::WallClock { text, .. } => {
                let bytes = text.as_bytes();
                let digits_ok = bytes.len() == 5
                    && bytes[2] == b':'
                    && bytes[0].is_ascii_digit()
                    && bytes[1].is_ascii_digit()
                    && bytes[3].is_ascii_digit()
                    && bytes[4].is_ascii_digit();
                if !digits_ok {
                    return Err(EveryScheduleError::BadWallClockFormat);
                }
                let hour: u32 = text[0..2].parse().unwrap_or(99);
                let minute: u32 = text[3..5].parse().unwrap_or(99);
                if hour > 23 {
                    return Err(EveryScheduleError::HourOutOfRange);
                }
                if minute > 59 {
                    return Err(EveryScheduleError::MinuteOutOfRange);
                }
                Ok(EverySchedule::DailyAt {
                    hour: hour as u8,
                    minute: minute as u8,
                })
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Param {
    pub convention: AccessConvention,
    pub name: String,
    pub name_span: Span,
    pub ty: Type,
    pub ty_span: Span,
    /// S61: trailing `= expr` default value. Only trailing params may have defaults.
    pub default: Option<Box<Expr>>,
    /// D-VARIADIC1: `name: ...T` — last parameter only; `ty` is the element type.
    pub variadic: bool,
    /// D-ANY-JAI1/D-VARARGBOUND1: `name: ...[TraitA, TraitB]` — an explicit
    /// trait-bound list on a variadic parameter (heterogeneous elements, one
    /// generic type slot per call-site argument, checked+monomorphized, zero
    /// boxing). `None` for a bare `name: ...T` — that covers both the
    /// D-VARIADIC1 homogeneous-concrete-type form and the `...Trait`
    /// single-bound sugar; sema tells the two apart the same way
    /// `resolve_type_name` already does (is `T` a registered trait name?).
    /// When `Some`, `ty`/`ty_span` are an unused placeholder (`Type::Named("")`
    /// spanning the bracket list).
    pub variadic_bound_list: Option<Vec<String>>,
}

impl Param {
    /// D-ANY-JAI1/D-VARARGBOUND1: the resolved trait-bound list for a variadic
    /// parameter, or `None` when it's the plain D-VARIADIC1 homogeneous-concrete
    /// form. `is_trait_name` lets each crate plug in its own trait-name lookup
    /// (`TraitRegistry::is_trait_name` in sema, `Cx::trait_names` in codegen) —
    /// the classification rule itself lives once, here.
    pub fn variadic_trait_bounds(
        &self,
        is_trait_name: impl Fn(&str) -> bool,
    ) -> Option<Vec<String>> {
        if !self.variadic {
            return None;
        }
        if let Some(list) = &self.variadic_bound_list {
            return if list.is_empty() {
                None
            } else {
                Some(list.clone())
            };
        }
        if let Type::Named(n) = &self.ty {
            if !n.is_empty() && is_trait_name(n) {
                return Some(vec![n.clone()]);
            }
        }
        None
    }
}

/// D-REPRC1 / D-SOA1 (ratified): the variant of `#layout(…)` on a struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructLayout {
    /// `#layout(c)` → `#[repr(C)]` on the generated Rust struct. (D-REPRC1=B)
    C,
    /// D-SOA1 / D-SOA2A=C: `#layout(columnar)` → a `[S]` collection is stored
    /// struct-of-arrays (one `Vec` per field) instead of array-of-structs. The
    /// logical `S` value and field-access syntax are unchanged; only the memory
    /// layout of the `[S]` collection differs (cache-friendly). Whole-struct only
    /// in v1 (D-SOA2B=A).
    Columnar,
}

/// D-REPRC2: selected C enum tag representation. `CInt` is C's `int`;
/// fixed-width forms are the explicit expert override.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CEnumTag { CInt, U8, I8, U16, I16, U32, I32, U64, I64 }

impl EnumDef {
    pub fn c_layout_tag(&self) -> Option<CEnumTag> {
        let marker = self.type_markers.iter().find(|m| m.name == crate::Syntax::ATTR_LAYOUT)?;
        let Some(Expr::Ident(first, _)) = marker.args.first() else { return None };
        if !first.eq_ignore_ascii_case("c") { return None; }
        Some(match marker.args.get(1) {
            None => CEnumTag::CInt,
            Some(Expr::Ident(n, _)) => match n.as_str() {
                "U8" => CEnumTag::U8, "I8" => CEnumTag::I8,
                "U16" => CEnumTag::U16, "I16" => CEnumTag::I16,
                "U32" => CEnumTag::U32, "I32" => CEnumTag::I32,
                "U64" => CEnumTag::U64, "I64" => CEnumTag::I64,
                _ => return None,
            },
            _ => return None,
        })
    }
}

/// D-ATTR2 / D-SERDE2–8: one `#[Name]` or `#[Name(arg, …)]` bracket marker.
/// Derive-trait markers (`Codable`/`Encode`/`Decode`/`Comparable`/…) are lifted
/// into `derives` at parse time (Codable expands to Encode+Decode); the serde
/// *attribute* markers — container `RenameAll`/`DenyUnknownFields`/`Tag`/`Untagged`
/// and field `Rename`/`Skip`/`Default`/`Flatten` — are kept raw here so sema can
/// validate them (E2407–E2412) and codegen can apply them. Args parse as
/// expressions: a string literal (`Rename("x")`), a bare word (`RenameAll(camel)`
/// → `Expr::Ident`), or any expression (`Default(8080)`).
#[derive(Debug, Clone)]
pub struct Marker {
    pub name: String,
    pub name_span: Span,
    pub args: Vec<Expr>,
    pub span: Span,
    /// D-MARK-DEBUG1 follow-up (card #498): the sigil this marker was
    /// actually written with — `'@'` (contract plane) or `'#'` (directive/
    /// serde plane). Set once at parse time (the parser already knows which
    /// bracket/prefix it is bumping past). The formatter re-emits a marker
    /// under THIS sigil, not `Syntax::is_contract_marker(&name)` — that
    /// classification answers "which plane does this name legally belong
    /// to" (E0062/E0063 teaching, derive-vs-serde split) and can diverge
    /// from a marker's written sigil once a name is retired from
    /// `CONTRACT_MARKERS`/`DIRECTIVE_MARKERS` (e.g. `Debug`, still a real
    /// `@`-plane trait name a user can type, just no longer a registered
    /// opt-in derive) — re-emission must preserve what the user wrote.
    pub sigil: char,
    /// Card #131 / D-SERDE5: for a `#[Default(expr)]` field marker, the
    /// compile-time value its argument evaluates to. Sema fills this once
    /// (`eval_default_markers`) so both the AOT codegen tier and the comptime
    /// decode tier bake the *exact same* value (R12 parity) — a non-primitive
    /// default never silently degrades to `Default::default()`. `None` for
    /// every non-`Default` marker and for a bare `#[Default]` (zero value).
    pub ct: Option<CtValue>,
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub span: Span,
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) struct …`.
    pub is_package_pub: bool,
    pub name: String,
    pub name_span: Span,
    /// S45: `<T>` after the struct name.
    pub type_params: Vec<TypeParam>,
    pub fields: Vec<Field>,
    pub methods: Vec<Func>,
    /// S28: in-type `impl Trait { … }` blocks.
    pub trait_impls: Vec<TraitImplBlock>,
    /// S55: `derive Comparable;` / `derive Serialize;` lines.
    pub derives: Vec<(String, Span)>,
    /// D-MIGRATE1 (ratified 2026-06-22): `@PublishedSchema` marker was present
    /// before `struct`. The span is retained for pointing at the annotation in E0910.
    pub is_published_schema: bool,
    pub published_schema_span: Option<Span>,
    /// D-LIN1 (ratified 2026-06-21): `#SingleUse` marker before `struct` — values
    /// of this type must be consumed exactly once on every path (E0140/E0141)
    /// and may not be aliased (E0142). Implies `#NoCopy`. The span points at the
    /// marker for diagnostics.
    pub is_single_use: bool,
    pub single_use_span: Option<Span>,
    /// D-MUSTUSE1 (c18iwxqx): `@MustUse` marker before `struct` — values of this
    /// type cannot be silently ignored as a bare expression statement (E0419).
    pub is_must_use: bool,
    pub must_use_span: Option<Span>,
    /// D-REPRC1 (ratified; D-REPRC1 = B): `#layout(…)` attribute. `None` = default layout.
    pub layout: Option<StructLayout>,
    pub layout_span: Option<Span>,
    /// D-SERDE3/8: container-level serde attribute markers (`RenameAll`,
    /// `DenyUnknownFields`) attached before the `struct`. Empty when none.
    pub serde_markers: Vec<Marker>,
    /// Round-trip fidelity (formatter): the exact `#[…]` bracket-marker list the
    /// user wrote before the type, verbatim and in source order (e.g. `Codable`,
    /// `RenameAll(camel)`). Derive-trait markers here are *also* lowered into
    /// `derives` (Codable → Encode+Decode) for sema/codegen, and serde attrs are
    /// *also* copied into `serde_markers`; this field exists only so `jet fmt`
    /// re-emits the surface the user actually typed instead of a lowered form.
    /// Empty when the type had no leading `#[…]` list.
    pub type_markers: Vec<Marker>,
    /// D-VALIDATE1 (ratified 2026-07-12, card #506): `validate { … }` in-body
    /// block. Each statement is a `check(cond, at: field, "msg")` call —
    /// sema resolves `field` as a bare sibling reference (D-FIELDPOL1) and
    /// purity-checks `cond`/`msg` (reuses the `@Pre`/`@Post` checker). All
    /// failing `check`s accumulate into `[FieldError]`; `Type.validate(value)`
    /// runs the block standalone and `decode<T>()` runs it automatically on
    /// a successfully shape-decoded value. Empty when the struct declares no
    /// `validate { }` block.
    pub validate_block: Vec<Stmt>,
    pub validate_span: Option<Span>,
}

/// D-TYPEALIAS1: `alias Name<T, E> = T ? E` — transparent generic type shortcut.
#[derive(Debug, Clone)]
pub struct TypeAliasDef {
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) alias …`.
    pub is_package_pub: bool,
    pub name: String,
    pub name_span: Span,
    pub type_params: Vec<TypeParam>,
    pub target: Type,
    pub target_span: Span,
    pub span: Span,
}

/// D-DIST1/D-DIST3: distinct type declaration — `[@Numeric] Name :: distinct Base`.
#[derive(Debug, Clone)]
pub struct DistinctDef {
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) Name :: distinct Base`.
    pub is_package_pub: bool,
    /// D-DIST3: whether `@Numeric` marker was present — enables same-type arithmetic.
    pub is_numeric: bool,
    /// D-CAPBUNDLE1: `@Comparable` was present — grants hash/sort on top of the
    /// ordering the base type already carries (D-DIST1 makes every distinct type
    /// `==`/`<`-comparable unconditionally already; that overlap is left alone
    /// per the ballot). Stacks with the other three bundles.
    pub is_comparable: bool,
    pub comparable_span: Option<Span>,
    /// D-CAPBUNDLE1: `@Printable` was present — grants `{value}` string
    /// interpolation (Display), forwarding to the base type's rendering.
    pub is_printable: bool,
    pub printable_span: Option<Span>,
    /// D-CAPBUNDLE1: `@CodableAsBase` was present — grants encode/decode via
    /// the base type's wire representation.
    pub is_codable_as_base: bool,
    pub codable_as_base_span: Option<Span>,
    pub name: String,
    pub name_span: Span,
    pub base: Type,
    pub base_span: Span,
    /// D-RANGETYPE1: an optional literal range constraint — `distinct
    /// Int(0..10)` provably holds `0..=10` (`..` inclusive, S22). `(lo, hi,
    /// span-of-the-`(lo..hi)`-clause)`.
    pub range: Option<(i64, i64, Span)>,
    pub span: Span,
}

/// D-QUAL3 (ratified 2026-06-24): unit-family declaration —
/// `#UnitFamily(currency) { usd, eur, gbp }`. Each member mints a distinct
/// `@Numeric` type erasing to `Float`. `members` carries each member's source
/// spelling (lowercase, e.g. `usd`) and span; the minted type name is the
/// PascalCase form (`Usd`).
#[derive(Debug, Clone)]
pub struct UnitFamilyDef {
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) #UnitFamily(…) { … }`.
    pub is_package_pub: bool,
    /// The family label, e.g. `currency` — documentation only; not a type name.
    pub family: String,
    pub family_span: Span,
    /// Each member's source spelling and span (e.g. `("usd", span)`).
    pub members: Vec<(String, Span)>,
    pub span: Span,
}

impl UnitFamilyDef {
    /// PascalCase the member spelling to its minted distinct-type name:
    /// `usd` → `Usd`, `m_per_s` → `MPerS`. Splits on `_`, uppercases each
    /// segment's first char. Empty/edge inputs return the input unchanged.
    pub fn type_name(member: &str) -> String {
        let mut out = String::with_capacity(member.len());
        for seg in member.split('_') {
            let mut chars = seg.chars();
            if let Some(first) = chars.next() {
                out.extend(first.to_uppercase());
                out.push_str(chars.as_str());
            }
        }
        if out.is_empty() {
            member.to_string()
        } else {
            out
        }
    }

    /// The minted `DistinctDef` for each member (`@Numeric`, base `Float`).
    /// Used by sema registration and codegen to lower the family.
    pub fn distinct_defs(&self) -> Vec<DistinctDef> {
        self.members
            .iter()
            .map(|(member, span)| DistinctDef {
                is_pub: self.is_pub,
                is_package_pub: self.is_package_pub,
                is_numeric: true,
                is_comparable: false,
                comparable_span: None,
                is_printable: false,
                printable_span: None,
                is_codable_as_base: false,
                codable_as_base_span: None,
                name: Self::type_name(member),
                name_span: *span,
                base: Type::Float,
                base_span: *span,
                range: None,
                span: *span,
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct EnumDef {
    pub span: Span,
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) enum …`.
    pub is_package_pub: bool,
    pub name: String,
    pub name_span: Span,
    pub type_params: Vec<TypeParam>,
    pub variants: Vec<Variant>,
    pub methods: Vec<Func>,
    pub trait_impls: Vec<TraitImplBlock>,
    pub derives: Vec<(String, Span)>,
    /// D-LIN1 (ratified 2026-06-21): `#SingleUse` marker before `enum`. See
    /// `StructDef::is_single_use`.
    pub is_single_use: bool,
    pub single_use_span: Option<Span>,
    /// D-MUSTUSE1 (c18iwxqx): `@MustUse` marker before `enum`. See
    /// `StructDef::is_must_use`.
    pub is_must_use: bool,
    pub must_use_span: Option<Span>,
    /// D-SERDE3/7/8: container-level serde markers (`RenameAll`, `Tag`,
    /// `Untagged`, `DenyUnknownFields`) attached before the `enum`. Empty when none.
    pub serde_markers: Vec<Marker>,
    /// Round-trip fidelity (formatter): the exact `#[…]` bracket-marker list the
    /// user wrote before the `enum`, verbatim and in source order. See
    /// `StructDef::type_markers`. Empty when none.
    pub type_markers: Vec<Marker>,
    /// D-TAG1 (ratified 2026-07-03): variant groups, in declaration order. A
    /// variant may enclose sub-variants in `{ }`; the parser FLATTENS leaves
    /// into `variants` (dotted paths, e.g. `Fire.Burn`) and records each group
    /// path here so sema knows the subtree structure and the formatter can
    /// reconstruct the nesting. Empty for a flat enum.
    pub groups: Vec<EnumGroup>,
}

/// D-TAG1: one variant group (`Physical { Blunt, Pierce }`). `path` is the full
/// dotted path from the enum root (`Physical`, or `Net.Http` when nested);
/// `name_span` covers the group's own name segment.
#[derive(Debug, Clone)]
pub struct EnumGroup {
    pub path: String,
    pub name_span: Span,
}

#[derive(Debug, Clone)]
pub struct Variant {
    /// The variant's full dotted path from the enum root. A flat variant is a
    /// bare name (`Cold`); a leaf inside D-TAG1 groups is dotted (`Fire.Burn`).
    /// A value is always a leaf — group paths never appear here (they live in
    /// `EnumDef::groups`).
    pub name: String,
    pub name_span: Span,
    pub payload: VariantPayload,
    /// D-REPRC2: explicit C discriminant (`Variant = 7`). Only meaningful on
    /// unit variants; sema rejects it elsewhere.
    pub discriminant: Option<i64>,
    /// D-SERDE5: per-variant serde markers (`Rename`). Empty when none.
    pub serde_markers: Vec<Marker>,
}

#[derive(Debug, Clone)]
pub enum VariantPayload {
    Unit,
    /// S30: single-field variants use a positional type only.
    Single(Type, Span),
    /// S30: two or more payload fields are named in the declaration.
    Named(Vec<VariantField>),
}

#[derive(Debug, Clone)]
pub struct VariantField {
    pub name: String,
    pub name_span: Span,
    pub ty: Type,
    pub ty_span: Span,
}

#[derive(Debug, Clone)]
pub struct ImplDef {
    pub span: Span,
    pub type_name: String,
    pub type_span: Span,
    /// S28: `impl Type: Trait` — `None` means plain `impl Type { fn … }`.
    pub trait_name: Option<String>,
    pub trait_span: Option<Span>,
    pub methods: Vec<Func>,
    /// S62: `impl Type: Trait using field_name;` — the field that supplies the
    /// delegation target. When `Some`, `methods` is empty and the compiler
    /// generates forwarding for all trait methods.
    pub delegation_field: Option<String>,
    /// D-LIB2: `type Name = ConcreteType;` in top-level impl blocks.
    pub assoc_type_impls: Vec<(String, Span, Type)>,
    /// True only for codec impls synthesized from a serde derive. Parsed user
    /// impls are always false, even when type and trait names match.
    pub is_generated_serde: bool,
    /// D-OSTARGET1=A (ratified 2026-07-01, c134): `#Target(Os.Linux|Os.Macos|Os.Windows)`
    /// before this `impl` block — native OS gating (Phase 8 native backends).
    /// `None` means this impl compiles for every OS. Only ratified at item
    /// (impl) scope, not per-function — the ballot's worked example gates
    /// whole backend impls, never individual methods.
    pub os_target: Option<crate::OsTarget::OsTarget>,
}

#[derive(Debug, Clone)]
pub struct Field {
    /// S18: visible to other files via `import` when true.
    pub is_pub: bool,
    /// D-PUBPKG1=A: true for `pub(package) fieldname: T`.
    pub is_package_pub: bool,
    pub name: String,
    pub name_span: Span,
    pub ty: Type,
    pub ty_span: Span,
    /// D-SERDE5: per-field serde markers (`Rename`/`Skip`/`Default`/`Flatten`)
    /// attached before this field. Empty when none.
    pub serde_markers: Vec<Marker>,
    /// D-DEBUG-REDACT: `@[Redact]` — omit/redact in auto-derived Debug output.
    pub redact: bool,
    /// D-FIELDPOL1 (ratified 2026-07-03, card #181): `name: T => expr` — a
    /// computed field. Never stored; every read recomputes `expr` against the
    /// struct's current sibling fields. `expr` is parsed with bare sibling
    /// field names still as plain `Ident`s — sema rewrites each one to
    /// `self.<field>` once every field of the struct is known (see
    /// `Sema::CheckerFieldPolicy`). `None` for an ordinary stored field.
    pub computed: Option<Box<Expr>>,
}
