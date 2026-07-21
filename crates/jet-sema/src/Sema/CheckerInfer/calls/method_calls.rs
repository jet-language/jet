use crate::AST::{AccessConvention, CtValue, EnumLitArg, Expr, StrPart, Type};
use crate::Collections;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Generics::e0901;
use crate::Sema::Checker;
use crate::Sema::CheckerCoreLib::{
    alloc_method_return, args_spec_method_return, binary_reader_method_return,
    civil_time_method_return, data_renamed_to_datatree, datatree_method_return,
    devserver_method_return, db_value_method_return, expiring_method_return,
    email_method_return, encoding_handle_method_return, file_handle_method_return, http_type_method_return, is_db_value_type_name,
    is_json_type_name, is_layout_axis_type, is_layout_type, is_math_type,
    is_polymorphic_core_special, is_reflect_type_name, is_simd_lane_type, json_ty,
    layout_method_arg_ty, layout_method_return, loadable_method_return, math_method_arg_ty,
    math_method_return, math_scalar_ty, math_static_arg_ty, math_static_return,
    net_method_return, parsed_args_method_return, path_method_return, require_net_method_labels,
    process_child_method_return, process_spec_method_return, process_stdin_method_return,
    process_stream_method_return, reflect_method_return, regex_method_return, result_ty,
    rotting_method_return, simd_reduce_markers, sketch_method_return, sketch_type_name,
    text_cursor_method_return, u8_ty, ui_backend_method_return, unit_ty, url_mime_method_return,
    wrong_core_arity,
};
use crate::Sema::CheckerInfer::contains_tuple_type;
use crate::Sema::Diagnostics::{
    aliasing_while_mut, builtin_type_from_ident, collection_changed_in_loop, expr_root_ident,
    type_is_copy,
};
use crate::Sema::Effects::Effect;
use crate::Syntax;

fn is_http_route_registration(type_name: &str, method: &str) -> bool {
    match type_name {
        "HttpRouter" => matches!(method, "get" | "post" | "put" | "delete"),
        "HttpMux" => matches!(method, "get" | "post" | "put" | "delete" | "patch" | "head" | "options"),
        _ => false,
    }
}

impl<'a> Checker<'a> {
    fn check_http_route_constant(
        &mut self,
        type_name: &str,
        method: &str,
        args: &[crate::AST::CallArg],
    ) {
        if !is_http_route_registration(type_name, method) {
            return;
        }
        let Some(arg) = args.first() else { return; };
        let result = if let Expr::Str(parts, _) = &arg.expr {
            if parts.iter().any(|part| matches!(part, StrPart::Interp(..))) {
                let mut source = String::new();
                for part in parts {
                    match part {
                        StrPart::Lit(text) => source.push_str(text),
                        StrPart::Interp(expr, _) => match expr.as_ref() {
                            Expr::Ident(name, _) => source.push_str(&format!("{{{name}}}")),
                            _ => source.push_str("{…}"),
                        },
                    }
                }
                Err((
                    source,
                    "brace interpolation is not allowed in route patterns; it is indistinguishable from the retired `{name}` parameter spelling".to_string(),
                ))
            } else {
                match self.evaluate_constant(&arg.expr) {
                    Some(CtValue::Str(pattern)) => Syntax::validate_http_route_pattern(&pattern)
                        .map_err(|reason| (pattern, reason)),
                    _ => return,
                }
            }
        } else {
            match self.evaluate_constant(&arg.expr) {
                Some(CtValue::Str(pattern)) => Syntax::validate_http_route_pattern(&pattern)
                    .map_err(|reason| (pattern, reason)),
                _ => return,
            }
        };
        if let Err((pattern, reason)) = result {
            self.diags.push(Diagnostic::error(
                "E2805",
                format!("invalid HTTP route `{pattern}`: {reason}"),
                "route patterns use one canonical grammar; ambiguous escapes, traversal, duplicate names, and retired markers would make routing and audit metadata disagree"
                    .to_string(),
                "use `:name` for one segment or final `*name` for a catch-all; percent-encode a literal leading `:` or `*`, and never encode `/`"
                    .to_string(),
                Some(arg.span),
            ));
        }
    }

    pub(crate) fn infer_method_call(
            &mut self,
            receiver: &mut Box<Expr>,
            method: &str,
            span: Span,
            type_args: &mut Vec<Type>,
            args: &mut Vec<crate::AST::CallArg>,
            recv_type_out: &mut Option<String>,
            resolved_ret_out: &mut Option<Type>,
        ) -> Option<Type> {
            // D-SHAPE-PLACE1=A: `.view(a..b)` is retired. Keep the parser's
            // range-shaped recovery long enough to point at the old spelling,
            // but never admit it to the type system.
            if method == Syntax::METHOD_VIEW {
                self.infer(receiver);
                for arg in args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                self.diags.push(Diagnostic::error(
                    "E0214",
                    "`.view(a..b)` is now a bare range place".to_string(),
                    "Jet uses one place-access rule: bare reads, `&` edits, and `~` copies"
                        .to_string(),
                    "replace `value.view(a..b)` with `value[a..b]`".to_string(),
                    Some(span),
                ));
                return None;
            }
            // D-TYPEDTEXT1=D: `Sql.raw("…")` / `Html.raw("…")` — the sole audited
            // escape from a runtime `String` into a typed-text position. `Sql`/
            // `Html` here name the type, not a value (checked via `lookup` so a
            // shadowing local of that name still resolves normally below).
            if method == "raw" {
                if let Expr::Ident(n, _) = receiver.as_ref() {
                    if (n == "Sql" || n == "Html" || n == Syntax::TYPE_SH) && self.lookup(n).is_none() {
                        let type_name = n.clone();
                        if args.len() != 1 {
                            self.diags.push(Diagnostic::error(
                                "E0103",
                                format!("`{}.raw()` takes exactly one argument", type_name),
                                "`.raw()` wraps one already-audited `String` as the escape hatch"
                                    .to_string(),
                                format!("write `{}.raw(text)`", type_name),
                                Some(span),
                            ));
                            return Some(Type::Named(type_name));
                        }
                        let arg_ty = self.infer(&mut args[0].expr);
                        if let Some(t) = arg_ty {
                            self.check_type_assignable(&Type::String, &t, args[0].expr.span());
                        }
                        return Some(Type::Named(type_name));
                    }
                }
            }
            // D-PATHFS1 / E0340: `read_dir` is not a Jet API — teach the typed path path.
            if method == "read_dir" {
                self.infer(receiver); // still type-check the receiver
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                self.diags.push(Diagnostic::error(
                    "E0340",
                    "`read_dir` is not a method in Jet".to_string(),
                    "Jet uses typed paths; raw-string directory helpers are not exposed".to_string(),
                    "write `Path.from(path).walk()` to list a directory recursively".to_string(),
                    Some(span),
                ));
                return None;
            }
            // D-CAP2 (D-MEM1/S4): `.clone()` is not user-typable Jet syntax — `clone`
            // falls through to the ordinary "no such method" path below like any
            // other unrecognized name (I8: `copy x` is the one copy spelling).
            self.lint_allocation_hints(method, span);
            // D-DIST3 (ratified 2026-06-20): `.raw()` unwraps a distinct type.
            if method == crate::Syntax::METHOD_DISTINCT_RAW {
                self.borrow_ctx = true;
                let recv_ty = self.infer(receiver)?;
                if let Type::Named(ref n) = recv_ty {
                    if let Some(base) = self.registry.distinct_base(n).cloned() {
                        if !args.is_empty() {
                            self.diags.push(Diagnostic::error(
                                "E0103",
                                format!(
                                    "`.{}()` takes no arguments",
                                    crate::Syntax::METHOD_DISTINCT_RAW
                                ),
                                "`.raw()` simply unwraps the base value — no arguments needed"
                                    .to_string(),
                                "write `.raw()` with no arguments".to_string(),
                                Some(span),
                            ));
                        }
                        return Some(base);
                    }
                }
                self.diags.push(Diagnostic::error(
                    "E0311",
                    format!(
                        "`.{}()` is only valid on a distinct type value",
                        crate::Syntax::METHOD_DISTINCT_RAW
                    ),
                    "`.raw()` unwraps a distinct type to its base representation".to_string(),
                    format!(
                        "only call `.raw()` on a value whose type was declared with `{} distinct`",
                        crate::Syntax::SIGIL_BIND_IMMUT
                    ),
                    Some(span),
                ));
                return None;
            }
            // D-TOOL4 (E2-M11): `expect(x).snapshot()` — the special snapshot
            // assertion. Recognized by checking the receiver type.
            if method == Syntax::BUILTIN_SNAPSHOT {
                let recv_ty = self.infer(receiver);
                if recv_ty
                    .as_ref()
                    .map(|t| t == &Type::Named("__JetExpect__".to_string()))
                    .unwrap_or(false)
                {
                    // Valid: snapshot assertion — void, no return type.
                    return None;
                }
                // Not from expect() — error.
                self.diags.push(Diagnostic::error(
                    "E2901",
                    format!(
                        "`.{}()` is only valid on the result of `{}(…)`",
                        Syntax::BUILTIN_SNAPSHOT,
                        Syntax::BUILTIN_EXPECT
                    ),
                    "snapshot testing: call `expect(value).snapshot()` in a test block".to_string(),
                    format!("e.g. `{}(my_result).snapshot()`", Syntax::BUILTIN_EXPECT),
                    Some(span),
                ));
                return None;
            }
            if let Expr::Ident(root, _) = &**receiver {
                if root == "File" && method == Syntax::FOREIGN_OPEN {
                    self.diags.push(Diagnostic::error(
                        "E0038",
                        "`File.open` is not the M10 file API".to_string(),
                        "M10 uses whole-file helpers in `core.files`, not a `File.open` handle type"
                            .to_string(),
                        "import `core.files as fs` and call `fs.read(path)` or `fs.write(path, text)` \
                         (or `fs.open(path)` for a streaming handle)"
                            .to_string(),
                        Some(span),
                    ));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
            }
            // D-PROTO1/D-PROTO2: `Payment.Client.client()` — static call on a dotted
            // protocol handle type (PascalCase.PascalCase).
            if let Expr::Field(base, leaf, _) = &**receiver {
                if let Expr::Ident(prefix, _) = &**base {
                    if prefix
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_uppercase())
                    {
                        let full = format!("{prefix}.{leaf}");
                        if self.registry.method(&full, method).is_some() {
                            return self.check_static_method(&full, method, span, type_args, args);
                        }
                    }
                }
            }
            // D-ENC1: nested-namespace access — `encoding.json.to_string(x)` where `encoding`
            // is a library alias (`use core.encoding`) and `json` a registered submodule. The
            // method-call receiver is `Field(Ident(alias), leaf)`; resolve to the submodule
            // `<ns>.<leaf>` as a core call. Guarded by `is_known_core_module`, so it fires only
            // for real submodules (e.g. `core.encoding.json`), never plain field access.
            if let Expr::Field(base, leaf, _) = &**receiver {
                if let Expr::Ident(alias, _) = &**base {
                    if let Some(ns) = self.core_imports.get(alias).cloned() {
                        if ns == "core.tls" && leaf == "ClientConfig" && method == "default" {
                            if !args.is_empty() {
                                self.diags.push(wrong_core_arity("ClientConfig.default", 0, args.len(), span));
                                for arg in args.iter_mut() {
                                    self.infer(&mut arg.expr);
                                }
                            }
                            *recv_type_out = Some("TlsClientConfigType".to_string());
                            return Some(Type::Named("TlsClientConfig".to_string()));
                        }
                        if ns == "core.http.client" && leaf == "Client" && method == "new" {
                            if !args.is_empty() {
                                self.diags
                                    .push(wrong_core_arity("Client.new", 0, args.len(), span));
                                for arg in args.iter_mut() {
                                    self.infer(&mut arg.expr);
                                }
                            }
                            *recv_type_out = Some("HttpClientType".to_string());
                            return Some(Type::Named("HttpClient".to_string()));
                        }
                        if ns == "core.tls" && leaf == "RootCertificates" && method == "from_pem" {
                            if args.len() != 1 {
                                self.diags.push(wrong_core_arity("RootCertificates.from_pem", 1, args.len(), span));
                            }
                            if let Some(arg) = args.first_mut() {
                                self.expect_core_arg("RootCertificates.from_pem", 0, &Type::List(Box::new(u8_ty())), arg);
                            }
                            *recv_type_out = Some("TlsRootCertificatesType".to_string());
                            return Some(result_ty(
                                Type::Named("TlsRootCertificates".to_string()),
                                Type::Named(Syntax::TYPE_IO_ERROR.to_string()),
                            ));
                        }
                        if ns == "core.tls" && leaf == "ClientIdentity" && method == "from_pem" {
                            if args.len() != 2 {
                                self.diags.push(wrong_core_arity("ClientIdentity.from_pem", 2, args.len(), span));
                            }
                            crate::Sema::CheckerCoreLib::require_exact_labels(
                                "ClientIdentity.from_pem", args,
                                &[(0, "cert_chain"), (1, "private_key")], span, &mut self.diags,
                            );
                            for (index, arg) in args.iter_mut().enumerate() {
                                self.expect_core_arg("ClientIdentity.from_pem", index, &Type::List(Box::new(u8_ty())), arg);
                            }
                            *recv_type_out = Some("TlsClientIdentityType".to_string());
                            return Some(result_ty(
                                Type::Named("TlsClientIdentity".to_string()),
                                Type::Named(Syntax::TYPE_IO_ERROR.to_string()),
                            ));
                        }
                        if ns == "core.vault"
                            && leaf == "KeyUnlock"
                            && matches!(method, "Recipient" | "Passphrase")
                        {
                            if args.len() != 1 {
                                self.diags.push(wrong_core_arity(
                                    &format!("KeyUnlock.{method}"),
                                    1,
                                    args.len(),
                                    span,
                                ));
                            }
                            let expected = crate::Sema::Diagnostics::core_crypto_nominal(if method == "Recipient" {
                                Type::Named("X25519SecretKey".to_string())
                            } else {
                                Type::Named("Secret".to_string())
                            });
                            if let Some(arg) = args.first_mut() {
                                self.expect_core_arg(&format!("KeyUnlock.{method}"), 0, &expected, arg);
                            }
                            **receiver = Expr::Ident("KeyUnlock".to_string(), span);
                            *resolved_ret_out = Some(Type::Named("KeyUnlock".to_string()));
                            return Some(Type::Named("KeyUnlock".to_string()));
                        }
                        if crate::Sema::CheckerCoreLib::core_module_type_item(&ns, leaf) {
                            let type_name = if matches!(ns.as_str(), "jet.http" | "core.http.client" | "core.http.server") {
                                match leaf.as_str() {
                                    "Method" | "Status" | "Version" | "HeaderName" | "HeaderValue"
                                    | "Headers" | "Request" | "Response" | "Body" | "Handler" | "Error" | "Proxy" => {
                                        format!("Http{leaf}")
                                    }
                                    "HttpError" => "HttpError".to_string(),
                                    _ => leaf.clone(),
                                }
                            } else {
                                leaf.clone()
                            };
                            **receiver = Expr::Ident(type_name.clone(), span);
                            let ty = Type::Named(type_name.clone());
                            let http_error = || Type::Named("HttpError".to_string());
                            let http_result = |ok: Type| Type::Result {
                                ok: Box::new(ok),
                                err: Box::new(http_error()),
                            };
                            let http_static = match (type_name.as_str(), method, args.len()) {
                                ("HttpMethod", "custom", 1) => {
                                    self.expect_core_arg("Method.custom", 0, &Type::String, &mut args[0]);
                                    Some(http_result(ty.clone()))
                                }
                                ("HttpMethod", "get" | "head" | "post" | "put" | "delete"
                                    | "connect" | "options" | "trace" | "patch", 0) => Some(ty.clone()),
                                ("HttpStatus", "new", 1) => {
                                    self.expect_core_arg("Status.new", 0, &Type::Int, &mut args[0]);
                                    Some(http_result(ty.clone()))
                                }
                                ("HttpVersion", "http_1_0" | "http_1_1" | "http_2", 0) => Some(ty.clone()),
                                ("HttpHeaderName", "new", 1) | ("HttpHeaderValue", "new", 1) => {
                                    self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                                    Some(http_result(ty.clone()))
                                }
                                ("HttpHeaders", "new", 0) | ("HttpBody", "empty", 0) => Some(ty.clone()),
                                ("HttpBody", "bytes", 1) => {
                                    self.expect_core_arg("Body.bytes", 0, &Type::List(Box::new(u8_ty())), &mut args[0]);
                                    Some(ty.clone())
                                }
                                ("HttpBody", "text", 1 | 2) => {
                                    self.expect_core_arg("Body.text", 0, &Type::String, &mut args[0]);
                                    if args.len() == 2 {
                                        self.expect_core_arg("Body.text", 1, &Type::Named("Mime".to_string()), &mut args[1]);
                                    }
                                    Some(ty.clone())
                                }
                                ("HttpBody", "json", 1) => {
                                    self.infer(&mut args[0].expr);
                                    Some(ty.clone())
                                }
                                ("HttpBody", "form" | "multipart", 1) => {
                                    self.expect_core_arg(
                                        method,
                                        0,
                                        &Type::Map { key: Box::new(Type::String), key_span: None, value: Box::new(Type::String) },
                                        &mut args[0],
                                    );
                                    Some(ty.clone())
                                }
                                ("HttpBody", "reader", 1 | 2) => {
                                    self.expect_core_arg_moving("Body.reader", 0, &Type::Named("FileReader".to_string()), &mut args[0]);
                                    args[0].convention = AccessConvention::Move;
                                    if args.len() == 2 {
                                        self.expect_core_arg("Body.reader", 1, &Type::Int, &mut args[1]);
                                    }
                                    Some(http_result(ty.clone()))
                                }
                                _ => None,
                            };
                            if let Some(ret) = http_static {
                                *resolved_ret_out = Some(ret.clone());
                                return Some(ret);
                            }
                            if let Some(ret) = Collections::builtin_method_return(&ty, method, args.len(), true) {
                                let ret = self.finish_builtin_method(receiver, method, &ty, args, span, ret);
                                let ret = if matches!(ns.as_str(), "jet.crypto" | "core.crypto") {
                                    ret.map(crate::Sema::Diagnostics::core_crypto_nominal)
                                } else {
                                    ret
                                };
                                *resolved_ret_out = ret.clone();
                                return ret;
                            }
                            return self.check_static_method(&type_name, method, span, type_args, args);
                        }
                        if ns == "core.encoding" && leaf == "EncodingLimits" && method == "safe" {
                            return self.check_static_method("EncodingLimits", method, span, type_args, args);
                        }
                        if ns == "core.encoding.cbor" && leaf == "CBOROptions" && method == "safe" {
                            return self.check_static_method("CBOROptions", method, span, type_args, args);
                        }
                        if ns == "core.encoding.xml" && (leaf == "XMLLimits" || leaf == "XMLParseOptions") && method == "safe" {
                            return self.check_static_method(leaf, method, span, type_args, args);
                        }
                        if ns == "core.email" && leaf == "Limits" && method == "safe" {
                            return self.check_static_method("Limits", method, span, type_args, args);
                        }
                        if ns == "core.encoding" && leaf == "DataEvent" {
                            let saved: Vec<Expr> = args
                                .iter_mut()
                                .map(|a| std::mem::replace(&mut a.expr, Expr::Int(0, a.span, None, None)))
                                .collect();
                            let mut enum_args: Vec<EnumLitArg> =
                                saved.into_iter().map(EnumLitArg::Positional).collect();
                            let ty = self.check_enum_lit("DataEvent", method, &mut enum_args, span);
                            for (arg, enum_arg) in args.iter_mut().zip(enum_args) {
                                if let EnumLitArg::Positional(expr) = enum_arg { arg.expr = expr; }
                            }
                            **receiver = Expr::Ident("DataEvent".to_string(), span);
                            return Some(ty);
                        }
                        if ns == "core.solve" && leaf == Syntax::SOLVER_TYPE && method == "new" {
                            if args.len() != 1 {
                                self.diags.push(Diagnostic::error(
                                    "E0101",
                                    format!("`Solver.new` takes 1 argument, got {}", args.len()),
                                    "solver construction needs one deterministic seed".to_string(),
                                    "write `Solve.Solver.new(seed)`".to_string(),
                                    Some(span),
                                ));
                            }
                            for a in args.iter_mut() {
                                self.infer(&mut a.expr);
                            }
                            *recv_type_out = Some(Syntax::SOLVER_TYPE.to_string());
                            return Some(Type::Named(Syntax::SOLVER_TYPE.to_string()));
                        }
                        if ns == "core.game" {
                            match (leaf.as_str(), method) {
                                ("Scene", "new") => {
                                    if args.len() != 1 {
                                        self.diags.push(wrong_core_arity(
                                            "Scene.new",
                                            1,
                                            args.len(),
                                            span,
                                        ));
                                    }
                                    if let Some(arg) = args.get_mut(0) {
                                        self.expect_core_arg("Scene.new", 0, &Type::String, arg);
                                    }
                                    *recv_type_out = Some("GameSceneType".to_string());
                                    return Some(Type::Named("GameScene".to_string()));
                                }
                                ("Replay", "record") => {
                                    if args.len() != 1 {
                                        self.diags.push(wrong_core_arity(
                                            "Replay.record",
                                            1,
                                            args.len(),
                                            span,
                                        ));
                                    }
                                    if let Some(arg) = args.get_mut(0) {
                                        self.expect_core_arg("Replay.record", 0, &Type::String, arg);
                                        if let Expr::Str(parts, literal_span) = &arg.expr {
                                            if let [StrPart::Lit(path)] = parts.as_slice() {
                                                if crate::Syntax::artifact_kind(path) != Some(crate::Syntax::ArtifactKind::GameReplay) {
                                                    let actual = crate::Syntax::artifact_kind(path);
                                                    let why = match actual {
                                                        Some(crate::Syntax::ArtifactKind::ProofReplay) => "that suffix identifies a proof replay, not a game input replay".to_string(),
                                                        Some(kind) => format!("that suffix identifies a {kind:?} artifact, not a game input replay"),
                                                        None => format!("game input replay paths end in `{}`", crate::Syntax::ARTIFACT_EXT_GAME_REPLAY),
                                                    };
                                                    self.diags.push(Diagnostic::error(
                                                        "E0103",
                                                        format!("`Replay.record` needs a `{}` game replay path", crate::Syntax::ARTIFACT_EXT_GAME_REPLAY),
                                                        why,
                                                        format!("rename the path to end in `{}`", crate::Syntax::ARTIFACT_EXT_GAME_REPLAY),
                                                        Some(*literal_span),
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                    *recv_type_out = Some("GameReplayType".to_string());
                                    return Some(Type::Named("GameReplay".to_string()));
                                }
                                ("Backend", "headless") => {
                                    if !args.is_empty() {
                                        self.diags.push(wrong_core_arity(
                                            "Backend.headless",
                                            0,
                                            args.len(),
                                            span,
                                        ));
                                        for a in args.iter_mut() {
                                            self.infer(&mut a.expr);
                                        }
                                    }
                                    *recv_type_out = Some("GameBackendType".to_string());
                                    return Some(Type::Named("GameBackend".to_string()));
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            if let Some((module, alias_span)) = self.core_module_path_from_receiver(receiver) {
                let ret = self.infer_core_call(&module, method, alias_span, span, type_args, args);
                if is_polymorphic_core_special(&module, method) {
                    *resolved_ret_out = ret.clone();
                }
                return ret;
            }
            if let Expr::Ident(alias, alias_span) = &**receiver {
                if let Some(module) = self.core_imports.get(alias).cloned() {
                    let ret = self.infer_core_call(&module, method, *alias_span, span, type_args, args);
                    // c109 Phase 20: write the resolved return type back onto the node
                    // for the polymorphic core specials whose type is arg-dependent and
                    // NOT in `core_fixed_sig` (so the TIR can read it totally — I3). The
                    // monomorphic calls (in `core_fixed_sig`) get their type from that
                    // table at lowering, so leave `resolved_ret = None` for them.
                    if is_polymorphic_core_special(&module, method) {
                        *resolved_ret_out = ret.clone();
                    }
                    return ret;
                }
                if let Some(&mod_idx) = self.imports.get(alias) {
                    self.record_import_alias_reference(alias, *alias_span);
                    return self.infer_import_call(mod_idx, method, *alias_span, span, type_args, args);
                }
                // D-MOD2: inline code module call — `math.double(x)` where `math` is an
                // inline `module math { … }` in this file. Resolve via mangled name.
                if let Some(canonical) = self.code_modules.get(alias.as_str()) {
                    let mangled = format!("{}__{}", canonical, method);
                    return self.infer_code_module_call(alias, &mangled, *alias_span, span, args);
                }
            }
            if let Expr::Ident(type_name, type_span) = &**receiver {
                // D-SERDE13=B: `Data.Text(x)` etc. — the retired spelling of the value
                // tree. Point at `DataTree` (no alias, I8) before generic resolution.
                if type_name == "Data" {
                    self.diags.push(data_renamed_to_datatree(*type_span));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return Some(json_ty());
                }
                // D-ENC-DYN1=A+: `DataTree`/`Json`/`Toml`/`Yaml`/`Csv` name the one dynamic
                // value; they are reserved core type names (a user type may not redefine them).
                if is_json_type_name(type_name) {
                    if let Some(ret) = self.check_core_json_lit(method, args, span) {
                        return Some(ret);
                    }
                }
                // D-DBDRIVER1: `DbValue.Int(n)` / `.Float(f)` / `.Text(s)` / `.Bool(b)` —
                // the tagged SQL parameter/column value construction (same mechanism as
                // `Data`/`Json` above). `DbValue.Null` (no args) is a `Field`, not a
                // `MethodCall` — handled in `infer_field` alongside `Data.Null`.
                if type_name == Syntax::TYPE_DB_VALUE {
                    if let Some(ret) = self.check_core_dbvalue_lit(method, args, span) {
                        return Some(ret);
                    }
                }
                {
                    let has_variant = self
                        .resolve_enum_variants_cloned(type_name)
                        .map(|v| v.contains_key(method))
                        .unwrap_or(false);
                    if has_variant {
                        let saved: Vec<Expr> = args
                            .iter_mut()
                            .map(|a| std::mem::replace(&mut a.expr, Expr::Int(0, a.span, None, None)))
                            .collect();
                        let mut enum_args: Vec<EnumLitArg> =
                            saved.into_iter().map(EnumLitArg::Positional).collect();
                        let ty = self.check_enum_lit(type_name, method, &mut enum_args, span);
                        for (a, ea) in args.iter_mut().zip(enum_args) {
                            if let EnumLitArg::Positional(e) = ea {
                                a.expr = e;
                            }
                        }
                        return Some(ty);
                    }
                }
                if ((type_name == "EncodingLimits" || type_name == "CBOROptions" || type_name == "XMLLimits" || type_name == "XMLParseOptions" || type_name == "Limits") && method == "safe")
                    || self.resolve_method_sig(type_name, method).is_some() {
                    return self.check_static_method(type_name, method, span, type_args, args);
                }
                // D-FIDELITY-API1=A: `core.perf.Perf` static API. `use core.perf as perf`
                // remains accepted as the existing module-alias path.
                if type_name == "Perf" && !self.registry.contains("Perf") {
                    return self.infer_core_call(
                        "core.perf",
                        method,
                        receiver.span(),
                        span,
                        type_args,
                        args,
                    );
                }
                // A value binding wins over an ambient built-in type spelling.
                // Keep this aligned with TIR's static-call shadow check so sema
                // never accepts code that lowering must reject.
                if self.lookup(type_name).is_none() {
                    if let Some(ty) = builtin_type_from_ident(type_name) {
                        if let Some(ret) =
                            Collections::builtin_method_return(&ty, method, args.len(), true)
                        {
                            return self
                                .finish_builtin_method(receiver, method, &ty, args, span, ret);
                        }
                    }
                }
                // D-SHAPE-CONVERT1=A: every numeric-backed distinct type,
                // including @UnitFamily members, gets the same destination-owned
                // conversion as its base (`UserId.from_int`, `Meter.from_float`).
                if self.registry.is_distinct(type_name) {
                    if let Some(base) = self.registry.distinct_base(type_name).cloned() {
                        let base_method = Syntax::conversion_method_for_source(&base.name());
                        if !base.is_numeric() && method == base_method {
                            if args.len() != 1 {
                                self.diags.push(Diagnostic::error(
                                    "E0104",
                                    format!("`{type_name}.{method}` takes one value, got {}", args.len()),
                                    "a distinct conversion wraps exactly one value of its base type".to_string(),
                                    format!("write `{type_name}.{method}(value)`"),
                                    Some(span),
                                ));
                                for arg in args.iter_mut() {
                                    self.infer(&mut arg.expr);
                                }
                                return None;
                            }
                            let old = self.expected_type.replace(base.clone());
                            let got = self.infer(&mut args[0].expr);
                            self.expected_type = old;
                            if got.as_ref().is_some_and(|got| got != &base) {
                                self.diags.push(Diagnostic::error(
                                    "E0108",
                                    format!("argument to `{type_name}.{method}` should be {}, not {}", base.name(), got.as_ref().unwrap().name()),
                                    "the source name fixes the conversion input type".to_string(),
                                    format!("pass a {} value", base.name()),
                                    Some(args[0].expr.span()),
                                ));
                            }
                            let ret = Type::Named(type_name.clone());
                            *resolved_ret_out = Some(ret.clone());
                            return Some(ret);
                        }
                        if let Some(source) = base
                            .is_numeric()
                            .then_some(method)
                            .and_then(Syntax::numeric_conversion_source)
                            .and_then(crate::AST::numeric_type_from_name)
                        {
                            if args.len() != 1 {
                                self.diags.push(Diagnostic::error(
                                    "E0104",
                                    format!("`{type_name}.{method}` takes one value, got {}", args.len()),
                                    "a distinct conversion wraps exactly one value of its base type".to_string(),
                                    format!("write `{type_name}.{method}(value)`"),
                                    Some(span),
                                ));
                                for arg in args.iter_mut() {
                                    self.infer(&mut arg.expr);
                                }
                                return None;
                            }
                            let old = self.expected_type.replace(source.clone());
                            let got = self.infer(&mut args[0].expr);
                            self.expected_type = old;
                            if got.as_ref().is_some_and(|got| got != &source) {
                                self.diags.push(Diagnostic::error(
                                    "E0108",
                                    format!("argument to `{type_name}.{method}` should be {}, not {}", source.name(), got.as_ref().unwrap().name()),
                                    "the source name fixes the conversion input type".to_string(),
                                    format!("pass a {} value", source.name()),
                                    Some(args[0].expr.span()),
                                ));
                            }
                            let target = Type::Named(type_name.clone());
                            let base_conversion = Collections::numeric_conversion_return(
                                &base,
                                method,
                                1,
                            )
                            .flatten()
                            .expect("numeric distinct conversion has a numeric base");
                            let converted_literal = match (&args[0].expr, &source, &base) {
                                (Expr::Int(n, literal_span, _, _), source, base)
                                    if source.is_integer() && base.is_integer() => {
                                    let fits_base = match base {
                                        Type::Int => true,
                                        Type::IntN { signed, bits } => {
                                            let (lo, hi) = crate::AST::int_range(*signed, *bits);
                                            i128::from(*n) >= lo && i128::from(*n) <= hi
                                        }
                                        _ => false,
                                    };
                                    fits_base.then_some((*n, *literal_span))
                                }
                                (Expr::Float(n, literal_span, _), source, base)
                                    if source.is_float() && base.is_integer() && n.is_finite() => {
                                    let truncated = n.trunc();
                                    let (lo, upper_exclusive) = match base {
                                        Type::Int => (i64::MIN as f64, -(i64::MIN as f64)),
                                        Type::IntN { signed, bits } => {
                                            let (lo, hi) = crate::AST::int_range(*signed, *bits);
                                            (lo as f64, hi as f64 + 1.0)
                                        }
                                        _ => unreachable!(),
                                    };
                                    (truncated >= lo && truncated < upper_exclusive)
                                        .then_some((truncated as i64, *literal_span))
                                }
                                _ => None,
                            };
                            let literal_in_range = self.registry.distinct_range(type_name).and_then(|(lo, hi)| {
                                converted_literal.map(|(n, literal_span)| {
                                    if n < lo || n > hi {
                                        self.diags.push(Diagnostic::error(
                                            "E0135",
                                            format!("`{n}` is outside `{type_name}`'s range {lo}..{hi}"),
                                            format!("a range type only holds values inside its bounds; `{n}` can never be a `{type_name}`"),
                                            format!("use a value in `{lo}..{hi}`, or widen the type's range"),
                                            Some(literal_span),
                                        ));
                                    }
                                })
                            }).is_some();
                            let ret = if matches!(base_conversion, Type::Result { .. })
                                || (!literal_in_range
                                    && self.registry.distinct_range(type_name).is_some()) {
                                Type::Result {
                                    ok: Box::new(target),
                                    err: Box::new(Type::String),
                                }
                            } else {
                                target
                            };
                            *resolved_ret_out = Some(ret.clone());
                            return Some(ret);
                        }
                    }
                }
                // D-COLLBREADTH1=A: `Set.from([...])` → `Set<T>`.
                // T is inferred from the list argument's element type.
                if type_name == "Set" && method == "from" && args.len() == 1 {
                    let arg_ty = self.infer(&mut args[0].expr);
                    let elem_ty = match arg_ty {
                        Some(Type::List(inner)) => *inner,
                        _ => Type::Int,
                    };
                    if !Collections::is_hashable_type(&elem_ty) {
                        self.diags.push(Diagnostic::error(
                            "E0506",
                            format!("`Set<{}>` is not valid — `{}` is not hashable", elem_ty.name(), elem_ty.name()),
                            "Set elements must implement Hash and Eq; use Int, Bool, String, Char, or a named type".to_string(),
                            format!("change the element type to a hashable type, or use a `[{}]` list instead", elem_ty.name()),
                            Some(span),
                        ));
                    }
                    return Some(Type::Apply {
                        name: "Set".to_string(),
                        args: vec![elem_ty],
                    });
                }
                // D-ITERTOOLS1=A: `SortedSet.from([...])` → `SortedSet<T>`.
                if type_name == Syntax::TYPE_SORTED_SET && method == "from" && args.len() == 1 {
                    let arg_ty = self.infer(&mut args[0].expr);
                    let elem_ty = match arg_ty {
                        Some(Type::List(inner)) => *inner,
                        _ => Type::Int,
                    };
                    return Some(Type::Apply {
                        name: Syntax::TYPE_SORTED_SET.to_string(),
                        args: vec![elem_ty],
                    });
                }
                if type_name == Syntax::TYPE_SORTED_SET && method == "new" && args.is_empty() {
                    let elem_ty = match &self.expected_type {
                        Some(Type::Apply { name, args, .. })
                            if name == Syntax::TYPE_SORTED_SET && !args.is_empty() =>
                        {
                            args[0].clone()
                        }
                        _ => Type::Int,
                    };
                    let ret = Type::Apply {
                        name: Syntax::TYPE_SORTED_SET.to_string(),
                        args: vec![elem_ty],
                    };
                    *resolved_ret_out = Some(ret.clone());
                    return Some(ret);
                }
                // D-ITERTOOLS1=A: `PriorityQueue.from([...])` / `.new()`.
                if type_name == Syntax::TYPE_PRIORITY_QUEUE && method == "from" && args.len() == 1 {
                    let arg_ty = self.infer(&mut args[0].expr);
                    let elem_ty = match arg_ty {
                        Some(Type::List(inner)) => *inner,
                        _ => Type::Int,
                    };
                    return Some(Type::Apply {
                        name: Syntax::TYPE_PRIORITY_QUEUE.to_string(),
                        args: vec![elem_ty],
                    });
                }
                if type_name == Syntax::TYPE_PRIORITY_QUEUE && method == "new" && args.is_empty() {
                    let elem_ty = match &self.expected_type {
                        Some(Type::Apply { name, args, .. })
                            if name == Syntax::TYPE_PRIORITY_QUEUE && !args.is_empty() =>
                        {
                            args[0].clone()
                        }
                        _ => Type::Int,
                    };
                    let ret = Type::Apply {
                        name: Syntax::TYPE_PRIORITY_QUEUE.to_string(),
                        args: vec![elem_ty],
                    };
                    *resolved_ret_out = Some(ret.clone());
                    return Some(ret);
                }
                // D-ITERTOOLS1=A: `Lru<K,V>.new(capacity)`, expected type supplies K/V.
                if type_name == Syntax::TYPE_LRU && method == "new" && args.len() == 1 {
                    self.expect_core_arg("new", 0, &Type::Int, &mut args[0]);
                    let (key_ty, value_ty) = match &self.expected_type {
                        Some(Type::Apply { name, args, .. })
                            if name == Syntax::TYPE_LRU && args.len() >= 2 =>
                        {
                            (args[0].clone(), args[1].clone())
                        }
                        _ => (Type::String, Type::Int),
                    };
                    let ret = Type::Apply {
                        name: Syntax::TYPE_LRU.to_string(),
                        args: vec![key_ty, value_ty],
                    };
                    *resolved_ret_out = Some(ret.clone());
                    return Some(ret);
                }
                if type_name == Syntax::TYPE_BIT_SET && method == "new" && args.is_empty() {
                    return Some(Type::Named(Syntax::TYPE_BIT_SET.to_string()));
                }
                if type_name == Syntax::TYPE_BYTE_BUFFER && method == "new" && args.is_empty() {
                    return Some(Type::Named(Syntax::TYPE_BYTE_BUFFER.to_string()));
                }
                if type_name == Syntax::TYPE_BYTE_BUFFER && method == "from" && args.len() == 1 {
                    self.expect_core_arg(
                        "from",
                        0,
                        &Type::List(Box::new(Type::IntN {
                            signed: false,
                            bits: 8,
                        })),
                        &mut args[0],
                    );
                    return Some(Type::Named(Syntax::TYPE_BYTE_BUFFER.to_string()));
                }
                // D-COLLBREADTH1=A: `Deque.new()` → `Deque<T>`.
                // T is inferred from the type annotation's expected type.
                if type_name == "Deque" && method == "new" && args.is_empty() {
                    let elem_ty = match &self.expected_type {
                        Some(Type::Apply { name, args, .. }) if name == "Deque" && !args.is_empty() => {
                            args[0].clone()
                        }
                        _ => Type::Int,
                    };
                    let ret = Type::Apply {
                        name: "Deque".to_string(),
                        args: vec![elem_ty],
                    };
                    *resolved_ret_out = Some(ret.clone());
                    return Some(ret);
                }
                // D-TAG1: `Bag.new()` → `Bag<T>`. T is inferred from the annotation.
                if type_name == "Bag" && method == "new" && args.is_empty() {
                    let elem_ty = match &self.expected_type {
                        Some(Type::Apply { name, args, .. }) if name == "Bag" && !args.is_empty() => {
                            args[0].clone()
                        }
                        _ => Type::Int,
                    };
                    if !Collections::is_hashable_type(&elem_ty) {
                        self.diags.push(Diagnostic::error(
                            "E0506",
                            format!(
                                "`Bag<{}>` is not valid — `{}` is not hashable",
                                elem_ty.name(),
                                elem_ty.name()
                            ),
                            "Bag elements must implement Hash and Eq; use Int, Bool, String, Char, or a named type".to_string(),
                            format!(
                                "change the element type to a hashable type, or use a `[{}]` list instead",
                                elem_ty.name()
                            ),
                            Some(span),
                        ));
                    }
                    let ret = Type::Apply {
                        name: "Bag".to_string(),
                        args: vec![elem_ty],
                    };
                    *resolved_ret_out = Some(ret.clone());
                    return Some(ret);
                }
                // D-MEM1 S6 (D-SHARED-API1=A): `Shared.new(x)` — a lock-guarded shared
                // handle (`Arc<RwLock<T>>` class). `T` is inferred from the constructor
                // argument, no turbofish — a bare type-name call like `Path.from` above.
                if type_name == "Shared" && method == "new" {
                    self.record_memory_event(crate::Sema::MemoryEvent::new(
                        crate::Sema::MemoryEventKind::Allocation,
                        span,
                        "`Shared.new` allocates shared storage",
                    ));
                    self.record_memory_event(crate::Sema::MemoryEvent::new(
                        crate::Sema::MemoryEventKind::RetainRelease,
                        span,
                        "`Shared.new` introduces reference-counted ownership",
                    ));
                    if args.len() != 1 {
                        self.diags.push(Diagnostic::error(
                            "E0104",
                            format!("`Shared.new` takes exactly one value, got {}", args.len()),
                            "a `Shared<T>` wraps exactly one starting value".to_string(),
                            "write `Shared.new(value)`".to_string(),
                            Some(span),
                        ));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        return None;
                    }
                    let elem_ty = self.infer(&mut args[0].expr).unwrap_or(Type::Int);
                    let ret = Type::Shared(Box::new(elem_ty));
                    *resolved_ret_out = Some(ret.clone());
                    return Some(ret);
                }
                // D-MEM1 S6 (D-POOLID-API1=A): `Pool<T>.new()` — an empty generational
                // arena. `T` comes from the call-site turbofish (`Pool<Player>.new()`)
                // or, failing that, the binding's type annotation — same fallback shape
                // as `Deque.new()`/`Bag.new()` above.
                if type_name == "Pool" && method == "new" && args.is_empty() {
                    let elem_ty =
                        type_args
                            .first()
                            .cloned()
                            .unwrap_or_else(|| match &self.expected_type {
                                Some(Type::Apply { name, args, .. })
                                    if name == "Pool" && !args.is_empty() =>
                                {
                                    args[0].clone()
                                }
                                _ => Type::Int,
                            });
                    let ret = Type::Apply {
                        name: "Pool".to_string(),
                        args: vec![elem_ty],
                    };
                    *resolved_ret_out = Some(ret.clone());
                    return Some(ret);
                }
                // D-PATHFS1: `Path.from(str)` — typed path constructor.
                if type_name == "Path" && method == "from" && !self.registry.contains("Path") {
                    if args.len() != 1 {
                        self.diags.push(Diagnostic::error(
                            "E0104",
                            format!("`Path.from` takes one string argument, got {}", args.len()),
                            "`Path.from` constructs a typed path from a string".to_string(),
                            "write `Path.from(some_string)`".to_string(),
                            Some(span),
                        ));
                    }
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return Some(Type::Named("Path".to_string()));
                }
                // D-SHIFT1 (c7shift): `Reader.over(bytes)` — bare static
                // constructor over `[U8]`, same shape as `Path.from` above (a
                // reserved core type name, no import needed; a user's own
                // `Reader` type always wins — `!self.registry.contains`).
                // Argument checking for all `Reader`/`Cursor` positions goes
                // through `check_shift_arg` (E0112 fallback included —
                // `check_type_assignable` alone lets a plain mismatch through).
                if type_name == "Reader" && method == "over" && !self.registry.contains("Reader") {
                    if args.len() != 1 {
                        self.diags.push(Diagnostic::error(
                            "E0104",
                            format!(
                                "`Reader.over` takes one `[U8]` argument, got {}",
                                args.len()
                            ),
                            "`Reader.over` wraps a byte list in a consuming, fallible cursor"
                                .to_string(),
                            "write `Reader.over(some_bytes)`".to_string(),
                            Some(span),
                        ));
                    }
                    if let Some(arg) = args.first_mut() {
                        let want = Type::List(Box::new(u8_ty()));
                        self.check_shift_arg("Reader.over", &want, arg);
                    }
                    for a in args.iter_mut().skip(1) {
                        self.infer(&mut a.expr);
                    }
                    return Some(Type::Named("Reader".to_string()));
                }
                // D-SHIFT1 (c7shift): `Cursor.over(s)` — bare static constructor
                // over `String`, same shape as `Reader.over` above.
                if type_name == "Cursor" && method == "over" && !self.registry.contains("Cursor") {
                    if args.len() != 1 {
                        self.diags.push(Diagnostic::error(
                            "E0104",
                            format!(
                                "`Cursor.over` takes one `String` argument, got {}",
                                args.len()
                            ),
                            "`Cursor.over` wraps a string in a consuming, fallible text cursor"
                                .to_string(),
                            "write `Cursor.over(some_string)`".to_string(),
                            Some(span),
                        ));
                    }
                    if let Some(arg) = args.first_mut() {
                        self.check_shift_arg("Cursor.over", &Type::String, arg);
                    }
                    for a in args.iter_mut().skip(1) {
                        self.infer(&mut a.expr);
                    }
                    return Some(Type::Named("Cursor".to_string()));
                }
                // D-HOLE1: `Option.lift2(f, a, b)` — apply a two-argument function to
                // `a`/`b` only when both are present; `null` otherwise. A static
                // combinator (both optionals are plain arguments, not the receiver), so
                // it's resolved directly here, the same static-constructor shape as
                // `Set.from`/`Deque.new`/`Path.from` above.
                if type_name == "Option" && method == "lift2" && !self.registry.contains("Option") {
                    return self.check_option_lift2(args, span, resolved_ret_out);
                }
                // D-SIMD2 / D-LINALG1: a STATIC method on a built-in math type —
                // `F32x4.splat(x)` / `Vec3.from_array([…])`. The arg is elaborated
                // against the method's expected type (so a literal becomes the
                // component type / the `[T#N]` bridge).
                if is_math_type(type_name) && !self.registry.contains(type_name) {
                    if let Some(ret) = math_static_return(type_name, method, args.len()) {
                        if let Some(want) = math_static_arg_ty(type_name, method) {
                            if let Some(arg) = args.first_mut() {
                                let old = self.expected_type.replace(want.clone());
                                let got = self.infer(&mut arg.expr);
                                self.expected_type = old;
                                if let Some(g) = got {
                                    if g != want {
                                        self.diags.push(Diagnostic::error(
                                            "E0128",
                                            format!(
                                                "`{}.{}()` expects a `{}`, got `{}`",
                                                type_name,
                                                method,
                                                want.name(),
                                                g.name()
                                            ),
                                            format!(
                                                "`{}.{}` builds a `{}` from a `{}`",
                                                type_name,
                                                method,
                                                type_name,
                                                want.name()
                                            ),
                                            format!("pass a `{}` value", want.name()),
                                            Some(arg.expr.span()),
                                        ));
                                    }
                                }
                            }
                        }
                        return Some(ret);
                    }
                }
                // Built-in type identity does not depend on whether a particular
                // static method is registered. All valid builtin static shapes
                // returned above; an unknown one follows the ordinary E0102
                // method diagnostic instead of reinterpreting the type as a value.
                if self.lookup(type_name).is_none()
                    && builtin_type_from_ident(type_name).is_some()
                {
                    return self.check_static_method(type_name, method, span, type_args, args);
                }
            }
            self.borrow_ctx = true;
            // D-MEM1 stage S5: chaining `.trim()`/`.after()`/`.before()` onto a
            // string-view name is the one builtin-method shape its bare `&str`
            // Rust place supports (the `_view` prelude helpers take `&str`) — the
            // general `Expr::Ident` E2307 check must not fire for THIS receiver
            // read. Every other method name reaches the same `self.infer(receiver)`
            // call with the flag left false, so it fires normally.
            let recv_is_exempt_view = matches!(receiver.as_ref(), Expr::Ident(n, _)
                if self.is_string_view(n) && matches!(method, "trim" | "after" | "before"));
            if recv_is_exempt_view {
                self.allow_string_view_read = true;
            }
            let recv_ty = self.infer(receiver);
            if recv_is_exempt_view {
                self.allow_string_view_read = false;
            }
            let recv_ty = recv_ty?;
            if matches!(&recv_ty, Type::Apply { name, .. } if name == crate::Syntax::TYPE_EVENT)
                && matches!(method, "emit_async" | "queued_count")
            {
                let (what, fix) = if method == "emit_async" {
                    (
                        "`Event.emit_async` was retired",
                        "construct an `AsyncEvent<T, E>` with `event.async_result`, then call `.emit_async(payload)`",
                    )
                } else {
                    (
                        "`Event.queued_count` was retired",
                        "use `.queued_count()` on an `AsyncEvent<T, E>`",
                    )
                };
                self.diags.push(Diagnostic::error(
                    "E0102",
                    what.to_string(),
                    "synchronous `Event<T>` dispatches immediately; only `AsyncEvent<T, E>` owns a scheduler queue".to_string(),
                    fix.to_string(),
                    Some(span),
                ));
                for arg in args.iter_mut() { self.infer(&mut arg.expr); }
                return None;
            }
            match &recv_ty {
                Type::Apply { name, .. } if name == "Pool" && method == "add" => {
                    self.record_memory_event(crate::Sema::MemoryEvent::new(
                        crate::Sema::MemoryEventKind::ArenaBytes(None),
                        span,
                        "`Pool.add` has no proven byte bound",
                    ));
                }
                Type::Named(name)
                    if matches!(name.as_str(), "Arena" | "Bump")
                        && matches!(method, "new" | "alloc" | "alloc_slice") =>
                {
                    if method == "new" {
                        let bound =
                        args.first().and_then(|arg| match &arg.expr {
                            Expr::Int(value, _, _, _) if *value >= 0 => Some(*value as u64),
                            _ => None,
                        });
                        self.record_memory_event(crate::Sema::MemoryEvent::new(
                            crate::Sema::MemoryEventKind::ArenaBytes(bound),
                            span,
                            format!("`{name}.new` reserves bounded arena storage"),
                        ));
                    }
                    self.record_memory_event(crate::Sema::MemoryEvent::new(
                        crate::Sema::MemoryEventKind::Allocation,
                        span,
                        format!("`{name}.{method}` allocates from arena storage"),
                    ));
                }
                _ => {}
            }
            if crate::Sema::Diagnostics::is_secret_bearing_crypto_type(&recv_ty)
                && matches!(method, "clone" | "encode" | "hash" | "debug" | "display")
            {
                let shown = recv_ty.show().trim_matches('`').to_string();
                self.diags.push(Diagnostic::error(
                    "E0311",
                    format!("`{method}` is forbidden on secret-bearing `{shown}`"),
                    "secret-bearing cryptographic values are move-only and expose no clone, hash, serialization, Debug, or Display capability".to_string(),
                    "move the value to its next owner; use only a named `core.crypto.expert` exposure inside an audited `@Unsafe` region when raw bytes are required".to_string(),
                    Some(span),
                ));
                for arg in args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return None;
            }
            // D-SERDE2=A: Encode is one public protocol for hand and generated impls.
            if method == "encode" {
                if args.is_empty() && type_args.is_empty() && self.is_encodable(&recv_ty) {
                    *recv_type_out = Some("__SerdeEncode__".to_string());
                    return Some(Type::Named(Syntax::TYPE_DATA.to_string()));
                }
                if !self.is_encodable(&recv_ty) {
                    let shown = recv_ty.show();
                    let shown = shown.trim_matches('`');
                    self.diags.push(Diagnostic::error(
                        "E0905",
                        format!("`{shown}` does not implement `Encode`"),
                        "`.encode()` can only call the value's Encode contract".to_string(),
                        format!("derive `Encode` on `{shown}`, or write `impl {shown}.Encode`"),
                        Some(span),
                    ));
                    return None;
                }
            }
            // D-SERDE16=A: public, target-directed Decode dispatch from an ordinary
            // DataTree subtree. This is the spelling generated derives emit too.
            if matches!(&recv_ty, Type::Named(n) if Syntax::is_data_type_name(n))
                && method == Syntax::METHOD_DATATREE_DECODE
            {
                *recv_type_out = Some(Syntax::TYPE_DATA.to_string());
                if !args.is_empty() || type_args.len() != 1 {
                    self.diags.push(wrong_core_arity("DataTree.decode<T>", 0, args.len(), span));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
                let target = type_args[0].clone();
                if !self.is_decodable(&target) {
                    let shown = target.show();
                    let shown = shown.trim_matches('`');
                    self.diags.push(Diagnostic::error(
                        "E0905",
                        format!("`{shown}` does not implement `Decode`"),
                        format!("`DataTree.decode<{shown}>()` can only call the type's Decode contract"),
                        format!("derive `Decode` on `{shown}`, or write `impl {shown}.Decode`"),
                        Some(span),
                    ));
                }
                let ret = Type::Result {
                    ok: Box::new(target),
                    err: Box::new(Type::Named("DecodeError".to_string())),
                };
                *resolved_ret_out = Some(ret.clone());
                return Some(ret);
            }
            if let Type::Named(ref n) | Type::Apply { name: ref n, .. } = &recv_ty {
                if n == Syntax::TYPE_TASKGROUP {
                    return self.infer_taskgroup_method(receiver, method, span, args, recv_type_out);
                }
                if n == Syntax::TYPE_SELECT_BUILDER {
                    return self.infer_select_method(receiver, method, span, args, recv_type_out);
                }
                // D-TYPEDTEXT1=D: inspect a checked `Sql`/`Html` value. `.template()`/
                // `.params()` expose the bound-parameter split (Sql never re-embeds a
                // hole's text into the query string); `.text()` reads the escaped Html.
                if n == "Sql" && matches!(method, "template" | "params") {
                    if !args.is_empty() {
                        self.diags
                            .push(wrong_core_arity(method, 0, args.len(), span));
                    }
                    *recv_type_out = Some(n.clone());
                    return Some(if method == "template" {
                        Type::String
                    } else {
                        Type::List(Box::new(Type::String))
                    });
                }
                if n == "Html" && method == "text" {
                    if !args.is_empty() {
                        self.diags
                            .push(wrong_core_arity(method, 0, args.len(), span));
                    }
                    *recv_type_out = Some(n.clone());
                    return Some(Type::String);
                }
            }
            // D-ERRCTX1=D: `<fallible>.context("loading config {path}")` — a lazily-
            // evaluated human boundary message added to the error chain. Ordinary
            // method: arity/type errors go through the normal call-arity/type-mismatch
            // paths (no new diagnostic code), per the ratified text.
            if method == "context" {
                if let Type::Result { err, .. } = &recv_ty {
                    // A custom error type (`T ? MyError`) isn't the string-erased
                    // `Error` surface `.context()` targets — fall through so the
                    // normal "unknown method" path teaches the actual shape.
                    if matches!(err.as_ref(), Type::Named(n) if n == Syntax::TYPE_ERROR) {
                        if args.len() != 1 {
                            self.diags
                                .push(wrong_core_arity("context", 1, args.len(), span));
                            for a in args.iter_mut() {
                                self.infer(&mut a.expr);
                            }
                            return Some(recv_ty);
                        }
                        let saved = self.expected_type.clone();
                        self.expected_type = Some(Type::String);
                        let msg_ty = self.infer(&mut args[0].expr);
                        self.expected_type = saved;
                        if let Some(t) = msg_ty {
                            self.check_type_assignable(&Type::String, &t, args[0].expr.span());
                        }
                        // D-ERRCTX1=D: cheap "receiver is `Result<_, Error>`" signal for
                        // the TIR subset gate (mirrors how a named type's method sets
                        // `recv_type_out`) — codegen/subset re-derive the shape from this
                        // rather than re-inferring the receiver's full type.
                        *recv_type_out = Some("__Fallible__".to_string());
                        return Some(recv_ty);
                    }
                }
            }
            // E0964: length-changing methods are forbidden on a fixed-size [T#N].
            if let Type::FixedList { .. } = &recv_ty {
                if matches!(method, "push" | "pop" | "insert" | "remove" | "clear") {
                    self.diags.push(Diagnostic::error(
                        "E0964",
                        format!(
                            "`{}` changes a list's length, but this is a fixed-size {}",
                            method,
                            recv_ty.show()
                        ),
                        "the length of `[T#N]` is fixed at compile time and cannot change".to_string(),
                        "bind a growable list with `:=` (e.g. `r := [...]`) if you need to change its length".to_string(),
                        Some(span),
                    ));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
            }
            // D-TXN3/D-TXN4: `<handle>.on_commit(() => { … })` on a `@Transact`
            // transaction handle. Same shape as `scope.guard`: a zero-parameter
            // lambda, Drop-backed, run LIFO on a clean commit and dropped on a
            // `?`-failure/rollback. Returns a guard handle (`TransactionGuard`),
            // bound or discarded like a `scope.guard`. Sets `recv_type` so codegen
            // routes the node to the commit-guard lowering (I3).
            if let Type::Named(handle_ty) = &recv_ty {
                if handle_ty == crate::Syntax::TXN_HANDLE_TYPE && method == crate::Syntax::TXN_ON_COMMIT
                {
                    if args.len() != 1 {
                        self.diags.push(Diagnostic::error(
                            "E0104",
                            format!(
                                "`{}` takes one lambda, got {}",
                                crate::Syntax::TXN_ON_COMMIT,
                                args.len()
                            ),
                            "a post-commit hook registers a single cleanup lambda".to_string(),
                            format!(
                                "write `{}.{}(() => {{ … }})`",
                                "<handle>",
                                crate::Syntax::TXN_ON_COMMIT
                            ),
                            Some(span),
                        ));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        *recv_type_out = Some(handle_ty.clone());
                        return Some(Type::Named("TransactionGuard".to_string()));
                    }
                    let lam_ty = self.infer(&mut args[0].expr);
                    match &lam_ty {
                        Some(Type::Fn { params, .. }) => {
                            if !params.is_empty() {
                                self.diags.push(Diagnostic::error(
                                    "E0104",
                                    format!(
                                        "`{}` needs a zero-parameter lambda, got {} parameter{}",
                                        crate::Syntax::TXN_ON_COMMIT,
                                        params.len(),
                                        if params.len() == 1 { "" } else { "s" }
                                    ),
                                    "the hook body takes no arguments — it captures what it needs via closure".to_string(),
                                    format!("write `<handle>.{}(() => {{ … }})` with no parameters", crate::Syntax::TXN_ON_COMMIT),
                                    Some(args[0].expr.span()),
                                ));
                            }
                        }
                        Some(other) => {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!(
                                    "`{}` needs a lambda, not {}",
                                    crate::Syntax::TXN_ON_COMMIT,
                                    other.show()
                                ),
                                "a post-commit hook runs a lambda only after the transaction commits"
                                    .to_string(),
                                format!(
                                    "write `<handle>.{}(() => {{ … }})`",
                                    crate::Syntax::TXN_ON_COMMIT
                                ),
                                Some(args[0].expr.span()),
                            ));
                        }
                        None => {}
                    }
                    *recv_type_out = Some(handle_ty.clone());
                    return Some(Type::Named("TransactionGuard".to_string()));
                }
            }
            // D-TXN-ROLLBACK (layer 3): `<handle>.on_rollback(() => { … })` on a
            // `@Transact` handle — the exact mirror of `on_commit`. A zero-parameter
            // lambda, Drop-backed, run LIFO on a `?`-failure/rollback and dropped on a
            // clean commit. Returns the same `TransactionGuard` handle.
            if let Type::Named(handle_ty) = &recv_ty {
                if handle_ty == crate::Syntax::TXN_HANDLE_TYPE
                    && method == crate::Syntax::TXN_ON_ROLLBACK
                {
                    if args.len() != 1 {
                        self.diags.push(Diagnostic::error(
                            "E0104",
                            format!(
                                "`{}` takes one lambda, got {}",
                                crate::Syntax::TXN_ON_ROLLBACK,
                                args.len()
                            ),
                            "a rollback hook registers a single undo lambda".to_string(),
                            format!(
                                "write `{}.{}(() => {{ … }})`",
                                "<handle>",
                                crate::Syntax::TXN_ON_ROLLBACK
                            ),
                            Some(span),
                        ));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        *recv_type_out = Some(handle_ty.clone());
                        return Some(Type::Named("TransactionGuard".to_string()));
                    }
                    let lam_ty = self.infer(&mut args[0].expr);
                    match &lam_ty {
                        Some(Type::Fn { params, .. }) => {
                            if !params.is_empty() {
                                self.diags.push(Diagnostic::error(
                                    "E0104",
                                    format!(
                                        "`{}` needs a zero-parameter lambda, got {} parameter{}",
                                        crate::Syntax::TXN_ON_ROLLBACK,
                                        params.len(),
                                        if params.len() == 1 { "" } else { "s" }
                                    ),
                                    "the hook body takes no arguments — it captures what it needs via closure".to_string(),
                                    format!("write `<handle>.{}(() => {{ … }})` with no parameters", crate::Syntax::TXN_ON_ROLLBACK),
                                    Some(args[0].expr.span()),
                                ));
                            }
                        }
                        Some(other) => {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!(
                                    "`{}` needs a lambda, not {}",
                                    crate::Syntax::TXN_ON_ROLLBACK,
                                    other.show()
                                ),
                                "a rollback hook runs a lambda only when the transaction rolls back"
                                    .to_string(),
                                format!(
                                    "write `<handle>.{}(() => {{ … }})`",
                                    crate::Syntax::TXN_ON_ROLLBACK
                                ),
                                Some(args[0].expr.span()),
                            ));
                        }
                        None => {}
                    }
                    *recv_type_out = Some(handle_ty.clone());
                    return Some(Type::Named("TransactionGuard".to_string()));
                }
            }
            // D-DBDRIVER1: method calls on a `DbConnection` handle. A bespoke block
            // (like the `@Transact` handle above) rather than the generic
            // `file_handle_method_return` table, because `.query`/`.query_one`/
            // `.execute` need real expected-type-directed arg elaboration
            // (`sql: String, params: [DbValue]`) — an empty `[]` params literal must
            // resolve its element type from the parameter, not blind inference.
            if let Type::Named(handle_ty) = &recv_ty {
                if handle_ty == "DbConnection" {
                    if let Some(ret) = self.check_db_connection_method(method, args, span) {
                        *recv_type_out = Some(handle_ty.clone());
                        return ret;
                    }
                }
            }
            // D-DEP-WASM1=A / D-PLUGIN1=B (c81): method calls on a `Plugin` handle
            // — same bespoke-block shape as `DbConnection` above (`.call`/
            // `.call_int` need `(name: String, args: [T])` elaboration, not the
            // generic `file_handle_method_return` table).
            if let Type::Named(handle_ty) = &recv_ty {
                if handle_ty == "Plugin" {
                    if let Some(ret) = self.check_plugin_method(method, args, span) {
                        *recv_type_out = Some(handle_ty.clone());
                        return ret;
                    }
                }
            }
            if let Type::Named(handle_ty) = &recv_ty {
                let needs_edit = |checker: &mut Checker<'a>, api: &str| {
                    if let Some(root) = expr_root_ident(receiver) {
                        if let Some(info) = checker.lookup(root) {
                            if !info.mutable {
                                checker.diags.push(Diagnostic::error(
                                    "E0202",
                                    format!("`{api}` needs edit access to `{root}`"),
                                    "game scene setup changes durable scene state".to_string(),
                                    format!("declare `{root} := game.Scene.new(...)` before calling `{api}`"),
                                    Some(receiver.span()),
                                ));
                            }
                        }
                    }
                };
                match (handle_ty.as_str(), method) {
                    ("GameScene", "on_frame") => {
                        needs_edit(self, "on_frame");
                        if args.len() != 1 {
                            self.diags
                                .push(wrong_core_arity("on_frame", 1, args.len(), span));
                            for a in args.iter_mut() {
                                self.infer(&mut a.expr);
                            }
                        } else {
                            let expected_fn = Type::Fn {
                                params: vec![Type::Named("GameFrame".to_string())],
                                ret: None,
                                effect_bound: None,
                            };
                            let saved_esc = self.lambda_escapes;
                            let saved_exp = self.expected_type.clone();
                            self.lambda_escapes = true;
                            self.expected_type = Some(expected_fn);
                            self.infer(&mut args[0].expr);
                            self.expected_type = saved_exp;
                            self.lambda_escapes = saved_esc;
                        }
                        *recv_type_out = Some(handle_ty.clone());
                        return None;
                    }
                    ("GameScene", "component") => {
                        needs_edit(self, "component");
                        if args.is_empty() && type_args.len() == 1 {
                            args.push(Self::synthesized_string_arg(
                                Self::type_arg_name(&type_args[0]),
                                span,
                            ));
                        }
                        if args.len() != 1 {
                            self.diags
                                .push(wrong_core_arity("component", 1, args.len(), span));
                        }
                        if let Some(arg) = args.get_mut(0) {
                            self.expect_core_arg("component", 0, &Type::String, arg);
                        }
                        *recv_type_out = Some(handle_ty.clone());
                        return None;
                    }
                    ("GameScene", "query") => {
                        if args.is_empty() && !type_args.is_empty() {
                            let names = type_args
                                .iter()
                                .map(Self::type_arg_name)
                                .collect::<Vec<_>>()
                                .join(",");
                            args.push(Self::synthesized_string_arg(names, span));
                        }
                        if args.len() != 1 {
                            self.diags
                                .push(wrong_core_arity("query", 1, args.len(), span));
                        }
                        if let Some(arg) = args.get_mut(0) {
                            self.expect_core_arg("query", 0, &Type::String, arg);
                        }
                        *recv_type_out = Some(handle_ty.clone());
                        return Some(Type::List(Box::new(Type::String)));
                    }
                    ("GameAssets", "image" | "sound") => {
                        needs_edit(self, method);
                        if args.len() != 1 {
                            self.diags
                                .push(wrong_core_arity(method, 1, args.len(), span));
                        }
                        if let Some(arg) = args.get_mut(0) {
                            self.expect_core_arg(method, 0, &Type::String, arg);
                        }
                        *recv_type_out = Some(handle_ty.clone());
                        let ok = if method == "image" {
                            "GameImage"
                        } else {
                            "GameSound"
                        };
                        return Some(result_ty(Type::Named(ok.to_string()), Type::String));
                    }
                    ("GameInputMap", "bind") => {
                        needs_edit(self, "input.bind");
                        if args.len() != 2 {
                            self.diags
                                .push(wrong_core_arity("bind", 2, args.len(), span));
                        }
                        for i in 0..2 {
                            if let Some(arg) = args.get_mut(i) {
                                self.expect_core_arg("bind", i, &Type::String, arg);
                            }
                        }
                        *recv_type_out = Some(handle_ty.clone());
                        return None;
                    }
                    ("GameInputSnapshot", "pressed") => {
                        if args.len() != 1 {
                            self.diags
                                .push(wrong_core_arity("pressed", 1, args.len(), span));
                        }
                        if let Some(arg) = args.get_mut(0) {
                            self.expect_core_arg("pressed", 0, &Type::String, arg);
                        }
                        *recv_type_out = Some(handle_ty.clone());
                        return Some(Type::Bool);
                    }
                    _ => {}
                }
            }
            // D-ENCSTREAM-SURFACE1=A: methods on opaque codec handles.
            if let Type::Named(handle_ty) = &recv_ty {
                if let Some(ret) = encoding_handle_method_return(handle_ty, method, args.len()) {
                    if handle_ty == "JSONWriter" && method == "write" {
                        if let Some(arg) = args.first_mut() {
                            self.expect_core_arg(
                                "write",
                                0,
                                &Type::Named("DataEvent".to_string()),
                                arg,
                            );
                        }
                    } else if handle_ty == "JSONLWriter" && method == "write" {
                        if let Some(arg) = args.first_mut() {
                            self.expect_core_arg(
                                "write",
                                0,
                                &Type::Named("DataTree".to_string()),
                                arg,
                            );
                        }
                    } else if handle_ty == "CSVWriter" && method == "write" {
                        if let Some(arg) = args.first_mut() {
                            self.expect_core_arg("write", 0, &Type::List(Box::new(Type::String)), arg);
                        }
                    } else if handle_ty == "CBORWriter" && method == "write" {
                        if let Some(arg) = args.first_mut() { self.expect_core_arg("write", 0, &Type::Named("DataEvent".to_string()), arg); }
                    } else {
                        for a in args.iter_mut() { self.infer(&mut a.expr); }
                    }
                    *recv_type_out = Some(handle_ty.clone());
                    return ret;
                }
            }
            // E2-M7: method calls on streaming file handles (D-IO2).
            if let Type::Named(handle_ty) = &recv_ty {
                if let Some(ret) =
                    file_handle_method_return(handle_ty, method, args.len(), span, &mut self.diags)
                {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    *recv_type_out = Some(handle_ty.clone());
                    return ret;
                }
            }
            // E2-M10: method calls on net/http opaque types.
            if let Type::Named(handle_ty) = &recv_ty {
                if let Some(ret) =
                    net_method_return(handle_ty, method, args.len(), span, &mut self.diags)
                {
                    self.check_http_route_constant(handle_ty, method, args);
                    require_net_method_labels(handle_ty, method, args, span, &mut self.diags);
                    if handle_ty == "TlsClientConfig" && method == "with_alpn" {
                        if let Some(arg) = args.first_mut() {
                            self.expect_core_arg(
                                "with_alpn",
                                0,
                                &Type::List(Box::new(Type::String)),
                                arg,
                            );
                        }
                    } else if handle_ty == "TlsClientConfig" && method == "with_trust" {
                        if let Some(arg) = args.first_mut() {
                            self.expect_core_arg("with_trust", 0, &Type::Named("TlsClientTrust".to_string()), arg);
                        }
                    } else if handle_ty == "TlsClientConfig" && method == "with_client_identity" {
                        if let Some(arg) = args.first_mut() {
                            self.expect_core_arg("with_client_identity", 0, &Type::Named("TlsClientIdentity".to_string()), arg);
                        }
                    } else if handle_ty == "TlsClientConfig" && method == "with_version_bounds" {
                        for (index, arg) in args.iter_mut().enumerate() {
                            self.expect_core_arg("with_version_bounds", index, &Type::Named("TlsVersion".to_string()), arg);
                        }
                    } else if handle_ty == "TlsStream" && method == "close_write" {
                        if let Some(arg) = args.first_mut() {
                            self.expect_core_arg("close_write", 0, &Type::Named("Duration".to_string()), arg);
                        }
                    } else {
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                    }
                    *recv_type_out = Some(handle_ty.clone());
                    return ret;
                }
            }
            // D-PATHFS1: method calls on `Path` typed handle.
            if let Type::Named(handle_ty) = &recv_ty {
                if handle_ty == "Path" {
                    if let Some(ret) =
                        path_method_return(handle_ty, method, args.len(), span, &mut self.diags)
                    {
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        *recv_type_out = Some("Path".to_string());
                        return ret;
                    }
                }
            }
            // D-SHIFT1 (c7shift): `cursor.take_pattern("…")` — argument-dependent
            // (the return shape comes from the pattern's holes), resolved
            // directly here, for the same reason `Arena.alloc` is above.
            if let Type::Named(handle_ty) = &recv_ty {
                if handle_ty == "Cursor" && method == Syntax::METHOD_TAKE_PATTERN {
                    *recv_type_out = Some("Cursor".to_string());
                    if args.len() != 1 {
                        self.diags
                            .push(wrong_core_arity(method, 1, args.len(), span));
                        return Some(result_ty(unit_ty(), Type::String));
                    }
                    let Expr::StrMatchLit(parts, lit_span) = &args[0].expr else {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`{}` needs a literal pattern string", Syntax::METHOD_TAKE_PATTERN),
                            "the pattern is matched at compile time, so it can't be a computed `String` value"
                                .to_string(),
                            "write the pattern directly: `take_pattern(\"literal-{hole}-pattern\")`"
                                .to_string(),
                            Some(args[0].expr.span()),
                        ));
                        return Some(result_ty(unit_ty(), Type::String));
                    };
                    let _ = lit_span;
                    let mut holes: Vec<(String, Type)> = Vec::new();
                    for part in parts {
                        if let crate::AST::StrMatchPart::Hole {
                            name,
                            ty,
                            span: hole_span,
                        } = part
                        {
                            let bound_ty = self.str_match_hole_type(name, ty, *hole_span);
                            holes.push((name.clone(), bound_ty));
                        }
                    }
                    let ok_ty = if holes.is_empty() {
                        unit_ty()
                    } else {
                        Type::Tuple(
                            holes
                                .iter()
                                .map(|(n, t)| (n.clone(), Box::new(t.clone())))
                                .collect(),
                        )
                    };
                    let out = result_ty(ok_ty, Type::String);
                    *resolved_ret_out = Some(out.clone());
                    return Some(out);
                }
            }
            // D-BINPAT1 (card #506 follow-up): `reader.take_pattern(b"…")` —
            // the byte-mode sibling of `Cursor.take_pattern` above. Same
            // reason it's resolved directly here rather than through
            // `binary_reader_method_return`'s generic table: the return shape
            // comes from the pattern's holes (I8: one `take_pattern` mechanism,
            // dispatched on the literal kind the parser already committed to).
            if let Type::Named(handle_ty) = &recv_ty {
                if handle_ty == "Reader" && method == Syntax::METHOD_TAKE_PATTERN {
                    *recv_type_out = Some("Reader".to_string());
                    if args.len() != 1 {
                        self.diags
                            .push(wrong_core_arity(method, 1, args.len(), span));
                        return Some(result_ty(unit_ty(), Type::String));
                    }
                    let Expr::BinMatchLit(parts, lit_span) = &args[0].expr else {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`{}` needs a literal binary pattern", Syntax::METHOD_TAKE_PATTERN),
                            "the pattern is matched at compile time, so it can't be a computed value"
                                .to_string(),
                            "write the pattern directly: `take_pattern(b\"literal-{hole:U8}-pattern\")`"
                                .to_string(),
                            Some(args[0].expr.span()),
                        ));
                        return Some(result_ty(unit_ty(), Type::String));
                    };
                    let _ = lit_span;
                    let holes = self.bin_match_hole_types(parts, span);
                    let ok_ty = if holes.is_empty() {
                        unit_ty()
                    } else {
                        Type::Tuple(
                            holes
                                .iter()
                                .map(|(n, t)| (n.clone(), Box::new(t.clone())))
                                .collect(),
                        )
                    };
                    let out = result_ty(ok_ty, Type::String);
                    *resolved_ret_out = Some(out.clone());
                    return Some(out);
                }
            }
            // D-SHIFT1 (c7shift): `binary.Reader` instance methods (every read
            // fallible — bounds miss is an ordinary `?` error, not a panic).
            // `take(n)` wants an `Int` — sized ints convert explicitly per S42
            // (`Int.from_u16(count)`), never implicitly.
            if let Type::Named(handle_ty) = &recv_ty {
                if let Some(ret) = binary_reader_method_return(handle_ty, method, args.len()) {
                    for a in args.iter_mut() {
                        if method == "take" {
                            self.check_shift_arg("Reader.take", &Type::Int, a);
                        } else {
                            self.infer(&mut a.expr);
                        }
                    }
                    *recv_type_out = Some(handle_ty.clone());
                    return ret;
                }
            }
            // D-SHIFT1: `text.Cursor` instance methods (excluding `take_pattern`,
            // handled above). `take_until(delim)` wants a `String`.
            if let Type::Named(handle_ty) = &recv_ty {
                if let Some(ret) = text_cursor_method_return(handle_ty, method, args.len()) {
                    for a in args.iter_mut() {
                        if method == "take_until" {
                            self.check_shift_arg("Cursor.take_until", &Type::String, a);
                        } else {
                            self.infer(&mut a.expr);
                        }
                    }
                    *recv_type_out = Some(handle_ty.clone());
                    return ret;
                }
            }
            // D-PENDING1=B: method calls on `Loadable<T,E>` handle.
            if let Some(ret) = loadable_method_return(&recv_ty, method, args.len()) {
                if method == "or_else" {
                    let Type::Apply { args: type_args, .. } = &recv_ty else {
                        unreachable!("loadable_method_return accepted a non-Loadable type")
                    };
                    self.expect_core_arg("Loadable.or_else", 0, &type_args[0], &mut args[0]);
                } else {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                }
                *recv_type_out = Some("Loadable".to_string());
                return ret;
            }
            // D-TTLVAL1=A: Expiring<T> / Rotting<T> — fallible get, reject force().
            if let Type::Apply { name, .. } = &recv_ty {
                if name == "Expiring" {
                    if method == "force" {
                        self.diags.push(Diagnostic::error(
                            "E0511",
                            "`Expiring.force` bypasses expiry checking".to_string(),
                            "TTL-wrapped values must use fallible `get(clock)` so expired access is handled explicitly (D-TTLVAL1)".to_string(),
                            "use `match item.get(clock) { .Ok(v) -> …; .Err(Expired) -> … }` instead".to_string(),
                            Some(span),
                        ));
                    }
                    if let Some(ret) = expiring_method_return(&recv_ty, method, args.len()) {
                        if method == "get" && args.len() == 1 {
                            self.expect_core_arg(
                                "get",
                                0,
                                &Type::Named(crate::Syntax::CLOCK_TYPE.to_string()),
                                &mut args[0],
                            );
                        }
                        *recv_type_out = Some("Expiring".to_string());
                        return ret;
                    }
                }
                if name == "Rotting" {
                    if method == "force" {
                        self.diags.push(Diagnostic::error(
                            "E0511",
                            "`Rotting.force` bypasses expiry checking".to_string(),
                            "rotting secrets must use fallible `get(clock)` so expired access zeroizes storage (D-TTLVAL1, I1)".to_string(),
                            "use `match secret.get(clock) { .Ok(v) -> …; .Err(Expired) -> … }` instead".to_string(),
                            Some(span),
                        ));
                    }
                    if let Some(ret) = rotting_method_return(&recv_ty, method, args.len()) {
                        if method == "get" && args.len() == 1 {
                            self.expect_core_arg(
                                "get",
                                0,
                                &Type::Named(crate::Syntax::CLOCK_TYPE.to_string()),
                                &mut args[0],
                            );
                        }
                        *recv_type_out = Some("Rotting".to_string());
                        return ret;
                    }
                }
            }
            // D-APPROX1=A: method calls on sketch data structures.
            if let Some(ret) = sketch_method_return(&recv_ty, method, args) {
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                *recv_type_out = Some(sketch_type_name(&recv_ty).unwrap_or("Sketch").to_string());
                return ret;
            }
            // D-NETDEP1=A / D-HTTPLIB1=A: method calls on HTTP types.
            if matches!(&recv_ty, Type::Named(name) if name == "HttpBody") {
                let error = Type::Named("HttpError".to_string());
                let result = |ok| Type::Result { ok: Box::new(ok), err: Box::new(error.clone()) };
                let special = match (method, args.len()) {
                    ("json", 1) => {
                        self.expect_core_arg("Body.json", 0, &Type::Int, &mut args[0]);
                        let target = type_args.first().cloned().or_else(|| {
                            match &self.expected_type {
                                Some(Type::Result { ok, .. }) => Some((**ok).clone()),
                                _ => None,
                            }
                        });
                        match target {
                            Some(target) => Some(result(target)),
                            None => {
                                self.diags.push(Diagnostic::error(
                                    "E0901",
                                    "`Body.json` needs a decode type".to_string(),
                                    "JSON bytes can decode to many types, so the target must be explicit".to_string(),
                                    "write `body.json<Type>(limit)`".to_string(),
                                    Some(span),
                                ));
                                Some(result(Type::Named("Unknown".to_string())))
                            }
                        }
                    }
                    ("copy_to", 2) => {
                        self.expect_core_arg_moving("Body.copy_to", 0, &Type::Named("FileWriter".to_string()), &mut args[0]);
                        args[0].convention = AccessConvention::Move;
                        self.expect_core_arg("Body.copy_to", 1, &Type::Int, &mut args[1]);
                        Some(result(Type::Int))
                    }
                    ("bytes" | "text", 1) => {
                        self.expect_core_arg(method, 0, &Type::Int, &mut args[0]);
                        None
                    }
                    ("chunks", 0) => {
                        *recv_type_out = Some("HttpBody".to_string());
                        return Some(Type::Named("HttpBodyChunks".to_string()));
                    }
                    ("chunks", 1) => {
                        self.expect_core_arg("Body.chunks", 0, &Type::Int, &mut args[0]);
                        None
                    }
                    _ => None,
                };
                if let Some(ret) = special {
                    *recv_type_out = Some("HttpBody".to_string());
                    return Some(ret);
                }
            }
            if let Some(ret) = http_type_method_return(&recv_ty, method, args) {
                if let Type::Named(name) = &recv_ty {
                    self.check_http_route_constant(name, method, args);
                }
                if matches!(&recv_ty, Type::Named(name) if name == "HttpMux")
                    && matches!(method, "get" | "post" | "put" | "delete" | "patch" | "head" | "options")
                    && args.len() == 2
                {
                    self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                    self.expect_core_arg(
                        method,
                        1,
                        &Type::Fn {
                            params: vec![Type::Named("HttpRequest".to_string())],
                            ret: Some(Box::new(Type::Result {
                                ok: Box::new(Type::Named("HttpResponse".to_string())),
                                err: Box::new(Type::Named("HttpError".to_string())),
                            })),
                            effect_bound: None,
                        },
                        &mut args[1],
                    );
                } else if matches!(&recv_ty, Type::Named(name) if name == "HttpClient") {
                    let want = match method {
                        "cookies" | "redirects" | "send" | "proxy" | "tls" => 1,
                        "protocols" => 3,
                        "timeouts" => 7,
                        "raw_encoding" => 0,
                        _ => args.len(),
                    };
                    if args.len() != want {
                        self.diags.push(wrong_core_arity(method, want, args.len(), span));
                    }
                    let expected = match method {
                        "cookies" | "protocols" => Some(Type::Bool),
                        "redirects" | "timeouts" => Some(Type::Int),
                        "send" => Some(Type::Named("HttpRequest".to_string())),
                        "proxy" => Some(Type::Named("HttpProxy".to_string())),
                        "tls" => Some(Type::Named("TlsClientConfig".to_string())),
                        _ => None,
                    };
                    for (index, arg) in args.iter_mut().enumerate() {
                        if let Some(expected) = &expected {
                            self.expect_core_arg(method, index, expected, arg);
                        } else {
                            self.infer(&mut arg.expr);
                        }
                    }
                } else {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                }
                *recv_type_out = Some(match &recv_ty {
                    Type::Named(n) => n.clone(),
                    _ => "HttpRequest".to_string(),
                });
                return ret;
            }
            // D-URL1=A: method calls on typed Url/Mime values.
            if let Some(ret) = url_mime_method_return(&recv_ty, method, args) {
                let recv_name = match &recv_ty {
                    Type::Named(n) => n.as_str(),
                    _ => "",
                };
                match (recv_name, method, args.len()) {
                    ("Url", "join", 1) | ("Mime", "param", 1) => {
                        self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                    }
                    ("Url", "set_query" | "add_query", 2) => {
                        self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                        self.expect_core_arg(method, 1, &Type::String, &mut args[1]);
                    }
                    _ => {
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                    }
                }
                *recv_type_out = Some(match &recv_ty {
                    Type::Named(n) => n.clone(),
                    _ => "Url".to_string(),
                });
                return ret;
            }
            // D-EMAIL-SMTP-SURFACE1=A: exact envelope access/replacement methods.
            if let Some(ret) = email_method_return(&recv_ty, method, args.len()) {
                if method == "with_envelope" {
                    self.expect_core_arg(method, 0, &Type::Named("Envelope".to_string()), &mut args[0]);
                } else if method == "send" {
                    self.expect_core_arg(method, 0, &Type::Named("Message".to_string()), &mut args[0]);
                }
                *recv_type_out = Some(match &recv_ty { Type::Named(name) => name.clone(), _ => "Message".to_string() });
                return ret;
            }
            // D-REGEXENGINE1=A: method calls on compiled Regex and Match values.
            if let Some(ret) = regex_method_return(&recv_ty, method, args) {
                let recv_name = match &recv_ty {
                    Type::Named(n) => n.as_str(),
                    _ => "",
                };
                match (recv_name, method, args.len()) {
                    ("Regex", "is_match" | "match" | "find" | "find_all" | "matches" | "split", 1) => {
                        self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                    }
                    ("Regex", "replace" | "replace_all", 2) => {
                        self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                        self.expect_core_arg(method, 1, &Type::String, &mut args[1]);
                    }
                    ("Regex", "split_limit", 2) => {
                        self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                        self.expect_core_arg(method, 1, &Type::Int, &mut args[1]);
                    }
                    ("Regex", "replace_all_with", 2) => {
                        self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                        let cb = Type::Fn {
                            params: vec![Type::Named("Match".to_string())],
                            ret: Some(Box::new(Type::String)),
                            effect_bound: None,
                        };
                        self.expect_core_arg(method, 1, &cb, &mut args[1]);
                    }
                    ("Match", "group" | "group_start" | "group_end", 1) => {
                        self.expect_core_arg(method, 0, &Type::Int, &mut args[0]);
                    }
                    ("Match", "name", 1) => {
                        self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                    }
                    _ => {
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                    }
                }
                *recv_type_out = Some(match &recv_ty {
                    Type::Named(n) => n.clone(),
                    _ => "Regex".to_string(),
                });
                return ret;
            }
            // D-TIMEDEPTH1=A: method calls on Date/DateTime.
            if let Some(ret) = civil_time_method_return(&recv_ty, method, args) {
                let recv_name = match &recv_ty {
                    Type::Named(n) => n.as_str(),
                    _ => "",
                };
                match (recv_name, method, args.len()) {
                    ("Date" | "LocalDate", "add_days" | "add_months", 1) => {
                        self.expect_core_arg(method, 0, &Type::Int, &mut args[0]);
                    }
                    ("Date" | "LocalDate", "diff_days", 1) => {
                        self.expect_core_arg(
                            method,
                            0,
                            &Type::Named("LocalDate".to_string()),
                            &mut args[0],
                        );
                    }
                    ("Date" | "LocalDate", "add_period", 1) => {
                        self.expect_core_arg(
                            method,
                            0,
                            &Type::Named("Period".to_string()),
                            &mut args[0],
                        );
                    }
                    ("Date" | "LocalDate", "truncate" | "format", 1) => {
                        self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                    }
                    ("DateTime", "plus_duration", 1) => {
                        self.expect_core_arg(
                            method,
                            0,
                            &Type::Named("Duration".to_string()),
                            &mut args[0],
                        );
                    }
                    ("DateTime", "in_zone", 1) => {
                        self.expect_core_arg(method, 0, &Type::Named("Zone".to_string()), &mut args[0]);
                    }
                    ("DateTime", "truncate" | "round" | "format", 1) => {
                        self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                    }
                    ("ZonedDateTime", "add_duration", 1) => {
                        self.expect_core_arg(
                            method,
                            0,
                            &Type::Named("Duration".to_string()),
                            &mut args[0],
                        );
                    }
                    ("ZonedDateTime", "add_period", 1) => {
                        self.expect_core_arg(
                            method,
                            0,
                            &Type::Named("Period".to_string()),
                            &mut args[0],
                        );
                    }
                    ("ZonedDateTime", "format", 1) => {
                        self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                    }
                    _ => {
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                    }
                }
                *recv_type_out = Some(match &recv_ty {
                    Type::Named(n) => n.clone(),
                    _ => "Date".to_string(),
                });
                return ret;
            }
            // D-ALLOC1/D-ALLOC-C/D-ALLOC-D (ratified 2026-06-19): method calls on
            // Arena/Bump/Pool/Fixed allocators. Reset invalidates live views;
            // universal `close(^allocator)` owns terminal release.
            if let Type::Named(handle_ty) = &recv_ty {
                let handle_ty_s = handle_ty.clone();
                if handle_ty_s == "Fixed" && matches!(method, "new" | "over") {
                    *recv_type_out = Some(handle_ty_s.clone());
                    if !self.allow_fixed_constructor {
                        self.diags.push(Diagnostic::error(
                            "E0103",
                            format!("`Fixed.{method}` must directly initialize a lexical binding"),
                            "the Fixed handle borrows inline storage whose lifetime and drop order are fixed at its declaration".to_string(),
                            format!("write `fixed :: mem.Fixed.{method}(…)`"),
                            Some(span),
                        ));
                    }
                    if args.len() != 1 {
                        self.diags.push(Diagnostic::error(
                            "E0103",
                            format!("`Fixed.{method}` takes exactly one argument"),
                            if method == "new" {
                                "the inline backing size must be a positive compile-time integer".to_string()
                            } else {
                                "the backing must be one mutable fixed-size byte array".to_string()
                            },
                            if method == "new" {
                                "write `fixed :: mem.Fixed.new(size: 4096)`".to_string()
                            } else {
                                "write `fixed :: mem.Fixed.over(&bytes)`".to_string()
                            },
                            Some(span),
                        ));
                        for arg in args.iter_mut() {
                            self.infer(&mut arg.expr);
                        }
                        return Some(Type::Named(handle_ty_s));
                    }
                    let inferred = self.infer(&mut args[0].expr);
                    if method == "new" {
                        let direct_size = match &args[0].expr {
                            Expr::Int(value, _, _, _) => Some(*value),
                            Expr::ComptimeSplice {
                                value: Some(CtValue::Int(value)),
                                ..
                            } => Some(*value),
                            Expr::Ident(name, _) => match self.current_ct_globals().get(name) {
                                Some(CtValue::Int(value)) => Some(*value),
                                _ => None,
                            },
                            _ => None,
                        };
                        let size = direct_size.or_else(|| {
                            let globals = self.current_ct_globals();
                            match crate::Comptime::evaluate_owned_with_imports_opts_collecting(
                                &args[0].expr,
                                self.ct_funcs,
                                self.ct_externs,
                                self.ct_base_dir,
                                &globals,
                                self.core_imports,
                                false,
                                0,
                            ) {
                                Ok((CtValue::Int(value), inputs)) => {
                                    self.ct_embed_inputs.extend(inputs);
                                    Some(value)
                                }
                                _ => None,
                            }
                        });
                        if let Some(size) = size.filter(|size| *size > 0) {
                            let arg_span = args[0].expr.span();
                            args[0].expr = Expr::Int(size, arg_span, None, None);
                        } else {
                            self.diags.push(Diagnostic::error(
                                "E0103",
                                "`Fixed.new` needs a positive compile-time byte size".to_string(),
                                "runtime-sized storage cannot become an inline fixed array in the current stack frame".to_string(),
                                "use a positive literal or comptime integer, e.g. `mem.Fixed.new(size: 4096)`".to_string(),
                                Some(args[0].expr.span()),
                            ));
                        }
                    } else {
                        let fixed_bytes = matches!(
                            inferred,
                            Some(Type::FixedList { elem, len, .. })
                                if len > 0 && matches!(*elem, Type::IntN { signed: false, bits: 8 })
                        );
                        let direct_mutable_buffer = args[0].convention == AccessConvention::Write
                            && matches!(&args[0].expr, Expr::Ident(..));
                        if !fixed_bytes || !direct_mutable_buffer {
                            self.diags.push(Diagnostic::error(
                                "E0103",
                                "`Fixed.over` needs one mutable fixed-size byte buffer".to_string(),
                                "the allocator exclusively borrows that exact inline buffer until the Fixed handle is closed".to_string(),
                                "bind `[Byte#N]` storage, then write `fixed :: mem.Fixed.over(&storage)`".to_string(),
                                Some(args[0].expr.span()),
                            ));
                        }
                    }
                    return Some(Type::Named(handle_ty_s));
                }
                if let Some(ret) =
                    alloc_method_return(&handle_ty_s, method, args, span, &mut self.diags)
                {
                    let recv_name = if let Expr::Ident(n, _) = &**receiver {
                        Some(n.clone())
                    } else {
                        None
                    };
                    // D-ALLOC2: `reset` invalidates every value previously
                    // allocated in this arena. Any view of it used afterward is
                    // E0632 (use-after-reset) — the runtime `&mut self`
                    // signatures would also reject, so Jet rejects first (I2).
                    if method == "reset" {
                        if let Some(ref name) = recv_name {
                            if handle_ty_s == "Fixed" {
                                self.check_owner_change(name, "be reset", span);
                            }
                            self.kill_views_of_arena(name, method, span);
                        }
                    }
                    *recv_type_out = Some(handle_ty_s.clone());
                    // For `alloc`, infer the argument and return its type.
                    if method == "alloc" {
                        if let Some(arg) = args.get_mut(0) {
                            let inferred = self.infer(&mut arg.expr);
                            return inferred;
                        }
                        return None;
                    }
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return ret;
                }
            }
            // D-ARGS1: method calls on ArgsSpec / ParsedArgs (builder and result types).
            if let Type::Named(handle_ty) = &recv_ty {
                if handle_ty == "ArgsSpec" {
                    if let Some(ret) =
                        args_spec_method_return(method, args.len(), span, &mut self.diags)
                    {
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        *recv_type_out = Some("ArgsSpec".to_string());
                        return ret;
                    }
                }
                if handle_ty == "ParsedArgs" {
                    if let Some(ret) =
                        parsed_args_method_return(method, args.len(), span, &mut self.diags)
                    {
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        *recv_type_out = Some("ParsedArgs".to_string());
                        return ret;
                    }
                }
                if handle_ty == "ProcessSpec" {
                    if let Some(ret) =
                        process_spec_method_return(method, args.len(), span, &mut self.diags)
                    {
                        if matches!(method, "run" | "spawn") {
                            self.record_effect(Effect::Exec.name(), span);
                        }
                        let stream_mode_ty = Type::Named("ProcessStreamMode".to_string());
                        match method {
                            "cwd" | "env_remove" if args.len() == 1 => {
                                self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                            }
                            "stdin" | "stdout" | "stderr" if args.len() == 1 => {
                                self.expect_core_arg(method, 0, &stream_mode_ty, &mut args[0]);
                            }
                            "env" if args.len() == 2 => {
                                self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                                self.expect_core_arg(method, 1, &Type::String, &mut args[1]);
                            }
                            "timeout" if args.len() == 1 => {
                                self.expect_core_arg(
                                    method,
                                    0,
                                    &Type::Named("Duration".to_string()),
                                    &mut args[0],
                                );
                            }
                            "output_limit" if args.len() == 1 => {
                                self.expect_core_arg(method, 0, &Type::Int, &mut args[0]);
                            }
                            _ => {
                                for a in args.iter_mut() {
                                    self.infer(&mut a.expr);
                                }
                            }
                        }
                        *recv_type_out = Some("ProcessSpec".to_string());
                        return ret;
                    }
                }
                if handle_ty == "ProcessChild" {
                    if let Some(ret) =
                        process_child_method_return(method, args.len(), span, &mut self.diags)
                    {
                        if method != "id" {
                            self.record_effect(Effect::Exec.name(), span);
                        }
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        *recv_type_out = Some("ProcessChild".to_string());
                        return ret;
                    }
                }
                // D-PROCESS1=A: `.write(text)` on the `child.stdin` writer handle.
                if handle_ty == "ProcessStdin" {
                    if let Some(ret) =
                        process_stdin_method_return(method, args.len(), span, &mut self.diags)
                    {
                        self.record_effect(Effect::Exec.name(), span);
                        if method == "write" && args.len() == 1 {
                            self.expect_core_arg(method, 0, &Type::String, &mut args[0]);
                        } else {
                            for a in args.iter_mut() {
                                self.infer(&mut a.expr);
                            }
                        }
                        *recv_type_out = Some("ProcessStdin".to_string());
                        return ret;
                    }
                }
                // D-PROCESS1=A: `.lines()` on `child.stdout`/`child.stderr` streaming
                // reader handles (loop-source-only — E2502 in bindings.rs).
                if handle_ty == "ProcessStdoutStream" || handle_ty == "ProcessStderrStream" {
                    if let Some(ret) =
                        process_stream_method_return(method, args.len(), span, &mut self.diags)
                    {
                        self.record_effect(Effect::Exec.name(), span);
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        *recv_type_out = Some(handle_ty.clone());
                        return ret;
                    }
                }
                // D-RENDERTGT2=A (c133 M1/M2): UI backend measure/layout/paint/on_event.
                if handle_ty == "NullBackend" || handle_ty == "TuiBackend" || handle_ty == "GtkBackend"
                {
                    if let Some(ret) =
                        ui_backend_method_return(handle_ty, method, args.len(), span, &mut self.diags)
                    {
                        // D-A11YGATE1=B (c134 Phase 6, E2931): duplicate accessible
                        // labels among an inline focus group's interactive nodes.
                        if method == "set_focus_group" {
                            self.check_a11y_focus_group_duplicates(args, span);
                        }
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        *recv_type_out = Some(handle_ty.to_string());
                        return ret;
                    }
                }
                // c-devserver (owner-directed 2026-07-01): DevServer builder
                // methods (.html/.port/.serve).
                if handle_ty == "DevServer" {
                    if let Some(ret) =
                        devserver_method_return(method, args.len(), span, &mut self.diags)
                    {
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        *recv_type_out = Some("DevServer".to_string());
                        return ret;
                    }
                }
            }
            // D-DET1: methods on the deterministic injected Clock/Rng capability (and
            // Stopwatch). Reading time/randomness THROUGH the handle is reproducible.
            // Set `recv_type_out` so codegen routes the call to the handle-method op
            // (TIR shape (h)) rather than failing the typed-IR subset check.
            if let Type::Named(handle_ty) = &recv_ty {
                // D-DET-CAPAPI: `rng.pick(list)` / `rng.shuffle(&list)` are GENERIC — the
                // element type comes from the `[T]` arg, mirroring the ambient
                // `random.pick`/`random.shuffle`. Resolve element-aware here (the
                // `builtin_method_return` table only carries Int placeholders).
                if handle_ty == crate::Syntax::RNG_TYPE
                    && matches!(method, "pick" | "weighted_pick" | "sample" | "shuffle")
                {
                    let handle_ty = handle_ty.clone();
                    let result = self.finish_rng_generic(receiver, method, args, span);
                    *recv_type_out = Some(handle_ty);
                    return result;
                }
                if matches!(
                    handle_ty.as_str(),
                    "Clock" | "Rng" | "Stopwatch" | "Duration" | "Solver"
                ) {
                    if let Some(ret) =
                        Collections::builtin_method_return(&recv_ty, method, args.len(), false)
                    {
                        let handle_ty = handle_ty.clone();
                        let result =
                            self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
                        *recv_type_out = Some(handle_ty);
                        return result;
                    }
                }
            }
            // D-REACT1=B: methods on the reactive handle types `Signal<T>`/`Derived<T>`
            // (`.get()`, `.set(v)`). Set `recv_type_out` to the handle name so codegen
            // routes the call to the reactive-method shape (keyed on recv_type, not the
            // bare method name — `get`/0 would otherwise alias a list `get`).
            if let Type::Apply { name, .. } = &recv_ty {
                if matches!(name.as_str(), "Signal" | "Derived" | "Computed") {
                    if let Some(ret) =
                        Collections::builtin_method_return(&recv_ty, method, args.len(), false)
                    {
                        let handle_ty = name.clone();
                        let result =
                            self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
                        *recv_type_out = Some(handle_ty);
                        return result;
                    }
                }
            }
            // D-EVENT1=D: methods on compiler-known Event<T>/Hook<T,R> values.
            // Set `recv_type_out` so TIR lowers these to the event runtime method
            // shape instead of the generic collection builtin fallback.
            if let Type::Apply { name, .. } = &recv_ty {
                if matches!(
                    name.as_str(),
                    crate::Syntax::TYPE_EVENT
                        | crate::Syntax::TYPE_ASYNC_EVENT
                        | crate::Syntax::TYPE_HOOK
                        | crate::Syntax::TYPE_DECISION_HOOK
                        | crate::Syntax::TYPE_DISPATCH_REPORT
                ) {
                    if let Some(ret) =
                        Collections::builtin_method_return(&recv_ty, method, args.len(), false)
                    {
                        let handle_ty = name.clone();
                        let result =
                            self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
                        *recv_type_out = Some(handle_ty);
                        return result;
                    }
                }
            }
            if let Type::Named(name) = &recv_ty {
                if matches!(
                    name.as_str(),
                    crate::Syntax::TYPE_EFFECT
                        | crate::Syntax::TYPE_SUBSCRIPTION
                        | crate::Syntax::TYPE_EVENT_SCOPE
                        | crate::Syntax::TYPE_EVENT_TRACE
                ) {
                    if let Some(ret) =
                        Collections::builtin_method_return(&recv_ty, method, args.len(), false)
                    {
                        let handle_ty = name.clone();
                        let result =
                            self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
                        *recv_type_out = Some(handle_ty);
                        return result;
                    }
                }
            }
            if let Type::Named(name) = &recv_ty {
                if matches!(
                    name.as_str(),
                    crate::Syntax::TYPE_WATCH_HANDLE | crate::Syntax::TYPE_WATCH_SET
                ) {
                    if let Some(ret) =
                        Collections::builtin_method_return(&recv_ty, method, args.len(), false)
                    {
                        let handle_ty = name.clone();
                        let result =
                            self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
                        *recv_type_out = Some(handle_ty);
                        return result;
                    }
                }
            }
            // D-HONESTNUM1=A: methods on `Measurement<Float>` (value ± uncertainty).
            // `.add/sub/mul/div(m)` → Measurement<Float>; `.value()/.uncertainty()` → Float.
            // Operator overloading is NOT extended here — I8 closed-family rule.
            if let Type::Apply { name, .. } = &recv_ty {
                if name == crate::Syntax::TYPE_MEASUREMENT {
                    let meas_ty = recv_ty.clone();
                    let ret = match (method, args.len()) {
                        ("add" | "sub" | "mul" | "div", 1) => {
                            let arg_ty = self.infer(&mut args[0].expr);
                            if let Some(got) = &arg_ty {
                                if got != &meas_ty {
                                    self.diags.push(Diagnostic::error(
                                        "E0128",
                                        format!(
                                            "`.{}()` expects a `Measurement<Float>`, got `{}`",
                                            method, got.name()
                                        ),
                                        "Measurement arithmetic requires both operands to have the same type".to_string(),
                                        "wrap the value with `M.from(value, uncertainty)` first".to_string(),
                                        Some(args[0].expr.span()),
                                    ));
                                }
                            }
                            Some(meas_ty.clone())
                        }
                        ("value" | "uncertainty", 0) => Some(Type::Float),
                        _ => None,
                    };
                    if let Some(r) = ret {
                        *recv_type_out = Some(crate::Syntax::TYPE_MEASUREMENT.to_string());
                        return Some(r);
                    }
                }
            }
            // D-MEM1 S6 (D-POOLID-API1=A / D-SHARED-API1=A): `Pool<T>.add/remove/ids`
            // and `Shared<T>.read/edit` set `recv_type_out` explicitly (unlike Task/
            // Sender/Receiver, whose method NAMES alone are globally unambiguous —
            // `add`/`remove`/`ids`/`read`/`edit` collide with Set/List/Map method
            // names, so codegen's subset gate needs `recv_type` to disambiguate the
            // receiver, the same reason Signal/Derived/Measurement set it above).
            // The actual dispatch (`finish_pool_add` etc.) already lives in
            // `finish_builtin_method`, reached the same way Signal/Derived reach it.
            if let Type::Apply { name, .. } = &recv_ty {
                if name == "Pool" {
                    if let Some(ret) =
                        Collections::builtin_method_return(&recv_ty, method, args.len(), false)
                    {
                        let result =
                            self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
                        *recv_type_out = Some("Pool".to_string());
                        return result;
                    }
                }
            }
            if let Type::Shared(_) = &recv_ty {
                if let Some(ret) =
                    Collections::builtin_method_return(&recv_ty, method, args.len(), false)
                {
                    let result =
                        self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
                    *recv_type_out = Some("Shared".to_string());
                    // D-STM1=A (card #506): a `Shared.edit` on the direct path of a
                    // `@Transact` block joins the block's atomic commit — the write is
                    // DEFERRED, so the call yields nothing (Unit), matching codegen's
                    // `edit_txn(…) -> ()`. Consuming it (`x :: h.edit(…)`) then fails as an
                    // ordinary type error against Unit. `txn_depth` is 0 inside a nested
                    // lambda (an `on_commit` hook / a spawned task), where the edit stays
                    // immediate and keeps its normal return — so this narrows exactly to
                    // the edits the STM plane actually defers (the codegen routes the same
                    // set, resetting `in_stm_transact` for lambda bodies).
                    if method == "edit" && self.txn_depth > 0 {
                        return Some(Type::Tuple(vec![]));
                    }
                    return result;
                }
            }
            // D-SIMD2 / D-LINALG1: methods on the built-in math value types
            // (`v.dot(w)`, `v.length()`, `v.sum()`, `v.reduce(@Max)`, `m.matmul(n)`).
            // Operator overloading on this closed family is blessed; named methods are
            // the rest of the surface. Set `recv_type_out` so codegen routes to the
            // math-method op (TIR handle-method path).
            if let Type::Named(math_ty) = &recv_ty {
                if is_math_type(math_ty) && !self.registry.contains(math_ty) {
                    let math_ty = math_ty.clone();
                    // `reduce(#Op)` — the sole arg is a reduce-op marker, validated here.
                    if method == "reduce" && is_simd_lane_type(&math_ty) {
                        if args.len() != 1 {
                            self.diags.push(Diagnostic::error(
                                "E2510",
                                format!("`reduce` takes one reduce-op marker, got {}", args.len()),
                                "a lane reduction names its operation with a marker".to_string(),
                                "write `v.reduce(@Add)`, `@Mul`, `@Min`, or `@Max`".to_string(),
                                Some(span),
                            ));
                        } else if let Expr::ReduceMarker(op, mspan) = &args[0].expr {
                            if !simd_reduce_markers().contains(&op.as_str()) {
                                self.diags.push(Diagnostic::error(
                                    "E2510",
                                    format!("`@{}` isn't a reduce operation", op),
                                    "a lane reduction folds the lanes with one of a fixed set of operations".to_string(),
                                    "use `@Add`, `@Mul`, `@Min`, or `@Max`".to_string(),
                                    Some(*mspan),
                                ));
                            }
                        } else {
                            self.diags.push(Diagnostic::error(
                                "E2510",
                                "`reduce` takes a reduce-op marker, not a value".to_string(),
                                "the operation is named with a marker so the fold is explicit"
                                    .to_string(),
                                "write `v.reduce(@Add)`, `@Mul`, `@Min`, or `@Max`".to_string(),
                                Some(args[0].expr.span()),
                            ));
                            self.infer(&mut args[0].expr);
                        }
                        *recv_type_out = Some(math_ty.clone());
                        return Some(math_scalar_ty(&math_ty));
                    }
                    if let Some(ret) = math_method_return(&math_ty, method, args.len()) {
                        // Check each arg against its expected type (so a literal
                        // elaborates), then return the method's result type.
                        for arg in args.iter_mut() {
                            let want = math_method_arg_ty(&math_ty, method);
                            let old = self.expected_type.take();
                            if let Some(w) = &want {
                                self.expected_type = Some(w.clone());
                            }
                            let got = self.infer(&mut arg.expr);
                            self.expected_type = old;
                            if let (Some(w), Some(g)) = (&want, &got) {
                                if g != w {
                                    self.diags.push(Diagnostic::error(
                                        "E0128",
                                        format!(
                                            "`.{}()` on `{}` expects a `{}`, got `{}`",
                                            method,
                                            math_ty,
                                            w.name(),
                                            g.name()
                                        ),
                                        format!(
                                            "`{}.{}(…)` operates on a `{}`",
                                            math_ty,
                                            method,
                                            w.name()
                                        ),
                                        format!("pass a `{}` value", w.name()),
                                        Some(arg.expr.span()),
                                    ));
                                }
                            }
                        }
                        *recv_type_out = Some(math_ty.clone());
                        return Some(ret);
                    }
                    // A math type but an unknown method — fall through to the generic
                    // "no such method" path below, which prints a teaching diagnostic.
                }
            }
            if let Type::Named(n) = &recv_ty {
                if let Some(param) = self.type_param_scope.iter().find(|p| p.name == *n) {
                    let bounds = param.bounds.clone();
                    for (trait_name, info) in &self.trait_reg.traits {
                        if let Some(msig) = info.methods.get(method) {
                            self.record_open_memory_dispatch(
                                span,
                                "generic trait dispatch has no sealed target set",
                            );
                            self.record_edge(
                                crate::Sema::effect_key(Some(trait_name), method),
                                span,
                            );
                            if !bounds.iter().any(|b| b == trait_name) {
                                self.diags.push(e0901(method, trait_name, span));
                            }
                            *recv_type_out = Some(n.clone());
                            for (arg, param) in args.iter_mut().zip(msig.params.iter().skip(1)) {
                                arg.convention = param.convention;
                                let old = self.expected_type.replace(param.ty.clone());
                                self.infer(&mut arg.expr);
                                self.expected_type = old;
                            }
                            let ret = msig.return_type.clone();
                            *resolved_ret_out = ret.clone();
                            return ret;
                        }
                    }
                }
            }
            // D-SOA1: a `[S]` of a `#layout(columnar)` struct is stored
            // struct-of-arrays. v1 supports the core list surface (construct,
            // index-read, field-read, `len`, `is_empty`, `push`, iterate); the rest
            // is deferred — reject the method with a clear message rather than
            // silently miscompiling on columnar storage.
            if let Type::List(inner) = &recv_ty {
                if let Type::Named(elem) = inner.as_ref() {
                    if self.registry.is_columnar(elem)
                        && !matches!(method, "len" | "is_empty" | "push")
                        && Collections::builtin_method_return(&recv_ty, method, args.len(), false)
                            .is_some()
                    {
                        self.diags.push(Diagnostic::error(
                            "E1108",
                            format!(
                                "`.{}()` isn't supported on a columnar list `{}` yet",
                                method,
                                recv_ty.show()
                            ),
                            "`@Layout(columnar)` lists support the core surface in v1: indexing, field access, `len`, `is_empty`, `push`, and iteration".to_string(),
                            format!(
                                "drop `@Layout(columnar)` from `{}` to use `.{}()`, or rewrite the loop with indexing",
                                elem, method
                            ),
                            Some(span),
                        ));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        return None;
                    }
                }
            }
            // D-SERDE-ACCESS=B: accessor methods on Data/Json/DataTree.
            if let Type::Named(ref tn) = recv_ty {
                if is_json_type_name(tn) {
                    if let Some(ret) = datatree_method_return(method, args.len()) {
                        let json_ret = match ret {
                            Type::Result { ok, err } => Type::Result {
                                ok: if matches!(*ok, Type::Named(ref n) if n == "DataTree") {
                                    Box::new(json_ty())
                                } else {
                                    ok
                                },
                                err,
                            },
                            other => other,
                        };
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        *recv_type_out = Some(tn.clone());
                        return Some(json_ret);
                    }
                }
                if tn == "DataTree" {
                    if let Some(ret) = datatree_method_return(method, args.len()) {
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        *recv_type_out = Some("DataTree".to_string());
                        return Some(ret);
                    }
                }
                // D-DBDRIVER1: accessor methods on `DbValue` (`.int()`/`.text()`/
                // `.bool()`/`.float()`/`.is_null()`) — read back a bound/column value.
                if is_db_value_type_name(tn) {
                    if let Some(ret) = db_value_method_return(method, args.len()) {
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        *recv_type_out = Some(tn.clone());
                        return Some(ret);
                    }
                }
                // D-ANY-JAI1 (c7jaiany §6): accessor methods on `reflect.of(x)`'s
                // `Value`/`Field` handles — same zero-arg-getter shape as `DbValue`
                // above. `Value`/`Field` are common enough words a user struct might
                // reuse them (`examples/features/memory/zerocopy.jet` already has its
                // own `struct Field`) — `!self.registry.contains(tn)` makes a
                // user-declared type of that name win, same principle as codegen's
                // `!self.type_names.contains(name)` guard on the Rust-type-name side.
                if is_reflect_type_name(tn) && !self.registry.contains(tn) {
                    if let Some(ret) = reflect_method_return(tn, method, args.len()) {
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        *recv_type_out = Some(tn.clone());
                        return Some(ret);
                    }
                }
                // D-LAYOUT1 / D-LAYOUT-GATES1: methods on the built-in
                // `LayoutHandle`/`Constraint` types (`form.h("label","width")`,
                // `form.value(v)`, `form.suggest(v, 90.0)`, `c.medium()`, …).
                // `.h`/`.v`/`suggest`'s var argument (index 0 for `.value`/
                // `.suggest`) accepts any axis type (`HVar`/`VVar`/`LengthVar`),
                // so it's checked with `is_layout_axis_type` instead of the
                // single-fixed-`Type` table `layout_method_arg_ty` covers.
                if is_layout_type(tn) {
                    if let Some(ret) = layout_method_return(tn, method, args.len()) {
                        for (i, arg) in args.iter_mut().enumerate() {
                            let axis_arg = (tn == "LayoutHandle"
                                && ((method == "value" && i == 0) || (method == "suggest" && i == 0)))
                                .then_some(());
                            let want = layout_method_arg_ty(method, i);
                            let old = self.expected_type.take();
                            if let Some(w) = &want {
                                self.expected_type = Some(w.clone());
                            }
                            let got = self.infer(&mut arg.expr);
                            self.expected_type = old;
                            if axis_arg.is_some() {
                                let ok = matches!(&got, Some(Type::Named(n)) if is_layout_axis_type(n));
                                if !ok {
                                    self.diags.push(Diagnostic::error(
                                        "E0128",
                                        format!(
                                            "`.{}()` on `{}` expects a layout value (`HVar`/`VVar`/`LengthVar`), got `{}`",
                                            method,
                                            tn,
                                            got.as_ref().map(|t| t.name()).unwrap_or_default()
                                        ),
                                        "layout variables come from `handle.h(box, anchor)` / `handle.v(box, anchor)`, or a `box.anchor` read inside a `layout { … }` block".to_string(),
                                        "pass a layout variable, not a plain value".to_string(),
                                        Some(arg.expr.span()),
                                    ));
                                }
                            } else if let (Some(w), Some(g)) = (&want, &got) {
                                if g != w {
                                    self.diags.push(Diagnostic::error(
                                        "E0128",
                                        format!(
                                            "`.{}()` on `{}` expects a `{}`, got `{}`",
                                            method,
                                            tn,
                                            w.name(),
                                            g.name()
                                        ),
                                        format!("`{}.{}(…)` operates on a `{}`", tn, method, w.name()),
                                        format!("pass a `{}` value", w.name()),
                                        Some(arg.expr.span()),
                                    ));
                                }
                            }
                        }
                        *recv_type_out = Some(tn.clone());
                        return Some(ret);
                    }
                }
            }
            // D-HOLE1: `.zip` on `T?` pairs two optionals into `(a: T, b: U)?` — present
            // only when both operands are present. `U` is independent of the receiver's
            // `T`, which doesn't fit `Collections::builtin_method_arg_types`'s
            // one-fixed-placeholder-type table (that table's `[T].zip([T])` entry works
            // around the same limitation by forcing the same element type instead);
            // handled directly here, the same shape as `finish_rng_generic`'s hand-rolled
            // generic dispatch below.
            if let Type::Option(a_inner) = &recv_ty {
                if method == "zip" {
                    let a_inner = (**a_inner).clone();
                    return self.finish_option_zip(a_inner, args, span, resolved_ret_out);
                }
            }
            if let Some(ret) = Collections::builtin_method_return(&recv_ty, method, args.len(), false) {
                let nominal_recv = match &recv_ty {
                    Type::Named(name) => Some(name.as_str()),
                    Type::Tagged { marker, inner }
                        if marker == crate::AST::CORE_CRYPTO_NOMINAL_MARKER =>
                    {
                        match inner.as_ref() {
                            Type::Named(name) => Some(name.as_str()),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some(name) = nominal_recv {
                    if matches!(name, "SigningKey" | "X25519SecretKey" | "VerifyKey" | "X25519PublicKey" | "Signature" | "Sealed" | "WrappedKey" | "WrappedVaultKey" | "Digest256" | "Digest512" | "PasswordHash") {
                        *recv_type_out = Some(name.to_string());
                    }
                }
                // D-NUMOPS1: hand codegen the receiver's numeric width so it picks the
                // same widening/narrowing form sema just chose for the return type.
                if recv_ty.is_numeric() {
                    *recv_type_out = Some(recv_ty.name());
                }
                // D-BIGINT1 / D-DECIMAL1: precise numeric handle methods.
                if matches!(
                    &recv_ty,
                    Type::Named(n)
                        if n == crate::Syntax::TYPE_BIGINT || n == crate::Syntax::TYPE_DECIMAL
                ) {
                    *recv_type_out = Some(recv_ty.name());
                }
                let declared_ret = ret.clone();
                let result =
                    self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
                // Preserve only exact facts the generic table could not express:
                // callback/argument-refined results, tuple shapes, and numeric
                // identities needed when a sequence is empty.
                if let Some(ref ty) = result {
                    let refinement_capable = matches!(
                        method,
                        "zip"
                            | "map"
                            | "reduce"
                            | "flat_map"
                            | "filter_map"
                            | "scan"
                            | "fold"
                            | "group_by"
                            | "count_by"
                            | "para_map"
                            | "para_fold"
                    );
                    if refinement_capable
                        || result != declared_ret
                        || contains_tuple_type(ty)
                        || matches!(method, "sum" | "product" | "min" | "max")
                    {
                        *resolved_ret_out = Some(ty.clone());
                    }
                }
                return result;
            }
            if recv_ty.is_numeric() {
                if let Some(target) = Syntax::retired_numeric_conversion_target(method) {
                    let source = recv_ty.name();
                    let from = Syntax::numeric_conversion_method(&source).unwrap_or("from_value");
                    self.diags.push(Diagnostic::error(
                        "E0311",
                        format!("source-owned conversion `.{method}()` is retired"),
                        "explicit conversion belongs to the destination type, so the promised result is visible first".to_string(),
                        format!("write `{target}.{from}(value)`"),
                        Some(span),
                    ));
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return None;
                }
            }
            if recv_ty == Type::String && method == "to_int" {
                self.diags.push(Diagnostic::error(
                    "E0311",
                    "source-owned text parsing `.to_int()` is retired".to_string(),
                    "text interpretation belongs to the destination type's `parse` operation".to_string(),
                    "write `Int.parse(text)`".to_string(),
                    Some(span),
                ));
                for arg in args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return None;
            }
            if let Type::TraitObject(trait_names) = &recv_ty {
                // D-ANY-JAI1: a multi-trait bound (`...[A, B]`) types its loop element as a
                // multi-name `TraitObject` — check EVERY bound trait for the method, not
                // just the first, so a call inside the body can reach a method on any of
                // them. First match wins (S48 single-trait dispatch is the n=1 case of
                // this loop, unchanged).
                let sig = trait_names.iter().find_map(|tn| {
                    self.trait_reg
                        .traits
                        .get(tn)
                        .and_then(|t| t.methods.get(method))
                        .map(|msig| (tn, msig))
                });
                if let Some((trait_name, msig)) = sig {
                    self.record_open_memory_dispatch(
                        span,
                        "trait-object dispatch has no sealed target set",
                    );
                    self.record_edge(
                        crate::Sema::effect_key(Some(trait_name), method),
                        span,
                    );
                    *recv_type_out = Some(trait_name.clone());
                    let ret = msig.return_type.clone();
                    for arg in args.iter_mut() {
                        self.infer(&mut arg.expr);
                    }
                    return ret;
                }
                // Keep the original single-trait wording byte-for-byte (it's snapshot-
                // pinned product copy, docs/spec/diagnostics.md) when there's only one
                // bound trait; only the multi-bound case needs the "none of" phrasing.
                let (headline, fix) = match trait_names.as_slice() {
                    [only] => (
                        format!("trait `{only}` has no method `{method}`"),
                        format!("add `fn {method}(…)` to `trait {only}`"),
                    ),
                    many => (
                        format!(
                            "none of {} has a method `{method}`",
                            many.iter()
                                .map(|t| format!("`{t}`"))
                                .collect::<Vec<_>>()
                                .join(" + ")
                        ),
                        format!("add `fn {method}(…)` to one of the bound traits"),
                    ),
                };
                self.diags.push(Diagnostic::error(
                    "E0102",
                    headline,
                    "check the method name on this trait value".to_string(),
                    fix,
                    Some(span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            }
            let type_name = match &recv_ty {
                Type::Named(n) | Type::Apply { name: n, .. } => n.clone(),
                Type::Option(inner) => match inner.as_ref() {
                    Type::Named(n) | Type::Apply { name: n, .. } => n.clone(),
                    _ => {
                        self.diags.push(Diagnostic::error(
                            "E0311",
                            format!("`{}` isn't a method on this value", method),
                            "instance methods belong to struct or enum values".to_string(),
                            format!(
                                "call it on the type: `{}.{method}(...)` if it's static",
                                recv_ty.name()
                            ),
                            Some(span),
                        ));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        return None;
                    }
                },
                _ => {
                    self.diags.push(Diagnostic::error(
                        "E0311",
                        format!("`{}` isn't a method on this value", method),
                        "only struct and enum values have instance methods".to_string(),
                        format!("check the spelling of `{}`", method),
                        Some(span),
                    ));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
            };
            if let Some(fields) = self.registry.struct_fields(&type_name) {
                if let Some((_, _, field_ty, _)) =
                    fields.iter().find(|(fname, _, _, _)| fname == method)
                {
                    if matches!(field_ty, Type::Fn { .. }) {
                        *recv_type_out = Some(type_name.clone());
                        let mut callee =
                            Box::new(Expr::Field(receiver.clone(), method.to_string(), span));
                        let end = args.last().map(|a| a.expr.span().end).unwrap_or(span.end);
                        let call_span = Span::new(span.start, end);
                        return self.infer_call_value(&mut callee, args, call_span);
                    }
                }
            }
            let Some((owner_mod, mut msig)) = self.resolve_method_sig(&type_name, method) else {
                self.diags.push(Diagnostic::error(
                    "E0102",
                    format!("`{}` has no method `{}`", type_name, method),
                    "check the method name on this type".to_string(),
                    format!("define it inside `struct {type_name}` or `impl {type_name}`"),
                    Some(span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            };
            if owner_mod != self.module_idx && !msig.is_pub {
                self.diags.push(crate::Sema::Diagnostics::private_item(method, span));
            } else if owner_mod != self.module_idx
                && Syntax::classify_identifier(method) == Syntax::IdentifierClass::SoftPublic {
                self.diags.push(crate::Sema::Diagnostics::soft_public_use(method, span));
            }
            let applied_args = match &recv_ty {
                Type::Apply { args, .. } => Some(args.as_slice()),
                Type::Option(inner) => match inner.as_ref() {
                    Type::Apply { args, .. } => Some(args.as_slice()),
                    _ => None,
                },
                _ => None,
            };
            if let Some(args) = applied_args {
                self.instantiate_method_sig(&type_name, &mut msig, args);
            }
            self.record_method_reference(&type_name, method, span);
            self.record_edge(crate::Sema::effect_key(Some(&type_name), method), span);
            if msig.is_static {
                self.diags.push(Diagnostic::error(
                    "E0311",
                    format!("`{}` is a static method on `{}`", method, type_name),
                    "static methods belong to the type name, not a value".to_string(),
                    format!("write `{}.{method}(...)` instead", type_name),
                    Some(span),
                ));
            }
            *recv_type_out = Some(type_name.clone());
            // `mut self` methods change the receiver: it must be changeable,
            // free of an active `for` borrow, and not aliased by an argument.
            if msig.self_conv == Some(AccessConvention::Write) {
                self.check_expr_change(
                    receiver,
                    &format!("be changed by `.{method}()`"),
                    span,
                );
                if let Some(root) = expr_root_ident(receiver) {
                    let root = root.to_string();
                    if self.iter_borrowed.contains(&root) {
                        self.diags.push(collection_changed_in_loop(&root, span));
                    }
                    if let Some(info) = self.lookup(&root) {
                        if !info.mutable {
                            let (what, fix) = if root == Syntax::KW_SELF {
                                (
                                    format!(
                                        "`.{}()` edits `{}`, but this method has read access only",
                                        method,
                                        Syntax::KW_SELF
                                    ),
                                    format!(
                                        "declare the enclosing method with `{}{}`",
                                        Syntax::SIGIL_WRITE,
                                        Syntax::KW_SELF
                                    ),
                                )
                            } else {
                                (
                                    format!(
                                        "cannot write to `{}` — it does not have edit access (`&`); required before calling `.{}()`",
                                        root,
                                        method
                                    ),
                                    format!("declare `{} {} ...`", root, Syntax::SIGIL_BIND_MUT),
                                )
                            };
                            self.diags.push(Diagnostic::error(
                                "E0202",
                                what,
                                "this method edits the value it's called on; write access (`&`) is required".to_string(),
                                fix,
                                Some(span),
                            ));
                        }
                    }
                    for arg in args.iter() {
                        if matches!(&arg.expr, Expr::Ident(n, _) if *n == root) {
                            self.diags.push(aliasing_while_mut(&root, arg.expr.span()));
                        }
                    }
                }
            }
            if msig.self_conv == Some(AccessConvention::Move) {
                if let Expr::Ident(n, nspan) = &**receiver {
                    // A borrowed parameter can't be consumed (the generated Rust
                    // would move out of a `&T`/`&mut T`).
                    if let Some(info) = self.lookup(n) {
                        if !type_is_copy(&info.ty)
                            && matches!(
                                info.param_conv,
                                Some(AccessConvention::Read) | Some(AccessConvention::Write)
                            )
                        {
                            self.diags.push(Diagnostic::error(
                                "E0120",
                                format!(
                                    "`{}` was not moved here, so `.{}()` cannot take it (`^`)",
                                    n, method
                                ),
                                "this function has read access only and does not own the value".to_string(),
                                format!(
                                    "call it on a copy: `({}{}).{}(...)` — or take ownership with `{}: {}{}`",
                                    Syntax::SIGIL_COPY,
                                    n,
                                    method,
                                    n,
                                    Syntax::SIGIL_MOVE,
                                    info.ty.name()
                                ),
                                Some(*nspan),
                            ));
                        }
                    }
                    self.mark_moved(n.clone(), *nspan);
                }
            }
            self.check_method_args(&type_name, method, &msig, args, span, None)?;
            let ret = msig.return_type.clone().map(|t| self.resolve_type(t));
            *resolved_ret_out = ret.clone();
            ret
        }
    
}
