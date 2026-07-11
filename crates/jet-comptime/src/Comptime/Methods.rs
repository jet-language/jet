//! Call/method dispatch for the interpreter: `eval_call`, `eval_method`,
//! `eval_fan_out`, `eval_require`, `eval_embed_file`. These are further
//! `impl Interp` methods; the struct and spine live in `interp.rs`.

use std::collections::HashMap;
use std::path::Path;

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
/// `builtin` names the function (`embed_file` / `embed_bytes` / `find`) for the message.
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
    let noun = if builtin == crate::Syntax::BUILTIN_FIND {
        "glob"
    } else {
        "path"
    };
    let inline_example = if builtin == crate::Syntax::BUILTIN_FIND {
        format!("{builtin}(\"content/**/*.jet\")")
    } else {
        format!("{builtin}(\"data/file\")")
    };
    let (msg, why, fix) = match kind {
        "literal" => (
            format!("`{builtin}` {noun} must be a string literal"),
            format!("a computed {noun} can't be audited at build time, so the compiler can't tell which file it reads"),
            format!("pass the {noun} inline, e.g. `{inline_example}`"),
        ),
        "absolute" => (
            format!("`{builtin}` {noun} must be relative"),
            format!("an absolute {noun} reaches outside the project"),
            format!("use a {noun} relative to this file's own directory"),
        ),
        _ => (
            format!("`{builtin}` {noun} escapes the project with `..`"),
            "`..` could read files outside your project; comptime inputs must stay inside it".to_string(),
            format!("drop the `..` and use a {noun} under this file's directory"),
        ),
    };
    Diagnostic::error("E0957", msg, why, fix, Some(span))
}

fn find_glob(base_dir: &Path, glob: &str, span: Span) -> Result<Vec<String>, Diagnostic> {
    let pattern = normalize_rel(glob);
    let segments = split_rel(&pattern);
    let root_rel = static_glob_prefix(&segments);
    let root = if root_rel.is_empty() {
        base_dir.to_path_buf()
    } else {
        base_dir.join(root_rel.join("/"))
    };
    let has_recursive = segments.iter().any(|segment| segment == "**");
    let max_depth = if has_recursive {
        usize::MAX
    } else {
        segments.len()
    };
    let mut out = Vec::new();
    if root.is_file() {
        let rel = root_rel.join("/");
        if glob_segments_match(&segments, &split_rel(&rel)) {
            out.push(rel);
        }
        return Ok(out);
    }
    if !root.exists() {
        return Ok(out);
    }
    walk_find(
        base_dir,
        &root,
        &segments,
        root_rel.len(),
        max_depth,
        &mut out,
    )
    .map_err(|e| {
        Diagnostic::error(
            "E0955",
            format!("`{}` can't walk `{glob}`", crate::Syntax::BUILTIN_FIND),
            e.to_string(),
            "check that every directory matched by the glob can be read".to_string(),
            Some(span),
        )
    })?;
    Ok(out)
}

fn normalize_rel(path: &str) -> String {
    path.replace('\\', "/")
}

fn split_rel(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .map(str::to_string)
        .collect()
}

fn static_glob_prefix(segments: &[String]) -> Vec<String> {
    let mut prefix = Vec::new();
    for segment in segments {
        if segment == "**" || segment_has_glob(segment) {
            break;
        }
        prefix.push(segment.clone());
    }
    prefix
}

fn segment_has_glob(segment: &str) -> bool {
    segment.chars().any(|c| matches!(c, '*' | '?' | '{' | '['))
}

fn walk_find(
    base_dir: &Path,
    dir: &Path,
    pattern: &[String],
    depth: usize,
    max_depth: usize,
    out: &mut Vec<String>,
) -> std::io::Result<()> {
    let mut entries = std::fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let ty = entry.file_type()?;
        let path = entry.path();
        if ty.is_dir() {
            let dir_depth = depth + 1;
            if dir_depth < max_depth {
                walk_find(base_dir, &path, pattern, dir_depth, max_depth, out)?;
            }
        } else if ty.is_file() {
            let rel = path.strip_prefix(base_dir).unwrap_or(&path);
            let rel = normalize_rel(&rel.to_string_lossy());
            if glob_segments_match(pattern, &split_rel(&rel)) {
                out.push(rel);
            }
        }
    }
    Ok(())
}

fn glob_segments_match(pattern: &[String], path: &[String]) -> bool {
    if pattern.is_empty() {
        return path.is_empty();
    }
    if pattern[0] == "**" {
        return glob_segments_match(&pattern[1..], path)
            || (!path.is_empty() && glob_segments_match(pattern, &path[1..]));
    }
    !path.is_empty()
        && segment_match(&pattern[0], &path[0])
        && glob_segments_match(&pattern[1..], &path[1..])
}

fn segment_match(pattern: &str, text: &str) -> bool {
    segment_match_chars(
        &pattern.chars().collect::<Vec<_>>(),
        &text.chars().collect::<Vec<_>>(),
    )
}

fn segment_match_chars(pattern: &[char], text: &[char]) -> bool {
    if pattern.is_empty() {
        return text.is_empty();
    }
    match pattern[0] {
        '*' => {
            segment_match_chars(&pattern[1..], text)
                || (!text.is_empty() && segment_match_chars(pattern, &text[1..]))
        }
        '?' => !text.is_empty() && segment_match_chars(&pattern[1..], &text[1..]),
        '{' => match find_closing(pattern, 0, '}') {
            Some(end) => split_alternatives(&pattern[1..end]).into_iter().any(|alt| {
                let mut next = alt;
                next.extend_from_slice(&pattern[end + 1..]);
                segment_match_chars(&next, text)
            }),
            None => {
                !text.is_empty() && text[0] == '{' && segment_match_chars(&pattern[1..], &text[1..])
            }
        },
        '[' => match find_closing(pattern, 0, ']') {
            Some(end) => {
                !text.is_empty()
                    && class_matches(&pattern[1..end], text[0])
                    && segment_match_chars(&pattern[end + 1..], &text[1..])
            }
            None => {
                !text.is_empty() && text[0] == '[' && segment_match_chars(&pattern[1..], &text[1..])
            }
        },
        c => !text.is_empty() && text[0] == c && segment_match_chars(&pattern[1..], &text[1..]),
    }
}

fn find_closing(chars: &[char], start: usize, close: char) -> Option<usize> {
    chars
        .iter()
        .enumerate()
        .skip(start + 1)
        .find_map(|(idx, c)| (*c == close).then_some(idx))
}

fn split_alternatives(chars: &[char]) -> Vec<Vec<char>> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    for c in chars {
        if *c == ',' {
            out.push(cur);
            cur = Vec::new();
        } else {
            cur.push(*c);
        }
    }
    out.push(cur);
    out
}

fn class_matches(class: &[char], needle: char) -> bool {
    let mut i = 0;
    let mut matched = false;
    while i < class.len() {
        if i + 2 < class.len() && class[i + 1] == '-' {
            matched |= class[i] <= needle && needle <= class[i + 2];
            i += 3;
        } else {
            matched |= class[i] == needle;
            i += 1;
        }
    }
    matched
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
        if name == crate::Syntax::BUILTIN_FIND {
            return self.eval_find(args, span);
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
        // D-BIGINT1: `BigInt(100)` / `BigInt("999…")` — explicit construction
        // only, same arg shape sema already validated (E0103/E0128). Checked
        // ahead of the user-function/closure lookups, same as the distinct-
        // type-ctor case below: `BigInt` is never a real `fn` name.
        if name == crate::Syntax::TYPE_BIGINT
            && self.funcs.get(name).is_none()
            && !scope.contains_key(name)
        {
            let arg = match args.first() {
                Some(a) => self.eval(&a.expr, scope)?,
                None => return Err(unsupported("`BigInt` with no argument", span)),
            };
            return match arg {
                CtValue::Int(n) => Ok(CtValue::BigInt(crate::Numeric::CtBigInt::from_int(n))),
                CtValue::Str(s) => crate::Numeric::CtBigInt::from_str(&s)
                    .map(CtValue::BigInt)
                    .map_err(|_| unsupported(&format!("`BigInt(\"{}\")`", s), span)),
                _ => Err(unsupported("`BigInt` with a non-Int/String argument", span)),
            };
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
        let qualified = name.split_once('.').map(|(module, symbol)| format!("{module}::{symbol}"));
        let func = match self.funcs.get(name).copied().or_else(|| qualified.as_ref().and_then(|name| self.funcs.get(name).copied())) {
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
    pub(super) fn call_func(
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
        Ok(self.display_fallback_value(v))
    }

    fn display_fallback_value(&self, v: &CtValue) -> String {
        match v {
            CtValue::Struct { type_name, fields } => {
                let Some(def) = self.structs.get(type_name) else {
                    return v.jet_show();
                };
                if def
                    .fields
                    .iter()
                    .any(|field| matches!(field.ty, Type::Fn { .. }))
                {
                    return format!("{type_name} {{ ... }}");
                }
                if def.type_params.is_empty() {
                    let parts: Vec<String> = def
                        .fields
                        .iter()
                        .filter(|field| field.computed.is_none())
                        .map(|field| {
                            let rendered = fields
                                .iter()
                                .find(|(name, _)| name == &field.name)
                                .map(|(_, value)| value.debug_rust())
                                .unwrap_or_else(|| CtValue::Unit.debug_rust());
                            format!("user_{}: {}", field.name, rendered)
                        })
                        .collect();
                    return format!("user_{type_name} {{ {} }}", parts.join(", "));
                }
                let parts: Vec<String> = def
                    .fields
                    .iter()
                    .map(|field| {
                        let rendered = fields
                            .iter()
                            .find(|(name, _)| name == &field.name)
                            .map(|(_, value)| value.jet_show())
                            .unwrap_or_else(|| CtValue::Unit.jet_show());
                        format!("{}: {}", field.name, rendered)
                    })
                    .collect();
                format!("{type_name}({})", parts.join(", "))
            }
            _ => v.jet_show(),
        }
    }

    pub(super) fn debug_value(&self, v: &CtValue) -> String {
        match v {
            CtValue::Struct { type_name, fields } => {
                let Some(def) = self.structs.get(type_name) else {
                    return v.debug_rust();
                };
                if def
                    .fields
                    .iter()
                    .any(|field| matches!(field.ty, Type::Fn { .. }))
                {
                    return format!("{type_name} {{ ... }}");
                }
                if def.fields.is_empty() {
                    return format!("{type_name} {{}}");
                }
                let parts: Vec<String> = def
                    .fields
                    .iter()
                    .map(|field| {
                        if field.redact {
                            format!("{}: [redacted]", field.name)
                        } else {
                            let rendered = fields
                                .iter()
                                .find(|(name, _)| name == &field.name)
                                .map(|(_, value)| value.debug_rust())
                                .unwrap_or_else(|| CtValue::Unit.debug_rust());
                            format!("{}: {}", field.name, rendered)
                        }
                    })
                    .collect();
                format!("{type_name} {{ {} }}", parts.join(", "))
            }
            _ => v.debug_rust(),
        }
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

    /// D-CTFIND1/2: `find(glob) -> [String]` walks inside the source file's
    /// directory, returns sorted relative file paths, and records each match's
    /// hash as Tier-1 lock evidence.
    fn eval_find(&mut self, args: &[CallArg], span: Span) -> Result<CtValue, Diagnostic> {
        let builtin = crate::Syntax::BUILTIN_FIND;
        let arg = args
            .first()
            .ok_or_else(|| unsupported(&format!("{builtin} with no glob"), span))?;
        let glob = check_embed_path(builtin, arg, span)?;
        if args.len() != 1 {
            return Err(unsupported(
                &format!("{builtin} with extra arguments"),
                span,
            ));
        }
        let mut matches = find_glob(self.base_dir, &glob, span)?;
        matches.sort();
        for rel in &matches {
            let full = self.base_dir.join(rel);
            let bytes = std::fs::read(&full).map_err(|e| {
                Diagnostic::error(
                    "E0955",
                    format!("`{builtin}` can't open `{rel}`"),
                    format!("{} (matched while expanding `{glob}`)", e),
                    "check the glob and remove unreadable files from its match set".to_string(),
                    Some(span),
                )
            })?;
            self.embed_inputs.push(crate::AST::ComptimeInput {
                path: rel.clone(),
                hash: crate::SHA256::sha256_hex(&bytes),
            });
        }
        Ok(CtValue::List(
            matches.into_iter().map(CtValue::Str).collect(),
        ))
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
                // D-DET1: `random.shuffle(&xs)` edits its list in place (E0202 requires
                // write access) — the one `core.random` call that mutates a caller
                // binding rather than returning a value, so it needs `write_back`
                // (the same mechanism `.sort`/`.push` use) instead of the generic
                // by-value `apply_core_call` dispatch below.
                if matches!((module.as_str(), method), ("core.random", "shuffle")) {
                    let Some(arg) = args.first() else {
                        return Err(unsupported("random.shuffle(): missing arg 0", span));
                    };
                    let list = self.eval(&arg.expr, scope)?;
                    let CtValue::List(mut items) = list else {
                        return Err(unsupported("random.shuffle needs a list", span));
                    };
                    with_ambient_rng(|st| shuffle_ct_list(st, &mut items));
                    self.write_back(&arg.expr, CtValue::List(items), scope)?;
                    return Ok(CtValue::Unit);
                }
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
                let qualified = format!("{name}::{method}");
                if let Some(f) = self.funcs.get(qualified.as_str()).copied() {
                    if f.params.len() == args.len() {
                        let mut frame = HashMap::new();
                        for (p, a) in f.params.iter().zip(args) {
                            let v = self.eval(&a.expr, scope)?;
                            frame.insert(p.name.clone(), v);
                        }
                        return self.call_func(&qualified, f, frame);
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
        let is_build_context = matches!(
            &recv,
            CtValue::Struct { type_name, .. }
                if type_name == crate::Syntax::TYPE_BUILD_CONTEXT
        );
        if is_build_context {
            match method {
                "find" => return self.eval_find(args, span),
                _ => {}
            }
        }
        let mut argv = Vec::new();
        for a in args {
            argv.push(self.eval(&a.expr, scope)?);
        }
        if is_build_context && method == "fetch" {
            return self
                .eval_net_fetch(argv, span)
                .map(|value| CtValue::ResOk(Box::new(value)));
        }
        if is_build_context && method == "embed" {
            let rel = match argv.first() {
                Some(CtValue::Str(path)) => path,
                _ => return Err(unsupported("`b.embed` requires a path string", span)),
            };
            let path = std::path::Path::new(rel);
            if path.is_absolute()
                || path.components().any(|component| matches!(component, std::path::Component::ParentDir))
            {
                return Err(Diagnostic::error(
                    "E0957",
                    format!("`b.embed` path `{rel}` escapes the build root"),
                    "locked build inputs must stay beneath the selected source directory".to_string(),
                    "use a relative path returned by `b.find`, without `..`".to_string(),
                    Some(span),
                ));
            }
            let bytes = std::fs::read(self.base_dir.join(path)).map_err(|error| Diagnostic::error(
                "E0955",
                format!("`b.embed` cannot open `{rel}`"),
                error.to_string(),
                "check the locked relative path".to_string(),
                Some(span),
            ))?;
            self.embed_inputs.push(crate::AST::ComptimeInput {
                path: rel.clone(),
                hash: crate::SHA256::sha256_hex(&bytes),
            });
            return String::from_utf8(bytes).map(CtValue::Str).map_err(|_| Diagnostic::error(
                "E0955",
                format!("`b.embed` cannot decode `{rel}` as text"),
                "the embedded file is not valid UTF-8".to_string(),
                "embed a UTF-8 text file".to_string(),
                Some(span),
            ));
        }
        // D-BUILDENTRY1: selected-root `BuildContext` is interpreter-owned.
        // Driver removes `fn build` before runtime codegen.
        if let Some(result) =
            super::Build::eval_program_build_method(
                &recv,
                method,
                argv.clone(),
                span,
                self.impure_depth > 0,
            )
        {
            return result;
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

/// D-URL1=A: `Vec<Vec<String>>`-shaped arg (`[[String]]`) — used by
/// `core.url.from_parts`'s `query` param and `core.url.query`'s pairs param,
/// mirroring AOT's `&Vec<Vec<String>>` signature.
fn as_string_rows(v: &CtValue, span: Span) -> Result<Vec<Vec<String>>, Diagnostic> {
    match v {
        CtValue::List(rows) => rows
            .iter()
            .map(|row| match row {
                CtValue::List(cols) => cols
                    .iter()
                    .map(|c| Ok(as_string(c, span)?.to_string()))
                    .collect::<Result<Vec<_>, _>>(),
                _ => Err(unsupported("core.url query rows must be `[[String]]`", span)),
            })
            .collect(),
        _ => Err(unsupported("core.url query rows must be `[[String]]`", span)),
    }
}

/// Mirrors AOT's `JetUrl` field shape 1:1 so `.scheme`/`.host`/`.path`/
/// `.query`/`.fragment` struct-field reads (generic member access,
/// `Interpreter.rs`) work the same as any other `CtValue::Struct`.
fn url_parts_to_ct(u: &super::UrlLite::UrlParts) -> CtValue {
    CtValue::Struct {
        type_name: "Url".to_string(),
        fields: vec![
            ("scheme".to_string(), CtValue::Str(u.scheme.clone())),
            (
                "host".to_string(),
                match &u.host {
                    Some(h) if !h.is_empty() => CtValue::Some(Box::new(CtValue::Str(h.clone()))),
                    _ => CtValue::None(Type::String),
                },
            ),
            (
                "port".to_string(),
                match u.port {
                    Some(p) => CtValue::Some(Box::new(CtValue::Int(p))),
                    None => CtValue::None(Type::Int),
                },
            ),
            ("path".to_string(), CtValue::Str(u.path.clone())),
            (
                "query".to_string(),
                CtValue::List(
                    u.query
                        .iter()
                        .map(|(k, v)| {
                            CtValue::List(vec![CtValue::Str(k.clone()), CtValue::Str(v.clone())])
                        })
                        .collect(),
                ),
            ),
            (
                "fragment".to_string(),
                match &u.fragment {
                    Some(f) => CtValue::Some(Box::new(CtValue::Str(f.clone()))),
                    None => CtValue::None(Type::String),
                },
            ),
        ],
    }
}

/// `[Float]` argument — `core.data`'s stats functions all take `&Vec<f64>`.
fn as_float_list(v: &CtValue, span: Span) -> Result<Vec<f64>, Diagnostic> {
    match v {
        CtValue::List(xs) => xs.iter().map(|x| as_float(x, span)).collect(),
        _ => Err(unsupported("core.data: argument must be `[Float]`", span)),
    }
}

/// `[DataGroup]` argument — `bar_text`/`bar_svg` only read `.key`/`.count`
/// (never `.sum`/`.mean`), matching AOT's `jet_data_bar_text`/`_svg`.
fn as_data_groups(v: &CtValue, span: Span) -> Result<Vec<(String, i64)>, Diagnostic> {
    match v {
        CtValue::List(xs) => xs
            .iter()
            .map(|x| match x {
                CtValue::Struct { type_name, fields } if type_name == "DataGroup" => {
                    let key = fields
                        .iter()
                        .find(|(n, _)| n == "key")
                        .map(|(_, v)| v.clone());
                    let count = fields
                        .iter()
                        .find(|(n, _)| n == "count")
                        .map(|(_, v)| v.clone());
                    match (key, count) {
                        (Some(CtValue::Str(k)), Some(CtValue::Int(c))) => Ok((k, c)),
                        _ => Err(unsupported(
                            "core.data: a `DataGroup` needs `key: String` and `count: Int`",
                            span,
                        )),
                    }
                }
                _ => Err(unsupported("core.data: argument must be `[DataGroup]`", span)),
            })
            .collect(),
        _ => Err(unsupported("core.data: argument must be `[DataGroup]`", span)),
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

const BASE32_CHARS: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut buffer: u32 = 0;
    let mut bits = 0u8;
    for &b in bytes {
        buffer = (buffer << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            let idx = ((buffer >> (bits - 5)) & 31) as usize;
            out.push(BASE32_CHARS[idx] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        let idx = ((buffer << (5 - bits)) & 31) as usize;
        out.push(BASE32_CHARS[idx] as char);
    }
    while out.len() % 8 != 0 {
        out.push('=');
    }
    out
}

fn base32_val(b: u8) -> Result<u8, String> {
    match b {
        b'A'..=b'Z' => Ok(b - b'A'),
        b'a'..=b'z' => Ok(b - b'a'),
        b'2'..=b'7' => Ok(b - b'2' + 26),
        _ => Err(format!("invalid base32 character: {:?}", b as char)),
    }
}

fn base32_decode(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits = 0u8;
    for b in text.bytes().filter(|b| !b.is_ascii_whitespace() && *b != b'=') {
        buffer = (buffer << 5) | base32_val(b)? as u32;
        bits += 5;
        if bits >= 8 {
            out.push(((buffer >> (bits - 8)) & 0xff) as u8);
            bits -= 8;
        }
    }
    Ok(out)
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

/// D-DET1 widened ambient draws. Mirrors AOT's `jet_std_random_*` (Process.rs)
/// byte-for-byte — same `jet_rng_next`-equivalent `splitmix64` stream, same
/// formulas — so an ambient `core.random.*` call at comptime and the same
/// call at AOT runtime draw the identical sequence from the identical seed
/// (R12 parity).
fn random_float_open(state: &mut u64) -> f64 {
    let x = random_float(state);
    if x <= 0.0 {
        f64::MIN_POSITIVE
    } else {
        x
    }
}

fn random_float_range(state: &mut u64, low: f64, high: f64) -> f64 {
    if !(high > low) {
        return low;
    }
    low + (high - low) * random_float(state)
}

fn random_bool_p(state: &mut u64, p: f64) -> bool {
    if p <= 0.0 || p.is_nan() {
        false
    } else if p >= 1.0 {
        true
    } else {
        random_float(state) < p
    }
}

fn random_normal(state: &mut u64, mean: f64, stddev: f64) -> f64 {
    let u1 = random_float_open(state);
    let u2 = random_float(state);
    let z0 = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
    mean + z0 * stddev.max(0.0)
}

fn random_exponential(state: &mut u64, lambda: f64) -> f64 {
    if lambda <= 0.0 || lambda.is_nan() {
        return 0.0;
    }
    -random_float_open(state).ln() / lambda
}

fn random_bytes(state: &mut u64, n: i64) -> Vec<u8> {
    let n = n.max(0) as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(splitmix64(state) as u8);
    }
    out
}

fn random_pick_ct(state: &mut u64, xs: &[CtValue]) -> Option<CtValue> {
    if xs.is_empty() {
        None
    } else {
        Some(xs[random_int(state, 0, xs.len() as i64 - 1) as usize].clone())
    }
}

fn random_weighted_pick_ct(
    state: &mut u64,
    xs: &[CtValue],
    weights: &[f64],
) -> Option<CtValue> {
    if xs.is_empty() || xs.len() != weights.len() {
        return None;
    }
    let mut total = 0.0;
    for &w in weights {
        if w.is_finite() && w > 0.0 {
            total += w;
        }
    }
    if total <= 0.0 {
        return None;
    }
    let mut needle = random_float_range(state, 0.0, total);
    for (item, &weight) in xs.iter().zip(weights.iter()) {
        let w = if weight.is_finite() && weight > 0.0 { weight } else { 0.0 };
        if needle < w {
            return Some(item.clone());
        }
        needle -= w;
    }
    xs.last().cloned()
}

fn random_sample_ct(state: &mut u64, xs: &[CtValue], k: i64) -> Vec<CtValue> {
    let want = (k.max(0) as usize).min(xs.len());
    let mut pool = xs.to_vec();
    for i in 0..want {
        let j = random_int(state, i as i64, pool.len() as i64 - 1) as usize;
        pool.swap(i, j);
    }
    pool.truncate(want);
    pool
}

fn shuffle_ct_list(state: &mut u64, xs: &mut [CtValue]) {
    let len = xs.len();
    for i in (1..len).rev() {
        let j = random_int(state, 0, i as i64) as usize;
        xs.swap(i, j);
    }
}

// ── core.fmt: pure text formatting, mirrors AOT's `jet_fmt_*` (DataFmt.rs)
// byte-for-byte (comma grouping, byte-size units, duration parts, ordinal
// suffix, pad fill) so a comptime call prints identically to the same call
// at AOT runtime (R12 parity). ────────────────────────────────────────────

fn comma_int_ct(value: i64) -> String {
    let raw = value.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, ch) in raw.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    let mut text: String = out.chars().rev().collect();
    if value < 0 {
        text.insert(0, '-');
    }
    text
}

fn comma_decimal_ct(raw: String) -> String {
    let (sign, rest) = raw.strip_prefix('-').map_or(("", raw.as_str()), |s| ("-", s));
    let mut split = rest.splitn(2, '.');
    let whole = split.next().unwrap_or("0");
    let frac = split.next();
    let whole_value = whole.parse::<i64>().unwrap_or(0);
    let whole_text = comma_int_ct(whole_value);
    match frac {
        Some(frac) => format!("{}{}.{}", sign, whole_text, frac),
        None => format!("{}{}", sign, whole_text),
    }
}

fn fmt_decimal_ct(value: f64, precision: i64) -> String {
    let precision = precision.clamp(0, 9) as usize;
    comma_decimal_ct(format!("{:.*}", precision, value))
}

fn fmt_bytes_ct(value: i64) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let mut size = (value as f64).abs();
    let units = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
    let mut unit = 0usize;
    while size >= 1000.0 && unit + 1 < units.len() {
        size /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{}{} {}", sign, size as i64, units[unit])
    } else if size >= 10.0 {
        format!("{}{} {}", sign, size.round() as i64, units[unit])
    } else {
        let shown = format!("{:.1}", size);
        format!("{}{} {}", sign, shown.trim_end_matches(".0"), units[unit])
    }
}

fn fmt_duration_ct(ms: i64) -> String {
    let sign = if ms < 0 { "-" } else { "" };
    let mut rest = ms.abs();
    if rest < 1000 {
        return format!("{}{}ms", sign, rest);
    }
    let days = rest / 86_400_000;
    rest %= 86_400_000;
    let hours = rest / 3_600_000;
    rest %= 3_600_000;
    let minutes = rest / 60_000;
    rest %= 60_000;
    let seconds = rest / 1000;
    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{}d", days));
    }
    if hours > 0 {
        parts.push(format!("{}h", hours));
    }
    if minutes > 0 {
        parts.push(format!("{}m", minutes));
    }
    if seconds > 0 || parts.is_empty() {
        parts.push(format!("{}s", seconds));
    }
    format!(
        "{}{}",
        sign,
        parts.into_iter().take(3).collect::<Vec<_>>().join(" ")
    )
}

fn pad_need_ct(text: &str, width: i64) -> usize {
    let width = width.max(0) as usize;
    width.saturating_sub(text.chars().count())
}

fn pad_fill_ct(fill: &str, len: usize) -> String {
    if len == 0 {
        return String::new();
    }
    let fill = if fill.is_empty() { " " } else { fill };
    let mut out = String::new();
    while out.chars().count() < len {
        out.push_str(fill);
    }
    out.chars().take(len).collect()
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
        // card #392 gap fix: the rest of `core.math` — mechanical ports of
        // the same one-line Rust std calls AOT's codegen emits
        // (`Codegen/TIR/emit/core_calls.rs`), so results match exactly.
        ("core.math", "sin") => Ok(CtValue::Float(as_float(one(0)?, span)?.sin())),
        ("core.math", "cos") => Ok(CtValue::Float(as_float(one(0)?, span)?.cos())),
        ("core.math", "tan") => Ok(CtValue::Float(as_float(one(0)?, span)?.tan())),
        ("core.math", "asin") => Ok(CtValue::Float(as_float(one(0)?, span)?.asin())),
        ("core.math", "acos") => Ok(CtValue::Float(as_float(one(0)?, span)?.acos())),
        ("core.math", "atan") => Ok(CtValue::Float(as_float(one(0)?, span)?.atan())),
        ("core.math", "sinh") => Ok(CtValue::Float(as_float(one(0)?, span)?.sinh())),
        ("core.math", "cosh") => Ok(CtValue::Float(as_float(one(0)?, span)?.cosh())),
        ("core.math", "tanh") => Ok(CtValue::Float(as_float(one(0)?, span)?.tanh())),
        ("core.math", "exp") => Ok(CtValue::Float(as_float(one(0)?, span)?.exp())),
        ("core.math", "ln") => Ok(CtValue::Float(as_float(one(0)?, span)?.ln())),
        ("core.math", "trunc") => Ok(CtValue::Float(as_float(one(0)?, span)?.trunc())),
        ("core.math", "fract") => Ok(CtValue::Float(as_float(one(0)?, span)?.fract())),
        ("core.math", "degrees") => Ok(CtValue::Float(as_float(one(0)?, span)?.to_degrees())),
        ("core.math", "radians") => Ok(CtValue::Float(as_float(one(0)?, span)?.to_radians())),
        ("core.math", "atan2") => Ok(CtValue::Float(
            as_float(one(0)?, span)?.atan2(as_float(one(1)?, span)?),
        )),
        ("core.math", "hypot") => Ok(CtValue::Float(
            as_float(one(0)?, span)?.hypot(as_float(one(1)?, span)?),
        )),
        ("core.math", "lerp") => {
            let a = as_float(one(0)?, span)?;
            let b = as_float(one(1)?, span)?;
            let t = as_float(one(2)?, span)?;
            Ok(CtValue::Float(a + (b - a) * t))
        }
        ("core.math", "is_nan") => Ok(CtValue::Bool(as_float(one(0)?, span)?.is_nan())),
        ("core.math", "is_inf") => Ok(CtValue::Bool(as_float(one(0)?, span)?.is_infinite())),
        ("core.math", "is_finite") => Ok(CtValue::Bool(as_float(one(0)?, span)?.is_finite())),
        ("core.math", "sign") => {
            let x = as_float(one(0)?, span)?;
            Ok(CtValue::Int(if x > 0.0 {
                1
            } else if x < 0.0 {
                -1
            } else {
                0
            }))
        }
        ("core.math", "to_bits") => Ok(CtValue::Int(as_float(one(0)?, span)?.to_bits() as i64)),
        ("core.math", "from_bits") => Ok(CtValue::Float(f64::from_bits(
            as_int(one(0)?, span)? as u64,
        ))),
        ("core.math", "checked_add") => {
            let a = as_int(one(0)?, span)?;
            let b = as_int(one(1)?, span)?;
            Ok(match a.checked_add(b) {
                Some(n) => CtValue::Some(Box::new(CtValue::Int(n))),
                None => CtValue::None(Type::Int),
            })
        }
        ("core.math", "checked_sub") => {
            let a = as_int(one(0)?, span)?;
            let b = as_int(one(1)?, span)?;
            Ok(match a.checked_sub(b) {
                Some(n) => CtValue::Some(Box::new(CtValue::Int(n))),
                None => CtValue::None(Type::Int),
            })
        }
        ("core.math", "checked_mul") => {
            let a = as_int(one(0)?, span)?;
            let b = as_int(one(1)?, span)?;
            Ok(match a.checked_mul(b) {
                Some(n) => CtValue::Some(Box::new(CtValue::Int(n))),
                None => CtValue::None(Type::Int),
            })
        }
        ("core.math", "checked_pow") => {
            let base = as_int(one(0)?, span)?;
            let exp = as_int(one(1)?, span)?;
            Ok(if exp < 0 {
                CtValue::None(Type::Int)
            } else {
                match base.checked_pow(exp as u32) {
                    Some(n) => CtValue::Some(Box::new(CtValue::Int(n))),
                    None => CtValue::None(Type::Int),
                }
            })
        }
        ("core.math", "saturating_add") => Ok(CtValue::Int(
            as_int(one(0)?, span)?.saturating_add(as_int(one(1)?, span)?),
        )),
        ("core.math", "saturating_sub") => Ok(CtValue::Int(
            as_int(one(0)?, span)?.saturating_sub(as_int(one(1)?, span)?),
        )),
        ("core.math", "saturating_mul") => Ok(CtValue::Int(
            as_int(one(0)?, span)?.saturating_mul(as_int(one(1)?, span)?),
        )),
        ("core.math", "wrapping_add") => Ok(CtValue::Int(
            as_int(one(0)?, span)?.wrapping_add(as_int(one(1)?, span)?),
        )),
        ("core.math", "wrapping_sub") => Ok(CtValue::Int(
            as_int(one(0)?, span)?.wrapping_sub(as_int(one(1)?, span)?),
        )),
        ("core.math", "wrapping_mul") => Ok(CtValue::Int(
            as_int(one(0)?, span)?.wrapping_mul(as_int(one(1)?, span)?),
        )),
        ("core.math", "int_pow") => {
            let base = as_int(one(0)?, span)?;
            let exp = as_int(one(1)?, span)?;
            Ok(CtValue::Int(if exp < 0 {
                0
            } else {
                base.saturating_pow(exp as u32)
            }))
        }
        ("core.math", "gcd") => {
            let mut a = as_int(one(0)?, span)?.abs();
            let mut b = as_int(one(1)?, span)?.abs();
            while b != 0 {
                let r = a % b;
                a = b;
                b = r;
            }
            Ok(CtValue::Int(a))
        }
        ("core.math", "lcm") => {
            let a = as_int(one(0)?, span)?;
            let b = as_int(one(1)?, span)?;
            Ok(CtValue::Int(if a == 0 || b == 0 {
                0
            } else {
                let mut x = a.abs();
                let mut y = b.abs();
                while y != 0 {
                    let r = x % y;
                    x = y;
                    y = r;
                }
                (a / x).saturating_mul(b).abs()
            }))
        }
        // --- core.text module whitelist (card #392: `"core.string"` was a
        // dead key here — no import ever resolves to it, `core.text` is the
        // only ratified spelling (KNOWN_CORE_MODULES), so every arm below was
        // unreachable and every `use core.text as t; t.trim(s)`-style call
        // hit the E0956 fallback. Logic ported verbatim from AOT's
        // `jet_text_*` prelude fns via `TextLite` — R12 parity. ---
        ("core.text", "nfc") => Ok(CtValue::Str(super::TextLite::nfc(as_string(one(0)?, span)?))),
        ("core.text", "nfd") => Ok(CtValue::Str(super::TextLite::nfd(as_string(one(0)?, span)?))),
        ("core.text", "nfkc") => Ok(CtValue::Str(super::TextLite::nfkc(as_string(one(0)?, span)?))),
        ("core.text", "nfkd") => Ok(CtValue::Str(super::TextLite::nfkd(as_string(one(0)?, span)?))),
        ("core.text", "casefold") => Ok(CtValue::Str(super::TextLite::casefold(as_string(
            one(0)?,
            span,
        )?))),
        ("core.text", "lower") => Ok(CtValue::Str(as_string(one(0)?, span)?.to_lowercase())),
        ("core.text", "upper") => Ok(CtValue::Str(as_string(one(0)?, span)?.to_uppercase())),
        ("core.text", "caseless_eq") => Ok(CtValue::Bool(super::TextLite::caseless_eq(
            as_string(one(0)?, span)?,
            as_string(one(1)?, span)?,
        ))),
        ("core.text", "graphemes") => Ok(CtValue::List(
            super::TextLite::graphemes(as_string(one(0)?, span)?)
                .into_iter()
                .map(CtValue::Str)
                .collect(),
        )),
        ("core.text", "words") => Ok(CtValue::List(
            super::TextLite::words(as_string(one(0)?, span)?)
                .into_iter()
                .map(CtValue::Str)
                .collect(),
        )),
        ("core.text", "sentences") => Ok(CtValue::List(
            super::TextLite::sentences(as_string(one(0)?, span)?)
                .into_iter()
                .map(CtValue::Str)
                .collect(),
        )),
        ("core.text", "scalars") => Ok(CtValue::List(
            as_string(one(0)?, span)?
                .chars()
                .map(|c| CtValue::Str(c.to_string()))
                .collect(),
        )),
        ("core.text", "width") => Ok(CtValue::Int(super::TextLite::width(as_string(
            one(0)?,
            span,
        )?))),
        ("core.text", "scalar_count") => {
            Ok(CtValue::Int(as_string(one(0)?, span)?.chars().count() as i64))
        }
        ("core.text", "byte_count") => Ok(CtValue::Int(as_string(one(0)?, span)?.len() as i64)),
        ("core.text", "is_alphabetic") => Ok(CtValue::Bool(super::TextLite::is_alphabetic(
            as_string(one(0)?, span)?,
        ))),
        ("core.text", "is_numeric") => Ok(CtValue::Bool(super::TextLite::is_numeric(as_string(
            one(0)?,
            span,
        )?))),
        ("core.text", "is_whitespace") => Ok(CtValue::Bool(super::TextLite::is_whitespace(
            as_string(one(0)?, span)?,
        ))),
        ("core.text", "is_ascii") => Ok(CtValue::Bool(as_string(one(0)?, span)?.is_ascii())),
        ("core.text", "splitn") => {
            let s = as_string(one(0)?, span)?.to_string();
            let pat = as_string(one(1)?, span)?.to_string();
            let n = as_int(one(2)?, span)?;
            Ok(CtValue::List(
                super::TextLite::splitn(&s, &pat, n)
                    .into_iter()
                    .map(CtValue::Str)
                    .collect(),
            ))
        }
        ("core.text", "rsplitn") => {
            let s = as_string(one(0)?, span)?.to_string();
            let pat = as_string(one(1)?, span)?.to_string();
            let n = as_int(one(2)?, span)?;
            Ok(CtValue::List(
                super::TextLite::rsplitn(&s, &pat, n)
                    .into_iter()
                    .map(CtValue::Str)
                    .collect(),
            ))
        }
        ("core.text", "trim") => Ok(CtValue::Str(as_string(one(0)?, span)?.trim().to_string())),
        ("core.text", "trim_start") => Ok(CtValue::Str(
            as_string(one(0)?, span)?.trim_start().to_string(),
        )),
        ("core.text", "trim_end") => Ok(CtValue::Str(
            as_string(one(0)?, span)?.trim_end().to_string(),
        )),
        ("core.text", "pad_start") => {
            let s = as_string(one(0)?, span)?.to_string();
            let w = as_int(one(1)?, span)?;
            let fill = as_string(one(2)?, span)?.to_string();
            Ok(CtValue::Str(super::TextLite::pad_start(&s, w, &fill)))
        }
        ("core.text", "pad_end") => {
            let s = as_string(one(0)?, span)?.to_string();
            let w = as_int(one(1)?, span)?;
            let fill = as_string(one(2)?, span)?.to_string();
            Ok(CtValue::Str(super::TextLite::pad_end(&s, w, &fill)))
        }
        ("core.text", "center") => {
            let s = as_string(one(0)?, span)?.to_string();
            let w = as_int(one(1)?, span)?;
            let fill = as_string(one(2)?, span)?.to_string();
            Ok(CtValue::Str(super::TextLite::center(&s, w, &fill)))
        }
        ("core.text", "starts_any") => {
            let s = as_string(one(0)?, span)?.to_string();
            let prefixes = match one(1)? {
                CtValue::List(xs) => xs
                    .iter()
                    .map(|v| as_string(v, span).map(|s| s.to_string()))
                    .collect::<Result<Vec<_>, _>>()?,
                _ => return Err(unsupported("core.text.starts_any: non-list argument", span)),
            };
            Ok(CtValue::Bool(super::TextLite::starts_any(&s, &prefixes)))
        }
        ("core.text", "ends_any") => {
            let s = as_string(one(0)?, span)?.to_string();
            let suffixes = match one(1)? {
                CtValue::List(xs) => xs
                    .iter()
                    .map(|v| as_string(v, span).map(|s| s.to_string()))
                    .collect::<Result<Vec<_>, _>>()?,
                _ => return Err(unsupported("core.text.ends_any: non-list argument", span)),
            };
            Ok(CtValue::Bool(super::TextLite::ends_any(&s, &suffixes)))
        }
        ("core.text", "char_indices") => Ok(CtValue::List(
            super::TextLite::char_indices(as_string(one(0)?, span)?)
                .into_iter()
                .map(CtValue::Str)
                .collect(),
        )),
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
        // --- jet.regex / core.regex (D-REGEXENGINE1) ---
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
        ("core.random", "split") => {
            let seed = match one(0)? {
                CtValue::Int(n) => *n as u64,
                _ => return Err(unsupported("random.split expects an Int seed", span)),
            };
            let mixed = with_ambient_rng(|st| seed ^ splitmix64(st).rotate_left(17));
            Ok(CtValue::Struct {
                type_name: crate::Syntax::RNG_TYPE.to_string(),
                fields: vec![("state".to_string(), CtValue::Int(mixed as i64))],
            })
        }
        ("core.random", "float_range") => {
            let low = as_float(one(0)?, span)?;
            let high = as_float(one(1)?, span)?;
            Ok(CtValue::Float(with_ambient_rng(|st| {
                random_float_range(st, low, high)
            })))
        }
        ("core.random", "bool") => {
            let p = as_float(one(0)?, span)?;
            Ok(CtValue::Bool(with_ambient_rng(|st| random_bool_p(st, p))))
        }
        ("core.random", "normal") => {
            let mean = as_float(one(0)?, span)?;
            let stddev = as_float(one(1)?, span)?;
            Ok(CtValue::Float(with_ambient_rng(|st| {
                random_normal(st, mean, stddev)
            })))
        }
        ("core.random", "exponential") => {
            let lambda = as_float(one(0)?, span)?;
            Ok(CtValue::Float(with_ambient_rng(|st| {
                random_exponential(st, lambda)
            })))
        }
        ("core.random", "bytes") => {
            let n = match one(0)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("random.bytes expects an Int count", span)),
            };
            Ok(CtValue::Bytes(with_ambient_rng(|st| random_bytes(st, n))))
        }
        ("core.random", "pick") => {
            let CtValue::List(xs) = one(0)? else {
                return Err(unsupported("random.pick needs a list", span));
            };
            Ok(match with_ambient_rng(|st| random_pick_ct(st, xs)) {
                Some(v) => CtValue::Some(Box::new(v)),
                None => CtValue::None(Type::Int),
            })
        }
        ("core.random", "weighted_pick") => {
            let CtValue::List(xs) = one(0)? else {
                return Err(unsupported("random.weighted_pick needs a list", span));
            };
            let CtValue::List(ws) = one(1)? else {
                return Err(unsupported(
                    "random.weighted_pick needs a [Float] weights list",
                    span,
                ));
            };
            let weights: Vec<f64> = ws
                .iter()
                .map(|w| as_float(w, span))
                .collect::<Result<_, _>>()?;
            Ok(
                match with_ambient_rng(|st| random_weighted_pick_ct(st, xs, &weights)) {
                    Some(v) => CtValue::Some(Box::new(v)),
                    None => CtValue::None(Type::Int),
                },
            )
        }
        ("core.random", "sample") => {
            let CtValue::List(xs) = one(0)? else {
                return Err(unsupported("random.sample needs a list", span));
            };
            let k = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("random.sample count must be Int", span)),
            };
            Ok(CtValue::List(with_ambient_rng(|st| {
                random_sample_ct(st, xs, k)
            })))
        }
        // --- core.fmt (pure text formatting; mirrors AOT's `jet_fmt_*`, DataFmt.rs) ---
        ("core.fmt", "number") => {
            let n = match one(0)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.number expects an Int", span)),
            };
            Ok(CtValue::Str(comma_int_ct(n)))
        }
        ("core.fmt", "decimal") => {
            let value = as_float(one(0)?, span)?;
            let precision = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.decimal precision must be Int", span)),
            };
            Ok(CtValue::Str(fmt_decimal_ct(value, precision)))
        }
        ("core.fmt", "percent") => {
            let value = as_float(one(0)?, span)?;
            let precision = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.percent precision must be Int", span)),
            };
            Ok(CtValue::Str(format!(
                "{}%",
                fmt_decimal_ct(value * 100.0, precision)
            )))
        }
        ("core.fmt", "bytes") => {
            let n = match one(0)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.bytes expects an Int", span)),
            };
            Ok(CtValue::Str(fmt_bytes_ct(n)))
        }
        ("core.fmt", "duration") => {
            let ms = match one(0)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.duration expects an Int (ms)", span)),
            };
            Ok(CtValue::Str(fmt_duration_ct(ms)))
        }
        ("core.fmt", "ordinal") => {
            let n = match one(0)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.ordinal expects an Int", span)),
            };
            let abs = n.abs();
            let suffix = if (11..=13).contains(&(abs % 100)) {
                "th"
            } else {
                match abs % 10 {
                    1 => "st",
                    2 => "nd",
                    3 => "rd",
                    _ => "th",
                }
            };
            Ok(CtValue::Str(format!("{}{}", comma_int_ct(n), suffix)))
        }
        ("core.fmt", "plural") => {
            let count = match one(0)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.plural count must be Int", span)),
            };
            let singular = as_string(one(1)?, span)?;
            let plural = as_string(one(2)?, span)?;
            let word = if count.abs() == 1 { singular } else { plural };
            Ok(CtValue::Str(format!("{} {}", comma_int_ct(count), word)))
        }
        ("core.fmt", "pad_left") => {
            let text = as_string(one(0)?, span)?;
            let width = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.pad_left width must be Int", span)),
            };
            let fill = as_string(one(2)?, span)?;
            let need = pad_need_ct(text, width);
            Ok(CtValue::Str(format!("{}{}", pad_fill_ct(fill, need), text)))
        }
        ("core.fmt", "pad_right") => {
            let text = as_string(one(0)?, span)?;
            let width = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.pad_right width must be Int", span)),
            };
            let fill = as_string(one(2)?, span)?;
            let need = pad_need_ct(text, width);
            Ok(CtValue::Str(format!("{}{}", text, pad_fill_ct(fill, need))))
        }
        ("core.fmt", "pad_center") => {
            let text = as_string(one(0)?, span)?;
            let width = match one(1)? {
                CtValue::Int(n) => *n,
                _ => return Err(unsupported("fmt.pad_center width must be Int", span)),
            };
            let fill = as_string(one(2)?, span)?;
            let need = pad_need_ct(text, width);
            let left = need / 2;
            let right = need - left;
            Ok(CtValue::Str(format!(
                "{}{}{}",
                pad_fill_ct(fill, left),
                text,
                pad_fill_ct(fill, right)
            )))
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
        // --- core.encoding.base64 URL-safe variant (pure; mirrors AOT's
        // `jet_std_b64url_*`, EncodingCodecs.rs — the same alphabet with
        // `+`/`/` swapped for `-`/`_` and no padding) ---
        ("core.encoding.base64", "encode_url") => {
            let bytes = as_bytes(one(0)?, span)?;
            Ok(CtValue::Str(
                base64_encode(bytes)
                    .trim_end_matches('=')
                    .replace('+', "-")
                    .replace('/', "_"),
            ))
        }
        ("core.encoding.base64", "decode_url") => {
            let s = as_string(one(0)?, span)?;
            let mut padded = s.trim().replace('-', "+").replace('_', "/");
            while padded.len() % 4 != 0 {
                padded.push('=');
            }
            Ok(match base64_decode(&padded) {
                Some(bytes) => CtValue::ResOk(Box::new(CtValue::Bytes(bytes))),
                None => CtValue::ResErr(Box::new(CtValue::Str(format!(
                    "`{}` isn't valid base64url",
                    s
                )))),
            })
        }
        // --- core.encoding.base32 (pure; mirrors AOT's `jet_std_base32_*`,
        // EncodingCodecs.rs, byte-for-byte — same alphabet, same bit-packing) ---
        ("core.encoding.base32", "encode") => {
            let bytes = as_bytes(one(0)?, span)?;
            Ok(CtValue::Str(base32_encode(&bytes)))
        }
        ("core.encoding.base32", "decode") => {
            let s = as_string(one(0)?, span)?;
            Ok(match base32_decode(s) {
                Ok(bytes) => CtValue::ResOk(Box::new(CtValue::Bytes(bytes))),
                Err(e) => CtValue::ResErr(Box::new(CtValue::Str(e))),
            })
        }
        // --- D-URL1=A: core.url (pure RFC-3986-shaped parser, ported
        // verbatim from AOT's `JetUrl`/`jet_url_*` in `UrlMime.rs` — see
        // `UrlLite.rs`) ---
        ("core.url", "parse") => {
            let s = as_string(one(0)?, span)?;
            Ok(match super::UrlLite::UrlParts::parse(s) {
                Ok(u) => CtValue::ResOk(Box::new(url_parts_to_ct(&u))),
                Err(e) => CtValue::ResErr(Box::new(CtValue::Str(e))),
            })
        }
        ("core.url", "from_parts") => {
            let scheme = as_string(one(0)?, span)?.to_string();
            let host = as_string(one(1)?, span)?.to_string();
            let path = as_string(one(2)?, span)?.to_string();
            let query = as_string_rows(one(3)?, span)?;
            let fragment = as_string(one(4)?, span)?.to_string();
            Ok(
                match super::UrlLite::UrlParts::from_parts(&scheme, &host, &path, &query, &fragment)
                {
                    Ok(u) => CtValue::ResOk(Box::new(url_parts_to_ct(&u))),
                    Err(e) => CtValue::ResErr(Box::new(CtValue::Str(e))),
                },
            )
        }
        ("core.url", "file") => {
            let path = as_string(one(0)?, span)?;
            Ok(url_parts_to_ct(&super::UrlLite::UrlParts::file(path)))
        }
        ("core.url", "data") => {
            // `mime` arg is a `CtValue::Struct { type_name: "Mime", .. }`
            // (D-URL1's `Mime` type) with `top`/`sub`/`params` fields — the
            // `core.mime` module port isn't in this card's slice, so render
            // its essence + params here the same way AOT's
            // `JetMime::to_string_value` does, matching field-for-field.
            let mime = one(0)?;
            let text = as_string(one(1)?, span)?;
            let rendered = match mime {
                CtValue::Struct { type_name, fields } if type_name == "Mime" => {
                    let get = |name: &str| {
                        fields
                            .iter()
                            .find(|(n, _)| n == name)
                            .map(|(_, v)| v.clone())
                    };
                    let top = match get("top") {
                        Some(CtValue::Str(s)) => s,
                        _ => return Err(unsupported("core.url.data: mime.top must be String", span)),
                    };
                    let sub = match get("sub") {
                        Some(CtValue::Str(s)) => s,
                        _ => return Err(unsupported("core.url.data: mime.sub must be String", span)),
                    };
                    let mut out = format!("{}/{}", top, sub);
                    if let Some(CtValue::List(params)) = get("params") {
                        for p in params {
                            if let CtValue::List(kv) = p {
                                if let [CtValue::Str(k), CtValue::Str(v)] = &kv[..] {
                                    out.push_str("; ");
                                    out.push_str(k);
                                    out.push('=');
                                    out.push_str(v);
                                }
                            }
                        }
                    }
                    out
                }
                _ => return Err(unsupported("core.url.data: first argument must be a Mime", span)),
            };
            Ok(url_parts_to_ct(&super::UrlLite::UrlParts::data(
                &rendered, text,
            )))
        }
        ("core.url", "query") => {
            let rows = as_string_rows(one(0)?, span)?;
            let pairs: Vec<(String, String)> = rows
                .iter()
                .filter(|r| !r.is_empty())
                .map(|r| {
                    (
                        r.get(0).cloned().unwrap_or_default(),
                        r.get(1).cloned().unwrap_or_default(),
                    )
                })
                .collect();
            Ok(CtValue::Str(super::UrlLite::url_render_query(&pairs)))
        }
        ("core.url", "percent_encode") => {
            let s = as_string(one(0)?, span)?;
            Ok(CtValue::Str(super::UrlLite::url_percent_encode(s, false)))
        }
        ("core.url", "percent_decode") => {
            let s = as_string(one(0)?, span)?;
            Ok(match super::UrlLite::url_percent_decode_str(s) {
                Ok(v) => CtValue::ResOk(Box::new(CtValue::Str(v))),
                Err(e) => CtValue::ResErr(Box::new(CtValue::Str(e))),
            })
        }
        // --- D-DATA-SURFACE1/PLOT1/STATUS1: core.data's fixed-signature
        // stats + plot surface (pure, ported verbatim from AOT's
        // `jet_data_*` — see `DataLite.rs`). The generic call-site-typed
        // table/lazy-pipeline half of `core.data` is a separate, larger
        // design pass (see `DataLite.rs`'s doc comment) and isn't here.
        ("core.data", "sum") => Ok(CtValue::Float(super::DataLite::sum(&as_float_list(
            one(0)?,
            span,
        )?))),
        ("core.data", "mean") => Ok(CtValue::Float(super::DataLite::mean(&as_float_list(
            one(0)?,
            span,
        )?))),
        ("core.data", "min") => Ok(CtValue::Float(super::DataLite::min(&as_float_list(
            one(0)?,
            span,
        )?))),
        ("core.data", "max") => Ok(CtValue::Float(super::DataLite::max(&as_float_list(
            one(0)?,
            span,
        )?))),
        ("core.data", "median") => Ok(CtValue::Float(super::DataLite::median(&as_float_list(
            one(0)?,
            span,
        )?))),
        ("core.data", "variance") => Ok(CtValue::Float(super::DataLite::variance(
            &as_float_list(one(0)?, span)?,
        ))),
        ("core.data", "stddev") => Ok(CtValue::Float(super::DataLite::stddev(&as_float_list(
            one(0)?,
            span,
        )?))),
        ("core.data", "quantile") => {
            let values = as_float_list(one(0)?, span)?;
            let q = as_float(one(1)?, span)?;
            Ok(CtValue::Float(super::DataLite::quantile(&values, q)))
        }
        ("core.data", "rolling_mean") => {
            let values = as_float_list(one(0)?, span)?;
            let width = as_int(one(1)?, span)?;
            Ok(CtValue::List(
                super::DataLite::rolling_mean(&values, width)
                    .into_iter()
                    .map(CtValue::Float)
                    .collect(),
            ))
        }
        ("core.data", "describe") => {
            let values = as_float_list(one(0)?, span)?;
            Ok(CtValue::Struct {
                type_name: "DataSummary".to_string(),
                fields: vec![
                    (
                        "count".to_string(),
                        CtValue::Int(values.len() as i64),
                    ),
                    ("sum".to_string(), CtValue::Float(super::DataLite::sum(&values))),
                    ("mean".to_string(), CtValue::Float(super::DataLite::mean(&values))),
                    ("min".to_string(), CtValue::Float(super::DataLite::min(&values))),
                    ("max".to_string(), CtValue::Float(super::DataLite::max(&values))),
                    (
                        "median".to_string(),
                        CtValue::Float(super::DataLite::median(&values)),
                    ),
                    (
                        "variance".to_string(),
                        CtValue::Float(super::DataLite::variance(&values)),
                    ),
                    (
                        "stddev".to_string(),
                        CtValue::Float(super::DataLite::stddev(&values)),
                    ),
                ],
            })
        }
        ("core.data", "status") => Ok(CtValue::List(
            super::DataLite::status_rows()
                .into_iter()
                .map(|(step, path, replacement)| CtValue::Struct {
                    type_name: "DataStatus".to_string(),
                    fields: vec![
                        ("step".to_string(), CtValue::Str(step.to_string())),
                        ("path".to_string(), CtValue::Str(path.to_string())),
                        (
                            "replacement".to_string(),
                            CtValue::Str(replacement.to_string()),
                        ),
                    ],
                })
                .collect(),
        )),
        ("core.data", "bar_text") => Ok(CtValue::Str(super::DataLite::bar_text(
            &as_data_groups(one(0)?, span)?,
        ))),
        ("core.data", "bar_svg") => Ok(CtValue::Str(super::DataLite::bar_svg(&as_data_groups(
            one(0)?,
            span,
        )?))),
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

fn regex_pattern(args: &[CtValue], span: Span) -> Result<super::RegexLite::RegexLite, Diagnostic> {
    let pat = as_string(
        args.first()
            .ok_or_else(|| unsupported("regex call: missing pattern argument", span))?,
        span,
    )?;
    super::RegexLite::RegexLite::parse(pat).map_err(|e| {
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
        Some(m) => CtValue::Some(Box::new(CtValue::Str(text[m.start..m.end].to_string()))),
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
        .find_all(text)
        .into_iter()
        .map(|m| CtValue::Str(m.to_string()))
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
        .into_iter()
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
        re.replace_all(text, rep),
    ))))
}

fn regex_match(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let re = regex_pattern(&args, span)?;
    let text = as_string(
        args.get(1)
            .ok_or_else(|| unsupported("regex.match: missing text argument", span))?,
        span,
    )?;
    Ok(CtValue::ResOk(Box::new(match re.find(text) {
        Some(m) => {
            let groups: Vec<CtValue> = m
                .groups
                .iter()
                .map(|i| {
                    i.map(|(start, end)| {
                        CtValue::Some(Box::new(CtValue::Str(text[start..end].to_string())))
                    })
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
