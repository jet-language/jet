//! Sema-owned source-reference targets for refactoring consumers.
//!
//! Runs after ordinary bundle checking, over checked AST nodes (including
//! receiver types written by method resolution). This is deliberately inside
//! sema: semantic-index consumers receive immutable facts and perform no name
//! lookup of their own.

use super::Effects::DefinitionAnchorFact;
use crate::AST::{self, ElseBranch, Expr, ForKind, Item, LambdaBody, LValue, ProgramBundle, Stmt};
use crate::Diagnostics::Span;
use std::collections::HashMap;

pub(super) fn collect(bundle: &ProgramBundle) -> HashMap<(String, usize, usize), DefinitionAnchorFact> {
    let mut out = HashMap::new();
    for (module_index, module) in bundle.modules.iter().enumerate() {
        let mut resolver = Resolver {
            bundle,
            module_path: module.display.clone(),
            globals: globals_for(bundle, module_index),
            scopes: vec![HashMap::new()],
            aliases: import_aliases(bundle, module_index),
            out: &mut out,
        };
        for item in &module.items { resolver.item(item); }
    }
    out
}

struct Resolver<'a> {
    bundle: &'a ProgramBundle,
    module_path: String,
    globals: HashMap<String, DefinitionAnchorFact>,
    scopes: Vec<HashMap<String, DefinitionAnchorFact>>,
    aliases: HashMap<String, (DefinitionAnchorFact, Option<usize>)>,
    out: &'a mut HashMap<(String, usize, usize), DefinitionAnchorFact>,
}

fn anchor(path: &str, kind: &str, span: Span) -> DefinitionAnchorFact {
    DefinitionAnchorFact { module_path: path.to_string(), kind: kind.to_string(), def_span: span }
}

fn item_definition(item: &Item, path: &str) -> Option<(String, DefinitionAnchorFact)> {
    let (name, kind, span) = match item {
        Item::Func(x) => (&x.name, "function", x.name_span),
        Item::Struct(x) => (&x.name, "struct", x.name_span),
        Item::Enum(x) => (&x.name, "enum", x.name_span),
        Item::Distinct(x) => (&x.name, "distinct", x.name_span),
        Item::TypeAlias(x) => (&x.name, "type_alias", x.name_span),
        Item::Trait(x) => (&x.name, "trait", x.name_span),
        Item::Tag(x) => (&x.name, "tag", x.name_span),
        Item::Const(x) => (&x.name, "const", x.name_span),
        Item::CodeModule(x) => (&x.name, "module", x.name_span),
        Item::GenericModule(x) => (&x.name, "module", x.name_span),
        Item::ModuleAlias(x) => (&x.name, "module_alias", x.name_span),
        _ => return None,
    };
    Some((name.clone(), anchor(path, kind, span)))
}

fn globals_for(bundle: &ProgramBundle, module_index: usize) -> HashMap<String, DefinitionAnchorFact> {
    let module = &bundle.modules[module_index];
    let mut globals = module.items.iter().filter_map(|item| item_definition(item, &module.display)).collect::<HashMap<_, _>>();
    for import in &module.imports {
        if let AST::ImportKind::Unqualified { module_alias, items, .. } = &import.kind {
            let target_index = bundle.import_targets.get(&(module_index, import.span)).copied().or_else(|| {
                module.imports.iter().find(|candidate| {
                    !matches!(candidate.kind, AST::ImportKind::Unqualified { .. })
                        && candidate.import_alias() == *module_alias
                }).and_then(|candidate| bundle.import_targets.get(&(module_index, candidate.span)).copied())
            });
            if let Some(target_index) = target_index {
                let target = &bundle.modules[target_index];
                for (original, alias) in items {
                    let local = alias.as_ref().unwrap_or(original);
                    if let Some((_, definition)) = target.items.iter().filter_map(|item| item_definition(item, &target.display)).find(|(candidate, _)| candidate == original) {
                        globals.insert(local.clone(), definition);
                    }
                }
            }
        }
    }
    globals
}

fn import_aliases(bundle: &ProgramBundle, module_index: usize) -> HashMap<String, (DefinitionAnchorFact, Option<usize>)> {
    let module = &bundle.modules[module_index];
    module.imports.iter().filter_map(|import| {
        if matches!(import.kind, AST::ImportKind::Unqualified { .. }) { return None; }
        let alias = import.import_alias();
        let target = bundle.import_targets.get(&(module_index, import.span)).copied();
        Some((alias, (anchor(&module.display, "import_alias", import.alias_span), target)))
    }).collect()
}

impl Resolver<'_> {
    fn bind(&mut self, name: &str, kind: &str, span: Span) {
        self.scopes.last_mut().unwrap().insert(name.to_string(), anchor(&self.module_path, kind, span));
    }
    fn resolve(&self, name: &str) -> Option<DefinitionAnchorFact> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name).cloned())
            .or_else(|| self.globals.get(name).cloned())
            .or_else(|| self.aliases.get(name).map(|entry| entry.0.clone()))
    }
    fn record(&mut self, name: &str, span: Span) {
        if let Some(definition) = self.resolve(name) {
            self.out.insert((self.module_path.clone(), span.start, span.end), definition);
        }
    }
    fn member_target(&self, receiver_type: Option<&str>, receiver: &Expr, member: &str) -> Option<DefinitionAnchorFact> {
        if let Expr::Ident(alias, _) = receiver {
            if let Some((_, Some(module_index))) = self.aliases.get(alias) {
                let module = &self.bundle.modules[*module_index];
                if let Some((_, definition)) = module.items.iter().filter_map(|item| item_definition(item, &module.display)).find(|(name, _)| name == member) {
                    return Some(definition);
                }
            }
        }
        let owner = receiver_type?;
        for module in &self.bundle.modules {
            for item in &module.items {
                match item {
                    Item::Struct(value) if value.name == owner => {
                        if let Some(field) = value.fields.iter().find(|field| field.name == member) {
                            return Some(anchor(&module.display, "field", field.name_span));
                        }
                        if let Some(method) = value.methods.iter().find(|method| method.name == member) {
                            return Some(anchor(&module.display, "function", method.name_span));
                        }
                    }
                    Item::Impl(value) if value.type_name == owner => {
                        if let Some(method) = value.methods.iter().find(|method| method.name == member) {
                            return Some(anchor(&module.display, "function", method.name_span));
                        }
                    }
                    _ => {}
                }
            }
        }
        None
    }
    fn item(&mut self, item: &Item) {
        match item {
            Item::Func(function) => self.function(function),
            Item::Struct(value) => {
                for method in &value.methods { self.function(method); }
                for field in &value.fields { if let Some(expr) = &field.computed { self.expr(expr); } }
            }
            Item::Enum(value) => for method in &value.methods { self.function(method); },
            Item::Trait(value) => for method in &value.methods { if let Some(body) = &method.default_body { self.block(body); } },
            Item::Impl(value) => for method in &value.methods { self.function(method); },
            Item::Const(value) => self.expr(&value.value),
            Item::Test(value) => self.block(&value.body),
            Item::Bench(value) => self.block(&value.body),
            Item::CodeModule(value) => if let Some(body) = &value.body { for item in body { self.item(item); } },
            Item::GenericModule(value) => for item in &value.body { self.item(item); },
            _ => {}
        }
    }
    fn function(&mut self, function: &AST::Func) {
        self.scopes.push(HashMap::new());
        for param in &function.params { self.bind(&param.name, "param", param.name_span); }
        self.block_same_scope(&function.body);
        self.scopes.pop();
    }
    fn block(&mut self, stmts: &[Stmt]) {
        self.scopes.push(HashMap::new()); self.block_same_scope(stmts); self.scopes.pop();
    }
    fn block_same_scope(&mut self, stmts: &[Stmt]) { for stmt in stmts { self.stmt(stmt); } }
    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Val(binding) => { self.expr(&binding.init); self.bind(&binding.name, "local", binding.name_span); }
            Stmt::Expr(expr) | Stmt::Yield(expr, _) => self.expr(expr),
            Stmt::Assign { target, value, .. } => { self.lvalue(target); self.expr(value); }
            Stmt::Return(Some(expr), _) => self.expr(expr),
            Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) | Stmt::BreakLabel(..) | Stmt::ContinueLabel(..) => {}
            Stmt::If(value) => self.if_stmt(value),
            Stmt::While { cond, body, .. } => { self.expr(cond); self.block(body); }
            Stmt::For { var, var_span, var2, kind, body, .. } => {
                match kind { ForKind::Range { start, end, step } => { self.expr(start); self.expr(end); if let Some(step) = step { self.expr(step); } }, ForKind::In { collection } => self.expr(collection) }
                self.scopes.push(HashMap::new()); self.bind(var, "local", *var_span); if let Some((name, span)) = var2 { self.bind(name, "local", *span); } self.block_same_scope(body); self.scopes.pop();
            }
            Stmt::Switch { subject, arms, else_body, .. } | Stmt::ComptimeSwitch { subject, arms, else_body, .. } => {
                self.expr(subject); for arm in arms { self.expr(&arm.cond); self.block(&arm.body); } if let Some(body) = else_body { self.block(body); }
            }
            Stmt::CountedLoop { init, cond, body, .. } => { self.expr(&init.init); self.scopes.push(HashMap::new()); self.bind(&init.name, "local", init.name_span); self.expr(cond); self.block_same_scope(body); self.scopes.pop(); }
            Stmt::Loop { body, .. } | Stmt::Unsafe { body, .. } | Stmt::Impure { body, .. } | Stmt::Reactive { body, .. } | Stmt::Shield { body, .. } | Stmt::Off { body, .. } | Stmt::DebugOnly { body, .. } | Stmt::Region { body, .. } | Stmt::TaskGroup { body, .. } | Stmt::Layout { body, .. } | Stmt::Caps { body, .. } | Stmt::Grant { body, .. } | Stmt::Transact { body, .. } | Stmt::AssumeDet { body, .. } | Stmt::ComptimeBlock { body, .. } | Stmt::Live { body, .. } | Stmt::SuppressMustUse { body, .. } | Stmt::ScopeMember { body, .. } => self.block(body),
            Stmt::ComptimeIf { cond, then_body, else_body, .. } => { self.expr(cond); self.block(then_body); if let Some(body) = else_body { self.block(body); } }
            Stmt::ContextBlock { fields, body, .. } => { for (_, expr, _) in fields { self.expr(expr); } self.block(body); }
        }
    }
    fn if_stmt(&mut self, value: &AST::IfStmt) {
        self.expr(&value.cond); self.block(&value.then_body);
        if let Some(branch) = &value.else_branch { match branch { ElseBranch::Else(body) => self.block(body), ElseBranch::ElseIf(next) => self.if_stmt(next) } }
    }
    fn lvalue(&mut self, value: &LValue) { match value { LValue::Local { name, name_span } => self.record(name, *name_span), LValue::Index { base, index, .. } => { self.expr(base); self.expr(index); }, LValue::Field { base, .. } => self.expr(base) } }
    fn expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Ident(name, span) => self.record(name, *span),
            Expr::Call(call) => { self.record(&call.name, call.name_span); for arg in &call.args { self.expr(&arg.expr); } }
            Expr::MethodCall { receiver, method, method_span, args, recv_type, .. } => { self.expr(receiver); if let Some(target) = self.member_target(recv_type.as_deref(), receiver, method) { self.out.insert((self.module_path.clone(), method_span.start, method_span.end), target); } for arg in args { self.expr(&arg.expr); } }
            Expr::Field(base, member, span) => { self.expr(base); if let Some(target) = self.member_target(None, base, member) { self.out.insert((self.module_path.clone(), span.start, span.end), target); } }
            Expr::OptField { base, member, member_span, .. } => { self.expr(base); if let Some(target) = self.member_target(None, base, member) { self.out.insert((self.module_path.clone(), member_span.start, member_span.end), target); } }
            Expr::PtrFromAddr { addr, .. } => self.expr(addr),
            Expr::Binary(_, lhs, rhs, _) => { self.expr(lhs); self.expr(rhs); }
            Expr::CompareChain { operands, .. } | Expr::ListLit(operands, _) => for value in operands { self.expr(value); },
            Expr::Unary(_, value, _) | Expr::IncDec { operand: value, .. } | Expr::Deref(value, _) | Expr::RawOf(value, _) | Expr::Copy(value, _) | Expr::Spread(value, _) | Expr::Tainted(value, _) | Expr::Present(value, _) | Expr::Ok(value, _) | Expr::Err(value, _) | Expr::Try(value, _, _) | Expr::Paren(value, _) | Expr::PatternTest { subject: value, .. } => self.expr(value),
            Expr::Index { base, index, .. } => { self.expr(base); self.expr(index); }
            Expr::Slice { base, start, end, .. } => { self.expr(base); self.expr(start); self.expr(end); }
            Expr::Str(parts, _) => for part in parts { if let AST::StrPart::Interp(value, _) = part { self.expr(value); } },
            Expr::MapLit(entries, _) => for (key, value) in entries { self.expr(key); self.expr(value); },
            Expr::TupleLit(fields, _, _) => for (_, value) in fields { self.expr(value); },
            Expr::StructLit { fields, .. } => for (_, _, value) in fields { self.expr(value); },
            Expr::EnumLit { args, .. } => for arg in args { match arg { AST::EnumLitArg::Positional(value) | AST::EnumLitArg::Named { expr: value, .. } => self.expr(value) } },
            Expr::OrFallback { value, fallback, .. } => { self.expr(value); match fallback { AST::OrFallback::Value(value) | AST::OrFallback::Return(Some(value), _) => self.expr(value), AST::OrFallback::Panic { args, .. } => for arg in args { self.expr(&arg.expr); }, _ => {} } }
            Expr::Lambda(value) => { self.scopes.push(HashMap::new()); for param in &value.params { self.bind(&param.name, "param", param.name_span); } match &value.body { LambdaBody::Expr(expr) => self.expr(expr), LambdaBody::Block(body) => self.block_same_scope(body) } self.scopes.pop(); }
            Expr::CallValue { callee, args, .. } => { self.expr(callee); for arg in args { self.expr(&arg.expr); } }
            Expr::If { cond, then_body, then_value, else_body, else_value, .. } => { self.expr(cond); self.block(then_body); self.expr(then_value); self.block(else_body); self.expr(else_value); }
            Expr::FanOut { callee, items, .. } => { self.expr(callee); for item in items { self.expr(item); } }
            Expr::Int(..) | Expr::Float(..) | Expr::Bool(..) | Expr::Char(..) | Expr::Absent(_) | Expr::ReduceMarker(..) | Expr::Todo { .. } | Expr::UnitLit { .. } | Expr::ComptimeSplice { .. } | Expr::StrMatchLit(..) => {}
        }
    }
}
