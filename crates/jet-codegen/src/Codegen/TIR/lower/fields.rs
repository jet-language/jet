use crate::AST::{Expr, Type, UnOp};
use crate::Codegen::Cx;
use crate::Codegen::TIR::emit_tir_expr;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::lower_expr;
use crate::Syntax;

/// D-INCR1: Rust place string for `++`/`--` read/update on an lvalue operand.
pub(super) fn lower_incdec_place(operand: &Expr, cx: &Cx, env: &mut LowerEnv) -> String {
    match operand {
        Expr::Ident(name, _) => env.place_of(name),
        Expr::Field(base, field, span) => {
            let field_expr = Expr::Field(base.clone(), field.clone(), *span);
            emit_tir_expr(&lower_expr(&field_expr, cx, env), cx)
        }
        other => emit_tir_expr(&lower_expr(other, cx, env), cx),
    }
}

/// Replay codegen's `operand_is_integer` (Codegen/Expression.rs) on an AST
/// operand, using the lowering env for identifier types. The result MUST match
/// that function bit-for-bit so the TIR's overflow-trap decision is identical to
/// the AST path's. Like the original: literals/negation/nested-arithmetic-left
/// resolve structurally; an `Ident` resolves via its slot type; everything else
/// (notably a struct-field read) is unresolved (`None`) and so never traps.
pub(crate) fn ast_operand_is_integer(e: &Expr, env: &LowerEnv) -> Option<bool> {
    match e {
        Expr::Int(..) => Some(true),
        Expr::Float(..) => Some(false),
        Expr::Unary(UnOp::Neg, inner, _) => ast_operand_is_integer(inner, env),
        Expr::Binary(_, l, _, _) => ast_operand_is_integer(l, env),
        // Mirror `expr_jet_ty`: only `Ident`/`Str`/`Char` resolve here. A `Field`
        // (and anything else) resolves to `None` — exactly as the AST path does,
        // so a field operand never enables the overflow trap.
        Expr::Ident(name, _) => env.ty_of(name).map(|t| t.is_integer()),
        Expr::Str(..) => Some(false),
        Expr::Char(..) => Some(false),
        _ => None,
    }
}

/// c109 Phase 15: the PLAIN Rust field name for a CORE-struct field read, mirroring
/// `core_struct_field_rust_name` (Source/Codegen/Expression.rs) — but keyed on the
/// RESOLVED receiver type (the TIR's total `recv.ty`) instead of `expr_jet_ty(env)`.
/// Returns `Some(plain_name)` for a known core-struct field (so it is emitted
/// unprefixed, B2), `None` otherwise (the caller falls back to `mangle(member)`).
pub(crate) fn core_struct_field_rust_name(cx: &Cx, recv_ty: &Type, member: &str) -> Option<String> {
    // D-MIGRATE3=A: `DecodeResult<T>` is the one reserved core struct with a
    // generic type argument (`Type::Apply`, not `Type::Named`) — handle it
    // before the `Type::Named`-only path below. User-type-wins (D-SHIFT1
    // precedent): a user struct named `DecodeResult` shadows the core one.
    if let Type::Apply { name, .. } = recv_ty {
        if name == "DataJoin"
            && !cx.type_names.contains(name)
            && matches!(member, "left" | "right")
        {
            return Some(member.to_string());
        }
        if name == "DecodeResult"
            && !cx.type_names.contains(name)
            && matches!(member, "value" | "migration")
        {
            return Some(member.to_string());
        }
        if name == "Rotation"
            && !cx.type_names.contains(name)
            && matches!(member, "previous" | "current")
        {
            return Some(member.to_string());
        }
        return None;
    }
    let Type::Named(type_name) = recv_ty else {
        return None;
    };
    // User structs named Point/Rect/Size/MigrationStatus keep `user_<field>`
    // lowering (c133 M1 precedent; D-MIGRATE3=A extends it to `MigrationStatus`).
    let ui_name_collision = matches!(
        type_name.as_str(),
        "Point"
            | "Size"
            | "Rect"
            | "SizeConstraint"
            | "UiNode"
            | "MigrationStatus"
            | "DataGroup"
            | "DataColumn"
            | "DataStatus"
            | "DataSummary"
            | "Claims"
    );
    if ui_name_collision && cx.type_names.contains(type_name) {
        return None;
    }
    let known = match type_name.as_str() {
        "ProcessResult" => matches!(
            member,
            "code" | "output" | "errors" | "success" | "signal" | "timed_out"
        ),
        // D-PROCESS1=A: `child.stdin`/`.stdout`/`.stderr` read the real
        // `ProcessChild` Rust struct field directly (a writer/reader handle),
        // not a `user_<field>` name.
        "ProcessChild" => matches!(member, "stdin" | "stdout" | "stderr"),
        n if n == Syntax::TYPE_JSON_ERROR || n == "JsonError" => {
            matches!(member, "line" | "message")
        }
        n if n == Syntax::TYPE_UTF8_ERROR || n == "Utf8Error" => member == "message",
        n if n == Syntax::TYPE_IO_CONTEXT => Syntax::IO_CONTEXT_FIELDS.contains(&member),
        // D-LSDIR1=A: DirEntry fields — name (bare filename), path (full path), is_dir.
        "DirEntry" => matches!(member, "name" | "path" | "is_dir"),
        // D-FSOPS1/D-WATCH-SCOPE1: core filesystem/watch structs use plain Rust fields.
        "Stat" => matches!(
            member,
            "size"
                | "modified_ms"
                | "created_ms"
                | "readonly"
                | "is_file"
                | "is_dir"
                | "is_symlink"
                | "kind"
        ),
        "WalkEntry" => matches!(member, "path" | "relative" | "is_dir" | "depth"),
        "TempDir" | "TempFile" | "FileLock" => member == "path",
        "WatchEvent" => matches!(
            member,
            "domain" | "kind" | "path" | "detail" | "pid" | "port"
        ),
        // D-DATA-SURFACE1=A / D-DATA-STATUS1=A: core.data fields use plain Rust names.
        "DataGroup" => matches!(member, "key" | "count" | "sum" | "mean"),
        "DataColumn" => matches!(member, "name" | "type_name"),
        "DataStatus" => matches!(member, "step" | "path" | "replacement"),
        "DataSummary" => matches!(
            member,
            "count" | "sum" | "mean" | "min" | "max" | "median" | "variance" | "stddev"
        ),
        // D-RENDERTGT2=A (c133 M1): UI geometry fields.
        "Point" => matches!(member, "x" | "y"),
        "Size" => matches!(member, "width" | "height"),
        "Rect" => matches!(member, "x" | "y" | "width" | "height"),
        "SizeConstraint" => {
            matches!(
                member,
                "min_width" | "min_height" | "max_width" | "max_height"
            )
        }
        "UiNode" => matches!(member, "label" | "width" | "height"),
        // E2-M10: HttpRequest / HttpResponse field access.
        "HttpRequest" | "HttpResponse" => {
            matches!(member, "method" | "path" | "body" | "headers" | "status")
        }
        "TlsPeerIdentity" => {
            matches!(member, "verified_server_name" | "leaf" | "certificate_chain")
        }
        "TlsCertificate" => matches!(
            member,
            "der"
                | "sha256"
                | "spki_sha256"
                | "dns_names"
                | "valid_from_unix_ms"
                | "valid_until_unix_ms"
                | "subject"
                | "issuer"
        ),
        "GameScene" => matches!(member, "assets" | "input"),
        "GameFrame" => matches!(member, "index" | "input"),
        // D-MIGRATE3=A: `MigrationStatus` — `.migrated`/`.from`/`.steps`.
        "MigrationStatus" => matches!(member, "migrated" | "from" | "steps"),
        "DecodeError" => matches!(member, "path" | "reason"),
        "FieldError" => matches!(member, "path" | "reason"),
        "EncodingLimits" => matches!(member, "buffer_bytes" | "max_depth" | "max_item_bytes" | "max_total_bytes" | "max_expansion_depth" | "max_expansion_bytes"),
        "EncodingCause" => matches!(member, "kind" | "os_code" | "message"),
        "EncodingError" => matches!(member, "format" | "kind" | "byte_offset" | "line" | "column" | "path" | "reason" | "cause"),
        "CBOROptions" => matches!(member, "max_depth" | "max_items" | "max_bytes" | "require_canonical"),
        "CBORError" => matches!(member, "kind" | "byte_offset" | "path" | "reason"),
        "XMLLimits" => matches!(member, "max_depth" | "max_nodes" | "max_attributes_per_element" | "max_name_bytes" | "max_text_bytes" | "max_entity_declarations" | "max_entity_depth" | "max_entity_replacement_bytes"),
        "XMLParseOptions" => matches!(member, "entities" | "limits"),
        "XMLRenderOptions" => matches!(member, "encoding" | "lexical"),
        "XMLCanonical" => matches!(member, "mode" | "comments" | "inclusive_prefixes"),
        "XMLError" => matches!(member, "kind" | "byte_offset" | "line" | "column" | "path" | "reason"),
        "Envelope" => matches!(member, "from" | "recipients"),
        "RecipientReport" => matches!(member, "address" | "accepted" | "code" | "message"),
        "Limits" => matches!(member,
            "max_reply_line_bytes" | "max_reply_lines" | "max_capabilities" |
            "max_recipients" | "max_message_bytes" | "max_auth_challenge_bytes"),
        "SendReport" => matches!(member, "server" | "accepted" | "rejected" | "response_code" | "response" | "accepted_at"),
        "Claims" => matches!(member, "subject" | "audience" | "issuer" | "expires_at" | "issued_at"),
        _ => false,
    };
    if known {
        Some(member.to_string())
    } else {
        None
    }
}

/// Look up a field's declared type on a resolved struct receiver type. Returns
/// `None` when the receiver is not a known struct or the field is absent — both
/// impossible for a covered function (sema validated the access).
pub(crate) fn struct_field_type(cx: &Cx, recv_ty: &Type, field: &str) -> Option<Type> {
    // c109 Phase 23: a named-tuple field read (`p.x`) — resolve the field's type off
    // the `Type::Tuple` directly (a tuple has no `cx.struct_fields` entry; its struct
    // is the generated `JetTup_<hash>`). Keeps the field read's result type total.
    if let Type::Tuple(fields) = recv_ty {
        return fields
            .iter()
            .find(|(f, _)| f == field)
            .map(|(_, t)| (**t).clone());
    }
    // D-MIGRATE3=A: `DecodeResult<T>` — a reserved core generic, not a user
    // struct, so it normally has no `cx.struct_fields` entry. Mirrors
    // `core_generic_struct_field` (sema/CheckerCoreLib.rs) so a chained access
    // (`r.migration.migrated`) resolves the intermediate `.migration` type
    // instead of falling back to `Type::Int` and mis-mangling the next field.
    // User-type-wins (D-SHIFT1 precedent): if the user declared their own
    // `struct DecodeResult<T>`, `cx.struct_fields` has a real entry — try that
    // first so a same-named user field always wins.
    if let Type::Apply { name, args } = recv_ty {
        if let Some(fields) = cx.struct_fields.get(name) {
            let field_ty = fields
                .iter()
                .find(|(f, _)| f == field)
                .map(|(_, t)| t.clone())?;
            let params = cx.struct_type_param_order.get(name)?;
            let subst = params
                .iter()
                .zip(args)
                .map(|(param, arg)| (param.clone(), arg.clone()))
                .collect();
            return Some(crate::Generics::substitute_type(&field_ty, &subst));
        }
        if name == "DecodeResult" {
            return match field {
                "value" => args.first().cloned(),
                "migration" => Some(Type::Named("MigrationStatus".to_string())),
                _ => None,
            };
        }
        if name == "DataJoin" && args.len() == 2 {
            return match field {
                "left" => Some(args[0].clone()),
                "right" => Some(args[1].clone()),
                _ => None,
            };
        }
        return None;
    }
    let Type::Named(name) = recv_ty else {
        return None;
    };
    if name == "Claims" && !cx.struct_fields.contains_key(name) {
        return match field {
            "subject" | "issuer" => Some(Type::Option(Box::new(Type::String))),
            "audience" => Some(Type::String),
            "expires_at" => Some(Type::Int),
            "issued_at" => Some(Type::Option(Box::new(Type::Int))),
            _ => None,
        };
    }
    if name == "TlsPeerIdentity" && !cx.struct_fields.contains_key(name) {
        return match field {
            "verified_server_name" => Some(Type::String),
            "leaf" => Some(Type::Named("TlsCertificate".to_string())),
            "certificate_chain" => Some(Type::List(Box::new(Type::Named(
                "TlsCertificate".to_string(),
            )))),
            _ => None,
        };
    }
    if name == "TlsCertificate" && !cx.struct_fields.contains_key(name) {
        return match field {
            "der" | "sha256" | "spki_sha256" => Some(Type::List(Box::new(Type::IntN {
                signed: false,
                bits: 8,
            }))),
            "dns_names" => Some(Type::List(Box::new(Type::String))),
            "valid_from_unix_ms" | "valid_until_unix_ms" => Some(Type::Int),
            "subject" | "issuer" => Some(Type::String),
            _ => None,
        };
    }
    // D-MIGRATE3=A: `MigrationStatus` is likewise a reserved core struct —
    // same user-type-wins order.
    if name == "MigrationStatus" && !cx.struct_fields.contains_key(name) {
        return match field {
            "migrated" => Some(Type::Bool),
            "from" => Some(Type::String),
            "steps" => Some(Type::List(Box::new(Type::String))),
            _ => None,
        };
    }
    if name == "DecodeError" && !cx.struct_fields.contains_key(name) {
        return matches!(field, "path" | "reason").then_some(Type::String);
    }
    if name == "FieldError" && !cx.struct_fields.contains_key(name) {
        return matches!(field, "path" | "reason").then_some(Type::String);
    }
    if name == "EncodingLimits" && !cx.struct_fields.contains_key(name) {
        return match field {
            "buffer_bytes" | "max_depth" | "max_item_bytes" | "max_expansion_depth" | "max_expansion_bytes" => Some(Type::Int),
            "max_total_bytes" => Some(Type::Option(Box::new(Type::Int))),
            _ => None,
        };
    }
    if name == "EncodingCause" && !cx.struct_fields.contains_key(name) {
        return match field { "kind" | "message" => Some(Type::String), "os_code" => Some(Type::Option(Box::new(Type::Int))), _ => None };
    }
    if name == "EncodingError" && !cx.struct_fields.contains_key(name) {
        return match field {
            "format" => Some(Type::Named("EncodingFormat".to_string())),
            "kind" => Some(Type::Named("EncodingErrorKind".to_string())),
            "byte_offset" => Some(Type::Int),
            "line" | "column" => Some(Type::Option(Box::new(Type::Int))),
            "path" | "reason" => Some(Type::String),
            "cause" => Some(Type::Option(Box::new(Type::Named("EncodingCause".to_string())))),
            _ => None,
        };
    }
    if name == "CBOROptions" && !cx.struct_fields.contains_key(name) {
        return match field {
            "max_depth" | "max_items" | "max_bytes" => Some(Type::Int),
            "require_canonical" => Some(Type::Bool),
            _ => None,
        };
    }
    if name == "CBORError" && !cx.struct_fields.contains_key(name) {
        return match field {
            "kind" => Some(Type::Named("CBORErrorKind".to_string())),
            "byte_offset" => Some(Type::Int),
            "path" | "reason" => Some(Type::String),
            _ => None,
        };
    }
    if name == "XMLLimits" && !cx.struct_fields.contains_key(name) {
        return matches!(field, "max_depth" | "max_nodes" | "max_attributes_per_element" | "max_name_bytes" | "max_text_bytes" | "max_entity_declarations" | "max_entity_depth" | "max_entity_replacement_bytes").then_some(Type::Int);
    }
    if name == "XMLParseOptions" && !cx.struct_fields.contains_key(name) {
        return match field {
            "entities" => Some(Type::Named("XMLEntityPolicy".to_string())),
            "limits" => Some(Type::Named("XMLLimits".to_string())),
            _ => None,
        };
    }
    if name == "XMLRenderOptions" && !cx.struct_fields.contains_key(name) {
        return match field {
            "encoding" => Some(Type::Named("XMLEncoding".to_string())),
            "lexical" => Some(Type::Named("XMLLexicalPolicy".to_string())),
            _ => None,
        };
    }
    if name == "XMLCanonical" && !cx.struct_fields.contains_key(name) {
        return match field {
            "mode" => Some(Type::Named("XMLCanonicalMode".to_string())),
            "comments" => Some(Type::Bool),
            "inclusive_prefixes" => Some(Type::List(Box::new(Type::String))),
            _ => None,
        };
    }
    if name == "XMLError" && !cx.struct_fields.contains_key(name) {
        return match field {
            "kind" => Some(Type::Named("XMLReason".to_string())),
            "byte_offset" | "line" | "column" => Some(Type::Option(Box::new(Type::Int))),
            "path" | "reason" => Some(Type::String),
            _ => None,
        };
    }
    if name == "GameScene" {
        return match field {
            "assets" => Some(Type::Named("GameAssets".to_string())),
            "input" => Some(Type::Named("GameInputMap".to_string())),
            _ => None,
        };
    }
    if name == "GameFrame" {
        return match field {
            "index" => Some(Type::Int),
            "input" => Some(Type::Named("GameInputSnapshot".to_string())),
            _ => None,
        };
    }
    if name == "DataGroup" && !cx.struct_fields.contains_key(name) {
        return match field {
            "key" => Some(Type::String),
            "count" => Some(Type::Int),
            "sum" | "mean" => Some(Type::Float),
            _ => None,
        };
    }
    if name == "DataColumn" && !cx.struct_fields.contains_key(name) {
        return match field {
            "name" | "type_name" => Some(Type::String),
            _ => None,
        };
    }
    if name == "DataStatus" && !cx.struct_fields.contains_key(name) {
        return match field {
            "step" | "path" | "replacement" => Some(Type::String),
            _ => None,
        };
    }
    if name == "DataSummary" && !cx.struct_fields.contains_key(name) {
        return match field {
            "count" => Some(Type::Int),
            "sum" | "mean" | "min" | "max" | "median" | "variance" | "stddev" => {
                Some(Type::Float)
            }
            _ => None,
        };
    }
    cx.struct_fields
        .get(name)?
        .iter()
        .find(|(f, _)| f == field)
        .map(|(_, t)| t.clone())
}

/// The type of an integer literal given its elaborated width.
pub(crate) fn int_lit_type(width: &Option<(bool, u8)>) -> Type {
    match width {
        Some((signed, bits)) => Type::IntN {
            signed: *signed,
            bits: *bits,
        },
        None => Type::Int,
    }
}

pub(crate) fn unit_type() -> Type {
    Type::Named("Unit".to_string())
}

/// The resolved return type of a called plain function: its declared return
/// type if known, else `Unit`. (In the subset, callees return scalar/String/Unit.)
/// Read from `cx.fn_types`, which sema-built `Type::Fn { ret, .. }` per function.
pub(crate) fn call_return_type(cx: &Cx, name: &str) -> Type {
    match cx.fn_types.get(name) {
        Some(Type::Fn { ret: Some(r), .. }) => (**r).clone(),
        // c109 Phase 23: a distinct-type constructor `UserId(x)` yields the distinct
        // type itself (it has no `fn_types` entry). Keeps the call's result type total.
        _ if cx.distinct_types.contains_key(name) => Type::Named(name.to_string()),
        _ => unit_type(),
    }
}
