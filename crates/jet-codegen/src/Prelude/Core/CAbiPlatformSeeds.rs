// Mechanical Windows/macOS seed leaves. Dies when each target has a Jet OS backend.
// The source owns validation, authority, defaults, environment and error policy.
#[cfg(any(test, target_os = "windows", target_os = "macos"))]
pub(crate) mod jet_platform_seed_abi {
    pub const READ: u32 = 1;
    pub const WRITE: u32 = 2;
    pub const CREATE: u32 = 4;
    pub const TRUNCATE: u32 = 8;
    pub const APPEND: u32 = 16;
    pub const EXCLUSIVE: u32 = 32;
    pub const DIRECTORY: u32 = 64;
    pub const NOFOLLOW: u32 = 128;
    // O_CLOEXEC is always mechanical, never a public source flag.
    pub fn posix_open(bits: u32, macos: bool) -> i32 {
        let access = match (bits & READ != 0, bits & (WRITE | APPEND) != 0) {
            (true, true) => 2, (false, true) => 1, _ => 0,
        };
        let flags = if macos {
            [(CREATE, 0x200), (TRUNCATE, 0x400), (APPEND, 8),
             (EXCLUSIVE, 0xa00), (DIRECTORY, 0x100000), (NOFOLLOW, 0x100)]
        } else {
            [(CREATE, 0x40), (TRUNCATE, 0x200), (APPEND, 0x400),
             (EXCLUSIVE, 0xc0), (DIRECTORY, 0x10000), (NOFOLLOW, 0x20000)]
        };
        flags.into_iter().fold(access | if macos { 0x1000000 } else { 0x80000 },
            |value, (source, native)| value | if bits & source != 0 { native } else { 0 })
    }
    pub fn macos_rename(bits: u32) -> u32 {
        (if bits & 1 != 0 { 4 } else { 0 }) | (if bits & 2 != 0 { 2 } else { 0 })
    }
    // Same ErrorKind conversion as JetStd/JSONDataTree.rs::io_error_at.
    // Ordinals are generated from the one CoreEnumPayloadRows declaration.
    #[cfg(any(test, target_os = "windows", target_os = "macos"))]
    pub fn kind(error: std::io::ErrorKind) -> u32 {
        use crate::jet_c_abi_host::*;
        use std::io::ErrorKind::*;
        (match error {
            InvalidInput | InvalidData => HOST_IO_KIND_INVALID_INPUT,
            NotFound => HOST_IO_KIND_NOT_FOUND,
            PermissionDenied => HOST_IO_KIND_PERMISSION_DENIED,
            TimedOut => HOST_IO_KIND_TIMED_OUT,
            NotConnected | BrokenPipe => HOST_IO_KIND_CLOSED,
            AlreadyExists => HOST_IO_KIND_ALREADY_EXISTS,
            Unsupported => HOST_IO_KIND_UNSUPPORTED,
            _ => HOST_IO_KIND_OTHER,
        }) as u32
    }
    #[derive(Debug)]
    pub struct HresultCause { pub operation: &'static str, pub status: i32 }
    impl std::fmt::Display for HresultCause {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{} failed with HRESULT 0x{:08x}", self.operation, self.status as u32)
        }
    }
    impl std::error::Error for HresultCause {}
    #[repr(C)]
    pub struct Error32 {
        pub domain: u32, pub stage: u32, pub code: i64,
        pub kind: u32, pub pad: u32, pub message: usize,
    }
    impl Error32 {
        pub const fn success() -> Self {
            Self { domain: 0, stage: 0, code: 0, kind: 0, pad: 0, message: 0 }
        }
    }
    pub fn error_facts(failure: &std::io::Error, domain: u32) -> (Error32, bool) {
        let mut row = Error32::success();
        row.kind = kind(failure.kind());
        let custom = if let Some(cause) = failure.get_ref().and_then(|e| e.downcast_ref::<HresultCause>()) {
            row.domain = 3; row.code = cause.status as i64; true
        } else if let Some(code) = failure.raw_os_error() {
            row.domain = domain; row.code = code as i64; false
        } else { row.domain = 4; true };
        (row, custom)
    }
    // Apple Libc gen/FreeBSD/exec.c:228-309 (f4cdf57f318d66e79383fdd1862685d9accf7c88).
    // These are Darwin errno numbers, not Linux errno numbers.
    #[derive(Debug, PartialEq, Eq)]
    pub enum AppleExecAction { Continue, Stop, Script, Stat }
    pub fn apple_exec_action(errno: i32, fallback: bool) -> AppleExecAction {
        match errno {
            2 | 20 | 62 | 63 => AppleExecAction::Continue,
            7 | 12 | 26 => AppleExecAction::Stop,
            8 if fallback => AppleExecAction::Script,
            8 => AppleExecAction::Stop,
            _ => AppleExecAction::Stat,
        }
    }
    pub fn apple_exec_finish(searched: bool, last: i32, access: bool) -> i32 {
        if access { 13 } else if searched { 2 } else { last }
    }
    #[repr(C)]
    pub struct WindowsLaunchFacts136 {
        pub appcontainer: u32, pub internet_client: u32, pub name: usize,
        pub grant_count: i64, pub grant_paths: *const *const u8,
        pub grant_access: *const u32, pub grant_inheritance: *const u32,
        pub grant_mode: *const u32, pub job: u32, pub has_cpu: u32,
        pub cpu_ms: i64, pub has_memory: u32, pub has_open: u32,
        pub memory_bytes: i64, pub open_files: i64, pub pty: u32,
        pub creation_flags: u32, pub cols: u16, pub rows: u16, pub pad: u32,
        pub job_active_processes: u32, pub job_limit_flags: u32,
        pub job_process_memory: u64, pub job_memory: u64,
    }
    #[repr(C)]
    pub struct ExecCandidates32 {
        pub count: i64, pub column: *const *const u8,
        pub script_fallback: i64, pub searched: i64,
    }
    // Valid source-prepared WTF-8 may encode unpaired Windows surrogates.
    // Do not use from_utf8_lossy: it changes filenames and environment values.
    pub fn wide_wtf8(bytes: &[u8]) -> Vec<u16> {
        let mut wide = Vec::with_capacity(bytes.len() + 1);
        let mut at = 0;
        while at < bytes.len() {
            let first = bytes[at]; at += 1;
            let (mut code, count) = match first {
                0..=0x7f => (first as u32, 0),
                0xc0..=0xdf => ((first & 31) as u32, 1),
                0xe0..=0xef => ((first & 15) as u32, 2),
                _ => ((first & 7) as u32, 3),
            };
            for _ in 0..count { code = (code << 6) | (bytes[at] & 63) as u32; at += 1; }
            if code <= 0xffff { wide.push(code as u16); }
            else { code -= 0x10000; wide.push(0xd800 | (code >> 10) as u16); wide.push(0xdc00 | (code & 1023) as u16); }
        }
        wide.push(0); wide
    }
    #[test]
    fn translations() {
        assert_eq!(posix_open(READ, false), 0x80000);
        assert_eq!(posix_open(READ, true), 0x1000000);
        assert_eq!(posix_open(WRITE | CREATE | TRUNCATE, false), 0x80241);
        assert_eq!(posix_open(WRITE | CREATE | TRUNCATE, true), 0x1000601);
        assert_eq!(posix_open(APPEND | EXCLUSIVE, true), 0x1000a09);
        assert_eq!(posix_open(READ | WRITE | DIRECTORY | NOFOLLOW, true), 0x1100102);
        assert_eq!(macos_rename(1), 4);
        assert_eq!(macos_rename(2), 2);
        assert_eq!(std::mem::size_of::<Error32>(), 32);
        assert_eq!(std::mem::offset_of!(Error32, message), 24);
        assert_eq!(std::mem::size_of::<WindowsLaunchFacts136>(), 136);
        assert_eq!(std::mem::offset_of!(WindowsLaunchFacts136, cols), 104);
        assert_eq!(std::mem::offset_of!(WindowsLaunchFacts136, job_process_memory), 120);
        assert_eq!(wide_wtf8(b"a\xed\xa0\x80\xf0\x9f\x98\x80"), [97, 0xd800, 0xd83d, 0xde00, 0]);
        assert_eq!(HresultCause { operation: "CreatePseudoConsole", status: -2147024891 }.to_string(),
            "CreatePseudoConsole failed with HRESULT 0x80070005");
    }
    #[test]
    fn error_site_classes_preserve_public_os_code() {
        let failures = [
            std::io::Error::from_raw_os_error(5),
            std::io::Error::new(std::io::ErrorKind::Other, HresultCause { operation: "CreateAppContainerProfile", status: -2147024891 }),
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "CreateProcessW(ConPTY): access denied (os error 5)"),
        ];
        for (failure, expected_domain) in failures.iter().zip([2, 3, 4]) {
            let (mut row, needs_message) = error_facts(failure, 2);
            row.message = usize::from(needs_message); // Projection witness, no text ownership is created.
            assert_eq!(row.domain, expected_domain);
            assert_eq!((row.domain == 1 || row.domain == 2) && row.message == 0, failure.raw_os_error().is_some());
            assert_eq!(row.kind, kind(failure.kind()));
        }
        let (row, _) = error_facts(&failures[1], 2);
        assert_eq!(row.code, -2147024891);
        assert_eq!(kind(std::io::ErrorKind::AlreadyExists), crate::jet_c_abi_host::HOST_IO_KIND_ALREADY_EXISTS as u32);
        assert_eq!(kind(std::io::ErrorKind::Unsupported), crate::jet_c_abi_host::HOST_IO_KIND_UNSUPPORTED as u32);
    }
    #[test]
    fn apple_exec_classes() {
        for code in [2, 20, 62, 63] { assert_eq!(apple_exec_action(code, true), AppleExecAction::Continue); }
        for code in [7, 12, 26] { assert_eq!(apple_exec_action(code, true), AppleExecAction::Stop); }
        for code in [13, 14, 5, 70, 19, 60] { assert_eq!(apple_exec_action(code, true), AppleExecAction::Stat); }
        assert_eq!(apple_exec_action(8, true), AppleExecAction::Script);
        assert_eq!(apple_exec_action(8, false), AppleExecAction::Stop);
        assert_eq!(apple_exec_finish(true, 70, false), 2);
        assert_eq!(apple_exec_finish(true, 70, true), 13);
        assert_eq!(apple_exec_finish(false, 62, false), 62);
        assert_eq!(std::mem::size_of::<ExecCandidates32>(), 32);
        assert_eq!(std::mem::offset_of!(ExecCandidates32, searched), 24);
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
pub(crate) mod jet_c_abi_platform_seeds {
    use super::jet_platform_seed_abi::*;
    use crate::jet_c_abi::{handle, view, JetCString};
    use std::fs::{File, OpenOptions};
    use std::io::{self, Read, Write, Seek, SeekFrom};
    use std::mem::ManuallyDrop;
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::ptr;
    #[cfg(windows)]
    use std::os::windows::io::{FromRawHandle, IntoRawHandle};
    #[cfg(target_os = "macos")]
    use std::os::fd::{FromRawFd, IntoRawFd};
    unsafe extern "C" {
        fn jet_rt_alloc(size: usize, align: usize) -> *mut u8;
        fn jet_rt_free(ptr: *mut u8, size: usize, align: usize);
        fn jet_rt_string_clone(value: JetCString) -> JetCString;
        fn jet_rt_string_drop(value: JetCString);
        fn jet_int_owned_to_i64(value: i64) -> i64;
        fn jet_int_owned_from_i64(value: i64) -> i64;
    }
    #[repr(C)]
    pub struct Fd72 {
        lock: AtomicI64, number: i64, closed: i64, path: JetCString,
        buffer: *mut u8, capacity: i64, filled: usize, position: usize, refs: i64,
    }
    const _: () = {
        assert!(std::mem::size_of::<Fd72>() == 72);
        assert!(std::mem::offset_of!(Fd72, lock) == 0);
        assert!(std::mem::offset_of!(Fd72, capacity) == 40);
        assert!(std::mem::offset_of!(Fd72, refs) == 64);
    };
    // SAFETY: these helpers only borrow source-owned live C carriers. A leaf
    // holds their lock for every mutable access and never borrows Rust layout.
    unsafe fn lock<'a>(state: *mut Fd72) -> &'a mut Fd72 {
        let mutex = &(*state).lock;
        while mutex.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_err() {
            std::thread::yield_now();
        }
        &mut *state
    }
    fn unlock(state: &Fd72) { state.lock.store(0, Ordering::Release); }
    unsafe fn native(word: i64) -> i64 { jet_int_owned_to_i64(word) }
    unsafe fn file(number: i64) -> ManuallyDrop<File> {
        #[cfg(windows)] let file = File::from_raw_handle(number as usize as _);
        #[cfg(target_os = "macos")] let file = File::from_raw_fd(number as i32);
        ManuallyDrop::new(file)
    }
    fn into_number(file: File) -> i64 {
        #[cfg(windows)] { file.into_raw_handle() as usize as i64 }
        #[cfg(target_os = "macos")] { file.into_raw_fd() as i64 }
    }
    unsafe fn new_fd(number: i64, path: JetCString, capacity: i64) -> *mut Fd72 {
        let state = jet_rt_alloc(72, 8).cast::<Fd72>();
        state.write(Fd72 { lock: AtomicI64::new(0), number, closed: 0,
            path: if path.is_null() { path } else { jet_rt_string_clone(path) },
            buffer: ptr::null_mut(), capacity, filled: 0, position: 0, refs: 1 });
        state
    }
    pub(crate) unsafe fn error(output: *mut Error32, stage: u32, failure: io::Error) {
        let (mut row, custom) = error_facts(&failure, if cfg!(windows) { 2 } else { 1 });
        row.stage = stage;
        if custom { row.message = handle(failure.to_string()) as usize; }
        output.write(row);
    }
    unsafe fn result<T>(output: *mut Error32, value: io::Result<T>) -> Option<T> {
        output.write(Error32::success());
        match value { Ok(value) => Some(value), Err(failure) => { error(output, 0, failure); None } }
    }
    fn closed() -> io::Error { io::Error::from_raw_os_error(if cfg!(windows) { 6 } else { 9 }) }
    fn console(state: &Fd72) -> bool { cfg!(windows) && state.capacity == i64::MIN }
    fn console_bytes() -> io::Error {
        io::Error::new(io::ErrorKind::Unsupported, "byte operations are not available on a terminal control handle")
    }
    unsafe fn close_number(state: &mut Fd72) {
        #[cfg(windows)] if console(state) { crate::jet_process_pty::close_console(state.number as usize); return; }
        drop(ManuallyDrop::into_inner(file(state.number)));
    }
    unsafe fn buffer(state: &mut Fd72) -> *mut u8 {
        if state.buffer.is_null() { state.buffer = jet_rt_alloc(8192, 1); }
        state.buffer
    }
    unsafe fn flush(state: &mut Fd72) -> io::Result<()> {
        if console(state) { return Err(console_bytes()); }
        if state.closed != 0 { return Err(closed()); }
        if state.capacity >= 0 { return Ok(()); }
        let mut file = file(state.number);
        let mut at = 0;
        let answer = loop {
            if at == state.filled { break Ok(()); }
            match file.write(std::slice::from_raw_parts(state.buffer.add(at), state.filled - at)) {
                Ok(0) => break Err(io::ErrorKind::WriteZero.into()),
                Ok(count) => at += count,
                Err(failure) if failure.kind() == io::ErrorKind::Interrupted => {},
                Err(failure) => break Err(failure),
            }
        };
        if at != 0 { ptr::copy(state.buffer.add(at), state.buffer, state.filled - at); }
        state.filled -= at;
        answer
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_open_raw(path: JetCString, flags: i64, mode: i64, out: *mut Error32) -> *mut Fd72 {
        let bits = native(flags) as u32;
        let mut options = OpenOptions::new();
        options.read(bits & READ != 0).write(bits & WRITE != 0)
            .append(bits & APPEND != 0).create(bits & CREATE != 0)
            .truncate(bits & TRUNCATE != 0).create_new(bits & EXCLUSIVE != 0);
        #[cfg(target_os = "macos")] {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(native(mode) as u32).custom_flags(posix_open(bits, true) & !3);
        }
        #[cfg(windows)] {
            use std::os::windows::fs::OpenOptionsExt;
            let _ = mode;
            options.custom_flags((if bits & DIRECTORY != 0 { 0x02000000 } else { 0 }) |
                (if bits & NOFOLLOW != 0 { 0x00200000 } else { 0 }));
        }
        result(out, options.open(view(path))).map_or(ptr::null_mut(), |file|
            new_fd(into_number(file), path, if bits & (WRITE | APPEND) != 0 { -8192 } else { 0 }))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_adopt_raw(number: i64, path: JetCString, writer: bool) -> *mut Fd72 {
        new_fd(native(number), path, if writer { -8192 } else { 0 })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_adopt_unbuffered_raw(number: i64, path: JetCString) -> *mut Fd72 {
        new_fd(native(number), path, 0)
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_number_raw(state: *mut Fd72) -> i64 {
        let state = lock(state); let number = if state.closed != 0 { -1 } else { state.number }; unlock(state); number
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_path_raw(state: *mut Fd72) -> JetCString {
        let state = lock(state);
        let path = if state.path.is_null() { state.path } else { jet_rt_string_clone(state.path) };
        unlock(state); path
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_read_raw(state: *mut Fd72, bytes: *mut u8, length: i64, out: *mut Error32) -> i64 {
        unbuffered(state, bytes, length, out, false)
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_write_raw(state: *mut Fd72, bytes: *const u8, length: i64, out: *mut Error32, zero: *mut u8) -> i64 {
        let length = native(length) as usize; let state = lock(state); zero.write(0);
        let answer = (|| {
            if console(state) { return Err(console_bytes()); }
            if state.closed != 0 { return Err(closed()); }
            if length > 8192 - state.filled { flush(state)?; }
            if length >= 8192 { return file(state.number).write(std::slice::from_raw_parts(bytes, length)); }
            let data = buffer(state); ptr::copy_nonoverlapping(bytes, data.add(state.filled), length);
            state.filled += length; Ok(length)
        })(); unlock(state);
        if answer.as_ref().is_err_and(|e| e.kind() == io::ErrorKind::WriteZero) {
            zero.write(1); out.write(Error32::success()); -1
        } else { result(out, answer).map_or(-1, |count| count as i64) }
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_flush_raw(state: *mut Fd72, out: *mut Error32, zero: *mut u8) {
        let state = lock(state); let answer = flush(state); unlock(state); zero.write(0);
        if answer.as_ref().is_err_and(|e| e.kind() == io::ErrorKind::WriteZero) {
            zero.write(1); out.write(Error32::success());
        } else { result(out, answer); }
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_close_raw(state: *mut Fd72, out: *mut Error32, zero: *mut u8) {
        let state = lock(state); zero.write(0);
        let answer = if state.closed == 0 {
            let answer = if console(state) { Ok(()) } else { flush(state) }; close_number(state);
            state.closed = 1; state.number = -1; answer
        } else { Ok(()) }; unlock(state);
        if answer.as_ref().is_err_and(|e| e.kind() == io::ErrorKind::WriteZero) {
            zero.write(1); out.write(Error32::success());
        } else { result(out, answer); }
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_retain_raw(pointer: *mut Fd72) -> *mut Fd72 {
        let state = lock(pointer); state.refs += 1; unlock(state); pointer
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_drop_raw(pointer: *mut Fd72) {
        if pointer.is_null() { return; }
        let state = lock(pointer); state.refs -= 1;
        if state.refs != 0 { unlock(state); return; }
        if state.closed == 0 { if !console(state) { let _ = flush(state); } close_number(state); }
        if !state.path.is_null() { jet_rt_string_drop(state.path); }
        if !console(state) && !state.buffer.is_null() { jet_rt_free(state.buffer, 8192, 1); }
        jet_rt_free(pointer.cast(), 72, 8);
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_dup_raw(pointer: *mut Fd72, out: *mut Error32) -> *mut Fd72 {
        let state = lock(pointer);
        let answer = if console(state) { Err(console_bytes()) } else if state.closed != 0 { Err(closed()) } else { file(state.number).try_clone() };
        let duplicate = result(out, answer).map_or(ptr::null_mut(), |file| new_fd(into_number(file), state.path, state.capacity));
        unlock(state); duplicate
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_sync_raw(pointer: *mut Fd72, data_only: bool, out: *mut Error32) {
        let state = lock(pointer);
        let answer = if console(state) { Err(console_bytes()) } else if state.closed != 0 { Err(closed()) } else if data_only { file(state.number).sync_data() } else { file(state.number).sync_all() };
        unlock(state); result(out, answer);
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_truncate_raw(pointer: *mut Fd72, length: i64, out: *mut Error32) {
        let length = native(length); let state = lock(pointer);
        let answer = if console(state) { Err(console_bytes()) } else if state.closed != 0 { Err(closed()) } else { file(state.number).set_len(length as u64) };
        unlock(state); result(out, answer);
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_seek_raw(pointer: *mut Fd72, offset: i64, whence: i64, out: *mut Error32, zero: *mut u8) -> i64 {
        let offset = native(offset); let whence = native(whence); let state = lock(pointer); zero.write(0);
        let answer = flush(state).and_then(|()| {
            let origin = match whence { 0 => SeekFrom::Start(offset as u64), 1 => SeekFrom::Current(offset), _ => SeekFrom::End(offset) };
            let position = file(state.number).seek(origin)?; state.filled = 0; state.position = 0; Ok(position)
        }); unlock(state);
        if answer.as_ref().is_err_and(|e| e.kind() == io::ErrorKind::WriteZero) {
            zero.write(1); out.write(Error32::success()); -1
        } else { result(out, answer).map_or(-1, |position| jet_int_owned_from_i64(position as i64)) }
    }
    unsafe fn unbuffered(pointer: *mut Fd72, bytes: *mut u8, length: i64, out: *mut Error32, write: bool) -> i64 {
        let length = native(length) as usize; let state = lock(pointer);
        let answer = if console(state) { Err(console_bytes()) } else if state.closed != 0 { Err(closed()) } else {
            if write {
                loop {
                    let answer = file(state.number).write(std::slice::from_raw_parts(bytes, length));
                    if !answer.as_ref().is_err_and(|e| e.kind() == io::ErrorKind::Interrupted) { break answer; }
                }
            } else { file(state.number).read(std::slice::from_raw_parts_mut(bytes, length)) }
        }; unlock(state); result(out, answer).map_or(-1, |count| count as i64)
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_write_unbuffered_raw(pointer: *mut Fd72, bytes: *mut u8, length: i64, out: *mut Error32) -> i64 { unbuffered(pointer, bytes, length, out, true) }
    fn timestamp(value: std::time::SystemTime) -> (i64, i64) {
        match value.duration_since(std::time::UNIX_EPOCH) {
            Ok(d) => (d.as_secs() as i64, d.subsec_nanos() as i64),
            Err(e) => { let d = e.duration(); if d.subsec_nanos() == 0 { (-(d.as_secs() as i64), 0) }
                else { (-(d.as_secs() as i64) - 1, 1_000_000_000 - d.subsec_nanos() as i64) } }
        }
    }
    unsafe fn stat(metadata: &std::fs::Metadata, output: *mut i64) -> io::Result<()> {
        #[cfg(target_os = "macos")] let row = {
            use std::os::unix::fs::MetadataExt;
            [metadata.dev() as i64, metadata.ino() as i64, metadata.mode() as i64,
                metadata.nlink() as i64, metadata.uid() as i64, metadata.gid() as i64,
                metadata.rdev() as i64, metadata.size() as i64,
                metadata.atime(), metadata.atime_nsec(), metadata.mtime(), metadata.mtime_nsec(),
                metadata.ctime(), metadata.ctime_nsec(), metadata.blocks() as i64, metadata.blksize() as i64]
        };
        #[cfg(windows)] let row = {
            let mut row = [0i64; 16];
            row[2] = if metadata.is_dir() { 0o40777 } else if metadata.is_symlink() { 0o120777 } else { 0o100666 };
            row[3] = 1; row[7] = metadata.len() as i64;
            let accessed = timestamp(metadata.accessed()?); row[8] = accessed.0; row[9] = accessed.1;
            let modified = timestamp(metadata.modified()?); row[10] = modified.0; row[11] = modified.1;
            row
        };
        ptr::copy_nonoverlapping(row.as_ptr(), output, 16); Ok(())
    }
    #[cfg(windows)]
    unsafe fn stat_identity(file: &File, output: *mut i64) -> io::Result<()> {
        use std::os::windows::io::AsRawHandle;
        #[repr(C)] struct Information { attributes: u32, created: [u32; 2], accessed: [u32; 2],
            written: [u32; 2], volume: u32, size_high: u32, size_low: u32, links: u32, index_high: u32, index_low: u32 }
        unsafe extern "system" { fn GetFileInformationByHandle(handle: *mut std::ffi::c_void, out: *mut Information) -> i32; }
        let mut info: Information = std::mem::zeroed();
        if GetFileInformationByHandle(file.as_raw_handle(), &mut info) == 0 { return Err(io::Error::last_os_error()); }
        output.write(info.volume as i64); output.add(1).write(((info.index_high as u64) << 32 | info.index_low as u64) as i64);
        output.add(3).write(info.links as i64); Ok(())
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_stat_raw(pointer: *mut Fd72, output: *mut i64, out: *mut Error32) {
        let state = lock(pointer);
        let answer = if console(state) { Err(console_bytes()) } else if state.closed != 0 { Err(closed()) } else {
            let file = file(state.number);
            file.metadata().and_then(|m| stat(&m, output)).and_then(|()| {
                #[cfg(windows)] { stat_identity(&file, output) }
                #[cfg(target_os = "macos")] { Ok(()) }
            })
        }; unlock(state); result(out, answer);
    }
    unsafe fn path_stat(path: &std::path::Path, follow: bool, output: *mut i64) -> io::Result<()> {
        let metadata = if follow { std::fs::metadata(path) } else { std::fs::symlink_metadata(path) }?;
        stat(&metadata, output)?;
        #[cfg(windows)] {
            use std::os::windows::fs::OpenOptionsExt;
            let file = OpenOptions::new().access_mode(0).custom_flags(0x02000000 |
                if follow { 0 } else { 0x00200000 }).open(path)?;
            stat_identity(&file, output)?;
        }
        Ok(())
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fs_stat_raw(path: JetCString, follow: bool, output: *mut i64, out: *mut Error32) {
        result(out, path_stat(std::path::Path::new(view(path)), follow, output));
    }
    unsafe fn birth(metadata: io::Result<std::fs::Metadata>, output: *mut i64, out: *mut Error32) {
        output.write(0); output.add(1).write(0); output.add(2).write(0);
        if let Some(metadata) = result(out, metadata) {
            match metadata.created() {
                Ok(time) => { let (sec, nanos) = timestamp(time); output.write(1); output.add(1).write(sec); output.add(2).write(nanos); }
                Err(e) if e.kind() == io::ErrorKind::Unsupported => {},
                Err(e) => error(out, 0, e),
            }
        }
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_created_raw(pointer: *mut Fd72, output: *mut i64, out: *mut Error32) {
        let state = lock(pointer); birth(if console(state) { Err(console_bytes()) } else if state.closed != 0 { Err(closed()) } else { file(state.number).metadata() }, output, out); unlock(state);
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fs_created_raw(path: JetCString, follow: bool, output: *mut i64, out: *mut Error32) {
        birth(if follow { std::fs::metadata(view(path)) } else { std::fs::symlink_metadata(view(path)) }, output, out);
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fs_mkdir_raw(path: JetCString, mode: i64, out: *mut Error32) {
        #[cfg(target_os = "macos")] let mut builder = std::fs::DirBuilder::new();
        #[cfg(windows)] let builder = std::fs::DirBuilder::new();
        #[cfg(target_os = "macos")] { use std::os::unix::fs::DirBuilderExt; builder.mode(native(mode) as u32); }
        #[cfg(windows)] let _ = mode;
        result(out, builder.create(view(path)));
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fs_unlink_raw(path: JetCString, directory: bool, out: *mut Error32) {
        result(out, if directory { std::fs::remove_dir(view(path)) } else { std::fs::remove_file(view(path)) });
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fs_link_raw(old: JetCString, new: JetCString, symbolic: bool, out: *mut Error32) {
        let answer = if symbolic {
            #[cfg(target_os = "macos")] { std::os::unix::fs::symlink(view(old), view(new)) }
            #[cfg(windows)] { std::os::windows::fs::symlink_file(view(old), view(new)) }
        } else { std::fs::hard_link(view(old), view(new)) }; result(out, answer);
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fs_rename_raw(old: JetCString, new: JetCString, out: *mut Error32) {
        result(out, std::fs::rename(view(old), view(new)));
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_chmod_raw(pointer: *mut Fd72, mode: i64, out: *mut Error32) {
        let mode = native(mode); let state = lock(pointer);
        let answer: io::Result<()> = if console(state) { Err(console_bytes()) } else if state.closed != 0 { Err(closed()) } else {
            #[cfg(target_os = "macos")] { use std::os::unix::fs::PermissionsExt;
                file(state.number).set_permissions(std::fs::Permissions::from_mode(mode as u32)) }
            #[cfg(windows)] { let _ = mode; Err(io::Error::new(io::ErrorKind::Unsupported, "changing Unix permission bits is not available on this operating system")) }
        }; unlock(state); result(out, answer);
    }
    #[cfg(target_os = "macos")]
    mod mac {
        use std::ffi::c_char;
        // Darwin's 64-bit struct stat; retained only as the raw stat output buffer.
        #[repr(C)]
        pub struct DarwinStat {
            dev: i32, mode: u16, nlink: u16, ino: u64, uid: u32, gid: u32, rdev: i32,
            atime: [i64; 2], mtime: [i64; 2], ctime: [i64; 2], birthtime: [i64; 2],
            size: i64, blocks: i64, blksize: i32, flags: u32, generation: u32,
            spare: i32, qspare: [i64; 2],
        }
        unsafe extern "C" {
            pub fn openat(fd: i32, path: *const c_char, flags: i32, mode: u32) -> i32;
            pub fn mkdirat(fd: i32, path: *const c_char, mode: u32) -> i32;
            pub fn unlinkat(fd: i32, path: *const c_char, flags: i32) -> i32;
            pub fn readlinkat(fd: i32, path: *const c_char, bytes: *mut u8, len: usize) -> isize;
            pub fn renameatx_np(fd: i32, old: *const c_char, into: i32, new: *const c_char, flags: u32) -> i32;
            pub fn flock(fd: i32, op: i32) -> i32;
            pub fn kill(pid: i32, signal: i32) -> i32;
            pub fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
            pub fn fchdir(fd: i32) -> i32;
            pub fn dup2(from: i32, to: i32) -> i32;
            pub fn fcntl(fd: i32, operation: i32, ...) -> i32;
            pub fn execve(path: *const u8, argv: *const *const u8, env: *const *const u8) -> i32;
            #[link_name = "__error"] pub fn errno() -> *mut i32;
            #[cfg_attr(target_arch = "x86_64", link_name = "stat$INODE64")]
            pub fn stat(path: *const u8, output: *mut DarwinStat) -> i32;
        }
        pub fn rc(value: i32) -> std::io::Result<()> { if value < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) } }
    }
    fn terminated(text: &str) -> Vec<u8> { text.bytes().chain(std::iter::once(0)).collect() }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_open_at_raw(pointer: *mut Fd72, path: JetCString, flags: i64, mode: i64, out: *mut Error32) -> *mut Fd72 {
        #[cfg(target_os = "macos")] {
            let bits = native(flags) as u32; let mode = native(mode) as u32;
            let path_bytes = terminated(view(path)); let state = lock(pointer);
            let fd = if state.closed != 0 { -1 } else { mac::openat(state.number as i32, path_bytes.as_ptr().cast(), posix_open(bits, true), mode) };
            let answer = if state.closed != 0 { Err(closed()) } else if fd < 0 { Err(io::Error::last_os_error()) } else { Ok(fd) };
            unlock(state); result(out, answer).map_or(ptr::null_mut(), |fd| new_fd(fd as i64, path, if bits & (WRITE | APPEND) != 0 { -8192 } else { 0 }))
        }
        #[cfg(windows)] {
            let _ = (pointer, path, flags, mode);
            error(out, 0, io::Error::new(io::ErrorKind::Unsupported, "opening inside a held directory is not available on this operating system")); ptr::null_mut()
        }
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_mkdir_at_raw(pointer: *mut Fd72, path: JetCString, mode: i64, out: *mut Error32) {
        #[cfg(target_os = "macos")] {
            let path = terminated(view(path)); let mode = native(mode) as u32; let state = lock(pointer);
            let answer = if state.closed != 0 { Err(closed()) } else { mac::rc(mac::mkdirat(state.number as i32, path.as_ptr().cast(), mode)) };
            unlock(state); result(out, answer);
        }
        #[cfg(windows)] { let _ = (pointer, path, mode); error(out, 0, io::Error::new(io::ErrorKind::Unsupported, "creating inside a held directory is not available on this operating system")); }
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_unlink_at_raw(pointer: *mut Fd72, path: JetCString, directory: bool, out: *mut Error32) {
        #[cfg(target_os = "macos")] {
            let path = terminated(view(path)); let state = lock(pointer);
            let answer = if state.closed != 0 { Err(closed()) } else { mac::rc(mac::unlinkat(state.number as i32, path.as_ptr().cast(), if directory { 0x80 } else { 0 })) };
            unlock(state); result(out, answer);
        }
        #[cfg(windows)] { let _ = (pointer, path, directory); error(out, 0, io::Error::new(io::ErrorKind::Unsupported, "removing inside a held directory is not available on this operating system")); }
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_readlink_at_raw(pointer: *mut Fd72, path: JetCString, bytes: *mut u8, length: i64, out: *mut Error32) -> i64 {
        #[cfg(target_os = "macos")] {
            let path = terminated(view(path)); let length = native(length) as usize; let state = lock(pointer);
            let answer = if state.closed != 0 { Err(closed()) } else {
                let count = mac::readlinkat(state.number as i32, path.as_ptr().cast(), bytes, length);
                if count < 0 { Err(io::Error::last_os_error()) } else { Ok(count) }
            }; unlock(state); result(out, answer).map_or(-1, |count| count as i64)
        }
        #[cfg(windows)] { let _ = (pointer, path, bytes, length); error(out, 0, io::Error::new(io::ErrorKind::Unsupported, "reading a link inside a held directory is not available on this operating system")); -1 }
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_rename_at_raw(source: *mut Fd72, old: JetCString, destination: *mut Fd72, new: JetCString, flags: i64, out: *mut Error32) {
        #[cfg(target_os = "macos")] {
            // Duplicate separately: never hold two carrier locks, even aliases.
            let mut error_row = Error32::success();
            let a = jet_fd_dup_raw(source, &mut error_row);
            if a.is_null() { out.write(error_row); return; }
            let b = jet_fd_dup_raw(destination, &mut error_row);
            if b.is_null() { jet_fd_drop_raw(a); out.write(error_row); return; }
            let old = terminated(view(old)); let new = terminated(view(new));
            result(out, mac::rc(mac::renameatx_np((*a).number as i32, old.as_ptr().cast(), (*b).number as i32, new.as_ptr().cast(), macos_rename(native(flags) as u32))));
            jet_fd_drop_raw(a); jet_fd_drop_raw(b);
        }
        #[cfg(windows)] { let _ = (source, old, destination, new, flags); error(out, 0, io::Error::new(io::ErrorKind::Unsupported, "renaming inside a held directory is not available on this operating system")); }
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_link_at_raw(source: *mut Fd72, directory: *mut Fd72, path: JetCString, out: *mut Error32) {
        let _ = (source, directory, path);
        error(out, 0, io::Error::new(io::ErrorKind::Unsupported, "linking an open file inside a held directory is not available on this operating system"));
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fs_readlink_raw(path: JetCString, bytes: *mut u8, length: i64, out: *mut Error32) -> i64 {
        #[cfg(target_os = "macos")] {
            use std::os::unix::ffi::OsStrExt;
            result(out, std::fs::read_link(view(path))).map_or(-1, |path| {
                let data = path.as_os_str().as_bytes(); let count = data.len().min(native(length) as usize);
                ptr::copy_nonoverlapping(data.as_ptr(), bytes, count); count as i64
            })
        }
        #[cfg(windows)] {
            use std::os::windows::ffi::OsStrExt;
            result(out, std::fs::read_link(view(path))).map_or(-1, |path| {
                let data = wtf8_from_wide(path.as_os_str().encode_wide());
                let count = data.len().min(native(length) as usize); ptr::copy_nonoverlapping(data.as_ptr(), bytes, count); count as i64
            })
        }
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_lock_raw(pointer: *mut Fd72, operation: i64, out: *mut Error32) {
        let operation = native(operation) as i32; let state = lock(pointer);
        #[cfg(target_os = "macos")]
        let answer = if state.closed != 0 { Err(closed()) } else { mac::rc(mac::flock(state.number as i32, operation)) };
        #[cfg(windows)]
        let answer = {
            #[repr(C)] struct Overlapped { internal: usize, internal_high: usize, offset: u32, offset_high: u32, event: usize }
            unsafe extern "system" {
                fn LockFileEx(file: usize, flags: u32, reserved: u32, low: u32, high: u32, overlapped: *mut Overlapped) -> i32;
                fn UnlockFileEx(file: usize, reserved: u32, low: u32, high: u32, overlapped: *mut Overlapped) -> i32;
            }
            if console(state) { Err(console_bytes()) } else if state.closed != 0 { Err(closed()) } else {
                let mut overlapped: Overlapped = std::mem::zeroed();
                let rc = if operation & 8 != 0 { UnlockFileEx(state.number as usize, 0, u32::MAX, u32::MAX, &mut overlapped) }
                else { LockFileEx(state.number as usize, u32::from(operation & 4 != 0) | if operation & 2 != 0 { 2 } else { 0 }, 0, u32::MAX, u32::MAX, &mut overlapped) };
                if rc == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
            }
        }; unlock(state); result(out, answer);
    }
    #[cfg(windows)]
    fn wtf8_from_wide(wide: impl Iterator<Item = u16>) -> Vec<u8> {
        let mut bytes = Vec::new();
        for code in std::char::decode_utf16(wide) {
            let code = code.map(|c| c as u32).unwrap_or_else(|e| e.unpaired_surrogate() as u32);
            if code < 0x80 { bytes.push(code as u8); }
            else if code < 0x800 { bytes.extend_from_slice(&[(0xc0 | code >> 6) as u8, (0x80 | code & 63) as u8]); }
            else if code < 0x10000 { bytes.extend_from_slice(&[(0xe0 | code >> 12) as u8, (0x80 | code >> 6 & 63) as u8, (0x80 | code & 63) as u8]); }
            else { bytes.extend_from_slice(&[(0xf0 | code >> 18) as u8, (0x80 | code >> 12 & 63) as u8, (0x80 | code >> 6 & 63) as u8, (0x80 | code & 63) as u8]); }
        } bytes
    }
    unsafe fn nul_path<'a>(path: *const u8) -> std::borrow::Cow<'a, std::path::Path> {
        let bytes = std::ffi::CStr::from_ptr(path.cast()).to_bytes();
        #[cfg(target_os = "macos")] {
            use std::os::unix::ffi::OsStrExt; std::borrow::Cow::Borrowed(std::path::Path::new(std::ffi::OsStr::from_bytes(bytes)))
        }
        #[cfg(windows)] {
            use std::os::windows::ffi::OsStringExt;
            let wide = wide_wtf8(bytes);
            std::borrow::Cow::Owned(std::path::PathBuf::from(std::ffi::OsString::from_wide(&wide[..wide.len() - 1])))
        }
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_process_path_stat_raw(path: *const u8, output: *mut i64, out: *mut Error32) {
        result(out, path_stat(&nul_path(path), true, output));
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_process_path_canonicalize_raw(path: *const u8, length: *mut i64, capacity: *mut i64, out: *mut Error32) -> *mut u8 {
        length.write(0); capacity.write(0);
        result(out, std::fs::canonicalize(&nul_path(path))).map_or(ptr::null_mut(), |path| {
            #[cfg(target_os = "macos")] let bytes = {
                use std::os::unix::ffi::OsStrExt; std::borrow::Cow::Borrowed(path.as_os_str().as_bytes())
            };
            #[cfg(windows)] let bytes = {
                use std::os::windows::ffi::OsStrExt; std::borrow::Cow::<[u8]>::Owned(wtf8_from_wide(path.as_os_str().encode_wide()))
            };
            let size = bytes.len(); let data = jet_rt_alloc(size, 1);
            ptr::copy_nonoverlapping(bytes.as_ptr(), data, size); length.write(size as i64); capacity.write(size as i64); data
        })
    }
    unsafe fn os_text(value: &std::ffi::OsStr) -> JetCString {
        #[cfg(windows)] let bytes = {
            use std::os::windows::ffi::OsStrExt; std::borrow::Cow::<[u8]>::Owned(wtf8_from_wide(value.encode_wide()))
        };
        #[cfg(target_os = "macos")] let bytes = {
            use std::os::unix::ffi::OsStrExt; std::borrow::Cow::Borrowed(value.as_bytes())
        };
        let length = bytes.len();
        let data = if length == 0 { 1usize as *mut u8 } else { jet_rt_alloc(length, 1) };
        ptr::copy_nonoverlapping(bytes.as_ptr(), data, length);
        let text = jet_rt_alloc(24, 8).cast::<usize>();
        text.write(data as usize); text.add(1).write(length); text.add(2).write(length);
        text.cast()
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_process_search_fact_raw(selector: i64, out: *mut Error32) -> JetCString {
        let selector = native(selector);
        let value: io::Result<std::ffi::OsString> = match selector {
            0 => std::env::current_exe().map(|path| path.parent().unwrap().as_os_str().to_owned()),
            // Source captures this fact with startup env, before any env.set.
            3 => Ok(std::env::var_os("PATH").unwrap_or_default()),
            4 => Ok(std::env::temp_dir().into_os_string()),
            #[cfg(windows)]
            1 | 2 => {
                #[link(name = "kernel32")]
                unsafe extern "system" {
                    fn GetSystemDirectoryW(buffer: *mut u16, capacity: u32) -> u32;
                    fn GetWindowsDirectoryW(buffer: *mut u16, capacity: u32) -> u32;
                }
                let mut buffer = vec![0u16; 260];
                loop {
                    let count = if selector == 1 { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) }
                        else { GetWindowsDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
                    if count == 0 { break Err(io::Error::last_os_error()); }
                    if count as usize >= buffer.len() { buffer.resize(count as usize + 1, 0); continue; }
                    use std::os::windows::ffi::OsStringExt;
                    break Ok(std::ffi::OsString::from_wide(&buffer[..count as usize]));
                }
            }
            _ => Err(io::Error::new(io::ErrorKind::Unsupported, "process search fact is unsupported on this target")),
        };
        result(out, value).map_or(ptr::null_mut(), |value| os_text(&value))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_process_temp_mkdir_raw(path: *const u8, out: *mut Error32) -> i64 {
        result(out, std::fs::create_dir(&nul_path(path))).map_or(-1, |()| 0)
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_process_temp_remove_raw(path: *const u8, out: *mut Error32) -> i64 {
        result(out, std::fs::remove_dir_all(&nul_path(path))).map_or(-1, |()| 0)
    }
    #[repr(C)]
    struct List24 { len: usize, capacity: usize, data: *mut i64 }
    unsafe fn list_clone(list: *mut List24, directories: bool) -> *mut List24 {
        if list.is_null() { return list; }
        let row = &*list; let stride = if directories { 3 } else { 1 };
        let data = if row.len == 0 { ptr::null_mut() } else { jet_rt_alloc(row.len * stride * 8, 8).cast::<i64>() };
        for index in 0..row.len {
            if directories {
                data.add(index * 3).write(jet_fd_retain_raw((*row.data.add(index * 3)) as *mut Fd72) as i64);
                data.add(index * 3 + 1).write(*row.data.add(index * 3 + 1));
                data.add(index * 3 + 2).write(*row.data.add(index * 3 + 2));
            } else { data.add(index).write(jet_rt_string_clone((*row.data.add(index)) as JetCString) as i64); }
        }
        let out = jet_rt_alloc(24, 8).cast::<List24>(); out.write(List24 { len: row.len, capacity: row.len, data }); out
    }
    unsafe fn list_drop(list: *mut List24, directories: bool) {
        if list.is_null() { return; }
        let row = &*list; let stride = if directories { 3 } else { 1 };
        for index in 0..row.len {
            if directories { jet_fd_drop_raw((*row.data.add(index * 3)) as *mut Fd72); }
            else { jet_rt_string_drop((*row.data.add(index)) as JetCString); }
        }
        if !row.data.is_null() { jet_rt_free(row.data.cast(), row.capacity * stride * 8, 8); }
        jet_rt_free(list.cast(), 24, 8);
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_process_spec_take_raw(words: *const i64) -> *mut i64 {
        let out = jet_rt_alloc(216, 8).cast::<i64>(); ptr::copy_nonoverlapping(words, out, 27); out
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_process_spec_clone_raw(words: *const i64) -> *mut i64 {
        let out = jet_process_spec_take_raw(words);
        for index in [0, 4, 5, 6] { out.add(index).write(list_clone((*words.add(index)) as *mut List24, false) as i64); }
        for index in [2, 25] {
            let text = *words.add(index) as JetCString;
            out.add(index).write(if text.is_null() { 0 } else { jet_rt_string_clone(text) as i64 });
        }
        let terminal = *words.add(23) as *const u8;
        if !terminal.is_null() {
            let copied = jet_rt_alloc(24, 8); ptr::copy_nonoverlapping(terminal, copied, 24); out.add(23).write(copied as i64);
        }
        out.add(26).write(list_clone((*words.add(26)) as *mut List24, true) as i64); out
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_process_spec_drop_raw(words: *mut i64) {
        if words.is_null() { return; }
        for index in [0, 4, 5, 6] { list_drop((*words.add(index)) as *mut List24, false); }
        for index in [2, 25] { let text = *words.add(index) as JetCString; if !text.is_null() { jet_rt_string_drop(text); } }
        let terminal = *words.add(23) as *mut u8; if !terminal.is_null() { jet_rt_free(terminal, 24, 8); }
        list_drop((*words.add(26)) as *mut List24, true); jet_rt_free(words.cast(), 216, 8);
    }
    #[repr(C)]
    pub struct Child72 {
        lock: AtomicI64, token: i64, reaped: i64, status: i64,
        stdin: i64, stdout: i64, stderr: i64, pty: i64, refs: i64,
    }
    const _: () = {
        assert!(std::mem::size_of::<Child72>() == 72);
        assert!(std::mem::offset_of!(Child72, lock) == 0);
        assert!(std::mem::offset_of!(Child72, pty) == 56);
        assert!(std::mem::offset_of!(Child72, refs) == 64);
        assert!(std::mem::size_of::<ExecCandidates32>() == 32);
        assert!(std::mem::offset_of!(ExecCandidates32, searched) == 24);
        assert!(std::mem::size_of::<WindowsLaunchFacts136>() == 136);
        assert!(std::mem::offset_of!(WindowsLaunchFacts136, job_memory) == 128);
        assert!(std::mem::size_of::<Error32>() == 32);
        assert!(std::mem::offset_of!(Error32, message) == 24);
    };
    unsafe fn child_lock<'a>(pointer: *mut Child72) -> &'a mut Child72 {
        while (*pointer).lock.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_err() { std::thread::yield_now(); }
        &mut *pointer
    }
    fn child_unlock(child: &Child72) { child.lock.store(0, Ordering::Release); }
    unsafe fn child_new(token: i64) -> *mut Child72 {
        let out = jet_rt_alloc(72, 8).cast::<Child72>();
        out.write(Child72 { lock: AtomicI64::new(0), token, reaped: 0, status: 0,
            stdin: -1, stdout: -1, stderr: -1, pty: -1, refs: 1 }); out
    }
    unsafe fn column<'a>(pointer: *const *const u8) -> impl Iterator<Item = &'a [u8]> {
        let mut at = pointer;
        std::iter::from_fn(move || { if at.is_null() || (*at).is_null() { return None; }
            let bytes = std::ffi::CStr::from_ptr((*at).cast()).to_bytes(); at = at.add(1); Some(bytes) })
    }
    #[cfg(target_os = "macos")]
    unsafe fn mac_spawn(spec: *const i64, candidates: *const ExecCandidates32, argv: *const *const u8, env: *const *const u8, out: *mut Error32) -> *mut Child72 {
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};
        let executable = *(*candidates).column;
        let mut command = Command::new(&*nul_path(executable));
        if *spec.add(1) != 0 { command.current_dir(view(*spec.add(2) as JetCString)); }
        let mode = |value: i64| match value { 1 => Stdio::inherit(), 3 => Stdio::null(), _ => Stdio::piped() };
        command.stdin(if *spec.add(7) == 0 { Stdio::null() } else { mode(*spec.add(8)) });
        command.stdout(mode(*spec.add(9))); command.stderr(mode(*spec.add(10)));
        let limits = crate::jet_process_pty::ResourceLimits {
            cpu_time_ms: if *spec.add(15) != 0 { Some(native(*spec.add(16))) } else { None },
            memory_bytes: if *spec.add(17) != 0 { Some(native(*spec.add(18))) } else { None },
            open_files: if *spec.add(19) != 0 { Some(native(*spec.add(20))) } else { None },
        };
        let terminal = if *spec.add(22) != 0 {
            let row = *spec.add(23) as *const i64;
            match crate::jet_process_pty::open(crate::jet_process_pty::PtyConfig { cols: native(*row), rows: native(*row.add(1)), raw: *row.add(2) == 0 }) {
                Ok(pair) => {
                    let configured = pair.slave.try_clone().and_then(|input| {
                        let output = pair.slave.try_clone()?; let error = pair.slave.try_clone()?;
                        command.stdin(input).stdout(output).stderr(error); Ok(())
                    });
                    if let Err(failure) = configured { error(out, 4, failure); return ptr::null_mut(); }
                    if let Err(failure) = crate::jet_process_pty::attach_command(&mut command, limits) { error(out, 5, failure); return ptr::null_mut(); }
                    Some(pair)
                }
                Err(failure) => { error(out, 9, failure); return ptr::null_mut(); }
            }
        } else {
            if *spec.add(21) != 0 {
                if let Err(failure) = crate::jet_process_pty::attach_process_group(&mut command, limits) { error(out, 5, failure); return ptr::null_mut(); }
            } else { command.process_group(0); }
            None
        };
        // Parent duplicates before fork, so source aliases may close freely.
        let dirs = *spec.add(26) as *const List24;
        let mut inherited = Vec::new();
        let mut minimum = 3;
        if !dirs.is_null() {
            for index in 0..(*dirs).len { minimum = minimum.max(native(*(*dirs).data.add(index * 3 + 1)) as i32 + 1); }
        }
        if !dirs.is_null() {
            for index in 0..(*dirs).len {
                let row = (*dirs).data.add(index * 3);
                let state = lock(*row as *mut Fd72);
                let owned = if state.closed != 0 { Err(closed()) } else {
                    let duplicate = mac::fcntl(state.number as i32, 67, minimum);
                    if duplicate < 0 { Err(io::Error::last_os_error()) } else { Ok(File::from_raw_fd(duplicate)) }
                };
                unlock(state);
                match owned {
                    Ok(file) => inherited.push((file, native(*row.add(1)) as i32, *row.add(2) != 0)),
                    Err(failure) => { error(out, 4, failure); return ptr::null_mut(); }
                }
            }
        }
        if !inherited.is_empty() {
            command.pre_exec(move || {
                use std::os::fd::AsRawFd;
                for (file, target, cwd) in &inherited {
                    mac::rc(mac::dup2(file.as_raw_fd(), *target))?;
                    if *cwd { mac::rc(mac::fchdir(*target))?; }
                }
                Ok(())
            });
        }
        // Every pointer is borrowed through synchronous spawn; the forked child
        // has its own COW view. Only sh argv is materialized, in the parent.
        let candidate_ptr = candidates as usize; let argv_ptr = argv as usize; let env_ptr = env as usize;
        let mut sh_argv = Vec::<usize>::new();
        let mut argc = 0usize; while !(*argv.add(argc)).is_null() { argc += 1; }
        sh_argv.reserve(argc.max(1) + 2);
        sh_argv.push(if argc == 0 { c"sh".as_ptr() as usize } else { *argv as usize });
        sh_argv.push(0);
        for index in 1..argc { sh_argv.push(*argv.add(index) as usize); }
        sh_argv.push(0);
        command.pre_exec(move || {
            let candidates = &*(candidate_ptr as *const ExecCandidates32);
            let argv = argv_ptr as *const *const u8; let env = env_ptr as *const *const u8;
            let mut access = false; let mut last = 2;
            for index in 0..candidates.count as usize {
                let candidate = *candidates.column.add(index);
                mac::execve(candidate, argv, env);
                last = *mac::errno();
                // Direct paths do not run the PATH retry table; only ENOEXEC
                // can invoke the parent-prepared shell fallback on that route.
                if candidates.searched == 0 && (last != 8 || candidates.script_fallback == 0) {
                    return Err(io::Error::from_raw_os_error(last));
                }
                match apple_exec_action(last, candidates.script_fallback != 0) {
                    AppleExecAction::Stop => return Err(io::Error::from_raw_os_error(last)),
                    AppleExecAction::Continue => {},
                    AppleExecAction::Script => {
                        sh_argv[1] = candidate as usize;
                        mac::execve(c"/bin/sh".as_ptr().cast(), sh_argv.as_ptr().cast(), env);
                        return Err(io::Error::from_raw_os_error(*mac::errno()));
                    }
                    AppleExecAction::Stat => {
                        let mut scratch = std::mem::MaybeUninit::<mac::DarwinStat>::uninit();
                        if mac::stat(candidate, scratch.as_mut_ptr()) != 0 { last = *mac::errno(); }
                        else if last == 13 { access = true; }
                        else { return Err(io::Error::from_raw_os_error(last)); }
                    }
                }
            }
            Err(io::Error::from_raw_os_error(apple_exec_finish(candidates.searched != 0, last, access)))
        });
        match command.spawn() {
            Ok(mut process) => {
                let child = child_new(process.id() as i64);
                (*child).stdin = process.stdin.take().map_or(-1, |file| file.into_raw_fd() as i64);
                (*child).stdout = process.stdout.take().map_or(-1, |file| file.into_raw_fd() as i64);
                (*child).stderr = process.stderr.take().map_or(-1, |file| file.into_raw_fd() as i64);
                if let Some(pair) = terminal { (*child).pty = pair.master.into_raw_fd() as i64; }
                out.write(Error32::success()); child
            }
            Err(failure) => { error(out, 7, failure); ptr::null_mut() }
        }
    }
    #[cfg(windows)]
    enum WindowsProcess {
        Sandbox(crate::jet_process_sandbox::WindowsSeedChild),
        Terminal { process: File, job: File, console: usize, pid: u32 },
    }
    #[cfg(windows)]
    struct WindowsSidecar { process: WindowsProcess }
    #[cfg(windows)]
    unsafe fn win_spawn(spec: *const i64, candidates: *const ExecCandidates32, argv: *const *const u8, env: *const *const u8,
        facts: *const WindowsLaunchFacts136, out: *mut Error32) -> *mut Child72 {
        let executable = *(*candidates).column;
        let facts = &*facts;
        let cwd = if *spec.add(1) != 0 { Some(wide_wtf8(view(*spec.add(2) as JetCString).as_bytes())) } else { None };
        let application = wide_wtf8(std::ffi::CStr::from_ptr(executable.cast()).to_bytes());
        let mut command = windows_command_line(column(argv));
        let mut environment = Vec::new();
        for bytes in column(env) { environment.extend(wide_wtf8(bytes)); }
        if environment.is_empty() { environment.push(0); } environment.push(0);
        let mut streams = [-1; 3]; let mut console = -1;
        let launch = if facts.pty != 0 {
            crate::jet_process_pty::spawn_seed(application, command, cwd, environment, facts).map(|process| {
                streams[0] = into_number(process.input); streams[1] = into_number(process.output);
                console = process.console as i64;
                WindowsProcess::Terminal { process: process.process, job: process.job, console: process.console, pid: process.pid }
            }).map_err(|e| (25, e))
        } else {
            let name = if facts.appcontainer != 0 { wide_wtf8(view(facts.name as JetCString).as_bytes()) } else { Vec::new() };
            let stdio = [if *spec.add(7) == 0 { 3 } else { *spec.add(8) }, *spec.add(9), *spec.add(10)];
            crate::jet_process_sandbox::windows_seed_spawn(&application, &mut command, &mut environment,
                cwd.as_deref(), facts, name, stdio).map(|mut process| {
                    streams[0] = process.stdin.take().map_or(-1, into_number);
                    streams[1] = process.stdout.take().map_or(-1, into_number);
                    streams[2] = process.stderr.take().map_or(-1, into_number);
                    WindowsProcess::Sandbox(process)
                })
        };
        match launch {
            Ok(process) => {
                let token = Box::into_raw(Box::new(WindowsSidecar { process }));
                let child = child_new(token as i64); (*child).stdin = streams[0]; (*child).stdout = streams[1]; (*child).stderr = streams[2]; (*child).pty = console;
                out.write(Error32::success()); child
            }
            Err((stage, failure)) => { error(out, stage, failure); ptr::null_mut() }
        }
    }
    #[cfg(windows)]
    fn windows_command_line<'a>(args: impl Iterator<Item = &'a [u8]>) -> Vec<u16> {
        let mut command = Vec::new();
        for arg in args {
            if !command.is_empty() { command.push(32); }
            let wide = wide_wtf8(arg); let value = &wide[..wide.len() - 1];
            let quote = value.is_empty() || value.iter().any(|c| *c == 32 || *c == 9 || *c == 34);
            if quote { command.push(34); }
            let mut slashes = 0;
            for &code in value {
                if code == 92 { slashes += 1; continue; }
                command.extend(std::iter::repeat_n(92, if code == 34 { slashes * 2 + 1 } else { slashes }));
                command.push(code); slashes = 0;
            }
            command.extend(std::iter::repeat_n(92, if quote { slashes * 2 } else { slashes }));
            if quote { command.push(34); }
        }
        command.push(0); command
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_process_spawn_raw(spec: *const i64, executable: *const ExecCandidates32, argv: *const *const u8,
        env: *const *const u8, os_facts: *const WindowsLaunchFacts136, out: *mut Error32) -> *mut Child72 {
        // Owner #33: a filtered-empty PATH is a defined not-found result,
        // never a candidate-column dereference or an attempted exec.
        if (*executable).count == 0 {
            error(out, if cfg!(windows) { 27 } else { 7 }, io::Error::from_raw_os_error(2));
            return ptr::null_mut();
        }
        #[cfg(target_os = "macos")] { let _ = os_facts; mac_spawn(spec, executable, argv, env, out) }
        #[cfg(windows)] { win_spawn(spec, executable, argv, env, os_facts, out) }
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_process_id_raw(pointer: *mut Child72) -> i64 {
        let child = child_lock(pointer);
        #[cfg(target_os = "macos")] let pid = child.token;
        #[cfg(windows)] let pid = match &(*(child.token as *const WindowsSidecar)).process {
            WindowsProcess::Sandbox(p) => p.pid, WindowsProcess::Terminal { pid, .. } => *pid,
        } as i64;
        child_unlock(child); pid
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_process_wait_raw(pointer: *mut Child72, nohang: bool, status: *mut i64, out: *mut Error32) -> i64 {
        out.write(Error32::success());
        let child = loop {
            let child = child_lock(pointer);
            if child.reaped != -1 { break child; }
            child_unlock(child);
            if nohang { return 0; }
            // Only another alias's blocking wait reaches this seed-only path.
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        #[cfg(target_os = "macos")] let pid = child.token;
        #[cfg(windows)] let pid = match &(*(child.token as *const WindowsSidecar)).process {
            WindowsProcess::Sandbox(p) => p.pid, WindowsProcess::Terminal { pid, .. } => *pid,
        } as i64;
        if child.reaped == 1 {
            status.write(child.status); child_unlock(child); return pid;
        }
        #[cfg(windows)] let (process_number, terminal) = {
            use std::os::windows::io::AsRawHandle;
            match &(*(child.token as *const WindowsSidecar)).process {
                WindowsProcess::Sandbox(p) => (p.process.as_raw_handle() as usize as i64, false),
                WindowsProcess::Terminal { process, .. } => (process.as_raw_handle() as usize as i64, true),
            }
        };
        child.reaped = -1; child_unlock(child);
        #[cfg(target_os = "macos")] let answer = {
            let mut raw = 0; let rc = mac::waitpid(pid as i32, &mut raw, i32::from(nohang));
            if rc < 0 { Err(io::Error::last_os_error()) } else if rc == 0 { Ok(None) } else { Ok(Some(raw as i64)) }
        };
        #[cfg(windows)] let answer = {
            if terminal {
                let process = file(process_number);
                let answer = if nohang { crate::jet_process_pty::try_wait(&process) } else { crate::jet_process_pty::wait(&process).map(Some) };
                answer.map(|s| s.map(|s| s as i64))
            } else {
                #[link(name = "kernel32")]
                unsafe extern "system" {
                    fn WaitForSingleObject(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
                    fn GetExitCodeProcess(handle: *mut std::ffi::c_void, code: *mut u32) -> i32;
                }
                let rc = WaitForSingleObject(process_number as usize as _, if nohang { 0 } else { u32::MAX });
                if rc == 258 { Ok(None) } else if rc == u32::MAX { Err(io::Error::last_os_error()) }
                else {
                    let mut code = 0;
                    if GetExitCodeProcess(process_number as usize as _, &mut code) == 0 { Err(io::Error::last_os_error()) }
                    else { Ok(Some(code as i64)) }
                }
            }
        };
        let child = child_lock(pointer);
        child.reaped = 0;
        let value = match answer {
            Ok(Some(raw)) => {
                child.reaped = 1; child.status = raw; status.write(raw);
                #[cfg(windows)] {
                    match &mut (*(child.token as *mut WindowsSidecar)).process {
                        WindowsProcess::Sandbox(p) => { let _ = p.wait(true); }
                        WindowsProcess::Terminal { job, .. } => { let _ = crate::jet_process_pty::terminate(job); }
                    }
                }
                pid
            },
            Ok(None) => 0, Err(failure) => { error(out, 0, failure); -1 }
        }; child_unlock(child); value
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_process_kill_raw(pointer: *mut Child72, signal: i64, group: bool, out: *mut Error32) {
        let signal = native(signal) as i32; let child = child_lock(pointer);
        #[cfg(target_os = "macos")] let answer = mac::rc(mac::kill(if group { -(child.token as i32) } else { child.token as i32 }, signal));
        #[cfg(windows)] let answer = match &mut (*(child.token as *mut WindowsSidecar)).process {
            WindowsProcess::Sandbox(p) => p.kill(),
            WindowsProcess::Terminal { job, pid, .. } => {
                if signal == 2 { let input = if child.stdin >= 0 { Some(file(child.stdin)) } else { None };
                    crate::jet_process_pty::interrupt(*pid, job, input.as_deref()) }
                else { crate::jet_process_pty::terminate(job) }
            }
        };
        #[cfg(windows)] let _ = group;
        child_unlock(child); result(out, answer);
    }
    fn stream(child: &mut Child72, index: i64) -> &mut i64 {
        match index { 0 => &mut child.stdin, 1 => &mut child.stdout, 2 => &mut child.stderr, _ => &mut child.pty }
    }
    unsafe fn child_io(pointer: *mut Child72, index: i64, bytes: *mut u8, length: i64, out: *mut Error32, write: bool) -> i64 {
        let index = native(index); let length = native(length) as usize; let child = child_lock(pointer);
        // ConPTY input/output are pipes; the console token is never a File.
        #[cfg(windows)] let index = if index == 3 { if write { 0 } else { 1 } } else { index };
        let number = *stream(child, index);
        let answer = if number < 0 { Err(closed()) } else if write {
            file(number).write(std::slice::from_raw_parts(bytes, length))
        } else { file(number).read(std::slice::from_raw_parts_mut(bytes, length)) };
        child_unlock(child); result(out, answer).map_or(-1, |count| count as i64)
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_process_read_raw(pointer: *mut Child72, index: i64, bytes: *mut u8, length: i64, out: *mut Error32) -> i64 { child_io(pointer, index, bytes, length, out, false) }
    #[no_mangle]
    pub unsafe extern "C" fn jet_process_write_raw(pointer: *mut Child72, index: i64, bytes: *mut u8, length: i64, out: *mut Error32) -> i64 { child_io(pointer, index, bytes, length, out, true) }
    #[no_mangle]
    pub unsafe extern "C" fn jet_process_resize_raw(pointer: *mut Child72, cols: i64, rows: i64, out: *mut Error32) {
        let config = crate::jet_process_pty::PtyConfig { cols: native(cols), rows: native(rows), raw: false };
        let child = child_lock(pointer);
        let answer = if child.pty < 0 { Err(closed()) } else {
            #[cfg(target_os = "macos")] { crate::jet_process_pty::resize(&file(child.pty), config) }
            #[cfg(windows)] { crate::jet_process_pty::resize_console(child.pty as usize, config) }
        }; child_unlock(child); result(out, answer);
    }
    unsafe fn close_stream(child: &mut Child72, index: i64) {
        let number = stream(child, index); if *number < 0 { return; }
        #[cfg(windows)] if index == 3 {
            crate::jet_process_pty::close_console(*number as usize);
            if let WindowsProcess::Terminal { console, .. } = &mut (*(child.token as *mut WindowsSidecar)).process { *console = 0; }
            child.pty = -1; return;
        }
        drop(ManuallyDrop::into_inner(file(*number))); *number = -1;
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_process_close_stream_raw(pointer: *mut Child72, index: i64, out: *mut Error32) {
        let index = native(index); let child = child_lock(pointer); close_stream(child, index); child_unlock(child); out.write(Error32::success());
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_process_retain_raw(pointer: *mut Child72) -> *mut Child72 {
        let child = child_lock(pointer); child.refs += 1; child_unlock(child); pointer
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_process_drop_raw(pointer: *mut Child72) {
        if pointer.is_null() { return; }
        let child = child_lock(pointer); child.refs -= 1;
        if child.refs != 0 { child_unlock(child); return; }
        for index in 0..4 { close_stream(child, index); }
        #[cfg(target_os = "macos")] if child.reaped == 0 {
            let mut status = 0; mac::waitpid(child.token as i32, &mut status, 0);
        }
        #[cfg(windows)] {
            let mut sidecar = Box::from_raw(child.token as *mut WindowsSidecar);
            match &mut sidecar.process {
                WindowsProcess::Sandbox(p) => { let _ = p.kill(); let _ = p.wait(false); },
                WindowsProcess::Terminal { process, job, .. } => { let _ = crate::jet_process_pty::terminate(job); let _ = crate::jet_process_pty::wait(process); },
            }
            drop(sidecar);
        }
        jet_rt_free(pointer.cast(), 72, 8);
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_process_take_stream_raw(pointer: *mut Child72, index: i64, out: *mut Error32) -> *mut Fd72 {
        let index = native(index); let child = child_lock(pointer);
        #[cfg(target_os = "macos")]
        let answer = if (index == 0 || index == 1) && child.pty >= 0 {
            result(out, file(child.pty).try_clone()).map_or(ptr::null_mut(), |f| new_fd(into_number(f), ptr::null_mut(), 0))
        } else {
            let slot = stream(child, index);
            if *slot < 0 { error(out, 0, closed()); ptr::null_mut() }
            else { let fd = new_fd(*slot, ptr::null_mut(), 0); *slot = -1; out.write(Error32::success()); fd }
        };
        #[cfg(windows)]
        let answer = {
            let number = *stream(child, index);
            if number < 0 { error(out, 0, closed()); ptr::null_mut() }
            else {
                let fd = new_fd(number, ptr::null_mut(), if index == 3 { i64::MIN } else { 0 });
                *stream(child, index) = -1;
                if index == 3 {
                    if let WindowsProcess::Terminal { console, .. } = &mut (*(child.token as *mut WindowsSidecar)).process { *console = 0; }
                }
                out.write(Error32::success()); fd
            }
        };
        child_unlock(child); answer
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_fd_resize_raw(pointer: *mut Fd72, cols: i64, rows: i64, out: *mut Error32) {
        let config = crate::jet_process_pty::PtyConfig { cols: native(cols), rows: native(rows), raw: false };
        let state = lock(pointer);
        let answer = if state.closed != 0 { Err(closed()) } else {
            #[cfg(windows)] { if console(state) { crate::jet_process_pty::resize_console(state.number as usize, config) } else { Err(console_bytes()) } }
            #[cfg(target_os = "macos")] { crate::jet_process_pty::resize(&file(state.number), config) }
        }; unlock(state); result(out, answer);
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_host_os_error_text_raw(domain: i64, code: i64) -> JetCString {
        let domain = native(domain); let code = native(code);
        handle(if domain == 3 { format!("HRESULT 0x{:08x}", code as u32) }
            else { io::Error::from_raw_os_error(code as i32).to_string() })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_process_path_exists_raw(path: *const u8) -> bool {
        #[cfg(windows)] {
            unsafe extern "system" { fn GetFileAttributesW(path: *const u16) -> u32; }
            let wide = wide_wtf8(std::ffi::CStr::from_ptr(path.cast()).to_bytes());
            GetFileAttributesW(wide.as_ptr()) != u32::MAX
        }
        #[cfg(target_os = "macos")] { std::fs::symlink_metadata(&nul_path(path)).is_ok() }
    }
}
