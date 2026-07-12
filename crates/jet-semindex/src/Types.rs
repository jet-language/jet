//! D-SEMINDEX1: stable public query types (versioned independently of LSP internals).

use jet_foundation::Diagnostics::Span;

/// Schema version for JSON snapshots and API consumers. Bump when the exported
/// fact shape changes incompatibly.
pub const SCHEMA_VERSION: u32 = 3;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDossier {
    pub schema_version: u32,
    pub target: String,
    pub definition: Option<SymbolDef>,
    pub members: Vec<MemberFact>,
    pub references: Vec<SymbolRef>,
}

/// One named definition in the program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolDef {
    pub identity: String,
    pub name: String,
    pub module_path: String,
    pub def_span: SourceSpan,
    pub kind: SymbolKind,
}

/// Immutable compiler definition anchor used by semantic refactors. Names are
/// intentionally absent: spelling can change, while module, kind, and defining
/// source span identify the declaration selected by name resolution.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DefinitionAnchor {
    pub module_path: String,
    pub kind: String,
    pub def_span: SourceSpan,
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
    ) -> Self {
        SemIndex {
            schema_version: SCHEMA_VERSION,
            defs,
            refs,
            calls,
            effects,
            members,
            nodes,
        }
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
        TypeDossier {
            schema_version: SCHEMA_VERSION,
            target: owner.to_string(),
            definition,
            members,
            references,
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
        SymbolKind::Function { params, ret } => {
            let params = params
                .iter()
                .map(|(n, t)| format!("{n}:{t}"))
                .collect::<Vec<_>>()
                .join(",");
            format!("fn({params})->{}", ret.as_deref().unwrap_or("Void"))
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
        SymbolKind::Const => "const".to_string(),
        SymbolKind::EnumVariant { parent } => format!("variant:{parent}"),
        SymbolKind::Field { ty, parent } => format!("field:{parent}:{ty}"),
        SymbolKind::Local { mutable, ty } => {
            format!("local:{}:{}", mutable, ty.as_deref().unwrap_or("?"))
        }
        SymbolKind::Param { ty } => format!("param:{ty}"),
    }
}
