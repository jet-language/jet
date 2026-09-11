// D-DX-BROWSERTEST1=A (#2498): typed browser-test lifecycle and report support.
//
// This fragment is emitted only for the browser test harness. It consumes the
// native BiDi handles from Browser.rs, owns one fresh context per attempt, and
// records source-bound facts without putting page payloads in traces. The CLI
// selects engines and starts/reuses the local web server; this source keeps the
// runner deterministic across AOT, resident JIT, and the TIR host.

const JET_BROWSER_TEST_MAX_RETRIES: i64 = 5;
const JET_BROWSER_TEST_MAX_ENGINES: usize = 3;
const JET_BROWSER_TEST_MAX_SNAPSHOT_BYTES: usize = 2 * 1024 * 1024;
const JET_BROWSER_TEST_MAX_SCREENSHOT_BYTES: usize = 8 * 1024 * 1024;
const JET_BROWSER_TEST_MAX_ARTIFACT_BYTES: usize = 16 * 1024 * 1024;
const JET_BROWSER_TEST_MAX_SERVER_LOG_BYTES: usize = 64 * 1024;
const JET_BROWSER_TEST_DEFAULT_TIMEOUT_MS: i64 = 30_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JetBrowserTestSource {
    path: String,
    line: u32,
    column: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JetBrowserTestConfig {
    engines: Vec<String>,
    retries: i64,
    timeout: JetBrowserTimeout,
    filter: String,
    reporter: String,
    ui: bool,
    visual: bool,
    watch: bool,
    trace_on_retry: bool,
    trace_all: bool,
    server_url: String,
    server_command: Vec<String>,
    server_reuse: bool,
    server_cwd: String,
    artifact_dir: String,
    report_path: String,
    source_root: String,
}

fn jet_browser_test_config_value(root: &str, key: &str) -> Option<String> {
    let path = std::path::Path::new(root).join("browser-tests.conf");
    let contents = std::fs::read_to_string(path).ok()?;
    contents.lines().find_map(|line| {
        let line = line.trim();
        let (name, value) = line.split_once('=')?;
        if name.trim().eq_ignore_ascii_case(key) {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
}

impl JetBrowserTestConfig {
    fn from_environment() -> Result<Self, JetBrowserError> {
        let project_root = jet_browser_project_root();
        let browser_text = std::env::var("JET_TEST_BROWSERS")
            .ok()
            .or_else(|| std::env::var("JET_TEST_BROWSER").ok())
            .or_else(|| jet_browser_test_config_value(&project_root, "browsers"))
            .unwrap_or_else(|| "chromium".to_string());
        let engines = jet_browser_test_engines(&browser_text)?;
        let retries = std::env::var("JET_TEST_RETRIES")
            .ok()
            .or_else(|| jet_browser_test_config_value(&project_root, "retries"))
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(0);
        if !(0..=JET_BROWSER_TEST_MAX_RETRIES).contains(&retries) {
            return Err(JetBrowserError::new("protocol"));
        }
        let timeout_ms = std::env::var("JET_TEST_TIMEOUT_MS")
            .ok()
            .or_else(|| jet_browser_test_config_value(&project_root, "timeout-ms"))
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(JET_BROWSER_TEST_DEFAULT_TIMEOUT_MS);
        let timeout = jet_browser_timeout(timeout_ms)?;
        let reporter = std::env::var("JET_TEST_REPORTER")
            .ok()
            .or_else(|| jet_browser_test_config_value(&project_root, "reporter"))
            .unwrap_or_else(|| "text".to_string())
            .to_ascii_lowercase();
        if !matches!(reporter.as_str(), "text" | "json" | "html") {
            return Err(JetBrowserError::new("protocol"));
        }
        let artifact_dir = std::env::var("JET_TEST_ARTIFACTS")
            .ok()
            .or_else(|| jet_browser_test_config_value(&project_root, "artifacts"))
            .unwrap_or_else(|| {
                std::path::Path::new(&project_root)
                    .join(".jet/browser-tests/artifacts")
                    .to_string_lossy()
                    .into_owned()
            });
        let report_path = std::env::var("JET_TEST_REPORT")
            .ok()
            .or_else(|| jet_browser_test_config_value(&project_root, "report"))
            .unwrap_or_else(|| {
                std::path::Path::new(&project_root)
                    .join(".jet/browser-tests/report.json")
                    .to_string_lossy()
                    .into_owned()
            });
        let server_url = std::env::var("JET_TEST_WEB_SERVER_URL")
            .ok()
            .or_else(|| std::env::var("JET_BROWSER_TEST_SERVER_URL").ok())
            .or_else(|| jet_browser_test_config_value(&project_root, "server-url"))
            .unwrap_or_default();
        let server_command = std::env::var("JET_TEST_WEB_SERVER_CMD")
            .ok()
            .or_else(|| std::env::var("JET_BROWSER_TEST_SERVER_CMD").ok())
            .or_else(|| jet_browser_test_config_value(&project_root, "webServer"))
            .or_else(|| jet_browser_test_config_value(&project_root, "web_server"))
            .map(|command| jet_browser_test_split_command(&command))
            .unwrap_or_default();
        let server_reuse = std::env::var("JET_TEST_WEB_SERVER_REUSE")
            .ok()
            .map(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
            .or_else(|| {
                jet_browser_test_config_value(&project_root, "server")
                    .map(|value| value.eq_ignore_ascii_case("reuse"))
            })
            .unwrap_or(false);
        let server_cwd = std::env::var("JET_TEST_WEB_SERVER_CWD")
            .ok()
            .or_else(|| jet_browser_test_config_value(&project_root, "server-cwd"))
            .unwrap_or_else(|| project_root.clone());
        Ok(Self {
            engines,
            retries,
            timeout,
            filter: std::env::var("JET_TEST_FILTER")
                .ok()
                .or_else(|| jet_browser_test_config_value(&project_root, "filter"))
                .unwrap_or_default(),
            reporter,
            server_reuse,
            ui: jet_browser_test_env_bool("JET_TEST_BROWSER_UI", false),
            visual: jet_browser_test_env_bool("JET_TEST_BROWSER_VISUAL", false),
            watch: jet_browser_test_env_bool("JET_TEST_WATCH", false),
            trace_on_retry: jet_browser_test_env_bool("JET_TEST_TRACE_ON_RETRY", true),
            trace_all: jet_browser_test_env_bool("JET_TEST_TRACE", false),
            server_url,
            server_command,
            server_cwd,
            artifact_dir,
            report_path,
            source_root: project_root,
        })
    }
}
pub(crate) fn jet_browser_test_config_from_env() -> Result<JetBrowserTestConfig, JetBrowserError> {
    JetBrowserTestConfig::from_environment()
}

pub(crate) fn jet_browser_test_config() -> Result<JetBrowserTestConfig, JetBrowserError> {
    jet_browser_test_config_from_env()
}

pub(crate) fn jet_browser_test_config_engines(config: &JetBrowserTestConfig) -> Vec<String> {
    config.engines.clone()
}

pub(crate) fn jet_browser_test_config_retries(config: &JetBrowserTestConfig) -> i64 {
    config.retries
}

pub(crate) fn jet_browser_test_config_filter(config: &JetBrowserTestConfig) -> String {
    config.filter.clone()
}

pub(crate) fn jet_browser_test_config_reporter(config: &JetBrowserTestConfig) -> String {
    config.reporter.clone()
}

pub(crate) fn jet_browser_test_config_ui(config: &JetBrowserTestConfig) -> bool {
    config.ui
}

pub(crate) fn jet_browser_test_config_visual(config: &JetBrowserTestConfig) -> bool {
    config.visual
}

pub(crate) fn jet_browser_test_config_watch(config: &JetBrowserTestConfig) -> bool {
    config.watch
}

pub(crate) fn jet_browser_test_config_server_url(config: &JetBrowserTestConfig) -> String {
    config.server_url.clone()
}
pub(crate) fn jet_browser_test_config_artifact_dir(config: &JetBrowserTestConfig) -> String {
    config.artifact_dir.clone()
}

pub(crate) fn jet_browser_test_config_report_path(config: &JetBrowserTestConfig) -> String {
    config.report_path.clone()
}
fn jet_browser_test_env_bool(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => default,
        },
        Err(_) => default,
    }
}

fn jet_browser_test_engines(raw: &str) -> Result<Vec<String>, JetBrowserError> {
    let mut requested = Vec::new();
    for value in raw.split(|ch: char| ch == ',' || ch.is_ascii_whitespace()) {
        let value = value.trim().to_ascii_lowercase();
        if value.is_empty() {
            continue;
        }
        if !matches!(value.as_str(), "chromium" | "firefox" | "webkit") {
            return Err(JetBrowserError::new("unknown engine"));
        }
        if !requested.contains(&value) {
            requested.push(value);
        }
    }
    if requested.is_empty() || requested.len() > JET_BROWSER_TEST_MAX_ENGINES {
        return Err(JetBrowserError::new("unknown engine"));
    }
    let mut engines = Vec::new();
    for value in ["chromium", "firefox", "webkit"] {
        if requested.iter().any(|entry| entry == value) {
            engines.push(value.to_string());
        }
    }
    Ok(engines)
}

fn jet_browser_test_split_command(raw: &str) -> Vec<String> {
    let mut command = Vec::new();
    let mut value = String::new();
    let mut quote = None;
    for ch in raw.chars() {
        match (quote, ch) {
            (Some(delimiter), ch) if ch == delimiter => quote = None,
            (Some(_), ch) => value.push(ch),
            (None, '\'' | '"') => quote = Some(ch),
            (None, ch) if ch.is_ascii_whitespace() => {
                if !value.is_empty() {
                    command.push(std::mem::take(&mut value));
                }
            }
            (None, ch) => value.push(ch),
        }
    }
    if !value.is_empty() {
        command.push(value);
    }
    command
}

pub(crate) fn jet_browser_test_selected(config: &JetBrowserTestConfig, name: &String) -> bool {
    if config.filter.trim().is_empty() {
        return true;
    }
    config
        .filter
        .split(|ch: char| ch == ',' || ch.is_ascii_whitespace())
        .filter(|value| !value.is_empty())
        .any(|pattern| jet_browser_test_glob_match(pattern, name))
}

fn jet_browser_test_glob_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let mut p = 0;
    let mut v = 0;
    let mut star = None;
    let mut star_value = 0;
    while v < value.len() {
        if p < pattern.len() && (pattern[p] == value[v] || pattern[p] == b'?') {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            star_value = v;
        } else if let Some(star_position) = star {
            p = star_position + 1;
            star_value += 1;
            v = star_value;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

fn jet_browser_test_source(path: &String, line: i64, column: i64) -> JetBrowserTestSource {
    JetBrowserTestSource {
        path: path.clone(),
        line: u32::try_from(line.max(0)).unwrap_or(u32::MAX),
        column: u32::try_from(column.max(0)).unwrap_or(u32::MAX),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JetBrowserTestAction {
    sequence: usize,
    kind: String,
    fact: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JetBrowserTestSnapshot {
    kind: String,
    hash: String,
    bytes: usize,
    path: String,
    source: JetBrowserTestSource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JetBrowserTestEventFact {
    kind: String,
    request_id_hash: String,
    request_method: String,
    url_hash: String,
    status_code: i64,
    is_blocked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JetBrowserTestArtifact {
    name: String,
    path: String,
    mime: String,
    bytes: usize,
    hash: String,
    source: JetBrowserTestSource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JetBrowserTestAttempt {
    test: String,
    engine: String,
    attempt: i64,
    passed: bool,
    expected_failure: bool,
    duration_ms: i64,
    source: JetBrowserTestSource,
    actions: Vec<JetBrowserTestAction>,
    snapshots: Vec<JetBrowserTestSnapshot>,
    events: Vec<JetBrowserTestEventFact>,
    artifacts: Vec<JetBrowserTestArtifact>,
    trace: Vec<String>,
    error: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JetBrowserTestCase {
    name: String,
    selected: bool,
    attempts: Vec<JetBrowserTestAttempt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JetBrowserTestReport {
    source_root: String,
    reporter: String,
    ui: bool,
    watch: bool,
    cases: Vec<JetBrowserTestCase>,
}

pub(crate) fn jet_browser_test_report_new(config: &JetBrowserTestConfig) -> JetBrowserTestReport {
    JetBrowserTestReport {
        source_root: config.source_root.clone(),
        reporter: config.reporter.clone(),
        ui: config.ui,
        watch: config.watch,
        cases: Vec::new(),
    }
}

fn jet_browser_test_parse_actions(raw: &[String]) -> Vec<JetBrowserTestAction> {
    raw.iter()
        .enumerate()
        .map(|(sequence, entry)| {
            let (kind, fact) = entry
                .split_once(':')
                .map_or((entry.as_str(), ""), |(kind, fact)| (kind, fact));
            JetBrowserTestAction {
                sequence,
                kind: kind.to_string(),
                fact: fact.to_string(),
            }
        })
        .collect()
}

fn jet_browser_test_endpoint(engine: &str) -> Result<String, JetBrowserError> {
    let key = format!("JET_BROWSER_{}_ENDPOINT", engine.to_ascii_uppercase());
    let endpoint = std::env::var(&key)
        .or_else(|_| std::env::var("JET_BROWSER_ENDPOINT"))
        .unwrap_or_default();
    if endpoint.starts_with("ws://") || endpoint.starts_with("wss://") {
        Ok(endpoint)
    } else {
        Err(JetBrowserError::new("transport"))
    }
}

fn jet_browser_test_profile(locked: &JetBrowserLocked) -> Result<JetBrowserProfile, JetBrowserError> {
    let name = if locked.protocol.starts_with("bidi-") {
        locked.protocol.clone()
    } else {
        "bidi-2025.5".to_string()
    };
    jet_browser_profile(&name)
}

pub(crate) struct JetBrowserTestSession {
    browser: JetBrowser,
    context: JetBrowserContext,
    page: JetBrowserPage,
    actions: JetBrowserTestActionSink,
    config: JetBrowserTestConfig,
    test: String,
    engine: String,
    attempt: i64,
    source: JetBrowserTestSource,
    started: std::time::Instant,
}

fn jet_browser_test_begin(
    config: &JetBrowserTestConfig,
    test: &String,
    engine: &String,
    attempt: i64,
    source: JetBrowserTestSource,
) -> Result<JetBrowserTestSession, JetBrowserError> {
    let locked = jet_browser_locked(engine)?;
    let endpoint = jet_browser_test_endpoint(engine)?;
    let profile = jet_browser_test_profile(&locked)?;
    let browser = jet_browser_connect_profile(&endpoint, &profile, config.timeout)?;
    let actions: JetBrowserTestActionSink =
        std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    jet_browser_test_attach_actions(&browser, Some(actions.clone()));
    let context = jet_browser_context(&browser)?;
    let page = match jet_browser_context_page(&context) {
        Ok(page) => page,
        Err(error) => {
            let _ = jet_browser_context_close(&context);
            let _ = jet_browser_close(&browser);
            return Err(error);
        }
    };
    // A browser may not implement every optional event in an older profile.
    // The native event queue remains authoritative for the events it accepts.
    for event in [
        "log.entryAdded",
        "network.beforeRequestSent",
        "network.responseStarted",
        "network.fetchError",
        "browsingContext.consoleMessage",
    ] {
        let _ = jet_browser_subscribe(&browser, &event.to_string());
    }
    Ok(JetBrowserTestSession {
        browser,
        context,
        page,
        actions,
        config: config.clone(),
        test: test.clone(),
        engine: engine.clone(),
        attempt,
        source,
        started: std::time::Instant::now(),
    })
}
pub(crate) fn jet_browser_test_begin_named(
    config: &JetBrowserTestConfig,
    test: &String,
    engine: &String,
    attempt: i64,
    source_path: &String,
    line: i64,
    column: i64,
) -> Result<JetBrowserTestFixture, JetBrowserError> {
    jet_browser_test_begin(
        config,
        test,
        engine,
        attempt,
        jet_browser_test_source(source_path, line, column),
    )
}

fn jet_browser_test_fixture_source(
    fixture: &JetBrowserTestFixture,
) -> String {
    format!(
        "{}:{}:{}",
        fixture.source.path, fixture.source.line, fixture.source.column
    )
}
pub(crate) type JetBrowserTestFixture = JetBrowserTestSession;

fn jet_browser_test_fixture_page(fixture: &JetBrowserTestFixture) -> JetBrowserPage {
    fixture.page.clone()
}

fn jet_browser_test_fixture_context(
    fixture: &JetBrowserTestFixture,
) -> JetBrowserContext {
    fixture.context.clone()
}

fn jet_browser_test_fixture_finish(
    fixture: JetBrowserTestFixture,
    result: Result<(), JetBrowserError>,
    expected_failure: bool,
) -> JetBrowserTestAttempt {
    jet_browser_test_finish(fixture, result, expected_failure)
}

fn jet_browser_test_drain_events(browser: &JetBrowser) -> Vec<JetBrowserTestEventFact> {
    let mut events = Vec::new();
    loop {
        let timeout = JetBrowserTimeout { milliseconds: 1 };
        let Ok(event) = jet_browser_next_event(browser, timeout) else {
            break;
        };
        let method = jet_browser_event_kind(&event);
        let kind = if method.starts_with("log.") || method.contains("console") {
            "console"
        } else if method.starts_with("network.") {
            "network"
        } else {
            "event"
        };
        events.push(JetBrowserTestEventFact {
            kind: kind.to_string(),
            request_id_hash: jet_browser_fact_hash(&jet_browser_event_request_id(&event)),
            request_method: jet_browser_event_request_method(&event),
            url_hash: jet_browser_event_url_hash(&event),
            status_code: jet_browser_event_status_code(&event),
            is_blocked: jet_browser_event_is_blocked(&event),
        });
        if events.len() == JET_BROWSER_EVENT_LIMIT {
            break;
        }
    }
    events
}

fn jet_browser_test_safe_component(value: &str, fallback: &str) -> String {
    let safe: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if safe.is_empty() || safe == "." || safe == ".." {
        fallback.to_string()
    } else {
        safe
    }
}

fn jet_browser_test_attempt_dir(session: &JetBrowserTestSession) -> std::path::PathBuf {
    std::path::Path::new(&session.config.artifact_dir)
        .join(jet_browser_test_safe_component(&session.test, "test"))
        .join(jet_browser_test_safe_component(&session.engine, "engine"))
        .join(format!("attempt-{}", session.attempt))
}

fn jet_browser_test_write_bytes(path: &std::path::Path, bytes: &[u8]) -> Result<(), JetBrowserError> {
    if bytes.len() > JET_BROWSER_TEST_MAX_ARTIFACT_BYTES {
        return Err(JetBrowserError::new("protocol"));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| JetBrowserError::new("transport"))?;
    }
    std::fs::write(path, bytes).map_err(|_| JetBrowserError::new("transport"))
}

fn jet_browser_test_capture_text_snapshot(
    session: &JetBrowserTestSession,
    kind: &str,
    value: Result<String, JetBrowserError>,
    snapshots: &mut Vec<JetBrowserTestSnapshot>,
    path: Option<&std::path::Path>,
) {
    let Ok(value) = value else {
        return;
    };
    let full_hash = jet_browser_fact_hash(&value);
    let bytes = value.len();
    let retained = if bytes > JET_BROWSER_TEST_MAX_SNAPSHOT_BYTES {
        String::from_utf8_lossy(&value.as_bytes()[..JET_BROWSER_TEST_MAX_SNAPSHOT_BYTES]).into_owned()
    } else {
        value
    };
    let mut artifact_path = String::new();
    if let Some(path) = path {
        if jet_browser_test_write_bytes(path, retained.as_bytes()).is_ok() {
            artifact_path = path.to_string_lossy().into_owned();
        }
    }
    snapshots.push(JetBrowserTestSnapshot {
        kind: kind.to_string(),
        hash: full_hash,
        bytes,
        path: artifact_path,
        source: session.source.clone(),
    });
}

fn jet_browser_test_capture_binary_artifact(
    session: &JetBrowserTestSession,
    name: &str,
    mime: &str,
    encoded: Result<String, JetBrowserError>,
    artifacts: &mut Vec<JetBrowserTestArtifact>,
) {
    let Ok(encoded) = encoded else {
        return;
    };
    let Ok(bytes) = jet_browser_test_base64_decode(&encoded) else {
        return;
    };
    if bytes.len() > JET_BROWSER_TEST_MAX_ARTIFACT_BYTES {
        return;
    }
    let dir = jet_browser_test_attempt_dir(session);
    let path = dir.join(jet_browser_test_safe_component(name, "artifact"));
    if jet_browser_test_write_bytes(&path, &bytes).is_err() {
        return;
    }
    artifacts.push(JetBrowserTestArtifact {
        name: name.to_string(),
        path: path.to_string_lossy().into_owned(),
        mime: mime.to_string(),
        bytes: bytes.len(),
        hash: jet_browser_fact_hash(&String::from_utf8_lossy(&bytes)),
        source: session.source.clone(),
    });
}

fn jet_browser_test_capture_facts(
    session: &JetBrowserTestSession,
    include_payloads: bool,
) -> (
    Vec<JetBrowserTestSnapshot>,
    Vec<JetBrowserTestArtifact>,
    Vec<JetBrowserTestEventFact>,
) {
    let mut snapshots = Vec::new();
    let mut artifacts = Vec::new();
    let dir = jet_browser_test_attempt_dir(session);
    let dom_path = include_payloads.then(|| dir.join("dom.html"));
    jet_browser_test_capture_text_snapshot(
        session,
        "url",
        jet_browser_page_url(&session.page),
        &mut snapshots,
        None,
    );
    jet_browser_test_capture_text_snapshot(
        session,
        "title",
        jet_browser_page_title(&session.page),
        &mut snapshots,
        None,
    );
    jet_browser_test_capture_text_snapshot(
        session,
        "text",
        jet_browser_page_text(&session.page),
        &mut snapshots,
        None,
    );
    jet_browser_test_capture_text_snapshot(
        session,
        "dom",
        jet_browser_page_dom(&session.page),
        &mut snapshots,
        dom_path.as_deref(),
    );
    if include_payloads || session.config.visual {
        jet_browser_test_capture_binary_artifact(
            session,
            "screenshot.png",
            "image/png",
            jet_browser_page_screenshot(&session.page),
            &mut artifacts,
        );
    }
    let events = jet_browser_test_drain_events(&session.browser);
    (snapshots, artifacts, events)
}

fn jet_browser_test_finish(
    session: JetBrowserTestSession,
    result: Result<(), JetBrowserError>,
    expected_failure: bool,
) -> JetBrowserTestAttempt {
    let raw_passed = result.is_ok();
    let error = result
        .err()
        .map_or_else(String::new, |error| error.jet_show());
    let passed = if expected_failure {
        !raw_passed
    } else {
        raw_passed
    };
    let retry_requested = session.attempt > 0 && session.config.trace_on_retry;
    let trace_requested = session.config.trace_all || retry_requested;
    let include_payloads = !raw_passed || trace_requested || session.config.ui;
    let (snapshots, artifacts, events) = jet_browser_test_capture_facts(&session, include_payloads);
    let trace = if !raw_passed || trace_requested || session.config.ui {
        jet_browser_trace(&session.browser).entries
    } else {
        Vec::new()
    };
    let actions = jet_browser_test_parse_actions(&session.actions.borrow());
    let duration_ms = i64::try_from(session.started.elapsed().as_millis()).unwrap_or(i64::MAX);
    let _ = jet_browser_page_close(&session.page);
    let _ = jet_browser_context_close(&session.context);
    let _ = jet_browser_close(&session.browser);
    JetBrowserTestAttempt {
        test: session.test,
        engine: session.engine,
        attempt: session.attempt,
        passed,
        expected_failure,
        duration_ms,
        source: session.source,
        actions,
        snapshots,
        events,
        artifacts,
        trace,
        error,
    }
}

fn jet_browser_test_setup_failure(
    test: &String,
    engine: &String,
    attempt: i64,
    expected_failure: bool,
    source: JetBrowserTestSource,
    error: JetBrowserError,
) -> JetBrowserTestAttempt {
    JetBrowserTestAttempt {
        test: test.clone(),
        engine: engine.clone(),
        attempt,
        passed: false,
        expected_failure,
        duration_ms: 0,
        source,
        actions: Vec::new(),
        snapshots: Vec::new(),
        events: Vec::new(),
        artifacts: Vec::new(),
        trace: Vec::new(),
        error: error.jet_show(),
    }
}

type JetBrowserTestBody = fn(&JetBrowserPage) -> Result<(), JetBrowserError>;

fn jet_browser_test_run_case(
    config: &JetBrowserTestConfig,
    name: &String,
    source: JetBrowserTestSource,
    expected_failure: bool,
    body: JetBrowserTestBody,
) -> JetBrowserTestCase {
    if !jet_browser_test_selected(config, name) {
        return JetBrowserTestCase {
            name: name.clone(),
            selected: false,
            attempts: Vec::new(),
        };
    }
    let mut attempts = Vec::new();
    for engine in &config.engines {
        for attempt in 0..=config.retries {
            let session = jet_browser_test_begin(config, name, engine, attempt, source.clone());
            let attempt_result = match session {
                Ok(session) => {
                    let result = body(&session.page);
                    jet_browser_test_finish(session, result, expected_failure)
                }
                Err(error) => jet_browser_test_setup_failure(
                    name,
                    engine,
                    attempt,
                    expected_failure,
                    source.clone(),
                    error,
                ),
            };
            let passed = attempt_result.passed;
            attempts.push(attempt_result);
            if passed {
                break;
            }
        }
    }
    JetBrowserTestCase {
        name: name.clone(),
        selected: true,
        attempts,
    }
}

pub(crate) fn jet_browser_test_report_add_case(
    mut report: JetBrowserTestReport,
    case: JetBrowserTestCase,
) -> JetBrowserTestReport {
    report.cases.push(case);
    report.cases.sort_by(|left, right| left.name.cmp(&right.name));
    report
}

pub(crate) fn jet_browser_test_report_exit_code(report: &JetBrowserTestReport) -> i64 {
    report
        .cases
        .iter()
        .any(|case| {
            case.selected
                && case
                    .attempts
                    .last()
                    .map_or(true, |attempt| !attempt.passed)
        }) as i64
}

fn jet_browser_test_report_summary(report: &JetBrowserTestReport) -> String {
    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;
    for case in &report.cases {
        if !case.selected {
            skipped += 1;
        } else if case.attempts.last().is_some_and(|attempt| attempt.passed)
            && !case.attempts.is_empty()
        {
            passed += 1;
        } else {
            failed += 1;
        }
    }
    format!("{passed} passed, {failed} failed, {skipped} skipped")
}

fn jet_browser_test_json_source(source: &JetBrowserTestSource) -> jet_std::DataTree {
    jet_browser_object(vec![
        ("path", jet_browser_text(&source.path)),
        ("line", jet_std::DataTree::Float(f64::from(source.line))),
        ("column", jet_std::DataTree::Float(f64::from(source.column))),
    ])
}

fn jet_browser_test_json_attempt(attempt: &JetBrowserTestAttempt) -> jet_std::DataTree {
    let actions = jet_std::DataTree::Array(
        attempt
            .actions
            .iter()
            .map(|action| {
                jet_browser_object(vec![
                    ("sequence", jet_std::DataTree::Float(action.sequence as f64)),
                    ("kind", jet_browser_text(&action.kind)),
                    ("fact", jet_browser_text(&action.fact)),
                    ("source", jet_browser_test_json_source(&attempt.source)),
                ])
            })
            .collect(),
    );
    let snapshots = jet_std::DataTree::Array(
        attempt
            .snapshots
            .iter()
            .map(|snapshot| {
                jet_browser_object(vec![
                    ("kind", jet_browser_text(&snapshot.kind)),
                    ("hash", jet_browser_text(&snapshot.hash)),
                    ("bytes", jet_std::DataTree::Float(snapshot.bytes as f64)),
                    ("path", jet_browser_text(&snapshot.path)),
                    ("source", jet_browser_test_json_source(&snapshot.source)),
                ])
            })
            .collect(),
    );
    let events = jet_std::DataTree::Array(
        attempt
            .events
            .iter()
            .map(|event| {
                jet_browser_object(vec![
                    ("kind", jet_browser_text(&event.kind)),
                    ("requestIdHash", jet_browser_text(&event.request_id_hash)),
                    ("requestMethod", jet_browser_text(&event.request_method)),
                    ("urlHash", jet_browser_text(&event.url_hash)),
                    (
                        "statusCode",
                        jet_std::DataTree::Float(event.status_code as f64),
                    ),
                    ("isBlocked", jet_std::DataTree::Bool(event.is_blocked)),
                ])
            })
            .collect(),
    );
    let artifacts = jet_std::DataTree::Array(
        attempt
            .artifacts
            .iter()
            .map(|artifact| {
                jet_browser_object(vec![
                    ("name", jet_browser_text(&artifact.name)),
                    ("path", jet_browser_text(&artifact.path)),
                    ("mime", jet_browser_text(&artifact.mime)),
                    ("bytes", jet_std::DataTree::Float(artifact.bytes as f64)),
                    ("hash", jet_browser_text(&artifact.hash)),
                    ("source", jet_browser_test_json_source(&artifact.source)),
                ])
            })
            .collect(),
    );
    let trace = jet_std::DataTree::Array(
        attempt
            .trace
            .iter()
            .map(|entry| jet_browser_text(entry))
            .collect(),
    );
    jet_browser_object(vec![
        ("test", jet_browser_text(&attempt.test)),
        ("engine", jet_browser_text(&attempt.engine)),
        (
            "attempt",
            jet_std::DataTree::Float(attempt.attempt as f64),
        ),
        ("passed", jet_std::DataTree::Bool(attempt.passed)),
        (
            "expectedFailure",
            jet_std::DataTree::Bool(attempt.expected_failure),
        ),
        (
            "durationMs",
            jet_std::DataTree::Float(attempt.duration_ms as f64),
        ),
        ("source", jet_browser_test_json_source(&attempt.source)),
        ("actions", actions),
        ("snapshots", snapshots),
        ("events", events),
        ("artifacts", artifacts),
        ("trace", trace),
        (
            "error",
            if attempt.error.is_empty() {
                jet_std::DataTree::Null
            } else {
                jet_browser_text(&attempt.error)
            },
        ),
    ])
}

fn jet_browser_test_json(report: &JetBrowserTestReport) -> String {
    let cases = jet_std::DataTree::Array(
        report
            .cases
            .iter()
            .map(|case| {
                jet_browser_object(vec![
                    ("name", jet_browser_text(&case.name)),
                    ("selected", jet_std::DataTree::Bool(case.selected)),
                    (
                        "attempts",
                        jet_std::DataTree::Array(
                            case
                                .attempts
                                .iter()
                                .map(jet_browser_test_json_attempt)
                                .collect(),
                        ),
                    ),
                ])
            })
            .collect(),
    );
    jet_std::render_json(
        &jet_browser_object(vec![
            ("schema", jet_browser_text("jet.browser.test/v1")),
            ("sourceRoot", jet_browser_text(&report.source_root)),
            ("reporter", jet_browser_text(&report.reporter)),
            ("ui", jet_std::DataTree::Bool(report.ui)),
            ("watch", jet_std::DataTree::Bool(report.watch)),
            ("summary", jet_browser_text(&jet_browser_test_report_summary(report))),
            (
                "exitCode",
                jet_std::DataTree::Float(jet_browser_test_report_exit_code(report) as f64),
            ),
            ("cases", cases),
        ]),
        true,
        0,
    )
}
pub(crate) fn jet_browser_test_report_json(report: &JetBrowserTestReport) -> String {
    jet_browser_test_json(report)
}

pub(crate) fn jet_browser_test_report_text(report: &JetBrowserTestReport) -> String {
    jet_browser_test_text(report)
}

pub(crate) fn jet_browser_test_report_html(report: &JetBrowserTestReport) -> String {
    jet_browser_test_html(report)
}

fn jet_browser_test_text(report: &JetBrowserTestReport) -> String {
    let mut out = format!("Browser tests: {}\n", jet_browser_test_report_summary(report));
    for case in &report.cases {
        if !case.selected {
            out.push_str(&format!("SKIP {}\n", case.name));
            continue;
        }
        for attempt in &case.attempts {
            out.push_str(&format!(
                "{} {} [{}] attempt={} duration={}ms\n",
                if attempt.passed { "PASS" } else { "FAIL" },
                attempt.test,
                attempt.engine,
                attempt.attempt,
                attempt.duration_ms
            ));
            if !attempt.error.is_empty() {
                out.push_str(&format!("  error: {}\n", attempt.error));
            }
            for action in &attempt.actions {
                out.push_str(&format!("  action {}: {} {}\n", action.sequence, action.kind, action.fact));
            }
            for snapshot in &attempt.snapshots {
                out.push_str(&format!(
                    "  snapshot {}: {} bytes={} hash={}{}\n",
                    snapshot.kind,
                    snapshot.path,
                    snapshot.bytes,
                    snapshot.hash,
                    if snapshot.source.path.is_empty() {
                        String::new()
                    } else {
                        format!(" source={}:{}", snapshot.source.path, snapshot.source.line)
                    }
                ));
            }
            for event in &attempt.events {
                out.push_str(&format!(
                    "  {} request={} method={} url={} status={} blocked={}\n",
                    event.kind,
                    event.request_id_hash,
                    event.request_method,
                    event.url_hash,
                    event.status_code,
                    event.is_blocked
                ));
            }
            for artifact in &attempt.artifacts {
                out.push_str(&format!("  attachment {}: {}\n", artifact.name, artifact.path));
            }
            if !attempt.trace.is_empty() {
                out.push_str(&format!("  trace: {}\n", attempt.trace.join(",")));
            }
        }
    }
    out
}

fn jet_browser_test_html_json(value: &str) -> String {
    value
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn jet_browser_test_html(report: &JetBrowserTestReport) -> String {
    let json = jet_browser_test_html_json(&jet_browser_test_json(report));
    let mut out = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Jet browser tests</title><style>\
         :root{color-scheme:dark;font:14px system-ui,sans-serif;background:#10151d;color:#e7edf5}\
         body{margin:0;padding:24px;max-width:1400px;margin-inline:auto}\
         header{display:flex;gap:16px;align-items:end;justify-content:space-between;border-bottom:1px solid #344052;padding-bottom:16px}\
         input{background:#171f2b;border:1px solid #40506a;color:inherit;padding:8px;border-radius:5px;min-width:260px}\
         main{display:grid;grid-template-columns:minmax(220px,.8fr) minmax(0,2fr);gap:18px;margin-top:18px}\
         section{border:1px solid #344052;border-radius:8px;background:#151c27;padding:14px;margin-bottom:12px}\
         .tree button{display:block;width:100%;text-align:left;background:none;border:0;color:inherit;padding:8px;border-radius:4px;cursor:pointer}\
         .tree button:hover,.tree button.active{background:#26364b}.pass{color:#6ee7a8}.fail{color:#ff8e8e}.muted{color:#9aabc0}\
         pre{white-space:pre-wrap;overflow:auto;background:#0d1118;border-radius:5px;padding:10px;font-size:12px}\
         .chip{display:inline-block;border:1px solid #40506a;border-radius:999px;padding:2px 7px;margin:2px;font-size:12px}\
         @media(max-width:760px){main{display:block}body{padding:14px}input{min-width:0;width:100%}}\
         </style></head><body><header><div><div class=\"muted\">Jet browser test viewer</div><h1 id=\"summary\">Loading</h1></div><label>Filter <input id=\"filter\" placeholder=\"test or engine\"></label></header><main><section class=\"tree\"><h2>Test tree</h2><div id=\"tree\"></div></section><section><div id=\"details\"><p class=\"muted\">Select a test.</p></div></section></main><script id=\"jet-browser-data\" type=\"application/json\">",
    );
    out.push_str(&json);
    out.push_str(
        r#"</script><script>
const data=JSON.parse(document.getElementById('jet-browser-data').textContent);
const tree=document.getElementById('tree'), details=document.getElementById('details'), filter=document.getElementById('filter');
const text=(tag,value,cls='')=>{const e=document.createElement(tag);e.textContent=value??'';if(cls)e.className=cls;return e;};
const allAttempts=c=>c.attempts||[];
function renderTree(){tree.replaceChildren();const q=filter.value.toLowerCase();for(const c of data.cases){if(q&&!(`${c.name} ${allAttempts(c).map(a=>a.engine).join(' ')}`.toLowerCase().includes(q)))continue;const b=document.createElement('button');const last=allAttempts(c).at(-1);b.append(text('span',c.selected?(last&&last.passed?'PASS ':'FAIL '):'SKIP ','',c.selected?(last&&last.passed?'pass':'fail'):'muted'),text('span',c.name));b.onclick=()=>renderDetails(c);tree.append(b);}}
function renderDetails(c){details.replaceChildren();details.append(text('h2',c.name));for(const a of allAttempts(c)){const s=document.createElement('section');s.append(text('h3',`${a.passed?'PASS':'FAIL'} · ${a.engine} · attempt ${a.attempt}`,a.passed?'pass':'fail'));s.append(text('p',`Duration ${a.durationMs} ms · source ${a.source.path}:${a.source.line}:${a.source.column}`,'muted'));if(a.error)s.append(text('p',`Error: ${a.error}`,'fail'));const timeline=document.createElement('div');timeline.append(text('h4','Timeline'));for(const x of a.actions||[])timeline.append(text('span',`${x.sequence} ${x.kind} ${x.fact}`,'chip'));for(const x of a.trace||[])timeline.append(text('span',x,'chip'));s.append(timeline);const dom=document.createElement('div');dom.append(text('h4','DOM snapshots'));for(const x of a.snapshots||[])dom.append(text('p',`${x.kind}: ${x.hash} · ${x.bytes} bytes${x.path?' · '+x.path:''}`));s.append(dom);const source=document.createElement('div');source.append(text('h4','Source'));source.append(text('p',`${a.source.path}:${a.source.line}:${a.source.column}`));s.append(source);const logs=document.createElement('div');logs.append(text('h4','Logs / console'));for(const x of (a.events||[]).filter(x=>x.kind==='console'))logs.append(text('p',`${x.requestMethod||'console'} · ${x.urlHash||''}`));s.append(logs);const network=document.createElement('div');network.append(text('h4','Network'));for(const x of (a.events||[]).filter(x=>x.kind==='network'))network.append(text('p',`${x.requestMethod||''} · ${x.urlHash||''} · ${x.statusCode} · blocked=${x.isBlocked}`));s.append(network);const attachments=document.createElement('div');attachments.append(text('h4','Attachments'));for(const x of a.artifacts||[])attachments.append(text('p',`${x.name} · ${x.path} · ${x.mime} · ${x.bytes} bytes`));s.append(attachments);details.append(s);}}
summary.textContent=`${data.summary} · exit ${data.exitCode}`;filter.addEventListener('input',renderTree);renderTree();if(data.watch)setInterval(()=>location.reload(),1000);
</script></body></html>"#,
    );
    out
}

fn jet_browser_test_render(report: &JetBrowserTestReport) -> String {
    match report.reporter.as_str() {
        "json" => jet_browser_test_json(report),
        "html" => jet_browser_test_html(report),
        _ => jet_browser_test_text(report),
    }
}

pub(crate) fn jet_browser_test_write_report(
    report: &JetBrowserTestReport,
    path: &String,
) -> Result<(), JetBrowserError> {
    let rendered = jet_browser_test_render(report);
    let target = std::path::Path::new(path);
    jet_browser_test_write_bytes(target, rendered.as_bytes())?;
    if report.ui || report.reporter == "html" {
        let viewer = target.with_extension("html");
        jet_browser_test_write_bytes(&viewer, jet_browser_test_html(report).as_bytes())?;
    }
    Ok(())
}
pub(crate) fn jet_browser_test_watch_changed(
    path: &String,
    last_modified_ms: i64,
) -> Result<bool, JetBrowserError> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return Ok(last_modified_ms >= 0),
    };
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    Ok(modified_ms > last_modified_ms)
}


fn jet_browser_test_escape_source(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch => escaped.push(ch),
        }
    }
    escaped
}

pub(crate) fn jet_browser_test_generate_source(
    name: &String,
    route: &String,
    output: &String,
) -> Result<String, JetBrowserError> {
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        || route.is_empty()
    {
        return Err(JetBrowserError::new("protocol"));
    }
    let route = jet_browser_test_escape_source(route);
    let source = format!(
        "use core.web.browser\n\n#Test fn {name}(page: BrowserPage) {{\n  page.goto(\"{route}\")\n}}\n"
    );
    jet_browser_test_write_bytes(std::path::Path::new(output), source.as_bytes())?;
    Ok(source)
}

pub(crate) struct JetBrowserTestServer {
    child: Option<std::process::Child>,
    url: String,
    logs: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
}

impl JetBrowserTestServer {
    fn start(config: &JetBrowserTestConfig) -> Result<Self, JetBrowserError> {
        if config.server_url.is_empty() {
            return Err(JetBrowserError::new("transport"));
        }
        let logs = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let reusable = if config.server_reuse {
            let probe = Self {
                child: None,
                url: config.server_url.clone(),
                logs: logs.clone(),
            };
            probe
                .wait_ready(JetBrowserTimeout { milliseconds: 100 })
                .is_ok()
        } else {
            false
        };
        let mut child = None;
        if !reusable && !config.server_command.is_empty() {
            let mut command = std::process::Command::new(&config.server_command[0]);
            command.args(&config.server_command[1..]);
            if !config.server_cwd.is_empty() {
                command.current_dir(&config.server_cwd);
            }
            command.stdout(std::process::Stdio::piped());
            command.stderr(std::process::Stdio::piped());
            let mut process = command
                .spawn()
                .map_err(|_| JetBrowserError::new("transport"))?;
            if let Some(stdout) = process.stdout.take() {
                jet_browser_test_server_capture(stdout, logs.clone());
            }
            if let Some(stderr) = process.stderr.take() {
                jet_browser_test_server_capture(stderr, logs.clone());
            }
            child = Some(process);
        }
        let server = Self {
            child,
            url: config.server_url.clone(),
            logs,
        };
        if !reusable {
            server.wait_ready(config.timeout)?;
        }
        Ok(server)
    }

    fn wait_ready(&self, timeout: JetBrowserTimeout) -> Result<(), JetBrowserError> {
        let Some((host, port)) = jet_browser_test_host_port(&self.url) else {
            return Err(JetBrowserError::new("transport"));
        };
        let addresses = std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), port))
            .map_err(|_| JetBrowserError::new("transport"))?
            .collect::<Vec<_>>();
        if addresses.is_empty() {
            return Err(JetBrowserError::new("transport"));
        }
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_millis(timeout.milliseconds as u64);
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(JetBrowserError::new("timeout"));
            }
            if addresses.iter().any(|address| {
                std::net::TcpStream::connect_timeout(
                    address,
                    remaining.min(std::time::Duration::from_millis(100)),
                )
                .is_ok()
            }) {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    fn logs(&self) -> String {
        self.logs
            .lock()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default()
    }

    fn stop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child = None;
    }
}

impl Drop for JetBrowserTestServer {
    fn drop(&mut self) {
        self.stop();
    }
}
pub(crate) fn jet_browser_test_server_start(
    config: &JetBrowserTestConfig,
) -> Result<JetBrowserTestServer, JetBrowserError> {
    JetBrowserTestServer::start(config)
}

pub(crate) fn jet_browser_test_server_stop(server: &mut JetBrowserTestServer) {
    server.stop();
}

pub(crate) fn jet_browser_test_server_url(server: &JetBrowserTestServer) -> String {
    server.url.clone()
}

pub(crate) fn jet_browser_test_server_logs(server: &JetBrowserTestServer) -> String {
    server.logs()
}

fn jet_browser_test_server_capture<R>(mut reader: R, logs: std::sync::Arc<std::sync::Mutex<Vec<u8>>>)
where
    R: std::io::Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = reader.read_to_end(&mut bytes);
        bytes.truncate(JET_BROWSER_TEST_MAX_SERVER_LOG_BYTES);
        if let Ok(mut output) = logs.lock() {
            let available = JET_BROWSER_TEST_MAX_SERVER_LOG_BYTES.saturating_sub(output.len());
            output.extend_from_slice(&bytes[..bytes.len().min(available)]);
        }
    });
}

fn jet_browser_test_host_port(url: &str) -> Option<(String, u16)> {
    let authority = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))?;
    let authority = authority.split('/').next()?;
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    if let Some((host, port)) = authority.rsplit_once(':') {
        if host.is_empty() {
            return None;
        }
        return Some((host.to_string(), port.parse().ok()?));
    }
    Some((authority.to_string(), 80))
}

fn jet_browser_test_base64_decode(value: &str) -> Result<Vec<u8>, JetBrowserError> {
    let mut output = Vec::with_capacity(value.len() * 3 / 4);
    let mut quartet = [0u8; 4];
    let mut count = 0;
    for byte in value.bytes() {
        if byte.is_ascii_whitespace() {
            continue;
        }
        if byte == b'=' {
            quartet[count] = 64;
        } else {
            quartet[count] = match byte {
                b'A'..=b'Z' => byte - b'A',
                b'a'..=b'z' => byte - b'a' + 26,
                b'0'..=b'9' => byte - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                _ => return Err(JetBrowserError::new("protocol")),
            };
        }
        count += 1;
        if count != 4 {
            continue;
        }
        output.push((quartet[0] << 2) | (quartet[1] >> 4));
        if quartet[2] != 64 {
            output.push((quartet[1] << 4) | (quartet[2] >> 2));
        }
        if quartet[3] != 64 {
            output.push((quartet[2] << 6) | quartet[3]);
        }
        if output.len() > JET_BROWSER_TEST_MAX_SCREENSHOT_BYTES {
            return Err(JetBrowserError::new("protocol"));
        }
        count = 0;
    }
    if count != 0 {
        return Err(JetBrowserError::new("protocol"));
    }
    Ok(output)
}
