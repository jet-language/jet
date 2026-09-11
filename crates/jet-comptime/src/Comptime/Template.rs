use crate::AST::{CtReport, CtValue};
use crate::Diagnostics::{Diagnostic, Span};
use std::collections::HashMap;

/// One checked item-template body. The expression parameter is the owning
/// tier's typed expression node (`TExpr` for TIR); this module never retains
/// parser nodes or a pre-rendered generated module.
#[derive(Clone)]
pub struct TemplateBody<E> {
    pub items: Vec<Box<TemplateItem<E>>>,
    pub span: Span,
}

/// A template item keeps static source bytes and typed splice/control facts.
#[derive(Clone)]
pub enum TemplateItem<E> {
    Item(TemplateSource<E>),
    Statement(TemplateStatement<E>),
    Loop {
        var: String,
        source: Box<E>,
        body: Vec<Box<TemplateItem<E>>>,
        span: Span,
    },
    If {
        condition: Box<E>,
        then_body: Vec<Box<TemplateItem<E>>>,
        else_body: Vec<Box<TemplateItem<E>>>,
        /// Sema's selected arm, when the condition was already folded.
        /// `None` means the owning evaluator must evaluate the typed condition.
        selected: Option<bool>,
        span: Span,
    },
    /// A checked shape that cannot be represented because its source span is malformed.
    /// It remains explicit so no renderer invents an empty source fallback.
    Invalid { construct: String, span: Span },
}

/// A compile-time statement which changes template scope but emits no source.
#[derive(Clone)]
pub enum TemplateStatement<E> {
    Binding {
        name: String,
        value: Box<E>,
        span: Span,
    },
    Expr {
        value: Box<E>,
        span: Span,
    },
}

/// Static source for one item, with byte ranges for every dynamic splice.
#[derive(Clone)]
pub struct TemplateSource<E> {
    pub source: String,
    pub holes: Vec<TemplateHole<E>>,
    pub span: Span,
}

/// One typed dynamic splice into static template source.
#[derive(Clone)]
pub struct TemplateHole<E> {
    /// Byte range relative to [`TemplateSource::source`].
    pub start: usize,
    pub end: usize,
    pub expr: Box<E>,
    pub kind: TemplateHoleKind,
    pub span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TemplateHoleKind {
    /// The value is substituted as a Jet expression literal.
    Value,
    /// The value is substituted as an item/type/member name.
    Name,
}

/// Render one checked template body. The owning evaluator supplies the only
/// operation over its typed expression node; loop and binding scope changes
/// stay in this shared model so every tier applies one control law.
pub fn format_tir_template_body<E, F>(
    body: &TemplateBody<E>,
    scope: &mut HashMap<String, CtValue>,
    mut eval: F,
) -> Result<String, Diagnostic>
where
    F: FnMut(&E, &mut HashMap<String, CtValue>) -> Result<CtValue, Diagnostic>,
{
    let mut template_scope = scope.clone();
    render_items(&body.items, &mut template_scope, &mut eval)
}

fn render_items<E, F>(
    items: &[Box<TemplateItem<E>>],
    scope: &mut HashMap<String, CtValue>,
    eval: &mut F,
) -> Result<String, Diagnostic>
where
    F: FnMut(&E, &mut HashMap<String, CtValue>) -> Result<CtValue, Diagnostic>,
{
    let mut rendered = Vec::new();
    for item in items {
        match item.as_ref() {
            TemplateItem::Item(source) => rendered.push(render_source(source, scope, eval)?),
            TemplateItem::Invalid { construct, span } => {
                return Err(template_error(
                    construct,
                    "checked template source must remain attached to its source span",
                    "report the malformed generated template to the compiler",
                    *span,
                ));
            }
            TemplateItem::Statement(statement) => match statement {
                TemplateStatement::Binding { name, value, .. } => {
                    let value = eval(value, scope)?;
                    scope.insert(name.clone(), value.clone());
                    scope.insert(format!("@{name}"), value);
                }
                TemplateStatement::Expr { value, .. } => {
                    let _ = eval(value, scope)?;
                }
            },
            TemplateItem::Loop {
                var,
                source,
                body,
                span,
            } => {
                let value = eval(source, scope)?;
                let CtValue::List(values) = value else {
                    return Err(template_error(
                        "an `@loop` source is not a compile-time list",
                        "`@loop` expands one item template for each value in its source list",
                        "use a reflected/comptime list or a closed type list",
                        *span,
                    ));
                };
                let marked_var = format!("@{var}");
                let previous = scope.get(var).cloned();
                let previous_marked = scope.get(&marked_var).cloned();
                for value in values {
                    scope.insert(var.clone(), value.clone());
                    scope.insert(marked_var.clone(), value);
                    let expansion = render_items(body, scope, eval)?;
                    if !expansion.is_empty() {
                        rendered.push(expansion);
                    }
                }
                restore_scope(scope, var, previous);
                restore_scope(scope, &marked_var, previous_marked);
            }
            TemplateItem::If {
                condition,
                then_body,
                else_body,
                selected,
                span,
            } => {
                let selected = match selected {
                    Some(selected) => *selected,
                    None => {
                        let value = eval(condition, scope)?;
                        let CtValue::Bool(selected) = value else {
                            return Err(template_error(
                                "a template condition is not Bool",
                                "compile-time template control requires a checked boolean expression",
                                "use a Bool-valued condition",
                                *span,
                            ));
                        };
                        selected
                    }
                };
                let selected = if selected { then_body } else { else_body };
                let expansion = render_items(selected, scope, eval)?;
                if !expansion.is_empty() {
                    rendered.push(expansion);
                }
            }
        }
    }
    Ok(rendered.join("\n"))
}

fn restore_scope(scope: &mut HashMap<String, CtValue>, name: &str, previous: Option<CtValue>) {
    if let Some(previous) = previous {
        scope.insert(name.to_string(), previous);
    } else {
        scope.remove(name);
    }
}

fn render_source<E, F>(
    source: &TemplateSource<E>,
    scope: &mut HashMap<String, CtValue>,
    eval: &mut F,
) -> Result<String, Diagnostic>
where
    F: FnMut(&E, &mut HashMap<String, CtValue>) -> Result<CtValue, Diagnostic>,
{
    let mut holes = source.holes.iter().collect::<Vec<_>>();
    holes.sort_by_key(|hole| hole.start);
    let mut rendered = String::with_capacity(source.source.len());
    let mut cursor = 0usize;
    for hole in holes {
        if hole.start < cursor
            || hole.end < hole.start
            || hole.end > source.source.len()
            || !source.source.is_char_boundary(hole.start)
            || !source.source.is_char_boundary(hole.end)
        {
            return Err(template_error(
                "template splice range is invalid",
                "checked template spans must remain byte-aligned with their static source",
                "report the malformed generated template to the compiler",
                hole.span,
            ));
        }
        rendered.push_str(&source.source[cursor..hole.start]);
        let value = eval(&hole.expr, scope)?;
        let replacement = match hole.kind {
            TemplateHoleKind::Value => template_value_source(&value, hole.span)?,
            TemplateHoleKind::Name => template_name_source(&value, hole.span)?,
        };
        rendered.push_str(&replacement);
        cursor = hole.end;
    }
    rendered.push_str(&source.source[cursor..]);
    Ok(rendered)
}

fn template_name_source(value: &CtValue, span: Span) -> Result<String, Diagnostic> {
    match value {
        CtValue::Str(name) if !name.is_empty() => Ok(name.clone()),
        CtValue::Struct { fields, .. } => fields
            .iter()
            .find_map(|(field, value)| {
                (field == "name").then_some(value).and_then(|value| match value {
                    CtValue::Str(name) if !name.is_empty() => Some(Ok(name.clone())),
                    _ => None,
                })
            })
            .unwrap_or_else(|| Err(template_error(
                "a generated item name needs text",
                "item and type splices use the reflected name fact",
                "bind the name to a non-empty String value",
                span,
            ))),
        _ => Err(template_error(
            "a generated item name needs text",
            "item and type splices use a String value",
            "bind the name to a String value",
            span,
        )),
    }
}

fn template_value_source(value: &CtValue, span: Span) -> Result<String, Diagnostic> {
    match value {
        CtValue::Bool(value) => Ok(value.to_string()),
        CtValue::Int(value) => Ok(value.to_string()),
        CtValue::Float(value) => Ok(value.render()),
        CtValue::Char(value) => Ok(format!("{value:?}")),
        CtValue::Str(value) => Ok(format!("{value:?}")),
        CtValue::BigInt(value) => Ok(value.to_string_rep()),
        CtValue::Enum {
            type_name,
            variant,
            args,
        } if args.is_empty() => {
            if type_name.is_empty() {
                Ok(variant.clone())
            } else {
                Ok(format!("{type_name}.{variant}"))
            }
        }
        CtValue::Present(value) => Ok(format!("value({})", template_value_source(value, span)?)),
        CtValue::Failed(CtReport::Clean(_)) => Ok("null".to_string()),
        _ => Err(template_error(
            "a template value has no source literal",
            "dynamic template values must be representable Jet literals",
            "convert the value to Bool, Int, Float, Char, String, or a closed enum",
            span,
        )),
    }
}

fn template_error(
    message: &str,
    detail: &str,
    hint: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic::error("E0956", message.to_string(), detail.to_string(), hint.to_string(), Some(span))
}
