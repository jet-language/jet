use crate::jet_generated_format as jet_format;
use crate::Codegen::mangle;
use crate::Codegen::Cx;
use crate::Codegen::TIR::clone_env;
use crate::Codegen::TIR::lower_expr;
use crate::Codegen::TIR::lower_lambda_expecting;
use crate::Codegen::TIR::lower_lambda_expecting_callable;
use crate::Codegen::TIR::unit_type;
use crate::Codegen::TIR::with_lambda_body_expr_cache;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::TCallArg;
use crate::Codegen::TIR::TEnumArg;
use crate::Codegen::TIR::TEnumPayload;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::TExternArg;
use crate::Codegen::TIR::TFnCoerce;
use crate::Codegen::TIR::TLocal;
use crate::Codegen::TIR::TStrPart;
use crate::Diagnostics::Span;
use crate::AST::{AccessConvention, CtValue, Expr, Lambda, LambdaBody, Stmt, StrPart, Type};

/// D-UNIONTYPE1=A: wrap a member value into the compiler-generated union enum.
pub(crate) fn maybe_widen_expr_to_union(value: TExpr, want: &Type) -> TExpr {
    match want {
        Type::Union(members)
            if members.iter().any(|m| m == &value.ty) && !matches!(&value.ty, Type::Union(_)) =>
        {
            let enum_type = crate::AST::union_enum_name(members);
            let variant = crate::AST::union_member_tag(&value.ty);
            TExpr {
                ty: want.clone(),
                kind: TExprKind::EnumLit {
                    enum_type,
                    variant,
                    payload: TEnumPayload::Positional(vec![TEnumArg {
                        value,
                        clone: false,
                        boxed: false,
                    }]),
                },
            }
        }
        _ => value,
    }
}

/// Last expression-producing statement in a lambda block (mirrors sema tail rules).
/// Only the **final** statement may be a tail; an earlier `send()`/`call()` followed
/// by a loop is not a tail expression.
pub(super) fn lambda_block_tail<'a>(stmts: &'a [Stmt]) -> Option<(&'a [Stmt], &'a Stmt)> {
    let last_idx = stmts.len().checked_sub(1)?;
    let last = &stmts[last_idx];
    match last {
        Stmt::Return(Some(_), _) | Stmt::Expr(_) => Some((&stmts[..last_idx], last)),
        _ => None,
    }
}

fn callback_fn_type(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Fn { .. } => Some(ty),
        Type::Tagged { marker, inner }
            if matches!(
                marker,
                crate::AST::TagMarker::Internal(crate::AST::InternalTag::CppCallbackAbi)
            ) && matches!(inner.as_ref(), Type::Fn { .. }) =>
        {
            Some(inner)
        }
        _ => None,
    }
}

fn is_opaque_handle_type(ty: &Type, cx: &Cx) -> bool {
    match ty {
        Type::Named(name) => cx.opaque_handles.contains(name),
        Type::Tagged { inner, .. } => is_opaque_handle_type(inner, cx),
        _ => false,
    }
}
fn invariant_arg_value(
    arg: &crate::AST::CallArg,
    construct: impl Into<String>,
) -> TExpr {
    TExpr {
        ty: Type::Named(crate::Syntax::TYPE_NEVER.to_string()),
        kind: TExprKind::InvariantViolation {
            construct: construct.into(),
            span: arg.span,
        },
    }
}

fn invariant_call_arg(
    arg: &crate::AST::CallArg,
    construct: impl Into<String>,
) -> TCallArg {
    TCallArg {
        value: invariant_arg_value(arg, construct),
        template_items: None,
        borrow: false,
        mut_borrow: false,
        clone: false,
        arc_clone: false,
        fn_coerce: None,
        widen_to_vec: false,
        widen_to_union: None,
        box_as_trait: None,
    }
}

fn invariant_extern_arg(
    arg: &crate::AST::CallArg,
    construct: impl Into<String>,
) -> TExternArg {
    TExternArg {
        value: invariant_arg_value(arg, construct),
        clone: false,
        mut_borrow: false,
    }
}

fn template_never_expr(name: impl Into<String>) -> TExpr {
    TExpr {
        ty: Type::Named(crate::Syntax::TYPE_NEVER.to_string()),
        kind: TExprKind::Local(TLocal::user(name)),
    }
}

fn template_reference_expr(name: &str, env: &LowerEnv) -> TExpr {
    let bare_name = name.trim_start_matches('@');
    let marked_name = format!("@{bare_name}");
    let local_name = if env.locals.contains_key(name) {
        name
    } else if env.locals.contains_key(bare_name) {
        bare_name
    } else if env.locals.contains_key(&marked_name) {
        marked_name.as_str()
    } else {
        name
    };
    let mut value = template_never_expr(local_name);
    if let Some(ty) = env.ty_of(local_name) {
        value.ty = ty;
        value.kind = TExprKind::Local(env.local_of(local_name));
    }
    value
}

fn template_expr_root(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident(name, _) | Expr::ComptimeName { name, .. } => Some(name),
        Expr::Field(base, ..) | Expr::OptField { base, .. } => template_expr_root(base),
        Expr::Paren(inner, _) => template_expr_root(inner),
        _ => None,
    }
}

fn lower_template_expr(
    expr: &Expr,
    cx: &Cx,
    env: &mut LowerEnv,
    template_vars: &std::collections::HashSet<String>,
) -> TExpr {
    match expr {
        Expr::ComptimeName { name, .. } if cx.const_values.contains_key(name) => {
            lower_expr(expr, cx, env)
        }
        Expr::ComptimeName { value: Some(_), .. } => lower_expr(expr, cx, env),
        Expr::ComptimeName { name, .. } => template_reference_expr(name, env),
        Expr::Ident(name, _) if name.starts_with('@') => template_reference_expr(name, env),
        Expr::Ident(name, _) if template_vars.contains(name) => {
            template_reference_expr(name, env)
        }
        Expr::Field(base, member, _)
            if member.starts_with('@')
                || template_expr_root(base)
                    .is_some_and(|name| template_vars.contains(name.trim_start_matches('@'))) =>
        {
            let recv = lower_template_expr(base, cx, env, template_vars);
            TExpr {
                ty: Type::Named(crate::Syntax::TYPE_NEVER.to_string()),
                kind: TExprKind::Field {
                    recv: Box::new(recv),
                    field: member.trim_start_matches('@').to_string(),
                    boxed: false,
                },
            }
        }
        Expr::Str(parts, _) => TExpr {
            ty: Type::String,
            kind: TExprKind::StrLit(
                parts
                    .iter()
                    .map(|part| match part {
                        StrPart::Lit(text) => TStrPart::Lit(text.clone()),
                        StrPart::Interp(value, format) => TStrPart::Interp(
                            lower_template_expr(value, cx, env, template_vars),
                            format.clone(),
                        ),
                    })
                    .collect(),
            ),
        },
        Expr::ListLit(values, _) => TExpr {
            ty: Type::List(Box::new(Type::Named(crate::Syntax::TYPE_NEVER.to_string()))),
            kind: TExprKind::ListLit(
                values
                    .iter()
                    .map(|value| lower_template_expr(value, cx, env, template_vars))
                    .collect(),
            ),
        },
        Expr::Paren(inner, _) => lower_template_expr(inner, cx, env, template_vars),
        _ => lower_expr(expr, cx, env),
    }
}

fn lower_template_loop_expr(
    expr: &Expr,
    cx: &Cx,
    env: &mut LowerEnv,
    template_vars: &std::collections::HashSet<String>,
) -> TExpr {
    if let Expr::ListLit(values, _) = expr {
        if let Some(items) = values
            .iter()
            .map(|value| {
                let name = match value {
                    Expr::Ident(name, _) | Expr::ComptimeName { name, .. } => name,
                    _ => return None,
                };
                Some(TExpr {
                    ty: Type::String,
                    kind: TExprKind::CtLit(CtValue::Str(
                        name.trim_start_matches('@').to_string(),
                    )),
                })
            })
            .collect::<Option<Vec<_>>>()
        {
            return TExpr {
                ty: Type::List(Box::new(Type::String)),
                kind: TExprKind::ListLit(items),
            };
        }
    }
    lower_template_expr(expr, cx, env, template_vars)
}

struct TemplateMarker {
    start: usize,
    end: usize,
    value: bool,
}

fn scan_template_markers(
    source: &str,
    template_vars: &std::collections::HashSet<String>,
) -> Vec<TemplateMarker> {
    fn ident(byte: u8) -> bool {
        byte.is_ascii_alphanumeric() || byte == b'_'
    }
    fn name_context(bytes: &[u8], start: usize) -> bool {
        let mut end = start;
        while end > 0 && bytes[end - 1].is_ascii_whitespace() {
            end -= 1;
        }
        if end > 0 && bytes[end - 1] == b'.' {
            return true;
        }
        let word_end = end;
        while end > 0 && ident(bytes[end - 1]) {
            end -= 1;
        }
        std::str::from_utf8(&bytes[end..word_end]).is_ok_and(|word| {
            matches!(
                word,
                "fn" | "impl" | "struct" | "enum" | "type" | "trait" | "module" | "const"
            )
        })
    }
    fn skip_quoted(bytes: &[u8], mut at: usize, quote: u8) -> usize {
        at += 1;
        while at < bytes.len() {
            match bytes[at] {
                b'\\' => at = at.saturating_add(2),
                current if current == quote => return at + 1,
                _ => at += 1,
            }
        }
        bytes.len()
    }
    fn scan_code(
        bytes: &[u8],
        mut at: usize,
        stop_at_brace: bool,
        value_context: bool,
        template_vars: &std::collections::HashSet<String>,
        out: &mut Vec<TemplateMarker>,
    ) -> usize {
        while at < bytes.len() {
            match bytes[at] {
                b'/' if bytes.get(at + 1) == Some(&b'/') => {
                    at += 2;
                    while at < bytes.len() && bytes[at] != b'\n' {
                        at += 1;
                    }
                }
                b'/' if bytes.get(at + 1) == Some(&b'*') => {
                    at += 2;
                    let mut nested = 1usize;
                    while at < bytes.len() && nested > 0 {
                        if bytes
                            .get(at..at + 2)
                            .is_some_and(|pair| pair == b"/*")
                        {
                            nested += 1;
                            at += 2;
                        } else if bytes
                            .get(at..at + 2)
                            .is_some_and(|pair| pair == b"*/")
                        {
                            nested -= 1;
                            at += 2;
                        } else {
                            at += 1;
                        }
                    }
                }
                b'"' => {
                    at = scan_string(bytes, at, template_vars, out);
                }
                b'\'' => {
                    at = skip_quoted(bytes, at, b'\'');
                }
                b'{' if value_context => {
                    at = scan_code(bytes, at + 1, true, true, template_vars, out);
                }
                b'{' => at += 1,
                b'}' if stop_at_brace => {
                    return at + 1;
                }
                byte if ident(byte) => {
                    let start = at;
                    at += 1;
                    while at < bytes.len() && ident(bytes[at]) {
                        at += 1;
                    }
                    let name = std::str::from_utf8(&bytes[start..at]).ok();
                    let mut marker_end = at;
                    let mut field_chain = false;
                    if name.is_some_and(|name| template_vars.contains(name)) {
                        loop {
                            let Some(&b'.') = bytes.get(marker_end) else {
                                break;
                            };
                            let field_start = marker_end + 1;
                            let Some(&first) = bytes.get(field_start) else {
                                break;
                            };
                            if first == b'@' || !ident(first) {
                                break;
                            }
                            let mut field_end = field_start + 1;
                            while field_end < bytes.len() && ident(bytes[field_end]) {
                                field_end += 1;
                            }
                            marker_end = field_end;
                            field_chain = true;
                        }
                    }
                    if let Some(name) = name {
                        if template_vars.contains(name)
                            && (marker_end != at || bytes.get(at) != Some(&b'.'))
                        {
                            out.push(TemplateMarker {
                                start,
                                end: marker_end,
                                value: value_context || field_chain,
                            });
                        }
                    }
                    at = marker_end;
                }
                b'@' => {
                    let start = at;
                    at += 1;
                    while at < bytes.len() && ident(bytes[at]) {
                        at += 1;
                    }
                    if at > start + 1 {
                        let name = &bytes[start + 1..at];
                        if name != b"loop" {
                            out.push(TemplateMarker {
                                start,
                                end: at,
                                value: value_context || !name_context(bytes, start),
                            });
                        }
                    }
                }
                _ => at += 1,
            }
        }
        bytes.len()
    }
    fn scan_string(
        bytes: &[u8],
        mut at: usize,
        template_vars: &std::collections::HashSet<String>,
        out: &mut Vec<TemplateMarker>,
    ) -> usize {
        at += 1;
        while at < bytes.len() {
            match bytes[at] {
                b'\\' => at = at.saturating_add(2),
                b'"' => return at + 1,
                b'{' if bytes.get(at + 1) == Some(&b'{') => {
                    at += 2;
                    while at < bytes.len() {
                        if bytes
                            .get(at..at + 2)
                            .is_some_and(|pair| pair == b"}}")
                        {
                            at += 2;
                            break;
                        }
                        at += 1;
                    }
                }
                b'{' => {
                    at = scan_code(bytes, at + 1, true, true, template_vars, out);
                }
                _ => at += 1,
            }
        }
        bytes.len()
    }
    let mut markers = Vec::new();
    scan_code(
        source.as_bytes(),
        0,
        false,
        false,
        template_vars,
        &mut markers,
    );
    markers
}

fn template_marker_base(source: &str, start: usize) -> Option<String> {
    let before = source.get(..start)?.strip_suffix('.')?;
    let end = before.len();
    let begin = before
        .char_indices()
        .rev()
        .find_map(|(index, character)| {
            (!character.is_ascii_alphanumeric() && character != '_').then_some(index + character.len_utf8())
        })
        .unwrap_or(0);
    (begin < end).then(|| before[begin..end].trim_start_matches('@').to_string())
}

fn template_path_expr(
    path: &str,
    env: &LowerEnv,
    template_vars: &std::collections::HashSet<String>,
) -> Option<TExpr> {
    let mut segments = path.split('.');
    let root_segment = segments.next()?;
    let explicit_root = root_segment.starts_with('@');
    let root = root_segment.trim_start_matches('@');
    if root.is_empty()
        || (!explicit_root
            && !template_vars.contains(root)
            && !env.locals.contains_key(root)
            && !env.locals.contains_key(&format!("@{root}")))
    {
        return None;
    }
    let root_name = if explicit_root {
        format!("@{root}")
    } else {
        root.to_string()
    };
    let mut value = template_reference_expr(&root_name, env);
    for field in segments {
        let field = field.trim_start_matches('@');
        if field.is_empty() {
            return None;
        }
        value = TExpr {
            ty: Type::Named(crate::Syntax::TYPE_NEVER.to_string()),
            kind: TExprKind::Field {
                recv: Box::new(value),
                field: field.to_string(),
                boxed: false,
            },
        };
    }
    Some(value)
}

fn template_marker_expr(
    source: &str,
    marker: &TemplateMarker,
    cx: &Cx,
    env: &LowerEnv,
    template_vars: &std::collections::HashSet<String>,
) -> TExpr {
    let marker_text = &source[marker.start..marker.end];
    if marker_text.contains('.') {
        if let Some(value) = template_path_expr(marker_text, env, template_vars) {
            return value;
        }
    }
    let marker_name = marker_text.strip_prefix('@').unwrap_or(marker_text);
    if template_vars.contains(marker_name) {
        return template_reference_expr(marker_name, env);
    }
    if let Some(base) = template_marker_base(source, marker.start) {
        let marked_base = format!("@{base}");
        if template_vars.contains(&base)
            || env.locals.contains_key(&base)
            || env.locals.contains_key(&marked_base)
        {
            return TExpr {
                ty: Type::Named(crate::Syntax::TYPE_NEVER.to_string()),
                kind: TExprKind::Field {
                    recv: Box::new(template_reference_expr(&base, env)),
                    field: source[marker.start + 1..marker.end].to_string(),
                    boxed: false,
                },
            };
        }
    }
    let _ = cx;
    template_reference_expr(&source[marker.start..marker.end], env)
}

fn template_source(
    span: Span,
    cx: &Cx,
    env: &LowerEnv,
    template_vars: &std::collections::HashSet<String>,
) -> Option<crate::Comptime::TemplateSource<TExpr>> {
    let source = cx.src.get(span.start..span.end)?.to_string();
    let holes = scan_template_markers(&source, template_vars)
        .into_iter()
        .map(|marker| crate::Comptime::TemplateHole {
            start: marker.start,
            end: marker.end,
            expr: Box::new(template_marker_expr(
                &source,
                &marker,
                cx,
                env,
                template_vars,
            )),
            kind: if marker.value {
                crate::Comptime::TemplateHoleKind::Value
            } else {
                crate::Comptime::TemplateHoleKind::Name
            },
            span: Span::new(span.start + marker.start, span.start + marker.end),
        })
        .collect();
    Some(crate::Comptime::TemplateSource {
        source,
        holes,
        span,
    })
}

fn item_span(item: &crate::AST::Item, cx: &Cx) -> Span {
    match item {
        crate::AST::Item::Func(item) => item.span,
        crate::AST::Item::Struct(item) => item.span,
        crate::AST::Item::Enum(item) => item.span,
        crate::AST::Item::Distinct(item) => item.span,
        crate::AST::Item::TypeAlias(item) => item.span,
        crate::AST::Item::UnitFamily(item) => item.span,
        crate::AST::Item::Trait(item) => item.span,
        crate::AST::Item::Tag(item) => item.span,
        crate::AST::Item::EffectDecl(item) => item.span,
        crate::AST::Item::Impl(item) => item.span,
        crate::AST::Item::Const(item) => item.span,
        crate::AST::Item::Test(item) => item.span,
        crate::AST::Item::ExternRust(item) => item.span,
        crate::AST::Item::Module(item) => item.span,
        crate::AST::Item::CModule(item) => item.span,
        crate::AST::Item::CodeModule(item) => item.span,
        crate::AST::Item::ErrorConv(item) => {
            let line_start = cx
                .src
                .get(..item.from_span.start)
                .and_then(|source| source.rfind('\n'))
                .map_or(0, |index| index + 1);
            let start = cx
                .src
                .get(line_start..item.from_span.start)
                .and_then(|line| line.rfind("impl "))
                .map_or(item.from_span.start, |offset| line_start + offset);
            Span::new(start, item.body_span.end)
        }
        crate::AST::Item::Migration(item) => item.span,
        crate::AST::Item::ProtocolDecl(item) => item.span,
        crate::AST::Item::UserDerive(item) => item.span,
        crate::AST::Item::TemplateLoop(item) => item.span,
        crate::AST::Item::GenericModule(item) => item.span,
        crate::AST::Item::ModuleAlias(item) => item.span,
        crate::AST::Item::MarkerDecl(item) => item.span,
        crate::AST::Item::FactDecl(item) => item.span,
    }
}

fn lower_template_stmt(
    statement: &Stmt,
    cx: &Cx,
    env: &mut LowerEnv,
    template_vars: &std::collections::HashSet<String>,
) -> Vec<Box<crate::Comptime::TemplateItem<TExpr>>> {
    match statement {
        Stmt::Val(binding) if !binding.name.is_empty() && binding.pattern.is_none() => {
            let name = binding.name.trim_start_matches('@').to_string();
            let value = lower_template_expr(&binding.init, cx, env, template_vars);
            env.bind(&name, TLocal::user(name.clone()), Some(value.ty.clone()));
            vec![Box::new(crate::Comptime::TemplateItem::Statement(
                crate::Comptime::TemplateStatement::Binding {
                    name,
                    value: Box::new(value),
                    span: statement.span(),
                },
            ))]
        }
        Stmt::Expr(expr) => {
            vec![Box::new(crate::Comptime::TemplateItem::Statement(
                crate::Comptime::TemplateStatement::Expr {
                    value: Box::new(lower_template_expr(expr, cx, env, template_vars)),
                    span: statement.span(),
                },
            ))]
        }
        Stmt::ComptimeIf {
            cond,
            then_body,
            else_body,
            span,
            selected_then,
            ..
        } => {
            let then_body = lower_template_stmts(then_body, cx, env, template_vars);
            let else_body = match else_body {
                Some(body) => lower_template_stmts(body, cx, env, template_vars),
                None => Vec::new(),
            };
            vec![Box::new(crate::Comptime::TemplateItem::If {
                condition: Box::new(lower_template_expr(cond, cx, env, template_vars)),
                then_body,
                else_body,
                selected: *selected_then,
                span: *span,
            })]
        }
        Stmt::ComptimeBlock { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Unsafe { body, .. } => lower_template_stmts(body, cx, env, template_vars),
        _ => vec![Box::new(crate::Comptime::TemplateItem::Invalid {
            construct: "unsupported checked template statement shape".to_string(),
            span: statement.span(),
        })],
    }
}

fn lower_template_stmts(
    statements: &[Stmt],
    cx: &Cx,
    env: &mut LowerEnv,
    template_vars: &std::collections::HashSet<String>,
) -> Vec<Box<crate::Comptime::TemplateItem<TExpr>>> {
    statements
        .iter()
        .flat_map(|statement| lower_template_stmt(statement, cx, env, template_vars))
        .collect()
}

fn lower_template_items(
    items: &[crate::AST::DeriveBodyItem],
    body_span: Span,
    cx: &Cx,
    env: &mut LowerEnv,
    template_vars: &std::collections::HashSet<String>,
) -> crate::Comptime::TemplateBody<TExpr> {
    let mut lowered = Vec::new();
    for item in items {
        match item {
            crate::AST::DeriveBodyItem::Item(item) => {
                let span = item_span(item, cx);
                let lowered_item = match template_source(span, cx, env, template_vars) {
                    Some(source) => crate::Comptime::TemplateItem::Item(source),
                    None => crate::Comptime::TemplateItem::Invalid {
                        construct: "template item source span is outside the source file".to_string(),
                        span,
                    },
                };
                lowered.push(Box::new(lowered_item));
            }
            crate::AST::DeriveBodyItem::Stmt(statement) => {
                lowered.extend(lower_template_stmt(statement, cx, env, template_vars));
            }
            crate::AST::DeriveBodyItem::Loop {
                var,
                source,
                body,
                span,
                ..
            } => {
                let source = lower_template_loop_expr(source, cx, env, template_vars);
                let element_ty = match &source.ty {
                    Type::List(inner) | Type::FixedList { elem: inner, .. } => {
                        Some(inner.as_ref().clone())
                    }
                    _ => None,
                };
                let mut loop_env = clone_env(env);
                loop_env.bind(var, TLocal::user(var.clone()), element_ty);
                let mut nested_vars = template_vars.clone();
                nested_vars.insert(var.clone());
                let body =
                    lower_template_items(body, *span, cx, &mut loop_env, &nested_vars).items;
                lowered.push(Box::new(crate::Comptime::TemplateItem::Loop {
                    var: var.clone(),
                    source: Box::new(source),
                    body,
                    span: *span,
                }));
            }
        }
    }
    crate::Comptime::TemplateBody {
        items: lowered,
        span: body_span,
    }
}

/// Lower a named function for a collection adapter's callback ABI.
///
/// Ordinary function values carry their effective failure result. Collection
/// adapters choose either that carrier or the source-success callback shape;
/// this helper selects the requested view and emits the one boundary wrapper.
pub(crate) fn lower_named_collection_callback(
    expr: &Expr,
    cx: &Cx,
    env: &LowerEnv,
    effective: bool,
    params: Option<&[Type]>,
) -> Option<TExpr> {
    let Expr::Ident(name, _) = expr else {
        return None;
    };
    if env.locals.contains_key(name) || cx.consts.contains_key(name) {
        return None;
    }
    let ty = if effective {
        cx.fn_types.get(name)
    } else {
        cx.fn_source_types.get(name)
    }?;
    if !matches!(ty, Type::Fn { .. }) {
        return None;
    }
    let callable = TExpr {
        ty: ty.clone(),
        kind: TExprKind::FnValue {
            kind: crate::Codegen::TIR::TFnValueKind::NamedFn {
                name: Some(name.clone()),
                lambda: None,
            },
        },
    };
    // Collection Prelude callbacks borrow their inputs. Keep the named
    // function value behind one direct closure so the adapter receives
    // `Fn(&T, ...)`, not an Rc whose ABI is incompatible with `Fn`.
    if let Some(params) = params {
        return Some(TExpr {
            ty: callable.ty.clone(),
            kind: TExprKind::HostBorrowCallback {
                callable: Box::new(callable),
                params: params.to_vec(),
            },
        });
    }
    Some(callable)
}

/// Compare element types at the fixed-list to growable-list boundary.
///
/// `Int` is the canonical signed 64-bit integer. Keep this comparison
/// structural: a narrower or unsigned integer must never be widened without
/// an explicit conversion.
pub(crate) fn fixed_list_elem_compatible(actual: &Type, want: &Type) -> bool {
    fn canonical(ty: &Type) -> Type {
        match ty.erased_carrier() {
            Type::IntN {
                signed: true,
                bits: 64,
            } => Type::Int,
            ty => ty,
        }
    }

    canonical(actual) == canonical(want)
}

/// c109 Phase 13: the type of a lambda's body (its return), used for a `spawn`ed
/// closure's `Task<T>` element type. Block bodies use the tail expression/return
/// (same rule as sema), not `Unit`.
pub(crate) fn lambda_body_ty(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> Type {
    lambda_body_ty_expecting(lam, cx, env, None)
}

/// D-CONC-SPAWN1: the source-level element type of a spawned task. The
/// callable's effective failure carrier is an execution detail; `Task<T>`
/// exposes the body's successful value `T` to `join()` and task combinators.
pub(crate) fn spawn_body_result_ty(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> Type {
    lambda_body_ty(lam, cx, env)
}

/// D-CONC-FAIL1=A: the private carrier returned by a spawned closure. Keep
/// this separate from [`spawn_body_result_ty`]: the worker closure may use a
/// `Result`/`Option` carrier while the source-level task remains `Task<T>`.
pub(crate) fn spawn_body_carrier_ty(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> Type {
    let t = spawn_body_result_ty(lam, cx, env);
    let checked_carrier = lam.meta.fallible_carrier.as_ref().or_else(|| {
        lam.meta
            .fallible_propagation
            .then(|| env.ret_ty.as_ref())
            .flatten()
    });
    if let Some(carrier) = checked_carrier {
        return match carrier {
            Type::Result { err, .. } => Type::Result {
                ok: Box::new(match &t {
                    Type::Result { ok, .. } => (**ok).clone(),
                    other => other.clone(),
                }),
                err: err.clone(),
            },
            Type::Option(_) => Type::Option(Box::new(match &t {
                Type::Option(inner) => (**inner).clone(),
                other => other.clone(),
            })),
            other => other.clone(),
        };
    }
    match t {
        Type::Result { .. } | Type::Option(_) => t,
        t => Type::Result {
            ok: Box::new(t),
            err: Box::new(Type::Named(crate::Syntax::TYPE_ERR.to_string())),
        },
    }
}

/// `lambda_body_ty`, but seeding a bare (unannotated) param from `expected_params`
/// at the same position — same fallback `lower_lambda_expecting` uses (D-MEM1 S6:
/// `Shared<T>.read(s => …)`'s bare `s` needs its real type here too, or the
/// closure's OWN return type comes back wrong for a chained field/method read).
pub(crate) fn lambda_body_ty_expecting(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
    expected_params: Option<&[Type]>,
) -> Type {
    fn bind_params(lam: &Lambda, env: &LowerEnv, expected_params: Option<&[Type]>) -> LowerEnv {
        let mut lam_env = clone_env(env);
        lam_env.fallback_subject = false;
        for (i, p) in lam.params.iter().enumerate() {
            let ty =
                p.ty.clone()
                    .or_else(|| expected_params.and_then(|ps| ps.get(i)).cloned());
            lam_env
                .locals
                .insert(p.name.clone(), (TLocal::user(&p.name), ty));
        }
        lam_env
    }
    // Type probing lowers the expression to recover its total type, but the
    // probe is not the executable TIR pass. Do not publish spawn callbacks it
    // discovers into the shared JIT lambda table — table AND site map, or a
    // later pass would dedup onto an index whose entry was just discarded; the
    // real lowering pass that follows owns those entries and their site
    // indexes. The probe also binds params in an env of its own, so it gets
    // its own expression memo.
    let saved_spawn_lambdas = std::mem::take(&mut *cx.jit_spawn_lambdas.borrow_mut());
    let saved_spawn_sites = cx.jit_spawn_sites.borrow().clone();
    let body_ty = with_lambda_body_expr_cache(|| match &lam.body {
        LambdaBody::Expr(e) => {
            let mut lam_env = bind_params(lam, env, expected_params);
            lower_expr(e, cx, &mut lam_env).ty
        }
        LambdaBody::Block(stmts) => {
            if let Some((_, tail)) = lambda_block_tail(stmts) {
                let mut lam_env = bind_params(lam, env, expected_params);
                match tail {
                    Stmt::Return(Some(e), _) | Stmt::Expr(e) => lower_expr(e, cx, &mut lam_env).ty,
                    _ => unit_type(),
                }
            } else {
                unit_type()
            }
        }
    });
    *cx.jit_spawn_lambdas.borrow_mut() = saved_spawn_lambdas;
    *cx.jit_spawn_sites.borrow_mut() = saved_spawn_sites;
    body_ty
}

/// c109 Phase 6/13: lower method-call arguments. The clone/Arc, borrow, and mut-borrow
/// wrappers, and the Fn-typed Box-coercion are all decided here from total facts
/// (`CallArg.flags` + the resolved param convention/type), never re-derived in emit.
pub(crate) fn lower_method_args(
    args: &[crate::AST::CallArg],
    sig: &[(AccessConvention, Type)],
    env: &mut LowerEnv,
    cx: &Cx,
) -> Vec<TCallArg> {
    args.iter()
        .enumerate()
        .map(|(i, a)| {
            let Some((convention, ty)) = sig.get(i) else {
                return invariant_call_arg(
                    a,
                    format!("method call argument {} has no resolved parameter", i + 1),
                );
            };
            lower_one_call_arg(a, Some((*convention, ty.clone())), env, cx)
        })
        .collect()
}

/// c109 Phase 13: lower ONE call argument — the single source of truth for
/// the clone/Arc, Fn-coercion, and borrow/mut-borrow wrapper order. `conv` is the
/// resolved param `(convention, type)` for this position (`None` when the callee has
/// no known signature, e.g. a `CallValue`). The emit order is exactly the AST path's:
///   1. the implicit-clone / Arc-clone wrapper (`(…).clone()` / `Arc::clone(&…)`);
///   2. the Fn-typed coercion (`Rc`/`Arc`/`Box::new(…) as <fn-type>`, or just
///      ` as <fn-type>` when already wrapped);
///   3. the borrow wrapper (`&(…)` for a `Read` non-scalar non-Fn, `&mut (…)` for a
///      `Mutate`).
pub(crate) fn lower_call_arg_value(
    a: &crate::AST::CallArg,
    conv: Option<(AccessConvention, Type)>,
    env: &mut LowerEnv,
    cx: &Cx,
) -> TExpr {
    if a.flags.c_callback_symbol
        && !conv
            .as_ref()
            .is_some_and(|(_, ty)| callback_fn_type(ty).is_some())
    {
        return invariant_arg_value(a, "C callback argument has no function signature");
    }
    let saved_binder_refs = env.binder_refs.clone();
    if !a.flags.binder_refs.is_empty() {
        let Some(site) = a.flags.binder_site else {
            return invariant_arg_value(a, "call argument binder has no site");
        };
        for (name, slot, ty) in &a.flags.binder_refs {
            let temp = jet_format!("{jet_prefix}arg{site}_{slot}");
            env.binder_refs.insert(name.clone(), (temp, ty.clone()));
        }
    }
    let _arg_cache_scope = env
        .fallback_subject
        .then(super::expressions::ExprCacheScope::enter);
    let fallback_subject = env.fallback_subject;
    env.fallback_subject = false;
    // A bare lambda flowing into a user fn-typed parameter takes its param
    // types from that fn-type so codegen emits the Rust closure-param types
    // rustc needs (c142). Other args lower normally.
    let value = match (&a.expr, &conv) {
        (Expr::Ident(name, _), Some((AccessConvention::Move, ty))) if env.is_resource(name) => {
            TExpr {
                ty: ty.clone(),
                kind: TExprKind::ResourceTake(env.resource_take_place(name)),
            }
        }
        (Expr::Ident(name, _), Some((_, ty)))
            if a.flags.c_callback_symbol && callback_fn_type(ty).is_some() =>
        {
            TExpr {
                ty: ty.clone(),
                kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::FnName(
                    crate::Codegen::TIR::c_callback_adapter_name(name),
                ))),
            }
        }
        (Expr::Ident(name, _), Some((_, ty @ Type::Fn { .. })))
            if !env.locals.contains_key(name)
                && !cx.consts.contains_key(name)
                && cx
                    .fn_types
                    .get(name)
                    .is_some_and(|fn_ty| matches!(fn_ty, Type::Fn { .. })) =>
        {
            TExpr {
                ty: ty.clone(),
                kind: TExprKind::FnValue {
                    kind: crate::Codegen::TIR::TFnValueKind::NamedFn {
                        name: Some(name.clone()),
                        lambda: None,
                    },
                },
            }
        }
        (Expr::Lambda(lam), Some((_, ty)))
            if a.flags.c_callback_symbol && callback_fn_type(ty).is_some() =>
        {
            match callback_fn_type(ty) {
                Some(Type::Fn { params, ret, .. }) => {
                    let tl = lower_lambda_expecting(lam, cx, env, Some(params.as_slice()));
                    let name = mangle(&format!("c_callback_{}_{}", lam.span.start, lam.span.end));
                    TExpr {
                        ty: ty.clone(),
                        kind: TExprKind::HostCall(Box::new(
                            crate::Codegen::TIR::THostCall::CCallback {
                                symbol: name,
                                lambda: tl,
                                ret: ret.as_deref().cloned(),
                                managed: a.flags.c_callback_managed,
                                plan_digest: a.flags.c_callback_plan_digest.clone(),
                                callback_identity: a.flags.c_callback_identity.clone(),
                            },
                        )),
                    }
                }
                _ => invariant_arg_value(a, "C callback argument has a non-function signature"),
            }
        }
        (Expr::Lambda(lam), Some((_, ty @ Type::Fn { .. }))) => {
            let tl = lower_lambda_expecting_callable(lam, cx, env, ty);
            TExpr {
                ty: ty.clone(),
                kind: TExprKind::Lambda(Box::new(tl)),
            }
        }
        _ => lower_expr(&a.expr, cx, env),
    };
    env.fallback_subject = fallback_subject;
    env.binder_refs = saved_binder_refs;
    value
}

pub(crate) fn lower_one_call_arg(
    a: &crate::AST::CallArg,
    conv: Option<(AccessConvention, Type)>,
    env: &mut LowerEnv,
    cx: &Cx,
) -> TCallArg {
    if a.flags.c_callback_symbol
        && !conv
            .as_ref()
            .is_some_and(|(_, ty)| callback_fn_type(ty).is_some())
    {
        return invariant_call_arg(a, "C callback argument has no function signature");
    }
    let template_items = a.flags.template_items.as_deref().map(|items| {
        let _template_cache_scope = env
            .fallback_subject
            .then(super::expressions::ExprCacheScope::enter);
        let mut template_env = clone_env(env);
        template_env.fallback_subject = false;
        lower_template_items(
            items,
            a.span,
            cx,
            &mut template_env,
            &std::collections::HashSet::new(),
        )
    });
    let resource_move = matches!(
        (&a.expr, &conv),
        (Expr::Ident(name, _), Some((AccessConvention::Move, _))) if env.is_resource(name)
    );
    // Default expressions carry private references to earlier declaration
    // slots. A plain worklist pass cannot install that mapping, so never reuse
    // its cached value here; lower the argument through the binder-aware path.
    let value = if template_items.is_some() {
        TExpr {
            ty: Type::Named(crate::Syntax::INTERNAL_UNIT_TYPE.to_string()),
            kind: TExprKind::Unit,
        }
    } else {
        (if a.flags.binder_refs.is_empty() && !a.flags.c_callback_symbol {
            super::take_scheduled_expr(&a.expr, cx)
        } else {
            None
        })
        .unwrap_or_else(|| lower_call_arg_value(a, conv.clone(), env, cx))
    };
    // A worklist cache may have lowered a local identifier before an earlier
    // binding's refined type was installed in the sequential environment.
    // Refresh local reads from that environment before resolving boundary
    // conversions such as `[T#N]` to `[T]`.
    let value = match (&a.expr, value.kind) {
        (Expr::Ident(name, _), TExprKind::Local(local)) => {
            let Some(ty) = env.ty_of(name) else {
                return invariant_call_arg(a, "call argument local has no resolved type");
            };
            TExpr {
                ty,
                kind: TExprKind::Local(local),
            }
        }
        (_, kind) => TExpr { ty: value.ty, kind },
    };
    // Decide this before `preserve_typed_list_shape` retags the local read to
    // the callee's growable-list type. The source expression is still the
    // fixed array at this point.
    let widen_to_vec = matches!(
        (&value.ty, conv.as_ref().map(|(_, t)| t)),
        (Type::FixedList { elem: arg_elem, .. }, Some(Type::List(param_elem)))
            if fixed_list_elem_compatible(arg_elem, param_elem)
    );
    // D-SG9: call-site `[U8].{…}` / contextual list args need IntN suffixes.
    //
    // D-GENERIC-CALL1=A: a generic callee's `[T]` must NOT be stamped onto the
    // argument. The retag would overwrite the argument's real type with the
    // callee's binder, and every consumer of the lowered argument type then
    // reads a type variable instead of the actual: `call_return_type_with_args`
    // binds `T := T` and hands engines an unsubstituted `T?`, and
    // `specialize_generic_free_functions` records the same binder as the
    // monomorphisation shape, so no instantiation ever happens. The retag's
    // jobs (IntN suffixes, trait-object element injection, one-element typed
    // head collapse) all need a concrete element type, so nothing is lost.
    let value = match (&conv, value) {
        (Some((_, want @ Type::List(_))), v)
            if !crate::Generics::free_type_params(want).is_empty() =>
        {
            v
        }
        (Some((_, want @ (Type::List(_) | Type::FixedList { .. }))), v) => {
            super::preserve_typed_list_shape(v, want, cx)
        }
        (_, v) => v,
    };
    let web_noncopy_int = cx.web_wasm_noncopy_int
        && match &value.ty {
            Type::Int => true,
            Type::InlineRange { base, .. } => matches!(base.as_ref(), Type::Int),
            _ => false,
        };
    let clone = !resource_move
        && !is_opaque_handle_type(&value.ty, cx)
        && (web_noncopy_int
            || a.flags.implicit_clone
            || matches!(
                (&a.expr, conv.as_ref()),
                (Expr::Ident(name, _), None | Some((AccessConvention::Move, _)))
                    if env.is_borrowed(name)
                        && env.ty_of(name).is_some_and(|ty| !ty.is_scalar())
            ));
    let arc_clone = a.flags.shared_auto_clone;
    // The fn-value carrier is materialized exactly once at the call boundary.
    // Named values and escaping lambda TIR already emit the carrier; an
    // immediate lambda still needs coercion into the expected function slot.
    let fn_coerce = match &conv {
        Some((_, ty)) if a.flags.c_callback_symbol && callback_fn_type(ty).is_some() => None,
        Some((_, ty @ Type::Fn { .. })) => {
            let already_carrier = ast_arg_is_named_fn_value(&a.expr, cx, env)
                || matches!(
                    &a.expr,
                    Expr::Ident(name, _)
                        if env.ty_of(name).is_some_and(|t| matches!(t, Type::Fn { .. }))
                )
                || matches!(
                    &value.kind,
                    TExprKind::Lambda(lam) if lam.boxed || lam.rc || lam.arc
                );
            Some(TFnCoerce {
                ty: ty.clone(),
                already_boxed: already_carrier,
            })
        }
        _ => None,
    };
    // D-UNIONTYPE1=A: member → union inject at the call boundary.
    let widen_to_union = match (&value.ty, conv.as_ref().map(|(_, t)| t)) {
        (got, Some(want @ Type::Union(members))) if members.iter().any(|m| m == got) => {
            Some(want.clone())
        }
        _ => None,
    };
    // S48: a concrete value meeting a single-trait value slot is boxed at the
    // call boundary. Keep the target type as a typed TIR fact; MIR resolves it
    // to a MirTypeId and each backend consumes that row.
    let box_as_trait = match (&value.ty, conv.as_ref().map(|(_, t)| t)) {
        (Type::TraitObject(_), _) => None,
        (Type::Named(concrete) | Type::Apply { name: concrete, .. }, Some(want)) => {
            let trait_name = match want {
                Type::TraitObject(names) if names.len() == 1 => names.first(),
                Type::Named(name) if cx.trait_names.contains(name) => Some(name),
                _ => None,
            };
            trait_name
                .filter(|trait_name| *trait_name != concrete)
                .cloned()
                .map(|name| Type::TraitObject(vec![name]))
        }
        _ => None,
    };
    // Borrow wrappers (applied after the clone + fn-coerce wrappers). A `Read`
    // non-scalar is `&(…)`; a `Mutate` is `&mut (…)`.
    // When widening to Vec, the borrow wrapper applies to the widened Vec (not the array).
    let (borrow, mut_borrow) = match &conv {
        Some((AccessConvention::Read, t))
            if !t.is_scalar() && !(a.flags.c_callback_symbol && callback_fn_type(t).is_some()) =>
        {
            (true, false)
        }
        Some((AccessConvention::Write, _)) => (false, true),
        _ => (false, false),
    };
    TCallArg {
        value,
        template_items,
        borrow,
        mut_borrow,
        clone,
        arc_clone,
        fn_coerce,
        widen_to_vec,
        widen_to_union,
        box_as_trait,
    }
}

/// c109 Phase 14: lower a cross-module call's arguments against the callee's import
/// signature, reproducing `emit_call_args`. Each arg's borrow/clone/fn-coercion is
/// resolved from the sig param convention (the same `lower_one_call_arg` used by the
/// plain-call path).
pub(crate) fn lower_module_args(
    args: &[crate::AST::CallArg],
    sig: Option<&[(AccessConvention, Type)]>,
    env: &mut LowerEnv,
    cx: &Cx,
) -> Vec<TCallArg> {
    let Some(sig) = sig else {
        return args
            .iter()
            .map(|arg| invariant_call_arg(arg, "module call has no resolved signature"))
            .collect();
    };
    args.iter()
        .enumerate()
        .map(|(i, a)| {
            let Some((convention, ty)) = sig.get(i) else {
                return invariant_call_arg(
                    a,
                    format!("module call argument {} has no resolved parameter", i + 1),
                );
            };
            lower_one_call_arg(a, Some((*convention, ty.clone())), env, cx)
        })
        .collect()
}

/// c109 Phase 14: lower one FFI extern-call argument. The value is wrapped in
/// `(…).clone()` when the arg carries `implicit_clone`, OR when its param is a
/// non-scalar `Read`-convention type and `implicit_clone` is NOT already set (the AST
/// `if a.flags.implicit_clone { … } else if … } if let Some((_, ty)) = sig … if
/// !ty.is_scalar() && !implicit_clone`). The Arc (`shared_auto_clone`) form is excluded
/// from the subset, so it never reaches here.
pub(crate) fn lower_extern_call_arg(
    a: &crate::AST::CallArg,
    conv: Option<(AccessConvention, Type)>,
    env: &mut LowerEnv,
    cx: &Cx,
) -> TExternArg {
    let Some((convention, ty)) = conv.as_ref() else {
        return invariant_extern_arg(a, "extern call argument has no resolved parameter");
    };
    // D-CABI-CALLBACK1: the C bridge is still an extern call, but its callback
    // argument needs the same stable function item / lambda wrapper as a direct
    // C call. Preserve sema's fact instead of re-boxing it as `dyn Fn`.
    let c_callback = a.flags.c_callback_symbol && callback_fn_type(ty).is_some();
    if a.flags.c_callback_symbol && !c_callback {
        return invariant_extern_arg(a, "C callback argument has a non-function signature");
    }
    // Lower through the shared call-argument path so an owning resource move
    // becomes `ResourceTake` and invalidates the source slot before the native
    // close call. A plain `lower_expr` would read/clone the handle, then move
    // the clone and leave the original for the drop edge.
    let value = lower_call_arg_value(a, conv.clone(), env, cx);
    let mut_borrow = *convention == AccessConvention::Write;
    let non_scalar_param = !ty.is_scalar() && !c_callback;
    // `(…).clone()` is emitted once: either the explicit implicit_clone flag, or
    // the non-scalar-param clone (when implicit_clone is false). The two never stack —
    // the AST applies the param clone only `&& !a.flags.implicit_clone`.
    let clone = *convention == AccessConvention::Read
        && !is_opaque_handle_type(ty, cx)
        && (a.flags.implicit_clone || (non_scalar_param && !a.flags.implicit_clone));
    TExternArg {
        value,
        clone,
        mut_borrow,
    }
}

/// c109 Phase 13: does this AST arg expression emit as a `Box::new(…)` (a bare
/// fn-name value via `emit_named_fn_value`)? That is exactly an `Expr::Ident` which
/// is NOT a local and resolves to a `Type::Fn` in `cx.fn_types` (a top-level fn used
/// as a value). Mirrors `emit_expr`'s `Expr::Ident` arm + `emit_call_args`'
/// `s.starts_with("Box::new(")` check, resolved at lowering.
pub(crate) fn ast_arg_is_named_fn_value(e: &Expr, cx: &Cx, env: &LowerEnv) -> bool {
    if let Expr::Ident(name, _) = e {
        if !env.locals.contains_key(name) && !cx.consts.contains_key(name) {
            return matches!(cx.fn_types.get(name), Some(Type::Fn { .. }));
        }
    }
    false
}

/// The type a built-in method dispatches on, once compile-time-only tag
/// wrappers are erased. Shared by `tir_recv_jet_ty` and by `builtin_recv_ty`
/// (TIR/lower/builtins.rs), which closes this function's struct-`Field` hole for
/// the builtin table — one tag rule, read the same way from both.
///
/// D-TAINT1/D-TAG-SURFACE1: most `Type::Tagged` markers (the terminal
/// capability report's own D-PROCESS-SESSION2 tag, `#Input`/other user
/// taint facts, …) are compile-time-only dataflow facts with no runtime
/// representation — the same erasure `Expr::Tainted` gets in
/// `expr_in_subset` (TIR/subset/expressions.rs). Builtin-method
/// dispatch must see the real underlying type (String/List/Map/…)
/// or a tagged receiver's `.split()`/`.get()`/etc. misses the curated
/// `TBuiltinOp` fast path and falls back to a generic call the JIT
/// declines to native-compile.
///
/// The compiler-owned exceptions mirror sema's OWN rule at
/// `infer_method_call` (Sema/CheckerInfer/calls/method_calls.rs,
/// "Most fact tags are type-transparent"): SharedGuard read/edit and
/// the crypto nominal tag carry method POLICY, not just a dataflow
/// fact — `SharedGuard.wait()` dispatches through the tagged type's
/// own handle-method table, not a generic `TypeName::method` lookup.
/// Stripping those here misroutes the call and regresses an
/// already-JIT-covered stem (memory/shared_guard_queue).
pub(crate) fn builtin_dispatch_ty(ty: Type) -> Type {
    match ty {
        Type::Tagged { marker, inner }
            if matches!(
                marker,
                crate::AST::TagMarker::Internal(
                    crate::AST::InternalTag::SharedGuardRead
                        | crate::AST::InternalTag::SharedGuardEdit
                        | crate::AST::InternalTag::CoreCryptoNominal
                )
            ) =>
        {
            Type::Tagged { marker, inner }
        }
        Type::Tagged { inner, .. } => builtin_dispatch_ty(*inner),
        other => other,
    }
}

/// c109 Phase 9: the receiver type of a built-in method call, read off the TIR
/// lowering env's slot types. Only `Ident` (via its slot type), `Str`/`Char`, and
/// transparent `Paren`/`Copy` wrappers around those or chained method calls
/// resolve; everything else (notably a struct `Field` read) is `None`.
///
/// That partiality is load-bearing for the callers that read a `None` as a fact —
/// the for-in `lines` split (a `child.stdout` receiver is recognized by its BASE),
/// the mutable-place hint, the view-owner peeks. It is NOT load-bearing for the
/// builtin TABLE, which is receiver-TYPED: there a `None` silently means "take the
/// List surface", which is how a `String` field's `.replace(a, b)` reached
/// `jet_list_replace` (I2). `builtin_recv_ty` (TIR/lower/builtins.rs) closes that
/// one hole for the dispatch; keep this function as it is.
pub(crate) fn tir_recv_jet_ty(e: &Expr, env: &LowerEnv) -> Option<Type> {
    fn literal_ty(expr: &Expr) -> Option<Type> {
        match expr {
            Expr::Int(..) => Some(Type::Int),
            Expr::Float(_, _, _, _) => Some(Type::Float),
            Expr::Bool(_, _) => Some(Type::Bool),
            Expr::Char(_, _) => Some(Type::Char),
            Expr::Str(_, _) => Some(Type::String),
            _ => None,
        }
    }

    fn set_constructor_elem(expr: &Expr, env: &LowerEnv) -> Option<Type> {
        match expr {
            Expr::ListLit(items, _) => items.first().and_then(literal_ty),
            Expr::Ident(name, _) => match env.ty_of(name) {
                Some(Type::List(elem)) | Some(Type::FixedList { elem, .. }) => Some(*elem),
                _ => None,
            },
            _ => None,
        }
    }

    match e {
        Expr::Paren(inner, _) | Expr::Copy(inner, _) => tir_recv_jet_ty(inner, env),
        Expr::Binary(crate::AST::BinOp::Compare, _, _, _) => {
            Some(Type::Named(crate::Syntax::TYPE_ORDERING.to_string()))
        }
        Expr::Ident(name, _) => env.ty_of(name).map(builtin_dispatch_ty),
        Expr::Str(_, _) => Some(Type::String),
        Expr::Char(_, _) => Some(Type::Char),
        Expr::TupleLit(_, _, Some(ty)) => Some(builtin_dispatch_ty(ty.clone())),
        Expr::Try(inner, ..) => tir_recv_jet_ty(inner, env).map(|ty| match ty {
            Type::Result { ok, .. } => builtin_dispatch_ty(*ok),
            Type::Option(inner) => builtin_dispatch_ty(*inner),
            other => other,
        }),
        Expr::MethodCall {
            receiver,
            method,
            args,
            resolved_ret,
            ..
        } => {
            // `resolved_ret` exists only when sema persisted a result more exact
            // than the generic method table (or another required exact shape).
            if let Some(ty) = resolved_ret {
                return Some(builtin_dispatch_ty(ty.clone()));
            }
            if let Expr::Ident(name, _) = receiver.as_ref() {
                if !env.locals.contains_key(name)
                    && method == "from"
                    && matches!(
                        name.as_str(),
                        crate::Syntax::TYPE_SET | crate::Syntax::TYPE_RANK
                    )
                    && args.len() == 1
                {
                    let Some(elem) = set_constructor_elem(&args[0].expr, env) else {
                        return None;
                    };
                    return Some(Type::Apply {
                        name: name.clone(),
                        args: vec![elem],
                    });
                }
                // Bytes static constructors used as chain receivers
                // (`Bytes.from([…]).to_lower()`).
                if !env.locals.contains_key(name)
                    && name == crate::Syntax::TYPE_BYTES
                    && matches!(
                        (method.as_str(), args.len()),
                        ("new", 0) | ("from", 1) | ("with_capacity", 1)
                    )
                {
                    return Some(Type::Named(crate::Syntax::TYPE_BYTES.to_string()));
                }
            }
            // D-ITERTOOLS1=A: chained adapters (`nums.take(3).to_list()`) must
            // resolve as `Iter`, not fall through to the list receiver — otherwise
            // `to_list` lowers as SetToList and rustc sees `.iter()` on JetIter.
            if let Some(recv_ty) = tir_recv_jet_ty(receiver, env) {
                if let Some(Some(ret)) =
                    crate::Collections::builtin_method_return(&recv_ty, method, args.len(), false)
                {
                    return Some(builtin_dispatch_ty(ret));
                }
            }
            if method == "chars" {
                return Some(Type::List(Box::new(Type::Char)));
            }
            // D-DYNARRAY1: `xs.view(a..b).fold(...)` chained with no intermediate
            // binding — resolve the constructed `View<T>`'s element type from the
            // list receiver so the chained call still dispatches correctly.
            if method == crate::Syntax::METHOD_VIEW {
                if let Some(list_ty) = tir_recv_jet_ty(receiver, env) {
                    let elem = match list_ty {
                        Type::List(e) => Some(*e),
                        Type::FixedList { elem, .. } => Some(*elem),
                        _ => None,
                    };
                    if let Some(elem) = elem {
                        return Some(Type::Apply {
                            name: "View".to_string(),
                            args: vec![elem],
                        });
                    }
                }
                return None;
            }
            tir_recv_jet_ty(receiver, env)
        }
        _ => None,
    }
}
