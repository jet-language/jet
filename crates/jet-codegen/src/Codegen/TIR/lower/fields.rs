use crate::AST::{Expr, Type, UnOp};
use crate::Codegen::Cx;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::lower_expr;
use crate::Syntax;

/// D-INCR1: the structured place `++`/`--` reads and updates. A bare identifier
/// resolves to its slot; anything else is the already-structured place expression
/// the operand lowers to.
pub(super) fn lower_incdec_place(
    operand: &Expr,
    cx: &Cx,
    env: &mut LowerEnv,
) -> crate::Codegen::TIR::TPlace {
    use crate::Codegen::TIR::TPlace;
    match operand {
        Expr::Ident(name, _) => TPlace::Local(env.local_of(name)),
        other => TPlace::Expr(Box::new(lower_expr(other, cx, env))),
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
            | "DataLineOptions"
            | "DataPivotCell"
            | "DataLimits"
            | "DataError"
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
        "ProcessChild" => matches!(member, "stdin" | "stdout" | "stderr" | "terminal"),
        "TerminalSize" => matches!(member, "cols" | "rows"),
        "TerminalPolicy" => matches!(member, "size" | "mode"),
        "Range" => matches!(member, "start" | "end" | "exclusive"),
        n if n == Syntax::TYPE_JSON_ERROR || n == "JSONError" => {
            matches!(member, "line" | "message")
        }
        n if n == Syntax::TYPE_UTF8_ERROR || n == "UTF8Error" => member == "message",
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
        // D-DATA-SURFACE1=A / D-DATA-STATUS1=A / D-DATA-PLOT1=A: core.data fields
        // use plain Rust names.
        "DataGroup" => matches!(member, "key" | "count" | "sum" | "mean"),
        "DataLineOptions" => matches!(
            member,
            "title" | "x_label" | "y_label" | "markers" | "reference" | "style"
                | "color" | "legend"
        ),
        "DataPivotCell" => matches!(member, "row_key" | "column_key" | "count" | "sum" | "mean"),
        "DataLimits" => matches!(
            member,
            "encoding" | "max_groups" | "max_sort_rows" | "max_join_rows" | "max_output_rows"
        ),
        "DataError" => matches!(
            member,
            "kind" | "operation" | "row" | "column" | "index" | "reason" | "cause"
        ),
        "DataColumn" => matches!(member, "name" | "type_name"),
        "DataStatus" => matches!(
            member,
            "step" | "path" | "copy" | "ownership" | "trust" | "fallback" | "replacement"
        ),
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
        // E2-M10: HTTPRequest / HTTPResponse field access.
        "HTTPRequest" | "HTTPResponse" => {
            matches!(member, "method" | "path" | "body" | "headers" | "status")
        }
        "TLSPeerIdentity" => {
            matches!(member, "verified_server_name" | "leaf" | "certificate_chain")
        }
        "TLSCertificate" => matches!(
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
        "Session" => matches!(member, "id" | "user_id" | "expires_at" | "cookie"),
        "Auth" => matches!(member, "users_table"),
        n if n == Syntax::TYPE_TYPE_INFO => {
            matches!(member, "layout")
        }
        n if n == Syntax::TYPE_LAYOUT_INFO => matches!(
            member,
            "kind" | "size" | "alignment" | "stride" | "target" | "guarantee" | "source" | "fields"
        ),
        n if n == Syntax::TYPE_LAYOUT_FIELD => {
            matches!(member, "name" | "ty" | "offset" | "size" | "target" | "guarantee" | "source")
        }
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
    // D-PIN2=A / D-PIN3=A: a pin is a window onto a place, so reaching a field
    // through `Pin<T>` resolves against `T`. The field's own declared type is
    // the mark: a `Pin<U>` field comes back as `Pin<U>` and stays a window.
    if let Type::Apply { name, args } = recv_ty {
        if name == crate::Syntax::TYPE_PIN && args.len() == 1 {
            return struct_field_type(cx, &args[0], field);
        }
    }
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
    if name == "TLSPeerIdentity" && !cx.struct_fields.contains_key(name) {
        return match field {
            "verified_server_name" => Some(Type::String),
            "leaf" => Some(Type::Named("TLSCertificate".to_string())),
            "certificate_chain" => Some(Type::List(Box::new(Type::Named(
                "TLSCertificate".to_string(),
            )))),
            _ => None,
        };
    }
    if name == "TLSCertificate" && !cx.struct_fields.contains_key(name) {
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
    // D-WATCH-SCOPE1: WatchEvent is a reserved core struct (not a user Item::Struct).
    if name == "WatchEvent" && !cx.struct_fields.contains_key(name) {
        return match field {
            "domain" | "kind" | "path" | "detail" => Some(Type::String),
            "pid" | "port" => Some(Type::Int),
            _ => None,
        };
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
    if name == "DataLineOptions" && !cx.struct_fields.contains_key(name) {
        return match field {
            "title" | "x_label" | "y_label" | "style" | "color" | "legend" => {
                Some(Type::String)
            }
            "markers" => Some(Type::Bool),
            "reference" => Some(Type::Option(Box::new(Type::Float))),
            _ => None,
        };
    }
    if name == "DataPivotCell" && !cx.struct_fields.contains_key(name) {
        return match field {
            "row_key" | "column_key" => Some(Type::String),
            "count" => Some(Type::Int),
            "sum" | "mean" => Some(Type::Float),
            _ => None,
        };
    }
    if name == "DataLimits" && !cx.struct_fields.contains_key(name) {
        return match field {
            "encoding" => Some(Type::Named("EncodingLimits".to_string())),
            "max_groups" | "max_sort_rows" | "max_join_rows" | "max_output_rows" => {
                Some(Type::Int)
            }
            _ => None,
        };
    }
    if name == "DataError" && !cx.struct_fields.contains_key(name) {
        return match field {
            "kind" => Some(Type::Named("DataErrorKind".to_string())),
            "operation" | "reason" => Some(Type::String),
            "row" | "column" | "index" => Some(Type::Option(Box::new(Type::Int))),
            "cause" => Some(Type::Option(Box::new(Type::Named(
                "EncodingError".to_string(),
            )))),
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
            "step" | "path" | "copy" | "ownership" | "trust" | "fallback" | "replacement" => {
                Some(Type::String)
            }
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
    if name == Syntax::TYPE_TYPE_INFO {
        return match field {
            "layout" => {
                Some(Type::Named(Syntax::TYPE_LAYOUT_INFO.to_string()))
            }
            _ => None,
        };
    }
    if name == Syntax::TYPE_LAYOUT_INFO {
        return match field {
            "kind" | "target" | "guarantee" | "source" => Some(Type::String),
            "size" | "alignment" | "stride" => Some(Type::Option(Box::new(Type::Int))),
            "fields" => Some(Type::List(Box::new(Type::Named(
                Syntax::TYPE_LAYOUT_FIELD.to_string(),
            )))),
            _ => None,
        };
    }
    if name == Syntax::TYPE_LAYOUT_FIELD {
        return match field {
            "name" | "ty" | "target" | "guarantee" | "source" => Some(Type::String),
            "offset" | "size" => Some(Type::Option(Box::new(Type::Int))),
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

pub(crate) fn let_ty_for_opt(
    ty: Option<&Type>,
    cx: &Cx,
    mut_fn: bool,
    is_resource: bool,
    gc: bool,
) -> crate::Codegen::TIR::TLetTy {
    use crate::Codegen::TIR::{TLetTy, TLetWrapper};
    let Some(ty) = ty else {
        return TLetTy::Inferred;
    };
    if is_resource {
        return TLetTy::resource(ty.clone());
    }
    if gc {
        return TLetTy::automatic_root(ty.clone());
    }
    if let Type::Fn { .. } = ty {
        return TLetTy::of(ty.clone(), mut_fn, TLetWrapper::None);
    }
    let _ = cx;
    TLetTy::plain(ty.clone())
}

pub(crate) fn let_ty_tuple(types: Vec<Type>) -> crate::Codegen::TIR::TLetTy {
    crate::Codegen::TIR::TLetTy::Tuple(types)
}

/// A comptime scalar as a structured literal node. Sema already folded the
/// value, so the scalar cases carry the number/text/flag/char itself instead of the
/// rendered Rust text — every engine reads the fact, and emit still renders the
/// same bytes `CtValue::serialize` would have produced.
pub(crate) fn lower_comptime_scalar(
    value: Option<&crate::AST::CtValue>,
    ty: Option<&Type>,
) -> Option<crate::Codegen::TIR::TExprKind> {
    use crate::Codegen::TIR::{TExprKind, TStrPart};
    match value? {
        crate::AST::CtValue::Int(int) => Some(TExprKind::IntLit(
            *int,
            match ty {
                Some(Type::IntN { signed, bits }) => Some((*signed, *bits)),
                _ => None,
            },
        )),
        crate::AST::CtValue::Float(float) => Some(TExprKind::FloatLit(float.as_f64())),
        crate::AST::CtValue::Bool(flag) => Some(TExprKind::BoolLit(*flag)),
        crate::AST::CtValue::Char(ch) => Some(TExprKind::CharLit(*ch)),
        crate::AST::CtValue::Str(text) => {
            Some(TExprKind::StrLit(vec![TStrPart::Lit(text.clone())]))
        }
        _ => None,
    }
}

/// The resolved return type of a called plain function: its declared return
/// type if known, else `Unit`. (In the subset, callees return scalar/String/Unit.)
/// Read from `cx.fn_types`, which sema-built `Type::Fn { ret, .. }` per function.
pub(crate) fn call_return_type(cx: &Cx, name: &str) -> Type {
    match cx.fn_types.get(name) {
        Some(Type::Fn { ret: Some(r), .. }) => cx.expand_type_aliases(r),
        // c109 Phase 23: a distinct-type constructor `UserId(x)` yields the distinct
        // type itself (it has no `fn_types` entry). Keeps the call's result type total.
        _ if cx.distinct_types.contains_key(name) => Type::Named(name.to_string()),
        _ => unit_type(),
    }
}

/// Resolve a generic call's result using the explicit arguments first, then
/// the concrete lowered argument types. This is the codegen-side mirror of
/// sema's substitution; engines receive a concrete TIR type, never a binder.
pub(crate) fn call_return_type_with_args(
    cx: &Cx,
    name: &str,
    type_args: &[Type],
    args: &[crate::Codegen::TIR::TCallArg],
) -> Type {
    let declared = call_return_type(cx, name);
    let Some(params) = cx.fn_type_params.get(name) else {
        return declared;
    };
    if params.is_empty() {
        return declared;
    }
    let Some(order) = cx.fn_type_param_order.get(name) else {
        return declared;
    };
    let mut subst = std::collections::HashMap::new();
    for (param, actual) in order.iter().zip(type_args) {
        subst.insert(param.clone(), cx.expand_type_aliases(actual));
    }
    if let Some(sig) = cx.sigs.get(name) {
        for ((_, template), actual) in sig.iter().zip(args) {
            if !crate::Codegen::TIR::bind_generic_type(
                template,
                &actual.value.ty,
                params,
                &mut subst,
            ) {
                return declared;
            }
        }
    }
    crate::Generics::substitute_type(&declared, &subst)
}
