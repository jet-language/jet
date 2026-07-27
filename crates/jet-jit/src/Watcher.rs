//! `core.event` / `core.watcher` hosts (#1219).
//! Canonical watch snapshot/diff + EventScope cancel semantics (D-WATCH-SCOPE1 / D-EVENT1).

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone)]
struct WatchEvent {
    domain: String,
    kind: String,
    path: String,
    detail: String,
    pid: i64,
    port: i64,
}

type Snapshot = BTreeMap<String, (u64, i64, bool)>;

#[derive(Clone)]
enum WatchTarget {
    Files { root: String },
    Process { pid: i64 },
    Port { host: String, port: i64 },
}

struct WatchState {
    target: WatchTarget,
    snapshot: Snapshot,
    seen_ready: bool,
    active: bool,
    callbacks: Vec<WatchCallback>,
}

#[derive(Clone)]
struct WatchCallback {
    scope: i64,
    once: bool,
    fn_ptr: i64,
    caps: Vec<i64>,
    active: Rc<Cell<bool>>,
}

struct EventScopeState {
    cancelled: bool,
    subs: Vec<Rc<Cell<bool>>>,
}

struct SubscriptionState {
    active: Rc<Cell<bool>>,
}

thread_local! {
    static WATCHES: RefCell<Vec<Option<WatchState>>> = const { RefCell::new(Vec::new()) };
    static SCOPES: RefCell<Vec<Option<EventScopeState>>> = const { RefCell::new(Vec::new()) };
    static SUBS: RefCell<Vec<Option<SubscriptionState>>> = const { RefCell::new(Vec::new()) };
    static SETS: RefCell<Vec<Option<Vec<i64>>>> = const { RefCell::new(Vec::new()) };
    /// Nested block frames: EventScope handles created in each frame.
    static SCOPE_FRAMES: RefCell<Vec<Vec<i64>>> = const { RefCell::new(Vec::new()) };
    static NEXT_ID: AtomicU64 = const { AtomicU64::new(1) };
}

fn clone_str(id: i64) -> String {
    Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(id).unwrap_or_default())
}

fn alloc_str(s: String) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s))
}

fn event_record(ev: &WatchEvent) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let d = rt.heap.alloc_string(ev.domain.clone());
        let k = rt.heap.alloc_string(ev.kind.clone());
        let p = rt.heap.alloc_string(ev.path.clone());
        let det = rt.heap.alloc_string(ev.detail.clone());
        let rec = rt.heap.alloc_record(6);
        // String fields must be JetVal::String (`record_set_string`); `struct_get_str`
        // rejects Int-tagged string ids.
        let _ = rt.heap.record_set_string(rec, 0, d);
        let _ = rt.heap.record_set_string(rec, 1, k);
        let _ = rt.heap.record_set_string(rec, 2, p);
        let _ = rt.heap.record_set_string(rec, 3, det);
        let _ = rt.heap.record_set_int(rec, 4, ev.pid);
        let _ = rt.heap.record_set_int(rec, 5, ev.port);
        rec
    })
}

fn list_from_events(events: Vec<WatchEvent>) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for ev in &events {
            let d = rt.heap.alloc_string(ev.domain.clone());
            let k = rt.heap.alloc_string(ev.kind.clone());
            let p = rt.heap.alloc_string(ev.path.clone());
            let det = rt.heap.alloc_string(ev.detail.clone());
            let rec = rt.heap.alloc_record(6);
            let _ = rt.heap.record_set_string(rec, 0, d);
            let _ = rt.heap.record_set_string(rec, 1, k);
            let _ = rt.heap.record_set_string(rec, 2, p);
            let _ = rt.heap.record_set_string(rec, 3, det);
            let _ = rt.heap.record_set_int(rec, 4, ev.pid);
            let _ = rt.heap.record_set_int(rec, 5, ev.port);
            let _ = rt.heap.list_push_int(list, rec);
        }
        list
    })
}

fn result_ok(bits: u64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.results.push(super::JitResultValue { ok: true, bits });
        rt.results.len() as i64
    })
}

fn result_err(msg: &str) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let sid = rt.heap.alloc_string(msg.to_string());
        rt.results.push(super::JitResultValue {
            ok: false,
            bits: sid as u64,
        });
        rt.results.len() as i64
    })
}

fn push_watch(state: WatchState) -> i64 {
    WATCHES.with(|slot| {
        let mut v = slot.borrow_mut();
        v.push(Some(state));
        v.len() as i64
    })
}

fn push_scope(state: EventScopeState) -> i64 {
    let h = SCOPES.with(|slot| {
        let mut v = slot.borrow_mut();
        v.push(Some(state));
        v.len() as i64
    });
    SCOPE_FRAMES.with(|frames| {
        if let Some(frame) = frames.borrow_mut().last_mut() {
            frame.push(h);
        }
    });
    h
}

fn push_sub(state: SubscriptionState) -> i64 {
    SUBS.with(|slot| {
        let mut v = slot.borrow_mut();
        v.push(Some(state));
        v.len() as i64
    })
}

fn watch_snapshot(root: &str) -> Result<Snapshot, String> {
    let mut out = Snapshot::new();
    let mut stack = vec![std::path::PathBuf::from(root)];
    while let Some(path) = stack.pop() {
        let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let path_s = path.to_string_lossy().to_string();
        let is_dir = meta.is_dir();
        out.insert(path_s, (modified, meta.len() as i64, is_dir));
        if is_dir {
            for entry in std::fs::read_dir(&path).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                stack.push(entry.path());
            }
        }
    }
    Ok(out)
}

fn watch_event(kind: &str, path: &str, is_dir: bool) -> WatchEvent {
    WatchEvent {
        domain: "file".to_string(),
        kind: kind.to_string(),
        path: path.to_string(),
        detail: if is_dir { "dir" } else { "file" }.to_string(),
        pid: 0,
        port: 0,
    }
}

fn watch_diff(old: &Snapshot, new: &Snapshot) -> Vec<WatchEvent> {
    let mut out = Vec::new();
    for (path, facts) in new {
        match old.get(path) {
            None => out.push(watch_event("Created", path, facts.2)),
            Some(prev) if prev != facts => out.push(watch_event("Modified", path, facts.2)),
            _ => {}
        }
    }
    for (path, facts) in old {
        if !new.contains_key(path) {
            out.push(watch_event("Deleted", path, facts.2));
        }
    }
    out
}

fn process_alive(pid: i64) -> bool {
    if pid <= 0 {
        return false;
    }
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

fn invoke_callback(cb: &WatchCallback, ev: &WatchEvent) {
    if !cb.active.get() {
        return;
    }
    if SCOPES.with(|s| {
        s.borrow()
            .get(cb.scope.saturating_sub(1) as usize)
            .and_then(|x| x.as_ref())
            .map(|st| st.cancelled)
            .unwrap_or(true)
    }) {
        cb.active.set(false);
        return;
    }
    let event = event_record(ev);
    let ptr = cb.fn_ptr as usize as *const u8;
    unsafe {
        match cb.caps.len() {
            0 => {
                let f: extern "C" fn(i64) = std::mem::transmute(ptr);
                f(event);
            }
            1 => {
                let f: extern "C" fn(i64, i64) = std::mem::transmute(ptr);
                f(cb.caps[0], event);
            }
            2 => {
                let f: extern "C" fn(i64, i64, i64) = std::mem::transmute(ptr);
                f(cb.caps[0], cb.caps[1], event);
            }
            3 => {
                let f: extern "C" fn(i64, i64, i64, i64) = std::mem::transmute(ptr);
                f(cb.caps[0], cb.caps[1], cb.caps[2], event);
            }
            _ => {
                let f: extern "C" fn(i64, i64, i64, i64, i64) = std::mem::transmute(ptr);
                f(cb.caps[0], cb.caps[1], cb.caps[2], cb.caps[3], event);
            }
        }
    }
    if cb.once {
        cb.active.set(false);
    }
}

fn poll_watch(handle: i64) -> Vec<WatchEvent> {
    WATCHES.with(|slot| {
        let mut v = slot.borrow_mut();
        let Some(Some(state)) = v.get_mut(handle.saturating_sub(1) as usize) else {
            return Vec::new();
        };
        if !state.active {
            return Vec::new();
        }
        let events = match &state.target {
            WatchTarget::Files { root } => match watch_snapshot(root) {
                Ok(next) => {
                    let events = watch_diff(&state.snapshot, &next);
                    state.snapshot = next;
                    events
                }
                Err(e) => vec![WatchEvent {
                    domain: "file".to_string(),
                    kind: "Error".to_string(),
                    path: root.clone(),
                    detail: e,
                    pid: 0,
                    port: 0,
                }],
            },
            WatchTarget::Process { pid } => {
                let alive = process_alive(*pid);
                if state.seen_ready && !alive {
                    state.seen_ready = false;
                    vec![WatchEvent {
                        domain: "process".to_string(),
                        kind: "Exited".to_string(),
                        path: String::new(),
                        detail: "process exited".to_string(),
                        pid: *pid,
                        port: 0,
                    }]
                } else if !state.seen_ready && !alive {
                    vec![WatchEvent {
                        domain: "process".to_string(),
                        kind: "Exited".to_string(),
                        path: String::new(),
                        detail: "process is not running".to_string(),
                        pid: *pid,
                        port: 0,
                    }]
                } else {
                    Vec::new()
                }
            }
            WatchTarget::Port { host, port } => {
                let ready = std::net::TcpStream::connect((host.as_str(), *port as u16)).is_ok();
                if ready && !state.seen_ready {
                    state.seen_ready = true;
                    vec![WatchEvent {
                        domain: "port".to_string(),
                        kind: "Ready".to_string(),
                        path: String::new(),
                        detail: format!("{host}:{port}"),
                        pid: 0,
                        port: *port,
                    }]
                } else {
                    Vec::new()
                }
            }
        };
        let callbacks = state.callbacks.clone();
        drop(v);
        for ev in &events {
            for cb in &callbacks {
                invoke_callback(cb, ev);
            }
        }
        events
    })
}

extern "C" fn jet_jit_event_scope() -> i64 {
    let _ = NEXT_ID.with(|n| n.fetch_add(1, Ordering::Relaxed));
    push_scope(EventScopeState {
        cancelled: false,
        subs: Vec::new(),
    })
}

extern "C" fn jet_jit_event_scope_cancel(handle: i64) {
    SCOPES.with(|slot| {
        if let Some(Some(st)) = slot.borrow_mut().get_mut(handle.saturating_sub(1) as usize) {
            if st.cancelled {
                return;
            }
            st.cancelled = true;
            for sub in st.subs.drain(..) {
                sub.set(false);
            }
        }
    });
}

extern "C" fn jet_jit_event_scope_frame_push() {
    SCOPE_FRAMES.with(|f| f.borrow_mut().push(Vec::new()));
}

extern "C" fn jet_jit_event_scope_frame_pop() {
    let handles = SCOPE_FRAMES.with(|f| f.borrow_mut().pop()).unwrap_or_default();
    for h in handles {
        jet_jit_event_scope_cancel(h);
    }
}

extern "C" fn jet_jit_subscription_is_active(handle: i64) -> i8 {
    SUBS.with(|slot| {
        slot.borrow()
            .get(handle.saturating_sub(1) as usize)
            .and_then(|s| s.as_ref())
            .map(|s| i8::from(s.active.get()))
            .unwrap_or(0)
    })
}

extern "C" fn jet_jit_watcher_files(path: i64) -> i64 {
    let root = clone_str(path);
    match watch_snapshot(&root) {
        Ok(snapshot) => result_ok(push_watch(WatchState {
            target: WatchTarget::Files { root },
            snapshot,
            seen_ready: false,
            active: true,
            callbacks: Vec::new(),
        }) as u64),
        Err(e) => result_err(&e),
    }
}

extern "C" fn jet_jit_watcher_process_pid(pid: i64) -> i64 {
    push_watch(WatchState {
        target: WatchTarget::Process { pid },
        snapshot: Snapshot::new(),
        seen_ready: process_alive(pid),
        active: true,
        callbacks: Vec::new(),
    })
}

extern "C" fn jet_jit_watcher_port(host: i64, port: i64) -> i64 {
    push_watch(WatchState {
        target: WatchTarget::Port {
            host: clone_str(host),
            port,
        },
        snapshot: Snapshot::new(),
        seen_ready: false,
        active: true,
        callbacks: Vec::new(),
    })
}

extern "C" fn jet_jit_watcher_set() -> i64 {
    SETS.with(|slot| {
        let mut v = slot.borrow_mut();
        v.push(Some(Vec::new()));
        v.len() as i64
    })
}

extern "C" fn jet_jit_watch_poll(handle: i64) -> i64 {
    list_from_events(poll_watch(handle))
}

extern "C" fn jet_jit_watch_cancel(handle: i64) {
    WATCHES.with(|slot| {
        if let Some(Some(st)) = slot.borrow_mut().get_mut(handle.saturating_sub(1) as usize) {
            st.active = false;
        }
    });
}

extern "C" fn jet_jit_watch_is_active(handle: i64) -> i8 {
    WATCHES.with(|slot| {
        slot.borrow()
            .get(handle.saturating_sub(1) as usize)
            .and_then(|s| s.as_ref())
            .map(|s| i8::from(s.active))
            .unwrap_or(0)
    })
}

extern "C" fn jet_jit_watch_summary(handle: i64) -> i64 {
    let text = WATCHES.with(|slot| {
        slot.borrow()
            .get(handle.saturating_sub(1) as usize)
            .and_then(|s| s.as_ref())
            .map(|s| match &s.target {
                WatchTarget::Files { root } => format!("watch file {root}"),
                WatchTarget::Process { pid } => format!("watch process {pid}"),
                WatchTarget::Port { host, port } => format!("watch port {host}:{port}"),
            })
            .unwrap_or_default()
    });
    alloc_str(text)
}

extern "C" fn jet_jit_watch_on(
    watch: i64,
    scope: i64,
    fn_ptr: i64,
    n_caps: i64,
    c0: i64,
    c1: i64,
    c2: i64,
    c3: i64,
) -> i64 {
    watch_register(watch, scope, fn_ptr, n_caps, c0, c1, c2, c3, false)
}

extern "C" fn jet_jit_watch_once(
    watch: i64,
    scope: i64,
    fn_ptr: i64,
    n_caps: i64,
    c0: i64,
    c1: i64,
    c2: i64,
    c3: i64,
) -> i64 {
    watch_register(watch, scope, fn_ptr, n_caps, c0, c1, c2, c3, true)
}

fn watch_register(
    watch: i64,
    scope: i64,
    fn_ptr: i64,
    n_caps: i64,
    c0: i64,
    c1: i64,
    c2: i64,
    c3: i64,
    once: bool,
) -> i64 {
    let caps = [c0, c1, c2, c3]
        .into_iter()
        .take(n_caps.max(0) as usize)
        .collect::<Vec<_>>();
    let active = Rc::new(Cell::new(true));
    SCOPES.with(|slot| {
        if let Some(Some(st)) = slot.borrow_mut().get_mut(scope.saturating_sub(1) as usize) {
            if st.cancelled {
                active.set(false);
            } else {
                st.subs.push(active.clone());
            }
        }
    });
    WATCHES.with(|slot| {
        if let Some(Some(st)) = slot.borrow_mut().get_mut(watch.saturating_sub(1) as usize) {
            st.callbacks.push(WatchCallback {
                scope,
                once,
                fn_ptr,
                caps,
                active: active.clone(),
            });
        }
    });
    push_sub(SubscriptionState { active })
}

extern "C" fn jet_jit_watchset_add(set: i64, watch: i64) {
    SETS.with(|slot| {
        if let Some(Some(handles)) = slot.borrow_mut().get_mut(set.saturating_sub(1) as usize) {
            handles.push(watch);
        }
    });
}

extern "C" fn jet_jit_watchset_poll(set: i64) -> i64 {
    let handles = SETS.with(|slot| {
        slot.borrow()
            .get(set.saturating_sub(1) as usize)
            .and_then(|s| s.clone())
            .unwrap_or_default()
    });
    let mut events = Vec::new();
    for h in handles {
        events.extend(poll_watch(h));
    }
    list_from_events(events)
}

extern "C" fn jet_jit_watchset_summary(set: i64) -> i64 {
    let n = SETS.with(|slot| {
        slot.borrow()
            .get(set.saturating_sub(1) as usize)
            .and_then(|s| s.as_ref())
            .map(|h| h.len())
            .unwrap_or(0)
    });
    alloc_str(format!("watchset handles={n}"))
}

pub(crate) fn clear_watcher_state() {
    WATCHES.with(|s| s.borrow_mut().clear());
    SCOPES.with(|s| s.borrow_mut().clear());
    SUBS.with(|s| s.borrow_mut().clear());
    SETS.with(|s| s.borrow_mut().clear());
    SCOPE_FRAMES.with(|s| s.borrow_mut().clear());
}

pub(crate) fn register_watcher_symbols(builder: &mut JITBuilder) {
    builder.symbol("jet_jit_event_scope", jet_jit_event_scope as *const u8);
    builder.symbol(
        "jet_jit_event_scope_cancel",
        jet_jit_event_scope_cancel as *const u8,
    );
    builder.symbol(
        "jet_jit_event_scope_frame_push",
        jet_jit_event_scope_frame_push as *const u8,
    );
    builder.symbol(
        "jet_jit_event_scope_frame_pop",
        jet_jit_event_scope_frame_pop as *const u8,
    );
    builder.symbol(
        "jet_jit_subscription_is_active",
        jet_jit_subscription_is_active as *const u8,
    );
    builder.symbol("jet_jit_watcher_files", jet_jit_watcher_files as *const u8);
    builder.symbol(
        "jet_jit_watcher_process_pid",
        jet_jit_watcher_process_pid as *const u8,
    );
    builder.symbol("jet_jit_watcher_port", jet_jit_watcher_port as *const u8);
    builder.symbol("jet_jit_watcher_set", jet_jit_watcher_set as *const u8);
    builder.symbol("jet_jit_watch_poll", jet_jit_watch_poll as *const u8);
    builder.symbol("jet_jit_watch_cancel", jet_jit_watch_cancel as *const u8);
    builder.symbol("jet_jit_watch_is_active", jet_jit_watch_is_active as *const u8);
    builder.symbol("jet_jit_watch_summary", jet_jit_watch_summary as *const u8);
    builder.symbol("jet_jit_watch_on", jet_jit_watch_on as *const u8);
    builder.symbol("jet_jit_watch_once", jet_jit_watch_once as *const u8);
    builder.symbol("jet_jit_watchset_add", jet_jit_watchset_add as *const u8);
    builder.symbol("jet_jit_watchset_poll", jet_jit_watchset_poll as *const u8);
    builder.symbol(
        "jet_jit_watchset_summary",
        jet_jit_watchset_summary as *const u8,
    );
}

#[derive(Clone, Copy)]
pub(crate) struct WatcherHostFns {
    pub event_scope: FuncId,
    pub event_scope_cancel: FuncId,
    pub event_scope_frame_push: FuncId,
    pub event_scope_frame_pop: FuncId,
    pub subscription_is_active: FuncId,
    pub watcher_files: FuncId,
    pub watcher_process_pid: FuncId,
    pub watcher_port: FuncId,
    pub watcher_set: FuncId,
    pub watch_poll: FuncId,
    pub watch_cancel: FuncId,
    pub watch_is_active: FuncId,
    pub watch_summary: FuncId,
    pub watch_on: FuncId,
    pub watch_once: FuncId,
    pub watchset_add: FuncId,
    pub watchset_poll: FuncId,
    pub watchset_summary: FuncId,
}

pub(crate) fn declare_watcher_host_fns(module: &mut JITModule) -> Result<WatcherHostFns, String> {
    let cc = module.target_config().default_call_conv;
    let mut import = |name: &str, params: &[cranelift_codegen::ir::Type], ret: Option<cranelift_codegen::ir::Type>| {
        let mut sig = Signature::new(cc);
        for p in params {
            sig.params.push(AbiParam::new(*p));
        }
        if let Some(r) = ret {
            sig.returns.push(AbiParam::new(r));
        }
        module
            .declare_function(name, Linkage::Import, &sig)
            .map_err(|e| e.to_string())
    };
    Ok(WatcherHostFns {
        event_scope: import("jet_jit_event_scope", &[], Some(types::I64))?,
        event_scope_cancel: import("jet_jit_event_scope_cancel", &[types::I64], None)?,
        event_scope_frame_push: import("jet_jit_event_scope_frame_push", &[], None)?,
        event_scope_frame_pop: import("jet_jit_event_scope_frame_pop", &[], None)?,
        subscription_is_active: import("jet_jit_subscription_is_active", &[types::I64], Some(types::I8))?,
        watcher_files: import("jet_jit_watcher_files", &[types::I64], Some(types::I64))?,
        watcher_process_pid: import("jet_jit_watcher_process_pid", &[types::I64], Some(types::I64))?,
        watcher_port: import(
            "jet_jit_watcher_port",
            &[types::I64, types::I64],
            Some(types::I64),
        )?,
        watcher_set: import("jet_jit_watcher_set", &[], Some(types::I64))?,
        watch_poll: import("jet_jit_watch_poll", &[types::I64], Some(types::I64))?,
        watch_cancel: import("jet_jit_watch_cancel", &[types::I64], None)?,
        watch_is_active: import("jet_jit_watch_is_active", &[types::I64], Some(types::I8))?,
        watch_summary: import("jet_jit_watch_summary", &[types::I64], Some(types::I64))?,
        watch_on: import(
            "jet_jit_watch_on",
            &[
                types::I64, types::I64, types::I64, types::I64, types::I64, types::I64, types::I64,
                types::I64,
            ],
            Some(types::I64),
        )?,
        watch_once: import(
            "jet_jit_watch_once",
            &[
                types::I64, types::I64, types::I64, types::I64, types::I64, types::I64, types::I64,
                types::I64,
            ],
            Some(types::I64),
        )?,
        watchset_add: import("jet_jit_watchset_add", &[types::I64, types::I64], None)?,
        watchset_poll: import("jet_jit_watchset_poll", &[types::I64], Some(types::I64))?,
        watchset_summary: import("jet_jit_watchset_summary", &[types::I64], Some(types::I64))?,
    })
}
