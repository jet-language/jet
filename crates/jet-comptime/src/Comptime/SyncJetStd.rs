#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
include!("../../../jet-codegen/src/Prelude/CoreLib/JetStd/WireOrder.rs");
include!("../../../jet-codegen/src/Prelude/CoreLib/JetStd/DataTreeKind.rs");
include!("../../../jet-codegen/src/Prelude/CoreLib/JetStd/DataTree.rs");

fn quote_json(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn render_datatree_json(tree: &DataTree, pretty: bool, depth: usize) -> String {
    match tree {
        DataTree::Null => "null".to_string(),
        DataTree::Bool(value) => value.to_string(),
        DataTree::Int(value) => value.to_string(),
        DataTree::Float(value) => format!("{:?}", value),
        DataTree::Text(value) => quote_json(value),
        DataTree::Bytes(values) => format!(
            "[{}]",
            values.iter().map(u8::to_string).collect::<Vec<_>>().join(",")
        ),
        DataTree::Array(values) => {
            if values.is_empty() {
                return "[]".to_string();
            }
            if !pretty {
                return format!(
                    "[{}]",
                    values
                        .iter()
                        .map(|value| render_datatree_json(value, false, depth))
                        .collect::<Vec<_>>()
                        .join(",")
                );
            }
            let pad = "  ".repeat(depth + 1);
            let end = "  ".repeat(depth);
            let parts = values
                .iter()
                .map(|value| format!("{}{}", pad, render_datatree_json(value, true, depth + 1)))
                .collect::<Vec<_>>();
            format!("[\n{}\n{}]", parts.join(",\n"), end)
        }
        DataTree::Object(fields) => {
            if fields.is_empty() {
                return "{}".to_string();
            }
            if !pretty {
                return format!(
                    "{{{}}}",
                    fields
                        .iter()
                        .map(|(key, value)| {
                            format!(
                                "{}:{}",
                                quote_json(key),
                                render_datatree_json(value, false, depth)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                );
            }
            let pad = "  ".repeat(depth + 1);
            let end = "  ".repeat(depth);
            let parts = fields
                .iter()
                .map(|(key, value)| {
                    format!(
                        "{}{}: {}",
                        pad,
                        quote_json(key),
                        render_datatree_json(value, true, depth + 1)
                    )
                })
                .collect::<Vec<_>>();
            format!("{{\n{}\n{}}}", parts.join(",\n"), end)
        }
    }
}
