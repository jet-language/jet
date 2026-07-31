//! D-MEMPROVENANCE3=A: trailing `from` clause after a return type.

use crate::AST::{
    Param, ViewProvenance, ViewProvenanceMap, ViewSource, ViewSourcePath, ViewSourceProjection,
};
use crate::Lexer::TokKind;
use crate::Parser::Parser;
use crate::Syntax;
use std::collections::BTreeSet;

impl<'a> Parser<'a> {
    /// Parse an optional `from src (| src)*` or `from (slot: srcs, …)` after a
    /// return type. Parameter names resolve against ordinary (non-`self`) params.
    pub(crate) fn parse_opt_declared_view_from(
        &mut self,
        params: &[Param],
    ) -> Option<ViewProvenanceMap> {
        if !self.peek_is_ident(Syntax::VIEW_FROM) {
            return None;
        }
        if !matches!(
            self.peek2().kind,
            TokKind::Ident(_) | TokKind::KwSelf | TokKind::LParen
        ) {
            return None;
        }
        self.bump(); // `from`
        if matches!(self.peek().kind, TokKind::LParen) {
            return Some(self.parse_declared_view_from_slots(params));
        }
        let sources = self.parse_declared_view_source_union(params);
        let mut map = ViewProvenanceMap::new();
        map.insert(
            Vec::new(),
            ViewProvenance {
                sources,
                mutable: false,
            },
        );
        Some(map)
    }

    fn parse_declared_view_from_slots(&mut self, params: &[Param]) -> ViewProvenanceMap {
        self.bump(); // `(`
        let mut map = ViewProvenanceMap::new();
        loop {
            if matches!(self.peek().kind, TokKind::RParen | TokKind::Eof) {
                break;
            }
            let Some(slot) = self.bump_ident() else {
                self.bump();
                break;
            };
            if !matches!(self.peek().kind, TokKind::Colon) {
                break;
            }
            self.bump(); // `:`
            let sources = self.parse_declared_view_source_union(params);
            map.insert(
                vec![slot],
                ViewProvenance {
                    sources,
                    mutable: false,
                },
            );
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
                continue;
            }
            break;
        }
        let _ = self.expect(TokKind::RParen, "to close the `from (…)` slot list");
        map
    }

    fn parse_declared_view_source_union(&mut self, params: &[Param]) -> BTreeSet<ViewSourcePath> {
        let mut sources = BTreeSet::new();
        loop {
            if let Some(path) = self.parse_declared_view_source(params) {
                sources.insert(path);
            } else {
                break;
            }
            if matches!(self.peek().kind, TokKind::Pipe) {
                self.bump();
                continue;
            }
            break;
        }
        sources
    }

    fn parse_declared_view_source(&mut self, params: &[Param]) -> Option<ViewSourcePath> {
        let ordinary: Vec<&str> = params
            .iter()
            .filter(|param| param.name != Syntax::KW_SELF)
            .map(|param| param.name.as_str())
            .collect();
        let source = if matches!(self.peek().kind, TokKind::KwSelf)
            || self.peek_is_ident(Syntax::KW_SELF)
        {
            self.bump();
            ViewSource::Receiver
        } else if self.peek_is_ident(Syntax::VIEW_FROM_STATIC) {
            self.bump(); // `static`
            if !matches!(self.peek().kind, TokKind::Dot) {
                return None;
            }
            self.bump();
            let mut parts = Vec::new();
            let Some(first) = self.bump_ident() else {
                return None;
            };
            parts.push(first);
            while matches!(self.peek().kind, TokKind::Dot) {
                self.bump();
                let Some(next) = self.bump_ident() else {
                    break;
                };
                parts.push(next);
            }
            let name = parts.pop().unwrap_or_default();
            let module_path = parts.join(".");
            ViewSource::Static {
                module_path,
                name,
            }
        } else {
            let Some(name) = self.bump_ident() else {
                return None;
            };
            let index = ordinary.iter().position(|param| *param == name)?;
            ViewSource::Parameter(index)
        };
        let mut projections = Vec::new();
        while matches!(self.peek().kind, TokKind::Dot) {
            self.bump();
            if let Some(field) = self.bump_ident() {
                projections.push(ViewSourceProjection::Field(field));
            } else {
                break;
            }
        }
        Some(ViewSourcePath {
            source,
            projections,
        })
    }

    /// Parse optional `from src (| src)*` after a parameter type. Names stay
    /// unresolved so a later parameter may be referenced (Rust `'b: 'a`).
    pub(crate) fn parse_opt_param_view_from_names(&mut self) -> Option<Vec<String>> {
        if !self.peek_is_ident(Syntax::VIEW_FROM) {
            return None;
        }
        if !matches!(
            self.peek2().kind,
            TokKind::Ident(_) | TokKind::KwSelf
        ) {
            return None;
        }
        self.bump(); // `from`
        let mut names = Vec::new();
        loop {
            if matches!(self.peek().kind, TokKind::KwSelf) || self.peek_is_ident(Syntax::KW_SELF)
            {
                self.bump();
                names.push(Syntax::KW_SELF.to_string());
            } else if let Some(name) = self.bump_ident() {
                names.push(name);
            } else {
                break;
            }
            // Projections are allowed in the grammar; param requirements key on
            // the owner name only for this slice.
            while matches!(self.peek().kind, TokKind::Dot) {
                self.bump();
                let _ = self.bump_ident();
            }
            if matches!(self.peek().kind, TokKind::Pipe) {
                self.bump();
                continue;
            }
            break;
        }
        if names.is_empty() {
            None
        } else {
            Some(names)
        }
    }

    fn bump_ident(&mut self) -> Option<String> {
        match &self.peek().kind {
            TokKind::Ident(name) => {
                let name = name.clone();
                self.bump();
                Some(name)
            }
            _ => None,
        }
    }
}
