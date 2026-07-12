//! Semantic index builder — walks a checked `ProgramBundle` AST.
//! D-SEMINDEX1: shared by LSP and the public `jet-semindex` query API.

use jet_foundation::Diagnostics::Span;
use jet_foundation::Syntax;
use jet_foundation::AST::{self, Item, LoadedModule, ProgramBundle};
use jet_sema::{effect_key, SemIndexEffectFacts};
use std::collections::HashMap;

use crate::Json::{convert_defs, convert_effects, convert_refs};
use crate::Types::{CallEdge, DefinitionAnchor, MemberFact, MemberKind, MemberOrigin, SemIndex, StructuralNode, StructuralSlotBoundary, StructuralSlotKind};
use crate::Symbols::{build_semantic_symbol_index, SemanticSymbolIndex};

/// The semantic kind of a defined symbol (LSP-facing; uses AST types internally).
#[derive(Debug, Clone)]
pub enum SymKind {
    Module,
    Function {
        params: Vec<(String, AST::Type)>,
        ret: Option<AST::Type>,
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
                Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(),
            ),
            defs: Vec::new(),
            refs: Vec::new(),
            calls: Vec::new(),
            members: Vec::new(),
            hover: Vec::new(),
            inlay: Vec::new(),
            nodes: Vec::new(),
            slot_boundaries: Vec::new(),
            symbols: SemanticSymbolIndex::language(),
        }
    }

    fn finalize_index(&mut self, facts: &SemIndexEffectFacts) {
        let defs = convert_defs(&self.defs);
        let refs = convert_refs(&self.refs);
        let effects = convert_effects(facts);
        self.index = SemIndex::new(
            defs,
            refs,
            self.calls.clone(),
            effects,
            self.members.clone(),
            self.nodes.clone(),
        );
    }

    /// Find the definition whose def_span contains `offset` in module at `path`.
    #[allow(dead_code)]
    fn def_at_offset(&self, path: &str, offset: usize) -> Option<&SymDef> {
        self.defs.iter().find(|d| {
            d.module_path == path && d.def_span.start <= offset && offset <= d.def_span.end
        })
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

fn fn_signature(name: &str, params: &[(String, AST::Type)], ret: &Option<AST::Type>) -> String {
    let params = params
        .iter()
        .map(|(n, t)| format!("{n}: {}", t.name()))
        .collect::<Vec<_>>()
        .join(", ");
    match ret {
        Some(t) => format!("fn {name}({params}) -> {}", t.name()),
        None => format!("fn {name}({params})"),
    }
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
        signature: fn_signature(&f.name, &params, &f.return_type),
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
                        && (!exact_parent_end || span.end <= parent_end)
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
    }
    add_breadcrumb_hints(&mut db);
    db.finalize_index(facts);
    db.symbols = build_semantic_symbol_index(&db, bundle);
    db
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
        Item::Func(f) => {
            record_func_type_nodes(f, mp, ctx);
            let fn_identity = callable_identity(&ctx.scope_identity, None, &f.name, f);
            let params: Vec<(String, AST::Type)> = method_params(f);
            let sym = SymDef {
                identity: fn_identity.clone(),
                name: f.name.clone(),
                def_span: f.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Function {
                    params: params.clone(),
                    ret: f.return_type.clone(),
                },
            };
            let hover_text = hover_for_fn(f);
            ctx.db.hover.push(HoverEntry {
                span: f.name_span,
                module_path: mp.to_string(),
                text: hover_text,
            });
            ctx.db.defs.push(sym);
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
                    text: format!("`{}`: {}", p.name, p.ty.name()),
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
                    signature: fn_signature(&sig.name, &params, &sig.return_type),
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
            let identity = module_identity(&ctx.scope_identity, &m.name);
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
        // D-DIST1: distinct types aren't yet indexed for symbols/hover.
        Item::Distinct(value) => {
            structural_slot(ctx, "base", StructuralSlotKind::Scalar, |ctx| { record_node(ctx, "type", "type", mp, value.base_span); });
        }
        // D-TYPEALIAS1: type aliases aren't yet indexed for symbols/hover.
        Item::TypeAlias(value) => {
            structural_slot(ctx, "target", StructuralSlotKind::Scalar, |ctx| {
                record_node(ctx, "type", "type", mp, value.target_span);
            });
        }
        // D-QUAL3: unit families aren't yet indexed for symbols/hover.
        Item::UnitFamily(_) => {}
        // D-ERR-CONV: error conversions aren't yet indexed for symbols/hover.
        Item::ErrorConv(_) => {}
        // D-MIGRATE1: migration blocks aren't yet indexed for symbols/hover.
        Item::Migration(_) => {}
        // D-STATE-DECL: state-set declarations aren't yet indexed for symbols/hover.
        Item::StateDecl(_) => {}
        Item::ProtocolDecl(_) => {}
        // D-METADERIVE1=A: user-authored derive blocks aren't indexed (expanded in sema).
        Item::UserDerive(_) => {}
        // D-GENMOD2=A: templates/aliases aren't indexed (erased).
        Item::GenericModule(_) | Item::ModuleAlias(_) => {}
    }
    if structural_id.is_some() { ctx.structural_parents.pop(); }
}

fn hover_for_fn(f: &AST::Func) -> String {
    let params: Vec<String> = f
        .params
        .iter()
        .filter(|p| p.name != Syntax::KW_SELF)
        .map(|p| format!("{}: {}", p.name, p.ty.name()))
        .collect();
    let ret = match &f.return_type {
        Some(t) => format!(" -> {}", t.name()),
        None => String::new(),
    };
    format!("fn {}({}){}", f.name, params.join(", "), ret)
}

fn collect_stmts(stmts: &[AST::Stmt], mp: &str, module: &LoadedModule, ctx: &mut WalkCtx<'_>) {
    for stmt in stmts {
        collect_stmt(stmt, mp, module, ctx);
    }
}

fn collect_stmt(stmt: &AST::Stmt, mp: &str, module: &LoadedModule, ctx: &mut WalkCtx<'_>) {
    let span = match stmt {
        AST::Stmt::If(if_stmt) => if_stmt.span,
        _ => stmt.span(),
    };
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
        AST::Stmt::If(if_stmt) => {
            collect_if(if_stmt, mp, module, ctx);
        }
        AST::Stmt::While { cond, body, .. } => {
            structural_slot(ctx, "condition", StructuralSlotKind::Scalar, |ctx| collect_expr(cond, mp, ctx));
            structural_slot(ctx, "body", StructuralSlotKind::List, |ctx| collect_stmts(body, mp, module, ctx));
        }
        AST::Stmt::For {
            var,
            var_span,
            var2,
            kind,
            body,
            ..
        } => {
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
                AST::ForKind::Range { start, end, step } => {
                    structural_slot(ctx, "range_start", StructuralSlotKind::Scalar, |ctx| collect_expr(start, mp, ctx));
                    structural_slot(ctx, "range_end", StructuralSlotKind::Scalar, |ctx| collect_expr(end, mp, ctx));
                    if let Some(step) = step {
                        structural_slot(ctx, "range_step", StructuralSlotKind::Scalar, |ctx| collect_expr(step, mp, ctx));
                    }
                }
                AST::ForKind::In { collection } => {
                    structural_slot(ctx, "collection", StructuralSlotKind::Scalar, |ctx| collect_expr(collection, mp, ctx));
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
        // D-OSTARGET2=B: `comptime if build.os == { … }` — index arm bodies
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
            cond, body, init, ..
        } => {
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
            structural_slot(ctx, "initializer", StructuralSlotKind::Scalar, |ctx| collect_expr(&init.init, mp, ctx));
            structural_slot(ctx, "condition", StructuralSlotKind::Scalar, |ctx| collect_expr(cond, mp, ctx));
            structural_slot(ctx, "body", StructuralSlotKind::List, |ctx| collect_stmts(body, mp, module, ctx));
        }
        AST::Stmt::Loop { body, .. }
        | AST::Stmt::Unsafe { body, .. }
        | AST::Stmt::Impure { body, .. }
        | AST::Stmt::Reactive { body, .. }
        | AST::Stmt::Shield { body, .. }
        | AST::Stmt::Off { body, .. }
        | AST::Stmt::DebugOnly { body, .. }
        | AST::Stmt::Region { body, .. }
        | AST::Stmt::TaskGroup { body, .. }
        | AST::Stmt::Layout { body, .. }
        | AST::Stmt::Caps { body, .. }
        | AST::Stmt::Grant { body, .. }
        | AST::Stmt::Transact { body, .. }
        | AST::Stmt::AssumeDet { body, .. } => {
            structural_slot(ctx, "body", StructuralSlotKind::List, |ctx| collect_stmts(body, mp, module, ctx));
        }
        AST::Stmt::Break(_)
        | AST::Stmt::Continue(_)
        | AST::Stmt::BreakLabel(..)
        | AST::Stmt::ContinueLabel(..) => {}
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
        // D-IGNORERET2=A: collect symbols from suppress-must-use block body.
        AST::Stmt::SuppressMustUse { body, .. } => {
            structural_slot(ctx, "body", StructuralSlotKind::List, |ctx| collect_stmts(body, mp, module, ctx));
        }
    }
    if structural_id.is_some() { ctx.structural_parents.pop(); }
}

fn collect_if(if_stmt: &AST::IfStmt, mp: &str, module: &LoadedModule, ctx: &mut WalkCtx<'_>) {
    structural_slot(ctx, "condition", StructuralSlotKind::Scalar, |ctx| collect_expr(&if_stmt.cond, mp, ctx));
    structural_slot(ctx, "then_body", StructuralSlotKind::List, |ctx| collect_stmts(&if_stmt.then_body, mp, module, ctx));
    if let Some(eb) = &if_stmt.else_branch {
        match eb {
            AST::ElseBranch::ElseIf(inner) => structural_slot(ctx, "else_if", StructuralSlotKind::Scalar, |ctx| collect_if(inner, mp, module, ctx)),
            AST::ElseBranch::Else(body) => structural_slot(ctx, "else_body", StructuralSlotKind::List, |ctx| collect_stmts(body, mp, module, ctx)),
        }
    }
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
        AST::Expr::Deref(inner, _) | AST::Expr::RawOf(inner, _) | AST::Expr::Copy(inner, _) => {
            structural_slot(ctx, "operand", StructuralSlotKind::Scalar, |ctx| collect_expr(inner, mp, ctx));
        }
        AST::Expr::Index { base, index, .. } => {
            structural_slot(ctx, "base", StructuralSlotKind::Scalar, |ctx| collect_expr(base, mp, ctx));
            structural_slot(ctx, "index", StructuralSlotKind::Scalar, |ctx| collect_expr(index, mp, ctx));
        }
        AST::Expr::Slice {
            base, start, end, ..
        } => {
            structural_slot(ctx, "base", StructuralSlotKind::Scalar, |ctx| collect_expr(base, mp, ctx));
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
        AST::Expr::StructLit { fields, .. } => {
            structural_slot(ctx, "fields", StructuralSlotKind::List, |ctx| {
                for (_, _, expr) in fields { collect_expr(expr, mp, ctx); }
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
        AST::Expr::Tainted(inner, _) // D-TAINT1: tag erased; recurse into the value.
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
        AST::Expr::FanOut { callee, items, .. } => {
            structural_slot(ctx, "callee", StructuralSlotKind::Scalar, |ctx| collect_expr(callee, mp, ctx));
            structural_slot(ctx, "items", StructuralSlotKind::List, |ctx| {
                for item in items { collect_expr(item, mp, ctx); }
            });
        }
        AST::Expr::Int(_, _, _)
        | AST::Expr::Float(_, _, _)
        | AST::Expr::Bool(_, _)
        | AST::Expr::Char(_, _)
        | AST::Expr::Absent(_)
        | AST::Expr::ReduceMarker(_, _)
        | AST::Expr::Todo { .. }
        | AST::Expr::UnitLit { .. }
        | AST::Expr::ComptimeSplice { .. }
        // D-SHIFT1 (c7shift): a leaf literal, no nested `Expr` to recurse into.
        | AST::Expr::StrMatchLit(_, _) => {}
        AST::Expr::Paren(inner, _) => structural_slot(ctx, "inner", StructuralSlotKind::Scalar, |ctx| collect_expr(inner, mp, ctx)),
    }
    if structural_id.is_some() { ctx.structural_parents.pop(); }
}

fn collect_expr_stmt(stmt: &AST::Stmt, mp: &str, ctx: &mut WalkCtx<'_>) {
    if let AST::Stmt::Expr(e) = stmt {
        collect_expr(e, mp, ctx);
    }
}
