// D-PLACE1=A: one scalar carrier for every execution tier.
//
// The checked `Atomic<T>` surface is deliberately closed to the scalar set
// represented by this module. The compiler lowers every tier to this same
// carrier; no tier supplies a lock or a target-specific fallback. `T` remains
// a type parameter so generated records retain their source-facing shape while
// the cell stores one carrier word.
use core::marker::PhantomData;
use core::sync::atomic::{AtomicU64, Ordering};
use jet_foundation::Outcome::AllocError;

/// Marker for Jet's exact `Int` atomic lane.
///
/// Jet's source `Int` and fixed-width `I64` both use an `i64` ABI word, but
/// they have different arithmetic and comparison laws. Keeping this marker
/// distinct makes that difference part of the generated Rust type rather than
/// a convention hidden in an adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JetAtomicInt;

/// Scalar representation accepted by the checked `Atomic<T>` surface.
///
/// `Wire` is the one-word value handed to the atomic cell. Fixed-width lanes
/// use their native scalar. Exact `Int` uses a raw i64 ABI word backed by the
/// Foundation's owned `JetInt` nodes.
pub trait JetAtomicValue: Send + Sync + 'static {
    type Wire: Send + Sync + 'static;
    type Guard: Default;
    /// Clone a borrowed wire before publishing it in the cell.
    fn jet_atomic_clone_value(value: &Self::Wire) -> Self::Wire;
    /// Encode the independently owned publication value into one word.
    fn jet_atomic_into_bits(value: Self::Wire) -> u64;
    /// Drop a publication word that still owns a spilled exact node. Copy
    /// lanes leave the word untouched.
    fn jet_atomic_drop_bits(_bits: u64) {}

    /// Clone a value observed in an atomic word. The reload callback validates
    /// that the word is still current while the exact carrier retains it.
    fn jet_atomic_clone_from_bits<F>(bits: u64, reload: F) -> Self::Wire
    where
        F: FnMut() -> u64;
    /// Drop an owned wire value that has not been published. Copy lanes are
    /// no-ops; the exact lane releases its spilled node reference.
    fn jet_atomic_drop_value(value: Self::Wire) {
        let bits = Self::jet_atomic_into_bits(value);
        Self::jet_atomic_drop_bits(bits);
    }

    fn jet_atomic_equal(left: &Self::Wire, right: &Self::Wire) -> bool;
    fn jet_atomic_equal_bits<F>(bits: u64, right: &Self::Wire, reload: F) -> bool
    where
        F: FnMut() -> u64,
    {
        let left = Self::jet_atomic_clone_from_bits(bits, reload);
        Self::jet_atomic_equal(&left, right)
    }
    /// Compare while retaining any owner cloned from the publication word.
    /// The guard must live through the following CAS so pointer reuse cannot
    /// turn a value comparison into an ABA match.
    fn jet_atomic_equal_and_hold_bits<F>(
        bits: u64,
        right: &Self::Wire,
        reload: F,
    ) -> (bool, Self::Guard)
    where
        F: FnMut() -> u64,
    {
        (
            Self::jet_atomic_equal_bits(bits, right, reload),
            Self::Guard::default(),
        )
    }
}

macro_rules! impl_copy_atomic_value {
    ($ty:ty, $encode:expr, $decode:expr) => {
        impl JetAtomicValue for $ty {
            type Wire = $ty;
            type Guard = ();

            #[inline(always)]
            fn jet_atomic_clone_value(value: &Self::Wire) -> Self::Wire {
                *value
            }
            fn jet_atomic_into_bits(value: Self::Wire) -> u64 {
                ($encode)(value)
            }

            #[inline(always)]
            fn jet_atomic_clone_from_bits<F>(bits: u64, _reload: F) -> Self::Wire
            where
                F: FnMut() -> u64,
            {
                ($decode)(bits)
            }

            #[inline(always)]
            fn jet_atomic_drop_bits(_bits: u64) {}

            #[inline(always)]
            fn jet_atomic_equal(left: &Self::Wire, right: &Self::Wire) -> bool {
                left == right
            }
        }
    };
}

impl_copy_atomic_value!(bool, |value: bool| if value { 1 } else { 0 }, |bits: u64| {
    bits != 0
});
impl_copy_atomic_value!(i32, |value: i32| u64::from(value as u32), |bits: u64| {
    bits as u32 as i32
});
impl_copy_atomic_value!(u32, |value: u32| u64::from(value), |bits: u64| {
    bits as u32
});
impl_copy_atomic_value!(i64, |value: i64| value as u64, |bits: u64| {
    bits as i64
});
impl_copy_atomic_value!(u64, |value: u64| value, |bits: u64| bits);
impl JetAtomicValue for JetAtomicInt {
    type Wire = i64;
    type Guard = Option<jet_foundation::Numeric::JetInt>;

    // The generated Rust and C ABI already carry source `Int` as one i64
    // word. Each publication clones its borrowed exact owner; the cell then
    // releases that clone independently.
    fn jet_atomic_clone_value(value: &Self::Wire) -> Self::Wire {
        // SAFETY: adapters borrow an owned raw word and retain its node.
        unsafe { jet_foundation::Numeric::JetInt::clone_from_raw(*value) }.into_raw()
    }

    #[inline(always)]
    fn jet_atomic_into_bits(value: Self::Wire) -> u64 {
        value as u64
    }

    #[inline(always)]
    fn jet_atomic_clone_from_bits<F>(bits: u64, reload: F) -> Self::Wire
    where
        F: FnMut() -> u64,
    {
        jet_foundation::Numeric::JetInt::clone_from_atomic(bits, reload).into_raw()
    }

    #[inline(always)]
    fn jet_atomic_drop_bits(bits: u64) {
        // SAFETY: the cell owns exactly one reference for its publication
        // word, and this function consumes that reference.
        unsafe {
            drop(jet_foundation::Numeric::JetInt::from_raw_owned(bits as i64));
        }
    }

    #[inline(always)]
    fn jet_atomic_equal(left: &Self::Wire, right: &Self::Wire) -> bool {
        let left = unsafe { jet_foundation::Numeric::JetInt::clone_from_raw(*left) };
        let right = unsafe { jet_foundation::Numeric::JetInt::clone_from_raw(*right) };
        left == right
    }

    #[inline(always)]
    fn jet_atomic_equal_bits<F>(bits: u64, right: &Self::Wire, reload: F) -> bool
    where
        F: FnMut() -> u64,
    {
        let left = jet_foundation::Numeric::JetInt::clone_from_atomic(bits, reload);
        let right = unsafe { jet_foundation::Numeric::JetInt::clone_from_raw(*right) };
        left == right
    }

    #[inline(always)]
    fn jet_atomic_equal_and_hold_bits<F>(
        bits: u64,
        right: &Self::Wire,
        reload: F,
    ) -> (bool, Self::Guard)
    where
        F: FnMut() -> u64,
    {
        let left = jet_foundation::Numeric::JetInt::clone_from_atomic(bits, reload);
        let right = unsafe { jet_foundation::Numeric::JetInt::clone_from_raw(*right) };
        let equal = left == right;
        (equal, Some(left))
    }
}

/// Numeric subset for which `Atomic<T>.add(delta)` is meaningful.
///
/// `Bool` intentionally implements `JetAtomicValue` but not this trait: the
/// checker rejects `add` on a boolean atomic instead of inventing arithmetic
/// semantics for a predicate value. Fixed-width lanes use their ratified
/// wrapping arithmetic. Exact `Int` delegates to the surrounding tier's
/// canonical exact-integer operation.
pub trait JetAtomicAdd: JetAtomicValue {
    fn jet_atomic_add(left: &Self::Wire, delta: &Self::Wire) -> Self::Wire;
}

impl JetAtomicAdd for i32 {
    #[inline(always)]
    fn jet_atomic_add(left: &Self::Wire, delta: &Self::Wire) -> Self::Wire {
        left.wrapping_add(*delta)
    }
}

impl JetAtomicAdd for u32 {
    #[inline(always)]
    fn jet_atomic_add(left: &Self::Wire, delta: &Self::Wire) -> Self::Wire {
        left.wrapping_add(*delta)
    }
}

impl JetAtomicAdd for i64 {
    #[inline(always)]
    fn jet_atomic_add(left: &Self::Wire, delta: &Self::Wire) -> Self::Wire {
        left.wrapping_add(*delta)
    }
}

impl JetAtomicAdd for u64 {
    #[inline(always)]
    fn jet_atomic_add(left: &Self::Wire, delta: &Self::Wire) -> Self::Wire {
        left.wrapping_add(*delta)
    }
}

impl JetAtomicAdd for JetAtomicInt {
    #[inline(always)]
    fn jet_atomic_add(left: &Self::Wire, delta: &Self::Wire) -> Self::Wire {
        let left = unsafe { jet_foundation::Numeric::JetInt::clone_from_raw(*left) };
        let delta = unsafe { jet_foundation::Numeric::JetInt::clone_from_raw(*delta) };
        left.add(&delta)
            .unwrap_or_else(|_| std::process::abort())
            .into_raw()
    }
}

/// A non-copyable scalar cell with lock-free word operations.
///
/// The cell is inline so it remains one machine word in compiler-owned
/// `#Layout(c)` records. Cloning the cell would create a different
/// synchronization location, so this type intentionally does not implement
/// `Clone`.
#[repr(transparent)]
pub struct JetAtomic<T: JetAtomicValue> {
    bits: AtomicU64,
    marker: PhantomData<T>,
}

/// Fallible operations for scalar lanes whose update may need ordinary
/// numeric storage. Fixed-width lanes never allocate; exact `Int` returns the
/// canonical allocator's structured refusal before publication.
pub trait JetAtomicFallible: JetAtomicValue {
    fn jet_atomic_try_new(value: Self::Wire) -> Result<Self::Wire, AllocError> {
        Ok(value)
    }

    fn jet_atomic_try_add(
        left: &Self::Wire,
        delta: &Self::Wire,
    ) -> Result<Self::Wire, AllocError>;
}

impl JetAtomicFallible for i32 {
    #[inline(always)]
    fn jet_atomic_try_add(left: &Self::Wire, delta: &Self::Wire) -> Result<Self::Wire, AllocError> {
        Ok(left.wrapping_add(*delta))
    }
}

impl JetAtomicFallible for u32 {
    #[inline(always)]
    fn jet_atomic_try_add(left: &Self::Wire, delta: &Self::Wire) -> Result<Self::Wire, AllocError> {
        Ok(left.wrapping_add(*delta))
    }
}

impl JetAtomicFallible for i64 {
    #[inline(always)]
    fn jet_atomic_try_add(left: &Self::Wire, delta: &Self::Wire) -> Result<Self::Wire, AllocError> {
        Ok(left.wrapping_add(*delta))
    }
}

impl JetAtomicFallible for u64 {
    #[inline(always)]
    fn jet_atomic_try_add(left: &Self::Wire, delta: &Self::Wire) -> Result<Self::Wire, AllocError> {
        Ok(left.wrapping_add(*delta))
    }
}

impl JetAtomicFallible for JetAtomicInt {
    #[inline(always)]
    fn jet_atomic_try_new(value: Self::Wire) -> Result<Self::Wire, AllocError> {
        Ok(value)
    }

    #[inline(always)]
    fn jet_atomic_try_add(
        left: &Self::Wire,
        delta: &Self::Wire,
    ) -> Result<Self::Wire, AllocError> {
        let left = unsafe { jet_foundation::Numeric::JetInt::clone_from_raw(*left) };
        let delta = unsafe { jet_foundation::Numeric::JetInt::clone_from_raw(*delta) };
        left.try_add(&delta)
            .map(jet_foundation::Numeric::JetInt::into_raw)
    }
}

impl<T: JetAtomicValue> JetAtomic<T> {
    pub fn new(value: T::Wire) -> Self {
        Self {
            bits: AtomicU64::new(T::jet_atomic_into_bits(value)),
            marker: PhantomData,
        }
    }

    pub fn try_new(value: T::Wire) -> Result<Self, AllocError>
    where
        T: JetAtomicFallible,
    {
        Ok(Self::new(T::jet_atomic_try_new(value)?))
    }

    pub fn load(&self) -> T::Wire {
        let bits = self.bits.load(Ordering::SeqCst);
        T::jet_atomic_clone_from_bits(bits, || self.bits.load(Ordering::SeqCst))
    }

    pub fn observe(&self) -> T::Wire {
        self.load()
    }

    pub fn store(&self, value: T::Wire) {
        let next = T::jet_atomic_into_bits(value);
        let previous = self.bits.swap(next, Ordering::SeqCst);
        T::jet_atomic_drop_bits(previous);
    }

    pub fn publish(&self, next: T::Wire) {
        self.store(next);
    }

    pub fn add(&self, delta: T::Wire) -> T::Wire
    where
        T: JetAtomicAdd,
    {
        loop {
            let bits = self.bits.load(Ordering::SeqCst);
            let current = T::jet_atomic_clone_from_bits(bits, || self.bits.load(Ordering::SeqCst));
            let next = T::jet_atomic_add(&current, &delta);
            let published = T::jet_atomic_into_bits(T::jet_atomic_clone_value(&next));
            match self.bits.compare_exchange_weak(
                bits,
                published,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    T::jet_atomic_drop_bits(bits);
                    T::jet_atomic_drop_value(next);
                    T::jet_atomic_drop_value(delta);
                    return current;
                }
                Err(_) => {
                    T::jet_atomic_drop_bits(published);
                    T::jet_atomic_drop_value(current);
                    T::jet_atomic_drop_value(next);
                }
            }
        }
    }

    pub fn try_add(&self, delta: T::Wire) -> Result<T::Wire, AllocError>
    where
        T: JetAtomicFallible + JetAtomicAdd,
    {
        loop {
            let bits = self.bits.load(Ordering::SeqCst);
            let current = T::jet_atomic_clone_from_bits(bits, || self.bits.load(Ordering::SeqCst));
            let next = match T::jet_atomic_try_add(&current, &delta) {
                Ok(next) => next,
                Err(error) => {
                    T::jet_atomic_drop_value(current);
                    T::jet_atomic_drop_value(delta);
                    return Err(error);
                }
            };
            let published = T::jet_atomic_into_bits(T::jet_atomic_clone_value(&next));
            match self.bits.compare_exchange_weak(
                bits,
                published,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    T::jet_atomic_drop_bits(bits);
                    T::jet_atomic_drop_value(next);
                    T::jet_atomic_drop_value(delta);
                    return Ok(current);
                }
                Err(_) => {
                    T::jet_atomic_drop_bits(published);
                    T::jet_atomic_drop_value(current);
                    T::jet_atomic_drop_value(next);
                }
            }
        }
    }

    pub fn compare_exchange(&self, expected: T::Wire, replacement: T::Wire) -> bool {
        loop {
            let bits = self.bits.load(Ordering::SeqCst);
            let (equal, guard) = T::jet_atomic_equal_and_hold_bits(bits, &expected, || {
                self.bits.load(Ordering::SeqCst)
            });
            if !equal {
                T::jet_atomic_drop_value(expected);
                T::jet_atomic_drop_value(replacement);
                drop(guard);
                return false;
            }
            let published = T::jet_atomic_into_bits(T::jet_atomic_clone_value(&replacement));
            match self.bits.compare_exchange(bits, published, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => {
                    T::jet_atomic_drop_bits(bits);
                    T::jet_atomic_drop_value(expected);
                    T::jet_atomic_drop_value(replacement);
                    drop(guard);
                    return true;
                }
                Err(_) => {
                    T::jet_atomic_drop_bits(published);
                    drop(guard);
                }
            }
        }
    }
}

impl<T: JetAtomicValue> Drop for JetAtomic<T> {
    fn drop(&mut self) {
        T::jet_atomic_drop_bits(self.bits.load(Ordering::SeqCst));
    }
}

#[inline(always)]
pub fn jet_atomic_load<T: JetAtomicValue>(value: &JetAtomic<T>) -> T::Wire {
    value.load()
}

#[inline(always)]
pub fn jet_atomic_store<T: JetAtomicValue>(value: &JetAtomic<T>, next: T::Wire) {
    value.store(next)
}

#[inline(always)]
pub fn jet_atomic_add<T: JetAtomicAdd>(value: &JetAtomic<T>, delta: T::Wire) -> T::Wire {
    value.add(delta)
}

#[inline(always)]
pub fn jet_atomic_try_add<T: JetAtomicFallible + JetAtomicAdd>(
    value: &JetAtomic<T>,
    delta: T::Wire,
) -> Result<T::Wire, AllocError> {
    value.try_add(delta)
}

#[inline(always)]
pub fn jet_atomic_compare_exchange<T: JetAtomicValue>(
    value: &JetAtomic<T>,
    expected: T::Wire,
    replacement: T::Wire,
) -> bool {
    value.compare_exchange(expected, replacement)
}

#[inline(always)]
pub fn jet_atomic_publish<T: JetAtomicValue>(value: &JetAtomic<T>, next: T::Wire) {
    value.publish(next)
}

#[inline(always)]
pub fn jet_atomic_observe<T: JetAtomicValue>(value: &JetAtomic<T>) -> T::Wire {
    value.observe()
}
