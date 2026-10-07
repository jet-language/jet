// D-EXEC1 / #4720: typed places owned by the one resident runtime.
// A slot contains the canonical native layout, not a serialized rendering.
// The host pins every defining image until AFTER session destruction: payloads
// may contain function pointers as well as the drop glue recorded here.
//
// JET_VETTED_UNSAFE_BEGIN: jet_c_abi_session
// AUDIT: only checked native code accesses these places. reserve returns
// uninitialized, correctly aligned storage; publish follows complete typed
// construction. resolve borrows an initialized place for one turn. take marks
// the place uninitialized before the caller transfers its payload. No borrowed
// pointer may outlive the turn or survive retire/destroy. Drop glue has the
// canonical layout, cannot unwind, and remains executable until destruction.
// This is memory infrastructure, not a user-accessible dynamic-loading API.
#[cfg(not(target_arch = "wasm32"))]
mod jet_c_abi_session {
    use std::alloc::{alloc, dealloc, Layout};
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64, Ordering};

    type DropGlue = unsafe extern "C" fn(*mut u8);

    #[derive(Clone, Copy, PartialEq, Eq)]
    struct Identity {
        // Digest of the schema-7 MIR type identity key, not its display name.
        key: [u64; 4],
        size: usize,
        align: usize,
    }

    struct Slot {
        identity: Identity,
        layout: Layout,
        ptr: *mut u8,
        drop_glue: Option<DropGlue>,
        initialized: bool,
    }

    impl Drop for Slot {
        fn drop(&mut self) {
            if self.initialized {
                if let Some(drop_glue) = self.drop_glue {
                    // SAFETY: publish established complete initialization;
                    // take cleared it on transfer. The host pins the image.
                    unsafe { drop_glue(self.ptr) };
                }
            }
            // SAFETY: this allocation was obtained with exactly this Layout;
            // slots are never cloned, and their payload glue does not free it.
            unsafe { dealloc(self.ptr, self.layout) };
        }
    }

    #[derive(Default)]
    struct Session {
        slots: BTreeMap<u64, Slot>,
        active: bool,
        last_binding: Option<u64>,
    }

    impl Drop for Session {
        fn drop(&mut self) {
            // IDs increase with declaration order. Teardown outside generated
            // frames follows the reverse lexical owner order; in-turn cleanup
            // remains explicit MIR retire/drop operations, never an ID guess.
            while let Some((_, slot)) = self.slots.pop_last() { drop(slot); }
        }
    }

    // Places are confined to their owning worker thread. A handle used on a
    // different thread is rejected; it never manufactures Send for a payload.
    // Native callbacks may borrow places only under sema's ordinary borrow rules.
    thread_local! {
        static SESSIONS: RefCell<BTreeMap<u64, Session>> = const { RefCell::new(BTreeMap::new()) };
    }
    static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

    #[no_mangle]
    pub extern "C" fn jet_rt_session_create() -> u64 {
        let Ok(id) = NEXT_SESSION.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1)) else {
            return 0;
        };
        SESSIONS.with(|sessions| sessions.borrow_mut().insert(id, Session::default()));
        id
    }

    /// Begin one non-reentrant turn. All slot operations require an active turn.
    #[no_mangle]
    pub extern "C" fn jet_rt_session_begin(id: u64) -> bool {
        SESSIONS.with(|sessions| {
            let mut sessions = sessions.borrow_mut();
            let Some(session) = sessions.get_mut(&id) else { return false };
            if session.active { return false; }
            session.active = true;
            true
        })
    }

    /// Release uninitialized reservations/moved storage AFTER all turn frames
    /// have returned. Initialized places survive; this is not a transaction or
    /// an implicit clone/rollback of writes to existing mutable bindings.
    #[no_mangle]
    pub extern "C" fn jet_rt_session_end(id: u64) -> bool {
        SESSIONS.with(|sessions| {
            let mut sessions = sessions.borrow_mut();
            let Some(session) = sessions.get_mut(&id) else { return false };
            if !session.active { return false; }
            session.slots.retain(|_, slot| slot.initialized);
            session.active = false;
            true
        })
    }

    /// IDs increase with declaration order and are never reused in a session
    /// (shadowing gets a new ID). Null means invalid session/turn, stale ID,
    /// duplicate ID, bad layout, or OOM.
    /// Zero-size places receive a unique aligned one-byte allocation.
    #[no_mangle]
    pub extern "C" fn jet_rt_session_reserve(
        id: u64, binding: u64, key0: u64, key1: u64, key2: u64, key3: u64,
        size: usize, align: usize, drop_glue: Option<DropGlue>,
    ) -> *mut u8 {
        let Ok(layout) = Layout::from_size_align(size.max(1), align) else { return std::ptr::null_mut() };
        SESSIONS.with(|sessions| {
            let mut sessions = sessions.borrow_mut();
            let Some(session) = sessions.get_mut(&id) else { return std::ptr::null_mut() };
            if !session.active || session.slots.contains_key(&binding) { return std::ptr::null_mut(); }
            if session.last_binding.is_some_and(|last| binding <= last) { return std::ptr::null_mut(); }
            // SAFETY: validated nonzero Layout; ownership transfers into Slot.
            let ptr = unsafe { alloc(layout) };
            if ptr.is_null() { return ptr; }
            session.last_binding = Some(binding);
            session.slots.insert(binding, Slot {
                identity: Identity { key: [key0, key1, key2, key3], size, align },
                layout, ptr, drop_glue, initialized: false,
            });
            ptr
        })
    }

    /// # Safety
    /// The complete payload must have been constructed in the reserved place,
    /// with the registered layout and drop glue, before this call.
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_session_publish(id: u64, binding: u64) -> bool {
        SESSIONS.with(|sessions| {
            let mut sessions = sessions.borrow_mut();
            let Some(session) = sessions.get_mut(&id) else { return false };
            if !session.active { return false; }
            let Some(slot) = session.slots.get_mut(&binding) else { return false };
            if slot.initialized { return false; }
            slot.initialized = true;
            true
        })
    }

    fn access(id: u64, binding: u64, identity: Identity, take: bool) -> *mut u8 {
        SESSIONS.with(|sessions| {
            let mut sessions = sessions.borrow_mut();
            let Some(session) = sessions.get_mut(&id) else { return std::ptr::null_mut() };
            if !session.active { return std::ptr::null_mut(); }
            let Some(slot) = session.slots.get_mut(&binding) else { return std::ptr::null_mut() };
            if !slot.initialized || slot.identity != identity { return std::ptr::null_mut(); }
            if take { slot.initialized = false; }
            slot.ptr
        })
    }

    /// Borrowed pointer: sema determines read/write permission and lifetime.
    #[no_mangle]
    pub extern "C" fn jet_rt_session_resolve(
        id: u64, binding: u64, key0: u64, key1: u64, key2: u64, key3: u64,
        size: usize, align: usize,
    ) -> *mut u8 {
        access(id, binding, Identity { key: [key0, key1, key2, key3], size, align }, false)
    }

    /// # Safety
    /// The caller must immediately move the payload out of the returned place
    /// and assume its ordinary ownership/drop obligation. Nobody else may hold
    /// a borrow of it. Storage remains valid until end/retire/destroy.
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_session_take(
        id: u64, binding: u64, key0: u64, key1: u64, key2: u64, key3: u64,
        size: usize, align: usize,
    ) -> *mut u8 {
        access(id, binding, Identity { key: [key0, key1, key2, key3], size, align }, true)
    }

    /// # Safety
    /// No native frame may still borrow this place. Glue must remain pinned.
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_session_retire(id: u64, binding: u64) -> bool {
        let removed = SESSIONS.with(|sessions| {
            let mut sessions = sessions.borrow_mut();
            let session = sessions.get_mut(&id)?;
            if !session.active { return None; }
            session.slots.remove(&binding)
        });
        let existed = removed.is_some();
        // Drop outside RefCell's borrow: canonical glue may call the runtime.
        drop(removed);
        existed
    }

    /// # Safety
    /// All native turn/callback frames have returned, no borrowed places survive,
    /// and every defining image remains pinned until this function returns.
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_session_destroy(id: u64) -> bool {
        let removed = SESSIONS.with(|sessions| {
            let mut sessions = sessions.borrow_mut();
            if sessions.get(&id).is_some_and(|session| session.active) { return None; }
            sessions.remove(&id)
        });
        let existed = removed.is_some();
        drop(removed);
        existed
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::atomic::{AtomicUsize, Ordering};
        static DROPS: AtomicUsize = AtomicUsize::new(0);
        unsafe extern "C" fn drop_box(ptr: *mut u8) {
            // SAFETY: this test publishes a Box<u64> in a Box-sized slot.
            drop(unsafe { ptr.cast::<Box<u64>>().read() });
            DROPS.fetch_add(1, Ordering::Relaxed);
        }
        fn reserve(id: u64, binding: u64) -> *mut u8 {
            jet_rt_session_reserve(id, binding, 1, 2, 3, 4, 8, 8, None)
        }
        fn resolve(id: u64, binding: u64) -> *mut u8 {
            jet_rt_session_resolve(id, binding, 1, 2, 3, 4, 8, 8)
        }
        #[test]
        fn carries_places_without_reexecution_or_copy() {
            let id = jet_rt_session_create();
            assert!(jet_rt_session_begin(id));
            let ptr = reserve(id, 7);
            assert!(!ptr.is_null());
            assert!(resolve(id, 7).is_null());
            unsafe { ptr.cast::<u64>().write(41); }
            assert!(unsafe { jet_rt_session_publish(id, 7) });
            assert!(jet_rt_session_end(id));
            assert!(resolve(id, 7).is_null());
            for _ in 0..3 {
                assert!(jet_rt_session_begin(id));
                assert_eq!(resolve(id, 7), ptr);
                assert_eq!(unsafe { ptr.cast::<u64>().read() }, 41);
                assert!(jet_rt_session_end(id));
            }
            assert!(unsafe { jet_rt_session_destroy(id) });
        }
        #[test]
        fn validates_identity_layout_and_lifecycle() {
            let id = jet_rt_session_create();
            assert!(reserve(id, 1).is_null());
            assert!(jet_rt_session_begin(id));
            assert!(!jet_rt_session_begin(id));
            assert!(!reserve(id, 1).is_null());
            assert!(reserve(id, 1).is_null());
            assert!(jet_rt_session_reserve(id, 2, 1, 2, 3, 4, 8, 3, None).is_null());
            // Complete the dropless u64 before publishing.
            unsafe { reserve(id, 3).cast::<u64>().write(9); }
            assert!(unsafe { jet_rt_session_publish(id, 3) });
            assert!(jet_rt_session_resolve(id, 3, 0, 2, 3, 4, 8, 8).is_null());
            assert!(jet_rt_session_resolve(id, 3, 1, 2, 3, 4, 4, 8).is_null());
            assert!(jet_rt_session_resolve(id, 3, 1, 2, 3, 4, 8, 16).is_null());
            assert!(!unsafe { jet_rt_session_destroy(id) });
            assert!(jet_rt_session_end(id));
            assert!(!jet_rt_session_end(id));
            assert!(jet_rt_session_begin(id));
            assert!(resolve(id, 1).is_null());
            assert!(jet_rt_session_end(id));
            assert!(unsafe { jet_rt_session_destroy(id) });
            assert!(!unsafe { jet_rt_session_destroy(id) });
        }
        #[test]
        fn glue_drops_once_and_moves_transfer_ownership() {
            let before = DROPS.load(Ordering::Relaxed);
            let id = jet_rt_session_create();
            assert!(jet_rt_session_begin(id));
            for binding in 1..=3 {
                let ptr = jet_rt_session_reserve(id, binding, 1, 2, 3, 4, 8, 8, Some(drop_box));
                assert!(!ptr.is_null());
                if binding != 3 {
                    unsafe { ptr.cast::<Box<u64>>().write(Box::new(binding)); }
                    assert!(unsafe { jet_rt_session_publish(id, binding) });
                }
            }
            assert!(unsafe { jet_rt_session_retire(id, 1) });
            let ptr = unsafe { jet_rt_session_take(id, 2, 1, 2, 3, 4, 8, 8) };
            assert!(!ptr.is_null());
            assert!(resolve(id, 2).is_null());
            assert!(unsafe { jet_rt_session_take(id, 2, 1, 2, 3, 4, 8, 8) }.is_null());
            let owner = unsafe { ptr.cast::<Box<u64>>().read() };
            assert_eq!(*owner, 2);
            drop(owner);
            assert!(jet_rt_session_end(id));
            assert!(unsafe { jet_rt_session_destroy(id) });
            assert_eq!(DROPS.load(Ordering::Relaxed) - before, 1);
        }
        #[test]
        fn aligned_zero_size_and_thread_confinement() {
            let id = jet_rt_session_create();
            assert!(jet_rt_session_begin(id));
            let ptr = jet_rt_session_reserve(id, 1, 0, 0, 0, 0, 0, 64, None);
            assert!(!ptr.is_null());
            assert_eq!(ptr as usize % 64, 0);
            assert!(unsafe { jet_rt_session_publish(id, 1) });
            assert!(!std::thread::spawn(move || jet_rt_session_begin(id)).join().unwrap());
            assert!(jet_rt_session_end(id));
            assert!(unsafe { jet_rt_session_destroy(id) });
        }
        thread_local! {
            static DROP_ORDER: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
        }
        unsafe extern "C" fn record_drop(ptr: *mut u8) {
            let value = unsafe { ptr.cast::<u64>().read() };
            DROP_ORDER.with(|order| order.borrow_mut().push(value));
        }
        #[test]
        fn teardown_reverses_declarations_and_rejects_reused_ids() {
            DROP_ORDER.with(|order| order.borrow_mut().clear());
            let id = jet_rt_session_create();
            assert!(jet_rt_session_begin(id));
            for binding in 1..=3 {
                let ptr = jet_rt_session_reserve(id, binding, 1, 2, 3, 4, 8, 8, Some(record_drop));
                assert!(!ptr.is_null());
                unsafe { ptr.cast::<u64>().write(binding); }
                assert!(unsafe { jet_rt_session_publish(id, binding) });
            }
            assert!(unsafe { jet_rt_session_retire(id, 2) });
            assert!(reserve(id, 2).is_null());
            assert!(jet_rt_session_end(id));
            assert!(unsafe { jet_rt_session_destroy(id) });
            DROP_ORDER.with(|order| assert_eq!(*order.borrow(), vec![2, 3, 1]));
        }
    }
}
// JET_VETTED_UNSAFE_END: jet_c_abi_session
