use crate::AST::{AccessConvention, Type};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Checker;
use crate::Sema::Diagnostics::{is_displayable, is_printable, type_fix_hint, types_comparable};
use crate::Sema::Effects::{core_effect, e0746, is_irreversible_effect};
use crate::Sema::FFI::e3301;
use crate::Sema::Purity::{e3401, e3403, is_impure_core, is_nondeterministic_core};
use crate::Sema::SendCrossing;
use crate::Syntax;
use super::alloc_ptrs::{e3101, io_error_ty, ptr_elem, result_ty};
use super::core_types::{game_run_label_error, decode_error_ty, u8_ty, unit_ty};
use super::fixed_sigs::core_fixed_sig;
use super::serde_diags::{
    freestanding_hint, is_freestanding_forbidden, module_short_name, reactive_derived_unit,
    reactive_lambda_arity, reactive_not_lambda, unknown_core_item, wrong_core_arity,
};

fn is_string_literal_expr(expr: &crate::AST::Expr) -> bool {
    match expr {
        crate::AST::Expr::Str(..) => true,
        crate::AST::Expr::Paren(inner, _) => is_string_literal_expr(inner),
        _ => false,
    }
}

impl<'a> Checker<'a> {
        pub(crate) fn infer_core_call(
            &mut self,
            module: &str,
            name: &str,
            alias_span: Span,
            span: Span,
            type_args: &[Type],
            args: &mut [crate::AST::CallArg],
        ) -> Option<Type> {
            // D-EFF1: record the effect this Core call contributes to the enclosing
            // function's inferred set (erased in codegen; purely a sema fact).
            if let Some(e) = core_effect(module, name) {
                // D-EFFTREE1: Core calls (this module-call path) stay tagged with
                // a bare root — real stdlib call sites are unchanged (no migration
                // break: existing diagnostics naming `Fs`/`Db`/… keep their exact
                // wording). Leaf precision (`Fs.Read`, …) is otherwise a
                // user-declared-contract concept (a function's own `#(…)` bound,
                // D-PROP1-seeded into its `direct` set) — see Registration.rs /
                // Bundle.rs. The one exception is D-EFFDBREAD1=A: `core.db`'s own
                // closed connection-method table infers `Db.Read`/`Db.Write` leaves
                // (in `check_db_connection_method`, the method-call path — those
                // methods never reach this module-call `core_effect`).
                self.record_effect(e.name());
                // D-TXN2: an irreversible effect (Net/Fs/Exec — a network/file/
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
                return core_fixed_sig(module, name).and_then(|(_, ret)| ret);
            }
            // D-STDIN1=A / E3401: `pure fn` cannot read from stdin.
            if self.in_pure && self.det_suppress == 0 && is_impure_core(module, name) {
                let api = format!("{}.{}", module_short_name(module), name);
                self.diags
                    .push(e3401(&self.fn_name.clone(), &api, &[], span));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return core_fixed_sig(module, name).and_then(|(_, ret)| ret);
            }
            // D-EFF1: `@Pure` is the empty effect set, so any effectful Core call —
            // `Fs`/`Net`/`Env`/`Exec`/`Db`/`Log`/`Io` — is impure inside a `@Pure fn`.
            // (Time/Rand return early above via E3403; stdin via the E3401 check
            // above, so this catches the remaining effect-carrying Core modules.)
            if self.in_pure && self.det_suppress == 0 && core_effect(module, name).is_some() {
                let api = format!("{}.{}", module_short_name(module), name);
                self.diags
                    .push(e3401(&self.fn_name.clone(), &api, &[], span));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return core_fixed_sig(module, name).and_then(|(_, ret)| ret);
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
            let sig = core_fixed_sig(module, name);
            match (module, name) {
                ("core.encoding.cbor", "parse") => {
                    if !(1..=2).contains(&args.len()) {
                        self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                    }
                    if let Some(arg) = args.get_mut(0) { self.expect_core_arg(name, 0, &Type::List(Box::new(u8_ty())), arg); }
                    if let Some(arg) = args.get_mut(1) { self.expect_core_arg(name, 1, &Type::Named("CBOROptions".to_string()), arg); }
                    return Some(result_ty(Type::Named("DataTree".to_string()), Type::Named("CBORError".to_string())));
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
                    let t = type_args[0].clone();
                    self.check_decodable(&t, span);
                    return Some(result_ty(t, Type::Named("CBORError".to_string())));
                }
                ("core.encoding.json" | "core.encoding.jsonl" | "core.encoding.csv" | "core.encoding.cbor", "reader" | "writer")
                | ("core.encoding.xml", "reader") => {
                    let max = if (module == "core.encoding.json" && name == "writer")
                        || (module == "core.encoding.xml" && name == "reader")
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
                                "write `xml.reader(^file)`, `xml.reader(^file, limits)`, or `xml.reader(^file, limits, options)`".to_string()
                            } else if name == "reader" { format!("write `{}.reader(^file)` or `{}.reader(^file, limits)`", module_short_name(module), module_short_name(module)) } else if module == "core.encoding.json" { "write `json.writer(^file)`, `json.writer(^file, limits)`, or `json.writer(^file, limits, canonical)`".to_string() } else { format!("write `{}.writer(^file)` or `{}.writer(^file, limits)`", module_short_name(module), module_short_name(module)) },
                            Some(span),
                        ));
                    }
                    let Some((params, ret)) = &sig else { unreachable!() };
                    for (i, ((conv, param_ty), arg)) in params.iter().zip(args.iter_mut()).enumerate() {
                        if *conv == AccessConvention::Move {
                            if arg.convention != AccessConvention::Move {
                                self.diags.push(Diagnostic::error(
                                    "E0201",
                                    format!("argument {} to `{}` transfers ownership (`^`)", i + 1, name),
                                    "this standard library constructor retains the consumed handle".to_string(),
                                    format!("write `{}value` for this argument", Syntax::SIGIL_MOVE),
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
                    if let Some(scene) = args.get_mut(0) {
                        self.check_game_run_scene_edit(&scene.expr);
                        self.expect_core_arg("run", 0, &Type::Named("GameScene".to_string()), scene);
                    }
                    match args.len() {
                        1 => {}
                        2 => {
                            let label = args[1].label.as_ref().map(|(l, _)| l.clone());
                            match label.as_deref() {
                                Some("backend") => self.expect_core_arg(
                                    "run",
                                    1,
                                    &Type::Named("GameBackend".to_string()),
                                    &mut args[1],
                                ),
                                Some("replay") | None => self.expect_core_arg(
                                    "run",
                                    1,
                                    &Type::Named("GameReplay".to_string()),
                                    &mut args[1],
                                ),
                                Some(label) => {
                                    game_run_label_error(&mut self.diags, label, &args[1], 1, span);
                                    self.infer(&mut args[1].expr);
                                }
                            }
                        }
                        3 => {
                            let label1 = args[1].label.as_ref().map(|(l, _)| l.clone());
                            match label1.as_deref() {
                                Some("replay") | None => self.expect_core_arg(
                                    "run",
                                    1,
                                    &Type::Named("GameReplay".to_string()),
                                    &mut args[1],
                                ),
                                Some(label) => {
                                    game_run_label_error(&mut self.diags, label, &args[1], 1, span);
                                    self.infer(&mut args[1].expr);
                                }
                            }
                            let label2 = args[2].label.as_ref().map(|(l, _)| l.clone());
                            match label2.as_deref() {
                                Some("backend") | None => self.expect_core_arg(
                                    "run",
                                    2,
                                    &Type::Named("GameBackend".to_string()),
                                    &mut args[2],
                                ),
                                Some(label) => {
                                    game_run_label_error(&mut self.diags, label, &args[2], 2, span);
                                    self.infer(&mut args[2].expr);
                                }
                            }
                        }
                        _ => {
                            self.diags.push(Diagnostic::error(
                                "E0104",
                                format!("`game.run` expects 1 to 3 arguments, got {}", args.len()),
                                "`game.run` accepts a scene plus optional replay and backend handles"
                                    .to_string(),
                                "write `game.run(scene)`, `game.run(scene, replay: replay)`, or `game.run(scene, replay: replay, backend: backend)`".to_string(),
                                Some(span),
                            ));
                            for a in args.iter_mut().skip(1) {
                                self.infer(&mut a.expr);
                            }
                        }
                    }
                    return Some(Type::String);
                }
                // D-ENC1 / D-SERDE6: typed encode/decode over the Encode/Decode model.
                // `to_string`/`to_string_pretty` accept any encodable value (the dynamic
                // `Json` / `[[String]]` / `Map` forms AND a `@[Codable]` value); the
                // codegen routes by the lowered arg type. `decode<T>` is the typed decode
                // (→ `T`, or `[T]` for CSV) keyed by the call-site type argument.
                (
                    "core.encoding.json" | "core.encoding.csv" | "core.encoding.toml"
                    | "core.encoding.yaml",
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
                    let t = type_args[0].clone();
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
                    let t = type_args[0].clone();
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
                // D-DATA-SURFACE1=A: the beginner facade reuses typed CSV decoding,
                // then keeps table/stat selectors as ordinary typed Jet lambdas.
                ("core.data", "csv") if !type_args.is_empty() => {
                    if args.len() != 1 {
                        self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                    }
                    for a in args.iter_mut() {
                        self.expect_core_arg(name, 0, &Type::String, a);
                    }
                    let t = type_args[0].clone();
                    self.check_decodable(&t, span);
                    return Some(result_ty(Type::List(Box::new(t)), decode_error_ty()));
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
                        Type::Apply {
                            name: "Table".to_string(),
                            args: vec![elem],
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
                            effect_bound: None,
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
                            effect_bound: None,
                        };
                        self.expect_core_arg(name, 1, &fn_ty, fn_arg);
                    }
                    return Some(Type::List(Box::new(row_ty)));
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
                            effect_bound: None,
                        };
                        self.expect_core_arg(name, 1, &key_fn, key_arg);
                    }
                    if name != "group_count" {
                        if let Some(value_arg) = args.get_mut(2) {
                            let value_fn = Type::Fn {
                                params: vec![row_ty],
                                ret: Some(Box::new(Type::Float)),
                                effect_bound: None,
                            };
                            self.expect_core_arg(name, 2, &value_fn, value_arg);
                        }
                    }
                    return Some(Type::List(Box::new(Type::Named("DataGroup".to_string()))));
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
                            effect_bound: None,
                        };
                        self.expect_core_arg(name, 2, &key_fn, left_key);
                    }
                    if let Some(right_key) = args.get_mut(3) {
                        let key_fn = Type::Fn {
                            params: vec![right_row.clone()],
                            ret: Some(Box::new(Type::String)),
                            effect_bound: None,
                        };
                        self.expect_core_arg(name, 3, &key_fn, right_key);
                    }
                    let joined_right = if name == "left_join" {
                        Type::Option(Box::new(right_row))
                    } else {
                        right_row
                    };
                    return Some(Type::List(Box::new(Type::Apply {
                        name: "DataJoin".to_string(),
                        args: vec![left_row, joined_right],
                    })));
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
                                effect_bound: None,
                            };
                            self.expect_core_arg(name, idx, &key_fn, arg);
                        }
                    }
                    if let Some(value_arg) = args.get_mut(3) {
                        let value_fn = Type::Fn {
                            params: vec![row_ty],
                            ret: Some(Box::new(Type::Float)),
                            effect_bound: None,
                        };
                        self.expect_core_arg(name, 3, &value_fn, value_arg);
                    }
                    return Some(Type::List(Box::new(Type::Named("DataGroup".to_string()))));
                }
                ("core.mem", "volatile_read") => {
                    if !self.in_unsafe {
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
                    if !self.in_unsafe {
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
                        self.check_type_assignable(&elem, &value_ty, value_arg.expr.span());
                    }
                    return Some(unit_ty());
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
                ("core.io", "print") => {
                    // D-PRELUDEX1=A: qualified twin of ambient `print` for `#NoPrelude` files.
                    if args.len() != 1 {
                        self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                    }
                    if let Some(arg) = args.get_mut(0) {
                        self.borrow_ctx = true;
                        if let Some(ty) = self.infer(&mut arg.expr) {
                            if !is_printable(&ty, self.registry) {
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
                ("core.io", "eprint") => {
                    if args.len() != 1 {
                        self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                    }
                    if let Some(arg) = args.get_mut(0) {
                        self.borrow_ctx = true;
                        if let Some(ty) = self.infer(&mut arg.expr) {
                            if !is_printable(&ty, self.registry) {
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
                        if !is_displayable(&ty, self.trait_reg) {
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
                // D-FLOATW1 (ratified 2026-06-22): sqrt/floor/ceil/pow are width-generic —
                // they return the same float width they receive (Float→Float, F32→F32).
                // Mixing widths is a compile error; explicit .to_f32()/.to_f64() converts.
                (
                    "core.math",
                    "sqrt" | "floor" | "ceil" | "sin" | "cos" | "tan" | "asin" | "acos"
                    | "atan" | "sinh" | "cosh" | "tanh" | "exp" | "ln" | "log2" | "log10"
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
                ("core.math", "atan2" | "hypot" | "lerp") => {
                    if args.len() != if name == "lerp" { 3 } else { 2 } {
                        self.diags.push(wrong_core_arity(
                            name,
                            if name == "lerp" { 3 } else { 2 },
                            args.len(),
                            span,
                        ));
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
                    "checked_add" | "checked_sub" | "checked_mul" | "checked_pow",
                ) => {
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
                                format!("convert with `.to_f32()` or `.to_float()` to match"),
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
                            "write access (`&`) is required; the list must be passed with `&`"
                                .to_string(),
                            "write `random.shuffle(&xs)`".to_string(),
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
                ("core.tasks", "spawn") => {
                    if args.len() != 1 {
                        self.diags
                            .push(wrong_core_arity("spawn", 1, args.len(), span));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        return None;
                    }
                    let saved_esc = self.lambda_escapes;
                    let saved_task = self.is_task_spawn;
                    self.lambda_escapes = true;
                    self.is_task_spawn = true;
                    let lam_ty = self.infer(&mut args[0].expr);
                    self.lambda_escapes = saved_esc;
                    self.is_task_spawn = saved_task;
                    // Extract the return type from the closure's function type.
                    let t = match lam_ty {
                        Some(Type::Fn { params, ret, .. }) => {
                            if !params.is_empty() {
                                self.diags.push(Diagnostic::error(
                                    "E0104",
                                    format!(
                                        "`spawn` needs a zero-parameter lambda, got {} parameter{}",
                                        params.len(),
                                        if params.len() == 1 { "" } else { "s" }
                                    ),
                                    "a task starts by calling the lambda with no arguments"
                                        .to_string(),
                                    "move data into the task with `take(name)` instead of lambda parameters"
                                        .to_string(),
                                    Some(args[0].expr.span()),
                                ));
                            }
                            ret.map(|r| *r)
                                .unwrap_or_else(|| Type::Named("Unit".to_string()))
                        }
                        Some(other) => {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`spawn` needs a lambda, not {}", other.show()),
                                "a task starts by running a zero-parameter lambda".to_string(),
                                "write `tasks.spawn(() => work())`".to_string(),
                                Some(args[0].expr.span()),
                            ));
                            Type::Named("Unit".to_string())
                        }
                        None => Type::Named("Unit".to_string()),
                    };
                    if let Some(problem) = self.sendability_problem(&t, false) {
                        self.report_unsendable(
                            "task result",
                            &t,
                            problem,
                            SendCrossing::TaskResult,
                            args[0].expr.span(),
                        );
                    }
                    return Some(Type::Apply {
                        name: "Task".to_string(),
                        args: vec![t],
                    });
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
                // D-ROUTE1=A: jet.http.router() → HttpRouter.
                ("jet.http", "router") => {
                    if !args.is_empty() {
                        self.diags
                            .push(wrong_core_arity("router", 0, args.len(), span));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                    }
                    return Some(Type::Named("HttpRouter".to_string()));
                }
                // D-ROUTE1=A: http.parse(raw_string) → HttpRequest (parses HTTP/1.1 bytes).
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
                    return Some(Type::Named("HttpRequest".to_string()));
                }
                // D-ROUTE1=A: http.dispatch(router, req) → HttpResponse.
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
                        Some(Type::Named(n)) if n == "HttpRouter" => {}
                        Some(other) => {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`http.dispatch` needs an HttpRouter, not {}", other.show()),
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
                            Some(Type::Named(n)) if n == "HttpRequest" => {}
                            Some(other) => {
                                self.diags.push(Diagnostic::error(
                                    "E0112",
                                    format!(
                                        "`http.dispatch` needs an HttpRequest, not {}",
                                        other.show()
                                    ),
                                    "parse the raw request with `http.parse(raw)`".to_string(),
                                    "write `http.dispatch(router, req)` where `req` is an HttpRequest"
                                        .to_string(),
                                    Some(arg.expr.span()),
                                ));
                            }
                            _ => {}
                        }
                    }
                    return Some(Type::Named("HttpResponse".to_string()));
                }
                // E2-M10: jet.http.serve(addr, handler) — blocking accept loop.
                // handler: fn(HttpRequest) -> HttpResponse (lambda) or HttpRouter.
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
                    // Accept an HttpRouter or a callable (lambda/fn pointer).
                    let handler_ty = self.infer(&mut args[1].expr);
                    match &handler_ty {
                        Some(Type::Fn { .. }) => {}
                        Some(Type::Named(n)) if n == "HttpRouter" => {}
                        Some(other) => {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`http.serve` handler must be a function or HttpRouter, not {}", other.show()),
                                "the handler is called with each incoming `HttpRequest`".to_string(),
                                "pass a router (`http.router()`) or a lambda: `(req) => HttpResponse { … }`".to_string(),
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
                // D-REACT1=B: reactive.effect(() => { … }) runs the body now and again
                // whenever a signal it read changes. The body is a zero-parameter,
                // unit-returning closure; the call itself yields nothing.
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
                    return None; // effect returns nothing
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
                ("core.event", "policy_async") => {
                    if args.len() != 1 {
                        self.diags
                            .push(wrong_core_arity("policy_async", 1, args.len(), span));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        return None;
                    }
                    self.expect_core_arg("policy_async", 0, &Type::Int, &mut args[0]);
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
                // D-PENDING1=B: Loadable<T,E> constructors — idle/loading/loaded/failed.
                ("core.reactive.loadable", "idle") => {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return Some(Type::Apply {
                        name: "Loadable".to_string(),
                        args: vec![
                            Type::Named("Unknown".to_string()),
                            Type::Named("Unknown".to_string()),
                        ],
                    });
                }
                ("core.reactive.loadable", "loading") => {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return Some(Type::Apply {
                        name: "Loadable".to_string(),
                        args: vec![
                            Type::Named("Unknown".to_string()),
                            Type::Named("Unknown".to_string()),
                        ],
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
                        args: vec![val_ty, Type::Named("Unknown".to_string())],
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
                        args: vec![Type::Named("Unknown".to_string()), err_ty],
                    });
                }
                // D-TTLVAL1=A: Expiring<T> — value + TTL + injectable Clock.
                ("core.time.expiring", "new") => {
                    if args.len() != 3 {
                        self.diags
                            .push(wrong_core_arity("new", 3, args.len(), span));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        return None;
                    }
                    let val_ty = self
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
                        name: "Expiring".to_string(),
                        args: vec![val_ty],
                    });
                }
                // D-TTLVAL1=A: Rotting<T> — secret with TTL + zeroize on expiry.
                ("core.vault", "rotting_new") => {
                    if args.len() != 3 {
                        self.diags
                            .push(wrong_core_arity("rotting_new", 3, args.len(), span));
                        for a in args.iter_mut() {
                            self.infer(&mut a.expr);
                        }
                        return None;
                    }
                    let val_ty = self
                        .infer(&mut args[0].expr)
                        .unwrap_or(Type::Named("Unknown".to_string()));
                    self.expect_core_arg(
                        "rotting_new",
                        1,
                        &Type::Named(crate::Syntax::DURATION_TYPE.to_string()),
                        &mut args[1],
                    );
                    self.expect_core_arg(
                        "rotting_new",
                        2,
                        &Type::Named(crate::Syntax::CLOCK_TYPE.to_string()),
                        &mut args[2],
                    );
                    return Some(Type::Apply {
                        name: "Rotting".to_string(),
                        args: vec![val_ty],
                    });
                }
                // D-CRYPTOENV1=A: expert-only raw crypto — requires import + #Unsafe gate.
                ("core.crypto.expert", _) => {
                    let has_import = self
                        .core_imports
                        .values()
                        .any(|m| m == "core.crypto.expert");
                    if !has_import {
                        self.diags.push(Diagnostic::error(
                            "E0510",
                            format!("`core.crypto.expert.{name}` bypasses the misuse-resistant envelope"),
                            "raw AES/ChaCha primitives are expert-only and hide none of the footguns that `crypto.seal`/`open` prevent (D-CRYPTOENV1)".to_string(),
                            "use `core.crypto.seal` / `core.crypto.open` for encryption, or add `use core.crypto.expert` inside an audited `#Unsafe(\"reason\")` region".to_string(),
                            Some(span),
                        ));
                    } else if !self.in_unsafe {
                        self.diags.push(Diagnostic::error(
                            "E0510",
                            format!("`core.crypto.expert.{name}` requires an audited `#Unsafe` region"),
                            "raw crypto primitives may only run inside an explicit expert-tier gate (I1)".to_string(),
                            "wrap the call in `#Unsafe(\"crypto expert: …\") { … }` or use `crypto.seal`/`open` instead".to_string(),
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
                        ok: Box::new(Type::Named("HttpClientResp".to_string())),
                        err: Box::new(Type::String),
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
                        ok: Box::new(Type::Named("HttpClientResp".to_string())),
                        err: Box::new(Type::String),
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
                    return Some(Type::Named("HttpClientReq".to_string()));
                }
                ("core.http.server", "mux") => {
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return Some(Type::Named("HttpMux".to_string()));
                }
                ("core.http.server", "bind") => {
                    if args.len() != 2 {
                        self.diags.push(wrong_core_arity("bind", 2, args.len(), span));
                        for arg in args.iter_mut() { self.infer(&mut arg.expr); }
                        return None;
                    }
                    self.expect_core_arg("bind", 0, &Type::String, &mut args[0]);
                    self.expect_core_arg("bind", 1, &Type::Named("HttpMux".to_string()), &mut args[1]);
                    return Some(Type::Result {
                        ok: Box::new(Type::Named("HttpServer".to_string())),
                        err: Box::new(Type::String),
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
                    if args.len() == 3 {
                        match args[2].label.as_ref().map(|(label, span)| (label.as_str(), *span)) {
                            Some(("tls", _)) => {}
                            Some((label, label_span)) => self.diags.push(Diagnostic::error(
                                "E0125",
                                format!("`serve` has no option named `{label}` here"),
                                "the third HTTP server argument is a named TLS option, not a positional value".to_string(),
                                "write `tls: Server.tls(cert, key)`".to_string(),
                                Some(label_span),
                            )),
                            None => self.diags.push(Diagnostic::error(
                                "E0125",
                                "`serve` needs `tls:` before the third argument".to_string(),
                                "the label makes the transport switch explicit at the call site".to_string(),
                                "write `Server.serve(addr, mux, tls: Server.tls(cert, key))`".to_string(),
                                Some(args[2].span),
                            )),
                        }
                        self.expect_core_arg(
                            "serve",
                            2,
                            &Type::Named("HttpServerTls".to_string()),
                            &mut args[2],
                        );
                    }
                    return Some(Type::Result {
                        ok: Box::new(unit_ty()),
                        err: Box::new(Type::String),
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
                        err: Box::new(Type::String),
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
                        err: Box::new(Type::String),
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
                    return Some(Type::Named("HttpServerTls".to_string()));
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
                    return Some(Type::Named("HttpSrvResp".to_string()));
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
                    return Some(Type::Named("HttpSrvResp".to_string()));
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
                        ok: Box::new(Type::Named("HttpSrvResp".to_string())),
                        err: Box::new(Type::String),
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
                        &Type::Named("HttpSrvReq".to_string()),
                        &mut args[0],
                    );
                    self.expect_core_arg("static_file_range", 1, &Type::String, &mut args[1]);
                    self.expect_core_arg("static_file_range", 2, &Type::String, &mut args[2]);
                    return Some(Type::Result {
                        ok: Box::new(Type::Named("HttpSrvResp".to_string())),
                        err: Box::new(Type::String),
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
                        &Type::Named("HttpSrvReq".to_string()),
                        &mut args[0],
                    );
                    self.expect_core_arg("access_log", 1, &Type::Int, &mut args[1]);
                    return Some(Type::String);
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
                            self.expect_core_arg("display_width", 0, &Type::String, &mut args[0]);
                            let label = args[1].label.as_ref().map(|(l, _)| l.clone());
                            match label.as_deref() {
                                Some("policy") | None => self.expect_core_arg(
                                    "display_width",
                                    1,
                                    &Type::Named("TextWidth".to_string()),
                                    &mut args[1],
                                ),
                                Some(label) => {
                                    let label_span = args[1].label.as_ref().map(|(_, s)| *s).unwrap_or(span);
                                    self.diags.push(Diagnostic::error(
                                        "E0125",
                                        format!("`display_width` has no `{label}:` option at argument 2"),
                                        "this position accepts a `TextWidth` policy; labels document the positional shape and never reorder arguments".to_string(),
                                        "write `policy:` here, or drop the label".to_string(),
                                        Some(label_span),
                                    ));
                                    self.infer(&mut args[1].expr);
                                }
                            }
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

            // D-FFI-SH1=A: `process.run` gives its literal argument expected type
            // `Sh`, activating the shared typed-text rewrite. Keep the older explicit
            // argv value accepted as compatibility sugar over `process.cmd(argv).run()`;
            // both lower to the same argv-only primitive and neither invokes a shell.
            if module == "core.process" && name == "run" {
                let Some((_, ret)) = sig else { unreachable!() };
                if args.len() != 1 {
                    self.diags.push(wrong_core_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    let saved = self.expected_type.clone();
                    self.expected_type = is_string_literal_expr(&arg.expr)
                        .then(|| Type::Named(Syntax::TYPE_SH.to_string()));
                    let got = self.infer(&mut arg.expr);
                    self.expected_type = saved;
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

            let Some((params, ret)) = sig else {
                self.diags.push(unknown_core_item(module, name, span));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                let _ = alias_span;
                return None;
            };
            if module == "core.crypto.expert" && name == "x25519" {
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
                return ret;
            }
            if args.len() != params.len() {
                self.diags
                    .push(wrong_core_arity(name, params.len(), args.len(), span));
            }
            for (i, ((conv, param_ty), arg)) in params.iter().zip(args.iter_mut()).enumerate() {
                if *conv == AccessConvention::Move {
                    if arg.convention != AccessConvention::Move {
                        self.diags.push(Diagnostic::error(
                            "E0201",
                            format!("argument {} to `{}` transfers ownership (`^`)", i + 1, name),
                            "this standard library constructor retains the consumed handle".to_string(),
                            format!("write `{}value` for this argument", Syntax::SIGIL_MOVE),
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
                            "argument {} to `{}` requires write access (`&`)",
                            i + 1,
                            name
                        ),
                        "this standard library call edits that value in place".to_string(),
                        format!("write `{}value` for this argument", Syntax::SIGIL_WRITE),
                        Some(arg.span),
                    ));
                }
                self.expect_core_arg(name, i, param_ty, arg);
            }
            for arg in args.iter_mut().skip(params.len()) {
                self.infer(&mut arg.expr);
            }
            ret
        }
    
}
