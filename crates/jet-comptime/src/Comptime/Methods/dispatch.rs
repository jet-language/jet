//! Call/method dispatch for the interpreter: `eval_call`, `eval_method`,
//! `eval_fan_out`, `eval_require`, `eval_embed_file`. These are further
//! `impl Interp` methods; the struct and spine live in `interp.rs`.

use std::collections::HashMap;
use std::path::Path;

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{AccessConvention, CallArg, Expr, Func, LambdaBody, StrPart, Type, UnOp};

use super::super::Builtins::{
    apply_method, apply_mutating, apply_static_type_method, as_bool, as_int, cmp,
};
use super::super::Diagnostics::{comptime_panic, unsupported};
use super::super::Diagnostics::{EARLY_RETURN_CODE, ERR_PROPAGATE_CODE};
use super::super::Interpreter::{Flow, Interp};
use super::super::Value::CtValue;
use super::core_calls::{apply_core_call, apply_impure_core_call, as_bytes, shuffle_ct_list, with_ambient_rng};
use super::repl_process::{apply_repl_fs_call, pin_repl_command, repl_effect_request};

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
        Expr::Int(n, _, _, _) => Some(*n),
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
    pub(in super::super) fn eval_fan_out(
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

    pub(in super::super) fn eval_call(
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
                // known function is the distinct-type / `@UnitFamily` constructor
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
                let (mut frame, writebacks) =
                    self.bind_fixed_call_args(fixed, &args[..fixed.len()], scope)?;
                let mut rest = Vec::with_capacity(args.len() - fixed.len());
                for a in &args[fixed.len()..] {
                    rest.push(self.eval(&a.expr, scope)?);
                }
                frame.insert(last.name.clone(), CtValue::List(rest));
                return self.call_func_with_writebacks(name, func, frame, writebacks, scope);
            }
        }
        if func.params.len() != args.len() {
            return Err(unsupported(
                &format!("`{}` (wrong number of arguments)", name),
                span,
            ));
        }
        let (frame, writebacks) = self.bind_fixed_call_args(&func.params, args, scope)?;
        self.call_func_with_writebacks(name, func, frame, writebacks, scope)
    }

    fn bind_fixed_call_args(
        &mut self,
        params: &[crate::AST::Param],
        args: &[CallArg],
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<(HashMap<String, CtValue>, Vec<(String, String)>), Diagnostic> {
        let mut frame = HashMap::new();
        let mut writebacks = Vec::new();
        for (p, a) in params.iter().zip(args) {
            frame.insert(p.name.clone(), self.eval(&a.expr, scope)?);
            if p.convention == AccessConvention::Write {
                if let Expr::Ident(caller_name, _) = &a.expr {
                    writebacks.push((p.name.clone(), caller_name.clone()));
                }
            }
        }
        Ok((frame, writebacks))
    }

    fn call_func_with_writebacks(
        &mut self,
        name: &str,
        func: &Func,
        frame: HashMap<String, CtValue>,
        writebacks: Vec<(String, String)>,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let (result, frame) = self.call_func_with_frame(name, func, frame)?;
        for (param_name, caller_name) in writebacks {
            if let Some(value) = frame.get(&param_name) {
                scope.insert(caller_name, value.clone());
            }
        }
        Ok(result)
    }

    /// c139: run a resolved `Func`'s body in `frame` (already bound: params,
    /// and `self` for an instance/associated method), threading the same
    /// debugger bookkeeping and `?`/`?? return` sentinel handling `eval_call`
    /// always has. Shared by plain calls, instance-method dispatch, and
    /// code-module namespaced calls.
    pub(in super::super) fn call_func(
        &mut self,
        name: &str,
        func: &Func,
        frame: HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        self.call_func_with_frame(name, func, frame)
            .map(|(value, _)| value)
    }

    fn call_func_with_frame(
        &mut self,
        name: &str,
        func: &Func,
        mut frame: HashMap<String, CtValue>,
    ) -> Result<(CtValue, HashMap<String, CtValue>), Diagnostic> {
        // D-DBG3: enter a user-function frame — bump the debugger's call depth
        // and current-function name so `next`/`finish` and the `in fn()` banner
        // track correctly, then restore both on the way out (every path).
        let prev_depth = self.depth;
        let prev_func = std::mem::replace(&mut self.cur_func, name.to_string());
        self.depth = prev_depth + 1;
        let result = self.exec_block(&func.body, &mut frame);
        self.depth = prev_depth;
        self.cur_func = prev_func;
        let value = match result {
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
        }?;
        Ok((value, frame))
    }

    /// c139 (D-DISPLAYDBG1/2): render `v` as `{value}` interpolation / `print`
    /// would in the compiled program. When `v`'s type has a user-written
    /// `impl Type.Display { fn display(self) -> String }`, run that exact Jet
    /// function body (byte-identical to what the real build does); otherwise
    /// fall back to the built-in `jet_show()` rendering (every primitive, and
    /// any struct/enum with no such impl — sema only accepts those in
    /// interpolation when they're "auto-printable", which `jet_show()`
    /// already matches).
    pub(in super::super) fn show_value(&mut self, v: &CtValue, _span: Span) -> Result<String, Diagnostic> {
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

    pub(in super::super) fn debug_value(&self, v: &CtValue) -> String {
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
    pub(in super::super) fn call_closure(
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

    /// c139: `Name(expr)` — the distinct-type / `@UnitFamily` constructor.
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
    /// This stub preserves the correct Tier-1 routing (no `@Impure` gate) so
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

    pub(in super::super) fn eval_method(
        &mut self,
        receiver: &Expr,
        method: &str,
        span: Span,
        type_args: &[Type],
        args: &[crate::AST::CallArg],
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        // D-ENC-XML-SURFACE1=A: qualified safe whole-value XML constructors.
        if method == "safe" && args.is_empty() {
            if let Expr::Field(base, type_name, _) = receiver {
                if let Expr::Ident(alias, _) = base.as_ref() {
                    if self.core_imports.get(alias).map(String::as_str) == Some("core.encoding.xml") {
                        if type_name == "XMLLimits" {
                            return Ok(super::super::EncodingLite::xml_safe_limits_value());
                        }
                        if type_name == "XMLParseOptions" {
                            return Ok(super::super::EncodingLite::xml_safe_options_value());
                        }
                    }
                }
            }
        }
        // D-SHAPE-CONVERT1=A: numeric-backed distinct/unit conversion is the
        // existing distinct constructor with destination-owned spelling.
        if let Expr::Ident(type_name, _) = receiver {
            if let Some(range) = self.distinct_ranges.get(type_name).copied() {
                if crate::Syntax::numeric_conversion_source(method).is_some() && args.len() == 1 {
                    return self.eval_distinct_ctor(type_name, range, args, span, scope);
                }
            }
        }
        // c97/D-STRPARSE1: static method on a built-in type name (e.g. `Int.parse(s)`).
        // Check *before* evaluating the receiver so `Int`/`Float` don't fail scope lookup.
        if let Expr::Ident(type_name, _) = receiver {
            if type_name == crate::Syntax::DURATION_TYPE {
                let Some(unit) = crate::Syntax::duration_unit_for_constructor(method) else {
                    return Err(super::super::Diagnostics::unsupported(
                        &format!("`{}.{}()`", type_name, method),
                        span,
                    ));
                };
                let scale = match unit {
                    "Milliseconds" => 1,
                    "Seconds" => 1_000,
                    "Minutes" => 60_000,
                    "Hours" => 3_600_000,
                    _ => unreachable!("Syntax returned a closed duration unit"),
                };
                let value = self.eval(&args[0].expr, scope)?;
                let ms = match value {
                    CtValue::Int(n) => n.checked_mul(scale),
                    CtValue::Float(n) => {
                        let scaled = n * scale as f64;
                        (scaled.is_finite()
                            && scaled >= i64::MIN as f64
                            && scaled < 9_223_372_036_854_775_808.0)
                            .then_some(scaled.trunc() as i64)
                    }
                    _ => None,
                };
                return Ok(match ms {
                    Some(ms) => CtValue::ResOk(Box::new(CtValue::Struct {
                        type_name: crate::Syntax::DURATION_TYPE.to_string(),
                        fields: vec![("ms".to_string(), CtValue::Int(ms))],
                    })),
                    None => CtValue::ResErr(Box::new(CtValue::Struct {
                        type_name: crate::Syntax::DURATION_RANGE_ERROR_TYPE.to_string(),
                        fields: vec![(
                            "reason".to_string(),
                            CtValue::Str("duration must be finite and inside the supported range".to_string()),
                        )],
                    })),
                });
            }
            // Only intercept known built-in type names; user struct names use normal path.
            let is_builtin_type = crate::AST::numeric_type_from_name(type_name).is_some()
                || matches!(type_name.as_str(), "Bool" | "String");
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
                return Err(super::super::Diagnostics::unsupported(
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
                    if self.repl_mode {
                        let request = super::super::ReplEffectRequest {
                            root: "Rand".to_string(),
                            operation: "Draw".to_string(),
                            resource: "shuffle".to_string(),
                        };
                        if !self.repl_grants.iter().any(|cap| cap == "Rand") {
                            return Err(Diagnostic::error(
                                "E1803",
                                "Rand.Draw for `shuffle` has no REPL runtime authority".to_string(),
                                "REPL ambient randomness requires lexical `@Grant(Rand)` authority; the RNG state did not advance".to_string(),
                                "wrap this draw in `@Grant(Rand) { caps -> ... }` and approve it or pass `--allow-rand`".to_string(),
                                Some(span),
                            ));
                        }
                        let Some(authorizer) = self.repl_authorizer.as_deref_mut() else {
                            return Err(Diagnostic::error(
                                "E1803",
                                "Rand.Draw for `shuffle` was denied".to_string(),
                                "this REPL mode has no runtime authority provider; the RNG state did not advance".to_string(),
                                "restart with `jet repl --allow-rand`".to_string(),
                                Some(span),
                            ));
                        };
                        authorizer.authorize(&request, span)?;
                    }
                    with_ambient_rng(|st| shuffle_ct_list(st, &mut items));
                    self.write_back(&arg.expr, CtValue::List(items), scope)?;
                    return Ok(CtValue::Unit);
                }
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval(&a.expr, scope)?);
                }
                // D-FE-REPL-INTERRUPT1=A: poll before and after every Core
                // runtime call. Long host calls stay marked so the raw UI can
                // explain why cancellation has not returned yet.
                self.poll_repl_interrupt();
                let _runtime_call = super::super::ReplRuntimeCallGuard::new(self.repl_interruptible);
                // Card #392 pass 5: `core.data`'s typed table/lazy pipeline — a
                // generic call-site-typed surface built from ordinary Jet
                // lambdas over dynamically-typed `CtValue` rows, so (unlike
                // `decode<T>` below) only `csv<T>` actually reads `type_args`.
                // The pre-existing fixed-signature stats/plot surface (`sum`/
                // `mean`/…/`bar_svg`, `DataLite.rs`) stays on the
                // `apply_core_call` path below — only the table/lazy pipeline
                // names are new here.
                if matches!(
                    (module.as_str(), method),
                    (
                        "core.data",
                        "csv" | "count" | "table" | "rows" | "series" | "values" | "missing_count"
                            | "lazy" | "lazy_filter" | "lazy_sort_by" | "collect" | "plan" | "filter"
                            | "sort_by" | "group_count" | "group_sum" | "group_mean" | "inner_join"
                            | "left_join",
                    )
                ) {
                    return self.eval_data_call(method, argv, type_args, span);
                }
                // D-ENC-CBOR-SURFACE1: encoding a Codable value needs its
                // declared field types. CtValue intentionally erases `[U8]`
                // to an integer list, so generic by-value dispatch cannot
                // distinguish CBOR byte strings from ordinary arrays.
                if module == "core.encoding.cbor"
                    && matches!(method, "to_bytes" | "to_bytes_canonical")
                {
                    let Some(value) = argv.first() else {
                        return Err(unsupported(
                            "core.encoding.cbor.to_bytes(): missing arg 0",
                            span,
                        ));
                    };
                    return Ok(match super::super::EncodingLite::cbor_encode_typed(
                        value,
                        self.structs,
                        method == "to_bytes_canonical",
                    ) {
                        Ok(bytes) => CtValue::ResOk(Box::new(CtValue::Bytes(bytes))),
                        Err(reason) => CtValue::ResErr(Box::new(CtValue::Struct {
                            type_name: "CBORError".to_string(),
                            fields: vec![
                                (
                                    "kind".to_string(),
                                    CtValue::Enum {
                                        type_name: "CBORErrorKind".to_string(),
                                        variant: "Unsupported".to_string(),
                                        args: Vec::new(),
                                    },
                                ),
                                ("byte_offset".to_string(), CtValue::Int(0)),
                                ("path".to_string(), CtValue::Str("$".to_string())),
                                ("reason".to_string(), CtValue::Str(reason)),
                            ],
                        })),
                    });
                }
                // D-MIGRATE3=A / D-SERDE6: `decode<T>`/`decode_traced<T>` — typed
                // Decode dispatch. Untyped `.decode()` (no turbofish, D-JSON3
                // lenient form) keeps its existing `apply_core_call` arm below.
                if matches!(
                    (module.as_str(), method),
                    (
                        "core.encoding.json" | "core.encoding.csv" | "core.encoding.toml" | "core.encoding.yaml",
                        "decode" | "decode_traced",
                    )
                ) && !type_args.is_empty()
                {
                    let Some(text) = argv.first().and_then(|v| match v {
                        CtValue::Str(s) => Some(s.clone()),
                        _ => None,
                    }) else {
                        return Err(unsupported(
                            &format!("`{}.{}()`: expected a string argument", module, method),
                            span,
                        ));
                    };
                    return self.eval_typed_decode(&module, method, &text, &type_args[0], span);
                }
                // D-ENC-CBOR-SURFACE1 / R12: generic whole-value CBOR decode
                // uses the same typed tree walker as every other codec.  Keep
                // this ahead of the compatibility `cbor.decode(DataTree)` arm
                // below: the type argument is the semantic distinction.
                if module == "core.encoding.cbor" && method == "decode" && !type_args.is_empty() {
                    let bytes = match argv.first() {
                        Some(value) => as_bytes(value, span)?,
                        None => return Err(unsupported("core.encoding.cbor.decode(): missing arg 0", span)),
                    };
                    let options = match super::super::EncodingLite::cbor_options(argv.get(1)) {
                        Ok(options) => options,
                        Err(error) => {
                            return Ok(CtValue::ResErr(Box::new(
                                super::super::EncodingLite::cbor_error_value(error),
                            )))
                        }
                    };
                    let tree = match super::super::EncodingLite::cbor_decode(&bytes, &options, true) {
                        Ok(tree) => tree,
                        Err(error) => {
                            return Ok(CtValue::ResErr(Box::new(
                                super::super::EncodingLite::cbor_error_value(error),
                            )))
                        }
                    };
                    return match self.typed_decode_top(&type_args[0], &tree, span) {
                        Ok((value, _)) => Ok(CtValue::ResOk(Box::new(value))),
                        Err(error) => {
                            let (path, reason) = match error {
                                CtValue::Struct { fields, .. } => {
                                    let path = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
                                        ("path", CtValue::Str(value)) => Some(value.clone()),
                                        _ => None,
                                    }).unwrap_or_default();
                                    let reason = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
                                        ("reason", CtValue::Str(value)) => Some(value.clone()),
                                        _ => None,
                                    }).unwrap_or_else(|| "CBOR value does not match requested type".to_string());
                                    let path = if path.is_empty() {
                                        "$".to_string()
                                    } else {
                                        format!("${path}")
                                    };
                                    (path, reason)
                                }
                                _ => (String::new(), "CBOR value does not match requested type".to_string()),
                            };
                            Ok(CtValue::ResErr(Box::new(CtValue::Struct {
                                type_name: "CBORError".to_string(),
                                fields: vec![
                                    ("kind".to_string(), CtValue::Enum {
                                        type_name: "CBORErrorKind".to_string(),
                                        variant: "TypeMismatch".to_string(),
                                        args: Vec::new(),
                                    }),
                                    ("byte_offset".to_string(), CtValue::Int(0)),
                                    ("path".to_string(), CtValue::Str(path)),
                                    ("reason".to_string(), CtValue::Str(reason)),
                                ],
                            })))
                        }
                    };
                }
                // D-CTEFFECT1 Tier-1: fetch is hermetic (sha256-pinned); no gate.
                if module == "core.net" && method == "fetch" {
                    return self.eval_net_fetch(argv, span);
                }
                // U13 (D-JPK-SECRETCRYPTO1): `core.vault.get` is denied at build time
                // unconditionally — unlike the Tier-2 effects below, there is no
                // `@Impure`/`--allow-impure` escape hatch, because a build artifact
                // must never bake in a decrypted secret (I1).
                if module == "core.vault" {
                    return Err(Diagnostic::error(
                        "E1265",
                        format!("`{}.{}()` can't be reached from a build-time context", module, method),
                        "module-field/comptime evaluation runs before secrets are ever decrypted; \
                         a repo's encrypted store is only ever opened at ordinary runtime, and — \
                         unlike the Tier-2 comptime effect gate — there is no `@Impure` escape hatch \
                         here.".to_string(),
                        "move the secret read out of comptime/module-field evaluation and into \
                         ordinary runtime code.".to_string(),
                        Some(span),
                    ));
                }
                // D-CTEFFECT1: Tier-2 effect calls require an @Impure gate (or REPL sandbox).
                let is_tier2 = matches!(
                    module.as_str(),
                    "core.files"
                        | "core.env"
                        | "core.io"
                        | "core.exec"
                        | "core.net"
                        | "core.tls"
                        | "core.process"
                ) || (self.repl_mode && module == "core.random" && method != "rng");
                if is_tier2 {
                    if self.repl_mode && matches!((module.as_str(), method), ("core.io", "eprint")) {
                        return apply_impure_core_call(
                            &module,
                            method,
                            argv,
                            span,
                            self.base_dir,
                            self.sink.as_deref_mut(),
                            true,
                            None,
                            None,
                        );
                    }
                    let ambient_random = self.repl_mode
                        && module == "core.random"
                        && !matches!(method, "rng");
                    if ambient_random {
                        // Ambient draws and global seeding consume/mutate session RNG state.
                        // Explicit `random.rng(seed)` is injected data and stays pure.
                    }
                    let mut repl_executable = None;
                    let mut repl_root = None;
                    if self.repl_mode {
                        if matches!((module.as_str(), method), ("core.process", "run")) {
                            repl_executable = Some(pin_repl_command(&mut argv, self.base_dir, span)?);
                        }
                        let request = repl_effect_request(&module, method, &argv);
                        let Some(authorizer) = self.repl_authorizer.as_deref_mut() else {
                            return Err(Diagnostic::error(
                                "E1803",
                                format!("{}.{} for `{}` was denied", request.root, request.operation, request.resource),
                                "this REPL mode has no runtime authority provider, so the host operation did not run".to_string(),
                                format!("restart with `jet repl --allow-{}` or use an interactive session and approve the exact operation", request.root.to_ascii_lowercase()),
                                Some(span),
                            ));
                        };
                        authorizer.preflight(&request, span)?;
                        let granted = self.repl_grants.iter().any(|cap| {
                            cap == &request.root || cap.starts_with(&format!("{}.", request.root))
                        });
                        if !granted {
                            return Err(Diagnostic::error(
                                "E1803",
                                format!("{}.{} for `{}` has no REPL runtime authority", request.root, request.operation, request.resource),
                                "REPL host effects require both lexical `@Grant` authority and invocation policy; no host operation ran".to_string(),
                                format!("wrap this operation in `@Grant({}) {{ caps -> ... }}`; interactive sessions then prompt, while non-TTY sessions also need `--allow-{}`", request.root, request.root.to_ascii_lowercase()),
                                Some(span),
                            ));
                        }
                        authorizer.authorize(&request, span)?;
                        if module == "core.files" {
                            return apply_repl_fs_call(method, &argv, span, authorizer);
                        }
                        if ambient_random {
                            return apply_core_call(&module, method, argv, span, true);
                        }
                        if module == "core.process" && method == "run" {
                            repl_root = Some(authorizer.verified_root().map_err(|error| {
                                unsupported(&format!("REPL project root handle is unavailable: {error}"), span)
                            })?);
                        }
                    }
                    if self.impure_depth == 0 {
                        if self.repl_mode {
                            unreachable!("REPL lexical grant checked above");
                        }
                        return Err(Diagnostic::error(
                            "E3410",
                            format!("`{}.{}()` is a Tier-2 comptime effect — it requires a `@Impure` gate", module, method),
                            "ambient I/O (filesystem, environment, process) is not allowed in \
                             pure comptime evaluation".to_string(),
                            "wrap the comptime binding in `@Impure(\"reason\") { … }` and \
                             pass `--allow-impure` to the build".to_string(),
                            Some(span),
                        ));
                    }
                    // Gate present (impure_depth > 0) but check --allow-impure flag too.
                    if !self.allow_impure {
                        return Err(Diagnostic::error(
                            "E3411",
                            format!("`{}.{}()` inside `@Impure` gate, but `--allow-impure` was not passed", module, method),
                            "the `@Impure` block opts in to ambient comptime I/O, but the build \
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
                            self.repl_mode,
                            repl_executable.as_ref(),
                            repl_root.as_ref(),
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
            super::super::Build::eval_program_build_method(
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
