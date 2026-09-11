//! Opaque C typedef discovery for the C binding generator.
//!
//! This module deliberately stops at a source/model boundary. It records the
//! ownership, thread-safety, and link-closure facts that a later compiler seam
//! can lower into ordinary Jet handle types. It never turns an unknown pointer
//! into an integer, and it never infers thread safety or transitive libraries.

use std::collections::{BTreeMap, BTreeSet};

const CLOSE_VERBS: &[&str] = &[
    "close",
    "destroy",
    "free",
    "release",
    "unref",
    "deinit",
    "fini",
    "finalize",
    "dispose",
];

/// Thread-safety is an explicit overlay fact. `Unspecified` is intentional:
/// the binder must not infer a concurrency guarantee from C spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadSafety {
    Unspecified,
    Safe,
    Unsafe,
}

impl Default for ThreadSafety {
    fn default() -> Self {
        Self::Unspecified
    }
}

/// Why a handle's close symbol was selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseSource {
    Conventional,
    Overlay,
}

/// Native libraries that must remain attached to one generated binding.
///
/// `primary` is the library named by the C import. `transitive` contains only
/// libraries explicitly supplied by the overlay; the binder never guesses
/// dependencies from an include or a symbol name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkClosure {
    pub primary: String,
    pub transitive: Vec<String>,
}

impl LinkClosure {
    pub(crate) fn new(primary: &str, transitive: &[String]) -> Self {
        let mut seen = BTreeSet::new();
        let mut links = Vec::new();
        for link in transitive {
            if link == primary || !seen.insert(link.clone()) {
                continue;
            }
            links.push(link.clone());
        }
        links.sort();
        Self {
            primary: primary.to_string(),
            transitive: links,
        }
    }

    /// Return the complete deterministic link list, primary first.
    pub fn libraries(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(1 + self.transitive.len());
        out.push(self.primary.clone());
        out.extend(self.transitive.iter().cloned());
        out
    }
}

/// Explicit metadata supplied by a package's C binding overlay.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HandleOverlay {
    /// Map a C typedef, its generated Jet name, or an opaque struct tag to a
    /// close symbol. This is required when conventional discovery is absent or
    /// ambiguous.
    pub close_functions: BTreeMap<String, String>,
    /// Per-handle concurrency declarations. Missing entries stay unspecified.
    pub thread_safety: BTreeMap<String, ThreadSafety>,
    /// Explicit transitive libraries needed by this binding.
    pub links: Vec<String>,
}

impl HandleOverlay {
    /// Add an explicit close mapping while retaining builder-style use.
    pub fn with_close(mut self, handle: impl Into<String>, symbol: impl Into<String>) -> Self {
        self.close_functions.insert(handle.into(), symbol.into());
        self
    }

    /// Add an explicit thread-safety declaration while retaining builder-style use.
    pub fn with_thread_safety(mut self, handle: impl Into<String>, safety: ThreadSafety) -> Self {
        self.thread_safety.insert(handle.into(), safety);
        self
    }

    /// Add one explicit transitive library while retaining builder-style use.
    pub fn with_link(mut self, library: impl Into<String>) -> Self {
        self.links.push(library.into());
        self
    }
}

/// A generated owned handle fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandleFact {
    /// The C typedef spelling that introduced the opaque pointer.
    pub typedef_name: String,
    /// The generated nominal Jet type name.
    pub jet_name: String,
    /// The native close function. Every fact in this collection is owned, so a
    /// close symbol is mandatory; unresolved handles remain in diagnostics.
    pub close: String,
    pub close_source: CloseSource,
    pub thread_safety: ThreadSafety,
    pub link_closure: LinkClosure,
}

impl HandleFact {
    /// Alias for callers that use the more explicit C terminology.
    pub fn c_typedef(&self) -> &str {
        &self.typedef_name
    }

    /// Alias for callers that want the native close symbol by its role.
    pub fn close_symbol(&self) -> &str {
        &self.close
    }
}

/// Internal discovery result consumed by `CBind`.
#[derive(Debug, Clone)]
pub(crate) struct Discovery {
    pub handles: Vec<HandleFact>,
    /// Opaque typedefs that cannot safely become owned handles. The reason is
    /// retained so CBind can report it without pretending the type is scalar.
    pub unresolved: Vec<(String, String)>,
    opaque: BTreeMap<String, OpaqueShape>,
    handle_indices: BTreeMap<String, usize>,
}

#[derive(Debug, Clone)]
struct ResolvedAlias {
    tag: String,
    pointer_depth: usize,
}

#[derive(Debug, Clone)]
struct OpaqueShape {
    tag: String,
    pointer_depth: usize,
}

#[derive(Debug, Clone)]
struct AliasDecl {
    base: AliasBase,
    pointer_depth: usize,
}

#[derive(Debug, Clone)]
enum AliasBase {
    Struct(String),
    Alias(String),
    Opaque,
    Other,
}

#[derive(Debug, Clone)]
struct Proto {
    name: String,
    params: Vec<String>,
}

#[derive(Debug, Clone)]
struct TypeShape {
    base: Option<String>,
    tag: Option<String>,
    pointer_depth: usize,
}

#[derive(Debug, Clone)]
struct StructTags {
    complete: BTreeSet<String>,
}

impl Discovery {
    pub(crate) fn handle_for_type(&self, c_type: &str) -> Option<&HandleFact> {
        self.opaque.iter().find_map(|(alias, shape)| {
            if !matches_shape(c_type, alias, shape) {
                return None;
            }
            let index = self.handle_indices.get(alias)?;
            self.handles.get(*index)
        })
    }

    pub(crate) fn unresolved_reason_for_type(&self, c_type: &str) -> Option<&str> {
        self.opaque.iter().find_map(|(alias, shape)| {
            if !matches_shape(c_type, alias, shape) {
                return None;
            }
            self.unresolved
                .iter()
                .find(|(name, _)| name == alias)
                .map(|(_, reason)| reason.as_str())
        })
    }

    pub(crate) fn is_close_function(&self, function: &str, c_type: &str) -> bool {
        self.handle_for_type(c_type)
            .is_some_and(|handle| handle.close == function)
    }

    pub(crate) fn render_type_declarations(&self) -> String {
        let mut out = String::new();
        for handle in &self.handles {
            out.push_str("pub struct ");
            out.push_str(&handle.jet_name);
            out.push_str(" {}\n");
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out
    }
}

/// Discover opaque struct-pointer typedefs and resolve their ownership facts.
///
/// `source` may be either the original header or CBind's already-cleaned
/// declaration stream. Unsupported declarations are ignored here and remain
/// CBind's responsibility.
pub(crate) fn discover(source: &str, lib: &str, overlay: &HandleOverlay) -> Discovery {
    let cleaned = strip_comments_and_directives(source);
    let declarations = split_declarations(&cleaned);
    let tags = collect_struct_tags(&declarations);
    let alias_decls = declarations
        .iter()
        .filter_map(|decl| parse_typedef(decl))
        .collect::<BTreeMap<_, _>>();

    let mut aliases = BTreeMap::new();
    for name in alias_decls.keys() {
        let mut visiting = BTreeSet::new();
        if let Some(alias) = resolve_alias(name, &alias_decls, &tags, &mut visiting) {
            aliases.insert(name.clone(), alias);
        }
    }

    let opaque = aliases
        .iter()
        .filter(|(_, alias)| alias.pointer_depth <= 1 && !tags.complete.contains(&alias.tag))
        .map(|(name, alias)| {
            (
                name.clone(),
                OpaqueShape {
                    tag: alias.tag.clone(),
                    pointer_depth: alias.pointer_depth,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    let protos = declarations
        .iter()
        .filter_map(|decl| parse_prototype(decl))
        .collect::<Vec<_>>();
    let mut used_names = BTreeSet::new();
    let mut handles = Vec::new();
    let mut unresolved = Vec::new();
    let links = LinkClosure::new(lib, &overlay.links);

    for (typedef_name, shape) in &opaque {
        let candidates = protos
            .iter()
            .filter(|proto| proto.params.len() == 1)
            .filter(|proto| matches_shape(&proto.params[0], typedef_name, shape))
            .filter(|proto| conventional_close_name(&proto.name, typedef_name, &shape.tag))
            .map(|proto| proto.name.clone())
            .collect::<Vec<_>>();
        let jet_name = unique_name(
            &mut used_names,
            &crate::Syntax::sanitize_generated_name(
                handle_name_seed(typedef_name),
                crate::Syntax::NameCase::Pascal,
                "Handle",
            ),
        );
        let overlay_close = overlay_close(overlay, typedef_name, &jet_name, &shape.tag);
        let (close, close_source) = match overlay_close {
            Some(symbol) if is_ident(&symbol) => (symbol, CloseSource::Overlay),
            Some(symbol) => {
                unresolved.push((
                    typedef_name.clone(),
                    format!(
                        "overlay close symbol `{symbol}` for opaque handle `{typedef_name}` is not a valid C identifier"
                    ),
                ));
                continue;
            }
            None if candidates.len() == 1 => (candidates[0].clone(), CloseSource::Conventional),
            None if candidates.is_empty() => {
                unresolved.push((
                    typedef_name.clone(),
                    format!(
                        "opaque handle `{typedef_name}` has no conventional close function; provide an overlay close mapping"
                    ),
                ));
                continue;
            }
            None => {
                unresolved.push((
                    typedef_name.clone(),
                    format!(
                        "opaque handle `{typedef_name}` has ambiguous conventional close functions ({}); provide an overlay close mapping",
                        candidates.join(", ")
                    ),
                ));
                continue;
            }
        };
        let safety = overlay_thread_safety(overlay, typedef_name, &jet_name, &shape.tag);
        handles.push(HandleFact {
            typedef_name: typedef_name.clone(),
            jet_name,
            close,
            close_source,
            thread_safety: safety,
            link_closure: links.clone(),
        });
    }

    let handle_indices = handles
        .iter()
        .enumerate()
        .map(|(index, handle)| (handle.typedef_name.clone(), index))
        .collect();
    Discovery {
        handles,
        unresolved,
        opaque,
        handle_indices,
    }
}

fn handle_name_seed(typedef_name: &str) -> &str {
    typedef_name
        .strip_suffix("_t")
        .filter(|name| !name.is_empty())
        .unwrap_or(typedef_name)
}

fn overlay_close(
    overlay: &HandleOverlay,
    typedef_name: &str,
    jet_name: &str,
    tag: &str,
) -> Option<String> {
    [typedef_name, jet_name, tag]
        .into_iter()
        .find_map(|key| overlay.close_functions.get(key).cloned())
}

fn overlay_thread_safety(
    overlay: &HandleOverlay,
    typedef_name: &str,
    jet_name: &str,
    tag: &str,
) -> ThreadSafety {
    [typedef_name, jet_name, tag]
        .into_iter()
        .find_map(|key| overlay.thread_safety.get(key).copied())
        .unwrap_or_default()
}

fn conventional_close_name(function: &str, typedef_name: &str, tag: &str) -> bool {
    let mut stems = BTreeSet::new();
    for raw in [typedef_name, tag] {
        let raw = raw.trim_start_matches('_').trim_end_matches("_t");
        if raw.is_empty() {
            continue;
        }
        let lower = raw.to_ascii_lowercase();
        stems.insert(lower.clone());
        let snake = camel_to_snake(raw);
        stems.insert(snake.clone());
        let parts = snake.split('_').collect::<Vec<_>>();
        for end in 1..=parts.len() {
            stems.insert(parts[..end].join("_"));
        }
    }
    let function = function.to_ascii_lowercase();
    stems.into_iter().any(|stem| {
        CLOSE_VERBS.iter().any(|verb| {
            function == format!("{stem}{verb}")
                || function == format!("{stem}_{verb}")
                || function == format!("{verb}_{stem}")
        })
    })
}

fn camel_to_snake(raw: &str) -> String {
    let mut out = String::new();
    let mut previous_lower = false;
    for ch in raw.chars() {
        if ch.is_ascii_uppercase() && previous_lower && !out.ends_with('_') {
            out.push('_');
        }
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            previous_lower = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        } else {
            if !out.ends_with('_') {
                out.push('_');
            }
            previous_lower = false;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    out
}

fn matches_shape(raw: &str, alias: &str, shape: &OpaqueShape) -> bool {
    let actual = parse_type_shape(parameter_type(raw));
    let Some(base) = actual.base.as_deref() else {
        return false;
    };
    if actual.tag.as_deref() == Some(shape.tag.as_str()) {
        return actual.pointer_depth == 1;
    }
    if base != alias {
        return false;
    }
    // A typedef that already names a pointer is written without another `*`;
    // a typedef naming an incomplete struct is written with exactly one `*`.
    let expected_pointer_depth = if shape.pointer_depth == 0 { 1 } else { 0 };
    actual.pointer_depth == expected_pointer_depth
}

/// Remove the declarator name before comparing a prototype parameter's C type.
///
/// Opaque discovery only needs the type shape. Keeping the name in the token
/// stream would make `gzFile file` look like a type named `file` and would
/// prevent both conventional close discovery and borrowed handle matching.
fn parameter_type(raw: &str) -> &str {
    let raw = raw.trim();
    let Some(pos) = raw.rfind(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_')) else {
        return raw;
    };
    let tail = &raw[pos + 1..];
    let separator = raw.as_bytes()[pos];
    if is_ident(tail) && (separator == b'*' || separator.is_ascii_whitespace()) {
        let ty = raw[..pos + 1].trim();
        if !ty.is_empty() {
            return ty;
        }
    }
    raw
}

fn resolve_alias(
    name: &str,
    aliases: &BTreeMap<String, AliasDecl>,
    tags: &StructTags,
    visiting: &mut BTreeSet<String>,
) -> Option<ResolvedAlias> {
    if !visiting.insert(name.to_string()) {
        return None;
    }
    let decl = aliases.get(name)?;
    let result = match &decl.base {
        AliasBase::Struct(tag) if !tags.complete.contains(tag) => Some(ResolvedAlias {
            tag: tag.clone(),
            pointer_depth: decl.pointer_depth,
        }),
        AliasBase::Opaque => Some(ResolvedAlias {
            tag: name.to_string(),
            pointer_depth: decl.pointer_depth,
        }),
        AliasBase::Struct(_) | AliasBase::Other => None,
        AliasBase::Alias(other) => {
            let mut resolved = resolve_alias(other, aliases, tags, visiting)?;
            resolved.pointer_depth += decl.pointer_depth;
            Some(resolved)
        }
    };
    visiting.remove(name);
    result
}

fn collect_struct_tags(declarations: &[String]) -> StructTags {
    let mut complete = BTreeSet::new();
    for decl in declarations {
        let tokens = tokenize(decl);
        for index in 0..tokens.len().saturating_sub(2) {
            if tokens[index] != "struct" {
                continue;
            }
            let Some(tag) = tokens.get(index + 1).filter(|token| is_ident(token)) else {
                continue;
            };
            if tokens[index + 2..].iter().any(|token| token == "{") {
                complete.insert(tag.clone());
            }
        }
    }
    StructTags { complete }
}

fn parse_typedef(decl: &str) -> Option<(String, AliasDecl)> {
    let tokens = tokenize(decl);
    if tokens.first().map(String::as_str) != Some("typedef") {
        return None;
    }
    // Function-pointer typedefs need a declarator parser, not a guessed handle.
    if tokens.iter().skip(1).any(|token| token == "(") {
        return None;
    }
    let alias_pos = tokens.iter().rposition(|token| is_ident(token))?;
    let alias = tokens[alias_pos].clone();
    let pointer_depth = tokens[1..alias_pos]
        .iter()
        .filter(|token| token.as_str() == "*")
        .count();
    let type_tokens = &tokens[1..alias_pos];
    let base = if let Some(struct_pos) = type_tokens.iter().position(|token| token == "struct") {
        match type_tokens.get(struct_pos + 1) {
            Some(tag) if is_ident(tag) => AliasBase::Struct(tag.clone()),
            _ => AliasBase::Other,
        }
    } else if pointer_depth > 0
        && type_tokens.iter().any(|token| token == "void")
        && type_tokens.iter().all(|token| {
            matches!(token.as_str(), "void" | "const" | "volatile" | "restrict" | "*")
        })
    {
        AliasBase::Opaque
    } else if type_tokens.len() == 1 && is_ident(&type_tokens[0]) {
        AliasBase::Alias(type_tokens[0].clone())
    } else {
        AliasBase::Other
    };
    Some((
        alias,
        AliasDecl {
            base,
            pointer_depth,
        },
    ))
}

fn parse_prototype(decl: &str) -> Option<Proto> {
    if decl.contains('{') || decl.trim_start().starts_with("typedef") {
        return None;
    }
    let open = decl.find('(')?;
    let close = decl.rfind(')')?;
    if close < open || !decl[close + 1..].trim().is_empty() {
        return None;
    }
    let before = decl[..open].trim();
    if before.contains('(') {
        return None;
    }
    let name_start = before
        .char_indices()
        .rev()
        .find(|(_, ch)| !(ch.is_ascii_alphanumeric() || *ch == '_'))
        .map(|(index, ch)| index + ch.len_utf8())
        .unwrap_or(0);
    let name = &before[name_start..];
    if !is_ident(name) {
        return None;
    }
    let ret = before[..name_start].trim();
    if ret.is_empty() {
        return None;
    }
    Some(Proto {
        name: name.to_string(),
        params: split_params(decl[open + 1..close].trim())?,
    })
}

fn split_params(src: &str) -> Option<Vec<String>> {
    if src.is_empty() || src == "void" {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    for ch in src.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            ',' if depth == 0 => {
                if cur.trim().is_empty() {
                    return None;
                }
                out.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(ch);
    }
    if depth != 0 || cur.trim().is_empty() {
        return None;
    }
    out.push(cur);
    Some(out)
}

fn parse_type_shape(raw: &str) -> TypeShape {
    let tokens = tokenize(raw);
    let mut pointer_depth = 0;
    let mut tag = None;
    let mut base = None;
    let mut previous_struct = false;
    for token in tokens {
        match token.as_str() {
            "*" => pointer_depth += 1,
            "const" | "volatile" | "restrict" | "register" | "extern" | "unsigned"
            | "signed" | "long" | "short" => {}
            "struct" => previous_struct = true,
            value if is_ident(value) => {
                if previous_struct {
                    tag = Some(value.to_string());
                    base = Some(value.to_string());
                    previous_struct = false;
                } else {
                    base = Some(value.to_string());
                }
            }
            _ => {}
        }
    }
    TypeShape {
        base,
        tag,
        pointer_depth,
    }
}

fn tokenize(src: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in src.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch);
            continue;
        }
        if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
        if matches!(ch, '*' | '(' | ')' | '{' | '}' | '[' | ']') {
            tokens.push(ch.to_string());
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn split_declarations(src: &str) -> Vec<String> {
    let mut declarations = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    for ch in src.chars() {
        match ch {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
        current.push(ch);
        if ch == ';' && depth == 0 {
            current.pop();
            declarations.push(std::mem::take(&mut current));
        }
    }
    declarations
}

fn strip_comments_and_directives(src: &str) -> String {
    let mut comments_removed = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'/' && index + 1 < bytes.len() && bytes[index + 1] == b'*' {
            index += 2;
            while index + 1 < bytes.len()
                && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
            {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
            comments_removed.push(' ');
        } else if bytes[index] == b'/' && index + 1 < bytes.len() && bytes[index + 1] == b'/' {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
        } else {
            comments_removed.push(bytes[index] as char);
            index += 1;
        }
    }
    let mut out = String::with_capacity(comments_removed.len());
    let mut continued = false;
    for line in comments_removed.lines() {
        let trimmed = line.trim_start();
        if continued || trimmed.starts_with('#') {
            continued = line.trim_end().ends_with('\\');
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn unique_name(used: &mut BTreeSet<String>, preferred: &str) -> String {
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

fn is_ident(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_zlib_style_typedef_and_close() {
        let overlay = HandleOverlay::default();
        let discovered = discover(
            "typedef struct gzFile_s *gzFile; gzFile gzopen(const char *path, const char *mode); int gzclose(gzFile file);",
            "zlib",
            &overlay,
        );
        assert_eq!(discovered.handles.len(), 1);
        let handle = &discovered.handles[0];
        assert_eq!(handle.typedef_name, "gzFile");
        assert_eq!(handle.jet_name, "GzFile");
        assert_eq!(handle.close, "gzclose");
        assert_eq!(handle.close_source, CloseSource::Conventional);
    }

    #[test]
    fn discovers_harfbuzz_style_forward_struct() {
        let discovered = discover(
            "typedef struct hb_buffer_t hb_buffer_t; hb_buffer_t *hb_buffer_create(void); void hb_buffer_destroy(hb_buffer_t *buffer);",
            "harfbuzz",
            &HandleOverlay::default(),
        );
        assert_eq!(discovered.handles[0].jet_name, "HbBuffer");
        assert_eq!(discovered.handles[0].close, "hb_buffer_destroy");
    }

    #[test]
    fn ambiguous_close_requires_overlay() {
        let source = "typedef struct thing_t thing_t; thing_t *thing_create(void); void thing_destroy(thing_t *x); void thing_free(thing_t *x);";
        let unresolved = discover(source, "thing", &HandleOverlay::default());
        assert!(unresolved.handles.is_empty());
        assert!(unresolved.unresolved[0].1.contains("ambiguous"));
        let overlay = HandleOverlay::default().with_close("thing_t", "thing_destroy");
        let resolved = discover(source, "thing", &overlay);
        assert_eq!(resolved.handles[0].close, "thing_destroy");
        assert_eq!(resolved.handles[0].close_source, CloseSource::Overlay);
    }

    #[test]
    fn no_thread_or_link_fact_is_inferred() {
        let discovered = discover(
            "typedef struct thing_t thing_t; thing_t *thing_new(void); void thing_destroy(thing_t *x);",
            "thing",
            &HandleOverlay::default(),
        );
        assert_eq!(discovered.handles[0].thread_safety, ThreadSafety::Unspecified);
        assert_eq!(discovered.handles[0].link_closure.libraries(), vec!["thing"]);
    }
}
