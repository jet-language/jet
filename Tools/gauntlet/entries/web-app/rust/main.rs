#![no_std]

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicI32, Ordering};

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}

// The counter's state lives in Wasm; the page's click handlers call these exports.
static COUNT: AtomicI32 = AtomicI32::new(0);

#[no_mangle]
pub extern "C" fn increment() -> i32 {
    COUNT.fetch_add(1, Ordering::Relaxed) + 1
}

#[no_mangle]
pub extern "C" fn reset() -> i32 {
    COUNT.store(0, Ordering::Relaxed);
    0
}

#[no_mangle]
pub extern "C" fn count() -> i32 {
    COUNT.load(Ordering::Relaxed)
}
