#![allow(dead_code)]
// Cross-target type-check witness: the real leaf bodies and existing OS modules,
// not a second backend. Runtime-owned allocator/text/Int symbols remain imports.
#[path = "../../../../../crates/jet-codegen/src/Prelude/Core/CAbiPlatformSeeds.rs"]
mod seeds;
#[cfg(windows)]
pub(crate) use seeds::jet_platform_seed_abi;
mod jet_c_abi_host { include!(concat!(env!("OUT_DIR"), "/host-io-kinds.rs")); }
#[cfg(any(target_os = "windows", target_os = "macos"))]
mod jet_c_abi {
    #[repr(C)]
    pub(crate) struct JetCText { ptr: *mut u8, len: usize, cap: usize }
    pub(crate) type JetCString = *mut JetCText;
    unsafe extern "C" { fn jet_rt_alloc(size: usize, align: usize) -> *mut u8; }
    pub(crate) fn handle(value: String) -> JetCString {
        unsafe {
            let size = value.len(); let data = if size == 0 { 1usize as *mut u8 } else { jet_rt_alloc(size, 1) };
            std::ptr::copy_nonoverlapping(value.as_ptr(), data, size);
            let out = jet_rt_alloc(24, 8).cast::<JetCText>();
            out.write(JetCText { ptr: data, len: size, cap: size }); out
        }
    }
    pub(crate) fn view<'a>(value: JetCString) -> &'a str {
        unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts((*value).ptr, (*value).len)) }
    }
}
#[cfg(any(target_os = "windows", target_os = "macos"))]
#[path = "../../../../../crates/jet-codegen/src/Prelude/TerminalDefault.rs"]
mod terminal_default;
#[cfg(target_os = "macos")]
#[path = "../../../../../crates/jet-codegen/src/Prelude/Term.rs"]
mod term;
#[cfg(target_os = "macos")]
pub(crate) use term::jet_term_configure_fd;
#[cfg(any(target_os = "windows", target_os = "macos"))]
#[path = "../../../../../crates/jet-codegen/src/Prelude/CoreLib/ProcessPty.rs"]
mod jet_process_pty;
#[cfg(windows)]
mod jet_process_sandbox {
    include!("../../../../../crates/jet-codegen/src/Prelude/CoreLib/Top/ProcessSandbox.rs");
    include!("../../../../../crates/jet-codegen/src/Prelude/CoreLib/Top/ProcessWindowsSandbox.rs");
}
