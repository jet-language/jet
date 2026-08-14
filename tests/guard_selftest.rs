//! Guard self-test (Tower #1850): explicit, small-window proofs that the
//! allocation cap and wall-clock watchdog installed by `tests/common/mod.rs`
//! actually trip. The two failure-mode tests are `#[ignore]`d and must be run
//! individually with a tiny cap/deadline, e.g.:
//!
//!   JET_TEST_ALLOC_CAP_GB=1 timeout 120 scripts/agent/jet-env \
//!     cargo test --test guard_selftest -- --ignored alloc_cap --test-threads=1
//!
//!   JET_TEST_DEADLINE_SECS=3 timeout 60 scripts/agent/jet-env \
//!     cargo test --test guard_selftest -- --ignored deadline --test-threads=1
//!
//! Both are expected to abort the whole process (SIGABRT / non-zero exit)
//! with the guard's stderr line — they do not "pass" in the normal sense.

mod common;
#[path = "unsafe_ratchet.rs"]
mod unsafe_ratchet;

#[test]
fn guards_stay_quiet_on_a_healthy_run() {
    // A normal, bounded allocation pattern well under any sane cap, and a
    // runtime far under any sane deadline — proves the guard rails add no
    // observable behavior to a healthy test.
    let mut v: Vec<u8> = Vec::new();
    for _ in 0..1024 {
        v.extend_from_slice(&[0u8; 1024]);
    }
    assert_eq!(v.len(), 1024 * 1024);
}

#[test]
fn unsafe_region_ratchet_trips_on_seeded_growth() {
    unsafe_ratchet::ratchet_trips_on_seeded_growth();
}

#[test]
fn unsafe_region_ratchet_allows_shrink() {
    unsafe_ratchet::ratchet_allows_shrink();
}

#[test]
fn generated_ffi_does_not_move_unsafe_baseline() {
    unsafe_ratchet::generated_ffi_does_not_move_baseline();
}

#[test]
#[ignore = "trips the allocation-cap guard on purpose; run explicitly with a small JET_TEST_ALLOC_CAP_GB"]
fn alloc_cap_guard_trips() {
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    loop {
        chunks.push(vec![0u8; 16 * 1024 * 1024]); // 16 MiB per step
        if chunks.len() > 4096 {
            // 64 GiB in — the guard should have aborted the process long
            // before this, even with the default 10 GB cap.
            panic!(
                "guard did not abort before allocating {} chunks",
                chunks.len()
            );
        }
    }
}

#[test]
#[ignore = "trips the wall-clock watchdog on purpose; run explicitly with a small JET_TEST_DEADLINE_SECS"]
fn deadline_guard_trips() {
    std::thread::sleep(std::time::Duration::from_secs(120));
    panic!("guard did not abort before the sleep finished");
}
