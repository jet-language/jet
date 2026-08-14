//! D-MEM-SENTRY1 runtime witness kernel.
//!
//! The kernel owns gate state, allocation provenance, quarantine, and the
//! R08xx fault facts. AOT embeds the equivalent `jet_mem` Prelude because its
//! generated binary cannot link the compiler seam crate; JIT and TIR call this
//! Foundation Prelude directly. No adapter owns report wording or policy.

use std::cell::RefCell;
use std::sync::{Mutex, OnceLock};

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

thread_local! {
    static GATE: RefCell<Option<Gate>> = const { RefCell::new(None) };
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
}

impl Drop for JetSentryGuard {
    fn drop(&mut self) {
        GATE.with(|gate| *gate.borrow_mut() = self.saved.take());
    }
}

pub fn jet_sentry_scope(enabled: bool, file: &str, line: u32, reason: &str) -> JetSentryGuard {
    let saved = GATE.with(|gate| {
        gate.borrow_mut().replace(Gate {
            enabled: enabled && cfg!(not(jet_release)),
            file: file.to_string(),
            line,
            reason: reason.to_string(),
        })
    });
    JetSentryGuard { saved }
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
        current.enabled = enabled && cfg!(not(jet_release));
        std::mem::replace(&mut *gate, Some(current))
    });
    JetSentryGuard { saved }
}

pub fn jet_sentry_reset() {
    GATE.with(|gate| *gate.borrow_mut() = None);
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
    if !cfg!(not(jet_release)) || start == 0 {
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
    if !cfg!(not(jet_release)) || start == 0 {
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
    if !cfg!(not(jet_release)) {
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
    if !cfg!(not(jet_release)) {
        return None;
    }
    let gate = GATE.with(|gate| gate.borrow().clone())?;
    if !gate.enabled {
        return None;
    }
    let bytes = bytes.max(1);
    let alignment = alignment.max(1);
    let code_detail = if start % alignment != 0 {
        Some(("R0803", format!("address {start:#x} is not aligned to {alignment} bytes")))
    } else {
        let end = start.checked_add(bytes);
        let records = allocations()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let live = end.is_some_and(|end| records.iter().rev().any(|allocation| {
            allocation.live
                && start >= allocation.start
                && end <= allocation.start.saturating_add(allocation.len)
        }));
        if live {
            None
        } else {
            let freed = records.iter().rev().any(|allocation| {
                !allocation.live
                    && start >= allocation.start
                    && start < allocation.start.saturating_add(allocation.len)
            });
                Some(if freed {
                    ("R0802", "the address belongs to quarantined storage".to_string())
                } else {
                    ("R0801", "no live allocation contains this address".to_string())
                })
        }
    }?;
    let name = gate_name(&gate);
    Some(JetSentryFault {
        code: code_detail.0,
        file: gate.file,
        line: gate.line,
        gate: name,
        operation: operation.to_string(),
        obligation: obligation.to_string(),
        detail: code_detail.1,
    })
}
