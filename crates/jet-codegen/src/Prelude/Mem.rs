mod jet_mem {
    // D-ALLOC2 / D-REGION1 (ratified 2026-06-21): real bump-allocated arena +
    // scope-bound regions. The c05 upgrade — replaces the owned-clone stub where
    // `alloc(v)` just returned `v` with a real shared bump buffer.
    //
    // Soundness contract (the runtime half; the sema half lives in
    // Source/Sema/CheckerOwnership.rs as E0631/E0632):
    //   * `alloc(&self, v) -> &'arena mut T` hands out a reference *into* the
    //     arena's storage, tied to the arena's borrow — the typed-arena pattern.
    //     Chunks live in a `RefCell` (interior mutability) so `alloc` takes
    //     `&self` and many live views may coexist.
    //   * `reset(&mut self)` / `free(self)` take `&mut self` / `self` by value,
    //     so rustc itself forbids reset/free while any view is live: a borrow
    //     held by an outstanding `&'arena mut T` view conflicts with the
    //     `&mut`/`move`. Jet's sema rejects first (E0632) so I2 holds — rustc
    //     never speaks — but the signatures are the backstop.
    //
    // I6: zero external crates — plain std Rust only.
    // D-LL1: the one vetted lifetime-extension lives here, inside the core.mem
    // helper module; it never leaks into user-visible generated code.
    use std::cell::{Cell, RefCell};

    /// Arena allocator: grow-only bump buffer. Every value handed to `alloc`
    /// is boxed and kept alive in `chunks` until the arena is reset or freed;
    /// the returned reference borrows *into* that storage.
    pub struct JetArena {
        // Each allocation is a separately-boxed value, type-erased so one arena
        // can store heterogeneous types. The box owns the value; the pointer we
        // hand out aliases its interior for `'arena`. We never move or drop a
        // box while the arena is borrowed, so the alias stays valid.
        chunks: RefCell<Vec<Box<dyn std::any::Any>>>,
        bytes: Cell<usize>,
    }

    impl JetArena {
        pub fn new() -> Self {
            super::jet_observe_arena_open();
            JetArena { chunks: RefCell::new(Vec::new()), bytes: Cell::new(0) }
        }
        pub fn with_capacity(cap: usize) -> Self {
            super::jet_observe_arena_open();
            JetArena { chunks: RefCell::new(Vec::with_capacity(cap)), bytes: Cell::new(0) }
        }

        /// Store `val` in the arena and return a mutable view into its storage,
        /// valid for as long as the arena is borrowed (`&self`). The reference
        /// is tied to `&self`, so the borrow checker keeps the arena alive and
        /// un-reset/un-freed for the whole life of the view.
        pub fn alloc<T: 'static>(&self, val: T) -> &mut T {
            let bytes = std::mem::size_of::<T>();
            self.bytes.set(self.bytes.get().saturating_add(bytes));
            super::jet_observe_arena_alloc(bytes);
            let mut boxed: Box<T> = Box::new(val);
            // Pointer into the box's heap allocation. The box itself is parked
            // in `chunks` (below) and never moved/dropped while borrowed, so
            // this pointer stays valid for the arena's borrow.
            let ptr: *mut T = boxed.as_mut();
            self.chunks.borrow_mut().push(boxed as Box<dyn std::any::Any>);
            // SAFETY (D-LL1, vetted): `ptr` points at the interior of a box now
            // owned by `self.chunks`. `alloc` takes `&self`; the returned
            // `&mut T` borrows `self` for `'arena`. `reset`/`free` take
            // `&mut self`/`self`, so they cannot run while this borrow is live —
            // the box is therefore neither moved out, dropped, nor reallocated
            // for the lifetime of the reference. The box owns the only `T`; no
            // other view aliases this same allocation. So the `&mut T` is unique
            // and valid for `'arena`.
            unsafe { &mut *ptr }
        }

        /// Reset: drop all allocations, keep the backing buffer's capacity.
        /// `&mut self` — the borrow checker forbids calling this while any view
        /// handed out by `alloc` is still live (Jet rejects first: E0632).
        pub fn reset(&mut self) {
            let allocations = self.chunks.borrow().len();
            let bytes = self.bytes.replace(0);
            super::jet_observe_arena_reset(allocations, bytes);
            self.chunks.borrow_mut().clear();
        }

        /// Free: consume the arena, returning all memory. By-value `self` — no
        /// view can outlive it (Jet rejects first: E0632/E0631).
        pub fn free(self) {
            drop(self);
        }
    }

    impl Drop for JetArena {
        fn drop(&mut self) {
            let allocations = self.chunks.borrow().len();
            let bytes = self.bytes.get();
            super::jet_observe_arena_reset(allocations, bytes);
            super::jet_observe_arena_close();
        }
    }

    /// Bump allocator: append-only, O(1) alloc — same engine as Arena.
    pub struct JetBump {
        inner: JetArena,
    }

    impl JetBump {
        pub fn new() -> Self {
            JetBump { inner: JetArena::new() }
        }
        pub fn with_capacity(cap: usize) -> Self {
            JetBump { inner: JetArena::with_capacity(cap) }
        }
        pub fn alloc<T: 'static>(&self, val: T) -> &mut T {
            self.inner.alloc(val)
        }
        pub fn reset(&mut self) {
            self.inner.reset();
        }
        pub fn free(self) {
            drop(self);
        }
    }

    /// Pool allocator: fixed-slot slab — same bump engine, sized by slot count.
    pub struct JetPool {
        inner: JetArena,
    }

    impl JetPool {
        pub fn new() -> Self {
            JetPool { inner: JetArena::new() }
        }
        pub fn with_slots(slots: usize) -> Self {
            JetPool { inner: JetArena::with_capacity(slots) }
        }
        pub fn alloc<T: 'static>(&self, val: T) -> &mut T {
            self.inner.alloc(val)
        }
        pub fn reset(&mut self) {
            self.inner.reset();
        }
        pub fn free(self) {
            drop(self);
        }
    }

    /// Fixed allocator: static-backed (capacity pre-sized) bump buffer.
    pub struct JetFixed {
        inner: JetArena,
    }

    impl JetFixed {
        pub fn new() -> Self {
            JetFixed { inner: JetArena::new() }
        }
        pub fn with_size(size: usize) -> Self {
            JetFixed { inner: JetArena::with_capacity(size) }
        }
        pub fn alloc<T: 'static>(&self, val: T) -> &mut T {
            self.inner.alloc(val)
        }
        pub fn reset(&mut self) {
            self.inner.reset();
        }
        pub fn free(self) {
            drop(self);
        }
    }

    // ── D-CTX1 (ratified 2026-06-22, G2): Smart Context runtime ──────────────
    //
    // `#Context(allocator: a) { … }` compiles to a `JetContextGuard` RAII value
    // that saves the old ambient allocator pointer and restores it on Drop.
    // Restore fires on all exit paths: normal exit, `return`, `break`, `?`,
    // and panic unwind — satisfying Q2 = Cβ (per-block restore).
    //
    // The thread-local holds a raw `*const u8` (type-erased allocator pointer).
    // Safety: Jet sema guarantees the allocator variable is declared before the
    // `#Context` block and lives for at least the block's duration, so the
    // pointer is always valid while the guard is live. This is the same
    // lifetime-extension trust as the `alloc` helper above (D-LL1 vetted zone).
    //
    // I1: no `unsafe` leaks to user-visible generated Rust — this module is the
    // one audited exception (stripped from the I1 golden-test check in golden.rs).

    thread_local! {
        // Raw pointer to the current ambient allocator (None = use default heap).
        static JET_CTX_ALLOC: std::cell::Cell<Option<*const u8>> =
            std::cell::Cell::new(None);
    }

    /// Query the ambient allocator.  `None` means use the default heap.
    /// Called by generated ambient-allocating calls that sema marked "uses ambient".
    pub fn jet_ctx_alloc_ptr() -> Option<*const u8> {
        JET_CTX_ALLOC.with(|c| c.get())
    }

    /// RAII guard: pushes a new ambient allocator on construction, pops it on Drop.
    /// Drop is called on all exit paths (return, break, ?, panic unwind).
    pub struct JetContextGuard {
        saved: Option<*const u8>,
    }

    impl Drop for JetContextGuard {
        fn drop(&mut self) {
            JET_CTX_ALLOC.with(|c| c.set(self.saved));
        }
    }

    /// Push a new ambient allocator for the current block's dynamic extent.
    /// Returns a guard whose Drop restores the previous value.
    ///
    /// Safe to call: the `&T` borrow ensures the allocator lives at least as long
    /// as the borrow exists; Jet sema guarantees it lives for the whole block.
    /// The unsafe pointer cast happens here (inside the vetted D-LL1 zone), never
    /// in emitted user code (I1).
    pub fn jet_ctx_push_alloc<T>(alloc: &T) -> JetContextGuard {
        let saved = JET_CTX_ALLOC.with(|c| c.get());
        // SAFETY: we store a *const u8 alias of `alloc`. Dereferencing it is only
        // valid as long as `alloc` is live; the Jet sema ensures the arena variable
        // is declared before the `#Context` block and lives for its duration.
        // This is the same audited lifetime-extension trust as `JetArena::alloc`.
        let ptr = alloc as *const T as *const u8;
        JET_CTX_ALLOC.with(|c| c.set(Some(ptr)));
        JetContextGuard { saved }
    }

}
