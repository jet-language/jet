use crate::AST::{Expr, Item, ProgramBundle, Type, UnOp};
use crate::Codegen::Cx;
use crate::Codegen::TIR::{LowerEnv, TExpr};
use crate::Codegen::TIR::lower_expr;
use crate::Syntax;
use std::collections::HashSet;

pub(crate) fn imported_type_name(owner: &str, leaf: &str) -> String {
    format!("{owner}::{leaf}")
}

pub(crate) fn imported_type_owners(bundle: &ProgramBundle, module_idx: usize) -> Vec<String> {
    bundle
        .name_ledger
        .module_identity(module_idx)
        .into_iter()
        .collect()
}

fn module_owned_type_names(items: &[Item]) -> HashSet<String> {
    let mut names = HashSet::new();
    // `Ordering` is NOT listed. It has no source Item because it is declared
    // once per generated crate and imported into every module (`MOD_USE`), so
    // an imported `compare` return keeps the bare nominal and both sides name
    // the same Rust type. Claiming module ownership forced a
    // `__jet_<module>::__jet_Ordering` qualification that the Prelude-owned
    // `__jet_Comparable` rejects (E0053, I2).
    for item in items {
        match item {
            Item::Struct(definition) => {
                names.insert(definition.name.clone());
            }
            Item::Enum(definition) => {
                names.insert(definition.name.clone());
            }
            Item::UnitFamily(family) => {
                names.extend(family.distinct_defs().iter().map(|member| member.name.clone()));
            }
            Item::Distinct(definition) => {
                names.insert(definition.name.clone());
            }
            Item::MarkerDecl(definition) if definition.text.is_some() => {
                names.insert(definition.name.clone());
            }
            _ => {}
        }
    }
    names
}

fn module_has_nominal_type(items: &[Item], name: &str) -> bool {
    items.iter().any(|item| match item {
        Item::Struct(definition) => definition.name == name,
        Item::Enum(definition) => definition.name == name,
        Item::Distinct(definition) => definition.name == name,
        Item::UnitFamily(family) => family
            .distinct_defs()
            .iter()
            .any(|member| member.name == name),
        Item::MarkerDecl(definition) => definition.text.is_some() && definition.name == name,
        _ => false,
    })
}

fn canonical_nominal_name(
    bundle: &ProgramBundle,
    module_idx: usize,
    name: &str,
    owned: &HashSet<String>,
    seen: &mut HashSet<(usize, String)>,
) -> Option<String> {
    if name.contains("::") {
        return Some(name.to_string());
    }
    if !seen.insert((module_idx, name.to_string())) {
        return None;
    }
    if owned.contains(name) || module_has_nominal_type(&bundle.modules[module_idx].items, name) {
        return bundle.name_ledger.nominal_identity(module_idx, name);
    }
    if let Some((namespace, leaf)) = name.rsplit_once('.') {
        let target = bundle
            .name_ledger
            .effective_alias(module_idx, namespace)
            .and_then(|alias| alias.target_module)?;
        return canonical_nominal_name(bundle, target, leaf, &HashSet::new(), seen);
    }
    let alias = bundle.name_ledger.effective_alias(module_idx, name)?;
    let target = alias.target_module?;
    let leaf = alias
        .target
        .rsplit_once('.')
        .map_or(alias.target.as_str(), |(_, leaf)| leaf);
    canonical_nominal_name(bundle, target, leaf, &HashSet::new(), seen)
}

fn qualify_imported_nominal_name(
    bundle: &ProgramBundle,
    target: usize,
    name: &str,
    owned: &HashSet<String>,
) -> String {
    canonical_nominal_name(bundle, target, name, owned, &mut HashSet::new())
        .unwrap_or_else(|| name.to_string())
}

fn rewrite_apply_heads(ty: &Type, qualify: &impl Fn(&str) -> String) -> Type {
    match ty {
        Type::Apply { name, args } => Type::Apply {
            name: qualify(name),
            args: args
                .iter()
                .map(|arg| rewrite_apply_heads(arg, qualify))
                .collect(),
        },
        Type::List(inner) => Type::List(Box::new(rewrite_apply_heads(inner, qualify))),
        Type::Map {
            key,
            key_span,
            value,
        } => Type::Map {
            key: Box::new(rewrite_apply_heads(key, qualify)),
            key_span: *key_span,
            value: Box::new(rewrite_apply_heads(value, qualify)),
        },
        Type::Shared(inner) => Type::Shared(Box::new(rewrite_apply_heads(inner, qualify))),
        Type::Option(inner) => Type::Option(Box::new(rewrite_apply_heads(inner, qualify))),
        Type::Result { ok, err } => Type::Result {
            ok: Box::new(rewrite_apply_heads(ok, qualify)),
            err: Box::new(rewrite_apply_heads(err, qualify)),
        },
        Type::Fn {
            params,
            ret,
            effect_bound,
            param_contract,
            return_view_provenance,
            call_metadata,
        } => Type::Fn {
            params: params
                .iter()
                .map(|param| rewrite_apply_heads(param, qualify))
                .collect(),
            ret: ret
                .as_ref()
                .map(|ret| Box::new(rewrite_apply_heads(ret, qualify))),
            effect_bound: effect_bound.clone(),
            param_contract: param_contract.clone(),
            return_view_provenance: return_view_provenance.clone(),
            call_metadata: call_metadata.clone(),
        },
        Type::Tuple(fields) => Type::Tuple(
            fields
                .iter()
                .map(|(name, ty)| {
                    (name.clone(), Box::new(rewrite_apply_heads(ty, qualify)))
                })
                .collect(),
        ),
        Type::FixedList { elem, len } => Type::FixedList {
            elem: Box::new(rewrite_apply_heads(elem, qualify)),
            len: len.clone(),
        },
        Type::Tagged { marker, inner } => Type::Tagged {
            marker: marker.clone(),
            inner: Box::new(rewrite_apply_heads(inner, qualify)),
        },
        Type::Union(members) => crate::AST::canonicalize_union(
            members
                .iter()
                .map(|member| rewrite_apply_heads(member, qualify))
                .collect(),
        ),
        Type::Quantity { base, dimension } => Type::Quantity {
            base: Box::new(rewrite_apply_heads(base, qualify)),
            dimension: dimension.clone(),
        },
        Type::InlineRange { base, lo, hi } => Type::InlineRange {
            base: Box::new(rewrite_apply_heads(base, qualify)),
            lo: *lo,
            hi: *hi,
        },
        _ => ty.clone(),
    }
}

/// Keep every nominal reference owned by an imported module under its canonical
/// module identity. This is used by both field-shape registration and
/// cross-module call metadata, so nested/generic references share one key.
pub(crate) fn qualify_imported_type(
    bundle: &ProgramBundle,
    target: usize,
    _owner: &str,
    ty: &Type,
) -> Type {
    let owned = module_owned_type_names(&bundle.modules[target].items);
    let mapped = ty.map_named_types(&|name| {
        let qualified = qualify_imported_nominal_name(bundle, target, name, &owned);
        (qualified != name).then_some(qualified)
    });
    rewrite_apply_heads(&mapped, &|name| {
        qualify_imported_nominal_name(bundle, target, name, &owned)
    })
}

/// Register imported struct shapes under canonical nominal identities. A
/// leaf-only table cannot distinguish two imported modules that export the same
/// nominal.
pub(crate) fn register_imported_struct_shapes(
    cx: &mut Cx,
    bundle: &ProgramBundle,
    module_idx: usize,
) {
    let module = &bundle.modules[module_idx];
    let mut imported = Vec::<(usize, String)>::new();
    for import in &module.imports {
        if import.is_c_import().unwrap_or_else(|error| {
            unreachable!("invalid foreign import reached codegen: {}", error.path)
        }) {
            continue;
        }
        let Some(target) = bundle.name_ledger.import_target(module_idx, import.span) else {
            continue;
        };
        for item in &bundle.modules[target].items {
            if let Item::Struct(definition) = item {
                if bundle.name_ledger.visible(module_idx, target, &definition.name) {
                    imported.push((target, definition.name.clone()));
                }
            }
        }
    }
    imported.extend(crate::Codegen::Imports::selective_nominal_targets(
        bundle,
        module_idx,
    ));
    for ((_, _), (rust_mod, _)) in
        crate::Codegen::Imports::reexport_call_map(bundle, module_idx)
    {
        let Some(target) = bundle
            .modules
            .iter()
            .position(|candidate| crate::Codegen::mangle(&candidate.alias) == rust_mod)
        else {
            continue;
        };
        for item in &bundle.modules[target].items {
            if let Item::Struct(definition) = item {
                if bundle.name_ledger.visible(module_idx, target, &definition.name) {
                    imported.push((target, definition.name.clone()));
                }
            }
        }
    }
    imported.sort();
    imported.dedup();
    for (target, definition_name) in imported {
        let Some(definition) = bundle.modules[target].items.iter().find_map(|item| match item {
            Item::Struct(definition) if definition.name == definition_name => Some(definition),
            _ => None,
        }) else {
            continue;
        };
        let owner = bundle
            .name_ledger
            .module_identity(target)
            .expect("name ledger must contain every loaded module");
        let qualified = imported_type_name(&owner, &definition.name);
        let rust_mod = crate::Codegen::mangle(&bundle.modules[target].alias);
        let target_type_names: HashSet<String> = bundle.modules[target]
            .items
            .iter()
            .flat_map(|item| match item {
                Item::Struct(definition) => vec![definition.name.clone()],
                Item::Enum(definition) => vec![definition.name.clone()],
                Item::UnitFamily(family) => family
                    .distinct_defs()
                    .iter()
                    .map(|member| member.name.clone())
                    .collect(),
                Item::Distinct(definition) => vec![definition.name.clone()],
                Item::MarkerDecl(definition) if definition.text.is_some() => {
                    vec![definition.name.clone()]
                }
                _ => Vec::new(),
            })
            .collect();
        let fields = definition
            .reflection_fields()
            .map(|field| {
                (
                    field.name.clone(),
                    qualify_imported_type(bundle, target, &owner, &field.ty),
                )
            })
            .collect::<Vec<(String, Type)>>();
        let reflection_fields = jet_foundation::Reflection::fields(definition)
            .into_iter()
            .map(|mut field| {
                field.ty = qualify_imported_type(bundle, target, &owner, &field.ty);
                field
            })
            .collect::<Vec<_>>();
        cx.type_names.insert(qualified.clone());
        cx.foreign_types.insert(qualified.clone(), rust_mod.clone());
        cx.struct_fields.insert(qualified.clone(), fields.clone());
        let computed = definition
            .fields
            .iter()
            .filter(|field| field.computed.is_some())
            .map(|field| field.name.clone())
            .collect::<HashSet<_>>();
        if !computed.is_empty() {
            cx.computed_fields.insert(qualified.clone(), computed);
        }
        let (memo_fields, memo_dependencies) =
            crate::Codegen::Context::memo_facts_for_struct(definition);
        if !memo_fields.is_empty() {
            cx.memo_fields.insert(qualified.clone(), memo_fields);
        }
        if !memo_dependencies.is_empty() {
            cx.memo_dependencies
                .insert(qualified.clone(), memo_dependencies);
        }
        cx.reflection_fields
            .insert(qualified.clone(), reflection_fields);
        if crate::Codegen::type_is_cloneable_struct(definition, &target_type_names) {
            cx.cloneable.insert(qualified.clone());
        }
        if !definition.type_params.is_empty() {
            let params = definition
                .type_params
                .iter()
                .map(|param| param.name.clone())
                .collect::<Vec<_>>();
            cx.struct_type_params
                .insert(qualified.clone(), params.iter().cloned().collect());
            cx.struct_type_param_order
                .insert(qualified.clone(), params);
        }
    }
}

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
        Expr::Ident(name, _) => TPlace::Local(
            cx.persistent_local(name)
                .unwrap_or_else(|| env.local_of(name)),
        ),
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

/// c109 Phase 15: the PLAIN Rust field name for a CORE-struct field read, keyed on the
/// RESOLVED receiver type (the TIR's total `recv.ty`) instead of `expr_jet_ty(env)`.
/// Returns `Some(plain_name)` for a known core-struct field (so it is emitted
/// unprefixed, B2), `None` otherwise (the caller falls back to `mangle(member)`).
pub(crate) fn core_struct_field_rust_name(cx: &Cx, recv_ty: &Type, member: &str) -> Option<String> {
    if let Type::Apply { name, .. } = recv_ty {
        if name == "DataJoin"
            && !cx.type_names.contains(name)
            && matches!(member, "left" | "right")
        {
            return Some(member.to_string());
        }
        if name == "Rotation"
            && !cx.type_names.contains(name)
            && matches!(member, "previous" | "current")
        {
            return Some(member.to_string());
        }
        if name == "VjpRun"
            && !cx.type_names.contains(name)
            && matches!(member, "value" | "pull" | "grads")
        {
            return Some(member.to_string());
        }
        return None;
    }
    let Type::Named(type_name) = recv_ty else {
        return None;
    };
    if type_name == "VjpRun"
        && !cx.type_names.contains(type_name)
        && matches!(member, "value" | "pull" | "grads")
    {
        return Some(member.to_string());
    }
    // User structs named Point/Rect/Size keep `__jet_<field>` lowering.
    let ui_name_collision = matches!(
        type_name.as_str(),
        "Point"
            | "Size"
            | "Rect"
            | "SizeConstraint"
            | "UiNode"
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
        // `Err` and `GameFrame` are Prelude-owned carriers.  Their Rust
        // fields stay plain even when a lowered expression has already been
        // mapped to the carrier's Rust name.
        n if n == Syntax::TYPE_ERR || n == "JetErr" => {
            matches!(member, "message" | "code" | "cause")
        }
        n if n == Syntax::TYPE_ALLOC_ERROR => {
            matches!(member, "requested_bytes" | "allocator")
        }
        "ProcessResult" | "ProcessReceipt" => matches!(
            member,
            "code"
                | "output"
                | "errors"
                | "success"
                | "signal"
                | "timed_out"
                | "executable_identity"
                | "input_digest"
                | "argv"
                | "policy_digest"
                | "backend"
                | "authority"
                | "descendants"
                | "limits"
                | "outputs"
                | "redacted"
                | "pid"
                | "limit_hit"
        ),
        "ProcessPlan" => matches!(
            member,
            "executable_identity"
                | "argv"
                | "input_digest"
                | "policy_digest"
                | "backend"
                | "authority"
                | "descendants"
                | "limits"
                | "outputs"
        ),
        // D-PROCESS1=A: `child.stdin`/`.stdout`/`.stderr` read the real
        // `ProcessChild` Rust struct field directly (a writer/reader handle),
        // not a `__jet_<field>` name.
        "ProcessChild" => matches!(member, "stdin" | "stdout" | "stderr" | "terminal"),
        "TerminalSize" => matches!(member, "cols" | "rows"),
        "TerminalPolicy" => matches!(member, "size" | "mode"),
        "TestSuite" => matches!(member, "iteration" | "result"),
        "Range" => matches!(member, "start" | "end" | "exclusive"),
        "DimensionAxis" => matches!(member, "name" | "exponent"),
        "DimensionInfo" => matches!(member, "axes" | "identity" | "display"),
        "StateRef" => matches!(member, "owner" | "name" | "path"),
        "StateInfo" => matches!(member, "name" | "path"),
        "EffectInfo" => member == "values",
        "TrackOriginInfo" => matches!(member, "tracked" | "source"),
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
        "HTTPShutdownReport" => {
            matches!(member, "accepted" | "overloaded" | "completed" | "cancelled")
        }
        "TLSPeerIdentity" => {
            matches!(member, "verified_server_name" | "leaf" | "certificate_chain" | "cipher_suite" | "tls_version")
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
        "GameFrame" | "JetGameFrame" => matches!(member, "index" | "input"),
        n if n == Syntax::TYPE_MEMO_STATS => {
            matches!(member, "hits" | "misses" | "size" | "bound")
        }
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
        "Claims" => matches!(member, "subject" | "audience" | "issuer" | "expires_at" | "not_before" | "issued_at"),
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
        if type_name == "HTTPShutdownReport" {
            return Some(format!("user_{member}"));
        }
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
    // Card 2021: a CORE record's field types are NOT restated here. This
    // function used to carry a hand-kept ladder of 41 core structs, while sema
    // declared 115 — so every field of the other 74 (`ProcessResult` among
    // them) resolved through the caller's `.unwrap_or(Type::Int)` and print
    // picked the INTEGER accessor for a `String`. Reading the declaring table
    // instead makes the answer total for every core struct at once, which is
    // what a site-local repair of one field could never do.
    //
    // Precedence mirrors `Checker::field_type` (CheckerInfer/expr.rs) exactly:
    // a user struct claiming the name wins (D-SHIFT1 user-type-wins), and only
    // then does the reserved core shape answer. Codegen must agree with the
    // types sema already committed to; disagreeing is how rustc gets handed
    // Jet's own ill-typed output, which is an internal compiler error (I2).
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
        // A reserved core GENERIC (`DataJoin<L, R>`, `VjpRun<T>`, `Rotation<T>`)
        // resolves its field
        // against its type arguments, so a chained access (`r.migration
        // .migrated`) types the intermediate instead of mis-mangling the next
        // field.
        return crate::Sema::core_struct_field_type(name, field, args);
    }
    let Type::Named(name) = recv_ty else {
        return None;
    };
    if let Some(field_ty) = cx
        .struct_fields
        .get(name)
        .and_then(|fields| fields.iter().find(|(f, _)| f == field))
        .map(|(_, t)| t.clone())
    {
        return Some(field_ty);
    }
    crate::Sema::core_struct_field_type(name, field, &[])
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
        // D-TYPE2-IMAG1=A: a folded Complex still enters the canonical
        // precise constructor seam. This keeps AOT, resident JIT, and web
        // from treating its CtValue struct as an unrelated user record.
        crate::AST::CtValue::Struct { type_name, fields }
            if type_name == Syntax::TYPE_COMPLEX => {
            let part = |name: &str| {
                fields
                    .iter()
                    .find(|(field, _)| field == name)
                    .and_then(|(_, value)| match value {
                        crate::AST::CtValue::Int(value) => Some(*value as f64),
                        crate::AST::CtValue::Float(value) => Some(value.as_f64()),
                        _ => None,
                    })
            };
            let real = part("real")?;
            let imaginary = part("imaginary")?;
            Some(TExprKind::PreciseBuiltin {
                type_name: Syntax::TYPE_COMPLEX.to_string(),
                func: "from_parts".to_string(),
                args: vec![
                    TExpr {
                        ty: Type::Float,
                        kind: TExprKind::FloatLit(real),
                    },
                    TExpr {
                        ty: Type::Float,
                        kind: TExprKind::FloatLit(imaginary),
                    },
                ],
            })
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
            let actual_ty = if actual.widen_to_vec {
                match &actual.value.ty {
                    Type::FixedList { elem, .. } => Type::List(elem.clone()),
                    other => other.clone(),
                }
            } else {
                actual.value.ty.clone()
            };
            if !crate::Codegen::TIR::bind_generic_type(
                template,
                &actual_ty,
                params,
                &mut subst,
            ) {
                return declared;
            }
        }
    }
    crate::Generics::substitute_type(&declared, &subst)
}
