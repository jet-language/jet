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
    //   * `reset(&mut self)` reuses storage; terminal release is the shared
    //     compiler-owned `close(^resource)` protocol,
    //     so rustc itself forbids reset/close while any view is live: a borrow
    //     held by an outstanding `&'arena mut T` view conflicts with the
    //     `&mut`/`move`. Jet's sema rejects first (E0632) so I2 holds — rustc
    //     never speaks — but the signatures are the backstop.
    //
    // I6: zero external crates — plain std Rust only.
    // D-LL1: the one vetted lifetime-extension lives here, inside the core.mem
    // helper module; it never leaks into user-visible generated code.
    use std::cell::RefCell;
    use std::ptr::NonNull;

    pub use super::jet_uninit_semantics::{JetUninit, JetUninitFixed};

    const DEFAULT_ARENA_BYTES: usize = 4096;
    const DEFAULT_BUMP_BYTES: usize = 64 * 1024;
    const DEFAULT_POOL_SLOTS: usize = 64;
    const BASE_ALIGNMENT: usize = 4096;

    #[derive(Clone, Copy, Debug)]
    pub struct AllocatorFacts {
        pub live_allocations: usize,
        pub live_bytes: usize,
        pub retained_bytes: usize,
        pub high_water_bytes: usize,
    }

    struct DropEntry {
        ptr: *mut u8,
        drop_fn: unsafe fn(*mut u8),
    }

    unsafe fn drop_at<T>(ptr: *mut u8) {
        // SAFETY: callers register exactly one entry after writing one T at ptr.
        unsafe { std::ptr::drop_in_place(ptr.cast::<T>()) };
    }

    struct RawBlock {
        ptr: NonNull<u8>,
        layout: std::alloc::Layout,
        used: usize,
    }

    impl RawBlock {
        fn new(bytes: usize, alignment: usize) -> Self {
            let size = bytes.max(1);
            let align = alignment.max(1).next_power_of_two();
            let layout = std::alloc::Layout::from_size_align(size, align)
                .expect("allocator capacity/alignment overflow");
            // SAFETY: layout is non-zero and valid. Null is handled as allocation failure.
            let raw = unsafe { std::alloc::alloc(layout) };
            let ptr = NonNull::new(raw).unwrap_or_else(|| std::alloc::handle_alloc_error(layout));
            super::jet_observe_arena_retain(layout.size());
            RawBlock { ptr, layout, used: 0 }
        }

        fn capacity(&self) -> usize {
            self.layout.size()
        }

        fn aligned_offset(&self, align: usize, bytes: usize) -> Option<usize> {
            if align > self.layout.align() {
                return None;
            }
            let address = self.ptr.as_ptr() as usize + self.used;
            let padding = (align - address % align) % align;
            let start = self.used.checked_add(padding)?;
            let end = start.checked_add(bytes.max(1))?;
            (end <= self.capacity()).then_some(start)
        }

        unsafe fn write<T>(&mut self, val: T) -> Option<*mut T> {
            let offset = self.aligned_offset(std::mem::align_of::<T>(), std::mem::size_of::<T>())?;
            // SAFETY: aligned_offset proved the typed write fits this live block.
            let ptr = unsafe { self.ptr.as_ptr().add(offset).cast::<T>() };
            unsafe { ptr.write(val) };
            self.used = offset + std::mem::size_of::<T>().max(1);
            Some(ptr)
        }

        fn rewind(&mut self) {
            self.used = 0;
        }
    }

    impl Drop for RawBlock {
        fn drop(&mut self) {
            super::jet_observe_arena_release(self.layout.size());
            // SAFETY: ptr was allocated with this exact layout and values were dropped first.
            unsafe { std::alloc::dealloc(self.ptr.as_ptr(), self.layout) };
        }
    }

    fn drop_entries(entries: &mut Vec<DropEntry>) {
        for entry in entries.drain(..).rev() {
            // SAFETY: each entry corresponds to one initialized, still-live value.
            unsafe { (entry.drop_fn)(entry.ptr) };
        }
    }

    fn observe_alloc<T>() -> usize {
        let bytes = std::mem::size_of::<T>();
        super::jet_observe_arena_alloc(bytes);
        bytes
    }

    fn record_drop<T>(drops: &mut Vec<DropEntry>, ptr: *mut T) {
        if std::mem::needs_drop::<T>() {
            drops.push(DropEntry { ptr: ptr.cast::<u8>(), drop_fn: drop_at::<T> });
        }
    }

    struct ArenaState {
        blocks: Vec<RawBlock>,
        current_block: usize,
        drops: Vec<DropEntry>,
        live_allocations: usize,
        live_bytes: usize,
        high_water_bytes: usize,
        next_block_bytes: usize,
    }

    /// General arena: heterogeneous allocations grow across retained chunks.
    pub struct JetArena {
        state: RefCell<ArenaState>,
    }

    impl JetArena {
        pub fn new() -> Self {
            Self::with_capacity(DEFAULT_ARENA_BYTES)
        }

        pub fn with_capacity(cap: usize) -> Self {
            super::jet_observe_arena_open();
            let first = cap.max(1);
            JetArena {
                state: RefCell::new(ArenaState {
                    blocks: vec![RawBlock::new(first, BASE_ALIGNMENT)],
                    current_block: 0,
                    drops: Vec::new(),
                    live_allocations: 0,
                    live_bytes: 0,
                    high_water_bytes: 0,
                    next_block_bytes: first.saturating_mul(2),
                }),
            }
        }

        pub fn alloc<T: 'static>(&self, val: T) -> &mut T {
            let mut state = self.state.borrow_mut();
            let required = std::mem::size_of::<T>().max(1)
                .saturating_add(std::mem::align_of::<T>());
            let selected = (state.current_block..state.blocks.len()).find(|index| {
                state.blocks[*index]
                    .aligned_offset(std::mem::align_of::<T>(), std::mem::size_of::<T>())
                    .is_some()
            });
            let selected = if let Some(index) = selected {
                index
            } else {
                let bytes = state.next_block_bytes.max(required);
                state.blocks.push(RawBlock::new(bytes, BASE_ALIGNMENT.max(std::mem::align_of::<T>())));
                state.next_block_bytes = bytes.saturating_mul(2);
                state.blocks.len() - 1
            };
            state.current_block = selected;
            // SAFETY: the selected block was sized/aligned above and owns the value until reset.
            let ptr = unsafe { state.blocks[selected].write(val).unwrap() };
            record_drop(&mut state.drops, ptr);
            let bytes = observe_alloc::<T>();
            state.live_allocations += 1;
            state.live_bytes = state.live_bytes.saturating_add(bytes);
            state.high_water_bytes = state.high_water_bytes.max(state.live_bytes);
            // SAFETY: ptr remains in a retained block; &self ties the view to this arena.
            unsafe { &mut *ptr }
        }

        pub fn facts(&self) -> AllocatorFacts {
            let state = self.state.borrow();
            AllocatorFacts {
                live_allocations: state.live_allocations,
                live_bytes: state.live_bytes,
                retained_bytes: state.blocks.iter().map(RawBlock::capacity).sum(),
                high_water_bytes: state.high_water_bytes,
            }
        }

        pub fn reset(&mut self) {
            let state = self.state.get_mut();
            drop_entries(&mut state.drops);
            super::jet_observe_arena_reset(state.live_allocations, state.live_bytes);
            state.live_allocations = 0;
            state.live_bytes = 0;
            for block in &mut state.blocks {
                block.rewind();
            }
            state.current_block = 0;
        }

    }

    impl Drop for JetArena {
        fn drop(&mut self) {
            self.reset();
            super::jet_observe_arena_close();
        }
    }

    struct BumpState {
        block: RawBlock,
        drops: Vec<DropEntry>,
        live_allocations: usize,
        live_bytes: usize,
        high_water_bytes: usize,
    }

    /// Bump allocator: one caller-sized contiguous buffer, monotonic until reset.
    pub struct JetBump {
        state: RefCell<BumpState>,
    }

    impl JetBump {
        pub fn new() -> Self {
            Self::with_capacity(DEFAULT_BUMP_BYTES)
        }

        pub fn with_capacity(cap: usize) -> Self {
            super::jet_observe_arena_open();
            JetBump {
                state: RefCell::new(BumpState {
                    block: RawBlock::new(cap.max(1), BASE_ALIGNMENT),
                    drops: Vec::new(),
                    live_allocations: 0,
                    live_bytes: 0,
                    high_water_bytes: 0,
                }),
            }
        }

        pub fn alloc<T: 'static>(&self, val: T) -> &mut T {
            let mut state = self.state.borrow_mut();
            // SAFETY: RawBlock validates capacity and alignment before writing.
            let ptr = unsafe { state.block.write(val) }
                .unwrap_or_else(|| panic!("Bump allocator exhausted its contiguous buffer"));
            record_drop(&mut state.drops, ptr);
            let bytes = observe_alloc::<T>();
            state.live_allocations += 1;
            state.live_bytes = state.live_bytes.saturating_add(bytes);
            state.high_water_bytes = state.high_water_bytes.max(state.live_bytes);
            // SAFETY: ptr remains in the single retained block until reset/close.
            unsafe { &mut *ptr }
        }

        pub fn facts(&self) -> AllocatorFacts {
            let state = self.state.borrow();
            AllocatorFacts {
                live_allocations: state.live_allocations,
                live_bytes: state.live_bytes,
                retained_bytes: state.block.capacity(),
                high_water_bytes: state.high_water_bytes,
            }
        }

        pub fn reset(&mut self) {
            let state = self.state.get_mut();
            drop_entries(&mut state.drops);
            super::jet_observe_arena_reset(state.live_allocations, state.live_bytes);
            state.live_allocations = 0;
            state.live_bytes = 0;
            state.block.rewind();
        }

    }

    impl Drop for JetBump {
        fn drop(&mut self) {
            self.reset();
            super::jet_observe_arena_close();
        }
    }

    struct PoolSlot {
        block: Option<RawBlock>,
        occupied: bool,
    }

    struct PoolState {
        slots: Vec<PoolSlot>,
        generation: u64,
        drops: Vec<DropEntry>,
        live_bytes: usize,
        high_water_bytes: usize,
    }

    /// Pool allocator: a fixed slot count with retained size/alignment classes.
    pub struct JetPool {
        state: RefCell<PoolState>,
    }

    impl JetPool {
        pub fn new() -> Self {
            Self::with_slots(DEFAULT_POOL_SLOTS)
        }

        pub fn with_slots(slots: usize) -> Self {
            super::jet_observe_arena_open();
            JetPool {
                state: RefCell::new(PoolState {
                    slots: (0..slots)
                        .map(|_| PoolSlot { block: None, occupied: false })
                        .collect(),
                    generation: 1,
                    drops: Vec::new(),
                    live_bytes: 0,
                    high_water_bytes: 0,
                }),
            }
        }

        pub fn alloc<T: 'static>(&self, val: T) -> &mut T {
            let mut state = self.state.borrow_mut();
            let bytes = std::mem::size_of::<T>().max(1);
            let align = std::mem::align_of::<T>();
            let compatible = state.slots.iter().position(|slot| {
                !slot.occupied
                    && slot.block.as_ref().is_some_and(|block| {
                        block.capacity() >= bytes && block.layout.align() >= align
                    })
            });
            let selected = compatible
                .or_else(|| state.slots.iter().position(|slot| !slot.occupied && slot.block.is_none()))
                .or_else(|| state.slots.iter().position(|slot| !slot.occupied))
                .unwrap_or_else(|| panic!("Pool allocator exhausted its slab slots"));
            let slot = &mut state.slots[selected];
            let replace = slot.block.as_ref().map_or(true, |block| {
                block.capacity() < bytes || block.layout.align() < align
            });
            if replace {
                slot.block = Some(RawBlock::new(bytes, align));
            }
            let block = slot.block.as_mut().unwrap();
            block.rewind();
            // SAFETY: the retained slot class is capacity/alignment compatible and unique.
            let ptr = unsafe { block.write(val).unwrap() };
            slot.occupied = true;
            record_drop(&mut state.drops, ptr);
            let live_bytes = observe_alloc::<T>();
            state.live_bytes = state.live_bytes.saturating_add(live_bytes);
            state.high_water_bytes = state.high_water_bytes.max(state.live_bytes);
            // SAFETY: ptr remains in the retained slab until reset/close.
            unsafe { &mut *ptr }
        }

        pub fn facts(&self) -> AllocatorFacts {
            let state = self.state.borrow();
            AllocatorFacts {
                live_allocations: state.slots.iter().filter(|slot| slot.occupied).count(),
                live_bytes: state.live_bytes,
                retained_bytes: state
                    .slots
                    .iter()
                    .filter_map(|slot| slot.block.as_ref())
                    .map(RawBlock::capacity)
                    .sum(),
                high_water_bytes: state.high_water_bytes,
            }
        }

        pub fn generation(&self) -> u64 {
            self.state.borrow().generation
        }

        pub fn reset(&mut self) {
            let state = self.state.get_mut();
            drop_entries(&mut state.drops);
            let allocations = state.slots.iter().filter(|slot| slot.occupied).count();
            super::jet_observe_arena_reset(allocations, state.live_bytes);
            for slot in &mut state.slots {
                slot.occupied = false;
                if let Some(block) = &mut slot.block {
                    block.rewind();
                }
            }
            state.live_bytes = 0;
            state.generation = state.generation.wrapping_add(1).max(1);
        }

    }

    impl Drop for JetPool {
        fn drop(&mut self) {
            self.reset();
            super::jet_observe_arena_close();
        }
    }

    #[derive(Clone, Copy)]
    struct FixedHeader {
        previous: usize,
        value_offset: usize,
        drop_fn: unsafe fn(*mut u8),
    }

    struct FixedState {
        ptr: NonNull<u8>,
        capacity: usize,
        used: usize,
        metadata_start: usize,
        last_header: usize,
        live_allocations: usize,
        live_bytes: usize,
        high_water_bytes: usize,
    }

    /// A fixed allocator over exactly one caller-owned inline byte buffer.
    /// Payloads, alignment padding, and reverse-drop metadata all consume that
    /// buffer; exhaustion therefore has one deterministic capacity boundary.
    /// This module is private generated runtime: Jet sema proves the backing
    /// outlives the handle and rejects every escape before this raw-pointer seam.
    pub struct JetFixed {
        state: RefCell<FixedState>,
        _thread_confined: std::marker::PhantomData<std::rc::Rc<()>>,
    }

    impl JetFixed {
        pub fn over(bytes: &mut [u8]) -> Self {
            // SAFETY: initialized bytes are also valid MaybeUninit<u8> storage.
            Self::over_raw(bytes.as_mut_ptr(), bytes.len())
        }

        pub fn over_uninit(bytes: &mut [std::mem::MaybeUninit<u8>]) -> Self {
            Self::over_raw(bytes.as_mut_ptr().cast::<u8>(), bytes.len())
        }

        pub fn over_uninit_fixed<const N: usize>(
            bytes: &mut JetUninitFixed<u8, N>,
        ) -> Self {
            Self::over_uninit(bytes.uninit_bytes())
        }

        fn over_raw(ptr: *mut u8, capacity: usize) -> Self {
            assert!(capacity > 0, "Fixed allocator needs a non-empty backing buffer");
            super::jet_observe_arena_open();
            super::jet_observe_arena_retain(capacity);
            JetFixed {
                state: RefCell::new(FixedState {
                    ptr: NonNull::new(ptr).expect("non-empty Fixed backing buffer was null"),
                    capacity,
                    used: 0,
                    metadata_start: capacity,
                    last_header: usize::MAX,
                    live_allocations: 0,
                    live_bytes: 0,
                    high_water_bytes: 0,
                }),
                _thread_confined: std::marker::PhantomData,
            }
        }

        fn aligned_offset(base: usize, cursor: usize, align: usize) -> Option<usize> {
            let address = base.checked_add(cursor)?;
            let padding = (align - address % align) % align;
            cursor.checked_add(padding)
        }

        fn aligned_down_offset(
            base: usize,
            end: usize,
            size: usize,
            align: usize,
        ) -> Option<usize> {
            let unaligned = end.checked_sub(size)?;
            let address = base.checked_add(unaligned)?;
            unaligned.checked_sub(address % align)
        }

        pub fn alloc<T: 'static>(&self, val: T) -> &mut T {
            let mut state = self.state.borrow_mut();
            let base = state.ptr.as_ptr() as usize;
            let value_offset = Self::aligned_offset(base, state.used, std::mem::align_of::<T>());
            let end = value_offset.and_then(|offset| {
                offset.checked_add(std::mem::size_of::<T>().max(1))
            });
            let header_offset = Self::aligned_down_offset(
                base,
                state.metadata_start,
                std::mem::size_of::<FixedHeader>(),
                std::mem::align_of::<FixedHeader>(),
            );
            let (header_offset, value_offset, end) = match (header_offset, value_offset, end) {
                (Some(header), Some(value), Some(end)) if end <= header => {
                    (header, value, end)
                }
                _ => panic!("Fixed allocator exhausted its inline backing buffer"),
            };
            // SAFETY: both offsets were aligned against the real backing address
            // and the complete header/payload range was checked against capacity.
            unsafe {
                state.ptr.as_ptr().add(header_offset).cast::<FixedHeader>().write(FixedHeader {
                    previous: state.last_header,
                    value_offset,
                    drop_fn: drop_at::<T>,
                });
                state.ptr.as_ptr().add(value_offset).cast::<T>().write(val);
            }
            state.last_header = header_offset;
            state.metadata_start = header_offset;
            state.used = end;
            let bytes = observe_alloc::<T>();
            state.live_allocations += 1;
            state.live_bytes = state.live_bytes.saturating_add(bytes);
            state.high_water_bytes = state.high_water_bytes.max(state.live_bytes);
            let ptr = unsafe { state.ptr.as_ptr().add(value_offset).cast::<T>() };
            drop(state);
            // SAFETY: the value stays in caller-owned backing until reset/close;
            // sema rejects reset, escape, capture, or owner mutation while live.
            unsafe { &mut *ptr }
        }

        pub fn facts(&self) -> AllocatorFacts {
            let state = self.state.borrow();
            AllocatorFacts {
                live_allocations: state.live_allocations,
                live_bytes: state.live_bytes,
                retained_bytes: state.capacity,
                high_water_bytes: state.high_water_bytes,
            }
        }

        pub fn reset(&mut self) {
            let state = self.state.get_mut();
            let mut header_offset = state.last_header;
            while header_offset != usize::MAX {
                // SAFETY: every link was written by alloc within this buffer.
                let header = unsafe {
                    state.ptr.as_ptr().add(header_offset).cast::<FixedHeader>().read()
                };
                unsafe { (header.drop_fn)(state.ptr.as_ptr().add(header.value_offset)) };
                header_offset = header.previous;
            }
            super::jet_observe_arena_reset(state.live_allocations, state.live_bytes);
            state.used = 0;
            state.metadata_start = state.capacity;
            state.last_header = usize::MAX;
            state.live_allocations = 0;
            state.live_bytes = 0;
        }
    }

    impl Drop for JetFixed {
        fn drop(&mut self) {
            self.reset();
            super::jet_observe_arena_release(self.state.get_mut().capacity);
            super::jet_observe_arena_close();
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
