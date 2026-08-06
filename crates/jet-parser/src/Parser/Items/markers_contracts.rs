use super::super::{Diagnostic, Func, Parser, Span, Syntax, TokKind};
use super::TargetMarker;

impl<'a> Parser<'a> {

        /// S60 (D-CASING1 follow-on) / D-MARKERMOVE1/2: consume a `#Pure` /
        /// prefix already confirmed present by `at_pure_fn`.
        pub(super) fn bump_pure_marker(&mut self) {
            let span = self.bump().span; // `@`
            self.bump(); // `Pure`
            self.diags.push(Self::retired_effect_syntax(span));
        }
    
        pub(super) fn retired_inline_always(name: &str) -> bool {
            crate::Policy::applied_rule(name).is_some_and(|row| {
                matches!(
                    row.status,
                    crate::Policy::RuleStatus::Retired {
                        replacement: "#Inline(Always)"
                    }
                )
            })
        }

    
    
        pub(in crate::Parser) fn func(&mut self) -> Result<Func, Diagnostic> {
            while matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            self.func_with_modifiers(false, false)
        }
    
        /// D-PREPOST1: is the cursor at `#Pre(`/`#Post(`?
        pub(super) fn at_contract_clause_fn(&self, kw: &str) -> bool {
            matches!(self.peek().kind, TokKind::Hash)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == kw)
                && matches!(self.peek3().kind, TokKind::LParen)
        }
    
    
        /// D-WASM1=A: is the cursor at `#Target(Wasm|JS)`?
        pub(in crate::Parser) fn at_web_target(&self) -> bool {
            matches!(self.peek().kind, TokKind::Hash)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_TARGET)
                && matches!(self.peek3().kind, TokKind::LParen)
        }
    
        /// D-HTMLPAIR1 (ratified 2026-07-01, c134): detect `#HTML(`.
        pub(super) fn at_html_marker(&self) -> bool {
            matches!(self.peek().kind, TokKind::Hash)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_HTML)
                && matches!(self.peek3().kind, TokKind::LParen)
        }
    
        /// D-HTMLPAIR1 (ratified 2026-07-01, c134): parse `#HTML("path.html")` — the file's
        /// explicit companion host page for `--target=web` builds.
        pub(super) fn parse_html_marker(
            &mut self,
        ) -> Result<(crate::AST::Marker, Option<String>), Diagnostic> {
            let marker = self.parse_rule_marker()?;
            self.bind_rule_fact(
                marker.name_span,
                None,
                crate::Policy::RuleSite::File,
            );
            let path = self.html_from_marker(&marker)?;
            Ok((marker, path))
        }

        pub(super) fn html_from_marker(
            &self,
            marker: &crate::AST::Marker,
        ) -> Result<Option<String>, Diagnostic> {
            let arguments = self.bound_registered_rule_arguments(marker)?;
            let Some(path) = arguments.parameter(0) else {
                return Err(crate::Policy::marker_argument_shape_error(Syntax::ATTR_HTML, marker.span));
            };
            match path {
                crate::AST::Expr::Str(parts, _) if parts.len() == 1 => match &parts[0] {
                    crate::AST::StrPart::Lit(s) => Ok(Some(s.clone())),
                    crate::AST::StrPart::Interp(..) => Ok(None),
                },
                _ => Ok(None),
            }
        }
    
        /// D-WASM1=A / D-WEBDEFAULT1 (ratified 2026-07-01, c134): `#Target(Wasm)` / `#Target(JS)`
        /// (a partition ceiling) or `#Target(Web)` (this file's default CLI
        /// backend — a different axis, same marker).
        pub(in crate::Parser) fn parse_web_target_marker(&mut self) -> Result<TargetMarker, Diagnostic> {
            let marker = self.parse_rule_marker()?;
            self.web_target_from_marker(&marker)
        }

        pub(super) fn web_target_from_marker(&self, marker: &crate::AST::Marker) -> Result<TargetMarker, Diagnostic> {
            let arguments = self.bound_registered_rule_arguments(marker)?;
            let Some(target) = arguments.parameter(0) else {
                return Err(crate::Policy::marker_argument_shape_error(
                    Syntax::ATTR_TARGET,
                    marker.span,
                ));
            };
            let Some(name) = Self::marker_enum_path(target, "Target") else {
                return Err(crate::Policy::marker_argument_shape_error(
                    Syntax::ATTR_TARGET,
                    marker.span,
                ));
            };
            // D-OSTARGET1=A: `#Target(OS.Linux|OS.MacOS|OS.Windows)` — the second,
            // mutually-exclusive axis (native platform gating on an `impl`).
            if let Some(os_name) = name.strip_prefix("OS.") {
                return crate::Syntax::OSTarget::parse(os_name)
                    .map(TargetMarker::OS)
                    .ok_or_else(|| {
                        Diagnostic::error(
                            "E0003",
                            format!("`#Target(OS.{os_name})` is not a known native OS"),
                            "native OS targets are `OS.Linux`, `OS.MacOS`, or `OS.Windows`"
                                .to_string(),
                            format!(
                                "write `#Target(OS.{})`, `#Target(OS.{})`, or `#Target(OS.{})`",
                                Syntax::TARGET_OS_LINUX,
                                Syntax::TARGET_OS_MACOS,
                                Syntax::TARGET_OS_WINDOWS,
                            ),
                            Some(target.span()),
                        )
                    });
            }
            if name == crate::Syntax::WEB_TARGET_DEFAULT_WEB {
                return Ok(TargetMarker::DefaultWeb);
            }
            crate::Syntax::WebBucket::parse(&name)
                .map(TargetMarker::Bucket)
                .ok_or_else(|| {
                    Diagnostic::error(
                    "E0003",
                        format!("`#Target({name})` is not a known web partition"),
                    "web targets are `Wasm` (compute), `JS` (DOM/view), `Web` (default CLI backend), or `OS.Linux`/`OS.MacOS`/`OS.Windows` (native platform gating)"
                        .to_string(),
                    format!(
                        "write `#Target({})`, `#Target({})`, `#Target({})`, or `#Target(OS.{{Linux|MacOS|Windows}})`",
                        Syntax::WEB_BUCKET_WASM,
                        Syntax::WEB_BUCKET_JS,
                        Syntax::WEB_TARGET_DEFAULT_WEB,
                    ),
                    Some(target.span()),
                )
            })
        }
    
    
    


        /// E0925: `#Every(…)` without the `#Job` marker it schedules.
        pub(super) fn e0925_every_without_task(span: Span) -> Diagnostic {
            Diagnostic::error(
                "E0925",
                "`#Every(…)` needs `#Job` on the same function".to_string(),
                "a schedule only means something on a task — `#Every(…)` names when `#Job` \
                 runs, it isn't a standalone timer."
                    .to_string(),
                "add `#Job` (`#Job #Every(5min) fn …`), or drop `#Every(…)` if this isn't a \
                 scheduled task."
                    .to_string(),
                Some(span),
            )
        }

}
