use super::super::{Diagnostic, Func, Parser, Span, StrTokPart, Syntax, TokKind, describe};

impl<'a> Parser<'a> {
        /// D-REACTCORE1: is the cursor at `@Reactive fn …` or `@Reactive pub fn …`?
        pub(crate) fn at_reactive_fn(&self) -> bool {
            matches!(self.peek().kind, TokKind::At)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_REACTIVE)
                && matches!(self.peek3().kind, TokKind::KwFn | TokKind::KwPub)
        }
    
        /// D-MARK-TARGET1=A (ratified 2026-07-11, card #498): is the cursor at
        /// `@Target(Wasm) fn` / `@Target(Js) fn` (per-function bucket
        /// override, unified with the file/module ceiling spelling) or the
        /// untouched `@WasmExport fn`?
        pub(crate) fn at_web_partition_fn(&self) -> bool {
            if !matches!(self.peek().kind, TokKind::At) {
                return false;
            }
            // `@WasmExport fn` (untouched by D-MARK-TARGET1).
            if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_WASM_EXPORT) {
                return self.token_after_web_marker_is_fn(2);
            }
            // `@Target(Wasm) fn` / `@Target(Js) fn` per-function override.
            if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_TARGET)
                && matches!(self.peek3().kind, TokKind::LParen)
            {
                let is_bucket = matches!(
                    &self.peek4().kind,
                    TokKind::Ident(n) if n == Syntax::WEB_BUCKET_WASM || n == Syntax::WEB_BUCKET_JS
                );
                if is_bucket && matches!(self.peek5().kind, TokKind::RParen) {
                    return self.token_after_web_marker_is_fn(5);
                }
            }
            false
        }
    
        /// True when `fn` / `pub fn` follows a web partition marker, allowing a line break.
        pub(super) fn token_after_web_marker_is_fn(&self, start: usize) -> bool {
            let mut i = self.pos + start;
            while i < self.toks.len() {
                match &self.toks[i].kind {
                    TokKind::Semi => i += 1,
                    TokKind::KwFn => return true,
                    TokKind::KwPub => return true,
                    _ => return false,
                }
            }
            false
        }
    
        /// D-REACTCORE1 (ratified 2026-06-27, opt D): parse `@Reactive fn …`. The body
        /// lowers to a reactive effect scope at codegen; sema requires a unit return.
        pub(crate) fn reactive_fn(&mut self) -> Result<Func, Diagnostic> {
            self.expect(TokKind::At, "before `Reactive`")?;
            self.expect_ident(&format!("`@{}`", Syntax::KW_REACTIVE))?;
            let (is_pub, is_package_pub) = self.parse_item_visibility();
            self.expect_kw(TokKind::KwFn, "after `@Reactive`")?;
            self.func_after_fn(
                is_pub,
                is_package_pub,
                false,
                None,
                None,
                false,
                false,
                None,
                None,
                None,
                true,
                None,
                false,
                None,
                None,
                None,
                false,
                false,
                None,
                false,
                None,
            )
        }
    
        /// D-FFI-INLINE1=A (ratified 2026-07-11, card #501): is the cursor at
        /// `#FFI(<lang>) fn …`, optionally preceded by an `@Unsafe("reason")`
        /// gate (`@Unsafe("…") #FFI(asm) fn …`)? The unsafe-language gate is
        /// enforced in sema; the parser only needs to route the item here.
        pub(super) fn at_ffi_fn(&self) -> bool {
            // Direct `#FFI(<lang>)`.
            if matches!(self.peek().kind, TokKind::Hash)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_FFI)
                && matches!(self.peek3().kind, TokKind::LParen)
            {
                return true;
            }
            // `@Unsafe(["reason"]) #FFI(<lang>)` — scan past the unsafe gate.
            if matches!(self.peek().kind, TokKind::At)
                && matches!(self.peek2().kind, TokKind::KwUnsafe)
            {
                let mut i = self.pos + 2;
                if matches!(self.toks.get(i).map(|t| &t.kind), Some(TokKind::LParen)) {
                    while i < self.toks.len()
                        && !matches!(self.toks[i].kind, TokKind::RParen | TokKind::Eof)
                    {
                        i += 1;
                    }
                    i += 1; // past `)`
                }
                while matches!(self.toks.get(i).map(|t| &t.kind), Some(TokKind::Semi)) {
                    i += 1;
                }
                return matches!(self.toks.get(i).map(|t| &t.kind), Some(TokKind::Hash))
                    && matches!(self.toks.get(i + 1).map(|t| &t.kind), Some(TokKind::Ident(n)) if n == Syntax::ATTR_FFI)
                    && matches!(self.toks.get(i + 2).map(|t| &t.kind), Some(TokKind::LParen));
            }
            false
        }

        /// D-FFI-INLINE1=A (card #501): parse `#FFI(<lang>) fn name(sig) -> T {
        /// """<foreign source>""" }` (the inline foreign tier), optionally
        /// preceded by an `@Unsafe("reason")` gate. The Jet signature is parsed
        /// as an ordinary function signature; the body must be a single
        /// foreign-source string literal, captured into `Func::inline_foreign`
        /// (the statement body is left empty). Language validity and the
        /// unsafe-language gate are checked in sema (Names are validated in
        /// sema, not the parser).
        pub(super) fn ffi_fn(&mut self) -> Result<Func, Diagnostic> {
            let decl_start = self.peek().span.start;
            // Optional leading `@Unsafe("reason")` gate.
            let (is_unsafe, unsafe_reason, unsafe_span) = if matches!(self.peek().kind, TokKind::At)
                && matches!(self.peek2().kind, TokKind::KwUnsafe)
            {
                let start = self.peek().span;
                self.bump(); // `#`
                self.bump(); // `Unsafe`
                let marker_span = Span::new(start.start, self.toks[self.pos - 1].span.end);
                let mut reason = None;
                if matches!(self.peek().kind, TokKind::LParen) {
                    self.bump(); // `(`
                    let (value, _) = self.expect_plain_string(
                        "for the safety reason",
                        "`@Unsafe` takes one piece of quoted text explaining why the function is safe to call",
                        "write: @Unsafe(\"caller must ensure …\") #FFI(asm) fn …",
                    )?;
                    reason = Some(value);
                    self.expect(TokKind::RParen, "after the safety reason")?;
                    if matches!(self.peek().kind, TokKind::Semi) {
                        self.bump();
                    }
                }
                (true, reason, Some(marker_span))
            } else {
                (false, None, None)
            };

            // `#FFI(<lang>)`
            let ffi_start = self.peek().span;
            self.expect(TokKind::Hash, "before `FFI`")?;
            let ffi_ident_span = self.peek().span;
            match &self.peek().kind {
                TokKind::Ident(n) if n == Syntax::ATTR_FFI => {
                    self.bump();
                }
                other => {
                    return Err(Diagnostic::error(
                        "E0003",
                        format!("expected `{}` after `#`, found {}", Syntax::ATTR_FFI, describe(other)),
                        "the inline foreign tier is written `#FFI(<lang>) fn` — `#FFI(c)`, `#FFI(cpp)`, `#FFI(asm)`".to_string(),
                        "write: #FFI(c) fn name(...) -> T { \"\"\"<foreign source>\"\"\" }".to_string(),
                        Some(ffi_ident_span),
                    ));
                }
            }
            self.expect(TokKind::LParen, "after `#FFI` to name the foreign language")?;
            let (lang, lang_span) = self.expect_ident("for the foreign language name in `#FFI(<lang>)`")?;
            self.expect(TokKind::RParen, "after the foreign language name")?;
            let marker_span = Span::new(ffi_start.start, self.toks[self.pos - 1].span.end);
            // A synthetic `;` may separate the marker line from `fn`/`pub`.
            while matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            let (is_pub, is_package_pub) = self.parse_item_visibility();
            self.expect_kw(TokKind::KwFn, "after `#FFI(<lang>)`")?;

            // Ordinary Jet signature: name, type params, parameter list, optional
            // `--[effects]-> T` or plain `-> T`. Reuses the same sub-parsers as a
            // normal `fn` so the checked contract is identical.
            let (name, name_span) = self.expect_ident("after `fn`")?;
            let type_params = self.parse_opt_type_params()?;
            self.expect(TokKind::LParen, "after the function name")?;
            let mut params = Vec::new();
            if !matches!(self.peek().kind, TokKind::RParen) {
                loop {
                    params.push(self.param()?);
                    if matches!(self.peek().kind, TokKind::RParen) {
                        break;
                    }
                    self.expect(TokKind::Comma, "between parameters")?;
                }
            }
            self.expect(TokKind::RParen, "to close the parameter list")?;
            self.validate_variadic_params(&params);
            let (declared_effects, effect_via) = self.parse_opt_func_effects()?;
            let decorated_arrow = declared_effects.is_some() || effect_via.is_some();
            let mut return_type = None;
            let mut return_type_span = None;
            if decorated_arrow || matches!(self.peek().kind, TokKind::Arrow) {
                if !decorated_arrow {
                    self.bump();
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

            Ok(Func {
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
            gc_return: false,
            gc_scope: false,
                is_unsafe,
                unsafe_reason,
                unsafe_span,
                is_pure: false,
                is_sanitizer: false,
                is_reactive: false,
                is_replayable: false,
                replayable_span: None,
                is_task: false,
                task_span: None,
                every: None,
                declared_effects,
                effect_via,
                state_requires: None,
                state_transition: None,
                web_marker: None,
                is_must_use: false,
                must_use_span: None,
                maturity: None,
                maturity_span: None,
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
                body: Vec::new(),
            })
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

        /// D-UNSAFE2: is the cursor at `@Unsafe fn …` or `@Unsafe("…") fn …`?
        pub(super) fn at_unsafe_fn(&self) -> bool {
            if !matches!(self.peek().kind, TokKind::At) {
                return false;
            }
            if !matches!(self.peek2().kind, TokKind::KwUnsafe) {
                return false;
            }
            // `@Unsafe fn` or `@Unsafe pub fn` (no reason arg)
            if matches!(self.peek3().kind, TokKind::KwFn | TokKind::KwPub) {
                return true;
            }
            if matches!(self.peek3().kind, TokKind::LParen) {
                let mut depth = 0usize;
                let mut index = self.pos + 2;
                while let Some(token) = self.toks.get(index) {
                    match token.kind {
                        TokKind::LParen => depth += 1,
                        TokKind::RParen => { depth -= 1; if depth == 0 { index += 1; break; } }
                        _ => {}
                    }
                    index += 1;
                }
                if matches!(self.toks.get(index).map(|t| &t.kind), Some(TokKind::Semi)) { index += 1; }
                return matches!(self.toks.get(index).map(|t| &t.kind), Some(TokKind::KwFn | TokKind::KwPub));
            }
            false
        }
    
        /// D-UNSAFE2 (ratified 2026-06-22, opt B): parse `@Unsafe("reason") fn …`
        /// or bare `@Unsafe fn …` (reason-less; L3101 fires in sema). The body is
        /// checked like any other fn; the contract is enforced at call sites (E3103).
        pub(super) fn unsafe_fn(&mut self) -> Result<Func, Diagnostic> {
            let start = self.peek().span;
            self.expect(TokKind::At, "before `Unsafe`")?;
            self.expect_kw(TokKind::KwUnsafe, "to mark a whole-function contract")?;
            let marker_span = Span::new(start.start, self.toks[self.pos - 1].span.end);
            let mut reason = None;
            let mut obligation_mode = None;
            if matches!(self.peek().kind, TokKind::LParen) {
                self.bump(); // `(`
                if matches!(self.peek().kind, TokKind::Str(_)) {
                    let (value, _) = self.expect_plain_string(
                        "for the safety reason",
                        "`@Unsafe` takes quoted text explaining why the function is safe to call",
                        "write: @Unsafe(\"caller must ensure …\") fn …",
                    )?;
                    reason = Some(value);
                    if matches!(self.peek().kind, TokKind::Comma) { self.bump(); }
                }
                if !matches!(self.peek().kind, TokKind::RParen) {
                    let (field, field_span) = self.expect_ident("for the `@Unsafe` option")?;
                    if field != "obligations" { return Err(Diagnostic::error("E3108", format!("`{field}` is not an unsafe-gate option"), "per-site control has one typed field: `obligations`".to_string(), "write `obligations: .Track` or `obligations: .Skip`".to_string(), Some(field_span))); }
                    self.expect(TokKind::Colon, "after `obligations`")?;
                    self.expect(TokKind::Dot, "before the obligation mode")?;
                    let (mode, mode_span) = self.expect_ident("after `obligations: .`")?;
                    obligation_mode = Some(match mode.as_str() {
                        "Track" => crate::Policy::PolicyValue::UnsafeTrack,
                        "Skip" => crate::Policy::PolicyValue::UnsafeSkip,
                        _ => return Err(Diagnostic::error("E3108", format!("`.{mode}` is not a per-site obligation mode"), "a gate either tracks typed obligations or explicitly skips them when policy permits".to_string(), "write `.Track` or `.Skip`".to_string(), Some(mode_span))),
                    });
                }
                self.expect(TokKind::RParen, "after the safety reason")?;
                // S6-R: when `@Unsafe("reason")` is on its own line above `fn`,
                // the lexer inserts a synthetic `;` after `)`. Skip it.
                if matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                }
            }
            let (is_pub, is_package_pub) = self.parse_item_visibility();
            self.expect_kw(TokKind::KwFn, "after `@Unsafe`")?;
            let function = self.func_after_fn(
                is_pub,
                is_package_pub,
                true,
                reason,
                Some(marker_span),
                false,
                false,
                None,
                None,
                None,
                false,
                None,
                false,
                None,
                None,
                None,
                false,
                false,
                None,
                false,
                None,
            )?;
            if let Some(value) = obligation_mode {
                self.policy_declarations.push(crate::Policy::PolicyDeclaration {
                    key: crate::Policy::PolicyKey::Unsafe,
                    value,
                    scope: crate::Policy::PolicyScope::Function,
                    span: marker_span,
                    target: Some(function.span),
                    source: "<source>".to_string(),
                });
            }
            Ok(function)
        }
    
        /// S59 (E2-M14): is the cursor at the start of a C FFI module — `@Extern
        /// module …` or `@Bindgen module …`? Retired lowercase markers are also
        /// recognized here so E0060 can recover to the canonical form.
        pub(super) fn at_c_module(&self) -> bool {
            if !matches!(self.peek().kind, TokKind::At) {
                return false;
            }
            let intro_is_c = match &self.peek2().kind {
                TokKind::KwExtern => true,
                TokKind::Ident(n) => {
                    n == Syntax::ATTR_EXTERN_MODULE
                        || n == Syntax::ATTR_BINDGEN
                        || n == Syntax::ATTR_BINDGEN_RETIRED
                }
                _ => false,
            };
            intro_is_c && matches!(self.peek3().kind, TokKind::KwModule)
        }
    
        pub(super) fn at_retired_at_c_module(&self) -> bool {
            if !matches!(self.peek().kind, TokKind::At) {
                return false;
            }
            let intro_is_c = match &self.peek2().kind {
                TokKind::KwExtern => true,
                TokKind::Ident(n) => n == Syntax::ATTR_BINDGEN_RETIRED,
                _ => false,
            };
            intro_is_c && matches!(self.peek3().kind, TokKind::KwModule)
        }
    
        /// S59 (E2-M14): parse `@Extern module c.<lib> { … }` (overlay) or
        /// `@Bindgen module c.<lib>.__bindgen__ { … }` (generated cache). Body
        /// declarations share the `extern_fn` shape (`fn name(args) -> T = "Sym";`).
        pub(super) fn c_module(&mut self) -> Result<crate::AST::CModule, Diagnostic> {
            use crate::AST::CModuleKind;
            let start = self.bump().span; // `#`
            let kind = match &self.peek().kind {
                TokKind::KwExtern => {
                    let span = Span::new(start.start, self.bump().span.end);
                    let old = format!("@{}", "extern");
                    self.diags
                        .push(self.retired_c_module_marker_diag(&old, "@Extern", span));
                    CModuleKind::Extern
                }
                TokKind::Ident(n) if n == Syntax::ATTR_EXTERN_MODULE => {
                    self.bump();
                    CModuleKind::Extern
                }
                TokKind::Ident(n) if n == Syntax::ATTR_BINDGEN => {
                    self.bump();
                    CModuleKind::Bindgen
                }
                TokKind::Ident(n) if n == Syntax::ATTR_BINDGEN_RETIRED => {
                    let span = Span::new(start.start, self.bump().span.end);
                    let old = format!("@{}", Syntax::ATTR_BINDGEN_RETIRED);
                    self.diags
                        .push(self.retired_c_module_marker_diag(&old, "@Bindgen", span));
                    CModuleKind::Bindgen
                }
                other => {
                    return Err(Diagnostic::error(
                        "E0003",
                        format!(
                            "expected `{}` or `{}` after `@`, found {}",
                            Syntax::ATTR_EXTERN_MODULE,
                            Syntax::ATTR_BINDGEN,
                            describe(other)
                        ),
                        "a C FFI module begins with `@Extern module c.<lib>` or `@Bindgen module c.<lib>.__bindgen__`".to_string(),
                        "write: @Extern module c.raylib { fn init_window(w: Int, h: Int, title: String) = \"InitWindow\"; }".to_string(),
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
                    let old = format!("@{}", "extern");
                    self.diags
                        .push(self.retired_c_module_marker_diag(&old, "@Extern", span));
                    CModuleKind::Extern
                }
                TokKind::Ident(n) if n == Syntax::ATTR_BINDGEN_RETIRED => {
                    let span = Span::new(start.start, self.bump().span.end);
                    let old = format!("@{}", Syntax::ATTR_BINDGEN_RETIRED);
                    self.diags
                        .push(self.retired_c_module_marker_diag(&old, "@Bindgen", span));
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
                "C FFI rules use the one PascalCase `@Rule` family in generated and hand-written bindings"
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
                            CModuleKind::Extern => "@Extern",
                            CModuleKind::Bindgen => "@Bindgen",
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
                        format!("write: @Extern module {}.{} {{ … }}", Syntax::C_MODULE_ROOT, lib),
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
                        "autogen lives in `{}.<lib>.{}`; users declare overlays as `@{} module {}.<lib>` only",
                        Syntax::C_MODULE_ROOT, Syntax::C_BINDGEN_SEGMENT, Syntax::ATTR_EXTERN_MODULE, Syntax::C_MODULE_ROOT
                    ),
                    format!(
                        "drop `{}` from your module path, or use `@{} module {}.{} {{ … }}`",
                        Syntax::C_BINDGEN_SEGMENT, Syntax::ATTR_EXTERN_MODULE, Syntax::C_MODULE_ROOT, lib
                    ),
                    Some(path_span),
                ));
            }
            // A `@Bindgen` module must carry the `__bindgen__` segment (it is the
            // generated surface). Without it the path is malformed.
            if kind == CModuleKind::Bindgen && !has_bindgen_seg {
                return Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "a `@Bindgen` module path must end in `.{}`",
                        Syntax::C_BINDGEN_SEGMENT
                    ),
                    "the compiler generates `@Bindgen module c.<lib>.__bindgen__` cache files"
                        .to_string(),
                    format!(
                        "write: @Bindgen module {}.{}.{} {{ … }}",
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
