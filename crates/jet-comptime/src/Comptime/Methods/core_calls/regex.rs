//! `jet.regex` evaluator, kept separate from the Core call registry.

use crate::AST::CtValue;
use crate::Diagnostics::{Diagnostic, Span};

use super::as_string;
use super::super::super::Builtins::as_int;
use super::super::super::Diagnostics::unsupported;

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
        "is_match" => regex_is_match(call_args, span),
        "find" => regex_find(call_args, span),
        "find_all" => regex_find_all(call_args, span),
        "matches" => regex_matches(call_args, span),
        "split" => regex_split(call_args, span),
        "split_limit" => regex_split_limit(call_args, span),
        "replace" => regex_replace(call_args, span, false),
        "replace_all" => regex_replace(call_args, span, true),
        "match" => regex_match(call_args, span),
        _ => return None,
    })
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
            args.first()
                .ok_or_else(|| unsupported("regex.replace_all_with: missing text argument", span))?,
            span,
        )?;
        let callback = args.get(1).ok_or_else(|| {
            unsupported("regex.replace_all_with: missing callback argument", span)
        })?;
        let replaced = re.replace_all_with(text, |found| {
            let value = invoke(callback.clone(), vec![regex_match_value(text, found)])?;
            Ok::<String, Diagnostic>(as_string(&value, span)?.to_string())
        })?;
        Ok(CtValue::Str(replaced))
    })())
}

pub(super) fn regex_pattern(
    args: &[CtValue],
    span: Span,
) -> Result<super::super::super::RegexLite::RegexLite, Diagnostic> {
    let value = args
        .first()
        .ok_or_else(|| unsupported("regex call: missing pattern argument", span))?;
    let pat = match value {
        CtValue::Str(pattern) => pattern.as_str(),
        CtValue::Struct { type_name, fields } if type_name == "__JetRegex" => fields
            .iter()
            .find_map(|(name, value)| match (name.as_str(), value) {
                ("pattern", CtValue::Str(pattern)) => Some(pattern.as_str()),
                _ => None,
            })
            .ok_or_else(|| unsupported("Regex literal value", span))?,
        _ => return Err(unsupported("Regex pattern value", span)),
    };
    super::super::super::RegexLite::RegexLite::parse(pat).map_err(|e| {
        Diagnostic::error(
            "E0956",
            format!("bad regex pattern: {}", e),
            "the pattern could not be compiled".to_string(),
            "fix the pattern syntax".to_string(),
            Some(span),
        )
    })
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

pub(super) fn regex_find(args: Vec<CtValue>, span: Span) -> Result<CtValue, Diagnostic> {
    let re = regex_pattern(&args, span)?;
    let text = as_string(
        args.get(1)
            .ok_or_else(|| unsupported("regex.find: missing text argument", span))?,
        span,
    )?;
    Ok(match re.find(text) {
        Some(m) => CtValue::Present(Box::new(CtValue::Str(text[m.start..m.end].to_string()))),
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
    all: bool,
) -> Result<CtValue, Diagnostic> {
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
    Ok(CtValue::Str(if all {
        re.replace_all(text, rep)
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
    Ok(match re.find(text) {
        Some(found) => CtValue::Present(Box::new(regex_match_value(text, found))),
        None => CtValue::absent(crate::AST::Type::Named("Match".to_string())),
    })
}

pub(super) fn regex_match_value(
    text: &str,
    found: super::super::super::RegexLite::MatchLite,
) -> CtValue {
    let groups = found
        .groups
        .iter()
        .map(|item| {
            item.map(|(start, end)| {
                CtValue::Present(Box::new(CtValue::Str(text[start..end].to_string())))
            })
            .unwrap_or_else(|| CtValue::absent(crate::AST::Type::String))
        })
        .collect();
    let spans = found
        .groups
        .iter()
        .map(|item| {
            item.map(|(start, end)| {
                CtValue::Present(Box::new(CtValue::Struct {
                    type_name: "__RegexSpan".to_string(),
                    fields: vec![
                        ("start".to_string(), CtValue::Int(start as i64)),
                        ("end".to_string(), CtValue::Int(end as i64)),
                    ],
                }))
            })
            .unwrap_or_else(|| {
                CtValue::absent(crate::AST::Type::Named("__RegexSpan".to_string()))
            })
        })
        .collect();
    let names = found
        .names
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
