//! `core.db` hosts (#729). `include!` canonical SQLite runtime + wire codec —
//! no third algorithm.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.

use jet_foundation::AST::{CtValue, Type};
use jet_foundation::Diagnostics::{Diagnostic, Span};
use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

trait JetShow {
    fn jet_show(&self) -> String;
}

trait JetDebug {
    fn jet_debug(&self) -> String;
}

/// D-FAIL-CONV2=A: included error fragments render failure text through this seam.
trait JetDisplay {
    fn jet_display(&self) -> String;
}

// The shared DB wire fragment receives the host's row carrier through this
// name. AOT supplies its JetMap; JIT keeps rows in the native map until heap
// marshalling.
type JetMap<K, V> = std::collections::BTreeMap<K, V>;

/// Canonical `core.db` FFI runtime (rusqlite).
mod runtime {
    include!("../../jet-pkg-model/src/Prelude/DB.rs");
}

/// Canonical wire encode/decode (`jet_std` DBPluginWire fragment) plus the one
/// closed row-policy language (`RowPolicy.rs`) it compiles through — the same
/// two fragments AOT splices into `mod jet_std` (I9).
mod wire {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    // DBPluginWire re-exports the shared plugin carrier from `plugin_wire`, which
    // AOT splices into `mod jet_std` from jet-foundation's PluginWire.rs.
    use jet_foundation::PluginWire as plugin_wire;
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/RowPolicy.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/DBPluginWire.rs");
}

fn jet_std_time_now() -> i64 {
    jet_codegen::scheduler::jet_std_time_now()
}

pub const MIGRATION_SCHEMA_IDENTITY: &str = "jet.db.migration/v2";

/// Typed migration operation shared by the CLI, resident console, and JIT
/// adapter.  The database ledger, not a generated child program, records it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationOperation {
    Apply,
    Rollback,
    Direct,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationLock {
    Shared,
    Exclusive,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MigrationSql {
    pub ordinal: u64,
    pub sql: String,
    pub inverse_sql: Option<String>,
    pub risk: String,
    pub lock: MigrationLock,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MigrationRequest {
    pub operation: MigrationOperation,
    pub migration_key: String,
    pub target: String,
    pub target_identity: String,
    pub database_identity: String,
    pub source_identity: String,
    pub schema_identity: String,
    pub tool_identity: String,
    pub from_version: u64,
    pub to_version: u64,
    pub lock: MigrationLock,
    pub risk: String,
    pub steps: Vec<MigrationSql>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MigrationOutcome {
    pub receipt_id: String,
    pub operation: String,
    pub status: String,
    pub target: String,
    pub target_identity: String,
    pub source_identity: String,
    pub schema_identity: String,
    pub database_identity: String,
    pub tool_identity: String,
    pub from_version: u64,
    pub to_version: u64,
    pub step_count: usize,
    pub step_ids: Vec<String>,
    pub checksum: String,
    pub lock: MigrationLock,
    pub risk: String,
    pub started_unix_ms: u128,
    pub finished_unix_ms: u128,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationStateRequest {
    pub target: String,
    pub target_identity: String,
    pub database_identity: String,
    pub source_identity: String,
    pub schema_identity: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MigrationState {
    pub target: String,
    pub target_identity: String,
    pub database_identity: String,
    pub source_identity: String,
    pub schema_identity: String,
    pub current_version: u64,
    pub checksum: String,
    pub receipts: Vec<MigrationOutcome>,
    pub drift: Vec<String>,
}

impl MigrationState {
    pub fn is_drifted(&self) -> bool {
        !self.drift.is_empty()
    }
}

fn wire_migration_lock(lock: MigrationLock) -> wire::JetMigrationLock {
    match lock {
        MigrationLock::Shared => wire::JetMigrationLock::Shared,
        MigrationLock::Exclusive => wire::JetMigrationLock::Exclusive,
    }
}

fn wire_migration_operation(operation: MigrationOperation) -> wire::JetMigrationOperation {
    match operation {
        MigrationOperation::Apply => wire::JetMigrationOperation::Apply,
        MigrationOperation::Rollback => wire::JetMigrationOperation::Rollback,
        MigrationOperation::Direct => wire::JetMigrationOperation::Direct,
    }
}

fn wire_migration_request(request: &MigrationRequest) -> wire::JetMigrationRequest {
    wire::JetMigrationRequest {
        operation: wire_migration_operation(request.operation),
        migration_key: request.migration_key.clone(),
        target: request.target.clone(),
        target_identity: request.target_identity.clone(),
        database_identity: request.database_identity.clone(),
        source_identity: request.source_identity.clone(),
        schema_identity: request.schema_identity.clone(),
        tool_identity: request.tool_identity.clone(),
        from_version: request.from_version,
        to_version: request.to_version,
        lock: wire_migration_lock(request.lock),
        risk: request.risk.clone(),
        steps: request
            .steps
            .iter()
            .map(|step| wire::JetMigrationSql {
                ordinal: step.ordinal,
                sql: (step.sql.clone(), Vec::new()),
                inverse_sql: step
                    .inverse_sql
                    .as_ref()
                    .map(|sql| (sql.clone(), Vec::new())),
                risk: step.risk.clone(),
                lock: wire_migration_lock(step.lock),
            })
            .collect(),
    }
}

fn migration_lock_from_wire(lock: wire::JetMigrationLock) -> MigrationLock {
    match lock {
        wire::JetMigrationLock::Shared => MigrationLock::Shared,
        wire::JetMigrationLock::Exclusive => MigrationLock::Exclusive,
    }
}

fn migration_outcome_from_wire(outcome: wire::JetMigrationOutcome) -> MigrationOutcome {
    MigrationOutcome {
        receipt_id: outcome.receipt_id,
        operation: outcome.operation,
        status: outcome.status,
        target: outcome.target,
        target_identity: outcome.target_identity,
        source_identity: outcome.source_identity,
        schema_identity: outcome.schema_identity,
        database_identity: outcome.database_identity,
        tool_identity: outcome.tool_identity,
        from_version: outcome.from_version,
        to_version: outcome.to_version,
        step_count: outcome.step_count,
        step_ids: outcome.step_ids,
        checksum: outcome.checksum,
        lock: migration_lock_from_wire(outcome.lock),
        risk: outcome.risk,
        started_unix_ms: outcome.started_unix_ms,
        finished_unix_ms: outcome.finished_unix_ms,
        error: outcome.error,
    }
}

fn migration_state_from_wire(state: wire::JetMigrationState) -> MigrationState {
    MigrationState {
        target: state.target,
        target_identity: state.target_identity,
        database_identity: state.database_identity,
        source_identity: state.source_identity,
        schema_identity: state.schema_identity,
        current_version: state.current_version,
        checksum: state.checksum,
        receipts: state
            .receipts
            .into_iter()
            .map(migration_outcome_from_wire)
            .collect(),
        drift: state.drift,
    }
}

struct JitMigrationBackend {
    handle: u64,
}

impl wire::JetDBBackend for JitMigrationBackend {
    fn begin(&mut self) -> bool {
        runtime::jet_db_begin(self.handle)
    }

    fn begin_with_lock(&mut self, lock: wire::JetMigrationLock) -> bool {
        runtime::jet_db_begin_mode(
            self.handle,
            if matches!(lock, wire::JetMigrationLock::Exclusive) {
                1
            } else {
                0
            },
        )
    }

    fn commit(&mut self) -> bool {
        runtime::jet_db_commit(self.handle)
    }

    fn rollback(&mut self) {
        let _ = runtime::jet_db_rollback(self.handle);
    }

    fn execute(
        &mut self,
        sql: &wire::SQL,
        _allow_schema: bool,
    ) -> Result<i64, wire::DBError> {
        wire::jet_db_decode_execute_result(&runtime::jet_db_execute(
            self.handle,
            &sql.0,
            &wire::jet_db_encode_params(&sql.1),
        ))
    }

    fn query(
        &mut self,
        sql: &wire::SQL,
        _allow_schema: bool,
    ) -> Result<Vec<wire::JetDBRow>, wire::DBError> {
        wire::jet_db_decode_query_result(&runtime::jet_db_query(
            self.handle,
            &sql.0,
            &wire::jet_db_encode_params(&sql.1),
        ))
    }
}

fn run_wire_migration(
    handle: u64,
    request: &MigrationRequest,
) -> Result<MigrationOutcome, String> {
    let wire_request = wire_migration_request(request);
    let mut backend = JitMigrationBackend { handle };
    wire::jet_db_migration_request(&mut backend, &wire_request)
        .map(migration_outcome_from_wire)
        .map_err(|error| error.message)
}

fn inspect_wire_migrations(
    handle: u64,
    query: &MigrationStateRequest,
) -> Result<MigrationState, String> {
    let wire_query = wire::JetMigrationStateRequest {
        target: query.target.clone(),
        target_identity: query.target_identity.clone(),
        database_identity: query.database_identity.clone(),
        source_identity: query.source_identity.clone(),
        schema_identity: query.schema_identity.clone(),
    };
    let mut backend = JitMigrationBackend { handle };
    wire::jet_db_migration_state(&mut backend, &wire_query)
        .map(migration_state_from_wire)
        .map_err(|error| error.message)
}

pub fn migration_request_checksum(request: &MigrationRequest) -> String {
    wire::jet_db_migration_request_checksum(&wire_migration_request(request))
}

pub fn run_migration(
    path: impl AsRef<std::path::Path>,
    request: MigrationRequest,
) -> Result<MigrationOutcome, String> {
    let connection = ConsoleDbConnection::open(path)?;
    connection.run_migration(request)
}

pub fn inspect_migrations(
    path: impl AsRef<std::path::Path>,
    query: MigrationStateRequest,
) -> Result<MigrationState, String> {
    let connection = ConsoleDbConnection::open(path)?;
    connection.inspect_migrations(query)
}

use jet_codegen::Comptime::ServicesLite::{
    jet_job_queue_install_store_provider_if_absent, JetJobQueueRow, JetJobQueueStore,
    JetJobQueueStoreProvider, JetJobQueueValue, JetServiceError,
};

struct JitJobQueueStore {
    handle: u64,
}

impl Drop for JitJobQueueStore {
    fn drop(&mut self) {
        let _ = runtime::jet_db_close(self.handle);
    }
}

fn jit_job_queue_error(operation: &str, error: wire::DBError) -> JetServiceError {
    JetServiceError::Unavailable(format!("queue SQLite {operation} failed: {}", error.message))
}

fn jit_job_queue_values(values: &[JetJobQueueValue]) -> Vec<wire::DBValue> {
    values
        .iter()
        .map(|value| match value {
            JetJobQueueValue::Null => wire::DBValue::Null,
            JetJobQueueValue::Int(value) => wire::DBValue::Int(*value),
            JetJobQueueValue::Text(value) => wire::DBValue::Text(value.clone()),
            JetJobQueueValue::Blob(value) => wire::DBValue::Blob(value.clone()),
        })
        .collect()
}

impl JetJobQueueStore for JitJobQueueStore {
    fn begin(&mut self) -> Result<(), JetServiceError> {
        if runtime::jet_db_begin(self.handle) {
            Ok(())
        } else {
            Err(JetServiceError::Unavailable(
                "queue SQLite begin failed".to_string(),
            ))
        }
    }

    fn commit(&mut self) -> Result<(), JetServiceError> {
        if runtime::jet_db_commit(self.handle) {
            Ok(())
        } else {
            Err(JetServiceError::Unavailable(
                "queue SQLite commit failed".to_string(),
            ))
        }
    }

    fn rollback(&mut self) {
        let _ = runtime::jet_db_rollback(self.handle);
    }

    fn execute(
        &mut self,
        sql: &str,
        params: &[JetJobQueueValue],
    ) -> Result<i64, JetServiceError> {
        let params = wire::jet_db_encode_params(&jit_job_queue_values(params));
        wire::jet_db_decode_execute_result(&runtime::jet_db_execute(
            self.handle,
            sql,
            &params,
        ))
        .map_err(|error| jit_job_queue_error("execute", error))
    }

    fn query(
        &mut self,
        sql: &str,
        params: &[JetJobQueueValue],
    ) -> Result<Vec<JetJobQueueRow>, JetServiceError> {
        let params = wire::jet_db_encode_params(&jit_job_queue_values(params));
        let rows = wire::jet_db_decode_query_result(&runtime::jet_db_query(
            self.handle,
            sql,
            &params,
        ))
        .map_err(|error| jit_job_queue_error("query", error))?;
        Ok(rows
            .into_iter()
            .map(|row| {
                JetJobQueueRow::new(
                    row.into_iter()
                        .map(|(name, value)| {
                            let value = match value {
                                wire::DBValue::Null => JetJobQueueValue::Null,
                                wire::DBValue::Int(value) => JetJobQueueValue::Int(value),
                                wire::DBValue::Float(value) => {
                                    JetJobQueueValue::Text(value.to_string())
                                }
                                wire::DBValue::Text(value) => JetJobQueueValue::Text(value),
                                wire::DBValue::Bool(value) => {
                                    JetJobQueueValue::Int(i64::from(value))
                                }
                                wire::DBValue::Blob(value) => JetJobQueueValue::Blob(value),
                            };
                            (name, value)
                        })
                        .collect(),
                )
            })
            .collect())
    }
}

struct JitJobQueueProvider;

impl JetJobQueueStoreProvider for JitJobQueueProvider {
    fn open(
        &self,
        path: &str,
        _authority: &str,
    ) -> Result<Box<dyn JetJobQueueStore>, JetServiceError> {
        if let Ok(metadata) = std::fs::symlink_metadata(path) {
            if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
                return Err(JetServiceError::Policy(
                    "queue SQLite path must be a regular file, not a symlink".to_string(),
                ));
            }
        }
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                JetServiceError::Unavailable(format!("queue SQLite directory failed: {error}"))
            })?;
        }
        let handle = runtime::jet_db_open(path);
        if handle == 0 {
            return Err(JetServiceError::Unavailable(
                "queue SQLite open failed".to_string(),
            ));
        }
        Ok(Box::new(JitJobQueueStore { handle }))
    }
}

static JIT_JOB_QUEUE_PROVIDER: std::sync::Once = std::sync::Once::new();

pub(crate) fn jit_job_queue_install_provider() {
    JIT_JOB_QUEUE_PROVIDER.call_once(|| {
        let _ = jet_job_queue_install_store_provider_if_absent(Box::new(JitJobQueueProvider));
    });
}

/// Shared typed database facts. The JIT only constructs these facts after the
/// host callback has matched an observed statement and explicit authority.
mod devtools {
    pub use jet_foundation::Devtools::*;
    include!("../../jet-codegen/src/Prelude/Core/DevtoolsDatabasePanel.rs");
}

/// Shared bounded pool state machine. The adapter keeps only native driver
/// handles here; admission, deadlines, health, reset, and draining live in the
/// included Prelude kernel.
pub(crate) mod pool {
    use super::{JetDebug, JetDisplay, JetShow};

    pub(crate) mod jet_std {
        pub(crate) use super::super::wire::DBError;
    }

    fn jet_ctx_deadline_ms() -> Option<i64> {
        jet_codegen::scheduler::jet_ctx_deadline_ms()
    }

    fn jet_std_time_now() -> i64 {
        jet_codegen::scheduler::jet_std_time_now()
    }

    include!("../../jet-codegen/src/Prelude/CoreLib/Top/DbPool.rs");
}

thread_local! {
    /// JIT-local policy capabilities. The token is passed through Cranelift
    /// as the DBScope value; the policy and user never become mutable heap
    /// fields visible to Jet code.
    static DB_SCOPES: std::cell::RefCell<
        HashMap<u64, (u64, String, wire::JetRowPolicyExpr, String, Option<String>)>,
    > = std::cell::RefCell::new(HashMap::new());
}
thread_local! {
    static DB_POOLS: std::cell::RefCell<HashMap<u64, pool::JetDbPool<u64>>> =
        std::cell::RefCell::new(HashMap::new());
    static DB_LEASES: std::cell::RefCell<HashMap<u64, Arc<Mutex<pool::JetDbLease<u64>>>>> =
        std::cell::RefCell::new(HashMap::new());
    /// Direct `core.db` handles are tracked separately from pool drivers. A
    /// console captures these handles without reopening their path.
    static DB_CONNECTIONS: std::cell::RefCell<BTreeSet<u64>> =
        std::cell::RefCell::new(BTreeSet::new());
}

static NEXT_DB_POOL: AtomicU64 = AtomicU64::new(2_000_000_000);
static NEXT_DB_LEASE: AtomicU64 = AtomicU64::new(3_000_000_000);

static NEXT_DB_SCOPE: AtomicU64 = AtomicU64::new(1_000_000_000);

fn scope_parts(
    handle: u64,
) -> Option<(u64, String, wire::JetRowPolicyExpr, String, Option<String>)> {
    DB_SCOPES.with(|scopes| scopes.borrow().get(&handle).cloned())
}

fn base_handle(handle: u64) -> u64 {
    scope_parts(handle)
        .map(|(base, _, _, _, _)| base)
        .unwrap_or(handle)
}

fn new_scope(
    connection: u64,
    table: String,
    compiled: wire::JetRowPolicyExpr,
    user: String,
) -> i64 {
    jit_job_queue_install_provider();
    if connection == 0 {
        return 0;
    }
    let id = NEXT_DB_SCOPE.fetch_add(1, Ordering::Relaxed);
    let base = base_handle(connection);
    let request_id = crate::net_http_rt::jet_db_current_request_id();
    DB_SCOPES.with(|scopes| {
        scopes
            .borrow_mut()
            .insert(id, (base, table, compiled, user, request_id));
    });
    id as i64
}

/// Close a direct connection, or a policy scope and the connection it owns.
/// Shared by the Cranelift symbol and the interpreter adapter.
fn close_db_handle(handle: u64) -> bool {
    if scope_parts(handle).is_some() {
        let base = base_handle(handle);
        DB_SCOPES.with(|scopes| {
            scopes.borrow_mut().remove(&handle);
        });
        runtime_close(base)
    } else {
        runtime_close(handle)
    }
}

fn scoped_execute(scope: u64, sql: &wire::SQL, allow_schema: bool) -> Result<i64, wire::DBError> {
    let Some((base, table, compiled, user, _)) = scope_parts(scope) else {
        return Err(wire::DBError {
            message: "database row operations require a policy scope".to_string(),
        });
    };
    let sql = if allow_schema {
        wire::jet_db_apply_compiled_migration_policy_with_proof(sql, &table, compiled, &user)?
            .into_sql()?
    } else {
        wire::jet_db_apply_compiled_policy_with_proof(sql, &table, compiled, &user)?.into_sql()?
    };
    let result = runtime::jet_db_execute(base, &sql.0, &wire::jet_db_encode_params(&sql.1));
    wire::jet_db_decode_execute_result(&result)
}

fn scoped_query(
    scope: u64,
    sql: &wire::SQL,
    allow_schema: bool,
) -> Result<Vec<wire::JetDBRow>, wire::DBError> {
    let Some((base, table, compiled, user, _)) = scope_parts(scope) else {
        return Err(wire::DBError {
            message: "database row operations require a policy scope".to_string(),
        });
    };
    let sql = if allow_schema {
        wire::jet_db_apply_compiled_migration_policy_with_proof(sql, &table, compiled, &user)?
            .into_sql()?
    } else {
        wire::jet_db_apply_compiled_policy_with_proof(sql, &table, compiled, &user)?.into_sql()?
    };
    let result = runtime::jet_db_query(base, &sql.0, &wire::jet_db_encode_params(&sql.1));
    wire::jet_db_decode_query_result(&result)
}

struct JitDbBackend {
    scope: u64,
}

impl wire::JetDBBackend for JitDbBackend {
    fn begin(&mut self) -> bool {
        runtime::jet_db_begin(base_handle(self.scope))
    }

    fn begin_with_lock(&mut self, lock: wire::JetMigrationLock) -> bool {
        runtime::jet_db_begin_mode(
            base_handle(self.scope),
            if matches!(lock, wire::JetMigrationLock::Exclusive) {
                1
            } else {
                0
            },
        )
    }

    fn commit(&mut self) -> bool {
        runtime::jet_db_commit(base_handle(self.scope))
    }

    fn rollback(&mut self) {
        let _ = runtime::jet_db_rollback(base_handle(self.scope));
    }

    fn execute(&mut self, sql: &wire::SQL, allow_schema: bool) -> Result<i64, wire::DBError> {
        scoped_execute(self.scope, sql, allow_schema)
    }

    fn query(
        &mut self,
        sql: &wire::SQL,
        allow_schema: bool,
    ) -> Result<Vec<wire::JetDBRow>, wire::DBError> {
        scoped_query(self.scope, sql, allow_schema)
    }
}
fn run_scoped_migration(
    scope: u64,
    name: &String,
    steps: &Vec<wire::SQL>,
) -> Result<i64, wire::DBError> {
    if scope_parts(scope).is_none() {
        return Err(wire::DBError {
            message: "database migration requires a policy scope".to_string(),
        });
    }
    let mut backend = JitDbBackend { scope };
    wire::jet_db_migrate(&mut backend, name, steps)
}

fn run_scoped_transaction(
    scope: u64,
    label: &String,
    steps: &Vec<wire::SQL>,
) -> Result<i64, wire::DBError> {
    if scope_parts(scope).is_none() {
        return Err(wire::DBError {
            message: "database transaction requires a policy scope".to_string(),
        });
    }
    let mut backend = JitDbBackend { scope };
    wire::jet_db_transaction(&mut backend, label, steps)
}

fn invalidate_scoped_writes(steps: &Vec<wire::SQL>) {
    let write_set = wire::jet_db_write_set(steps);
    if !write_set.is_empty() {
        let _ = crate::Reactive::jet_jit_app_transact_invalidate_text(write_set);
    }
}

fn db_pool_hooks() -> pool::JetDbPoolHooks<u64> {
    pool::JetDbPoolHooks::new(
        |path| {
            // Pool drivers belong to the canonical pool lease. They are not
            // direct console connections and must not be captured twice.
            let handle = runtime_open_untracked(path);
            if handle == 0 {
                Err(pool::jet_std::DBError {
                    message: "database pool backend open failed".to_string(),
                })
            } else {
                Ok(handle)
            }
        },
        |handle| runtime::jet_db_health(*handle),
        |handle| runtime::jet_db_reset(*handle),
        |handle| {
            let _ = runtime_close_untracked(handle);
        },
    )
}

fn pool_from_id(handle: u64) -> Option<pool::JetDbPool<u64>> {
    DB_POOLS.with(|pools| pools.borrow().get(&handle).cloned())
}

fn pool_diag(message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0956",
        message.into(),
        "the interpreter ambient database adapter rejected the pool operation".to_string(),
        "report this as a compiler bug".to_string(),
        Some(span),
    )
}
fn db_diag(message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0956",
        message.into(),
        "the interpreter ambient database adapter rejected the operation".to_string(),
        "report this as a compiler bug".to_string(),
        Some(span),
    )
}

fn pool_lifecycle_name(lifecycle: pool::JetDbPoolLifecycle) -> &'static str {
    match lifecycle {
        pool::JetDbPoolLifecycle::Open => "open",
        pool::JetDbPoolLifecycle::Draining => "draining",
        pool::JetDbPoolLifecycle::Drained => "drained",
    }
}

fn pool_value(handle: u64) -> CtValue {
    CtValue::Struct {
        type_name: "DBPool".to_string(),
        fields: vec![("handle".to_string(), CtValue::Int(handle as i64))],
    }
}

fn lease_value(handle: u64) -> CtValue {
    CtValue::Struct {
        type_name: "DBLease".to_string(),
        fields: vec![("handle".to_string(), CtValue::Int(handle as i64))],
    }
}

fn receipt_value(receipt: &pool::JetDbPoolReceipt) -> CtValue {
    CtValue::Struct {
        type_name: "DBPoolReceipt".to_string(),
        fields: vec![
            (
                "lifecycle".to_string(),
                CtValue::Str(pool_lifecycle_name(receipt.lifecycle).to_string()),
            ),
            ("max".to_string(), CtValue::Int(receipt.max)),
            ("available".to_string(), CtValue::Int(receipt.available)),
            ("leased".to_string(), CtValue::Int(receipt.leased)),
            ("opening".to_string(), CtValue::Int(receipt.opening)),
            ("ready".to_string(), CtValue::Bool(receipt.ready)),
            ("acquires".to_string(), CtValue::Int(receipt.acquires)),
            ("releases".to_string(), CtValue::Int(receipt.releases)),
            ("timeouts".to_string(), CtValue::Int(receipt.timeouts)),
            ("open_failures".to_string(), CtValue::Int(receipt.open_failures)),
            ("unhealthy".to_string(), CtValue::Int(receipt.unhealthy)),
            ("replacements".to_string(), CtValue::Int(receipt.replacements)),
            (
                "replacement_failures".to_string(),
                CtValue::Int(receipt.replacement_failures),
            ),
        ],
    }
}

fn db_error_value(message: String) -> CtValue {
    CtValue::Struct {
        type_name: "DBError".to_string(),
        fields: vec![("message".to_string(), CtValue::Str(message))],
    }
}

fn pool_id(value: &CtValue, expected: &str, span: Span) -> Result<u64, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(pool_diag(format!("expected a {expected} handle"), span));
    };
    if type_name != expected {
        return Err(pool_diag(format!("expected a {expected} handle"), span));
    }
    fields
        .iter()
        .find_map(|(name, value)| {
            (name == "handle").then_some(value).and_then(|value| match value {
                CtValue::Int(handle) if *handle > 0 => Some(*handle as u64),
                _ => None,
            })
        })
        .ok_or_else(|| pool_diag(format!("malformed {expected} handle"), span))
}
fn ambient_db_field<'a>(value: &'a CtValue, name: &str) -> Option<&'a CtValue> {
    match value {
        CtValue::Struct { fields, .. } => fields
            .iter()
            .find_map(|(field, value)| (field == name).then_some(value)),
        _ => None,
    }
}

fn ambient_db_value_payload<'a>(
    variant: &str,
    args: &'a [(Option<String>, CtValue)],
    span: Span,
) -> Result<&'a CtValue, Diagnostic> {
    if args.len() == 1 {
        Ok(&args[0].1)
    } else {
        Err(db_diag(
            format!("database value `{variant}` requires one payload"),
            span,
        ))
    }
}

fn ambient_db_value(value: &CtValue, span: Span) -> Result<wire::DBValue, Diagnostic> {
    let CtValue::Enum {
        type_name,
        variant,
        args,
    } = value
    else {
        return Err(db_diag("database SQL parameter is not a DBValue", span));
    };
    if type_name != "DBValue" {
        return Err(db_diag("database SQL parameter is not a DBValue", span));
    }
    match variant.as_str() {
        "Null" if args.is_empty() => Ok(wire::DBValue::Null),
        "Null" => Err(db_diag("database value `Null` takes no payload", span)),
        "Int" => match ambient_db_value_payload(variant, args, span)? {
            CtValue::Int(value) => Ok(wire::DBValue::Int(*value)),
            _ => Err(db_diag("database value `Int` payload is not an Int", span)),
        },
        "Float" => match ambient_db_value_payload(variant, args, span)? {
            CtValue::Float(value) => Ok(wire::DBValue::Float(value.as_f64())),
            _ => Err(db_diag(
                "database value `Float` payload is not a Float",
                span,
            )),
        },
        "Text" => match ambient_db_value_payload(variant, args, span)? {
            CtValue::Str(value) => Ok(wire::DBValue::Text(value.clone())),
            _ => Err(db_diag(
                "database value `Text` payload is not a String",
                span,
            )),
        },
        "Bool" => match ambient_db_value_payload(variant, args, span)? {
            CtValue::Bool(value) => Ok(wire::DBValue::Bool(*value)),
            _ => Err(db_diag(
                "database value `Bool` payload is not a Bool",
                span,
            )),
        },
        "Blob" => match ambient_db_value_payload(variant, args, span)? {
            CtValue::Bytes(value) => Ok(wire::DBValue::Blob(value.clone())),
            CtValue::List(values) => values
                .iter()
                .map(|value| match value {
                    CtValue::Int(value) if (0..=255).contains(value) => Ok(*value as u8),
                    _ => Err(db_diag(
                        "database value `Blob` payload contains a non-byte value",
                        span,
                    )),
                })
                .collect::<Result<Vec<_>, Diagnostic>>()
                .map(wire::DBValue::Blob),
            _ => Err(db_diag(
                "database value `Blob` payload is not a byte list",
                span,
            )),
        },
        _ => Err(db_diag(
            format!("unknown database value variant `{variant}`"),
            span,
        )),
    }
}

fn ambient_sql_value(value: &CtValue, span: Span) -> Result<wire::SQL, Diagnostic> {
    let CtValue::Struct { type_name, .. } = value else {
        return Err(db_diag("database migration step is not SQL", span));
    };
    if type_name != "SQL" {
        return Err(db_diag("database migration step is not SQL", span));
    }
    let template = match ambient_db_field(value, "template") {
        Some(CtValue::Str(template)) => template.clone(),
        _ => return Err(db_diag("database SQL value has no text template", span)),
    };
    let params = match ambient_db_field(value, "params") {
        Some(CtValue::List(params)) => params
            .iter()
            .map(|value| ambient_db_value(value, span))
            .collect::<Result<Vec<_>, Diagnostic>>()?,
        _ => return Err(db_diag("database SQL value has no parameter list", span)),
    };
    Ok((template, params))
}

fn ambient_sql_list(value: &CtValue, span: Span) -> Result<Vec<wire::SQL>, Diagnostic> {
    let CtValue::List(values) = value else {
        return Err(db_diag("database migration steps must be a list of SQL values", span));
    };
    values
        .iter()
        .map(|value| ambient_sql_value(value, span))
        .collect()
}

fn duration_deadline(value: &CtValue, span: Span) -> Result<i64, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(pool_diag(
            "database pool acquire deadline must be a Duration",
            span,
        ));
    };
    if type_name != "Duration" {
        return Err(pool_diag(
            "database pool acquire deadline must be a Duration",
            span,
        ));
    }
    let ns = fields.iter().find_map(|(name, value)| {
        (name == "ns").then_some(value).and_then(|value| match value {
            CtValue::Int(ns) => Some(*ns),
            _ => None,
        })
    });
    ns.map(|ns| {
        jet_codegen::scheduler::jet_std_time_now()
            .saturating_add(ns.saturating_div(1_000_000))
    })
    .ok_or_else(|| pool_diag("malformed database pool deadline", span))
}

fn pool_new(url: String, max: i64) -> Result<u64, String> {
    let pool = pool::JetDbPool::new(url, max, db_pool_hooks())
        .map_err(|error| error.message)?;
    let id = NEXT_DB_POOL.fetch_add(1, Ordering::Relaxed);
    DB_POOLS.with(|pools| {
        pools.borrow_mut().insert(id, pool);
    });
    Ok(id)
}

fn pool_acquire(handle: u64, deadline: Option<i64>) -> Result<u64, String> {
    let pool = pool_from_id(handle).ok_or_else(|| "unknown database pool handle".to_string())?;
    let lease = pool.acquire(deadline).map_err(|error| error.message)?;
    let id = NEXT_DB_LEASE.fetch_add(1, Ordering::Relaxed);
    DB_LEASES.with(|leases| {
        leases
            .borrow_mut()
            .insert(id, Arc::new(Mutex::new(lease)));
    });
    Ok(id)
}
fn pool_lease_close(handle: u64) {
    DB_LEASES.with(|leases| {
        let _ = leases.borrow_mut().remove(&handle);
    });
}

fn pool_ready(handle: u64) -> Result<bool, String> {
    pool_from_id(handle)
        .ok_or_else(|| "unknown database pool handle".to_string())?
        .ready()
        .map_err(|error| error.message)
}

fn pool_drain(handle: u64, deadline: Option<i64>) -> Result<pool::JetDbPoolReceipt, String> {
    pool_from_id(handle)
        .ok_or_else(|| "unknown database pool handle".to_string())?
        .drain(deadline)
        .map_err(|error| error.message)
}

fn pool_receipt(handle: u64) -> Result<pool::JetDbPoolReceipt, String> {
    pool_from_id(handle)
        .ok_or_else(|| "unknown database pool handle".to_string())
        .map(|pool| pool.receipt())
}

fn pool_handle_operation(
    operation: &str,
    receiver: &CtValue,
    deadline: Option<i64>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let handle = pool_id(receiver, "DBPool", span)?;
    match operation {
        "db_pool.acquire" | "db_pool.acquire_deadline" => match pool_acquire(handle, deadline) {
            Ok(lease) => Ok(CtValue::Present(Box::new(lease_value(lease)))),
            Err(error) => Ok(CtValue::failed(Box::new(db_error_value(error)))),
        },
        "db_pool.ready" => match pool_ready(handle) {
            Ok(ready) => Ok(CtValue::Present(Box::new(CtValue::Bool(ready)))),
            Err(error) => Ok(CtValue::failed(Box::new(db_error_value(error)))),
        },
        "db_pool.drain" => match pool_drain(handle, deadline) {
            Ok(receipt) => Ok(CtValue::Present(Box::new(receipt_value(&receipt)))),
            Err(error) => Ok(CtValue::failed(Box::new(db_error_value(error)))),
        },
        "db_pool.receipt" => pool_receipt(handle)
            .map(|receipt| receipt_value(&receipt))
            .map_err(|error| pool_diag(error, span)),
        _ => Err(pool_diag(
            format!("unknown database pool operation `{operation}`"),
            span,
        )),
    }
}

pub(crate) fn ambient_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    _resolved_ret: Option<Type>,
    _sink: Option<&mut jet_codegen::Comptime::DevSink>,
) -> Option<Result<CtValue, Diagnostic>> {
    // JetEval delegates these Ambient rows through its existing host adapter.
    // This boundary only marshals the existing SQLite bridge/wire carriers.
    if module == "core.db" {
        let raw = match (method, args.as_slice()) {
            ("__queue_raw_open", [CtValue::Str(path)]) => {
                Some(Ok(CtValue::Int(runtime::jet_db_open(path) as i64)))
            }
            ("__queue_raw_close", [CtValue::Int(handle)]) => {
                Some(Ok(CtValue::Bool(runtime::jet_db_close(*handle as u64))))
            }
            ("__queue_raw_begin", [CtValue::Int(handle)]) => {
                Some(Ok(CtValue::Bool(runtime::jet_db_begin(*handle as u64))))
            }
            ("__queue_raw_commit", [CtValue::Int(handle)]) => {
                Some(Ok(CtValue::Bool(runtime::jet_db_commit(*handle as u64))))
            }
            ("__queue_raw_rollback", [CtValue::Int(handle)]) => {
                Some(Ok(CtValue::Bool(runtime::jet_db_rollback(*handle as u64))))
            }
            ("__queue_raw_execute" | "__queue_raw_query", [CtValue::Int(handle), CtValue::Str(sql), CtValue::List(params)]) => {
                Some(params.iter().map(|value| ambient_db_value(value, span))
                    .collect::<Result<Vec<_>, _>>()
                    .map(|params| {
                        let params = wire::jet_db_encode_params(&params);
                        if method == "__queue_raw_execute" {
                            db_outcome(wire::jet_db_decode_execute_result(
                                &runtime::jet_db_execute(*handle as u64, sql, &params)), CtValue::Int)
                        } else {
                            db_outcome(wire::jet_db_decode_query_result(
                                &runtime::jet_db_query(*handle as u64, sql, &params)),
                                |rows| CtValue::List(rows.into_iter().map(db_row_ct).collect()))
                        }
                    }))
            }
            _ => None,
        };
        if raw.is_some() {
            return raw;
        }
    }
    // Reserved Core source leaves retain this one SQLite host implementation.
    let method = match (module, method) {
        ("core.db", "__open") => "open",
        ("core.db", "__open_memory") => "open_memory",
        ("core.db", "__pool") => "pool",
        ("core.db", "__policy") => "policy",
        ("core.db", "__policy_audit") => "policy_audit",
        ("core.db", "__transaction") => "transaction",
        ("core.db", "__migrate") => "migrate",
        _ => method,
    };
    if module == "core.db" && (method == "migrate" || method == "transaction") {
        let [scope, CtValue::Str(label), steps] = args.as_slice() else {
            return Some(Err(db_diag(
                format!("core.db.{method} received malformed arguments"),
                span,
            )));
        };
        let scope = match pool_id(scope, "DBScope", span) {
            Ok(scope) => scope,
            Err(error) => return Some(Err(error)),
        };
        let steps = match ambient_sql_list(steps, span) {
            Ok(steps) => steps,
            Err(error) => return Some(Err(error)),
        };
        let result = if method == "migrate" {
            run_scoped_migration(scope, label, &steps)
        } else {
            run_scoped_transaction(scope, label, &steps)
        };
        return Some(match result {
            Ok(done) => {
                if method == "migrate" {
                    if done > 0 {
                        invalidate_scoped_writes(&steps);
                    }
                } else {
                    invalidate_scoped_writes(&steps);
                }
                Ok(CtValue::Present(Box::new(CtValue::Int(done))))
            }
            Err(error) => Ok(CtValue::failed(Box::new(db_error_value(error.message)))),
        });
    }

    if module == "core.db" && method == "pool" {
        let [CtValue::Str(url), CtValue::Int(max)] = args.as_slice() else {
            return Some(Err(pool_diag(
                "core.db.pool received malformed arguments",
                span,
            )));
        };
        return Some(match pool_new(url.clone(), *max) {
            Ok(handle) => Ok(CtValue::Present(Box::new(pool_value(handle)))),
            Err(error) => Ok(CtValue::failed(Box::new(db_error_value(error)))),
        });
    }
    if module == "core.handle" && method == "db_lease.close" {
        let [receiver] = args.as_slice() else {
            return Some(Err(pool_diag(
                "database lease close received malformed arguments",
                span,
            )));
        };
        let handle = match pool_id(receiver, "DBLease", span) {
            Ok(handle) => handle,
            Err(error) => return Some(Err(error)),
        };
        pool_lease_close(handle);
        // `DBLease.close()` is `Unit DBError!`: success is the Ok carrier.
        return Some(Ok(CtValue::Present(Box::new(CtValue::Unit))));
    }

    if module == "core.handle" && method.starts_with("db_pool.") {
        let deadline = match method {
            "db_pool.acquire_deadline" => args
                .get(1)
                .ok_or_else(|| pool_diag("database pool acquire deadline is missing", span))
                .and_then(|value| duration_deadline(value, span))
                .map(Some),
            _ => Ok(None),
        };
        return Some(match (args.first(), deadline) {
            (Some(receiver), Ok(deadline)) => pool_handle_operation(method, receiver, deadline, span),
            (None, _) => Err(pool_diag("database pool method is missing its receiver", span)),
            (_, Err(error)) => Err(error),
        });
    }
    connection_core_call(module, method, &args, span)
}

fn handle_value(type_name: &str, handle: u64) -> CtValue {
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields: vec![("handle".to_string(), CtValue::Int(handle as i64))],
    }
}

fn is_handle_value(value: &CtValue, expected: &str) -> bool {
    matches!(value, CtValue::Struct { type_name, .. } if type_name == expected)
}

/// A connection or policy scope receiver: both carry one canonical native id.
fn connection_or_scope_id(value: &CtValue, span: Span) -> Result<u64, Diagnostic> {
    match value {
        CtValue::Struct { type_name, .. } if type_name == "DBConnection" || type_name == "DBScope" => {
            pool_id(value, type_name, span)
        }
        _ => Err(db_diag("expected a DBConnection or DBScope handle", span)),
    }
}

fn opened_connection(handle: u64, span: Span) -> Result<CtValue, Diagnostic> {
    if handle == 0 {
        return Err(db_diag("database open failed", span));
    }
    Ok(handle_value("DBConnection", handle))
}

/// `RowPolicy` keeps the table and the canonical spelling of the compiled
/// expression; `with_policy` recompiles it through the one policy language.
fn policy_value(table: &str, compiled: wire::JetRowPolicyExpr) -> CtValue {
    CtValue::Struct {
        type_name: "RowPolicy".to_string(),
        fields: vec![
            ("table".to_string(), CtValue::Str(table.to_string())),
            (
                "expression".to_string(),
                CtValue::Str(compiled.canonical().to_string()),
            ),
        ],
    }
}

fn policy_parts(
    value: &CtValue,
    span: Span,
) -> Result<(String, wire::JetRowPolicyExpr), Diagnostic> {
    let (Some(CtValue::Str(table)), Some(CtValue::Str(expression))) = (
        ambient_db_field(value, "table"),
        ambient_db_field(value, "expression"),
    ) else {
        return Err(db_diag("expected a RowPolicy value", span));
    };
    wire::jet_db_policy_compile(table, expression).map_err(|message| db_diag(message, span))
}

fn db_value_ct(value: wire::DBValue) -> CtValue {
    let (variant, payload) = match value {
        wire::DBValue::Null => ("Null", None),
        wire::DBValue::Int(value) => ("Int", Some(CtValue::Int(value))),
        wire::DBValue::Float(value) => (
            "Float",
            Some(CtValue::Float(jet_foundation::AST::CtFloat::f64(value))),
        ),
        wire::DBValue::Text(value) => ("Text", Some(CtValue::Str(value))),
        wire::DBValue::Bool(value) => ("Bool", Some(CtValue::Bool(value))),
        wire::DBValue::Blob(value) => ("Blob", Some(CtValue::Bytes(value))),
    };
    CtValue::Enum {
        type_name: "DBValue".to_string(),
        variant: variant.to_string(),
        args: payload.into_iter().map(|value| (None, value)).collect(),
    }
}

fn db_row_ct(row: wire::JetDBRow) -> CtValue {
    CtValue::Map(
        row.into_iter()
            .map(|(key, value)| (jet_foundation::AST::CtKey::Str(key), db_value_ct(value)))
            .collect(),
    )
}

fn db_row_from_ct(value: &CtValue, span: Span) -> Result<wire::JetDBRow, Diagnostic> {
    let CtValue::Map(entries) = value else {
        return Err(db_diag("database row is not a map", span));
    };
    entries
        .iter()
        .map(|(key, value)| match key {
            jet_foundation::AST::CtKey::Str(key) => Ok((key.clone(), ambient_db_value(value, span)?)),
            _ => Err(db_diag("database row column name is not a String", span)),
        })
        .collect()
}

fn db_outcome<T>(result: Result<T, wire::DBError>, ok: impl FnOnce(T) -> CtValue) -> CtValue {
    match result {
        Ok(value) => CtValue::Present(Box::new(ok(value))),
        Err(error) => CtValue::failed(Box::new(db_error_value(error.message))),
    }
}

/// `DBValue` accessors report a plain `String` failure (`T String!`).
fn db_value_outcome<T>(result: Result<T, String>, ok: impl FnOnce(T) -> CtValue) -> CtValue {
    match result {
        Ok(value) => CtValue::Present(Box::new(ok(value))),
        Err(message) => CtValue::failed(Box::new(CtValue::Str(message))),
    }
}

/// D-DBDRIVER1 / D-DBPOLICY-BIND1: interpreter connections, policies, scopes,
/// rows and `DBValue` accessors. Handles are the canonical native ids the
/// Cranelift host uses, and SQL rewriting, row decoding and accessor meaning
/// stay in the shared wire kernel.
fn connection_core_call(
    module: &str,
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    let result = match (module, method, args) {
        ("core.db", "open_memory", []) => {
            jit_job_queue_install_provider();
            opened_connection(runtime_open_memory(), span)
        }
        ("core.db", "open", [CtValue::Str(path)]) => {
            jit_job_queue_install_provider();
            opened_connection(runtime_open(path), span)
        }
        ("core.db", "policy", [CtValue::Str(table), CtValue::Str(expression)]) => Ok(db_outcome(
            wire::jet_db_policy_compile(table, expression)
                .map_err(|message| wire::DBError { message }),
            |(table, compiled)| policy_value(&table, compiled),
        )),
        ("core.db", "policy_audit", [scope]) => pool_id(scope, "DBScope", span).and_then(|scope| {
            scope_parts(scope)
                .map(|(_, table, compiled, user, _)| {
                    CtValue::Str(wire::jet_db_policy_audit_line(&table, compiled, &user))
                })
                .ok_or_else(|| db_diag("unknown database scope handle", span))
        }),
        ("core.handle", "db.with_policy", [connection, policy, CtValue::Str(user)])
            if is_handle_value(connection, "DBConnection") =>
        {
            pool_id(connection, "DBConnection", span).and_then(|connection| {
                let (table, compiled) = policy_parts(policy, span)?;
                match new_scope(connection, table, compiled, user.clone()) {
                    0 => Err(db_diag("database policy scope could not be created", span)),
                    scope => Ok(handle_value("DBScope", scope as u64)),
                }
            })
        }
        ("core.handle", "db.begin" | "db.commit" | "db.rollback" | "db.close", [receiver])
            if is_handle_value(receiver, "DBConnection") || is_handle_value(receiver, "DBScope") =>
        {
            connection_or_scope_id(receiver, span).map(|handle| {
                CtValue::Bool(match method {
                    "db.begin" => runtime::jet_db_begin(base_handle(handle)),
                    "db.commit" => runtime::jet_db_commit(base_handle(handle)),
                    "db.rollback" => runtime::jet_db_rollback(base_handle(handle)),
                    _ => close_db_handle(handle),
                })
            })
        }
        ("core.handle", "query" | "query_one" | "execute", [scope, sql, _metadata])
            if is_handle_value(scope, "DBScope") =>
        {
            let scope = match pool_id(scope, "DBScope", span) {
                Ok(scope) => scope,
                Err(error) => return Some(Err(error)),
            };
            ambient_sql_value(sql, span).map(|sql| match method {
                "execute" => db_outcome(scoped_execute(scope, &sql, false), CtValue::Int),
                "query" => db_outcome(scoped_query(scope, &sql, false), |rows| {
                    CtValue::List(rows.into_iter().map(db_row_ct).collect())
                }),
                _ => db_outcome(
                    scoped_query(scope, &sql, false).map(wire::jet_db_first_row),
                    |row| match row {
                        Ok(row) => CtValue::Present(Box::new(db_row_ct(row))),
                        Err(_) => CtValue::absent(Type::Map {
                            key: Box::new(Type::String),
                            key_span: None,
                            value: Box::new(Type::Named("DBValue".to_string())),
                        }),
                    },
                ),
            })
        }
        (
            "core.encoding.dyn",
            "int" | "float" | "text" | "bool" | "blob" | "is_null",
            [value @ CtValue::Enum { type_name, .. }],
        ) if type_name == "DBValue" => ambient_db_value(value, span).map(|value| match method {
            "int" => db_value_outcome(value.int(), CtValue::Int),
            "float" => db_value_outcome(value.float(), |value| {
                CtValue::Float(jet_foundation::AST::CtFloat::f64(value))
            }),
            "text" => db_value_outcome(value.text(), CtValue::Str),
            "bool" => db_value_outcome(value.bool(), CtValue::Bool),
            "blob" => db_value_outcome(value.blob(), CtValue::Bytes),
            _ => CtValue::Bool(value.is_null()),
        }),
        _ => return None,
    };
    Some(result)
}

pub(crate) fn ambient_handle(
    operation: &str,
    receiver: &mut CtValue,
    args: &mut [CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    if operation == "db_lease.close" {
        if !args.is_empty() {
            return Some(Err(pool_diag(
                "database lease close received unexpected arguments",
                span,
            )));
        }
        let handle = match pool_id(receiver, "DBLease", span) {
            Ok(handle) => handle,
            Err(error) => return Some(Err(error)),
        };
        pool_lease_close(handle);
        return Some(Ok(CtValue::Present(Box::new(CtValue::Unit))));
    }

    let deadline = match operation {
        "db_pool.acquire_deadline" => args
            .first()
            .ok_or_else(|| pool_diag("database pool acquire deadline is missing", span))
            .and_then(|value| duration_deadline(value, span))
            .map(Some),
        _ => Ok(None),
    };
    operation
        .strip_prefix("db_pool.")
        .map(|_| match deadline {
            Ok(deadline) => pool_handle_operation(operation, receiver, deadline, span),
            Err(error) => Err(error),
        })
}

pub(crate) fn register_interpreter_ambient(
    context: &mut crate::InterpreterAmbientContext,
) {
    context.register_core_call(ambient_core_call);
    context.register_handle(ambient_handle);
    jit_job_queue_install_provider();
}

fn track_connection(handle: u64) {
    if handle != 0 {
        DB_CONNECTIONS.with(|connections| {
            connections.borrow_mut().insert(handle);
        });
    }
}

fn untrack_connection(handle: u64) {
    DB_CONNECTIONS.with(|connections| {
        connections.borrow_mut().remove(&handle);
    });
}

fn runtime_open_memory_untracked() -> u64 {
    runtime::jet_db_open_memory()
}

fn runtime_open_untracked(path: &str) -> u64 {
    runtime::jet_db_open(path)
}

/// Release every native database owner a run created, while the native
/// connection table is still alive. Leases go first so their drivers return
/// to their pools; dropping the pools then closes the pooled drivers, and the
/// remaining direct handles close last. Left to thread-local destruction, a
/// pool could close its drivers after the connection table was destroyed.
pub(crate) fn release_db_resources() {
    DB_SCOPES.with(|scopes| scopes.borrow_mut().clear());
    DB_LEASES.with(|leases| {
        let _ = std::mem::take(&mut *leases.borrow_mut());
    });
    DB_POOLS.with(|pools| {
        let _ = std::mem::take(&mut *pools.borrow_mut());
    });
    let connections = DB_CONNECTIONS.with(|connections| {
        std::mem::take(&mut *connections.borrow_mut())
    });
    for handle in connections {
        let _ = runtime::jet_db_close(handle);
    }
}

/// Open in-memory SQLite (interpreter ambient host).
pub(crate) fn runtime_open_memory() -> u64 {
    let handle = runtime_open_memory_untracked();
    track_connection(handle);
    handle
}

/// Open SQLite file (interpreter ambient host).
pub(crate) fn runtime_open(path: &str) -> u64 {
    let handle = runtime_open_untracked(path);
    track_connection(handle);
    handle
}

pub(crate) fn runtime_close(handle: u64) -> bool {
    untrack_connection(handle);
    runtime::jet_db_close(handle)
}

fn runtime_close_untracked(handle: u64) -> bool {
    runtime::jet_db_close(handle)
}

pub(crate) fn runtime_begin(handle: u64) -> bool {
    runtime::jet_db_begin(handle)
}

pub(crate) fn runtime_commit(handle: u64) -> bool {
    runtime::jet_db_commit(handle)
}

pub(crate) fn runtime_rollback(handle: u64) -> bool {
    runtime::jet_db_rollback(handle)
}

pub(crate) fn runtime_execute(handle: u64, sql: &str, params_wire: &str) -> String {
    runtime::jet_db_execute(handle, sql, params_wire)
}

pub(crate) fn runtime_query(handle: u64, sql: &str, params_wire: &str) -> String {
    runtime::jet_db_query(handle, sql, params_wire)
}
/// Typed SQLite value used by the console host bridge.
#[derive(Clone, Debug, PartialEq)]
pub enum ConsoleDbValue {
    Null,
    Int(i64),
    Float(f64),
    Bool(bool),
    Text(String),
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConsoleDbQueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<ConsoleDbValue>>,
    pub truncated: bool,
}

/// A database execution target is either a direct application handle or an
/// existing canonical pool lease.  The lease variant shares ownership without
/// moving the lease out of the JIT's handle table.
#[derive(Clone)]
enum DbExecution {
    Owned(u64),
    Lease(Arc<Mutex<pool::JetDbLease<u64>>>),
}

fn with_execution<R, F>(execution: &DbExecution, operation: F) -> Result<R, String>
where
    F: FnOnce(u64) -> Result<R, String>,
{
    match execution {
        DbExecution::Owned(handle) => operation(*handle),
        DbExecution::Lease(lease) => {
            let mut lease = lease
                .lock()
                .map_err(|_| "database pool lease lock is poisoned".to_string())?;
            lease
                .with_driver(|driver| {
                    operation(*driver).map_err(|message| pool::jet_std::DBError { message })
                })
                .map_err(|error| error.message)
        }
    }
}

fn decode_console_query(
    wire_result: &str,
    limit: usize,
) -> Result<ConsoleDbQueryResult, String> {
    let rows = wire::jet_db_decode_query_result(wire_result).map_err(|error| error.message)?;
    let mut columns = BTreeSet::new();
    for row in &rows {
        columns.extend(row.keys().cloned());
    }
    let columns = columns.into_iter().collect::<Vec<_>>();
    let mut rows = rows
        .into_iter()
        .map(|row| {
            columns
                .iter()
                .map(|name| {
                    row.get(name)
                        .map(console_db_value)
                        .unwrap_or(ConsoleDbValue::Null)
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let truncated = rows.len() > limit;
    rows.truncate(limit);
    Ok(ConsoleDbQueryResult {
        columns,
        rows,
        truncated,
    })
}

/// A live handle to the canonical JIT SQLite runtime.  The handle is
/// deliberately opaque; callers cannot bypass the typed query/transaction
/// boundary to reach the runtime's connection map.
pub struct ConsoleDbConnection {
    execution: DbExecution,
}

impl ConsoleDbConnection {
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self, String> {
        let path = path
            .as_ref()
            .to_str()
            .ok_or_else(|| "database path is not valid UTF-8".to_string())?;
        let handle = runtime_open(path);
        (handle != 0)
            .then_some(Self {
                execution: DbExecution::Owned(handle),
            })
            .ok_or_else(|| "database connection could not be opened".to_string())
    }

    pub fn query(
        &self,
        sql: &str,
        values: &[ConsoleDbValue],
        limit: usize,
    ) -> Result<ConsoleDbQueryResult, String> {
        if limit == 0 {
            return Err("database query limit must be positive".to_string());
        }
        let params = wire::jet_db_encode_params(&console_db_values(values));
        let result = with_execution(&self.execution, |handle| {
            Ok(runtime_query(handle, sql, &params))
        })?;
        decode_console_query(&result, limit)
    }

    pub fn begin(&self, transaction_id: impl Into<String>) -> Result<ConsoleDbTransaction, String> {
        with_execution(&self.execution, |handle| {
            if runtime_begin(handle) {
                Ok(())
            } else {
                Err("database transaction could not be opened".to_string())
            }
        })?;
        Ok(ConsoleDbTransaction {
            execution: self.execution.clone(),
            transaction_id: transaction_id.into(),
            writes: 0,
            finished: false,
        })
    }

    pub fn run_migration(
        &self,
        request: MigrationRequest,
    ) -> Result<MigrationOutcome, String> {
        with_execution(&self.execution, |handle| run_wire_migration(handle, &request))
    }

    pub fn inspect_migrations(
        &self,
        query: MigrationStateRequest,
    ) -> Result<MigrationState, String> {
        with_execution(&self.execution, |handle| inspect_wire_migrations(handle, &query))
    }
}

impl Drop for ConsoleDbConnection {
    fn drop(&mut self) {
        if let DbExecution::Owned(handle) = &self.execution {
            let _ = runtime_close(*handle);
        }
    }
}

pub struct ConsoleDbTransaction {
    execution: DbExecution,
    transaction_id: String,
    writes: u64,
    finished: bool,
}

impl ConsoleDbTransaction {
    pub fn transaction_id(&self) -> &str {
        &self.transaction_id
    }

    pub fn savepoint(&mut self, name: &str) -> Result<(), String> {
        self.run_control("SAVEPOINT", name)
    }

    pub fn rollback_to(&mut self, name: &str) -> Result<(), String> {
        self.run_control("ROLLBACK TO SAVEPOINT", name)
    }

    pub fn execute(
        &mut self,
        sql: &str,
        values: &[ConsoleDbValue],
    ) -> Result<u64, String> {
        if self.finished {
            return Err("database transaction is already finished".to_string());
        }
        let params = wire::jet_db_encode_params(&console_db_values(values));
        let affected = with_execution(&self.execution, |handle| {
            wire::jet_db_decode_execute_result(&runtime_execute(handle, sql, &params))
                .map_err(|error| error.message)
        })?;
        let affected = u64::try_from(affected)
            .map_err(|_| "database affected-row count is outside the console range".to_string())?;
        self.writes = self.writes.saturating_add(affected);
        Ok(affected)
    }

    pub fn commit(&mut self) -> Result<(), String> {
        if self.finished {
            return Err("database transaction is already finished".to_string());
        }
        if with_execution(&self.execution, |handle| Ok(runtime_commit(handle)))? {
            self.finished = true;
            Ok(())
        } else {
            Err("database transaction commit failed".to_string())
        }
    }

    pub fn rollback(&mut self) -> Result<(), String> {
        if self.finished {
            return Ok(());
        }
        if with_execution(&self.execution, |handle| Ok(runtime_rollback(handle)))? {
            self.finished = true;
            Ok(())
        } else {
            Err("database transaction rollback failed".to_string())
        }
    }

    pub fn writes(&self) -> u64 {
        self.writes
    }

    fn run_control(&mut self, operation: &str, name: &str) -> Result<(), String> {
        if self.finished {
            return Err("database transaction is already finished".to_string());
        }
        if name.is_empty() || name.chars().any(|character| character.is_control()) {
            return Err("database savepoint name is invalid".to_string());
        }
        let quoted = format!("\"{}\"", name.replace('\"', "\"\""));
        let sql = format!("{operation} {quoted}");
        let params = wire::jet_db_encode_params(&Vec::new());
        with_execution(&self.execution, |handle| {
            wire::jet_db_decode_execute_result(&runtime_execute(handle, &sql, &params))
                .map(|_| ())
                .map_err(|error| error.message)
        })
    }
}

impl Drop for ConsoleDbTransaction {
    fn drop(&mut self) {
        if !self.finished {
            let _ = with_execution(&self.execution, |handle| {
                Ok(runtime_rollback(handle))
            });
        }
    }
}

fn console_db_values(values: &[ConsoleDbValue]) -> Vec<wire::DBValue> {
    values
        .iter()
        .map(|value| match value {
            ConsoleDbValue::Null => wire::DBValue::Null,
            ConsoleDbValue::Int(value) => wire::DBValue::Int(*value),
            ConsoleDbValue::Float(value) => wire::DBValue::Float(*value),
            ConsoleDbValue::Bool(value) => wire::DBValue::Bool(*value),
            ConsoleDbValue::Text(value) => wire::DBValue::Text(value.clone()),
            ConsoleDbValue::Bytes(value) => wire::DBValue::Blob(value.clone()),
        })
        .collect()
}

fn console_db_value(value: &wire::DBValue) -> ConsoleDbValue {
    match value {
        wire::DBValue::Null => ConsoleDbValue::Null,
        wire::DBValue::Int(value) => ConsoleDbValue::Int(*value),
        wire::DBValue::Float(value) => ConsoleDbValue::Float(*value),
        wire::DBValue::Bool(value) => ConsoleDbValue::Bool(*value),
        wire::DBValue::Text(value) => ConsoleDbValue::Text(value.clone()),
        wire::DBValue::Blob(value) => ConsoleDbValue::Bytes(value.clone()),
    }
}

