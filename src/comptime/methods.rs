//! Call/method dispatch for the interpreter: `eval_call`, `eval_method`,
//! `eval_fan_out`, `eval_require`, `eval_embed_file`. These are further
//! `impl Interp` methods; the struct and spine live in `interp.rs`.

use std::collections::HashMap;

use crate::ast::Expr;
use crate::diag::{Diagnostic, Span};

use super::builtins::{apply_method, apply_mutating, as_bool};
use super::diag::{comptime_panic, unsupported};
use super::interp::{Flow, Interp};
use super::value::CtValue;

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
        args: &[crate::ast::CallArg],
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
        // The two sanctioned comptime builtins.
        if name == "embed_file" {
            return self.eval_embed_file(args, span, scope);
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
        match self.exec_block(&func.body, &mut frame)? {
            Flow::Return(v) => Ok(v),
            _ => Ok(CtValue::Unit),
        }
    }

    fn eval_require(
        &mut self,
        name: &str,
        args: &[crate::ast::CallArg],
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
        args: &[crate::ast::CallArg],
        span: Span,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let arg = args
            .first()
            .ok_or_else(|| unsupported("embed_file with no path", span))?;
        let path_val = self.eval(&arg.expr, scope)?;
        let CtValue::Str(rel) = path_val else {
            return Err(unsupported("embed_file with a non-text path", span));
        };
        let full = self.base_dir.join(&rel);
        match std::fs::read(&full) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => Ok(CtValue::Str(text)),
                Err(_) => Err(Diagnostic::error(
                    "E0955",
                    format!("`embed_file` can't read `{}` as text", rel),
                    "the file isn't valid UTF-8; comptime `embed_file` returns a String in v1"
                        .to_string(),
                    "embed a text file, or wait for the byte-buffer version (M10)".to_string(),
                    Some(span),
                )),
            },
            Err(e) => Err(Diagnostic::error(
                "E0955",
                format!("`embed_file` can't open `{}`", rel),
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
        args: &[crate::ast::CallArg],
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
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
