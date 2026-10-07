// Native scalar adapters the canonical C ABI (CAbi.rs) lacks. An F32 crosses
// the System V ABI in the low 32 bits of an xmm register; its text is the
// Prelude's own JetDisplay (Values.rs: `{:?}`, Display and Debug alike).
#[cfg(not(target_arch = "wasm32"))]
mod jet_c_abi_numeric {
    use super::jet_c_abi::{guard, handle, JetCString};

    #[no_mangle]
    pub extern "C" fn jet_rt_float32_to_string(value: f32) -> JetCString {
        guard(|| handle(super::jet_fmt_display(&value)))
    }
}
