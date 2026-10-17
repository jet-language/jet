// Appended only to the native ABI witness's freshly emitted runtime source.
// Crate-local access exercises the REAL remaining Numeric hazard domain, not
// a second list or a mocked reader. Native allocator LIFO reuse witnesses free.
#[no_mangle]
pub extern "C" fn jet_test_int_hazard_fixture() {
    use std::sync::{Arc, Barrier};
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use crate::jet_foundation_numeric::JetInt;
    #[repr(C)]
    struct Node {
        refs: AtomicUsize, retired_next: usize, negative: u64,
        limbs: *const u32, len: usize, cap: usize,
    }
    unsafe extern "C" {
        fn jet_int_owned_from_i64(value: i64) -> i64;
        fn jet_rt_int_clone(value: i64) -> i64;
        fn jet_rt_int_drop(value: i64);
        fn jet_rt_alloc(size: usize, align: usize) -> *mut u8;
        fn jet_rt_free(ptr: *mut u8, size: usize, align: usize);
    }
    let node_of = |raw: i64| ((raw as u64) & ((1_u64 << 62) - 1)) as usize;
    unsafe {
        // Keep this size class nonempty, preventing empty-slab unmapping.
        let keeper = jet_rt_alloc(48, 8);
        let raw = jet_int_owned_from_i64(i64::MAX);
        let node = node_of(raw);
        let clone = jet_rt_int_clone(raw);
        assert_eq!((*(node as *const Node)).refs.load(Ordering::Acquire), 2);
        jet_rt_int_drop(clone);
        assert_eq!((*(node as *const Node)).refs.load(Ordering::Acquire), 1);
        jet_rt_int_drop(raw);
        let reused = jet_rt_alloc(48, 8);
        assert_eq!(reused as usize, node, "unhazarded final drop did not free");
        let distinct = jet_rt_alloc(48, 8);
        assert_ne!(distinct, reused, "final drop double-freed its header");
        jet_rt_free(distinct, 48, 8);
        jet_rt_free(reused, 48, 8);

        let raw = jet_int_owned_from_i64(i64::MAX);
        let node = node_of(raw);
        let slot = Arc::new(AtomicU64::new(raw as u64));
        let barrier = Arc::new(Barrier::new(2));
        let reader_slot = slot.clone();
        let reader_barrier = barrier.clone();
        let reader = std::thread::spawn(move || {
            let mut reloads = 0;
            let observed = JetInt::clone_from_atomic(raw as u64, || {
                reloads += 1;
                if reloads == 2 {
                    // clone_from_atomic has published its hazard before its
                    // second reload. Wait until the writer removes/final-drops.
                    reader_barrier.wait();
                    reader_barrier.wait();
                    let protected = &*(node as *const Node);
                    assert_eq!(protected.refs.load(Ordering::Acquire), 0);
                    assert_eq!(protected.len, 3);
                    assert_eq!(std::slice::from_raw_parts(protected.limbs, 3), &[854775807, 223372036, 9]);
                }
                reader_slot.load(Ordering::SeqCst)
            });
            assert_eq!(observed.to_i64(), Some(0));
        });
        barrier.wait();
        slot.store(0, Ordering::SeqCst); // consume the atomic slot's owner
        jet_rt_int_drop(raw);
        let while_hazarded = jet_rt_alloc(48, 8);
        assert_ne!(while_hazarded as usize, node, "freed a hazarded node");
        barrier.wait();
        reader.join().expect("hazard reader failed");
        // Clearing the real read guard reclaims the real retired stack.
        let reclaimed = jet_rt_alloc(48, 8);
        assert_eq!(reclaimed as usize, node, "cleared hazard did not reclaim");
        let distinct = jet_rt_alloc(48, 8);
        assert_ne!(distinct, reclaimed, "retirement double-freed its header");
        jet_rt_free(distinct, 48, 8);
        jet_rt_free(reclaimed, 48, 8);
        jet_rt_free(while_hazarded, 48, 8);
        jet_rt_free(keeper, 48, 8);
        jet_rt_int_drop(0);
        jet_rt_int_drop(-1);
    }
}
