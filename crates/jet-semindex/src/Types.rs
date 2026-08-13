//! D-SEMINDEX1: stable public query types (versioned independently of LSP internals).

use jet_foundation::Diagnostics::Span;
use jet_pkg_model::Overlay::OverlayPolicy;
use jet_pkg_model::Package::PackageFacts;

/// Schema version for JSON snapshots and API consumers. Bump when the exported
/// fact shape changes incompatibly.
pub const SCHEMA_VERSION: u32 = 14;

/// Canonical JSON values for additive tooling projections. Keeping this small
/// value model in the semantic-index crate prevents CLI consumers from
/// inventing a second serializer or passing unvalidated JSON fragments into
/// the index document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpandValue {
    Null,
    Bool(bool),
    Number(usize),
    String(String),
    Array(Vec<ExpandValue>),
    Object(Vec<(String, ExpandValue)>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandLens {
    pub name: String,
    pub summary: String,
    pub facts: Vec<ExpandValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandProjection {
    pub selection: String,
    pub lenses: Vec<ExpandLens>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewSourceFact {
    Receiver,
    Parameter(usize),
    Static { module_path: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewProjectionFact {
    Field(String),
    Index,
    Range,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewSourcePathFact {
    pub source: ViewSourceFact,
    pub projections: Vec<ViewProjectionFact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewProvenanceFact {
    pub output_path: Vec<String>,
    pub sources: Vec<ViewSourcePathFact>,
    pub mutable: bool,
}

/// One checked parameter row for the callable-signature inspection lens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallableParameterFact {
    pub name: String,
    pub label: String,
    pub default: Option<String>,
    pub access: String,
    pub zone: String,
    pub ty: String,
    pub variadic: bool,
}

/// The complete semindex projection of a checked callable contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallableSignatureFact {
    pub parameters: Vec<CallableParameterFact>,
    pub effects: Vec<String>,
    pub errors: Vec<String>,
    pub returned_views: Vec<ViewProvenanceFact>,
    pub policies: Vec<String>,
}

impl ViewProvenanceFact {
    pub fn canonical(&self) -> String {
        let access = if self.mutable { "write" } else { "read" };
        let source_paths = self
            .sources
            .iter()
            .map(|source_path| {
                let source = match &source_path.source {
                    ViewSourceFact::Receiver => "receiver".to_string(),
                    ViewSourceFact::Parameter(index) => format!("parameter:{index}"),
                    ViewSourceFact::Static { module_path, name } => {
                        format!("static:{module_path}::{name}")
                    }
                };
                let path = source_path
                    .projections
                    .iter()
                    .map(|projection| match projection {
                        ViewProjectionFact::Field(name) => format!("field:{name}"),
                        ViewProjectionFact::Index => "index".to_string(),
                        ViewProjectionFact::Range => "range".to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join("/");
                (source, path)
            })
            .collect::<Vec<_>>();
        let slot = if self.output_path.is_empty() {
            "$".to_string()
        } else {
            self.output_path.join(".")
        };
        if let [(source, path)] = source_paths.as_slice() {
            format!("slot:{slot};{source};access:{access};path:{path}")
        } else {
            let sources = source_paths
                .iter()
                .map(|(source, path)| format!("{source};path:{path}"))
                .collect::<Vec<_>>()
                .join(",");
            format!("slot:{slot};one_of({sources});access:{access}")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceApplicationFact {
    pub name: String,
    pub module_path: String,
    pub semantic_identity: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceFact {
    pub name: String,
    pub module_path: String,
    pub fingerprint: String,
    pub full_key_hex: String,
    pub template_definition_id: String,
    pub template_span: SourceSpan,
    pub arguments: Vec<String>,
    pub argument_values: Vec<String>,
    pub argument_provenance: Vec<Vec<String>>,
    pub applications: Vec<InstanceApplicationFact>,
    pub exported_members: Vec<String>,
}

/// Byte span in a source file (same coordinates as diagnostics).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceSpan {
    pub start: usize,
    pub end: usize,
}

impl From<Span> for SourceSpan {
    fn from(s: Span) -> Self {
        SourceSpan {
            start: s.start,
            end: s.end,
        }
    }
}

/// Semantic kind of a defined symbol (stable string types, not AST handles).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolKind {
    Module,
    Function {
        params: Vec<(String, String)>,
        /// Declaration-ordered public call contract. Local parameter names
        /// stay compiler-internal and are not part of this projection.
        call_contract: Vec<(String, String, bool)>,
        ret: Option<String>,
    },
    Struct {
        fields: Vec<(String, String)>,
    },
    Enum {
        variants: Vec<String>,
    },
    Trait,
    Tag,
    Type,
    Const,
    EnumVariant {
        parent: String,
    },
    Field {
        ty: String,
        parent: String,
    },
    Local {
        mutable: bool,
        ty: Option<String>,
    },
    Param {
        ty: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberKind {
    Field,
    Variant,
    Method,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberOrigin {
    TypeBody,
    InherentImpl,
    TraitImpl { trait_name: String },
    TraitRequirement { trait_name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberFact {
    pub owner: String,
    pub name: String,
    pub identity: String,
    pub kind: MemberKind,
    pub origin: MemberOrigin,
    pub signature: String,
    pub module_path: String,
    pub span: SourceSpan,
}

/// D-LINTPOLICY1=A (the override law): the kind of spelled bypass a
/// `BypassFact` records. Every expert escape hatch this law governs is
/// audited the same way — named at the site, never hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BypassKind {
    /// `#Unsafe("reason") { … }` — an audited region (S58).
    UnsafeRegion,
    /// `#Unsafe("reason") fn …` — an audited whole-function contract (S58).
    UnsafeFn,
    /// `.drop("reason")` — the sole intentional-discard spelling (D-IGNORERET2).
    ExplicitDrop,
    /// `#[allow(lint)]` — a source-level lint suppression (D-DECIMAL1 and kin).
    LintAllow,
}

impl BypassKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BypassKind::UnsafeRegion => "unsafe_region",
            BypassKind::UnsafeFn => "unsafe_fn",
            BypassKind::ExplicitDrop => "explicit_drop",
            BypassKind::LintAllow => "lint_allow",
        }
    }
}

/// D-LINTPOLICY1=A: one spelled bypass, as the override law's audit clause
/// requires — every bypass lands in the record, not just the flag/marker at
/// the site. `site` names what was bypassed (a function, a call's receiver
/// expression, a field); `detail` carries the reason string (`#Unsafe`/
/// `.drop`) or the suppressed lint name (`#[allow(...)]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BypassFact {
    pub kind: BypassKind,
    pub site: String,
    pub detail: String,
    pub module_path: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDossier {
    pub schema_version: u32,
    pub target: String,
    pub definition: Option<SymbolDef>,
    pub members: Vec<MemberFact>,
    pub references: Vec<SymbolRef>,
    /// D-LINTPOLICY1=A: every spelled bypass in the whole checked program
    /// (`#Unsafe(reason)`, `.drop(reason)`, `#[allow(lint)]`) — the override
    /// law's audit clause made concrete. Program-wide, not scoped to
    /// `target`, so a dossier is where a reviewer sees every expert escape
    /// hatch at once.
    pub bypass_facts: Vec<BypassFact>,
}

/// One named definition in the program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolDef {
    pub identity: String,
    pub name: String,
    /// Canonical typeable spelling from the sema name ledger.
    pub qualified_name: String,
    pub module_path: String,
    pub def_span: SourceSpan,
    pub kind: SymbolKind,
    /// Sema-proved owner source for a returned view, when this definition is a
    /// function. Kept beside `kind` to preserve the established enum shape.
    pub view_provenance: Vec<ViewProvenanceFact>,
    /// D-EXPANDCLI1: checked callable facts used by the derive/signature
    /// lenses. Kept on the canonical definition so both projections share
    /// one semindex document.
    pub callable_signature: Option<CallableSignatureFact>,
    /// D-ONCE-DERIVE1: capabilities already attached to this type.
    pub derives: Vec<String>,
}

/// Compiler-owned definition facts for conservative structural tools.
/// `stable_id` is an ancestry-class ID: it hashes semantic ancestry and kind,
/// not name, file, signature, or body. Same-kind siblings may intentionally
/// collide, so consumers must require a unique remaining candidate before
/// treating it as identity. `signature_id` classifies typed signature changes;
/// `content_id` addresses normalized source. Human spelling remains separate
/// so reports stay readable without making names the machine identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionFact {
    pub stable_id: String,
    pub signature_id: String,
    pub content_id: String,
    pub human_identity: String,
    pub name: String,
    pub kind: String,
    pub module_path: String,
    pub span: SourceSpan,
}

/// Immutable compiler definition anchor used by semantic refactors. Names are
/// intentionally absent: spelling can change, while module, kind, and defining
/// source span identify the declaration selected by name resolution.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DefinitionAnchor {
    pub module_path: String,
    pub kind: String,
    pub def_span: SourceSpan,
    pub semantic_identity: Option<String>,
}

/// One use-site reference (identifier occurrence).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolRef {
    pub name: String,
    pub module_path: String,
    pub scope_identity: Option<String>,
    /// Definition selected during the compiler's lexical AST traversal.
    /// `None` means unresolved or ambiguous; semantic refactors never fall
    /// back to spelling.
    pub target: Option<DefinitionAnchor>,
    pub span: SourceSpan,
}

/// One compiler-owned structural AST boundary. Spans come from the parsed AST,
/// not token-window inference, so structural tools cannot reinterpret an expr
/// as a stmt/item/type merely because its bytes happen to parse there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralNode {
    /// Stable within one module walk. Child edges refer to this ID directly;
    /// consumers never reconstruct AST ownership from overlapping spans.
    pub id: usize,
    pub parent: Option<usize>,
    /// Parser/AST field name (`callee`, `args`, `lhs`, `body`, ...).
    pub slot: String,
    pub slot_kind: StructuralSlotKind,
    pub ordinal: usize,
    pub class: String,
    pub shape: String,
    pub module_path: String,
    pub span: SourceSpan,
}

/// Exact parser-owned boundary of one structural AST slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralSlotBoundary {
    pub parent: usize,
    pub slot: String,
    pub module_path: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuralSlotKind {
    Scalar,
    List,
}

/// A direct call from one function to another (or an unresolved callee name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallEdge {
    pub caller: String,
    pub callee: String,
    pub module_path: String,
    pub call_span: SourceSpan,
}

/// Transitive effect facts for one function (after whole-program solve).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectFact {
    pub function: String,
    pub direct: Vec<String>,
    pub callees: Vec<String>,
    pub inferred: Vec<String>,
    pub maximal: bool,
    /// One deterministic witness for each inferred effect: the expanded
    /// function path plus source spans for call edges and the direct origin.
    pub provenance: Vec<EffectProvenance>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectProvenance {
    pub effect: String,
    pub call_path: Vec<String>,
    pub spans: Vec<SourceSpan>,
}

/// D-SHAPE-OUTPUT-CALLABLE1: one sema-resolved runnable Output. This is a
/// projection of the compiler fact, not a second package/output resolver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputFact {
    pub binding: String,
    pub kind: String,
    pub name: String,
    pub module_path: String,
    pub span: SourceSpan,
    pub entry: OutputEntryFact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputEntryFact {
    pub identity: String,
    pub name: String,
    pub module_path: String,
    pub definition_span: SourceSpan,
    pub reference_span: SourceSpan,
    pub params: Vec<String>,
    pub return_type: Option<String>,
    pub authority: String,
    pub effects: Vec<String>,
}

/// Versioned semantic index over one checked program bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemIndex {
    schema_version: u32,
    defs: Vec<SymbolDef>,
    refs: Vec<SymbolRef>,
    calls: Vec<CallEdge>,
    effects: Vec<EffectFact>,
    members: Vec<MemberFact>,
    nodes: Vec<StructuralNode>,
    definition_facts: Vec<DefinitionFact>,
    /// D-LINTPOLICY1=A: spelled bypasses across the whole checked program.
    /// Set separately from the constructor (`set_bypasses`) so existing
    /// callers of `SemIndex::new` are unaffected.
    bypasses: Vec<BypassFact>,
    instances: Vec<InstanceFact>,
    outputs: Vec<OutputFact>,
    /// Canonical Package/Config facts for the owning source tree, when this
    /// index was built for a package entry.
    package_facts: Option<PackageFacts>,
    workspace_overlay_policy: Option<OverlayPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralAudit {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<String>,
}

impl SemIndex {
    pub(crate) fn new(
        defs: Vec<SymbolDef>,
        refs: Vec<SymbolRef>,
        calls: Vec<CallEdge>,
        effects: Vec<EffectFact>,
        members: Vec<MemberFact>,
        nodes: Vec<StructuralNode>,
        definition_facts: Vec<DefinitionFact>,
    ) -> Self {
        SemIndex {
            schema_version: SCHEMA_VERSION,
            defs,
            refs,
            calls,
            effects,
            members,
            nodes,
            definition_facts,
            bypasses: Vec::new(),
            instances: Vec::new(),
            outputs: Vec::new(),
            package_facts: None,
            workspace_overlay_policy: None,
        }
    }

    /// D-LINTPOLICY1=A: attach the whole-program bypass facts collected
    /// during the walk. Separate from `new` so it can be filled after the
    /// walk without reordering every existing constructor call.
    pub(crate) fn set_bypasses(&mut self, bypasses: Vec<BypassFact>) {
        self.bypasses = bypasses;
    }

    pub(crate) fn set_instances(&mut self, instances: Vec<InstanceFact>) { self.instances = instances; }
    pub fn instances(&self) -> &[InstanceFact] { &self.instances }
    pub(crate) fn set_outputs(&mut self, outputs: Vec<OutputFact>) { self.outputs = outputs; }
    pub fn outputs(&self) -> &[OutputFact] { &self.outputs }

    pub fn package_facts(&self) -> Option<&PackageFacts> {
        self.package_facts.as_ref()
    }

    /// Attach the shared typed package model after the compiler facts exist.
    /// Existing package-neutral constructors remain valid.
    pub fn attach_package_facts(&mut self, facts: PackageFacts) {
        self.package_facts = Some(facts);
    }

    pub fn workspace_overlay_policy(&self) -> Option<&OverlayPolicy> {
        self.workspace_overlay_policy.as_ref()
    }

    /// Attach the persisted workspace policy consumed by package-facing
    /// tooling. The lock is the authority; source evaluation is not repeated
    /// here.
    pub fn attach_workspace_overlay_policy(&mut self, policy: OverlayPolicy) {
        self.workspace_overlay_policy = Some(policy);
    }

    pub fn bypasses(&self) -> &[BypassFact] {
        &self.bypasses
    }

    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn definitions(&self) -> &[SymbolDef] {
        &self.defs
    }

    pub fn references(&self) -> &[SymbolRef] {
        &self.refs
    }

    pub fn references_to(&self, name: &str) -> Vec<&SymbolRef> {
        self.refs.iter().filter(|r| r.name == name).collect()
    }

    pub fn call_edges(&self) -> &[CallEdge] {
        &self.calls
    }

    pub fn call_sites(&self, callee: &str) -> Vec<&CallEdge> {
        self.calls.iter().filter(|c| c.callee == callee).collect()
    }

    pub fn effects(&self) -> &[EffectFact] {
        &self.effects
    }

    pub fn members(&self) -> &[MemberFact] {
        &self.members
    }

    pub fn structural_nodes(&self) -> &[StructuralNode] {
        &self.nodes
    }

    pub fn definition_facts(&self) -> &[DefinitionFact] {
        &self.definition_facts
    }

    pub fn members_of(&self, owner: &str) -> Vec<&MemberFact> {
        self.members.iter().filter(|m| m.owner == owner).collect()
    }

    pub fn effect_of(&self, function: &str) -> Option<&EffectFact> {
        self.effects.iter().find(|e| e.function == function)
    }

    pub fn lookup(&self, name: &str) -> Option<&SymbolDef> {
        self.defs.iter().find(|d| d.name == name)
    }

    pub fn lookup_identity(&self, identity: &str) -> Option<&SymbolDef> {
        self.defs.iter().find(|d| d.identity == identity)
    }

    pub fn dossier(&self, target: &str) -> TypeDossier {
        let definition = self
            .definitions()
            .iter()
            .find(|d| d.name == target || d.identity == target)
            .cloned();
        let owner = definition
            .as_ref()
            .map(|d| d.name.as_str())
            .unwrap_or(target);
        let mut members: Vec<MemberFact> = self
            .members
            .iter()
            .filter(|m| m.owner == owner)
            .cloned()
            .collect();
        sort_members(&mut members);
        let mut references: Vec<SymbolRef> = self
            .refs
            .iter()
            .filter(|r| r.name == owner)
            .cloned()
            .collect();
        references.sort_by(|a, b| {
            a.module_path
                .cmp(&b.module_path)
                .then(a.span.start.cmp(&b.span.start))
                .then(a.span.end.cmp(&b.span.end))
        });
        // D-LINTPOLICY1=A: bypass facts are program-wide, not `target`-scoped
        // — the dossier is where every spelled expert escape hatch is
        // visible in one audit, not just the ones touching this symbol.
        let mut bypass_facts = self.bypasses.clone();
        bypass_facts.sort_by(|a, b| {
            a.module_path
                .cmp(&b.module_path)
                .then(a.span.start.cmp(&b.span.start))
        });
        TypeDossier {
            schema_version: SCHEMA_VERSION,
            target: owner.to_string(),
            definition,
            members,
            references,
            bypass_facts,
        }
    }

    pub fn structural_audit(&self, next: &SemIndex) -> StructuralAudit {
        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut changed = Vec::new();
        for def in &next.defs {
            match self.lookup_identity(&def.identity) {
                Some(prev) if structural_signature(prev) != structural_signature(def) => {
                    changed.push(def.identity.clone());
                }
                Some(_) => {}
                None => added.push(def.identity.clone()),
            }
        }
        for def in &self.defs {
            if next.lookup_identity(&def.identity).is_none() {
                removed.push(def.identity.clone());
            }
        }
        added.sort();
        removed.sort();
        changed.sort();
        StructuralAudit {
            added,
            removed,
            changed,
        }
    }
}

fn sort_members(members: &mut [MemberFact]) {
    members.sort_by(|a, b| {
        member_rank(&a.kind)
            .cmp(&member_rank(&b.kind))
            .then(origin_rank(&a.origin).cmp(&origin_rank(&b.origin)))
            .then(a.name.cmp(&b.name))
            .then(a.module_path.cmp(&b.module_path))
            .then(a.span.start.cmp(&b.span.start))
    });
}

fn member_rank(kind: &MemberKind) -> u8 {
    match kind {
        MemberKind::Field => 0,
        MemberKind::Variant => 1,
        MemberKind::Method => 2,
    }
}

fn origin_rank(origin: &MemberOrigin) -> u8 {
    match origin {
        MemberOrigin::TypeBody => 0,
        MemberOrigin::InherentImpl => 1,
        MemberOrigin::TraitImpl { .. } => 2,
        MemberOrigin::TraitRequirement { .. } => 3,
    }
}

fn structural_signature(def: &SymbolDef) -> String {
    match &def.kind {
        SymbolKind::Module => "module".to_string(),
        SymbolKind::Function {
            params,
            call_contract,
            ret,
        } => {
            let params = params
                .iter()
                .map(|(_, t)| t.as_str())
                .collect::<Vec<_>>()
                .join(",");
            let call_contract = call_contract
                .iter()
                .map(|(label, zone, variadic)| format!("{label}:{zone}:{variadic}"))
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "fn({params});call_contract=[{call_contract}]=>{};view_source={}",
                ret.as_deref().unwrap_or("()"),
                def.view_provenance
                    .iter()
                    .map(ViewProvenanceFact::canonical)
                    .collect::<Vec<_>>()
                    .join("|"),
            )
        }
        SymbolKind::Struct { fields } => {
            let fields = fields
                .iter()
                .map(|(n, t)| format!("{n}:{t}"))
                .collect::<Vec<_>>()
                .join(",");
            format!("struct{{{fields}}}")
        }
        SymbolKind::Enum { variants } => format!("enum{{{}}}", variants.join(",")),
        SymbolKind::Trait => "trait".to_string(),
        SymbolKind::Tag => "tag".to_string(),
        SymbolKind::Type => "type".to_string(),
        SymbolKind::Const => "const".to_string(),
        SymbolKind::EnumVariant { parent } => format!("variant:{parent}"),
        SymbolKind::Field { ty, parent } => format!("field:{parent}:{ty}"),
        SymbolKind::Local { mutable, ty } => {
            format!("local:{}:{}", mutable, ty.as_deref().unwrap_or("?"))
        }
        SymbolKind::Param { ty } => format!("param:{ty}"),
    }
}
