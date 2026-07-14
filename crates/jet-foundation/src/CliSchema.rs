//! D-SHAPE-CLI1: the checked command-schema projection shared by codegen and
//! inspection. The entry parameter type remains source truth; consumers never
//! reconstruct shell names, requiredness, defaults, or help independently.

use crate::AST::{CtValue, Expr, Marker, StrPart, StructDef, Type};
use crate::Syntax;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliValueKind {
    Bool,
    Int,
    Float,
    String,
    Path,
}

impl CliValueKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CliValueKind::Bool => "Bool",
            CliValueKind::Int => "Int",
            CliValueKind::Float => "Float",
            CliValueKind::String => "String",
            CliValueKind::Path => "Path",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CliDefault {
    TypeDefault,
    Value(CtValue),
}

impl CliDefault {
    pub fn display(&self) -> String {
        match self {
            CliDefault::TypeDefault => "type default".to_string(),
            CliDefault::Value(value) => value.jet_show(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CliInputShape {
    Flag,
    Value {
        kind: CliValueKind,
        optional: bool,
        default: Option<CliDefault>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CliInputSchema {
    pub field: String,
    pub flag: String,
    pub help: String,
    pub metavar: Option<String>,
    pub shape: CliInputShape,
}

impl CliInputSchema {
    pub fn required(&self) -> bool {
        matches!(
            self.shape,
            CliInputShape::Value {
                optional: false,
                default: None,
                ..
            }
        )
    }

    pub fn value_kind(&self) -> CliValueKind {
        match self.shape {
            CliInputShape::Flag => CliValueKind::Bool,
            CliInputShape::Value { kind, .. } => kind,
        }
    }

    pub fn default_display(&self) -> Option<String> {
        match &self.shape {
            CliInputShape::Value { default, .. } => {
                default.as_ref().map(CliDefault::display)
            }
            CliInputShape::Flag => Some("false".to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CliCommandSchema {
    pub entry_type: String,
    pub inputs: Vec<CliInputSchema>,
}

impl CliCommandSchema {
    pub fn completion_words(&self) -> Vec<String> {
        let mut words = vec!["--help".to_string()];
        words.extend(self.inputs.iter().map(|input| format!("--{}", input.flag)));
        words
    }
}

pub fn command_schema(structure: &StructDef) -> Option<CliCommandSchema> {
    if !structure
        .derives
        .iter()
        .any(|(name, _)| name == Syntax::CONTRACT_CLI)
    {
        return None;
    }

    let inputs = structure
        .fields
        .iter()
        .filter(|field| field.computed.is_none())
        .map(|field| {
            let flag = field.name.replace('_', "-");
            let help = marker(&field.serde_markers, Syntax::CONTRACT_DOC)
                .and_then(marker_string)
                .unwrap_or_else(|| format!("value for --{flag}"));
            let metavar = flag.replace('-', "_").to_uppercase();
            let shape = match &field.ty {
                Type::Bool => CliInputShape::Flag,
                Type::Option(inner) => CliInputShape::Value {
                    kind: scalar_kind(inner)
                        .expect("sema permits only scalar Option fields on a Cli struct"),
                    optional: true,
                    default: None,
                },
                ty => CliInputShape::Value {
                    kind: scalar_kind(ty)
                        .expect("sema permits only scalar fields on a Cli struct"),
                    optional: false,
                    default: field_default(&field.serde_markers),
                },
            };
            CliInputSchema {
                field: field.name.clone(),
                flag,
                help,
                metavar: (!matches!(shape, CliInputShape::Flag)).then_some(metavar),
                shape,
            }
        })
        .collect();

    Some(CliCommandSchema {
        entry_type: structure.name.clone(),
        inputs,
    })
}

fn scalar_kind(ty: &Type) -> Option<CliValueKind> {
    match ty {
        Type::Bool => Some(CliValueKind::Bool),
        Type::Int => Some(CliValueKind::Int),
        Type::Float => Some(CliValueKind::Float),
        Type::String => Some(CliValueKind::String),
        Type::Named(name) if name == "Path" => Some(CliValueKind::Path),
        _ => None,
    }
}

fn marker<'a>(markers: &'a [Marker], name: &str) -> Option<&'a Marker> {
    markers.iter().find(|marker| marker.name == name)
}

fn marker_string(marker: &Marker) -> Option<String> {
    match marker.args.first() {
        Some(Expr::Str(parts, _)) if parts.len() == 1 => match &parts[0] {
            StrPart::Lit(value) => Some(value.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn field_default(markers: &[Marker]) -> Option<CliDefault> {
    let marker = marker(markers, Syntax::ATTR_DEFAULT)?;
    Some(match (&marker.args[..], &marker.ct) {
        ([_, ..], Some(value)) => CliDefault::Value(value.clone()),
        _ => CliDefault::TypeDefault,
    })
}
