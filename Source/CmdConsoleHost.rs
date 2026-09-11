//! In-process host adapters for the project console.
//!
//! The console owns policy and output bounds.  This module only binds those
//! transport-neutral seams to the existing resident JIT HTTP mux and SQLite
//! runtime.  Database handles and checked statements are explicit inputs; no
//! path or ambient authority is guessed here.

use std::collections::BTreeMap;

use jet::REPL::{
    ConsoleDataBackend, ConsoleDataColumn, ConsoleDataRequest, ConsoleDataResult,
    ConsoleDataStatement, ConsoleDataTransaction, ConsoleDataValue, ConsoleDatabaseHandle,
    ConsoleError, ConsoleHostAdapters, ConsoleProject, ConsoleRequest, ConsoleResponse,
    ConsoleServiceHandle, ConsoleSession, InProcessRouter,
};
use jet_jit::{
    ConsoleDbConnection, ConsoleDbQueryResult, ConsoleDbResource, ConsoleDbTransaction,
    ConsoleDbValue, ConsoleHttpRequest, ConsoleHttpRouter, ConsoleServiceBinding,
};

const BUILTIN_READ_OPERATIONS: [&str; 4] = ["tables", "schema", "count", "describe"];

/// One checked statement made available to the console by the application
/// host.  The SQL remains host-owned and is never parsed from console input.
#[derive(Clone)]
pub(crate) struct ConsoleDbStatement {
    operation: String,
    sql: String,
}

impl ConsoleDbStatement {
    pub(crate) fn new(
        operation: impl Into<String>,
        sql: impl Into<String>,
    ) -> Result<Self, ConsoleError> {
        let operation = operation.into();
        let sql = sql.into();
        validate_token("database operation", &operation)?;
        if sql.trim().is_empty() || sql.chars().any(char::is_control) {
            return Err(ConsoleError::InvalidInput(
                "database statement SQL must be non-empty text".to_string(),
            ));
        }
        Ok(Self { operation, sql })
    }
}

/// Explicit binding of one declared project database to the canonical JIT
/// SQLite runtime.  A binding does not infer a path or create a fallback DB.
pub(crate) struct ConsoleDbAdapter {
    identity: String,
    connection: ConsoleDbConnection,
    reads: BTreeMap<String, ConsoleDbStatement>,
    writes: BTreeMap<String, ConsoleDbStatement>,
}

impl ConsoleDbAdapter {
    pub(crate) fn new(
        identity: impl Into<String>,
        connection: ConsoleDbConnection,
    ) -> Result<Self, ConsoleError> {
        let identity = identity.into();
        validate_token("database identity", &identity)?;
        Ok(Self {
            identity,
            connection,
            reads: BTreeMap::new(),
            writes: BTreeMap::new(),
        })
    }

    pub(crate) fn with_read_statement(
        mut self,
        identity: impl Into<String>,
        statement: ConsoleDbStatement,
    ) -> Result<Self, ConsoleError> {
        let identity = identity.into();
        validate_token("statement identity", &identity)?;
        if self.reads.insert(identity, statement).is_some() {
            return Err(ConsoleError::InvalidInput(
                "database read statement identity is duplicated".to_string(),
            ));
        }
        Ok(self)
    }

    pub(crate) fn with_write_statement(
        mut self,
        identity: impl Into<String>,
        statement: ConsoleDbStatement,
    ) -> Result<Self, ConsoleError> {
        let identity = identity.into();
        validate_token("statement identity", &identity)?;
        if self.writes.insert(identity, statement).is_some() {
            return Err(ConsoleError::InvalidInput(
                "database write statement identity is duplicated".to_string(),
            ));
        }
        Ok(self)
    }

    fn read_statement(&self, identity: &str, operation: &str) -> Option<ConsoleDbStatement> {
        self.reads
            .get(identity)
            .filter(|statement| statement.operation.eq_ignore_ascii_case(operation))
            .cloned()
            .or_else(|| {
                BUILTIN_READ_OPERATIONS
                    .iter()
                    .find(|candidate| candidate.eq_ignore_ascii_case(identity))
                    .filter(|candidate| candidate.eq_ignore_ascii_case(operation))
                    .map(|candidate| ConsoleDbStatement {
                        operation: (*candidate).to_string(),
                        sql: String::new(),
                    })
            })
    }

    fn write_statement(&self, identity: &str, operation: &str) -> Option<ConsoleDbStatement> {
        self.writes
            .get(identity)
            .filter(|statement| statement.operation.eq_ignore_ascii_case(operation))
            .cloned()
    }
}

struct ConsoleDatabaseBinding {
    handle: ConsoleDatabaseHandle,
    adapter: ConsoleDbAdapter,
}

struct ConsoleServiceBindingHost {
    handle: ConsoleServiceHandle,
    endpoint: ConsoleServiceBinding,
}

/// Builder for the concrete host seams. Callers attach the live resident
/// resource graph explicitly; this type never chooses paths or derives
/// authority from process state.
pub(crate) struct ConsoleHost {
    router: Option<ConsoleHttpRouter>,
    databases: Vec<ConsoleDatabaseBinding>,
    services: Vec<ConsoleServiceBindingHost>,
}

impl ConsoleHost {
    pub(crate) fn new() -> Self {
        Self {
            router: None,
            databases: Vec::new(),
            services: Vec::new(),
        }
    }

    pub(crate) fn with_router(mut self, router: ConsoleHttpRouter) -> Self {
        self.router = Some(router);
        self
    }

    pub(crate) fn with_database(
        mut self,
        name: impl Into<String>,
        adapter: ConsoleDbAdapter,
    ) -> Result<Self, ConsoleError> {
        let name = name.into();
        let handle = ConsoleDatabaseHandle::new(&name, adapter.identity.clone())?;
        if self
            .databases
            .iter()
            .any(|binding| binding.handle.name == name)
        {
            return Err(ConsoleError::InvalidInput(format!(
                "database `{name}` is attached more than once"
            )));
        }
        self.databases.push(ConsoleDatabaseBinding { handle, adapter });
        Ok(self)
    }
    pub(crate) fn with_database_resource(
        self,
        resource: ConsoleDbResource,
    ) -> Result<Self, ConsoleError> {
        let (name, identity, connection) = resource.into_parts();
        self.with_database(name, ConsoleDbAdapter::new(identity, connection)?)
    }

    pub(crate) fn with_service_binding(
        mut self,
        endpoint: ConsoleServiceBinding,
    ) -> Result<Self, ConsoleError> {
        let handle =
            ConsoleServiceHandle::new(endpoint.name(), endpoint.identity(), endpoint.state())?;
        if self
            .services
            .iter()
            .any(|binding| binding.handle.name == handle.name)
        {
            return Err(ConsoleError::InvalidInput(format!(
                "service `{}` is attached more than once",
                handle.name
            )));
        }
        self.services
            .push(ConsoleServiceBindingHost { handle, endpoint });
        Ok(self)

    }

    /// Add the host's declared handles to the project facts before the session
    /// is opened. Existing matching facts are accepted so package facts can
    /// remain the source of the declaration when the CLI has one.
    pub(crate) fn attach_project(
        &self,
        mut project: ConsoleProject,
    ) -> Result<ConsoleProject, ConsoleError> {
        for binding in &self.databases {
            if let Some(existing) = project
                .databases
                .iter()
                .find(|database| database.name == binding.handle.name)
            {
                if existing.identity != binding.handle.identity {
                    return Err(ConsoleError::InvalidRequest(format!(
                        "database `{}` identity does not match its host adapter",
                        binding.handle.name
                    )));
                }
                continue;
            }
            project.add_database(binding.handle.clone())?;
        }
        for binding in &self.services {
            if project
                .services
                .iter()
                .any(|service| service.name == binding.handle.name)
            {
                continue;
            }
            project.add_service(binding.handle.clone())?;
        }
        Ok(project)
    }

    pub(crate) fn open(
        self,
        options: jet::REPL::ConsoleOptions,
    ) -> Result<ConsoleSession, ConsoleError> {
        let ConsoleHost {
            router,
            databases,
            services,
        } = self;
        let adapters = ConsoleHost {
            router,
            databases,
            services: Vec::new(),
        }
        .adapters();
        let mut session = jet::REPL::open_attached(options, adapters)?;
        for binding in services {
            session.attach_resource(binding.endpoint);
        }
        Ok(session)
    }

    pub(crate) fn adapters(self) -> ConsoleHostAdapters {
        let mut adapters = ConsoleHostAdapters::new();
        if let Some(router) = self.router {
            adapters = adapters.with_router(ConsoleHttpAdapter { router });
        }
        if !self.databases.is_empty() {
            let databases = self
                .databases
                .into_iter()
                .map(|binding| (binding.handle.name, binding.adapter))
                .collect();
            adapters = adapters.with_data_backend(ConsoleDbBackend { databases });
        }
        adapters
    }
}

struct ConsoleHttpAdapter {
    router: ConsoleHttpRouter,
}

impl InProcessRouter for ConsoleHttpAdapter {
    fn dispatch(&mut self, request: ConsoleRequest) -> Result<ConsoleResponse, ConsoleError> {
        let response = self
            .router
            .dispatch(ConsoleHttpRequest {
                request_id: request.request_id,
                session_id: request.session_id,
                method: request.method,
                path: request.path,
                origin: request.origin,
                headers: request.headers,
                body: request.body,
            })
            .map_err(console_http_error)?;
        let mut output = ConsoleResponse::new(response.request_id, response.status)?;
        for (name, value) in response.headers {
            output = output.with_header(name, value)?;
        }
        output.with_body(response.body)
    }

    fn cancel(&mut self, request_id: &str) {
        self.router.cancel(request_id);
    }
}

struct ConsoleDbBackend {
    databases: BTreeMap<String, ConsoleDbAdapter>,
}

impl ConsoleDbBackend {
    fn database<'a>(
        &'a mut self,
        handle: &ConsoleDatabaseHandle,
    ) -> Result<&'a mut ConsoleDbAdapter, ConsoleError> {
        let database = self.databases.get_mut(&handle.name).ok_or_else(|| {
            ConsoleError::InvalidRequest(format!(
                "database `{}` has no attached host adapter",
                handle.name
            ))
        })?;
        if database.identity != handle.identity {
            return Err(ConsoleError::InvalidRequest(format!(
                "database `{}` identity does not match its host adapter",
                handle.name
            )));
        }
        if !database.connection.is_live() {
            return Err(ConsoleError::Backend(format!(
                "database `{}` connection is unavailable",
                handle.name
            )));
        }
        Ok(database)
    }

    fn builtin_sql(operation: &str, limit: usize) -> Option<String> {
        let limit = limit.max(1);
        match operation.to_ascii_lowercase().as_str() {
            "tables" => Some(format!(
                "SELECT name, type FROM sqlite_master WHERE type IN ('table','view') ORDER BY name LIMIT {limit}"
            )),
            "schema" | "describe" => Some(format!(
                "SELECT name, type, sql FROM sqlite_master WHERE type IN ('table','view') ORDER BY name LIMIT {limit}"
            )),
            "count" => Some(
                "SELECT COUNT(*) AS count FROM sqlite_master WHERE type IN ('table','view')"
                    .to_string(),
            ),
            _ => None,
        }
    }

    fn resolve_read(
        &mut self,
        database: &ConsoleDatabaseHandle,
        request: &ConsoleDataRequest,
    ) -> Result<(), ConsoleError> {
        let adapter = self.database(database)?;
        if !is_checked_read(request.operation.as_str()) {
            return Err(ConsoleError::InvalidRequest(
                "data inspection requires a checked read statement identity".to_string(),
            ));
        }
        if adapter
            .read_statement(&request.statement_identity, &request.operation)
            .is_none()
        {
            return Err(ConsoleError::InvalidRequest(format!(
                "read statement `{}` is not declared for database `{}`",
                request.statement_identity, database.name
            )));
        }
        Ok(())
    }

    fn resolve_write(
        &mut self,
        database: &ConsoleDatabaseHandle,
        statement: &ConsoleDataStatement,
    ) -> Result<(), ConsoleError> {
        let adapter = self.database(database)?;
        if !is_checked_write(statement.operation.as_str()) {
            return Err(ConsoleError::InvalidRequest(
                "data mutation requires a checked write statement identity".to_string(),
            ));
        }
        if adapter
            .write_statement(&statement.statement_identity, &statement.operation)
            .is_none()
        {
            return Err(ConsoleError::InvalidRequest(format!(
                "write statement `{}` is not declared for database `{}`",
                statement.statement_identity, database.name
            )));
        }
        Ok(())
    }
}

impl ConsoleDataBackend for ConsoleDbBackend {
    fn resolve_inspect(
        &mut self,
        database: &ConsoleDatabaseHandle,
        request: &ConsoleDataRequest,
    ) -> Result<ConsoleDataRequest, ConsoleError> {
        self.resolve_read(database, request)?;
        Ok(request.clone())
    }

    fn inspect(&mut self, request: &ConsoleDataRequest) -> Result<ConsoleDataResult, ConsoleError> {
        let database = self.databases.get_mut(&request.database).ok_or_else(|| {
            ConsoleError::InvalidRequest(format!(
                "database `{}` has no attached host adapter",
                request.database
            ))
        })?;
        let statement = database
            .read_statement(&request.statement_identity, &request.operation)
            .ok_or_else(|| {
                ConsoleError::InvalidRequest(format!(
                    "read statement `{}` is not declared for database `{}`",
                    request.statement_identity, request.database
                ))
            })?;
        let sql = if statement.sql.is_empty() {
            Self::builtin_sql(&statement.operation, request.limit).ok_or_else(|| {
                ConsoleError::Backend(format!(
                    "read statement `{}` has no host SQL",
                    request.statement_identity
                ))
            })?
        } else {
            statement.sql
        };
        let result = database
            .connection
            .query(&sql, &[], request.limit)
            .map_err(ConsoleError::Backend)?;
        console_data_result(result)
    }

    fn resolve_statement(
        &mut self,
        database: &ConsoleDatabaseHandle,
        statement: &ConsoleDataStatement,
    ) -> Result<ConsoleDataStatement, ConsoleError> {
        self.resolve_write(database, statement)?;
        Ok(statement.clone())
    }

    fn begin(
        &mut self,
        database: &ConsoleDatabaseHandle,
        transaction_id: &str,
    ) -> Result<Box<dyn ConsoleDataTransaction>, ConsoleError> {
        let adapter = self.database(database)?;
        let transaction = adapter
            .connection
            .begin(transaction_id.to_string())
            .map_err(ConsoleError::Backend)?;
        Ok(Box::new(ConsoleDbTransactionAdapter {
            transaction,
            writes: adapter.writes.clone(),
        }))
    }
}

struct ConsoleDbTransactionAdapter {
    transaction: ConsoleDbTransaction,
    writes: BTreeMap<String, ConsoleDbStatement>,
}

impl ConsoleDataTransaction for ConsoleDbTransactionAdapter {
    fn savepoint(&mut self, name: &str) -> Result<(), ConsoleError> {
        self.transaction
            .savepoint(name)
            .map_err(ConsoleError::Backend)
    }

    fn rollback_to(&mut self, name: &str) -> Result<(), ConsoleError> {
        self.transaction
            .rollback_to(name)
            .map_err(ConsoleError::Backend)
    }

    fn execute(
        &mut self,
        statement: &ConsoleDataStatement,
    ) -> Result<jet::REPL::ConsoleDataMutation, ConsoleError> {
        let Some(registered) = self.writes.get(&statement.statement_identity) else {
            return Err(ConsoleError::InvalidRequest(format!(
                "write statement `{}` is not declared",
                statement.statement_identity
            )));
        };
        if !registered.operation.eq_ignore_ascii_case(&statement.operation) {
            return Err(ConsoleError::InvalidRequest(
                "write statement operation does not match its declaration".to_string(),
            ));
        }
        let values = console_db_values(&statement.parameters)?;
        let affected = self
            .transaction
            .execute(&registered.sql, &values)
            .map_err(ConsoleError::Backend)?;
        Ok(jet::REPL::ConsoleDataMutation {
            transaction_id: self.transaction.transaction_id().to_string(),
            statement_identity: statement.statement_identity.clone(),
            rows_affected: affected,
        })
    }

    fn commit(&mut self) -> Result<(), ConsoleError> {
        self.transaction.commit().map_err(ConsoleError::Backend)
    }

    fn rollback(&mut self) -> Result<(), ConsoleError> {
        self.transaction.rollback().map_err(ConsoleError::Backend)
    }

    fn writes(&self) -> u64 {
        self.transaction.writes()
    }
}

fn console_data_result(result: ConsoleDbQueryResult) -> Result<ConsoleDataResult, ConsoleError> {
    let columns = result
        .columns
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let type_name = result
                .rows
                .iter()
                .filter_map(|row| row.get(index))
                .map(console_type_name)
                .next()
                .unwrap_or("Null");
            ConsoleDataColumn::new(name, type_name)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let rows = result
        .rows
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(console_data_value)
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ConsoleDataResult {
        columns,
        rows,
        truncated: result.truncated,
    })
}

fn console_type_name(value: &ConsoleDbValue) -> &'static str {
    match value {
        ConsoleDbValue::Null => "Null",
        ConsoleDbValue::Int(_) => "Int",
        ConsoleDbValue::Float(_) => "Float",
        ConsoleDbValue::Bool(_) => "Bool",
        ConsoleDbValue::Text(_) => "String",
        ConsoleDbValue::Bytes(_) => "Bytes",
    }
}

fn console_data_value(value: ConsoleDbValue) -> Result<ConsoleDataValue, ConsoleError> {
    Ok(match value {
        ConsoleDbValue::Null => ConsoleDataValue::Null,
        ConsoleDbValue::Int(value) => ConsoleDataValue::Int(value),
        ConsoleDbValue::Float(value) => ConsoleDataValue::Float(value.to_string()),
        ConsoleDbValue::Bool(value) => ConsoleDataValue::Bool(value),
        ConsoleDbValue::Text(value) => ConsoleDataValue::Text(value),
        ConsoleDbValue::Bytes(value) => ConsoleDataValue::Bytes(value),
    })
}

fn console_db_values(
    values: &[ConsoleDataValue],
) -> Result<Vec<ConsoleDbValue>, ConsoleError> {
    values
        .iter()
        .map(|value| match value {
            ConsoleDataValue::Null => Ok(ConsoleDbValue::Null),
            ConsoleDataValue::Int(value) => Ok(ConsoleDbValue::Int(*value)),
            ConsoleDataValue::Float(value) => {
                let parsed = value.parse::<f64>().map_err(|_| {
                    ConsoleError::InvalidInput("data float parameter is invalid".to_string())
                })?;
                if !parsed.is_finite() {
                    return Err(ConsoleError::InvalidInput(
                        "data float parameter must be finite".to_string(),
                    ));
                }
                Ok(ConsoleDbValue::Float(parsed))
            }
            ConsoleDataValue::Bool(value) => Ok(ConsoleDbValue::Bool(*value)),
            ConsoleDataValue::Text(value) => Ok(ConsoleDbValue::Text(value.clone())),
            ConsoleDataValue::Bytes(value) => Ok(ConsoleDbValue::Bytes(value.clone())),
            ConsoleDataValue::Secret => Err(ConsoleError::InvalidRequest(
                "secret data parameters cannot be sent to a console write".to_string(),
            )),
        })
        .collect()
}

fn is_checked_read(operation: &str) -> bool {
    matches!(
        operation.to_ascii_lowercase().as_str(),
        "read" | "select" | "query" | "count" | "schema" | "tables" | "describe"
    )
}

fn is_checked_write(operation: &str) -> bool {
    matches!(
        operation.to_ascii_lowercase().as_str(),
        "write" | "insert" | "update" | "delete" | "remove"
    )
}

fn validate_token(label: &str, value: &str) -> Result<(), ConsoleError> {
    if value.is_empty()
        || value.len() > 256
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

fn console_http_error(error: String) -> ConsoleError {
    if error == "HTTP operation cancelled" {
        ConsoleError::Cancelled
    } else {
        ConsoleError::Backend(error)
    }
}
