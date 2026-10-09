// One thread-confined native callback boundary, shared by emitted Core and
// SchedulerHost. Native packs read the canonical Jet domain, including proven
// Task capture/completion cleanup after the machine body returns. Seed packs
// retain the real Rust callback scope below; neither path fabricates a Task.
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
    #[cfg(jet_native_pack)]
    { jet_c_abi::stop_domain() == 2 }
    #[cfg(not(jet_native_pack))]
    { JET_NATIVE_TASK_BODY.with(|active| active.get()) }
}
pub struct JetNativeTaskPanic(pub String);

// Every native callback body which belongs to a scheduler task uses this
// boundary, including #Test and property generators. Never inspect or mark
// callback output initialized until this delivered-only poll has succeeded.
pub fn jet_native_callback_body<R>(invoke: impl FnOnce() -> R) -> R {
    let _scope = JetNativeTaskBodyScope::enter();
    let value = invoke();
    if jet_scheduler_native_propagate() != 0 {
        jet_scheduler_resume_native_stop();
    }
    value
}
