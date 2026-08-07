//! Semantic index builder — walks a checked `ProgramBundle` AST.
//! D-SEMINDEX1: shared by LSP and the public `jet-semindex` query API.

use jet_foundation::Diagnostics::Span;
use jet_foundation::Syntax;
use jet_foundation::AST::{self, Item, LoadedModule, ProgramBundle};
use jet_sema::{effect_key, SemIndexEffectFacts};
use std::collections::HashMap;

use crate::JSON::{convert_defs, convert_effects, convert_refs};
use crate::Types::{BypassFact, BypassKind, CallEdge, DefinitionAnchor, DefinitionFact, InstanceApplicationFact, InstanceFact, MemberFact, MemberKind, MemberOrigin, OutputEntryFact, OutputFact, SemIndex, StructuralNode, StructuralSlotBoundary, StructuralSlotKind, SymbolDef, SymbolKind};
use crate::Symbols::{build_semantic_symbol_index, SemanticSymbolIndex};

/// The semantic kind of a defined symbol (LSP-facing; uses AST types internally).
#[derive(Debug, Clone)]
pub enum SymKind {
    Module,
    Function {
        params: Vec<(String, AST::Type)>,
        ret: Option<AST::Type>,
        effects: Option<Vec<(String, Span)>>,
        effect_via: Option<(String, Span)>,
    },
    Struct {
        fields: Vec<(String, AST::Type)>,
    },
    Enum {
        variants: Vec<String>,
    },
    Trait,
    /// D-QUAL2: a `tag` marker qualifier (no methods, erases at runtime).
    Tag,
    /// Type-like declaration without struct/enum payload (alias, distinct,
    /// unit family, state set, protocol).
    Type,
    Const,
    EnumVariant {
        parent: String,
    },
    Field {
        ty: AST::Type,
        parent: String,
    },
    Local {
        mutable: bool,
        ty: Option<AST::Type>,
    },
    Param {
        ty: AST::Type,
    },
}

/// One named definition in the program.
#[derive(Debug, Clone)]
pub struct SymDef {
    pub identity: String,
    pub name: String,
    pub def_span: Span,
    pub module_path: String,
    pub kind: SymKind,
}

/// One use-site reference (identifier occurrence).
#[derive(Debug, Clone)]
pub struct SymRef {
    pub name: String,
    pub span: Span,
    pub module_path: String,
    pub scope_identity: Option<String>,
    pub target: Option<DefinitionAnchor>,
}

/// Hover entry: an expression/token span + text to show on hover.
#[derive(Debug, Clone)]
pub struct HoverEntry {
    pub span: Span,
    pub module_path: String,
    pub text: String,
}

/// Inlay hint: position (just past the binding name) + type text to show.
#[derive(Debug, Clone)]
pub struct InlayHint {
    pub span: Span,
    pub module_path: String,
    pub label: String,
}

/// LSP symbol database — one consumer of the shared semantic-index build.
pub struct SymbolDB {
    pub index: SemIndex,
    pub defs: Vec<SymDef>,
    pub refs: Vec<SymRef>,
    pub calls: Vec<CallEdge>,
    pub members: Vec<MemberFact>,
    pub hover: Vec<HoverEntry>,
    pub inlay: Vec<InlayHint>,
    pub nodes: Vec<StructuralNode>,
    /// D-LINTPOLICY1=A: every spelled bypass (`#Unsafe`, `.drop(reason)`,
    /// `#[allow(lint)]`) collected during the walk.
    pub bypasses: Vec<BypassFact>,
    /// Sema-owned returned-view summaries keyed by semantic function identity.
    /// Kept beside `SymKind` so existing LSP/REPL consumers retain its shape.
    pub view_provenance: HashMap<String, AST::ViewProvenanceMap>,
    pub slot_boundaries: Vec<StructuralSlotBoundary>,
    pub symbols: SemanticSymbolIndex,
}

struct CallerFrame {
    name: String,
    owner: Option<String>,
    identity: String,
}

struct WalkCtx<'a> {
    db: &'a mut SymbolDB,
    caller: Option<CallerFrame>,
    scope_identity: String,
    reference_anchors: &'a HashMap<(String, usize, usize), jet_sema::DefinitionAnchorFact>,
    structural_parents: Vec<usize>,
    structural_slot: String,
    structural_slot_kind: StructuralSlotKind,
    block_spans: &'a [Span],
    claimed_block_spans: std::collections::HashSet<(usize, usize)>,
}

impl SymbolDB {
    pub fn new() -> Self {
        SymbolDB {
            index: SemIndex::new(
                Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(),
            ),
            defs: Vec::new(),
            refs: Vec::new(),
            calls: Vec::new(),
            members: Vec::new(),
            hover: Vec::new(),
            inlay: Vec::new(),
            nodes: Vec::new(),
            bypasses: Vec::new(),
            view_provenance: HashMap::new(),
            slot_boundaries: Vec::new(),
            symbols: SemanticSymbolIndex::language(),
        }
    }

    fn finalize_index(&mut self, facts: &SemIndexEffectFacts, bundle: &ProgramBundle) {
        for module in &bundle.modules {
            for item in &module.items {
                let Item::CodeModule(instance) = item else { continue };
                let Some(identity) = &instance.instance_identity else { continue };
                let semantic_identity = format!("instance:{}", identity.fingerprint);
                for application in &identity.applications {
                    if self.defs.iter().any(|definition| {
                        definition.identity == semantic_identity
                            && definition.def_span == application.span
                            && definition.module_path == application.source_module
                    }) {
                        continue;
                    }
                    self.defs.push(SymDef {
                        identity: application.semantic_identity.clone(),
                        name: application.name.clone(),
                        def_span: application.span,
                        module_path: application.source_module.clone(),
                        kind: SymKind::Module,
                    });
                }
            }
        }
        let defs = convert_defs(&self.defs, &self.view_provenance);
        let refs = convert_refs(&self.refs);
        let effects = convert_effects(facts);
        let definition_facts = build_definition_facts(&defs, &self.nodes, bundle);
        self.index = SemIndex::new(
            defs,
            refs,
            self.calls.clone(),
            effects,
            self.members.clone(),
            self.nodes.clone(),
            definition_facts,
        );
        self.index.set_bypasses(self.bypasses.clone());
        self.index.set_instances(bundle.modules.iter().flat_map(|module| module.items.iter().filter_map(|item| {
            let Item::CodeModule(cm) = item else { return None };
            let identity = cm.instance_identity.as_ref()?;
            Some(InstanceFact {
                name: cm.name.clone(), module_path: module.display.clone(),
                fingerprint: identity.fingerprint.clone(),
                full_key_hex: identity.full_key.iter().map(|byte| format!("{byte:02x}")).collect(),
                template_definition_id: identity.definition_id.clone(),
                template_span: identity.template_span.into(),
                arguments: identity.argument_keys.iter().map(|key| key.iter().map(|byte| format!("{byte:02x}")).collect()).collect(),
                applications: identity.applications.iter().map(|application| InstanceApplicationFact {
                    name: application.name.clone(),
                    module_path: application.source_module.clone(),
                    semantic_identity: application.semantic_identity.clone(),
                    span: application.span.into(),
                }).collect(),
                exported_members: cm.body.as_deref().unwrap_or_default().iter().filter_map(|item| match item {
                    Item::Func(def) if def.is_pub || def.is_package_pub => Some(def.name.clone()),
                    Item::Struct(def) if def.is_pub || def.is_package_pub => Some(def.name.clone()),
                    Item::Enum(def) if def.is_pub || def.is_package_pub => Some(def.name.clone()),
                    Item::Trait(def) if def.is_pub || def.is_package_pub => Some(def.name.clone()),
                    Item::Tag(def) if def.is_pub || def.is_package_pub => Some(def.name.clone()),
                    _ => None,
                }).collect(),
            })
        })).collect());
        self.index.set_outputs(bundle.modules.iter().flat_map(|module| {
            module.items.iter().filter_map(|item| {
                let Item::Const(value) = item else { return None };
                let output = value.resolved_output.as_ref()?;
                let target = &bundle.modules[output.module];
                Some(OutputFact {
                    binding: value.name.clone(),
                    kind: output.kind.as_str().to_string(),
                    name: output.output_name.clone(),
                    module_path: module.display.clone(),
                    span: value.span.into(),
                    entry: OutputEntryFact {
                        identity: format!("{}::{}", target.alias, output.semantic_name),
                        name: output.source_name.clone(),
                        module_path: output.source_path.clone(),
                        definition_span: output.definition.into(),
                        reference_span: output.reference.into(),
                        params: output.params.iter().map(|(_, ty)| ty.name()).collect(),
                        return_type: output.return_type.as_ref().map(AST::Type::name),
                        authority: output.authority.as_str().to_string(),
                        effects: output.effects.clone(),
                    },
                })
            })
        }).collect());
    }

    /// Hover text for the symbol at `offset` in `path`.
    pub fn hover_at(&self, path: &str, offset: usize) -> Option<&str> {
        self.hover
            .iter()
            .find(|h| h.module_path == path && h.span.start <= offset && offset <= h.span.end)
            .map(|h| h.text.as_str())
    }

    /// All inlay hints for a module path.
    pub fn inlay_hints_for(&self, path: &str) -> Vec<&InlayHint> {
        self.inlay
            .iter()
            .filter(|h| h.module_path == path)
            .collect()
    }
}

fn caller_key(frame: &CallerFrame) -> String {
    effect_key(frame.owner.as_deref(), &frame.name)
}

fn root_identity(mp: &str) -> String {
    format!("module:{mp}")
}

fn module_identity(scope: &str, name: &str) -> String {
    format!("module:{scope}::{name}")
}

fn type_identity(scope: &str, name: &str) -> String {
    format!("type:{scope}::{name}")
}

fn member_identity(scope: &str, parent: &str, kind: &str, name: &str) -> String {
    format!("{kind}:{scope}::{parent}.{name}")
}

fn callable_identity(scope: &str, owner: Option<&str>, name: &str, _f: &AST::Func) -> String {
    match owner {
        Some(owner) => format!("method:{scope}::{owner}.{name}"),
        None => format!("fn:{scope}::{name}"),
    }
}

fn trait_method_identity(scope: &str, owner: &str, sig: &AST::TraitMethodSig) -> String {
    format!("method:{scope}::{owner}.{}", sig.name)
}

fn active_scope<'a, 'b>(ctx: &'a WalkCtx<'b>) -> &'a str {
    ctx.caller
        .as_ref()
        .map(|c| c.identity.as_str())
        .unwrap_or(&ctx.scope_identity)
}

fn local_identity(scope: &str, kind: &str, name: &str) -> String {
    format!("{kind}:{scope}::{name}")
}

fn fn_signature(
    name: &str,
    params: &[(String, AST::Type)],
    ret: &Option<AST::Type>,
    effects: Option<&Vec<(String, Span)>>,
    effect_via: Option<&(String, Span)>,
    view_provenance: Option<&AST::ViewProvenanceMap>,
) -> String {
    let params = params
        .iter()
        .map(|(n, t)| format!("{n}: {}", t.name()))
        .collect::<Vec<_>>()
        .join(", ");
    let arrow = if let Some((param, _)) = effect_via {
        format!(" =[via {param}]=>")
    } else {
        effects.map_or_else(
            || ret.as_ref().map(|_| " =>".to_string()).unwrap_or_default(),
            |row| format!(" =[{}]=>", row.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(", ")),
        )
    };
    let result = ret.as_ref().map(|ty| format!(" {}", ty.name())).unwrap_or_default();
    let mut signature = format!("fn {name}({params}){arrow}{result}");
    if let Some(map) = view_provenance {
        if let Some(direct) = map.get(&Vec::<String>::new()).filter(|_| map.len() == 1) {
            signature.push_str(" ; view_source = ");
            signature.push_str(&direct.canonical());
        } else {
            signature.push_str(" ; view_sources = ");
            signature.push_str(&AST::canonical_view_provenance_map(map));
        }
    }
    signature
}

fn method_params(f: &AST::Func) -> Vec<(String, AST::Type)> {
    f.params
        .iter()
        .filter(|p| p.name != Syntax::KW_SELF)
        .map(|p| (p.name.clone(), p.ty.clone()))
        .collect()
}

fn method_fact(
    scope: &str,
    owner: &str,
    f: &AST::Func,
    origin: MemberOrigin,
    mp: &str,
) -> MemberFact {
    let params = method_params(f);
    MemberFact {
        owner: owner.to_string(),
        name: f.name.clone(),
        identity: callable_identity(scope, Some(owner), &f.name, f),
        kind: MemberKind::Method,
        origin,
        signature: fn_signature(
            &f.name,
            &params,
            &f.return_type,
            f.declared_effects.as_ref(),
            f.effect_via.as_ref(),
            f.return_view_provenance.as_ref(),
        ),
        module_path: mp.to_string(),
        span: f.name_span.into(),
    }
}

fn origin_label(origin: &MemberOrigin) -> String {
    match origin {
        MemberOrigin::TypeBody => "type".to_string(),
        MemberOrigin::InherentImpl => "impl".to_string(),
        MemberOrigin::TraitImpl { trait_name } => format!("impl {trait_name}"),
        MemberOrigin::TraitRequirement { trait_name } => format!("trait {trait_name}"),
    }
}

fn scoped_local_identity(ctx: &WalkCtx<'_>, kind: &str, name: &str) -> String {
    local_identity(active_scope(ctx), kind, name)
}

fn scoped_ref(name: String, span: Span, mp: &str, ctx: &WalkCtx<'_>) -> SymRef {
    let target = ctx.reference_anchors
        .get(&(mp.to_string(), span.start, span.end))
        .map(|fact| DefinitionAnchor {
            module_path: fact.module_path.clone(),
            kind: fact.kind.clone(),
            def_span: fact.def_span.into(),
            semantic_identity: fact.semantic_identity.clone(),
        });
    SymRef {
        name,
        span,
        module_path: mp.to_string(),
        scope_identity: Some(active_scope(ctx).to_string()),
        target,
    }
}

fn record_node(ctx: &mut WalkCtx<'_>, class: &str, shape: &str, mp: &str, span: Span) -> Option<usize> {
    if span.end >= span.start {
        let id = ctx.db.nodes.len();
        let parent = ctx.structural_parents.last().copied();
        assert!(
            parent.is_none() || ctx.structural_slot != "root",
            "compiler structural child missing explicit slot: {class}/{shape}"
        );
        let ordinal = ctx.db.nodes.iter().filter(|node| {
            node.parent == parent && node.slot == ctx.structural_slot
        }).count();
        ctx.db.nodes.push(StructuralNode {
            id,
            parent,
            slot: ctx.structural_slot.clone(),
            slot_kind: ctx.structural_slot_kind,
            ordinal,
            class: class.to_string(),
            shape: shape.to_string(),
            module_path: mp.to_string(),
            span: span.into(),
        });
        Some(id)
    } else {
        None
    }
}

fn structural_slot<T>(
    ctx: &mut WalkCtx<'_>,
    name: &str,
    kind: StructuralSlotKind,
    f: impl FnOnce(&mut WalkCtx<'_>) -> T,
) -> T {
    if is_lexical_structural_slot(name) {
        if let Some(parent) = ctx.structural_parents.last().copied() {
            let parent_start = ctx.db.nodes[parent].span.start;
            let parent_end = ctx.db.nodes[parent].span.end;
            let exact_parent_end = ctx.db.nodes[parent].class == "arm";
            if let Some(span) = ctx
                .block_spans
                .iter()
                .filter(|span| {
                    parent_start <= span.start
                        // Classic `if` lowers to a private arm whose AST span
                        // ends at the condition. Its body still owns the next
                        // parser block span; keep the exact upper bound for
                        // real arm spans, but allow this head-only shape.
                        && (!exact_parent_end
                            || span.end <= parent_end
                            || parent_end <= span.start)
                        && !ctx.claimed_block_spans.contains(&(span.start, span.end))
                })
                .min_by_key(|span| span.start)
                .copied()
            {
                ctx.claimed_block_spans.insert((span.start, span.end));
                ctx.db.slot_boundaries.push(StructuralSlotBoundary {
                    parent,
                    slot: name.to_string(),
                    module_path: ctx.db.nodes[parent].module_path.clone(),
                    span: span.into(),
                });
            }
        }
    }
    let old_name = std::mem::replace(&mut ctx.structural_slot, name.to_string());
    let old_kind = std::mem::replace(&mut ctx.structural_slot_kind, kind);
    let value = f(ctx);
    ctx.structural_slot = old_name;
    ctx.structural_slot_kind = old_kind;
    value
}

fn is_lexical_structural_slot(slot: &str) -> bool {
    slot == "body" || slot.ends_with("_body") || slot.ends_with("_bodies")
}

fn expr_shape(expr: &AST::Expr) -> String {
    format!("{:?}", std::mem::discriminant(expr))
}

fn structural_expr_span(expr: &AST::Expr) -> Span {
    match expr {
        AST::Expr::Call(call) => Span::new(
            call.name_span.start,
            call.args
                .last()
                .map_or(call.name_span.end.saturating_add(2), |arg| {
                    structural_expr_span(&arg.expr).end.saturating_add(1)
                }),
        ),
        AST::Expr::MethodCall {
            receiver,
            method_span,
            args,
            ..
        } => Span::new(
            structural_expr_span(receiver).start,
            args.last()
                .map_or(method_span.end.saturating_add(2), |arg| {
                    structural_expr_span(&arg.expr).end.saturating_add(1)
                }),
        ),
        _ => expr.span(),
    }
}

fn stmt_shape(stmt: &AST::Stmt) -> String {
    format!("{:?}", std::mem::discriminant(stmt))
}

fn item_shape(item: &AST::Item) -> String {
    format!("{:?}", std::mem::discriminant(item))
}

fn item_span(item: &AST::Item) -> Span {
    match item {
        AST::Item::EffectDecl(x) => x.span,
        AST::Item::Func(x) => x.span,
        AST::Item::Struct(x) => x.span,
        AST::Item::Enum(x) => x.span,
        AST::Item::Distinct(x) => x.span,
        AST::Item::TypeAlias(x) => x.span,
        AST::Item::UnitFamily(x) => x.span,
        AST::Item::Trait(x) => x.span,
        AST::Item::Tag(x) => x.span,
        AST::Item::Impl(x) => x.span,
        AST::Item::Const(x) => x.span,
        AST::Item::Test(x) => x.span,
        AST::Item::Bench(x) => x.span,
        AST::Item::ExternRust(x) => x.span,
        AST::Item::Module(x) => x.span,
        AST::Item::CModule(x) => x.span,
        AST::Item::CodeModule(x) => x.span,
        AST::Item::ErrorConv(x) => x.from_span,
        AST::Item::Migration(x) => x.span,
        AST::Item::StateDecl(x) => x.span,
        AST::Item::ProtocolDecl(x) => x.span,
        AST::Item::UserDerive(x) => x.span,
        AST::Item::GenericModule(x) => x.span,
        AST::Item::ModuleAlias(x) => x.span,
    }
}

fn record_func_type_nodes(f: &AST::Func, mp: &str, ctx: &mut WalkCtx<'_>) {
    structural_slot(ctx, "params", StructuralSlotKind::List, |ctx| {
        for param in &f.params { record_node(ctx, "type", "type", mp, param.ty_span); }
    });
    if let Some(span) = f.return_type_span {
        structural_slot(ctx, "return_type", StructuralSlotKind::Scalar, |ctx| {
            record_node(ctx, "type", "type", mp, span);
        });
    }
}

/// D-LINTPOLICY1=A: record one `BypassFact` per lint name inside an
/// `#[allow(lint, …)]` marker (D-DECIMAL1 and kin) — any marker whose
/// `name` is `"allow"`, wherever it appears (struct or field).
fn collect_allow_markers(markers: &[AST::Marker], site: &str, mp: &str, ctx: &mut WalkCtx<'_>) {
    for marker in markers {
        if marker.name != "allow" {
            continue;
        }
        for arg in &marker.args {
            if let AST::Expr::Ident(lint_name, _) = arg {
                ctx.db.bypasses.push(BypassFact {
                    kind: BypassKind::LintAllow,
                    site: site.to_string(),
                    detail: lint_name.clone(),
                    module_path: mp.to_string(),
                    span: marker.span.into(),
                });
            }
        }
    }
}

fn record_call(ctx: &mut WalkCtx<'_>, mp: &str, callee: &str, span: Span) {
    if let Some(caller) = &ctx.caller {
        ctx.db.calls.push(CallEdge {
            caller: caller_key(caller),
            callee: callee.to_string(),
            module_path: mp.to_string(),
            call_span: span.into(),
        });
    }
}

fn with_caller<F>(
    ctx: &mut WalkCtx<'_>,
    frame: CallerFrame,
    params: &[AST::Param],
    mp: &str,
    f: F,
)
where
    F: FnOnce(&mut WalkCtx<'_>),
{
    let prev = ctx.caller.replace(frame);
    let _ = (params, mp);
    f(ctx);
    ctx.caller = prev;
}

/// Build the shared symbol index for a checked bundle (LSP + public API).
pub fn build_symbol_db(bundle: &ProgramBundle, facts: &SemIndexEffectFacts) -> SymbolDB {
    let mut db = SymbolDB::new();
    for module in &bundle.modules {
        let mp = module.display.clone();
        let mut ctx = WalkCtx {
            db: &mut db,
            caller: None,
            scope_identity: root_identity(&mp),
            reference_anchors: &facts.reference_anchors,
            structural_parents: Vec::new(),
            structural_slot: "root".to_string(),
            structural_slot_kind: StructuralSlotKind::List,
            block_spans: &module.block_spans,
            claimed_block_spans: std::collections::HashSet::new(),
        };
        for item in &module.items {
            collect_item(item, &mp, module, &mut ctx);
        }
        apply_inferred_effect_rows(&mut db, module, facts);
    }
    add_breadcrumb_hints(&mut db);
    db.finalize_index(facts, bundle);
    db.symbols = build_semantic_symbol_index(&db, bundle);
    db
}

fn apply_inferred_effect_rows(
    db: &mut SymbolDB,
    module: &LoadedModule,
    facts: &SemIndexEffectFacts,
) {
    let mut inline_keys = HashMap::new();
    for item in &module.items {
        let Item::CodeModule(code_module) = item else { continue };
        let Some(body) = &code_module.body else { continue };
        for item in body {
            if let Item::Func(function) = item {
                inline_keys.insert(
                    (function.name_span.start, function.name_span.end),
                    format!("{}__{}", code_module.name, function.name),
                );
            }
        }
    }
    let module_prefix = format!("{}::", module.alias);
    for def in db
        .defs
        .iter_mut()
        .filter(|def| def.module_path == module.display)
    {
        let SymKind::Function {
            params,
            ret,
            effects,
            effect_via,
        } = &mut def.kind
        else {
            continue;
        };
        if effects.is_some() || effect_via.is_some() {
            continue;
        }
        let local_key = if let Some(key) = inline_keys.get(&(def.def_span.start, def.def_span.end)) {
            key.clone()
        } else if def.identity.starts_with("method:") {
            def.identity
                .rsplit("::")
                .next()
                .and_then(|tail| tail.strip_suffix(&format!(".{}", def.name)))
                .map(|owner| format!("{owner}::{}", def.name))
                .unwrap_or_else(|| def.name.clone())
        } else {
            def.name.clone()
        };
        let Some(row) = facts.solved.get(&format!("{module_prefix}{local_key}")) else {
            continue;
        };
        *effects = Some(
            row.iter()
                .cloned()
                .map(|effect| (effect, def.def_span))
                .collect(),
        );
        let signature = fn_signature(
            &def.name,
            params,
            ret,
            effects.as_ref(),
            None,
            db.view_provenance.get(&def.identity),
        );
        if let Some(hover) = db.hover.iter_mut().find(|hover| {
            hover.module_path == def.module_path && hover.span == def.def_span
        }) {
            let tail = hover
                .text
                .split_once('\n')
                .map(|(_, tail)| format!("\n{tail}"))
                .unwrap_or_default();
            hover.text = format!("{signature}{tail}");
        }
        for member in db
            .members
            .iter_mut()
            .filter(|member| member.identity == def.identity)
        {
            member.signature = signature.clone();
        }
    }
}

fn build_definition_facts(
    defs: &[SymbolDef],
    nodes: &[StructuralNode],
    bundle: &ProgramBundle,
) -> Vec<DefinitionFact> {
    let mut out = Vec::new();
    for node in nodes.iter().filter(|node| node.parent.is_none() && node.class == "item") {
        let Some(module) = bundle.modules.iter().find(|module| module.display == node.module_path) else { continue };
        let Some(def) = defs.iter().filter(|def| {
            def.module_path == node.module_path
                && node.span.start <= def.def_span.start
                && def.def_span.end <= node.span.end
                && !matches!(def.kind, SymbolKind::Local { .. } | SymbolKind::Param { .. } | SymbolKind::Field { .. } | SymbolKind::EnumVariant { .. })
        }).min_by_key(|def| def.def_span.start) else { continue };
        // Definition IDs identify a checked declaration, never merely its
        // ancestry class. `def.identity` is the compiler's semantic identity
        // (module scope + declared name), so same-kind siblings cannot collapse
        // onto one ID. Rename/move pairing remains a separate, explicit merge
        // operation; it must not be smuggled into the identity hash.
        let structural = format!("{}|{}", definition_kind(&def.kind), def.identity);
        let source = module.source.get(node.span.start..node.span.end).unwrap_or("");
        out.push(DefinitionFact {
            stable_id: format!("def:{}", &jet_foundation::SHA256::sha256_hex(structural.as_bytes())[..16]),
            signature_id: format!("sig:{}", &jet_foundation::SHA256::sha256_hex(format!("{}|{}", definition_kind(&def.kind), definition_signature(def)).as_bytes())[..16]),
            content_id: format!("sha256:{}", jet_foundation::SHA256::sha256_hex(normalize_definition(source).as_bytes())),
            human_identity: def.identity.clone(),
            name: def.name.clone(),
            kind: definition_kind(&def.kind).to_string(),
            module_path: def.module_path.clone(),
            span: node.span,
        });
    }
    out.sort_by(|a, b| a.stable_id.cmp(&b.stable_id).then(a.human_identity.cmp(&b.human_identity)));
    out
}

fn definition_kind(kind: &SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Module => "module",
        SymbolKind::Function { .. } => "function",
        SymbolKind::Struct { .. } => "struct",
        SymbolKind::Enum { .. } => "enum",
        SymbolKind::Trait => "trait",
        SymbolKind::Tag => "tag",
        SymbolKind::Type => "type",
        SymbolKind::Const => "const",
        SymbolKind::EnumVariant { .. } => "variant",
        SymbolKind::Field { .. } => "field",
        SymbolKind::Local { .. } => "local",
        SymbolKind::Param { .. } => "param",
    }
}

fn definition_signature(def: &SymbolDef) -> String {
    match &def.kind {
        SymbolKind::Function { params, ret } => format!(
            "({})->{};view_source={}",
            params.iter().map(|(_, ty)| ty.as_str()).collect::<Vec<_>>().join(","),
            ret.as_deref().unwrap_or("()"),
            def.view_provenance
                .iter()
                .map(|provenance| provenance.canonical())
                .collect::<Vec<_>>()
                .join("|"),
        ),
        SymbolKind::Struct { fields } => format!("{{{}}}", fields.iter().map(|(_, ty)| ty.as_str()).collect::<Vec<_>>().join(",")),
        SymbolKind::Enum { variants } => format!("variants:{}", variants.len()),
        _ => definition_kind(&def.kind).to_string(),
    }
}

fn normalize_definition(source: &str) -> String {
    let mut out = String::new();
    let mut chars = source.chars().peekable();
    let mut string = false;
    while let Some(ch) = chars.next() {
        if string {
            out.push(ch);
            if ch == '\\' { if let Some(next) = chars.next() { out.push(next); } }
            else if ch == '"' { string = false; }
        } else if ch == '"' {
            string = true;
            out.push(ch);
        } else if ch == '/' && chars.peek() == Some(&'/') {
            chars.next();
            for next in chars.by_ref() { if next == '\n' { break; } }
        } else if !ch.is_whitespace() {
            out.push(ch);
        }
    }
    out
}

/// Compiler-owned typed AST boundaries for an already parsed module. Codemod
/// patterns use this same walker as checked semantic indexes, so fragment
/// parsing and candidate matching cannot invent a parallel syntax tree.
pub fn structural_nodes_from_parsed(module: &LoadedModule) -> Vec<StructuralNode> {
    let mp = module.display.clone();
    let mut db = SymbolDB::new();
    let reference_anchors = HashMap::new();
    let mut ctx = WalkCtx {
        db: &mut db,
        caller: None,
        scope_identity: root_identity(&mp),
        reference_anchors: &reference_anchors,
        structural_parents: Vec::new(),
        structural_slot: "root".to_string(),
        structural_slot_kind: StructuralSlotKind::List,
        block_spans: &module.block_spans,
        claimed_block_spans: std::collections::HashSet::new(),
    };
    for item in &module.items {
        collect_item(item, &mp, module, &mut ctx);
    }
    db.nodes
}

fn add_breadcrumb_hints(db: &mut SymbolDB) {
    let type_defs: Vec<SymDef> = db
        .defs
        .iter()
        .filter(|d| matches!(d.kind, SymKind::Struct { .. } | SymKind::Enum { .. }))
        .cloned()
        .collect();
    for def in type_defs {
        let mut remote: Vec<MemberFact> = db
            .members
            .iter()
            .filter(|m| {
                m.owner == def.name
                    && m.kind == MemberKind::Method
                    && matches!(
                        m.origin,
                        MemberOrigin::InherentImpl | MemberOrigin::TraitImpl { .. }
                    )
            })
            .cloned()
            .collect();
        remote.sort_by(|a, b| {
            origin_label(&a.origin)
                .cmp(&origin_label(&b.origin))
                .then(a.name.cmp(&b.name))
                .then(a.module_path.cmp(&b.module_path))
                .then(a.span.start.cmp(&b.span.start))
        });
        if remote.is_empty() {
            continue;
        }
        let label = remote
            .iter()
            .map(|m| {
                format!(
                    "+ {} [{}:{}..{}]",
                    m.signature, m.module_path, m.span.start, m.span.end
                )
            })
            .collect::<Vec<_>>()
            .join(" | ");
        db.inlay.push(InlayHint {
            span: def.def_span,
            module_path: def.module_path,
            label,
        });
    }
}

/// Public query surface over the same facts (no LSP hover/inlay extras).
pub fn build_index(bundle: &ProgramBundle, facts: &SemIndexEffectFacts) -> SemIndex {
    build_symbol_db(bundle, facts).index
}

fn collect_item(item: &Item, mp: &str, module: &LoadedModule, ctx: &mut WalkCtx<'_>) {
    let structural_id = record_node(
        ctx,
        "item",
        &item_shape(item),
        mp,
        item_span(item),
    );
    if let Some(id) = structural_id { ctx.structural_parents.push(id); }
    match item {
        Item::EffectDecl(_) => {}
        Item::Func(f) => {
            record_func_type_nodes(f, mp, ctx);
            let fn_identity = callable_identity(&ctx.scope_identity, None, &f.name, f);
            if let Some(provenance) = &f.return_view_provenance {
                ctx.db
                    .view_provenance
                    .insert(fn_identity.clone(), provenance.clone());
            }
            let params: Vec<(String, AST::Type)> = method_params(f);
            let sym = SymDef {
                identity: fn_identity.clone(),
                name: f.name.clone(),
                def_span: f.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Function {
                    params: params.clone(),
                    ret: f.return_type.clone(),
                    effects: f.declared_effects.clone(),
                    effect_via: f.effect_via.clone(),
                },
            };
            let mut hover_text = hover_for_fn(f);
            for (active, name) in [(f.is_unsafe, Syntax::KW_UNSAFE), (f.is_pure, Syntax::KW_PURE), (f.is_replayable, Syntax::MARKER_REPLAYABLE)] {
                if active && jet_foundation::Policy::rule_allows(name, jet_foundation::Policy::RuleSite::Function) {
                    hover_text.push_str(&format!("\nrule: #{name} (function, site-bound)"));
                }
            }
            if let Some(tag) = &f.scrub_tag {
                hover_text.push_str(&format!("\nrule: #Scrub({tag}) (function, site-bound)"));
            }
            let declarations = module.policy_declarations.iter().filter(|d| matches!(d.scope, jet_foundation::Policy::PolicyScope::Organization | jet_foundation::Policy::PolicyScope::Package | jet_foundation::Policy::PolicyScope::Module) || (d.scope == jet_foundation::Policy::PolicyScope::Function && d.target == Some(f.span))).cloned().collect::<Vec<_>>();
            for key in [jet_foundation::Policy::PolicyKey::NoAlloc, jet_foundation::Policy::PolicyKey::ZeroRc, jet_foundation::Policy::PolicyKey::ArenaBounded, jet_foundation::Policy::PolicyKey::Unsafe, jet_foundation::Policy::PolicyKey::ScopedGc] {
                if let Ok(Some(effective)) = jet_foundation::Policy::resolve(key, declarations.clone()) {
                    hover_text.push_str("\npolicy: ");
                    hover_text.push_str(&jet_foundation::Policy::explain(&effective));
                }
            }
            ctx.db.hover.push(HoverEntry {
                span: f.name_span,
                module_path: mp.to_string(),
                text: hover_text,
            });
            ctx.db.defs.push(sym);
            // D-LINTPOLICY1=A: `#Unsafe("reason") fn …` is a spelled
            // whole-function bypass, distinct from an in-body `#Unsafe`
            // region.
            if f.is_unsafe {
                ctx.db.bypasses.push(BypassFact {
                    kind: BypassKind::UnsafeFn,
                    site: f.name.clone(),
                    detail: f.unsafe_reason.clone().unwrap_or_default(),
                    module_path: mp.to_string(),
                    span: f.unsafe_span.unwrap_or(f.name_span).into(),
                });
            }
            // param defs
            for p in &f.params {
                if p.name == Syntax::KW_SELF {
                    continue;
                }
                ctx.db.defs.push(SymDef {
                    identity: local_identity(&fn_identity, "param", &p.name),
                    name: p.name.clone(),
                    def_span: p.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Param { ty: p.ty.clone() },
                });
                ctx.db.hover.push(HoverEntry {
                    span: p.name_span,
                    module_path: mp.to_string(),
                    // D-APILABEL1=A: name the public label when it differs, so
                    // hovering the local name still tells you what to type.
                    text: match &p.public_label {
                        Some((label, _)) => format!(
                            "`{}`: {} — callers write `{label}:`",
                            p.name,
                            p.ty.name()
                        ),
                        None => format!("`{}`: {}", p.name, p.ty.name()),
                    },
                });
            }
            with_caller(
                ctx,
                CallerFrame {
                    name: f.name.clone(),
                    owner: None,
                    identity: fn_identity,
                },
                &f.params,
                mp,
                |ctx| {
                    structural_slot(ctx, "body", StructuralSlotKind::List, |ctx| collect_stmts(&f.body, mp, module, ctx));
                },
            );
        }
        Item::Struct(s) => {
            structural_slot(ctx, "field_types", StructuralSlotKind::List, |ctx| {
                for field in &s.fields { record_node(ctx, "type", "type", mp, field.ty_span); }
            });
            // D-LINTPOLICY1=A: `#[allow(lint)]` (D-DECIMAL1 and kin) is a
            // spelled source-level lint suppression — the struct itself, and
            // each field, may carry one.
            collect_allow_markers(&s.serde_markers, &s.name, mp, ctx);
            for field in &s.fields {
                collect_allow_markers(&field.serde_markers, &field.name, mp, ctx);
            }
            for field in &s.fields {
                if let Some(computed) = &field.computed {
                    structural_slot(ctx, "computed_fields", StructuralSlotKind::List, |ctx| collect_expr(computed, mp, ctx));
                }
            }
            let fields: Vec<(String, AST::Type)> = s
                .fields
                .iter()
                .map(|f| (f.name.clone(), f.ty.clone()))
                .collect();
            ctx.db.defs.push(SymDef {
                identity: type_identity(&ctx.scope_identity, &s.name),
                name: s.name.clone(),
                def_span: s.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Struct {
                    fields: fields.clone(),
                },
            });
            ctx.db.hover.push(HoverEntry {
                span: s.name_span,
                module_path: mp.to_string(),
                text: format!("struct `{}`", s.name),
            });
            for f in &s.fields {
                ctx.db.defs.push(SymDef {
                    identity: member_identity(&ctx.scope_identity, &s.name, "field", &f.name),
                    name: f.name.clone(),
                    def_span: f.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Field {
                        ty: f.ty.clone(),
                        parent: s.name.clone(),
                    },
                });
                ctx.db.members.push(MemberFact {
                    owner: s.name.clone(),
                    name: f.name.clone(),
                    identity: member_identity(&ctx.scope_identity, &s.name, "field", &f.name),
                    kind: MemberKind::Field,
                    origin: MemberOrigin::TypeBody,
                    signature: format!("{}: {}", f.name, f.ty.name()),
                    module_path: mp.to_string(),
                    span: f.name_span.into(),
                });
                ctx.db.hover.push(HoverEntry {
                    span: f.name_span,
                    module_path: mp.to_string(),
                    text: format!("`{}`: {} (field of `{}`)", f.name, f.ty.name(), s.name),
                });
            }
            for meth in &s.methods {
                record_func_type_nodes(meth, mp, ctx);
                let method_identity =
                    callable_identity(&ctx.scope_identity, Some(&s.name), &meth.name, meth);
                let hover_text = hover_for_fn(meth);
                ctx.db.members.push(method_fact(
                    &ctx.scope_identity,
                    &s.name,
                    meth,
                    MemberOrigin::TypeBody,
                    mp,
                ));
                ctx.db.defs.push(SymDef {
                    identity: method_identity.clone(),
                    name: meth.name.clone(),
                    def_span: meth.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Function {
                        params: meth
                            .params
                            .iter()
                            .filter(|p| p.name != Syntax::KW_SELF)
                            .map(|p| (p.name.clone(), p.ty.clone()))
                            .collect(),
                        ret: meth.return_type.clone(),
                        effects: meth.declared_effects.clone(),
                        effect_via: meth.effect_via.clone(),
                    },
                });
                ctx.db.hover.push(HoverEntry {
                    span: meth.name_span,
                    module_path: mp.to_string(),
                    text: hover_text,
                });
                for p in &meth.params {
                    if p.name != Syntax::KW_SELF {
                        ctx.db.defs.push(SymDef {
                            identity: local_identity(&method_identity, "param", &p.name),
                            name: p.name.clone(),
                            def_span: p.name_span,
                            module_path: mp.to_string(),
                            kind: SymKind::Param { ty: p.ty.clone() },
                        });
                    }
                }
                let owner = s.name.clone();
                let meth_name = meth.name.clone();
                let body = &meth.body;
                with_caller(
                    ctx,
                    CallerFrame {
                        name: meth_name,
                        owner: Some(owner.clone()),
                        identity: method_identity,
                    },
                    &meth.params,
                    mp,
                    |ctx| structural_slot(ctx, "method_bodies", StructuralSlotKind::List, |ctx| collect_stmts(body, mp, module, ctx)),
                );
            }
            for tb in &s.trait_impls {
                for meth in &tb.methods {
                    record_func_type_nodes(meth, mp, ctx);
                    let method_identity =
                        callable_identity(&ctx.scope_identity, Some(&s.name), &meth.name, meth);
                    ctx.db.members.push(method_fact(
                        &ctx.scope_identity,
                        &s.name,
                        meth,
                        MemberOrigin::TraitImpl {
                            trait_name: tb.trait_name.clone(),
                        },
                        mp,
                    ));
                    ctx.db.defs.push(SymDef {
                        identity: method_identity.clone(),
                        name: meth.name.clone(),
                        def_span: meth.name_span,
                        module_path: mp.to_string(),
                        kind: SymKind::Function {
                            params: method_params(meth),
                            ret: meth.return_type.clone(),
                            effects: meth.declared_effects.clone(),
                            effect_via: meth.effect_via.clone(),
                        },
                    });
                    with_caller(
                        ctx,
                        CallerFrame {
                            name: meth.name.clone(),
                            owner: Some(s.name.clone()),
                            identity: method_identity,
                        },
                        &meth.params,
                        mp,
                        |ctx| structural_slot(ctx, "method_bodies", StructuralSlotKind::List, |ctx| collect_stmts(&meth.body, mp, module, ctx)),
                    );
                }
            }
        }
        Item::Enum(e) => {
            for variant in &e.variants {
                match &variant.payload {
                    AST::VariantPayload::Unit => {}
                    AST::VariantPayload::Single(_, span) => {
                        structural_slot(ctx, "variant_types", StructuralSlotKind::List, |ctx| { record_node(ctx, "type", "type", mp, *span); });
                    }
                    AST::VariantPayload::Named(fields) => {
                        structural_slot(ctx, "variant_types", StructuralSlotKind::List, |ctx| {
                            for field in fields { record_node(ctx, "type", "type", mp, field.ty_span); }
                        });
                    }
                }
            }
            let variants: Vec<String> = e.variants.iter().map(|v| v.name.clone()).collect();
            ctx.db.defs.push(SymDef {
                identity: type_identity(&ctx.scope_identity, &e.name),
                name: e.name.clone(),
                def_span: e.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Enum {
                    variants: variants.clone(),
                },
            });
            ctx.db.hover.push(HoverEntry {
                span: e.name_span,
                module_path: mp.to_string(),
                text: format!("enum `{}` — variants: {}", e.name, variants.join(", ")),
            });
            for v in &e.variants {
                let identity = member_identity(&ctx.scope_identity, &e.name, "variant", &v.name);
                ctx.db.defs.push(SymDef {
                    identity: identity.clone(),
                    name: v.name.clone(),
                    def_span: v.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::EnumVariant {
                        parent: e.name.clone(),
                    },
                });
                ctx.db.members.push(MemberFact {
                    owner: e.name.clone(),
                    name: v.name.clone(),
                    identity,
                    kind: MemberKind::Variant,
                    origin: MemberOrigin::TypeBody,
                    signature: v.name.clone(),
                    module_path: mp.to_string(),
                    span: v.name_span.into(),
                });
                ctx.db.hover.push(HoverEntry {
                    span: v.name_span,
                    module_path: mp.to_string(),
                    text: format!("`{}` — variant of `{}`", v.name, e.name),
                });
            }
            for meth in &e.methods {
                record_func_type_nodes(meth, mp, ctx);
                let method_identity =
                    callable_identity(&ctx.scope_identity, Some(&e.name), &meth.name, meth);
                ctx.db.members.push(method_fact(
                    &ctx.scope_identity,
                    &e.name,
                    meth,
                    MemberOrigin::TypeBody,
                    mp,
                ));
                ctx.db.defs.push(SymDef {
                    identity: method_identity.clone(),
                    name: meth.name.clone(),
                    def_span: meth.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Function {
                        params: meth
                            .params
                            .iter()
                            .filter(|p| p.name != Syntax::KW_SELF)
                            .map(|p| (p.name.clone(), p.ty.clone()))
                            .collect(),
                        ret: meth.return_type.clone(),
                        effects: meth.declared_effects.clone(),
                        effect_via: meth.effect_via.clone(),
                    },
                });
                with_caller(
                    ctx,
                    CallerFrame {
                        name: meth.name.clone(),
                        owner: Some(e.name.clone()),
                        identity: method_identity,
                    },
                    &meth.params,
                    mp,
                    |ctx| structural_slot(ctx, "method_bodies", StructuralSlotKind::List, |ctx| collect_stmts(&meth.body, mp, module, ctx)),
                );
            }
            for tb in &e.trait_impls {
                for meth in &tb.methods {
                    record_func_type_nodes(meth, mp, ctx);
                    let method_identity =
                        callable_identity(&ctx.scope_identity, Some(&e.name), &meth.name, meth);
                    ctx.db.members.push(method_fact(
                        &ctx.scope_identity,
                        &e.name,
                        meth,
                        MemberOrigin::TraitImpl {
                            trait_name: tb.trait_name.clone(),
                        },
                        mp,
                    ));
                    ctx.db.defs.push(SymDef {
                        identity: method_identity.clone(),
                        name: meth.name.clone(),
                        def_span: meth.name_span,
                        module_path: mp.to_string(),
                        kind: SymKind::Function {
                            params: method_params(meth),
                            ret: meth.return_type.clone(),
                            effects: meth.declared_effects.clone(),
                            effect_via: meth.effect_via.clone(),
                        },
                    });
                    with_caller(
                        ctx,
                        CallerFrame {
                            name: meth.name.clone(),
                            owner: Some(e.name.clone()),
                            identity: method_identity,
                        },
                        &meth.params,
                        mp,
                        |ctx| structural_slot(ctx, "method_bodies", StructuralSlotKind::List, |ctx| collect_stmts(&meth.body, mp, module, ctx)),
                    );
                }
            }
        }
        Item::Trait(t) => {
            ctx.db.defs.push(SymDef {
                identity: type_identity(&ctx.scope_identity, &t.name),
                name: t.name.clone(),
                def_span: t.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Trait,
            });
            ctx.db.hover.push(HoverEntry {
                span: t.name_span,
                module_path: mp.to_string(),
                text: format!("trait `{}`", t.name),
            });
            for sig in &t.methods {
                let params: Vec<(String, AST::Type)> = sig
                    .params
                    .iter()
                    .filter(|p| p.name != Syntax::KW_SELF)
                    .map(|p| (p.name.clone(), p.ty.clone()))
                    .collect();
                ctx.db.defs.push(SymDef {
                    identity: trait_method_identity(&ctx.scope_identity, &t.name, sig),
                    name: sig.name.clone(),
                    def_span: sig.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Function {
                        params: params.clone(),
                        ret: sig.return_type.clone(),
                        effects: sig.declared_effects.clone(),
                        effect_via: None,
                    },
                });
                ctx.db.members.push(MemberFact {
                    owner: t.name.clone(),
                    name: sig.name.clone(),
                    identity: trait_method_identity(&ctx.scope_identity, &t.name, sig),
                    kind: MemberKind::Method,
                    origin: MemberOrigin::TraitRequirement {
                        trait_name: t.name.clone(),
                    },
                    signature: fn_signature(&sig.name, &params, &sig.return_type, sig.declared_effects.as_ref(), None, None),
                    module_path: mp.to_string(),
                    span: sig.name_span.into(),
                });
            }
        }
        Item::Tag(t) => {
            ctx.db.defs.push(SymDef {
                identity: type_identity(&ctx.scope_identity, &t.name),
                name: t.name.clone(),
                def_span: t.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Tag,
            });
            ctx.db.hover.push(HoverEntry {
                span: t.name_span,
                module_path: mp.to_string(),
                text: format!("tag `{}`", t.name),
            });
        }
        Item::Impl(i) => {
            for meth in &i.methods {
                record_func_type_nodes(meth, mp, ctx);
                let method_identity =
                    callable_identity(&ctx.scope_identity, Some(&i.type_name), &meth.name, meth);
                let origin = match &i.trait_name {
                    Some(trait_name) => MemberOrigin::TraitImpl {
                        trait_name: trait_name.clone(),
                    },
                    None => MemberOrigin::InherentImpl,
                };
                ctx.db.members.push(method_fact(
                    &ctx.scope_identity,
                    &i.type_name,
                    meth,
                    origin,
                    mp,
                ));
                ctx.db.defs.push(SymDef {
                    identity: method_identity.clone(),
                    name: meth.name.clone(),
                    def_span: meth.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Function {
                        params: meth
                            .params
                            .iter()
                            .filter(|p| p.name != Syntax::KW_SELF)
                            .map(|p| (p.name.clone(), p.ty.clone()))
                            .collect(),
                        ret: meth.return_type.clone(),
                        effects: meth.declared_effects.clone(),
                        effect_via: meth.effect_via.clone(),
                    },
                });
                for p in &meth.params {
                    if p.name != Syntax::KW_SELF {
                        ctx.db.defs.push(SymDef {
                            identity: local_identity(&method_identity, "param", &p.name),
                            name: p.name.clone(),
                            def_span: p.name_span,
                            module_path: mp.to_string(),
                            kind: SymKind::Param { ty: p.ty.clone() },
                        });
                    }
                }
                with_caller(
                    ctx,
                    CallerFrame {
                        name: meth.name.clone(),
                        owner: Some(i.type_name.clone()),
                        identity: method_identity,
                    },
                    &meth.params,
                    mp,
                    |ctx| structural_slot(ctx, "method_bodies", StructuralSlotKind::List, |ctx| collect_stmts(&meth.body, mp, module, ctx)),
                );
            }
        }
        Item::Const(c) => {
            ctx.db.defs.push(SymDef {
                identity: format!("const:{}::{}", ctx.scope_identity, c.name),
                name: c.name.clone(),
                def_span: c.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Const,
            });
            ctx.db.hover.push(HoverEntry {
                span: c.name_span,
                module_path: mp.to_string(),
                text: format!("const `{}`", c.name),
            });
            structural_slot(ctx, "value", StructuralSlotKind::Scalar, |ctx| collect_expr(&c.value, mp, ctx));
        }
        Item::Test(t) => {
            structural_slot(ctx, "body", StructuralSlotKind::List, |ctx| collect_stmts(&t.body, mp, module, ctx));
        }
        Item::Bench(b) => {
            structural_slot(ctx, "body", StructuralSlotKind::List, |ctx| collect_stmts(&b.body, mp, module, ctx));
        }
        Item::ExternRust(_) => {}
        Item::Module(m) => {
            ctx.db.defs.push(SymDef {
                identity: module_identity(&ctx.scope_identity, &m.name),
                name: m.name.clone(),
                def_span: m.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Module,
            });
        }
        // S59: C FFI boundary modules aren't yet indexed for symbols/hover.
        Item::CModule(_) => {}
        Item::CodeModule(m) => {
            let identity = m.instance_identity.as_ref().map(|instance| format!("instance:{}", instance.fingerprint))
                .unwrap_or_else(|| module_identity(&ctx.scope_identity, &m.name));
            ctx.db.defs.push(SymDef {
                identity: identity.clone(),
                name: m.name.clone(),
                def_span: m.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Module,
            });
            if let Some(body) = &m.body {
                let prev = std::mem::replace(&mut ctx.scope_identity, identity);
                structural_slot(ctx, "items", StructuralSlotKind::List, |ctx| {
                    for item in body { collect_item(item, mp, module, ctx); }
                });
                ctx.scope_identity = prev;
            }
        }
        Item::Distinct(value) => {
            ctx.db.defs.push(SymDef {
                identity: format!("type:{}::{}", ctx.scope_identity, value.name),
                name: value.name.clone(),
                def_span: value.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Type,
            });
            structural_slot(ctx, "base", StructuralSlotKind::Scalar, |ctx| { record_node(ctx, "type", "type", mp, value.base_span); });
        }
        Item::TypeAlias(value) => {
            ctx.db.defs.push(SymDef {
                identity: format!("type:{}::{}", ctx.scope_identity, value.name),
                name: value.name.clone(),
                def_span: value.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Type,
            });
            structural_slot(ctx, "target", StructuralSlotKind::Scalar, |ctx| {
                record_node(ctx, "type", "type", mp, value.target_span);
            });
        }
        Item::UnitFamily(value) => {
            ctx.db.defs.push(SymDef {
                identity: format!("type:{}::{}", ctx.scope_identity, value.family),
                name: value.family.clone(),
                def_span: value.family_span,
                module_path: mp.to_string(),
                kind: SymKind::Type,
            });
            for def in value.distinct_defs() {
                ctx.db.defs.push(SymDef {
                    identity: format!("type:{}::{}", ctx.scope_identity, def.name),
                    name: def.name,
                    def_span: def.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Type,
                });
            }
            for member in &value.members {
                ctx.db.defs.push(SymDef {
                    identity: format!("unit:{}::{}", ctx.scope_identity, member.name),
                    name: member.name.clone(),
                    def_span: member.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Const,
                });
            }
        }
        // D-ERR-CONV: error conversions aren't yet indexed for symbols/hover.
        Item::ErrorConv(_) => {}
        // D-MIGRATE1: migration blocks aren't yet indexed for symbols/hover.
        Item::Migration(_) => {}
        Item::StateDecl(value) => {
            ctx.db.defs.push(SymDef {
                identity: format!("type:{}::{}", ctx.scope_identity, value.type_name),
                name: value.type_name.clone(),
                def_span: value.type_name_span,
                module_path: mp.to_string(),
                kind: SymKind::Type,
            });
            for (name, span) in &value.states {
                ctx.db.defs.push(SymDef {
                    identity: format!("state:{}::{}", ctx.scope_identity, name),
                    name: name.clone(),
                    def_span: *span,
                    module_path: mp.to_string(),
                    kind: SymKind::EnumVariant { parent: value.type_name.clone() },
                });
            }
        }
        Item::ProtocolDecl(value) => {
            ctx.db.defs.push(SymDef {
                identity: format!("type:{}::{}", ctx.scope_identity, value.name),
                name: value.name.clone(),
                def_span: value.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Type,
            });
            for message in &value.messages {
                ctx.db.defs.push(SymDef {
                    identity: format!("protocol:{}::{}", ctx.scope_identity, message.name),
                    name: message.name.clone(),
                    def_span: message.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::EnumVariant { parent: value.name.clone() },
                });
            }
        }
        Item::UserDerive(value) => {
            ctx.db.refs.push(scoped_ref(value.trait_name.clone(), value.trait_span, mp, ctx));
            structural_slot(ctx, "body", StructuralSlotKind::List, |ctx| collect_stmts(&value.body, mp, module, ctx));
        }
        Item::GenericModule(value) => {
            ctx.db.defs.push(SymDef {
                identity: module_identity(&ctx.scope_identity, &value.name),
                name: value.name.clone(),
                def_span: value.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Module,
            });
        }
        Item::ModuleAlias(value) => {
            ctx.db.defs.push(SymDef {
                identity: module_identity(&ctx.scope_identity, &value.name),
                name: value.name.clone(),
                def_span: value.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Module,
            });
        }
    }
    if structural_id.is_some() { ctx.structural_parents.pop(); }
}

fn hover_for_fn(f: &AST::Func) -> String {
    // D-APILABEL1=A: hover and completion show the CALL contract, because that
    // is what the reader has to type: the public label, and the `/` and `*`
    // zone separators that decide whether a label is forbidden or required.
    let mut params: Vec<String> = Vec::new();
    let mut star_done = false;
    let callable: Vec<&AST::Param> = f
        .params
        .iter()
        .filter(|p| p.name != Syntax::KW_SELF)
        .collect();
    for (index, p) in callable.iter().enumerate() {
        if p.zone == AST::ParamZone::LabelOnly && !star_done {
            star_done = true;
            params.push(Syntax::PARAM_ZONE_LABEL_ONLY.to_string());
        }
        let head = match &p.public_label {
            Some((label, _)) => format!("{label} {}", p.name),
            None => p.name.clone(),
        };
        let default = match &p.default {
            Some(_) => " = …",
            None => "",
        };
        params.push(format!("{head}: {}{default}", p.ty.name()));
        let last_positional_only = p.zone == AST::ParamZone::PositionalOnly
            && callable
                .get(index + 1)
                .is_none_or(|next| next.zone != AST::ParamZone::PositionalOnly);
        if last_positional_only {
            params.push(Syntax::PARAM_ZONE_POSITIONAL_ONLY.to_string());
        }
    }
    let arrow = if let Some((param, _)) = &f.effect_via {
        format!(" =[via {param}]=>")
    } else {
        f.declared_effects.as_ref().map_or_else(
            || f.return_type.as_ref().map(|_| " =>".to_string()).unwrap_or_default(),
            |row| format!(" =[{}]=>", row.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(", ")),
        )
    };
    let ret = f.return_type.as_ref().map(|t| format!(" {}", t.name())).unwrap_or_default();
    format!("fn {}({}){}{}", f.name, params.join(", "), arrow, ret)
}

fn collect_stmts(stmts: &[AST::Stmt], mp: &str, module: &LoadedModule, ctx: &mut WalkCtx<'_>) {
    for stmt in stmts {
        collect_stmt(stmt, mp, module, ctx);
    }
}

fn collect_loop_label_def(label: &Option<(String, Span)>, mp: &str, ctx: &mut WalkCtx<'_>) {
    if let Some((name, span)) = label {
        ctx.db.defs.push(SymDef {
            identity: scoped_local_identity(ctx, "loop_label", name),
            name: name.clone(),
            def_span: *span,
            module_path: mp.to_string(),
            // Loop labels share the ordinary namespace but carry no runtime type.
            kind: SymKind::Local {
                mutable: false,
                ty: None,
            },
        });
    }
}

fn collect_loop_label_ref(name: &str, span: Span, mp: &str, ctx: &mut WalkCtx<'_>) {
    let name_span = Span::new(span.start, span.start.saturating_add(name.len()));
    ctx.db
        .refs
        .push(scoped_ref(name.to_string(), name_span, mp, ctx));
}

fn collect_stmt(stmt: &AST::Stmt, mp: &str, module: &LoadedModule, ctx: &mut WalkCtx<'_>) {
    let span = stmt.span();
    let structural_id = record_node(ctx, "stmt", &stmt_shape(stmt), mp, span);
    if let Some(id) = structural_id { ctx.structural_parents.push(id); }
    match stmt {
        AST::Stmt::Val(b) => {
            collect_binding(b, mp, ctx);
        }
        AST::Stmt::Expr(e) | AST::Stmt::Yield(e, _) => {
            structural_slot(ctx, "value", StructuralSlotKind::Scalar, |ctx| collect_expr(e, mp, ctx));
        }
        AST::Stmt::Assign { target, value, .. } => {
            collect_lvalue(target, mp, ctx);
            structural_slot(ctx, "value", StructuralSlotKind::Scalar, |ctx| collect_expr(value, mp, ctx));
        }
        AST::Stmt::Return(e, _) => {
            if let Some(e) = e {
                structural_slot(ctx, "value", StructuralSlotKind::Scalar, |ctx| collect_expr(e, mp, ctx));
            }
        }
        AST::Stmt::BreakValue(value, _) => {
            structural_slot(ctx, "value", StructuralSlotKind::Scalar, |ctx| {
                collect_expr(value, mp, ctx)
            });
        }
        AST::Stmt::BreakLabelValue(name, name_span, value, _) => {
            collect_loop_label_ref(name, *name_span, mp, ctx);
            structural_slot(ctx, "value", StructuralSlotKind::Scalar, |ctx| {
                collect_expr(value, mp, ctx)
            });
        }
        AST::Stmt::While {
            cond, body, label, ..
        } => {
            collect_loop_label_def(label, mp, ctx);
            structural_slot(ctx, "condition", StructuralSlotKind::Scalar, |ctx| collect_expr(cond, mp, ctx));
            structural_slot(ctx, "body", StructuralSlotKind::List, |ctx| collect_stmts(body, mp, module, ctx));
        }
        AST::Stmt::For {
            var,
            var_span,
            var2,
            kind,
            body,
            label,
            ..
        } => {
            collect_loop_label_def(label, mp, ctx);
            ctx.db.defs.push(SymDef {
                identity: scoped_local_identity(ctx, "local", var),
                name: var.clone(),
                def_span: *var_span,
                module_path: mp.to_string(),
                kind: SymKind::Local {
                    mutable: false,
                    ty: None,
                },
            });
            if let Some((v2, s2)) = var2 {
                ctx.db.defs.push(SymDef {
                    identity: scoped_local_identity(ctx, "local", v2),
                    name: v2.clone(),
                    def_span: *s2,
                    module_path: mp.to_string(),
                    kind: SymKind::Local {
                        mutable: false,
                        ty: None,
                    },
                });
            }
            match kind {
                AST::ForKind::Range { start, end, step, exclusive: _ } => {
                    structural_slot(ctx, "range_start", StructuralSlotKind::Scalar, |ctx| collect_expr(start, mp, ctx));
                    structural_slot(ctx, "range_end", StructuralSlotKind::Scalar, |ctx| collect_expr(end, mp, ctx));
                    if let Some(step) = step {
                        structural_slot(ctx, "range_step", StructuralSlotKind::Scalar, |ctx| collect_expr(step, mp, ctx));
                    }
                }
                AST::ForKind::In { collection, step } => {
                    structural_slot(ctx, "source", StructuralSlotKind::Scalar, |ctx| collect_expr(collection, mp, ctx));
                    if let Some(step) = step {
                        structural_slot(ctx, "stride", StructuralSlotKind::Scalar, |ctx| collect_expr(step, mp, ctx));
                    }
                }
            }
            structural_slot(ctx, "body", StructuralSlotKind::List, |ctx| collect_stmts(body, mp, module, ctx));
        }
        AST::Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        }
        // D-OSTARGET2=B: `#Known if build.os == { … }` — index arm bodies
        // the same as a runtime dispatch (sema desugars it away later).
        | AST::Stmt::ComptimeSwitch {
            subject,
            arms,
            else_body,
            ..
        } => {
            structural_slot(ctx, "subject", StructuralSlotKind::Scalar, |ctx| collect_expr(subject, mp, ctx));
            structural_slot(ctx, "arms", StructuralSlotKind::List, |ctx| {
                for arm in arms {
                    let arm_id = record_node(ctx, "arm", "switch_arm", mp, arm.span);
                    if let Some(id) = arm_id { ctx.structural_parents.push(id); }
                    structural_slot(ctx, "condition", StructuralSlotKind::Scalar, |ctx| collect_expr(&arm.cond, mp, ctx));
                    structural_slot(ctx, "body", StructuralSlotKind::List, |ctx| collect_stmts(&arm.body, mp, module, ctx));
                    if let Some(id) = arm_id {
                        let has_boundary = ctx.db.slot_boundaries.iter().any(|boundary| {
                            boundary.parent == id && boundary.slot == "body"
                        });
                        if !has_boundary && arm.body.len() == 1 {
                            ctx.db.slot_boundaries.push(StructuralSlotBoundary {
                                parent: id,
                                slot: "body".to_string(),
                                module_path: mp.to_string(),
                                span: arm.body[0].span().into(),
                            });
                        }
                        ctx.structural_parents.pop();
                    }
                }
            });
            if let Some(eb) = else_body {
                structural_slot(ctx, "else_body", StructuralSlotKind::List, |ctx| collect_stmts(eb, mp, module, ctx));
            }
        }
        AST::Stmt::CountedLoop {
            cond, body, init, step, label, ..
        } => {
            collect_loop_label_def(label, mp, ctx);
            ctx.db.defs.push(SymDef {
                identity: scoped_local_identity(ctx, "local", &init.name),
                name: init.name.clone(),
                def_span: init.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Local {
                    mutable: true,
                    ty: None,
                },
            });
            structural_slot(ctx, "init", StructuralSlotKind::Scalar, |ctx| collect_expr(&init.init, mp, ctx));
            structural_slot(ctx, "condition", StructuralSlotKind::Scalar, |ctx| collect_expr(cond, mp, ctx));
            if let Some(step) = step {
                structural_slot(ctx, "afterthought", StructuralSlotKind::Scalar, |ctx| collect_stmt(step, mp, module, ctx));
            }
            structural_slot(ctx, "body", StructuralSlotKind::List, |ctx| collect_stmts(body, mp, module, ctx));
        }
        // D-LINTPOLICY1=A: an `#Unsafe("reason") { … }` audited region is a
        // spelled bypass — record it before recursing into its body.
        AST::Stmt::Unsafe {
            audit, body, span, ..
        } => {
            ctx.db.bypasses.push(BypassFact {
                kind: BypassKind::UnsafeRegion,
                site: active_scope(ctx).to_string(),
                detail: audit.clone().unwrap_or_default(),
                module_path: mp.to_string(),
                span: (*span).into(),
            });
            structural_slot(ctx, "body", StructuralSlotKind::List, |ctx| collect_stmts(body, mp, module, ctx));
        }
        AST::Stmt::Loop { body, label, .. } => {
            collect_loop_label_def(label, mp, ctx);
            structural_slot(ctx, "body", StructuralSlotKind::List, |ctx| collect_stmts(body, mp, module, ctx));
        }
        AST::Stmt::Impure { body, .. }
        | AST::Stmt::Reactive { body, .. }
        | AST::Stmt::Shield { body, .. }
        | AST::Stmt::Off { body, .. }
        | AST::Stmt::DebugOnly { body, .. }
        | AST::Stmt::Region { body, .. }
        | AST::Stmt::Policy { body, .. }
        | AST::Stmt::TaskGroup { body, .. }
        | AST::Stmt::Layout { body, .. }
        | AST::Stmt::Caps { body, .. }
        | AST::Stmt::Grant { body, .. }
        | AST::Stmt::Transact { body, .. }
        | AST::Stmt::AssumeDet { body, .. } => {
            structural_slot(ctx, "body", StructuralSlotKind::List, |ctx| collect_stmts(body, mp, module, ctx));
        }
        AST::Stmt::Break(_) | AST::Stmt::Continue(_) => {}
        AST::Stmt::BreakLabel(name, span) | AST::Stmt::ContinueLabel(name, span) => {
            collect_loop_label_ref(name, *span, mp, ctx);
        }
        // D-CTMARKER1: collect symbols from comptime block body.
        AST::Stmt::ComptimeBlock { body, .. } => structural_slot(ctx, "body", StructuralSlotKind::List, |ctx| collect_stmts(body, mp, module, ctx)),
        AST::Stmt::ComptimeIf {
            cond,
            then_body,
            else_body,
            ..
        } => {
            structural_slot(ctx, "condition", StructuralSlotKind::Scalar, |ctx| collect_expr(cond, mp, ctx));
            structural_slot(ctx, "then_body", StructuralSlotKind::List, |ctx| collect_stmts(then_body, mp, module, ctx));
            if let Some(eb) = else_body {
                structural_slot(ctx, "else_body", StructuralSlotKind::List, |ctx| collect_stmts(eb, mp, module, ctx));
            }
        }
        AST::Stmt::ContextBlock { fields, body, .. } => {
            structural_slot(ctx, "fields", StructuralSlotKind::List, |ctx| {
                for (_, e, _) in fields { collect_expr(e, mp, ctx); }
            });
            structural_slot(ctx, "body", StructuralSlotKind::List, |ctx| collect_stmts(body, mp, module, ctx));
        }
        // D-TERM1 (ratified 2026-06-22): collect symbols from live block body.
        AST::Stmt::Live { body, .. } => {
            structural_slot(ctx, "body", StructuralSlotKind::List, |ctx| collect_stmts(body, mp, module, ctx));
        }
        // D-DOTSCOPE1: collect symbols from a scope-member region body.
        AST::Stmt::ScopeMember { body, .. } => {
            structural_slot(ctx, "body", StructuralSlotKind::List, |ctx| collect_stmts(body, mp, module, ctx));
        }
    }
    if structural_id.is_some() { ctx.structural_parents.pop(); }
}

fn collect_lvalue(lv: &AST::LValue, mp: &str, ctx: &mut WalkCtx<'_>) {
    match lv {
        AST::LValue::Local { name, name_span } => {
            ctx.db
                .refs
                .push(scoped_ref(name.clone(), *name_span, mp, ctx));
        }
        AST::LValue::Index { base, index, .. } => {
            structural_slot(ctx, "target_base", StructuralSlotKind::Scalar, |ctx| collect_expr(base, mp, ctx));
            structural_slot(ctx, "target_index", StructuralSlotKind::Scalar, |ctx| collect_expr(index, mp, ctx));
        }
        // D-MUTSELF1: `place.field = v` — record references in the base place.
        AST::LValue::Field { base, .. } => structural_slot(ctx, "target_base", StructuralSlotKind::Scalar, |ctx| collect_expr(base, mp, ctx)),
    }
}

fn collect_binding(b: &AST::Binding, mp: &str, ctx: &mut WalkCtx<'_>) {
    if let Some(span) = b.ty_span {
        structural_slot(ctx, "type", StructuralSlotKind::Scalar, |ctx| { record_node(ctx, "type", "type", mp, span); });
    }
    // S74: a destructuring binding brings each named field/element into scope.
    if let Some(pat) = &b.pattern {
        structural_slot(ctx, "initializer", StructuralSlotKind::Scalar, |ctx| collect_expr(&b.init, mp, ctx));
        for n in pat.names() {
            ctx.db.defs.push(SymDef {
                identity: scoped_local_identity(ctx, "local", &n.name),
                name: n.name.clone(),
                def_span: n.span,
                module_path: mp.to_string(),
                kind: SymKind::Local {
                    mutable: b.mutable,
                    ty: None,
                },
            });
        }
        return;
    }
    let ty = b.ty.clone();
    // has_explicit_annotation: user wrote `: Type` — ty_span is Some iff the annotation is in source
    let has_explicit = b.ty_span.is_some();
    ctx.db.defs.push(SymDef {
        identity: scoped_local_identity(ctx, "local", &b.name),
        name: b.name.clone(),
        def_span: b.name_span,
        module_path: mp.to_string(),
        kind: SymKind::Local {
            mutable: b.mutable,
            ty: ty.clone(),
        },
    });
    if let Some(t) = &ty {
        let text = if b.mutable {
            format!("`{}`: {} (mutable)", b.name, t.name())
        } else {
            format!("`{}`: {} (immutable)", b.name, t.name())
        };
        ctx.db.hover.push(HoverEntry {
            span: b.name_span,
            module_path: mp.to_string(),
            text,
        });
        // Inlay hint: only when user omitted the annotation (sema filled it in)
        if !has_explicit {
            let label = format!(": {}", t.name());
            ctx.db.inlay.push(InlayHint {
                span: b.name_span,
                module_path: mp.to_string(),
                label,
            });
        }
    }
    structural_slot(ctx, "initializer", StructuralSlotKind::Scalar, |ctx| collect_expr(&b.init, mp, ctx));
}

fn collect_expr(e: &AST::Expr, mp: &str, ctx: &mut WalkCtx<'_>) {
    let structural_id = record_node(ctx, "expr", &expr_shape(e), mp, structural_expr_span(e));
    if let Some(id) = structural_id { ctx.structural_parents.push(id); }
    match e {
        AST::Expr::PtrFromAddr { addr, .. } => {
            structural_slot(ctx, "address", StructuralSlotKind::Scalar, |ctx| collect_expr(addr, mp, ctx));
        }
        AST::Expr::Ident(name, span) => {
            ctx.db.refs.push(scoped_ref(name.clone(), *span, mp, ctx));
        }
        AST::Expr::Call(call) => {
            record_call(ctx, mp, &call.name, call.name_span);
            ctx.db
                .refs
                .push(scoped_ref(call.name.clone(), call.name_span, mp, ctx));
            structural_slot(ctx, "args", StructuralSlotKind::List, |ctx| {
                for arg in &call.args { collect_expr(&arg.expr, mp, ctx); }
            });
        }
        AST::Expr::MethodCall {
            receiver,
            method,
            method_span,
            args,
            ..
        } => {
            structural_slot(ctx, "receiver", StructuralSlotKind::Scalar, |ctx| collect_expr(receiver, mp, ctx));
            record_call(ctx, mp, method, *method_span);
            ctx.db
                .refs
                .push(scoped_ref(method.clone(), *method_span, mp, ctx));
            // D-LINTPOLICY1=A: `.drop("reason")` is the sole intentional
            // discard spelling (D-IGNORERET2) — a spelled bypass of the
            // must-use return-value check. Record it before the args walk.
            if method == Syntax::METHOD_DROP {
                let reason = args.first().and_then(|a| match &a.expr {
                    AST::Expr::Str(parts, _) => match parts.as_slice() {
                        [AST::StrPart::Lit(s)] => Some(s.clone()),
                        _ => None,
                    },
                    _ => None,
                });
                ctx.db.bypasses.push(BypassFact {
                    kind: BypassKind::ExplicitDrop,
                    site: active_scope(ctx).to_string(),
                    detail: reason.unwrap_or_default(),
                    module_path: mp.to_string(),
                    span: (*method_span).into(),
                });
            }
            structural_slot(ctx, "args", StructuralSlotKind::List, |ctx| {
                for arg in args { collect_expr(&arg.expr, mp, ctx); }
            });
        }
        AST::Expr::Field(base, field, span) => {
            structural_slot(ctx, "base", StructuralSlotKind::Scalar, |ctx| collect_expr(base, mp, ctx));
            ctx.db.refs.push(scoped_ref(field.clone(), *span, mp, ctx));
        }
        AST::Expr::OptField {
            base,
            member,
            member_span,
            ..
        } => {
            structural_slot(ctx, "base", StructuralSlotKind::Scalar, |ctx| collect_expr(base, mp, ctx));
            ctx.db
                .refs
                .push(scoped_ref(member.clone(), *member_span, mp, ctx));
        }
        AST::Expr::Binary(_, l, r, _) => {
            structural_slot(ctx, "lhs", StructuralSlotKind::Scalar, |ctx| collect_expr(l, mp, ctx));
            structural_slot(ctx, "rhs", StructuralSlotKind::Scalar, |ctx| collect_expr(r, mp, ctx));
        }
        AST::Expr::CompareChain { operands, .. } => {
            structural_slot(ctx, "operands", StructuralSlotKind::List, |ctx| {
                for e in operands { collect_expr(e, mp, ctx); }
            });
        }
        AST::Expr::Unary(_, inner, _) | AST::Expr::IncDec { operand: inner, .. } => {
            structural_slot(ctx, "operand", StructuralSlotKind::Scalar, |ctx| collect_expr(inner, mp, ctx));
        }
        AST::Expr::Deref(inner, _)
        | AST::Expr::RawOf(inner, _)
        | AST::Expr::Copy(inner, _)
        | AST::Expr::Place(inner, _, _) => {
            structural_slot(ctx, "operand", StructuralSlotKind::Scalar, |ctx| collect_expr(inner, mp, ctx));
        }
        AST::Expr::Index { base, index, .. } => {
            structural_slot(ctx, "base", StructuralSlotKind::Scalar, |ctx| collect_expr(base, mp, ctx));
            structural_slot(ctx, "index", StructuralSlotKind::Scalar, |ctx| collect_expr(index, mp, ctx));
        }
        AST::Expr::Slice {
            base, start, end, range, ..
        } => {
            structural_slot(ctx, "base", StructuralSlotKind::Scalar, |ctx| collect_expr(base, mp, ctx));
            if let Some(range) = range {
                structural_slot(ctx, "range", StructuralSlotKind::Scalar, |ctx| collect_expr(range, mp, ctx));
            } else {
                structural_slot(ctx, "start", StructuralSlotKind::Scalar, |ctx| collect_expr(start, mp, ctx));
                structural_slot(ctx, "end", StructuralSlotKind::Scalar, |ctx| collect_expr(end, mp, ctx));
            }
        }
        AST::Expr::Range { start, end, .. } => {
            structural_slot(ctx, "start", StructuralSlotKind::Scalar, |ctx| collect_expr(start, mp, ctx));
            structural_slot(ctx, "end", StructuralSlotKind::Scalar, |ctx| collect_expr(end, mp, ctx));
        }
        AST::Expr::Str(parts, _) => {
            structural_slot(ctx, "interpolations", StructuralSlotKind::List, |ctx| {
                for part in parts {
                    if let AST::StrPart::Interp(inner, _) = part { collect_expr(inner, mp, ctx); }
                }
            });
        }
        AST::Expr::ListLit(items, _) => {
            structural_slot(ctx, "items", StructuralSlotKind::List, |ctx| {
                for i in items { collect_expr(i, mp, ctx); }
            });
        }
        AST::Expr::MemberSpread { base, .. } => {
            structural_slot(ctx, "base", StructuralSlotKind::Scalar, |ctx| collect_expr(base, mp, ctx));
        }
        AST::Expr::Spread(inner, _) => structural_slot(ctx, "value", StructuralSlotKind::Scalar, |ctx| collect_expr(inner, mp, ctx)),
        AST::Expr::MapLit(pairs, _) => {
            structural_slot(ctx, "keys", StructuralSlotKind::List, |ctx| {
                for (k, _) in pairs { collect_expr(k, mp, ctx); }
            });
            structural_slot(ctx, "values", StructuralSlotKind::List, |ctx| {
                for (_, v) in pairs { collect_expr(v, mp, ctx); }
            });
        }
        AST::Expr::TupleLit(fields, _, _) => {
            structural_slot(ctx, "fields", StructuralSlotKind::List, |ctx| {
                for (_, expr) in fields { collect_expr(expr, mp, ctx); }
            });
        }
        AST::Expr::StructLit {
            type_name,
            type_args,
            fields,
            inferred,
            span,
            ..
        } => {
            // After sema fills `type_name` on inferred `.{…}`, surface it in IDE.
            if *inferred && !type_name.is_empty() {
                let ty_label = if type_args.is_empty() {
                    type_name.clone()
                } else {
                    AST::Type::Apply {
                        name: type_name.clone(),
                        args: type_args.clone(),
                    }
                    .show()
                };
                ctx.db.hover.push(HoverEntry {
                    span: *span,
                    module_path: mp.to_string(),
                    text: format!("`{}`", ty_label),
                });
                ctx.db.inlay.push(InlayHint {
                    span: *span,
                    module_path: mp.to_string(),
                    label: format!(": {}", ty_label),
                });
            }
            structural_slot(ctx, "fields", StructuralSlotKind::List, |ctx| {
                for (_, _, expr) in fields {
                    collect_expr(expr, mp, ctx);
                }
            });
        }
        AST::Expr::TypedLit { body, .. } => {
            structural_slot(ctx, "body", StructuralSlotKind::List, |ctx| {
                body.for_each_expr(|expr| collect_expr(expr, mp, ctx));
            });
        }
        AST::Expr::EnumLit { args, .. } => {
            structural_slot(ctx, "args", StructuralSlotKind::List, |ctx| {
                for arg in args {
                    match arg {
                        AST::EnumLitArg::Positional(e) => collect_expr(e, mp, ctx),
                        AST::EnumLitArg::Named { expr, .. } => collect_expr(expr, mp, ctx),
                    }
                }
            });
        }
        AST::Expr::Tainted(inner, _, _) // D-TAINT1: tag erased; recurse into the value.
        | AST::Expr::Present(inner, _)
        | AST::Expr::Ok(inner, _)
        | AST::Expr::Err(inner, _)
        | AST::Expr::Try(inner, _, _) => structural_slot(ctx, "value", StructuralSlotKind::Scalar, |ctx| collect_expr(inner, mp, ctx)),
        AST::Expr::OrFallback {
            value, fallback, ..
        } => {
            structural_slot(ctx, "value", StructuralSlotKind::Scalar, |ctx| collect_expr(value, mp, ctx));
            match fallback {
                AST::OrFallback::Value(v) => structural_slot(ctx, "fallback", StructuralSlotKind::Scalar, |ctx| collect_expr(v, mp, ctx)),
                AST::OrFallback::Panic { args, .. } => {
                    structural_slot(ctx, "fallback_args", StructuralSlotKind::List, |ctx| {
                        for a in args { collect_expr(&a.expr, mp, ctx); }
                    });
                }
                AST::OrFallback::Return(v, _) => {
                    if let Some(v) = v {
                        structural_slot(ctx, "fallback", StructuralSlotKind::Scalar, |ctx| collect_expr(v, mp, ctx));
                    }
                }
                AST::OrFallback::Break(_) | AST::OrFallback::Continue(_) => {}
                AST::OrFallback::BreakLabel(name, span)
                | AST::OrFallback::ContinueLabel(name, span) => {
                    collect_loop_label_ref(name, *span, mp, ctx);
                }
            }
        }
        AST::Expr::PatternTest { subject, .. } => structural_slot(ctx, "subject", StructuralSlotKind::Scalar, |ctx| collect_expr(subject, mp, ctx)),
        AST::Expr::Lambda(l) => {
            for p in &l.params {
                ctx.db.defs.push(SymDef {
                    identity: scoped_local_identity(ctx, "param", &p.name),
                    name: p.name.clone(),
                    def_span: p.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Param {
                        ty: p.ty.clone().unwrap_or(AST::Type::Int),
                    },
                });
            }
            match &l.body {
                AST::LambdaBody::Expr(e) => structural_slot(ctx, "body", StructuralSlotKind::Scalar, |ctx| collect_expr(e, mp, ctx)),
                AST::LambdaBody::Block(stmts) => {
                    structural_slot(ctx, "body", StructuralSlotKind::List, |ctx| {
                        for s in stmts {
                            if let AST::Stmt::Val(b) = s { collect_binding(b, mp, ctx); }
                            else { collect_expr_stmt(s, mp, ctx); }
                        }
                    });
                }
            }
        }
        AST::Expr::CallValue { callee, args, .. } => {
            structural_slot(ctx, "callee", StructuralSlotKind::Scalar, |ctx| collect_expr(callee, mp, ctx));
            structural_slot(ctx, "args", StructuralSlotKind::List, |ctx| {
                for a in args { collect_expr(&a.expr, mp, ctx); }
            });
        }
        AST::Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            structural_slot(ctx, "condition", StructuralSlotKind::Scalar, |ctx| collect_expr(cond, mp, ctx));
            structural_slot(ctx, "then_body", StructuralSlotKind::List, |ctx| {
                for s in then_body {
                    if let AST::Stmt::Val(b) = s { collect_binding(b, mp, ctx); }
                    else { collect_expr_stmt(s, mp, ctx); }
                }
            });
            structural_slot(ctx, "then_value", StructuralSlotKind::Scalar, |ctx| collect_expr(then_value, mp, ctx));
            structural_slot(ctx, "else_body", StructuralSlotKind::List, |ctx| {
                for s in else_body {
                    if let AST::Stmt::Val(b) = s { collect_binding(b, mp, ctx); }
                    else { collect_expr_stmt(s, mp, ctx); }
                }
            });
            structural_slot(ctx, "else_value", StructuralSlotKind::Scalar, |ctx| collect_expr(else_value, mp, ctx));
        }
        AST::Expr::Int(_, _, _, _)
        | AST::Expr::Float(_, _, _)
        | AST::Expr::Bool(_, _)
        | AST::Expr::Char(_, _)
        | AST::Expr::Absent(_)
        | AST::Expr::ReduceMarker(_, _)
        | AST::Expr::Todo { .. }
        | AST::Expr::NoElse(_)
        | AST::Expr::UnitLit { .. }
        // D-SHIFT1 (c7shift) / D-BINPAT1 (card #506 follow-up): a leaf
        // literal, no nested `Expr` to recurse into.
        | AST::Expr::StrMatchLit(_, _)
        | AST::Expr::BinMatchLit(_, _) => {}
        AST::Expr::Paren(inner, _) => structural_slot(ctx, "inner", StructuralSlotKind::Scalar, |ctx| collect_expr(inner, mp, ctx)),
    }
    if structural_id.is_some() { ctx.structural_parents.pop(); }
}

fn collect_expr_stmt(stmt: &AST::Stmt, mp: &str, ctx: &mut WalkCtx<'_>) {
    if let AST::Stmt::Expr(e) = stmt {
        collect_expr(e, mp, ctx);
    }
}
