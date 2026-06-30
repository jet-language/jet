//! Semantic index builder — walks a checked `ProgramBundle` AST.
//! D-SEMINDEX1: shared by LSP and the public `jet-semindex` query API.

use jet_foundation::AST::{self, Item, LoadedModule, ProgramBundle};
use jet_foundation::Diagnostics::Span;
use jet_foundation::Syntax;
use jet_sema::{effect_key, SemIndexEffectFacts};

use crate::Json::{convert_defs, convert_effects, convert_refs};
use crate::Types::{CallEdge, SemIndex};

/// The semantic kind of a defined symbol (LSP-facing; uses AST types internally).
#[derive(Debug, Clone)]
pub enum SymKind {
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
    pub hover: Vec<HoverEntry>,
    pub inlay: Vec<InlayHint>,
}

struct CallerFrame {
    name: String,
    owner: Option<String>,
}

struct WalkCtx<'a> {
    db: &'a mut SymbolDB,
    caller: Option<CallerFrame>,
}

impl SymbolDB {
    pub fn new() -> Self {
        SymbolDB {
            index: SemIndex::new(Vec::new(), Vec::new(), Vec::new(), Vec::new()),
            defs: Vec::new(),
            refs: Vec::new(),
            calls: Vec::new(),
            hover: Vec::new(),
            inlay: Vec::new(),
        }
    }

    fn finalize_index(&mut self, facts: &SemIndexEffectFacts) {
        let defs = convert_defs(&self.defs);
        let refs = convert_refs(&self.refs);
        let effects = convert_effects(facts);
        self.index = SemIndex::new(defs, refs, self.calls.clone(), effects);
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

fn with_caller<F>(ctx: &mut WalkCtx<'_>, frame: CallerFrame, f: F)
where
    F: FnOnce(&mut WalkCtx<'_>),
{
    let prev = ctx.caller.replace(frame);
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
        };
        for item in &module.items {
            collect_item(item, &mp, module, &mut ctx);
        }
    }
    db.finalize_index(facts);
    db
}

/// Public query surface over the same facts (no LSP hover/inlay extras).
pub fn build_index(bundle: &ProgramBundle, facts: &SemIndexEffectFacts) -> SemIndex {
    build_symbol_db(bundle, facts).index
}

fn collect_item(item: &Item, mp: &str, module: &LoadedModule, ctx: &mut WalkCtx<'_>) {
    match item {
        Item::Func(f) => {
            let params: Vec<(String, AST::Type)> = f
                .params
                .iter()
                .filter(|p| p.name != Syntax::KW_SELF)
                .map(|p| (p.name.clone(), p.ty.clone()))
                .collect();
            let sym = SymDef {
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
                },
                |ctx| {
                    collect_stmts(&f.body, mp, module, ctx);
                    if f.is_view_return {
                        collect_view_return_hints(&f.body, mp, ctx);
                    }
                },
            );
        }
        Item::Struct(s) => {
            let fields: Vec<(String, AST::Type)> = s
                .fields
                .iter()
                .map(|f| (f.name.clone(), f.ty.clone()))
                .collect();
            ctx.db.defs.push(SymDef {
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
                    name: f.name.clone(),
                    def_span: f.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Field {
                        ty: f.ty.clone(),
                        parent: s.name.clone(),
                    },
                });
                ctx.db.hover.push(HoverEntry {
                    span: f.name_span,
                    module_path: mp.to_string(),
                    text: format!("`{}`: {} (field of `{}`)", f.name, f.ty.name(), s.name),
                });
            }
            for meth in &s.methods {
                let hover_text = hover_for_fn(meth);
                ctx.db.defs.push(SymDef {
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
                    },
                    |ctx| collect_stmts(body, mp, module, ctx),
                );
            }
            for tb in &s.trait_impls {
                for meth in &tb.methods {
                    with_caller(
                        ctx,
                        CallerFrame {
                            name: meth.name.clone(),
                            owner: Some(s.name.clone()),
                        },
                        |ctx| collect_stmts(&meth.body, mp, module, ctx),
                    );
                }
            }
        }
        Item::Enum(e) => {
            let variants: Vec<String> = e.variants.iter().map(|v| v.name.clone()).collect();
            ctx.db.defs.push(SymDef {
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
                ctx.db.defs.push(SymDef {
                    name: v.name.clone(),
                    def_span: v.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::EnumVariant {
                        parent: e.name.clone(),
                    },
                });
                ctx.db.hover.push(HoverEntry {
                    span: v.name_span,
                    module_path: mp.to_string(),
                    text: format!("`{}` — variant of `{}`", v.name, e.name),
                });
            }
            for meth in &e.methods {
                ctx.db.defs.push(SymDef {
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
                    },
                    |ctx| collect_stmts(&meth.body, mp, module, ctx),
                );
            }
            for tb in &e.trait_impls {
                for meth in &tb.methods {
                    with_caller(
                        ctx,
                        CallerFrame {
                            name: meth.name.clone(),
                            owner: Some(e.name.clone()),
                        },
                        |ctx| collect_stmts(&meth.body, mp, module, ctx),
                    );
                }
            }
        }
        Item::Trait(t) => {
            ctx.db.defs.push(SymDef {
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
                ctx.db.defs.push(SymDef {
                    name: sig.name.clone(),
                    def_span: sig.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Function {
                        params: sig
                            .params
                            .iter()
                            .filter(|p| p.name != Syntax::KW_SELF)
                            .map(|p| (p.name.clone(), p.ty.clone()))
                            .collect(),
                        ret: sig.return_type.clone(),
                    },
                });
            }
        }
        Item::Tag(t) => {
            ctx.db.defs.push(SymDef {
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
                ctx.db.defs.push(SymDef {
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
                    },
                    |ctx| collect_stmts(&meth.body, mp, module, ctx),
                );
            }
        }
        Item::Const(c) => {
            ctx.db.defs.push(SymDef {
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
            collect_expr(&c.value, mp, ctx);
        }
        Item::Test(t) => {
            collect_stmts(&t.body, mp, module, ctx);
        }
        Item::Bench(b) => {
            collect_stmts(&b.body, mp, module, ctx);
        }
        Item::ExternRust(_) => {}
        // Stage 1a: modules aren't yet indexed for symbols/hover.
        Item::Module(_) => {}
        // S59: C FFI boundary modules aren't yet indexed for symbols/hover.
        Item::CModule(_) => {}
        // Code modules aren't yet indexed for symbols/hover.
        Item::CodeModule(_) => {}
        // D-DIST1: distinct types aren't yet indexed for symbols/hover.
        Item::Distinct(_) => {}
        // D-TYPEALIAS1: type aliases aren't yet indexed for symbols/hover.
        Item::TypeAlias(_) => {}
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

/// L2301 (E2-M5, D-REF3): walk a `-> view` function body and, at every
/// `return`, push an advisory inlay hint naming the source the returned view
/// borrows (the parameter or `param.field` it points into) plus a reminder
/// that the borrow lives only as long as that source. On by default — no
/// diagnostic, just a hint, so it never blocks compilation.
fn collect_view_return_hints(stmts: &[AST::Stmt], mp: &str, ctx: &mut WalkCtx<'_>) {
    for stmt in stmts {
        match stmt {
            AST::Stmt::Return(Some(e), _) => {
                if let Some(src) = view_return_source(e) {
                    ctx.db.inlay.push(InlayHint {
                        span: e.span(),
                        module_path: mp.to_string(),
                        label: format!(" borrows `{}` — lives as long as it does", src),
                    });
                }
            }
            AST::Stmt::If(if_stmt) => collect_view_return_hints_if(if_stmt, mp, ctx),
            AST::Stmt::While { body, .. }
            | AST::Stmt::For { body, .. }
            | AST::Stmt::Caps { body, .. }
            | AST::Stmt::Grant { body, .. }
            | AST::Stmt::Region { body, .. }
            | AST::Stmt::TaskGroup { body, .. }
            | AST::Stmt::Transact { body, .. }
            | AST::Stmt::AssumeDet { body, .. }
            | AST::Stmt::CountedLoop { body, .. }
            | AST::Stmt::Loop { body, .. } => collect_view_return_hints(body, mp, ctx),
            AST::Stmt::Switch {
                arms, else_body, ..
            } => {
                for arm in arms {
                    collect_view_return_hints(&arm.body, mp, ctx);
                }
                if let Some(eb) = else_body {
                    collect_view_return_hints(eb, mp, ctx);
                }
            }
            _ => {}
        }
    }
}

fn collect_view_return_hints_if(if_stmt: &AST::IfStmt, mp: &str, ctx: &mut WalkCtx<'_>) {
    collect_view_return_hints(&if_stmt.then_body, mp, ctx);
    match &if_stmt.else_branch {
        Some(AST::ElseBranch::ElseIf(inner)) => collect_view_return_hints_if(inner, mp, ctx),
        Some(AST::ElseBranch::Else(body)) => collect_view_return_hints(body, mp, ctx),
        None => {}
    }
}

/// Name the source a returned `view` borrows: the root identifier of an
/// `Ident` or `Field` path. `None` for shapes that don't read into a name.
fn view_return_source(e: &AST::Expr) -> Option<String> {
    match e {
        AST::Expr::Ident(name, _) => Some(name.clone()),
        AST::Expr::Field(base, _, _) => view_return_source(base),
        _ => None,
    }
}

fn collect_stmt(stmt: &AST::Stmt, mp: &str, module: &LoadedModule, ctx: &mut WalkCtx<'_>) {
    match stmt {
        AST::Stmt::Val(b) => {
            collect_binding(b, mp, ctx);
        }
        AST::Stmt::Expr(e) => collect_expr(e, mp, ctx),
        AST::Stmt::Assign { target, value, .. } => {
            collect_lvalue(target, mp, ctx);
            collect_expr(value, mp, ctx);
        }
        AST::Stmt::Return(e, _) => {
            if let Some(e) = e {
                collect_expr(e, mp, ctx);
            }
        }
        AST::Stmt::If(if_stmt) => {
            collect_if(if_stmt, mp, module, ctx);
        }
        AST::Stmt::While { cond, body, .. } => {
            collect_expr(cond, mp, ctx);
            collect_stmts(body, mp, module, ctx);
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
                    collect_expr(start, mp, ctx);
                    collect_expr(end, mp, ctx);
                    if let Some(step) = step {
                        collect_expr(step, mp, ctx);
                    }
                }
                AST::ForKind::In { collection } => {
                    collect_expr(collection, mp, ctx);
                }
            }
            collect_stmts(body, mp, module, ctx);
        }
        AST::Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        } => {
            collect_expr(subject, mp, ctx);
            for arm in arms {
                collect_expr(&arm.cond, mp, ctx);
                collect_stmts(&arm.body, mp, module, ctx);
            }
            if let Some(eb) = else_body {
                collect_stmts(eb, mp, module, ctx);
            }
        }
        AST::Stmt::CountedLoop {
            cond, body, init, ..
        } => {
            ctx.db.defs.push(SymDef {
                name: init.name.clone(),
                def_span: init.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Local {
                    mutable: true,
                    ty: None,
                },
            });
            collect_expr(&init.init, mp, ctx);
            collect_expr(cond, mp, ctx);
            collect_stmts(body, mp, module, ctx);
        }
        AST::Stmt::Loop { body, .. }
        | AST::Stmt::Unsafe { body, .. }
        | AST::Stmt::Impure { body, .. }
        | AST::Stmt::Reactive { body, .. }
        | AST::Stmt::Region { body, .. }
        | AST::Stmt::TaskGroup { body, .. }
        | AST::Stmt::Caps { body, .. }
        | AST::Stmt::Grant { body, .. }
        | AST::Stmt::Transact { body, .. }
        | AST::Stmt::AssumeDet { body, .. } => {
            collect_stmts(body, mp, module, ctx);
        }
        AST::Stmt::Break(_)
        | AST::Stmt::Continue(_)
        | AST::Stmt::BreakLabel(..)
        | AST::Stmt::ContinueLabel(..) => {}
        // D-CTMARKER1: collect symbols from comptime block body.
        AST::Stmt::ComptimeBlock { body, .. } => collect_stmts(body, mp, module, ctx),
        AST::Stmt::ComptimeIf {
            cond,
            then_body,
            else_body,
            ..
        } => {
            collect_expr(cond, mp, ctx);
            collect_stmts(then_body, mp, module, ctx);
            if let Some(eb) = else_body {
                collect_stmts(eb, mp, module, ctx);
            }
        }
        AST::Stmt::ContextBlock { fields, body, .. } => {
            for (_, e, _) in fields {
                collect_expr(e, mp, ctx);
            }
            collect_stmts(body, mp, module, ctx);
        }
        // D-TERM1 (ratified 2026-06-22): collect symbols from live block body.
        AST::Stmt::Live { body, .. } => {
            collect_stmts(body, mp, module, ctx);
        }
        // D-IGNORERET2=A: collect symbols from suppress-must-use block body.
        AST::Stmt::SuppressMustUse { body, .. } => {
            collect_stmts(body, mp, module, ctx);
        }
    }
}

fn collect_if(if_stmt: &AST::IfStmt, mp: &str, module: &LoadedModule, ctx: &mut WalkCtx<'_>) {
    collect_expr(&if_stmt.cond, mp, ctx);
    collect_stmts(&if_stmt.then_body, mp, module, ctx);
    if let Some(eb) = &if_stmt.else_branch {
        match eb {
            AST::ElseBranch::ElseIf(inner) => collect_if(inner, mp, module, ctx),
            AST::ElseBranch::Else(body) => collect_stmts(body, mp, module, ctx),
        }
    }
}

fn collect_lvalue(lv: &AST::LValue, mp: &str, ctx: &mut WalkCtx<'_>) {
    match lv {
        AST::LValue::Local { name, name_span } => {
            ctx.db.refs.push(SymRef {
                name: name.clone(),
                span: *name_span,
                module_path: mp.to_string(),
            });
        }
        AST::LValue::Index { base, index, .. } => {
            collect_expr(base, mp, ctx);
            collect_expr(index, mp, ctx);
        }
        // D-MUTSELF1: `place.field = v` — record references in the base place.
        AST::LValue::Field { base, .. } => collect_expr(base, mp, ctx),
    }
}

fn collect_binding(b: &AST::Binding, mp: &str, ctx: &mut WalkCtx<'_>) {
    // S74: a destructuring binding brings each named field/element into scope.
    if let Some(pat) = &b.pattern {
        for n in pat.names() {
            ctx.db.defs.push(SymDef {
                name: n.name.clone(),
                def_span: n.span,
                module_path: mp.to_string(),
                kind: SymKind::Local {
                    mutable: b.mutable,
                    ty: None,
                },
            });
        }
        collect_expr(&b.init, mp, ctx);
        return;
    }
    let ty = b.ty.clone();
    // has_explicit_annotation: user wrote `: Type` — ty_span is Some iff the annotation is in source
    let has_explicit = b.ty_span.is_some();
    ctx.db.defs.push(SymDef {
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
    collect_expr(&b.init, mp, ctx);
}

fn collect_expr(e: &AST::Expr, mp: &str, ctx: &mut WalkCtx<'_>) {
    match e {
        AST::Expr::PtrFromAddr { addr, .. } => collect_expr(addr, mp, ctx),
        AST::Expr::Ident(name, span) => {
            ctx.db.refs.push(SymRef {
                name: name.clone(),
                span: *span,
                module_path: mp.to_string(),
            });
        }
        AST::Expr::Call(call) => {
            record_call(ctx, mp, &call.name, call.name_span);
            ctx.db.refs.push(SymRef {
                name: call.name.clone(),
                span: call.name_span,
                module_path: mp.to_string(),
            });
            for arg in &call.args {
                collect_expr(&arg.expr, mp, ctx);
            }
        }
        AST::Expr::MethodCall {
            receiver,
            method,
            method_span,
            args,
            ..
        } => {
            collect_expr(receiver, mp, ctx);
            record_call(ctx, mp, method, *method_span);
            ctx.db.refs.push(SymRef {
                name: method.clone(),
                span: *method_span,
                module_path: mp.to_string(),
            });
            for arg in args {
                collect_expr(&arg.expr, mp, ctx);
            }
        }
        AST::Expr::Field(base, field, span) => {
            collect_expr(base, mp, ctx);
            ctx.db.refs.push(SymRef {
                name: field.clone(),
                span: *span,
                module_path: mp.to_string(),
            });
        }
        AST::Expr::OptField {
            base,
            member,
            member_span,
            ..
        } => {
            collect_expr(base, mp, ctx);
            ctx.db.refs.push(SymRef {
                name: member.clone(),
                span: *member_span,
                module_path: mp.to_string(),
            });
        }
        AST::Expr::Binary(_, l, r, _) => {
            collect_expr(l, mp, ctx);
            collect_expr(r, mp, ctx);
        }
        AST::Expr::Unary(_, inner, _) => collect_expr(inner, mp, ctx),
        AST::Expr::Deref(inner, _) | AST::Expr::RawOf(inner, _) => collect_expr(inner, mp, ctx),
        AST::Expr::Index { base, index, .. } => {
            collect_expr(base, mp, ctx);
            collect_expr(index, mp, ctx);
        }
        AST::Expr::Slice {
            base, start, end, ..
        } => {
            collect_expr(base, mp, ctx);
            collect_expr(start, mp, ctx);
            collect_expr(end, mp, ctx);
        }
        AST::Expr::Str(parts, _) => {
            for part in parts {
                if let AST::StrPart::Interp(inner) = part {
                    collect_expr(inner, mp, ctx);
                }
            }
        }
        AST::Expr::ListLit(items, _) => {
            for i in items {
                collect_expr(i, mp, ctx);
            }
        }
        AST::Expr::Spread(inner, _) => collect_expr(inner, mp, ctx),
        AST::Expr::MapLit(pairs, _) => {
            for (k, v) in pairs {
                collect_expr(k, mp, ctx);
                collect_expr(v, mp, ctx);
            }
        }
        AST::Expr::TupleLit(fields, _, _) => {
            for (_, expr) in fields {
                collect_expr(expr, mp, ctx);
            }
        }
        AST::Expr::StructLit { fields, .. } => {
            for (_, _, expr) in fields {
                collect_expr(expr, mp, ctx);
            }
        }
        AST::Expr::EnumLit { args, .. } => {
            for arg in args {
                match arg {
                    AST::EnumLitArg::Positional(e) => collect_expr(e, mp, ctx),
                    AST::EnumLitArg::Named { expr, .. } => collect_expr(expr, mp, ctx),
                }
            }
        }
        AST::Expr::Tainted(inner, _) // D-TAINT1: tag erased; recurse into the value.
        | AST::Expr::Present(inner, _)
        | AST::Expr::Ok(inner, _)
        | AST::Expr::Err(inner, _)
        | AST::Expr::Try(inner, _, _) => collect_expr(inner, mp, ctx),
        AST::Expr::OrFallback {
            value, fallback, ..
        } => {
            collect_expr(value, mp, ctx);
            match fallback {
                AST::OrFallback::Value(v) => collect_expr(v, mp, ctx),
                AST::OrFallback::Panic { args, .. } => {
                    for a in args {
                        collect_expr(&a.expr, mp, ctx);
                    }
                }
                AST::OrFallback::Return(v, _) => {
                    if let Some(v) = v {
                        collect_expr(v, mp, ctx);
                    }
                }
                AST::OrFallback::Break(_) | AST::OrFallback::Continue(_) => {}
            }
        }
        AST::Expr::PatternTest { subject, .. } => collect_expr(subject, mp, ctx),
        AST::Expr::Lambda(l) => {
            for p in &l.params {
                ctx.db.defs.push(SymDef {
                    name: p.name.clone(),
                    def_span: p.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Param {
                        ty: p.ty.clone().unwrap_or(AST::Type::Int),
                    },
                });
            }
            match &l.body {
                AST::LambdaBody::Expr(e) => collect_expr(e, mp, ctx),
                AST::LambdaBody::Block(stmts) => {
                    for s in stmts {
                        if let AST::Stmt::Val(b) = s {
                            collect_binding(b, mp, ctx);
                        } else {
                            collect_expr_stmt(s, mp, ctx);
                        }
                    }
                }
            }
        }
        AST::Expr::CallValue { callee, args, .. } => {
            collect_expr(callee, mp, ctx);
            for a in args {
                collect_expr(&a.expr, mp, ctx);
            }
        }
        AST::Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            collect_expr(cond, mp, ctx);
            for s in then_body.iter().chain(else_body.iter()) {
                if let AST::Stmt::Val(b) = s {
                    collect_binding(b, mp, ctx);
                } else {
                    collect_expr_stmt(s, mp, ctx);
                }
            }
            collect_expr(then_value, mp, ctx);
            collect_expr(else_value, mp, ctx);
        }
        AST::Expr::FanOut { callee, items, .. } => {
            collect_expr(callee, mp, ctx);
            for item in items {
                collect_expr(item, mp, ctx);
            }
        }
        AST::Expr::Int(_, _, _)
        | AST::Expr::Float(_, _, _)
        | AST::Expr::Bool(_, _)
        | AST::Expr::Char(_, _)
        | AST::Expr::Absent(_)
        | AST::Expr::ReduceMarker(_, _)
        | AST::Expr::Todo { .. }
        | AST::Expr::ComptimeSplice { .. } => {}
        AST::Expr::Paren(inner, _) => collect_expr(inner, mp, ctx),
    }
}

fn collect_expr_stmt(stmt: &AST::Stmt, mp: &str, ctx: &mut WalkCtx<'_>) {
    if let AST::Stmt::Expr(e) = stmt {
        collect_expr(e, mp, ctx);
    }
}
