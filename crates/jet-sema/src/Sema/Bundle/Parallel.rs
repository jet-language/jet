//! Parallel sema work: a bounded pool of compiler-stack workers that see the
//! same thread-local compiler context as the checking thread.
//!
//! Callers hand in independent jobs and get the results back in job order,
//! then merge them serially in source order. That keeps every diagnostic,
//! fact and checked body identical to a serial run; `JET_CHECK_THREADS=1`
//! runs the same jobs inline, in order, for debugging and for comparing the
//! two.

/// Default upper bound on sema workers. Each worker holds a full checker and
/// may lower whole-module comptime fragments, so the peak grows with the
/// worker count; `JET_CHECK_THREADS` sets a different count explicitly.
const MAX_CHECK_WORKERS: usize = 8;

/// Workers for `jobs` independent jobs: `JET_CHECK_THREADS` when set to a
/// positive number, otherwise the machine's available parallelism, capped by
/// the job count and `MAX_CHECK_WORKERS`. The setting only changes how the
/// work is scheduled, never its result, so it is read directly rather than
/// through `CheckReads` (it is not an input of the checked program).
pub(crate) fn check_worker_count(jobs: usize) -> usize {
    let requested = std::env::var("JET_CHECK_THREADS")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|threads| *threads > 0);
    let threads = requested.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(std::num::NonZeroUsize::get)
            .unwrap_or(1)
            .min(MAX_CHECK_WORKERS)
    });
    threads.min(jobs).max(1)
}

/// Run `work` on every job with `check_worker_count` workers and return the
/// results in job order. Each worker runs with the checking thread's package
/// edition, comptime ambient hooks and terminator-pass driver, the
/// thread-local context body checking reads.
pub(crate) fn map_checked<T: Send, R: Send>(
    jobs: Vec<T>,
    work: impl Fn(T) -> R + Sync,
) -> Vec<R> {
    let workers = check_worker_count(jobs.len());
    if workers <= 1 {
        return jobs.into_iter().map(work).collect();
    }
    let edition = jet_foundation::PackageEdition::package_edition();
    let ambient = crate::Comptime::ambient_runtime_snapshot();
    let terminator_driver = crate::Lexer::terminator_driver();
    jet_foundation::CompilerStack::map_on_compiler_workers(jobs, workers, |job| {
        jet_foundation::PackageEdition::with_package_edition(&edition, || {
            crate::Lexer::with_terminator_driver(terminator_driver.clone(), || {
                crate::Comptime::with_ambient_runtime_snapshot(ambient.clone(), || work(job))
            })
        })
    })
}
