// D-FIXED-KERNEL1=A: one allocation, metadata, and reverse-drop kernel for
// hosted and freestanding `core.mem.Fixed` adapters. The adapter owns only
// backing acquisition and tier-specific observations; this source owns the
// storage boundary and value lifetime.

// AUDIT: FixedState writes typed values and reverse-drops them inside one
// caller-owned backing span. Safe Rust cannot express the typed pointer
// arithmetic and destructor function pointer needed for a heterogeneous arena.
// The caller must provide a live, non-null, addressable span for the full
// capacity and call reset only after no stored value is reachable. Violating
// that contract can write outside the span, double-drop, or dereference
// invalid storage.
use core::mem;
use core::ptr::NonNull;

const EMPTY_HEADER: usize = usize::MAX;

#[derive(Clone, Copy)]
pub struct FixedHeader {
    previous: usize,
    value_offset: usize,
    drop_fn: Option<unsafe fn(*mut u8)>,
    bytes: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FixedResetStats {
    pub live_allocations: usize,
    pub live_bytes: usize,
}

pub struct FixedState {
    ptr: NonNull<u8>,
    capacity: usize,
    used: usize,
    metadata_start: usize,
    last_header: usize,
    live_allocations: usize,
    live_bytes: usize,
    high_water_bytes: usize,
}

unsafe fn drop_at<T>(ptr: *mut u8) {
    ptr.cast::<T>().drop_in_place();
}

impl FixedState {
    /// Construct state over one non-empty, addressable backing span.
    ///
    /// The caller owns the backing resource. This kernel never acquires or
    /// releases it; the adapter decides whether it is inline or heap-backed.
    pub unsafe fn from_raw(ptr: *mut u8, capacity: usize) -> Self {
        assert!(capacity > 0, "Fixed allocator needs a non-empty backing buffer");
        let base = ptr as usize;
        assert!(base.checked_add(capacity).is_some(), "Fixed backing range overflow");
        Self {
            ptr: NonNull::new(ptr).expect("non-empty Fixed backing buffer was null"),
            capacity,
            used: 0,
            metadata_start: capacity,
            last_header: EMPTY_HEADER,
            live_allocations: 0,
            live_bytes: 0,
            high_water_bytes: 0,
        }
    }

    #[inline]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    #[inline]
    pub const fn used(&self) -> usize {
        self.used
    }

    #[inline]
    pub const fn live_allocations(&self) -> usize {
        self.live_allocations
    }

    #[inline]
    pub const fn live_bytes(&self) -> usize {
        self.live_bytes
    }

    #[inline]
    pub const fn high_water_bytes(&self) -> usize {
        self.high_water_bytes
    }

    #[inline]
    fn aligned_offset(base: usize, cursor: usize, align: usize) -> Option<usize> {
        let address = base.checked_add(cursor)?;
        let mask = align.checked_sub(1)?;
        let aligned = address.checked_add(mask)? & !mask;
        aligned.checked_sub(base)
    }

    #[inline]
    fn aligned_down_offset(
        base: usize,
        end: usize,
        size: usize,
        align: usize,
    ) -> Option<usize> {
        let end_address = base.checked_add(end)?;
        let unaligned = end_address.checked_sub(size)?;
        let mask = align.checked_sub(1)?;
        let aligned = unaligned & !mask;
        let offset = aligned.checked_sub(base)?;
        let header_end = offset.checked_add(size)?;
        (header_end <= end).then_some(offset)
    }

    /// Place one value and return its stable typed address. On failure the
    /// value is returned untouched, so the caller can construct its ordinary
    /// `AllocError` without leaking or dropping twice.
    pub fn try_alloc<T: 'static>(&mut self, value: T) -> Result<*mut T, T> {
        let value_bytes = mem::size_of::<T>();
        let reserved_bytes = value_bytes.max(1);
        let header_bytes = mem::size_of::<FixedHeader>();
        let header_align = mem::align_of::<FixedHeader>();
        let base = self.ptr.as_ptr() as usize;

        // A zero-sized value still reserves one cursor byte, preserving the
        // historical Fixed capacity boundary, but its typed pointer comes
        // from the correctly aligned ZST dangling address rather than from a
        // potentially misaligned caller byte buffer.
        let (value_offset, end) = if value_bytes == 0 {
            let end = self.used.checked_add(reserved_bytes);
            let Some(end) = end else {
                return Err(value);
            };
            (
                NonNull::<T>::dangling().as_ptr() as usize,
                end,
            )
        } else {
            let value_align = mem::align_of::<T>().max(1);
            let value_offset = Self::aligned_offset(base, self.used, value_align);
            let end = value_offset.and_then(|offset| offset.checked_add(reserved_bytes));
            let (Some(value_offset), Some(end)) = (value_offset, end) else {
                return Err(value);
            };
            (value_offset, end)
        };

        let Some(header_offset) = Self::aligned_down_offset(
            base,
            self.metadata_start,
            header_bytes,
            header_align,
        ) else {
            return Err(value);
        };
        if end > header_offset {
            return Err(value);
        }

        // SAFETY: the checked header range is within the backing span.
        let header_ptr = unsafe {
            self.ptr
                .as_ptr()
                .add(header_offset)
                .cast::<FixedHeader>()
        };
        let value_ptr = if value_bytes == 0 {
            value_offset as *mut T
        } else {
            // SAFETY: `value_offset + reserved_bytes` was checked against the
            // metadata boundary, so this typed pointer is inside the span.
            unsafe { self.ptr.as_ptr().add(value_offset).cast::<T>() }
        };
        // SAFETY: header_ptr is aligned for FixedHeader; value_ptr is aligned
        // and reserves either a real payload range or a valid dangling ZST
        // address.
        unsafe {
            header_ptr.write(FixedHeader {
                previous: self.last_header,
                value_offset,
                drop_fn: mem::needs_drop::<T>().then_some(drop_at::<T>),
                bytes: value_bytes,
            });
            value_ptr.write(value);
        }
        self.last_header = header_offset;
        self.metadata_start = header_offset;
        self.used = end;
        self.live_allocations = self.live_allocations.saturating_add(1);
        self.live_bytes = self.live_bytes.saturating_add(value_bytes);
        self.high_water_bytes = self.high_water_bytes.max(self.live_bytes);
        Ok(value_ptr)
    }

    /// Drop all live values in reverse allocation order and rewind cursors.
    /// `after_drop` is the adapter's optional quarantine/poison observation;
    /// it runs only after the value's destructor has completed.
    pub unsafe fn reset(&mut self, mut after_drop: impl FnMut(*mut u8, usize)) -> FixedResetStats {
        let stats = FixedResetStats {
            live_allocations: self.live_allocations,
            live_bytes: self.live_bytes,
        };
        let base = self.ptr.as_ptr();
        let mut header_offset = self.last_header;
        while header_offset != EMPTY_HEADER {
            // SAFETY: every link was written by `try_alloc` inside this
            // backing span and has not been overwritten before this walk.
            let header = unsafe {
                base.add(header_offset)
                    .cast::<FixedHeader>()
                    .read()
            };
            let value_ptr = if header.bytes == 0 {
                header.value_offset as *mut u8
            } else {
                // SAFETY: non-ZST offsets were checked against the backing
                // span before their matching header was written.
                unsafe { base.add(header.value_offset) }
            };
            if let Some(drop_fn) = header.drop_fn {
                // SAFETY: the function pointer and address were recorded by
                // the matching typed allocation.
                unsafe { drop_fn(value_ptr) };
            }
            after_drop(value_ptr, header.bytes);
            header_offset = header.previous;
        }
        self.used = 0;
        self.metadata_start = self.capacity;
        self.last_header = EMPTY_HEADER;
        self.live_allocations = 0;
        self.live_bytes = 0;
        stats
    }
}
