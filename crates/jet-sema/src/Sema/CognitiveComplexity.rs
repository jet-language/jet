//! Deterministic cognitive-complexity scoring for `jet lint --complexity`.
//!
//! Score rule:
//! - branch, loop, or dispatch arm-set: `1 + enclosing-structure-depth`;
//! - a mixed `&&`/`||` sequence: `+1`;
//! - direct recursion: `+1` per function;
//! - lambda bodies increase enclosing-structure depth, but add no score.

use crate::AST::{BinOp, Expr, Func, Item, LambdaBody, Program, Stmt};
use crate::Diagnostics::Span;

#[derive(Debug, Clone)]
pub struct CognitiveComplexityReport {
    pub name: String,
    pub span: Span,
    pub score: u32,
}

pub fn cognitive_complexity_reports(program: &Program) -> Vec<CognitiveComplexityReport> {
    let mut functions = Vec::new();
    collect_items(&program.items, &mut functions);
    let mut reports: Vec<_> = functions
        .into_iter()
        .map(|function| CognitiveComplexityReport {
            name: function.name.clone(),
            span: function.span,
            score: cognitive_complexity(function),
        })
        .collect();
    reports.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.span.start.cmp(&right.span.start))
            .then_with(|| left.name.cmp(&right.name))
    });
    reports
}

fn collect_items<'a>(items: &'a [Item], functions: &mut Vec<&'a Func>) {
    for item in items {
        match item {
            Item::Func(function) => functions.push(function),
            Item::Impl(implementation) => functions.extend(&implementation.methods),
            Item::Struct(definition) => {
                functions.extend(&definition.methods);
                for implementation in &definition.trait_impls {
                    functions.extend(&implementation.methods);
                }
            }
            Item::Enum(definition) => {
                functions.extend(&definition.methods);
                for implementation in &definition.trait_impls {
                    functions.extend(&implementation.methods);
                }
            }
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    collect_items(body, functions);
                }
            }
            Item::GenericModule(module) => collect_items(&module.body, functions),
            _ => {}
        }
    }
}

fn cognitive_complexity(function: &Func) -> u32 {
    let mut body = function.body.clone();
    let mut controls = Vec::new();
    let mut nesting = Vec::new();
    let mut mixed_boolean_sequences = 0;
    let mut recursive = false;

    for statement in &mut body {
        visit_statement_shapes(statement, &mut controls, &mut nesting);
        statement.for_each_expr_mut(|expression| match expression {
            Expr::If { span, .. } => {
                controls.push(*span);
                nesting.push(*span);
            }
            Expr::Lambda(lambda) => {
                nesting.push(lambda.span);
                if let LambdaBody::Block(body) = &lambda.body {
                    for statement in body {
                        visit_statement_shapes(statement, &mut controls, &mut nesting);
                    }
                }
            }
            Expr::Call(call) if call.name == function.name => recursive = true,
            Expr::Binary(BinOp::And | BinOp::Or, ..)
                if is_mixed_boolean_sequence(expression) =>
            {
                mixed_boolean_sequences += 1;
            }
            _ => {}
        });
    }

    let structure_score: u32 = controls
        .iter()
        .map(|inner| {
            1 + nesting
                .iter()
                .filter(|outer| contains(**outer, *inner))
                .count() as u32
        })
        .sum();
    structure_score + mixed_boolean_sequences + if recursive { 1 } else { 0 }
}

fn contains(outer: Span, inner: Span) -> bool {
    outer.start < inner.start && inner.end < outer.end
}

fn is_mixed_boolean_sequence(expression: &Expr) -> bool {
    let mut has_and = false;
    let mut has_or = false;
    collect_boolean_operators(expression, &mut has_and, &mut has_or);
    has_and && has_or
}

fn collect_boolean_operators(expression: &Expr, has_and: &mut bool, has_or: &mut bool) {
    match expression {
        Expr::Binary(BinOp::And, left, right, _) => {
            *has_and = true;
            collect_boolean_operators(left, has_and, has_or);
            collect_boolean_operators(right, has_and, has_or);
        }
        Expr::Binary(BinOp::Or, left, right, _) => {
            *has_or = true;
            collect_boolean_operators(left, has_and, has_or);
            collect_boolean_operators(right, has_and, has_or);
        }
        Expr::Paren(inner, _) => collect_boolean_operators(inner, has_and, has_or),
        _ => {}
    }
}

fn visit_statement_shapes(statement: &Stmt, controls: &mut Vec<Span>, nesting: &mut Vec<Span>) {
    fn visit_body(body: &[Stmt], controls: &mut Vec<Span>, nesting: &mut Vec<Span>) {
        for statement in body {
            visit_statement_shapes(statement, controls, nesting);
        }
    }

    match statement {
        Stmt::While { body, span, .. }
        | Stmt::For { body, span, .. }
        | Stmt::Loop { body, span, .. } => {
            controls.push(*span);
            nesting.push(*span);
            visit_body(body, controls, nesting);
        }
        Stmt::CountedLoop {
            body, step, span, ..
        } => {
            controls.push(*span);
            nesting.push(*span);
            if let Some(step) = step {
                visit_statement_shapes(step, controls, nesting);
            }
            visit_body(body, controls, nesting);
        }
        Stmt::Switch {
            arms,
            else_body,
            span,
            ..
        }
        | Stmt::ComptimeSwitch {
            arms,
            else_body,
            span,
            ..
        } => {
            controls.push(*span);
            nesting.push(*span);
            for arm in arms {
                visit_body(&arm.body, controls, nesting);
            }
            if let Some(body) = else_body {
                visit_body(body, controls, nesting);
            }
        }
        Stmt::ComptimeIf {
            then_body,
            else_body,
            span,
            ..
        } => {
            controls.push(*span);
            nesting.push(*span);
            visit_body(then_body, controls, nesting);
            if let Some(body) = else_body {
                visit_body(body, controls, nesting);
            }
        }
        Stmt::Unsafe { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Switched { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::Caps { body, .. }
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::ScopeMember { body, .. }
        | Stmt::ContextBlock { body, .. }
        | Stmt::AssumeDet { body, .. } => visit_body(body, controls, nesting),
        _ => {}
    }
}
