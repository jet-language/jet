//! Call/method dispatch for the interpreter: `eval_call`, `eval_method`,
//! `eval_fan_out`, `eval_require`, `eval_embed_file`. These are further
//! `impl Interp` methods; the struct and spine live in `interp.rs`.

use std::collections::HashMap;

use crate::AST::{CallArg, Expr, StrPart};
use crate::Diagnostics::{Diagnostic, Span};

use super::Builtins::{apply_method, apply_mutating, apply_static_type_method, as_bool};
use super::Diagnostics::{ERR_PROPAGATE_CODE, EARLY_RETURN_CODE};
use super::Diagnostics::{comptime_panic, unsupported};
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
                    Some(a) => self.eval(&a.expr, scope)?.jet_show(),
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
        // A user function: bind params, run the body in a fresh frame.
        let func = self
            .funcs
            .get(name)
            .copied()
            .ok_or_else(|| unsupported(&format!("calling `{}`", name), span))?;
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

    fn eval_embed_file(
        &mut self,
        args: &[CallArg],
        span: Span,
    ) -> Result<CtValue, Diagnostic> {
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
    fn eval_embed_bytes(
        &mut self,
        args: &[CallArg],
        span: Span,
    ) -> Result<CtValue, Diagnostic> {
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
                self.embed_inputs.push(crate::Lock::ComptimeInput {
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
                // D-CTEFFECT1: Tier-2 effect calls require an #Impure gate.
                let is_tier2 = matches!(module.as_str(),
                    "core.fs" | "core.env" | "core.io" | "core.exec" | "core.net");
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
                    return apply_impure_core_call(&module, method, argv, span, self.base_dir);
                }
                return apply_core_call(&module, method, argv, span);
            }
        }

        // Mutating list/map methods on a named variable write back in place.
        const MUTATING: &[&str] = &[
            "push", "pop", "insert", "remove", "clear", "reverse", "sort",
        ];
        if MUTATING.contains(&method) {
            if let Expr::Ident(bname, _) = receiver {
                let mut container = scope
                    .get(bname)
                    .cloned()
                    .ok_or_else(|| unsupported(&format!("the name `{}`", bname), span))?;
                let mut argv = Vec::new();
                for a in args {
                    argv.push(self.eval(&a.expr, scope)?);
                }
                let ret = apply_mutating(&mut container, method, argv, span)?;
                scope.insert(bname.clone(), container);
                return Ok(ret);
            }
        }
        let recv = self.eval(receiver, scope)?;
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
        _ => Err(unsupported("non-numeric argument to comptime math call", span)),
    }
}

fn as_string(v: &CtValue, span: Span) -> Result<&str, Diagnostic> {
    match v {
        CtValue::Str(s) => Ok(s.as_str()),
        _ => Err(unsupported("non-string argument to comptime string call", span)),
    }
}

/// Evaluate a whitelisted pure Core call at comptime.
/// `module` is the full path (e.g. `"core.math"`, `"core.string"`).
fn apply_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let one = |i: usize| {
        args.get(i)
            .ok_or_else(|| unsupported(&format!("{}.{}(): missing arg {}", module, method, i), span))
    };

    match (module, method) {
        // --- core.math whitelist ---
        ("core.math", "sqrt") => Ok(CtValue::Float(as_float(one(0)?, span)?.sqrt())),
        ("core.math", "floor") => Ok(CtValue::Float(as_float(one(0)?, span)?.floor())),
        ("core.math", "ceil") => Ok(CtValue::Float(as_float(one(0)?, span)?.ceil())),
        ("core.math", "round") => Ok(CtValue::Int(as_float(one(0)?, span)?.round() as i64)),
        ("core.math", "abs") => {
            match one(0)? {
                CtValue::Int(n) => Ok(CtValue::Int(n.abs())),
                CtValue::Float(f) => Ok(CtValue::Float(f.abs())),
                _ => Err(unsupported("core.math.abs: non-numeric argument", span)),
            }
        }
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
        // --- impure / build-time I/O → teaching diagnostic (reached only when
        // no #Impure gate intercepts first in eval_method) ---
        ("core.fs", _) | ("core.env", _) | ("core.io", _) | ("core.exec", _) | ("core.net", _) => {
            Err(Diagnostic::error(
                "E3410",
                format!("`{}.{}()` is a Tier-2 comptime effect — it requires a `#Impure` gate", module, method),
                "ambient I/O (filesystem, environment, process) is not allowed in \
                 pure comptime evaluation".to_string(),
                format!("wrap the comptime binding in `#Impure(\"reason\") {{ … }}` and \
                         pass `--allow-impure` to the build"),
                Some(span),
            ))
        }
        // --- unknown / not yet whitelisted ---
        _ => Err(unsupported(
            &format!("`{}.{}()` at comptime", module, method),
            span,
        )),
    }
}

/// D-CTEFFECT1: execute a Tier-2 ambient comptime I/O effect.
/// Only called from `eval_method` when `impure_depth > 0` and `allow_impure`.
/// Supports: `core.fs.read`, `core.env.get`. `core.net.*` is E3412 pending ballot.
fn apply_impure_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    base_dir: &std::path::Path,
) -> Result<CtValue, Diagnostic> {
    let one = |i: usize| args.get(i).ok_or_else(|| unsupported(
        &format!("`{}.{}` (wrong number of arguments)", module, method),
        span,
    ));
    match (module, method) {
        ("core.fs", "read") => {
            let path_str = as_string(one(0)?, span)?;
            let path = base_dir.join(path_str);
            match std::fs::read_to_string(&path) {
                Ok(s) => Ok(CtValue::Str(s)),
                Err(e) => Err(Diagnostic::error(
                    "E3410",
                    format!("comptime `core.fs.read({:?})` failed: {}", path, e),
                    "the file could not be read at compile time".to_string(),
                    "check that the path is correct and readable".to_string(),
                    Some(span),
                )),
            }
        }
        ("core.env", "get") => {
            let key = as_string(one(0)?, span)?;
            match std::env::var(key) {
                Ok(v) => Ok(CtValue::Str(v)),
                // env var absent → null (Option None)
                Err(_) => Ok(CtValue::None(crate::AST::Type::String)),
            }
        }
        // E3412: network calls pending owner ballot on D-NETDEP1.
        ("core.net", _) => Err(Diagnostic::error(
            "E3412",
            format!("`core.net.{}()` is not available at comptime yet", method),
            "network access at compile time requires a fetch mechanism with \
             content-hash pinning, which is pending an owner decision".to_string(),
            "use `embed_file` or `embed_bytes` for local build-time data; \
             track D-NETDEP1 in Tower for network support".to_string(),
            Some(span),
        )),
        _ => Err(unsupported(
            &format!("`{}.{}()` at comptime (impure tier)", module, method),
            span,
        )),
    }
}
