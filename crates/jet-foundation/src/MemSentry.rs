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
    atomic::{AtomicBool, Ordering},
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
}

static ALLOCATIONS: OnceLock<Mutex<Vec<Allocation>>> = OnceLock::new();
static HARDENED: AtomicBool = AtomicBool::new(false);
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
    HARDENED.store(false, Ordering::Relaxed);
    allocations()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
}

pub fn jet_sentry_register_allocation(start: usize, bytes: usize) {
    jet_sentry_register_owned_allocation_inner(None, start, bytes);
}

pub fn jet_sentry_register_owned_allocation(owner: usize, start: usize, bytes: usize) {
    jet_sentry_register_owned_allocation_inner(Some(owner), start, bytes);
}

fn jet_sentry_register_owned_allocation_inner(owner: Option<usize>, start: usize, bytes: usize) {
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
    // Provenance classification precedes alignment: an untracked address is R0801.
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
        let freed = records.iter().rev().any(|allocation| {
            !allocation.live
                && start >= allocation.start
                && start < allocation.start.saturating_add(allocation.len)
        });
        Some(if freed {
            (
                "R0802",
                "the address belongs to quarantined storage".to_string(),
            )
        } else {
            (
                "R0801",
                "no live allocation contains this address".to_string(),
            )
        })
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
