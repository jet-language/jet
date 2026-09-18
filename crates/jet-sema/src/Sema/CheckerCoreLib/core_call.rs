use super::alloc_ptrs::{db_row_ty, e3101, io_error_ty, ptr_elem, result_ty};
use super::core_types::{decode_error_ty, encoding_error_ty, json_ty, u8_ty, unit_ty};
use super::fixed_sigs::{core_fixed_sig, core_fixed_sig_for_row};
use super::serde_diags::{
    is_no_os_forbidden, literal_string_value, module_short_name, no_os_hint, reactive_derived_unit,
    reactive_lambda_arity, reactive_not_lambda, unknown_core_item, wrong_core_arity,
};
use crate::Diagnostics::{CryptoMisuseReason, Diagnostic, Span};
use crate::Sema::Checker;
use crate::Sema::Diagnostics::{
    is_debuggable, is_displayable, is_printable, suggest_field, type_fix_hint, types_comparable,
};
use crate::Sema::Effects::{
    core_effect, core_effect_leaf, e0746, is_irreversible_effect, Effect,
};
use crate::Sema::Purity::e3401;
use crate::Sema::SendCrossing;
use crate::Sema::FFI::e3301;
use crate::Syntax;
use crate::AST::{AccessConvention, CallArg, CallArgFlags, Expr, OrFallback, ParamZone, StrPart, Type};
use jet_foundation::Effects::is_nondeterministic_core;
use jet_foundation::Game::JetGameFrameBudget;

/// The Core row owns the lookup key for plain calls. Keep the fallback for
/// polymorphic/closure forms until their projection rows land, but do not let
/// a plain row silently acquire a second effect lookup here.
fn core_effect_for_call(module: &str, name: &str) -> Option<crate::Sema::Effects::Effect> {
    match Syntax::core_call(module, name) {
        Some(row) => row.effect(),
        None => core_effect(module, name),
    }
}

fn core_effect_leaf_for_call(module: &str, name: &str) -> Option<&'static str> {
    match Syntax::core_call(module, name) {
        Some(row) => row.effect_leaf(),
        None => core_effect_leaf(module, name),
    }
}

fn core_call_is_known(module: &str, name: &str) -> bool {
    matches!(
        (module, name),
            ("core.service", "tree")
            | (
                "core.data.loader",
                "authority" | "bind" | "bind_text" | "cancel" | "invalidate" | "needs_refresh"
                    | "offline" | "ready" | "source_identity" | "status" | "stream",
            )
            | ("core.build", "graph" | "receipt_diff")
            | ("core.auth", "verify_jwt" | "verify_paseto")
            | ("core.net.tls", "client")
            | ("core.net", "unix_connect")
            | ("core.process", "run")
            | ("core.plugin", "load")
    ) || core_effect_for_call(module, name).is_some()
        || Syntax::core_call(module, name).is_some()
        || super::is_polymorphic_core_special(module, name)
        || core_fixed_sig(module, name).is_some()
        || super::core_param_contract(module, name).is_some()
        || Syntax::core_marker_application(module, name).is_some()
}
fn unit_callback_type() -> Type {
    Type::Fn {
        params: Vec::new(),
        ret: Some(Box::new(unit_ty())),
        effect_bound: None,
        return_view_provenance: None,
        param_contract: None,
        call_metadata: None,
    }
}
fn deterministic_world_callback_type() -> Type {
    Type::Fn {
        params: vec![Type::Named(
            crate::Syntax::DETERMINISTIC_WORLD_TYPE.to_string(),
        )],
        ret: Some(Box::new(unit_ty())),
        effect_bound: None,
        return_view_provenance: None,
        param_contract: None,
        call_metadata: None,
    }
}

fn data_carrier_type(name: &str, row: Type) -> Type {
    Type::Apply {
        name: name.to_string(),
        args: vec![row],
    }
}


fn data_callback_type(row: Type, ret: Type) -> Type {
    Type::Fn {
        params: vec![row],
        ret: Some(Box::new(ret)),
        effect_bound: None,
        return_view_provenance: None,
        param_contract: None,
        call_metadata: None,
    }
}
fn history_strategy_type(command: Type) -> Type {
    Type::Apply {
        name: "HistoryStrategy".to_string(),
        args: vec![command],
    }
}

fn ui_drop_callback_type() -> Type {
    Type::Fn {
        params: vec![Type::List(Box::new(Type::Named("UiDropItem".to_string())))],
        ret: Some(Box::new(unit_ty())),
        effect_bound: None,
        param_contract: None,
        call_metadata: None,
        return_view_provenance: None,
    }
}


fn analytics_sql_literal(expr: &Expr) -> Option<String> {
    let Expr::TypedLit {
        head: Some(Type::Named(head)),
        body: crate::AST::TypedLitBody::Value(value),
        ..
    } = expr
    else {
        return None;
    };
    if head != "SQL" {
        return None;
    }
    let Expr::Str(parts, _) = value.as_ref() else {
        return None;
    };
    Some(
        parts
            .iter()
            .filter_map(|part| match part {
                crate::AST::StrPart::Lit(text) => Some(text.as_str()),
                crate::AST::StrPart::Interp(..) => None,
            })
            .collect(),
    )
}

fn analytics_sql_columns(sql: &str) -> Vec<String> {
    let tokens: Vec<String> = sql
        .split_whitespace()
        .map(|token| token.trim_matches(',').to_string())
        .collect();
    let mut columns = Vec::new();
    if let (Some(select), Some(from)) = (
        tokens
            .iter()
            .position(|token| token.eq_ignore_ascii_case("select")),
        tokens
            .iter()
            .position(|token| token.eq_ignore_ascii_case("from")),
    ) {
        for token in tokens
            .iter()
            .skip(select + 1)
            .take(from.saturating_sub(select + 1))
        {
            let name = token
                .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .split('(')
                .last()
                .unwrap_or_default();
            if !name.is_empty() && name != "*" && !name.eq_ignore_ascii_case("as") {
                columns.push(name.rsplit('.').next().unwrap_or(name).to_string());
            }
        }
    }
    for keyword in ["where", "order", "group"] {
        if let Some(at) = tokens
            .iter()
            .position(|token| token.eq_ignore_ascii_case(keyword))
        {
            let offset = if keyword == "where" { 1 } else { 2 };
            if let Some(token) = tokens.get(at + offset) {
                let name = token
                    .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                    .rsplit('.')
                    .next()
                    .unwrap_or_default();
                if !name.is_empty() {
                    columns.push(name.to_string());
                }
            }
        }
    }
    columns.sort();
    columns.dedup();
    columns
}

// D-TYPE2-DEFAULT1: these functions leave the rational world. An exact value
// may cross this one explicit semantic boundary into Float; the other math
// calls keep their existing exact/inexact contracts.
//
// Both exact carriers cross, not just Fraction. `sqrt(1 / 3)` and `sqrt(0.5)`
// are equally exact inputs to a function whose result is irrational, so
// admitting one and rejecting the other was one boundary with two rules — and
// the rejected spelling is the one a beginner writes first, because a decimal
// literal is exact by default under this same decision.
fn exact_rational_math_approx(name: &str) -> bool {
    matches!(
        name,
        "sqrt"
            | "sin"
            | "cos"
            | "tan"
            | "asin"
            | "acos"
            | "atan"
            | "sinh"
            | "cosh"
            | "tanh"
            | "exp"
            | "ln"
            | "log2"
            | "log10"
            | "acosh"
            | "asinh"
            | "atanh"
            | "cbrt"
            | "exp2"
            | "exp_m1"
            | "ln_1p"
            | "degrees"
            | "radians"
    )
}

impl<'a> Checker<'a> {
    pub(crate) fn check_analytics_sql_schema(&mut self, row: &Type, query: &Expr) {
        let Some(sql) = analytics_sql_literal(query) else {
            return;
        };
        let type_name = match row {
            Type::Named(name) | Type::Apply { name, .. } => name,
            _ => return,
        };
        let Some(fields) = self.struct_fields_for_type_name(type_name) else {
            return;
        };
        let known: Vec<String> = fields.iter().map(|(name, _, _)| name.clone()).collect();
        for column in analytics_sql_columns(&sql) {
            if known.iter().any(|known| known == &column) {
                continue;
            }
            // The suggestion is embedded in `what`, while `query.span()` is
            // the whole SQL expression; there is no safe column-token span
            // for a typed edit here.
            let suggestion = suggest_field(&column, &known)
                .map(|name| format!(" Did you mean `{name}`?"))
                .unwrap_or_default();
            self.diags.push(Diagnostic::error(
                "E2420",
                format!("unknown analytics column `{column}`{suggestion}"),
                format!("`{column}` is not a field on `{type_name}`; DuckDB would fail at runtime, but Jet checks this schema at compile time"),
                format!("write one of: {}", known.join(", ")),
                Some(query.span()),
            ));
        }
    }

    /// D-QUERY-RETAIN1=A: infer a one-row query callback (`map`, `min`,
    /// `max`, `group_by`, or a grouped reducer) whose result type is open.
    /// The row type seeds the lambda parameter; the callback's own result type
    /// is returned so projections, keys, and values keep their nominal/exact
    /// types instead of a String/Float projection.
    pub(crate) fn infer_query_callback(
        &mut self,
        call_name: &str,
        row: &Type,
        arg: &mut crate::AST::CallArg,
    ) -> Option<Type> {
        let expected = Type::Fn {
            params: vec![row.clone()],
            ret: None,
            effect_bound: Some(Vec::new()),
            return_view_provenance: None,
            param_contract: None,
            call_metadata: None,
        };
        let saved_expected = self.expected_type.clone();
        self.expected_type = Some(expected.clone());
        let got = self.infer(&mut arg.expr);
        self.expected_type = saved_expected;
        match &got {
            Some(callback_ty @ Type::Fn { params, ret, .. }) => {
                self.check_query_callback_effect(
                    call_name,
                    &expected,
                    callback_ty,
                    arg.expr.span(),
                );
                if params.len() != 1 || params[0] != *row {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "`{call_name}` wants a callback over {} for argument 1",
                            row.show()
                        ),
                        "a query callback receives exactly one row".to_string(),
                        format!("write `{} -> …`", "row"),
                        Some(arg.expr.span()),
                    ));
                }
                ret.as_ref().map(|ret| (**ret).clone())
            }
            Some(other) => {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!(
                        "`{call_name}` wants a callback for argument 1, but this is {}",
                        other.show()
                    ),
                    "query reducers take one callback over the row".to_string(),
                    "pass a lambda such as `row -> row.field`".to_string(),
                    Some(arg.expr.span()),
                ));
                None
            }
            None => None,
        }
    }
    /// D-QUERY-LIVE1: maintained callbacks must prove an explicit empty
    /// effect bound. An absent bound is open, not evidence of replay safety.
    pub(crate) fn check_query_callback_effect(
        &mut self,
        call_name: &str,
        expected: &Type,
        offered: &Type,
        span: Span,
    ) -> bool {
        let requires_pure = matches!(
            call_name,
            "filter"
                | "sort_by"
                | "map"
                | "min"
                | "max"
                | "inner_join"
                | "left_join"
                | "group_by"
                | "sum"
                | "mean"
        ) && matches!(
            expected,
            Type::Fn {
                effect_bound: Some(bound),
                ..
            } if bound.is_empty()
        );
        if !requires_pure {
            return false;
        }
        let safe = matches!(
            offered,
            Type::Fn {
                effect_bound: Some(bound),
                ..
            } if bound.is_empty()
        );
        if safe {
            return false;
        }
        self.diags.push(Diagnostic::error(
            "E2476",
            format!("live query callback `{call_name}` is not replay-safe"),
            "a maintained query replays callbacks after each source delta, so the callback must have an explicit empty effect bound"
                .to_string(),
            "remove the effectful operation, or declare/use a pure callback over the tracked row"
                .to_string(),
            Some(span),
        ));
        true
    }
}

fn vault_key_arg(ty: &Type) -> Option<Type> {
    match ty {
        Type::Apply { name, args }
            if matches!(
                name.as_str(),
                "KeyRef" | "MutationPlan" | "VaultWrite" | "Rotation" | "WrappedImportPlan"
            ) =>
        {
            args.first().cloned()
        }
        Type::Named(name) if matches!(name.as_str(), "SigningKey" | "X25519SecretKey") => {
            Some(ty.clone())
        }
        Type::Tagged { inner, .. } if matches!(inner.as_ref(), Type::Named(name) if matches!(name.as_str(), "SigningKey" | "X25519SecretKey")) => {
            Some(ty.clone())
        }
        _ => None,
    }
}

fn literal_list_len(expr: &crate::AST::Expr) -> Option<usize> {
    match expr {
        crate::AST::Expr::ListLit(items, _)
            if !items
                .iter()
                .any(|item| matches!(item, crate::AST::Expr::Spread(..))) =>
        {
            Some(items.len())
        }
        // An empty typed list keeps its head after inference (`[U8]{}`).
        crate::AST::Expr::TypedLit {
            head: Some(Type::List(_) | Type::FixedList { .. }),
            body: crate::AST::TypedLitBody::Empty,
            ..
        } => Some(0),
        crate::AST::Expr::Paren(inner, _) => literal_list_len(inner),
        _ => None,
    }
}

fn known_list_len(checker: &Checker<'_>, expr: &crate::AST::Expr) -> Option<usize> {
    match expr {
        crate::AST::Expr::Ident(name, _) => match &checker.lookup(name)?.ty {
            Type::FixedList { len, .. } => usize::try_from(len.require_literal()).ok(),
            _ => None,
        },
        crate::AST::Expr::Paren(inner, _) => known_list_len(checker, inner),
        _ => literal_list_len(expr),
    }
}

fn exactly_one_type_arg(
    checker: &mut Checker<'_>,
    name: &str,
    type_args: &[Type],
    span: Span,
) -> Option<Type> {
    if type_args.len() != 1 {
        checker.diags.push(Diagnostic::error(
            "E0119",
            format!(
                "`{name}` expects exactly one type argument, got {}",
                type_args.len()
            ),
            "a typed decode call needs one target type".to_string(),
            format!("write `{name}<Target>(...)` with one target type"),
            Some(span),
        ));
        return None;
    }
    Some(type_args[0].clone())
}

fn model_type_leaf(ty: &Type) -> Option<&str> {
    match ty {
        Type::Named(name) => Some(name.rsplit_once('.').map_or(name.as_str(), |(_, leaf)| leaf)),
        Type::TraitObject(names) if names.len() == 1 => names.first().map(String::as_str),
        _ => None,
    }
}

fn literal_int(expr: &crate::AST::Expr) -> Option<i64> {
    match expr {
        crate::AST::Expr::Int(value, ..) => Some(*value),
        crate::AST::Expr::Paren(inner, _) => literal_int(inner),
        crate::AST::Expr::Unary(crate::AST::UnOp::Neg, inner, _) => {
            literal_int(inner)?.checked_neg()
        }
        _ => None,
    }
}

fn compute_alias_return(name: &str, args: &[crate::AST::CallArg]) -> Option<Type> {
    match name {
        "vec" => literal_int(&args.first()?.expr)
            .filter(|value| *value >= 0)
            .map(|value| {
                result_ty(
                    Type::compute_shape_type("Vec", &[value as u64]),
                    Type::Named("ComputeError".to_string()),
                )
            }),
        "matrix" => {
            let rows = literal_int(&args.first()?.expr).filter(|value| *value >= 0)?;
            let cols = literal_int(&args.get(1)?.expr).filter(|value| *value >= 0)?;
            Some(result_ty(
                Type::compute_shape_type("Matrix", &[rows as u64, cols as u64]),
                Type::Named("ComputeError".to_string()),
            ))
        }
        _ => None,
    }
}

fn compute_tensor_type() -> Type {
    Type::Named("Tensor".to_string())
}

fn is_compute_tensor(ty: &Type) -> bool {
    ty.is_compute_tensor_family()
}

fn compute_gradient_value_type(output: &Type) -> Option<Type> {
    match output {
        ty if is_compute_tensor(ty) => Some(compute_tensor_type()),
        Type::Tuple(fields) if fields.iter().all(|(_, ty)| is_compute_tensor(ty)) => {
            Some(Type::Tuple(
                fields
                    .iter()
                    .map(|(name, _)| (name.clone(), Box::new(compute_tensor_type())))
                    .collect(),
            ))
        }
        _ => None,
    }
}

fn compute_tensor_tuple(names: &[String], value_type: &Type) -> Type {
    Type::Tuple(
        names
            .iter()
            .map(|name| (name.clone(), Box::new(value_type.clone())))
            .collect(),
    )
}

fn compute_wrt_names(expr: &Expr) -> Option<Vec<String>> {
    match expr {
        Expr::ListLit(items, _) => items
            .iter()
            .map(|item| match item {
                Expr::Ident(name, _) => Some(name.clone()),
                Expr::Paren(inner, _) => match inner.as_ref() {
                    Expr::Ident(name, _) => Some(name.clone()),
                    _ => None,
                },
                _ => None,
            })
            .collect(),
        Expr::Paren(inner, _) => compute_wrt_names(inner),
        _ => None,
    }
}

fn compute_function_value_names(ty: &Type) -> Option<Vec<String>> {
    let Type::Fn {
        params,
        param_contract,
        call_metadata,
        ..
    } = ty
    else {
        return None;
    };
    // `param_contract` intentionally omits implicit labels.  A derivative
    // transform still has to carry the declaration-local names so a later
    // `compute.gradient` can build its named higher-order result.  Metadata
    // is the source-of-truth for those names; the contract remains the
    // fallback for synthetic callable types that have no declaration metadata.
    if let Some(names) = call_metadata
        .as_ref()
        .map(|metadata| &metadata.names)
        .filter(|names| names.len() == params.len())
    {
        return Some(names.clone());
    }
    param_contract.as_ref().and_then(|contract| {
        (contract.len() == params.len()).then(|| {
            contract
                .iter()
                .map(|(name, _)| name.clone())
                .collect()
        })
    })
}

fn compute_function_names(checker: &Checker<'_>, expr: &Expr) -> Option<Vec<String>> {
    match expr {
        Expr::Paren(inner, _) => compute_function_names(checker, inner),
        Expr::Ident(name, _) => checker
            .funcs
            .get(name)
            .map(|sig| {
                sig.param_info
                    .iter()
                    .map(|(name, _)| name.clone())
                    .collect()
            })
            .or_else(|| {
                let info = checker.lookup(name)?;
                compute_function_value_names(&info.ty)
            }),
        Expr::Lambda(lambda) => Some(
            lambda
                .params
                .iter()
                .map(|param| param.name.clone())
                .collect(),
        ),
        Expr::MethodCall { method, args, .. }
            if matches!(
                method.as_str(),
                "gradient" | "value_and_gradient" | "vjp" | "jvp"
            ) =>
        {
            args.first()
                .and_then(|arg| compute_function_names(checker, &arg.expr))
        }
        Expr::Call(call)
            if matches!(
                call.name.as_str(),
                "gradient" | "value_and_gradient" | "vjp" | "jvp"
            ) =>
        {
            call.args
                .first()
                .and_then(|arg| compute_function_names(checker, &arg.expr))
        }
        _ => None,
    }
}

fn compute_function_identity(checker: &Checker<'_>, expr: &Expr) -> Option<String> {
    match expr {
        Expr::Paren(inner, _) => compute_function_identity(checker, inner),
        Expr::Ident(name, _) if checker.funcs.contains_key(name) => Some(name.clone()),
        Expr::MethodCall { method, args, .. }
            if matches!(
                method.as_str(),
                "gradient" | "value_and_gradient" | "vjp" | "jvp"
            ) =>
        {
            args.first()
                .and_then(|arg| compute_function_identity(checker, &arg.expr))
        }
        Expr::Call(call)
            if matches!(
                call.name.as_str(),
                "gradient" | "value_and_gradient" | "vjp" | "jvp"
            ) =>
        {
            call.args
                .first()
                .and_then(|arg| compute_function_identity(checker, &arg.expr))
        }
        _ => None,
    }
}

fn safe_envelope_raw_argument(
    module: &str,
    name: &str,
    args: &[crate::AST::CallArg],
    expected_arity: usize,
) -> Option<Diagnostic> {
    if module != "core.crypto"
        || !matches!(name, "seal" | "open" | "file_seal" | "file_open")
        || args.len() != expected_arity + 1
    {
        return None;
    }
    let extra = args.last()?;
    let (label, label_span) = extra.label.as_ref()?;
    let (reason, why, fix) = match label.as_str() {
        "nonce" => (
            CryptoMisuseReason::RawNonce,
            format!("`{name}` would use a caller-supplied nonce, which violates the safe envelope requirement that Jet manages nonces internally"),
            "remove the `nonce:` argument, or use a raw expert primitive inside `#Unsafe` for protocol interop".to_string(),
        ),
        "algorithm" => (
            CryptoMisuseReason::RawAlgorithm,
            format!("`{name}` would select a caller-supplied algorithm, which violates the safe envelope requirement that Jet selects the algorithm internally"),
            "remove the `algorithm:` argument, or use a raw expert primitive inside `#Unsafe` for protocol interop".to_string(),
        ),
        _ => return None,
    };
    let operation = match name {
        "seal" => "seal",
        "open" => "open",
        "file_seal" => "file_seal",
        "file_open" => "file_open",
        _ => unreachable!(),
    };
    Some(Diagnostic::crypto_misuse_fact(
        why,
        fix,
        Span::new(label_span.start, extra.expr.span().end),
        reason,
        operation,
    ))
}

fn crypto_misuse_diagnostic(
    checker: &Checker<'_>,
    module: &str,
    name: &str,
    args: &[crate::AST::CallArg],
) -> Option<Diagnostic> {
    if module == "core.crypto" && name == "password_hash_with_salt" {
        return Some(Diagnostic::crypto_misuse_fact(
            "`password_hash_with_salt` would use caller-controlled salt bytes, which makes a deterministic entropy seam reachable from a release build".to_string(),
            "use `crypto.password_hash` so Jet generates the salt, or move a fixed vector to `expert.argon2id` inside `#Unsafe`".to_string(),
            args.get(1)?.expr.span(),
            CryptoMisuseReason::DeterministicEntropy,
            "password_hash_with_salt",
        ));
    }
    let hkdf_operation = match (module, name) {
        ("core.crypto", "hkdf_sha256") => Some("hkdf_sha256"),
        ("core.crypto.expert", "hkdf_sha256_raw") => Some("hkdf_sha256_raw"),
        _ => None,
    };
    if let Some(operation) = hkdf_operation {
        let length = args.get(3)?;
        let actual = literal_int(&length.expr)?;
        if !(0..=8160).contains(&actual) {
            return Some(Diagnostic::crypto_misuse(
                format!(
                    "HKDF-SHA256 output length is {actual} bytes; this operation requires 0..8160"
                ),
                "pass an output length from 0 through 8160 bytes".to_string(),
                length.expr.span(),
                CryptoMisuseReason::OutputLength,
                operation,
                "0..8160",
                i128::from(actual),
            ));
        }
    }
    if module == "core.crypto.expert" && name == "argon2id" {
        let salt = args.get(1)?;
        if let Some(actual) = known_list_len(checker, &salt.expr) {
            if !(8..=64).contains(&actual) {
                let unit = if actual == 1 { "byte" } else { "bytes" };
                return Some(Diagnostic::crypto_misuse(
                    format!("Argon2id salt has {actual} {unit}; this operation requires 8..64"),
                    "pass an explicit salt from 8 through 64 bytes".to_string(),
                    salt.expr.span(),
                    CryptoMisuseReason::SaltLength,
                    "argon2id",
                    "8..64",
                    actual as i128,
                ));
            }
        }

        let memory = args.get(2)?;
        let memory_kib = literal_int(&memory.expr);
        if let Some(actual) = memory_kib {
            if !(8_192..=262_144).contains(&actual) {
                return Some(Diagnostic::crypto_misuse(
                    format!("Argon2id memory cost is {actual} KiB; this operation requires 8192..262144"),
                    "pass a memory cost from 8192 through 262144 KiB".to_string(),
                    memory.expr.span(),
                    CryptoMisuseReason::MemoryCost,
                    "argon2id",
                    "8192..262144",
                    i128::from(actual),
                ));
            }
        }

        let iterations = args.get(3)?;
        let iteration_count = literal_int(&iterations.expr);
        if let Some(actual) = iteration_count {
            if !(1..=10).contains(&actual) {
                return Some(Diagnostic::crypto_misuse(
                    format!("Argon2id iteration count is {actual}; this operation requires 1..10"),
                    "pass an iteration count from 1 through 10".to_string(),
                    iterations.expr.span(),
                    CryptoMisuseReason::IterationCount,
                    "argon2id",
                    "1..10",
                    i128::from(actual),
                ));
            }
        }

        let lanes = args.get(4)?;
        if let Some(actual) = literal_int(&lanes.expr) {
            if !(1..=8).contains(&actual) {
                return Some(Diagnostic::crypto_misuse(
                    format!("Argon2id lane count is {actual}; this operation requires 1..8"),
                    "pass a lane count from 1 through 8".to_string(),
                    lanes.expr.span(),
                    CryptoMisuseReason::LaneCount,
                    "argon2id",
                    "1..8",
                    i128::from(actual),
                ));
            }
        }

        let output = args.get(5)?;
        if let Some(actual) = literal_int(&output.expr) {
            if !(16..=64).contains(&actual) {
                return Some(Diagnostic::crypto_misuse(
                    format!(
                        "Argon2id output length is {actual} bytes; this operation requires 16..64"
                    ),
                    "pass an output length from 16 through 64 bytes".to_string(),
                    output.expr.span(),
                    CryptoMisuseReason::OutputLength,
                    "argon2id",
                    "16..64",
                    i128::from(actual),
                ));
            }
        }

        if let (Some(memory_kib), Some(iteration_count)) = (memory_kib, iteration_count) {
            if let Some(actual) = memory_kib.checked_mul(iteration_count) {
                if actual > 1_048_576 {
                    return Some(Diagnostic::crypto_misuse(
                        format!("Argon2id memory-time cost is {actual} KiB-rounds; this operation permits at most 1048576"),
                        "reduce memory or iterations so their product is at most 1048576 KiB-rounds".to_string(),
                        Span::new(memory.expr.span().start, iterations.expr.span().end),
                        CryptoMisuseReason::MemoryTimeCost,
                        "argon2id",
                        "at most 1048576",
                        i128::from(actual),
                    ));
                }
            }
        }
    }
    let operation_name = match name {
        "xchacha20poly1305_seal" => "xchacha20poly1305_seal",
        "xchacha20poly1305_open" => "xchacha20poly1305_open",
        "aes256gcm_seal" => "aes256gcm_seal",
        "aes256gcm_open" => "aes256gcm_open",
        "ed25519_sign" => "ed25519_sign",
        "ed25519_verify_strict" => "ed25519_verify_strict",
        "x25519_raw" => "x25519_raw",
        _ => return None,
    };
    let material_requirements: &[(usize, &str, &str, usize)] = match (module, name) {
        ("core.crypto.expert", "xchacha20poly1305_seal" | "xchacha20poly1305_open") => {
            &[(0, "XChaCha20-Poly1305 key", "key", 32)]
        }
        ("core.crypto.expert", "aes256gcm_seal" | "aes256gcm_open") => {
            &[(0, "AES-256-GCM key", "key", 32)]
        }
        ("core.crypto.expert", "ed25519_sign") => {
            &[(0, "Ed25519 signing seed", "signing seed", 32)]
        }
        ("core.crypto.expert", "ed25519_verify_strict") => &[
            (0, "Ed25519 public key", "public key", 32),
            (2, "Ed25519 signature", "signature", 64),
        ],
        ("core.crypto.expert", "x25519_raw") => &[
            (0, "X25519 secret key", "secret key", 32),
            (1, "X25519 public key", "public key", 32),
        ],
        _ => &[],
    };
    for &(index, fact, replacement, expected) in material_requirements {
        let arg = args.get(index)?;
        let Some(actual) = known_list_len(checker, &arg.expr) else {
            continue;
        };
        if actual != expected {
            let unit = if actual == 1 { "byte" } else { "bytes" };
            return Some(Diagnostic::crypto_misuse(
                format!("{fact} has {actual} {unit}; this operation requires exactly {expected}"),
                format!("pass a {expected}-byte {replacement}"),
                arg.expr.span(),
                CryptoMisuseReason::InvalidLength,
                operation_name,
                if expected == 64 {
                    "exactly 64"
                } else {
                    "exactly 32"
                },
                actual as i128,
            ));
        }
    }
    let (operation, expected, expected_text) = match name {
        "xchacha20poly1305_seal" | "xchacha20poly1305_open" => {
            ("XChaCha20-Poly1305", 24, "exactly 24")
        }
        "aes256gcm_seal" | "aes256gcm_open" => ("AES-256-GCM", 12, "exactly 12"),
        _ => return None,
    };
    let nonce = args.get(1)?;
    let actual = known_list_len(checker, &nonce.expr)?;
    if actual == expected {
        return None;
    }
    let unit = if actual == 1 { "byte" } else { "bytes" };
    Some(Diagnostic::crypto_misuse(
        format!(
            "{operation} nonce has {actual} {unit}; this operation requires exactly {expected}"
        ),
        format!("pass a {expected}-byte nonce, or use core.crypto.seal so Jet generates it"),
        nonce.expr.span(),
        CryptoMisuseReason::NonceLength,
        operation_name,
        expected_text,
        actual as i128,
    ))
}

fn resolved_core_fixed_sig(
    module: &str,
    name: &str,
    type_args: &[Type],
    span: Span,
    diags: &mut Vec<Diagnostic>,
) -> Option<(Vec<(AccessConvention, Type)>, Option<Type>)> {
    let (params, ret) = match Syntax::core_call(module, name) {
        Some(row) => {
            let row = Syntax::core_call_projection(
                module,
                name,
                Syntax::CoreCallCoverage::SEMA,
                row.arity(),
            )
            .ok()?;
            // `time.now` is a registered runtime Core row whose return type is
            // not represented in the ordinary fixed-signature table. Keep its
            // sema type attached so pure diagnostics do not lose the call's
            // return type or fall through to E1004.
            core_fixed_sig_for_row(row).or_else(|| {
                (module == "core.time" && name == "now")
                    .then(|| (Vec::new(), Some(Type::Int)))
            })?
        }
        None => core_fixed_sig(module, name)?,
    };
    let resolved = if matches!(module, "core.crypto" | "core.crypto.expert") {
        Some((
            params
                .into_iter()
                .map(|(access, ty)| (access, crate::Sema::Diagnostics::core_crypto_nominal(ty)))
                .collect(),
            ret.map(crate::Sema::Diagnostics::core_crypto_nominal),
        ))
    } else {
        Some((params, ret))
    };
    if !type_args.is_empty() && resolved.is_some() {
        diags.push(Diagnostic::error(
            "E0119",
            format!("`{name}` is not generic"),
            "only functions declared with type parameters accept call-site type arguments"
                .to_string(),
            format!("call {name} without type arguments"),
            Some(span),
        ));
    }
    resolved
}

fn core_compiler_return(name: &str) -> Type {
    let value = match name {
        "lex" => "CompilerLexed",
        "parse" => "CompilerSyntaxTree",
        "check" => "CompilerChecked",
        "source_map" => "CompilerSourceMap",
        "manifest" => "CompilerManifest",
        "package" => "CompilerPackage",
        "lock" => "CompilerLock",
        "profiles" => "CompilerProfileSet",
        _ => "CompilerError",
    };
    let error = if matches!(name, "manifest" | "package" | "lock" | "profiles") {
        "CompilerPackageError"
    } else {
        "CompilerError"
    };
    Type::Result {
        ok: Box::new(Type::Named(value.to_string())),
        err: Box::new(Type::Named(error.to_string())),
    }
}
fn core_build_return(name: &str) -> Type {
    Type::Named(match name {
        "graph" => Syntax::TYPE_BUILD_GRAPH,
        "receipt_diff" => Syntax::TYPE_BUILD_GRAPH_DIFF,
        _ => Syntax::TYPE_BUILD_GRAPH_DIFF,
    }.to_string())
}

impl<'a> Checker<'a> {
    fn infer_compute_transform(
        &mut self,
        name: &str,
        span: Span,
        args: &mut [crate::AST::CallArg],
    ) -> Option<Type> {
        let Some(first) = args.first_mut() else {
            self.diags.push(wrong_core_arity(name, 1, 0, span));
            return None;
        };
        let f_expr = first.expr.clone();
        let f_ty = self.infer(&mut first.expr);
        let Some(Type::Fn {
            params,
            ret,
            effect_bound,
            param_contract,
            call_metadata,
            ..
        }) = f_ty
        else {
            self.diags.push(Diagnostic::error(
                "E0112",
                format!("`compute.{name}` expects a function value"),
                "autodiff transforms differentiate a function over Tensor arguments".to_string(),
                "bind a Tensor function before passing it to the transform".to_string(),
                Some(span),
            ));
            for arg in args.iter_mut().skip(1) {
                self.infer(&mut arg.expr);
            }
            return None;
        };
        if let Some(target) = compute_function_identity(self, &f_expr) {
            self.fx_autodiff_obligations
                .push(crate::Sema::Effects::AutodiffObligation {
                    method: name.to_string(),
                    target,
                    span,
                });
        }
        if effect_bound.as_ref().is_some_and(|row| !row.is_empty()) {
            self.diags.push(Diagnostic::error(
                    "E0112",
                    format!("`compute.{name}` needs a pure Tensor function"),
                    "autodiff records only pure Tensor operations and cannot carry an effectful callable".to_string(),
                    "remove the effect row from the differentiated function or differentiate a pure Tensor function".to_string(),
                    Some(span),
                ));
        }
        // D-FAIL-IMPLICIT: a function value's return is carrier-shaped by
        // default. The transform differentiates the SUCCESS value; the
        // carrier is the caller's concern, not a storage-law violation.
        let output = ret.map(|ret| *ret).unwrap_or_else(unit_ty);
        let output = match output {
            Type::Result { ok, .. } => *ok,
            other => other,
        };
        let gradient_output = name == "gradient" && compute_gradient_value_type(&output).is_some();
        if !is_compute_tensor(&output) && !gradient_output {
            self.diags.push(Diagnostic::error(
                    "E0112",
                    format!("`compute.{name}` needs a function returning `Tensor`"),
                    "the reverse and forward transforms have one Tensor output and keep Tensor storage law".to_string(),
                    "return a Tensor from the differentiated function".to_string(),
                    Some(span),
                ));
        }
        if gradient_output && name != "gradient" {
            self.diags.push(Diagnostic::error(
                    "E0112",
                    format!("`compute.{name}` needs a function returning `Tensor`"),
                    "only a gradient transform can differentiate a named Tensor tuple; value and pull surfaces require one Tensor output".to_string(),
                    "return one Tensor from the differentiated function".to_string(),
                    Some(span),
                ));
        }
        if params.iter().any(|param| !param.is_compute_tensor_family()) {
            self.diags.push(Diagnostic::error(
                "E0112",
                format!("`compute.{name}` needs Tensor function arguments"),
                "autodiff records only pure Tensor operations".to_string(),
                "use a function whose parameters are all `Tensor`".to_string(),
                Some(span),
            ));
        }
        let parameter_names = compute_function_names(self, &f_expr);
        let mut value_indexes = Vec::new();
        let mut wrt_expr = None;
        for (index, arg) in args.iter().enumerate().skip(1) {
            if arg.label.as_ref().is_some_and(|(label, _)| label == "wrt") {
                wrt_expr = Some(arg.expr.clone());
            } else {
                value_indexes.push(index);
            }
        }
        for index in &value_indexes {
            let value_ty = self.infer(&mut args[*index].expr);
            if !value_ty
                .as_ref()
                .is_some_and(Type::is_compute_tensor_family)
            {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!("argument {} to `compute.{name}` should be `Tensor`", index),
                    "autodiff transforms record Tensor values, not scalar policy arguments"
                        .to_string(),
                    "pass a Tensor value".to_string(),
                    Some(args[*index].span),
                ));
            }
        }
        if let Some(wrt) = &mut wrt_expr {
            if compute_wrt_names(wrt).is_none() {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!("`wrt:` for `compute.{name}` must list parameter names"),
                    "the differentiation target is selected by the function parameter name"
                        .to_string(),
                    "write `wrt: [parameter]`".to_string(),
                    Some(wrt.span()),
                ));
            }
        }
        if wrt_expr.is_some() && !matches!(name, "gradient" | "value_and_gradient") {
            self.diags.push(Diagnostic::error(
                    "E0112",
                    format!("`wrt:` is not supported by `compute.{name}`"),
                    "`wrt:` selects named gradients on `compute.gradient` and `compute.value_and_gradient`".to_string(),
                    "remove `wrt:` and provide the full VJP or JVP inputs".to_string(),
                    Some(span),
                ));
        }
        let expected_values = if name == "jvp" {
            params.len().saturating_mul(2)
        } else {
            params.len()
        };
        let direct = !value_indexes.is_empty();
        if name == "jvp" && direct {
            self.diags.push(Diagnostic::error(
                    "E0112",
                    "`compute.jvp` is a function transform, not a direct call".to_string(),
                    "the ratified JVP surface returns a callable that accepts primal and tangent values".to_string(),
                    "bind `d_f :: compute.jvp(f)`, then call `d_f(primal..., tangent...)`".to_string(),
                    Some(span),
                ));
            return None;
        }
        if direct && value_indexes.len() != expected_values {
            self.diags.push(wrong_core_arity(
                name,
                expected_values + 1,
                args.len(),
                span,
            ));
        }
        let Some(names) = parameter_names else {
            self.diags.push(Diagnostic::error(
                "E0112",
                format!("`compute.{name}` needs named Tensor parameters"),
                "named gradient fields come from the differentiated function signature".to_string(),
                "pass a named Tensor function or lambda".to_string(),
                Some(span),
            ));
            return None;
        };
        let mut selected =
            if let Some(wrt) = wrt_expr.as_ref().and_then(|expr| compute_wrt_names(expr)) {
                let mut selected = Vec::new();
                for target in wrt {
                    let index = names.iter().position(|name| name == &target);
                    let Some(index) = index else {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!(
                                "`wrt` names no parameter `{target}`; parameters are [{}]",
                                names.join(", ")
                            ),
                            "a differentiation target must name one function parameter".to_string(),
                            "use a parameter name from the differentiated function".to_string(),
                            Some(span),
                        ));
                        continue;
                    };
                    if !selected.contains(&index) {
                        selected.push(index);
                    }
                }
                selected
            } else {
                (0..params.len()).collect()
            };
        if selected.is_empty() {
            selected = (0..params.len()).collect();
        }
        let Some(gradient_names) = selected
            .iter()
            .map(|index| names.get(*index).cloned())
            .collect::<Option<Vec<_>>>()
        else {
            self.diags.push(Diagnostic::error(
                "E0112",
                format!("`compute.{name}` cannot name every Tensor parameter"),
                "gradient fields come from the differentiated function signature".to_string(),
                "pass a callable with named Tensor parameters".to_string(),
                Some(span),
            ));
            return None;
        };
        let gradient_value_type =
            compute_gradient_value_type(&output).unwrap_or_else(compute_tensor_type);
        let gradient_ty = compute_tensor_tuple(&gradient_names, &gradient_value_type);
        let run_ty = Type::Apply {
            name: "VjpRun".to_string(),
            args: vec![gradient_ty.clone()],
        };
        let direct_return = match name {
            "gradient" => gradient_ty.clone(),
            "value_and_gradient" => Type::Tuple(vec![
                ("value".to_string(), Box::new(compute_tensor_type())),
                ("gradients".to_string(), Box::new(gradient_ty.clone())),
            ]),
            "vjp" => run_ty.clone(),
            "jvp" => Type::Tuple(vec![
                ("value".to_string(), Box::new(compute_tensor_type())),
                ("tangent".to_string(), Box::new(compute_tensor_type())),
            ]),
            _ => unreachable!("compute transform name"),
        };
        if direct {
            return Some(direct_return);
        }
        let transform_params = if name == "jvp" {
            params
                .iter()
                .cloned()
                .chain(params.iter().cloned())
                .collect()
        } else {
            params.clone()
        };
        let transform_contract = param_contract.map(|contract| {
            if name == "jvp" {
                contract
                    .iter()
                    .cloned()
                    .chain(contract.iter().cloned())
                    .collect()
            } else {
                contract
            }
        });
        let transform_metadata = call_metadata.map(|metadata| {
            if name != "jvp" {
                return metadata;
            }
            crate::AST::FunctionCallMetadata {
                names: metadata
                    .names
                    .iter()
                    .cloned()
                    .chain(metadata.names.iter().cloned())
                    .collect(),
                defaults: metadata
                    .defaults
                    .iter()
                    .cloned()
                    .chain(metadata.defaults.iter().cloned())
                    .collect(),
                variadic: metadata
                    .variadic
                    .iter()
                    .copied()
                    .chain(metadata.variadic.iter().copied())
                    .collect(),
                conventions: metadata
                    .conventions
                    .iter()
                    .copied()
                    .chain(metadata.conventions.iter().copied())
                    .collect(),
                policies: metadata.policies.clone(),
            }
        });
        let transform_return = match name {
            "gradient" => gradient_ty.clone(),
            "value_and_gradient" => Type::Tuple(vec![
                ("value".to_string(), Box::new(compute_tensor_type())),
                ("gradients".to_string(), Box::new(gradient_ty.clone())),
            ]),
            "vjp" => run_ty,
            "jvp" => Type::Tuple(vec![
                ("value".to_string(), Box::new(compute_tensor_type())),
                ("tangent".to_string(), Box::new(compute_tensor_type())),
            ]),
            _ => unreachable!("compute transform name"),
        };
        Some(Type::Fn {
            params: transform_params,
            ret: Some(Box::new(transform_return)),
            effect_bound: effect_bound.clone(),
            param_contract: transform_contract,
            call_metadata: transform_metadata,
            return_view_provenance: None,
        })
    }

}

fn web_apply(name: &str, arg: Type) -> Type {
    Type::Apply {
        name: name.to_string(),
        args: vec![arg],
    }
}

fn web_callback(param: Type, ret: Type) -> Type {
    Type::Fn {
        params: vec![param],
        ret: Some(Box::new(ret)),
        effect_bound: None,
        return_view_provenance: None,
        param_contract: None,
        call_metadata: None,
    }
}
fn web_result(ok: Type) -> Type {
    Type::Result {
        ok: Box::new(ok),
        err: Box::new(Type::String),
    }
}
fn web_callback_result(ok: Type) -> Type {
    Type::Result {
        ok: Box::new(ok),
        err: Box::new(Type::Named(Syntax::TYPE_ERR.to_string())),
    }
}

impl<'a> Checker<'a> {
    /// D-WEBFORM1=A: elaborate `web.form(Model, action: handler)` once,
    /// from the declared model fields, into the existing typed-form input
    /// constructor. The runtime only sees the canonical `WebFormInput` and
    /// action-name values, so no second form or server-function mechanism can
    /// drift from `core.web.forms.typed` / #2477.
    fn infer_web_form_core_call(
        &mut self,
        span: Span,
        args: &mut Vec<CallArg>,
    ) -> Option<Type> {
        let typed = Type::Named("WebFormTyped".to_string());
        if args.len() != 2 {
            self.diags
                .push(wrong_core_arity("form", 2, args.len(), span));
            for arg in args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return Some(typed);
        }

        let action_index = args
            .iter()
            .position(|arg| arg.label.as_ref().is_some_and(|(label, _)| label == "action"))
            .unwrap_or(1);
        let model_index = if action_index == 0 { 1 } else { 0 };
        let model_arg = args[model_index].clone();
        let action_arg = args[action_index].clone();
        let model_name = match &model_arg.expr {
            Expr::Ident(name, _) => name.clone(),
            _ => {
                self.diags.push(Diagnostic::error(
                    "E2474",
                    "web.form needs a named model type".to_string(),
                    "the form field set is derived from one declared struct, so a runtime value cannot provide its schema"
                        .to_string(),
                    "pass the struct name as the first argument: `web.form(Checkout, action: submit)`"
                        .to_string(),
                    Some(model_arg.expr.span()),
                ));
                for arg in args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return Some(typed);
            }
        };
        let Some(fields) = self
            .struct_fields_for_type_name(&model_name)
            .map(|fields| fields.to_vec())
        else {
            self.diags.push(Diagnostic::error(
                "E2474",
                format!("web.form model `{model_name}` is not a declared struct"),
                "the form field set must come from the model's declared fields".to_string(),
                format!("declare `struct {model_name} {{ ... }}` before constructing the form"),
                Some(model_arg.expr.span()),
            ));
            return Some(typed);
        };

        let action_name = match &action_arg.expr {
            Expr::Ident(name, _) => name.clone(),
            _ => {
                self.diags.push(Diagnostic::error(
                    "E2474",
                    "web.form action must be a named function".to_string(),
                    "the action is registered through the existing typed server-function boundary"
                        .to_string(),
                    "pass a named action function: `web.form(Model, action: submit)`".to_string(),
                    Some(action_arg.expr.span()),
                ));
                return Some(typed);
            }
        };
        let action_params = self
            .funcs
            .get(&action_name)
            .map(|sig| sig.params.clone());
        let action_accepts_model = action_params
            .as_ref()
            .and_then(|params| params.first())
            .is_some_and(|(_, ty)| self.resolve_type(ty.clone()) == Type::Named(model_name.clone()))
            && action_params.as_ref().is_some_and(|params| params.len() == 1);
        if !action_accepts_model {
            self.diags.push(Diagnostic::error(
                "E2474",
                format!("form action `{action_name}` does not accept `{model_name}`"),
                "the form derives its fields from the model and the one server-function action must receive that exact input type"
                    .to_string(),
                format!("change `{action_name}` to accept `{model_name}` as its only parameter"),
                Some(action_arg.expr.span()),
            ));
        }

        let call_arg = |expr| CallArg {
            convention: AccessConvention::Read,
            expr,
            span,
            flags: CallArgFlags::default(),
            label: None,
            spread: false,
        };
        let enum_lit = |type_name: &str, variant: &str, field_span: Span| Expr::EnumLit {
            type_name: type_name.to_string(),
            variant: variant.to_string(),
            variant_span: Some(field_span),
            args: Vec::new(),
            leading_dot: false,
            span: field_span,
        };
        let mut descriptors = Vec::with_capacity(fields.len());
        for (field, field_span, field_ty) in fields {
            let (value_variant, control_variant) = match field_ty {
                Type::String => ("String", "Text"),
                Type::Int => ("Int", "Number"),
                Type::Bool => ("Bool", "Checkbox"),
                Type::Float => ("Float", "Number"),
                other => {
                    self.diags.push(Diagnostic::error(
                        "E2474",
                        format!(
                            "form field `{field}` on `{model_name}` has unsupported type `{}`",
                            other.show()
                        ),
                        "web forms decode scalar controls into the declared model fields without a second coercion policy"
                            .to_string(),
                        format!("change `{field}` to `String`, `Int`, `Bool`, or `Float`"),
                        Some(field_span),
                    ));
                    continue;
                }
            };
            let text = |value: String| Expr::Str(vec![StrPart::Lit(value)], field_span);
            descriptors.push(Expr::StructLit {
                type_name: "WebFormFieldSpec".to_string(),
                type_args: Vec::new(),
                import_ns: None,
                as_trait: None,
                fields: vec![
                    ("name".to_string(), field_span, text(field.clone())),
                    (
                        "value_type".to_string(),
                        field_span,
                        enum_lit("WebFormValueType", value_variant, field_span),
                    ),
                    ("required".to_string(), field_span, Expr::Bool(true, field_span)),
                    ("default".to_string(), field_span, Expr::Absent(field_span)),
                    ("label".to_string(), field_span, text(field.clone())),
                    (
                        "control".to_string(),
                        field_span,
                        enum_lit("WebFormControl", control_variant, field_span),
                    ),
                    ("group".to_string(), field_span, Expr::Absent(field_span)),
                    ("wire_name".to_string(), field_span, text(field)),
                ],
                inferred: false,
                span: field_span,
            });
        }

        let input_call = Expr::MethodCall {
            receiver: Box::new(Expr::Field(
                Box::new(Expr::Ident("web".to_string(), span)),
                "forms".to_string(),
                span,
            )),
            method: "input".to_string(),
            method_span: span,
            owner_type_args: Vec::new(),
            type_args: Vec::new(),
            args: vec![
                call_arg(Expr::Str(
                    vec![StrPart::Lit(model_name.clone())],
                    span,
                )),
                call_arg(Expr::ListLit(descriptors, span)),
            ],
            recv_type: None,
            resolved_ret: None,
            operator_rhs: None,
            checked_widen: false,
        };
        let input = Expr::OrFallback {
            value: Box::new(input_call),
            fallback: OrFallback::Panic {
                name_span: span,
                args: vec![call_arg(Expr::Str(
                    vec![StrPart::Lit("web.form could not construct its derived input".to_string())],
                    span,
                ))],
            },
            is_option: false,
            span,
        };
        *args = vec![
            call_arg(input),
            call_arg(Expr::Str(vec![StrPart::Lit(action_name)], span)),
        ];
        let _ = self.infer(&mut args[0].expr);
        let _ = self.infer(&mut args[1].expr);
        Some(typed)
    }

    /// D-FLAGSHIP-WEBAPI1=A: resolve the element type for the generic web
    /// suite calls in one sema path. The foundation rows own lookup and
    /// arity; this path owns the resolved Jet return shape.
    fn infer_web_generic_core_call(
        &mut self,
        module: &str,
        name: &str,
        type_args: &[Type],
        span: Span,
        args: &mut [crate::AST::CallArg],
    ) -> Option<Type> {
        if !matches!(
            (module, name),
            ("core.web.table",
                "new" | "new_keyed" | "column" | "with_column" | "with_server_page"
                    | "state" | "facts" | "keys" | "page_state" | "sort" | "filter" | "page"
                    | "sort_by" | "filter_by" | "paginate" | "set_rows" | "set_selected"
                    | "toggle_selection" | "clear_selection" | "focus" | "clear_focus" | "focused_key"
                    | "selected_keys" | "selected_rows"
                    | "insert_row" | "replace_row" | "update_row" | "remove_row"
                    | "first_page" | "next_page" | "last_page" | "visible_rows")
                | ("core.web.virtual",
                    "window" | "window_measured" | "slice" | "indices"
                        | "plan" | "plan_measured" | "plan_from_sizes" | "plan_indices"
                        | "plan_slice" | "plan_viewport" | "plan_measure"
                        | "plan_viewport_state" | "plan_scroll_to" | "plan_resize"
                        | "plan_viewport_measure" | "plan_facts")
                | ("core.web.store",
                    "new" | "with_history" | "value" | "signal" | "state_signal"
                        | "transaction" | "update" | "batch"
                        | "set" | "set_state" | "optimistic" | "patch" | "patch_generation"
                        | "patch_transaction" | "patch_active" | "patch_commit" | "patch_rollback"
                        | "back" | "forward" | "jump" | "scrub" | "restore" | "history"
                        | "history_at" | "events" | "events_since" | "clear_history"
                        | "history_enabled" | "history_limit" | "set_history_limit" | "cursor"
                        | "current_generation" | "subscribe" | "subscribe_selector"
                        | "subscription_unsubscribe" | "subscription_active" | "derived" | "selector"
                        | "inspect" | "facts_json" | "event_json")
        ) {
            return None;
        }
        let expected = match (module, name) {
            ("core.web.table", "new") => 2,
            ("core.web.table", "new_keyed" | "column" | "sort_by" | "filter_by" | "paginate"
                | "replace_row" | "update_row") => 3,
            ("core.web.table", "with_column" | "with_server_page" | "set_rows"
                | "insert_row" | "visible_rows") => 2,
            ("core.web.table", "state" | "facts" | "keys" | "page_state" | "filter"
                | "clear_selection" | "selected_keys" | "selected_rows" | "clear_focus" | "focused_key"
                | "first_page" | "next_page" | "last_page") => 1,
            ("core.web.table", "focus") => 2,
            ("core.web.table", "set_selected") => 3,
            ("core.web.table", "toggle_selection" | "remove_row") => 2,
            ("core.web.table", "sort" | "page") => 3,
            ("core.web.virtual", "window") => 5,
            ("core.web.virtual", "window_measured") => 6,
            ("core.web.virtual", "slice" | "plan_slice" | "plan_scroll_to") => 2,
            ("core.web.virtual", "plan_resize" | "plan_viewport_measure") => 3,
            ("core.web.virtual", "indices" | "plan_indices" | "plan_viewport" | "plan_facts"
                | "plan_viewport_state") => 1,
            ("core.web.virtual", "plan") => 6,
            ("core.web.virtual", "plan_measure") => 3,
            ("core.web.virtual", "plan_measured" | "plan_from_sizes") => 7,
            ("core.web.store", "new") => 2,
            ("core.web.store", "with_history") => 3,
            ("core.web.store", "value" | "signal" | "state_signal" | "back" | "forward" | "history" | "events"
                | "clear_history" | "history_enabled" | "history_limit" | "cursor"
                | "current_generation" | "inspect" | "facts_json" | "event_json"
                | "patch_generation" | "patch_transaction" | "patch_active" | "patch_commit"
                | "patch_rollback" | "subscription_unsubscribe" | "subscription_active") => 1,
            ("core.web.store", "transaction" | "update" | "batch" | "optimistic" | "patch") => 4,
            ("core.web.store", "set" | "set_state" | "jump" | "scrub" | "restore"
                | "history_at" | "events_since" | "set_history_limit") => 2,
            ("core.web.store", "subscribe" | "derived" | "selector") => 2,
            ("core.web.store", "subscribe_selector") => 3,
            _ => unreachable!("web generic call was checked above"),
        };
        if args.len() != expected {
            self.diags
                .push(wrong_core_arity(name, expected, args.len(), span));
            for arg in args.iter_mut() {
                self.infer(&mut arg.expr);
            }
        }
        let hinted = type_args.first().cloned();
        macro_rules! web_list_element {
            ($ty:expr) => {{
                match $ty {
                    Type::List(inner) => *inner,
                    other => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`{module}.{name}` needs a typed list, not {}", other.show()),
                            "the web collection helpers preserve one element type".to_string(),
                            "pass a `[T]` value".to_string(),
                            Some(span),
                        ));
                        other
                    }
                }
            }};
        }
        macro_rules! web_expect {
            ($index:expr, $ty:expr) => {{
                let ty = $ty;
                if let Some(arg) = args.get_mut($index) {
                    self.expect_core_arg(name, $index, &ty, arg);
                }
            }};
        }
        macro_rules! web_handle_element {
            ($index:expr, $handle:literal) => {{
                args.get_mut($index)
                    .and_then(|arg| self.infer(&mut arg.expr))
                    .and_then(|ty| match ty {
                        Type::Apply { name, args } if name == $handle && args.len() == 1 => {
                            Some(args[0].clone())
                        }
                        _ => None,
                    })
                    .or_else(|| hinted.clone())
                    .unwrap_or(Type::Int)
            }};
        }
        match (module, name) {
            ("core.web.table", "new") => {
                web_expect!(0, Type::String);
                let row = hinted.unwrap_or_else(|| {
                    args.get_mut(1)
                        .and_then(|arg| self.infer(&mut arg.expr))
                        .and_then(|ty| match ty {
                            Type::List(inner) => Some(*inner),
                            _ => None,
                        })
                        .unwrap_or(Type::Int)
                });
                web_expect!(1, Type::List(Box::new(row.clone())));
                Some(web_apply("WebTable", row))
            }
            ("core.web.table", "column") => {
                web_expect!(0, Type::String);
                web_expect!(1, Type::String);
                let row = hinted
                    .or_else(|| {
                        args.get_mut(2)
                            .and_then(|arg| self.infer(&mut arg.expr))
                            .and_then(|ty| match ty {
                                Type::Fn { params, .. } => params.first().cloned(),
                                _ => None,
                            })
                    })
                    .unwrap_or(Type::Int);
                if let Some(column_name) = args.get(0).and_then(|arg| literal_string_value(&arg.expr)) {
                    let mut schema = &row;
                    while let Type::Tagged { inner, .. } = schema {
                        schema = inner.as_ref();
                    }
                    let span = args.get(0).map(|arg| arg.span).unwrap_or(span);
                    let Some(type_name) = (match schema {
                        Type::Named(name) | Type::Apply { name, .. } => Some(name.as_str()),
                        _ => None,
                    }) else {
                        self.diags.push(Diagnostic::error(
                            "E2475",
                            format!(
                                "table column `{column_name}` needs a declared row schema, got `{}`",
                                schema.show()
                            ),
                            "table columns are checked against row fields before a renderer can consume them"
                                .to_string(),
                            "declare a row struct, then use one of its fields as the column name"
                                .to_string(),
                            Some(span),
                        ));
                        return Some(web_apply("WebTableColumn", row));
                    };
                    let Some(fields) = self
                        .struct_fields_for_type_name(type_name)
                        .map(|fields| fields.to_vec())
                    else {
                        self.diags.push(Diagnostic::error(
                            "E2475",
                            format!(
                                "table column `{column_name}` needs fields from row type `{type_name}`"
                            ),
                            "a typed table column must resolve against a declared row schema"
                                .to_string(),
                            "declare the row fields before constructing the table column".to_string(),
                            Some(span),
                        ));
                        return Some(web_apply("WebTableColumn", row));
                    };
                    if !fields.iter().any(|(field, _, _)| field == &column_name) {
                        let known = fields
                            .iter()
                            .map(|(field, _, _)| field.clone())
                            .collect::<Vec<_>>();
                        let suggestion = suggest_field(&column_name, &known);
                        let what = suggestion
                            .as_ref()
                            .map(|field| {
                                format!(
                                    "table column `{column_name}` is not a field on `{type_name}`; did you mean `{field}`?"
                                )
                            })
                            .unwrap_or_else(|| {
                                format!(
                                    "table column `{column_name}` is not a field on `{type_name}`"
                                )
                            });
                        let fix = suggestion
                            .as_ref()
                            .map(|field| format!("rename the column to the existing field `{field}`"))
                            .unwrap_or_else(|| {
                                format!(
                                    "rename the column to one of the existing fields: {}",
                                    known.join(", ")
                                )
                            });
                        self.diags.push(Diagnostic::error(
                            "E2475",
                            what,
                            "the table keeps one row schema for sorting, filtering, and every renderer"
                                .to_string(),
                            fix,
                            Some(span),
                        ));
                    }
                }
                web_expect!(2, web_callback(row.clone(), Type::String));
                Some(web_apply("WebTableColumn", row))
            }
            ("core.web.table", "sort") => {
                let Some(row) = hinted.or_else(|| {
                    args.get_mut(0)
                        .and_then(|arg| self.infer(&mut arg.expr))
                        .map(|ty| web_list_element!(ty))
                }) else {
                    return None;
                };
                web_expect!(0, Type::List(Box::new(row.clone())));
                web_expect!(1, Type::Bool);
                web_expect!(2, web_callback(row.clone(), Type::String));
                Some(Type::List(Box::new(row)))
            }
            ("core.web.table", "filter") => {
                let Some(row) = hinted.or_else(|| {
                    args.get_mut(0)
                        .and_then(|arg| self.infer(&mut arg.expr))
                        .map(|ty| web_list_element!(ty))
                }) else {
                    return None;
                };
                web_expect!(0, Type::List(Box::new(row.clone())));
                web_expect!(1, web_callback(row.clone(), Type::Bool));
                Some(Type::List(Box::new(row)))
            }
            ("core.web.table", "page") => {
                let Some(row) = hinted.or_else(|| {
                    args.get_mut(0)
                        .and_then(|arg| self.infer(&mut arg.expr))
                        .map(|ty| web_list_element!(ty))
                }) else {
                    return None;
                };
                web_expect!(0, Type::List(Box::new(row.clone())));
                web_expect!(1, Type::Int);
                web_expect!(2, Type::Int);
                Some(web_apply("WebTablePage", row))
            }
            ("core.web.table", "new_keyed") => {
                web_expect!(0, Type::String);
                let row = hinted.clone().or_else(|| {
                    args.get_mut(1)
                        .and_then(|arg| self.infer(&mut arg.expr))
                        .and_then(|ty| match ty {
                            Type::List(inner) => Some(*inner),
                            _ => None,
                        })
                }).unwrap_or(Type::Int);
                web_expect!(1, Type::List(Box::new(row.clone())));
                web_expect!(2, web_callback(row.clone(), Type::String));
                Some(web_apply("WebTable", row))
            }
            ("core.web.table", "with_column") => {
                let row = web_handle_element!(0, "WebTable");
                web_expect!(0, web_apply("WebTable", row.clone()));
                web_expect!(1, web_apply("WebTableColumn", row.clone()));
                Some(web_apply("WebTable", row))
            }
            ("core.web.table", "with_server_page") => {
                let row = web_handle_element!(0, "WebTable");
                web_expect!(0, web_apply("WebTable", row.clone()));
                web_expect!(1, web_callback(
                    Type::Named("WebTableState".to_string()),
                    web_callback_result(web_apply("WebTablePage", row.clone())),
                ));
                Some(web_apply("WebTable", row))
            }
            ("core.web.table", "state" | "facts" | "keys" | "page_state") => {
                let row = web_handle_element!(0, "WebTable");
                web_expect!(0, web_apply("WebTable", row.clone()));
                Some(match name {
                    "state" => Type::Named("WebTableState".to_string()),
                    "facts" => Type::String,
                    "keys" => web_result(Type::List(Box::new(Type::String))),
                    "page_state" => web_result(web_apply("WebTablePage", row)),
                    _ => unreachable!(),
                })
            }
            ("core.web.table", "sort_by" | "filter_by") => {
                let row = web_handle_element!(0, "WebTable");
                web_expect!(0, web_apply("WebTable", row.clone()));
                web_expect!(1, Type::String);
                match name {
                    "sort_by" => web_expect!(2, Type::Named("WebTableSortDirection".to_string())),
                    "filter_by" => web_expect!(2, Type::String),
                    _ => unreachable!(),
                }
                Some(web_apply("WebTable", row))
            }
            ("core.web.table", "paginate") => {
                let row = web_handle_element!(0, "WebTable");
                web_expect!(0, web_apply("WebTable", row.clone()));
                web_expect!(1, Type::Int);
                web_expect!(2, Type::Int);
                Some(web_apply("WebTable", row))
            }
            ("core.web.table", "set_rows") => {
                let row = web_handle_element!(0, "WebTable");
                web_expect!(0, web_apply("WebTable", row.clone()));
                web_expect!(1, Type::List(Box::new(row)));
                Some(web_result(unit_ty()))
            }
            ("core.web.table", "set_selected" | "toggle_selection" | "remove_row") => {
                let row = web_handle_element!(0, "WebTable");
                web_expect!(0, web_apply("WebTable", row.clone()));
                web_expect!(1, Type::String);
                if name == "set_selected" {
                    web_expect!(2, Type::Bool);
                }
                Some(match name {
                    "set_selected" | "toggle_selection" => web_result(web_apply("WebTable", row)),
                    "remove_row" => web_result(row),
                    _ => unreachable!(),
                })
            }
            ("core.web.table", "focus" | "clear_focus" | "focused_key") => {
                let row = web_handle_element!(0, "WebTable");
                web_expect!(0, web_apply("WebTable", row.clone()));
                if name == "focus" {
                    web_expect!(1, Type::String);
                }
                Some(match name {
                    "focus" | "clear_focus" => web_apply("WebTable", row),
                    "focused_key" => Type::Option(Box::new(Type::String)),
                    _ => unreachable!(),
                })
            }
            ("core.web.table", "clear_selection" | "selected_keys" | "selected_rows"
                | "first_page" | "next_page" | "last_page") => {
                let row = web_handle_element!(0, "WebTable");
                web_expect!(0, web_apply("WebTable", row.clone()));
                Some(match name {
                    "clear_selection" | "first_page" | "next_page" => {
                        if name == "clear_selection" {
                            web_apply("WebTable", row)
                        } else {
                            web_apply("WebTable", row)
                        }
                    }
                    "last_page" => web_result(web_apply("WebTable", row)),
                    "selected_keys" => Type::List(Box::new(Type::String)),
                    "selected_rows" => web_result(Type::List(Box::new(web_apply(
                        "WebTableRow",
                        row,
                    )))),
                    _ => unreachable!(),
                })
            }
            ("core.web.table", "insert_row") => {
                let row = web_handle_element!(0, "WebTable");
                web_expect!(0, web_apply("WebTable", row.clone()));
                web_expect!(1, row);
                Some(web_result(Type::String))
            }
            ("core.web.table", "replace_row" | "update_row") => {
                let row = web_handle_element!(0, "WebTable");
                web_expect!(0, web_apply("WebTable", row.clone()));
                web_expect!(1, Type::String);
                if name == "replace_row" {
                    web_expect!(2, row);
                } else {
                    web_expect!(2, web_callback(row.clone(), row));
                }
                Some(web_result(unit_ty()))
            }
            ("core.web.table", "visible_rows") => {
                let row = web_handle_element!(0, "WebTable");
                web_expect!(0, web_apply("WebTable", row.clone()));
                web_expect!(1, Type::Named("WebVirtualPlan".to_string()));
                Some(web_result(Type::List(Box::new(web_apply("WebTableRow", row)))))
            }
            ("core.web.virtual", "slice") => {
                let Some(row) = hinted.or_else(|| {
                    args.get_mut(0)
                        .and_then(|arg| self.infer(&mut arg.expr))
                        .map(|ty| web_list_element!(ty))
                }) else {
                    return None;
                };
                web_expect!(0, Type::List(Box::new(row.clone())));
                web_expect!(1, Type::Named("WebVirtualWindow".to_string()));
                Some(Type::List(Box::new(row)))
            }
            ("core.web.virtual", "window") => {
                for index in 0..5 {
                    web_expect!(index, Type::Int);
                }
                Some(Type::Named("WebVirtualWindow".to_string()))
            }
            ("core.web.virtual", "window_measured") => {
                for index in 0..5 {
                    web_expect!(index, Type::Int);
                }
                web_expect!(5, Type::List(Box::new(Type::Int)));
                Some(Type::Named("WebVirtualWindow".to_string()))
            }
            ("core.web.virtual", "indices") => {
                web_expect!(0, Type::Named("WebVirtualWindow".to_string()));
                Some(Type::List(Box::new(Type::Int)))
            }
            ("core.web.virtual", "plan") => {
                for index in 0..6 {
                    web_expect!(index, Type::Int);
                }
                Some(Type::Named("WebVirtualPlan".to_string()))
            }
            ("core.web.virtual", "plan_measured") => {
                for index in 0..6 {
                    web_expect!(index, Type::Int);
                }
                web_expect!(
                    6,
                    Type::List(Box::new(Type::Tuple(vec![
                        ("index".to_string(), Box::new(Type::Int)),
                        ("size".to_string(), Box::new(Type::Int)),
                    ])))
                );
                Some(Type::Named("WebVirtualPlan".to_string()))
            }
            ("core.web.virtual", "plan_from_sizes") => {
                for index in 0..6 {
                    web_expect!(index, Type::Int);
                }
                web_expect!(6, Type::List(Box::new(Type::Int)));
                Some(Type::Named("WebVirtualPlan".to_string()))
            }
            ("core.web.virtual", "plan_measure") => {
                web_expect!(0, Type::Named("WebVirtualPlan".to_string()));
                web_expect!(1, Type::Int);
                web_expect!(2, Type::Int);
                Some(Type::Named("WebVirtualPlan".to_string()))
            }
            ("core.web.virtual", "plan_slice") => {
                let Some(row) = hinted.clone().or_else(|| {
                    args.get_mut(0)
                        .and_then(|arg| self.infer(&mut arg.expr))
                        .map(|ty| web_list_element!(ty))
                }) else {
                    return None;
                };
                web_expect!(0, Type::List(Box::new(row.clone())));
                web_expect!(1, Type::Named("WebVirtualPlan".to_string()));
                Some(Type::List(Box::new(row)))
            }
            ("core.web.virtual", "plan_indices") => {
                web_expect!(0, Type::Named("WebVirtualPlan".to_string()));
                Some(Type::List(Box::new(Type::Int)))
            }
            ("core.web.virtual", "plan_viewport") => {
                web_expect!(0, Type::Named("WebVirtualPlan".to_string()));
                Some(Type::Named("WebVirtualPlanViewport".to_string()))
            }
            ("core.web.virtual", "plan_viewport_state") => {
                web_expect!(0, Type::Named("WebVirtualPlanViewport".to_string()));
                Some(Type::Named("WebVirtualPlan".to_string()))
            }
            ("core.web.virtual", "plan_scroll_to") => {
                web_expect!(0, Type::Named("WebVirtualPlanViewport".to_string()));
                web_expect!(1, Type::Int);
                Some(unit_ty())
            }
            ("core.web.virtual", "plan_resize") => {
                web_expect!(0, Type::Named("WebVirtualPlanViewport".to_string()));
                web_expect!(1, Type::Int);
                web_expect!(2, Type::Int);
                Some(unit_ty())
            }
            ("core.web.virtual", "plan_viewport_measure") => {
                web_expect!(0, Type::Named("WebVirtualPlanViewport".to_string()));
                web_expect!(1, Type::Int);
                web_expect!(2, Type::Int);
                Some(unit_ty())
            }
            ("core.web.virtual", "plan_facts") => {
                web_expect!(0, Type::Named("WebVirtualPlan".to_string()));
                Some(Type::String)
            }
            ("core.web.store", "new") => {
                web_expect!(0, Type::String);
                let value = hinted.unwrap_or_else(|| {
                    args.get_mut(1)
                        .and_then(|arg| self.infer(&mut arg.expr))
                        .unwrap_or(Type::Int)
                });
                web_expect!(1, value.clone());
                Some(web_apply("WebStore", value))
            }
            ("core.web.store", "with_history") => {
                web_expect!(0, Type::String);
                let value = hinted.unwrap_or_else(|| {
                    args.get_mut(1)
                        .and_then(|arg| self.infer(&mut arg.expr))
                        .unwrap_or(Type::Int)
                });
                web_expect!(1, value.clone());
                web_expect!(2, Type::Int);
                Some(web_apply("WebStore", value))
            }
            ("core.web.store", "transaction") => {
                let value = hinted.unwrap_or_else(|| {
                    args.get_mut(0)
                        .and_then(|arg| self.infer(&mut arg.expr))
                        .and_then(|ty| match ty {
                            Type::Apply { name, args }
                                if name == "WebStore" && args.len() == 1 =>
                            {
                                Some(args[0].clone())
                            }
                            _ => None,
                        })
                        .unwrap_or(Type::Int)
                });
                web_expect!(0, web_apply("WebStore", value.clone()));
                web_expect!(1, Type::String);
                web_expect!(2, Type::List(Box::new(Type::String)));
                web_expect!(3, web_callback(value.clone(), unit_ty()));
                Some(web_apply("WebStoreTransaction", value))
            }
            ("core.web.store", "update" | "batch") => {
                let value = web_handle_element!(0, "WebStore");
                web_expect!(0, web_apply("WebStore", value.clone()));
                web_expect!(1, Type::String);
                web_expect!(2, Type::List(Box::new(Type::String)));
                web_expect!(3, web_callback(value.clone(), unit_ty()));
                Some(web_apply("WebStoreTransaction", value))
            }
            ("core.web.store", "set" | "set_state") => {
                let value = web_handle_element!(0, "WebStore");
                web_expect!(0, web_apply("WebStore", value.clone()));
                web_expect!(1, value.clone());
                Some(web_apply("WebStoreTransaction", value))
            }
            ("core.web.store", "optimistic" | "patch") => {
                let value = web_handle_element!(0, "WebStore");
                web_expect!(0, web_apply("WebStore", value.clone()));
                web_expect!(1, Type::String);
                web_expect!(2, Type::List(Box::new(Type::String)));
                web_expect!(3, web_callback(value.clone(), unit_ty()));
                Some(web_apply("WebStorePatch", value))
            }
            ("core.web.store", "patch_generation" | "patch_transaction" | "patch_active"
                | "patch_commit" | "patch_rollback") => {
                let value = web_handle_element!(0, "WebStorePatch");
                web_expect!(0, web_apply("WebStorePatch", value.clone()));
                Some(match name {
                    "patch_generation" => Type::Int,
                    "patch_transaction" | "patch_commit" => {
                        web_apply("WebStoreTransaction", value)
                    }
                    "patch_active" => Type::Bool,
                    "patch_rollback" => Type::Option(Box::new(value)),
                    _ => unreachable!(),
                })
            }
            ("core.web.store", "value" | "signal" | "state_signal" | "back" | "forward" | "history" | "events"
                | "clear_history" | "history_enabled" | "history_limit" | "cursor"
                | "current_generation" | "inspect" | "facts_json" | "event_json") => {
                let value = web_handle_element!(0, "WebStore");
                web_expect!(0, web_apply("WebStore", value.clone()));
                Some(match name {
                    "value" => value,
                    "signal" | "state_signal" => web_apply("Signal", value),
                    "back" | "forward" => Type::Option(Box::new(value)),
                    "history" => Type::List(Box::new(web_apply("WebStoreTransaction", value))),
                    "events" => Type::List(Box::new(Type::Named("WebStoreEvent".to_string()))),
                    "clear_history" => unit_ty(),
                    "history_enabled" => Type::Bool,
                    "history_limit" | "cursor" | "current_generation" => Type::Int,
                    "inspect" => web_apply("WebStoreInspection", value),
                    "facts_json" | "event_json" => Type::String,
                    _ => unreachable!(),
                })
            }
            ("core.web.store", "jump" | "scrub" | "restore" | "history_at" | "events_since"
                | "set_history_limit") => {
                let value = web_handle_element!(0, "WebStore");
                web_expect!(0, web_apply("WebStore", value.clone()));
                web_expect!(1, Type::Int);
                Some(match name {
                    "jump" | "scrub" | "restore" => Type::Option(Box::new(value)),
                    "history_at" => Type::Option(Box::new(web_apply(
                        "WebStoreTransaction",
                        value,
                    ))),
                    "events_since" => Type::List(Box::new(Type::Named("WebStoreEvent".to_string()))),
                    "set_history_limit" => unit_ty(),
                    _ => unreachable!(),
                })
            }
            ("core.web.store", "subscribe") => {
                let value = web_handle_element!(0, "WebStore");
                web_expect!(0, web_apply("WebStore", value.clone()));
                web_expect!(1, web_callback(value, unit_ty()));
                Some(Type::Named("WebStoreSubscription".to_string()))
            }
            ("core.web.store", "subscribe_selector") => {
                let value = web_handle_element!(0, "WebStore");
                let selected = args
                    .get_mut(1)
                    .and_then(|arg| self.infer(&mut arg.expr))
                    .and_then(|ty| match ty {
                        Type::Fn {
                            params,
                            ret: Some(ret),
                            ..
                        } if params.len() == 1 => Some(*ret),
                        _ => None,
                    })
                    .unwrap_or(Type::Int);
                web_expect!(0, web_apply("WebStore", value.clone()));
                web_expect!(1, web_callback(value, selected.clone()));
                web_expect!(2, web_callback(selected, unit_ty()));
                Some(Type::Named("WebStoreSubscription".to_string()))
            }
            ("core.web.store", "subscription_unsubscribe" | "subscription_active") => {
                web_expect!(0, Type::Named("WebStoreSubscription".to_string()));
                Some(if name == "subscription_unsubscribe" {
                    unit_ty()
                } else {
                    Type::Bool
                })
            }
            ("core.web.store", "derived" | "selector") => {
                let value = web_handle_element!(0, "WebStore");
                let selected = args
                    .get_mut(1)
                    .and_then(|arg| self.infer(&mut arg.expr))
                    .and_then(|ty| match ty {
                        Type::Fn {
                            params,
                            ret: Some(ret),
                            ..
                        } if params.len() == 1 => Some(*ret),
                        _ => None,
                    })
                    .unwrap_or(Type::Int);
                web_expect!(0, web_apply("WebStore", value.clone()));
                web_expect!(1, web_callback(value, selected.clone()));
                Some(web_apply("Derived", selected))
            }
            _ => unreachable!("web generic call was checked above"),
        }
    }
}

impl<'a> Checker<'a> {
    /// D-MODEL-PACKAGE1=A: resolve one source-declared model trait without
    /// inventing a universal `Embedder` type. Qualified source names select a
    /// single imported module; bare names must have one unambiguous trait owner.
    fn model_trait_owner(&self, raw: &Type) -> Result<(usize, String), &'static str> {
        let (qualified, leaf) = match raw {
            Type::Named(name) => {
                let (qualified, leaf) = name.rsplit_once('.').map_or((None, name.as_str()), |(q, l)| {
                    (Some(q), l)
                });
                (qualified, leaf)
            }
            Type::TraitObject(names) if names.len() == 1 => (None, names[0].as_str()),
            _ => return Err("model.open expects one trait type argument"),
        };
        let mut candidates = Vec::new();
        if let Some(alias) = qualified {
            let Some(&owner) = self.imports.get(alias) else {
                return Err("model trait module is not imported");
            };
            if self
                .modules
                .is_some_and(|modules| modules[owner].trait_reg.traits.contains_key(leaf))
            {
                candidates.push((owner, leaf.to_string()));
            }
        } else {
            if self.trait_reg.traits.contains_key(leaf) {
                candidates.push((self.module_idx, leaf.to_string()));
            }
            if let Some(modules) = self.modules {
                for (owner, module) in modules.iter().enumerate() {
                    if owner != self.module_idx
                        && module.trait_reg.traits.contains_key(leaf)
                    {
                        candidates.push((owner, leaf.to_string()));
                    }
                }
            }
        }
        candidates.sort_unstable();
        candidates.dedup();
        match candidates.as_slice() {
            [(owner, name)] => Ok((*owner, name.clone())),
            [] => Err("model trait is not an exported source declaration"),
            _ => Err("model trait name is ambiguous"),
        }
    }

    fn infer_model_open(
        &mut self,
        span: Span,
        type_args: &[Type],
        args: &mut [crate::AST::CallArg],
    ) -> Option<Type> {
        let invalid = |checker: &mut Self, message: String, detail: String, fix: String| {
            checker
                .diags
                .push(Diagnostic::error("E-MODEL-SIGNATURE", message, detail, fix, Some(span)));
            None
        };
        if args.len() != 1 {
            self.diags
                .push(wrong_core_arity("open", 1, args.len(), span));
            for arg in args {
                self.infer(&mut arg.expr);
            }
            return None;
        }
        let Some(raw_trait) = exactly_one_type_arg(self, "models.open", type_args, span) else {
            self.infer(&mut args[0].expr);
            return None;
        };
        self.expect_core_arg("open", 0, &Type::String, &mut args[0]);
        let Some(output) = literal_string_value(&args[0].expr) else {
            return invalid(
                self,
                "`models.open` needs a literal output name".to_string(),
                "model output selection is checked before provider execution".to_string(),
                "write `models.open<Trait>(\"output\")` with a package output name".to_string(),
            );
        };
        let (owner, trait_name) = match self.model_trait_owner(&raw_trait) {
            Ok(owner) => owner,
            Err(reason) => {
                return invalid(
                    self,
                    format!("cannot bind model trait `{}`", raw_trait.name()),
                    reason.to_string(),
                    "export one public trait with the declared model signature and import its module".to_string(),
                )
            }
        };
        let Some(modules) = self.modules else {
            return invalid(
                self,
                "model source binding is unavailable".to_string(),
                "the checker has no loaded module graph".to_string(),
                "load the package source before opening its model output".to_string(),
            );
        };
        let items = modules[owner].items.clone();
        let Some(trait_def) = items.iter().find_map(|item| match item {
            crate::AST::Item::Trait(definition) if definition.name == trait_name => {
                Some(definition.clone())
            }
            _ => None,
        }) else {
            return invalid(
                self,
                format!("model trait `{trait_name}` has no source declaration"),
                "model signatures bind only to ordinary exported Jet traits".to_string(),
                "declare `pub trait TraitName { fn embed(self, documents: [String]) Batch !Err }`".to_string(),
            );
        };
        if !trait_def.is_pub {
            return invalid(
                self,
                format!("model trait `{trait_name}` is not public"),
                "a model provider boundary cannot expose a private trait".to_string(),
                "mark the source trait `pub`".to_string(),
            );
        }
        let Some(method) = trait_def
            .methods
            .iter()
            .find(|method| method.name == "embed")
            .cloned()
        else {
            return invalid(
                self,
                format!("model trait `{trait_name}` has no `embed` method"),
                "the checked model signature is document-to-batch".to_string(),
                "declare `fn embed(self, documents: [String]) Batch !Err`".to_string(),
            );
        };
        if method.params.len() != 2
            || method.params[0].name != "self"
            || method.params[1].name != "documents"
            || method.params[1].variadic
            || self.resolve_type(method.params[1].ty.clone())
                != Type::List(Box::new(Type::String))
        {
            return invalid(
                self,
                format!("model trait `{trait_name}` has an incompatible `embed` input"),
                "the model boundary requires `embed(self, documents: [String])`".to_string(),
                "use exactly one `[String]` documents parameter after `self`".to_string(),
            );
        }
        let Some(Type::Result { ok, err }) = method.return_type.as_ref() else {
            return invalid(
                self,
                format!("model trait `{trait_name}` has an incompatible `embed` result"),
                "provider failure must remain the explicit `!Err` result domain".to_string(),
                "declare `EmbeddingBatch !Err` as the method result".to_string(),
            );
        };
        let Some(batch_name) = model_type_leaf(ok) else {
            return invalid(
                self,
                format!("model trait `{trait_name}` has no named batch carrier"),
                "the provider must return one exported batch struct".to_string(),
                "return the exported batch carrier from `embed`".to_string(),
            );
        };
        if !matches!(err.as_ref(), Type::Named(name) if name == crate::Syntax::TYPE_ERR) {
            return invalid(
                self,
                format!("model trait `{trait_name}` has a non-`Err` failure domain"),
                "model provider failures use the ordinary `Err` domain".to_string(),
                "declare the method as `... Batch !Err`".to_string(),
            );
        }
        let matching_batches = items.iter().filter_map(|item| match item {
            crate::AST::Item::Struct(definition)
                if definition.name == batch_name && definition.is_pub =>
            {
                Some(definition)
            }
            _ => None,
        }).collect::<Vec<_>>();
        if matching_batches.len() != 1 {
            return invalid(
                self,
                format!("model batch carrier `{batch_name}` is not uniquely exported"),
                "exactly one public source struct must carry provider vectors".to_string(),
                "export exactly one `pub struct Batch` for this model trait".to_string(),
            );
        }
        let batch = matching_batches[0];
        let Some(values_ty) = batch.fields.iter().find(|field| field.name == "values") else {
            return invalid(
                self,
                format!("model batch carrier `{batch_name}` has no `values` field"),
                "the provider returns one dense floating-point vector per document".to_string(),
                "add a public `values: [[Float]]` field to the batch carrier".to_string(),
            );
        };
        if self.resolve_type(values_ty.ty.clone())
            != Type::List(Box::new(Type::List(Box::new(Type::Float))))
        {
            return invalid(
                self,
                format!("model batch carrier `{batch_name}` has an incompatible `values` field"),
                "model providers expose checked dense vectors as `[[Float]]`".to_string(),
                "declare `pub values: [[Float]]` on the batch carrier".to_string(),
            );
        }
        let Some(space_ty) = batch.fields.iter().find(|field| field.name == "space") else {
            return invalid(
                self,
                format!("model batch carrier `{batch_name}` has no `space` witness"),
                "embedding vectors must carry their checked model-space identity".to_string(),
                "add a private `space: Space` field to the batch carrier".to_string(),
            );
        };
        if space_ty.is_pub || space_ty.is_package_pub {
            return invalid(
                self,
                format!("model batch carrier `{batch_name}` exposes its space witness"),
                "safe consumers must not retag a provider-created embedding space".to_string(),
                "keep the `space` field private and expose only checked accessors".to_string(),
            );
        }
        let Some(space_name) = model_type_leaf(&space_ty.ty) else {
            return invalid(
                self,
                format!("model batch carrier `{batch_name}` has an unnamed space type"),
                "the space witness must be one exported source struct".to_string(),
                "type the private `space` field as the exported space carrier".to_string(),
            );
        };
        let matching_spaces = items.iter().filter_map(|item| match item {
            crate::AST::Item::Struct(definition)
                if definition.name == space_name && definition.is_pub =>
            {
                Some(definition)
            }
            _ => None,
        }).collect::<Vec<_>>();
        if matching_spaces.len() != 1 {
            return invalid(
                self,
                format!("model space carrier `{space_name}` is not uniquely exported"),
                "exactly one public source struct must carry model identity".to_string(),
                "export exactly one `pub struct Space` for this model trait".to_string(),
            );
        }
        let space = matching_spaces[0];
        for (required, expected) in [
            ("model_digest", Type::String),
            ("dimension", Type::Int),
            ("metric", Type::String),
            ("normalization", Type::String),
        ] {
            let Some(field) = space.fields.iter().find(|field| field.name == required) else {
                return invalid(
                    self,
                    format!("model space carrier `{space_name}` is missing `{required}`"),
                    "the embedding-space identity is part of the checked carrier".to_string(),
                    "add the four private model identity fields".to_string(),
                );
            };
            if field.is_pub || field.is_package_pub {
                return invalid(
                    self,
                    format!("model space field `{required}` is public"),
                    "safe consumers cannot relabel model identity metadata".to_string(),
                    "keep model-space identity fields private".to_string(),
                );
            }
            if self.resolve_type(field.ty.clone()) != expected {
                return invalid(
                    self,
                    format!("model space field `{required}` has an incompatible type"),
                    "model identity fields have fixed String/Int carrier types".to_string(),
                    format!("declare `{required}` with the checked model-space type"),
                );
            }
        }
        let matching_outputs = self
            .model_outputs
            .iter()
            .filter(|fact| {
                fact.output == output
                    && fact.signature_name.as_deref() == Some(trait_name.as_str())
            })
            .collect::<Vec<_>>();
        if matching_outputs.is_empty() {
            return invalid(
                self,
                format!("model output `{output}` does not bind trait `{trait_name}`"),
                "the package output's `name` must select exactly one source trait".to_string(),
                format!("declare `.Model {{ name: \"{trait_name}\", ... }}` for this output"),
            );
        }
        if matching_outputs.len() != 1 {
            return invalid(
                self,
                format!("model output `{output}` binds trait `{trait_name}` more than once"),
                "missing, ambiguous, and conflicting model exports fail before execution".to_string(),
                "keep one package output and one source trait binding".to_string(),
            );
        }
        self.record_effect("IO", span);
        Some(result_ty(
            Type::TraitObject(vec![trait_name]),
            Type::Named(crate::Syntax::TYPE_ERR.to_string()),
        ))
    }

    fn infer_browser_test_core_call(
        &mut self,
        module: &str,
        name: &str,
        span: Span,
        args: &mut [crate::AST::CallArg],
    ) -> Option<Type> {
        if module != "core.web.browser" {
            return None;
        }
        let browser_error = Type::Named("BrowserError".to_string());
        let config = Type::Named("BrowserTestConfig".to_string());
        let fixture = Type::Named("BrowserTestFixture".to_string());
        let report = Type::Named("BrowserTestReport".to_string());
        let case = Type::Named("BrowserTestCase".to_string());
        let server = Type::Named("BrowserTestServer".to_string());
        let string = Type::String;
        let int = Type::Int;
        let expected = match name {
            "config" | "config_from_env" => vec![],
            "begin_named" => vec![
                config.clone(),
                string.clone(),
                string.clone(),
                int.clone(),
                string.clone(),
                int.clone(),
                int.clone(),
            ],
            "generate_source" => vec![string.clone(), string.clone(), string.clone()],
            "selected" => vec![config.clone(), string.clone()],
            "report_new" => vec![config.clone()],
            "report_add_case" => vec![report.clone(), case],
            "report_json" | "report_text" | "report_html" => vec![report.clone()],
            "write_report" => vec![report.clone(), string.clone()],
            "report_exit_code" => vec![report.clone()],
            "server_start" => vec![config],
            "watch_changed" => vec![string, int],
            _ => return None,
        };
        let result = match name {
            "config" | "config_from_env" => Type::Result {
                ok: Box::new(Type::Named("BrowserTestConfig".to_string())),
                err: Box::new(browser_error.clone()),
            },
            "begin_named" => Type::Result {
                ok: Box::new(fixture),
                err: Box::new(browser_error.clone()),
            },
            "generate_source" => Type::Result {
                ok: Box::new(Type::String),
                err: Box::new(browser_error.clone()),
            },
            "selected" => Type::Bool,
            "report_new" => report.clone(),
            "report_add_case" => report.clone(),
            "report_json" | "report_text" | "report_html" => Type::String,
            "write_report" => Type::Result {
                ok: Box::new(unit_ty()),
                err: Box::new(browser_error.clone()),
            },
            "report_exit_code" => Type::Int,
            "server_start" => Type::Result {
                ok: Box::new(server),
                err: Box::new(browser_error.clone()),
            },
            "watch_changed" => Type::Result {
                ok: Box::new(Type::Bool),
                err: Box::new(browser_error),
            },
            _ => unreachable!(),
        };
        if args.len() != expected.len() {
            self.diags
                .push(wrong_core_arity(name, expected.len(), args.len(), span));
            for arg in args {
                self.infer(&mut arg.expr);
            }
            return Some(result);
        }
        for (index, expected) in expected.iter().enumerate() {
            self.expect_core_arg(name, index, expected, &mut args[index]);
        }
        Some(result)
    }

}

fn core_call_argument_convention(
    module: &str,
    name: &str,
    index: usize,
) -> AccessConvention {
    Syntax::core_call(module, name)
        .and_then(|row| row.signature.borrow_mask.get(index).copied())
        .map_or(AccessConvention::Read, |borrowed| {
            if borrowed {
                AccessConvention::Read
            } else {
                AccessConvention::Move
            }
        })
}

impl<'a> Checker<'a> {
    /// Infer a callback retained by a reactive/UI host. Retention owns the
    /// lambda's captures, and the host ABI uses the checked SendFn carrier;
    /// ordinary escaping inference would otherwise leave borrowed/Rc facts
    /// behind for MIR to erase.
    fn infer_retained_callback(
        &mut self,
        expr: &mut Expr,
        expected: Option<&Type>,
    ) -> Option<Type> {
        let saved_escapes = self.lambda_escapes;
        self.lambda_escapes = true;
        let ty = expected
            .map(|expected| self.infer_with_expected(expr, expected))
            .unwrap_or_else(|| self.infer(expr));
        self.lambda_escapes = saved_escapes;
        if let Some(ty) = ty.as_ref() {
            self.check_stream_callback_expr(expr, ty);
        }
        ty
    }

    pub(crate) fn infer_core_call(
        &mut self,
        module: &str,
        name: &str,
        alias: Option<&str>,
        alias_span: Span,
        span: Span,
        type_args: &[Type],
        args: &mut Vec<crate::AST::CallArg>,
    ) -> Option<Type> {
        if let Some(alias) = alias.filter(|_| core_call_is_known(module, name)) {
            self.record_import_alias_reference(alias, alias_span);
        }
        let fixed_sig =
            resolved_core_fixed_sig(module, name, type_args, span, &mut self.diags);
        if module == "core.web" && name == "form" {
            return self.infer_web_form_core_call(span, args);
        }
        if module == "core.models" && name == "open" {
            return self.infer_model_open(span, type_args, args);
        }
        // D-FRONTENDAPI1=A: the compiler surface is a read-only
        // compile-time value API. It is intentionally handled before
        // the ordinary Core effect/fixed-signature tables so it cannot become
        // a runtime or ambient fallback by accident.
        if module == "core.compiler" {
            if !matches!(
                name,
                "lex"
                    | "parse"
                    | "check"
                    | "source_map"
                    | "manifest"
                    | "package"
                    | "lock"
                    | "profiles"
            ) {
                self.diags.push(unknown_core_item(module, name, span));
                for arg in args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return Some(core_compiler_return("unknown"));
            }
            if !self.in_comptime && !self.compiler_api_allowed {
                self.diags.push(Diagnostic::error(
                        "E0956",
                        format!("`core.compiler.{name}` is compile-time only"),
                        "the compiler API exposes read-only front-end facts to build and comptime code; it is not a runtime service".to_string(),
                        "move this call into `fn build` or a `comptime` binding".to_string(),
                        Some(span),
                    ));
            }
            let package_view = matches!(name, "manifest" | "package" | "lock" | "profiles");
            if package_view {
                if !args.is_empty() {
                    self.diags.push(wrong_core_arity(name, 0, args.len(), span));
                }
                for arg in args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return Some(core_compiler_return(name));
            }
            if !matches!(args.len(), 1 | 2) {
                self.diags.push(wrong_core_arity(name, 1, args.len(), span));
            }
            if let Some(arg) = args.get_mut(0) {
                let input_type = if name == "check" {
                    Type::Named("CompilerSyntaxTree".to_string())
                } else {
                    Type::String
                };
                self.expect_core_arg(name, 0, &input_type, arg);
            }
            return Some(core_compiler_return(name));
        }
        // D-BUILDQUERY1=A: checked build graph queries are read-only values
        // available only while the selected build/comptime program runs.
        if module == "core.build" {
            if !matches!(name, "graph" | "receipt_diff") {
                self.diags.push(unknown_core_item(module, name, span));
                for arg in args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return Some(core_build_return("unknown"));
            }
            if !self.in_comptime && !self.compiler_api_allowed {
                self.diags.push(Diagnostic::error(
                    "E0956",
                    format!("`core.build.{name}` is compile-time only"),
                    "build graph values are checked facts, not a runtime service".to_string(),
                    "move this call into `fn build` or a `comptime` binding".to_string(),
                    Some(span),
                ));
            }
            let expected = if name == "graph" {
                vec![Type::Named(Syntax::TYPE_BUILD_PLAN.to_string())]
            } else {
                vec![
                    Type::Named(Syntax::TYPE_BUILD_GRAPH.to_string()),
                    Type::Named(Syntax::TYPE_BUILD_GRAPH.to_string()),
                ]
            };
            if args.len() != expected.len() {
                self.diags.push(wrong_core_arity(name, expected.len(), args.len(), span));
            }
            for (index, arg) in args.iter_mut().enumerate() {
                if let Some(expected) = expected.get(index) {
                    self.expect_core_arg(name, index, expected, arg);
                } else {
                    self.infer(&mut arg.expr);
                }
            }
            return Some(core_build_return(name));
        }
        // D-BENCH-KEEP1=A: `keep` is the one generic identity sink. Sema
        // infers its argument and returns that exact type; no engine gets
        // a second signature or value policy.
        if module == "core.prelude" && name == "keep" {
            if args.len() != 1 {
                self.diags.push(wrong_core_arity(name, 1, args.len(), span));
            }
            let value_type = args.get_mut(0).and_then(|arg| self.infer(&mut arg.expr));
            for arg in args.iter_mut().skip(1) {
                self.infer(&mut arg.expr);
            }
            return value_type;
        }
        if module == "core.tasks" && name == "channel" {
            self.diags.push(Diagnostic::error(
                "E0102",
                "`tasks.channel` is retired".to_string(),
                "channels are a readable builtin and need no module import".to_string(),
                "write `channel<T>()` or `channel<T>(capacity: 8)".to_string(),
                Some(span),
            ));
            for argument in args {
                self.infer(&mut argument.expr);
            }
            return None;
        }
        // D-SERVICE1=D: a tree value is the typed topology root. The
        // inline declaration form promotes ordinary function values into
        // named workers; the direct string form remains the constructor
        // used by the same typed tree API.
        if module == "core.service" && name == "tree" {
            if args.len() != 1 {
                self.diags.push(wrong_core_arity(name, 1, args.len(), span));
            }
            if let Some(arg) = args.get_mut(0) {
                self.expect_core_arg(name, 0, &Type::String, arg);
                if let Expr::Str(parts, _) = &arg.expr {
                    let literal = parts
                        .iter()
                        .map(|part| match part {
                            crate::AST::StrPart::Lit(value) => Some(value.as_str()),
                            crate::AST::StrPart::Interp(..) => None,
                        })
                        .collect::<Option<Vec<_>>>()
                        .map(|parts| parts.concat());
                    if let Some(value) = literal {
                        if !jet_foundation::ServiceTree::valid_name(&value) {
                            self.diags.push(Diagnostic::error(
                                        "E0112",
                                        "service tree name is outside the checked builder shape".to_string(),
                                        "service topology names must be non-empty and visible before runtime construction".to_string(),
                                        "use a non-empty visible tree name of at most 256 bytes".to_string(),
                                        Some(arg.expr.span()),
                                    ));
                        }
                    }
                }
            }
            for arg in args.iter_mut().skip(1) {
                self.infer(&mut arg.expr);
            }
            return Some(Type::Named("ServiceTree".to_string()));
        }
        // D-SQL-SURFACE1=C: typed CSV query and in-memory query share one
        // checked row shape and one Prelude result carrier.
        if module == "core.encoding.csv" && name == "query" && !type_args.is_empty() {
            if args.len() != 2 {
                self.diags.push(wrong_core_arity(name, 2, args.len(), span));
            }
            let Some(row) = exactly_one_type_arg(self, name, type_args, span) else {
                return None;
            };
            if let Some(path) = args.get_mut(0) {
                self.expect_core_arg(name, 0, &Type::String, path);
            }
            if let Some(query) = args.get_mut(1) {
                self.check_analytics_sql_schema(&row, &query.expr);
                self.expect_core_arg(name, 1, &Type::Named("SQL".to_string()), query);
            }
            self.check_decodable(&row, span);
            self.check_encodable(&row, span);
            return Some(result_ty(
                Type::Apply {
                    name: "Query".to_string(),
                    args: vec![row],
                },
                Type::List(Box::new(Type::Named("FieldError".to_string()))),
            ));
        }
        if module == "core.data" && name == "track" {
            if args.len() != 2 {
                self.diags.push(wrong_core_arity(name, 2, args.len(), span));
            }
            let row = match args.get_mut(0).and_then(|arg| self.infer(&mut arg.expr)) {
                Some(Type::List(inner)) => *inner,
                Some(other) => {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("`data.track` needs a typed row list, not {}", other.show()),
                        "a tracked source owns one typed row shape and one nominal key".to_string(),
                        "pass a `[Row]` value and a key callback".to_string(),
                        Some(span),
                    ));
                    Type::Int
                }
                None => Type::Int,
            };
            let key = args
                .get_mut(1)
                .and_then(|callback| self.infer_query_callback(name, &row, callback))
                .unwrap_or(Type::Int);
            return Some(result_ty(
                Type::Apply {
                    name: "DataTracked".to_string(),
                    args: vec![row, key],
                },
                Type::Named("DataError".to_string()),
            ));
        }
        if module == "core.data" && name == "query" {
            if args.len() != 1 {
                self.diags.push(wrong_core_arity(name, 1, args.len(), span));
            }
            let row = match args.get_mut(0).and_then(|arg| self.infer(&mut arg.expr)) {
                Some(Type::List(inner)) => *inner,
                Some(Type::Apply {
                    name: tracked_name,
                    args: tracked_args,
                }) if tracked_name == "DataTracked" && tracked_args.len() == 2 => {
                    tracked_args[0].clone()
                }
                Some(Type::Apply { name, args }) if name == "DataStream" && args.len() == 1 => {
                    args[0].clone()
                }
                Some(other) => {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("`data.query` needs a typed row list or stream, not {}", other.show()),
                        "a query defers calculations over one typed row shape".to_string(),
                        "pass a `[Row]` value or `DataStream<Row>`".to_string(),
                        Some(span),
                    ));
                    Type::Int
                }
                None => Type::Int,
            };
            return Some(Type::Apply {
                name: "Query".to_string(),
                args: vec![row],
            });
        }
        // D-DATAFLOW1=A: typed stream consumers preserve the DataStream<T>
        // row shape while keeping collection fallible and one-shot.
        if module == "core.data.stream" {
            if !type_args.is_empty() {
                self.diags.push(Diagnostic::error(
                    "E0119",
                    format!("`{name}` does not take an explicit type argument"),
                    "stream operations read their row type from the DataStream<T> value"
                        .to_string(),
                    "remove the type argument and pass a concrete DataStream<T>".to_string(),
                    Some(span),
                ));
            }
            let expected = 1;
            if args.len() != expected {
                self.diags.push(wrong_core_arity(name, expected, args.len(), span));
            }
            let row = match args.get_mut(0).and_then(|arg| self.infer(&mut arg.expr)) {
                Some(Type::Apply { name, args }) if name == "DataStream" && args.len() == 1 => {
                    args.into_iter().next().unwrap_or(Type::Named("Unknown".to_string()))
                }
                Some(other) => {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("`{name}` expects a `DataStream<T>` handle, got {}", other.show()),
                        "stream operations preserve the row type carried by the stream".to_string(),
                        "pass a `DataStream<T>` value as the first argument".to_string(),
                        Some(span),
                    ));
                    Type::Named("Unknown".to_string())
                }
                None => Type::Named("Unknown".to_string()),
            };
            for arg in args.iter_mut().skip(1) {
                self.infer(&mut arg.expr);
            }
            return Some(match name {
                "next" => result_ty(
                    Type::Option(Box::new(row)),
                    Type::Named("DataError".to_string()),
                ),
                "collect" => result_ty(
                    Type::List(Box::new(row)),
                    Type::Named("DataError".to_string()),
                ),
                "cancel" => unit_ty(),
                _ => {
                    self.diags.push(unknown_core_item(module, name, span));
                    unit_ty()
                }
            });
        }
        // D-EFF1: record the effect this Core call contributes to the enclosing
        // function's inferred set (erased in codegen; purely a sema fact).
        if module == "core.compute" {
            self.fx_compute_calls
                .push(crate::Sema::Effects::ComputeCallFact {
                    method: name.to_string(),
                    span,
                });
        }
        // Plain calls carry their erased arity in the foundation record.
        // Keep the richer Jet type construction below in sema, but make
        // every consumer reject a row-shaped call from the same fact.
        if Syntax::core_call(module, name).is_some() {
            // A Core parameter contract may omit trailing defaulted slots at
            // the source boundary. Its binder below fills those slots before
            // the fixed ABI reaches any engine, so validate the raw
            // arity only for rows without a contract.
            if super::core_param_contract(module, name).is_none() {
                if let Err(Syntax::CoreCallProjectionError::Arity { expected, actual }) =
                    Syntax::core_call_projection(
                        module,
                        name,
                        Syntax::CoreCallCoverage::SEMA,
                        args.len(),
                    )
                {
                    self.diags
                        .push(wrong_core_arity(name, expected, actual, span));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
            }
        }
        if let Some(e) = core_effect_for_call(module, name) {
            if self.deterministic_world_depth > 0
                && (!matches!(e, Effect::Time | Effect::Rand)
                    || module == "core.crypto.random")
            {
                let api = format!("{}.{}", module_short_name(module), name);
                self.reject_uncontrolled_deterministic_world(&api, span);
            }
            // D-EFFTREE1: keep the broad root for existing transaction and
            // diagnostic behavior. `Time.Wait` is consumed by ordinary callback
            // effect solving; it is not a callback-specific allowlist.
            self.record_effect(e.name(), span);
            // D-TXN2: an irreversible effect (Net/FS/Exec — a network/file/
            // subprocess effect) can't be rolled back, so it is rejected when it
            // occurs directly inside a `#Transact { … }` block (E0746). The fix
            // is to move it after the block, or register it via
            // `name.on_commit(() => { … })` so it runs only on a clean commit.
            // D-CONC-SHARE1=A: the wall counts only transactions the
            // author opened, never a synthesized one-statement commit.
            if self.txn_wall_depth > 0 && is_irreversible_effect(e) {
                let api = format!("{}.{}", module_short_name(module), name);
                self.diags.push(e0746(&api, e, span));
            }
        }
        if let Some(leaf) = core_effect_leaf_for_call(module, name) {
            self.record_effect(leaf, span);
        }
        // E2-M15 / E3301: reject OS-dependent APIs on no-OS targets.
        if self.no_os && is_no_os_forbidden(module) {
            let api = format!("{}.{}", module_short_name(module), name);
            let hint = no_os_hint(module);
            self.diags.push(e3301(&api, hint, span));
            // Still infer args to avoid cascading errors.
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        }
        // #1465 / I1: POSIX process/session control is expert-tier.
        if module == "core.sys"
            && matches!(
                name,
                "fork"
                    | "setuid"
                    | "setgid"
                    | "setpgid"
                    | "setpgrp"
                    | "setsid"
                    | "initgroups"
                    | "kill"
                    | "wait"
                    | "waitpid"
                    | "pipe"
                    | "close_fd"
                    | "mkfifo"
                    | "umask"
                    | "getpriority"
                    | "setpriority"
                    | "utime"
                    | "atexit"
                    | "stop"
            )
            && !self.in_unsafe
        {
            self.diags
                .push(super::alloc_ptrs::e3101(&format!("os.{name}"), span));
        }
        // E2-M16 / E3403: a `pure fn` cannot reach a non-deterministic std call
        // (time/random). `jet eval --pure` requires every fn to be `pure`, so
        // this covers the --pure path too.
        // D-DET1: `assume_deterministic { … }` (det_suppress > 0) suspends the
        // determinism rejection — the expert escape hatch.
        if self.in_pure && self.det_suppress == 0 && is_nondeterministic_core(module, name) {
            let api = format!("{}.{}", module_short_name(module), name);
            self.diags.push(Diagnostic::e3403(&api, Some(span)));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            // Return the declared type so the call site doesn't cascade.
            return fixed_sig.as_ref().and_then(|(_, ret)| ret.clone());
        }
        // D-STDIN1=A / E3401: `pure fn` cannot read from stdin.
        if self.in_pure
            && self.det_suppress == 0
            && jet_foundation::Authority::is_impure_core(module, name)
        {
            let api = format!("{}.{}", module_short_name(module), name);
            self.diags
                .push(e3401(&self.fn_name.clone(), &api, &[], span));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return fixed_sig.as_ref().and_then(|(_, ret)| ret.clone());
        }
        // D-STRUCT-LIFE1=A: Core aliases use marker metadata attached to
        // their ordinary declaration row. This is the same lifecycle
        // renderer used by user declarations; there is no Core-only
        // deprecation table or match arm.
        if type_args.is_empty() {
            if let Some(marker) = Syntax::core_marker_application(module, name) {
                let dep = crate::AST::Deprecation {
                    since: marker.since.to_string(),
                    replacement: marker.replacement.to_string(),
                    removed_in: marker.removed_in.map(str::to_string),
                };
                let item = format!("{}.{}", module_short_name(module), name);
                self.check_deprecation(&item, &dep, span);
            }
        }
        // D-EFF1: `#Pure` is the empty effect set, so any effectful Core call —
        // `FS`/`Net`/`Env`/`Exec`/`DB`/`Log`/`IO` — is impure inside a `#Pure fn`.
        // (Time/Rand return early above via E3403; stdin via the E3401 check
        // above, so this catches the remaining effect-carrying Core modules.)
        if self.in_pure && self.det_suppress == 0 && core_effect_for_call(module, name).is_some() {
            let api = format!("{}.{}", module_short_name(module), name);
            self.diags
                .push(e3401(&self.fn_name.clone(), &api, &[], span));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return fixed_sig.as_ref().and_then(|(_, ret)| ret.clone());
        }
        // D-A11YGATE1=B (c134 Phase 6): E2930 (empty accessible label on an
        // interactive-role node) is checked here, on the raw call-site args,
        // independent of `sig`/arity checking below. It always runs — the
        // diagnostic is `Severity::Lint`, and CLI layers decide whether to
        // show it (`jet lint --a11y`) or suppress it (`jet build`/`jet run`),
        // per D-A11YGATE1's opt-in-surface, never-blocking contract.
        if module == "core.ui" && name == "node_role" {
            self.check_a11y_node_role_label(args, span);
        }
        if module == "core.auth" && matches!(name, "verify_jwt" | "verify_paseto") {
            let required = if name == "verify_jwt" {
                &[
                    (1, "key"),
                    (2, "audience"),
                    (3, "issuer"),
                    (4, "clock_skew"),
                ][..args.len().min(5).saturating_sub(1)]
            } else {
                &[
                    (1, "key"),
                    (2, "audience"),
                    (3, "issuer"),
                    (4, "clock_skew"),
                    (5, "footer"),
                    (6, "implicit"),
                ][..args.len().min(7).saturating_sub(1)]
            };
            super::net_text_time::require_exact_labels(
                &format!("auth.{name}"),
                args,
                required,
                span,
                &mut self.diags,
            );
        } else if module == "core.net.tls" && name == "client" {
            let required = match args.len() {
                3 => &[(1, "server_name"), (2, "deadline")][..],
                4 => &[(1, "server_name"), (2, "config"), (3, "deadline")][..],
                _ => &[],
            };
            super::net_text_time::require_exact_labels(
                "tls.client",
                args,
                required,
                span,
                &mut self.diags,
            );
        } else if module == "core.net" && name == "unix_connect" && args.len() == 2 {
            super::net_text_time::require_exact_labels(
                "net.unix_connect",
                args,
                &[(1, "deadline")],
                span,
                &mut self.diags,
            );
        }
        if matches!(module, "app" | "core.web") && name == "sync" && args.len() == 2 {
            super::net_text_time::require_exact_labels(
                "app.sync",
                args,
                &[(1, "over")],
                span,
                &mut self.diags,
            );
        }
        if module == "core.service" && name == "runtime" && args.len() == 2 {
            super::net_text_time::require_exact_labels(
                "service.runtime",
                args,
                &[(1, "retention")],
                span,
                &mut self.diags,
            );
        }
        if module == "core.compute"
            && matches!(name, "gradient" | "value_and_gradient" | "vjp" | "jvp")
        {
            return self.infer_compute_transform(name, span, args);
        }
        let sig = if module == "core.auth" && name == "verify_jwt" && (3..=5).contains(&args.len())
        {
            let mut params = vec![
                (AccessConvention::Read, Type::String),
                (AccessConvention::Read, Type::List(Box::new(u8_ty()))),
                (AccessConvention::Read, Type::String),
            ];
            if args.len() >= 4 {
                params.push((AccessConvention::Read, Type::String));
            }
            if args.len() >= 5 {
                params.push((AccessConvention::Read, Type::Named("Duration".to_string())));
            }
            Some((
                params,
                Some(result_ty(
                    Type::Named("Claims".to_string()),
                    Type::Named("AuthError".to_string()),
                )),
            ))
        } else if module == "core.auth" && name == "verify_paseto" && (3..=7).contains(&args.len())
        {
            let mut params = vec![
                (AccessConvention::Read, Type::String),
                (AccessConvention::Read, Type::List(Box::new(u8_ty()))),
                (AccessConvention::Read, Type::String),
            ];
            if args.len() >= 4 {
                params.push((AccessConvention::Read, Type::String));
            }
            if args.len() >= 5 {
                params.push((AccessConvention::Read, Type::Named("Duration".to_string())));
            }
            if args.len() >= 6 {
                params.push((AccessConvention::Read, Type::List(Box::new(u8_ty()))));
            }
            if args.len() >= 7 {
                params.push((AccessConvention::Read, Type::List(Box::new(u8_ty()))));
            }
            Some((
                params,
                Some(result_ty(
                    Type::Named("Claims".to_string()),
                    Type::Named("AuthError".to_string()),
                )),
            ))
        } else if module == "core.net.tls" && name == "client" && args.len() == 4 {
            Some((
                vec![
                    (AccessConvention::Move, Type::Named("TcpStream".to_string())),
                    (AccessConvention::Read, Type::String),
                    (
                        AccessConvention::Read,
                        Type::Named("TLSClientConfig".to_string()),
                    ),
                    (AccessConvention::Read, Type::Named("Duration".to_string())),
                ],
                Some(result_ty(
                    Type::Named("TLSStream".to_string()),
                    Type::Named("NetError".to_string()),
                )),
            ))
        } else if module == "core.net.tls" && name == "client" && args.len() == 3 {
            Some((
                vec![
                    (AccessConvention::Move, Type::Named("TcpStream".to_string())),
                    (AccessConvention::Read, Type::String),
                    (AccessConvention::Read, Type::Named("Duration".to_string())),
                ],
                Some(result_ty(
                    Type::Named("TLSStream".to_string()),
                    Type::Named("NetError".to_string()),
                )),
            ))
        } else if module == "core.net" && name == "unix_connect" && args.len() == 2 {
            Some((
                vec![
                    (AccessConvention::Read, Type::String),
                    (AccessConvention::Read, Type::Named("Duration".to_string())),
                ],
                Some(result_ty(
                    Type::Named("UnixStream".to_string()),
                    Type::Named("NetError".to_string()),
                )),
            ))
        } else {
            fixed_sig
        };
        // D-UI-CLOSURE1=A: parser-owned trailing blocks are unlabeled until
        // the Core contract identifies their callback slot. Normalize that
        // one source marker before the shared binder sees label ordering.
        if module == "core.ui" {
            let callback_label = match name {
                "button" => Some("on_click"),
                "text_input" => Some("on_drop"),
                _ => None,
            };
            if let Some(callback_label) = callback_label {
                for arg in args.iter_mut().filter(|arg| arg.flags.is_trailing_block) {
                    if arg.label.is_none() {
                        arg.label = Some((callback_label.to_string(), arg.span));
                    }
                }
            }
        }
        let ui_plain_constructor = module == "core.ui"
            && ((name == "button"
                && args.len() == 1
                && args.iter().all(|arg| arg.label.is_none()))
                || (name == "text_input"
                    && args.len() == 2
                    && args.iter().all(|arg| arg.label.is_none())));
        if !ui_plain_constructor {
            if let Some(contract) = super::core_param_contract(module, name) {
            let params: Vec<crate::Sema::CallBinder::BindParam<'_>> = contract
                .iter()
                .enumerate()
                .map(|(index, param)| crate::Sema::CallBinder::BindParam {
                    label: param.label,
                    name: param.label,
                    zone: param.zone,
                    default: None,
                    convention: sig
                        .as_ref()
                        .and_then(|(params, _)| params.get(index))
                        .map(|(convention, _)| *convention)
                        .unwrap_or(AccessConvention::Read),
                    ty: sig
                        .as_ref()
                        .and_then(|(params, _)| params.get(index))
                        .map(|(_, ty)| ty),
                    variadic: false,
                    core_default: param.default,
                })
                .collect();
            if crate::Sema::CallBinder::bind_call_args(name, &params, args, span, &mut self.diags)
                .is_none()
            {
                for arg in args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return sig.and_then(|(_, ret)| ret);
            }
            self.register_binder_refs(args);
        }
        }
        // D-DX-PLUGIN1=D: the publication gate binds exactly two explicit
        // labels, infers only the value slot, and records the typed fact in
        // the existing module TypeRegistry. The selector is metadata, not a
        // runtime value to infer.
        if module == Syntax::CORE_DEVTOOLS_MODULE && name == Syntax::CORE_DEVTOOLS_PUBLISH {
            let value_type = args
                .get_mut(1)
                .and_then(|arg| self.infer(&mut arg.expr));
            crate::Sema::check_devtools_publish(
                self.package_scope,
                self.module_path,
                args,
                value_type,
                span,
                self.devtools_registry,
                self.registry,
                &mut self.diags,
            );
            return Some(unit_ty());
        }
        if let Some(web_return) =
            self.infer_web_generic_core_call(module, name, type_args, span, args)
        {
            return Some(web_return);
        }
        if let Some(browser_test_return) =
            self.infer_browser_test_core_call(module, name, span, args)
        {
            return Some(browser_test_return);
        }
        match (module, name) {
            (
                "core.crypto.vault",
                "current"
                | "versions"
                | "load"
                | "status"
                | "prepare_generate"
                | "prepare_store"
                | "prepare_rotate"
                | "prepare_retire"
                | "prepare_revoke"
                | "authorize_write"
                | "commit_generate"
                | "commit_store"
                | "commit_rotate"
                | "commit_retire"
                | "commit_revoke"
                | "export_to_recipients"
                | "export_to_passphrase"
                | "prepare_import_wrapped"
                | "authorize_wrapped_import"
                | "commit_import_wrapped",
            ) => {
                let inferred_from = match name {
                    "prepare_store" => 1,
                    "export_to_recipients" | "export_to_passphrase" => 0,
                    "load"
                    | "status"
                    | "prepare_retire"
                    | "prepare_revoke"
                    | "authorize_write"
                    | "commit_generate"
                    | "commit_store"
                    | "commit_rotate"
                    | "commit_retire"
                    | "commit_revoke"
                    | "authorize_wrapped_import"
                    | "commit_import_wrapped" => 0,
                    _ => usize::MAX,
                };
                let inferred_key = if type_args.is_empty() && inferred_from < args.len() {
                    self.infer(&mut args[inferred_from].expr)
                        .and_then(|ty| vault_key_arg(&ty))
                } else {
                    None
                };
                if type_args.len() > 1 || (type_args.is_empty() && inferred_key.is_none()) {
                    self.diags.push(Diagnostic::error(
                            "E0904",
                            format!("`vault.{name}` needs one vault key type"),
                            "typed vault operations are restricted to SigningKey and X25519SecretKey".to_string(),
                            format!("call it with an explicit type argument: `vault.{name}<crypto.SigningKey>(...)`"),
                            Some(span),
                        ));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                let key_ty =
                    self.resolve_type(type_args.first().cloned().or(inferred_key).unwrap());
                let key_leaf = match &key_ty {
                    Type::Named(leaf) => Some(leaf.as_str()),
                    Type::Tagged { inner, .. } => match inner.as_ref() {
                        Type::Named(leaf) => Some(leaf.as_str()),
                        _ => None,
                    },
                    _ => None,
                };
                if !key_leaf.is_some_and(|leaf| matches!(leaf, "SigningKey" | "X25519SecretKey")) {
                    self.diags.push(Diagnostic::error(
                        "E0905",
                        format!("`{}` is not a persistent vault key type", key_ty.show()),
                        "VaultKey is sealed and implemented only by SigningKey and X25519SecretKey"
                            .to_string(),
                        "use `crypto.SigningKey` or `crypto.X25519SecretKey`".to_string(),
                        Some(span),
                    ));
                }
                let apply = |name: &str| Type::Apply {
                    name: name.to_string(),
                    args: vec![key_ty.clone()],
                };
                let (params, ok): (Vec<(AccessConvention, Type)>, Type) = match name {
                    "current" => (
                        vec![(AccessConvention::Read, Type::String)],
                        Type::Option(Box::new(apply("KeyRef"))),
                    ),
                    "versions" => (
                        vec![(AccessConvention::Read, Type::String)],
                        Type::List(Box::new(apply("KeyRef"))),
                    ),
                    "load" => (
                        vec![(AccessConvention::Read, apply("KeyRef"))],
                        key_ty.clone(),
                    ),
                    "status" => (
                        vec![(AccessConvention::Read, apply("KeyRef"))],
                        Type::Named("KeyStatus".into()),
                    ),
                    "prepare_generate" | "prepare_rotate" => (
                        vec![(AccessConvention::Read, Type::String)],
                        apply("MutationPlan"),
                    ),
                    "prepare_store" => (
                        vec![
                            (AccessConvention::Read, Type::String),
                            (AccessConvention::Move, key_ty.clone()),
                        ],
                        apply("MutationPlan"),
                    ),
                    "prepare_retire" | "prepare_revoke" => (
                        vec![
                            (AccessConvention::Read, apply("KeyRef")),
                            (AccessConvention::Read, Type::String),
                        ],
                        apply("MutationPlan"),
                    ),
                    "authorize_write" => (
                        vec![
                            (AccessConvention::Read, apply("MutationPlan")),
                            (AccessConvention::Read, Type::String),
                        ],
                        apply("VaultWrite"),
                    ),
                    "export_to_recipients" => (
                        vec![
                            (AccessConvention::Read, apply("KeyRef")),
                            (
                                AccessConvention::Read,
                                Type::List(Box::new(Type::Named("X25519PublicKey".into()))),
                            ),
                        ],
                        Type::Named("WrappedVaultKey".into()),
                    ),
                    "export_to_passphrase" => (
                        vec![
                            (AccessConvention::Read, apply("KeyRef")),
                            (
                                AccessConvention::Read,
                                crate::Sema::Diagnostics::core_crypto_nominal(Type::Named(
                                    "Secret".into(),
                                )),
                            ),
                        ],
                        Type::Named("WrappedVaultKey".into()),
                    ),
                    "prepare_import_wrapped" => (
                        vec![
                            (AccessConvention::Read, Type::String),
                            (
                                AccessConvention::Read,
                                Type::Named("WrappedVaultKey".into()),
                            ),
                            (AccessConvention::Read, Type::Named("KeyUnlock".into())),
                        ],
                        apply("WrappedImportPlan"),
                    ),
                    "authorize_wrapped_import" => (
                        vec![
                            (AccessConvention::Read, apply("WrappedImportPlan")),
                            (AccessConvention::Read, Type::String),
                        ],
                        apply("VaultWrite"),
                    ),
                    "commit_import_wrapped" => (
                        vec![
                            (AccessConvention::Move, apply("VaultWrite")),
                            (AccessConvention::Move, apply("WrappedImportPlan")),
                        ],
                        apply("KeyRef"),
                    ),
                    "commit_generate" | "commit_store" => (
                        vec![
                            (AccessConvention::Move, apply("VaultWrite")),
                            (AccessConvention::Move, apply("MutationPlan")),
                        ],
                        apply("KeyRef"),
                    ),
                    "commit_rotate" => (
                        vec![
                            (AccessConvention::Move, apply("VaultWrite")),
                            (AccessConvention::Move, apply("MutationPlan")),
                        ],
                        apply("Rotation"),
                    ),
                    _ => (
                        vec![
                            (AccessConvention::Move, apply("VaultWrite")),
                            (AccessConvention::Move, apply("MutationPlan")),
                        ],
                        Type::Named("Unit".into()),
                    ),
                };
                if args.len() != params.len() {
                    self.diags
                        .push(wrong_core_arity(name, params.len(), args.len(), span));
                }
                for (i, ((convention, ty), arg)) in params.iter().zip(args.iter_mut()).enumerate() {
                    if *convention == AccessConvention::Move {
                        self.expect_core_arg_moving(name, i, ty, arg);
                        self.finish_core_call_ownership(name, i, arg, *convention, ty);
                    } else {
                        self.expect_core_arg(name, i, ty, arg);
                    }
                }
                for arg in args.iter_mut().skip(params.len()) {
                    self.infer(&mut arg.expr);
                }
                let err = if matches!(
                    name,
                    "export_to_recipients"
                        | "export_to_passphrase"
                        | "prepare_import_wrapped"
                        | "authorize_wrapped_import"
                        | "commit_import_wrapped"
                ) {
                    "KeyWrapError"
                } else {
                    "VaultError"
                };
                return Some(result_ty(ok, Type::Named(err.into())));
            }
            ("core.encoding.cbor", "parse") => {
                if !(1..=2).contains(&args.len()) {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.expect_core_arg(name, 0, &Type::List(Box::new(u8_ty())), arg);
                }
                if let Some(arg) = args.get_mut(1) {
                    self.expect_core_arg(name, 1, &Type::Named("CBOROptions".to_string()), arg);
                }
                return Some(result_ty(
                    Type::Named("DataTree".to_string()),
                    Type::Named("CBORError".to_string()),
                ));
            }
            ("core.encoding.cbor", "encode") => {
                if super::super::Edition::edition_at_least("2028") {
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.expect_core_arg(name, 0, &Type::Named("DataTree".to_string()), arg);
                }
                return Some(Type::List(Box::new(u8_ty())));
            }
            ("core.encoding.cbor", "decode") if type_args.is_empty() => {
                if !(1..=2).contains(&args.len()) {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.expect_core_arg(name, 0, &Type::List(Box::new(u8_ty())), arg);
                }
                if let Some(arg) = args.get_mut(1) {
                    self.expect_core_arg(name, 1, &Type::Named("CBOROptions".to_string()), arg);
                }
                if super::super::Edition::edition_at_least("2027") {
                    return Some(result_ty(
                        Type::Named("DataTree".to_string()),
                        Type::Named("CBORError".to_string()),
                    ));
                }
                return Some(result_ty(Type::Named("DataTree".to_string()), Type::String));
            }
            ("core.encoding.xml", "parse_bytes") => {
                if !(1..=2).contains(&args.len()) {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.expect_core_arg(name, 0, &Type::List(Box::new(u8_ty())), arg);
                }
                if let Some(arg) = args.get_mut(1) {
                    self.expect_core_arg(name, 1, &Type::Named("XMLParseOptions".to_string()), arg);
                }
                return Some(result_ty(
                    Type::Named("DataTree".to_string()),
                    Type::Named("XMLError".to_string()),
                ));
            }
            ("core.encoding.xml", "to_bytes") => {
                if !(1..=2).contains(&args.len()) {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.expect_core_arg(name, 0, &Type::Named("DataTree".to_string()), arg);
                }
                if let Some(arg) = args.get_mut(1) {
                    self.expect_core_arg(
                        name,
                        1,
                        &Type::Named("XMLRenderOptions".to_string()),
                        arg,
                    );
                }
                return Some(result_ty(
                    Type::List(Box::new(u8_ty())),
                    Type::Named("XMLError".to_string()),
                ));
            }
            ("core.encoding.xml", "decode") if !type_args.is_empty() => {
                if !(1..=2).contains(&args.len()) {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.expect_core_arg(name, 0, &Type::String, arg);
                }
                if let Some(arg) = args.get_mut(1) {
                    self.expect_core_arg(name, 1, &Type::Named("XMLParseOptions".to_string()), arg);
                }
                let Some(t) = exactly_one_type_arg(self, name, type_args, span) else {
                    return None;
                };
                self.check_decodable(&t, span);
                return Some(result_ty(t, decode_error_ty()));
            }
            ("core.encoding.xml", "decode_bytes") if !type_args.is_empty() => {
                if !(1..=2).contains(&args.len()) {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.expect_core_arg(name, 0, &Type::List(Box::new(u8_ty())), arg);
                }
                if let Some(arg) = args.get_mut(1) {
                    self.expect_core_arg(name, 1, &Type::Named("XMLParseOptions".to_string()), arg);
                }
                let Some(t) = exactly_one_type_arg(self, name, type_args, span) else {
                    return None;
                };
                self.check_decodable(&t, span);
                return Some(result_ty(t, decode_error_ty()));
            }
            ("core.encoding.xml", "expanded_name") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.expect_core_arg(name, 0, &Type::Named("DataTree".to_string()), arg);
                }
                return Some(result_ty(
                    Type::Tuple(vec![
                        ("raw".to_string(), Box::new(Type::String)),
                        (
                            "prefix".to_string(),
                            Box::new(Type::Option(Box::new(Type::String))),
                        ),
                        ("local".to_string(), Box::new(Type::String)),
                        (
                            "namespace_uri".to_string(),
                            Box::new(Type::Option(Box::new(Type::String))),
                        ),
                    ]),
                    Type::Named("XMLError".to_string()),
                ));
            }
            ("core.encoding.xml", "root") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.expect_core_arg(name, 0, &Type::Named("DataTree".to_string()), arg);
                }
                return Some(result_ty(
                    Type::Named("DataTree".to_string()),
                    Type::Named("XMLError".to_string()),
                ));
            }
            ("core.encoding.xml", "attribute") => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.expect_core_arg(name, 0, &Type::Named("DataTree".to_string()), arg);
                }
                if let Some(arg) = args.get_mut(1) {
                    self.expect_core_arg(name, 1, &Type::String, arg);
                }
                return Some(result_ty(
                    Type::Option(Box::new(Type::String)),
                    Type::Named("XMLError".to_string()),
                ));
            }
            ("core.encoding.xml", "content") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.expect_core_arg(name, 0, &Type::Named("DataTree".to_string()), arg);
                }
                return Some(result_ty(
                    Type::List(Box::new(Type::Named("DataTree".to_string()))),
                    Type::Named("XMLError".to_string()),
                ));
            }
            ("core.encoding.cbor", "to_bytes" | "to_bytes_canonical") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                for arg in args.iter_mut() {
                    self.borrow_ctx = true;
                    if let Some(t) = self.infer(&mut arg.expr) {
                        self.check_encodable(&t, arg.expr.span());
                    }
                }
                return Some(result_ty(
                    Type::List(Box::new(u8_ty())),
                    Type::Named("CBORError".to_string()),
                ));
            }
            ("core.encoding.cbor", "decode") if !type_args.is_empty() => {
                if !(1..=2).contains(&args.len()) {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.expect_core_arg(name, 0, &Type::List(Box::new(u8_ty())), arg);
                }
                if let Some(arg) = args.get_mut(1) {
                    self.expect_core_arg(name, 1, &Type::Named("CBOROptions".to_string()), arg);
                }
                let Some(t) = exactly_one_type_arg(self, name, type_args, span) else {
                    return None;
                };
                self.check_decodable(&t, span);
                return Some(result_ty(t, decode_error_ty()));
            }
            (
                "core.encoding.json"
                | "core.encoding.jsonl"
                | "core.encoding.csv"
                | "core.encoding.cbor",
                "reader" | "writer",
            )
            | ("core.encoding.xml", "reader" | "writer") => {
                let csv_reader = module == "core.encoding.csv" && name == "reader";
                let max = if csv_reader {
                    5
                } else if (module == "core.encoding.json" && name == "writer")
                    || module == "core.encoding.xml"
                {
                    3
                } else {
                    2
                };
                let (min, max) = (1, max);
                if !(min..=max).contains(&args.len()) {
                    self.diags.push(Diagnostic::error(
                            "E0104",
                            format!("`{}.{}` expects {} to {} arguments, got {}", module_short_name(module), name, min, max, args.len()),
                            "the file handle is required; limits, XML options, and canonical mode use safe defaults when omitted".to_string(),
                            if module == "core.encoding.xml" {
                                format!("write `xml.{name}(^file)`, `xml.{name}(^file, limits)`, or `xml.{name}(^file, limits, options)` with the move marker `^`")
                            } else if name == "reader" { format!("write `{}.reader(^file)` or `{}.reader(^file, limits)` with the move marker `^`", module_short_name(module), module_short_name(module)) } else if module == "core.encoding.json" { "write `json.writer(^file)`, `json.writer(^file, limits)`, or `json.writer(^file, limits, canonical)` with the move marker `^`".to_string() } else { format!("write `{}.writer(^file)` or `{}.writer(^file, limits)` with the move marker `^`", module_short_name(module), module_short_name(module)) },
                            Some(span),
                        ));
                }
                let Some((params, ret)) = &sig else {
                    unreachable!()
                };
                if csv_reader {
                    let Some(contract) = super::core_param_contract(module, name) else {
                        unreachable!()
                    };
                    let bind_params: Vec<crate::Sema::CallBinder::BindParam<'_>> = contract
                        .iter()
                        .enumerate()
                        .map(|(index, param)| crate::Sema::CallBinder::BindParam {
                            label: param.label,
                            name: param.label,
                            zone: param.zone,
                            default: None,
                            convention: params
                                .get(index)
                                .map(|(convention, _)| *convention)
                                .unwrap_or(AccessConvention::Read),
                            ty: params.get(index).map(|(_, ty)| ty),
                            variadic: false,
                            core_default: param.default,
                        })
                        .collect();
                    if crate::Sema::CallBinder::bind_call_args(
                        name,
                        &bind_params,
                        args,
                        span,
                        &mut self.diags,
                    )
                    .is_none()
                    {
                        for arg in args.iter_mut() {
                            self.infer(&mut arg.expr);
                        }
                        return ret.clone();
                    }
                    self.register_binder_refs(args);
                }
                for (i, ((conv, param_ty), arg)) in params.iter().zip(args.iter_mut()).enumerate() {
                    if *conv == AccessConvention::Move {
                        self.expect_core_arg_moving(name, i, param_ty, arg);
                        self.finish_core_call_ownership(name, i, arg, *conv, param_ty);
                    } else {
                        self.expect_core_arg(name, i, param_ty, arg);
                    }
                }
                for arg in args.iter_mut().skip(params.len()) {
                    self.infer(&mut arg.expr);
                }
                return ret.clone();
            }
            ("core.game", "run") => {
                if !(1..=4).contains(&args.len()) {
                    self.diags.push(Diagnostic::error(
                            "E0104",
                            format!("`game.run` expects 1 to 4 arguments, got {}", args.len()),
                            "`game.run` accepts a scene plus optional replay, backend, and frame-count handles"
                                .to_string(),
                            "write `game.run(scene)`, `game.run(scene, replay: replay)`, `game.run(scene, replay: replay, backend: backend)`, or `game.run(scene, frames: 60)`"
                                .to_string(),
                            Some(span),
                        ));
                }
                if let Some(scene) = args.get_mut(0) {
                    self.check_game_run_scene_edit(&scene.expr);
                }
                for (index, ((_, param_ty), arg)) in sig
                    .as_ref()
                    .map(|(params, _)| params.as_slice())
                    .unwrap_or(&[])
                    .iter()
                    .zip(args.iter_mut())
                    .enumerate()
                {
                    let inserted_absent = arg.flags.source_index.is_none()
                        && arg.flags.binder_slot == Some(index)
                        && matches!(arg.expr, Expr::Absent(_));
                    if !inserted_absent {
                        self.expect_core_arg("run", index, param_ty, arg);
                    }
                }
                if let Some(frames) = args.get(3) {
                    if literal_int(&frames.expr)
                        .is_some_and(|value| JetGameFrameBudget::validate(value).is_err())
                    {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            "`game.run` frame budget must be positive".to_string(),
                            "a non-positive frame budget cannot execute a deterministic frame transcript"
                                .to_string(),
                            "pass a positive frame budget, such as `frames: 600`".to_string(),
                            Some(frames.expr.span()),
                        ));
                    }
                }
                return Some(Type::String);
            }
            // D-FOUND-COREAPI1=A: the ignore-file default is one binder-owned
            // absence. Every tier receives the same two-slot checked shape.
            ("core.files", "walk" | "walk_parallel" | "walk_files") => {
                if !(1..=2).contains(&args.len()) {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let Some((params, ret)) = sig.as_ref() else {
                    return None;
                };
                for (index, ((_, param_ty), arg)) in params
                    .iter()
                    .zip(args.iter_mut())
                    .enumerate()
                {
                    let optional_slot = matches!(param_ty, Type::Option(_));
                    if !optional_slot || !matches!(arg.expr, Expr::Absent(_)) {
                        // Optional ABI slots are represented as `Option<T>` in the
                        // checked signature, while source callers pass `T` or
                        // `Absent`. Check present source values against the
                        // payload type, as other optional Core slots do.
                        let source_ty = param_ty.unwrap_option().unwrap_or(param_ty);
                        self.expect_core_arg(name, index, source_ty, arg);
                    }
                }
                for arg in args.iter_mut().skip(params.len()) {
                    self.infer(&mut arg.expr);
                }
                return ret.clone();
            }
            // D-ENC1 / D-GENERIC-CALL1 / D-SERDE6: typed encode/decode over
            // the Encode/Decode model.
            // `to_string`/`to_string_pretty` accept any encodable value (the dynamic
            // `JSON` / `[[String]]` / `Map` forms AND a `#[Codable]` value); the
            // codegen routes by the lowered arg type. `decode<T>` is the typed decode
            // (→ `T`, or `[T]` for CSV) keyed by the call-site type argument.
            ("core.encoding.csv", "to_string") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                for a in args.iter_mut() {
                    self.borrow_ctx = true;
                    let Some(t) = self.infer(&mut a.expr) else {
                        continue;
                    };
                    let valid = match &t {
                        Type::List(elem) if matches!(elem.as_ref(), Type::List(cell) if matches!(cell.as_ref(), Type::String)) => {
                            true
                        }
                        Type::List(elem) => {
                            let type_name = match elem.as_ref() {
                                Type::Named(name) => Some(name.as_str()),
                                Type::Apply { name, .. } => Some(name.as_str()),
                                _ => None,
                            };
                            type_name.is_some_and(|name| {
                                let (namespace, leaf) = name
                                    .rsplit_once('.')
                                    .map_or((None, name), |(namespace, leaf)| {
                                        (Some(namespace), leaf)
                                    });
                                self.struct_owner_module(leaf, namespace)
                                    .and_then(|owner| self.struct_fields_of(owner, leaf))
                                    .is_some()
                            })
                        }
                        _ => false,
                    };
                    if valid {
                        self.check_encodable(&t, a.expr.span());
                    } else {
                        self.diags.push(Diagnostic::error(
                                "E0112",
                                format!(
                                    "`{}` wants [[String]] rows or a list of #Codable records for argument 1, but this is {}",
                                    name,
                                    t.show()
                                ),
                                "CSV output accepts string rows or typed records".to_string(),
                                "use [[String]] rows or a list of #Codable records here".to_string(),
                                Some(a.expr.span()),
                            ));
                    }
                }
                return Some(Type::String);
            }
            ("core.text.fmt", "pretty") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.first_mut() {
                    self.borrow_ctx = true;
                    if let Some(t) = self.infer(&mut arg.expr) {
                        if !is_debuggable(&t, self.registry, self.trait_reg) {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("{} can't use `core.text.fmt.pretty` yet", t.show()),
                                "pretty formatting needs a debuggable value".to_string(),
                                "implement `Debug` or pass a debuggable value".to_string(),
                                Some(arg.expr.span()),
                            ));
                        }
                    }
                }
                for arg in args.iter_mut().skip(1) {
                    self.infer(&mut arg.expr);
                }
                return Some(Type::String);
            }
            // D-FMT-INTERP3=B: decimal accepts both machine Float and exact
            // Int. The return stays String; the carrier-specific formatter is
            // selected by each engine only after this sema fact is resolved.
            ("core.text.fmt", method @ ("decimal" | "grouped")) => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.borrow_ctx = true;
                    let got = self.infer(&mut arg.expr);
                    if !matches!(got.as_ref(), Some(Type::Float | Type::Int)) {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!(
                                "{} can't use `core.text.fmt.{method}`",
                                got.map_or_else(|| "this value".to_string(), |ty| ty.show())
                            ),
                            format!("{method} formatting accepts a Float or exact Int value"),
                            format!("pass a Float or Int as the value"),
                            Some(arg.expr.span()),
                        ));
                    }
                }
                if let Some(arg) = args.get_mut(1) {
                    self.expect_core_arg(name, 1, &Type::Int, arg);
                }
                for arg in args.iter_mut().skip(2) {
                    self.infer(&mut arg.expr);
                }
                return Some(Type::String);
            }
            (
                "core.encoding.json" | "core.encoding.toml" | "core.encoding.yaml",
                "to_string" | "to_string_pretty",
            ) => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                for a in args.iter_mut() {
                    self.borrow_ctx = true;
                    if let Some(t) = self.infer(&mut a.expr) {
                        self.check_encodable(&t, a.expr.span());
                    }
                }
                return Some(Type::String);
            }
            // D-SHAPE-PROJECT1=A: `args.decode<T>()` decodes a `#CLI` struct from
            // the process arguments anywhere, through the same builder rows the
            // entry `fn run(args: T)` derives from T.
            ("core.args", "decode") => {
                if !args.is_empty() {
                    self.diags.push(wrong_core_arity(name, 0, args.len(), span));
                }
                for arg in args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                let Some(t) = exactly_one_type_arg(self, name, type_args, span) else {
                    return None;
                };
                self.check_cli_shape(&t, span);
                return Some(result_ty(t, decode_error_ty()));
            }
            // D-JSON3: the UNTYPED `json.decode(text)` form is the lenient
            // dynamic decode — same `DataTree !EncodingError` shape as `parse`, with
            // string→number/bool coercions surfaced as log lines
            // (docs/spec/reference/core-library.md, `jet_std_json_decode_lenient`
            // in the Prelude, `enc_ok_is_json` in emit). `decode` is registered
            // in `is_polymorphic_core_special`, so its `core_fixed_sig` row is
            // never consulted; without this arm the call fell through to
            // `unknown_core_item` and E1004 claimed `core.encoding.json` has no
            // item `decode` while suggesting `decode`.
            ("core.encoding.json", "decode") if type_args.is_empty() => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.expect_core_arg(name, 0, &Type::String, arg);
                }
                for arg in args.iter_mut().skip(1) {
                    self.infer(&mut arg.expr);
                }
                return Some(result_ty(json_ty(), encoding_error_ty()));
            }
            ("core.sys", "decode") if !type_args.is_empty() => {
                if args.len() > 3 {
                    self.diags.push(wrong_core_arity(name, 3, args.len(), span));
                }
                for (index, arg) in args.iter_mut().enumerate() {
                    match index {
                        0 | 1 => self.expect_core_arg(name, index, &Type::String, arg),
                        2 => self.expect_core_arg(
                            name,
                            index,
                            &Type::List(Box::new(Type::String)),
                            arg,
                        ),
                        _ => {
                            self.infer(&mut arg.expr);
                        }
                    }
                }
                let Some(t) = exactly_one_type_arg(self, name, type_args, span) else {
                    return None;
                };
                self.check_decodable(&t, span);
                return Some(result_ty(t, decode_error_ty()));
            }
            // D-SHAPE-ONE1=A: decode one existing DB row through the same
            // typed DataTree decoder used by the wire formats. The explicit
            // row remains the authoritative table/transaction carrier.
            ("core.db", "decode") if !type_args.is_empty() => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.expect_core_arg(name, 0, &db_row_ty(), arg);
                }
                for arg in args.iter_mut().skip(1) {
                    self.infer(&mut arg.expr);
                }
                let Some(t) = exactly_one_type_arg(self, name, type_args, span) else {
                    return None;
                };
                self.check_decodable(&t, span);
                return Some(result_ty(t, decode_error_ty()));
            }
            (
                "core.encoding.json" | "core.encoding.csv" | "core.encoding.toml"
                | "core.encoding.yaml",
                "decode",
            ) if !type_args.is_empty() => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.expect_core_arg(name, 0, &Type::String, arg);
                }
                for arg in args.iter_mut().skip(1) {
                    self.infer(&mut arg.expr);
                }
                let Some(t) = exactly_one_type_arg(self, name, type_args, span) else {
                    return None;
                };
                self.check_decodable(&t, span);
                let inner = if module == "core.encoding.csv" {
                    Type::List(Box::new(t))
                } else {
                    t
                };
                return Some(result_ty(inner, decode_error_ty()));
            }
            // D-COLUMNAR-BOUNDARY1=A: Arrow owners carry their checked row
            // type in the unique `DataArrowBatch<T>` carrier. Import and query
            // consume that owner; the row's registered `borrow_mask` supplies
            // the same Move convention used by every other Core call. This
            // arm only projects the carrier's row type and cannot invent a
            // second ownership policy.
            ("core.data.arrow", "import" | "query") => {
                if !type_args.is_empty() {
                    self.diags.push(Diagnostic::error(
                        "E0119",
                        format!("`{name}` does not take an explicit type argument"),
                        "Arrow operations infer their row type from the `DataArrowBatch<T>` value"
                            .to_string(),
                        "remove the explicit type argument and pass a `DataArrowBatch<T>` value"
                            .to_string(),
                        Some(span),
                    ));
                }
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let row = args.get_mut(0).and_then(|arg| {
                    let actual = self.infer(&mut arg.expr)?;
                    let convention = core_call_argument_convention(module, name, 0);
                    self.finish_core_call_ownership(name, 0, arg, convention, &actual);
                    match actual {
                        Type::Apply { name: carrier, args } if carrier == "DataArrowBatch" && args.len() == 1 => {
                            args.into_iter().next()
                        }
                        other => {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`{name}` expects a `DataArrowBatch<T>` value, got {}", other.show()),
                                "Arrow operations preserve the row type carried by the checked owner"
                                    .to_string(),
                                "pass a `DataArrowBatch<T>` value as the argument".to_string(),
                                Some(span),
                            ));
                            None
                        }
                    }
                });
                for arg in args.iter_mut().skip(1) {
                    self.infer(&mut arg.expr);
                }
                let Some(row) = row else {
                    return None;
                };
                if name == "import" {
                    return Some(data_carrier_type("DataArrowBatch", row));
                }
                return Some(result_ty(
                    data_carrier_type("Query", row),
                    Type::Named("DataError".to_string()),
                ));
            }
            // D-DX-LOADERS1=A: loader lifecycle operations infer their row
            // type from the concrete `DataLoader<T>` argument. The child
            // operations are not a second generic-call family: only the
            // top-level data constructors carry explicit `<T>` syntax.
            (
                "core.data.loader",
                "authority" | "bind" | "bind_text" | "cancel" | "invalidate"
                    | "needs_refresh" | "offline" | "ready" | "source_identity"
                    | "status" | "stream",
            ) => {
                if !type_args.is_empty() {
                    self.diags.push(Diagnostic::error(
                        "E0119",
                        format!("`{name}` does not take an explicit type argument"),
                        "the loader operation reads its row type from the `DataLoader<T>` value"
                            .to_string(),
                        "remove the `<T>` and pass a concrete `DataLoader<T>` handle".to_string(),
                        Some(span),
                    ));
                }
                let expected = match name {
                    "bind" | "bind_text" | "offline" | "invalidate" => 2,
                    _ => 1,
                };
                if args.len() != expected {
                    self.diags.push(wrong_core_arity(name, expected, args.len(), span));
                }
                let mutating = matches!(
                    name,
                    "bind" | "bind_text" | "cancel" | "invalidate" | "offline" | "stream"
                );
                let loader_ty = args.get_mut(0).and_then(|arg| {
                    if mutating && arg.convention != AccessConvention::Write {
                        self.diags.push(Diagnostic::error(
                            "E0202",
                            format!("argument 1 to `{name}` requires the write-access marker `&`"),
                            "this data-loader operation changes the loader state in place"
                                .to_string(),
                            format!(
                                "write the write-access marker `&`: `{}loader`",
                                Syntax::SIGIL_WRITE
                            ),
                            Some(arg.span),
                        ));
                    }
                    self.infer(&mut arg.expr)
                });
                let _row = match loader_ty {
                    Some(Type::Apply { name, args }) if name == "DataLoader" && args.len() == 1 => {
                        args.into_iter().next()
                    }
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!(
                                "`{name}` expects a `DataLoader<T>` handle, got {}",
                                other.show()
                            ),
                            "loader lifecycle operations preserve the row type carried by the loader"
                                .to_string(),
                            "pass a `DataLoader<T>` value as the first argument".to_string(),
                            Some(span),
                        ));
                        None
                    }
                    None => None,
                };
                for (index, arg) in args.iter_mut().enumerate().skip(1) {
                    let expected_ty = match (name, index) {
                        ("bind", 1) => Some(Type::List(Box::new(u8_ty()))),
                        ("bind_text", 1) => Some(Type::String),
                        ("offline", 1) => Some(Type::Bool),
                        ("invalidate", 1) => {
                            Some(Type::Named("DataInvalidationCause".to_string()))
                        }
                        _ => None,
                    };
                    if let Some(expected_ty) = expected_ty {
                        self.expect_core_arg(name, index, &expected_ty, arg);
                    } else {
                        self.infer(&mut arg.expr);
                    }
                }
                let result = match name {
                    "bind" | "bind_text" => {
                        result_ty(unit_ty(), Type::Named("DataError".to_string()))
                    }
                    "cancel" | "offline" | "invalidate" => unit_ty(),
                    "needs_refresh" | "ready" => Type::Bool,
                    "status" => Type::Named("DataLoaderStatus".to_string()),
                    "source_identity" => Type::Named("DataSourceIdentity".to_string()),
                    "authority" => Type::Named("DataAuthority".to_string()),
                    "stream" => result_ty(
                        Type::Named("DataStream".to_string()),
                        Type::Named("DataError".to_string()),
                    ),
                    _ => unreachable!(),
                };
                return Some(result);
            }
            // D-DX-LOADERS1=A: declarations preserve the row type while
            // snapshot decoding remains an explicit fallible operation.
            ("core.data", "load" | "load_default" | "file" | "file_member" | "url" | "database" | "value" | "snapshot")
                if !type_args.is_empty() =>
            {
                let Some(t) = exactly_one_type_arg(self, name, type_args, span) else {
                    return None;
                };
                let expected = match name {
                    "load" => 2..=2,
                    "load_default" => 1..=1,
                    "file" => 3..=3,
                    "file_member" => 4..=4,
                    "url" => 4..=4,
                    "database" => 4..=4,
                    "value" => 2..=2,
                    "snapshot" => 1..=1,
                    _ => unreachable!(),
                };
                if !expected.contains(&args.len()) {
                    self.diags.push(wrong_core_arity(name, *expected.end(), args.len(), span));
                }
                for (index, arg) in args.iter_mut().enumerate() {
                    let ty = match name {
                        "load" | "load_default" => {
                            if index == 0 { Some(Type::String) } else { Some(Type::Named("DataLimits".to_string())) }
                        }
                        "file" => Some(if index < 2 { Type::String } else { Type::Named("DataLimits".to_string()) }),
                        "file_member" => Some(if index < 3 { Type::String } else { Type::Named("DataLimits".to_string()) }),
                        "url" => Some(if index < 3 { Type::String } else { Type::Named("DataLimits".to_string()) }),
                        "database" => Some(match index {
                            0 => Type::String,
                            1 => Type::List(Box::new(Type::String)),
                            2 => Type::String,
                            _ => Type::Named("DataLimits".to_string()),
                        }),
                        "value" => Some(t.clone()),
                        "snapshot" => Some(Type::Apply { name: "DataLoader".to_string(), args: vec![t.clone()] }),
                        _ => None,
                    };
                    if let Some(ty) = ty {
                        self.expect_core_arg(name, index, &ty, arg);
                    } else {
                        self.infer(&mut arg.expr);
                    }
                }
                if name == "value" {
                    self.check_encodable(&t, span);
                }
                if name == "snapshot" {
                    self.check_decodable(&t, span);
                    self.check_encodable(&t, span);
                    return Some(result_ty(
                        Type::Apply { name: "DataSnapshot".to_string(), args: vec![t] },
                        Type::Named("DataError".to_string()),
                    ));
                }
                return Some(result_ty(
                    Type::Apply { name: "DataLoader".to_string(), args: vec![t] },
                    Type::Named("DataError".to_string()),
                ));
            }
            // D-DX-PLOT1=A: plotting consumes an ordinary typed list. The
            // compiler derives schema facts from the row type.
            ("core.data", "plot") | ("core.data.plot", "plot") if !type_args.is_empty() => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let Some(row) = exactly_one_type_arg(self, name, type_args, span) else {
                    return None;
                };
                if let Some(arg) = args.get_mut(0) {
                    self.expect_core_arg(name, 0, &Type::List(Box::new(row.clone())), arg);
                }
                self.check_encodable(&row, span);
                return Some(result_ty(
                    Type::Apply { name: "JetDataPlot".to_string(), args: vec![row] },
                    Type::Named("DataError".to_string()),
                ));
            }
            ("core.data", "inspect" | "inspect_json" | "text" | "svg" | "show" | "render")
            | ("core.data.plot", "inspect" | "inspect_json" | "text" | "svg" | "show" | "render")
                if !type_args.is_empty() =>
            {
                let Some(row) = exactly_one_type_arg(self, name, type_args, span) else {
                    return None;
                };
                let expected = if name == "render" { 2 } else { 1 };
                if args.len() != expected {
                    self.diags.push(wrong_core_arity(name, expected, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.expect_core_arg(
                        name,
                        0,
                        &Type::Apply { name: "JetDataPlot".to_string(), args: vec![row] },
                        arg,
                    );
                }
                if name == "render" {
                    if let Some(arg) = args.get_mut(1) {
                        self.expect_core_arg(name, 1, &Type::Named("JetDataPlotBackend".to_string()), arg);
                    }
                }
                let ok = match name {
                    "inspect" => Type::Named("JetDataPlotInspection".to_string()),
                    "inspect_json" | "text" | "svg" => Type::String,
                    _ => Type::Named("JetDataPlotRender".to_string()),
                };
                return Some(result_ty(ok, Type::Named("DataError".to_string())));
            }
            // D-DATA-SURFACE1=A: the beginner facade reuses typed CSV/JSON decoding,
            // then keeps table/stat selectors as ordinary typed Jet lambdas.
            ("core.data", "csv" | "json") if !type_args.is_empty() => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                for a in args.iter_mut() {
                    self.expect_core_arg(name, 0, &Type::String, a);
                }
                let Some(t) = exactly_one_type_arg(self, name, type_args, span) else {
                    return None;
                };
                self.check_decodable(&t, span);
                return Some(result_ty(Type::List(Box::new(t)), decode_error_ty()));
            }
            ("core.data", "csv_reader" | "json_reader") if !type_args.is_empty() => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.expect_core_arg(name, 0, &Type::Named("FileReader".to_string()), arg);
                }
                if let Some(arg) = args.get_mut(1) {
                    self.expect_core_arg(name, 1, &Type::Named("DataLimits".to_string()), arg);
                }
                let Some(t) = exactly_one_type_arg(self, name, type_args, span) else {
                    return None;
                };
                self.check_decodable(&t, span);
                return Some(result_ty(
                    Type::Apply {
                        name: "DataStream".to_string(),
                        args: vec![t],
                    },
                    Type::Named("DataError".to_string()),
                ));
            }
            ("core.data", "count") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else {
                    return Some(Type::Int);
                };
                let ty = self.infer(&mut arg.expr)?;
                if !matches!(ty, Type::List(_)) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("`data.count` needs a typed row list, not {}", ty.show()),
                        "core.data counts ordinary list elements".to_string(),
                        "pass a `[T]` value".to_string(),
                        Some(arg.expr.span()),
                    ));
                }
                return Some(Type::Int);
            }
            ("core.data", "schema") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else {
                    return Some(Type::List(Box::new(Type::Named("DataColumn".to_string()))));
                };
                let ty = self.infer(&mut arg.expr)?;
                if !matches!(ty, Type::List(_)) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("`data.schema` needs a typed row list, not {}", ty.show()),
                        "core.data schema reads column names and types from ordinary rows"
                            .to_string(),
                        "pass a `[T]` value".to_string(),
                        Some(arg.expr.span()),
                    ));
                }
                return Some(Type::List(Box::new(Type::Named("DataColumn".to_string()))));
            }
            ("core.data", "inner_join" | "left_join") => {
                if args.len() != 4 {
                    self.diags.push(wrong_core_arity(name, 4, args.len(), span));
                }
                let left_ty = args.get_mut(0).and_then(|a| self.infer(&mut a.expr));
                let right_ty = args.get_mut(1).and_then(|a| self.infer(&mut a.expr));
                let left_row = match left_ty {
                    Some(Type::List(inner)) => *inner,
                    Some(other) => {
                        if let Some(arg) = args.get(0) {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!(
                                    "`data.{}` needs a typed left list, not {}",
                                    name,
                                    other.show()
                                ),
                                "core.data joins rows from typed lists".to_string(),
                                "pass `[LeftRow]` and `[RightRow]` values".to_string(),
                                Some(arg.expr.span()),
                            ));
                        }
                        Type::Int
                    }
                    None => Type::Int,
                };
                let right_row = match right_ty {
                    Some(Type::List(inner)) => *inner,
                    Some(other) => {
                        if let Some(arg) = args.get(1) {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!(
                                    "`data.{}` needs a typed right list, not {}",
                                    name,
                                    other.show()
                                ),
                                "core.data joins rows from typed lists".to_string(),
                                "pass `[LeftRow]` and `[RightRow]` values".to_string(),
                                Some(arg.expr.span()),
                            ));
                        }
                        Type::Int
                    }
                    None => Type::Int,
                };
                if let Some(left_key) = args.get_mut(2) {
                    let key_fn = Type::Fn {
                        params: vec![left_row.clone()],
                        ret: Some(Box::new(Type::String)),
                        effect_bound: None,
                        return_view_provenance: None,
                        param_contract: None,
                        call_metadata: None,
                    };
                    self.expect_core_arg(name, 2, &key_fn, left_key);
                }
                if let Some(right_key) = args.get_mut(3) {
                    let key_fn = Type::Fn {
                        params: vec![right_row.clone()],
                        ret: Some(Box::new(Type::String)),
                        effect_bound: None,
                        return_view_provenance: None,
                        param_contract: None,
                        call_metadata: None,
                    };
                    self.expect_core_arg(name, 3, &key_fn, right_key);
                }
                let joined_right = if name == "left_join" {
                    Type::Option(Box::new(right_row))
                } else {
                    right_row
                };
                let joined = Type::List(Box::new(Type::Apply {
                    name: "DataJoin".to_string(),
                    args: vec![left_row, joined_right],
                }));
                return Some(result_ty(joined, Type::Named("DataError".to_string())));
            }
            ("core.data", "pivot_sum") => {
                if args.len() != 4 {
                    self.diags.push(wrong_core_arity(name, 4, args.len(), span));
                }
                let Some(rows_arg) = args.get_mut(0) else {
                    return Some(Type::List(Box::new(Type::Named("DataPivotCell".to_string()))));
                };
                let rows_ty = self.infer(&mut rows_arg.expr);
                let row_ty = match rows_ty {
                    Some(Type::List(inner)) => *inner,
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`data.{}` needs a typed list, not {}", name, other.show()),
                            "core.data pivots rows from ordinary typed lists".to_string(),
                            "pass a `[Row]` value, such as `data.csv<Row>(text)?`".to_string(),
                            Some(rows_arg.expr.span()),
                        ));
                        Type::Int
                    }
                    None => Type::Int,
                };
                for idx in [1usize, 2usize] {
                    if let Some(arg) = args.get_mut(idx) {
                        let key_fn = Type::Fn {
                            params: vec![row_ty.clone()],
                            ret: Some(Box::new(Type::String)),
                            effect_bound: None,
                            return_view_provenance: None,
                            param_contract: None,
                            call_metadata: None,
                        };
                        self.expect_core_arg(name, idx, &key_fn, arg);
                    }
                }
                if let Some(value_arg) = args.get_mut(3) {
                    let value_fn = Type::Fn {
                        params: vec![row_ty],
                        ret: Some(Box::new(Type::Float)),
                        effect_bound: None,
                        return_view_provenance: None,
                        param_contract: None,
                        call_metadata: None,
                    };
                    self.expect_core_arg(name, 3, &value_fn, value_arg);
                }
                let cell = Type::Named("DataPivotCell".to_string());
                let cells = Type::List(Box::new(cell));
                return Some(result_ty(cells, Type::Named("DataError".to_string())));
            }

            ("core.mem", "volatile_read") => {
                if Syntax::core_mem_requires_audit(Syntax::MEM_VOLATILE_READ) && !self.in_unsafe {
                    self.diags.push(e3101(Syntax::MEM_VOLATILE_READ, span));
                }
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                    return None;
                }
                let arg = args.get_mut(0)?;
                let t = self.infer(&mut arg.expr)?;
                return match ptr_elem(&t) {
                    Some(elem) => Some(elem),
                    None => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!(
                                "`{}` needs a `Ptr<T>`, not {}",
                                Syntax::MEM_VOLATILE_READ,
                                t.show()
                            ),
                            "a volatile read reads through a typed pointer".to_string(),
                            "build a pointer first with `mem.Ptr<T>.from_addr(addr)`".to_string(),
                            Some(arg.expr.span()),
                        ));
                        None
                    }
                };
            }
            ("core.mem", "volatile_write") => {
                if Syntax::core_mem_requires_audit(Syntax::MEM_VOLATILE_WRITE) && !self.in_unsafe {
                    self.diags.push(e3101(Syntax::MEM_VOLATILE_WRITE, span));
                }
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                    return None;
                }
                let ptr_arg = args.get_mut(0)?;
                let ptr_ty = self.infer(&mut ptr_arg.expr)?;
                let Some(elem) = ptr_elem(&ptr_ty) else {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "`{}` needs a `Ptr<T>`, not {}",
                            Syntax::MEM_VOLATILE_WRITE,
                            ptr_ty.show()
                        ),
                        "a volatile write writes through a typed pointer".to_string(),
                        "build a pointer first with `mem.Ptr<T>.from_addr(addr)`".to_string(),
                        Some(ptr_arg.expr.span()),
                    ));
                    return None;
                };
                let value_arg = args.get_mut(1)?;
                if let Some(value_ty) = self.infer(&mut value_arg.expr) {
                    let value_ty = self.widen_numeric_argument(
                        &mut value_arg.expr,
                        value_ty,
                        &elem,
                        AccessConvention::Read,
                    );
                    self.check_type_assignable(&elem, &value_ty, value_arg.expr.span());
                }
                return Some(unit_ty());
            }
            // D-PIN1=A: `mem.pin(&place) -> Pin<T>`. Pinning is inert on its
            // own (S58's rule for `address_of`): it starts a tracked write
            // window, and the window itself is what safe code relies on, so
            // no `#Unsafe` is needed here. The window is recorded by
            // `view_call_sources`; this arm only types the call.
            ("core.mem", "pin") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                    return None;
                }
                let arg = args.get_mut(0)?;
                let write_place =
                    matches!(&arg.expr, Expr::Place(_, crate::AST::PlaceAccess::Write, _))
                        || arg.convention == AccessConvention::Write;
                // A pin borrows the place; it never duplicates it. Without
                // this the auto-copy pass (D-CAP2) would wrap a field or
                // index place in `copy`, and the pin would then promise
                // address stability for a temporary instead of the owner.
                self.borrow_ctx = true;
                let t = self.infer(&mut arg.expr)?;
                if !write_place {
                    self.diags.push(Diagnostic::error(
                            "E0218",
                            format!(
                                "`{}` needs a write window made with the write-access marker `&` into the place being pinned",
                                Syntax::MEM_PIN
                            ),
                            "a pin promises one storage location will not move, so it has to name that location with the write-access marker `&` instead of a copied value"
                                .to_string(),
                            format!(
                                "write `mem.{}({}place)` with the write-access marker `&`",
                                Syntax::MEM_PIN,
                                Syntax::SIGIL_WRITE
                            ),
                            Some(arg.expr.span()),
                        ));
                    return None;
                }
                let _ = alias_span;
                // Pinning is idempotent: a place reached through a live pin
                // is already address-stable, so `Pin<Pin<T>>` never exists
                // (I8 — one mechanism, one spelling for one promise).
                if matches!(&t, Type::Apply { name, args }
                        if name == Syntax::TYPE_PIN && args.len() == 1)
                {
                    return Some(t);
                }
                return Some(Type::Apply {
                    name: Syntax::TYPE_PIN.to_string(),
                    args: vec![t],
                });
            }
            ("core.mem", "address_of") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                    return None;
                }
                // Taking an address is inert (S58): legal outside `#Unsafe`.
                let arg = args.get_mut(0)?;
                self.infer(&mut arg.expr);
                let _ = alias_span;
                return Some(Type::Int);
            }
            ("core.term", "print") => {
                // D-PRELUDEX1=A: qualified twin of ambient `print` for `#NoPrelude` files.
                // D-VERDICT-1321-1: variadic — each argument prints on its own line.
                if args.is_empty() {
                    self.diags.push(Diagnostic::error(
                        "E0103",
                        format!("`{name}` needs at least one thing to print"),
                        "printing nothing isn't meaningful".to_string(),
                        "e.g. io.print(\"hello\")".to_string(),
                        Some(span),
                    ));
                }
                for arg in args.iter_mut() {
                    self.borrow_ctx = true;
                    if let Some(ty) = self.infer(&mut arg.expr) {
                        if !is_printable(&ty, self.registry, self.trait_reg)
                            && !self.is_unit_type(&ty)
                        {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("{} can't be printed yet", ty.show()),
                                "`io.print` prints the same values as ambient `print`".to_string(),
                                "print one of its fields, or make it a printable type".to_string(),
                                Some(arg.expr.span()),
                            ));
                        }
                    }
                }
                return None;
            }
            ("core.term", "progress") => {
                if args.is_empty() || args.len() > 3 {
                    self.diags.push(Diagnostic::error(
                            "E0112",
                            "`io.progress` needs one to three arguments".to_string(),
                            "the first argument is a String message or a List/Iter source; the optional arguments set the description and format".to_string(),
                            "write `io.progress(items, description, format)` for an iterable".to_string(),
                            Some(span),
                        ));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                let source = self.infer(&mut args[0].expr)?;
                if matches!(source, Type::String) {
                    if args.len() != 1 {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            "`io.progress` takes only one argument for a text update".to_string(),
                            "the one-string form writes one progress message".to_string(),
                            "use `io.progress(items, description, format)` for an iterable"
                                .to_string(),
                            Some(args[1].expr.span()),
                        ));
                    }
                    return Some(result_ty(unit_ty(), io_error_ty()));
                }
                let elem = match source {
                    Type::List(inner) => *inner,
                    Type::FixedList { elem, .. } => *elem,
                    Type::Apply { name, mut args }
                        if name == Syntax::TYPE_ITER && args.len() == 1 =>
                    {
                        args.pop().expect("Iter has one element type")
                    }
                    _ => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`io.progress` cannot wrap {}", source.show()),
                            "progress adapters wrap List<T> and Iter<T> values".to_string(),
                            "pass a list or lazy iterator".to_string(),
                            Some(args[0].expr.span()),
                        ));
                        return None;
                    }
                };
                for arg in args.iter_mut().skip(1) {
                    self.expect_core_arg("progress", 1, &Type::String, arg);
                }
                return Some(crate::Collections::iter_ty(elem));
            }
            ("core.term", "eprint") => {
                // D-VERDICT-1321-1: variadic — each argument prints on its own line.
                if args.is_empty() {
                    self.diags.push(Diagnostic::error(
                        "E0103",
                        format!("`{name}` needs at least one thing to print"),
                        "printing nothing isn't meaningful".to_string(),
                        "e.g. io.eprint(\"warning\")".to_string(),
                        Some(span),
                    ));
                }
                for arg in args.iter_mut() {
                    self.borrow_ctx = true;
                    if let Some(ty) = self.infer(&mut arg.expr) {
                        if !is_printable(&ty, self.registry, self.trait_reg)
                            && !self.is_unit_type(&ty)
                        {
                            self.diags.push(Diagnostic::error(
                                    "E0112",
                                    format!("{} can't be printed yet", ty.show()),
                                    "`io.eprint` prints the same values as `print`, but writes to stderr"
                                        .to_string(),
                                    "print one of its fields, or make it a printable type".to_string(),
                                    Some(arg.expr.span()),
                                ));
                        }
                    }
                }
                return Some(Type::Named("Unit".to_string()));
            }
            // D-ANY-JAI1 (c7jaiany §6): `reflect.of(x)` — the runtime reflection
            // floor. Legal wherever `x` is interpolatable (`"{x}"`) — the SAME
            // gate a trait-bounded variadic's `...Renderable` bound uses
            // (`is_displayable`, reuse, I8), not the looser `is_printable`
            // `print`/`io.eprint` accept: `Value.display()` is backed by
            // `jet_display()` (JetDisplay), not `jet_show()`/`{:?}`, so it
            // shows exactly what `"{x}"` would — never codegen's mangled Rust
            // field names, which `is_printable` would let through for a
            // struct with no auto/explicit `Display`.
            ("core.reflect", "of") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return Some(Type::Named("Value".to_string()));
                }
                let arg = &mut args[0];
                if let Some(ty) = self.infer(&mut arg.expr) {
                    if !is_displayable(&ty, self.registry, self.trait_reg)
                        && !self.is_unit_type(&ty)
                    {
                        if crate::Sema::Diagnostics::is_secret_bearing_crypto_type(&ty) {
                            self.diags.push(Diagnostic::error(
                                    "E0112",
                                    format!("secret-bearing `{}` cannot be reflected", ty.name()),
                                    "reflection would expose a cryptographic secret through generic inspection or display".to_string(),
                                    "keep the value opaque; inspect only public keys, signatures, digests, or envelope metadata".to_string(),
                                    Some(arg.expr.span()),
                                ));
                        } else {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("{} can't be reflected yet", ty.show()),
                                "`reflect.of` inspects the same values `\"{x}\"` interpolation can show"
                                    .to_string(),
                                "implement `Display` for its type, or pass one of its fields instead"
                                    .to_string(),
                                Some(arg.expr.span()),
                            ));
                        }
                    }
                }
                return Some(Type::Named("Value".to_string()));
            }
            ("core.term", "input") => {
                if args.len() > 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.expect_core_arg(name, 0, &Type::String, arg);
                }
                return Some(result_ty(Type::String, io_error_ty()));
            }
            // Parity of a whole number.
            ("core.math", "is_even" | "is_odd") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    let ty = self.infer(&mut arg.expr)?;
                    if !matches!(ty, Type::Int | Type::IntN { .. } | Type::InlineRange { .. }) {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`{}` needs a whole number, not {}", name, ty.show()),
                            "parity is a property of whole numbers".to_string(),
                            "pass an Int or a sized integer".to_string(),
                            Some(arg.expr.span()),
                        ));
                        return None;
                    }
                }
                return Some(Type::Bool);
            }
            // D-FLOATW1 (ratified 2026-06-22): sqrt/floor/ceil/pow are width-generic —
            // they return the same float width they receive (Float→Float, F32→F32).
            // Mixing widths is a compile error; destination-owned conversion is explicit.
            (
                "core.math",
                "sqrt" | "floor" | "ceil" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan"
                | "sinh" | "cosh" | "tanh" | "exp" | "ln" | "log2" | "log10" | "acosh" | "asinh"
                | "atanh" | "cbrt" | "exp2" | "exp_m1" | "ln_1p" | "signum" | "trunc" | "fract"
                | "degrees" | "radians",
            ) => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else {
                    return Some(Type::Float);
                };
                let ty = self.infer(&mut arg.expr)?;
                if exact_rational_math_approx(name)
                    && matches!(&ty, Type::Named(type_name)
                            if type_name == Syntax::TYPE_FRACTION
                                || type_name == Syntax::TYPE_DECIMAL)
                {
                    return Some(Type::Float);
                }
                if name == "sqrt"
                    && matches!(
                        &ty,
                        Type::Apply { name, args }
                            if name == Syntax::TYPE_MEASUREMENT
                                && args == &[Type::Float]
                    )
                {
                    return Some(ty);
                }
                if !matches!(ty, Type::Float | Type::Float32) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "`{}` needs Float, F32, or Measurement<Float>, not {}",
                            name,
                            ty.show()
                        ),
                        "math functions preserve their numeric knowledge grade".to_string(),
                        "pass a Float or F32 value, or use `sqrt` with a measurement".to_string(),
                        Some(arg.expr.span()),
                    ));
                    return None;
                }
                return Some(ty);
            }
            ("core.math", "atan2" | "hypot" | "lerp" | "copysign" | "log" | "fma") => {
                let wanted = if name == "lerp" || name == "fma" {
                    3
                } else {
                    2
                };
                if args.len() != wanted {
                    self.diags
                        .push(wrong_core_arity(name, wanted, args.len(), span));
                }
                // D-TYPE2-DEFAULT1: these leave the exact world too, so an
                // exact carrier crosses here exactly as it does at the
                // one-argument functions above. Admitting `sqrt(0.5)` while
                // rejecting `log(0.5, 2)` would be one boundary with two
                // rules again.
                let exact_crosses = |ty: &Type| {
                    matches!(ty, Type::Named(name)
                            if name == Syntax::TYPE_FRACTION || name == Syntax::TYPE_DECIMAL)
                };
                let Some(first) = args.get_mut(0).and_then(|a| self.infer(&mut a.expr)) else {
                    for a in args.iter_mut().skip(1) {
                        self.infer(&mut a.expr);
                    }
                    return Some(Type::Float);
                };
                let first = if exact_crosses(&first) {
                    Type::Float
                } else {
                    first
                };
                if !matches!(first, Type::Float | Type::Float32) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("`{}` needs Float or F32, not {}", name, first.show()),
                        "this math function operates on floating-point numbers".to_string(),
                        "pass Float or F32 values".to_string(),
                        Some(args[0].expr.span()),
                    ));
                    return None;
                }
                for i in 1..args.len() {
                    if let Some(got) = args.get_mut(i).and_then(|a| self.infer(&mut a.expr)) {
                        let got = if exact_crosses(&got) {
                            Type::Float
                        } else {
                            got
                        };
                        if got != first {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!(
                                    "`{}` needs all arguments to have the same float type",
                                    name
                                ),
                                "D-FLOATW1: mixing float widths is not allowed".to_string(),
                                "convert the arguments to the same float width".to_string(),
                                Some(args[i].expr.span()),
                            ));
                        }
                    }
                }
                return Some(first);
            }
            ("core.math", "is_nan" | "is_inf" | "is_finite") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else {
                    return Some(Type::Bool);
                };
                let ty = self.infer(&mut arg.expr)?;
                if !matches!(ty, Type::Float | Type::Float32) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("`{}` needs Float or F32, not {}", name, ty.show()),
                        "floating-point classification only applies to floats".to_string(),
                        "pass a Float or F32 value".to_string(),
                        Some(arg.expr.span()),
                    ));
                    return None;
                }
                return Some(Type::Bool);
            }
            ("core.math", "sign") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    let ty = self.infer(&mut arg.expr)?;
                    if !matches!(ty, Type::Float | Type::Float32) {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`sign` needs Float or F32, not {}", ty.show()),
                            "`sign` classifies a floating-point value as negative, zero, or positive"
                                .to_string(),
                            "pass a Float or F32 value".to_string(),
                            Some(arg.expr.span()),
                        ));
                        return None;
                    }
                }
                return Some(Type::Int);
            }
            ("core.math", "to_bits") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    let ty = self.infer(&mut arg.expr)?;
                    if !matches!(ty, Type::Float | Type::Float32) {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`to_bits` needs Float or F32, not {}", ty.show()),
                            "only floating-point values have this bit representation".to_string(),
                            "pass a Float or F32 value".to_string(),
                            Some(arg.expr.span()),
                        ));
                        return None;
                    }
                }
                return Some(Type::Int);
            }
            ("core.math", "from_bits") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.expect_core_arg(name, 0, &Type::Int, arg);
                }
                return Some(Type::Float);
            }
            (
                "core.math",
                "checked_add" | "checked_sub" | "checked_mul" | "checked_pow" | "checked_div"
                | "checked_rem",
            ) => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                }
                for (idx, arg) in args.iter_mut().enumerate() {
                    self.expect_core_arg(name, idx, &Type::Int, arg);
                }
                return Some(Type::Option(Box::new(Type::Int)));
            }
            // One whole number in, one optional whole number out: the answer
            // is absent exactly where it would leave the range.
            ("core.math", "checked_abs" | "checked_neg") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.expect_core_arg(name, 0, &Type::Int, arg);
                }
                return Some(Type::Option(Box::new(Type::Int)));
            }
            ("core.math", "isqrt" | "factorial") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.expect_core_arg(name, 0, &Type::Int, arg);
                }
                return Some(Type::Option(Box::new(Type::Int)));
            }
            ("core.math", "binomial") => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                }
                for (idx, arg) in args.iter_mut().enumerate() {
                    self.expect_core_arg(name, idx, &Type::Int, arg);
                }
                return Some(Type::Option(Box::new(Type::Int)));
            }
            ("core.math", "leading_ones" | "trailing_ones" | "digits" | "radix") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    if name == "radix" {
                        let ty = self.infer(&mut arg.expr)?;
                        if !matches!(
                            ty,
                            Type::Float
                                | Type::Float32
                                | Type::Int
                                | Type::IntN { .. }
                                | Type::InlineRange { .. }
                        ) {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`radix` needs a number, not {}", ty.show()),
                                "radix reports the base of a numeric type".to_string(),
                                "pass a Float or Int".to_string(),
                                Some(arg.expr.span()),
                            ));
                            return None;
                        }
                    } else {
                        self.expect_core_arg(name, 0, &Type::Int, arg);
                    }
                }
                return Some(Type::Int);
            }
            (
                "core.math",
                "is_normal" | "is_subnormal" | "is_canonical" | "is_signed" | "is_zero"
                | "is_integer" | "sign_bit",
            ) => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else {
                    return Some(Type::Bool);
                };
                let ty = self.infer(&mut arg.expr)?;
                if !matches!(ty, Type::Float | Type::Float32) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("`{}` needs Float or F32, not {}", name, ty.show()),
                        "floating-point classification only applies to floats".to_string(),
                        "pass a Float or F32 value".to_string(),
                        Some(arg.expr.span()),
                    ));
                    return None;
                }
                return Some(Type::Bool);
            }
            (
                "core.math",
                "next_up" | "next_down" | "copy" | "cot" | "inv" | "erf" | "erfc" | "gamma"
                | "lgamma" | "logb" | "significand" | "ulp",
            ) => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else {
                    return Some(Type::Float);
                };
                let ty = self.infer(&mut arg.expr)?;
                if !matches!(ty, Type::Float | Type::Float32) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("`{}` needs Float or F32, not {}", name, ty.show()),
                        "this math function operates on floating-point numbers".to_string(),
                        "pass a Float or F32 value".to_string(),
                        Some(arg.expr.span()),
                    ));
                    return None;
                }
                return Some(ty);
            }
            ("core.math", "zero") => {
                if !args.is_empty() {
                    self.diags.push(wrong_core_arity(name, 0, args.len(), span));
                }
                return Some(Type::Float);
            }
            ("core.math", "cmp" | "next_after" | "ldexp" | "scaleb") => {
                let wanted = 2;
                if args.len() != wanted {
                    self.diags
                        .push(wrong_core_arity(name, wanted, args.len(), span));
                }
                if name == "cmp" || name == "next_after" {
                    let Some(first) = args.get_mut(0).and_then(|a| self.infer(&mut a.expr)) else {
                        for a in args.iter_mut().skip(1) {
                            self.infer(&mut a.expr);
                        }
                        return Some(if name == "cmp" {
                            Type::Int
                        } else {
                            Type::Float
                        });
                    };
                    if !matches!(first, Type::Float | Type::Float32) {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`{}` needs Float or F32, not {}", name, first.show()),
                            "this math function operates on floating-point numbers".to_string(),
                            "pass Float or F32 values".to_string(),
                            Some(args[0].expr.span()),
                        ));
                        return None;
                    }
                    if let Some(got) = args.get_mut(1).and_then(|a| self.infer(&mut a.expr)) {
                        if got != first {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!(
                                    "`{}` needs all arguments to have the same float type",
                                    name
                                ),
                                "D-FLOATW1: mixing float widths is not allowed".to_string(),
                                "convert the arguments to the same float width".to_string(),
                                Some(args[1].expr.span()),
                            ));
                        }
                    }
                    return Some(if name == "cmp" { Type::Int } else { first });
                }
                // ldexp / scaleb: float, then whole-number exponent.
                if let Some(arg) = args.get_mut(0) {
                    let ty = self.infer(&mut arg.expr)?;
                    if !matches!(ty, Type::Float | Type::Float32) {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`{}` needs Float or F32, not {}", name, ty.show()),
                            "this math function operates on floating-point numbers".to_string(),
                            "pass a Float or F32 value".to_string(),
                            Some(arg.expr.span()),
                        ));
                        return None;
                    }
                    if let Some(exp) = args.get_mut(1) {
                        self.expect_core_arg(name, 1, &Type::Int, exp);
                    }
                    return Some(ty);
                }
                return Some(Type::Float);
            }
            ("core.math", "ilogb") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    let ty = self.infer(&mut arg.expr)?;
                    if !matches!(ty, Type::Float | Type::Float32) {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`ilogb` needs Float or F32, not {}", ty.show()),
                            "ilogb reads the exponent of a floating-point value".to_string(),
                            "pass a Float or F32 value".to_string(),
                            Some(arg.expr.span()),
                        ));
                        return None;
                    }
                }
                return Some(Type::Option(Box::new(Type::Int)));
            }
            ("core.math", "sin_cos" | "modf") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let float_ty = if let Some(arg) = args.get_mut(0) {
                    let ty = self.infer(&mut arg.expr)?;
                    if !matches!(ty, Type::Float | Type::Float32) {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`{}` needs Float or F32, not {}", name, ty.show()),
                            "this math function operates on floating-point numbers".to_string(),
                            "pass a Float or F32 value".to_string(),
                            Some(arg.expr.span()),
                        ));
                        return None;
                    }
                    ty
                } else {
                    Type::Float
                };
                let fields = if name == "sin_cos" {
                    vec![
                        ("sin".to_string(), Box::new(float_ty.clone())),
                        ("cos".to_string(), Box::new(float_ty)),
                    ]
                } else {
                    vec![
                        ("fract".to_string(), Box::new(float_ty.clone())),
                        ("whole".to_string(), Box::new(float_ty)),
                    ]
                };
                return Some(Type::Tuple(fields));
            }
            ("core.math", "frexp") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let float_ty = if let Some(arg) = args.get_mut(0) {
                    let ty = self.infer(&mut arg.expr)?;
                    if !matches!(ty, Type::Float | Type::Float32) {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`frexp` needs Float or F32, not {}", ty.show()),
                            "frexp splits a float into fraction and exponent".to_string(),
                            "pass a Float or F32 value".to_string(),
                            Some(arg.expr.span()),
                        ));
                        return None;
                    }
                    ty
                } else {
                    Type::Float
                };
                return Some(Type::Tuple(vec![
                    ("frac".to_string(), Box::new(float_ty)),
                    ("exp".to_string(), Box::new(Type::Int)),
                ]));
            }
            ("core.math", "div_mod" | "div_rem") => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                }
                for (idx, arg) in args.iter_mut().enumerate() {
                    self.expect_core_arg(name, idx, &Type::Int, arg);
                }
                return Some(Type::Tuple(vec![
                    ("quot".to_string(), Box::new(Type::Int)),
                    ("rem".to_string(), Box::new(Type::Int)),
                ]));
            }
            (
                "core.math",
                "saturating_add" | "saturating_sub" | "saturating_mul" | "gcd" | "lcm" | "int_pow",
            ) => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                }
                for (idx, arg) in args.iter_mut().enumerate() {
                    self.expect_core_arg(name, idx, &Type::Int, arg);
                }
                return Some(Type::Int);
            }
            ("core.math", "pow") => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                }
                let Some(first) = args.get_mut(0).and_then(|a| self.infer(&mut a.expr)) else {
                    for a in args.iter_mut().skip(1) {
                        self.infer(&mut a.expr);
                    }
                    return None;
                };
                if !matches!(first, Type::Float | Type::Float32) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("`pow` needs Float or F32, not {}", first.show()),
                        "`pow` operates on floating-point numbers".to_string(),
                        "pass a Float or F32 base".to_string(),
                        Some(args[0].expr.span()),
                    ));
                    return None;
                }
                if let Some(second) = args.get_mut(1).and_then(|a| self.infer(&mut a.expr)) {
                    if second != first {
                        self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`pow` needs both arguments to have the same float type, but base is {} and exponent is {}", first.show(), second.show()),
                                "D-FLOATW1: mixing float widths is not allowed — use the same width for both".to_string(),
                                "convert with `F32.from_float(value)` or `Float.from_f32(value)` to match".to_string(),
                                Some(args[1].expr.span()),
                            ));
                    }
                }
                return Some(first);
            }
            ("core.math", "abs") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else {
                    return Some(Type::Int);
                };
                let ty = self.infer(&mut arg.expr)?;
                // D-FLOATW1: abs also works on F32.
                let is_complex = matches!(&ty, Type::Named(name) if name == Syntax::TYPE_COMPLEX);
                if !matches!(&ty, Type::Int | Type::Float | Type::Float32) && !is_complex {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("`abs` needs Int, Float, or F32, not {}", ty.show()),
                        "absolute value is only defined for numbers".to_string(),
                        "pass an Int, Float, or F32".to_string(),
                        Some(arg.expr.span()),
                    ));
                    return None;
                }
                return Some(if is_complex { Type::Float } else { ty });
            }
            ("core.math", "min" | "max") => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                }
                let Some(first) = args.get_mut(0).and_then(|a| self.infer(&mut a.expr)) else {
                    for a in args.iter_mut().skip(1) {
                        self.infer(&mut a.expr);
                    }
                    return None;
                };
                if !types_comparable(&first, self.registry) {
                    self.diags.push(Diagnostic::error(
                        "E0905",
                        format!("`{}` needs comparable values", name),
                        "min/max compare their two arguments".to_string(),
                        "use Int, Float, String, Char, Bool, or a comparable type".to_string(),
                        Some(args[0].expr.span()),
                    ));
                }
                if let Some(second) = args.get_mut(1).and_then(|a| self.infer(&mut a.expr)) {
                    if second != first {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`{}` needs two values of the same type", name),
                            "min/max compare like with like".to_string(),
                            type_fix_hint(&first, &second),
                            Some(args[1].expr.span()),
                        ));
                    }
                }
                return Some(first);
            }
            ("core.math", "clamp") => {
                if args.len() != 3 {
                    self.diags.push(wrong_core_arity(name, 3, args.len(), span));
                }
                let Some(first) = args.get_mut(0).and_then(|a| self.infer(&mut a.expr)) else {
                    for a in args.iter_mut().skip(1) {
                        self.infer(&mut a.expr);
                    }
                    return None;
                };
                if !types_comparable(&first, self.registry) {
                    self.diags.push(Diagnostic::error(
                        "E0905",
                        "`clamp` needs comparable values".to_string(),
                        "clamp compares the value with its lower and upper bounds".to_string(),
                        "use Int, Float, String, Char, Bool, or a comparable type".to_string(),
                        Some(args[0].expr.span()),
                    ));
                }
                for i in 1..3 {
                    if let Some(got) = args.get_mut(i).and_then(|a| self.infer(&mut a.expr)) {
                        if got != first {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`clamp` needs all three values to have the same type"),
                                "the value and both bounds are compared together".to_string(),
                                type_fix_hint(&first, &got),
                                Some(args[i].expr.span()),
                            ));
                        }
                    }
                }
                return Some(first);
            }
            ("core.math.random", "pick") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else {
                    return Some(Type::Option(Box::new(Type::Int)));
                };
                let ty = self.infer(&mut arg.expr)?;
                if let Type::List(inner) = ty {
                    return Some(Type::Option(inner));
                }
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!("`pick` needs a list, not {}", ty.show()),
                    "random.pick chooses one item from a List".to_string(),
                    "pass a `[T]` value".to_string(),
                    Some(arg.expr.span()),
                ));
                return None;
            }
            ("core.math.random", "sample") => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return Some(Type::List(Box::new(Type::Int)));
                };
                let arg_span = arg.expr.span();
                let ty = self.infer(&mut arg.expr)?;
                if let Some(k) = args.get_mut(1).and_then(|a| self.infer(&mut a.expr)) {
                    if !matches!(&k, Type::Int | Type::InlineRange { .. }) {
                        let k_span = args.get(1).map(|a| a.expr.span()).unwrap_or(span);
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`sample` count must be Int, not {}", k.show()),
                            "random.sample chooses up to k items without replacement".to_string(),
                            "pass an Int count".to_string(),
                            Some(k_span),
                        ));
                    }
                }
                if let Type::List(inner) = ty {
                    return Some(Type::List(inner));
                }
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!("`sample` needs a list, not {}", ty.show()),
                    "random.sample chooses items from a List".to_string(),
                    "pass a `[T]` value".to_string(),
                    Some(arg_span),
                ));
                return None;
            }
            ("core.math.random", "weighted_pick") => {
                if args.len() != 2 {
                    self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                }
                let Some(items_arg) = args.get_mut(0) else {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return Some(Type::Option(Box::new(Type::Int)));
                };
                let items_span = items_arg.expr.span();
                let items_ty = self.infer(&mut items_arg.expr)?;
                if let Some(weights_arg) = args.get_mut(1) {
                    let weights_ty = self.infer(&mut weights_arg.expr);
                    if weights_ty != Some(Type::List(Box::new(Type::Float))) {
                        if let Some(got) = weights_ty {
                            self.diags.push(Diagnostic::error(
                                    "E0112",
                                    format!("`weighted_pick` weights must be [Float], not {}", got.show()),
                                    "random.weighted_pick pairs each item with a non-negative Float weight".to_string(),
                                    "pass a `[Float]` weights list".to_string(),
                                    Some(weights_arg.expr.span()),
                                ));
                        }
                    }
                }
                if let Type::List(inner) = items_ty {
                    return Some(Type::Option(inner));
                }
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!("`weighted_pick` needs a list, not {}", items_ty.show()),
                    "random.weighted_pick chooses one weighted item from a List".to_string(),
                    "pass a `[T]` value".to_string(),
                    Some(items_span),
                ));
                return None;
            }
            ("core.math.random", "shuffle") => {
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else {
                    return None;
                };
                if arg.convention != AccessConvention::Write {
                    self.diags.push(Diagnostic::error(
                        "E0202",
                        "`shuffle` edits its list in place".to_string(),
                        "the write-access marker `&` is required; pass the list with that marker"
                            .to_string(),
                        "write `random.shuffle(&xs)` with the write-access marker `&`".to_string(),
                        Some(arg.span),
                    ));
                }
                let ty = self.infer(&mut arg.expr)?;
                if !matches!(ty, Type::List(_)) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("`shuffle` needs a list, not {}", ty.show()),
                        "random.shuffle reorders a List in place".to_string(),
                        "pass a `[T]` value".to_string(),
                        Some(arg.expr.span()),
                    ));
                }
                return None;
            }
            // emitted here: `fs.read` is kept as sugar (D-IO3) and firing on every call
            // site is too noisy (breaks showcase golden tests via path-specific output).
            // Revisit when the test harness can normalise paths in exact comparisons.
            ("core.files", "read") => {}
            // D-TYPE2-TIME1=A: scheduler timer channels consume the canonical
            // Duration delta; `after(duration)` emits a unit tick and
            // `after(duration, value)` emits a typed timeout value.
            ("core.tasks", "after") => {
                if !(args.len() == 1 || args.len() == 2) {
                    self.diags.push(Diagnostic::error(
                            "E0104",
                            format!(
                                "`tasks.after` takes one Duration and an optional value, got {} argument{}",
                                args.len(),
                                if args.len() == 1 { "" } else { "s" }
                            ),
                            "a one-shot timer channel fires after the given Duration".to_string(),
                            "write `tasks.after(100ms)` or `tasks.after(100ms, fallback)`".to_string(),
                            Some(span),
                        ));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let duration_ty = self.infer(&mut args[0].expr)?;
                if !matches!(duration_ty, Type::Named(ref n) if n == "Duration") {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "`tasks.after(duration: …)` needs a Duration, not {}",
                            duration_ty.show()
                        ),
                        "timer channels use the canonical Time delta".to_string(),
                        "write `tasks.after(100ms)`".to_string(),
                        Some(args[0].expr.span()),
                    ));
                }
                let elem = if args.len() == 2 {
                    self.infer(&mut args[1].expr)?
                } else {
                    Type::Named("Unit".to_string())
                };
                if let Some(problem) =
                    self.crossing_problem(&elem, SendCrossing::ChannelSend, false)
                {
                    self.report_unsendable(
                        "timer value",
                        &elem,
                        problem,
                        SendCrossing::ChannelSend,
                        args.get(1).map(|a| a.expr.span()).unwrap_or(span),
                    );
                }
                return Some(Type::Apply {
                    name: "Receiver".to_string(),
                    args: vec![elem],
                });
            }
            // D-TYPE2-TIME1=A: interval timer consumes the canonical Duration
            // and sends tick numbers (1, 2, ...).
            ("core.tasks", "interval") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("interval", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let duration_ty = self.infer(&mut args[0].expr)?;
                if !matches!(duration_ty, Type::Named(ref n) if n == "Duration") {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "`tasks.interval(duration: …)` needs a Duration, not {}",
                            duration_ty.show()
                        ),
                        "interval channels use the canonical Time delta".to_string(),
                        "write `tasks.interval(1000ms)`".to_string(),
                        Some(args[0].expr.span()),
                    ));
                }
                return Some(Type::Apply {
                    name: "Receiver".to_string(),
                    args: vec![Type::Int],
                });
            }
            // D-ROUTE1=A: jet.http.router() → HTTPRouter.
            ("core.http", "router") => {
                if !args.is_empty() {
                    self.diags
                        .push(wrong_core_arity("router", 0, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                }
                return Some(Type::Named("HTTPRouter".to_string()));
            }
            // D-ROUTE1=A: http.parse(raw_string) → HTTPRequest (parses HTTP/1.1 bytes).
            ("core.http", "parse") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("parse", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("parse", 0, &Type::String, &mut args[0]);
                return Some(Type::Named("HTTPRequest".to_string()));
            }
            // D-HTTP-CORE2=A: the router's sole Handler propagates HTTPError.
            ("core.http", "dispatch") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("dispatch", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let router_ty = self.infer(&mut args[0].expr);
                match &router_ty {
                    Some(Type::Named(n)) if n == "HTTPRouter" => {}
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`http.dispatch` needs an HTTPRouter, not {}", other.show()),
                                "build a router with `http.router()` and register routes with `.get/.post/…`".to_string(),
                                "write `http.dispatch(router, req)`".to_string(),
                                Some(args[0].expr.span()),
                            ));
                    }
                    _ => {}
                }
                if let Some(arg) = args.get_mut(1) {
                    let req_ty = self.infer(&mut arg.expr);
                    match &req_ty {
                        Some(Type::Named(n)) if n == "HTTPRequest" => {}
                        Some(other) => {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!(
                                    "`http.dispatch` needs an HTTPRequest, not {}",
                                    other.show()
                                ),
                                "parse the raw request with `http.parse(raw)`".to_string(),
                                "write `http.dispatch(router, req)` where `req` is an HTTPRequest"
                                    .to_string(),
                                Some(arg.expr.span()),
                            ));
                        }
                        _ => {}
                    }
                }
                return Some(Type::Result {
                    ok: Box::new(Type::Named("HTTPResponse".to_string())),
                    err: Box::new(Type::Named("HTTPError".to_string())),
                });
            }
            // D-FOUND-LIFECYCLE1=A: the beginner surface binds a loopback
            // server with one shared per-request Context deadline. The legacy
            // address/handler form remains below for expert compatibility.
            ("core.http", "serve") => {
                let first_ty = args
                    .get_mut(0)
                    .map(|arg| self.infer(&mut arg.expr));
                if matches!(
                    first_ty.as_ref(),
                    Some(Some(Type::Named(name))) if name == "HTTPMux"
                ) {
                    if args.len() != 2 {
                        self.diags
                            .push(wrong_core_arity("serve", 2, args.len(), span));
                        for arg in args.iter_mut().skip(1) {
                            self.infer(&mut arg.expr);
                        }
                        return None;
                    }
                    super::net_text_time::require_exact_labels(
                        "http.serve",
                        args,
                        &[(1, "deadline")],
                        span,
                        &mut self.diags,
                    );
                    self.expect_core_arg(
                        "serve",
                        1,
                        &Type::Named("Duration".to_string()),
                        &mut args[1],
                    );
                    return Some(Type::Named("HTTPServer".to_string()));
                }

                // E2-M10: jet.http.serve(addr, handler) — blocking accept
                // loop. handler is a function or HTTPRouter.
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("serve", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("serve", 0, &Type::String, &mut args[0]);
                let saved_http_depth = self.http_handler_depth;
                let saved_escapes = self.lambda_escapes;
                self.http_handler_depth += 1;
                self.lambda_escapes = true;
                let handler_ty = self.infer(&mut args[1].expr);
                self.http_handler_depth = saved_http_depth;
                self.lambda_escapes = saved_escapes;
                match &handler_ty {
                    Some(Type::Fn { .. }) => {}
                    Some(Type::Named(n)) if n == "HTTPRouter" => {}
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!(
                                "`http.serve` handler must be a function or HTTPRouter, not {}",
                                other.show()
                            ),
                            "the handler is called with each incoming `HTTPRequest`"
                                .to_string(),
                            "pass a router (`http.router()`) or a lambda: `(req) -> HTTPResponse { … }`"
                                .to_string(),
                            Some(args[1].expr.span()),
                        ));
                    }
                    None => {}
                }
                return None;
            }
            // D-TEST-WORLD1=A: execute a callback inside one scoped deterministic
            // world. The callback receives the world handle and cannot escape
            // the call's checked lifetime.
            ("core.testing", "world") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("world", 1, args.len(), span));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                let saved_world_depth = self.deterministic_world_depth;
                self.deterministic_world_depth += 1;
                let callback_ty = self.infer_with_expected(
                    &mut args[0].expr,
                    &deterministic_world_callback_type(),
                );
                self.deterministic_world_depth = saved_world_depth;
                match &callback_ty {
                    Some(Type::Fn { params, .. }) if params.len() == 1 => {
                        if params[0]
                            != Type::Named(crate::Syntax::DETERMINISTIC_WORLD_TYPE.to_string())
                        {
                            self.diags.push(Diagnostic::error(
                                "E0104",
                                format!(
                                    "`testing.world` callback needs a DeterministicWorld parameter, got {}",
                                    params[0].show()
                                ),
                                "the callback receives the active controlled execution world"
                                    .to_string(),
                                "write `testing.world(world -> { … })`".to_string(),
                                Some(args[0].expr.span()),
                            ));
                        }
                    }
                    Some(Type::Fn { params, .. }) => {
                        self.diags.push(Diagnostic::error(
                            "E0104",
                            format!(
                                "`testing.world` callback needs one parameter, got {}",
                                params.len()
                            ),
                            "the callback receives exactly one scoped execution world"
                                .to_string(),
                            "write `testing.world(world -> { … })`".to_string(),
                            Some(args[0].expr.span()),
                        ));
                    }
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!(
                                "`testing.world` needs a lambda, not {}",
                                other.show()
                            ),
                            "the world callback runs with controlled time and scheduling"
                                .to_string(),
                            "write `testing.world(world -> { … })`".to_string(),
                            Some(args[0].expr.span()),
                        ));
                    }
                    None => {}
                }
                return Some(unit_ty());
            }
            // D-TEST-STRATEGY1=A: one optional explicit strategy replaces the
            // compiler-derived strategy when present. The runner remains the
            // shared Foundation implementation and keeps typed callbacks.
            ("core.testing", "histories") => {
                if type_args.len() == 1 {
                    self.check_declared_type(&type_args[0], span);
                } else {
                    let _ = exactly_one_type_arg(self, name, type_args, span);
                }
                let command = type_args
                    .first()
                    .cloned()
                    .unwrap_or_else(|| Type::Named("DataTree".to_string()));
                let data_tree = Type::Named("DataTree".to_string());
                let command_callback = data_callback_type(
                    Type::List(Box::new(command.clone())),
                    data_tree.clone(),
                );
                let observe_callback = data_callback_type(data_tree.clone(), data_tree);
                let strategy_callback = Type::Option(Box::new(history_strategy_type(command)));
                let expected = vec![
                    Type::Int,
                    Type::Int,
                    strategy_callback,
                    command_callback.clone(),
                    command_callback,
                    observe_callback,
                ];
                if args.len() != expected.len() {
                    self.diags
                        .push(wrong_core_arity(name, expected.len(), args.len(), span));
                }
                for (index, expected) in expected.iter().enumerate() {
                    if let Some(arg) = args.get_mut(index) {
                        self.expect_core_arg(name, index, expected, arg);
                    }
                }
                for arg in args.iter_mut().skip(expected.len()) {
                    self.infer(&mut arg.expr);
                }
                return Some(Type::Result {
                    ok: Box::new(Type::Named("TestComparison".to_string())),
                    err: Box::new(Type::String),
                });
            }
            // D-DEFER1 option B: scope.guard(() -> { … }) → ScopeGuard
            // The argument must be a zero-parameter lambda. LIFO drop order is
            // guaranteed by Rust's reverse-declaration semantics.
            ("core.mem.scope", "guard") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("guard", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let lam_ty =
                    self.infer_with_expected(&mut args[0].expr, &unit_callback_type());
                match &lam_ty {
                    Some(Type::Fn { params, .. }) => {
                        if !params.is_empty() {
                            self.diags.push(Diagnostic::error(
                                    "E0104",
                                    format!(
                                        "`scope.guard` needs a zero-parameter lambda, got {} parameter{}",
                                        params.len(),
                                        if params.len() == 1 { "" } else { "s" }
                                    ),
                                    "the guard body takes no arguments — it captures what it needs via closure".to_string(),
                                    "write `scope.guard(() -> { cleanup_code })` with no parameters".to_string(),
                                    Some(args[0].expr.span()),
                                ));
                        }
                    }
                    Some(other) => {
                        self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`scope.guard` needs a lambda, not {}", other.show()),
                                "a scope guard runs a cleanup lambda when the binding goes out of scope".to_string(),
                                "write `scope.guard(() -> { cleanup_code })`".to_string(),
                                Some(args[0].expr.span()),
                            ));
                    }
                    None => {}
                }
                return Some(Type::Named("ScopeGuard".to_string()));
            }
            // D-REACT1=B: reactive.signal(initial) → Signal<T>. The value type is
            // inferred from the initial value; an explicit annotation may guide an
            // empty/ambiguous literal via `expected_type`.
            ("core.reactive", "signal") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("signal", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                // If the binding is annotated `Signal<T>`, push `T` as the expected
                // type for the initial value so an ambiguous literal elaborates.
                let saved = self.expected_type.clone();
                if let Some(Type::Apply { name, args: ta }) = &self.expected_type {
                    if name == crate::Syntax::TYPE_SIGNAL && ta.len() == 1 {
                        self.expected_type = Some(ta[0].clone());
                    }
                }
                let init_ty = self.infer(&mut args[0].expr);
                self.expected_type = saved;
                let elem = init_ty.unwrap_or(Type::Int);
                if !self.reactive_value_ok(&elem, args[0].expr.span(), "signal") {
                    return None;
                }
                return Some(Type::Apply {
                    name: crate::Syntax::TYPE_SIGNAL.to_string(),
                    args: vec![elem],
                });
            }
            // D-REACT1=B: reactive.derived(() => expr) → Derived<T>. The compute
            // closure takes no parameters; `T` is its return type. Reading a signal
            // (`.get()`) inside the body subscribes the derived to it.
            ("core.reactive", "derived") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("derived", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let lam_ty = self.infer_retained_callback(&mut args[0].expr, None);
                let elem = match &lam_ty {
                    Some(Type::Fn { params, ret, .. }) => {
                        if !params.is_empty() {
                            self.diags.push(reactive_lambda_arity(
                                "derived",
                                params.len(),
                                args[0].expr.span(),
                            ));
                            return None;
                        }
                        match ret {
                            Some(r)
                                if matches!(
                                    r.as_ref(),
                                    Type::Named(name) if name == crate::Syntax::INTERNAL_UNIT_TYPE
                                ) =>
                            {
                                self.diags.push(reactive_derived_unit(args[0].expr.span()));
                                return None;
                            }
                            Some(r) => (**r).clone(),
                            None => {
                                self.diags.push(reactive_derived_unit(args[0].expr.span()));
                                return None;
                            }
                        }
                    }
                    Some(other) => {
                        self.diags
                            .push(reactive_not_lambda("derived", other, args[0].expr.span()));
                        return None;
                    }
                    None => return None,
                };
                if !self.reactive_value_ok(&elem, args[0].expr.span(), "derived") {
                    return None;
                }
                return Some(Type::Apply {
                    name: crate::Syntax::TYPE_DERIVED.to_string(),
                    args: vec![elem],
                });
            }
            // D-SIGNAL1: `reactive.computed` is a canonical alias for `derived`.
            ("core.reactive", "computed") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("computed", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let lam_ty = self.infer_retained_callback(&mut args[0].expr, None);
                let elem = match &lam_ty {
                    Some(Type::Fn { params, ret, .. }) => {
                        if !params.is_empty() {
                            self.diags.push(reactive_lambda_arity(
                                "computed",
                                params.len(),
                                args[0].expr.span(),
                            ));
                            return None;
                        }
                        match ret {
                            Some(r)
                                if matches!(
                                    r.as_ref(),
                                    Type::Named(name) if name == crate::Syntax::INTERNAL_UNIT_TYPE
                                ) =>
                            {
                                self.diags.push(reactive_derived_unit(args[0].expr.span()));
                                return None;
                            }
                            Some(r) => (**r).clone(),
                            None => {
                                self.diags.push(reactive_derived_unit(args[0].expr.span()));
                                return None;
                            }
                        }
                    }
                    Some(other) => {
                        self.diags.push(reactive_not_lambda(
                            "computed",
                            other,
                            args[0].expr.span(),
                        ));
                        return None;
                    }
                    None => return None,
                };
                if !self.reactive_value_ok(&elem, args[0].expr.span(), "computed") {
                    return None;
                }
                return Some(Type::Apply {
                    name: crate::Syntax::TYPE_COMPUTED.to_string(),
                    args: vec![elem],
                });
            }
            // D-RENDERTGT2=A (c133 M2): `ui.reactive_render(() -> { … })` — reactive
            // measure/layout/paint loop; re-runs when a signal read inside changes.
            ("core.ui", "reactive_render") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("reactive_render", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let lam_ty =
                    self.infer_retained_callback(&mut args[0].expr, Some(&unit_callback_type()));
                match &lam_ty {
                    Some(Type::Fn { params, .. }) => {
                        if !params.is_empty() {
                            self.diags.push(reactive_lambda_arity(
                                "reactive_render",
                                params.len(),
                                args[0].expr.span(),
                            ));
                            return None;
                        }
                    }
                    Some(other) => {
                        self.diags.push(reactive_not_lambda(
                            "reactive_render",
                            other,
                            args[0].expr.span(),
                        ));
                        return None;
                    }
                    None => return None,
                }
                return None;
            }
            // D-UI-CLOSURE1=A: the optional shortcut and accessible label
            // are metadata on one canonical button node. A callback uses the
            // trailing block spelling and is lowered to one fixed four-word
            // runtime route.
            ("core.ui", "button") => {
                if args.len() == 1 {
                    self.expect_core_arg("button", 0, &Type::String, &mut args[0]);
                    return Some(Type::Named("UiNode".to_string()));
                }
                if args.len() != 4 {
                    self.diags
                        .push(wrong_core_arity("button", 4, args.len(), span));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                self.expect_core_arg("button", 0, &Type::String, &mut args[0]);
                if !matches!(args[1].expr, Expr::Absent(_)) {
                    self.expect_core_arg(
                        "button",
                        1,
                        &Type::Named("UiShortcut".to_string()),
                        &mut args[1],
                    );
                }
                if !matches!(args[2].expr, Expr::Absent(_)) {
                    self.expect_core_arg("button", 2, &Type::String, &mut args[2]);
                }
                if !args[3].flags.is_trailing_block
                    || matches!(args[3].expr, Expr::Absent(_))
                {
                    self.diags.push(Diagnostic::error(
                        "E0335",
                        "button callbacks use a trailing block".to_string(),
                        "the callback is the button's trailing closure, after its labeled metadata"
                            .to_string(),
                        "write `button(\"Save\") { save() }`".to_string(),
                        Some(args[3].span),
                    ));
                    self.infer(&mut args[3].expr);
                    return Some(Type::Named("UiNode".to_string()));
                }
                let callback_ty =
                    self.infer_retained_callback(&mut args[3].expr, Some(&unit_callback_type()));
                if let Some(Type::Fn { params, .. }) = callback_ty {
                    if !params.is_empty() {
                        self.diags.push(reactive_lambda_arity(
                            "button on_click",
                            params.len(),
                            args[3].expr.span(),
                        ));
                    }
                }
                return Some(Type::Named("UiNode".to_string()));
            }
            // D-UI-DROP1=A: text input keeps its existing two-value node
            // constructor and adds one typed `on_drop` closure slot.
            ("core.ui", "text_input") => {
                if args.len() == 2 {
                    self.expect_core_arg("text_input", 0, &Type::String, &mut args[0]);
                    self.expect_core_arg(
                        "text_input",
                        1,
                        &Type::Named("UiImeMode".to_string()),
                        &mut args[1],
                    );
                    return Some(Type::Named("UiNode".to_string()));
                }
                if args.len() != 3 {
                    self.diags
                        .push(wrong_core_arity("text_input", 3, args.len(), span));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                if !matches!(args[2].expr, Expr::Absent(_)) {
                    let callback_ty = self.infer_retained_callback(
                        &mut args[2].expr,
                        Some(&ui_drop_callback_type()),
                    );
                    if let Some(Type::Fn { params, .. }) = callback_ty {
                        if params.len() != 1 {
                            self.diags.push(reactive_lambda_arity(
                                "text_input on_drop",
                                params.len(),
                                args[2].expr.span(),
                            ));
                        }
                    }
                }
                return Some(Type::Named("UiNode".to_string()));
            }
            // D-UI-MOUNT1=A: `ui.mount(backend, tree)` or `ui.mount(backend, tree, constraint)`.
            ("core.ui", "mount") => {
                if args.len() != 2 && args.len() != 3 {
                    self.diags
                        .push(wrong_core_arity("mount", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let backend_ty = self.infer(&mut args[0].expr);
                let tree_ty = self.infer(&mut args[1].expr);
                if let Some(ty) = &backend_ty {
                    match ty.name().as_str() {
                        "NullBackend" | "TuiBackend" | "GtkBackend" => {}
                        _ => {
                            self.diags.push(Diagnostic::error(
                                    "E0108",
                                    format!(
                                        "`ui.mount` needs a UI backend, but the first argument is {}",
                                        ty.show()
                                    ),
                                    "pass `ui.null_backend()`, `ui.tui_backend()`, or `ui.gtk_backend()`"
                                        .to_string(),
                                    "backend :: ui.tui_backend()\nui.mount(backend, tree)".to_string(),
                                    Some(args[0].expr.span()),
                                ));
                        }
                    }
                }
                if let Some(ty) = &tree_ty {
                    if ty.name() != "UiNode" {
                        self.diags.push(Diagnostic::error(
                            "E0108",
                            format!(
                                "`ui.mount` needs a `UiNode` tree, but the second argument is {}",
                                ty.show()
                            ),
                            "build the tree with `ui.text` / `ui.box` / `ui.node` / …".to_string(),
                            "ui.mount(backend, ui.box([ui.text(\"hi\")]))".to_string(),
                            Some(args[1].expr.span()),
                        ));
                    }
                }
                if args.len() == 3 {
                    if let Some(ty) = self.infer(&mut args[2].expr) {
                        if ty.name() != "SizeConstraint" {
                            self.diags.push(Diagnostic::error(
                                    "E0108",
                                    format!(
                                        "`ui.mount` optional third argument is a `SizeConstraint`, but got {}",
                                        ty.show()
                                    ),
                                    "use `ui.constraint(min_w, min_h, max_w, max_h)`".to_string(),
                                    "ui.mount(backend, tree, ui.constraint(0.0, 0.0, 80.0, 24.0))"
                                        .to_string(),
                                    Some(args[2].expr.span()),
                                ));
                        }
                    }
                }
                return Some(unit_ty());
            }
            // D-REACT1=B: reactive.effect(() => { … }) runs the body now and again
            // whenever a signal it read changes. The body is a zero-parameter,
            // unit-returning closure; the call returns a retained Effect.
            ("core.reactive", "effect") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("effect", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let lam_ty =
                    self.infer_retained_callback(&mut args[0].expr, Some(&unit_callback_type()));
                match &lam_ty {
                    Some(Type::Fn { params, .. }) => {
                        if !params.is_empty() {
                            self.diags.push(reactive_lambda_arity(
                                "effect",
                                params.len(),
                                args[0].expr.span(),
                            ));
                            return None;
                        }
                    }
                    Some(other) => {
                        self.diags
                            .push(reactive_not_lambda("effect", other, args[0].expr.span()));
                        return None;
                    }
                    None => return None,
                }
                return Some(Type::Named(crate::Syntax::TYPE_EFFECT.to_string()));
            }
            // D-EVENT1=D: first-party typed Event/Hook family. Constructors are
            // module functions so the semantic family is one Core library surface,
            // not new syntax.
            ("core.event", "scope") => {
                if !args.is_empty() {
                    self.diags
                        .push(wrong_core_arity("scope", 0, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                return Some(Type::Named(crate::Syntax::TYPE_EVENT_SCOPE.to_string()));
            }
            ("core.event", "policy_sync") => {
                if !args.is_empty() {
                    self.diags
                        .push(wrong_core_arity("policy_sync", 0, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                return Some(Type::Named(crate::Syntax::TYPE_EVENT_POLICY.to_string()));
            }
            ("core.event", "new") => {
                if !args.is_empty() {
                    self.diags
                        .push(wrong_core_arity("new", 0, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                if type_args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0904",
                        "`event.new` needs one payload type".to_string(),
                        "`Event<T>` carries exactly one typed payload for each emit".to_string(),
                        "call it with an explicit type argument: `event.new<Click>()`".to_string(),
                        Some(span),
                    ));
                    return None;
                }
                self.check_declared_type(&type_args[0], span);
                return Some(Type::Apply {
                    name: crate::Syntax::TYPE_EVENT.to_string(),
                    args: vec![type_args[0].clone()],
                });
            }
            ("core.event", "with_policy") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("with_policy", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                if type_args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                            "E0904",
                            "`event.with_policy` needs one payload type".to_string(),
                            "`Event<T>` carries exactly one typed payload for each emit".to_string(),
                            "call it with an explicit type argument: `event.with_policy<Click>(policy)`".to_string(),
                            Some(span),
                        ));
                    return None;
                }
                self.check_declared_type(&type_args[0], span);
                self.expect_core_arg(
                    "with_policy",
                    0,
                    &Type::Named(crate::Syntax::TYPE_EVENT_POLICY.to_string()),
                    &mut args[0],
                );
                return Some(Type::Apply {
                    name: crate::Syntax::TYPE_EVENT.to_string(),
                    args: vec![type_args[0].clone()],
                });
            }
            ("core.event", "async_result") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("async_result", 2, args.len(), span));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                if type_args.len() != 2 {
                    self.diags.push(Diagnostic::error(
                            "E0904",
                            "`event.async_result` needs payload and error types".to_string(),
                            "`AsyncEvent<T, E>` dispatches typed payloads and preserves typed handler failures".to_string(),
                            "call it with explicit type arguments: `event.async_result<Job, JobError>(policy, failures)`".to_string(),
                            Some(span),
                        ));
                    return None;
                }
                self.check_declared_type(&type_args[0], span);
                self.check_declared_type(&type_args[1], span);
                self.expect_core_arg(
                    "async_result",
                    0,
                    &Type::Named(crate::Syntax::TYPE_ASYNC_POLICY.to_string()),
                    &mut args[0],
                );
                self.expect_core_arg(
                    "async_result",
                    1,
                    &Type::Named(crate::Syntax::TYPE_FAILURE_POLICY.to_string()),
                    &mut args[1],
                );
                return Some(Type::Result {
                    ok: Box::new(Type::Apply {
                        name: crate::Syntax::TYPE_ASYNC_EVENT.to_string(),
                        args: vec![type_args[0].clone(), type_args[1].clone()],
                    }),
                    err: Box::new(Type::Named(
                        crate::Syntax::TYPE_EVENT_CONFIG_ERROR.to_string(),
                    )),
                });
            }
            ("core.event", "hook") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("hook", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                if type_args.len() != 2 {
                    self.diags.push(Diagnostic::error(
                            "E0904",
                            "`event.hook` needs payload and result types".to_string(),
                            "`Hook<T, R>` receives a typed payload and combines handler results into one `R`".to_string(),
                            "call it with explicit type arguments: `event.hook<Request, Decision>(fallback)`".to_string(),
                            Some(span),
                        ));
                    return None;
                }
                self.check_declared_type(&type_args[0], span);
                self.check_declared_type(&type_args[1], span);
                self.expect_core_arg("hook", 0, &type_args[1], &mut args[0]);
                return Some(Type::Apply {
                    name: crate::Syntax::TYPE_HOOK.to_string(),
                    args: vec![type_args[0].clone(), type_args[1].clone()],
                });
            }
            ("core.event", "decision_hook") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("decision_hook", 1, args.len(), span));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                if type_args.len() != 2 {
                    self.diags.push(Diagnostic::error(
                            "E0904",
                            "`event.decision_hook` needs payload and error types".to_string(),
                            "`DecisionHook<T, E>` transforms or cancels a typed payload and preserves typed failures".to_string(),
                            "call it with explicit type arguments: `event.decision_hook<Request, Err>(HookPolicy.FirstCancelElseTransform)`".to_string(),
                            Some(span),
                        ));
                    return None;
                }
                self.check_declared_type(&type_args[0], span);
                self.check_declared_type(&type_args[1], span);
                self.expect_core_arg(
                    "decision_hook",
                    0,
                    &Type::Named(crate::Syntax::TYPE_HOOK_POLICY.to_string()),
                    &mut args[0],
                );
                return Some(Type::Apply {
                    name: crate::Syntax::TYPE_DECISION_HOOK.to_string(),
                    args: vec![type_args[0].clone(), type_args[1].clone()],
                });
            }
            // D-PENDING1=B: Loadable<T,E> constructors — idle/loading/loaded/failed.
            ("core.reactive.loadable", "idle") => {
                if !args.is_empty() {
                    self.diags
                        .push(wrong_core_arity("idle", 0, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                return Some(Type::Apply {
                    name: "Loadable".to_string(),
                    args: vec![unit_ty(), unit_ty()],
                });
            }
            ("core.reactive.loadable", "loading") => {
                if !args.is_empty() {
                    self.diags
                        .push(wrong_core_arity("loading", 0, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                return Some(Type::Apply {
                    name: "Loadable".to_string(),
                    args: vec![unit_ty(), unit_ty()],
                });
            }
            ("core.reactive.loadable", "loaded") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("loaded", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let val_ty = self
                    .infer(&mut args[0].expr)
                    .unwrap_or(Type::Named("Unknown".to_string()));
                return Some(Type::Apply {
                    name: "Loadable".to_string(),
                    args: vec![val_ty, unit_ty()],
                });
            }
            ("core.reactive.loadable", "failed") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("failed", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let err_ty = self
                    .infer(&mut args[0].expr)
                    .unwrap_or(Type::Named("Unknown".to_string()));
                return Some(Type::Apply {
                    name: "Loadable".to_string(),
                    args: vec![unit_ty(), err_ty],
                });
            }
            // D-SHAPE-CTORVERB1=C: recover after the retired module factory
            // as if `ExpiringValue.new` had been written.
            ("core.time.expiring", "new") => {
                self.diags.push(unknown_core_item(module, name, span));
                if args.len() != 3 {
                    self.diags
                        .push(wrong_core_arity("new", 3, args.len(), span));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                let value_ty = self
                    .infer(&mut args[0].expr)
                    .unwrap_or(Type::Named("Unknown".to_string()));
                self.expect_core_arg(
                    "new",
                    1,
                    &Type::Named(crate::Syntax::DURATION_TYPE.to_string()),
                    &mut args[1],
                );
                self.expect_core_arg(
                    "new",
                    2,
                    &Type::Named(crate::Syntax::CLOCK_TYPE.to_string()),
                    &mut args[2],
                );
                return Some(Type::Apply {
                    name: crate::Syntax::EXPIRING_VALUE_TYPE.to_string(),
                    args: vec![value_ty],
                });
            }
            // #1465: POSIX process/session control requires `#Unsafe` (I1).
            (
                "core.sys",
                "fork" | "setuid" | "setgid" | "setpgid" | "setpgrp" | "setsid" | "initgroups"
                | "kill" | "wait" | "waitpid" | "pipe" | "close_fd" | "mkfifo" | "umask"
                | "getpriority" | "setpriority" | "utime" | "atexit" | "stop",
            ) => {
                if !self.in_unsafe {
                    self.diags.push(Diagnostic::error(
                            "E3101",
                            format!("`core.sys.{name}` requires an audited `#Unsafe` region"),
                            "POSIX process and session control can change credentials, signals, and process topology (I1)".to_string(),
                            format!("wrap the call in `#Unsafe(\"posix {name}: …\") {{ … }}` and gate the host OS with `@if @build.os` / `#Target(OS.*)`"),
                            Some(span),
                        ));
                }
                // Continue into shared fixed-signature checking below.
            }
            // U13 (D-JPK-SECRETCRYPTO1): `core.crypto.vault.get` reads a
            // decrypted repo secret and returns `String?`; it is not a raw
            // key-material operation, so the expert arm below must not
            // claim it. `docs/spec/spec.md` § Secrets states its whole gate:
            // the `Secret` effect (E1264, and `Secret` is the one effect
            // denied even with no declared row at all) plus unconditional
            // denial at build/comptime time (E1265, no `#Impure` escape).
            // The catch-all demanded a third gate it describes as "raw key
            // import", which this call is not, and that rejected the shipped
            // executable spec `examples/features/crypto/vault_secret.jet`
            // — which declares `-[Secret, IO]>` exactly as the spec says
            // (I5, #2018). Empty body: fall through to the shared
            // fixed-signature check like every other gated arm here.
            ("core.crypto.vault", "get") => {}
            // D-CRYPTOENV1=A: expert-only raw crypto — requires import + #Unsafe gate.
            ("core.crypto.expert" | "core.crypto.vault", _) => {
                let has_import = self
                    .core_imports
                    .values()
                    .any(|imported| imported == module);
                if !has_import {
                    let (what, why, fix) = if module == "core.crypto.expert" {
                        (
                                format!("`core.crypto.expert.{name}` bypasses the misuse-resistant envelope"),
                                "raw AES/ChaCha primitives are expert-only and hide none of the footguns that `crypto.seal`/`open` prevent (D-CRYPTOENV1)".to_string(),
                                "use `core.crypto.seal` / `core.crypto.open` for encryption, or add `use core.crypto.expert` inside an audited `#Unsafe(\"reason\")` region".to_string(),
                            )
                    } else {
                        (
                                format!("`{module}.{name}` is an expert-only key material operation"),
                                "raw key material operations bypass the misuse-resistant typed surface".to_string(),
                                format!("import `{module}` and call it inside an audited `#Unsafe(\"reason\")` region"),
                            )
                    };
                    self.diags
                        .push(Diagnostic::error("E0510", what, why, fix, Some(span)));
                } else if !self.in_unsafe {
                    let (what, why, fix) = if module == "core.crypto.expert" {
                        (
                                format!("`core.crypto.expert.{name}` requires an audited `#Unsafe` region"),
                                "raw crypto primitives may only run inside an explicit expert-tier gate (I1)".to_string(),
                                "wrap the call in `#Unsafe(\"crypto expert: …\") { … }` or use `crypto.seal`/`open` instead".to_string(),
                            )
                    } else {
                        (
                            format!("`{module}.{name}` requires an audited `#Unsafe` region"),
                            "raw key import may only run inside an explicit expert-tier gate (I1)"
                                .to_string(),
                            "wrap the call in `#Unsafe(\"vault key import: …\") { … }`".to_string(),
                        )
                    };
                    self.diags
                        .push(Diagnostic::error("E0510", what, why, fix, Some(span)));
                }
                // Continue into shared fixed-signature checking below. The
                // unsafe gate is additional policy, never a type/arity bypass.
            }
            // D-NETDEP1=A / D-HTTPLIB1=A: HTTP constructors.
            ("core.http.client", "get") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("get", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_url_arg("get", 0, &mut args[0]);
                return Some(Type::Result {
                    ok: Box::new(Type::Named("HTTPResponse".to_string())),
                    err: Box::new(Type::Named("HTTPError".to_string())),
                });
            }
            ("core.http.client", "post") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("post", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_url_arg("post", 0, &mut args[0]);
                self.expect_core_arg("post", 1, &Type::String, &mut args[1]);
                return Some(Type::Result {
                    ok: Box::new(Type::Named("HTTPResponse".to_string())),
                    err: Box::new(Type::Named("HTTPError".to_string())),
                });
            }
            ("core.http.client", "request") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("request", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("request", 0, &Type::String, &mut args[0]);
                self.expect_url_arg("request", 1, &mut args[1]);
                return Some(Type::Named("HTTPRequest".to_string()));
            }
            // D-WS1=B: WebSocket entry points.
            ("core.net.ws", "connect") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("connect", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("connect", 0, &Type::String, &mut args[0]);
                return Some(Type::Result {
                    ok: Box::new(Type::Named("WsConn".to_string())),
                    err: Box::new(Type::Named("WsError".to_string())),
                });
            }
            ("core.net.ws", "upgrade") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("upgrade", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg(
                    "upgrade",
                    0,
                    &Type::Named("HTTPRequest".to_string()),
                    &mut args[0],
                );
                return Some(Type::Result {
                    ok: Box::new(Type::Named("WsConn".to_string())),
                    err: Box::new(Type::Named("WsError".to_string())),
                });
            }
            // D-BROWSER-AUTO1=A: versioned native BiDi protocol core.
            ("core.web.browser", "profile") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("profile", 1, args.len(), span));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                self.expect_core_arg("profile", 0, &Type::String, &mut args[0]);
                return Some(Type::Result {
                    ok: Box::new(Type::Named("BrowserProfile".to_string())),
                    err: Box::new(Type::Named("BrowserError".to_string())),
                });
            }
            ("core.web.browser", "timeout") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("timeout", 1, args.len(), span));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                self.expect_core_arg("timeout", 0, &Type::Int, &mut args[0]);
                return Some(Type::Result {
                    ok: Box::new(Type::Named("BrowserTimeout".to_string())),
                    err: Box::new(Type::Named("BrowserError".to_string())),
                });
            }
            ("core.web.browser", "locked") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("locked", 1, args.len(), span));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                self.expect_core_arg("locked", 0, &Type::String, &mut args[0]);
                return Some(Type::Result {
                    ok: Box::new(Type::Named("BrowserLocked".to_string())),
                    err: Box::new(Type::Named("BrowserError".to_string())),
                });
            }
            ("core.web.browser", "connect") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("connect", 1, args.len(), span));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                self.expect_core_arg("connect", 0, &Type::String, &mut args[0]);
                return Some(Type::Result {
                    ok: Box::new(Type::Named("Browser".to_string())),
                    err: Box::new(Type::Named("BrowserError".to_string())),
                });
            }
            ("core.web.browser", "connect_profile") => {
                if args.len() != 3 {
                    self.diags
                        .push(wrong_core_arity("connect_profile", 3, args.len(), span));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                self.expect_core_arg("connect_profile", 0, &Type::String, &mut args[0]);
                self.expect_core_arg(
                    "connect_profile",
                    1,
                    &Type::Named("BrowserProfile".to_string()),
                    &mut args[1],
                );
                self.expect_core_arg(
                    "connect_profile",
                    2,
                    &Type::Named("BrowserTimeout".to_string()),
                    &mut args[2],
                );
                return Some(Type::Result {
                    ok: Box::new(Type::Named("Browser".to_string())),
                    err: Box::new(Type::Named("BrowserError".to_string())),
                });
            }
            ("core.http.server", "mux") => {
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return Some(Type::Named("HTTPMux".to_string()));
            }
            ("core.http.server", "bind") => {
                // The shared Core parameter binder materializes both optional
                // labeled slots before this checked branch reaches the
                // backend. Keeping one four-value shape makes the AOT, JIT,
                // and evaluator routes agree on the server ABI.
                if args.len() != 4 {
                    self.diags.push(Diagnostic::error(
                            "E0104",
                            format!("`bind` expects address, mux, optional `tls:`, and optional `deadline:`, got {}", args.len()),
                            "HTTPS binding and request deadlines use named options while plaintext keeps the same entry point".to_string(),
                            "write `Server.bind(addr, mux)`, add `tls: Server.tls(cert, key)`, or add `deadline: duration`".to_string(),
                            Some(span),
                        ));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                self.expect_core_arg("bind", 0, &Type::String, &mut args[0]);
                self.expect_core_arg("bind", 1, &Type::Named("HTTPMux".to_string()), &mut args[1]);
                if !matches!(&args[2].expr, Expr::Absent(_)) {
                    self.expect_core_arg(
                        "bind",
                        2,
                        &Type::Named("HTTPServerTls".to_string()),
                        &mut args[2],
                    );
                }
                if !matches!(&args[3].expr, Expr::Absent(_)) {
                    self.expect_core_arg(
                        "bind",
                        3,
                        &Type::Named("Duration".to_string()),
                        &mut args[3],
                    );
                }
                return Some(Type::Result {
                    ok: Box::new(Type::Named("HTTPServer".to_string())),
                    err: Box::new(Type::Named("HTTPError".to_string())),
                });
            }
            ("core.http.server", "serve") => {
                if args.len() != 4 {
                    self.diags.push(Diagnostic::error(
                            "E0104",
                            format!("`serve` expects address, mux, optional `tls:`, and optional `deadline:`, got {}", args.len()),
                            "HTTPS serving and request deadlines use named options while plaintext keeps the same entry point".to_string(),
                            "write `Server.serve(addr, mux)`, add `tls: Server.tls(cert, key)`, or add `deadline: duration`".to_string(),
                            Some(span),
                        ));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                self.expect_core_arg("serve", 0, &Type::String, &mut args[0]);
                // second arg is a Mux — just infer it
                self.infer(&mut args[1].expr);
                if !matches!(&args[2].expr, Expr::Absent(_)) {
                    self.expect_core_arg(
                        "serve",
                        2,
                        &Type::Named("HTTPServerTls".to_string()),
                        &mut args[2],
                    );
                }
                if !matches!(&args[3].expr, Expr::Absent(_)) {
                    self.expect_core_arg(
                        "serve",
                        3,
                        &Type::Named("Duration".to_string()),
                        &mut args[3],
                    );
                }
                return Some(Type::Result {
                    ok: Box::new(unit_ty()),
                    err: Box::new(Type::Named("HTTPError".to_string())),
                });
            }
            ("core.http.server", "serve_once") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("serve_once", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("serve_once", 0, &Type::String, &mut args[0]);
                self.infer(&mut args[1].expr);
                return Some(Type::Result {
                    ok: Box::new(unit_ty()),
                    err: Box::new(Type::Named("HTTPError".to_string())),
                });
            }
            ("core.http.server", "serve_once_listener") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("serve_once_listener", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg(
                    "serve_once_listener",
                    0,
                    &Type::Named("TcpListener".to_string()),
                    &mut args[0],
                );
                self.infer(&mut args[1].expr);
                return Some(Type::Result {
                    ok: Box::new(unit_ty()),
                    err: Box::new(Type::Named("HTTPError".to_string())),
                });
            }
            ("core.http.server", "tls") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("tls", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("tls", 0, &Type::String, &mut args[0]);
                self.expect_core_arg("tls", 1, &Type::String, &mut args[1]);
                return Some(Type::Named("HTTPServerTls".to_string()));
            }
            ("core.http.server", "response") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("response", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("response", 0, &Type::Int, &mut args[0]);
                self.expect_core_arg("response", 1, &Type::String, &mut args[1]);
                return Some(Type::Named("HTTPResponse".to_string()));
            }
            ("core.http.server", "sse") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("sse", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("sse", 0, &Type::String, &mut args[0]);
                return Some(Type::Named("HTTPResponse".to_string()));
            }
            ("core.http.server", "static_file") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("static_file", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("static_file", 0, &Type::String, &mut args[0]);
                self.expect_core_arg("static_file", 1, &Type::String, &mut args[1]);
                return Some(Type::Result {
                    ok: Box::new(Type::Named("HTTPResponse".to_string())),
                    err: Box::new(Type::Named("HTTPError".to_string())),
                });
            }
            ("core.http.server", "static_file_range") => {
                if args.len() != 3 {
                    self.diags
                        .push(wrong_core_arity("static_file_range", 3, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg(
                    "static_file_range",
                    0,
                    &Type::Named("HTTPRequest".to_string()),
                    &mut args[0],
                );
                self.expect_core_arg("static_file_range", 1, &Type::String, &mut args[1]);
                self.expect_core_arg("static_file_range", 2, &Type::String, &mut args[2]);
                return Some(Type::Result {
                    ok: Box::new(Type::Named("HTTPResponse".to_string())),
                    err: Box::new(Type::Named("HTTPError".to_string())),
                });
            }
            ("core.http.server", "access_log") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("access_log", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg(
                    "access_log",
                    0,
                    &Type::Named("HTTPRequest".to_string()),
                    &mut args[0],
                );
                self.expect_core_arg("access_log", 1, &Type::Int, &mut args[1]);
                return Some(Type::String);
            }
            ("core.http.server", "request_id") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("request_id", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg(
                    "request_id",
                    0,
                    &Type::Named("HTTPMux".to_string()),
                    &mut args[0],
                );
                return Some(Type::Named("Unit".to_string()));
            }
            ("core.http.server", "mux_handler") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("mux_handler", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg(
                    "mux_handler",
                    0,
                    &Type::Named("HTTPMux".to_string()),
                    &mut args[0],
                );
                return Some(Type::Named("HTTPHandler".to_string()));
            }
            // D-HTTP-STATIC-FILES1=A: mount a directory under a prefix. The
            // trailing `index`, `dotfiles`, and `follow_links` options are
            // the expert opt-in; leaving them off keeps the safe defaults.
            ("core.http.server", "static_files") => {
                if args.len() < 3 || args.len() > 6 {
                    self.diags
                        .push(wrong_core_arity("static_files", 3, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg(
                    "static_files",
                    0,
                    &Type::Named("HTTPMux".to_string()),
                    &mut args[0],
                );
                self.expect_core_arg("static_files", 1, &Type::String, &mut args[1]);
                self.expect_core_arg("static_files", 2, &Type::String, &mut args[2]);
                for index in 3..args.len() {
                    self.expect_core_arg("static_files", index, &Type::Bool, &mut args[index]);
                }
                return Some(Type::Named("Unit".to_string()));
            }
            ("core.http.middleware", "timeout") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("timeout", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg(
                    "timeout",
                    0,
                    &Type::Named("Duration".to_string()),
                    &mut args[0],
                );
                self.expect_core_arg(
                    "timeout",
                    1,
                    &Type::Named("HTTPHandler".to_string()),
                    &mut args[1],
                );
                return Some(Type::Named("HTTPHandler".to_string()));
            }
            ("core.http.middleware", "body_limit") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("body_limit", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("body_limit", 0, &Type::Int, &mut args[0]);
                self.expect_core_arg(
                    "body_limit",
                    1,
                    &Type::Named("HTTPHandler".to_string()),
                    &mut args[1],
                );
                return Some(Type::Named("HTTPHandler".to_string()));
            }
            // D-HTTP-CORS1=A: one policy value, then one install on the mux.
            // `origins` takes a plain `[String]` list or the `.Any` case.
            ("core.http.server", "cors_policy") => {
                if args.is_empty() || args.len() > 5 {
                    self.diags
                        .push(wrong_core_arity("cors_policy", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                let origins = self.infer(&mut args[0].expr);
                let list_form = matches!(&origins, Some(Type::List(_)));
                let case_form =
                    matches!(&origins, Some(Type::Named(name)) if name == "HTTPCorsOrigins");
                if !list_form && !case_form {
                    self.expect_core_arg(
                        "cors_policy",
                        0,
                        &Type::Named("HTTPCorsOrigins".to_string()),
                        &mut args[0],
                    );
                }
                let string_list = Type::List(Box::new(Type::String));
                for (index, want) in [
                    (1, &string_list),
                    (2, &string_list),
                    (3, &Type::Bool),
                    (4, &Type::Int),
                ] {
                    if let Some(arg) = args.get_mut(index) {
                        self.expect_core_arg("cors_policy", index, want, arg);
                    }
                }
                return Some(Type::Result {
                    ok: Box::new(Type::Named("HTTPCorsPolicy".to_string())),
                    err: Box::new(Type::Named("HTTPError".to_string())),
                });
            }
            ("core.http.server", "cors") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("cors", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("cors", 0, &Type::Named("HTTPMux".to_string()), &mut args[0]);
                self.expect_core_arg(
                    "cors",
                    1,
                    &Type::Named("HTTPCorsPolicy".to_string()),
                    &mut args[1],
                );
                return Some(Type::Named("Unit".to_string()));
            }
            // D-HTTP-JSON1=A: one typed JSON response.
            ("core.http.server", "json") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("json", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("json", 0, &Type::Int, &mut args[0]);
                if let Some(value) = self.infer(&mut args[1].expr) {
                    self.check_encodable(&value, args[1].expr.span());
                }
                return Some(Type::Named("HTTPResponse".to_string()));
            }
            ("core.http.middleware", "compress") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("compress", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg(
                    "compress",
                    0,
                    &Type::Named("HTTPCompressEncoding".to_string()),
                    &mut args[0],
                );
                self.expect_core_arg(
                    "compress",
                    1,
                    &Type::Named("HTTPHandler".to_string()),
                    &mut args[1],
                );
                return Some(Type::Named("HTTPHandler".to_string()));
            }
            ("core.http.middleware", "access_log") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("access_log", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg(
                    "access_log",
                    0,
                    &Type::Named("HTTPHandler".to_string()),
                    &mut args[0],
                );
                return Some(Type::Named("HTTPHandler".to_string()));
            }
            // D-TIMEDEPTH1=A: civil-time constructors.
            ("core.time", "new") => {
                if args.len() != 3 {
                    self.diags
                        .push(wrong_core_arity("new", 3, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("new", 0, &Type::Int, &mut args[0]);
                self.expect_core_arg("new", 1, &Type::Int, &mut args[1]);
                self.expect_core_arg("new", 2, &Type::Int, &mut args[2]);
                return Some(Type::Named("LocalDate".to_string()));
            }
            ("core.time", "today") => {
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return Some(Type::Named("LocalDate".to_string()));
            }
            ("core.time", "parse") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("parse", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("parse", 0, &Type::String, &mut args[0]);
                return Some(Type::Result {
                    ok: Box::new(Type::Named("LocalDate".to_string())),
                    err: Box::new(Type::String),
                });
            }
            ("core.time", "from_timestamp") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("from_timestamp", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("from_timestamp", 0, &Type::Int, &mut args[0]);
                return Some(Type::Named("DateTime".to_string()));
            }
            // D-APPROX1=A: sketch constructors.
            ("core.data.sketch.hll", "new") => {
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return Some(Type::Named("HyperLogLog".to_string()));
            }
            ("core.data.sketch.tdigest", "new") => {
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return Some(Type::Named("TDigest".to_string()));
            }
            ("core.data.sketch.cms", "new") => {
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return Some(Type::Named("CountMinSketch".to_string()));
            }
            ("core.data.sketch.reservoir", "new") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("new", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("new", 0, &Type::Int, &mut args[0]);
                return Some(Type::Named("ReservoirSampler".to_string()));
            }
            // D-NUMTYPE1=A: core.math.fraction(top, bottom) answers an exact
            // ratio, or nothing when the bottom is zero.
            ("core.math", "fraction") => {
                if args.len() != 2 {
                    self.diags
                        .push(wrong_core_arity("fraction", 2, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("fraction", 0, &Type::Int, &mut args[0]);
                self.expect_core_arg("fraction", 1, &Type::Int, &mut args[1]);
                return Some(Type::Option(Box::new(Type::Named(
                    crate::Syntax::TYPE_FRACTION.to_string(),
                ))));
            }
            // D-CORE-NUMERIC1=A: `core.math.decimal(s)` → `Decimal`.
            ("core.math", "decimal") => {
                if args.len() != 1 {
                    self.diags
                        .push(wrong_core_arity("decimal", 1, args.len(), span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
                self.expect_core_arg("decimal", 0, &Type::String, &mut args[0]);
                return Some(Type::Named(crate::Syntax::TYPE_DECIMAL.to_string()));
            }
            // D-TEXTWIDTH1=B: `text.display_width(s)` (portable default,
            // returns bare `Int`) vs `text.display_width(s, policy: cjk)`
            // (the `.Reject` control policy can fail, so it returns
            // `Int !TextError`). Named-arg dispatch mirrors `game.run`.
            ("core.text", "display_width") => match args.len() {
                1 => {
                    self.expect_core_arg("display_width", 0, &Type::String, &mut args[0]);
                    return Some(Type::Int);
                }
                2 => {
                    let params = vec![
                        crate::Sema::CallBinder::BindParam {
                            label: "text",
                            name: "text",
                            zone: ParamZone::PositionalOnly,
                            default: None,
                            convention: AccessConvention::Read,
                            ty: None,
                            variadic: false,
                            core_default: None,
                        },
                        crate::Sema::CallBinder::BindParam {
                            label: "policy",
                            name: "policy",
                            zone: ParamZone::Either,
                            default: None,
                            convention: AccessConvention::Read,
                            ty: None,
                            variadic: false,
                            core_default: None,
                        },
                    ];
                    if crate::Sema::CallBinder::bind_call_args(
                        "display_width",
                        &params,
                        args,
                        span,
                        &mut self.diags,
                    )
                    .is_none()
                    {
                        for arg in args.iter_mut() {
                            self.infer(&mut arg.expr);
                        }
                        return Some(Type::Result {
                            ok: Box::new(Type::Int),
                            err: Box::new(Type::Named("TextError".to_string())),
                        });
                    }
                    self.expect_core_arg("display_width", 0, &Type::String, &mut args[0]);
                    self.expect_core_arg(
                        "display_width",
                        1,
                        &Type::Named("TextWidth".to_string()),
                        &mut args[1],
                    );
                    return Some(Type::Result {
                        ok: Box::new(Type::Int),
                        err: Box::new(Type::Named("TextError".to_string())),
                    });
                }
                n => {
                    self.diags
                        .push(wrong_core_arity("display_width", 1, n, span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
            },
            _ => {}
        }

        // D-FFI-SH1=A / D-UNIFYLIT1=A: `process.run` accepts `Sh` (from
        // `Sh.{"…"}` / `Sh.raw`) or an explicit `[String]` argv list.
        // Bare `"…"` is `String` and E0149 — no silent typed-text rewrite.
        if module == "core.process" && name == "run" {
            let Some((_, ret)) = sig else { unreachable!() };
            if !matches!(args.len(), 1 | 2) {
                self.diags.push(wrong_core_arity(name, 1, args.len(), span));
            }
            if let Some(arg) = args.get_mut(0) {
                let got = self.infer(&mut arg.expr);
                if let Some(got) = got {
                    let explicit_argv = matches!(
                        got,
                        Type::List(ref elem) | Type::FixedList { ref elem, .. }
                            if **elem == Type::String
                    );
                    if got != Type::Named(Syntax::TYPE_SH.to_string()) && !explicit_argv {
                        if let Some(diag) = crate::Sema::Diagnostics::typed_text_mismatch(
                            &Type::Named(Syntax::TYPE_SH.to_string()),
                            &got,
                            arg.expr.span(),
                        ) {
                            self.diags.push(diag);
                        } else {
                            self.diags.push(Diagnostic::error(
                                    "E0112",
                                    format!("`run` needs Sh, but this is {}", got.show()),
                                    "process.run executes a checked argv command without a shell".to_string(),
                                    "pass a Sh literal, or build an explicit argv command with process.cmd(argv).run()".to_string(),
                                    Some(arg.expr.span()),
                                ));
                        }
                    }
                }
            }
            if args.len() == 2 {
                self.expect_core_arg(
                    name,
                    1,
                    &Type::Named(crate::Syntax::AUTHORITY_HANDLE_TYPE.to_string()),
                    &mut args[1],
                );
                crate::Sema::Effects::check_authority_boundary_scope(self, &args[1].expr);
                args[1].flags.authority_boundary = true;
            }
            for arg in args.iter_mut().skip(2) {
                self.infer(&mut arg.expr);
            }
            return ret;
        }

        // D-PLUGIN-AUTHORITY1: plugin loading has one exact ABI. The path and
        // tightened Authority are both required; omitted authority is never
        // replaced with ambient host policy. A static artifact also selects a
        // frozen Component interface before body checking can see methods.
        if module == "core.plugin" && name == "load" {
            if args.len() != 2 {
                self.diags.push(wrong_core_arity(name, 2, args.len(), span));
            }
            if let Some(arg) = args.get_mut(0) {
                self.expect_core_arg(name, 0, &Type::String, arg);
            }
            if let Some(arg) = args.get_mut(1) {
                self.expect_core_arg(
                    name,
                    1,
                    &Type::Named(crate::Syntax::AUTHORITY_HANDLE_TYPE.to_string()),
                    arg,
                );
                crate::Sema::Effects::check_authority_boundary_scope(self, &arg.expr);
                arg.flags.authority_boundary = true;
            }
            for arg in args.iter_mut().skip(2) {
                self.infer(&mut arg.expr);
            }

            let artifact = args.first().and_then(|arg| match &arg.expr {
                Expr::Str(parts, _) => match parts.as_slice() {
                    [crate::AST::StrPart::Lit(path)] => Some(path.as_str()),
                    _ => None,
                },
                _ => None,
            });
            let interface = artifact.and_then(|path| self.plugin_interfaces.interface_for_artifact(path));
            let Some(interface) = interface else {
                self.diags.push(Diagnostic::error(
                    "E1257",
                    "plugin.load needs a registered Component interface".to_string(),
                    "plugin members are available only when the artifact is a literal with a frozen interface snapshot"
                        .to_string(),
                    "pass a literal registered artifact path, or publish its plugin interface snapshot"
                        .to_string(),
                    args.first()
                        .map(|arg| arg.expr.span())
                        .or(Some(span)),
                ));
                return Some(Type::Named("Plugin".to_string()));
            };
            return Some(Type::Apply {
                name: "Plugin".to_string(),
                args: vec![Type::Named(interface.identity.clone())],
            });
        }

        let compute_alias_ret = if module == "core.compute" {
            compute_alias_return(name, args)
        } else {
            None
        };
        let Some((params, ret)) = sig else {
            self.diags.push(unknown_core_item(module, name, span));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            let _ = alias_span;
            return None;
        };
        if module == "core.sys" && name == "on_interrupt" {
            // D-OSINTERRUPT1/I3: this is the one fixed callback crossing.
            // Keep the callback expectation tied to the complete one-slot
            // signature. Do not silently type-check only `params.first()`
            // if the registry row ever drifts to another shape.
            debug_assert_eq!(params.len(), 1);
            if args.len() != params.len() {
                self.diags
                    .push(wrong_core_arity(name, params.len(), args.len(), span));
            }
            if params.len() == 1 {
                let (conv, param_ty) = &params[0];
                if let Some(arg) = args.get_mut(0) {
                    debug_assert_eq!(*conv, AccessConvention::Read);
                    let saved_lambda_escapes = self.lambda_escapes;
                    let saved_callback_depth = self.interrupt_callback_depth;
                    self.lambda_escapes = true;
                    self.interrupt_callback_depth += 1;
                    self.expect_core_arg(name, 0, param_ty, arg);
                    self.interrupt_callback_depth = saved_callback_depth;
                    self.lambda_escapes = saved_lambda_escapes;
                }
            }
            for arg in args.iter_mut().skip(params.len()) {
                self.infer(&mut arg.expr);
            }
            return ret;
        }
        // D-COMPUTE-RAW1/I1: a raw kernel contract is an expert escape
        // hatch, not a safe compute constructor. Keep the fixed signature
        // and normal type checking, but require the same lexical audit
        // gate as the other low-level memory/device operations.
        if module == "core.crypto.expert" && name == "x25519_raw" {
            let validation_diag_start = self.diags.len();
            if !(2..=3).contains(&args.len()) {
                self.diags.push(wrong_core_arity(name, 2, args.len(), span));
            }
            for (i, ((conv, param_ty), arg)) in params.iter().zip(args.iter_mut()).enumerate() {
                debug_assert_eq!(*conv, AccessConvention::Read);
                self.expect_core_arg(name, i, param_ty, arg);
            }
            for arg in args.iter_mut().skip(params.len()) {
                self.infer(&mut arg.expr);
            }
            if self.in_unsafe
                && self
                    .core_imports
                    .values()
                    .any(|imported| imported == module)
                && (2..=3).contains(&args.len())
                && self.diags.len() == validation_diag_start
            {
                if let Some(diagnostic) = crypto_misuse_diagnostic(self, module, name, args) {
                    self.fx_pending_diagnostics.push(diagnostic);
                }
            }
            return ret;
        }
        let validation_diag_start = self.diags.len();
        let raw_envelope_diagnostic = safe_envelope_raw_argument(module, name, args, params.len());
        if args.len() != params.len() && raw_envelope_diagnostic.is_none() {
            self.diags
                .push(wrong_core_arity(name, params.len(), args.len(), span));
        }
        for (i, ((conv, param_ty), arg)) in params.iter().zip(args.iter_mut()).enumerate() {
            let retained_ui_callback = matches!(
                (module, name),
                ("core.ui", "preview" | "playground")
            ) && matches!(param_ty, Type::Fn { .. });
            let saved_callback_escapes = self.lambda_escapes;
            if retained_ui_callback {
                self.lambda_escapes = true;
            }
            if *conv == AccessConvention::Move {
                self.expect_core_arg_moving(name, i, param_ty, arg);
                if retained_ui_callback {
                    self.lambda_escapes = saved_callback_escapes;
                    if !matches!(arg.expr, Expr::Absent(_)) {
                        self.check_stream_callback_expr(&arg.expr, param_ty);
                    }
                }
                self.finish_core_call_ownership(name, i, arg, *conv, param_ty);
                continue;
            }
            if *conv == AccessConvention::Write && arg.convention != AccessConvention::Write {
                self.diags.push(Diagnostic::error(
                    "E0202",
                    format!(
                        "argument {} to `{}` requires the write-access marker `&`",
                        i + 1,
                        name
                    ),
                    "this standard library call edits that value in place".to_string(),
                    format!(
                        "write the write-access marker `&`: `{}value` for this argument",
                        Syntax::SIGIL_WRITE
                    ),
                    Some(arg.span),
                ));
            } else if *conv == AccessConvention::Write {
                if let Expr::Ident(ident, ident_span) = &arg.expr {
                    if let Some(info) = self.lookup(ident) {
                        if !info.mutable {
                            let mut diagnostic = Diagnostic::error(
                                "E0111",
                                format!(
                                    "`{}` was made with `{}`, so it can't be changed",
                                    ident,
                                    Syntax::SIGIL_BIND_IMMUT
                                ),
                                format!(
                                    "`{}` will change this value, so it must be mutable (`{}`)",
                                    name,
                                    Syntax::SIGIL_BIND_MUT
                                ),
                                format!(
                                    "declare it with `{} {} ...`",
                                    ident,
                                    Syntax::SIGIL_BIND_MUT
                                ),
                                Some(*ident_span),
                            );
                            if let Some(sigil_span) = info.binding_sigil_span {
                                diagnostic = diagnostic.with_edit(crate::Diagnostics::TextEdit {
                                    span: sigil_span,
                                    new_text: Syntax::SIGIL_BIND_MUT.to_string(),
                                });
                            }
                            self.diags.push(diagnostic);
                        }
                    }
                }
            }
            self.expect_core_arg(name, i, param_ty, arg);
            if retained_ui_callback {
                self.lambda_escapes = saved_callback_escapes;
                if !matches!(arg.expr, Expr::Absent(_)) {
                    self.check_stream_callback_expr(&arg.expr, param_ty);
                }
            }
        }
        for arg in args.iter_mut().skip(params.len()) {
            self.infer(&mut arg.expr);
        }
        let expert_gate_ok = module != "core.crypto.expert"
            || (self.in_unsafe
                && self
                    .core_imports
                    .values()
                    .any(|imported| imported == "core.crypto.expert"));
        if expert_gate_ok
            && (args.len() == params.len() || raw_envelope_diagnostic.is_some())
            && self.diags.len() == validation_diag_start
        {
            if let Some(diagnostic) = raw_envelope_diagnostic
                .or_else(|| crypto_misuse_diagnostic(self, module, name, args))
            {
                self.fx_pending_diagnostics.push(diagnostic);
            }
        }
        if module == "core.time" && name == "clock" {
            ret.map(crate::Sema::Diagnostics::deterministic_clock_type)
        } else {
            compute_alias_ret.or(ret)
        }
    }
}
