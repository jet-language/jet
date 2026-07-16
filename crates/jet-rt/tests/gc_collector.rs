use jet_rt::__gc::{Collector, Edge, Fault};
use std::sync::{Arc, Barrier, Mutex};

struct Holder {
    child: Edge<u64>,
}

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
fn active_unreachable_parent_keeps_transitive_children_live() {
    let collector = Collector::new();
    let parent = collector.allocate(3_u64).unwrap();
    let child = collector.allocate(4_u64).unwrap();
    let parent_id = parent.id();
    let child_id = child.id();
    parent.replace_edges(&[child_id]).unwrap();
    let parent_edge = parent.edge();
    drop(parent);
    drop(child);
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let worker_entered = Arc::clone(&entered);
    let worker_release = Arc::clone(&release);
    let worker = std::thread::spawn(move || {
        parent_edge
            .read(|value| {
            worker_entered.wait();
            worker_release.wait();
            *value
        })
            .unwrap()
    });

    entered.wait();
    let first = collector.collect().unwrap();
    assert_eq!(first.reclaimed, Vec::new());
    assert_eq!(first.reachable, 2);
    assert_eq!(first.deferred, Vec::new());
    release.wait();
    assert_eq!(worker.join().unwrap(), 3);
    assert_eq!(
        collector.collect().unwrap().reclaimed,
        vec![parent_id, child_id]
    );
}

#[test]
fn edit_with_edges_commits_only_after_successful_mutation() {
    let collector = Collector::new();
    let parent = collector.allocate(1_u64).unwrap();
    let old_child = collector.allocate(2_u64).unwrap();
    let new_child = collector.allocate(3_u64).unwrap();
    let parent_id = parent.id();
    let old_id = old_child.id();
    let new_id = new_child.id();
    parent.replace_edges(&[old_id]).unwrap();
    drop(old_child);
    drop(new_child);

    let conflict = parent
        .edit(|_| {
            parent
                .edit_with_edges(&[new_id], |value| *value = 9)
                .unwrap_err()
        })
        .unwrap();
    assert_eq!(conflict, Fault::BorrowConflict(parent_id));
    let after_conflict = collector.collect().unwrap();
    assert_eq!(after_conflict.reclaimed, vec![new_id]);
    assert_eq!(after_conflict.reachable, 2);
    drop(parent);
    assert_eq!(
        collector.collect().unwrap().reclaimed,
        vec![parent_id, old_id]
    );

    let collector = Collector::new();
    let parent = collector.allocate(1_u64).unwrap();
    let old_child = collector.allocate(2_u64).unwrap();
    let new_child = collector.allocate(3_u64).unwrap();
    let parent_id = parent.id();
    let old_id = old_child.id();
    let new_id = new_child.id();
    parent.replace_edges(&[old_id]).unwrap();
    drop(old_child);
    drop(new_child);

    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        parent
            .edit_with_edges(&[new_id], |value| {
                *value = 9;
                std::panic::panic_any("mutation failure");
            })
            .unwrap();
    }));
    assert!(panicked.is_err());
    let after_panic = collector.collect().unwrap();
    assert_eq!(after_panic.reclaimed, vec![new_id]);
    assert_eq!(after_panic.reachable, 2);
    drop(parent);
    assert_eq!(
        collector.collect().unwrap().reclaimed,
        vec![parent_id, old_id]
    );
}

#[test]
fn reentrant_edge_rewrite_cannot_overwrite_reserved_graph() {
    let collector = Collector::new();
    let old_child = collector.allocate(2_u64).unwrap();
    let new_child = collector.allocate(3_u64).unwrap();
    let old_id = old_child.id();
    let new_id = new_child.id();
    let parent = collector
        .allocate(Holder {
            child: old_child.edge(),
        })
        .unwrap();
    let parent_id = parent.id();
    parent.replace_edges(&[old_id]).unwrap();
    drop(old_child);
    drop(new_child);

    parent
        .edit_with_edges(&[old_id], |holder| {
            assert_eq!(holder.child.read(|value| *value).unwrap(), 2);
            assert_eq!(
                parent.replace_edges(&[new_id]),
                Err(Fault::MutationConflict(parent_id))
            );
        })
        .unwrap();

    let after = collector.collect().unwrap();
    assert_eq!(after.reclaimed, vec![new_id]);
    assert_eq!(after.reachable, 2);
    drop(parent);
    assert_eq!(
        collector.collect().unwrap().reclaimed,
        vec![old_id, parent_id]
    );
}

#[test]
fn concurrent_rewrite_cannot_drop_payload_held_old_edge() {
    let collector = Collector::new();
    let old_child = collector.allocate(2_u64).unwrap();
    let new_child = collector.allocate(3_u64).unwrap();
    let old_id = old_child.id();
    let new_id = new_child.id();
    let parent = collector
        .allocate(Holder {
            child: old_child.edge(),
        })
        .unwrap();
    let parent_id = parent.id();
    parent.replace_edges(&[old_id]).unwrap();
    let worker_parent = parent.try_clone().unwrap();
    drop(old_child);
    drop(new_child);

    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let worker_entered = Arc::clone(&entered);
    let worker_release = Arc::clone(&release);
    let worker = std::thread::spawn(move || {
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            worker_parent
                .edit_with_edges(&[new_id], |holder| {
                    worker_entered.wait();
                    worker_release.wait();
                    assert_eq!(holder.child.read(|value| *value).unwrap(), 2);
                    std::panic::panic_any("abort reserved mutation");
                })
                .unwrap();
        }));
        assert!(panicked.is_err());
    });

    entered.wait();
    assert_eq!(
        parent.replace_edges(&[new_id]),
        Err(Fault::MutationConflict(parent_id))
    );
    let during = collector.collect().unwrap();
    assert_eq!(during.reclaimed, Vec::new());
    assert_eq!(during.reachable, 3);
    release.wait();
    worker.join().unwrap();

    let after = collector.collect().unwrap();
    assert_eq!(after.reclaimed, vec![new_id]);
    assert_eq!(after.reachable, 2);
    drop(parent);
    assert_eq!(
        collector.collect().unwrap().reclaimed,
        vec![old_id, parent_id]
    );
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
