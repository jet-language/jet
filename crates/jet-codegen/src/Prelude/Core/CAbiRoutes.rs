// BEGIN GENERATED C-ABI ROUTES (Tools/agent/gen-c-abi-routes.mjs; do not hand-edit)
// One adapter per Core call registry route whose kernel signature has a C
// carrier (CAbi.rs carrier rules). Skipped routes are listed at the end.
#[cfg(not(target_arch = "wasm32"))]
#[allow(non_snake_case, clippy::all)]
mod jet_c_abi_routes {
    use super::jet_c_abi::{guard, handle, native, view, JetCString};

    #[cold]
    fn range_stop() -> ! {
        super::jet_arithmetic_stop("<core.prelude>", 0, "native Int argument exceeds host range")
    }

    /// `native_int_input` narrowed to a kernel's fixed-width parameter.
    fn fixed<T: TryFrom<i64>>(value: i64) -> T {
        T::try_from(native(value)).unwrap_or_else(|_| range_stop())
    }

    /// A Char word: its Unicode scalar value (as jet_rt_char_to_string reads it).
    fn scalar(value: i64) -> char {
        u32::try_from(value).ok().and_then(char::from_u32).unwrap_or(char::REPLACEMENT_CHARACTER)
    }

    /// A List argument's `len` eight-byte slots at `data`, as Rust values.
    fn list_in<T>(data: *const u64, len: i64, item: impl Fn(u64) -> T) -> Vec<T> {
        if len <= 0 || data.is_null() {
            return Vec::new();
        }
        // SAFETY: the caller passes its List's slot buffer and length.
        unsafe { std::slice::from_raw_parts(data, len as usize) }.iter().map(|w| item(*w)).collect()
    }

    /// A List result: a slot buffer from the allocator `jet_rt_free` returns
    /// to (null when empty), written to `data`; returns the length.
    fn list_out(words: Vec<u64>, data: *mut *mut u64) -> i64 {
        let len = words.len();
        let buffer = if len == 0 {
            std::ptr::null_mut()
        } else {
            let layout = std::alloc::Layout::from_size_align(len * 8, 8).unwrap_or_else(|_| range_stop());
            // SAFETY: `layout` has a nonzero size.
            let buffer = unsafe { std::alloc::alloc(layout) } as *mut u64;
            if buffer.is_null() {
                std::alloc::handle_alloc_error(layout);
            }
            // SAFETY: `buffer` holds `len` slots.
            unsafe { std::ptr::copy_nonoverlapping(words.as_ptr(), buffer, len) };
            buffer
        };
        // SAFETY: the caller passes a writable out slot.
        unsafe { data.write(buffer) };
        len as i64
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_Clock(value: *mut crate::jet_std::Clock) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_DataTree(value: *mut crate::jet_std::DataTree) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DataTree(value: *mut crate::jet_std::DataTree) -> *mut crate::jet_std::DataTree {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_DBError(value: *mut crate::jet_std::DBError) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DBError(value: *mut crate::jet_std::DBError) -> *mut crate::jet_std::DBError {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_DBValue(value: *mut crate::jet_std::DBValue) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DBValue(value: *mut crate::jet_std::DBValue) -> *mut crate::jet_std::DBValue {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_Duration(value: *mut crate::jet_std::Duration) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_Duration(value: *mut crate::jet_std::Duration) -> *mut crate::jet_std::Duration {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_EncodingError(value: *mut crate::jet_std::EncodingError) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_EncodingError(value: *mut crate::jet_std::EncodingError) -> *mut crate::jet_std::EncodingError {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_Fake(value: *mut crate::jet_std::Fake) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_Fake(value: *mut crate::jet_std::Fake) -> *mut crate::jet_std::Fake {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_IOError(value: *mut crate::jet_std::IOError) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_IOError(value: *mut crate::jet_std::IOError) -> *mut crate::jet_std::IOError {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetBitSet(value: *mut crate::JetBitSet) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetChain(value: *mut crate::JetChain) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetCounter(value: *mut crate::JetCounter) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetCursor(value: *mut crate::JetCursor) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetCursor(value: *mut crate::JetCursor) -> *mut crate::JetCursor {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDate(value: *mut crate::JetDate) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDateTime(value: *mut crate::JetDateTime) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDateTime(value: *mut crate::JetDateTime) -> *mut crate::JetDateTime {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDBRow(value: *mut crate::jet_std::JetDBRow) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDecimal(value: *mut crate::jet_std::JetDecimal) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDeterministicWorld(value: *mut crate::JetDeterministicWorld) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDeterministicWorld(value: *mut crate::JetDeterministicWorld) -> *mut crate::JetDeterministicWorld {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDNSSrv(value: *mut crate::JetDNSSrv) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDNSSrv(value: *mut crate::JetDNSSrv) -> *mut crate::JetDNSSrv {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetErr(value: *mut crate::JetErr) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetErr(value: *mut crate::JetErr) -> *mut crate::JetErr {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetFraction(value: *mut crate::jet_std::JetFraction) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetFraction(value: *mut crate::jet_std::JetFraction) -> *mut crate::jet_std::JetFraction {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHistoryRng(value: *mut crate::JetHistoryRng) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetInstant(value: *mut crate::JetInstant) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetInstant(value: *mut crate::JetInstant) -> *mut crate::JetInstant {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetIpAddr(value: *mut crate::JetIpAddr) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetIpAddr(value: *mut crate::JetIpAddr) -> *mut crate::JetIpAddr {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetKey(value: *mut crate::JetKey) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetKey(value: *mut crate::JetKey) -> *mut crate::JetKey {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetLocalTime(value: *mut crate::JetLocalTime) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetMemoStats(value: *mut crate::JetMemoStats) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetMemoStats(value: *mut crate::JetMemoStats) -> *mut crate::JetMemoStats {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetMIME(value: *mut crate::jet_std::JetMIME) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetMIME(value: *mut crate::jet_std::JetMIME) -> *mut crate::jet_std::JetMIME {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetNetError(value: *mut crate::JetNetError) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetNetError(value: *mut crate::JetNetError) -> *mut crate::JetNetError {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetNetReady(value: *mut crate::JetNetReady) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetNetReady(value: *mut crate::JetNetReady) -> *mut crate::JetNetReady {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetOrderedMap(value: *mut crate::JetOrderedMap) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetPeriod(value: *mut crate::JetPeriod) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetReader(value: *mut crate::JetReader) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetReader(value: *mut crate::JetReader) -> *mut crate::JetReader {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetRegex(value: *mut crate::jet_std::JetRegex) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetRegex(value: *mut crate::jet_std::JetRegex) -> *mut crate::jet_std::JetRegex {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetRegexMatch(value: *mut crate::jet_std::JetRegexMatch) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetSocketAddr(value: *mut crate::JetSocketAddr) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetSocketAddr(value: *mut crate::JetSocketAddr) -> *mut crate::JetSocketAddr {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTCPListener(value: *mut crate::JetTCPListener) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTCPStream(value: *mut crate::JetTCPStream) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTestComparison(value: *mut crate::jet_std::JetTestComparison) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTestComparison(value: *mut crate::jet_std::JetTestComparison) -> *mut crate::jet_std::JetTestComparison {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTestSuite(value: *mut crate::jet_std::JetTestSuite) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTestSuite(value: *mut crate::jet_std::JetTestSuite) -> *mut crate::jet_std::JetTestSuite {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTLSClientConfig(value: *mut crate::JetTLSClientConfig) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTLSClientConfig(value: *mut crate::JetTLSClientConfig) -> *mut crate::JetTLSClientConfig {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTLSPeerIdentity(value: *mut crate::JetTLSPeerIdentity) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTLSPeerIdentity(value: *mut crate::JetTLSPeerIdentity) -> *mut crate::JetTLSPeerIdentity {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTLSStream(value: *mut crate::JetTLSStream) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetUDPPacket(value: *mut crate::JetUDPPacket) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetUDPPacket(value: *mut crate::JetUDPPacket) -> *mut crate::JetUDPPacket {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetUDPSocket(value: *mut crate::JetUDPSocket) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetUDPSocket(value: *mut crate::JetUDPSocket) -> *mut crate::JetUDPSocket {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetURL(value: *mut crate::jet_std::JetURL) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetURL(value: *mut crate::jet_std::JetURL) -> *mut crate::jet_std::JetURL {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetZone(value: *mut crate::JetZone) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetZonedDateTime(value: *mut crate::JetZonedDateTime) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_LogField(value: *mut crate::jet_std::LogField) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_LogField(value: *mut crate::jet_std::LogField) -> *mut crate::jet_std::LogField {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_LogSpan(value: *mut crate::jet_std::LogSpan) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_LogSpan(value: *mut crate::jet_std::LogSpan) -> *mut crate::jet_std::LogSpan {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_RangeError(value: *mut crate::jet_std::RangeError) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_RangeError(value: *mut crate::jet_std::RangeError) -> *mut crate::jet_std::RangeError {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_RaylibColor(value: *mut crate::RaylibColor) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_RaylibSound(value: *mut crate::RaylibSound) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_RaylibTextureAtlas(value: *mut crate::RaylibTextureAtlas) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_RaylibWindow(value: *mut crate::RaylibWindow) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_RegexFlags(value: *mut crate::jet_std::RegexFlags) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_RegexFlags(value: *mut crate::jet_std::RegexFlags) -> *mut crate::jet_std::RegexFlags {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_Rng(value: *mut crate::jet_std::Rng) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_Rng(value: *mut crate::jet_std::Rng) -> *mut crate::jet_std::Rng {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_Solver(value: *mut crate::jet_std::Solver) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_Solver(value: *mut crate::jet_std::Solver) -> *mut crate::jet_std::Solver {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_Stopwatch(value: *mut crate::jet_std::Stopwatch) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_Stopwatch(value: *mut crate::jet_std::Stopwatch) -> *mut crate::jet_std::Stopwatch {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_UTF8Error(value: *mut crate::jet_std::UTF8Error) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_UTF8Error(value: *mut crate::jet_std::UTF8Error) -> *mut crate::jet_std::UTF8Error {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_XMLCanonical(value: *mut crate::jet_std::XMLCanonical) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_XMLCanonical(value: *mut crate::jet_std::XMLCanonical) -> *mut crate::jet_std::XMLCanonical {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_XMLError(value: *mut crate::jet_std::XMLError) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_XMLError(value: *mut crate::jet_std::XMLError) -> *mut crate::jet_std::XMLError {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_XMLParseOptions(value: *mut crate::jet_std::XMLParseOptions) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_XMLParseOptions(value: *mut crate::jet_std::XMLParseOptions) -> *mut crate::jet_std::XMLParseOptions {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    /// core.builtin.bit_set_count
    #[no_mangle]
    pub extern "C" fn jet_bit_set_count(a0: *mut crate::JetBitSet) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_bit_set_count(unsafe { &*a0 })))
    }

    /// core.builtin.bit_set_to_list
    #[no_mangle]
    pub extern "C" fn jet_bit_set_to_list(a0: *mut crate::JetBitSet, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_bit_set_to_list(unsafe { &*a0 }).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.builtin.bitset_copy
    #[no_mangle]
    pub extern "C" fn jet_bits_copy(a0: *mut crate::JetBitSet) -> *mut crate::JetBitSet {
        guard(|| Box::into_raw(Box::new(crate::jet_bits_copy(unsafe { &*a0 }))))
    }

    /// core.time.calendar.day_abbr
    #[no_mangle]
    pub extern "C" fn jet_calendar_day_abbr(a0: i64) -> JetCString {
        guard(|| handle(crate::jet_calendar_day_abbr(a0)))
    }

    /// core.time.calendar.day_name
    #[no_mangle]
    pub extern "C" fn jet_calendar_day_name(a0: i64) -> JetCString {
        guard(|| handle(crate::jet_calendar_day_name(a0)))
    }

    /// core.time.calendar.formatmonth
    #[no_mangle]
    pub extern "C" fn jet_calendar_formatmonth(a0: i64, a1: i64, a2: i64) -> JetCString {
        guard(|| handle(crate::jet_calendar_formatmonth(a0, a1, a2)))
    }

    /// core.time.calendar.formatyear
    #[no_mangle]
    pub extern "C" fn jet_calendar_formatyear(a0: i64) -> JetCString {
        guard(|| handle(crate::jet_calendar_formatyear(a0)))
    }

    /// core.time.calendar.isleap
    #[no_mangle]
    pub extern "C" fn jet_calendar_isleap(a0: i64) -> bool {
        guard(|| crate::jet_calendar_isleap(a0))
    }

    /// core.time.calendar.month_abbr
    #[no_mangle]
    pub extern "C" fn jet_calendar_month_abbr(a0: i64) -> JetCString {
        guard(|| handle(crate::jet_calendar_month_abbr(a0)))
    }

    /// core.time.calendar.month_name
    #[no_mangle]
    pub extern "C" fn jet_calendar_month_name(a0: i64) -> JetCString {
        guard(|| handle(crate::jet_calendar_month_name(a0)))
    }

    /// core.time.calendar.weekheader
    #[no_mangle]
    pub extern "C" fn jet_calendar_weekheader(a0: i64, a1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_calendar_weekheader(a0, a1).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.handle.clock.now
    #[no_mangle]
    pub extern "C" fn jet_clock_now(a0: *mut crate::jet_std::Clock) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_clock_now(unsafe { &*a0 })))
    }

    /// core.collections.chain
    #[no_mangle]
    pub extern "C" fn jet_coll_chain() -> *mut crate::JetChain {
        guard(|| Box::into_raw(Box::new(crate::jet_coll_chain())))
    }

    /// core.collections.chain_contains
    #[no_mangle]
    pub extern "C" fn jet_coll_chain_contains(a0: *mut crate::JetChain, a1: JetCString) -> bool {
        guard(|| crate::jet_coll_chain_contains(unsafe { &*a0 }, unsafe { &*a1 }))
    }

    /// core.collections.chain_get
    #[no_mangle]
    pub extern "C" fn jet_coll_chain_get(a0: *mut crate::JetChain, a1: JetCString, some: *mut JetCString) -> i64 {
        guard(|| match crate::jet_coll_chain_get(unsafe { &*a0 }, unsafe { &*a1 }) {
            Some(value) => { unsafe { some.write(handle(value)) }; 1 }
            None => 0,
        })
    }

    /// core.collections.chain_push
    #[no_mangle]
    pub extern "C" fn jet_coll_chain_push(a0: *mut crate::JetChain, a1_0: *const u64, a1_1: i64, a2_0: *const u64, a2_1: i64) -> *mut crate::JetChain {
        guard(|| Box::into_raw(Box::new(crate::jet_coll_chain_push(unsafe { &*a0 }, &list_in(a1_0, a1_1, |w| view(w as JetCString).to_owned()), &list_in(a2_0, a2_1, |w| view(w as JetCString).to_owned())))))
    }

    /// core.collections.counter
    #[no_mangle]
    pub extern "C" fn jet_coll_counter() -> *mut crate::JetCounter {
        guard(|| Box::into_raw(Box::new(crate::jet_coll_counter())))
    }

    /// core.collections.add
    #[no_mangle]
    pub extern "C" fn jet_coll_counter_add(a0: *mut crate::JetCounter, a1: JetCString, a2: i64) -> *mut crate::JetCounter {
        guard(|| Box::into_raw(Box::new(crate::jet_coll_counter_add(unsafe { &*a0 }, unsafe { &*a1 }, a2))))
    }

    /// core.collections.clear_counter
    #[no_mangle]
    pub extern "C" fn jet_coll_counter_clear(a0: *mut crate::JetCounter) -> *mut crate::JetCounter {
        guard(|| Box::into_raw(Box::new(crate::jet_coll_counter_clear(unsafe { &*a0 }))))
    }

    /// core.collections.dec
    #[no_mangle]
    pub extern "C" fn jet_coll_counter_dec(a0: *mut crate::JetCounter, a1: JetCString) -> *mut crate::JetCounter {
        guard(|| Box::into_raw(Box::new(crate::jet_coll_counter_dec(unsafe { &*a0 }, unsafe { &*a1 }))))
    }

    /// core.collections.elements
    #[no_mangle]
    pub extern "C" fn jet_coll_counter_elements(a0: *mut crate::JetCounter, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_coll_counter_elements(unsafe { &*a0 }).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.collections.counter_from
    #[no_mangle]
    pub extern "C" fn jet_coll_counter_from(a0_0: *const u64, a0_1: i64) -> *mut crate::JetCounter {
        guard(|| Box::into_raw(Box::new(crate::jet_coll_counter_from(&list_in(a0_0, a0_1, |w| view(w as JetCString).to_owned())))))
    }

    /// core.collections.get
    #[no_mangle]
    pub extern "C" fn jet_coll_counter_get(a0: *mut crate::JetCounter, a1: JetCString) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_coll_counter_get(unsafe { &*a0 }, unsafe { &*a1 })))
    }

    /// core.collections.inc
    #[no_mangle]
    pub extern "C" fn jet_coll_counter_inc(a0: *mut crate::JetCounter, a1: JetCString) -> *mut crate::JetCounter {
        guard(|| Box::into_raw(Box::new(crate::jet_coll_counter_inc(unsafe { &*a0 }, unsafe { &*a1 }))))
    }

    /// core.collections.merge_add
    #[no_mangle]
    pub extern "C" fn jet_coll_counter_merge_add(a0: *mut crate::JetCounter, a1: *mut crate::JetCounter) -> *mut crate::JetCounter {
        guard(|| Box::into_raw(Box::new(crate::jet_coll_counter_merge_add(unsafe { &*a0 }, unsafe { &*a1 }))))
    }

    /// core.collections.most_common
    #[no_mangle]
    pub extern "C" fn jet_coll_counter_most_common(a0: *mut crate::JetCounter, a1: i64) -> *mut crate::JetCounter {
        guard(|| Box::into_raw(Box::new(crate::jet_coll_counter_most_common(unsafe { &*a0 }, a1))))
    }

    /// core.collections.names
    #[no_mangle]
    pub extern "C" fn jet_coll_counter_names(a0: *mut crate::JetCounter, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_coll_counter_names(unsafe { &*a0 }).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.collections.set_count
    #[no_mangle]
    pub extern "C" fn jet_coll_counter_set_count(a0: *mut crate::JetCounter, a1: JetCString, a2: i64) -> *mut crate::JetCounter {
        guard(|| Box::into_raw(Box::new(crate::jet_coll_counter_set_count(unsafe { &*a0 }, unsafe { &*a1 }, a2))))
    }

    /// core.collections.subtract
    #[no_mangle]
    pub extern "C" fn jet_coll_counter_subtract(a0: *mut crate::JetCounter, a1: *mut crate::JetCounter) -> *mut crate::JetCounter {
        guard(|| Box::into_raw(Box::new(crate::jet_coll_counter_subtract(unsafe { &*a0 }, unsafe { &*a1 }))))
    }

    /// core.collections.total
    #[no_mangle]
    pub extern "C" fn jet_coll_counter_total(a0: *mut crate::JetCounter) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_coll_counter_total(unsafe { &*a0 })))
    }

    /// core.collections.map_contains
    #[no_mangle]
    pub extern "C" fn jet_coll_map_contains(a0: *mut crate::JetOrderedMap, a1: JetCString) -> bool {
        guard(|| crate::jet_coll_map_contains(unsafe { &*a0 }, unsafe { &*a1 }))
    }

    /// core.collections.map_get
    #[no_mangle]
    pub extern "C" fn jet_coll_map_get(a0: *mut crate::JetOrderedMap, a1: JetCString, some: *mut JetCString) -> i64 {
        guard(|| match crate::jet_coll_map_get(unsafe { &*a0 }, unsafe { &*a1 }) {
            Some(value) => { unsafe { some.write(handle(value)) }; 1 }
            None => 0,
        })
    }

    /// core.collections.map_keys
    #[no_mangle]
    pub extern "C" fn jet_coll_map_keys(a0: *mut crate::JetOrderedMap, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_coll_map_keys(unsafe { &*a0 }).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.collections.map_len
    #[no_mangle]
    pub extern "C" fn jet_coll_map_len(a0: *mut crate::JetOrderedMap) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_coll_map_len(unsafe { &*a0 })))
    }

    /// core.collections.map_remove
    #[no_mangle]
    pub extern "C" fn jet_coll_map_remove(a0: *mut crate::JetOrderedMap, a1: JetCString) -> *mut crate::JetOrderedMap {
        guard(|| Box::into_raw(Box::new(crate::jet_coll_map_remove(unsafe { &*a0 }, unsafe { &*a1 }))))
    }

    /// core.collections.map_set
    #[no_mangle]
    pub extern "C" fn jet_coll_map_set(a0: *mut crate::JetOrderedMap, a1: JetCString, a2: JetCString) -> *mut crate::JetOrderedMap {
        guard(|| Box::into_raw(Box::new(crate::jet_coll_map_set(unsafe { &*a0 }, unsafe { &*a1 }, unsafe { &*a2 }))))
    }

    /// core.collections.map_values
    #[no_mangle]
    pub extern "C" fn jet_coll_map_values(a0: *mut crate::JetOrderedMap, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_coll_map_values(unsafe { &*a0 }).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.collections.ordered_map
    #[no_mangle]
    pub extern "C" fn jet_coll_ordered_map() -> *mut crate::JetOrderedMap {
        guard(|| Box::into_raw(Box::new(crate::jet_coll_ordered_map())))
    }

    /// core.crypto.__zeroize
    #[no_mangle]
    pub extern "C" fn jet_crypto_zeroize(a0_0: *const u64, a0_1: i64) {
        guard(|| crate::jet_crypto_zeroize(list_in(a0_0, a0_1, |w| w as u8)))
    }

    /// core.handle.cursor.over
    #[no_mangle]
    pub extern "C" fn jet_cursor_over(a0: JetCString) -> *mut crate::JetCursor {
        guard(|| Box::into_raw(Box::new(crate::jet_cursor_over(unsafe { &*a0 }))))
    }

    /// core.math.decimal
    #[no_mangle]
    pub extern "C" fn jet_decimal_from_str(a0: JetCString) -> *mut crate::jet_std::JetDecimal {
        guard(|| Box::into_raw(Box::new(crate::jet_decimal_from_str(unsafe { &*a0 }))))
    }

    /// core.handle.duration.abs
    #[no_mangle]
    pub extern "C" fn jet_duration_abs(a0: *mut crate::jet_std::Duration) -> *mut crate::jet_std::Duration {
        guard(|| Box::into_raw(Box::new(crate::jet_duration_abs(unsafe { &*a0 }))))
    }

    /// core.handle.duration.difference
    #[no_mangle]
    pub extern "C" fn jet_duration_difference(a0: *mut crate::jet_std::Duration, a1: *mut crate::jet_std::Duration) -> *mut crate::jet_std::Duration {
        guard(|| Box::into_raw(Box::new(crate::jet_duration_difference(unsafe { &*a0 }, unsafe { &*a1 }))))
    }

    /// core.time.duration_new
    #[no_mangle]
    pub extern "C" fn jet_duration_from_float(a0: f64, a1: i64, ok: *mut *mut crate::jet_std::Duration, err: *mut *mut crate::jet_std::RangeError) -> i64 {
        guard(|| match crate::jet_duration_from_float(a0, match a1 { 0 => crate::jet_std::DurationUnit::Nanoseconds, 1 => crate::jet_std::DurationUnit::Microseconds, 2 => crate::jet_std::DurationUnit::Milliseconds, 3 => crate::jet_std::DurationUnit::Seconds, 4 => crate::jet_std::DurationUnit::Minutes, 5 => crate::jet_std::DurationUnit::Hours, _ => range_stop() }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.time.duration_new
    #[no_mangle]
    pub extern "C" fn jet_duration_from_int(a0: i64, a1: i64, ok: *mut *mut crate::jet_std::Duration, err: *mut *mut crate::jet_std::RangeError) -> i64 {
        guard(|| match crate::jet_duration_from_int(unsafe { crate::jet_foundation::Numeric::JetInt::clone_from_raw(a0) }, match a1 { 0 => crate::jet_std::DurationUnit::Nanoseconds, 1 => crate::jet_std::DurationUnit::Microseconds, 2 => crate::jet_std::DurationUnit::Milliseconds, 3 => crate::jet_std::DurationUnit::Seconds, 4 => crate::jet_std::DurationUnit::Minutes, 5 => crate::jet_std::DurationUnit::Hours, _ => range_stop() }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.duration.in
    #[no_mangle]
    pub extern "C" fn jet_duration_in(a0: *mut crate::jet_std::Duration, a1: i64, ok: *mut i64, err: *mut *mut crate::jet_std::RangeError) -> i64 {
        guard(|| match crate::jet_duration_in(unsafe { &*a0 }, &match a1 { 0 => crate::jet_std::DurationUnit::Nanoseconds, 1 => crate::jet_std::DurationUnit::Microseconds, 2 => crate::jet_std::DurationUnit::Milliseconds, 3 => crate::jet_std::DurationUnit::Seconds, 4 => crate::jet_std::DurationUnit::Minutes, 5 => crate::jet_std::DurationUnit::Hours, _ => range_stop() }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.duration.is_zero
    #[no_mangle]
    pub extern "C" fn jet_duration_is_zero(a0: *mut crate::jet_std::Duration) -> bool {
        guard(|| crate::jet_duration_is_zero(unsafe { &*a0 }))
    }

    /// core.handle.duration.negated
    #[no_mangle]
    pub extern "C" fn jet_duration_negated(a0: *mut crate::jet_std::Duration) -> *mut crate::jet_std::Duration {
        guard(|| Box::into_raw(Box::new(crate::jet_duration_negated(unsafe { &*a0 }))))
    }

    /// core.handle.duration.ns_value
    #[no_mangle]
    pub extern "C" fn jet_duration_ns_value(a0: *mut crate::jet_std::Duration) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_duration_ns_value(unsafe { &*a0 })))
    }

    /// core.handle.duration.seconds_value
    #[no_mangle]
    pub extern "C" fn jet_duration_seconds_value(a0: *mut crate::jet_std::Duration) -> f64 {
        guard(|| crate::jet_duration_seconds_value(unsafe { &*a0 }))
    }

    /// core.handle.duration.sign
    #[no_mangle]
    pub extern "C" fn jet_duration_sign(a0: *mut crate::jet_std::Duration) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_duration_sign(unsafe { &*a0 })))
    }

    /// core.handle.duration.total_in
    #[no_mangle]
    pub extern "C" fn jet_duration_total_in(a0: *mut crate::jet_std::Duration, a1: JetCString) -> f64 {
        guard(|| crate::jet_duration_total_in(unsafe { &*a0 }, unsafe { &*a1 }))
    }

    /// core.handle.duration.total_seconds
    #[no_mangle]
    pub extern "C" fn jet_duration_total_seconds(a0: *mut crate::jet_std::Duration) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_duration_total_seconds(unsafe { &*a0 })))
    }

    /// core.errors.from_message
    #[no_mangle]
    pub extern "C" fn jet_err_from_message(a0: JetCString) -> *mut crate::JetErr {
        guard(|| Box::into_raw(Box::new(crate::jet_err_from_message(view(a0).to_owned()))))
    }

    /// core.handle.fake.locale
    #[no_mangle]
    pub extern "C" fn jet_fake_locale(a0: *mut crate::jet_std::Fake, a1: JetCString) -> *mut crate::jet_std::Fake {
        guard(|| Box::into_raw(Box::new(crate::jet_fake_locale(unsafe { &*a0 }, unsafe { &*a1 }))))
    }

    /// core.text.fmt.bin
    #[no_mangle]
    pub extern "C" fn jet_fmt_bin(a0: i64) -> JetCString {
        guard(|| handle(crate::jet_fmt_bin(a0)))
    }

    /// core.text.fmt.bytes
    #[no_mangle]
    pub extern "C" fn jet_fmt_bytes(a0: i64) -> JetCString {
        guard(|| handle(crate::jet_fmt_bytes(a0)))
    }

    /// core.text.fmt.decimal
    #[no_mangle]
    pub extern "C" fn jet_fmt_decimal(a0: f64, a1: i64) -> JetCString {
        guard(|| handle(crate::jet_fmt_decimal(a0, a1)))
    }

    /// core.text.fmt.duration
    #[no_mangle]
    pub extern "C" fn jet_fmt_duration(a0: i64) -> JetCString {
        guard(|| handle(crate::jet_fmt_duration(a0)))
    }

    /// core.text.fmt.grouped
    #[no_mangle]
    pub extern "C" fn jet_fmt_grouped(a0: f64, a1: i64) -> JetCString {
        guard(|| handle(crate::jet_fmt_grouped(a0, a1)))
    }

    /// core.text.fmt.hex
    #[no_mangle]
    pub extern "C" fn jet_fmt_hex(a0: i64, a1: i64) -> JetCString {
        guard(|| handle(crate::jet_fmt_hex(a0, a1)))
    }

    /// core.text.fmt.number
    #[no_mangle]
    pub extern "C" fn jet_fmt_number(a0: i64) -> JetCString {
        guard(|| handle(crate::jet_fmt_number(a0)))
    }

    /// core.text.fmt.oct
    #[no_mangle]
    pub extern "C" fn jet_fmt_oct(a0: i64) -> JetCString {
        guard(|| handle(crate::jet_fmt_oct(a0)))
    }

    /// core.text.fmt.ordinal
    #[no_mangle]
    pub extern "C" fn jet_fmt_ordinal(a0: i64) -> JetCString {
        guard(|| handle(crate::jet_fmt_ordinal(a0)))
    }

    /// core.text.fmt.pad
    #[no_mangle]
    pub extern "C" fn jet_fmt_pad(a0: JetCString, a1: i64, a2: JetCString) -> JetCString {
        guard(|| handle(crate::jet_fmt_pad(unsafe { &*a0 }, a1, unsafe { &*a2 })))
    }

    /// core.text.fmt.pad_center
    #[no_mangle]
    pub extern "C" fn jet_fmt_pad_center(a0: JetCString, a1: i64, a2: JetCString) -> JetCString {
        guard(|| handle(crate::jet_fmt_pad_center(unsafe { &*a0 }, a1, unsafe { &*a2 })))
    }

    /// core.text.fmt.pad_left
    #[no_mangle]
    pub extern "C" fn jet_fmt_pad_left(a0: JetCString, a1: i64, a2: JetCString) -> JetCString {
        guard(|| handle(crate::jet_fmt_pad_left(unsafe { &*a0 }, a1, unsafe { &*a2 })))
    }

    /// core.text.fmt.pad_right
    #[no_mangle]
    pub extern "C" fn jet_fmt_pad_right(a0: JetCString, a1: i64, a2: JetCString) -> JetCString {
        guard(|| handle(crate::jet_fmt_pad_right(unsafe { &*a0 }, a1, unsafe { &*a2 })))
    }

    /// core.text.fmt.percent
    #[no_mangle]
    pub extern "C" fn jet_fmt_percent(a0: f64, a1: i64) -> JetCString {
        guard(|| handle(crate::jet_fmt_percent(a0, a1)))
    }

    /// core.text.fmt.plural
    #[no_mangle]
    pub extern "C" fn jet_fmt_plural(a0: i64, a1: JetCString, a2: JetCString) -> JetCString {
        guard(|| handle(crate::jet_fmt_plural(a0, unsafe { &*a1 }, unsafe { &*a2 })))
    }

    /// core.text.fmt.pretty
    #[no_mangle]
    pub extern "C" fn jet_fmt_pretty(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_fmt_pretty(view(a0))))
    }

    /// core.text.fmt.quantity
    #[no_mangle]
    pub extern "C" fn jet_fmt_quantity(a0: f64) -> JetCString {
        guard(|| handle(crate::jet_fmt_quantity(a0)))
    }

    /// core.text.fmt.sci
    #[no_mangle]
    pub extern "C" fn jet_fmt_sci(a0: f64, a1: i64) -> JetCString {
        guard(|| handle(crate::jet_fmt_sci(a0, a1)))
    }

    /// core.math.fraction
    #[no_mangle]
    pub extern "C" fn jet_fraction_new(a0: i64, a1: i64, some: *mut *mut crate::jet_std::JetFraction) -> i64 {
        guard(|| match crate::jet_fraction_new(a0, a1) {
            Some(value) => { unsafe { some.write(Box::into_raw(Box::new(value))) }; 1 }
            None => 0,
        })
    }

    /// core.numeric.inline_range, core.numeric.inline_range
    #[no_mangle]
    pub extern "C" fn jet_inline_range_from_int(a0: i64, a1: i64, a2: i64, ok: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_inline_range_from_int(a0, a1, a2) {
            Ok(value) => { unsafe { ok.write(value) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.errors.journey_frame_text
    #[no_mangle]
    pub extern "C" fn jet_journey_frame_text(a0: JetCString, a1: i64, a2: i64, a3: JetCString, a4: JetCString) {
        guard(|| crate::jet_journey_frame_text(view(a0), fixed::<u32>(a1), fixed::<u32>(a2), view(a3), view(a4)))
    }

    /// core.errors.journey_origin
    #[no_mangle]
    pub extern "C" fn jet_journey_origin(a0: JetCString, a1: i64, a2: i64, a3: JetCString) {
        guard(|| crate::jet_journey_origin(view(a0), fixed::<u32>(a1), fixed::<u32>(a2), view(a3)))
    }

    /// core.errors.journey_reset
    #[no_mangle]
    pub extern "C" fn jet_journey_reset() {
        guard(|| crate::jet_journey_reset())
    }

    /// core.memo.stats
    #[no_mangle]
    pub extern "C" fn jet_memo_stats(a0: JetCString, a1: i64) -> *mut crate::JetMemoStats {
        guard(|| Box::into_raw(Box::new(crate::jet_memo_stats(view(a0).to_owned(), a1))))
    }

    /// core.net.mime.extension
    #[no_mangle]
    pub extern "C" fn jet_mime_extension(a0: JetCString, some: *mut JetCString) -> i64 {
        guard(|| match crate::jet_mime_extension(unsafe { &*a0 }) {
            Some(value) => { unsafe { some.write(handle(value)) }; 1 }
            None => 0,
        })
    }

    /// core.net.mime.from_extension
    #[no_mangle]
    pub extern "C" fn jet_mime_from_extension(a0: JetCString, some: *mut JetCString) -> i64 {
        guard(|| match crate::jet_mime_from_extension(unsafe { &*a0 }) {
            Some(value) => { unsafe { some.write(handle(value)) }; 1 }
            None => 0,
        })
    }

    /// core.net.mime.parse
    #[no_mangle]
    pub extern "C" fn jet_mime_parse(a0: JetCString, ok: *mut *mut crate::jet_std::JetMIME, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_mime_parse(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.net.dns_srv_port
    #[no_mangle]
    pub extern "C" fn jet_net_dns_srv_port(a0: *mut crate::JetDNSSrv) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_net_dns_srv_port(unsafe { &*a0 })))
    }

    /// core.net.dns_srv_priority
    #[no_mangle]
    pub extern "C" fn jet_net_dns_srv_priority(a0: *mut crate::JetDNSSrv) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_net_dns_srv_priority(unsafe { &*a0 })))
    }

    /// core.net.dns_srv_target
    #[no_mangle]
    pub extern "C" fn jet_net_dns_srv_target(a0: *mut crate::JetDNSSrv) -> JetCString {
        guard(|| handle(crate::jet_net_dns_srv_target(unsafe { &*a0 })))
    }

    /// core.net.dns_srv_weight
    #[no_mangle]
    pub extern "C" fn jet_net_dns_srv_weight(a0: *mut crate::JetDNSSrv) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_net_dns_srv_weight(unsafe { &*a0 })))
    }

    /// core.net.error_address
    #[no_mangle]
    pub extern "C" fn jet_net_error_address(a0: *mut crate::JetNetError, some: *mut JetCString) -> i64 {
        guard(|| match crate::jet_net_error_address(unsafe { &*a0 }) {
            Some(value) => { unsafe { some.write(handle(value)) }; 1 }
            None => 0,
        })
    }

    /// core.net.error_message
    #[no_mangle]
    pub extern "C" fn jet_net_error_message(a0: *mut crate::JetNetError) -> JetCString {
        guard(|| handle(crate::jet_net_error_message(unsafe { &*a0 })))
    }

    /// core.net.error_name
    #[no_mangle]
    pub extern "C" fn jet_net_error_name(a0: *mut crate::JetNetError, some: *mut JetCString) -> i64 {
        guard(|| match crate::jet_net_error_name(unsafe { &*a0 }) {
            Some(value) => { unsafe { some.write(handle(value)) }; 1 }
            None => 0,
        })
    }

    /// core.net.error_operation
    #[no_mangle]
    pub extern "C" fn jet_net_error_operation(a0: *mut crate::JetNetError) -> JetCString {
        guard(|| handle(crate::jet_net_error_operation(unsafe { &*a0 })))
    }

    /// core.net.error_os_code
    #[no_mangle]
    pub extern "C" fn jet_net_error_os_code(a0: *mut crate::JetNetError, some: *mut i64) -> i64 {
        guard(|| match crate::jet_net_error_os_code(unsafe { &*a0 }) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.net.getservbyname
    #[no_mangle]
    pub extern "C" fn jet_net_getservbyname(a0: JetCString, ok: *mut i64, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_getservbyname(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.getservbyport
    #[no_mangle]
    pub extern "C" fn jet_net_getservbyport(a0: i64, ok: *mut JetCString, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_getservbyport(a0) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.ip_addr
    #[no_mangle]
    pub extern "C" fn jet_net_ip_addr(a0: JetCString, ok: *mut *mut crate::JetIpAddr, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_ip_addr(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.ip_is_ipv4
    #[no_mangle]
    pub extern "C" fn jet_net_ip_is_ipv4(a0: *mut crate::JetIpAddr) -> bool {
        guard(|| crate::jet_net_ip_is_ipv4(unsafe { &*a0 }))
    }

    /// core.net.ip_to_string
    #[no_mangle]
    pub extern "C" fn jet_net_ip_to_string(a0: *mut crate::JetIpAddr) -> JetCString {
        guard(|| handle(crate::jet_net_ip_to_string(unsafe { &*a0 })))
    }

    /// core.handle.tcp_listener.local_addr
    #[no_mangle]
    pub extern "C" fn jet_net_listener_local_addr(a0: *mut crate::JetTCPListener, ok: *mut JetCString, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_listener_local_addr(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.listener_local_socket_addr
    #[no_mangle]
    pub extern "C" fn jet_net_listener_local_socket_addr(a0: *mut crate::JetTCPListener, ok: *mut *mut crate::JetSocketAddr, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_listener_local_socket_addr(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.nodelay
    #[no_mangle]
    pub extern "C" fn jet_net_nodelay(a0: *mut crate::JetTCPStream, ok: *mut bool, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_nodelay(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(value) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.ready_readable
    #[no_mangle]
    pub extern "C" fn jet_net_ready_readable(a0: *mut crate::JetNetReady) -> bool {
        guard(|| crate::jet_net_ready_readable(unsafe { &*a0 }))
    }

    /// core.net.ready_writable
    #[no_mangle]
    pub extern "C" fn jet_net_ready_writable(a0: *mut crate::JetNetReady) -> bool {
        guard(|| crate::jet_net_ready_writable(unsafe { &*a0 }))
    }

    /// core.net.set_nodelay
    #[no_mangle]
    pub extern "C" fn jet_net_set_nodelay(a0: *mut crate::JetTCPStream, a1: u8, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_set_nodelay(unsafe { &*a0 }, a1 != 0) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.set_ttl
    #[no_mangle]
    pub extern "C" fn jet_net_set_ttl(a0: *mut crate::JetTCPStream, a1: i64, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_set_ttl(unsafe { &*a0 }, a1) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.socket_addr
    #[no_mangle]
    pub extern "C" fn jet_net_socket_addr(a0: JetCString, a1: i64, ok: *mut *mut crate::JetSocketAddr, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_socket_addr(unsafe { &*a0 }, a1) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.socket_addr_parse
    #[no_mangle]
    pub extern "C" fn jet_net_socket_addr_parse(a0: JetCString, ok: *mut *mut crate::JetSocketAddr, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_socket_addr_parse(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.socket_host
    #[no_mangle]
    pub extern "C" fn jet_net_socket_host(a0: *mut crate::JetSocketAddr) -> JetCString {
        guard(|| handle(crate::jet_net_socket_host(unsafe { &*a0 })))
    }

    /// core.net.socket_port
    #[no_mangle]
    pub extern "C" fn jet_net_socket_port(a0: *mut crate::JetSocketAddr) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_net_socket_port(unsafe { &*a0 })))
    }

    /// core.net.socket_to_string
    #[no_mangle]
    pub extern "C" fn jet_net_socket_to_string(a0: *mut crate::JetSocketAddr) -> JetCString {
        guard(|| handle(crate::jet_net_socket_to_string(unsafe { &*a0 })))
    }

    /// core.net.socket_type
    #[no_mangle]
    pub extern "C" fn jet_net_socket_type(a0: *mut crate::JetTCPStream) -> JetCString {
        guard(|| handle(crate::jet_net_socket_type(unsafe { &*a0 })))
    }

    /// core.net.tcp_accept, core.handle.tcp_listener.accept
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_accept(a0: *mut crate::JetTCPListener, ok: *mut *mut crate::JetTCPStream, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_accept(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tcp_listener.accept_deadline
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_accept_deadline(a0: *mut crate::JetTCPListener, a1: *mut crate::jet_std::Duration, ok: *mut *mut crate::JetTCPStream, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_accept_deadline(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.tcp_connect
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_connect(a0: JetCString, ok: *mut *mut crate::JetTCPStream, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_connect(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.tcp_connect_addr
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_connect_addr(a0: *mut crate::JetSocketAddr, ok: *mut *mut crate::JetTCPStream, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_connect_addr(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.tcp_connect_happy
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_connect_happy(a0: JetCString, a1: i64, a2: i64, ok: *mut *mut crate::JetTCPStream, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_connect_happy(unsafe { &*a0 }, a1, a2) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.tcp_connect_timeout
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_connect_timeout(a0: *mut crate::JetSocketAddr, a1: i64, ok: *mut *mut crate::JetTCPStream, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_connect_timeout(unsafe { &*a0 }, a1) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.tcp_listen
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_listen(a0: JetCString, ok: *mut *mut crate::JetTCPListener, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_listen(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.tcp_listen_addr
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_listen_addr(a0: *mut crate::JetSocketAddr, ok: *mut *mut crate::JetTCPListener, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_listen_addr(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.tcp_local_addr, core.handle.tcp_stream.local_addr
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_local_addr(a0: *mut crate::JetTCPStream, ok: *mut JetCString, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_local_addr(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.tcp_local_socket_addr
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_local_socket_addr(a0: *mut crate::JetTCPStream, ok: *mut *mut crate::JetSocketAddr, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_local_socket_addr(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.tcp_peer_addr, core.handle.tcp_stream.peer_addr
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_peer_addr(a0: *mut crate::JetTCPStream, ok: *mut JetCString, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_peer_addr(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.tcp_peer_socket_addr
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_peer_socket_addr(a0: *mut crate::JetTCPStream, ok: *mut *mut crate::JetSocketAddr, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_peer_socket_addr(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tls_stream.peer_identity
    #[no_mangle]
    pub extern "C" fn jet_net_tls_peer_identity(a0: *mut crate::JetTLSStream) -> *mut crate::JetTLSPeerIdentity {
        guard(|| Box::into_raw(Box::new(crate::jet_net_tls_peer_identity(unsafe { &*a0 }))))
    }

    /// core.handle.tls_stream.ready
    #[no_mangle]
    pub extern "C" fn jet_net_tls_ready(a0: *mut crate::JetTLSStream, a1: i64, a2: *mut crate::jet_std::Duration, ok: *mut *mut crate::JetNetReady, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_net_tls_ready(unsafe { &*a0 }, match a1 { 0 => crate::JetNetReadyInterest::Read, 1 => crate::JetNetReadyInterest::Write, 2 => crate::JetNetReadyInterest::ReadWrite, _ => range_stop() }, unsafe { &*a2 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.ttl
    #[no_mangle]
    pub extern "C" fn jet_net_ttl(a0: *mut crate::JetTCPStream, ok: *mut i64, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_ttl(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.udp_bind
    #[no_mangle]
    pub extern "C" fn jet_net_udp_bind(a0: JetCString, ok: *mut *mut crate::JetUDPSocket, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_udp_bind(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.udp_bind_addr
    #[no_mangle]
    pub extern "C" fn jet_net_udp_bind_addr(a0: *mut crate::JetSocketAddr, ok: *mut *mut crate::JetUDPSocket, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_udp_bind_addr(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.udp_socket.close
    #[no_mangle]
    pub extern "C" fn jet_net_udp_close(a0: *mut crate::JetUDPSocket, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_udp_close(unsafe { &*a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.udp_local_addr
    #[no_mangle]
    pub extern "C" fn jet_net_udp_local_addr(a0: *mut crate::JetUDPSocket, ok: *mut *mut crate::JetSocketAddr, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_udp_local_addr(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.udp_packet_addr
    #[no_mangle]
    pub extern "C" fn jet_net_udp_packet_addr(a0: *mut crate::JetUDPPacket) -> *mut crate::JetSocketAddr {
        guard(|| Box::into_raw(Box::new(crate::jet_net_udp_packet_addr(unsafe { &*a0 }))))
    }

    /// core.net.udp_packet_bytes
    #[no_mangle]
    pub extern "C" fn jet_net_udp_packet_bytes(a0: *mut crate::JetUDPPacket, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_net_udp_packet_bytes(unsafe { &*a0 }).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.net.udp_packet_data
    #[no_mangle]
    pub extern "C" fn jet_net_udp_packet_data(a0: *mut crate::JetUDPPacket) -> JetCString {
        guard(|| handle(crate::jet_net_udp_packet_data(unsafe { &*a0 })))
    }

    /// core.net.udp_packet_original_len
    #[no_mangle]
    pub extern "C" fn jet_net_udp_packet_original_len(a0: *mut crate::JetUDPPacket) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_net_udp_packet_original_len(unsafe { &*a0 })))
    }

    /// core.net.udp_packet_truncated
    #[no_mangle]
    pub extern "C" fn jet_net_udp_packet_truncated(a0: *mut crate::JetUDPPacket) -> bool {
        guard(|| crate::jet_net_udp_packet_truncated(unsafe { &*a0 }))
    }

    /// core.handle.udp_socket.ready
    #[no_mangle]
    pub extern "C" fn jet_net_udp_ready(a0: *mut crate::JetUDPSocket, a1: i64, a2: *mut crate::jet_std::Duration, ok: *mut *mut crate::JetNetReady, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_udp_ready(unsafe { &*a0 }, match a1 { 0 => crate::JetNetReadyInterest::Read, 1 => crate::JetNetReadyInterest::Write, 2 => crate::JetNetReadyInterest::ReadWrite, _ => range_stop() }, unsafe { &*a2 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.udp_receive
    #[no_mangle]
    pub extern "C" fn jet_net_udp_receive(a0: *mut crate::JetUDPSocket, a1: i64, ok: *mut *mut crate::JetUDPPacket, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_udp_receive(unsafe { &*a0 }, a1) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.udp_socket.receive_deadline
    #[no_mangle]
    pub extern "C" fn jet_net_udp_receive_deadline(a0: *mut crate::JetUDPSocket, a1: i64, a2: *mut crate::jet_std::Duration, ok: *mut *mut crate::JetUDPPacket, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_udp_receive_deadline(unsafe { &*a0 }, a1, unsafe { &*a2 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.udp_recv_from
    #[no_mangle]
    pub extern "C" fn jet_net_udp_recv_from(a0: *mut crate::JetUDPSocket, a1: i64, ok: *mut *mut crate::JetUDPPacket, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_udp_recv_from(unsafe { &*a0 }, a1) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.udp_send_bytes_to
    #[no_mangle]
    pub extern "C" fn jet_net_udp_send_bytes_to(a0: *mut crate::JetUDPSocket, a1_0: *const u64, a1_1: i64, a2: *mut crate::JetSocketAddr, ok: *mut i64, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_udp_send_bytes_to(unsafe { &*a0 }, &list_in(a1_0, a1_1, |w| w as u8), unsafe { &*a2 }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.udp_socket.send_to_deadline
    #[no_mangle]
    pub extern "C" fn jet_net_udp_send_bytes_to_deadline(a0: *mut crate::JetUDPSocket, a1_0: *const u64, a1_1: i64, a2: *mut crate::JetSocketAddr, a3: *mut crate::jet_std::Duration, ok: *mut i64, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_udp_send_bytes_to_deadline(unsafe { &*a0 }, &list_in(a1_0, a1_1, |w| w as u8), unsafe { &*a2 }, unsafe { &*a3 }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.udp_send_to
    #[no_mangle]
    pub extern "C" fn jet_net_udp_send_to(a0: *mut crate::JetUDPSocket, a1: JetCString, a2: *mut crate::JetSocketAddr, ok: *mut i64, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_udp_send_to(unsafe { &*a0 }, unsafe { &*a1 }, unsafe { &*a2 }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.udp_set_timeout
    #[no_mangle]
    pub extern "C" fn jet_net_udp_set_timeout(a0: *mut crate::JetUDPSocket, a1: i64, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_udp_set_timeout(unsafe { &*a0 }, a1) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.numeric.checked_widen
    #[no_mangle]
    pub extern "C" fn jet_numeric_checked_widen_at(a0: i64, a1: u8, a2: u8, a3: JetCString, a4: i64) -> f64 {
        guard(|| crate::jet_numeric_checked_widen_at(fixed::<u64>(a0), a1 != 0, a2 != 0, view(a3), fixed::<u32>(a4)))
    }

    /// core.game.raylib.begin_drawing
    #[no_mangle]
    pub extern "C" fn jet_raylib_begin_drawing(a0: *mut crate::RaylibWindow) {
        guard(|| crate::jet_raylib_begin_drawing(unsafe { &*a0 }))
    }

    /// core.game.raylib.clear_background
    #[no_mangle]
    pub extern "C" fn jet_raylib_clear_background(a0: *mut crate::RaylibColor) {
        guard(|| crate::jet_raylib_clear_background(unsafe { &*a0 }))
    }

    /// core.game.raylib.close_window
    #[no_mangle]
    pub extern "C" fn jet_raylib_close_window(a0: *mut crate::RaylibWindow) {
        guard(|| crate::jet_raylib_close_window(unsafe { &*a0 }))
    }

    /// core.game.raylib.color
    #[no_mangle]
    pub extern "C" fn jet_raylib_color(a0: i64, a1: i64, a2: i64, a3: i64) -> *mut crate::RaylibColor {
        guard(|| Box::into_raw(Box::new(crate::jet_raylib_color(a0, a1, a2, a3))))
    }

    /// core.game.raylib.draw_rectangle
    #[no_mangle]
    pub extern "C" fn jet_raylib_draw_rectangle(a0: i64, a1: i64, a2: i64, a3: i64, a4: *mut crate::RaylibColor) {
        guard(|| crate::jet_raylib_draw_rectangle(a0, a1, a2, a3, unsafe { &*a4 }))
    }

    /// core.game.raylib.draw_sprite
    #[no_mangle]
    pub extern "C" fn jet_raylib_draw_sprite(a0: *mut crate::RaylibTextureAtlas, a1: JetCString, a2: i64, a3: i64) {
        guard(|| crate::jet_raylib_draw_sprite(unsafe { &*a0 }, unsafe { &*a1 }, a2, a3))
    }

    /// core.game.raylib.draw_text
    #[no_mangle]
    pub extern "C" fn jet_raylib_draw_text(a0: JetCString, a1: i64, a2: i64, a3: i64, a4: *mut crate::RaylibColor) {
        guard(|| crate::jet_raylib_draw_text(unsafe { &*a0 }, a1, a2, a3, unsafe { &*a4 }))
    }

    /// core.game.raylib.end_drawing
    #[no_mangle]
    pub extern "C" fn jet_raylib_end_drawing() {
        guard(|| crate::jet_raylib_end_drawing())
    }

    /// core.game.raylib.gamepad_axis
    #[no_mangle]
    pub extern "C" fn jet_raylib_gamepad_axis(a0: i64, a1: JetCString) -> f64 {
        guard(|| crate::jet_raylib_gamepad_axis(a0, unsafe { &*a1 }))
    }

    /// core.game.raylib.gamepad_down
    #[no_mangle]
    pub extern "C" fn jet_raylib_gamepad_down(a0: i64, a1: JetCString) -> bool {
        guard(|| crate::jet_raylib_gamepad_down(a0, unsafe { &*a1 }))
    }

    /// core.game.raylib.key_down
    #[no_mangle]
    pub extern "C" fn jet_raylib_key_down(a0: JetCString) -> bool {
        guard(|| crate::jet_raylib_key_down(unsafe { &*a0 }))
    }

    /// core.game.raylib.load_sound
    #[no_mangle]
    pub extern "C" fn jet_raylib_load_sound(a0: JetCString) -> *mut crate::RaylibSound {
        guard(|| Box::into_raw(Box::new(crate::jet_raylib_load_sound(unsafe { &*a0 }))))
    }

    /// core.game.raylib.load_texture_atlas
    #[no_mangle]
    pub extern "C" fn jet_raylib_load_texture_atlas(a0: JetCString) -> *mut crate::RaylibTextureAtlas {
        guard(|| Box::into_raw(Box::new(crate::jet_raylib_load_texture_atlas(unsafe { &*a0 }))))
    }

    /// core.game.raylib.play_sound
    #[no_mangle]
    pub extern "C" fn jet_raylib_play_sound(a0: *mut crate::RaylibSound) -> bool {
        guard(|| crate::jet_raylib_play_sound(unsafe { &*a0 }))
    }

    /// core.game.raylib.set_target_fps
    #[no_mangle]
    pub extern "C" fn jet_raylib_set_target_fps(a0: i64) {
        guard(|| crate::jet_raylib_set_target_fps(a0))
    }

    /// core.game.raylib.window_open
    #[no_mangle]
    pub extern "C" fn jet_raylib_window_open(a0: i64, a1: i64, a2: JetCString) -> *mut crate::RaylibWindow {
        guard(|| Box::into_raw(Box::new(crate::jet_raylib_window_open(a0, a1, unsafe { &*a2 }))))
    }

    /// core.game.raylib.window_ready
    #[no_mangle]
    pub extern "C" fn jet_raylib_window_ready(a0: *mut crate::RaylibWindow) -> bool {
        guard(|| crate::jet_raylib_window_ready(unsafe { &*a0 }))
    }

    /// core.game.raylib.window_should_close
    #[no_mangle]
    pub extern "C" fn jet_raylib_window_should_close(a0: *mut crate::RaylibWindow) -> bool {
        guard(|| crate::jet_raylib_window_should_close(unsafe { &*a0 }))
    }

    /// core.handle.reader.at_end
    #[no_mangle]
    pub extern "C" fn jet_reader_at_end(a0: *mut crate::JetReader) -> bool {
        guard(|| crate::jet_reader_at_end(unsafe { &*a0 }))
    }

    /// core.handle.reader.over
    #[no_mangle]
    pub extern "C" fn jet_reader_over(a0_0: *const u64, a0_1: i64) -> *mut crate::JetReader {
        guard(|| Box::into_raw(Box::new(crate::jet_reader_over(&list_in(a0_0, a0_1, |w| w as u8)))))
    }

    /// core.handle.reader.over
    #[no_mangle]
    pub extern "C" fn jet_reader_over_owned(a0_0: *const u64, a0_1: i64) -> *mut crate::JetReader {
        guard(|| Box::into_raw(Box::new(crate::jet_reader_over_owned(list_in(a0_0, a0_1, |w| w as u8)))))
    }

    /// core.handle.reader.peek
    #[no_mangle]
    pub extern "C" fn jet_reader_peek(a0: *mut crate::JetReader, ok: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_reader_peek(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(i64::try_from(value).unwrap_or_else(|_| range_stop()))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.handle.reader.remaining
    #[no_mangle]
    pub extern "C" fn jet_reader_remaining(a0: *mut crate::JetReader) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_reader_remaining(unsafe { &*a0 })))
    }

    /// core.log.bool
    #[no_mangle]
    pub extern "C" fn jet_ring_log_bool(a0: JetCString, a1: u8) -> *mut crate::jet_std::LogField {
        guard(|| Box::into_raw(Box::new(crate::jet_ring_log_bool(unsafe { &*a0 }, a1 != 0))))
    }

    /// core.log.close
    #[no_mangle]
    pub extern "C" fn jet_ring_log_close(a0: *mut crate::jet_std::LogSpan) {
        guard(|| crate::jet_ring_log_close(unsafe { &*a0 }))
    }

    /// core.log.counter
    #[no_mangle]
    pub extern "C" fn jet_ring_log_counter(a0: JetCString, a1: i64) -> *mut crate::jet_std::LogField {
        guard(|| Box::into_raw(Box::new(crate::jet_ring_log_counter(unsafe { &*a0 }, a1))))
    }

    /// core.log.critical
    #[no_mangle]
    pub extern "C" fn jet_ring_log_critical(a0: JetCString) {
        guard(|| crate::jet_ring_log_critical(unsafe { &*a0 }))
    }

    /// core.log.debug
    #[no_mangle]
    pub extern "C" fn jet_ring_log_debug(a0: JetCString) {
        guard(|| crate::jet_ring_log_debug(unsafe { &*a0 }))
    }

    /// core.log.disable
    #[no_mangle]
    pub extern "C" fn jet_ring_log_disable() {
        guard(|| crate::jet_ring_log_disable())
    }

    /// core.log.enabled
    #[no_mangle]
    pub extern "C" fn jet_ring_log_enabled(a0: JetCString) -> bool {
        guard(|| crate::jet_ring_log_enabled(unsafe { &*a0 }))
    }

    /// core.log.enter
    #[no_mangle]
    pub extern "C" fn jet_ring_log_enter(a0: *mut crate::jet_std::LogSpan) {
        guard(|| crate::jet_ring_log_enter(unsafe { &*a0 }))
    }

    /// core.log.error
    #[no_mangle]
    pub extern "C" fn jet_ring_log_error(a0: JetCString) {
        guard(|| crate::jet_ring_log_error(unsafe { &*a0 }))
    }

    /// core.log.fatal
    #[no_mangle]
    pub extern "C" fn jet_ring_log_fatal(a0: JetCString) {
        guard(|| crate::jet_ring_log_fatal(unsafe { &*a0 }))
    }

    /// core.log.field
    #[no_mangle]
    pub extern "C" fn jet_ring_log_field(a0: JetCString, a1: JetCString) -> *mut crate::jet_std::LogField {
        guard(|| Box::into_raw(Box::new(crate::jet_ring_log_field(unsafe { &*a0 }, unsafe { &*a1 }))))
    }

    /// core.log.float
    #[no_mangle]
    pub extern "C" fn jet_ring_log_float(a0: JetCString, a1: f64) -> *mut crate::jet_std::LogField {
        guard(|| Box::into_raw(Box::new(crate::jet_ring_log_float(unsafe { &*a0 }, a1))))
    }

    /// core.log.flush
    #[no_mangle]
    pub extern "C" fn jet_ring_log_flush() {
        guard(|| crate::jet_ring_log_flush())
    }

    /// core.log.info
    #[no_mangle]
    pub extern "C" fn jet_ring_log_info(a0: JetCString) {
        guard(|| crate::jet_ring_log_info(unsafe { &*a0 }))
    }

    /// core.log.int
    #[no_mangle]
    pub extern "C" fn jet_ring_log_int(a0: JetCString, a1: i64) -> *mut crate::jet_std::LogField {
        guard(|| Box::into_raw(Box::new(crate::jet_ring_log_int(unsafe { &*a0 }, a1))))
    }

    /// core.log.otlp_file
    #[no_mangle]
    pub extern "C" fn jet_ring_log_otlp_file(a0: JetCString) {
        guard(|| crate::jet_ring_log_otlp_file(unsafe { &*a0 }))
    }

    /// core.log.redact
    #[no_mangle]
    pub extern "C" fn jet_ring_log_redact(a0: JetCString) -> *mut crate::jet_std::LogField {
        guard(|| Box::into_raw(Box::new(crate::jet_ring_log_redact(unsafe { &*a0 }))))
    }

    /// core.log.sample_every
    #[no_mangle]
    pub extern "C" fn jet_ring_log_sample_every(a0: i64) {
        guard(|| crate::jet_ring_log_sample_every(a0))
    }

    /// core.log.set_level
    #[no_mangle]
    pub extern "C" fn jet_ring_log_set_level(a0: JetCString) {
        guard(|| crate::jet_ring_log_set_level(unsafe { &*a0 }))
    }

    /// core.log.set_sink
    #[no_mangle]
    pub extern "C" fn jet_ring_log_set_sink(a0: JetCString, a1: JetCString) {
        guard(|| crate::jet_ring_log_set_sink(unsafe { &*a0 }, unsafe { &*a1 }))
    }

    /// core.log.set_trace_id
    #[no_mangle]
    pub extern "C" fn jet_ring_log_set_trace_id(a0: JetCString) {
        guard(|| crate::jet_ring_log_set_trace_id(view(a0)))
    }

    /// core.log.setup
    #[no_mangle]
    pub extern "C" fn jet_ring_log_setup(a0: JetCString) {
        guard(|| crate::jet_ring_log_setup(unsafe { &*a0 }))
    }

    /// core.log.span
    #[no_mangle]
    pub extern "C" fn jet_ring_log_span(a0: JetCString) -> *mut crate::jet_std::LogSpan {
        guard(|| Box::into_raw(Box::new(crate::jet_ring_log_span(unsafe { &*a0 }))))
    }

    /// core.log.warn
    #[no_mangle]
    pub extern "C" fn jet_ring_log_warn(a0: JetCString) {
        guard(|| crate::jet_ring_log_warn(unsafe { &*a0 }))
    }

    /// core.handle.solver.failure_count
    #[no_mangle]
    pub extern "C" fn jet_solver_failure_count(a0: *mut crate::jet_std::Solver) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_solver_failure_count(unsafe { &*a0 })))
    }

    /// core.handle.solver.new
    #[no_mangle]
    pub extern "C" fn jet_solver_new(a0: i64) -> *mut crate::jet_std::Solver {
        guard(|| Box::into_raw(Box::new(crate::jet_solver_new(a0))))
    }

    /// core.handle.solver.status
    #[no_mangle]
    pub extern "C" fn jet_solver_status(a0: *mut crate::jet_std::Solver) -> JetCString {
        guard(|| handle(crate::jet_solver_status(unsafe { &*a0 })))
    }

    /// core.encoding.hex.b2a_base64, core.encoding.base64.encode
    #[no_mangle]
    pub extern "C" fn jet_std_b64_encode(a0_0: *const u64, a0_1: i64) -> JetCString {
        guard(|| handle(crate::jet_std_b64_encode(&list_in(a0_0, a0_1, |w| w as u8))))
    }

    /// core.encoding.base64.encodebytes
    #[no_mangle]
    pub extern "C" fn jet_std_b64_encodebytes(a0_0: *const u64, a0_1: i64) -> JetCString {
        guard(|| handle(crate::jet_std_b64_encodebytes(&list_in(a0_0, a0_1, |w| w as u8))))
    }

    /// core.encoding.base64.pad
    #[no_mangle]
    pub extern "C" fn jet_std_b64_pad(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_std_b64_pad(unsafe { &*a0 })))
    }

    /// core.encoding.base64.unpad
    #[no_mangle]
    pub extern "C" fn jet_std_b64_unpad(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_std_b64_unpad(unsafe { &*a0 })))
    }

    /// core.encoding.base64.encode_url
    #[no_mangle]
    pub extern "C" fn jet_std_b64url_encode(a0_0: *const u64, a0_1: i64) -> JetCString {
        guard(|| handle(crate::jet_std_b64url_encode(&list_in(a0_0, a0_1, |w| w as u8))))
    }

    /// core.encoding.base64.encode_url_padded
    #[no_mangle]
    pub extern "C" fn jet_std_b64url_encode_padded(a0_0: *const u64, a0_1: i64) -> JetCString {
        guard(|| handle(crate::jet_std_b64url_encode_padded(&list_in(a0_0, a0_1, |w| w as u8))))
    }

    /// core.encoding.base32.b32encode
    #[no_mangle]
    pub extern "C" fn jet_std_base32_encode(a0_0: *const u64, a0_1: i64) -> JetCString {
        guard(|| handle(crate::jet_std_base32_encode(&list_in(a0_0, a0_1, |w| w as u8))))
    }

    /// core.encoding.base32.b32hexencode
    #[no_mangle]
    pub extern "C" fn jet_std_base32hex_encode(a0_0: *const u64, a0_1: i64) -> JetCString {
        guard(|| handle(crate::jet_std_base32hex_encode(&list_in(a0_0, a0_1, |w| w as u8))))
    }

    /// core.encoding.binary.calcsize
    #[no_mangle]
    pub extern "C" fn jet_std_binary_calcsize(a0: JetCString, ok: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_std_binary_calcsize(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.encoding.binary.pack_f64be
    #[no_mangle]
    pub extern "C" fn jet_std_binary_pack_f64be(a0: f64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_std_binary_pack_f64be(a0).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.encoding.binary.pack_f64le
    #[no_mangle]
    pub extern "C" fn jet_std_binary_pack_f64le(a0: f64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_std_binary_pack_f64le(a0).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.encoding.binary.pack_i8
    #[no_mangle]
    pub extern "C" fn jet_std_binary_pack_i8(a0: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_std_binary_pack_i8(a0).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.encoding.binary.pack_u16be
    #[no_mangle]
    pub extern "C" fn jet_std_binary_pack_u16be(a0: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_std_binary_pack_u16be(a0).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.encoding.binary.pack_u16le
    #[no_mangle]
    pub extern "C" fn jet_std_binary_pack_u16le(a0: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_std_binary_pack_u16le(a0).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.encoding.binary.pack_u32be
    #[no_mangle]
    pub extern "C" fn jet_std_binary_pack_u32be(a0: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_std_binary_pack_u32be(a0).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.encoding.binary.pack_u32le
    #[no_mangle]
    pub extern "C" fn jet_std_binary_pack_u32le(a0: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_std_binary_pack_u32le(a0).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.encoding.binary.pack_u64be
    #[no_mangle]
    pub extern "C" fn jet_std_binary_pack_u64be(a0: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_std_binary_pack_u64be(a0).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.encoding.binary.pack_u64le
    #[no_mangle]
    pub extern "C" fn jet_std_binary_pack_u64le(a0: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_std_binary_pack_u64le(a0).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.encoding.binary.pack_u8
    #[no_mangle]
    pub extern "C" fn jet_std_binary_pack_u8(a0: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_std_binary_pack_u8(a0).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.encoding.binary.sign_extend
    #[no_mangle]
    pub extern "C" fn jet_std_binary_sign_extend(a0: i64, a1: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_binary_sign_extend(a0, a1)))
    }

    /// core.encoding.binary.unpack_f64be
    #[no_mangle]
    pub extern "C" fn jet_std_binary_unpack_f64be(a0_0: *const u64, a0_1: i64, a1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_std_binary_unpack_f64be(&list_in(a0_0, a0_1, |w| w as u8), a1) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.encoding.binary.unpack_f64le
    #[no_mangle]
    pub extern "C" fn jet_std_binary_unpack_f64le(a0_0: *const u64, a0_1: i64, a1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_std_binary_unpack_f64le(&list_in(a0_0, a0_1, |w| w as u8), a1) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.encoding.binary.unpack_u16be
    #[no_mangle]
    pub extern "C" fn jet_std_binary_unpack_u16be(a0_0: *const u64, a0_1: i64, a1: i64, some: *mut i64) -> i64 {
        guard(|| match crate::jet_std_binary_unpack_u16be(&list_in(a0_0, a0_1, |w| w as u8), a1) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.encoding.binary.unpack_u16le
    #[no_mangle]
    pub extern "C" fn jet_std_binary_unpack_u16le(a0_0: *const u64, a0_1: i64, a1: i64, some: *mut i64) -> i64 {
        guard(|| match crate::jet_std_binary_unpack_u16le(&list_in(a0_0, a0_1, |w| w as u8), a1) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.encoding.binary.unpack_u32be
    #[no_mangle]
    pub extern "C" fn jet_std_binary_unpack_u32be(a0_0: *const u64, a0_1: i64, a1: i64, some: *mut i64) -> i64 {
        guard(|| match crate::jet_std_binary_unpack_u32be(&list_in(a0_0, a0_1, |w| w as u8), a1) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.encoding.binary.unpack_u32le
    #[no_mangle]
    pub extern "C" fn jet_std_binary_unpack_u32le(a0_0: *const u64, a0_1: i64, a1: i64, some: *mut i64) -> i64 {
        guard(|| match crate::jet_std_binary_unpack_u32le(&list_in(a0_0, a0_1, |w| w as u8), a1) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.encoding.binary.unpack_u64be
    #[no_mangle]
    pub extern "C" fn jet_std_binary_unpack_u64be(a0_0: *const u64, a0_1: i64, a1: i64, some: *mut i64) -> i64 {
        guard(|| match crate::jet_std_binary_unpack_u64be(&list_in(a0_0, a0_1, |w| w as u8), a1) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.encoding.binary.unpack_u64le
    #[no_mangle]
    pub extern "C" fn jet_std_binary_unpack_u64le(a0_0: *const u64, a0_1: i64, a1: i64, some: *mut i64) -> i64 {
        guard(|| match crate::jet_std_binary_unpack_u64le(&list_in(a0_0, a0_1, |w| w as u8), a1) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.encoding.binary.unpack_u8
    #[no_mangle]
    pub extern "C" fn jet_std_binary_unpack_u8(a0_0: *const u64, a0_1: i64, a1: i64, some: *mut i64) -> i64 {
        guard(|| match crate::jet_std_binary_unpack_u8(&list_in(a0_0, a0_1, |w| w as u8), a1) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.testing.fake_clock, core.time.clock
    #[no_mangle]
    pub extern "C" fn jet_std_clock_new(a0: i64) -> *mut crate::jet_std::Clock {
        guard(|| Box::into_raw(Box::new(crate::jet_std_clock_new(a0))))
    }

    /// core.encoding.hex.crc_hqx
    #[no_mangle]
    pub extern "C" fn jet_std_crc_hqx(a0_0: *const u64, a0_1: i64, a1: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_crc_hqx(&list_in(a0_0, a0_1, |w| w as u8), a1)))
    }

    /// core.encoding.hex.crc32
    #[no_mangle]
    pub extern "C" fn jet_std_crc32(a0_0: *const u64, a0_1: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_crc32(&list_in(a0_0, a0_1, |w| w as u8))))
    }

    /// core.files.append_all
    #[no_mangle]
    pub extern "C" fn jet_std_fs_append(a0: JetCString, a1: JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_append(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.copy
    #[no_mangle]
    pub extern "C" fn jet_std_fs_copy(a0: JetCString, a1: JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_copy(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.copy_dir
    #[no_mangle]
    pub extern "C" fn jet_std_fs_copy_dir(a0: JetCString, a1: JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_copy_dir(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.create_dir
    #[no_mangle]
    pub extern "C" fn jet_std_fs_create_dir(a0: JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_create_dir(unsafe { &*a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.create_dir_all
    #[no_mangle]
    pub extern "C" fn jet_std_fs_create_dir_all(a0: JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_create_dir_all(unsafe { &*a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.exists
    #[no_mangle]
    pub extern "C" fn jet_std_fs_exists(a0: JetCString) -> bool {
        guard(|| crate::jet_std_fs_exists(unsafe { &*a0 }))
    }

    /// core.files.is_dir
    #[no_mangle]
    pub extern "C" fn jet_std_fs_is_dir(a0: JetCString) -> bool {
        guard(|| crate::jet_std_fs_is_dir(unsafe { &*a0 }))
    }

    /// core.files.read
    #[no_mangle]
    pub extern "C" fn jet_std_fs_read(a0: JetCString, ok: *mut JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_read(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.remove
    #[no_mangle]
    pub extern "C" fn jet_std_fs_remove(a0: JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_remove(unsafe { &*a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.remove_all
    #[no_mangle]
    pub extern "C" fn jet_std_fs_remove_all(a0: JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_remove_all(unsafe { &*a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.remove_dir
    #[no_mangle]
    pub extern "C" fn jet_std_fs_remove_dir(a0: JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_remove_dir(unsafe { &*a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.write
    #[no_mangle]
    pub extern "C" fn jet_std_fs_write(a0: JetCString, a1: JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_write(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.encoding.hex.dump
    #[no_mangle]
    pub extern "C" fn jet_std_hex_dump(a0_0: *const u64, a0_1: i64) -> JetCString {
        guard(|| handle(crate::jet_std_hex_dump(&list_in(a0_0, a0_1, |w| w as u8))))
    }

    /// core.encoding.hex.encode
    #[no_mangle]
    pub extern "C" fn jet_std_hex_encode(a0_0: *const u64, a0_1: i64) -> JetCString {
        guard(|| handle(crate::jet_std_hex_encode(&list_in(a0_0, a0_1, |w| w as u8))))
    }

    /// core.encoding.hex.encode_sep
    #[no_mangle]
    pub extern "C" fn jet_std_hex_encode_sep(a0_0: *const u64, a0_1: i64, a1: JetCString) -> JetCString {
        guard(|| handle(crate::jet_std_hex_encode_sep(&list_in(a0_0, a0_1, |w| w as u8), unsafe { &*a1 })))
    }

    /// core.encoding.hex.encode_upper
    #[no_mangle]
    pub extern "C" fn jet_std_hex_encode_upper(a0_0: *const u64, a0_1: i64) -> JetCString {
        guard(|| handle(crate::jet_std_hex_encode_upper(&list_in(a0_0, a0_1, |w| w as u8))))
    }

    /// core.encoding.hex.is_hex
    #[no_mangle]
    pub extern "C" fn jet_std_hex_is_hex(a0: JetCString) -> bool {
        guard(|| crate::jet_std_hex_is_hex(unsafe { &*a0 }))
    }

    /// core.encoding.json.events
    #[no_mangle]
    pub extern "C" fn jet_std_json_events(a0: *mut crate::jet_std::DataTree) -> JetCString {
        guard(|| handle(crate::jet_std_json_events(unsafe { &*a0 })))
    }

    /// core.encoding.json.parse, core.encoding.json.loads
    #[no_mangle]
    pub extern "C" fn jet_std_json_parse(a0: JetCString, ok: *mut *mut crate::jet_std::DataTree, err: *mut *mut crate::jet_std::EncodingError) -> i64 {
        guard(|| match crate::jet_std_json_parse(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.encoding.json.to_string, core.encoding.json.dumps
    #[no_mangle]
    pub extern "C" fn jet_std_json_render(a0: *mut crate::jet_std::DataTree) -> JetCString {
        guard(|| handle(crate::jet_std_json_render(unsafe { &*a0 })))
    }

    /// core.encoding.json.to_string_pretty
    #[no_mangle]
    pub extern "C" fn jet_std_json_render_pretty(a0: *mut crate::jet_std::DataTree) -> JetCString {
        guard(|| handle(crate::jet_std_json_render_pretty(unsafe { &*a0 })))
    }

    /// core.encoding.jsonl.append_line
    #[no_mangle]
    pub extern "C" fn jet_std_jsonl_append_line(a0: JetCString, a1: *mut crate::jet_std::DataTree) -> JetCString {
        guard(|| handle(crate::jet_std_jsonl_append_line(unsafe { &*a0 }, unsafe { &*a1 })))
    }

    /// core.testing.fake_rng, core.math.random.rng
    #[no_mangle]
    pub extern "C" fn jet_std_rng_new(a0: i64) -> *mut crate::jet_std::Rng {
        guard(|| Box::into_raw(Box::new(crate::jet_std_rng_new(a0))))
    }

    /// core.time.now
    #[no_mangle]
    pub extern "C" fn jet_std_time_now() -> i64 {
        guard(|| crate::jet_std_time_now())
    }

    /// core.time.sleep
    #[no_mangle]
    pub extern "C" fn jet_std_time_sleep_duration(a0: *mut crate::jet_std::Duration) {
        guard(|| crate::jet_std_time_sleep_duration(unsafe { &*a0 }))
    }

    /// core.time.start
    #[no_mangle]
    pub extern "C" fn jet_std_time_start() -> *mut crate::jet_std::Stopwatch {
        guard(|| Box::into_raw(Box::new(crate::jet_std_time_start())))
    }

    /// core.encoding.xml.canonical
    #[no_mangle]
    pub extern "C" fn jet_std_xml_canonical(a0: *mut crate::jet_std::DataTree, a1: *mut crate::jet_std::XMLCanonical, ok: *mut JetCString, err: *mut *mut crate::jet_std::XMLError) -> i64 {
        guard(|| match crate::jet_std_xml_canonical(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.encoding.xml.parse
    #[no_mangle]
    pub extern "C" fn jet_std_xml_parse(a0: JetCString, ok: *mut *mut crate::jet_std::DataTree, err: *mut *mut crate::jet_std::XMLError) -> i64 {
        guard(|| match crate::jet_std_xml_parse(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.encoding.xml.parse_with
    #[no_mangle]
    pub extern "C" fn jet_std_xml_parse_with(a0: JetCString, a1: *mut crate::jet_std::XMLParseOptions, ok: *mut *mut crate::jet_std::DataTree, err: *mut *mut crate::jet_std::XMLError) -> i64 {
        guard(|| match crate::jet_std_xml_parse_with(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.encoding.xml.to_string
    #[no_mangle]
    pub extern "C" fn jet_std_xml_render(a0: *mut crate::jet_std::DataTree, ok: *mut JetCString, err: *mut *mut crate::jet_std::XMLError) -> i64 {
        guard(|| match crate::jet_std_xml_render(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.encoding.xml.root
    #[no_mangle]
    pub extern "C" fn jet_std_xml_root(a0: *mut crate::jet_std::DataTree, ok: *mut *mut crate::jet_std::DataTree, err: *mut *mut crate::jet_std::XMLError) -> i64 {
        guard(|| match crate::jet_std_xml_root(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.db.row_bool
    #[no_mangle]
    pub extern "C" fn jet_db_row_bool(a0: *mut crate::jet_std::JetDBRow, a1: JetCString, ok: *mut bool, err: *mut *mut crate::jet_std::DBError) -> i64 {
        guard(|| match crate::jet_std::jet_db_row_bool(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(value) => { unsafe { ok.write(value) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.db.row_float
    #[no_mangle]
    pub extern "C" fn jet_db_row_float(a0: *mut crate::jet_std::JetDBRow, a1: JetCString, ok: *mut f64, err: *mut *mut crate::jet_std::DBError) -> i64 {
        guard(|| match crate::jet_std::jet_db_row_float(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(value) => { unsafe { ok.write(value) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.db.row_int
    #[no_mangle]
    pub extern "C" fn jet_db_row_int(a0: *mut crate::jet_std::JetDBRow, a1: JetCString, ok: *mut i64, err: *mut *mut crate::jet_std::DBError) -> i64 {
        guard(|| match crate::jet_std::jet_db_row_int(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.db.row_text
    #[no_mangle]
    pub extern "C" fn jet_db_row_text(a0: *mut crate::jet_std::JetDBRow, a1: JetCString, ok: *mut JetCString, err: *mut *mut crate::jet_std::DBError) -> i64 {
        guard(|| match crate::jet_std::jet_db_row_text(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.db.row_value
    #[no_mangle]
    pub extern "C" fn jet_db_row_value(a0: *mut crate::jet_std::JetDBRow, a1: JetCString, ok: *mut *mut crate::jet_std::DBValue, err: *mut *mut crate::jet_std::DBError) -> i64 {
        guard(|| match crate::jet_std::jet_db_row_value(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.builtin.float_parse
    #[no_mangle]
    pub extern "C" fn jet_float_parse(a0: JetCString, ok: *mut f64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_std::jet_float_parse(view(a0)) {
            Ok(value) => { unsafe { ok.write(value) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.numeric.checked_widen
    #[no_mangle]
    pub extern "C" fn jet_int_checked_widen(a0: i64, a1: u8, a2: JetCString, a3: i64) -> f64 {
        guard(|| crate::jet_std::jet_int_checked_widen(a0, a1 != 0, view(a2), fixed::<u32>(a3)))
    }

    /// core.builtin.int_from_radix
    #[no_mangle]
    pub extern "C" fn jet_int_from_radix(a0: JetCString, a1: i64, ok: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_std::jet_int_from_radix(view(a0), a1) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.builtin.int_parse
    #[no_mangle]
    pub extern "C" fn jet_int_parse(a0: JetCString, ok: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_std::jet_int_parse(view(a0)) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.builtin.int_to_radix
    #[no_mangle]
    pub extern "C" fn jet_int_to_radix(a0: i64, a1: i64, ok: *mut JetCString, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_std::jet_int_to_radix(a0, a1) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.regex.compile
    #[no_mangle]
    pub extern "C" fn jet_regex_compile(a0: JetCString, ok: *mut *mut crate::jet_std::JetRegex, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_std::jet_regex_compile(view(a0)) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.regex.compile_with
    #[no_mangle]
    pub extern "C" fn jet_regex_compile_with(a0: JetCString, a1: *mut crate::jet_std::RegexFlags, ok: *mut *mut crate::jet_std::JetRegex, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_std::jet_regex_compile_with(view(a0), unsafe { &*a1 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.regex.escape
    #[no_mangle]
    pub extern "C" fn jet_regex_escape(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_std::jet_regex_escape(view(a0))))
    }

    /// core.regex.find
    #[no_mangle]
    pub extern "C" fn jet_regex_find(a0: *mut crate::jet_std::JetRegex, a1: JetCString, some: *mut JetCString) -> i64 {
        guard(|| match crate::jet_std::jet_regex_find(unsafe { &*a0 }, view(a1)) {
            Some(value) => { unsafe { some.write(handle(value)) }; 1 }
            None => 0,
        })
    }

    /// core.regex.find_all, core.regex.findall
    #[no_mangle]
    pub extern "C" fn jet_regex_find_all(a0: *mut crate::jet_std::JetRegex, a1: JetCString, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_std::jet_regex_find_all(unsafe { &*a0 }, view(a1)).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.regex.flags
    #[no_mangle]
    pub extern "C" fn jet_regex_flags(a0: u8, a1: u8, a2: u8) -> *mut crate::jet_std::RegexFlags {
        guard(|| Box::into_raw(Box::new(crate::jet_std::jet_regex_flags(a0 != 0, a1 != 0, a2 != 0))))
    }

    /// core.regex.full_match, core.regex.fullmatch
    #[no_mangle]
    pub extern "C" fn jet_regex_full_match(a0: *mut crate::jet_std::JetRegex, a1: JetCString) -> bool {
        guard(|| crate::jet_std::jet_regex_full_match(unsafe { &*a0 }, view(a1)))
    }

    /// core.regex.is_match
    #[no_mangle]
    pub extern "C" fn jet_regex_is_match(a0: *mut crate::jet_std::JetRegex, a1: JetCString) -> bool {
        guard(|| crate::jet_std::jet_regex_is_match(unsafe { &*a0 }, view(a1)))
    }

    /// core.regex.literal
    #[no_mangle]
    pub extern "C" fn jet_regex_literal(a0: JetCString) -> *mut crate::jet_std::JetRegex {
        guard(|| Box::into_raw(Box::new(crate::jet_std::jet_regex_literal(view(a0)))))
    }

    /// core.regex.match, core.regex.search
    #[no_mangle]
    pub extern "C" fn jet_regex_match(a0: *mut crate::jet_std::JetRegex, a1: JetCString, some: *mut *mut crate::jet_std::JetRegexMatch) -> i64 {
        guard(|| match crate::jet_std::jet_regex_match(unsafe { &*a0 }, view(a1)) {
            Some(value) => { unsafe { some.write(Box::into_raw(Box::new(value))) }; 1 }
            None => 0,
        })
    }

    /// core.regex.replace, core.regex.sub
    #[no_mangle]
    pub extern "C" fn jet_regex_replace(a0: *mut crate::jet_std::JetRegex, a1: JetCString, a2: JetCString) -> JetCString {
        guard(|| handle(crate::jet_std::jet_regex_replace(unsafe { &*a0 }, view(a1), view(a2))))
    }

    /// core.regex.replace_first
    #[no_mangle]
    pub extern "C" fn jet_regex_replace_first(a0: *mut crate::jet_std::JetRegex, a1: JetCString, a2: JetCString) -> JetCString {
        guard(|| handle(crate::jet_std::jet_regex_replace_first(unsafe { &*a0 }, view(a1), view(a2))))
    }

    /// core.regex.split
    #[no_mangle]
    pub extern "C" fn jet_regex_split(a0: *mut crate::jet_std::JetRegex, a1: JetCString, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_std::jet_regex_split(unsafe { &*a0 }, view(a1)).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.regex.split_limit
    #[no_mangle]
    pub extern "C" fn jet_regex_split_limit(a0: *mut crate::jet_std::JetRegex, a1: JetCString, a2: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_std::jet_regex_split_limit(unsafe { &*a0 }, view(a1), a2).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.tasks.current_task
    #[no_mangle]
    pub extern "C" fn jet_task_current_trace() -> JetCString {
        guard(|| handle(crate::jet_std::jet_task_current_trace()))
    }

    /// core.tasks.yield_now
    #[no_mangle]
    pub extern "C" fn jet_task_yield() {
        guard(|| crate::jet_std::jet_task_yield())
    }

    /// core.builtin.string_after
    #[no_mangle]
    pub extern "C" fn jet_string_after(a0: JetCString, a1: JetCString) -> JetCString {
        guard(|| handle(crate::jet_string_after(view(a0), view(a1))))
    }

    /// core.builtin.string_before
    #[no_mangle]
    pub extern "C" fn jet_string_before(a0: JetCString, a1: JetCString) -> JetCString {
        guard(|| handle(crate::jet_string_before(view(a0), view(a1))))
    }

    /// core.builtin.bytes, core.builtin.bytes
    #[no_mangle]
    pub extern "C" fn jet_string_bytes(a0: JetCString, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_string_bytes(unsafe { &*a0 }).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.builtin.string_chars
    #[no_mangle]
    pub extern "C" fn jet_string_chars(a0: JetCString, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_string_chars(unsafe { &*a0 }).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.builtin.string_from_bytes
    #[no_mangle]
    pub extern "C" fn jet_string_from_bytes(a0_0: *const u64, a0_1: i64, ok: *mut JetCString, err: *mut *mut crate::jet_std::UTF8Error) -> i64 {
        guard(|| match crate::jet_string_from_bytes(&list_in(a0_0, a0_1, |w| w as u8)) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.builtin.string_from_bytes_lossy
    #[no_mangle]
    pub extern "C" fn jet_string_from_bytes_lossy(a0_0: *const u64, a0_1: i64) -> JetCString {
        guard(|| handle(crate::jet_string_from_bytes_lossy(&list_in(a0_0, a0_1, |w| w as u8))))
    }

    /// core.builtin.string_lines
    #[no_mangle]
    pub extern "C" fn jet_string_lines(a0: JetCString, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_string_lines(unsafe { &*a0 }).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.builtin.string_repeat
    #[no_mangle]
    pub extern "C" fn jet_string_repeat(a0: JetCString, a1: i64) -> JetCString {
        guard(|| handle(crate::jet_string_repeat(unsafe { &*a0 }, a1)))
    }

    /// core.collections.slice_string
    #[no_mangle]
    pub extern "C" fn jet_string_slice(a0: JetCString, a1: i64, a2: i64, a3: JetCString, a4: i64) -> JetCString {
        guard(|| handle(crate::jet_string_slice(unsafe { &*a0 }, a1, a2, view(a3), fixed::<u32>(a4))))
    }

    /// core.builtin.string_slice
    #[no_mangle]
    pub extern "C" fn jet_string_slice_builtin(a0: JetCString, a1: i64, a2: i64) -> JetCString {
        guard(|| handle(crate::jet_string_slice_builtin(unsafe { &*a0 }, a1, a2)))
    }

    /// core.term.read_key
    #[no_mangle]
    pub extern "C" fn jet_term_read_key() -> *mut crate::JetKey {
        guard(|| Box::into_raw(Box::new(crate::jet_term_read_key())))
    }

    /// core.testing.test_suite
    #[no_mangle]
    pub extern "C" fn jet_test_suite_new() -> *mut crate::jet_std::JetTestSuite {
        guard(|| Box::into_raw(Box::new(crate::jet_test_suite_new())))
    }

    /// core.handle.test_suite.run
    #[no_mangle]
    pub extern "C" fn jet_test_suite_run(a0: *mut crate::jet_std::JetTestSuite) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_test_suite_run(unsafe { &*a0 })))
    }

    /// core.testing.assert_equal
    #[no_mangle]
    pub extern "C" fn jet_testing_assert_equal(a0: *mut crate::jet_std::JetTestComparison) -> bool {
        guard(|| crate::jet_testing_assert_equal(unsafe { &*a0 }))
    }

    /// core.testing.fake_data
    #[no_mangle]
    pub extern "C" fn jet_testing_fake_new(a0: i64) -> *mut crate::jet_std::Fake {
        guard(|| Box::into_raw(Box::new(crate::jet_testing_fake_new(a0))))
    }

    /// core.handle.history_rng.below
    #[no_mangle]
    pub extern "C" fn jet_testing_history_rng_below(a0: *mut crate::JetHistoryRng, a1: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(i64::try_from(crate::jet_testing_history_rng_below(unsafe { &*a0 }, fixed::<u64>(a1))).unwrap_or_else(|_| range_stop())))
    }

    /// core.handle.history_rng.next_u64
    #[no_mangle]
    pub extern "C" fn jet_testing_history_rng_next_u64(a0: *mut crate::JetHistoryRng) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(i64::try_from(crate::jet_testing_history_rng_next_u64(unsafe { &*a0 })).unwrap_or_else(|_| range_stop())))
    }

    /// core.testing.status
    #[no_mangle]
    pub extern "C" fn jet_testing_status(a0: *mut crate::jet_std::JetTestComparison) -> JetCString {
        guard(|| handle(crate::jet_testing_status(unsafe { &*a0 })))
    }

    /// core.builtin.string_ascii_lower
    #[no_mangle]
    pub extern "C" fn jet_text_ascii_lower(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_ascii_lower(unsafe { &*a0 })))
    }

    /// core.builtin.string_ascii_upper
    #[no_mangle]
    pub extern "C" fn jet_text_ascii_upper(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_ascii_upper(unsafe { &*a0 })))
    }

    /// core.text.parse.capitalize
    #[no_mangle]
    pub extern "C" fn jet_text_capitalize(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_capitalize(unsafe { &*a0 })))
    }

    /// core.text.casefold
    #[no_mangle]
    pub extern "C" fn jet_text_casefold(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_casefold(unsafe { &*a0 })))
    }

    /// core.text.caseless_eq
    #[no_mangle]
    pub extern "C" fn jet_text_caseless_eq(a0: JetCString, a1: JetCString) -> bool {
        guard(|| crate::jet_text_caseless_eq(unsafe { &*a0 }, unsafe { &*a1 }))
    }

    /// core.text.center, core.text.parse.center
    #[no_mangle]
    pub extern "C" fn jet_text_center_i64(a0: JetCString, a1: i64, a2: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_center_i64(unsafe { &*a0 }, a1, unsafe { &*a2 })))
    }

    /// core.text.char_indices
    #[no_mangle]
    pub extern "C" fn jet_text_char_indices(a0: JetCString, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_text_char_indices(unsafe { &*a0 }).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.text.wrap.dedent
    #[no_mangle]
    pub extern "C" fn jet_text_dedent(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_dedent(unsafe { &*a0 })))
    }

    /// core.text.parse.encode
    #[no_mangle]
    pub extern "C" fn jet_text_encode(a0: JetCString, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_text_encode(unsafe { &*a0 }).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.text.ends_any
    #[no_mangle]
    pub extern "C" fn jet_text_ends_any(a0: JetCString, a1_0: *const u64, a1_1: i64) -> bool {
        guard(|| crate::jet_text_ends_any(unsafe { &*a0 }, &list_in(a1_0, a1_1, |w| view(w as JetCString).to_owned())))
    }

    /// core.text.wrap.expand_tabs
    #[no_mangle]
    pub extern "C" fn jet_text_expand_tabs(a0: JetCString, a1: i64) -> JetCString {
        guard(|| handle(crate::jet_text_expand_tabs(unsafe { &*a0 }, a1)))
    }

    /// core.text.parse.expandtabs, core.builtin.expandtabs
    #[no_mangle]
    pub extern "C" fn jet_text_expandtabs(a0: JetCString, a1: i64) -> JetCString {
        guard(|| handle(crate::jet_text_expandtabs(unsafe { &*a0 }, unsafe { crate::jet_foundation::Numeric::JetInt::clone_from_raw(a1) })))
    }

    /// core.text.wrap.fill
    #[no_mangle]
    pub extern "C" fn jet_text_fill(a0: JetCString, a1: i64) -> JetCString {
        guard(|| handle(crate::jet_text_fill(unsafe { &*a0 }, a1)))
    }

    /// core.text.graphemes
    #[no_mangle]
    pub extern "C" fn jet_text_graphemes(a0: JetCString, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_text_graphemes(unsafe { &*a0 }).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.text.wrap.hanging_indent
    #[no_mangle]
    pub extern "C" fn jet_text_hanging_indent(a0: JetCString, a1: JetCString, a2: JetCString, a3: i64) -> JetCString {
        guard(|| handle(crate::jet_text_hanging_indent(unsafe { &*a0 }, unsafe { &*a1 }, unsafe { &*a2 }, a3)))
    }

    /// core.text.wrap.html_escape, core.text.html.escape_quote
    #[no_mangle]
    pub extern "C" fn jet_text_html_escape(a0: JetCString, a1: u8) -> JetCString {
        guard(|| handle(crate::jet_text_html_escape(unsafe { &*a0 }, a1 != 0)))
    }

    /// core.text.html.escape, core.text.html.attr_escape
    #[no_mangle]
    pub extern "C" fn jet_text_html_escape_quoted(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_html_escape_quoted(unsafe { &*a0 })))
    }

    /// core.text.html.text_escape
    #[no_mangle]
    pub extern "C" fn jet_text_html_escape_text(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_html_escape_text(unsafe { &*a0 })))
    }

    /// core.text.html.strip_tags
    #[no_mangle]
    pub extern "C" fn jet_text_html_strip_tags(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_html_strip_tags(unsafe { &*a0 })))
    }

    /// core.text.wrap.html_unescape, core.text.html.unescape
    #[no_mangle]
    pub extern "C" fn jet_text_html_unescape(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_html_unescape(unsafe { &*a0 })))
    }

    /// core.text.html.unescape_and_strip
    #[no_mangle]
    pub extern "C" fn jet_text_html_unescape_and_strip(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_html_unescape_and_strip(unsafe { &*a0 })))
    }

    /// core.text.wrap.indent
    #[no_mangle]
    pub extern "C" fn jet_text_indent(a0: JetCString, a1: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_indent(unsafe { &*a0 }, unsafe { &*a1 })))
    }

    /// core.text.wrap.indent_with
    #[no_mangle]
    pub extern "C" fn jet_text_indent_with(a0: JetCString, a1: JetCString, a2: u8) -> JetCString {
        guard(|| handle(crate::jet_text_indent_with(unsafe { &*a0 }, unsafe { &*a1 }, a2 != 0)))
    }

    /// core.text.inspect
    #[no_mangle]
    pub extern "C" fn jet_text_inspect(a0: JetCString, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_text_inspect(unsafe { &*a0 }).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.text.is_alphabetic, core.text.parse.isalpha, core.builtin.string_is_alphabetic
    #[no_mangle]
    pub extern "C" fn jet_text_is_alphabetic(a0: JetCString) -> bool {
        guard(|| crate::jet_text_is_alphabetic(unsafe { &*a0 }))
    }

    /// core.text.parse.islower, core.text.parse.is_lower
    #[no_mangle]
    pub extern "C" fn jet_text_is_lower(a0: JetCString) -> bool {
        guard(|| crate::jet_text_is_lower(unsafe { &*a0 }))
    }

    /// core.text.is_numeric, core.builtin.string_is_numeric
    #[no_mangle]
    pub extern "C" fn jet_text_is_numeric(a0: JetCString) -> bool {
        guard(|| crate::jet_text_is_numeric(unsafe { &*a0 }))
    }

    /// core.text.parse.isupper, core.text.parse.is_upper
    #[no_mangle]
    pub extern "C" fn jet_text_is_upper(a0: JetCString) -> bool {
        guard(|| crate::jet_text_is_upper(unsafe { &*a0 }))
    }

    /// core.text.is_whitespace, core.text.parse.isspace, core.text.parse.is_space, core.builtin.string_is_whitespace
    #[no_mangle]
    pub extern "C" fn jet_text_is_whitespace(a0: JetCString) -> bool {
        guard(|| crate::jet_text_is_whitespace(unsafe { &*a0 }))
    }

    /// core.text.parse.isalnum, core.text.parse.is_alnum
    #[no_mangle]
    pub extern "C" fn jet_text_isalnum(a0: JetCString) -> bool {
        guard(|| crate::jet_text_isalnum(unsafe { &*a0 }))
    }

    /// core.text.parse.isdecimal
    #[no_mangle]
    pub extern "C" fn jet_text_isdecimal(a0: JetCString) -> bool {
        guard(|| crate::jet_text_isdecimal(unsafe { &*a0 }))
    }

    /// core.text.parse.isdigit, core.text.parse.is_digit
    #[no_mangle]
    pub extern "C" fn jet_text_isdigit(a0: JetCString) -> bool {
        guard(|| crate::jet_text_isdigit(unsafe { &*a0 }))
    }

    /// core.text.parse.isidentifier, core.text.parse.is_identifier
    #[no_mangle]
    pub extern "C" fn jet_text_isidentifier(a0: JetCString) -> bool {
        guard(|| crate::jet_text_isidentifier(unsafe { &*a0 }))
    }

    /// core.text.parse.isnumeric
    #[no_mangle]
    pub extern "C" fn jet_text_isnumeric(a0: JetCString) -> bool {
        guard(|| crate::jet_text_isnumeric(unsafe { &*a0 }))
    }

    /// core.text.parse.isprintable
    #[no_mangle]
    pub extern "C" fn jet_text_isprintable(a0: JetCString) -> bool {
        guard(|| crate::jet_text_isprintable(unsafe { &*a0 }))
    }

    /// core.text.parse.istitle, core.text.parse.is_title
    #[no_mangle]
    pub extern "C" fn jet_text_istitle(a0: JetCString) -> bool {
        guard(|| crate::jet_text_istitle(unsafe { &*a0 }))
    }

    /// core.text.lower, core.text.parse.lower
    #[no_mangle]
    pub extern "C" fn jet_text_lower(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_lower(unsafe { &*a0 })))
    }

    /// core.text.nfc
    #[no_mangle]
    pub extern "C" fn jet_text_nfc(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_nfc(unsafe { &*a0 })))
    }

    /// core.text.nfd
    #[no_mangle]
    pub extern "C" fn jet_text_nfd(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_nfd(unsafe { &*a0 })))
    }

    /// core.text.nfkc
    #[no_mangle]
    pub extern "C" fn jet_text_nfkc(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_nfkc(unsafe { &*a0 })))
    }

    /// core.text.nfkd
    #[no_mangle]
    pub extern "C" fn jet_text_nfkd(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_nfkd(unsafe { &*a0 })))
    }

    /// core.builtin.string_pad_end
    #[no_mangle]
    pub extern "C" fn jet_text_pad_end(a0: JetCString, a1: i64, a2: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_pad_end(unsafe { &*a0 }, unsafe { crate::jet_foundation::Numeric::JetInt::clone_from_raw(a1) }, unsafe { &*a2 })))
    }

    /// core.text.pad_end, core.text.parse.ljust
    #[no_mangle]
    pub extern "C" fn jet_text_pad_end_i64(a0: JetCString, a1: i64, a2: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_pad_end_i64(unsafe { &*a0 }, a1, unsafe { &*a2 })))
    }

    /// core.builtin.string_pad_start
    #[no_mangle]
    pub extern "C" fn jet_text_pad_start(a0: JetCString, a1: i64, a2: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_pad_start(unsafe { &*a0 }, unsafe { crate::jet_foundation::Numeric::JetInt::clone_from_raw(a1) }, unsafe { &*a2 })))
    }

    /// core.text.pad_start, core.text.parse.rjust
    #[no_mangle]
    pub extern "C" fn jet_text_pad_start_i64(a0: JetCString, a1: i64, a2: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_pad_start_i64(unsafe { &*a0 }, a1, unsafe { &*a2 })))
    }

    /// core.text.parse.parse_bool
    #[no_mangle]
    pub extern "C" fn jet_text_parse_bool(a0: JetCString, some: *mut bool) -> i64 {
        guard(|| match crate::jet_text_parse_bool(unsafe { &*a0 }) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.text.parse.capwords
    #[no_mangle]
    pub extern "C" fn jet_text_parse_capwords(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_parse_capwords(unsafe { &*a0 })))
    }

    /// core.text.parse.contains
    #[no_mangle]
    pub extern "C" fn jet_text_parse_contains(a0: JetCString, a1: JetCString) -> bool {
        guard(|| crate::jet_text_parse_contains(unsafe { &*a0 }, unsafe { &*a1 }))
    }

    /// core.text.parse.count
    #[no_mangle]
    pub extern "C" fn jet_text_parse_count(a0: JetCString, a1: JetCString) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_text_parse_count(unsafe { &*a0 }, unsafe { &*a1 })))
    }

    /// core.text.parse.escape_c
    #[no_mangle]
    pub extern "C" fn jet_text_parse_escape_c(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_parse_escape_c(unsafe { &*a0 })))
    }

    /// core.text.parse.find
    #[no_mangle]
    pub extern "C" fn jet_text_parse_find(a0: JetCString, a1: JetCString) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_text_parse_find(unsafe { &*a0 }, unsafe { &*a1 })))
    }

    /// core.text.parse.find_from
    #[no_mangle]
    pub extern "C" fn jet_text_parse_find_from(a0: JetCString, a1: JetCString, a2: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_text_parse_find_from(unsafe { &*a0 }, unsafe { &*a1 }, a2)))
    }

    /// core.text.parse.parse_float
    #[no_mangle]
    pub extern "C" fn jet_text_parse_float(a0: JetCString, some: *mut f64) -> i64 {
        guard(|| match crate::jet_text_parse_float(unsafe { &*a0 }) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.text.parse.index
    #[no_mangle]
    pub extern "C" fn jet_text_parse_index(a0: JetCString, a1: JetCString) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_text_parse_index(unsafe { &*a0 }, unsafe { &*a1 })))
    }

    /// core.text.parse.parse_int, core.text.parse.parse
    #[no_mangle]
    pub extern "C" fn jet_text_parse_int(a0: JetCString, some: *mut i64) -> i64 {
        guard(|| match crate::jet_text_parse_int(unsafe { &*a0 }) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.text.parse.parse_int_base
    #[no_mangle]
    pub extern "C" fn jet_text_parse_int_base(a0: JetCString, a1: i64, some: *mut i64) -> i64 {
        guard(|| match crate::jet_text_parse_int_base(unsafe { &*a0 }, a1) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.text.parse.join
    #[no_mangle]
    pub extern "C" fn jet_text_parse_join(a0_0: *const u64, a0_1: i64, a1: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_parse_join(&list_in(a0_0, a0_1, |w| view(w as JetCString).to_owned()), unsafe { &*a1 })))
    }

    /// core.text.parse.lstrip
    #[no_mangle]
    pub extern "C" fn jet_text_parse_lstrip(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_parse_lstrip(unsafe { &*a0 })))
    }

    /// core.text.parse.replace
    #[no_mangle]
    pub extern "C" fn jet_text_parse_replace_n(a0: JetCString, a1: JetCString, a2: JetCString, a3: i64) -> JetCString {
        guard(|| handle(crate::jet_text_parse_replace_n(unsafe { &*a0 }, unsafe { &*a1 }, unsafe { &*a2 }, a3)))
    }

    /// core.text.parse.rfind
    #[no_mangle]
    pub extern "C" fn jet_text_parse_rfind(a0: JetCString, a1: JetCString) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_text_parse_rfind(unsafe { &*a0 }, unsafe { &*a1 })))
    }

    /// core.text.parse.rfind_from
    #[no_mangle]
    pub extern "C" fn jet_text_parse_rfind_from(a0: JetCString, a1: JetCString, a2: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_text_parse_rfind_from(unsafe { &*a0 }, unsafe { &*a1 }, a2)))
    }

    /// core.text.parse.rsplit
    #[no_mangle]
    pub extern "C" fn jet_text_parse_rsplit(a0: JetCString, a1: JetCString, a2: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_text_parse_rsplit(unsafe { &*a0 }, unsafe { &*a1 }, a2).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.text.parse.rstrip
    #[no_mangle]
    pub extern "C" fn jet_text_parse_rstrip(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_parse_rstrip(unsafe { &*a0 })))
    }

    /// core.text.parse.split
    #[no_mangle]
    pub extern "C" fn jet_text_parse_split(a0: JetCString, a1: JetCString, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_text_parse_split(unsafe { &*a0 }, unsafe { &*a1 }).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.text.parse.split_ws
    #[no_mangle]
    pub extern "C" fn jet_text_parse_split_ws(a0: JetCString, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_text_parse_split_ws(unsafe { &*a0 }).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.text.parse.splitlines
    #[no_mangle]
    pub extern "C" fn jet_text_parse_splitlines(a0: JetCString, a1: u8, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_text_parse_splitlines(unsafe { &*a0 }, a1 != 0).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.text.parse.strip
    #[no_mangle]
    pub extern "C" fn jet_text_parse_strip(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_parse_strip(unsafe { &*a0 })))
    }

    /// core.text.parse.unescape_c
    #[no_mangle]
    pub extern "C" fn jet_text_parse_unescape_c(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_parse_unescape_c(unsafe { &*a0 })))
    }

    /// core.text.parse.removeprefix, core.text.parse.strip_prefix
    #[no_mangle]
    pub extern "C" fn jet_text_remove_prefix(a0: JetCString, a1: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_remove_prefix(unsafe { &*a0 }, unsafe { &*a1 })))
    }

    /// core.text.parse.removesuffix, core.text.parse.strip_suffix
    #[no_mangle]
    pub extern "C" fn jet_text_remove_suffix(a0: JetCString, a1: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_remove_suffix(unsafe { &*a0 }, unsafe { &*a1 })))
    }

    /// core.text.parse.rindex
    #[no_mangle]
    pub extern "C" fn jet_text_rindex(a0: JetCString, a1: JetCString) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_text_rindex(unsafe { &*a0 }, unsafe { &*a1 })))
    }

    /// core.text.rsplitn
    #[no_mangle]
    pub extern "C" fn jet_text_rsplitn(a0: JetCString, a1: JetCString, a2: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_text_rsplitn(unsafe { &*a0 }, unsafe { &*a1 }, a2).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.text.sentences
    #[no_mangle]
    pub extern "C" fn jet_text_sentences(a0: JetCString, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_text_sentences(unsafe { &*a0 }).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.text.wrap.shorten
    #[no_mangle]
    pub extern "C" fn jet_text_shorten(a0: JetCString, a1: i64, a2: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_shorten(unsafe { &*a0 }, a1, unsafe { &*a2 })))
    }

    /// core.text.splitn
    #[no_mangle]
    pub extern "C" fn jet_text_splitn(a0: JetCString, a1: JetCString, a2: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_text_splitn(unsafe { &*a0 }, unsafe { &*a1 }, a2).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.text.starts_any
    #[no_mangle]
    pub extern "C" fn jet_text_starts_any(a0: JetCString, a1_0: *const u64, a1_1: i64) -> bool {
        guard(|| crate::jet_text_starts_any(unsafe { &*a0 }, &list_in(a1_0, a1_1, |w| view(w as JetCString).to_owned())))
    }

    /// core.text.parse.swapcase
    #[no_mangle]
    pub extern "C" fn jet_text_swapcase(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_swapcase(unsafe { &*a0 })))
    }

    /// core.text.parse.title, core.builtin.string_to_title
    #[no_mangle]
    pub extern "C" fn jet_text_title(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_title(unsafe { &*a0 })))
    }

    /// core.text.trim
    #[no_mangle]
    pub extern "C" fn jet_text_trim(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_trim(unsafe { &*a0 })))
    }

    /// core.text.trim_end, core.builtin.string_trim_end
    #[no_mangle]
    pub extern "C" fn jet_text_trim_end(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_trim_end(unsafe { &*a0 })))
    }

    /// core.text.trim_start, core.builtin.string_trim_start
    #[no_mangle]
    pub extern "C" fn jet_text_trim_start(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_trim_start(unsafe { &*a0 })))
    }

    /// core.text.byte_count
    #[no_mangle]
    pub extern "C" fn jet_text_unicode_byte_count(a0: JetCString) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_text_unicode_byte_count(unsafe { &*a0 })))
    }

    /// core.text.is_ascii, core.text.parse.isascii, core.text.parse.is_ascii, core.builtin.string_is_ascii
    #[no_mangle]
    pub extern "C" fn jet_text_unicode_is_ascii(a0: JetCString) -> bool {
        guard(|| crate::jet_text_unicode_is_ascii(unsafe { &*a0 }))
    }

    /// core.text.scalar_count
    #[no_mangle]
    pub extern "C" fn jet_text_unicode_scalar_count(a0: JetCString) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_text_unicode_scalar_count(unsafe { &*a0 })))
    }

    /// core.text.scalars
    #[no_mangle]
    pub extern "C" fn jet_text_unicode_scalars(a0: JetCString, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_text_unicode_scalars(unsafe { &*a0 }).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.text.upper, core.text.parse.upper
    #[no_mangle]
    pub extern "C" fn jet_text_upper(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_upper(unsafe { &*a0 })))
    }

    /// core.text.words
    #[no_mangle]
    pub extern "C" fn jet_text_words(a0: JetCString, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_text_words(unsafe { &*a0 }).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.text.wrap.wrap
    #[no_mangle]
    pub extern "C" fn jet_text_wrap(a0: JetCString, a1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_text_wrap(unsafe { &*a0 }, a1).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.text.wrap.wrap_paragraphs
    #[no_mangle]
    pub extern "C" fn jet_text_wrap_paragraphs(a0: JetCString, a1: i64) -> JetCString {
        guard(|| handle(crate::jet_text_wrap_paragraphs(unsafe { &*a0 }, a1)))
    }

    /// core.text.parse.zfill
    #[no_mangle]
    pub extern "C" fn jet_text_zfill(a0: JetCString, a1: i64) -> JetCString {
        guard(|| handle(crate::jet_text_zfill(unsafe { &*a0 }, a1)))
    }

    /// core.time.datetime
    #[no_mangle]
    pub extern "C" fn jet_time_datetime(a0: i64, a1: i64, a2: i64, a3: i64, a4: i64, a5: i64) -> *mut crate::JetDateTime {
        guard(|| Box::into_raw(Box::new(crate::jet_time_datetime(a0, a1, a2, a3, a4, a5))))
    }

    /// core.time.days_in_month
    #[no_mangle]
    pub extern "C" fn jet_time_days_in_month(a0: i64, a1: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_time_days_in_month(a0, a1)))
    }

    /// core.time.from_iso_week
    #[no_mangle]
    pub extern "C" fn jet_time_from_iso_week(a0: i64, a1: i64, a2: i64, ok: *mut *mut crate::JetDate, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_time_from_iso_week(a0, a1, a2) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.time.instant
    #[no_mangle]
    pub extern "C" fn jet_time_instant_now() -> *mut crate::JetInstant {
        guard(|| Box::into_raw(Box::new(crate::jet_time_instant_now())))
    }

    /// core.time.is_leap_year
    #[no_mangle]
    pub extern "C" fn jet_time_is_leap_year(a0: i64) -> bool {
        guard(|| crate::jet_time_is_leap_year(a0))
    }

    /// core.time.now_utc
    #[no_mangle]
    pub extern "C" fn jet_time_now_utc() -> *mut crate::JetDateTime {
        guard(|| Box::into_raw(Box::new(crate::jet_time_now_utc())))
    }

    /// core.time.parse_rfc3339
    #[no_mangle]
    pub extern "C" fn jet_time_parse_rfc3339(a0: JetCString, ok: *mut *mut crate::JetDateTime, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_time_parse_rfc3339(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.time.parse_zoned
    #[no_mangle]
    pub extern "C" fn jet_time_parse_zoned(a0: JetCString, ok: *mut *mut crate::JetZonedDateTime, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_time_parse_zoned(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.time.period
    #[no_mangle]
    pub extern "C" fn jet_time_period(a0: i64, a1: i64, a2: i64) -> *mut crate::JetPeriod {
        guard(|| Box::into_raw(Box::new(crate::jet_time_period(a0, a1, a2))))
    }

    /// core.time.period_days
    #[no_mangle]
    pub extern "C" fn jet_time_period_days(a0: i64) -> *mut crate::JetPeriod {
        guard(|| Box::into_raw(Box::new(crate::jet_time_period_days(a0))))
    }

    /// core.time.period_months
    #[no_mangle]
    pub extern "C" fn jet_time_period_months(a0: i64) -> *mut crate::JetPeriod {
        guard(|| Box::into_raw(Box::new(crate::jet_time_period_months(a0))))
    }

    /// core.time.period_years
    #[no_mangle]
    pub extern "C" fn jet_time_period_years(a0: i64) -> *mut crate::JetPeriod {
        guard(|| Box::into_raw(Box::new(crate::jet_time_period_years(a0))))
    }

    /// core.time.sleep_until
    #[no_mangle]
    pub extern "C" fn jet_time_sleep_until(a0: *mut crate::JetInstant) {
        guard(|| crate::jet_time_sleep_until(unsafe { &*a0 }))
    }

    /// core.time.zone
    #[no_mangle]
    pub extern "C" fn jet_time_zone_named(a0: JetCString, ok: *mut *mut crate::JetZone, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_time_zone_named(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.time.utc
    #[no_mangle]
    pub extern "C" fn jet_time_zone_utc() -> *mut crate::JetZone {
        guard(|| Box::into_raw(Box::new(crate::jet_time_zone_utc())))
    }

    /// core.time.zoned
    #[no_mangle]
    pub extern "C" fn jet_time_zoned(a0: *mut crate::JetDateTime, a1: *mut crate::JetZone) -> *mut crate::JetZonedDateTime {
        guard(|| Box::into_raw(Box::new(crate::jet_time_zoned(unsafe { &*a0 }, unsafe { &*a1 }))))
    }

    /// core.time.zoned_local
    #[no_mangle]
    pub extern "C" fn jet_time_zoned_local(a0: *mut crate::JetDate, a1: *mut crate::JetLocalTime, a2: *mut crate::JetZone, a3: JetCString, ok: *mut *mut crate::JetZonedDateTime, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_time_zoned_local(unsafe { &*a0 }, unsafe { &*a1 }, unsafe { &*a2 }, unsafe { &*a3 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.handle.tls.config_default
    #[no_mangle]
    pub extern "C" fn jet_tls_client_config_default() -> *mut crate::JetTLSClientConfig {
        guard(|| Box::into_raw(Box::new(crate::jet_tls_client_config_default())))
    }

    /// core.builtin.string_count
    #[no_mangle]
    pub extern "C" fn jet_unicode_count(a0: JetCString, a1: JetCString) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_unicode_count(view(a0), view(a1))))
    }

    /// core.builtin.string_index_of
    #[no_mangle]
    pub extern "C" fn jet_unicode_index_of(a0: JetCString, a1: JetCString, some: *mut i64) -> i64 {
        guard(|| match crate::jet_unicode_index_of(view(a0), view(a1)) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.builtin.string_lower
    #[no_mangle]
    pub extern "C" fn jet_unicode_lower(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_unicode_lower(view(a0))))
    }

    /// core.builtin.string_trim
    #[no_mangle]
    pub extern "C" fn jet_unicode_trim(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_unicode_trim(view(a0))))
    }

    /// core.builtin.string_upper
    #[no_mangle]
    pub extern "C" fn jet_unicode_upper(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_unicode_upper(view(a0))))
    }

    /// core.units.conversion_exact
    #[no_mangle]
    pub extern "C" fn jet_unit_conversion_exact(a0: f64, a1: JetCString, a2: JetCString, a3: JetCString, a4: JetCString, some: *mut f64) -> i64 {
        guard(|| match crate::jet_unit_conversion_exact(a0, view(a1), view(a2), view(a3), view(a4)) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.net.url.data
    #[no_mangle]
    pub extern "C" fn jet_url_data(a0: *mut crate::jet_std::JetMIME, a1: JetCString) -> *mut crate::jet_std::JetURL {
        guard(|| Box::into_raw(Box::new(crate::jet_url_data(unsafe { &*a0 }, unsafe { &*a1 }))))
    }

    /// core.net.url.file
    #[no_mangle]
    pub extern "C" fn jet_url_file(a0: JetCString) -> *mut crate::jet_std::JetURL {
        guard(|| Box::into_raw(Box::new(crate::jet_url_file(unsafe { &*a0 }))))
    }

    /// core.net.url.urljoin
    #[no_mangle]
    pub extern "C" fn jet_url_join(a0: JetCString, a1: JetCString) -> JetCString {
        guard(|| handle(crate::jet_url_join(unsafe { &*a0 }, unsafe { &*a1 })))
    }

    /// core.net.url.parse, core.net.url.urlparse, core.net.url.urlsplit
    #[no_mangle]
    pub extern "C" fn jet_url_parse(a0: JetCString, ok: *mut *mut crate::jet_std::JetURL, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_url_parse(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.net.url.percent_decode
    #[no_mangle]
    pub extern "C" fn jet_url_percent_decode_component(a0: JetCString, ok: *mut JetCString, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_url_percent_decode_component(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.net.url.percent_encode
    #[no_mangle]
    pub extern "C" fn jet_url_percent_encode_component(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_url_percent_encode_component(unsafe { &*a0 })))
    }

    /// core.net.url.quote
    #[no_mangle]
    pub extern "C" fn jet_url_quote(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_url_quote(unsafe { &*a0 })))
    }

    /// core.net.url.quote_from_bytes
    #[no_mangle]
    pub extern "C" fn jet_url_quote_from_bytes(a0_0: *const u64, a0_1: i64) -> JetCString {
        guard(|| handle(crate::jet_url_quote_from_bytes(&list_in(a0_0, a0_1, |w| w as u8))))
    }

    /// core.net.url.quote_plus
    #[no_mangle]
    pub extern "C" fn jet_url_quote_plus(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_url_quote_plus(unsafe { &*a0 })))
    }

    /// core.net.url.geturl, core.net.url.unparse, core.net.url.urlunparse, core.net.url.urlunsplit
    #[no_mangle]
    pub extern "C" fn jet_url_to_string(a0: *mut crate::jet_std::JetURL) -> JetCString {
        guard(|| handle(crate::jet_url_to_string(unsafe { &*a0 })))
    }

    /// core.net.url.unquote
    #[no_mangle]
    pub extern "C" fn jet_url_unquote(a0: JetCString, ok: *mut JetCString, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_url_unquote(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.net.url.unquote_plus
    #[no_mangle]
    pub extern "C" fn jet_url_unquote_plus(a0: JetCString, ok: *mut JetCString, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_url_unquote_plus(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.handle.deterministic_world.advance
    #[no_mangle]
    pub extern "C" fn jet_world_advance(a0: *mut crate::JetDeterministicWorld, a1: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_world_advance(unsafe { &*a0 }, a1)))
    }

    /// core.handle.deterministic_world.history
    #[no_mangle]
    pub extern "C" fn jet_world_history(a0: *mut crate::JetDeterministicWorld) -> JetCString {
        guard(|| handle(crate::jet_world_history(unsafe { &*a0 })))
    }

    /// core.handle.deterministic_world.now
    #[no_mangle]
    pub extern "C" fn jet_world_now(a0: *mut crate::JetDeterministicWorld) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_world_now(unsafe { &*a0 })))
    }

    /// core.handle.deterministic_world.wait_idle
    #[no_mangle]
    pub extern "C" fn jet_world_wait_idle(a0: *mut crate::JetDeterministicWorld) {
        guard(|| crate::jet_world_wait_idle(unsafe { &*a0 }))
    }
}
// Skipped routes (no C carrier yet):
//   jet_app_auth_routes (app.auth_routes): no runtime function
//   jet_app_auth_show (app.auth_show): no runtime function
//   jet_app_invalidate (app.invalidate): no runtime function
//   jet_app_live (app.live): no runtime function
//   jet_app_live_get (app.live_get): no runtime function
//   jet_app_live_show (app.live_show): no runtime function
//   jet_app_live_stats (app.live_stats): no runtime function
//   jet_app_signal_push (app.signal_push): no runtime function
//   jet_app_subscribe (app.subscribe): no runtime function
//   jet_app_sync (app.sync): no runtime function
//   jet_app_transact_invalidate (app.transact_invalidate): no runtime function
//   jet_args_completion (core.handle.args.completion): no runtime function
//   jet_args_decode (core.args.decode): no runtime function
//   jet_args_description (core.handle.args.description): no runtime function
//   jet_args_flag (core.handle.args.flag): no runtime function
//   jet_args_flag_short (core.handle.args.flag_short): no runtime function
//   jet_args_option (core.handle.args.option): no runtime function
//   jet_args_option_choice (core.handle.args.option_choice): no runtime function
//   jet_args_option_default (core.handle.args.option_default): no runtime function
//   jet_args_option_env (core.handle.args.option_env): no runtime function
//   jet_args_option_float (core.handle.args.option_float): no runtime function
//   jet_args_option_int (core.handle.args.option_int): no runtime function
//   jet_args_option_short (core.handle.args.option_short): no runtime function
//   jet_args_parse (core.handle.args.parse): no runtime function
//   jet_args_parse_or_exit (core.handle.args.parse_or_exit): no runtime function
//   jet_args_positional (core.handle.args.positional): no runtime function
//   jet_args_repeat (core.handle.args.repeat): no runtime function
//   jet_args_required_option (core.handle.args.required_option): no runtime function
//   jet_args_spec (core.args.spec): no runtime function
//   jet_args_subcommand (core.handle.args.subcommand): no runtime function
//   jet_args_version (core.handle.args.version): no runtime function
//   jet_auth_session_cookie (core.auth.session_cookie): no runtime function
//   jet_auth_session_id (core.auth.session_id): no runtime function
//   jet_auth_session_show (core.auth.session_show): no runtime function
//   jet_auth_session_user (core.auth.session_user): no runtime function
//   jet_auth_session_validate (core.auth.session_validate): no runtime function
//   jet_auth_verify_jwt_defaulted (core.auth.verify_jwt): no runtime function
//   jet_bag_add (core.builtin.bag_add): generic
//   jet_bag_count (core.builtin.bag_count): generic
//   jet_bag_has (core.builtin.bag_has): generic
//   jet_bag_remove (core.builtin.bag_remove): generic
//   jet_bitset_add (core.builtin.bitset_add): no runtime function
//   jet_bitset_remove (core.builtin.bitset_remove): no runtime function
//   jet_calendar_leapdays (core.time.calendar.leapdays): result type jet_foundation::Numeric::JetInt
//   jet_calendar_monthcalendar (core.time.calendar.monthcalendar): result type Vec<Vec<jet_foundation::Numeric::JetInt>>
//   jet_calendar_monthcalendar_start (core.time.calendar.monthcalendar_start): result type Vec<Vec<jet_foundation::Numeric::JetInt>>
//   jet_calendar_monthrange (core.time.calendar.monthrange): parameter types  year: i64, month: i64, ) -> ( jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt, 
//   jet_calendar_timegm (core.time.calendar.timegm): result type jet_foundation::Numeric::JetInt
//   jet_calendar_weekday (core.time.calendar.weekday): result type jet_foundation::Numeric::JetInt
//   jet_calendar_yearcalendar (core.time.calendar.yearcalendar): result type Vec<Vec<Vec<jet_foundation::Numeric::JetInt>>>
//   jet_clock_advance (core.handle.clock.advance): parameter types c: &mut jet_std::Clock, to_ms: i64
//   jet_clock_tick (core.handle.clock.tick): parameter types c: &mut jet_std::Clock, ms: i64
//   jet_clock_wait (core.handle.clock.wait): parameter types c: &mut jet_std::Clock, d: &jet_std::Duration
//   jet_coll_bisect_left (core.collections.bisect_left): no runtime function
//   jet_coll_bisect_right (core.collections.bisect_right): no runtime function
//   jet_coll_heapify (core.collections.heapify): no runtime function
//   jet_coll_heappop (core.collections.heappop): parameter types  values: &[i64], ) -> ( Vec<jet_foundation::Numeric::JetInt>, JetOutcome<jet_foundation::Numeric::JetInt, JetAbsent>, 
//   jet_coll_heappush (core.collections.heappush): no runtime function
//   jet_coll_heappushpop (core.collections.heappushpop): parameter types  values: &[i64], value: i64, ) -> ( Vec<jet_foundation::Numeric::JetInt>, jet_foundation::Numeric::JetInt, 
//   jet_coll_heapreplace (core.collections.heapreplace): parameter types  values: &[i64], value: i64, ) -> ( Vec<jet_foundation::Numeric::JetInt>, JetOutcome<jet_foundation::Numeric::JetInt, JetAbsent>, 
//   jet_coll_insort_left (core.collections.insort_left): no runtime function
//   jet_coll_insort_right (core.collections.insort_right): no runtime function
//   jet_coll_merge_sorted (core.collections.merge_sorted): no runtime function
//   jet_coll_nlargest (core.collections.nlargest): no runtime function
//   jet_coll_nsmallest (core.collections.nsmallest): no runtime function
//   jet_comb_accumulate (core.math.combinatorics.accumulate): no runtime function
//   jet_comb_batched (core.math.combinatorics.batched): no runtime function
//   jet_comb_cartesian (core.math.combinatorics.cartesian): no runtime function
//   jet_comb_chain (core.math.combinatorics.chain): no runtime function
//   jet_comb_combinations (core.math.combinatorics.combinations): no runtime function
//   jet_comb_combinations_with_replacement (core.math.combinatorics.combinations_with_replacement): no runtime function
//   jet_comb_compress (core.math.combinatorics.compress): no runtime function
//   jet_comb_count_from (core.math.combinatorics.count_from): no runtime function
//   jet_comb_cycle (core.math.combinatorics.cycle): no runtime function
//   jet_comb_drop (core.math.combinatorics.drop): no runtime function
//   jet_comb_dropwhile (core.math.combinatorics.dropwhile): no runtime function
//   jet_comb_filterfalse (core.math.combinatorics.filterfalse): no runtime function
//   jet_comb_flatten (core.math.combinatorics.flatten): no runtime function
//   jet_comb_groupby (core.math.combinatorics.groupby): no runtime function
//   jet_comb_islice (core.math.combinatorics.islice): no runtime function
//   jet_comb_pairwise (core.math.combinatorics.pairwise): no runtime function
//   jet_comb_permutations (core.math.combinatorics.permutations): no runtime function
//   jet_comb_powerset (core.math.combinatorics.powerset): no runtime function
//   jet_comb_product (core.math.combinatorics.product): no runtime function
//   jet_comb_repeat (core.math.combinatorics.repeat): no runtime function
//   jet_comb_reverse (core.math.combinatorics.reverse): no runtime function
//   jet_comb_starmap (core.math.combinatorics.starmap): no runtime function
//   jet_comb_takewhile (core.math.combinatorics.takewhile): no runtime function
//   jet_comb_tee (core.math.combinatorics.tee): no runtime function
//   jet_comb_unique (core.math.combinatorics.unique): no runtime function
//   jet_comb_windows (core.math.combinatorics.windows): no runtime function
//   jet_comb_zip_longest (core.math.combinatorics.zip_longest): no runtime function
//   jet_compute_abs (core.compute.abs): no runtime function
//   jet_compute_add (core.compute.add): no runtime function
//   jet_compute_broadcast_to (core.compute.broadcast_to): no runtime function
//   jet_compute_deserialize (core.compute.deserialize): no runtime function
//   jet_compute_det (core.compute.det): no runtime function
//   jet_compute_device_auto (core.compute.device_auto): no runtime function
//   jet_compute_device_cpu (core.compute.device_cpu): no runtime function
//   jet_compute_device_cuda (core.compute.device_cuda): no runtime function
//   jet_compute_device_metal (core.compute.device_metal): no runtime function
//   jet_compute_device_vulkan (core.compute.device_vulkan): no runtime function
//   jet_compute_device_webgpu (core.compute.device_webgpu): no runtime function
//   jet_compute_div (core.compute.div): no runtime function
//   jet_compute_exp (core.compute.exp): no runtime function
//   jet_compute_eye (core.compute.eye): no runtime function
//   jet_compute_fft (core.compute.fft): no runtime function
//   jet_compute_from_list (core.compute.from_list): no runtime function
//   jet_compute_full (core.compute.full): no runtime function
//   jet_compute_get (core.compute.get): no runtime function
//   jet_compute_inv (core.compute.inv): no runtime function
//   jet_compute_kernel_bounds_ok (core.compute.kernel_bounds_ok): no runtime function
//   jet_compute_log (core.compute.log): no runtime function
//   jet_compute_matmul (core.compute.matmul): no runtime function
//   jet_compute_matmul_f32_tile (core.compute.matmul_f32_tile): no runtime function
//   jet_compute_matrix (core.compute.matrix): no runtime function
//   jet_compute_maximum (core.compute.maximum): no runtime function
//   jet_compute_minimum (core.compute.minimum): no runtime function
//   jet_compute_mse_loss (core.compute.mse_loss): no runtime function
//   jet_compute_mul (core.compute.mul): no runtime function
//   jet_compute_negate (core.compute.negate): no runtime function
//   jet_compute_on_device (core.compute.on_device): no runtime function
//   jet_compute_ones (core.compute.ones): no runtime function
//   jet_compute_profile_f32_strict (core.compute.profile_f32_strict): no runtime function
//   jet_compute_profile_show (core.compute.profile_show): no runtime function
//   jet_compute_reshape (core.compute.reshape): no runtime function
//   jet_compute_serialize (core.compute.serialize): no runtime function
//   jet_compute_sgd_step (core.compute.sgd_step): no runtime function
//   jet_compute_slice_range (core.compute.slice_range): no runtime function
//   jet_compute_solve (core.compute.solve): no runtime function
//   jet_compute_sparse_mv (core.compute.sparse_mv): no runtime function
//   jet_compute_sparse_nnz (core.compute.sparse_nnz): no runtime function
//   jet_compute_sparse_show (core.compute.sparse_show): no runtime function
//   jet_compute_sqrt (core.compute.sqrt): no runtime function
//   jet_compute_stream_new (core.compute.stream_new): no runtime function
//   jet_compute_stream_new_on_device (core.compute.stream_new_on): no runtime function
//   jet_compute_stream_show (core.compute.stream_show): no runtime function
//   jet_compute_stream_sync (core.compute.stream_sync): no runtime function
//   jet_compute_sub (core.compute.sub): no runtime function
//   jet_compute_sum_axis (core.compute.sum_axis): no runtime function
//   jet_compute_tensor_device (core.compute.device): no runtime function
//   jet_compute_tensor_numel (core.compute.numel): no runtime function
//   jet_compute_tensor_placement (core.compute.placement): no runtime function
//   jet_compute_tensor_rank (core.compute.rank): no runtime function
//   jet_compute_tensor_shape (core.compute.shape): no runtime function
//   jet_compute_tensor_to_list (core.compute.to_list): no runtime function
//   jet_compute_to_sparse (core.compute.to_sparse): no runtime function
//   jet_compute_transfer (core.compute.transfer): no runtime function
//   jet_compute_transfer_show (core.compute.transfer_show): no runtime function
//   jet_compute_transpose (core.compute.transpose): no runtime function
//   jet_compute_vec (core.compute.vec): no runtime function
//   jet_compute_view (core.builtin.compute_view_new): no runtime function
//   jet_compute_view_mut (core.builtin.compute_view_mut_new): no runtime function
//   jet_compute_zeros (core.compute.zeros): no runtime function
//   jet_config_merge (core.args.merge): no runtime function
//   jet_cursor_skip_ws (core.handle.cursor.skip_ws): parameter types c: &mut JetCursor
//   jet_cursor_take_until (core.handle.cursor.take_until): parameter types c: &mut JetCursor, delim: &String
//   jet_data_arrow_import (core.data.arrow.import): no runtime function
//   jet_data_bar_text_checked (core.data.bar_text): no runtime function
//   jet_data_count (core.data.count): generic
//   jet_data_csv_reader (core.data.csv_reader): no runtime function
//   jet_data_describe_checked (core.data.describe): no runtime function
//   jet_data_entries_to_map (core.collections.entries_to_map): generic
//   jet_data_json_decode (core.data.json): generic
//   jet_data_json_reader (core.data.json_reader): no runtime function
//   jet_data_line_svg_plot_checked (core.data.line_svg): no runtime function
//   jet_data_line_text_plot_checked (core.data.line_text): no runtime function
//   jet_data_loader_authority (core.data.loader.authority): no runtime function
//   jet_data_loader_bind (core.data.loader.bind): no runtime function
//   jet_data_loader_bind_text (core.data.loader.bind_text): no runtime function
//   jet_data_loader_cancel (core.data.loader.cancel): no runtime function
//   jet_data_loader_database (core.data.database): no runtime function
//   jet_data_loader_file (core.data.file): no runtime function
//   jet_data_loader_file_member (core.data.file_member): no runtime function
//   jet_data_loader_invalidate (core.data.loader.invalidate): no runtime function
//   jet_data_loader_load (core.data.load): no runtime function
//   jet_data_loader_load_default (core.data.load_default): no runtime function
//   jet_data_loader_needs_refresh (core.data.loader.needs_refresh): no runtime function
//   jet_data_loader_offline (core.data.loader.offline): no runtime function
//   jet_data_loader_ready (core.data.loader.ready): no runtime function
//   jet_data_loader_snapshot (core.data.snapshot): no runtime function
//   jet_data_loader_source_identity (core.data.loader.source_identity): no runtime function
//   jet_data_loader_status (core.data.loader.status): no runtime function
//   jet_data_loader_stream (core.data.loader.stream): no runtime function
//   jet_data_loader_url (core.data.url): no runtime function
//   jet_data_loader_value (core.data.value): no runtime function
//   jet_data_mean_checked (core.data.mean): no runtime function
//   jet_data_plot (core.data.plot.plot): no runtime function
//   jet_data_plot_inspect (core.data.inspect): no runtime function
//   jet_data_plot_inspect_json (core.data.inspect_json): no runtime function
//   jet_data_plot_render (core.data.render): no runtime function
//   jet_data_plot_show (core.data.show): no runtime function
//   jet_data_plot_svg (core.data.svg): no runtime function
//   jet_data_plot_text (core.data.text): no runtime function
//   jet_data_quantile_checked (core.data.quantile): no runtime function
//   jet_data_query (core.data.query): no runtime function
//   jet_data_query_arrow (core.data.arrow.query): no runtime function
//   jet_data_require_bridge (core.data.require_bridge): no runtime function
//   jet_data_rolling_mean_checked (core.data.rolling_mean): no runtime function
//   jet_data_snapshot_reusable (core.data.loader.snapshot_reusable): no runtime function
//   jet_data_status (core.data.status): no runtime function
//   jet_data_stream_cancel (core.data.stream.cancel): no runtime function
//   jet_data_stream_collect (core.data.stream.collect): no runtime function
//   jet_data_stream_next (core.data.stream.next): no runtime function
//   jet_data_sum_checked (core.data.sum): no runtime function
//   jet_data_track (core.data.track): no runtime function
//   jet_data_variance_checked (core.data.variance): no runtime function
//   jet_db_begin (core.handle.db.begin): no runtime function
//   jet_db_close (core.handle.db.close): no runtime function
//   jet_db_commit (core.handle.db.commit): no runtime function
//   jet_db_decode (core.db.decode): generic
//   jet_db_lease_close (core.handle.db_lease.close): no runtime function
//   jet_db_open (core.db.open): no runtime function
//   jet_db_open_memory (core.db.open_memory): no runtime function
//   jet_db_policy_allows (core.sync.policy_allows): no runtime function
//   jet_db_policy_audit (core.db.policy_audit): no runtime function
//   jet_db_policy_checked (core.db.policy): no runtime function
//   jet_db_policy_new (core.sync.policy_new): no runtime function
//   jet_db_policy_show (core.sync.policy_show): no runtime function
//   jet_db_pool_acquire (core.handle.db_pool.acquire): no runtime function
//   jet_db_pool_acquire_deadline (core.handle.db_pool.acquire_deadline): no runtime function
//   jet_db_pool_drain (core.handle.db_pool.drain): no runtime function
//   jet_db_pool_new (core.db.pool): no runtime function
//   jet_db_pool_ready (core.handle.db_pool.ready): no runtime function
//   jet_db_pool_receipt (core.handle.db_pool.receipt): no runtime function
//   jet_db_rollback (core.handle.db.rollback): no runtime function
//   jet_db_with_policy (core.handle.db.with_policy): no runtime function
//   jet_deque_capacity (core.builtin.deque_capacity): generic
//   jet_deque_contains (core.builtin.deque_contains): generic
//   jet_deque_delete (core.builtin.deque_delete): generic
//   jet_deque_from (core.builtin.deque_from): generic
//   jet_deque_get (core.builtin.deque_get): generic
//   jet_deque_join (core.builtin.deque_join): generic
//   jet_deque_peek_back (core.builtin.deque_peek_back): generic
//   jet_deque_peek_front (core.builtin.deque_peek_front): generic
//   jet_deque_pop_back_kernel (core.builtin.deque_pop_back): generic
//   jet_deque_pop_front_kernel (core.builtin.deque_pop_front): generic
//   jet_deque_push_back (core.builtin.deque_push_back): generic
//   jet_deque_push_front (core.builtin.deque_push_front): generic
//   jet_deque_reverse (core.builtin.deque_reverse): generic
//   jet_deque_split (core.builtin.deque_split): generic
//   jet_deque_to_list (core.builtin.deque_to_list): generic
//   jet_devserver_app (core.web.devserver.app): no runtime function
//   jet_devserver_for_app (core.web.devserver.for_app): no runtime function
//   jet_devtools_publish (core.devtools.publish): no runtime function
//   jet_duration_divide (core.handle.duration.divide): parameter types d: &jet_std::Duration, factor: &i64
//   jet_duration_round (core.handle.duration.round): parameter types  d: &jet_std::Duration, unit: &String, increment: &i64, mode: &String, 
//   jet_duration_scale (core.handle.duration.scale): parameter types d: &jet_std::Duration, factor: &i64
//   jet_email::address (core.email.address): no runtime function
//   jet_email::attachment (core.email.attachment): no runtime function
//   jet_email::envelope (core.email.envelope): no runtime function
//   jet_email::Limits::safe (core.email.limits_safe): no runtime function
//   jet_email::message (core.email.message): no runtime function
//   jet_email::serialize (core.email.serialize): no runtime function
//   jet_enc_cbor_reader (core.encoding.cbor.reader): parameter types input: JetFileReader, limits: jet_std::EncodingLimits
//   jet_enc_cbor_reader_next (core.handle.cbor_reader.next): parameter types reader: &mut jet_std::CBORReader
//   jet_enc_cbor_to_bytes (core.encoding.cbor.to_bytes): generic
//   jet_enc_cbor_to_bytes_canonical (core.encoding.cbor.to_bytes_canonical): generic
//   jet_enc_cbor_writer (core.encoding.cbor.writer): parameter types output: JetFileWriter, limits: jet_std::EncodingLimits
//   jet_enc_cbor_writer_finish (core.handle.cbor_writer.finish): parameter types writer:&mut jet_std::CBORWriter
//   jet_enc_cbor_writer_flush (core.handle.cbor_writer.flush): parameter types writer:&mut jet_std::CBORWriter
//   jet_enc_cbor_writer_write (core.handle.cbor_writer.write): parameter types writer:&mut jet_std::CBORWriter,event:jet_std::DataEvent
//   jet_enc_csv_decode (core.encoding.csv.decode): generic
//   jet_enc_csv_query (core.encoding.csv.query): no runtime function
//   jet_enc_csv_reader (core.encoding.csv.reader): parameter types  input: JetFileReader, limits: jet_std::EncodingLimits, delimiter: String, header: bool, skip_blank: bool, 
//   jet_enc_csv_reader_next (core.handle.csv_reader.next): parameter types  reader: &mut jet_std::CSVReader, 
//   jet_enc_csv_to_string (core.encoding.csv.to_string): no runtime function
//   jet_enc_csv_writer (core.encoding.csv.writer): parameter types output: JetFileWriter, limits: jet_std::EncodingLimits
//   jet_enc_csv_writer_finish (core.handle.csv_writer.finish): parameter types writer: &mut jet_std::CSVWriter
//   jet_enc_csv_writer_flush (core.handle.csv_writer.flush): parameter types writer: &mut jet_std::CSVWriter
//   jet_enc_csv_writer_write (core.handle.csv_writer.write): parameter types writer: &mut jet_std::CSVWriter, row: Vec<String>
//   jet_enc_json_reader (core.encoding.json.reader): parameter types  input: JetFileReader, limits: jet_std::EncodingLimits, 
//   jet_enc_json_reader_next (core.handle.json_reader.next): parameter types reader: &mut jet_std::JSONReader
//   jet_enc_json_writer (core.encoding.json.writer): parameter types  output: JetFileWriter, limits: jet_std::EncodingLimits, canonical: bool, 
//   jet_enc_json_writer_finish (core.handle.json_writer.finish): parameter types writer: &mut jet_std::JSONWriter
//   jet_enc_json_writer_flush (core.handle.json_writer.flush): parameter types writer: &mut jet_std::JSONWriter
//   jet_enc_json_writer_write (core.handle.json_writer.write): parameter types writer: &mut jet_std::JSONWriter, event: jet_std::DataEvent
//   jet_enc_jsonl_reader (core.encoding.jsonl.reader): parameter types  input: JetFileReader, limits: jet_std::EncodingLimits, 
//   jet_enc_jsonl_reader_next (core.handle.jsonl_reader.next): parameter types  reader: &mut jet_std::JSONLReader, 
//   jet_enc_jsonl_writer (core.encoding.jsonl.writer): parameter types  output: JetFileWriter, limits: jet_std::EncodingLimits, 
//   jet_enc_jsonl_writer_finish (core.handle.jsonl_writer.finish): parameter types writer: &mut jet_std::JSONLWriter
//   jet_enc_jsonl_writer_flush (core.handle.jsonl_writer.flush): parameter types writer: &mut jet_std::JSONLWriter
//   jet_enc_jsonl_writer_write (core.handle.jsonl_writer.write): parameter types  writer: &mut jet_std::JSONLWriter, value: jet_std::DataTree, 
//   jet_enc_toml_to_string (core.encoding.toml.to_string): no runtime function
//   jet_enc_xml_reader (core.encoding.xml.reader): parameter types  input: JetFileReader, limits: jet_std::EncodingLimits, xml: jet_std::XMLParseOptions, 
//   jet_enc_xml_reader_next (core.handle.xml_reader.next): parameter types  reader: &mut jet_std::XMLReader, 
//   jet_enc_xml_writer_finish (core.handle.xml_writer.finish): parameter types writer: &mut jet_std::XMLWriter
//   jet_enc_xml_writer_flush (core.handle.xml_writer.flush): parameter types writer: &mut jet_std::XMLWriter
//   jet_enc_xml_writer_write (core.handle.xml_writer.write): parameter types writer: &mut jet_std::XMLWriter, event: jet_std::DataTree
//   jet_enc_yaml_to_string (core.encoding.yaml.to_string): no runtime function
//   jet_entry_error_exit_jet (core.errors.entry_error_exit): parameter types error: JetErr
//   jet_err_apply_conversion (core.errors.apply_conversion): parameter types mut error: JetErr, source: String, target: String
//   jet_err_with_context_frame (core.errors.err_with_context_frame): parameter types  mut error: JetErr, file: &str, line: u32, column: u32, fn_name: &str, note: String, 
//   jet_expect_snapshot (core.test.expect_snapshot): no runtime function
//   jet_expiring_get (core.handle.expiring.get): generic
//   jet_fake_address (core.handle.fake.address): parameter types fake: &mut jet_std::Fake
//   jet_fake_email (core.handle.fake.email): parameter types fake: &mut jet_std::Fake
//   jet_fake_host (core.handle.fake.host): parameter types fake: &mut jet_std::Fake
//   jet_fake_name (core.handle.fake.name): parameter types fake: &mut jet_std::Fake
//   jet_ffi_callback_boundary (core.ffi.callback_boundary): generic
//   jet_ffi_callback_event_stop_unit (core.ffi.callback_event_stop): no runtime function
//   jet_font_shape (core.font.shape): no runtime function
//   jet_font_system (core.font.system): no runtime function
//   jet_game_assets_image (core.handle.game.assets_image): no runtime function
//   jet_game_assets_sound (core.handle.game.assets_sound): no runtime function
//   jet_game_backend_headless (core.game.backend_headless): no runtime function
//   jet_game_backend_present (core.handle.game.backend_present): no runtime function
//   jet_game_backend_should_continue (core.handle.game.backend_should_continue): no runtime function
//   jet_game_input_bind (core.handle.game.input_bind): no runtime function
//   jet_game_input_pressed (core.handle.game.input_pressed): no runtime function
//   jet_game_replay_record (core.handle.game.replay_record): no runtime function
//   jet_game_scene_component (core.handle.game.scene_component): no runtime function
//   jet_game_scene_new (core.handle.game.scene_new): no runtime function
//   jet_game_scene_on_frame (core.handle.game.scene_on_frame): no runtime function
//   jet_game_scene_query (core.handle.game.scene_query): no runtime function
//   jet_gc_read (core.gc.read): no runtime function
//   jet_get_disjoint_write (core.builtin.get_disjoint_write): generic
//   jet_http_basic_auth (core.http.basic_auth): no runtime function
//   jet_http_bearer_auth (core.http.bearer_auth): no runtime function
//   jet_http_client_get (core.http.get): no runtime function
//   jet_http_client_new_impl (core.handle.http.client_new): no runtime function
//   jet_http_client_post (core.http.client.post): no runtime function
//   jet_http_client_request_new (core.http.client.request): no runtime function
//   jet_http_mux_middleware (core.handle.http_mux.middleware): no runtime function
//   jet_http_parse_request (core.http.parse): no runtime function
//   jet_http_reason_phrase (core.http.reason_phrase): no runtime function
//   jet_http_router_dispatch (core.http.dispatch): no runtime function
//   jet_http_router_new (core.http.router): no runtime function
//   jet_http_server_default (core.http.serve): no runtime function
//   jet_http_server_local_addr (core.handle.http_server.local_addr): no runtime function
//   jet_http_server_serve (core.handle.http_server.serve): no runtime function
//   jet_http_server_shutdown (core.handle.http_server.shutdown): no runtime function
//   jet_http_server_wait (core.handle.http_server.wait): no runtime function
//   jet_http_srv_req_header (core.handle.http.request_header): no runtime function
//   jet_http_srv_req_param (core.handle.http.request_param): no runtime function
//   jet_http_srv_req_trailers (core.handle.http.request_trailers): no runtime function
//   jet_http_srv_response_trailers (core.handle.http.response_trailers): no runtime function
//   jet_iter_enumerate (core.builtin.iter_enumerate): generic
//   jet_iter_indexes (core.builtin.indexes): result type JetIter<i64>
//   jet_iter_intersperse (core.builtin.iter_intersperse): generic
//   jet_iter_next (core.builtin.iter_next): generic
//   jet_iter_split_at (core.builtin.iter_split): generic
//   jet_iter_string_split (core.builtin.string_split): result type JetIter<String>
//   jet_jit_reflect_display (core.reflect.value.display): no runtime function
//   jet_jit_reflect_field_name (core.reflect.field.name): no runtime function
//   jet_jit_reflect_field_value (core.reflect.field.value): no runtime function
//   jet_jit_reflect_fields (core.reflect.value.fields): no runtime function
//   jet_jit_reflect_path (core.reflect.value.path): no runtime function
//   jet_jit_reflect_type_name (core.reflect.value.type_name): no runtime function
//   jet_jit_watch_cancel (core.handle.watch.cancel): no runtime function
//   jet_jit_watch_is_active (core.handle.watch.is_active): no runtime function
//   jet_jit_watch_poll (core.handle.watch.poll): no runtime function
//   jet_jit_watch_summary (core.handle.watch.summary): no runtime function
//   jet_jit_watchset_add (core.handle.watchset.add): no runtime function
//   jet_jit_watchset_poll (core.handle.watchset.poll): no runtime function
//   jet_jit_watchset_summary (core.handle.watchset.summary): no runtime function
//   jet_job_queue_default (core.jobs.queue): no runtime function
//   jet_keep (core.prelude.keep): generic
//   jet_keyed_stream_window (core.handle.stream.window): generic
//   jet_list_binary_search (core.builtin.list_binary_search): generic
//   jet_list_clear (core.builtin.list_clear): generic
//   jet_list_concat (core.builtin.list_concat): generic
//   jet_list_count (core.builtin.list_count): generic
//   jet_list_counts (core.builtin.list_counts): generic
//   jet_list_equal (core.builtin.list_equal): generic
//   jet_list_extend (core.builtin.list_extend): generic
//   jet_list_flatten (core.builtin.list_flatten): generic
//   jet_list_index_of (core.builtin.list_index_of): generic
//   jet_list_insert (core.builtin.list_insert): generic
//   jet_list_join (core.builtin.join): generic
//   jet_list_max (core.builtin.max): generic
//   jet_list_max_float (core.builtin.max_float): generic
//   jet_list_min (core.builtin.min): generic
//   jet_list_min_float (core.builtin.min_float): generic
//   jet_list_pop_kernel (core.builtin.list_pop): generic
//   jet_list_product (core.builtin.product): generic
//   jet_list_push (core.builtin.list_push): generic
//   jet_list_remove_slot (core.builtin.list_remove_slot): generic
//   jet_list_remove_value (core.builtin.list_remove_value): generic
//   jet_list_replace (core.builtin.list_replace): generic
//   jet_list_reverse (core.builtin.list_reverse): generic
//   jet_list_slice (core.builtin.list_slice): generic
//   jet_list_sort (core.builtin.list_sort): generic
//   jet_list_sort_desc (core.builtin.list_sort_desc): generic
//   jet_list_starts_with (core.builtin.list_starts_with): generic
//   jet_list_sum (core.builtin.sum): generic
//   jet_list_sum_fixed_f32 (core.builtin.sum_fixed_f32): generic
//   jet_list_sum_fixed_f64 (core.builtin.sum_fixed_f64): generic
//   jet_list_try_collect (core.builtin.try_collect): generic
//   jet_list_try_new (core.builtin.list_try_new): generic
//   jet_list_try_push (core.builtin.list_try_push): generic
//   jet_list_try_reserve (core.builtin.list_try_reserve): generic
//   jet_list_try_with_capacity (core.builtin.list_try_with_capacity): generic
//   jet_lru_add_new (core.builtin.lru_add_new): no runtime function
//   jet_lru_get (core.builtin.lru_get): no runtime function
//   jet_lru_keys (core.builtin.lru_keys): generic
//   jet_lru_put (core.builtin.lru_put): no runtime function
//   jet_map_add_new (core.builtin.map_add_new): generic
//   jet_map_contains_value (core.builtin.map_contains_value): generic
//   jet_map_equal (core.builtin.map_equal): generic
//   jet_map_first_key (core.builtin.map_first): generic
//   jet_map_from_keys_kernel (core.builtin.map_from_keys): generic
//   jet_map_has_key (core.builtin.map_has_key): generic
//   jet_map_insert (core.builtin.map_insert): generic
//   jet_map_intersection (core.builtin.map_intersection): generic
//   jet_map_keys (core.builtin.map_keys): generic
//   jet_map_max_value_kernel (core.builtin.map_max): generic
//   jet_map_merge (core.builtin.map_merge): generic
//   jet_map_merge_with (core.builtin.map_merge_with): generic
//   jet_map_min_value_kernel (core.builtin.map_min): generic
//   jet_map_pop_first (core.builtin.map_pop_first): generic
//   jet_map_pop_kernel (core.builtin.map_remove): generic
//   jet_map_setdefault (core.builtin.map_setdefault): generic
//   jet_map_slice (core.builtin.map_slice): no runtime function
//   jet_map_top_n (core.builtin.map_top_n): generic
//   jet_map_try_insert (core.builtin.map_try_insert): generic
//   jet_map_update_all (core.builtin.map_update): generic
//   jet_map_values (core.builtin.map_values): generic
//   jet_mod_load (core.mod.load): no runtime function
//   jet_net_dns_a (core.net.dns_a): result type Result<Vec<JetIpAddr>, String>
//   jet_net_dns_a_at (core.net.dns_a_at): result type Result<Vec<JetIpAddr>, String>
//   jet_net_dns_aaaa (core.net.dns_aaaa): result type Result<Vec<JetIpAddr>, String>
//   jet_net_dns_aaaa_at (core.net.dns_aaaa_at): result type Result<Vec<JetIpAddr>, String>
//   jet_net_dns_ptr (core.net.dns_ptr): result type Result<Vec<String>, String>
//   jet_net_dns_srv (core.net.dns_srv): result type Result<Vec<JetDNSSrv>, String>
//   jet_net_dns_srv_at (core.net.dns_srv_at): result type Result<Vec<JetDNSSrv>, String>
//   jet_net_dns_txt (core.net.dns_txt): result type Result<Vec<String>, String>
//   jet_net_dns_txt_at (core.net.dns_txt_at): result type Result<Vec<String>, String>
//   jet_net_set_read_timeout (core.net.set_read_timeout): parameter types stream: &mut JetTCPStream, ms: i64
//   jet_net_set_timeout (core.net.set_timeout): parameter types stream: &mut JetTCPStream, ms: i64
//   jet_net_set_write_timeout (core.net.set_write_timeout): parameter types stream: &mut JetTCPStream, ms: i64
//   jet_net_tcp_close (core.handle.tcp_stream.close): parameter types stream: &mut JetTCPStream
//   jet_net_tcp_read (core.handle.tcp_stream.read): parameter types stream: &mut JetTCPStream
//   jet_net_tcp_read_bytes (core.handle.tcp_stream.read_bytes): parameter types stream: &mut JetTCPStream, limit: i64
//   jet_net_tcp_read_bytes_deadline (core.handle.tcp_stream.read_bytes_deadline): parameter types  stream: &mut JetTCPStream, limit: i64, deadline: &jet_std::Duration, 
//   jet_net_tcp_read_text (core.handle.tcp_stream.read_text): parameter types stream: &mut JetTCPStream, limit: i64
//   jet_net_tcp_read_text_deadline (core.handle.tcp_stream.read_text_deadline): parameter types  stream: &mut JetTCPStream, limit: i64, deadline: &jet_std::Duration, 
//   jet_net_tcp_ready_deadline (core.handle.tcp_stream.ready): parameter types  stream: &mut JetTCPStream, interest: JetNetReadyInterest, deadline: &jet_std::Duration, 
//   jet_net_tcp_reply (core.net.tcp_reply): parameter types  mut stream: JetTCPStream, status: &String, body: &String, 
//   jet_net_tcp_shutdown (core.handle.tcp_stream.shutdown): parameter types stream: &mut JetTCPStream, how: JetNetShutdown
//   jet_net_tcp_write (core.handle.tcp_stream.write): parameter types stream: &mut JetTCPStream, data: &String
//   jet_net_tcp_write_all_bytes (core.handle.tcp_stream.write_all_bytes): parameter types  stream: &mut JetTCPStream, data: &Vec<u8>, 
//   jet_net_tcp_write_all_bytes_deadline (core.handle.tcp_stream.write_all_bytes_deadline): parameter types  stream: &mut JetTCPStream, data: &Vec<u8>, deadline: &jet_std::Duration, 
//   jet_net_tcp_write_bytes (core.handle.tcp_stream.write_bytes): parameter types stream: &mut JetTCPStream, data: &Vec<u8>
//   jet_net_tcp_write_bytes_deadline (core.handle.tcp_stream.write_bytes_deadline): parameter types  stream: &mut JetTCPStream, data: &Vec<u8>, deadline: &jet_std::Duration, 
//   jet_net_tcp_write_text (core.handle.tcp_stream.write_text): parameter types stream: &mut JetTCPStream, text: &String
//   jet_net_tcp_write_text_deadline (core.handle.tcp_stream.write_text_deadline): parameter types  stream: &mut JetTCPStream, text: &String, deadline: &jet_std::Duration, 
//   jet_net_tls_close (core.handle.tls_stream.close): parameter types stream: &mut JetTLSStream
//   jet_net_tls_close_write (core.handle.tls_stream.close_write): parameter types  stream: &mut JetTLSStream, deadline: &jet_std::Duration, 
//   jet_net_tls_read_bytes (core.net.tls.read): parameter types  stream: &mut JetTLSStream, limit: i64, 
//   jet_net_tls_read_bytes_deadline (core.handle.tls_stream.read_deadline): parameter types  stream: &mut JetTLSStream, limit: i64, deadline: &jet_std::Duration, 
//   jet_net_tls_write_all_bytes (core.net.tls.write_all): parameter types  stream: &mut JetTLSStream, data: &Vec<u8>, 
//   jet_net_tls_write_all_bytes_deadline (core.handle.tls_stream.write_all_deadline): parameter types  stream: &mut JetTLSStream, data: &Vec<u8>, deadline: &jet_std::Duration, 
//   jet_net_tls_write_bytes (core.net.tls.write): parameter types  stream: &mut JetTLSStream, data: &Vec<u8>, 
//   jet_net_tls_write_text (core.net.tls.write_text): parameter types  stream: &mut JetTLSStream, text: &String, 
//   jet_net_unix_accept (core.net.unix_accept): platform-gated
//   jet_net_unix_accept_deadline (core.handle.unix_listener.accept_deadline): platform-gated
//   jet_net_unix_close (core.net.unix_close): platform-gated
//   jet_net_unix_connect (core.net.unix_connect): platform-gated
//   jet_net_unix_listen (core.net.unix_listen): platform-gated
//   jet_net_unix_read (core.net.unix_read): platform-gated
//   jet_net_unix_read_bytes_deadline (core.handle.unix_stream.read_deadline): platform-gated
//   jet_net_unix_ready (core.handle.unix_stream.ready): platform-gated
//   jet_net_unix_set_timeout (core.handle.unix_stream.set_timeout): platform-gated
//   jet_net_unix_shutdown (core.net.unix_shutdown): platform-gated
//   jet_net_unix_write_all_bytes (core.net.unix_write_all_bytes): platform-gated
//   jet_net_unix_write_all_bytes_deadline (core.handle.unix_stream.write_all_deadline): platform-gated
//   jet_numeric_float_narrow (core.numeric.float_narrow): result type Result<f32, &'static str>
//   jet_numeric_float_to_int (core.numeric.float_to_int): result type Result<i128, &'static str>
//   jet_numeric_try_from_fixed (core.numeric.fixed_try_from): result type Result<i128, &'static str>
//   jet_ordering_reverse (core.builtin.ordering_reverse): parameter types value: &__jet_Ordering
//   jet_ordering_then (core.builtin.ordering_then): parameter types first: &__jet_Ordering, second: &__jet_Ordering
//   jet_panic (core.index.index_miss): result type !
//   jet_parsed_flag (core.handle.parsed.flag): no runtime function
//   jet_parsed_option (core.handle.parsed.option): no runtime function
//   jet_parsed_option_float (core.handle.parsed.option_float): no runtime function
//   jet_parsed_option_int (core.handle.parsed.option_int): no runtime function
//   jet_parsed_options (core.handle.parsed.options): no runtime function
//   jet_parsed_positional (core.handle.parsed.positional): no runtime function
//   jet_parsed_subcommand (core.handle.parsed.subcommand): no runtime function
//   jet_path_extension (core.handle.path.extension): no runtime function
//   jet_path_from (core.handle.path.from): no runtime function
//   jet_path_home (core.handle.path.home): no runtime function
//   jet_path_is_within (core.handle.path.is_within): no runtime function
//   jet_path_join (core.handle.path.join): no runtime function
//   jet_path_normalize (core.handle.path.normalize): no runtime function
//   jet_path_parent (core.handle.path.parent): no runtime function
//   jet_path_stem (core.handle.path.stem): no runtime function
//   jet_path_to_string (core.handle.path.to_string): no runtime function
//   jet_path_walk (core.handle.path.walk): no runtime function
//   jet_path_write_atomic (core.handle.path.write_atomic): no runtime function
//   jet_priority_queue_from (core.builtin.priority_queue_from): generic
//   jet_priority_queue_peek (core.builtin.priority_queue_peek): generic
//   jet_priority_queue_pop_kernel (core.builtin.priority_queue_pop): generic
//   jet_priority_queue_remove_slot_canonical (core.builtin.priority_queue_remove_slot): no runtime function
//   jet_priority_queue_remove_value (core.builtin.priority_queue_remove_value): generic
//   jet_priority_queue_to_sorted_list (core.builtin.priority_queue_to_sorted_list): generic
//   jet_process_child_exited (core.handle.process.child.exited): no runtime function
//   jet_process_child_id (core.handle.process.child.id): no runtime function
//   jet_process_child_interrupt (core.handle.process.child.interrupt): no runtime function
//   jet_process_child_kill (core.handle.process.child.kill): no runtime function
//   jet_process_child_terminate (core.handle.process.child.terminate): no runtime function
//   jet_process_child_wait (core.handle.process.child.wait): no runtime function
//   jet_process_on_signal (core.process.on_signal): no runtime function
//   jet_process_spec_abilities (core.handle.process.spec.abilities): no runtime function
//   jet_process_spec_arg (core.handle.process.spec.arg): no runtime function
//   jet_process_spec_args_extend (core.handle.process.spec.args_extend): no runtime function
//   jet_process_spec_cpu_time_limit (core.handle.process.spec.cpu_time_limit): no runtime function
//   jet_process_spec_cwd (core.handle.process.spec.cwd): no runtime function
//   jet_process_spec_detached (core.handle.process.spec.detached): no runtime function
//   jet_process_spec_env (core.handle.process.spec.env): no runtime function
//   jet_process_spec_env_clear (core.handle.process.spec.env_clear): no runtime function
//   jet_process_spec_env_remove (core.handle.process.spec.env_remove): no runtime function
//   jet_process_spec_memory_limit (core.handle.process.spec.memory_limit): no runtime function
//   jet_process_spec_open_file_limit (core.handle.process.spec.open_file_limit): no runtime function
//   jet_process_spec_output_limit (core.handle.process.spec.output_limit): no runtime function
//   jet_process_spec_plan (core.handle.process.spec.plan): no runtime function
//   jet_process_spec_run (core.handle.process.spec.run): no runtime function
//   jet_process_spec_run_checked (core.handle.process.spec.run_checked): no runtime function
//   jet_process_spec_spawn (core.handle.process.spec.spawn): no runtime function
//   jet_process_spec_stderr (core.handle.process.spec.stderr): no runtime function
//   jet_process_spec_stdin (core.handle.process.spec.stdin): no runtime function
//   jet_process_spec_stdout (core.handle.process.spec.stdout): no runtime function
//   jet_process_spec_terminal (core.handle.process.spec.terminal): no runtime function
//   jet_process_spec_terminal_with_policy (core.handle.process.spec.terminal_with_policy): no runtime function
//   jet_process_spec_timeout (core.handle.process.spec.timeout): no runtime function
//   jet_process_stdin_close (core.handle.process.stdin_close): no runtime function
//   jet_process_stdin_write (core.handle.process.stdin_write): no runtime function
//   jet_reader_read_f32_be (core.handle.reader.read_f32_be): parameter types r: &mut JetReader
//   jet_reader_read_f32_le (core.handle.reader.read_f32_le): parameter types r: &mut JetReader
//   jet_reader_read_f64_be (core.handle.reader.read_f64_be): parameter types r: &mut JetReader
//   jet_reader_read_f64_le (core.handle.reader.read_f64_le): parameter types r: &mut JetReader
//   jet_reader_read_i16_be (core.handle.reader.read_i16_be): parameter types r: &mut JetReader
//   jet_reader_read_i16_le (core.handle.reader.read_i16_le): parameter types r: &mut JetReader
//   jet_reader_read_i32_be (core.handle.reader.read_i32_be): parameter types r: &mut JetReader
//   jet_reader_read_i32_le (core.handle.reader.read_i32_le): parameter types r: &mut JetReader
//   jet_reader_read_i64_be (core.handle.reader.read_i64_be): parameter types r: &mut JetReader
//   jet_reader_read_i64_le (core.handle.reader.read_i64_le): parameter types r: &mut JetReader
//   jet_reader_read_i8 (core.handle.reader.read_i8): parameter types r: &mut JetReader
//   jet_reader_read_u16_be (core.handle.reader.read_u16_be): parameter types r: &mut JetReader
//   jet_reader_read_u16_le (core.handle.reader.read_u16_le): parameter types r: &mut JetReader
//   jet_reader_read_u32_be (core.handle.reader.read_u32_be): parameter types r: &mut JetReader
//   jet_reader_read_u32_le (core.handle.reader.read_u32_le): parameter types r: &mut JetReader
//   jet_reader_read_u64_be (core.handle.reader.read_u64_be): parameter types r: &mut JetReader
//   jet_reader_read_u64_le (core.handle.reader.read_u64_le): parameter types r: &mut JetReader
//   jet_reader_read_u8 (core.handle.reader.read_u8): parameter types r: &mut JetReader
//   jet_reader_seek (core.handle.reader.seek): parameter types r: &mut JetReader, position: i64
//   jet_reader_skip (core.handle.reader.skip): parameter types r: &mut JetReader, count: i64
//   jet_reader_take (core.handle.reader.take): parameter types r: &mut JetReader, n: i64
//   jet_receipt_attach (receipt.attach): generic
//   jet_ring_csv_parse (core.encoding.csv.parse): result type Result<Vec<Vec<String>>, String>
//   jet_ring_log_debug_fields (core.log.debug_fields): parameter types msg: &String, fields: &Vec<jet_std::LogField>
//   jet_ring_log_error_fields (core.log.error_fields): parameter types msg: &String, fields: &Vec<jet_std::LogField>
//   jet_ring_log_info_fields (core.log.info_fields): parameter types msg: &String, fields: &Vec<jet_std::LogField>
//   jet_ring_log_warn_fields (core.log.warn_fields): parameter types msg: &String, fields: &Vec<jet_std::LogField>
//   jet_rng_bool (core.handle.rng.bool): parameter types r: &mut jet_std::Rng
//   jet_rng_bool_p (core.handle.rng.bool_p): parameter types r: &mut jet_std::Rng, p: f64
//   jet_rng_bytes (core.handle.rng.bytes): parameter types r: &mut jet_std::Rng, n: i64
//   jet_rng_exponential (core.handle.rng.exponential): parameter types r: &mut jet_std::Rng, lambda: f64
//   jet_rng_float (core.handle.rng.float): parameter types r: &mut jet_std::Rng
//   jet_rng_float_range (core.handle.rng.float_range): parameter types r: &mut jet_std::Rng, low: f64, high: f64
//   jet_rng_int (core.handle.rng.int): parameter types r: &mut jet_std::Rng, lo: i64, hi: i64
//   jet_rng_normal (core.handle.rng.normal): parameter types r: &mut jet_std::Rng, mean: f64, stddev: f64
//   jet_rng_pick (core.handle.rng.pick): generic
//   jet_rng_sample (core.handle.rng.sample): generic
//   jet_rng_shuffle (core.handle.rng.shuffle): generic
//   jet_rng_split (core.handle.rng.split): parameter types r: &mut jet_std::Rng
//   jet_rng_weighted_pick (core.handle.rng.weighted_pick): generic
//   jet_rt_callback (core.rt.callback): no runtime function
//   jet_rt_cancel (core.handle.realtime.cancel): no runtime function
//   jet_rt_is_cancelled (core.handle.realtime.is_cancelled): no runtime function
//   jet_rt_next_deadline (core.handle.realtime.next_deadline): no runtime function
//   jet_rt_receipt (core.handle.realtime.receipt): no runtime function
//   jet_set_difference (core.builtin.set_difference): generic
//   jet_set_difference_update (core.builtin.set_difference_update): generic
//   jet_set_equal (core.builtin.set_equal): generic
//   jet_set_first (core.builtin.set_first): generic
//   jet_set_from (core.builtin.set_from): generic
//   jet_set_insert (core.builtin.set_insert): generic
//   jet_set_intersection (core.builtin.set_intersection): generic
//   jet_set_intersection_update (core.builtin.set_intersection_update): generic
//   jet_set_is_disjoint (core.builtin.set_is_disjoint): generic
//   jet_set_is_subset (core.builtin.set_is_subset): generic
//   jet_set_is_superset (core.builtin.set_is_superset): generic
//   jet_set_pop_kernel (core.builtin.set_pop): generic
//   jet_set_remove (core.builtin.set_remove): generic
//   jet_set_replace_kernel (core.builtin.set_replace): generic
//   jet_set_shuffle (core.builtin.set_shuffle): generic
//   jet_set_sort (core.builtin.set_sort): generic
//   jet_set_symmetric_difference (core.builtin.set_symmetric_difference): generic
//   jet_set_symmetric_difference_update (core.builtin.set_symmetric_difference_update): generic
//   jet_set_to_list (core.builtin.set_to_list): generic
//   jet_set_union (core.builtin.set_union): generic
//   jet_set_update (core.builtin.set_update): generic
//   jet_set_values (core.builtin.set_values): generic
//   jet_shared_guard_wait_once (core.shared_guard.wait): parameter types  guard: Option<&JetSharedGuardState>, condition: Option<&std::sync::Arc<JetConditionProtocol>>, waiter: std::sync::Arc<dyn JetConditionWaiter>, 
//   jet_slice_range (core.collections.slice_string_range): generic
//   jet_slice_vec (core.collections.slice_list): generic
//   jet_slice_vec_range (core.collections.slice_list_range): generic
//   jet_solver_require (core.handle.solver.require): parameter types solver: &mut jet_std::Solver, ok: bool
//   jet_sorted_set_difference (core.builtin.sorted_set_difference): generic
//   jet_sorted_set_from (core.builtin.sorted_set_from): generic
//   jet_sorted_set_insert (core.builtin.sorted_set_insert): generic
//   jet_sorted_set_intersection (core.builtin.sorted_set_intersection): generic
//   jet_sorted_set_is_disjoint (core.builtin.sorted_set_is_disjoint): generic
//   jet_sorted_set_is_subset (core.builtin.sorted_set_is_subset): generic
//   jet_sorted_set_is_superset (core.builtin.sorted_set_is_superset): generic
//   jet_sorted_set_remove (core.builtin.sorted_set_remove): generic
//   jet_sorted_set_symmetric_difference (core.builtin.sorted_set_symmetric_difference): generic
//   jet_sorted_set_to_list (core.builtin.sorted_set_to_list): generic
//   jet_sorted_set_union (core.builtin.sorted_set_union): generic
//   jet_split_write (core.builtin.split_write): generic
//   jet_stats_clip (core.math.stats.clip): no runtime function
//   jet_stats_correlation (core.math.stats.correlation): no runtime function
//   jet_stats_count (core.math.stats.count): no runtime function
//   jet_stats_covariance (core.math.stats.covariance): no runtime function
//   jet_stats_covariance_population (core.math.stats.covariance_population): no runtime function
//   jet_stats_cumprod (core.math.stats.cumprod): no runtime function
//   jet_stats_cumsum (core.math.stats.cumsum): no runtime function
//   jet_stats_diff (core.math.stats.diff): no runtime function
//   jet_stats_ewma (core.math.stats.ewma): no runtime function
//   jet_stats_geometric_mean (core.math.stats.geometric_mean): no runtime function
//   jet_stats_harmonic_mean (core.math.stats.harmonic_mean): no runtime function
//   jet_stats_histogram (core.math.stats.histogram): no runtime function
//   jet_stats_iqr (core.math.stats.iqr): no runtime function
//   jet_stats_kde (core.math.stats.kde): no runtime function
//   jet_stats_kde_random (core.math.stats.kde_random): no runtime function
//   jet_stats_kurtosis (core.math.stats.kurtosis): no runtime function
//   jet_stats_mad (core.math.stats.mad): no runtime function
//   jet_stats_max (core.math.stats.max): no runtime function
//   jet_stats_mean (core.math.stats.mean): no runtime function
//   jet_stats_mean_abs_deviation (core.math.stats.mean_abs_deviation): no runtime function
//   jet_stats_median (core.math.stats.median): no runtime function
//   jet_stats_median_grouped (core.math.stats.median_grouped): no runtime function
//   jet_stats_median_high (core.math.stats.median_high): no runtime function
//   jet_stats_median_low (core.math.stats.median_low): no runtime function
//   jet_stats_min (core.math.stats.min): no runtime function
//   jet_stats_mode (core.math.stats.mode): no runtime function
//   jet_stats_moving_average (core.math.stats.moving_average): no runtime function
//   jet_stats_multimode (core.math.stats.multimode): no runtime function
//   jet_stats_percentile (core.math.stats.percentile): no runtime function
//   jet_stats_prod (core.math.stats.prod): no runtime function
//   jet_stats_pstdev (core.math.stats.pstdev): no runtime function
//   jet_stats_pvariance (core.math.stats.pvariance): no runtime function
//   jet_stats_quantile (core.math.stats.quantile): no runtime function
//   jet_stats_quantiles (core.math.stats.quantiles): no runtime function
//   jet_stats_range (core.math.stats.range): no runtime function
//   jet_stats_rank (core.math.stats.rank): no runtime function
//   jet_stats_residuals (core.math.stats.residuals): no runtime function
//   jet_stats_skew (core.math.stats.skew): no runtime function
//   jet_stats_spearman (core.math.stats.spearman): no runtime function
//   jet_stats_stdev (core.math.stats.stdev): no runtime function
//   jet_stats_sum (core.math.stats.sum): no runtime function
//   jet_stats_sumprod (core.math.stats.sumprod): no runtime function
//   jet_stats_variance (core.math.stats.variance): no runtime function
//   jet_stats_weighted_mean (core.math.stats.weighted_mean): no runtime function
//   jet_stats_winsorize (core.math.stats.winsorize): no runtime function
//   jet_stats_zscore (core.math.stats.zscore): no runtime function
//   jet_std_b64_decode (core.encoding.base64.decode): result type Result<Vec<u8>, String>
//   jet_std_b64_decodebytes (core.encoding.hex.a2b_base64): result type Result<Vec<u8>, String>
//   jet_std_b64url_decode (core.encoding.base64.decode_url): result type Result<Vec<u8>, String>
//   jet_std_base32_decode (core.encoding.base32.b32decode): result type Result<Vec<u8>, String>
//   jet_std_base32hex_decode (core.encoding.base32.b32hexdecode): result type Result<Vec<u8>, String>
//   jet_std_binary_iter_unpack (core.encoding.binary.iter_unpack): result type Result<Vec<Vec<i64>>, String>
//   jet_std_binary_pack (core.encoding.binary.pack): result type Result<Vec<u8>, String>
//   jet_std_binary_unpack (core.encoding.binary.unpack): result type Result<Vec<i64>, String>
//   jet_std_crypto_random_bytes_controlled (core.crypto.random.bytes): result type Option<Vec<u8>>
//   jet_std_env_current_dir (core.sys.current_dir): no runtime function
//   jet_std_env_decode (core.sys.decode): no runtime function
//   jet_std_env_get (core.sys.get): no runtime function
//   jet_std_env_home_dir (core.sys.home_dir): no runtime function
//   jet_std_env_set (core.sys.set): no runtime function
//   jet_std_env_unset (core.sys.unset): no runtime function
//   jet_std_env_vars (core.sys.vars): no runtime function
//   jet_std_file_reader_path (core.handle.file_reader.path): no runtime function
//   jet_std_file_reader_read_line (core.handle.file_reader.read_line): no runtime function
//   jet_std_file_writer_flush (core.handle.file_writer.flush): no runtime function
//   jet_std_file_writer_path (core.handle.file_writer.path): no runtime function
//   jet_std_file_writer_write_line (core.handle.file_writer.write_line): no runtime function
//   jet_std_files_append (core.files.append): no runtime function
//   jet_std_files_create (core.files.create): no runtime function
//   jet_std_files_open (core.files.open): no runtime function
//   jet_std_fs_absolute (core.files.absolute): no runtime function
//   jet_std_fs_canonicalize (core.files.canonicalize): no runtime function
//   jet_std_fs_chown (core.files.chown): no runtime function
//   jet_std_fs_fsync (core.files.fsync): no runtime function
//   jet_std_fs_glob (core.files.glob): no runtime function
//   jet_std_fs_hard_link (core.files.hard_link): no runtime function
//   jet_std_fs_is_fifo (core.files.is_fifo): no runtime function
//   jet_std_fs_is_socket (core.files.is_socket): no runtime function
//   jet_std_fs_list_dir (core.files.list_dir): result type Result<Vec<jet_std::DirEntry>, jet_std::IOError>
//   jet_std_fs_lock (core.files.lock): no runtime function
//   jet_std_fs_map (core.files.map): no runtime function
//   jet_std_fs_map_is_empty (core.handle.mapped_file.is_empty): no runtime function
//   jet_std_fs_map_len (core.handle.mapped_file.len): no runtime function
//   jet_std_fs_map_lines_view (core.handle.mapped_file.lines): no runtime function
//   jet_std_fs_map_window_len_view (core.handle.mapped_file.window_len): no runtime function
//   jet_std_fs_map_window_view (core.handle.mapped_file.window): no runtime function
//   jet_std_fs_mktemp_path (core.files.mktemp): no runtime function
//   jet_std_fs_read_at (core.files.read_at): no runtime function
//   jet_std_fs_read_bytes (core.files.read_bytes): no runtime function
//   jet_std_fs_read_link (core.files.read_link): no runtime function
//   jet_std_fs_rename (core.files.rename): no runtime function
//   jet_std_fs_scope (core.files.scope): no runtime function
//   jet_std_fs_set_mode (core.files.set_mode): no runtime function
//   jet_std_fs_stat (core.files.stat): no runtime function
//   jet_std_fs_symlink (core.files.symlink): no runtime function
//   jet_std_fs_temp_dir (core.files.temp_dir): no runtime function
//   jet_std_fs_temp_dir_path (core.files.mkdtemp): no runtime function
//   jet_std_fs_temp_file (core.files.temp_file): no runtime function
//   jet_std_fs_walk (core.files.walk): no runtime function
//   jet_std_fs_walk_files (core.files.walk_files): no runtime function
//   jet_std_fs_walk_parallel (core.files.walk_parallel): no runtime function
//   jet_std_fs_write_at (core.files.write_at): no runtime function
//   jet_std_fs_write_atomic (core.files.write_atomic): no runtime function
//   jet_std_fs_write_bytes (core.files.write_bytes): no runtime function
//   jet_std_hex_decode (core.encoding.hex.decode): result type Result<Vec<u8>, String>
//   jet_std_io_args (core.process.argv): no runtime function
//   jet_std_io_binread (core.term.binread): no runtime function
//   jet_std_io_buffered (core.term.buffered): no runtime function
//   jet_std_io_choose (core.term.choose): no runtime function
//   jet_std_io_confirm (core.term.confirm): no runtime function
//   jet_std_io_eprint (core.term.eprint): no runtime function
//   jet_std_io_input (core.term.input): no runtime function
//   jet_std_io_input_secret (core.term.input_secret): no runtime function
//   jet_std_io_process_args (core.process.args): no runtime function
//   jet_std_io_progress (core.term.progress): no runtime function
//   jet_std_io_progress_iter (core.term.progress_iter): no runtime function
//   jet_std_io_read_all_input (core.term.read_all_input): no runtime function
//   jet_std_io_read_until (core.term.read_until): no runtime function
//   jet_std_io_readline (core.term.readline): no runtime function
//   jet_std_io_stderr (core.term.stderr): no runtime function
//   jet_std_io_stderr_flush (core.handle.stderr.flush): no runtime function
//   jet_std_io_stderr_is_tty (core.handle.stderr.is_tty): no runtime function
//   jet_std_io_stderr_write (core.handle.stderr.write): no runtime function
//   jet_std_io_stderr_write_bytes (core.handle.stderr.write_bytes): no runtime function
//   jet_std_io_stderr_write_line (core.handle.stderr.write_line): no runtime function
//   jet_std_io_stdin (core.term.stdin): no runtime function
//   jet_std_io_stdin_read_line (core.handle.stdin.read_line): no runtime function
//   jet_std_io_stdout (core.term.stdout): no runtime function
//   jet_std_io_stdout_flush (core.handle.stdout.flush): no runtime function
//   jet_std_io_stdout_is_tty (core.handle.stdout.is_tty): no runtime function
//   jet_std_io_stdout_write (core.handle.stdout.write): no runtime function
//   jet_std_io_stdout_write_bytes (core.handle.stdout.write_bytes): no runtime function
//   jet_std_io_stdout_write_line (core.handle.stdout.write_line): no runtime function
//   jet_std_io_style (core.term.style): no runtime function
//   jet_std_io_style_force (core.term.style_force): no runtime function
//   jet_std_io_take (core.term.take): no runtime function
//   jet_std_io_terminal_height (core.term.terminal_height): no runtime function
//   jet_std_io_terminal_width (core.term.terminal_width): no runtime function
//   jet_std_jsonl_count_rows (core.encoding.jsonl.count_rows): result type jet_foundation::Numeric::JetInt
//   jet_std_jsonl_first (core.encoding.jsonl.first): result type Result<Option<jet_std::DataTree>, jet_std::EncodingError>
//   jet_std_jsonl_parse (core.encoding.jsonl.parse): result type Result<Vec<jet_std::DataTree>, jet_std::EncodingError>
//   jet_std_jsonl_render (core.encoding.jsonl.to_string): parameter types rows: &Vec<jet_std::DataTree>
//   jet_std_math_abs_diff (core.math.abs_diff): no runtime function
//   jet_std_math_abs_f64 (core.math.fabs): no runtime function
//   jet_std_math_acos (core.math.acos): no runtime function
//   jet_std_math_acosh (core.math.acosh): no runtime function
//   jet_std_math_asin (core.math.asin): no runtime function
//   jet_std_math_asinh (core.math.asinh): no runtime function
//   jet_std_math_atan (core.math.atan): no runtime function
//   jet_std_math_atan2 (core.math.atan2): no runtime function
//   jet_std_math_atanh (core.math.atanh): no runtime function
//   jet_std_math_binomial (core.math.combinatorics.binomial): no runtime function
//   jet_std_math_cbrt (core.math.cbrt): no runtime function
//   jet_std_math_ceil (core.math.ceil): no runtime function
//   jet_std_math_checked_abs (core.math.checked_abs): no runtime function
//   jet_std_math_checked_add (core.math.checked_add): no runtime function
//   jet_std_math_checked_div (core.math.checked_div): no runtime function
//   jet_std_math_checked_mul (core.math.checked_mul): no runtime function
//   jet_std_math_checked_neg (core.math.checked_neg): no runtime function
//   jet_std_math_checked_pow (core.math.checked_pow): no runtime function
//   jet_std_math_checked_rem (core.math.checked_rem): no runtime function
//   jet_std_math_checked_sub (core.math.checked_sub): no runtime function
//   jet_std_math_clamp_f64 (core.math.clamp_float): no runtime function
//   jet_std_math_cmp (core.math.cmp): no runtime function
//   jet_std_math_copysign (core.math.copysign): no runtime function
//   jet_std_math_cos (core.math.cos): no runtime function
//   jet_std_math_cosh (core.math.cosh): no runtime function
//   jet_std_math_cot (core.math.cot): no runtime function
//   jet_std_math_degrees (core.math.degrees): no runtime function
//   jet_std_math_digits (core.math.digits): no runtime function
//   jet_std_math_dist (core.math.dist): no runtime function
//   jet_std_math_div_mod (core.math.div_mod): no runtime function
//   jet_std_math_div_rem (core.math.div_rem): no runtime function
//   jet_std_math_erf (core.math.erf): no runtime function
//   jet_std_math_erfc (core.math.erfc): no runtime function
//   jet_std_math_exp (core.math.exp): no runtime function
//   jet_std_math_exp_m1 (core.math.exp_m1): no runtime function
//   jet_std_math_exp2 (core.math.exp2): no runtime function
//   jet_std_math_factorial (core.math.factorial): no runtime function
//   jet_std_math_floor (core.math.floor): no runtime function
//   jet_std_math_fma (core.math.fma): no runtime function
//   jet_std_math_fmod (core.math.fmod): no runtime function
//   jet_std_math_fract (core.math.fract): no runtime function
//   jet_std_math_frexp (core.math.frexp): no runtime function
//   jet_std_math_from_bits (core.math.from_bits): no runtime function
//   jet_std_math_fsum (core.math.fsum): no runtime function
//   jet_std_math_gamma (core.math.gamma): no runtime function
//   jet_std_math_gcd_many (core.math.gcd_many): no runtime function
//   jet_std_math_hypot (core.math.hypot): no runtime function
//   jet_std_math_hypot3 (core.math.hypot3): no runtime function
//   jet_std_math_identity_f64 (core.math.real): no runtime function
//   jet_std_math_ilogb (core.math.ilogb): no runtime function
//   jet_std_math_in_range (core.math.in_range): no runtime function
//   jet_std_math_int_pow (core.math.int_pow): no runtime function
//   jet_std_math_inv (core.math.inv): no runtime function
//   jet_std_math_is_canonical (core.math.is_canonical): no runtime function
//   jet_std_math_is_even (core.math.even): no runtime function
//   jet_std_math_is_finite (core.math.is_finite): no runtime function
//   jet_std_math_is_infinite (core.math.isinf): no runtime function
//   jet_std_math_is_integer (core.math.is_integer): no runtime function
//   jet_std_math_is_nan (core.math.isnan): no runtime function
//   jet_std_math_is_normal (core.math.is_normal): no runtime function
//   jet_std_math_is_odd (core.math.odd): no runtime function
//   jet_std_math_is_signed (core.math.is_signed): no runtime function
//   jet_std_math_is_subnormal (core.math.is_subnormal): no runtime function
//   jet_std_math_is_zero (core.math.is_zero): no runtime function
//   jet_std_math_isclose (core.math.isclose): no runtime function
//   jet_std_math_lcm_many (core.math.lcm_many): no runtime function
//   jet_std_math_ldexp (core.math.ldexp): no runtime function
//   jet_std_math_leading_ones (core.math.leading_ones): no runtime function
//   jet_std_math_lerp (core.math.lerp): no runtime function
//   jet_std_math_lgamma (core.math.lgamma): no runtime function
//   jet_std_math_ln (core.math.ln): no runtime function
//   jet_std_math_ln_1p (core.math.ln_1p): no runtime function
//   jet_std_math_log (core.math.log): no runtime function
//   jet_std_math_log10 (core.math.log10): no runtime function
//   jet_std_math_log2 (core.math.log2): no runtime function
//   jet_std_math_logb (core.math.logb): no runtime function
//   jet_std_math_max_f64 (core.math.max_float): no runtime function
//   jet_std_math_midpoint (core.math.midpoint): no runtime function
//   jet_std_math_min_f64 (core.math.min_float): no runtime function
//   jet_std_math_modf (core.math.modf): no runtime function
//   jet_std_math_multinomial (core.math.combinatorics.multinomial): no runtime function
//   jet_std_math_next_after (core.math.next_after): no runtime function
//   jet_std_math_next_down (core.math.next_down): no runtime function
//   jet_std_math_next_up (core.math.next_up): no runtime function
//   jet_std_math_perm (core.math.combinatorics.permutations_count): no runtime function
//   jet_std_math_pi (core.math.pi): no runtime function
//   jet_std_math_pow (core.math.pow): no runtime function
//   jet_std_math_powmod (core.math.powmod): no runtime function
//   jet_std_math_prod_int (core.math.prod): no runtime function
//   jet_std_math_prod_int_ref (core.math.prod_int): no runtime function
//   jet_std_math_radians (core.math.radians): no runtime function
//   jet_std_math_radix (core.math.radix): no runtime function
//   jet_std_math_remainder (core.math.remainder): no runtime function
//   jet_std_math_rising_factorial (core.math.combinatorics.rising_factorial): no runtime function
//   jet_std_math_round (core.math.round): no runtime function
//   jet_std_math_saturating_add (core.math.saturating_add): no runtime function
//   jet_std_math_saturating_mul (core.math.saturating_mul): no runtime function
//   jet_std_math_saturating_sub (core.math.saturating_sub): no runtime function
//   jet_std_math_sign_bit (core.math.sign_bit): no runtime function
//   jet_std_math_significand (core.math.significand): no runtime function
//   jet_std_math_signum (core.math.signum): no runtime function
//   jet_std_math_sin (core.math.sin): no runtime function
//   jet_std_math_sin_cos (core.math.sin_cos): no runtime function
//   jet_std_math_sinh (core.math.sinh): no runtime function
//   jet_std_math_sum_int (core.math.sum_int): no runtime function
//   jet_std_math_sumprod (core.math.sumprod): no runtime function
//   jet_std_math_tan (core.math.tan): no runtime function
//   jet_std_math_tanh (core.math.tanh): no runtime function
//   jet_std_math_tau (core.math.tau_const): no runtime function
//   jet_std_math_to_bits (core.math.to_bits): no runtime function
//   jet_std_math_trailing_ones (core.math.trailing_ones): no runtime function
//   jet_std_math_trunc (core.math.trunc): no runtime function
//   jet_std_math_ulp (core.math.ulp): no runtime function
//   jet_std_math_xor (core.math.xor): no runtime function
//   jet_std_math_zero (core.math.zero): no runtime function
//   jet_std_math_zero_f64 (core.math.imag): no runtime function
//   jet_std_os_arch (core.sys.arch): no runtime function
//   jet_std_os_atexit (core.sys.atexit): generic
//   jet_std_os_close_fd (core.sys.close_fd): no runtime function
//   jet_std_os_cpu_count (core.sys.cpu_count): no runtime function
//   jet_std_os_executable (core.sys.executable): no runtime function
//   jet_std_os_exitcode (core.sys.exitcode): no runtime function
//   jet_std_os_expand (core.sys.expand): no runtime function
//   jet_std_os_family (core.sys.family): no runtime function
//   jet_std_os_fork (core.sys.fork): no runtime function
//   jet_std_os_getegid (core.sys.getegid): no runtime function
//   jet_std_os_geteuid (core.sys.geteuid): no runtime function
//   jet_std_os_getgid (core.sys.getgid): no runtime function
//   jet_std_os_getgroups (core.sys.getgroups): no runtime function
//   jet_std_os_getpgid (core.sys.getpgid): no runtime function
//   jet_std_os_getpgrp (core.sys.getpgrp): no runtime function
//   jet_std_os_getppid (core.sys.getppid): no runtime function
//   jet_std_os_getpriority (core.sys.getpriority): no runtime function
//   jet_std_os_getsid (core.sys.getsid): no runtime function
//   jet_std_os_getuid (core.sys.getuid): no runtime function
//   jet_std_os_hostname (core.sys.hostname): no runtime function
//   jet_std_os_initgroups (core.sys.initgroups): no runtime function
//   jet_std_os_kill (core.sys.kill): no runtime function
//   jet_std_os_loadavg (core.sys.loadavg): no runtime function
//   jet_std_os_mkfifo (core.sys.mkfifo): no runtime function
//   jet_std_os_name (core.sys.name): no runtime function
//   jet_std_os_pid (core.sys.pid): no runtime function
//   jet_std_os_pipe (core.sys.pipe): no runtime function
//   jet_std_os_release (core.sys.release): no runtime function
//   jet_std_os_set_current_dir (core.sys.set_current_dir): no runtime function
//   jet_std_os_setgid (core.sys.setgid): no runtime function
//   jet_std_os_setpgid (core.sys.setpgid): no runtime function
//   jet_std_os_setpgrp (core.sys.setpgrp): no runtime function
//   jet_std_os_setpriority (core.sys.setpriority): no runtime function
//   jet_std_os_setsid (core.sys.setsid): no runtime function
//   jet_std_os_setuid (core.sys.setuid): no runtime function
//   jet_std_os_stop (core.sys.stop): no runtime function
//   jet_std_os_sync (core.sys.sync): no runtime function
//   jet_std_os_temp_dir (core.sys.temp_dir): no runtime function
//   jet_std_os_times (core.sys.times): no runtime function
//   jet_std_os_umask (core.sys.umask): no runtime function
//   jet_std_os_uptime (core.sys.uptime): no runtime function
//   jet_std_os_username (core.sys.username): no runtime function
//   jet_std_os_utime (core.sys.utime): no runtime function
//   jet_std_os_version (core.sys.version): no runtime function
//   jet_std_os_wait (core.sys.wait): no runtime function
//   jet_std_os_waitpid (core.sys.waitpid): no runtime function
//   jet_std_process_cmd (core.process.cmd): no runtime function
//   jet_std_process_exit (core.process.exit): no runtime function
//   jet_std_process_pipeline (core.process.pipeline): no runtime function
//   jet_std_process_spec_under (core.handle.process.spec.under): no runtime function
//   jet_std_random_float (core.math.random): no runtime function
//   jet_std_random_getrandbits (core.math.random.getrandbits): no runtime function
//   jet_std_random_seed (core.math.random.seed): no runtime function
//   jet_std_random_split (core.math.random.split): no runtime function
//   jet_std_toml_parse (core.encoding.toml.parse): no runtime function
//   jet_std_xml_attribute (core.encoding.xml.attribute): result type Result<JetOutcome<String, JetAbsent>, jet_std::XMLError>
//   jet_std_xml_content (core.encoding.xml.content): result type Result<Vec<jet_std::DataTree>, jet_std::XMLError>
//   jet_std_xml_to_bytes (core.encoding.xml.to_bytes): parameter types  d: &jet_std::DataTree, options: jet_std::XMLRenderOptions, 
//   jet_std_yaml_parse (core.encoding.yaml.parse): no runtime function
//   jet_std::after_value (core.tasks.after): generic
//   jet_std::FieldError::under (core.encoding.decode_under): no runtime function
//   jet_std::interval (core.tasks.interval): result type JetReceiver<i64>
//   jet_std::jet_int_checked_fixed (core.numeric.int_checked_fixed): result type i128
//   jet_std::jet_int_try_from_checked (core.numeric.int_try_from): result type Result<i128, String>
//   jet_std::jet_regex_matches (core.regex.matches): result type Vec<JetRegexMatch>
//   jet_std::jet_unit_conversion_exact_measurement (core.units.conversion_exact_measurement): result type Option<JetMeasurement<f64>>
//   jet_std::jet_unit_conversion_rounded_measurement (core.units.conversion_rounded_measurement): result type Result<JetMeasurement<f64>, &'static str>
//   jet_std::JetAsyncEvent::new (core.event.async_result): no runtime function
//   jet_std::JetEvent::new (core.event.new): no runtime function
//   jet_std::JetEvent::with_policy (core.event.with_policy): no runtime function
//   jet_std::JetEventPolicy::sync (core.event.policy_sync): no runtime function
//   jet_std::JetEventScope::new (core.event.scope): no runtime function
//   jet_std::JetHook::new (core.event.hook): no runtime function
//   jet_std::JetReceiver::delay_ms (core.channels.receiver.delay_ms): no runtime function
//   jet_std::JetReceiver::is_cancelled (core.channels.receiver.is_cancelled): no runtime function
//   jet_std::JetReceiver::is_interval (core.channels.receiver.is_interval): no runtime function
//   jet_std::JetReceiver::is_ready (core.channels.receiver.is_ready): no runtime function
//   jet_std::JetReceiver::is_timer (core.channels.receiver.is_timer): no runtime function
//   jet_std::JetReceiver::receive (core.channels.receiver.receive): no runtime function
//   jet_std::JetReceiver::try_receive (core.channels.receiver.try_receive): no runtime function
//   jet_std::JetRegexMatch::group (core.builtin.match_group): no runtime function
//   jet_std::JetSender::send (core.channels.sender.send): no runtime function
//   jet_std::JetSignal::new (core.reactive.signal): no runtime function
//   jet_std::JetTask::cancel (core.handle.task.cancel): no runtime function
//   jet_std::JetTask::detach (core.tasks.detach): no runtime function
//   jet_std::JetTask::pause (core.handle.task.pause): no runtime function
//   jet_std::JetTask::resume (core.handle.task.resume): no runtime function
//   jet_stream_key_by (core.handle.stream.key_by): generic
//   jet_stream_with_event_time (core.handle.stream.with_event_time): generic
//   jet_stream_with_event_time_ns (core.handle.stream.with_event_time.datetime): generic
//   jet_string_after_view (core.builtin.string_after_view): generic
//   jet_string_before_view (core.builtin.string_before_view): generic
//   jet_string_try_push (core.builtin.string_try_push): parameter types text: &mut String, addition: &str
//   jet_sync_counter_merge (core.sync.counter_merge): no runtime function
//   jet_sync_counter_value (core.sync.counter_value): no runtime function
//   jet_sync_list_merge (core.sync.list_merge): no runtime function
//   jet_sync_list_new (core.sync.list_new): no runtime function
//   jet_sync_list_push (core.sync.list_push): no runtime function
//   jet_sync_list_show (core.sync.list_show): no runtime function
//   jet_sync_map_get (core.sync.map_get): no runtime function
//   jet_sync_map_merge (core.sync.map_merge): no runtime function
//   jet_sync_map_new (core.sync.map_new): no runtime function
//   jet_sync_map_show (core.sync.map_show): no runtime function
//   jet_sync_text_edit (core.sync.text_edit): no runtime function
//   jet_sync_text_merge (core.sync.text_merge): no runtime function
//   jet_sync_text_metadata (core.sync.text_metadata): no runtime function
//   jet_sync_text_show (core.sync.text_show): no runtime function
//   jet_terminal_session_resize (core.handle.terminal.resize): no runtime function
//   jet_testing_compare (core.testing.compare): parameter types  cases: &Vec<jet_std::DataTree>, reference: Box<dyn Fn(jet_std::DataTree) -> jet_std::DataTree>, candidate: Box<dyn Fn(jet_std::DataTree) -> jet_std::DataTree>, relation: &String, 
//   jet_testing_corpus (core.testing.corpus): no runtime function
//   jet_testing_fixture (core.testing.fixture): no runtime function
//   jet_testing_golden (core.testing.golden): no runtime function
//   jet_testing_histories (core.testing.histories): generic
//   jet_testing_snap (core.testing.snap): no runtime function
//   jet_testing_temp_dir (core.testing.temp_dir): no runtime function
//   jet_testing_world (core.testing.world): generic
//   jet_text_byte_views (core.text.byte_views): generic
//   jet_text_grapheme_views (core.text.grapheme_views): generic
//   jet_text_line_views (core.text.line_views): generic
//   jet_text_parse_kv (core.text.parse.parse_kv): parameter types  text: &String, separator: &String, ) -> (String, bool, String
//   jet_text_parse_partition (core.text.parse.partition): parameter types  text: &String, separator: &String, ) -> (String, String, String
//   jet_text_parse_rpartition (core.text.parse.rpartition): parameter types  text: &String, separator: &String, ) -> (String, String, String
//   jet_text_parse_split_once (core.text.parse.split_once): parameter types  text: &String, separator: &String, ) -> (bool, String, String
//   jet_text_word_views (core.text.word_views): generic
//   jet_tls_client_config_with_alpn (core.handle.tls.config_with_alpn): parameter types  mut config: JetTLSClientConfig, protocols: &Vec<String>, 
//   jet_tls_client_config_with_client_identity (core.handle.tls.config_with_identity): parameter types  mut config: JetTLSClientConfig, identity: &JetTLSClientIdentity, 
//   jet_tls_client_config_with_trust (core.handle.tls.config_with_trust): parameter types  mut config: JetTLSClientConfig, trust: JetTLSTrust, 
//   jet_tls_client_config_with_version_bounds (core.handle.tls.config_with_version_bounds): parameter types  mut config: JetTLSClientConfig, min: JetTLSVersion, max: JetTLSVersion, 
//   jet_tls_client_identity_from_pem (core.handle.tls.client_identity_from_pem): parameter types  cert_chain: &Vec<u8>, private_key: &Vec<u8>, validate: fn(&Vec<u8>, &Vec<u8>) -> Result<(), String>, 
//   jet_tls_root_certificates_from_pem (core.handle.tls.root_certificates_from_pem): parameter types  pem: &Vec<u8>, validate: fn(&Vec<u8>) -> Result<(), String>, 
//   jet_tui_ascii (core.tui.ascii): no runtime function
//   jet_tui_capabilities (core.tui.capabilities): no runtime function
//   jet_tui_close_event (core.tui.close_event): no runtime function
//   jet_tui_color_ansi16 (core.tui.color_ansi16): no runtime function
//   jet_tui_color_ansi256 (core.tui.color_ansi256): no runtime function
//   jet_tui_color_rgb (core.tui.color_rgb): no runtime function
//   jet_tui_display_width_int (core.tui.display_width): no runtime function
//   jet_tui_fill (core.tui.fill): no runtime function
//   jet_tui_focus_event (core.tui.focus_event): no runtime function
//   jet_tui_horizontal (core.tui.horizontal): no runtime function
//   jet_tui_interrupt_event (core.tui.interrupt_event): no runtime function
//   jet_tui_io_event (core.tui.io_event): no runtime function
//   jet_tui_key_event (core.tui.key_event): no runtime function
//   jet_tui_key_event_modifiers (core.tui.key_event_modifiers): no runtime function
//   jet_tui_layout (core.tui.layout): no runtime function
//   jet_tui_length (core.tui.length): no runtime function
//   jet_tui_list (core.tui.list): no runtime function
//   jet_tui_list_state (core.tui.list_state): no runtime function
//   jet_tui_list_state_offset (core.tui.list_state_offset): no runtime function
//   jet_tui_list_state_select (core.tui.list_state_select): no runtime function
//   jet_tui_list_state_selected (core.tui.list_state_selected): no runtime function
//   jet_tui_max (core.tui.max): no runtime function
//   jet_tui_min (core.tui.min): no runtime function
//   jet_tui_percent (core.tui.percent): no runtime function
//   jet_tui_resize_event (core.tui.resize_event): no runtime function
//   jet_tui_style (core.tui.style): no runtime function
//   jet_tui_style_background (core.tui.style_background): no runtime function
//   jet_tui_style_bold (core.tui.style_bold): no runtime function
//   jet_tui_style_dim (core.tui.style_dim): no runtime function
//   jet_tui_style_foreground (core.tui.style_foreground): no runtime function
//   jet_tui_style_text (core.tui.style_text): no runtime function
//   jet_tui_style_underline (core.tui.style_underline): no runtime function
//   jet_tui_table (core.tui.table): no runtime function
//   jet_tui_timer_event (core.tui.timer_event): no runtime function
//   jet_tui_vertical (core.tui.vertical): no runtime function
//   jet_ui_aria_role_button (core.ui.aria_role_button): no runtime function
//   jet_ui_aria_role_container (core.ui.aria_role_container): no runtime function
//   jet_ui_aria_role_label (core.ui.aria_role_label): no runtime function
//   jet_ui_aria_role_text_input (core.ui.aria_role_text_input): no runtime function
//   jet_ui_button (core.ui.button): no runtime function
//   jet_ui_constraint (core.ui.constraint): no runtime function
//   jet_ui_desktop (core.ui.desktop): no runtime function
//   jet_ui_gtk (core.ui.gtk_backend): no runtime function
//   jet_ui_host_accessibility (core.ui.host.accessibility): no runtime function
//   jet_ui_host_attach_accessibility (core.ui.host.accessibility.attach): no runtime function
//   jet_ui_host_capabilities (core.ui.host.capabilities): no runtime function
//   jet_ui_host_clipboard_read_text (core.ui.host.clipboard.read_text): no runtime function
//   jet_ui_host_clipboard_write_text (core.ui.host.clipboard.write_text): no runtime function
//   jet_ui_host_drag_poll (core.ui.host.drag_drop.poll): no runtime function
//   jet_ui_host_file_filter (core.ui.host.file_filter): no runtime function
//   jet_ui_host_file_filter_text (core.ui.host.file_filter_text): no runtime function
//   jet_ui_host_fs_grant (core.ui.host.fs_grant): no runtime function
//   jet_ui_host_fs_rights_read (core.ui.host.fs_rights_read): no runtime function
//   jet_ui_host_fs_rights_read_write (core.ui.host.fs_rights_read_write): no runtime function
//   jet_ui_host_fs_rights_write (core.ui.host.fs_rights_write): no runtime function
//   jet_ui_host_ime_poll (core.ui.host.ime.poll): no runtime function
//   jet_ui_host_open_file (core.ui.host.open_file): no runtime function
//   jet_ui_host_open_request (core.ui.host.open_request): no runtime function
//   jet_ui_host_project_accessibility (core.ui.host.accessibility.project): no runtime function
//   jet_ui_host_save_file (core.ui.host.save_file): no runtime function
//   jet_ui_host_save_request (core.ui.host.save_request): no runtime function
//   jet_ui_host_shortcut (core.ui.host.shortcut): no runtime function
//   jet_ui_host_shortcut_binding (core.ui.host.shortcuts.binding): no runtime function
//   jet_ui_host_shortcuts_dispatch (core.ui.host.shortcuts.dispatch): no runtime function
//   jet_ui_host_shortcuts_register (core.ui.host.shortcuts.register): no runtime function
//   jet_ui_key_event (core.ui.key_event): no runtime function
//   jet_ui_node (core.ui.node): no runtime function
//   jet_ui_node_accessibility (core.ui.node_accessibility): no runtime function
//   jet_ui_node_color (core.ui.node_color): no runtime function
//   jet_ui_node_role (core.ui.node_role): no runtime function
//   jet_ui_node_shortcut (core.ui.node_shortcut): no runtime function
//   jet_ui_null (core.ui.null_backend): no runtime function
//   jet_ui_phone (core.ui.phone): no runtime function
//   jet_ui_playground_with_viewport (core.ui.playground): no runtime function
//   jet_ui_playgrounds (core.ui.playgrounds): no runtime function
//   jet_ui_point (core.ui.point): no runtime function
//   jet_ui_preview_with_viewport (core.ui.preview): no runtime function
//   jet_ui_previews (core.ui.previews): no runtime function
//   jet_ui_reactive_render (core.ui.reactive_render): no runtime function
//   jet_ui_rect (core.ui.rect): no runtime function
//   jet_ui_resize_event (core.ui.resize_event): no runtime function
//   jet_ui_size (core.ui.size): no runtime function
//   jet_ui_tablet (core.ui.tablet): no runtime function
//   jet_ui_text (core.ui.text): no runtime function
//   jet_ui_text_input (core.ui.text_input): no runtime function
//   jet_ui_tui (core.ui.tui_backend): no runtime function
//   jet_unicode_cut_last (core.builtin.string_cut_last): result type jet_foundation::Outcome::JetOutcome< (String, String), jet_foundation::Outcome::JetAbsent, >
//   jet_unicode_split_once (core.builtin.string_split_once): result type jet_foundation::Outcome::JetOutcome< (String, String), jet_foundation::Outcome::JetAbsent, >
//   jet_unicode_trim_view (core.builtin.string_trim_view): result type &str
//   jet_unit_conversion_rounded (core.units.conversion_rounded): result type Result<f64, &'static str>
//   jet_url_from_parts (core.net.url.from_parts): parameter types  scheme: &String, host: &String, path: &String, query: &Vec<Vec<String>>, fragment: &String, 
//   jet_url_parse_qsl (core.net.url.parse_qsl): result type Vec<Vec<String>>
//   jet_url_query (core.net.url.query): parameter types pairs: &Vec<Vec<String>>
//   jet_url_split_fragment (core.net.url.split_fragment): parameter types text: &String) -> (String, String
//   jet_url_unquote_to_bytes (core.net.url.unquote_to_bytes): result type Result<Vec<u8>, String>
//   jet_url_urldefrag (core.net.url.urldefrag): parameter types text: &String) -> (String, String
//   jet_url_urlencode (core.net.url.urlencode): parameter types pairs: &Vec<Vec<String>>
//   jet_view_mut_new (core.builtin.view_mut_new): generic
//   jet_view_new (core.builtin.view_new): generic
//   jet_watcher_files (core.watcher.files): no runtime function
//   jet_watcher_port (core.watcher.port): no runtime function
//   jet_watcher_process_pid (core.watcher.process_pid): no runtime function
//   jet_watcher_set (core.watcher.set): no runtime function
//   jet_web_virtual_window_facts (core.web.virtual.facts_json): no runtime function
//   jet_ws_close (core.handle.ws.close): no runtime function
//   jet_ws_message_is_text (core.handle.ws.message_is_text): no runtime function
//   jet_ws_message_text (core.handle.ws.message_text): no runtime function
//   jet_ws_recv (core.handle.ws.recv): no runtime function
//   jet_ws_send_text (core.handle.ws.send_text): no runtime function
//   JetByteBuffer::capacity (core.builtin.byte_buffer_capacity): no runtime function
//   JetByteBuffer::from (core.builtin.byte_buffer_from): no runtime function
//   JetByteBuffer::to_bytes (core.builtin.byte_buffer_to_bytes): no runtime function
//   JetByteBuffer::with_capacity (core.builtin.byte_buffer_with_capacity): no runtime function
//   JetDate::parse_iso_week_date (core.time.parse_iso_week_date): no runtime function
// END GENERATED C-ABI ROUTES
