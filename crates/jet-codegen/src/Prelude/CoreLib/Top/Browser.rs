// D-BROWSER-AUTO1=A: native std-only WebDriver BiDi protocol core.
//
// Profiles pin the wire contract. Contexts are isolated BiDi user contexts.
// Traces keep method/sequence facts only: endpoints, parameters, results, event
// payloads, and page data never enter the trace.

const JET_BROWSER_TRACE_LIMIT: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetBrowserError {
    kind: &'static str,
}

impl JetBrowserError {
    fn new(kind: &'static str) -> Self {
        Self { kind }
    }
}

impl JetShow for JetBrowserError {
    fn jet_show(&self) -> String {
        match self.kind {
            "invalid profile" => "browser profile is not supported",
            "invalid timeout" => "browser timeout must be between 1 and 600000 milliseconds",
            "unsupported protocol" => "browser protocol is not available in this session",
            "timeout" => "browser operation timed out",
            "closed" => "browser session is closed",
            "transport" => "browser transport failed",
            _ => "browser protocol failed",
        }
        .to_string()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetBrowserProfile {
    name: String,
    version: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct JetBrowserTimeout {
    milliseconds: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetBrowserCapabilities {
    bidi: bool,
    cdp: bool,
    profile: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetBrowserEvent {
    method: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetBrowserTrace {
    entries: Vec<String>,
}

struct JetBrowserState {
    conn: JetWsConn,
    next_id: i64,
    closed: bool,
    timeout_ms: i64,
    profile: String,
    cdp: bool,
    trace: Vec<String>,
    events: std::collections::VecDeque<JetBrowserEvent>,
}

#[derive(Clone)]
struct JetBrowser {
    state: std::rc::Rc<std::cell::RefCell<JetBrowserState>>,
}

#[derive(Clone)]
struct JetBrowserContext {
    state: std::rc::Rc<JetBrowserContextState>,
}

struct JetBrowserContextState {
    browser: JetBrowser,
    id: String,
    closed: std::cell::Cell<bool>,
}

#[derive(Clone)]
struct JetBrowserPage {
    state: std::rc::Rc<JetBrowserPageState>,
}

struct JetBrowserPageState {
    browser: JetBrowser,
    context: std::rc::Rc<JetBrowserContextState>,
    id: String,
    closed: std::cell::Cell<bool>,
}

#[derive(Clone)]
struct JetBrowserLocator {
    page: std::rc::Rc<JetBrowserPageState>,
    role: String,
    name: String,
}

#[derive(Clone)]
struct JetBrowserProtocol {
    browser: JetBrowser,
    kind: String,
}

fn jet_browser_trace_push(state: &mut JetBrowserState, entry: String) {
    if state.trace.len() == JET_BROWSER_TRACE_LIMIT {
        state.trace.remove(0);
    }
    state.trace.push(entry);
}

fn jet_browser_object(entries: Vec<(&str, jet_std::Json)>) -> jet_std::Json {
    jet_std::Json::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

fn jet_browser_text(value: &str) -> jet_std::Json {
    jet_std::Json::Text(value.to_string())
}

fn jet_browser_get<'a>(value: &'a jet_std::Json, key: &str) -> Option<&'a jet_std::Json> {
    match value {
        jet_std::Json::Object(fields) => fields.get(key),
        _ => None,
    }
}

fn jet_browser_string(value: &jet_std::Json, key: &str) -> Result<String, JetBrowserError> {
    match jet_browser_get(value, key) {
        Some(jet_std::Json::Text(text)) => Ok(text.clone()),
        _ => Err(JetBrowserError::new("protocol")),
    }
}

fn jet_browser_id(value: &jet_std::Json) -> Option<i64> {
    match jet_browser_get(value, "id") {
        Some(jet_std::Json::Number(value))
            if value.is_finite()
                && value.fract() == 0.0
                && *value >= 0.0
                && *value <= i64::MAX as f64 =>
        {
            Some(*value as i64)
        }
        _ => None,
    }
}

fn jet_browser_ws_error(error: JetWsError) -> JetBrowserError {
    match error {
        JetWsError::Timeout => JetBrowserError::new("timeout"),
        JetWsError::Closed => JetBrowserError::new("closed"),
        _ => JetBrowserError::new("transport"),
    }
}

fn jet_browser_set_timeout(conn: &JetWsConn, milliseconds: i64) -> Result<(), JetBrowserError> {
    let duration = Some(std::time::Duration::from_millis(milliseconds as u64));
    let stream = conn.stream.borrow();
    stream
        .set_read_timeout(duration)
        .map_err(|_| JetBrowserError::new("transport"))?;
    stream
        .set_write_timeout(duration)
        .map_err(|_| JetBrowserError::new("transport"))
}

fn jet_browser_parse_message(text: &str) -> Result<jet_std::Json, JetBrowserError> {
    let value =
        jet_std::parse_json_strict(text).map_err(|_| JetBrowserError::new("protocol"))?;
    if matches!(value, jet_std::Json::Object(_)) {
        Ok(value)
    } else {
        Err(JetBrowserError::new("protocol"))
    }
}

fn jet_browser_recv_json(state: &mut JetBrowserState) -> Result<jet_std::Json, JetBrowserError> {
    let message = jet_ws_recv(&state.conn).map_err(jet_browser_ws_error)?;
    let text = jet_ws_message_text(&message).map_err(jet_browser_ws_error)?;
    jet_browser_parse_message(&text)
}

fn jet_browser_capture_event(
    state: &mut JetBrowserState,
    value: &jet_std::Json,
) -> Result<bool, JetBrowserError> {
    let Some(jet_std::Json::Text(kind)) = jet_browser_get(value, "type") else {
        return Err(JetBrowserError::new("protocol"));
    };
    if kind != "event" {
        return Ok(false);
    }
    let method = jet_browser_string(value, "method")?;
    jet_browser_trace_push(state, format!("event:{method}"));
    state.events.push_back(JetBrowserEvent { method });
    Ok(true)
}

fn jet_browser_command(
    browser: &JetBrowser,
    method: &str,
    params: jet_std::Json,
) -> Result<jet_std::Json, JetBrowserError> {
    let mut state = browser.state.borrow_mut();
    if state.closed {
        return Err(JetBrowserError::new("closed"));
    }
    jet_browser_set_timeout(&state.conn, state.timeout_ms)?;
    let id = state.next_id;
    state.next_id += 1;
    let request = jet_browser_object(vec![
        ("id", jet_std::Json::Number(id as f64)),
        ("method", jet_browser_text(method)),
        ("params", params),
    ]);
    let text = jet_std::render_json(&request, false, 0);
    jet_browser_trace_push(&mut state, format!("send:{id}:{method}"));
    jet_ws_send_text(&state.conn, &text).map_err(jet_browser_ws_error)?;
    loop {
        let response = jet_browser_recv_json(&mut state)?;
        if jet_browser_capture_event(&mut state, &response)? {
            continue;
        }
        if jet_browser_id(&response) != Some(id) {
            return Err(JetBrowserError::new("protocol"));
        }
        let response_type = jet_browser_string(&response, "type")?;
        if response_type == "error" || jet_browser_get(&response, "error").is_some() {
            jet_browser_trace_push(&mut state, format!("error:{id}:{method}"));
            return Err(JetBrowserError::new("protocol"));
        }
        if response_type != "success" {
            return Err(JetBrowserError::new("protocol"));
        }
        let Some(result) = jet_browser_get(&response, "result").cloned() else {
            return Err(JetBrowserError::new("protocol"));
        };
        jet_browser_trace_push(&mut state, format!("recv:{id}:{method}"));
        return Ok(result);
    }
}

fn jet_browser_profile(name: &String) -> Result<JetBrowserProfile, JetBrowserError> {
    let version = match name.as_str() {
        "bidi-2025.5" => "2025.5",
        "bidi-2024.11" => "2024.11",
        _ => return Err(JetBrowserError::new("invalid profile")),
    };
    Ok(JetBrowserProfile {
        name: name.clone(),
        version: version.to_string(),
    })
}

fn jet_browser_timeout(milliseconds: i64) -> Result<JetBrowserTimeout, JetBrowserError> {
    if !(1..=600_000).contains(&milliseconds) {
        return Err(JetBrowserError::new("invalid timeout"));
    }
    Ok(JetBrowserTimeout { milliseconds })
}

fn jet_browser_connect(endpoint: &String) -> Result<JetBrowser, JetBrowserError> {
    let profile = jet_browser_profile(&"bidi-2025.5".to_string())?;
    let timeout = JetBrowserTimeout {
        milliseconds: 30_000,
    };
    jet_browser_connect_profile(endpoint, &profile, timeout)
}

fn jet_browser_connect_profile(
    endpoint: &String,
    profile: &JetBrowserProfile,
    timeout: JetBrowserTimeout,
) -> Result<JetBrowser, JetBrowserError> {
    let conn = jet_ws_connect(endpoint).map_err(jet_browser_ws_error)?;
    jet_browser_set_timeout(&conn, timeout.milliseconds)?;
    let browser = JetBrowser {
        state: std::rc::Rc::new(std::cell::RefCell::new(JetBrowserState {
            conn,
            next_id: 1,
            closed: false,
            timeout_ms: timeout.milliseconds,
            profile: profile.name.clone(),
            cdp: false,
            trace: vec!["connect:<redacted>".to_string()],
            events: std::collections::VecDeque::new(),
        })),
    };
    let status = jet_browser_command(
        &browser,
        "session.status",
        jet_browser_object(Vec::new()),
    )?;
    let cdp = jet_browser_get(&status, "capabilities")
        .and_then(|caps| jet_browser_get(caps, "goog:cdp"))
        .is_some_and(|value| matches!(value, jet_std::Json::Boolean(true)));
    browser.state.borrow_mut().cdp = cdp;
    Ok(browser)
}

fn jet_browser_capabilities(browser: &JetBrowser) -> JetBrowserCapabilities {
    let state = browser.state.borrow();
    JetBrowserCapabilities {
        bidi: true,
        cdp: state.cdp,
        profile: state.profile.clone(),
    }
}

fn jet_browser_context(browser: &JetBrowser) -> Result<JetBrowserContext, JetBrowserError> {
    let result = jet_browser_command(
        browser,
        "browser.createUserContext",
        jet_browser_object(Vec::new()),
    )?;
    Ok(JetBrowserContext {
        state: std::rc::Rc::new(JetBrowserContextState {
            browser: browser.clone(),
            id: jet_browser_string(&result, "userContext")?,
            closed: std::cell::Cell::new(false),
        }),
    })
}

fn jet_browser_subscribe(
    browser: &JetBrowser,
    event: &String,
) -> Result<(), JetBrowserError> {
    let events = jet_std::Json::Array(vec![jet_browser_text(event)]);
    jet_browser_command(
        browser,
        "session.subscribe",
        jet_browser_object(vec![("events", events)]),
    )
    .map(|_| ())
}

fn jet_browser_next_event(
    browser: &JetBrowser,
    timeout: JetBrowserTimeout,
) -> Result<JetBrowserEvent, JetBrowserError> {
    let mut state = browser.state.borrow_mut();
    if state.closed {
        return Err(JetBrowserError::new("closed"));
    }
    if let Some(event) = state.events.pop_front() {
        return Ok(event);
    }
    jet_browser_set_timeout(&state.conn, timeout.milliseconds)?;
    loop {
        let value = jet_browser_recv_json(&mut state)?;
        if jet_browser_capture_event(&mut state, &value)? {
            return state
                .events
                .pop_front()
                .ok_or_else(|| JetBrowserError::new("protocol"));
        }
        return Err(JetBrowserError::new("protocol"));
    }
}

fn jet_browser_protocol(
    browser: &JetBrowser,
    kind: &String,
) -> Result<JetBrowserProtocol, JetBrowserError> {
    let supported = match kind.as_str() {
        "bidi" => true,
        "cdp" => browser.state.borrow().cdp,
        _ => false,
    };
    if !supported {
        return Err(JetBrowserError::new("unsupported protocol"));
    }
    Ok(JetBrowserProtocol {
        browser: browser.clone(),
        kind: kind.clone(),
    })
}

fn jet_browser_trace(browser: &JetBrowser) -> JetBrowserTrace {
    JetBrowserTrace {
        entries: browser.state.borrow().trace.clone(),
    }
}

fn jet_browser_close(browser: &JetBrowser) -> Result<(), JetBrowserError> {
    if browser.state.borrow().closed {
        return Ok(());
    }
    jet_browser_command(browser, "session.end", jet_browser_object(Vec::new()))?;
    browser.state.borrow_mut().closed = true;
    Ok(())
}

impl Drop for JetBrowser {
    fn drop(&mut self) {
        if std::rc::Rc::strong_count(&self.state) == 1 {
            let _ = jet_browser_close(self);
        }
    }
}

fn jet_browser_context_page(
    context: &JetBrowserContext,
) -> Result<JetBrowserPage, JetBrowserError> {
    if context.state.closed.get() {
        return Err(JetBrowserError::new("closed"));
    }
    let result = jet_browser_command(
        &context.state.browser,
        "browsingContext.create",
        jet_browser_object(vec![
            ("type", jet_browser_text("tab")),
            ("userContext", jet_browser_text(&context.state.id)),
        ]),
    )?;
    Ok(JetBrowserPage {
        state: std::rc::Rc::new(JetBrowserPageState {
            browser: context.state.browser.clone(),
            context: context.state.clone(),
            id: jet_browser_string(&result, "context")?,
            closed: std::cell::Cell::new(false),
        }),
    })
}

fn jet_browser_context_close(context: &JetBrowserContext) -> Result<(), JetBrowserError> {
    jet_browser_context_state_close(&context.state)
}

fn jet_browser_context_state_close(
    context: &JetBrowserContextState,
) -> Result<(), JetBrowserError> {
    if context.closed.get() {
        return Ok(());
    }
    jet_browser_command(
        &context.browser,
        "browser.removeUserContext",
        jet_browser_object(vec![("userContext", jet_browser_text(&context.id))]),
    )?;
    context.closed.set(true);
    Ok(())
}

impl Drop for JetBrowserContextState {
    fn drop(&mut self) {
        let _ = jet_browser_context_state_close(self);
    }
}

fn jet_browser_page_goto(
    page: &JetBrowserPage,
    url: &String,
) -> Result<(), JetBrowserError> {
    if page.state.closed.get() || page.state.context.closed.get() {
        return Err(JetBrowserError::new("closed"));
    }
    jet_browser_command(
        &page.state.browser,
        "browsingContext.navigate",
        jet_browser_object(vec![
            ("context", jet_browser_text(&page.state.id)),
            ("url", jet_browser_text(url)),
            ("wait", jet_browser_text("complete")),
        ]),
    )
    .map(|_| ())
}

fn jet_browser_page_get_by_role(
    page: &JetBrowserPage,
    role: &String,
    name: &String,
) -> JetBrowserLocator {
    JetBrowserLocator {
        page: page.state.clone(),
        role: role.clone(),
        name: name.clone(),
    }
}

fn jet_browser_page_close(page: &JetBrowserPage) -> Result<(), JetBrowserError> {
    jet_browser_page_state_close(&page.state)
}

fn jet_browser_page_state_close(page: &JetBrowserPageState) -> Result<(), JetBrowserError> {
    if page.closed.get() {
        return Ok(());
    }
    jet_browser_command(
        &page.browser,
        "browsingContext.close",
        jet_browser_object(vec![("context", jet_browser_text(&page.id))]),
    )?;
    page.closed.set(true);
    Ok(())
}

impl Drop for JetBrowserPageState {
    fn drop(&mut self) {
        let _ = jet_browser_page_state_close(self);
    }
}

fn jet_browser_locator_query(
    locator: &JetBrowserLocator,
) -> Result<Option<String>, JetBrowserError> {
    let value = jet_browser_object(vec![
        ("role", jet_browser_text(&locator.role)),
        ("name", jet_browser_text(&locator.name)),
    ]);
    let result = jet_browser_command(
        &locator.page.browser,
        "browsingContext.locateNodes",
        jet_browser_object(vec![
            ("context", jet_browser_text(&locator.page.id)),
            (
                "locator",
                jet_browser_object(vec![
                    ("type", jet_browser_text("accessibility")),
                    ("value", value),
                ]),
            ),
            ("maxNodeCount", jet_std::Json::Number(1.0)),
        ]),
    )?;
    let Some(jet_std::Json::Array(nodes)) = jet_browser_get(&result, "nodes") else {
        return Err(JetBrowserError::new("protocol"));
    };
    match nodes.first() {
        None => Ok(None),
        Some(node) => Ok(Some(jet_browser_string(node, "sharedId")?)),
    }
}

fn jet_browser_locator_wait(
    locator: &JetBrowserLocator,
    timeout: JetBrowserTimeout,
) -> Result<(), JetBrowserError> {
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_millis(timeout.milliseconds as u64);
    loop {
        if jet_browser_locator_query(locator)?.is_some() {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(JetBrowserError::new("timeout"));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn jet_browser_locator_click(locator: &JetBrowserLocator) -> Result<(), JetBrowserError> {
    let shared_id = jet_browser_locator_query(locator)?
        .ok_or_else(|| JetBrowserError::new("timeout"))?;
    let origin = jet_browser_object(vec![
        ("type", jet_browser_text("element")),
        (
            "element",
            jet_browser_object(vec![("sharedId", jet_browser_text(&shared_id))]),
        ),
    ]);
    let actions = jet_std::Json::Array(vec![
        jet_browser_object(vec![
            ("type", jet_browser_text("pointerMove")),
            ("x", jet_std::Json::Number(0.0)),
            ("y", jet_std::Json::Number(0.0)),
            ("origin", origin),
        ]),
        jet_browser_object(vec![
            ("type", jet_browser_text("pointerDown")),
            ("button", jet_std::Json::Number(0.0)),
        ]),
        jet_browser_object(vec![
            ("type", jet_browser_text("pointerUp")),
            ("button", jet_std::Json::Number(0.0)),
        ]),
    ]);
    let source = jet_browser_object(vec![
        ("type", jet_browser_text("pointer")),
        ("id", jet_browser_text("mouse")),
        (
            "parameters",
            jet_browser_object(vec![("pointerType", jet_browser_text("mouse"))]),
        ),
        ("actions", actions),
    ]);
    jet_browser_command(
        &locator.page.browser,
        "input.performActions",
        jet_browser_object(vec![
            ("context", jet_browser_text(&locator.page.id)),
            ("actions", jet_std::Json::Array(vec![source])),
        ]),
    )
    .map(|_| ())
}

fn jet_browser_event_kind(event: &JetBrowserEvent) -> String {
    event.method.clone()
}

fn jet_browser_protocol_send(
    protocol: &JetBrowserProtocol,
    method: &String,
    params_json: &String,
) -> Result<String, JetBrowserError> {
    let params =
        jet_std::parse_json_strict(params_json).map_err(|_| JetBrowserError::new("protocol"))?;
    if !matches!(params, jet_std::Json::Object(_)) {
        return Err(JetBrowserError::new("protocol"));
    }
    let result = if protocol.kind == "bidi" {
        jet_browser_command(&protocol.browser, method, params)?
    } else {
        jet_browser_command(
            &protocol.browser,
            "goog:cdp.sendCommand",
            jet_browser_object(vec![
                ("method", jet_browser_text(method)),
                ("params", params),
            ]),
        )?
    };
    Ok(jet_std::render_json(&result, false, 0))
}

fn jet_browser_capabilities_bidi(caps: &JetBrowserCapabilities) -> bool {
    caps.bidi
}

fn jet_browser_capabilities_cdp(caps: &JetBrowserCapabilities) -> bool {
    caps.cdp
}

fn jet_browser_capabilities_profile(caps: &JetBrowserCapabilities) -> String {
    caps.profile.clone()
}

fn jet_browser_trace_entry_count(trace: &JetBrowserTrace) -> i64 {
    trace.entries.len() as i64
}

fn jet_browser_trace_redacted(_trace: &JetBrowserTrace) -> bool {
    true
}

fn jet_browser_trace_summary(trace: &JetBrowserTrace) -> String {
    trace.entries.join(",")
}
