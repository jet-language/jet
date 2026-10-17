// One decode-diagnostic vocabulary for every Dyn representation.
// Engines pass only their resident variant tag; the meaning stays here.
pub fn dyn_kind(tag: &str) -> &'static str {
    match tag {
        "Null" => "null",
        "Bool" => "Bool",
        "Int" => "Int",
        "Float" => "Float",
        "Number" => "number",
        "Text" => "Text",
        "Bytes" => "Bytes",
        "DateTime" => "DateTime",
        "Array" => "a list",
        "Object" => "an object",
        _ => "value",
    }
}
