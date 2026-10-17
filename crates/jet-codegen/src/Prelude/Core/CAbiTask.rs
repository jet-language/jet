// Native task marshaling for the existing scheduler kernels (MathTaskMem.rs
// JetTask / JetTaskGroup, TaskGroup.rs flattening). A task body is a native
// SendFn closure box; its result travels as a NativeValue (CAbiIter.rs), so
// spawning, joining, fail-fast and cancellation stay in the Prelude kernels.
#[cfg(not(target_arch = "wasm32"))]
mod jet_c_abi_task {
    use super::*;
    use super::jet_c_abi::{guard, native, view, JetCString};
    use super::jet_c_abi_iter::{allocate, list_value, Failure, Header, NativeValue};
    use super::jet_native_meta::{Meta, NativeMeta};
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
            let mut value = NativeValue::uninitialized(self.output.clone());
            super::jet_native_callback_body(|| unsafe {
                invoke(self.env as *mut u8, value.slot_mut())
            });
            value.initialized()
        }
    }
    impl Drop for Body {
        fn drop(&mut self) {
            let release: unsafe extern "C" fn(*mut u8) = unsafe { std::mem::transmute(self.release) };
            super::jet_c_abi::jet_rt_session_shield_enter();
            unsafe { release(self.env as *mut u8) };
            super::jet_c_abi::jet_rt_session_shield_leave();
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

    /// `JetTaskGroup.new()` / `.with_limit(limit)`: the owning group of a
    /// lexical `task.group`; dropping it joins every child (MathTaskMem.rs).
    #[no_mangle]
    pub extern "C" fn jet_rt_native_task_group_new() -> *mut jet_std::JetTaskGroup {
        guard(|| Box::into_raw(Box::new(jet_std::JetTaskGroup::new())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_native_task_group_with_limit(limit: i64) -> *mut jet_std::JetTaskGroup {
        guard(|| Box::into_raw(Box::new(jet_std::JetTaskGroup::with_limit(native(limit)))))
    }

    /// One owned native `fn()` closure box (Closures.jet layout: 0 reference
    /// count, 8 code, 16 release helper) held as a process exit handler.
    struct ExitCallback(*mut u8);
    impl ExitCallback {
        fn run(&self) {
            let code: unsafe extern "C" fn(*mut u8) = unsafe { std::mem::transmute(*(self.0.add(8) as *const usize)) };
            unsafe { code(self.0) };
        }
    }
    impl Drop for ExitCallback {
        fn drop(&mut self) {
            unsafe {
                let count = self.0 as *mut i64;
                *count -= 1;
                if *count == 0 {
                    let release: unsafe extern "C" fn(*mut u8) = std::mem::transmute(*(self.0.add(16) as *const usize));
                    release(self.0);
                }
            }
        }
    }

    /// `core.sys.atexit(callback)`: the callback runs once at the process
    /// exit boundary, as generated Rust's `jet_std_os_atexit` registers it.
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_atexit(boxed: *mut u8) {
        let callback = ExitCallback(boxed);
        guard(move || super::jet_std_os_atexit(std::rc::Rc::new(move || callback.run())))
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
    pub unsafe extern "C" fn jet_rt_native_task_pause(task: *mut NativeTask) {
        guard(|| (*task).pause())
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_task_resume(task: *mut NativeTask) {
        guard(|| (*task).resume())
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_task_detach(task: *mut NativeTask) {
        guard(|| Box::from_raw(task).detach())
    }

    /// An outcome as the route Result carrier (Routes.jet): returns 1 and
    /// writes the success payload to `out`, or returns 0 and writes the owned
    /// runtime `TaskFailure` handle to `out`. The lowering builds the native
    /// Result box and converts the failure through its record schema.
    fn outcome(result: Result<NativeValue, JetTaskFailure>, out: *mut u8) -> i64 {
        match result {
            Ok(value) => { value.put(out); 1 }
            Err(failure) => {
                unsafe { ptr::write_unaligned(out.cast::<*mut JetTaskFailure>(), Box::into_raw(Box::new(failure))) };
                0
            }
        }
    }

    /// `task.join()`: `flatten` when the body is a `T E!` carrier.
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_task_join(task: *mut NativeTask, flatten: u8, out: *mut u8) -> i64 {
        guard(|| {
            let joined = Box::from_raw(task).join();
            let result = if flatten != 0 { jet_task_flatten_result(joined.map(body_result)) } else { joined };
            outcome(result, out)
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
    pub unsafe extern "C" fn jet_rt_native_task_group(kind: i64, flatten: u8, list: *mut Header, spawn: u8, body_meta: *const NativeMeta, invoke: usize, release: usize, ok_meta: *const NativeMeta, out: *mut u8) -> i64 {
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
            outcome(result, out)
        })
    }

    /// Heterogeneous task.all: each Task carries its own result descriptor;
    /// success values move into the checked tuple's offsets in source order.
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_task_all_tuple(handles: *const *mut NativeTask, count: usize, flatten: u8, offsets: *const usize, out: *mut u8) -> i64 {
        guard(|| {
            let tasks = (0..count).map(|at| *Box::from_raw(ptr::read_unaligned(handles.add(at)))).collect();
            let joined = jet_std::jet_task_all(tasks);
            let result = if flatten != 0 {
                jet_task_flatten_results(joined.map(|values| values.into_iter().map(body_result).collect()))
            } else { joined };
            match result {
                Ok(values) => {
                    for (at, value) in values.into_iter().enumerate() {
                        value.put(out.add(ptr::read_unaligned(offsets.add(at))));
                    }
                    1
                }
                Err(failure) => outcome(Err(failure), out),
            }
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

    /// `receive`: returns 1 and writes the next value to `out`, or returns 0
    /// and writes the owned runtime `Closed` handle to `out`.
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_receiver_receive(rx: *mut NativeReceiver, out: *mut u8) -> i64 {
        guard(|| match (*rx).receive() {
            Ok(value) => { value.put(out); 1 }
            Err(closed) => {
                ptr::write_unaligned(out.cast::<*mut jet_std::Closed>(), Box::into_raw(Box::new(closed)));
                0
            }
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
    pub unsafe extern "C" fn jet_rt_native_receiver_iter(rx: *mut NativeReceiver, by_value: u8, _meta: *const NativeMeta) -> *mut JetIter<NativeValue> {
        guard(|| {
            let receiver = if by_value != 0 { *Box::from_raw(rx) } else { (*rx).clone() };
            Box::into_raw(Box::new(JetIter(Box::new(std::iter::from_fn(move || receiver.receive().ok())))))
        })
    }

    /// A normalized readiness table delegates both blocking wait and else-arm
    /// probe to the canonical channel selector. Lists transfer their owners.
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_select_wait(recvs: *mut Header, timers: *mut Header, probing: u8, arm: *mut i64, value: *mut *mut u8) {
        guard(|| {
            let receivers = ptr::read_unaligned(recvs);
            ptr::write_unaligned(recvs, Header { len: 0, cap: 0, data: ptr::null_mut() });
            let mut owned_receivers = Vec::with_capacity(receivers.len);
            for at in 0..receivers.len {
                let raw = ptr::read_unaligned(receivers.data.add(at * 8).cast::<*mut NativeReceiver>());
                owned_receivers.push(*Box::from_raw(raw));
            }
            super::jet_c_abi_iter::free(receivers.data, receivers.cap * 8);
            let durations = ptr::read_unaligned(timers);
            ptr::write_unaligned(timers, Header { len: 0, cap: 0, data: ptr::null_mut() });
            let mut owned_durations = Vec::with_capacity(durations.len);
            for at in 0..durations.len {
                owned_durations.push(jet_foundation::Numeric::JetInt::from_raw_owned(ptr::read_unaligned(durations.data.add(at * 8).cast::<i64>())));
            }
            super::jet_c_abi_iter::free(durations.data, durations.cap * 8);
            let (selected, received) = if probing != 0 {
                jet_std::jet_select_try_wait_tagged(owned_receivers, owned_durations)
            } else {
                jet_std::jet_select_wait_tagged(owned_receivers, owned_durations)
            };
            ptr::write_unaligned(arm, selected.into_raw());
            ptr::write_unaligned(value, super::jet_c_abi_iter::option(received));
        })
    }

    // Shared<T> (MathTaskMem.rs JetShared over NativeValue; its read / edit
    // kernels, which SharedRoutes.rs jet_shared_read / jet_shared_edit call). The callback is the native closure
    // `invoke(env, value_slot, out_slot)`, run under the shared lock; it
    // writes its result into the caller's `out` slot.
    type NativeShared = jet_std::JetShared<NativeValue>;

    // #29 bootstrap seam: dies at L8a-4. Cells and nodes remain pinned while
    // registered. Reset runs only after all readers/tasks of the old world
    // have quiesced; it releases the site's owner, never outstanding clones.
    struct GlobalSharedNode {
        cell: *const std::sync::atomic::AtomicI64,
        next: *mut GlobalSharedNode,
    }
    static GLOBAL_SHARED_HEAD: std::sync::atomic::AtomicPtr<GlobalSharedNode> =
        std::sync::atomic::AtomicPtr::new(ptr::null_mut());

    unsafe fn jet_rt_global_register(cell: *const std::sync::atomic::AtomicI64) {
        guard(|| {
            use std::sync::atomic::Ordering;
            // A cell publishes at most once between quiescent resets. Kept
            // nodes make re-registration O(sites), with no repeated allocation.
            let mut current = GLOBAL_SHARED_HEAD.load(Ordering::Acquire);
            while !current.is_null() {
                if (*current).cell == cell { return; }
                current = (*current).next;
            }
            let node = Box::into_raw(Box::new(GlobalSharedNode { cell, next: ptr::null_mut() }));
            let mut head = GLOBAL_SHARED_HEAD.load(Ordering::Acquire);
            loop {
                (*node).next = head;
                match GLOBAL_SHARED_HEAD.compare_exchange_weak(
                    head, node, Ordering::Release, Ordering::Acquire,
                ) {
                    Ok(_) => break,
                    Err(new_head) => head = new_head,
                }
            }
        })
    }

    // Lifetime tags are the canonical MIR order: Process=0, World=1,
    // Thread=2, Attempt=3. Only Process/World use this CAS publication path.
    // The caller transfers its candidate; the returned winner is borrowed
    // from the site's pinned owner and must be cloned before returning it.
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_shared_site_publish(
        cell: *const std::sync::atomic::AtomicI64,
        candidate: i64,
        lifetime: i64,
    ) -> i64 {
        guard(|| {
            use std::sync::atomic::Ordering;
            assert!(candidate != 0 && matches!(lifetime, 0 | 1));
            match (*cell).compare_exchange(0, candidate, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => {
                    if lifetime == 1 { jet_rt_global_register(cell); }
                    candidate
                }
                Err(winner) => {
                    jet_rt_native_shared_drop(candidate as *mut NativeShared);
                    winner
                }
            }
        })
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_global_reset_registered() {
        guard(|| {
            use std::sync::atomic::Ordering;
            let mut node = GLOBAL_SHARED_HEAD.load(Ordering::Acquire);
            while !node.is_null() {
                let handle = (*(*node).cell).swap(0, Ordering::AcqRel);
                if handle != 0 {
                    jet_rt_native_shared_drop(handle as *mut NativeShared);
                }
                node = (*node).next;
            }
        })
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_shared_new(slot: *const u8, meta: *const NativeMeta, drop_code: usize) -> *mut NativeShared {
        guard(|| {
            let meta = Meta::copy(meta);
            // Canonical Source supplies this exact typed destructor, including
            // consuming Close. Refusal leaves the caller's staged slot untouched.
            meta.require_shared_drop_code(drop_code);
            Box::into_raw(Box::new(NativeShared::new(NativeValue::moved(slot, meta))))
        })
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_shared_read(shared: *mut NativeShared, env: *mut u8, invoke: usize, out: *mut u8) {
        guard(|| {
            let invoke: unsafe extern "C" fn(*mut u8, *const u8, *mut u8) -> u64 = std::mem::transmute(invoke);
            (*shared).read(|value: &NativeValue| { invoke(env, value.slot(), out); })
        })
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_shared_edit(shared: *mut NativeShared, env: *mut u8, invoke: usize, out: *mut u8) {
        guard(|| {
            let invoke: unsafe extern "C" fn(*mut u8, *mut u8, *mut u8) -> u64 = std::mem::transmute(invoke);
            (*shared).edit(|value: &mut NativeValue| { invoke(env, value.slot_mut(), out); })
        })
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_shared_clone(shared: *mut NativeShared) -> *mut NativeShared {
        guard(|| Box::into_raw(Box::new((*shared).clone())))
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_shared_drop(shared: *mut NativeShared) {
        guard(|| { if !shared.is_null() { drop(Box::from_raw(shared)); } })
    }
}
