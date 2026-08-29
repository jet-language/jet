//! `core.regex` evaluator, kept separate from the Core call registry.

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::CtValue;

use super::super::super::Builtins::{as_bool, as_int};
use super::super::super::Diagnostics::unsupported;
use super::as_string;

pub(in super::super::super) fn apply_regex_method(
    recv: &CtValue,
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    if !matches!(
        recv,
        CtValue::Struct { type_name, .. } if type_name == "__JetRegex"
    ) {
        return None;
    }
    let mut call_args = Vec::with_capacity(args.len() + 1);
    call_args.push(recv.clone());
    call_args.extend_from_slice(args);
    Some(match method {
        "pattern" | "source" | "flags" | "options" | "names" | "count" => {
            regex_metadata(call_args, method, span)
        }
        "is_match" => regex_is_match(call_args, span),
        "full_match" => regex_full_match(call_args, span),
        "find" => regex_find(call_args, span),
        "find_all" => regex_find_all(call_args, span),
        "matches" => regex_matches(call_args, span),
        "split" => regex_split(call_args, span),
        "split_limit" => regex_split_limit(call_args, span),
        "replace" => regex_replace(call_args, span, false),
        "replace_first" => regex_replace(call_args, span, true),
        "replace_all" => regex_replace(call_args, span, false),
        "match" => regex_match(call_args, span),
        _ => return None,
    })
}

fn regex_metadata(
    args: Vec<CtValue>,
    method: &str,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let re = regex_pattern(&args, span)?;
    match method {
        "pattern" | "source" => Ok(CtValue::Str(re.pattern())),
        "flags" | "options" => Ok(CtValue::Str(re.flags())),
        "names" => Ok(CtValue::List(re.names().into_iter().map(CtValue::Str).collect())),
        "count" => Ok(CtValue::Int(re.count(as_string(
            args.get(1)
                .ok_or_else(|| unsupported("regex.count: missing text argument", span))?,
            span,
        )?))),
        _ => unreachable!("guarded regex metadata method"),
    }
}

pub fn eval_regex_replace_all_with(
    recv: &CtValue,
    args: &[CtValue],
    span: Span,
    invoke: &mut impl FnMut(CtValue, Vec<CtValue>) -> Result<CtValue, Diagnostic>,
) -> Option<Result<CtValue, Diagnostic>> {
    if !matches!(
        recv,
        CtValue::Struct { type_name, .. } if type_name == "__JetRegex"
    ) {
        return None;
    }
    Some((|| {
        let re = regex_pattern(std::slice::from_ref(recv), span)?;
        let text = as_string(
            args.first().ok_or_else(|| {
                unsupported("regex.replace_all_with: missing text argument", span)
            })?,
            span,
        )?;
        let callback = args.get(1).ok_or_else(|| {
            unsupported("regex.replace_all_with: missing callback argument", span)
        })?;
        let replaced = re.replace_all_with_result(text, |found| {
            let value = invoke(callback.clone(), vec![regex_match_value(text, found)])?;
            Ok::<String, Diagnostic>(as_string(&value, span)?.to_string())
        })?;
        Ok(CtValue::Str(replaced))
    })())
}

pub(super) fn regex_pattern(
    args: &[CtValue],
    span: Span,
) -> Result<super::super::super::regex_kernel::JetRegex, Diagnostic> {
    let value = args
        .first()
        .ok_or_else(|| unsupported("regex call: missing pattern argument", span))?;
    let (pat, flags) = match value {
        CtValue::Str(pattern) => (
            pattern.as_str(),
            super::super::super::regex_kernel::RegexFlags::default(),
        ),
        CtValue::Struct { type_name, fields } if type_name == "__JetRegex" => {
            let pattern = fields
                .iter()
                .find_map(|(name, value)| match (name.as_str(), value) {
                    ("pattern", CtValue::Str(pattern)) => Some(pattern.as_str()),
                    _ => None,
                })
                .ok_or_else(|| unsupported("Regex literal value", span))?;
            let flags = fields
                .iter()
                .find(|(name, _)| name == "flags")
                .map(|(_, value)| regex_flags_value(value, span))
                .transpose()?
                .unwrap_or_default();
            (pattern, flags)
        }
        _ => return Err(unsupported("Regex pattern value", span)),
    };
    super::super::super::regex_kernel::jet_regex_compile_with(pat, &flags).map_err(|e| {
        Diagnostic::error(
            "E0956",
            format!("bad regex pattern: {}", e),
            "the pattern could not be compiled".to_string(),
            "fix the pattern syntax".to_string(),
            Some(span),
        )
    })
}

fn regex_flags_value(
    value: &CtValue,
    span: Span,
) -> Result<super::super::super::regex_kernel::RegexFlags, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("RegexFlags value", span));
    };
    if type_name != "__JetRegexFlags" {
        return Err(unsupported("RegexFlags value", span));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| as_bool(value, span))
            .transpose()
    };
    Ok(super::super::super::regex_kernel::RegexFlags {
        case_insensitive: field("case_insensitive")?.unwrap_or(false),
        multiline: field("multiline")?.unwrap_or(false),
        dotall: field("dotall")?.unwrap_or(false),
    })
}

pub(super) fn regex_flags(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let field = |index: usize, name: &str| {
        as_bool(
            args.get(index)
                .ok_or_else(|| unsupported(&format!("regex.flags: missing {name}"), span))?,
            span,
        )
    };
    Ok(CtValue::Struct {
        type_name: "__JetRegexFlags".to_string(),
        fields: vec![
            (
                "case_insensitive".to_string(),
                CtValue::Bool(field(0, "case_insensitive")?),
            ),
            (
                "multiline".to_string(),
                CtValue::Bool(field(1, "multiline")?),
            ),
            ("dotall".to_string(), CtValue::Bool(field(2, "dotall")?)),
        ],
    })
}

pub(super) fn regex_compile(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let pattern = as_string(
        args.first()
            .ok_or_else(|| unsupported("regex.compile: missing pattern", span))?,
        span,
    )?;
    let flags = args
        .get(1)
        .map(|value| regex_flags_value(value, span))
        .transpose()?
        .unwrap_or_default();
    match super::super::super::regex_kernel::jet_regex_compile_with(pattern, &flags) {
        Ok(_) => Ok(CtValue::Present(Box::new(CtValue::Struct {
            type_name: "__JetRegex".to_string(),
            fields: vec![
                ("pattern".to_string(), CtValue::Str(pattern.to_string())),
                (
                    "flags".to_string(),
                    CtValue::Struct {
                        type_name: "__JetRegexFlags".to_string(),
                        fields: vec![
                            (
                                "case_insensitive".to_string(),
                                CtValue::Bool(flags.case_insensitive),
                            ),
                            ("multiline".to_string(), CtValue::Bool(flags.multiline)),
                            ("dotall".to_string(), CtValue::Bool(flags.dotall)),
                        ],
                    },
                ),
            ],
        }))),
        Err(error) => Ok(CtValue::failed(Box::new(CtValue::Str(error)))),
    }
}

pub(super) fn regex_escape(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let text = as_string(
        args.first()
            .ok_or_else(|| unsupported("regex.escape: missing text argument", span))?,
        span,
    )?;
    Ok(CtValue::Str(escape_regex_text(text)))
}

fn escape_regex_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        let meta = ch == '\\'
            || ch == '.'
            || ch == '+'
            || ch == '*'
            || ch == '?'
            || ch == '('
            || ch == ')'
            || ch == '['
            || ch == ']'
            || ch == '{'
            || ch == '}'
            || ch == '^'
            || ch == '\u{24}'
            || ch == '\u{7c}';
        if meta {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

pub(super) fn regex_is_match(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let re = regex_pattern(&args, span)?;
    let text = as_string(
        args.get(1)
            .ok_or_else(|| unsupported("regex.is_match: missing text argument", span))?,
        span,
    )?;
    Ok(CtValue::Bool(re.is_match(text)))
}

pub(super) fn regex_full_match(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let re = regex_pattern(&args, span)?;
    let text = as_string(
        args.get(1)
            .ok_or_else(|| unsupported("regex.full_match: missing text argument", span))?,
        span,
    )?;
    Ok(CtValue::Bool(re.full_match(text)))
}

pub(super) fn regex_find(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let re = regex_pattern(&args, span)?;
    let text = as_string(
        args.get(1)
            .ok_or_else(|| unsupported("regex.find: missing text argument", span))?,
        span,
    )?;
    Ok(match re.find(text).ok() {
        Some(value) => CtValue::Present(Box::new(CtValue::Str(value))),
        None => CtValue::absent(crate::AST::Type::String),
    })
}

pub(super) fn regex_find_all(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
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
    Ok(CtValue::List(items))
}

pub(super) fn regex_matches(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let re = regex_pattern(&args, span)?;
    let text = as_string(
        args.get(1)
            .ok_or_else(|| unsupported("regex.matches: missing text argument", span))?,
        span,
    )?;
    Ok(CtValue::List(
        re.matches(text)
            .into_iter()
            .map(|found| regex_match_value(text, found))
            .collect(),
    ))
}

pub(super) fn regex_split(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
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
    Ok(CtValue::List(items))
}

pub(super) fn regex_split_limit(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let re = regex_pattern(&args, span)?;
    let text = as_string(
        args.get(1)
            .ok_or_else(|| unsupported("regex.split_limit: missing text argument", span))?,
        span,
    )?;
    let limit = as_int(
        args.get(2)
            .ok_or_else(|| unsupported("regex.split_limit: missing limit argument", span))?,
        span,
    )?;
    Ok(CtValue::List(
        re.split_limit(text, limit)
            .into_iter()
            .map(|item| CtValue::Str(item.to_string()))
            .collect(),
    ))
}

pub(super) fn regex_replace(
    args: Vec<CtValue>,
    span: Span,
    first: bool,
) -> Result<CtValue, Diagnostic> {
    let re = regex_pattern(&args, span)?;
    let rep = as_string(
        args.get(1)
            .ok_or_else(|| unsupported("regex.replace: missing replacement argument", span))?,
        span,
    )?;
    let text = as_string(
        args.get(2)
            .ok_or_else(|| unsupported("regex.replace: missing text argument", span))?,
        span,
    )?;
    Ok(CtValue::Str(if first {
        re.replace_first(text, rep)
    } else {
        re.replace(text, rep)
    }))
}

pub(super) fn regex_match(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let re = regex_pattern(&args, span)?;
    let text = as_string(
        args.get(1)
            .ok_or_else(|| unsupported("regex.match: missing text argument", span))?,
        span,
    )?;
    Ok(match re.match_value(text).ok() {
        Some(found) => CtValue::Present(Box::new(regex_match_value(text, found))),
        None => CtValue::absent(crate::AST::Type::Named("Match".to_string())),
    })
}

pub(super) fn regex_match_value(
    _text: &str,
    found: super::super::super::regex_kernel::JetRegexMatch,
) -> CtValue {
    let groups = (0..=found.group_count())
        .map(|index| match found.group(index as i64) {
            Ok(value) => CtValue::Present(Box::new(CtValue::Str(value))),
            Err(_) => CtValue::absent(crate::AST::Type::String),
        })
        .collect();
    let spans = (0..=found.group_count())
        .map(|index| match (
            found.group_start(index as i64),
            found.group_end(index as i64),
        ) {
            (Ok(start), Ok(end)) => CtValue::Present(Box::new(CtValue::Struct {
                type_name: "__RegexSpan".to_string(),
                fields: vec![
                    ("start".to_string(), CtValue::Int(start)),
                    ("end".to_string(), CtValue::Int(end)),
                ],
            })),
            _ => CtValue::absent(crate::AST::Type::Named("__RegexSpan".to_string())),
        })
        .collect();
    let names = found
        .capture_names()
        .into_iter()
        .map(|name| {
            name.map(|name| CtValue::Present(Box::new(CtValue::Str(name))))
                .unwrap_or_else(|| CtValue::absent(crate::AST::Type::String))
        })
        .collect();
    CtValue::Struct {
        type_name: "Match".to_string(),
        fields: vec![
            ("groups".to_string(), CtValue::List(groups)),
            ("spans".to_string(), CtValue::List(spans)),
            ("names".to_string(), CtValue::List(names)),
        ],
    }
}
