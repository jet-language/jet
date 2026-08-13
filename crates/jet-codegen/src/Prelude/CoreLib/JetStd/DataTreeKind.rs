// One decode-diagnostic vocabulary for every DataTree representation.
// Engines pass only their resident variant tag; the meaning stays here.
pub fn datatree_kind(tag: &str) -> &'static str {
    match tag {
        "Null" => "null",
        "Bool" => "Bool",
        "Int" => "Int",
        "Float" => "Float",
        "Text" => "Text",
        "Bytes" => "Bytes",
        "Array" => "a list",
        "Object" => "an object",
        _ => "value",
    }
}
