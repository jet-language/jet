// ── D-HTTPLIB2=B / D-HTTPLIB4=B: core.http.client — request builder ─────────
// JetHttpRequest and JetHttpResponse live here (in the generated program's
// crate) so they're accessible without cross-crate type imports. The ureq
// bridge functions use only primitive types (i64, String, Vec<String>) and are
// called through wrappers here. This is the I6-safe pattern.

struct JetHttpClientOwner {
    handle: i64,
    drop_handle: fn(i64),
}

impl Drop for JetHttpClientOwner {
    fn drop(&mut self) {
        (self.drop_handle)(self.handle);
    }
}

#[derive(Clone)]
struct JetHttpClient {
    owner: std::sync::Arc<JetHttpClientOwner>,
    policy_error: Option<JetHttpError>,
}

impl JetHttpClient {
    fn new(handle: i64, drop_handle: fn(i64)) -> Self {
        Self {
            owner: std::sync::Arc::new(JetHttpClientOwner { handle, drop_handle }),
            policy_error: None,
        }
    }

    fn policy(self, next: Result<i64, JetHttpError>, drop_handle: fn(i64)) -> Self {
        match next {
            Ok(handle) => Self::new(handle, drop_handle),
            Err(error) => Self {
                policy_error: Some(error),
                ..self
            },
        }
    }
}

fn jet_http_client_request_new(method: &String, url: &String) -> JetHttpRequest {
    JetHttpRequest {
        method: method.clone(),
        url: url.clone(),
        path: String::new(),
        version: "HTTP/1.1".to_string(),
        headers: JetHttpHeaders::new(),
        header_error: None,
        body: JetHttpBody::empty(),
        body_set: false,
        params: std::collections::BTreeMap::new(),
        route_template: None,
        timeout_ms: None,
        connect_timeout_ms: None,
        read_timeout_ms: None,
        total_timeout_ms: None,
        redirects: None,
        proxy: None,
        cookies: Vec::new(),
        form: Vec::new(),
        multipart: Vec::new(),
    }
}

fn jet_http_client_request_header(
    mut req: JetHttpRequest,
    name: &String,
    value: &String,
) -> JetHttpRequest {
    if let Err(error) = req.headers.append(name, value) {
        let _ = error;
        req.header_error = Some(JetHttpError::InvalidHeader);
    }
    req
}

fn jet_http_client_request_body(mut req: JetHttpRequest, body: &String) -> JetHttpRequest {
    req.body = JetHttpBody::from_text(body.clone());
    req.body_set = true;
    req
}

fn jet_http_client_request_body_stream(mut req: JetHttpRequest, body: JetHttpBody) -> JetHttpRequest {
    req.body = body;
    req.body_set = true;
    req
}

fn jet_http_client_body_upload(
    req: &JetHttpRequest,
) -> Result<(Option<i64>, bool, Option<JetHttpBodyChunks>), JetHttpError> {
    if !req.body_set {
        return Ok((None, false, None));
    }
    let length = req.body.length().map(|length| length as i64);
    let chunks = req.body.chunks(64 * 1024)?;
    Ok((length, true, Some(chunks)))
}

fn jet_http_client_request_timeout(mut req: JetHttpRequest, ms: i64) -> JetHttpRequest {
    req.timeout_ms = Some(ms);
    req
}

fn jet_http_client_request_connect_timeout(
    mut req: JetHttpRequest,
    ms: i64,
) -> JetHttpRequest {
    req.connect_timeout_ms = Some(ms);
    req
}

fn jet_http_client_request_read_timeout(mut req: JetHttpRequest, ms: i64) -> JetHttpRequest {
    req.read_timeout_ms = Some(ms);
    req
}

fn jet_http_client_request_total_timeout(mut req: JetHttpRequest, ms: i64) -> JetHttpRequest {
    req.total_timeout_ms = Some(ms);
    req
}

fn jet_http_client_request_redirects(mut req: JetHttpRequest, limit: i64) -> JetHttpRequest {
    req.redirects = Some(limit);
    req
}

fn jet_http_client_request_proxy(mut req: JetHttpRequest, proxy: &String) -> JetHttpRequest {
    req.proxy = Some(proxy.clone());
    req
}

fn jet_http_client_request_cookie(
    mut req: JetHttpRequest,
    name: &String,
    value: &String,
) -> JetHttpRequest {
    req.cookies.push(name.clone());
    req.cookies.push(value.clone());
    req
}

fn jet_http_client_request_form(
    mut req: JetHttpRequest,
    name: &String,
    value: &String,
) -> JetHttpRequest {
    req.form.push(name.clone());
    req.form.push(value.clone());
    req
}

fn jet_http_client_request_multipart_text(
    mut req: JetHttpRequest,
    name: &String,
    value: &String,
) -> JetHttpRequest {
    req.multipart.push(name.clone());
    req.multipart.push(value.clone());
    req
}

fn jet_http_client_response_status(resp: &JetHttpResponse) -> i64 {
    resp.status
}

fn jet_http_client_response_new(
    status: i64,
    body_handle: i64,
    body_length: Option<i64>,
    headers: Vec<String>,
    body_read: fn(i64, usize) -> Result<Option<Vec<u8>>, JetHttpError>,
    body_close: fn(i64),
    protocol: String,
    remote_address: String,
    redirect_history: Vec<String>,
    timings_ms: Vec<i64>,
    reused_connection: bool,
    raw_content_encoding: Option<String>,
) -> Result<JetHttpResponse, JetHttpError> {
    let body_length = body_length
        .map(usize::try_from)
        .transpose()
        .map_err(|_| JetHttpError::InvalidFraming)?;
    Ok(JetHttpResponse {
        status,
        version: "HTTP/1.1".to_string(),
        body: JetHttpBody::bridge(body_handle, body_length, body_read, body_close),
        headers: JetHttpHeaders::from_flat(headers).map_err(|_| JetHttpError::InvalidHeader)?,
        trailers: JetHttpHeaders::new(),
        head_content_length: None,
        protocol,
        remote_address,
        redirect_history,
        timings_ms,
        reused_connection,
        raw_content_encoding,
    })
}
fn jet_http_client_response_body(resp: &JetHttpResponse) -> JetHttpBody {
    resp.body.clone()
}
fn jet_http_client_response_header(resp: &JetHttpResponse, name: &String) -> Option<String> {
    resp.headers.get(name).cloned()
}

fn jet_http_response_cookies(resp: &JetHttpResponse) -> Vec<String> {
    resp.headers
        .all("set-cookie")
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn jet_http_client_response_protocol(resp: &JetHttpResponse) -> String {
    resp.protocol.clone()
}
fn jet_http_client_response_remote_address(resp: &JetHttpResponse) -> String {
    resp.remote_address.clone()
}
fn jet_http_client_response_redirect_history(resp: &JetHttpResponse) -> Vec<String> {
    resp.redirect_history.clone()
}
fn jet_http_client_response_timings(resp: &JetHttpResponse) -> Vec<i64> {
    resp.timings_ms.clone()
}
fn jet_http_client_response_reused(resp: &JetHttpResponse) -> bool {
    resp.reused_connection
}
fn jet_http_client_response_raw_encoding(resp: &JetHttpResponse) -> Option<String> {
    resp.raw_content_encoding.clone()
}
