//! D-EACH1=C / D-VERDICT-1320-1 fenced statement expansion.
//!
//! The parser receives ordinary statements: this pass copies one authored
//! statement per fence entry and substitutes each fence in lock-step. A
//! binding-target fence carries plain names (or a numbered-name range); an
//! expression-position fence may also carry expression entries separated by
//! top-level commas. The authored fence facts survive only for formatter
//! emission.

use std::collections::HashSet;

use crate::AST::{FencedNames, FencedStatement};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Lexer::{TokKind, Token};

pub(crate) fn expand(
    toks: &[Token],
) -> Result<(Vec<Token>, Vec<FencedStatement>), Vec<Diagnostic>> {
    let mut out = Vec::with_capacity(toks.len());
    let mut facts = Vec::new();
    let mut diags = Vec::new();
    let mut segment_start = 0usize;
    let mut brace_depth = 0usize;
    let mut nested_brace_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;

    for (index, token) in toks.iter().enumerate() {
        let nested_expression_brace = matches!(token.kind, TokKind::LBrace)
            && (nested_brace_depth > 0
                || paren_depth > 0
                || bracket_depth > 0
                || index
                    .checked_sub(1)
                    .is_some_and(|previous| {
                        matches!(toks[previous].kind, TokKind::LambdaArrow | TokKind::Dot)
                    }));
        let boundary = matches!(token.kind, TokKind::Eof)
            || (matches!(token.kind, TokKind::Semi)
                && nested_brace_depth == 0
                && paren_depth == 0
                && bracket_depth == 0)
            || (matches!(token.kind, TokKind::LBrace) && !nested_expression_brace)
            || (matches!(token.kind, TokKind::RBrace)
                && nested_brace_depth == 0
                && paren_depth == 0
                && bracket_depth == 0);

        if boundary {
            flush_segment(
                &toks[segment_start..index],
                brace_depth,
                &mut out,
                &mut facts,
                &mut diags,
            );
            out.push(token.clone());
            match token.kind {
                TokKind::LBrace => brace_depth += 1,
                TokKind::RBrace => brace_depth = brace_depth.saturating_sub(1),
                _ => {}
            }
            segment_start = index + 1;
            continue;
        }

        match token.kind {
            TokKind::LParen => paren_depth += 1,
            TokKind::RParen => paren_depth = paren_depth.saturating_sub(1),
            TokKind::LBracket => bracket_depth += 1,
            TokKind::RBracket => bracket_depth = bracket_depth.saturating_sub(1),
            TokKind::LBrace => nested_brace_depth += 1,
            TokKind::RBrace => nested_brace_depth = nested_brace_depth.saturating_sub(1),
            _ => {}
        }
    }

    if diags.is_empty() {
        Ok((out, facts))
    } else {
        Err(diags)
    }
}

fn flush_segment(
    segment: &[Token],
    brace_depth: usize,
    out: &mut Vec<Token>,
    facts: &mut Vec<FencedStatement>,
    diags: &mut Vec<Diagnostic>,
) {
    if !segment
        .iter()
        .any(|token| matches!(token.kind, TokKind::FenceOpen | TokKind::FenceClose))
    {
        out.extend_from_slice(segment);
        return;
    }

    match expand_segment(segment, brace_depth) {
        Ok((expanded, fact)) => {
            out.extend(expanded);
            facts.push(fact);
        }
        Err(mut errors) => {
            out.extend_from_slice(segment);
            diags.append(&mut errors);
        }
    }
}

fn expand_segment(
    segment: &[Token],
    brace_depth: usize,
) -> Result<(Vec<Token>, FencedStatement), Vec<Diagnostic>> {
    let mut fences = Vec::new();
    let mut fence_entries: Vec<Vec<Vec<Token>>> = Vec::new();
    let mut pairs = Vec::new();
    let mut index = 0usize;
    let mut diags = Vec::new();

    while index < segment.len() {
        match segment[index].kind {
            TokKind::FenceOpen => {
                let open = index;
                let Some(relative_close) = segment[index + 1..]
                    .iter()
                    .position(|token| matches!(token.kind, TokKind::FenceClose))
                else {
                    diags.push(rejected_position(
                        segment[open].span,
                        &format!(
                            "this fence has no closing `{}`",
                            crate::Syntax::SIGIL_FENCE_CLOSE
                        ),
                    ));
                    break;
                };
                let close = index + 1 + relative_close;
                if segment[index + 1..close]
                    .iter()
                    .any(|token| matches!(token.kind, TokKind::FenceOpen))
                {
                    diags.push(rejected_position(
                        Span::new(segment[open].span.start, segment[close].span.end),
                        "fences cannot nest",
                    ));
                }
                pairs.push((open, close));
                index = close + 1;
            }
            TokKind::FenceClose => {
                diags.push(rejected_position(
                    segment[index].span,
                    &format!(
                        "this `{}` has no opening `{}`",
                        crate::Syntax::SIGIL_FENCE_CLOSE,
                        crate::Syntax::SIGIL_FENCE_OPEN
                    ),
                ));
                index += 1;
            }
            _ => index += 1,
        }
    }

    if pairs.is_empty() {
        return Err(diags);
    }

    let first = pairs[0];
    let binding_target = first.0 == 0
        && first.1 + 1 < segment.len()
        && matches!(
            segment[first.1 + 1].kind,
            TokKind::ColonColon | TokKind::ColonEq
        );
    let expression_statement = brace_depth > 0
        && !segment.iter().any(|token| {
            matches!(
                token.kind,
                TokKind::ColonColon | TokKind::ColonEq | TokKind::Eq
            )
        })
        && !matches!(
            segment.first().map(|token| &token.kind),
            Some(
                TokKind::KwFn
                    | TokKind::KwIf
                    | TokKind::KwElse
                    | TokKind::KwLoop
                    | TokKind::KwWhile
                    | TokKind::KwFor
                    | TokKind::KwSwitch
                    | TokKind::KwStruct
                    | TokKind::KwEnum
                    | TokKind::KwImpl
                    | TokKind::KwTrait
                    | TokKind::KwTag
                    | TokKind::KwEffect
                    | TokKind::KwUse
                    | TokKind::KwModule
                    | TokKind::KwConst
                    | TokKind::KwComptime
                    | TokKind::KwReturn
            )
        );
    if !binding_target && !expression_statement {
        let span = statement_span(segment);
        diags.push(rejected_position(
            span,
            "fences are allowed only in a binding target or an expression statement",
        ));
    }

    for (fence_index, &(open, close)) in pairs.iter().enumerate() {
        let binding_fence = binding_target && fence_index == 0;
        match parse_entries(&segment[open + 1..close], segment[open].span, binding_fence) {
            Ok((mut names, entries)) => {
                names.span = Span::new(segment[open].span.start, segment[close].span.end);
                fences.push(names);
                fence_entries.push(entries);
            }
            Err(diagnostic) => diags.push(diagnostic),
        }
    }

    if let Some(expected) = fences.first().map(|fence| fence.names.len()) {
        for fence in fences.iter().skip(1) {
            if fence.names.len() != expected {
                diags.push(Diagnostic::error(
                    "E0370",
                    "fences on one statement have different entry counts".to_string(),
                    "multiple fences expand in lock-step, so every fence needs one entry for each copy"
                        .to_string(),
                    format!("give every fence {expected} entries"),
                    Some(fence.span),
                ));
            }
        }
    }

    if !diags.is_empty() {
        return Err(diags);
    }

    let copies = fences[0].names.len();
    let mut expanded = Vec::with_capacity(segment.len() * copies);
    for copy in 0..copies {
        if copy > 0 {
            let at = segment.first().map_or(0, |token| token.span.start);
            expanded.push(Token {
                kind: TokKind::Semi,
                span: Span::new(at, at),
            });
        }
        let mut cursor = 0usize;
        for (fence_index, &(open, close)) in pairs.iter().enumerate() {
            expanded.extend_from_slice(&segment[cursor..open]);
            expanded.extend_from_slice(&fence_entries[fence_index][copy]);
            cursor = close + 1;
        }
        expanded.extend_from_slice(&segment[cursor..]);
    }

    Ok((
        expanded,
        FencedStatement {
            span: statement_span(segment),
            fences,
            copies,
        },
    ))
}

/// Parse one fence's content into entries. A binding fence needs plain names
/// (or a numbered-name range). An expression fence also accepts expression
/// entries split on top-level commas; each entry substitutes token-for-token.
fn parse_entries(
    content: &[Token],
    open_span: Span,
    binding_fence: bool,
) -> Result<(FencedNames, Vec<Vec<Token>>), Diagnostic> {
    if content.is_empty() {
        return Err(Diagnostic::error(
            "E0368",
            "this fence is empty".to_string(),
            "a fenced statement needs at least one entry to expand".to_string(),
            format!(
                "write one or more entries between `{}` and `{}`",
                crate::Syntax::SIGIL_FENCE_OPEN,
                crate::Syntax::SIGIL_FENCE_CLOSE
            ),
            Some(open_span),
        ));
    }

    // Numbered-name range: exactly `prefixN..prefixM`. Anything else with a
    // `..` falls through and reads as an ordinary expression entry.
    if content.len() == 3 && matches!(content[1].kind, TokKind::DotDot) {
        if let (TokKind::Ident(start), TokKind::Ident(end)) = (&content[0].kind, &content[2].kind)
        {
            let range_span = Span::new(content[0].span.start, content[2].span.end);
            match expand_numbered_range(start, end, range_span) {
                Ok(names) => {
                    let entries = names
                        .iter()
                        .map(|(name, span)| {
                            vec![Token {
                                kind: TokKind::Ident(name.clone()),
                                span: *span,
                            }]
                        })
                        .collect();
                    let fact = FencedNames {
                        span: open_span,
                        range: Some((
                            names.first().unwrap().0.clone(),
                            names.last().unwrap().0.clone(),
                        )),
                        names,
                    };
                    return Ok((fact, entries));
                }
                Err(diagnostic) if binding_fence => return Err(diagnostic),
                Err(_) => {}
            }
        } else if binding_fence {
            return Err(rejected_entry(
                Span::new(content[0].span.start, content[2].span.end),
                "a fenced name range needs two numbered names",
            ));
        }
    }

    // Split on top-level commas, tracking every bracket family so entry
    // expressions keep their internal commas.
    let mut entries: Vec<Vec<Token>> = Vec::new();
    let mut current: Vec<Token> = Vec::new();
    let mut depth = 0usize;
    for token in content {
        match token.kind {
            TokKind::LParen | TokKind::LBracket | TokKind::LBrace => depth += 1,
            TokKind::RParen | TokKind::RBracket | TokKind::RBrace => {
                depth = depth.saturating_sub(1)
            }
            TokKind::Comma if depth == 0 => {
                if current.is_empty() {
                    return Err(rejected_entry(token.span, "this fence entry is empty"));
                }
                entries.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(token.clone());
    }
    if current.is_empty() {
        return Err(rejected_entry(
            content.last().map_or(open_span, |token| token.span),
            "this fence ends after a comma",
        ));
    }
    entries.push(current);

    let mut names = Vec::new();
    for entry in &entries {
        let span = Span::new(
            entry.first().unwrap().span.start,
            entry.last().unwrap().span.end,
        );
        match (&entry[..], binding_fence) {
            ([Token { kind: TokKind::Ident(name), .. }], _) => {
                names.push((name.clone(), span));
            }
            (_, true) => {
                return Err(rejected_entry(
                    span,
                    "a binding fence needs plain names separated by commas",
                ));
            }
            (_, false) => names.push((String::new(), span)),
        }
    }

    if binding_fence {
        let mut seen = HashSet::new();
        for (name, span) in &names {
            if !seen.insert(name.clone()) {
                return Err(Diagnostic::error(
                    "E0369",
                    format!("`{name}` appears twice in this fence"),
                    "one binding fence must name each generated copy once".to_string(),
                    format!("remove the second `{name}` or give it a different name"),
                    Some(*span),
                ));
            }
        }
    }

    Ok((
        FencedNames {
            span: open_span,
            range: None,
            names,
        },
        entries,
    ))
}

fn expand_numbered_range(
    start: &str,
    end: &str,
    span: Span,
) -> Result<Vec<(String, Span)>, Diagnostic> {
    let Some((start_prefix, start_number, width)) = numbered_name(start) else {
        return Err(rejected_entry(
            span,
            "the first range endpoint needs a trailing number",
        ));
    };
    let Some((end_prefix, end_number, end_width)) = numbered_name(end) else {
        return Err(rejected_entry(
            span,
            "the last range endpoint needs a trailing number",
        ));
    };
    if start_prefix != end_prefix || start_number > end_number {
        return Err(rejected_entry(
            span,
            "a fenced name range needs one prefix and ascending numbers",
        ));
    }
    let width = width.max(end_width);
    Ok((start_number..=end_number)
        .map(|number| {
            (
                format!("{start_prefix}{number:0width$}"),
                span,
            )
        })
        .collect())
}

fn numbered_name(name: &str) -> Option<(&str, usize, usize)> {
    let split = name
        .char_indices()
        .rev()
        .find(|(_, ch)| !ch.is_ascii_digit())
        .map_or(0, |(index, ch)| index + ch.len_utf8());
    if split == name.len() {
        return None;
    }
    let digits = &name[split..];
    Some((&name[..split], digits.parse().ok()?, digits.len()))
}

fn statement_span(segment: &[Token]) -> Span {
    Span::new(
        segment.first().map_or(0, |token| token.span.start),
        segment.last().map_or(0, |token| token.span.end),
    )
}

fn rejected_position(span: Span, what: &str) -> Diagnostic {
    Diagnostic::error(
        "E0371",
        what.to_string(),
        "D-EACH1 expands complete binding or expression statements, not headers, items, or nested syntax"
            .to_string(),
        "move the fence to a binding target or a complete expression statement".to_string(),
        Some(span),
    )
}

/// Entry-shape rejection: the fence sits in a legal position but one of its
/// entries has the wrong shape. Distinct from `rejected_position` so the
/// why/fix teach the entry rules, not statement placement (I4).
fn rejected_entry(span: Span, what: &str) -> Diagnostic {
    Diagnostic::error(
        "E0371",
        what.to_string(),
        "a binding fence takes plain names or one ascending numbered-name range; an expression fence takes comma-separated expressions"
            .to_string(),
        format!(
            "fix this entry, e.g. `{open} a, b {close}` or `{open} t1..t8 {close}`",
            open = crate::Syntax::SIGIL_FENCE_OPEN,
            close = crate::Syntax::SIGIL_FENCE_CLOSE
        ),
        Some(span),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Lexer;

    fn expanded(src: &str) -> (Vec<Token>, Vec<FencedStatement>) {
        let (tokens, lex_diags) = Lexer::lex(src);
        assert!(lex_diags.is_empty(), "{lex_diags:?}");
        expand(&Lexer::without_comments(&tokens)).expect("fence expansion")
    }

    #[test]
    fn lexer_uses_fence_digraphs_and_close_suppresses_a_terminator() {
        let (tokens, diagnostics) = Lexer::lex("$[\n    first,\n    second\n]$");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert!(matches!(tokens[0].kind, TokKind::FenceOpen));
        assert!(tokens
            .iter()
            .any(|token| matches!(token.kind, TokKind::FenceClose)));
        let close = tokens
            .iter()
            .position(|token| matches!(token.kind, TokKind::FenceClose))
            .unwrap();
        assert!(!matches!(tokens[close - 1].kind, TokKind::Semi));
    }

    #[test]
    fn expands_numbered_binding_and_lock_step_reference_fences() {
        let (tokens, facts) = expanded(
            "fn run() {\n$[ t1..t3 ]$ :: work()\n$[ t1..t3 ]$.wait()\nuse_pair($[ t1, t2, t3 ]$, $[ a, b, c ]$)\n}\n",
        );
        let names = tokens
            .iter()
            .filter_map(|token| match &token.kind {
                TokKind::Ident(name) if name.starts_with('t') => Some(name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(names.windows(3).any(|window| window == ["t1", "t2", "t3"]));
        assert_eq!(facts.len(), 3);
        assert_eq!(facts[0].copies, 3);
        assert!(facts[1].fences[0].range.is_some());
        assert_eq!(facts[2].fences.len(), 2);
    }

    #[test]
    fn diagnoses_empty_duplicate_mismatch_and_header_position() {
        for (source, code) in [
            ("fn run() { $[ ]$ :: 1 }", "E0368"),
            ("fn run() { $[ a, a ]$ :: 1 }", "E0369"),
            ("fn run() { $[ f(x), g ]$ :: 1 }", "E0371"),
            ("fn run() { call($[ a, b ]$, $[ c ]$) }", "E0370"),
            ("fn run() { if $[ a, b ]$ { print(a) } }", "E0371"),
        ] {
            let (tokens, lex_diags) = Lexer::lex(source);
            assert!(lex_diags.is_empty(), "{lex_diags:?}");
            let diagnostics = expand(&Lexer::without_comments(&tokens)).unwrap_err();
            assert!(
                diagnostics.iter().any(|diagnostic| diagnostic.code == code),
                "{source}: {diagnostics:?}"
            );
        }
    }

    #[test]
    fn formatter_emits_one_fence_and_is_stable() {
        let source = "fn run() {\n    $[ first, second ]$ :: work()\n    show($[ first, second ]$)\n}\n";
        let (tokens, diagnostics) = Lexer::lex(source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let comments = Lexer::comments(&tokens);
        let program = crate::Parser::parse_for_fmt(&tokens).expect("parse fenced source");
        let formatted = crate::Formatter::format_program(&program, source, &comments);
        assert_eq!(formatted.matches("$[ first, second ]$").count(), 2);
        let (tokens, diagnostics) = Lexer::lex(&formatted);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let comments = Lexer::comments(&tokens);
        let program = crate::Parser::parse_for_fmt(&tokens).expect("reparse formatted fence");
        assert_eq!(
            crate::Formatter::format_program(&program, &formatted, &comments),
            formatted
        );
    }

    #[test]
    fn formatter_preserves_whitespace_inside_fenced_string_literals() {
        let source =
            "fn run() {\n    show($[ first, second ]$, \"keep  two   spaces\")\n}\n";
        let (tokens, diagnostics) = Lexer::lex(source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let comments = Lexer::comments(&tokens);
        let program = crate::Parser::parse_for_fmt(&tokens).expect("parse fenced string");
        let formatted = crate::Formatter::format_program(&program, source, &comments);
        assert!(formatted.contains("\"keep  two   spaces\""));
    }

    #[test]
    fn formatter_keeps_fenced_lambda_body_after_an_inline_comment() {
        let source = "\
fn run() {
    $[ first, second ]$ :: work(() => { // keep this note
        print(\"body\")
        return 1
    })
}
";
        let (tokens, diagnostics) = Lexer::lex(source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let comments = Lexer::comments(&tokens);
        let program = crate::Parser::parse_for_fmt(&tokens).expect("parse fenced lambda");
        let formatted = crate::Formatter::format_program(&program, source, &comments);
        assert!(
            formatted.contains("// keep this note\n"),
            "inline comment swallowed the fenced lambda body:\n{formatted}"
        );
        let (tokens, diagnostics) = Lexer::lex(&formatted);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let comments = Lexer::comments(&tokens);
        let program =
            crate::Parser::parse_for_fmt(&tokens).expect("reparse formatted fenced lambda");
        assert_eq!(
            crate::Formatter::format_program(&program, &formatted, &comments),
            formatted
        );
    }

    #[test]
    fn formatter_wraps_a_wide_fence_one_name_per_line() {
        let source = "fn run() {\n    $[ this_name_is_deliberately_long_one, this_name_is_deliberately_long_two, this_name_is_deliberately_long_three ]$ :: work()\n}\n";
        let (tokens, diagnostics) = Lexer::lex(source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let comments = Lexer::comments(&tokens);
        let program = crate::Parser::parse_for_fmt(&tokens).expect("parse wide fence");
        let formatted = crate::Formatter::format_program(&program, source, &comments);
        assert!(formatted.contains("$[\n"));
        assert!(formatted.contains("this_name_is_deliberately_long_one,\n"));
        assert!(formatted.contains("\n    ]$ :: work()"));
    }

    #[test]
    fn numbered_range_expands_in_binding_and_receiver_expression() {
        let source =
            "fn run() {\n    $[ task1..task3 ]$ :: spawn()\n    $[ task1..task3 ]$.wait()\n}\n";
        let (tokens, diagnostics) = Lexer::lex(source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let program = crate::Parser::parse(&tokens).expect("parse range fences");
        let run = program
            .items
            .iter()
            .find_map(|item| match item {
                crate::AST::Item::Func(function) if function.name == "run" => Some(function),
                _ => None,
            })
            .expect("run function");
        assert_eq!(run.body.len(), 6);
    }

    #[test]
    fn expression_entries_expand_an_expression_statement() {
        let (tokens, facts) = expanded(
            "fn run() {\n    print($[ \"a={x}\", \"b\", total(1, 2) ]$)\n}\n",
        );
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].copies, 3);
        let prints = tokens
            .iter()
            .filter(|token| matches!(&token.kind, TokKind::Ident(name) if name == "print"))
            .count();
        assert_eq!(prints, 3);
        // The third copy substitutes the whole call, commas included.
        let totals = tokens
            .iter()
            .filter(|token| matches!(&token.kind, TokKind::Ident(name) if name == "total"))
            .count();
        assert_eq!(totals, 1);
    }

    #[test]
    fn nested_lambda_and_struct_braces_stay_inside_fenced_statements() {
        let source = "fn run() {\n    $[ first, second ]$ :: () => {\n        print(\"nested\")\n    }\n    show(Thing.{ value: $[ first, second ]$ })\n}\n";
        let (tokens, diagnostics) = Lexer::lex(source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let program = crate::Parser::parse(&tokens).expect("parse nested fenced statements");
        assert_eq!(program.fenced_statements.len(), 2);
        let run = program
            .items
            .iter()
            .find_map(|item| match item {
                crate::AST::Item::Func(function) if function.name == "run" => Some(function),
                _ => None,
            })
            .expect("run function");
        assert_eq!(run.body.len(), 4);
    }
}
