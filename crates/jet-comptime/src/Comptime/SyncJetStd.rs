#[allow(unused_imports)]
pub use jet_foundation::Outcome::*;
// D-DBPOLICY1=A: the one closed row-policy language. `Top/Sync.rs` reaches it
// through `jet_std::jet_db_policy_compile` on this tier exactly as it does in
// the AOT Prelude, so comptime cannot accept a policy AOT rejects (I9).
include!("../../../jet-codegen/src/Prelude/CoreLib/JetStd/RowPolicy.rs");
include!("../../../jet-codegen/src/Prelude/CoreLib/JetStd/WireOrder.rs");
include!("../../../jet-codegen/src/Prelude/CoreLib/JetStd/DataTreeKind.rs");
// The one FieldError projection (I9): `DataTree.rs`'s `JetShow`/`JetDisplay`
// impls call it, exactly as the AOT Prelude and the Cranelift host do.
include!("../../../jet-codegen/src/Prelude/Core/FieldError.rs");
include!("../../../jet-codegen/src/Prelude/CoreLib/JetStd/DataTree.rs");
jet_datatree_decode_helpers!();

// I9 marshalling adapters for the exact-`Int` carrier (D-INTBIG1). AOT packs
// out-of-range values into a std-only arena and the JIT parks them in its
// runtime heap; this comptime mirror of the `jet_std` wire surface keeps `Int`
// in a plain `i64`, so the packed carrier collapses to identity here. Text
// still goes through the same arbitrary-precision parse the AOT carrier uses,
// and only genuinely unrepresentable values are refused.
pub fn jet_int_from_i64(value: i64) -> i64 {
    value
}

pub fn jet_int_from_str(value: &str) -> Result<i64, String> {
    crate::Numeric::CtBigInt::from_str(value)?
        .try_i64()
        .ok_or_else(|| format!("exact Int `{value}` is out of range for this tier"))
}

pub fn jet_int_to_i64(value: i64) -> Option<i64> {
    Some(value)
}

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
        DataTree::Number(text) => text.clone(),
        DataTree::TypedText(text) => quote_json(text),
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
