// ── D-HTTPLIB2=B / D-HTTPLIB4=B: core.http.client — request builder ─────────
// JetHTTPRequest and JetHTTPResponse live here (in the generated program's
// crate) so they're accessible without cross-crate type imports. The native
// client seam passes owned request/response rows through the bridge; legacy
// primitive wrappers remain only for direct compatibility callers.

#[derive(Clone)]
enum JetHTTPProxy {
    FromEnvironment,
    None,
    Url(String),
}

/// D-HTTP-CLIENT2=A: typed redirect policy for `Client.redirects`.
#[derive(Clone)]
enum JetHTTPRedirectPolicy {
    Follow {
        max: i64,
        same_origin_credentials: bool,
    },
}

/// D-HTTP-CLIENT2=A: stale-pool connection retry policy for `Client.retries`.
/// Default unset is Safe (GET/HEAD/OPTIONS/TRACE), max one attempt, never
/// status-based. Idempotent opts in PUT/DELETE; None disables.
#[derive(Clone)]
enum JetHTTPRetryPolicy {
    None,
    Safe,
    Idempotent,
}

/// D-HTTP-CLIENT2=A: explicit in-memory RFC6265bis cookie jar.
#[derive(Clone)]
enum JetHTTPCookieJar {
    Memory,
}

struct JetHTTPClientOwner {
    handle: i64,
    drop_handle: fn(i64),
}

impl Drop for JetHTTPClientOwner {
    fn drop(&mut self) {
        (self.drop_handle)(self.handle);
    }
}

#[derive(Clone)]
struct JetHTTPClient {
    owner: std::sync::Arc<JetHTTPClientOwner>,
    policy_error: Option<JetHTTPError>,
}

impl JetHTTPClient {
    fn new(handle: i64, drop_handle: fn(i64)) -> Self {
        Self {
            owner: std::sync::Arc::new(JetHTTPClientOwner { handle, drop_handle }),
            policy_error: None,
        }
    }

    fn policy(self, next: Result<i64, JetHTTPError>, drop_handle: fn(i64)) -> Self {
        match next {
            Ok(handle) => Self::new(handle, drop_handle),
            Err(error) => Self {
                policy_error: Some(error),
                ..self
            },
        }
    }
}

fn jet_http_client_request_new(method: &String, url: &String) -> JetHTTPRequest {
    jet_http_client_request_new_owned(method.clone(), url.clone())
}

fn jet_http_client_request_new_owned(method: String, url: String) -> JetHTTPRequest {
    let path = jet_http_request_target_path(&url);
    JetHTTPRequest {
        method,
        url,
        path,
        version: "HTTP/1.1".to_string(),
        headers: JetHTTPHeaders::new(),
        trailers: std::sync::Arc::new(std::sync::Mutex::new(JetHTTPHeaders::new())),
        header_error: None,
        body: JetHTTPBody::empty(),
        body_set: false,
        params: std::collections::BTreeMap::new(),
        route_template: None,
        timeout_ms: None,
        connect_timeout_ms: None,
        read_timeout_ms: None,
        total_timeout_ms: None,
        dns_timeout_ms: None,
        tls_timeout_ms: None,
        write_timeout_ms: None,
        first_byte_timeout_ms: None,
        redirects: None,
        proxy: None,
        cookies: Vec::new(),
        form: Vec::new(),
        multipart: Vec::new(),
    }
}

fn jet_http_client_request_header_owned(
    mut req: JetHTTPRequest,
    name: String,
    value: String,
) -> JetHTTPRequest {
    if !JetHTTPHeaders::valid_name(&name) || !JetHTTPHeaders::valid_value(&value) {
        req.header_error = Some(JetHTTPError::InvalidHeader);
    } else {
        req.headers.entries.push((name, value));
    }
    req
}

fn jet_http_client_request_body_owned(mut req: JetHTTPRequest, body: String) -> JetHTTPRequest {
    req.body = JetHTTPBody::from_text(body);
    req.body_set = true;
    req
}

fn jet_http_client_request_json_text_owned(
    req: JetHTTPRequest,
    body: String,
) -> JetHTTPRequest {
    jet_http_client_request_json_body(
        req,
        JetHTTPBody::from_bytes_with_content_type(
            body.into_bytes(),
            Some("application/json".to_string()),
        ),
    )
}


fn jet_http_client_request_header(
    req: JetHTTPRequest,
    name: &String,
    value: &String,
) -> JetHTTPRequest {
    jet_http_client_request_header_owned(req, name.clone(), value.clone())
}

fn jet_http_client_request_body(mut req: JetHTTPRequest, body: &String) -> JetHTTPRequest {
    jet_http_client_request_body_owned(req, body.clone())
}

fn jet_http_client_request_json_body(
    mut req: JetHTTPRequest,
    body: JetHTTPBody,
) -> JetHTTPRequest {
    req.body = body;
    req.body_set = true;
    if req.headers.set("content-type", "application/json").is_err() {
        req.header_error = Some(JetHTTPError::InvalidHeader);
    }
    req
}

fn jet_http_client_request_json<T: __jet_Encode>(
    req: JetHTTPRequest,
    value: T,
) -> JetHTTPRequest {
    jet_http_client_request_json_body(req, JetHTTPBody::from_json(value))
}

fn jet_http_client_request_json_text(
    req: JetHTTPRequest,
    body: &String,
) -> JetHTTPRequest {
    jet_http_client_request_json_text_owned(req, body.clone())
}


fn jet_http_client_request_body_stream(mut req: JetHTTPRequest, body: JetHTTPBody) -> JetHTTPRequest {
    req.body = body;
    req.body_set = true;
    req
}

fn jet_http_client_body_upload(
    req: &JetHTTPRequest,
) -> Result<(Option<i64>, bool, Option<JetHTTPBodyChunks>), JetHTTPError> {
    if !req.body_set {
        return Ok((None, false, None));
    }
    let length = req.body.length().map(|length| length as i64);
    let chunks = req.body.chunks(64 * 1024)?;
    Ok((length, true, Some(chunks)))
}

fn jet_http_client_request_timeout(mut req: JetHTTPRequest, ms: i64) -> JetHTTPRequest {
    req.timeout_ms = Some(ms);
    req
}

fn jet_http_client_request_connect_timeout(
    mut req: JetHTTPRequest,
    ms: i64,
) -> JetHTTPRequest {
    req.connect_timeout_ms = Some(ms);
    req
}

fn jet_http_client_request_read_timeout(mut req: JetHTTPRequest, ms: i64) -> JetHTTPRequest {
    req.read_timeout_ms = Some(ms);
    req
}

fn jet_http_client_request_total_timeout(mut req: JetHTTPRequest, ms: i64) -> JetHTTPRequest {
    req.total_timeout_ms = Some(ms);
    req
}

fn jet_http_client_request_dns_timeout(mut req: JetHTTPRequest, ms: i64) -> JetHTTPRequest {
    req.dns_timeout_ms = Some(ms);
    req
}

fn jet_http_client_request_tls_timeout(mut req: JetHTTPRequest, ms: i64) -> JetHTTPRequest {
    req.tls_timeout_ms = Some(ms);
    req
}

fn jet_http_client_request_write_timeout(mut req: JetHTTPRequest, ms: i64) -> JetHTTPRequest {
    req.write_timeout_ms = Some(ms);
    req
}

fn jet_http_client_request_first_byte_timeout(
    mut req: JetHTTPRequest,
    ms: i64,
) -> JetHTTPRequest {
    req.first_byte_timeout_ms = Some(ms);
    req
}

fn jet_http_client_request_redirects(mut req: JetHTTPRequest, limit: i64) -> JetHTTPRequest {
    req.redirects = Some(limit);
    req
}

fn jet_http_client_request_proxy_owned(
    mut req: JetHTTPRequest,
    proxy: String,
) -> JetHTTPRequest {
    req.proxy = Some(proxy);
    req
}

fn jet_http_client_request_proxy(req: JetHTTPRequest, proxy: &String) -> JetHTTPRequest {
    jet_http_client_request_proxy_owned(req, proxy.clone())
}

fn jet_http_client_request_cookie_owned(
    mut req: JetHTTPRequest,
    name: String,
    value: String,
) -> JetHTTPRequest {
    req.cookies.push(name);
    req.cookies.push(value);
    req
}

fn jet_http_client_request_cookie(
    req: JetHTTPRequest,
    name: &String,
    value: &String,
) -> JetHTTPRequest {
    jet_http_client_request_cookie_owned(req, name.clone(), value.clone())
}

fn jet_http_client_request_form_owned(
    mut req: JetHTTPRequest,
    name: String,
    value: String,
) -> JetHTTPRequest {
    req.form.push(name);
    req.form.push(value);
    req
}

fn jet_http_client_request_form(
    req: JetHTTPRequest,
    name: &String,
    value: &String,
) -> JetHTTPRequest {
    jet_http_client_request_form_owned(req, name.clone(), value.clone())
}

fn jet_http_client_request_multipart_text_owned(
    mut req: JetHTTPRequest,
    name: String,
    value: String,
) -> JetHTTPRequest {
    req.multipart.push(name);
    req.multipart.push(value);
    req
}

fn jet_http_client_request_multipart_text(
    req: JetHTTPRequest,
    name: &String,
    value: &String,
) -> JetHTTPRequest {
    jet_http_client_request_multipart_text_owned(req, name.clone(), value.clone())
}

fn jet_http_client_response_status(resp: &JetHTTPResponse) -> i64 {
    resp.status
}
fn jet_http_client_response_status_value(
    resp: &JetHTTPResponse,
) -> jet_foundation::Numeric::JetInt {
    jet_foundation::Numeric::JetInt::from_i64(jet_http_client_response_status(resp))
}


fn jet_http_client_response_new(
    status: i64,
    body_handle: i64,
    body_length: Option<i64>,
    headers: Vec<(String, String)>,
    body_read: fn(i64, usize) -> Result<Option<Vec<u8>>, JetHTTPError>,
    body_close: fn(i64),
    protocol: String,
    remote_address: String,
    redirect_history: Vec<String>,
    timings_ms: Vec<i64>,
    reused_connection: bool,
    raw_content_encoding: Option<String>,
) -> Result<JetHTTPResponse, JetHTTPError> {
    let body_length = match body_length.map(usize::try_from).transpose() {
        Ok(length) => length,
        Err(_) => {
            body_close(body_handle);
            return Err(JetHTTPError::InvalidFraming);
        }
    };
    if headers
        .iter()
        .any(|(name, value)| !JetHTTPHeaders::valid_name(name) || !JetHTTPHeaders::valid_value(value))
    {
        body_close(body_handle);
        return Err(JetHTTPError::InvalidHeader);
    }
    Ok(JetHTTPResponse {
        status,
        version: "HTTP/1.1".to_string(),
        body: JetHTTPBody::bridge(body_handle, body_length, body_read, body_close),
        headers: JetHTTPHeaders { entries: headers },
        trailers: JetHTTPHeaders::new(),
        head_content_length: None,
        suppress_body: false,
        protocol,
        remote_address,
        redirect_history,
        timings_ms,
        reused_connection,
        raw_content_encoding,
    })
}
fn jet_http_client_response_body(resp: &JetHTTPResponse) -> JetHTTPBody {
    resp.body.clone()
}
fn jet_http_client_response_header(resp: &JetHTTPResponse, name: &String) -> JetOutcome<String, JetAbsent> {
    jet_outcome_of(resp.headers.get(name).cloned())
}

fn jet_http_response_cookies(resp: &JetHTTPResponse) -> Vec<String> {
    resp.headers
        .all("set-cookie")
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn jet_http_client_response_protocol(resp: &JetHTTPResponse) -> String {
    resp.protocol.clone()
}
fn jet_http_client_response_remote_address(resp: &JetHTTPResponse) -> String {
    resp.remote_address.clone()
}
fn jet_http_client_response_redirect_history(resp: &JetHTTPResponse) -> Vec<String> {
    resp.redirect_history.clone()
}
fn jet_http_client_response_timings(resp: &JetHTTPResponse) -> Vec<i64> {
    resp.timings_ms.clone()
}
fn jet_http_client_response_reused(resp: &JetHTTPResponse) -> bool {
    resp.reused_connection
}
fn jet_http_client_response_raw_encoding(resp: &JetHTTPResponse) -> Option<String> {
    resp.raw_content_encoding.clone()
}

// The prepared transport namespace differs between generated AOT code and
// resident hosts. Both instantiate the same response, error and send adapter.
macro_rules! jet_http_client_bridge {
    ($bridge:ident) => {
        fn native_http_error(error: $bridge::JetHTTPBridgeError) -> JetHTTPError {
            match error {
                $bridge::JetHTTPBridgeError::InvalidUrl => JetHTTPError::InvalidUrl,
                $bridge::JetHTTPBridgeError::InvalidHeader => JetHTTPError::InvalidHeader,
                $bridge::JetHTTPBridgeError::InvalidFraming => JetHTTPError::InvalidFraming,
                $bridge::JetHTTPBridgeError::UnsupportedEncoding => JetHTTPError::UnsupportedEncoding,
                $bridge::JetHTTPBridgeError::Resolve => JetHTTPError::Resolve { host: "<redacted>".into() },
                $bridge::JetHTTPBridgeError::Connect => JetHTTPError::Connect { address: "<redacted>".into() },
                $bridge::JetHTTPBridgeError::TLS => JetHTTPError::TLS { stage: "handshake".into() },
                $bridge::JetHTTPBridgeError::Timeout => JetHTTPError::Timeout { phase: "transport".into() },
                $bridge::JetHTTPBridgeError::Proxy => JetHTTPError::Proxy { stage: "transport".into() },
                $bridge::JetHTTPBridgeError::Redirect => JetHTTPError::Redirect { reason: "limit".into() },
                $bridge::JetHTTPBridgeError::Protocol => JetHTTPError::Protocol { version: "unsupported".into() },
                $bridge::JetHTTPBridgeError::IO => JetHTTPError::IO { operation: "transport".into() },
                $bridge::JetHTTPBridgeError::ResourceUnavailable => JetHTTPError::ResourceUnavailable { resource: "transport".into() },
                $bridge::JetHTTPBridgeError::Cancelled => JetHTTPError::Cancelled,
                $bridge::JetHTTPBridgeError::UnsupportedTarget => JetHTTPError::UnsupportedTarget { operation: JetHTTPOperation::ClientConnect },
                $bridge::JetHTTPBridgeError::Internal => JetHTTPError::Internal { incident_id: "http-transport".into() },
            }
        }

        fn native_http_body_read(
            handle: i64,
            max_chunk: usize,
        ) -> Result<Option<Vec<u8>>, JetHTTPError> {
            $bridge::jet_http_client_body_read_impl(handle, max_chunk).map_err(native_http_error)
        }

        fn native_http_body_close(handle: i64) {
            $bridge::jet_http_client_body_close_impl(handle);
        }

        fn native_http_response(
            result: Result<$bridge::JetHTTPResponseParts, $bridge::JetHTTPBridgeError>,
        ) -> Result<JetHTTPResponse, JetHTTPError> {
            let parts = result.map_err(native_http_error)?;
            let context = parts.context;
            jet_http_client_response_new(
                parts.status,
                parts.body_handle,
                parts.body_length,
                parts.headers,
                native_http_body_read,
                native_http_body_close,
                context.protocol,
                context.remote_address,
                context.redirect_history,
                context.timings_ms.to_vec(),
                context.reused_connection,
                context.raw_content_encoding,
            )
        }
        fn native_http_request(req: JetHTTPRequest) -> Result<JetHTTPResponse, JetHTTPError> {
            if let Some(error) = req.header_error.as_ref() {
                return Err(error.clone());
            }
            let JetHTTPRequest {
                method,
                url,
                headers,
                body,
                body_set,
                timeout_ms,
                connect_timeout_ms,
                read_timeout_ms,
                total_timeout_ms,
                dns_timeout_ms,
                tls_timeout_ms,
                write_timeout_ms,
                first_byte_timeout_ms,
                redirects,
                proxy,
                cookies,
                form,
                multipart,
                ..
            } = req;
            let body = if body_set {
                Some(body.bytes(8 * 1024 * 1024)?)
            } else {
                None
            };
            native_http_response($bridge::jet_http_client_send_owned_impl(
                &method,
                &url,
                headers.entries,
                body,
                timeout_ms,
                connect_timeout_ms,
                read_timeout_ms,
                total_timeout_ms,
                dns_timeout_ms,
                tls_timeout_ms,
                write_timeout_ms,
                first_byte_timeout_ms,
                redirects,
                proxy.as_deref(),
                &cookies,
                &form,
                &multipart,
            ))
        }

        fn jet_http_client_get(url: &String) -> Result<JetHTTPResponse, JetHTTPError> {
            native_http_response($bridge::jet_http_client_get_parts_impl(url))
        }

        fn jet_http_client_post(url: &String, body: &String) -> Result<JetHTTPResponse, JetHTTPError> {
            native_http_response($bridge::jet_http_client_post_parts_impl(url, body))
        }

        fn jet_http_client_request_send(req: JetHTTPRequest) -> Result<JetHTTPResponse, JetHTTPError> {
            native_http_request(req)
        }
    };
}
