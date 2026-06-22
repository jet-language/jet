use super::*;
use crate::AST::{
    AccessConvention, EnumLitArg, Expr, IndexKind, Lambda, LambdaBody, StrPart, TryConvert, Type,
    UnOp, VariantPayload,
};
use crate::Diagnostics::span_line_col;
use crate::Generics;
use crate::Syntax;
use std::collections::HashMap;
pub(crate) fn emit_expr_stmt(cx: &Cx, e: &Expr, env: &HashMap<String, Slot>) -> String {
    emit_expr(cx, e, env)
}

fn user_type_apply_rust(cx: &Cx, name: &str, type_args: &[Type]) -> String {
    if type_args.is_empty() {
        format!("user_{name}")
    } else {
        format!(
            "user_{name}::<{}>",
            type_args
                .iter()
                .map(|a| cx.rust_type(a))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

/// S68 (D-SG2): emit a value block `{ stmts…; value }` for an `if`-expression
/// branch, mirroring how lambda block bodies are lowered.
fn emit_value_block(
    cx: &Cx,
    stmts: &[crate::AST::Stmt],
    value: &Expr,
    env: &HashMap<String, Slot>,
) -> String {
    let mut local = env.clone();
    let mut inner = String::new();
    emit_stmts(cx, stmts, &mut local, &mut inner, 1, false);
    let v = emit_expr(cx, value, &local);
    format!("{{ {} {} }}", inner, v)
}

pub(crate) fn emit_expr(cx: &Cx, e: &Expr, env: &HashMap<String, Slot>) -> String {
    match e {
        // S68 (D-SG2): `if` as a value lowers straight to a Rust if-expression.
        Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            let c = emit_expr(cx, cond, env);
            let then_block = emit_value_block(cx, then_body, then_value, env);
            let else_block = emit_value_block(cx, else_body, else_value, env);
            format!("if {} {} else {}", c, then_block, else_block)
        }
        // S58 (E2-M13): `mem.Ptr<T>.from_addr(addr)` builds a raw pointer from a
        // machine address. The cast itself is safe in Rust (only *using* the
        // pointer needs `unsafe`, which the surrounding `@unsafe` provides).
        Expr::PtrFromAddr { elem, addr, .. } => {
            let cx_ty = cx.rust_type(elem);
            format!("(({}) as usize as *mut {})", emit_expr(cx, addr, env), cx_ty)
        }
        // D-SG9: emit the suffix for the literal's elaborated width (`200u8`,
        // `100000i32`); a default `Int` literal stays `i64`. Sema range-checked
        // the value, so the suffix is always exact.
        Expr::Int(n, _, width) => match width {
            Some((signed, bits)) => format!("{}{}{}", n, if *signed { 'i' } else { 'u' }, bits),
            None => format!("{}i64", n),
        },
        // Float literals carry no suffix: Rust defaults floats to `f64` (Jet's
        // `Float`), and an `F32` context coerces the bare literal to `f32`.
        Expr::Float(v, _) => format!("{:?}", v),
        Expr::Bool(b, _) => b.to_string(),
        Expr::Char(ch, _) => format!("{:?}", ch),
        Expr::ListLit(elems, _) => {
            let parts = elems
                .iter()
                .map(|e| emit_expr(cx, e, env))
                .collect::<Vec<_>>()
                .join(", ");
            format!("vec![{}]", parts)
        }
        Expr::TupleLit(fields, _, ty) => {
            let canonical = ty
                .as_ref()
                .and_then(|t| match t {
                    Type::Tuple(fs) => Some(tuple_fields_plain(fs)),
                    _ => None,
                })
                .unwrap_or_default();
            let struct_name = tuple_struct_name(&canonical);
            let field_map: HashMap<String, &Expr> =
                fields.iter().map(|(n, e)| (n.clone(), e)).collect();
            let parts = canonical
                .iter()
                .map(|(n, _)| {
                    let val = field_map
                        .get(n)
                        .map(|e| emit_expr(cx, e, env))
                        .unwrap_or_else(|| "0i64".to_string());
                    format!("{}: {}", mangle(n), val)
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{} {{ {} }}", struct_name, parts)
        }
        Expr::MapLit(entries, _) => {
            if entries.is_empty() {
                "std::collections::BTreeMap::new()".to_string()
            } else {
                let mut s = String::from("{ let mut _m = std::collections::BTreeMap::new(); ");
                for (k, v) in entries {
                    s.push_str(&format!(
                        "_m.insert(({}).clone(), {}); ",
                        emit_expr(cx, k, env),
                        emit_expr(cx, v, env)
                    ));
                }
                s.push_str("_m }");
                s
            }
        }
        Expr::Index {
            base,
            index,
            span,
            kind,
        } => {
            let (line, _) = span_line_col(&cx.src, span.start);
            let b = emit_expr(cx, base, env);
            let i = emit_expr(cx, index, env);
            match kind {
                IndexKind::Map => {
                    format!("jet_index_map(&({}), &({}), {:?}, {})", b, i, cx.file, line)
                }
                _ => format!("jet_index_vec(&({}), {}, {:?}, {})", b, i, cx.file, line),
            }
        }
        Expr::Slice {
            base,
            start,
            end,
            span,
        } => {
            let (line, _) = span_line_col(&cx.src, span.start);
            let b = emit_expr(cx, base, env);
            let a = emit_expr(cx, start, env);
            let e = emit_expr(cx, end, env);
            format!(
                "jet_slice_vec(&({}), {}, {}, {:?}, {})",
                b, a, e, cx.file, line
            )
        }
        Expr::Str(parts, _) => emit_str(cx, parts, env),
        Expr::Ident(name, _) => {
            if let Some(c) = cx.consts.get(name) {
                return c.clone();
            }
            if env.get(name).is_none() {
                if let Some(ft) = cx.fn_types.get(name) {
                    return emit_named_fn_value(cx, name, ft);
                }
            }
            place_of(env, name)
        }
        Expr::Unary(op, inner, _) => {
            let i = emit_expr(cx, inner, env);
            match op {
                UnOp::Neg => format!("(-({}))", i),
                UnOp::Not => format!("(!({}))", i),
            }
        }
        Expr::Binary(op, l, r, span) => {
            let ls = emit_expr(cx, l, env);
            let rs = emit_expr(cx, r, env);
            // D-NUMOPS1: plain integer `+`/`-`/`*`/`/` traps on overflow. Floats and
            // `#Numeric` distinct types keep the native Rust operator (they have no
            // `JetArith` impl). When neither operand's type resolves, fall back to
            // the native operator — always valid Rust, just not trap-checked.
            use crate::AST::BinOp;
            let trap_method = match op {
                BinOp::Add => Some("jet_add"),
                BinOp::Sub => Some("jet_sub"),
                BinOp::Mul => Some("jet_mul"),
                BinOp::Div => Some("jet_div"),
                _ => None,
            };
            let is_int_arith = trap_method.is_some()
                && (operand_is_integer(cx, l, env) == Some(true)
                    || operand_is_integer(cx, r, env) == Some(true));
            if is_int_arith {
                let (line, _) = span_line_col(&cx.src, span.start);
                format!(
                    "({}).{}(({}), {:?}, {})",
                    ls,
                    trap_method.unwrap(),
                    rs,
                    cx.file,
                    line
                )
            } else {
                format!("(({}) {} ({}))", ls, op.spell(), rs)
            }
        }
        Expr::Call(call) => emit_call(cx, call, env),
        Expr::Deref(inner, _) => format!("*{}", emit_expr(cx, inner, env)),
        Expr::Field(inner, member, _) => {
            // Fields of a core struct (e.g. `ProcessResult.code`) are emitted by
            // their plain Rust name, never `user_<name>` — the core structs in
            // Source/Prelude/Core.rs declare unprefixed fields (B2).
            let field = core_struct_field_rust_name(cx, inner, member, env)
                .unwrap_or_else(|| mangle(member));
            if member == "clone" {
                format!("({}).clone()", emit_expr(cx, inner, env))
            } else if let Expr::Ident(alias, _) = &**inner {
                if let Some(module) = cx.core_imports.get(alias) {
                    emit_core_field(module, member)
                } else if is_json_type_name(alias) && member == "Null" {
                    format!("{}jet_std::Json::Null", cx.root_prefix)
                } else if let Some(c) = crate::AST::numeric_type_from_name(alias)
                    .filter(|_| {
                        matches!(member.as_str(), "MIN" | "MAX" | "INFINITY" | "NEG_INFINITY" | "NAN" | "EPSILON")
                    })
                    .map(|nt| format!("{}::{}", cx.rust_type(&nt), member))
                {
                    // D-NUMOPS1: numeric type constants (`u8::MAX`, `f64::INFINITY`).
                    c
                } else if cx.enum_variants.contains_key(alias) {
                    // Qualify with the foreign module path when the enum type
                    // comes from an imported file-module.
                    if let Some(rust_mod) = cx.foreign_types.get(alias.as_str()) {
                        format!(
                            "{}{}::user_{}::{}",
                            cx.root_prefix,
                            rust_mod,
                            alias,
                            mangle(member)
                        )
                    } else {
                        format!("user_{}::{}", alias, mangle(member))
                    }
                } else {
                    format!("({}).{}", emit_expr(cx, inner, env), field)
                }
            } else {
                format!("({}).{}", emit_expr(cx, inner, env), field)
            }
        }
        Expr::OptField {
            base,
            member,
            flatten,
            ..
        } => {
            // S71 (D-SG6): `base?.field` maps over the optional, flattening when
            // the field is itself optional. `__optv` owns the unwrapped value.
            let combinator = if *flatten { "and_then" } else { "map" };
            format!(
                "({}).clone().{}(|__optv| __optv.{})",
                emit_expr(cx, base, env),
                combinator,
                mangle(member)
            )
        }
        Expr::MethodCall {
            receiver,
            method,
            method_span,
            args,
            recv_type,
        } => emit_method_call(cx, receiver, method, *method_span, args, recv_type.as_deref(), env),
        Expr::StructLit {
            type_name,
            type_args,
            import_ns,
            as_trait,
            fields,
            ..
        } => {
            let mut parts = Vec::new();
            if let Some(ns) = import_ns {
                let mod_name = cx
                    .import_mods
                    .get(ns)
                    .map(|s| s.as_str())
                    .unwrap_or("user_unknown");
                let rust_type = if type_args.is_empty() {
                    format!("{}{}::{}", cx.root_prefix, mod_name, mangle(type_name))
                } else {
                    format!(
                        "{}{}::{}::<{}>",
                        cx.root_prefix,
                        mod_name,
                        mangle(type_name),
                        type_args
                            .iter()
                            .map(|a| cx.rust_type(a))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                for (n, _, e) in fields {
                    parts.push(format!("{}: {}", mangle(n), emit_expr(cx, e, env)));
                }
                let lit = format!("{} {{ {} }}", rust_type, parts.join(", "));
                if let Some(trait_name) = as_trait {
                    return format!(
                        "Box::new({lit}) as Box<dyn {}>",
                        Generics::user_trait_rust(trait_name)
                    );
                }
                return lit;
            }
            // E2-M10: compiler-known struct types (HttpResponse, HttpRequest) use
            // prelude types, not user_* prefixed names. Their fields are also plain
            // (not mangled) since they're defined in the prelude, not user code.
            let is_prelude_struct = net_handle_rust_type(type_name).is_some();
            let rust_type = if is_prelude_struct {
                format!("{}{}", cx.root_prefix, net_handle_rust_type(type_name).unwrap())
            } else {
                user_type_apply_rust(cx, type_name, type_args)
            };
            for (n, _, e) in fields {
                let field_name = if is_prelude_struct {
                    n.clone() // prelude struct fields are not mangled
                } else {
                    mangle(n)
                };
                parts.push(format!("{}: {}", field_name, emit_expr(cx, e, env)));
            }
            // D-ROUTE1=A: HttpRequest has a `params` field (set by dispatch) that
            // users never provide; always initialize it to an empty BTreeMap.
            if type_name == "HttpRequest" && is_prelude_struct {
                parts.push("params: std::collections::BTreeMap::new()".to_string());
            }
            let lit = format!("{} {{ {} }}", rust_type, parts.join(", "));
            if let Some(trait_name) = as_trait {
                format!(
                    "Box::new({lit}) as Box<dyn {}>",
                    Generics::user_trait_rust(trait_name)
                )
            } else {
                lit
            }
        }
        Expr::EnumLit {
            type_name,
            variant,
            args,
            ..
        } => emit_enum_lit(cx, type_name, variant, args, env),
        Expr::Present(inner, _) => format!("Some({})", emit_expr(cx, inner, env)),
        Expr::Absent(_) => "None".to_string(),
        Expr::Todo { span, expected_type } => {
            // D-TOOL2 (E2-M11; D-CASING1 follow-on spelling): emit a runtime panic
            // with file, line, and expected type. `todo!()` is diverging in Rust so
            // it type-checks anywhere (I1: no unsafe generated).
            let ty = expected_type.as_deref().unwrap_or("(unknown)");
            let (line, _) = span_line_col(&cx.src, span.start);
            format!(
                "todo!(\"#{} at {}:{} — expected {}\")",
                crate::Syntax::KW_TODO,
                cx.file,
                line,
                ty
            )
        }
        Expr::Ok(inner, _) => format!("Ok({})", emit_expr(cx, inner, env)),
        Expr::Err(inner, _) => format!("Err({})", emit_expr(cx, inner, env)),
        Expr::Try(inner, span, convert) => {
            // E3002 (E2-M12): wrap each `?` so a propagating Err prints one trace
            // frame in debug builds. jet_trace_err returns the Result unchanged.
            let (line, _col) = span_line_col(&cx.src, span.start);
            let file = escape_rust_str(&cx.file);
            let fn_name = cx.current_fn.borrow().clone();
            let fn_name = escape_rust_str(&fn_name);
            match convert {
                TryConvert::Fallible => {
                    // S80/D-LIB3: error type implements Fallible; convert via .to_error()
                    format!(
                        "jet_trace_err({}.map_err(|e| e.to_error()), {}, {}, {})?",
                        emit_expr(cx, inner, env),
                        file,
                        line,
                        fn_name
                    )
                }
                TryConvert::Typed(conv_fn) => {
                    // D-ERR-CONV: apply the declared `impl Source -> Target` conversion.
                    format!(
                        "jet_trace_err({}.map_err({}), {}, {}, {})?",
                        emit_expr(cx, inner, env),
                        conv_fn,
                        file,
                        line,
                        fn_name
                    )
                }
                TryConvert::None => {
                    format!(
                        "jet_trace_err({}, {}, {}, {})?",
                        emit_expr(cx, inner, env),
                        file,
                        line,
                        fn_name
                    )
                }
            }
        }
        Expr::OrFallback {
            value,
            fallback,
            is_option,
            ..
        } => emit_or_fallback(cx, value, fallback, *is_option, env),
        Expr::PatternTest {
            subject, pattern, ..
        } => {
            let subj = emit_expr(cx, subject, env);
            emit_pattern_matches(cx, &subj, pattern)
        }
        Expr::Lambda(lam) => emit_lambda(cx, lam, env),
        Expr::CallValue { callee, args, .. } => {
            let f = emit_expr(cx, callee, env);
            let arg_str = emit_call_args(cx, None, args, env);
            format!("({})({})", f, arg_str)
        }
        // S75/S76 codegen: fan-out lowers to a Vec built by calling the callee on each item.
        // Sema erased [T#N] to the same Vec representation so this is straightforward.
        Expr::FanOut { callee, items, .. } => {
            let elems: Vec<String> = if let Expr::Ident(name, name_span) = callee.as_ref() {
                // Route through emit_call so builtins (print, panic, …) are handled correctly.
                items
                    .iter()
                    .map(|item| {
                        let call = crate::AST::Call {
                            name: name.clone(),
                            name_span: *name_span,
                            args: vec![crate::AST::CallArg {
                                convention: AccessConvention::Read,
                                expr: item.clone(),
                                span: item.span(),
                                flags: Default::default(),
                                label: None,
                            }],
                        };
                        emit_call(cx, &call, env)
                    })
                    .collect()
            } else {
                let f = emit_expr(cx, callee, env);
                items
                    .iter()
                    .map(|item| {
                        let arg = emit_expr(cx, item, env);
                        format!("({})({})", f, arg)
                    })
                    .collect()
            };
            format!("vec![{}]", elems.join(", "))
        }
    }
}

fn emit_lambda(cx: &Cx, lam: &Lambda, env: &HashMap<String, Slot>) -> String {
    let mut prep = String::new();
    let mut lam_env = env.clone();
    for name in &lam.meta.cloned_captures {
        let cap = format!("_jet_cap_{}", mangle(name));
        prep.push_str(&format!(
            "let {} = ({}).clone();\n    ",
            cap,
            place_of(env, name)
        ));
        lam_env.insert(
            name.clone(),
            Slot {
                rust_name: cap,
                deref: false,
                jet_ty: None,
            },
        );
    }
    for p in &lam.params {
        lam_env.insert(
            p.name.clone(),
            Slot {
                rust_name: mangle(&p.name),
                deref: false,
                jet_ty: p.ty.clone(),
            },
        );
    }
    let params: Vec<String> = lam
        .params
        .iter()
        .map(|p| {
            let ty =
                p.ty.as_ref()
                    .map(|t| format!(": {}", cx.rust_type(t)))
                    .unwrap_or_default();
            format!("{}{}", mangle(&p.name), ty)
        })
        .collect();
    let body = match &lam.body {
        LambdaBody::Expr(e) => emit_expr(cx, e, &lam_env),
        LambdaBody::Block(stmts) => {
            let mut inner = String::new();
            emit_stmts(cx, stmts, &mut lam_env, &mut inner, 1, false);
            format!("{{ {} }}", inner)
        }
    };
    let move_kw = if lam.meta.needs_fn_mut && !lam.meta.escapes {
        ""
    } else {
        "move "
    };
    let closure = format!("{}|{}| {}", move_kw, params.join(", "), body);
    let wrapped = if lam.meta.escapes {
        format!("Box::new({})", closure)
    } else {
        closure
    };
    if prep.is_empty() {
        wrapped
    } else {
        format!("{{ {} {} }}", prep, wrapped)
    }
}

fn emit_spawn_lambda(cx: &Cx, lam: &Lambda, env: &HashMap<String, Slot>) -> String {
    let mut prep = String::new();
    let mut lam_env = env.clone();
    for name in &lam.meta.cloned_captures {
        let cap = format!("_jet_cap_{}", mangle(name));
        prep.push_str(&format!(
            "let {} = ({}).clone();\n    ",
            cap,
            place_of(env, name)
        ));
        lam_env.insert(
            name.clone(),
            Slot {
                rust_name: cap,
                deref: false,
                jet_ty: None,
            },
        );
    }
    for p in &lam.params {
        lam_env.insert(
            p.name.clone(),
            Slot {
                rust_name: mangle(&p.name),
                deref: false,
                jet_ty: p.ty.clone(),
            },
        );
    }
    let params: Vec<String> = lam
        .params
        .iter()
        .map(|p| {
            let ty =
                p.ty.as_ref()
                    .map(|t| format!(": {}", cx.rust_type(t)))
                    .unwrap_or_default();
            format!("{}{}", mangle(&p.name), ty)
        })
        .collect();
    let body = match &lam.body {
        LambdaBody::Expr(e) => emit_expr(cx, e, &lam_env),
        LambdaBody::Block(stmts) => {
            let mut inner = String::new();
            emit_stmts(cx, stmts, &mut lam_env, &mut inner, 1, false);
            format!("{{ {} }}", inner)
        }
    };
    let closure = format!("move |{}| {}", params.join(", "), body);
    if prep.is_empty() {
        closure
    } else {
        format!("{{ {} {} }}", prep, closure)
    }
}

fn emit_boxed_enum_arg(
    cx: &Cx,
    type_name: &str,
    edge: &str,
    expr: &Expr,
    env: &HashMap<String, Slot>,
) -> String {
    let payload_ty = enum_variant_payload_type(cx, type_name, edge);
    let mut s = emit_expr(cx, expr, env);
    if payload_ty.is_some_and(|t| !t.is_scalar()) && expr_borrowed_in_env(expr, env) {
        s = format!("({}).clone()", s);
    }
    if cx
        .boxed_edges
        .contains(&(type_name.to_string(), edge.to_string()))
    {
        format!("Box::new({})", s)
    } else {
        s
    }
}

fn enum_variant_payload_type<'a>(cx: &'a Cx, type_name: &str, variant: &str) -> Option<&'a Type> {
    let variants = cx.enum_variants.get(type_name)?;
    let (_, payload) = variants.iter().find(|(v, _)| v == variant)?;
    match payload {
        VariantPayload::Single(t, _) => Some(t),
        VariantPayload::Named(fs) if fs.len() == 1 => Some(&fs[0].ty),
        _ => None,
    }
}

fn expr_borrowed_in_env(expr: &Expr, env: &HashMap<String, Slot>) -> bool {
    match expr {
        Expr::Ident(name, _) => env.get(name).is_some_and(|s| s.deref),
        _ => false,
    }
}

fn emit_enum_lit(
    cx: &Cx,
    type_name: &str,
    variant: &str,
    args: &[EnumLitArg],
    env: &HashMap<String, Slot>,
) -> String {
    let type_prefix = if let Some(rust_mod) = cx.foreign_types.get(type_name) {
        format!("{}{}::user_{}", cx.root_prefix, rust_mod, type_name)
    } else {
        format!("user_{}", type_name)
    };
    let prefix = format!("{}::{}", type_prefix, mangle(variant));
    if args.is_empty() {
        return prefix;
    }
    if args.iter().all(|a| matches!(a, EnumLitArg::Positional(_))) {
        let pos = args
            .iter()
            .map(|a| match a {
                EnumLitArg::Positional(e) => emit_boxed_enum_arg(cx, type_name, variant, e, env),
                // Dead by the enclosing `.all(... Positional)` guard, not a sema
                // fact — every arg in this branch is positional.
                _ => unreachable!("all args are positional in this branch"),
            })
            .collect::<Vec<_>>()
            .join(", ");
        return format!("{}({})", prefix, pos);
    }
    let fields = args
        .iter()
        .map(|a| match a {
            EnumLitArg::Named { label, expr } => {
                let edge = format!("{}.{}", variant, label);
                format!(
                    "{}: {}",
                    mangle(label),
                    emit_boxed_enum_arg(cx, type_name, &edge, expr, env)
                )
            }
            EnumLitArg::Positional(e) => emit_expr(cx, e, env),
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{} {{ {} }}", prefix, fields)
}

/// When `inner` has a known std-struct type and `member` is one of its fields,
/// returns the field's plain Rust name (std structs use unprefixed fields, so
/// emitting `user_<member>` would not compile — B2). Returns `None` otherwise,
/// so user structs keep their mangled field names.
fn core_struct_field_rust_name(
    cx: &Cx,
    inner: &Expr,
    member: &str,
    env: &HashMap<String, Slot>,
) -> Option<String> {
    let _ = cx;
    let Type::Named(type_name) = expr_jet_ty(inner, env)? else {
        return None;
    };
    let known = match type_name.as_str() {
        "ProcessResult" => matches!(member, "code" | "output" | "errors"),
        _ if is_json_error_type_name(&type_name) => matches!(member, "line" | "message"),
        _ if is_utf8_error_type_name(&type_name) => member == "message",
        // E2-M10: HttpRequest and HttpResponse field access.
        "HttpRequest" | "HttpResponse" => matches!(member, "method" | "path" | "body" | "headers" | "status"),
        _ => false,
    };
    if known {
        Some(member.to_string())
    } else {
        None
    }
}

fn is_json_error_type_name(name: &str) -> bool {
    name == Syntax::TYPE_JSON_ERROR || name == "JsonError"
}

fn is_utf8_error_type_name(name: &str) -> bool {
    name == Syntax::TYPE_UTF8_ERROR || name == "Utf8Error"
}

pub(crate) fn expr_jet_ty(expr: &Expr, env: &HashMap<String, Slot>) -> Option<Type> {
    match expr {
        Expr::Ident(name, _) => env.get(name).and_then(|s| s.jet_ty.clone()),
        Expr::TupleLit(_, _, Some(ty)) => Some(ty.clone()),
        Expr::Str(_, _) => Some(Type::String),
        Expr::Char(_, _) => Some(Type::Char),
        Expr::MethodCall {
            receiver, method, ..
        } => {
            if method == "chars" {
                return Some(Type::List(Box::new(Type::Char)));
            }
            if method == "split" {
                return Some(Type::List(Box::new(Type::String)));
            }
            expr_jet_ty(receiver, env)
        }
        _ => None,
    }
}

/// D-NUMOPS1: `Some(true)` if `e` is provably a plain integer (`Int`/`IntN`),
/// `Some(false)` if it's some other resolved type (float, distinct, …), `None`
/// if its type can't be determined. Drives overflow-trap codegen on arithmetic;
/// literals, negation, and nested arithmetic resolve without env help.
fn operand_is_integer(cx: &Cx, e: &Expr, env: &HashMap<String, Slot>) -> Option<bool> {
    match e {
        Expr::Int(..) => Some(true),
        Expr::Float(..) => Some(false),
        Expr::Unary(crate::AST::UnOp::Neg, inner, _) => operand_is_integer(cx, inner, env),
        Expr::Binary(_, l, _, _) => operand_is_integer(cx, l, env),
        _ => expr_jet_ty_with_cx(cx, e, env).map(|t| t.is_integer()),
    }
}

/// Like `expr_jet_ty` but also resolves core module call return types.
/// Used to determine the subject type for pattern match switches so that
/// `Ok(binding)` / `Err(binding)` patterns carry the correct inner type.
pub(crate) fn expr_jet_ty_with_cx(cx: &Cx, expr: &Expr, env: &HashMap<String, Slot>) -> Option<Type> {
    // First try the base resolver.
    if let Some(ty) = expr_jet_ty(expr, env) {
        return Some(ty);
    }
    // Handle core module method calls: `alias.method(...)` where `alias` is a
    // core import.  Only the ones that return a `Result` matter here.
    if let Expr::MethodCall { receiver, method, .. } = expr {
        if let Expr::Ident(alias, _) = receiver.as_ref() {
            if let Some(module) = cx.core_imports.get(alias) {
                let json_ty = || Type::Named(Syntax::TYPE_JSON.to_string());
                let json_err_ty = || Type::Named(Syntax::TYPE_JSON_ERROR.to_string());
                let io_err_ty = || Type::Named(Syntax::TYPE_IO_ERROR.to_string());
                let str_ty = || Type::String;
                let unit_ty = || Type::Named("Unit".to_string());
                let list_str = || Type::List(Box::new(Type::String));
                let list_u8 = || Type::List(Box::new(Type::IntN { signed: false, bits: 8 }));
                let result = |ok: Type, err: Type| Type::Result { ok: Box::new(ok), err: Box::new(err) };
                let list_list_str = || Type::List(Box::new(Type::List(Box::new(Type::String))));
                let map_str_str = || Type::Map { key: Box::new(Type::String), value: Box::new(Type::String) };
                let ty: Option<Type> = match (module.as_str(), method.as_str()) {
                    ("core.json", "parse") | ("jet.json", "parse") | ("jet.json", "decode") => Some(result(json_ty(), json_err_ty())),
                    ("core.fs", "read") => Some(result(str_ty(), io_err_ty())),
                    ("core.fs", "read_bytes") => Some(result(list_u8(), io_err_ty())),
                    ("core.fs", "write" | "append" | "remove" | "create_dir" | "copy" | "rename") => {
                        Some(result(unit_ty(), io_err_ty()))
                    }
                    ("core.fs", "list_dir") => Some(result(list_str(), io_err_ty())),
                    ("core.io", "read_all_input") => Some(result(str_ty(), io_err_ty())),
                    ("core.env", "current_dir") => Some(result(str_ty(), io_err_ty())),
                    // E2-M9: ring module result types.
                    ("jet.csv", "parse") => Some(result(list_list_str(), str_ty())),
                    ("jet.toml", "parse") | ("jet.yaml", "parse") => Some(result(map_str_str(), str_ty())),
                    // D-REGEX1: regex result types (for `?` propagation + chaining).
                    ("jet.regex", "is_match") => Some(result(Type::Bool, str_ty())),
                    ("jet.regex", "match") => Some(result(
                        Type::Option(Box::new(Type::Named("Match".to_string()))),
                        str_ty(),
                    )),
                    ("jet.regex", "find") => Some(result(Type::Option(Box::new(str_ty())), str_ty())),
                    ("jet.regex", "find_all") | ("jet.regex", "split") => {
                        Some(result(list_str(), str_ty()))
                    }
                    ("jet.regex", "replace") | ("jet.regex", "replace_all") => {
                        Some(result(str_ty(), str_ty()))
                    }
                    _ => None,
                };
                if ty.is_some() {
                    return ty;
                }
            }
        }
    }
    None
}

fn list_carries_trait(cx: &Cx, inner: &Type) -> bool {
    matches!(inner, Type::TraitObject(_))
        || matches!(inner, Type::Named(n) if cx.trait_names.contains(n))
}

/// D-NUMOPS1: the Rust type, spelling, and integer `(signed, bits)` (or `None`
/// for a float) that a `to_*` width-conversion method targets.
fn conv_rust_target(method: &str) -> Option<(&'static str, &'static str, Option<(bool, u8)>)> {
    Some(match method {
        "to_i8" => ("i8", "I8", Some((true, 8))),
        "to_i16" => ("i16", "I16", Some((true, 16))),
        "to_i32" => ("i32", "I32", Some((true, 32))),
        "to_i64" | "to_int" => ("i64", "Int", Some((true, 64))),
        "to_u8" => ("u8", "U8", Some((false, 8))),
        "to_u16" => ("u16", "U16", Some((false, 16))),
        "to_u32" => ("u32", "U32", Some((false, 32))),
        "to_u64" => ("u64", "U64", Some((false, 64))),
        "to_f32" => ("f32", "F32", None),
        "to_f64" | "to_float" => ("f64", "Float", None),
        _ => return None,
    })
}

/// D-SG9: parse a numeric type name (`Int`, `U8`, `I32`, `Float`, `F32`) to an
/// integer `(signed, bits)`, or `None` for floats / non-numeric names.
fn parse_int_name(name: &str) -> Option<(bool, u8)> {
    match name {
        "Int" => Some((true, 64)),
        "Float" | "F32" | "F64" => None,
        _ => {
            let signed = name.starts_with('I');
            if (signed || name.starts_with('U')) && name.len() > 1 {
                name[1..].parse::<u8>().ok().map(|b| (signed, b))
            } else {
                None
            }
        }
    }
}

/// D-NUMOPS1: emit a width conversion. Widening (target range ⊇ source range) and
/// every float-targeted/float-sourced conversion is an infallible `as`; integer
/// narrowing is a checked `try_from` returning `Result<T, String>`.
fn numeric_conversion(method: &str, recv: &str, src_name: Option<&str>) -> Option<String> {
    let (dst_rust, dst_spelling, dst_int) = conv_rust_target(method)?;
    let Some((dsigned, dbits)) = dst_int else {
        // Float target: int→float or float→float, always representable.
        return Some(format!("(({}) as {})", recv, dst_rust));
    };
    let src_int = src_name.and_then(parse_int_name);
    match src_int {
        Some((ssigned, sbits)) => {
            let (slo, shi) = crate::AST::int_range(ssigned, sbits);
            let (dlo, dhi) = crate::AST::int_range(dsigned, dbits);
            if dlo <= slo && shi <= dhi {
                Some(format!("(({}) as {})", recv, dst_rust)) // widening
            } else {
                Some(format!(
                    "<{dst_rust}>::try_from(({recv}) as i128).map_err(|_| \
                     \"value doesn't fit in {dst_spelling}\".to_string())"
                ))
            }
        }
        // Float (or unknown) source converting to an integer target: a saturating
        // `as` cast. Sema only reaches a float→int conversion as the infallible
        // form, so this stays type-consistent.
        None => Some(format!("(({}) as {})", recv, dst_rust)),
    }
}

fn emit_builtin_method(
    cx: &Cx,
    receiver: &Expr,
    method: &str,
    method_span: crate::Diagnostics::Span,
    args: &[crate::AST::CallArg],
    recv_type: Option<&str>,
    env: &HashMap<String, Slot>,
) -> Option<String> {
    let recv = emit_expr(cx, receiver, env);
    let arg = |i: usize| {
        args.get(i)
            .map(|a| emit_expr(cx, &a.expr, env))
            .unwrap_or_default()
    };
    if let Expr::Ident(name, _) = receiver {
        match (name.as_str(), method) {
            (Syntax::TYPE_INT, "parse") => {
                return Some(format!("({}).trim().parse::<i64>()", arg(0)));
            }
            (Syntax::TYPE_FLOAT, "parse") => {
                return Some(format!("({}).trim().parse::<f64>()", arg(0)));
            }
            (Syntax::TYPE_STRING, "from_bytes") => {
                return Some(format!(
                    "{}jet_string_from_bytes(&({}))",
                    cx.root_prefix,
                    arg(0)
                ));
            }
            _ => {}
        }
    }
    let rty = expr_jet_ty(receiver, env);
    match method {
        "len" => Some(match rty {
            Some(Type::String) => format!("jet_char_len(&({}))", recv),
            _ => format!("({}).len() as i64", recv),
        }),
        "is_empty" => Some(format!("({}).is_empty()", recv)),
        "push" => Some(format!("({}).push({})", recv, arg(0))),
        "pop" => Some(format!("({}).pop()", recv)),
        "insert" => Some(match rty {
            Some(Type::Map { .. }) => {
                format!("({}).insert(({}).clone(), {})", recv, arg(0), arg(1))
            }
            _ => format!("({}).insert({} as usize, {})", recv, arg(0), arg(1)),
        }),
        "remove" => Some(match rty {
            Some(Type::Map { .. }) => format!("({}).remove(&({}).clone())", recv, arg(0)),
            _ => {
                let (line, _) = span_line_col(&cx.src, method_span.start);
                format!("jet_list_remove(&mut ({}), {}, {:?}, {})", recv, arg(0), cx.file, line)
            }
        }),
        // D-ROUTE1=A: router.get/post/put/delete must come BEFORE the generic `"get"` arm.
        // Emit the handler as a boxed Send+Sync closure so it satisfies JetHttpHandler.
        "get" if matches!(&rty, Some(Type::Named(n)) if n == "HttpRouter") => Some(format!(
            "{}jet_http_router_register(&mut ({}), \"GET\".to_string(), {}, {})",
            cx.root_prefix, recv, arg(0), emit_router_handler(cx, args, env)
        )),
        "post" if matches!(&rty, Some(Type::Named(n)) if n == "HttpRouter") => Some(format!(
            "{}jet_http_router_register(&mut ({}), \"POST\".to_string(), {}, {})",
            cx.root_prefix, recv, arg(0), emit_router_handler(cx, args, env)
        )),
        "put" if matches!(&rty, Some(Type::Named(n)) if n == "HttpRouter") => Some(format!(
            "{}jet_http_router_register(&mut ({}), \"PUT\".to_string(), {}, {})",
            cx.root_prefix, recv, arg(0), emit_router_handler(cx, args, env)
        )),
        "delete" if matches!(&rty, Some(Type::Named(n)) if n == "HttpRouter") => Some(format!(
            "{}jet_http_router_register(&mut ({}), \"DELETE\".to_string(), {}, {})",
            cx.root_prefix, recv, arg(0), emit_router_handler(cx, args, env)
        )),
        "get" => Some(match rty {
            Some(Type::Map { .. }) => format!("({}).get(&({}).clone()).cloned()", recv, arg(0)),
            _ => format!("({}).get({} as usize).cloned()", recv, arg(0)),
        }),
        "first" => Some(format!("({}).first().cloned()", recv)),
        "last" => Some(format!("({}).last().cloned()", recv)),
        "contains" => Some(format!("({}).contains(&{})", recv, arg(0))),
        "index_of" => Some(format!(
            "({}).iter().position(|x| *x == {}).map(|i| i as i64)",
            recv,
            arg(0)
        )),
        "reverse" => Some(format!("({}).reverse()", recv)),
        "sort" => Some(format!("({}).sort()", recv)),
        "join" if args.is_empty() => Some(format!("({}).join()", recv)),
        // D-DETACH1: fire-and-forget — drop the JetTask handle (Rust drops the JoinHandle,
        // which detaches the thread; the task keeps running). Returns unit.
        // No type guard: `detach` is only valid on Task values (sema enforces this);
        // codegen drops the handle unconditionally.
        "detach" if args.is_empty() => {
            Some(format!("{{ let _detach = ({}); }}", recv))
        }
        "join" => Some(format!(
            "({}).iter().map(|x| x.jet_show()).collect::<Vec<_>>().join(({}).as_str())",
            recv,
            arg(0)
        )),
        "receive" => Some(format!("({}).receive()", recv)),
        "sender" => Some(format!("({}).sender()", recv)),
        "send" => Some(format!("({}).send({})", recv, arg(0))),
        "clear" => Some(format!("({}).clear()", recv)),
        "chars" => Some(format!("({}).chars().collect::<Vec<char>>()", recv)),
        "bytes" => Some(format!("{}jet_string_bytes(&({}))", cx.root_prefix, recv)),
        "trim" => Some(format!("({}).trim().to_string()", recv)),
        "split" => Some(format!("jet_string_split(&({}), &{})", recv, arg(0))),
        "starts_with" => Some(format!("({}).starts_with(&{})", recv, arg(0))),
        "ends_with" => Some(format!("({}).ends_with(&{})", recv, arg(0))),
        "replace" => Some(format!("({}).replace(&{}, &{})", recv, arg(0), arg(1))),
        "to_upper" => Some(format!("({}).to_uppercase()", recv)),
        "to_lower" => Some(format!("({}).to_lowercase()", recv)),
        "repeat" => Some(format!("({}).repeat({} as usize)", recv, arg(0))),
        "slice" => {
            let (line, _) = span_line_col(&cx.src, receiver.span().start);
            Some(format!(
                "jet_string_slice(&({}), {}, {}, {:?}, {})",
                recv,
                arg(0),
                arg(1),
                cx.file,
                line
            ))
        }
        "keys" => Some(format!("({}).keys().cloned().collect::<Vec<_>>()", recv)),
        "values" => Some(format!("({}).values().cloned().collect::<Vec<_>>()", recv)),
        "contains_key" => Some(format!("({}).contains_key(&{})", recv, arg(0))),
        "to_string" => Some(format!("({}).jet_show()", recv)),
        // D-NUMOPS1: float predicates (return bool directly).
        "is_nan" | "is_infinite" | "is_finite" => Some(format!("({}).{}()", recv, method)),
        // D-NUMOPS1: integer bit-population queries; Rust returns u32 → widen to Int.
        "count_ones" | "count_zeros" | "leading_zeros" | "trailing_zeros" => {
            Some(format!("(({}).{}() as i64)", recv, method))
        }
        // D-NUMOPS1: width conversions. Source width comes from sema (`recv_type`,
        // authoritative) with a literal/expr fallback; widening is `as`, narrowing
        // is a checked `try_from` returning `Result<T, String>`.
        _ if conv_rust_target(method).is_some() => {
            let src = recv_type
                .map(|s| s.to_string())
                .or_else(|| rty.as_ref().map(|t| t.name()));
            numeric_conversion(method, &recv, src.as_deref())
        }
        "elapsed_millis" => Some(format!(
            "{}jet_stopwatch_elapsed_millis(&({}))",
            cx.root_prefix, recv
        )),
        // E2-M7: FileWriter methods (D-IO2).
        "write_line" if matches!(&rty, Some(Type::Named(n)) if n == "FileWriter") => {
            Some(format!(
                "{}jet_std_file_writer_write_line(&mut ({}), &({}))",
                cx.root_prefix, recv, arg(0)
            ))
        }
        "flush" if matches!(&rty, Some(Type::Named(n)) if n == "FileWriter") => {
            Some(format!(
                "{}jet_std_file_writer_flush(&mut ({}))",
                cx.root_prefix, recv
            ))
        }
        // E2-M7: FileReader.lines() is handled in emit_for_in; read_line for direct use.
        "read_line" if matches!(&rty, Some(Type::Named(n)) if n == "FileReader") => {
            Some(format!(
                "{}jet_std_file_reader_read_line(&mut ({}))",
                cx.root_prefix, recv
            ))
        }
        // c109/I3: `.lines()` is consumed in loop position by `emit_for_in`
        // (lowered to `BufRead::lines`); it never reaches this expression path,
        // because sema rejects a line stream bound or used as a value (E2502).
        // This arm is dead — emit a diverging `unreachable!` (valid in any type
        // position, so no I2 risk) to document the sema guarantee.
        "lines" if matches!(&rty, Some(Type::Named(n)) if n == "FileReader") => {
            let _ = &recv;
            Some("unreachable!(\"jet: a line stream is loop-source-only (E2502)\")".to_string())
        }
        // D-STDIN1=A: StdinHandle methods.
        "read_line" if matches!(&rty, Some(Type::Named(n)) if n == "StdinHandle") => {
            Some(format!(
                "{}jet_std_io_stdin_read_line(&mut ({}))",
                cx.root_prefix, recv
            ))
        }
        // c109/I3: dead for the same reason as the FileReader arm above (E2502).
        "lines" if matches!(&rty, Some(Type::Named(n)) if n == "StdinHandle") => {
            let _ = &recv;
            Some("unreachable!(\"jet: a line stream is loop-source-only (E2502)\")".to_string())
        }
        // D-ALLOC1/D-ALLOC-C/D-ALLOC-D (ratified 2026-06-19): arena/bump/pool/fixed
        // instance methods. recv is a `JetArena`/`JetBump`/`JetPool`/`JetFixed`.
        "alloc" if matches!(&rty, Some(Type::Named(n)) if matches!(n.as_str(), "Arena" | "Bump" | "Pool" | "Fixed")) => {
            Some(format!("({}).alloc({})", recv, arg(0)))
        }
        "reset" if matches!(&rty, Some(Type::Named(n)) if matches!(n.as_str(), "Arena" | "Bump" | "Pool" | "Fixed")) => {
            Some(format!("({}).reset()", recv))
        }
        "free" if matches!(&rty, Some(Type::Named(n)) if matches!(n.as_str(), "Arena" | "Bump" | "Pool" | "Fixed")) => {
            Some(format!("drop({})", recv))
        }
        // E2-M10: TcpListener methods.
        "accept" if matches!(&rty, Some(Type::Named(n)) if n == "TcpListener") => Some(format!(
            "{}jet_net_tcp_accept(&({}))", cx.root_prefix, recv
        )),
        "local_addr" if matches!(&rty, Some(Type::Named(n)) if n == "TcpListener") => Some(format!(
            "{}jet_net_listener_local_addr(&({}))", cx.root_prefix, recv
        )),
        // E2-M10: TcpStream methods.
        "read" if matches!(&rty, Some(Type::Named(n)) if n == "TcpStream") => Some(format!(
            "{}jet_net_tcp_read(&mut ({}))", cx.root_prefix, recv
        )),
        "write" if matches!(&rty, Some(Type::Named(n)) if n == "TcpStream") => Some(format!(
            "{}jet_net_tcp_write(&mut ({}), &({}))", cx.root_prefix, recv, arg(0)
        )),
        "peer_addr" if matches!(&rty, Some(Type::Named(n)) if n == "TcpStream") => Some(format!(
            "{}jet_net_tcp_peer_addr(&({}))", cx.root_prefix, recv
        )),
        "local_addr" if matches!(&rty, Some(Type::Named(n)) if n == "TcpStream") => Some(format!(
            "{}jet_net_tcp_local_addr(&({}))", cx.root_prefix, recv
        )),
        "close" if matches!(&rty, Some(Type::Named(n)) if n == "TcpStream") => Some(format!(
            "{{ drop({}); }}", recv
        )),
        // E2-M10: HttpRequest field accessors.
        "method" if matches!(&rty, Some(Type::Named(n)) if n == "HttpRequest") => Some(format!(
            "({}).method.clone()", recv
        )),
        "path" if matches!(&rty, Some(Type::Named(n)) if n == "HttpRequest") => Some(format!(
            "({}).path.clone()", recv
        )),
        "body" if matches!(&rty, Some(Type::Named(n)) if n == "HttpRequest") => Some(format!(
            "({}).body.clone()", recv
        )),
        "header" if matches!(&rty, Some(Type::Named(n)) if n == "HttpRequest") => Some(format!(
            "({}).headers.get(&{}).cloned()", recv, arg(0)
        )),
        // D-ROUTE1=A: req.param(name) → String?
        "param" if matches!(&rty, Some(Type::Named(n)) if n == "HttpRequest") => Some(format!(
            "{}jet_http_request_param(&({}), &({}))", cx.root_prefix, recv, arg(0)
        )),
        // E2-M10: HttpResponse field accessors.
        "status" if matches!(&rty, Some(Type::Named(n)) if n == "HttpResponse") => Some(format!(
            "({}).status.clone()", recv
        )),
        "body" if matches!(&rty, Some(Type::Named(n)) if n == "HttpResponse") => Some(format!(
            "({}).body.clone()", recv
        )),
        "header" if matches!(&rty, Some(Type::Named(n)) if n == "HttpResponse") => Some(format!(
            "({}).headers.get(&{}).cloned()", recv, arg(0)
        )),
        // D-REGEX1: `Match.group(n)` — capture group n as `String?`. The match is a
        // `Vec<Option<String>>`; indexing past the end (or an absent group) is `none`.
        "group" if matches!(&rty, Some(Type::Named(n)) if n == "Match") => Some(format!(
            "({}).get(({}) as usize).cloned().flatten()", recv, arg(0)
        )),
        "map" => {
            if let Expr::Lambda(l) = &args[0].expr {
                if l.meta.needs_fn_mut {
                    Some(format!("jet_list_map_mut(({}).clone(), {})", recv, arg(0)))
                } else {
                    Some(format!("jet_list_map(({}).clone(), {})", recv, arg(0)))
                }
            } else {
                Some(format!("jet_list_map(({}).clone(), {})", recv, arg(0)))
            }
        }
        "filter" => Some(format!("jet_list_filter(({}).clone(), {})", recv, arg(0))),
        "each" => {
            let trait_obj_list =
                matches!(rty, Some(Type::List(ref inner)) if list_carries_trait(cx, inner));
            let list_each = |a: &str| {
                if trait_obj_list {
                    format!("jet_list_each_ref(&({recv}), {a})")
                } else if let Expr::Lambda(l) = &args[0].expr {
                    if l.meta.needs_fn_mut {
                        format!("jet_list_each_mut(({}).clone(), {})", recv, a)
                    } else {
                        format!("jet_list_each(({}).clone(), {})", recv, a)
                    }
                } else {
                    format!("jet_list_each(({}).clone(), {})", recv, a)
                }
            };
            match rty {
                Some(Type::Map { .. }) => {
                    Some(format!("jet_map_each(({}).clone(), {})", recv, arg(0)))
                }
                _ => Some(list_each(&arg(0))),
            }
        }
        "find" => Some(format!("jet_list_find(({}).clone(), {})", recv, arg(0))),
        "any" => Some(format!("jet_list_any(({}).clone(), {})", recv, arg(0))),
        "all" => Some(format!("jet_list_all(({}).clone(), {})", recv, arg(0))),
        "sort_by" => Some(format!(
            "{{ jet_list_sort_by(&mut {}, {}); }}",
            recv,
            arg(0)
        )),
        "reduce" => Some(format!(
            "jet_list_reduce(({}).clone(), {}, {})",
            recv,
            arg(0),
            arg(1)
        )),
        _ => None,
    }
}

/// D-ROUTE1=A: emit the handler argument for router.get/post/put/delete as a
/// `Box<dyn Fn(JetHttpRequest)->JetHttpResponse + Send + Sync>` (JetHttpHandler).
/// The second arg (index 1) is the handler: either a named fn reference or a lambda.
fn emit_router_handler(
    cx: &Cx,
    args: &[crate::AST::CallArg],
    env: &HashMap<String, Slot>,
) -> String {
    let Some(handler_arg) = args.get(1) else {
        return "/* missing handler */".to_string();
    };
    // For a named function reference (Ident not in local env = top-level fn),
    // emit it directly — Rust can coerce fn items to the closure bound.
    if let Expr::Ident(name, _) = &handler_arg.expr {
        if env.get(name).is_none() {
            let rust_name = mangle(name);
            return format!(
                "Box::new(move |__req: {}JetHttpRequest| {}(&__req)) as Box<dyn Fn({}JetHttpRequest) -> {}JetHttpResponse + Send + Sync>",
                cx.root_prefix, rust_name, cx.root_prefix, cx.root_prefix
            );
        }
    }
    // For a lambda, emit it directly — lambdas are already closures.
    let inner = emit_expr(cx, &handler_arg.expr, env);
    format!(
        "Box::new({}) as Box<dyn Fn({}JetHttpRequest) -> {}JetHttpResponse + Send + Sync>",
        inner, cx.root_prefix, cx.root_prefix
    )
}

/// D-REGEX1: qualify a regex runtime function with the FFI bridge crate name.
/// The runtime lives only in that crate (the sole `regex`-crate dependant), so
/// `cx.ffi_crate` is always `Some` here — `FFI::prepare` builds the bridge
/// whenever `jet.regex` is imported.
fn regex_fn(cx: &Cx, name: &str) -> String {
    let crate_name = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
    format!("{}::{}", crate_name, name)
}

fn emit_core_field(module: &str, name: &str) -> String {
    match (module, name) {
        ("core.math", "pi") => "std::f64::consts::PI".to_string(),
        ("core.math", "e") => "std::f64::consts::E".to_string(),
        // D-ALLOC1/D-ALLOC-C (ratified 2026-06-19): allocator type sentinels.
        // These only appear as the receiver of `.new()` — see emit_method_call.
        ("core.mem", "Arena") => "jet_mem::JetArena".to_string(),
        ("core.mem", "Bump") => "jet_mem::JetBump".to_string(),
        ("core.mem", "Pool") => "jet_mem::JetPool".to_string(),
        ("core.mem", "Fixed") => "jet_mem::JetFixed".to_string(),
        _ => "/* unknown std field */".to_string(),
    }
}

fn emit_core_call(
    cx: &Cx,
    module: &str,
    method: &str,
    args: &[crate::AST::CallArg],
    env: &HashMap<String, Slot>,
) -> String {
    let arg = |i: usize| {
        args.get(i)
            .map(|a| emit_expr(cx, &a.expr, env))
            .unwrap_or_default()
    };
    let helper = |name: &str| format!("{}{}", cx.root_prefix, name);
    match (module, method) {
        // S58 (E2-M13): low-level pointer ops. `address_of` is inert (a plain
        // address as Int); `volatile_read` reads through a `Ptr<T>`. Both are
        // only reachable inside an `@unsafe` gate (sema E3101), which codegen
        // has already lowered to a Rust `unsafe` region, so `read_volatile`
        // sits in a valid unsafe context.
        ("core.mem", "address_of") => format!("(&({}) as *const _ as usize as i64)", arg(0)),
        ("core.mem", "volatile_read") => format!("std::ptr::read_volatile({})", arg(0)),
        ("core.fs", "read") => format!("{}(&({}))", helper("jet_std_fs_read"), arg(0)),
        ("core.fs", "read_bytes") => format!("{}(&({}))", helper("jet_std_fs_read_bytes"), arg(0)),
        ("core.fs", "write") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_write"),
            arg(0),
            arg(1)
        ),
        ("core.fs", "append") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_append"),
            arg(0),
            arg(1)
        ),
        ("core.fs", "exists") => format!("{}(&({}))", helper("jet_std_fs_exists"), arg(0)),
        ("core.fs", "remove") => format!("{}(&({}))", helper("jet_std_fs_remove"), arg(0)),
        ("core.fs", "list_dir") => format!("{}(&({}))", helper("jet_std_fs_list_dir"), arg(0)),
        ("core.fs", "create_dir") => format!("{}(&({}))", helper("jet_std_fs_create_dir"), arg(0)),
        ("core.fs", "is_dir") => format!("{}(&({}))", helper("jet_std_fs_is_dir"), arg(0)),
        ("core.fs", "copy") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_copy"),
            arg(0),
            arg(1)
        ),
        ("core.fs", "rename") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_rename"),
            arg(0),
            arg(1)
        ),
        ("core.io", "args") => format!("{}()", helper("jet_std_io_args")),
        ("core.io", "input") => {
            if args.is_empty() {
                format!("{}(None)", helper("jet_std_io_input"))
            } else {
                format!("{}(Some(&({})))", helper("jet_std_io_input"), arg(0))
            }
        }
        ("core.io", "read_all_input") => format!("{}()", helper("jet_std_io_read_all_input")),
        // D-STDIN1=A: io.stdin() → JetStdinReader handle.
        ("core.io", "stdin") => format!("{}()", helper("jet_std_io_stdin")),
        ("core.io", "eprint") => format!("eprintln!(\"{{}}\", ({}).jet_show())", arg(0)),
        ("core.env", "get") => format!("{}(&({}))", helper("jet_std_env_get"), arg(0)),
        ("core.env", "set") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_env_set"),
            arg(0),
            arg(1)
        ),
        ("core.env", "current_dir") => format!("{}()", helper("jet_std_env_current_dir")),
        ("core.env", "home_dir") => format!("{}()", helper("jet_std_env_home_dir")),
        ("core.process", "exit") => format!("{}({})", helper("jet_std_process_exit"), arg(0)),
        ("core.process", "run") => format!("{}(&({}))", helper("jet_std_process_run"), arg(0)),
        ("core.math", "sqrt") => format!("{}({})", helper("jet_std_math_sqrt"), arg(0)),
        ("core.math", "pow") => format!("{}({}, {})", helper("jet_std_math_pow"), arg(0), arg(1)),
        ("core.math", "abs") => format!("({}).abs()", arg(0)),
        ("core.math", "min") => format!("({}).min({})", arg(0), arg(1)),
        ("core.math", "max") => format!("({}).max({})", arg(0), arg(1)),
        ("core.math", "floor") => format!("{}({})", helper("jet_std_math_floor"), arg(0)),
        ("core.math", "ceil") => format!("{}({})", helper("jet_std_math_ceil"), arg(0)),
        ("core.math", "round") => format!("{}({})", helper("jet_std_math_round"), arg(0)),
        ("core.math", "clamp") => format!("({}).clamp({}, {})", arg(0), arg(1), arg(2)),
        ("core.random", "int") => {
            format!("{}({}, {})", helper("jet_std_random_int"), arg(0), arg(1))
        }
        ("core.random", "float") => format!("{}()", helper("jet_std_random_float")),
        ("core.random", "pick") => format!("{}(&({}))", helper("jet_std_random_pick"), arg(0)),
        ("core.random", "shuffle") => {
            format!("{}(&mut ({}))", helper("jet_std_random_shuffle"), arg(0))
        }
        ("core.random", "seed") => format!("{}({})", helper("jet_std_random_seed"), arg(0)),
        ("core.time", "now") => format!("{}()", helper("jet_std_time_now")),
        ("core.time", "sleep") => format!("{}({})", helper("jet_std_time_sleep"), arg(0)),
        ("core.time", "start") => format!("{}()", helper("jet_std_time_start")),
        ("core.json", "parse") => format!("{}(&({}))", helper("jet_std_json_parse"), arg(0)),
        ("core.json", "render") => format!("{}(&({}))", helper("jet_std_json_render"), arg(0)),
        ("core.json", "render_pretty") => {
            format!("{}(&({}))", helper("jet_std_json_render_pretty"), arg(0))
        }
        ("core.tasks", "spawn") => {
            if let Some(lam_arg) = args.first() {
                if let crate::AST::Expr::Lambda(lam) = &lam_arg.expr {
                    let closure = emit_spawn_lambda(cx, lam, env);
                    return format!("{}jet_std::JetTask::spawn({})", cx.root_prefix, closure);
                }
            }
            format!("{}jet_std::JetTask::spawn({})", cx.root_prefix, arg(0))
        }
        ("core.tasks", "channel") => format!("{}jet_std::JetChannel::new()", cx.root_prefix),
        // E2-M7: streaming file handles (D-IO2).
        ("core.files", "open") => format!("{}(&({}))", helper("jet_std_files_open"), arg(0)),
        ("core.files", "create") => format!("{}(&({}))", helper("jet_std_files_create"), arg(0)),
        ("core.files", "append") => format!("{}(&({}))", helper("jet_std_files_append"), arg(0)),
        // E2-M7: std.path helpers (D-IO1).
        ("core.path", "join") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_path_join"), arg(0), arg(1)
        ),
        ("core.path", "parent") => format!("{}(&({}))", helper("jet_std_path_parent"), arg(0)),
        ("core.path", "extension") => format!("{}(&({}))", helper("jet_std_path_extension"), arg(0)),
        ("core.path", "normalize") => format!("{}(&({}))", helper("jet_std_path_normalize"), arg(0)),
        // E2-M9: first-party ring packages.
        ("jet.csv", "parse") => format!("{}(&({}))", helper("jet_ring_csv_parse"), arg(0)),
        ("jet.csv", "render") => format!("{}(&({}))", helper("jet_ring_csv_render"), arg(0)),
        ("jet.toml", "parse") => format!("{}(&({}))", helper("jet_ring_toml_parse"), arg(0)),
        ("jet.toml", "render") => format!("{}(&({}))", helper("jet_ring_toml_render"), arg(0)),
        ("jet.yaml", "parse") => format!("{}(&({}))", helper("jet_ring_yaml_parse"), arg(0)),
        ("jet.yaml", "render") => format!("{}(&({}))", helper("jet_ring_yaml_render"), arg(0)),
        ("jet.log", "info") => format!("{}(&({}))", helper("jet_ring_log_info"), arg(0)),
        ("jet.log", "warn") => format!("{}(&({}))", helper("jet_ring_log_warn"), arg(0)),
        ("jet.log", "error") => format!("{}(&({}))", helper("jet_ring_log_error"), arg(0)),
        ("jet.log", "debug") => format!("{}(&({}))", helper("jet_ring_log_debug"), arg(0)),
        ("jet.log", "set_level") => format!("{}(&({}))", helper("jet_ring_log_set_level"), arg(0)),
        // E2-M12 D-OBS3: trace context for structured log records.
        ("jet.log", "set_trace_id") => format!("{}(&({}))", helper("jet_ring_log_set_trace_id"), arg(0)),
        // D-LOGFMT1=A: explicit log format override.
        ("jet.log", "setup") => format!("{}(&({}))", helper("jet_ring_log_setup"), arg(0)),
        ("jet.json", "parse") => format!("{}(&({}))", helper("jet_std_json_parse"), arg(0)),
        // D-JSON3=B: lenient decode emits one log line per coercion; decoded value is plain.
        ("jet.json", "decode") => format!("{}(&({}))", helper("jet_std_json_decode_lenient"), arg(0)),
        ("jet.json", "render") => format!("{}(&({}))", helper("jet_std_json_render"), arg(0)),
        ("jet.json", "render_pretty") => format!("{}(&({}))", helper("jet_std_json_render_pretty"), arg(0)),
        ("jet.time", "now") => format!("{}()", helper("jet_std_time_now")),
        ("jet.time", "format") => format!("{}({}, &({}))", helper("jet_ring_time_format"), arg(0), arg(1)),
        ("jet.crypto", "sha256") => format!("{}(&({}))", helper("jet_ring_crypto_sha256"), arg(0)),
        ("jet.crypto", "sha256_bytes") => format!("{}(&({}))", helper("jet_ring_crypto_sha256_bytes"), arg(0)),
        // E2-M10: core.net — blocking TCP sockets.
        ("core.net", "tcp_listen") => format!("{}(&({}))", helper("jet_net_tcp_listen"), arg(0)),
        ("core.net", "tcp_accept") => format!("{}(&({}))", helper("jet_net_tcp_accept"), arg(0)),
        ("core.net", "tcp_connect") => format!("{}(&({}))", helper("jet_net_tcp_connect"), arg(0)),
        ("core.net", "tcp_read") => format!("{}(&mut ({}))", helper("jet_net_tcp_read"), arg(0)),
        ("core.net", "tcp_write") => {
            format!("{}(&mut ({}), &({}))", helper("jet_net_tcp_write"), arg(0), arg(1))
        }
        ("core.net", "tcp_local_addr") => format!("{}(&({}))", helper("jet_net_tcp_local_addr"), arg(0)),
        ("core.net", "tcp_peer_addr") => format!("{}(&({}))", helper("jet_net_tcp_peer_addr"), arg(0)),
        ("core.net", "set_timeout") => {
            format!("{}(&mut ({}), {})", helper("jet_net_set_timeout"), arg(0), arg(1))
        }
        ("core.net", "tcp_reply") => {
            format!("{}({}, &({}), &({}))", helper("jet_net_tcp_reply"), arg(0), arg(1), arg(2))
        }
        // E2-M10: jet.http — HTTP client and server.
        ("jet.http", "get") => format!("{}(&({}))", helper("jet_http_get"), arg(0)),
        ("jet.http", "post") => {
            format!("{}(&({}), &({}))", helper("jet_http_post"), arg(0), arg(1))
        }
        // D-ROUTE1=A: jet.http.router() → JetHttpRouter.
        ("jet.http", "router") => format!("{}()", helper("jet_http_router_new")),
        // D-ROUTE1=A: http.parse(raw) → JetHttpRequest.
        ("jet.http", "parse") => format!("{}(&({}))", helper("jet_http_parse_request"), arg(0)),
        // D-ROUTE1=A: http.dispatch(router, req) → JetHttpResponse.
        ("jet.http", "dispatch") => format!(
            "{}(&({}), {})",
            helper("jet_http_router_dispatch"), arg(0), arg(1)
        ),
        ("jet.http", "serve") => {
            // serve(addr, handler): blocking — handler is a closure/fn or HttpRouter.
            // Detect a router arg by inspecting the Expr: a Lambda is always a closure;
            // anything else is assumed to be an HttpRouter (sema already enforced this).
            let is_lambda = args.get(1).map(|a| matches!(a.expr, crate::AST::Expr::Lambda(_))).unwrap_or(false);
            if is_lambda {
                format!("{}(&({}), {})", helper("jet_http_serve"), arg(0), arg(1))
            } else {
                // Wrap the router in a closure so jet_http_serve's F: Fn bound is satisfied.
                let rtr = arg(1);
                let root = &cx.root_prefix;
                format!(
                    "{{ let __rtr = {}; {}(&({}), move |req| {}jet_http_router_dispatch(&__rtr, req)); }}",
                    rtr, helper("jet_http_serve"), arg(0), root
                )
            }
        }
        // D-REGEX1: jet.regex — calls land in the FFI bridge crate (the only place
        // the `regex` crate lives). `regex_fn` qualifies them with the bridge crate
        // name carried in `cx.ffi_crate`.
        ("jet.regex", "is_match") => {
            format!("{}(&({}), &({}))", regex_fn(cx, "jet_regex_is_match"), arg(0), arg(1))
        }
        ("jet.regex", "match") => {
            format!("{}(&({}), &({}))", regex_fn(cx, "jet_regex_match"), arg(0), arg(1))
        }
        ("jet.regex", "find") => {
            format!("{}(&({}), &({}))", regex_fn(cx, "jet_regex_find"), arg(0), arg(1))
        }
        ("jet.regex", "find_all") => {
            format!("{}(&({}), &({}))", regex_fn(cx, "jet_regex_find_all"), arg(0), arg(1))
        }
        ("jet.regex", "split") => {
            format!("{}(&({}), &({}))", regex_fn(cx, "jet_regex_split"), arg(0), arg(1))
        }
        ("jet.regex", "replace") => format!(
            "{}(&({}), &({}), &({}))",
            regex_fn(cx, "jet_regex_replace"), arg(0), arg(1), arg(2)
        ),
        ("jet.regex", "replace_all") => format!(
            "{}(&({}), &({}), &({}))",
            regex_fn(cx, "jet_regex_replace_all"), arg(0), arg(1), arg(2)
        ),
        // D-DEFER1 option B: scope.guard(() => { … }) → JetScopeGuard<F>
        // The closure is emitted directly; Rust infers the generic F.
        // Drop runs the closure on every exit path (LIFO by reverse-declaration order).
        ("core.scope", "guard") => {
            format!("{}jet_scope_guard({})", cx.root_prefix, arg(0))
        }
        _ => "/* unknown std call */".to_string(),
    }
}

fn emit_core_json_lit(
    cx: &Cx,
    variant: &str,
    args: &[crate::AST::CallArg],
    env: &HashMap<String, Slot>,
) -> String {
    let arg = |i: usize| {
        args.get(i)
            .map(|a| {
                let s = emit_expr(cx, &a.expr, env);
                if a.flags.implicit_clone {
                    format!("({}).clone()", s)
                } else {
                    s
                }
            })
            .unwrap_or_default()
    };
    let prefix = format!("{}jet_std::Json", cx.root_prefix);
    match variant {
        "Null" => format!("{prefix}::Null"),
        "Boolean" => format!("{prefix}::Boolean({})", arg(0)),
        "Number" => format!("{prefix}::Number({})", arg(0)),
        "Text" => format!("{prefix}::Text({})", arg(0)),
        "Array" => format!("{prefix}::Array({})", arg(0)),
        "Object" => format!("{prefix}::Object({})", arg(0)),
        _ => "/* unknown JSON variant */".to_string(),
    }
}

fn emit_method_call(
    cx: &Cx,
    receiver: &Expr,
    method: &str,
    method_span: crate::Diagnostics::Span,
    args: &[crate::AST::CallArg],
    recv_type: Option<&str>,
    env: &HashMap<String, Slot>,
) -> String {
    if method == "clone" {
        return format!("({}).clone()", emit_expr(cx, receiver, env));
    }
    // D-ALLOC1/D-ALLOC-C (ratified 2026-06-19): `mem.Arena.new()` and friends.
    // The receiver is `Field(Ident("mem"), "Arena")` — detect the constructor call.
    if method == Syntax::MEM_ALLOC_NEW {
        if let Expr::Field(inner, alloc_type, _) = receiver {
            if let Expr::Ident(alias, _) = &**inner {
                if cx.core_imports.get(alias).map(|m| m == Syntax::CORE_MEM_MODULE).unwrap_or(false) {
                    let rust_type = match alloc_type.as_str() {
                        "Arena" => "jet_mem::JetArena",
                        "Bump" => "jet_mem::JetBump",
                        "Pool" => "jet_mem::JetPool",
                        "Fixed" => "jet_mem::JetFixed",
                        _ => return format!("/* unknown allocator {} */", alloc_type),
                    };
                    // Capacity/slots/size optional arg.
                    let arg = |i: usize| {
                        args.get(i).map(|a| emit_expr(cx, &a.expr, env)).unwrap_or_default()
                    };
                    if args.is_empty() {
                        return format!("{}::new()", rust_type);
                    }
                    // The optional arg label determines the constructor variant:
                    // capacity: → with_capacity (Arena, Bump)
                    // slots:    → with_slots (Pool)
                    // size:     → with_size (Fixed)
                    let ctor = match alloc_type.as_str() {
                        "Pool" => "with_slots",
                        "Fixed" => "with_size",
                        _ => "with_capacity",
                    };
                    return format!("{}::{}({} as usize)", rust_type, ctor, arg(0));
                }
            }
        }
    }
    // D-DIST3 (ratified 2026-06-20): `.raw()` on a distinct type — lowers to `.0`.
    if method == crate::Syntax::METHOD_DISTINCT_RAW {
        let recv = emit_expr(cx, receiver, env);
        return format!("({}).0", recv);
    }
    // D-TOOL4 (E2-M11): `expect(x).snapshot()` — snapshot assertion.
    // The receiver is a `Call` to `expect`; emit the jet_expect(…).snapshot(…) form.
    if method == crate::Syntax::BUILTIN_SNAPSHOT {
        if let Expr::Call(call) = receiver {
            if call.name == crate::Syntax::BUILTIN_EXPECT && call.args.len() == 1 {
                let val = emit_expr(cx, &call.args[0].expr, env);
                // Derive a stable snapshot path from the source file + location.
                let (line, _) = span_line_col(&cx.src, call.args[0].expr.span().start);
                let snap_path = format!("snapshots/{}_{}.snap", cx.file.replace(['/', '\\', '.'], "_"), line);
                return format!(
                    "jet_expect(format!(\"{{}}\", ({}).jet_show())).snapshot({snap_path:?})?",
                    val,
                    snap_path = snap_path
                );
            }
        }
    }
    let struct_ty = recv_type
        .map(|s| s.to_string())
        .or_else(|| receiver_struct_type(receiver, env));
    if let Some(type_name) = struct_ty {
        if let Some(fields) = cx.struct_fields.get(&type_name) {
            if let Some((_, field_ty)) = fields.iter().find(|(n, _)| n == method) {
                if matches!(field_ty, Type::Fn { .. }) {
                    let recv = emit_expr(cx, receiver, env);
                    let arg_str = emit_call_args(cx, None, args, env);
                    return format!("(({}).{})({})", recv, mangle(method), arg_str);
                }
            }
        }
    }
    if let Expr::Ident(alias, _) = receiver {
        if let Some(module) = cx.core_imports.get(alias) {
            return emit_core_call(cx, module, method, args, env);
        }
        // D-MOD4: `pub use` re-export — emit the module that really defines the item.
        if let Some((real_mod, real_fn)) =
            cx.reexport_calls.get(&(alias.clone(), method.to_string()))
        {
            let sig = cx
                .import_sigs
                .get(&(alias.clone(), method.to_string()))
                .map(|s| s.as_slice());
            let arg_str = emit_call_args(cx, sig, args, env);
            return format!(
                "{}{}::{}({})",
                cx.root_prefix,
                real_mod,
                mangle(real_fn),
                arg_str
            );
        }
        if let Some(mod_name) = cx.import_mods.get(alias) {
            let sig = cx
                .import_sigs
                .get(&(alias.clone(), method.to_string()))
                .map(|s| s.as_slice());
            let arg_str = emit_call_args(cx, sig, args, env);
            return format!(
                "{}{}::{}({})",
                cx.root_prefix,
                mod_name,
                mangle(method),
                arg_str
            );
        }
        // D-MOD2: inline code module call — emit `user_{alias}__{method}(args)`.
        // The function was emitted as `user_{alias}__{method}` by emit_program_items.
        if cx.code_modules.contains(alias.as_str()) {
            let mangled_key = format!("{}__{}", alias, method);
            let sig = cx.sigs.get(&mangled_key).map(|s| s.as_slice());
            let arg_str = emit_call_args(cx, sig, args, env);
            return format!("{}user_{}__{}({})", cx.root_prefix, alias, method, arg_str);
        }
    }
    // Built-in collection/string methods take precedence when they match.
    if let Some(s) = emit_builtin_method(cx, receiver, method, method_span, args, recv_type, env) {
        return s;
    }
    if let Expr::Ident(type_name, _) = receiver {
        if is_json_type_name(type_name) {
            return emit_core_json_lit(cx, method, args, env);
        }
        if let Some(variants) = cx.enum_variants.get(type_name) {
            if variants.iter().any(|(v, _)| v == method) {
                let enum_args: Vec<EnumLitArg> = args
                    .iter()
                    .map(|a| EnumLitArg::Positional(a.expr.clone()))
                    .collect();
                return emit_enum_lit(cx, type_name, method, &enum_args, env);
            }
        }
        if cx.type_names.contains(type_name) {
            let sig = cx.method_sigs.get(&(type_name.clone(), method.to_string()));
            let arg_str = emit_call_args(cx, sig.map(|s| s.as_slice()), args, env);
            return format!(
                "{}::{}({})",
                cx.type_prefix(type_name),
                cx.mangle_name(method),
                arg_str
            );
        }
    }
    let recv = emit_expr(cx, receiver, env);
    if let Some(rt) = recv_type {
        if cx.trait_names.contains(rt) {
            let arg_str = emit_call_args(cx, None, args, env);
            return format!("({}).{}({})", recv, method, arg_str);
        }
    }
    let sig = recv_type.and_then(|t| cx.method_sigs.get(&(t.to_string(), method.to_string())));
    let arg_str = emit_call_args(cx, sig.map(|s| s.as_slice()), args, env);
    // S62: trait-impl methods are not prefixed with `user_` in Rust (the trait
    // owns the name). Check the recv_type's trait_methods set.
    let method_name = if recv_type.is_some_and(|rt| cx.trait_methods.contains(&(rt.to_string(), method.to_string()))) {
        method.to_string()
    } else {
        mangle(method).to_string()
    };
    format!("({}).{}({})", recv, method_name, arg_str)
}

fn emit_call_args(
    cx: &Cx,
    sig: Option<&[(AccessConvention, Type)]>,
    args: &[crate::AST::CallArg],
    env: &HashMap<String, Slot>,
) -> String {
    args.iter()
        .enumerate()
        .map(|(i, a)| {
            let mut s = emit_expr(cx, &a.expr, env);
            if a.flags.implicit_clone {
                s = format!("({}).clone()", s);
            } else if a.flags.shared_auto_clone {
                s = format!("std::sync::Arc::clone(&{})", s);
            }
            let conv = sig.and_then(|ps| ps.get(i)).map(|(c, t)| (*c, t.clone()));
            if let Some((_, Type::Fn { .. })) = &conv {
                let already_boxed = s.starts_with("Box::new(")
                    || matches!(
                        &a.expr,
                        Expr::Ident(name, _)
                            if env
                                .get(name)
                                .and_then(|slot| slot.jet_ty.as_ref())
                                .is_some_and(|t| matches!(t, Type::Fn { .. }))
                    );
                if !already_boxed {
                    s = format!("Box::new({})", s);
                }
                if let Some((_, ty)) = &conv {
                    s = format!("{} as {}", s, cx.rust_type(ty));
                }
            }
            match conv {
                Some((AccessConvention::Read, t))
                    if !t.is_scalar() && !matches!(t, Type::Fn { .. }) =>
                {
                    format!("&({})", s)
                }
                Some((AccessConvention::Mutate, _)) => format!("&mut ({})", s),
                _ => s,
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn emit_str(cx: &Cx, parts: &[StrPart], env: &HashMap<String, Slot>) -> String {
    if parts.len() == 1 {
        if let StrPart::Lit(s) = &parts[0] {
            return format!("{:?}.to_string()", s);
        }
    }
    let mut body = String::from("{ let mut _jet_s = String::new(); ");
    for p in parts {
        match p {
            StrPart::Lit(s) => {
                if !s.is_empty() {
                    body.push_str(&format!("_jet_s.push_str({:?}); ", s));
                }
            }
            StrPart::Interp(e) => {
                body.push_str(&format!(
                    "_jet_s.push_str(&({}).jet_show()); ",
                    emit_expr(cx, e, env)
                ));
            }
        }
    }
    body.push_str("_jet_s }");
    body
}

fn emit_call(cx: &Cx, call: &crate::AST::Call, env: &HashMap<String, Slot>) -> String {
    if call.name == Syntax::BUILTIN_PRINT {
        let arg = emit_expr(cx, &call.args[0].expr, env);
        return format!("println!(\"{{}}\", ({}).jet_show())", arg);
    }
    // D-NUMOPS1: overflow opt-ins lower the wrapped op to Rust's matching method.
    // `checked_*` returns `Option`; the others return the value. Sema validated
    // the shape, so the argument is one integer `+`/`-`/`*`/`/`.
    if matches!(
        call.name.as_str(),
        Syntax::BUILTIN_WRAPPING | Syntax::BUILTIN_SATURATING | Syntax::BUILTIN_CHECKED
    ) && !cx.sigs.contains_key(&call.name)
    {
        if let Some(Expr::Binary(op, l, r, _)) = call.args.first().map(|a| &a.expr) {
            use crate::AST::BinOp;
            let suffix = match op {
                BinOp::Add => "add",
                BinOp::Sub => "sub",
                BinOp::Mul => "mul",
                BinOp::Div => "div",
                _ => "add",
            };
            let ls = emit_expr(cx, l, env);
            let rs = emit_expr(cx, r, env);
            return format!("({}).{}_{}({})", ls, call.name, suffix, rs);
        }
    }
    // D-PRELUDE1 = B: bare `input(...)` is ambient — same lowering as `io.input(...)`.
    // Only applies when the user has not defined their own `input` function
    // (user-defined shadows the prelude, handled by sema; codegen follows suit).
    if call.name == Syntax::BUILTIN_INPUT
        && !cx.sigs.contains_key(Syntax::BUILTIN_INPUT)
        && !env.contains_key(Syntax::BUILTIN_INPUT)
    {
        let helper = |name: &str| format!("{}{}", cx.root_prefix, name);
        return if call.args.is_empty() {
            format!("{}(None)", helper("jet_std_io_input"))
        } else {
            let arg = emit_expr(cx, &call.args[0].expr, env);
            format!("{}(Some(&({})))", helper("jet_std_io_input"), arg)
        };
    }
    if call.name == Syntax::BUILTIN_PANIC {
        return emit_panic_stop(cx, call, env);
    }
    if call.name == Syntax::BUILTIN_REQUIRE {
        return emit_require(cx, call, env);
    }
    if call.name == Syntax::BUILTIN_REQUIRE_EQ {
        return emit_require_eq(cx, call, env);
    }
    // D-TOOL4 (E2-M11): `expect(x)` emits as `jet_expect(format!("{}", x.jet_show()))`.
    // The full `.snapshot()` form is handled in emit_method_call above.
    if call.name == Syntax::BUILTIN_EXPECT {
        if call.args.len() == 1 {
            let val = emit_expr(cx, &call.args[0].expr, env);
            return format!("jet_expect(format!(\"{{}}\", ({}).jet_show()))", val);
        }
        return "jet_expect(String::new())".to_string();
    }
    if env.contains_key(&call.name) && !cx.consts.contains_key(&call.name) {
        let callee = place_of(env, &call.name);
        let arg_str = emit_call_args(cx, None, &call.args, env);
        return format!("({})({})", callee, arg_str);
    }
    let sig = cx.sigs.get(&call.name);
    let args = if cx.extern_funcs.contains_key(&call.name) {
        emit_extern_call_args(cx, sig.map(|s| s.as_slice()), &call.args, env)
    } else {
        emit_call_args(cx, sig.map(|s| s.as_slice()), &call.args, env)
    };
    if let Some(wrapper) = cx.extern_funcs.get(&call.name) {
        let crate_name = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
        return format!("{}::{}({})", crate_name, wrapper, args);
    }
    // D-MOD3: unqualified inline-module import — emit `user_{alias}__{method}(...)`.
    if let Some(mangled_key) = cx.unqualified_inline.get(&call.name) {
        let sig = cx.sigs.get(mangled_key).map(|s| s.as_slice());
        let arg_str = emit_call_args(cx, sig, &call.args, env);
        return format!("{}user_{}({})", cx.root_prefix, mangled_key, arg_str);
    }
    // D-MOD3: unqualified file-module import — emit `{root}{rust_mod}::user_{fn}(...)`.
    if let Some((rust_mod, fn_name)) = cx.unqualified_file.get(&call.name) {
        let sig = cx
            .import_sigs
            .get(&(call.name.clone(), fn_name.clone()))
            .map(|s| s.as_slice());
        let arg_str = emit_call_args(cx, sig, &call.args, env);
        return format!("{}{}::{}({})", cx.root_prefix, rust_mod, mangle(fn_name), arg_str);
    }
    format!("{}({})", cx.mangle_name(&call.name), args)
}

fn emit_extern_call_args(
    cx: &Cx,
    sig: Option<&[(AccessConvention, Type)]>,
    args: &[crate::AST::CallArg],
    env: &HashMap<String, Slot>,
) -> String {
    args.iter()
        .enumerate()
        .map(|(i, a)| {
            let mut s = emit_expr(cx, &a.expr, env);
            if a.flags.implicit_clone {
                s = format!("({}).clone()", s);
            } else if a.flags.shared_auto_clone {
                s = format!("std::sync::Arc::clone(&{})", s);
            }
            if let Some((_, ty)) = sig.and_then(|ps| ps.get(i)) {
                if !ty.is_scalar() && !a.flags.implicit_clone {
                    s = format!("({}).clone()", s);
                }
            }
            s
        })
        .collect::<Vec<_>>()
        .join(", ")
}

