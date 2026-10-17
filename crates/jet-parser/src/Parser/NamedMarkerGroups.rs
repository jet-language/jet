//! D-MARKER-GROUP1=A: fixed group expansion precedes ordinary marker readers.
use super::{Span, TokKind, Token};
use std::borrow::Cow;

#[derive(Clone)]
pub struct NamedMarkerGroupTokens {
    pub name: String,
    pub name_span: Span,
    pub members: Vec<Token>,
}

pub struct ExpandedMarkerGroupUse {
    pub name: String,
    pub span: Span,
}

/// Borrowing lookahead for every marker path, including multi-segment imports.
pub fn marker_path_end(tokens: &[Token], start: usize) -> Option<usize> {
    if !matches!(tokens.get(start)?.kind, TokKind::Ident(_)) { return None; }
    let mut end = start + 1;
    while matches!(tokens.get(end).map(|token| &token.kind), Some(TokKind::Dot)) {
        if !matches!(tokens.get(end + 1).map(|token| &token.kind), Some(TokKind::Ident(_))) { return None; }
        end += 2;
    }
    Some(end)
}

pub fn marker_path_at(tokens: &[Token], start: usize) -> Option<(String, usize)> {
    let TokKind::Ident(first) = &tokens.get(start)?.kind else { return None };
    let end = marker_path_end(tokens, start)?;
    let mut name = first.clone();
    for segment in (start + 2..end).step_by(2) {
        let TokKind::Ident(member) = &tokens[segment].kind else { unreachable!() };
        name.push('.'); name.push_str(member);
    }
    Some((name, end))
}

fn declaration_range(tokens: &[Token], start: usize) -> Option<(String, usize, usize)> {
    if !matches!(&tokens.get(start)?.kind, TokKind::Ident(word) if word == crate::Syntax::KW_MARKER)
        || !matches!(tokens.get(start + 2)?.kind, TokKind::Eq)
        || !matches!(tokens.get(start + 3)?.kind, TokKind::Hash)
        || !matches!(tokens.get(start + 4)?.kind, TokKind::LBracket) { return None; }
    let TokKind::Ident(name) = &tokens.get(start + 1)?.kind else { return None };
    let mut depth = 1;
    let mut end = start + 5;
    while end < tokens.len() {
        match tokens[end].kind {
            TokKind::LBracket => depth += 1,
            TokKind::RBracket => { depth -= 1; if depth == 0 { return Some((name.clone(), start + 5, end)); } }
            TokKind::Eof => break,
            _ => {}
        }
        end += 1;
    }
    None
}

#[derive(Default)]
struct NamespaceScope {
    braces: Vec<bool>,
    path: String,
}
impl NamespaceScope {
    fn advance(&mut self, tokens: &[Token], index: usize) {
        match tokens[index].kind {
            TokKind::LBrace => {
                let module = index.checked_sub(2).and_then(|start| match (&tokens[start].kind, &tokens[start + 1].kind) {
                    (TokKind::Ident(keyword), TokKind::Ident(name)) if keyword == crate::Syntax::KW_MODULE => Some(name.as_str()),
                    _ => None,
                });
                self.braces.push(module.is_some());
                if let Some(name) = module { if !self.path.is_empty() { self.path.push('.'); } self.path.push_str(name); }
            }
            TokKind::RBrace => {
                if self.braces.pop() == Some(true) {
                    if let Some(last) = self.path.rfind('.') { self.path.truncate(last); } else { self.path.clear(); }
                }
            }
            _ => {}
        }
    }
    fn declarative(&self) -> bool { self.braces.iter().all(|module| *module) }
    fn qualify(&self, name: &str) -> String {
        if self.path.is_empty() { name.into() } else { format!("{}.{name}", self.path) }
    }
}

pub fn named_marker_groups(tokens: &[Token]) -> Vec<NamedMarkerGroupTokens> {
    let mut groups = Vec::new();
    let mut index = 0;
    let mut scope = NamespaceScope::default();
    while index < tokens.len() {
        if scope.declarative() {
            if let Some((name, start, end)) = declaration_range(tokens, index) {
                groups.push(NamedMarkerGroupTokens { name: scope.qualify(&name), name_span: tokens[index + 1].span, members: tokens[start..end].to_vec() });
                index = end + 1; continue;
            }
        }
        scope.advance(tokens, index);
        index += 1;
    }
    groups
}

/// Rewrite only member heads, never identifiers inside fixed arguments.
/// Replay uses this to carry lexical custom-rule names through the ordinary
/// parser, which intentionally defers qualified names to bundle resolution.
pub fn map_marker_group_members(group: &mut NamedMarkerGroupTokens, mut map: impl FnMut(&str) -> Option<String>) {
    let mut output = Vec::with_capacity(group.members.len());
    let mut index = 0;
    let mut depth = 0usize;
    let mut entry = true;
    while index < group.members.len() {
        let token = &group.members[index];
        if depth == 0 && entry && !matches!(token.kind, TokKind::Semi) {
            if matches!(token.kind, TokKind::Bang) { output.push(token.clone()); index += 1; }
            if index == group.members.len() { break; }
            if let Some((name, end)) = marker_path_at(&group.members, index) {
                if let Some(mapped) = map(&name) {
                    let span = group.members[index].span;
                    for (segment, name) in mapped.split('.').enumerate() {
                        if segment != 0 { output.push(Token { kind: TokKind::Dot, span }); }
                        output.push(Token { kind: TokKind::Ident(name.into()), span });
                    }
                } else { output.extend_from_slice(&group.members[index..end]); }
                index = end; entry = false; continue;
            }
        }
        match group.members[index].kind {
            TokKind::LParen | TokKind::LBracket | TokKind::LBrace => depth += 1,
            TokKind::RParen | TokKind::RBracket | TokKind::RBrace => depth = depth.saturating_sub(1),
            TokKind::Comma if depth == 0 => entry = true,
            _ => {}
        }
        output.push(group.members[index].clone()); index += 1;
    }
    group.members = output;
}

fn local_marker_names(tokens: &[Token]) -> Vec<String> {
    let mut names = Vec::new();
    let mut scope = NamespaceScope::default();
    for index in 0..tokens.len() {
        if scope.declarative()
            && matches!(&tokens[index].kind, TokKind::Ident(word) if word == crate::Syntax::KW_MARKER)
            && matches!(tokens.get(index + 2).map(|token| &token.kind), Some(TokKind::LParen | TokKind::Eq)) {
            if let Some(Token { kind: TokKind::Ident(name), .. }) = tokens.get(index + 1) { names.push(scope.qualify(name)); }
        }
        scope.advance(tokens, index);
    }
    names
}

fn lexical_name<'a>(names: &'a [String], mut scope: &str, name: &str) -> Option<&'a str> {
    loop {
        if !scope.is_empty() {
            if let Some(found) = names.iter().find(|candidate| candidate.strip_prefix(scope)
                .and_then(|tail| tail.strip_prefix('.')).is_some_and(|tail| tail == name)) { return Some(found); }
            scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
        } else { return names.iter().find(|candidate| candidate.as_str() == name).map(String::as_str); }
    }
}

// Invalid rows must reach the declaration checker, not produce a parser error
// at whichever use happened to expand first. No signature/site check lives here.
fn expandable(group: &NamedMarkerGroupTokens, groups: &[&NamedMarkerGroupTokens], tokens: &[Token]) -> bool {
    let mut index = 0;
    let mut depth = 0usize;
    let mut entry = true;
    let mut count = 0;
    while index < group.members.len() {
        let token = &group.members[index];
        if depth == 0 && entry && !matches!(token.kind, TokKind::Semi) {
            let negated = matches!(token.kind, TokKind::Bang);
            let Some((name, end)) = marker_path_at(&group.members, index + usize::from(negated)) else { return false };
            if crate::Policy::marker_group_member_forbidden(&name, negated) || groups.iter().any(|row| row.name == name) { return false; }
            if !name.contains('.') && crate::Policy::applied_rule(&name).is_none()
                && !tokens.windows(3).any(|row| matches!(&row[0].kind, TokKind::Ident(keyword) if keyword == crate::Syntax::KW_MARKER)
                    && matches!(&row[1].kind, TokKind::Ident(declared) if declared == &name) && matches!(row[2].kind, TokKind::LParen)) { return false; }
            count += 1; entry = false; index = end; continue;
        }
        match token.kind {
            TokKind::LParen | TokKind::LBracket | TokKind::LBrace => depth += 1,
            TokKind::RParen | TokKind::RBracket | TokKind::RBrace => depth = depth.saturating_sub(1),
            TokKind::Comma if depth == 0 => entry = true,
            _ => {}
        }
        index += 1;
    }
    count >= 2
}

fn push_members(output: &mut Vec<Token>, members: &[Token], span: Span) {
    output.extend(members.iter().cloned().map(|mut token| { token.span = span; token }));
}

pub fn expand_named_marker_groups<'a>(tokens: &'a [Token], imported: &[NamedMarkerGroupTokens]) -> Cow<'a, [Token]> {
    expand_groups(tokens, imported, None)
}

pub fn expand_named_marker_groups_with_uses<'a>(tokens: &'a [Token], imported: &[NamedMarkerGroupTokens]) -> (Cow<'a, [Token]>, Vec<ExpandedMarkerGroupUse>) {
    let mut uses = Vec::new();
    let expanded = expand_groups(tokens, imported, Some(&mut uses));
    (expanded, uses)
}

fn expand_groups<'a>(tokens: &'a [Token], imported: &[NamedMarkerGroupTokens], mut uses: Option<&mut Vec<ExpandedMarkerGroupUse>>) -> Cow<'a, [Token]> {
    let mut local = named_marker_groups(tokens);
    if local.is_empty() && imported.is_empty() { return Cow::Borrowed(tokens); }
    let names = local_marker_names(tokens);
    for group in &mut local {
        let scope = group.name.rsplit_once('.').map_or("", |(scope, _)| scope).to_owned();
        map_marker_group_members(group, |name| {
            if crate::Policy::applied_rule(name).is_some() { return None; }
            lexical_name(&names, &scope, name).map(str::to_owned)
        });
    }
    let all: Vec<_> = imported.iter().chain(local.iter().filter(|local| !imported.iter().any(|row| row.name == local.name))).collect();
    let eligible: Vec<_> = all.iter().copied().filter(|group| expandable(group, &all, tokens)).collect();
    let mut output = Vec::with_capacity(tokens.len());
    let group = |name: &str, scope: &NamespaceScope| {
        let mut path = scope.path.as_str();
        loop {
            if !path.is_empty() {
                if let Some(group) = eligible.iter().copied().find(|group| group.name.strip_prefix(path)
                    .and_then(|tail| tail.strip_prefix('.')).is_some_and(|tail| tail == name)) { return Some(group); }
                path = path.rsplit_once('.').map_or("", |(parent, _)| parent);
            } else { return eligible.iter().copied().find(|group| group.name == name); }
        }
    };
    let mut index = 0;
    let mut scope = NamespaceScope::default();
    while index < tokens.len() {
        if let Some((_, _, end)) = declaration_range(tokens, index) { output.extend_from_slice(&tokens[index..=end]); index = end + 1; continue; }
        if matches!(tokens[index].kind, TokKind::Hash) {
            if let Some((name, end)) = marker_path_at(tokens, index + 1) {
                if let Some(group) = group(&name, &scope) {
                    if !matches!(tokens.get(end).map(|t| &t.kind), Some(TokKind::LParen)) {
                        let span = Span::new(tokens[index].span.start, tokens[end - 1].span.end);
                        if let Some(uses) = &mut uses { uses.push(ExpandedMarkerGroupUse { name: group.name.clone(), span }); }
                        output.push(Token { kind: TokKind::Hash, span }); output.push(Token { kind: TokKind::LBracket, span });
                        push_members(&mut output, &group.members, span); output.push(Token { kind: TokKind::RBracket, span }); index = end; continue;
                    }
                }
            }
            if matches!(tokens.get(index + 1).map(|t| &t.kind), Some(TokKind::LBracket)) {
                output.extend_from_slice(&tokens[index..index + 2]); index += 2;
                let mut depth = 0usize;
                let mut entry = true;
                while index < tokens.len() {
                    if depth == 0 && matches!(tokens[index].kind, TokKind::RBracket) { break; }
                    if depth == 0 && entry {
                        if let Some((name, end)) = marker_path_at(tokens, index) {
                            if let Some(group) = group(&name, &scope) {
                                if !matches!(tokens.get(end).map(|t| &t.kind), Some(TokKind::LParen)) {
                                    let span = Span::new(tokens[index].span.start, tokens[end - 1].span.end);
                                    if let Some(uses) = &mut uses { uses.push(ExpandedMarkerGroupUse { name: group.name.clone(), span }); }
                                    push_members(&mut output, &group.members, span); index = end; entry = false; continue;
                                }
                            }
                        }
                    }
                    match tokens[index].kind {
                        TokKind::LParen | TokKind::LBracket | TokKind::LBrace => depth += 1,
                        TokKind::RParen | TokKind::RBracket | TokKind::RBrace => depth = depth.saturating_sub(1),
                        _ => {}
                    }
                    entry = depth == 0 && matches!(tokens[index].kind, TokKind::Comma);
                    output.push(tokens[index].clone()); index += 1;
                }
                continue;
            }
        }
        scope.advance(tokens, index);
        output.push(tokens[index].clone()); index += 1;
    }
    Cow::Owned(output)
}
