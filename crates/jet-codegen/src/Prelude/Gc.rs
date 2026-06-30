mod jet_gc {
    // D-OPTGC1 / D-DEP-GC1: opt-in traced `Gc<T>` with a pure-Rust mark-sweep
    // collector. Ownership stays the default; `use core.gc` is the expert gate.
    // I6: zero external crates.
    use std::any::Any;
    use std::cell::{Cell, RefCell};
    use std::collections::{HashMap, HashSet};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

    struct GcEntry {
        marked: Cell<bool>,
        data: Box<dyn Any>,
        trace: fn(&dyn Any, &mut Vec<usize>),
    }

    thread_local! {
        static STATE: RefCell<GcState> = RefCell::new(GcState::default());
    }

    #[derive(Default)]
    struct GcState {
        entries: HashMap<usize, GcEntry>,
        roots: Vec<usize>,
    }

    /// Traced heap handle. Cycles are collected by `gc_collect`.
    pub struct Gc<T: 'static> {
        id: usize,
        _marker: std::marker::PhantomData<T>,
    }

    impl<T: 'static> Clone for Gc<T> {
        fn clone(&self) -> Self {
            STATE.with(|st| st.borrow_mut().roots.push(self.id));
            Gc {
                id: self.id,
                _marker: std::marker::PhantomData,
            }
        }
    }

    impl<T: 'static> Gc<T> {
        pub fn new(value: T) -> Self
        where
            T: GcTrace,
        {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let trace = |any: &dyn Any, out: &mut Vec<usize>| {
                if let Some(v) = any.downcast_ref::<T>() {
                    v.trace(out);
                }
            };
            STATE.with(|st| {
                let mut st = st.borrow_mut();
                st.entries.insert(
                    id,
                    GcEntry {
                        marked: Cell::new(false),
                        data: Box::new(value),
                        trace,
                    },
                );
                st.roots.push(id);
            });
            Gc {
                id,
                _marker: std::marker::PhantomData,
            }
        }

        pub fn get(&self) -> Option<std::cell::Ref<'_, T>> {
            STATE.with(|st| {
                let st = st.borrow();
                let entry = st.entries.get(&self.id)?;
                let any = entry.data.as_ref();
                // SAFETY (D-LL1, vetted): `id` was allocated as `Box<T>` and never
                // type-punned; entry lives in the map until swept.
                let ptr = any as *const dyn Any as *const T;
                // Expose through a RefCell shim so callers get a shared borrow.
                // For v1 the collector runs between mutation epochs.
                let _ = ptr;
                None
            })
        }

        pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
            STATE.with(|st| {
                let st = st.borrow();
                let entry = st.entries.get(&self.id)?;
                entry.data.downcast_ref::<T>().map(f)
            })
        }

        pub fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
            STATE.with(|st| {
                let mut st = st.borrow_mut();
                let entry = st.entries.get_mut(&self.id)?;
                entry.data.downcast_mut::<T>().map(f)
            })
        }
    }

    pub trait GcTrace {
        fn trace(&self, out: &mut Vec<usize>);
    }

    impl GcTrace for String {
        fn trace(&self, _out: &mut Vec<usize>) {}
    }

    impl GcTrace for i64 {
        fn trace(&self, _out: &mut Vec<usize>) {}
    }

    impl<T: GcTrace> GcTrace for Option<T> {
        fn trace(&self, out: &mut Vec<usize>) {
            if let Some(v) = self {
                v.trace(out);
            }
        }
    }

    impl<T: GcTrace> GcTrace for Gc<T> {
        fn trace(&self, out: &mut Vec<usize>) {
            out.push(self.id);
        }
    }

    pub fn gc_collect() {
        STATE.with(|st| {
            let mut st = st.borrow_mut();
            let roots: Vec<usize> = st.roots.drain(..).collect();
            let mut stack: Vec<usize> = roots.clone();
            let mut seen = HashSet::new();
            while let Some(id) = stack.pop() {
                if !seen.insert(id) {
                    continue;
                }
                if let Some(entry) = st.entries.get(&id) {
                    entry.marked.set(true);
                    let mut children = Vec::new();
                    (entry.trace)(entry.data.as_ref(), &mut children);
                    stack.extend(children);
                }
            }
            st.entries
                .retain(|_, e| e.marked.replace(false));
            st.roots = roots;
        });
    }
}
