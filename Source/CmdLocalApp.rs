//! `jet dev <source> --app <function>` — a checked local-app projection.
//!
//! The server is deliberately thin: the notebook `Kernel` remains the only
//! evaluator and each browser tab owns one Kernel/session.  This module only
//! supplies the local HTTP transport, session routing, and checked-schema
//! projection used by the app shell.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use jet::ExitCodes;
use jet::REPL::Notebook::{
    self, CellKind, ClientKind, JetNotebook, Kernel, KernelIdentity, NotebookAppInput,
    NotebookAppSchema,
};
use jet_foundation::DataTree::DataTree;
use jet_foundation::JSON::parse_json;
const MAX_REQUEST_HEADERS: usize = 64 * 1024;
const MAX_REQUEST_BODY: usize = 8 * 1024 * 1024;
const MAX_CONNECTIONS: usize = 64;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const BOOTSTRAP_ROUTE: &str = "/__jet_local_app_bootstrap";
const BOOTSTRAP_TTL: Duration = Duration::from_secs(30);
const RECEIPT_SCHEMA: &str = "jet.local-app/v1";

/// Return whether the canonical `--app` selector is present in the shared CLI
/// argv.  The CLI registry owns the flag vocabulary; this helper only lets the
/// `dev` dispatch hand the already-parsed value to this module.
pub(crate) fn app_requested(raw: &[String]) -> bool {
    raw.iter()
        .any(|arg| arg == "--app" || arg.starts_with("--app="))
}

/// Read the selected app function through the same `--name`/`--name=value`
/// convention used by the rest of the CLI.
pub(crate) fn app_function<'a>(raw: &'a [String]) -> Option<&'a str> {
    flag_value(raw, "--app")
}

/// Read the explicit sharing authority.  Absence is the safe loopback mode.
pub(crate) fn app_share<'a>(raw: &'a [String]) -> Option<&'a str> {
    flag_value(raw, "--share")
}

/// Start a checked notebook/source projection on the dev command's local-app
/// path.  This function owns the blocking HTTP loop; it never starts a second
/// evaluator or child runtime.
pub(crate) fn run_local_app(
    path: &Path,
    function: &str,
    share: Option<&str>,
    explicit_token: Option<&str>,
    json: bool,
) -> ! {
    let share = match ShareMode::parse(share.unwrap_or("loopback")) {
        Ok(share) => share,
        Err(error) => fail_preflight(&error, json),
    };
    if function.trim().is_empty() {
        fail_preflight(
            "`jet dev --app` needs a function name",
            json,
        );
    }
    let token = match explicit_token.filter(|token| !token.is_empty()) {
        Some(token) => token.to_string(),
        None => match mint_token() {
            Ok(token) => token,
            Err(error) => fail_preflight(
                &format!("local app could not create an authentication token: {error}"),
                json,
            ),
        },
    };
    let definition = match AppDefinition::load(path, function) {
        Ok(definition) => definition,
        Err(error) => fail_preflight(&error, json),
    };
    let host = match AppHost::new(definition, token.clone()) {
        Ok(host) => host,
        Err(error) => fail_preflight(&error, json),
    };
    let bind = share.bind_address();
    let auto_open = share == ShareMode::Loopback && io::stdin().is_terminal();
    match serve(host, &bind, token, share, auto_open, json) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            crate::emit_cli_report(
                "E2105",
                format!("local app server failed: {error}"),
                "the checked dev host owns the local-app listener and session boundary".into(),
                "correct the bind or operating-system error, then run `jet dev --app` again".into(),
                json,
            );
            std::process::exit(ExitCodes::ICE);
        }
    }
}

fn fail_preflight(message: &str, json: bool) -> ! {
    let structured = match parse_json(message) {
        Ok(DataTree::Object(fields)) => {
            let text = |key: &str| {
                fields.iter().find_map(|(field, value)| {
                    if field != key {
                        return None;
                    }
                    match value {
                        DataTree::Text(value) | DataTree::TypedText(value) => Some(value.clone()),
                        _ => None,
                    }
                })
            };
            Some((
                text("code").unwrap_or_else(|| "E2104".into()),
                text("what").unwrap_or_else(|| message.into()),
                text("why").unwrap_or_else(|| {
                    "a local app must select one checked function and one explicit sharing authority"
                        .into()
                }),
                text("fix").unwrap_or_else(|| {
                    "choose an available function, use `--share loopback` by default, and retry"
                        .into()
                }),
            ))
        }
        _ => None,
    };
    let (code, what, why, fix) = structured.unwrap_or_else(|| {
        (
            "E2104".into(),
            message.into(),
            "a local app must select one checked function and one explicit sharing authority".into(),
            "choose an available function, use `--share loopback` by default, and retry".into(),
        )
    });
    crate::emit_cli_report(&code, what, why, fix, json);
    std::process::exit(ExitCodes::USER_ERROR)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShareMode {
    Loopback,
    Lan,
}

impl ShareMode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "loopback" | "local" => Ok(Self::Loopback),
            "lan" => Ok(Self::Lan),
            other => Err(format!(
                "unknown local-app sharing mode `{other}`; expected `loopback` or `lan`"
            )),
        }
    }

    fn bind_address(self) -> String {
        match self {
            Self::Loopback => "127.0.0.1:0".into(),
            Self::Lan => "0.0.0.0:0".into(),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Loopback => "loopback",
            Self::Lan => "lan",
        }
    }

    fn authority(self) -> &'static str {
        match self {
            Self::Loopback => "Network.Listen(loopback)",
            Self::Lan => "Network.Listen(approved by --share lan)",
        }
    }
}

#[derive(Clone)]
enum AppDocument {
    Notebook { path: PathBuf },
    Source { source: String },
}

#[derive(Clone)]
struct AppDefinition {
    document: AppDocument,
    path: PathBuf,
    function: String,
    source: String,
    source_hash: String,
    environment_hash: String,
    controls: Vec<ControlSchema>,
}

impl AppDefinition {
    fn load(path: &Path, function: &str) -> Result<Self, String> {
        let bytes = fs::read(path).map_err(|error| {
            format!(
                "local app source `{}` could not be read: {error}",
                path.display()
            )
        })?;
        let source_hash = jet::SHA256::sha256_hex(&bytes);
        let (document, source) = if path.extension().and_then(|extension| extension.to_str())
            == Some("jetnb")
        {
            let notebook = Notebook::load_jetnb(path)?;
            let source = notebook
                .cells
                .iter()
                .filter(|cell| cell.kind == CellKind::Jet)
                .map(|cell| cell.source.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            (AppDocument::Notebook { path: path.to_path_buf() }, source)
        } else {
            let source = String::from_utf8(bytes).map_err(|_| {
                format!("local app source `{}` is not UTF-8", path.display())
            })?;
            (AppDocument::Source { source: source.clone() }, source)
        };
        let environment_root = path.parent().unwrap_or_else(|| Path::new("."));
        Ok(Self {
            document,
            path: path.to_path_buf(),
            function: function.to_string(),
            source,
            source_hash,
            environment_hash: Kernel::environment_hash(environment_root),
            controls: Vec::new(),
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct ControlSchema {
    function: String,
    name: String,
    label: String,
    type_name: String,
    required: bool,
    variadic: bool,
}

impl ControlSchema {
    fn from_input(function: &str, input: NotebookAppInput) -> Self {
        Self {
            function: function.to_string(),
            name: input.name,
            label: input.label,
            type_name: input.type_name,
            required: input.required,
            variadic: input.variadic,
        }
    }

    fn json(&self) -> String {
        self.json_with_optional_value(None)
    }

    fn json_with_value(&self, value: &str) -> String {
        self.json_with_optional_value(Some(value))
    }

    fn json_with_optional_value(&self, value: Option<&str>) -> String {
        let value = value
            .map(|value| format!(",\"value\":{}", json_str(value)))
            .unwrap_or_default();
        format!(
            "{{\"function\":{},\"name\":{},\"label\":{},\"type\":{},\"required\":{},\"variadic\":{}{}}}",
            json_str(&self.function),
            json_str(&self.name),
            json_str(&self.label),
            json_str(&self.type_name),
            self.required,
            self.variadic,
            value,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AppSchema {
    function: String,
    controls: Vec<ControlSchema>,
    return_projection: String,
    effects: Vec<String>,
    authority: String,
    source_identity: String,
    compatibility_digest: String,
}

impl AppSchema {
    fn from_checked(schema: NotebookAppSchema) -> Self {
        let NotebookAppSchema {
            function,
            inputs,
            return_projection,
            effects,
            authority,
            source_identity,
            compatibility_digest,
            ..
        } = schema;
        let controls = inputs
            .into_iter()
            .map(|input| ControlSchema::from_input(&function, input))
            .collect();
        Self {
            function,
            controls,
            return_projection,
            effects,
            authority,
            source_identity,
            compatibility_digest,
        }
    }
}

#[derive(Clone, Debug)]
struct ControlValue {
    schema: ControlSchema,
    value: String,
}

fn build_kernel(definition: &AppDefinition, token: &str) -> Result<AppSession, String> {
    let mut kernel = match &definition.document {
        AppDocument::Notebook { path } => Kernel::open(
            Some(path.as_path()),
            definition.environment_hash.clone(),
        )?,
        AppDocument::Source { source, .. } => {
            let mut kernel = Kernel::open(None, definition.environment_hash.clone())?;
            kernel.add_cell(CellKind::Jet, source.clone())?;
            kernel
        }
    };
    kernel.set_authority(token);
    let schema = AppSchema::from_checked(kernel.app_schema(&definition.function)?);
    let call_cell = kernel.add_cell(
        CellKind::Jet,
        format!("{}()", definition.function),
    )?;
    Ok(AppSession {
        kernel,
        call_cell,
        schema,
        values: BTreeMap::new(),
        callback: 0,
    })
}

fn values_as_controls(
    schema: &AppSchema,
    values: &BTreeMap<String, String>,
) -> Vec<ControlValue> {
    schema
        .controls
        .iter()
        .cloned()
        .map(|schema| ControlValue {
            value: values.get(&schema.name).cloned().unwrap_or_default(),
            schema,
        })
        .collect()
}

fn required_string(fields: &[(String, DataTree)], key: &str) -> Result<String, String> {
    match fields
        .iter()
        .find_map(|(field, value)| (field == key).then_some(value))
    {
        Some(DataTree::Text(value)) | Some(DataTree::TypedText(value)) => Ok(value.clone()),
        _ => Err(format!("local-app identity field `{key}` is not text")),
    }
}

struct AppSession {
    kernel: Kernel,
    call_cell: String,
    schema: AppSchema,
    values: BTreeMap<String, String>,
    callback: u64,
}


struct AppHost {
    definition: AppDefinition,
    schema: AppSchema,
    token: String,
    prototype: Option<AppSession>,
    sessions: BTreeMap<String, AppSession>,
    generation: u64,
    reload: ReloadFact,
    receipt: ServerReceipt,
}

impl AppHost {
    fn new(mut definition: AppDefinition, token: String) -> Result<Self, String> {
        let prototype = build_kernel(&definition, &token)?;
        let schema = prototype.schema.clone();
        let controls = schema.controls.clone();
        definition.controls = controls;
        Ok(Self {
            reload: ReloadFact::initial(&definition.source_hash),
            definition,
            schema,
            token,
            prototype: Some(prototype),
            sessions: BTreeMap::new(),
            generation: 0,
            receipt: ServerReceipt::empty(),
        })
    }

    fn create_session(&mut self) -> Result<String, String> {
        let id = JetNotebook::mint_cell_id();
        let session = self
            .prototype
            .take()
            .map(Ok)
            .unwrap_or_else(|| build_kernel(&self.definition, &self.token))?;
        self.sessions.insert(id.clone(), session);
        Ok(id)
    }
    fn reset_session(&mut self, id: &str) -> Result<(), String> {
        if !self.sessions.contains_key(id) {
            return Err("local app session does not exist".to_string());
        }
        let replacement = build_kernel(&self.definition, &self.token)?;
        let session = self
            .sessions
            .get_mut(id)
            .ok_or_else(|| "local app session does not exist".to_string())?;
        session.kernel = replacement.kernel;
        session.call_cell = replacement.call_cell;
        session.schema = replacement.schema;
        session.values.clear();
        session.callback = session.callback.saturating_add(1);
        Ok(())
    }

    fn callback(&self, id: &str) -> Result<u64, String> {
        self.sessions
            .get(id)
            .map(|session| session.callback)
            .ok_or_else(|| "local app session does not exist".to_string())
    }
    fn reload_if_changed(&mut self) {
        let bytes = match fs::read(&self.definition.path) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.reload = ReloadFact::error(
                    &self.definition.source_hash,
                    format!("source refresh failed: {error}"),
                );
                return;
            }
        };
        let source_hash = jet::SHA256::sha256_hex(&bytes);
        if source_hash == self.definition.source_hash {
            return;
        }
        let mut candidate =
            match AppDefinition::load(&self.definition.path, &self.definition.function) {
                Ok(candidate) => candidate,
                Err(error) => {
                    self.reload = ReloadFact::error(&source_hash, error);
                    return;
                }
            };
        let candidate_prototype = match build_kernel(&candidate, &self.token) {
            Ok(prototype) => prototype,
            Err(error) => {
                self.reload = ReloadFact::error(&source_hash, error);
                return;
            }
        };
        let candidate_schema = candidate_prototype.schema.clone();
        let controls = candidate_schema.controls.clone();
        candidate.controls = controls;
        let compatible = self.schema.compatibility_digest == candidate_schema.compatibility_digest;
        let mut replacement_sessions = BTreeMap::new();
        let mut reset_count = 0usize;
        for (id, old_session) in &self.sessions {
            let mut replacement = match build_kernel(&candidate, &self.token) {
                Ok(replacement) => replacement,
                Err(error) => {
                    self.reload = ReloadFact::error(&source_hash, error);
                    return;
                }
            };
            replacement.callback = old_session.callback;
            if compatible {
                for control in &candidate.controls {
                    if let Some(value) = old_session.values.get(&control.name) {
                        if !value.is_empty()
                            && replacement
                                .kernel
                                .set_control(&control.function, &control.name, value.clone())
                                .is_ok()
                        {
                            replacement.values.insert(control.name.clone(), value.clone());
                        }
                    }
                }
            } else {
                reset_count = reset_count.saturating_add(1);
            }
            replacement_sessions.insert(id.clone(), replacement);
        }
        self.definition = candidate;
        self.schema = candidate_schema;
        self.prototype = Some(candidate_prototype);
        self.sessions = replacement_sessions;
        self.generation = self.generation.saturating_add(1);
        self.reload = ReloadFact {
            status: "reloaded".into(),
            reason: if compatible {
                "source changed; checked controls and compatible input state restored".into()
            } else {
                "source changed; checked app schema changed and sessions were reset".into()
            },
            source_hash,
            generation: self.generation,
            reset_count,
            stale: true,
        };
    }

    fn state(&self, id: &str) -> Result<String, String> {
        let session = self
            .sessions
            .get(id)
            .ok_or_else(|| "local app session does not exist".to_string())?;
        let controls = values_as_controls(&self.schema, &session.values);
        Ok(format!(
            "{{\"ok\":true,\"schema\":{},\"session\":{},\"generation\":{},\"app\":{},\"reload\":{},\"state\":{}}}",
            json_str(RECEIPT_SCHEMA),
            json_str(id),
            self.generation,
            self.app_json_with_values(&controls),
            self.reload.json(),
            session.kernel.state_json_for(ClientKind::FirstParty)
        ))
    }

    fn app_json(&self) -> String {
        let controls = self
            .schema
            .controls
            .iter()
            .map(ControlSchema::json)
            .collect::<Vec<_>>()
            .join(",");
        self.app_json_with_control_json(&controls)
    }

    fn app_json_with_values(&self, values: &[ControlValue]) -> String {
        let controls = self
            .schema
            .controls
            .iter()
            .map(|schema| {
                let value = values
                    .iter()
                    .find(|current| {
                        current.schema.function == schema.function
                            && current.schema.name == schema.name
                    })
                    .map(|current| current.value.as_str())
                    .unwrap_or_default();
                schema.json_with_value(value)
            })
            .collect::<Vec<_>>()
            .join(",");
        self.app_json_with_control_json(&controls)
    }

    fn app_json_with_control_json(&self, controls: &str) -> String {
        let effects = self
            .schema
            .effects
            .iter()
            .map(|effect| json_str(effect))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"path\":{},\"function\":{},\"source_hash\":{},\"source_identity\":{},\"compatibility_digest\":{},\"return_projection\":{},\"effects\":[{}],\"authority\":{},\"controls\":[{}]}}",
            json_str(&self.definition.path.display().to_string()),
            json_str(&self.schema.function),
            json_str(&self.definition.source_hash),
            json_str(&self.schema.source_identity),
            json_str(&self.schema.compatibility_digest),
            json_str(&self.schema.return_projection),
            effects,
            json_str(&self.schema.authority),
            controls,
        )
    }

    fn receipt_json(&self) -> String {
        format!(
            "{{\"schema\":{},\"bind\":{},\"url\":{},\"share\":{},\"authority\":{},\"app\":{},\"data\":{{\"input\":\"checked typed controls\",\"output\":\"Kernel MIME/state projection\",\"session_isolation\":\"one Kernel per browser tab\",\"source_hash\":{}}},\"trust\":{{\"policy\":{},\"renderer\":\"jet-notebook\",\"grants\":\"kernel-owned\"}},\"sessions\":{},\"generation\":{},\"reload\":{}}}",
            json_str(RECEIPT_SCHEMA),
            json_str(&self.receipt.bind),
            json_str(&self.receipt.url),
            json_str(&self.receipt.share),
            json_str(&self.receipt.authority),
            self.app_json(),
            json_str(&self.definition.source_hash),
            json_str(Notebook::POLICY_VERSION),
            self.sessions.len(),
            self.generation,
            self.reload.json(),
        )
    }
}

#[derive(Clone)]
struct ReloadFact {
    status: String,
    reason: String,
    source_hash: String,
    generation: u64,
    reset_count: usize,
    stale: bool,
}

impl ReloadFact {
    fn initial(source_hash: &str) -> Self {
        Self {
            status: "initial".into(),
            reason: "source checked before launch".into(),
            source_hash: source_hash.into(),
            generation: 0,
            reset_count: 0,
            stale: false,
        }
    }

    fn error(source_hash: &str, reason: String) -> Self {
        Self {
            status: "error".into(),
            reason,
            source_hash: source_hash.into(),
            generation: 0,
            reset_count: 0,
            stale: true,
        }
    }

    fn json(&self) -> String {
        format!(
            "{{\"status\":{},\"reason\":{},\"source_hash\":{},\"generation\":{},\"reset_count\":{},\"stale\":{}}}",
            json_str(&self.status),
            json_str(&self.reason),
            json_str(&self.source_hash),
            self.generation,
            self.reset_count,
            self.stale,
        )
    }
}

#[derive(Default)]
struct ServerReceipt {
    bind: String,
    url: String,
    share: String,
    authority: String,
}

impl ServerReceipt {
    fn empty() -> Self {
        Self::default()
    }
}

fn serve(
    mut host: AppHost,
    bind: &str,
    token: String,
    share: ShareMode,
    auto_open: bool,
    json: bool,
) -> Result<i32, String> {
    let listener = TcpListener::bind(bind).map_err(|error| error.to_string())?;
    let bound = listener.local_addr().map_err(|error| error.to_string())?;
    let url = format!("http://{bound}/");
    host.receipt = ServerReceipt {
        bind: bound.to_string(),
        url: url.clone(),
        share: share.as_str().into(),
        authority: share.authority().into(),
    };
    let shared = Arc::new(Mutex::new(host));
    let bootstrap = if auto_open {
        let nonce = mint_token()?;
        let state = Arc::new(Mutex::new(Some(BootstrapGrant {
            nonce: nonce.clone(),
            expires_at: Instant::now() + BOOTSTRAP_TTL,
        })));
        open_local_app_browser(&format!(
            "http://{bound}{BOOTSTRAP_ROUTE}?nonce={}",
            fragment_component(&nonce)
        ));
        Some(state)
    } else {
        None
    };
    let (receipt, function) = {
        let host = shared
            .lock()
            .map_err(|_| "local app host lock poisoned".to_string())?;
        (host.receipt_json(), host.definition.function.clone())
    };
    if json {
        println!("{receipt}");
    } else {
        eprintln!("jet local app listening on {url}(authentication token withheld)");
        eprintln!("function: {function}");
        eprintln!("share: {}", share.as_str());
        if share == ShareMode::Lan {
            eprintln!("bearer token: {token}");
        }
        eprintln!("receipt: GET /api/receipt with the bearer token");
    }
    let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    loop {
        let Some(connection) = listener.incoming().next() else {
            break;
        };
        let Ok(stream) = connection else { continue };
        let Ok(previous) = active.fetch_update(
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
            |count| (count < MAX_CONNECTIONS).then_some(count + 1),
        ) else {
            continue;
        };
        let _ = previous;
        let shared_for_thread = Arc::clone(&shared);
        let active_for_thread = Arc::clone(&active);
        let token_for_thread = token.clone();
        let bootstrap_for_thread = bootstrap.clone();
        if std::thread::Builder::new()
            .spawn(move || {
                let mut stream = stream;
                let _ = stream.set_write_timeout(Some(REQUEST_TIMEOUT));
                let _ = handle_connection(
                    &mut stream,
                    &shared_for_thread,
                    &token_for_thread,
                    bootstrap_for_thread.as_ref(),
                );
                active_for_thread.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
            })
            .is_err()
        {
            active.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        }
    }
    Ok(ExitCodes::OK)
}

struct BootstrapGrant {
    nonce: String,
    expires_at: Instant,
}

fn handle_connection(
    stream: &mut TcpStream,
    host: &Arc<Mutex<AppHost>>,
    token: &str,
    bootstrap: Option<&Arc<Mutex<Option<BootstrapGrant>>>>,
) -> Result<(), String> {
    stream
        .set_read_timeout(Some(REQUEST_TIMEOUT))
        .map_err(|error| error.to_string())?;
    let request = read_request(stream)?;
    let (route, query) = split_target(&request.target);
    if request.method == "GET" && route == BOOTSTRAP_ROUTE {
        let valid = bootstrap.is_some_and(|state| {
            form_value(query, "nonce").is_some_and(|nonce| consume_bootstrap(state, &nonce))
        });
        return if valid {
            write_redirect(stream, token)
        } else {
            write_response(
                stream,
                "401 Unauthorized",
                "application/json; charset=utf-8",
                &error_json("E2104", "invalid or expired local-app bootstrap"),
            )
        };
    }
    let public = request.method == "GET" && (route == "/" || route == "/index.html");
    if !public && !authorized(&request, token) {
        return write_response(
            stream,
            "401 Unauthorized",
            "application/json; charset=utf-8",
            &error_json("E2104", "missing bearer token"),
        );
    }
    if public {
        return write_response(
            stream,
            "200 OK",
            "text/html; charset=utf-8",
            APP_HTML,
        );
    }
    let mut host = host
        .lock()
        .map_err(|_| "local app host lock poisoned".to_string())?;
    host.reload_if_changed();
    if request.method == "GET" && route == "/health" {
        return write_response(
            stream,
            "200 OK",
            "application/json; charset=utf-8",
            &format!(
                "{{\"ok\":true,\"schema\":{},\"sessions\":{},\"generation\":{}}}",
                json_str(RECEIPT_SCHEMA),
                host.sessions.len(),
                host.generation,
            ),
        );
    }
    if request.method == "GET" && route == "/api/receipt" {
        return write_response(
            stream,
            "200 OK",
            "application/json; charset=utf-8",
            &host.receipt_json(),
        );
    }
    if request.method != "POST" || !route.starts_with("/api/") {
        return write_response(stream, "404 Not Found", "text/plain; charset=utf-8", "not found");
    }
    let response = api_message(&mut host, route, &request.body, session_header(&request));
    let (status, body) = match response {
        ApiResponse::Ok(body) => ("200 OK", body),
        ApiResponse::Error(body) => ("400 Bad Request", body),
    };
    write_response(
        stream,
        status,
        "application/json; charset=utf-8",
        &body,
    )
}

enum ApiResponse {
    Ok(String),
    Error(String),
}

fn api_message(
    host: &mut AppHost,
    route: &str,
    body: &str,
    session: Option<&str>,
) -> ApiResponse {
    if route == "/api/session" {
        return match host.create_session() {
            Ok(id) => ApiResponse::Ok(format!(
                "{{\"ok\":true,\"schema\":{},\"session\":{},\"receipt\":{},\"state\":{}}}",
                json_str(RECEIPT_SCHEMA),
                json_str(&id),
                host.receipt_json(),
                host.state(&id).unwrap_or_else(|_| "null".into()),
            )),
            Err(error) => ApiResponse::Error(error_json("E2105", &error)),
        };
    }
    let Some(session_id) = session.filter(|id| !id.is_empty()) else {
        return ApiResponse::Error(error_json(
            "E2104",
            "local-app API request needs X-Jet-App-Session",
        ));
    };
    if !host.sessions.contains_key(session_id) {
        return ApiResponse::Error(error_json("E2104", "local app session does not exist"));
    }
    match route {
        "/api/state" => match host.state(session_id) {
            Ok(body) => ApiResponse::Ok(body),
            Err(error) => ApiResponse::Error(error_json("E2104", &error)),
        },
        "/api/call" => match call_message(host, session_id, body) {
            Ok(body) => ApiResponse::Ok(body),
            Err(error) => ApiResponse::Error(error),
        },
        "/api/reset" => match host.reset_session(session_id) {
            Ok(()) => {
                let callback = host.callback(session_id).unwrap_or_default();
                ApiResponse::Ok(format!(
                    "{{\"ok\":true,\"callback\":{},\"reason\":\"explicit reset\",\"state\":{}}}",
                    callback,
                    host.state(session_id).unwrap_or_else(|_| "null".into()),
                ))
            }
            Err(error) => ApiResponse::Error(error_json("E2105", &error)),
        },
        "/api/reconnect" => match reconnect_message(host, session_id, body) {
            Ok(body) => ApiResponse::Ok(body),
            Err(error) => ApiResponse::Error(error),
        },
        "/api/receipt" => ApiResponse::Ok(host.receipt_json()),
        other => ApiResponse::Error(error_json(
            "E2104",
            &format!("unknown local-app route `{other}`"),
        )),
    }
}

fn call_message(host: &mut AppHost, id: &str, body: &str) -> Result<String, String> {
    let values = parse_values(body).map_err(|error| error_json("E2104", &error))?;
    let definition = host.definition.clone();
    let schema = host.schema.clone();
    let result = (|| -> Result<_, String> {
        let session = host
            .sessions
            .get_mut(id)
            .ok_or_else(|| error_json("E2104", "local app session does not exist"))?;
        for (name, value) in values {
            let control = definition
                .controls
                .iter()
                .find(|control| control.name == name || control.label == name)
                .ok_or_else(|| {
                    error_json(
                        "E2104",
                        &format!("checked app function has no control `{name}`"),
                    )
                })?;
            session
                .kernel
                .set_control(&control.function, &control.name, value.clone())
                .map_err(|error| checked_error_json("E2104", &error))?;
            session.values.insert(control.name.clone(), value);
        }
        let current = values_as_controls(&schema, &session.values);
        for control in &current {
            if control.schema.required && control.value.is_empty() {
                return Err(error_json(
                    "E2104",
                    &format!("required checked control `{}` has no value", control.schema.label),
                ));
            }
        }
        let args = current
            .iter()
            .filter(|control| !control.value.is_empty())
            .map(|control| format!("{}: {}", control.schema.name, control.value))
            .collect::<Vec<_>>()
            .join(", ");
        let source = format!("{}({args})", definition.function);
        session
            .kernel
            .edit_cell(&session.call_cell, source)
            .map_err(|error| error_json("E2104", &error))?;
        session
            .kernel
            .execute_cell(ClientKind::FirstParty, &session.call_cell)
            .map_err(|error| error_json("E2105", &error))
    })()?;
    let callback = {
        let session = host
            .sessions
            .get_mut(id)
            .ok_or_else(|| error_json("E2104", "local app session does not exist"))?;
        session.callback = session.callback.saturating_add(1);
        session.callback
    };
    let state = host
        .state(id)
        .map_err(|error| error_json("E2105", &error))?;
    Ok(format!(
        "{{\"ok\":true,\"callback\":{},\"result\":{{\"ok\":{},\"elapsed_ms\":{},\"text\":{}}},\"state\":{}}}",
        callback,
        result.ok(),
        result.elapsed_ms,
        json_str(&result.bundle.text_plain),
        state,
    ))
}

fn reconnect_message(host: &mut AppHost, id: &str, body: &str) -> Result<String, String> {
    let identity = parse_identity(body).map_err(|error| error_json("E2104", &error))?;
    {
        let session = host
            .sessions
            .get(id)
            .ok_or_else(|| error_json("E2104", "local app session does not exist"))?;
        session
            .kernel
            .reconnect(&identity)
            .map_err(|error| checked_error_json("E2104", &error))?;
    }
    let callback = {
        let session = host
            .sessions
            .get_mut(id)
            .ok_or_else(|| error_json("E2104", "local app session does not exist"))?;
        session.callback = session.callback.saturating_add(1);
        session.callback
    };
    let state = host
        .state(id)
        .map_err(|error| error_json("E2105", &error))?;
    Ok(format!(
        "{{\"ok\":true,\"callback\":{},\"state\":{}}}",
        callback, state,
    ))
}


fn parse_values(body: &str) -> Result<BTreeMap<String, String>, String> {
    let value = parse_json(body).map_err(|_| "local-app call body is not valid JSON".to_string())?;
    let DataTree::Object(root) = value else {
        return Err("local-app call body must be a JSON object".into());
    };
    let values = match root
        .iter()
        .find_map(|(field, value)| (field == "values").then_some(value))
    {
        Some(DataTree::Object(values)) => values,
        Some(_) => return Err("local-app call `values` must be a JSON object".into()),
        None => return Ok(BTreeMap::new()),
    };
    values
        .iter()
        .map(|(name, value)| {
            let value = match value {
                DataTree::Text(value) | DataTree::TypedText(value) => value.clone(),
                DataTree::Bool(value) => value.to_string(),
                DataTree::Int(value) => value.to_string(),
                DataTree::Float(value) => value.to_string(),
                DataTree::Number(value) => value.clone(),
                DataTree::Null => "null".into(),
                DataTree::Array(_) | DataTree::Object(_) | DataTree::Bytes(_) => {
                    return Err(format!("local-app control `{name}` must be a scalar Jet literal"))
                }
            };
            Ok((name.clone(), value))
        })
        .collect()
}

fn parse_identity(body: &str) -> Result<KernelIdentity, String> {
    let value = parse_json(body).map_err(|_| "reconnect body is not valid JSON".to_string())?;
    let root = value
        .as_object()
        .map_err(|_| "reconnect body must be a JSON object".to_string())?;
    let identity = root
        .iter()
        .find_map(|(field, value)| (field == "identity").then_some(value))
        .ok_or_else(|| "reconnect body is missing identity".to_string())?
        .as_object()
        .map_err(|_| "reconnect identity must be a JSON object".to_string())?;
    Ok(KernelIdentity {
        source: required_string(identity, "source")?,
        build: required_string(identity, "build")?,
        session: required_string(identity, "session")?,
        authority: required_string(identity, "authority")?,
    })
}

fn session_header(request: &Request) -> Option<&str> {
    request
        .headers
        .get("x-jet-app-session")
        .map(String::as_str)
}

fn flag_value<'a>(raw: &'a [String], name: &str) -> Option<&'a str> {
    raw.iter()
        .find_map(|arg| arg.strip_prefix(&format!("{name}=")))
        .or_else(|| {
            raw.iter()
                .position(|arg| arg == name)
                .and_then(|index| raw.get(index + 1))
                .filter(|value| !value.starts_with('-'))
                .map(String::as_str)
        })
}

fn mint_token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    let mut source = fs::File::open("/dev/urandom").map_err(|error| error.to_string())?;
    source
        .read_exact(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn open_local_app_browser(url: &str) {
    let explicit = std::env::var_os("JET_CANVAS_BROWSER")
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var_os("BROWSER").filter(|value| !value.is_empty()));
    let (program, args) = if let Some(browser) = explicit {
        (browser, vec![url.to_string()])
    } else {
        #[cfg(target_os = "macos")]
        {
            ("open".into(), vec![url.to_string()])
        }
        #[cfg(target_os = "windows")]
        {
            ("explorer.exe".into(), vec![url.to_string()])
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            ("xdg-open".into(), vec![url.to_string()])
        }
    };
    let _ = Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

struct Request {
    method: String,
    target: String,
    headers: HashMap<String, String>,
    body: String,
}

fn read_request(stream: &mut TcpStream) -> Result<Request, String> {
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0u8; 4096];
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("request deadline exceeded".into());
        }
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|error| error.to_string())?;
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("request ended before headers".into());
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > MAX_REQUEST_HEADERS {
            return Err("request headers exceed 64 KiB".into());
        }
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let header = std::str::from_utf8(&bytes[..header_end]).map_err(|_| "headers are not UTF-8")?;
    let mut lines = header.lines();
    let mut request_line = lines
        .next()
        .ok_or("missing request line")?
        .split_whitespace();
    let method = request_line.next().unwrap_or_default().to_string();
    let target = request_line.next().unwrap_or_default().to_string();
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let length = headers
        .get("content-length")
        .map(|value| value.parse::<usize>().map_err(|_| "invalid content length"))
        .transpose()?
        .unwrap_or(0);
    if length > MAX_REQUEST_BODY {
        return Err("request body exceeds 8 MiB".into());
    }
    while bytes.len() < header_end + length {
        let mut chunk = [0u8; 4096];
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("request deadline exceeded".into());
        }
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|error| error.to_string())?;
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("request ended before body".into());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    let body = String::from_utf8(bytes[header_end..header_end + length].to_vec())
        .map_err(|_| "request body is not UTF-8")?;
    Ok(Request {
        method,
        target,
        headers,
        body,
    })
}

fn split_target(target: &str) -> (&str, &str) {
    target.split_once('?').unwrap_or((target, ""))
}

fn authorized(request: &Request, token: &str) -> bool {
    let expected = format!("Bearer {token}");
    request
        .headers
        .get("authorization")
        .is_some_and(|value| constant_time_eq(value.as_bytes(), expected.as_bytes()))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= usize::from(
            left.get(index).copied().unwrap_or_default()
                ^ right.get(index).copied().unwrap_or_default(),
        );
    }
    difference == 0
}

fn consume_bootstrap(state: &Arc<Mutex<Option<BootstrapGrant>>>, nonce: &str) -> bool {
    let Ok(mut grant) = state.lock() else {
        return false;
    };
    if grant
        .as_ref()
        .is_some_and(|grant| Instant::now() >= grant.expires_at)
    {
        *grant = None;
        return false;
    }
    let matches = grant
        .as_ref()
        .is_some_and(|grant| constant_time_eq(grant.nonce.as_bytes(), nonce.as_bytes()));
    if matches {
        *grant = None;
    }
    matches
}

fn form_value(body: &str, key: &str) -> Option<String> {
    body.split('&').find_map(|part| {
        let (name, value) = part.split_once('=')?;
        (decode_component(name).ok()? == key)
            .then(|| decode_component(value).ok())
            .flatten()
    })
}

fn decode_component(value: &str) -> Result<String, String> {
    let mut out = Vec::new();
    let mut chars = value.bytes();
    while let Some(byte) = chars.next() {
        match byte {
            b'+' => out.push(b' '),
            b'%' => {
                let high = chars.next().ok_or("incomplete percent escape")?;
                let low = chars.next().ok_or("incomplete percent escape")?;
                let digits = [high, low];
                let text = std::str::from_utf8(&digits).map_err(|_| "invalid percent escape")?;
                out.push(u8::from_str_radix(text, 16).map_err(|_| "invalid percent escape")?);
            }
            byte => out.push(byte),
        }
    }
    String::from_utf8(out).map_err(|_| "form value is not UTF-8".into())
}

fn write_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &str,
) -> Result<(), String> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|error| error.to_string())
}

fn write_redirect(stream: &mut TcpStream, token: &str) -> Result<(), String> {
    let location = format!("/#token={}", fragment_component(token));
    let response = format!(
        "HTTP/1.1 303 See Other\r\nLocation: {location}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|error| error.to_string())
}

fn fragment_component(value: &str) -> String {
    value.bytes().fold(String::new(), |mut encoded, byte| {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
        encoded
    })
}

fn error_json(code: &str, message: &str) -> String {
    format!(
        "{{\"ok\":false,\"error\":{{\"code\":{},\"what\":{},\"why\":{},\"fix\":{}}}}}",
        json_str(code),
        json_str(message),
        json_str("the local app accepts only checked schema data and an existing tab session"),
        json_str("refresh the app state, correct the typed input, and retry"),
    )
}

fn checked_error_json(code: &str, message: &str) -> String {
    match parse_json(message) {
        Ok(DataTree::Object(_)) => format!("{{\"ok\":false,\"error\":{message}}}"),
        _ => error_json(code, message),
    }
}

fn json_str(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

const APP_HTML: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Jet local app</title>
<style>
:root{color-scheme:dark;font:15px system-ui,sans-serif;background:#111827;color:#e5e7eb}
body{max-width:960px;margin:2rem auto;padding:0 1rem}button,input{font:inherit}button{border:1px solid #4b5563;background:#1f2937;color:inherit;border-radius:.35rem;padding:.45rem .75rem;cursor:pointer}button:hover{border-color:#93c5fd}input{background:#030712;color:inherit;border:1px solid #4b5563;border-radius:.3rem;padding:.4rem;width:min(26rem,100%)}header{display:flex;justify-content:space-between;gap:1rem;align-items:start;border-bottom:1px solid #374151;padding-bottom:1rem}header h1{margin:0;font-size:1.35rem}header p{margin:.3rem 0;color:#9ca3af}.actions{display:flex;gap:.5rem;flex-wrap:wrap}.control{display:grid;gap:.35rem;margin:1rem 0}.control label{color:#d1d5db}.type{color:#9ca3af;font-size:.8rem}section{margin:1.25rem 0;padding:1rem;border:1px solid #374151;border-radius:.5rem;background:#172033}.cell{border-top:1px solid #374151;padding:.75rem 0}.cell:first-child{border-top:0}.cell small{color:#9ca3af}.cell pre{white-space:pre-wrap;overflow:auto;margin:.4rem 0 0}.status{color:#93c5fd;min-height:1.4rem}.error{color:#fca5a5}
</style>
</head>
<body>
<header><div><h1 id="title">Jet local app</h1><p id="meta">connecting…</p></div><div class="actions"><button id="reset">Reset</button><button id="reconnect">Reconnect</button></div></header>
<section><div id="controls"></div><button id="call">Run checked function</button><p id="status" class="status"></p></section>
<section><h2>Notebook outputs</h2><div id="cells"></div></section>
<script>
(() => {
  const token = new URLSearchParams((location.hash || '').slice(1)).get('token') || '';
  if (location.hash) history.replaceState(null, '', location.pathname);
  let session = sessionStorage.getItem('jet-local-app-session');
  let state = null;
  const auth = {'Authorization': `Bearer ${token}`, 'Content-Type':'application/json'};
  const api = async (path, body = {}) => {
    const headers = {...auth};
    if (session) headers['X-Jet-App-Session'] = session;
    const response = await fetch(path, {method:'POST', headers, body:JSON.stringify(body)});
    const value = await response.json();
    if (!response.ok || value.ok === false) throw new Error(value.error?.what || value.error || 'request failed');
    return value;
  };
  const outputNode = (output) => {
    const box = document.createElement('div'); box.className = 'output';
    if (!output) { box.textContent = 'No output yet.'; return box; }
    if (!output.live && !output.quarantined) { box.textContent = 'Stale output hidden. Run the function again.'; return box; }
    if (output.quarantined) {
      const notice = document.createElement('div'); notice.className = 'notice';
      notice.textContent = 'Rich output is quarantined; the safe text projection remains visible.';
      box.append(notice);
    }
    const text = document.createElement('pre'); text.textContent = output.text || '(no text/plain)'; box.append(text);
    for (const part of (output.mime || [])) {
      if (part.type === 'image/svg+xml') {
        const image = document.createElement('img'); image.alt = 'Jet app plot';
        image.src = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(part.data); box.append(image);
      } else if (part.type === 'text/html') {
        const frame = document.createElement('iframe'); frame.title = 'Sandboxed rich output';
        frame.sandbox = ''; frame.srcdoc = part.data; box.append(frame);
      }
    }
    return box;
  };
  const render = (value) => {
    state = value.state || value;
    const app = state.app || {};
    document.getElementById('title').textContent = `Jet · ${app.function || 'local app'}`;
    document.getElementById('meta').textContent = `${app.path || ''} · generation ${state.generation ?? 0}`;
    const controls = document.getElementById('controls'); controls.replaceChildren();
    for (const item of (app.controls || [])) {
      const wrap = document.createElement('div'); wrap.className = 'control';
      const label = document.createElement('label'); label.textContent = item.label || item.name;
      const type = document.createElement('span'); type.className = 'type'; type.textContent = item.type || 'checked value';
      const input = document.createElement('input'); input.dataset.name = item.name; input.value = item.value || '';
      input.placeholder = item.required ? 'required Jet literal' : 'optional Jet literal';
      wrap.append(label, type, input); controls.append(wrap);
    }
    const cells = document.getElementById('cells'); cells.replaceChildren();
    for (const cell of (state.state?.cells || state.cells || [])) {
      const box = document.createElement('article'); box.className = 'cell';
      const name = document.createElement('small'); name.textContent = `${cell.kind || 'jet'} · ${cell.status || 'idle'}`;
      box.append(name, outputNode(cell.output)); cells.append(box);
    }
    const reload = state.reload;
    document.getElementById('status').textContent = reload?.stale ? `${reload.status}: ${reload.reason}` : '';
  };
  const refresh = async () => { try { render(await api('/api/state')); } catch (error) { document.getElementById('status').textContent = error.message; } };
  const start = async () => {
    try {
      if (!session) { const value = await api('/api/session'); session = value.session; sessionStorage.setItem('jet-local-app-session', session); }
      await refresh();
    } catch (error) { document.getElementById('status').textContent = error.message; }
  };
  document.getElementById('call').onclick = async () => {
    const values = {}; for (const input of document.querySelectorAll('#controls input')) values[input.dataset.name] = input.value;
    try { render(await api('/api/call', {values})); } catch (error) { document.getElementById('status').textContent = error.message; }
  };
  document.getElementById('reset').onclick = async () => { try { render(await api('/api/reset')); } catch (error) { document.getElementById('status').textContent = error.message; } };
  document.getElementById('reconnect').onclick = async () => { try { render(await api('/api/reconnect', {identity: state.state?.identity || state.identity})); } catch (error) { document.getElementById('status').textContent = error.message; } };
  start(); setInterval(refresh, 1500);
})();
</script>
</body>
</html>"##;
