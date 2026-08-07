//! Public read-only front-end toolkit API (D-FRONTENDAPI1=A).

use crate::Diagnostics::{span_line_col, Diagnostic, Severity, Span};
use crate::AST::{CtValue, Type};
use crate::Lexer::{TokKind, Token};
use crate::{Lexer, Parser, AST};

pub const API_VERSION: u32 = 1;
pub const SCHEMA_VERSION: u32 = 1;

fn compiler_error_value(code: &str, message: impl Into<String>, span: Span) -> CtValue {
    ct_struct(
        "CompilerError",
        vec![
            ("code", CtValue::Str(code.to_string())),
            ("message", CtValue::Str(message.into())),
            ("span", span_value(span.into())),
        ],
    )
}

/// D-FRONTENDAPI1=A: comptime bridge for the same read-only compiler values
/// exposed by this Rust module. The callback is installed at the compiler
/// entry seam; it deliberately declines every other Core module so the normal
/// interpreter/AOT paths remain unchanged.
pub fn eval_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    if module != "core.compiler" {
        return None;
    }
    let source = match args.first() {
        Some(CtValue::Str(source)) if args.len() == 1 && method != "check" => source.clone(),
        Some(CtValue::Struct { type_name, fields })
            if args.len() == 1 && method == "check" && type_name == "CompilerSyntaxTree" =>
        {
            match fields.iter().find_map(|(name, value)| {
                (name == "source").then(|| match value {
                    CtValue::Str(source) => Some(source.clone()),
                    _ => None,
                })
            }).flatten() {
                Some(source) => source,
                None => {
                    return Some(Ok(CtValue::ResErr(Box::new(compiler_error_value(
                        "E0956",
                        "`core.compiler.check` needs a parsed syntax tree with its source",
                        span,
                    )))))
                }
            }
        }
        _ => {
            let message = if method == "check" {
                "`core.compiler.check` expects one CompilerSyntaxTree".to_string()
            } else {
                format!("`core.compiler.{method}` expects one source String")
            };
            return Some(Ok(CtValue::ResErr(Box::new(compiler_error_value(
                "E0956",
                message,
                span,
            )))))
        }
    };
    if method == "check" {
        let Some(CtValue::Struct { fields, .. }) = args.first() else {
            unreachable!("check input was validated above")
        };
        let schema = fields.iter().find_map(|(name, value)| {
            (name == "schema_version").then_some(value)
        });
        if !matches!(schema, Some(CtValue::Int(value)) if *value == i64::from(SCHEMA_VERSION)) {
            let got = schema
                .and_then(|value| match value { CtValue::Int(value) => Some(*value), _ => None })
                .map_or_else(|| "missing".to_string(), |value| value.to_string());
            return Some(Ok(CtValue::ResErr(Box::new(compiler_error_value(
                "E0956",
                format!("unsupported CompilerSyntaxTree schema version {got}"),
                span,
            )))));
        }
    }
    let value = match method {
        "lex" => lexed_value(&lex_source(&source)),
        "parse" => syntax_tree_value(&parse_source(&source)),
        "check" => checked_value(&source),
        "source_map" => source_map_value(&source_map_from_generated_rust(&source)),
        _ => {
            return Some(Ok(CtValue::ResErr(Box::new(compiler_error_value(
                "E0956",
                format!("unknown `core.compiler` operation `{method}`"),
                span,
            )))))
        }
    };
    Some(Ok(CtValue::ResOk(Box::new(value))))
}

fn ct_struct(type_name: &str, fields: Vec<(&str, CtValue)>) -> CtValue {
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields: fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect(),
    }
}

fn span_value(range: TextRange) -> CtValue {
    ct_struct(
        crate::Syntax::TYPE_SOURCE_SPAN,
        vec![
            ("start", CtValue::Int(range.start as i64)),
            ("end", CtValue::Int(range.end as i64)),
        ],
    )
}

fn optional_span(range: Option<TextRange>) -> CtValue {
    range.map_or(
        CtValue::None(Type::Named(crate::Syntax::TYPE_SOURCE_SPAN.to_string())),
        |range| CtValue::Some(Box::new(span_value(range))),
    )
}

fn diagnostic_value(diagnostic: &DiagnosticView) -> CtValue {
    ct_struct(
        "CompilerDiagnostic",
        vec![
            ("code", CtValue::Str(diagnostic.code.clone())),
            (
                "severity",
                CtValue::Str(match diagnostic.severity {
                    DiagnosticSeverity::Error => "error".to_string(),
                    DiagnosticSeverity::Lint => "lint".to_string(),
                }),
            ),
            ("message", CtValue::Str(diagnostic.message.clone())),
            ("why", CtValue::Str(diagnostic.why.clone())),
            ("fix", CtValue::Str(diagnostic.fix.clone())),
            ("span", optional_span(diagnostic.span)),
        ],
    )
}

fn lexed_value(lexed: &LexedSource) -> CtValue {
    ct_struct(
        "CompilerLexed",
        vec![
            ("schema_version", CtValue::Int(i64::from(SCHEMA_VERSION))),
            ("source", CtValue::Str(lexed.source.clone())),
            (
                "tokens",
                CtValue::List(
                    lexed
                        .tokens
                        .iter()
                        .map(|token| {
                            ct_struct(
                                "CompilerToken",
                                vec![
                                    ("kind", CtValue::Str(token.kind.to_string())),
                                    ("text", CtValue::Str(token.text.clone())),
                                    ("span", span_value(token.span)),
                                ],
                            )
                        })
                        .collect(),
                ),
            ),
            (
                "diagnostics",
                CtValue::List(lexed.diagnostics.iter().map(diagnostic_value).collect()),
            ),
        ],
    )
}

fn syntax_node_value(node: &SyntaxNode) -> CtValue {
    let kind = match node.kind {
        SyntaxNodeKind::Function => "function",
        SyntaxNodeKind::Struct => "struct",
        SyntaxNodeKind::Enum => "enum",
        SyntaxNodeKind::Trait => "trait",
        SyntaxNodeKind::Tag => "tag",
        SyntaxNodeKind::Effect => "effect",
        SyntaxNodeKind::Impl => "impl",
        SyntaxNodeKind::Const => "const",
        SyntaxNodeKind::Test => "test",
        SyntaxNodeKind::Bench => "bench",
        SyntaxNodeKind::ExternRust => "extern_rust",
        SyntaxNodeKind::Module => "module",
        SyntaxNodeKind::CModule => "c_module",
        SyntaxNodeKind::CodeModule => "code_module",
        SyntaxNodeKind::ErrorConversion => "error_conversion",
        SyntaxNodeKind::Migration => "migration",
        SyntaxNodeKind::State => "state",
        SyntaxNodeKind::Protocol => "protocol",
        SyntaxNodeKind::Derive => "derive",
        SyntaxNodeKind::GenericModule => "generic_module",
        SyntaxNodeKind::ModuleAlias => "module_alias",
        SyntaxNodeKind::Distinct => "distinct",
        SyntaxNodeKind::TypeAlias => "type_alias",
        SyntaxNodeKind::UnitFamily => "unit_family",
        SyntaxNodeKind::Marker => "marker",
    };
    ct_struct(
        "CompilerNode",
        vec![
            ("kind", CtValue::Str(kind.to_string())),
            (
                "name",
                node.name.clone().map_or(
                    CtValue::None(Type::String),
                    |name| CtValue::Some(Box::new(CtValue::Str(name))),
                ),
            ),
            ("span", span_value(node.span)),
        ],
    )
}

fn syntax_tree_value(tree: &SyntaxTree) -> CtValue {
    ct_struct(
        "CompilerSyntaxTree",
        vec![
            ("schema_version", CtValue::Int(i64::from(SCHEMA_VERSION))),
            ("source", CtValue::Str(tree.source.clone())),
            (
                "items",
                CtValue::List(tree.items.iter().map(syntax_node_value).collect()),
            ),
            (
                "diagnostics",
                CtValue::List(tree.diagnostics.iter().map(diagnostic_value).collect()),
            ),
        ],
    )
}

fn compiler_function_value(node: &SyntaxNode) -> CtValue {
    let name = node.name.clone().unwrap_or_default();
    ct_struct(
        "FunctionInfo",
        vec![
            ("name", CtValue::Str(name.clone())),
            ("module", CtValue::Str("core.compiler".to_string())),
            (
                "identity",
                CtValue::Str(format!("core.compiler::{name}")),
            ),
            ("params", CtValue::List(Vec::new())),
            ("span", span_value(node.span)),
            (
                "effects",
                ct_struct("EffectInfo", vec![("values", CtValue::List(Vec::new()))]),
            ),
            ("reaches_panic", CtValue::Bool(false)),
        ],
    )
}

fn field_value(value: &CtValue, name: &str) -> Option<CtValue> {
    match value {
        CtValue::Struct { fields, .. } => fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.clone()),
        _ => None,
    }
}

fn effect_info_value(values: impl IntoIterator<Item = String>) -> CtValue {
    ct_struct(
        "EffectInfo",
        vec![
            (
                "values",
                CtValue::List(values.into_iter().map(CtValue::Str).collect()),
            ),
        ],
    )
}

fn compiler_option_string(value: Option<&str>) -> CtValue {
    value.map_or(
        CtValue::None(Type::String),
        |value| CtValue::Some(Box::new(CtValue::Str(value.to_string()))),
    )
}

fn compiler_option_int(value: Option<usize>) -> CtValue {
    value.map_or(
        CtValue::None(Type::Int),
        |value| CtValue::Some(Box::new(CtValue::Int(value as i64))),
    )
}

fn compiler_string_list(values: impl IntoIterator<Item = String>) -> CtValue {
    CtValue::List(values.into_iter().map(CtValue::Str).collect())
}

fn compiler_semantic_span(span: jet_semindex::SourceSpan) -> CtValue {
    span_value(TextRange {
        start: span.start,
        end: span.end,
    })
}

fn compiler_symbol_kind_value(kind: &jet_semindex::SymbolKind) -> CtValue {
    let (kind_name, params, ret, fields, variants, parent, mutable, ty) = match kind {
        jet_semindex::SymbolKind::Module => {
            ("module", Vec::new(), None, Vec::new(), Vec::new(), None, None, None)
        }
        jet_semindex::SymbolKind::Function { params, ret } => (
            "function",
            params
                .iter()
                .map(|(name, ty)| {
                    ct_struct(
                        "CompilerParam",
                        vec![
                            ("name", CtValue::Str(name.clone())),
                            ("ty", CtValue::Str(ty.clone())),
                        ],
                    )
                })
                .collect(),
            ret.as_deref(),
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
        ),
        jet_semindex::SymbolKind::Struct { fields } => (
            "struct",
            Vec::new(),
            None,
            fields
                .iter()
                .map(|(name, ty)| {
                    ct_struct(
                        "CompilerField",
                        vec![
                            ("name", CtValue::Str(name.clone())),
                            ("ty", CtValue::Str(ty.clone())),
                        ],
                    )
                })
                .collect(),
            Vec::new(),
            None,
            None,
            None,
        ),
        jet_semindex::SymbolKind::Enum { variants } => (
            "enum",
            Vec::new(),
            None,
            Vec::new(),
            variants.clone(),
            None,
            None,
            None,
        ),
        jet_semindex::SymbolKind::Trait => {
            ("trait", Vec::new(), None, Vec::new(), Vec::new(), None, None, None)
        }
        jet_semindex::SymbolKind::Tag => {
            ("tag", Vec::new(), None, Vec::new(), Vec::new(), None, None, None)
        }
        jet_semindex::SymbolKind::Type => {
            ("type", Vec::new(), None, Vec::new(), Vec::new(), None, None, None)
        }
        jet_semindex::SymbolKind::Const => {
            ("const", Vec::new(), None, Vec::new(), Vec::new(), None, None, None)
        }
        jet_semindex::SymbolKind::EnumVariant { parent } => (
            "enum_variant",
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            Some(parent.as_str()),
            None,
            None,
        ),
        jet_semindex::SymbolKind::Field { ty, parent } => (
            "field",
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            Some(parent.as_str()),
            None,
            Some(ty.as_str()),
        ),
        jet_semindex::SymbolKind::Local { mutable, ty } => (
            "local",
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            None,
            Some(*mutable),
            ty.as_deref(),
        ),
        jet_semindex::SymbolKind::Param { ty } => (
            "param",
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            None,
            None,
            Some(ty.as_str()),
        ),
    };
    ct_struct(
        "CompilerSymbolKind",
        vec![
            ("kind", CtValue::Str(kind_name.to_string())),
            ("params", CtValue::List(params)),
            ("ret", compiler_option_string(ret)),
            ("fields", CtValue::List(fields)),
            ("variants", compiler_string_list(variants)),
            ("parent", compiler_option_string(parent)),
            (
                "mutable",
                mutable.map_or(CtValue::None(Type::Bool), |value| {
                    CtValue::Some(Box::new(CtValue::Bool(value)))
                }),
            ),
            ("ty", compiler_option_string(ty)),
        ],
    )
}

fn compiler_view_source_value(source: &jet_semindex::ViewSourceFact) -> CtValue {
    let (kind, index, module, name) = match source {
        jet_semindex::ViewSourceFact::Receiver => ("receiver", None, None, None),
        jet_semindex::ViewSourceFact::Parameter(index) => {
            ("parameter", Some(*index), None, None)
        }
        jet_semindex::ViewSourceFact::Static { module_path, name } => {
            ("static", None, Some(module_path.as_str()), Some(name.as_str()))
        }
    };
    ct_struct(
        "CompilerViewSource",
        vec![
            ("kind", CtValue::Str(kind.to_string())),
            ("index", compiler_option_int(index)),
            ("module", compiler_option_string(module)),
            ("name", compiler_option_string(name)),
        ],
    )
}

fn compiler_view_projection_value(
    projection: &jet_semindex::ViewProjectionFact,
) -> CtValue {
    let (kind, name) = match projection {
        jet_semindex::ViewProjectionFact::Field(name) => ("field", Some(name.as_str())),
        jet_semindex::ViewProjectionFact::Index => ("index", None),
        jet_semindex::ViewProjectionFact::Range => ("range", None),
    };
    ct_struct(
        "CompilerViewProjection",
        vec![
            ("kind", CtValue::Str(kind.to_string())),
            ("name", compiler_option_string(name)),
        ],
    )
}

fn compiler_view_provenance_value(
    provenance: &jet_semindex::ViewProvenanceFact,
) -> CtValue {
    let sources = provenance
        .sources
        .iter()
        .map(|source| {
            ct_struct(
                "CompilerViewSourcePath",
                vec![
                    ("source", compiler_view_source_value(&source.source)),
                    (
                        "projections",
                        CtValue::List(
                            source
                                .projections
                                .iter()
                                .map(compiler_view_projection_value)
                                .collect(),
                        ),
                    ),
                ],
            )
        })
        .collect();
    ct_struct(
        "CompilerViewProvenance",
        vec![
            (
                "output_path",
                compiler_string_list(provenance.output_path.clone()),
            ),
            ("sources", CtValue::List(sources)),
            ("mutable", CtValue::Bool(provenance.mutable)),
        ],
    )
}

fn compiler_definition_value(definition: &jet_semindex::SymbolDef) -> CtValue {
    ct_struct(
        "CompilerDefinition",
        vec![
            ("identity", CtValue::Str(definition.identity.clone())),
            ("name", CtValue::Str(definition.name.clone())),
            ("module", CtValue::Str(definition.module_path.clone())),
            ("span", compiler_semantic_span(definition.def_span)),
            ("kind", compiler_symbol_kind_value(&definition.kind)),
            (
                "view_provenance",
                CtValue::List(
                    definition
                        .view_provenance
                        .iter()
                        .map(compiler_view_provenance_value)
                        .collect(),
                ),
            ),
        ],
    )
}

fn compiler_anchor_value(anchor: &jet_semindex::DefinitionAnchor) -> CtValue {
    ct_struct(
        "CompilerDefinitionAnchor",
        vec![
            ("module", CtValue::Str(anchor.module_path.clone())),
            ("kind", CtValue::Str(anchor.kind.clone())),
            (
                "semantic_identity",
                compiler_option_string(anchor.semantic_identity.as_deref()),
            ),
            ("span", compiler_semantic_span(anchor.def_span)),
        ],
    )
}

fn compiler_reference_value(reference: &jet_semindex::SymbolRef) -> CtValue {
    ct_struct(
        "CompilerReference",
        vec![
            ("name", CtValue::Str(reference.name.clone())),
            ("module", CtValue::Str(reference.module_path.clone())),
            (
                "scope_identity",
                compiler_option_string(reference.scope_identity.as_deref()),
            ),
            (
                "target",
                reference.target.as_ref().map_or(
                    CtValue::None(Type::Named("CompilerDefinitionAnchor".to_string())),
                    |target| CtValue::Some(Box::new(compiler_anchor_value(target))),
                ),
            ),
            ("span", compiler_semantic_span(reference.span)),
        ],
    )
}

fn compiler_call_value(call: &jet_semindex::CallEdge) -> CtValue {
    ct_struct(
        "CompilerCall",
        vec![
            ("caller", CtValue::Str(call.caller.clone())),
            ("callee", CtValue::Str(call.callee.clone())),
            ("module", CtValue::Str(call.module_path.clone())),
            ("span", compiler_semantic_span(call.call_span)),
        ],
    )
}

fn compiler_effect_value(effect: &jet_semindex::EffectFact) -> CtValue {
    let provenance = effect
        .provenance
        .iter()
        .map(|origin| {
            ct_struct(
                "CompilerEffectProvenance",
                vec![
                    ("effect", CtValue::Str(origin.effect.clone())),
                    (
                        "call_path",
                        compiler_string_list(origin.call_path.clone()),
                    ),
                    (
                        "spans",
                        CtValue::List(
                            origin
                                .spans
                                .iter()
                                .copied()
                                .map(compiler_semantic_span)
                                .collect(),
                        ),
                    ),
                ],
            )
        })
        .collect();
    ct_struct(
        "CompilerEffect",
        vec![
            ("function", CtValue::Str(effect.function.clone())),
            ("direct", compiler_string_list(effect.direct.clone())),
            ("callees", compiler_string_list(effect.callees.clone())),
            ("inferred", compiler_string_list(effect.inferred.clone())),
            ("maximal", CtValue::Bool(effect.maximal)),
            ("provenance", CtValue::List(provenance)),
        ],
    )
}

fn compiler_output_entry_value(entry: &jet_semindex::OutputEntryFact) -> CtValue {
    ct_struct(
        "CompilerOutputEntry",
        vec![
            ("identity", CtValue::Str(entry.identity.clone())),
            ("name", CtValue::Str(entry.name.clone())),
            ("module", CtValue::Str(entry.module_path.clone())),
            ("definition_span", compiler_semantic_span(entry.definition_span)),
            ("reference_span", compiler_semantic_span(entry.reference_span)),
            ("params", compiler_string_list(entry.params.clone())),
            (
                "return_type",
                compiler_option_string(entry.return_type.as_deref()),
            ),
            ("authority", CtValue::Str(entry.authority.clone())),
            ("effects", compiler_string_list(entry.effects.clone())),
        ],
    )
}

fn compiler_output_value(output: &jet_semindex::OutputFact) -> CtValue {
    ct_struct(
        "CompilerOutput",
        vec![
            ("binding", CtValue::Str(output.binding.clone())),
            ("kind", CtValue::Str(output.kind.clone())),
            ("name", CtValue::Str(output.name.clone())),
            ("module", CtValue::Str(output.module_path.clone())),
            ("span", compiler_semantic_span(output.span)),
            ("entry", compiler_output_entry_value(&output.entry)),
        ],
    )
}

fn compiler_semantic_index_value(index: &jet_semindex::SemIndex, source: &str) -> CtValue {
    ct_struct(
        "CompilerSemanticIndex",
        vec![
            ("schema_version", CtValue::Int(index.schema_version() as i64)),
            (
                "source_digest",
                CtValue::Str(crate::SHA256::sha256_hex(source.as_bytes())),
            ),
            (
                "definitions",
                CtValue::List(
                    index
                        .definitions()
                        .iter()
                        .map(compiler_definition_value)
                        .collect(),
                ),
            ),
            (
                "references",
                CtValue::List(
                    index
                        .references()
                        .iter()
                        .map(compiler_reference_value)
                        .collect(),
                ),
            ),
            (
                "calls",
                CtValue::List(index.call_edges().iter().map(compiler_call_value).collect()),
            ),
            (
                "effects",
                CtValue::List(index.effects().iter().map(compiler_effect_value).collect()),
            ),
            (
                "outputs",
                CtValue::List(index.outputs().iter().map(compiler_output_value).collect()),
            ),
        ],
    )
}

fn checked_value(source: &str) -> CtValue {
    let (checked_diagnostics, bundle, effect_facts) =
        crate::Driver::check_eval_with_effect_facts(source, "core.compiler.jet");
    checked_value_from_parts(
        source,
        &checked_diagnostics,
        bundle.as_ref(),
        &effect_facts,
    )
}

fn checked_value_from_parts(
    source: &str,
    checked_diagnostics: &[Diagnostic],
    bundle: Option<&AST::ProgramBundle>,
    effect_facts: &crate::Sema::SemIndexEffectFacts,
) -> CtValue {
    let syntax = parse_source(source);
    let diagnostics = checked_diagnostics
        .iter()
        .map(diagnostic_view)
        .collect::<Vec<_>>();
    let has_errors = checked_diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error);
    let syntax_value = syntax_tree_value(&syntax);
    let (functions, effects, semantic_index) = if let Some(bundle) = bundle {
        let semantic_facts = crate::Driver::program_semantic_facts(bundle, effect_facts);
        let program = crate::Comptime::build_program_info(bundle, &semantic_facts);
        let functions = field_value(&program, "functions")
            .unwrap_or_else(|| CtValue::List(Vec::new()));
        let index = jet_semindex::from_checked(bundle, effect_facts);
        let effects = CtValue::List(
            index
                .effects()
                .iter()
                .map(|effect| effect_info_value(effect.inferred.clone()))
                .collect(),
        );
        let semantic_index = if has_errors {
            CtValue::None(Type::Named("CompilerSemanticIndex".to_string()))
        } else {
            CtValue::Some(Box::new(compiler_semantic_index_value(&index, source)))
        };
        (functions, effects, semantic_index)
    } else {
        let functions = syntax
            .items
            .iter()
            .filter(|node| node.kind == SyntaxNodeKind::Function)
            .map(compiler_function_value)
            .collect::<Vec<_>>();
        (
            CtValue::List(functions),
            CtValue::None(Type::Named("CompilerSemanticIndex".to_string())),
            CtValue::None(Type::Named("CompilerSemanticIndex".to_string())),
        )
    };
    ct_struct(
        "CompilerChecked",
        vec![
            ("schema_version", CtValue::Int(i64::from(SCHEMA_VERSION))),
            ("source", CtValue::Str(source.to_string())),
            ("syntax", syntax_value),
            (
                "diagnostics",
                CtValue::List(diagnostics.iter().map(diagnostic_value).collect()),
            ),
            ("functions", functions),
            ("effects", effects),
            ("semantic_index", semantic_index),
        ],
    )
}

fn source_map_value(map: &SourceMap) -> CtValue {
    ct_struct(
        "CompilerSourceMap",
        vec![
            ("schema_version", CtValue::Int(i64::from(SCHEMA_VERSION))),
            (
                "sources",
                CtValue::List(map.sources.iter().cloned().map(CtValue::Str).collect()),
            ),
            (
                "generated_lines",
                CtValue::List(
                    map.generated_lines
                        .iter()
                        .map(|line| {
                            ct_struct(
                                "CompilerGeneratedLine",
                                vec![
                                    ("generated_line", CtValue::Int(line.generated_line as i64)),
                                    (
                                        "source",
                                        line.source.clone().map_or(
                                            CtValue::None(Type::String),
                                            |source| CtValue::Some(Box::new(CtValue::Str(source))),
                                        ),
                                    ),
                                    ("source_line", CtValue::Int(line.source_line as i64)),
                                ],
                            )
                        })
                        .collect(),
                ),
            ),
        ],
    )
}

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
    Effect,
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
    Marker,
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
    pub schema_version: u32,
    pub source: String,
    pub items: Vec<SyntaxNode>,
    pub diagnostics: Vec<DiagnosticView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexedSource {
    pub api_version: u32,
    pub schema_version: u32,
    pub source: String,
    pub tokens: Vec<TokenView>,
    pub diagnostics: Vec<DiagnosticView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMap {
    pub api_version: u32,
    pub schema_version: u32,
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
    pub source_digest: String,
    pub definitions: Vec<jet_semindex::SymbolDef>,
    pub references: Vec<jet_semindex::SymbolRef>,
    pub calls: Vec<jet_semindex::CallEdge>,
    pub effects: Vec<jet_semindex::EffectFact>,
    pub outputs: Vec<jet_semindex::OutputFact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedFile {
    pub api_version: u32,
    pub schema_version: u32,
    pub diagnostics: Vec<DiagnosticView>,
    pub syntax: Option<SyntaxTree>,
    pub semantic_index: Option<SemIndexView>,
}

pub fn lex_source(src: &str) -> LexedSource {
    let (tokens, diagnostics) = Lexer::lex(src);
    LexedSource {
        api_version: API_VERSION,
        schema_version: SCHEMA_VERSION,
        source: src.to_string(),
        tokens: tokens.iter().map(|token| token_view(src, token)).collect(),
        diagnostics: diagnostics.iter().map(diagnostic_view).collect(),
    }
}

pub fn parse_source(src: &str) -> SyntaxTree {
    let lexed = lex_source(src);
    if !lexed.diagnostics.is_empty() {
            return SyntaxTree {
                api_version: API_VERSION,
                schema_version: SCHEMA_VERSION,
            source: src.to_string(),
            items: Vec::new(),
            diagnostics: lexed.diagnostics,
        };
    }

    let (tokens, _) = Lexer::lex(src);
    match Parser::parse_for_check(&tokens) {
        Ok((program, parse_teaching)) => SyntaxTree {
            api_version: API_VERSION,
            schema_version: SCHEMA_VERSION,
            source: src.to_string(),
            items: program.items.iter().map(item_node).collect(),
            diagnostics: parse_teaching.iter().map(diagnostic_view).collect(),
        },
        Err(diagnostics) => SyntaxTree {
            api_version: API_VERSION,
            schema_version: SCHEMA_VERSION,
            source: src.to_string(),
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
    let source = std::fs::read_to_string(path).unwrap_or_default();
    let syntax = bundle.as_ref().map(|bundle| bundle_syntax_tree(bundle, &source));
    let semantic_index = if has_errors {
        None
    } else {
        bundle
            .as_ref()
            .map(|bundle| {
                SemIndexView::from_index(jet_semindex::from_checked(bundle, &facts), &source)
            })
    };
    CheckedFile {
        api_version: API_VERSION,
        schema_version: SCHEMA_VERSION,
        diagnostics: diagnostics.iter().map(diagnostic_view).collect(),
        syntax,
        semantic_index,
    }
}

/// Stable JSON envelope shared by the CLI mirror and callers that need to
/// persist compiler facts. This is deliberately hand-written: the compiler
/// seam has no serialization dependency, and the field order is part of the
/// schema's deterministic output.
pub const JSON_SCHEMA_VERSION: u32 = SCHEMA_VERSION;

pub fn lex_source_json(source: &str) -> String {
    format!(
        "{{\"schema_version\":{},\"api_version\":{},\"operation\":\"lex\",\"value\":{}}}",
        JSON_SCHEMA_VERSION,
        API_VERSION,
        json_lexed(&lex_source(source)),
    )
}

pub fn parse_source_json(source: &str) -> String {
    format!(
        "{{\"schema_version\":{},\"api_version\":{},\"operation\":\"parse\",\"value\":{}}}",
        JSON_SCHEMA_VERSION,
        API_VERSION,
        json_syntax_tree(&parse_source(source)),
    )
}

pub fn check_file_json(path: &std::path::Path) -> String {
    let file = path.to_string_lossy();
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            return compiler_api_error_json(
                "check",
                &file,
                "E0956",
                format!("could not read compiler input: {error}"),
            )
        }
    };
    // Keep the JSON mirror byte-for-byte aligned with the typed, source-only
    // operation. The file belongs in the outer envelope; it must not change
    // the `CompilerChecked` value returned by `core.compiler.check`.
    let value = checked_value(&source).to_json();
    format!(
        "{{\"schema_version\":{},\"api_version\":{},\"operation\":\"check\",\"file\":{},\"value\":{}}}",
        JSON_SCHEMA_VERSION,
        API_VERSION,
        json_string(&file),
        value,
    )
}

/// Serialize one compiler-operation failure at the JSON boundary. The typed
/// Jet surface uses `CompilerError`; JSON carries the same fields without
/// leaking a Rust or rustc error string as the whole payload.
pub fn compiler_api_error_json(
    operation: &str,
    file: &str,
    code: &str,
    message: impl Into<String>,
) -> String {
    format!(
        "{{\"schema_version\":{},\"api_version\":{},\"operation\":{},\"file\":{},\"error\":{{\"code\":{},\"message\":{}}}}}",
        JSON_SCHEMA_VERSION,
        API_VERSION,
        json_string(operation),
        json_string(file),
        json_string(code),
        json_string(&message.into()),
    )
}

pub fn source_map_json(rust_source: &str) -> String {
    format!(
        "{{\"schema_version\":{},\"api_version\":{},\"operation\":\"source_map\",\"value\":{}}}",
        JSON_SCHEMA_VERSION,
        API_VERSION,
        json_source_map(&source_map_from_generated_rust(rust_source)),
    )
}

fn json_string(value: &str) -> String {
    format!("\"{}\"", jet_foundation::JSON::json_escape(value))
}

fn json_span(span: Option<TextRange>) -> String {
    span.map_or_else(
        || "null".to_string(),
        |range| format!("{{\"start\":{},\"end\":{}}}", range.start, range.end),
    )
}

fn json_diagnostic(diagnostic: &DiagnosticView) -> String {
    format!(
        "{{\"code\":{},\"severity\":{},\"message\":{},\"why\":{},\"fix\":{},\"span\":{}}}",
        json_string(&diagnostic.code),
        json_string(match diagnostic.severity {
            DiagnosticSeverity::Error => "error",
            DiagnosticSeverity::Lint => "lint",
        }),
        json_string(&diagnostic.message),
        json_string(&diagnostic.why),
        json_string(&diagnostic.fix),
        json_span(diagnostic.span),
    )
}

fn json_diagnostics(diagnostics: &[DiagnosticView]) -> String {
    format!(
        "[{}]",
        diagnostics
            .iter()
            .map(json_diagnostic)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn json_lexed(lexed: &LexedSource) -> String {
    let tokens = lexed
        .tokens
        .iter()
        .map(|token| {
            format!(
                "{{\"kind\":{},\"text\":{},\"span\":{{\"start\":{},\"end\":{}}},\"start\":{{\"line\":{},\"column\":{}}},\"end\":{{\"line\":{},\"column\":{}}}}}",
                json_string(token.kind),
                json_string(&token.text),
                token.span.start,
                token.span.end,
                token.start.line,
                token.start.column,
                token.end.line,
                token.end.column,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema_version\":{},\"api_version\":{},\"source\":{},\"tokens\":[{}],\"diagnostics\":{}}}",
        lexed.schema_version,
        lexed.api_version,
        json_string(&lexed.source),
        tokens,
        json_diagnostics(&lexed.diagnostics),
    )
}

fn json_syntax_tree(tree: &SyntaxTree) -> String {
    let items = tree
        .items
        .iter()
        .map(|node| {
            format!(
                "{{\"kind\":{},\"name\":{},\"span\":{{\"start\":{},\"end\":{}}}}}",
                json_string(match node.kind {
                    SyntaxNodeKind::Function => "function",
                    SyntaxNodeKind::Struct => "struct",
                    SyntaxNodeKind::Enum => "enum",
                    SyntaxNodeKind::Trait => "trait",
                    SyntaxNodeKind::Tag => "tag",
                    SyntaxNodeKind::Effect => "effect",
                    SyntaxNodeKind::Impl => "impl",
                    SyntaxNodeKind::Const => "const",
                    SyntaxNodeKind::Test => "test",
                    SyntaxNodeKind::Bench => "bench",
                    SyntaxNodeKind::ExternRust => "extern_rust",
                    SyntaxNodeKind::Module => "module",
                    SyntaxNodeKind::CModule => "c_module",
                    SyntaxNodeKind::CodeModule => "code_module",
                    SyntaxNodeKind::ErrorConversion => "error_conversion",
                    SyntaxNodeKind::Migration => "migration",
                    SyntaxNodeKind::State => "state",
                    SyntaxNodeKind::Protocol => "protocol",
                    SyntaxNodeKind::Derive => "derive",
                    SyntaxNodeKind::GenericModule => "generic_module",
                    SyntaxNodeKind::ModuleAlias => "module_alias",
                    SyntaxNodeKind::Distinct => "distinct",
                    SyntaxNodeKind::TypeAlias => "type_alias",
                    SyntaxNodeKind::UnitFamily => "unit_family",
                    SyntaxNodeKind::Marker => "marker",
                }),
                node.name
                    .as_deref()
                    .map(json_string)
                    .unwrap_or_else(|| "null".to_string()),
                node.span.start,
                node.span.end,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema_version\":{},\"api_version\":{},\"source\":{},\"items\":[{}],\"diagnostics\":{}}}",
        tree.schema_version,
        tree.api_version,
        json_string(&tree.source),
        items,
        json_diagnostics(&tree.diagnostics),
    )
}

fn json_source_map(map: &SourceMap) -> String {
    let sources = map
        .sources
        .iter()
        .map(|source| json_string(source))
        .collect::<Vec<_>>()
        .join(",");
    let lines = map
        .generated_lines
        .iter()
        .map(|line| {
            format!(
                "{{\"generated_line\":{},\"source\":{},\"source_line\":{}}}",
                line.generated_line,
                line.source
                    .as_deref()
                    .map(json_string)
                    .unwrap_or_else(|| "null".to_string()),
                line.source_line,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema_version\":{},\"api_version\":{},\"sources\":[{}],\"generated_lines\":[{}]}}",
        map.schema_version, map.api_version, sources, lines
    )
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
        schema_version: SCHEMA_VERSION,
        sources,
        generated_lines,
    }
}

impl SemIndexView {
    fn from_index(index: jet_semindex::SemIndex, source: &str) -> Self {
        SemIndexView {
            schema_version: index.schema_version(),
            source_digest: crate::SHA256::sha256_hex(source.as_bytes()),
            definitions: index.definitions().to_vec(),
            references: index.references().to_vec(),
            calls: index.call_edges().to_vec(),
            effects: index.effects().to_vec(),
            outputs: index.outputs().to_vec(),
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

fn bundle_syntax_tree(bundle: &AST::ProgramBundle, source: &str) -> SyntaxTree {
    let mut items = Vec::new();
    for module in &bundle.modules {
        items.extend(module.items.iter().map(item_node));
    }
    SyntaxTree {
        api_version: API_VERSION,
        schema_version: SCHEMA_VERSION,
        source: source.to_string(),
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
        AST::Item::EffectDecl(effect) => (
            SyntaxNodeKind::Effect,
            Some(effect.name.clone()),
            effect.name_span,
        ),
        AST::Item::Impl(i) => (SyntaxNodeKind::Impl, Some(i.type_name.clone()), i.type_span),
        AST::Item::Const(c) => (SyntaxNodeKind::Const, Some(c.name.clone()), c.name_span),
        AST::Item::Test(t) => (SyntaxNodeKind::Test, t.name.clone(), t.name_span),
        AST::Item::Bench(b) => (SyntaxNodeKind::Bench, b.name.clone(), b.name_span),
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
        AST::Item::MarkerDecl(m) => (SyntaxNodeKind::Marker, Some(m.name.clone()), m.name_span),
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
        TokKind::KwSwitch => "keyword.switch",
        TokKind::KwBreak => "keyword.break",
        TokKind::KwTrue => "literal.true",
        TokKind::KwFalse => "literal.false",
        TokKind::KwMutate => "keyword.mutate",
        TokKind::KwMove => "keyword.move",
        TokKind::KwCopy => "keyword.copy",
        TokKind::KwStruct => "keyword.struct",
        TokKind::KwEnum => "keyword.enum",
        TokKind::KwImpl => "keyword.impl",
        TokKind::KwTrait => "keyword.trait",
        TokKind::KwTag => "keyword.tag",
        TokKind::KwEffect => "keyword.effect",
        TokKind::KwDerive => "keyword.derive",
        TokKind::KwSelf => "keyword.self",
        TokKind::KwNull => "literal.null",
        TokKind::KwIt => "keyword.it",
        TokKind::KwConst => "keyword.const",
        TokKind::KwComptime => "keyword.comptime",
        TokKind::KwReturn => "keyword.return",
        TokKind::KwLoop => "keyword.loop",
        TokKind::KwYield => "keyword.yield",
        TokKind::KwUse => "keyword.use",
        TokKind::KwExtern => "keyword.extern",
        TokKind::KwModule => "keyword.module",
        TokKind::Ident(_) => "identifier",
        TokKind::Str(_) => "literal.string",
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
        TokKind::FenceOpen => "operator.fence_open",
        TokKind::FenceClose => "operator.fence_close",
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
        TokKind::DotDotLt => "operator.range_exclusive",
        TokKind::DotDotDot => "operator.spread",
        TokKind::At => "punctuation.at",
        TokKind::Question => "operator.try",
        TokKind::QuestionQuestion => "operator.fallback",
        TokKind::QuestionDot => "operator.optional_field",
        TokKind::Plus => "operator.add",
        TokKind::Minus => "operator.subtract",
        TokKind::Star => "operator.star",
        TokKind::Slash => "operator.divide",
        TokKind::SlashPercent => "operator.floor_divide",
        TokKind::Percent => "operator.modulo",
        TokKind::PercentPercent => "operator.remainder",
        TokKind::Amp => "operator.amp",
        TokKind::Pipe => "operator.alternative",
        TokKind::Caret => "operator.caret",
        TokKind::Tilde => "operator.tilde",
        TokKind::TildePipe => "operator.tilde_pipe",
        TokKind::TildePipeEq => "operator.tilde_pipe_assign",
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
        TokKind::SlashPercentEq => "operator.floor_divide_assign",
        TokKind::PercentEq => "operator.modulo_assign",
        TokKind::PercentPercentEq => "operator.remainder_assign",
        TokKind::AmpEq => "operator.amp_assign",
        TokKind::PipeEq => "operator.bit_or_assign",
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
