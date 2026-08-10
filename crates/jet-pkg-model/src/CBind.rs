//! Native C-header → Jet `#Bindgen` cache generator (E2-M14).
//!
//! Owner 2026-06-18: this supersedes the D-CBIND3=B `bindgen` route. The shipped
//! `jet` stays std-only (I6) — no `bindgen`, no libclang. This is a focused
//! parser for **C function prototypes** over the type subset Jet's FFI binds
//! (scalars, `char*` strings, `void`). A declaration it cannot map is *skipped
//! and reported* — never faked (I3). Anything beyond this subset is hand-written
//! as an `#Extern module c.<lib>` overlay, which still wins on merge.
//!
//! Output is a `#Bindgen module c.<lib>.__bindgen__ { … }` cache as parsed by
//! `src/cffi.rs`; each binding is `fn name(p: T, …) [=> R] = "c_symbol";`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::JSON::JSONValue;

/// Compute the bind hash for a header + cflags pair (Phase 3 invalidation).
/// The hash is SHA-256(header_src || "\0" || cflags_str) rendered as 64 hex digits.
/// `cflags` is a space-joined list of flags; pass `""` when there are none.
pub fn compute_bind_hash(header_src: &str, cflags: &str) -> String {
    let mut input = Vec::with_capacity(header_src.len() + 1 + cflags.len());
    input.extend_from_slice(header_src.as_bytes());
    input.push(0u8); // null separator
    input.extend_from_slice(cflags.as_bytes());
    crate::SHA256::sha256_hex(&input)
}

/// Sidecar filename for the hash that accompanies `<lib>.jet` in the cache.
pub fn hash_sidecar_path(cache_path: &std::path::Path) -> std::path::PathBuf {
    cache_path.with_extension("hash")
}

/// Read the stored hash from the sidecar, or `None` if absent / unreadable.
pub fn read_stored_hash(cache_path: &std::path::Path) -> Option<String> {
    let sidecar = hash_sidecar_path(cache_path);
    std::fs::read_to_string(sidecar)
        .ok()
        .map(|s| s.trim().to_string())
}

/// Write a hash sidecar next to `cache_path`. Returns `Ok(())` on success.
pub fn write_bind_hash(
    cache_path: &std::path::Path,
    header_src: &str,
    cflags: &str,
) -> std::io::Result<()> {
    let hash = compute_bind_hash(header_src, cflags);
    let sidecar = hash_sidecar_path(cache_path);
    std::fs::write(sidecar, hash)
}

/// Result of translating a header: the cache source plus what was/wasn't bound.
pub struct BindResult {
    /// The `#Bindgen module …` cache file text.
    pub source: String,
    /// Function names successfully bound.
    pub bound: Vec<String>,
    /// `(name, reason)` for prototypes skipped because a type isn't bindable.
    pub skipped: Vec<(String, String)>,
}

/// Translate C header source into a `#Bindgen` cache for library `lib`.
/// `Err` only on a structural failure (no bindable function found at all), so
/// the caller can surface E3208 honestly instead of writing an empty cache.
pub fn generate(header_src: &str, lib: &str) -> Result<BindResult, String> {
    let cleaned = strip_comments_and_directives(header_src);
    let mut bound = Vec::new();
    let mut skipped = Vec::new();
    let mut lines = String::new();

    for decl in split_declarations(&cleaned) {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        match parse_prototype(decl) {
            Some(proto) => match render_binding(&proto) {
                Ok(line) => {
                    bound.push(proto.name.clone());
                    lines.push_str("    ");
                    lines.push_str(&line);
                    lines.push('\n');
                }
                Err(reason) => skipped.push((proto.name, reason)),
            },
            None => {}
        }
    }

    if bound.is_empty() {
        return Err(format!(
            "no bindable C function prototypes found for `{}`",
            lib
        ));
    }

    let source = format!("#Bindgen module c.{}.__bindgen__ {{\n{}}}\n", lib, lines);
    Ok(BindResult {
        source,
        bound,
        skipped,
    })
}

/// One parsed C function prototype.
struct Proto {
    ret: String,
    name: String,
    params: Vec<String>,
}

/// Remove `/* … */` and `//` comments and preprocessor directives (`#…`,
/// honouring `\`-continued lines). Conservative: keeps everything else verbatim.
fn strip_comments_and_directives(src: &str) -> String {
    // 1. Strip comments.
    let mut out = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            out.push(' ');
        } else if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    // 2. Drop preprocessor directives (line-continuation aware).
    let mut result = String::with_capacity(out.len());
    let mut continued = false;
    for line in out.lines() {
        let trimmed = line.trim_start();
        if continued || trimmed.starts_with('#') {
            continued = line.trim_end().ends_with('\\');
            continue;
        }
        result.push_str(line);
        result.push('\n');
    }
    result
}

/// Split top-level declarations on `;` and skip brace-delimited bodies (struct /
/// enum / union definitions, inline function bodies). Returns the text up to
/// each `;` that sits at brace-depth 0.
fn split_declarations(src: &str) -> Vec<String> {
    let mut decls = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    for ch in src.chars() {
        match ch {
            '{' => {
                depth += 1;
                cur.push(ch);
            }
            '}' => {
                depth -= 1;
                cur.push(ch);
            }
            ';' if depth == 0 => {
                decls.push(std::mem::take(&mut cur));
            }
            _ => cur.push(ch),
        }
    }
    decls
}

/// Try to read `decl` as a function prototype `<ret> name(<params>)`.
/// Returns None if it isn't a prototype (no parameter list, has a body, etc.).
fn parse_prototype(decl: &str) -> Option<Proto> {
    // A definition (`… ) { … }`) has a brace — headers declare, so skip bodies.
    if decl.contains('{') {
        return None;
    }
    let open = decl.find('(')?;
    let close = decl.rfind(')')?;
    if close < open || !decl[close + 1..].trim().is_empty() {
        return None;
    }
    let before = decl[..open].trim();
    let params_src = decl[open + 1..close].trim();

    // The function name is the last identifier in `before`; everything earlier
    // is the return type. Strip a leading `*` that belongs to the return type.
    let name_start = match before
        .char_indices()
        .rev()
        .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_'))
    {
        Some((i, separator)) if separator.is_ascii() => i + separator.len_utf8(),
        Some(_) => return None,
        None => 0,
    };
    let name = &before[name_start..];
    if !is_ident(name) {
        return None;
    }
    let ret = before[..name_start].trim().to_string();
    if ret.is_empty() {
        // No return type at all (e.g. a bare `foo(x)` K&R decl) — not in scope.
        return None;
    }

    let params = split_params(params_src)?;
    Some(Proto {
        ret,
        name: name.to_string(),
        params,
    })
}

/// Split a parameter list on top-level commas (no nested parens in this subset).
fn split_params(src: &str) -> Option<Vec<String>> {
    let s = src.trim();
    if s.is_empty() || s == "void" {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    for ch in s.chars() {
        match ch {
            '(' => {
                depth += 1;
                cur.push(ch);
            }
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
                cur.push(ch);
            }
            ',' if depth == 0 => {
                if cur.trim().is_empty() {
                    return None;
                }
                out.push(std::mem::take(&mut cur));
            }
            _ => cur.push(ch),
        }
    }
    if depth != 0 || cur.trim().is_empty() {
        return None;
    }
    out.push(cur);
    Some(out)
}

/// Render one prototype as a Jet `#Bindgen` line, or Err(reason) if a type in it
/// isn't bindable in this subset.
fn render_binding(p: &Proto) -> Result<String, String> {
    let ret_jet = match map_return_type(&p.ret) {
        Ok(t) => t,
        Err(e) => return Err(e),
    };
    let mut params = Vec::new();
    for (idx, raw) in p.params.iter().enumerate() {
        let raw = raw.trim();
        if raw == "..." {
            return Err("variadic (`...`) parameters aren't bindable".to_string());
        }
        let (ty, name) = split_param_type_and_name(raw, idx);
        let jet_ty = map_type(&ty).ok_or_else(|| format!("type `{}` isn't bindable", ty.trim()))?;
        params.push(format!("{}: {}", name, jet_ty));
    }
    let params_str = params.join(", ");
    let line = match ret_jet {
        Some(r) => format!("fn {}({}) => {} = \"{}\";", p.name, params_str, r, p.name),
        None => format!("fn {}({}) = \"{}\";", p.name, params_str, p.name),
    };
    Ok(line)
}

/// Map a C return type to `Some(JetType)` / `None` for `void`. Err if unbindable.
fn map_return_type(c: &str) -> Result<Option<String>, String> {
    let norm = normalize_type(c);
    if norm == "void" {
        return Ok(None);
    }
    match map_type(c) {
        Some(t) => Ok(Some(t)),
        None => Err(format!("return type `{}` isn't bindable", c.trim())),
    }
}

/// Separate a parameter's type from its (optional) name, synthesising `argN`.
fn split_param_type_and_name(raw: &str, idx: usize) -> (String, String) {
    let raw = raw.trim();
    // A trailing identifier not glued to a `*` is the parameter name.
    let last_non_ident = raw.rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'));
    if let Some(pos) = last_non_ident {
        let tail = &raw[pos + 1..];
        let sep = raw.as_bytes()[pos];
        // `int *p` / `int x`: a name follows a `*` or whitespace. `char*` (no
        // name) leaves an empty tail.
        if is_ident(tail) && (sep == b'*' || sep.is_ascii_whitespace()) && !tail.is_empty() {
            // Don't treat a lone type keyword (`int`) as a name.
            let ty = raw[..pos + 1].trim();
            if !ty.is_empty() {
                return (ty.to_string(), tail.to_string());
            }
        }
    }
    (raw.to_string(), format!("arg{}", idx))
}

/// Normalise a C type: collapse whitespace, drop qualifiers we ignore.
fn normalize_type(c: &str) -> String {
    let mut toks: Vec<&str> = c.split_whitespace().collect();
    toks.retain(|t| {
        !matches!(
            *t,
            "const" | "volatile" | "register" | "restrict" | "extern"
        )
    });
    toks.join(" ")
}

/// Map a C type to a Jet FFI type, or None if it's outside the bound subset.
fn map_type(c: &str) -> Option<String> {
    let norm = normalize_type(c);
    let t = norm.trim();
    // Pointers: only `char *` (any signedness) maps — to `String`. Other
    // pointers cross the C boundary as raw addresses (E3202) and are out of
    // scope for the auto-binder.
    if t.ends_with('*') {
        let base = t[..t.len() - 1].trim();
        let base = normalize_type(base);
        return match base.as_str() {
            "char" | "signed char" | "unsigned char" => Some("String".to_string()),
            _ => None,
        };
    }
    match t {
        "void" => None,
        "bool" | "_Bool" => Some("Bool".to_string()),
        "float" | "double" | "long double" => Some("Float".to_string()),
        // Integer family (signedness/width all land on Jet's 64-bit `Int`).
        "char" | "signed char" | "unsigned char" | "short" | "unsigned short" | "short int"
        | "unsigned short int" | "int" | "unsigned" | "unsigned int" | "signed" | "signed int"
        | "long" | "unsigned long" | "long int" | "unsigned long int" | "long long"
        | "unsigned long long" | "long long int" | "size_t" | "ssize_t" | "ptrdiff_t"
        | "intptr_t" | "uintptr_t" | "int8_t" | "int16_t" | "int32_t" | "int64_t" | "uint8_t"
        | "uint16_t" | "uint32_t" | "uint64_t" => Some("Int".to_string()),
        _ => None,
    }
}

fn is_ident(s: &str) -> bool {
    let s = s.trim();
    !s.is_empty()
        && s.chars()
            .next()
            .map(|c| c.is_ascii_alphabetic() || c == '_')
            .unwrap_or(false)
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// ── data-schema binders (D-BOUND-BIND1=A) ────────────────────────────────

/// Result of binding a data schema into ordinary, visible Jet source.
pub struct DataBindResult {
    pub source: String,
    pub record_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum BoundType {
    Scalar(&'static str),
    Named(String),
    Array(Box<BoundType>),
    Optional(Box<BoundType>),
    Dynamic,
}

impl BoundType {
    fn render(&self) -> String {
        match self {
            Self::Scalar(name) => (*name).to_string(),
            Self::Named(name) => name.clone(),
            Self::Array(element) => format!("[{}]", element.render()),
            Self::Optional(value) => format!("{}?", value.render()),
            Self::Dynamic => "DataTree".to_string(),
        }
    }
}

struct FieldSeed {
    wire_name: String,
    ty: BoundType,
    optional: bool,
    note: Option<String>,
}

struct BoundField {
    wire_name: String,
    name: String,
    ty: BoundType,
    optional: bool,
    note: Option<String>,
}

struct BoundRecord {
    name: String,
    fields: Vec<BoundField>,
}

struct SchemaBuilder {
    records: Vec<BoundRecord>,
    used_type_names: BTreeSet<String>,
}

impl SchemaBuilder {
    fn new() -> Self {
        Self {
            records: Vec::new(),
            used_type_names: BTreeSet::new(),
        }
    }

    fn begin_record(&mut self, preferred: &str) -> (String, usize) {
        let name = unique_name(&mut self.used_type_names, &sanitize_type_name(preferred));
        let index = self.records.len();
        self.records.push(BoundRecord {
            name: name.clone(),
            fields: Vec::new(),
        });
        (name, index)
    }

    fn set_fields(&mut self, index: usize, seeds: Vec<FieldSeed>) {
        let mut used = BTreeSet::new();
        let fields = seeds
            .into_iter()
            .map(|seed| {
                let base = sanitize_field_name(&seed.wire_name);
                let name = unique_name(&mut used, &base);
                BoundField {
                    wire_name: seed.wire_name,
                    name,
                    ty: seed.ty,
                    optional: seed.optional,
                    note: seed.note,
                }
            })
            .collect();
        self.records[index].fields = fields;
    }
}

fn unique_name(used: &mut BTreeSet<String>, preferred: &str) -> String {
    let preferred = if preferred.is_empty() { "value" } else { preferred };
    if used.insert(preferred.to_string()) {
        return preferred.to_string();
    }
    let mut suffix = 2usize;
    loop {
        let candidate = format!("{preferred}_{suffix}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        suffix += 1;
    }
}

fn is_jet_keyword(name: &str) -> bool {
    matches!(
        name,
        "as"
            | "break"
            | "const"
            | "continue"
            | "distinct"
            | "else"
            | "enum"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "module"
            | "None"
            | "ok"
            | "pub"
            | "return"
            | "state"
            | "struct"
            | "trait"
            | "true"
            | "type"
            | "use"
            | "where"
            | "while"
            | "Self"
            | "self"
            | "Some"
            | "Ok"
            | "Err"
    )
}

fn sanitize_field_name(wire_name: &str) -> String {
    let source = wire_name
        .strip_prefix('@')
        .or_else(|| wire_name.strip_prefix('$'))
        .unwrap_or(wire_name);
    let mut out = String::new();
    let mut previous_was_lower_or_digit = false;
    for ch in source.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            if ch.is_ascii_uppercase() && previous_was_lower_or_digit && !out.ends_with('_') {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
            previous_was_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        } else {
            if !out.ends_with('_') {
                out.push('_');
            }
            previous_was_lower_or_digit = false;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("field");
    }
    if out.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        out.insert_str(0, "field_");
    }
    if is_jet_keyword(&out) || out.starts_with("__") {
        out.push_str("_field");
    }
    out
}

fn sanitize_type_name(raw: &str) -> String {
    let mut out = String::new();
    let mut upper_next = true;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            if upper_next {
                out.push(ch.to_ascii_uppercase());
                upper_next = false;
            } else {
                out.push(ch);
            }
        } else {
            upper_next = true;
        }
    }
    if out.is_empty() {
        out.push_str("Schema");
    }
    if out.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        out.insert_str(0, "Type");
    }
    if is_jet_keyword(&out) {
        out.push_str("Type");
    }
    out
}

fn pascal_name(raw: &str) -> String {
    sanitize_type_name(raw)
}

fn input_stem_type_name(input_path: &str) -> String {
    let base = input_path
        .rsplit(|ch| ch == '/' || ch == '\\')
        .next()
        .unwrap_or(input_path);
    let stem = base
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(base);
    sanitize_type_name(stem)
}

fn json_values_type(
    builder: &mut SchemaBuilder,
    values: &[&JSONValue],
    hint: &str,
) -> Result<(BoundType, bool), String> {
    let nullable = values.iter().any(|value| matches!(value, JSONValue::Null));
    let nonnull: Vec<&JSONValue> = values
        .iter()
        .copied()
        .filter(|value| !matches!(value, JSONValue::Null))
        .collect();
    if nonnull.is_empty() {
        return Ok((BoundType::Dynamic, true));
    }

    let ty = match nonnull[0] {
        JSONValue::Object(_) => {
            if !nonnull.iter().all(|value| matches!(value, JSONValue::Object(_))) {
                BoundType::Dynamic
            } else {
                let objects: Vec<&JSONValue> = nonnull;
                let name = add_json_record(builder, hint, &objects)?;
                BoundType::Named(name)
            }
        }
        JSONValue::Array(_) => {
            if !nonnull.iter().all(|value| matches!(value, JSONValue::Array(_))) {
                BoundType::Dynamic
            } else {
                let mut elements = Vec::new();
                for value in nonnull {
                    if let JSONValue::Array(items) = value {
                        elements.extend(items.iter());
                    }
                }
                let (element, element_optional) = if elements.is_empty() {
                    (BoundType::Dynamic, false)
                } else {
                    json_values_type(builder, &elements, &format!("{}Item", pascal_name(hint)))?
                };
                let element = if element_optional {
                    BoundType::Optional(Box::new(element))
                } else {
                    element
                };
                BoundType::Array(Box::new(element))
            }
        }
        JSONValue::Bool(_) => {
            if nonnull.iter().all(|value| matches!(value, JSONValue::Bool(_))) {
                BoundType::Scalar("Bool")
            } else {
                BoundType::Dynamic
            }
        }
        JSONValue::Str(_) => {
            if nonnull.iter().all(|value| matches!(value, JSONValue::Str(_))) {
                BoundType::Scalar("String")
            } else {
                BoundType::Dynamic
            }
        }
        JSONValue::Num(_) => {
            if nonnull.iter().all(|value| matches!(value, JSONValue::Num(_))) {
                if nonnull.iter().any(|value| {
                    matches!(value, JSONValue::Num(number) if number.fract() != 0.0)
                }) {
                    BoundType::Scalar("Float")
                } else {
                    BoundType::Scalar("Int")
                }
            } else {
                BoundType::Dynamic
            }
        }
        JSONValue::Null => BoundType::Dynamic,
    };
    Ok((ty, nullable))
}

fn add_json_record(
    builder: &mut SchemaBuilder,
    preferred: &str,
    objects: &[&JSONValue],
) -> Result<String, String> {
    let (name, index) = builder.begin_record(preferred);
    let mut keys = BTreeSet::new();
    for value in objects {
        let object = value.as_object().map_err(|_| {
            "JSON schema contains a non-object where an object was inferred".to_string()
        })?;
        keys.extend(object.keys().cloned());
    }
    let mut fields = Vec::new();
    for key in keys {
        let mut values = Vec::new();
        let mut optional = false;
        for value in objects {
            let object = value.as_object().expect("object checked above");
            if let Some(field) = object.get(&key) {
                values.push(field);
            } else {
                optional = true;
            }
        }
        let child_hint = format!("{}{}", name, pascal_name(&key));
        let (ty, nullable) = json_values_type(builder, &values, &child_hint)?;
        fields.push(FieldSeed {
            wire_name: key,
            ty,
            optional: optional || nullable,
            note: None,
        });
    }
    builder.set_fields(index, fields);
    Ok(name)
}

fn bind_json(input: &str, root_name: &str) -> Result<SchemaBuilder, String> {
    let value = crate::JSON::parse(input).map_err(|error| format!("malformed JSON: {error}"))?;
    if !matches!(&value, JSONValue::Object(_)) {
        return Err("JSON schema root must be an object".to_string());
    }
    let mut builder = SchemaBuilder::new();
    add_json_record(&mut builder, root_name, &[&value])?;
    Ok(builder)
}

fn parse_csv(input: &str) -> Result<Vec<Vec<String>>, String> {
    if input.is_empty() {
        return Err("CSV input is empty".to_string());
    }
    let chars: Vec<char> = input.chars().collect();
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut after_quote = false;
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if quoted {
            match ch {
                '"' if chars.get(index + 1) == Some(&'"') => {
                    field.push('"');
                    index += 2;
                }
                '"' => {
                    quoted = false;
                    after_quote = true;
                    index += 1;
                }
                _ => {
                    field.push(ch);
                    index += 1;
                }
            }
            continue;
        }
        if after_quote {
            match ch {
                ',' => {
                    row.push(std::mem::take(&mut field));
                    after_quote = false;
                    index += 1;
                }
                '\n' => {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                    after_quote = false;
                    index += 1;
                }
                '\r' => {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                    after_quote = false;
                    index += 1;
                    if chars.get(index) == Some(&'\n') {
                        index += 1;
                    }
                }
                _ => return Err("CSV has characters after a closing quote".to_string()),
            }
            continue;
        }
        match ch {
            '"' if field.is_empty() => {
                quoted = true;
                index += 1;
            }
            '"' => return Err("CSV quote must start a field".to_string()),
            ',' => {
                row.push(std::mem::take(&mut field));
                index += 1;
            }
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                index += 1;
            }
            '\r' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                index += 1;
                if chars.get(index) == Some(&'\n') {
                    index += 1;
                }
            }
            _ => {
                field.push(ch);
                index += 1;
            }
        }
    }
    if quoted {
        return Err("CSV has an unterminated quoted field".to_string());
    }
    if after_quote || !row.is_empty() || !field.is_empty() {
        row.push(field);
        rows.push(row);
    }
    while rows.last().is_some_and(Vec::is_empty) {
        rows.pop();
    }
    if rows.len() < 2 {
        return Err("CSV needs a header and at least one data row".to_string());
    }
    Ok(rows)
}

fn csv_type(values: &[String]) -> (BoundType, bool) {
    let optional = values.iter().any(|value| value.is_empty());
    let nonempty: Vec<&str> = values
        .iter()
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .collect();
    if !nonempty.is_empty()
        && nonempty.iter().all(|value| {
            matches!(
                *value,
                "true" | "false" | "TRUE" | "FALSE" | "True" | "False"
            )
        })
    {
        return (BoundType::Scalar("Bool"), optional);
    }
    if !nonempty.is_empty() && nonempty.iter().all(|value| value.parse::<i64>().is_ok()) {
        return (BoundType::Scalar("Int"), optional);
    }
    if !nonempty.is_empty() && nonempty.iter().all(|value| value.parse::<f64>().is_ok()) {
        return (BoundType::Scalar("Float"), optional);
    }
    (BoundType::Scalar("String"), optional)
}

fn bind_csv(input: &str, root_name: &str) -> Result<SchemaBuilder, String> {
    let rows = parse_csv(input)?;
    let headers = &rows[0];
    let mut seen = BTreeSet::new();
    for header in headers {
        if header.trim().is_empty() {
            return Err("CSV has a missing header".to_string());
        }
        if !seen.insert(header.clone()) {
            return Err(format!("CSV has a duplicate header {header}"));
        }
    }
    let width = headers.len();
    let mut columns = vec![Vec::new(); width];
    for row in rows.iter().skip(1) {
        if row.len() != width {
            return Err(format!(
                "CSV row has {} fields but header has {width}",
                row.len()
            ));
        }
        for (index, value) in row.iter().enumerate() {
            columns[index].push(value.clone());
        }
    }
    let mut builder = SchemaBuilder::new();
    let (_name, index) = builder.begin_record(root_name);
    let fields = headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            let (ty, optional) = csv_type(&columns[index]);
            FieldSeed {
                wire_name: header.clone(),
                ty,
                optional,
                note: None,
            }
        })
        .collect();
    builder.set_fields(index, fields);
    Ok(builder)
}

fn strip_sql_comments(input: &str) -> Result<String, String> {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut index = 0usize;
    let mut quote = None;
    while index < chars.len() {
        let ch = chars[index];
        if let Some(delimiter) = quote {
            out.push(ch);
            if ch == delimiter {
                if chars.get(index + 1) == Some(&delimiter) {
                    out.push(delimiter);
                    index += 2;
                    continue;
                }
                quote = None;
            }
            index += 1;
            continue;
        }
        if ch == '\'' || ch == '"' || ch == char::from(96) {
            quote = Some(ch);
            out.push(ch);
            index += 1;
        } else if ch == '-' && chars.get(index + 1) == Some(&'-') {
            index += 2;
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
        } else if ch == '/' && chars.get(index + 1) == Some(&'*') {
            index += 2;
            let start = index;
            while index + 1 < chars.len()
                && !(chars[index] == '*' && chars[index + 1] == '/')
            {
                index += 1;
            }
            if index + 1 >= chars.len() {
                return Err("SQL has an unterminated block comment".to_string());
            }
            out.push_str(&" ".repeat(index.saturating_sub(start).max(1)));
            index += 2;
        } else {
            out.push(ch);
            index += 1;
        }
    }
    if quote.is_some() {
        return Err("SQL has an unterminated quoted value".to_string());
    }
    Ok(out)
}

fn split_sql_statements(input: &str) -> Result<Vec<String>, String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut quote = None;
    let chars: Vec<char> = input.chars().collect();
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if let Some(delimiter) = quote {
            current.push(ch);
            if ch == delimiter {
                if chars.get(index + 1) == Some(&delimiter) {
                    current.push(delimiter);
                    index += 2;
                    continue;
                }
                quote = None;
            }
            index += 1;
            continue;
        }
        if ch == '\'' || ch == '"' || ch == char::from(96) {
            quote = Some(ch);
            current.push(ch);
        } else {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth < 0 {
                        return Err("SQL has an unmatched close parenthesis".to_string());
                    }
                }
                ';' if depth == 0 => {
                    if !current.trim().is_empty() {
                        statements.push(std::mem::take(&mut current));
                    }
                    index += 1;
                    continue;
                }
                _ => {}
            }
            current.push(ch);
        }
        index += 1;
    }
    if quote.is_some() {
        return Err("SQL has an unterminated quoted value".to_string());
    }
    if depth != 0 {
        return Err("SQL has unbalanced parentheses".to_string());
    }
    if !current.trim().is_empty() {
        statements.push(current);
    }
    Ok(statements)
}

fn sql_tokens(statement: &str) -> Result<Vec<String>, String> {
    let chars: Vec<char> = statement.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if ch.is_ascii_whitespace() {
            index += 1;
        } else if "(),.<>*%+=|&!~^:[]".contains(ch) {
            tokens.push(ch.to_string());
            index += 1;
        } else if ch == '\'' || ch == '"' || ch == char::from(96) {
            let delimiter = ch;
            index += 1;
            let mut value = String::new();
            let mut closed = false;
            while index < chars.len() {
                if chars[index] == delimiter {
                    if chars.get(index + 1) == Some(&delimiter) {
                        value.push(delimiter);
                        index += 2;
                    } else {
                        index += 1;
                        closed = true;
                        break;
                    }
                } else {
                    value.push(chars[index]);
                    index += 1;
                }
            }
            if !closed {
                return Err("SQL has an unterminated quoted token".to_string());
            }
            tokens.push(value);
        } else if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
            let start = index;
            index += 1;
            while index < chars.len()
                && (chars[index].is_ascii_alphanumeric()
                    || chars[index] == '_'
                    || chars[index] == '$')
            {
                index += 1;
            }
            tokens.push(chars[start..index].iter().collect());
        } else if ch == '+' || ch == '-' {
            tokens.push(ch.to_string());
            index += 1;
        } else {
            return Err(format!("SQL has unsupported character {ch}"));
        }
    }
    Ok(tokens)
}

fn sql_type(tokens: &[String]) -> Option<&'static str> {
    let first = tokens.first()?.to_ascii_uppercase();
    match first.as_str() {
        "INT" | "INTEGER" | "BIGINT" | "SMALLINT" | "TINYINT" | "MEDIUMINT" | "SERIAL"
        | "BIGSERIAL" => Some("Int"),
        "REAL" | "FLOAT" | "DOUBLE" => Some("Float"),
        "DECIMAL" | "NUMERIC" | "MONEY" => Some("Decimal"),
        "BOOL" | "BOOLEAN" => Some("Bool"),
        "TEXT" | "TINYTEXT" | "MEDIUMTEXT" | "LONGTEXT" | "VARCHAR" | "NVARCHAR"
        | "CHAR" | "CHARACTER" | "CLOB" => Some("String"),
        "DATE" => Some("Date"),
        "TIME" => Some("LocalTime"),
        "TIMESTAMP" | "DATETIME" => Some("DateTime"),
        "BLOB" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" | "BYTEA" | "BINARY"
        | "VARBINARY" => Some("Bytes"),
        _ => None,
    }
}

fn sql_constraint_start(token: &str) -> bool {
    matches!(
        token.to_ascii_uppercase().as_str(),
        "NOT"
            | "NULL"
            | "PRIMARY"
            | "UNIQUE"
            | "REFERENCES"
            | "CHECK"
            | "DEFAULT"
            | "COLLATE"
            | "CONSTRAINT"
            | "GENERATED"
            | "AUTO_INCREMENT"
            | "AUTOINCREMENT"
    )
}

fn parse_sql_table(statement: &str) -> Result<(String, Vec<FieldSeed>), String> {
    let tokens = sql_tokens(statement)?;
    let mut index = 0usize;
    let word = |at: usize| tokens.get(at).map(String::as_str);
    if !word(index).is_some_and(|value| value.eq_ignore_ascii_case("CREATE")) {
        return Err("SQL input contains a non-CREATE TABLE statement".to_string());
    }
    index += 1;
    if word(index).is_some_and(|value| value.eq_ignore_ascii_case("TEMP"))
        || word(index).is_some_and(|value| value.eq_ignore_ascii_case("TEMPORARY"))
    {
        index += 1;
    }
    if !word(index).is_some_and(|value| value.eq_ignore_ascii_case("TABLE")) {
        return Err("SQL statement is not CREATE TABLE".to_string());
    }
    index += 1;
    if word(index).is_some_and(|value| value.eq_ignore_ascii_case("IF")) {
        if !word(index + 1).is_some_and(|value| value.eq_ignore_ascii_case("NOT"))
            || !word(index + 2).is_some_and(|value| value.eq_ignore_ascii_case("EXISTS"))
        {
            return Err("SQL has incomplete IF NOT EXISTS".to_string());
        }
        index += 3;
    }
    let first_name = tokens
        .get(index)
        .filter(|value| !value.is_empty() && value.as_str() != "(")
        .cloned()
        .ok_or_else(|| "SQL table has no name".to_string())?;
    index += 1;
    let table_name = if word(index) == Some(".") {
        index += 1;
        let name = tokens
            .get(index)
            .cloned()
            .ok_or_else(|| "SQL qualified table has no final name".to_string())?;
        index += 1;
        name
    } else {
        first_name
    };
    if word(index) != Some("(") {
        return Err("SQL table declaration needs an open parenthesis".to_string());
    }
    let open = index;
    let mut depth = 0i32;
    let mut close = None;
    while index < tokens.len() {
        match word(index) {
            Some("(") => depth += 1,
            Some(")") => {
                depth -= 1;
                if depth == 0 {
                    close = Some(index);
                    break;
                }
                if depth < 0 {
                    return Err("SQL table has an unmatched close parenthesis".to_string());
                }
            }
            _ => {}
        }
        index += 1;
    }
    let close = close.ok_or_else(|| "SQL table declaration is missing a close parenthesis".to_string())?;
    if tokens.get(close + 1).is_some() {
        return Err("SQL has tokens after the table declaration".to_string());
    }
    let mut segments: Vec<&[String]> = Vec::new();
    let mut start = open + 1;
    depth = 0;
    for position in (open + 1)..close {
        match word(position) {
            Some("(") => depth += 1,
            Some(")") => depth -= 1,
            Some(",") if depth == 0 => {
                if position == start {
                    return Err("SQL table has an empty column declaration".to_string());
                }
                segments.push(&tokens[start..position]);
                start = position + 1;
            }
            _ => {}
        }
    }
    if start == close {
        return Err("SQL table has an empty column declaration".to_string());
    }
    segments.push(&tokens[start..close]);
    let mut names = BTreeSet::new();
    let mut fields = Vec::new();
    for segment in segments {
        let first = segment.first().map(String::as_str).unwrap_or("");
        if matches!(
            first.to_ascii_uppercase().as_str(),
            "PRIMARY" | "UNIQUE" | "FOREIGN" | "CHECK" | "CONSTRAINT"
        ) {
            continue;
        }
        if segment.len() < 2 {
            return Err("SQL column needs a name and type".to_string());
        }
        let column = segment[0].clone();
        if !names.insert(column.clone()) {
            return Err(format!("SQL has duplicate column {column}"));
        }
        let mut type_end = segment.len();
        for (position, token) in segment.iter().enumerate().skip(1) {
            if sql_constraint_start(token) {
                type_end = position;
                break;
            }
        }
        if type_end == 1 {
            return Err(format!("SQL column {column} has no type"));
        }
        let ty = sql_type(&segment[1..type_end])
            .ok_or_else(|| format!("SQL type for {column} is unsupported"))?;
        let constraint_text = segment[type_end..]
            .iter()
            .map(|token| token.to_ascii_uppercase())
            .collect::<Vec<_>>();
        let not_null = constraint_text
            .windows(2)
            .any(|pair| pair[0] == "NOT" && pair[1] == "NULL")
            || constraint_text.iter().any(|token| token == "PRIMARY");
        fields.push(FieldSeed {
            wire_name: column,
            ty: BoundType::Scalar(ty),
            optional: !not_null,
            note: None,
        });
    }
    if fields.is_empty() {
        return Err(format!("SQL table {table_name} has no columns"));
    }
    Ok((table_name, fields))
}

fn bind_sql(input: &str, root_name: Option<&str>) -> Result<SchemaBuilder, String> {
    let cleaned = strip_sql_comments(input)?;
    let statements = split_sql_statements(&cleaned)?;
    if statements.is_empty() {
        return Err("SQL input has no CREATE TABLE declaration".to_string());
    }
    let mut tables = Vec::new();
    let mut table_names = BTreeSet::new();
    for statement in statements {
        let (name, fields) = parse_sql_table(&statement)?;
        if !table_names.insert(name.clone()) {
            return Err(format!("SQL has duplicate table {name}"));
        }
        tables.push((name, fields));
    }
    let single_table = tables.len() == 1;
    let mut builder = SchemaBuilder::new();
    for (table, fields) in tables {
        let preferred = if root_name.is_some() && single_table {
            root_name.unwrap_or(&table)
        } else {
            &table
        };
        let (_name, index) = builder.begin_record(preferred);
        builder.set_fields(index, fields);
    }
    Ok(builder)
}

#[derive(Clone, Debug)]
struct XmlNode {
    name: String,
    attrs: Vec<(String, String)>,
    text: String,
    children: Vec<XmlNode>,
}

struct XmlParser {
    chars: Vec<char>,
    pos: usize,
}

impl XmlParser {
    fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    fn starts_with(&self, text: &str) -> bool {
        text.chars()
            .enumerate()
            .all(|(offset, ch)| self.chars.get(self.pos + offset) == Some(&ch))
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(|ch| ch.is_ascii_whitespace()) {
            self.pos += 1;
        }
    }

    fn read_name(&mut self) -> Result<String, String> {
        let valid_start = |ch: char| ch == '_' || ch == ':' || ch.is_ascii_alphabetic() || ch.is_alphabetic();
        let valid_rest = |ch: char| {
            valid_start(ch) || ch == '-' || ch == '.' || ch.is_ascii_digit()
        };
        let Some(first) = self.peek() else {
            return Err("XML name is missing".to_string());
        };
        if !valid_start(first) {
            return Err(format!("XML has invalid name start {first}"));
        }
        let start = self.pos;
        self.pos += 1;
        while self.peek().is_some_and(valid_rest) {
            self.pos += 1;
        }
        Ok(self.chars[start..self.pos].iter().collect())
    }

    fn attach(
        stack: &mut Vec<XmlNode>,
        root: &mut Option<XmlNode>,
        node: XmlNode,
    ) -> Result<(), String> {
        if let Some(parent) = stack.last_mut() {
            parent.children.push(node);
        } else if root.is_some() {
            return Err("XML has more than one root element".to_string());
        } else {
            *root = Some(node);
        }
        Ok(())
    }

    fn parse(mut self) -> Result<XmlNode, String> {
        if self.peek() == Some('\u{feff}') {
            self.pos += 1;
        }
        let mut stack: Vec<XmlNode> = Vec::new();
        let mut root = None;
        while self.pos < self.chars.len() {
            if self.peek() != Some('<') {
                let start = self.pos;
                while self.peek().is_some_and(|ch| ch != '<') {
                    self.pos += 1;
                }
                let text: String = self.chars[start..self.pos].iter().collect();
                if text.contains("]]>") {
                    return Err("XML text contains a close CDATA marker".to_string());
                }
                validate_xml_entities(&text)?;
                if let Some(parent) = stack.last_mut() {
                    parent.text.push_str(&text);
                } else if !text.trim().is_empty() {
                    return Err("XML has text outside its root element".to_string());
                }
                continue;
            }
            if self.starts_with("<!--") {
                self.pos += 4;
                let start = self.pos;
                while self.pos < self.chars.len() && !self.starts_with("-->") {
                    self.pos += 1;
                }
                if self.pos >= self.chars.len() {
                    return Err("XML comment is not closed".to_string());
                }
                let body: String = self.chars[start..self.pos].iter().collect();
                if body.contains("--") {
                    return Err("XML comment contains a double hyphen".to_string());
                }
                self.pos += 3;
                continue;
            }
            if self.starts_with("<![CDATA[") {
                self.pos += 9;
                let start = self.pos;
                while self.pos < self.chars.len() && !self.starts_with("]]>") {
                    self.pos += 1;
                }
                if self.pos >= self.chars.len() {
                    return Err("XML CDATA section is not closed".to_string());
                }
                let body: String = self.chars[start..self.pos].iter().collect();
                if let Some(parent) = stack.last_mut() {
                    parent.text.push_str(&body);
                } else if !body.trim().is_empty() {
                    return Err("XML CDATA appears outside its root element".to_string());
                }
                self.pos += 3;
                continue;
            }
            if self.starts_with("<?") {
                self.pos += 2;
                while self.pos < self.chars.len() && !self.starts_with("?>") {
                    self.pos += 1;
                }
                if self.pos >= self.chars.len() {
                    return Err("XML processing instruction is not closed".to_string());
                }
                self.pos += 2;
                continue;
            }
            if self.starts_with("</") {
                self.pos += 2;
                let name = self.read_name()?;
                self.skip_space();
                if self.peek() != Some('>') {
                    return Err("XML close tag has trailing content".to_string());
                }
                self.pos += 1;
                let node = stack
                    .pop()
                    .ok_or_else(|| "XML has an unmatched close tag".to_string())?;
                if node.name != name {
                    return Err(format!(
                        "XML close tag {name} does not match {}",
                        node.name
                    ));
                }
                Self::attach(&mut stack, &mut root, node)?;
                continue;
            }
            if self.starts_with("<!") {
                return Err("XML declaration is unsupported here".to_string());
            }
            self.pos += 1;
            let name = self.read_name()?;
            let mut attrs = Vec::new();
            let mut attr_names = BTreeSet::new();
            let self_closing;
            loop {
                self.skip_space();
                if self.starts_with("/>") {
                    self.pos += 2;
                    self_closing = true;
                    break;
                }
                if self.peek() == Some('>') {
                    self.pos += 1;
                    self_closing = false;
                    break;
                }
                let attr_name = self.read_name()?;
                if !attr_names.insert(attr_name.clone()) {
                    return Err(format!("XML has duplicate attribute {attr_name}"));
                }
                self.skip_space();
                if self.peek() != Some('=') {
                    return Err(format!("XML attribute {attr_name} has no equals sign"));
                }
                self.pos += 1;
                self.skip_space();
                let quote = self
                    .peek()
                    .filter(|ch| *ch == '"' || *ch == '\'')
                    .ok_or_else(|| format!("XML attribute {attr_name} is not quoted"))?;
                self.pos += 1;
                let start = self.pos;
                while self.pos < self.chars.len() && self.peek() != Some(quote) {
                    self.pos += 1;
                }
                if self.pos >= self.chars.len() {
                    return Err(format!("XML attribute {attr_name} is not closed"));
                }
                let value: String = self.chars[start..self.pos].iter().collect();
                validate_xml_entities(&value)?;
                self.pos += 1;
                attrs.push((attr_name, value));
            }
            let node = XmlNode {
                name,
                attrs,
                text: String::new(),
                children: Vec::new(),
            };
            if self_closing {
                Self::attach(&mut stack, &mut root, node)?;
            } else {
                stack.push(node);
            }
        }
        if !stack.is_empty() {
            return Err(format!("XML element {} is not closed", stack[0].name));
        }
        root.ok_or_else(|| "XML has no root element".to_string())
    }
}

fn validate_xml_entities(text: &str) -> Result<(), String> {
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] != '&' {
            index += 1;
            continue;
        }
        let start = index;
        index += 1;
        while index < chars.len() && chars[index] != ';' {
            index += 1;
        }
        if index >= chars.len() {
            return Err("XML entity is not terminated".to_string());
        }
        let entity: String = chars[start + 1..index].iter().collect();
        let valid = matches!(entity.as_str(), "amp" | "lt" | "gt" | "quot" | "apos")
            || entity
                .strip_prefix("#x")
                .is_some_and(|digits| !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_hexdigit()))
            || entity
                .strip_prefix('#')
                .is_some_and(|digits| !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit()));
        if !valid {
            return Err(format!("XML entity {entity} is invalid"));
        }
        index += 1;
    }
    Ok(())
}

fn add_xml_record(
    builder: &mut SchemaBuilder,
    preferred: &str,
    nodes: &[&XmlNode],
) -> String {
    let (name, index) = builder.begin_record(preferred);
    let mut attr_names = BTreeSet::new();
    let mut child_names = BTreeSet::new();
    for node in nodes {
        attr_names.extend(node.attrs.iter().map(|(attr, _)| attr.clone()));
        child_names.extend(node.children.iter().map(|child| child.name.clone()));
    }
    let mut fields = Vec::new();
    for attr in attr_names {
        let present = nodes
            .iter()
            .filter(|node| node.attrs.iter().any(|(name, _)| name == &attr))
            .count();
        fields.push(FieldSeed {
            wire_name: format!("@{attr}"),
            ty: BoundType::Scalar("String"),
            optional: present != nodes.len(),
            note: None,
        });
    }
    let text_present = nodes.iter().any(|node| !node.text.trim().is_empty());
    if text_present {
        fields.push(FieldSeed {
            wire_name: "$text".to_string(),
            ty: BoundType::Scalar("String"),
            optional: nodes.iter().any(|node| node.text.trim().is_empty()),
            note: None,
        });
    }
    for child_name in child_names {
        let mut children = Vec::new();
        let mut present_count = 0usize;
        let mut repeated = false;
        for node in nodes {
            let matching: Vec<&XmlNode> = node
                .children
                .iter()
                .filter(|child| child.name == child_name)
                .collect();
            if !matching.is_empty() {
                present_count += 1;
            }
            if matching.len() > 1 {
                repeated = true;
            }
            children.extend(matching);
        }
        let child_hint = format!("{}{}", name, pascal_name(&child_name));
        let child_type = add_xml_record(builder, &child_hint, &children);
        let ty = if repeated {
            BoundType::Array(Box::new(BoundType::Named(child_type)))
        } else {
            BoundType::Named(child_type)
        };
        fields.push(FieldSeed {
            wire_name: child_name,
            ty,
            optional: present_count != nodes.len(),
            note: None,
        });
    }
    builder.set_fields(index, fields);
    name
}

fn bind_xml(input: &str, root_name: Option<&str>) -> Result<SchemaBuilder, String> {
    let root = XmlParser::new(input).parse()?;
    let preferred = root_name
        .map(str::to_string)
        .unwrap_or_else(|| pascal_name(&root.name));
    let mut builder = SchemaBuilder::new();
    add_xml_record(&mut builder, &preferred, &[&root]);
    Ok(builder)
}

#[derive(Clone, Debug)]
enum ProtoToken {
    Word(String),
    Number(String),
    StringLiteral,
    Symbol(char),
}

fn lex_proto(input: &str) -> Result<Vec<ProtoToken>, String> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if ch.is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if ch == '/' && chars.get(index + 1) == Some(&'/') {
            index += 2;
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            continue;
        }
        if ch == '/' && chars.get(index + 1) == Some(&'*') {
            index += 2;
            while index + 1 < chars.len()
                && !(chars[index] == '*' && chars[index + 1] == '/')
            {
                index += 1;
            }
            if index + 1 >= chars.len() {
                return Err("proto block comment is not closed".to_string());
            }
            index += 2;
            continue;
        }
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = index;
            index += 1;
            while index < chars.len()
                && (chars[index].is_ascii_alphanumeric() || chars[index] == '_')
            {
                index += 1;
            }
            tokens.push(ProtoToken::Word(chars[start..index].iter().collect()));
            continue;
        }
        if ch.is_ascii_digit() {
            let start = index;
            index += 1;
            while index < chars.len() && chars[index].is_ascii_digit() {
                index += 1;
            }
            tokens.push(ProtoToken::Number(chars[start..index].iter().collect()));
            continue;
        }
        if ch == '"' {
            index += 1;
            let mut closed = false;
            while index < chars.len() {
                match chars[index] {
                    '"' => {
                        index += 1;
                        closed = true;
                        break;
                    }
                    '\\' => {
                        index += 2;
                    }
                    '\n' | '\r' => return Err("proto string contains a newline".to_string()),
                    _ => index += 1,
                }
            }
            if !closed {
                return Err("proto string is not closed".to_string());
            }
            tokens.push(ProtoToken::StringLiteral);
            continue;
        }
        if "{}[]()=;,.<>:+-".contains(ch) {
            tokens.push(ProtoToken::Symbol(ch));
            index += 1;
            continue;
        }
        return Err(format!("proto has unsupported character {ch}"));
    }
    Ok(tokens)
}

struct ProtoField {
    wire_name: String,
    type_name: String,
    repeated: bool,
    optional: bool,
    number: u32,
}

struct ProtoMessage {
    source_name: String,
    short_name: String,
    fields: Vec<ProtoField>,
}

struct ProtoParser {
    tokens: Vec<ProtoToken>,
    pos: usize,
    messages: Vec<ProtoMessage>,
    message_names: BTreeSet<String>,
}

impl ProtoParser {
    fn new(input: &str) -> Result<Self, String> {
        Ok(Self {
            tokens: lex_proto(input)?,
            pos: 0,
            messages: Vec::new(),
            message_names: BTreeSet::new(),
        })
    }

    fn word_at(&self, offset: usize) -> Option<&str> {
        match self.tokens.get(self.pos + offset) {
            Some(ProtoToken::Word(word)) => Some(word),
            _ => None,
        }
    }

    fn symbol_at(&self, offset: usize, expected: char) -> bool {
        matches!(
            self.tokens.get(self.pos + offset),
            Some(ProtoToken::Symbol(symbol)) if *symbol == expected
        )
    }

    fn take_word(&mut self, what: &str) -> Result<String, String> {
        match self.tokens.get(self.pos).cloned() {
            Some(ProtoToken::Word(word)) => {
                self.pos += 1;
                Ok(word)
            }
            _ => Err(format!("proto expected {what}")),
        }
    }

    fn expect_symbol(&mut self, expected: char, what: &str) -> Result<(), String> {
        if self.symbol_at(0, expected) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("proto expected {what}"))
        }
    }

    fn skip_statement(&mut self) -> Result<(), String> {
        let mut square = 0i32;
        let mut paren = 0i32;
        while self.pos < self.tokens.len() {
            match self.tokens.get(self.pos) {
                Some(ProtoToken::Symbol('[')) => square += 1,
                Some(ProtoToken::Symbol(']')) => {
                    square -= 1;
                    if square < 0 {
                        return Err("proto has an unmatched close bracket".to_string());
                    }
                }
                Some(ProtoToken::Symbol('(')) => paren += 1,
                Some(ProtoToken::Symbol(')')) => {
                    paren -= 1;
                    if paren < 0 {
                        return Err("proto has an unmatched close parenthesis".to_string());
                    }
                }
                Some(ProtoToken::Symbol(';')) if square == 0 && paren == 0 => {
                    self.pos += 1;
                    return Ok(());
                }
                _ => {}
            }
            self.pos += 1;
        }
        Err("proto statement is missing a semicolon".to_string())
    }

    fn skip_named_block(&mut self, kind: &str) -> Result<(), String> {
        let _ = self.take_word(kind)?;
        let _ = self.take_word("name")?;
        self.expect_symbol('{', "an open block")?;
        let mut depth = 1i32;
        while self.pos < self.tokens.len() {
            match self.tokens.get(self.pos) {
                Some(ProtoToken::Symbol('{')) => depth += 1,
                Some(ProtoToken::Symbol('}')) => {
                    depth -= 1;
                    if depth == 0 {
                        self.pos += 1;
                        return Ok(());
                    }
                }
                _ => {}
            }
            self.pos += 1;
        }
        Err(format!("proto {kind} block is not closed"))
    }

    fn parse_type_name(&mut self) -> Result<String, String> {
        let mut name = String::new();
        if self.symbol_at(0, '.') {
            name.push('.');
            self.pos += 1;
        }
        name.push_str(&self.take_word("a field type")?);
        while self.symbol_at(0, '.') {
            self.pos += 1;
            name.push('.');
            name.push_str(&self.take_word("a type segment")?);
        }
        Ok(name)
    }

    fn skip_field_options(&mut self) -> Result<(), String> {
        self.expect_symbol('[', "field options")?;
        let mut depth = 1i32;
        while self.pos < self.tokens.len() {
            match self.tokens.get(self.pos) {
                Some(ProtoToken::Symbol('[')) => depth += 1,
                Some(ProtoToken::Symbol(']')) => {
                    depth -= 1;
                    if depth == 0 {
                        self.pos += 1;
                        return Ok(());
                    }
                }
                _ => {}
            }
            self.pos += 1;
        }
        Err("proto field options are not closed".to_string())
    }

    fn parse_field(&mut self) -> Result<ProtoField, String> {
        let label = self.word_at(0).map(str::to_ascii_lowercase);
        let (repeated, optional) = match label.as_deref() {
            Some("repeated") => {
                self.pos += 1;
                (true, false)
            }
            Some("optional") => {
                self.pos += 1;
                (false, true)
            }
            Some("required") => {
                self.pos += 1;
                (false, false)
            }
            _ => (false, false),
        };
        let type_name = self.parse_type_name()?;
        if self.word_at(0).is_none() {
            return Err("proto field has no name".to_string());
        }
        let wire_name = self.take_word("a field name")?;
        self.expect_symbol('=', "an equals sign")?;
        let number = match self.tokens.get(self.pos).cloned() {
            Some(ProtoToken::Number(number)) => {
                self.pos += 1;
                number
                    .parse::<u32>()
                    .map_err(|_| "proto field number is invalid".to_string())?
            }
            _ => return Err("proto field number is missing".to_string()),
        };
        if number == 0 || number > 536_870_911 {
            return Err(format!("proto field number {number} is out of range"));
        }
        if self.symbol_at(0, '[') {
            self.skip_field_options()?;
        }
        self.expect_symbol(';', "a field semicolon")?;
        Ok(ProtoField {
            wire_name,
            type_name,
            repeated,
            optional,
            number,
        })
    }

    fn parse_message(&mut self, parent: Option<&str>) -> Result<(), String> {
        let _ = self.take_word("message")?;
        let short_name = self.take_word("a message name")?;
        let source_name = parent
            .map(|parent| format!("{parent}.{short_name}"))
            .unwrap_or_else(|| short_name.clone());
        if !self.message_names.insert(source_name.clone()) {
            return Err(format!("proto has duplicate message {source_name}"));
        }
        self.expect_symbol('{', "a message block")?;
        let mut fields = Vec::new();
        let mut field_names = BTreeSet::new();
        let mut field_numbers = BTreeSet::new();
        while self.pos < self.tokens.len() {
            if self.symbol_at(0, '}') {
                self.pos += 1;
                self.messages.push(ProtoMessage {
                    source_name,
                    short_name,
                    fields,
                });
                return Ok(());
            }
            if self.word_at(0) == Some("message") {
                self.parse_message(Some(&source_name))?;
                continue;
            }
            if self.word_at(0) == Some("enum") {
                self.skip_named_block("enum")?;
                continue;
            }
            if matches!(
                self.word_at(0),
                Some("reserved") | Some("extensions") | Some("option")
            ) {
                self.skip_statement()?;
                continue;
            }
            if self.symbol_at(0, ';') {
                self.pos += 1;
                continue;
            }
            let field = self.parse_field()?;
            if !field_names.insert(field.wire_name.clone()) {
                return Err(format!("proto has duplicate field {}", field.wire_name));
            }
            if !field_numbers.insert(field.number) {
                return Err(format!("proto has duplicate field number {}", field.number));
            }
            fields.push(field);
        }
        Err(format!("proto message {source_name} is not closed"))
    }

    fn parse(mut self) -> Result<Vec<ProtoMessage>, String> {
        while self.pos < self.tokens.len() {
            if self.word_at(0) == Some("message") {
                self.parse_message(None)?;
            } else if matches!(
                self.word_at(0),
                Some("syntax") | Some("package") | Some("import") | Some("option")
            ) {
                self.skip_statement()?;
            } else if self.word_at(0) == Some("enum") {
                self.skip_named_block("enum")?;
            } else if self.symbol_at(0, ';') {
                self.pos += 1;
            } else {
                return Err(format!(
                    "proto has unsupported top-level declaration {}",
                    self.word_at(0).unwrap_or("unknown")
                ));
            }
        }
        if self.messages.is_empty() {
            return Err("proto input has no message block".to_string());
        }
        Ok(self.messages)
    }
}

fn proto_scalar(type_name: &str) -> Option<&'static str> {
    match type_name.trim_start_matches('.').to_ascii_lowercase().as_str() {
        "double" | "float" => Some("Float"),
        "int32" | "sint32" | "sfixed32" | "int64" | "sint64" | "sfixed64" | "uint32"
        | "uint64" | "fixed32" | "fixed64" => Some("Int"),
        "bool" => Some("Bool"),
        "string" => Some("String"),
        "bytes" => Some("Bytes"),
        "google.protobuf.timestamp" => Some("DateTime"),
        "google.protobuf.duration" => Some("Duration"),
        "google.protobuf.stringvalue" => Some("String"),
        "google.protobuf.bytesvalue" => Some("Bytes"),
        "google.protobuf.int32value" | "google.protobuf.int64value" => Some("Int"),
        "google.protobuf.boolvalue" => Some("Bool"),
        "google.protobuf.doublevalue" | "google.protobuf.floatvalue" => Some("Float"),
        _ => None,
    }
}

fn bind_proto(input: &str, root_name: Option<&str>) -> Result<SchemaBuilder, String> {
    let messages = ProtoParser::new(input)?.parse()?;
    let mut builder = SchemaBuilder::new();
    let use_root = messages.len() == 1;
    let mut source_names = BTreeMap::new();
    let mut leaf_names = BTreeMap::new();
    let mut ambiguous_leaves = BTreeSet::new();
    for (index, message) in messages.iter().enumerate() {
        let preferred = if use_root && index == 0 {
            root_name.unwrap_or(&message.short_name)
        } else {
            &message.short_name
        };
        let (name, record_index) = builder.begin_record(preferred);
        source_names.insert(message.source_name.clone(), name.clone());
        let leaf = message
            .source_name
            .rsplit('.')
            .next()
            .unwrap_or(&message.source_name)
            .to_string();
        if leaf_names.insert(leaf.clone(), name).is_some() {
            ambiguous_leaves.insert(leaf);
        }
        let _ = record_index;
    }
    for (index, message) in messages.iter().enumerate() {
        let mut fields = Vec::new();
        for field in &message.fields {
            let base = if let Some(scalar) = proto_scalar(&field.type_name) {
                BoundType::Scalar(scalar)
            } else {
                let reference = field.type_name.trim_start_matches('.');
                let leaf = reference.rsplit('.').next().unwrap_or(reference);
                let resolved = source_names
                    .get(reference)
                    .or_else(|| {
                        if ambiguous_leaves.contains(leaf) {
                            None
                        } else {
                            leaf_names.get(leaf)
                        }
                    })
                    .ok_or_else(|| {
                        format!(
                            "proto field {} references unknown message type {}",
                            field.wire_name, field.type_name
                        )
                    })?;
                BoundType::Named(resolved.clone())
            };
            let ty = if field.repeated {
                BoundType::Array(Box::new(base))
            } else {
                base
            };
            fields.push(FieldSeed {
                wire_name: field.wire_name.clone(),
                ty,
                optional: field.optional,
                note: Some(format!("proto field number: {}", field.number)),
            });
        }
        builder.set_fields(index, fields);
    }
    Ok(builder)
}

fn inference_rules(format: &str) -> &'static [&'static str] {
    match format {
        "json" => &[
            "JSON object fields become #Codable fields; nested objects become records.",
            "JSON arrays keep inferred element types; null and missing values add optionality.",
        ],
        "csv" => &[
            "CSV headers become wire keys; every data row participates in scalar inference.",
            "Empty cells become optional fields; quoted fields keep commas, quotes, and newlines.",
        ],
        "sql" => &[
            "SQL declared column types map to Jet scalar and core value types.",
            "NOT NULL and PRIMARY KEY columns stay required; table constraints are ignored.",
        ],
        "xml" => &[
            "XML attributes use @ wire keys; element text uses the $text wire key.",
            "Nested elements become records; repeated child elements become arrays.",
        ],
        "proto" => &[
            "Protobuf message blocks become #Codable records; scalar types use Jet core types.",
            "repeated and optional labels map to lists and optional fields; numbers stay in comments.",
        ],
        _ => &["The input was parsed as a named data schema."],
    }
}

fn render_data_source(
    format: &str,
    input_path: &str,
    input: &str,
    command: &str,
    builder: &SchemaBuilder,
) -> String {
    let hash = crate::SHA256::sha256_hex(input.as_bytes());
    let mut source = String::new();
    let command = if command.is_empty() {
        format!("jet bind {format} {input_path}")
    } else {
        command.to_string()
    };
    let _ = writeln!(source, "// generated by: {command}");
    let _ = writeln!(source, "// input: {input_path}");
    let _ = writeln!(source, "// sha256: {hash}");
    let _ = writeln!(source, "// format: {format}");
    for rule in inference_rules(format) {
        let _ = writeln!(source, "// inference: {rule}");
    }
    source.push('\n');
    for record in &builder.records {
        source.push_str("#Codable\n");
        let _ = writeln!(source, "struct {} {{", record.name);
        for field in &record.fields {
            if let Some(note) = &field.note {
                let _ = writeln!(source, "    // {note}");
            }
            if field.wire_name != field.name {
                let _ = writeln!(
                    source,
                    "    #Rename({}) {}: {}{}",
                    crate::JSON::quote(&field.wire_name),
                    field.name,
                    field.ty.render(),
                    if field.optional { "?" } else { "" }
                );
            } else {
                let _ = writeln!(
                    source,
                    "    {}: {}{}",
                    field.name,
                    field.ty.render(),
                    if field.optional { "?" } else { "" }
                );
            }
        }
        source.push_str("}\n\n");
    }
    source
}

/// Parse one supported data schema and render visible ordinary Jet source.
///
/// command is recorded verbatim in the stable provenance header. Callers
/// should pass the exact user-facing command line, without shell quoting.
pub fn generate_data(
    format: &str,
    input_path: &str,
    input: &str,
    root_name: Option<&str>,
    command: &str,
) -> Result<DataBindResult, String> {
    let format = format.to_ascii_lowercase();
    let default_name = root_name
        .map(str::to_string)
        .unwrap_or_else(|| input_stem_type_name(input_path));
    let sanitized_root = root_name.map(sanitize_type_name);
    let builder = match format.as_str() {
        "json" => bind_json(input, &sanitize_type_name(&default_name))?,
        "csv" => bind_csv(input, &sanitize_type_name(&default_name))?,
        "sql" => bind_sql(input, sanitized_root.as_deref())?,
        "xml" => bind_xml(input, sanitized_root.as_deref())?,
        "proto" => bind_proto(input, sanitized_root.as_deref())?,
        _ => return Err(format!("unsupported data schema format {format}")),
    };
    if builder.records.is_empty() {
        return Err("schema did not produce any records".to_string());
    }
    let record_count = builder.records.len();
    let source = render_data_source(&format, input_path, input, command, &builder);
    Ok(DataBindResult {
        source,
        record_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_prototype_names_return_existing_error_without_panic() {
        let expected = |lib: &str| format!("no bindable C function prototypes found for `{lib}`");

        assert!(parse_prototype("(int)").is_none());
        assert_eq!(
            generate("(int);", "empty-name").err(),
            Some(expected("empty-name"))
        );

        assert!(parse_prototype("éfoo(int)").is_none());
        assert_eq!(
            generate("éfoo(int);", "unicode-name").err(),
            Some(expected("unicode-name"))
        );

        let valid = generate("int foo(int value);", "valid").unwrap();
        assert_eq!(valid.bound, vec!["foo"]);
        assert!(valid
            .source
            .contains("fn foo(value: Int) => Int = \"foo\";"));

        let leading_underscore = generate("int _foo(int value);", "underscore").unwrap();
        assert_eq!(leading_underscore.bound, vec!["_foo"]);
        assert!(leading_underscore
            .source
            .contains("fn _foo(value: Int) => Int = \"_foo\";"));
    }

    #[test]
    fn malformed_or_unsupported_declarators_are_rejected_without_guessing() {
        let header = r#"
            int trailing_comma(int value,);
            int trailing_junk(int value) garbage;
            int unbalanced(int (*callback)(int);
            int valid(int value);
        "#;
        let result = generate(header, "hostile").unwrap();
        assert_eq!(result.bound, vec!["valid"]);
        assert!(result.source.contains("fn valid(value: Int) => Int"));
        assert!(!result.source.contains("trailing_comma"));
        assert!(!result.source.contains("trailing_junk"));
        assert!(!result.source.contains("unbalanced"));
    }

    #[test]
    fn binds_simple_prototypes() {
        let h = r#"
            // a small lib
            int jetc_add(int a, int b);
            double scale(double x, float k);
            void reset(void);
            const char *name_of(int id);
            bool is_ready();
        "#;
        let r = generate(h, "jetc").unwrap();
        assert!(r.source.contains("#Bindgen module c.jetc.__bindgen__ {"));
        assert!(r
            .source
            .contains("fn jetc_add(a: Int, b: Int) => Int = \"jetc_add\";"));
        assert!(r
            .source
            .contains("fn scale(x: Float, k: Float) => Float = \"scale\";"));
        assert!(r.source.contains("fn reset() = \"reset\";"));
        assert!(r
            .source
            .contains("fn name_of(id: Int) => String = \"name_of\";"));
        assert!(r.source.contains("fn is_ready() => Bool = \"is_ready\";"));
        assert_eq!(r.bound.len(), 5);
        assert!(r.skipped.is_empty());
    }

    #[test]
    fn skips_unbindable_and_keeps_the_rest() {
        let h = r#"
            int ok(int x);
            struct Point { int x; int y; };
            void *raw_alloc(size_t n);
            int sum(int *items, int n);
            void log_msg(const char *fmt, ...);
        "#;
        let r = generate(h, "lib").unwrap();
        assert!(r.source.contains("fn ok(x: Int) => Int = \"ok\";"));
        // `void*` return, `int*` param, and varargs are all skipped.
        let skipped: Vec<&str> = r.skipped.iter().map(|(n, _)| n.as_str()).collect();
        assert!(skipped.contains(&"raw_alloc"));
        assert!(skipped.contains(&"sum"));
        assert!(skipped.contains(&"log_msg"));
        assert_eq!(r.bound, vec!["ok"]);
    }

    #[test]
    fn errors_when_nothing_bindable() {
        let h = "struct Opaque; typedef int Handle;";
        assert!(generate(h, "x").is_err());
    }

    #[test]
    fn unnamed_params_get_synthetic_names() {
        let r = generate("int f(int, double);", "m").unwrap();
        assert!(r
            .source
            .contains("fn f(arg0: Int, arg1: Float) => Int = \"f\";"));
    }

    // c43: U32/uint32_t boundary — C integers of all widths map to Jet `Int`.
    // uint32_t is the archetypal case: a 32-bit unsigned type that fits in Int
    // (i64) without loss. Verify the CBind layer produces a correct signature
    // and does NOT map uint32_t to a fixed-width type on the Jet surface.
    #[test]
    fn uint32_t_maps_to_int_in_cbind() {
        let h = r#"
            #include <stdint.h>
            uint32_t add_u32(uint32_t a, uint32_t b);
            int32_t sub_i32(int32_t a, int32_t b);
            uint64_t identity_u64(uint64_t x);
        "#;
        let r = generate(h, "lib").unwrap();
        // All C integer fixed-width types unify to Jet's `Int` (i64) at the FFI
        // surface; signed vs unsigned and width are transparent to Jet callers.
        assert!(
            r.source
                .contains("fn add_u32(a: Int, b: Int) => Int = \"add_u32\";"),
            "uint32_t must map to Int: got:\n{}",
            r.source
        );
        assert!(
            r.source
                .contains("fn sub_i32(a: Int, b: Int) => Int = \"sub_i32\";"),
            "int32_t must map to Int: got:\n{}",
            r.source
        );
        assert!(
            r.source
                .contains("fn identity_u64(x: Int) => Int = \"identity_u64\";"),
            "uint64_t must map to Int: got:\n{}",
            r.source
        );
        assert_eq!(r.bound, vec!["add_u32", "sub_i32", "identity_u64"]);
    }
}
