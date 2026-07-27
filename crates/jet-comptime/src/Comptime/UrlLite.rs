//! Card #392 gap fix: `core.url` (D-URL1=A) ported verbatim from AOT's
//! hand-rolled RFC-3986-shaped parser
//! (`crates/jet-codegen/src/Prelude/CoreLib/JetStd/UrlMime.rs::JetURL` +
//! `jet_url_*` free functions) so comptime/REPL tier-0 matches AOT
//! byte-for-byte (R12 parity). Only the module-level free functions in
//! `fixed_sigs.rs`'s `"core.url"` table are ported here (`parse`/
//! `from_parts`/`file`/`data`/`query`/`percent_encode`/`percent_decode`) —
//! `Url` instance methods (`.scheme()`/`.host()`/...) aren't in that table;
//! they read struct fields generically (`Interpreter.rs`'s member-access
//! path), which already works once a `CtValue::Struct { type_name: "Url",
//! .. }` exists with matching field names.

/// Plain-data mirror of AOT's `JetURL` struct — same shape, same field
/// names, so `to_ct_value`/`from_ct_value` in `Methods.rs` map 1:1 onto a
/// `CtValue::Struct { type_name: "Url", .. }`.
pub(super) struct UrlParts {
    pub scheme: String,
    pub host: Option<String>,
    pub port: Option<i64>,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub fragment: Option<String>,
}

impl UrlParts {
    pub(super) fn parse(input: &str) -> Result<Self, String> {
        let raw = input.trim();
        let Some(colon) = raw.find(':') else {
            return Err("URL needs a scheme".to_string());
        };
        let scheme_raw = &raw[..colon];
        if !url_valid_scheme(scheme_raw) {
            return Err(format!("invalid URL scheme `{}`", scheme_raw));
        }
        let scheme = scheme_raw.to_ascii_lowercase();
        let mut rest = &raw[colon + 1..];
        let mut fragment = None;
        if let Some(i) = rest.find('#') {
            fragment = Some(url_percent_decode_str(&rest[i + 1..])?);
            rest = &rest[..i];
        }
        let mut query = Vec::new();
        if let Some(i) = rest.find('?') {
            query = url_parse_query(&rest[i + 1..])?;
            rest = &rest[..i];
        }
        let mut host = None;
        let mut port = None;
        let path;
        if let Some(after_slashes) = rest.strip_prefix("//") {
            let auth_end = after_slashes.find('/').unwrap_or(after_slashes.len());
            let authority = &after_slashes[..auth_end];
            let path_raw = &after_slashes[auth_end..];
            let (h, p) = url_parse_authority(authority)?;
            host = Some(h);
            port = p;
            path = if path_raw.is_empty() {
                "/".to_string()
            } else {
                url_percent_decode_str(path_raw)?
            };
        } else if matches!(scheme.as_str(), "http" | "https") {
            return Err(format!("{} URL needs `//host`", scheme));
        } else if scheme == "data" {
            path = rest.to_string();
        } else {
            path = url_percent_decode_str(rest)?;
        }
        let url = UrlParts {
            scheme,
            host,
            port,
            path,
            query,
            fragment,
        };
        Ok(url.normalize())
    }

    pub(super) fn from_parts(
        scheme: &str,
        host: &str,
        path: &str,
        query: &[Vec<String>],
        fragment: &str,
    ) -> Result<Self, String> {
        if !url_valid_scheme(scheme) {
            return Err(format!("invalid URL scheme `{}`", scheme));
        }
        let host = if host.is_empty() {
            None
        } else {
            Some(url_host_to_ascii(host)?)
        };
        let fragment = if fragment.is_empty() {
            None
        } else {
            Some(fragment.to_string())
        };
        Ok(UrlParts {
            scheme: scheme.to_ascii_lowercase(),
            host,
            port: None,
            path: path.to_string(),
            query: url_pairs_from_rows(query),
            fragment,
        }
        .normalize())
    }

    pub(super) fn file(path: &str) -> Self {
        UrlParts {
            scheme: "file".to_string(),
            host: Some(String::new()),
            port: None,
            path: if path.starts_with('/') {
                path.to_string()
            } else {
                format!("/{}", path)
            },
            query: Vec::new(),
            fragment: None,
        }
    }

    /// `mime_essence`/`mime_params` are the caller's already-rendered MIME
    /// `to_string_value()`-equivalent (essence + `; k=v` params) — the
    /// `core.mime` port isn't in this card's slice, so `data()` accepts the
    /// caller's MIME struct rendered to a string, matching what
    /// `mime.to_string_value()` produces in AOT.
    pub(super) fn data(mime_rendered: &str, text: &str) -> Self {
        UrlParts {
            scheme: "data".to_string(),
            host: None,
            port: None,
            path: format!("{},{}", mime_rendered, url_percent_encode(text, false)),
            query: Vec::new(),
            fragment: None,
        }
    }

    fn normalize(&self) -> Self {
        let mut out = UrlParts {
            scheme: self.scheme.to_ascii_lowercase(),
            host: self.host.clone(),
            port: self.port,
            path: self.path.clone(),
            query: self.query.clone(),
            fragment: self.fragment.clone(),
        };
        if let Some(h) = &out.host {
            if let Ok(ascii) = url_host_to_ascii(h) {
                out.host = Some(ascii);
            }
        }
        if out.scheme != "data" {
            out.path = url_remove_dot_segments(&out.path);
        }
        out
    }

    // Note: no `to_string_value`/`Display` port here — comptime's generic
    // `CtValue::Struct` `jet_show` (`AST/comptime.rs`) prints every builtin
    // struct type (`Duration`, `Match`, ...) as `TypeName(field: val, ...)`
    // uniformly rather than dispatching to a per-type renderer; `Url` follows
    // that same existing precedent rather than being special-cased alone.
}

impl Clone for UrlParts {
    fn clone(&self) -> Self {
        UrlParts {
            scheme: self.scheme.clone(),
            host: self.host.clone(),
            port: self.port,
            path: self.path.clone(),
            query: self.query.clone(),
            fragment: self.fragment.clone(),
        }
    }
}

fn url_valid_scheme(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

fn url_parse_authority(authority: &str) -> Result<(String, Option<i64>), String> {
    let host_port = authority.rsplit_once('@').map(|(_, h)| h).unwrap_or(authority);
    if host_port.is_empty() {
        return Err("URL host is empty".to_string());
    }
    if host_port.starts_with('[') {
        let Some(end) = host_port.find(']') else {
            return Err("IPv6 host is missing `]`".to_string());
        };
        let host = host_port[..=end].to_ascii_lowercase();
        let port = if host_port[end + 1..].starts_with(':') {
            Some(url_parse_port(&host_port[end + 2..])?)
        } else {
            None
        };
        return Ok((host, port));
    }
    let (host, port) = if let Some((h, p)) = host_port.rsplit_once(':') {
        if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) {
            (h, Some(url_parse_port(p)?))
        } else {
            (host_port, None)
        }
    } else {
        (host_port, None)
    };
    Ok((url_host_to_ascii(host)?, port))
}

fn url_parse_port(p: &str) -> Result<i64, String> {
    let n: i64 = p.parse().map_err(|_| format!("invalid URL port `{}`", p))?;
    if !(0..=65535).contains(&n) {
        return Err(format!("URL port out of range `{}`", p));
    }
    Ok(n)
}

fn url_host_to_ascii(host: &str) -> Result<String, String> {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let mut labels = Vec::new();
    for label in host.split('.') {
        if label.is_empty() {
            return Err("URL host has an empty label".to_string());
        }
        labels.push(if label.is_ascii() {
            label.to_string()
        } else {
            format!("xn--{}", punycode_encode(label)?)
        });
    }
    Ok(labels.join("."))
}

fn punycode_encode(input: &str) -> Result<String, String> {
    const BASE: u32 = 36;
    const TMIN: u32 = 1;
    const TMAX: u32 = 26;
    const INITIAL_BIAS: u32 = 72;
    const INITIAL_N: u32 = 128;
    let codepoints: Vec<u32> = input.chars().map(|c| c as u32).collect();
    let mut out = String::new();
    for &cp in &codepoints {
        if cp < 0x80 {
            out.push(char::from_u32(cp).ok_or_else(|| "bad codepoint".to_string())?);
        }
    }
    let basic = out.chars().count() as u32;
    let mut handled = basic;
    if basic > 0 && handled < codepoints.len() as u32 {
        out.push('-');
    }
    let mut n = INITIAL_N;
    let mut delta = 0u32;
    let mut bias = INITIAL_BIAS;
    while handled < codepoints.len() as u32 {
        let m = *codepoints
            .iter()
            .filter(|&&cp| cp >= n)
            .min()
            .ok_or_else(|| "bad punycode input".to_string())?;
        delta = delta
            .checked_add((m - n).saturating_mul(handled + 1))
            .ok_or_else(|| "punycode overflow".to_string())?;
        n = m;
        for &cp in &codepoints {
            if cp < n {
                delta = delta.checked_add(1).ok_or_else(|| "punycode overflow".to_string())?;
            } else if cp == n {
                let mut q = delta;
                let mut k = BASE;
                loop {
                    let t = if k <= bias {
                        TMIN
                    } else if k >= bias + TMAX {
                        TMAX
                    } else {
                        k - bias
                    };
                    if q < t {
                        break;
                    }
                    out.push(punycode_digit(t + ((q - t) % (BASE - t)))?);
                    q = (q - t) / (BASE - t);
                    k += BASE;
                }
                out.push(punycode_digit(q)?);
                bias = punycode_adapt(delta, handled + 1, handled == basic);
                delta = 0;
                handled += 1;
            }
        }
        delta = delta.checked_add(1).ok_or_else(|| "punycode overflow".to_string())?;
        n = n.checked_add(1).ok_or_else(|| "punycode overflow".to_string())?;
    }
    Ok(out)
}

fn punycode_digit(d: u32) -> Result<char, String> {
    char::from_u32(if d < 26 { b'a' as u32 + d } else { b'0' as u32 + d - 26 })
        .ok_or_else(|| "bad punycode digit".to_string())
}

fn punycode_adapt(mut delta: u32, points: u32, first: bool) -> u32 {
    delta = if first { delta / 700 } else { delta / 2 };
    delta += delta / points;
    let mut k = 0;
    while delta > ((36 - 1) * 26) / 2 {
        delta /= 36 - 1;
        k += 36;
    }
    k + (((36 - 1 + 1) * delta) / (delta + 38))
}

fn url_parse_query(q: &str) -> Result<Vec<(String, String)>, String> {
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for part in q.split('&') {
        let (k, v) = part.split_once('=').unwrap_or((part, ""));
        out.push((url_percent_decode_str(k)?, url_percent_decode_str(v)?));
    }
    Ok(out)
}

fn url_pairs_from_rows(rows: &[Vec<String>]) -> Vec<(String, String)> {
    rows.iter()
        .filter(|r| !r.is_empty())
        .map(|r| {
            (
                r.get(0).cloned().unwrap_or_default(),
                r.get(1).cloned().unwrap_or_default(),
            )
        })
        .collect()
}

pub(super) fn url_render_query(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", url_percent_encode(k, false), url_percent_encode(v, false)))
        .collect::<Vec<_>>()
        .join("&")
}

pub(super) fn url_percent_encode(s: &str, path: bool) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        let keep = b.is_ascii_alphanumeric()
            || matches!(*b, b'-' | b'.' | b'_' | b'~')
            || (path && *b == b'/');
        if keep {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

pub(super) fn url_percent_decode_str(s: &str) -> Result<String, String> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err("truncated percent escape".to_string());
            }
            let hex = &s[i + 1..i + 3];
            let byte = u8::from_str_radix(hex, 16)
                .map_err(|_| format!("invalid percent escape `%{}`", hex))?;
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "percent escape is not valid UTF-8".to_string())
}

fn url_remove_dot_segments(path: &str) -> String {
    if path.is_empty() || !path.contains('.') {
        return path.to_string();
    }
    let absolute = path.starts_with('/');
    let trailing = path.ends_with('/');
    let mut parts = Vec::new();
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(p),
        }
    }
    let mut out = String::new();
    if absolute {
        out.push('/');
    }
    out.push_str(&parts.join("/"));
    if trailing && !out.ends_with('/') {
        out.push('/');
    }
    if out.is_empty() && absolute {
        "/".to_string()
    } else {
        out
    }
}
