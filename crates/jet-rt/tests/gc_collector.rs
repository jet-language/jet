use jet_rt::__gc::{Collector, Fault};
use std::sync::{Arc, Barrier, Mutex};

#[test]
fn roots_edges_and_cycles_collect_at_safepoints() {
    let collector = Collector::new();
    let a = collector.allocate(String::from("a")).unwrap();
    let b = collector.allocate(String::from("b")).unwrap();
    let a_id = a.id();
    let b_id = b.id();

    a.replace_edges(&[b_id]).unwrap();
    b.replace_edges(&[a_id]).unwrap();
    assert_eq!(collector.collect().unwrap().reclaimed, Vec::new());

    drop(a);
    drop(b);
    assert_eq!(collector.collect().unwrap().reclaimed, vec![a_id, b_id]);
    assert_eq!(collector.live_count().unwrap(), 0);
}

#[test]
fn finalization_is_identity_ordered_and_panic_bounded() {
    let collector = Collector::new();
    let finalized = Arc::new(Mutex::new(Vec::new()));
    let first_log = Arc::clone(&finalized);
    let first = collector
        .allocate_with_finalizer(1_u64, move |value| {
            first_log.lock().unwrap().push(*value);
            std::panic::panic_any("finalizer failure");
        })
        .unwrap();
    let second_log = Arc::clone(&finalized);
    let second = collector
        .allocate_with_finalizer(2_u64, move |value| {
            second_log.lock().unwrap().push(*value);
        })
        .unwrap();
    let first_id = first.id();
    let second_id = second.id();

    drop(second);
    drop(first);
    let result = collector.collect().unwrap();
    assert_eq!(result.reclaimed, vec![first_id, second_id]);
    assert_eq!(result.finalizer_panics, vec![first_id]);
    assert_eq!(*finalized.lock().unwrap(), vec![1, 2]);
}

#[test]
fn malformed_edges_fail_closed_without_changing_graph() {
    let collector = Collector::new();
    let root = collector.allocate(7_u64).unwrap();
    let missing = collector.allocate(8_u64).unwrap().id();
    let child = collector.allocate(9_u64).unwrap();
    let child_id = child.id();
    drop(child);
    collector.collect().unwrap();

    assert_eq!(
        root.replace_edges(&[missing]),
        Err(Fault::UnknownObject(missing))
    );
    drop(root);
    assert_eq!(collector.collect().unwrap().reclaimed.len(), 1);
    assert_eq!(collector.live_count().unwrap(), 0);
    assert_ne!(missing, child_id);

    let left = Collector::new();
    let right = Collector::new();
    let left_root = left.allocate(1_u64).unwrap();
    let right_root = right.allocate(2_u64).unwrap();
    assert_eq!(
        left_root.replace_edges(&[right_root.id()]),
        Err(Fault::UnknownObject(right_root.id()))
    );

    let oversized = vec![left_root.id(); 65_537];
    assert!(matches!(
        left_root.replace_edges(&oversized),
        Err(Fault::TooManyEdges { count: 65_537, .. })
    ));
}

#[test]
fn roots_cross_threads_and_unwind_releases_them() {
    let collector = Collector::new();
    let root = collector.allocate(1_u64).unwrap();
    let thread_root = root.try_clone().unwrap();
    let id = root.id();

    std::thread::spawn(move || {
        thread_root.edit(|value| *value += 1).unwrap();
        assert_eq!(thread_root.read(|value| *value).unwrap(), 2);
    })
    .join()
    .unwrap();

    let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        root.edit(|_| std::panic::panic_any("edit failure"))
            .unwrap();
    }));
    assert!(panic_result.is_err());
    drop(root);

    let result = collector.collect().unwrap();
    assert_eq!(result.reclaimed, vec![id]);
    assert_eq!(result.poisoned_payloads, vec![id]);
}

#[test]
fn active_access_defers_sweep_and_nested_mutation_fails_fast() {
    let collector = Collector::new();
    let root = collector.allocate(3_u64).unwrap();
    let conflict = root
        .edit(|_| root.edit(|value| *value += 1).unwrap_err())
        .unwrap();
    assert_eq!(conflict, Fault::BorrowConflict(root.id()));

    let id = root.id();
    let edge = root.edge();
    drop(root);
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let worker_entered = Arc::clone(&entered);
    let worker_release = Arc::clone(&release);
    let worker = std::thread::spawn(move || {
        edge.read(|value| {
            worker_entered.wait();
            worker_release.wait();
            *value
        })
        .unwrap()
    });

    entered.wait();
    let first = collector.collect().unwrap();
    assert_eq!(first.reclaimed, Vec::new());
    assert_eq!(first.deferred, vec![id]);
    release.wait();
    assert_eq!(worker.join().unwrap(), 3);
    assert_eq!(collector.collect().unwrap().reclaimed, vec![id]);
}

#[test]
fn heap_shutdown_runs_remaining_finalizers() {
    let finalized = Arc::new(Mutex::new(Vec::new()));
    let collector = Collector::new();
    let log = Arc::clone(&finalized);
    let root = collector
        .allocate_with_finalizer(11_u64, move |value| log.lock().unwrap().push(*value))
        .unwrap();

    drop(collector);
    drop(root);
    assert_eq!(*finalized.lock().unwrap(), vec![11]);
}
