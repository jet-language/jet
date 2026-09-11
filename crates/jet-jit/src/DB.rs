//! `core.db` hosts (#729). `include!` canonical SQLite runtime + wire codec —
//! no third algorithm.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.
#![allow(dead_code)]

use super::Concurrency;
use crate::Marshal::{clone_string, result_err_msg, result_ok};
use jet_foundation::AST::{CtValue, Type};
use jet_foundation::Diagnostics::{Diagnostic, Span};
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_module::Module;
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
    jet_job_queue_install_store_provider_if_absent, jet_services_active_execution_endpoint,
    JetJobEnqueue, JetJobError, JetJobPayload, JetJobQueue, JetJobQueueClaim,
    JetJobQueueDeliveryPolicy, JetJobQueueEvent, JetJobQueuePolicy, JetJobQueueReceipt,
    JetJobQueueRecord, JetJobQueueRow, JetJobQueueState, JetJobQueueStatus, JetJobQueueStore,
    JetJobQueueStoreProvider, JetJobQueueValue, JetJobResult, JetServiceError,
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

fn jit_job_queue_install_provider() {
    JIT_JOB_QUEUE_PROVIDER.call_once(|| {
        let _ = jet_job_queue_install_store_provider_if_absent(Box::new(JitJobQueueProvider));
    });
}
/// Runtime-owned opaque JobQueue slot. The queue keeps the provider's native
/// SQLite connection alive until this slot is dropped.
pub(crate) struct JitJobQueueSlot {
    pub(crate) queue: JetJobQueue<'static>,
}

fn queue_policy_error(message: impl Into<String>) -> JetServiceError {
    JetServiceError::Policy(message.into())
}

fn queue_unavailable_error(message: impl Into<String>) -> JetServiceError {
    JetServiceError::Unavailable(message.into())
}

fn queue_invalid_handle() -> JetServiceError {
    queue_unavailable_error("invalid JobQueue handle")
}

fn queue_result_err(rt: &mut crate::runtime_host::JitRuntime, error: JetServiceError) -> i64 {
    let (message, discriminant) = match error {
        JetServiceError::Full(message) => (message, 0u64),
        JetServiceError::Ambiguous(message) => (message, 1u64),
        JetServiceError::Unknown(message) => (message, 2u64),
        JetServiceError::NotStarted(message) => (message, 3u64),
        JetServiceError::Policy(message) => (message, 4u64),
        JetServiceError::Unavailable(message) => (message, 5u64),
        JetServiceError::Partitioned(message) => (message, 6u64),
        JetServiceError::Revoked(message) => (message, 7u64),
        JetServiceError::Stale(message) => (message, 8u64),
        JetServiceError::Expired(message) => (message, 9u64),
    };
    let message = rt.heap.alloc_string(message);
    crate::runtime_host::alloc_jit_result(
        rt,
        false,
        ((message as u64) << 8) | discriminant,
    )
}

fn queue_result<T>(
    rt: &mut crate::runtime_host::JitRuntime,
    result: Result<T, JetServiceError>,
    encode: impl FnOnce(&mut crate::runtime_host::JitRuntime, T) -> u64,
) -> i64 {
    match result {
        Ok(value) => {
            let bits = encode(rt, value);
            crate::runtime_host::alloc_jit_result(rt, true, bits)
        }
        Err(error) => queue_result_err(rt, error),
    }
}

fn queue_slot_index(handle: i64) -> Option<usize> {
    usize::try_from(handle.checked_sub(1)?).ok()
}

fn with_jit_job_queue<T>(
    rt: &mut crate::runtime_host::JitRuntime,
    handle: i64,
    operation: impl FnOnce(&mut JetJobQueue<'static>) -> Result<T, JetServiceError>,
) -> Result<T, JetServiceError> {
    let index = queue_slot_index(handle).ok_or_else(queue_invalid_handle)?;
    let slot = rt
        .job_queues
        .get_mut(index)
        .and_then(Option::as_mut)
        .ok_or_else(queue_invalid_handle)?;
    operation(&mut slot.queue)
}

fn queue_string(
    rt: &crate::runtime_host::JitRuntime,
    handle: i64,
    label: &str,
) -> Result<String, JetServiceError> {
    rt.heap.clone_string(handle).ok_or_else(|| {
        queue_policy_error(format!("JobQueue {label} must be a string descriptor"))
    })
}

fn queue_record_int(
    rt: &crate::runtime_host::JitRuntime,
    record: i64,
    index: i64,
    label: &str,
) -> Result<i64, JetServiceError> {
    rt.heap.record_get_int(record, index).ok_or_else(|| {
        queue_policy_error(format!("JobQueue {label} must be an integer field"))
    })
}

fn queue_record_bool(
    rt: &crate::runtime_host::JitRuntime,
    record: i64,
    index: i64,
    label: &str,
) -> Result<bool, JetServiceError> {
    rt.heap.record_get_bool(record, index).ok_or_else(|| {
        queue_policy_error(format!("JobQueue {label} must be a boolean field"))
    })
}

fn queue_record_string(
    rt: &crate::runtime_host::JitRuntime,
    record: i64,
    index: i64,
    label: &str,
) -> Result<String, JetServiceError> {
    rt.heap.record_clone_string(record, index).ok_or_else(|| {
        queue_policy_error(format!("JobQueue {label} must be a string field"))
    })
}

fn queue_record_record(
    rt: &crate::runtime_host::JitRuntime,
    record: i64,
    index: i64,
    label: &str,
) -> Result<i64, JetServiceError> {
    rt.heap.record_get_record(record, index).ok_or_else(|| {
        queue_policy_error(format!("JobQueue {label} must be a record field"))
    })
}

fn queue_bool_word(raw: i64, label: &str) -> Result<bool, JetServiceError> {
    match raw {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(queue_policy_error(format!(
            "JobQueue {label} must be a boolean word"
        ))),
    }
}

fn queue_limit(raw: i64, label: &str) -> Result<usize, JetServiceError> {
    usize::try_from(raw).map_err(|_| {
        queue_policy_error(format!("JobQueue {label} must be a non-negative limit"))
    })
}

fn queue_optional_string(
    rt: &crate::runtime_host::JitRuntime,
    raw: i64,
    label: &str,
) -> Result<Option<String>, JetServiceError> {
    if raw == 0 {
        return Ok(None);
    }
    let handle = raw.checked_sub(1).ok_or_else(|| {
        queue_policy_error(format!("JobQueue {label} has an invalid optional string"))
    })?;
    rt.heap.clone_string(handle).map(Some).ok_or_else(|| {
        queue_policy_error(format!("JobQueue {label} has an invalid optional string"))
    })
}

fn queue_optional_int(raw: i64, label: &str) -> Result<Option<i64>, JetServiceError> {
    if raw == 0 {
        Ok(None)
    } else {
        raw.checked_sub(1).map(Some).ok_or_else(|| {
            queue_policy_error(format!("JobQueue {label} has an invalid optional integer"))
        })
    }
}

fn queue_record_optional_string(
    rt: &crate::runtime_host::JitRuntime,
    record: i64,
    index: i64,
    label: &str,
) -> Result<Option<String>, JetServiceError> {
    if let Some(value) = rt.heap.record_clone_string(record, index) {
        return Ok(Some(value));
    }
    queue_optional_string(rt, queue_record_int(rt, record, index, label)?, label)
}

fn queue_record_optional_int(
    rt: &crate::runtime_host::JitRuntime,
    record: i64,
    index: i64,
    label: &str,
) -> Result<Option<i64>, JetServiceError> {
    queue_optional_int(queue_record_int(rt, record, index, label)?, label)
}

fn queue_bytes(
    rt: &crate::runtime_host::JitRuntime,
    list: i64,
    label: &str,
) -> Result<Vec<u8>, JetServiceError> {
    let length = rt.heap.list_len(list).ok_or_else(|| {
        queue_policy_error(format!("JobQueue {label} must be a byte list"))
    })?;
    let length = usize::try_from(length).map_err(|_| {
        queue_policy_error(format!("JobQueue {label} has an invalid byte-list length"))
    })?;
    let mut bytes = Vec::with_capacity(length);
    for index in 0..length {
        let value = rt.heap.list_get_int(list, index as i64).ok_or_else(|| {
            queue_policy_error(format!("JobQueue {label} contains an invalid byte"))
        })?;
        bytes.push(u8::try_from(value).map_err(|_| {
            queue_policy_error(format!("JobQueue {label} contains a non-byte integer"))
        })?);
    }
    Ok(bytes)
}

fn queue_payload(
    rt: &crate::runtime_host::JitRuntime,
    record: i64,
) -> Result<JetJobPayload, JetServiceError> {
    let type_id = queue_record_string(rt, record, 0, "payload type")?;
    let bytes_handle = queue_record_int(rt, record, 1, "payload bytes")?;
    let bytes = queue_bytes(rt, bytes_handle, "payload bytes")?;
    let publish = queue_record_bool(rt, record, 2, "payload publish")?;
    JetJobPayload::new(type_id, bytes, publish)
}

fn queue_result_value(
    rt: &crate::runtime_host::JitRuntime,
    record: i64,
) -> Result<JetJobResult, JetServiceError> {
    let type_id = queue_record_string(rt, record, 0, "result type")?;
    let bytes_handle = queue_record_int(rt, record, 1, "result bytes")?;
    let bytes = queue_bytes(rt, bytes_handle, "result bytes")?;
    let publish = queue_record_bool(rt, record, 2, "result publish")?;
    JetJobResult::new(type_id, bytes, publish)
}

fn queue_error_value(
    rt: &crate::runtime_host::JitRuntime,
    record: i64,
) -> Result<JetJobError, JetServiceError> {
    let type_id = queue_record_string(rt, record, 0, "error type")?;
    let reason = queue_record_string(rt, record, 1, "error reason")?;
    let detail = queue_record_optional_string(rt, record, 2, "error detail")?;
    JetJobError::new(type_id, reason, detail)
}

fn queue_state(raw: i64) -> Result<JetJobQueueState, JetServiceError> {
    match raw {
        0 => Ok(JetJobQueueState::Queued),
        1 => Ok(JetJobQueueState::Running),
        2 => Ok(JetJobQueueState::Retrying),
        3 => Ok(JetJobQueueState::Completed),
        4 => Ok(JetJobQueueState::Failed),
        5 => Ok(JetJobQueueState::DeadLettered),
        6 => Ok(JetJobQueueState::Cancelled),
        _ => Err(queue_policy_error("JobQueue receipt has an invalid state")),
    }
}

fn queue_delivery(raw: i64) -> Result<JetJobQueueDeliveryPolicy, JetServiceError> {
    if raw == 0 {
        Ok(JetJobQueueDeliveryPolicy::AtLeastOnce)
    } else {
        Err(queue_policy_error(
            "JobQueue receipt has an invalid delivery policy",
        ))
    }
}

fn queue_receipt(
    rt: &crate::runtime_host::JitRuntime,
    record: i64,
) -> Result<JetJobQueueReceipt, JetServiceError> {
    Ok(JetJobQueueReceipt {
        id: queue_record_string(rt, record, 0, "receipt id")?,
        authority: queue_record_string(rt, record, 1, "receipt authority")?,
        queue: queue_record_string(rt, record, 2, "receipt queue")?,
        job_type: queue_record_string(rt, record, 3, "receipt job type")?,
        state: queue_state(queue_record_int(rt, record, 4, "receipt state")?)?,
        sequence: queue_record_int(rt, record, 5, "receipt sequence")?,
        attempts: u32::try_from(queue_record_int(rt, record, 6, "receipt attempts")?)
            .map_err(|_| queue_policy_error("JobQueue receipt attempts are invalid"))?,
        due_at_ms: queue_record_int(rt, record, 7, "receipt due time")?,
        accepted_at_ms: queue_record_int(rt, record, 8, "receipt accepted time")?,
        request_id: queue_record_optional_string(rt, record, 9, "receipt request id")?,
        idempotency_key: queue_record_optional_string(rt, record, 10, "receipt idempotency key")?,
        lease_until_ms: queue_record_optional_int(rt, record, 11, "receipt lease")?,
        duration_ms: queue_record_optional_int(rt, record, 12, "receipt duration")?,
        error_reason: queue_record_optional_string(rt, record, 13, "receipt error")?,
        delivery: queue_delivery(queue_record_int(rt, record, 14, "receipt delivery")?)?,
        duplicate: queue_record_bool(rt, record, 15, "receipt duplicate")?,
        signature: queue_record_string(rt, record, 16, "receipt signature")?,
    })
}

fn queue_claim(
    rt: &crate::runtime_host::JitRuntime,
    record: i64,
) -> Result<JetJobQueueClaim, JetServiceError> {
    Ok(JetJobQueueClaim {
        receipt: queue_receipt(rt, queue_record_record(rt, record, 0, "claim receipt")?)?,
        payload: queue_payload(rt, queue_record_record(rt, record, 1, "claim payload")?)?,
        worker: queue_record_string(rt, record, 2, "claim worker")?,
        lease_token: queue_record_string(rt, record, 3, "claim lease token")?,
        lease_until_ms: queue_record_int(rt, record, 4, "claim lease")?,
    })
}

fn queue_duration_ms(
    rt: &crate::runtime_host::JitRuntime,
    duration: i64,
    label: &str,
) -> Result<i64, JetServiceError> {
    let nanos = queue_record_int(rt, duration, 0, label)?;
    if nanos < 0 {
        return Err(queue_policy_error(format!(
            "JobQueue {label} must be non-negative"
        )));
    }
    Ok(nanos / 1_000_000)
}

fn queue_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn queue_set_string(
    rt: &mut crate::runtime_host::JitRuntime,
    record: i64,
    index: i64,
    value: String,
) {
    let value = rt.heap.alloc_string(value);
    let _ = rt.heap.record_set_string(record, index, value);
}

fn queue_set_optional_string(
    rt: &mut crate::runtime_host::JitRuntime,
    record: i64,
    index: i64,
    value: Option<String>,
) {
    let value = value
        .map(|value| rt.heap.alloc_string(value).wrapping_add(1))
        .unwrap_or(0);
    let _ = rt.heap.record_set_int(record, index, value);
}

fn queue_set_optional_int(
    rt: &mut crate::runtime_host::JitRuntime,
    record: i64,
    index: i64,
    value: Option<i64>,
) {
    let value = value.map(|value| value.wrapping_add(1)).unwrap_or(0);
    let _ = rt.heap.record_set_int(record, index, value);
}

fn queue_set_optional_record(
    rt: &mut crate::runtime_host::JitRuntime,
    record: i64,
    index: i64,
    value: Option<i64>,
) {
    let value = value.map(|value| value.wrapping_add(1)).unwrap_or(0);
    let _ = rt.heap.record_set_int(record, index, value);
}

fn queue_alloc_bytes(rt: &mut crate::runtime_host::JitRuntime, bytes: Vec<u8>) -> i64 {
    rt.heap
        .alloc_int_list(bytes.into_iter().map(i64::from).collect())
}

fn queue_alloc_payload(
    rt: &mut crate::runtime_host::JitRuntime,
    payload: JetJobPayload,
) -> i64 {
    let record = rt.heap.alloc_record(3);
    let bytes = queue_alloc_bytes(rt, payload.bytes);
    queue_set_string(rt, record, 0, payload.type_id);
    let _ = rt.heap.record_set_int(record, 1, bytes);
    let _ = rt.heap.record_set_bool(record, 2, payload.publish);
    record
}

fn queue_alloc_result_value(
    rt: &mut crate::runtime_host::JitRuntime,
    result: JetJobResult,
) -> i64 {
    let record = rt.heap.alloc_record(3);
    let bytes = queue_alloc_bytes(rt, result.bytes);
    queue_set_string(rt, record, 0, result.type_id);
    let _ = rt.heap.record_set_int(record, 1, bytes);
    let _ = rt.heap.record_set_bool(record, 2, result.publish);
    record
}

fn queue_alloc_error_value(
    rt: &mut crate::runtime_host::JitRuntime,
    error: JetJobError,
) -> i64 {
    let record = rt.heap.alloc_record(3);
    queue_set_string(rt, record, 0, error.type_id);
    queue_set_string(rt, record, 1, error.reason);
    queue_set_optional_string(rt, record, 2, error.detail);
    record
}

fn queue_alloc_receipt(
    rt: &mut crate::runtime_host::JitRuntime,
    receipt: JetJobQueueReceipt,
) -> i64 {
    let record = rt.heap.alloc_record(17);
    queue_set_string(rt, record, 0, receipt.id);
    queue_set_string(rt, record, 1, receipt.authority);
    queue_set_string(rt, record, 2, receipt.queue);
    queue_set_string(rt, record, 3, receipt.job_type);
    let _ = rt
        .heap
        .record_set_int(record, 4, queue_state_wire(receipt.state));
    let _ = rt.heap.record_set_int(record, 5, receipt.sequence);
    let _ = rt
        .heap
        .record_set_int(record, 6, i64::from(receipt.attempts));
    let _ = rt.heap.record_set_int(record, 7, receipt.due_at_ms);
    let _ = rt.heap.record_set_int(record, 8, receipt.accepted_at_ms);
    queue_set_optional_string(rt, record, 9, receipt.request_id);
    queue_set_optional_string(rt, record, 10, receipt.idempotency_key);
    queue_set_optional_int(rt, record, 11, receipt.lease_until_ms);
    queue_set_optional_int(rt, record, 12, receipt.duration_ms);
    queue_set_optional_string(rt, record, 13, receipt.error_reason);
    let _ = rt
        .heap
        .record_set_int(record, 14, queue_delivery_wire(receipt.delivery));
    let _ = rt.heap.record_set_bool(record, 15, receipt.duplicate);
    queue_set_string(rt, record, 16, receipt.signature);
    record
}

fn queue_alloc_claim(
    rt: &mut crate::runtime_host::JitRuntime,
    claim: JetJobQueueClaim,
) -> i64 {
    let record = rt.heap.alloc_record(5);
    let receipt = queue_alloc_receipt(rt, claim.receipt);
    let payload = queue_alloc_payload(rt, claim.payload);
    let _ = rt.heap.record_set_record(record, 0, receipt);
    let _ = rt.heap.record_set_record(record, 1, payload);
    queue_set_string(rt, record, 2, claim.worker);
    queue_set_string(rt, record, 3, claim.lease_token);
    let _ = rt.heap.record_set_int(record, 4, claim.lease_until_ms);
    record
}

fn queue_alloc_event(
    rt: &mut crate::runtime_host::JitRuntime,
    event: JetJobQueueEvent,
) -> i64 {
    let record = rt.heap.alloc_record(7);
    let _ = rt.heap.record_set_int(record, 0, event.sequence);
    let _ = rt.heap.record_set_int(record, 1, queue_state_wire(event.state));
    let _ = rt
        .heap
        .record_set_int(record, 2, i64::from(event.attempts));
    let _ = rt.heap.record_set_int(record, 3, event.timestamp_ms);
    queue_set_optional_string(rt, record, 4, event.reason);
    queue_set_optional_int(rt, record, 5, event.duration_ms);
    queue_set_optional_string(rt, record, 6, event.worker);
    record
}

fn queue_alloc_record(
    rt: &mut crate::runtime_host::JitRuntime,
    value: JetJobQueueRecord,
) -> i64 {
    let record = rt.heap.alloc_record(6);
    let receipt = queue_alloc_receipt(rt, value.receipt);
    let _ = rt.heap.record_set_record(record, 0, receipt);
    let payload = value.payload.map(|value| queue_alloc_payload(rt, value));
    let result = value
        .result
        .map(|value| queue_alloc_result_value(rt, value));
    let error = value.error.map(|value| queue_alloc_error_value(rt, value));
    queue_set_optional_record(rt, record, 1, payload);
    queue_set_optional_record(rt, record, 2, result);
    queue_set_optional_record(rt, record, 3, error);
    queue_set_optional_int(rt, record, 4, value.started_at_ms);
    queue_set_optional_int(rt, record, 5, value.finished_at_ms);
    record
}

fn queue_alloc_status(
    rt: &mut crate::runtime_host::JitRuntime,
    status: JetJobQueueStatus,
) -> i64 {
    let record = rt.heap.alloc_record(15);
    queue_set_string(rt, record, 0, status.queue);
    queue_set_string(rt, record, 1, status.authority);
    let _ = rt.heap.record_set_int(record, 2, queue_i64(status.queued));
    let _ = rt.heap.record_set_int(record, 3, queue_i64(status.running));
    let _ = rt.heap.record_set_int(record, 4, queue_i64(status.retrying));
    let _ = rt.heap.record_set_int(record, 5, queue_i64(status.completed));
    let _ = rt.heap.record_set_int(record, 6, queue_i64(status.failed));
    let _ = rt
        .heap
        .record_set_int(record, 7, queue_i64(status.dead_lettered));
    let _ = rt
        .heap
        .record_set_int(record, 8, queue_i64(status.cancelled));
    let _ = rt.heap.record_set_int(record, 9, queue_i64(status.depth));
    let _ = rt.heap.record_set_int(record, 10, queue_i64(status.wait_ms));
    let _ = rt
        .heap
        .record_set_int(record, 11, queue_i64(status.throughput));
    let _ = rt
        .heap
        .record_set_int(record, 12, queue_i64(status.capacity as u64));
    let _ = rt.heap.record_set_bool(record, 13, status.paused);
    let _ = rt
        .heap
        .record_set_int(record, 14, queue_i64(status.freshness_ms));
    record
}

fn queue_state_wire(state: JetJobQueueState) -> i64 {
    match state {
        JetJobQueueState::Queued => 0,
        JetJobQueueState::Running => 1,
        JetJobQueueState::Retrying => 2,
        JetJobQueueState::Completed => 3,
        JetJobQueueState::Failed => 4,
        JetJobQueueState::DeadLettered => 5,
        JetJobQueueState::Cancelled => 6,
    }
}

fn queue_delivery_wire(delivery: JetJobQueueDeliveryPolicy) -> i64 {
    match delivery {
        JetJobQueueDeliveryPolicy::AtLeastOnce => 0,
    }
}

fn queue_alloc_list(
    rt: &mut crate::runtime_host::JitRuntime,
    values: impl IntoIterator<Item = i64>,
) -> i64 {
    rt.heap.alloc_int_list(values.into_iter().collect())
}
fn jet_jit_job_queue_default() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        jit_job_queue_install_provider();
        let result = (|| -> Result<i64, JetServiceError> {
            let endpoint = jet_services_active_execution_endpoint()?;
            let handle = i64::try_from(
                rt.job_queues
                    .len()
                    .checked_add(1)
                    .ok_or_else(|| queue_unavailable_error("JobQueue handle space exhausted"))?,
            )
            .map_err(|_| queue_unavailable_error("JobQueue handle space exhausted"))?;
            let queue = JetJobQueue::open_default(
                &endpoint,
                "default".to_string(),
                JetJobQueuePolicy::default(),
            )?;
            rt.job_queues.push(Some(JitJobQueueSlot { queue }));
            Ok(handle)
        })();
        queue_result(rt, result, |_, handle| handle as u64)
    })
}

fn jet_jit_job_queue_enqueue(queue: i64, job: i64, payload: i64, key: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = (|| -> Result<JetJobQueueReceipt, JetServiceError> {
            let job_type = queue_string(rt, job, "job type")?;
            let payload = queue_payload(rt, payload)?;
            let idempotency_key = queue_optional_string(rt, key, "idempotency key")?;
            let request = JetJobEnqueue {
                job_type,
                payload,
                idempotency_key,
                request_id: None,
                delay_ms: 0,
            };
            with_jit_job_queue(rt, queue, |queue| queue.enqueue(request))
        })();
        queue_result(rt, result, |rt, receipt| {
            queue_alloc_receipt(rt, receipt) as u64
        })
    })
}

fn jet_jit_job_queue_delay(queue: i64, job: i64, payload: i64, duration: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = (|| -> Result<JetJobQueueReceipt, JetServiceError> {
            let job_type = queue_string(rt, job, "job type")?;
            let payload = queue_payload(rt, payload)?;
            let duration_ms = queue_duration_ms(rt, duration, "delay duration")?;
            with_jit_job_queue(rt, queue, |queue| {
                queue.enqueue_delayed(job_type, payload, duration_ms, None, None)
            })
        })();
        queue_result(rt, result, |rt, receipt| {
            queue_alloc_receipt(rt, receipt) as u64
        })
    })
}

fn jet_jit_job_queue_receipt(queue: i64, id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = (|| -> Result<JetJobQueueReceipt, JetServiceError> {
            let id = queue_string(rt, id, "receipt id")?;
            with_jit_job_queue(rt, queue, |queue| queue.receipt(&id))
        })();
        queue_result(rt, result, |rt, receipt| {
            queue_alloc_receipt(rt, receipt) as u64
        })
    })
}

fn jet_jit_job_queue_inspect(queue: i64, limit: i64, include_payload: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = (|| -> Result<Vec<JetJobQueueRecord>, JetServiceError> {
            let limit = queue_limit(limit, "inspect limit")?;
            let include_payload = queue_bool_word(include_payload, "inspect payload flag")?;
            with_jit_job_queue(rt, queue, |queue| queue.inspect(limit, include_payload))
        })();
        queue_result(rt, result, |rt, values| {
            let values = values
                .into_iter()
                .map(|value| queue_alloc_record(rt, value))
                .collect::<Vec<_>>();
            queue_alloc_list(rt, values) as u64
        })
    })
}

fn jet_jit_job_queue_events(queue: i64, id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = (|| -> Result<Vec<JetJobQueueEvent>, JetServiceError> {
            let id = queue_string(rt, id, "event job id")?;
            with_jit_job_queue(rt, queue, |queue| queue.events(&id))
        })();
        queue_result(rt, result, |rt, values| {
            let values = values
                .into_iter()
                .map(|value| queue_alloc_event(rt, value))
                .collect::<Vec<_>>();
            queue_alloc_list(rt, values) as u64
        })
    })
}

fn jet_jit_job_queue_claim(queue: i64, worker: i64, limit: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = (|| -> Result<Vec<JetJobQueueClaim>, JetServiceError> {
            let worker = queue_string(rt, worker, "claim worker")?;
            let limit = queue_limit(limit, "claim limit")?;
            with_jit_job_queue(rt, queue, |queue| queue.claim(&worker, limit))
        })();
        queue_result(rt, result, |rt, values| {
            let values = values
                .into_iter()
                .map(|value| queue_alloc_claim(rt, value))
                .collect::<Vec<_>>();
            queue_alloc_list(rt, values) as u64
        })
    })
}

fn jet_jit_job_queue_heartbeat(queue: i64, claim: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = (|| -> Result<JetJobQueueReceipt, JetServiceError> {
            let claim = queue_claim(rt, claim)?;
            with_jit_job_queue(rt, queue, |queue| queue.heartbeat(&claim))
        })();
        queue_result(rt, result, |rt, receipt| {
            queue_alloc_receipt(rt, receipt) as u64
        })
    })
}

fn jet_jit_job_queue_acknowledge(queue: i64, claim: i64, result_value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = (|| -> Result<JetJobQueueReceipt, JetServiceError> {
            let claim = queue_claim(rt, claim)?;
            let result_value = queue_result_value(rt, result_value)?;
            with_jit_job_queue(rt, queue, |queue| queue.acknowledge(&claim, result_value))
        })();
        queue_result(rt, result, |rt, receipt| {
            queue_alloc_receipt(rt, receipt) as u64
        })
    })
}

fn jet_jit_job_queue_fail(queue: i64, claim: i64, error_value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = (|| -> Result<JetJobQueueReceipt, JetServiceError> {
            let claim = queue_claim(rt, claim)?;
            let error_value = queue_error_value(rt, error_value)?;
            with_jit_job_queue(rt, queue, |queue| queue.fail(&claim, error_value))
        })();
        queue_result(rt, result, |rt, receipt| {
            queue_alloc_receipt(rt, receipt) as u64
        })
    })
}

fn jet_jit_job_queue_transition(
    queue: i64,
    id: i64,
    reason: i64,
    lease: i64,
    dead_letter: bool,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = (|| -> Result<JetJobQueueReceipt, JetServiceError> {
            let id = queue_string(rt, id, "transition job id")?;
            let reason = queue_string(rt, reason, "transition reason")?;
            let lease = queue_optional_string(rt, lease, "transition lease")?;
            with_jit_job_queue(rt, queue, move |queue| {
                if dead_letter {
                    queue.dead_letter(&id, reason, lease.as_deref())
                } else {
                    queue.cancel(&id, reason, lease.as_deref())
                }
            })
        })();
        queue_result(rt, result, |rt, receipt| {
            queue_alloc_receipt(rt, receipt) as u64
        })
    })
}

fn jet_jit_job_queue_cancel(queue: i64, id: i64, reason: i64, lease: i64) -> i64 {
    jet_jit_job_queue_transition(queue, id, reason, lease, false)
}

fn jet_jit_job_queue_dead_letter(queue: i64, id: i64, reason: i64, lease: i64) -> i64 {
    jet_jit_job_queue_transition(queue, id, reason, lease, true)
}

fn jet_jit_job_queue_recover_expired(queue: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = with_jit_job_queue(rt, queue, |queue| queue.recover_expired());
        queue_result(rt, result, |_, ()| 0)
    })
}

fn jet_jit_job_queue_status(queue: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = with_jit_job_queue(rt, queue, |queue| queue.status());
        queue_result(rt, result, |rt, status| {
            queue_alloc_status(rt, status) as u64
        })
    })
}

fn jet_jit_job_queue_pause(queue: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = with_jit_job_queue(rt, queue, |queue| queue.pause());
        queue_result(rt, result, |rt, status| {
            queue_alloc_status(rt, status) as u64
        })
    })
}

fn jet_jit_job_queue_resume(queue: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = with_jit_job_queue(rt, queue, |queue| queue.resume());
        queue_result(rt, result, |rt, status| {
            queue_alloc_status(rt, status) as u64
        })
    })
}

fn jet_jit_job_queue_wait(queue: i64, duration: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = (|| -> Result<JetJobQueueStatus, JetServiceError> {
            let duration_ms = queue_duration_ms(rt, duration, "wait duration")?;
            with_jit_job_queue(rt, queue, |queue| queue.wait(duration_ms))
        })();
        queue_result(rt, result, |rt, status| {
            queue_alloc_status(rt, status) as u64
        })
    })
}

fn jet_jit_job_queue_prune(queue: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = with_jit_job_queue(rt, queue, |queue| queue.prune());
        queue_result(rt, result, |_, count| queue_i64(count) as u64)
    })
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
    use super::{JetDebug, JetShow};

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

// DBValue heap record ABI (same 2-slot shape as DataTree): [disc:i64, payload].
const DV_NULL: i64 = 0;
const DV_INT: i64 = 1;
const DV_FLOAT: i64 = 2;
const DV_TEXT: i64 = 3;
const DV_BOOL: i64 = 4;
const DV_BLOB: i64 = 5;

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

fn scope_request_id(handle: u64) -> Option<String> {
    scope_parts(handle).and_then(|(_, _, _, _, request_id)| request_id)
}
#[derive(Clone)]
struct JitObservedQuery {
    session_id: String,
    statement_identity: String,
    handle: u64,
    sql: wire::SQL,
    source: String,
}

static JIT_OBSERVED_QUERIES: std::sync::LazyLock<
    std::sync::Mutex<HashMap<(String, String), JitObservedQuery>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));
static JIT_DB_EXPLAIN_CALLBACK_ONCE: std::sync::Once = std::sync::Once::new();
static JIT_DB_EXPLAIN_CALLBACK_GUARD: std::sync::OnceLock<
    jet_foundation::Devtools::JetDevtoolsDatabaseExplainCallbackGuard,
> = std::sync::OnceLock::new();

fn jit_db_devtools_now_ms() -> u64 {
    u64::try_from(jet_codegen::scheduler::jet_std_time_now()).unwrap_or(0)
}

fn jit_db_devtools_authorized() -> bool {
    std::env::var("JET_DEVTOOLS_DB_CREDENTIALS")
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

fn jit_db_devtools_session_id() -> Option<String> {
    std::env::var("JET_DEVTOOLS_RELAY_SESSION_ID")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(jet_codegen::scheduler::jet_devtools_native_session_id)
}

fn jit_db_json_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len().saturating_add(2));
    escaped.push('"');
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                escaped.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

fn jit_db_publish_explain_error(
    session_id: &str,
    request_id: &str,
    statement_identity: &str,
    source: &str,
    code: &str,
) {
    let mut payload = String::from("{\"query_id\":");
    payload.push_str(&jit_db_json_string(statement_identity));
    payload.push_str(",\"statement_identity\":");
    payload.push_str(&jit_db_json_string(statement_identity));
    payload.push_str(",\"source\":");
    payload.push_str(&jit_db_json_string(source));
    payload.push_str(",\"request_id\":");
    payload.push_str(&jit_db_json_string(request_id));
    payload.push_str(",\"capability\":\"DB.Explain\",\"authority_granted\":false,\"status\":\"error\",\"error_code\":");
    payload.push_str(&jit_db_json_string(code));
    payload.push_str(",\"error_message\":\"database EXPLAIN was not completed\",\"duration_ms\":0,\"deadline_ms\":0,\"finished_at_ms\":");
    payload.push_str(&jit_db_devtools_now_ms().to_string());
    payload.push_str(",\"freshness_ms\":0,\"timed_out\":false,\"truncated\":false,\"row_count\":0,\"plan_rows\":[]}");
    if let Ok(event) = jet_foundation::Devtools::JetDevtoolsEvent::from_parts(
        jit_db_devtools_now_ms(),
        "core.db",
        "DatabaseExplainResult",
        statement_identity.to_string(),
        payload,
    ) {
        jet_codegen::scheduler::jet_devtools_publish_event_for_session(session_id, event);
    }
}

fn jit_db_remember_observed_query(
    handle: u64,
    sql: &wire::SQL,
    metadata: &wire::JetDbQueryMetadata,
) {
    if !jit_db_devtools_authorized() {
        return;
    }
    let Some(session_id) = jit_db_devtools_session_id() else {
        return;
    };
    let Ok(mut observed) = JIT_OBSERVED_QUERIES.lock() else {
        return;
    };
    observed.insert(
        (session_id.clone(), metadata.statement_identity.clone()),
        JitObservedQuery {
            session_id,
            statement_identity: metadata.statement_identity.clone(),
            handle: base_handle(handle),
            sql: sql.clone(),
            source: metadata.source_file.clone(),
        },
    );
    while observed.len() > 256 {
        if let Some(key) = observed.keys().next().cloned() {
            observed.remove(&key);
        } else {
            break;
        }
    }
}

fn jit_db_dispatch_devtools_explain(
    session_id: &str,
    request_id: &str,
    statement_identity: &str,
    timeout_ms: u64,
    max_rows: u64,
) {
    if !jit_db_devtools_authorized() {
        jit_db_publish_explain_error(
            session_id,
            request_id,
            statement_identity,
            "<redacted>",
            "authority_denied",
        );
        return;
    }
    let Some(observed) = JIT_OBSERVED_QUERIES
        .lock()
        .ok()
        .and_then(|queries| {
            queries
                .get(&(session_id.to_string(), statement_identity.to_string()))
                .cloned()
        })
    else {
        jit_db_publish_explain_error(
            session_id,
            request_id,
            statement_identity,
            "<unknown>",
            "statement_not_observed",
        );
        return;
    };
    let started_at_ms = jit_db_devtools_now_ms();
    let request = match devtools::JetDevtoolsDatabaseExplainFactRequest::new(
        observed.statement_identity.clone(),
        observed.statement_identity.clone(),
        devtools::JetDevtoolsDatabaseStatementClass::ReadOnly,
        observed.source.clone(),
        Some(request_id.to_string()),
        Vec::new(),
        timeout_ms,
        max_rows,
    ) {
        Ok(request) => request,
        Err(_) => {
            jit_db_publish_explain_error(
                session_id,
                request_id,
                statement_identity,
                &observed.source,
                "invalid_request",
            );
            return;
        }
    };
    let grant = match devtools::JetDevtoolsDatabaseCapabilityFact::for_explain(
        observed.statement_identity.clone(),
        true,
    ) {
        Ok(grant) => grant,
        Err(_) => {
            jit_db_publish_explain_error(
                session_id,
                request_id,
                statement_identity,
                &observed.source,
                "authority_invalid",
            );
            return;
        }
    };
    let authorized = match request.authorize(&grant, started_at_ms) {
        Ok(authorized) => authorized,
        Err(_) => {
            jit_db_publish_explain_error(
                session_id,
                request_id,
                statement_identity,
                &observed.source,
                "authority_denied",
            );
            return;
        }
    };
    let params = wire::jet_db_encode_params(&observed.sql.1);
    let explain_wire = runtime::jet_db_explain(
        observed.handle,
        &observed.sql.0,
        &params,
        max_rows,
        timeout_ms,
    );
    let facts = match wire::jet_db_decode_explain_result(&explain_wire) {
        Ok(facts) => facts,
        Err(_) => {
            jit_db_publish_explain_error(
                session_id,
                request_id,
                statement_identity,
                &observed.source,
                "driver_error",
            );
            return;
        }
    };
    let elapsed_ms = facts.elapsed_ms;
    let timed_out = facts.timed_out;
    let driver_truncated = facts.truncated;
    let mut rows = Vec::with_capacity(facts.plan.len());
    for plan in facts.plan {
        let parent = (plan.parent != 0).then(|| plan.parent.to_string());
        let Ok(row) = devtools::JetDevtoolsDatabasePlanRow::new(
            plan.ordinal,
            plan.ordinal.to_string(),
            parent,
            0,
            plan.operation,
            plan.relation,
            plan.index,
            None,
            None,
        ) else {
            jit_db_publish_explain_error(
                session_id,
                request_id,
                statement_identity,
                &observed.source,
                "invalid_plan",
            );
            return;
        };
        rows.push(row);
    }
    let finished_at_ms = started_at_ms.saturating_add(elapsed_ms);
    if let Ok(result) =
        authorized.finish_with_status(finished_at_ms, rows, timed_out, driver_truncated)
    {
        if let Ok(event) = result.to_protocol_event("core.db") {
            jet_codegen::scheduler::jet_devtools_publish_event_for_session(session_id, event);
        }
    }
}

fn jit_db_install_devtools_explain_callback() {
    JIT_DB_EXPLAIN_CALLBACK_ONCE.call_once(|| {
        if let Ok(guard) = jet_codegen::scheduler::jet_devtools_install_database_explain_callback(
            jit_db_dispatch_devtools_explain,
        ) {
            let _ = JIT_DB_EXPLAIN_CALLBACK_GUARD.set(guard);
        }
    });
}

fn alloc_policy_record(table: &str, compiled: wire::JetRowPolicyExpr) -> i64 {
    let table_id = table.to_string();
    let expression_id = compiled.canonical().to_string();
    let compiled_id = match compiled {
        wire::JetRowPolicyExpr::AllowAll => 0,
        wire::JetRowPolicyExpr::OwnerEqualsUser => 1,
    };
    Concurrency::with_runtime_mut(|rt| {
        let table_id = rt.heap.alloc_string(table_id);
        let expression_id = rt.heap.alloc_string(expression_id);
        let record = rt.heap.alloc_record(3);
        let _ = rt.heap.record_set_int(record, 0, table_id);
        let _ = rt.heap.record_set_int(record, 1, expression_id);
        let _ = rt.heap.record_set_int(record, 2, compiled_id);
        record
    })
}

fn policy_record_parts(policy: i64) -> Option<(String, wire::JetRowPolicyExpr)> {
    Concurrency::with_runtime_mut(|rt| {
        let table = rt
            .heap
            .record_get_int(policy, 0)
            .and_then(|id| rt.heap.clone_string(id))?;
        let compiled = match rt.heap.record_get_int(policy, 2)? {
            0 => wire::JetRowPolicyExpr::AllowAll,
            1 => wire::JetRowPolicyExpr::OwnerEqualsUser,
            _ => return None,
        };
        Some((table, compiled))
    })
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

fn alloc_dbvalue_record(disc: i64, payload: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let h = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_int(h, 0, disc);
        let _ = rt.heap.record_set_int(h, 1, payload);
        h
    })
}

fn alloc_dbvalue_float(f: f64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let h = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_int(h, 0, DV_FLOAT);
        let _ = rt.heap.record_set_float(h, 1, f);
        h
    })
}

fn alloc_byte_list(bytes: &[u8]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for byte in bytes {
            let _ = rt.heap.list_push_int(list, i64::from(*byte));
        }
        list
    })
}

fn alloc_dbvalue_value(value: wire::DBValue) -> i64 {
    match value {
        wire::DBValue::Null => alloc_dbvalue_record(DV_NULL, 0),
        wire::DBValue::Int(value) => alloc_dbvalue_record(DV_INT, value),
        wire::DBValue::Float(value) => alloc_dbvalue_float(value),
        wire::DBValue::Text(value) => {
            let text = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(value));
            alloc_dbvalue_record(DV_TEXT, text)
        }
        wire::DBValue::Bool(value) => alloc_dbvalue_record(DV_BOOL, i64::from(value)),
        wire::DBValue::Blob(value) => {
            let bytes = alloc_byte_list(&value);
            alloc_dbvalue_record(DV_BLOB, bytes)
        }
    }
}

fn read_dbvalue(handle: i64) -> Option<wire::DBValue> {
    Concurrency::with_runtime_mut(|rt| {
        let disc = rt.heap.record_get_int(handle, 0)?;
        match disc {
            DV_NULL => Some(wire::DBValue::Null),
            DV_INT => Some(wire::DBValue::Int(
                rt.heap.record_get_int(handle, 1).unwrap_or(0),
            )),
            DV_FLOAT => Some(wire::DBValue::Float(
                rt.heap.record_get_float(handle, 1).unwrap_or(0.0),
            )),
            DV_TEXT => {
                let sid = rt.heap.record_get_int(handle, 1).unwrap_or(0);
                Some(wire::DBValue::Text(
                    rt.heap.clone_string(sid).unwrap_or_default(),
                ))
            }
            DV_BOOL => Some(wire::DBValue::Bool(
                rt.heap.record_get_int(handle, 1).unwrap_or(0) != 0,
            )),
            DV_BLOB => {
                let list = rt.heap.record_get_int(handle, 1)?;
                let len = rt.heap.list_len(list)?;
                let mut bytes = Vec::with_capacity(len as usize);
                for i in 0..len {
                    bytes.push(u8::try_from(rt.heap.list_get_int(list, i)?).ok()?);
                }
                Some(wire::DBValue::Blob(bytes))
            }
            _ => None,
        }
    })
}
fn dbvalue_to_datatree(value: wire::DBValue) -> crate::Encoding::json_rt::DataTree {
    match value {
        wire::DBValue::Null => crate::Encoding::json_rt::DataTree::Null,
        wire::DBValue::Int(value) => crate::Encoding::json_rt::DataTree::Int(
            crate::Encoding::json_rt::jet_int_from_i64(value),
        ),
        wire::DBValue::Float(value) => crate::Encoding::json_rt::DataTree::Float(value),
        wire::DBValue::Text(value) => crate::Encoding::json_rt::DataTree::Text(value),
        wire::DBValue::Bool(value) => crate::Encoding::json_rt::DataTree::Bool(value),
        wire::DBValue::Blob(value) => crate::Encoding::json_rt::DataTree::Bytes(value),
    }
}

/// Convert the checked DB row carrier (a string-keyed map of DBValue handles)
/// through the canonical DB-column projection and DataTree carrier.
pub(crate) fn db_row_to_datatree(
    row: i64,
    names: &[(&str, &str)],
) -> Result<crate::Encoding::json_rt::DataTree, String> {
    let pairs = Concurrency::with_runtime_string(|rt| {
        let len = rt
            .heap
            .map_len(row)
            .ok_or_else(|| "invalid DB row carrier".to_string())?;
        let mut pairs = Vec::with_capacity(len as usize);
        for index in 0..len {
            let key_id = rt
                .heap
                .map_key_at(row, index)
                .ok_or_else(|| "DB row has an invalid key".to_string())?;
            let key = rt
                .heap
                .clone_string(key_id)
                .ok_or_else(|| "DB row key is not a String".to_string())?;
            let value = rt
                .heap
                .map_value_at(row, index)
                .ok_or_else(|| "DB row has an invalid value".to_string())?;
            pairs.push((key, value));
        }
        Ok::<_, String>(pairs)
    })?;
    let row: wire::JetDBRow = pairs
        .into_iter()
        .map(|(key, value)| {
            read_dbvalue(value)
                .map(|value| (key, value))
                .ok_or_else(|| "DB row contains an invalid DBValue".to_string())
        })
        .collect::<Result<_, _>>()?;
    let projected = wire::jet_db_row_project(&row, names, |value| {
        dbvalue_to_datatree(value.clone())
    });
    Ok(crate::Encoding::json_rt::DataTree::Object(projected))
}


fn value_handles_from_list(list: i64) -> Option<Vec<i64>> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list)?;
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            out.push(rt.heap.list_get_int(list, i)?);
        }
        Some(out)
    })
}

pub(crate) fn values_from_list_checked(list: i64) -> Option<Vec<wire::DBValue>> {
    value_handles_from_list(list)?
        .into_iter()
        .map(read_dbvalue)
        .collect()
}

pub(crate) fn values_from_list(list: i64) -> Vec<wire::DBValue> {
    values_from_list_checked(list).unwrap_or_default()
}

pub(crate) fn alloc_dbvalue_list(values: Vec<wire::DBValue>) -> i64 {
    let handles = values
        .into_iter()
        .map(|value| match value {
            wire::DBValue::Null => alloc_dbvalue_record(DV_NULL, 0),
            wire::DBValue::Int(value) => alloc_dbvalue_record(DV_INT, value),
            wire::DBValue::Float(value) => alloc_dbvalue_float(value),
            wire::DBValue::Text(value) => {
                let text = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(value));
                alloc_dbvalue_record(DV_TEXT, text)
            }
            wire::DBValue::Bool(value) => alloc_dbvalue_record(DV_BOOL, i64::from(value)),
            wire::DBValue::Blob(value) => {
                let bytes = alloc_byte_list(&value);
                alloc_dbvalue_record(DV_BLOB, bytes)
            }
        })
        .collect::<Vec<_>>();
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for handle in handles {
            let _ = rt.heap.list_push_int(list, handle);
        }
        list
    })
}

pub(crate) fn alloc_sql_value(value: wire::SQL) -> i64 {
    let (template, params) = value;
    let template = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(template));
    let params_list = alloc_dbvalue_list(params);
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_int(record, 0, template);
        let _ = rt.heap.record_set_int(record, 1, params_list);
        record
    })
}

pub(crate) fn clone_sql_value(value: i64) -> Option<wire::SQL> {
    let (template, params_list) = Concurrency::with_runtime_mut(|rt| {
        let template = rt
            .heap
            .record_get_int(value, 0)
            .and_then(|id| rt.heap.clone_string(id))?;
        let params_list = rt.heap.record_get_int(value, 1)?;
        Some((template, params_list))
    })?;
    Some((template, values_from_list_checked(params_list)?))
}

fn jet_jit_db_policy(table: i64, expression: i64) -> i64 {
    let table = clone_string(table);
    let expression = clone_string(expression);
    match wire::jet_db_policy_compile(&table, &expression) {
        Ok((table, compiled)) => result_ok(alloc_policy_record(&table, compiled) as u64),
        Err(message) => result_err_msg(&message),
    }
}

fn jet_jit_db_policy_audit(scope: i64) -> i64 {
    let Some((_, table, compiled, user, _)) = scope_parts(scope as u64) else {
        return 0;
    };
    let line = wire::jet_db_policy_audit_line(&table, compiled, &user);
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(line))
}

fn jet_jit_db_with_policy(connection: i64, policy: i64, user: i64) -> i64 {
    let Some((table, compiled)) = policy_record_parts(policy) else {
        return 0;
    };
    let scope = new_scope(connection as u64, table, compiled, clone_string(user));
    scope
}
fn rows_to_list_of_maps(rows: Vec<wire::JetDBRow>) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for row in rows {
            let map = rt.heap.alloc_empty_map();
            for (k, v) in row {
                let kid = rt.heap.alloc_string(k);
                let vh = match &v {
                    wire::DBValue::Null => {
                        let h = rt.heap.alloc_record(2);
                        let _ = rt.heap.record_set_int(h, 0, DV_NULL);
                        let _ = rt.heap.record_set_int(h, 1, 0);
                        h
                    }
                    wire::DBValue::Int(n) => {
                        let h = rt.heap.alloc_record(2);
                        let _ = rt.heap.record_set_int(h, 0, DV_INT);
                        let _ = rt.heap.record_set_int(h, 1, *n);
                        h
                    }
                    wire::DBValue::Float(f) => {
                        let h = rt.heap.alloc_record(2);
                        let _ = rt.heap.record_set_int(h, 0, DV_FLOAT);
                        let _ = rt.heap.record_set_float(h, 1, *f);
                        h
                    }
                    wire::DBValue::Text(s) => {
                        let sid = rt.heap.alloc_string(s.clone());
                        let h = rt.heap.alloc_record(2);
                        let _ = rt.heap.record_set_int(h, 0, DV_TEXT);
                        let _ = rt.heap.record_set_int(h, 1, sid);
                        h
                    }
                    wire::DBValue::Bool(b) => {
                        let h = rt.heap.alloc_record(2);
                        let _ = rt.heap.record_set_int(h, 0, DV_BOOL);
                        let _ = rt.heap.record_set_int(h, 1, i64::from(*b));
                        h
                    }
                    wire::DBValue::Blob(bytes) => {
                        let list = rt.heap.alloc_empty_list();
                        for byte in bytes {
                            let _ = rt.heap.list_push_int(list, i64::from(*byte));
                        }
                        let h = rt.heap.alloc_record(2);
                        let _ = rt.heap.record_set_int(h, 0, DV_BLOB);
                        let _ = rt.heap.record_set_int(h, 1, list);
                        h
                    }
                };
                let _ = rt.heap.map_insert(map, kid, vh);
            }
            let _ = rt.heap.list_push_int(list, map);
        }
        list
    })
}

fn jet_jit_db_open_memory() -> i64 {
    jit_job_queue_install_provider();
    runtime_open_memory() as i64
}

fn jet_jit_db_open(path: i64) -> i64 {
    jit_job_queue_install_provider();
    runtime_open(&clone_string(path)) as i64
}

fn jet_jit_db_close(handle: i64) -> i8 {
    let handle = handle as u64;
    if scope_parts(handle).is_some() {
        let base = base_handle(handle);
        DB_SCOPES.with(|scopes| {
            scopes.borrow_mut().remove(&handle);
        });
        i8::from(runtime_close(base))
    } else {
        i8::from(runtime_close(handle))
    }
}

fn jet_jit_db_begin(handle: i64) -> i8 {
    i8::from(runtime::jet_db_begin(base_handle(handle as u64)))
}

fn jet_jit_db_begin_mode(handle: i64, mode: i64) -> i8 {
    i8::from(runtime::jet_db_begin_mode(base_handle(handle as u64), mode))
}

fn jet_jit_db_commit(handle: i64) -> i8 {
    i8::from(runtime::jet_db_commit(base_handle(handle as u64)))
}

fn jet_jit_db_rollback(handle: i64) -> i8 {
    i8::from(runtime::jet_db_rollback(base_handle(handle as u64)))
}

fn jet_jit_db_execute(handle: i64, sql: i64) -> i64 {
    let Some(sql) = clone_sql_value(sql) else {
        return result_err_msg("malformed SQL value");
    };
    match scoped_execute(handle as u64, &sql, false) {
        Ok(n) => result_ok(n as u64),
        Err(error) => result_err_msg(&error.message),
    }
}

fn jet_jit_db_query(handle: i64, sql: i64) -> i64 {
    let Some(sql) = clone_sql_value(sql) else {
        return result_err_msg("malformed SQL value");
    };
    match scoped_query(handle as u64, &sql, false) {
        Ok(rows) => result_ok(rows_to_list_of_maps(rows) as u64),
        Err(error) => result_err_msg(&error.message),
    }
}

fn jet_jit_db_query_one(handle: i64, sql: i64) -> i64 {
    let Some(sql) = clone_sql_value(sql) else {
        return result_err_msg("malformed SQL value");
    };
    match scoped_query(handle as u64, &sql, false).map(wire::jet_db_first_row) {
        Ok(Ok(row)) => {
            let list = rows_to_list_of_maps(vec![row]);
            let map =
                Concurrency::with_runtime_mut(|rt| rt.heap.list_get_int(list, 0).unwrap_or(0));
            result_ok(map.wrapping_add(1) as u64)
        }
        Ok(Err(_)) => result_ok(0),
        Err(error) => result_err_msg(&error.message),
    }
}

fn jet_jit_db_execute_with_metadata(handle: i64, sql: i64, metadata: i64) -> i64 {
    jit_db_install_devtools_explain_callback();
    let Some(sql) = clone_sql_value(sql) else {
        return result_err_msg("malformed SQL value");
    };
    let metadata = clone_string(metadata);
    if let Err(error) = wire::JetDbQueryMetadata::from_wire(&metadata) {
        return result_err_msg(&error.message);
    }
    match scoped_execute_observed(handle as u64, &sql) {
        Ok((value, _)) => result_ok(value as u64),
        Err(error) => result_err_msg(&error.message),
    }
}

fn jet_jit_db_query_with_metadata(handle: i64, sql: i64, metadata: i64) -> i64 {
    jit_db_install_devtools_explain_callback();
    let Some(sql) = clone_sql_value(sql) else {
        return result_err_msg("malformed SQL value");
    };
    let metadata = clone_string(metadata);
    let metadata = match wire::JetDbQueryMetadata::from_wire(&metadata) {
        Ok(metadata) => metadata,
        Err(error) => return result_err_msg(&error.message),
    };
    jit_db_remember_observed_query(handle as u64, &sql, &metadata);
    match scoped_query_observed(handle as u64, &sql) {
        Ok((rows, _)) => result_ok(rows_to_list_of_maps(rows) as u64),
        Err(error) => result_err_msg(&error.message),
    }
}

fn jet_jit_db_query_one_with_metadata(handle: i64, sql: i64, metadata: i64) -> i64 {
    jit_db_install_devtools_explain_callback();
    let Some(sql) = clone_sql_value(sql) else {
        return result_err_msg("malformed SQL value");
    };
    let metadata = clone_string(metadata);
    let metadata = match wire::JetDbQueryMetadata::from_wire(&metadata) {
        Ok(metadata) => metadata,
        Err(error) => return result_err_msg(&error.message),
    };
    jit_db_remember_observed_query(handle as u64, &sql, &metadata);
    match scoped_query_observed(handle as u64, &sql)
        .map(|(rows, _)| rows)
        .map(wire::jet_db_first_row)
    {
        Ok(Ok(row)) => {
            let list = rows_to_list_of_maps(vec![row]);
            let map =
                Concurrency::with_runtime_mut(|rt| rt.heap.list_get_int(list, 0).unwrap_or(0));
            result_ok(map.wrapping_add(1) as u64)
        }
        Ok(Err(_)) => result_ok(0),
        Err(error) => result_err_msg(&error.message),
    }
}

fn list_of_sql(list: i64) -> Option<Vec<wire::SQL>> {
    value_handles_from_list(list)?
        .into_iter()
        .map(clone_sql_value)
        .collect()
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
fn scoped_execute_observed(
    scope: u64,
    sql: &wire::SQL,
) -> Result<(i64, wire::JetDbQueryRuntimeFacts), wire::DBError> {
    let Some((base, table, compiled, user, _)) = scope_parts(scope) else {
        return Err(wire::DBError {
            message: "database row operations require a policy scope".to_string(),
        });
    };
    let sql =
        wire::jet_db_apply_compiled_policy_with_proof(sql, &table, compiled, &user)?.into_sql()?;
    let result = runtime::jet_db_execute_observed(
        base,
        &sql.0,
        &wire::jet_db_encode_params(&sql.1),
    );
    wire::jet_db_decode_observed_execute_result(&result)
}

fn scoped_query_observed(
    scope: u64,
    sql: &wire::SQL,
) -> Result<(Vec<wire::JetDBRow>, wire::JetDbQueryRuntimeFacts), wire::DBError> {
    let Some((base, table, compiled, user, _)) = scope_parts(scope) else {
        return Err(wire::DBError {
            message: "database row operations require a policy scope".to_string(),
        });
    };
    let sql =
        wire::jet_db_apply_compiled_policy_with_proof(sql, &table, compiled, &user)?.into_sql()?;
    let result =
        runtime::jet_db_query_observed(base, &sql.0, &wire::jet_db_encode_params(&sql.1));
    wire::jet_db_decode_observed_query_result(&result)
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


fn jet_jit_db_migrate(conn: i64, name: i64, steps: i64) -> i64 {
    let steps_v = match list_of_sql(steps) {
        Some(steps) => steps,
        None => return result_err_msg("database migration steps must be SQL values"),
    };
    let name_s = clone_string(name);
    match run_scoped_migration(conn as u64, &name_s, &steps_v) {
        Ok(done) => {
            if done > 0 {
                invalidate_scoped_writes(&steps_v);
            }
            result_ok(done as u64)
        }
        Err(error) => result_err_msg(&error.message),
    }
}


fn jet_jit_db_transaction(conn: i64, label: i64, steps: i64) -> i64 {
    let steps_v = match list_of_sql(steps) {
        Some(steps) => steps,
        None => return result_err_msg("database transaction steps must be SQL values"),
    };
    let label_s = clone_string(label);
    match run_scoped_transaction(conn as u64, &label_s, &steps_v) {
        Ok(done) => {
            invalidate_scoped_writes(&steps_v);
            result_ok(done as u64)
        }
        Err(error) => result_err_msg(&error.message),
    }
}


fn jet_jit_db_row_int(row: i64, key: i64) -> i64 {
    let key_s = clone_string(key);
    let val = Concurrency::with_runtime_mut(|rt| {
        let kid = rt.heap.alloc_string(key_s.clone());
        rt.heap.map_get(row, kid)
    });
    match val.and_then(read_dbvalue) {
        Some(v) => match v.int() {
            Ok(n) => result_ok(n as u64),
            Err(error) => result_err_msg(&error),
        },
        None => result_err_msg(&format!("missing column `{key_s}`")),
    }
}

fn jet_jit_db_row_float(row: i64, key: i64) -> i64 {
    let key_s = clone_string(key);
    let val = Concurrency::with_runtime_mut(|rt| {
        let kid = rt.heap.alloc_string(key_s.clone());
        rt.heap.map_get(row, kid)
    });
    match val.and_then(read_dbvalue) {
        Some(v) => match v.float() {
            Ok(value) => result_ok(value.to_bits()),
            Err(error) => result_err_msg(&error),
        },
        None => result_err_msg(&format!("missing column `{key_s}`")),
    }
}

fn jet_jit_db_row_bool(row: i64, key: i64) -> i64 {
    let key_s = clone_string(key);
    let val = Concurrency::with_runtime_mut(|rt| {
        let kid = rt.heap.alloc_string(key_s.clone());
        rt.heap.map_get(row, kid)
    });
    match val.and_then(read_dbvalue) {
        Some(v) => match v.bool() {
            Ok(value) => result_ok(u64::from(value)),
            Err(error) => result_err_msg(&error),
        },
        None => result_err_msg(&format!("missing column `{key_s}`")),
    }
}

fn jet_jit_db_row_value(row: i64, key: i64) -> i64 {
    let key_s = clone_string(key);
    let val = Concurrency::with_runtime_mut(|rt| {
        let kid = rt.heap.alloc_string(key_s.clone());
        rt.heap.map_get(row, kid)
    });
    match val.and_then(read_dbvalue) {
        Some(value) => result_ok(alloc_dbvalue_value(value) as u64),
        None => result_err_msg(&format!("missing column `{key_s}`")),
    }
}

fn jet_jit_db_row_text(row: i64, key: i64) -> i64 {
    let key_s = clone_string(key);
    let val = Concurrency::with_runtime_mut(|rt| {
        let kid = rt.heap.alloc_string(key_s.clone());
        rt.heap.map_get(row, kid)
    });
    match val.and_then(read_dbvalue) {
        Some(v) => match v.text() {
            Ok(s) => {
                let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s));
                result_ok(sid as u64)
            }
            Err(e) => result_err_msg(&e),
        },
        None => result_err_msg(&format!("missing column `{key_s}`")),
    }
}

fn jet_jit_dbvalue_pack(disc: i64, payload: i64) -> i64 {
    if disc == DV_FLOAT {
        alloc_dbvalue_float(f64::from_bits(payload as u64))
    } else {
        alloc_dbvalue_record(disc, payload)
    }
}

fn jet_jit_dbvalue_int(handle: i64) -> i64 {
    match read_dbvalue(handle) {
        Some(v) => match v.int() {
            Ok(n) => result_ok(n as u64),
            Err(e) => result_err_msg(&e),
        },
        None => result_err_msg("invalid DBValue"),
    }
}

fn jet_jit_dbvalue_float(handle: i64) -> i64 {
    match read_dbvalue(handle) {
        Some(v) => match v.float() {
            Ok(f) => result_ok(f.to_bits()),
            Err(e) => result_err_msg(&e),
        },
        None => result_err_msg("invalid DBValue"),
    }
}

fn jet_jit_dbvalue_text(handle: i64) -> i64 {
    match read_dbvalue(handle) {
        Some(v) => match v.text() {
            Ok(s) => {
                let sid = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s));
                result_ok(sid as u64)
            }
            Err(e) => result_err_msg(&e),
        },
        None => result_err_msg("invalid DBValue"),
    }
}
fn jet_jit_dbvalue_blob(handle: i64) -> i64 {
    match read_dbvalue(handle) {
        Some(v) => match v.blob() {
            Ok(bytes) => result_ok(alloc_byte_list(&bytes) as u64),
            Err(e) => result_err_msg(&e),
        },
        None => result_err_msg("invalid DBValue"),
    }
}

fn jet_jit_dbvalue_bool(handle: i64) -> i64 {
    match read_dbvalue(handle) {
        Some(v) => match v.bool() {
            Ok(b) => result_ok(u64::from(b)),
            Err(e) => result_err_msg(&e),
        },
        None => result_err_msg("invalid DBValue"),
    }
}

fn jet_jit_dbvalue_is_null(handle: i64) -> i8 {
    match read_dbvalue(handle) {
        Some(v) => i8::from(v.is_null()),
        None => 0,
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

fn alloc_pool_receipt(receipt: &pool::JetDbPoolReceipt) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(13);
        let lifecycle = rt
            .heap
            .alloc_string(pool_lifecycle_name(receipt.lifecycle).to_string());
        let _ = rt.heap.record_set_string(record, 0, lifecycle);
        let values = [
            receipt.max,
            receipt.available,
            receipt.leased,
            receipt.opening,
            i64::from(receipt.ready),
            receipt.acquires,
            receipt.releases,
            receipt.timeouts,
            receipt.open_failures,
            receipt.unhealthy,
            receipt.replacements,
            receipt.replacement_failures,
        ];
        for (index, value) in values.into_iter().enumerate() {
            let _ = rt.heap.record_set_int(record, (index + 1) as i64, value);
        }
        record
    })
}

fn pool_value(handle: u64) -> CtValue {
    CtValue::Struct {
        type_name: "DbPool".to_string(),
        fields: vec![("handle".to_string(), CtValue::Int(handle as i64))],
    }
}

fn lease_value(handle: u64) -> CtValue {
    CtValue::Struct {
        type_name: "DbLease".to_string(),
        fields: vec![("handle".to_string(), CtValue::Int(handle as i64))],
    }
}

fn receipt_value(receipt: &pool::JetDbPoolReceipt) -> CtValue {
    CtValue::Struct {
        type_name: "DbPoolReceipt".to_string(),
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
    let handle = pool_id(receiver, "DbPool", span)?;
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
        let handle = match pool_id(receiver, "DbLease", span) {
            Ok(handle) => handle,
            Err(error) => return Some(Err(error)),
        };
        pool_lease_close(handle);
        return Some(Ok(CtValue::Unit));
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
    None
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
        let handle = match pool_id(receiver, "DbLease", span) {
            Ok(handle) => handle,
            Err(error) => return Some(Err(error)),
        };
        pool_lease_close(handle);
        return Some(Ok(CtValue::Unit));
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

fn jet_jit_db_pool_new(url: i64, max: i64) -> i64 {
    match pool_new(clone_string(url), max) {
        Ok(handle) => result_ok(handle),
        Err(error) => result_err_msg(&error),
    }
}

fn jet_jit_db_pool_acquire(handle: i64) -> i64 {
    match pool_acquire(handle as u64, None) {
        Ok(lease) => result_ok(lease),
        Err(error) => result_err_msg(&error),
    }
}

fn jet_jit_db_pool_acquire_deadline(handle: i64, deadline_ns: i64) -> i64 {
    let deadline = Some(
        jet_codegen::scheduler::jet_std_time_now()
            .saturating_add(deadline_ns.saturating_div(1_000_000)),
    );
    match pool_acquire(handle as u64, deadline) {
        Ok(lease) => result_ok(lease),
        Err(error) => result_err_msg(&error),
    }
}

fn jet_jit_db_pool_ready(handle: i64) -> i64 {
    match pool_ready(handle as u64) {
        Ok(ready) => result_ok(u64::from(ready)),
        Err(error) => result_err_msg(&error),
    }
}

fn jet_jit_db_pool_drain(handle: i64) -> i64 {
    match pool_drain(handle as u64, None) {
        Ok(receipt) => result_ok(alloc_pool_receipt(&receipt) as u64),
        Err(error) => result_err_msg(&error),
    }
}

fn jet_jit_db_pool_receipt(handle: i64) -> i64 {
    pool_receipt(handle as u64)
        .map(|receipt| alloc_pool_receipt(&receipt))
        .unwrap_or(0)
}
fn jet_jit_db_pool_lease_close(handle: i64) -> i64 {
    pool_lease_close(handle as u64);
    0
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

/// Release every native database owner associated with the resident image.
/// Console leases drop their typed proxies before this boundary, so removing
/// the canonical maps also closes returned pool drivers.
pub(crate) fn teardown_resident_resources() {
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

/// A live application-owned database resource made available to the console.
/// The resource carries the existing native owner; it never opens a path or
/// exposes the native handle.
pub struct ConsoleDbResource {
    name: String,
    identity: String,
    connection: ConsoleDbConnection,
}

impl ConsoleDbResource {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn identity(&self) -> &str {
        &self.identity
    }

    pub fn into_parts(self) -> (String, String, ConsoleDbConnection) {
        (self.name, self.identity, self.connection)
    }
}

pub(crate) fn capture_console_resources() -> Vec<ConsoleDbResource> {
    let direct = DB_CONNECTIONS.with(|connections| {
        connections.borrow().iter().copied().collect::<Vec<_>>()
    });
    let leases = DB_LEASES.with(|leases| {
        leases
            .borrow()
            .iter()
            .map(|(id, lease)| (*id, lease.clone()))
            .collect::<Vec<_>>()
    });
    let mut resources = Vec::new();
    for handle in direct {
        let connection = ConsoleDbConnection::from_owned_handle(handle);
        if connection.is_live() {
            resources.push(ConsoleDbResource {
                name: format!("resident-db-{handle}"),
                identity: format!("resident:db:{handle}"),
                connection,
            });
        }
    }
    for (id, lease) in leases {
        let connection = ConsoleDbConnection::from_pool_lease(lease);
        if connection.is_live() {
            resources.push(ConsoleDbResource {
                name: format!("resident-db-lease-{id}"),
                identity: format!("resident:db-lease:{id}"),
                connection,
            });
        }
    }
    resources
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

    pub fn open_memory() -> Result<Self, String> {
        let handle = runtime_open_memory();
        (handle != 0)
            .then_some(Self {
                execution: DbExecution::Owned(handle),
            })
            .ok_or_else(|| "in-memory database connection could not be opened".to_string())
    }

    fn from_owned_handle(handle: u64) -> Self {
        Self {
            execution: DbExecution::Owned(handle),
        }
    }

    fn from_pool_lease(lease: Arc<Mutex<pool::JetDbLease<u64>>>) -> Self {
        Self {
            execution: DbExecution::Lease(lease),
        }
    }

    pub fn is_live(&self) -> bool {
        with_execution(&self.execution, |handle| Ok(runtime::jet_db_health(handle))).unwrap_or(false)
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

host_fns! {
    struct DBHostFns;
    register: register_db_symbols;
    declare: declare_db_host_fns(module) {
        let cc = module.target_config().default_call_conv;
        let mut nullary = Signature::new(cc);
        nullary.returns.push(AbiParam::new(types::I64));
        let mut unary = Signature::new(cc);
        unary.params.push(AbiParam::new(types::I64));
        unary.returns.push(AbiParam::new(types::I64));
        let mut unary_i8 = Signature::new(cc);
        unary_i8.params.push(AbiParam::new(types::I64));
        unary_i8.returns.push(AbiParam::new(types::I8));
        let mut ternary = Signature::new(cc);
        for _ in 0..3 {
            ternary.params.push(AbiParam::new(types::I64));
        }
        ternary.returns.push(AbiParam::new(types::I64));
        let mut quaternary = Signature::new(cc);
        for _ in 0..4 {
            quaternary.params.push(AbiParam::new(types::I64));
        }
        quaternary.returns.push(AbiParam::new(types::I64));
        let mut binary = Signature::new(cc);
        binary.params.push(AbiParam::new(types::I64));
        binary.params.push(AbiParam::new(types::I64));
        binary.returns.push(AbiParam::new(types::I64));
        let mut binary_i8 = Signature::new(cc);
        binary_i8.params.push(AbiParam::new(types::I64));
        binary_i8.params.push(AbiParam::new(types::I64));
        binary_i8.returns.push(AbiParam::new(types::I8));
    }
    job_queue_default: "jet_jit_job_queue_default" => jet_jit_job_queue_default: nullary;
    job_queue_enqueue: "jet_jit_job_queue_enqueue" => jet_jit_job_queue_enqueue: quaternary;
    job_queue_delay: "jet_jit_job_queue_delay" => jet_jit_job_queue_delay: quaternary;
    job_queue_receipt: "jet_jit_job_queue_receipt" => jet_jit_job_queue_receipt: binary;
    job_queue_inspect: "jet_jit_job_queue_inspect" => jet_jit_job_queue_inspect: ternary;
    job_queue_events: "jet_jit_job_queue_events" => jet_jit_job_queue_events: binary;
    job_queue_claim: "jet_jit_job_queue_claim" => jet_jit_job_queue_claim: ternary;
    job_queue_heartbeat: "jet_jit_job_queue_heartbeat" => jet_jit_job_queue_heartbeat: binary;
    job_queue_acknowledge: "jet_jit_job_queue_acknowledge" => jet_jit_job_queue_acknowledge: ternary;
    job_queue_fail: "jet_jit_job_queue_fail" => jet_jit_job_queue_fail: ternary;
    job_queue_cancel: "jet_jit_job_queue_cancel" => jet_jit_job_queue_cancel: quaternary;
    job_queue_dead_letter: "jet_jit_job_queue_dead_letter" => jet_jit_job_queue_dead_letter: quaternary;
    job_queue_recover_expired: "jet_jit_job_queue_recover_expired" => jet_jit_job_queue_recover_expired: unary;
    job_queue_status: "jet_jit_job_queue_status" => jet_jit_job_queue_status: unary;
    job_queue_pause: "jet_jit_job_queue_pause" => jet_jit_job_queue_pause: unary;
    job_queue_resume: "jet_jit_job_queue_resume" => jet_jit_job_queue_resume: unary;
    job_queue_wait: "jet_jit_job_queue_wait" => jet_jit_job_queue_wait: binary;
    job_queue_prune: "jet_jit_job_queue_prune" => jet_jit_job_queue_prune: unary;
    open_memory: "jet_jit_db_open_memory" => jet_jit_db_open_memory: nullary;
    open: "jet_jit_db_open" => jet_jit_db_open: unary;
    row_value: "jet_jit_db_row_value" => jet_jit_db_row_value: binary;
    policy_audit: "jet_jit_db_policy_audit" => jet_jit_db_policy_audit: unary;
    begin_mode: "jet_jit_db_begin_mode" => jet_jit_db_begin_mode: binary_i8;
    policy: "jet_jit_db_policy" => jet_jit_db_policy: binary;
    with_policy: "jet_jit_db_with_policy" => jet_jit_db_with_policy: ternary;
    close: "jet_jit_db_close" => jet_jit_db_close: unary_i8;
    begin: "jet_jit_db_begin" => jet_jit_db_begin: unary_i8;
    commit: "jet_jit_db_commit" => jet_jit_db_commit: unary_i8;
    rollback: "jet_jit_db_rollback" => jet_jit_db_rollback: unary_i8;
    execute: "jet_jit_db_execute" => jet_jit_db_execute: binary;
    query: "jet_jit_db_query" => jet_jit_db_query: binary;
    query_one: "jet_jit_db_query_one" => jet_jit_db_query_one: binary;
    execute_with_metadata: "jet_jit_db_execute_with_metadata" => jet_jit_db_execute_with_metadata: ternary;
    query_with_metadata: "jet_jit_db_query_with_metadata" => jet_jit_db_query_with_metadata: ternary;
    query_one_with_metadata: "jet_jit_db_query_one_with_metadata" => jet_jit_db_query_one_with_metadata: ternary;
    migrate: "jet_jit_db_migrate" => jet_jit_db_migrate: ternary;
    transaction: "jet_jit_db_transaction" => jet_jit_db_transaction: ternary;
    migrate_canonical: "jet_db_scope_migrate" => jet_jit_db_migrate: ternary;
    transaction_canonical: "jet_db_scope_transaction" => jet_jit_db_transaction: ternary;
    row_float: "jet_jit_db_row_float" => jet_jit_db_row_float: binary;
    row_bool: "jet_jit_db_row_bool" => jet_jit_db_row_bool: binary;
    row_int: "jet_jit_db_row_int" => jet_jit_db_row_int: binary;
    row_text: "jet_jit_db_row_text" => jet_jit_db_row_text: binary;
    dbvalue_pack: "jet_jit_dbvalue_pack" => jet_jit_dbvalue_pack: binary;
    dbvalue_int: "jet_jit_dbvalue_int" => jet_jit_dbvalue_int: unary;
    dbvalue_float: "jet_jit_dbvalue_float" => jet_jit_dbvalue_float: unary;
    dbvalue_text: "jet_jit_dbvalue_text" => jet_jit_dbvalue_text: unary;
    dbvalue_blob: "jet_jit_dbvalue_blob" => jet_jit_dbvalue_blob: unary;
    dbvalue_bool: "jet_jit_dbvalue_bool" => jet_jit_dbvalue_bool: unary;
    dbvalue_is_null: "jet_jit_dbvalue_is_null" => jet_jit_dbvalue_is_null: unary_i8;
    pool_new: "jet_jit_db_pool_new" => jet_jit_db_pool_new: binary;
    pool_acquire: "jet_jit_db_pool_acquire" => jet_jit_db_pool_acquire: unary;
    pool_acquire_deadline: "jet_jit_db_pool_acquire_deadline" => jet_jit_db_pool_acquire_deadline: binary;
    pool_ready: "jet_jit_db_pool_ready" => jet_jit_db_pool_ready: unary;
    pool_drain: "jet_jit_db_pool_drain" => jet_jit_db_pool_drain: unary;
    pool_receipt: "jet_jit_db_pool_receipt" => jet_jit_db_pool_receipt: unary;
    pool_lease_close: "jet_jit_db_pool_lease_close" => jet_jit_db_pool_lease_close: unary;
    pool_new_canonical: "jet_db_pool_new" => jet_jit_db_pool_new: binary;
    pool_acquire_canonical: "jet_db_pool_acquire" => jet_jit_db_pool_acquire: unary;
    pool_acquire_deadline_canonical: "jet_db_pool_acquire_deadline" => jet_jit_db_pool_acquire_deadline: binary;
    pool_ready_canonical: "jet_db_pool_ready" => jet_jit_db_pool_ready: unary;
    pool_drain_canonical: "jet_db_pool_drain" => jet_jit_db_pool_drain: unary;
    pool_receipt_canonical: "jet_db_pool_receipt" => jet_jit_db_pool_receipt: unary;
    pool_lease_close_canonical: "jet_db_lease_close" => jet_jit_db_pool_lease_close: unary;
}
