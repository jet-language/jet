// D-FREESTAND-PROVIDER1=A: target adapters marshal the checked provider
// contracts.  This file contains no host fallback and no success-shaped stub.
// Codegen appends only declarations/wrappers for ProviderPolicy facts selected
// by the checked TargetDossier.

use core::fmt;
use core::fmt::Write;
use core::sync::atomic::Ordering;
use crate::RuntimeDiagnosticCore::{
    JetRuntimeStopContext, jet_runtime_stop_status, jet_write_runtime_stop,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JetTargetError {
    pub operation: &'static str,
    pub status: i32,
}

impl JetTargetError {
    pub const fn provider(operation: &'static str, status: i32) -> Self {
        Self { operation, status }
    }

    pub const fn unavailable(operation: &'static str) -> Self {
        Self {
            operation,
            status: -1,
        }
    }

    pub const fn protocol(operation: &'static str) -> Self {
        Self {
            operation,
            status: -2,
        }
    }
}

impl JetPortableError for JetTargetError {
    fn jet_error_bytes(&self) -> &[u8] {
        self.operation.as_bytes()
    }

    fn jet_error_status(&self) -> i32 {
        self.status
    }
}

#[inline(always)]
fn checked_used(operation: &'static str, requested: usize, used: usize) -> Result<usize, JetTargetError> {
    if used <= requested {
        Ok(used)
    } else {
        Err(JetTargetError::protocol(operation))
    }
}


#[inline(always)]
pub fn jet_target_read_bytes(buffer: &mut [u8]) -> Result<usize, JetTargetError> {
    let used = jet_target_read_bytes_impl(buffer)?;
    checked_used("read", buffer.len(), used)
}

#[inline(always)]
pub fn jet_target_write_bytes(buffer: &[u8]) -> Result<usize, JetTargetError> {
    let used = jet_target_write_bytes_impl(buffer)?;
    checked_used("write", buffer.len(), used)
}

#[inline(always)]
pub fn jet_target_report_bytes(buffer: &[u8]) -> Result<usize, JetTargetError> {
    let used = jet_target_report_bytes_impl(buffer)?;
    checked_used("report", buffer.len(), used)
}

#[inline(always)]
pub fn jet_target_wall_clock() -> Result<i64, JetTargetError> {
    jet_target_wall_clock_impl()
}

#[inline(always)]
pub fn jet_target_monotonic_clock() -> Result<u64, JetTargetError> {
    jet_target_monotonic_clock_impl()
}

#[inline(always)]
pub fn jet_target_sleep(nanoseconds: u64) -> Result<(), JetTargetError> {
    jet_target_sleep_impl(nanoseconds)
}

#[inline(always)]
pub fn jet_target_entropy(buffer: &mut [u8]) -> Result<(), JetTargetError> {
    jet_target_entropy_impl(buffer)
}

#[inline(always)]
pub fn jet_target_mmio_read(address: u64, buffer: &mut [u8]) -> Result<usize, JetTargetError> {
    let used = jet_target_mmio_read_impl(address, buffer)?;
    checked_used("mmio_read", buffer.len(), used)
}

#[inline(always)]
pub fn jet_target_mmio_write(address: u64, buffer: &[u8]) -> Result<usize, JetTargetError> {
    let used = jet_target_mmio_write_impl(address, buffer)?;
    checked_used("mmio_write", buffer.len(), used)
}

#[inline(always)]
pub fn jet_target_scheduler_yield() {
    jet_target_scheduler_yield_impl()
}

struct JetTargetWriter {
    report: bool,
}

impl fmt::Write for JetTargetWriter {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let mut remaining = value.as_bytes();
        while !remaining.is_empty() {
            let used = if self.report {
                jet_target_report_bytes(remaining)
            } else {
                jet_target_write_bytes(remaining)
            }.map_err(|_| fmt::Error)?;
            if used == 0 {
                return Err(fmt::Error);
            }
            remaining = &remaining[used..];
        }
        Ok(())
    }
}

#[inline]
pub fn jet_target_print(args: fmt::Arguments<'_>) {
    if (JetTargetWriter { report: false }).write_fmt(args).is_err() {
        jet_target_failure_bytes(b"target write failed", -1);
    }
}

#[inline]
pub fn jet_target_println(args: fmt::Arguments<'_>) {
    jet_target_print(args);
    jet_target_print(format_args!("\n"));
}

#[inline]
pub fn jet_target_report(args: fmt::Arguments<'_>) {
    if (JetTargetWriter { report: true }).write_fmt(args).is_err() {
        jet_target_failure_bytes(b"target report failed", -1);
    }
}

#[inline]
pub fn jet_target_reportln(args: fmt::Arguments<'_>) {
    jet_target_report(args);
    jet_target_report(format_args!("\n"));
}

#[inline(never)]
pub fn jet_target_failure_bytes(message: &[u8], status: i32) -> ! {
    __JET_LAST_TARGET_STATUS.store(status, Ordering::Release);
    let _ = jet_target_report_bytes(message);
    jet_target_abort()
}

#[inline(never)]
pub fn jet_target_failure_render<F>(status: i32, render: F) -> !
where
    F: FnOnce(&mut dyn fmt::Write) -> fmt::Result,
{
    __JET_LAST_TARGET_STATUS.store(status, Ordering::Release);
    let mut writer = JetTargetWriter { report: true };
    let _ = render(&mut writer);
    jet_target_abort()
}

#[inline(never)]
pub fn jet_target_failure_fmt<M: fmt::Display>(
    code: &str,
    file: &str,
    line: u32,
    message: M,
    status: i32,
) -> ! {
    jet_target_failure_render(status, |out| {
        out.write_fmt(format_args!("{code} {file}:{line}: {message}\n"))
    })
}

#[inline(never)]
pub fn jet_target_exit(status: i32) -> ! {
    __JET_LAST_TARGET_STATUS.store(status, Ordering::Release);
    jet_target_abort()
}

// Board profiles expose the authoritative abort ABI.  Wasm has no provider
// object in the no-OS profile and therefore traps through its target primitive;
// it must never acquire a C import through this fallback.
#[cfg(target_arch = "wasm32")]
#[inline(never)]
pub fn jet_target_abort() -> ! {
    core::arch::wasm32::unreachable()
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "none"))]
extern "C" {
    fn __jet_target_abort();
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "none"))]
#[inline(never)]
pub fn jet_target_abort() -> ! {
    unsafe { __jet_target_abort() };
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "none")))]
#[inline(never)]
pub fn jet_target_abort() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn jet_panic_handler(info: &core::panic::PanicInfo<'_>) -> ! {
    if __JET_TARGET_PANIC_REPORT {
        let (file, line) = info
            .location()
            .map(|location| (location.file(), location.line()))
            .unwrap_or(("", 0));
        let message = info.message();
        let row = jet_runtime_diagnostic_row("E3001");
        jet_target_failure_render(jet_runtime_stop_status(row), |out| {
            jet_write_runtime_stop(
                out,
                row,
                "E3001",
                JetRuntimeStopContext {
                    file,
                    line,
                    function: "",
                    source_line: "",
                    column: 1,
                    caret_len: 1,
                    expected_type: "",
                },
                &message,
                None,
            )
        });
    }
    jet_target_exit(101)
}
