//! CtValue adapters over the canonical Prelude URL/MIME kernel.

mod url_kernel {
    #[derive(Clone, Debug)]
    pub struct JetURLParts {
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

pub(super) type UrlParts = url_kernel::JetURLParts;

/// Re-enter the canonical URL kernel from the `core.net.url.URL` record fields
/// without reparsing or normalizing; the kernel owns the projection.
#[allow(clippy::too_many_arguments)]
pub(super) fn from_core_record(
    scheme: &str,
    user_text: &str,
    password_text: &str,
    host: &str,
    port: i64,
    port_explicit: bool,
    path: &str,
    query: &str,
    fragment: &str,
) -> UrlParts {
    url_kernel::JetURLParts::from_core_record(
        scheme,
        user_text,
        password_text,
        host,
        port,
        port_explicit,
        path,
        query,
        fragment,
    )
}

pub(super) fn parse(input: &str) -> Result<UrlParts, String> {
    url_kernel::JetURLParts::parse(&input.to_string())
}

pub(super) fn validate_typed_url_literal(literals: &[String]) -> Result<(), String> {
    let literal_refs = literals.iter().map(String::as_str).collect::<Vec<_>>();
    url_kernel::jet_validate_typed_url_literal(&literal_refs)
}


pub(super) fn from_parts(
    scheme: &str,
    host: &str,
    path: &str,
    query: &[Vec<String>],
    fragment: &str,
) -> Result<UrlParts, String> {
    url_kernel::JetURLParts::from_parts(
        &scheme.to_string(),
        &host.to_string(),
        &path.to_string(),
        &query.to_vec(),
        &fragment.to_string(),
    )
}

pub(super) fn file(path: &str) -> UrlParts {
    url_kernel::JetURLParts::file(&path.to_string())
}

pub(super) fn data(mime_rendered: &str, text: &str) -> UrlParts {
    url_kernel::JetURLParts::data(mime_rendered, &text.to_string())
}

pub(super) fn url_render_query(pairs: &[(String, String)]) -> String {
    url_kernel::render_query(pairs)
}

pub(crate) fn url_percent_encode(value: &str, path: bool) -> String {
    url_kernel::jet_url_percent_encode(value, path)
}

pub(crate) fn url_percent_decode_str(value: &str) -> Result<String, String> {
    url_kernel::percent_decode(value)
}
