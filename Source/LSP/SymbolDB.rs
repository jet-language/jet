//! Symbol DB — built by walking the ProgramBundle's AST after parsing +
//! type-checking. The server builds this DB on every document check and uses
//! it for completion, hover, go-to-def, references, rename, and inlay hints.
//! (LSP-I1: no new "language knowledge" here — just indexing what sema knows.)

use crate::AST::{self, Item, LoadedModule, ProgramBundle};
use crate::Diagnostics::Span;

/// The semantic kind of a defined symbol.
#[derive(Debug, Clone)]
pub(crate) enum SymKind {
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
pub(crate) struct SymDef {
    pub(crate) name: String,
    pub(crate) def_span: Span,
    pub(crate) module_path: String,
    pub(crate) kind: SymKind,
}

/// One use-site reference (identifier occurrence).
#[derive(Debug, Clone)]
pub(crate) struct SymRef {
    pub(crate) name: String,
    pub(crate) span: Span,
    pub(crate) module_path: String,
}

/// Hover entry: an expression/token span + text to show on hover.
#[derive(Debug, Clone)]
pub(crate) struct HoverEntry {
    pub(crate) span: Span,
    pub(crate) module_path: String,
    pub(crate) text: String,
}

/// Inlay hint: position (just past the binding name) + type text to show.
#[derive(Debug, Clone)]
pub(crate) struct InlayHint {
    pub(crate) span: Span,
    pub(crate) module_path: String,
    pub(crate) label: String,
}

/// The full symbol index for a checked program bundle.
pub(crate) struct SymbolDB {
    pub(crate) defs: Vec<SymDef>,
    pub(crate) refs: Vec<SymRef>,
    pub(crate) hover: Vec<HoverEntry>,
    pub(crate) inlay: Vec<InlayHint>,
}

impl SymbolDB {
    pub(crate) fn new() -> Self {
        SymbolDB {
            defs: Vec::new(),
            refs: Vec::new(),
            hover: Vec::new(),
            inlay: Vec::new(),
        }
    }

    /// Find definition(s) of `name` (all modules).
    #[allow(dead_code)] // wired in M13 (LSP go-to-def)
    fn find_def(&self, name: &str) -> Option<&SymDef> {
        self.defs.iter().find(|d| d.name == name)
    }

    /// Find the definition whose def_span contains `offset` in module at `path`.
    fn def_at_offset(&self, path: &str, offset: usize) -> Option<&SymDef> {
        self.defs.iter().find(|d| {
            d.module_path == path && d.def_span.start <= offset && offset <= d.def_span.end
        })
    }

    /// Find any ref or def name at `offset` in module `path`.
    #[allow(dead_code)] // wired in M13 (LSP rename / hover)
    fn name_at_offset(&self, path: &str, offset: usize) -> Option<&str> {
        // Check defs first
        if let Some(d) = self.def_at_offset(path, offset) {
            return Some(&d.name);
        }
        // Check refs
        self.refs
            .iter()
            .find(|r| r.module_path == path && r.span.start <= offset && offset <= r.span.end)
            .map(|r| r.name.as_str())
    }

    /// All references to `name` across all modules.
    #[allow(dead_code)] // wired in M13 (LSP find-all-references)
    fn all_refs(&self, name: &str) -> Vec<&SymRef> {
        self.refs.iter().filter(|r| r.name == name).collect()
    }

    /// Hover text for the symbol at `offset` in `path`.
    pub(crate) fn hover_at(&self, path: &str, offset: usize) -> Option<&str> {
        self.hover
            .iter()
            .find(|h| h.module_path == path && h.span.start <= offset && offset <= h.span.end)
            .map(|h| h.text.as_str())
    }

    /// All inlay hints for a module path.
    pub(crate) fn inlay_hints_for(&self, path: &str) -> Vec<&InlayHint> {
        self.inlay
            .iter()
            .filter(|h| h.module_path == path)
            .collect()
    }
}

/// Build the symbol DB by walking all items in a bundle.
pub(crate) fn build_symbol_db(bundle: &ProgramBundle) -> SymbolDB {
    let mut db = SymbolDB::new();
    for module in &bundle.modules {
        let mp = module.display.clone();
        for item in &module.items {
            collect_item(item, &mp, module, &mut db);
        }
    }
    db
}

fn collect_item(item: &Item, mp: &str, module: &LoadedModule, db: &mut SymbolDB) {
    match item {
        Item::Func(f) => {
            let params: Vec<(String, AST::Type)> = f
                .params
                .iter()
                .filter(|p| p.name != crate::Syntax::KW_SELF)
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
            db.hover.push(HoverEntry {
                span: f.name_span,
                module_path: mp.to_string(),
                text: hover_text,
            });
            db.defs.push(sym);
            // param defs
            for p in &f.params {
                if p.name == crate::Syntax::KW_SELF {
                    continue;
                }
                db.defs.push(SymDef {
                    name: p.name.clone(),
                    def_span: p.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Param { ty: p.ty.clone() },
                });
                db.hover.push(HoverEntry {
                    span: p.name_span,
                    module_path: mp.to_string(),
                    text: format!("`{}`: {}", p.name, p.ty.name()),
                });
            }
            collect_stmts(&f.body, mp, module, db);
            // L2301 (E2-M5, D-REF3): in a `-> view` function, every `return`
            // hands back a borrow. Surface an advisory inlay naming the source
            // the view points into, so the borrow is visible without reading
            // the signature. On by default beyond the clone hints.
            if f.is_view_return {
                collect_view_return_hints(&f.body, mp, db);
            }
        }
        Item::Struct(s) => {
            let fields: Vec<(String, AST::Type)> = s
                .fields
                .iter()
                .map(|f| (f.name.clone(), f.ty.clone()))
                .collect();
            db.defs.push(SymDef {
                name: s.name.clone(),
                def_span: s.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Struct {
                    fields: fields.clone(),
                },
            });
            db.hover.push(HoverEntry {
                span: s.name_span,
                module_path: mp.to_string(),
                text: format!("struct `{}`", s.name),
            });
            for f in &s.fields {
                db.defs.push(SymDef {
                    name: f.name.clone(),
                    def_span: f.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Field {
                        ty: f.ty.clone(),
                        parent: s.name.clone(),
                    },
                });
                db.hover.push(HoverEntry {
                    span: f.name_span,
                    module_path: mp.to_string(),
                    text: format!("`{}`: {} (field of `{}`)", f.name, f.ty.name(), s.name),
                });
            }
            for meth in &s.methods {
                let hover_text = hover_for_fn(meth);
                db.defs.push(SymDef {
                    name: meth.name.clone(),
                    def_span: meth.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Function {
                        params: meth
                            .params
                            .iter()
                            .filter(|p| p.name != crate::Syntax::KW_SELF)
                            .map(|p| (p.name.clone(), p.ty.clone()))
                            .collect(),
                        ret: meth.return_type.clone(),
                    },
                });
                db.hover.push(HoverEntry {
                    span: meth.name_span,
                    module_path: mp.to_string(),
                    text: hover_text,
                });
                for p in &meth.params {
                    if p.name != crate::Syntax::KW_SELF {
                        db.defs.push(SymDef {
                            name: p.name.clone(),
                            def_span: p.name_span,
                            module_path: mp.to_string(),
                            kind: SymKind::Param { ty: p.ty.clone() },
                        });
                    }
                }
                collect_stmts(&meth.body, mp, module, db);
            }
            for tb in &s.trait_impls {
                for meth in &tb.methods {
                    collect_stmts(&meth.body, mp, module, db);
                }
            }
        }
        Item::Enum(e) => {
            let variants: Vec<String> = e.variants.iter().map(|v| v.name.clone()).collect();
            db.defs.push(SymDef {
                name: e.name.clone(),
                def_span: e.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Enum {
                    variants: variants.clone(),
                },
            });
            db.hover.push(HoverEntry {
                span: e.name_span,
                module_path: mp.to_string(),
                text: format!("enum `{}` — variants: {}", e.name, variants.join(", ")),
            });
            for v in &e.variants {
                db.defs.push(SymDef {
                    name: v.name.clone(),
                    def_span: v.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::EnumVariant {
                        parent: e.name.clone(),
                    },
                });
                db.hover.push(HoverEntry {
                    span: v.name_span,
                    module_path: mp.to_string(),
                    text: format!("`{}` — variant of `{}`", v.name, e.name),
                });
            }
            for meth in &e.methods {
                db.defs.push(SymDef {
                    name: meth.name.clone(),
                    def_span: meth.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Function {
                        params: meth
                            .params
                            .iter()
                            .filter(|p| p.name != crate::Syntax::KW_SELF)
                            .map(|p| (p.name.clone(), p.ty.clone()))
                            .collect(),
                        ret: meth.return_type.clone(),
                    },
                });
                collect_stmts(&meth.body, mp, module, db);
            }
            for tb in &e.trait_impls {
                for meth in &tb.methods {
                    collect_stmts(&meth.body, mp, module, db);
                }
            }
        }
        Item::Trait(t) => {
            db.defs.push(SymDef {
                name: t.name.clone(),
                def_span: t.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Trait,
            });
            db.hover.push(HoverEntry {
                span: t.name_span,
                module_path: mp.to_string(),
                text: format!("trait `{}`", t.name),
            });
            for sig in &t.methods {
                db.defs.push(SymDef {
                    name: sig.name.clone(),
                    def_span: sig.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Function {
                        params: sig
                            .params
                            .iter()
                            .filter(|p| p.name != crate::Syntax::KW_SELF)
                            .map(|p| (p.name.clone(), p.ty.clone()))
                            .collect(),
                        ret: sig.return_type.clone(),
                    },
                });
            }
        }
        Item::Tag(t) => {
            db.defs.push(SymDef {
                name: t.name.clone(),
                def_span: t.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Tag,
            });
            db.hover.push(HoverEntry {
                span: t.name_span,
                module_path: mp.to_string(),
                text: format!("tag `{}`", t.name),
            });
        }
        Item::Impl(i) => {
            for meth in &i.methods {
                db.defs.push(SymDef {
                    name: meth.name.clone(),
                    def_span: meth.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Function {
                        params: meth
                            .params
                            .iter()
                            .filter(|p| p.name != crate::Syntax::KW_SELF)
                            .map(|p| (p.name.clone(), p.ty.clone()))
                            .collect(),
                        ret: meth.return_type.clone(),
                    },
                });
                for p in &meth.params {
                    if p.name != crate::Syntax::KW_SELF {
                        db.defs.push(SymDef {
                            name: p.name.clone(),
                            def_span: p.name_span,
                            module_path: mp.to_string(),
                            kind: SymKind::Param { ty: p.ty.clone() },
                        });
                    }
                }
                collect_stmts(&meth.body, mp, module, db);
            }
        }
        Item::Const(c) => {
            db.defs.push(SymDef {
                name: c.name.clone(),
                def_span: c.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Const,
            });
            db.hover.push(HoverEntry {
                span: c.name_span,
                module_path: mp.to_string(),
                text: format!("const `{}`", c.name),
            });
            collect_expr(&c.value, mp, db);
        }
        Item::Test(t) => {
            collect_stmts(&t.body, mp, module, db);
        }
        Item::Bench(b) => {
            collect_stmts(&b.body, mp, module, db);
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
        // D-QUAL3: unit families aren't yet indexed for symbols/hover.
        Item::UnitFamily(_) => {}
        // D-ERR-CONV: error conversions aren't yet indexed for symbols/hover.
        Item::ErrorConv(_) => {}
        // D-MIGRATE1: migration blocks aren't yet indexed for symbols/hover.
        Item::Migration(_) => {}
    }
}

fn hover_for_fn(f: &AST::Func) -> String {
    let params: Vec<String> = f
        .params
        .iter()
        .filter(|p| p.name != crate::Syntax::KW_SELF)
        .map(|p| format!("{}: {}", p.name, p.ty.name()))
        .collect();
    let ret = match &f.return_type {
        Some(t) => format!(" -> {}", t.name()),
        None => String::new(),
    };
    format!("fn {}({}){}", f.name, params.join(", "), ret)
}

fn collect_stmts(stmts: &[AST::Stmt], mp: &str, module: &LoadedModule, db: &mut SymbolDB) {
    for stmt in stmts {
        collect_stmt(stmt, mp, module, db);
    }
}

/// L2301 (E2-M5, D-REF3): walk a `-> view` function body and, at every
/// `return`, push an advisory inlay hint naming the source the returned view
/// borrows (the parameter or `param.field` it points into) plus a reminder
/// that the borrow lives only as long as that source. On by default — no
/// diagnostic, just a hint, so it never blocks compilation.
fn collect_view_return_hints(stmts: &[AST::Stmt], mp: &str, db: &mut SymbolDB) {
    for stmt in stmts {
        match stmt {
            AST::Stmt::Return(Some(e), _) => {
                if let Some(src) = view_return_source(e) {
                    db.inlay.push(InlayHint {
                        span: e.span(),
                        module_path: mp.to_string(),
                        label: format!(" borrows `{}` — lives as long as it does", src),
                    });
                }
            }
            AST::Stmt::If(if_stmt) => collect_view_return_hints_if(if_stmt, mp, db),
            AST::Stmt::While { body, .. }
            | AST::Stmt::For { body, .. }
            | AST::Stmt::Caps { body, .. }
            | AST::Stmt::Grant { body, .. }
            | AST::Stmt::Region { body, .. }
            | AST::Stmt::Transact { body, .. }
            | AST::Stmt::AssumeDet { body, .. }
            | AST::Stmt::Loop { body, .. } => collect_view_return_hints(body, mp, db),
            AST::Stmt::Switch {
                arms, else_body, ..
            } => {
                for arm in arms {
                    collect_view_return_hints(&arm.body, mp, db);
                }
                if let Some(eb) = else_body {
                    collect_view_return_hints(eb, mp, db);
                }
            }
            _ => {}
        }
    }
}

fn collect_view_return_hints_if(if_stmt: &AST::IfStmt, mp: &str, db: &mut SymbolDB) {
    collect_view_return_hints(&if_stmt.then_body, mp, db);
    match &if_stmt.else_branch {
        Some(AST::ElseBranch::ElseIf(inner)) => collect_view_return_hints_if(inner, mp, db),
        Some(AST::ElseBranch::Else(body)) => collect_view_return_hints(body, mp, db),
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

fn collect_stmt(stmt: &AST::Stmt, mp: &str, module: &LoadedModule, db: &mut SymbolDB) {
    match stmt {
        AST::Stmt::Val(b) => {
            collect_binding(b, mp, db);
        }
        AST::Stmt::Expr(e) => collect_expr(e, mp, db),
        AST::Stmt::Assign { target, value, .. } => {
            collect_lvalue(target, mp, db);
            collect_expr(value, mp, db);
        }
        AST::Stmt::Return(e, _) => {
            if let Some(e) = e {
                collect_expr(e, mp, db);
            }
        }
        AST::Stmt::If(if_stmt) => {
            collect_if(if_stmt, mp, module, db);
        }
        AST::Stmt::While { cond, body, .. } => {
            collect_expr(cond, mp, db);
            collect_stmts(body, mp, module, db);
        }
        AST::Stmt::For {
            var,
            var_span,
            var2,
            kind,
            body,
            ..
        } => {
            db.defs.push(SymDef {
                name: var.clone(),
                def_span: *var_span,
                module_path: mp.to_string(),
                kind: SymKind::Local {
                    mutable: false,
                    ty: None,
                },
            });
            if let Some((v2, s2)) = var2 {
                db.defs.push(SymDef {
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
                    collect_expr(start, mp, db);
                    collect_expr(end, mp, db);
                    if let Some(step) = step {
                        collect_expr(step, mp, db);
                    }
                }
                AST::ForKind::In { collection } => {
                    collect_expr(collection, mp, db);
                }
            }
            collect_stmts(body, mp, module, db);
        }
        AST::Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        } => {
            collect_expr(subject, mp, db);
            for arm in arms {
                collect_expr(&arm.cond, mp, db);
                collect_stmts(&arm.body, mp, module, db);
            }
            if let Some(eb) = else_body {
                collect_stmts(eb, mp, module, db);
            }
        }
        AST::Stmt::Loop { body, .. }
        | AST::Stmt::Unsafe { body, .. }
        | AST::Stmt::Region { body, .. }
        | AST::Stmt::Caps { body, .. }
        | AST::Stmt::Grant { body, .. }
        | AST::Stmt::Transact { body, .. }
        | AST::Stmt::AssumeDet { body, .. } => {
            collect_stmts(body, mp, module, db);
        }
        AST::Stmt::Break(_)
        | AST::Stmt::Continue(_)
        | AST::Stmt::BreakLabel(..)
        | AST::Stmt::ContinueLabel(..) => {}
        AST::Stmt::ComptimeIf { cond, then_body, else_body, .. } => {
            collect_expr(cond, mp, db);
            collect_stmts(then_body, mp, module, db);
            if let Some(eb) = else_body {
                collect_stmts(eb, mp, module, db);
            }
        }
        AST::Stmt::ContextBlock { fields, body, .. } => {
            for (_, e, _) in fields {
                collect_expr(e, mp, db);
            }
            collect_stmts(body, mp, module, db);
        }
        // D-TERM1 (ratified 2026-06-22): collect symbols from live block body.
        AST::Stmt::Live { body, .. } => {
            collect_stmts(body, mp, module, db);
        }
    }
}

fn collect_if(if_stmt: &AST::IfStmt, mp: &str, module: &LoadedModule, db: &mut SymbolDB) {
    collect_expr(&if_stmt.cond, mp, db);
    collect_stmts(&if_stmt.then_body, mp, module, db);
    if let Some(eb) = &if_stmt.else_branch {
        match eb {
            AST::ElseBranch::ElseIf(inner) => collect_if(inner, mp, module, db),
            AST::ElseBranch::Else(body) => collect_stmts(body, mp, module, db),
        }
    }
}

fn collect_lvalue(lv: &AST::LValue, mp: &str, db: &mut SymbolDB) {
    match lv {
        AST::LValue::Local { name, name_span } => {
            db.refs.push(SymRef {
                name: name.clone(),
                span: *name_span,
                module_path: mp.to_string(),
            });
        }
        AST::LValue::Index { base, index, .. } => {
            collect_expr(base, mp, db);
            collect_expr(index, mp, db);
        }
        // D-MUTSELF1: `place.field = v` — record references in the base place.
        AST::LValue::Field { base, .. } => collect_expr(base, mp, db),
    }
}

fn collect_binding(b: &AST::Binding, mp: &str, db: &mut SymbolDB) {
    // S74: a destructuring binding brings each named field/element into scope.
    if let Some(pat) = &b.pattern {
        for n in pat.names() {
            db.defs.push(SymDef {
                name: n.name.clone(),
                def_span: n.span,
                module_path: mp.to_string(),
                kind: SymKind::Local {
                    mutable: b.mutable,
                    ty: None,
                },
            });
        }
        collect_expr(&b.init, mp, db);
        return;
    }
    let ty = b.ty.clone();
    // has_explicit_annotation: user wrote `: Type` — ty_span is Some iff the annotation is in source
    let has_explicit = b.ty_span.is_some();
    db.defs.push(SymDef {
        name: b.name.clone(),
        def_span: b.name_span,
        module_path: mp.to_string(),
        kind: SymKind::Local {
            mutable: b.mutable,
            ty: ty.clone(),
        },
    });
    if let Some(t) = &ty {
        let kw = if b.mutable { "var" } else { "val" };
        db.hover.push(HoverEntry {
            span: b.name_span,
            module_path: mp.to_string(),
            text: format!("`{}`: {} ({})", b.name, t.name(), kw),
        });
        // Inlay hint: only when user omitted the annotation (sema filled it in)
        if !has_explicit {
            db.inlay.push(InlayHint {
                span: b.name_span,
                module_path: mp.to_string(),
                label: format!(": {}", t.name()),
            });
        }
    }
    collect_expr(&b.init, mp, db);
}

fn collect_expr(e: &AST::Expr, mp: &str, db: &mut SymbolDB) {
    match e {
        AST::Expr::PtrFromAddr { addr, .. } => collect_expr(addr, mp, db),
        AST::Expr::Ident(name, span) => {
            db.refs.push(SymRef {
                name: name.clone(),
                span: *span,
                module_path: mp.to_string(),
            });
        }
        AST::Expr::Call(call) => {
            db.refs.push(SymRef {
                name: call.name.clone(),
                span: call.name_span,
                module_path: mp.to_string(),
            });
            for arg in &call.args {
                collect_expr(&arg.expr, mp, db);
            }
        }
        AST::Expr::MethodCall {
            receiver,
            method,
            method_span,
            args,
            ..
        } => {
            collect_expr(receiver, mp, db);
            db.refs.push(SymRef {
                name: method.clone(),
                span: *method_span,
                module_path: mp.to_string(),
            });
            for arg in args {
                collect_expr(&arg.expr, mp, db);
            }
        }
        AST::Expr::Field(base, field, span) => {
            collect_expr(base, mp, db);
            db.refs.push(SymRef {
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
            collect_expr(base, mp, db);
            db.refs.push(SymRef {
                name: member.clone(),
                span: *member_span,
                module_path: mp.to_string(),
            });
        }
        AST::Expr::Binary(_, l, r, _) => {
            collect_expr(l, mp, db);
            collect_expr(r, mp, db);
        }
        AST::Expr::Unary(_, inner, _) => collect_expr(inner, mp, db),
        AST::Expr::Deref(inner, _) | AST::Expr::RawOf(inner, _) => collect_expr(inner, mp, db),
        AST::Expr::Index { base, index, .. } => {
            collect_expr(base, mp, db);
            collect_expr(index, mp, db);
        }
        AST::Expr::Slice {
            base, start, end, ..
        } => {
            collect_expr(base, mp, db);
            collect_expr(start, mp, db);
            collect_expr(end, mp, db);
        }
        AST::Expr::Str(parts, _) => {
            for part in parts {
                if let AST::StrPart::Interp(inner) = part {
                    collect_expr(inner, mp, db);
                }
            }
        }
        AST::Expr::ListLit(items, _) => {
            for i in items {
                collect_expr(i, mp, db);
            }
        }
        AST::Expr::MapLit(pairs, _) => {
            for (k, v) in pairs {
                collect_expr(k, mp, db);
                collect_expr(v, mp, db);
            }
        }
        AST::Expr::TupleLit(fields, _, _) => {
            for (_, expr) in fields {
                collect_expr(expr, mp, db);
            }
        }
        AST::Expr::StructLit { fields, .. } => {
            for (_, _, expr) in fields {
                collect_expr(expr, mp, db);
            }
        }
        AST::Expr::EnumLit { args, .. } => {
            for arg in args {
                match arg {
                    AST::EnumLitArg::Positional(e) => collect_expr(e, mp, db),
                    AST::EnumLitArg::Named { expr, .. } => collect_expr(expr, mp, db),
                }
            }
        }
        AST::Expr::Tainted(inner, _) // D-TAINT1: tag erased; recurse into the value.
        | AST::Expr::Present(inner, _)
        | AST::Expr::Ok(inner, _)
        | AST::Expr::Err(inner, _)
        | AST::Expr::Try(inner, _, _) => collect_expr(inner, mp, db),
        AST::Expr::OrFallback {
            value, fallback, ..
        } => {
            collect_expr(value, mp, db);
            match fallback {
                AST::OrFallback::Value(v) => collect_expr(v, mp, db),
                AST::OrFallback::Panic { args, .. } => {
                    for a in args {
                        collect_expr(&a.expr, mp, db);
                    }
                }
                AST::OrFallback::Return(v, _) => {
                    if let Some(v) = v {
                        collect_expr(v, mp, db);
                    }
                }
            }
        }
        AST::Expr::PatternTest { subject, .. } => collect_expr(subject, mp, db),
        AST::Expr::Lambda(l) => {
            for p in &l.params {
                db.defs.push(SymDef {
                    name: p.name.clone(),
                    def_span: p.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Param {
                        ty: p.ty.clone().unwrap_or(AST::Type::Int),
                    },
                });
            }
            match &l.body {
                AST::LambdaBody::Expr(e) => collect_expr(e, mp, db),
                AST::LambdaBody::Block(stmts) => {
                    for s in stmts {
                        if let AST::Stmt::Val(b) = s {
                            collect_binding(b, mp, db);
                        } else {
                            collect_expr_stmt(s, mp, db);
                        }
                    }
                }
            }
        }
        AST::Expr::CallValue { callee, args, .. } => {
            collect_expr(callee, mp, db);
            for a in args {
                collect_expr(&a.expr, mp, db);
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
            collect_expr(cond, mp, db);
            for s in then_body.iter().chain(else_body.iter()) {
                if let AST::Stmt::Val(b) = s {
                    collect_binding(b, mp, db);
                } else {
                    collect_expr_stmt(s, mp, db);
                }
            }
            collect_expr(then_value, mp, db);
            collect_expr(else_value, mp, db);
        }
        AST::Expr::FanOut { callee, items, .. } => {
            collect_expr(callee, mp, db);
            for item in items {
                collect_expr(item, mp, db);
            }
        }
        AST::Expr::Int(_, _, _)
        | AST::Expr::Float(_, _, _)
        | AST::Expr::Bool(_, _)
        | AST::Expr::Char(_, _)
        | AST::Expr::Absent(_)
        | AST::Expr::ReduceMarker(_, _)
        | AST::Expr::Todo { .. } => {}
    }
}

fn collect_expr_stmt(stmt: &AST::Stmt, mp: &str, db: &mut SymbolDB) {
    if let AST::Stmt::Expr(e) = stmt {
        collect_expr(e, mp, db);
    }
}
