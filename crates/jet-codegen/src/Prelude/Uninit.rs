/// Canonical initialization-state policy for uninitialized storage.
///
/// AOT, web, resident JIT, and TIR evaluation include this exact source. Each
/// engine owns only its value carrier; bounds and readiness live here.
pub fn jet_uninit_bitmap(len: usize) -> Vec<bool> {
    vec![false; len]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetUninitAccessError {
    OutOfBounds,
    Uninitialized,
}

pub fn jet_uninit_write(
    bitmap: &mut [bool],
    index: usize,
) -> Result<(usize, bool), JetUninitAccessError> {
    if index >= bitmap.len() {
        return Err(JetUninitAccessError::OutOfBounds);
    }
    let replaced = bitmap[index];
    bitmap[index] = true;
    Ok((index, replaced))
}

pub fn jet_uninit_read(
    bitmap: &[bool],
    index: usize,
) -> Result<usize, JetUninitAccessError> {
    if index >= bitmap.len() {
        return Err(JetUninitAccessError::OutOfBounds);
    }
    if !bitmap[index] {
        return Err(JetUninitAccessError::Uninitialized);
    }
    Ok(index)
}

pub fn jet_uninit_all(bitmap: &[bool]) -> Result<(), JetUninitAccessError> {
    if bitmap.iter().all(|initialized| *initialized) {
        Ok(())
    } else {
        Err(JetUninitAccessError::Uninitialized)
    }
}

/// Safe storage for one value that sema proves is written before use.
/// Invalid scalar bit patterns never exist: `MaybeUninit` stays inside this
/// vetted Prelude module until `write` establishes a real `T`.
pub struct JetUninit<T> {
    value: std::mem::MaybeUninit<T>,
    initialized: bool,
}

impl<T> JetUninit<T> {
    pub fn new() -> Self {
        Self {
            value: std::mem::MaybeUninit::uninit(),
            initialized: false,
        }
    }

    pub fn write(&mut self, value: T) {
        if self.initialized {
            // SAFETY: the flag is set only after a successful write.
            unsafe { self.value.assume_init_drop() };
        }
        self.value.write(value);
        self.initialized = true;
    }

    pub fn read(&self) -> &T {
        assert!(self.initialized, "value read before initialization");
        // SAFETY: the checked flag proves `write` initialized the value.
        unsafe { self.value.assume_init_ref() }
    }
}

impl<T> Drop for JetUninit<T> {
    fn drop(&mut self) {
        if self.initialized {
            // SAFETY: the checked flag proves `write` initialized the value.
            unsafe { self.value.assume_init_drop() };
        }
    }
}

/// Safe storage for a fixed list whose elements are filled before use.
/// The payload bytes are not zeroed. The initialized bitmap makes the safe
/// indexing API sound even if a compiler regression misses the sema proof.
pub struct JetUninitFixed<T, const N: usize> {
    values: [std::mem::MaybeUninit<T>; N],
    initialized: [bool; N],
}

impl<T, const N: usize> JetUninitFixed<T, N> {
    pub fn new() -> Self {
        Self {
            values: std::array::from_fn(|_| std::mem::MaybeUninit::uninit()),
            initialized: [false; N],
        }
    }

    pub fn len(&self) -> usize {
        N
    }

    pub fn write(&mut self, index: usize, value: T) {
        let (index, replaced) = jet_uninit_write(&mut self.initialized, index)
            .expect("fixed-list index out of range");
        if replaced {
            // SAFETY: the bitmap is set only after `write` initializes this slot.
            unsafe { self.values[index].assume_init_drop() };
        }
        self.values[index].write(value);
        self.initialized[index] = true;
    }

    pub fn write_array(&mut self, values: [T; N]) {
        for (index, value) in values.into_iter().enumerate() {
            self.write(index, value);
        }
    }

    pub fn read_array(&self) -> [T; N]
    where
        T: Clone,
    {
        jet_uninit_all(&self.initialized)
            .expect("fixed list read before every element was initialized");
        std::array::from_fn(|index| self[index].clone())
    }

    pub fn as_array(&self) -> &[T; N] {
        jet_uninit_all(&self.initialized)
            .expect("fixed list read before every element was initialized");
        // SAFETY: every slot is initialized and `[MaybeUninit<T>; N]` has the
        // same layout as `[T; N]`.
        unsafe { &*((&self.values as *const [std::mem::MaybeUninit<T>; N]).cast::<[T; N]>()) }
    }

    pub fn as_array_mut(&mut self) -> &mut [T; N] {
        jet_uninit_all(&self.initialized)
            .expect("fixed list read before every element was initialized");
        // SAFETY: every slot is initialized and the exclusive borrow prevents
        // aliasing while the ordinary fixed-list view is live.
        unsafe {
            &mut *((&mut self.values as *mut [std::mem::MaybeUninit<T>; N]).cast::<[T; N]>())
        }
    }

    pub fn uninit_bytes(&mut self) -> &mut [std::mem::MaybeUninit<T>] {
        &mut self.values
    }
}

impl<T, const N: usize> std::ops::Index<usize> for JetUninitFixed<T, N> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        let index =
            jet_uninit_read(&self.initialized, index).expect("fixed-list element is not readable");
        // SAFETY: the canonical readiness check proves this slot was initialized.
        unsafe { self.values[index].assume_init_ref() }
    }
}

impl<T, const N: usize> Drop for JetUninitFixed<T, N> {
    fn drop(&mut self) {
        for index in 0..N {
            if self.initialized[index] {
                // SAFETY: the checked bitmap proves this slot was initialized.
                unsafe { self.values[index].assume_init_drop() };
            }
        }
    }
}
