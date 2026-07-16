// Temporary compatibility adapter for the pre-#658 `core.gc` surface.
// #658 retires this source API; collector mechanics stay in jet_rt::__gc.
fn collector() -> &'static Collector {
    static COLLECTOR: std::sync::OnceLock<Collector> = std::sync::OnceLock::new();
    COLLECTOR.get_or_init(Collector::new)
}

pub struct Gc<T: std::any::Any + Send> {
    root: Root<T>,
}

impl<T: std::any::Any + Send> Clone for Gc<T> {
    fn clone(&self) -> Self {
        Self {
            root: self
                .root
                .try_clone()
                .expect("collector root must remain live while its handle exists"),
        }
    }
}

impl<T: GcTrace + Send> Gc<T> {
    pub fn new(value: T) -> Self {
        let mut edges = Vec::new();
        value.trace(&mut edges);
        let root = collector()
            .allocate(value)
            .expect("collector allocation must satisfy internal bounds");
        root.replace_edges(&edges)
            .expect("traced edges must belong to the shared collector");
        Self { root }
    }

    pub fn get(&self) -> Option<std::cell::Ref<'_, T>> {
        None
    }

    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        self.root.read(f).ok()
    }

    pub fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        let (result, edges) = self
            .root
            .edit(|value| {
                let result = f(value);
                let mut edges = Vec::new();
                value.trace(&mut edges);
                (result, edges)
            })
            .ok()?;
        self.root.replace_edges(&edges).ok()?;
        Some(result)
    }
}

pub trait GcTrace: std::any::Any {
    fn trace(&self, out: &mut Vec<ObjectId>);
}

macro_rules! no_gc_edges {
    ($($ty:ty),* $(,)?) => {$ (
        impl GcTrace for $ty {
            fn trace(&self, _out: &mut Vec<ObjectId>) {}
        }
    )* };
}

no_gc_edges!((), bool, char, i64, f64, String);

impl<T: GcTrace> GcTrace for Option<T> {
    fn trace(&self, out: &mut Vec<ObjectId>) {
        if let Some(value) = self {
            value.trace(out);
        }
    }
}

impl<T: GcTrace> GcTrace for Vec<T> {
    fn trace(&self, out: &mut Vec<ObjectId>) {
        for value in self {
            value.trace(out);
        }
    }
}

impl<T: GcTrace> GcTrace for Box<T> {
    fn trace(&self, out: &mut Vec<ObjectId>) {
        self.as_ref().trace(out);
    }
}

impl<T: GcTrace + Send> GcTrace for Gc<T> {
    fn trace(&self, out: &mut Vec<ObjectId>) {
        out.push(self.root.id());
    }
}

pub fn gc_collect() {
    let _ = collector().safepoint();
}
