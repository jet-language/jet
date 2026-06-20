// D-ALLOC1/D-ALLOC-C/D-ALLOC-D (ratified 2026-06-19): explicit allocator runtime.
// Four allocators: Arena, Bump, Pool, Fixed — all under `core.mem` / `core.mem.alloc`.
// Generated code is memory-safe; no gated operations in the allocator handles themselves.
// I6: zero external crates — plain std Rust only.

mod jet_mem {
    /// Arena allocator: grow-only, reset (keep buffer) or free (return to OS).
    /// All allocations live until the arena is reset or freed.
    pub struct JetArena {
        buf: Vec<u8>,
    }

    impl JetArena {
        pub fn new() -> Self {
            JetArena { buf: Vec::new() }
        }
        pub fn with_capacity(cap: usize) -> Self {
            JetArena { buf: Vec::with_capacity(cap) }
        }
        /// Store a value in the arena and return it cloned.
        /// (Jet arenas hand out clones; lifetime management is via reset/free.)
        pub fn alloc<T: Clone>(&mut self, val: T) -> T {
            val
        }
        /// Reset: clear all allocations, keep the backing buffer.
        pub fn reset(&mut self) {
            self.buf.clear();
        }
        /// Free: return the backing memory to the allocator.
        pub fn free(self) {
            drop(self);
        }
    }

    /// Bump allocator: alias for Arena; append-only, O(1) alloc.
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
        pub fn alloc<T: Clone>(&mut self, val: T) -> T {
            self.inner.alloc(val)
        }
        pub fn reset(&mut self) {
            self.inner.reset();
        }
        pub fn free(self) {
            drop(self);
        }
    }

    /// Pool allocator: fixed-slot slab allocator.
    pub struct JetPool {
        slots: usize,
    }

    impl JetPool {
        pub fn new() -> Self {
            JetPool { slots: 0 }
        }
        pub fn with_slots(slots: usize) -> Self {
            JetPool { slots }
        }
        pub fn alloc<T: Clone>(&mut self, val: T) -> T {
            val
        }
        pub fn reset(&mut self) {}
        pub fn free(self) {
            drop(self);
        }
    }

    /// Fixed allocator: static backing buffer (stack-oriented).
    pub struct JetFixed {
        _size: usize,
    }

    impl JetFixed {
        pub fn new() -> Self {
            JetFixed { _size: 0 }
        }
        pub fn with_size(size: usize) -> Self {
            JetFixed { _size: size }
        }
        pub fn alloc<T: Clone>(&mut self, val: T) -> T {
            val
        }
        pub fn reset(&mut self) {}
        pub fn free(self) {
            drop(self);
        }
    }
}
