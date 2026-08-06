use super::super::{Diagnostic, Func, Parser, Span, StrTokPart, Syntax, TokKind, describe};

impl<'a> Parser<'a> {
    
    
    
    


        pub(super) fn ffi_fn_from_markers(
            &mut self,
            markers: Vec<crate::AST::Marker>,
        ) -> Result<Func, Diagnostic> {
            let decl_start = markers
                .first()
                .map_or(self.peek().span.start, |marker| marker.span.start);
            let Some(ffi) = markers.iter().find(|marker| marker.name == Syntax::MARKER_FFI) else {
                return Err(crate::Policy::marker_argument_shape_error(
                    Syntax::MARKER_FFI,
                    markers.first().map_or(self.peek().span, |marker| marker.span),
                ));
            };
            let arguments = self.bound_registered_rule_arguments(ffi)?;
            let Some(crate::AST::Expr::Ident(lang, lang_span)) = arguments.parameter(0) else {
                return Err(crate::Policy::marker_argument_shape_error(Syntax::MARKER_FFI, ffi.span));
            };
            let lang = lang.clone();
            let lang_span = *lang_span;
            let marker_span = ffi.span;
            // A synthetic `;` may separate the marker line from `fn`/`pub`.
            while matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            let (is_pub, is_package_pub) = self.parse_item_visibility();
            self.expect_kw(TokKind::KwFn, "after `#FFI(<lang>)`")?;

            // Ordinary Jet signature: name, type params, parameter list, optional
            // `=[effects]=> T` or plain `=> T`. Reuses the same sub-parsers as a
            // normal `fn` so the checked contract is identical.
            let (name, name_span) = self.expect_ident("after `fn`")?;
            let type_params = self.parse_opt_type_params()?;
            self.expect(TokKind::LParen, "after the function name")?;
            let params = self.parse_param_list()?;
            self.validate_variadic_params(&params);
            self.validate_param_labels(&params);
            let (declared_effects, effect_via) = self.parse_opt_func_effects()?;
            let decorated_arrow = declared_effects.is_some() || effect_via.is_some();
            let mut return_type = None;
            let mut return_type_span = None;
            if decorated_arrow
                || matches!(self.peek().kind, TokKind::LambdaArrow | TokKind::Arrow)
            {
                if !decorated_arrow {
                    let arrow = self.bump();
                    if matches!(arrow.kind, TokKind::Arrow) {
                        self.diags.push(Self::retired_callable_arrow(arrow.span));
                    }
                }
                if self.type_starts_here() {
                    let (ty, span) = self.return_type()?;
                    return_type = Some(ty);
                    return_type_span = Some(span);
                }
            }

            // The body is a single foreign-source string literal, not a Jet block.
            self.expect(TokKind::LBrace, "to open the `#FFI` foreign-source body")?;
            while matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            let (source, source_span) = self.expect_inline_foreign_source(&lang, marker_span)?;
            // The lexer inserts a synthetic `;` statement terminator after the
            // string; skip it before the closing brace.
            while matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            self.expect(TokKind::RBrace, "to close the `#FFI` foreign-source body")?;
            let declaration_end = self.toks[self.pos - 1].span.end;

            let function = Func {
                span: Span::new(decl_start, declaration_end),
                is_pub,
                is_package_pub,
                external_type: None,
                name,
                name_span,
                meta: None,
                type_params,
                params,
                return_type,
                return_type_span,
                return_view_provenance: None,
                declared_return_view_provenance: None,
            gc_return: false,
            gc_scope: false,
                is_unsafe: false,
                unsafe_reason: None,
                unsafe_span: None,
                is_pure: false,
                is_sanitizer: false,
                scrub_tag: None,
                is_reactive: false,
                reactive_upgrades: Vec::new(),
                is_replayable: false,
                replayable_span: None,
                is_task: false,
                task_span: None,
                every: None,
                task_metadata: None,
                declared_effects,
                effect_via,
                state_requires: None,
                state_transition: None,
                web_marker: None,
                is_must_use: false,
                must_use_span: None,
                maturity: None,
                maturity_span: None,
                kernel: None,
                is_inline: false,
                is_inline_always: false,
                inline_span: None,
                pre: Vec::new(),
                post: Vec::new(),
                inline_foreign: Some(crate::AST::InlineForeign {
                    lang,
                    lang_span,
                    marker_span,
                    source,
                    source_span,
                }),
                markers: Vec::new(),
                body: Vec::new(),
            };
            self.apply_function_markers(function, markers)
        }

        /// D-FFI-INLINE1=A (card #501): read the single foreign-source string that
        /// forms a `#FFI(<lang>) fn` body. Must be exactly one non-interpolated
        /// string literal (`"""…"""` or `"…"`); anything else — an interpolation,
        /// a Jet statement, or an empty block — is E0064.
        fn expect_inline_foreign_source(
            &mut self,
            lang: &str,
            marker_span: Span,
        ) -> Result<(String, Span), Diagnostic> {
            let e0064 = |span: Span| {
                Diagnostic::error(
                    "E0064",
                    format!("a `#FFI({lang})` function body must be one string of {lang} source"),
                    "the inline foreign tier carries the foreign source as a single `\"\"\"…\"\"\"` string; the Jet signature above is the checked contract (D-FFI-INLINE1)".to_string(),
                    format!("write the body as `{{ \"\"\"<{lang} source>\"\"\" }}` — one string literal, no other statements or interpolation"),
                    Some(span),
                )
            };
            match &self.peek().kind {
                TokKind::Str(parts) => {
                    let parts = parts.clone();
                    let span = self.bump().span;
                    match parts.as_slice() {
                        [StrTokPart::Lit(s)] => Ok((s.clone(), span)),
                        _ => Err(e0064(span)),
                    }
                }
                _ => Err(e0064(marker_span)),
            }
        }

    
    
        /// S59 (E2-M14): is the cursor at the start of a C FFI module — `#Extern
        /// module …` or `#Bindgen module …`? Retired lowercase markers are also
        /// recognized here so E0060 can recover to the canonical form.
        pub(super) fn at_c_module(&self) -> bool {
            if !matches!(self.peek().kind, TokKind::Hash) {
                return false;
            }
            let intro_is_c = match &self.peek2().kind {
                TokKind::KwExtern => true,
                TokKind::Ident(n) => {
                    n == Syntax::MARKER_EXTERN_MODULE
                        || n == Syntax::MARKER_BINDGEN
                        || n == Syntax::MARKER_BINDGEN_RETIRED
                }
                _ => false,
            };
            intro_is_c && matches!(self.peek3().kind, TokKind::KwModule)
        }
    
        pub(super) fn at_retired_at_c_module(&self) -> bool {
            if !matches!(self.peek().kind, TokKind::Hash) {
                return false;
            }
            let intro_is_c = match &self.peek2().kind {
                TokKind::KwExtern => true,
                TokKind::Ident(n) => n == Syntax::MARKER_BINDGEN_RETIRED,
                _ => false,
            };
            intro_is_c && matches!(self.peek3().kind, TokKind::KwModule)
        }
    
        /// S59 (E2-M14): parse `#Extern module c.<lib> { … }` (overlay) or
        /// `#Bindgen module c.<lib>.__bindgen__ { … }` (generated cache). Body
        /// declarations share the `extern_fn` shape (`fn name(args) => T = "Sym";`).
        pub(super) fn c_module(&mut self) -> Result<crate::AST::CModule, Diagnostic> {
            use crate::AST::CModuleKind;
            let start = self.bump().span; // `#`
            let kind = match &self.peek().kind {
                TokKind::KwExtern => {
                    let span = Span::new(start.start, self.bump().span.end);
                    let old = format!("#{}", "extern");
                    self.diags
                        .push(self.retired_c_module_marker_diag(&old, "#Extern", span));
                    CModuleKind::Extern
                }
                TokKind::Ident(n) if n == Syntax::MARKER_EXTERN_MODULE => {
                    self.bump();
                    CModuleKind::Extern
                }
                TokKind::Ident(n) if n == Syntax::MARKER_BINDGEN => {
                    self.bump();
                    CModuleKind::Bindgen
                }
                TokKind::Ident(n) if n == Syntax::MARKER_BINDGEN_RETIRED => {
                    let span = Span::new(start.start, self.bump().span.end);
                    let old = format!("#{}", Syntax::MARKER_BINDGEN_RETIRED);
                    self.diags
                        .push(self.retired_c_module_marker_diag(&old, "#Bindgen", span));
                    CModuleKind::Bindgen
                }
                other => {
                    return Err(Diagnostic::error(
                        "E0003",
                        format!(
                            "expected `{}` or `{}` after `@`, found {}",
                            Syntax::MARKER_EXTERN_MODULE,
                            Syntax::MARKER_BINDGEN,
                            describe(other)
                        ),
                        "a C FFI module begins with `#Extern module c.<lib>` or `#Bindgen module c.<lib>.__bindgen__`".to_string(),
                        "write: #Extern module c.raylib { fn init_window(w: Int, h: Int, title: String) = \"InitWindow\"; }".to_string(),
                        Some(self.peek().span),
                    ));
                }
            };
            self.c_module_after_kind(start, kind)
        }
    
        pub(super) fn retired_at_c_module(&mut self) -> Result<crate::AST::CModule, Diagnostic> {
            use crate::AST::CModuleKind;
            let start = self.bump().span; // `@`
            let kind = match &self.peek().kind {
                TokKind::KwExtern => {
                    let span = Span::new(start.start, self.bump().span.end);
                    let old = format!("#{}", "extern");
                    self.diags
                        .push(self.retired_c_module_marker_diag(&old, "#Extern", span));
                    CModuleKind::Extern
                }
                TokKind::Ident(n) if n == Syntax::MARKER_BINDGEN_RETIRED => {
                    let span = Span::new(start.start, self.bump().span.end);
                    let old = format!("#{}", Syntax::MARKER_BINDGEN_RETIRED);
                    self.diags
                        .push(self.retired_c_module_marker_diag(&old, "#Bindgen", span));
                    CModuleKind::Bindgen
                }
                _ => unreachable!("at_retired_at_c_module guards marker spelling"),
            };
            self.c_module_after_kind(start, kind)
        }
    
        fn retired_c_module_marker_diag(&self, old: &str, new: &str, span: Span) -> Diagnostic {
            Diagnostic::error(
                "E0060",
                format!("C FFI modules use `{}`, not `{}`", new, old),
                "C FFI rules use the one PascalCase `#Rule` family in generated and hand-written bindings"
                    .to_string(),
                format!("write `{}` before `module c.<lib>`", new),
                Some(span),
            )
        }
    
        fn c_module_after_kind(
            &mut self,
            start: Span,
            kind: crate::AST::CModuleKind,
        ) -> Result<crate::AST::CModule, Diagnostic> {
            use crate::AST::CModuleKind;
            self.expect_kw(TokKind::KwModule, "to declare a C FFI module")?;
    
            // Parse the dotted module path: `c` `.` `<lib>` [ `.` `__bindgen__` ].
            let path_start = self.peek().span;
            let (root, _) = self.expect_ident("after `module`")?;
            if root != Syntax::C_MODULE_ROOT {
                return Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "a C FFI module path starts with `{}.`, found `{}`",
                        Syntax::C_MODULE_ROOT,
                        root
                    ),
                    "C libraries live under the `c.` module root — `c.raylib`, `c.sqlite3`".to_string(),
                    format!(
                        "write: {} module {}.<lib> {{ … }}",
                        match kind {
                            CModuleKind::Extern => "#Extern",
                            CModuleKind::Bindgen => "#Bindgen",
                        },
                        Syntax::C_MODULE_ROOT
                    ),
                    Some(path_start),
                ));
            }
            self.expect(TokKind::Dot, "after `c` in a C FFI module path")?;
            let (lib, lib_span) = self.expect_ident("for the C library name")?;
            let mut has_bindgen_seg = false;
            let mut path_end = lib_span.end;
            if matches!(self.peek().kind, TokKind::Dot) {
                self.bump();
                let (seg, seg_span) = self.expect_ident("after `.` in a C FFI module path")?;
                path_end = seg_span.end;
                if seg == Syntax::C_BINDGEN_SEGMENT {
                    has_bindgen_seg = true;
                } else {
                    return Err(Diagnostic::error(
                        "E0003",
                        format!("a C FFI module path can't have a `.{}` segment", seg),
                        "the only legal third segment is the reserved `__bindgen__` on a generated cache module".to_string(),
                        format!("write: #Extern module {}.{} {{ … }}", Syntax::C_MODULE_ROOT, lib),
                        Some(seg_span),
                    ));
                }
            }
            let path_span = Span::new(path_start.start, path_end);
    
            // E3206: a user overlay must not name the reserved `__bindgen__` segment.
            if kind == CModuleKind::Extern && has_bindgen_seg {
                return Err(Diagnostic::error(
                    "E3206",
                    format!(
                        "module path `{}.{}.{}` uses the reserved segment `{}`",
                        Syntax::C_MODULE_ROOT, lib, Syntax::C_BINDGEN_SEGMENT, Syntax::C_BINDGEN_SEGMENT
                    ),
                    format!(
                        "autogen lives in `{}.<lib>.{}`; users declare overlays as `#{} module {}.<lib>` only",
                        Syntax::C_MODULE_ROOT, Syntax::C_BINDGEN_SEGMENT, Syntax::MARKER_EXTERN_MODULE, Syntax::C_MODULE_ROOT
                    ),
                    format!(
                        "drop `{}` from your module path, or use `#{} module {}.{} {{ … }}`",
                        Syntax::C_BINDGEN_SEGMENT, Syntax::MARKER_EXTERN_MODULE, Syntax::C_MODULE_ROOT, lib
                    ),
                    Some(path_span),
                ));
            }
            // A `#Bindgen` module must carry the `__bindgen__` segment (it is the
            // generated surface). Without it the path is malformed.
            if kind == CModuleKind::Bindgen && !has_bindgen_seg {
                return Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "a `#Bindgen` module path must end in `.{}`",
                        Syntax::C_BINDGEN_SEGMENT
                    ),
                    "the compiler generates `#Bindgen module c.<lib>.__bindgen__` cache files"
                        .to_string(),
                    format!(
                        "write: #Bindgen module {}.{}.{} {{ … }}",
                        Syntax::C_MODULE_ROOT,
                        lib,
                        Syntax::C_BINDGEN_SEGMENT
                    ),
                    Some(path_span),
                ));
            }
    
            self.expect(TokKind::LBrace, "to open the C FFI module body")?;
            let mut functions = Vec::new();
            while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
                if matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                    continue;
                }
                functions.push(self.extern_fn()?);
            }
            self.expect(TokKind::RBrace, "to close the C FFI module body")?;
            let end = self.toks[self.pos - 1].span.end;
            Ok(crate::AST::CModule {
                kind,
                lib,
                path_span,
                functions,
                span: Span::new(start.start, end),
            })
        }
    
        pub(in crate::Parser) fn expect_plain_string(
            &mut self,
            context: &str,
            why_interp: &str,
            fix: &str,
        ) -> Result<(String, Span), Diagnostic> {
            let parts = match &self.peek().kind {
                TokKind::Str(parts) => parts.clone(),
                other => {
                    return Err(Diagnostic::error(
                        "E0003",
                        format!(
                            "expected a piece of quoted text {}, found {}",
                            context,
                            describe(other)
                        ),
                        why_interp.to_string(),
                        fix.to_string(),
                        Some(self.peek().span),
                    ));
                }
            };
            let span = self.bump().span;
            if parts.len() != 1 {
                return Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "expected a piece of quoted text {}, found interpolation",
                        context
                    ),
                    why_interp.to_string(),
                    fix.to_string(),
                    Some(span),
                ));
            }
            match &parts[0] {
                StrTokPart::Lit(s) => Ok((s.clone(), span)),
                StrTokPart::Interp(_) => Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "expected a piece of quoted text {}, found interpolation",
                        context
                    ),
                    why_interp.to_string(),
                    fix.to_string(),
                    Some(span),
                )),
            }
        }
    
}
