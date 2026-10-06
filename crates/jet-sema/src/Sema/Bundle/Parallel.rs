//! Parallel sema work: a bounded pool of compiler-stack workers that see the
//! same thread-local compiler context as the checking thread.
//!
//! Callers hand in independent jobs and get the results back in job order,
//! then merge them serially in source order. That keeps every diagnostic,
//! fact and checked body identical to a serial run; `--threads 1` runs the
//! same jobs inline, in order, for debugging and for comparing the two.

/// Automatic upper bound on sema workers. Each worker holds a full checker
/// and may lower whole-module comptime fragments, so the peak grows with the
/// worker count; `--threads N` sets a different cap explicitly.
const MAX_CHECK_WORKERS: usize = 8;

/// Workers for `jobs` independent jobs, admitted under the compiler's one
/// thread cap (`jet_foundation::CompilerThreads`). The count only changes
/// how the work is scheduled, never its result.
pub(crate) fn check_worker_count(jobs: usize) -> usize {
    jet_foundation::CompilerThreads::admit(jobs, MAX_CHECK_WORKERS)
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
