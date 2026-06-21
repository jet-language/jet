//! Comptime value model: `CtValue` / `CtKey` and the `mangle` helper.

use std::collections::BTreeMap;

use crate::AST::Type;

/// A fully-evaluated compile-time value. Self-describing: the Jet type is
/// recovered by [`CtValue::jet_type`] and the Rust literal by
/// [`CtValue::serialize`].
#[derive(Clone, Debug, PartialEq)]
pub enum CtValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Char(char),
    Str(String),
    List(Vec<CtValue>),
    Map(BTreeMap<CtKey, CtValue>),
    Struct {
        type_name: String,
        fields: Vec<(String, CtValue)>,
    },
    Enum {
        type_name: String,
        variant: String,
        args: Vec<(Option<String>, CtValue)>,
    },
    Some(Box<CtValue>),
    None(Type),
    Unit,
}

/// Orderable map key (S38: maps are `BTreeMap`, so keys must be `Ord`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CtKey {
    Int(i64),
    Str(String),
    Bool(bool),
    Char(char),
}

impl CtKey {
    pub(super) fn from_value(v: CtValue) -> Option<CtKey> {
        match v {
            CtValue::Int(n) => Some(CtKey::Int(n)),
            CtValue::Str(s) => Some(CtKey::Str(s)),
            CtValue::Bool(b) => Some(CtKey::Bool(b)),
            CtValue::Char(c) => Some(CtKey::Char(c)),
            _ => None,
        }
    }
    pub(super) fn to_value(&self) -> CtValue {
        match self {
            CtKey::Int(n) => CtValue::Int(*n),
            CtKey::Str(s) => CtValue::Str(s.clone()),
            CtKey::Bool(b) => CtValue::Bool(*b),
            CtKey::Char(c) => CtValue::Char(*c),
        }
    }
    fn jet_type(&self) -> Type {
        match self {
            CtKey::Int(_) => Type::Int,
            CtKey::Str(_) => Type::String,
            CtKey::Bool(_) => Type::Bool,
            CtKey::Char(_) => Type::Char,
        }
    }
    fn jet_show(&self) -> String {
        self.to_value().jet_show()
    }
}

impl CtValue {
    /// The Jet type this value inhabits — used to register the binding so
    /// the rest of the program type-checks references to it.
    pub fn jet_type(&self) -> Type {
        match self {
            CtValue::Int(_) => Type::Int,
            CtValue::Float(_) => Type::Float,
            CtValue::Bool(_) => Type::Bool,
            CtValue::Char(_) => Type::Char,
            CtValue::Str(_) => Type::String,
            CtValue::List(xs) => {
                let inner = xs.first().map(|x| x.jet_type()).unwrap_or(Type::Int);
                Type::List(Box::new(inner))
            }
            CtValue::Map(m) => {
                let (k, v) = m
                    .iter()
                    .next()
                    .map(|(k, v)| (k.jet_type(), v.jet_type()))
                    .unwrap_or((Type::String, Type::Int));
                Type::Map {
                    key: Box::new(k),
                    value: Box::new(v),
                }
            }
            CtValue::Some(inner) => Type::Option(Box::new(inner.jet_type())),
            CtValue::None(t) => Type::Option(Box::new(t.clone())),
            CtValue::Struct { type_name, .. } | CtValue::Enum { type_name, .. } => {
                Type::Named(type_name.clone())
            }
            CtValue::Unit => Type::Named(String::new()),
        }
    }

    /// Runtime display, identical to the generated `JetShow` impls (codegen
    /// PRELUDE). This is what string interpolation produces.
    pub fn jet_show(&self) -> String {
        match self {
            CtValue::Int(n) => n.to_string(),
            CtValue::Float(f) => format!("{:?}", f),
            CtValue::Bool(b) => b.to_string(),
            CtValue::Char(c) => c.to_string(),
            CtValue::Str(s) => s.clone(),
            CtValue::List(xs) => {
                let parts: Vec<String> = xs.iter().map(|x| x.jet_show()).collect();
                format!("[{}]", parts.join(", "))
            }
            CtValue::Map(m) => {
                let parts: Vec<String> = m
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k.jet_show(), v.jet_show()))
                    .collect();
                format!("[{}]", parts.join(", "))
            }
            CtValue::Some(v) => v.jet_show(),
            CtValue::None(_) => "null".to_string(),
            CtValue::Struct { type_name, fields } => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(n, v)| format!("{}: {}", n, v.jet_show()))
                    .collect();
                format!("{}({})", type_name, parts.join(", "))
            }
            CtValue::Enum { variant, .. } => variant.clone(),
            CtValue::Unit => String::new(),
        }
    }

    /// Human-readable, Jet-typed pretty output (D-EVAL1=A).
    ///
    /// Structs render as `Name { field: value, … }`, lists as `[…]`, maps as
    /// `{key: value, …}`. Scalars render with their Jet literal syntax.
    /// Nested values indent by 2 spaces per level. `--json` callers use
    /// `serialize()` (byte-stable) instead.
    pub fn render_pretty(&self) -> String {
        let mut out = String::new();
        self.render_pretty_inner(&mut out, 0);
        out
    }

    fn render_pretty_inner(&self, out: &mut String, depth: usize) {
        let indent = "  ".repeat(depth);
        let inner_indent = "  ".repeat(depth + 1);
        match self {
            CtValue::Int(n) => out.push_str(&n.to_string()),
            CtValue::Float(f) => out.push_str(&format!("{:?}", f)),
            CtValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            CtValue::Char(c) => {
                out.push('\'');
                out.push(*c);
                out.push('\'');
            }
            CtValue::Str(s) => {
                out.push('"');
                out.push_str(s);
                out.push('"');
            }
            CtValue::List(xs) => {
                if xs.is_empty() {
                    out.push_str("[]");
                } else {
                    out.push_str("[\n");
                    for item in xs {
                        out.push_str(&inner_indent);
                        item.render_pretty_inner(out, depth + 1);
                        out.push_str(",\n");
                    }
                    out.push_str(&indent);
                    out.push(']');
                }
            }
            CtValue::Map(m) => {
                if m.is_empty() {
                    out.push_str("{}");
                } else {
                    out.push_str("{\n");
                    for (k, v) in m {
                        out.push_str(&inner_indent);
                        out.push_str(&k.jet_show());
                        out.push_str(": ");
                        v.render_pretty_inner(out, depth + 1);
                        out.push_str(",\n");
                    }
                    out.push_str(&indent);
                    out.push('}');
                }
            }
            CtValue::Struct { type_name, fields } => {
                if fields.is_empty() {
                    out.push_str(type_name);
                    out.push_str(" {}");
                } else {
                    out.push_str(type_name);
                    out.push_str(" {\n");
                    for (name, value) in fields {
                        out.push_str(&inner_indent);
                        out.push_str(name);
                        out.push_str(": ");
                        value.render_pretty_inner(out, depth + 1);
                        out.push_str(",\n");
                    }
                    out.push_str(&indent);
                    out.push('}');
                }
            }
            CtValue::Enum { type_name, variant, args } => {
                out.push_str(type_name);
                out.push_str("::");
                out.push_str(variant);
                if !args.is_empty() {
                    out.push('(');
                    let mut first = true;
                    for (label, v) in args {
                        if !first { out.push_str(", "); }
                        first = false;
                        if let Some(lbl) = label {
                            out.push_str(lbl);
                            out.push_str(": ");
                        }
                        v.render_pretty_inner(out, depth);
                    }
                    out.push(')');
                }
            }
            CtValue::Some(v) => {
                out.push_str("Some(");
                v.render_pretty_inner(out, depth);
                out.push(')');
            }
            CtValue::None(_) => out.push_str("None"),
            CtValue::Unit => out.push_str("()"),
        }
    }

    /// Stable JSON output for `jet eval --pure --json` (machine consumers).
    /// Byte-stable: same program always produces the same bytes.
    pub fn to_json(&self) -> String {
        match self {
            CtValue::Int(n) => n.to_string(),
            CtValue::Float(f) => {
                let s = format!("{:?}", f);
                // Ensure it contains a decimal point so JSON consumers treat it as float.
                if s.contains('.') || s.contains('e') || s.contains('E') {
                    s
                } else {
                    format!("{}.0", s)
                }
            }
            CtValue::Bool(b) => b.to_string(),
            CtValue::Char(c) => format!("\"{}\"", c),
            CtValue::Str(s) => {
                // JSON-escape the string.
                let mut out = String::from('"');
                for ch in s.chars() {
                    match ch {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        c => out.push(c),
                    }
                }
                out.push('"');
                out
            }
            CtValue::List(xs) => {
                let parts: Vec<String> = xs.iter().map(|x| x.to_json()).collect();
                format!("[{}]", parts.join(","))
            }
            CtValue::Map(m) => {
                let parts: Vec<String> = m
                    .iter()
                    .map(|(k, v)| format!("{}:{}", k.to_value().to_json(), v.to_json()))
                    .collect();
                format!("{{{}}}", parts.join(","))
            }
            CtValue::Struct { fields, .. } => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(n, v)| format!("\"{}\":{}", n, v.to_json()))
                    .collect();
                format!("{{{}}}", parts.join(","))
            }
            CtValue::Enum { variant, args, .. } => {
                if args.is_empty() {
                    format!("\"{}\"", variant)
                } else {
                    let parts: Vec<String> = args
                        .iter()
                        .map(|(label, v)| {
                            if let Some(lbl) = label {
                                format!("\"{}\":{}", lbl, v.to_json())
                            } else {
                                v.to_json()
                            }
                        })
                        .collect();
                    if args.iter().all(|(label, _)| label.is_some()) {
                        format!("{{\"{}\":{{{}}}}}", variant, parts.join(","))
                    } else {
                        format!("{{\"{}\":[{}]}}", variant, parts.join(","))
                    }
                }
            }
            CtValue::Some(v) => v.to_json(),
            CtValue::None(_) => "null".to_string(),
            CtValue::Unit => "null".to_string(),
        }
    }

    /// A Rust expression that reconstructs this value, matching codegen's
    /// `emit_expr` representations exactly (Vec, BTreeMap, Option, owned
    /// String). Inlined at each use site (codegen stays dumb, I3).
    pub fn serialize(&self) -> String {
        match self {
            CtValue::Int(n) => format!("{}i64", n),
            CtValue::Float(f) => format!("{:?}f64", f),
            CtValue::Bool(b) => b.to_string(),
            CtValue::Char(c) => format!("{:?}", c),
            CtValue::Str(s) => format!("{:?}.to_string()", s),
            CtValue::List(xs) => {
                let parts: Vec<String> = xs.iter().map(|x| x.serialize()).collect();
                format!("vec![{}]", parts.join(", "))
            }
            CtValue::Map(m) => {
                if m.is_empty() {
                    "std::collections::BTreeMap::new()".to_string()
                } else {
                    let mut s = String::from("{ let mut _m = std::collections::BTreeMap::new(); ");
                    for (k, v) in m {
                        s.push_str(&format!(
                            "_m.insert(({}), {}); ",
                            k.to_value().serialize(),
                            v.serialize()
                        ));
                    }
                    s.push_str("_m }");
                    s
                }
            }
            CtValue::Some(v) => format!("Some({})", v.serialize()),
            CtValue::None(_) => "None".to_string(),
            CtValue::Struct { type_name, fields } => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(n, v)| format!("{}: {}", mangle(n), v.serialize()))
                    .collect();
                format!("user_{} {{ {} }}", type_name, parts.join(", "))
            }
            CtValue::Enum {
                type_name,
                variant,
                args,
            } => {
                let prefix = format!("user_{}::{}", type_name, mangle(variant));
                if args.is_empty() {
                    prefix
                } else if args.iter().all(|(label, _)| label.is_none()) {
                    let parts: Vec<String> = args.iter().map(|(_, v)| v.serialize()).collect();
                    format!("{}({})", prefix, parts.join(", "))
                } else {
                    let parts: Vec<String> = args
                        .iter()
                        .filter_map(|(label, v)| {
                            label
                                .as_ref()
                                .map(|name| format!("{}: {}", mangle(name), v.serialize()))
                        })
                        .collect();
                    format!("{} {{ {} }}", prefix, parts.join(", "))
                }
            }
            CtValue::Unit => "()".to_string(),
        }
    }
}

fn mangle(name: &str) -> String {
    if name == "main" {
        "main".to_string()
    } else {
        format!("user_{}", name)
    }
}
