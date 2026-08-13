//! Call/method dispatch for the interpreter: `eval_call`, `eval_method`,
//! `eval_require`, `eval_embed_file`. These are further
//! `impl Interp` methods; the struct and spine live in `interp.rs`.

#[path = "../SequenceParity.rs"]
mod sequence_parity;
#[path = "dispatch/eval_method.rs"]
mod eval_method;

use std::collections::HashMap;
use std::path::Path;

use crate::Diagnostics::{Diagnostic, Span};
use jet_foundation::Effects::{core_requires_comptime_gate, is_nondeterministic_core};
use crate::AST::{
    AccessConvention, CallArg, CtFloat, Expr, Func, LambdaBody, StrPart, Type, UnOp,
};

use super::super::Builtins::{
    apply_method, apply_mutating, apply_static_type_method, as_bool, as_int, cmp,
};
use super::super::Diagnostics::{comptime_panic, unsupported};
use super::super::Diagnostics::{EARLY_RETURN_CODE, ERR_PROPAGATE_CODE};
use super::super::Interpreter::{Flow, Interp};
use crate::AST::CtValue;
use jet_foundation::Prelude::jet_as_bytes as as_bytes;
use jet_foundation::Names::{mangle, mangle_path};
use super::core_calls::{
    apply_core_call_with_type, apply_data_line_call,
    apply_impure_core_call_with_type, as_float, display_core_pure_value,
    eval_regex_replace_all_with, sketch_add, solver_new, solver_require,
};
use super::repl_process::{
    apply_repl_authorized_core_call_with_type,
};

mod seeded_random_kernel {
    include!("../../../../jet-codegen/src/Prelude/Core/SeededRandom.rs");
}

fn seeded_rng_int(state: &mut u64, low: i64, high: i64) -> i64 {
    seeded_random_kernel::jet_seeded_rng_int(state, low, high)
}

fn seeded_rng_float(state: &mut u64) -> f64 {
    seeded_random_kernel::jet_seeded_rng_float(state)
}

fn sorted_unique(mut items: Vec<CtValue>, span: Span) -> Result<Vec<CtValue>, Diagnostic> {
    let mut sort_error = None;
    items.sort_by(|left, right| match cmp(left.clone(), right.clone(), span) {
        Ok(order) => order,
        Err(error) => {
            sort_error.get_or_insert(error);
            std::cmp::Ordering::Equal
        }
    });
    if let Some(error) = sort_error {
        return Err(error);
    }
    items.dedup();
    Ok(items)
}

fn unique_values(items: Vec<CtValue>) -> Vec<CtValue> {
    // ponytail: comptime sets are small; O(n²) equality dedup avoids a second
    // hash representation. Revisit only with measured large-set evidence.
    let mut unique = Vec::new();
    for item in items {
        if !unique.contains(&item) {
            unique.push(item);
        }
    }
    unique
}

/// D-META-USER1=A: turn the four declared rejection fields into the one
/// registered project diagnostic used by both comptime evaluators.
pub fn project_rejection(args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let [
        CtValue::Str(code),
        CtValue::Str(what),
        CtValue::Str(why),
        CtValue::Str(fix),
    ] = args
    else {
        return Err(unsupported(
            "`reject` requires String code, what, why, and fix arguments",
            span,
        ));
    };
    if jet_foundation::Registry::diagnostic(code).is_none() {
        return Err(unsupported(
            &format!("`reject` uses unregistered diagnostic `{code}`"),
            span,
        ));
    }
    Err(Diagnostic::project_error(
        code.clone(),
        what.clone(),
        why.clone(),
        fix.clone(),
        Some(span),
    )
    .expect("registered rule diagnostic"))
}

pub fn is_tier2_core_call(module: &str, method: &str, repl_mode: bool) -> bool {
    // `app.live(…)` and friends are the web module's live-query registry under
    // the entry alias; resolve the alias before asking for the fact.
    let resolved = if module == "app" { "core.web" } else { module };
    if core_requires_comptime_gate(resolved, method) {
        return true;
    }
    // The REPL re-reads ambient randomness between lines, so a folded draw
    // would go stale; the seeded constructor stays deterministic.
    repl_mode && is_nondeterministic_core(resolved, method)
}

pub fn vault_comptime_denied(module: &str, method: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1265",
        format!("`{module}.{method}()` can't be reached from a build-time context"),
        "module-field/comptime evaluation runs before secrets are ever decrypted; \
         a repo's encrypted store is only ever opened at ordinary runtime, and — \
         unlike the Tier-2 comptime effect gate — there is no `#Impure` escape hatch \
         here."
            .to_string(),
        "move the secret read out of comptime/module-field evaluation and into \
         ordinary runtime code."
            .to_string(),
        Some(span),
    )
}

fn sorted_descending(mut items: Vec<CtValue>, span: Span) -> Result<Vec<CtValue>, Diagnostic> {
    let mut sort_error = None;
    items.sort_by(|left, right| match cmp(right.clone(), left.clone(), span) {
        Ok(order) => order,
        Err(error) => {
            sort_error.get_or_insert(error);
            std::cmp::Ordering::Equal
        }
    });
    match sort_error {
        Some(error) => Err(error),
        None => Ok(items),
    }
}

pub fn apply_seeded_rng_method(
    state: &mut u64,
    method: &str,
    args: &mut [CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    apply_seeded_rng_method_with_type(state, method, args, span, None)
}

pub fn apply_seeded_rng_method_with_type(
    state: &mut u64,
    method: &str,
    args: &mut [CtValue],
    span: Span,
    resolved_ret: Option<&Type>,
) -> Result<CtValue, Diagnostic> {
    let float = |index| {
        args.get(index)
            .ok_or_else(|| unsupported("this Rng method argument", span))
            .and_then(|value| as_float(value, span))
    };
    let list = |index| match args.get(index) {
        Some(CtValue::List(values)) => Ok(values.as_slice()),
        _ => Err(unsupported("this Rng method list argument", span)),
    };
    match method {
        "int" => Ok(CtValue::Int(seeded_rng_int(
            state,
            as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?,
            as_int(args.get(1).unwrap_or(&CtValue::Int(0)), span)?,
        ))),
        "float" => Ok(CtValue::Float(CtFloat::f64(seeded_rng_float(state)))),
        "float_range" => {
            let low = float(0)?;
            let high = float(1)?;
            Ok(CtValue::Float(CtFloat::f64(
                seeded_random_kernel::jet_seeded_rng_float_range(state, low, high),
            )))
        }
        "bool" if args.is_empty() => Ok(CtValue::Bool(
            seeded_random_kernel::jet_seeded_rng_bool(state),
        )),
        "bool" => {
            let p = float(0)?;
            Ok(CtValue::Bool(
                seeded_random_kernel::jet_seeded_rng_bool_p(state, p),
            ))
        }
        "normal" => {
            let mean = float(0)?;
            let stddev = float(1)?;
            Ok(CtValue::Float(CtFloat::f64(
                seeded_random_kernel::jet_seeded_rng_normal(state, mean, stddev),
            )))
        }
        "exponential" => {
            let lambda = float(0)?;
            Ok(CtValue::Float(CtFloat::f64(
                seeded_random_kernel::jet_seeded_rng_exponential(state, lambda),
            )))
        }
        "bytes" => {
            let count = as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?;
            Ok(CtValue::Bytes(
                seeded_random_kernel::jet_seeded_rng_bytes(state, count),
            ))
        }
        "split" => Ok(CtValue::Struct {
            type_name: crate::Syntax::RNG_TYPE.to_string(),
            fields: vec![(
                "state".to_string(),
                CtValue::Int(seeded_random_kernel::jet_seeded_rng_split(state) as i64),
            )],
        }),
        "pick" => {
            let values = list(0)?;
            match seeded_random_kernel::jet_seeded_rng_pick(state, &values.to_vec()) {
                Some(value) => Ok(CtValue::Present(Box::new(value))),
                None => Ok(CtValue::absent(
                    CtValue::resolved_option_element_type(resolved_ret).ok_or_else(|| {
                        unsupported("Rng.pick needs a resolved element type", span)
                    })?,
                )),
            }
        }
        "weighted_pick" => {
            let values = list(0)?;
            let weights = list(1)?;
            if values.is_empty() || values.len() != weights.len() {
                return Ok(CtValue::absent(
                    CtValue::resolved_option_element_type(resolved_ret).ok_or_else(|| {
                        unsupported("Rng.weighted_pick needs a resolved element type", span)
                    })?,
                ));
            }
            let weights = weights
                .iter()
                .map(|weight| as_float(weight, span))
                .collect::<Result<Vec<_>, _>>()?;
            match seeded_random_kernel::jet_seeded_rng_weighted_pick(
                state,
                &values.to_vec(),
                &weights,
            ) {
                Some(value) => Ok(CtValue::Present(Box::new(value))),
                None => Ok(CtValue::absent(
                    CtValue::resolved_option_element_type(resolved_ret).ok_or_else(|| {
                        unsupported("Rng.weighted_pick needs a resolved element type", span)
                    })?,
                )),
            }
        }
        "sample" => {
            let values = list(0)?;
            let count = as_int(args.get(1).unwrap_or(&CtValue::Int(0)), span)?;
            Ok(CtValue::List(seeded_random_kernel::jet_seeded_rng_sample(
                state,
                &values.to_vec(),
                count,
            )))
        }
        "shuffle" => {
            let Some(CtValue::List(values)) = args.first_mut() else {
                return Err(unsupported("Rng.shuffle with a non-list argument", span));
            };
            seeded_random_kernel::jet_seeded_rng_shuffle(state, values);
            Ok(CtValue::Unit)
        }
        _ => Err(unsupported("this Rng method", span)),
    }
}

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
    check_literal_embed_path(builtin, &path, span)
}

pub(crate) fn check_literal_embed_path(
    builtin: &str,
    path: &str,
    span: Span,
) -> Result<String, Diagnostic> {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        return Err(embed_path_err(builtin, "absolute", span));
    }
    if p.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(embed_path_err(builtin, "escape", span));
    }
    Ok(path.to_string())
}

pub(crate) fn embed_path_err(builtin: &str, kind: &str, span: Span) -> Diagnostic {
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

pub(crate) fn find_glob(
    base_dir: &Path,
    glob: &str,
    span: Span,
) -> Result<Vec<String>, Diagnostic> {
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

/// D-CTIO1: the one implementation of `embed_file` / `embed_bytes` / `find`.
/// `literal` is the argument's source string literal, or `None` when the
/// argument was computed (E0957). Both the AST dispatcher below and the
/// canonical TIR evaluator call this, so the path law and its diagnostics live
/// in exactly one place.
pub fn eval_build_time_io(
    builtin: &str,
    base_dir: &Path,
    literal: Option<&str>,
    mut embed_inputs: Option<&mut Vec<crate::AST::ComptimeInput>>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let Some(literal) = literal else {
        return Err(embed_path_err(builtin, "literal", span));
    };
    let rel = check_literal_embed_path(builtin, literal, span)?;
    if builtin == crate::Syntax::BUILTIN_FIND {
        return eval_locked_find(base_dir, &rel, embed_inputs, span);
    }
    let bytes = std::fs::read(base_dir.join(&rel)).map_err(|error| {
        Diagnostic::error(
            "E0955",
            format!("`{builtin}` can't open `{rel}`"),
            format!("{error} (looked next to the file doing the embedding)"),
            "check the path — it is relative to the file's own directory".to_string(),
            Some(span),
        )
    })?;
    // D-CTEFFECT1 Tier-1: record the embed input hash for .jet/lock.
    if let Some(inputs) = embed_inputs.as_deref_mut() {
        inputs.push(crate::AST::ComptimeInput {
            path: rel.clone(),
            hash: crate::SHA256::sha256_hex(&bytes),
        });
    }
    if builtin == crate::Syntax::BUILTIN_EMBED_BYTES {
        return Ok(CtValue::Bytes(bytes));
    }
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

/// The source string literal of a call argument, or `None` when computed.
pub(crate) fn arg_string_literal(arg: &CallArg) -> Option<&str> {
    match &arg.expr {
        Expr::Str(parts, _) if parts.len() == 1 => match &parts[0] {
            StrPart::Lit(value) => Some(value.as_str()),
            _ => None,
        },
        _ => None,
    }
}

pub fn eval_locked_find(
    base_dir: &Path,
    glob: &str,
    mut embed_inputs: Option<&mut Vec<crate::AST::ComptimeInput>>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let builtin = crate::Syntax::BUILTIN_FIND;
    let mut matches = find_glob(base_dir, glob, span)?;
    matches.sort();
    for rel in &matches {
        let bytes = std::fs::read(base_dir.join(rel)).map_err(|error| {
            Diagnostic::error(
                "E0955",
                format!("`{builtin}` can't open `{rel}`"),
                format!("{error} (matched while expanding `{glob}`)"),
                "check the glob and remove unreadable files from its match set".to_string(),
                Some(span),
            )
        })?;
        if let Some(inputs) = embed_inputs.as_deref_mut() {
            inputs.push(crate::AST::ComptimeInput {
                path: rel.clone(),
                hash: crate::SHA256::sha256_hex(&bytes),
            });
        }
    }
    Ok(CtValue::List(
        matches.into_iter().map(CtValue::Str).collect(),
    ))
}

pub fn eval_build_embed(
    args: &[CtValue],
    base_dir: &Path,
    embed_inputs: Option<&mut Vec<crate::AST::ComptimeInput>>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let rel = match args.first() {
        Some(CtValue::Str(path)) => path,
        _ => return Err(unsupported("`b.embed` requires a path string", span)),
    };
    let path = Path::new(rel);
    if args.len() != 1
        || path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(Diagnostic::error(
            "E0957",
            format!("`b.embed` path `{rel}` escapes the build root"),
            "locked build inputs must stay beneath the selected source directory".to_string(),
            "use a relative path returned by `b.find`, without `..`".to_string(),
            Some(span),
        ));
    }
    let bytes = std::fs::read(base_dir.join(path)).map_err(|error| {
        Diagnostic::error(
            "E0955",
            format!("`b.embed` cannot open `{rel}`"),
            error.to_string(),
            "check the locked relative path".to_string(),
            Some(span),
        )
    })?;
    if let Some(inputs) = embed_inputs {
        inputs.push(crate::AST::ComptimeInput {
            path: rel.clone(),
            hash: crate::SHA256::sha256_hex(&bytes),
        });
    }
    String::from_utf8(bytes).map(CtValue::Str).map_err(|_| {
        Diagnostic::error(
            "E0955",
            format!("`b.embed` cannot decode `{rel}` as text"),
            "the embedded file is not valid UTF-8".to_string(),
            "embed a UTF-8 text file".to_string(),
            Some(span),
        )
    })
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

/// D-META-STAGE1=B (formerly D-CTMARKER1=C's splice spelling): substitute
/// `$name` mentions in a string with their compile-time value from the
/// comptime scope. Unknown names are left as-is (`$unknown`). Used by `emit(…)`.
pub fn apply_dollar_splices(s: &str, scope: &HashMap<String, CtValue>) -> String {
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

fn integer_width_from_type(ty: &Type) -> Option<u32> {
    match ty {
        Type::Int => Some(64),
        Type::IntN { bits, .. } => Some(u32::from(*bits)),
        _ => None,
    }
}

fn integer_width_from_name(name: &str) -> Option<u32> {
    crate::AST::numeric_type_from_name(name)
        .as_ref()
        .and_then(integer_width_from_type)
}

/// D-CTEFFECT1 Tier-1 / D-NETDEP1=A: evaluate a sha256-pinned text fetch and
/// record its lock input. Shared by AST and canonical TIR evaluation.
pub fn eval_net_fetch(
    args: &[CtValue],
    embed_inputs: Option<&mut Vec<crate::AST::ComptimeInput>>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let url = match args.first() {
        Some(CtValue::Str(s)) => s.clone(),
        _ => {
            return Err(Diagnostic::error(
                "E3414",
                "fetch: first argument must be a string URL".to_string(),
                "`core.net.fetch` expects a string URL as its first argument".to_string(),
                "pass a string literal: `net.fetch('https://example.com/data.txt', sha256: '…')`"
                    .to_string(),
                Some(span),
            ))
        }
    };
    let expected = match args.get(1) {
        Some(CtValue::Str(s)) => s.clone(),
        _ => {
            return Err(Diagnostic::error(
                "E3414",
                "fetch: `sha256:` argument missing or not a string".to_string(),
                "`core.net.fetch` requires a `sha256:` labelled argument for content verification"
                    .to_string(),
                "add `sha256: '<64-hex-chars>'` as the second argument".to_string(),
                Some(span),
            ))
        }
    };

    let bytes = jet_net::fetch(&url).map_err(|error| {
        Diagnostic::error(
            error.diagnostic_code(),
            error.diagnostic_what(&url),
            error.diagnostic_why(&url),
            error.diagnostic_fix(),
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
    let content = String::from_utf8(bytes).map_err(|_| {
        Diagnostic::error(
            "E3414",
            format!("fetch: content at `{url}` is not valid UTF-8"),
            "the downloaded bytes could not be decoded as UTF-8 text".to_string(),
            "binary content is not supported by comptime fetch; use `embed_bytes` for binary data"
                .to_string(),
            Some(span),
        )
    })?;
    if let Some(inputs) = embed_inputs {
        inputs.push(crate::AST::ComptimeInput {
            path: format!("url:{url}"),
            hash: actual,
        });
    }
    Ok(CtValue::Str(content))
}

impl<'a> Interp<'a> {

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
        // these are unreachable (the purity check rejects them first, E3401).
        if name == "print" || name == "eprint" {
            if self.sink.is_some() {
                let text = match args.first() {
                    // D-DISPLAYDBG1/2: same Display-impl-aware rendering as
                    // `{value}` string interpolation (`show_value`) — `print`
                    // is bare-Display too, never the `:Debug` form.
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
        // D-META-USER1=A: a checked rule may reject with a registered
        // project diagnostic. Labels keep the product fields explicit; the
        // registry still owns the code's severity and rendering metadata.
        if name == "reject" {
            let mut values = HashMap::new();
            for argument in args {
                let Some((label, _)) = &argument.label else {
                    return Err(unsupported(
                        "`reject` requires code, what, why, and fix labels",
                        span,
                    ));
                };
                values.insert(label.as_str(), self.eval(&argument.expr, scope)?);
            }
            let text = |name: &str| -> Result<String, Diagnostic> {
                match values.get(name) {
                    Some(CtValue::Str(value)) => Ok(value.clone()),
                    _ => Err(unsupported(
                        &format!("`reject` requires a String `{name}` argument"),
                        span,
                    )),
                }
            };
            let code = text("code")?;
            let what = text("what")?;
            let why = text("why")?;
            let fix = text("fix")?;
            return project_rejection(
                &[
                    CtValue::Str(code),
                    CtValue::Str(what),
                    CtValue::Str(why),
                    CtValue::Str(fix),
                ],
                span,
            );
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
        // D-DECIMAL1: `Decimal("10.50")` — explicit string construction only.
        if name == crate::Syntax::TYPE_DECIMAL
            && self.funcs.get(name).is_none()
            && !scope.contains_key(name)
        {
            let arg = match args.first() {
                Some(a) => self.eval(&a.expr, scope)?,
                None => return Err(unsupported("`Decimal` with no argument", span)),
            };
            return match arg {
                CtValue::Str(s) => crate::Numeric::CtDecimal::from_str(&s)
                    .map(|decimal| decimal.to_value())
                    .map_err(|_| unsupported(&format!("`Decimal(\"{}\")`", s), span)),
                _ => Err(unsupported("`Decimal` with a non-String argument", span)),
            };
        }
        // D-SIMD2 / D-LINALG1: `Vec3(…)` / `F32x4(…)` / `Mat3(…)` constructors.
        if (name == crate::Syntax::LINALG_VEC2_TYPE
            || name == crate::Syntax::LINALG_VEC3_TYPE
            || name == crate::Syntax::LINALG_VEC4_TYPE
            || name == crate::Syntax::LINALG_MAT3_TYPE
            || name == crate::Syntax::LINALG_MAT4_TYPE
            || name == crate::Syntax::SIMD_F32X4_TYPE
            || name == crate::Syntax::SIMD_F64X2_TYPE)
            && self.funcs.get(name).is_none()
            && !scope.contains_key(name)
        {
            let mut vals = Vec::with_capacity(args.len());
            for a in args {
                vals.push(self.eval(&a.expr, scope)?);
            }
            return super::super::MathLayout::construct(name, &vals, span);
        }
        // D-LIN1-DROP: `consume(x)` — eval then discard.
        if name == crate::Syntax::BUILTIN_CONSUME
            && self.funcs.get(name).is_none()
            && !scope.contains_key(name)
        {
            if args.len() != 1 {
                return Err(unsupported("`consume` discards exactly one value", span));
            }
            let _ = self.eval(&args[0].expr, scope)?;
            return Ok(CtValue::Unit);
        }
        // D-TOOL4: `expect(x)` — wrap Display text for `.snapshot()`.
        if name == crate::Syntax::BUILTIN_EXPECT
            && self.funcs.get(name).is_none()
            && !scope.contains_key(name)
        {
            if args.len() != 1 {
                return Err(unsupported("`expect` needs exactly one value", span));
            }
            let value = self.eval(&args[0].expr, scope)?;
            let shown = self.show_value(&value, args[0].expr.span())?;
            return Ok(CtValue::Struct {
                type_name: "__JetExpect__".to_string(),
                fields: vec![("value".into(), CtValue::Str(shown))],
            });
        }
        // D-NUMOPS1: `wrapping`/`saturating`/`checked` over one integer binary.
        if name == crate::Syntax::BUILTIN_WRAPPING
            || name == crate::Syntax::BUILTIN_SATURATING
            || name == crate::Syntax::BUILTIN_CHECKED
        {
            if self.funcs.get(name).is_none() && !scope.contains_key(name) {
                return self.eval_overflow_opt(name, args, span, scope);
            }
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
                let (mut frame, writebacks) =
                    self.bind_fixed_call_args(fixed, &args[..fixed.len()], scope)?;
                let mut rest = Vec::with_capacity(args.len() - fixed.len());
                for a in &args[fixed.len()..] {
                    let value = self.eval(&a.expr, scope)?;
                    rest.push(super::super::Interpreter::coerce_value_to_type(
                        value, &last.ty,
                    ));
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
            let value = self.eval(&a.expr, scope)?;
            frame.insert(
                p.name.clone(),
                super::super::Interpreter::coerce_value_to_type(value, &p.ty),
            );
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
        let frame_types = func
            .params
            .iter()
            .map(|param| (param.name.clone(), param.ty.clone()))
            .collect();
        let prev_types = std::mem::replace(&mut self.binding_types, frame_types);
        self.depth = prev_depth + 1;
        let result = self.exec_block(&func.body, &mut frame);
        self.depth = prev_depth;
        self.cur_func = prev_func;
        self.binding_types = prev_types;
        let value = match result {
            Ok(Flow::Return(v)) => Ok(v),
            Ok(_) => Ok(CtValue::Unit),
            Err(ref d) if d.code == ERR_PROPAGATE_CODE => {
                // c97/D-STRPARSE1: `?` on an `Err` or `null` propagated via
                // the sentinel — convert to an `Err` return from this callee
                // so the caller can handle it (e.g. with `??`).
                let msg = d.what.clone();
                Ok(CtValue::failed(Box::new(CtValue::Str(msg))))
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
        let value = func.return_type.as_ref().map_or(value.clone(), |ty| {
            super::super::Interpreter::coerce_value_to_type(value, ty)
        });
        Ok((value, frame))
    }

    /// c139 (D-DISPLAYDBG1/2): render `v` as `{value}` interpolation / `print`
    /// would in the compiled program. When `v`'s type has a user-written
    /// `impl Type.Display { fn display(self) => String }`, run that exact Jet
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
        if let Some(rendered) = display_core_pure_value(v) {
            return Ok(rendered);
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
                            format!("{}: {}", mangle(&field.name), rendered)
                        })
                        .collect();
                    return format!("{} {{ {} }}", mangle_path(type_name), parts.join(", "));
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
        self.call_closure_inner(f, args, span, None)
    }

    pub(in super::super) fn call_inline_closure(
        &mut self,
        f: &CtValue,
        args: Vec<CtValue>,
        span: Span,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        self.call_closure_inner(f, args, span, Some(scope))
    }

    fn call_closure_inner(
        &mut self,
        f: &CtValue,
        args: Vec<CtValue>,
        span: Span,
        mut writeback_scope: Option<&mut HashMap<String, CtValue>>,
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
        if let Some(scope) = writeback_scope.as_deref() {
            // The public REPL evaluates its retained source AST, while sema's
            // capture metadata lives on the checked clone. Closures already
            // over-capture that source scope, so the explicit writeback path
            // synchronizes only bindings that still exist in the caller.
            for name in data.captured.keys() {
                if let Some(value) = scope.get(name) {
                    frame.insert(name.clone(), value.clone());
                }
            }
        }
        let previous_types = self.binding_types.clone();
        for (p, a) in data.lambda.params.iter().zip(args) {
            if let Some(ty) = &p.ty {
                self.binding_types.insert(p.name.clone(), ty.clone());
                frame.insert(
                    p.name.clone(),
                    super::super::Interpreter::coerce_value_to_type(a, ty),
                );
            } else {
                frame.insert(p.name.clone(), a);
            }
        }
        let result = (|| match &data.lambda.body {
            LambdaBody::Expr(e) => self.eval(e, &mut frame),
            LambdaBody::Block(stmts) => match self.exec_block(stmts, &mut frame)? {
                Flow::Return(v) => Ok(v),
                _ => Ok(CtValue::Unit),
            },
        })();
        self.binding_types = previous_types;
        if result.is_ok() {
            if let Some(scope) = writeback_scope.as_deref_mut() {
                for name in data.captured.keys() {
                    let shadowed = data.lambda.params.iter().any(|param| param.name == *name);
                    let taken = data.lambda.take_names.iter().any(|(taken, _)| taken == name);
                    if !shadowed && !taken && scope.contains_key(name) {
                        let value = frame.get(name).expect("captured binding stays in frame");
                        scope.insert(name.clone(), value.clone());
                    }
                }
            }
        }
        result.map(|value| {
            data.return_type.as_ref().map_or(value.clone(), |ty| {
                super::super::Interpreter::coerce_value_to_type(value, ty)
            })
        })
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
            CtValue::Present(Box::new(CtValue::Int(n)))
        } else {
            CtValue::failed(Box::new(CtValue::Str(format!(
                "{} out of range {}..{}",
                n, lo, hi
            ))))
        })
    }

    fn eval_overflow_opt(
        &mut self,
        mode: &str,
        args: &[CallArg],
        span: Span,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let Some(arg) = args.first() else {
            return Err(unsupported(&format!("`{mode}` needs one binary expression"), span));
        };
        let Expr::Binary(op, left, right, _) = &arg.expr else {
            return Err(unsupported(
                &format!("`{mode}` wraps one integer +/−/*/÷"),
                span,
            ));
        };
        let lv = self.eval(left, scope)?;
        let rv = self.eval(right, scope)?;
        let left_n = as_int(&lv, left.span())?;
        let right_n = as_int(&rv, right.span())?;
        let width = self
            .overflow_opt_width(left.as_ref())
            .or_else(|| self.overflow_opt_width(right.as_ref()))
            .unwrap_or((true, 64));
        super::super::MathLayout::overflow_opt(mode, *op, left_n, right_n, width.0, width.1, span)
    }

    fn overflow_opt_width(&self, expr: &Expr) -> Option<(bool, u8)> {
        match expr {
            Expr::Ident(name, _) => match self.binding_types.get(name) {
                Some(Type::IntN { signed, bits }) => Some((*signed, *bits)),
                Some(Type::Int) => Some((true, 64)),
                _ => None,
            },
            Expr::Binary(_, left, right, _) => self
                .overflow_opt_width(left)
                .or_else(|| self.overflow_opt_width(right)),
            _ => None,
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
        self.eval_build_time_io(crate::Syntax::BUILTIN_EMBED_FILE, args, span)
    }

    /// D-CTIO1 + D-ARROW-CONTROL1: `embed_bytes("path") => [U8]` — the
    /// binary-safe sibling of
    /// `embed_file`. Same path-safety (E0957) and missing/unreadable (E0955)
    /// checks, but no UTF-8 requirement: any file embeds as raw bytes.
    fn eval_embed_bytes(&mut self, args: &[CallArg], span: Span) -> Result<CtValue, Diagnostic> {
        self.eval_build_time_io(crate::Syntax::BUILTIN_EMBED_BYTES, args, span)
    }

    /// D-CTFIND1/2 + D-ARROW-CONTROL1: `find(glob) => [String]` walks inside
    /// the source file's
    /// directory, returns sorted relative file paths, and records each match's
    /// hash as Tier-1 lock evidence.
    fn eval_find(&mut self, args: &[CallArg], span: Span) -> Result<CtValue, Diagnostic> {
        self.eval_build_time_io(crate::Syntax::BUILTIN_FIND, args, span)
    }

    /// Arity and literal extraction for the three build-time IO builtins; the
    /// path law, the reads, and every diagnostic live in `eval_build_time_io`.
    fn eval_build_time_io(
        &mut self,
        builtin: &str,
        args: &[CallArg],
        span: Span,
    ) -> Result<CtValue, Diagnostic> {
        let noun = if builtin == crate::Syntax::BUILTIN_FIND {
            "glob"
        } else {
            "path"
        };
        let arg = args
            .first()
            .ok_or_else(|| unsupported(&format!("{builtin} with no {noun}"), span))?;
        if args.len() != 1 {
            return Err(unsupported(
                &format!("{builtin} with extra arguments"),
                span,
            ));
        }
        eval_build_time_io(
            builtin,
            self.base_dir,
            arg_string_literal(arg),
            Some(&mut self.embed_inputs),
            span,
        )
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
            // D-META-STAGE1=B: the mark is part of the identifier, so a marked
            // name is written back exactly like a plain one. Without this a
            // marked receiver never advances: `$ct.read_u8()` twice read the
            // same byte while the runtime copy moved on.
            Expr::Ident(name, _) | Expr::ComptimeName { name, .. } => {
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

    fn integer_width_for_expr(&self, expr: &Expr) -> Option<u32> {
        match expr {
            Expr::Ident(name, _) => self
                .binding_types
                .get(name)
                .and_then(integer_width_from_type),
            Expr::Call(call) => self
                .funcs
                .get(&call.name)
                .and_then(|func| func.return_type.as_ref())
                .and_then(integer_width_from_type),
            Expr::Int(_, _, width, _) => Some(width.map_or(64, |(_, bits)| u32::from(bits))),
            Expr::Unary(_, inner, _) | Expr::Copy(inner, _) => self.integer_width_for_expr(inner),
            Expr::Binary(_, left, _, _) => self.integer_width_for_expr(left),
            Expr::MethodCall { receiver, .. } => match receiver.as_ref() {
                Expr::Ident(type_name, _) => integer_width_from_name(type_name),
                _ => None,
            },
            _ => None,
        }
    }
}
