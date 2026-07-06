//! Call/method dispatch for the interpreter: `eval_call`, `eval_method`,
//! `eval_fan_out`, `eval_require`, `eval_embed_file`. These are further
//! `impl Interp` methods; the struct and spine live in `interp.rs`.

use std::collections::HashMap;

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{CallArg, Expr, Func, LambdaBody, StrPart, Type, UnOp};

use super::Builtins::{
    apply_method, apply_mutating, apply_static_type_method, as_bool, as_int, cmp,
};
use super::Diagnostics::{comptime_panic, unsupported};
use super::Diagnostics::{EARLY_RETURN_CODE, ERR_PROPAGATE_CODE};
use super::Interpreter::{Flow, Interp};
use super::Value::CtValue;

/// D-CTIO1 (ratified 2026-06-22): a comptime embed path must be a string
/// literal that stays inside the project — never computed, never absolute, and
/// never escaping via `..`. Computed or escaping paths can't be audited at
/// build time and open a supply-chain hole, so they are E0957, not file reads.
/// `builtin` names the function (`embed_file` / `embed_bytes`) for the message.
fn check_embed_path(builtin: &str, arg: &CallArg, span: Span) -> Result<String, Diagnostic> {
    let path = match &arg.expr {
        Expr::Str(parts, _) if parts.len() == 1 => match &parts[0] {
            StrPart::Lit(s) => s.clone(),
            _ => return Err(embed_path_err(builtin, "literal", span)),
        },
        _ => return Err(embed_path_err(builtin, "literal", span)),
    };
    let p = std::path::Path::new(&path);
    if p.is_absolute() {
        return Err(embed_path_err(builtin, "absolute", span));
    }
    if p.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(embed_path_err(builtin, "escape", span));
    }
    Ok(path)
}

fn embed_path_err(builtin: &str, kind: &str, span: Span) -> Diagnostic {
    let (msg, why, fix) = match kind {
        "literal" => (
            format!("`{builtin}` path must be a string literal"),
            "a computed path can't be audited at build time, so the compiler can't tell which file it reads".to_string(),
            format!("pass the path inline, e.g. `{builtin}(\"data/file\")`"),
        ),
        "absolute" => (
            format!("`{builtin}` path must be relative"),
            "an absolute path reaches outside the project".to_string(),
            "use a path relative to this file's own directory".to_string(),
        ),
        _ => (
            format!("`{builtin}` path escapes the project with `..`"),
            "`..` could read files outside your project; embeds must stay inside it".to_string(),
            "drop the `..` and use a path under this file's directory".to_string(),
        ),
    };
    Diagnostic::error("E0957", msg, why, fix, Some(span))
}

/// D-CTMARKER1=C: substitute `$name` splices in a string using values from the
/// comptime scope. Unknown names are left as-is (`$unknown`). Used by `emit(…)`.
fn apply_dollar_splices(s: &str, scope: &HashMap<String, CtValue>) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' {
            let mut name = String::new();
            while let Some(&nc) = chars.peek() {
                if nc.is_alphanumeric() || nc == '_' {
                    name.push(nc);
                    chars.next();
                } else {
                    break;
                }
            }
            if !name.is_empty() {
                if let Some(val) = scope.get(&name) {
                    result.push_str(&val.jet_show());
                } else {
                    result.push('$');
                    result.push_str(&name);
                }
            } else {
                result.push('$');
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// c139: the integer value of `e` when it is (possibly parenthesized/negated)
/// an `Int` literal — used to mirror sema's compile-time-proof shortcut for a
/// ranged distinct-type constructor (`eval_distinct_ctor`).
fn literal_int(e: &Expr) -> Option<i64> {
    match e {
        Expr::Int(n, _, _) => Some(*n),
        Expr::Unary(UnOp::Neg, inner, _) => literal_int(inner).and_then(i64::checked_neg),
        Expr::Paren(inner, _) => literal_int(inner),
        _ => None,
    }
}

impl<'a> Interp<'a> {
    /// `f.[a, b, c]` → `[f(a), f(b), f(c)]` (fan-out, ratified in
    /// docs/spec/spec.md (S75 fan-out). Comptime only supports
    /// the named-one-arg-function callee case; sources/type-constructor
    /// callees are jetpack-module-specific sugar handled structurally by the
    /// jetpack module evaluator (src/jetpack/modeval.rs), not here.
    pub(super) fn eval_fan_out(
        &mut self,
        callee: &Expr,
        items: &[Expr],
        span: Span,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let Expr::Ident(name, _) = callee else {
            return Err(unsupported("this fan-out callee", callee.span()));
        };
        let func = self
            .funcs
            .get(name.as_str())
            .copied()
            .ok_or_else(|| unsupported(&format!("calling `{}`", name), span))?;
        if func.params.len() != 1 {
            return Err(unsupported(
                &format!("`{}` (fan-out needs a one-argument function)", name),
                span,
            ));
        }
        let param_name = func.params[0].name.clone();
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            let arg = self.eval(item, scope)?;
            let mut frame = HashMap::new();
            frame.insert(param_name.clone(), arg);
            let v = match self.exec_block(&func.body, &mut frame)? {
                Flow::Return(v) => v,
                _ => CtValue::Unit,
            };
            out.push(v);
        }
        Ok(CtValue::List(out))
    }

    pub(super) fn eval_call(
        &mut self,
        name: &str,
        span: Span,
        args: &[crate::AST::CallArg],
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        // Whole-program dev mode (E2-M4): the two IO builtins write to the
        // buffered sink, producing bytes identical to the compiled program
        // (`jet_show()` + `\n`). In pure comptime mode the sink is `None` and
        // these are unreachable (the purity check rejects them first, E0951).
        if name == "print" || name == "eprint" {
            if self.sink.is_some() {
                let text = match args.first() {
                    // D-DISPLAYDBG1/2: same Display-impl-aware rendering as
                    // `{value}` string interpolation (`show_value`) — `print`
                    // is bare-Display too, never the `@Debug` form.
                    Some(a) => {
                        let v = self.eval(&a.expr, scope)?;
                        self.show_value(&v, a.expr.span())?
                    }
                    None => String::new(),
                };
                let sink = self.sink.as_mut().expect("dev-mode sink");
                if name == "print" {
                    sink.stdout.push_str(&text);
                    sink.stdout.push('\n');
                } else {
                    sink.stderr.push_str(&text);
                    sink.stderr.push('\n');
                }
                return Ok(CtValue::Unit);
            }
            // Pure comptime mode: unreachable in practice, but stay honest.
            return Err(unsupported(&format!("`{}`", name), span));
        }
        // The sanctioned comptime build-time I/O builtins (D-CTIO1).
        if name == crate::Syntax::BUILTIN_EMBED_FILE {
            return self.eval_embed_file(args, span);
        }
        if name == crate::Syntax::BUILTIN_EMBED_BYTES {
            return self.eval_embed_bytes(args, span);
        }
        if name == "panic" {
            let msg = match args.first() {
                Some(a) => self.eval(&a.expr, scope)?.jet_show(),
                None => "comptime panic".to_string(),
            };
            return Err(comptime_panic(&msg, span));
        }
        if name == "require" || name == "require_eq" {
            return self.eval_require(name, args, span, scope);
        }
        // D-METADERIVE1=A: `emit(source_string)` — push a re-entry fragment.
        if name == "emit" {
            let val = match args.first() {
                Some(a) => self.eval(&a.expr, scope)?,
                None => return Err(unsupported("`emit` requires one argument", span)),
            };
            if let CtValue::Str(s) = val {
                let fragment = apply_dollar_splices(&s, scope);
                self.emitted_fragments.push(fragment);
                return Ok(CtValue::Unit);
            }
            return Err(unsupported("`emit` argument must be a string", span));
        }
        // c139/HOF: `f(x)` where `f` is a local binding (a lambda param, or a
        // `let f = someLambdaOrFn` variable) rather than a top-level `fn`
        // name — every bare-name call, whatever the callee resolves to,
        // parses as `Expr::Call` (the parser can't tell values from function
        // names apart), so this is where a stored closure value gets called.
        // Checked before the top-level-function lookup: a local binding
        // shadows a same-named top-level function, same as any other name.
        if let Some(f @ CtValue::Closure(_)) = scope.get(name).cloned() {
            let mut vals = Vec::with_capacity(args.len());
            for a in args {
                vals.push(self.eval(&a.expr, scope)?);
            }
            return self.call_closure(&f, vals, span);
        }
        // A user function: bind params, run the body in a fresh frame.
        let func = match self.funcs.get(name).copied() {
            Some(f) => f,
            None => {
                // c139 JIT/interpreter-parity: `Name(expr)` where `Name` isn't a
                // known function is the distinct-type / `#UnitFamily` constructor
                // call — the only capitalized-name *call* form Jet has (struct
                // literals use `.{ }`, enum variants use `.Variant`).
                if let Some(range) = self.distinct_ranges.get(name).copied() {
                    return self.eval_distinct_ctor(name, range, args, span, scope);
                }
                return Err(unsupported(&format!("calling `{}`", name), span));
            }
        };
        // D-VARIADIC1/D-ANY-JAI1: `name: ...T` / `name: ...[A, B]` — the last
        // parameter only. Every call-site argument from that position on
        // (any count, including zero) collects into one `List` bound to it;
        // fixed params before it still bind 1:1.
        if let Some(last) = func.params.last() {
            if last.variadic {
                let fixed = &func.params[..func.params.len() - 1];
                if args.len() < fixed.len() {
                    return Err(unsupported(
                        &format!("`{}` (wrong number of arguments)", name),
                        span,
                    ));
                }
                let mut frame = HashMap::new();
                for (p, a) in fixed.iter().zip(args) {
                    let v = self.eval(&a.expr, scope)?;
                    frame.insert(p.name.clone(), v);
                }
                let mut rest = Vec::with_capacity(args.len() - fixed.len());
                for a in &args[fixed.len()..] {
                    rest.push(self.eval(&a.expr, scope)?);
                }
                frame.insert(last.name.clone(), CtValue::List(rest));
                return self.call_func(name, func, frame);
            }
        }
        if func.params.len() != args.len() {
            return Err(unsupported(
                &format!("`{}` (wrong number of arguments)", name),
                span,
            ));
        }
        let mut frame = HashMap::new();
        for (p, a) in func.params.iter().zip(args) {
            let v = self.eval(&a.expr, scope)?;
            frame.insert(p.name.clone(), v);
        }
        self.call_func(name, func, frame)
    }

    /// c139: run a resolved `Func`'s body in `frame` (already bound: params,
    /// and `self` for an instance/associated method), threading the same
    /// debugger bookkeeping and `?`/`?? return` sentinel handling `eval_call`
    /// always has. Shared by plain calls, instance-method dispatch, and
    /// code-module namespaced calls.
    fn call_func(
        &mut self,
        name: &str,
        func: &Func,
        mut frame: HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        // D-DBG3: enter a user-function frame — bump the debugger's call depth
        // and current-function name so `next`/`finish` and the `in fn()` banner
        // track correctly, then restore both on the way out (every path).
        let prev_depth = self.depth;
        let prev_func = std::mem::replace(&mut self.cur_func, name.to_string());
        self.depth = prev_depth + 1;
        let result = self.exec_block(&func.body, &mut frame);
        self.depth = prev_depth;
        self.cur_func = prev_func;
        match result {
            Ok(Flow::Return(v)) => Ok(v),
            Ok(_) => Ok(CtValue::Unit),
            Err(ref d) if d.code == ERR_PROPAGATE_CODE => {
                // c97/D-STRPARSE1: `?` on an `Err` or `null` propagated via
                // the sentinel — convert to an `Err` return from this callee
                // so the caller can handle it (e.g. with `??`).
                let msg = d.what.clone();
                Ok(CtValue::ResErr(Box::new(CtValue::Str(msg))))
            }
            Err(ref d) if d.code == EARLY_RETURN_CODE => {
                // `?? return expr` inside a function — the return value was
                // encoded as a string; use Unit since we can't re-parse it.
                // In practice comptime code rarely uses `?? return` in a callee;
                // the primary use case is `?? return` at the top-level comptime
                // binding site where `exec_block` gets it directly.
                let _ = d; // diagnostic already matched; nothing to extract
                Ok(CtValue::Unit)
            }
            Err(e) => Err(e),
        }
    }

    /// c139 (D-DISPLAYDBG1/2): render `v` as `{value}` interpolation / `print`
    /// would in the compiled program. When `v`'s type has a user-written
    /// `impl Type.Display { fn display(self) -> String }`, run that exact Jet
    /// function body (byte-identical to what the real build does); otherwise
    /// fall back to the built-in `jet_show()` rendering (every primitive, and
    /// any struct/enum with no such impl — sema only accepts those in
    /// interpolation when they're "auto-printable", which `jet_show()`
    /// already matches).
    pub(super) fn show_value(&mut self, v: &CtValue, _span: Span) -> Result<String, Diagnostic> {
        let type_name = match v {
            CtValue::Struct { type_name, .. } | CtValue::Enum { type_name, .. } => {
                Some(type_name.clone())
            }
            _ => None,
        };
        if let Some(tn) = type_name {
            if let Some(f) = self
                .methods
                .get(&(tn.clone(), "display".to_string()))
                .copied()
            {
                if f.params.len() == 1 {
                    let mut frame = HashMap::new();
                    frame.insert(f.params[0].name.clone(), v.clone());
                    if let CtValue::Str(s) = self.call_func(&format!("{}.display", tn), f, frame)? {
                        return Ok(s);
                    }
                }
            }
        }
        Ok(v.jet_show())
    }

    /// c139: invoke a closure value (`(x) => x > 3`) with already-evaluated
    /// arguments — the counterpart of `call_func` for a lambda instead of a
    /// named `fn`. The frame starts from the closure's captured scope (so it
    /// still sees the bindings visible where it was created) with the params
    /// bound over that.
    pub(super) fn call_closure(
        &mut self,
        f: &CtValue,
        args: Vec<CtValue>,
        span: Span,
    ) -> Result<CtValue, Diagnostic> {
        let CtValue::Closure(data) = f else {
            return Err(unsupported(
                "calling this value (it isn't a function)",
                span,
            ));
        };
        if data.lambda.params.len() != args.len() {
            return Err(unsupported(
                "this closure (wrong number of arguments)",
                span,
            ));
        }
        let mut frame = data.captured.clone();
        for (p, a) in data.lambda.params.iter().zip(args) {
            frame.insert(p.name.clone(), a);
        }
        match &data.lambda.body {
            LambdaBody::Expr(e) => self.eval(e, &mut frame),
            LambdaBody::Block(stmts) => match self.exec_block(stmts, &mut frame)? {
                Flow::Return(v) => Ok(v),
                _ => Ok(CtValue::Unit),
            },
        }
    }

    /// c139: `Name(expr)` — the distinct-type / `#UnitFamily` constructor.
    /// `range` is the type's `distinct Base(lo..hi)` bound, if any
    /// (D-RANGETYPE1). Distinct types have zero runtime representation
    /// difference from their base (D-DIST1), so an unranged constructor is
    /// identity. A ranged one mirrors sema's own compile-time proof: a
    /// literal argument already known in-range folds to a direct value
    /// (matching what codegen bakes in); anything else is the fallible
    /// `Result`-wrapped form the `?`/`??` surface at the call site expects.
    fn eval_distinct_ctor(
        &mut self,
        name: &str,
        range: Option<(i64, i64)>,
        args: &[CallArg],
        span: Span,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let arg = args
            .first()
            .ok_or_else(|| unsupported(&format!("`{}` (wrong number of arguments)", name), span))?;
        let Some((lo, hi)) = range else {
            return self.eval(&arg.expr, scope);
        };
        if let Some(n) = literal_int(&arg.expr) {
            if n >= lo && n <= hi {
                return Ok(CtValue::Int(n));
            }
        }
        let v = self.eval(&arg.expr, scope)?;
        let n = as_int(&v, arg.expr.span())?;
        Ok(if n >= lo && n <= hi {
            CtValue::ResOk(Box::new(CtValue::Int(n)))
        } else {
            CtValue::ResErr(Box::new(CtValue::Str(format!(
                "{} out of range {}..{}",
                n, lo, hi
            ))))
        })
    }

    fn eval_require(
        &mut self,
        name: &str,
        args: &[crate::AST::CallArg],
        span: Span,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        if name == "require_eq" {
            let a = self.eval(&args[0].expr, scope)?;
            let b = self.eval(&args[1].expr, scope)?;
            if a != b {
                let msg = args
                    .get(2)
                    .map(|a| self.eval(&a.expr, scope))
                    .transpose()?
                    .map(|v| v.jet_show())
                    .unwrap_or_else(|| format!("{} != {}", a.jet_show(), b.jet_show()));
                return Err(comptime_panic(&msg, span));
            }
            return Ok(CtValue::Unit);
        }
        let cond = as_bool(&self.eval(&args[0].expr, scope)?, span)?;
        if !cond {
            let msg = args
                .get(1)
                .map(|a| self.eval(&a.expr, scope))
                .transpose()?
                .map(|v| v.jet_show())
                .unwrap_or_else(|| "a requirement was not met".to_string());
            return Err(comptime_panic(&msg, span));
        }
        Ok(CtValue::Unit)
    }

    fn eval_embed_file(&mut self, args: &[CallArg], span: Span) -> Result<CtValue, Diagnostic> {
        let builtin = crate::Syntax::BUILTIN_EMBED_FILE;
        let (rel, bytes) = self.read_embed(builtin, args, span)?;
        match String::from_utf8(bytes) {
            Ok(text) => Ok(CtValue::Str(text)),
            Err(_) => Err(Diagnostic::error(
                "E0955",
                format!("`{builtin}` can't read `{rel}` as text"),
                format!("the file isn't valid UTF-8; `{builtin}` returns a String"),
                "embed it with `embed_bytes(\"path\")` instead — it returns raw `[U8]`".to_string(),
                Some(span),
            )),
        }
    }

    /// D-CTIO1: `embed_bytes("path") -> [U8]` — the binary-safe sibling of
    /// `embed_file`. Same path-safety (E0957) and missing/unreadable (E0955)
    /// checks, but no UTF-8 requirement: any file embeds as raw bytes.
    fn eval_embed_bytes(&mut self, args: &[CallArg], span: Span) -> Result<CtValue, Diagnostic> {
        let builtin = crate::Syntax::BUILTIN_EMBED_BYTES;
        let (_rel, bytes) = self.read_embed(builtin, args, span)?;
        Ok(CtValue::Bytes(bytes))
    }

    /// Shared `embed_file`/`embed_bytes` front half: validate the path (E0957)
    /// and read the file (E0955 missing/unreadable). Returns the relative path
    /// (for messages) and the raw bytes; the UTF-8 decision is the caller's.
    ///
    /// D-CTEFFECT1 Tier-1: the sha256 of the bytes is pushed to `self.embed_inputs`
    /// so the caller can record it in `.jet/lock` for reproducible builds.
    fn read_embed(
        &mut self,
        builtin: &str,
        args: &[CallArg],
        span: Span,
    ) -> Result<(String, Vec<u8>), Diagnostic> {
        let arg = args
            .first()
            .ok_or_else(|| unsupported(&format!("{builtin} with no path"), span))?;
        let rel = check_embed_path(builtin, arg, span)?;
        let full = self.base_dir.join(&rel);
        match std::fs::read(&full) {
            Ok(bytes) => {
                // D-CTEFFECT1 Tier-1: record the embed input hash for .jet/lock.
                let hash = crate::SHA256::sha256_hex(&bytes);
                self.embed_inputs.push(crate::AST::ComptimeInput {
                    path: rel.clone(),
                    hash,
                });
                Ok((rel, bytes))
            }
            Err(e) => Err(Diagnostic::error(
                "E0955",
                format!("`{builtin}` can't open `{rel}`"),
                format!("{} (looked next to the file doing the embedding)", e),
                "check the path — it is relative to the file's own directory".to_string(),
                Some(span),
            )),
        }
    }

    /// D-CTEFFECT1 Tier-1 / D-NETDEP1=A: `core.net.fetch(url, sha256:)`.
    ///
    /// **Stub — backend pending.** D-NETDEP1=A ratified `ureq`/`minreq` as the
    /// HTTP backend (runtime-side, in a `jet-net/` workspace member; I6 holds).
    /// This stub preserves the correct Tier-1 routing (no `#Impure` gate) so
    /// the architecture is in place; replace the `E3412` body below with the
    /// real download once the workspace member is wired.
    ///
    /// When implemented:
    ///   1. Validate `url` (arg 0) and `expected_sha256` (arg 1 / labelled `sha256:`).
    ///   2. Download via the `jet-net` crate's blocking fetch.
    ///   3. `sha256_hex(bytes)` → compare; mismatch → **E3413**.
    ///   4. Unreachable / general fetch error → **E3414**; TLS client failures
    ///      reachable through HTTPS → **E4201**–**E4203**.
    ///   5. Push `ComptimeInput { path: "url:{url}", hash: actual }` to `embed_inputs`.
    ///   6. Return `CtValue::Str(content)` (non-UTF-8 content needs its own code).
    fn eval_net_fetch(&mut self, args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
        let url = match args.first() {
            Some(CtValue::Str(s)) => s.clone(),
            _ => return Err(Diagnostic::error(
                "E3414",
                "fetch: first argument must be a string URL".to_string(),
                "`core.net.fetch` expects a string URL as its first argument".to_string(),
                "pass a string literal: `net.fetch('https://example.com/data.txt', sha256: '…')`"
                    .to_string(),
                Some(span),
            )),
        };
        let expected = match args.get(1) {
            Some(CtValue::Str(s)) => s.clone(),
            _ => return Err(Diagnostic::error(
                "E3414",
                "fetch: `sha256:` argument missing or not a string".to_string(),
                "`core.net.fetch` requires a `sha256:` labelled argument for content verification"
                    .to_string(),
                "add `sha256: '<64-hex-chars>'` as the second argument".to_string(),
                Some(span),
            )),
        };

        let bytes = jet_net::fetch(&url).map_err(|e| {
            Diagnostic::error(
                e.diagnostic_code(),
                e.diagnostic_what(&url),
                e.diagnostic_why(&url),
                e.diagnostic_fix(),
                Some(span),
            )
        })?;

        let actual = crate::SHA256::sha256_hex(&bytes);
        if actual != expected {
            return Err(Diagnostic::error(
                "E3413",
                format!("fetch: sha256 mismatch for `{url}`"),
                format!("expected `{expected}` but content hashes to `{actual}`"),
                "update the `sha256:` pin to match the content, or verify the URL is correct"
                    .to_string(),
                Some(span),
            ));
        }

        let content = String::from_utf8(bytes).map_err(|_| Diagnostic::error(
            "E3414",
            format!("fetch: content at `{url}` is not valid UTF-8"),
            "the downloaded bytes could not be decoded as UTF-8 text".to_string(),
            "binary content is not supported by comptime fetch; use `embed_bytes` for binary data".to_string(),
            Some(span),
        ))?;

        self.embed_inputs.push(crate::AST::ComptimeInput {
            path: format!("url:{url}"),
            hash: actual,
        });

        Ok(CtValue::Str(content))
    }

    /// c139: write `new_value` back to the place `target` reads from — the
    /// mutating-method counterpart of evaluating `target` for a read. Handles
    /// a bare local (`xs.push(v)`) and a field-access chain of any depth
    /// (`copy.items.push(v)`), recursing on the base of a `Field` so the
    /// write propagates all the way back to the owning local.
    fn write_back(
        &mut self,
        target: &Expr,
        new_value: CtValue,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic> {
        match target {
            Expr::Ident(name, _) => {
                scope.insert(name.clone(), new_value);
                Ok(())
            }
            Expr::Field(inner, member, span) => {
                let mut base = self.eval(inner, scope)?;
                match &mut base {
                    CtValue::Struct { fields, .. } => {
                        match fields.iter_mut().find(|(n, _)| n == member) {
                            Some(slot) => slot.1 = new_value,
                            None => {
                                return Err(unsupported(&format!("the field `.{}`", member), *span))
                            }
                        }
                    }
                    _ => return Err(unsupported("this indexed assignment", *span)),
                }
                self.write_back(inner, base, scope)
            }
            _ => Err(unsupported("this indexed assignment", target.span())),
        }
    }

    pub(super) fn eval_method(
        &mut self,
        receiver: &Expr,
        method: &str,
        span: Span,
        args: &[crate::AST::CallArg],
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        // c97/D-STRPARSE1: static method on a built-in type name (e.g. `Int.parse(s)`).
        // Check *before* evaluating the receiver so `Int`/`Float` don't fail scope lookup.
        if let Expr::Ident(type_name, _) = receiver {
            // Only intercept known built-in type names; user struct names use normal path.
            let is_builtin_type = matches!(type_name.as_str(), "Int" | "Float" | "Bool" | "String");
            if is_builtin_type {
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval(&a.expr, scope)?);
                }
                if let Some(result) = apply_static_type_method(type_name, method, argv, span) {
                    return result;
                }
                // If the static dispatch didn't match, fall through to the error below —
                // a built-in type name is not a valid receiver for unknown methods.
                return Err(super::Diagnostics::unsupported(
                    &format!("`{}.{}()`", type_name, method),
                    span,
                ));
            }
        }
        // D-CTCORE1 (ratified 2026-06-22): module alias calls like `math.sqrt(x)`.
        // Check *before* evaluating the receiver so unknown aliases don't fail.
        if let Expr::Ident(alias, _) = receiver {
            if let Some(module) = self.core_imports.get(alias.as_str()).cloned() {
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval(&a.expr, scope)?);
                }
                // D-CTEFFECT1 Tier-1: fetch is hermetic (sha256-pinned); no gate.
                if module == "core.net" && method == "fetch" {
                    return self.eval_net_fetch(argv, span);
                }
                // U13 (D-JPK-SECRETCRYPTO1): `core.vault.get` is denied at build time
                // unconditionally — unlike the Tier-2 effects below, there is no
                // `#Impure`/`--allow-impure` escape hatch, because a build artifact
                // must never bake in a decrypted secret (I1).
                if module == "core.vault" {
                    return Err(Diagnostic::error(
                        "E1265",
                        format!("`{}.{}()` can't be reached from a build-time context", module, method),
                        "module-field/comptime evaluation runs before secrets are ever decrypted; \
                         a repo's encrypted store is only ever opened at ordinary runtime, and — \
                         unlike the Tier-2 comptime effect gate — there is no `#Impure` escape hatch \
                         here.".to_string(),
                        "move the secret read out of comptime/module-field evaluation and into \
                         ordinary runtime code.".to_string(),
                        Some(span),
                    ));
                }
                // D-CTEFFECT1: Tier-2 effect calls require an #Impure gate (or REPL sandbox).
                let is_tier2 = matches!(
                    module.as_str(),
                    "core.files"
                        | "core.env"
                        | "core.io"
                        | "core.exec"
                        | "core.net"
                        | "core.process"
                );
                if is_tier2 {
                    if self.impure_depth == 0 {
                        return Err(Diagnostic::error(
                            "E3410",
                            format!("`{}.{}()` is a Tier-2 comptime effect — it requires a `#Impure` gate", module, method),
                            "ambient I/O (filesystem, environment, process) is not allowed in \
                             pure comptime evaluation".to_string(),
                            "wrap the comptime binding in `#Impure(\"reason\") { … }` and \
                             pass `--allow-impure` to the build".to_string(),
                            Some(span),
                        ));
                    }
                    // Gate present (impure_depth > 0) but check --allow-impure flag too.
                    if !self.allow_impure {
                        return Err(Diagnostic::error(
                            "E3411",
                            format!("`{}.{}()` inside `#Impure` gate, but `--allow-impure` was not passed", module, method),
                            "the `#Impure` block opts in to ambient comptime I/O, but the build \
                             flag is required so CI can audit builds that touch the host".to_string(),
                            "add `--allow-impure` to your `jet build` / `jet run` invocation".to_string(),
                            Some(span),
                        ));
                    }
                    return apply_impure_core_call(
                        &module,
                        method,
                        argv,
                        span,
                        self.base_dir,
                        self.sink.as_deref_mut(),
                    );
                }
                return apply_core_call(&module, method, argv, span, self.repl_mode);
            }
        }

        // Enum-variant construction with a payload, called with an explicit type
        // name (`ParseError.BadDigit(raw)`), mirrors the no-arg `Field` fallback
        // below in interp.rs (`Color.Red`): sema already checked the variant
        // exists, so at eval time an unbound, capitalized receiver whose method
        // is also capitalized (variant-naming convention, S34) is a variant
        // literal, not a real method call. Checked after the builtin-type and
        // core-import cases above, and only when the receiver isn't a bound
        // local, so real static dispatch on a bound value is unaffected.
        if let Expr::Ident(type_name, _) = receiver {
            let is_type_name = type_name.chars().next().is_some_and(|c| c.is_uppercase());
            let is_variant_name = method.chars().next().is_some_and(|c| c.is_uppercase());
            if is_type_name && is_variant_name && !scope.contains_key(type_name.as_str()) {
                let mut out = Vec::with_capacity(args.len());
                for a in args {
                    let label = a.label.as_ref().map(|(n, _)| n.clone());
                    out.push((label, self.eval(&a.expr, scope)?));
                }
                return Ok(CtValue::Enum {
                    type_name: type_name.clone(),
                    variant: method.to_string(),
                    args: out,
                });
            }
        }

        // c139 JIT/interpreter-parity: a static/associated call (`Type.assoc_fn(…)`,
        // no `self` param — `impl`/in-struct methods registered in
        // `self.methods`) or a D-MOD2 code-module namespaced call
        // (`alias.fn(…)` — registered `alias__fn` in `self.funcs`), reached via
        // an unbound name. A real instance receiver is always a scoped value,
        // so it never lands here. Checked after the enum-variant-literal case
        // above (capitalized `Type.Variant(…)` wins first).
        if let Expr::Ident(name, _) = receiver {
            if !scope.contains_key(name.as_str()) {
                if let Some(f) = self
                    .methods
                    .get(&(name.clone(), method.to_string()))
                    .copied()
                {
                    if f.params.len() == args.len() {
                        let mut frame = HashMap::new();
                        for (p, a) in f.params.iter().zip(args) {
                            let v = self.eval(&a.expr, scope)?;
                            frame.insert(p.name.clone(), v);
                        }
                        return self.call_func(&format!("{}.{}", name, method), f, frame);
                    }
                }
                let mangled = format!("{}__{}", name, method);
                if let Some(f) = self.funcs.get(mangled.as_str()).copied() {
                    if f.params.len() == args.len() {
                        let mut frame = HashMap::new();
                        for (p, a) in f.params.iter().zip(args) {
                            let v = self.eval(&a.expr, scope)?;
                            frame.insert(p.name.clone(), v);
                        }
                        return self.call_func(&mangled, f, frame);
                    }
                }
            }
        }

        // D-HOLE1: `Option.lift2(f, a, b)` — a static call on the builtin
        // `Option` pseudo-type (never a real scoped value), so it's checked
        // here alongside the unbound-name case above rather than folded into
        // it (`Option` isn't in `self.methods`/`self.funcs`, and `f` is a
        // closure this dispatch must itself invoke).
        if let Expr::Ident(name, _) = receiver {
            if name == "Option" && method == "lift2" && !scope.contains_key("Option") {
                if args.len() != 3 {
                    return Err(unsupported(
                        "`Option.lift2` (wrong number of arguments)",
                        span,
                    ));
                }
                let f = self.eval(&args[0].expr, scope)?;
                let a = self.eval(&args[1].expr, scope)?;
                let b = self.eval(&args[2].expr, scope)?;
                return Ok(match (a, b) {
                    (CtValue::Some(av), CtValue::Some(bv)) => {
                        CtValue::Some(Box::new(self.call_closure(&f, vec![*av, *bv], span)?))
                    }
                    _ => CtValue::None(Type::Int),
                });
            }
        }

        // c139: higher-order methods — need `&mut self` to invoke a closure
        // argument, so (like the mutating methods just below) they can't be
        // plain `apply_method` entries. Guarded to the receiver shapes they
        // actually apply to; anything else falls through to the generic
        // dispatch at the end of this function.
        const HOF_METHODS: &[&str] = &["filter", "map", "each", "sort_by", "find", "reduce"];
        if HOF_METHODS.contains(&method) {
            let recv = self.eval(receiver, scope)?;
            match (&recv, method) {
                (CtValue::List(xs), "filter") => {
                    let f = self.eval(&args[0].expr, scope)?;
                    let mut out = Vec::new();
                    for x in xs {
                        if as_bool(&self.call_closure(&f, vec![x.clone()], span)?, span)? {
                            out.push(x.clone());
                        }
                    }
                    return Ok(CtValue::List(out));
                }
                (CtValue::List(xs), "map") => {
                    let f = self.eval(&args[0].expr, scope)?;
                    let mut out = Vec::with_capacity(xs.len());
                    for x in xs {
                        out.push(self.call_closure(&f, vec![x.clone()], span)?);
                    }
                    return Ok(CtValue::List(out));
                }
                (CtValue::List(xs), "each") => {
                    let f = self.eval(&args[0].expr, scope)?;
                    for x in xs {
                        self.call_closure(&f, vec![x.clone()], span)?;
                    }
                    return Ok(CtValue::Unit);
                }
                (CtValue::List(xs), "find") => {
                    let f = self.eval(&args[0].expr, scope)?;
                    for x in xs {
                        if as_bool(&self.call_closure(&f, vec![x.clone()], span)?, span)? {
                            return Ok(CtValue::Some(Box::new(x.clone())));
                        }
                    }
                    return Ok(CtValue::None(Type::Int));
                }
                (CtValue::List(xs), "reduce") => {
                    let f = self.eval(&args[0].expr, scope)?;
                    let mut acc = self.eval(&args[1].expr, scope)?;
                    for x in xs {
                        acc = self.call_closure(&f, vec![acc, x.clone()], span)?;
                    }
                    return Ok(acc);
                }
                // `.sort_by` writes back like the MUTATING list methods below
                // (D-BIND4 `:=` receiver) — key every element once, sort the
                // keyed pairs, then write the reordered list back through the
                // same lvalue path `push`/`pop`/… use.
                (CtValue::List(xs), "sort_by")
                    if matches!(receiver, Expr::Ident(..) | Expr::Field(..)) =>
                {
                    let f = self.eval(&args[0].expr, scope)?;
                    let mut keyed = Vec::with_capacity(xs.len());
                    for x in xs {
                        let k = self.call_closure(&f, vec![x.clone()], span)?;
                        keyed.push((k, x.clone()));
                    }
                    let mut sort_err = None;
                    keyed.sort_by(|a, b| match cmp(a.0.clone(), b.0.clone(), span) {
                        Ok(o) => o,
                        Err(e) => {
                            sort_err.get_or_insert(e);
                            std::cmp::Ordering::Equal
                        }
                    });
                    if let Some(e) = sort_err {
                        return Err(e);
                    }
                    let sorted = CtValue::List(keyed.into_iter().map(|(_, v)| v).collect());
                    self.write_back(receiver, sorted, scope)?;
                    return Ok(CtValue::Unit);
                }
                (CtValue::Some(inner), "map") => {
                    let f = self.eval(&args[0].expr, scope)?;
                    let v = self.call_closure(&f, vec![(**inner).clone()], span)?;
                    return Ok(CtValue::Some(Box::new(v)));
                }
                (CtValue::None(t), "map") => return Ok(CtValue::None(t.clone())),
                _ => {}
            }
        }

        // D-ANY-JAI1: `reflect.of(x).display()` — same Display-impl-aware
        // rendering `{x}`/`print(x)` use (`show_value`), so it needs `&mut
        // self` and can't be a plain `apply_method` entry like `.type_name`/
        // `.fields` just above it. Guarded specifically to the `__Reflect`
        // wrapper — a *user* type's own `.display()` call (the method a
        // `impl Type.Display` block defines) still falls through to the
        // ordinary instance-method dispatch below, unaffected.
        if method == "display" {
            let peek = self.eval(receiver, scope)?;
            if let CtValue::Struct { type_name, fields } = &peek {
                if type_name == "__Reflect" {
                    let inner = fields
                        .iter()
                        .find(|(n, _)| n == "value")
                        .map(|(_, v)| v.clone())
                        .unwrap_or(CtValue::Unit);
                    let s = self.show_value(&inner, span)?;
                    return Ok(CtValue::Str(s));
                }
            }
        }

        // Mutating list/map methods on a named variable write back in place.
        const MUTATING: &[&str] = &[
            "push", "pop", "insert", "remove", "clear", "reverse", "sort",
        ];
        if MUTATING.contains(&method) && matches!(receiver, Expr::Ident(..) | Expr::Field(..)) {
            let mut argv = Vec::new();
            for a in args {
                argv.push(self.eval(&a.expr, scope)?);
            }
            let mut container = self.eval(receiver, scope)?;
            let ret = apply_mutating(&mut container, method, argv, span)?;
            self.write_back(receiver, container, scope)?;
            return Ok(ret);
        }
        let recv = self.eval(receiver, scope)?;
        // c139: an instance method the user wrote — `impl Type { fn … }` /
        // in-struct `fn`/`impl Trait { … }`. `recv`'s own `type_name` (not the
        // receiver expression's static type) picks the impl, so a value bound
        // through a trait-typed parameter still dispatches to its concrete
        // type's method (matches trait dynamic dispatch, no vtable needed).
        // Checked before the generic builtin dispatch so a user method always
        // wins over a same-named builtin.
        let type_name = match &recv {
            CtValue::Struct { type_name, .. } | CtValue::Enum { type_name, .. } => {
                Some(type_name.clone())
            }
            _ => None,
        };
        if let Some(tn) = type_name {
            if let Some(f) = self.methods.get(&(tn.clone(), method.to_string())).copied() {
                if !f.params.is_empty() && f.params.len() == args.len() + 1 {
                    let mut frame = HashMap::new();
                    frame.insert(f.params[0].name.clone(), recv.clone());
                    for (p, a) in f.params[1..].iter().zip(args) {
                        let v = self.eval(&a.expr, scope)?;
                        frame.insert(p.name.clone(), v);
                    }
                    return self.call_func(&format!("{}.{}", tn, method), f, frame);
                }
            }
        }
        let mut argv = Vec::new();
        for a in args {
            argv.push(self.eval(&a.expr, scope)?);
        }
        apply_method(&recv, method, argv, span)
    }
}

// ---------------------------------------------------------------------------
// D-CTCORE1 (ratified 2026-06-22): curated pure Core whitelist for comptime.
//
// Only deterministic, pure functions may run at comptime. I/O (`fs.read`,
// `env.get`, etc.) is rejected here with a teaching diagnostic; the user
// can get build-time I/O via the explicit `embed_file`/`embed_bytes` tier.
//
// The whitelist grows with tests; start with core.math and core.string.
// ---------------------------------------------------------------------------

fn as_float(v: &CtValue, span: Span) -> Result<f64, Diagnostic> {
    match v {
        CtValue::Float(f) => Ok(*f),
        CtValue::Int(n) => Ok(*n as f64),
        _ => Err(unsupported(
            "non-numeric argument to comptime math call",
            span,
        )),
    }
}

fn as_string(v: &CtValue, span: Span) -> Result<&str, Diagnostic> {
    match v {
        CtValue::Str(s) => Ok(s.as_str()),
        _ => Err(unsupported(
            "non-string argument to comptime string call",
            span,
        )),
    }
}

/// D-UUIDENC1=A: a `[U8]` argument — either the literal `Bytes` shape
/// (`embed_bytes`'s output) or a `List` of `Int` elements (an ordinary `[U8]`
/// list literal), matching whichever the caller happens to be holding.
fn as_bytes(v: &CtValue, span: Span) -> Result<Vec<u8>, Diagnostic> {
    match v {
        CtValue::Bytes(bs) => Ok(bs.clone()),
        CtValue::List(xs) => xs
            .iter()
            .map(|x| match x {
                CtValue::Int(n) if (0..=255).contains(n) => Ok(*n as u8),
                _ => Err(unsupported(
                    "a `[U8]` list with an out-of-range element",
                    span,
                )),
            })
            .collect(),
        _ => Err(unsupported(
            "non-`[U8]` argument to comptime encoding call",
            span,
        )),
    }
}

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

fn hex_encode(bytes: Vec<u8>) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX_DIGITS[(b >> 4) as usize] as char);
        out.push(HEX_DIGITS[(b & 0xf) as usize] as char);
    }
    out
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::with_capacity(chars.len() / 2);
    for pair in chars.chunks(2) {
        let hi = pair[0].to_digit(16)?;
        let lo = pair[1].to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
    }
    Some(out)
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(bytes: Vec<u8>) -> String {
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied();
        let b2 = chunk.get(2).copied();
        out.push(BASE64_ALPHABET[(b0 >> 2) as usize] as char);
        out.push(BASE64_ALPHABET[(((b0 & 0x03) << 4) | (b1.unwrap_or(0) >> 4)) as usize] as char);
        out.push(match b1 {
            Some(b1) => {
                BASE64_ALPHABET[(((b1 & 0x0f) << 2) | (b2.unwrap_or(0) >> 6)) as usize] as char
            }
            None => '=',
        });
        out.push(match b2 {
            Some(b2) => BASE64_ALPHABET[(b2 & 0x3f) as usize] as char,
            None => '=',
        });
    }
    out
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    fn digit(c: u8) -> Option<u8> {
        BASE64_ALPHABET
            .iter()
            .position(|&d| d == c)
            .map(|i| i as u8)
    }
    let s = s.trim_end_matches('=');
    let bytes = s.as_bytes();
    if bytes.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let d: Vec<u8> = chunk.iter().map(|&c| digit(c)).collect::<Option<_>>()?;
        out.push((d[0] << 2) | (d.get(1).copied().unwrap_or(0) >> 4));
        if d.len() > 2 {
            out.push((d[1] << 4) | (d[2] >> 2));
        }
        if d.len() > 3 {
            out.push((d[2] << 6) | d[3]);
        }
    }
    Some(out)
}

/// Core modules the REPL interpreter cannot run (native FFI / threads / HTTP stack).
fn repl_native_only_module(module: &str) -> Option<&'static str> {
    match module {
        "core.http" | "core.http.client" | "core.http.server" | "jet.http" => {
            Some("the HTTP client/server (`core.http`)")
        }
        "core.db" | "jet.db" => Some("`core.db` (SQLite)"),
        "core.net" => Some("network sockets (`core.net`)"),
        "core.archive" | "jet.archive" => Some("`core.archive`"),
        "core.reactive" | "jet.reactive" => Some("`core.reactive`"),
        "core.crypto" | "core.crypto.random" | "jet.crypto" => Some("`core.crypto`"),
        "core.tasks" | "core.channels" => Some("tasks/channels (`core.tasks`)"),
        "core.mem" | "core.mem.alloc" => Some("`core.mem` (low-level memory tier)"),
        "jet.log" => Some("`core.log`"),
        _ => None,
    }
}

fn repl_native_module_diag(module: &str, method: &str, span: Span) -> Diagnostic {
    let feature = repl_native_only_module(module).unwrap_or("a native-only core module");
    Diagnostic::error(
        "E1802",
        format!("the REPL interpreter can't run `{}.{method}()`", module),
        format!(
            "the REPL is an interpreter for learning Jet; {feature} needs the real compiler \
             and native runtime"
        ),
        "run `jet run <file.jet>` or `jet build <file.jet>` to use the full compiler".to_string(),
        Some(span),
    )
}

fn io_error_value(path: &str, e: std::io::Error) -> CtValue {
    let kind = match e.kind() {
        std::io::ErrorKind::NotFound => "NotFound",
        std::io::ErrorKind::PermissionDenied => "PermissionDenied",
        _ => "Other",
    };
    CtValue::Struct {
        type_name: "IoError".to_string(),
        fields: if kind == "Other" {
            vec![
                ("kind".to_string(), CtValue::Str(kind.to_string())),
                ("message".to_string(), CtValue::Str(e.to_string())),
            ]
        } else {
            vec![
                ("kind".to_string(), CtValue::Str(kind.to_string())),
                ("path".to_string(), CtValue::Str(path.to_string())),
            ]
        },
    }
}

fn splitmix64(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 7;
    x ^= x >> 9;
    x = x.wrapping_mul(0x9e3779b97f4a7c15);
    *state = x;
    x
}

pub(super) fn random_int(state: &mut u64, low: i64, high: i64) -> i64 {
    if high <= low {
        return low;
    }
    low + (splitmix64(state) % ((high - low + 1) as u64)) as i64
}

pub(super) fn random_float(state: &mut u64) -> f64 {
    (splitmix64(state) as f64) / (u64::MAX as f64)
}

thread_local! {
    static JET_AMBIENT_RNG: std::cell::Cell<u64> = std::cell::Cell::new(0x4d595df4d0f33173);
}

fn with_ambient_rng<R>(f: impl FnOnce(&mut u64) -> R) -> R {
    JET_AMBIENT_RNG.with(|cell| {
        let mut state = cell.get();
        let out = f(&mut state);
        cell.set(state);
        out
    })
}

/// Evaluate a whitelisted pure Core call at comptime / in the REPL.
/// `module` is the full path (e.g. `"core.math"`, `"jet.regex"`).
fn apply_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    repl_mode: bool,
) -> Result<CtValue, Diagnostic> {
    if repl_mode {
        if let Some(_) = repl_native_only_module(module) {
            return Err(repl_native_module_diag(module, method, span));
        }
    }

    let one = |i: usize| {
        args.get(i).ok_or_else(|| {
            unsupported(&format!("{}.{}(): missing arg {}", module, method, i), span)
        })
    };

    match (module, method) {
        // --- core.math whitelist ---
        ("core.math", "sqrt") => Ok(CtValue::Float(as_float(one(0)?, span)?.sqrt())),
        ("core.math", "floor") => Ok(CtValue::Float(as_float(one(0)?, span)?.floor())),
        ("core.math", "ceil") => Ok(CtValue::Float(as_float(one(0)?, span)?.ceil())),
        ("core.math", "round") => Ok(CtValue::Int(as_float(one(0)?, span)?.round() as i64)),
        ("core.math", "abs") => match one(0)? {
            CtValue::Int(n) => Ok(CtValue::Int(n.abs())),
            CtValue::Float(f) => Ok(CtValue::Float(f.abs())),
            _ => Err(unsupported("core.math.abs: non-numeric argument", span)),
        },
        ("core.math", "pow") => {
            let a = as_float(one(0)?, span)?;
            let b = as_float(one(1)?, span)?;
            Ok(CtValue::Float(a.powf(b)))
        }
        ("core.math", "min") => {
            let a = as_float(one(0)?, span)?;
            let b = as_float(one(1)?, span)?;
            Ok(CtValue::Float(a.min(b)))
        }
        ("core.math", "max") => {
            let a = as_float(one(0)?, span)?;
            let b = as_float(one(1)?, span)?;
            Ok(CtValue::Float(a.max(b)))
        }
        ("core.math", "clamp") => {
            let v = as_float(one(0)?, span)?;
            let lo = as_float(one(1)?, span)?;
            let hi = as_float(one(2)?, span)?;
            Ok(CtValue::Float(v.clamp(lo, hi)))
        }
        ("core.math", "log2") => Ok(CtValue::Float(as_float(one(0)?, span)?.log2())),
        ("core.math", "log10") => Ok(CtValue::Float(as_float(one(0)?, span)?.log10())),
        // --- core.string whitelist ---
        ("core.string", "len") | ("core.string", "length") => {
            let s = as_string(one(0)?, span)?;
            Ok(CtValue::Int(s.chars().count() as i64))
        }
        ("core.string", "to_upper") | ("core.string", "upper") => {
            let s = as_string(one(0)?, span)?.to_uppercase();
            Ok(CtValue::Str(s))
        }
        ("core.string", "to_lower") | ("core.string", "lower") => {
            let s = as_string(one(0)?, span)?.to_lowercase();
            Ok(CtValue::Str(s))
        }
        ("core.string", "trim") => {
            let s = as_string(one(0)?, span)?.trim().to_string();
            Ok(CtValue::Str(s))
        }
        ("core.string", "starts_with") => {
            let s = as_string(one(0)?, span)?;
            let prefix = as_string(one(1)?, span)?;
            Ok(CtValue::Bool(s.starts_with(prefix)))
        }
        ("core.string", "ends_with") => {
            let s = as_string(one(0)?, span)?;
            let suffix = as_string(one(1)?, span)?;
            Ok(CtValue::Bool(s.ends_with(suffix)))
        }
        ("core.string", "contains") => {
            let s = as_string(one(0)?, span)?;
            let needle = as_string(one(1)?, span)?;
            Ok(CtValue::Bool(s.contains(needle)))
        }
        ("core.string", "replace") => {
            let s = as_string(one(0)?, span)?.to_string();
            let from = as_string(one(1)?, span)?.to_string();
            let to = as_string(one(2)?, span)?.to_string();
            Ok(CtValue::Str(s.replace(&from, &to)))
        }
        // --- core.path (pure) ---
        ("core.path", "join") => {
            let a = as_string(one(0)?, span)?;
            let b = as_string(one(1)?, span)?;
            let joined = std::path::Path::new(a)
                .join(b)
                .to_string_lossy()
                .into_owned();
            Ok(CtValue::Str(joined))
        }
        ("core.path", "parent") => {
            let p = as_string(one(0)?, span)?;
            Ok(CtValue::Str(
                std::path::Path::new(p)
                    .parent()
                    .map(|x| x.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ))
        }
        ("core.path", "extension") => {
            let p = as_string(one(0)?, span)?;
            Ok(CtValue::Str(
                std::path::Path::new(p)
                    .extension()
                    .map(|x| x.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ))
        }
        ("core.path", "normalize") => {
            let p = as_string(one(0)?, span)?;
            Ok(CtValue::Str(
                std::path::Path::new(p)
                    .components()
                    .collect::<std::path::PathBuf>()
                    .to_string_lossy()
                    .into_owned(),
            ))
        }
        // --- core.encoding.json ---
        ("core.encoding.json", "parse") => {
            let text = as_string(one(0)?, span)?;
            match super::JsonInterp::parse_json(text) {
                Ok(v) => Ok(CtValue::ResOk(Box::new(v))),
                Err(e) => Ok(CtValue::ResErr(Box::new(
                    super::JsonInterp::json_error_value(e),
                ))),
            }
        }
        ("core.encoding.json", "decode") => {
            // D-JSON3: lenient decode — same tree as `.parse()`, but a `Text`
            // leaf that looks like a number or a boolean coerces to that type
            // (a wire value from a source that only has strings, e.g. a form
            // post or a CSV cell, still lands as the type the rest of the tree
            // expects). The coercion log line D-JSON3 also specifies is
            // stderr-only and not part of any golden comparison, so it's not
            // reproduced here.
            let text = as_string(one(0)?, span)?;
            match super::JsonInterp::parse_json(text) {
                Ok(v) => Ok(CtValue::ResOk(Box::new(super::JsonInterp::coerce_json(v)))),
                Err(e) => Ok(CtValue::ResErr(Box::new(
                    super::JsonInterp::json_error_value(e),
                ))),
            }
        }
        ("core.encoding.json", "to_string") => {
            let v = one(0)?;
            Ok(CtValue::Str(super::JsonInterp::render_json_pretty(
                v, false, 0,
            )))
        }
        ("core.encoding.json", "to_string_pretty") => {
            let v = one(0)?;
            Ok(CtValue::Str(super::JsonInterp::render_json_pretty(
                v, true, 0,
            )))
        }
        // --- core.time pure constructors ---
        ("core.time", "ms") => {
            let n = match one(0)? {
                CtValue::Int(v) => *v,
                _ => return Err(unsupported("time.ms expects an Int", span)),
            };
            Ok(CtValue::Struct {
                type_name: crate::Syntax::DURATION_TYPE.to_string(),
                fields: vec![("ms".to_string(), CtValue::Int(n))],
            })
        }
        ("core.time", "secs") => {
            let n = match one(0)? {
                CtValue::Int(v) => *v,
                _ => return Err(unsupported("time.secs expects an Int", span)),
            };
            Ok(CtValue::Struct {
                type_name: crate::Syntax::DURATION_TYPE.to_string(),
                fields: vec![("ms".to_string(), CtValue::Int(n * 1000))],
            })
        }
        ("core.time", "clock") => {
            let seed = match one(0)? {
                CtValue::Int(v) => *v,
                _ => return Err(unsupported("time.clock expects an Int seed", span)),
            };
            Ok(CtValue::Struct {
                type_name: crate::Syntax::CLOCK_TYPE.to_string(),
                fields: vec![("now".to_string(), CtValue::Int(seed))],
            })
        }
        // --- jet.regex / core.regex (D-REGEX1) ---
        ("jet.regex", "is_match") => regex_is_match(args, span),
        ("jet.regex", "find") => regex_find(args, span),
        ("jet.regex", "find_all") => regex_find_all(args, span),
        ("jet.regex", "split") => regex_split(args, span),
        ("jet.regex", "replace") | ("jet.regex", "replace_all") => regex_replace(args, span),
        ("jet.regex", "match") => regex_match(args, span),
        // --- core.random (ambient; seed for deterministic REPL transcripts) ---
        ("core.random", "seed") => {
            let seed = match one(0)? {
                CtValue::Int(n) => *n as u64,
                _ => return Err(unsupported("random.seed expects an Int", span)),
            };
            with_ambient_rng(|st| *st = seed);
            Ok(CtValue::Unit)
        }
        ("core.random", "int") => {
            let low = match one(0)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("random.int expects Int bounds", span)),
            };
            let high = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("random.int expects Int bounds", span)),
            };
            Ok(CtValue::Int(with_ambient_rng(|st| {
                random_int(st, low, high)
            })))
        }
        ("core.random", "float") => Ok(CtValue::Float(with_ambient_rng(|st| random_float(st)))),
        ("core.random", "rng") => {
            let seed = match one(0)? {
                CtValue::Int(n) => *n as u64,
                _ => return Err(unsupported("random.rng expects an Int seed", span)),
            };
            Ok(CtValue::Struct {
                type_name: crate::Syntax::RNG_TYPE.to_string(),
                fields: vec![("state".to_string(), CtValue::Int(seed as i64))],
            })
        }
        // --- D-ANY-JAI1: core.reflect (the runtime reflection floor, pure).
        // `"__Reflect"`/`"__ReflectField"` are internal-only tags (like
        // `"TypeInfo"`/`"Match"`/`"IoError"` elsewhere in this file) — never a
        // real Jet type name a user can write, so no `Syntax.rs` entry (I7 is
        // about user-typeable names). `.type_name`/`.fields` are plain reads
        // (`Builtins::apply_method`); `.display` needs `&mut self` (it may
        // run a user `Display` impl), so it's dispatched in `eval_method`.
        ("core.reflect", "of") => Ok(CtValue::Struct {
            type_name: "__Reflect".to_string(),
            fields: vec![("value".to_string(), one(0)?.clone())],
        }),
        // --- D-UUIDENC1=A: core.encoding.hex / core.encoding.base64 (pure) ---
        ("core.encoding.hex", "encode") => {
            let bytes = as_bytes(one(0)?, span)?;
            Ok(CtValue::Str(hex_encode(bytes)))
        }
        ("core.encoding.hex", "decode") => {
            let s = as_string(one(0)?, span)?;
            Ok(match hex_decode(s) {
                Some(bytes) => CtValue::ResOk(Box::new(CtValue::Bytes(bytes))),
                None => CtValue::ResErr(Box::new(CtValue::Str(format!("`{}` isn't valid hex", s)))),
            })
        }
        ("core.encoding.base64", "encode") => {
            let bytes = as_bytes(one(0)?, span)?;
            Ok(CtValue::Str(base64_encode(bytes)))
        }
        ("core.encoding.base64", "decode") => {
            let s = as_string(one(0)?, span)?;
            Ok(match base64_decode(s) {
                Some(bytes) => CtValue::ResOk(Box::new(CtValue::Bytes(bytes))),
                None => CtValue::ResErr(Box::new(CtValue::Str(format!(
                    "`{}` isn't valid base64",
                    s
                )))),
            })
        }
        // --- core.text.unicode (std-only Unicode scalar helpers, pure) ---
        ("core.text.unicode", "scalar_count") => Ok(CtValue::Int(
            as_string(one(0)?, span)?.chars().count() as i64,
        )),
        ("core.text.unicode", "byte_count") => {
            Ok(CtValue::Int(as_string(one(0)?, span)?.len() as i64))
        }
        ("core.text.unicode", "is_ascii") => {
            Ok(CtValue::Bool(as_string(one(0)?, span)?.is_ascii()))
        }
        ("core.text.unicode", "lower") => {
            Ok(CtValue::Str(as_string(one(0)?, span)?.to_lowercase()))
        }
        ("core.text.unicode", "upper") => {
            Ok(CtValue::Str(as_string(one(0)?, span)?.to_uppercase()))
        }
        ("core.text.unicode", "scalars") => Ok(CtValue::List(
            as_string(one(0)?, span)?
                .chars()
                .map(CtValue::Char)
                .collect(),
        )),
        // --- impure / build-time I/O → teaching diagnostic (reached only when
        // no #Impure gate intercepts first in eval_method) ---
        ("core.files", _)
        | ("core.env", _)
        | ("core.io", _)
        | ("core.exec", _)
        | ("core.net", _) => Err(Diagnostic::error(
            "E3410",
            format!(
                "`{}.{}()` is a Tier-2 comptime effect — it requires a `#Impure` gate",
                module, method
            ),
            "ambient I/O (filesystem, environment, process) is not allowed in \
                 pure comptime evaluation"
                .to_string(),
            format!(
                "wrap the comptime binding in `#Impure(\"reason\") {{ … }}` and \
                         pass `--allow-impure` to the build"
            ),
            Some(span),
        )),
        // --- unknown / not yet whitelisted ---
        _ => {
            if repl_mode {
                if let Some(_) = repl_native_only_module(module) {
                    return Err(repl_native_module_diag(module, method, span));
                }
            }
            Err(unsupported(
                &format!("`{}.{}()` at comptime", module, method),
                span,
            ))
        }
    }
}

fn regex_pattern(args: &[CtValue], span: Span) -> Result<regex::Regex, Diagnostic> {
    let pat = as_string(
        args.first()
            .ok_or_else(|| unsupported("regex call: missing pattern argument", span))?,
        span,
    )?;
    regex::Regex::new(pat).map_err(|e| {
        Diagnostic::error(
            "E0956",
            format!("bad regex pattern: {}", e),
            "the pattern could not be compiled".to_string(),
            "fix the pattern syntax".to_string(),
            Some(span),
        )
    })
}

fn regex_is_match(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let re = regex_pattern(&args, span)?;
    let text = as_string(
        args.get(1)
            .ok_or_else(|| unsupported("regex.is_match: missing text argument", span))?,
        span,
    )?;
    Ok(CtValue::ResOk(Box::new(CtValue::Bool(re.is_match(text)))))
}

fn regex_find(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let re = regex_pattern(&args, span)?;
    let text = as_string(
        args.get(1)
            .ok_or_else(|| unsupported("regex.find: missing text argument", span))?,
        span,
    )?;
    Ok(CtValue::ResOk(Box::new(match re.find(text) {
        Some(m) => CtValue::Some(Box::new(CtValue::Str(m.as_str().to_string()))),
        None => CtValue::None(crate::AST::Type::String),
    })))
}

fn regex_find_all(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let re = regex_pattern(&args, span)?;
    let text = as_string(
        args.get(1)
            .ok_or_else(|| unsupported("regex.find_all: missing text argument", span))?,
        span,
    )?;
    let items: Vec<CtValue> = re
        .find_iter(text)
        .map(|m| CtValue::Str(m.as_str().to_string()))
        .collect();
    Ok(CtValue::ResOk(Box::new(CtValue::List(items))))
}

fn regex_split(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let re = regex_pattern(&args, span)?;
    let text = as_string(
        args.get(1)
            .ok_or_else(|| unsupported("regex.split: missing text argument", span))?,
        span,
    )?;
    let items: Vec<CtValue> = re
        .split(text)
        .map(|s| CtValue::Str(s.to_string()))
        .collect();
    Ok(CtValue::ResOk(Box::new(CtValue::List(items))))
}

fn regex_replace(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let re = regex_pattern(&args, span)?;
    let text = as_string(
        args.get(1)
            .ok_or_else(|| unsupported("regex.replace: missing text argument", span))?,
        span,
    )?;
    let rep = as_string(
        args.get(2)
            .ok_or_else(|| unsupported("regex.replace: missing replacement argument", span))?,
        span,
    )?;
    Ok(CtValue::ResOk(Box::new(CtValue::Str(
        re.replace_all(text, rep).into_owned(),
    ))))
}

fn regex_match(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let re = regex_pattern(&args, span)?;
    let text = as_string(
        args.get(1)
            .ok_or_else(|| unsupported("regex.match: missing text argument", span))?,
        span,
    )?;
    Ok(CtValue::ResOk(Box::new(match re.captures(text) {
        Some(caps) => {
            let groups: Vec<CtValue> = (0..caps.len())
                .map(|i| {
                    caps.get(i)
                        .map(|m| CtValue::Some(Box::new(CtValue::Str(m.as_str().to_string()))))
                        .unwrap_or_else(|| CtValue::None(crate::AST::Type::String))
                })
                .collect();
            CtValue::Some(Box::new(CtValue::Struct {
                type_name: "Match".to_string(),
                fields: vec![("groups".to_string(), CtValue::List(groups))],
            }))
        }
        None => CtValue::None(crate::AST::Type::Named("Match".to_string())),
    })))
}

/// D-CTEFFECT1: execute a Tier-2 ambient comptime I/O effect (or REPL sandbox I/O).
/// Only called from `eval_method` when `impure_depth > 0` and `allow_impure`.
fn apply_impure_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    base_dir: &std::path::Path,
    sink: Option<&mut super::Interpreter::DevSink>,
) -> Result<CtValue, Diagnostic> {
    let one = |i: usize| {
        args.get(i).ok_or_else(|| {
            unsupported(
                &format!("`{}.{}` (wrong number of arguments)", module, method),
                span,
            )
        })
    };
    match (module, method) {
        ("core.files", "read") => {
            let path_str = as_string(one(0)?, span)?;
            let path = base_dir.join(path_str);
            match std::fs::read_to_string(&path) {
                Ok(s) => Ok(CtValue::ResOk(Box::new(CtValue::Str(s)))),
                Err(e) => Ok(CtValue::ResErr(Box::new(io_error_value(
                    &path.to_string_lossy(),
                    e,
                )))),
            }
        }
        ("core.files", "read_bytes") => {
            let path_str = as_string(one(0)?, span)?;
            let path = base_dir.join(path_str);
            match std::fs::read(&path) {
                Ok(bs) => Ok(CtValue::ResOk(Box::new(CtValue::Bytes(bs)))),
                Err(e) => Ok(CtValue::ResErr(Box::new(io_error_value(
                    &path.to_string_lossy(),
                    e,
                )))),
            }
        }
        // D-FILES-APPEND1=A: whole-file one-shot is `append_all` (not `append`,
        // which names the streaming handle's method).
        ("core.files", "write" | "append_all") => {
            let path_str = as_string(one(0)?, span)?;
            let content = as_string(one(1)?, span)?;
            let path = base_dir.join(path_str);
            let result = if method == "append_all" {
                use std::io::Write;
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .and_then(|mut f| f.write_all(content.as_bytes()).map(|_| ()))
            } else {
                std::fs::write(&path, content)
            };
            match result {
                Ok(()) => Ok(CtValue::ResOk(Box::new(CtValue::Unit))),
                Err(e) => Ok(CtValue::ResErr(Box::new(io_error_value(
                    &path.to_string_lossy(),
                    e,
                )))),
            }
        }
        ("core.files", "exists" | "is_dir") => {
            let path_str = as_string(one(0)?, span)?;
            let path = base_dir.join(path_str);
            let meta = std::fs::metadata(&path);
            Ok(CtValue::Bool(match (method, meta) {
                ("exists", Ok(_)) => true,
                ("exists", Err(_)) => false,
                ("is_dir", Ok(m)) => m.is_dir(),
                ("is_dir", Err(_)) => false,
                _ => false,
            }))
        }
        ("core.files", "create_dir") => {
            let path_str = as_string(one(0)?, span)?;
            let path = base_dir.join(path_str);
            match std::fs::create_dir_all(&path) {
                Ok(()) => Ok(CtValue::ResOk(Box::new(CtValue::Unit))),
                Err(e) => Ok(CtValue::ResErr(Box::new(io_error_value(
                    &path.to_string_lossy(),
                    e,
                )))),
            }
        }
        ("core.files", "remove") => {
            let path_str = as_string(one(0)?, span)?;
            let path = base_dir.join(path_str);
            let result = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            match result {
                Ok(()) => Ok(CtValue::ResOk(Box::new(CtValue::Unit))),
                Err(e) => Ok(CtValue::ResErr(Box::new(io_error_value(
                    &path.to_string_lossy(),
                    e,
                )))),
            }
        }
        ("core.env", "get") => {
            let key = as_string(one(0)?, span)?;
            match std::env::var(key) {
                Ok(v) => Ok(CtValue::Str(v)),
                Err(_) => Ok(CtValue::None(crate::AST::Type::String)),
            }
        }
        ("core.env", "set") => {
            let key = as_string(one(0)?, span)?;
            let val = as_string(one(1)?, span)?;
            std::env::set_var(key, val);
            Ok(CtValue::Unit)
        }
        ("core.env", "current_dir") => match std::env::current_dir() {
            Ok(p) => Ok(CtValue::ResOk(Box::new(CtValue::Str(
                p.to_string_lossy().into_owned(),
            )))),
            Err(e) => Ok(CtValue::ResErr(Box::new(io_error_value(".", e)))),
        },
        ("core.env", "home_dir") => Ok(
            match std::env::var("HOME")
                .ok()
                .or_else(|| std::env::var("USERPROFILE").ok())
            {
                Some(v) => CtValue::Some(Box::new(CtValue::Str(v))),
                None => CtValue::None(crate::AST::Type::String),
            },
        ),
        ("core.io", "args") => Ok(CtValue::List(
            std::env::args()
                .skip(1)
                .map(CtValue::Str)
                .collect::<Vec<_>>(),
        )),
        ("core.io", "eprint") => {
            let text = match args.first() {
                Some(v) => v.jet_show(),
                None => String::new(),
            };
            if let Some(s) = sink {
                s.stderr.push_str(&text);
                s.stderr.push('\n');
            }
            Ok(CtValue::Unit)
        }
        ("core.io", "input") | ("core.io", "read_all_input") => {
            Ok(CtValue::ResOk(Box::new(CtValue::Str(String::new()))))
        }
        ("core.io", "stdin") => Ok(CtValue::Struct {
            type_name: "StdinHandle".to_string(),
            fields: vec![],
        }),
        ("core.process", "exit") => {
            let code = match one(0)? {
                CtValue::Int(n) => *n,
                _ => 0,
            };
            std::process::exit(code as i32);
        }
        ("core.process", "run") => {
            let cmd = match one(0)? {
                CtValue::List(items) => items.iter().map(|v| v.jet_show()).collect::<Vec<_>>(),
                _ => {
                    return Err(unsupported(
                        "process.run expects a list of command words",
                        span,
                    ))
                }
            };
            if cmd.is_empty() {
                return Ok(CtValue::ResErr(Box::new(CtValue::Struct {
                    type_name: "IoError".to_string(),
                    fields: vec![(
                        "message".to_string(),
                        CtValue::Str("process.run needs at least one command word".to_string()),
                    )],
                })));
            }
            match std::process::Command::new(&cmd[0]).args(&cmd[1..]).output() {
                Ok(out) => Ok(CtValue::ResOk(Box::new(CtValue::Struct {
                    type_name: "ProcessResult".to_string(),
                    fields: vec![
                        (
                            "code".to_string(),
                            CtValue::Int(out.status.code().unwrap_or(-1) as i64),
                        ),
                        (
                            "output".to_string(),
                            CtValue::Str(String::from_utf8_lossy(&out.stdout).into_owned()),
                        ),
                        (
                            "errors".to_string(),
                            CtValue::Str(String::from_utf8_lossy(&out.stderr).into_owned()),
                        ),
                    ],
                }))),
                Err(e) => Ok(CtValue::ResErr(Box::new(io_error_value(&cmd[0], e)))),
            }
        }
        // E3412: other core.net methods not yet implemented at comptime.
        ("core.net", _) => Err(Diagnostic::error(
            "E3412",
            format!("`core.net.{}()` is not available at comptime", method),
            "only `core.net.fetch(url, sha256:)` is supported at compile time".to_string(),
            "use `core.net.fetch(url, sha256: \"<hash>\")` for content-hash-pinned downloads"
                .to_string(),
            Some(span),
        )),
        _ => Err(unsupported(
            &format!("`{}.{}()` at comptime (impure tier)", module, method),
            span,
        )),
    }
}
