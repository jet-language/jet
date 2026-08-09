use std::fs;
use std::path::PathBuf;

use jet::Lexer::{StrTokPart, TokKind, Token};

const UI_PARSE_INVALID: &[&str] = &[
    "tests/ui/E0927_allow_marker_legal.jet",
    "tests/ui/E0927_retired_marker.jet",
    "tests/ui/E0927_retired_pure_callable.jet",
    "tests/ui/E0927_retired_pure_trait_oneline.jet",
    "tests/ui/E0927_retired_pure_trait_twoline.jet",
    "tests/ui/E0927_unknown_marker_contract.jet",
    "tests/ui/E0927_unknown_marker_directive.jet",
    "tests/ui/E0927_unknown_marker_function.jet",
    "tests/ui/E0927_unknown_marker_typo.jet",
    "tests/ui/E2714_derive_old_for.jet",
    "tests/ui/assign_in_condition.jet",
    "tests/ui/auto_derive_invalid_sign.jet",
    "tests/ui/bad_escape.jet",
    "tests/ui/binpat_bad_width.jet",
    "tests/ui/binpat_multibyte_needs_endian.jet",
    "tests/ui/binpat_rest_not_final.jet",
    "tests/ui/binpat_take_pattern_bad_width.jet",
    "tests/ui/cffi_e3206_reserved_segment.jet",
    "tests/ui/cffi_e3207_bindgen_outside_cache.jet",
    "tests/ui/cffi_retired_at_extern.jet",
    "tests/ui/cffi_retired_hash_extern.jet",
    "tests/ui/chained_comparison_mixed_direction.jet",
    "tests/ui/compiler_fact_unknown_member.jet",
    "tests/ui/comptime_keyword_retired.jet",
    "tests/ui/comptime_known_block_retired.jet",
    "tests/ui/comptime_known_retired.jet",
    "tests/ui/const_retired.jet",
    "tests/ui/context_eq_rejected.jet",
    "tests/ui/context_unknown_field.jet",
    "tests/ui/continue_teaches_next.jet",
    "tests/ui/control_body_needs_braces.jet",
    "tests/ui/copy_keyword_retired_e0991.jet",
    "tests/ui/core_selective_import.jet",
    "tests/ui/debug_unknown_selector.jet",
    "tests/ui/defer_only_close.jet",
    "tests/ui/deref_forbidden.jet",
    "tests/ui/dispatch_missing_eq.jet",
    "tests/ui/dispatch_pattern_needs_eq.jet",
    "tests/ui/dispatch_redundant_subject.jet",
    "tests/ui/dotless_struct_e0320.jet",
    "tests/ui/dunder_marker_not_generated.jet",
    "tests/ui/dunder_reserved.jet",
    "tests/ui/effect_arrow_retired.jet",
    "tests/ui/empty_map_colon_retired.jet",
    "tests/ui/enum_group_payload.jet",
    "tests/ui/enum_multi_positional_payload.jet",
    "tests/ui/enum_pattern_needs_dot.jet",
    "tests/ui/enum_pattern_needs_dot_or.jet",
    "tests/ui/enum_pattern_needs_dot_payload.jet",
    "tests/ui/external_method_retired_separator.jet",
    "tests/ui/fenced_name_bad_position.jet",
    "tests/ui/fenced_name_binding_nonname.jet",
    "tests/ui/fenced_name_duplicate.jet",
    "tests/ui/fenced_name_empty.jet",
    "tests/ui/fenced_name_mismatched_counts.jet",
    "tests/ui/ffi_body_not_string.jet",
    "tests/ui/fixed_interpolation_malformed_precision.jet",
    "tests/ui/fixed_interpolation_missing_precision.jet",
    "tests/ui/flow_pipe_unassigned.jet",
    "tests/ui/fn_type_zone_misplaced.jet",
    "tests/ui/generated_cffi_e3206.jet",
    "tests/ui/generated_cffi_e3207.jet",
    "tests/ui/generic_square_brackets.jet",
    "tests/ui/if_expr_branch_type_mismatch.jet",
    "tests/ui/if_expr_missing_else.jet",
    "tests/ui/impl_colon_separator.jet",
    "tests/ui/int_too_big.jet",
    "tests/ui/interp_debug_label_empty.jet",
    "tests/ui/interp_empty.jet",
    "tests/ui/interp_unclosed.jet",
    "tests/ui/label_not_on_loop.jet",
    "tests/ui/layout_columnar_partial.jet",
    "tests/ui/layout_columnar_prefix_reserved.jet",
    "tests/ui/layout_keyword_retired.jet",
    "tests/ui/layout_unknown_variant.jet",
    "tests/ui/layout_unsupported_variant.jet",
    "tests/ui/loop_counter_form_retired.jet",
    "tests/ui/loop_header_semicolon_retired.jet",
    "tests/ui/loop_label_prefix_old_form.jet",
    "tests/ui/marker_argument_shape.jet",
    "tests/ui/marker_decl_rejected_forms.jet",
    "tests/ui/marker_empty_arguments.jet",
    "tests/ui/marker_experimental_at.jet",
    "tests/ui/marker_experimental_hash.jet",
    "tests/ui/marker_hardened_at.jet",
    "tests/ui/marker_hardened_hash.jet",
    "tests/ui/marker_repeated_on_one_target.jet",
    "tests/ui/marker_retired_tag.jet",
    "tests/ui/marker_retired_task.jet",
    "tests/ui/marker_tested_at.jet",
    "tests/ui/marker_tested_hash.jet",
    "tests/ui/marker_wrong_at_plane.jet",
    "tests/ui/member_spread_not_ident.jet",
    "tests/ui/meta_bad_maturity.jet",
    "tests/ui/meta_on_expression.jet",
    "tests/ui/meta_unknown_field.jet",
    "tests/ui/migration_unknown_op.jet",
    "tests/ui/module_unknown_namespace.jet",
    "tests/ui/module_wildcard.jet",
    "tests/ui/namespace_foreign_kw.jet",
    "tests/ui/nested_option.jet",
    "tests/ui/no_prelude_duplicate.jet",
    "tests/ui/off_debug_attr_on_expression.jet",
    "tests/ui/off_debug_attr_on_item.jet",
    "tests/ui/off_debug_doubled_attr.jet",
    "tests/ui/operator_foreign_guess.jet",
    "tests/ui/opt_chain_method.jet",
    "tests/ui/param_label_duplicate.jet",
    "tests/ui/param_zone_empty.jet",
    "tests/ui/param_zone_empty_positional.jet",
    "tests/ui/param_zone_misplaced.jet",
    "tests/ui/param_zone_repeated.jet",
    "tests/ui/params_not_yet.jet",
    "tests/ui/parse_pattern_adjacent_holes.jet",
    "tests/ui/perf_budget_unknown_role_field.jet",
    "tests/ui/persist_not_module_level.jet",
    "tests/ui/policy_conflicting_module.jet",
    "tests/ui/policy_site_bound_authority.jet",
    "tests/ui/positional_tuple.jet",
    "tests/ui/project_module_invalid/broken.jet",
    "tests/ui/protocol_bad_endpoint.jet",
    "tests/ui/pub_file_duplicate_marker.jet",
    "tests/ui/pub_file_priv_without_marker.jet",
    "tests/ui/pub_file_private_teaching.jet",
    "tests/ui/pub_file_pub_priv_conflict.jet",
    "tests/ui/pub_file_publicfile_teaching.jet",
    "tests/ui/pub_file_redundant_pub.jet",
    "tests/ui/pub_file_section_label.jet",
    "tests/ui/pub_package_bad_qualifier.jet",
    "tests/ui/qq_block_fallback.jet",
    "tests/ui/quantity_unknown_kind.jet",
    "tests/ui/range_arm_dot_dot_eq.jet",
    "tests/ui/range_arm_step.jet",
    "tests/ui/range_constraint_value_rejected.jet",
    "tests/ui/range_type_empty_range.jet",
    "tests/ui/repl_effect_denied_e1803.jet",
    "tests/ui/result_old_syntax.jet",
    "tests/ui/retired_bare_sanitizer.jet",
    "tests/ui/retired_cli_marker.jet",
    "tests/ui/retired_void_result.jet",
    "tests/ui/return_arrow_split.jet",
    "tests/ui/root_param_shape.jet",
    "tests/ui/schedule_every_without_task.jet",
    "tests/ui/schedule_task_on_method.jet",
    "tests/ui/shield_arguments.jet",
    "tests/ui/single_bracket_marker.jet",
    "tests/ui/stacked_type_markers.jet",
    "tests/ui/string_lone_brace.jet",
    "tests/ui/subjectless_guard_direct_nesting.jet",
    "tests/ui/subjectless_guard_value_missing_else.jet",
    "tests/ui/suppress_retired.jet",
    "tests/ui/tag_missing_policy.jet",
    "tests/ui/tag_with_method.jet",
    "tests/ui/take_pattern_bad_hole.jet",
    "tests/ui/take_pattern_computed_arg.jet",
    "tests/ui/take_pattern_string_typed_bad_hole.jet",
    "tests/ui/test_block_nested.jet",
    "tests/ui/trailing_block_double.jet",
    "tests/ui/trailing_block_not_function.jet",
    "tests/ui/trailing_block_on_index.jet",
    "tests/ui/tuple_numeric_field.jet",
    "tests/ui/tuple_single_field.jet",
    "tests/ui/two_capability_markers.jet",
    "tests/ui/two_parse_errors.jet",
    "tests/ui/typed_binding_retired.jet",
    "tests/ui/uninit_annotated_retired.jet",
    "tests/ui/uninit_marker_retired.jet",
    "tests/ui/uninit_no_type.jet",
    "tests/ui/unit_family_bad_denominator.jet",
    "tests/ui/unit_family_base_metadata.jet",
    "tests/ui/unit_family_derived_requires_base.jet",
    "tests/ui/unit_family_duplicate_metadata.jet",
    "tests/ui/unit_family_float_metadata.jet",
    "tests/ui/unit_family_missing_base.jet",
    "tests/ui/unit_family_unknown_header_field.jet",
    "tests/ui/unit_family_unknown_metadata.jet",
    "tests/ui/unit_family_zero_denominator.jet",
    "tests/ui/unit_family_zero_scale.jet",
    "tests/ui/unit_format_unknown_style.jet",
    "tests/ui/unknown_char.jet",
    "tests/ui/unsafe_extern_rust_teaching.jet",
    "tests/ui/unsafe_fn_missing_reason.jet",
    "tests/ui/unsafe_missing_reason.jet",
    "tests/ui/unterminated_block_comment.jet",
    "tests/ui/unterminated_string.jet",
    "tests/ui/unterminated_triple_string.jet",
    "tests/ui/value_dispatch_missing_else.jet",
    "tests/ui/variadic_not_last.jet",
    "tests/ui/web_target_web_on_module.jet",
    "tests/ui/yielding_loop_missing_item.jet",
    "tests/ui/yielding_loop_nonfinite.jet",
];

fn collect_jet_files_recursive(dir: &PathBuf) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_matching_jet_files(dir, &mut out);
    out.sort();
    out
}

fn collect_matching_jet_files(dir: &PathBuf, out: &mut Vec<PathBuf>) {
    if dir.file_name().and_then(|name| name.to_str()) == Some(".jet") {
        return; // generated bindings/cache, not checked source corpus
    }
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_matching_jet_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some(jet::Syntax::FILE_EXT) {
            out.push(path);
        }
    }
}

// The loss oracle is ordered and exact after these parser-equivalent rewrites:
// top-level formatter ordering; marker-list grouping; `Type<T>.method()`;
// external-method and task-block sugar; canonical anonymous-union member order;
// optional declaration/trailing commas; redundant default file aliases. The
// comparator itself permits only formatter-added arm blocks, bare-lambda parens,
// leading-dot variant patterns, struct shorthand labels/separators, and a
// required default alias on a dotted module import. No rule can reorder or
// discard an expression operand, operator, marker payload, comment, or string
// interpolation.
fn canonical_tokens(src: &str, path: &std::path::Path) -> Vec<Token> {
    let (tokens, diagnostics) = jet::Lexer::lex(src);
    assert!(
        diagnostics.is_empty(),
        "lex failed on {}:\n{}",
        path.display(),
        jet::render_diagnostics(&path.display().to_string(), src, &diagnostics)
    );
    let tokens = canonicalize_task_blocks(canonicalize_external_methods(
        canonicalize_static_generic_calls(expand_marker_groups(canonicalize_file_prefix(tokens))),
    ));
    let enum_group_commas = enum_group_commas(&tokens);
    let declaration_field_commas = declaration_field_commas(&tokens);
    let tokens: Vec<_> = tokens
        .into_iter()
        .enumerate()
        .filter(|(index, _)| {
            !enum_group_commas[*index] && !declaration_field_commas[*index]
        })
        .map(|(_, token)| token)
        .filter(|token| !matches!(token.kind, TokKind::Semi | TokKind::Eof))
        .collect();
    let mut canonical: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut index = 0;
    while index < tokens.len() {
        if matches!(tokens[index].kind, TokKind::Comma)
            && tokens.get(index + 1).is_some_and(|next| {
                matches!(
                    next.kind,
                    TokKind::RParen | TokKind::RBracket | TokKind::RBrace
                )
            })
        {
            index += 1;
            continue;
        }
        // D-FAIL-ERROR1=A: the formatter omits an explicit default `Err` in a
        // fallible function return type (`T ? Err` -> `T ?`). Keep that
        // canonical spelling rewrite out of the loss comparison without
        // matching ordinary `Err(...)` constructor expressions.
        if matches!(&tokens[index].kind, TokKind::Ident(name) if name == jet::Syntax::TYPE_ERR)
            && matches!(canonical.last().map(|token| &token.kind), Some(TokKind::Question))
            && matches!(
                tokens.get(index + 1).map(|token| &token.kind),
                Some(TokKind::LBrace | TokKind::Eq)
            )
        {
            index += 1;
            continue;
        }
        // `use "./foo" as foo` and `use "./foo"` parse identically. The
        // formatter removes that redundant default alias, so erase it from the
        // comparison stream as a canonical spelling rewrite, not token loss.
        if redundant_default_file_alias(&canonical, &tokens, index) {
            index += 2;
            continue;
        }
        canonical.push(tokens[index].clone());
        index += 1;
    }
    canonical
}

fn canonicalize_file_prefix(tokens: Vec<Token>) -> Vec<Token> {
    let mut chunks: Vec<Vec<Token>> = Vec::new();
    let mut chunk = Vec::new();
    let mut brace_depth = 0usize;
    for token in tokens {
        match token.kind {
            TokKind::LBrace => brace_depth += 1,
            TokKind::RBrace => brace_depth -= 1,
            _ => {}
        }
        let end = matches!(token.kind, TokKind::Eof)
            || (brace_depth == 0 && matches!(token.kind, TokKind::Semi));
        chunk.push(token);
        if end {
            chunks.push(std::mem::take(&mut chunk));
        }
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }

    let mut groups: Vec<(usize, Vec<Token>)> = Vec::new();
    let mut leading_comments = Vec::new();
    for chunk in chunks {
        if chunk.iter().all(|token| {
            matches!(
                token.kind,
                TokKind::LineComment(_) | TokKind::BlockComment(_) | TokKind::Semi | TokKind::Eof
            )
        }) {
            leading_comments.extend(chunk);
            continue;
        }
        leading_comments.extend(chunk);
        let category = file_chunk_category(&leading_comments);
        groups.push((category, std::mem::take(&mut leading_comments)));
    }
    if !leading_comments.is_empty() {
        groups.push((3, leading_comments));
    }
    for directive_i in 0..groups.len() {
        if !is_spanless_file_marker_group(&groups[directive_i].1) {
            continue;
        }
        let mut displaced = Vec::new();
        while matches!(
            groups[directive_i].1.first().map(|token| &token.kind),
            Some(TokKind::LineComment(_) | TokKind::BlockComment(_) | TokKind::Semi)
        ) {
            displaced.push(groups[directive_i].1.remove(0));
        }
        if let Some((_, item_tokens)) = groups
            .iter_mut()
            .skip(directive_i + 1)
            .find(|(category, _)| *category == 3)
        {
            displaced.append(item_tokens);
            *item_tokens = displaced;
        }
    }
    for import_i in 0..groups.len() {
        if groups[import_i].0 != 1 {
            continue;
        }
        let mut hoisted_comments = Vec::new();
        for (_, tokens) in groups.iter_mut().take(import_i) {
            hoisted_comments.extend(drain_top_level_comments(tokens));
        }
        if !hoisted_comments.is_empty() {
            hoisted_comments.append(&mut groups[import_i].1);
            groups[import_i].1 = hoisted_comments;
        }
    }
    groups.retain(|(_, tokens)| !tokens.is_empty());
    groups.sort_by_key(|(category, _)| *category);
    groups.into_iter().flat_map(|(_, tokens)| tokens).collect()
}

fn is_spanless_file_marker_group(tokens: &[Token]) -> bool {
    let kinds: Vec<_> = tokens
        .iter()
        .filter(|token| {
            !matches!(
                token.kind,
                TokKind::LineComment(_) | TokKind::BlockComment(_) | TokKind::Semi | TokKind::Eof
            )
        })
        .map(|token| &token.kind)
        .collect();
    matches!(
        kinds.as_slice(),
        [TokKind::Hash, TokKind::Ident(name), ..]
            if matches!(name.as_str(), jet::Syntax::MARKER_TARGET | jet::Syntax::MARKER_HTML)
    )
}

fn drain_top_level_comments(tokens: &mut Vec<Token>) -> Vec<Token> {
    let mut comments = Vec::new();
    let mut retained = Vec::with_capacity(tokens.len());
    let mut brace_depth = 0usize;
    for token in tokens.drain(..) {
        match token.kind {
            TokKind::LBrace => brace_depth += 1,
            TokKind::RBrace => brace_depth -= 1,
            TokKind::LineComment(_) | TokKind::BlockComment(_) if brace_depth == 0 => {
                comments.push(token);
                continue;
            }
            _ => {}
        }
        retained.push(token);
    }
    *tokens = retained;
    comments
}

fn file_chunk_category(chunk: &[Token]) -> usize {
    let kinds: Vec<_> = chunk
        .iter()
        .filter(|token| {
            !matches!(
                token.kind,
                TokKind::LineComment(_) | TokKind::BlockComment(_) | TokKind::Semi | TokKind::Eof
            )
        })
        .map(|token| &token.kind)
        .collect();
    match kinds.as_slice() {
        [TokKind::KwUse, ..] => 1,
        [TokKind::KwPub, rest @ ..] if rest.iter().any(|kind| matches!(kind, TokKind::KwUse)) => {
            1
        }
        [TokKind::Hash, TokKind::Ident(name), ..]
            if matches!(
                name.as_str(),
                jet::Syntax::MARKER_PUB_FILE | jet::Syntax::MARKER_NO_PRELUDE
            ) =>
        {
            0
        }
        [TokKind::Hash, TokKind::Ident(name), ..]
            if matches!(name.as_str(), jet::Syntax::MARKER_TARGET | jet::Syntax::MARKER_HTML) =>
        {
            2
        }
        [TokKind::Hash, TokKind::Ident(policy), TokKind::LParen, TokKind::Ident(no_alloc), ..]
            if policy == jet::Syntax::MARKER_POLICY && no_alloc == jet::Syntax::POLICY_NO_ALLOC =>
        {
            2
        }
        _ => 3,
    }
}

fn expand_marker_groups(tokens: Vec<Token>) -> Vec<Token> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut index = 0;
    while index < tokens.len() {
        if matches!(tokens[index].kind, TokKind::Hash)
            && matches!(tokens.get(index + 1).map(|t| &t.kind), Some(TokKind::LBracket))
        {
            if let Some(end) = matching_bracket(&tokens, index + 1) {
                let mut group_start = index + 2;
                let mut paren_depth = 0usize;
                let mut brace_depth = 0usize;
                let mut bracket_depth = 0usize;
                for cursor in index + 2..=end {
                    let split = cursor == end
                        || (matches!(tokens[cursor].kind, TokKind::Comma)
                            && paren_depth == 0
                            && brace_depth == 0
                            && bracket_depth == 0);
                    if split {
                        out.push(tokens[index].clone());
                        out.extend(tokens[group_start..cursor].iter().cloned());
                        group_start = cursor + 1;
                        continue;
                    }
                    match tokens[cursor].kind {
                        TokKind::LParen => paren_depth += 1,
                        TokKind::RParen => paren_depth -= 1,
                        TokKind::LBrace => brace_depth += 1,
                        TokKind::RBrace => brace_depth -= 1,
                        TokKind::LBracket => bracket_depth += 1,
                        TokKind::RBracket => bracket_depth -= 1,
                        _ => {}
                    }
                }
                index = end + 1;
                continue;
            }
        }
        out.push(tokens[index].clone());
        index += 1;
    }
    out
}

fn matching_bracket(tokens: &[Token], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        match token.kind {
            TokKind::LBracket => depth += 1,
            TokKind::RBracket => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn canonicalize_static_generic_calls(tokens: Vec<Token>) -> Vec<Token> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut index = 0;
    while index < tokens.len() {
        if matches!(tokens[index].kind, TokKind::Ident(_))
            && matches!(tokens.get(index + 1).map(|t| &t.kind), Some(TokKind::Lt))
        {
            let mut depth = 0usize;
            let mut close = None;
            for cursor in index + 1..tokens.len() {
                match tokens[cursor].kind {
                    TokKind::Lt => depth += 1,
                    TokKind::Gt => {
                        depth -= 1;
                        if depth == 0 {
                            close = Some(cursor);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(close) = close {
                if matches!(tokens.get(close + 1).map(|t| &t.kind), Some(TokKind::Dot))
                    && matches!(tokens.get(close + 2).map(|t| &t.kind), Some(TokKind::Ident(_)))
                    && matches!(tokens.get(close + 3).map(|t| &t.kind), Some(TokKind::LParen))
                {
                    out.push(tokens[index].clone());
                    out.push(tokens[close + 1].clone());
                    out.push(tokens[close + 2].clone());
                    out.extend(tokens[index + 1..=close].iter().cloned());
                    index = close + 3;
                    continue;
                }
            }
        }
        out.push(tokens[index].clone());
        index += 1;
    }
    out
}

fn canonicalize_external_methods(tokens: Vec<Token>) -> Vec<Token> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut index = 0;
    while index < tokens.len() {
        if matches!(tokens[index].kind, TokKind::KwFn)
            && matches!(tokens.get(index + 1).map(|t| &t.kind), Some(TokKind::Ident(_)))
            && matches!(tokens.get(index + 2).map(|t| &t.kind), Some(TokKind::Dot))
            && matches!(tokens.get(index + 3).map(|t| &t.kind), Some(TokKind::Ident(_)))
            && matches!(tokens.get(index + 4).map(|t| &t.kind), Some(TokKind::LParen))
        {
            if let Some(body_open) = tokens[index + 4..]
                .iter()
                .position(|token| matches!(token.kind, TokKind::LBrace))
                .map(|offset| index + 4 + offset)
            {
                if let Some(body_close) = matching_brace(&tokens, body_open) {
                    out.push(Token {
                        kind: TokKind::KwImpl,
                        span: tokens[index].span,
                    });
                    out.push(tokens[index + 1].clone());
                    out.push(Token {
                        kind: TokKind::LBrace,
                        span: tokens[index].span,
                    });
                    out.push(tokens[index].clone());
                    out.push(tokens[index + 3].clone());
                    out.extend(tokens[index + 4..=body_close].iter().cloned());
                    out.push(Token {
                        kind: TokKind::RBrace,
                        span: tokens[body_close].span,
                    });
                    index = body_close + 1;
                    continue;
                }
            }
        }
        out.push(tokens[index].clone());
        index += 1;
    }
    out
}

fn matching_brace(tokens: &[Token], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        match token.kind {
            TokKind::LBrace => depth += 1,
            TokKind::RBrace => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn canonicalize_task_blocks(tokens: Vec<Token>) -> Vec<Token> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut index = 0;
    while index < tokens.len() {
        if matches!(tokens[index].kind, TokKind::Ident(_))
            && matches!(tokens.get(index + 1).map(|t| &t.kind), Some(TokKind::Dot))
            && matches!(
                tokens.get(index + 2).map(|t| &t.kind),
                Some(TokKind::Ident(name)) if name == jet::Syntax::TASKGROUP_SPAWN_METHOD
            )
            && matches!(tokens.get(index + 3).map(|t| &t.kind), Some(TokKind::LBrace))
        {
            if let Some(close) = matching_brace(&tokens, index + 3) {
                out.extend(tokens[index..index + 3].iter().cloned());
                for kind in [
                    TokKind::LParen,
                    TokKind::LParen,
                    TokKind::RParen,
                    TokKind::LambdaArrow,
                ] {
                    out.push(Token {
                        kind,
                        span: tokens[index + 3].span,
                    });
                }
                out.extend(tokens[index + 3..=close].iter().cloned());
                out.push(Token {
                    kind: TokKind::RParen,
                    span: tokens[close].span,
                });
                index = close + 1;
                continue;
            }
        }
        out.push(tokens[index].clone());
        index += 1;
    }
    out
}

fn enum_group_commas(tokens: &[Token]) -> Vec<bool> {
    let mut ignored = vec![false; tokens.len()];
    let mut enum_base_depth = None;
    let mut awaiting_enum_body = false;
    let mut brace_stack: Vec<usize> = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            TokKind::KwEnum => awaiting_enum_body = true,
            TokKind::LBrace => {
                brace_stack.push(index);
                if awaiting_enum_body {
                    enum_base_depth = Some(brace_stack.len());
                    awaiting_enum_body = false;
                }
            }
            TokKind::RBrace => {
                if enum_base_depth == Some(brace_stack.len()) {
                    enum_base_depth = None;
                }
                brace_stack.pop();
            }
            TokKind::Comma
                if enum_base_depth.is_some_and(|base| brace_stack.len() > base)
                    && matches!(tokens.get(index.wrapping_sub(1)).map(|t| &t.kind), Some(TokKind::Ident(_)))
                    && matches!(tokens.get(index + 1).map(|t| &t.kind), Some(TokKind::Ident(_))) =>
            {
                let group_open = *brace_stack.last().unwrap();
                let group_has_field_colon = tokens[group_open + 1..index]
                    .iter()
                    .any(|token| matches!(token.kind, TokKind::Colon));
                if !group_has_field_colon {
                    ignored[index] = true;
                }
            }
            _ => {}
        }
    }
    ignored
}

fn declaration_field_commas(tokens: &[Token]) -> Vec<bool> {
    let mut ignored = vec![false; tokens.len()];
    let mut awaiting_struct_body = false;
    let mut struct_depth = None;
    let mut brace_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            TokKind::KwStruct => awaiting_struct_body = true,
            TokKind::LBrace => {
                brace_depth += 1;
                if awaiting_struct_body {
                    struct_depth = Some(brace_depth);
                    awaiting_struct_body = false;
                }
            }
            TokKind::RBrace => {
                if struct_depth == Some(brace_depth) {
                    struct_depth = None;
                }
                brace_depth -= 1;
            }
            TokKind::LParen => paren_depth += 1,
            TokKind::RParen => paren_depth -= 1,
            TokKind::LBracket => bracket_depth += 1,
            TokKind::RBracket => bracket_depth -= 1,
            TokKind::Comma
                if struct_depth == Some(brace_depth)
                    && paren_depth == 0
                    && bracket_depth == 0 =>
            {
                ignored[index] = true;
            }
            _ => {}
        }
    }
    ignored
}

fn redundant_default_file_alias(canonical: &[Token], tokens: &[Token], index: usize) -> bool {
    if index + 1 >= tokens.len()
        || !matches!(&tokens[index].kind, TokKind::Ident(name) if name == jet::Syntax::KW_AS)
    {
        return false;
    }
    let Some(Token {
        kind: TokKind::Str(parts),
        ..
    }) = canonical.last()
    else {
        return false;
    };
    let Some(path) = plain_string_value(parts) else {
        return false;
    };
    matches!(
        &tokens[index + 1].kind,
        TokKind::Ident(alias)
            if path.rsplit('/').next().is_some_and(|default| default == alias)
    )
}

fn plain_string_value(parts: &[StrTokPart]) -> Option<String> {
    let mut value = String::new();
    for part in parts {
        match part {
            StrTokPart::Lit(text) => value.push_str(text),
            StrTokPart::Interp(_) => return None,
        }
    }
    Some(value)
}

fn token_kinds_equal(left: &TokKind, right: &TokKind) -> bool {
    match (left, right) {
        (TokKind::Str(left), TokKind::Str(right)) => string_parts_equal(left, right),
        // D-BINPAT1 / D-UNIFYLIT1=A: `[U8].{"…"}` binary patterns are ordinary
        // `Str` parts inside a typed literal, so the `Str` arm already covers them.
        _ => left == right,
    }
}

fn string_parts_equal(left: &[StrTokPart], right: &[StrTokPart]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| match (left, right) {
            (StrTokPart::Lit(left), StrTokPart::Lit(right)) => left == right,
            (StrTokPart::Interp(left), StrTokPart::Interp(right)) => {
                let left: Vec<_> = left
                    .iter()
                    .filter(|token| !matches!(token.kind, TokKind::Semi | TokKind::Eof))
                    .collect();
                let right: Vec<_> = right
                    .iter()
                    .filter(|token| !matches!(token.kind, TokKind::Semi | TokKind::Eof))
                    .collect();
                left.len() == right.len()
                    && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| token_kinds_equal(&left.kind, &right.kind))
            }
            _ => false,
        })
}

fn assert_fmt_preserves_token_stream(path: &std::path::Path, src: &str, formatted: &str) {
    let original_tokens = canonical_tokens(&src, path);
    let formatted_tokens = canonical_tokens(&formatted, path);
    if let Err(diff) = ordered_token_diff(path, &original_tokens, &formatted_tokens) {
        panic!("{diff}");
    }
    let twice = format_supported_source(path, formatted).unwrap_or_else(|diagnostics| {
        panic!(
            "second fmt failed on {}:\n{}",
            path.display(),
            jet::render_diagnostics(&path.display().to_string(), &formatted, &diagnostics)
        )
    });
    assert_eq!(
        formatted,
        twice,
        "fmt is not byte-stable on {}",
        path.display()
    );
}

fn format_supported_source(
    path: &std::path::Path,
    src: &str,
) -> Result<String, Vec<jet::Diagnostics::Diagnostic>> {
    match jet::format_source(src) {
        Ok(formatted) => Ok(formatted),
        Err(diagnostics) => jet::Package::format_source(src, path.display().to_string())
            .or(Err(diagnostics)),
    }
}

fn ordered_token_diff(
    path: &std::path::Path,
    original: &[Token],
    formatted: &[Token],
) -> Result<(), String> {
    let mut original_i = 0;
    let mut formatted_i = 0;
    let mut inserted_arm_braces = 0usize;
    let mut inserted_lambda_parens = 0usize;
    while original_i < original.len() || formatted_i < formatted.len() {
        if original_i < original.len()
            && formatted_i < formatted.len()
            && token_kinds_equal(&original[original_i].kind, &formatted[formatted_i].kind)
        {
            original_i += 1;
            formatted_i += 1;
            continue;
        }
        if let Some((next_original, next_formatted)) =
            reordered_simple_union_type(original, original_i, formatted, formatted_i)
        {
            original_i = next_original;
            formatted_i = next_formatted;
            continue;
        }
        if formatted_arm_block_opens(formatted, formatted_i)
            && !matches!(original.get(original_i).map(|t| &t.kind), Some(TokKind::LBrace))
        {
            inserted_arm_braces += 1;
            formatted_i += 1;
            continue;
        }
        if formatted_lambda_params_open(formatted, formatted_i)
            && !matches!(original.get(original_i).map(|t| &t.kind), Some(TokKind::LParen))
        {
            inserted_lambda_parens += 1;
            formatted_i += 1;
            continue;
        }
        if formatted_variant_pattern_dot(original, original_i, formatted, formatted_i) {
            formatted_i += 1;
            continue;
        }
        if inserted_lambda_parens > 0
            && matches!(formatted.get(formatted_i).map(|t| &t.kind), Some(TokKind::RParen))
            && matches!(
                formatted.get(formatted_i + 1).map(|t| &t.kind),
                Some(TokKind::LambdaArrow)
            )
            && matches!(
                original.get(original_i).map(|t| &t.kind),
                Some(TokKind::LambdaArrow)
            )
        {
            inserted_lambda_parens -= 1;
            formatted_i += 1;
            continue;
        }
        if inserted_arm_braces > 0
            && matches!(formatted.get(formatted_i).map(|t| &t.kind), Some(TokKind::RBrace))
            && (next_token_matches(original, original_i, formatted, formatted_i + 1)
                || formatted_variant_pattern_dot(
                    original,
                    original_i,
                    formatted,
                    formatted_i + 1,
                ))
        {
            inserted_arm_braces -= 1;
            formatted_i += 1;
            continue;
        }
        if formatted_struct_shorthand_expansion(formatted, formatted_i) {
            formatted_i += 2;
            continue;
        }
        if formatted_default_module_alias(formatted, formatted_i) {
            formatted_i += 2;
            continue;
        }
        if formatted_struct_separator(formatted, formatted_i) {
            formatted_i += 1;
            continue;
        }
        return Err(format!(
            "fmt changed ordered token {} on {}:\n  before: {:?}\n  after: {:?}",
            original_i,
            path.display(),
            original.get(original_i).map(|token| &token.kind),
            formatted.get(formatted_i).map(|token| &token.kind)
        ));
    }
    if inserted_arm_braces != 0 {
        return Err(format!(
            "fmt left an unmatched canonical arm block on {}",
            path.display()
        ));
    }
    if inserted_lambda_parens != 0 {
        return Err(format!(
            "fmt left unmatched canonical lambda parens on {}",
            path.display()
        ));
    }
    Ok(())
}

fn reordered_simple_union_type(
    original: &[Token],
    original_i: usize,
    formatted: &[Token],
    formatted_i: usize,
) -> Option<(usize, usize)> {
    if !simple_union_type_annotation(original, original_i)
        || !simple_union_type_annotation(formatted, formatted_i)
    {
        return None;
    }
    let (mut original_members, next_original) =
        simple_union_members(original, original_i)?;
    let (mut formatted_members, next_formatted) =
        simple_union_members(formatted, formatted_i)?;
    original_members.sort();
    formatted_members.sort();
    (original_members == formatted_members).then_some((next_original, next_formatted))
}

fn simple_union_type_annotation(tokens: &[Token], start: usize) -> bool {
    let Some(colon) = start.checked_sub(1) else {
        return false;
    };
    if !matches!(tokens.get(colon).map(|token| &token.kind), Some(TokKind::Colon)) {
        return false;
    }

    let mut awaiting_struct_body = false;
    let mut awaiting_fn_params = false;
    let mut struct_scopes = Vec::new();
    let mut fn_param_scopes = Vec::new();
    let mut brace_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    for token in &tokens[..colon] {
        match token.kind {
            TokKind::KwStruct => awaiting_struct_body = true,
            TokKind::KwFn => awaiting_fn_params = true,
            TokKind::LBrace => {
                brace_depth += 1;
                if awaiting_struct_body {
                    struct_scopes.push((brace_depth, paren_depth, bracket_depth));
                    awaiting_struct_body = false;
                }
            }
            TokKind::RBrace => {
                if struct_scopes
                    .last()
                    .is_some_and(|scope| scope.0 == brace_depth)
                {
                    struct_scopes.pop();
                }
                brace_depth -= 1;
            }
            TokKind::LParen => {
                paren_depth += 1;
                if awaiting_fn_params {
                    fn_param_scopes.push((brace_depth, paren_depth, bracket_depth));
                    awaiting_fn_params = false;
                }
            }
            TokKind::RParen => {
                if fn_param_scopes
                    .last()
                    .is_some_and(|scope| scope.1 == paren_depth)
                {
                    fn_param_scopes.pop();
                }
                paren_depth -= 1;
            }
            TokKind::LBracket => bracket_depth += 1,
            TokKind::RBracket => bracket_depth -= 1,
            _ => {}
        }
    }
    let scope = (brace_depth, paren_depth, bracket_depth);
    struct_scopes.last() == Some(&scope) || fn_param_scopes.last() == Some(&scope)
}

fn simple_union_members(tokens: &[Token], start: usize) -> Option<(Vec<&str>, usize)> {
    let TokKind::Ident(first) = &tokens.get(start)?.kind else {
        return None;
    };
    let mut members = vec![first.as_str()];
    let mut cursor = start + 1;
    while matches!(tokens.get(cursor).map(|token| &token.kind), Some(TokKind::Pipe)) {
        let TokKind::Ident(member) = &tokens.get(cursor + 1)?.kind else {
            return None;
        };
        members.push(member);
        cursor += 2;
    }
    (members.len() > 1
        && matches!(
            tokens.get(cursor).map(|token| &token.kind),
            Some(TokKind::Comma | TokKind::Eq | TokKind::RBrace | TokKind::RParen)
        ))
    .then_some((members, cursor))
}

fn formatted_variant_pattern_dot(
    original: &[Token],
    original_i: usize,
    formatted: &[Token],
    formatted_i: usize,
) -> bool {
    let (Some(original_token), Some(Token { kind: TokKind::Dot, .. }), Some(formatted_token)) = (
        original.get(original_i),
        formatted.get(formatted_i),
        formatted.get(formatted_i + 1),
    ) else {
        return false;
    };
    let same_variant = match (&original_token.kind, &formatted_token.kind) {
        (TokKind::Ident(original), TokKind::Ident(formatted)) => {
            original == formatted && original.starts_with(char::is_uppercase)
        }
        (TokKind::KwNull, TokKind::KwNull) => true,
        _ => false,
    };
    same_variant && variant_pattern_context(original, original_i)
}

fn variant_pattern_context(tokens: &[Token], index: usize) -> bool {
    if matches!(
        index.checked_sub(1).and_then(|i| tokens.get(i)).map(|token| &token.kind),
        Some(TokKind::EqEq)
    ) {
        return true;
    }

    let mut brace_depth = 0usize;
    let mut dispatch_open = None;
    for cursor in (0..index).rev() {
        match tokens[cursor].kind {
            TokKind::RBrace => brace_depth += 1,
            TokKind::LBrace if brace_depth > 0 => brace_depth -= 1,
            TokKind::LBrace => {
                dispatch_open = Some(cursor);
                break;
            }
            _ => {}
        }
    }
    let Some(open) = dispatch_open else {
        return false;
    };
    matches!(
        open.checked_sub(1).and_then(|i| tokens.get(i)).map(|token| &token.kind),
        Some(TokKind::EqEq)
    ) && same_brace_scope_has_if(tokens, open - 1)
        && pattern_reaches_arrow(tokens, index)
}

fn same_brace_scope_has_if(tokens: &[Token], before: usize) -> bool {
    for token in tokens[..before].iter().rev() {
        match token.kind {
            TokKind::KwIf => return true,
            TokKind::LBrace | TokKind::RBrace | TokKind::Arrow => return false,
            _ => {}
        }
    }
    false
}

fn pattern_reaches_arrow(tokens: &[Token], index: usize) -> bool {
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    for token in &tokens[index + 1..] {
        match token.kind {
            TokKind::LParen => paren_depth += 1,
            TokKind::RParen if paren_depth > 0 => paren_depth -= 1,
            TokKind::LBracket => bracket_depth += 1,
            TokKind::RBracket if bracket_depth > 0 => bracket_depth -= 1,
            TokKind::Arrow if paren_depth == 0 && bracket_depth == 0 => return true,
            TokKind::LBrace | TokKind::RBrace if paren_depth == 0 && bracket_depth == 0 => {
                return false;
            }
            _ => {}
        }
    }
    false
}

fn formatted_arm_block_opens(tokens: &[Token], index: usize) -> bool {
    matches!(tokens.get(index).map(|t| &t.kind), Some(TokKind::LBrace))
        && matches!(
            index.checked_sub(1).and_then(|i| tokens.get(i)).map(|t| &t.kind),
            Some(TokKind::Arrow)
        )
}

fn formatted_lambda_params_open(tokens: &[Token], index: usize) -> bool {
    if !matches!(tokens.get(index).map(|t| &t.kind), Some(TokKind::LParen)) {
        return false;
    }
    let mut depth = 0usize;
    for (cursor, token) in tokens.iter().enumerate().skip(index) {
        match token.kind {
            TokKind::LParen => depth += 1,
            TokKind::RParen => {
                depth -= 1;
                if depth == 0 {
                    return matches!(
                        tokens.get(cursor + 1).map(|t| &t.kind),
                        Some(TokKind::LambdaArrow)
                    );
                }
            }
            _ => {}
        }
    }
    false
}

fn next_token_matches(
    original: &[Token],
    original_i: usize,
    formatted: &[Token],
    formatted_i: usize,
) -> bool {
    match (original.get(original_i), formatted.get(formatted_i)) {
        (None, None) => true,
        (Some(original), Some(formatted)) => token_kinds_equal(&original.kind, &formatted.kind),
        _ => false,
    }
}

fn formatted_struct_shorthand_expansion(tokens: &[Token], index: usize) -> bool {
    let (Some(Token { kind: TokKind::Colon, .. }), Some(Token { kind: TokKind::Ident(value), .. })) =
        (tokens.get(index), tokens.get(index + 1))
    else {
        return false;
    };
    let Some(Token { kind: TokKind::Ident(field), .. }) =
        index.checked_sub(1).and_then(|i| tokens.get(i))
    else {
        return false;
    };
    if field != value {
        return false;
    }
    let mut depth = 0usize;
    for cursor in (0..index).rev() {
        match tokens[cursor].kind {
            TokKind::RBrace => depth += 1,
            TokKind::LBrace if depth == 0 => {
                return matches!(
                    cursor.checked_sub(1).and_then(|i| tokens.get(i)).map(|t| &t.kind),
                    Some(TokKind::Dot)
                );
            }
            TokKind::LBrace => depth -= 1,
            _ => {}
        }
    }
    false
}

fn formatted_default_module_alias(tokens: &[Token], index: usize) -> bool {
    let (Some(Token { kind: TokKind::Ident(as_kw), .. }), Some(Token { kind: TokKind::Ident(alias), .. })) =
        (tokens.get(index), tokens.get(index + 1))
    else {
        return false;
    };
    as_kw == jet::Syntax::KW_AS
        && matches!(
            index.checked_sub(1).and_then(|i| tokens.get(i)).map(|t| &t.kind),
            Some(TokKind::Ident(segment)) if segment == alias
        )
        && matches!(
            index.checked_sub(2).and_then(|i| tokens.get(i)).map(|t| &t.kind),
            Some(TokKind::Dot)
        )
        && exact_use_path_before_alias(tokens, index)
}

fn exact_use_path_before_alias(tokens: &[Token], alias_index: usize) -> bool {
    let Some(use_index) = tokens[..alias_index]
        .iter()
        .rposition(|token| matches!(token.kind, TokKind::KwUse))
    else {
        return false;
    };
    let path = &tokens[use_index + 1..alias_index];
    if path.len() < 3 || path.len() % 2 == 0 {
        return false;
    }
    path.iter().enumerate().all(|(index, token)| {
        if index % 2 == 0 {
            matches!(token.kind, TokKind::Ident(_))
        } else {
            matches!(token.kind, TokKind::Dot)
        }
    })
}

fn formatted_struct_separator(tokens: &[Token], index: usize) -> bool {
    if !matches!(tokens.get(index).map(|t| &t.kind), Some(TokKind::Comma)) {
        return false;
    }
    let mut depth = 0usize;
    for cursor in (0..index).rev() {
        match tokens[cursor].kind {
            TokKind::RBrace => depth += 1,
            TokKind::LBrace if depth == 0 => {
                return matches!(
                    cursor.checked_sub(1).and_then(|i| tokens.get(i)).map(|t| &t.kind),
                    Some(TokKind::Dot)
                );
            }
            TokKind::LBrace => depth -= 1,
            _ => {}
        }
    }
    false
}

#[test]
fn fmt_is_lossless_on_supported_source_corpus() {
    // The recursive parser's large expression frames exceed the default test
    // thread stack on the nested XML corpus fixture. Isolate this one corpus
    // worker without changing global test settings, and preserve its panic.
    let worker = std::thread::Builder::new()
        .name("fmt-lossless-corpus".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(run_supported_source_corpus)
        .expect("start lossless formatter corpus worker");
    if let Err(payload) = worker.join() {
        std::panic::resume_unwind(payload);
    }
}

fn run_supported_source_corpus() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let example_files = collect_jet_files_recursive(&root.join("examples"));
    let ui_files = collect_jet_files_recursive(&root.join("tests/ui"));
    let mut example_programs = 0usize;
    let mut example_configs = 0usize;
    for path in &example_files {
        let name = path.file_name().and_then(|name| name.to_str());
        if matches!(
            name,
            Some(jet::Syntax::PAYLOAD_FILE)
                | Some(jet::Syntax::WORKSPACE_FILE)
                | Some(jet::Syntax::PACKAGE_FILE)
        ) {
            example_configs += 1;
            continue;
        }
        let src = fs::read_to_string(path).unwrap();
        match jet::format_source(&src) {
            Ok(formatted) => {
                assert_fmt_preserves_token_stream(path, &src, &formatted);
                example_programs += 1;
            }
            Err(diagnostics) => {
                let formatted = jet::Package::format_source(&src, path.display().to_string())
                    .unwrap_or_else(|_| {
                        panic!(
                            "example program failed to parse for fmt on {}:\n{}",
                            path.display(),
                            jet::render_diagnostics(
                                &path.display().to_string(),
                                &src,
                                &diagnostics
                            )
                        )
                    });
                assert_fmt_preserves_token_stream(path, &src, &formatted);
                example_configs += 1;
            }
        }
    }
    let mut ui_parse_valid = 0usize;
    let mut ui_parse_invalid = 0usize;
    let mut ui_configs = 0usize;
    let mut actual_invalid = Vec::new();
    let mut mismatches = Vec::new();
    for path in &ui_files {
        let name = path.file_name().and_then(|name| name.to_str());
        if matches!(
            name,
            Some(jet::Syntax::PAYLOAD_FILE)
                | Some(jet::Syntax::WORKSPACE_FILE)
                | Some(jet::Syntax::PACKAGE_FILE)
        ) {
            // Package/workspace manifests are config, never program source — the
            // same rule the examples loop above applies. They are not pinned in
            // UI_PARSE_INVALID; a new manifest fixture needs no manifest entry.
            ui_configs += 1;
            continue;
        }
        let src = fs::read_to_string(path).unwrap();
        let parsed = jet::Compiler::parse_source(&src);
        let relative = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let expected_invalid = UI_PARSE_INVALID.binary_search(&relative.as_str()).is_ok();
        let actual_is_invalid = !parsed.diagnostics.is_empty();
        if actual_is_invalid != expected_invalid {
            mismatches.push(format!(
                "UI parser-validity changed for {relative}; update the pinned manifest only after review (expected_invalid={expected_invalid}, actual_is_invalid={actual_is_invalid})"
            ));
            continue;
        }
        if expected_invalid {
            actual_invalid.push(relative);
            ui_parse_invalid += 1;
        } else {
            let is_fixed_companion = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".fixed.jet"));
            // UI error fixtures may use intentional non-canonical spelling; .fixed.jet
            // companions are fmt outputs and are not re-checked for token identity.
            if !is_fixed_companion {
                let formatted = jet::format_source(&src).unwrap_or_else(|diagnostics| {
                    panic!(
                        "parse-valid UI fixture failed formatter parse on {}:\n{}",
                        path.display(),
                        jet::render_diagnostics(&path.display().to_string(), &src, &diagnostics)
                    )
                });
                assert_fmt_preserves_token_stream(path, &src, &formatted);
            }
            ui_parse_valid += 1;
        }
    }
    assert!(
        mismatches.is_empty(),
        "UI corpus parser-validity mismatches ({} found):\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    assert_eq!(
        example_files.len(),
        example_programs + example_configs,
        "every example .jet file must be program source or explicit package/workspace config"
    );
    assert_eq!(
        ui_files.len(),
        ui_parse_valid + ui_parse_invalid + ui_configs,
        "every UI .jet fixture must be classified by its actual formatter parse result"
    );
    assert!(
        UI_PARSE_INVALID.windows(2).all(|pair| pair[0] < pair[1]),
        "UI parse-invalid manifest must be sorted and duplicate-free"
    );
    assert_eq!(
        actual_invalid,
        UI_PARSE_INVALID,
        "discovered parser-invalid UI set must exactly equal the pinned manifest"
    );
    eprintln!(
        "formatter corpus: {example_programs} example programs, {example_configs} configs, \
         {ui_parse_valid} parse-valid UI fixtures, {ui_parse_invalid} parse-invalid UI fixtures, \
         {ui_configs} UI manifest configs"
    );
}

fn ordered_sources_diff(original: &str, formatted: &str) -> Result<(), String> {
    let path = std::path::Path::new("canonical-rule.jet");
    let original = canonical_tokens(original, path);
    let formatted = canonical_tokens(formatted, path);
    ordered_token_diff(path, &original, &formatted)
}

#[test]
fn canonical_rewrite_rules_are_explicit_and_narrow() {
    let allowed = [
        (
            "top-level ordering",
            "#Target(Web)\nuse core.ui as ui\nfn run() {}\n",
            "use core.ui as ui\n#Target(Web)\nfn run() {}\n",
        ),
        (
            "import-hoisted top-level comments",
            "// helper\nfn helper() {}\n// import\nuse core.ui as ui\n",
            "// helper\n// import\nuse core.ui as ui\nfn helper() {}\n",
        ),
        (
            "spanless marker comment position",
            "// web target\n#Target(Web)\nfn run() {}\n",
            "#Target(Web)\n// web target\nfn run() {}\n",
        ),
        (
            "marker grouping",
            "#Codable\n#RenameAll(camel)\nstruct S {}\n",
            "#[Codable, RenameAll(camel)]\nstruct S {}\n",
        ),
        (
            "static generic call",
            "fn run() { Pool<Node>.new() }\n",
            "fn run() { Pool.new<Node>() }\n",
        ),
        (
            "external method",
            "fn Point.len(self) => Int { return 1 }\n",
            "impl Point { fn len(self) => Int { return 1 } }\n",
        ),
        (
            "enum group separators",
            "enum E { G { A, B } }\n",
            "enum E { G { A B } }\n",
        ),
        (
            "struct declaration separators",
            "struct S { x: Int, y: Int }\n",
            "struct S { x: Int y: Int }\n",
        ),
        (
            "trailing comma",
            "fn run() { xs :: [1,] }\n",
            "fn run() { xs :: [1] }\n",
        ),
        (
            "default file alias",
            "use \"./foo\" as foo\nfn run() {}\n",
            "use \"./foo\"\nfn run() {}\n",
        ),
        (
            "dispatch arm block",
            "fn run() { if x == { .A -> print(1) } }\n",
            "fn run() { if x == { .A -> { print(1) } } }\n",
        ),
        (
            "bare lambda params",
            "fn run() { f :: x => x }\n",
            "fn run() { f :: (x) => x }\n",
        ),
        (
            "bare enum variant pattern",
            "fn run() { if x == { A(v) -> print(v) } }\n",
            "fn run() { if x == { .A(v) -> print(v) } }\n",
        ),
        (
            "bare None variant pattern",
            "fn run() { if x == { None -> print(0) } }\n",
            "fn run() { if x == { .None -> print(0) } }\n",
        ),
        (
            "struct shorthand label",
            "fn run() { p :: Point.{x} }\n",
            "fn run() { p :: Point.{x: x} }\n",
        ),
        (
            "default module alias",
            "use core.encoding.json\nfn run() {}\n",
            "use core.encoding.json as json\nfn run() {}\n",
        ),
        (
            "struct literal separator",
            "fn run() { p :: Point.{x: 1 y: 2} }\n",
            "fn run() { p :: Point.{x: 1, y: 2} }\n",
        ),
        (
            "anonymous union member order",
            "struct S { value: String | Char }\n",
            "struct S { value: Char | String }\n",
        ),
    ];
    for (name, original, formatted) in allowed {
        assert!(
            ordered_sources_diff(original, formatted).is_ok(),
            "allowed canonical rewrite failed: {name}"
        );
    }

    let paired_forbidden = [
        (
            "top-level ordering does not reorder ordinary items",
            "fn first() {}\nfn second() {}\n",
            "fn second() {}\nfn first() {}\n",
        ),
        (
            "import hoisting does not reorder comments",
            "// helper\nfn helper() {}\n// import\nuse core.ui as ui\n",
            "// import\n// helper\nuse core.ui as ui\nfn helper() {}\n",
        ),
        (
            "spanless marker movement does not delete comments",
            "// web target\n#Target(Web)\nfn run() {}\n",
            "#Target(Web)\nfn run() {}\n",
        ),
        (
            "marker grouping does not reorder markers",
            "#[Codable, RenameAll(camel)]\nstruct S {}\n",
            "#[RenameAll(camel), Codable]\nstruct S {}\n",
        ),
        (
            "generic-call rewrite requires a call",
            "fn run() { value :: Pool<Node>.field }\n",
            "fn run() { value :: Pool.field<Node> }\n",
        ),
        (
            "external-method rewrite preserves receiver",
            "fn Point.len(self) => Int { return 1 }\n",
            "impl Other { fn len(self) => Int { return 1 } }\n",
        ),
        (
            "task-block rewrite preserves body",
            "fn run() { g.task { work() } }\n",
            "fn run() { g.task => { other() } }\n",
        ),
        (
            "enum-group comma rule preserves variant order",
            "enum E { G { A, B } }\n",
            "enum E { G { B A } }\n",
        ),
        (
            "struct-declaration comma rule preserves field order",
            "struct S { x: Int, y: Int }\n",
            "struct S { y: Int x: Int }\n",
        ),
        (
            "trailing-comma rule does not remove argument separators",
            "fn run() { call(a, b) }\n",
            "fn run() { call(a b) }\n",
        ),
        (
            "file-alias rule requires the default alias",
            "use \"./foo\" as bar\nfn run() {}\n",
            "use \"./foo\"\nfn run() {}\n",
        ),
        (
            "arm-block rule does not remove explicit braces",
            "fn run() { if x == { .A -> { print(1) } } }\n",
            "fn run() { if x == { .A -> print(1) } }\n",
        ),
        (
            "lambda rule does not remove explicit parens",
            "fn run() { f :: (x) => x }\n",
            "fn run() { f :: x => x }\n",
        ),
        (
            "variant-dot rule requires a PascalCase pattern",
            "fn run() { if x == { value -> print(value) } }\n",
            "fn run() { if x == { .value -> print(value) } }\n",
        ),
        (
            "variant-dot rule requires a dispatch arm context",
            "fn run() { Foo work -> print(1) }\n",
            "fn run() { .Foo work -> print(1) }\n",
        ),
        (
            "shorthand expansion preserves field value",
            "fn run() { p :: Point.{x} }\n",
            "fn run() { p :: Point.{x: y} }\n",
        ),
        (
            "module alias requires exact use-path context",
            "use core.io as io\nfn run() { foo.bar }\n",
            "use core.io as io\nfn run() { foo.bar as bar }\n",
        ),
        (
            "struct-literal separator rule does not remove commas",
            "fn run() { p :: Point.{x: 1, y: 2} }\n",
            "fn run() { p :: Point.{x: 1 y: 2} }\n",
        ),
        (
            "union order rule requires a type annotation",
            "fn run() { value :: left | right }\n",
            "fn run() { value :: right | left }\n",
        ),
        (
            "union order rule rejects struct field values",
            "fn run() { p :: Point.{x: left | right} }\n",
            "fn run() { p :: Point.{x: right | left} }\n",
        ),
        (
            "union order rule rejects dotted member replacement",
            "struct S { value: A | B.C }\n",
            "struct S { value: B | A.C }\n",
        ),
        (
            "union order rule rejects generic member replacement",
            "struct S { value: A | B<C> }\n",
            "struct S { value: B | A<C> }\n",
        ),
        (
            "union order rule preserves every member",
            "struct S { value: A | B }\n",
            "struct S { value: A | C }\n",
        ),
    ];
    for (name, original, formatted) in paired_forbidden {
        assert!(
            ordered_sources_diff(original, formatted).is_err(),
            "paired forbidden neighbor was accepted: {name}"
        );
    }

    let forbidden = [
        (
            "operand reorder",
            "fn run() { x :: a + b }\n",
            "fn run() { x :: b + a }\n",
        ),
        (
            "marker payload deletion",
            "#Pre(x > 0, \"positive\") fn f(x: Int) {}\n",
            "#Pre(x > 0) fn f(x: Int) {}\n",
        ),
        (
            "comment reorder",
            "// first\n// second\nfn run() {}\n",
            "// second\n// first\nfn run() {}\n",
        ),
        (
            "grouping delimiter deletion",
            "fn run() { x :: (a + b) }\n",
            "fn run() { x :: a + b }\n",
        ),
        (
            "interpolation deletion",
            "fn run() { print(\"{a + b}\") }\n",
            "fn run() { print(\"{a}\") }\n",
        ),
        (
            "arbitrary dotted alias insertion",
            "fn run() { foo.bar }\n",
            "fn run() { foo.bar as bar }\n",
        ),
        (
            "operator deletion",
            "fn run() { x :: a && b }\n",
            "fn run() { x :: a b }\n",
        ),
        (
            "meaningful token addition",
            "fn run() { call(a) }\n",
            "fn run() { call(a, b) }\n",
        ),
    ];
    for (name, original, formatted) in forbidden {
        assert!(
            ordered_sources_diff(original, formatted).is_err(),
            "forbidden rewrite was accepted: {name}"
        );
    }
}
