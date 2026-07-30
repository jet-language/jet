// D-LOCALCELL1=A (card #1201): the one local-interior-mutability runtime.
// `JetCell` is deliberately `!Send`/`!Sync`: its `Rc`, `UnsafeCell`, and
// single-threaded borrow counter have no atomic or operating-system lock cost.
//
// Guards carry one shared loan token. Mapping transfers that token; splitting
// clones it. The dynamic borrow is released only after the final derived guard
// drops, including during panic unwinding.
struct JetCellInner<T> {
    value: std::cell::UnsafeCell<T>,
    borrows: std::cell::Cell<isize>,
}

trait JetCellLoanRoot {
    fn release(&self, write: bool);
}

impl<T> JetCellLoanRoot for JetCellInner<T> {
    fn release(&self, write: bool) {
        let state = self.borrows.get();
        if write {
            debug_assert_eq!(state, -1);
            self.borrows.set(0);
        } else {
            debug_assert!(state > 0);
            self.borrows.set(state - 1);
        }
    }
}

struct JetCellLoan {
    root: std::rc::Rc<dyn JetCellLoanRoot>,
    write: bool,
}

impl Drop for JetCellLoan {
    fn drop(&mut self) {
        self.root.release(self.write);
    }
}

pub struct JetCell<T> {
    inner: std::rc::Rc<JetCellInner<T>>,
}

impl<T> Clone for JetCell<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T> std::fmt::Debug for JetCell<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Cell(..)")
    }
}

impl<T> PartialEq for JetCell<T> {
    fn eq(&self, other: &Self) -> bool {
        std::rc::Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl<T> Eq for JetCell<T> {}

impl<T> std::hash::Hash for JetCell<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&std::rc::Rc::as_ptr(&self.inner), state);
    }
}

impl<T: 'static> JetCell<T> {
    pub fn new(value: T) -> Self {
        Self {
            inner: std::rc::Rc::new(JetCellInner {
                value: std::cell::UnsafeCell::new(value),
                borrows: std::cell::Cell::new(0),
            }),
        }
    }

    pub fn get(&self) -> T
    where
        T: Clone,
    {
        self.read(Clone::clone)
    }

    pub fn set(&self, value: T) {
        self.edit(|slot| *slot = value);
    }

    pub fn replace(&self, value: T) -> T {
        self.edit(|slot| std::mem::replace(slot, value))
    }

    pub fn read<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        self.guard_read().read(f)
    }

    pub fn edit<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        self.guard_edit().edit(f)
    }

    pub fn guard_read(&self) -> JetCellReadGuard<T> {
        let state = self.inner.borrows.get();
        if state < 0 {
            panic!("Cell borrow conflict: cannot read while an edit guard is active");
        }
        self.inner
            .borrows
            .set(state.checked_add(1).expect("Cell borrow count overflow"));
        let root: std::rc::Rc<dyn JetCellLoanRoot> = self.inner.clone();
        JetCellReadGuard {
            loan: std::rc::Rc::new(JetCellLoan { root, write: false }),
            // SAFETY: the read loan was acquired above. The shared loan token
            // outlives this pointer and releases only after every derived guard.
            ptr: std::ptr::NonNull::from(unsafe { &*self.inner.value.get() }),
        }
    }

    pub fn guard_edit(&self) -> JetCellEditGuard<T> {
        if self.inner.borrows.get() != 0 {
            panic!("Cell borrow conflict: cannot edit while a guard is active");
        }
        self.inner.borrows.set(-1);
        let root: std::rc::Rc<dyn JetCellLoanRoot> = self.inner.clone();
        JetCellEditGuard {
            loan: std::rc::Rc::new(JetCellLoan { root, write: true }),
            // SAFETY: the exclusive loan was acquired above. Its token
            // outlives this pointer and forbids every competing root access.
            ptr: std::ptr::NonNull::from(unsafe { &mut *self.inner.value.get() }),
            access: std::cell::Cell::new(0),
        }
    }
}

/// Adapter boundary for the canonical optional-slot policy. AOT uses
/// `Option<T>`; the TIR evaluator implements this for its tagged value carrier.
pub trait JetCellOptionLike: Clone {
    type Value: Clone;

    fn value(&self) -> Option<&Self::Value>;
    fn store(&mut self, value: Self::Value);
}

impl<T: Clone> JetCellOptionLike for Option<T> {
    type Value = T;

    fn value(&self) -> Option<&T> {
        self.as_ref()
    }

    fn store(&mut self, value: T) {
        *self = Some(value);
    }
}

impl<T: JetCellOptionLike + 'static> JetCell<T> {
    pub fn begin_get_or_set(&self) -> JetCellGetOrSet<T> {
        let guard = self.guard_edit();
        match guard.read(|slot| slot.value().cloned()) {
            Some(value) => JetCellGetOrSet::Value(value),
            None => JetCellGetOrSet::Empty(guard),
        }
    }

    pub fn get_or_set<F>(&self, init: F) -> T::Value
    where
        F: FnOnce() -> T::Value,
    {
        self.try_get_or_set::<_, std::convert::Infallible>(|| Ok(init()))
            .expect("infallible Cell initializer")
    }

    pub fn try_get_or_set<F, E>(&self, init: F) -> Result<T::Value, E>
    where
        F: FnOnce() -> Result<T::Value, E>,
    {
        match self.begin_get_or_set() {
            JetCellGetOrSet::Value(value) => Ok(value),
            JetCellGetOrSet::Empty(guard) => {
                let value = init()?;
                guard.store_option_value(value.clone());
                Ok(value)
            }
        }
    }
}

pub enum JetCellGetOrSet<T: JetCellOptionLike + 'static> {
    Value(T::Value),
    Empty(JetCellEditGuard<T>),
}

impl<T: JetCellOptionLike + 'static> JetCellEditGuard<T> {
    /// Canonical completion of an empty optional slot. Runtime tiers may
    /// marshal the value, but they must call this policy instead of rebuilding
    /// the `Some` representation themselves.
    pub fn store_option_value(&self, value: T::Value) {
        self.edit(|slot| slot.store(value));
    }
}

pub struct JetCellReadGuard<T: ?Sized> {
    loan: std::rc::Rc<JetCellLoan>,
    ptr: std::ptr::NonNull<T>,
}

impl<T: ?Sized + 'static> JetCellReadGuard<T> {
    pub fn read<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        // SAFETY: the guard's shared loan is live for this call.
        f(unsafe { self.ptr.as_ref() })
    }

    pub fn get(&self) -> T
    where
        T: Clone + Sized,
    {
        self.read(Clone::clone)
    }

    pub fn map<U: ?Sized + 'static, F>(self, project: F) -> JetCellReadGuard<U>
    where
        F: FnOnce(&T) -> &U,
    {
        // SAFETY: the projection runs while the original shared loan is live.
        let ptr = std::ptr::NonNull::from(project(unsafe { self.ptr.as_ref() }));
        JetCellReadGuard {
            loan: self.loan,
            ptr,
        }
    }

    pub fn split<A: ?Sized + 'static, B: ?Sized + 'static, F>(
        self,
        project: F,
    ) -> (JetCellReadGuard<A>, JetCellReadGuard<B>)
    where
        F: FnOnce(&T) -> (&A, &B),
    {
        // SAFETY: both projections remain under the original shared loan.
        let (a, b) = project(unsafe { self.ptr.as_ref() });
        (
            JetCellReadGuard {
                loan: self.loan.clone(),
                ptr: std::ptr::NonNull::from(a),
            },
            JetCellReadGuard {
                loan: self.loan,
                ptr: std::ptr::NonNull::from(b),
            },
        )
    }
}

pub struct JetCellEditGuard<T: ?Sized> {
    loan: std::rc::Rc<JetCellLoan>,
    ptr: std::ptr::NonNull<T>,
    access: std::cell::Cell<isize>,
}

struct JetCellGuardAccess<'a> {
    state: &'a std::cell::Cell<isize>,
    write: bool,
}

impl Drop for JetCellGuardAccess<'_> {
    fn drop(&mut self) {
        if self.write {
            debug_assert_eq!(self.state.get(), -1);
            self.state.set(0);
        } else {
            let readers = self.state.get();
            debug_assert!(readers > 0);
            self.state.set(readers - 1);
        }
    }
}

impl<T: ?Sized + 'static> JetCellEditGuard<T> {
    pub fn read<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        let state = self.access.get();
        if state < 0 {
            panic!("Cell guard conflict: cannot read during an edit callback");
        }
        self.access
            .set(state.checked_add(1).expect("Cell guard read count overflow"));
        let _access = JetCellGuardAccess {
            state: &self.access,
            write: false,
        };
        // SAFETY: this callback access is tracked above and the root edit loan
        // remains live.
        f(unsafe { self.ptr.as_ref() })
    }

    pub fn edit<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        if self.access.get() != 0 {
            panic!("Cell guard conflict: cannot edit during another callback");
        }
        self.access.set(-1);
        let _access = JetCellGuardAccess {
            state: &self.access,
            write: true,
        };
        // SAFETY: this callback owns the only active access under the root edit
        // loan.
        f(unsafe { &mut *self.ptr.as_ptr() })
    }

    pub fn get(&self) -> T
    where
        T: Clone + Sized,
    {
        self.read(Clone::clone)
    }

    pub fn set(&self, value: T)
    where
        T: Sized,
    {
        self.edit(|slot| *slot = value);
    }

    pub fn map<U: ?Sized + 'static, F>(self, project: F) -> JetCellEditGuard<U>
    where
        F: FnOnce(&mut T) -> &mut U,
    {
        // SAFETY: the original edit loan is live and transferred to the result.
        let ptr = std::ptr::NonNull::from(project(unsafe { &mut *self.ptr.as_ptr() }));
        JetCellEditGuard {
            loan: self.loan,
            ptr,
            access: std::cell::Cell::new(0),
        }
    }

    pub fn split<A: ?Sized + 'static, B: ?Sized + 'static, F>(
        self,
        project: F,
    ) -> (JetCellEditGuard<A>, JetCellEditGuard<B>)
    where
        F: FnOnce(&mut T) -> (&mut A, &mut B),
    {
        // SAFETY: sema proves the two projected paths disjoint. Both derived
        // guards keep the original exclusive loan live.
        let (a, b) = project(unsafe { &mut *self.ptr.as_ptr() });
        (
            JetCellEditGuard {
                loan: self.loan.clone(),
                ptr: std::ptr::NonNull::from(a),
                access: std::cell::Cell::new(0),
            },
            JetCellEditGuard {
                loan: self.loan,
                ptr: std::ptr::NonNull::from(b),
                access: std::cell::Cell::new(0),
            },
        )
    }
}
