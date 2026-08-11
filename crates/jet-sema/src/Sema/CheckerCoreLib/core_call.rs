use crate::AST::{AccessConvention, Expr, ParamZone, Type};
use crate::Diagnostics::{CryptoMisuseReason, Diagnostic, Span};
use crate::Sema::Checker;
use crate::Sema::Diagnostics::{is_displayable, is_printable, type_fix_hint, types_comparable};
use crate::Sema::Effects::{core_effect, e0746, is_irreversible_effect};
use crate::Sema::FFI::e3301;
use crate::Sema::Purity::{e3401, e3403, is_impure_core, is_nondeterministic_core};
use crate::Sema::SendCrossing;
use crate::Syntax;
use super::alloc_ptrs::{e3101, io_error_ty, ptr_elem, result_ty};
use super::core_types::{decode_error_ty, u8_ty, unit_ty};
use super::fixed_sigs::{core_fixed_sig, core_fixed_sig_for_row};
use super::serde_diags::{
    freestanding_hint, is_freestanding_forbidden, module_short_name, reactive_derived_unit,
    reactive_lambda_arity, reactive_not_lambda, unknown_core_item, wrong_core_arity,
};

/// The Core row owns the lookup key for plain calls. Keep the fallback for
/// polymorphic/closure forms until their projection rows land, but do not let
/// a plain row silently acquire a second effect lookup here.
fn core_effect_for_call(module: &str, name: &str) -> Option<crate::Sema::Effects::Effect> {
    match Syntax::core_call(module, name) {
        Some(row) => row.effect(),
        None => core_effect(module, name),
    }
}

fn vault_key_arg(ty: &Type) -> Option<Type> {
    match ty {
        Type::Apply { name, args }
            if matches!(name.as_str(), "KeyRef" | "MutationPlan" | "VaultWrite" | "Rotation" | "WrappedImportPlan") =>
        {
            args.first().cloned()
        }
        Type::Named(name) if matches!(name.as_str(), "SigningKey" | "X25519SecretKey") => Some(ty.clone()),
        Type::Tagged { inner, .. }
            if matches!(inner.as_ref(), Type::Named(name) if matches!(name.as_str(), "SigningKey" | "X25519SecretKey")) => Some(ty.clone()),
        _ => None,
    }
}

fn literal_list_len(expr: &crate::AST::Expr) -> Option<usize> {
    match expr {
        crate::AST::Expr::ListLit(items, _)
            if !items.iter().any(|item| matches!(item, crate::AST::Expr::Spread(..))) =>
        {
            Some(items.len())
        }
        crate::AST::Expr::Paren(inner, _) => literal_list_len(inner),
        _ => None,
    }
}

fn collect_task_handles(expr: &Expr, out: &mut std::collections::HashSet<String>) {
    match expr {
        Expr::Ident(name, _) => {
            out.insert(name.clone());
        }
        Expr::ListLit(items, _) => {
            for item in items {
                collect_task_handles(item, out);
            }
        }
        _ => {}
    }
}

fn known_list_len(checker: &Checker<'_>, expr: &crate::AST::Expr) -> Option<usize> {
    match expr {
        crate::AST::Expr::Ident(name, _) => match &checker.lookup(name)?.ty {
            Type::FixedList { len, .. } => usize::try_from(*len).ok(),
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
            format!("`{name}` expects exactly one type argument, got {}", type_args.len()),
            "a typed decode call needs one target type".to_string(),
            format!("write `{name}<Target>(...)` with one target type"),
            Some(span),
        ));
        return None;
    }
    Some(type_args[0].clone())
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
    matches!(ty, Type::Named(type_name) if type_name == "Tensor")
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

fn compute_function_names(checker: &Checker<'_>, expr: &Expr) -> Option<Vec<String>> {
    match expr {
        Expr::Paren(inner, _) => compute_function_names(checker, inner),
        Expr::Ident(name, _) => checker
            .funcs
            .get(name)
            .map(|sig| sig.param_info.iter().map(|(name, _)| name.clone()).collect())
            .or_else(|| {
                let info = checker.lookup(name)?;
                let Type::Fn {
                    param_contract: Some(contract),
                    ..
                } = &info.ty
                else {
                    return None;
                };
                Some(contract.iter().map(|(name, _)| name.clone()).collect())
            }),
        Expr::Lambda(lambda) => Some(lambda.params.iter().map(|param| param.name.clone()).collect()),
        Expr::MethodCall { method, args, .. }
            if matches!(method.as_str(), "gradient" | "value_and_gradient" | "vjp" | "jvp") =>
        {
            args.first()
                .and_then(|arg| compute_function_names(checker, &arg.expr))
        }
        Expr::Call(call)
            if matches!(call.name.as_str(), "gradient" | "value_and_gradient" | "vjp" | "jvp") =>
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
            if matches!(method.as_str(), "gradient" | "value_and_gradient" | "vjp" | "jvp") =>
        {
            args.first()
                .and_then(|arg| compute_function_identity(checker, &arg.expr))
        }
        Expr::Call(call)
            if matches!(call.name.as_str(), "gradient" | "value_and_gradient" | "vjp" | "jvp") =>
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
    if !matches!(module, "jet.crypto" | "core.crypto")
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
    if matches!(module, "jet.crypto" | "core.crypto") && name == "password_hash_with_salt" {
        return Some(Diagnostic::crypto_misuse_fact(
            "`password_hash_with_salt` would use caller-controlled salt bytes, which makes a deterministic entropy seam reachable from a release build".to_string(),
            "use `crypto.password_hash` so Jet generates the salt, or move a fixed vector to `expert.argon2id` inside `#Unsafe`".to_string(),
            args.get(1)?.expr.span(),
            CryptoMisuseReason::DeterministicEntropy,
            "password_hash_with_salt",
        ));
    }
    if matches!(module, "jet.crypto" | "core.crypto" | "core.crypto.expert")
        && name == "hkdf_sha256"
    {
        let length = args.get(3)?;
        let actual = literal_int(&length.expr)?;
        if !(0..=8160).contains(&actual) {
            return Some(Diagnostic::crypto_misuse(
                format!("HKDF-SHA256 output length is {actual} bytes; this operation requires 0..8160"),
                "pass an output length from 0 through 8160 bytes".to_string(),
                length.expr.span(),
                CryptoMisuseReason::OutputLength,
                "hkdf_sha256",
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
                    format!("Argon2id output length is {actual} bytes; this operation requires 16..64"),
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
                if expected == 64 { "exactly 64" } else { "exactly 32" },
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
    if actual == expected { return None; }
    let unit = if actual == 1 { "byte" } else { "bytes" };
    Some(Diagnostic::crypto_misuse(
        format!("{operation} nonce has {actual} {unit}; this operation requires exactly {expected}"),
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
) -> Option<(Vec<(AccessConvention, Type)>, Option<Type>)> {
    let (params, ret) = match Syntax::core_call(module, name) {
        Some(row) => core_fixed_sig_for_row(row)?,
        None => core_fixed_sig(module, name)?,
    };
    if matches!(module, "jet.crypto" | "core.crypto" | "core.crypto.expert") {
        Some((
            params
                .into_iter()
                .map(|(access, ty)| {
                    (access, crate::Sema::Diagnostics::core_crypto_nominal(ty))
                })
                .collect(),
            ret.map(crate::Sema::Diagnostics::core_crypto_nominal),
        ))
    } else {
        Some((params, ret))
    }
}

fn core_compiler_return(name: &str) -> Type {
    let value = match name {
        "lex" => "CompilerLexed",
        "parse" => "CompilerSyntaxTree",
        "check" => "CompilerChecked",
        "source_map" => "CompilerSourceMap",
        _ => "CompilerError",
    };
    Type::Result {
        ok: Box::new(Type::Named(value.to_string())),
        err: Box::new(Type::Named("CompilerError".to_string())),
    }
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
            let output = ret.map(|ret| *ret).unwrap_or_else(unit_ty);
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
            if params
                .iter()
                .any(|param| !matches!(param, Type::Named(type_name) if type_name == "Tensor"))
            {
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
                if arg
                    .label
                    .as_ref()
                    .is_some_and(|(label, _)| label == "wrt")
                {
                    wrt_expr = Some(arg.expr.clone());
                } else {
                    value_indexes.push(index);
                }
            }
            for index in &value_indexes {
                let value_ty = self.infer(&mut args[*index].expr);
                if !matches!(value_ty, Some(Type::Named(type_name)) if type_name == "Tensor") {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("argument {} to `compute.{name}` should be `Tensor`", index),
                        "autodiff transforms record Tensor values, not scalar policy arguments".to_string(),
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
                        "the differentiation target is selected by the function parameter name".to_string(),
                        "write `wrt: [parameter]`".to_string(),
                        Some(wrt.span()),
                    ));
                }
                self.infer(wrt);
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
                self.diags.push(wrong_core_arity(name, expected_values + 1, args.len(), span));
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
            let mut selected = if let Some(wrt) = wrt_expr.as_ref().and_then(|expr| compute_wrt_names(expr)) {
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
            let gradient_value_type = compute_gradient_value_type(&output)
                .unwrap_or_else(compute_tensor_type);
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
                params.iter().cloned().chain(params.iter().cloned()).collect()
            } else {
                params.clone()
            };
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
                param_contract: Some(
                    names
                        .iter()
                        .map(|name| (name.clone(), ParamZone::Either))
                        .collect(),
                ),
                call_metadata: None,
                return_view_provenance: None,
            })
        }

        pub(crate) fn infer_core_call(
            &mut self,
            module: &str,
            name: &str,
            alias_span: Span,
            span: Span,
            type_args: &[Type],
            args: &mut Vec<crate::AST::CallArg>,
        ) -> Option<Type> {
            // D-FRONTENDAPI1=A: the compiler surface is a read-only
            // compile-time value API. It is intentionally handled before the
            // ordinary Core effect/fixed-signature tables so it cannot become
            // a runtime or ambient fallback by accident.
            if module == "core.compiler" {
                if !matches!(name, "lex" | "parse" | "check" | "source_map") {
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
                if args.len() != 1 {
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
            if let Some(row) = Syntax::core_call(module, name) {
                if matches!(row.fallibility, Syntax::CoreCallFallibility::Sema)
                    && !row.accepts_arity(args.len())
                {
                    self.diags
                        .push(wrong_core_arity(name, row.arity(), args.len(), span));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
            }
            if let Some(e) = core_effect_for_call(module, name) {
                // D-EFFTREE1: Core calls (this module-call path) stay tagged with
                // a bare root — real stdlib call sites are unchanged (no migration
                // break: existing diagnostics naming `FS`/`DB`/… keep their exact
                // wording). Leaf precision (`FS.Read`, …) is otherwise a
                // user-declared-contract concept (a function's own `#(…)` bound,
                // D-PROP1-seeded into its `direct` set) — see Registration.rs /
                // Bundle.rs. The one exception is D-EFFDBREAD1=A: `core.db`'s own
                // closed connection-method table infers `DB.Read`/`DB.Write` leaves
                // (in `check_db_connection_method`, the method-call path — those
                // methods never reach this module-call `core_effect`).
                self.record_effect(e.name(), span);
                // D-TXN2: an irreversible effect (Net/FS/Exec — a network/file/
                // subprocess effect) can't be rolled back, so it is rejected when it
                // occurs directly inside a `#Transact { … }` block (E0746). The fix
                // is to move it after the block, or register it via
                // `name.on_commit(() => { … })` so it runs only on a clean commit.
                if self.txn_depth > 0 && is_irreversible_effect(e) {
                    let api = format!("{}.{}", module_short_name(module), name);
                    self.diags.push(e0746(&api, e, span));
                }
            }
            // E2-M15 / E3301: reject OS-dependent APIs in freestanding builds.
            if self.freestanding && is_freestanding_forbidden(module) {
                let api = format!("{}.{}", module_short_name(module), name);
                let hint = freestanding_hint(module);
                self.diags.push(e3301(&api, hint, span));
                // Still infer args to avoid cascading errors.
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            }
            // #1465 / I1: POSIX process/session control is expert-tier.
            if module == "core.os"
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
                self.diags.push(super::alloc_ptrs::e3101(
                    &format!("os.{name}"),
                    span,
                ));
            }
            // E2-M16 / E3403: a `pure fn` cannot reach a non-deterministic std call
            // (time/random). `jet eval --pure` requires every fn to be `pure`, so
            // this covers the --pure path too.
            // D-DET1: `assume_deterministic { … }` (det_suppress > 0) suspends the
            // determinism rejection — the expert escape hatch.
            if self.in_pure && self.det_suppress == 0 && is_nondeterministic_core(module, name) {
                let api = format!("{}.{}", module_short_name(module), name);
                self.diags.push(e3403(&api, Some(span)));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                // Return the declared type so the call site doesn't cascade.
                return resolved_core_fixed_sig(module, name).and_then(|(_, ret)| ret);
            }
            // D-STDIN1=A / E3401: `pure fn` cannot read from stdin.
            if self.in_pure && self.det_suppress == 0 && is_impure_core(module, name) {
                let api = format!("{}.{}", module_short_name(module), name);
                self.diags
                    .push(e3401(&self.fn_name.clone(), &api, &[], span));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return resolved_core_fixed_sig(module, name).and_then(|(_, ret)| ret);
            }
            if module == "core.encoding.cbor"
                && matches!(name, "encode" | "decode")
                && (name != "decode" || type_args.is_empty())
            {
                if let Some(dep) = super::super::Edition::check_core_deprecation(module, name) {
                    use super::super::Edition::{deprecation_phase, DeprecationPhase};
                    match deprecation_phase(&dep) {
                        DeprecationPhase::Removed => {
                            self.diags
                                .push(super::super::Edition::e2002(&dep, Some(span)));
                        }
                        DeprecationPhase::Deprecated => {
                            self.diags
                                .push(super::super::Edition::l2001(&dep, Some(span)));
                        }
                        DeprecationPhase::Active => {}
                    }
                }
            }
            // D-EFF1: `#Pure` is the empty effect set, so any effectful Core call —
            // `FS`/`Net`/`Env`/`Exec`/`DB`/`Log`/`IO` — is impure inside a `#Pure fn`.
            // (Time/Rand return early above via E3403; stdin via the E3401 check
            // above, so this catches the remaining effect-carrying Core modules.)
            if self.in_pure
                && self.det_suppress == 0
                && core_effect_for_call(module, name).is_some()
            {
                let api = format!("{}.{}", module_short_name(module), name);
                self.diags
                    .push(e3401(&self.fn_name.clone(), &api, &[], span));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return resolved_core_fixed_sig(module, name).and_then(|(_, ret)| ret);
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
                    &[(1, "key"), (2, "audience"), (3, "issuer"), (4, "clock_skew")][..args.len().min(5).saturating_sub(1)]
                } else {
                    &[(1, "key"), (2, "audience"), (3, "issuer"), (4, "clock_skew"), (5, "footer"), (6, "implicit")][..args.len().min(7).saturating_sub(1)]
                };
                super::net_text_time::require_exact_labels(
                    &format!("auth.{name}"), args, required, span, &mut self.diags,
                );
            } else if module == "core.tls" && name == "client" {
                let required = match args.len() {
                    3 => &[(1, "server_name"), (2, "deadline")][..],
                    4 => &[(1, "server_name"), (2, "config"), (3, "deadline")][..],
                    _ => &[],
                };
                super::net_text_time::require_exact_labels(
                    "tls.client", args, required, span, &mut self.diags,
                );
            } else if module == "core.net" && name == "unix_connect" && args.len() == 2 {
                super::net_text_time::require_exact_labels(
                    "net.unix_connect", args, &[(1, "deadline")], span, &mut self.diags,
                );
            }
            if matches!(module, "app" | "core.web") && name == "sync" && args.len() == 2 {
                super::net_text_time::require_exact_labels(
                    "app.sync", args, &[(1, "over")], span, &mut self.diags,
                );
            }
            if module == "core.services" && name == "runtime" && args.len() == 2 {
                super::net_text_time::require_exact_labels(
                    "services.runtime", args, &[(1, "retention")], span, &mut self.diags,
                );
            }
            if module == "core.compute"
                && matches!(name, "gradient" | "value_and_gradient" | "vjp" | "jvp")
            {
                return self.infer_compute_transform(name, span, args);
            }
            let sig = if module == "core.auth" && name == "verify_jwt" && (3..=5).contains(&args.len()) {
                let mut params = vec![
                    (AccessConvention::Read, Type::String),
                    (AccessConvention::Read, Type::List(Box::new(u8_ty()))),
                    (AccessConvention::Read, Type::String),
                ];
                if args.len() >= 4 { params.push((AccessConvention::Read, Type::String)); }
                if args.len() >= 5 { params.push((AccessConvention::Read, Type::Named("Duration".to_string()))); }
                Some((params, Some(result_ty(Type::Named("Claims".to_string()), Type::Named("AuthError".to_string())))))
            } else if module == "core.auth" && name == "verify_paseto" && (3..=7).contains(&args.len()) {
                let mut params = vec![
                    (AccessConvention::Read, Type::String),
                    (AccessConvention::Read, Type::List(Box::new(u8_ty()))),
                    (AccessConvention::Read, Type::String),
                ];
                if args.len() >= 4 { params.push((AccessConvention::Read, Type::String)); }
                if args.len() >= 5 { params.push((AccessConvention::Read, Type::Named("Duration".to_string()))); }
                if args.len() >= 6 { params.push((AccessConvention::Read, Type::List(Box::new(u8_ty())))); }
                if args.len() >= 7 { params.push((AccessConvention::Read, Type::List(Box::new(u8_ty())))); }
                Some((params, Some(result_ty(Type::Named("Claims".to_string()), Type::Named("AuthError".to_string())))))
            } else if module == "core.tls" && name == "client" && args.len() == 4 {
                Some((
                    vec![
                        (AccessConvention::Move, Type::Named("TcpStream".to_string())),
                        (AccessConvention::Read, Type::String),
                        (AccessConvention::Read, Type::Named("TLSClientConfig".to_string())),
                        (AccessConvention::Read, Type::Named("Duration".to_string())),
                    ],
                    Some(result_ty(
                        Type::Named("TLSStream".to_string()),
                        Type::Named("NetError".to_string()),
                    )),
                ))
            } else if module == "core.tls" && name == "client" && args.len() == 3 {
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
                resolved_core_fixed_sig(module, name)
            };
            // D-APILABEL1=A: a Core function that publishes a call contract
            // binds through the same binder as user code, so a caller can name
            // the one policy it changes and skip the rest. Filling the skipped
            // defaults here is also what stops each engine spelling its own
            // fallback: every tier now receives the same argument.
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
                if crate::Sema::CallBinder::bind_call_args(
                    name,
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
                    return sig.and_then(|(_, ret)| ret);
                }
                self.register_binder_refs(args);
            }
            match (module, name) {
                ("core.vault", "current" | "versions" | "load" | "status"
                    | "prepare_generate" | "prepare_store" | "prepare_rotate" | "prepare_retire" | "prepare_revoke"
                    | "authorize_write" | "commit_generate" | "commit_store" | "commit_rotate" | "commit_retire" | "commit_revoke"
                    | "export_to_recipients" | "export_to_passphrase" | "prepare_import_wrapped"
                    | "authorize_wrapped_import" | "commit_import_wrapped") => {
                    let inferred_from = match name {
                        "prepare_store" => 1,
                        "export_to_recipients" | "export_to_passphrase" => 0,
                        "load" | "status" | "prepare_retire" | "prepare_revoke" | "authorize_write"
                        | "commit_generate" | "commit_store" | "commit_rotate" | "commit_retire" | "commit_revoke"
                        | "authorize_wrapped_import" | "commit_import_wrapped" => 0,
                        _ => usize::MAX,
                    };
                    let inferred_key = if type_args.is_empty() && inferred_from < args.len() {
                        self.infer(&mut args[inferred_from].expr).and_then(|ty| vault_key_arg(&ty))
                    } else { None };
                    if type_args.len() > 1 || (type_args.is_empty() && inferred_key.is_none()) {
                        self.diags.push(Diagnostic::error(
                            "E0904",
                            format!("`vault.{name}` needs one vault key type"),
                            "typed vault operations are restricted to SigningKey and X25519SecretKey".to_string(),
                            format!("call it with an explicit type argument: `vault.{name}<crypto.SigningKey>(...)`"),
                            Some(span),
                        ));
                        for arg in args.iter_mut() { self.infer(&mut arg.expr); }
                        return None;
                    }
                    let key_ty = self.resolve_type(type_args.first().cloned().or(inferred_key).unwrap());
                    let key_leaf = match &key_ty {
                        Type::Named(leaf) => Some(leaf.as_str()),
                        Type::Tagged { inner, .. } => match inner.as_ref() { Type::Named(leaf) => Some(leaf.as_str()), _ => None },
                        _ => None,
                    };
                    if !key_leaf.is_some_and(|leaf| matches!(leaf, "SigningKey" | "X25519SecretKey")) {
                        self.diags.push(Diagnostic::error(
                            "E0905",
                            format!("`{}` is not a persistent vault key type", key_ty.show()),
                            "VaultKey is sealed and implemented only by SigningKey and X25519SecretKey".to_string(),
                            "use `crypto.SigningKey` or `crypto.X25519SecretKey`".to_string(),
                            Some(span),
                        ));
                    }
                    let apply = |name: &str| Type::Apply { name: name.to_string(), args: vec![key_ty.clone()] };
                    let (params, ok): (Vec<(AccessConvention, Type)>, Type) = match name {
                        "current" => (vec![(AccessConvention::Read, Type::String)], Type::Option(Box::new(apply("KeyRef")))),
                        "versions" => (vec![(AccessConvention::Read, Type::String)], Type::List(Box::new(apply("KeyRef")))),
                        "load" => (vec![(AccessConvention::Read, apply("KeyRef"))], key_ty.clone()),
                        "status" => (vec![(AccessConvention::Read, apply("KeyRef"))], Type::Named("KeyStatus".into())),
                        "prepare_generate" | "prepare_rotate" => (vec![(AccessConvention::Read, Type::String)], apply("MutationPlan")),
                        "prepare_store" => (vec![(AccessConvention::Read, Type::String), (AccessConvention::Move, key_ty.clone())], apply("MutationPlan")),
                        "prepare_retire" | "prepare_revoke" => (vec![(AccessConvention::Read, apply("KeyRef")), (AccessConvention::Read, Type::String)], apply("MutationPlan")),
                        "authorize_write" => (vec![(AccessConvention::Read, apply("MutationPlan")), (AccessConvention::Read, Type::String)], apply("VaultWrite")),
                        "export_to_recipients" => (vec![(AccessConvention::Read, apply("KeyRef")), (AccessConvention::Read, Type::List(Box::new(Type::Named("X25519PublicKey".into()))))], Type::Named("WrappedVaultKey".into())),
                        "export_to_passphrase" => (vec![(AccessConvention::Read, apply("KeyRef")), (AccessConvention::Read, crate::Sema::Diagnostics::core_crypto_nominal(Type::Named("Secret".into())))], Type::Named("WrappedVaultKey".into())),
                        "prepare_import_wrapped" => (vec![(AccessConvention::Read, Type::String), (AccessConvention::Read, Type::Named("WrappedVaultKey".into())), (AccessConvention::Read, Type::Named("KeyUnlock".into()))], apply("WrappedImportPlan")),
                        "authorize_wrapped_import" => (vec![(AccessConvention::Read, apply("WrappedImportPlan")), (AccessConvention::Read, Type::String)], apply("VaultWrite")),
                        "commit_import_wrapped" => (vec![(AccessConvention::Move, apply("VaultWrite")), (AccessConvention::Move, apply("WrappedImportPlan"))], apply("KeyRef")),
                        "commit_generate" | "commit_store" => (vec![(AccessConvention::Move, apply("VaultWrite")), (AccessConvention::Move, apply("MutationPlan"))], apply("KeyRef")),
                        "commit_rotate" => (vec![(AccessConvention::Move, apply("VaultWrite")), (AccessConvention::Move, apply("MutationPlan"))], apply("Rotation")),
                        _ => (vec![(AccessConvention::Move, apply("VaultWrite")), (AccessConvention::Move, apply("MutationPlan"))], Type::Named("Unit".into())),
                    };
                    if args.len() != params.len() { self.diags.push(wrong_core_arity(name, params.len(), args.len(), span)); }
                    for (i, ((convention, ty), arg)) in params.iter().zip(args.iter_mut()).enumerate() {
                        if *convention == AccessConvention::Move {
                            if arg.convention != AccessConvention::Move { self.diags.push(Diagnostic::error("E0201", format!("argument {} to `{name}` transfers ownership through the move-capability marker `^`", i + 1), "this vault operation consumes its single-use authority value".to_string(), format!("write the move-capability marker `^`: `{}value` for this argument", Syntax::SIGIL_MOVE), Some(arg.span))); }
                            self.expect_core_arg_moving(name, i, ty, arg);
                        } else { self.expect_core_arg(name, i, ty, arg); }
                    }
                    for arg in args.iter_mut().skip(params.len()) { self.infer(&mut arg.expr); }
                    let err = if matches!(name, "export_to_recipients" | "export_to_passphrase" | "prepare_import_wrapped" | "authorize_wrapped_import" | "commit_import_wrapped") {
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
                    if let Some(arg) = args.get_mut(0) { self.expect_core_arg(name, 0, &Type::List(Box::new(u8_ty())), arg); }
                    if let Some(arg) = args.get_mut(1) { self.expect_core_arg(name, 1, &Type::Named("CBOROptions".to_string()), arg); }
                    return Some(result_ty(Type::Named("DataTree".to_string()), Type::Named("CBORError".to_string())));
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
                    if let Some(arg) = args.get_mut(0) { self.expect_core_arg(name, 0, &Type::List(Box::new(u8_ty())), arg); }
                    if let Some(arg) = args.get_mut(1) { self.expect_core_arg(name, 1, &Type::Named("XMLParseOptions".to_string()), arg); }
                    return Some(result_ty(Type::Named("DataTree".to_string()), Type::Named("XMLError".to_string())));
                }
                ("core.encoding.xml", "to_bytes") => {
                    if !(1..=2).contains(&args.len()) {
                        self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                    }
                    if let Some(arg) = args.get_mut(0) { self.expect_core_arg(name, 0, &Type::Named("DataTree".to_string()), arg); }
                    if let Some(arg) = args.get_mut(1) { self.expect_core_arg(name, 1, &Type::Named("XMLRenderOptions".to_string()), arg); }
                    return Some(result_ty(Type::List(Box::new(u8_ty())), Type::Named("XMLError".to_string())));
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
                    if args.len() != 1 { self.diags.push(wrong_core_arity(name, 1, args.len(), span)); }
                    for arg in args.iter_mut() {
                        self.borrow_ctx = true;
                        if let Some(t) = self.infer(&mut arg.expr) { self.check_encodable(&t, arg.expr.span()); }
                    }
                    return Some(result_ty(Type::List(Box::new(u8_ty())), Type::Named("CBORError".to_string())));
                }
                ("core.encoding.cbor", "decode") if !type_args.is_empty() => {
                    if !(1..=2).contains(&args.len()) { self.diags.push(wrong_core_arity(name, 1, args.len(), span)); }
                    if let Some(arg) = args.get_mut(0) { self.expect_core_arg(name, 0, &Type::List(Box::new(u8_ty())), arg); }
                    if let Some(arg) = args.get_mut(1) { self.expect_core_arg(name, 1, &Type::Named("CBOROptions".to_string()), arg); }
                    let Some(t) = exactly_one_type_arg(self, name, type_args, span) else {
                        return None;
                    };
                    self.check_decodable(&t, span);
                    return Some(result_ty(t, decode_error_ty()));
                }
                ("core.encoding.json" | "core.encoding.jsonl" | "core.encoding.csv" | "core.encoding.cbor", "reader" | "writer")
                | ("core.encoding.xml", "reader" | "writer") => {
                    let max = if (module == "core.encoding.json" && name == "writer")
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
                                format!("write `xml.{name}(^file)`, `xml.{name}(^file, limits)`, or `xml.{name}(^file, limits, options)` with the move-capability marker `^`")
                            } else if name == "reader" { format!("write `{}.reader(^file)` or `{}.reader(^file, limits)` with the move-capability marker `^`", module_short_name(module), module_short_name(module)) } else if module == "core.encoding.json" { "write `json.writer(^file)`, `json.writer(^file, limits)`, or `json.writer(^file, limits, canonical)` with the move-capability marker `^`".to_string() } else { format!("write `{}.writer(^file)` or `{}.writer(^file, limits)` with the move-capability marker `^`", module_short_name(module), module_short_name(module)) },
                            Some(span),
                        ));
                    }
                    let Some((params, ret)) = &sig else { unreachable!() };
                    for (i, ((conv, param_ty), arg)) in params.iter().zip(args.iter_mut()).enumerate() {
                        if *conv == AccessConvention::Move {
                            if arg.convention != AccessConvention::Move {
                                self.diags.push(Diagnostic::error(
                                    "E0201",
                                    format!("argument {} to `{}` transfers ownership through the move-capability marker `^`", i + 1, name),
                                    "this standard library constructor retains the consumed handle".to_string(),
                                    format!("write the move-capability marker `^`: `{}value` for this argument", Syntax::SIGIL_MOVE),
                                    Some(arg.span),
                                ));
                            }
                            self.expect_core_arg_moving(name, i, param_ty, arg);
                        } else {
                            self.expect_core_arg(name, i, param_ty, arg);
                        }
                    }
                    for arg in args.iter_mut().skip(params.len()) { self.infer(&mut arg.expr); }
                    return ret.clone();
                }
                ("core.game", "run") => {
                    if args.len() != 3 {
                        self.diags.push(Diagnostic::error(
                            "E0104",
                            format!("`game.run` expects 1 to 3 arguments, got {}", args.len()),
                            "`game.run` accepts a scene plus optional replay and backend handles"
                                .to_string(),
                            "write `game.run(scene)`, `game.run(scene, replay: replay)`, or `game.run(scene, replay: replay, backend: backend)`".to_string(),
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
                    return Some(Type::String);
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
                            Type::List(elem)
                                if matches!(elem.as_ref(), Type::List(cell) if matches!(cell.as_ref(), Type::String)) =>
                            {
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
                (
                    "core.encoding.json" | "core.encoding.csv" | "core.encoding.toml"
                    | "core.encoding.yaml",
                    "decode",
                ) if !type_args.is_empty() => {
                    if args.len() != 1 {
                        self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                    }
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
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
                // D-MIGRATE3=A: `decode_traced<T>` — the one extra opt-in method beside
                // `decode` on every codec that shares the decode machinery. Same target
                // typing as `decode`, wrapped in `DecodeResult<T>` (`DecodeResult<[T]>`
                // for CSV) so the caller can ask `.migration.migrated` without `decode`
                // itself changing shape or cost (I8).
                (
                    "core.encoding.json" | "core.encoding.csv" | "core.encoding.toml"
                    | "core.encoding.yaml",
                    "decode_traced",
                ) if !type_args.is_empty() => {
                    if args.len() != 1 {
                        self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                    }
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
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
                    let decode_result = Type::Apply {
                        name: "DecodeResult".to_string(),
                        args: vec![inner],
                    };
                    return Some(result_ty(decode_result, decode_error_ty()));
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
                    let countable = match &ty {
                        Type::List(_) => true,
                        Type::Apply { name, .. } => {
                            matches!(name.as_str(), "Table" | "Series" | "LazyFrame")
                        }
                        _ => false,
                    };
                    if !countable {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!(
                                "`data.count` needs a typed table or series, not {}",
                                ty.show()
                            ),
                            "core.data counts rows from a list-backed table or series".to_string(),
                            "pass a `[T]` value, such as `data.csv<Row>(text)?`".to_string(),
                            Some(arg.expr.span()),
                        ));
                    }
                    return Some(Type::Int);
                }
                ("core.data", "table" | "series") => {
                    if args.len() != 1 {
                        self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                    }
                    let Some(rows_arg) = args.get_mut(0) else {
                        return Some(Type::Apply {
                            name: if name == "table" { "Table" } else { "Series" }.to_string(),
                            args: vec![Type::Int],
                        });
                    };
                    let ty = self.infer(&mut rows_arg.expr);
                    let elem = match ty {
                        Some(Type::List(inner)) => *inner,
                        Some(other) => {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`data.{}` needs a list-backed value, not {}", name, other.show()),
                                "core.data tables and series are built from typed lists".to_string(),
                                "pass `[Row]` to `data.table` or `[T]` to `data.series`".to_string(),
                                Some(rows_arg.expr.span()),
                            ));
                            Type::Int
                        }
                        None => Type::Int,
                    };
                    return Some(Type::Apply {
                        name: if name == "table" { "Table" } else { "Series" }.to_string(),
                        args: vec![elem],
                    });
                }
                ("core.data", "rows" | "values") => {
                    if args.len() != 1 {
                        self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                    }
                    let Some(arg) = args.get_mut(0) else {
                        return Some(Type::List(Box::new(Type::Int)));
                    };
                    let want = if name == "rows" { "Table" } else { "Series" };
                    let ty = self.infer(&mut arg.expr);
                    let elem = match ty {
                        Some(Type::Apply { name: head, args }) if head == want && args.len() == 1 => {
                            args[0].clone()
                        }
                        Some(other) => {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`data.{}` needs a `{}` value, not {}", name, want, other.show()),
                                "core.data unwraps typed table/series containers through explicit helpers".to_string(),
                                format!("pass a `{want}<T>` value"),
                                Some(arg.expr.span()),
                            ));
                            Type::Int
                        }
                        None => Type::Int,
                    };
                    return Some(Type::List(Box::new(elem)));
                }
                ("core.data", "schema") => {
                    if args.len() != 1 {
                        self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                    }
                    let Some(arg) = args.get_mut(0) else {
                        return Some(Type::List(Box::new(Type::Named("DataColumn".to_string()))));
                    };
                    let ty = self.infer(&mut arg.expr)?;
                    let ok = match &ty {
                        Type::List(_) => true,
                        Type::Apply { name, args } => {
                            matches!(name.as_str(), "Table" | "Series" | "LazyFrame")
                                && args.len() == 1
                        }
                        _ => false,
                    };
                    if !ok {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!(
                                "`data.schema` needs a typed table or series, not {}",
                                ty.show()
                            ),
                            "core.data schema reads column names and types from the row model".to_string(),
                            "pass a `Table<T>`, `Series<T>`, `LazyFrame<T>`, or `[T]` value".to_string(),
                            Some(arg.expr.span()),
                        ));
                    }
                    return Some(Type::List(Box::new(Type::Named("DataColumn".to_string()))));
                }
                ("core.data", "missing_count") => {
                    if args.len() != 1 {
                        self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                    }
                    let Some(arg) = args.get_mut(0) else {
                        return Some(Type::Int);
                    };
                    let ty = self.infer(&mut arg.expr);
                    if !matches!(&ty, Some(Type::Apply { name: head, args }) if head == "Series" && args.len() == 1) {
                        let shown = ty.map(|t| t.show()).unwrap_or_else(|| "<unknown>".to_string());
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`data.missing_count` needs a `Series<T?>`, not {}", shown),
                            "missing values are represented by Jet optionals in a typed series".to_string(),
                            "build a series from `[T?]` values with `data.series(values)`".to_string(),
                            Some(arg.expr.span()),
                        ));
                    }
                    return Some(Type::Int);
                }
                ("core.data", "lazy") => {
                    if args.len() != 1 {
                        self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                    }
                    let Some(arg) = args.get_mut(0) else {
                        return Some(Type::Apply {
                            name: "LazyFrame".to_string(),
                            args: vec![Type::Int],
                        });
                    };
                    let ty = self.infer(&mut arg.expr);
                    let elem = match ty {
                        Some(Type::Apply { name: head, args }) if head == "Table" && args.len() == 1 => {
                            args[0].clone()
                        }
                        Some(other) => {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`data.lazy` needs a `Table<T>`, not {}", other.show()),
                                "lazy plans start from the same typed table model as eager helpers".to_string(),
                                "wrap rows with `data.table(rows)` first".to_string(),
                                Some(arg.expr.span()),
                            ));
                            Type::Int
                        }
                        None => Type::Int,
                    };
                    return Some(Type::Apply {
                        name: "LazyFrame".to_string(),
                        args: vec![elem],
                    });
                }
                ("core.data", "collect" | "plan") => {
                    if args.len() != 1 {
                        self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                    }
                    let Some(arg) = args.get_mut(0) else {
                        return Some(if name == "collect" {
                            Type::Apply {
                                name: "Table".to_string(),
                                args: vec![Type::Int],
                            }
                        } else {
                            Type::List(Box::new(Type::String))
                        });
                    };
                    let ty = self.infer(&mut arg.expr);
                    let elem = match ty {
                        Some(Type::Apply { name: head, args }) if head == "LazyFrame" && args.len() == 1 => {
                            args[0].clone()
                        }
                        Some(other) => {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`data.{}` needs a `LazyFrame<T>`, not {}", name, other.show()),
                                "lazy plan inspection and collection operate on core.data lazy frames".to_string(),
                                "call `data.lazy(table)` first".to_string(),
                                Some(arg.expr.span()),
                            ));
                            Type::Int
                        }
                        None => Type::Int,
                    };
                    return Some(if name == "collect" {
                        let table = Type::Apply {
                            name: "Table".to_string(),
                            args: vec![elem],
                        };
                        if super::super::Edition::edition_at_least("2027") {
                            result_ty(table, Type::Named("DataError".to_string()))
                        } else {
                            table
                        }
                    } else {
                        Type::List(Box::new(Type::String))
                    });
                }
                ("core.data", "lazy_filter" | "lazy_sort_by") => {
                    if args.len() != 2 {
                        self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                    }
                    let Some(frame_arg) = args.get_mut(0) else {
                        return Some(Type::Apply {
                            name: "LazyFrame".to_string(),
                            args: vec![Type::Int],
                        });
                    };
                    let frame_ty = self.infer(&mut frame_arg.expr);
                    let row_ty = match frame_ty {
                        Some(Type::Apply { name: head, args }) if head == "LazyFrame" && args.len() == 1 => {
                            args[0].clone()
                        }
                        Some(other) => {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`data.{}` needs a `LazyFrame<T>`, not {}", name, other.show()),
                                "lazy table operations keep a typed row model through the plan".to_string(),
                                "call `data.lazy(table)` first".to_string(),
                                Some(frame_arg.expr.span()),
                            ));
                            Type::Int
                        }
                        None => Type::Int,
                    };
                    if let Some(fn_arg) = args.get_mut(1) {
                        let ret = if name == "lazy_filter" {
                            Type::Bool
                        } else {
                            Type::String
                        };
                        let fn_ty = Type::Fn {
                            params: vec![row_ty.clone()],
                            ret: Some(Box::new(ret)),
                            effect_bound: None, return_view_provenance: None,
                            param_contract: None,
                call_metadata: None,
                        };
                        self.expect_core_arg(name, 1, &fn_ty, fn_arg);
                    }
                    return Some(Type::Apply {
                        name: "LazyFrame".to_string(),
                        args: vec![row_ty],
                    });
                }
                ("core.data", "filter" | "sort_by") => {
                    if args.len() != 2 {
                        self.diags.push(wrong_core_arity(name, 2, args.len(), span));
                    }
                    let Some(rows_arg) = args.get_mut(0) else {
                        return Some(Type::List(Box::new(Type::Int)));
                    };
                    let rows_ty = self.infer(&mut rows_arg.expr);
                    let row_ty = match rows_ty {
                        Some(Type::List(inner)) => *inner,
                        Some(other) => {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`data.{}` needs a typed table, not {}", name, other.show()),
                                "core.data pipelines rows from a list-backed typed table".to_string(),
                                "pass a `[Row]` value, such as `data.csv<Row>(text)?`".to_string(),
                                Some(rows_arg.expr.span()),
                            ));
                            Type::Int
                        }
                        None => Type::Int,
                    };
                    if let Some(fn_arg) = args.get_mut(1) {
                        let ret = if name == "filter" {
                            Type::Bool
                        } else {
                            Type::String
                        };
                        let fn_ty = Type::Fn {
                            params: vec![row_ty.clone()],
                            ret: Some(Box::new(ret)),
                            effect_bound: None, return_view_provenance: None,
                            param_contract: None,
                call_metadata: None,
                        };
                        self.expect_core_arg(name, 1, &fn_ty, fn_arg);
                    }
                    return Some(if name == "sort_by" && super::super::Edition::edition_at_least("2027")
                    {
                        result_ty(
                            Type::List(Box::new(row_ty)),
                            Type::Named("DataError".to_string()),
                        )
                    } else {
                        Type::List(Box::new(row_ty))
                    });
                }
                ("core.data", "group_count" | "group_sum" | "group_mean") => {
                    let want = if name == "group_count" { 2 } else { 3 };
                    if args.len() != want {
                        self.diags
                            .push(wrong_core_arity(name, want, args.len(), span));
                    }
                    let Some(rows_arg) = args.get_mut(0) else {
                        return Some(Type::List(Box::new(Type::Named("DataGroup".to_string()))));
                    };
                    let rows_ty = self.infer(&mut rows_arg.expr);
                    let row_ty = match rows_ty {
                        Some(Type::List(inner)) => *inner,
                        Some(Type::Apply { name: ref an, args: ref ta })
                            if an == "DataStream" && ta.len() == 1 =>
                        {
                            ta[0].clone()
                        }
                        Some(other) => {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`data.{}` needs a typed table, not {}", name, other.show()),
                                "core.data groups rows from a list-backed typed table".to_string(),
                                "pass a `[Row]` value, such as `data.csv<Row>(text)?`".to_string(),
                                Some(rows_arg.expr.span()),
                            ));
                            Type::Int
                        }
                        None => Type::Int,
                    };
                    if let Some(key_arg) = args.get_mut(1) {
                        let key_fn = Type::Fn {
                            params: vec![row_ty.clone()],
                            ret: Some(Box::new(Type::String)),
                            effect_bound: None, return_view_provenance: None,
                            param_contract: None,
                call_metadata: None,
                        };
                        self.expect_core_arg(name, 1, &key_fn, key_arg);
                    }
                    if name != "group_count" {
                        if let Some(value_arg) = args.get_mut(2) {
                            let value_fn = Type::Fn {
                                params: vec![row_ty],
                                ret: Some(Box::new(Type::Float)),
                                effect_bound: None, return_view_provenance: None,
                                param_contract: None,
                call_metadata: None,
                            };
                            self.expect_core_arg(name, 2, &value_fn, value_arg);
                        }
                    }
                    let groups = Type::List(Box::new(Type::Named("DataGroup".to_string())));
                    return Some(if super::super::Edition::edition_at_least("2027") {
                        result_ty(groups, Type::Named("DataError".to_string()))
                    } else {
                        groups
                    });
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
                                    format!("`data.{}` needs a typed left table, not {}", name, other.show()),
                                    "core.data joins rows from list-backed typed tables".to_string(),
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
                                    format!("`data.{}` needs a typed right table, not {}", name, other.show()),
                                    "core.data joins rows from list-backed typed tables".to_string(),
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
                            effect_bound: None, return_view_provenance: None,
                            param_contract: None,
                call_metadata: None,
                        };
                        self.expect_core_arg(name, 2, &key_fn, left_key);
                    }
                    if let Some(right_key) = args.get_mut(3) {
                        let key_fn = Type::Fn {
                            params: vec![right_row.clone()],
                            ret: Some(Box::new(Type::String)),
                            effect_bound: None, return_view_provenance: None,
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
                    return Some(if super::super::Edition::edition_at_least("2027") {
                        result_ty(joined, Type::Named("DataError".to_string()))
                    } else {
                        joined
                    });
                }
                ("core.data", "pivot_sum") => {
                    if args.len() != 4 {
                        self.diags.push(wrong_core_arity(name, 4, args.len(), span));
                    }
                    let Some(rows_arg) = args.get_mut(0) else {
                        return Some(Type::List(Box::new(Type::Named("DataGroup".to_string()))));
                    };
                    let rows_ty = self.infer(&mut rows_arg.expr);
                    let row_ty = match rows_ty {
                        Some(Type::List(inner)) => *inner,
                        Some(other) => {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`data.{}` needs a typed table, not {}", name, other.show()),
                                "core.data pivots rows from a list-backed typed table".to_string(),
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
                                effect_bound: None, return_view_provenance: None,
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
                            effect_bound: None, return_view_provenance: None,
                            param_contract: None,
                call_metadata: None,
                        };
                        self.expect_core_arg(name, 3, &value_fn, value_arg);
                    }
                    let cell = if super::super::Edition::edition_at_least("2027") {
                        Type::Named("DataPivotCell".to_string())
                    } else {
                        Type::Named("DataGroup".to_string())
                    };
                    let cells = Type::List(Box::new(cell));
                    return Some(if super::super::Edition::edition_at_least("2027") {
                        result_ty(cells, Type::Named("DataError".to_string()))
                    } else {
                        cells
                    });
                }
                ("core.mem", "volatile_read") => {
                    if Syntax::core_mem_requires_audit(Syntax::MEM_VOLATILE_READ)
                        && !self.in_unsafe
                    {
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
                    if Syntax::core_mem_requires_audit(Syntax::MEM_VOLATILE_WRITE)
                        && !self.in_unsafe
                    {
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
                    let write_place = matches!(
                        &arg.expr,
                        Expr::Place(_, crate::AST::PlaceAccess::Write, _)
                    ) || arg.convention == AccessConvention::Write;
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
                                "`{}` needs a write window made with the write-capability marker `&` into the place being pinned",
                                Syntax::MEM_PIN
                            ),
                            "a pin promises one storage location will not move, so it has to name that location with the write-capability marker `&` instead of a copied value"
                                .to_string(),
                            format!(
                                "write `mem.{}({}place)` with the write-capability marker `&`",
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
                ("core.io", "print" | "println") => {
                    // D-PRELUDEX1=A: qualified twin of ambient `print` for `#NoPrelude` files.
                    // D-VERDICT-1321-1: variadic — each argument prints on its own line.
                    // #1480: `println` is the peer spelling; same mechanism as `print`.
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
                                    "`io.print` prints the same values as ambient `print`"
                                        .to_string(),
                                    "print one of its fields, or make it a printable type".to_string(),
                                    Some(arg.expr.span()),
                                ));
                            }
                        }
                    }
                    return None;
                }
                ("core.io", "progress") => {
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
                                "use `io.progress(items, description, format)` for an iterable".to_string(),
                                Some(args[1].expr.span()),
                            ));
                        }
                        return Some(result_ty(unit_ty(), io_error_ty()));
                    }
                    let elem = match source {
                        Type::List(inner) => *inner,
                        Type::FixedList { elem, .. } => *elem,
                        Type::Apply { name, mut args } if name == Syntax::TYPE_ITER && args.len() == 1 => {
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
                ("core.io", "eprint") => {
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
                    return None;
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
                            } else { self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("{} can't be reflected yet", ty.show()),
                                "`reflect.of` inspects the same values `\"{x}\"` interpolation can show"
                                    .to_string(),
                                "implement `Display` for its type, or pass one of its fields instead"
                                    .to_string(),
                                Some(arg.expr.span()),
                            )); }
                        }
                    }
                    return Some(Type::Named("Value".to_string()));
                }
                ("core.io", "input") => {
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
                        if !matches!(ty, Type::Int | Type::IntN { .. }) {
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
                    "sqrt" | "floor" | "ceil" | "sin" | "cos" | "tan" | "asin" | "acos"
                    | "atan" | "sinh" | "cosh" | "tanh" | "exp" | "ln" | "log2" | "log10"
                    | "acosh" | "asinh" | "atanh" | "cbrt" | "exp2" | "exp_m1"
                    | "ln_1p" | "signum"
                    | "trunc" | "fract" | "degrees" | "radians",
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
                            "math functions in this family operate on floating-point numbers".to_string(),
                            "pass a Float or F32 value".to_string(),
                            Some(arg.expr.span()),
                        ));
                        return None;
                    }
                    return Some(ty);
                }
                ("core.math", "atan2" | "hypot" | "lerp" | "copysign" | "log" | "fma") => {
                    let wanted = if name == "lerp" || name == "fma" { 3 } else { 2 };
                    if args.len() != wanted {
                        self.diags.push(wrong_core_arity(name, wanted, args.len(), span));
                    }
                    let Some(first) = args.get_mut(0).and_then(|a| self.infer(&mut a.expr)) else {
                        for a in args.iter_mut().skip(1) {
                            self.infer(&mut a.expr);
                        }
                        return Some(Type::Float);
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
                            if got != first {
                                self.diags.push(Diagnostic::error(
                                    "E0112",
                                    format!("`{}` needs all arguments to have the same float type", name),
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
                                "sign classifies a floating-point value as negative, zero, or positive".to_string(),
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
                    "checked_add" | "checked_sub" | "checked_mul" | "checked_pow"
                    | "checked_div" | "checked_rem",
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
                (
                    "core.math",
                    "leading_ones" | "trailing_ones" | "digits" | "radix",
                ) => {
                    if args.len() != 1 {
                        self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                    }
                    if let Some(arg) = args.get_mut(0) {
                        if name == "radix" {
                            let ty = self.infer(&mut arg.expr)?;
                            if !matches!(ty, Type::Float | Type::Float32 | Type::Int | Type::IntN { .. }) {
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
                    "is_normal"
                        | "is_subnormal"
                        | "is_canonical"
                        | "is_signed"
                        | "is_zero"
                        | "is_integer"
                        | "sign_bit",
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
                    "next_up"
                        | "next_down"
                        | "copy"
                        | "cot"
                        | "inv"
                        | "erf"
                        | "erfc"
                        | "gamma"
                        | "lgamma"
                        | "logb"
                        | "significand"
                        | "ulp",
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
                        self.diags.push(wrong_core_arity(name, wanted, args.len(), span));
                    }
                    if name == "cmp" || name == "next_after" {
                        let Some(first) = args.get_mut(0).and_then(|a| self.infer(&mut a.expr)) else {
                            for a in args.iter_mut().skip(1) {
                                self.infer(&mut a.expr);
                            }
                            return Some(if name == "cmp" { Type::Int } else { Type::Float });
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
                                    format!("`{}` needs all arguments to have the same float type", name),
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
                    "saturating_add" | "saturating_sub" | "saturating_mul" | "wrapping_add"
                    | "wrapping_sub" | "wrapping_mul" | "gcd" | "lcm" | "int_pow",
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
                            "pow operates on floating-point numbers".to_string(),
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
                    if !matches!(ty, Type::Int | Type::Float | Type::Float32) {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`abs` needs Int, Float, or F32, not {}", ty.show()),
                            "absolute value is only defined for numbers".to_string(),
                            "pass an Int, Float, or F32".to_string(),
                            Some(arg.expr.span()),
                        ));
                        return None;
                    }
                    return Some(ty);
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
                ("core.random", "pick") => {
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
                ("core.random", "sample") => {
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
                        if k != Type::Int {
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
                ("core.random", "weighted_pick") => {
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
                ("core.random", "shuffle") => {
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
                            "the write-capability marker `&` is required; pass the list with that marker"
                                .to_string(),
                            "write `random.shuffle(&xs)` with the write-capability marker `&`".to_string(),
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
                // D-FANOUT3=C: consume a list of task handles and join them in
                // list order. Runtime meaning reuses TaskGroupAll/jet_task_all.
                ("core.tasks", "join_all") => {
                    if args.len() != 1 {
                        self.diags.push(wrong_core_arity(
                            "join_all",
                            1,
                            args.len(),
                            span,
                        ));
                        for arg in args.iter_mut() {
                            self.infer(&mut arg.expr);
                        }
                        return None;
                    }
                    let literal_handles = matches!(&args[0].expr, Expr::ListLit(..));
                    if !literal_handles && args[0].convention != AccessConvention::Move {
                        self.diags.push(Diagnostic::error(
                            "E0201",
                            "`tasks.join_all` consumes its list of task handles".to_string(),
                            "each task handle can be joined only once".to_string(),
                            format!(
                                "pass ownership with the move-capability marker `^`: `tasks.join_all({}handles)`",
                                Syntax::SIGIL_MOVE
                            ),
                            Some(args[0].span),
                        ));
                    }
                    let elem = match self.infer(&mut args[0].expr) {
                        Some(Type::List(inner)) => match *inner {
                            Type::Apply {
                                ref name,
                                ref args,
                                ..
                            } if name == "Task" && args.len() == 1 => args[0].clone(),
                            other => {
                                self.diags.push(Diagnostic::error(
                                    "E0112",
                                    format!(
                                        "`tasks.join_all` needs a list of task handles, not `[{}]`",
                                        other.show()
                                    ),
                                    "each element must be a `Task<T>` handle".to_string(),
                                    "pass a list such as `[first, second]` where each value came from `tasks.spawn`".to_string(),
                                    Some(args[0].expr.span()),
                                ));
                                return None;
                            }
                        },
                        Some(other) => {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!(
                                    "`tasks.join_all` needs a list of task handles, not {}",
                                    other.show()
                                ),
                                "the call waits for and consumes each `Task<T>` in one list"
                                    .to_string(),
                                "pass a `[Task<T>]` list".to_string(),
                                Some(args[0].expr.span()),
                            ));
                            return None;
                        }
                        None => return None,
                    };
                    let mut names = std::collections::HashSet::new();
                    collect_task_handles(&args[0].expr, &mut names);
                    for name in names {
                        self.mark_taskgroup_spawn_consumed(&name);
                    }
                    if let Expr::Ident(name, name_span) = &args[0].expr {
                        self.mark_moved(name.clone(), *name_span);
                    }
                    return Some(Type::List(Box::new(elem)));
                }
                // First finished task wins (consumes the list). Runtime meaning
                // reuses TaskGroupAny/jet_task_any — twin of join_all.
                ("core.tasks", "wait_any") => {
                    if args.len() != 1 {
                        self.diags.push(wrong_core_arity(
                            "wait_any",
                            1,
                            args.len(),
                            span,
                        ));
                        for arg in args.iter_mut() {
                            self.infer(&mut arg.expr);
                        }
                        return None;
                    }
                    let literal_handles = matches!(&args[0].expr, Expr::ListLit(..));
                    if !literal_handles && args[0].convention != AccessConvention::Move {
                        self.diags.push(Diagnostic::error(
                            "E0201",
                            "`tasks.wait_any` consumes its list of task handles".to_string(),
                            "each task handle can be joined only once".to_string(),
                            format!(
                                "pass ownership with the move-capability marker `^`: `tasks.wait_any({}handles)`",
                                Syntax::SIGIL_MOVE
                            ),
                            Some(args[0].span),
                        ));
                    }
                    let elem = match self.infer(&mut args[0].expr) {
                        Some(Type::List(inner)) => match *inner {
                            Type::Apply {
                                ref name,
                                ref args,
                                ..
                            } if name == "Task" && args.len() == 1 => args[0].clone(),
                            other => {
                                self.diags.push(Diagnostic::error(
                                    "E0112",
                                    format!(
                                        "`tasks.wait_any` needs a list of task handles, not `[{}]`",
                                        other.show()
                                    ),
                                    "each element must be a `Task<T>` handle".to_string(),
                                    "pass a list such as `[first, second]` where each value came from `tasks.spawn`".to_string(),
                                    Some(args[0].expr.span()),
                                ));
                                return None;
                            }
                        },
                        Some(other) => {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!(
                                    "`tasks.wait_any` needs a list of task handles, not {}",
                                    other.show()
                                ),
                                "the call waits for the first finished `Task<T>` in one list"
                                    .to_string(),
                                "pass a `[Task<T>]` list".to_string(),
                                Some(args[0].expr.span()),
                            ));
                            return None;
                        }
                        None => return None,
                    };
                    let mut names = std::collections::HashSet::new();
                    collect_task_handles(&args[0].expr, &mut names);
                    for name in names {
                        self.mark_taskgroup_spawn_consumed(&name);
                    }
                    if let Expr::Ident(name, name_span) = &args[0].expr {
                        self.mark_moved(name.clone(), *name_span);
                    }
                    return Some(elem);
                }
                // L2501 is reserved for "whole-file read advisory" but intentionally not
                // emitted here: `fs.read` is kept as sugar (D-IO3) and firing on every call
                // site is too noisy (breaks showcase golden tests via path-specific output).
                // Revisit when the test harness can normalise paths in exact comparisons.
                ("core.files", "read") => {}
                // D-TASKRUNTIME1=A: scheduler timer channels. `after(ms)` emits a unit tick;
                // `after(ms, value)` emits a typed timeout value that can join a select.
                ("core.tasks", "after") => {
                    if !(args.len() == 1 || args.len() == 2) {
                        self.diags.push(Diagnostic::error(
                            "E0104",
                            format!(
                                "`tasks.after` takes one duration and an optional value, got {} argument{}",
                                args.len(),
                                if args.len() == 1 { "" } else { "s" }
                            ),
                            "a one-shot timer channel fires after a whole-millisecond delay".to_string(),
                            "write `tasks.after(ms: 100)` or `tasks.after(ms: 100, value: fallback)`".to_string(),
                            Some(span),
                        ));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        return None;
                    }
                    let ms_ty = self.infer(&mut args[0].expr)?;
                    if !(matches!(ms_ty, Type::Int)
                        || matches!(ms_ty, Type::Named(ref n) if n == "Int" || n == "I64" || n == "I32"))
                    {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!(
                                "`tasks.after(ms: …)` needs an integer millisecond count, not {}",
                                ms_ty.show()
                            ),
                            "timer channels use whole milliseconds".to_string(),
                            "write `tasks.after(ms: 100)`".to_string(),
                            Some(args[0].expr.span()),
                        ));
                    }
                    let elem = if args.len() == 2 {
                        self.infer(&mut args[1].expr)?
                    } else {
                        Type::Named("Unit".to_string())
                    };
                    if let Some(problem) = self.sendability_problem(&elem, false) {
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
                // D-TASKRUNTIME1=A: interval timer sends tick numbers (1, 2, ...).
                ("core.tasks", "interval") => {
                    if args.len() != 1 {
                        self.diags
                            .push(wrong_core_arity("interval", 1, args.len(), span));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        return None;
                    }
                    let ms_ty = self.infer(&mut args[0].expr)?;
                    if !(matches!(ms_ty, Type::Int)
                        || matches!(ms_ty, Type::Named(ref n) if n == "Int" || n == "I64" || n == "I32"))
                    {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!(
                                "`tasks.interval(ms: …)` needs an integer millisecond count, not {}",
                                ms_ty.show()
                            ),
                            "interval channels use whole milliseconds".to_string(),
                            "write `tasks.interval(ms: 1000)`".to_string(),
                            Some(args[0].expr.span()),
                        ));
                    }
                    return Some(Type::Apply {
                        name: "Receiver".to_string(),
                        args: vec![Type::Int],
                    });
                }
                // D-TUPLE-DESTRUCT1: `tasks.channel<T>()` returns the `(Sender<T>,
                // Receiver<T>)` pair directly — mirrors the turbofish `decode<T>` pattern
                // above (the element type `T` comes from the explicit call-site type
                // argument, not a binding annotation; there's no combined "Channel" value
                // to infer against anymore).
                ("core.tasks", "channel") => {
                    if args.len() > 1 {
                        self.diags.push(Diagnostic::error(
                            "E0104",
                            format!(
                                "`tasks.channel` takes an optional capacity, got {} arguments",
                                args.len()
                            ),
                            "a channel may be unbounded or have one whole-number backpressure bound"
                                .to_string(),
                            "write `tasks.channel<T>()` or `tasks.channel<T>(capacity: 1)`".to_string(),
                            Some(span),
                        ));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        return None;
                    }
                    if let Some(cap) = args.get_mut(0) {
                        let cap_ty = self.infer(&mut cap.expr)?;
                        if !(matches!(cap_ty, Type::Int)
                            || matches!(cap_ty, Type::Named(ref n) if n == "Int" || n == "I64" || n == "I32"))
                        {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!(
                                    "`tasks.channel<T>(capacity: …)` needs an integer capacity, not {}",
                                    cap_ty.show()
                                ),
                                "bounded channels use a whole-number memory/backpressure limit"
                                    .to_string(),
                                "write `tasks.channel<T>(capacity: 1)`".to_string(),
                                Some(cap.expr.span()),
                            ));
                        }
                    }
                    let Some(t) = type_args.first().cloned() else {
                        self.diags.push(Diagnostic::error(
                            "E0904",
                            "`tasks.channel` needs a type argument to infer the element type"
                                .to_string(),
                            "the element type `T` can't be guessed without `<T>`".to_string(),
                            "call it with an explicit type argument: `tasks.channel<T>()`".to_string(),
                            Some(span),
                        ));
                        return None;
                    };
                    return Some(Type::Tuple(vec![
                        (
                            "sender".to_string(),
                            Box::new(Type::Apply {
                                name: "Sender".to_string(),
                                args: vec![t.clone()],
                            }),
                        ),
                        (
                            "receiver".to_string(),
                            Box::new(Type::Apply {
                                name: "Receiver".to_string(),
                                args: vec![t],
                            }),
                        ),
                    ]));
                }
                // D-ROUTE1=A: jet.http.router() → HTTPRouter.
                ("jet.http", "router") => {
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
                ("jet.http", "parse") => {
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
                ("jet.http", "dispatch") => {
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
                // E2-M10: jet.http.serve(addr, handler) — blocking accept loop.
                // handler: fn(HTTPRequest) => HTTPResponse (lambda) or HTTPRouter.
                ("jet.http", "serve") => {
                    if args.len() != 2 {
                        self.diags
                            .push(wrong_core_arity("serve", 2, args.len(), span));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        return None;
                    }
                    self.expect_core_arg("serve", 0, &Type::String, &mut args[0]);
                    // Accept an HTTPRouter or a callable (lambda/fn pointer).
                    let handler_ty = self.infer(&mut args[1].expr);
                    match &handler_ty {
                        Some(Type::Fn { .. }) => {}
                        Some(Type::Named(n)) if n == "HTTPRouter" => {}
                        Some(other) => {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`http.serve` handler must be a function or HTTPRouter, not {}", other.show()),
                                "the handler is called with each incoming `HTTPRequest`".to_string(),
                                "pass a router (`http.router()`) or a lambda: `(req) => HTTPResponse { … }`".to_string(),
                                Some(args[1].expr.span()),
                            ));
                        }
                        None => {}
                    }
                    return None; // serve runs forever; no meaningful return type
                }
                // D-DEFER1 option B: scope.guard(() => { … }) → ScopeGuard
                // The argument must be a zero-parameter lambda. LIFO drop order is
                // guaranteed by Rust's reverse-declaration semantics.
                ("core.scope", "guard") => {
                    if args.len() != 1 {
                        self.diags
                            .push(wrong_core_arity("guard", 1, args.len(), span));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        return None;
                    }
                    let lam_ty = self.infer(&mut args[0].expr);
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
                                    "write `scope.guard(() => { cleanup_code })` with no parameters".to_string(),
                                    Some(args[0].expr.span()),
                                ));
                            }
                        }
                        Some(other) => {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`scope.guard` needs a lambda, not {}", other.show()),
                                "a scope guard runs a cleanup lambda when the binding goes out of scope".to_string(),
                                "write `scope.guard(() => { cleanup_code })`".to_string(),
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
                ("jet.reactive", "signal") => {
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
                ("jet.reactive", "derived") => {
                    if args.len() != 1 {
                        self.diags
                            .push(wrong_core_arity("derived", 1, args.len(), span));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        return None;
                    }
                    let lam_ty = self.infer(&mut args[0].expr);
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
                ("jet.reactive", "computed") => {
                    if args.len() != 1 {
                        self.diags
                            .push(wrong_core_arity("computed", 1, args.len(), span));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        return None;
                    }
                    let lam_ty = self.infer(&mut args[0].expr);
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
                // D-RENDERTGT2=A (c133 M2): `ui.reactive_render(() => { … })` — reactive
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
                    let lam_ty = self.infer(&mut args[0].expr);
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
                // D-WEB-CLICK-PORT1=D: `ui.button(label)` or
                // `ui.button(label, on_click: () => …)`.
                ("core.ui", "button") => {
                    if args.len() != 1 && args.len() != 2 {
                        self.diags
                            .push(wrong_core_arity("button", 1, args.len(), span));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        return None;
                    }
                    if args.len() == 2 {
                        super::net_text_time::require_exact_labels(
                            "ui.button",
                            args,
                            &[(1, "on_click")],
                            span,
                            &mut self.diags,
                        );
                    }
                    let label_ty = self.infer(&mut args[0].expr);
                    if let Some(got) = label_ty {
                        if got != Type::String {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!(
                                    "`button` wants String for argument 1, but this is {}",
                                    got.show()
                                ),
                                "every argument must match its parameter's type".to_string(),
                                "pass a string label".to_string(),
                                Some(args[0].expr.span()),
                            ));
                        }
                    }
                    if args.len() == 2 {
                        let saved_esc = self.lambda_escapes;
                        self.lambda_escapes = true;
                        let lam_ty = self.infer(&mut args[1].expr);
                        self.lambda_escapes = saved_esc;
                        match &lam_ty {
                            Some(Type::Fn { params, .. }) => {
                                if !params.is_empty() {
                                    self.diags.push(reactive_lambda_arity(
                                        "button on_click",
                                        params.len(),
                                        args[1].expr.span(),
                                    ));
                                }
                            }
                            Some(other) => {
                                self.diags.push(reactive_not_lambda(
                                    "button on_click",
                                    other,
                                    args[1].expr.span(),
                                ));
                            }
                            None => {}
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
                ("jet.reactive", "effect") => {
                    if args.len() != 1 {
                        self.diags
                            .push(wrong_core_arity("effect", 1, args.len(), span));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        return None;
                    }
                    let lam_ty = self.infer(&mut args[0].expr);
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
                        self.diags.push(wrong_core_arity("async_result", 2, args.len(), span));
                        for arg in args.iter_mut() { self.infer(&mut arg.expr); }
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
                    self.expect_core_arg("async_result", 0, &Type::Named(crate::Syntax::TYPE_ASYNC_POLICY.to_string()), &mut args[0]);
                    self.expect_core_arg("async_result", 1, &Type::Named(crate::Syntax::TYPE_FAILURE_POLICY.to_string()), &mut args[1]);
                    return Some(Type::Result {
                        ok: Box::new(Type::Apply {
                            name: crate::Syntax::TYPE_ASYNC_EVENT.to_string(),
                            args: vec![type_args[0].clone(), type_args[1].clone()],
                        }),
                        err: Box::new(Type::Named(crate::Syntax::TYPE_EVENT_CONFIG_ERROR.to_string())),
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
                        self.diags.push(wrong_core_arity("decision_hook", 1, args.len(), span));
                        for arg in args.iter_mut() { self.infer(&mut arg.expr); }
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
                        self.diags.push(wrong_core_arity("idle", 0, args.len(), span));
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
                        self.diags.push(wrong_core_arity("loading", 0, args.len(), span));
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
                    "core.os",
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
                        | "stop",
                ) => {
                    if !self.in_unsafe {
                        self.diags.push(Diagnostic::error(
                            "E3101",
                            format!("`core.os.{name}` requires an audited `#Unsafe` region"),
                            "POSIX process and session control can change credentials, signals, and process topology (I1)".to_string(),
                            format!("wrap the call in `#Unsafe(\"posix {name}: …\") {{ … }}` and gate the host OS with `$if build.os` / `#Target(OS.*)`"),
                            Some(span),
                        ));
                    }
                    // Continue into shared fixed-signature checking below.
                }
                // D-CRYPTOENV1=A: expert-only raw crypto — requires import + #Unsafe gate.
                ("core.crypto.expert" | "core.vault.expert", _) => {
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
                        self.diags.push(Diagnostic::error(
                            "E0510",
                            what,
                            why,
                            fix,
                            Some(span),
                        ));
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
                                "raw key import may only run inside an explicit expert-tier gate (I1)".to_string(),
                                "wrap the call in `#Unsafe(\"vault key import: …\") { … }`".to_string(),
                            )
                        };
                        self.diags.push(Diagnostic::error(
                            "E0510",
                            what,
                            why,
                            fix,
                            Some(span),
                        ));
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
                ("core.ws", "connect") => {
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
                ("core.ws", "upgrade") => {
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
                ("core.browser", "profile") => {
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
                ("core.browser", "timeout") => {
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
                ("core.browser", "locked") => {
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
                ("core.browser", "connect") => {
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
                ("core.browser", "connect_profile") => {
                    if args.len() != 3 {
                        self.diags.push(wrong_core_arity(
                            "connect_profile",
                            3,
                            args.len(),
                            span,
                        ));
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
                    if args.len() != 2 && args.len() != 3 {
                        self.diags.push(Diagnostic::error(
                            "E0104",
                            format!("`bind` expects 2 arguments, or 3 with `tls:`, got {}", args.len()),
                            "HTTPS binding uses the named `tls:` option so plaintext and TLS share one entry point".to_string(),
                            "write `Server.bind(addr, mux)` or `Server.bind(addr, mux, tls: Server.tls(cert, key))`".to_string(),
                            Some(span),
                        ));
                        for arg in args.iter_mut() { self.infer(&mut arg.expr); }
                        return None;
                    }
                    self.expect_core_arg("bind", 0, &Type::String, &mut args[0]);
                    self.expect_core_arg("bind", 1, &Type::Named("HTTPMux".to_string()), &mut args[1]);
                    if args.len() == 3 && !matches!(&args[2].expr, Expr::Absent(_)) {
                        self.expect_core_arg(
                            "bind",
                            2,
                            &Type::Named("HTTPServerTls".to_string()),
                            &mut args[2],
                        );
                    }
                    return Some(Type::Result {
                        ok: Box::new(Type::Named("HTTPServer".to_string())),
                        err: Box::new(Type::Named("HTTPError".to_string())),
                    });
                }
                ("core.http.server", "serve") => {
                    if args.len() != 2 && args.len() != 3 {
                        self.diags.push(Diagnostic::error(
                            "E0104",
                            format!("`serve` expects 2 arguments, or 3 with `tls:`, got {}", args.len()),
                            "HTTPS serving uses the named `tls:` option so plaintext and TLS share one entry point".to_string(),
                            "write `Server.serve(addr, mux)` or `Server.serve(addr, mux, tls: Server.tls(cert, key))`".to_string(),
                            Some(span),
                        ));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        return None;
                    }
                    self.expect_core_arg("serve", 0, &Type::String, &mut args[0]);
                    // second arg is a Mux — just infer it
                    self.infer(&mut args[1].expr);
                    if args.len() == 3 && !matches!(&args[2].expr, Expr::Absent(_)) {
                        self.expect_core_arg(
                            "serve",
                            2,
                            &Type::Named("HTTPServerTls".to_string()),
                            &mut args[2],
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
                    for (index, want) in
                        [(1, &string_list), (2, &string_list), (3, &Type::Bool), (4, &Type::Int)]
                    {
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
                    self.expect_core_arg(
                        "cors",
                        0,
                        &Type::Named("HTTPMux".to_string()),
                        &mut args[0],
                    );
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
                ("core.time.date", "new") => {
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
                ("core.time.date", "today") => {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return Some(Type::Named("LocalDate".to_string()));
                }
                ("core.time.date", "parse") => {
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
                ("core.time.datetime", "from_timestamp") => {
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
                ("core.time.datetime", "now") => {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return Some(Type::Named("DateTime".to_string()));
                }
                // D-APPROX1=A: sketch constructors.
                ("core.sketch.hll", "new") => {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return Some(Type::Named("HyperLogLog".to_string()));
                }
                ("core.sketch.tdigest", "new") => {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return Some(Type::Named("TDigest".to_string()));
                }
                ("core.sketch.cms", "new") => {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return Some(Type::Named("CountMinSketch".to_string()));
                }
                ("core.sketch.reservoir", "new") => {
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
                // D-HONESTNUM1=A: `M.from(value, uncertainty)` → `Measurement<Float>`.
                ("core.science.measurement", "from") => {
                    if args.len() != 2 {
                        self.diags
                            .push(wrong_core_arity("from", 2, args.len(), span));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        return None;
                    }
                    self.expect_core_arg("from", 0, &Type::Float, &mut args[0]);
                    self.expect_core_arg("from", 1, &Type::Float, &mut args[1]);
                    return Some(Type::Apply {
                        name: crate::Syntax::TYPE_MEASUREMENT.to_string(),
                        args: vec![Type::Float],
                    });
                }
                // D-NUMTYPE1=A: core.math.fraction(top, bottom) answers an exact
                // ratio, or nothing when the bottom is zero.
                ("core.math", "fraction") => {
                    if args.len() != 2 {
                        self.diags.push(wrong_core_arity("fraction", 2, args.len(), span));
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
                // `Int ? TextError`). Named-arg dispatch mirrors `game.run`.
                ("core.text", "display_width") => {
                    match args.len() {
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
                            self.diags.push(wrong_core_arity("display_width", 1, n, span));
                            for a in args.iter_mut() {
                                self.infer(&mut a.expr);
                            }
                            return None;
                        }
                    }
                }
                _ => {}
            }

            // D-FFI-SH1=A / D-UNIFYLIT1=A: `process.run` accepts `Sh` (from
            // `Sh.{"…"}` / `Sh.raw`) or an explicit `[String]` argv list.
            // Bare `"…"` is `String` and E0149 — no silent typed-text rewrite.
            if module == "core.process" && name == "run" {
                let Some((_, ret)) = sig else { unreachable!() };
                if args.len() != 1 {
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
                for arg in args.iter_mut().skip(1) {
                    self.infer(&mut arg.expr);
                }
                return ret;
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
            if module == "core.os" && name == "on_interrupt" {
                if args.len() != params.len() {
                    self.diags
                        .push(wrong_core_arity(name, params.len(), args.len(), span));
                }
                if let (Some((conv, param_ty)), Some(arg)) =
                    (params.first(), args.get_mut(0))
                {
                    debug_assert_eq!(*conv, AccessConvention::Read);
                    let saved_lambda_escapes = self.lambda_escapes;
                    let saved_callback_depth = self.interrupt_callback_depth;
                    self.lambda_escapes = true;
                    self.interrupt_callback_depth += 1;
                    self.expect_core_arg(name, 0, param_ty, arg);
                    self.interrupt_callback_depth = saved_callback_depth;
                    self.lambda_escapes = saved_lambda_escapes;
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
                    self.diags
                        .push(wrong_core_arity(name, 2, args.len(), span));
                }
                for (i, ((conv, param_ty), arg)) in
                    params.iter().zip(args.iter_mut()).enumerate()
                {
                    debug_assert_eq!(*conv, AccessConvention::Read);
                    self.expect_core_arg(name, i, param_ty, arg);
                }
                for arg in args.iter_mut().skip(params.len()) {
                    self.infer(&mut arg.expr);
                }
                if self.in_unsafe
                    && self.core_imports.values().any(|imported| imported == module)
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
            let raw_envelope_diagnostic =
                safe_envelope_raw_argument(module, name, args, params.len());
            if args.len() != params.len() && raw_envelope_diagnostic.is_none() {
                self.diags
                    .push(wrong_core_arity(name, params.len(), args.len(), span));
            }
            for (i, ((conv, param_ty), arg)) in params.iter().zip(args.iter_mut()).enumerate() {
                if *conv == AccessConvention::Move {
                    if arg.convention != AccessConvention::Move {
                        self.diags.push(Diagnostic::error(
                            "E0201",
                            format!("argument {} to `{}` transfers ownership through the move-capability marker `^`", i + 1, name),
                            "this standard library constructor retains the consumed handle".to_string(),
                            format!("write the move-capability marker `^`: `{}value` for this argument", Syntax::SIGIL_MOVE),
                            Some(arg.span),
                        ));
                    }
                    self.expect_core_arg_moving(name, i, param_ty, arg);
                    continue;
                }
                if *conv == AccessConvention::Write && arg.convention != AccessConvention::Write {
                    self.diags.push(Diagnostic::error(
                        "E0202",
                        format!(
                            "argument {} to `{}` requires the write-capability marker `&`",
                            i + 1,
                            name
                        ),
                        "this standard library call edits that value in place".to_string(),
                        format!("write the write-capability marker `&`: `{}value` for this argument", Syntax::SIGIL_WRITE),
                        Some(arg.span),
                    ));
                }
                self.expect_core_arg(name, i, param_ty, arg);
            }
            for arg in args.iter_mut().skip(params.len()) {
                self.infer(&mut arg.expr);
            }
            let expert_gate_ok = module != "core.crypto.expert" || (
                self.in_unsafe
                    && self.core_imports.values().any(|imported| imported == "core.crypto.expert")
            );
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
