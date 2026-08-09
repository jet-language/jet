//! Dependency-inversion seam: jet-codegen installs the TIR evaluator so
//! comptime/REPL/dev entry points share one engine without a crate cycle.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;

use crate::AST::{ComptimeInput, Expr, Func, ProgramBundle, Stmt, StructDef, Type};
use crate::Diagnostics::Diagnostic;
use crate::Comptime::CtValue;
use crate::Comptime::DevSink;

fn reflected_struct_field<'a>(value: &'a CtValue, field: &str) -> Option<&'a CtValue> {
    let CtValue::Struct { fields, .. } = value else {
        return None;
    };
    fields.iter().find(|(name, _)| name == field).map(|(_, value)| value)
}

fn literal_index(expr: &Expr, globals: &HashMap<String, CtValue>) -> Option<usize> {
    match expr {
        Expr::Int(value, ..) => usize::try_from(*value).ok(),
        Expr::Paren(inner, _) | Expr::Copy(inner, _) => literal_index(inner, globals),
        Expr::Ident(name, _) => match globals.get(name) {
            Some(CtValue::Int(value)) => usize::try_from(*value).ok(),
            _ => None,
        },
        _ => None,
    }
}

fn reflected_value<'a>(
    expr: &Expr,
    globals: &'a HashMap<String, CtValue>,
) -> Option<&'a CtValue> {
    match expr {
        Expr::Ident(name, _) | Expr::ComptimeName { name, .. } => globals.get(name),
        Expr::Paren(inner, _) | Expr::Copy(inner, _) | Expr::Place(inner, _, _) => {
            reflected_value(inner, globals)
        }
        Expr::MethodCall {
            receiver, method, ..
        } if method == "reflect" => {
            let value = reflected_value(receiver, globals)?;
            matches!(
                value,
                CtValue::Struct { type_name, .. }
                    if type_name == crate::Syntax::TYPE_TYPE_INFO
            )
            .then_some(value)
        }
        Expr::Field(base, field, _) => {
            let value = reflected_value(base, globals)?;
            let field = if crate::Syntax::compiler_fact_member(field).is_some() {
                let CtValue::Struct { type_name, .. } = value else {
                    return None;
                };
                if type_name != crate::Syntax::TYPE_TYPE_INFO {
                    return None;
                }
                crate::Syntax::compiler_fact_member(field)?
            } else {
                field.as_str()
            };
            reflected_struct_field(value, field)
        }
        Expr::OptField { base, member, .. } => {
            let value = reflected_value(base, globals)?;
            reflected_struct_field(value, member)
        }
        Expr::Index { base, index, .. } => {
            let value = reflected_value(base, globals)?;
            if let CtValue::Struct { type_name, .. } = value {
                if type_name == crate::Syntax::TYPE_LAYOUT_INFO {
                    let selector = match index.as_ref() {
                        Expr::Ident(name, _) => crate::Syntax::layout_selector_name(name),
                        _ => None,
                    }?;
                    let CtValue::List(fields) = reflected_struct_field(value, "fields")? else {
                        return None;
                    };
                    return fields.iter().find(|field| {
                        matches!(
                            reflected_struct_field(field, "name"),
                            Some(CtValue::Str(name)) if name == selector
                        )
                    });
                }
            }
            let CtValue::List(values) = value else {
                return None;
            };
            values.get(literal_index(index, globals)?)
        }
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReflectedShape {
    TypeInfo,
    LayoutInfo,
    LayoutFields,
    LayoutField,
}

fn reflected_shape_from_value(value: &CtValue) -> Option<ReflectedShape> {
    let CtValue::Struct { type_name, .. } = value else {
        return None;
    };
    match type_name.as_str() {
        crate::Syntax::TYPE_TYPE_INFO => Some(ReflectedShape::TypeInfo),
        crate::Syntax::TYPE_LAYOUT_INFO => Some(ReflectedShape::LayoutInfo),
        crate::Syntax::TYPE_LAYOUT_FIELD => Some(ReflectedShape::LayoutField),
        _ => None,
    }
}

fn reflected_shape_field(base: ReflectedShape, field: &str) -> Option<ReflectedShape> {
    let field = if base == ReflectedShape::TypeInfo {
        crate::Syntax::compiler_fact_member(field).unwrap_or(field)
    } else {
        field
    };
    match (base, field) {
        (ReflectedShape::TypeInfo, "layout") => Some(ReflectedShape::LayoutInfo),
        (ReflectedShape::LayoutInfo, "fields") => Some(ReflectedShape::LayoutFields),
        _ => None,
    }
}

fn reflected_shape(
    expr: &Expr,
    globals: &HashMap<String, CtValue>,
    locals: &HashMap<String, ReflectedShape>,
) -> Option<ReflectedShape> {
    match expr {
        Expr::Ident(name, _) | Expr::ComptimeName { name, .. } => locals
            .get(name)
            .copied()
            .or_else(|| globals.get(name).and_then(reflected_shape_from_value)),
        Expr::Paren(inner, _) | Expr::Copy(inner, _) | Expr::Place(inner, _, _) => {
            reflected_shape(inner, globals, locals)
        }
        Expr::MethodCall {
            receiver, method, ..
        } if method == "reflect" => {
            (reflected_shape(receiver, globals, locals) == Some(ReflectedShape::TypeInfo))
                .then_some(ReflectedShape::TypeInfo)
        }
        Expr::Field(base, field, _) => {
            reflected_shape(base, globals, locals)
                .and_then(|shape| reflected_shape_field(shape, field))
        }
        Expr::OptField { base, member, .. } => {
            reflected_shape(base, globals, locals)
                .and_then(|shape| reflected_shape_field(shape, member))
        }
        Expr::Index { base, .. } => match reflected_shape(base, globals, locals) {
            Some(ReflectedShape::LayoutInfo | ReflectedShape::LayoutFields) => {
                Some(ReflectedShape::LayoutField)
            }
            _ => None,
        },
        _ => None,
    }
}

fn remember_reflected_binding(
    name: &str,
    init: &Expr,
    globals: &HashMap<String, CtValue>,
    locals: &mut HashMap<String, ReflectedShape>,
) {
    if name.is_empty() {
        return;
    }
    if let Some(shape) = reflected_shape(init, globals, locals) {
        locals.insert(name.to_string(), shape);
    } else {
        locals.remove(name);
    }
}

fn collect_reflected_shapes(
    stmts: &[Stmt],
    globals: &HashMap<String, CtValue>,
    locals: &mut HashMap<String, ReflectedShape>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Val(binding) => {
                if binding.pattern.is_none() {
                    remember_reflected_binding(&binding.name, &binding.init, globals, locals);
                }
            }
            Stmt::Assign {
                target: crate::AST::LValue::Local { name, .. },
                value,
                ..
            } => remember_reflected_binding(name, value, globals, locals),
            Stmt::For {
                var, kind, body, ..
            } => {
                let item_shape = match kind {
                    crate::AST::ForKind::In { collection, .. }
                        if reflected_shape(collection, globals, locals)
                            == Some(ReflectedShape::LayoutFields) =>
                    {
                        Some(ReflectedShape::LayoutField)
                    }
                    _ => None,
                };
                if let Some(shape) = item_shape {
                    locals.insert(var.clone(), shape);
                } else {
                    locals.remove(var);
                }
                collect_reflected_shapes(body, globals, locals);
            }
            Stmt::CountedLoop {
                init, step, body, ..
            } => {
                if init.pattern.is_none() {
                    remember_reflected_binding(&init.name, &init.init, globals, locals);
                }
                collect_reflected_shapes(body, globals, locals);
                if let Some(step) = step {
                    collect_reflected_shapes(std::slice::from_ref(&**step), globals, locals);
                }
            }
            Stmt::While { body, .. }
            | Stmt::Loop { body, .. }
            | Stmt::Unsafe { body, .. }
            | Stmt::Impure { body, .. }
            | Stmt::Reactive { body, .. }
            | Stmt::Shield { body, .. }
            | Stmt::Switched { body, .. }
            | Stmt::Region { body, .. }
            | Stmt::Policy { body, .. }
            | Stmt::TaskGroup { body, .. }
            | Stmt::Layout { body, .. }
            | Stmt::Caps { body, .. }
            | Stmt::Grant { body, .. }
            | Stmt::ComptimeBlock { body, .. }
            | Stmt::ContextBlock { body, .. }
            | Stmt::Live { body, .. }
            | Stmt::AssumeDet { body, .. }
            | Stmt::Transact { body, .. }
            | Stmt::ScopeMember { body, .. } => {
                collect_reflected_shapes(body, globals, locals);
            }
            Stmt::Switch {
                arms, else_body, ..
            }
            | Stmt::ComptimeSwitch {
                arms, else_body, ..
            } => {
                for arm in arms {
                    collect_reflected_shapes(&arm.body, globals, locals);
                }
                if let Some(body) = else_body {
                    collect_reflected_shapes(body, globals, locals);
                }
            }
            Stmt::ComptimeIf {
                then_body, else_body, ..
            } => {
                collect_reflected_shapes(then_body, globals, locals);
                if let Some(body) = else_body {
                    collect_reflected_shapes(body, globals, locals);
                }
            }
            _ => {}
        }
    }
}

fn layout_byte_fact_at(
    node: &Expr,
    globals: &HashMap<String, CtValue>,
    locals: &HashMap<String, ReflectedShape>,
) -> Option<Diagnostic> {
    let (base, member, span) = match node {
        Expr::Field(base, member, span) => (base.as_ref(), member.as_str(), *span),
        Expr::OptField {
            base,
            member,
            member_span,
            ..
        } => (base.as_ref(), member.as_str(), *member_span),
        _ => return None,
    };
    let type_name = match reflected_value(base, globals) {
        Some(CtValue::Struct { type_name, .. })
            if matches!(
                type_name.as_str(),
                crate::Syntax::TYPE_LAYOUT_INFO | crate::Syntax::TYPE_LAYOUT_FIELD
            ) => type_name.as_str(),
        _ => match reflected_shape(base, globals, locals) {
            Some(ReflectedShape::LayoutInfo) => crate::Syntax::TYPE_LAYOUT_INFO,
            Some(ReflectedShape::LayoutField) => crate::Syntax::TYPE_LAYOUT_FIELD,
            _ => return None,
        },
    };
    if !crate::Syntax::is_layout_byte_fact(type_name, member) {
        return None;
    }
    Some(Diagnostic::error(
        "E0956",
        format!(
            "`{type_name}.{member}` is unavailable until a canonical target layout engine ships (D-LAYOUT-FACTS1=B)"
        ),
        "D-LAYOUT-FACTS1=B keeps byte facts absent until a canonical target layout engine exists".to_string(),
        "read `kind`, `target`, `guarantee`, and `source`, or a field's `name` and `ty`; ship the canonical target layout engine before reading byte facts".to_string(),
        Some(span),
    ))
}

fn layout_byte_fact_diagnostic(
    expr: &Expr,
    globals: &HashMap<String, CtValue>,
) -> Option<Diagnostic> {
    let locals = HashMap::new();
    let mut diagnostic = None;
    super::Purity::walk_expr_nodes_for_validation(expr, &mut |node| {
        if diagnostic.is_none() {
            diagnostic = layout_byte_fact_at(node, globals, &locals);
        }
    });
    diagnostic
}

fn layout_byte_fact_diagnostic_in_stmts(
    stmts: &[Stmt],
    globals: &HashMap<String, CtValue>,
) -> Option<Diagnostic> {
    let mut locals = HashMap::new();
    collect_reflected_shapes(stmts, globals, &mut locals);
    let mut diagnostic = None;
    super::Purity::walk_stmt_expr_nodes_for_validation(stmts, &mut |node| {
        if diagnostic.is_none() {
            diagnostic = layout_byte_fact_at(node, globals, &locals);
        }
    });
    diagnostic
}

pub struct ExprEvalRequest<'a> {
    pub expr: &'a Expr,
    pub funcs: &'a HashMap<String, &'a Func>,
    /// Instance/associated methods are kept in their semantic owner/name table
    /// by comptime. The TIR fragment host needs the same table to lower
    /// computed-field getters and to call user methods.
    pub methods: &'a HashMap<(String, String), &'a Func>,
    pub extern_names: &'a HashSet<String>,
    pub base_dir: &'a Path,
    pub globals: &'a HashMap<String, CtValue>,
    pub core_imports: &'a HashMap<String, String>,
    pub allow_impure: bool,
    pub initial_impure_depth: usize,
    pub structs: &'a HashMap<String, &'a StructDef>,
    pub computed_fields: &'a HashMap<(String, String), &'a Expr>,
    pub distinct_ranges: &'a HashMap<String, Option<(i64, i64)>>,
    pub distinct_bases: &'a HashMap<String, Type>,
    pub fuel: u64,
    pub sink: Option<&'a mut DevSink>,
    pub repl_mode: bool,
    /// D-METADERIVE1: `emit(…)` fragments (usually unused for single exprs).
    pub emitted_fragments: Option<&'a mut Vec<String>>,
    /// D-CTEFFECT1 Tier-1 inputs recorded by the canonical host surface.
    pub embed_inputs: Option<&'a mut Vec<ComptimeInput>>,
    /// Bindings as the fragment left them. An expression can mutate a binding
    /// it reads — `reader.read_u32_le()` advances the reader — and a statement
    /// driver has to see the advance, or the next expression starts over.
    /// `None` for pure const evaluation, which has no caller scope to update.
    pub mutated: Option<&'a mut HashMap<String, CtValue>>,
}

pub struct BlockEvalRequest<'a> {
    pub stmts: &'a [Stmt],
    pub funcs: &'a HashMap<String, &'a Func>,
    pub methods: &'a HashMap<(String, String), &'a Func>,
    pub extern_names: &'a HashSet<String>,
    pub base_dir: &'a Path,
    pub globals: &'a HashMap<String, CtValue>,
    pub core_imports: &'a HashMap<String, String>,
    pub structs: &'a HashMap<String, &'a StructDef>,
    pub distinct_ranges: &'a HashMap<String, Option<(i64, i64)>>,
    pub distinct_bases: &'a HashMap<String, Type>,
    pub fuel: u64,
    pub sink: Option<&'a mut DevSink>,
    pub repl_mode: bool,
    pub allow_impure: bool,
    pub impure_depth: usize,
    pub computed_fields: &'a HashMap<(String, String), &'a Expr>,
    /// D-METADERIVE1: `emit(…)` fragments from a derive body.
    pub emitted_fragments: Option<&'a mut Vec<String>>,
    /// D-CTEFFECT1 Tier-1 inputs recorded by the canonical host surface.
    pub embed_inputs: Option<&'a mut Vec<ComptimeInput>>,
}

/// Outcome of evaluating a statement list through the canonical TIR evaluator.
pub enum StmtOutcome {
    /// Finished normally; scope holds bindings.
    Done(HashMap<String, CtValue>),
    /// `return` escaped the fragment.
    Returned {
        value: CtValue,
        scope: HashMap<String, CtValue>,
    },
}

pub struct Hooks {
    pub run_bundle: fn(&ProgramBundle, &mut DevSink, bool) -> Result<CtValue, Diagnostic>,
    pub eval_expr: fn(&mut ExprEvalRequest<'_>) -> Result<CtValue, Diagnostic>,
    pub eval_block: fn(&mut BlockEvalRequest<'_>) -> Result<StmtOutcome, Diagnostic>,
}

static HOOKS: OnceLock<Hooks> = OnceLock::new();

pub fn install(hooks: Hooks) {
    let _ = HOOKS.set(hooks);
}

fn hooks() -> &'static Hooks {
    HOOKS
        .get()
        .expect("TIR eval bridge not installed — call Codegen::TIR::eval::install_comptime_bridge()")
}

pub fn run_bundle(
    bundle: &ProgramBundle,
    sink: &mut DevSink,
    allow_impure: bool,
) -> Result<CtValue, Diagnostic> {
    (hooks().run_bundle)(bundle, sink, allow_impure)
}

pub fn eval_expr(req: &mut ExprEvalRequest<'_>) -> Result<CtValue, Diagnostic> {
    if let Some(diagnostic) = layout_byte_fact_diagnostic(req.expr, req.globals) {
        return Err(diagnostic);
    }
    (hooks().eval_expr)(req)
}

pub fn eval_block(req: &mut BlockEvalRequest<'_>) -> Result<StmtOutcome, Diagnostic> {
    if let Some(diagnostic) = layout_byte_fact_diagnostic_in_stmts(req.stmts, req.globals) {
        return Err(diagnostic);
    }
    (hooks().eval_block)(req)
}
