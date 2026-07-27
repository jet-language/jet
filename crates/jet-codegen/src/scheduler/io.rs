use super::{jet_scheduler_fatal, jet_scheduler_yield, ParkSlot};
#[cfg(all(target_os = "windows", feature = "jet_native_io"))]
use super::{
    METRIC_IO_ACTIVE, METRIC_IO_ALLOCATED, METRIC_IO_FAILURES, METRIC_IO_PORT_CLOSED,
    METRIC_IO_RETIRED, METRIC_IO_STALE,
};
use std::collections::{HashMap, HashSet};
use std::net::TcpStream;
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(target_os = "windows")]
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::Duration;
#[cfg(all(target_os = "windows", feature = "jet_native_io"))]
use std::time::Instant;

// ── M2: IO poll substrate (native epoll on Linux; portable fallback elsewhere) ─

struct IOInterest {
    stream_id: usize,
    slot: Arc<ParkSlot>,
    readable: bool,
    writable: bool,
}

#[allow(dead_code)]
#[derive(Clone)]
enum IOBackendState {
    Starting,
    Running,
    Failed(&'static str),
    Closed,
}

struct IOPoller {
    interests: Mutex<Vec<IOInterest>>,
    streams: Mutex<HashMap<usize, Arc<Mutex<TcpStream>>>>,
    retire_requested: Mutex<HashSet<usize>>,
    backend_state: Mutex<IOBackendState>,
    notify: Condvar,
    next_key: AtomicUsize,
    #[cfg(target_os = "windows")]
    iocp_port: AtomicUsize,
    #[cfg(target_os = "windows")]
    iocp_shutdown_done: AtomicBool,
}

// jet:scheduler-native-begin — vetted std-only OS FFI and poller dispatch.
#[allow(dead_code)]
impl IOPoller {
    fn register(
        self: &Arc<Self>,
        stream: Arc<Mutex<TcpStream>>,
        readable: bool,
        writable: bool,
    ) -> Result<(usize, Arc<ParkSlot>), &'static str> {
        let state = self.backend_state.lock().unwrap();
        if let IOBackendState::Failed(error) = &*state {
            return Err(*error);
        }
        if matches!(&*state, IOBackendState::Closed) {
            return Err("scheduler IO backend is closed");
        }
        let slot = ParkSlot::new();
        let mut streams = self.streams.lock().unwrap();
        let id = self.next_key.fetch_add(1, Ordering::Relaxed);
        streams.insert(id, stream);
        drop(streams);
        self.interests.lock().unwrap().push(IOInterest {
            stream_id: id,
            slot: slot.clone(),
            readable,
            writable,
        });
        self.notify.notify_one();
        #[cfg(target_os = "windows")]
        self.iocp_notify();
        drop(state);
        Ok((id, slot))
    }

    fn unregister(&self, id: usize) {
        self.interests.lock().unwrap().retain(|i| i.stream_id != id);
        self.retire_requested.lock().unwrap().insert(id);
        #[cfg(not(target_os = "windows"))]
        {
            self.streams.lock().unwrap().remove(&id);
            self.retire_requested.lock().unwrap().remove(&id);
        }
        #[cfg(target_os = "windows")]
        self.iocp_notify();
    }

    #[cfg(target_os = "windows")]
    fn iocp_notify(&self) {
        #[link(name = "kernel32")]
        extern "system" {
            fn PostQueuedCompletionStatus(port: usize, bytes: u32, key: usize, ov: *mut std::ffi::c_void) -> i32;
        }
        let port = self.iocp_port.load(Ordering::Acquire);
        if port != 0 {
            unsafe { PostQueuedCompletionStatus(port, 0, 0, std::ptr::null_mut()); }
        }
    }

    fn run(self: Arc<Self>) {
        #[cfg(all(target_os = "linux", feature = "jet_native_io"))]
        {
            self.run_linux_epoll();
        }
        #[cfg(all(
            any(
                target_os = "macos",
                target_os = "ios",
                target_os = "tvos",
                target_os = "watchos",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "openbsd"
            ),
            feature = "jet_native_io"
        ))]
        {
            self.run_kqueue();
        }
        #[cfg(all(target_os = "windows", feature = "jet_native_io"))]
        {
            self.run_iocp();
        }
        #[cfg(not(any(
            all(target_os = "linux", feature = "jet_native_io"),
            all(
                any(
                    target_os = "macos",
                    target_os = "ios",
                    target_os = "tvos",
                    target_os = "watchos",
                    target_os = "freebsd",
                    target_os = "netbsd",
                    target_os = "openbsd"
                ),
                feature = "jet_native_io"
            ),
            all(target_os = "windows", feature = "jet_native_io")
        )))]
        {
            self.run_portable_poll();
        }
    }

    #[cfg(all(target_os = "linux", feature = "jet_native_io"))]
    fn run_linux_epoll(self: Arc<Self>) {
        use std::collections::HashMap;
        use std::os::unix::io::AsRawFd;

        #[repr(C)]
        #[derive(Clone, Copy)]
        struct EpollEvent {
            events: u32,
            data: u64,
        }

        const EPOLLIN: u32 = 1;
        const EPOLLOUT: u32 = 4;
        const EPOLL_CTL_ADD: i32 = 1;
        const EPOLL_CTL_DEL: i32 = 2;

        #[link(name = "c")]
        extern "C" {
            fn epoll_create1(flags: i32) -> i32;
            fn epoll_ctl(epfd: i32, op: i32, fd: i32, event: *const EpollEvent) -> i32;
            fn epoll_wait(
                epfd: i32,
                events: *mut EpollEvent,
                maxevents: i32,
                timeout: i32,
            ) -> i32;
        }

        let epfd = unsafe { epoll_create1(0x80000) }; // CLOEXEC
        assert!(epfd >= 0, "epoll_create1 failed");
        let mut fd_slots: HashMap<i32, Arc<ParkSlot>> = HashMap::new();

        loop {
            // Register new interests with epoll.
            let pending: Vec<(i32, u32, Arc<ParkSlot>)> = {
                let interests = self.interests.lock().unwrap();
                let streams = self.streams.lock().unwrap();
                let mut out = Vec::new();
                for interest in interests.iter() {
                    let Some(stream) = streams.get(&interest.stream_id) else {
                        continue;
                    };
                    let fd = stream.lock().unwrap().as_raw_fd();
                    if fd_slots.contains_key(&fd) {
                        continue;
                    }
                    let mut events = 0u32;
                    if interest.readable {
                        events |= EPOLLIN;
                    }
                    if interest.writable {
                        events |= EPOLLOUT;
                    }
                    out.push((fd, events, interest.slot.clone()));
                }
                out
            };
            for (fd, events, slot) in pending {
                let ev = EpollEvent {
                    events,
                    data: fd as u64,
                };
                let rc = unsafe { epoll_ctl(epfd, EPOLL_CTL_ADD, fd, &ev) };
                if rc == 0 {
                    fd_slots.insert(fd, slot);
                }
            }

            let mut events = [EpollEvent {
                events: 0,
                data: 0,
            }; 64];
            let n = unsafe { epoll_wait(epfd, events.as_mut_ptr(), 64, 50) };
            if n > 0 {
                super::METRIC_POLLER_WAKE.fetch_add(n as usize, Ordering::Relaxed);
                for ev in &events[..n as usize] {
                    let fd = ev.data as i32;
                    if let Some(slot) = fd_slots.remove(&fd) {
                        let _ = unsafe { epoll_ctl(epfd, EPOLL_CTL_DEL, fd, std::ptr::null()) };
                        let mut interests = self.interests.lock().unwrap();
                        interests.retain(|i| !Arc::ptr_eq(&i.slot, &slot));
                        slot.wake();
                    }
                }
            }
        }
    }

    #[cfg(not(all(target_os = "linux", feature = "jet_native_io")))]
    fn run_linux_epoll(self: Arc<Self>) {
        self.run_portable_poll();
    }

    #[cfg(all(
        any(
            target_os = "macos",
            target_os = "ios",
            target_os = "tvos",
            target_os = "watchos",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        ),
        feature = "jet_native_io"
    ))]
    fn run_kqueue(self: Arc<Self>) {
        use std::collections::HashMap;
        use std::os::unix::io::AsRawFd;

        #[repr(C)]
        #[derive(Clone, Copy)]
        struct Kevent {
            ident: usize,
            filter: i16,
            flags: u16,
            fflags: u32,
            data: i64,
            udata: *mut std::ffi::c_void,
        }

        const EVFILT_READ: i16 = -1;
        const EVFILT_WRITE: i16 = -2;
        const EV_ADD: u16 = 0x0001;
        const EV_DELETE: u16 = 0x0002;
        const EV_ONESHOT: u16 = 0x0010;

        #[link(name = "c")]
        extern "C" {
            fn kqueue() -> i32;
            fn kevent(
                kq: i32,
                changelist: *const Kevent,
                nchanges: i32,
                eventlist: *mut Kevent,
                nevents: i32,
                timeout: *const libc_timespec,
            ) -> i32;
        }

        #[repr(C)]
        struct libc_timespec {
            tv_sec: i64,
            tv_nsec: i64,
        }

        let kq = unsafe { kqueue() };
        assert!(kq >= 0, "kqueue() failed");
        let mut fd_slots: HashMap<i32, Arc<ParkSlot>> = HashMap::new();

        loop {
            let pending: Vec<(i32, i16, Arc<ParkSlot>)> = {
                let interests = self.interests.lock().unwrap();
                let streams = self.streams.lock().unwrap();
                let mut out = Vec::new();
                for interest in interests.iter() {
                    let Some(stream) = streams.get(&interest.stream_id) else {
                        continue;
                    };
                    let fd = stream.lock().unwrap().as_raw_fd();
                    if fd_slots.contains_key(&fd) {
                        continue;
                    }
                    if interest.readable {
                        out.push((fd, EVFILT_READ, interest.slot.clone()));
                    }
                    if interest.writable {
                        out.push((fd, EVFILT_WRITE, interest.slot.clone()));
                    }
                }
                out
            };
            for (fd, filter, slot) in pending {
                let ev = Kevent {
                    ident: fd as usize,
                    filter,
                    flags: EV_ADD | EV_ONESHOT,
                    fflags: 0,
                    data: 0,
                    udata: std::ptr::null_mut(),
                };
                let rc = unsafe { kevent(kq, &ev, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
                if rc == 0 {
                    fd_slots.insert(fd, slot);
                }
            }

            let mut events = [Kevent {
                ident: 0,
                filter: 0,
                flags: 0,
                fflags: 0,
                data: 0,
                udata: std::ptr::null_mut(),
            }; 64];
            let timeout = libc_timespec {
                tv_sec: 0,
                tv_nsec: 50_000_000,
            };
            let n = unsafe {
                kevent(
                    kq,
                    std::ptr::null(),
                    0,
                    events.as_mut_ptr(),
                    64,
                    &timeout,
                )
            };
            if n > 0 {
                super::METRIC_POLLER_WAKE.fetch_add(n as usize, Ordering::Relaxed);
                for ev in &events[..n as usize] {
                    let fd = ev.ident as i32;
                    if let Some(slot) = fd_slots.remove(&fd) {
                        let del = Kevent {
                            ident: ev.ident,
                            filter: ev.filter,
                            flags: EV_DELETE,
                            fflags: 0,
                            data: 0,
                            udata: std::ptr::null_mut(),
                        };
                        let _ = unsafe { kevent(kq, &del, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
                        let mut interests = self.interests.lock().unwrap();
                        interests.retain(|i| !Arc::ptr_eq(&i.slot, &slot));
                        slot.wake();
                    }
                }
            }
        }
    }

    #[cfg(not(all(
        any(
            target_os = "macos",
            target_os = "ios",
            target_os = "tvos",
            target_os = "watchos",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        ),
        feature = "jet_native_io"
    )))]
    fn run_kqueue(self: Arc<Self>) {
        self.run_portable_poll();
    }

    #[cfg(all(target_os = "windows", feature = "jet_native_io"))]
    fn run_iocp(self: Arc<Self>) {
        use std::collections::HashMap;
        use std::os::windows::io::AsRawSocket;
        #[repr(C)]
        struct Overlapped { internal: usize, internal_high: usize, offset: u32, offset_high: u32, event: usize }
        #[repr(C)]
        struct WsaBuf { len: u32, buf: *mut u8 }
        struct Active {
            _stream: Arc<Mutex<TcpStream>>,
            socket: usize,
            operations: Vec<*mut Overlapped>,
            cancel_requested: bool,
        }
        const INVALID_HANDLE_VALUE: usize = usize::MAX;
        const WSA_IO_PENDING: i32 = 997;
        #[link(name = "kernel32")]
        extern "system" {
            fn CreateIoCompletionPort(file: usize, existing: usize, key: usize, threads: u32) -> usize;
            fn GetQueuedCompletionStatus(port: usize, bytes: *mut u32, key: *mut usize, ov: *mut *mut Overlapped, timeout_ms: u32) -> i32;
            fn CancelIoEx(file: usize, ov: *mut Overlapped) -> i32;
            fn GetLastError() -> u32;
            fn CloseHandle(handle: usize) -> i32;
        }
        #[link(name = "ws2_32")]
        extern "system" {
            fn WSARecv(socket: usize, buffers: *mut WsaBuf, count: u32, bytes: *mut u32, flags: *mut u32, ov: *mut Overlapped, completion: *mut std::ffi::c_void) -> i32;
            fn WSASend(socket: usize, buffers: *mut WsaBuf, count: u32, bytes: *mut u32, flags: u32, ov: *mut Overlapped, completion: *mut std::ffi::c_void) -> i32;
            fn WSAGetLastError() -> i32;
        }

        let port = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, 0, 0, 1) };
        if port == 0 {
            METRIC_IO_FAILURES.fetch_add(1, Ordering::Relaxed);
            *self.backend_state.lock().unwrap() =
                IOBackendState::Failed("internal scheduler IOCP creation failed");
            for interest in self.interests.lock().unwrap().drain(..) { interest.slot.wake(); }
            self.streams.lock().unwrap().clear();
            self.retire_requested.lock().unwrap().clear();
            self.iocp_shutdown_done.store(true, Ordering::Release);
            return;
        }
        self.iocp_port.store(port, Ordering::Release);
        self.iocp_shutdown_done.store(false, Ordering::Release);
        *self.backend_state.lock().unwrap() = IOBackendState::Running;
        let mut active: HashMap<usize, Active> = HashMap::new();
        loop {
            let pending: Vec<(usize, Arc<Mutex<TcpStream>>, usize, bool, bool)> = {
                let interests = self.interests.lock().unwrap();
                let streams = self.streams.lock().unwrap();
                interests.iter()
                    .filter(|interest| !active.contains_key(&interest.stream_id))
                    .filter_map(|interest| streams.get(&interest.stream_id).map(|stream| (
                        interest.stream_id,
                        stream.clone(),
                        stream.lock().unwrap().as_raw_socket() as usize,
                        interest.readable,
                        interest.writable,
                    )))
                    .collect()
            };
            for (id, stream, socket, readable, writable) in pending {
                let associated = unsafe { CreateIoCompletionPort(socket, port, id + 1, 0) };
                if associated == 0 {
                    let _error = unsafe { GetLastError() };
                    METRIC_IO_FAILURES.fetch_add(1, Ordering::Relaxed);
                    if let Some(slot) = self.interests.lock().unwrap().iter()
                        .find(|interest| interest.stream_id == id).map(|interest| interest.slot.clone())
                    { slot.wake(); }
                    self.interests.lock().unwrap().retain(|interest| interest.stream_id != id);
                    self.streams.lock().unwrap().remove(&id);
                    continue;
                }
                let mut operations = Vec::new();
                for read in [true, false] {
                    if (read && !readable) || (!read && !writable) { continue; }
                    let raw = Box::into_raw(Box::new(Overlapped {
                        internal: 0, internal_high: 0, offset: 0, offset_high: 0, event: 0,
                    }));
                    METRIC_IO_ALLOCATED.fetch_add(1, Ordering::Relaxed);
                    let mut buffer = WsaBuf { len: 0, buf: std::ptr::null_mut() };
                    let mut bytes = 0;
                    let rc = if read {
                        let mut flags = 0;
                        unsafe { WSARecv(socket, &mut buffer, 1, &mut bytes, &mut flags, raw, std::ptr::null_mut()) }
                    } else {
                        unsafe { WSASend(socket, &mut buffer, 1, &mut bytes, 0, raw, std::ptr::null_mut()) }
                    };
                    if rc == 0 || unsafe { WSAGetLastError() } == WSA_IO_PENDING {
                        operations.push(raw);
                    } else {
                        unsafe { drop(Box::from_raw(raw)); }
                        METRIC_IO_RETIRED.fetch_add(1, Ordering::Relaxed);
                    }
                }
                if operations.is_empty() {
                    if let Some(slot) = self.interests.lock().unwrap().iter()
                        .find(|interest| interest.stream_id == id).map(|interest| interest.slot.clone())
                    { slot.wake(); }
                } else {
                    active.insert(id, Active { _stream: stream, socket, operations, cancel_requested: false });
                    METRIC_IO_ACTIVE.fetch_add(1, Ordering::Relaxed);
                }
            }

            let retiring: Vec<usize> = self.retire_requested.lock().unwrap().iter().copied().collect();
            for id in retiring {
                if let Some(entry) = active.get_mut(&id) {
                    if !entry.cancel_requested {
                        entry.cancel_requested = true;
                        for operation in &entry.operations { unsafe { CancelIoEx(entry.socket, *operation); } }
                    }
                } else {
                    self.streams.lock().unwrap().remove(&id);
                    self.retire_requested.lock().unwrap().remove(&id);
                }
            }

            for (id, entry) in active.iter_mut() {
                if !self.interests.lock().unwrap().iter().any(|interest| interest.stream_id == *id)
                    && !entry.cancel_requested {
                    entry.cancel_requested = true;
                    for operation in &entry.operations { unsafe { CancelIoEx(entry.socket, *operation); } }
                }
            }

            let (mut bytes, mut key, mut operation) = (0, 0usize, std::ptr::null_mut());
            #[cfg(test)]
            let inject_fatal = TEST_IOCP_GQCS_FATAL.swap(false, Ordering::AcqRel);
            #[cfg(not(test))]
            let inject_fatal = false;
            let ok = if inject_fatal {
                0
            } else {
                unsafe { GetQueuedCompletionStatus(port, &mut bytes, &mut key, &mut operation, u32::MAX) }
            };
            if operation.is_null() {
                if ok == 0 {
                    METRIC_IO_FAILURES.fetch_add(1, Ordering::Relaxed);
                    *self.backend_state.lock().unwrap() =
                        IOBackendState::Failed("internal scheduler IOCP completion port failed");
                    for interest in self.interests.lock().unwrap().drain(..) { interest.slot.wake(); }
                    for entry in active.values_mut() {
                        if !entry.cancel_requested {
                            entry.cancel_requested = true;
                            for pending in &entry.operations {
                                let cancelled = unsafe { CancelIoEx(entry.socket, *pending) };
                                if cancelled == 0 {
                                    let error = unsafe { GetLastError() };
                                    if error != 1168 { // ERROR_NOT_FOUND: completion already queued.
                                        METRIC_IO_FAILURES.fetch_add(1, Ordering::Relaxed);
                                    }
                                }
                            }
                        }
                    }
                    // CancelIoEx on an IOCP-associated socket queues one terminal
                    // completion per outstanding OVERLAPPED. Keep every Active
                    // socket owner and Box alive until those completions arrive.
                    let drain_deadline = Instant::now() + Duration::from_secs(5);
                    while !active.is_empty() && Instant::now() < drain_deadline {
                        let (mut drain_bytes, mut drain_key, mut drain_operation) =
                            (0, 0usize, std::ptr::null_mut());
                        unsafe {
                            GetQueuedCompletionStatus(
                                port,
                                &mut drain_bytes,
                                &mut drain_key,
                                &mut drain_operation,
                                50,
                            );
                        }
                        if drain_operation.is_null() { continue; }
                        unsafe { drop(Box::from_raw(drain_operation)); }
                        METRIC_IO_RETIRED.fetch_add(1, Ordering::Relaxed);
                        let owner = active.iter().find_map(|(id, entry)|
                            entry.operations.contains(&drain_operation).then_some(*id));
                        if let Some(id) = owner {
                            let entry = active.get_mut(&id).unwrap();
                            entry.operations.retain(|candidate| *candidate != drain_operation);
                            if entry.operations.is_empty() {
                                active.remove(&id);
                                METRIC_IO_ACTIVE.fetch_sub(1, Ordering::Relaxed);
                                self.streams.lock().unwrap().remove(&id);
                                self.retire_requested.lock().unwrap().remove(&id);
                            }
                        } else {
                            METRIC_IO_STALE.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    self.iocp_port.store(0, Ordering::Release);
                    *self.backend_state.lock().unwrap() = IOBackendState::Closed;
                    if unsafe { CloseHandle(port) } != 0 {
                        METRIC_IO_PORT_CLOSED.fetch_add(1, Ordering::Relaxed);
                    } else {
                        METRIC_IO_FAILURES.fetch_add(1, Ordering::Relaxed);
                    }
                    if active.is_empty() {
                        self.streams.lock().unwrap().clear();
                        self.retire_requested.lock().unwrap().clear();
                    } else {
                        // Kernel did not return cancellation completions before
                        // bounded shutdown. Retain sockets and OVERLAPPED boxes
                        // permanently: truthful nonzero counters beat UAF.
                        std::mem::forget(active);
                    }
                    *self.backend_state.lock().unwrap() =
                        IOBackendState::Failed("internal scheduler IOCP completion port failed");
                    self.iocp_shutdown_done.store(true, Ordering::Release);
                    return;
                }
                if key != 0 { METRIC_IO_STALE.fetch_add(1, Ordering::Relaxed); }
                continue;
            }
            if ok == 0 && unsafe { GetLastError() } != 995 {
                METRIC_IO_FAILURES.fetch_add(1, Ordering::Relaxed);
            }
            unsafe { drop(Box::from_raw(operation)); }
            METRIC_IO_RETIRED.fetch_add(1, Ordering::Relaxed);
            let id = key.saturating_sub(1);
            let Some(entry) = active.get_mut(&id) else { continue; };
            entry.operations.retain(|candidate| *candidate != operation);
            let slot = {
                let mut interests = self.interests.lock().unwrap();
                let slot = interests.iter().find(|interest| interest.stream_id == id)
                    .map(|interest| interest.slot.clone());
                interests.retain(|interest| interest.stream_id != id);
                slot
            };
            if let Some(slot) = slot {
                super::METRIC_POLLER_WAKE.fetch_add(1, Ordering::Relaxed);
                for pending in &entry.operations { unsafe { CancelIoEx(entry.socket, *pending); } }
                slot.wake();
            }
            if entry.operations.is_empty() {
                active.remove(&id);
                METRIC_IO_ACTIVE.fetch_sub(1, Ordering::Relaxed);
                self.streams.lock().unwrap().remove(&id);
                self.retire_requested.lock().unwrap().remove(&id);
            }
        }
    }
    // jet:scheduler-native-end

    #[cfg(not(all(target_os = "windows", feature = "jet_native_io")))]
    fn run_iocp(self: Arc<Self>) {
        self.run_portable_poll();
    }

    #[allow(dead_code)]
    fn run_portable_poll(self: Arc<Self>) {
        use std::io::Write;
        loop {
            let ready: Vec<Arc<ParkSlot>> = {
                let interests = self.interests.lock().unwrap();
                let streams = self.streams.lock().unwrap();
                let mut slots = Vec::new();
                for interest in interests.iter() {
                    let Some(stream) = streams.get(&interest.stream_id) else {
                        continue;
                    };
                    let mut s = stream.lock().unwrap();
                    let _ = s.set_nonblocking(true);
                    let mut buf = [0u8; 1];
                    if interest.readable {
                        match s.peek(&mut buf) {
                            Ok(0) | Ok(_) => slots.push(interest.slot.clone()),
                            Err(_) => {}
                        }
                    }
                    if interest.writable {
                        match s.write(&[]) {
                            Ok(_) => slots.push(interest.slot.clone()),
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                            Err(_) => slots.push(interest.slot.clone()),
                        }
                    }
                }
                slots
            };
            if !ready.is_empty() {
                let mut interests = self.interests.lock().unwrap();
                for slot in &ready {
                    interests.retain(|i| !Arc::ptr_eq(&i.slot, slot));
                    slot.wake();
                }
            }
            let g = self.interests.lock().unwrap();
            let _ = self
                .notify
                .wait_timeout(g, Duration::from_millis(5))
                .unwrap();
        }
    }
}

static IO_POLLER: OnceLock<Arc<IOPoller>> = OnceLock::new();

fn io_poller() -> Arc<IOPoller> {
    IO_POLLER
        .get_or_init(|| {
            let poller = Arc::new(IOPoller {
                interests: Mutex::new(Vec::new()),
                streams: Mutex::new(HashMap::new()),
                retire_requested: Mutex::new(HashSet::new()),
                backend_state: Mutex::new(if cfg!(target_os = "windows") {
                    IOBackendState::Starting
                } else {
                    IOBackendState::Running
                }),
                notify: Condvar::new(),
                next_key: AtomicUsize::new(0),
                #[cfg(target_os = "windows")]
                iocp_port: AtomicUsize::new(0),
                #[cfg(target_os = "windows")]
                iocp_shutdown_done: AtomicBool::new(false),
            });
            let p = poller.clone();
            thread::spawn(move || p.run());
            poller
        })
        .clone()
}

/// Park until `stream` looks readable or writable (non-blocking probe via poller).
pub fn jet_scheduler_io_wait(stream: &TcpStream, read: bool, write: bool, wait_kind: &str) {
    let shared = Arc::new(Mutex::new(stream.try_clone().expect("tcp clone")));
    let poller = io_poller();
    let (id, slot) = poller
        .register(shared, read, write)
        .unwrap_or_else(|error| jet_scheduler_fatal(error));
    struct Registration(Arc<IOPoller>, usize);
    impl Drop for Registration {
        fn drop(&mut self) { self.0.unregister(self.1); }
    }
    let _registration = Registration(poller, id);
    jet_scheduler_yield(wait_kind, &slot, None);
    if let IOBackendState::Failed(error) = &*io_poller().backend_state.lock().unwrap() {
        jet_scheduler_fatal(error);
    }
}

#[cfg(all(test, target_os = "windows"))]
static TEST_IOCP_GQCS_FATAL: AtomicBool = AtomicBool::new(false);

#[cfg(all(test, target_os = "windows", feature = "jet_native_io"))]
mod iocp_runtime_tests {
    use super::*;
    use super::super::{
        jet_scheduler_drain, jet_scheduler_io_backend, jet_scheduler_metric_io_operations,
        jet_scheduler_spawn, jet_scheduler_spawn_with_control, JetSchedulerJoin,
        JetSchedulerResult, JetTaskControl, TEST_DEADLINE_EXCEEDED,
    };
    use std::io::Write;
    use std::net::TcpListener;

    fn connected() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        (client, server)
    }

    fn wait_result<T: Send + 'static>(join: &JetSchedulerJoin<T>) -> JetSchedulerResult<T> {
        let start = Instant::now();
        loop {
            if let Some(result) = join.try_recv() { return result; }
            assert!(start.elapsed() < Duration::from_secs(10), "IOCP task timed out");
            std::thread::yield_now();
        }
    }

    #[test]
    fn iocp_wake_cancel_deadline_stale_scale_and_cleanup() {
        assert_eq!(jet_scheduler_io_backend(), "iocp");

        let (client, mut server) = connected();
        let wake = jet_scheduler_spawn(move || {
            jet_scheduler_io_wait(&client, true, false, "iocp read");
            1i64
        });
        server.write_all(b"x").unwrap();
        assert!(matches!(wait_result(&wake), JetSchedulerResult::Value(1)));

        let (client, _server) = connected();
        let control = JetTaskControl::new();
        let cancelled = jet_scheduler_spawn_with_control(
            move || {
                jet_scheduler_io_wait(&client, true, false, "iocp cancel");
                2i64
            },
            control.clone(),
        );
        while io_poller().interests.lock().unwrap().is_empty() { std::thread::yield_now(); }
        control.cancel();
        assert!(matches!(wait_result(&cancelled), JetSchedulerResult::Cancelled));

        let (client, _server) = connected();
        let deadline = jet_scheduler_spawn(move || {
            TEST_DEADLINE_EXCEEDED.with(|value| value.set(true));
            jet_scheduler_io_wait(&client, true, false, "iocp deadline");
            3i64
        });
        assert!(matches!(wait_result(&deadline), JetSchedulerResult::Panicked));

        // Never-reused keys reject stale completions. Concurrent readers prove
        // eventual completion without starvation; each retires after readiness.
        #[link(name = "kernel32")]
        extern "system" {
            fn PostQueuedCompletionStatus(port: usize, bytes: u32, key: usize, ov: *mut std::ffi::c_void) -> i32;
        }
        let stale_before = METRIC_IO_STALE.load(Ordering::Relaxed);
        let port = io_poller().iocp_port.load(Ordering::Acquire);
        assert_ne!(port, 0);
        assert_ne!(unsafe {
            PostQueuedCompletionStatus(port, 0, usize::MAX, std::ptr::null_mut())
        }, 0);
        let stale_start = Instant::now();
        while METRIC_IO_STALE.load(Ordering::Relaxed) == stale_before {
            assert!(stale_start.elapsed() < Duration::from_secs(5), "stale IOCP packet not observed");
            std::thread::yield_now();
        }
        let before = io_poller().next_key.load(Ordering::Relaxed);
        let mut joins = Vec::new();
        let mut writers = Vec::new();
        for value in 0..64i64 {
            let (client, server) = connected();
            joins.push(jet_scheduler_spawn(move || {
                jet_scheduler_io_wait(&client, true, false, "iocp scale");
                value
            }));
            writers.push(server);
        }
        for writer in &mut writers { writer.write_all(b"x").unwrap(); }
        let mut values = Vec::new();
        for join in &joins {
            match wait_result(join) {
                JetSchedulerResult::Value(value) => values.push(value),
                _ => panic!("IOCP scale task did not complete"),
            }
        }
        values.sort_unstable();
        assert_eq!(values, (0..64).collect::<Vec<_>>());
        assert!(io_poller().next_key.load(Ordering::Relaxed) >= before + 64);
        let start = Instant::now();
        while !io_poller().interests.lock().unwrap().is_empty() {
            assert!(start.elapsed() < Duration::from_secs(5), "IOCP interests leaked");
            std::thread::yield_now();
        }
        jet_scheduler_drain();
        assert!(io_poller().streams.lock().unwrap().is_empty(), "IOCP socket clones leaked");
        let (active, allocated, retired) = jet_scheduler_metric_io_operations();
        assert_eq!(active, 0, "IOCP active registrations leaked");
        assert_eq!(allocated, retired, "OVERLAPPED allocations leaked");

        // Force fatal/null GQCS while a real OVERLAPPED read is active.
        let (active_client, _active_server) = connected();
        let active_wait = jet_scheduler_spawn(move || {
            jet_scheduler_io_wait(&active_client, true, false, "fatal iocp");
            8i64
        });
        let active_start = Instant::now();
        while jet_scheduler_metric_io_operations().0 == 0 {
            assert!(active_start.elapsed() < Duration::from_secs(5), "IOCP op never activated");
            std::thread::yield_now();
        }
        let closed_before = METRIC_IO_PORT_CLOSED.load(Ordering::Relaxed);
        TEST_IOCP_GQCS_FATAL.store(true, Ordering::Release);
        io_poller().iocp_notify();
        assert!(matches!(wait_result(&active_wait), JetSchedulerResult::Panicked));
        let shutdown_start = Instant::now();
        while !io_poller().iocp_shutdown_done.load(Ordering::Acquire) {
            assert!(
                shutdown_start.elapsed() < Duration::from_secs(10),
                "IOCP terminal shutdown did not finish"
            );
            std::thread::yield_now();
        }
        jet_scheduler_drain();
        let (active, allocated, retired) = jet_scheduler_metric_io_operations();
        assert_eq!((active, allocated), (0, retired));
        assert_eq!(METRIC_IO_PORT_CLOSED.load(Ordering::Relaxed), closed_before + 1);
        assert!(matches!(*io_poller().backend_state.lock().unwrap(), IOBackendState::Failed(_)));

        // Published terminal failure rejects later waits synchronously.
        let (client, _server) = connected();
        let failed = jet_scheduler_spawn(move || {
            jet_scheduler_io_wait(&client, true, false, "failed iocp");
            9i64
        });
        assert!(matches!(wait_result(&failed), JetSchedulerResult::Panicked));
        jet_scheduler_drain();
    }
}
