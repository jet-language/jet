use super::*;

pub(super) fn no_matching_enum(span: Span) -> Stmt {
    ret(
        err(
            list_literal(
                vec![field_error("", "no matching enum variant", span)],
                span,
            ),
            span,
        ),
        span,
    )
}

pub(super) fn no_matching_union(span: Span) -> Stmt {
    ret(
        err(
            list_literal(
                vec![field_error("", "no matching union member", span)],
                span,
            ),
            span,
        ),
        span,
    )
}

pub(super) fn enum_payload_type(variant: &crate::AST::Variant) -> Type {
    match &variant.payload {
        VariantPayload::Unit | VariantPayload::Named(_) => data_tree_type(),
        VariantPayload::Single(ty, _) => ty.clone(),
    }
}

pub(super) fn payload_bindings(payload: &VariantPayload, prefix: &str) -> Vec<String> {
    let count = match payload {
        VariantPayload::Unit => 0,
        VariantPayload::Single(..) => 1,
        VariantPayload::Named(fields) => fields.len(),
    };
    (0..count).map(|index| format!("{prefix}{index}")).collect()
}

pub(super) fn pattern_test(
    subject: &str,
    variant: &crate::AST::Variant,
    bindings: Vec<String>,
    span: Span,
) -> Expr {
    Expr::PatternTest {
        subject: Box::new(ident(subject, span)),
        pattern: Pattern::Variant {
            variant: variant.name.clone(),
            bindings: bindings
                .into_iter()
                .map(|name| PatSlot::Bind { name, span })
                .collect(),
            leading_dot: true,
            span,
        },
        span,
    }
}

pub(super) fn pattern_switch(
    subject: Expr,
    variant: &str,
    bindings: Vec<String>,
    body: Vec<Stmt>,
    else_body: Option<Vec<Stmt>>,
    span: Span,
) -> Stmt {
    let pattern_subject = subject.clone();
    Stmt::Switch {
        subject,
        arms: vec![SwitchArm {
            cond: Expr::PatternTest {
                subject: Box::new(pattern_subject),
                pattern: Pattern::Variant {
                    variant: variant.to_string(),
                    bindings: bindings
                        .into_iter()
                        .map(|name| PatSlot::Bind { name, span })
                        .collect(),
                    leading_dot: true,
                    span,
                },
                span,
            },
            body,
            span,
        }],
        else_body,
        span,
    }
}

pub(super) fn result_switch(
    subject: Expr,
    ok_binding: &str,
    ok_body: Vec<Stmt>,
    err_binding: &str,
    err_body: Vec<Stmt>,
    span: Span,
) -> Stmt {
    let ok_subject = subject.clone();
    let err_subject = subject.clone();
    Stmt::Switch {
        subject,
        arms: vec![
            SwitchArm {
                cond: Expr::PatternTest {
                    subject: Box::new(ok_subject),
                    pattern: Pattern::Variant {
                        variant: "Ok".to_string(),
                        bindings: vec![PatSlot::Bind {
                            name: ok_binding.to_string(),
                            span,
                        }],
                        leading_dot: true,
                        span,
                    },
                    span,
                },
                body: ok_body,
                span,
            },
            SwitchArm {
                cond: Expr::PatternTest {
                    subject: Box::new(err_subject),
                    pattern: Pattern::Variant {
                        variant: "Err".to_string(),
                        bindings: vec![PatSlot::Bind {
                            name: err_binding.to_string(),
                            span,
                        }],
                        leading_dot: true,
                        span,
                    },
                    span,
                },
                body: err_body,
                span,
            },
        ],
        else_body: Some(Vec::new()),
        span,
    }
}

pub(super) fn option_switch(
    subject: Expr,
    binding_name: &str,
    body: Vec<Stmt>,
    else_body: Vec<Stmt>,
    span: Span,
) -> Stmt {
    pattern_switch(
        subject,
        "Val",
        vec![binding_name.to_string()],
        body,
        Some(else_body),
        span,
    )
}

pub(super) fn option_encode(subject: Expr, key: &str, field_names: Vec<String>, span: Span) -> Stmt {
    let binding_name = format!(
        "jet_serde_option_value_{}",
        field_names
            .first()
            .expect("one field for option encode")
            .replace('.', "_")
    );
    option_switch(
        subject,
        &binding_name,
        vec![assign_index(
            "out",
            string_expr(key, span),
            method(ident(&binding_name, span), "encode", Vec::new(), span),
            span,
        )],
        Vec::new(),
        span,
    )
}

pub(super) fn for_each_map(var: &str, var2: &str, collection: Expr, body: Vec<Stmt>, span: Span) -> Stmt {
    Stmt::For {
        var: var.to_string(),
        var_span: span,
        var2: Some((var2.to_string(), span)),
        kind: ForKind::In {
            collection,
            step: None,
        },
        body,
        span,
        arrow_body: false,
        label: None,
        auto_vectorization: None,
    }
}

pub(super) fn for_each(var: &str, collection: Expr, body: Vec<Stmt>, span: Span) -> Stmt {
    Stmt::For {
        var: var.to_string(),
        var_span: span,
        var2: None,
        kind: ForKind::In {
            collection,
            step: None,
        },
        body,
        span,
        arrow_body: false,
        label: None,
        auto_vectorization: None,
    }
}

pub(super) fn binding(name: &str, ty: Option<Type>, init: Expr, mutable: bool, span: Span) -> Stmt {
    Stmt::Val(crate::AST::Binding {
        mutable,
        markers: Vec::new(),
        reactive_upgrade: false,
        meta: None,
        name: name.to_string(),
        name_span: span,
        sigil_span: None,
        pattern: None,
        ty,
        ty_span: Some(span),
        init,
        is_comptime: false,
        ct: None,
        uninit: false,
        arena_view: false,
        string_view: false,
        gc_promotion: None,
        gc_transferred: false,
    })
}

pub(super) fn assign_local(name: &str, value: Expr, span: Span) -> Stmt {
    Stmt::Assign {
        target: LValue::Local {
            name: name.to_string(),
            name_span: span,
        },
        op: None,
        op_span: span,
        value,
    }
}

pub(super) fn assign_index(base: &str, index: Expr, value: Expr, span: Span) -> Stmt {
    Stmt::Assign {
        target: LValue::Index {
            base: Box::new(ident(base, span)),
            index: Box::new(index),
            span,
            kind: IndexKind::Unknown,
        },
        op: None,
        op_span: span,
        value,
    }
}

pub(super) fn expr_stmt(expr: Expr) -> Stmt {
    Stmt::Expr(expr)
}
pub(super) fn ret(expr: Expr, span: Span) -> Stmt {
    Stmt::Return(Some(expr), span)
}
pub(super) fn ok(expr: Expr, span: Span) -> Expr {
    Expr::Ok(Box::new(expr), span)
}
pub(super) fn err(expr: Expr, span: Span) -> Expr {
    Expr::Err(Box::new(expr), span)
}
pub(super) fn copy(expr: Expr, span: Span) -> Expr {
    Expr::Copy(Box::new(expr), span)
}
pub(super) fn unary_not(expr: Expr, span: Span) -> Expr {
    Expr::Unary(crate::AST::UnOp::Not, Box::new(expr), span)
}

pub(super) fn ident(name: &str, span: Span) -> Expr {
    Expr::Ident(name.to_string(), span)
}

pub(super) fn binary(op: BinOp, left: Expr, right: Expr, span: Span) -> Expr {
    Expr::Binary(op, Box::new(left), Box::new(right), span)
}

pub(super) fn if_stmt(cond: Expr, body: Vec<Stmt>, span: Span) -> Stmt {
    Stmt::Switch {
        subject: Expr::Bool(true, span),
        arms: vec![SwitchArm { cond, body, span }],
        else_body: Some(Vec::new()),
        span,
    }
}

pub(super) fn string_expr(value: &str, span: Span) -> Expr {
    Expr::Str(vec![crate::AST::StrPart::Lit(value.to_string())], span)
}

pub(super) fn data_tree_type() -> Type {
    Type::Named("DataTree".to_string())
}
pub(super) fn field_error_type() -> Type {
    Type::Named("FieldError".to_string())
}
pub(super) fn field_error_list_type(_span: Span) -> Type {
    Type::List(Box::new(field_error_type()))
}
pub(super) fn result_type(ok: Type, _span: Span) -> Type {
    Type::Result {
        ok: Box::new(ok),
        err: Box::new(field_error_list_type(_span)),
    }
}

pub(super) fn map_type(value: Type, _span: Span) -> Type {
    Type::Map {
        key: Box::new(Type::String),
        key_span: None,
        value: Box::new(value),
    }
}

pub(super) fn target_type(name: &str, params: &[TypeParam]) -> Type {
    if params.is_empty() {
        Type::Named(name.to_string())
    } else {
        Type::Apply {
            name: name.to_string(),
            args: params
                .iter()
                .map(|param| Type::Named(param.name.clone()))
                .collect(),
        }
    }
}

pub(super) fn target_type_name(name: &str) -> String {
    name.to_string()
}

pub(super) fn type_args_from_params(params: &[TypeParam], _span: Span) -> Vec<Type> {
    params
        .iter()
        .map(|param| Type::Named(param.name.clone()))
        .collect()
}

pub(super) fn list_literal(values: Vec<Expr>, span: Span) -> Expr {
    Expr::ListLit(values, span)
}
pub(super) fn map_literal(values: Vec<(Expr, Expr)>, span: Span) -> Expr {
    Expr::MapLit(values, span)
}

pub(super) fn data_tree_object(entries: Vec<(Expr, Expr)>, span: Span) -> Expr {
    data_tree_variant("Object", vec![map_literal(entries, span)], span)
}

pub(super) fn data_tree_object_expr(map: Expr, span: Span) -> Expr {
    data_tree_variant("Object", vec![map], span)
}

pub(super) fn data_tree_null(span: Span) -> Expr {
    data_tree_variant("Null", Vec::new(), span)
}
pub(super) fn data_tree_text(value: &str, span: Span) -> Expr {
    data_tree_variant("Text", vec![string_expr(value, span)], span)
}

pub(super) fn data_tree_variant(variant: &str, args: Vec<Expr>, span: Span) -> Expr {
    if args.is_empty() {
        Expr::Field(Box::new(ident("DataTree", span)), variant.to_string(), span)
    } else {
        Expr::MethodCall {
            receiver: Box::new(ident("DataTree", span)),
            method: variant.to_string(),
            method_span: span,
            owner_type_args: Vec::new(),
            type_args: Vec::new(),
            args: args.into_iter().map(|expr| call_arg(expr, span)).collect(),
            recv_type: None,
            resolved_ret: None,
            operator_rhs: None,
            checked_widen: false,
        }
    }
}

pub(super) fn struct_literal(
    name: String,
    type_args: Vec<Type>,
    fields: Vec<(String, Expr)>,
    span: Span,
) -> Expr {
    Expr::StructLit {
        type_name: name,
        type_args,
        import_ns: None,
        as_trait: None,
        fields: fields
            .into_iter()
            .map(|(name, expr)| (name, span, expr))
            .collect(),
        inferred: false,
        span,
    }
}

pub(super) fn field_error(path: &str, reason: &str, span: Span) -> Expr {
    field_error_expr_value(string_expr(path, span), string_expr(reason, span), span)
}

pub(super) fn field_error_expr_value(path: Expr, reason: Expr, span: Span) -> Expr {
    struct_literal(
        "FieldError".to_string(),
        Vec::new(),
        vec![("path".to_string(), path), ("reason".to_string(), reason)],
        span,
    )
}

pub(super) fn interpolated_string(prefix: &str, value: Expr, suffix: &str, span: Span) -> Expr {
    Expr::Str(
        vec![
            crate::AST::StrPart::Lit(prefix.to_string()),
            crate::AST::StrPart::Interp(Box::new(value), crate::AST::StrFormat::default()),
            crate::AST::StrPart::Lit(suffix.to_string()),
        ],
        span,
    )
}

pub(super) fn method(receiver: Expr, name: &str, args: Vec<Expr>, span: Span) -> Expr {
    method_with_owner_args(receiver, name, Vec::new(), args, span)
}

pub(super) fn method_with_owner_args(
    receiver: Expr,
    name: &str,
    owner_type_args: Vec<Type>,
    args: Vec<Expr>,
    span: Span,
) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(receiver),
        method: name.to_string(),
        method_span: span,
        owner_type_args,
        type_args: Vec::new(),
        args: args.into_iter().map(|expr| call_arg(expr, span)).collect(),
        recv_type: (name == "encode").then(|| "__SerdeEncode__".to_string()),
        resolved_ret: None,
        operator_rhs: None,
        checked_widen: false,
    }
}

pub(super) fn method_with_type_args(receiver: Expr, name: &str, type_args: Vec<Type>, span: Span) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(receiver),
        method: name.to_string(),
        method_span: span,
        owner_type_args: Vec::new(),
        type_args,
        args: Vec::new(),
        recv_type: (name == Syntax::METHOD_DATATREE_DECODE).then(|| Syntax::TYPE_DATA.to_string()),
        resolved_ret: None,
        operator_rhs: None,
        checked_widen: false,
    }
}

pub(super) fn field_read(base: &str, field: &str, span: Span) -> Expr {
    Expr::Field(Box::new(ident(base, span)), field.to_string(), span)
}

pub(super) fn field_value(field: &crate::AST::Field, span: Span) -> Expr {
    field
        .computed
        .as_ref()
        .map(|computed| computed.as_ref().clone())
        .unwrap_or_else(|| field_read("self", &field.name, span))
}

pub(super) fn call_arg(expr: Expr, span: Span) -> CallArg {
    CallArg {
        convention: AccessConvention::Read,
        expr,
        span,
        flags: CallArgFlags::default(),
        label: None,
        spread: false,
    }
}

pub(super) fn self_param(span: Span) -> Param {
    named_param("self", Type::Named(String::new()), span)
}

pub(super) fn named_param(name: &str, ty: Type, span: Span) -> Param {
    Param {
        convention: AccessConvention::Read,
        root: false,
        name: name.to_string(),
        name_span: span,
        public_label: None,
        zone: crate::AST::ParamZone::Either,
        ty,
        ty_span: span,
        default: None,
        variadic: false,
        variadic_bound_list: None,
        declared_view_from_names: None,
    }
}

pub(super) fn try_expr(expr: Expr, span: Span) -> Expr {
    Expr::Try(Box::new(expr), span, TryConvert::None, None)
}

pub(super) fn or_fallback(value: Expr, fallback: Expr, span: Span) -> Expr {
    Expr::OrFallback {
        value: Box::new(value),
        fallback: crate::AST::OrFallback::Value(Box::new(fallback)),
        is_option: false,
        span,
    }
}

pub(super) fn field_error_under(path: &str, value: Expr, span: Span) -> Expr {
    method(
        ident("FieldError", span),
        "under",
        vec![string_expr(path, span), value],
        span,
    )
}

pub(super) fn serde_zero_expr(ty: &Type, span: Span) -> Expr {
    match ty {
        Type::Int | Type::IntN { .. } => Expr::Int(0, span, None, None),
        Type::InlineRange { lo, .. } => Expr::Int(*lo, span, None, None),
        Type::Float | Type::Float32 => Expr::Float(0.0, span, matches!(ty, Type::Float32), None),
        Type::Bool => Expr::Bool(false, span),
        Type::String => string_expr("", span),
        Type::Option(_) => Expr::Absent(span),
        Type::List(_) | Type::Map { .. } => list_literal(Vec::new(), span),
        Type::Apply { name, args } => struct_literal(name.clone(), args.clone(), Vec::new(), span),
        Type::Named(name) => struct_literal(name.clone(), Vec::new(), Vec::new(), span),
        _ => struct_literal(ty.name(), Vec::new(), Vec::new(), span),
    }
}

pub(super) fn serde_default_expr(field: &crate::AST::Field) -> Option<Expr> {
    if let Some(expr) = &field.default {
        return Some(expr.as_ref().clone());
    }
    let marker = field
        .serde_markers
        .iter()
        .find(|marker| marker.name == crate::Syntax::MARKER_DEFAULT)?;
    if let Some(expr) = marker.expr_arg(0) {
        if let Some(value) = marker.ct.as_ref() {
            return serde_ct_expr(value, expr.span());
        }
        return Some(expr.clone());
    }
    marker
        .ct
        .as_ref()
        .and_then(|value| serde_ct_expr(value, marker.span))
}

pub(super) fn serde_ct_expr(value: &CtValue, span: Span) -> Option<Expr> {
    Some(match value {
        CtValue::Int(value) => Expr::Int(*value, span, None, None),
        CtValue::Float(value) => Expr::Float(value.as_f64(), span, false, None),
        CtValue::Bool(value) => Expr::Bool(*value, span),
        CtValue::Char(value) => Expr::Char(*value, span),
        CtValue::Str(value) => string_expr(value, span),
        // Exact default `Int` values keep their decimal source spelling so the
        // ordinary literal pipeline owns parsing and lowering on every tier.
        CtValue::BigInt(value) => Expr::Int(0, span, None, Some(value.to_string_rep())),
        CtValue::Bytes(values) => list_literal(
            values
                .iter()
                .map(|value| Expr::Int(i64::from(*value), span, None, None))
                .collect(),
            span,
        ),
        CtValue::List(values) => list_literal(
            values
                .iter()
                .map(|value| serde_ct_expr(value, span))
                .collect::<Option<Vec<_>>>()?,
            span,
        ),
        CtValue::Map(values) => map_literal(
            values
                .iter()
                .map(|(key, value)| {
                    Some((
                        serde_ct_expr(&key.to_value(), span)?,
                        serde_ct_expr(value, span)?,
                    ))
                })
                .collect::<Option<Vec<_>>>()?,
            span,
        ),
        CtValue::Struct { type_name, fields } => struct_literal(
            type_name.clone(),
            Vec::new(),
            fields
                .iter()
                .map(|(name, value)| Some((name.clone(), serde_ct_expr(value, span)?)))
                .collect::<Option<Vec<_>>>()?,
            span,
        ),
        CtValue::Enum {
            type_name,
            variant,
            args,
        } => Expr::EnumLit {
            type_name: type_name.clone(),
            variant: variant.clone(),
            variant_span: None,
            args: args
                .iter()
                .map(|(label, value)| {
                    Some(match label {
                        Some(label) => EnumLitArg::Named {
                            label: label.clone(),
                            expr: serde_ct_expr(value, span)?,
                        },
                        None => EnumLitArg::Positional(serde_ct_expr(value, span)?),
                    })
                })
                .collect::<Option<Vec<_>>>()?,
            leading_dot: false,
            span,
        },
        CtValue::Present(value) => Expr::Present(Box::new(serde_ct_expr(value, span)?), span),
        CtValue::Failed(CtReport::Clean(_)) => Expr::Absent(span),
        CtValue::Failed(CtReport::Told(value)) => {
            Expr::Err(Box::new(serde_ct_expr(value, span)?), span)
        }
        CtValue::Unit | CtValue::Closure(_) => return None,
    })
}

pub(super) fn has_marker(markers: &[crate::AST::Marker], name: &str) -> bool {
    markers.iter().any(|marker| marker.name == name)
}

pub(super) fn serde_enum_variant_key(v: &crate::AST::Variant, style: Option<&str>) -> String {
    v.serde_markers
        .iter()
        .find(|marker| marker.name == crate::Syntax::MARKER_RENAME)
        .and_then(|marker| {
            marker_static_string_for(marker, "json").or_else(|| marker_static_string(marker))
        })
        .unwrap_or_else(|| serde_rename_all_name(style, &v.name))
}

pub(super) fn marker_static_string(marker: &crate::AST::Marker) -> Option<String> {
    if let Some(CtValue::Str(value)) = &marker.ct {
        return Some(value.clone());
    }
    marker.expr_arg(0).and_then(static_string_expr)
}

pub(super) fn marker_static_string_for(marker: &crate::AST::Marker, label: &str) -> Option<String> {
    marker
        .arg_labels
        .iter()
        .enumerate()
        .find(|(_, value)| value.as_ref().is_some_and(|(name, _)| name == label))
        .and_then(|(index, _)| marker.args.get(index))
        .and_then(crate::AST::MarkerCallArg::as_expr)
        .and_then(static_string_expr)
}

pub(super) fn static_string_expr(expression: &Expr) -> Option<String> {
    match expression {
        Expr::Str(parts, _) => parts.first().and_then(|part| match part {
            crate::AST::StrPart::Lit(value) => Some(value.clone()),
            crate::AST::StrPart::Interp(..) => None,
        }),
        _ => None,
    }
}


pub(super) fn serde_rename_all_style(container: &[crate::AST::Marker]) -> Option<&str> {
    container
        .iter()
        .find(|marker| marker.name == crate::Syntax::MARKER_RENAME_ALL)
        .and_then(|marker| marker.expr_arg(0))
        .and_then(|expression| match expression {
            Expr::Ident(name, _) => Some(name.as_str()),
            _ => None,
        })
}

pub(super) fn serde_rename_all_name(style: Option<&str>, name: &str) -> String {
    match style {
        Some("camel") => crate::Syntax::to_camel_acronym(name),
        Some("kebab") => crate::Syntax::to_snake_acronym(name).replace('_', "-"),
        Some("screaming") => crate::Syntax::to_shouty_acronym(name),
        Some("pascal") => crate::Syntax::to_pascal_acronym(name),
        Some("snake") => crate::Syntax::to_snake_acronym(name),
        _ => name.to_string(),
    }
}

pub(super) fn serde_field_key(
    structure: &crate::AST::StructDef,
    field: &crate::AST::Field,
) -> String {
    jet_foundation::CLISchema::shape_field_names(structure, field)
        .name_for(jet_foundation::Shape::ShapeProjectionKind::Json)
        .expect("checked serde field is missing its JSON shape name")
        .to_owned()
}

