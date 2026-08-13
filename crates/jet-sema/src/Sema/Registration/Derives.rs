use super::*;
use crate::AST::{
    AccessConvention, BinOp, Call, CallArg, CallArgFlags, Expr, Func, ImplDef, Item, Param,
    PatSlot, Pattern, Stmt, SwitchArm, TraitImplBlock, Type, VariantPayload,
};

/// D-ONCE-DERIVE1=A / I3: compiler-owned capability requests lower to the
/// same checked item surface as user-authored code. The builder below creates
/// AST items directly. No generated source is lexed again.
pub(in super::super) fn expand_builtin_derive_items(
    items: &mut Vec<Item>,
    _diags: &mut Vec<Diagnostic>,
) {
    let auto = crate::Traits::TraitRegistry::auto_derives_for_items(items);
    let invalid_distinct_names: std::collections::HashSet<String> = items
        .iter()
        .filter_map(|item| {
            let Item::Distinct(d) = item else { return None };
            let Type::Named(base) = &d.base else { return None };
            items.iter().any(|item| {
                matches!(item, Item::Distinct(other) if other.name == *base)
            }).then(|| d.name.clone())
        })
        .collect();

    let mut generated = Vec::new();
    let snapshot = items.clone();
    for item in &snapshot {
        match item {
            Item::Struct(s) => {
                let comparable = has_derive(&s.derives, crate::Generics::COMPARABLE)
                    || auto.auto_comparable.contains(&s.name);
                let equatable = comparable
                    || has_derive(&s.derives, crate::Generics::EQUATABLE)
                    || auto.auto_equatable.contains(&s.name);
                if equatable || comparable {
                    generated.extend(struct_derive_items(s, equatable, comparable));
                }
            }
            Item::Enum(e) => {
                let comparable = has_derive(&e.derives, crate::Generics::COMPARABLE)
                    || auto.auto_comparable.contains(&e.name);
                let equatable = comparable
                    || has_derive(&e.derives, crate::Generics::EQUATABLE)
                    || auto.auto_equatable.contains(&e.name);
                if equatable || comparable {
                    generated.extend(enum_derive_items(e, equatable, comparable));
                }
            }
            Item::Distinct(d) if !invalid_distinct_names.contains(&d.name) => {
                generated.extend(distinct_derive_items(d));
            }
            Item::UnitFamily(family) => {
                for d in family.distinct_defs() {
                    generated.extend(distinct_derive_items(&d));
                }
            }
            _ => {}
        }
    }

    for item in generated {
        attach_generated_derive_item(items, item);
    }
}

fn has_derive(derives: &[(String, Span)], name: &str) -> bool {
    derives.iter().any(|(derive, _)| derive == name)
}

fn struct_derive_items(
    s: &crate::AST::StructDef,
    equatable: bool,
    comparable: bool,
) -> Vec<Item> {
    let fields: Vec<_> = s.fields.iter().filter(|field| field.computed.is_none()).collect();
    let mut out = Vec::new();
    if equatable {
        let equality = fields
            .iter()
            .map(|field| binary(
                BinOp::Eq,
                field_read("self", &field.name, s.name_span),
                field_read("rhs", &field.name, s.name_span),
                s.name_span,
            ))
            .reduce(|left, right| binary(BinOp::And, left, right, s.name_span))
            .unwrap_or_else(|| Expr::Bool(true, s.name_span));
        out.push(Item::Impl(derive_impl(
            &s.name,
            crate::Generics::EQUATABLE,
            generated_func(
                "equal",
                vec![self_param(s.name_span), named_param("rhs", Type::Named(s.name.clone()), s.name_span)],
                Some(Type::Bool),
                vec![Stmt::Return(Some(equality), s.name_span)],
                s.name_span,
            ),
            s.name_span,
        )));
    }
    if comparable {
        let mut body = Vec::new();
        for field in fields {
            let left = field_read("self", &field.name, s.name_span);
            let right = field_read("rhs", &field.name, s.name_span);
            body.push(if_stmt(
                binary(BinOp::Lt, left.clone(), right.clone(), s.name_span),
                vec![return_ordering("Less", s.name_span)],
                s.name_span,
            ));
            body.push(if_stmt(
                binary(BinOp::Gt, left, right, s.name_span),
                vec![return_ordering("Greater", s.name_span)],
                s.name_span,
            ));
        }
        body.push(Stmt::Return(Some(ordering("Equal", s.name_span)), s.name_span));
        out.push(Item::Impl(derive_impl(
            &s.name,
            crate::Generics::COMPARABLE,
            generated_func(
                "compare",
                vec![self_param(s.name_span), named_param("rhs", Type::Named(s.name.clone()), s.name_span)],
                Some(Type::Named(crate::Syntax::TYPE_ORDERING.to_string())),
                body,
                s.name_span,
            ),
            s.name_span,
        )));
    }
    out
}

fn enum_derive_items(
    e: &crate::AST::EnumDef,
    equatable: bool,
    comparable: bool,
) -> Vec<Item> {
    let mut out = Vec::new();
    if equatable {
        let recursive = e.variants.iter().any(|variant| match &variant.payload {
            VariantPayload::Unit => false,
            VariantPayload::Single(ty, _) => matches!(ty, Type::Named(name) if name == &e.name),
            VariantPayload::Named(fields) => fields.iter().any(|field| {
                matches!(&field.ty, Type::Named(name) if name == &e.name)
            }),
        });
        if recursive {
            let helper_name = format!("_jet_derive_equal_{}", e.name);
            let helper_body = enum_dispatch_body(e, "left", "right", DispatchKind::Equality);
            out.push(Item::Func(generated_func(
                &helper_name,
                vec![
                    named_param("left", Type::Named(e.name.clone()), e.name_span),
                    named_param("right", Type::Named(e.name.clone()), e.name_span),
                ],
                Some(Type::Bool),
                helper_body,
                e.name_span,
            )));
            let call = free_call(
                &helper_name,
                vec![ident("self", e.name_span), ident("rhs", e.name_span)],
                e.name_span,
            );
            out.push(Item::Impl(derive_impl(
                &e.name,
                crate::Generics::EQUATABLE,
                generated_func(
                    "equal",
                    vec![self_param(e.name_span), named_param("rhs", Type::Named(e.name.clone()), e.name_span)],
                    Some(Type::Bool),
                    vec![Stmt::Return(Some(call), e.name_span)],
                    e.name_span,
                ),
                e.name_span,
            )));
        } else {
            out.push(Item::Impl(derive_impl(
                &e.name,
                crate::Generics::EQUATABLE,
                generated_func(
                    "equal",
                    vec![self_param(e.name_span), named_param("rhs", Type::Named(e.name.clone()), e.name_span)],
                    Some(Type::Bool),
                    enum_dispatch_body(e, "self", "rhs", DispatchKind::Equality),
                    e.name_span,
                ),
                e.name_span,
            )));
        }
    }
    if comparable {
        out.push(Item::Impl(derive_impl(
            &e.name,
            crate::Generics::COMPARABLE,
            generated_func(
                "compare",
                vec![self_param(e.name_span), named_param("rhs", Type::Named(e.name.clone()), e.name_span)],
                Some(Type::Named(crate::Syntax::TYPE_ORDERING.to_string())),
                enum_dispatch_body(e, "self", "rhs", DispatchKind::Comparison),
                e.name_span,
            ),
            e.name_span,
        )));
    }
    out
}

fn distinct_derive_items(d: &crate::AST::DistinctDef) -> Vec<Item> {
    let Some(derive_span) = d.derives.first().map(|(_, span)| *span) else {
        return Vec::new();
    };
    let equality = if matches!(d.base, Type::Float | Type::Float32) {
        binary(
            BinOp::And,
            binary(
                BinOp::Le,
                method(ident("self", derive_span), "raw", Vec::new(), derive_span),
                method(ident("rhs", derive_span), "raw", Vec::new(), derive_span),
                derive_span,
            ),
            binary(
                BinOp::Ge,
                method(ident("self", derive_span), "raw", Vec::new(), derive_span),
                method(ident("rhs", derive_span), "raw", Vec::new(), derive_span),
                derive_span,
            ),
            derive_span,
        )
    } else {
        binary(
            BinOp::Eq,
            method(ident("self", derive_span), "raw", Vec::new(), derive_span),
            method(ident("rhs", derive_span), "raw", Vec::new(), derive_span),
            derive_span,
        )
    };
    let mut out = vec![Item::Impl(derive_impl(
        &d.name,
        crate::Generics::EQUATABLE,
        generated_func(
            "equal",
            vec![self_param(derive_span), named_param("rhs", Type::Named(d.name.clone()), derive_span)],
            Some(Type::Bool),
            vec![Stmt::Return(Some(equality), derive_span)],
            derive_span,
        ),
        derive_span,
    ))];
    if has_derive(&d.derives, crate::Generics::COMPARABLE) {
        let self_raw = method(ident("self", derive_span), "raw", Vec::new(), derive_span);
        let rhs_raw = method(ident("rhs", derive_span), "raw", Vec::new(), derive_span);
        out.push(Item::Impl(derive_impl(
            &d.name,
            crate::Generics::COMPARABLE,
            generated_func(
                "compare",
                vec![self_param(derive_span), named_param("rhs", Type::Named(d.name.clone()), derive_span)],
                Some(Type::Named(crate::Syntax::TYPE_ORDERING.to_string())),
                vec![
                    if_stmt(binary(BinOp::Lt, self_raw.clone(), rhs_raw.clone(), derive_span), vec![return_ordering("Less", derive_span)], derive_span),
                    if_stmt(binary(BinOp::Gt, self_raw, rhs_raw, derive_span), vec![return_ordering("Greater", derive_span)], derive_span),
                    Stmt::Return(Some(ordering("Equal", derive_span)), derive_span),
                ],
                derive_span,
            ),
            derive_span,
        )));
    }
    out
}

#[derive(Clone, Copy)]
enum DispatchKind {
    Equality,
    Comparison,
}

fn enum_dispatch_body(
    e: &crate::AST::EnumDef,
    left_name: &str,
    right_name: &str,
    kind: DispatchKind,
) -> Vec<Stmt> {
    let mut outer = Vec::new();
    let mut outer_arms = Vec::new();
    for (left_index, left_variant) in e.variants.iter().enumerate() {
        let left_bindings = payload_bindings(&left_variant.payload, "left");
        let mut inner_arms = Vec::new();
        for (right_index, right_variant) in e.variants.iter().enumerate() {
            let right_bindings = payload_bindings(&right_variant.payload, "right");
            let body = if left_index != right_index {
                match kind {
                    DispatchKind::Equality => vec![Stmt::Return(Some(Expr::Bool(false, e.name_span)), e.name_span)],
                    DispatchKind::Comparison => vec![Stmt::Return(Some(ordering(
                        if left_index < right_index { "Less" } else { "Greater" },
                        e.name_span,
                    )), e.name_span)],
                }
            } else {
                match kind {
                    DispatchKind::Equality => vec![Stmt::Return(Some(
                        equality_expression(&left_bindings, &right_bindings, e.name_span),
                    ), e.name_span)],
                    DispatchKind::Comparison => comparison_body(&left_bindings, &right_bindings, e.name_span),
                }
            };
            inner_arms.push(SwitchArm {
                cond: pattern_test(right_name, right_variant, right_bindings, e.name_span),
                body,
                span: e.name_span,
            });
        }
        outer_arms.push(SwitchArm {
            cond: pattern_test(left_name, left_variant, left_bindings, e.name_span),
            body: vec![Stmt::Switch {
                subject: ident(right_name, e.name_span),
                arms: inner_arms,
                else_body: None,
                span: e.name_span,
            }],
            span: e.name_span,
        });
        let _ = left_index;
    }
    outer.push(Stmt::Switch {
        subject: ident(left_name, e.name_span),
        arms: outer_arms,
        else_body: None,
        span: e.name_span,
    });
    outer.push(Stmt::Return(
        Some(match kind {
            DispatchKind::Equality => Expr::Bool(false, e.name_span),
            DispatchKind::Comparison => ordering("Equal", e.name_span),
        }),
        e.name_span,
    ));
    outer
}

fn comparison_body(left: &[String], right: &[String], span: Span) -> Vec<Stmt> {
    let mut body = Vec::new();
    for (left, right) in left.iter().zip(right) {
        body.push(if_stmt(
            binary(BinOp::Lt, ident(left, span), ident(right, span), span),
            vec![return_ordering("Less", span)],
            span,
        ));
        body.push(if_stmt(
            binary(BinOp::Gt, ident(left, span), ident(right, span), span),
            vec![return_ordering("Greater", span)],
            span,
        ));
    }
    body.push(Stmt::Return(Some(ordering("Equal", span)), span));
    body
}

fn equality_expression(left: &[String], right: &[String], span: Span) -> Expr {
    left.iter()
        .zip(right)
        .map(|(left, right)| binary(BinOp::Eq, ident(left, span), ident(right, span), span))
        .reduce(|left, right| binary(BinOp::And, left, right, span))
        .unwrap_or(Expr::Bool(true, span))
}

fn payload_bindings(payload: &VariantPayload, prefix: &str) -> Vec<String> {
    let count = match payload {
        VariantPayload::Unit => 0,
        VariantPayload::Single(..) => 1,
        VariantPayload::Named(fields) => fields.len(),
    };
    (0..count).map(|index| format!("{prefix}_{index}")).collect()
}

fn pattern_test(
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

fn attach_generated_derive_item(items: &mut Vec<Item>, item: Item) {
    let Item::Impl(implementation) = item else {
        if let Item::Func(function) = item {
            if !items.iter().any(|existing| {
                matches!(existing, Item::Func(old) if old.name == function.name)
            }) {
                items.push(Item::Func(function));
            }
        }
        return;
    };
    let Some(trait_name) = implementation.trait_name.clone() else { return };
    if has_trait_impl(items, &implementation.type_name, &trait_name) {
        return;
    }
    if let Some(target) = items.iter_mut().find_map(|item| match item {
        Item::Struct(s) if s.name == implementation.type_name => Some(&mut s.trait_impls),
        Item::Enum(e) if e.name == implementation.type_name => Some(&mut e.trait_impls),
        _ => None,
    }) {
        target.push(TraitImplBlock {
            trait_name,
            trait_span: implementation.trait_span.unwrap_or(implementation.type_span),
            methods: implementation.methods,
            assoc_type_impls: implementation.assoc_type_impls,
        });
    } else {
        items.push(Item::Impl(implementation));
    }
}

fn has_trait_impl(items: &[Item], type_name: &str, trait_name: &str) -> bool {
    items.iter().any(|item| match item {
        Item::Impl(i) => i.type_name == type_name && i.trait_name.as_deref() == Some(trait_name),
        Item::Struct(s) => s.name == type_name && s.trait_impls.iter().any(|i| i.trait_name == trait_name),
        Item::Enum(e) => e.name == type_name && e.trait_impls.iter().any(|i| i.trait_name == trait_name),
        _ => false,
    })
}

fn derive_impl(type_name: &str, trait_name: &str, method: Func, span: Span) -> ImplDef {
    ImplDef {
        span,
        type_name: type_name.to_string(),
        type_span: span,
        trait_name: Some(trait_name.to_string()),
        trait_span: Some(span),
        methods: vec![method],
        delegation_field: None,
        assoc_type_impls: Vec::new(),
        is_generated_serde: false,
        os_target: None,
    }
}

fn generated_func(
    name: &str,
    params: Vec<Param>,
    return_type: Option<Type>,
    body: Vec<Stmt>,
    span: Span,
) -> Func {
    let mut function = Func::implicit_run(body, span);
    function.name = name.to_string();
    function.name_span = span;
    function.params = params;
    function.return_type = return_type;
    function.return_type_span = function.return_type.as_ref().map(|_| span);
    function
}

fn self_param(span: Span) -> Param {
    named_param_with_ty("self", Type::Named(String::new()), span, AccessConvention::Read)
}

fn named_param(name: &str, ty: Type, span: Span) -> Param {
    named_param_with_ty(name, ty, span, AccessConvention::Read)
}

fn named_param_with_ty(name: &str, ty: Type, span: Span, convention: AccessConvention) -> Param {
    Param {
        convention,
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

fn ident(name: &str, span: Span) -> Expr {
    Expr::Ident(name.to_string(), span)
}

fn field_read(base: &str, field: &str, span: Span) -> Expr {
    Expr::Field(Box::new(ident(base, span)), field.to_string(), span)
}

fn binary(op: BinOp, left: Expr, right: Expr, span: Span) -> Expr {
    Expr::Binary(op, Box::new(left), Box::new(right), span)
}

fn call_arg(expr: Expr, span: Span) -> CallArg {
    CallArg {
        convention: AccessConvention::Read,
        expr,
        span,
        flags: CallArgFlags::default(),
        label: None,
        spread: false,
    }
}

fn free_call(name: &str, args: Vec<Expr>, span: Span) -> Expr {
    Expr::Call(Call {
        name: name.to_string(),
        name_span: span,
        type_args: Vec::new(),
        args: args.into_iter().map(|expr| call_arg(expr, span)).collect(),
        resolved_ret: None,
        range_checked: false,
        widen_approx: false,
    })
}

fn method(receiver: Expr, name: &str, args: Vec<Expr>, span: Span) -> Expr {
    Expr::MethodCall {
        receiver: Box::new(receiver),
        method: name.to_string(),
        method_span: span,
        owner_type_args: Vec::new(),
        type_args: Vec::new(),
        args: args.into_iter().map(|expr| call_arg(expr, span)).collect(),
        recv_type: None,
        resolved_ret: None,
        checked_widen: false,
    }
}

fn ordering(variant: &str, span: Span) -> Expr {
    Expr::EnumLit {
        type_name: crate::Syntax::TYPE_ORDERING.to_string(),
        variant: variant.to_string(),
        args: Vec::new(),
        leading_dot: false,
        span,
    }
}

fn return_ordering(variant: &str, span: Span) -> Stmt {
    Stmt::Return(Some(ordering(variant, span)), span)
}

fn if_stmt(cond: Expr, body: Vec<Stmt>, span: Span) -> Stmt {
    Stmt::Switch {
        subject: Expr::Bool(true, span),
        arms: vec![SwitchArm { cond, body, span }],
        else_body: Some(Vec::new()),
        span,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_derive_is_an_ast_item_not_source() {
        let (tokens, lex_diags) = crate::Lexer::lex("#Comparable struct Point { value: Int }");
        assert!(lex_diags.is_empty());
        let mut program = crate::Parser::parse(&tokens).expect("source parses");
        let mut diags = Vec::new();
        expand_builtin_derive_items(&mut program.items, &mut diags);
        assert!(diags.is_empty(), "built-in derive diagnostics: {diags:?}");
        assert!(program.items.iter().any(|item| matches!(
            item,
            Item::Struct(s)
                if s.trait_impls
                    .iter()
                    .any(|implementation| implementation.trait_name == Syntax::MARKER_COMPARABLE)
        )));
    }
}
