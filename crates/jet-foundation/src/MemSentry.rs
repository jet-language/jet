//! D-MEM-SENTRY1 runtime witness kernel.
//!
//! The kernel owns gate state, allocation provenance, quarantine, and the
//! R08xx fault facts. AOT embeds this exact source under its `jet_mem` Prelude
//! because its generated binary cannot link the compiler seam crate; JIT and
//! TIR call this Foundation Prelude directly. No adapter owns report wording
//! or policy.

use std::cell::{Cell, RefCell};
use std::io::Write as IOWrite;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Mutex, OnceLock,
};

#[derive(Clone)]
struct Gate {
    enabled: bool,
    file: String,
    line: u32,
    reason: String,
}

#[derive(Clone, Copy)]
struct Allocation {
    start: usize,
    len: usize,
    live: bool,
    owner: Option<usize>,
    stack_frame: Option<usize>,
}

static ALLOCATIONS: OnceLock<Mutex<Vec<Allocation>>> = OnceLock::new();
static HARDENED: AtomicBool = AtomicBool::new(false);
static NEXT_SENTRY_FRAME: AtomicUsize = AtomicUsize::new(1);
const MAX_MEMORY_LEDGER_BYTES: u64 = 4 * 1024 * 1024;
static MEMORY_LEDGER_LOCK: Mutex<()> = Mutex::new(());

/// One exercised runtime witness for `jet audit memory` (card #1895).
///
/// This source is a Foundation Prelude part: AOT embeds it verbatim and the
/// hosted tiers call it directly, so ledger persistence has one meaning.
pub struct MemoryLedgerWitness<'a> {
    pub kind: &'a str,
    pub code: &'a str,
    pub source: &'a str,
    pub span_start: u64,
    pub span_end: u64,
    pub byte_spans: bool,
    pub scope: &'a str,
    pub provenance: &'a str,
    pub detail: &'a str,
    pub expected: Option<&'a str>,
    pub repairs: &'a [&'a str],
}

fn memory_ledger_path() -> Option<PathBuf> {
    std::env::var_os("JET_MEMORY_LEDGER")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|root| root.join(".jet").join("memory").join("ledger-v1.jsonl"))
        })
}

fn memory_ledger_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if ch.is_control() => escaped.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => escaped.push(ch),
        }
    }
    escaped
}

/// Append one bounded JSON-lines row. Ledger I/O is observational: a failed
/// write never changes the program's memory meaning or suppresses its fault.
pub fn jet_memory_ledger_record(witness: MemoryLedgerWitness<'_>) -> Result<(), String> {
    let Some(path) = memory_ledger_path() else {
        return Ok(());
    };
    let _guard = MEMORY_LEDGER_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    match std::fs::symlink_metadata(parent) {
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err("memory ledger directory is not a real directory".to_string());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create memory ledger directory: {error}"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                    .map_err(|error| format!("cannot secure memory ledger directory: {error}"))?;
            }
        }
        Err(error) => return Err(format!("cannot inspect memory ledger directory: {error}")),
    }
    let existing = match std::fs::symlink_metadata(&path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err("memory ledger path is not a regular file".to_string());
        }
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => return Err(format!("cannot inspect memory ledger: {error}")),
    };
    let repairs = witness
        .repairs
        .iter()
        .map(|repair| format!("\"{}\"", memory_ledger_escape(repair)))
        .collect::<Vec<_>>()
        .join(",");
    let expected = witness
        .expected
        .map(|value| format!("\"{}\"", memory_ledger_escape(value)))
        .unwrap_or_else(|| "null".to_string());
    let row = format!(
        "{{\"schema\":\"jet.memory.ledger\",\"version\":1,\"kind\":\"{}\",\"code\":\"{}\",\"source\":\"{}\",\"span_start\":{},\"span_end\":{},\"byte_spans\":{},\"scope\":\"{}\",\"provenance\":\"{}\",\"detail\":\"{}\",\"expected\":{},\"repairs\":[{}]}}\n",
        memory_ledger_escape(witness.kind),
        memory_ledger_escape(witness.code),
        memory_ledger_escape(witness.source),
        witness.span_start,
        witness.span_end,
        witness.byte_spans,
        memory_ledger_escape(witness.scope),
        memory_ledger_escape(witness.provenance),
        memory_ledger_escape(witness.detail),
        expected,
        repairs,
    );
    if existing.saturating_add(row.len() as u64) > MAX_MEMORY_LEDGER_BYTES {
        return Err("memory ledger exceeds its 4 MiB safety limit".to_string());
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .map_err(|error| format!("cannot open memory ledger: {error}"))?;
    file.write_all(row.as_bytes())
        .and_then(|()| file.flush())
        .map_err(|error| format!("cannot append memory ledger: {error}"))
}

thread_local! {
    static GATE: RefCell<Option<Gate>> = const { RefCell::new(None) };
    static FENCE_DEPTH: Cell<usize> = const { Cell::new(0) };
    static SENTRY_FRAMES: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

fn allocations() -> &'static Mutex<Vec<Allocation>> {
    ALLOCATIONS.get_or_init(|| Mutex::new(Vec::new()))
}

fn gate_name(gate: &Gate) -> String {
    if gate.reason.is_empty() {
        format!("{}:{}", gate.file, gate.line)
    } else {
        gate.reason.clone()
    }
}

pub struct JetSentryGuard {
    saved: Option<Gate>,
    saved_fence_depth: usize,
}

impl Drop for JetSentryGuard {
    fn drop(&mut self) {
        GATE.with(|gate| *gate.borrow_mut() = self.saved.take());
        FENCE_DEPTH.with(|depth| depth.set(self.saved_fence_depth));
    }
}

/// One runtime lifetime token for stack storage minted by `mem.address_of` or
/// raw-of. The token is deliberately separate from `JetSentryGuard`: a source
/// `#Unsafe` gate controls whether an access is checked, while this token owns
/// the lifetime of the stack allocation fact. AOT, JIT/TIR, and the embedded
/// Prelude all use this same RAII boundary.
pub struct JetSentryFrame {
    id: Option<usize>,
}

impl Drop for JetSentryFrame {
    fn drop(&mut self) {
        let Some(id) = self.id.take() else {
            return;
        };
        SENTRY_FRAMES.with(|frames| {
            let mut frames = frames.borrow_mut();
            if frames.last().copied() == Some(id) {
                frames.pop();
            } else {
                // Normal generated code drops frames in LIFO order. Retain a
                // safe recovery path for an explicit early drop so a stale
                // token can never remain the current owner.
                frames.retain(|active| *active != id);
            }
        });
        let mut records = allocations()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for allocation in records.iter_mut() {
            if allocation.live && allocation.stack_frame == Some(id) {
                allocation.live = false;
            }
        }
    }
}

fn runtime_available() -> bool {
    cfg!(not(jet_release)) || HARDENED.load(Ordering::Relaxed) || FENCE_DEPTH.with(Cell::get) != 0
}

fn jet_sentry_scope_inner(
    enabled: bool,
    fenced: bool,
    file: &str,
    line: u32,
    reason: &str,
) -> JetSentryGuard {
    let saved_fence_depth = FENCE_DEPTH.with(|depth| {
        let saved = depth.get();
        if fenced {
            depth.set(saved.saturating_add(1));
        }
        saved
    });
    let saved = GATE.with(|gate| {
        gate.borrow_mut().replace(Gate {
            enabled: enabled && runtime_available(),
            file: file.to_string(),
            line,
            reason: reason.to_string(),
        })
    });
    JetSentryGuard {
        saved,
        saved_fence_depth,
    }
}

pub fn jet_sentry_scope(enabled: bool, file: &str, line: u32, reason: &str) -> JetSentryGuard {
    jet_sentry_scope_inner(enabled, false, file, line, reason)
}

pub fn jet_sentry_fenced_scope(
    enabled: bool,
    file: &str,
    line: u32,
    reason: &str,
) -> JetSentryGuard {
    jet_sentry_scope_inner(enabled, true, file, line, reason)
}

pub fn jet_sentry_policy_scope(enabled: bool) -> JetSentryGuard {
    let saved = GATE.with(|gate| {
        let mut gate = gate.borrow_mut();
        let mut current = (*gate).clone().unwrap_or(Gate {
            enabled: false,
            file: String::new(),
            line: 0,
            reason: String::new(),
        });
        current.enabled = enabled && runtime_available();
        std::mem::replace(&mut *gate, Some(current))
    });
    let saved_fence_depth = FENCE_DEPTH.with(Cell::get);
    JetSentryGuard {
        saved,
        saved_fence_depth,
    }
}

pub fn jet_sentry_set_hardened(enabled: bool) {
    HARDENED.store(enabled, Ordering::Relaxed);
}

pub fn jet_sentry_reset() {
    GATE.with(|gate| *gate.borrow_mut() = None);
    FENCE_DEPTH.with(|depth| depth.set(0));
    SENTRY_FRAMES.with(|frames| frames.borrow_mut().clear());
    HARDENED.store(false, Ordering::Relaxed);
    allocations()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
}

pub fn jet_sentry_register_allocation(start: usize, bytes: usize) {
    jet_sentry_register_allocation_inner(None, None, start, bytes);
}

pub fn jet_sentry_register_owned_allocation(owner: usize, start: usize, bytes: usize) {
    jet_sentry_register_allocation_inner(Some(owner), None, start, bytes);
}

/// Return the active Jet frame token on this execution context. The numeric
/// token is globally unique, so synthetic TIR identities and task workers do
/// not alias another frame's stack storage.
pub fn jet_sentry_current_frame() -> Option<usize> {
    SENTRY_FRAMES.with(|frames| frames.borrow().last().copied())
}

/// Enter one stack-lifetime scope. The caller only emits this hook for a body
/// that can name stack storage. Registration remains gated by
/// `runtime_available`, so an unwatched release records no allocation; keeping
/// the token itself unconditional lets a fenced scope activate observation
/// after the enclosing function has already started.
pub fn jet_sentry_frame() -> JetSentryFrame {
    let id = NEXT_SENTRY_FRAME.fetch_add(1, Ordering::Relaxed);
    SENTRY_FRAMES.with(|frames| frames.borrow_mut().push(id));
    JetSentryFrame { id: Some(id) }
}

/// Register storage owned by the currently active Jet frame. No active frame
/// means no registration: stack storage must never become process-global live
/// state merely because an address was observed.
pub fn jet_sentry_register_stack_allocation(start: usize, bytes: usize) {
    let Some(frame) = jet_sentry_current_frame() else {
        return;
    };
    jet_sentry_register_allocation_inner(None, Some(frame), start, bytes);
}

fn jet_sentry_register_allocation_inner(
    owner: Option<usize>,
    stack_frame: Option<usize>,
    start: usize,
    bytes: usize,
) {
    if !runtime_available() || start == 0 {
        return;
    }
    allocations()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(Allocation {
            start,
            len: bytes.max(1),
            live: true,
            owner,
            stack_frame,
        });
}

pub fn jet_sentry_quarantine(start: usize, bytes: usize) {
    if !runtime_available() || start == 0 {
        return;
    }
    let mut records = allocations()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut poisoned = false;
    for allocation in records.iter_mut().rev() {
        if allocation.live && allocation.start == start {
            allocation.live = false;
            poisoned = true;
        }
    }
    if poisoned && bytes != 0 {
        // SAFETY: the caller releases the exact allocation previously
        // registered at this address, before poisoning its bytes.
        unsafe { std::ptr::write_bytes(start as *mut u8, 0xDD, bytes) };
    }
}

pub fn jet_sentry_quarantine_owner(owner: usize) {
    if !runtime_available() {
        return;
    }
    let mut records = allocations()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for allocation in records.iter_mut() {
        if allocation.live && allocation.owner == Some(owner) {
            allocation.live = false;
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetSentryFault {
    pub code: &'static str,
    pub file: String,
    pub line: u32,
    pub gate: String,
    pub operation: String,
    pub obligation: String,
    pub detail: String,
}

pub fn jet_sentry_check(
    start: usize,
    bytes: usize,
    alignment: usize,
    operation: &str,
    obligation: &str,
) -> Option<JetSentryFault> {
    jet_sentry_check_inner(start, bytes, alignment, operation, obligation, true)
}

/// Check a pointer that is about to cross a foreign boundary.
///
/// A borrowed Rust reference can point at ordinary stack or foreign-owned
/// storage that Jet did not allocate, so an unknown non-null address is legal
/// at this boundary. Tracked storage still gets the same liveness, range, and
/// alignment witness as a raw operation; raw `Ptr<T>` arguments use
/// [`jet_sentry_check`] instead and therefore require tracked provenance.
pub fn jet_sentry_check_foreign(
    start: usize,
    bytes: usize,
    alignment: usize,
    operation: &str,
    obligation: &str,
) -> Option<JetSentryFault> {
    jet_sentry_check_inner(start, bytes, alignment, operation, obligation, false)
}

fn jet_sentry_check_inner(
    start: usize,
    bytes: usize,
    alignment: usize,
    operation: &str,
    obligation: &str,
    require_provenance: bool,
) -> Option<JetSentryFault> {
    let gate = GATE.with(|gate| gate.borrow().clone())?;
    if !gate.enabled {
        return None;
    }
    let bytes = bytes.max(1);
    let alignment = alignment.max(1);
    let end = start.checked_add(bytes);
    let records = allocations()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let live = end.is_some_and(|end| {
        records.iter().rev().any(|allocation| {
            allocation.live
                && start >= allocation.start
                && end <= allocation.start.saturating_add(allocation.len)
        })
    });
    let starts_in_live = records.iter().rev().any(|allocation| {
        allocation.live
            && start >= allocation.start
            && start <= allocation.start.saturating_add(allocation.len)
    });
    // Provenance classification precedes alignment for raw accesses: an
    // untracked address is R0801. A foreign borrowed reference may be
    // untracked, but a tracked allocation that does not contain the complete
    // requested range is still a boundary violation.
    let code_detail = if live {
        if start % alignment != 0 {
            Some((
                "R0803",
                format!("address {start:#x} is not aligned to {alignment} bytes"),
            ))
        } else {
            None
        }
    } else {
        let freed = records.iter().rev().find(|allocation| {
            !allocation.live
                && start >= allocation.start
                && start < allocation.start.saturating_add(allocation.len)
        });
        if let Some(freed) = freed {
            let detail = if freed.stack_frame.is_some() {
                "the owning Jet frame has expired"
            } else {
                "the address belongs to quarantined storage"
            };
            Some(("R0802", detail.to_string()))
        } else if starts_in_live {
            Some((
                "R0801",
                "the foreign access extends beyond a live allocation".to_string(),
            ))
        } else if require_provenance || start == 0 || end.is_none() {
            Some((
                "R0801",
                "no live allocation contains this address".to_string(),
            ))
        } else if start % alignment != 0 {
            Some((
                "R0803",
                format!("address {start:#x} is not aligned to {alignment} bytes"),
            ))
        } else {
            None
        }
    }?;
    let name = gate_name(&gate);
    let repairs: &[&str] = match code_detail.0 {
        "R0802" => &[
            "move the raw access before the storage is released",
            "replace the raw pointer with an owned value",
        ],
        "R0803" => &[
            "derive an aligned pointer from the live allocation",
            "use the typed memory operation instead of raw address arithmetic",
        ],
        _ => &[
            "derive the pointer from live storage inside this gate",
            "remove the raw access",
        ],
    };
    let fault = JetSentryFault {
        code: code_detail.0,
        file: gate.file,
        line: gate.line,
        gate: name,
        operation: operation.to_string(),
        obligation: obligation.to_string(),
        detail: code_detail.1,
    };
    let _ = jet_memory_ledger_record(MemoryLedgerWitness {
        kind: "sentry",
        code: fault.code,
        source: &fault.file,
        span_start: u64::from(fault.line),
        span_end: u64::from(fault.line),
        byte_spans: false,
        provenance: "source #Unsafe gate",
        scope: &fault.gate,
        detail: &fault.detail,
        expected: None,
        repairs,
    });
    Some(fault)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn stack_registration_is_live_only_inside_its_frame() {
        let _serial = TEST_LOCK.lock().unwrap();
        jet_sentry_reset();
        jet_sentry_set_hardened(true);
        let mut value = 7i64;
        let frame = jet_sentry_frame();
        let address = (&mut value as *mut i64) as usize;
        let gate = jet_sentry_scope(true, "stack.jet", 1, "same-scope");
        jet_sentry_register_stack_allocation(address, std::mem::size_of::<i64>());
        assert!(jet_sentry_check(address, 8, 8, "read", "valid_ptr").is_none());

        drop(frame);
        let fault = jet_sentry_check(address, 8, 8, "read", "valid_ptr")
            .expect("a stack allocation must expire with its frame");
        assert_eq!(fault.code, "R0802");
        assert_eq!(fault.detail, "the owning Jet frame has expired");
        drop(gate);
        jet_sentry_reset();
    }

    #[test]
    fn frame_expiry_does_not_expire_heap_or_foreign_storage() {
        let _serial = TEST_LOCK.lock().unwrap();
        jet_sentry_reset();
        jet_sentry_set_hardened(true);
        let mut value = 17i64;
        let frame = jet_sentry_frame();
        let address = (&mut value as *mut i64) as usize;
        let gate = jet_sentry_scope(true, "persistent.jet", 1, "persistent");
        jet_sentry_register_allocation(address, 8);
        drop(frame);
        assert!(jet_sentry_check(address, 8, 8, "read", "valid_ptr").is_none());
        assert!(jet_sentry_check_foreign(0x1000, 8, 8, "ffi_ptr", "ffi_contract").is_none());

        jet_sentry_quarantine(address, 8);
        let fault = jet_sentry_check(address, 8, 8, "read", "valid_ptr")
            .expect("allocator quarantine must still expire persistent storage");
        assert_eq!(fault.code, "R0802");
        assert_eq!(fault.detail, "the address belongs to quarantined storage");
        drop(gate);
        jet_sentry_reset();
    }

    #[test]
    fn nested_and_thread_frames_have_distinct_lifetimes() {
        let _serial = TEST_LOCK.lock().unwrap();
        jet_sentry_reset();
        jet_sentry_set_hardened(true);
        let mut value = 11i64;
        let gate = jet_sentry_scope(true, "nested.jet", 1, "nested-frame");
        let outer = jet_sentry_frame();
        let outer_id = jet_sentry_current_frame().expect("outer frame token");
        let address = (&mut value as *mut i64) as usize;
        jet_sentry_register_stack_allocation(address, 8);
        let inner = jet_sentry_frame();
        let inner_id = jet_sentry_current_frame().expect("inner frame token");
        assert_ne!(outer_id, inner_id);
        jet_sentry_register_stack_allocation(address, 8);
        drop(inner);
        assert!(jet_sentry_check(address, 8, 8, "read", "valid_ptr").is_none());
        drop(outer);
        let fault = jet_sentry_check(address, 8, 8, "read", "valid_ptr")
            .expect("all nested stack registrations must expire with the outer frame");
        assert_eq!(fault.code, "R0802");

        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let _gate = jet_sentry_scope(true, "task.jet", 2, "task-frame");
                    let mut task_value = 13i64;
                    let task_frame = jet_sentry_frame();
                    let task_id = jet_sentry_current_frame().expect("task frame token");
                    assert_ne!(outer_id, task_id);
                    let task_address = (&mut task_value as *mut i64) as usize;
                    jet_sentry_register_stack_allocation(task_address, 8);
                    assert!(jet_sentry_check(task_address, 8, 8, "read", "valid_ptr").is_none());
                    drop(task_frame);
                    let fault = jet_sentry_check(task_address, 8, 8, "read", "valid_ptr")
                        .expect("task stack allocation must expire with its task frame");
                    assert_eq!(fault.code, "R0802");
                })
                .join()
                .expect("task frame test");
        });
        drop(gate);
        jet_sentry_reset();
    }
}
