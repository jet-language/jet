//! Native C-header → Jet `@bindgen` cache generator (E2-M14).
//!
//! Owner 2026-06-18: this supersedes the D-CBIND3=B `bindgen` route. The shipped
//! `jet` stays std-only (I6) — no `bindgen`, no libclang. This is a focused
//! parser for **C function prototypes** over the type subset Jet's FFI binds
//! (scalars, `char*` strings, `void`). A declaration it cannot map is *skipped
//! and reported* — never faked (I3). Anything beyond this subset is hand-written
//! as an `@extern module c.<lib>` overlay, which still wins on merge.
//!
//! Output is a `@bindgen module c.<lib>.__bindgen__ { … }` cache as parsed by
//! `src/cffi.rs`; each binding is `fn name(p: T, …) [-> R] = "c_symbol";`.

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
    /// The `@bindgen module …` cache file text.
    pub source: String,
    /// Function names successfully bound.
    pub bound: Vec<String>,
    /// `(name, reason)` for prototypes skipped because a type isn't bindable.
    pub skipped: Vec<(String, String)>,
}

/// Translate C header source into a `@bindgen` cache for library `lib`.
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

    let source = format!("#bindgen module c.{}.__bindgen__ {{\n{}}}\n", lib, lines);
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
    if close < open {
        return None;
    }
    let before = decl[..open].trim();
    let params_src = decl[open + 1..close].trim();

    // The function name is the last identifier in `before`; everything earlier
    // is the return type. Strip a leading `*` that belongs to the return type.
    let name_start = before
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .map(|i| i + 1)
        .unwrap_or(0);
    let name = &before[name_start..];
    if name.is_empty() || !name.chars().next().unwrap().is_ascii_alphabetic() && name != "_" {
        return None;
    }
    if !is_ident(name) {
        return None;
    }
    let ret = before[..name_start].trim().to_string();
    if ret.is_empty() {
        // No return type at all (e.g. a bare `foo(x)` K&R decl) — not in scope.
        return None;
    }

    let params = split_params(params_src);
    Some(Proto {
        ret,
        name: name.to_string(),
        params,
    })
}

/// Split a parameter list on top-level commas (no nested parens in this subset).
fn split_params(src: &str) -> Vec<String> {
    let s = src.trim();
    if s.is_empty() || s == "void" {
        return Vec::new();
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
                cur.push(ch);
            }
            ',' if depth == 0 => out.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// Render one prototype as a Jet `@bindgen` line, or Err(reason) if a type in it
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
        Some(r) => format!("fn {}({}) -> {} = \"{}\";", p.name, params_str, r, p.name),
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(r.source.contains("#bindgen module c.jetc.__bindgen__ {"));
        assert!(r
            .source
            .contains("fn jetc_add(a: Int, b: Int) -> Int = \"jetc_add\";"));
        assert!(r
            .source
            .contains("fn scale(x: Float, k: Float) -> Float = \"scale\";"));
        assert!(r.source.contains("fn reset() = \"reset\";"));
        assert!(r
            .source
            .contains("fn name_of(id: Int) -> String = \"name_of\";"));
        assert!(r.source.contains("fn is_ready() -> Bool = \"is_ready\";"));
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
        assert!(r.source.contains("fn ok(x: Int) -> Int = \"ok\";"));
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
            .contains("fn f(arg0: Int, arg1: Float) -> Int = \"f\";"));
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
                .contains("fn add_u32(a: Int, b: Int) -> Int = \"add_u32\";"),
            "uint32_t must map to Int: got:\n{}",
            r.source
        );
        assert!(
            r.source
                .contains("fn sub_i32(a: Int, b: Int) -> Int = \"sub_i32\";"),
            "int32_t must map to Int: got:\n{}",
            r.source
        );
        assert!(
            r.source
                .contains("fn identity_u64(x: Int) -> Int = \"identity_u64\";"),
            "uint64_t must map to Int: got:\n{}",
            r.source
        );
        assert_eq!(r.bound, vec!["add_u32", "sub_i32", "identity_u64"]);
    }
}
