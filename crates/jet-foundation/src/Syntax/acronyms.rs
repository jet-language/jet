//! D-ACRO-CASE1=A + D-ACRO-LEX1=A — acronym casing law and applied lexicon.
//!
//! Acronyms stay all-caps inside PascalCase names and glue to the next word
//! (`HTTPHeader`). Underscore is legal only between two touching capital runs
//! (`HTTP_API`). Word split: maximal capital run; a run followed by lowercase
//! gives its last capital to the next word.
//!
//! Lexicon (initials test): anything formed from initial letters is an acronym
//! regardless of pronunciation. Coined contractions (`Wasm`, `Bindgen`) stay
//! ordinary words.

/// Closed applied acronym lexicon (D-ACRO-LEX1=A). Longer entries first so
/// compound renames win over bare roots.
pub const ACRONYM_RESPILLS: &[(&str, &str)] = &[
    // Jet-prefixed emitted runtime families
    ("JetHttp", "JetHTTP"),
    ("JetTls", "JetTLS"),
    ("JetDns", "JetDNS"),
    ("JetTcp", "JetTCP"),
    ("JetUdp", "JetUDP"),
    ("JetIo", "JetIO"),
    ("JetJson", "JetJSON"),
    ("JetCbor", "JetCBOR"),
    ("JetUrl", "JetURL"),
    ("JetMime", "JetMIME"),
    // Direct surface respells
    ("Utf8Error", "UTF8Error"),
    ("Utf8", "UTF8"),
    ("IpAddr", "IPAddr"),
    ("Ip", "IP"),
    ("DbValue", "DBValue"),
    ("IoError", "IOError"),
    ("IoContext", "IOContext"),
    ("IoOperation", "IOOperation"),
    ("Macos", "MacOS"),
    ("Http", "HTTP"),
    ("Smtp", "SMTP"),
    ("Tls", "TLS"),
    ("Dns", "DNS"),
    ("Tcp", "TCP"),
    ("Udp", "UDP"),
    ("Html", "HTML"),
    ("Json", "JSON"),
    ("Toml", "TOML"),
    ("Yaml", "YAML"),
    ("Csv", "CSV"),
    ("Sql", "SQL"),
    ("Cli", "CLI"),
    ("Abi", "ABI"),
    ("Gpu", "GPU"),
    ("Fs", "FS"),
    ("Db", "DB"),
    ("Io", "IO"),
    ("Os", "OS"),
    ("Js", "JS"),
    ("Cbor", "CBOR"),
    ("Url", "URL"),
    ("Mime", "MIME"),
];

/// Acronyms that stay ordinary words (not initials-formed).
pub const ACRONYM_WORD_EXCEPTIONS: &[&str] = &["Wasm", "WasmExport", "Bindgen"];

/// Retired → canonical spelling for one teaching fix each (I8, no aliases).
pub fn retired_acronym_spelling(name: &str) -> Option<String> {
    let next = respell_acronym_name(name);
    if next == name {
        None
    } else {
        Some(next)
    }
}

/// Apply the lexicon respell to a PascalCase identifier (prefix-aware).
pub fn respell_acronym_name(name: &str) -> String {
    for (from, to) in ACRONYM_RESPILLS {
        if let Some(rest) = name.strip_prefix(from) {
            if rest.is_empty()
                || rest.starts_with(|c: char| c.is_uppercase() || c == '_')
            {
                return format!("{to}{rest}");
            }
        }
    }
    name.to_string()
}

/// Mechanical word split for PascalCase / glued-acronym names (D-ACRO-CASE1=A).
/// Underscore is a hard boundary. Capital run followed by lowercase yields its
/// last capital to the next word (`HTTPHeader` → `HTTP` + `Header`).
pub fn split_pascal_words(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    for segment in name.split('_').filter(|s| !s.is_empty()) {
        let chars: Vec<char> = segment.chars().collect();
        if chars.is_empty() {
            continue;
        }
        let mut start = 0usize;
        let mut i = 0usize;
        while i < chars.len() {
            if chars[i].is_uppercase() {
                let run_start = i;
                while i < chars.len() && chars[i].is_uppercase() {
                    i += 1;
                }
                if i < chars.len() && chars[i].is_lowercase() && i - run_start > 1 {
                    // Last capital begins the next word (`HTTPHeader` → HTTP + Header).
                    let last_cap = i - 1;
                    if last_cap > start {
                        words.push(chars[start..last_cap].iter().collect());
                    }
                    start = last_cap;
                    while i < chars.len() && !chars[i].is_uppercase() {
                        i += 1;
                    }
                    words.push(chars[start..i].iter().collect());
                    start = i;
                } else if i < chars.len() && chars[i].is_lowercase() {
                    // Single capital + lowercase = ordinary word start.
                    if run_start > start {
                        words.push(chars[start..run_start].iter().collect());
                    }
                    start = run_start;
                    while i < chars.len() && !chars[i].is_uppercase() {
                        i += 1;
                    }
                    words.push(chars[start..i].iter().collect());
                    start = i;
                }
                // else: all-caps run continues; flush at end / next boundary
            } else {
                while i < chars.len() && !chars[i].is_uppercase() {
                    i += 1;
                }
                if i > start {
                    words.push(chars[start..i].iter().collect());
                    start = i;
                }
            }
        }
        if start < chars.len() {
            words.push(chars[start..].iter().collect());
        }
    }
    words
}

/// Convert a PascalCase / snake name to snake_case with the acronym split rule.
pub fn to_snake_acronym(name: &str) -> String {
    if name.contains('_') && name.chars().all(|c| !c.is_uppercase() || c == '_') {
        return name.to_string();
    }
    split_pascal_words(name)
        .into_iter()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join("_")
}

/// Convert snake / Pascal name to PascalCase, restoring known acronyms.
pub fn to_pascal_acronym(name: &str) -> String {
    let words = if name.contains('_') || name.chars().all(|c| !c.is_uppercase()) {
        name.split('_')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    } else {
        split_pascal_words(name)
    };
    words
        .into_iter()
        .map(|w| restore_acronym_word(&w))
        .collect()
}

fn restore_acronym_word(word: &str) -> String {
    let lower = word.to_lowercase();
    for (from, to) in ACRONYM_RESPILLS {
        if from.to_lowercase() == lower || to.to_lowercase() == lower {
            if to.chars().all(|c| !c.is_lowercase()) {
                return (*to).to_string();
            }
        }
    }
    let mut chars = word.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
        None => String::new(),
    }
}

/// Convert snake / Pascal name to camelCase via the acronym split + lexicon.
pub fn to_camel_acronym(name: &str) -> String {
    let pascal = to_pascal_acronym(name);
    let words = split_pascal_words(&pascal);
    let mut it = words.into_iter();
    let Some(first) = it.next() else {
        return String::new();
    };
    let mut out = first.to_lowercase();
    for w in it {
        out.push_str(&w);
    }
    out
}


/// Convert to screaming snake via the acronym split rule.
pub fn to_shouty_acronym(name: &str) -> String {
    split_pascal_words(name)
        .into_iter()
        .map(|w| w.to_uppercase())
        .collect::<Vec<_>>()
        .join("_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_glued_acronym() {
        assert_eq!(split_pascal_words("HTTPHeader"), vec!["HTTP", "Header"]);
        assert_eq!(split_pascal_words("WasmABI"), vec!["Wasm", "ABI"]);
        assert_eq!(split_pascal_words("HTTP_API"), vec!["HTTP", "API"]);
        assert_eq!(split_pascal_words("MacOS"), vec!["Mac", "OS"]);
        assert_eq!(split_pascal_words("IOError"), vec!["IO", "Error"]);
    }

    #[test]
    fn rename_all_styles() {
        assert_eq!(to_snake_acronym("HTTPHeader"), "http_header");
        assert_eq!(to_snake_acronym("HTTP_API"), "http_api");
        assert_eq!(to_camel_acronym("HTTPHeader"), "httpHeader");
        assert_eq!(to_camel_acronym("http_header"), "httpHeader");
        assert_eq!(to_pascal_acronym("http_header"), "HTTPHeader");
        assert_eq!(to_shouty_acronym("HTTPHeader"), "HTTP_HEADER");
        assert_eq!(to_snake_acronym("http_header"), "http_header");
    }

    #[test]
    fn lexicon_respells() {
        assert_eq!(respell_acronym_name("HttpRequest"), "HTTPRequest");
        assert_eq!(respell_acronym_name("JetHttpClient"), "JetHTTPClient");
        assert_eq!(respell_acronym_name("Json"), "JSON");
        assert_eq!(respell_acronym_name("Cli"), "CLI");
        assert_eq!(respell_acronym_name("Macos"), "MacOS");
        assert_eq!(respell_acronym_name("Wasm"), "Wasm");
        assert_eq!(respell_acronym_name("WasmExport"), "WasmExport");
    }

    #[test]
    fn vocabulary_rejects_old_spellings_in_surface_constants() {
        // Compiler-owned surface strings must already be lexicon-canonical.
        for name in [
            crate::Syntax::MARKER_CLI,
            crate::Syntax::MARKER_ABI,
            crate::Syntax::MARKER_HTML,
            crate::Syntax::DSL_BLOCK_SQL,
            crate::Syntax::WEB_BUCKET_JS,
            crate::Syntax::TARGET_OS_NAMESPACE,
            crate::Syntax::TARGET_OS_MACOS,
            crate::Syntax::TYPE_DATA_JSON,
            crate::Syntax::TYPE_DATA_CSV,
            crate::Syntax::TYPE_DATA_TOML,
            crate::Syntax::TYPE_DATA_YAML,
            crate::Syntax::TYPE_DB_VALUE,
            crate::Syntax::TYPE_IO_ERROR,
            crate::Syntax::TYPE_JSON,
            crate::Syntax::TYPE_UTF8_ERROR,
        ] {
            assert_eq!(
                respell_acronym_name(name),
                name,
                "surface constant `{name}` is not lexicon-canonical"
            );
            assert!(
                retired_acronym_spelling(name).is_none(),
                "surface constant `{name}` still looks retired"
            );
        }
        // Exceptions stay word-cased.
        for name in ACRONYM_WORD_EXCEPTIONS {
            assert_eq!(respell_acronym_name(name), *name);
        }
    }
}
