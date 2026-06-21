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
    // D-LL1: the one vetted lifetime-extension lives here, inside the std/mem
    // helper module; it never leaks into user-visible generated code.
    use std::cell::RefCell;

    /// Arena allocator: grow-only bump buffer. Every value handed to `alloc`
    /// is boxed and kept alive in `chunks` until the arena is reset or freed;
    /// the returned reference borrows *into* that storage.
    pub struct JetArena {
        // Each allocation is a separately-boxed value, type-erased so one arena
        // can store heterogeneous types. The box owns the value; the pointer we
        // hand out aliases its interior for `'arena`. We never move or drop a
        // box while the arena is borrowed, so the alias stays valid.
        chunks: RefCell<Vec<Box<dyn std::any::Any>>>,
    }

    impl JetArena {
        pub fn new() -> Self {
            JetArena { chunks: RefCell::new(Vec::new()) }
        }
        pub fn with_capacity(cap: usize) -> Self {
            JetArena { chunks: RefCell::new(Vec::with_capacity(cap)) }
        }

        /// Store `val` in the arena and return a mutable view into its storage,
        /// valid for as long as the arena is borrowed (`&self`). The reference
        /// is tied to `&self`, so the borrow checker keeps the arena alive and
        /// un-reset/un-freed for the whole life of the view.
        pub fn alloc<T: 'static>(&self, val: T) -> &mut T {
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
            self.chunks.borrow_mut().clear();
        }

        /// Free: consume the arena, returning all memory. By-value `self` — no
        /// view can outlive it (Jet rejects first: E0632/E0631).
        pub fn free(self) {
            drop(self);
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
}
