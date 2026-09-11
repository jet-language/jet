/// Typed native game-window hooks shared by the Foundation host and every
/// generated Prelude. The callback is deliberately independent of any
/// devserver or renderer implementation; hosts receive canonical events and
/// return bounded draw commands for the active window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JetDevtoolsNativeColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl JetDevtoolsNativeColor {
    pub const WHITE: Self = Self {
        red: 255,
        green: 255,
        blue: 255,
        alpha: 255,
    };

    pub const fn new(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetDevtoolsNativeDrawCommand {
    Rectangle {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        color: JetDevtoolsNativeColor,
    },
    Text {
        text: String,
        x: i32,
        y: i32,
        size: i32,
        color: JetDevtoolsNativeColor,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetDevtoolsNativeInput {
    Key { code: String, pressed: bool },
    Gamepad {
        gamepad: i64,
        control: String,
        pressed: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsNativeWindow {
    pub session_id: String,
    pub width: u32,
    pub height: u32,
}

impl JetDevtoolsNativeWindow {
    pub fn new(session_id: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            session_id: session_id.into(),
            width,
            height,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsNativeFrame {
    pub session_id: String,
    pub frame_index: u64,
    pub width: u32,
    pub height: u32,
}

impl JetDevtoolsNativeFrame {
    pub fn new(
        session_id: impl Into<String>,
        frame_index: u64,
        width: u32,
        height: u32,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            frame_index,
            width,
            height,
        }
    }
}

/// One canonical native-host callback. The host receives the same typed
/// `JetDevtoolsEvent` that generic sinks receive and may return only bounded,
/// typed drawing commands for the active frame.
pub trait JetDevtoolsNativeHost: Send + Sync {
    fn on_event(&self, session_id: &str, event: JetDevtoolsEvent);
    fn on_window_open(&self, window: JetDevtoolsNativeWindow);
    fn on_window_close(&self, session_id: &str);
    fn on_input(&self, session_id: &str, input: JetDevtoolsNativeInput) -> bool;
    fn on_frame_begin(&self, frame: JetDevtoolsNativeFrame) -> Vec<JetDevtoolsNativeDrawCommand>;
    fn on_draw(&self, session_id: &str, command: JetDevtoolsNativeDrawCommand);
    fn on_frame_end(&self, session_id: &str);
}

pub type JetDevtoolsNativeHostHandle = std::sync::Arc<dyn JetDevtoolsNativeHost>;

struct JetDevtoolsNativeHostRegistration {
    id: u64,
    session_id: String,
    host: JetDevtoolsNativeHostHandle,
}

#[derive(Default)]
struct JetDevtoolsNativeHostRegistry {
    next_id: u64,
    hosts: Vec<JetDevtoolsNativeHostRegistration>,
}

pub struct JetDevtoolsNativeHostGuard {
    id: u64,
    session_id: String,
}

/// Runtime callback for one bounded, host-authorized database EXPLAIN request.
/// The host owns command decoding and authority; the runtime receives only
/// canonical identity and limits, then resolves its observed statement.
pub type JetDevtoolsDatabaseExplainCallback =
    fn(&str, &str, &str, u64, u64);

pub struct JetDevtoolsDatabaseExplainCallbackGuard {
    callback: JetDevtoolsDatabaseExplainCallback,
}

fn validate_command_text(value: &str, field: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("jet.devtools.v1 {field} must not be empty"));
    }
    if value.len() > JET_DEVTOOLS_MAX_TEXT_BYTES {
        return Err(format!("jet.devtools.v1 {field} exceeds the Prelude text limit"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("jet.devtools.v1 {field} contains a control character"));
    }
    Ok(())
}

impl Drop for JetDevtoolsDatabaseExplainCallbackGuard {
    fn drop(&mut self) {
        if let Ok(mut callback) = JET_DEVTOOLS_DATABASE_EXPLAIN_CALLBACK.lock() {
            if callback
                .as_ref()
                .is_some_and(|installed| *installed as usize == self.callback as usize)
            {
                *callback = None;
            }
        }
    }
}

const JET_DEVTOOLS_MAX_DATABASE_EXPLAIN_ROWS: u64 = 256;
const JET_DEVTOOLS_MAX_DATABASE_EXPLAIN_TIMEOUT_MS: u64 = 30_000;
/// Runtime callback for one host-authorized game control. Returning the
/// callback's checked admission result keeps parse, queue, and authority
/// failures visible to the host instead of treating invocation as success.
pub type JetDevtoolsGameControlCallback = fn(&str, &str) -> Result<(), String>;

pub struct JetDevtoolsGameControlCallbackGuard {
    callback: JetDevtoolsGameControlCallback,
}

impl Drop for JetDevtoolsGameControlCallbackGuard {
    fn drop(&mut self) {
        if let Ok(mut callback) = JET_DEVTOOLS_GAME_CONTROL_CALLBACK.lock() {
            if callback
                .as_ref()
                .is_some_and(|installed| *installed as usize == self.callback as usize)
            {
                *callback = None;
            }
        }
    }
}


impl Drop for JetDevtoolsNativeHostGuard {
    fn drop(&mut self) {
        if self.id == 0 {
            return;
        }
        let mut registry = JET_DEVTOOLS_NATIVE_HOSTS
            .lock()
            .expect("native devtools host registry poisoned");
        registry.hosts.retain(|registration| registration.id != self.id);
        if !registry
            .hosts
            .iter()
            .any(|registration| registration.session_id == self.session_id)
        {
            let mut active = JET_DEVTOOLS_NATIVE_SESSION
                .lock()
                .expect("native devtools session poisoned");
            if active.as_deref() == Some(self.session_id.as_str()) {
                *active = None;
            }
        }
    }
}
static JET_DEVTOOLS_DATABASE_EXPLAIN_CALLBACK: std::sync::LazyLock<
    std::sync::Mutex<Option<JetDevtoolsDatabaseExplainCallback>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(None));
static JET_DEVTOOLS_GAME_CONTROL_CALLBACK: std::sync::LazyLock<
    std::sync::Mutex<Option<JetDevtoolsGameControlCallback>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(None));

static JET_DEVTOOLS_NATIVE_HOSTS: std::sync::LazyLock<
    std::sync::Mutex<JetDevtoolsNativeHostRegistry>,
> = std::sync::LazyLock::new(|| {
    std::sync::Mutex::new(JetDevtoolsNativeHostRegistry {
        next_id: 1,
        hosts: Vec::new(),
    })
});

static JET_DEVTOOLS_NATIVE_SESSION: std::sync::LazyLock<std::sync::Mutex<Option<String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(None));

/// Install the one runtime callback used by the Foundation devtools host.
/// Command parsing, session ownership, and authority remain host-owned.
pub fn jet_devtools_install_database_explain_callback(
    callback: JetDevtoolsDatabaseExplainCallback,
) -> Result<JetDevtoolsDatabaseExplainCallbackGuard, String> {
    let mut installed = JET_DEVTOOLS_DATABASE_EXPLAIN_CALLBACK
        .lock()
        .map_err(|_| "database EXPLAIN callback registry poisoned".to_string())?;
    if installed.is_some() {
        return Err("database EXPLAIN callback is already installed".to_string());
    }
    *installed = Some(callback);
    Ok(JetDevtoolsDatabaseExplainCallbackGuard { callback })
}

/// Dispatch one validated host command into the registered runtime. The
/// runtime callback receives no SQL, bindings, credentials, or authority.
pub fn jet_devtools_dispatch_database_explain(
    session_id: &str,
    request_id: &str,
    statement_identity: &str,
    timeout_ms: u64,
    max_rows: u64,
) -> Result<(), String> {
    validate_command_text(session_id, "session_id")?;
    validate_command_text(request_id, "request_id")?;
    validate_command_text(statement_identity, "statement_identity")?;
    if timeout_ms == 0 || timeout_ms > JET_DEVTOOLS_MAX_DATABASE_EXPLAIN_TIMEOUT_MS {
        return Err("database EXPLAIN timeout is outside its limit".to_string());
    }
    if max_rows == 0 || max_rows > JET_DEVTOOLS_MAX_DATABASE_EXPLAIN_ROWS {
        return Err("database EXPLAIN row limit is outside its limit".to_string());
    }
    let callback = JET_DEVTOOLS_DATABASE_EXPLAIN_CALLBACK
        .lock()
        .map_err(|_| "database EXPLAIN callback registry poisoned".to_string())?
        .ok_or_else(|| "database EXPLAIN runtime callback is unavailable".to_string())?;
    callback(session_id, request_id, statement_identity, timeout_ms, max_rows);
    Ok(())
}
/// Install the one runtime callback used by the Foundation game-control host.
/// The callback receives a checked session id and canonical DTO JSON; the
/// runtime adapter owns parsing and Prelude transition policy.
pub fn jet_devtools_install_game_control_callback(
    callback: JetDevtoolsGameControlCallback,
) -> Result<JetDevtoolsGameControlCallbackGuard, String> {
    let mut installed = JET_DEVTOOLS_GAME_CONTROL_CALLBACK
        .lock()
        .map_err(|_| "game control callback registry poisoned".to_string())?;
    if installed.is_some() {
        return Err("game control callback is already installed".to_string());
    }
    *installed = Some(callback);
    Ok(JetDevtoolsGameControlCallbackGuard { callback })
}

/// Dispatch one checked game-control payload into the installed runtime
/// adapter. The caller validates the DTO and session before this seam.
pub fn jet_devtools_dispatch_game_control(
    session_id: &str,
    payload_json: &str,
) -> Result<(), String> {
    validate_command_text(session_id, "session_id")?;
    validate_command_text(payload_json, "game control payload")?;
    let callback = JET_DEVTOOLS_GAME_CONTROL_CALLBACK
        .lock()
        .map_err(|_| "game control callback registry poisoned".to_string())?
        .ok_or_else(|| "game control runtime callback is unavailable".to_string())?;
    callback(session_id, payload_json)
}

const JET_DEVTOOLS_COMMAND_RELAY_PATH_ENV: &str = "JET_DEVTOOLS_COMMAND_RELAY_PATH";
const JET_DEVTOOLS_COMMAND_RELAY_MAX_FILES: usize = 256;
static JET_DEVTOOLS_COMMAND_RELAY_WRITE_LOCK: std::sync::LazyLock<std::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(()));
static JET_DEVTOOLS_COMMAND_RELAY_SEQUENCE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

fn jet_devtools_command_relay_path() -> Result<std::path::PathBuf, String> {
    let value = std::env::var(JET_DEVTOOLS_COMMAND_RELAY_PATH_ENV)
        .map_err(|_| "game control command relay is unavailable".to_string())?;
    if value.is_empty()
        || value.len() > JET_DEVTOOLS_MAX_TEXT_BYTES
        || value.chars().any(char::is_control)
    {
        return Err("game control command relay path is invalid".to_string());
    }
    Ok(std::path::PathBuf::from(value))
}

fn jet_devtools_relay_json_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\u{08}' => escaped.push_str("\\b"),
            '\u{0c}' => escaped.push_str("\\f"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                escaped.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => escaped.push(character),
        }
    }
    format!("\"{escaped}\"")
}
fn jet_devtools_relay_string_field(frame: &str, field: &str) -> Result<String, String> {
    let marker = format!("\"{field}\":\"");
    let start = frame
        .find(&marker)
        .ok_or_else(|| format!("game control relay is missing `{field}`"))?
        + marker.len();
    let mut value = String::new();
    let mut escaped = false;
    for character in frame[start..].chars() {
        if escaped {
            match character {
                '"' | '\\' | '/' => value.push(character),
                'b' => value.push('\u{08}'),
                'f' => value.push('\u{0c}'),
                'n' => value.push('\n'),
                'r' => value.push('\r'),
                't' => value.push('\t'),
                _ => {
                    return Err(format!(
                        "game control relay `{field}` contains an unsupported escape"
                    ))
                }
            }
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' {
            validate_command_text(&value, field)?;
            return Ok(value);
        } else {
            value.push(character);
        }
    }
    Err(format!("game control relay `{field}` is not a closed JSON string"))
}

fn jet_devtools_validate_relay_frame(frame: &str, session_id: &str) -> Result<(), String> {
    if frame.len() > JET_DEVTOOLS_MAX_ENVELOPE_BYTES {
        return Err("game control relay frame exceeds the envelope limit".to_string());
    }
    let protocol = jet_devtools_relay_string_field(frame, "protocol")?;
    if protocol != JET_DEVTOOLS_PROTOCOL {
        return Err("game control relay protocol is unsupported".to_string());
    }
    let frame_session = jet_devtools_relay_string_field(frame, "session_id")?;
    if frame_session != session_id {
        return Err("game control relay belongs to another devtools session".to_string());
    }
    let direction = jet_devtools_relay_string_field(frame, "direction")?;
    if direction != "host_to_runtime" {
        return Err("game control relay direction is unsupported".to_string());
    }
    if !frame.contains("\"commands\":[{\"kind\":\"GameControl\",\"payload\":") {
        return Err("game control relay has no GameControl command payload".to_string());
    }
    Ok(())
}


/// Append one host command to the process relay. Each command is an
/// independently renamed file, so a reader can consume complete commands in
/// lexical sequence without observing a torn write.
pub fn jet_devtools_write_game_control_relay(
    session_id: &str,
    payload_json: &str,
) -> Result<(), String> {
    let _write_guard = JET_DEVTOOLS_COMMAND_RELAY_WRITE_LOCK
        .lock()
        .map_err(|_| "game control command relay lock is poisoned".to_string())?;
    validate_command_text(session_id, "session_id")?;
    validate_command_text(payload_json, "game control payload")?;
    let directory = jet_devtools_command_relay_path()?;
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("cannot create game control command relay: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = std::fs::symlink_metadata(&directory)
            .map_err(|error| format!("cannot inspect game control command relay: {error}"))?;
        let owner = std::fs::metadata("/proc/self")
            .map(|process| process.uid())
            .unwrap_or(metadata.uid());
        if !metadata.file_type().is_dir() || metadata.uid() != owner {
            return Err("game control command relay directory is not owned by this user".to_string());
        }
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o700);
        let _ = std::fs::set_permissions(&directory, permissions);
    }
    let retained = std::fs::read_dir(&directory)
        .map_err(|error| format!("cannot inspect game control command relay: {error}"))?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "cmd"))
        .count();
    if retained >= JET_DEVTOOLS_COMMAND_RELAY_MAX_FILES {
        return Err("game control command relay is full".to_string());
    }
    let sequence =
        JET_DEVTOOLS_COMMAND_RELAY_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let started_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);
    let envelope = format!(
        "{{\"protocol\":{},\"session_id\":{},\"started_at_ms\":{},\
\"direction\":\"host_to_runtime\",\"commands\":[{{\"kind\":\"GameControl\",\"payload\":{}}}]}}",
        jet_devtools_relay_json_string(JET_DEVTOOLS_PROTOCOL),
        jet_devtools_relay_json_string(session_id),
        started_at_ms,
        payload_json,
    );
    if envelope.len() > JET_DEVTOOLS_MAX_ENVELOPE_BYTES {
        return Err("game control command relay payload exceeds the envelope limit".to_string());
    }
    let process = std::process::id();
    let temporary = directory.join(format!(".{process:020}-{sequence:020}.tmp"));
    let final_path = directory.join(format!("{sequence:020}-{process:020}.cmd"));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| format!("cannot open game control command relay: {error}"))?;
    use std::io::Write;
    if let Err(error) = file
        .write_all(envelope.as_bytes())
        .and_then(|()| file.sync_all())
    {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!("cannot write game control command relay: {error}"));
    }
    std::fs::rename(&temporary, &final_path)
        .map_err(|error| format!("cannot publish game control command relay: {error}"))
}

/// Consume complete host command files in sequence order for one session.
/// Invalid, foreign, and malformed frames are removed and returned as errors;
/// callers cannot silently discard a command that needs a verdict.
pub fn jet_devtools_take_game_control_relays(session_id: &str) -> Result<Vec<String>, String> {
    validate_command_text(session_id, "session_id")?;
    let directory = match std::env::var(JET_DEVTOOLS_COMMAND_RELAY_PATH_ENV) {
        Ok(value) => {
            if value.is_empty()
                || value.len() > JET_DEVTOOLS_MAX_TEXT_BYTES
                || value.chars().any(char::is_control)
            {
                return Err("game control command relay path is invalid".to_string());
            }
            std::path::PathBuf::from(value)
        }
        Err(std::env::VarError::NotPresent) => return Ok(Vec::new()),
        Err(error) => return Err(format!("cannot read game control command relay path: {error}")),
    };
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return Ok(Vec::new());
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "cmd"))
        .collect::<Vec<_>>();
    paths.sort();
    paths.truncate(JET_DEVTOOLS_COMMAND_RELAY_MAX_FILES);
    let mut frames = Vec::with_capacity(paths.len());
    for path in paths {
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            format!("cannot inspect game control command relay entry: {error}")
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let owner = std::fs::metadata("/proc/self")
                .map(|process| process.uid())
                .unwrap_or(metadata.uid());
            if metadata.uid() != owner || metadata.mode() & 0o077 != 0 {
                let _ = std::fs::remove_file(&path);
                return Err("game control command relay entry has unsafe ownership".to_string());
            }
        }
        if !metadata.file_type().is_file()
            || metadata.len() > JET_DEVTOOLS_MAX_ENVELOPE_BYTES as u64
        {
            let _ = std::fs::remove_file(&path);
            return Err("game control command relay entry is invalid".to_string());
        }
        let frame = std::fs::read_to_string(&path)
            .map_err(|error| format!("cannot read game control command relay: {error}"))?;
        std::fs::remove_file(&path)
            .map_err(|error| format!("cannot consume game control command relay: {error}"))?;
        jet_devtools_validate_relay_frame(&frame, session_id)?;
        frames.push(frame);
    }
    Ok(frames)
}
/// Remove pending relay files during session teardown. The relay directory is
/// process-scoped by construction, so this cannot touch another dev process.
pub fn jet_devtools_clear_game_control_relays() -> Result<(), String> {
    let directory = jet_devtools_command_relay_path()?;
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "cannot inspect game control command relay for teardown: {error}"
            ))
        }
    };
    for entry in entries {
        let path = entry
            .map_err(|error| format!("cannot inspect game control command relay: {error}"))?
            .path();
        if path.extension().is_some_and(|extension| extension == "cmd") {
            std::fs::remove_file(&path)
                .map_err(|error| format!("cannot clear game control command relay: {error}"))?;
        }
    }
    Ok(())
}


pub fn jet_devtools_install_native_host_unchecked(
    session_id: impl Into<String>,
    host: JetDevtoolsNativeHostHandle,
) -> Result<JetDevtoolsNativeHostGuard, String> {
    let session_id = session_id.into();
    if session_id.is_empty() || session_id.len() > JET_DEVTOOLS_MAX_TEXT_BYTES {
        return Err("native devtools host session identity is invalid".to_string());
    }
    if session_id.chars().any(char::is_control) {
        return Err("native devtools host session identity contains control text".to_string());
    }
    let mut registry = JET_DEVTOOLS_NATIVE_HOSTS
        .lock()
        .map_err(|_| "native devtools host registry poisoned".to_string())?;
    let id = registry.next_id;
    registry.next_id = registry.next_id.saturating_add(1).max(1);
    registry.hosts.push(JetDevtoolsNativeHostRegistration {
        id,
        session_id: session_id.clone(),
        host,
    });
    drop(registry);
    jet_devtools_native_bind_session_unchecked(&session_id)?;
    Ok(JetDevtoolsNativeHostGuard { id, session_id })
}

pub fn jet_devtools_native_bind_session_unchecked(session_id: &str) -> Result<(), String> {
    if session_id.is_empty()
        || session_id.len() > JET_DEVTOOLS_MAX_TEXT_BYTES
        || session_id.chars().any(char::is_control)
    {
        return Err("native devtools session identity is invalid".to_string());
    }
    let mut active = JET_DEVTOOLS_NATIVE_SESSION
        .lock()
        .map_err(|_| "native devtools session poisoned".to_string())?;
    *active = Some(session_id.to_string());
    Ok(())
}

pub fn jet_devtools_native_clear_session_unchecked(session_id: &str) {
    if let Ok(mut active) = JET_DEVTOOLS_NATIVE_SESSION.lock() {
        if active.as_deref() == Some(session_id) {
            *active = None;
        }
    }
}

fn jet_devtools_native_active_session() -> Option<String> {
    JET_DEVTOOLS_NATIVE_SESSION.lock().ok()?.clone()
}
/// Return the session selected by the native host registration.
pub fn jet_devtools_native_session_id() -> Option<String> {
    jet_devtools_native_active_session()
}

/// Invoke the first exact-session host without retaining the registry lock.
/// The empty/mismatched path returns before cloning an `Arc` or invoking user
/// code.
#[inline(always)]
pub fn jet_devtools_try_with_native_host<R>(
    session_id: &str,
    callback: impl FnOnce(&dyn JetDevtoolsNativeHost) -> R,
) -> Option<R> {
    let host = {
        let registry = JET_DEVTOOLS_NATIVE_HOSTS.lock().ok()?;
        registry
            .hosts
            .iter()
            .find(|registration| registration.session_id == session_id)
            .map(|registration| registration.host.clone())
    }?;
    Some(callback(host.as_ref()))
}

/// Publish one event through the ordinary sink registry and the matching
/// native host. The event remains typed; no callback decodes wire fields.
pub fn jet_devtools_publish_event_for_session_unchecked(
    session_id: &str,
    event: JetDevtoolsEvent,
) {
    jet_devtools_publish_event(event.clone());
    let _ = jet_devtools_try_with_native_host(session_id, |host| {
        host.on_event(session_id, event);
    });
}

pub fn jet_devtools_native_window_open_unchecked(width: u32, height: u32) {
    let Some(session_id) = jet_devtools_native_active_session() else {
        return;
    };
    let window = JetDevtoolsNativeWindow::new(session_id.clone(), width, height);
    let _ = jet_devtools_try_with_native_host(&session_id, |host| {
        host.on_window_open(window);
    });
}

pub fn jet_devtools_native_window_close_unchecked() {
    let Some(session_id) = jet_devtools_native_active_session() else {
        return;
    };
    let _ = jet_devtools_try_with_native_host(&session_id, |host| {
        host.on_window_close(&session_id);
    });
}

pub fn jet_devtools_native_input_unchecked(input: JetDevtoolsNativeInput) -> bool {
    let Some(session_id) = jet_devtools_native_active_session() else {
        return false;
    };
    jet_devtools_try_with_native_host(&session_id, |host| {
        host.on_input(&session_id, input)
    })
    .unwrap_or(false)
}

pub fn jet_devtools_native_frame_begin_unchecked(
    frame_index: u64,
    width: u32,
    height: u32,
) -> Vec<JetDevtoolsNativeDrawCommand> {
    let Some(session_id) = jet_devtools_native_active_session() else {
        return Vec::new();
    };
    let frame = JetDevtoolsNativeFrame::new(session_id.clone(), frame_index, width, height);
    let mut commands = jet_devtools_try_with_native_host(&session_id, |host| {
        host.on_frame_begin(frame)
    })
    .unwrap_or_default();
    commands.truncate(JET_DEVTOOLS_MAX_HISTORY);
    commands
}

pub fn jet_devtools_native_draw_unchecked(command: JetDevtoolsNativeDrawCommand) {
    let Some(session_id) = jet_devtools_native_active_session() else {
        return;
    };
    let _ = jet_devtools_try_with_native_host(&session_id, |host| {
        host.on_draw(&session_id, command);
    });
}

pub fn jet_devtools_native_frame_end_unchecked() {
    let Some(session_id) = jet_devtools_native_active_session() else {
        return;
    };
    let _ = jet_devtools_try_with_native_host(&session_id, |host| {
        host.on_frame_end(&session_id);
    });
}
