//! Public read-only front-end toolkit API (D-FRONTENDAPI1=A).

use crate::Diagnostics::{span_line_col, Diagnostic, Severity, Span};
use crate::Lexer::{TokKind, Token};
use crate::{Lexer, Parser, AST};

pub const API_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRange {
    pub start: usize,
    pub end: usize,
}

impl From<Span> for TextRange {
    fn from(span: Span) -> Self {
        TextRange {
            start: span.start,
            end: span.end,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCol {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenView {
    pub kind: &'static str,
    pub text: String,
    pub span: TextRange,
    pub start: LineCol,
    pub end: LineCol,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Lint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticView {
    pub code: String,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub why: String,
    pub fix: String,
    pub span: Option<TextRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxNodeKind {
    Function,
    Struct,
    Enum,
    Trait,
    Tag,
    Impl,
    Const,
    Test,
    Bench,
    ExternRust,
    Module,
    CModule,
    CodeModule,
    ErrorConversion,
    Migration,
    State,
    Protocol,
    Derive,
    GenericModule,
    ModuleAlias,
    Distinct,
    TypeAlias,
    UnitFamily,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxNode {
    pub kind: SyntaxNodeKind,
    pub name: Option<String>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxTree {
    pub api_version: u32,
    pub items: Vec<SyntaxNode>,
    pub diagnostics: Vec<DiagnosticView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexedSource {
    pub api_version: u32,
    pub tokens: Vec<TokenView>,
    pub diagnostics: Vec<DiagnosticView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMap {
    pub api_version: u32,
    pub sources: Vec<String>,
    pub generated_lines: Vec<GeneratedLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedLine {
    pub generated_line: usize,
    pub source: Option<String>,
    pub source_line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemIndexView {
    pub schema_version: u32,
    pub definitions: Vec<jet_semindex::SymbolDef>,
    pub references: Vec<jet_semindex::SymbolRef>,
    pub calls: Vec<jet_semindex::CallEdge>,
    pub effects: Vec<jet_semindex::EffectFact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedFile {
    pub api_version: u32,
    pub diagnostics: Vec<DiagnosticView>,
    pub syntax: Option<SyntaxTree>,
    pub semantic_index: Option<SemIndexView>,
}

pub fn lex_source(src: &str) -> LexedSource {
    let (tokens, diagnostics) = Lexer::lex(src);
    LexedSource {
        api_version: API_VERSION,
        tokens: tokens.iter().map(|token| token_view(src, token)).collect(),
        diagnostics: diagnostics.iter().map(diagnostic_view).collect(),
    }
}

pub fn parse_source(src: &str) -> SyntaxTree {
    let lexed = lex_source(src);
    if !lexed.diagnostics.is_empty() {
        return SyntaxTree {
            api_version: API_VERSION,
            items: Vec::new(),
            diagnostics: lexed.diagnostics,
        };
    }

    let (tokens, _) = Lexer::lex(src);
    match Parser::parse_for_check(&tokens) {
        Ok((program, parse_teaching)) => SyntaxTree {
            api_version: API_VERSION,
            items: program.items.iter().map(item_node).collect(),
            diagnostics: parse_teaching.iter().map(diagnostic_view).collect(),
        },
        Err(diagnostics) => SyntaxTree {
            api_version: API_VERSION,
            items: Vec::new(),
            diagnostics: diagnostics.iter().map(diagnostic_view).collect(),
        },
    }
}

pub fn check_file(path: &std::path::Path) -> CheckedFile {
    let file = path.to_string_lossy();
    let (diagnostics, bundle, facts) =
        crate::Driver::check_file_with_effect_facts(&file, None, true);
    let has_errors = diagnostics.iter().any(|d| d.severity == Severity::Error);
    let syntax = bundle.as_ref().map(bundle_syntax_tree);
    let semantic_index = if has_errors {
        None
    } else {
        bundle
            .as_ref()
            .map(|bundle| SemIndexView::from(jet_semindex::from_checked(bundle, &facts)))
    };
    CheckedFile {
        api_version: API_VERSION,
        diagnostics: diagnostics.iter().map(diagnostic_view).collect(),
        syntax,
        semantic_index,
    }
}

pub fn source_map_from_generated_rust(rust_src: &str) -> SourceMap {
    let mut sources = Vec::new();
    let mut current_source = None;
    let mut generated_lines = Vec::new();
    for (idx, line) in rust_src.lines().enumerate() {
        let generated_line = idx + 1;
        let trimmed = line.trim_start();
        if let Some(source) = trimmed.strip_prefix("// jet:source-map source=") {
            current_source = Some(source.to_string());
            if !sources.iter().any(|s| s == source) {
                sources.push(source.to_string());
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("// jet:line ") {
            if let Ok(source_line) = rest.trim().parse::<usize>() {
                generated_lines.push(GeneratedLine {
                    generated_line,
                    source: current_source.clone(),
                    source_line,
                });
            }
        }
    }
    SourceMap {
        api_version: API_VERSION,
        sources,
        generated_lines,
    }
}

impl From<jet_semindex::SemIndex> for SemIndexView {
    fn from(index: jet_semindex::SemIndex) -> Self {
        SemIndexView {
            schema_version: index.schema_version(),
            definitions: index.definitions().to_vec(),
            references: index.references().to_vec(),
            calls: index.call_edges().to_vec(),
            effects: index.effects().to_vec(),
        }
    }
}

fn token_view(src: &str, token: &Token) -> TokenView {
    let start = line_col(src, token.span.start);
    let end = line_col(src, token.span.end);
    TokenView {
        kind: token_kind_name(&token.kind),
        text: token_text(src, token),
        span: token.span.into(),
        start,
        end,
    }
}

fn diagnostic_view(diagnostic: &Diagnostic) -> DiagnosticView {
    DiagnosticView {
        code: diagnostic.code.to_string(),
        severity: match diagnostic.severity {
            Severity::Error => DiagnosticSeverity::Error,
            Severity::Lint => DiagnosticSeverity::Lint,
        },
        message: diagnostic.what.clone(),
        why: diagnostic.why.clone(),
        fix: diagnostic.fix.clone(),
        span: diagnostic.span.map(Into::into),
    }
}

fn bundle_syntax_tree(bundle: &AST::ProgramBundle) -> SyntaxTree {
    let mut items = Vec::new();
    for module in &bundle.modules {
        items.extend(module.items.iter().map(item_node));
    }
    SyntaxTree {
        api_version: API_VERSION,
        items,
        diagnostics: Vec::new(),
    }
}

fn item_node(item: &AST::Item) -> SyntaxNode {
    let (kind, name, span) = match item {
        AST::Item::Func(f) => (SyntaxNodeKind::Function, Some(f.name.clone()), f.name_span),
        AST::Item::Struct(s) => (SyntaxNodeKind::Struct, Some(s.name.clone()), s.name_span),
        AST::Item::Enum(e) => (SyntaxNodeKind::Enum, Some(e.name.clone()), e.name_span),
        AST::Item::Distinct(d) => (SyntaxNodeKind::Distinct, Some(d.name.clone()), d.name_span),
        AST::Item::TypeAlias(a) => (SyntaxNodeKind::TypeAlias, Some(a.name.clone()), a.name_span),
        AST::Item::UnitFamily(f) => (
            SyntaxNodeKind::UnitFamily,
            Some(f.family.clone()),
            f.family_span,
        ),
        AST::Item::Trait(t) => (SyntaxNodeKind::Trait, Some(t.name.clone()), t.name_span),
        AST::Item::Tag(t) => (SyntaxNodeKind::Tag, Some(t.name.clone()), t.name_span),
        AST::Item::Impl(i) => (SyntaxNodeKind::Impl, Some(i.type_name.clone()), i.type_span),
        AST::Item::Const(c) => (SyntaxNodeKind::Const, Some(c.name.clone()), c.name_span),
        AST::Item::Test(t) => (SyntaxNodeKind::Test, Some(t.name.clone()), t.name_span),
        AST::Item::Bench(b) => (SyntaxNodeKind::Bench, Some(b.name.clone()), b.name_span),
        AST::Item::ExternRust(e) => (
            SyntaxNodeKind::ExternRust,
            Some(e.crate_spec.clone()),
            e.span,
        ),
        AST::Item::Module(m) => (SyntaxNodeKind::Module, Some(m.name.clone()), m.name_span),
        AST::Item::CModule(m) => (SyntaxNodeKind::CModule, Some(m.lib.clone()), m.path_span),
        AST::Item::CodeModule(m) => (
            SyntaxNodeKind::CodeModule,
            Some(m.name.clone()),
            m.name_span,
        ),
        AST::Item::ErrorConv(e) => (
            SyntaxNodeKind::ErrorConversion,
            Some(format!("{} -> {}", e.from_ty, e.to_ty)),
            e.from_span,
        ),
        AST::Item::Migration(m) => (
            SyntaxNodeKind::Migration,
            Some(m.type_name.clone()),
            m.type_span,
        ),
        AST::Item::StateDecl(s) => (
            SyntaxNodeKind::State,
            Some(s.type_name.clone()),
            s.type_name_span,
        ),
        AST::Item::ProtocolDecl(p) => (SyntaxNodeKind::Protocol, Some(p.name.clone()), p.name_span),
        AST::Item::UserDerive(d) => (
            SyntaxNodeKind::Derive,
            Some(d.trait_name.clone()),
            d.trait_span,
        ),
        AST::Item::GenericModule(m) => (
            SyntaxNodeKind::GenericModule,
            Some(m.name.clone()),
            m.name_span,
        ),
        AST::Item::ModuleAlias(m) => (
            SyntaxNodeKind::ModuleAlias,
            Some(m.name.clone()),
            m.name_span,
        ),
    };
    SyntaxNode {
        kind,
        name,
        span: span.into(),
    }
}

fn token_text(src: &str, token: &Token) -> String {
    if token.span.start <= token.span.end && token.span.end <= src.len() {
        src[token.span.start..token.span.end].to_string()
    } else {
        String::new()
    }
}

fn line_col(src: &str, offset: usize) -> LineCol {
    let (line, column) = span_line_col(src, offset);
    LineCol { line, column }
}

fn token_kind_name(kind: &TokKind) -> &'static str {
    match kind {
        TokKind::KwFn => "keyword.fn",
        TokKind::KwPub => "keyword.pub",
        TokKind::KwPriv => "keyword.priv",
        TokKind::KwIf => "keyword.if",
        TokKind::KwElse => "keyword.else",
        TokKind::KwWhile => "keyword.while",
        TokKind::KwFor => "keyword.for",
        TokKind::KwIn => "keyword.in",
        TokKind::KwSwitch => "keyword.switch",
        TokKind::KwBreak => "keyword.break",
        TokKind::KwContinue => "keyword.continue",
        TokKind::KwTrue => "literal.true",
        TokKind::KwFalse => "literal.false",
        TokKind::KwMutate => "keyword.mutate",
        TokKind::KwMove => "keyword.move",
        TokKind::KwView => "keyword.view",
        TokKind::KwCopy => "keyword.copy",
        TokKind::KwStruct => "keyword.struct",
        TokKind::KwEnum => "keyword.enum",
        TokKind::KwImpl => "keyword.impl",
        TokKind::KwTrait => "keyword.trait",
        TokKind::KwTag => "keyword.tag",
        TokKind::KwDerive => "keyword.derive",
        TokKind::KwSelf => "keyword.self",
        TokKind::KwNull => "literal.null",
        TokKind::KwOk => "literal.ok",
        TokKind::KwErr => "literal.err",
        TokKind::KwIt => "keyword.it",
        TokKind::KwConst => "keyword.const",
        TokKind::KwComptime => "keyword.comptime",
        TokKind::KwReturn => "keyword.return",
        TokKind::KwLoop => "keyword.loop",
        TokKind::KwYield => "keyword.yield",
        TokKind::KwUnsafe => "keyword.unsafe",
        TokKind::KwUse => "keyword.use",
        TokKind::KwExtern => "keyword.extern",
        TokKind::KwModule => "keyword.module",
        TokKind::Ident(_) => "identifier",
        TokKind::Str(_) => "literal.string",
        TokKind::BinStr(_) => "literal.binpattern",
        TokKind::Int(..) => "literal.int",
        TokKind::Float(_) => "literal.float",
        TokKind::UnitNumber { .. } => "literal.unit_number",
        TokKind::Char(_) => "literal.char",
        TokKind::LParen => "punctuation.left_paren",
        TokKind::RParen => "punctuation.right_paren",
        TokKind::LBrace => "punctuation.left_brace",
        TokKind::RBrace => "punctuation.right_brace",
        TokKind::LBracket => "punctuation.left_bracket",
        TokKind::RBracket => "punctuation.right_bracket",
        TokKind::Colon => "punctuation.colon",
        TokKind::ColonColon => "operator.bind_immutable",
        TokKind::ColonEq => "operator.bind_mutable",
        TokKind::Comma => "punctuation.comma",
        TokKind::Arrow => "operator.arrow",
        TokKind::LambdaArrow => "operator.lambda_arrow",
        TokKind::Semi => "terminator",
        TokKind::Eq => "operator.assign",
        TokKind::Dot => "punctuation.dot",
        TokKind::DotDot => "operator.range",
        TokKind::DotDotDot => "operator.spread",
        TokKind::At => "punctuation.at",
        TokKind::Question => "operator.try",
        TokKind::QuestionQuestion => "operator.fallback",
        TokKind::QuestionDot => "operator.optional_field",
        TokKind::Plus => "operator.add",
        TokKind::Minus => "operator.subtract",
        TokKind::Star => "operator.star",
        TokKind::Slash => "operator.divide",
        TokKind::Percent => "operator.remainder",
        TokKind::Amp => "operator.amp",
        TokKind::Pipe => "operator.pipe",
        TokKind::Caret => "operator.caret",
        TokKind::Tilde => "operator.tilde",
        TokKind::TildeTilde => "operator.trait_attach",
        TokKind::Shl => "operator.shift_left",
        TokKind::Shr => "operator.shift_right",
        TokKind::AndAnd => "operator.and",
        TokKind::OrOr => "operator.or",
        TokKind::Bang => "operator.not",
        TokKind::EqEq => "operator.equal",
        TokKind::NotEq => "operator.not_equal",
        TokKind::Lt => "operator.less",
        TokKind::Gt => "operator.greater",
        TokKind::Le => "operator.less_equal",
        TokKind::Ge => "operator.greater_equal",
        TokKind::PlusEq => "operator.add_assign",
        TokKind::PlusPlus => "operator.increment",
        TokKind::MinusEq => "operator.subtract_assign",
        TokKind::MinusMinus => "operator.decrement",
        TokKind::StarEq => "operator.multiply_assign",
        TokKind::SlashEq => "operator.divide_assign",
        TokKind::PercentEq => "operator.remainder_assign",
        TokKind::AmpEq => "operator.amp_assign",
        TokKind::PipeEq => "operator.pipe_assign",
        TokKind::CaretEq => "operator.caret_assign",
        TokKind::ShlEq => "operator.shift_left_assign",
        TokKind::ShrEq => "operator.shift_right_assign",
        TokKind::Hash => "punctuation.hash",
        TokKind::Dollar => "punctuation.dollar",
        TokKind::LineComment(_) => "comment.line",
        TokKind::BlockComment(_) => "comment.block",
        TokKind::Eof => "eof",
    }
}
