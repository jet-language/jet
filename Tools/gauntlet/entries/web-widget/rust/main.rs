#![no_std]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn triangular(n: i32) -> i32 {
    let mut total = 0;
    let mut i = 1;
    while i <= n {
        total += i;
        i += 1;
    }
    total
}
