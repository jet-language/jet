// ──────────────────────────────────────────────
// Public API surface extraction
// ──────────────────────────────────────────────

/// An item in a package's public API. Two `ApiItem`s are "compatible" when they
/// have the same `kind`, `name`, and `signature` (a textual canonical form).
/// We store the signature as a string because full AST comparison is brittle;
/// the canonical form gives false-negative safety (we might miss a breaking
/// change in a complex generic; that is acceptable for v1).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ApiItem {
    /// "fn", "struct", "enum", "trait", "const"
    pub kind: String,
    pub name: String,
    /// Textual canonical form of the signature (param/field types, return type).
    /// Does not include the body. Whitespace-normalised.
    pub signature: String,
}

/// Extract the public API surface from a parsed Jet source file.
/// Only `pub` items at the top level are included.
pub fn extract_public_api(src: &str, file: &str) -> Vec<ApiItem> {
    use crate::loader;

    let bundle = match loader::load_entry_with_overlay(file, None, true) {
        Ok(b) => b,
        Err(_) => return vec![],
    };
    let _ = src; // bundle already loaded

    let mut out = Vec::new();
    // Entry file items (the main module).
    let entry = &bundle.modules[bundle.entry];
    for item in &entry.items {
        if let Some(api) = public_api_of_item(item) {
            out.push(api);
        }
    }
    out.sort();
    out
}

/// Build an `ApiItem` for a single AST item, or `None` if it is private.
fn public_api_of_item(item: &crate::ast::Item) -> Option<ApiItem> {
    use crate::ast::Item;
    match item {
        Item::Func(f) if f.is_pub => Some(ApiItem {
            kind: "fn".into(),
            name: f.name.clone(),
            signature: format_fn_sig(f),
        }),
        Item::Struct(s) if s.is_pub => Some(ApiItem {
            kind: "struct".into(),
            name: s.name.clone(),
            signature: format_struct_sig(s),
        }),
        Item::Enum(e) if e.is_pub => Some(ApiItem {
            kind: "enum".into(),
            name: e.name.clone(),
            signature: format_enum_sig(e),
        }),
        Item::Trait(t) if t.is_pub => Some(ApiItem {
            kind: "trait".into(),
            name: t.name.clone(),
            signature: format_trait_sig(t),
        }),
        // ConstDef does not carry is_pub in v1 — consts are accessible by name
        // and the pub distinction is enforced at use sites by sema. Skip from
        // public API for now; revisit when const visibility is added to the AST.
        Item::Const(_c) => None,
        _ => None,
    }
}

fn format_type(ty: &crate::ast::Type) -> String {
    ty.show()
}

fn format_fn_sig(f: &crate::ast::Func) -> String {
    use crate::ast::AccessConvention;
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| {
            let prefix = match p.convention {
                AccessConvention::Read => "",
                AccessConvention::Mutate => "mut ",
                AccessConvention::Move => "take ",
            };
            format!("{}{}: {}", prefix, p.name, format_type(&p.ty))
        })
        .collect();
    let ret = match &f.return_type {
        Some(t) => format!(" -> {}", format_type(t)),
        None => String::new(),
    };
    format!("fn {}({}){}", f.name, params.join(", "), ret)
}

fn format_struct_sig(s: &crate::ast::StructDef) -> String {
    let fields: Vec<String> = s
        .fields
        .iter()
        .map(|f| format!("{}: {}", f.name, format_type(&f.ty)))
        .collect();
    format!("struct {} {{ {} }}", s.name, fields.join("; "))
}

fn format_enum_sig(e: &crate::ast::EnumDef) -> String {
    let variants: Vec<String> = e
        .variants
        .iter()
        .map(|v| v.name.clone())
        .collect();
    format!("enum {} {{ {} }}", e.name, variants.join(", "))
}

fn format_trait_sig(t: &crate::ast::TraitDef) -> String {
    let methods: Vec<String> = t
        .methods
        .iter()
        .map(|m| m.name.clone())
        .collect();
    format!("trait {} {{ {} }}", t.name, methods.join(", "))
}
