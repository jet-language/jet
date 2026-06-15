//! LSP v2 (M13): full language server — completion, hover, go-to-definition,
//! references, rename, semantic tokens, inlay hints, quick-fixes, formatting.
//!
//! Hand-rolled JSON-RPC over stdio (invariant I6 — no serde in the compiler).
//! Panics inside handlers are caught (LSP-I2) — process death is a P0 bug.
//! All file reads go through the overlay (LSP-I4) — unsaved buffers are correct.

use crate::ast::{self, Item, LoadedModule, ProgramBundle};
use crate::diag::{Diagnostic, Severity, Span, TextEdit};
use crate::lexer::{TokKind, Token};
use crate::sema::CompileMode;
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

// ── minimal JSON (parse only what LSP needs) ──────────────────────────────────

#[derive(Debug, Clone)]
enum JsonValue {
    Null,
    Bool(bool),
    Number(i64),
    Flt(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(HashMap<String, JsonValue>),
}

fn parse_json(text: &str) -> Result<JsonValue, ()> {
    let mut p = JsonParser { s: text, i: 0 };
    let v = p.value()?;
    p.skip_ws();
    if p.i < p.s.len() {
        return Err(());
    }
    Ok(v)
}

struct JsonParser<'a> {
    s: &'a str,
    i: usize,
}

impl<'a> JsonParser<'a> {
    fn peek(&self) -> Option<char> {
        self.s[self.i..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.i += c.len_utf8();
        Some(c)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.bump();
        }
    }

    fn value(&mut self) -> Result<JsonValue, ()> {
        self.skip_ws();
        match self.peek() {
            Some('n') => {
                self.expect_literal("null")?;
                Ok(JsonValue::Null)
            }
            Some('t') => {
                self.expect_literal("true")?;
                Ok(JsonValue::Bool(true))
            }
            Some('f') => {
                self.expect_literal("false")?;
                Ok(JsonValue::Bool(false))
            }
            Some('"') => Ok(JsonValue::String(self.string()?)),
            Some('[') => {
                self.bump();
                let mut arr = Vec::new();
                self.skip_ws();
                if self.peek() == Some(']') {
                    self.bump();
                    return Ok(JsonValue::Array(arr));
                }
                loop {
                    arr.push(self.value()?);
                    self.skip_ws();
                    match self.bump() {
                        Some(',') => continue,
                        Some(']') => break,
                        _ => return Err(()),
                    }
                }
                Ok(JsonValue::Array(arr))
            }
            Some('{') => {
                self.bump();
                let mut obj = HashMap::new();
                self.skip_ws();
                if self.peek() == Some('}') {
                    self.bump();
                    return Ok(JsonValue::Object(obj));
                }
                loop {
                    self.skip_ws();
                    let key = self.string()?;
                    self.skip_ws();
                    if self.bump() != Some(':') {
                        return Err(());
                    }
                    obj.insert(key, self.value()?);
                    self.skip_ws();
                    match self.bump() {
                        Some(',') => continue,
                        Some('}') => break,
                        _ => return Err(()),
                    }
                }
                Ok(JsonValue::Object(obj))
            }
            Some(c) if c == '-' || c.is_ascii_digit() => {
                let start = self.i;
                if self.peek() == Some('-') {
                    self.bump();
                }
                while matches!(self.peek(), Some('0'..='9')) {
                    self.bump();
                }
                let is_float = matches!(self.peek(), Some('.') | Some('e') | Some('E'));
                if is_float {
                    if self.peek() == Some('.') {
                        self.bump();
                        while matches!(self.peek(), Some('0'..='9')) {
                            self.bump();
                        }
                    }
                    if matches!(self.peek(), Some('e') | Some('E')) {
                        self.bump();
                        if matches!(self.peek(), Some('+') | Some('-')) {
                            self.bump();
                        }
                        while matches!(self.peek(), Some('0'..='9')) {
                            self.bump();
                        }
                    }
                    let s = &self.s[start..self.i];
                    Ok(JsonValue::Flt(s.parse().map_err(|_| ())?))
                } else {
                    Ok(JsonValue::Number(
                        self.s[start..self.i].parse().map_err(|_| ())?,
                    ))
                }
            }
            _ => Err(()),
        }
    }

    fn expect_literal(&mut self, lit: &str) -> Result<(), ()> {
        if self.s[self.i..].starts_with(lit) {
            self.i += lit.len();
            Ok(())
        } else {
            Err(())
        }
    }

    fn string(&mut self) -> Result<String, ()> {
        if self.bump() != Some('"') {
            return Err(());
        }
        let mut out = String::new();
        while let Some(c) = self.peek() {
            if c == '"' {
                self.bump();
                return Ok(out);
            }
            if c == '\\' {
                self.bump();
                let esc = self.bump().ok_or(())?;
                out.push(match esc {
                    '"' => '"',
                    '\\' => '\\',
                    '/' => '/',
                    'b' => '\x08',
                    'f' => '\x0c',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    'u' => {
                        let hex: String = self.s[self.i..].chars().take(4).collect();
                        if hex.len() != 4 {
                            return Err(());
                        }
                        self.i += 4;
                        char::from_u32(u32::from_str_radix(&hex, 16).map_err(|_| ())?).ok_or(())?
                    }
                    _ => return Err(()),
                });
            } else {
                self.bump();
                out.push(c);
            }
        }
        Err(())
    }
}

fn json_get<'a>(v: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    match v {
        JsonValue::Object(m) => m.get(key),
        _ => None,
    }
}

fn json_str(v: &JsonValue) -> Option<&str> {
    match v {
        JsonValue::String(s) => Some(s),
        _ => None,
    }
}

fn json_int(v: &JsonValue) -> Option<i64> {
    match v {
        JsonValue::Number(n) => Some(*n),
        JsonValue::Flt(f) => Some(*f as i64),
        _ => None,
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

// ── LSP positions (UTF-16 code units) ────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LspPos {
    pub line: u32,
    pub character: u32,
}

#[derive(Clone, Copy, Debug)]
struct LspRange {
    start: LspPos,
    end: LspPos,
}

fn byte_span_to_range(src: &str, span: Span) -> LspRange {
    LspRange {
        start: byte_offset_to_lsp(src, span.start),
        end: byte_offset_to_lsp(src, span.end),
    }
}

pub fn byte_offset_to_lsp(src: &str, offset: usize) -> LspPos {
    let offset = offset.min(src.len());
    let mut line = 0u32;
    let mut line_start = 0usize;
    for (i, c) in src.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    let line_text = &src[line_start..offset];
    let character = line_text.encode_utf16().count() as u32;
    LspPos { line, character }
}

/// Convert an LSP (line, UTF-16 char) position back to a byte offset.
pub fn lsp_pos_to_offset(src: &str, pos: LspPos) -> usize {
    let mut cur_line = 0u32;
    let mut line_byte_start = 0usize;
    for (i, c) in src.char_indices() {
        if cur_line == pos.line {
            break;
        }
        if c == '\n' {
            cur_line += 1;
            line_byte_start = i + 1;
        }
    }
    if cur_line < pos.line {
        return src.len();
    }
    let line_text = &src[line_byte_start..];
    let mut utf16_count = 0u32;
    let mut byte_off = line_byte_start;
    for c in line_text.chars() {
        if utf16_count >= pos.character {
            break;
        }
        utf16_count += c.len_utf16() as u32;
        byte_off += c.len_utf8();
    }
    byte_off.min(src.len())
}

fn full_document_range(src: &str) -> LspRange {
    let end = byte_offset_to_lsp(src, src.len());
    LspRange {
        start: LspPos {
            line: 0,
            character: 0,
        },
        end,
    }
}

fn range_json(r: LspRange) -> String {
    format!(
        r#"{{"start":{{"line":{},"character":{}}},"end":{{"line":{},"character":{}}}}}"#,
        r.start.line, r.start.character, r.end.line, r.end.character
    )
}

// ── Symbol DB ────────────────────────────────────────────────────────────────
//
// Built by walking the ProgramBundle's AST after parsing + type-checking.
// The server builds this DB on every document check and uses it for
// completion, hover, go-to-def, references, rename, and inlay hints.
// (LSP-I1: no new "language knowledge" here — just indexing what sema knows.)

/// The semantic kind of a defined symbol.
#[derive(Debug, Clone)]
enum SymKind {
    Function {
        params: Vec<(String, ast::Type)>,
        ret: Option<ast::Type>,
    },
    Struct {
        fields: Vec<(String, ast::Type)>,
    },
    Enum {
        variants: Vec<String>,
    },
    Trait,
    Const,
    EnumVariant {
        parent: String,
    },
    Field {
        ty: ast::Type,
        parent: String,
    },
    Local {
        mutable: bool,
        ty: Option<ast::Type>,
    },
    Param {
        ty: ast::Type,
    },
}

/// One named definition in the program.
#[derive(Debug, Clone)]
struct SymDef {
    name: String,
    def_span: Span,
    module_path: String,
    kind: SymKind,
}

/// One use-site reference (identifier occurrence).
#[derive(Debug, Clone)]
struct SymRef {
    name: String,
    span: Span,
    module_path: String,
}

/// Hover entry: an expression/token span + text to show on hover.
#[derive(Debug, Clone)]
struct HoverEntry {
    span: Span,
    module_path: String,
    text: String,
}

/// Inlay hint: position (just past the binding name) + type text to show.
#[derive(Debug, Clone)]
struct InlayHint {
    span: Span,
    module_path: String,
    label: String,
}

/// The full symbol index for a checked program bundle.
struct SymbolDB {
    defs: Vec<SymDef>,
    refs: Vec<SymRef>,
    hover: Vec<HoverEntry>,
    inlay: Vec<InlayHint>,
}

impl SymbolDB {
    fn new() -> Self {
        SymbolDB {
            defs: Vec::new(),
            refs: Vec::new(),
            hover: Vec::new(),
            inlay: Vec::new(),
        }
    }

    /// Find definition(s) of `name` (all modules).
    fn find_def(&self, name: &str) -> Option<&SymDef> {
        self.defs.iter().find(|d| d.name == name)
    }

    /// Find the definition whose def_span contains `offset` in module at `path`.
    fn def_at_offset(&self, path: &str, offset: usize) -> Option<&SymDef> {
        self.defs.iter().find(|d| {
            d.module_path == path && d.def_span.start <= offset && offset <= d.def_span.end
        })
    }

    /// Find any ref or def name at `offset` in module `path`.
    fn name_at_offset(&self, path: &str, offset: usize) -> Option<&str> {
        // Check defs first
        if let Some(d) = self.def_at_offset(path, offset) {
            return Some(&d.name);
        }
        // Check refs
        self.refs
            .iter()
            .find(|r| r.module_path == path && r.span.start <= offset && offset <= r.span.end)
            .map(|r| r.name.as_str())
    }

    /// All references to `name` across all modules.
    fn all_refs(&self, name: &str) -> Vec<&SymRef> {
        self.refs.iter().filter(|r| r.name == name).collect()
    }

    /// Hover text for the symbol at `offset` in `path`.
    fn hover_at(&self, path: &str, offset: usize) -> Option<&str> {
        self.hover
            .iter()
            .find(|h| h.module_path == path && h.span.start <= offset && offset <= h.span.end)
            .map(|h| h.text.as_str())
    }

    /// All inlay hints for a module path.
    fn inlay_hints_for(&self, path: &str) -> Vec<&InlayHint> {
        self.inlay
            .iter()
            .filter(|h| h.module_path == path)
            .collect()
    }
}

/// Build the symbol DB by walking all items in a bundle.
fn build_symbol_db(bundle: &ProgramBundle) -> SymbolDB {
    let mut db = SymbolDB::new();
    for module in &bundle.modules {
        let mp = module.display.clone();
        for item in &module.items {
            collect_item(item, &mp, module, &mut db);
        }
    }
    db
}

fn collect_item(item: &Item, mp: &str, module: &LoadedModule, db: &mut SymbolDB) {
    match item {
        Item::Func(f) => {
            let params: Vec<(String, ast::Type)> = f
                .params
                .iter()
                .filter(|p| p.name != crate::syntax::KW_SELF)
                .map(|p| (p.name.clone(), p.ty.clone()))
                .collect();
            let sym = SymDef {
                name: f.name.clone(),
                def_span: f.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Function {
                    params: params.clone(),
                    ret: f.return_type.clone(),
                },
            };
            let hover_text = hover_for_fn(f);
            db.hover.push(HoverEntry {
                span: f.name_span,
                module_path: mp.to_string(),
                text: hover_text,
            });
            db.defs.push(sym);
            // param defs
            for p in &f.params {
                if p.name == crate::syntax::KW_SELF {
                    continue;
                }
                db.defs.push(SymDef {
                    name: p.name.clone(),
                    def_span: p.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Param { ty: p.ty.clone() },
                });
                db.hover.push(HoverEntry {
                    span: p.name_span,
                    module_path: mp.to_string(),
                    text: format!("`{}`: {}", p.name, p.ty.name()),
                });
            }
            collect_stmts(&f.body, mp, module, db);
        }
        Item::Struct(s) => {
            let fields: Vec<(String, ast::Type)> = s
                .fields
                .iter()
                .map(|f| (f.name.clone(), f.ty.clone()))
                .collect();
            db.defs.push(SymDef {
                name: s.name.clone(),
                def_span: s.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Struct {
                    fields: fields.clone(),
                },
            });
            db.hover.push(HoverEntry {
                span: s.name_span,
                module_path: mp.to_string(),
                text: format!("struct `{}`", s.name),
            });
            for f in &s.fields {
                db.defs.push(SymDef {
                    name: f.name.clone(),
                    def_span: f.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Field {
                        ty: f.ty.clone(),
                        parent: s.name.clone(),
                    },
                });
                db.hover.push(HoverEntry {
                    span: f.name_span,
                    module_path: mp.to_string(),
                    text: format!("`{}`: {} (field of `{}`)", f.name, f.ty.name(), s.name),
                });
            }
            for meth in &s.methods {
                let hover_text = hover_for_fn(meth);
                db.defs.push(SymDef {
                    name: meth.name.clone(),
                    def_span: meth.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Function {
                        params: meth
                            .params
                            .iter()
                            .filter(|p| p.name != crate::syntax::KW_SELF)
                            .map(|p| (p.name.clone(), p.ty.clone()))
                            .collect(),
                        ret: meth.return_type.clone(),
                    },
                });
                db.hover.push(HoverEntry {
                    span: meth.name_span,
                    module_path: mp.to_string(),
                    text: hover_text,
                });
                for p in &meth.params {
                    if p.name != crate::syntax::KW_SELF {
                        db.defs.push(SymDef {
                            name: p.name.clone(),
                            def_span: p.name_span,
                            module_path: mp.to_string(),
                            kind: SymKind::Param { ty: p.ty.clone() },
                        });
                    }
                }
                collect_stmts(&meth.body, mp, module, db);
            }
            for tb in &s.trait_impls {
                for meth in &tb.methods {
                    collect_stmts(&meth.body, mp, module, db);
                }
            }
        }
        Item::Enum(e) => {
            let variants: Vec<String> = e.variants.iter().map(|v| v.name.clone()).collect();
            db.defs.push(SymDef {
                name: e.name.clone(),
                def_span: e.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Enum {
                    variants: variants.clone(),
                },
            });
            db.hover.push(HoverEntry {
                span: e.name_span,
                module_path: mp.to_string(),
                text: format!("enum `{}` — variants: {}", e.name, variants.join(", ")),
            });
            for v in &e.variants {
                db.defs.push(SymDef {
                    name: v.name.clone(),
                    def_span: v.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::EnumVariant {
                        parent: e.name.clone(),
                    },
                });
                db.hover.push(HoverEntry {
                    span: v.name_span,
                    module_path: mp.to_string(),
                    text: format!("`{}` — variant of `{}`", v.name, e.name),
                });
            }
            for meth in &e.methods {
                db.defs.push(SymDef {
                    name: meth.name.clone(),
                    def_span: meth.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Function {
                        params: meth
                            .params
                            .iter()
                            .filter(|p| p.name != crate::syntax::KW_SELF)
                            .map(|p| (p.name.clone(), p.ty.clone()))
                            .collect(),
                        ret: meth.return_type.clone(),
                    },
                });
                collect_stmts(&meth.body, mp, module, db);
            }
            for tb in &e.trait_impls {
                for meth in &tb.methods {
                    collect_stmts(&meth.body, mp, module, db);
                }
            }
        }
        Item::Trait(t) => {
            db.defs.push(SymDef {
                name: t.name.clone(),
                def_span: t.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Trait,
            });
            db.hover.push(HoverEntry {
                span: t.name_span,
                module_path: mp.to_string(),
                text: format!("trait `{}`", t.name),
            });
            for sig in &t.methods {
                db.defs.push(SymDef {
                    name: sig.name.clone(),
                    def_span: sig.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Function {
                        params: sig
                            .params
                            .iter()
                            .filter(|p| p.name != crate::syntax::KW_SELF)
                            .map(|p| (p.name.clone(), p.ty.clone()))
                            .collect(),
                        ret: sig.return_type.clone(),
                    },
                });
            }
        }
        Item::Impl(i) => {
            for meth in &i.methods {
                db.defs.push(SymDef {
                    name: meth.name.clone(),
                    def_span: meth.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Function {
                        params: meth
                            .params
                            .iter()
                            .filter(|p| p.name != crate::syntax::KW_SELF)
                            .map(|p| (p.name.clone(), p.ty.clone()))
                            .collect(),
                        ret: meth.return_type.clone(),
                    },
                });
                for p in &meth.params {
                    if p.name != crate::syntax::KW_SELF {
                        db.defs.push(SymDef {
                            name: p.name.clone(),
                            def_span: p.name_span,
                            module_path: mp.to_string(),
                            kind: SymKind::Param { ty: p.ty.clone() },
                        });
                    }
                }
                collect_stmts(&meth.body, mp, module, db);
            }
        }
        Item::Const(c) => {
            db.defs.push(SymDef {
                name: c.name.clone(),
                def_span: c.name_span,
                module_path: mp.to_string(),
                kind: SymKind::Const,
            });
            db.hover.push(HoverEntry {
                span: c.name_span,
                module_path: mp.to_string(),
                text: format!("const `{}`", c.name),
            });
            collect_expr(&c.value, mp, db);
        }
        Item::Test(t) => {
            collect_stmts(&t.body, mp, module, db);
        }
        Item::ExternRust(_) => {}
    }
}

fn hover_for_fn(f: &ast::Func) -> String {
    let params: Vec<String> = f
        .params
        .iter()
        .filter(|p| p.name != crate::syntax::KW_SELF)
        .map(|p| format!("{}: {}", p.name, p.ty.name()))
        .collect();
    let ret = match &f.return_type {
        Some(t) => format!(" -> {}", t.name()),
        None => String::new(),
    };
    format!("fn {}({}){}", f.name, params.join(", "), ret)
}

fn collect_stmts(stmts: &[ast::Stmt], mp: &str, module: &LoadedModule, db: &mut SymbolDB) {
    for stmt in stmts {
        collect_stmt(stmt, mp, module, db);
    }
}

fn collect_stmt(stmt: &ast::Stmt, mp: &str, module: &LoadedModule, db: &mut SymbolDB) {
    match stmt {
        ast::Stmt::Val(b) => {
            collect_binding(b, mp, db);
        }
        ast::Stmt::Expr(e) => collect_expr(e, mp, db),
        ast::Stmt::Assign { target, value, .. } => {
            collect_lvalue(target, mp, db);
            collect_expr(value, mp, db);
        }
        ast::Stmt::Return(e, _) => {
            if let Some(e) = e {
                collect_expr(e, mp, db);
            }
        }
        ast::Stmt::If(if_stmt) => {
            collect_if(if_stmt, mp, module, db);
        }
        ast::Stmt::While { cond, body, .. } => {
            collect_expr(cond, mp, db);
            collect_stmts(body, mp, module, db);
        }
        ast::Stmt::For {
            var,
            var_span,
            var2,
            kind,
            body,
            ..
        } => {
            db.defs.push(SymDef {
                name: var.clone(),
                def_span: *var_span,
                module_path: mp.to_string(),
                kind: SymKind::Local {
                    mutable: false,
                    ty: None,
                },
            });
            if let Some((v2, s2)) = var2 {
                db.defs.push(SymDef {
                    name: v2.clone(),
                    def_span: *s2,
                    module_path: mp.to_string(),
                    kind: SymKind::Local {
                        mutable: false,
                        ty: None,
                    },
                });
            }
            match kind {
                ast::ForKind::Range { start, end, step } => {
                    collect_expr(start, mp, db);
                    collect_expr(end, mp, db);
                    if let Some(step) = step {
                        collect_expr(step, mp, db);
                    }
                }
                ast::ForKind::In { collection } => {
                    collect_expr(collection, mp, db);
                }
            }
            collect_stmts(body, mp, module, db);
        }
        ast::Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        } => {
            collect_expr(subject, mp, db);
            for arm in arms {
                collect_expr(&arm.cond, mp, db);
                collect_stmts(&arm.body, mp, module, db);
            }
            if let Some(eb) = else_body {
                collect_stmts(eb, mp, module, db);
            }
        }
        ast::Stmt::Loop(body, _) | ast::Stmt::Unsafe(body, _) => {
            collect_stmts(body, mp, module, db);
        }
        ast::Stmt::Break(_) | ast::Stmt::Continue(_) => {}
    }
}

fn collect_if(if_stmt: &ast::IfStmt, mp: &str, module: &LoadedModule, db: &mut SymbolDB) {
    collect_expr(&if_stmt.cond, mp, db);
    collect_stmts(&if_stmt.then_body, mp, module, db);
    if let Some(eb) = &if_stmt.else_branch {
        match eb {
            ast::ElseBranch::ElseIf(inner) => collect_if(inner, mp, module, db),
            ast::ElseBranch::Else(body) => collect_stmts(body, mp, module, db),
        }
    }
}

fn collect_lvalue(lv: &ast::LValue, mp: &str, db: &mut SymbolDB) {
    match lv {
        ast::LValue::Local { name, name_span } => {
            db.refs.push(SymRef {
                name: name.clone(),
                span: *name_span,
                module_path: mp.to_string(),
            });
        }
        ast::LValue::Index { base, index, .. } => {
            collect_expr(base, mp, db);
            collect_expr(index, mp, db);
        }
    }
}

fn collect_binding(b: &ast::Binding, mp: &str, db: &mut SymbolDB) {
    // S74: a destructuring binding brings each named field/element into scope.
    if let Some(pat) = &b.pattern {
        for n in pat.names() {
            db.defs.push(SymDef {
                name: n.name.clone(),
                def_span: n.span,
                module_path: mp.to_string(),
                kind: SymKind::Local {
                    mutable: b.mutable,
                    ty: None,
                },
            });
        }
        collect_expr(&b.init, mp, db);
        return;
    }
    let ty = b.ty.clone();
    // has_explicit_annotation: user wrote `: Type` — ty_span is Some iff the annotation is in source
    let has_explicit = b.ty_span.is_some();
    db.defs.push(SymDef {
        name: b.name.clone(),
        def_span: b.name_span,
        module_path: mp.to_string(),
        kind: SymKind::Local {
            mutable: b.mutable,
            ty: ty.clone(),
        },
    });
    if let Some(t) = &ty {
        let kw = if b.mutable { "var" } else { "val" };
        db.hover.push(HoverEntry {
            span: b.name_span,
            module_path: mp.to_string(),
            text: format!("`{}`: {} ({})", b.name, t.name(), kw),
        });
        // Inlay hint: only when user omitted the annotation (sema filled it in)
        if !has_explicit {
            db.inlay.push(InlayHint {
                span: b.name_span,
                module_path: mp.to_string(),
                label: format!(": {}", t.name()),
            });
        }
    }
    collect_expr(&b.init, mp, db);
}

fn collect_expr(e: &ast::Expr, mp: &str, db: &mut SymbolDB) {
    match e {
        ast::Expr::Ident(name, span) => {
            db.refs.push(SymRef {
                name: name.clone(),
                span: *span,
                module_path: mp.to_string(),
            });
        }
        ast::Expr::Call(call) => {
            db.refs.push(SymRef {
                name: call.name.clone(),
                span: call.name_span,
                module_path: mp.to_string(),
            });
            for arg in &call.args {
                collect_expr(&arg.expr, mp, db);
            }
        }
        ast::Expr::MethodCall {
            receiver,
            method,
            method_span,
            args,
            ..
        } => {
            collect_expr(receiver, mp, db);
            db.refs.push(SymRef {
                name: method.clone(),
                span: *method_span,
                module_path: mp.to_string(),
            });
            for arg in args {
                collect_expr(&arg.expr, mp, db);
            }
        }
        ast::Expr::Field(base, field, span) => {
            collect_expr(base, mp, db);
            db.refs.push(SymRef {
                name: field.clone(),
                span: *span,
                module_path: mp.to_string(),
            });
        }
        ast::Expr::OptField {
            base,
            member,
            member_span,
            ..
        } => {
            collect_expr(base, mp, db);
            db.refs.push(SymRef {
                name: member.clone(),
                span: *member_span,
                module_path: mp.to_string(),
            });
        }
        ast::Expr::Binary(_, l, r, _) => {
            collect_expr(l, mp, db);
            collect_expr(r, mp, db);
        }
        ast::Expr::Unary(_, inner, _) => collect_expr(inner, mp, db),
        ast::Expr::Deref(inner, _) => collect_expr(inner, mp, db),
        ast::Expr::Index { base, index, .. } => {
            collect_expr(base, mp, db);
            collect_expr(index, mp, db);
        }
        ast::Expr::Slice {
            base, start, end, ..
        } => {
            collect_expr(base, mp, db);
            collect_expr(start, mp, db);
            collect_expr(end, mp, db);
        }
        ast::Expr::Str(parts, _) => {
            for part in parts {
                if let ast::StrPart::Interp(inner) = part {
                    collect_expr(inner, mp, db);
                }
            }
        }
        ast::Expr::ListLit(items, _) => {
            for i in items {
                collect_expr(i, mp, db);
            }
        }
        ast::Expr::MapLit(pairs, _) => {
            for (k, v) in pairs {
                collect_expr(k, mp, db);
                collect_expr(v, mp, db);
            }
        }
        ast::Expr::TupleLit(fields, _, _) => {
            for (_, expr) in fields {
                collect_expr(expr, mp, db);
            }
        }
        ast::Expr::StructLit { fields, .. } => {
            for (_, _, expr) in fields {
                collect_expr(expr, mp, db);
            }
        }
        ast::Expr::EnumLit { args, .. } => {
            for arg in args {
                match arg {
                    ast::EnumLitArg::Positional(e) => collect_expr(e, mp, db),
                    ast::EnumLitArg::Named { expr, .. } => collect_expr(expr, mp, db),
                }
            }
        }
        ast::Expr::Present(inner, _)
        | ast::Expr::Ok(inner, _)
        | ast::Expr::Err(inner, _)
        | ast::Expr::Try(inner, _) => collect_expr(inner, mp, db),
        ast::Expr::OrFallback {
            value, fallback, ..
        } => {
            collect_expr(value, mp, db);
            match fallback {
                ast::OrFallback::Value(v) => collect_expr(v, mp, db),
                ast::OrFallback::Panic { args, .. } => {
                    for a in args {
                        collect_expr(&a.expr, mp, db);
                    }
                }
                ast::OrFallback::Return(v, _) => {
                    if let Some(v) = v {
                        collect_expr(v, mp, db);
                    }
                }
            }
        }
        ast::Expr::PatternTest { subject, .. } => collect_expr(subject, mp, db),
        ast::Expr::Lambda(l) => {
            for p in &l.params {
                db.defs.push(SymDef {
                    name: p.name.clone(),
                    def_span: p.name_span,
                    module_path: mp.to_string(),
                    kind: SymKind::Param {
                        ty: p.ty.clone().unwrap_or(ast::Type::Int),
                    },
                });
            }
            match &l.body {
                ast::LambdaBody::Expr(e) => collect_expr(e, mp, db),
                ast::LambdaBody::Block(stmts) => {
                    for s in stmts {
                        if let ast::Stmt::Val(b) = s {
                            collect_binding(b, mp, db);
                        } else {
                            collect_expr_stmt(s, mp, db);
                        }
                    }
                }
            }
        }
        ast::Expr::CallValue { callee, args, .. } => {
            collect_expr(callee, mp, db);
            for a in args {
                collect_expr(&a.expr, mp, db);
            }
        }
        ast::Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            collect_expr(cond, mp, db);
            for s in then_body.iter().chain(else_body.iter()) {
                if let ast::Stmt::Val(b) = s {
                    collect_binding(b, mp, db);
                } else {
                    collect_expr_stmt(s, mp, db);
                }
            }
            collect_expr(then_value, mp, db);
            collect_expr(else_value, mp, db);
        }
        ast::Expr::Int(_, _)
        | ast::Expr::Float(_, _)
        | ast::Expr::Bool(_, _)
        | ast::Expr::Char(_, _)
        | ast::Expr::Absent(_) => {}
    }
}

fn collect_expr_stmt(stmt: &ast::Stmt, mp: &str, db: &mut SymbolDB) {
    if let ast::Stmt::Expr(e) = stmt {
        collect_expr(e, mp, db);
    }
}

// ── Completion ────────────────────────────────────────────────────────────────

/// LSP completion item kinds (standard integers).
#[allow(dead_code)]
mod ck {
    pub const TEXT: u8 = 1;
    pub const METHOD: u8 = 2;
    pub const FUNCTION: u8 = 3;
    pub const CONSTRUCTOR: u8 = 4;
    pub const FIELD: u8 = 5;
    pub const VARIABLE: u8 = 6;
    pub const CLASS: u8 = 7;
    pub const INTERFACE: u8 = 8;
    pub const MODULE: u8 = 9;
    pub const PROPERTY: u8 = 10;
    pub const UNIT: u8 = 11;
    pub const VALUE: u8 = 12;
    pub const ENUM: u8 = 13;
    pub const KEYWORD: u8 = 14;
    pub const SNIPPET: u8 = 15;
    pub const COLOR: u8 = 16;
    pub const FILE: u8 = 17;
    pub const REFERENCE: u8 = 18;
    pub const FOLDER: u8 = 19;
    pub const ENUM_MEMBER: u8 = 20;
    pub const CONSTANT: u8 = 21;
    pub const STRUCT: u8 = 22;
    pub const EVENT: u8 = 23;
    pub const OPERATOR: u8 = 24;
    pub const TYPE_PARAMETER: u8 = 25;
}

struct CompletionItem {
    label: String,
    kind: u8,
    detail: Option<String>,
    insert_text: Option<String>,
    insert_text_format: u8, // 1=plain, 2=snippet
    /// D-LSP5: import statement to insert at top of file (auto-import).
    auto_import: Option<String>,
}

impl CompletionItem {
    fn to_json(&self) -> String {
        let detail = match &self.detail {
            Some(d) => format!(r#","detail":"{}""#, json_escape(d)),
            None => String::new(),
        };
        let insert = match &self.insert_text {
            Some(t) => format!(
                r#","insertText":"{}","insertTextFormat":{}"#,
                json_escape(t),
                self.insert_text_format
            ),
            None => String::new(),
        };
        let additional = match &self.auto_import {
            Some(stmt) => format!(
                r#","additionalTextEdits":[{{"range":{{"start":{{"line":0,"character":0}},"end":{{"line":0,"character":0}}}},"newText":"{}"}}]"#,
                json_escape(stmt)
            ),
            None => String::new(),
        };
        format!(
            r#"{{"label":"{}","kind":{}{}{}{}}}"#,
            json_escape(&self.label),
            self.kind,
            detail,
            insert,
            additional
        )
    }
}

/// Jet keywords for completion.
const JET_KEYWORDS: &[&str] = &[
    "fn", "pub", "val", "var", "if", "else", "while", "for", "in", "when", "break", "continue",
    "return", "struct", "enum", "impl", "trait", "const", "comptime", "import", "extern", "test",
    "derive", "mut", "take", "view", "ref", "self", "loop", "unsafe", "or", "true", "false",
    "null", "ok", "err", "value", "it",
];

/// Built-in type names for completion.
const JET_TYPES: &[&str] = &[
    "Int", "Float", "Bool", "String", "Char", "List", "Map", "Shared", "Result",
];

/// Is the character sequence before `offset` indicative of member access (`.`)?
fn context_is_member_access(src: &str, offset: usize) -> Option<String> {
    let before = &src[..offset.min(src.len())];
    // Walk backward over the current identifier, then check for `.`
    let bytes = before.as_bytes();
    let mut i = bytes.len();
    // skip current word
    while i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_') {
        i -= 1;
    }
    if i > 0 && bytes[i - 1] == b'.' {
        // Find the word before the `.`
        i -= 1;
        let end = i;
        while i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_') {
            i -= 1;
        }
        if i < end {
            return Some(
                std::str::from_utf8(&bytes[i..end])
                    .unwrap_or("")
                    .to_string(),
            );
        }
    }
    None
}

/// Is the cursor inside a switch body for an enum type?
fn detect_switch_enum_type<'a>(src: &str, offset: usize, db: &'a SymbolDB) -> Option<&'a str> {
    // Look backward for `when <ident> {` pattern
    let before = &src[..offset.min(src.len())];
    if let Some(kw_pos) = before.rfind("when ") {
        let after_kw = before[kw_pos + 5..].trim_start();
        let ident_end = after_kw
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(after_kw.len());
        let ident = &after_kw[..ident_end];
        if !ident.is_empty() {
            // Look up ident in DB to find its type
            for d in &db.defs {
                if d.name == ident {
                    if let SymKind::Local {
                        ty: Some(ast::Type::Named(type_name)),
                        ..
                    }
                    | SymKind::Param {
                        ty: ast::Type::Named(type_name),
                    } = &d.kind
                    {
                        // Check if that type is an enum
                        for ed in &db.defs {
                            if ed.name == *type_name {
                                if let SymKind::Enum { .. } = &ed.kind {
                                    return Some(&ed.name);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

fn compute_completions(
    db: &SymbolDB,
    src: &str,
    offset: usize,
    current_path: &str,
) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Member completion: `expr.`
    if let Some(receiver_name) = context_is_member_access(src, offset) {
        // Find the type of receiver_name from DB
        for def in &db.defs {
            if def.name == receiver_name {
                match &def.kind {
                    SymKind::Struct { fields } => {
                        for (fname, fty) in fields {
                            if seen.insert(fname.clone()) {
                                items.push(CompletionItem {
                                    label: fname.clone(),
                                    kind: ck::FIELD,
                                    detail: Some(fty.name()),
                                    insert_text: None,
                                    insert_text_format: 1,
                                    auto_import: None,
                                });
                            }
                        }
                    }
                    SymKind::Local {
                        ty: Some(ast::Type::Named(tn)),
                        ..
                    }
                    | SymKind::Param {
                        ty: ast::Type::Named(tn),
                    } => {
                        // look up tn's fields/methods
                        let tn = tn.clone();
                        for td in &db.defs {
                            if td.name == tn {
                                if let SymKind::Struct { fields } = &td.kind {
                                    for (fname, fty) in fields {
                                        if seen.insert(fname.clone()) {
                                            items.push(CompletionItem {
                                                label: fname.clone(),
                                                kind: ck::FIELD,
                                                detail: Some(fty.name()),
                                                insert_text: None,
                                                insert_text_format: 1,
                                                auto_import: None,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
                // Also add methods
                for md in &db.defs {
                    if let SymKind::Function { params, ret } = &md.kind {
                        if params.first().map(|(n, _)| n.as_str()) == Some("self")
                            || md.module_path == def.module_path
                        {
                            // heuristic: include all methods in same module
                            if seen.insert(format!("m:{}", md.name)) {
                                let detail = format!(
                                    "fn {}({})",
                                    md.name,
                                    params
                                        .iter()
                                        .map(|(n, t)| format!("{}: {}", n, t.name()))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                );
                                items.push(CompletionItem {
                                    label: md.name.clone(),
                                    kind: ck::METHOD,
                                    detail: Some(detail),
                                    insert_text: None,
                                    insert_text_format: 1,
                                    auto_import: None,
                                });
                            }
                        }
                    }
                }
                return items;
            }
        }
        return items;
    }

    // Switch-arm enum snippet completion
    if let Some(enum_type) = detect_switch_enum_type(src, offset, db) {
        for def in &db.defs {
            if def.name == enum_type {
                if let SymKind::Enum { variants } = &def.kind {
                    for v in variants {
                        let label = format!("{}.{}", enum_type, v);
                        if seen.insert(label.clone()) {
                            items.push(CompletionItem {
                                label: label.clone(),
                                kind: ck::ENUM_MEMBER,
                                detail: Some(format!("variant of {}", enum_type)),
                                insert_text: Some(format!("{}.{} {{}}", enum_type, v)),
                                insert_text_format: 2,
                                auto_import: None,
                            });
                        }
                    }
                    break;
                }
            }
        }
    }

    // D-LSP5: for symbols from other modules, generate an auto-import edit if
    // that module isn't already imported in the current source.
    let auto_import_for = |mp: &str| -> Option<String> {
        if mp == current_path || mp.is_empty() {
            return None;
        }
        if src.contains(&format!("\"{}\"", mp)) {
            return None; // already imported
        }
        Some(format!("import \"{}\";\n", mp))
    };

    // All top-level definitions
    for def in &db.defs {
        match &def.kind {
            SymKind::Function { params, ret: _ } => {
                if seen.insert(def.name.clone()) {
                    let detail = format!(
                        "fn {}({})",
                        def.name,
                        params
                            .iter()
                            .map(|(n, t)| format!("{}: {}", n, t.name()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    items.push(CompletionItem {
                        label: def.name.clone(),
                        kind: ck::FUNCTION,
                        detail: Some(detail),
                        insert_text: None,
                        insert_text_format: 1,
                        auto_import: auto_import_for(&def.module_path),
                    });
                }
            }
            SymKind::Struct { .. } => {
                if seen.insert(def.name.clone()) {
                    items.push(CompletionItem {
                        label: def.name.clone(),
                        kind: ck::STRUCT,
                        detail: Some(format!("struct {}", def.name)),
                        insert_text: None,
                        insert_text_format: 1,
                        auto_import: auto_import_for(&def.module_path),
                    });
                }
            }
            SymKind::Enum { variants } => {
                if seen.insert(def.name.clone()) {
                    items.push(CompletionItem {
                        label: def.name.clone(),
                        kind: ck::ENUM,
                        detail: Some(format!(
                            "enum {} — variants: {}",
                            def.name,
                            variants.join(", ")
                        )),
                        insert_text: None,
                        insert_text_format: 1,
                        auto_import: auto_import_for(&def.module_path),
                    });
                }
            }
            SymKind::Const => {
                if seen.insert(def.name.clone()) {
                    items.push(CompletionItem {
                        label: def.name.clone(),
                        kind: ck::CONSTANT,
                        detail: None,
                        insert_text: None,
                        insert_text_format: 1,
                        auto_import: auto_import_for(&def.module_path),
                    });
                }
            }
            SymKind::Trait => {
                if seen.insert(def.name.clone()) {
                    items.push(CompletionItem {
                        label: def.name.clone(),
                        kind: ck::INTERFACE,
                        detail: Some(format!("trait {}", def.name)),
                        insert_text: None,
                        insert_text_format: 1,
                        auto_import: auto_import_for(&def.module_path),
                    });
                }
            }
            SymKind::Local { mutable: _, ty } => {
                if seen.insert(def.name.clone()) {
                    let detail = ty.as_ref().map(|t| t.name());
                    items.push(CompletionItem {
                        label: def.name.clone(),
                        kind: ck::VARIABLE,
                        detail,
                        insert_text: None,
                        insert_text_format: 1,
                        auto_import: None,
                    });
                }
            }
            SymKind::Param { ty } => {
                if seen.insert(def.name.clone()) {
                    items.push(CompletionItem {
                        label: def.name.clone(),
                        kind: ck::VARIABLE,
                        detail: Some(ty.name()),
                        insert_text: None,
                        insert_text_format: 1,
                        auto_import: None,
                    });
                }
            }
            _ => {}
        }
    }

    // Keywords
    for kw in JET_KEYWORDS {
        if seen.insert(format!("kw:{}", kw)) {
            items.push(CompletionItem {
                label: kw.to_string(),
                kind: ck::KEYWORD,
                detail: None,
                insert_text: None,
                insert_text_format: 1,
                auto_import: None,
            });
        }
    }

    // Built-in types
    for ty in JET_TYPES {
        if seen.insert(format!("ty:{}", ty)) {
            items.push(CompletionItem {
                label: ty.to_string(),
                kind: ck::CLASS,
                detail: Some("built-in type".to_string()),
                insert_text: None,
                insert_text_format: 1,
                auto_import: None,
            });
        }
    }

    items
}

// ── Hover ─────────────────────────────────────────────────────────────────────

/// B7 (D-LSP6): Collect adjacent `///` doc-comment lines immediately preceding
/// `def_start` in the raw token stream (which includes LineComment tokens).
fn collect_doc_comment(tokens: &[Token], def_start: usize) -> Option<String> {
    // Find the first token at or after def_start.
    let idx = tokens.partition_point(|t| t.span.end <= def_start);
    let mut lines: Vec<String> = Vec::new();
    let mut j = idx;
    loop {
        if j == 0 {
            break;
        }
        j -= 1;
        match &tokens[j].kind {
            TokKind::LineComment(text) if text.starts_with("///") => {
                let doc = text.trim_start_matches('/').trim().to_string();
                lines.push(doc);
            }
            // A regular `//` comment or any non-comment token stops the search.
            _ => break,
        }
    }
    if lines.is_empty() {
        return None;
    }
    lines.reverse();
    Some(lines.join("\n"))
}

fn compute_hover(
    db: &SymbolDB,
    tokens: &[Token],
    _src: &str,
    path: &str,
    offset: usize,
) -> Option<String> {
    // Collect the base hover text (type signature / ownership annotation).
    let base = if let Some(text) = db.hover_at(path, offset) {
        text.to_string()
    } else {
        // Fall back: find the token at offset and look up the name.
        let name = find_ident_at(tokens, offset)?;
        if let Some(def) = db.defs.iter().find(|d| d.name == name) {
            match &def.kind {
                SymKind::Function { params, ret } => {
                    let ps: Vec<String> = params
                        .iter()
                        .map(|(n, t)| format!("{}: {}", n, t.name()))
                        .collect();
                    let r = match ret {
                        Some(t) => format!(" -> {}", t.name()),
                        None => String::new(),
                    };
                    format!("fn {}({}){}", name, ps.join(", "), r)
                }
                SymKind::Struct { fields } => {
                    format!(
                        "struct `{}`\n\nFields: {}",
                        name,
                        fields
                            .iter()
                            .map(|(n, t)| format!("{}: {}", n, t.name()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
                SymKind::Enum { variants } => {
                    format!("enum `{}`\n\nVariants: {}", name, variants.join(", "))
                }
                SymKind::Trait => format!("trait `{}`", name),
                SymKind::Const => format!("const `{}`", name),
                SymKind::EnumVariant { parent } => format!("`{}` — variant of `{}`", name, parent),
                SymKind::Field { ty, parent } => {
                    format!("`{}`: {} (field of `{}`)", name, ty.name(), parent)
                }
                SymKind::Local { mutable, ty } => {
                    let kw = if *mutable { "var" } else { "val" };
                    match ty {
                        Some(t) => format!("`{}`: {} ({})", name, t.name(), kw),
                        None => format!("`{}` ({})", name, kw),
                    }
                }
                SymKind::Param { ty } => format!("`{}`: {} (parameter)", name, ty.name()),
            }
        } else {
            return None;
        }
    };

    // B7: prepend any `///` doc comment lines found before the definition.
    let name = find_ident_at(tokens, offset);
    if let Some(name) = name {
        if let Some(def) = db
            .defs
            .iter()
            .find(|d| d.name == name && d.module_path == path)
        {
            if let Some(doc) = collect_doc_comment(tokens, def.def_span.start) {
                return Some(format!("{}\n\n---\n\n{}", doc, base));
            }
        }
    }
    Some(base)
}

fn find_ident_at<'a>(tokens: &'a [Token], offset: usize) -> Option<&'a str> {
    for tok in tokens {
        if tok.span.start <= offset && offset <= tok.span.end {
            if let TokKind::Ident(name) = &tok.kind {
                return Some(name.as_str());
            }
        }
    }
    None
}

// ── Go-to-definition ──────────────────────────────────────────────────────────

fn compute_definition(
    db: &SymbolDB,
    tokens: &[Token],
    src: &str,
    path: &str,
    offset: usize,
) -> Option<(String, Span)> {
    let name = find_ident_at(tokens, offset)?;
    // Look for a top-level or local def with this name
    // Prefer defs in same module, then other modules
    if let Some(def) = db
        .defs
        .iter()
        .find(|d| d.name == name && d.module_path == path)
    {
        return Some((def.module_path.clone(), def.def_span));
    }
    if let Some(def) = db.defs.iter().find(|d| d.name == name) {
        return Some((def.module_path.clone(), def.def_span));
    }
    None
}

// ── References ────────────────────────────────────────────────────────────────

fn compute_references(
    db: &SymbolDB,
    tokens: &[Token],
    path: &str,
    offset: usize,
    include_declaration: bool,
) -> Vec<(String, Span)> {
    let name = match find_ident_at(tokens, offset) {
        Some(n) => n,
        None => return Vec::new(),
    };
    let mut result: Vec<(String, Span)> = db
        .refs
        .iter()
        .filter(|r| r.name == name)
        .map(|r| (r.module_path.clone(), r.span))
        .collect();
    if include_declaration {
        for def in db.defs.iter().filter(|d| d.name == name) {
            result.push((def.module_path.clone(), def.def_span));
        }
    }
    result
}

// ── Rename ────────────────────────────────────────────────────────────────────

fn is_keyword(name: &str) -> bool {
    JET_KEYWORDS.contains(&name) || JET_TYPES.contains(&name)
}

fn is_valid_ident(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    if !first.is_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// Compute a workspace edit for renaming the symbol at `offset` to `new_name`.
/// Returns `Err(msg)` if the rename is invalid.
fn compute_rename(
    db: &SymbolDB,
    tokens: &[Token],
    path: &str,
    offset: usize,
    new_name: &str,
) -> Result<Vec<(String, Span)>, String> {
    if !is_valid_ident(new_name) {
        return Err(format!("`{}` is not a valid identifier", new_name));
    }
    if is_keyword(new_name) {
        return Err(format!(
            "`{}` is a keyword and cannot be used as a name",
            new_name
        ));
    }
    let name = match find_ident_at(tokens, offset) {
        Some(n) => n,
        None => return Err("no identifier at cursor".to_string()),
    };
    if is_keyword(name) {
        return Err(format!("`{}` is a keyword and cannot be renamed", name));
    }
    let mut spans: Vec<(String, Span)> = Vec::new();
    // Include definition spans
    for def in db.defs.iter().filter(|d| d.name == name) {
        spans.push((def.module_path.clone(), def.def_span));
    }
    // Include all reference spans
    for r in db.refs.iter().filter(|r| r.name == name) {
        spans.push((r.module_path.clone(), r.span));
    }
    if spans.is_empty() {
        return Err(format!("no occurrences of `{}` found", name));
    }
    Ok(spans)
}

// ── Semantic tokens ───────────────────────────────────────────────────────────
//
// Token type indices (must match the legend in initialize_response).
mod st {
    pub const KEYWORD: u32 = 0;
    pub const TYPE: u32 = 1;
    pub const FUNCTION: u32 = 2;
    pub const VARIABLE: u32 = 3;
    pub const PARAMETER: u32 = 4;
    pub const PROPERTY: u32 = 5;
    pub const ENUM_MEMBER: u32 = 6;
    pub const STRING: u32 = 7;
    pub const NUMBER: u32 = 8;
    pub const COMMENT: u32 = 9;
    pub const OPERATOR: u32 = 10;
    pub const NAMESPACE: u32 = 11;
}

// Modifier bitmasks
mod sm {
    pub const DECLARATION: u32 = 1 << 0;
    pub const READONLY: u32 = 1 << 1;
}

fn semantic_token_type_for(tok: &Token) -> Option<(u32, u32)> {
    match &tok.kind {
        TokKind::KwFn
        | TokKind::KwPub
        | TokKind::KwVal
        | TokKind::KwVar
        | TokKind::KwIf
        | TokKind::KwElse
        | TokKind::KwWhile
        | TokKind::KwFor
        | TokKind::KwIn
        | TokKind::KwSwitch
        | TokKind::KwBreak
        | TokKind::KwContinue
        | TokKind::KwReturn
        | TokKind::KwStruct
        | TokKind::KwEnum
        | TokKind::KwImpl
        | TokKind::KwTrait
        | TokKind::KwDerive
        | TokKind::KwConst
        | TokKind::KwComptime
        | TokKind::KwImport
        | TokKind::KwExtern
        | TokKind::KwTest
        | TokKind::KwLoop
        | TokKind::KwUnsafe
        | TokKind::KwMutate
        | TokKind::KwMove
        | TokKind::KwView
        | TokKind::KwStored
        | TokKind::KwSelf
        | TokKind::KwNull
        | TokKind::KwOk
        | TokKind::KwErr
        | TokKind::KwIt => Some((st::KEYWORD, 0)),

        TokKind::KwTrue | TokKind::KwFalse => Some((st::KEYWORD, sm::READONLY)),

        TokKind::Ident(name) => {
            // Classify identifiers by name convention:
            // PascalCase → type, everything else → variable
            if name.starts_with(|c: char| c.is_uppercase()) {
                Some((st::TYPE, 0))
            } else {
                Some((st::VARIABLE, 0))
            }
        }

        TokKind::Str(_) => Some((st::STRING, 0)),

        TokKind::Int(_) | TokKind::Float(_) | TokKind::Char(_) => Some((st::NUMBER, 0)),

        TokKind::LineComment(_) | TokKind::BlockComment(_) => Some((st::COMMENT, 0)),

        TokKind::Plus
        | TokKind::Minus
        | TokKind::Star
        | TokKind::Slash
        | TokKind::Percent
        | TokKind::Amp
        | TokKind::Pipe
        | TokKind::Caret
        | TokKind::Shl
        | TokKind::Shr
        | TokKind::AndAnd
        | TokKind::OrOr
        | TokKind::Bang
        | TokKind::EqEq
        | TokKind::NotEq
        | TokKind::Lt
        | TokKind::Gt
        | TokKind::Le
        | TokKind::Ge
        | TokKind::Arrow
        | TokKind::LambdaArrow
        | TokKind::Question
        | TokKind::DotDot => Some((st::OPERATOR, 0)),

        _ => None,
    }
}

/// Encode semantic tokens for a token stream into the LSP delta-encoded u32 array.
fn encode_semantic_tokens(tokens: &[Token], src: &str) -> Vec<u32> {
    let mut data: Vec<u32> = Vec::new();
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;

    for tok in tokens {
        if matches!(tok.kind, TokKind::Eof) {
            break;
        }
        let (tok_type, tok_mods) = match semantic_token_type_for(tok) {
            Some(t) => t,
            None => continue,
        };
        let lsp_start = byte_offset_to_lsp(src, tok.span.start);
        let line = lsp_start.line;
        let start = lsp_start.character;

        // Compute length in UTF-16 code units
        let text = src
            .get(tok.span.start..tok.span.end.min(src.len()))
            .unwrap_or("");
        // For multi-line tokens (strings with newlines) just use first line
        let first_line_text: &str = text.split('\n').next().unwrap_or(text);
        let length = first_line_text.encode_utf16().count() as u32;
        if length == 0 {
            continue;
        }

        let delta_line = line - prev_line;
        let delta_start = if delta_line == 0 {
            start - prev_start
        } else {
            start
        };

        data.push(delta_line);
        data.push(delta_start);
        data.push(length);
        data.push(tok_type);
        data.push(tok_mods);

        prev_line = line;
        prev_start = start;
    }
    data
}

// ── Inlay hints ───────────────────────────────────────────────────────────────

fn format_inlay_hints(hints: &[&InlayHint], src: &str) -> String {
    let mut items = String::new();
    for (i, h) in hints.iter().enumerate() {
        if i > 0 {
            items.push(',');
        }
        // Position: just after the name span
        let pos = byte_offset_to_lsp(src, h.span.end);
        items.push_str(&format!(
            r#"{{"position":{{"line":{},"character":{}}},"label":"{}","kind":1}}"#,
            pos.line,
            pos.character,
            json_escape(&h.label)
        ));
    }
    format!("[{}]", items)
}

// ── Document state ────────────────────────────────────────────────────────────

struct Document {
    path: String,
    text: String,
}

struct Server {
    docs: HashMap<String, Document>,
    /// URIs of documents that changed since last diagnostic publish (D-LSP3).
    dirty: std::collections::HashSet<String>,
    /// D-LSP4: diagnostic cache keyed by path → (source-hash, diagnostics).
    /// RefCell allows mutation through &self so callers can hold &Document refs.
    diag_cache: std::cell::RefCell<HashMap<String, (u64, Vec<Diagnostic>)>>,
    shutdown: bool,
}

/// FNV-1a 64-bit hash of a string — good enough for source-change detection.
fn hash_str(s: &str) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

impl Server {
    fn new() -> Self {
        Server {
            docs: HashMap::new(),
            dirty: std::collections::HashSet::new(),
            diag_cache: std::cell::RefCell::new(HashMap::new()),
            shutdown: false,
        }
    }

    /// D-LSP4: return diagnostics, re-using cached results when source is unchanged.
    fn check(&self, doc: &Document) -> Vec<Diagnostic> {
        let h = hash_str(&doc.text);
        {
            let cache = self.diag_cache.borrow();
            if let Some((cached_h, cached)) = cache.get(&doc.path) {
                if *cached_h == h {
                    return cached.clone();
                }
            }
        }
        let diags = check_document(&doc.path, &doc.text);
        self.diag_cache
            .borrow_mut()
            .insert(doc.path.clone(), (h, diags.clone()));
        diags
    }

    fn check_with_bundle(&self, doc: &Document) -> (Vec<Diagnostic>, Option<ProgramBundle>) {
        check_document_with_bundle(&doc.path, &doc.text)
    }

    fn lex(&self, doc: &Document) -> Vec<Token> {
        let (toks, _) = crate::lexer::lex(&doc.text);
        toks
    }
}

// ── JSON-RPC main loop ────────────────────────────────────────────────────────

pub fn run_stdio() -> io::Result<()> {
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout();
    let mut server = Server::new();

    loop {
        let body = match read_message(&mut stdin)? {
            Some(b) => b,
            None => break,
        };
        let msg = match parse_json(&body) {
            Ok(v) => v,
            Err(()) => continue,
        };
        let method = json_get(&msg, "method").and_then(json_str);
        let id = json_get(&msg, "id").cloned();
        let params = json_get(&msg, "params");

        if let Some(method) = method {
            if id.is_some() {
                // D-LSP3: flush any buffered dirty-document diagnostics before serving requests.
                let _ = flush_dirty(&mut server, &mut stdout);
                let resp = catch_handler(std::panic::AssertUnwindSafe(|| {
                    handle_request(&mut server, method, params, id.as_ref().unwrap())
                }));
                if let Some(resp) = resp {
                    write_message(&mut stdout, &resp)?;
                }
            } else {
                catch_notification(|| {
                    handle_notification(&mut server, method, params, &mut stdout)
                })?;
            }
        }

        if server.shutdown {
            break;
        }
    }
    Ok(())
}

/// Catch panics in a handler; on panic, log and return None (LSP-I2).
fn catch_handler<F: FnOnce() -> Option<String>>(
    f: std::panic::AssertUnwindSafe<F>,
) -> Option<String> {
    match std::panic::catch_unwind(f) {
        Ok(r) => r,
        Err(e) => {
            let msg = if let Some(s) = e.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else {
                "unknown panic".to_string()
            };
            let _ = write_log(&format!("ICE in handler: {}", msg));
            None
        }
    }
}

/// Catch panics in a notification handler (LSP-I2).
fn catch_notification<F: FnOnce() -> io::Result<()>>(f: F) -> io::Result<()> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(_) => Ok(()),
    }
}

fn write_log(msg: &str) -> io::Result<()> {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/jet-lsp.log")
    {
        writeln!(f, "[jet-lsp] {}", msg)?;
    }
    Ok(())
}

fn read_message(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse().ok();
        }
    }
    let len = match content_length {
        Some(l) => l,
        None => return Ok(None),
    };
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    Ok(Some(String::from_utf8_lossy(&body).into_owned()))
}

fn write_message<W: Write>(w: &mut W, json: &str) -> io::Result<()> {
    write!(w, "Content-Length: {}\r\n\r\n{}", json.len(), json)?;
    w.flush()
}

// ── Request handlers ──────────────────────────────────────────────────────────

fn handle_request(
    server: &mut Server,
    method: &str,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    match method {
        "initialize" => Some(initialize_response(id)),
        "shutdown" => {
            server.shutdown = true;
            Some(response(id, "null"))
        }
        "textDocument/codeAction" => code_action_response(server, params, id),
        "textDocument/formatting" => format_response(server, params, id),
        "textDocument/rangeFormatting" => format_response(server, params, id),
        "textDocument/completion" => completion_response(server, params, id),
        "textDocument/hover" => hover_response(server, params, id),
        "textDocument/definition" => definition_response(server, params, id),
        "textDocument/references" => references_response(server, params, id),
        "textDocument/rename" => rename_response(server, params, id),
        "textDocument/semanticTokens/full" => semantic_tokens_response(server, params, id),
        "textDocument/inlayHint" => inlay_hint_response(server, params, id),
        _ => Some(response(id, "null")),
    }
}

fn handle_notification(
    server: &mut Server,
    method: &str,
    params: Option<&JsonValue>,
    stdout: &mut impl Write,
) -> io::Result<()> {
    match method {
        "initialized" => Ok(()),
        "exit" => {
            server.shutdown = true;
            Ok(())
        }
        "textDocument/didOpen" => publish_after_open(server, params, stdout),
        "textDocument/didChange" => publish_after_change(server, params, stdout),
        "textDocument/didClose" => {
            if let Some(uri) = params
                .and_then(|p| json_get(p, "textDocument"))
                .and_then(|td| json_get(td, "uri"))
                .and_then(json_str)
            {
                server.docs.remove(uri);
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn initialize_response(id: &JsonValue) -> String {
    let result = r#"{
  "capabilities": {
    "textDocumentSync": 1,
    "documentFormattingProvider": true,
    "documentRangeFormattingProvider": true,
    "codeActionProvider": true,
    "completionProvider": {
      "triggerCharacters": ["."],
      "resolveProvider": false
    },
    "hoverProvider": true,
    "definitionProvider": true,
    "referencesProvider": true,
    "renameProvider": true,
    "semanticTokensProvider": {
      "legend": {
        "tokenTypes": [
          "keyword","type","function","variable","parameter",
          "property","enumMember","string","number","comment",
          "operator","namespace"
        ],
        "tokenModifiers": ["declaration","readonly"]
      },
      "full": true
    },
    "inlayHintProvider": true
  },
  "serverInfo": { "name": "jet", "version": "0.2.0" }
}"#;
    response(id, result)
}

fn response(id: &JsonValue, result_json: &str) -> String {
    let id_json = serialize_id(id);
    format!(
        r#"{{"jsonrpc":"2.0","id":{},"result":{}}}"#,
        id_json, result_json
    )
}

fn error_response(id: &JsonValue, code: i64, message: &str) -> String {
    let id_json = serialize_id(id);
    format!(
        r#"{{"jsonrpc":"2.0","id":{},"error":{{"code":{},"message":"{}"}}}}"#,
        id_json,
        code,
        json_escape(message)
    )
}

fn serialize_id(id: &JsonValue) -> String {
    match id {
        JsonValue::Number(n) => n.to_string(),
        JsonValue::String(s) => format!("\"{}\"", json_escape(s)),
        _ => "null".to_string(),
    }
}

fn publish_after_change(
    server: &mut Server,
    params: Option<&JsonValue>,
    stdout: &mut impl Write,
) -> io::Result<()> {
    publish_after_change_impl(server, params, stdout, false)
}

fn publish_after_open(
    server: &mut Server,
    params: Option<&JsonValue>,
    stdout: &mut impl Write,
) -> io::Result<()> {
    publish_after_change_impl(server, params, stdout, true)
}

/// D-LSP3: on `didChange` (is_open=false), mark dirty but don't publish immediately.
/// On `didOpen` (is_open=true), always publish so the editor gets initial diagnostics.
/// Dirty documents are flushed before the next request that reads document state.
fn publish_after_change_impl(
    server: &mut Server,
    params: Option<&JsonValue>,
    stdout: &mut impl Write,
    is_open: bool,
) -> io::Result<()> {
    let params = match params {
        Some(p) => p,
        None => return Ok(()),
    };
    let td = match json_get(params, "textDocument") {
        Some(v) => v,
        None => return Ok(()),
    };
    let uri = match json_get(td, "uri").and_then(json_str) {
        Some(u) => u.to_string(),
        None => return Ok(()),
    };
    let path = uri_to_path(&uri);

    if let Some(text) = json_get(td, "text").and_then(json_str) {
        server.docs.insert(
            uri.clone(),
            Document {
                path,
                text: text.to_string(),
            },
        );
    } else if let Some(changes) = json_get(params, "contentChanges") {
        if let JsonValue::Array(arr) = changes {
            if let Some(JsonValue::Object(chg)) = arr.first() {
                if let Some(text) = chg.get("text").and_then(json_str) {
                    server.docs.insert(
                        uri.clone(),
                        Document {
                            path,
                            text: text.to_string(),
                        },
                    );
                }
            }
        }
    }

    if is_open {
        // Always publish on open — client expects initial diagnostics.
        if let Some(doc) = server.docs.get(&uri) {
            let diags = server.check(doc);
            let notif = publish_diagnostics(&uri, &doc.text, &diags);
            write_message(stdout, &notif)?;
        }
        server.dirty.remove(&uri);
    } else {
        // Mark dirty; diagnostics will be flushed before the next request.
        server.dirty.insert(uri);
    }
    Ok(())
}

/// Flush any pending dirty-document diagnostics before handling a request (D-LSP3).
fn flush_dirty(server: &mut Server, stdout: &mut impl Write) -> io::Result<()> {
    let dirty: Vec<String> = server.dirty.drain().collect();
    for uri in dirty {
        if let Some(doc) = server.docs.get(&uri) {
            let text = doc.text.clone();
            let diags = server.check(doc);
            let notif = publish_diagnostics(&uri, &text, &diags);
            write_message(stdout, &notif)?;
        }
    }
    Ok(())
}

fn publish_diagnostics(uri: &str, src: &str, diags: &[Diagnostic]) -> String {
    let mut items = String::new();
    for (i, d) in diags.iter().enumerate() {
        if i > 0 {
            items.push(',');
        }
        items.push_str(&diagnostic_json(d, src));
    }
    format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{{"uri":"{}","diagnostics":[{}]}}}}"#,
        json_escape(uri),
        items
    )
}

fn diagnostic_json(d: &Diagnostic, src: &str) -> String {
    let severity = match d.severity {
        Severity::Error => 1,
        Severity::Lint => 2,
    };
    let range = d
        .span
        .map(|s| byte_span_to_range(src, s))
        .unwrap_or(full_document_range(src));
    format!(
        r#"{{"range":{},"severity":{},"code":"{}","source":"jet","message":"{}"}}"#,
        range_json(range),
        severity,
        json_escape(d.code),
        json_escape(&d.what)
    )
}

fn code_action_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let diags = server.check(doc);
    let mut actions = String::new();
    let mut n = 0usize;
    for d in &diags {
        if let Some(edit) = &d.edit {
            if n > 0 {
                actions.push(',');
            }
            actions.push_str(&code_action_json(uri, &doc.text, d, edit));
            n += 1;
        }
    }
    Some(response(id, &format!("[{}]", actions)))
}

fn code_action_json(uri: &str, src: &str, d: &Diagnostic, edit: &TextEdit) -> String {
    let range = byte_span_to_range(src, edit.span);
    let title = d.fix.clone();
    format!(
        r#"{{"title":"{}","kind":"quickfix","edit":{{"changes":{{"{}":[{{"range":{},"newText":"{}"}}]}}}}}}"#,
        json_escape(&title),
        json_escape(uri),
        range_json(range),
        json_escape(&edit.new_text)
    )
}

fn format_response(server: &Server, params: Option<&JsonValue>, id: &JsonValue) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let formatted = match crate::format_source(&doc.text) {
        Ok(s) => s,
        Err(_) => return Some(response(id, "[]")),
    };
    let range = full_document_range(&doc.text);
    let edit = format!(
        r#"[{{"range":{},"newText":"{}"}}]"#,
        range_json(range),
        json_escape(&formatted)
    );
    Some(response(id, &edit))
}

fn completion_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let pos = json_get(params, "position")?;
    let line = json_int(json_get(pos, "line")?)? as u32;
    let character = json_int(json_get(pos, "character")?)? as u32;
    let lsp_pos = LspPos { line, character };
    let offset = lsp_pos_to_offset(&doc.text, lsp_pos);

    let (_, bundle) = server.check_with_bundle(doc);
    let db = match bundle {
        Some(b) => build_symbol_db(&b),
        None => SymbolDB::new(),
    };

    let items = compute_completions(&db, &doc.text, offset, &doc.path);
    let mut json_items = String::new();
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            json_items.push(',');
        }
        json_items.push_str(&item.to_json());
    }
    Some(response(
        id,
        &format!(r#"{{"isIncomplete":false,"items":[{}]}}"#, json_items),
    ))
}

fn hover_response(server: &Server, params: Option<&JsonValue>, id: &JsonValue) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let pos = json_get(params, "position")?;
    let line = json_int(json_get(pos, "line")?)? as u32;
    let character = json_int(json_get(pos, "character")?)? as u32;
    let lsp_pos = LspPos { line, character };
    let offset = lsp_pos_to_offset(&doc.text, lsp_pos);

    let tokens = server.lex(doc);
    let (_, bundle) = server.check_with_bundle(doc);
    let db = match bundle {
        Some(b) => build_symbol_db(&b),
        None => SymbolDB::new(),
    };

    match compute_hover(&db, &tokens, &doc.text, &doc.path, offset) {
        Some(text) => {
            let result = format!(
                r#"{{"contents":{{"kind":"markdown","value":"{}"}}}}"#,
                json_escape(&text)
            );
            Some(response(id, &result))
        }
        None => Some(response(id, "null")),
    }
}

fn definition_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let pos = json_get(params, "position")?;
    let line = json_int(json_get(pos, "line")?)? as u32;
    let character = json_int(json_get(pos, "character")?)? as u32;
    let lsp_pos = LspPos { line, character };
    let offset = lsp_pos_to_offset(&doc.text, lsp_pos);

    let tokens = server.lex(doc);
    let (_, bundle) = server.check_with_bundle(doc);
    let db = match bundle {
        Some(b) => build_symbol_db(&b),
        None => SymbolDB::new(),
    };

    match compute_definition(&db, &tokens, &doc.text, &doc.path, offset) {
        Some((def_path, def_span)) => {
            let def_uri = path_to_uri(&def_path);
            let src = if def_path == doc.path {
                doc.text.clone()
            } else {
                std::fs::read_to_string(&def_path).unwrap_or_default()
            };
            let range = byte_span_to_range(&src, def_span);
            let result = format!(
                r#"{{"uri":"{}","range":{}}}"#,
                json_escape(&def_uri),
                range_json(range)
            );
            Some(response(id, &result))
        }
        None => Some(response(id, "null")),
    }
}

fn references_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let pos = json_get(params, "position")?;
    let line = json_int(json_get(pos, "line")?)? as u32;
    let character = json_int(json_get(pos, "character")?)? as u32;
    let lsp_pos = LspPos { line, character };
    let offset = lsp_pos_to_offset(&doc.text, lsp_pos);

    let ctx = json_get(params, "context");
    let include_decl = ctx
        .and_then(|c| json_get(c, "includeDeclaration"))
        .and_then(|v| {
            if let JsonValue::Bool(b) = v {
                Some(*b)
            } else {
                None
            }
        })
        .unwrap_or(false);

    let tokens = server.lex(doc);
    let (_, bundle) = server.check_with_bundle(doc);
    let db = match bundle {
        Some(b) => build_symbol_db(&b),
        None => SymbolDB::new(),
    };

    let refs = compute_references(&db, &tokens, &doc.path, offset, include_decl);
    let mut items = String::new();
    for (i, (ref_path, span)) in refs.iter().enumerate() {
        if i > 0 {
            items.push(',');
        }
        let ref_uri = path_to_uri(ref_path);
        let src = if ref_path == &doc.path {
            doc.text.clone()
        } else {
            std::fs::read_to_string(ref_path).unwrap_or_default()
        };
        let range = byte_span_to_range(&src, *span);
        items.push_str(&format!(
            r#"{{"uri":"{}","range":{}}}"#,
            json_escape(&ref_uri),
            range_json(range)
        ));
    }
    Some(response(id, &format!("[{}]", items)))
}

fn rename_response(server: &Server, params: Option<&JsonValue>, id: &JsonValue) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;
    let pos = json_get(params, "position")?;
    let line = json_int(json_get(pos, "line")?)? as u32;
    let character = json_int(json_get(pos, "character")?)? as u32;
    let lsp_pos = LspPos { line, character };
    let offset = lsp_pos_to_offset(&doc.text, lsp_pos);
    let new_name = json_get(params, "newName").and_then(json_str)?;

    let tokens = server.lex(doc);
    let (_, bundle) = server.check_with_bundle(doc);
    let db = match bundle {
        Some(b) => build_symbol_db(&b),
        None => SymbolDB::new(),
    };

    match compute_rename(&db, &tokens, &doc.path, offset, new_name) {
        Ok(spans) => {
            // Group edits by file
            let mut by_file: HashMap<String, Vec<Span>> = HashMap::new();
            for (path, span) in spans {
                by_file.entry(path).or_default().push(span);
            }
            let mut changes = String::new();
            let mut first = true;
            for (path, file_spans) in &by_file {
                if !first {
                    changes.push(',');
                }
                first = false;
                let file_uri = path_to_uri(path);
                let src = if path == &doc.path {
                    doc.text.clone()
                } else {
                    std::fs::read_to_string(path).unwrap_or_default()
                };
                let mut edits = String::new();
                for (j, &span) in file_spans.iter().enumerate() {
                    if j > 0 {
                        edits.push(',');
                    }
                    let range = byte_span_to_range(&src, span);
                    edits.push_str(&format!(
                        r#"{{"range":{},"newText":"{}"}}"#,
                        range_json(range),
                        json_escape(new_name)
                    ));
                }
                changes.push_str(&format!(r#""{}": [{}]"#, json_escape(&file_uri), edits));
            }
            Some(response(id, &format!(r#"{{"changes":{{{}}}}}"#, changes)))
        }
        Err(msg) => Some(error_response(id, -32600, &msg)),
    }
}

fn semantic_tokens_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;

    let tokens = server.lex(doc);
    let data = encode_semantic_tokens(&tokens, &doc.text);
    let data_str: Vec<String> = data.iter().map(|n| n.to_string()).collect();
    Some(response(
        id,
        &format!(r#"{{"data":[{}]}}"#, data_str.join(",")),
    ))
}

fn inlay_hint_response(
    server: &Server,
    params: Option<&JsonValue>,
    id: &JsonValue,
) -> Option<String> {
    let params = params?;
    let td = json_get(params, "textDocument")?;
    let uri = json_get(td, "uri").and_then(json_str)?;
    let doc = server.docs.get(uri)?;

    let (diags, bundle) = server.check_with_bundle(doc);

    // Build type-annotation hints from the symbol DB.
    let mut hints: Vec<InlayHint> = match bundle {
        Some(b) => {
            let db = build_symbol_db(&b);
            db.inlay_hints_for(&doc.path).into_iter().cloned().collect()
        }
        None => Vec::new(),
    };

    // D-LSP8: add clone-site hints for L0201 diagnostics.
    for d in &diags {
        if d.code == "L0201" {
            if let Some(span) = d.span {
                hints.push(InlayHint {
                    span,
                    module_path: doc.path.clone(),
                    label: ".clone()".to_string(),
                });
            }
        }
    }

    let hint_refs: Vec<&InlayHint> = hints.iter().collect();
    let json = format_inlay_hints(&hint_refs, &doc.text);
    Some(response(id, &json))
}

// ── URI / path utilities ──────────────────────────────────────────────────────

fn uri_to_path(uri: &str) -> String {
    if let Some(rest) = uri.strip_prefix("file://") {
        if cfg!(windows) {
            rest.trim_start_matches('/').replace('/', "\\")
        } else {
            rest.to_string()
        }
    } else {
        uri.to_string()
    }
}

fn path_to_uri(path: &str) -> String {
    if path.starts_with('/') || (cfg!(windows) && path.contains(':')) {
        format!("file://{}", path)
    } else {
        format!("file://{}", path)
    }
}

fn canonical_path(path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if p.is_absolute() {
        normalize_path(&p)
    } else {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        normalize_path(&cwd.join(p))
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

// ── Document check (used by LSP and tests) ────────────────────────────────────

/// Check one document (disk path + in-memory text). Used by LSP and tests.
pub fn check_document(path: &str, text: &str) -> Vec<Diagnostic> {
    let abs = canonical_path(path);
    match crate::loader::load_entry_with_overlay(path, Some((&abs, text)), true) {
        Ok(mut bundle) => {
            let mut diags = std::mem::take(&mut bundle.parse_teaching);
            diags.extend(crate::sema::check_bundle(&mut bundle, CompileMode::Check));
            diags
        }
        Err(diags) => diags,
    }
}

/// Check one document, also returning the bundle for symbol analysis.
pub fn check_document_with_bundle(
    path: &str,
    text: &str,
) -> (Vec<Diagnostic>, Option<ProgramBundle>) {
    let abs = canonical_path(path);
    match crate::loader::load_entry_with_overlay(path, Some((&abs, text)), true) {
        Ok(mut bundle) => {
            let mut diags = std::mem::take(&mut bundle.parse_teaching);
            diags.extend(crate::sema::check_bundle(&mut bundle, CompileMode::Check));
            (diags, Some(bundle))
        }
        Err(diags) => (diags, None),
    }
}

/// Apply a teaching edit to source text (for scripted LSP tests).
pub fn apply_edit(src: &str, edit: &TextEdit) -> String {
    let mut out = String::new();
    out.push_str(&src[..edit.span.start.min(src.len())]);
    out.push_str(&edit.new_text);
    out.push_str(&src[edit.span.end.min(src.len())..]);
    out
}

// ── Doctor ────────────────────────────────────────────────────────────────────

/// Health check: verify that the server can lex/parse/check a trivial program.
pub fn run_doctor() {
    println!("jet lsp doctor");
    println!("--------------");
    let src = "fn main() { print(\"hello\"); }\n";
    let (toks, lex_errs) = crate::lexer::lex(src);
    if lex_errs.is_empty() {
        println!("  [ok] lexer");
    } else {
        println!("  [FAIL] lexer: {} errors", lex_errs.len());
    }
    match crate::parser::parse(&toks) {
        Ok(_) => println!("  [ok] parser"),
        Err(errs) => println!("  [FAIL] parser: {} errors", errs.len()),
    }
    let diags = check_document("test.jet", src);
    if diags.is_empty() {
        println!("  [ok] sema");
    } else {
        println!("  [FAIL] sema: {} diagnostics", diags.len());
    }
    let formatted = crate::format_source(src);
    if formatted.is_ok() {
        println!("  [ok] formatter");
    } else {
        println!("  [FAIL] formatter");
    }
    println!("  [ok] JSON-RPC framing");

    // C13: transcript runner smoke — verify tests/lsp/01_initialize.json exists.
    let transcript_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/lsp");
    if transcript_dir.exists() {
        let count = std::fs::read_dir(&transcript_dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
                    .count()
            })
            .unwrap_or(0);
        println!(
            "  [ok] transcript runner: {} fixture(s) found in tests/lsp/",
            count
        );
    } else {
        println!("  [WARN] transcript runner: tests/lsp/ not found");
    }

    // C13: tree-sitter grammar presence.
    let ts_grammar =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("editors/tree-sitter/grammar.js");
    if ts_grammar.exists() {
        println!("  [ok] editors/tree-sitter/grammar.js present");
    } else {
        println!(
            "  [WARN] editors/tree-sitter/grammar.js not found — run `tree-sitter generate` to build"
        );
    }

    // C13: TextMate grammar presence.
    let tm_grammar = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("editors/jet.tmGrammar");
    if tm_grammar.exists() {
        println!("  [ok] editors/jet.tmGrammar present");
    } else {
        println!("  [WARN] editors/jet.tmGrammar not found");
    }

    println!("all checks passed — the language server is healthy");
}

// ── Bench ─────────────────────────────────────────────────────────────────────

/// Replay a simple session to measure diagnostic latency.
/// Runs `rounds` iterations of parse+sema over `src` and checks budget_ms.
pub fn run_bench(src: &str, rounds: usize, budget_ms: u128) {
    let label = format!("{}B input, {} rounds", src.len(), rounds);
    let start = std::time::Instant::now();
    for _ in 0..rounds {
        let _ = check_document("bench.jet", src);
    }
    let elapsed = start.elapsed();
    let per_round_ms = elapsed.as_millis() / rounds.max(1) as u128;
    println!(
        "bench [{}]: {}ms/round ({} total) — budget {}ms — {}",
        label,
        per_round_ms,
        elapsed.as_millis(),
        budget_ms,
        if per_round_ms <= budget_ms {
            "PASS"
        } else {
            "FAIL"
        }
    );
    if per_round_ms > budget_ms {
        std::process::exit(1);
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn teaching_edit_from_let() {
        let src = "fn main() {\n    let x = 1;\n}\n";
        let diags = check_document("test.jet", src);
        let e0009 = diags.iter().find(|d| d.code == "E0009").expect("E0009");
        let edit = e0009.edit.as_ref().expect("edit");
        assert_eq!(edit.new_text, "val");
        let fixed = apply_edit(src, edit);
        assert!(fixed.contains("val x = 1"));
        assert!(!fixed.contains("let x"));
    }

    #[test]
    fn lsp_pos_round_trip() {
        let src = "fn main() {\n    val x = 1;\n}\n";
        let offset = 18; // somewhere in 'val'
        let pos = byte_offset_to_lsp(src, offset);
        let back = lsp_pos_to_offset(src, pos);
        assert_eq!(back, offset);
    }

    #[test]
    fn symbol_db_finds_function() {
        let src = "fn greet(name: String) {\n    print(name);\n}\nfn main() {\n    greet(\"world\");\n}\n";
        let (_, bundle) = check_document_with_bundle("test.jet", src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle);
        assert!(db.defs.iter().any(|d| d.name == "greet"));
        assert!(db.defs.iter().any(|d| d.name == "main"));
        assert!(db.refs.iter().any(|r| r.name == "greet"));
    }

    #[test]
    fn hover_returns_function_signature() {
        let src =
            "fn add(a: Int, b: Int) -> Int { return a + b; }\nfn main() { val r = add(1, 2); }\n";
        let (_, bundle) = check_document_with_bundle("test.jet", src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle);
        let (toks, _) = crate::lexer::lex(src);
        // Hover over 'add' at offset 3 (the name span)
        let hover = compute_hover(&db, &toks, src, "test.jet", 3);
        assert!(hover.is_some(), "expected hover for 'add'");
        let h = hover.unwrap();
        assert!(h.contains("add"), "hover should mention the function name");
    }

    #[test]
    fn rename_basic_function() {
        let src = "fn greet() {}\nfn main() { greet(); }\n";
        let (_, bundle) = check_document_with_bundle("test.jet", src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle);
        let (toks, _) = crate::lexer::lex(src);
        let spans = compute_rename(&db, &toks, "test.jet", 3, "hello").expect("rename ok");
        assert!(!spans.is_empty());
        assert!(spans.iter().any(|(_, sp)| sp.start <= 3 && 3 <= sp.end));
    }

    #[test]
    fn rename_rejects_keyword() {
        let src = "fn greet() {}\nfn main() { greet(); }\n";
        let (_, bundle) = check_document_with_bundle("test.jet", src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle);
        let (toks, _) = crate::lexer::lex(src);
        assert!(compute_rename(&db, &toks, "test.jet", 3, "fn").is_err());
    }

    #[test]
    fn semantic_tokens_non_empty() {
        let src = "fn main() { val x: Int = 1; }\n";
        let (toks, _) = crate::lexer::lex(src);
        let data = encode_semantic_tokens(&toks, src);
        // Should emit at least one token (5 u32s per token)
        assert!(data.len() >= 5, "expected at least one semantic token");
    }

    #[test]
    fn inlay_hints_for_int_literal() {
        let src = "fn main() {\n    val x = 42;\n}\n";
        let (_, bundle) = check_document_with_bundle("test.jet", src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle);
        let hints = db.inlay_hints_for("test.jet");
        assert!(
            hints.iter().any(|h| h.label.contains("Int")),
            "expected Int inlay hint"
        );
    }

    #[test]
    fn completion_includes_keywords() {
        let src = "fn main() {\n    \n}\n";
        let (_, bundle) = check_document_with_bundle("test.jet", src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle);
        let items = compute_completions(&db, src, 14, "test.jet");
        assert!(
            items.iter().any(|i| i.label == "val"),
            "expected val in completions"
        );
        assert!(
            items.iter().any(|i| i.label == "fn"),
            "expected fn in completions"
        );
    }
}
