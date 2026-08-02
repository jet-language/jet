// D-TYPEDTEXT1=D: one typed-text semantic core for AOT and TIR adapters.

pub fn jet_typed_sql_raw(template: String) -> (String, Vec<String>) {
    (template, Vec::new())
}

pub fn jet_typed_sql_interpolate(literals: &[&str], holes: Vec<String>) -> (String, Vec<String>) {
    let mut template = String::new();
    for (index, literal) in literals.iter().enumerate() {
        template.push_str(literal);
        if index < holes.len() {
            template.push('?');
        }
    }
    (template, holes)
}

pub fn jet_typed_sql_template(value: &(String, Vec<String>)) -> String {
    value.0.clone()
}

pub fn jet_typed_sql_params(value: &(String, Vec<String>)) -> Vec<String> {
    value.1.clone()
}

pub fn jet_typed_html_raw(value: String) -> String {
    value
}

pub fn jet_typed_html_text(value: String) -> String {
    value
}

pub fn jet_typed_html_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

pub fn jet_typed_html_interpolate(literals: &[&str], holes: Vec<String>) -> String {
    let mut out = String::new();
    for (index, literal) in literals.iter().enumerate() {
        out.push_str(literal);
        if let Some(hole) = holes.get(index) {
            out.push_str(&jet_typed_html_escape(hole));
        }
    }
    out
}

pub fn jet_typed_sh_raw(value: String) -> Vec<String> {
    value.split_whitespace().map(str::to_string).collect()
}

pub fn jet_typed_sh_interpolate(literals: &[&str], holes: Vec<String>) -> Vec<String> {
    let mut argv = Vec::new();
    for (index, literal) in literals.iter().enumerate() {
        argv.extend(literal.split_whitespace().map(str::to_string));
        if let Some(hole) = holes.get(index) {
            argv.push(hole.clone());
        }
    }
    argv
}
