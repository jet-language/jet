// Vetted memory helper (I1): AOT embeds this file inside `mod jet_mem`
// (`Codegen::push_mem_prelude`); the JIT and comptime collection kernels
// `include!` it in their own `mod jet_mem` next to `Core/Collections.rs`.

/// #4566 slice 8 inline storage (`plan_inline_lists`): a list local that
/// never leaves its function, whose expected length the emitter knows.
/// Its first `N` values live inline in the frame; a push past `N` moves
/// them to the heap, so the list's contents never depend on the bound.
/// Reads see `[T]`. The list surface traits (`JetListSurface`,
/// `JetListPushSurface`) are implemented in safe code in the Collections
/// prelude on top of `new`, `push` and `Deref`.
///
/// Soundness invariant: while `heap` is `None`, exactly the slots
/// `inline[..len]` are initialized and owned by the list, and
/// `len <= N`. Once `heap` is `Some`, `len` is 0, no inline slot owns a
/// value, and the `Vec` owns every element. Each mutation below keeps
/// this invariant at every point where a panic can unwind.
///
/// Send/Sync: the auto traits apply. `[MaybeUninit<T>; N]` and
/// `Option<Vec<T>>` are Send/Sync exactly when `T` is, and the list
/// hands out `&T`/`&mut T` only through `&self`/`&mut self`, as `Vec<T>`
/// does, so no extra bound or manual impl is needed.
pub struct JetInlineList<T, const N: usize> {
    len: usize,
    inline: [std::mem::MaybeUninit<T>; N],
    heap: Option<Vec<T>>,
}

impl<T, const N: usize> JetInlineList<T, N> {
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            len: 0,
            inline: std::array::from_fn(|_| std::mem::MaybeUninit::uninit()),
            heap: None,
        }
    }

    #[inline(always)]
    pub fn push(&mut self, value: T) {
        if let Some(heap) = &mut self.heap {
            heap.push(value);
        } else if self.len < N {
            // `write` cannot panic, and `len` grows only after the slot
            // holds the value, so the slot is never counted before it is
            // initialized.
            self.inline[self.len].write(value);
            self.len += 1;
        } else {
            // The spill completes before `value` is pushed; if the
            // final push panics (allocation failure), `value` drops
            // during unwinding and the heap still owns every earlier
            // element.
            self.spill().push(value);
        }
    }

    /// Moves the inline values to the heap, leaving no inline value.
    #[cold]
    fn spill(&mut self) -> &mut Vec<T> {
        // Allocate before touching any slot: if this panics, the
        // invariant still holds (`heap` is `None`, `len` unchanged) and
        // `Drop` releases the inline values.
        let mut heap = Vec::with_capacity(N.max(4) * 2);
        // `len` is zero before the first read, so no path can see a
        // moved-out slot as initialized: a panic from here on would leak
        // the remaining inline values, never drop one twice.
        let len = std::mem::replace(&mut self.len, 0);
        for slot in &self.inline[..len] {
            // SAFETY: `slot` is below the old `len`, so `push` wrote it
            // and nothing has read it out yet. Each slot is read exactly
            // once, and the count was cleared first, so the slot is never
            // dropped or read again. `heap.push` cannot reallocate
            // (capacity `2 * max(N, 4)` > `len`), so no panic happens in
            // the middle of the move.
            heap.push(unsafe { slot.assume_init_read() });
        }
        self.heap.insert(heap)
    }
}

impl<T, const N: usize> std::ops::Deref for JetInlineList<T, N> {
    type Target = [T];

    #[inline(always)]
    fn deref(&self) -> &[T] {
        match &self.heap {
            Some(heap) => heap,
            // SAFETY: with `heap` `None`, `inline[..len]` are initialized
            // and `len <= N`; `MaybeUninit<T>` has `T`'s layout, so the
            // array pointer is a valid, aligned, non-null `*const T` (also
            // for `N == 0`, where `len == 0`). The slice borrows `self`.
            None => unsafe { std::slice::from_raw_parts(self.inline.as_ptr().cast::<T>(), self.len) },
        }
    }
}

impl<T, const N: usize> std::ops::DerefMut for JetInlineList<T, N> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut [T] {
        match &mut self.heap {
            Some(heap) => heap,
            // SAFETY: as in `deref`; the unique `&mut self` borrow makes
            // the mutable slice the only access to those slots.
            None => unsafe {
                std::slice::from_raw_parts_mut(self.inline.as_mut_ptr().cast::<T>(), self.len)
            },
        }
    }
}

impl<T, const N: usize> Drop for JetInlineList<T, N> {
    fn drop(&mut self) {
        // With `heap` `Some`, the `Vec` field drops its elements and no
        // inline slot owns one. Otherwise drop exactly the initialized
        // prefix.
        if self.heap.is_none() {
            let len = std::mem::replace(&mut self.len, 0);
            // SAFETY: `inline[..len]` are initialized and owned, and the
            // count is cleared first, so each value drops exactly once.
            // `drop_in_place` on a slice keeps dropping the remaining
            // elements if one element's `Drop` panics.
            unsafe {
                std::ptr::drop_in_place(std::ptr::slice_from_raw_parts_mut(
                    self.inline.as_mut_ptr().cast::<T>(),
                    len,
                ))
            };
        }
    }
}
