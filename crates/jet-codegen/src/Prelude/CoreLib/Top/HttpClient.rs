// ── D-HTTPLIB2=B / D-HTTPLIB4=B: core.http.client — request builder ─────────
// JetHttpClientReq and JetHttpClientResp live here (in the generated program's
// crate) so they're accessible without cross-crate type imports. The ureq
// bridge functions use only primitive types (i64, String, Vec<String>) and are
// called through wrappers here. This is the I6-safe pattern.

fn jet_http_client_request_new(method: &String, url: &String) -> JetHttpClientReq {
    JetHttpClientReq {
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
    mut req: JetHttpClientReq,
    name: &String,
    value: &String,
) -> JetHttpClientReq {
    if let Err(error) = req.headers.append(name, value) {
        let _ = error;
        req.header_error = Some(JetHttpError::InvalidHeader);
    }
    req
}

fn jet_http_client_request_body(mut req: JetHttpClientReq, body: &String) -> JetHttpClientReq {
    req.body = JetHttpBody::from_text(body.clone());
    req.body_set = true;
    req
}

fn jet_http_client_request_timeout(mut req: JetHttpClientReq, ms: i64) -> JetHttpClientReq {
    req.timeout_ms = Some(ms);
    req
}

fn jet_http_client_request_connect_timeout(
    mut req: JetHttpClientReq,
    ms: i64,
) -> JetHttpClientReq {
    req.connect_timeout_ms = Some(ms);
    req
}

fn jet_http_client_request_read_timeout(mut req: JetHttpClientReq, ms: i64) -> JetHttpClientReq {
    req.read_timeout_ms = Some(ms);
    req
}

fn jet_http_client_request_total_timeout(mut req: JetHttpClientReq, ms: i64) -> JetHttpClientReq {
    req.total_timeout_ms = Some(ms);
    req
}

fn jet_http_client_request_redirects(mut req: JetHttpClientReq, limit: i64) -> JetHttpClientReq {
    req.redirects = Some(limit);
    req
}

fn jet_http_client_request_proxy(mut req: JetHttpClientReq, proxy: &String) -> JetHttpClientReq {
    req.proxy = Some(proxy.clone());
    req
}

fn jet_http_client_request_cookie(
    mut req: JetHttpClientReq,
    name: &String,
    value: &String,
) -> JetHttpClientReq {
    req.cookies.push(name.clone());
    req.cookies.push(value.clone());
    req
}

fn jet_http_client_request_form(
    mut req: JetHttpClientReq,
    name: &String,
    value: &String,
) -> JetHttpClientReq {
    req.form.push(name.clone());
    req.form.push(value.clone());
    req
}

fn jet_http_client_request_multipart_text(
    mut req: JetHttpClientReq,
    name: &String,
    value: &String,
) -> JetHttpClientReq {
    req.multipart.push(name.clone());
    req.multipart.push(value.clone());
    req
}

fn jet_http_client_response_status(resp: &JetHttpClientResp) -> i64 {
    resp.status
}

fn jet_http_client_response_new(
    status: i64,
    body: Vec<u8>,
    headers: Vec<String>,
) -> Result<JetHttpClientResp, JetHttpError> {
    Ok(JetHttpClientResp {
        status,
        version: "HTTP/1.1".to_string(),
        body: JetHttpBody::from_bytes(body),
        headers: JetHttpHeaders::from_flat(headers).map_err(|_| JetHttpError::InvalidHeader)?,
        trailers: JetHttpHeaders::new(),
        head_content_length: None,
    })
}
fn jet_http_client_response_body(resp: &JetHttpClientResp) -> JetHttpBody {
    resp.body.clone()
}
fn jet_http_client_response_header(resp: &JetHttpClientResp, name: &String) -> Option<String> {
    resp.headers.get(name).cloned()
}

fn jet_http_client_response_cookies(resp: &JetHttpClientResp) -> Vec<String> {
    resp.headers
        .all("set-cookie")
        .into_iter()
        .map(str::to_string)
        .collect()
}
