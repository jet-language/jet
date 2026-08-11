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
    let mut used_names = BTreeSet::new();

    for decl in split_declarations(&cleaned) {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        match parse_prototype(decl) {
            Some(proto) => match render_binding(&proto, &mut used_names) {
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

    let module_lib = crate::Syntax::sanitize_generated_name(
        lib,
        crate::Syntax::NameCase::Snake,
        "library",
    );
    let source = format!("#Bindgen module c.{}.__bindgen__ {{\n{}}}\n", module_lib, lines);
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
fn render_binding(p: &Proto, used_names: &mut BTreeSet<String>) -> Result<String, String> {
    let ret_jet = match map_return_type(&p.ret) {
        Ok(t) => t,
        Err(e) => return Err(e),
    };
    let function_name = unique_name(
        used_names,
        &crate::Syntax::sanitize_generated_name(
            &p.name,
            crate::Syntax::NameCase::Snake,
            "function",
        ),
    );
    let mut params = Vec::new();
    let mut used_param_names = BTreeSet::new();
    for (idx, raw) in p.params.iter().enumerate() {
        let raw = raw.trim();
        if raw == "..." {
            return Err("variadic (`...`) parameters aren't bindable".to_string());
        }
        let (ty, name) = split_param_type_and_name(raw, idx);
        let jet_ty = map_type(&ty).ok_or_else(|| format!("type `{}` isn't bindable", ty.trim()))?;
        let name = unique_name(
            &mut used_param_names,
            &crate::Syntax::sanitize_generated_name(&name, crate::Syntax::NameCase::Snake, "arg"),
        );
        params.push(format!("{}: {}", name, jet_ty));
    }
    let params_str = params.join(", ");
    let line = match ret_jet {
        Some(r) => format!(
            "fn {}({}) => {} = {};",
            function_name,
            params_str,
            r,
            crate::JSON::quote(&p.name)
        ),
        None => format!(
            "fn {}({}) = {};",
            function_name,
            params_str,
            crate::JSON::quote(&p.name)
        ),
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
    Bytes,
    Named(String),
    Array(Box<BoundType>),
    Optional(Box<BoundType>),
    Dynamic,
}

impl BoundType {
    fn render(&self) -> String {
        match self {
            Self::Scalar(name) => (*name).to_string(),
            Self::Bytes => "[U8]".to_string(),
            Self::Named(name) => name.clone(),
            Self::Array(element) => format!("[{}]", element.render()),
            Self::Optional(value) => format!("{}?", value.render()),
            Self::Dynamic => "DataTree".to_string(),
        }
    }
}

fn bound_scalar(name: &'static str) -> BoundType {
    if name == "[U8]" {
        BoundType::Bytes
    } else {
        BoundType::Scalar(name)
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

fn sanitize_field_name(wire_name: &str) -> String {
    crate::Syntax::sanitize_generated_name(wire_name, crate::Syntax::NameCase::Snake, "field")
}

fn sanitize_type_name(raw: &str) -> String {
    crate::Syntax::sanitize_generated_name(raw, crate::Syntax::NameCase::Pascal, "Schema")
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

/// Binding keeps the original numeric spelling. It distinguishes an exact
/// integer lexeme from a decimal or exponent lexeme without asking a binary
/// floating-point representation to carry the schema's precision.
#[derive(Debug)]
enum JsonBindValue {
    Null,
    Bool,
    Number { lexeme: String },
    String,
    Array(Vec<JsonBindValue>),
    Object(BTreeMap<String, JsonBindValue>),
}

struct JsonBindParser {
    chars: Vec<char>,
    pos: usize,
}

impl JsonBindParser {
    fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn starts_with(&self, text: &str) -> bool {
        text.chars()
            .enumerate()
            .all(|(offset, ch)| self.chars.get(self.pos + offset) == Some(&ch))
    }

    fn skip_space(&mut self) {
        while self
            .peek()
            .is_some_and(|ch| matches!(ch, ' ' | '\t' | '\n' | '\r'))
        {
            self.pos += 1;
        }
    }

    fn expect(&mut self, expected: char) -> Result<(), String> {
        if self.peek() == Some(expected) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected `{expected}`"))
        }
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.expect('"')?;
        let mut value = String::new();
        while let Some(ch) = self.peek() {
            self.pos += 1;
            match ch {
                '"' => return Ok(value),
                '\\' => {
                    let escape = self
                        .peek()
                        .ok_or_else(|| "JSON escape is incomplete".to_string())?;
                    self.pos += 1;
                    match escape {
                        '"' => value.push('"'),
                        '\\' => value.push('\\'),
                        '/' => value.push('/'),
                        'b' => value.push('\u{0008}'),
                        'f' => value.push('\u{000c}'),
                        'n' => value.push('\n'),
                        'r' => value.push('\r'),
                        't' => value.push('\t'),
                        'u' => {
                            let first = self.hex_quad()?;
                            if (0xD800..=0xDBFF).contains(&first) {
                                if self.peek() != Some('\\') {
                                    return Err("JSON high surrogate is not followed by a low surrogate".to_string());
                                }
                                self.pos += 1;
                                if self.peek() != Some('u') {
                                    return Err("JSON surrogate pair is malformed".to_string());
                                }
                                self.pos += 1;
                                let second = self.hex_quad()?;
                                if !(0xDC00..=0xDFFF).contains(&second) {
                                    return Err("JSON surrogate pair has an invalid low surrogate".to_string());
                                }
                                let codepoint = 0x1_0000
                                    + ((first - 0xD800) << 10)
                                    + (second - 0xDC00);
                                value.push(
                                    char::from_u32(codepoint)
                                        .ok_or_else(|| "JSON unicode escape is invalid".to_string())?,
                                );
                            } else if (0xDC00..=0xDFFF).contains(&first) {
                                return Err("JSON low surrogate has no high surrogate".to_string());
                            } else {
                                value.push(
                                    char::from_u32(first)
                                        .ok_or_else(|| "JSON unicode escape is invalid".to_string())?,
                                );
                            }
                        }
                        _ => return Err(format!("JSON has an invalid escape `\\{escape}`")),
                    }
                }
                ch if ch <= '\u{001f}' => {
                    return Err("JSON string contains an unescaped control character".to_string())
                }
                _ => value.push(ch),
            }
        }
        Err("JSON string is not closed".to_string())
    }

    fn hex_quad(&mut self) -> Result<u32, String> {
        let mut value = 0u32;
        for _ in 0..4 {
            let ch = self
                .peek()
                .ok_or_else(|| "JSON unicode escape is incomplete".to_string())?;
            self.pos += 1;
            value = value
                .checked_mul(16)
                .and_then(|value| ch.to_digit(16).and_then(|digit| value.checked_add(digit)))
                .ok_or_else(|| "JSON unicode escape is invalid".to_string())?;
        }
        Ok(value)
    }

    fn parse_number(&mut self) -> Result<JsonBindValue, String> {
        let start = self.pos;
        if self.peek() == Some('-') {
            self.pos += 1;
        }
        match self.peek() {
            Some('0') => self.pos += 1,
            Some('1'..='9') => {
                while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                    self.pos += 1;
                }
            }
            _ => return Err("JSON number has no integer digits".to_string()),
        }
        if self.peek() == Some('.') {
            self.pos += 1;
            let digits = self.pos;
            while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                self.pos += 1;
            }
            if self.pos == digits {
                return Err("JSON number has no fractional digits".to_string());
            }
        }
        if self.peek().is_some_and(|ch| ch == 'e' || ch == 'E') {
            self.pos += 1;
            if self.peek().is_some_and(|ch| ch == '+' || ch == '-') {
                self.pos += 1;
            }
            let digits = self.pos;
            while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                self.pos += 1;
            }
            if self.pos == digits {
                return Err("JSON exponent has no digits".to_string());
            }
        }
        let lexeme: String = self.chars[start..self.pos].iter().collect();
        // Schema inference uses this spelling directly. A valid JSON number
        // may exceed ordinary machine precision or exponent range; neither
        // boundary changes whether its source lexeme is integer-shaped or
        // decimal/exponent-shaped.
        Ok(JsonBindValue::Number { lexeme })
    }

    fn parse_value(&mut self) -> Result<JsonBindValue, String> {
        self.skip_space();
        if self.starts_with("null") {
            self.pos += 4;
            return Ok(JsonBindValue::Null);
        }
        if self.starts_with("true") {
            self.pos += 4;
            return Ok(JsonBindValue::Bool);
        }
        if self.starts_with("false") {
            self.pos += 5;
            return Ok(JsonBindValue::Bool);
        }
        match self.peek() {
            Some('"') => {
                self.parse_string()?;
                Ok(JsonBindValue::String)
            }
            Some('[') => self.parse_array(),
            Some('{') => self.parse_object(),
            Some('-' | '0'..='9') => self.parse_number(),
            Some(ch) => Err(format!("JSON has an unexpected value start {ch}")),
            None => Err("JSON value is missing".to_string()),
        }
    }

    fn parse_array(&mut self) -> Result<JsonBindValue, String> {
        self.expect('[')?;
        self.skip_space();
        let mut values = Vec::new();
        if self.peek() == Some(']') {
            self.pos += 1;
            return Ok(JsonBindValue::Array(values));
        }
        loop {
            values.push(self.parse_value()?);
            self.skip_space();
            match self.peek() {
                Some(',') => {
                    self.pos += 1;
                    self.skip_space();
                    if self.peek() == Some(']') {
                        return Err("JSON array has a trailing comma".to_string());
                    }
                }
                Some(']') => {
                    self.pos += 1;
                    return Ok(JsonBindValue::Array(values));
                }
                _ => return Err("JSON array needs a comma or close bracket".to_string()),
            }
        }
    }

    fn parse_object(&mut self) -> Result<JsonBindValue, String> {
        self.expect('{')?;
        self.skip_space();
        let mut object = BTreeMap::new();
        if self.peek() == Some('}') {
            self.pos += 1;
            return Ok(JsonBindValue::Object(object));
        }
        loop {
            self.skip_space();
            let key = self.parse_string()?;
            if object.contains_key(&key) {
                return Err(format!("JSON object has duplicate key {key}"));
            }
            self.skip_space();
            self.expect(':')?;
            let value = self.parse_value()?;
            object.insert(key, value);
            self.skip_space();
            match self.peek() {
                Some(',') => {
                    self.pos += 1;
                    self.skip_space();
                    if self.peek() == Some('}') {
                        return Err("JSON object has a trailing comma".to_string());
                    }
                }
                Some('}') => {
                    self.pos += 1;
                    return Ok(JsonBindValue::Object(object));
                }
                _ => return Err("JSON object needs a comma or close brace".to_string()),
            }
        }
    }

    fn parse(mut self) -> Result<JsonBindValue, String> {
        let value = self.parse_value()?;
        self.skip_space();
        if self.pos != self.chars.len() {
            return Err("JSON has trailing content".to_string());
        }
        Ok(value)
    }
}

fn json_values_type(
    builder: &mut SchemaBuilder,
    values: &[&JsonBindValue],
    hint: &str,
) -> Result<(BoundType, bool), String> {
    let nullable = values.iter().any(|value| matches!(value, JsonBindValue::Null));
    let nonnull: Vec<&JsonBindValue> = values
        .iter()
        .copied()
        .filter(|value| !matches!(value, JsonBindValue::Null))
        .collect();
    if nonnull.is_empty() {
        return Ok((BoundType::Dynamic, true));
    }

    let ty = match nonnull[0] {
        JsonBindValue::Object(_) => {
            if !nonnull.iter().all(|value| matches!(value, JsonBindValue::Object(_))) {
                BoundType::Dynamic
            } else {
                let objects: Vec<&JsonBindValue> = nonnull;
                let name = add_json_record(builder, hint, &objects)?;
                BoundType::Named(name)
            }
        }
        JsonBindValue::Array(_) => {
            if !nonnull.iter().all(|value| matches!(value, JsonBindValue::Array(_))) {
                BoundType::Dynamic
            } else {
                let mut elements = Vec::new();
                for value in nonnull {
                    if let JsonBindValue::Array(items) = value {
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
        JsonBindValue::Bool => {
            if nonnull.iter().all(|value| matches!(value, JsonBindValue::Bool)) {
                BoundType::Scalar("Bool")
            } else {
                BoundType::Dynamic
            }
        }
        JsonBindValue::String => {
            if nonnull.iter().all(|value| matches!(value, JsonBindValue::String)) {
                BoundType::Scalar("String")
            } else {
                BoundType::Dynamic
            }
        }
        JsonBindValue::Number { .. } => {
            if nonnull
                .iter()
                .all(|value| matches!(value, JsonBindValue::Number { .. }))
            {
                if nonnull.iter().any(|value| {
                    matches!(
                        value,
                        JsonBindValue::Number { lexeme, .. }
                            if lexeme.contains(['.', 'e', 'E'])
                    )
                }) {
                    BoundType::Scalar("Float")
                } else {
                    BoundType::Scalar("Int")
                }
            } else {
                BoundType::Dynamic
            }
        }
        JsonBindValue::Null => BoundType::Dynamic,
    };
    Ok((ty, nullable))
}

fn add_json_record(
    builder: &mut SchemaBuilder,
    preferred: &str,
    objects: &[&JsonBindValue],
) -> Result<String, String> {
    let (name, index) = builder.begin_record(preferred);
    let mut keys = BTreeSet::new();
    for value in objects {
        let JsonBindValue::Object(object) = value else {
            return Err("JSON schema contains a non-object where an object was inferred".to_string());
        };
        keys.extend(object.keys().cloned());
    }
    let mut fields = Vec::new();
    for key in keys {
        let mut values = Vec::new();
        let mut optional = false;
        for value in objects {
            let JsonBindValue::Object(object) = value else {
                unreachable!("JSON object checked above")
            };
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
    let value = JsonBindParser::new(input)
        .parse()
        .map_err(|error| format!("malformed JSON: {error}"))?;
    if !matches!(&value, JsonBindValue::Object(_)) {
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
        } else if ch.is_ascii_digit() {
            let start = index;
            index += 1;
            while index < chars.len() && chars[index].is_ascii_digit() {
                index += 1;
            }
            if chars.get(index) == Some(&'.')
                && chars
                    .get(index + 1)
                    .is_some_and(|value| value.is_ascii_digit())
            {
                index += 1;
                while index < chars.len() && chars[index].is_ascii_digit() {
                    index += 1;
                }
            }
            if chars
                .get(index)
                .is_some_and(|value| *value == 'e' || *value == 'E')
            {
                index += 1;
                if chars
                    .get(index)
                    .is_some_and(|value| *value == '+' || *value == '-')
                {
                    index += 1;
                }
                let digits = index;
                while index < chars.len() && chars[index].is_ascii_digit() {
                    index += 1;
                }
                if digits == index {
                    return Err("SQL numeric exponent has no digits".to_string());
                }
            }
            tokens.push(chars[start..index].iter().collect());
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

fn sql_is_name(token: &str) -> bool {
    !token.is_empty()
        && !matches!(
            token,
            "(" | ")" | "," | "." | "<" | ">" | "*" | "%" | "+" | "=" | "|"
                | "&" | "!" | "~" | "^" | ":" | "[" | "]" | "-"
        )
}

fn sql_name_key(name: &str) -> String {
    name.to_ascii_lowercase()
}

fn sql_take_name(tokens: &[String], index: &mut usize, what: &str) -> Result<String, String> {
    let Some(token) = tokens.get(*index) else {
        return Err(format!("SQL {what} is missing"));
    };
    if !sql_is_name(token) {
        return Err(format!("SQL {what} is invalid"));
    }
    *index += 1;
    Ok(token.clone())
}

fn sql_take_digits(tokens: &[String], index: &mut usize, what: &str) -> Result<u64, String> {
    let value = sql_take_name(tokens, index, what)?;
    let number = value
        .parse::<u64>()
        .map_err(|_| format!("SQL {what} must be a non-negative integer"))?;
    Ok(number)
}

fn sql_take_positive_digits(
    tokens: &[String],
    index: &mut usize,
    what: &str,
) -> Result<u64, String> {
    let number = sql_take_digits(tokens, index, what)?;
    if number == 0 {
        return Err(format!("SQL {what} must be greater than zero"));
    }
    Ok(number)
}

fn sql_parenthesized_digits(
    tokens: &[String],
    index: &mut usize,
    what: &str,
    two: bool,
) -> Result<(), String> {
    if tokens.get(*index).map(String::as_str) != Some("(") {
        return Ok(());
    }
    *index += 1;
    let first = if what.contains("time precision") || what.contains("timestamp precision") {
        sql_take_digits(tokens, index, what)?
    } else {
        sql_take_positive_digits(tokens, index, what)?
    };
    let mut second = None;
    if tokens.get(*index).map(String::as_str) == Some(",") {
        if !two {
            return Err(format!("SQL {what} has an unsupported second modifier"));
        }
        *index += 1;
        second = Some(sql_take_digits(tokens, index, what)?);
    }
    if tokens.get(*index).map(String::as_str) != Some(")") {
        return Err(format!("SQL {what} modifier is not closed"));
    }
    *index += 1;
    if let Some(scale) = second {
        if scale > first {
            return Err(format!("SQL {what} scale exceeds its precision"));
        }
    }
    Ok(())
}

fn sql_numeric_modifiers(tokens: &[String], index: &mut usize) -> Result<(), String> {
    let mut signedness = false;
    let mut zerofill = false;
    loop {
        if tokens
            .get(*index)
            .is_some_and(|value| value.eq_ignore_ascii_case("SIGNED") || value.eq_ignore_ascii_case("UNSIGNED"))
        {
            if signedness {
                return Err("SQL numeric type repeats its signedness modifier".to_string());
            }
            signedness = true;
            *index += 1;
        } else if tokens
            .get(*index)
            .is_some_and(|value| value.eq_ignore_ascii_case("ZEROFILL"))
        {
            if zerofill {
                return Err("SQL numeric type repeats ZEROFILL".to_string());
            }
            zerofill = true;
            *index += 1;
        } else {
            return Ok(());
        }
    }
}

fn sql_type(tokens: &[String], index: &mut usize, column: &str) -> Result<&'static str, String> {
    let Some(token) = tokens.get(*index) else {
        return Err(format!("SQL column {column} has no type"));
    };
    let first = token.to_ascii_uppercase();
    *index += 1;
    let ty = match first.as_str() {
        "INT" | "INTEGER" | "BIGINT" | "SMALLINT" | "TINYINT" | "MEDIUMINT" | "SERIAL"
        | "BIGSERIAL" => {
            sql_numeric_modifiers(tokens, index)?;
            "Int"
        }
        "REAL" | "FLOAT" => {
            sql_parenthesized_digits(tokens, index, "floating-point precision", false)?;
            sql_numeric_modifiers(tokens, index)?;
            "Float"
        }
        "DOUBLE" => {
            if tokens
                .get(*index)
                .is_some_and(|value| value.eq_ignore_ascii_case("PRECISION"))
            {
                *index += 1;
            }
            sql_numeric_modifiers(tokens, index)?;
            "Float"
        }
        "DECIMAL" | "NUMERIC" | "MONEY" => {
            sql_parenthesized_digits(tokens, index, "decimal precision", true)?;
            sql_numeric_modifiers(tokens, index)?;
            "Decimal"
        }
        "BOOL" | "BOOLEAN" => "Bool",
        "TEXT" | "TINYTEXT" | "MEDIUMTEXT" | "LONGTEXT" | "VARCHAR" | "NVARCHAR"
        | "CHAR" | "CHARACTER" | "CLOB" => {
            if first == "CHARACTER"
                && tokens
                    .get(*index)
                    .is_some_and(|value| value.eq_ignore_ascii_case("VARYING"))
            {
                *index += 1;
            }
            if matches!(first.as_str(), "VARCHAR" | "NVARCHAR" | "CHAR" | "CHARACTER") {
                sql_parenthesized_digits(tokens, index, "character width", false)?;
            } else if tokens.get(*index).map(String::as_str) == Some("(") {
                return Err(format!("SQL type {first} does not accept a width modifier"));
            }
            "String"
        }
        "DATE" => "LocalDate",
        "TIME" => {
            sql_parenthesized_digits(tokens, index, "time precision", false)?;
            if tokens
                .get(*index)
                .is_some_and(|value| value.eq_ignore_ascii_case("WITH") || value.eq_ignore_ascii_case("WITHOUT"))
            {
                *index += 1;
                if !tokens
                    .get(*index)
                    .is_some_and(|value| value.eq_ignore_ascii_case("TIME"))
                {
                    return Err("SQL time zone modifier needs TIME ZONE".to_string());
                }
                *index += 1;
                if !tokens
                    .get(*index)
                    .is_some_and(|value| value.eq_ignore_ascii_case("ZONE"))
                {
                    return Err("SQL time zone modifier needs TIME ZONE".to_string());
                }
                *index += 1;
            }
            "LocalTime"
        }
        "TIMESTAMP" | "DATETIME" => {
            sql_parenthesized_digits(tokens, index, "timestamp precision", false)?;
            if first == "TIMESTAMP"
                && tokens
                    .get(*index)
                    .is_some_and(|value| value.eq_ignore_ascii_case("WITH") || value.eq_ignore_ascii_case("WITHOUT"))
            {
                *index += 1;
                if !tokens
                    .get(*index)
                    .is_some_and(|value| value.eq_ignore_ascii_case("TIME"))
                {
                    return Err("SQL time zone modifier needs TIME ZONE".to_string());
                }
                *index += 1;
                if !tokens
                    .get(*index)
                    .is_some_and(|value| value.eq_ignore_ascii_case("ZONE"))
                {
                    return Err("SQL time zone modifier needs TIME ZONE".to_string());
                }
                *index += 1;
            }
            "DateTime"
        }
        "BLOB" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" | "BYTEA" | "BINARY" | "VARBINARY" => {
            if matches!(first.as_str(), "BINARY" | "VARBINARY") {
                sql_parenthesized_digits(tokens, index, "binary width", false)?;
            } else if tokens.get(*index).map(String::as_str) == Some("(") {
                return Err(format!("SQL type {first} does not accept a width modifier"));
            }
            "[U8]"
        }
        _ => return Err(format!("SQL type for {column} is unsupported")),
    };
    Ok(ty)
}

fn sql_balanced_expression(
    tokens: &[String],
    index: &mut usize,
    what: &str,
) -> Result<(), String> {
    if tokens.get(*index).map(String::as_str) != Some("(") {
        return Err(format!("SQL {what} needs an open parenthesis"));
    }
    *index += 1;
    let start = *index;
    let mut depth = 1i32;
    while *index < tokens.len() {
        match tokens[*index].as_str() {
            "(" => depth += 1,
            ")" => {
                depth -= 1;
                if depth == 0 {
                    if *index == start {
                        return Err(format!("SQL {what} is empty"));
                    }
                    *index += 1;
                    return Ok(());
                }
            }
            _ => {}
        }
        *index += 1;
    }
    Err(format!("SQL {what} is not closed"))
}

fn sql_name_list(tokens: &[String], index: &mut usize, what: &str) -> Result<Vec<String>, String> {
    if tokens.get(*index).map(String::as_str) != Some("(") {
        return Err(format!("SQL {what} needs an open parenthesis"));
    }
    *index += 1;
    let mut names = Vec::new();
    loop {
        names.push(sql_take_name(tokens, index, what)?);
        if tokens.get(*index).map(String::as_str) == Some(",") {
            *index += 1;
            continue;
        }
        if tokens.get(*index).map(String::as_str) != Some(")") {
            return Err(format!("SQL {what} list is not closed"));
        }
        *index += 1;
        return Ok(names);
    }
}

fn sql_reference(tokens: &[String], index: &mut usize) -> Result<(), String> {
    sql_take_name(tokens, index, "referenced table")?;
    while tokens.get(*index).map(String::as_str) == Some(".") {
        *index += 1;
        sql_take_name(tokens, index, "referenced table segment")?;
    }
    if tokens.get(*index).map(String::as_str) == Some("(") {
        sql_name_list(tokens, index, "referenced column")?;
    }
    loop {
        let Some(token) = tokens.get(*index) else { break };
        match token.to_ascii_uppercase().as_str() {
            "MATCH" => {
                *index += 1;
                sql_take_name(tokens, index, "MATCH name")?;
            }
            "ON" => {
                *index += 1;
                let action_kind = sql_take_name(tokens, index, "foreign-key action")?;
                if !matches!(action_kind.to_ascii_uppercase().as_str(), "DELETE" | "UPDATE") {
                    return Err("SQL foreign-key action needs DELETE or UPDATE".to_string());
                }
                let action = sql_take_name(tokens, index, "foreign-key action")?;
                match action.to_ascii_uppercase().as_str() {
                    "NO" => {
                        let next = sql_take_name(tokens, index, "foreign-key action")?;
                        if !next.eq_ignore_ascii_case("ACTION") {
                            return Err("SQL foreign-key action `NO` needs ACTION".to_string());
                        }
                    }
                    "SET" => {
                        let next = sql_take_name(tokens, index, "foreign-key action")?;
                        if !matches!(next.to_ascii_uppercase().as_str(), "NULL" | "DEFAULT") {
                            return Err("SQL foreign-key SET action needs NULL or DEFAULT".to_string());
                        }
                    }
                    "RESTRICT" | "CASCADE" => {}
                    _ => return Err("SQL foreign-key action is unsupported".to_string()),
                }
            }
            "DEFERRABLE" => *index += 1,
            "NOT" if tokens
                .get(*index + 1)
                .is_some_and(|value| value.eq_ignore_ascii_case("DEFERRABLE")) => {
                *index += 2;
            }
            "INITIALLY" => {
                *index += 1;
                let mode = sql_take_name(tokens, index, "constraint mode")?;
                if !matches!(mode.to_ascii_uppercase().as_str(), "DEFERRED" | "IMMEDIATE") {
                    return Err("SQL constraint mode is unsupported".to_string());
                }
            }
            _ => break,
        }
    }
    Ok(())
}

fn sql_default(tokens: &[String], index: &mut usize) -> Result<(), String> {
    if tokens.get(*index).map(String::as_str) == Some("(") {
        return sql_balanced_expression(tokens, index, "DEFAULT expression");
    }
    if tokens
        .get(*index)
        .is_some_and(|value| value == "+" || value == "-")
    {
        *index += 1;
        sql_take_name(tokens, index, "DEFAULT numeric value")?;
    } else {
        sql_take_name(tokens, index, "DEFAULT value")?;
    }
    if tokens.get(*index).map(String::as_str) == Some(".") {
        *index += 1;
        sql_take_digits(tokens, index, "DEFAULT numeric value")?;
    }
    if tokens
        .get(*index)
        .is_some_and(|value| value.eq_ignore_ascii_case("E"))
    {
        *index += 1;
        if tokens
            .get(*index)
            .is_some_and(|value| value == "+" || value == "-")
        {
            *index += 1;
        }
        sql_take_digits(tokens, index, "DEFAULT exponent")?;
    }
    if tokens.get(*index).map(String::as_str) == Some("(") {
        *index += 1;
        if tokens.get(*index).map(String::as_str) == Some(")") {
            *index += 1;
        } else {
            *index -= 1;
            sql_balanced_expression(tokens, index, "DEFAULT function arguments")?;
        }
    }
    Ok(())
}

fn sql_constraint(
    tokens: &[String],
    index: &mut usize,
    required: &mut bool,
) -> Result<(), String> {
    if tokens
        .get(*index)
        .is_some_and(|value| value.eq_ignore_ascii_case("CONSTRAINT"))
    {
        *index += 1;
        sql_take_name(tokens, index, "constraint name")?;
    }
    let Some(token) = tokens.get(*index) else {
        return Err("SQL constraint is incomplete".to_string());
    };
    match token.to_ascii_uppercase().as_str() {
        "NOT" => {
            *index += 1;
            if !tokens
                .get(*index)
                .is_some_and(|value| value.eq_ignore_ascii_case("NULL"))
            {
                return Err("SQL NOT constraint needs NULL".to_string());
            }
            *index += 1;
            *required = true;
        }
        "NULL" => *index += 1,
        "PRIMARY" => {
            *index += 1;
            if !tokens
                .get(*index)
                .is_some_and(|value| value.eq_ignore_ascii_case("KEY"))
            {
                return Err("SQL PRIMARY constraint needs KEY".to_string());
            }
            *index += 1;
            *required = true;
            for modifier in ["ASC", "DESC", "AUTOINCREMENT", "AUTO_INCREMENT"] {
                if tokens
                    .get(*index)
                    .is_some_and(|value| value.eq_ignore_ascii_case(modifier))
                {
                    *index += 1;
                }
            }
        }
        "UNIQUE" => {
            *index += 1;
            if tokens
                .get(*index)
                .is_some_and(|value| value.eq_ignore_ascii_case("KEY") || value.eq_ignore_ascii_case("INDEX"))
            {
                *index += 1;
            }
            if tokens.get(*index).is_some_and(|value| {
                sql_is_name(value)
                    && !matches!(
                        value.to_ascii_uppercase().as_str(),
                        "NOT"
                            | "NULL"
                            | "PRIMARY"
                            | "REFERENCES"
                            | "CHECK"
                            | "DEFAULT"
                            | "COLLATE"
                            | "GENERATED"
                            | "AUTO_INCREMENT"
                            | "AUTOINCREMENT"
                            | "ON"
                            | "COMMENT"
                    )
            }) {
                *index += 1;
            }
        }
        "KEY" | "INDEX" => {
            *index += 1;
            if tokens.get(*index).is_some_and(|value| sql_is_name(value)) {
                *index += 1;
            }
        }
        "REFERENCES" => {
            *index += 1;
            sql_reference(tokens, index)?;
        }
        "CHECK" => {
            *index += 1;
            sql_balanced_expression(tokens, index, "CHECK expression")?;
        }
        "DEFAULT" => {
            *index += 1;
            sql_default(tokens, index)?;
        }
        "COLLATE" => {
            *index += 1;
            sql_take_name(tokens, index, "collation name")?;
        }
        "GENERATED" => {
            *index += 1;
            if tokens
                .get(*index)
                .is_some_and(|value| value.eq_ignore_ascii_case("ALWAYS"))
            {
                *index += 1;
            }
            if !tokens
                .get(*index)
                .is_some_and(|value| value.eq_ignore_ascii_case("AS"))
            {
                return Err("SQL GENERATED constraint needs AS".to_string());
            }
            *index += 1;
            if tokens
                .get(*index)
                .is_some_and(|value| value.eq_ignore_ascii_case("IDENTITY"))
            {
                *index += 1;
                if tokens.get(*index).map(String::as_str) == Some("(") {
                    sql_balanced_expression(tokens, index, "IDENTITY options")?;
                }
            } else {
                sql_balanced_expression(tokens, index, "GENERATED expression")?;
                if tokens
                    .get(*index)
                    .is_some_and(|value| value.eq_ignore_ascii_case("VIRTUAL") || value.eq_ignore_ascii_case("STORED"))
                {
                    *index += 1;
                } else {
                    return Err("SQL GENERATED constraint needs VIRTUAL or STORED".to_string());
                }
            }
        }
        "AUTO_INCREMENT" | "AUTOINCREMENT" => *index += 1,
        "ON" => {
            *index += 1;
            if !tokens
                .get(*index)
                .is_some_and(|value| value.eq_ignore_ascii_case("UPDATE"))
            {
                return Err("SQL ON constraint needs UPDATE".to_string());
            }
            *index += 1;
            sql_default(tokens, index)?;
        }
        "COMMENT" => {
            *index += 1;
            sql_take_name(tokens, index, "column comment")?;
        }
        _ => return Err(format!("SQL column constraint {} is unsupported", token)),
    }
    Ok(())
}

fn sql_table_constraint(
    segment: &[String],
    required_columns: &mut BTreeSet<String>,
    primary_seen: &mut bool,
) -> Result<(), String> {
    let mut index = 0usize;
    if segment
        .get(index)
        .is_some_and(|value| value.eq_ignore_ascii_case("CONSTRAINT"))
    {
        index += 1;
        sql_take_name(segment, &mut index, "constraint name")?;
    }
    let Some(token) = segment.get(index) else {
        return Err("SQL table constraint is incomplete".to_string());
    };
    match token.to_ascii_uppercase().as_str() {
        "PRIMARY" => {
            if *primary_seen {
                return Err("SQL table has more than one PRIMARY KEY constraint".to_string());
            }
            *primary_seen = true;
            index += 1;
            if !segment
                .get(index)
                .is_some_and(|value| value.eq_ignore_ascii_case("KEY"))
            {
                return Err("SQL table PRIMARY constraint needs KEY".to_string());
            }
            index += 1;
            let columns = sql_name_list(segment, &mut index, "PRIMARY KEY column")?;
            required_columns.extend(columns.into_iter().map(|name| sql_name_key(&name)));
        }
        "UNIQUE" => {
            index += 1;
            if segment
                .get(index)
                .is_some_and(|value| value.eq_ignore_ascii_case("KEY") || value.eq_ignore_ascii_case("INDEX"))
            {
                index += 1;
                if segment.get(index).is_some_and(|value| sql_is_name(value)) {
                    index += 1;
                }
            }
            sql_name_list(segment, &mut index, "UNIQUE column")?;
        }
        "FOREIGN" => {
            index += 1;
            if !segment
                .get(index)
                .is_some_and(|value| value.eq_ignore_ascii_case("KEY"))
            {
                return Err("SQL FOREIGN constraint needs KEY".to_string());
            }
            index += 1;
            sql_name_list(segment, &mut index, "FOREIGN KEY column")?;
            if !segment
                .get(index)
                .is_some_and(|value| value.eq_ignore_ascii_case("REFERENCES"))
            {
                return Err("SQL FOREIGN constraint needs REFERENCES".to_string());
            }
            index += 1;
            sql_reference(segment, &mut index)?;
        }
        "CHECK" => {
            index += 1;
            sql_balanced_expression(segment, &mut index, "table CHECK expression")?;
        }
        _ => return Err(format!("SQL table constraint {} is unsupported", token)),
    }
    if index != segment.len() {
        return Err("SQL table constraint has trailing tokens".to_string());
    }
    Ok(())
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
    let first_name = sql_take_name(&tokens, &mut index, "table name")?;
    let table_name = if word(index) == Some(".") {
        index += 1;
        sql_take_name(&tokens, &mut index, "qualified table name")?
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
                    return Err("SQL table has an empty declaration".to_string());
                }
                segments.push(&tokens[start..position]);
                start = position + 1;
            }
            _ => {}
        }
    }
    if start == close {
        return Err("SQL table has an empty declaration".to_string());
    }
    segments.push(&tokens[start..close]);
    let mut names = BTreeSet::new();
    let mut fields = Vec::new();
    let mut required_columns = BTreeSet::new();
    let mut primary_seen = false;
    for segment in segments {
        let first = segment.first().map(String::as_str).unwrap_or("");
        if matches!(
            first.to_ascii_uppercase().as_str(),
            "PRIMARY" | "UNIQUE" | "FOREIGN" | "CHECK" | "CONSTRAINT"
        ) {
            sql_table_constraint(segment, &mut required_columns, &mut primary_seen)?;
            continue;
        }
        if segment.len() < 2 {
            return Err("SQL column needs a name and type".to_string());
        }
        let column = segment[0].clone();
        if !sql_is_name(&column) {
            return Err("SQL column name is invalid".to_string());
        }
        if !names.insert(sql_name_key(&column)) {
            return Err(format!("SQL has duplicate column {column}"));
        }
        let mut type_index = 1usize;
        let ty = sql_type(segment, &mut type_index, &column)?;
        let mut required = false;
        while type_index < segment.len() {
            sql_constraint(segment, &mut type_index, &mut required)?;
        }
        fields.push(FieldSeed {
            wire_name: column,
            ty: bound_scalar(ty),
            optional: !required,
            note: None,
        });
    }
    if fields.is_empty() {
        return Err(format!("SQL table {table_name} has no columns"));
    }
    if required_columns
        .iter()
        .any(|column| !names.contains(column))
    {
        return Err("SQL PRIMARY KEY names an unknown column".to_string());
    }
    for field in &mut fields {
        if required_columns.contains(&sql_name_key(&field.wire_name)) {
            field.optional = false;
        }
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
        if !table_names.insert(sql_name_key(&name)) {
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
enum XmlContent {
    Text,
    CData,
    Element(XmlNode),
    Comment,
    ProcessingInstruction,
}

#[derive(Clone, Debug)]
struct XmlNode {
    name: String,
    attrs: Vec<(String, String)>,
    content: Vec<XmlContent>,
    namespaces: BTreeMap<String, String>,
}

const XML_NAMESPACE_URI: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS_NAMESPACE_URI: &str = "http://www.w3.org/2000/xmlns/";

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

    fn starts_with_xml_declaration(&self) -> bool {
        self.starts_with("<?xml")
            && self
                .chars
                .get(self.pos + 5)
                .is_some_and(|ch| ch.is_ascii_whitespace() || *ch == '?')
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn skip_space(&mut self) {
        while self
            .peek()
            .is_some_and(|ch| matches!(ch, ' ' | '\t' | '\r' | '\n'))
        {
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
        let name: String = self.chars[start..self.pos].iter().collect();
        if name.starts_with(':')
            || name.ends_with(':')
            || name.matches(':').count() > 1
            || name.contains("::")
        {
            return Err(format!("XML name {name} has an invalid namespace separator"));
        }
        Ok(name)
    }

    fn initial_namespaces() -> BTreeMap<String, String> {
        BTreeMap::from([(String::from("xml"), String::from(XML_NAMESPACE_URI))])
    }

    fn is_namespace_declaration(name: &str) -> bool {
        name == "xmlns" || name.starts_with("xmlns:")
    }

    fn resolve_namespaces(
        inherited: &BTreeMap<String, String>,
        attrs: &[(String, String)],
    ) -> Result<BTreeMap<String, String>, String> {
        let mut namespaces = inherited.clone();
        for (name, value) in attrs {
            let Some(prefix) = name.strip_prefix("xmlns:") else {
                if name == "xmlns" {
                    namespaces.insert(String::new(), value.clone());
                }
                continue;
            };
            if prefix.is_empty() || prefix == "xmlns" {
                return Err(format!("XML namespace prefix {prefix} is reserved"));
            }
            if value.is_empty() {
                return Err(format!("XML namespace prefix {prefix} has an empty URI"));
            }
            if prefix == "xml" && value != XML_NAMESPACE_URI {
                return Err("XML prefix xml must use its reserved namespace URI".to_string());
            }
            if value == XMLNS_NAMESPACE_URI {
                return Err("XML namespace URI for xmlns is reserved".to_string());
            }
            namespaces.insert(prefix.to_string(), value.clone());
        }
        if let Some(xml_uri) = namespaces.get("xml") {
            if xml_uri != XML_NAMESPACE_URI {
                return Err("XML prefix xml must use its reserved namespace URI".to_string());
            }
        }
        Ok(namespaces)
    }

    fn resolve_name(
        name: &str,
        namespaces: &BTreeMap<String, String>,
        element: bool,
    ) -> Result<String, String> {
        if let Some((prefix, local)) = name.split_once(':') {
            let namespace = namespaces
                .get(prefix)
                .filter(|namespace| !namespace.is_empty())
                .ok_or_else(|| format!("XML name {name} uses an undeclared namespace prefix {prefix}"))?;
            return Ok(format!("{{{namespace}}}{local}"));
        }
        if element {
            if let Some(namespace) = namespaces.get("").filter(|namespace| !namespace.is_empty()) {
                return Ok(format!("{{{namespace}}}{name}"));
            }
        }
        Ok(name.to_string())
    }

    fn attach(
        stack: &mut Vec<XmlNode>,
        root: &mut Option<XmlNode>,
        node: XmlNode,
    ) -> Result<(), String> {
        if let Some(parent) = stack.last_mut() {
            parent.content.push(XmlContent::Element(node));
        } else if root.is_some() {
            return Err("XML has more than one root element".to_string());
        } else {
            *root = Some(node);
        }
        Ok(())
    }

    fn read_quoted_value(&mut self, what: &str) -> Result<String, String> {
        let quote = self
            .peek()
            .filter(|ch| *ch == '"' || *ch == '\'')
            .ok_or_else(|| format!("XML {what} is not quoted"))?;
        self.pos += 1;
        let start = self.pos;
        while self.pos < self.chars.len() && self.peek() != Some(quote) {
            self.pos += 1;
        }
        if self.pos >= self.chars.len() {
            return Err(format!("XML {what} is not closed"));
        }
        let value: String = self.chars[start..self.pos].iter().collect();
        validate_xml_characters(&value)?;
        self.pos += 1;
        Ok(value)
    }

    fn read_declaration_field(&mut self, expected: &str) -> Result<String, String> {
        let name = self.read_name()?;
        if name != expected {
            return Err(format!("XML declaration expected {expected}"));
        }
        self.skip_space();
        if self.peek() != Some('=') {
            return Err(format!("XML declaration field {expected} has no equals sign"));
        }
        self.pos += 1;
        self.skip_space();
        self.read_quoted_value("declaration value")
    }

    fn consume_processing_instruction(&mut self) -> Result<(), String> {
        self.pos += 2;
        let target = self.read_name()?;
        if target.eq_ignore_ascii_case("xml") {
            return Err("XML processing-instruction target cannot be xml".to_string());
        }
        if self.starts_with("?>") {
            self.pos += 2;
            return Ok(());
        }
        if !self
            .peek()
            .is_some_and(|ch| matches!(ch, ' ' | '\t' | '\r' | '\n'))
        {
            return Err("XML processing instruction needs whitespace after its target".to_string());
        }
        let start = self.pos;
        while self.pos < self.chars.len() && !self.starts_with("?>") {
            self.pos += 1;
        }
        if self.pos >= self.chars.len() {
            return Err("XML processing instruction is not closed".to_string());
        }
        let data: String = self.chars[start..self.pos].iter().collect();
        validate_xml_characters(&data)?;
        self.pos += 2;
        Ok(())
    }

    fn consume_xml_declaration(&mut self) -> Result<(), String> {
        self.pos += 5;
        if !self
            .peek()
            .is_some_and(|ch| matches!(ch, ' ' | '\t' | '\r' | '\n'))
        {
            return Err("XML declaration needs a version field".to_string());
        }
        self.skip_space();
        let version = self.read_declaration_field("version")?;
        if version != "1.0" {
            return Err("XML declaration version must be 1.0".to_string());
        }
        if !self.starts_with("?>") {
            if !self
                .peek()
                .is_some_and(|ch| matches!(ch, ' ' | '\t' | '\r' | '\n'))
            {
                return Err("XML declaration fields need whitespace separators".to_string());
            }
            self.skip_space();
            if self.starts_with("encoding") {
                let encoding = self.read_declaration_field("encoding")?;
                if !encoding.eq_ignore_ascii_case("UTF-8") {
                    return Err("XML declaration encoding must be UTF-8".to_string());
                }
                if !self.starts_with("?>") {
                    if !self
                        .peek()
                        .is_some_and(|ch| matches!(ch, ' ' | '\t' | '\r' | '\n'))
                    {
                        return Err("XML declaration fields need whitespace separators".to_string());
                    }
                    self.skip_space();
                }
            }
            if !self.starts_with("?>") {
                let standalone = self.read_declaration_field("standalone")?;
                if !matches!(standalone.as_str(), "yes" | "no") {
                    return Err("XML declaration standalone must be yes or no".to_string());
                }
            }
        }
        self.skip_space();
        if !self.starts_with("?>") {
            return Err("XML declaration has trailing content".to_string());
        }
        self.pos += 2;
        Ok(())
    }

    fn parse_document(mut self) -> Result<XmlNode, String> {
        if self.peek() == Some('\u{feff}') {
            self.pos += 1;
        }
        let mut declaration_allowed = true;
        let mut stack: Vec<XmlNode> = Vec::new();
        let mut root = None;
        while self.pos < self.chars.len() {
            if self.starts_with_xml_declaration() {
                if !declaration_allowed {
                    return Err("XML declaration is out of order".to_string());
                }
                self.consume_xml_declaration()?;
                declaration_allowed = false;
                continue;
            }
            declaration_allowed = false;
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
                    parent.content.push(XmlContent::Text);
                } else if text.chars().any(|ch| !matches!(ch, ' ' | '\t' | '\r' | '\n')) {
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
                if body.contains("--") || body.ends_with('-') {
                    return Err("XML comment contains an invalid double hyphen".to_string());
                }
                validate_xml_characters(&body)?;
                if let Some(parent) = stack.last_mut() {
                    parent.content.push(XmlContent::Comment);
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
                validate_xml_characters(&body)?;
                if let Some(parent) = stack.last_mut() {
                    parent.content.push(XmlContent::CData);
                } else {
                    return Err("XML CDATA appears outside its root element".to_string());
                }
                self.pos += 3;
                continue;
            }
            if self.starts_with("<?") {
                self.consume_processing_instruction()?;
                if let Some(parent) = stack.last_mut() {
                    parent.content.push(XmlContent::ProcessingInstruction);
                }
                continue;
            }
            if self.starts_with("</") {
                self.pos += 2;
                let raw_name = self.read_name()?;
                self.skip_space();
                if self.peek() != Some('>') {
                    return Err("XML close tag has trailing content".to_string());
                }
                self.pos += 1;
                let node = stack
                    .pop()
                    .ok_or_else(|| "XML has an unmatched close tag".to_string())?;
                let name = Self::resolve_name(&raw_name, &node.namespaces, true)?;
                if node.name != name {
                    return Err(format!(
                        "XML close tag {raw_name} does not match {}",
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
            let raw_name = self.read_name()?;
            let mut raw_attrs = Vec::new();
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
                let value = self.read_quoted_value(&format!("attribute {attr_name}"))?;
                if value.contains('<') {
                    return Err(format!("XML attribute {attr_name} contains <"));
                }
                validate_xml_entities(&value)?;
                raw_attrs.push((attr_name, value));
            }
            let inherited = stack
                .last()
                .map(|node| node.namespaces.clone())
                .unwrap_or_else(Self::initial_namespaces);
            let namespaces = Self::resolve_namespaces(&inherited, &raw_attrs)?;
            let name = Self::resolve_name(&raw_name, &namespaces, true)?;
            let mut attrs = Vec::new();
            let mut expanded_attr_names = BTreeSet::new();
            for (attr_name, value) in raw_attrs {
                if Self::is_namespace_declaration(&attr_name) {
                    continue;
                }
                let expanded = Self::resolve_name(&attr_name, &namespaces, false)?;
                if !expanded_attr_names.insert(expanded.clone()) {
                    return Err(format!("XML has duplicate attribute {expanded}"));
                }
                attrs.push((expanded, value));
            }
            let node = XmlNode {
                name,
                attrs,
                content: Vec::new(),
                namespaces,
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

fn xml_child_elements(node: &XmlNode) -> impl Iterator<Item = &XmlNode> {
    node.content.iter().filter_map(|content| match content {
        XmlContent::Element(child) => Some(child),
        _ => None,
    })
}

fn xml_is_text_like(content: &XmlContent) -> bool {
    matches!(content, XmlContent::Text | XmlContent::CData)
}

fn xml_is_simple(node: &XmlNode) -> bool {
    node.content.iter().all(xml_is_text_like)
}

fn validate_xml_characters(text: &str) -> Result<(), String> {
    if text.chars().any(|ch| !xml_character_allowed(ch)) {
        return Err("XML contains an invalid control character".to_string());
    }
    Ok(())
}

fn xml_character_allowed(ch: char) -> bool {
    let codepoint = ch as u32;
    matches!(ch, '\t' | '\n' | '\r')
        || (0x20..=0xD7FF).contains(&codepoint)
            && !(0x7F..=0x9F).contains(&codepoint)
        || (0xE000..=0xFFFD).contains(&codepoint)
        || (0x10000..=0x10FFFF).contains(&codepoint)
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
        let valid_numeric = entity
            .strip_prefix("#x")
            .or_else(|| entity.strip_prefix("#X"))
            .and_then(|digits| {
                if !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_hexdigit()) {
                    u32::from_str_radix(digits, 16).ok()
                } else {
                    None
                }
            })
            .or_else(|| {
                entity.strip_prefix('#').and_then(|digits| {
                    if !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit()) {
                        digits.parse::<u32>().ok()
                    } else {
                        None
                    }
                })
            });
        let valid = matches!(entity.as_str(), "amp" | "lt" | "gt" | "quot" | "apos")
            || valid_numeric.is_some_and(|codepoint| {
                char::from_u32(codepoint).is_some_and(xml_character_allowed)
            });
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
        child_names.extend(xml_child_elements(node).map(|child| child.name.clone()));
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
    let simple_count = nodes.iter().filter(|node| xml_is_simple(node)).count();
    let mixed_count = nodes.len() - simple_count;
    if simple_count > 0 {
        fields.push(FieldSeed {
            wire_name: "$text".to_string(),
            ty: BoundType::Scalar("String"),
            optional: mixed_count > 0,
            note: None,
        });
    }
    if mixed_count > 0 {
        fields.push(FieldSeed {
            wire_name: "$content".to_string(),
            ty: BoundType::Scalar("DataTree"),
            optional: simple_count > 0,
            note: None,
        });
    }
    for child_name in child_names {
        let mut children = Vec::new();
        let mut present_count = 0usize;
        let mut repeated = false;
        for node in nodes {
            let matching: Vec<&XmlNode> = xml_child_elements(node)
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
        let ty = if children
            .iter()
            .all(|child| child.attrs.is_empty() && xml_is_simple(child))
        {
            let scalar = BoundType::Scalar("String");
            if repeated {
                BoundType::Array(Box::new(scalar))
            } else {
                scalar
            }
        } else {
            let child_type = add_xml_record(builder, &child_hint, &children);
            if repeated {
                BoundType::Array(Box::new(BoundType::Named(child_type)))
            } else {
                BoundType::Named(child_type)
            }
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
    let root = XmlParser::new(input).parse_document()?;
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
    StringLiteral(String),
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
            if ch == '0' && chars.get(index).is_some_and(|value| *value == 'x' || *value == 'X') {
                index += 1;
                let digits = index;
                while index < chars.len() && chars[index].is_ascii_hexdigit() {
                    index += 1;
                }
                if digits == index {
                    return Err("proto hexadecimal number has no digits".to_string());
                }
            } else {
                while index < chars.len() && chars[index].is_ascii_digit() {
                    index += 1;
                }
                if chars.get(index) == Some(&'.')
                    && chars
                        .get(index + 1)
                        .is_some_and(|value| value.is_ascii_digit())
                {
                    index += 1;
                    while index < chars.len() && chars[index].is_ascii_digit() {
                        index += 1;
                    }
                }
                if chars
                    .get(index)
                    .is_some_and(|value| *value == 'e' || *value == 'E')
                {
                    index += 1;
                    if chars
                        .get(index)
                        .is_some_and(|value| *value == '+' || *value == '-')
                    {
                        index += 1;
                    }
                    let digits = index;
                    while index < chars.len() && chars[index].is_ascii_digit() {
                        index += 1;
                    }
                    if digits == index {
                        return Err("proto numeric exponent has no digits".to_string());
                    }
                }
            }
            tokens.push(ProtoToken::Number(chars[start..index].iter().collect()));
            continue;
        }
        if ch == '"' || ch == '\'' {
            let quote = ch;
            index += 1;
            let mut value = String::new();
            let mut closed = false;
            while index < chars.len() {
                match chars[index] {
                    current if current == quote => {
                        index += 1;
                        closed = true;
                        break;
                    }
                    '\\' => {
                        index += 1;
                        let Some(escaped) = chars.get(index).copied() else {
                            return Err("proto string ends after an escape".to_string());
                        };
                        let decoded = match escaped {
                            'a' => '\u{0007}',
                            'b' => '\u{0008}',
                            'f' => '\u{000c}',
                            'n' => '\n',
                            'r' => '\r',
                            't' => '\t',
                            'v' => '\u{000b}',
                            '\\' => '\\',
                            '\'' => '\'',
                            '"' => '"',
                            '?' => '?',
                            'x' => {
                                let Some(first) = chars.get(index + 1).copied() else {
                                    return Err("proto hex escape needs two digits".to_string());
                                };
                                let Some(second) = chars.get(index + 2).copied() else {
                                    return Err("proto hex escape needs two digits".to_string());
                                };
                                if !first.is_ascii_hexdigit() || !second.is_ascii_hexdigit() {
                                    return Err("proto hex escape needs two hexadecimal digits".to_string());
                                }
                                index += 2;
                                let digits = [first, second].iter().collect::<String>();
                                char::from_u32(u32::from_str_radix(&digits, 16).unwrap_or(0))
                                    .ok_or_else(|| "proto hex escape is not a character".to_string())?
                            }
                            '0'..='7' => {
                                let start = index;
                                let mut end = index + 1;
                                while end < chars.len()
                                    && end < start + 3
                                    && matches!(chars[end], '0'..='7')
                                {
                                    end += 1;
                                }
                                let digits: String = chars[start..end].iter().collect();
                                index = end - 1;
                                char::from_u32(u32::from_str_radix(&digits, 8).unwrap_or(0))
                                    .ok_or_else(|| "proto octal escape is not a character".to_string())?
                            }
                            _ => return Err(format!("proto escape \\{escaped} is unsupported")),
                        };
                        if decoded == '\u{0000}' {
                            return Err("proto string contains a NUL escape".to_string());
                        }
                        value.push(decoded);
                        index += 1;
                    }
                    '\n' | '\r' => return Err("proto string contains a newline".to_string()),
                    current if current.is_control() => {
                        return Err("proto string contains a control character".to_string());
                    }
                    current => {
                        value.push(current);
                        index += 1;
                    }
                }
            }
            if !closed {
                return Err("proto string is not closed".to_string());
            }
            tokens.push(ProtoToken::StringLiteral(value));
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

fn proto_ranges_overlap(left: (i64, i64), right: (i64, i64)) -> bool {
    left.0 <= right.1 && right.0 <= left.1
}

fn proto_range_has_prohibited_number(range: (i64, i64)) -> bool {
    range.0 <= 19_999 && range.1 >= 19_000
}

fn parse_proto_integer_literal(value: &str) -> Option<i64> {
    if let Some(hex) = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).ok()
    } else {
        value.parse::<i64>().ok()
    }
}

struct ProtoField {
    wire_name: String,
    type_name: String,
    repeated: bool,
    optional: bool,
    number: u32,
}

#[derive(Default)]
struct ProtoReserved {
    names: BTreeSet<String>,
    ranges: Vec<(i64, i64)>,
}

struct ProtoMessage {
    source_name: String,
    short_name: String,
    fields: Vec<ProtoField>,
}

struct ProtoEnum {
    source_name: String,
}

struct ProtoSchema {
    messages: Vec<ProtoMessage>,
    enums: Vec<ProtoEnum>,
}

struct ProtoParser {
    tokens: Vec<ProtoToken>,
    pos: usize,
    messages: Vec<ProtoMessage>,
    enums: Vec<ProtoEnum>,
    message_names: BTreeSet<String>,
    enum_names: BTreeSet<String>,
    package: Option<String>,
    syntax_seen: bool,
    body_started: bool,
}

impl ProtoParser {
    fn new(input: &str) -> Result<Self, String> {
        Ok(Self {
            tokens: lex_proto(input)?,
            pos: 0,
            messages: Vec::new(),
            enums: Vec::new(),
            message_names: BTreeSet::new(),
            enum_names: BTreeSet::new(),
            package: None,
            syntax_seen: false,
            body_started: false,
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

    fn take_string(&mut self, what: &str) -> Result<String, String> {
        match self.tokens.get(self.pos).cloned() {
            Some(ProtoToken::StringLiteral(value)) => {
                self.pos += 1;
                Ok(value)
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

    fn parse_type_name(&mut self) -> Result<String, String> {
        let mut name = String::new();
        if self.symbol_at(0, '.') {
            name.push('.');
            self.pos += 1;
        }
        let first = self.take_word("a field type")?;
        if !is_proto_identifier(&first)
            || (is_proto_keyword(&first) && proto_scalar(&first).is_none())
        {
            return Err(format!("proto type name {first} is invalid"));
        }
        name.push_str(&first);
        while self.symbol_at(0, '.') {
            self.pos += 1;
            name.push('.');
            let segment = self.take_word("a type segment")?;
            if !is_proto_identifier(&segment)
                || (is_proto_keyword(&segment) && proto_scalar(&segment).is_none())
            {
                return Err(format!("proto type name segment {segment} is invalid"));
            }
            name.push_str(&segment);
        }
        Ok(name)
    }

    fn parse_qualified_name(&mut self, what: &str) -> Result<String, String> {
        let first = self.take_word(what)?;
        if !is_proto_identifier(&first) {
            return Err(format!("proto name {first} is invalid"));
        }
        let mut name = first;
        while self.symbol_at(0, '.') {
            self.pos += 1;
            let segment = self.take_word(what)?;
            if !is_proto_identifier(&segment) {
                return Err(format!("proto name segment {segment} is invalid"));
            }
            name.push('.');
            name.push_str(&segment);
        }
        Ok(name)
    }

    fn parse_option_name(&mut self) -> Result<String, String> {
        if self.symbol_at(0, '(') {
            self.pos += 1;
            let mut name = format!("({})", self.parse_qualified_name("an option name")?);
            self.expect_symbol(')', "the end of a custom option name")?;
            while self.symbol_at(0, '.') {
                self.pos += 1;
                let segment = self.take_word("an option name segment")?;
                if !is_proto_identifier(&segment) {
                    return Err(format!("proto option name segment {segment} is invalid"));
                }
                name.push('.');
                name.push_str(&segment);
            }
            Ok(name)
        } else {
            self.parse_qualified_name("an option name")
        }
    }

    fn parse_option_value(&mut self) -> Result<OptionValue, String> {
        if self.symbol_at(0, '{') {
            self.pos += 1;
            while !self.symbol_at(0, '}') {
                if self.pos >= self.tokens.len() {
                    return Err("proto option aggregate is not closed".to_string());
                }
                let _ = self.parse_option_name()?;
                if self.symbol_at(0, ':') || self.symbol_at(0, '=') {
                    self.pos += 1;
                } else {
                    return Err("proto option aggregate field needs a colon".to_string());
                }
                self.parse_option_value()?;
                if self.symbol_at(0, ',') || self.symbol_at(0, ';') {
                    self.pos += 1;
                } else if !self.symbol_at(0, '}') {
                    return Err("proto option aggregate needs a separator".to_string());
                }
            }
            self.pos += 1;
            return Ok(OptionValue::Aggregate);
        }
        if let Some(value) = self.tokens.get(self.pos).cloned() {
            match value {
                ProtoToken::StringLiteral(_) => {
                    self.pos += 1;
                    while matches!(self.tokens.get(self.pos), Some(ProtoToken::StringLiteral(_))) {
                        self.pos += 1;
                    }
                    Ok(OptionValue::String)
                }
                ProtoToken::Number(_) => {
                    self.pos += 1;
                    Ok(OptionValue::Number)
                }
                ProtoToken::Word(value) => {
                    self.pos += 1;
                    if self.symbol_at(0, '.') {
                        while self.symbol_at(0, '.') {
                            self.pos += 1;
                            self.take_word("an option value segment")?;
                        }
                        Ok(OptionValue::Identifier)
                    } else if value.eq_ignore_ascii_case("true") {
                        Ok(OptionValue::Bool(true))
                    } else if value.eq_ignore_ascii_case("false") {
                        Ok(OptionValue::Bool(false))
                    } else {
                        Ok(OptionValue::Identifier)
                    }
                }
                ProtoToken::Symbol('+') | ProtoToken::Symbol('-') => {
                    self.pos += 1;
                    if !matches!(self.tokens.get(self.pos), Some(ProtoToken::Number(_))) {
                        return Err("proto option sign needs a number".to_string());
                    }
                    self.pos += 1;
                    Ok(OptionValue::Number)
                }
                ProtoToken::Symbol('.') => {
                    self.parse_type_name()?;
                    Ok(OptionValue::Identifier)
                }
                _ => Err("proto option value is malformed".to_string()),
            }
        } else {
            Err("proto option value is missing".to_string())
        }
    }

    fn parse_option_assignment(&mut self) -> Result<(String, OptionValue), String> {
        let name = self.parse_option_name()?;
        self.expect_symbol('=', "an option equals sign")?;
        let value = self.parse_option_value()?;
        Ok((name, value))
    }

    fn parse_option_statement(&mut self) -> Result<(String, OptionValue), String> {
        let _ = self.take_word("option")?;
        let assignment = self.parse_option_assignment()?;
        self.expect_symbol(';', "an option semicolon")?;
        Ok(assignment)
    }

    fn parse_field_options(&mut self) -> Result<(), String> {
        self.expect_symbol('[', "field options")?;
        if self.symbol_at(0, ']') {
            return Err("proto field options cannot be empty".to_string());
        }
        loop {
            self.parse_option_assignment()?;
            if self.symbol_at(0, ',') {
                self.pos += 1;
                if self.symbol_at(0, ']') {
                    return Err("proto field options cannot end with a comma".to_string());
                }
                continue;
            }
            self.expect_symbol(']', "the end of field options")?;
            return Ok(());
        }
    }

    fn parse_integer(&mut self, what: &str, signed: bool) -> Result<i64, String> {
        let mut sign = 1i64;
        if signed && (self.symbol_at(0, '+') || self.symbol_at(0, '-')) {
            if self.symbol_at(0, '-') {
                sign = -1;
            }
            self.pos += 1;
        }
        let Some(ProtoToken::Number(number)) = self.tokens.get(self.pos).cloned() else {
            return Err(format!("proto expected {what}"));
        };
        if number.contains('.') || number.contains('e') || number.contains('E') {
            return Err(format!("proto {what} must be an integer"));
        }
        self.pos += 1;
        let value = parse_proto_integer_literal(&number)
            .ok_or_else(|| format!("proto {what} is out of range"))?;
        value
            .checked_mul(sign)
            .ok_or_else(|| format!("proto {what} is out of range"))
    }

    fn parse_reserved(
        &mut self,
        signed: bool,
        maximum: i64,
    ) -> Result<ProtoReserved, String> {
        let _ = self.take_word("reserved")?;
        let mut reserved = ProtoReserved::default();
        loop {
            if let Some(ProtoToken::StringLiteral(name)) = self.tokens.get(self.pos).cloned() {
                self.pos += 1;
                if name.is_empty()
                    || name.contains('.')
                    || name.split('.').any(|part| {
                        !is_proto_identifier(part) || is_proto_keyword(part)
                    })
                {
                    return Err(format!("proto reserved name {name:?} is invalid"));
                }
                if !reserved.names.insert(name.clone()) {
                    return Err(format!("proto reserved name {name} is repeated"));
                }
            } else {
                let start = self.parse_integer("a reserved number", signed)?;
                let end = if self.word_at(0) == Some("to") {
                    self.pos += 1;
                    if self.word_at(0) == Some("max") {
                        self.pos += 1;
                        maximum
                    } else {
                        self.parse_integer("a reserved range end", signed)?
                    }
                } else {
                    start
                };
                let minimum = if signed { i32::MIN as i64 } else { 1 };
                if start < minimum || end < minimum || start > end || end > maximum {
                    return Err("proto reserved range is out of range".to_string());
                }
                if proto_range_has_prohibited_number((start, end)) {
                    return Err("proto reserved range uses prohibited field numbers".to_string());
                }
                for (old_start, old_end) in &reserved.ranges {
                    if proto_ranges_overlap((start, end), (*old_start, *old_end)) {
                        return Err("proto reserved ranges overlap".to_string());
                    }
                }
                reserved.ranges.push((start, end));
            }
            if self.symbol_at(0, ',') {
                self.pos += 1;
                if self.symbol_at(0, ';') {
                    return Err("proto reserved list cannot end with a comma".to_string());
                }
                continue;
            }
            self.expect_symbol(';', "a reserved semicolon")?;
            return Ok(reserved);
        }
    }

    fn parse_extensions(&mut self) -> Result<(), String> {
        let _ = self.take_word("extensions")?;
        loop {
            let start = self.parse_integer("an extension number", false)?;
            let end = if self.word_at(0) == Some("to") {
                self.pos += 1;
                if self.word_at(0) == Some("max") {
                    self.pos += 1;
                    536_870_911
                } else {
                    self.parse_integer("an extension range end", false)?
                }
            } else {
                start
            };
            if start == 0 || start > end || end > 536_870_911 {
                return Err("proto extension range is out of range".to_string());
            }
            if proto_range_has_prohibited_number((start, end)) {
                return Err("proto extension range uses prohibited field numbers".to_string());
            }
            if self.symbol_at(0, ',') {
                self.pos += 1;
                continue;
            }
            self.expect_symbol(';', "an extensions semicolon")?;
            return Ok(());
        }
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
        if !is_proto_identifier(&wire_name) || is_proto_keyword(&wire_name) {
            return Err(format!("proto field name {wire_name} is invalid"));
        }
        self.expect_symbol('=', "an equals sign")?;
        let number = match self.tokens.get(self.pos).cloned() {
            Some(ProtoToken::Number(number)) => {
                self.pos += 1;
                if number.contains('.') || number.contains('e') || number.contains('E') {
                    return Err("proto field number must be an integer".to_string());
                }
                let value = parse_proto_integer_literal(&number)
                    .ok_or_else(|| "proto field number is invalid".to_string())?;
                u32::try_from(value).map_err(|_| "proto field number is invalid".to_string())?
            }
            _ => return Err("proto field number is missing".to_string()),
        };
        if number == 0 || number > 536_870_911 {
            return Err(format!("proto field number {number} is out of range"));
        }
        if (19_000..=19_999).contains(&number) {
            return Err(format!("proto field number {number} is prohibited"));
        }
        if self.symbol_at(0, '[') {
            self.parse_field_options()?;
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

    fn declare_message(&mut self, source_name: &str) -> Result<(), String> {
        if self.enum_names.contains(source_name) || !self.message_names.insert(source_name.to_string()) {
            return Err(format!("proto has duplicate declaration {source_name}"));
        }
        Ok(())
    }

    fn declare_enum(&mut self, source_name: &str) -> Result<(), String> {
        if self.message_names.contains(source_name) || !self.enum_names.insert(source_name.to_string()) {
            return Err(format!("proto has duplicate declaration {source_name}"));
        }
        Ok(())
    }

    fn full_name(&self, parent: Option<&str>, short_name: &str) -> String {
        if let Some(parent) = parent {
            format!("{parent}.{short_name}")
        } else if let Some(package) = &self.package {
            format!("{package}.{short_name}")
        } else {
            short_name.to_string()
        }
    }

    fn parse_message(&mut self, parent: Option<&str>) -> Result<String, String> {
        let _ = self.take_word("message")?;
        let short_name = self.take_word("a message name")?;
        if !is_proto_identifier(&short_name) || is_proto_keyword(&short_name) {
            return Err(format!("proto message name {short_name} is invalid"));
        }
        let source_name = self.full_name(parent, &short_name);
        self.declare_message(&source_name)?;
        self.expect_symbol('{', "a message block")?;
        let mut fields: Vec<ProtoField> = Vec::new();
        let mut field_names = BTreeSet::new();
        let mut field_numbers = BTreeSet::new();
        let mut nested_names = BTreeSet::new();
        let mut reserved = ProtoReserved::default();
        while self.pos < self.tokens.len() {
            if self.symbol_at(0, '}') {
                self.pos += 1;
                if field_names.iter().any(|name| nested_names.contains(name)) {
                    return Err("proto field collides with a nested message or enum".to_string());
                }
                if fields.iter().any(|field| {
                    reserved.names.contains(&field.wire_name)
                        || reserved.ranges.iter().any(|range| {
                            (range.0..=range.1).contains(&(field.number as i64))
                        })
                }) {
                    return Err("proto field uses a reserved name or number".to_string());
                }
                self.messages.push(ProtoMessage {
                    source_name: source_name.clone(),
                    short_name,
                    fields,
                });
                return Ok(source_name);
            }
            if self.word_at(0) == Some("message") {
                let nested = self.parse_message(Some(&source_name))?;
                nested_names.insert(nested.rsplit('.').next().unwrap_or(&nested).to_string());
                continue;
            }
            if self.word_at(0) == Some("enum") {
                let nested = self.parse_enum(Some(&source_name))?;
                nested_names.insert(nested.rsplit('.').next().unwrap_or(&nested).to_string());
                continue;
            }
            if self.word_at(0) == Some("reserved") {
                let parsed = self.parse_reserved(false, 536_870_911)?;
                for name in parsed.names {
                    if !reserved.names.insert(name) {
                        return Err("proto reserved name is repeated".to_string());
                    }
                }
                for range in parsed.ranges {
                    if reserved.ranges.iter().any(|(old_start, old_end)| {
                        proto_ranges_overlap(range, (*old_start, *old_end))
                    }) {
                        return Err("proto reserved ranges overlap".to_string());
                    }
                    reserved.ranges.push(range);
                }
                continue;
            }
            if self.word_at(0) == Some("extensions") {
                self.parse_extensions()?;
                continue;
            }
            if self.word_at(0) == Some("option") {
                self.parse_option_statement()?;
                continue;
            }
            if self.symbol_at(0, ';') {
                self.pos += 1;
                continue;
            }
            if matches!(self.word_at(0), Some("oneof") | Some("group") | Some("extend")) {
                return Err("proto oneof, group, and extend declarations are unsupported".to_string());
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

    fn parse_enum(&mut self, parent: Option<&str>) -> Result<String, String> {
        let _ = self.take_word("enum")?;
        let short_name = self.take_word("an enum name")?;
        if !is_proto_identifier(&short_name) || is_proto_keyword(&short_name) {
            return Err(format!("proto enum name {short_name} is invalid"));
        }
        let source_name = self.full_name(parent, &short_name);
        self.declare_enum(&source_name)?;
        self.expect_symbol('{', "an enum block")?;
        let mut names = BTreeSet::new();
        let mut numbers = BTreeSet::new();
        let mut reserved = ProtoReserved::default();
        let mut allow_alias = false;
        let mut values = Vec::new();
        while self.pos < self.tokens.len() {
            if self.symbol_at(0, '}') {
                self.pos += 1;
                if values.is_empty() {
                    return Err(format!("proto enum {source_name} has no values"));
                }
                if values.iter().any(|(name, number)| {
                    reserved.names.contains(name)
                        || reserved
                            .ranges
                            .iter()
                            .any(|range| (range.0..=range.1).contains(number))
                }) {
                    return Err("proto enum value uses a reserved name or number".to_string());
                }
                self.enums.push(ProtoEnum {
                    source_name: source_name.clone(),
                });
                return Ok(source_name);
            }
            if self.word_at(0) == Some("option") {
                let (name, value) = self.parse_option_statement()?;
                if name == "allow_alias" {
                    allow_alias = match value {
                        OptionValue::Bool(value) => value,
                        _ => return Err("proto enum allow_alias must be true or false".to_string()),
                    };
                }
                continue;
            }
            if self.word_at(0) == Some("reserved") {
                let parsed = self.parse_reserved(true, i32::MAX as i64)?;
                for name in parsed.names {
                    if !reserved.names.insert(name) {
                        return Err("proto enum reserved name is repeated".to_string());
                    }
                }
                for range in parsed.ranges {
                    if reserved.ranges.iter().any(|(old_start, old_end)| {
                        proto_ranges_overlap(range, (*old_start, *old_end))
                    }) {
                        return Err("proto enum reserved ranges overlap".to_string());
                    }
                    reserved.ranges.push(range);
                }
                continue;
            }
            if self.symbol_at(0, ';') {
                self.pos += 1;
                continue;
            }
            let value_name = self.take_word("an enum value name")?;
            if !is_proto_identifier(&value_name) || is_proto_keyword(&value_name) {
                return Err(format!("proto enum value name {value_name} is invalid"));
            }
            if value_name == short_name || !names.insert(value_name.clone()) {
                return Err(format!("proto enum has a duplicate or colliding value {value_name}"));
            }
            self.expect_symbol('=', "an enum value equals sign")?;
            let number = self.parse_integer("an enum value number", true)?;
            if !(-2_147_483_648..=2_147_483_647).contains(&number) {
                return Err("proto enum value number is out of int32 range".to_string());
            }
            if (19_000..=19_999).contains(&number) {
                return Err("proto enum value uses a prohibited number".to_string());
            }
            if !allow_alias && !numbers.insert(number) {
                return Err(format!("proto enum has duplicate value number {number}"));
            }
            if self.symbol_at(0, '[') {
                self.parse_field_options()?;
            }
            self.expect_symbol(';', "an enum value semicolon")?;
            values.push((value_name, number));
        }
        Err(format!("proto enum {source_name} is not closed"))
    }

    fn parse_syntax(&mut self) -> Result<(), String> {
        let _ = self.take_word("syntax")?;
        if self.syntax_seen {
            return Err("proto has more than one syntax declaration".to_string());
        }
        self.expect_symbol('=', "a syntax equals sign")?;
        let syntax = self.take_string("a syntax string")?;
        if !matches!(syntax.as_str(), "proto2" | "proto3") {
            return Err(format!("proto syntax {syntax:?} is unsupported"));
        }
        self.expect_symbol(';', "a syntax semicolon")?;
        self.syntax_seen = true;
        Ok(())
    }

    fn parse_package(&mut self) -> Result<(), String> {
        let _ = self.take_word("package")?;
        if self.package.is_some() {
            return Err("proto has more than one package declaration".to_string());
        }
        let package = self.parse_qualified_name("a package name")?;
        if package.split('.').any(|part| !is_proto_identifier(part)) {
            return Err(format!("proto package name {package} is invalid"));
        }
        self.expect_symbol(';', "a package semicolon")?;
        self.package = Some(package);
        Ok(())
    }

    fn parse_import(&mut self) -> Result<(), String> {
        let _ = self.take_word("import")?;
        if matches!(self.word_at(0), Some("public") | Some("weak")) {
            self.pos += 1;
        }
        let path = self.take_string("an import path")?;
        if path.is_empty() || path.chars().any(char::is_control) {
            return Err("proto import path is invalid".to_string());
        }
        self.expect_symbol(';', "an import semicolon")?;
        Ok(())
    }

    fn parse(mut self) -> Result<ProtoSchema, String> {
        while self.pos < self.tokens.len() {
            if self.word_at(0) == Some("syntax") {
                if self.body_started {
                    return Err("proto syntax declaration must precede messages and enums".to_string());
                }
                self.parse_syntax()?;
            } else if self.word_at(0) == Some("package") {
                if self.body_started {
                    return Err("proto package declaration must precede messages and enums".to_string());
                }
                self.parse_package()?;
            } else if self.word_at(0) == Some("import") {
                if self.body_started {
                    return Err("proto import declaration must precede messages and enums".to_string());
                }
                self.parse_import()?;
            } else if self.word_at(0) == Some("option") {
                if self.body_started {
                    return Err("proto file options must precede messages and enums".to_string());
                }
                self.parse_option_statement()?;
            } else if self.word_at(0) == Some("message") {
                self.body_started = true;
                self.parse_message(None)?;
            } else if self.word_at(0) == Some("enum") {
                self.body_started = true;
                self.parse_enum(None)?;
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
        Ok(ProtoSchema {
            messages: self.messages,
            enums: self.enums,
        })
    }
}

#[derive(Clone, Debug)]
enum OptionValue {
    Bool(bool),
    String,
    Number,
    Identifier,
    Aggregate,
}

fn is_proto_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(ch) if ch.is_ascii_alphabetic() || ch == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn is_proto_keyword(value: &str) -> bool {
    matches!(
        value,
        "syntax"
            | "package"
            | "import"
            | "option"
            | "message"
            | "enum"
            | "service"
            | "rpc"
            | "returns"
            | "stream"
            | "reserved"
            | "extensions"
            | "extend"
            | "oneof"
            | "group"
            | "required"
            | "optional"
            | "repeated"
            | "double"
            | "float"
            | "int32"
            | "sint32"
            | "sfixed32"
            | "int64"
            | "sint64"
            | "sfixed64"
            | "uint32"
            | "uint64"
            | "fixed32"
            | "fixed64"
            | "bool"
            | "string"
            | "bytes"
            | "to"
            | "max"
            | "public"
            | "weak"
    )
}

fn proto_scalar(type_name: &str) -> Option<&'static str> {
    match type_name.trim_start_matches('.').to_ascii_lowercase().as_str() {
        "double" | "float" => Some("Float"),
        "int32" | "sint32" | "sfixed32" | "int64" | "sint64" | "sfixed64" | "uint32"
        | "uint64" | "fixed32" | "fixed64" => Some("Int"),
        "bool" => Some("Bool"),
        "string" => Some("String"),
        "bytes" => Some("[U8]"),
        "google.protobuf.timestamp" => Some("DateTime"),
        "google.protobuf.duration" => Some("Duration"),
        "google.protobuf.stringvalue" => Some("String"),
        "google.protobuf.bytesvalue" => Some("[U8]"),
        "google.protobuf.int32value" | "google.protobuf.int64value" => Some("Int"),
        "google.protobuf.boolvalue" => Some("Bool"),
        "google.protobuf.doublevalue" | "google.protobuf.floatvalue" => Some("Float"),
        _ => None,
    }
}

fn bind_proto(input: &str, root_name: Option<&str>) -> Result<SchemaBuilder, String> {
    let schema = ProtoParser::new(input)?.parse()?;
    let messages = schema.messages;
    let enum_names: BTreeSet<String> = schema
        .enums
        .iter()
        .map(|item| item.source_name.clone())
        .collect();
    let mut builder = SchemaBuilder::new();
    let use_root = messages.len() == 1;
    let mut source_names = BTreeMap::new();
    let mut leaf_names = BTreeMap::new();
    let mut ambiguous_leaves = BTreeSet::new();
    let mut enum_leaf_names = BTreeMap::new();
    let mut ambiguous_enum_leaves = BTreeSet::new();
    for enum_name in &enum_names {
        let leaf = enum_name.rsplit('.').next().unwrap_or(enum_name);
        if enum_leaf_names
            .insert(leaf.to_string(), enum_name.clone())
            .is_some()
        {
            ambiguous_enum_leaves.insert(leaf.to_string());
        }
    }
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
                bound_scalar(scalar)
            } else {
                let reference = field.type_name.trim_start_matches('.');
                let leaf = reference.rsplit('.').next().unwrap_or(reference);
                if enum_names.contains(reference)
                    || (!ambiguous_enum_leaves.contains(leaf)
                        && enum_leaf_names.contains_key(leaf))
                {
                    BoundType::Scalar("Int")
                } else {
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
                                "proto field {} references unknown message or enum type {}",
                                field.wire_name, field.type_name
                            )
                        })?;
                    BoundType::Named(resolved.clone())
                }
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
            "NOT NULL and PRIMARY KEY columns stay required; supported constraints are validated.",
        ],
        "xml" => &[
            "XML attributes use @ wire keys; simple content uses $text and mixed content keeps ordered $content as DataTree.",
            "Nested elements become records; repeated child elements become arrays.",
        ],
        "proto" => &[
            "Protobuf message blocks become #Codable records; scalar types use Jet core types.",
            "repeated and optional labels map to lists, enums map to Int, and field numbers stay in comments.",
        ],
        _ => &["The input was parsed as a named data schema."],
    }
}

fn escape_provenance(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| match ch {
            '\\' => "\\\\".chars().collect::<Vec<_>>(),
            '\n' => "\\n".chars().collect(),
            '\r' => "\\r".chars().collect(),
            '\t' => "\\t".chars().collect(),
            ch if ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}') => {
                format!("\\u{:04x}", ch as u32).chars().collect()
            }
            ch => vec![ch],
        })
        .collect()
}

fn quote_jet_string(value: &str) -> String {
    crate::JSON::quote(value)
        .replace('{', "{{")
        .replace('}', "}}")
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
        format!("jet inspect bind {format} {input_path}")
    } else {
        command.to_string()
    };
    let _ = writeln!(source, "// generated by: {}", escape_provenance(&command));
    let _ = writeln!(source, "// input: {}", escape_provenance(input_path));
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
                    quote_jet_string(&field.wire_name),
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
/// command is recorded losslessly in the stable provenance header; line and
/// control characters are escaped so the generated comment cannot be injected.
/// Callers should pass the exact user-facing command line, without shell quoting.
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
