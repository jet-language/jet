//! CtValue adapters over the canonical Prelude URL/MIME kernel.

mod url_kernel {
    #[derive(Clone, Debug)]
    pub struct JetURL {
        pub scheme: String,
        pub username: Option<String>,
        pub password: Option<String>,
        pub host: Option<String>,
        pub port: Option<i64>,
        pub path: String,
        pub query: Vec<(String, String)>,
        pub fragment: Option<String>,
        pub typed_host: Option<Vec<(String, bool)>>,
        pub typed_path: Option<Vec<(String, bool)>>,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct JetMIME {
        pub top: String,
        pub sub: String,
        pub params: Vec<(String, String)>,
    }

    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../../jet-codegen/src/Prelude/CoreLib/JetStd/UrlMime.rs");

    pub(super) fn render_query(pairs: &[(String, String)]) -> String {
        jet_url_render_query(pairs)
    }

    pub(super) fn percent_encode(value: &str) -> String {
        jet_url_percent_encode(value, false)
    }

    pub(super) fn percent_decode(value: &str) -> Result<String, String> {
        jet_url_percent_decode_str(value)
    }
}

pub(super) type UrlParts = url_kernel::JetURL;

/// Re-enter the canonical URL value without reparsing or normalizing it.
///
/// The TIR value adapter may only marshal fields across the `CtValue` boundary;
/// the URL kernel remains the owner of the representation and formatter.
pub(super) fn from_marshaled(
    scheme: String,
    username: Option<String>,
    password: Option<String>,
    host: Option<String>,
    port: Option<i64>,
    path: String,
    query: Vec<(String, String)>,
    fragment: Option<String>,
    typed_host: Option<Vec<(String, bool)>>,
    typed_path: Option<Vec<(String, bool)>>,
) -> UrlParts {
    url_kernel::JetURL {
        scheme,
        username,
        password,
        host,
        port,
        path,
        query,
        fragment,
        typed_host,
        typed_path,
    }
}

pub(super) fn parse(input: &str) -> Result<UrlParts, String> {
    url_kernel::JetURL::parse(&input.to_string())
}

pub(super) fn validate_typed_url_literal(literals: &[String]) -> Result<(), String> {
    let literal_refs = literals.iter().map(String::as_str).collect::<Vec<_>>();
    url_kernel::jet_validate_typed_url_literal(&literal_refs)
}

pub(super) fn typed_url_literal(
    literals: &[String],
    holes: &[String],
) -> UrlParts {
    let literal_refs = literals.iter().map(String::as_str).collect::<Vec<_>>();
    url_kernel::jet_typed_url_literal(&literal_refs, holes.to_vec())
}

pub(super) fn from_parts(
    scheme: &str,
    host: &str,
    path: &str,
    query: &[Vec<String>],
    fragment: &str,
) -> Result<UrlParts, String> {
    url_kernel::JetURL::from_parts(
        &scheme.to_string(),
        &host.to_string(),
        &path.to_string(),
        &query.to_vec(),
        &fragment.to_string(),
    )
}

pub(super) fn file(path: &str) -> UrlParts {
    url_kernel::JetURL::file(&path.to_string())
}

pub(super) fn data(mime_rendered: &str, text: &str) -> UrlParts {
    let mime = url_kernel::JetMIME::parse(&mime_rendered.to_string()).unwrap_or_else(|_| {
        url_kernel::JetMIME {
            top: mime_rendered.to_string(),
            sub: String::new(),
            params: Vec::new(),
        }
    });
    url_kernel::JetURL::data(&mime, &text.to_string())
}

pub(super) fn url_render_query(pairs: &[(String, String)]) -> String {
    url_kernel::render_query(pairs)
}

pub(super) fn url_percent_encode(value: &str, _path: bool) -> String {
    url_kernel::percent_encode(value)
}

pub(super) fn url_percent_decode_str(value: &str) -> Result<String, String> {
    url_kernel::percent_decode(value)
}
