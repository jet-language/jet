// D-TASKSCOPE1=A / D-TASKGROUP-PARAM1=A: one ownership algorithm backs
// emitted AOT programs and the resident JIT host. Engines supply only the
// representation-specific cancel and join adapters.
pub struct JetTaskGroupRuntime<T> {
    children: std::sync::Mutex<Vec<T>>,
}

impl<T> JetTaskGroupRuntime<T> {
    pub fn new() -> Self {
        Self {
            children: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn register(&self, child: T) {
        self.children.lock().unwrap().push(child);
    }

    pub fn close_with<C, J>(&self, mut cancel: C, mut join: J)
    where
        C: FnMut(&T),
        J: FnMut(T),
    {
        let children = std::mem::take(&mut *self.children.lock().unwrap());
        for child in &children {
            cancel(child);
        }
        for child in children {
            join(child);
        }
    }
}
