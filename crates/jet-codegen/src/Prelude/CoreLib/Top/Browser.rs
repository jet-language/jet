// D-BROWSER-AUTO1=A: native std-only WebDriver BiDi protocol core.
//
// Profiles pin the wire contract. Contexts are isolated BiDi user contexts.
// Traces keep method/sequence facts only: endpoints, parameters, results, event
// payloads, and page data never enter the trace.

const JET_BROWSER_TRACE_LIMIT_BYTES: usize = 8 * 1024;
const JET_BROWSER_EVENT_LIMIT: usize = 256;

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
            "missing lock" => "no locked browser for this engine in .jet/lock",
            "unknown engine" => "browser engine is not supported",
            "missing binary" => "locked browser binary was not found",
            "size mismatch" => "locked browser binary size drifted",
            "invalid lock" => "browser lock entry is incomplete",
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

/// D-BROWSER-AUTO1=A (#1187): project-locked browser binary for later launch.
#[derive(Clone, Debug, PartialEq, Eq)]
struct JetBrowserLocked {
    engine: String,
    version: String,
    binary: String,
    protocol: String,
    size: i64,
    output_hash: String,
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
    session_started: bool,
    timeout_ms: i64,
    profile: String,
    cdp: bool,
    trace: Vec<String>,
    trace_bytes: usize,
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

/// D-BROWSER-AUTO1=A (#1188): browsing-context frame handle (main or child).
/// Explicit close only — listing frames must not close children on drop.
#[derive(Clone)]
struct JetBrowserFrame {
    state: std::rc::Rc<JetBrowserFrameState>,
}

struct JetBrowserFrameState {
    browser: JetBrowser,
    page: std::rc::Rc<JetBrowserPageState>,
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
    if entry.len() > JET_BROWSER_TRACE_LIMIT_BYTES {
        return;
    }
    while state.trace_bytes.saturating_add(entry.len()) > JET_BROWSER_TRACE_LIMIT_BYTES {
        if state.trace.is_empty() {
            break;
        }
        state.trace_bytes = state.trace_bytes.saturating_sub(state.trace.remove(0).len());
    }
    state.trace_bytes += entry.len();
    state.trace.push(entry);
}

fn jet_browser_fact_hash(value: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn jet_browser_object(entries: Vec<(&str, jet_std::JSON)>) -> jet_std::JSON {
    jet_std::JSON::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

fn jet_browser_text(value: &str) -> jet_std::JSON {
    jet_std::JSON::Text(value.to_string())
}

fn jet_browser_get<'a>(value: &'a jet_std::JSON, key: &str) -> Option<&'a jet_std::JSON> {
    match value {
        jet_std::JSON::Object(fields) => fields.get(key),
        _ => None,
    }
}

fn jet_browser_string(value: &jet_std::JSON, key: &str) -> Result<String, JetBrowserError> {
    match jet_browser_get(value, key) {
        Some(jet_std::JSON::Text(text)) => Ok(text.clone()),
        _ => Err(JetBrowserError::new("protocol")),
    }
}

fn jet_browser_id(value: &jet_std::JSON) -> Option<i64> {
    match jet_browser_get(value, "id") {
        Some(jet_std::JSON::Number(value))
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

fn jet_browser_parse_message(text: &str) -> Result<jet_std::JSON, JetBrowserError> {
    let value =
        jet_std::parse_json_strict(text).map_err(|_| JetBrowserError::new("protocol"))?;
    if matches!(value, jet_std::JSON::Object(_)) {
        Ok(value)
    } else {
        Err(JetBrowserError::new("protocol"))
    }
}

fn jet_browser_remaining_ms(deadline: std::time::Instant) -> Result<i64, JetBrowserError> {
    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
    if remaining.is_zero() {
        return Err(JetBrowserError::new("timeout"));
    }
    Ok(i64::try_from(remaining.as_millis().max(1)).unwrap_or(i64::MAX))
}

fn jet_browser_recv_json(
    state: &mut JetBrowserState,
    deadline: std::time::Instant,
) -> Result<jet_std::JSON, JetBrowserError> {
    jet_browser_set_timeout(&state.conn, jet_browser_remaining_ms(deadline)?)?;
    let message = jet_ws_recv(&state.conn).map_err(jet_browser_ws_error)?;
    let text = jet_ws_message_text(&message).map_err(jet_browser_ws_error)?;
    jet_browser_parse_message(&text)
}

fn jet_browser_capture_event(
    state: &mut JetBrowserState,
    value: &jet_std::JSON,
) -> Result<bool, JetBrowserError> {
    let Some(jet_std::JSON::Text(kind)) = jet_browser_get(value, "type") else {
        return Err(JetBrowserError::new("protocol"));
    };
    if kind != "event" {
        return Ok(false);
    }
    let method = jet_browser_string(value, "method")?;
    jet_browser_trace_push(state, format!("event:{}", jet_browser_fact_hash(&method)));
    if state.events.len() == JET_BROWSER_EVENT_LIMIT {
        state.events.pop_front();
    }
    state.events.push_back(JetBrowserEvent { method });
    Ok(true)
}

fn jet_browser_profile_allows(profile: &str, method: &str) -> bool {
    const BIDI_2024_11: &[&str] = &[
        "session.status",
        "session.new",
        "session.end",
        "session.subscribe",
        "browser.createUserContext",
        "browser.removeUserContext",
        "browsingContext.create",
        "browsingContext.close",
        "browsingContext.getTree",
        "browsingContext.navigate",
        "browsingContext.reload",
        "browsingContext.locateNodes",
        "browsingContext.captureScreenshot",
        "input.performActions",
        "input.releaseActions",
        "input.setFiles",
        "network.addIntercept",
        "network.continueRequest",
        "network.failRequest",
        "network.provideResponse",
        "network.removeIntercept",
        "script.callFunction",
        "script.disown",
        "script.evaluate",
        "script.getRealms",
        "goog:cdp.sendCommand",
    ];
    match profile {
        "bidi-2024.11" => BIDI_2024_11.contains(&method),
        "bidi-2025.5" => {
            BIDI_2024_11.contains(&method)
                || matches!(
                    method,
                    "permissions.setPermission"
                        | "webExtension.install"
                        | "webExtension.uninstall"
                )
        }
        _ => false,
    }
}

fn jet_browser_command_with_timeout(
    browser: &JetBrowser,
    method: &str,
    params: jet_std::JSON,
    timeout_ms: i64,
) -> Result<jet_std::JSON, JetBrowserError> {
    let mut state = browser.state.borrow_mut();
    if state.closed {
        return Err(JetBrowserError::new("closed"));
    }
    if !jet_browser_profile_allows(&state.profile, method) {
        return Err(JetBrowserError::new("unsupported protocol"));
    }
    if method == "goog:cdp.sendCommand" && !state.cdp {
        return Err(JetBrowserError::new("unsupported protocol"));
    }
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms as u64);
    jet_browser_set_timeout(&state.conn, jet_browser_remaining_ms(deadline)?)?;
    let id = state.next_id;
    state.next_id += 1;
    let request = jet_browser_object(vec![
        ("id", jet_std::JSON::Number(id as f64)),
        ("method", jet_browser_text(method)),
        ("params", params),
    ]);
    let text = jet_std::render_json(&request, false, 0);
    let method_hash = jet_browser_fact_hash(method);
    jet_browser_trace_push(&mut state, format!("send:{id}:{method_hash}"));
    jet_ws_send_text(&state.conn, &text).map_err(jet_browser_ws_error)?;
    loop {
        let response = jet_browser_recv_json(&mut state, deadline)?;
        if jet_browser_capture_event(&mut state, &response)? {
            continue;
        }
        if jet_browser_id(&response) != Some(id) {
            return Err(JetBrowserError::new("protocol"));
        }
        let response_type = jet_browser_string(&response, "type")?;
        if response_type == "error" || jet_browser_get(&response, "error").is_some() {
            jet_browser_trace_push(&mut state, format!("error:{id}:{method_hash}"));
            return Err(JetBrowserError::new("protocol"));
        }
        if response_type != "success" {
            return Err(JetBrowserError::new("protocol"));
        }
        let Some(result) = jet_browser_get(&response, "result").cloned() else {
            return Err(JetBrowserError::new("protocol"));
        };
        jet_browser_trace_push(&mut state, format!("recv:{id}:{method_hash}"));
        return Ok(result);
    }
}

fn jet_browser_command(
    browser: &JetBrowser,
    method: &str,
    params: jet_std::JSON,
) -> Result<jet_std::JSON, JetBrowserError> {
    let timeout_ms = browser.state.borrow().timeout_ms;
    jet_browser_command_with_timeout(browser, method, params, timeout_ms)
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

fn jet_browser_project_root() -> String {
    std::env::var("JET_PROJECT_ROOT").unwrap_or_else(|_| ".".to_string())
}

fn jet_browser_parse_locked(raw: &str, engine: &str) -> Result<JetBrowserLocked, JetBrowserError> {
    let mut current: Option<JetBrowserLocked> = None;
    let mut in_browser = false;
    let mut found: Option<JetBrowserLocked> = None;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            if in_browser {
                if let Some(entry) = current.take() {
                    if entry.engine == engine {
                        found = Some(entry);
                    }
                }
            }
            in_browser = line == "[[browser]]";
            if in_browser {
                current = Some(JetBrowserLocked {
                    engine: String::new(),
                    version: String::new(),
                    binary: String::new(),
                    protocol: String::new(),
                    size: 0,
                    output_hash: String::new(),
                });
            }
            continue;
        }
        if !in_browser {
            continue;
        }
        let Some(entry) = current.as_mut() else {
            continue;
        };
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let val = val.trim().trim_matches('"');
        match key {
            "engine" => entry.engine = val.to_string(),
            "version" => entry.version = val.to_string(),
            "binary" => entry.binary = val.to_string(),
            "protocol" => entry.protocol = val.to_string(),
            "size" => {
                entry.size = val.parse().map_err(|_| JetBrowserError::new("invalid lock"))?;
            }
            "output-hash" => entry.output_hash = val.to_string(),
            _ => {}
        }
    }
    if in_browser {
        if let Some(entry) = current.take() {
            if entry.engine == engine {
                found = Some(entry);
            }
        }
    }
    let locked = found.ok_or_else(|| JetBrowserError::new("missing lock"))?;
    if locked.engine.is_empty()
        || locked.binary.is_empty()
        || locked.protocol.is_empty()
        || locked.output_hash.is_empty()
    {
        return Err(JetBrowserError::new("invalid lock"));
    }
    Ok(locked)
}

fn jet_browser_locked(engine: &String) -> Result<JetBrowserLocked, JetBrowserError> {
    let engine_name = match engine.as_str() {
        "chromium" | "firefox" | "webkit" => engine.as_str(),
        _ => return Err(JetBrowserError::new("unknown engine")),
    };
    let root = jet_browser_project_root();
    let lock_path = std::path::Path::new(&root).join(".jet/lock");
    let raw = std::fs::read_to_string(&lock_path).map_err(|_| JetBrowserError::new("missing lock"))?;
    let locked = jet_browser_parse_locked(&raw, engine_name)?;
    let path = std::path::Path::new(&locked.binary);
    if !path.is_file() {
        return Err(JetBrowserError::new("missing binary"));
    }
    let size = std::fs::metadata(path)
        .map_err(|_| JetBrowserError::new("missing binary"))?
        .len() as i64;
    if size != locked.size {
        return Err(JetBrowserError::new("size mismatch"));
    }
    Ok(locked)
}

fn jet_browser_locked_engine(locked: &JetBrowserLocked) -> String {
    locked.engine.clone()
}

fn jet_browser_locked_version(locked: &JetBrowserLocked) -> String {
    locked.version.clone()
}

fn jet_browser_locked_binary(locked: &JetBrowserLocked) -> String {
    locked.binary.clone()
}

fn jet_browser_locked_protocol(locked: &JetBrowserLocked) -> String {
    locked.protocol.clone()
}

fn jet_browser_locked_verify(locked: &JetBrowserLocked) -> Result<(), JetBrowserError> {
    let path = std::path::Path::new(&locked.binary);
    if !path.is_file() {
        return Err(JetBrowserError::new("missing binary"));
    }
    let size = std::fs::metadata(path)
        .map_err(|_| JetBrowserError::new("missing binary"))?
        .len() as i64;
    if size != locked.size {
        return Err(JetBrowserError::new("size mismatch"));
    }
    Ok(())
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
            session_started: false,
            timeout_ms: timeout.milliseconds,
            profile: profile.name.clone(),
            cdp: false,
            trace: vec!["connect".to_string()],
            trace_bytes: "connect".len(),
            events: std::collections::VecDeque::new(),
        })),
    };
    let status = jet_browser_command(
        &browser,
        "session.status",
        jet_browser_object(Vec::new()),
    )?;
    if !matches!(
        jet_browser_get(&status, "ready"),
        Some(jet_std::JSON::Boolean(true))
    )
        || !matches!(jet_browser_get(&status, "message"), Some(jet_std::JSON::Text(_)))
    {
        return Err(JetBrowserError::new("protocol"));
    }
    let new_session = jet_browser_command(
        &browser,
        "session.new",
        jet_browser_object(vec![(
            "capabilities",
            jet_browser_object(vec![("alwaysMatch", jet_browser_object(Vec::new()))]),
        )]),
    )?;
    browser.state.borrow_mut().session_started = true;
    let _session_id = jet_browser_string(&new_session, "sessionId")?;
    let capabilities = jet_browser_get(&new_session, "capabilities")
        .filter(|value| matches!(value, jet_std::JSON::Object(_)))
        .ok_or_else(|| JetBrowserError::new("protocol"))?;
    let cdp = jet_browser_get(capabilities, "goog:cdp")
        .is_some_and(|value| matches!(value, jet_std::JSON::Boolean(true)));
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
    let events = jet_std::JSON::Array(vec![jet_browser_text(event)]);
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
    let deadline = std::time::Instant::now()
        + std::time::Duration::from_millis(timeout.milliseconds as u64);
    loop {
        let value = jet_browser_recv_json(&mut state, deadline)?;
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
    let state = browser.state.borrow();
    if state.closed {
        return Err(JetBrowserError::new("closed"));
    }
    let supported = match kind.as_str() {
        "bidi" => true,
        "cdp" => state.cdp,
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
    let state = browser.state.borrow();
    if state.closed {
        return Ok(());
    }
    if !state.session_started {
        drop(state);
        browser.state.borrow_mut().closed = true;
        return Ok(());
    }
    drop(state);
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

fn jet_browser_context_create_tab(
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

/// Beginner page handle — one BiDi tab under the isolated user context.
fn jet_browser_context_page(
    context: &JetBrowserContext,
) -> Result<JetBrowserPage, JetBrowserError> {
    jet_browser_context_create_tab(context)
}

/// Explicit tab create — same BiDi browsing context as `page()`.
fn jet_browser_context_tab(
    context: &JetBrowserContext,
) -> Result<JetBrowserPage, JetBrowserError> {
    jet_browser_context_create_tab(context)
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

fn jet_browser_frame_from_page(
    page: &JetBrowserPage,
    id: String,
) -> JetBrowserFrame {
    JetBrowserFrame {
        state: std::rc::Rc::new(JetBrowserFrameState {
            browser: page.state.browser.clone(),
            page: page.state.clone(),
            id,
            closed: std::cell::Cell::new(false),
        }),
    }
}

fn jet_browser_page_main_frame(
    page: &JetBrowserPage,
) -> Result<JetBrowserFrame, JetBrowserError> {
    if page.state.closed.get() || page.state.context.closed.get() {
        return Err(JetBrowserError::new("closed"));
    }
    Ok(jet_browser_frame_from_page(page, page.state.id.clone()))
}

fn jet_browser_collect_frame_ids(
    node: &jet_std::JSON,
    out: &mut Vec<String>,
) -> Result<(), JetBrowserError> {
    out.push(jet_browser_string(node, "context")?);
    match jet_browser_get(node, "children") {
        None => Ok(()),
        Some(jet_std::JSON::Array(children)) => {
            for child in children {
                jet_browser_collect_frame_ids(child, out)?;
            }
            Ok(())
        }
        Some(_) => Err(JetBrowserError::new("protocol")),
    }
}

fn jet_browser_page_frames(
    page: &JetBrowserPage,
) -> Result<Vec<JetBrowserFrame>, JetBrowserError> {
    if page.state.closed.get() || page.state.context.closed.get() {
        return Err(JetBrowserError::new("closed"));
    }
    let result = jet_browser_command(
        &page.state.browser,
        "browsingContext.getTree",
        jet_browser_object(vec![("root", jet_browser_text(&page.state.id))]),
    )?;
    let Some(jet_std::JSON::Array(contexts)) = jet_browser_get(&result, "contexts") else {
        return Err(JetBrowserError::new("protocol"));
    };
    let mut ids = Vec::new();
    for node in contexts {
        jet_browser_collect_frame_ids(node, &mut ids)?;
    }
    Ok(ids
        .into_iter()
        .map(|id| jet_browser_frame_from_page(page, id))
        .collect())
}

fn jet_browser_frame_close(frame: &JetBrowserFrame) -> Result<(), JetBrowserError> {
    if frame.state.closed.get() {
        return Ok(());
    }
    if frame.state.page.closed.get() || frame.state.page.context.closed.get() {
        frame.state.closed.set(true);
        return Ok(());
    }
    jet_browser_command(
        &frame.state.browser,
        "browsingContext.close",
        jet_browser_object(vec![("context", jet_browser_text(&frame.state.id))]),
    )?;
    frame.state.closed.set(true);
    if frame.state.id == frame.state.page.id {
        frame.state.page.closed.set(true);
    }
    Ok(())
}

fn jet_browser_page_close(page: &JetBrowserPage) -> Result<(), JetBrowserError> {
    jet_browser_page_state_close(&page.state)
}

fn jet_browser_page_state_close(page: &JetBrowserPageState) -> Result<(), JetBrowserError> {
    if page.closed.get() {
        return Ok(());
    }
    if page.context.closed.get() {
        page.closed.set(true);
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
    let timeout_ms = locator.page.browser.state.borrow().timeout_ms;
    jet_browser_locator_query_with_timeout(locator, timeout_ms)
}

fn jet_browser_locator_query_with_timeout(
    locator: &JetBrowserLocator,
    timeout_ms: i64,
) -> Result<Option<String>, JetBrowserError> {
    if locator.page.closed.get() || locator.page.context.closed.get() {
        return Err(JetBrowserError::new("closed"));
    }
    let value = jet_browser_object(vec![
        ("role", jet_browser_text(&locator.role)),
        ("name", jet_browser_text(&locator.name)),
    ]);
    let result = jet_browser_command_with_timeout(
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
            ("maxNodeCount", jet_std::JSON::Number(1.0)),
        ]),
        timeout_ms,
    )?;
    let Some(jet_std::JSON::Array(nodes)) = jet_browser_get(&result, "nodes") else {
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
        let remaining = jet_browser_remaining_ms(deadline)?;
        if jet_browser_locator_query_with_timeout(locator, remaining)?.is_some() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn jet_browser_locator_click(locator: &JetBrowserLocator) -> Result<(), JetBrowserError> {
    if locator.page.closed.get() || locator.page.context.closed.get() {
        return Err(JetBrowserError::new("closed"));
    }
    let shared_id = jet_browser_locator_query(locator)?
        .ok_or_else(|| JetBrowserError::new("timeout"))?;
    let origin = jet_browser_object(vec![
        ("type", jet_browser_text("element")),
        (
            "element",
            jet_browser_object(vec![("sharedId", jet_browser_text(&shared_id))]),
        ),
    ]);
    let actions = jet_std::JSON::Array(vec![
        jet_browser_object(vec![
            ("type", jet_browser_text("pointerMove")),
            ("x", jet_std::JSON::Number(0.0)),
            ("y", jet_std::JSON::Number(0.0)),
            ("origin", origin),
        ]),
        jet_browser_object(vec![
            ("type", jet_browser_text("pointerDown")),
            ("button", jet_std::JSON::Number(0.0)),
        ]),
        jet_browser_object(vec![
            ("type", jet_browser_text("pointerUp")),
            ("button", jet_std::JSON::Number(0.0)),
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
            ("actions", jet_std::JSON::Array(vec![source])),
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
    if !matches!(params, jet_std::JSON::Object(_)) {
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

fn jet_browser_trace_redacted(trace: &JetBrowserTrace) -> bool {
    let total: usize = trace.entries.iter().map(String::len).sum();
    total <= JET_BROWSER_TRACE_LIMIT_BYTES
        && trace.entries.iter().all(|entry| {
            if entry == "connect" {
                return true;
            }
            let mut parts = entry.split(':');
            let Some(kind) = parts.next() else {
                return false;
            };
            match kind {
                "event" => parts.next().is_some_and(|hash| {
                    hash.len() == 16
                        && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                        && parts.next().is_none()
                }),
                "send" | "recv" | "error" => {
                    parts.next().is_some_and(|id| {
                        !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit())
                    }) && parts.next().is_some_and(|hash| {
                        hash.len() == 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                    }) && parts.next().is_none()
                }
                _ => false,
            }
        })
}

fn jet_browser_trace_summary(trace: &JetBrowserTrace) -> String {
    trace.entries.join(",")
}
