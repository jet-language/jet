//! The compiler's one thread cap (D-JOBS1=A).
//!
//! `jet build|run|check|test|dev|inspect --threads N` installs the cap once,
//! before any compiling starts. Every compiler pool (the loader's lex/parse
//! fan-out, the checker's body workers, the JIT's define batch) asks
//! [`admit`] how many threads it may use, so one number bounds them all.
//! Without the flag the pools admit threads automatically, up to the cores
//! the system reports. The cap limits the compiler only, never the threads
//! of a program jet builds or runs, and every thread count gives the same
//! diagnostics and output bytes.

use std::sync::atomic::{AtomicUsize, Ordering};

/// The `--threads` cap; `0` while the flag is absent.
static CAP: AtomicUsize = AtomicUsize::new(0);

/// Install the `--threads N` cap (N >= 1) for this process.
pub fn set_cap(threads: usize) {
    CAP.store(threads.max(1), Ordering::Relaxed);
}

/// The `--threads N` cap, or `None` when the flag is absent.
pub fn cap() -> Option<usize> {
    match CAP.load(Ordering::Relaxed) {
        0 => None,
        threads => Some(threads),
    }
}

/// The cores the system reports (at least 1).
pub fn cores() -> usize {
    std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1)
}

/// The threads one compiler pool may use for `jobs` independent jobs.
/// Admission starts at the cap, or, without one, at the pool's own
/// `automatic_limit` (the bound its per-worker memory allows); it never
/// exceeds the reported cores or the job count, and is at least 1.
pub fn admit(jobs: usize, automatic_limit: usize) -> usize {
    cap()
        .unwrap_or(automatic_limit)
        .min(cores())
        .min(jobs)
        .max(1)
}

/// The build report's compiler-threads line.
pub fn report() -> String {
    match cap() {
        Some(1) => "at most 1 compiler thread (--threads 1)".to_string(),
        Some(threads) => format!("at most {threads} compiler threads (--threads {threads})"),
        None => format!("automatic, up to {} compiler threads", cores()),
    }
}
