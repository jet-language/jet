// D-CONC-SPAWN1 / D-CONC-FAIL1: synchronous web adapter for the canonical
// task Prelude. Wasm has no native thread handle, so spawn evaluates the
// closure once and retains its result until join or detach consumes it.

struct JetWebTask<T> {
    result: Option<Result<T, JetTaskFailure>>,
}

impl<T> JetWebTask<T> {
    fn join(mut self) -> Result<T, JetTaskFailure> {
        self.result
            .take()
            .expect("web task result must be present before join")
    }

    fn detach(mut self) {
        let _ = self.result.take();
    }
}

fn jet_task_spawn<T, F>(work: F) -> JetWebTask<T>
where
    F: FnOnce() -> T,
{
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
        .map_err(|_| JetTaskFailure::Panicked("task panicked".to_string()));
    JetWebTask { result: Some(result) }
}

fn jet_task_join<T>(task: JetWebTask<T>) -> Result<T, JetTaskFailure> {
    task.join()
}

fn jet_task_detach<T>(task: JetWebTask<T>) {
    task.detach();
}
