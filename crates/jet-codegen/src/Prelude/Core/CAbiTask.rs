// Native task marshaling for the existing scheduler kernels (MathTaskMem.rs
// JetTask / JetTaskGroup, TaskGroup.rs flattening). A task body is a native
// SendFn closure box; its result travels as a NativeValue (CAbiIter.rs), so
// spawning, joining, fail-fast and cancellation stay in the Prelude kernels.
#[cfg(not(target_arch = "wasm32"))]
mod jet_c_abi_task {
    use super::*;
    use super::jet_c_abi::{guard, native, view, JetCString};
    use super::jet_c_abi_iter::{allocate, list_value, Failure, Header, Meta, NativeMeta, NativeValue};
    use crate::jet_std::{jet_task_flatten_result, jet_task_flatten_results};
    use std::ptr;
    use std::sync::Arc;

    type NativeTask = jet_std::JetTask<NativeValue>;

    /// One owned native SendFn closure box: `invoke(env, out) -> consumed
    /// mask` writes the body's result into `out`; `release(env)` drops the
    /// box. SAFETY: Sema checks every SendFn capture is sendable, and the
    /// loaded code image outlives every task it spawns.
    struct Body { env: usize, invoke: usize, release: usize, output: Arc<Meta> }
    unsafe impl Send for Body {}
    impl Body {
        fn run(self) -> NativeValue {
            let invoke: unsafe extern "C" fn(*mut u8, *mut u8) -> u64 = unsafe { std::mem::transmute(self.invoke) };
            let mut value = NativeValue::zeroed(self.output.clone());
            unsafe { invoke(self.env as *mut u8, value.slot_mut()) };
            value
        }
    }
    impl Drop for Body {
        fn drop(&mut self) {
            let release: unsafe extern "C" fn(*mut u8) = unsafe { std::mem::transmute(self.release) };
            unsafe { release(self.env as *mut u8) };
        }
    }

    /// The task-body failure `E` of a flattened `T E!` body, rendered by its
    /// native Debug helper (the meta's show slot) as generated Rust's `{:?}`.
    struct BodyError(NativeValue);
    impl std::fmt::Debug for BodyError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.0.jet_show())
        }
    }
    fn body_result(value: NativeValue) -> Result<NativeValue, BodyError> {
        match value.into_result() {
            Ok(value) => Ok(value),
            Err(Failure::Error(error)) => Err(BodyError(error)),
            Err(Failure::Absent) => unreachable!("checked task body failure carrier"),
        }
    }

    fn site(word: i64) -> usize {
        usize::try_from(native(word)).unwrap_or_else(|_| {
            super::jet_arithmetic_stop("<mir>", 0, "spawn site is negative or exceeds usize range")
        })
    }

    unsafe fn body(env: *mut u8, invoke: usize, release: usize, meta: *const NativeMeta) -> Body {
        Body { env: env as usize, invoke, release, output: Meta::copy(meta) }
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_task_spawn(spawn_site: i64, spawn_label: JetCString, env: *mut u8, invoke: usize, release: usize, meta: *const NativeMeta) -> *mut NativeTask {
        guard(|| {
            let body = body(env, invoke, release, meta);
            Box::into_raw(Box::new(NativeTask::spawn_at(site(spawn_site), view(spawn_label), move || body.run())))
        })
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_group_spawn(group: *mut jet_std::JetTaskGroup, spawn_site: i64, spawn_label: JetCString, env: *mut u8, invoke: usize, release: usize, meta: *const NativeMeta) -> *mut NativeTask {
        guard(|| {
            let body = body(env, invoke, release, meta);
            Box::into_raw(Box::new((*group).spawn_at(site(spawn_site), view(spawn_label), move || body.run())))
        })
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_task_drop(task: *mut NativeTask) {
        guard(|| { if !task.is_null() { drop(Box::from_raw(task)); } })
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_task_cancel(task: *mut NativeTask) {
        guard(|| (*task).cancel())
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_task_detach(task: *mut NativeTask) {
        guard(|| Box::from_raw(task).detach())
    }

    /// The native `Result` box of an outcome: 1 and the success payload, or 0
    /// and the owned `TaskFailure` handle (Lower.jet lower_result_box layout).
    fn outcome(result: Result<NativeValue, JetTaskFailure>, ok: &Meta) -> *mut u8 {
        let size = 8 + ((ok.payload_size().max(8) + 7) & !7);
        let raw = unsafe { allocate(size) };
        match result {
            Ok(value) => unsafe {
                ptr::write_unaligned(raw.cast::<u64>(), 1);
                value.put(raw.add(8));
            },
            Err(failure) => unsafe {
                ptr::write_unaligned(raw.cast::<u64>(), 0);
                ptr::write_unaligned(raw.add(8).cast::<*mut JetTaskFailure>(), Box::into_raw(Box::new(failure)));
            },
        }
        raw
    }

    /// `task.join()`: `flatten` when the body is a `T E!` carrier.
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_task_join(task: *mut NativeTask, flatten: u8, ok_meta: *const NativeMeta) -> *mut u8 {
        guard(|| {
            let ok = Meta::copy(ok_meta);
            let joined = Box::from_raw(task).join();
            let result = if flatten != 0 { jet_task_flatten_result(joined.map(body_result)) } else { joined };
            outcome(result, &ok)
        })
    }

    /// Takes a List of task handles, or of SendFn closure boxes to spawn first
    /// (`spawn` = 1, `body_meta` describing each body's result).
    unsafe fn tasks(list: *mut Header, spawn: u8, body_meta: *const NativeMeta, invoke: usize, release: usize) -> Vec<NativeTask> {
        let header = ptr::read_unaligned(list);
        ptr::write_unaligned(list, Header { len: 0, cap: 0, data: ptr::null_mut() });
        let mut tasks = Vec::with_capacity(header.len);
        for at in 0..header.len {
            let word = ptr::read_unaligned(header.data.add(at * 8).cast::<*mut u8>());
            if spawn != 0 {
                let body = body(word, invoke, release, body_meta);
                tasks.push(NativeTask::spawn(move || body.run()));
            } else {
                tasks.push(*Box::from_raw(word.cast::<NativeTask>()));
            }
        }
        super::jet_c_abi_iter::free(header.data, header.cap * 8);
        tasks
    }

    /// `task.all` / `task.any` / `task.race` (`kind` 0 / 1 / 2) over a List
    /// of tasks or closures. `ok_meta` describes the success payload: the
    /// List for `all`, one value otherwise.
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_task_group(kind: i64, flatten: u8, list: *mut Header, spawn: u8, body_meta: *const NativeMeta, invoke: usize, release: usize, ok_meta: *const NativeMeta) -> *mut u8 {
        guard(|| {
            let ok = Meta::copy(ok_meta);
            let tasks = tasks(list, spawn, body_meta, invoke, release);
            let result = match (kind, flatten != 0) {
                (0, false) => jet_std::jet_task_all(tasks).map(|values| list_value(values, ok.clone())),
                (0, true) => jet_task_flatten_results(jet_std::jet_task_all(tasks).map(|values| values.into_iter().map(body_result).collect()))
                    .map(|values| list_value(values, ok.clone())),
                (1, false) => jet_std::jet_task_any(tasks),
                (1, true) => jet_task_flatten_result(jet_std::jet_task_any(tasks).map(body_result)),
                (2, false) => jet_std::jet_task_race(tasks),
                (2, true) => jet_task_flatten_result(jet_std::jet_task_race(tasks).map(body_result)),
                _ => unreachable!("checked task group kind"),
            };
            outcome(result, &ok)
        })
    }

    // Channels (MathTaskMem.rs channel / JetSender / JetReceiver). A value
    // crosses as its native slot plus descriptor; the sender takes ownership.
    type NativeSender = jet_std::JetSender<NativeValue>;
    type NativeReceiver = jet_std::JetReceiver<NativeValue>;

    /// `channel<T>()` (capacity < 0) or `channel<T>(capacity:)`: writes the
    /// two owned endpoint handles through `tx` and `rx`.
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_channel(bounded: u8, capacity: i64, tx: *mut *mut NativeSender, rx: *mut *mut NativeReceiver) {
        guard(|| {
            let (sender, receiver) = if bounded != 0 { jet_std::channel_bounded(native(capacity)) } else { jet_std::channel() };
            tx.write(Box::into_raw(Box::new(sender)));
            rx.write(Box::into_raw(Box::new(receiver)));
        })
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_sender_send(tx: *mut NativeSender, slot: *const u8, meta: *const NativeMeta) {
        guard(|| (*tx).send(NativeValue::moved(slot, Meta::copy(meta))))
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_sender_close(tx: *mut NativeSender) {
        guard(|| (*tx).close())
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_sender_clone(tx: *mut NativeSender) -> *mut NativeSender {
        guard(|| Box::into_raw(Box::new((*tx).clone())))
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_sender_drop(tx: *mut NativeSender) {
        guard(|| { if !tx.is_null() { drop(Box::from_raw(tx)); } })
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_receiver_close(rx: *mut NativeReceiver) {
        guard(|| (*rx).close())
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_receiver_is_ready(rx: *mut NativeReceiver) -> u8 {
        guard(|| u8::from((*rx).is_ready()))
    }

    /// `try_receive`: the native Option box of the next value (null when empty).
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_receiver_try_receive(rx: *mut NativeReceiver, meta: *const NativeMeta) -> *mut u8 {
        let size = (Meta::copy(meta).payload_size().max(8) + 7) & !7;
        guard(|| match (*rx).try_receive() {
            Ok(value) => {
                let raw = allocate(size);
                value.put(raw);
                raw
            }
            Err(_) => ptr::null_mut(),
        })
    }

    /// `receive`: the native Result box of the next value or the owned
    /// `Closed` handle once the channel is closed and drained.
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_receiver_receive(rx: *mut NativeReceiver, ok_meta: *const NativeMeta) -> *mut u8 {
        guard(|| {
            let ok = Meta::copy(ok_meta);
            let raw = allocate(8 + ((ok.payload_size().max(8) + 7) & !7));
            match (*rx).receive() {
                Ok(value) => { ptr::write_unaligned(raw.cast::<u64>(), 1); value.put(raw.add(8)); }
                Err(closed) => {
                    ptr::write_unaligned(raw.cast::<u64>(), 0);
                    ptr::write_unaligned(raw.add(8).cast::<*mut jet_std::Closed>(), Box::into_raw(Box::new(closed)));
                }
            }
            raw
        })
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_receiver_clone(rx: *mut NativeReceiver) -> *mut NativeReceiver {
        guard(|| Box::into_raw(Box::new((*rx).clone())))
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_receiver_drop(rx: *mut NativeReceiver) {
        guard(|| { if !rx.is_null() { drop(Box::from_raw(rx)); } })
    }

    /// `loop value in rx`: a lazy iterator of received values that ends when
    /// the channel is closed and drained (CAbiIter.rs cursor kernels).
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_receiver_iter(rx: *mut NativeReceiver, by_value: u8) -> *mut JetIter<NativeValue> {
        guard(|| {
            let receiver = if by_value != 0 { *Box::from_raw(rx) } else { (*rx).clone() };
            Box::into_raw(Box::new(JetIter(Box::new(std::iter::from_fn(move || receiver.receive().ok())))))
        })
    }
}
