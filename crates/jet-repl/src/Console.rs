//! D-DX-CONSOLE1=A — a capability-gated project console over the existing REPL.
//!
//! This module is deliberately transport-neutral.  The command line and
//! devtools hosts provide the in-process router and data adapter; this module
//! owns session identity, authority checks, bounded structured output, and the
//! real transaction/savepoint lifecycle.  It never starts a second HTTP
//! server, invents a database, or treats a loopback request as trusted merely
//! because it came from the console process.

#![allow(dead_code)]

use super::{
    collect_moved_names, display_value, rebuild_funcs, repl_executable_stmts,
    restore_move_diagnostic, type_check_item, type_check_stmts, update_core_imports_from_ledger,
    ReplAuthorization, ReplFlags, ReplPolicy, Session,
};
use crate::Comptime::{CtValue, DevSink, ReplAuthorizer, ReplEffectRequest, REPL_FUEL_BUDGET};
use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{Func, Stmt, StructDef};
use jet_foundation::Authority::{answer, parse_right, root, Authority, Holds, Verdict};
use jet_foundation::SHA256;
use std::any::Any;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::io::BufRead;
use std::path::{Path, PathBuf};

pub const CONSOLE_PROTOCOL: &str = "jet.console.v1";
pub const CONSOLE_MAX_HISTORY: usize = 256;
pub const CONSOLE_MAX_AUDIT: usize = 256;
pub const CONSOLE_MAX_OUTPUTS: usize = 256;
pub const CONSOLE_MAX_LOG_BYTES: usize = 64 * 1024;
pub const CONSOLE_MAX_RESULT_BYTES: usize = 64 * 1024;
pub const CONSOLE_MAX_HEADERS: usize = 64;
pub const CONSOLE_MAX_TOKEN_BYTES: usize = 256;
pub const CONSOLE_MAX_TEXT_BYTES: usize = 64 * 1024;
pub const CONSOLE_MAX_PARAMETERS: usize = 64;
pub const CONSOLE_MAX_BODY_BYTES: usize = 1024 * 1024;
pub const CONSOLE_MAX_ROWS: usize = 256;
pub const CONSOLE_DEFAULT_TTL_MS: u64 = 30 * 60 * 1000;

/// Console access starts read-only.  `SandboxData` permits a real transaction,
/// but still requires the `DB.Write` authority before it can be opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleMode {
    ReadOnly,
    SandboxData,
}

impl ConsoleMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::SandboxData => "sandbox-data",
        }
    }
}

/// One capability spelling shared by the console command surface and the
/// authority carrier.  The authority itself remains the only grant set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConsoleCapability {
    Repl,
    ServiceRead,
    DbRead,
    DbWrite,
    HttpRequest,
    Commit,
}

impl ConsoleCapability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Repl => "REPL.Execute",
            Self::ServiceRead => "Service.Read",
            Self::DbRead => "DB.Read",
            Self::DbWrite => "DB.Write",
            Self::HttpRequest => "Net.Connect",
            Self::Commit => "DB.Commit",
        }
    }

    pub fn required_right(self) -> &'static str {
        match self {
            Self::Repl => "IO",
            Self::ServiceRead => "Service.Read",
            Self::DbRead => "DB.Read",
            Self::DbWrite | Self::Commit => "DB.Write",
            Self::HttpRequest => "Net.Connect",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        let normalized = value.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "repl" | "repl.execute" | "io" => Some(Self::Repl),
            "service" | "service.read" => Some(Self::ServiceRead),
            "db" | "db.read" => Some(Self::DbRead),
            "db.write" | "write" => Some(Self::DbWrite),
            "net" | "net.connect" | "http" | "http.request" => Some(Self::HttpRequest),
            "db.commit" | "commit" => Some(Self::Commit),
            _ => None,
        }
    }
}

/// Stable identity of one project console.  The nonce lets callers create a
/// fresh session while preserving deterministic identity for a replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleIdentity {
    pub project_id: String,
    pub source_revision: String,
    pub session_id: String,
}

impl ConsoleIdentity {
    pub fn new(
        project_root: &Path,
        source_revision: impl Into<String>,
        session_nonce: impl Into<String>,
    ) -> Result<Self, ConsoleError> {
        let source_revision = source_revision.into();
        let session_nonce = session_nonce.into();
        validate_token("source revision", &source_revision)?;
        validate_token("session nonce", &session_nonce)?;
        let project_seed = format!("{CONSOLE_PROTOCOL}\0project\0{}", project_root.display());
        let project_digest = SHA256::sha256_hex(project_seed.as_bytes());
        let session_seed = format!(
            "{CONSOLE_PROTOCOL}\0session\0{}\0{}\0{}",
            project_digest, source_revision, session_nonce
        );
        let session_digest = SHA256::sha256_hex(session_seed.as_bytes());
        Ok(Self {
            project_id: format!("project-{}", &project_digest[..24]),
            source_revision,
            session_id: format!("console-{}", &session_digest[..24]),
        })
    }
}

/// A declared service handle.  The handle is an identity and status
/// projection; the service implementation stays owned by the host graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleServiceHandle {
    pub name: String,
    pub identity: String,
    pub state: String,
}

impl ConsoleServiceHandle {
    pub fn new(
        name: impl Into<String>,
        identity: impl Into<String>,
        state: impl Into<String>,
    ) -> Result<Self, ConsoleError> {
        let name = name.into();
        let identity = identity.into();
        let state = state.into();
        validate_token("service name", &name)?;
        validate_token("service identity", &identity)?;
        validate_token("service state", &state)?;
        Ok(Self {
            name,
            identity,
            state,
        })
    }
}

/// A declared database handle.  It contains no fake connection or rollback
/// implementation; the host supplies that through [`ConsoleDataBackend`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleDatabaseHandle {
    pub name: String,
    pub identity: String,
}

impl ConsoleDatabaseHandle {
    pub fn new(name: impl Into<String>, identity: impl Into<String>) -> Result<Self, ConsoleError> {
        let name = name.into();
        let identity = identity.into();
        validate_token("database name", &name)?;
        validate_token("database identity", &identity)?;
        Ok(Self { name, identity })
    }
}

/// Project facts loaded before the first console turn.  Source loading uses
/// the same REPL loader as the ordinary `jet repl --project` path; declared
/// handles are attached by the application/service host rather than guessed
/// from arbitrary source text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleProject {
    pub root: PathBuf,
    pub identity: ConsoleIdentity,
    pub services: Vec<ConsoleServiceHandle>,
    pub databases: Vec<ConsoleDatabaseHandle>,
}

impl ConsoleProject {
    pub fn new(
        root: impl Into<PathBuf>,
        source_revision: impl Into<String>,
    ) -> Result<Self, ConsoleError> {
        let root = root.into();
        if !root.is_dir() {
            return Err(ConsoleError::ProjectUnavailable(format!(
                "project root `{}` is not a directory",
                root.display()
            )));
        }
        let identity = ConsoleIdentity::new(&root, source_revision, "default")?;
        Ok(Self {
            root,
            identity,
            services: Vec::new(),
            databases: Vec::new(),
        })
    }

    pub fn from_root(root: impl Into<PathBuf>) -> Result<Self, ConsoleError> {
        Self::new(root, "working-tree")
    }
    fn validate(&self) -> Result<(), ConsoleError> {
        if !self.root.is_dir() {
            return Err(ConsoleError::ProjectUnavailable(format!(
                "project root `{}` is not a directory",
                self.root.display()
            )));
        }
        validate_token("project id", &self.identity.project_id)?;
        validate_token("source revision", &self.identity.source_revision)?;
        validate_token("session id", &self.identity.session_id)?;
        let expected = ConsoleIdentity::new(
            &self.root,
            self.identity.source_revision.clone(),
            "default",
        )?;
        if self.identity.project_id != expected.project_id {
            return Err(ConsoleError::InvalidInput(
                "project identity does not match its declared root".to_string(),
            ));
        }
        for service in &self.services {
            validate_token("service name", &service.name)?;
            validate_token("service identity", &service.identity)?;
            validate_token("service state", &service.state)?;
        }
        for database in &self.databases {
            validate_token("database name", &database.name)?;
            validate_token("database identity", &database.identity)?;
        }
        let mut service_names = BTreeSet::new();
        if self
            .services
            .iter()
            .any(|service| !service_names.insert(service.name.as_str()))
        {
            return Err(ConsoleError::InvalidInput(
                "project declares duplicate service handles".to_string(),
            ));
        }
        let mut database_names = BTreeSet::new();
        if self
            .databases
            .iter()
            .any(|database| !database_names.insert(database.name.as_str()))
        {
            return Err(ConsoleError::InvalidInput(
                "project declares duplicate database handles".to_string(),
            ));
        }
        Ok(())
    }

    pub fn with_identity(
        mut self,
        source_revision: impl Into<String>,
        session_nonce: impl Into<String>,
    ) -> Result<Self, ConsoleError> {
        self.identity = ConsoleIdentity::new(&self.root, source_revision, session_nonce)?;
        Ok(self)
    }

    pub fn add_service(&mut self, handle: ConsoleServiceHandle) -> Result<(), ConsoleError> {
        if self.services.iter().any(|existing| existing.name == handle.name) {
            return Err(ConsoleError::InvalidInput(format!(
                "service `{}` is declared more than once",
                handle.name
            )));
        }
        self.services.push(handle);
        self.services.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(())
    }

    pub fn add_database(&mut self, handle: ConsoleDatabaseHandle) -> Result<(), ConsoleError> {
        if self
            .databases
            .iter()
            .any(|existing| existing.name == handle.name)
        {
            return Err(ConsoleError::InvalidInput(format!(
                "database `{}` is declared more than once",
                handle.name
            )));
        }
        self.databases.push(handle);
        self.databases.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(())
    }

    pub fn with_service(mut self, handle: ConsoleServiceHandle) -> Result<Self, ConsoleError> {
        self.add_service(handle)?;
        Ok(self)
    }

    pub fn with_database(mut self, handle: ConsoleDatabaseHandle) -> Result<Self, ConsoleError> {
        self.add_database(handle)?;
        Ok(self)
    }

    pub fn has_database(&self, name: &str) -> bool {
        self.databases.iter().any(|database| database.name == name)
    }

    pub fn has_service(&self, name: &str) -> bool {
        self.services.iter().any(|service| service.name == name)
    }

    pub fn bindings_json(&self) -> String {
        let services = self
            .services
            .iter()
            .map(|service| {
                format!(
                    "{{\"name\":{},\"identity\":{},\"state\":{}}}",
                    quote(&service.name),
                    quote(&service.identity),
                    quote(&service.state),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let databases = self
            .databases
            .iter()
            .map(|database| {
                format!(
                    "{{\"name\":{},\"identity\":{}}}",
                    quote(&database.name),
                    quote(&database.identity),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"project\":{},\"services\":[{}],\"databases\":[{}]}}",
            quote(&self.identity.project_id),
            services,
            databases,
        )
    }
}

/// Options are explicit so release builds, authority, loopback trust, and
/// expiry cannot be inferred from ambient process state.
#[derive(Clone, Debug)]
pub struct ConsoleOptions {
    pub project: ConsoleProject,
    pub mode: ConsoleMode,
    pub authority: Authority,
    pub denied: Holds,
    pub session_nonce: String,
    pub now_ms: u64,
    pub ttl_ms: u64,
    pub release_build: bool,
    pub loopback: bool,
    pub max_history: usize,
    pub max_outputs: usize,
    pub max_audit: usize,
    pub script: Option<String>,
}

impl ConsoleOptions {
    pub fn new(project: ConsoleProject) -> Self {
        Self {
            project,
            mode: ConsoleMode::ReadOnly,
            authority: Authority::from_rights(["IO", "Mem.Alloc"]),
            denied: Holds::new(),
            session_nonce: "default".to_string(),
            now_ms: 0,
            ttl_ms: CONSOLE_DEFAULT_TTL_MS,
            release_build: false,
            loopback: true,
            max_history: CONSOLE_MAX_HISTORY,
            max_outputs: CONSOLE_MAX_OUTPUTS,
            max_audit: CONSOLE_MAX_AUDIT,
            script: None,
        }
    }

    pub fn for_project(root: impl Into<PathBuf>) -> Result<Self, ConsoleError> {
        Ok(Self::new(ConsoleProject::from_root(root)?))
    }

    pub fn with_mode(mut self, mode: ConsoleMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_authority(mut self, authority: Authority) -> Self {
        self.authority = authority;
        self
    }

    pub fn with_rights<I, S>(mut self, rights: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.authority = Authority::from_rights(rights);
        self
    }

    pub fn with_denied<I, S>(mut self, rights: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.denied = rights
            .into_iter()
            .map(Into::into)
            .map(|right: String| parse_right(&right).unwrap_or(right))
            .collect();
        self
    }

    pub fn with_session_nonce(mut self, nonce: impl Into<String>) -> Self {
        self.session_nonce = nonce.into();
        self
    }

    pub fn with_now_ms(mut self, now_ms: u64) -> Self {
        self.now_ms = now_ms;
        self
    }

    pub fn with_ttl_ms(mut self, ttl_ms: u64) -> Self {
        self.ttl_ms = ttl_ms;
        self
    }

    pub fn release_build(mut self, release: bool) -> Self {
        self.release_build = release;
        self
    }

    pub fn with_loopback(mut self, loopback: bool) -> Self {
        self.loopback = loopback;
        self
    }

    pub fn with_script(mut self, script: impl Into<String>) -> Self {
        self.script = Some(script.into());
        self
    }

    pub fn with_limits(mut self, history: usize, outputs: usize, audit: usize) -> Self {
        self.max_history = history.max(1);
        self.max_outputs = outputs.max(1);
        self.max_audit = audit.max(1);
        self
    }
}

/// Typed request sent to the existing in-process application router.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleRequest {
    pub request_id: String,
    pub session_id: String,
    pub method: String,
    pub path: String,
    pub origin: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl ConsoleRequest {
    pub fn new(
        request_id: impl Into<String>,
        session_id: impl Into<String>,
        method: impl Into<String>,
        path: impl Into<String>,
    ) -> Result<Self, ConsoleError> {
        let request_id = request_id.into();
        let session_id = session_id.into();
        let method = method.into().to_ascii_uppercase();
        let path = path.into();
        validate_token("request id", &request_id)?;
        validate_token("session id", &session_id)?;
        validate_method(&method)?;
        validate_local_path(&path)?;
        Ok(Self {
            request_id,
            session_id,
            method,
            path,
            origin: "127.0.0.1".to_string(),
            headers: BTreeMap::new(),
            body: Vec::new(),
        })
    }
    pub fn with_origin(mut self, origin: impl Into<String>) -> Result<Self, ConsoleError> {
        let origin = origin.into();
        validate_origin(&origin)?;
        self.origin = origin;
        Ok(self)
    }
    pub fn with_header(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, ConsoleError> {
        let name = name.into().to_ascii_lowercase();
        if self.headers.len() >= CONSOLE_MAX_HEADERS && !self.headers.contains_key(&name) {
            return Err(ConsoleError::LimitExceeded("request headers"));
        }
        let value = value.into();
        validate_token("header name", &name)?;
        if is_secret_header(&name) {
            return Err(ConsoleError::InvalidRequest(
                "credential headers are not permitted".to_string(),
            ));
        }
        validate_text("header value", &value)?;
        self.headers.insert(name, value);
        Ok(self)
    }

    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Result<Self, ConsoleError> {
        let body = body.into();
        if body.len() > CONSOLE_MAX_BODY_BYTES {
            return Err(ConsoleError::LimitExceeded("request body"));
        }
        self.body = body;
        self.validate()?;
        Ok(self)
    }
    pub fn is_loopback(&self) -> bool {
        is_loopback_host(&self.origin)
    }

    fn validate(&self) -> Result<(), ConsoleError> {
        validate_token("request id", &self.request_id)?;
        validate_token("session id", &self.session_id)?;
        validate_method(&self.method)?;
        validate_local_path(&self.path)?;
        validate_origin(&self.origin)?;
        if self.headers.len() > CONSOLE_MAX_HEADERS {
            return Err(ConsoleError::LimitExceeded("request headers"));
        }
        for (name, value) in &self.headers {
            validate_token("header name", name)?;
            if is_secret_header(name) {
                return Err(ConsoleError::InvalidRequest(
                    "credential headers are not permitted".to_string(),
                ));
            }
            validate_text("header value", value)?;
        }
        if self.body.len() > CONSOLE_MAX_BODY_BYTES {
            return Err(ConsoleError::LimitExceeded("request body"));
        }
        validate_safe_request(&self.method, &self.body)
    }
}

/// Typed response returned by an in-process route handler.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleResponse {
    pub request_id: String,
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl ConsoleResponse {
    pub fn new(request_id: impl Into<String>, status: u16) -> Result<Self, ConsoleError> {
        let request_id = request_id.into();
        validate_token("response request id", &request_id)?;
        if !(100..=599).contains(&status) {
            return Err(ConsoleError::InvalidInput(format!(
                "response status {status} is outside 100..=599"
            )));
        }
        Ok(Self {
            request_id,
            status,
            headers: BTreeMap::new(),
            body: Vec::new(),
        })
    }

    pub fn with_header(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, ConsoleError> {
        let name = name.into().to_ascii_lowercase();
        let value = value.into();
        if self.headers.len() >= CONSOLE_MAX_HEADERS && !self.headers.contains_key(&name) {
            return Err(ConsoleError::LimitExceeded("response headers"));
        }
        validate_token("response header name", &name)?;
        validate_text("response header value", &value)?;
        self.headers.insert(name, value);
        Ok(self)
    }

    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Result<Self, ConsoleError> {
        let body = body.into();
        if body.len() > CONSOLE_MAX_BODY_BYTES {
            return Err(ConsoleError::LimitExceeded("response body"));
        }
        self.body = body;
        self.validate()?;
        Ok(self)
    }

    fn validate(&self) -> Result<(), ConsoleError> {
        validate_token("response request id", &self.request_id)?;
        if !(100..=599).contains(&self.status) {
            return Err(ConsoleError::InvalidInput(format!(
                "response status {} is outside 100..=599",
                self.status
            )));
        }
        if self.headers.len() > CONSOLE_MAX_HEADERS {
            return Err(ConsoleError::LimitExceeded("response headers"));
        }
        for (name, value) in &self.headers {
            validate_token("response header name", name)?;
            validate_text("response header value", value)?;
        }
        if self.body.len() > CONSOLE_MAX_BODY_BYTES {
            return Err(ConsoleError::LimitExceeded("response body"));
        }
        Ok(())
    }
}
/// Host-owned seam for dispatching requests without starting a second server.
///
/// The console validates session identity, loopback origin, authority, and
/// response bounds before invoking this adapter. Implementations own the
/// resident application router and cancellation for active request identities.
pub trait InProcessRouter {
    fn dispatch(&mut self, request: ConsoleRequest) -> Result<ConsoleResponse, ConsoleError>;
    fn cancel(&mut self, request_id: &str);
}


#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleDataRequest {
    pub database: String,
    pub statement_identity: String,
    pub operation: String,
    pub limit: usize,
}

impl ConsoleDataRequest {
    pub fn new(
        database: impl Into<String>,
        statement_identity: impl Into<String>,
        operation: impl Into<String>,
        limit: usize,
    ) -> Result<Self, ConsoleError> {
        let request = Self {
            database: database.into(),
            statement_identity: statement_identity.into(),
            operation: operation.into(),
            limit,
        };
        request.validate()?;
        Ok(request)
    }

    fn validate(&self) -> Result<(), ConsoleError> {
        validate_token("database", &self.database)?;
        validate_token("statement identity", &self.statement_identity)?;
        validate_token("data operation", &self.operation)?;
        if !(1..=CONSOLE_MAX_ROWS).contains(&self.limit) {
            return Err(ConsoleError::LimitExceeded("data row limit"));
        }
        Ok(())
    }
}

/// Structured data cell.  `Secret` is intentionally a separate carrier so a
/// backend can mark values redacted before they enter console output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConsoleDataValue {
    Null,
    Int(i64),
    Float(String),
    Bool(bool),
    Text(String),
    Bytes(Vec<u8>),
    Secret,
}

impl ConsoleDataValue {
    fn to_json(&self) -> String {
        match self {
            Self::Null => "null".to_string(),
            Self::Int(value) => value.to_string(),
            Self::Float(value) => {
                if value.parse::<f64>().map(|parsed| parsed.is_finite()).unwrap_or(false) {
                    value.clone()
                } else {
                    "null".to_string()
                }
            }
            Self::Bool(value) => value.to_string(),
            Self::Text(value) => quote(value),
            Self::Bytes(value) => quote(&hex_bytes(value)),
            Self::Secret => quote("<redacted>"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleDataColumn {
    pub name: String,
    pub type_name: String,
}

impl ConsoleDataColumn {
    pub fn new(name: impl Into<String>, type_name: impl Into<String>) -> Result<Self, ConsoleError> {
        let name = name.into();
        let type_name = type_name.into();
        validate_token("data column", &name)?;
        validate_token("data type", &type_name)?;
        Ok(Self { name, type_name })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ConsoleDataResult {
    pub columns: Vec<ConsoleDataColumn>,
    pub rows: Vec<Vec<ConsoleDataValue>>,
    pub truncated: bool,
}

impl ConsoleDataResult {
    pub fn to_json(&self) -> String {
        let columns = self
            .columns
            .iter()
            .map(|column| {
                format!(
                    "{{\"name\":{},\"type\":{}}}",
                    quote(&column.name),
                    quote(&column.type_name)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let rows = self
            .rows
            .iter()
            .take(CONSOLE_MAX_ROWS)
            .map(|row| {
                format!(
                    "[{}]",
                    row.iter()
                        .take(self.columns.len().max(1))
                        .map(ConsoleDataValue::to_json)
                        .collect::<Vec<_>>()
                        .join(",")
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"columns\":[{columns}],\"rows\":[{rows}],\"truncated\":{}}}",
            self.truncated || self.rows.len() > CONSOLE_MAX_ROWS
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleDataStatement {
    pub database: String,
    pub statement_identity: String,
    pub operation: String,
    pub parameters: Vec<ConsoleDataValue>,
}

impl ConsoleDataStatement {
    pub fn new(
        database: impl Into<String>,
        statement_identity: impl Into<String>,
        operation: impl Into<String>,
        parameters: Vec<ConsoleDataValue>,
    ) -> Result<Self, ConsoleError> {
        let statement = Self {
            database: database.into(),
            statement_identity: statement_identity.into(),
            operation: operation.into(),
            parameters,
        };
        statement.validate()?;
        Ok(statement)
    }

    fn validate(&self) -> Result<(), ConsoleError> {
        validate_token("database", &self.database)?;
        validate_token("statement identity", &self.statement_identity)?;
        validate_token("data operation", &self.operation)?;
        if self.parameters.len() > CONSOLE_MAX_PARAMETERS {
            return Err(ConsoleError::LimitExceeded("data parameters"));
        }
        for parameter in &self.parameters {
            validate_data_value(parameter)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleDataMutation {
    pub transaction_id: String,
    pub statement_identity: String,
    pub rows_affected: u64,
}

impl ConsoleDataMutation {
    fn validate(&self) -> Result<(), ConsoleError> {
        validate_token("transaction id", &self.transaction_id)?;
        validate_token("statement identity", &self.statement_identity)?;
        if self.rows_affected > CONSOLE_MAX_ROWS as u64 {
            return Err(ConsoleError::LimitExceeded("affected data rows"));
        }
        Ok(())
    }
}

/// Adapter to the project's real data seam.  A backend must implement actual
/// transaction behavior; the console never emulates rollback by restoring a
/// copied value.
pub trait ConsoleDataBackend {
    /// Resolve a checked, read-only statement before the backend executes it.
    /// Hosts may override this to map a public identity to a prepared query.
    fn resolve_inspect(
        &mut self,
        database: &ConsoleDatabaseHandle,
        request: &ConsoleDataRequest,
    ) -> Result<ConsoleDataRequest, ConsoleError> {
        request.validate()?;
        if request.database != database.name {
            return Err(ConsoleError::InvalidRequest(
                "data request database does not match its declared handle".to_string(),
            ));
        }
        if request.statement_identity != request.operation
            || !is_checked_read_operation(&request.operation)
        {
            return Err(ConsoleError::InvalidRequest(
                "data inspection requires a checked read statement identity".to_string(),
            ));
        }
        Ok(request.clone())
    }

    fn inspect(&mut self, request: &ConsoleDataRequest) -> Result<ConsoleDataResult, ConsoleError>;

    /// Resolve a checked write statement against the same declared handle.
    fn resolve_statement(
        &mut self,
        database: &ConsoleDatabaseHandle,
        statement: &ConsoleDataStatement,
    ) -> Result<ConsoleDataStatement, ConsoleError> {
        statement.validate()?;
        if statement.database != database.name {
            return Err(ConsoleError::InvalidRequest(
                "data statement database does not match its declared handle".to_string(),
            ));
        }
        if statement.statement_identity != statement.operation
            || !is_checked_write_operation(&statement.operation)
        {
            return Err(ConsoleError::InvalidRequest(
                "data mutation requires a checked write statement identity".to_string(),
            ));
        }
        Ok(statement.clone())
    }

    fn begin(
        &mut self,
        database: &ConsoleDatabaseHandle,
        transaction_id: &str,
    ) -> Result<Box<dyn ConsoleDataTransaction>, ConsoleError>;
}

pub trait ConsoleDataTransaction {
    fn savepoint(&mut self, name: &str) -> Result<(), ConsoleError>;
    fn rollback_to(&mut self, name: &str) -> Result<(), ConsoleError>;
    fn execute(
        &mut self,
        statement: &ConsoleDataStatement,
    ) -> Result<ConsoleDataMutation, ConsoleError>;
    fn commit(&mut self) -> Result<(), ConsoleError>;
    fn rollback(&mut self) -> Result<(), ConsoleError>;
    fn writes(&self) -> u64;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleAuditOutcome {
    Allowed,
    Denied,
    Completed,
    Committed,
    RolledBack,
    Cancelled,
    Failed,
}

impl ConsoleAuditOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::Denied => "denied",
            Self::Completed => "completed",
            Self::Committed => "committed",
            Self::RolledBack => "rolled-back",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

/// Audit facts contain operation identities and digests, never request bodies,
/// SQL parameters, service credentials, or returned values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleAuditEvent {
    pub sequence: u64,
    pub session_id: String,
    pub request_id: Option<String>,
    pub operation: String,
    pub required_right: Option<String>,
    pub outcome: ConsoleAuditOutcome,
    pub reason: String,
    pub resource_digest: Option<String>,
    pub transaction_id: Option<String>,
}

impl ConsoleAuditEvent {
    fn new(
        session_id: &str,
        request_id: Option<String>,
        operation: impl Into<String>,
        required_right: Option<String>,
        outcome: ConsoleAuditOutcome,
        reason: impl Into<String>,
        resource: Option<&str>,
        transaction_id: Option<String>,
    ) -> Self {
        Self {
            sequence: 0,
            session_id: session_id.to_string(),
            request_id,
            operation: operation.into(),
            required_right,
            outcome,
            reason: bounded(reason.into(), 512),
            resource_digest: resource.map(|value| digest(value)),
            transaction_id,
        }
    }

    pub fn to_json(&self) -> String {
        format!(
            "{{\"sequence\":{},\"session_id\":{},\"request_id\":{},\"operation\":{},\"required_right\":{},\"outcome\":{},\"reason\":{},\"resource_digest\":{},\"transaction_id\":{}}}",
            self.sequence,
            quote(&self.session_id),
            optional_quote(self.request_id.as_deref()),
            quote(&self.operation),
            optional_quote(self.required_right.as_deref()),
            quote(self.outcome.as_str()),
            quote(&self.reason),
            optional_quote(self.resource_digest.as_deref()),
            optional_quote(self.transaction_id.as_deref()),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleGrant {
    pub capability: String,
    pub required_right: String,
    pub granted_at_ms: u64,
    pub expires_at_ms: u64,
}

impl ConsoleGrant {
    fn active_at(&self, now_ms: u64) -> bool {
        self.expires_at_ms == 0 || now_ms < self.expires_at_ms
    }

    fn to_json(&self, active: bool) -> String {
        format!(
            "{{\"capability\":{},\"required_right\":{},\"granted_at_ms\":{},\"expires_at_ms\":{},\"active\":{active}}}",
            quote(&self.capability),
            quote(&self.required_right),
            self.granted_at_ms,
            self.expires_at_ms,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleHistoryEntry {
    pub sequence: u64,
    pub input: String,
    pub status: String,
    pub output_digest: String,
}

#[derive(Clone, Debug)]
pub struct ConsoleHistory {
    entries: VecDeque<ConsoleHistoryEntry>,
    next_sequence: u64,
    limit: usize,
}

impl ConsoleHistory {
    pub fn new(limit: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            next_sequence: 1,
            limit: limit.max(1),
        }
    }

    fn record(&mut self, input: &str, status: &str, output: &str) {
        let entry = ConsoleHistoryEntry {
            sequence: self.next_sequence,
            input: bounded(input.to_string(), 4096),
            status: bounded(status.to_string(), 32),
            output_digest: digest(output),
        };
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.entries.push_back(entry);
        while self.entries.len() > self.limit {
            self.entries.pop_front();
        }
    }

    pub fn entries(&self) -> impl Iterator<Item = &ConsoleHistoryEntry> {
        self.entries.iter()
    }

    pub fn to_json(&self) -> String {
        let entries = self
            .entries
            .iter()
            .map(|entry| {
                format!(
                    "{{\"sequence\":{},\"input\":{},\"status\":{},\"output_digest\":{}}}",
                    entry.sequence,
                    quote(&entry.input),
                    quote(&entry.status),
                    quote(&entry.output_digest),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("[{}]", entries)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleOutputKind {
    Startup,
    Evaluation,
    Request,
    Data,
    Grant,
    Capabilities,
    Bindings,
    Audit,
    History,
    Sandbox,
    Commit,
    Rollback,
    Cancelled,
    Error,
    Goodbye,
}

impl ConsoleOutputKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Evaluation => "evaluation",
            Self::Request => "request",
            Self::Data => "data",
            Self::Grant => "grant",
            Self::Capabilities => "capabilities",
            Self::Bindings => "bindings",
            Self::Audit => "audit",
            Self::History => "history",
            Self::Sandbox => "sandbox",
            Self::Commit => "commit",
            Self::Rollback => "rollback",
            Self::Cancelled => "cancelled",
            Self::Error => "error",
            Self::Goodbye => "goodbye",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleOutput {
    pub sequence: u64,
    pub session_id: String,
    pub kind: ConsoleOutputKind,
    pub ok: bool,
    pub message: String,
    pub payload: String,
    pub request_id: Option<String>,
    pub transaction_id: Option<String>,
    pub truncated: bool,
}

impl ConsoleOutput {
    fn new(
        sequence: u64,
        session_id: &str,
        kind: ConsoleOutputKind,
        ok: bool,
        message: impl Into<String>,
        payload: impl Into<String>,
        request_id: Option<String>,
        transaction_id: Option<String>,
        truncated: bool,
    ) -> Self {
        let payload = payload.into();
        let payload_truncated = payload.len() > CONSOLE_MAX_RESULT_BYTES;
        Self {
            sequence,
            session_id: session_id.to_string(),
            kind,
            ok,
            message: bounded(message.into(), CONSOLE_MAX_RESULT_BYTES),
            payload: if payload_truncated {
                "{\"truncated\":true}".to_string()
            } else {
                payload
            },
            request_id,
            transaction_id,
            truncated: truncated || payload_truncated,
        }
    }

    pub fn to_json_line(&self) -> String {
        format!(
            "{{\"protocol\":{},\"sequence\":{},\"session_id\":{},\"kind\":{},\"ok\":{},\"message\":{},\"payload\":{},\"request_id\":{},\"transaction_id\":{},\"truncated\":{}}}",
            quote(CONSOLE_PROTOCOL),
            self.sequence,
            quote(&self.session_id),
            quote(self.kind.as_str()),
            self.ok,
            quote(&self.message),
            self.payload,
            optional_quote(self.request_id.as_deref()),
            optional_quote(self.transaction_id.as_deref()),
            self.truncated,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConsoleCommand {
    Eval(String),
    Request {
        method: String,
        path: String,
        body: Vec<u8>,
    },
    Inspect(ConsoleDataRequest),
    BeginSandbox,
    Savepoint(String),
    ResetSandbox,
    Execute(ConsoleDataStatement),
    Commit(String),
    ResetRepl,
    Grant {
        capability: ConsoleCapability,
        ttl_ms: u64,
    },
    Capabilities,
    Bindings,
    Audit,
    History,
    Cancel,
    Quit,
    Help,
}

impl ConsoleCommand {
    pub fn parse(input: &str) -> Result<Self, ConsoleError> {
        if input.len() > CONSOLE_MAX_TEXT_BYTES {
            return Err(ConsoleError::LimitExceeded("command"));
        }
        let input = input.trim();
        if input.is_empty() {
            return Ok(Self::Eval(String::new()));
        }
        let Some(meta) = input.strip_prefix(':') else {
            return Ok(Self::Eval(input.to_string()));
        };
        let (name, rest) = meta
            .split_once(char::is_whitespace)
            .map(|(name, rest)| (name, rest.trim()))
            .unwrap_or((meta, ""));
        match name.to_ascii_lowercase().as_str() {
            "q" | "quit" | "exit" => {
                require_no_args(rest, "usage: :quit")?;
                Ok(Self::Quit)
            }
            "help" => {
                require_no_args(rest, "usage: :help")?;
                Ok(Self::Help)
            }
            "capabilities" | "caps" => {
                require_no_args(rest, "usage: :capabilities")?;
                Ok(Self::Capabilities)
            }
            "bindings" | "services" | "project" => {
                require_no_args(rest, "usage: :bindings")?;
                Ok(Self::Bindings)
            }
            "audit" => {
                require_no_args(rest, "usage: :audit")?;
                Ok(Self::Audit)
            }
            "history" => {
                require_no_args(rest, "usage: :history")?;
                Ok(Self::History)
            }
            "cancel" => {
                require_no_args(rest, "usage: :cancel")?;
                Ok(Self::Cancel)
            }
            "reset" => {
                require_no_args(rest, "usage: :reset")?;
                Ok(Self::ResetRepl)
            }
            "grant" => parse_grant(rest),
            "request" | "http" => parse_request(rest),
            "inspect" | "data" => parse_inspect(rest),
            "sandbox" => parse_sandbox(rest),
            "savepoint" => {
                validate_token("savepoint", rest)?;
                Ok(Self::Savepoint(rest.to_string()))
            }
            "commit" => {
                let mut fields = rest.split_whitespace();
                let keyword = fields.next();
                let transaction_id = fields.next();
                if keyword != Some("COMMIT")
                    || transaction_id.is_none()
                    || fields.next().is_some()
                {
                    return Err(ConsoleError::ConfirmationRequired(
                        "COMMIT <transaction-id>".to_string(),
                    ));
                }
                let transaction_id = transaction_id.expect("checked above");
                validate_token("transaction id", transaction_id)?;
                Ok(Self::Commit(format!("COMMIT {transaction_id}")))
            }
            _ => Err(ConsoleError::UnknownCommand(name.to_string())),
        }
    }
}
pub struct ConsoleSession {
    project: ConsoleProject,
    identity: ConsoleIdentity,
    repl: Session,
    mode: ConsoleMode,
    parent_authority: Authority,
    denied: Holds,
    grants: Vec<ConsoleGrant>,
    now_ms: u64,
    expires_at_ms: u64,
    loopback: bool,
    closed: bool,
    cancelled: bool,
    router: Option<Box<dyn InProcessRouter>>,
    data: Option<Box<dyn ConsoleDataBackend>>,
    /// Host-owned resource guards live with the session, so cancellation and
    /// expiry cannot release an application endpoint before the session drops.
    resources: Vec<Box<dyn Any>>,
    transaction: Option<Box<dyn ConsoleDataTransaction>>,
    transaction_database: Option<ConsoleDatabaseHandle>,
    transaction_id: Option<String>,
    transaction_sequence: u64,
    request_sequence: u64,
    active_request: Option<String>,
    output_sequence: u64,
    audit_sequence: u64,
    history: ConsoleHistory,
    audit: VecDeque<ConsoleAuditEvent>,
    outputs: VecDeque<ConsoleOutput>,
    logs: VecDeque<String>,
    max_outputs: usize,
    max_audit: usize,
}

impl ConsoleSession {
    pub fn open(options: ConsoleOptions) -> Result<Self, ConsoleError> {
        if options.release_build {
            return Err(ConsoleError::ReleaseUnavailable);
        }
        options.project.validate()?;
        for right in options.authority.rights() {
            validate_token("authority right", right)?;
        }
        for right in &options.denied {
            validate_token("denied right", right)?;
        }
        validate_token("session nonce", &options.session_nonce)?;
        let identity = ConsoleIdentity::new(
            &options.project.root,
            &options.project.identity.source_revision,
            &options.session_nonce,
        )?;
        let mut repl = Session::new();
        let mut load_note = Vec::new();
        super::load_project_items(&options.project.root, &mut repl, &mut load_note);
        repl.preserve_project_baseline();
        let expires_at_ms = if options.ttl_ms == 0 {
            0
        } else {
            options.now_ms.saturating_add(options.ttl_ms)
        };
        let grants = options
            .authority
            .rights()
            .map(|right| ConsoleGrant {
                capability: right.to_string(),
                required_right: right.to_string(),
                granted_at_ms: options.now_ms,
                expires_at_ms,
            })
            .collect();
        Ok(Self {
            project: options.project,
            identity,
            repl,
            mode: options.mode,
            parent_authority: options.authority,
            denied: options.denied,
            grants,
            now_ms: options.now_ms,
            expires_at_ms,
            loopback: options.loopback,
            closed: false,
            cancelled: false,
            router: None,
            data: None,
            resources: Vec::new(),
            transaction: None,
            transaction_database: None,
            transaction_id: None,
            transaction_sequence: 0,
            request_sequence: 0,
            active_request: None,
            output_sequence: 0,
            audit_sequence: 0,
            history: ConsoleHistory::new(options.max_history),
            audit: VecDeque::new(),
            outputs: VecDeque::new(),
            logs: VecDeque::new(),
            max_outputs: options.max_outputs.max(1),
            max_audit: options.max_audit.max(1),
        })
    }
    pub fn new(options: ConsoleOptions) -> Result<Self, ConsoleError> {
        Self::open(options)
    }

    pub fn run(options: ConsoleOptions) -> Result<Vec<ConsoleOutput>, ConsoleError> {
        let script = options.script.clone();
        let mut session = Self::open(options)?;
        if let Some(script) = script {
            session.run_script(&script)?;
        }
        if !session.closed && !session.cancelled {
            session.close()?;
        }
        Ok(session.outputs.iter().cloned().collect())
    }

    pub fn identity(&self) -> &ConsoleIdentity {
        &self.identity
    }

    pub fn project(&self) -> &ConsoleProject {
        &self.project
    }

    pub fn repl(&self) -> &Session {
        &self.repl
    }

    pub fn repl_mut(&mut self) -> &mut Session {
        &mut self.repl
    }

    pub fn mode(&self) -> ConsoleMode {
        self.mode
    }

    pub fn authority(&self) -> &Authority {
        &self.parent_authority
    }

    pub fn history(&self) -> &ConsoleHistory {
        &self.history
    }

    pub fn audit(&self) -> impl Iterator<Item = &ConsoleAuditEvent> {

        self.audit.iter()
    }

    pub fn outputs(&self) -> impl Iterator<Item = &ConsoleOutput> {
        self.outputs.iter()
    }

    pub fn logs(&self) -> impl Iterator<Item = &String> {
        self.logs.iter()
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    pub fn is_expired(&self) -> bool {
        self.expires_at_ms != 0 && self.now_ms >= self.expires_at_ms
    }

    pub fn set_now_ms(&mut self, now_ms: u64) {
        self.now_ms = self.now_ms.max(now_ms);
    }

    pub fn attach_router<R>(&mut self, router: R)
    where
        R: InProcessRouter + 'static,
    {
        self.router = Some(Box::new(router));
    }

    pub fn attach_data_backend<B>(&mut self, backend: B)
    where
        B: ConsoleDataBackend + 'static,
    {
        self.data = Some(Box::new(backend));
    }
    /// Retain a host-owned resource for this session without coupling the
    /// transport-neutral REPL to the host's concrete runtime type.
    pub fn attach_resource<R>(&mut self, resource: R)
    where
        R: Any + 'static,
    {
        self.resources.push(Box::new(resource));
    }

    pub fn set_router(&mut self, router: Box<dyn InProcessRouter>) {
        self.router = Some(router);
    }

    pub fn set_data_backend(&mut self, backend: Box<dyn ConsoleDataBackend>) {
        self.data = Some(backend);
    }

    pub fn grant(
        &mut self,
        capability: ConsoleCapability,
        ttl_ms: u64,
    ) -> Result<ConsoleOutput, ConsoleError> {
        self.ensure_active("grant")?;
        let right = capability.required_right();
        if capability == ConsoleCapability::DbWrite || capability == ConsoleCapability::Commit {
            if self.mode != ConsoleMode::SandboxData {
                return self.fail(
                    "grant",
                    ConsoleError::Denied {
                        required: right.to_string(),
                        reason: "writable data authority requires `--sandbox data`".to_string(),
                    },
                );
            }
        }
        let verdict = answer(self.parent_authority.holds(), &self.denied, right);
        if verdict != Verdict::Allowed {
            let reason = match verdict {
                Verdict::Denied => "an explicit deny overrides the requested grant",
                Verdict::Missing => "the parent authority does not hold the requested right",
                Verdict::Allowed => unreachable!(),
            };
            return self.fail(
                "grant",
                ConsoleError::Denied {
                    required: right.to_string(),
                    reason: reason.to_string(),
                },
            );
        }
        let expires_at_ms = if ttl_ms == 0 {
            self.expires_at_ms
        } else {

            let requested = self.now_ms.saturating_add(ttl_ms);
            if self.expires_at_ms == 0 {
                requested
            } else {
                requested.min(self.expires_at_ms)
            }
        };
        let grant = ConsoleGrant {
            capability: capability.as_str().to_string(),
            required_right: right.to_string(),
            granted_at_ms: self.now_ms,
            expires_at_ms,
        };
        let expired_existing = self
            .grants
            .iter()
            .any(|existing| existing.required_right == right && !existing.active_at(self.now_ms));
        if expired_existing {
            return self.fail(
                "grant",
                ConsoleError::Denied {
                    required: right.to_string(),
                    reason: "the capability grant expired; restart the console to request it again"
                        .to_string(),
                },
            );
        }
        self.grants.retain(|existing| existing.required_right != right);
        self.grants.push(grant.clone());
        self.grants.sort_by(|left, right| left.capability.cmp(&right.capability));
        self.record_audit(ConsoleAuditEvent::new(
            &self.identity.session_id,
            None,
            "grant",
            Some(right.to_string()),
            ConsoleAuditOutcome::Allowed,
            "explicit capability grant",
            None,
            None,
        ));
        Ok(self.output(
            ConsoleOutputKind::Grant,
            true,
            format!("authority: {} until {}", right, expires_at_ms),
            grant.to_json(true),
            None,
            None,
            false,
        ))
    }

    pub fn capabilities(&self) -> String {
        let entries = self
            .grants
            .iter()
            .map(|grant| {
                let active = grant.active_at(self.now_ms)
                    && answer(
                        self.parent_authority.holds(),
                        &self.denied,
                        &grant.required_right,
                    ) == Verdict::Allowed;
                grant.to_json(active)
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"mode\":{},\"expires_at_ms\":{},\"grants\":[{}]}}",
            quote(self.mode.as_str()),
            self.expires_at_ms,
            entries
        )
    }
    pub fn bindings(&mut self) -> Result<ConsoleOutput, ConsoleError> {
        self.ensure_active("bindings")?;
        self.require_capability(ConsoleCapability::ServiceRead, "bindings")?;
        Ok(self.output(
            ConsoleOutputKind::Bindings,
            true,
            "project bindings",
            self.project.bindings_json(),
            None,
            None,
            false,
        ))
    }

    /// Effective authority is derived from unexpired grants on every check.
    /// The parent authority remains immutable for the lifetime of the session.
    pub fn effective_authority(&self) -> Authority {
        let holds = self
            .grants
            .iter()
            .filter(|grant| {
                grant.active_at(self.now_ms)
                    && answer(
                        self.parent_authority.holds(),
                        &self.denied,
                        &grant.required_right,
                    ) == Verdict::Allowed
            })
            .map(|grant| grant.required_right.clone())
            .collect();
        Authority::from_holds(holds)
    }

    pub fn request(
        &mut self,
        method: impl Into<String>,
        path: impl Into<String>,
    ) -> Result<ConsoleOutput, ConsoleError> {
        self.request_with_body(method, path, Vec::new())
    }

    pub fn request_with_body(
        &mut self,
        method: impl Into<String>,
        path: impl Into<String>,
        body: Vec<u8>,
    ) -> Result<ConsoleOutput, ConsoleError> {
        self.ensure_active("request")?;
        self.require_capability(ConsoleCapability::HttpRequest, "request")?;
        let method = method.into();
        let path = path.into();
        self.request_sequence = self.request_sequence.saturating_add(1);
        let request_id = self.next_request_id(self.request_sequence, &method, &path);
        let request = ConsoleRequest::new(
            request_id,
            self.identity.session_id.clone(),
            method,
            path,
        )?
        .with_body(body)?;
        self.dispatch_request(request)
    }

    pub fn dispatch_request(
        &mut self,
        request: ConsoleRequest,
    ) -> Result<ConsoleOutput, ConsoleError> {
        self.ensure_active("request")?;
        self.require_capability(ConsoleCapability::HttpRequest, "request")?;
        if let Err(error) = request.validate() {
            return self.fail("request", error);
        }
        if request.session_id != self.identity.session_id {
            return self.fail(
                "request",
                ConsoleError::InvalidInput("request belongs to another console session".to_string()),
            );
        }
        if !self.loopback || !request.is_loopback() {
            self.record_audit(ConsoleAuditEvent::new(
                &self.identity.session_id,
                Some(request.request_id.clone()),
                "request",
                Some(ConsoleCapability::HttpRequest.required_right().to_string()),
                ConsoleAuditOutcome::Denied,
                "non-loopback request rejected before dispatch",
                Some(&request.path),
                None,
            ));
            return self.fail(
                "request",
                ConsoleError::NonLoopbackDenied,
            );
        }
        self.record_audit(ConsoleAuditEvent::new(
            &self.identity.session_id,
            Some(request.request_id.clone()),
            "request",
            Some(ConsoleCapability::HttpRequest.required_right().to_string()),
            ConsoleAuditOutcome::Allowed,
            "in-process route dispatch",
            Some(&request.path),
            None,
        ));
        if self.router.is_none() {
            return self.fail("request", ConsoleError::RouterUnavailable);
        }
        self.active_request = Some(request.request_id.clone());
        let response = {
            let router = self
                .router
                .as_mut()
                .expect("router checked immediately before dispatch");
            router.dispatch(request.clone())
        };
        self.active_request = None;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                self.record_audit(ConsoleAuditEvent::new(
                    &self.identity.session_id,
                    Some(request.request_id.clone()),
                    "request",
                    Some(ConsoleCapability::HttpRequest.required_right().to_string()),
                    ConsoleAuditOutcome::Failed,
                    audit_error_reason(&error),
                    Some(&request.path),
                    None,
                ));
                return self.fail("request", error);
            }
        };
        if let Err(error) = response.validate() {
            return self.fail("request", error);
        }
        if response.request_id != request.request_id {
            return self.fail(
                "request",
                ConsoleError::InvalidInput(
                    "router response request id does not match request".to_string(),
                ),
            );
        }
        let (body, body_truncated) = bounded_bytes(response.body, CONSOLE_MAX_RESULT_BYTES);
        let headers_truncated = response.headers.len() > CONSOLE_MAX_HEADERS;
        self.record_audit(ConsoleAuditEvent::new(
            &self.identity.session_id,
            Some(request.request_id.clone()),
            "request",
            Some(ConsoleCapability::HttpRequest.required_right().to_string()),
            ConsoleAuditOutcome::Completed,
            format!("route returned status {}", response.status),
            Some(&request.path),
            None,
        ));
        let body_text = String::from_utf8_lossy(&body).into_owned();
        let headers = response
            .headers
            .iter()
            .take(CONSOLE_MAX_HEADERS)
            .map(|(name, value)| {
                let value = if is_secret_header(name) {
                    "<redacted>".to_string()
                } else {
                    bounded(value.clone(), 1024)
                };
                format!("{name}: {value}")
            })
            .collect::<Vec<_>>()
            .join("\\n");
        let payload = format!(
            "{{\"status\":{},\"headers\":{},\"body\":{}}}",
            response.status,
            quote(&headers),
            quote(&body_text),
        );
        Ok(self.output(
            ConsoleOutputKind::Request,

            true,
            format!("{} {} -> {}", request.method, request.path, response.status),
            payload,
            Some(request.request_id),
            None,
            body_truncated || headers_truncated,
        ))
    }
    /// Cancel a request currently owned by this session.  The host adapter
    /// receives the same request identity that was handed to `dispatch`.
    pub fn cancel_request(&mut self, request_id: &str) -> Result<(), ConsoleError> {
        self.ensure_active("request.cancel")?;
        validate_token("request id", request_id)?;
        if self.active_request.as_deref() != Some(request_id) {
            return self.fail(
                "request.cancel",
                ConsoleError::InvalidRequest("request is not active".to_string()),
            );
        }
        if let Some(router) = self.router.as_mut() {
            router.cancel(request_id);
        }
        self.active_request = None;
        self.record_audit(ConsoleAuditEvent::new(
            &self.identity.session_id,
            Some(request_id.to_string()),
            "request.cancel",
            Some(ConsoleCapability::HttpRequest.required_right().to_string()),
            ConsoleAuditOutcome::Cancelled,
            "request cancellation forwarded to the in-process router",
            None,
            self.transaction_id.clone(),
        ));
        Ok(())
    }

    pub fn inspect_data(
        &mut self,
        request: ConsoleDataRequest,
    ) -> Result<ConsoleOutput, ConsoleError> {
        self.ensure_active("data.inspect")?;
        self.require_capability(ConsoleCapability::DbRead, "data.inspect")?;
        if let Err(error) = request.validate() {
            return self.fail("data.inspect", error);
        }
        let Some(database) = self
            .project
            .databases
            .iter()
            .find(|database| database.name == request.database)
            .cloned()
        else {
            return self.fail(
                "data.inspect",
                ConsoleError::InvalidInput(format!(
                    "database `{}` is not declared by the project",
                    request.database
                )),
            );
        };
        let resolved = match self.data.as_mut() {
            Some(data) => match data.resolve_inspect(&database, &request) {
                Ok(request) => request,
                Err(error) => return self.fail("data.inspect", error),
            },
            None => return self.fail("data.inspect", ConsoleError::DataBackendUnavailable),
        };
        if let Err(error) = resolved.validate() {
            return self.fail("data.inspect", error);
        }
        if resolved.database != database.name
            || resolved.statement_identity != request.statement_identity
            || resolved.operation != request.operation
        {
            return self.fail(
                "data.inspect",
                ConsoleError::InvalidRequest(
                    "resolved data request changed its checked identity".to_string(),
                ),
            );
        }
        let result = match self.data.as_mut() {
            Some(data) => match data.inspect(&resolved) {
                Ok(result) => result,
                Err(error) => return self.fail("data.inspect", error),
            },
            None => return self.fail("data.inspect", ConsoleError::DataBackendUnavailable),
        };
        if let Err(error) = validate_data_result(&result) {
            return self.fail("data.inspect", error);
        }
        self.record_audit(ConsoleAuditEvent::new(
            &self.identity.session_id,
            None,
            "data.inspect",
            Some(ConsoleCapability::DbRead.required_right().to_string()),
            ConsoleAuditOutcome::Completed,
            "typed data inspection",
            Some(&resolved.statement_identity),
            self.transaction_id.clone(),
        ));
        let payload = result.to_json();
        Ok(self.output(
            ConsoleOutputKind::Data,
            true,
            format!("{} row(s)", result.rows.len()),
            payload,
            None,
            self.transaction_id.clone(),
            result.truncated || result.rows.len() > CONSOLE_MAX_ROWS,
        ))
    }

    pub fn begin_sandbox(&mut self) -> Result<ConsoleOutput, ConsoleError> {
        self.ensure_active("sandbox.begin")?;
        if self.project.databases.len() != 1 {
            return self.fail(
                "sandbox.begin",
                ConsoleError::InvalidInput(
                    "sandbox.begin requires exactly one declared database; use begin_sandbox_for for an explicit handle"
                        .to_string(),
                ),
            );
        }
        let database = self.project.databases[0].name.clone();
        self.begin_sandbox_for(&database)
    }

    pub fn begin_sandbox_for(
        &mut self,
        database_name: &str,
    ) -> Result<ConsoleOutput, ConsoleError> {
        self.ensure_active("sandbox.begin")?;
        validate_token("database", database_name)?;
        if self.mode != ConsoleMode::SandboxData {
            return self.fail(
                "sandbox.begin",
                ConsoleError::Denied {
                    required: ConsoleCapability::DbWrite.required_right().to_string(),
                    reason: "open the console with `--sandbox data` before requesting writes"
                        .to_string(),
                },
            );
        }
        self.require_capability(ConsoleCapability::DbWrite, "sandbox.begin")?;
        if self.transaction.is_some() {
            return self.fail("sandbox.begin", ConsoleError::TransactionActive);
        }
        let Some(database) = self
            .project
            .databases
            .iter()
            .find(|database| database.name == database_name)
            .cloned()
        else {
            return self.fail(
                "sandbox.begin",
                ConsoleError::InvalidInput(format!(
                    "database `{database_name}` is not declared by the project"
                )),
            );
        };
        self.transaction_sequence = self.transaction_sequence.saturating_add(1);
        let seed = format!(
            "{}\0transaction\0{}",
            self.identity.session_id, self.transaction_sequence
        );
        let transaction_id = format!("tx-{}", &digest(&seed)[..24]);
        let transaction_result = match self.data.as_mut() {
            Some(data) => data.begin(&database, &transaction_id),
            None => return self.fail("sandbox.begin", ConsoleError::DataBackendUnavailable),
        };
        let mut transaction = match transaction_result {
            Ok(transaction) => transaction,
            Err(error) => return self.fail("sandbox.begin", error),
        };
        if let Err(error) = transaction.savepoint("console-root") {
            let _ = transaction.rollback();
            return self.fail("sandbox.begin", error);
        }
        self.transaction = Some(transaction);
        self.transaction_id = Some(transaction_id.clone());
        self.transaction_database = Some(database);
        self.record_audit(ConsoleAuditEvent::new(
            &self.identity.session_id,
            None,
            "sandbox.begin",
            Some(ConsoleCapability::DbWrite.required_right().to_string()),
            ConsoleAuditOutcome::Allowed,
            "real transaction opened with console-root savepoint",
            None,
            Some(transaction_id.clone()),
        ));
        Ok(self.output(
            ConsoleOutputKind::Sandbox,
            true,
            format!("sandbox transaction {transaction_id} opened; writes roll back on exit"),
            format!(
                "{{\"transaction_id\":{},\"savepoint\":{}}}",
                quote(&transaction_id),
                quote("console-root")
            ),
            None,
            Some(transaction_id),
            false,
        ))
    }

    pub fn savepoint(&mut self, name: impl Into<String>) -> Result<ConsoleOutput, ConsoleError> {
        self.ensure_active("sandbox.savepoint")?;
        self.require_capability(ConsoleCapability::DbWrite, "sandbox.savepoint")?;
        let name = name.into();
        validate_token("savepoint", &name)?;
        if name == "console-root" {
            return self.fail(
                "sandbox.savepoint",
                ConsoleError::InvalidInput("savepoint name `console-root` is reserved".to_string()),
            );
        }
        let result = {
            let Some(transaction) = self.transaction.as_mut() else {
                return self.fail("sandbox.savepoint", ConsoleError::NoTransaction);
            };
            transaction.savepoint(&name)
        };
        if let Err(error) = result {
            return self.fail("sandbox.savepoint", error);
        }
        let transaction_id = self.transaction_id.clone();
        self.record_audit(ConsoleAuditEvent::new(
            &self.identity.session_id,
            None,
            "sandbox.savepoint",
            Some(ConsoleCapability::DbWrite.required_right().to_string()),
            ConsoleAuditOutcome::Allowed,
            "real database savepoint created",
            Some(&name),
            transaction_id.clone(),
        ));
        Ok(self.output(
            ConsoleOutputKind::Sandbox,
            true,
            format!("savepoint {name} created"),
            format!("{{\"savepoint\":{}}}", quote(&name)),
            None,
            transaction_id,
            false,
        ))
    }

    pub fn reset_sandbox(&mut self) -> Result<ConsoleOutput, ConsoleError> {
        self.ensure_active("sandbox.reset")?;
        self.require_capability(ConsoleCapability::DbWrite, "sandbox.reset")?;
        let result = {
            let Some(transaction) = self.transaction.as_mut() else {
                return self.fail("sandbox.reset", ConsoleError::NoTransaction);
            };
            transaction.rollback_to("console-root")
        };
        if let Err(error) = result {
            return self.fail("sandbox.reset", error);
        }
        let transaction_id = self.transaction_id.clone();
        self.record_audit(ConsoleAuditEvent::new(
            &self.identity.session_id,
            None,
            "sandbox.reset",
            Some(ConsoleCapability::DbWrite.required_right().to_string()),
            ConsoleAuditOutcome::RolledBack,
            "real transaction rolled back to console-root",
            None,
            transaction_id.clone(),
        ));
        Ok(self.output(
            ConsoleOutputKind::Rollback,
            true,
            "sandbox reset to console-root",
            "{\"savepoint\":\"console-root\",\"values\":\"redacted\"}".to_string(),
            None,
            transaction_id,
            false,
        ))
    }

    pub fn execute_data(
        &mut self,
        statement: ConsoleDataStatement,
    ) -> Result<ConsoleOutput, ConsoleError> {
        self.ensure_active("sandbox.execute")?;
        self.require_capability(ConsoleCapability::DbWrite, "sandbox.execute")?;
        if let Err(error) = statement.validate() {
            return self.fail("sandbox.execute", error);
        }
        let Some(transaction_id) = self.transaction_id.clone() else {
            return self.fail("sandbox.execute", ConsoleError::NoTransaction);
        };
        let Some(transaction_database) = self.transaction_database.clone() else {
            return self.fail(
                "sandbox.execute",
                ConsoleError::InvalidRequest("transaction has no declared database".to_string()),
            );
        };
        if statement.database != transaction_database.name {
            return self.fail(
                "sandbox.execute",
                ConsoleError::InvalidRequest(
                    "data statement database does not match the active transaction".to_string(),
                ),
            );
        }
        let resolved = match self.data.as_mut() {
            Some(data) => match data.resolve_statement(&transaction_database, &statement) {
                Ok(statement) => statement,
                Err(error) => return self.fail("sandbox.execute", error),
            },
            None => return self.fail("sandbox.execute", ConsoleError::DataBackendUnavailable),
        };
        if resolved.database != transaction_database.name
            || resolved.statement_identity != statement.statement_identity
            || resolved.operation != statement.operation
        {
            return self.fail(
                "sandbox.execute",
                ConsoleError::InvalidRequest(
                    "resolved data statement changed its checked identity".to_string(),
                ),
            );
        }
        let mutation = {
            let Some(transaction) = self.transaction.as_mut() else {
                return self.fail("sandbox.execute", ConsoleError::NoTransaction);
            };
            match transaction.execute(&resolved) {
                Ok(mutation) => mutation,
                Err(error) => return self.fail("sandbox.execute", error),
            }
        };
        if let Err(error) = mutation.validate() {
            return self.fail("sandbox.execute", error);
        }
        if mutation.transaction_id != transaction_id {
            return self.fail(
                "sandbox.execute",
                ConsoleError::InvalidRequest(
                    "data backend returned a mutation for another transaction".to_string(),
                ),
            );
        }
        if mutation.statement_identity != resolved.statement_identity {
            return self.fail(
                "sandbox.execute",
                ConsoleError::InvalidRequest(
                    "data backend returned a mutation for another statement".to_string(),
                ),
            );
        }
        self.record_audit(ConsoleAuditEvent::new(
            &self.identity.session_id,
            None,
            "sandbox.execute",
            Some(ConsoleCapability::DbWrite.required_right().to_string()),
            ConsoleAuditOutcome::Completed,
            format!("{} row(s) affected", mutation.rows_affected),
            Some(&resolved.statement_identity),
            Some(transaction_id.clone()),
        ));
        Ok(self.output(
            ConsoleOutputKind::Sandbox,
            true,
            format!("{} row(s) affected", mutation.rows_affected),
            format!(
                "{{\"transaction_id\":{},\"statement_identity\":{},\"rows_affected\":{}}}",
                quote(&mutation.transaction_id),
                quote(&mutation.statement_identity),
                mutation.rows_affected
            ),
            None,
            Some(transaction_id.clone()),
            false,
        ))
    }

    pub fn commit(&mut self, confirmation: impl AsRef<str>) -> Result<ConsoleOutput, ConsoleError> {
        self.ensure_active("sandbox.commit")?;
        self.require_capability(ConsoleCapability::Commit, "sandbox.commit")?;
        let Some(transaction_id) = self.transaction_id.clone() else {
            return self.fail("sandbox.commit", ConsoleError::NoTransaction);
        };
        let expected = format!("COMMIT {transaction_id}");
        if confirmation.as_ref() != expected {
            self.record_audit(ConsoleAuditEvent::new(
                &self.identity.session_id,
                None,
                "sandbox.commit",
                Some(ConsoleCapability::Commit.required_right().to_string()),
                ConsoleAuditOutcome::Denied,
                "explicit commit confirmation did not match",
                None,
                Some(transaction_id.clone()),
            ));
            return self.fail(
                "sandbox.commit",
                ConsoleError::ConfirmationRequired(expected),
            );
        }
        let result = {
            let Some(transaction) = self.transaction.as_mut() else {
                return self.fail("sandbox.commit", ConsoleError::NoTransaction);
            };
            transaction.commit()
        };
        if let Err(error) = result {
            return self.fail("sandbox.commit", error);
        }
        self.transaction.take();
        self.transaction_database = None;
        self.transaction_id = None;
        self.record_audit(ConsoleAuditEvent::new(
            &self.identity.session_id,
            None,
            "sandbox.commit",
            Some(ConsoleCapability::Commit.required_right().to_string()),
            ConsoleAuditOutcome::Committed,
            "explicit commit confirmation accepted",
            None,
            Some(transaction_id.clone()),
        ));
        Ok(self.output(
            ConsoleOutputKind::Commit,
            true,
            format!("transaction {transaction_id} committed"),
            format!(
                "{{\"transaction_id\":{},\"confirmation\":true}}",
                quote(&transaction_id)
            ),
            None,
            Some(transaction_id),
            false,
        ))
    }

    pub fn cancel(&mut self) -> Result<ConsoleOutput, ConsoleError> {
        if self.closed {
            return self.fail("cancel", ConsoleError::Closed);
        }
        if self.cancelled {
            return self.fail("cancel", ConsoleError::Cancelled);
        }
        if let Some(request_id) = self.active_request.take() {
            if let Some(router) = self.router.as_mut() {
                router.cancel(&request_id);
            }
            self.record_audit(ConsoleAuditEvent::new(
                &self.identity.session_id,
                Some(request_id),
                "request.cancel",
                Some(ConsoleCapability::HttpRequest.required_right().to_string()),
                ConsoleAuditOutcome::Cancelled,
                "active request cancelled with the console",
                None,
                self.transaction_id.clone(),
            ));
        }
        let rollback_result = if let Some(transaction) = self.transaction.as_mut() {
            Some(transaction.rollback())
        } else {
            None
        };
        if let Some(Err(error)) = rollback_result {
            return self.fail("cancel", error);
        }
        self.transaction.take();
        self.transaction_database = None;
        self.cancelled = true;
        let transaction_id = self.transaction_id.take();
        self.record_audit(ConsoleAuditEvent::new(
            &self.identity.session_id,
            None,
            "cancel",
            None,
            ConsoleAuditOutcome::Cancelled,
            "console cancelled; open transaction rolled back",
            None,
            transaction_id.clone(),
        ));
        Ok(self.output(
            ConsoleOutputKind::Cancelled,
            true,
            "console cancelled",
            "{\"cancelled\":true}".to_string(),
            None,
            transaction_id,
            false,
        ))
    }

    pub fn close(&mut self) -> Result<ConsoleOutput, ConsoleError> {
        if self.closed {
            return Ok(self.output(
                ConsoleOutputKind::Goodbye,
                true,
                "console already closed",
                "{\"closed\":true}".to_string(),
                None,
                None,
                false,
            ));
        }
        if let Some(request_id) = self.active_request.take() {
            if let Some(router) = self.router.as_mut() {
                router.cancel(&request_id);
            }
            self.record_audit(ConsoleAuditEvent::new(
                &self.identity.session_id,
                Some(request_id),
                "request.cancel",
                Some(ConsoleCapability::HttpRequest.required_right().to_string()),
                ConsoleAuditOutcome::Cancelled,
                "active request cancelled with the console",
                None,
                self.transaction_id.clone(),
            ));
        }
        let transaction_id = self.transaction_id.clone();
        let rollback_result = if let Some(transaction) = self.transaction.as_mut() {
            Some(transaction.rollback())
        } else {
            None
        };
        if let Some(Err(error)) = rollback_result {
            self.record_audit(ConsoleAuditEvent::new(
                &self.identity.session_id,
                None,
                "close",
                Some(ConsoleCapability::DbWrite.required_right().to_string()),
                ConsoleAuditOutcome::Failed,
                audit_error_reason(&error),
                None,
                transaction_id,
            ));
            return self.fail("close", error);
        }
        if self.transaction.is_some() {
            self.transaction.take();
            self.record_audit(ConsoleAuditEvent::new(
                &self.identity.session_id,
                None,
                "close",
                Some(ConsoleCapability::DbWrite.required_right().to_string()),
                ConsoleAuditOutcome::RolledBack,
                "open sandbox transaction rolled back on exit",
                None,
                transaction_id,
            ));
            self.transaction_id = None;
            self.transaction_database = None;
        }
        self.closed = true;
        self.record_audit(ConsoleAuditEvent::new(
            &self.identity.session_id,
            None,
            "close",
            None,
            ConsoleAuditOutcome::Completed,
            "console session closed",
            None,
            None,
        ));
        Ok(self.output(
            ConsoleOutputKind::Goodbye,
            true,
            "console closed; uncommitted data rolled back",
            "{\"closed\":true,\"uncommitted\":\"rolled-back\"}".to_string(),
            None,
            None,
            false,
        ))
    }

    pub fn evaluate(&mut self, input: &str) -> Result<ConsoleOutput, ConsoleError> {
        self.ensure_active("evaluate")?;
        self.require_capability(ConsoleCapability::Repl, "evaluate")?;
        if input.len() > CONSOLE_MAX_TEXT_BYTES {
            return self.fail("evaluate", ConsoleError::LimitExceeded("command"));
        }
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(self.output(
                ConsoleOutputKind::Evaluation,
                true,
                "ok",
                "null".to_string(),
                None,
                None,
                false,
            ));
        }
        self.repl.step = self.repl.step.saturating_add(1);
        let kind = match super::classify(trimmed, self.repl.step) {
            Ok(kind) => kind,
            Err(diags) => return self.evaluation_diagnostics(trimmed, &diags),
        };
        match kind {
            super::InputKind::Empty => Ok(self.output(
                ConsoleOutputKind::Evaluation,
                true,
                "ok",
                "null".to_string(),
                None,
                None,
                false,
            )),
            super::InputKind::Meta(command, _) => {
                let diagnostic = super::e1802(&format!("meta-command `:{command}`"));
                self.evaluation_diagnostic(trimmed, &diagnostic)
            }
            super::InputKind::Reject(feature) => {
                let diagnostic = super::e1802(&feature);
                self.evaluation_diagnostic(trimmed, &diagnostic)
            }
            super::InputKind::Item(src) => self.evaluate_item(trimmed, &src),
            super::InputKind::Import(src) => self.evaluate_import(trimmed, &src),
            super::InputKind::Stmts(stmts, suppress, check_src) => {
                self.evaluate_stmts(trimmed, stmts, suppress, &check_src)
            }
        }
    }

    pub fn run_line(&mut self, input: &str) -> Result<ConsoleOutput, ConsoleError> {
        let command = ConsoleCommand::parse(input)?;
        let output = match command {
            ConsoleCommand::Eval(source) => self.evaluate(&source),
            ConsoleCommand::Request { method, path, body } => {
                self.request_with_body(method, path, body)
            }
            ConsoleCommand::Inspect(request) => self.inspect_data(request),
            ConsoleCommand::BeginSandbox => self.begin_sandbox(),
            ConsoleCommand::Savepoint(name) => self.savepoint(name),
            ConsoleCommand::ResetSandbox => self.reset_sandbox(),
            ConsoleCommand::Execute(statement) => self.execute_data(statement),
            ConsoleCommand::Commit(confirmation) => self.commit(confirmation),
            ConsoleCommand::ResetRepl => {
                self.ensure_active("reset")?;
                self.repl.reset();
                Ok(self.output(
                    ConsoleOutputKind::Evaluation,
                    true,
                    "REPL session reset",
                    "{\"reset\":true}".to_string(),
                    None,
                    None,
                    false,
                ))
            }
            ConsoleCommand::Grant {
                capability,
                ttl_ms,
            } => self.grant(capability, ttl_ms),
            ConsoleCommand::Capabilities => {
                self.ensure_active("capabilities")?;
                Ok(self.output(
                    ConsoleOutputKind::Capabilities,
                    true,
                    "console authority",
                    self.capabilities(),
                    None,
                    None,
                    false,
                ))
            }
            ConsoleCommand::Bindings => self.bindings(),
            ConsoleCommand::Audit => {
                self.ensure_active("audit")?;
                let payload = format!(
                    "[{}]",
                    self.audit
                        .iter()
                        .map(ConsoleAuditEvent::to_json)
                        .collect::<Vec<_>>()
                        .join(",")
                );
                Ok(self.output(
                    ConsoleOutputKind::Audit,
                    true,
                    "audit facts",
                    payload,
                    None,
                    None,
                    false,
                ))
            }
            ConsoleCommand::History => {
                self.ensure_active("history")?;
                Ok(self.output(
                    ConsoleOutputKind::History,
                    true,
                    "console history",
                    self.history.to_json(),
                    None,
                    None,
                    false,
                ))
            }
            ConsoleCommand::Cancel => self.cancel(),
            ConsoleCommand::Quit => self.close(),
            ConsoleCommand::Help => {
                self.ensure_active("help")?;
                Ok(self.output(
                    ConsoleOutputKind::Startup,
                    true,
                    "console commands",
                    quote(":request GET /path; :inspect DB statement 50; :sandbox data; :grant DB.Read; :quit"),
                    None,
                    None,
                    false,
                ))
            }
        };
        match output {
            Ok(output) => {
                self.history.record(input, if output.ok { "ok" } else { "error" }, &output.to_json_line());
                Ok(output)
            }
            Err(error) => {
                self.history.record(input, "error", &error.to_string());
                Err(error)
            }
        }
    }

    pub fn run_script(&mut self, script: &str) -> Result<(), ConsoleError> {
        for line in script.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let output = self.run_line(line)?;
            if matches!(output.kind, ConsoleOutputKind::Goodbye | ConsoleOutputKind::Cancelled) {
                break;
            }
        }
        Ok(())
    }
    pub fn run_reader<R: BufRead>(&mut self, reader: &mut R) -> Result<(), ConsoleError> {
        let mut line = String::new();
        loop {
            line.clear();
            let bytes = reader
                .read_line(&mut line)
                .map_err(|error| ConsoleError::InvalidInput(format!("console input read failed: {error}")))?;
            if bytes == 0 {
                break;
            }
            let output = self.run_line(line.trim_end_matches(['\r', '\n']))?;
            if matches!(output.kind, ConsoleOutputKind::Goodbye | ConsoleOutputKind::Cancelled) {
                break;
            }
        }
        Ok(())
    }


    fn evaluate_item(&mut self, input: &str, src: &str) -> Result<ConsoleOutput, ConsoleError> {
        let bundle = match type_check_item(&self.repl, src) {
            Ok(bundle) => bundle,
            Err(diags) => return self.evaluation_diagnostics(input, &diags),
        };
        self.repl.item_srcs.push(src.to_string());
        rebuild_funcs(&mut self.repl);
        update_core_imports_from_ledger(&bundle, &mut self.repl.core_imports);
        self.repl.record_turn_ex(
            input,
            super::ReplTurnStatus::Ok,
            "ok".to_string(),
            false,
            None,
        );
        self.repl.remember_success(input);
        Ok(self.output(
            ConsoleOutputKind::Evaluation,
            true,
            "ok",
            "{\"accepted\":true,\"kind\":\"item\"}".to_string(),
            None,
            None,
            false,
        ))
    }

    fn evaluate_import(&mut self, input: &str, src: &str) -> Result<ConsoleOutput, ConsoleError> {
        self.repl.import_srcs.push(src.to_string());
        let bundle = match type_check_item(&self.repl, "") {
            Ok(bundle) => bundle,
            Err(diags) => {
                self.repl.import_srcs.pop();
                return self.evaluation_diagnostics(input, &diags);
            }
        };
        rebuild_funcs(&mut self.repl);
        update_core_imports_from_ledger(&bundle, &mut self.repl.core_imports);
        self.repl.record_turn_ex(
            input,
            super::ReplTurnStatus::Ok,
            "ok".to_string(),
            false,
            None,
        );
        self.repl.remember_success(input);
        Ok(self.output(
            ConsoleOutputKind::Evaluation,
            true,
            "ok",
            "{\"accepted\":true,\"kind\":\"import\"}".to_string(),
            None,
            None,
            false,
        ))
    }

    fn evaluate_stmts(
        &mut self,
        input: &str,
        stmts: Vec<Stmt>,
        suppress: bool,
        check_src: &str,
    ) -> Result<ConsoleOutput, ConsoleError> {
        let (checked_stmts, checked_core_imports) =
            match type_check_stmts(&self.repl, &stmts, self.repl.step, check_src) {
                Ok(checked) => checked,
                Err(diags) => return self.evaluation_diagnostics(input, &diags),
            };
        let session_binding_names: std::collections::HashSet<String> =
            self.repl.scope.keys().cloned().collect();
        let newly_moved = collect_moved_names(&stmts, &session_binding_names, &self.repl.scope);
        let executable = repl_executable_stmts(checked_stmts.clone());
        let mut core_imports = self.repl.core_imports.clone();
        core_imports.extend(checked_core_imports);
        let before_keys: std::collections::HashSet<String> = self.repl.scope.keys().cloned().collect();
        let funcs: std::collections::HashMap<String, &Func> = self
            .repl
            .func_defs
            .iter()
            .map(|(name, func)| (name.clone(), func))
            .collect();
        let structs: std::collections::HashMap<String, &StructDef> = self
            .repl
            .struct_defs
            .iter()
            .map(|(name, structure)| (name.clone(), structure))
            .collect();
        let authority = self.effective_authority();
        let denied = self.denied.clone();
        let flags = policy_flags(&authority, &denied);
        let mut policy = ReplPolicy::new(flags, &self.project.root);
        let mut inner = policy.authorizer(None);
        let mut effect_audit = Vec::new();
        let mut authorizer = ConsoleReplAuthorization {
            inner: &mut inner,
            authority: &authority,
            denied: &denied,
            mode: self.mode,
            session_id: &self.identity.session_id,
            events: &mut effect_audit,
        };
        let mut sink = DevSink::new();
        let mut trial_scope = self.repl.scope.clone();
        let result = crate::Comptime::run_repl_step(
            &executable,
            &funcs,
            &self.project.root,
            &mut sink,
            &mut trial_scope,
            REPL_FUEL_BUDGET,
            suppress,
            &core_imports,
            &structs,
            &self.repl.binding_types,
            &mut authorizer,
        );
        drop(authorizer);
        drop(inner);
        drop(policy);
        for event in effect_audit {
            self.record_audit(event);
        }
        match result {
            Ok(echo) => {
                for name in &newly_moved {
                    trial_scope.remove(name);
                }
                self.repl.scope = trial_scope;
                self.repl.moved_names.extend(newly_moved);
                let raw = input.trim_end_matches(';').trim();
                if !raw.is_empty() && !raw.starts_with("__repl_echo__") {
                    self.repl.stmt_srcs.push(format!("{};", raw));
                }
                let new_names: Vec<String> = self
                    .repl
                    .scope
                    .keys()
                    .filter(|name| !before_keys.contains(*name) && *name != "__repl_echo__")
                    .cloned()
                    .collect();
                self.repl.record_stmts(&checked_stmts);
                let mut summary = String::new();
                if !sink.stdout.is_empty() {
                    summary.push_str(&sink.stdout);
                }
                if !sink.stderr.is_empty() {
                    summary.push_str(&sink.stderr);
                }
                let value_text = echo
                    .as_ref()
                    .filter(|value| !matches!(value, CtValue::Unit))
                    .map(display_value);
                if let Some(value) = &value_text {
                    summary.push_str(value);
                }
                let had_effect = super::looks_effectful(input)
                    || !sink.stdout.is_empty()
                    || !sink.stderr.is_empty();
                let bound_name = match new_names.as_slice() {
                    [only] => Some(only.clone()),
                    _ => None,
                };
                self.repl.record_turn_ex(
                    input,
                    super::ReplTurnStatus::Ok,
                    summary.clone(),
                    had_effect,
                    bound_name,
                );
                self.repl.remember_success(input);
                let stdout = bounded(sink.stdout, CONSOLE_MAX_RESULT_BYTES);
                let stderr = bounded(sink.stderr, CONSOLE_MAX_RESULT_BYTES);
                let value_json = value_text
                    .as_ref()
                    .map(|value| quote(value))
                    .unwrap_or_else(|| "null".to_string());
                let payload = format!(
                    "{{\"stdout\":{},\"stderr\":{},\"value\":{}}}",
                    quote(&stdout),
                    quote(&stderr),
                    value_json,
                );
                Ok(self.output(
                    ConsoleOutputKind::Evaluation,
                    true,
                    if summary.is_empty() { "ok".to_string() } else { summary },
                    payload,
                    None,
                    None,
                    stdout.len() >= CONSOLE_MAX_RESULT_BYTES
                        || stderr.len() >= CONSOLE_MAX_RESULT_BYTES,
                ))
            }
            Err(diagnostic) => {
                let diagnostic = if diagnostic.code == "E2202" {
                    super::e1801(REPL_FUEL_BUDGET)
                } else {
                    restore_move_diagnostic(diagnostic, &self.repl.moved_names)
                };
                self.evaluation_diagnostic(input, &diagnostic)
            }
        }
    }

    fn evaluation_diagnostics(
        &mut self,
        input: &str,
        diagnostics: &[Diagnostic],
    ) -> Result<ConsoleOutput, ConsoleError> {
        let message = diagnostics
            .iter()
            .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.what))
            .collect::<Vec<_>>()
            .join("; ");
        self.repl.record_turn(
            input,
            super::ReplTurnStatus::Error,
            message.clone(),
        );
        Ok(self.output(
            ConsoleOutputKind::Error,
            false,
            message,
            format!("{{\"diagnostics\":{}}}", quote("see structured diagnostic text")),
            None,
            None,
            false,
        ))
    }

    fn evaluation_diagnostic(
        &mut self,
        input: &str,
        diagnostic: &Diagnostic,
    ) -> Result<ConsoleOutput, ConsoleError> {
        self.evaluation_diagnostics(input, std::slice::from_ref(diagnostic))
    }

    fn require_capability(
        &mut self,
        capability: ConsoleCapability,
        operation: &str,
    ) -> Result<(), ConsoleError> {
        let right = capability.required_right();
        if capability == ConsoleCapability::DbWrite || capability == ConsoleCapability::Commit {
            if self.mode != ConsoleMode::SandboxData {
                return self.fail(
                    operation,
                    ConsoleError::Denied {
                        required: right.to_string(),
                        reason: "writable operations require sandbox-data mode".to_string(),
                    },
                );
            }
        }
        let parent_verdict = answer(self.parent_authority.holds(), &self.denied, right);
        let effective = self.effective_authority();
        let verdict = answer(effective.holds(), &self.denied, right);
        if verdict != Verdict::Allowed {
            let reason = if verdict == Verdict::Denied {
                "explicit deny overrides the authority"
            } else if parent_verdict == Verdict::Allowed {
                "the capability grant expired"
            } else {
                "the authority does not hold the required right"
            };
            self.record_audit(ConsoleAuditEvent::new(
                &self.identity.session_id,
                None,
                operation,
                Some(right.to_string()),
                ConsoleAuditOutcome::Denied,
                reason,
                None,
                self.transaction_id.clone(),
            ));
            return Err(ConsoleError::Denied {
                required: right.to_string(),
                reason: reason.to_string(),
            });
        }
        Ok(())
    }

    fn ensure_active(&mut self, operation: &str) -> Result<(), ConsoleError> {
        if self.closed {
            return self.fail(operation, ConsoleError::Closed);
        }
        if self.cancelled {
            return self.fail(operation, ConsoleError::Cancelled);
        }
        if self.is_expired() {
            self.record_audit(ConsoleAuditEvent::new(
                &self.identity.session_id,
                None,
                operation,
                None,
                ConsoleAuditOutcome::Denied,
                "console session expired",
                None,
                self.transaction_id.clone(),
            ));
            return Err(ConsoleError::Expired);
        }
        Ok(())
    }

    fn fail<T>(&mut self, operation: &str, error: ConsoleError) -> Result<T, ConsoleError> {
        self.record_audit(ConsoleAuditEvent::new(
            &self.identity.session_id,
            None,
            operation,
            None,
            ConsoleAuditOutcome::Failed,
            audit_error_reason(&error),
            None,
            self.transaction_id.clone(),
        ));
        self.push_log(format!("{operation}: {error}"));
        Err(error)
    }

    fn next_request_id(&self, sequence: u64, method: &str, path: &str) -> String {
        let seed = format!(
            "{CONSOLE_PROTOCOL}\0request\0{}\0{}\0{}\0{}",
            self.identity.session_id, sequence, method, path
        );
        format!("req-{}", &digest(&seed)[..24])
    }

    fn output(
        &mut self,
        kind: ConsoleOutputKind,
        ok: bool,
        message: impl Into<String>,
        payload: impl Into<String>,
        request_id: Option<String>,
        transaction_id: Option<String>,
        truncated: bool,
    ) -> ConsoleOutput {
        self.output_sequence = self.output_sequence.saturating_add(1);
        let output = ConsoleOutput::new(
            self.output_sequence,
            &self.identity.session_id,
            kind,
            ok,
            message,
            payload,
            request_id,
            transaction_id,
            truncated,
        );
        self.outputs.push_back(output.clone());
        while self.outputs.len() > self.max_outputs {
            self.outputs.pop_front();
        }
        self.push_log(output.to_json_line());
        output
    }

    fn push_log(&mut self, line: String) {
        let line = bounded(line, CONSOLE_MAX_LOG_BYTES);
        self.logs.push_back(line);
        while self.logs.len() > self.max_outputs {
            self.logs.pop_front();
        }
    }

    fn record_audit(&mut self, mut event: ConsoleAuditEvent) {
        self.audit_sequence = self.audit_sequence.saturating_add(1);
        event.sequence = self.audit_sequence;
        self.audit.push_back(event);
        while self.audit.len() > self.max_audit {
            self.audit.pop_front();
        }
    }
}

impl Drop for ConsoleSession {
    fn drop(&mut self) {
        if let Some(request_id) = self.active_request.take() {
            if let Some(router) = self.router.as_mut() {
                router.cancel(&request_id);
            }
        }
        if let Some(transaction) = self.transaction.as_mut() {
            let _ = transaction.rollback();
        }
    }
}

/// The REPL policy remains the implementation of filesystem confinement and
/// host I/O.  This wrapper adds console-specific capability granularity and
/// redacted audit facts before delegating to that policy.
struct ConsoleReplAuthorization<'a, 'policy> {
    inner: &'a mut ReplAuthorization<'policy>,
    authority: &'a Authority,
    denied: &'a Holds,
    mode: ConsoleMode,
    session_id: &'a str,
    events: &'a mut Vec<ConsoleAuditEvent>,
}

impl ConsoleReplAuthorization<'_, '_> {
    fn required_right(&self, request: &ReplEffectRequest) -> String {
        match request.root.as_str() {
            "FS" => match request.operation.as_str() {
                "Read" => "FS.Read".to_string(),
                "Write" => "FS.Write".to_string(),
                _ => "FS".to_string(),
            },
            "DB" => {
                if is_read_operation(&request.operation) {
                    "DB.Read".to_string()
                } else {
                    "DB.Write".to_string()
                }
            }
            "Net" => "Net.Connect".to_string(),
            "Env" => format!("Env.{}", request.operation),
            "Exec" => "Exec.Run".to_string(),
            root => format!("{}.{}", root, request.operation),
        }
    }

    fn diagnostic(
        &self,
        request: &ReplEffectRequest,
        right: &str,
        reason: &str,
        span: Span,
    ) -> Diagnostic {
        Diagnostic::error(
            "E1803",
            format!("console capability `{right}` was denied"),
            format!(
                "{reason}; operation `{}` was stopped before the host path; session={}",
                request.operation, self.session_id
            ),
            format!("grant `{right}` explicitly or close the console"),
            Some(span),
        )
    }

    fn check(
        &mut self,
        request: &ReplEffectRequest,
        span: Span,
    ) -> Result<String, Diagnostic> {
        let right = self.required_right(request);
        let write = is_mutating_effect(request);
        if write
            && (self.mode != ConsoleMode::SandboxData
                || !request.root.eq_ignore_ascii_case("DB"))
        {
            let reason = if self.mode != ConsoleMode::SandboxData {
                "read-only console mode rejects mutations for every effect root"
            } else {
                "sandbox-data mode permits only database mutations"
            };
            let diagnostic = self.diagnostic(request, &right, reason, span);
            self.events.push(ConsoleAuditEvent::new(
                self.session_id,
                None,
                format!("repl.{}.{}", request.root, request.operation),
                Some(right),
                ConsoleAuditOutcome::Denied,
                reason,
                Some(&request.resource),
                None,
            ));
            return Err(diagnostic);
        }
        let verdict = answer(self.authority.holds(), self.denied, &right);
        if verdict != Verdict::Allowed {
            let reason = match verdict {
                Verdict::Denied => "explicit deny overrides the console authority",
                Verdict::Missing => "the console authority does not hold this right",
                Verdict::Allowed => unreachable!(),
            };
            let diagnostic = self.diagnostic(request, &right, reason, span);
            self.events.push(ConsoleAuditEvent::new(
                self.session_id,
                None,
                format!("repl.{}.{}", request.root, request.operation),
                Some(right),
                ConsoleAuditOutcome::Denied,
                reason,
                Some(&request.resource),
                None,
            ));
            return Err(diagnostic);
        }
        Ok(right)
    }
}

impl ReplAuthorizer for ConsoleReplAuthorization<'_, '_> {
    fn preflight(&mut self, request: &ReplEffectRequest, span: Span) -> Result<(), Diagnostic> {
        let right = self.check(request, span)?;
        if let Err(_diagnostic) = self.inner.preflight(request, span) {
            self.events.push(ConsoleAuditEvent::new(
                self.session_id,
                None,
                format!("repl.{}.{}", request.root, request.operation),
                Some(right),
                ConsoleAuditOutcome::Denied,
                "REPL host preflight rejected the operation",
                Some(&request.resource),
                None,
            ));
            return Err(self.diagnostic(
                request,
                &self.required_right(request),
                "REPL host preflight rejected the operation",
                span,
            ));
        }
        Ok(())
    }

    fn authorize(&mut self, request: &ReplEffectRequest, span: Span) -> Result<(), Diagnostic> {
        let right = self.check(request, span)?;
        if let Err(_diagnostic) = self.inner.authorize(request, span) {
            self.events.push(ConsoleAuditEvent::new(
                self.session_id,
                None,
                format!("repl.{}.{}", request.root, request.operation),
                Some(right.clone()),
                ConsoleAuditOutcome::Denied,
                "REPL authority rejected the operation",
                Some(&request.resource),
                None,
            ));
            return Err(self.diagnostic(
                request,
                &right,
                "REPL authority rejected the operation",
                span,
            ));
        }
        self.events.push(ConsoleAuditEvent::new(
            self.session_id,
            None,
            format!("repl.{}.{}", request.root, request.operation),
            Some(right),
            ConsoleAuditOutcome::Allowed,
            "REPL authority accepted the operation",
            Some(&request.resource),
            None,
        ));
        Ok(())
    }

    fn fs_read(&mut self, path: &str) -> std::io::Result<Vec<u8>> {
        self.inner.fs_read(path)
    }

    fn fs_write(&mut self, path: &str, bytes: &[u8], append: bool) -> std::io::Result<()> {
        self.inner.fs_write(path, bytes, append)
    }

    fn fs_exists(&mut self, path: &str) -> std::io::Result<bool> {
        self.inner.fs_exists(path)
    }

    fn fs_is_dir(&mut self, path: &str) -> std::io::Result<bool> {
        self.inner.fs_is_dir(path)
    }

    fn fs_create_dir(&mut self, path: &str) -> std::io::Result<()> {
        self.inner.fs_create_dir(path)
    }

    fn fs_remove(&mut self, path: &str) -> std::io::Result<()> {
        self.inner.fs_remove(path)
    }

    fn verified_root(&mut self) -> std::io::Result<std::fs::File> {
        self.inner.verified_root()
    }

    fn read_input(&mut self, prompt: &str) -> std::io::Result<String> {
        self.inner.read_input(prompt)
    }

    fn reset_session(&mut self) {
        self.inner.reset_session();
    }
}

fn policy_flags(authority: &Authority, denied: &Holds) -> ReplFlags {
    let allow = authority
        .rights()
        .map(root)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let deny = denied
        .iter()
        .map(|right| root(right))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    ReplFlags::new(&allow, &deny)
}

fn is_mutating_effect(request: &ReplEffectRequest) -> bool {
    let root = request.root.to_ascii_lowercase();
    if root == "db" {
        return !is_read_operation(&request.operation);
    }
    if matches!(
        root.as_str(),
        "net" | "exec" | "ffi" | "process" | "browser" | "secret"
    ) {
        return true;
    }
    !is_read_operation(&request.operation)
}

fn is_read_operation(operation: &str) -> bool {
    matches!(
        operation.to_ascii_lowercase().as_str(),
        "read"
            | "exists"
            | "is_dir"
            | "get"
            | "current_dir"
            | "home_dir"
            | "argv"
            | "args"
            | "input"
            | "receive"
            | "query"
            | "select"
            | "count"
            | "schema"
            | "tables"
            | "describe"
            | "show"
            | "list"
            | "lookup"
            | "scan"
    )
}


fn parse_grant(rest: &str) -> Result<ConsoleCommand, ConsoleError> {
    let mut fields = rest.split_whitespace();
    let capability = fields
        .next()
        .and_then(ConsoleCapability::parse)
        .ok_or_else(|| ConsoleError::InvalidInput("usage: :grant CAPABILITY [TTL_MS]".to_string()))?;
    let ttl_ms = fields
        .next()
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| ConsoleError::InvalidInput("TTL_MS must be an integer".to_string()))
        })
        .transpose()?
        .unwrap_or(0);
    if fields.next().is_some() {
        return Err(ConsoleError::InvalidInput(
            "usage: :grant CAPABILITY [TTL_MS]".to_string(),
        ));
    }
    Ok(ConsoleCommand::Grant { capability, ttl_ms })
}

fn parse_request(rest: &str) -> Result<ConsoleCommand, ConsoleError> {
    let trimmed = rest.trim_start();
    let (method, tail) = trimmed.split_once(char::is_whitespace).ok_or_else(|| {
        ConsoleError::InvalidInput("usage: :request METHOD /path [body]".to_string())
    })?;
    let tail = tail.trim_start();
    let (path, body) = match tail.split_once(char::is_whitespace) {
        Some((path, body)) => (path, body.trim_start()),
        None => (tail, ""),
    };
    if path.is_empty() {
        return Err(ConsoleError::InvalidInput(
            "usage: :request METHOD /path [body]".to_string(),
        ));
    }
    let method = method.to_ascii_uppercase();
    validate_method(&method)?;
    validate_local_path(path)?;
    if body.len() > CONSOLE_MAX_BODY_BYTES {
        return Err(ConsoleError::LimitExceeded("request body"));
    }
    Ok(ConsoleCommand::Request {
        method,
        path: path.to_string(),
        body: body.as_bytes().to_vec(),
    })
}

fn parse_inspect(rest: &str) -> Result<ConsoleCommand, ConsoleError> {
    let mut fields = rest.split_whitespace();
    let database = fields
        .next()
        .ok_or_else(|| ConsoleError::InvalidInput("usage: :inspect DATABASE OPERATION [LIMIT]".to_string()))?;
    let operation = fields
        .next()
        .ok_or_else(|| ConsoleError::InvalidInput("usage: :inspect DATABASE OPERATION [LIMIT]".to_string()))?;
    let limit = fields
        .next()
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|_| ConsoleError::InvalidInput("LIMIT must be an integer".to_string()))
        })
        .transpose()?
        .unwrap_or(CONSOLE_MAX_ROWS);
    if fields.next().is_some() {
        return Err(ConsoleError::InvalidInput(
            "usage: :inspect DATABASE OPERATION [LIMIT]".to_string(),
        ));
    }
    Ok(ConsoleCommand::Inspect(ConsoleDataRequest::new(
        database,
        operation,
        operation,
        limit,
    )?))
}

fn require_no_args(rest: &str, usage: &str) -> Result<(), ConsoleError> {
    if !rest.trim().is_empty() {
        return Err(ConsoleError::InvalidInput(usage.to_string()));
    }
    Ok(())
}

fn parse_sandbox(rest: &str) -> Result<ConsoleCommand, ConsoleError> {
    let mut fields = rest.split_whitespace();
    match fields.next().unwrap_or("data").to_ascii_lowercase().as_str() {
        "data" | "begin" => {
            if fields.next().is_some() {
                return Err(ConsoleError::InvalidInput(
                    "usage: :sandbox data".to_string(),
                ));
            }
            Ok(ConsoleCommand::BeginSandbox)
        }
        "reset" | "rollback" => {
            if fields.next().is_some() {
                return Err(ConsoleError::InvalidInput(
                    "usage: :sandbox reset".to_string(),
                ));
            }
            Ok(ConsoleCommand::ResetSandbox)
        }
        "savepoint" => {
            let name = fields
                .next()
                .ok_or_else(|| ConsoleError::InvalidInput("usage: :sandbox savepoint NAME".to_string()))?;
            if fields.next().is_some() {
                return Err(ConsoleError::InvalidInput(
                    "usage: :sandbox savepoint NAME".to_string(),
                ));
            }
            validate_token("savepoint", name)?;
            Ok(ConsoleCommand::Savepoint(name.to_string()))
        }
        "write" | "execute" => {
            let database = fields.next().ok_or_else(|| {
                ConsoleError::InvalidInput("usage: :sandbox write DATABASE ID OPERATION".to_string())
            })?;

            let identity = fields.next().ok_or_else(|| {
                ConsoleError::InvalidInput("usage: :sandbox write DATABASE ID OPERATION".to_string())
            })?;
            let operation = fields.next().ok_or_else(|| {
                ConsoleError::InvalidInput("usage: :sandbox write DATABASE ID OPERATION".to_string())
            })?;
            if fields.next().is_some() {
                return Err(ConsoleError::InvalidInput(
                    "usage: :sandbox write DATABASE ID OPERATION".to_string(),
                ));
            }
            Ok(ConsoleCommand::Execute(ConsoleDataStatement::new(
                database,
                identity,
                operation,
                Vec::new(),
            )?))
        }
        _ => Err(ConsoleError::InvalidInput(
            "usage: :sandbox data|reset|savepoint NAME|write DATABASE ID OPERATION".to_string(),
        )),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConsoleError {
    ReleaseUnavailable,
    ProjectUnavailable(String),
    InvalidInput(String),
    InvalidRequest(String),
    UnknownCommand(String),
    Closed,
    Cancelled,
    Expired,
    NonLoopbackDenied,
    RouterUnavailable,
    DataBackendUnavailable,
    TransactionActive,
    NoTransaction,
    ConfirmationRequired(String),
    Denied { required: String, reason: String },
    LimitExceeded(&'static str),
    Backend(String),
}

impl fmt::Display for ConsoleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReleaseUnavailable => write!(formatter, "project console is unavailable in release builds"),
            Self::ProjectUnavailable(reason) => write!(formatter, "project console unavailable: {reason}"),
            Self::InvalidInput(reason) | Self::InvalidRequest(reason) => write!(formatter, "invalid console input: {reason}"),
            Self::UnknownCommand(command) => write!(formatter, "unknown console command `:{command}`"),
            Self::Closed => write!(formatter, "console session is closed"),
            Self::Cancelled => write!(formatter, "console session is cancelled"),
            Self::Expired => write!(formatter, "console authority has expired; restart the console"),
            Self::NonLoopbackDenied => write!(formatter, "non-loopback console request rejected before dispatch"),
            Self::RouterUnavailable => write!(formatter, "in-process application router is not attached"),
            Self::DataBackendUnavailable => write!(formatter, "project data backend is not attached"),
            Self::TransactionActive => write!(formatter, "sandbox transaction is already active"),
            Self::NoTransaction => write!(formatter, "no sandbox transaction is active"),
            Self::ConfirmationRequired(expected) => write!(formatter, "explicit confirmation required: `{expected}`"),
            Self::Denied { required, reason } => write!(formatter, "console capability `{required}` denied: {reason}"),
            Self::LimitExceeded(what) => write!(formatter, "console {what} exceeds its bounded limit"),
            Self::Backend(reason) => write!(formatter, "console backend error: {reason}"),
        }
    }
}

impl std::error::Error for ConsoleError {}

pub fn run(options: ConsoleOptions) -> Result<Vec<ConsoleOutput>, ConsoleError> {
    ConsoleSession::run(options)
}

fn validate_token(label: &str, value: &str) -> Result<(), ConsoleError> {
    if value.is_empty()
        || value.len() > CONSOLE_MAX_TOKEN_BYTES
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(ConsoleError::InvalidInput(format!(
            "{label} must be a bounded non-empty token"
        )));
    }
    Ok(())
}

fn validate_text(label: &str, value: &str) -> Result<(), ConsoleError> {
    if value.len() > CONSOLE_MAX_TEXT_BYTES || value.chars().any(char::is_control) {
        return Err(ConsoleError::InvalidInput(format!(
            "{label} contains a control character or exceeds its limit"
        )));
    }
    Ok(())
}

fn validate_method(method: &str) -> Result<(), ConsoleError> {
    if method != method.to_ascii_uppercase()
        || method.is_empty()
        || method
            .chars()
            .any(|character| !character.is_ascii_alphanumeric() && character != '-')
    {
        return Err(ConsoleError::InvalidRequest(
            "request method must be an uppercase HTTP token".to_string(),
        ));
    }
    Ok(())
}

fn validate_local_path(path: &str) -> Result<(), ConsoleError> {
    if path.len() > CONSOLE_MAX_TEXT_BYTES
        || !path.starts_with('/')
        || path.starts_with("//")
        || path.contains('\0')
        || path.contains('\\')
        || path.contains("://")
        || path.split('/').any(|segment| segment == "..")
    {
        return Err(ConsoleError::NonLoopbackDenied);
    }
    if path.chars().any(char::is_control) {
        return Err(ConsoleError::InvalidRequest(
            "request path contains a control character".to_string(),
        ));
    }
    Ok(())
}

fn validate_origin(origin: &str) -> Result<(), ConsoleError> {
    validate_token("request origin", origin)?;
    if origin.contains('/')
        || origin.contains('\\')
        || origin.contains("://")
        || origin.contains('@')
        || origin.contains('?')
        || origin.contains('#')
    {
        return Err(ConsoleError::NonLoopbackDenied);
    }
    Ok(())
}

fn validate_safe_request(method: &str, body: &[u8]) -> Result<(), ConsoleError> {
    match method {
        "GET" | "HEAD" | "OPTIONS" => {
            if body.is_empty() {
                Ok(())
            } else {
                Err(ConsoleError::InvalidRequest(
                    "safe console requests cannot carry a body".to_string(),
                ))
            }
        }
        _ => Err(ConsoleError::Denied {
            required: ConsoleCapability::HttpRequest.required_right().to_string(),
            reason: "only GET, HEAD, and OPTIONS requests are allowed at the console boundary"
                .to_string(),
        }),
    }
}

fn is_loopback_host(host: &str) -> bool {
    let host = host.trim().to_ascii_lowercase();
    let host = if let Some(rest) = host.strip_prefix('[') {
        rest.split_once(']')
            .map(|(address, suffix)| {
                let valid_port = suffix
                    .strip_prefix(':')
                    .and_then(|port| port.parse::<u16>().ok())
                    .is_some();
                if suffix.is_empty() || valid_port {
                    address.to_string()
                } else {
                    String::new()
                }
            })
            .unwrap_or_default()
    } else if host.matches(':').count() == 1 {
        if let Some((address, port)) = host.rsplit_once(':') {
            if port.parse::<u16>().is_ok() {
                address.to_string()
            } else {
                host
            }
        } else {
            host
        }
    } else {
        host
    };
    matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1" | "loopback")
}

fn is_checked_read_operation(operation: &str) -> bool {
    matches!(
        operation.to_ascii_lowercase().as_str(),
        "read" | "select" | "query" | "count" | "schema" | "tables" | "describe"
    )
}

fn is_checked_write_operation(operation: &str) -> bool {
    matches!(
        operation.to_ascii_lowercase().as_str(),
        "write" | "insert" | "update" | "delete" | "remove"
    )
}

fn validate_data_value(value: &ConsoleDataValue) -> Result<(), ConsoleError> {
    match value {
        ConsoleDataValue::Float(value) => {
            if !value
                .parse::<f64>()
                .map(|parsed| parsed.is_finite())
                .unwrap_or(false)
            {
                return Err(ConsoleError::InvalidInput(
                    "data float must be finite".to_string(),
                ));
            }
        }
        ConsoleDataValue::Text(value) => validate_text("data text", value)?,
        ConsoleDataValue::Bytes(value) => {
            if value.len() > CONSOLE_MAX_BODY_BYTES {
                return Err(ConsoleError::LimitExceeded("data bytes"));
            }
        }
        ConsoleDataValue::Null
        | ConsoleDataValue::Int(_)
        | ConsoleDataValue::Bool(_)
        | ConsoleDataValue::Secret => {}
    }
    Ok(())
}

fn validate_data_result(result: &ConsoleDataResult) -> Result<(), ConsoleError> {
    if result.columns.len() > CONSOLE_MAX_HEADERS {
        return Err(ConsoleError::LimitExceeded("data columns"));
    }
    if result.rows.len() > CONSOLE_MAX_ROWS {
        return Err(ConsoleError::LimitExceeded("data rows"));
    }
    let mut column_names = BTreeSet::new();
    for column in &result.columns {
        validate_token("data column", &column.name)?;
        validate_token("data type", &column.type_name)?;
        if !column_names.insert(column.name.as_str()) {
            return Err(ConsoleError::InvalidRequest(
                "data result contains duplicate columns".to_string(),
            ));
        }
    }
    for row in &result.rows {
        if row.len() > result.columns.len() {
            return Err(ConsoleError::InvalidRequest(
                "data row contains more values than declared columns".to_string(),
            ));
        }
        if row.len() > CONSOLE_MAX_HEADERS {
            return Err(ConsoleError::LimitExceeded("data row columns"));
        }
        for value in row {
            validate_data_value(value)?;
        }
    }
    Ok(())
}


fn is_secret_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization"
            | "proxy-authorization"
            | "cookie"
            | "set-cookie"
            | "x-api-key"
            | "x-auth-token"
    )
}

fn quote(value: &str) -> String {
    format!("\"{}\"", jet_foundation::JSON::json_escape(value))
}

fn optional_quote(value: Option<&str>) -> String {
    value.map(quote).unwrap_or_else(|| "null".to_string())
}

fn digest(value: &str) -> String {
    SHA256::sha256_hex(value.as_bytes())
}

fn bounded(value: String, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value;
    }
    let suffix = "…[truncated]";
    let take = max_bytes.saturating_sub(suffix.len());
    let mut end = take.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    let mut output = value[..end].to_string();
    output.push_str(suffix);
    output
}

fn bounded_bytes(mut value: Vec<u8>, max_bytes: usize) -> (Vec<u8>, bool) {
    if value.len() <= max_bytes {
        return (value, false);
    }
    value.truncate(max_bytes);
    (value, true)
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}
fn audit_error_reason(error: &ConsoleError) -> String {
    match error {
        ConsoleError::Backend(_) => "backend operation failed".to_string(),
        _ => error.to_string(),
    }
}
