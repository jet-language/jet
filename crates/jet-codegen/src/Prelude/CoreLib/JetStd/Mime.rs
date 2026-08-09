// D-ONCE-LAW1 / I9: one MIME parser and extension table for every tier.
#[derive(Clone, Debug, PartialEq)]
pub struct JetMimeParts {
    pub top: String,
    pub sub: String,
    pub params: Vec<(String, String)>,
}

pub fn jet_mime_parse_parts(input: &str) -> Result<JetMimeParts, String> {
    let mut parts = input.split(';');
    let essence = parts.next().unwrap_or("").trim();
    let Some((top, sub)) = essence.split_once('/') else {
        return Err("MIME type needs `type/subtype`".to_string());
    };
    let top = top.trim().to_ascii_lowercase();
    let sub = sub.trim().to_ascii_lowercase();
    if top.is_empty() || sub.is_empty() || !jet_mime_token(&top) || !jet_mime_token(&sub) {
        return Err(format!("invalid MIME type `{}`", essence));
    }
    let mut params = Vec::new();
    for parameter in parts {
        let parameter = parameter.trim();
        if parameter.is_empty() {
            continue;
        }
        let Some((key, value)) = parameter.split_once('=') else {
            return Err(format!("invalid MIME parameter `{}`", parameter));
        };
        let key = key.trim().to_ascii_lowercase();
        if key.is_empty() || !jet_mime_token(&key) {
            return Err(format!("invalid MIME parameter `{}`", key.trim()));
        }
        params.push((key, value.trim().trim_matches('"').to_string()));
    }
    Ok(JetMimeParts { top, sub, params })
}

pub fn jet_mime_essence(top: &str, sub: &str) -> String {
    format!("{top}/{sub}")
}

pub fn jet_mime_param<'a>(params: &'a [(String, String)], name: &str) -> Option<&'a str> {
    let name = name.to_ascii_lowercase();
    params
        .iter()
        .find(|(key, _)| key == &name)
        .map(|(_, value)| value.as_str())
}

pub fn jet_mime_to_string(top: &str, sub: &str, params: &[(String, String)]) -> String {
    let mut output = jet_mime_essence(top, sub);
    for (key, value) in params {
        output.push_str("; ");
        output.push_str(key);
        output.push('=');
        output.push_str(value);
    }
    output
}

fn jet_mime_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#' | b'$' | b'&' | b'-' | b'^' | b'_' | b'.' | b'+'
                )
        })
}

pub fn jet_mime_from_extension(ext: &str) -> Option<&'static str> {
    match ext.trim_start_matches('.').to_ascii_lowercase().as_str() {
        "html" | "htm" => Some("text/html"),
        "css" => Some("text/css"),
        "csv" => Some("text/csv"),
        "txt" | "text" => Some("text/plain"),
        "md" => Some("text/markdown"),
        "json" => Some("application/json"),
        "js" | "mjs" => Some("text/javascript"),
        "wasm" => Some("application/wasm"),
        "pdf" => Some("application/pdf"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "svg" => Some("image/svg+xml"),
        "webp" => Some("image/webp"),
        "ico" => Some("image/x-icon"),
        "mp3" => Some("audio/mpeg"),
        "mp4" => Some("video/mp4"),
        "xml" => Some("application/xml"),
        "zip" => Some("application/zip"),
        "gz" => Some("application/gzip"),
        "tar" => Some("application/x-tar"),
        _ => None,
    }
}

pub fn jet_extension_from_mime(mime: &str) -> Option<&'static str> {
    match mime.to_ascii_lowercase().split(';').next().unwrap_or("").trim() {
        "text/html" => Some("html"),
        "text/css" => Some("css"),
        "text/csv" => Some("csv"),
        "text/plain" => Some("txt"),
        "text/markdown" => Some("md"),
        "application/json" => Some("json"),
        "text/javascript" | "application/javascript" => Some("js"),
        "application/wasm" => Some("wasm"),
        "application/pdf" => Some("pdf"),
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/svg+xml" => Some("svg"),
        "image/webp" => Some("webp"),
        "image/x-icon" => Some("ico"),
        "audio/mpeg" => Some("mp3"),
        "video/mp4" => Some("mp4"),
        "application/xml" | "text/xml" => Some("xml"),
        "application/zip" => Some("zip"),
        "application/gzip" => Some("gz"),
        "application/x-tar" => Some("tar"),
        _ => None,
    }
}
