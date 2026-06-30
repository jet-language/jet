//! Call/method dispatch for the interpreter: `eval_call`, `eval_method`,
//! `eval_fan_out`, `eval_require`, `eval_embed_file`. These are further
//! `impl Interp` methods; the struct and spine live in `interp.rs`.

use std::collections::HashMap;

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{CallArg, Expr, StrPart};

use super::Builtins::{apply_method, apply_mutating, apply_static_type_method, as_bool};
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
    ///   4. Unreachable / fetch error → **E3414**.
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

        let bytes = jet_net::fetch(&url).map_err(|e| Diagnostic::error(
            "E3414",
            format!("fetch failed: {e}"),
            format!("could not retrieve `{url}`"),
            "check the URL is reachable and the network is available; use `file://` for local paths".to_string(),
            Some(span),
        ))?;

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
                // D-CTEFFECT1: Tier-2 effect calls require an #Impure gate (or REPL sandbox).
                let is_tier2 = matches!(
                    module.as_str(),
                    "core.fs" | "core.env" | "core.io" | "core.exec" | "core.net" | "core.process"
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
                Err(e) => Ok(CtValue::ResErr(Box::new(super::JsonInterp::json_error_value(
                    e,
                )))),
            }
        }
        ("core.encoding.json", "decode") => {
            // Lenient decode: same as parse for the dynamic interpreter value.
            let text = as_string(one(0)?, span)?;
            match super::JsonInterp::parse_json(text) {
                Ok(v) => Ok(CtValue::ResOk(Box::new(v))),
                Err(e) => Ok(CtValue::ResErr(Box::new(super::JsonInterp::json_error_value(
                    e,
                )))),
            }
        }
        ("core.encoding.json", "to_string") => {
            let v = one(0)?;
            Ok(CtValue::Str(super::JsonInterp::render_json_pretty(v, false, 0)))
        }
        ("core.encoding.json", "to_string_pretty") => {
            let v = one(0)?;
            Ok(CtValue::Str(super::JsonInterp::render_json_pretty(v, true, 0)))
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
            Ok(CtValue::Int(with_ambient_rng(|st| random_int(st, low, high))))
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
        // --- impure / build-time I/O → teaching diagnostic (reached only when
        // no #Impure gate intercepts first in eval_method) ---
        ("core.fs", _) | ("core.env", _) | ("core.io", _) | ("core.exec", _) | ("core.net", _) => {
            Err(Diagnostic::error(
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
            ))
        }
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
    let pat = as_string(args.first().ok_or_else(|| {
        unsupported("regex call: missing pattern argument", span)
    })?, span)?;
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
    let text = as_string(args.get(1).ok_or_else(|| {
        unsupported("regex.is_match: missing text argument", span)
    })?, span)?;
    Ok(CtValue::ResOk(Box::new(CtValue::Bool(re.is_match(text)))))
}

fn regex_find(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let re = regex_pattern(&args, span)?;
    let text = as_string(args.get(1).ok_or_else(|| {
        unsupported("regex.find: missing text argument", span)
    })?, span)?;
    Ok(CtValue::ResOk(Box::new(match re.find(text) {
        Some(m) => CtValue::Some(Box::new(CtValue::Str(m.as_str().to_string()))),
        None => CtValue::None(crate::AST::Type::String),
    })))
}

fn regex_find_all(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let re = regex_pattern(&args, span)?;
    let text = as_string(args.get(1).ok_or_else(|| {
        unsupported("regex.find_all: missing text argument", span)
    })?, span)?;
    let items: Vec<CtValue> = re
        .find_iter(text)
        .map(|m| CtValue::Str(m.as_str().to_string()))
        .collect();
    Ok(CtValue::ResOk(Box::new(CtValue::List(items))))
}

fn regex_split(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let re = regex_pattern(&args, span)?;
    let text = as_string(args.get(1).ok_or_else(|| {
        unsupported("regex.split: missing text argument", span)
    })?, span)?;
    let items: Vec<CtValue> = re
        .split(text)
        .map(|s| CtValue::Str(s.to_string()))
        .collect();
    Ok(CtValue::ResOk(Box::new(CtValue::List(items))))
}

fn regex_replace(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let re = regex_pattern(&args, span)?;
    let text = as_string(args.get(1).ok_or_else(|| {
        unsupported("regex.replace: missing text argument", span)
    })?, span)?;
    let rep = as_string(args.get(2).ok_or_else(|| {
        unsupported("regex.replace: missing replacement argument", span)
    })?, span)?;
    Ok(CtValue::ResOk(Box::new(CtValue::Str(
        re.replace_all(text, rep).into_owned(),
    ))))
}

fn regex_match(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let re = regex_pattern(&args, span)?;
    let text = as_string(args.get(1).ok_or_else(|| {
        unsupported("regex.match: missing text argument", span)
    })?, span)?;
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
        ("core.fs", "read") => {
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
        ("core.fs", "read_bytes") => {
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
        ("core.fs", "write" | "append") => {
            let path_str = as_string(one(0)?, span)?;
            let content = as_string(one(1)?, span)?;
            let path = base_dir.join(path_str);
            let result = if method == "append" {
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
        ("core.fs", "exists" | "is_dir") => {
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
        ("core.fs", "create_dir") => {
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
        ("core.fs", "remove") => {
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
        ("core.env", "home_dir") => Ok(match std::env::var("HOME")
            .ok()
            .or_else(|| std::env::var("USERPROFILE").ok())
        {
            Some(v) => CtValue::Some(Box::new(CtValue::Str(v))),
            None => CtValue::None(crate::AST::Type::String),
        }),
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
        ("core.io", "input") | ("core.io", "read_all_input") => Ok(CtValue::ResOk(Box::new(
            CtValue::Str(String::new()),
        ))),
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
                CtValue::List(items) => items
                    .iter()
                    .map(|v| v.jet_show())
                    .collect::<Vec<_>>(),
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
            match std::process::Command::new(&cmd[0])
                .args(&cmd[1..])
                .output()
            {
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
