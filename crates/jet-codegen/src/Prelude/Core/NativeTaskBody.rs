// One thread-confined native callback boundary, shared by emitted Core and
// SchedulerHost. Session/comptime panics stay fatal; a SendFn body hands its
// intentional program panic to the scheduler only after native frames return.
thread_local! {
    static JET_NATIVE_TASK_BODY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
pub struct JetNativeTaskBodyScope(bool);
impl JetNativeTaskBodyScope {
    pub fn enter() -> Self {
        Self(JET_NATIVE_TASK_BODY.with(|active| active.replace(true)))
    }
}
impl Drop for JetNativeTaskBodyScope {
    fn drop(&mut self) { JET_NATIVE_TASK_BODY.with(|active| active.set(self.0)); }
}
pub fn jet_scheduler_native_task_body_active() -> bool {
    JET_NATIVE_TASK_BODY.with(|active| active.get())
}
pub struct JetNativeTaskPanic(pub String);
