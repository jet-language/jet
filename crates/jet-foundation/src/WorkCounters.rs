//! Deterministic work counters of the Rust compiler half (Tower #4601).
//!
//! A counter counts units of work, never time: tokens lexed, items visited,
//! scope lookups, entries of whole-program tables scanned, whole-program
//! copies, heap allocations. The same input always gives the same counts, so
//! the scaling gate (`Tools/perf/scaling`) can tell a superlinear phase from
//! timing noise and name the counter that grows fastest.
//!
//! Off by default: `add` costs one relaxed load and a branch. With
//! `JET_PHASE_COUNTERS` set the trace sink (`jet_driver::Trace`) enables the
//! counters and drains them whenever a phase span ends, so every count is
//! attributed to the phase that did the work and reported through the one
//! `counters.<phase>.<name>` protocol the Jet compiler half also uses.

use std::alloc::{GlobalAlloc, Layout};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// One kind of counted work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Work {
    /// Tokens produced by the lexer.
    Tokens,
    /// Top-level items produced by the parser.
    ItemsParsed,
    /// Top-level items whose declarations sema checked.
    ItemsChecked,
    /// Function bodies sema checked.
    FunctionsChecked,
    /// Local scope lookups of a name.
    ScopeLookups,
    /// Entries of a module's whole item list scanned by a per-use lookup.
    ItemScans,
    /// Impl or method rows scanned to resolve one method or trait.
    ImplScans,
    /// Copies of a whole module item list or another whole-program table.
    ProgramCopies,
    /// Elements in those whole-program copies.
    ProgramCopyElements,
    /// Functions lowered to MIR.
    FunctionsLowered,
    /// Functions emitted as backend text.
    FunctionsEmitted,
    /// Bytes of emitted backend text.
    EmittedBytes,
    /// Heap allocations (through `CountingAllocator`).
    Allocations,
    /// Bytes requested by those allocations.
    AllocatedBytes,
}

impl Work {
    pub const ALL: [Work; 14] = [
        Work::Tokens,
        Work::ItemsParsed,
        Work::ItemsChecked,
        Work::FunctionsChecked,
        Work::ScopeLookups,
        Work::ItemScans,
        Work::ImplScans,
        Work::ProgramCopies,
        Work::ProgramCopyElements,
        Work::FunctionsLowered,
        Work::FunctionsEmitted,
        Work::EmittedBytes,
        Work::Allocations,
        Work::AllocatedBytes,
    ];

    /// The counter name in `counters.<phase>.<name>`.
    pub const fn name(self) -> &'static str {
        match self {
            Work::Tokens => "tokens",
            Work::ItemsParsed => "items_parsed",
            Work::ItemsChecked => "items_checked",
            Work::FunctionsChecked => "functions_checked",
            Work::ScopeLookups => "scope_lookups",
            Work::ItemScans => "item_scans",
            Work::ImplScans => "impl_scans",
            Work::ProgramCopies => "program_copies",
            Work::ProgramCopyElements => "program_copy_elements",
            Work::FunctionsLowered => "functions_lowered",
            Work::FunctionsEmitted => "functions_emitted",
            Work::EmittedBytes => "emitted_bytes",
            Work::Allocations => "allocations",
            Work::AllocatedBytes => "allocated_bytes",
        }
    }
}

static ENABLED: AtomicBool = AtomicBool::new(false);
static COUNTS: [AtomicU64; Work::ALL.len()] = [const { AtomicU64::new(0) }; Work::ALL.len()];

/// Start counting (the trace sink calls this when `JET_PHASE_COUNTERS` is set).
pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

/// Whether counters are on; a caller that must compute an amount checks first.
#[inline]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Count `amount` units of `work`.
#[inline]
pub fn add(work: Work, amount: usize) {
    if enabled() {
        COUNTS[work as usize].fetch_add(amount as u64, Ordering::Relaxed);
    }
}

/// The counts since the last drain, nonzero ones only, reset to zero.
pub fn drain() -> Vec<(&'static str, u64)> {
    Work::ALL
        .iter()
        .filter_map(|work| {
            let value = COUNTS[*work as usize].swap(0, Ordering::Relaxed);
            (value > 0).then_some((work.name(), value))
        })
        .collect()
}

/// A global allocator that counts allocations and requested bytes while the
/// counters are on, delegating every call to the wrapped allocator.
pub struct CountingAllocator<A>(pub A);

// SAFETY: every method forwards the caller's pointer and layout unchanged to
// the wrapped allocator, which upholds the `GlobalAlloc` contract; counting
// touches only process-wide atomics and never allocates.
unsafe impl<A: GlobalAlloc> GlobalAlloc for CountingAllocator<A> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count_allocation(layout.size());
        // SAFETY: the caller's layout is forwarded unchanged.
        unsafe { self.0.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count_allocation(layout.size());
        // SAFETY: the caller's layout is forwarded unchanged.
        unsafe { self.0.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` and `layout` come from this allocator's `alloc`.
        unsafe { self.0.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count_allocation(new_size);
        // SAFETY: `ptr` and `layout` come from this allocator's `alloc`.
        unsafe { self.0.realloc(ptr, layout, new_size) }
    }
}

#[inline]
fn count_allocation(bytes: usize) {
    if enabled() {
        COUNTS[Work::Allocations as usize].fetch_add(1, Ordering::Relaxed);
        COUNTS[Work::AllocatedBytes as usize].fetch_add(bytes as u64, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_only_while_enabled_and_drain_resets() {
        add(Work::ItemScans, 5);
        assert!(drain().iter().all(|(name, _)| *name != "item_scans"));
        enable();
        add(Work::ItemScans, 5);
        add(Work::ItemScans, 2);
        let drained = drain();
        assert!(drained.contains(&("item_scans", 7)));
        assert!(drain().iter().all(|(name, _)| *name != "item_scans"));
    }
}
