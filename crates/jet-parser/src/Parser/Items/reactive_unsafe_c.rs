impl<'a> Parser<'a> {
        /// D-REACTCORE1: is the cursor at `#Reactive fn …` or `#Reactive pub fn …`?
        pub(crate) fn at_reactive_fn(&self) -> bool {
            matches!(self.peek().kind, TokKind::Hash)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_REACTIVE)
                && matches!(self.peek3().kind, TokKind::KwFn | TokKind::KwPub)
        }
    
        /// D-WASM1=A: is the cursor at `#Wasm fn` / `#Js fn` / `#WasmExport fn`?
        pub(crate) fn at_web_partition_fn(&self) -> bool {
            if !matches!(self.peek().kind, TokKind::Hash) {
                return false;
            }
            let is_marker = matches!(
                &self.peek2().kind,
                TokKind::Ident(n)
                    if n == Syntax::ATTR_WASM
                        || n == Syntax::ATTR_JS
                        || n == Syntax::ATTR_WASM_EXPORT
            );
            if !is_marker {
                return false;
            }
            self.token_after_web_marker_is_fn(2)
        }
    
        /// True when `fn` / `pub fn` follows a web partition marker, allowing a line break.
        fn token_after_web_marker_is_fn(&self, start: usize) -> bool {
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
    
        /// D-REACTCORE1 (ratified 2026-06-27, opt D): parse `#Reactive fn …`. The body
        /// lowers to a reactive effect scope at codegen; sema requires a unit return.
        pub(crate) fn reactive_fn(&mut self) -> Result<Func, Diagnostic> {
            self.expect(TokKind::Hash, "before `Reactive`")?;
            self.expect_ident(&format!("`#{}`", Syntax::KW_REACTIVE))?;
            let (is_pub, is_package_pub) = self.parse_item_visibility();
            self.expect_kw(TokKind::KwFn, "after `#Reactive`")?;
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
    
        /// D-UNSAFE2: is the cursor at `#Unsafe fn …` or `#Unsafe("…") fn …`?
        fn at_unsafe_fn(&self) -> bool {
            if !matches!(self.peek().kind, TokKind::Hash) {
                return false;
            }
            if !matches!(self.peek2().kind, TokKind::KwUnsafe) {
                return false;
            }
            // `#Unsafe fn` or `#Unsafe pub fn` (no reason arg)
            if matches!(self.peek3().kind, TokKind::KwFn | TokKind::KwPub) {
                return true;
            }
            // `#Unsafe("…") fn` or `#Unsafe("…") pub fn`
            // tokens (same line): `#`[0] `Unsafe`[1] `(`[2] `"str"`[3] `)`[4] `fn`/`pub`[5]
            // tokens (split line): `#`[0] `Unsafe`[1] `(`[2] `"str"`[3] `)`[4] `;`[5] `fn`/`pub`[6]
            // S6-R inserts a synthetic `;` after `)` when the reason and `fn` are on separate lines.
            if matches!(self.peek3().kind, TokKind::LParen) {
                let after_close = if matches!(self.peek6().kind, TokKind::Semi) {
                    &self.peek7().kind
                } else {
                    &self.peek6().kind
                };
                return matches!(after_close, TokKind::KwFn | TokKind::KwPub);
            }
            false
        }
    
        /// D-UNSAFE2 (ratified 2026-06-22, opt B): parse `#Unsafe("reason") fn …`
        /// or bare `#Unsafe fn …` (reason-less; L3101 fires in sema). The body is
        /// checked like any other fn; the contract is enforced at call sites (E3103).
        fn unsafe_fn(&mut self) -> Result<Func, Diagnostic> {
            let start = self.peek().span;
            self.expect(TokKind::Hash, "before `Unsafe`")?;
            self.expect_kw(TokKind::KwUnsafe, "to mark a whole-function contract")?;
            let marker_span = Span::new(start.start, self.toks[self.pos - 1].span.end);
            // Optional `("reason")` argument.
            let mut reason = None;
            if matches!(self.peek().kind, TokKind::LParen) {
                self.bump(); // `(`
                let (value, _) = self.expect_plain_string(
                    "for the safety reason",
                    "`#Unsafe` takes one piece of quoted text explaining why the function is safe to call",
                    "write: #Unsafe(\"caller must ensure …\") fn …",
                )?;
                reason = Some(value);
                self.expect(TokKind::RParen, "after the safety reason")?;
                // S6-R: when `#Unsafe("reason")` is on its own line above `fn`,
                // the lexer inserts a synthetic `;` after `)`. Skip it.
                if matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                }
            }
            let (is_pub, is_package_pub) = self.parse_item_visibility();
            self.expect_kw(TokKind::KwFn, "after `#Unsafe`")?;
            self.func_after_fn(
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
            )
        }
    
        /// S59 (E2-M14): is the cursor at the start of a C FFI module — `#Extern
        /// module …` or `#Bindgen module …`? Retired lowercase markers are also
        /// recognized here so E0060 can recover to the canonical form.
        fn at_c_module(&self) -> bool {
            if !matches!(self.peek().kind, TokKind::Hash) {
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
    
        fn at_retired_at_c_module(&self) -> bool {
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
    
        /// S59 (E2-M14): parse `#Extern module c.<lib> { … }` (overlay) or
        /// `#Bindgen module c.<lib>.__bindgen__ { … }` (generated cache). Body
        /// declarations share the `extern_fn` shape (`fn name(args) -> T = "Sym";`).
        fn c_module(&mut self) -> Result<crate::AST::CModule, Diagnostic> {
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
                    let old = format!("#{}", Syntax::ATTR_BINDGEN_RETIRED);
                    self.diags
                        .push(self.retired_c_module_marker_diag(&old, "#Bindgen", span));
                    CModuleKind::Bindgen
                }
                other => {
                    return Err(Diagnostic::error(
                        "E0003",
                        format!(
                            "expected `{}` or `{}` after `#`, found {}",
                            Syntax::ATTR_EXTERN_MODULE,
                            Syntax::ATTR_BINDGEN,
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
    
        fn retired_at_c_module(&mut self) -> Result<crate::AST::CModule, Diagnostic> {
            use crate::AST::CModuleKind;
            let start = self.bump().span; // `@`
            let kind = match &self.peek().kind {
                TokKind::KwExtern => {
                    let span = Span::new(start.start, self.bump().span.end);
                    let old = format!("@{}", "extern");
                    self.diags
                        .push(self.retired_c_module_marker_diag(&old, "#Extern", span));
                    CModuleKind::Extern
                }
                TokKind::Ident(n) if n == Syntax::ATTR_BINDGEN_RETIRED => {
                    let span = Span::new(start.start, self.bump().span.end);
                    let old = format!("@{}", Syntax::ATTR_BINDGEN_RETIRED);
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
                "C FFI markers are PascalCase `#` markers so generated and hand-written bindings share one marker family"
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
                        Syntax::C_MODULE_ROOT, Syntax::C_BINDGEN_SEGMENT, Syntax::ATTR_EXTERN_MODULE, Syntax::C_MODULE_ROOT
                    ),
                    format!(
                        "drop `{}` from your module path, or use `#{} module {}.{} {{ … }}`",
                        Syntax::C_BINDGEN_SEGMENT, Syntax::ATTR_EXTERN_MODULE, Syntax::C_MODULE_ROOT, lib
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
    
        pub(super) fn expect_plain_string(
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
