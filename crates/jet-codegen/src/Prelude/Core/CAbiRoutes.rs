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
    pub extern "C" fn jet_rt_handle_drop_Address(value: *mut crate::jet_email::Address) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_Address(value: *mut crate::jet_email::Address) -> *mut crate::jet_email::Address {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_Attachment(value: *mut crate::jet_email::Attachment) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_Attachment(value: *mut crate::jet_email::Attachment) -> *mut crate::jet_email::Attachment {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_CBORError(value: *mut crate::jet_std::CBORError) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_CBORError(value: *mut crate::jet_std::CBORError) -> *mut crate::jet_std::CBORError {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_CBORErrorKind(value: *mut crate::jet_std::CBORErrorKind) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_CBORErrorKind(value: *mut crate::jet_std::CBORErrorKind) -> *mut crate::jet_std::CBORErrorKind {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_CBOROptions(value: *mut crate::jet_std::CBOROptions) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_CBOROptions(value: *mut crate::jet_std::CBOROptions) -> *mut crate::jet_std::CBOROptions {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_CBORWriter(value: *mut crate::jet_std::CBORWriter) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_Clock(value: *mut crate::jet_std::Clock) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_Clock(value: *mut crate::jet_std::Clock) -> *mut crate::jet_std::Clock {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_Closed(value: *mut crate::jet_std::Closed) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_Closed(value: *mut crate::jet_std::Closed) -> *mut crate::jet_std::Closed {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_Constraint(value: *mut crate::jet_layout::Constraint) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_Constraint(value: *mut crate::jet_layout::Constraint) -> *mut crate::jet_layout::Constraint {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_CSVReader(value: *mut crate::jet_std::CSVReader) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_CSVRow(value: *mut crate::jet_std::CSVRow) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_CSVRow(value: *mut crate::jet_std::CSVRow) -> *mut crate::jet_std::CSVRow {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_CSVWriter(value: *mut crate::jet_std::CSVWriter) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_DataAuthority(value: *mut crate::jet_std::DataAuthority) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DataAuthority(value: *mut crate::jet_std::DataAuthority) -> *mut crate::jet_std::DataAuthority {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_DataColumn(value: *mut crate::jet_std::DataColumn) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DataColumn(value: *mut crate::jet_std::DataColumn) -> *mut crate::jet_std::DataColumn {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_DataError(value: *mut crate::jet_std::DataError) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DataError(value: *mut crate::jet_std::DataError) -> *mut crate::jet_std::DataError {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_DataErrorKind(value: *mut crate::jet_std::DataErrorKind) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DataErrorKind(value: *mut crate::jet_std::DataErrorKind) -> *mut crate::jet_std::DataErrorKind {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_DataEvent(value: *mut crate::jet_std::DataEvent) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DataEvent(value: *mut crate::jet_std::DataEvent) -> *mut crate::jet_std::DataEvent {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_DataFormat(value: *mut crate::jet_std::DataFormat) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DataFormat(value: *mut crate::jet_std::DataFormat) -> *mut crate::jet_std::DataFormat {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_DataFreshness(value: *mut crate::jet_std::DataFreshness) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DataFreshness(value: *mut crate::jet_std::DataFreshness) -> *mut crate::jet_std::DataFreshness {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_DataInvalidationCause(value: *mut crate::jet_std::DataInvalidationCause) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DataInvalidationCause(value: *mut crate::jet_std::DataInvalidationCause) -> *mut crate::jet_std::DataInvalidationCause {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_DataLimits(value: *mut crate::jet_std::DataLimits) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DataLimits(value: *mut crate::jet_std::DataLimits) -> *mut crate::jet_std::DataLimits {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_DataLineOptions(value: *mut crate::jet_std::DataLineOptions) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DataLineOptions(value: *mut crate::jet_std::DataLineOptions) -> *mut crate::jet_std::DataLineOptions {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_DataLoaderKind(value: *mut crate::jet_std::DataLoaderKind) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DataLoaderKind(value: *mut crate::jet_std::DataLoaderKind) -> *mut crate::jet_std::DataLoaderKind {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_DataLoaderStatus(value: *mut crate::jet_std::DataLoaderStatus) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DataLoaderStatus(value: *mut crate::jet_std::DataLoaderStatus) -> *mut crate::jet_std::DataLoaderStatus {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_DataPivotCell(value: *mut crate::jet_std::DataPivotCell) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DataPivotCell(value: *mut crate::jet_std::DataPivotCell) -> *mut crate::jet_std::DataPivotCell {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_DataProvenance(value: *mut crate::jet_std::DataProvenance) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DataProvenance(value: *mut crate::jet_std::DataProvenance) -> *mut crate::jet_std::DataProvenance {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_DataSnapshotIdentity(value: *mut crate::jet_std::DataSnapshotIdentity) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DataSnapshotIdentity(value: *mut crate::jet_std::DataSnapshotIdentity) -> *mut crate::jet_std::DataSnapshotIdentity {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_DataSourceIdentity(value: *mut crate::jet_std::DataSourceIdentity) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DataSourceIdentity(value: *mut crate::jet_std::DataSourceIdentity) -> *mut crate::jet_std::DataSourceIdentity {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_DataStatus(value: *mut crate::jet_std::DataStatus) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DataStatus(value: *mut crate::jet_std::DataStatus) -> *mut crate::jet_std::DataStatus {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_DataStream(value: *mut crate::jet_std::DataStream) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_DataSummary(value: *mut crate::jet_std::DataSummary) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DataSummary(value: *mut crate::jet_std::DataSummary) -> *mut crate::jet_std::DataSummary {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
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
    pub extern "C" fn jet_rt_handle_drop_DataWatchStatus(value: *mut crate::jet_std::DataWatchStatus) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DataWatchStatus(value: *mut crate::jet_std::DataWatchStatus) -> *mut crate::jet_std::DataWatchStatus {
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
    pub extern "C" fn jet_rt_handle_drop_DirEntry(value: *mut crate::jet_std::DirEntry) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DirEntry(value: *mut crate::jet_std::DirEntry) -> *mut crate::jet_std::DirEntry {
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
    pub extern "C" fn jet_rt_handle_drop_DurationUnit(value: *mut crate::jet_std::DurationUnit) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_DurationUnit(value: *mut crate::jet_std::DurationUnit) -> *mut crate::jet_std::DurationUnit {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_EncodingCause(value: *mut crate::jet_std::EncodingCause) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_EncodingCause(value: *mut crate::jet_std::EncodingCause) -> *mut crate::jet_std::EncodingCause {
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
    pub extern "C" fn jet_rt_handle_drop_EncodingErrorKind(value: *mut crate::jet_std::EncodingErrorKind) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_EncodingErrorKind(value: *mut crate::jet_std::EncodingErrorKind) -> *mut crate::jet_std::EncodingErrorKind {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_EncodingFormat(value: *mut crate::jet_std::EncodingFormat) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_EncodingFormat(value: *mut crate::jet_std::EncodingFormat) -> *mut crate::jet_std::EncodingFormat {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_EncodingLimits(value: *mut crate::jet_std::EncodingLimits) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_EncodingLimits(value: *mut crate::jet_std::EncodingLimits) -> *mut crate::jet_std::EncodingLimits {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_Envelope(value: *mut crate::jet_email::Envelope) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_Envelope(value: *mut crate::jet_email::Envelope) -> *mut crate::jet_email::Envelope {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_EnvError(value: *mut crate::jet_std::EnvError) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_EnvError(value: *mut crate::jet_std::EnvError) -> *mut crate::jet_std::EnvError {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_Error(value: *mut crate::jet_email::Error) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_Error(value: *mut crate::jet_email::Error) -> *mut crate::jet_email::Error {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_F64x4(value: *mut crate::jet_std::F64x4) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_F64x4(value: *mut crate::jet_std::F64x4) -> *mut crate::jet_std::F64x4 {
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
    pub extern "C" fn jet_rt_handle_drop_FieldError(value: *mut crate::jet_std::FieldError) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_FieldError(value: *mut crate::jet_std::FieldError) -> *mut crate::jet_std::FieldError {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_Handle(value: *mut crate::jet_layout::Handle) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_Handle(value: *mut crate::jet_layout::Handle) -> *mut crate::jet_layout::Handle {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_IOContext(value: *mut crate::jet_std::IOContext) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_IOContext(value: *mut crate::jet_std::IOContext) -> *mut crate::jet_std::IOContext {
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
    pub extern "C" fn jet_rt_handle_drop_IOOperation(value: *mut crate::jet_std::IOOperation) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_IOOperation(value: *mut crate::jet_std::IOOperation) -> *mut crate::jet_std::IOOperation {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetArena(value: *mut crate::jet_mem::JetArena) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetAsyncPolicy(value: *mut crate::jet_std::JetAsyncPolicy) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetAsyncPolicy(value: *mut crate::jet_std::JetAsyncPolicy) -> *mut crate::jet_std::JetAsyncPolicy {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetAuthApp(value: *mut crate::JetAuthApp) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetAuthApp(value: *mut crate::JetAuthApp) -> *mut crate::JetAuthApp {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetAuthority(value: *mut crate::JetAuthority) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetAuthority(value: *mut crate::JetAuthority) -> *mut crate::JetAuthority {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetAuthSession(value: *mut crate::JetAuthSession) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetAuthSession(value: *mut crate::JetAuthSession) -> *mut crate::JetAuthSession {
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
    pub extern "C" fn jet_rt_handle_clone_JetBitSet(value: *mut crate::JetBitSet) -> *mut crate::JetBitSet {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetBrowser(value: *mut crate::JetBrowser) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetBrowser(value: *mut crate::JetBrowser) -> *mut crate::JetBrowser {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetBrowserError(value: *mut crate::JetBrowserError) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetBrowserError(value: *mut crate::JetBrowserError) -> *mut crate::JetBrowserError {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetBrowserLocked(value: *mut crate::JetBrowserLocked) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetBrowserLocked(value: *mut crate::JetBrowserLocked) -> *mut crate::JetBrowserLocked {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetBrowserProfile(value: *mut crate::JetBrowserProfile) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetBrowserProfile(value: *mut crate::JetBrowserProfile) -> *mut crate::JetBrowserProfile {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetBrowserTimeout(value: *mut crate::JetBrowserTimeout) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetBrowserTimeout(value: *mut crate::JetBrowserTimeout) -> *mut crate::JetBrowserTimeout {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetBump(value: *mut crate::jet_mem::JetBump) {
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
    pub extern "C" fn jet_rt_handle_clone_JetChain(value: *mut crate::JetChain) -> *mut crate::JetChain {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetCoord2(value: *mut crate::JetCoord2) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetCoord2(value: *mut crate::JetCoord2) -> *mut crate::JetCoord2 {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetCounter(value: *mut crate::JetCounter) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetCounter(value: *mut crate::JetCounter) -> *mut crate::JetCounter {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetCountMinSketch(value: *mut crate::JetCountMinSketch) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetCountMinSketch(value: *mut crate::JetCountMinSketch) -> *mut crate::JetCountMinSketch {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
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
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotAccessibility(value: *mut crate::JetDataPlotAccessibility) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotAccessibility(value: *mut crate::JetDataPlotAccessibility) -> *mut crate::JetDataPlotAccessibility {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotAggregate(value: *mut crate::JetDataPlotAggregate) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotAggregate(value: *mut crate::JetDataPlotAggregate) -> *mut crate::JetDataPlotAggregate {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotAxis(value: *mut crate::JetDataPlotAxis) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotAxis(value: *mut crate::JetDataPlotAxis) -> *mut crate::JetDataPlotAxis {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotBackend(value: *mut crate::JetDataPlotBackend) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotBackend(value: *mut crate::JetDataPlotBackend) -> *mut crate::JetDataPlotBackend {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotCapability(value: *mut crate::JetDataPlotCapability) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotCapability(value: *mut crate::JetDataPlotCapability) -> *mut crate::JetDataPlotCapability {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotChannel(value: *mut crate::JetDataPlotChannel) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotChannel(value: *mut crate::JetDataPlotChannel) -> *mut crate::JetDataPlotChannel {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotDomain(value: *mut crate::JetDataPlotDomain) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotDomain(value: *mut crate::JetDataPlotDomain) -> *mut crate::JetDataPlotDomain {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotEncoding(value: *mut crate::JetDataPlotEncoding) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotEncoding(value: *mut crate::JetDataPlotEncoding) -> *mut crate::JetDataPlotEncoding {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotError(value: *mut crate::JetDataPlotError) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotError(value: *mut crate::JetDataPlotError) -> *mut crate::JetDataPlotError {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotErrorKind(value: *mut crate::JetDataPlotErrorKind) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotErrorKind(value: *mut crate::JetDataPlotErrorKind) -> *mut crate::JetDataPlotErrorKind {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotFacet(value: *mut crate::JetDataPlotFacet) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotFacet(value: *mut crate::JetDataPlotFacet) -> *mut crate::JetDataPlotFacet {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotFacetKind(value: *mut crate::JetDataPlotFacetKind) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotFacetKind(value: *mut crate::JetDataPlotFacetKind) -> *mut crate::JetDataPlotFacetKind {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotField(value: *mut crate::JetDataPlotField) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotField(value: *mut crate::JetDataPlotField) -> *mut crate::JetDataPlotField {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotFilterOp(value: *mut crate::JetDataPlotFilterOp) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotFilterOp(value: *mut crate::JetDataPlotFilterOp) -> *mut crate::JetDataPlotFilterOp {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotInspection(value: *mut crate::JetDataPlotInspection) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotInspection(value: *mut crate::JetDataPlotInspection) -> *mut crate::JetDataPlotInspection {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotInteraction(value: *mut crate::JetDataPlotInteraction) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotInteraction(value: *mut crate::JetDataPlotInteraction) -> *mut crate::JetDataPlotInteraction {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotLayer(value: *mut crate::JetDataPlotLayer) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotLayer(value: *mut crate::JetDataPlotLayer) -> *mut crate::JetDataPlotLayer {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotLayout(value: *mut crate::JetDataPlotLayout) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotLayout(value: *mut crate::JetDataPlotLayout) -> *mut crate::JetDataPlotLayout {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotLegend(value: *mut crate::JetDataPlotLegend) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotLegend(value: *mut crate::JetDataPlotLegend) -> *mut crate::JetDataPlotLegend {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotLegendPosition(value: *mut crate::JetDataPlotLegendPosition) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotLegendPosition(value: *mut crate::JetDataPlotLegendPosition) -> *mut crate::JetDataPlotLegendPosition {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotMark(value: *mut crate::JetDataPlotMark) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotMark(value: *mut crate::JetDataPlotMark) -> *mut crate::JetDataPlotMark {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotPlan(value: *mut crate::JetDataPlotPlan) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotPlan(value: *mut crate::JetDataPlotPlan) -> *mut crate::JetDataPlotPlan {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotProjection(value: *mut crate::JetDataPlotProjection) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotProjection(value: *mut crate::JetDataPlotProjection) -> *mut crate::JetDataPlotProjection {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotRender(value: *mut crate::JetDataPlotRender) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotRender(value: *mut crate::JetDataPlotRender) -> *mut crate::JetDataPlotRender {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotRenderFormat(value: *mut crate::JetDataPlotRenderFormat) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotRenderFormat(value: *mut crate::JetDataPlotRenderFormat) -> *mut crate::JetDataPlotRenderFormat {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotScale(value: *mut crate::JetDataPlotScale) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotScale(value: *mut crate::JetDataPlotScale) -> *mut crate::JetDataPlotScale {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotScaleKind(value: *mut crate::JetDataPlotScaleKind) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotScaleKind(value: *mut crate::JetDataPlotScaleKind) -> *mut crate::JetDataPlotScaleKind {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotSchema(value: *mut crate::JetDataPlotSchema) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotSchema(value: *mut crate::JetDataPlotSchema) -> *mut crate::JetDataPlotSchema {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotSelectedRow(value: *mut crate::JetDataPlotSelectedRow) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotSelectedRow(value: *mut crate::JetDataPlotSelectedRow) -> *mut crate::JetDataPlotSelectedRow {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotSourceFacts(value: *mut crate::JetDataPlotSourceFacts) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotSourceFacts(value: *mut crate::JetDataPlotSourceFacts) -> *mut crate::JetDataPlotSourceFacts {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotSupport(value: *mut crate::JetDataPlotSupport) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotSupport(value: *mut crate::JetDataPlotSupport) -> *mut crate::JetDataPlotSupport {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDataPlotValue(value: *mut crate::JetDataPlotValue) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDataPlotValue(value: *mut crate::JetDataPlotValue) -> *mut crate::JetDataPlotValue {
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
    pub extern "C" fn jet_rt_handle_clone_JetDate(value: *mut crate::JetDate) -> *mut crate::JetDate {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
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
    pub extern "C" fn jet_rt_handle_drop_JetDbConnection(value: *mut crate::JetDbConnection) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDbPoolLifecycle(value: *mut crate::JetDbPoolLifecycle) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDbPoolLifecycle(value: *mut crate::JetDbPoolLifecycle) -> *mut crate::JetDbPoolLifecycle {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDbPoolReceipt(value: *mut crate::JetDbPoolReceipt) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDbPoolReceipt(value: *mut crate::JetDbPoolReceipt) -> *mut crate::JetDbPoolReceipt {
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
    pub extern "C" fn jet_rt_handle_drop_JetDbScope(value: *mut crate::jet_sync::JetDbScope) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDbScope(value: *mut crate::jet_sync::JetDbScope) -> *mut crate::jet_sync::JetDbScope {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDecimal(value: *mut crate::jet_std::JetDecimal) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDelivery(value: *mut crate::JetDelivery) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDelivery(value: *mut crate::JetDelivery) -> *mut crate::JetDelivery {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDeliveryEvent(value: *mut crate::JetDeliveryEvent) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDeliveryEvent(value: *mut crate::JetDeliveryEvent) -> *mut crate::JetDeliveryEvent {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDeliveryReceipt(value: *mut crate::JetDeliveryReceipt) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDeliveryReceipt(value: *mut crate::JetDeliveryReceipt) -> *mut crate::JetDeliveryReceipt {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetDeliveryState(value: *mut crate::JetDeliveryState) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDeliveryState(value: *mut crate::JetDeliveryState) -> *mut crate::JetDeliveryState {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
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
    pub extern "C" fn jet_rt_handle_drop_JetDispatchState(value: *mut crate::jet_std::JetDispatchState) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetDispatchState(value: *mut crate::jet_std::JetDispatchState) -> *mut crate::jet_std::JetDispatchState {
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
    pub extern "C" fn jet_rt_handle_drop_JetEventConfigError(value: *mut crate::jet_std::JetEventConfigError) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetEventConfigError(value: *mut crate::jet_std::JetEventConfigError) -> *mut crate::jet_std::JetEventConfigError {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetEventOverflow(value: *mut crate::jet_std::JetEventOverflow) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetEventOverflow(value: *mut crate::jet_std::JetEventOverflow) -> *mut crate::jet_std::JetEventOverflow {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetEventPolicy(value: *mut crate::jet_std::JetEventPolicy) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetEventPolicy(value: *mut crate::jet_std::JetEventPolicy) -> *mut crate::jet_std::JetEventPolicy {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetEventResult(value: *mut crate::JetEventResult) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetEventResult(value: *mut crate::JetEventResult) -> *mut crate::JetEventResult {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetEventScope(value: *mut crate::jet_std::JetEventScope) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetEventScope(value: *mut crate::jet_std::JetEventScope) -> *mut crate::jet_std::JetEventScope {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetEventTrace(value: *mut crate::jet_std::JetEventTrace) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetEventTrace(value: *mut crate::jet_std::JetEventTrace) -> *mut crate::jet_std::JetEventTrace {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetFailurePolicy(value: *mut crate::jet_std::JetFailurePolicy) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetFailurePolicy(value: *mut crate::jet_std::JetFailurePolicy) -> *mut crate::jet_std::JetFailurePolicy {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetFfiCallbackRegistrationHandle(value: *mut crate::jet_std::JetFfiCallbackRegistrationHandle) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetFileLockOwner(value: *mut crate::JetFileLockOwner) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetFileLockOwner(value: *mut crate::JetFileLockOwner) -> *mut crate::JetFileLockOwner {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetFileReader(value: *mut crate::JetFileReader) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetFileScope(value: *mut crate::JetFileScope) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetFileScope(value: *mut crate::JetFileScope) -> *mut crate::JetFileScope {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetFileWriter(value: *mut crate::JetFileWriter) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetFixed(value: *mut crate::jet_mem::JetFixed) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetFontFace(value: *mut crate::JetFontFace) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetFontFace(value: *mut crate::JetFontFace) -> *mut crate::JetFontFace {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetFontStyle(value: *mut crate::JetFontStyle) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetFontStyle(value: *mut crate::JetFontStyle) -> *mut crate::JetFontStyle {
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
    pub extern "C" fn jet_rt_handle_drop_JetGlyph(value: *mut crate::JetGlyph) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetGlyph(value: *mut crate::JetGlyph) -> *mut crate::JetGlyph {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetGlyphRun(value: *mut crate::JetGlyphRun) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetGlyphRun(value: *mut crate::JetGlyphRun) -> *mut crate::JetGlyphRun {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetGlyphShaper(value: *mut crate::JetGlyphShaper) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetGlyphShaper(value: *mut crate::JetGlyphShaper) -> *mut crate::JetGlyphShaper {
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
    pub extern "C" fn jet_rt_handle_clone_JetHistoryRng(value: *mut crate::JetHistoryRng) -> *mut crate::JetHistoryRng {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHookPolicy(value: *mut crate::jet_std::JetHookPolicy) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHookPolicy(value: *mut crate::jet_std::JetHookPolicy) -> *mut crate::jet_std::JetHookPolicy {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPBody(value: *mut crate::JetHTTPBody) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPBody(value: *mut crate::JetHTTPBody) -> *mut crate::JetHTTPBody {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPBodyChunks(value: *mut crate::JetHTTPBodyChunks) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPClient(value: *mut crate::JetHTTPClient) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPClient(value: *mut crate::JetHTTPClient) -> *mut crate::JetHTTPClient {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPCompressEncoding(value: *mut crate::JetHTTPCompressEncoding) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPCompressEncoding(value: *mut crate::JetHTTPCompressEncoding) -> *mut crate::JetHTTPCompressEncoding {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPCookieJar(value: *mut crate::JetHTTPCookieJar) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPCookieJar(value: *mut crate::JetHTTPCookieJar) -> *mut crate::JetHTTPCookieJar {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPCorsOrigins(value: *mut crate::JetHTTPCorsOrigins) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPCorsOrigins(value: *mut crate::JetHTTPCorsOrigins) -> *mut crate::JetHTTPCorsOrigins {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPCorsPolicy(value: *mut crate::JetHTTPCorsPolicy) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPCorsPolicy(value: *mut crate::JetHTTPCorsPolicy) -> *mut crate::JetHTTPCorsPolicy {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPError(value: *mut crate::JetHTTPError) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPError(value: *mut crate::JetHTTPError) -> *mut crate::JetHTTPError {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPHeaderName(value: *mut crate::JetHTTPHeaderName) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPHeaderName(value: *mut crate::JetHTTPHeaderName) -> *mut crate::JetHTTPHeaderName {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPHeaders(value: *mut crate::JetHTTPHeaders) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPHeaders(value: *mut crate::JetHTTPHeaders) -> *mut crate::JetHTTPHeaders {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPHeaderValue(value: *mut crate::JetHTTPHeaderValue) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPHeaderValue(value: *mut crate::JetHTTPHeaderValue) -> *mut crate::JetHTTPHeaderValue {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPMethod(value: *mut crate::JetHTTPMethod) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPMethod(value: *mut crate::JetHTTPMethod) -> *mut crate::JetHTTPMethod {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPMux(value: *mut crate::JetHTTPMux) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPMux(value: *mut crate::JetHTTPMux) -> *mut crate::JetHTTPMux {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPOperation(value: *mut crate::JetHTTPOperation) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPOperation(value: *mut crate::JetHTTPOperation) -> *mut crate::JetHTTPOperation {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPProxy(value: *mut crate::JetHTTPProxy) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPProxy(value: *mut crate::JetHTTPProxy) -> *mut crate::JetHTTPProxy {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPRedirectPolicy(value: *mut crate::JetHTTPRedirectPolicy) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPRedirectPolicy(value: *mut crate::JetHTTPRedirectPolicy) -> *mut crate::JetHTTPRedirectPolicy {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPRequest(value: *mut crate::JetHTTPRequest) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPRequest(value: *mut crate::JetHTTPRequest) -> *mut crate::JetHTTPRequest {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPResponse(value: *mut crate::JetHTTPResponse) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPResponse(value: *mut crate::JetHTTPResponse) -> *mut crate::JetHTTPResponse {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPRetryPolicy(value: *mut crate::JetHTTPRetryPolicy) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPRetryPolicy(value: *mut crate::JetHTTPRetryPolicy) -> *mut crate::JetHTTPRetryPolicy {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPRouter(value: *mut crate::JetHTTPRouter) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPRouter(value: *mut crate::JetHTTPRouter) -> *mut crate::JetHTTPRouter {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPServer(value: *mut crate::JetHTTPServer) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPServer(value: *mut crate::JetHTTPServer) -> *mut crate::JetHTTPServer {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPServerTls(value: *mut crate::JetHTTPServerTls) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPServerTls(value: *mut crate::JetHTTPServerTls) -> *mut crate::JetHTTPServerTls {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPShutdownReport(value: *mut crate::JetHTTPShutdownReport) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPShutdownReport(value: *mut crate::JetHTTPShutdownReport) -> *mut crate::JetHTTPShutdownReport {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPStatus(value: *mut crate::JetHTTPStatus) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPStatus(value: *mut crate::JetHTTPStatus) -> *mut crate::JetHTTPStatus {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHTTPVersion(value: *mut crate::JetHTTPVersion) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHTTPVersion(value: *mut crate::JetHTTPVersion) -> *mut crate::JetHTTPVersion {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetHyperLogLog(value: *mut crate::JetHyperLogLog) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetHyperLogLog(value: *mut crate::JetHyperLogLog) -> *mut crate::JetHyperLogLog {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetInputEvent(value: *mut crate::JetInputEvent) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetInputEvent(value: *mut crate::JetInputEvent) -> *mut crate::JetInputEvent {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
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
    pub extern "C" fn jet_rt_handle_drop_JetJobError(value: *mut crate::JetJobError) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetJobError(value: *mut crate::JetJobError) -> *mut crate::JetJobError {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetJobPayload(value: *mut crate::JetJobPayload) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetJobPayload(value: *mut crate::JetJobPayload) -> *mut crate::JetJobPayload {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetJobQueueClaim(value: *mut crate::JetJobQueueClaim) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetJobQueueClaim(value: *mut crate::JetJobQueueClaim) -> *mut crate::JetJobQueueClaim {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetJobQueueDeliveryPolicy(value: *mut crate::JetJobQueueDeliveryPolicy) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetJobQueueDeliveryPolicy(value: *mut crate::JetJobQueueDeliveryPolicy) -> *mut crate::JetJobQueueDeliveryPolicy {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetJobQueueEvent(value: *mut crate::JetJobQueueEvent) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetJobQueueEvent(value: *mut crate::JetJobQueueEvent) -> *mut crate::JetJobQueueEvent {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetJobQueueReceipt(value: *mut crate::JetJobQueueReceipt) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetJobQueueReceipt(value: *mut crate::JetJobQueueReceipt) -> *mut crate::JetJobQueueReceipt {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetJobQueueRecord(value: *mut crate::JetJobQueueRecord) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetJobQueueRecord(value: *mut crate::JetJobQueueRecord) -> *mut crate::JetJobQueueRecord {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetJobQueueState(value: *mut crate::JetJobQueueState) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetJobQueueState(value: *mut crate::JetJobQueueState) -> *mut crate::JetJobQueueState {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetJobQueueStatus(value: *mut crate::JetJobQueueStatus) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetJobQueueStatus(value: *mut crate::JetJobQueueStatus) -> *mut crate::JetJobQueueStatus {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetJobResult(value: *mut crate::JetJobResult) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetJobResult(value: *mut crate::JetJobResult) -> *mut crate::JetJobResult {
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
    pub extern "C" fn jet_rt_handle_drop_JetLateEventDisposition(value: *mut crate::JetLateEventDisposition) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetLateEventDisposition(value: *mut crate::JetLateEventDisposition) -> *mut crate::JetLateEventDisposition {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetLayer(value: *mut crate::JetLayer) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetLayer(value: *mut crate::JetLayer) -> *mut crate::JetLayer {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetLiveQuery(value: *mut crate::JetLiveQuery) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetLiveQuery(value: *mut crate::JetLiveQuery) -> *mut crate::JetLiveQuery {
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
    pub extern "C" fn jet_rt_handle_clone_JetLocalTime(value: *mut crate::JetLocalTime) -> *mut crate::JetLocalTime {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetMappedFile(value: *mut crate::jet_std::JetMappedFile) {
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
    pub extern "C" fn jet_rt_handle_drop_JetMod(value: *mut crate::JetMod) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetModGrant(value: *mut crate::JetModGrant) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetModGrant(value: *mut crate::JetModGrant) -> *mut crate::JetModGrant {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetNetDnsError(value: *mut crate::JetNetDnsError) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetNetDnsError(value: *mut crate::JetNetDnsError) -> *mut crate::JetNetDnsError {
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
    pub extern "C" fn jet_rt_handle_drop_JetNetErrorDetail(value: *mut crate::JetNetErrorDetail) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetNetErrorDetail(value: *mut crate::JetNetErrorDetail) -> *mut crate::JetNetErrorDetail {
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
    pub extern "C" fn jet_rt_handle_drop_JetNetReadyInterest(value: *mut crate::JetNetReadyInterest) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetNetReadyInterest(value: *mut crate::JetNetReadyInterest) -> *mut crate::JetNetReadyInterest {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetNetShutdown(value: *mut crate::JetNetShutdown) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetNetShutdown(value: *mut crate::JetNetShutdown) -> *mut crate::JetNetShutdown {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetNullBackend(value: *mut crate::JetNullBackend) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetNullBackend(value: *mut crate::JetNullBackend) -> *mut crate::JetNullBackend {
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
    pub extern "C" fn jet_rt_handle_clone_JetOrderedMap(value: *mut crate::JetOrderedMap) -> *mut crate::JetOrderedMap {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetPath(value: *mut crate::JetPath) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetPath(value: *mut crate::JetPath) -> *mut crate::JetPath {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetPeriod(value: *mut crate::JetPeriod) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetPeriod(value: *mut crate::JetPeriod) -> *mut crate::JetPeriod {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetPlugin(value: *mut crate::JetPlugin) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetPlugin(value: *mut crate::JetPlugin) -> *mut crate::JetPlugin {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetPoint(value: *mut crate::JetPoint) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetPoint(value: *mut crate::JetPoint) -> *mut crate::JetPoint {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetPool(value: *mut crate::jet_mem::JetPool) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetRange(value: *mut crate::JetRange) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetRange(value: *mut crate::JetRange) -> *mut crate::JetRange {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetRay2(value: *mut crate::JetRay2) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetRay2(value: *mut crate::JetRay2) -> *mut crate::JetRay2 {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
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
    pub extern "C" fn jet_rt_handle_drop_JetRealtimeReceipt(value: *mut crate::JetRealtimeReceipt) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetRealtimeReceipt(value: *mut crate::JetRealtimeReceipt) -> *mut crate::JetRealtimeReceipt {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetRealtimeStream(value: *mut crate::JetRealtimeStream) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetRealtimeStream(value: *mut crate::JetRealtimeStream) -> *mut crate::JetRealtimeStream {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetRect(value: *mut crate::JetRect) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetRect(value: *mut crate::JetRect) -> *mut crate::JetRect {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetReflectField(value: *mut crate::JetReflectField) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetReflectField(value: *mut crate::JetReflectField) -> *mut crate::JetReflectField {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetReflectValue(value: *mut crate::JetReflectValue) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetReflectValue(value: *mut crate::JetReflectValue) -> *mut crate::JetReflectValue {
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
    pub extern "C" fn jet_rt_handle_clone_JetRegexMatch(value: *mut crate::jet_std::JetRegexMatch) -> *mut crate::jet_std::JetRegexMatch {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetReservoirSampler(value: *mut crate::JetReservoirSampler) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetReservoirSampler(value: *mut crate::JetReservoirSampler) -> *mut crate::JetReservoirSampler {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetRowPolicy(value: *mut crate::jet_sync::JetRowPolicy) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetRowPolicy(value: *mut crate::jet_sync::JetRowPolicy) -> *mut crate::jet_sync::JetRowPolicy {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetServiceDelivery(value: *mut crate::JetServiceDelivery) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetServiceDelivery(value: *mut crate::JetServiceDelivery) -> *mut crate::JetServiceDelivery {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetServiceEndpoint(value: *mut crate::JetServiceEndpoint) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetServiceEndpoint(value: *mut crate::JetServiceEndpoint) -> *mut crate::JetServiceEndpoint {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetServiceError(value: *mut crate::JetServiceError) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetServiceError(value: *mut crate::JetServiceError) -> *mut crate::JetServiceError {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetServiceRestart(value: *mut crate::JetServiceRestart) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetServiceRestart(value: *mut crate::JetServiceRestart) -> *mut crate::JetServiceRestart {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetServiceRuntime(value: *mut crate::JetServiceRuntime) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetServiceRuntime(value: *mut crate::JetServiceRuntime) -> *mut crate::JetServiceRuntime {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetServiceStateStore(value: *mut crate::JetServiceStateStore) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetServiceStateStore(value: *mut crate::JetServiceStateStore) -> *mut crate::JetServiceStateStore {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetServiceTree(value: *mut crate::JetServiceTree) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetServiceTree(value: *mut crate::JetServiceTree) -> *mut crate::JetServiceTree {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetServiceUpgradeReceipt(value: *mut crate::JetServiceUpgradeReceipt) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetServiceUpgradeReceipt(value: *mut crate::JetServiceUpgradeReceipt) -> *mut crate::JetServiceUpgradeReceipt {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetSharedTransaction(value: *mut crate::JetSharedTransaction) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetSize(value: *mut crate::JetSize) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetSize(value: *mut crate::JetSize) -> *mut crate::JetSize {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetSizeConstraint(value: *mut crate::JetSizeConstraint) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetSizeConstraint(value: *mut crate::JetSizeConstraint) -> *mut crate::JetSizeConstraint {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
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
    pub extern "C" fn jet_rt_handle_drop_JetStderr(value: *mut crate::JetStderr) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetStdinReader(value: *mut crate::JetStdinReader) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetStdout(value: *mut crate::JetStdout) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetStringSet(value: *mut crate::JetStringSet) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetStringSet(value: *mut crate::JetStringSet) -> *mut crate::JetStringSet {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetSubscription(value: *mut crate::jet_std::JetSubscription) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetSubscription(value: *mut crate::jet_std::JetSubscription) -> *mut crate::jet_std::JetSubscription {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTaskFailure(value: *mut crate::JetTaskFailure) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTaskFailure(value: *mut crate::JetTaskFailure) -> *mut crate::JetTaskFailure {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTaskGroup(value: *mut crate::jet_std::JetTaskGroup) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTaskGroup(value: *mut crate::jet_std::JetTaskGroup) -> *mut crate::jet_std::JetTaskGroup {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTaskOutcome(value: *mut crate::JetTaskOutcome) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTaskOutcome(value: *mut crate::JetTaskOutcome) -> *mut crate::JetTaskOutcome {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTaskStatus(value: *mut crate::JetTaskStatus) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTaskStatus(value: *mut crate::JetTaskStatus) -> *mut crate::JetTaskStatus {
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
    pub extern "C" fn jet_rt_handle_clone_JetTCPListener(value: *mut crate::JetTCPListener) -> *mut crate::JetTCPListener {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTCPStream(value: *mut crate::JetTCPStream) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTDigest(value: *mut crate::JetTDigest) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTDigest(value: *mut crate::JetTDigest) -> *mut crate::JetTDigest {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTempDirOwner(value: *mut crate::JetTempDirOwner) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTempDirOwner(value: *mut crate::JetTempDirOwner) -> *mut crate::JetTempDirOwner {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTempFileOwner(value: *mut crate::JetTempFileOwner) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTempFileOwner(value: *mut crate::JetTempFileOwner) -> *mut crate::JetTempFileOwner {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
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
    pub extern "C" fn jet_rt_handle_drop_JetTLSCertificate(value: *mut crate::JetTLSCertificate) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTLSCertificate(value: *mut crate::JetTLSCertificate) -> *mut crate::JetTLSCertificate {
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
    pub extern "C" fn jet_rt_handle_drop_JetTLSClientIdentity(value: *mut crate::JetTLSClientIdentity) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTLSClientIdentity(value: *mut crate::JetTLSClientIdentity) -> *mut crate::JetTLSClientIdentity {
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
    pub extern "C" fn jet_rt_handle_drop_JetTLSRootCertificates(value: *mut crate::JetTLSRootCertificates) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTLSRootCertificates(value: *mut crate::JetTLSRootCertificates) -> *mut crate::JetTLSRootCertificates {
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
    pub extern "C" fn jet_rt_handle_drop_JetTLSTrust(value: *mut crate::JetTLSTrust) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTLSTrust(value: *mut crate::JetTLSTrust) -> *mut crate::JetTLSTrust {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTLSVersion(value: *mut crate::JetTLSVersion) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTLSVersion(value: *mut crate::JetTLSVersion) -> *mut crate::JetTLSVersion {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTransaction(value: *mut crate::JetTransaction) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTransform2(value: *mut crate::JetTransform2) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTransform2(value: *mut crate::JetTransform2) -> *mut crate::JetTransform2 {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTuiBackend(value: *mut crate::JetTuiBackend) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTuiBackend(value: *mut crate::JetTuiBackend) -> *mut crate::JetTuiBackend {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTuiCapabilities(value: *mut crate::JetTuiCapabilities) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTuiCapabilities(value: *mut crate::JetTuiCapabilities) -> *mut crate::JetTuiCapabilities {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTuiColor(value: *mut crate::JetTuiColor) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTuiColor(value: *mut crate::JetTuiColor) -> *mut crate::JetTuiColor {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTuiConstraint(value: *mut crate::JetTuiConstraint) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTuiConstraint(value: *mut crate::JetTuiConstraint) -> *mut crate::JetTuiConstraint {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTuiDirection(value: *mut crate::JetTuiDirection) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTuiDirection(value: *mut crate::JetTuiDirection) -> *mut crate::JetTuiDirection {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTuiEvent(value: *mut crate::JetTuiEvent) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTuiEvent(value: *mut crate::JetTuiEvent) -> *mut crate::JetTuiEvent {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTuiListState(value: *mut crate::JetTuiListState) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTuiListState(value: *mut crate::JetTuiListState) -> *mut crate::JetTuiListState {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetTuiStyle(value: *mut crate::JetTuiStyle) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetTuiStyle(value: *mut crate::JetTuiStyle) -> *mut crate::JetTuiStyle {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
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
    pub extern "C" fn jet_rt_handle_drop_JetUiAccessibility(value: *mut crate::JetUiAccessibility) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetUiAccessibility(value: *mut crate::JetUiAccessibility) -> *mut crate::JetUiAccessibility {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetUiAriaRole(value: *mut crate::JetUiAriaRole) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetUiAriaRole(value: *mut crate::JetUiAriaRole) -> *mut crate::JetUiAriaRole {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetUiCapabilityFacts(value: *mut crate::JetUiCapabilityFacts) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetUiCapabilityFacts(value: *mut crate::JetUiCapabilityFacts) -> *mut crate::JetUiCapabilityFacts {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetUiFileDialogRequest(value: *mut crate::JetUiFileDialogRequest) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetUiFileDialogRequest(value: *mut crate::JetUiFileDialogRequest) -> *mut crate::JetUiFileDialogRequest {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetUiFileFilter(value: *mut crate::JetUiFileFilter) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetUiFileFilter(value: *mut crate::JetUiFileFilter) -> *mut crate::JetUiFileFilter {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetUiFsGrant(value: *mut crate::JetUiFsGrant) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetUiFsGrant(value: *mut crate::JetUiFsGrant) -> *mut crate::JetUiFsGrant {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetUiFsRights(value: *mut crate::JetUiFsRights) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetUiFsRights(value: *mut crate::JetUiFsRights) -> *mut crate::JetUiFsRights {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetUiNode(value: *mut crate::JetUiNode) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetUiNode(value: *mut crate::JetUiNode) -> *mut crate::JetUiNode {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetUiPreviewViewport(value: *mut crate::JetUiPreviewViewport) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetUiPreviewViewport(value: *mut crate::JetUiPreviewViewport) -> *mut crate::JetUiPreviewViewport {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetUiShortcut(value: *mut crate::JetUiShortcut) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetUiShortcut(value: *mut crate::JetUiShortcut) -> *mut crate::JetUiShortcut {
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
    pub extern "C" fn jet_rt_handle_drop_JetWebForm(value: *mut crate::JetWebForm) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebForm(value: *mut crate::JetWebForm) -> *mut crate::JetWebForm {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebFormActionError(value: *mut crate::JetWebFormActionError) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebFormActionError(value: *mut crate::JetWebFormActionError) -> *mut crate::JetWebFormActionError {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebFormControl(value: *mut crate::JetWebFormControl) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebFormControl(value: *mut crate::JetWebFormControl) -> *mut crate::JetWebFormControl {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebFormDecodedInput(value: *mut crate::JetWebFormDecodedInput) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebFormDecodedInput(value: *mut crate::JetWebFormDecodedInput) -> *mut crate::JetWebFormDecodedInput {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebFormErrorState(value: *mut crate::JetWebFormErrorState) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebFormErrorState(value: *mut crate::JetWebFormErrorState) -> *mut crate::JetWebFormErrorState {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebFormFieldSpec(value: *mut crate::JetWebFormFieldSpec) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebFormFieldSpec(value: *mut crate::JetWebFormFieldSpec) -> *mut crate::JetWebFormFieldSpec {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebFormFieldState(value: *mut crate::JetWebFormFieldState) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebFormFieldState(value: *mut crate::JetWebFormFieldState) -> *mut crate::JetWebFormFieldState {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebFormInput(value: *mut crate::JetWebFormInput) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebFormInput(value: *mut crate::JetWebFormInput) -> *mut crate::JetWebFormInput {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebFormLifecycle(value: *mut crate::JetWebFormLifecycle) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebFormLifecycle(value: *mut crate::JetWebFormLifecycle) -> *mut crate::JetWebFormLifecycle {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebFormLifecycleStatus(value: *mut crate::JetWebFormLifecycleStatus) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebFormLifecycleStatus(value: *mut crate::JetWebFormLifecycleStatus) -> *mut crate::JetWebFormLifecycleStatus {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebFormState(value: *mut crate::JetWebFormState) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebFormState(value: *mut crate::JetWebFormState) -> *mut crate::JetWebFormState {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebFormStatus(value: *mut crate::JetWebFormStatus) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebFormStatus(value: *mut crate::JetWebFormStatus) -> *mut crate::JetWebFormStatus {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebFormTyped(value: *mut crate::JetWebFormTyped) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebFormTyped(value: *mut crate::JetWebFormTyped) -> *mut crate::JetWebFormTyped {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebFormTypedSubmission(value: *mut crate::JetWebFormTypedSubmission) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebFormTypedValidation(value: *mut crate::JetWebFormTypedValidation) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebFormValidation(value: *mut crate::JetWebFormValidation) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebFormValidationTiming(value: *mut crate::JetWebFormValidationTiming) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebFormValidationTiming(value: *mut crate::JetWebFormValidationTiming) -> *mut crate::JetWebFormValidationTiming {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebFormValueType(value: *mut crate::JetWebFormValueType) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebFormValueType(value: *mut crate::JetWebFormValueType) -> *mut crate::JetWebFormValueType {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebMutationState(value: *mut crate::JetWebMutationState) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebMutationState(value: *mut crate::JetWebMutationState) -> *mut crate::JetWebMutationState {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebMutationStatus(value: *mut crate::JetWebMutationStatus) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebMutationStatus(value: *mut crate::JetWebMutationStatus) -> *mut crate::JetWebMutationStatus {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebQuery(value: *mut crate::JetWebQuery) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebQuery(value: *mut crate::JetWebQuery) -> *mut crate::JetWebQuery {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebQueryNetworkMode(value: *mut crate::JetWebQueryNetworkMode) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebQueryNetworkMode(value: *mut crate::JetWebQueryNetworkMode) -> *mut crate::JetWebQueryNetworkMode {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebQueryState(value: *mut crate::JetWebQueryState) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebQueryState(value: *mut crate::JetWebQueryState) -> *mut crate::JetWebQueryState {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebQueryStatus(value: *mut crate::JetWebQueryStatus) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebQueryStatus(value: *mut crate::JetWebQueryStatus) -> *mut crate::JetWebQueryStatus {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebStoreEvent(value: *mut crate::JetWebStoreEvent) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebStoreEvent(value: *mut crate::JetWebStoreEvent) -> *mut crate::JetWebStoreEvent {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebStoreSubscription(value: *mut crate::JetWebStoreSubscription) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebStoreSubscription(value: *mut crate::JetWebStoreSubscription) -> *mut crate::JetWebStoreSubscription {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebTableFilter(value: *mut crate::JetWebTableFilter) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebTableFilter(value: *mut crate::JetWebTableFilter) -> *mut crate::JetWebTableFilter {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebTablePageMode(value: *mut crate::JetWebTablePageMode) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebTablePageMode(value: *mut crate::JetWebTablePageMode) -> *mut crate::JetWebTablePageMode {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebTableSort(value: *mut crate::JetWebTableSort) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebTableSort(value: *mut crate::JetWebTableSort) -> *mut crate::JetWebTableSort {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebTableSortDirection(value: *mut crate::JetWebTableSortDirection) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebTableSortDirection(value: *mut crate::JetWebTableSortDirection) -> *mut crate::JetWebTableSortDirection {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebTableState(value: *mut crate::JetWebTableState) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebTableState(value: *mut crate::JetWebTableState) -> *mut crate::JetWebTableState {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebTableStatus(value: *mut crate::JetWebTableStatus) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebTableStatus(value: *mut crate::JetWebTableStatus) -> *mut crate::JetWebTableStatus {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebVirtualPlan(value: *mut crate::JetWebVirtualPlan) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebVirtualPlan(value: *mut crate::JetWebVirtualPlan) -> *mut crate::JetWebVirtualPlan {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebVirtualPlanViewport(value: *mut crate::JetWebVirtualPlanViewport) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebVirtualPlanViewport(value: *mut crate::JetWebVirtualPlanViewport) -> *mut crate::JetWebVirtualPlanViewport {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebVirtualViewport(value: *mut crate::JetWebVirtualViewport) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebVirtualViewport(value: *mut crate::JetWebVirtualViewport) -> *mut crate::JetWebVirtualViewport {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWebVirtualWindow(value: *mut crate::JetWebVirtualWindow) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWebVirtualWindow(value: *mut crate::JetWebVirtualWindow) -> *mut crate::JetWebVirtualWindow {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWorkflowHandle(value: *mut crate::JetWorkflowHandle) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWorkflowHandle(value: *mut crate::JetWorkflowHandle) -> *mut crate::JetWorkflowHandle {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWsConn(value: *mut crate::JetWsConn) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWsError(value: *mut crate::JetWsError) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWsError(value: *mut crate::JetWsError) -> *mut crate::JetWsError {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetWsMessage(value: *mut crate::JetWsMessage) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetWsMessage(value: *mut crate::JetWsMessage) -> *mut crate::JetWsMessage {
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
    pub extern "C" fn jet_rt_handle_clone_JetZone(value: *mut crate::JetZone) -> *mut crate::JetZone {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JetZonedDateTime(value: *mut crate::JetZonedDateTime) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_JetZonedDateTime(value: *mut crate::JetZonedDateTime) -> *mut crate::JetZonedDateTime {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JSONLReader(value: *mut crate::jet_std::JSONLReader) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JSONLWriter(value: *mut crate::jet_std::JSONLWriter) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JSONReader(value: *mut crate::jet_std::JSONReader) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_JSONWriter(value: *mut crate::jet_std::JSONWriter) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_Limits(value: *mut crate::jet_email::Limits) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_Limits(value: *mut crate::jet_email::Limits) -> *mut crate::jet_email::Limits {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_LinExpr(value: *mut crate::jet_layout::LinExpr) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_LinExpr(value: *mut crate::jet_layout::LinExpr) -> *mut crate::jet_layout::LinExpr {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
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
    pub extern "C" fn jet_rt_handle_drop_Mailer(value: *mut crate::jet_email::Mailer) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_Mat3(value: *mut crate::jet_std::Mat3) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_Mat3(value: *mut crate::jet_std::Mat3) -> *mut crate::jet_std::Mat3 {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_Mat4(value: *mut crate::jet_std::Mat4) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_Mat4(value: *mut crate::jet_std::Mat4) -> *mut crate::jet_std::Mat4 {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_Message(value: *mut crate::jet_email::Message) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_Message(value: *mut crate::jet_email::Message) -> *mut crate::jet_email::Message {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_ProcessChild(value: *mut crate::jet_std::ProcessChild) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_ProcessChild(value: *mut crate::jet_std::ProcessChild) -> *mut crate::jet_std::ProcessChild {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_ProcessPlan(value: *mut crate::jet_std::ProcessPlan) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_ProcessPlan(value: *mut crate::jet_std::ProcessPlan) -> *mut crate::jet_std::ProcessPlan {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_ProcessReceipt(value: *mut crate::jet_std::ProcessReceipt) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_ProcessReceipt(value: *mut crate::jet_std::ProcessReceipt) -> *mut crate::jet_std::ProcessReceipt {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_ProcessResourceLimit(value: *mut crate::jet_std::ProcessResourceLimit) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_ProcessResourceLimit(value: *mut crate::jet_std::ProcessResourceLimit) -> *mut crate::jet_std::ProcessResourceLimit {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_ProcessSpec(value: *mut crate::jet_std::ProcessSpec) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_ProcessSpec(value: *mut crate::jet_std::ProcessSpec) -> *mut crate::jet_std::ProcessSpec {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_ProcessStreamMode(value: *mut crate::jet_std::ProcessStreamMode) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_ProcessStreamMode(value: *mut crate::jet_std::ProcessStreamMode) -> *mut crate::jet_std::ProcessStreamMode {
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
    pub extern "C" fn jet_rt_handle_clone_RaylibColor(value: *mut crate::RaylibColor) -> *mut crate::RaylibColor {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_RaylibSound(value: *mut crate::RaylibSound) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_RaylibSound(value: *mut crate::RaylibSound) -> *mut crate::RaylibSound {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_RaylibTextureAtlas(value: *mut crate::RaylibTextureAtlas) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_RaylibTextureAtlas(value: *mut crate::RaylibTextureAtlas) -> *mut crate::RaylibTextureAtlas {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_RaylibWindow(value: *mut crate::RaylibWindow) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_RaylibWindow(value: *mut crate::RaylibWindow) -> *mut crate::RaylibWindow {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_RecipientPolicy(value: *mut crate::jet_email::RecipientPolicy) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_RecipientPolicy(value: *mut crate::jet_email::RecipientPolicy) -> *mut crate::jet_email::RecipientPolicy {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_RecipientReport(value: *mut crate::jet_email::RecipientReport) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_RecipientReport(value: *mut crate::jet_email::RecipientReport) -> *mut crate::jet_email::RecipientReport {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
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
    pub extern "C" fn jet_rt_handle_drop_SendReport(value: *mut crate::jet_email::SendReport) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_SendReport(value: *mut crate::jet_email::SendReport) -> *mut crate::jet_email::SendReport {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_SMTPSecurity(value: *mut crate::jet_email::SMTPSecurity) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_SMTPSecurity(value: *mut crate::jet_email::SMTPSecurity) -> *mut crate::jet_email::SMTPSecurity {
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
    pub extern "C" fn jet_rt_handle_drop_Stat(value: *mut crate::jet_std::Stat) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_Stat(value: *mut crate::jet_std::Stat) -> *mut crate::jet_std::Stat {
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
    pub extern "C" fn jet_rt_handle_drop_TerminalMode(value: *mut crate::jet_std::TerminalMode) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_TerminalMode(value: *mut crate::jet_std::TerminalMode) -> *mut crate::jet_std::TerminalMode {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_TerminalPolicy(value: *mut crate::jet_std::TerminalPolicy) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_TerminalPolicy(value: *mut crate::jet_std::TerminalPolicy) -> *mut crate::jet_std::TerminalPolicy {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_TerminalSession(value: *mut crate::jet_std::TerminalSession) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_TerminalSession(value: *mut crate::jet_std::TerminalSession) -> *mut crate::jet_std::TerminalSession {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_TerminalSize(value: *mut crate::jet_std::TerminalSize) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_TerminalSize(value: *mut crate::jet_std::TerminalSize) -> *mut crate::jet_std::TerminalSize {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_TextError(value: *mut crate::jet_std::TextError) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_TextError(value: *mut crate::jet_std::TextError) -> *mut crate::jet_std::TextError {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_TextWidth(value: *mut crate::jet_std::TextWidth) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_TextWidth(value: *mut crate::jet_std::TextWidth) -> *mut crate::jet_std::TextWidth {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_TextWidthAmbiguous(value: *mut crate::jet_std::TextWidthAmbiguous) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_TextWidthAmbiguous(value: *mut crate::jet_std::TextWidthAmbiguous) -> *mut crate::jet_std::TextWidthAmbiguous {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_TextWidthControls(value: *mut crate::jet_std::TextWidthControls) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_TextWidthControls(value: *mut crate::jet_std::TextWidthControls) -> *mut crate::jet_std::TextWidthControls {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_TLSTrust(value: *mut crate::jet_email::TLSTrust) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_TLSTrust(value: *mut crate::jet_email::TLSTrust) -> *mut crate::jet_email::TLSTrust {
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
    pub extern "C" fn jet_rt_handle_drop_Vec2(value: *mut crate::jet_std::Vec2) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_Vec2(value: *mut crate::jet_std::Vec2) -> *mut crate::jet_std::Vec2 {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_Vec3(value: *mut crate::jet_std::Vec3) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_Vec3(value: *mut crate::jet_std::Vec3) -> *mut crate::jet_std::Vec3 {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_Vec4(value: *mut crate::jet_std::Vec4) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_Vec4(value: *mut crate::jet_std::Vec4) -> *mut crate::jet_std::Vec4 {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_WalkEntry(value: *mut crate::jet_std::WalkEntry) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_WalkEntry(value: *mut crate::jet_std::WalkEntry) -> *mut crate::jet_std::WalkEntry {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_WatchDomain(value: *mut crate::jet_std::WatchDomain) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_WatchDomain(value: *mut crate::jet_std::WatchDomain) -> *mut crate::jet_std::WatchDomain {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_WatchEvent(value: *mut crate::jet_std::WatchEvent) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_WatchEvent(value: *mut crate::jet_std::WatchEvent) -> *mut crate::jet_std::WatchEvent {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_WatchHandle(value: *mut crate::jet_std::WatchHandle) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_WatchHandle(value: *mut crate::jet_std::WatchHandle) -> *mut crate::jet_std::WatchHandle {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_WatchKind(value: *mut crate::jet_std::WatchKind) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_WatchKind(value: *mut crate::jet_std::WatchKind) -> *mut crate::jet_std::WatchKind {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_WatchSet(value: *mut crate::jet_std::WatchSet) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_WatchSet(value: *mut crate::jet_std::WatchSet) -> *mut crate::jet_std::WatchSet {
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
    pub extern "C" fn jet_rt_handle_drop_XMLCanonicalMode(value: *mut crate::jet_std::XMLCanonicalMode) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_XMLCanonicalMode(value: *mut crate::jet_std::XMLCanonicalMode) -> *mut crate::jet_std::XMLCanonicalMode {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_XMLEncoding(value: *mut crate::jet_std::XMLEncoding) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_XMLEncoding(value: *mut crate::jet_std::XMLEncoding) -> *mut crate::jet_std::XMLEncoding {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_XMLEntityPolicy(value: *mut crate::jet_std::XMLEntityPolicy) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_XMLEntityPolicy(value: *mut crate::jet_std::XMLEntityPolicy) -> *mut crate::jet_std::XMLEntityPolicy {
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
    pub extern "C" fn jet_rt_handle_drop_XMLLexicalPolicy(value: *mut crate::jet_std::XMLLexicalPolicy) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_XMLLexicalPolicy(value: *mut crate::jet_std::XMLLexicalPolicy) -> *mut crate::jet_std::XMLLexicalPolicy {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_XMLLimits(value: *mut crate::jet_std::XMLLimits) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_XMLLimits(value: *mut crate::jet_std::XMLLimits) -> *mut crate::jet_std::XMLLimits {
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

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_XMLReader(value: *mut crate::jet_std::XMLReader) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_XMLReason(value: *mut crate::jet_std::XMLReason) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_XMLReason(value: *mut crate::jet_std::XMLReason) -> *mut crate::jet_std::XMLReason {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_XMLRenderOptions(value: *mut crate::jet_std::XMLRenderOptions) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_clone_XMLRenderOptions(value: *mut crate::jet_std::XMLRenderOptions) -> *mut crate::jet_std::XMLRenderOptions {
        // SAFETY: `value` is a live handle the caller owns for this call.
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_drop_XMLWriter(value: *mut crate::jet_std::XMLWriter) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_CBORError(value: *mut crate::jet_std::CBORError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::CBORError as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_CBORError(value: *mut crate::jet_std::CBORError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::CBORError as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_CBORError(value: *mut crate::jet_std::CBORError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::CBORError as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_Clock(value: *mut crate::jet_std::Clock) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::Clock as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_Clock(value: *mut crate::jet_std::Clock) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::Clock as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_Closed(value: *mut crate::jet_std::Closed) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::Closed as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_DataAuthority(value: *mut crate::jet_std::DataAuthority) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataAuthority as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_DataError(value: *mut crate::jet_std::DataError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataError as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_DataError(value: *mut crate::jet_std::DataError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataError as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_DataError(value: *mut crate::jet_std::DataError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataError as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_DataErrorKind(value: *mut crate::jet_std::DataErrorKind) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataErrorKind as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_DataErrorKind(value: *mut crate::jet_std::DataErrorKind) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataErrorKind as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_DataErrorKind(value: *mut crate::jet_std::DataErrorKind) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataErrorKind as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_DataFormat(value: *mut crate::jet_std::DataFormat) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataFormat as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_DataFreshness(value: *mut crate::jet_std::DataFreshness) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataFreshness as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_DataInvalidationCause(value: *mut crate::jet_std::DataInvalidationCause) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataInvalidationCause as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_DataLoaderKind(value: *mut crate::jet_std::DataLoaderKind) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataLoaderKind as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_DataLoaderStatus(value: *mut crate::jet_std::DataLoaderStatus) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataLoaderStatus as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_DataLoaderStatus(value: *mut crate::jet_std::DataLoaderStatus) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataLoaderStatus as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_DataLoaderStatus(value: *mut crate::jet_std::DataLoaderStatus) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataLoaderStatus as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_DataSnapshotIdentity(value: *mut crate::jet_std::DataSnapshotIdentity) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataSnapshotIdentity as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_DataSourceIdentity(value: *mut crate::jet_std::DataSourceIdentity) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataSourceIdentity as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_DataStatus(value: *mut crate::jet_std::DataStatus) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataStatus as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_DataStatus(value: *mut crate::jet_std::DataStatus) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataStatus as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_DataStatus(value: *mut crate::jet_std::DataStatus) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataStatus as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_DataTree(value: *mut crate::jet_std::DataTree) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataTree as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_DataTree(value: *mut crate::jet_std::DataTree) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataTree as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_DataTree(value: *mut crate::jet_std::DataTree) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataTree as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_DataWatchStatus(value: *mut crate::jet_std::DataWatchStatus) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataWatchStatus as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_DataWatchStatus(value: *mut crate::jet_std::DataWatchStatus) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataWatchStatus as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_DataWatchStatus(value: *mut crate::jet_std::DataWatchStatus) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DataWatchStatus as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_DBError(value: *mut crate::jet_std::DBError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DBError as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_DBError(value: *mut crate::jet_std::DBError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DBError as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_DBError(value: *mut crate::jet_std::DBError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DBError as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_DBValue(value: *mut crate::jet_std::DBValue) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DBValue as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_DBValue(value: *mut crate::jet_std::DBValue) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DBValue as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_DirEntry(value: *mut crate::jet_std::DirEntry) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::DirEntry as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_Duration(value: *mut crate::jet_std::Duration) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::Duration as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_Duration(value: *mut crate::jet_std::Duration) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::Duration as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_Duration(value: *mut crate::jet_std::Duration) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::Duration as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_EncodingError(value: *mut crate::jet_std::EncodingError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::EncodingError as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_EncodingError(value: *mut crate::jet_std::EncodingError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::EncodingError as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_EncodingError(value: *mut crate::jet_std::EncodingError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::EncodingError as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_EnvError(value: *mut crate::jet_std::EnvError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::EnvError as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_EnvError(value: *mut crate::jet_std::EnvError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::EnvError as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_EnvError(value: *mut crate::jet_std::EnvError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::EnvError as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_F64x4(value: *mut crate::jet_std::F64x4) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::F64x4 as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_F64x4(value: *mut crate::jet_std::F64x4) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::F64x4 as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_F64x4(value: *mut crate::jet_std::F64x4) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::F64x4 as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_Fake(value: *mut crate::jet_std::Fake) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::Fake as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_FieldError(value: *mut crate::jet_std::FieldError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::FieldError as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_FieldError(value: *mut crate::jet_std::FieldError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::FieldError as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_FieldError(value: *mut crate::jet_std::FieldError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::FieldError as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_IOContext(value: *mut crate::jet_std::IOContext) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::IOContext as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_IOError(value: *mut crate::jet_std::IOError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::IOError as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_IOError(value: *mut crate::jet_std::IOError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::IOError as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_IOError(value: *mut crate::jet_std::IOError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::IOError as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetBitSet(value: *mut crate::JetBitSet) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetBitSet as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetBitSet(value: *mut crate::JetBitSet) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetBitSet as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetBitSet(value: *mut crate::JetBitSet) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetBitSet as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetBrowserError(value: *mut crate::JetBrowserError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetBrowserError as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetBrowserError(value: *mut crate::JetBrowserError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetBrowserError as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetBrowserError(value: *mut crate::JetBrowserError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetBrowserError as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetChain(value: *mut crate::JetChain) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetChain as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetChain(value: *mut crate::JetChain) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetChain as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetChain(value: *mut crate::JetChain) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetChain as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetCounter(value: *mut crate::JetCounter) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetCounter as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetCounter(value: *mut crate::JetCounter) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetCounter as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetCounter(value: *mut crate::JetCounter) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetCounter as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetCountMinSketch(value: *mut crate::JetCountMinSketch) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetCountMinSketch as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetDate(value: *mut crate::JetDate) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetDate as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetDate(value: *mut crate::JetDate) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetDate as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetDate(value: *mut crate::JetDate) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetDate as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetDateTime(value: *mut crate::JetDateTime) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetDateTime as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetDateTime(value: *mut crate::JetDateTime) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetDateTime as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetDateTime(value: *mut crate::JetDateTime) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetDateTime as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetDbPoolReceipt(value: *mut crate::JetDbPoolReceipt) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetDbPoolReceipt as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetDbPoolReceipt(value: *mut crate::JetDbPoolReceipt) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetDbPoolReceipt as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetDbPoolReceipt(value: *mut crate::JetDbPoolReceipt) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetDbPoolReceipt as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetDecimal(value: *mut crate::jet_std::JetDecimal) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::JetDecimal as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetDecimal(value: *mut crate::jet_std::JetDecimal) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::JetDecimal as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetDecimal(value: *mut crate::jet_std::JetDecimal) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::JetDecimal as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetDelivery(value: *mut crate::JetDelivery) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetDelivery as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetDeliveryEvent(value: *mut crate::JetDeliveryEvent) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetDeliveryEvent as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetDeliveryReceipt(value: *mut crate::JetDeliveryReceipt) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetDeliveryReceipt as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetDeliveryState(value: *mut crate::JetDeliveryState) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetDeliveryState as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetDNSSrv(value: *mut crate::JetDNSSrv) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetDNSSrv as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetErr(value: *mut crate::JetErr) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetErr as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetErr(value: *mut crate::JetErr) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetErr as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetErr(value: *mut crate::JetErr) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetErr as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetEventResult(value: *mut crate::JetEventResult) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetEventResult as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetFraction(value: *mut crate::jet_std::JetFraction) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::JetFraction as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetFraction(value: *mut crate::jet_std::JetFraction) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::JetFraction as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetHTTPError(value: *mut crate::JetHTTPError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetHTTPError as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetHTTPError(value: *mut crate::JetHTTPError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetHTTPError as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetHTTPError(value: *mut crate::JetHTTPError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetHTTPError as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetHTTPHeaderName(value: *mut crate::JetHTTPHeaderName) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetHTTPHeaderName as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetHTTPHeaderValue(value: *mut crate::JetHTTPHeaderValue) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetHTTPHeaderValue as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetHTTPMethod(value: *mut crate::JetHTTPMethod) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetHTTPMethod as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetHTTPRequest(value: *mut crate::JetHTTPRequest) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetHTTPRequest as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetHTTPResponse(value: *mut crate::JetHTTPResponse) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetHTTPResponse as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetHTTPRouter(value: *mut crate::JetHTTPRouter) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetHTTPRouter as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetHTTPStatus(value: *mut crate::JetHTTPStatus) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetHTTPStatus as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetHTTPVersion(value: *mut crate::JetHTTPVersion) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetHTTPVersion as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetHyperLogLog(value: *mut crate::JetHyperLogLog) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetHyperLogLog as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetInstant(value: *mut crate::JetInstant) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetInstant as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetInstant(value: *mut crate::JetInstant) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetInstant as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetInstant(value: *mut crate::JetInstant) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetInstant as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetIpAddr(value: *mut crate::JetIpAddr) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetIpAddr as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetKey(value: *mut crate::JetKey) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetKey as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetKey(value: *mut crate::JetKey) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetKey as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetKey(value: *mut crate::JetKey) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetKey as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetLayer(value: *mut crate::JetLayer) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetLayer as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetLayer(value: *mut crate::JetLayer) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetLayer as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetLayer(value: *mut crate::JetLayer) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetLayer as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetLocalTime(value: *mut crate::JetLocalTime) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetLocalTime as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetLocalTime(value: *mut crate::JetLocalTime) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetLocalTime as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetLocalTime(value: *mut crate::JetLocalTime) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetLocalTime as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetMIME(value: *mut crate::jet_std::JetMIME) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::JetMIME as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetMIME(value: *mut crate::jet_std::JetMIME) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::JetMIME as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetMIME(value: *mut crate::jet_std::JetMIME) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::JetMIME as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetNetError(value: *mut crate::JetNetError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetNetError as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetNetError(value: *mut crate::JetNetError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetNetError as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetNetError(value: *mut crate::JetNetError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetNetError as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetOrderedMap(value: *mut crate::JetOrderedMap) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetOrderedMap as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetOrderedMap(value: *mut crate::JetOrderedMap) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetOrderedMap as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetOrderedMap(value: *mut crate::JetOrderedMap) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetOrderedMap as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetPath(value: *mut crate::JetPath) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetPath as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetPath(value: *mut crate::JetPath) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetPath as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetPath(value: *mut crate::JetPath) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetPath as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetPeriod(value: *mut crate::JetPeriod) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetPeriod as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetPeriod(value: *mut crate::JetPeriod) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetPeriod as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetPeriod(value: *mut crate::JetPeriod) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetPeriod as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetPoint(value: *mut crate::JetPoint) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetPoint as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetRange(value: *mut crate::JetRange) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetRange as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetRange(value: *mut crate::JetRange) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetRange as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetRange(value: *mut crate::JetRange) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetRange as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetRect(value: *mut crate::JetRect) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetRect as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetReflectField(value: *mut crate::JetReflectField) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetReflectField as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetReflectValue(value: *mut crate::JetReflectValue) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetReflectValue as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetReflectValue(value: *mut crate::JetReflectValue) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetReflectValue as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetRegex(value: *mut crate::jet_std::JetRegex) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::JetRegex as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetRegexMatch(value: *mut crate::jet_std::JetRegexMatch) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::JetRegexMatch as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetReservoirSampler(value: *mut crate::JetReservoirSampler) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetReservoirSampler as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetServiceDelivery(value: *mut crate::JetServiceDelivery) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetServiceDelivery as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetServiceDelivery(value: *mut crate::JetServiceDelivery) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetServiceDelivery as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetServiceEndpoint(value: *mut crate::JetServiceEndpoint) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetServiceEndpoint as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetServiceError(value: *mut crate::JetServiceError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetServiceError as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetServiceRestart(value: *mut crate::JetServiceRestart) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetServiceRestart as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetServiceRestart(value: *mut crate::JetServiceRestart) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetServiceRestart as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetServiceRuntime(value: *mut crate::JetServiceRuntime) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetServiceRuntime as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetServiceRuntime(value: *mut crate::JetServiceRuntime) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetServiceRuntime as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetServiceStateStore(value: *mut crate::JetServiceStateStore) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetServiceStateStore as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetServiceStateStore(value: *mut crate::JetServiceStateStore) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetServiceStateStore as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetServiceTree(value: *mut crate::JetServiceTree) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetServiceTree as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetServiceUpgradeReceipt(value: *mut crate::JetServiceUpgradeReceipt) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetServiceUpgradeReceipt as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetServiceUpgradeReceipt(value: *mut crate::JetServiceUpgradeReceipt) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetServiceUpgradeReceipt as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetServiceUpgradeReceipt(value: *mut crate::JetServiceUpgradeReceipt) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetServiceUpgradeReceipt as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetSize(value: *mut crate::JetSize) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetSize as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetSocketAddr(value: *mut crate::JetSocketAddr) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetSocketAddr as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetStringSet(value: *mut crate::JetStringSet) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetStringSet as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetStringSet(value: *mut crate::JetStringSet) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetStringSet as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetStringSet(value: *mut crate::JetStringSet) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetStringSet as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetTaskFailure(value: *mut crate::JetTaskFailure) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetTaskFailure as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetTaskFailure(value: *mut crate::JetTaskFailure) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetTaskFailure as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetTaskFailure(value: *mut crate::JetTaskFailure) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetTaskFailure as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetTaskOutcome(value: *mut crate::JetTaskOutcome) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetTaskOutcome as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetTaskOutcome(value: *mut crate::JetTaskOutcome) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetTaskOutcome as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetTaskOutcome(value: *mut crate::JetTaskOutcome) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetTaskOutcome as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetTaskStatus(value: *mut crate::JetTaskStatus) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetTaskStatus as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetTaskStatus(value: *mut crate::JetTaskStatus) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetTaskStatus as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetTaskStatus(value: *mut crate::JetTaskStatus) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetTaskStatus as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetTCPListener(value: *mut crate::JetTCPListener) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetTCPListener as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetTCPStream(value: *mut crate::JetTCPStream) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetTCPStream as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetTDigest(value: *mut crate::JetTDigest) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetTDigest as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetTestComparison(value: *mut crate::jet_std::JetTestComparison) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::JetTestComparison as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetTestSuite(value: *mut crate::jet_std::JetTestSuite) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::JetTestSuite as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetTLSStream(value: *mut crate::JetTLSStream) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetTLSStream as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetTLSVersion(value: *mut crate::JetTLSVersion) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetTLSVersion as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetTLSVersion(value: *mut crate::JetTLSVersion) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetTLSVersion as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetUDPPacket(value: *mut crate::JetUDPPacket) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetUDPPacket as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetUDPSocket(value: *mut crate::JetUDPSocket) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetUDPSocket as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetUiAriaRole(value: *mut crate::JetUiAriaRole) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetUiAriaRole as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetURL(value: *mut crate::jet_std::JetURL) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::JetURL as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetURL(value: *mut crate::jet_std::JetURL) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::JetURL as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetURL(value: *mut crate::jet_std::JetURL) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::JetURL as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetWorkflowHandle(value: *mut crate::JetWorkflowHandle) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetWorkflowHandle as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetWorkflowHandle(value: *mut crate::JetWorkflowHandle) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetWorkflowHandle as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetWorkflowHandle(value: *mut crate::JetWorkflowHandle) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetWorkflowHandle as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetWsError(value: *mut crate::JetWsError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetWsError as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetWsError(value: *mut crate::JetWsError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetWsError as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetWsError(value: *mut crate::JetWsError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetWsError as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetZone(value: *mut crate::JetZone) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetZone as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetZone(value: *mut crate::JetZone) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetZone as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetZone(value: *mut crate::JetZone) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetZone as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_JetZonedDateTime(value: *mut crate::JetZonedDateTime) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetZonedDateTime as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_JetZonedDateTime(value: *mut crate::JetZonedDateTime) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetZonedDateTime as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_JetZonedDateTime(value: *mut crate::JetZonedDateTime) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::JetZonedDateTime as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_LogField(value: *mut crate::jet_std::LogField) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::LogField as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_LogField(value: *mut crate::jet_std::LogField) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::LogField as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_ProcessChild(value: *mut crate::jet_std::ProcessChild) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::ProcessChild as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_ProcessPlan(value: *mut crate::jet_std::ProcessPlan) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::ProcessPlan as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_ProcessResourceLimit(value: *mut crate::jet_std::ProcessResourceLimit) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::ProcessResourceLimit as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_ProcessResourceLimit(value: *mut crate::jet_std::ProcessResourceLimit) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::ProcessResourceLimit as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_ProcessResourceLimit(value: *mut crate::jet_std::ProcessResourceLimit) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::ProcessResourceLimit as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_ProcessSpec(value: *mut crate::jet_std::ProcessSpec) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::ProcessSpec as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_RangeError(value: *mut crate::jet_std::RangeError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::RangeError as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_RangeError(value: *mut crate::jet_std::RangeError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::RangeError as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_RangeError(value: *mut crate::jet_std::RangeError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::RangeError as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_RaylibTextureAtlas(value: *mut crate::RaylibTextureAtlas) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::RaylibTextureAtlas as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_RaylibTextureAtlas(value: *mut crate::RaylibTextureAtlas) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::RaylibTextureAtlas as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_RegexFlags(value: *mut crate::jet_std::RegexFlags) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::RegexFlags as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_Rng(value: *mut crate::jet_std::Rng) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::Rng as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_Solver(value: *mut crate::jet_std::Solver) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::Solver as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_Stat(value: *mut crate::jet_std::Stat) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::Stat as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_Stopwatch(value: *mut crate::jet_std::Stopwatch) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::Stopwatch as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_TextError(value: *mut crate::jet_std::TextError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::TextError as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_TextError(value: *mut crate::jet_std::TextError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::TextError as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_TextError(value: *mut crate::jet_std::TextError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::TextError as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_UTF8Error(value: *mut crate::jet_std::UTF8Error) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::UTF8Error as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_UTF8Error(value: *mut crate::jet_std::UTF8Error) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::UTF8Error as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_UTF8Error(value: *mut crate::jet_std::UTF8Error) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::UTF8Error as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_WalkEntry(value: *mut crate::jet_std::WalkEntry) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::WalkEntry as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_WatchEvent(value: *mut crate::jet_std::WatchEvent) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::WatchEvent as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_display_XMLError(value: *mut crate::jet_std::XMLError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::XMLError as crate::JetDisplay>::jet_display(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_show_XMLError(value: *mut crate::jet_std::XMLError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::XMLError as crate::JetShow>::jet_show(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_debug_XMLError(value: *mut crate::jet_std::XMLError) -> JetCString {
        // SAFETY: `value` is a live handle the caller lends for this call.
        guard(|| handle(<crate::jet_std::XMLError as crate::JetDebug>::jet_debug(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_Address(left: *mut crate::jet_email::Address, right: *mut crate::jet_email::Address) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_Attachment(left: *mut crate::jet_email::Attachment, right: *mut crate::jet_email::Attachment) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_CBORError(left: *mut crate::jet_std::CBORError, right: *mut crate::jet_std::CBORError) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_CBORErrorKind(left: *mut crate::jet_std::CBORErrorKind, right: *mut crate::jet_std::CBORErrorKind) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_CBOROptions(left: *mut crate::jet_std::CBOROptions, right: *mut crate::jet_std::CBOROptions) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_Clock(left: *mut crate::jet_std::Clock, right: *mut crate::jet_std::Clock) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_Closed(left: *mut crate::jet_std::Closed, right: *mut crate::jet_std::Closed) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_CSVRow(left: *mut crate::jet_std::CSVRow, right: *mut crate::jet_std::CSVRow) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DataAuthority(left: *mut crate::jet_std::DataAuthority, right: *mut crate::jet_std::DataAuthority) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DataColumn(left: *mut crate::jet_std::DataColumn, right: *mut crate::jet_std::DataColumn) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DataError(left: *mut crate::jet_std::DataError, right: *mut crate::jet_std::DataError) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DataErrorKind(left: *mut crate::jet_std::DataErrorKind, right: *mut crate::jet_std::DataErrorKind) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DataEvent(left: *mut crate::jet_std::DataEvent, right: *mut crate::jet_std::DataEvent) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DataFormat(left: *mut crate::jet_std::DataFormat, right: *mut crate::jet_std::DataFormat) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DataFreshness(left: *mut crate::jet_std::DataFreshness, right: *mut crate::jet_std::DataFreshness) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DataInvalidationCause(left: *mut crate::jet_std::DataInvalidationCause, right: *mut crate::jet_std::DataInvalidationCause) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DataLimits(left: *mut crate::jet_std::DataLimits, right: *mut crate::jet_std::DataLimits) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DataLineOptions(left: *mut crate::jet_std::DataLineOptions, right: *mut crate::jet_std::DataLineOptions) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DataLoaderKind(left: *mut crate::jet_std::DataLoaderKind, right: *mut crate::jet_std::DataLoaderKind) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DataLoaderStatus(left: *mut crate::jet_std::DataLoaderStatus, right: *mut crate::jet_std::DataLoaderStatus) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DataPivotCell(left: *mut crate::jet_std::DataPivotCell, right: *mut crate::jet_std::DataPivotCell) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DataProvenance(left: *mut crate::jet_std::DataProvenance, right: *mut crate::jet_std::DataProvenance) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DataSnapshotIdentity(left: *mut crate::jet_std::DataSnapshotIdentity, right: *mut crate::jet_std::DataSnapshotIdentity) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DataSourceIdentity(left: *mut crate::jet_std::DataSourceIdentity, right: *mut crate::jet_std::DataSourceIdentity) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DataStatus(left: *mut crate::jet_std::DataStatus, right: *mut crate::jet_std::DataStatus) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DataSummary(left: *mut crate::jet_std::DataSummary, right: *mut crate::jet_std::DataSummary) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DataTree(left: *mut crate::jet_std::DataTree, right: *mut crate::jet_std::DataTree) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DataWatchStatus(left: *mut crate::jet_std::DataWatchStatus, right: *mut crate::jet_std::DataWatchStatus) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DBError(left: *mut crate::jet_std::DBError, right: *mut crate::jet_std::DBError) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DBValue(left: *mut crate::jet_std::DBValue, right: *mut crate::jet_std::DBValue) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DirEntry(left: *mut crate::jet_std::DirEntry, right: *mut crate::jet_std::DirEntry) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_Duration(left: *mut crate::jet_std::Duration, right: *mut crate::jet_std::Duration) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_DurationUnit(left: *mut crate::jet_std::DurationUnit, right: *mut crate::jet_std::DurationUnit) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_EncodingCause(left: *mut crate::jet_std::EncodingCause, right: *mut crate::jet_std::EncodingCause) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_EncodingError(left: *mut crate::jet_std::EncodingError, right: *mut crate::jet_std::EncodingError) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_EncodingErrorKind(left: *mut crate::jet_std::EncodingErrorKind, right: *mut crate::jet_std::EncodingErrorKind) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_EncodingFormat(left: *mut crate::jet_std::EncodingFormat, right: *mut crate::jet_std::EncodingFormat) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_EncodingLimits(left: *mut crate::jet_std::EncodingLimits, right: *mut crate::jet_std::EncodingLimits) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_Envelope(left: *mut crate::jet_email::Envelope, right: *mut crate::jet_email::Envelope) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_EnvError(left: *mut crate::jet_std::EnvError, right: *mut crate::jet_std::EnvError) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_Error(left: *mut crate::jet_email::Error, right: *mut crate::jet_email::Error) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_F64x4(left: *mut crate::jet_std::F64x4, right: *mut crate::jet_std::F64x4) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_Fake(left: *mut crate::jet_std::Fake, right: *mut crate::jet_std::Fake) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_FieldError(left: *mut crate::jet_std::FieldError, right: *mut crate::jet_std::FieldError) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_IOContext(left: *mut crate::jet_std::IOContext, right: *mut crate::jet_std::IOContext) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_IOError(left: *mut crate::jet_std::IOError, right: *mut crate::jet_std::IOError) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_IOOperation(left: *mut crate::jet_std::IOOperation, right: *mut crate::jet_std::IOOperation) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetAuthority(left: *mut crate::JetAuthority, right: *mut crate::JetAuthority) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetBrowserError(left: *mut crate::JetBrowserError, right: *mut crate::JetBrowserError) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetBrowserLocked(left: *mut crate::JetBrowserLocked, right: *mut crate::JetBrowserLocked) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetBrowserProfile(left: *mut crate::JetBrowserProfile, right: *mut crate::JetBrowserProfile) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetBrowserTimeout(left: *mut crate::JetBrowserTimeout, right: *mut crate::JetBrowserTimeout) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetCoord2(left: *mut crate::JetCoord2, right: *mut crate::JetCoord2) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotAccessibility(left: *mut crate::JetDataPlotAccessibility, right: *mut crate::JetDataPlotAccessibility) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotAggregate(left: *mut crate::JetDataPlotAggregate, right: *mut crate::JetDataPlotAggregate) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotAxis(left: *mut crate::JetDataPlotAxis, right: *mut crate::JetDataPlotAxis) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotBackend(left: *mut crate::JetDataPlotBackend, right: *mut crate::JetDataPlotBackend) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotCapability(left: *mut crate::JetDataPlotCapability, right: *mut crate::JetDataPlotCapability) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotChannel(left: *mut crate::JetDataPlotChannel, right: *mut crate::JetDataPlotChannel) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotDomain(left: *mut crate::JetDataPlotDomain, right: *mut crate::JetDataPlotDomain) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotEncoding(left: *mut crate::JetDataPlotEncoding, right: *mut crate::JetDataPlotEncoding) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotError(left: *mut crate::JetDataPlotError, right: *mut crate::JetDataPlotError) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotErrorKind(left: *mut crate::JetDataPlotErrorKind, right: *mut crate::JetDataPlotErrorKind) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotFacet(left: *mut crate::JetDataPlotFacet, right: *mut crate::JetDataPlotFacet) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotFacetKind(left: *mut crate::JetDataPlotFacetKind, right: *mut crate::JetDataPlotFacetKind) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotField(left: *mut crate::JetDataPlotField, right: *mut crate::JetDataPlotField) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotFilterOp(left: *mut crate::JetDataPlotFilterOp, right: *mut crate::JetDataPlotFilterOp) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotInspection(left: *mut crate::JetDataPlotInspection, right: *mut crate::JetDataPlotInspection) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotInteraction(left: *mut crate::JetDataPlotInteraction, right: *mut crate::JetDataPlotInteraction) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotLayer(left: *mut crate::JetDataPlotLayer, right: *mut crate::JetDataPlotLayer) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotLayout(left: *mut crate::JetDataPlotLayout, right: *mut crate::JetDataPlotLayout) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotLegend(left: *mut crate::JetDataPlotLegend, right: *mut crate::JetDataPlotLegend) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotLegendPosition(left: *mut crate::JetDataPlotLegendPosition, right: *mut crate::JetDataPlotLegendPosition) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotMark(left: *mut crate::JetDataPlotMark, right: *mut crate::JetDataPlotMark) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotPlan(left: *mut crate::JetDataPlotPlan, right: *mut crate::JetDataPlotPlan) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotProjection(left: *mut crate::JetDataPlotProjection, right: *mut crate::JetDataPlotProjection) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotRender(left: *mut crate::JetDataPlotRender, right: *mut crate::JetDataPlotRender) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotRenderFormat(left: *mut crate::JetDataPlotRenderFormat, right: *mut crate::JetDataPlotRenderFormat) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotScale(left: *mut crate::JetDataPlotScale, right: *mut crate::JetDataPlotScale) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotScaleKind(left: *mut crate::JetDataPlotScaleKind, right: *mut crate::JetDataPlotScaleKind) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotSchema(left: *mut crate::JetDataPlotSchema, right: *mut crate::JetDataPlotSchema) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotSelectedRow(left: *mut crate::JetDataPlotSelectedRow, right: *mut crate::JetDataPlotSelectedRow) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotSourceFacts(left: *mut crate::JetDataPlotSourceFacts, right: *mut crate::JetDataPlotSourceFacts) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotSupport(left: *mut crate::JetDataPlotSupport, right: *mut crate::JetDataPlotSupport) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDataPlotValue(left: *mut crate::JetDataPlotValue, right: *mut crate::JetDataPlotValue) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDate(left: *mut crate::JetDate, right: *mut crate::JetDate) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDateTime(left: *mut crate::JetDateTime, right: *mut crate::JetDateTime) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDbPoolLifecycle(left: *mut crate::JetDbPoolLifecycle, right: *mut crate::JetDbPoolLifecycle) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDbPoolReceipt(left: *mut crate::JetDbPoolReceipt, right: *mut crate::JetDbPoolReceipt) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDecimal(left: *mut crate::jet_std::JetDecimal, right: *mut crate::jet_std::JetDecimal) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDelivery(left: *mut crate::JetDelivery, right: *mut crate::JetDelivery) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDeliveryEvent(left: *mut crate::JetDeliveryEvent, right: *mut crate::JetDeliveryEvent) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDeliveryReceipt(left: *mut crate::JetDeliveryReceipt, right: *mut crate::JetDeliveryReceipt) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDeliveryState(left: *mut crate::JetDeliveryState, right: *mut crate::JetDeliveryState) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetDispatchState(left: *mut crate::jet_std::JetDispatchState, right: *mut crate::jet_std::JetDispatchState) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetErr(left: *mut crate::JetErr, right: *mut crate::JetErr) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetEventConfigError(left: *mut crate::jet_std::JetEventConfigError, right: *mut crate::jet_std::JetEventConfigError) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetEventOverflow(left: *mut crate::jet_std::JetEventOverflow, right: *mut crate::jet_std::JetEventOverflow) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetEventResult(left: *mut crate::JetEventResult, right: *mut crate::JetEventResult) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetFailurePolicy(left: *mut crate::jet_std::JetFailurePolicy, right: *mut crate::jet_std::JetFailurePolicy) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetFileScope(left: *mut crate::JetFileScope, right: *mut crate::JetFileScope) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetFontFace(left: *mut crate::JetFontFace, right: *mut crate::JetFontFace) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetFontStyle(left: *mut crate::JetFontStyle, right: *mut crate::JetFontStyle) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetFraction(left: *mut crate::jet_std::JetFraction, right: *mut crate::jet_std::JetFraction) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetGlyph(left: *mut crate::JetGlyph, right: *mut crate::JetGlyph) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetGlyphRun(left: *mut crate::JetGlyphRun, right: *mut crate::JetGlyphRun) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetGlyphShaper(left: *mut crate::JetGlyphShaper, right: *mut crate::JetGlyphShaper) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetHTTPCompressEncoding(left: *mut crate::JetHTTPCompressEncoding, right: *mut crate::JetHTTPCompressEncoding) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetHTTPError(left: *mut crate::JetHTTPError, right: *mut crate::JetHTTPError) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetHTTPHeaderName(left: *mut crate::JetHTTPHeaderName, right: *mut crate::JetHTTPHeaderName) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetHTTPHeaders(left: *mut crate::JetHTTPHeaders, right: *mut crate::JetHTTPHeaders) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetHTTPHeaderValue(left: *mut crate::JetHTTPHeaderValue, right: *mut crate::JetHTTPHeaderValue) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetHTTPMethod(left: *mut crate::JetHTTPMethod, right: *mut crate::JetHTTPMethod) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetHTTPOperation(left: *mut crate::JetHTTPOperation, right: *mut crate::JetHTTPOperation) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetHTTPStatus(left: *mut crate::JetHTTPStatus, right: *mut crate::JetHTTPStatus) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetHTTPVersion(left: *mut crate::JetHTTPVersion, right: *mut crate::JetHTTPVersion) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetInputEvent(left: *mut crate::JetInputEvent, right: *mut crate::JetInputEvent) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetInstant(left: *mut crate::JetInstant, right: *mut crate::JetInstant) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetIpAddr(left: *mut crate::JetIpAddr, right: *mut crate::JetIpAddr) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetJobError(left: *mut crate::JetJobError, right: *mut crate::JetJobError) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetJobPayload(left: *mut crate::JetJobPayload, right: *mut crate::JetJobPayload) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetJobQueueClaim(left: *mut crate::JetJobQueueClaim, right: *mut crate::JetJobQueueClaim) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetJobQueueDeliveryPolicy(left: *mut crate::JetJobQueueDeliveryPolicy, right: *mut crate::JetJobQueueDeliveryPolicy) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetJobQueueEvent(left: *mut crate::JetJobQueueEvent, right: *mut crate::JetJobQueueEvent) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetJobQueueReceipt(left: *mut crate::JetJobQueueReceipt, right: *mut crate::JetJobQueueReceipt) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetJobQueueRecord(left: *mut crate::JetJobQueueRecord, right: *mut crate::JetJobQueueRecord) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetJobQueueState(left: *mut crate::JetJobQueueState, right: *mut crate::JetJobQueueState) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetJobQueueStatus(left: *mut crate::JetJobQueueStatus, right: *mut crate::JetJobQueueStatus) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetJobResult(left: *mut crate::JetJobResult, right: *mut crate::JetJobResult) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetKey(left: *mut crate::JetKey, right: *mut crate::JetKey) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetLateEventDisposition(left: *mut crate::JetLateEventDisposition, right: *mut crate::JetLateEventDisposition) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetLocalTime(left: *mut crate::JetLocalTime, right: *mut crate::JetLocalTime) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetMIME(left: *mut crate::jet_std::JetMIME, right: *mut crate::jet_std::JetMIME) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetNetDnsError(left: *mut crate::JetNetDnsError, right: *mut crate::JetNetDnsError) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetNetError(left: *mut crate::JetNetError, right: *mut crate::JetNetError) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetNetErrorDetail(left: *mut crate::JetNetErrorDetail, right: *mut crate::JetNetErrorDetail) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetNetReady(left: *mut crate::JetNetReady, right: *mut crate::JetNetReady) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetNetReadyInterest(left: *mut crate::JetNetReadyInterest, right: *mut crate::JetNetReadyInterest) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetNetShutdown(left: *mut crate::JetNetShutdown, right: *mut crate::JetNetShutdown) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetPeriod(left: *mut crate::JetPeriod, right: *mut crate::JetPeriod) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetPoint(left: *mut crate::JetPoint, right: *mut crate::JetPoint) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetRange(left: *mut crate::JetRange, right: *mut crate::JetRange) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetRay2(left: *mut crate::JetRay2, right: *mut crate::JetRay2) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetRealtimeReceipt(left: *mut crate::JetRealtimeReceipt, right: *mut crate::JetRealtimeReceipt) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetRect(left: *mut crate::JetRect, right: *mut crate::JetRect) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetServiceDelivery(left: *mut crate::JetServiceDelivery, right: *mut crate::JetServiceDelivery) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetServiceEndpoint(left: *mut crate::JetServiceEndpoint, right: *mut crate::JetServiceEndpoint) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetServiceError(left: *mut crate::JetServiceError, right: *mut crate::JetServiceError) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetServiceRestart(left: *mut crate::JetServiceRestart, right: *mut crate::JetServiceRestart) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetServiceStateStore(left: *mut crate::JetServiceStateStore, right: *mut crate::JetServiceStateStore) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetServiceUpgradeReceipt(left: *mut crate::JetServiceUpgradeReceipt, right: *mut crate::JetServiceUpgradeReceipt) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetSize(left: *mut crate::JetSize, right: *mut crate::JetSize) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetSizeConstraint(left: *mut crate::JetSizeConstraint, right: *mut crate::JetSizeConstraint) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetSocketAddr(left: *mut crate::JetSocketAddr, right: *mut crate::JetSocketAddr) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetTaskFailure(left: *mut crate::JetTaskFailure, right: *mut crate::JetTaskFailure) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetTaskOutcome(left: *mut crate::JetTaskOutcome, right: *mut crate::JetTaskOutcome) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetTaskStatus(left: *mut crate::JetTaskStatus, right: *mut crate::JetTaskStatus) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetTLSVersion(left: *mut crate::JetTLSVersion, right: *mut crate::JetTLSVersion) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetTransform2(left: *mut crate::JetTransform2, right: *mut crate::JetTransform2) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetTuiCapabilities(left: *mut crate::JetTuiCapabilities, right: *mut crate::JetTuiCapabilities) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetTuiColor(left: *mut crate::JetTuiColor, right: *mut crate::JetTuiColor) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetTuiConstraint(left: *mut crate::JetTuiConstraint, right: *mut crate::JetTuiConstraint) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetTuiDirection(left: *mut crate::JetTuiDirection, right: *mut crate::JetTuiDirection) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetTuiEvent(left: *mut crate::JetTuiEvent, right: *mut crate::JetTuiEvent) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetTuiListState(left: *mut crate::JetTuiListState, right: *mut crate::JetTuiListState) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetTuiStyle(left: *mut crate::JetTuiStyle, right: *mut crate::JetTuiStyle) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetUiAccessibility(left: *mut crate::JetUiAccessibility, right: *mut crate::JetUiAccessibility) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetUiAriaRole(left: *mut crate::JetUiAriaRole, right: *mut crate::JetUiAriaRole) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetUiCapabilityFacts(left: *mut crate::JetUiCapabilityFacts, right: *mut crate::JetUiCapabilityFacts) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetUiFileDialogRequest(left: *mut crate::JetUiFileDialogRequest, right: *mut crate::JetUiFileDialogRequest) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetUiFileFilter(left: *mut crate::JetUiFileFilter, right: *mut crate::JetUiFileFilter) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetUiFsGrant(left: *mut crate::JetUiFsGrant, right: *mut crate::JetUiFsGrant) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetUiFsRights(left: *mut crate::JetUiFsRights, right: *mut crate::JetUiFsRights) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetUiNode(left: *mut crate::JetUiNode, right: *mut crate::JetUiNode) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetUiPreviewViewport(left: *mut crate::JetUiPreviewViewport, right: *mut crate::JetUiPreviewViewport) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetUiShortcut(left: *mut crate::JetUiShortcut, right: *mut crate::JetUiShortcut) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetURL(left: *mut crate::jet_std::JetURL, right: *mut crate::jet_std::JetURL) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWebFormActionError(left: *mut crate::JetWebFormActionError, right: *mut crate::JetWebFormActionError) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWebFormControl(left: *mut crate::JetWebFormControl, right: *mut crate::JetWebFormControl) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWebFormErrorState(left: *mut crate::JetWebFormErrorState, right: *mut crate::JetWebFormErrorState) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWebFormFieldState(left: *mut crate::JetWebFormFieldState, right: *mut crate::JetWebFormFieldState) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWebFormLifecycle(left: *mut crate::JetWebFormLifecycle, right: *mut crate::JetWebFormLifecycle) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWebFormLifecycleStatus(left: *mut crate::JetWebFormLifecycleStatus, right: *mut crate::JetWebFormLifecycleStatus) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWebFormStatus(left: *mut crate::JetWebFormStatus, right: *mut crate::JetWebFormStatus) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWebFormValidationTiming(left: *mut crate::JetWebFormValidationTiming, right: *mut crate::JetWebFormValidationTiming) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWebFormValueType(left: *mut crate::JetWebFormValueType, right: *mut crate::JetWebFormValueType) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWebMutationState(left: *mut crate::JetWebMutationState, right: *mut crate::JetWebMutationState) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWebMutationStatus(left: *mut crate::JetWebMutationStatus, right: *mut crate::JetWebMutationStatus) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWebQueryNetworkMode(left: *mut crate::JetWebQueryNetworkMode, right: *mut crate::JetWebQueryNetworkMode) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWebQueryState(left: *mut crate::JetWebQueryState, right: *mut crate::JetWebQueryState) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWebQueryStatus(left: *mut crate::JetWebQueryStatus, right: *mut crate::JetWebQueryStatus) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWebStoreEvent(left: *mut crate::JetWebStoreEvent, right: *mut crate::JetWebStoreEvent) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWebTableFilter(left: *mut crate::JetWebTableFilter, right: *mut crate::JetWebTableFilter) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWebTablePageMode(left: *mut crate::JetWebTablePageMode, right: *mut crate::JetWebTablePageMode) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWebTableSort(left: *mut crate::JetWebTableSort, right: *mut crate::JetWebTableSort) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWebTableSortDirection(left: *mut crate::JetWebTableSortDirection, right: *mut crate::JetWebTableSortDirection) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWebTableState(left: *mut crate::JetWebTableState, right: *mut crate::JetWebTableState) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWebTableStatus(left: *mut crate::JetWebTableStatus, right: *mut crate::JetWebTableStatus) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWebVirtualPlan(left: *mut crate::JetWebVirtualPlan, right: *mut crate::JetWebVirtualPlan) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWsError(left: *mut crate::JetWsError, right: *mut crate::JetWsError) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetWsMessage(left: *mut crate::JetWsMessage, right: *mut crate::JetWsMessage) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetZone(left: *mut crate::JetZone, right: *mut crate::JetZone) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_JetZonedDateTime(left: *mut crate::JetZonedDateTime, right: *mut crate::JetZonedDateTime) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_Limits(left: *mut crate::jet_email::Limits, right: *mut crate::jet_email::Limits) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_LogField(left: *mut crate::jet_std::LogField, right: *mut crate::jet_std::LogField) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_LogSpan(left: *mut crate::jet_std::LogSpan, right: *mut crate::jet_std::LogSpan) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_Mat3(left: *mut crate::jet_std::Mat3, right: *mut crate::jet_std::Mat3) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_Mat4(left: *mut crate::jet_std::Mat4, right: *mut crate::jet_std::Mat4) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_Message(left: *mut crate::jet_email::Message, right: *mut crate::jet_email::Message) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_ProcessChild(left: *mut crate::jet_std::ProcessChild, right: *mut crate::jet_std::ProcessChild) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_ProcessPlan(left: *mut crate::jet_std::ProcessPlan, right: *mut crate::jet_std::ProcessPlan) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_ProcessReceipt(left: *mut crate::jet_std::ProcessReceipt, right: *mut crate::jet_std::ProcessReceipt) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_ProcessResourceLimit(left: *mut crate::jet_std::ProcessResourceLimit, right: *mut crate::jet_std::ProcessResourceLimit) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_ProcessSpec(left: *mut crate::jet_std::ProcessSpec, right: *mut crate::jet_std::ProcessSpec) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_ProcessStreamMode(left: *mut crate::jet_std::ProcessStreamMode, right: *mut crate::jet_std::ProcessStreamMode) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_RangeError(left: *mut crate::jet_std::RangeError, right: *mut crate::jet_std::RangeError) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_RecipientPolicy(left: *mut crate::jet_email::RecipientPolicy, right: *mut crate::jet_email::RecipientPolicy) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_RecipientReport(left: *mut crate::jet_email::RecipientReport, right: *mut crate::jet_email::RecipientReport) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_RegexFlags(left: *mut crate::jet_std::RegexFlags, right: *mut crate::jet_std::RegexFlags) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_Rng(left: *mut crate::jet_std::Rng, right: *mut crate::jet_std::Rng) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_SendReport(left: *mut crate::jet_email::SendReport, right: *mut crate::jet_email::SendReport) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_SMTPSecurity(left: *mut crate::jet_email::SMTPSecurity, right: *mut crate::jet_email::SMTPSecurity) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_Solver(left: *mut crate::jet_std::Solver, right: *mut crate::jet_std::Solver) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_Stat(left: *mut crate::jet_std::Stat, right: *mut crate::jet_std::Stat) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_TerminalMode(left: *mut crate::jet_std::TerminalMode, right: *mut crate::jet_std::TerminalMode) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_TerminalPolicy(left: *mut crate::jet_std::TerminalPolicy, right: *mut crate::jet_std::TerminalPolicy) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_TerminalSession(left: *mut crate::jet_std::TerminalSession, right: *mut crate::jet_std::TerminalSession) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_TerminalSize(left: *mut crate::jet_std::TerminalSize, right: *mut crate::jet_std::TerminalSize) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_TextError(left: *mut crate::jet_std::TextError, right: *mut crate::jet_std::TextError) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_TextWidth(left: *mut crate::jet_std::TextWidth, right: *mut crate::jet_std::TextWidth) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_TextWidthAmbiguous(left: *mut crate::jet_std::TextWidthAmbiguous, right: *mut crate::jet_std::TextWidthAmbiguous) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_TextWidthControls(left: *mut crate::jet_std::TextWidthControls, right: *mut crate::jet_std::TextWidthControls) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_TLSTrust(left: *mut crate::jet_email::TLSTrust, right: *mut crate::jet_email::TLSTrust) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_UTF8Error(left: *mut crate::jet_std::UTF8Error, right: *mut crate::jet_std::UTF8Error) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_Vec2(left: *mut crate::jet_std::Vec2, right: *mut crate::jet_std::Vec2) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_Vec3(left: *mut crate::jet_std::Vec3, right: *mut crate::jet_std::Vec3) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_Vec4(left: *mut crate::jet_std::Vec4, right: *mut crate::jet_std::Vec4) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_WalkEntry(left: *mut crate::jet_std::WalkEntry, right: *mut crate::jet_std::WalkEntry) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_WatchDomain(left: *mut crate::jet_std::WatchDomain, right: *mut crate::jet_std::WatchDomain) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_WatchEvent(left: *mut crate::jet_std::WatchEvent, right: *mut crate::jet_std::WatchEvent) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_WatchKind(left: *mut crate::jet_std::WatchKind, right: *mut crate::jet_std::WatchKind) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_XMLCanonical(left: *mut crate::jet_std::XMLCanonical, right: *mut crate::jet_std::XMLCanonical) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_XMLCanonicalMode(left: *mut crate::jet_std::XMLCanonicalMode, right: *mut crate::jet_std::XMLCanonicalMode) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_XMLEncoding(left: *mut crate::jet_std::XMLEncoding, right: *mut crate::jet_std::XMLEncoding) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_XMLEntityPolicy(left: *mut crate::jet_std::XMLEntityPolicy, right: *mut crate::jet_std::XMLEntityPolicy) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_XMLError(left: *mut crate::jet_std::XMLError, right: *mut crate::jet_std::XMLError) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_XMLLexicalPolicy(left: *mut crate::jet_std::XMLLexicalPolicy, right: *mut crate::jet_std::XMLLexicalPolicy) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_XMLLimits(left: *mut crate::jet_std::XMLLimits, right: *mut crate::jet_std::XMLLimits) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_XMLParseOptions(left: *mut crate::jet_std::XMLParseOptions, right: *mut crate::jet_std::XMLParseOptions) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_XMLReason(left: *mut crate::jet_std::XMLReason, right: *mut crate::jet_std::XMLReason) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_handle_eq_XMLRenderOptions(left: *mut crate::jet_std::XMLRenderOptions, right: *mut crate::jet_std::XMLRenderOptions) -> bool {
        // SAFETY: both are live handles the caller lends for this call.
        guard(|| unsafe { &*left } == unsafe { &*right })
    }

    /// app.auth_routes, core.web.auth_routes
    #[no_mangle]
    pub extern "C" fn jet_app_auth_routes(a0: *mut crate::JetAuthApp) -> JetCString {
        guard(|| handle(crate::jet_app_auth_routes(unsafe { &*a0 })))
    }

    /// app.auth_show, core.web.auth_show
    #[no_mangle]
    pub extern "C" fn jet_app_auth_show(a0: *mut crate::JetAuthApp) -> JetCString {
        guard(|| handle(crate::jet_app_auth_show(unsafe { &*a0 })))
    }

    /// app.invalidate, core.web.invalidate
    #[no_mangle]
    pub extern "C" fn jet_app_invalidate(a0: JetCString) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_app_invalidate(view(a0).to_owned())))
    }

    /// app.live, core.web.live
    #[no_mangle]
    pub extern "C" fn jet_app_live(a0: JetCString, a1: JetCString) -> *mut crate::JetLiveQuery {
        guard(|| Box::into_raw(Box::new(crate::jet_app_live(view(a0).to_owned(), view(a1).to_owned()))))
    }

    /// app.live_get, core.web.live_get
    #[no_mangle]
    pub extern "C" fn jet_app_live_get(a0: *mut crate::JetLiveQuery) -> JetCString {
        guard(|| handle(crate::jet_app_live_get(unsafe { &*a0 })))
    }

    /// app.live_show, core.web.live_show
    #[no_mangle]
    pub extern "C" fn jet_app_live_show(a0: *mut crate::JetLiveQuery) -> JetCString {
        guard(|| handle(crate::jet_app_live_show(unsafe { &*a0 })))
    }

    /// app.live_stats, core.web.live_stats
    #[no_mangle]
    pub extern "C" fn jet_app_live_stats() -> JetCString {
        guard(|| handle(crate::jet_app_live_stats()))
    }

    /// app.signal_push, core.web.signal_push
    #[no_mangle]
    pub extern "C" fn jet_app_signal_push(a0: *mut crate::JetLiveQuery, a1: JetCString) -> *mut crate::JetLiveQuery {
        guard(|| Box::into_raw(Box::new(crate::jet_app_signal_push(unsafe { &*a0 }, view(a1).to_owned()))))
    }

    /// app.subscribe, core.web.subscribe
    #[no_mangle]
    pub extern "C" fn jet_app_subscribe(a0: JetCString) -> *mut crate::JetLiveQuery {
        guard(|| Box::into_raw(Box::new(crate::jet_app_subscribe(view(a0).to_owned()))))
    }

    /// app.transact_invalidate, core.web.transact_invalidate
    #[no_mangle]
    pub extern "C" fn jet_app_transact_invalidate(a0: JetCString) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_app_transact_invalidate(view(a0).to_owned())))
    }

    /// core.auth.session_cookie
    #[no_mangle]
    pub extern "C" fn jet_auth_session_cookie(a0: *mut crate::JetAuthSession) -> JetCString {
        guard(|| handle(crate::jet_auth_session_cookie(unsafe { &*a0 })))
    }

    /// core.auth.session_id
    #[no_mangle]
    pub extern "C" fn jet_auth_session_id(a0: *mut crate::JetAuthSession) -> JetCString {
        guard(|| handle(crate::jet_auth_session_id(unsafe { &*a0 })))
    }

    /// core.auth.session_show
    #[no_mangle]
    pub extern "C" fn jet_auth_session_show(a0: *mut crate::JetAuthSession) -> JetCString {
        guard(|| handle(crate::jet_auth_session_show(unsafe { &*a0 })))
    }

    /// core.auth.session_user
    #[no_mangle]
    pub extern "C" fn jet_auth_session_user(a0: *mut crate::JetAuthSession) -> JetCString {
        guard(|| handle(crate::jet_auth_session_user(unsafe { &*a0 })))
    }

    /// core.auth.session_validate
    #[no_mangle]
    pub extern "C" fn jet_auth_session_validate(a0: JetCString, a1: i64, ok: *mut *mut crate::JetAuthSession, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_auth_session_validate(unsafe { &*a0 }, a1) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
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

    /// core.web.browser.connect
    #[no_mangle]
    pub extern "C" fn jet_browser_connect(a0: JetCString, ok: *mut *mut crate::JetBrowser, err: *mut *mut crate::JetBrowserError) -> i64 {
        guard(|| match crate::jet_browser_connect(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.web.browser.connect_profile
    #[no_mangle]
    pub extern "C" fn jet_browser_connect_profile(a0: JetCString, a1: *mut crate::JetBrowserProfile, a2: *mut crate::JetBrowserTimeout, ok: *mut *mut crate::JetBrowser, err: *mut *mut crate::JetBrowserError) -> i64 {
        guard(|| match crate::jet_browser_connect_profile(unsafe { &*a0 }, unsafe { &*a1 }, unsafe { &*a2 }.clone()) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.web.browser.locked
    #[no_mangle]
    pub extern "C" fn jet_browser_locked(a0: JetCString, ok: *mut *mut crate::JetBrowserLocked, err: *mut *mut crate::JetBrowserError) -> i64 {
        guard(|| match crate::jet_browser_locked(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.web.browser.profile
    #[no_mangle]
    pub extern "C" fn jet_browser_profile(a0: JetCString, ok: *mut *mut crate::JetBrowserProfile, err: *mut *mut crate::JetBrowserError) -> i64 {
        guard(|| match crate::jet_browser_profile(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.web.browser.timeout
    #[no_mangle]
    pub extern "C" fn jet_browser_timeout(a0: i64, ok: *mut *mut crate::JetBrowserTimeout, err: *mut *mut crate::JetBrowserError) -> i64 {
        guard(|| match crate::jet_browser_timeout(a0) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
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

    /// core.time.calendar.leapdays
    #[no_mangle]
    pub extern "C" fn jet_calendar_leapdays(a0: i64, a1: i64) -> i64 {
        guard(|| (crate::jet_calendar_leapdays(a0, a1)).into_raw())
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

    /// core.time.calendar.monthrange
    #[no_mangle]
    pub extern "C" fn jet_calendar_monthrange(a0: i64, a1: i64, item0: *mut i64, item1: *mut i64) {
        guard(|| {
            let (e0, e1) = crate::jet_calendar_monthrange(a0, a1);
            unsafe { item0.write((e0).into_raw()) };
            unsafe { item1.write((e1).into_raw()) };
        })
    }

    /// core.time.calendar.timegm
    #[no_mangle]
    pub extern "C" fn jet_calendar_timegm(a0: i64, a1: i64, a2: i64, a3: i64, a4: i64, a5: i64) -> i64 {
        guard(|| (crate::jet_calendar_timegm(a0, a1, a2, a3, a4, a5)).into_raw())
    }

    /// core.time.calendar.weekday
    #[no_mangle]
    pub extern "C" fn jet_calendar_weekday(a0: i64, a1: i64, a2: i64) -> i64 {
        guard(|| (crate::jet_calendar_weekday(a0, a1, a2)).into_raw())
    }

    /// core.time.calendar.weekheader
    #[no_mangle]
    pub extern "C" fn jet_calendar_weekheader(a0: i64, a1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_calendar_weekheader(a0, a1).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.handle.clock.advance
    #[no_mangle]
    pub extern "C" fn jet_clock_advance(a0: *mut crate::jet_std::Clock, a1: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_clock_advance(unsafe { &mut *a0 }, a1)))
    }

    /// core.handle.clock.now
    #[no_mangle]
    pub extern "C" fn jet_clock_now(a0: *mut crate::jet_std::Clock) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_clock_now(unsafe { &*a0 })))
    }

    /// core.handle.clock.tick
    #[no_mangle]
    pub extern "C" fn jet_clock_tick(a0: *mut crate::jet_std::Clock, a1: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_clock_tick(unsafe { &mut *a0 }, a1)))
    }

    /// core.handle.clock.wait
    #[no_mangle]
    pub extern "C" fn jet_clock_wait(a0: *mut crate::jet_std::Clock, a1: *mut crate::jet_std::Duration) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_clock_wait(unsafe { &mut *a0 }, unsafe { &*a1 })))
    }

    /// core.collections.bisect_left
    #[no_mangle]
    pub extern "C" fn jet_coll_bisect_left(a0_0: *const u64, a0_1: i64, a1: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_coll_bisect_left(&list_in(a0_0, a0_1, |w| w as i64), a1)))
    }

    /// core.collections.bisect_right
    #[no_mangle]
    pub extern "C" fn jet_coll_bisect_right(a0_0: *const u64, a0_1: i64, a1: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_coll_bisect_right(&list_in(a0_0, a0_1, |w| w as i64), a1)))
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

    /// core.collections.heapify
    #[no_mangle]
    pub extern "C" fn jet_coll_heapify(a0_0: *const u64, a0_1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_coll_heapify(&list_in(a0_0, a0_1, |w| w as i64)).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.collections.heappush
    #[no_mangle]
    pub extern "C" fn jet_coll_heappush(a0_0: *const u64, a0_1: i64, a1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_coll_heappush(&list_in(a0_0, a0_1, |w| w as i64), a1).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.collections.insort_left
    #[no_mangle]
    pub extern "C" fn jet_coll_insort_left(a0_0: *const u64, a0_1: i64, a1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_coll_insort_left(&list_in(a0_0, a0_1, |w| w as i64), a1).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.collections.insort_right
    #[no_mangle]
    pub extern "C" fn jet_coll_insort_right(a0_0: *const u64, a0_1: i64, a1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_coll_insort_right(&list_in(a0_0, a0_1, |w| w as i64), a1).into_iter().map(|e| e as u64).collect(), data))
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

    /// core.collections.merge_sorted
    #[no_mangle]
    pub extern "C" fn jet_coll_merge_sorted(a0_0: *const u64, a0_1: i64, a1_0: *const u64, a1_1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_coll_merge_sorted(&list_in(a0_0, a0_1, |w| w as i64), &list_in(a1_0, a1_1, |w| w as i64)).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.collections.nlargest
    #[no_mangle]
    pub extern "C" fn jet_coll_nlargest(a0: i64, a1_0: *const u64, a1_1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_coll_nlargest(a0, &list_in(a1_0, a1_1, |w| w as i64)).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.collections.nsmallest
    #[no_mangle]
    pub extern "C" fn jet_coll_nsmallest(a0: i64, a1_0: *const u64, a1_1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_coll_nsmallest(a0, &list_in(a1_0, a1_1, |w| w as i64)).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.collections.ordered_map
    #[no_mangle]
    pub extern "C" fn jet_coll_ordered_map() -> *mut crate::JetOrderedMap {
        guard(|| Box::into_raw(Box::new(crate::jet_coll_ordered_map())))
    }

    /// core.math.combinatorics.accumulate
    #[no_mangle]
    pub extern "C" fn jet_comb_accumulate(a0_0: *const u64, a0_1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_comb_accumulate(list_in(a0_0, a0_1, |w| w as i64)).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.math.combinatorics.chain
    #[no_mangle]
    pub extern "C" fn jet_comb_chain(a0_0: *const u64, a0_1: i64, a1_0: *const u64, a1_1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_comb_chain(list_in(a0_0, a0_1, |w| w as i64), list_in(a1_0, a1_1, |w| w as i64)).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.math.combinatorics.compress
    #[no_mangle]
    pub extern "C" fn jet_comb_compress(a0_0: *const u64, a0_1: i64, a1_0: *const u64, a1_1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_comb_compress(list_in(a0_0, a0_1, |w| w as i64), list_in(a1_0, a1_1, |w| w != 0)).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.math.combinatorics.count_from, core.math.combinatorics.count
    #[no_mangle]
    pub extern "C" fn jet_comb_count_from(a0: i64, a1: i64, a2: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_comb_count_from(a0, a1, a2).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.math.combinatorics.cycle
    #[no_mangle]
    pub extern "C" fn jet_comb_cycle(a0_0: *const u64, a0_1: i64, a1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_comb_cycle(list_in(a0_0, a0_1, |w| w as i64), a1).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.math.combinatorics.drop
    #[no_mangle]
    pub extern "C" fn jet_comb_drop(a0_0: *const u64, a0_1: i64, a1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_comb_drop(list_in(a0_0, a0_1, |w| w as i64), a1).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.math.combinatorics.dropwhile
    #[no_mangle]
    pub extern "C" fn jet_comb_dropwhile(a0_0: *const u64, a0_1: i64, a1: u8, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_comb_dropwhile(list_in(a0_0, a0_1, |w| w as i64), a1 != 0).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.math.combinatorics.filterfalse
    #[no_mangle]
    pub extern "C" fn jet_comb_filterfalse(a0_0: *const u64, a0_1: i64, a1: u8, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_comb_filterfalse(list_in(a0_0, a0_1, |w| w as i64), a1 != 0).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.math.combinatorics.islice
    #[no_mangle]
    pub extern "C" fn jet_comb_islice(a0_0: *const u64, a0_1: i64, a1: i64, a2: i64, a3: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_comb_islice(list_in(a0_0, a0_1, |w| w as i64), a1, a2, a3).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.math.combinatorics.repeat
    #[no_mangle]
    pub extern "C" fn jet_comb_repeat(a0: i64, a1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_comb_repeat(a0, a1).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.math.combinatorics.reverse
    #[no_mangle]
    pub extern "C" fn jet_comb_reverse(a0_0: *const u64, a0_1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_comb_reverse(list_in(a0_0, a0_1, |w| w as i64)).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.math.combinatorics.takewhile
    #[no_mangle]
    pub extern "C" fn jet_comb_takewhile(a0_0: *const u64, a0_1: i64, a1: u8, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_comb_takewhile(list_in(a0_0, a0_1, |w| w as i64), a1 != 0).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.math.combinatorics.unique
    #[no_mangle]
    pub extern "C" fn jet_comb_unique(a0_0: *const u64, a0_1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_comb_unique(list_in(a0_0, a0_1, |w| w as i64)).into_iter().map(|e| e as u64).collect(), data))
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

    /// core.handle.cursor.skip_ws
    #[no_mangle]
    pub extern "C" fn jet_cursor_skip_ws(a0: *mut crate::JetCursor) {
        guard(|| crate::jet_cursor_skip_ws(unsafe { &mut *a0 }))
    }

    /// core.handle.cursor.take_until
    #[no_mangle]
    pub extern "C" fn jet_cursor_take_until(a0: *mut crate::JetCursor, a1: JetCString, ok: *mut JetCString, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_cursor_take_until(unsafe { &mut *a0 }, unsafe { &*a1 }) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.data.describe
    #[no_mangle]
    pub extern "C" fn jet_data_describe_checked(a0_0: *const u64, a0_1: i64, ok: *mut *mut crate::jet_std::DataSummary, err: *mut *mut crate::jet_std::DataError) -> i64 {
        guard(|| match crate::jet_data_describe_checked(&list_in(a0_0, a0_1, |w| f64::from_bits(w))) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.data.loader.authority
    #[no_mangle]
    pub extern "C" fn jet_data_loader_authority(a0: JetCString, a1: JetCString, ok: *mut *mut crate::jet_std::DataAuthority, err: *mut *mut crate::jet_std::DataError) -> i64 {
        guard(|| match crate::jet_data_loader_authority(view(a0), view(a1)) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.data.mean
    #[no_mangle]
    pub extern "C" fn jet_data_mean_checked(a0_0: *const u64, a0_1: i64, ok: *mut f64, err: *mut *mut crate::jet_std::DataError) -> i64 {
        guard(|| match crate::jet_data_mean_checked(&list_in(a0_0, a0_1, |w| f64::from_bits(w))) {
            Ok(value) => { unsafe { ok.write(value) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.data.quantile
    #[no_mangle]
    pub extern "C" fn jet_data_quantile_checked(a0_0: *const u64, a0_1: i64, a1: f64, ok: *mut f64, err: *mut *mut crate::jet_std::DataError) -> i64 {
        guard(|| match crate::jet_data_quantile_checked(&list_in(a0_0, a0_1, |w| f64::from_bits(w)), a1) {
            Ok(value) => { unsafe { ok.write(value) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.data.require_bridge
    #[no_mangle]
    pub extern "C" fn jet_data_require_bridge(a0: JetCString, err: *mut *mut crate::jet_std::DataError) -> i64 {
        guard(|| match crate::jet_data_require_bridge(unsafe { &*a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.data.rolling_mean
    #[no_mangle]
    pub extern "C" fn jet_data_rolling_mean_checked(a0_0: *const u64, a0_1: i64, a1: i64, ok: *mut *mut u64, ok_len: *mut i64, err: *mut *mut crate::jet_std::DataError) -> i64 {
        guard(|| match crate::jet_data_rolling_mean_checked(&list_in(a0_0, a0_1, |w| f64::from_bits(w)), a1) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| e.to_bits()).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.data.loader.snapshot_reusable
    #[no_mangle]
    pub extern "C" fn jet_data_snapshot_reusable(a0: *mut crate::jet_std::DataSnapshotIdentity, a1: *mut crate::jet_std::DataSnapshotIdentity) -> bool {
        guard(|| crate::jet_data_snapshot_reusable(unsafe { &*a0 }, unsafe { &*a1 }))
    }

    /// core.data.stream.cancel
    #[no_mangle]
    pub extern "C" fn jet_data_stream_cancel(a0: *mut crate::jet_std::DataStream) {
        guard(|| crate::jet_data_stream_cancel(unsafe { &mut *a0 }))
    }

    /// core.data.sum
    #[no_mangle]
    pub extern "C" fn jet_data_sum_checked(a0_0: *const u64, a0_1: i64, ok: *mut f64, err: *mut *mut crate::jet_std::DataError) -> i64 {
        guard(|| match crate::jet_data_sum_checked(&list_in(a0_0, a0_1, |w| f64::from_bits(w))) {
            Ok(value) => { unsafe { ok.write(value) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.data.variance
    #[no_mangle]
    pub extern "C" fn jet_data_variance_checked(a0_0: *const u64, a0_1: i64, ok: *mut f64, err: *mut *mut crate::jet_std::DataError) -> i64 {
        guard(|| match crate::jet_data_variance_checked(&list_in(a0_0, a0_1, |w| f64::from_bits(w))) {
            Ok(value) => { unsafe { ok.write(value) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.precise.decimal_add
    #[no_mangle]
    pub extern "C" fn jet_decimal_add(a0: *mut crate::jet_std::JetDecimal, a1: *mut crate::jet_std::JetDecimal) -> *mut crate::jet_std::JetDecimal {
        guard(|| Box::into_raw(Box::new(crate::jet_decimal_add(unsafe { &*a0 }, unsafe { &*a1 }))))
    }

    /// core.precise.decimal_ceil
    #[no_mangle]
    pub extern "C" fn jet_decimal_ceil(a0: *mut crate::jet_std::JetDecimal) -> *mut crate::jet_std::JetDecimal {
        guard(|| Box::into_raw(Box::new(crate::jet_decimal_ceil(unsafe { &*a0 }))))
    }

    /// core.precise.decimal_div
    #[no_mangle]
    pub extern "C" fn jet_decimal_div(a0: *mut crate::jet_std::JetDecimal, a1: *mut crate::jet_std::JetDecimal) -> *mut crate::jet_std::JetFraction {
        guard(|| Box::into_raw(Box::new(crate::jet_decimal_div(unsafe { &*a0 }, unsafe { &*a1 }))))
    }

    /// core.precise.decimal_equal
    #[no_mangle]
    pub extern "C" fn jet_decimal_equal(a0: *mut crate::jet_std::JetDecimal, a1: *mut crate::jet_std::JetDecimal) -> bool {
        guard(|| crate::jet_decimal_equal(unsafe { &*a0 }, unsafe { &*a1 }))
    }

    /// core.precise.decimal_floor
    #[no_mangle]
    pub extern "C" fn jet_decimal_floor(a0: *mut crate::jet_std::JetDecimal) -> *mut crate::jet_std::JetDecimal {
        guard(|| Box::into_raw(Box::new(crate::jet_decimal_floor(unsafe { &*a0 }))))
    }

    /// core.precise.decimal_from_float
    #[no_mangle]
    pub extern "C" fn jet_decimal_from_float(a0: f64) -> *mut crate::jet_std::JetDecimal {
        guard(|| Box::into_raw(Box::new(crate::jet_decimal_from_float(a0))))
    }

    /// core.precise.decimal_from_fraction
    #[no_mangle]
    pub extern "C" fn jet_decimal_from_fraction(a0: *mut crate::jet_std::JetFraction) -> *mut crate::jet_std::JetDecimal {
        guard(|| Box::into_raw(Box::new(crate::jet_decimal_from_fraction(unsafe { &*a0 }.clone()))))
    }

    /// core.precise.decimal_from_int
    #[no_mangle]
    pub extern "C" fn jet_decimal_from_int(a0: i64) -> *mut crate::jet_std::JetDecimal {
        guard(|| Box::into_raw(Box::new(crate::jet_decimal_from_int(a0))))
    }

    /// core.math.decimal, core.precise.decimal_from_str
    #[no_mangle]
    pub extern "C" fn jet_decimal_from_str(a0: JetCString) -> *mut crate::jet_std::JetDecimal {
        guard(|| Box::into_raw(Box::new(crate::jet_decimal_from_str(unsafe { &*a0 }))))
    }

    /// core.precise.decimal_mul
    #[no_mangle]
    pub extern "C" fn jet_decimal_mul(a0: *mut crate::jet_std::JetDecimal, a1: *mut crate::jet_std::JetDecimal) -> *mut crate::jet_std::JetDecimal {
        guard(|| Box::into_raw(Box::new(crate::jet_decimal_mul(unsafe { &*a0 }, unsafe { &*a1 }))))
    }

    /// core.precise.decimal_round
    #[no_mangle]
    pub extern "C" fn jet_decimal_round(a0: *mut crate::jet_std::JetDecimal) -> *mut crate::jet_std::JetDecimal {
        guard(|| Box::into_raw(Box::new(crate::jet_decimal_round(unsafe { &*a0 }))))
    }

    /// core.precise.decimal_sub
    #[no_mangle]
    pub extern "C" fn jet_decimal_sub(a0: *mut crate::jet_std::JetDecimal, a1: *mut crate::jet_std::JetDecimal) -> *mut crate::jet_std::JetDecimal {
        guard(|| Box::into_raw(Box::new(crate::jet_decimal_sub(unsafe { &*a0 }, unsafe { &*a1 }))))
    }

    /// core.precise.decimal_to_float
    #[no_mangle]
    pub extern "C" fn jet_decimal_to_float(a0: *mut crate::jet_std::JetDecimal) -> f64 {
        guard(|| crate::jet_decimal_to_float(unsafe { &*a0 }))
    }

    /// core.precise.decimal_to_fraction
    #[no_mangle]
    pub extern "C" fn jet_decimal_to_fraction(a0: *mut crate::jet_std::JetDecimal) -> *mut crate::jet_std::JetFraction {
        guard(|| Box::into_raw(Box::new(crate::jet_decimal_to_fraction(unsafe { &*a0 }))))
    }

    /// core.precise.decimal_to_int
    #[no_mangle]
    pub extern "C" fn jet_decimal_to_int(a0: *mut crate::jet_std::JetDecimal) -> i64 {
        guard(|| crate::jet_decimal_to_int(unsafe { &*a0 }))
    }

    /// core.precise.decimal_to_string
    #[no_mangle]
    pub extern "C" fn jet_decimal_to_string(a0: *mut crate::jet_std::JetDecimal) -> JetCString {
        guard(|| handle(crate::jet_decimal_to_string(unsafe { &*a0 })))
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

    /// core.email.address
    #[no_mangle]
    pub extern "C" fn address(a0: JetCString, ok: *mut *mut crate::jet_email::Address, err: *mut *mut crate::jet_email::Error) -> i64 {
        guard(|| match crate::jet_email::address(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.email.attachment
    #[no_mangle]
    pub extern "C" fn attachment(a0: JetCString, a1: JetCString, a2_0: *const u64, a2_1: i64, ok: *mut *mut crate::jet_email::Attachment, err: *mut *mut crate::jet_email::Error) -> i64 {
        guard(|| match crate::jet_email::attachment(unsafe { &*a0 }, unsafe { &*a1 }, &list_in(a2_0, a2_1, |w| w as u8)) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.email.serialize
    #[no_mangle]
    pub extern "C" fn serialize(a0: *mut crate::jet_email::Message, ok: *mut *mut u64, ok_len: *mut i64, err: *mut *mut crate::jet_email::Error) -> i64 {
        guard(|| match crate::jet_email::serialize(unsafe { &*a0 }) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| e as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.cbor_writer.finish
    #[no_mangle]
    pub extern "C" fn jet_enc_cbor_writer_finish(a0: *mut crate::jet_std::CBORWriter, err: *mut *mut crate::jet_std::EncodingError) -> i64 {
        guard(|| match crate::jet_enc_cbor_writer_finish(unsafe { &mut *a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.cbor_writer.flush
    #[no_mangle]
    pub extern "C" fn jet_enc_cbor_writer_flush(a0: *mut crate::jet_std::CBORWriter, err: *mut *mut crate::jet_std::EncodingError) -> i64 {
        guard(|| match crate::jet_enc_cbor_writer_flush(unsafe { &mut *a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.cbor_writer.write
    #[no_mangle]
    pub extern "C" fn jet_enc_cbor_writer_write(a0: *mut crate::jet_std::CBORWriter, a1: *mut crate::jet_std::DataEvent, err: *mut *mut crate::jet_std::EncodingError) -> i64 {
        guard(|| match crate::jet_enc_cbor_writer_write(unsafe { &mut *a0 }, unsafe { &*a1 }.clone()) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.csv_writer.finish
    #[no_mangle]
    pub extern "C" fn jet_enc_csv_writer_finish(a0: *mut crate::jet_std::CSVWriter, err: *mut *mut crate::jet_std::EncodingError) -> i64 {
        guard(|| match crate::jet_enc_csv_writer_finish(unsafe { &mut *a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.csv_writer.flush
    #[no_mangle]
    pub extern "C" fn jet_enc_csv_writer_flush(a0: *mut crate::jet_std::CSVWriter, err: *mut *mut crate::jet_std::EncodingError) -> i64 {
        guard(|| match crate::jet_enc_csv_writer_flush(unsafe { &mut *a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.csv_writer.write
    #[no_mangle]
    pub extern "C" fn jet_enc_csv_writer_write(a0: *mut crate::jet_std::CSVWriter, a1_0: *const u64, a1_1: i64, err: *mut *mut crate::jet_std::EncodingError) -> i64 {
        guard(|| match crate::jet_enc_csv_writer_write(unsafe { &mut *a0 }, list_in(a1_0, a1_1, |w| view(w as JetCString).to_owned())) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.json_writer.finish
    #[no_mangle]
    pub extern "C" fn jet_enc_json_writer_finish(a0: *mut crate::jet_std::JSONWriter, err: *mut *mut crate::jet_std::EncodingError) -> i64 {
        guard(|| match crate::jet_enc_json_writer_finish(unsafe { &mut *a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.json_writer.flush
    #[no_mangle]
    pub extern "C" fn jet_enc_json_writer_flush(a0: *mut crate::jet_std::JSONWriter, err: *mut *mut crate::jet_std::EncodingError) -> i64 {
        guard(|| match crate::jet_enc_json_writer_flush(unsafe { &mut *a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.json_writer.write
    #[no_mangle]
    pub extern "C" fn jet_enc_json_writer_write(a0: *mut crate::jet_std::JSONWriter, a1: *mut crate::jet_std::DataEvent, err: *mut *mut crate::jet_std::EncodingError) -> i64 {
        guard(|| match crate::jet_enc_json_writer_write(unsafe { &mut *a0 }, unsafe { &*a1 }.clone()) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.jsonl_writer.finish
    #[no_mangle]
    pub extern "C" fn jet_enc_jsonl_writer_finish(a0: *mut crate::jet_std::JSONLWriter, err: *mut *mut crate::jet_std::EncodingError) -> i64 {
        guard(|| match crate::jet_enc_jsonl_writer_finish(unsafe { &mut *a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.jsonl_writer.flush
    #[no_mangle]
    pub extern "C" fn jet_enc_jsonl_writer_flush(a0: *mut crate::jet_std::JSONLWriter, err: *mut *mut crate::jet_std::EncodingError) -> i64 {
        guard(|| match crate::jet_enc_jsonl_writer_flush(unsafe { &mut *a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.jsonl_writer.write
    #[no_mangle]
    pub extern "C" fn jet_enc_jsonl_writer_write(a0: *mut crate::jet_std::JSONLWriter, a1: *mut crate::jet_std::DataTree, err: *mut *mut crate::jet_std::EncodingError) -> i64 {
        guard(|| match crate::jet_enc_jsonl_writer_write(unsafe { &mut *a0 }, unsafe { &*a1 }.clone()) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.xml_writer.finish
    #[no_mangle]
    pub extern "C" fn jet_enc_xml_writer_finish(a0: *mut crate::jet_std::XMLWriter, err: *mut *mut crate::jet_std::EncodingError) -> i64 {
        guard(|| match crate::jet_enc_xml_writer_finish(unsafe { &mut *a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.xml_writer.flush
    #[no_mangle]
    pub extern "C" fn jet_enc_xml_writer_flush(a0: *mut crate::jet_std::XMLWriter, err: *mut *mut crate::jet_std::EncodingError) -> i64 {
        guard(|| match crate::jet_enc_xml_writer_flush(unsafe { &mut *a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.xml_writer.write
    #[no_mangle]
    pub extern "C" fn jet_enc_xml_writer_write(a0: *mut crate::jet_std::XMLWriter, a1: *mut crate::jet_std::DataTree, err: *mut *mut crate::jet_std::EncodingError) -> i64 {
        guard(|| match crate::jet_enc_xml_writer_write(unsafe { &mut *a0 }, unsafe { &*a1 }.clone()) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.errors.entry_error_exit
    #[no_mangle]
    pub extern "C" fn jet_entry_error_exit_jet(a0: *mut crate::JetErr) {
        guard(|| crate::jet_entry_error_exit_jet(unsafe { &*a0 }.clone()))
    }

    /// core.errors.apply_conversion
    #[no_mangle]
    pub extern "C" fn jet_err_apply_conversion(a0: *mut crate::JetErr, a1: JetCString, a2: JetCString) -> *mut crate::JetErr {
        guard(|| Box::into_raw(Box::new(crate::jet_err_apply_conversion(unsafe { &*a0 }.clone(), view(a1).to_owned(), view(a2).to_owned()))))
    }

    /// core.errors.from_message
    #[no_mangle]
    pub extern "C" fn jet_err_from_message(a0: JetCString) -> *mut crate::JetErr {
        guard(|| Box::into_raw(Box::new(crate::jet_err_from_message(view(a0).to_owned()))))
    }

    /// core.errors.err_with_context_frame
    #[no_mangle]
    pub extern "C" fn jet_err_with_context_frame(a0: *mut crate::JetErr, a1: JetCString, a2: i64, a3: i64, a4: JetCString, a5: JetCString) -> *mut crate::JetErr {
        guard(|| Box::into_raw(Box::new(crate::jet_err_with_context_frame(unsafe { &*a0 }.clone(), view(a1), fixed::<u32>(a2), fixed::<u32>(a3), view(a4), view(a5).to_owned()))))
    }

    /// core.handle.fake.address
    #[no_mangle]
    pub extern "C" fn jet_fake_address(a0: *mut crate::jet_std::Fake) -> JetCString {
        guard(|| handle(crate::jet_fake_address(unsafe { &mut *a0 })))
    }

    /// core.handle.fake.email
    #[no_mangle]
    pub extern "C" fn jet_fake_email(a0: *mut crate::jet_std::Fake) -> JetCString {
        guard(|| handle(crate::jet_fake_email(unsafe { &mut *a0 })))
    }

    /// core.handle.fake.host
    #[no_mangle]
    pub extern "C" fn jet_fake_host(a0: *mut crate::jet_std::Fake) -> JetCString {
        guard(|| handle(crate::jet_fake_host(unsafe { &mut *a0 })))
    }

    /// core.handle.fake.locale
    #[no_mangle]
    pub extern "C" fn jet_fake_locale(a0: *mut crate::jet_std::Fake, a1: JetCString) -> *mut crate::jet_std::Fake {
        guard(|| Box::into_raw(Box::new(crate::jet_fake_locale(unsafe { &*a0 }, unsafe { &*a1 }))))
    }

    /// core.handle.fake.name
    #[no_mangle]
    pub extern "C" fn jet_fake_name(a0: *mut crate::jet_std::Fake) -> JetCString {
        guard(|| handle(crate::jet_fake_name(unsafe { &mut *a0 })))
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

    /// core.font.shape
    #[no_mangle]
    pub extern "C" fn jet_font_shape(a0: JetCString, a1: *mut crate::JetFontFace) -> *mut crate::JetGlyphRun {
        guard(|| Box::into_raw(Box::new(crate::jet_font_shape(view(a0), unsafe { &*a1 }))))
    }

    /// core.font.system
    #[no_mangle]
    pub extern "C" fn jet_font_system(a0: i64) -> *mut crate::JetFontFace {
        guard(|| Box::into_raw(Box::new(crate::jet_font_system(match a0 { 0 => crate::JetFontStyle::Body, 1 => crate::JetFontStyle::Title, 2 => crate::JetFontStyle::Monospace, _ => range_stop() }))))
    }

    /// core.precise.fraction_add
    #[no_mangle]
    pub extern "C" fn jet_fraction_add(a0: *mut crate::jet_std::JetFraction, a1: *mut crate::jet_std::JetFraction) -> *mut crate::jet_std::JetFraction {
        guard(|| Box::into_raw(Box::new(crate::jet_fraction_add(unsafe { &*a0 }, unsafe { &*a1 }))))
    }

    /// core.precise.fraction_denominator
    #[no_mangle]
    pub extern "C" fn jet_fraction_denominator(a0: *mut crate::jet_std::JetFraction) -> i64 {
        guard(|| crate::jet_fraction_denominator(unsafe { &*a0 }))
    }

    /// core.precise.fraction_div
    #[no_mangle]
    pub extern "C" fn jet_fraction_div(a0: *mut crate::jet_std::JetFraction, a1: *mut crate::jet_std::JetFraction) -> *mut crate::jet_std::JetFraction {
        guard(|| Box::into_raw(Box::new(crate::jet_fraction_div(unsafe { &*a0 }, unsafe { &*a1 }))))
    }

    /// core.precise.fraction_equal
    #[no_mangle]
    pub extern "C" fn jet_fraction_equal(a0: *mut crate::jet_std::JetFraction, a1: *mut crate::jet_std::JetFraction) -> bool {
        guard(|| crate::jet_fraction_equal(unsafe { &*a0 }, unsafe { &*a1 }))
    }

    /// core.precise.fraction_from_float
    #[no_mangle]
    pub extern "C" fn jet_fraction_from_float(a0: f64) -> *mut crate::jet_std::JetFraction {
        guard(|| Box::into_raw(Box::new(crate::jet_fraction_from_float(a0))))
    }

    /// core.precise.fraction_from_int
    #[no_mangle]
    pub extern "C" fn jet_fraction_from_int(a0: i64) -> *mut crate::jet_std::JetFraction {
        guard(|| Box::into_raw(Box::new(crate::jet_fraction_from_int(a0))))
    }

    /// core.precise.fraction_from_parts
    #[no_mangle]
    pub extern "C" fn jet_fraction_from_parts(a0: i64, a1: i64) -> *mut crate::jet_std::JetFraction {
        guard(|| Box::into_raw(Box::new(crate::jet_fraction_from_parts(a0, a1))))
    }

    /// core.precise.fraction_is_zero
    #[no_mangle]
    pub extern "C" fn jet_fraction_is_zero(a0: *mut crate::jet_std::JetFraction) -> bool {
        guard(|| crate::jet_fraction_is_zero(unsafe { &*a0 }))
    }

    /// core.precise.fraction_mul
    #[no_mangle]
    pub extern "C" fn jet_fraction_mul(a0: *mut crate::jet_std::JetFraction, a1: *mut crate::jet_std::JetFraction) -> *mut crate::jet_std::JetFraction {
        guard(|| Box::into_raw(Box::new(crate::jet_fraction_mul(unsafe { &*a0 }, unsafe { &*a1 }))))
    }

    /// core.math.fraction, core.precise.fraction_new
    #[no_mangle]
    pub extern "C" fn jet_fraction_new(a0: i64, a1: i64, some: *mut *mut crate::jet_std::JetFraction) -> i64 {
        guard(|| match crate::jet_fraction_new(a0, a1) {
            Some(value) => { unsafe { some.write(Box::into_raw(Box::new(value))) }; 1 }
            None => 0,
        })
    }

    /// core.precise.fraction_numerator
    #[no_mangle]
    pub extern "C" fn jet_fraction_numerator(a0: *mut crate::jet_std::JetFraction) -> i64 {
        guard(|| crate::jet_fraction_numerator(unsafe { &*a0 }))
    }

    /// core.precise.fraction_sub
    #[no_mangle]
    pub extern "C" fn jet_fraction_sub(a0: *mut crate::jet_std::JetFraction, a1: *mut crate::jet_std::JetFraction) -> *mut crate::jet_std::JetFraction {
        guard(|| Box::into_raw(Box::new(crate::jet_fraction_sub(unsafe { &*a0 }, unsafe { &*a1 }))))
    }

    /// core.precise.fraction_to_decimal
    #[no_mangle]
    pub extern "C" fn jet_fraction_to_decimal(a0: *mut crate::jet_std::JetFraction) -> *mut crate::jet_std::JetDecimal {
        guard(|| Box::into_raw(Box::new(crate::jet_fraction_to_decimal(unsafe { &*a0 }))))
    }

    /// core.precise.fraction_to_float
    #[no_mangle]
    pub extern "C" fn jet_fraction_to_float(a0: *mut crate::jet_std::JetFraction) -> f64 {
        guard(|| crate::jet_fraction_to_float(unsafe { &*a0 }))
    }

    /// core.precise.fraction_to_int
    #[no_mangle]
    pub extern "C" fn jet_fraction_to_int(a0: *mut crate::jet_std::JetFraction) -> i64 {
        guard(|| crate::jet_fraction_to_int(unsafe { &*a0 }))
    }

    /// core.precise.fraction_to_string
    #[no_mangle]
    pub extern "C" fn jet_fraction_to_string(a0: *mut crate::jet_std::JetFraction) -> JetCString {
        guard(|| handle(crate::jet_fraction_to_string(unsafe { &*a0 })))
    }

    /// core.http.basic_auth
    #[no_mangle]
    pub extern "C" fn jet_http_basic_auth(a0: JetCString, a1: JetCString) -> JetCString {
        guard(|| handle(crate::jet_http_basic_auth(unsafe { &*a0 }, unsafe { &*a1 })))
    }

    /// core.http.bearer_auth
    #[no_mangle]
    pub extern "C" fn jet_http_bearer_auth(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_http_bearer_auth(unsafe { &*a0 })))
    }

    /// core.http.client.request
    #[no_mangle]
    pub extern "C" fn jet_http_client_request_new(a0: JetCString, a1: JetCString) -> *mut crate::JetHTTPRequest {
        guard(|| Box::into_raw(Box::new(crate::jet_http_client_request_new(unsafe { &*a0 }, unsafe { &*a1 }))))
    }

    /// core.http.server.mux
    #[no_mangle]
    pub extern "C" fn jet_http_mux_new() -> *mut crate::JetHTTPMux {
        guard(|| Box::into_raw(Box::new(crate::jet_http_mux_new())))
    }

    /// core.http.parse
    #[no_mangle]
    pub extern "C" fn jet_http_parse_request(a0: JetCString) -> *mut crate::JetHTTPRequest {
        guard(|| Box::into_raw(Box::new(crate::jet_http_parse_request(view(a0)))))
    }

    /// core.http.reason_phrase
    #[no_mangle]
    pub extern "C" fn jet_http_reason_phrase(a0: i64) -> JetCString {
        guard(|| handle(crate::jet_http_reason_phrase(a0)))
    }

    /// core.http.dispatch
    #[no_mangle]
    pub extern "C" fn jet_http_router_dispatch(a0: *mut crate::JetHTTPRouter, a1: *mut crate::JetHTTPRequest, ok: *mut *mut crate::JetHTTPResponse, err: *mut *mut crate::JetHTTPError) -> i64 {
        guard(|| match crate::jet_http_router_dispatch(unsafe { &*a0 }, unsafe { &*a1 }.clone()) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.http.router
    #[no_mangle]
    pub extern "C" fn jet_http_router_new() -> *mut crate::JetHTTPRouter {
        guard(|| Box::into_raw(Box::new(crate::jet_http_router_new())))
    }

    /// core.http.serve
    #[no_mangle]
    pub extern "C" fn jet_http_server_default(a0: *mut crate::JetHTTPMux, a1: *mut crate::jet_std::Duration) -> *mut crate::JetHTTPServer {
        guard(|| Box::into_raw(Box::new(crate::jet_http_server_default(unsafe { &*a0 }, unsafe { &*a1 }))))
    }

    /// core.handle.http_server.local_addr
    #[no_mangle]
    pub extern "C" fn jet_http_server_local_addr(a0: *mut crate::JetHTTPServer, ok: *mut JetCString, err: *mut *mut crate::JetHTTPError) -> i64 {
        guard(|| match crate::jet_http_server_local_addr(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.http_server.serve
    #[no_mangle]
    pub extern "C" fn jet_http_server_serve(a0: *mut crate::JetHTTPServer, ok: *mut *mut crate::JetHTTPShutdownReport, err: *mut *mut crate::JetHTTPError) -> i64 {
        guard(|| match crate::jet_http_server_serve(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.http_server.shutdown
    #[no_mangle]
    pub extern "C" fn jet_http_server_shutdown(a0: *mut crate::JetHTTPServer, a1: *mut crate::jet_std::Duration, ok: *mut *mut crate::JetHTTPShutdownReport, err: *mut *mut crate::JetHTTPError) -> i64 {
        guard(|| match crate::jet_http_server_shutdown(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.http_server.wait
    #[no_mangle]
    pub extern "C" fn jet_http_server_wait(a0: *mut crate::JetHTTPServer, ok: *mut *mut crate::JetHTTPShutdownReport, err: *mut *mut crate::JetHTTPError) -> i64 {
        guard(|| match crate::jet_http_server_wait(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.http.server.access_log
    #[no_mangle]
    pub extern "C" fn jet_http_srv_access_log(a0: *mut crate::JetHTTPRequest, a1: i64) -> JetCString {
        guard(|| handle(crate::jet_http_srv_access_log(unsafe { &*a0 }, a1)))
    }

    /// core.http.server.cors
    #[no_mangle]
    pub extern "C" fn jet_http_srv_install_cors(a0: *mut crate::JetHTTPMux, a1: *mut crate::JetHTTPCorsPolicy) {
        guard(|| crate::jet_http_srv_install_cors(unsafe { &*a0 }, unsafe { &*a1 }))
    }

    /// core.http.server.request_id
    #[no_mangle]
    pub extern "C" fn jet_http_srv_install_request_id(a0: *mut crate::JetHTTPMux) {
        guard(|| crate::jet_http_srv_install_request_id(unsafe { &*a0 }))
    }

    /// core.handle.http.request_trailers
    #[no_mangle]
    pub extern "C" fn jet_http_srv_req_trailers(a0: *mut crate::JetHTTPRequest, ok: *mut *mut crate::JetHTTPHeaders, err: *mut *mut crate::JetHTTPError) -> i64 {
        guard(|| match crate::jet_http_srv_req_trailers(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.http.response_trailers
    #[no_mangle]
    pub extern "C" fn jet_http_srv_response_trailers(a0: *mut crate::JetHTTPResponse, a1: *mut crate::JetHTTPHeaders, ok: *mut *mut crate::JetHTTPResponse, err: *mut *mut crate::JetHTTPError) -> i64 {
        guard(|| match crate::jet_http_srv_response_trailers(unsafe { &*a0 }.clone(), unsafe { &*a1 }.clone()) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.http.server.sse
    #[no_mangle]
    pub extern "C" fn jet_http_srv_sse(a0: JetCString) -> *mut crate::JetHTTPResponse {
        guard(|| Box::into_raw(Box::new(crate::jet_http_srv_sse(unsafe { &*a0 }))))
    }

    /// core.http.server.tls
    #[no_mangle]
    pub extern "C" fn jet_http_srv_tls(a0: JetCString, a1: JetCString) -> *mut crate::JetHTTPServerTls {
        guard(|| Box::into_raw(Box::new(crate::jet_http_srv_tls(unsafe { &*a0 }, unsafe { &*a1 }))))
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

    /// core.math.CameraDelta_add
    #[no_mangle]
    pub extern "C" fn jet_math_CameraDelta_add(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_CameraDelta_add(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.CameraDelta_new
    #[no_mangle]
    pub extern "C" fn jet_math_CameraDelta_new(a0: f64, a1: f64) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_CameraDelta_new(a0, a1))))
    }

    /// core.math.CameraDelta_sub
    #[no_mangle]
    pub extern "C" fn jet_math_CameraDelta_sub(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_CameraDelta_sub(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.CameraPoint_add
    #[no_mangle]
    pub extern "C" fn jet_math_CameraPoint_add(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_CameraPoint_add(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.CameraPoint_new
    #[no_mangle]
    pub extern "C" fn jet_math_CameraPoint_new(a0: f64, a1: f64) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_CameraPoint_new(a0, a1))))
    }

    /// core.math.CameraPoint_sub
    #[no_mangle]
    pub extern "C" fn jet_math_CameraPoint_sub(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_CameraPoint_sub(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.Delta2_add
    #[no_mangle]
    pub extern "C" fn jet_math_Delta2_add(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2, ok: *mut *mut crate::JetCoord2, err: *mut *mut crate::JetErr) -> i64 {
        guard(|| match crate::jet_math_Delta2_add(unsafe { &*a0 }, unsafe { &*a1 }.clone()) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.math.Delta2_new
    #[no_mangle]
    pub extern "C" fn jet_math_Delta2_new(a0: f64, a1: f64, a2: i64) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Delta2_new(a0, a1, a2))))
    }

    /// core.math.Delta2_sub
    #[no_mangle]
    pub extern "C" fn jet_math_Delta2_sub(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2, ok: *mut *mut crate::JetCoord2, err: *mut *mut crate::JetErr) -> i64 {
        guard(|| match crate::jet_math_Delta2_sub(unsafe { &*a0 }, unsafe { &*a1 }.clone()) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.math.DeviceDelta_add
    #[no_mangle]
    pub extern "C" fn jet_math_DeviceDelta_add(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_DeviceDelta_add(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.DeviceDelta_new
    #[no_mangle]
    pub extern "C" fn jet_math_DeviceDelta_new(a0: f64, a1: f64) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_DeviceDelta_new(a0, a1))))
    }

    /// core.math.DeviceDelta_sub
    #[no_mangle]
    pub extern "C" fn jet_math_DeviceDelta_sub(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_DeviceDelta_sub(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.DevicePoint_add
    #[no_mangle]
    pub extern "C" fn jet_math_DevicePoint_add(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_DevicePoint_add(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.DevicePoint_new
    #[no_mangle]
    pub extern "C" fn jet_math_DevicePoint_new(a0: f64, a1: f64) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_DevicePoint_new(a0, a1))))
    }

    /// core.math.DevicePoint_sub
    #[no_mangle]
    pub extern "C" fn jet_math_DevicePoint_sub(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_DevicePoint_sub(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.F64x4_lane
    #[no_mangle]
    pub extern "C" fn jet_math_F64x4_lane(a0: *mut crate::jet_std::F64x4, a1: i64, a2: JetCString, a3: i64) -> f64 {
        guard(|| crate::jet_math_F64x4_lane(unsafe { &*a0 }, a1, view(a2), fixed::<u32>(a3)))
    }

    /// core.math.F64x4_max
    #[no_mangle]
    pub extern "C" fn jet_math_F64x4_max(a0: *mut crate::jet_std::F64x4) -> f64 {
        guard(|| crate::jet_math_F64x4_max(unsafe { &*a0 }))
    }

    /// core.math.F64x4_min
    #[no_mangle]
    pub extern "C" fn jet_math_F64x4_min(a0: *mut crate::jet_std::F64x4) -> f64 {
        guard(|| crate::jet_math_F64x4_min(unsafe { &*a0 }))
    }

    /// core.math.F64x4_new
    #[no_mangle]
    pub extern "C" fn jet_math_F64x4_new(a0: f64, a1: f64, a2: f64, a3: f64) -> *mut crate::jet_std::F64x4 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_F64x4_new(a0, a1, a2, a3))))
    }

    /// core.math.F64x4_product
    #[no_mangle]
    pub extern "C" fn jet_math_F64x4_product(a0: *mut crate::jet_std::F64x4) -> f64 {
        guard(|| crate::jet_math_F64x4_product(unsafe { &*a0 }))
    }

    /// core.math.F64x4_reduce_add
    #[no_mangle]
    pub extern "C" fn jet_math_F64x4_reduce_add(a0: *mut crate::jet_std::F64x4) -> f64 {
        guard(|| crate::jet_math_F64x4_reduce_add(unsafe { &*a0 }))
    }

    /// core.math.F64x4_reduce_avg
    #[no_mangle]
    pub extern "C" fn jet_math_F64x4_reduce_avg(a0: *mut crate::jet_std::F64x4) -> f64 {
        guard(|| crate::jet_math_F64x4_reduce_avg(unsafe { &*a0 }))
    }

    /// core.math.F64x4_reduce_max
    #[no_mangle]
    pub extern "C" fn jet_math_F64x4_reduce_max(a0: *mut crate::jet_std::F64x4) -> f64 {
        guard(|| crate::jet_math_F64x4_reduce_max(unsafe { &*a0 }))
    }

    /// core.math.F64x4_reduce_min
    #[no_mangle]
    pub extern "C" fn jet_math_F64x4_reduce_min(a0: *mut crate::jet_std::F64x4) -> f64 {
        guard(|| crate::jet_math_F64x4_reduce_min(unsafe { &*a0 }))
    }

    /// core.math.F64x4_reduce_mul
    #[no_mangle]
    pub extern "C" fn jet_math_F64x4_reduce_mul(a0: *mut crate::jet_std::F64x4) -> f64 {
        guard(|| crate::jet_math_F64x4_reduce_mul(unsafe { &*a0 }))
    }

    /// core.math.F64x4_splat
    #[no_mangle]
    pub extern "C" fn jet_math_F64x4_splat(a0: f64) -> *mut crate::jet_std::F64x4 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_F64x4_splat(a0))))
    }

    /// core.math.F64x4_sqrt
    #[no_mangle]
    pub extern "C" fn jet_math_F64x4_sqrt(a0: *mut crate::jet_std::F64x4) -> *mut crate::jet_std::F64x4 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_F64x4_sqrt(unsafe { &*a0 }))))
    }

    /// core.math.F64x4_sum
    #[no_mangle]
    pub extern "C" fn jet_math_F64x4_sum(a0: *mut crate::jet_std::F64x4) -> f64 {
        guard(|| crate::jet_math_F64x4_sum(unsafe { &*a0 }))
    }

    /// core.math.Mat3_add
    #[no_mangle]
    pub extern "C" fn jet_math_Mat3_add(a0: *mut crate::jet_std::Mat3, a1: *mut crate::jet_std::Mat3) -> *mut crate::jet_std::Mat3 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Mat3_add(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.Mat3_matmul
    #[no_mangle]
    pub extern "C" fn jet_math_Mat3_matmul(a0: *mut crate::jet_std::Mat3, a1: *mut crate::jet_std::Mat3) -> *mut crate::jet_std::Mat3 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Mat3_matmul(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.Mat3_mul
    #[no_mangle]
    pub extern "C" fn jet_math_Mat3_mul(a0: *mut crate::jet_std::Mat3, a1: *mut crate::jet_std::Mat3) -> *mut crate::jet_std::Mat3 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Mat3_mul(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.Mat3_new
    #[no_mangle]
    pub extern "C" fn jet_math_Mat3_new(a0: f64, a1: f64, a2: f64, a3: f64, a4: f64, a5: f64, a6: f64, a7: f64, a8: f64) -> *mut crate::jet_std::Mat3 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Mat3_new(a0, a1, a2, a3, a4, a5, a6, a7, a8))))
    }

    /// core.math.Mat3_sub
    #[no_mangle]
    pub extern "C" fn jet_math_Mat3_sub(a0: *mut crate::jet_std::Mat3, a1: *mut crate::jet_std::Mat3) -> *mut crate::jet_std::Mat3 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Mat3_sub(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.Mat3_transform
    #[no_mangle]
    pub extern "C" fn jet_math_Mat3_transform(a0: *mut crate::jet_std::Mat3, a1: *mut crate::jet_std::Vec3) -> *mut crate::jet_std::Vec3 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Mat3_transform(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.Mat3_transpose
    #[no_mangle]
    pub extern "C" fn jet_math_Mat3_transpose(a0: *mut crate::jet_std::Mat3) -> *mut crate::jet_std::Mat3 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Mat3_transpose(unsafe { &*a0 }))))
    }

    /// core.math.Mat4_add
    #[no_mangle]
    pub extern "C" fn jet_math_Mat4_add(a0: *mut crate::jet_std::Mat4, a1: *mut crate::jet_std::Mat4) -> *mut crate::jet_std::Mat4 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Mat4_add(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.Mat4_matmul
    #[no_mangle]
    pub extern "C" fn jet_math_Mat4_matmul(a0: *mut crate::jet_std::Mat4, a1: *mut crate::jet_std::Mat4) -> *mut crate::jet_std::Mat4 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Mat4_matmul(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.Mat4_mul
    #[no_mangle]
    pub extern "C" fn jet_math_Mat4_mul(a0: *mut crate::jet_std::Mat4, a1: *mut crate::jet_std::Mat4) -> *mut crate::jet_std::Mat4 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Mat4_mul(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.Mat4_new
    #[no_mangle]
    pub extern "C" fn jet_math_Mat4_new(a0: f64, a1: f64, a2: f64, a3: f64, a4: f64, a5: f64, a6: f64, a7: f64, a8: f64, a9: f64, a10: f64, a11: f64, a12: f64, a13: f64, a14: f64, a15: f64) -> *mut crate::jet_std::Mat4 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Mat4_new(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15))))
    }

    /// core.math.Mat4_sub
    #[no_mangle]
    pub extern "C" fn jet_math_Mat4_sub(a0: *mut crate::jet_std::Mat4, a1: *mut crate::jet_std::Mat4) -> *mut crate::jet_std::Mat4 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Mat4_sub(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.Mat4_transform
    #[no_mangle]
    pub extern "C" fn jet_math_Mat4_transform(a0: *mut crate::jet_std::Mat4, a1: *mut crate::jet_std::Vec4) -> *mut crate::jet_std::Vec4 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Mat4_transform(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.Mat4_transpose
    #[no_mangle]
    pub extern "C" fn jet_math_Mat4_transpose(a0: *mut crate::jet_std::Mat4) -> *mut crate::jet_std::Mat4 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Mat4_transpose(unsafe { &*a0 }))))
    }

    /// core.math.Point2_add
    #[no_mangle]
    pub extern "C" fn jet_math_Point2_add(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2, ok: *mut *mut crate::JetCoord2, err: *mut *mut crate::JetErr) -> i64 {
        guard(|| match crate::jet_math_Point2_add(unsafe { &*a0 }, unsafe { &*a1 }.clone()) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.math.Point2_new
    #[no_mangle]
    pub extern "C" fn jet_math_Point2_new(a0: f64, a1: f64, a2: i64) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Point2_new(a0, a1, a2))))
    }

    /// core.math.Point2_sub
    #[no_mangle]
    pub extern "C" fn jet_math_Point2_sub(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2, ok: *mut *mut crate::JetCoord2, err: *mut *mut crate::JetErr) -> i64 {
        guard(|| match crate::jet_math_Point2_sub(unsafe { &*a0 }, unsafe { &*a1 }.clone()) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.math.ScreenDelta_add
    #[no_mangle]
    pub extern "C" fn jet_math_ScreenDelta_add(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_ScreenDelta_add(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.ScreenDelta_new
    #[no_mangle]
    pub extern "C" fn jet_math_ScreenDelta_new(a0: f64, a1: f64) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_ScreenDelta_new(a0, a1))))
    }

    /// core.math.ScreenDelta_sub
    #[no_mangle]
    pub extern "C" fn jet_math_ScreenDelta_sub(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_ScreenDelta_sub(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.ScreenPoint_add
    #[no_mangle]
    pub extern "C" fn jet_math_ScreenPoint_add(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_ScreenPoint_add(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.ScreenPoint_new
    #[no_mangle]
    pub extern "C" fn jet_math_ScreenPoint_new(a0: f64, a1: f64) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_ScreenPoint_new(a0, a1))))
    }

    /// core.math.ScreenPoint_sub
    #[no_mangle]
    pub extern "C" fn jet_math_ScreenPoint_sub(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_ScreenPoint_sub(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.Transform_affine
    #[no_mangle]
    pub extern "C" fn jet_math_Transform_affine(a0: f64, a1: f64, a2: f64, a3: f64, a4: f64, a5: f64, a6: i64, a7: i64) -> *mut crate::JetTransform2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Transform_affine(a0, a1, a2, a3, a4, a5, a6, a7))))
    }

    /// core.math.Transform_inverse
    #[no_mangle]
    pub extern "C" fn jet_math_Transform_inverse(a0: *mut crate::JetTransform2, ok: *mut *mut crate::JetTransform2, err: *mut *mut crate::JetErr) -> i64 {
        guard(|| match crate::jet_math_Transform_inverse(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.math.Transform_point
    #[no_mangle]
    pub extern "C" fn jet_math_Transform_point(a0: *mut crate::JetTransform2, a1: *mut crate::JetCoord2, ok: *mut *mut crate::JetCoord2, err: *mut *mut crate::JetErr) -> i64 {
        guard(|| match crate::jet_math_Transform_point(unsafe { &*a0 }, unsafe { &*a1 }.clone()) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.math.Transform_point_at_depth
    #[no_mangle]
    pub extern "C" fn jet_math_Transform_point_at_depth(a0: *mut crate::JetTransform2, a1: *mut crate::JetCoord2, a2: f64, ok: *mut *mut crate::JetCoord2, err: *mut *mut crate::JetErr) -> i64 {
        guard(|| match crate::jet_math_Transform_point_at_depth(unsafe { &*a0 }, unsafe { &*a1 }.clone(), a2) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.math.Transform_ray
    #[no_mangle]
    pub extern "C" fn jet_math_Transform_ray(a0: *mut crate::JetTransform2, a1: *mut crate::JetCoord2, ok: *mut *mut crate::JetRay2, err: *mut *mut crate::JetErr) -> i64 {
        guard(|| match crate::jet_math_Transform_ray(unsafe { &*a0 }, unsafe { &*a1 }.clone()) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.math.Transform_then
    #[no_mangle]
    pub extern "C" fn jet_math_Transform_then(a0: *mut crate::JetTransform2, a1: *mut crate::JetTransform2, ok: *mut *mut crate::JetTransform2, err: *mut *mut crate::JetErr) -> i64 {
        guard(|| match crate::jet_math_Transform_then(unsafe { &*a0 }, unsafe { &*a1 }.clone()) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.math.Transform2_affine
    #[no_mangle]
    pub extern "C" fn jet_math_Transform2_affine(a0: f64, a1: f64, a2: f64, a3: f64, a4: f64, a5: f64, a6: i64, a7: i64) -> *mut crate::JetTransform2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Transform2_affine(a0, a1, a2, a3, a4, a5, a6, a7))))
    }

    /// core.math.Transform2_inverse
    #[no_mangle]
    pub extern "C" fn jet_math_Transform2_inverse(a0: *mut crate::JetTransform2, ok: *mut *mut crate::JetTransform2, err: *mut *mut crate::JetErr) -> i64 {
        guard(|| match crate::jet_math_Transform2_inverse(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.math.Transform2_point
    #[no_mangle]
    pub extern "C" fn jet_math_Transform2_point(a0: *mut crate::JetTransform2, a1: *mut crate::JetCoord2, ok: *mut *mut crate::JetCoord2, err: *mut *mut crate::JetErr) -> i64 {
        guard(|| match crate::jet_math_Transform2_point(unsafe { &*a0 }, unsafe { &*a1 }.clone()) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.math.Transform2_point_at_depth
    #[no_mangle]
    pub extern "C" fn jet_math_Transform2_point_at_depth(a0: *mut crate::JetTransform2, a1: *mut crate::JetCoord2, a2: f64, ok: *mut *mut crate::JetCoord2, err: *mut *mut crate::JetErr) -> i64 {
        guard(|| match crate::jet_math_Transform2_point_at_depth(unsafe { &*a0 }, unsafe { &*a1 }.clone(), a2) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.math.Transform2_ray
    #[no_mangle]
    pub extern "C" fn jet_math_Transform2_ray(a0: *mut crate::JetTransform2, a1: *mut crate::JetCoord2, ok: *mut *mut crate::JetRay2, err: *mut *mut crate::JetErr) -> i64 {
        guard(|| match crate::jet_math_Transform2_ray(unsafe { &*a0 }, unsafe { &*a1 }.clone()) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.math.Transform2_then
    #[no_mangle]
    pub extern "C" fn jet_math_Transform2_then(a0: *mut crate::JetTransform2, a1: *mut crate::JetTransform2, ok: *mut *mut crate::JetTransform2, err: *mut *mut crate::JetErr) -> i64 {
        guard(|| match crate::jet_math_Transform2_then(unsafe { &*a0 }, unsafe { &*a1 }.clone()) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.math.Vec2_add
    #[no_mangle]
    pub extern "C" fn jet_math_Vec2_add(a0: *mut crate::jet_std::Vec2, a1: *mut crate::jet_std::Vec2) -> *mut crate::jet_std::Vec2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Vec2_add(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.Vec2_dot
    #[no_mangle]
    pub extern "C" fn jet_math_Vec2_dot(a0: *mut crate::jet_std::Vec2, a1: *mut crate::jet_std::Vec2) -> f64 {
        guard(|| crate::jet_math_Vec2_dot(unsafe { &*a0 }, unsafe { &*a1 }.clone()))
    }

    /// core.math.Vec2_lane
    #[no_mangle]
    pub extern "C" fn jet_math_Vec2_lane(a0: *mut crate::jet_std::Vec2, a1: i64, a2: JetCString, a3: i64) -> f64 {
        guard(|| crate::jet_math_Vec2_lane(unsafe { &*a0 }, a1, view(a2), fixed::<u32>(a3)))
    }

    /// core.math.Vec2_length
    #[no_mangle]
    pub extern "C" fn jet_math_Vec2_length(a0: *mut crate::jet_std::Vec2) -> f64 {
        guard(|| crate::jet_math_Vec2_length(unsafe { &*a0 }))
    }

    /// core.math.Vec2_mul
    #[no_mangle]
    pub extern "C" fn jet_math_Vec2_mul(a0: *mut crate::jet_std::Vec2, a1: *mut crate::jet_std::Vec2) -> *mut crate::jet_std::Vec2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Vec2_mul(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.Vec2_new
    #[no_mangle]
    pub extern "C" fn jet_math_Vec2_new(a0: f64, a1: f64) -> *mut crate::jet_std::Vec2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Vec2_new(a0, a1))))
    }

    /// core.math.Vec2_normalize
    #[no_mangle]
    pub extern "C" fn jet_math_Vec2_normalize(a0: *mut crate::jet_std::Vec2) -> *mut crate::jet_std::Vec2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Vec2_normalize(unsafe { &*a0 }))))
    }

    /// core.math.Vec2_splat
    #[no_mangle]
    pub extern "C" fn jet_math_Vec2_splat(a0: f64) -> *mut crate::jet_std::Vec2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Vec2_splat(a0))))
    }

    /// core.math.Vec2_sub
    #[no_mangle]
    pub extern "C" fn jet_math_Vec2_sub(a0: *mut crate::jet_std::Vec2, a1: *mut crate::jet_std::Vec2) -> *mut crate::jet_std::Vec2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Vec2_sub(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.Vec3_add
    #[no_mangle]
    pub extern "C" fn jet_math_Vec3_add(a0: *mut crate::jet_std::Vec3, a1: *mut crate::jet_std::Vec3) -> *mut crate::jet_std::Vec3 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Vec3_add(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.Vec3_cross
    #[no_mangle]
    pub extern "C" fn jet_math_Vec3_cross(a0: *mut crate::jet_std::Vec3, a1: *mut crate::jet_std::Vec3) -> *mut crate::jet_std::Vec3 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Vec3_cross(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.Vec3_div
    #[no_mangle]
    pub extern "C" fn jet_math_Vec3_div(a0: *mut crate::jet_std::Vec3, a1: f64) -> *mut crate::jet_std::Vec3 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Vec3_div(unsafe { &*a0 }, a1))))
    }

    /// core.math.Vec3_dot
    #[no_mangle]
    pub extern "C" fn jet_math_Vec3_dot(a0: *mut crate::jet_std::Vec3, a1: *mut crate::jet_std::Vec3) -> f64 {
        guard(|| crate::jet_math_Vec3_dot(unsafe { &*a0 }, unsafe { &*a1 }.clone()))
    }

    /// core.math.Vec3_hadamard_mul
    #[no_mangle]
    pub extern "C" fn jet_math_Vec3_hadamard_mul(a0: *mut crate::jet_std::Vec3, a1: *mut crate::jet_std::Vec3) -> *mut crate::jet_std::Vec3 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Vec3_hadamard_mul(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.Vec3_lane
    #[no_mangle]
    pub extern "C" fn jet_math_Vec3_lane(a0: *mut crate::jet_std::Vec3, a1: i64, a2: JetCString, a3: i64) -> f64 {
        guard(|| crate::jet_math_Vec3_lane(unsafe { &*a0 }, a1, view(a2), fixed::<u32>(a3)))
    }

    /// core.math.Vec3_length
    #[no_mangle]
    pub extern "C" fn jet_math_Vec3_length(a0: *mut crate::jet_std::Vec3) -> f64 {
        guard(|| crate::jet_math_Vec3_length(unsafe { &*a0 }))
    }

    /// core.math.Vec3_mul
    #[no_mangle]
    pub extern "C" fn jet_math_Vec3_mul(a0: *mut crate::jet_std::Vec3, a1: f64) -> *mut crate::jet_std::Vec3 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Vec3_mul(unsafe { &*a0 }, a1))))
    }

    /// core.math.Vec3_new
    #[no_mangle]
    pub extern "C" fn jet_math_Vec3_new(a0: f64, a1: f64, a2: f64) -> *mut crate::jet_std::Vec3 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Vec3_new(a0, a1, a2))))
    }

    /// core.math.Vec3_normalize
    #[no_mangle]
    pub extern "C" fn jet_math_Vec3_normalize(a0: *mut crate::jet_std::Vec3) -> *mut crate::jet_std::Vec3 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Vec3_normalize(unsafe { &*a0 }))))
    }

    /// core.math.Vec3_splat
    #[no_mangle]
    pub extern "C" fn jet_math_Vec3_splat(a0: f64) -> *mut crate::jet_std::Vec3 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Vec3_splat(a0))))
    }

    /// core.math.Vec3_sub
    #[no_mangle]
    pub extern "C" fn jet_math_Vec3_sub(a0: *mut crate::jet_std::Vec3, a1: *mut crate::jet_std::Vec3) -> *mut crate::jet_std::Vec3 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Vec3_sub(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.Vec4_add
    #[no_mangle]
    pub extern "C" fn jet_math_Vec4_add(a0: *mut crate::jet_std::Vec4, a1: *mut crate::jet_std::Vec4) -> *mut crate::jet_std::Vec4 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Vec4_add(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.Vec4_dot
    #[no_mangle]
    pub extern "C" fn jet_math_Vec4_dot(a0: *mut crate::jet_std::Vec4, a1: *mut crate::jet_std::Vec4) -> f64 {
        guard(|| crate::jet_math_Vec4_dot(unsafe { &*a0 }, unsafe { &*a1 }.clone()))
    }

    /// core.math.Vec4_lane
    #[no_mangle]
    pub extern "C" fn jet_math_Vec4_lane(a0: *mut crate::jet_std::Vec4, a1: i64, a2: JetCString, a3: i64) -> f64 {
        guard(|| crate::jet_math_Vec4_lane(unsafe { &*a0 }, a1, view(a2), fixed::<u32>(a3)))
    }

    /// core.math.Vec4_length
    #[no_mangle]
    pub extern "C" fn jet_math_Vec4_length(a0: *mut crate::jet_std::Vec4) -> f64 {
        guard(|| crate::jet_math_Vec4_length(unsafe { &*a0 }))
    }

    /// core.math.Vec4_mul
    #[no_mangle]
    pub extern "C" fn jet_math_Vec4_mul(a0: *mut crate::jet_std::Vec4, a1: *mut crate::jet_std::Vec4) -> *mut crate::jet_std::Vec4 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Vec4_mul(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.Vec4_new
    #[no_mangle]
    pub extern "C" fn jet_math_Vec4_new(a0: f64, a1: f64, a2: f64, a3: f64) -> *mut crate::jet_std::Vec4 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Vec4_new(a0, a1, a2, a3))))
    }

    /// core.math.Vec4_normalize
    #[no_mangle]
    pub extern "C" fn jet_math_Vec4_normalize(a0: *mut crate::jet_std::Vec4) -> *mut crate::jet_std::Vec4 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Vec4_normalize(unsafe { &*a0 }))))
    }

    /// core.math.Vec4_splat
    #[no_mangle]
    pub extern "C" fn jet_math_Vec4_splat(a0: f64) -> *mut crate::jet_std::Vec4 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Vec4_splat(a0))))
    }

    /// core.math.Vec4_sub
    #[no_mangle]
    pub extern "C" fn jet_math_Vec4_sub(a0: *mut crate::jet_std::Vec4, a1: *mut crate::jet_std::Vec4) -> *mut crate::jet_std::Vec4 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_Vec4_sub(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.ViewDelta_add
    #[no_mangle]
    pub extern "C" fn jet_math_ViewDelta_add(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_ViewDelta_add(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.ViewDelta_new
    #[no_mangle]
    pub extern "C" fn jet_math_ViewDelta_new(a0: f64, a1: f64) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_ViewDelta_new(a0, a1))))
    }

    /// core.math.ViewDelta_sub
    #[no_mangle]
    pub extern "C" fn jet_math_ViewDelta_sub(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_ViewDelta_sub(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.ViewPoint_add
    #[no_mangle]
    pub extern "C" fn jet_math_ViewPoint_add(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_ViewPoint_add(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.ViewPoint_new
    #[no_mangle]
    pub extern "C" fn jet_math_ViewPoint_new(a0: f64, a1: f64) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_ViewPoint_new(a0, a1))))
    }

    /// core.math.ViewPoint_sub
    #[no_mangle]
    pub extern "C" fn jet_math_ViewPoint_sub(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_ViewPoint_sub(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.WorldDelta_add
    #[no_mangle]
    pub extern "C" fn jet_math_WorldDelta_add(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_WorldDelta_add(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.WorldDelta_new
    #[no_mangle]
    pub extern "C" fn jet_math_WorldDelta_new(a0: f64, a1: f64) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_WorldDelta_new(a0, a1))))
    }

    /// core.math.WorldDelta_sub
    #[no_mangle]
    pub extern "C" fn jet_math_WorldDelta_sub(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_WorldDelta_sub(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.WorldPoint_add
    #[no_mangle]
    pub extern "C" fn jet_math_WorldPoint_add(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_WorldPoint_add(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
    }

    /// core.math.WorldPoint_new
    #[no_mangle]
    pub extern "C" fn jet_math_WorldPoint_new(a0: f64, a1: f64) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_WorldPoint_new(a0, a1))))
    }

    /// core.math.WorldPoint_sub
    #[no_mangle]
    pub extern "C" fn jet_math_WorldPoint_sub(a0: *mut crate::JetCoord2, a1: *mut crate::JetCoord2) -> *mut crate::JetCoord2 {
        guard(|| Box::into_raw(Box::new(crate::jet_math_WorldPoint_sub(unsafe { &*a0 }, unsafe { &*a1 }.clone()))))
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

    /// core.mod.load
    #[no_mangle]
    pub extern "C" fn jet_mod_load(a0: JetCString, a1: *mut crate::JetModGrant, ok: *mut *mut crate::JetMod, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_mod_load(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.net.dns_ptr
    #[no_mangle]
    pub extern "C" fn jet_net_dns_ptr(a0: JetCString, a1: i64, ok: *mut *mut u64, ok_len: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_net_dns_ptr(unsafe { &*a0 }, a1) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| handle(e) as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
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

    /// core.net.dns_txt
    #[no_mangle]
    pub extern "C" fn jet_net_dns_txt(a0: JetCString, a1: i64, ok: *mut *mut u64, ok_len: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_net_dns_txt(unsafe { &*a0 }, a1) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| handle(e) as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.net.dns_txt_at
    #[no_mangle]
    pub extern "C" fn jet_net_dns_txt_at(a0: JetCString, a1: JetCString, a2: i64, ok: *mut *mut u64, ok_len: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_net_dns_txt_at(unsafe { &*a0 }, unsafe { &*a1 }, a2) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| handle(e) as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
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

    /// core.net.set_read_timeout
    #[no_mangle]
    pub extern "C" fn jet_net_set_read_timeout(a0: *mut crate::JetTCPStream, a1: i64, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_set_read_timeout(unsafe { &mut *a0 }, a1) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.set_timeout
    #[no_mangle]
    pub extern "C" fn jet_net_set_timeout(a0: *mut crate::JetTCPStream, a1: i64, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_set_timeout(unsafe { &mut *a0 }, a1) {
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

    /// core.net.set_write_timeout
    #[no_mangle]
    pub extern "C" fn jet_net_set_write_timeout(a0: *mut crate::JetTCPStream, a1: i64, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_set_write_timeout(unsafe { &mut *a0 }, a1) {
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

    /// core.handle.tcp_stream.close
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_close(a0: *mut crate::JetTCPStream, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_close(unsafe { &mut *a0 }) {
            Ok(()) => 1,
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

    /// core.handle.tcp_stream.read
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_read(a0: *mut crate::JetTCPStream, ok: *mut JetCString, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_read(unsafe { &mut *a0 }) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tcp_stream.read_bytes
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_read_bytes(a0: *mut crate::JetTCPStream, a1: i64, ok: *mut *mut u64, ok_len: *mut i64, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_read_bytes(unsafe { &mut *a0 }, a1) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| e as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tcp_stream.read_bytes_deadline
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_read_bytes_deadline(a0: *mut crate::JetTCPStream, a1: i64, a2: *mut crate::jet_std::Duration, ok: *mut *mut u64, ok_len: *mut i64, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_read_bytes_deadline(unsafe { &mut *a0 }, a1, unsafe { &*a2 }) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| e as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tcp_stream.read_text
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_read_text(a0: *mut crate::JetTCPStream, a1: i64, ok: *mut JetCString, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_read_text(unsafe { &mut *a0 }, a1) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tcp_stream.read_text_deadline
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_read_text_deadline(a0: *mut crate::JetTCPStream, a1: i64, a2: *mut crate::jet_std::Duration, ok: *mut JetCString, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_read_text_deadline(unsafe { &mut *a0 }, a1, unsafe { &*a2 }) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tcp_stream.ready
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_ready_deadline(a0: *mut crate::JetTCPStream, a1: i64, a2: *mut crate::jet_std::Duration, ok: *mut *mut crate::JetNetReady, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_ready_deadline(unsafe { &mut *a0 }, match a1 { 0 => crate::JetNetReadyInterest::Read, 1 => crate::JetNetReadyInterest::Write, 2 => crate::JetNetReadyInterest::ReadWrite, _ => range_stop() }, unsafe { &*a2 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tcp_stream.shutdown
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_shutdown(a0: *mut crate::JetTCPStream, a1: i64, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_shutdown(unsafe { &mut *a0 }, match a1 { 0 => crate::JetNetShutdown::Read, 1 => crate::JetNetShutdown::Write, 2 => crate::JetNetShutdown::Both, _ => range_stop() }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tcp_stream.write
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_write(a0: *mut crate::JetTCPStream, a1: JetCString, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_write(unsafe { &mut *a0 }, unsafe { &*a1 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tcp_stream.write_all_bytes
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_write_all_bytes(a0: *mut crate::JetTCPStream, a1_0: *const u64, a1_1: i64, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_write_all_bytes(unsafe { &mut *a0 }, &list_in(a1_0, a1_1, |w| w as u8)) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tcp_stream.write_all_bytes_deadline
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_write_all_bytes_deadline(a0: *mut crate::JetTCPStream, a1_0: *const u64, a1_1: i64, a2: *mut crate::jet_std::Duration, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_write_all_bytes_deadline(unsafe { &mut *a0 }, &list_in(a1_0, a1_1, |w| w as u8), unsafe { &*a2 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tcp_stream.write_bytes
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_write_bytes(a0: *mut crate::JetTCPStream, a1_0: *const u64, a1_1: i64, ok: *mut i64, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_write_bytes(unsafe { &mut *a0 }, &list_in(a1_0, a1_1, |w| w as u8)) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tcp_stream.write_bytes_deadline
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_write_bytes_deadline(a0: *mut crate::JetTCPStream, a1_0: *const u64, a1_1: i64, a2: *mut crate::jet_std::Duration, ok: *mut i64, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_write_bytes_deadline(unsafe { &mut *a0 }, &list_in(a1_0, a1_1, |w| w as u8), unsafe { &*a2 }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tcp_stream.write_text
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_write_text(a0: *mut crate::JetTCPStream, a1: JetCString, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_write_text(unsafe { &mut *a0 }, unsafe { &*a1 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tcp_stream.write_text_deadline
    #[no_mangle]
    pub extern "C" fn jet_net_tcp_write_text_deadline(a0: *mut crate::JetTCPStream, a1: JetCString, a2: *mut crate::jet_std::Duration, err: *mut *mut crate::JetNetError) -> i64 {
        guard(|| match crate::jet_net_tcp_write_text_deadline(unsafe { &mut *a0 }, unsafe { &*a1 }, unsafe { &*a2 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tls_stream.close
    #[no_mangle]
    pub extern "C" fn jet_net_tls_close(a0: *mut crate::JetTLSStream, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_net_tls_close(unsafe { &mut *a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tls_stream.close_write
    #[no_mangle]
    pub extern "C" fn jet_net_tls_close_write(a0: *mut crate::JetTLSStream, a1: *mut crate::jet_std::Duration, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_net_tls_close_write(unsafe { &mut *a0 }, unsafe { &*a1 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tls_stream.peer_identity
    #[no_mangle]
    pub extern "C" fn jet_net_tls_peer_identity(a0: *mut crate::JetTLSStream) -> *mut crate::JetTLSPeerIdentity {
        guard(|| Box::into_raw(Box::new(crate::jet_net_tls_peer_identity(unsafe { &*a0 }))))
    }

    /// core.net.tls.read
    #[no_mangle]
    pub extern "C" fn jet_net_tls_read_bytes(a0: *mut crate::JetTLSStream, a1: i64, ok: *mut *mut u64, ok_len: *mut i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_net_tls_read_bytes(unsafe { &mut *a0 }, a1) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| e as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tls_stream.read_deadline
    #[no_mangle]
    pub extern "C" fn jet_net_tls_read_bytes_deadline(a0: *mut crate::JetTLSStream, a1: i64, a2: *mut crate::jet_std::Duration, ok: *mut *mut u64, ok_len: *mut i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_net_tls_read_bytes_deadline(unsafe { &mut *a0 }, a1, unsafe { &*a2 }) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| e as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tls_stream.ready
    #[no_mangle]
    pub extern "C" fn jet_net_tls_ready(a0: *mut crate::JetTLSStream, a1: i64, a2: *mut crate::jet_std::Duration, ok: *mut *mut crate::JetNetReady, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_net_tls_ready(unsafe { &*a0 }, match a1 { 0 => crate::JetNetReadyInterest::Read, 1 => crate::JetNetReadyInterest::Write, 2 => crate::JetNetReadyInterest::ReadWrite, _ => range_stop() }, unsafe { &*a2 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.tls.write_all
    #[no_mangle]
    pub extern "C" fn jet_net_tls_write_all_bytes(a0: *mut crate::JetTLSStream, a1_0: *const u64, a1_1: i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_net_tls_write_all_bytes(unsafe { &mut *a0 }, &list_in(a1_0, a1_1, |w| w as u8)) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tls_stream.write_all_deadline
    #[no_mangle]
    pub extern "C" fn jet_net_tls_write_all_bytes_deadline(a0: *mut crate::JetTLSStream, a1_0: *const u64, a1_1: i64, a2: *mut crate::jet_std::Duration, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_net_tls_write_all_bytes_deadline(unsafe { &mut *a0 }, &list_in(a1_0, a1_1, |w| w as u8), unsafe { &*a2 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.tls.write
    #[no_mangle]
    pub extern "C" fn jet_net_tls_write_bytes(a0: *mut crate::JetTLSStream, a1_0: *const u64, a1_1: i64, ok: *mut i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_net_tls_write_bytes(unsafe { &mut *a0 }, &list_in(a1_0, a1_1, |w| w as u8)) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.tls.write_text
    #[no_mangle]
    pub extern "C" fn jet_net_tls_write_text(a0: *mut crate::JetTLSStream, a1: JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_net_tls_write_text(unsafe { &mut *a0 }, unsafe { &*a1 }) {
            Ok(()) => 1,
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

    /// core.index.index_miss
    #[no_mangle]
    pub extern "C" fn jet_panic(a0: JetCString, a1: i64, a2: JetCString) {
        guard(|| crate::jet_panic(view(a0), fixed::<u32>(a1), view(a2)))
    }

    /// core.handle.path.from
    #[no_mangle]
    pub extern "C" fn jet_path_from(a0: JetCString) -> *mut crate::JetPath {
        guard(|| Box::into_raw(Box::new(crate::jet_path_from(unsafe { &*a0 }))))
    }

    /// core.handle.path.home
    #[no_mangle]
    pub extern "C" fn jet_path_home() -> *mut crate::JetPath {
        guard(|| Box::into_raw(Box::new(crate::jet_path_home())))
    }

    /// core.handle.path.is_within
    #[no_mangle]
    pub extern "C" fn jet_path_is_within(a0: *mut crate::JetPath, a1: *mut crate::JetPath) -> bool {
        guard(|| crate::jet_path_is_within(unsafe { &*a0 }, unsafe { &*a1 }))
    }

    /// core.handle.path.join
    #[no_mangle]
    pub extern "C" fn jet_path_join(a0: *mut crate::JetPath, a1: JetCString) -> *mut crate::JetPath {
        guard(|| Box::into_raw(Box::new(crate::jet_path_join(unsafe { &*a0 }, unsafe { &*a1 }))))
    }

    /// core.handle.path.normalize
    #[no_mangle]
    pub extern "C" fn jet_path_normalize(a0: *mut crate::JetPath) -> *mut crate::JetPath {
        guard(|| Box::into_raw(Box::new(crate::jet_path_normalize(unsafe { &*a0 }))))
    }

    /// core.handle.path.to_string
    #[no_mangle]
    pub extern "C" fn jet_path_to_string(a0: *mut crate::JetPath) -> JetCString {
        guard(|| handle(crate::jet_path_to_string(unsafe { &*a0 })))
    }

    /// core.handle.path.write_atomic
    #[no_mangle]
    pub extern "C" fn jet_path_write_atomic(a0: *mut crate::JetPath, a1_0: *const u64, a1_1: i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_path_write_atomic(unsafe { &*a0 }, &list_in(a1_0, a1_1, |w| w as u8)) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.perf.default_fidelity
    #[no_mangle]
    pub extern "C" fn jet_perf_default_fidelity() -> f64 {
        guard(|| crate::jet_perf_default_fidelity())
    }

    /// core.perf.fidelity
    #[no_mangle]
    pub extern "C" fn jet_perf_fidelity() -> f64 {
        guard(|| crate::jet_perf_fidelity())
    }

    /// core.perf.override_fidelity
    #[no_mangle]
    pub extern "C" fn jet_perf_override_fidelity(a0: f64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_perf_override_fidelity(a0) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.perf.reset_fidelity
    #[no_mangle]
    pub extern "C" fn jet_perf_reset_fidelity() {
        guard(|| crate::jet_perf_reset_fidelity())
    }

    /// core.handle.process.child.exited
    #[no_mangle]
    pub extern "C" fn jet_process_child_exited(a0: *mut crate::jet_std::ProcessChild, ok: *mut bool, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_process_child_exited(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(value) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.process.child.id
    #[no_mangle]
    pub extern "C" fn jet_process_child_id(a0: *mut crate::jet_std::ProcessChild) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_process_child_id(unsafe { &*a0 })))
    }

    /// core.handle.process.child.interrupt
    #[no_mangle]
    pub extern "C" fn jet_process_child_interrupt(a0: *mut crate::jet_std::ProcessChild, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_process_child_interrupt(unsafe { &*a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.process.child.kill
    #[no_mangle]
    pub extern "C" fn jet_process_child_kill(a0: *mut crate::jet_std::ProcessChild, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_process_child_kill(unsafe { &*a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.process.child.terminate
    #[no_mangle]
    pub extern "C" fn jet_process_child_terminate(a0: *mut crate::jet_std::ProcessChild, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_process_child_terminate(unsafe { &*a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.process.child.wait
    #[no_mangle]
    pub extern "C" fn jet_process_child_wait(a0: *mut crate::jet_std::ProcessChild, ok: *mut *mut crate::jet_std::ProcessReceipt, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_process_child_wait(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.process.on_signal
    #[no_mangle]
    pub extern "C" fn jet_process_on_signal(a0: i64) {
        guard(|| crate::jet_process_on_signal(&match a0 { 0 => crate::jet_std::ProcessSignal::Term, 1 => crate::jet_std::ProcessSignal::Hup, 2 => crate::jet_std::ProcessSignal::Int, _ => range_stop() }))
    }

    /// core.handle.process.spec.arg
    #[no_mangle]
    pub extern "C" fn jet_process_spec_arg(a0: *mut crate::jet_std::ProcessSpec, a1: JetCString) -> *mut crate::jet_std::ProcessSpec {
        guard(|| Box::into_raw(Box::new(crate::jet_process_spec_arg(unsafe { &*a0 }.clone(), unsafe { &*a1 }))))
    }

    /// core.handle.process.spec.args_extend
    #[no_mangle]
    pub extern "C" fn jet_process_spec_args_extend(a0: *mut crate::jet_std::ProcessSpec, a1_0: *const u64, a1_1: i64) -> *mut crate::jet_std::ProcessSpec {
        guard(|| Box::into_raw(Box::new(crate::jet_process_spec_args_extend(unsafe { &*a0 }.clone(), &list_in(a1_0, a1_1, |w| view(w as JetCString).to_owned())))))
    }

    /// core.handle.process.spec.cpu_time_limit
    #[no_mangle]
    pub extern "C" fn jet_process_spec_cpu_time_limit(a0: *mut crate::jet_std::ProcessSpec, a1: *mut crate::jet_std::Duration) -> *mut crate::jet_std::ProcessSpec {
        guard(|| Box::into_raw(Box::new(crate::jet_process_spec_cpu_time_limit(unsafe { &*a0 }.clone(), unsafe { &*a1 }))))
    }

    /// core.handle.process.spec.cwd
    #[no_mangle]
    pub extern "C" fn jet_process_spec_cwd(a0: *mut crate::jet_std::ProcessSpec, a1: JetCString) -> *mut crate::jet_std::ProcessSpec {
        guard(|| Box::into_raw(Box::new(crate::jet_process_spec_cwd(unsafe { &*a0 }.clone(), unsafe { &*a1 }))))
    }

    /// core.handle.process.spec.detached
    #[no_mangle]
    pub extern "C" fn jet_process_spec_detached(a0: *mut crate::jet_std::ProcessSpec) -> *mut crate::jet_std::ProcessSpec {
        guard(|| Box::into_raw(Box::new(crate::jet_process_spec_detached(unsafe { &*a0 }.clone()))))
    }

    /// core.handle.process.spec.env
    #[no_mangle]
    pub extern "C" fn jet_process_spec_env(a0: *mut crate::jet_std::ProcessSpec, a1: JetCString, a2: JetCString) -> *mut crate::jet_std::ProcessSpec {
        guard(|| Box::into_raw(Box::new(crate::jet_process_spec_env(unsafe { &*a0 }.clone(), unsafe { &*a1 }, unsafe { &*a2 }))))
    }

    /// core.handle.process.spec.env_clear
    #[no_mangle]
    pub extern "C" fn jet_process_spec_env_clear(a0: *mut crate::jet_std::ProcessSpec) -> *mut crate::jet_std::ProcessSpec {
        guard(|| Box::into_raw(Box::new(crate::jet_process_spec_env_clear(unsafe { &*a0 }.clone()))))
    }

    /// core.handle.process.spec.env_remove
    #[no_mangle]
    pub extern "C" fn jet_process_spec_env_remove(a0: *mut crate::jet_std::ProcessSpec, a1: JetCString) -> *mut crate::jet_std::ProcessSpec {
        guard(|| Box::into_raw(Box::new(crate::jet_process_spec_env_remove(unsafe { &*a0 }.clone(), unsafe { &*a1 }))))
    }

    /// core.handle.process.spec.memory_limit
    #[no_mangle]
    pub extern "C" fn jet_process_spec_memory_limit(a0: *mut crate::jet_std::ProcessSpec, a1: i64) -> *mut crate::jet_std::ProcessSpec {
        guard(|| Box::into_raw(Box::new(crate::jet_process_spec_memory_limit(unsafe { &*a0 }.clone(), unsafe { crate::jet_foundation::Numeric::JetInt::clone_from_raw(a1) }))))
    }

    /// core.handle.process.spec.open_file_limit
    #[no_mangle]
    pub extern "C" fn jet_process_spec_open_file_limit(a0: *mut crate::jet_std::ProcessSpec, a1: i64) -> *mut crate::jet_std::ProcessSpec {
        guard(|| Box::into_raw(Box::new(crate::jet_process_spec_open_file_limit(unsafe { &*a0 }.clone(), unsafe { crate::jet_foundation::Numeric::JetInt::clone_from_raw(a1) }))))
    }

    /// core.handle.process.spec.output_limit
    #[no_mangle]
    pub extern "C" fn jet_process_spec_output_limit(a0: *mut crate::jet_std::ProcessSpec, a1: i64) -> *mut crate::jet_std::ProcessSpec {
        guard(|| Box::into_raw(Box::new(crate::jet_process_spec_output_limit(unsafe { &*a0 }.clone(), unsafe { crate::jet_foundation::Numeric::JetInt::clone_from_raw(a1) }))))
    }

    /// core.handle.process.spec.plan
    #[no_mangle]
    pub extern "C" fn jet_process_spec_plan(a0: *mut crate::jet_std::ProcessSpec, ok: *mut *mut crate::jet_std::ProcessPlan, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_process_spec_plan(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.process.spec.run
    #[no_mangle]
    pub extern "C" fn jet_process_spec_run(a0: *mut crate::jet_std::ProcessSpec, ok: *mut *mut crate::jet_std::ProcessReceipt, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_process_spec_run(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.process.spec.run_checked
    #[no_mangle]
    pub extern "C" fn jet_process_spec_run_checked(a0: *mut crate::jet_std::ProcessSpec, ok: *mut *mut crate::jet_std::ProcessReceipt, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_process_spec_run_checked(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.process.spec.spawn
    #[no_mangle]
    pub extern "C" fn jet_process_spec_spawn(a0: *mut crate::jet_std::ProcessSpec, ok: *mut *mut crate::jet_std::ProcessChild, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_process_spec_spawn(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.process.spec.stderr
    #[no_mangle]
    pub extern "C" fn jet_process_spec_stderr(a0: *mut crate::jet_std::ProcessSpec, a1: i64) -> *mut crate::jet_std::ProcessSpec {
        guard(|| Box::into_raw(Box::new(crate::jet_process_spec_stderr(unsafe { &*a0 }.clone(), &match a1 { 0 => crate::jet_std::ProcessStreamMode::Stream, 1 => crate::jet_std::ProcessStreamMode::Inherit, 2 => crate::jet_std::ProcessStreamMode::Capture, _ => range_stop() }))))
    }

    /// core.handle.process.spec.stdin
    #[no_mangle]
    pub extern "C" fn jet_process_spec_stdin(a0: *mut crate::jet_std::ProcessSpec, a1: i64) -> *mut crate::jet_std::ProcessSpec {
        guard(|| Box::into_raw(Box::new(crate::jet_process_spec_stdin(unsafe { &*a0 }.clone(), &match a1 { 0 => crate::jet_std::ProcessStreamMode::Stream, 1 => crate::jet_std::ProcessStreamMode::Inherit, 2 => crate::jet_std::ProcessStreamMode::Capture, _ => range_stop() }))))
    }

    /// core.handle.process.spec.stdout
    #[no_mangle]
    pub extern "C" fn jet_process_spec_stdout(a0: *mut crate::jet_std::ProcessSpec, a1: i64) -> *mut crate::jet_std::ProcessSpec {
        guard(|| Box::into_raw(Box::new(crate::jet_process_spec_stdout(unsafe { &*a0 }.clone(), &match a1 { 0 => crate::jet_std::ProcessStreamMode::Stream, 1 => crate::jet_std::ProcessStreamMode::Inherit, 2 => crate::jet_std::ProcessStreamMode::Capture, _ => range_stop() }))))
    }

    /// core.handle.process.spec.terminal
    #[no_mangle]
    pub extern "C" fn jet_process_spec_terminal(a0: *mut crate::jet_std::ProcessSpec) -> *mut crate::jet_std::ProcessSpec {
        guard(|| Box::into_raw(Box::new(crate::jet_process_spec_terminal(unsafe { &*a0 }.clone()))))
    }

    /// core.handle.process.spec.terminal_with_policy
    #[no_mangle]
    pub extern "C" fn jet_process_spec_terminal_with_policy(a0: *mut crate::jet_std::ProcessSpec, a1: *mut crate::jet_std::TerminalPolicy) -> *mut crate::jet_std::ProcessSpec {
        guard(|| Box::into_raw(Box::new(crate::jet_process_spec_terminal_with_policy(unsafe { &*a0 }.clone(), unsafe { &*a1 }))))
    }

    /// core.handle.process.spec.timeout
    #[no_mangle]
    pub extern "C" fn jet_process_spec_timeout(a0: *mut crate::jet_std::ProcessSpec, a1: *mut crate::jet_std::Duration) -> *mut crate::jet_std::ProcessSpec {
        guard(|| Box::into_raw(Box::new(crate::jet_process_spec_timeout(unsafe { &*a0 }.clone(), unsafe { &*a1 }))))
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

    /// core.handle.reader.read_f64_be
    #[no_mangle]
    pub extern "C" fn jet_reader_read_f64_be(a0: *mut crate::JetReader, ok: *mut f64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_reader_read_f64_be(unsafe { &mut *a0 }) {
            Ok(value) => { unsafe { ok.write(value) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.handle.reader.read_f64_le
    #[no_mangle]
    pub extern "C" fn jet_reader_read_f64_le(a0: *mut crate::JetReader, ok: *mut f64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_reader_read_f64_le(unsafe { &mut *a0 }) {
            Ok(value) => { unsafe { ok.write(value) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.handle.reader.read_i16_be
    #[no_mangle]
    pub extern "C" fn jet_reader_read_i16_be(a0: *mut crate::JetReader, ok: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_reader_read_i16_be(unsafe { &mut *a0 }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(i64::try_from(value).unwrap_or_else(|_| range_stop()))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.handle.reader.read_i16_le
    #[no_mangle]
    pub extern "C" fn jet_reader_read_i16_le(a0: *mut crate::JetReader, ok: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_reader_read_i16_le(unsafe { &mut *a0 }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(i64::try_from(value).unwrap_or_else(|_| range_stop()))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.handle.reader.read_i32_be
    #[no_mangle]
    pub extern "C" fn jet_reader_read_i32_be(a0: *mut crate::JetReader, ok: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_reader_read_i32_be(unsafe { &mut *a0 }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(i64::try_from(value).unwrap_or_else(|_| range_stop()))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.handle.reader.read_i32_le
    #[no_mangle]
    pub extern "C" fn jet_reader_read_i32_le(a0: *mut crate::JetReader, ok: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_reader_read_i32_le(unsafe { &mut *a0 }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(i64::try_from(value).unwrap_or_else(|_| range_stop()))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.handle.reader.read_i64_be
    #[no_mangle]
    pub extern "C" fn jet_reader_read_i64_be(a0: *mut crate::JetReader, ok: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_reader_read_i64_be(unsafe { &mut *a0 }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.handle.reader.read_i64_le
    #[no_mangle]
    pub extern "C" fn jet_reader_read_i64_le(a0: *mut crate::JetReader, ok: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_reader_read_i64_le(unsafe { &mut *a0 }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.handle.reader.read_i8
    #[no_mangle]
    pub extern "C" fn jet_reader_read_i8(a0: *mut crate::JetReader, ok: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_reader_read_i8(unsafe { &mut *a0 }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(i64::try_from(value).unwrap_or_else(|_| range_stop()))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.handle.reader.read_u16_be
    #[no_mangle]
    pub extern "C" fn jet_reader_read_u16_be(a0: *mut crate::JetReader, ok: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_reader_read_u16_be(unsafe { &mut *a0 }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(i64::try_from(value).unwrap_or_else(|_| range_stop()))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.handle.reader.read_u16_le
    #[no_mangle]
    pub extern "C" fn jet_reader_read_u16_le(a0: *mut crate::JetReader, ok: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_reader_read_u16_le(unsafe { &mut *a0 }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(i64::try_from(value).unwrap_or_else(|_| range_stop()))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.handle.reader.read_u32_be
    #[no_mangle]
    pub extern "C" fn jet_reader_read_u32_be(a0: *mut crate::JetReader, ok: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_reader_read_u32_be(unsafe { &mut *a0 }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(i64::try_from(value).unwrap_or_else(|_| range_stop()))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.handle.reader.read_u32_le
    #[no_mangle]
    pub extern "C" fn jet_reader_read_u32_le(a0: *mut crate::JetReader, ok: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_reader_read_u32_le(unsafe { &mut *a0 }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(i64::try_from(value).unwrap_or_else(|_| range_stop()))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.handle.reader.read_u64_be
    #[no_mangle]
    pub extern "C" fn jet_reader_read_u64_be(a0: *mut crate::JetReader, ok: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_reader_read_u64_be(unsafe { &mut *a0 }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(i64::try_from(value).unwrap_or_else(|_| range_stop()))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.handle.reader.read_u64_le
    #[no_mangle]
    pub extern "C" fn jet_reader_read_u64_le(a0: *mut crate::JetReader, ok: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_reader_read_u64_le(unsafe { &mut *a0 }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(i64::try_from(value).unwrap_or_else(|_| range_stop()))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.handle.reader.read_u8
    #[no_mangle]
    pub extern "C" fn jet_reader_read_u8(a0: *mut crate::JetReader, ok: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_reader_read_u8(unsafe { &mut *a0 }) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(i64::try_from(value).unwrap_or_else(|_| range_stop()))) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.handle.reader.remaining
    #[no_mangle]
    pub extern "C" fn jet_reader_remaining(a0: *mut crate::JetReader) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_reader_remaining(unsafe { &*a0 })))
    }

    /// core.handle.reader.seek
    #[no_mangle]
    pub extern "C" fn jet_reader_seek(a0: *mut crate::JetReader, a1: i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_reader_seek(unsafe { &mut *a0 }, a1) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.handle.reader.skip
    #[no_mangle]
    pub extern "C" fn jet_reader_skip(a0: *mut crate::JetReader, a1: i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_reader_skip(unsafe { &mut *a0 }, a1) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.handle.reader.take
    #[no_mangle]
    pub extern "C" fn jet_reader_take(a0: *mut crate::JetReader, a1: i64, ok: *mut *mut u64, ok_len: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_reader_take(unsafe { &mut *a0 }, a1) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| e as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
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

    /// core.handle.rng.bool
    #[no_mangle]
    pub extern "C" fn jet_rng_bool(a0: *mut crate::jet_std::Rng) -> bool {
        guard(|| crate::jet_rng_bool(unsafe { &mut *a0 }))
    }

    /// core.handle.rng.bool_p
    #[no_mangle]
    pub extern "C" fn jet_rng_bool_p(a0: *mut crate::jet_std::Rng, a1: f64) -> bool {
        guard(|| crate::jet_rng_bool_p(unsafe { &mut *a0 }, a1))
    }

    /// core.handle.rng.bytes
    #[no_mangle]
    pub extern "C" fn jet_rng_bytes(a0: *mut crate::jet_std::Rng, a1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_rng_bytes(unsafe { &mut *a0 }, a1).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.handle.rng.exponential
    #[no_mangle]
    pub extern "C" fn jet_rng_exponential(a0: *mut crate::jet_std::Rng, a1: f64) -> f64 {
        guard(|| crate::jet_rng_exponential(unsafe { &mut *a0 }, a1))
    }

    /// core.handle.rng.float
    #[no_mangle]
    pub extern "C" fn jet_rng_float(a0: *mut crate::jet_std::Rng) -> f64 {
        guard(|| crate::jet_rng_float(unsafe { &mut *a0 }))
    }

    /// core.handle.rng.float_range
    #[no_mangle]
    pub extern "C" fn jet_rng_float_range(a0: *mut crate::jet_std::Rng, a1: f64, a2: f64) -> f64 {
        guard(|| crate::jet_rng_float_range(unsafe { &mut *a0 }, a1, a2))
    }

    /// core.handle.rng.int
    #[no_mangle]
    pub extern "C" fn jet_rng_int(a0: *mut crate::jet_std::Rng, a1: i64, a2: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_rng_int(unsafe { &mut *a0 }, a1, a2)))
    }

    /// core.handle.rng.normal
    #[no_mangle]
    pub extern "C" fn jet_rng_normal(a0: *mut crate::jet_std::Rng, a1: f64, a2: f64) -> f64 {
        guard(|| crate::jet_rng_normal(unsafe { &mut *a0 }, a1, a2))
    }

    /// core.handle.rng.split
    #[no_mangle]
    pub extern "C" fn jet_rng_split(a0: *mut crate::jet_std::Rng) -> *mut crate::jet_std::Rng {
        guard(|| Box::into_raw(Box::new(crate::jet_rng_split(unsafe { &mut *a0 }))))
    }

    /// core.handle.realtime.cancel
    #[no_mangle]
    pub extern "C" fn jet_rt_cancel(a0: *mut crate::JetRealtimeStream) {
        guard(|| crate::jet_rt_cancel(unsafe { &*a0 }.clone()))
    }

    /// core.handle.realtime.is_cancelled
    #[no_mangle]
    pub extern "C" fn jet_rt_is_cancelled(a0: *mut crate::JetRealtimeStream) -> bool {
        guard(|| crate::jet_rt_is_cancelled(unsafe { &*a0 }))
    }

    /// core.handle.realtime.next_deadline
    #[no_mangle]
    pub extern "C" fn jet_rt_next_deadline(a0: *mut crate::JetRealtimeStream) -> *mut crate::JetInstant {
        guard(|| Box::into_raw(Box::new(crate::jet_rt_next_deadline(unsafe { &*a0 }))))
    }

    /// core.handle.realtime.receipt
    #[no_mangle]
    pub extern "C" fn jet_rt_receipt(a0: *mut crate::JetRealtimeStream) -> *mut crate::JetRealtimeReceipt {
        guard(|| Box::into_raw(Box::new(crate::jet_rt_receipt(unsafe { &*a0 }))))
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

    /// core.handle.solver.require
    #[no_mangle]
    pub extern "C" fn jet_solver_require(a0: *mut crate::jet_std::Solver, a1: u8) {
        guard(|| crate::jet_solver_require(unsafe { &mut *a0 }, a1 != 0))
    }

    /// core.handle.solver.status
    #[no_mangle]
    pub extern "C" fn jet_solver_status(a0: *mut crate::jet_std::Solver) -> JetCString {
        guard(|| handle(crate::jet_solver_status(unsafe { &*a0 })))
    }

    /// core.math.stats.clip
    #[no_mangle]
    pub extern "C" fn jet_stats_clip(a0_0: *const u64, a0_1: i64, a1: f64, a2: f64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_stats_clip(list_in(a0_0, a0_1, |w| f64::from_bits(w)), a1, a2).into_iter().map(|e| e.to_bits()).collect(), data))
    }

    /// core.math.stats.correlation, core.math.stats.pearson
    #[no_mangle]
    pub extern "C" fn jet_stats_correlation(a0_0: *const u64, a0_1: i64, a1_0: *const u64, a1_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_correlation(list_in(a0_0, a0_1, |w| f64::from_bits(w)), list_in(a1_0, a1_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.count
    #[no_mangle]
    pub extern "C" fn jet_stats_count(a0_0: *const u64, a0_1: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_stats_count(list_in(a0_0, a0_1, |w| f64::from_bits(w)))))
    }

    /// core.math.stats.covariance
    #[no_mangle]
    pub extern "C" fn jet_stats_covariance(a0_0: *const u64, a0_1: i64, a1_0: *const u64, a1_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_covariance(list_in(a0_0, a0_1, |w| f64::from_bits(w)), list_in(a1_0, a1_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.covariance_population
    #[no_mangle]
    pub extern "C" fn jet_stats_covariance_population(a0_0: *const u64, a0_1: i64, a1_0: *const u64, a1_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_covariance_population(list_in(a0_0, a0_1, |w| f64::from_bits(w)), list_in(a1_0, a1_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.cumprod
    #[no_mangle]
    pub extern "C" fn jet_stats_cumprod(a0_0: *const u64, a0_1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_stats_cumprod(list_in(a0_0, a0_1, |w| f64::from_bits(w))).into_iter().map(|e| e.to_bits()).collect(), data))
    }

    /// core.math.stats.cumsum
    #[no_mangle]
    pub extern "C" fn jet_stats_cumsum(a0_0: *const u64, a0_1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_stats_cumsum(list_in(a0_0, a0_1, |w| f64::from_bits(w))).into_iter().map(|e| e.to_bits()).collect(), data))
    }

    /// core.math.stats.diff
    #[no_mangle]
    pub extern "C" fn jet_stats_diff(a0_0: *const u64, a0_1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_stats_diff(list_in(a0_0, a0_1, |w| f64::from_bits(w))).into_iter().map(|e| e.to_bits()).collect(), data))
    }

    /// core.math.stats.ewma
    #[no_mangle]
    pub extern "C" fn jet_stats_ewma(a0_0: *const u64, a0_1: i64, a1: f64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_stats_ewma(list_in(a0_0, a0_1, |w| f64::from_bits(w)), a1).into_iter().map(|e| e.to_bits()).collect(), data))
    }

    /// core.math.stats.geometric_mean
    #[no_mangle]
    pub extern "C" fn jet_stats_geometric_mean(a0_0: *const u64, a0_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_geometric_mean(list_in(a0_0, a0_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.harmonic_mean
    #[no_mangle]
    pub extern "C" fn jet_stats_harmonic_mean(a0_0: *const u64, a0_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_harmonic_mean(list_in(a0_0, a0_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.histogram
    #[no_mangle]
    pub extern "C" fn jet_stats_histogram(a0_0: *const u64, a0_1: i64, a1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_stats_histogram(list_in(a0_0, a0_1, |w| f64::from_bits(w)), a1).into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.math.stats.iqr
    #[no_mangle]
    pub extern "C" fn jet_stats_iqr(a0_0: *const u64, a0_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_iqr(list_in(a0_0, a0_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.kde
    #[no_mangle]
    pub extern "C" fn jet_stats_kde(a0_0: *const u64, a0_1: i64, a1: f64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_stats_kde(list_in(a0_0, a0_1, |w| f64::from_bits(w)), a1).into_iter().map(|e| e.to_bits()).collect(), data))
    }

    /// core.math.stats.kde_random
    #[no_mangle]
    pub extern "C" fn jet_stats_kde_random(a0_0: *const u64, a0_1: i64, a1: f64, a2: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_stats_kde_random(list_in(a0_0, a0_1, |w| f64::from_bits(w)), a1, a2).into_iter().map(|e| e.to_bits()).collect(), data))
    }

    /// core.math.stats.kurtosis
    #[no_mangle]
    pub extern "C" fn jet_stats_kurtosis(a0_0: *const u64, a0_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_kurtosis(list_in(a0_0, a0_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.mad
    #[no_mangle]
    pub extern "C" fn jet_stats_mad(a0_0: *const u64, a0_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_mad(list_in(a0_0, a0_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.max
    #[no_mangle]
    pub extern "C" fn jet_stats_max(a0_0: *const u64, a0_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_max(list_in(a0_0, a0_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.mean, core.math.stats.fmean
    #[no_mangle]
    pub extern "C" fn jet_stats_mean(a0_0: *const u64, a0_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_mean(list_in(a0_0, a0_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.mean_abs_deviation
    #[no_mangle]
    pub extern "C" fn jet_stats_mean_abs_deviation(a0_0: *const u64, a0_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_mean_abs_deviation(list_in(a0_0, a0_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.median
    #[no_mangle]
    pub extern "C" fn jet_stats_median(a0_0: *const u64, a0_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_median(list_in(a0_0, a0_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.median_grouped
    #[no_mangle]
    pub extern "C" fn jet_stats_median_grouped(a0_0: *const u64, a0_1: i64, a1: f64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_median_grouped(list_in(a0_0, a0_1, |w| f64::from_bits(w)), a1) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.median_high
    #[no_mangle]
    pub extern "C" fn jet_stats_median_high(a0_0: *const u64, a0_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_median_high(list_in(a0_0, a0_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.median_low
    #[no_mangle]
    pub extern "C" fn jet_stats_median_low(a0_0: *const u64, a0_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_median_low(list_in(a0_0, a0_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.min
    #[no_mangle]
    pub extern "C" fn jet_stats_min(a0_0: *const u64, a0_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_min(list_in(a0_0, a0_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.mode
    #[no_mangle]
    pub extern "C" fn jet_stats_mode(a0_0: *const u64, a0_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_mode(list_in(a0_0, a0_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.moving_average
    #[no_mangle]
    pub extern "C" fn jet_stats_moving_average(a0_0: *const u64, a0_1: i64, a1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_stats_moving_average(list_in(a0_0, a0_1, |w| f64::from_bits(w)), a1).into_iter().map(|e| e.to_bits()).collect(), data))
    }

    /// core.math.stats.multimode
    #[no_mangle]
    pub extern "C" fn jet_stats_multimode(a0_0: *const u64, a0_1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_stats_multimode(list_in(a0_0, a0_1, |w| f64::from_bits(w))).into_iter().map(|e| e.to_bits()).collect(), data))
    }

    /// core.math.stats.percentile
    #[no_mangle]
    pub extern "C" fn jet_stats_percentile(a0_0: *const u64, a0_1: i64, a1: f64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_percentile(list_in(a0_0, a0_1, |w| f64::from_bits(w)), a1) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.prod
    #[no_mangle]
    pub extern "C" fn jet_stats_prod(a0_0: *const u64, a0_1: i64) -> f64 {
        guard(|| crate::jet_stats_prod(list_in(a0_0, a0_1, |w| f64::from_bits(w))))
    }

    /// core.math.stats.pstdev
    #[no_mangle]
    pub extern "C" fn jet_stats_pstdev(a0_0: *const u64, a0_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_pstdev(list_in(a0_0, a0_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.pvariance
    #[no_mangle]
    pub extern "C" fn jet_stats_pvariance(a0_0: *const u64, a0_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_pvariance(list_in(a0_0, a0_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.quantile
    #[no_mangle]
    pub extern "C" fn jet_stats_quantile(a0_0: *const u64, a0_1: i64, a1: f64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_quantile(list_in(a0_0, a0_1, |w| f64::from_bits(w)), a1) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.quantiles
    #[no_mangle]
    pub extern "C" fn jet_stats_quantiles(a0_0: *const u64, a0_1: i64, a1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_stats_quantiles(list_in(a0_0, a0_1, |w| f64::from_bits(w)), a1).into_iter().map(|e| e.to_bits()).collect(), data))
    }

    /// core.math.stats.range
    #[no_mangle]
    pub extern "C" fn jet_stats_range(a0_0: *const u64, a0_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_range(list_in(a0_0, a0_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.rank
    #[no_mangle]
    pub extern "C" fn jet_stats_rank(a0_0: *const u64, a0_1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_stats_rank(list_in(a0_0, a0_1, |w| f64::from_bits(w))).into_iter().map(|e| e.to_bits()).collect(), data))
    }

    /// core.math.stats.residuals
    #[no_mangle]
    pub extern "C" fn jet_stats_residuals(a0_0: *const u64, a0_1: i64, a1_0: *const u64, a1_1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_stats_residuals(list_in(a0_0, a0_1, |w| f64::from_bits(w)), list_in(a1_0, a1_1, |w| f64::from_bits(w))).into_iter().map(|e| e.to_bits()).collect(), data))
    }

    /// core.math.stats.skew
    #[no_mangle]
    pub extern "C" fn jet_stats_skew(a0_0: *const u64, a0_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_skew(list_in(a0_0, a0_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.spearman
    #[no_mangle]
    pub extern "C" fn jet_stats_spearman(a0_0: *const u64, a0_1: i64, a1_0: *const u64, a1_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_spearman(list_in(a0_0, a0_1, |w| f64::from_bits(w)), list_in(a1_0, a1_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.stdev
    #[no_mangle]
    pub extern "C" fn jet_stats_stdev(a0_0: *const u64, a0_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_stdev(list_in(a0_0, a0_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.sum
    #[no_mangle]
    pub extern "C" fn jet_stats_sum(a0_0: *const u64, a0_1: i64) -> f64 {
        guard(|| crate::jet_stats_sum(list_in(a0_0, a0_1, |w| f64::from_bits(w))))
    }

    /// core.math.stats.sumprod
    #[no_mangle]
    pub extern "C" fn jet_stats_sumprod(a0_0: *const u64, a0_1: i64, a1_0: *const u64, a1_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_sumprod(list_in(a0_0, a0_1, |w| f64::from_bits(w)), list_in(a1_0, a1_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.variance
    #[no_mangle]
    pub extern "C" fn jet_stats_variance(a0_0: *const u64, a0_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_variance(list_in(a0_0, a0_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.weighted_mean
    #[no_mangle]
    pub extern "C" fn jet_stats_weighted_mean(a0_0: *const u64, a0_1: i64, a1_0: *const u64, a1_1: i64, some: *mut f64) -> i64 {
        guard(|| match crate::jet_stats_weighted_mean(list_in(a0_0, a0_1, |w| f64::from_bits(w)), list_in(a1_0, a1_1, |w| f64::from_bits(w))) {
            Some(value) => { unsafe { some.write(value) }; 1 }
            None => 0,
        })
    }

    /// core.math.stats.winsorize
    #[no_mangle]
    pub extern "C" fn jet_stats_winsorize(a0_0: *const u64, a0_1: i64, a1: f64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_stats_winsorize(list_in(a0_0, a0_1, |w| f64::from_bits(w)), a1).into_iter().map(|e| e.to_bits()).collect(), data))
    }

    /// core.math.stats.zscore
    #[no_mangle]
    pub extern "C" fn jet_stats_zscore(a0_0: *const u64, a0_1: i64, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_stats_zscore(list_in(a0_0, a0_1, |w| f64::from_bits(w))).into_iter().map(|e| e.to_bits()).collect(), data))
    }

    /// core.encoding.base64.decode
    #[no_mangle]
    pub extern "C" fn jet_std_b64_decode(a0: JetCString, ok: *mut *mut u64, ok_len: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_std_b64_decode(unsafe { &*a0 }) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| e as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.encoding.hex.a2b_base64, core.encoding.base64.decodebytes
    #[no_mangle]
    pub extern "C" fn jet_std_b64_decodebytes(a0: JetCString, ok: *mut *mut u64, ok_len: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_std_b64_decodebytes(unsafe { &*a0 }) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| e as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
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

    /// core.encoding.base64.decode_url
    #[no_mangle]
    pub extern "C" fn jet_std_b64url_decode(a0: JetCString, ok: *mut *mut u64, ok_len: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_std_b64url_decode(unsafe { &*a0 }) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| e as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
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

    /// core.encoding.base32.b32decode
    #[no_mangle]
    pub extern "C" fn jet_std_base32_decode(a0: JetCString, ok: *mut *mut u64, ok_len: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_std_base32_decode(unsafe { &*a0 }) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| e as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.encoding.base32.b32encode
    #[no_mangle]
    pub extern "C" fn jet_std_base32_encode(a0_0: *const u64, a0_1: i64) -> JetCString {
        guard(|| handle(crate::jet_std_base32_encode(&list_in(a0_0, a0_1, |w| w as u8))))
    }

    /// core.encoding.base32.b32hexdecode
    #[no_mangle]
    pub extern "C" fn jet_std_base32hex_decode(a0: JetCString, ok: *mut *mut u64, ok_len: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_std_base32hex_decode(unsafe { &*a0 }) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| e as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
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

    /// core.encoding.binary.pack
    #[no_mangle]
    pub extern "C" fn jet_std_binary_pack(a0: JetCString, a1_0: *const u64, a1_1: i64, ok: *mut *mut u64, ok_len: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_std_binary_pack(unsafe { &*a0 }, &list_in(a1_0, a1_1, |w| w as i64)) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| e as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
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

    /// core.encoding.binary.unpack
    #[no_mangle]
    pub extern "C" fn jet_std_binary_unpack(a0: JetCString, a1_0: *const u64, a1_1: i64, ok: *mut *mut u64, ok_len: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_std_binary_unpack(unsafe { &*a0 }, &list_in(a1_0, a1_1, |w| w as u8)) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| e as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
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

    /// core.crypto.random.bytes
    #[no_mangle]
    pub extern "C" fn jet_std_crypto_random_bytes_controlled(a0: i64, some: *mut *mut u64, some_len: *mut i64) -> i64 {
        guard(|| match crate::jet_std_crypto_random_bytes_controlled(a0) {
            Some(value) => { let len = list_out(value.into_iter().map(|e| e as u64).collect(), some); unsafe { some_len.write(len) }; 1 }
            None => 0,
        })
    }

    /// core.sys.current_dir
    #[no_mangle]
    pub extern "C" fn jet_std_env_current_dir(ok: *mut JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_env_current_dir() {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.sys.get
    #[no_mangle]
    pub extern "C" fn jet_std_env_get(a0: JetCString, some: *mut JetCString) -> i64 {
        guard(|| match crate::jet_std_env_get(unsafe { &*a0 }) {
            Some(value) => { unsafe { some.write(handle(value)) }; 1 }
            None => 0,
        })
    }

    /// core.sys.home_dir
    #[no_mangle]
    pub extern "C" fn jet_std_env_home_dir(some: *mut JetCString) -> i64 {
        guard(|| match crate::jet_std_env_home_dir() {
            Some(value) => { unsafe { some.write(handle(value)) }; 1 }
            None => 0,
        })
    }

    /// core.sys.set
    #[no_mangle]
    pub extern "C" fn jet_std_env_set(a0: JetCString, a1: JetCString, err: *mut *mut crate::jet_std::EnvError) -> i64 {
        guard(|| match crate::jet_std_env_set(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.sys.unset
    #[no_mangle]
    pub extern "C" fn jet_std_env_unset(a0: JetCString, ok: *mut bool, err: *mut *mut crate::jet_std::EnvError) -> i64 {
        guard(|| match crate::jet_std_env_unset(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(value) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.sys.vars
    #[no_mangle]
    pub extern "C" fn jet_std_env_vars(ok: *mut *mut u64, ok_len: *mut i64, err: *mut *mut crate::jet_std::EnvError) -> i64 {
        guard(|| match crate::jet_std_env_vars() {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| handle(e) as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.file_reader.path
    #[no_mangle]
    pub extern "C" fn jet_std_file_reader_path(a0: *mut crate::JetFileReader) -> JetCString {
        guard(|| handle(crate::jet_std_file_reader_path(unsafe { &*a0 })))
    }

    /// core.handle.file_writer.flush
    #[no_mangle]
    pub extern "C" fn jet_std_file_writer_flush(a0: *mut crate::JetFileWriter, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_file_writer_flush(unsafe { &mut *a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.file_writer.path
    #[no_mangle]
    pub extern "C" fn jet_std_file_writer_path(a0: *mut crate::JetFileWriter) -> JetCString {
        guard(|| handle(crate::jet_std_file_writer_path(unsafe { &*a0 })))
    }

    /// core.handle.file_writer.write_line
    #[no_mangle]
    pub extern "C" fn jet_std_file_writer_write_line(a0: *mut crate::JetFileWriter, a1: JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_file_writer_write_line(unsafe { &mut *a0 }, unsafe { &*a1 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.append
    #[no_mangle]
    pub extern "C" fn jet_std_files_append(a0: JetCString, ok: *mut *mut crate::JetFileWriter, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_files_append(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.create
    #[no_mangle]
    pub extern "C" fn jet_std_files_create(a0: JetCString, ok: *mut *mut crate::JetFileWriter, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_files_create(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.open
    #[no_mangle]
    pub extern "C" fn jet_std_files_open(a0: JetCString, ok: *mut *mut crate::JetFileReader, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_files_open(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.absolute
    #[no_mangle]
    pub extern "C" fn jet_std_fs_absolute(a0: JetCString, ok: *mut JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_absolute(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.append_all
    #[no_mangle]
    pub extern "C" fn jet_std_fs_append(a0: JetCString, a1: JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_append(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.canonicalize
    #[no_mangle]
    pub extern "C" fn jet_std_fs_canonicalize(a0: JetCString, ok: *mut JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_canonicalize(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.chown
    #[no_mangle]
    pub extern "C" fn jet_std_fs_chown(a0: JetCString, a1: i64, a2: i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_chown(unsafe { &*a0 }, a1, a2) {
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

    /// core.files.fsync
    #[no_mangle]
    pub extern "C" fn jet_std_fs_fsync(a0: JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_fsync(unsafe { &*a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.glob
    #[no_mangle]
    pub extern "C" fn jet_std_fs_glob(a0: JetCString, ok: *mut *mut u64, ok_len: *mut i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_glob(unsafe { &*a0 }) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| handle(e) as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.hard_link
    #[no_mangle]
    pub extern "C" fn jet_std_fs_hard_link(a0: JetCString, a1: JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_hard_link(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.is_dir
    #[no_mangle]
    pub extern "C" fn jet_std_fs_is_dir(a0: JetCString) -> bool {
        guard(|| crate::jet_std_fs_is_dir(unsafe { &*a0 }))
    }

    /// core.files.is_fifo
    #[no_mangle]
    pub extern "C" fn jet_std_fs_is_fifo(a0: JetCString, ok: *mut bool, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_is_fifo(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(value) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.is_socket
    #[no_mangle]
    pub extern "C" fn jet_std_fs_is_socket(a0: JetCString, ok: *mut bool, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_is_socket(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(value) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.lock
    #[no_mangle]
    pub extern "C" fn jet_std_fs_lock(a0: JetCString, ok: *mut *mut crate::JetFileLockOwner, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_lock(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.map
    #[no_mangle]
    pub extern "C" fn jet_std_fs_map(a0: JetCString, ok: *mut *mut crate::jet_std::JetMappedFile, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_map(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.mapped_file.is_empty
    #[no_mangle]
    pub extern "C" fn jet_std_fs_map_is_empty(a0: *mut crate::jet_std::JetMappedFile) -> bool {
        guard(|| crate::jet_std_fs_map_is_empty(unsafe { &*a0 }))
    }

    /// core.handle.mapped_file.len
    #[no_mangle]
    pub extern "C" fn jet_std_fs_map_len(a0: *mut crate::jet_std::JetMappedFile) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_fs_map_len(unsafe { &*a0 })))
    }

    /// core.files.mktemp
    #[no_mangle]
    pub extern "C" fn jet_std_fs_mktemp_path(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_std_fs_mktemp_path(unsafe { &*a0 })))
    }

    /// core.files.read
    #[no_mangle]
    pub extern "C" fn jet_std_fs_read(a0: JetCString, ok: *mut JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_read(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.read_at
    #[no_mangle]
    pub extern "C" fn jet_std_fs_read_at(a0: JetCString, a1: i64, a2: i64, ok: *mut *mut u64, ok_len: *mut i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_read_at(unsafe { &*a0 }, a1, a2) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| e as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.read_bytes
    #[no_mangle]
    pub extern "C" fn jet_std_fs_read_bytes(a0: JetCString, ok: *mut *mut u64, ok_len: *mut i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_read_bytes(unsafe { &*a0 }) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| e as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.read_link
    #[no_mangle]
    pub extern "C" fn jet_std_fs_read_link(a0: JetCString, ok: *mut JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_read_link(unsafe { &*a0 }) {
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

    /// core.files.rename
    #[no_mangle]
    pub extern "C" fn jet_std_fs_rename(a0: JetCString, a1: JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_rename(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.scope
    #[no_mangle]
    pub extern "C" fn jet_std_fs_scope(a0: *mut crate::JetAuthority) -> *mut crate::JetFileScope {
        guard(|| Box::into_raw(Box::new(crate::jet_std_fs_scope(unsafe { &*a0 }))))
    }

    /// core.files.set_mode
    #[no_mangle]
    pub extern "C" fn jet_std_fs_set_mode(a0: JetCString, a1: i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_set_mode(unsafe { &*a0 }, a1) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.stat, core.files.lstat
    #[no_mangle]
    pub extern "C" fn jet_std_fs_stat(a0: JetCString, ok: *mut *mut crate::jet_std::Stat, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_stat(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.symlink
    #[no_mangle]
    pub extern "C" fn jet_std_fs_symlink(a0: JetCString, a1: JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_symlink(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.temp_dir
    #[no_mangle]
    pub extern "C" fn jet_std_fs_temp_dir(a0: JetCString, ok: *mut *mut crate::JetTempDirOwner, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_temp_dir(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.mkdtemp
    #[no_mangle]
    pub extern "C" fn jet_std_fs_temp_dir_path(a0: JetCString, ok: *mut JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_temp_dir_path(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.temp_file
    #[no_mangle]
    pub extern "C" fn jet_std_fs_temp_file(a0: JetCString, ok: *mut *mut crate::JetTempFileOwner, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_temp_file(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
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

    /// core.files.write_at
    #[no_mangle]
    pub extern "C" fn jet_std_fs_write_at(a0: JetCString, a1: i64, a2_0: *const u64, a2_1: i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_write_at(unsafe { &*a0 }, a1, &list_in(a2_0, a2_1, |w| w as u8)) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.write_atomic
    #[no_mangle]
    pub extern "C" fn jet_std_fs_write_atomic(a0: JetCString, a1_0: *const u64, a1_1: i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_write_atomic(unsafe { &*a0 }, &list_in(a1_0, a1_1, |w| w as u8)) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.files.write_bytes
    #[no_mangle]
    pub extern "C" fn jet_std_fs_write_bytes(a0: JetCString, a1_0: *const u64, a1_1: i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_fs_write_bytes(unsafe { &*a0 }, &list_in(a1_0, a1_1, |w| w as u8)) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.encoding.hex.decode
    #[no_mangle]
    pub extern "C" fn jet_std_hex_decode(a0: JetCString, ok: *mut *mut u64, ok_len: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_std_hex_decode(unsafe { &*a0 }) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| e as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
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

    /// core.process.argv
    #[no_mangle]
    pub extern "C" fn jet_std_io_args(data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_std_io_args().into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.term.binread
    #[no_mangle]
    pub extern "C" fn jet_std_io_binread(a0: JetCString, ok: *mut *mut u64, ok_len: *mut i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_io_binread(unsafe { &*a0 }) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| e as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.term.buffered
    #[no_mangle]
    pub extern "C" fn jet_std_io_buffered() -> *mut crate::JetStdinReader {
        guard(|| Box::into_raw(Box::new(crate::jet_std_io_buffered())))
    }

    /// core.term.choose
    #[no_mangle]
    pub extern "C" fn jet_std_io_choose(a0: JetCString, a1_0: *const u64, a1_1: i64, ok: *mut JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_io_choose(unsafe { &*a0 }, &list_in(a1_0, a1_1, |w| view(w as JetCString).to_owned())) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.term.confirm
    #[no_mangle]
    pub extern "C" fn jet_std_io_confirm(a0: JetCString) -> bool {
        guard(|| crate::jet_std_io_confirm(unsafe { &*a0 }))
    }

    /// core.term.eprint
    #[no_mangle]
    pub extern "C" fn jet_std_io_eprint(a0: JetCString) {
        guard(|| crate::jet_std_io_eprint(unsafe { &*a0 }))
    }

    /// core.term.input_secret
    #[no_mangle]
    pub extern "C" fn jet_std_io_input_secret(a0: JetCString, ok: *mut JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_io_input_secret(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.process.args
    #[no_mangle]
    pub extern "C" fn jet_std_io_process_args(data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_std_io_process_args().into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.term.progress
    #[no_mangle]
    pub extern "C" fn jet_std_io_progress(a0: JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_io_progress(unsafe { &*a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.term.read_all_input
    #[no_mangle]
    pub extern "C" fn jet_std_io_read_all_input(ok: *mut JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_io_read_all_input() {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.term.read_until
    #[no_mangle]
    pub extern "C" fn jet_std_io_read_until(a0: JetCString, ok: *mut JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_io_read_until(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.term.readline
    #[no_mangle]
    pub extern "C" fn jet_std_io_readline(ok: *mut JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_io_readline() {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.term.stderr
    #[no_mangle]
    pub extern "C" fn jet_std_io_stderr() -> *mut crate::JetStderr {
        guard(|| Box::into_raw(Box::new(crate::jet_std_io_stderr())))
    }

    /// core.handle.stderr.flush
    #[no_mangle]
    pub extern "C" fn jet_std_io_stderr_flush(a0: *mut crate::JetStderr, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_io_stderr_flush(unsafe { &*a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.stderr.is_tty
    #[no_mangle]
    pub extern "C" fn jet_std_io_stderr_is_tty(a0: *mut crate::JetStderr) -> bool {
        guard(|| crate::jet_std_io_stderr_is_tty(unsafe { &*a0 }))
    }

    /// core.handle.stderr.write
    #[no_mangle]
    pub extern "C" fn jet_std_io_stderr_write(a0: *mut crate::JetStderr, a1: JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_io_stderr_write(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.stderr.write_bytes
    #[no_mangle]
    pub extern "C" fn jet_std_io_stderr_write_bytes(a0: *mut crate::JetStderr, a1_0: *const u64, a1_1: i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_io_stderr_write_bytes(unsafe { &*a0 }, &list_in(a1_0, a1_1, |w| w as u8)) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.stderr.write_line
    #[no_mangle]
    pub extern "C" fn jet_std_io_stderr_write_line(a0: *mut crate::JetStderr, a1: JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_io_stderr_write_line(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.term.stdin
    #[no_mangle]
    pub extern "C" fn jet_std_io_stdin() -> *mut crate::JetStdinReader {
        guard(|| Box::into_raw(Box::new(crate::jet_std_io_stdin())))
    }

    /// core.term.stdout
    #[no_mangle]
    pub extern "C" fn jet_std_io_stdout() -> *mut crate::JetStdout {
        guard(|| Box::into_raw(Box::new(crate::jet_std_io_stdout())))
    }

    /// core.handle.stdout.flush
    #[no_mangle]
    pub extern "C" fn jet_std_io_stdout_flush(a0: *mut crate::JetStdout, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_io_stdout_flush(unsafe { &*a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.stdout.is_tty
    #[no_mangle]
    pub extern "C" fn jet_std_io_stdout_is_tty(a0: *mut crate::JetStdout) -> bool {
        guard(|| crate::jet_std_io_stdout_is_tty(unsafe { &*a0 }))
    }

    /// core.handle.stdout.write
    #[no_mangle]
    pub extern "C" fn jet_std_io_stdout_write(a0: *mut crate::JetStdout, a1: JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_io_stdout_write(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.stdout.write_bytes
    #[no_mangle]
    pub extern "C" fn jet_std_io_stdout_write_bytes(a0: *mut crate::JetStdout, a1_0: *const u64, a1_1: i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_io_stdout_write_bytes(unsafe { &*a0 }, &list_in(a1_0, a1_1, |w| w as u8)) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.stdout.write_line
    #[no_mangle]
    pub extern "C" fn jet_std_io_stdout_write_line(a0: *mut crate::JetStdout, a1: JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_io_stdout_write_line(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.term.style
    #[no_mangle]
    pub extern "C" fn jet_std_io_style(a0: JetCString, a1: JetCString) -> JetCString {
        guard(|| handle(crate::jet_std_io_style(unsafe { &*a0 }, unsafe { &*a1 })))
    }

    /// core.term.style_force
    #[no_mangle]
    pub extern "C" fn jet_std_io_style_force(a0: JetCString, a1: JetCString) -> JetCString {
        guard(|| handle(crate::jet_std_io_style_force(unsafe { &*a0 }, unsafe { &*a1 })))
    }

    /// core.term.take
    #[no_mangle]
    pub extern "C" fn jet_std_io_take(a0: i64, ok: *mut *mut u64, ok_len: *mut i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_io_take(a0) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| e as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.term.terminal_height
    #[no_mangle]
    pub extern "C" fn jet_std_io_terminal_height() -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_io_terminal_height()))
    }

    /// core.term.terminal_width
    #[no_mangle]
    pub extern "C" fn jet_std_io_terminal_width() -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_io_terminal_width()))
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

    /// core.encoding.jsonl.count_rows
    #[no_mangle]
    pub extern "C" fn jet_std_jsonl_count_rows(a0: JetCString) -> i64 {
        guard(|| (crate::jet_std_jsonl_count_rows(unsafe { &*a0 })).into_raw())
    }

    /// core.math.abs_diff
    #[no_mangle]
    pub extern "C" fn jet_std_math_abs_diff(a0: i64, a1: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_math_abs_diff(a0, a1)))
    }

    /// core.math.fabs
    #[no_mangle]
    pub extern "C" fn jet_std_math_abs_f64(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_abs_f64(a0))
    }

    /// core.math.acos
    #[no_mangle]
    pub extern "C" fn jet_std_math_acos(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_acos(a0))
    }

    /// core.math.acosh
    #[no_mangle]
    pub extern "C" fn jet_std_math_acosh(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_acosh(a0))
    }

    /// core.math.asin
    #[no_mangle]
    pub extern "C" fn jet_std_math_asin(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_asin(a0))
    }

    /// core.math.asinh
    #[no_mangle]
    pub extern "C" fn jet_std_math_asinh(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_asinh(a0))
    }

    /// core.math.atan
    #[no_mangle]
    pub extern "C" fn jet_std_math_atan(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_atan(a0))
    }

    /// core.math.atan2
    #[no_mangle]
    pub extern "C" fn jet_std_math_atan2(a0: f64, a1: f64) -> f64 {
        guard(|| crate::jet_std_math_atan2(a0, a1))
    }

    /// core.math.atanh
    #[no_mangle]
    pub extern "C" fn jet_std_math_atanh(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_atanh(a0))
    }

    /// core.math.combinatorics.binomial, core.math.combinatorics.combinations_count, core.math.combinatorics.ncr, core.math.binomial, core.math.comb
    #[no_mangle]
    pub extern "C" fn jet_std_math_binomial(a0: i64, a1: i64, some: *mut i64) -> i64 {
        guard(|| match crate::jet_std_math_binomial(a0, a1) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.math.cbrt
    #[no_mangle]
    pub extern "C" fn jet_std_math_cbrt(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_cbrt(a0))
    }

    /// core.math.ceil
    #[no_mangle]
    pub extern "C" fn jet_std_math_ceil(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_ceil(a0))
    }

    /// core.math.checked_abs
    #[no_mangle]
    pub extern "C" fn jet_std_math_checked_abs(a0: i64, some: *mut i64) -> i64 {
        guard(|| match crate::jet_std_math_checked_abs(a0) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.math.checked_add
    #[no_mangle]
    pub extern "C" fn jet_std_math_checked_add(a0: i64, a1: i64, some: *mut i64) -> i64 {
        guard(|| match crate::jet_std_math_checked_add(a0, a1) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.math.checked_div
    #[no_mangle]
    pub extern "C" fn jet_std_math_checked_div(a0: i64, a1: i64, some: *mut i64) -> i64 {
        guard(|| match crate::jet_std_math_checked_div(a0, a1) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.math.checked_mul
    #[no_mangle]
    pub extern "C" fn jet_std_math_checked_mul(a0: i64, a1: i64, some: *mut i64) -> i64 {
        guard(|| match crate::jet_std_math_checked_mul(a0, a1) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.math.checked_neg
    #[no_mangle]
    pub extern "C" fn jet_std_math_checked_neg(a0: i64, some: *mut i64) -> i64 {
        guard(|| match crate::jet_std_math_checked_neg(a0) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.math.checked_pow
    #[no_mangle]
    pub extern "C" fn jet_std_math_checked_pow(a0: i64, a1: i64, some: *mut i64) -> i64 {
        guard(|| match crate::jet_std_math_checked_pow(a0, a1) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.math.checked_rem
    #[no_mangle]
    pub extern "C" fn jet_std_math_checked_rem(a0: i64, a1: i64, some: *mut i64) -> i64 {
        guard(|| match crate::jet_std_math_checked_rem(a0, a1) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.math.checked_sub
    #[no_mangle]
    pub extern "C" fn jet_std_math_checked_sub(a0: i64, a1: i64, some: *mut i64) -> i64 {
        guard(|| match crate::jet_std_math_checked_sub(a0, a1) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.math.clamp_float
    #[no_mangle]
    pub extern "C" fn jet_std_math_clamp_f64(a0: f64, a1: f64, a2: f64) -> f64 {
        guard(|| crate::jet_std_math_clamp_f64(a0, a1, a2))
    }

    /// core.math.cmp
    #[no_mangle]
    pub extern "C" fn jet_std_math_cmp(a0: f64, a1: f64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_math_cmp(a0, a1)))
    }

    /// core.math.copysign
    #[no_mangle]
    pub extern "C" fn jet_std_math_copysign(a0: f64, a1: f64) -> f64 {
        guard(|| crate::jet_std_math_copysign(a0, a1))
    }

    /// core.math.cos
    #[no_mangle]
    pub extern "C" fn jet_std_math_cos(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_cos(a0))
    }

    /// core.math.cosh
    #[no_mangle]
    pub extern "C" fn jet_std_math_cosh(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_cosh(a0))
    }

    /// core.math.cot
    #[no_mangle]
    pub extern "C" fn jet_std_math_cot(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_cot(a0))
    }

    /// core.math.degrees
    #[no_mangle]
    pub extern "C" fn jet_std_math_degrees(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_degrees(a0))
    }

    /// core.math.digits
    #[no_mangle]
    pub extern "C" fn jet_std_math_digits(a0: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_math_digits(a0)))
    }

    /// core.math.dist
    #[no_mangle]
    pub extern "C" fn jet_std_math_dist(a0_0: *const u64, a0_1: i64, a1_0: *const u64, a1_1: i64) -> f64 {
        guard(|| crate::jet_std_math_dist(list_in(a0_0, a0_1, |w| f64::from_bits(w)), list_in(a1_0, a1_1, |w| f64::from_bits(w))))
    }

    /// core.math.div_mod
    #[no_mangle]
    pub extern "C" fn jet_std_math_div_mod(a0: i64, a1: i64, item0: *mut i64, item1: *mut i64) {
        guard(|| {
            let (e0, e1) = crate::jet_std_math_div_mod(a0, a1);
            unsafe { item0.write(crate::jet_std::jet_int_from_i64(e0)) };
            unsafe { item1.write(crate::jet_std::jet_int_from_i64(e1)) };
        })
    }

    /// core.math.div_rem
    #[no_mangle]
    pub extern "C" fn jet_std_math_div_rem(a0: i64, a1: i64, item0: *mut i64, item1: *mut i64) {
        guard(|| {
            let (e0, e1) = crate::jet_std_math_div_rem(a0, a1);
            unsafe { item0.write(crate::jet_std::jet_int_from_i64(e0)) };
            unsafe { item1.write(crate::jet_std::jet_int_from_i64(e1)) };
        })
    }

    /// core.math.erf
    #[no_mangle]
    pub extern "C" fn jet_std_math_erf(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_erf(a0))
    }

    /// core.math.erfc
    #[no_mangle]
    pub extern "C" fn jet_std_math_erfc(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_erfc(a0))
    }

    /// core.math.exp
    #[no_mangle]
    pub extern "C" fn jet_std_math_exp(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_exp(a0))
    }

    /// core.math.exp_m1, core.math.expm1
    #[no_mangle]
    pub extern "C" fn jet_std_math_exp_m1(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_exp_m1(a0))
    }

    /// core.math.exp2
    #[no_mangle]
    pub extern "C" fn jet_std_math_exp2(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_exp2(a0))
    }

    /// core.math.factorial, core.math.combinatorics.factorial
    #[no_mangle]
    pub extern "C" fn jet_std_math_factorial(a0: i64, some: *mut i64) -> i64 {
        guard(|| match crate::jet_std_math_factorial(a0) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.math.floor
    #[no_mangle]
    pub extern "C" fn jet_std_math_floor(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_floor(a0))
    }

    /// core.math.fma, core.math.muladd
    #[no_mangle]
    pub extern "C" fn jet_std_math_fma(a0: f64, a1: f64, a2: f64) -> f64 {
        guard(|| crate::jet_std_math_fma(a0, a1, a2))
    }

    /// core.math.fmod
    #[no_mangle]
    pub extern "C" fn jet_std_math_fmod(a0: f64, a1: f64) -> f64 {
        guard(|| crate::jet_std_math_fmod(a0, a1))
    }

    /// core.math.fract
    #[no_mangle]
    pub extern "C" fn jet_std_math_fract(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_fract(a0))
    }

    /// core.math.frexp
    #[no_mangle]
    pub extern "C" fn jet_std_math_frexp(a0: f64, item0: *mut f64, item1: *mut i64) {
        guard(|| {
            let (e0, e1) = crate::jet_std_math_frexp(a0);
            unsafe { item0.write(e0) };
            unsafe { item1.write(crate::jet_std::jet_int_from_i64(e1)) };
        })
    }

    /// core.math.from_bits
    #[no_mangle]
    pub extern "C" fn jet_std_math_from_bits(a0: i64) -> f64 {
        guard(|| crate::jet_std_math_from_bits(a0))
    }

    /// core.math.fsum
    #[no_mangle]
    pub extern "C" fn jet_std_math_fsum(a0_0: *const u64, a0_1: i64) -> f64 {
        guard(|| crate::jet_std_math_fsum(list_in(a0_0, a0_1, |w| f64::from_bits(w))))
    }

    /// core.math.gamma
    #[no_mangle]
    pub extern "C" fn jet_std_math_gamma(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_gamma(a0))
    }

    /// core.math.gcd_many
    #[no_mangle]
    pub extern "C" fn jet_std_math_gcd_many(a0_0: *const u64, a0_1: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_math_gcd_many(&list_in(a0_0, a0_1, |w| w as i64))))
    }

    /// core.math.hypot
    #[no_mangle]
    pub extern "C" fn jet_std_math_hypot(a0: f64, a1: f64) -> f64 {
        guard(|| crate::jet_std_math_hypot(a0, a1))
    }

    /// core.math.hypot3
    #[no_mangle]
    pub extern "C" fn jet_std_math_hypot3(a0: f64, a1: f64, a2: f64) -> f64 {
        guard(|| crate::jet_std_math_hypot3(a0, a1, a2))
    }

    /// core.math.real, core.math.conj, core.math.float32, core.math.float64
    #[no_mangle]
    pub extern "C" fn jet_std_math_identity_f64(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_identity_f64(a0))
    }

    /// core.math.ilogb
    #[no_mangle]
    pub extern "C" fn jet_std_math_ilogb(a0: f64, some: *mut i64) -> i64 {
        guard(|| match crate::jet_std_math_ilogb(a0) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.math.in_range
    #[no_mangle]
    pub extern "C" fn jet_std_math_in_range(a0: i64, a1: i64, a2: i64) -> bool {
        guard(|| crate::jet_std_math_in_range(a0, a1, a2))
    }

    /// core.math.int_pow
    #[no_mangle]
    pub extern "C" fn jet_std_math_int_pow(a0: i64, a1: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_math_int_pow(a0, a1)))
    }

    /// core.math.inv
    #[no_mangle]
    pub extern "C" fn jet_std_math_inv(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_inv(a0))
    }

    /// core.math.is_canonical
    #[no_mangle]
    pub extern "C" fn jet_std_math_is_canonical(a0: f64) -> bool {
        guard(|| crate::jet_std_math_is_canonical(a0))
    }

    /// core.math.even
    #[no_mangle]
    pub extern "C" fn jet_std_math_is_even(a0: i64) -> bool {
        guard(|| crate::jet_std_math_is_even(a0))
    }

    /// core.math.is_finite, core.math.isfinite
    #[no_mangle]
    pub extern "C" fn jet_std_math_is_finite(a0: f64) -> bool {
        guard(|| crate::jet_std_math_is_finite(a0))
    }

    /// core.math.isinf, core.math.is_inf
    #[no_mangle]
    pub extern "C" fn jet_std_math_is_infinite(a0: f64) -> bool {
        guard(|| crate::jet_std_math_is_infinite(a0))
    }

    /// core.math.is_integer
    #[no_mangle]
    pub extern "C" fn jet_std_math_is_integer(a0: f64) -> bool {
        guard(|| crate::jet_std_math_is_integer(a0))
    }

    /// core.math.isnan, core.math.is_nan
    #[no_mangle]
    pub extern "C" fn jet_std_math_is_nan(a0: f64) -> bool {
        guard(|| crate::jet_std_math_is_nan(a0))
    }

    /// core.math.is_normal
    #[no_mangle]
    pub extern "C" fn jet_std_math_is_normal(a0: f64) -> bool {
        guard(|| crate::jet_std_math_is_normal(a0))
    }

    /// core.math.odd
    #[no_mangle]
    pub extern "C" fn jet_std_math_is_odd(a0: i64) -> bool {
        guard(|| crate::jet_std_math_is_odd(a0))
    }

    /// core.math.is_signed
    #[no_mangle]
    pub extern "C" fn jet_std_math_is_signed(a0: f64) -> bool {
        guard(|| crate::jet_std_math_is_signed(a0))
    }

    /// core.math.is_subnormal
    #[no_mangle]
    pub extern "C" fn jet_std_math_is_subnormal(a0: f64) -> bool {
        guard(|| crate::jet_std_math_is_subnormal(a0))
    }

    /// core.math.is_zero
    #[no_mangle]
    pub extern "C" fn jet_std_math_is_zero(a0: f64) -> bool {
        guard(|| crate::jet_std_math_is_zero(a0))
    }

    /// core.math.isclose
    #[no_mangle]
    pub extern "C" fn jet_std_math_isclose(a0: f64, a1: f64, a2: f64, a3: f64) -> bool {
        guard(|| crate::jet_std_math_isclose(a0, a1, a2, a3))
    }

    /// core.math.lcm_many
    #[no_mangle]
    pub extern "C" fn jet_std_math_lcm_many(a0_0: *const u64, a0_1: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_math_lcm_many(&list_in(a0_0, a0_1, |w| w as i64))))
    }

    /// core.math.ldexp, core.math.scaleb
    #[no_mangle]
    pub extern "C" fn jet_std_math_ldexp(a0: f64, a1: i64) -> f64 {
        guard(|| crate::jet_std_math_ldexp(a0, a1))
    }

    /// core.math.leading_ones
    #[no_mangle]
    pub extern "C" fn jet_std_math_leading_ones(a0: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_math_leading_ones(a0)))
    }

    /// core.math.lerp
    #[no_mangle]
    pub extern "C" fn jet_std_math_lerp(a0: f64, a1: f64, a2: f64) -> f64 {
        guard(|| crate::jet_std_math_lerp(a0, a1, a2))
    }

    /// core.math.lgamma
    #[no_mangle]
    pub extern "C" fn jet_std_math_lgamma(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_lgamma(a0))
    }

    /// core.math.ln
    #[no_mangle]
    pub extern "C" fn jet_std_math_ln(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_ln(a0))
    }

    /// core.math.ln_1p, core.math.log1p
    #[no_mangle]
    pub extern "C" fn jet_std_math_ln_1p(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_ln_1p(a0))
    }

    /// core.math.log
    #[no_mangle]
    pub extern "C" fn jet_std_math_log(a0: f64, a1: f64) -> f64 {
        guard(|| crate::jet_std_math_log(a0, a1))
    }

    /// core.math.log10
    #[no_mangle]
    pub extern "C" fn jet_std_math_log10(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_log10(a0))
    }

    /// core.math.log2
    #[no_mangle]
    pub extern "C" fn jet_std_math_log2(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_log2(a0))
    }

    /// core.math.logb
    #[no_mangle]
    pub extern "C" fn jet_std_math_logb(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_logb(a0))
    }

    /// core.math.max_float
    #[no_mangle]
    pub extern "C" fn jet_std_math_max_f64(a0: f64, a1: f64) -> f64 {
        guard(|| crate::jet_std_math_max_f64(a0, a1))
    }

    /// core.math.midpoint
    #[no_mangle]
    pub extern "C" fn jet_std_math_midpoint(a0: f64, a1: f64) -> f64 {
        guard(|| crate::jet_std_math_midpoint(a0, a1))
    }

    /// core.math.min_float
    #[no_mangle]
    pub extern "C" fn jet_std_math_min_f64(a0: f64, a1: f64) -> f64 {
        guard(|| crate::jet_std_math_min_f64(a0, a1))
    }

    /// core.math.modf
    #[no_mangle]
    pub extern "C" fn jet_std_math_modf(a0: f64, item0: *mut f64, item1: *mut f64) {
        guard(|| {
            let (e0, e1) = crate::jet_std_math_modf(a0);
            unsafe { item0.write(e0) };
            unsafe { item1.write(e1) };
        })
    }

    /// core.math.combinatorics.multinomial
    #[no_mangle]
    pub extern "C" fn jet_std_math_multinomial(a0_0: *const u64, a0_1: i64, some: *mut i64) -> i64 {
        guard(|| match crate::jet_std_math_multinomial(list_in(a0_0, a0_1, |w| w as i64)) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.math.next_after, core.math.nextafter
    #[no_mangle]
    pub extern "C" fn jet_std_math_next_after(a0: f64, a1: f64) -> f64 {
        guard(|| crate::jet_std_math_next_after(a0, a1))
    }

    /// core.math.next_down
    #[no_mangle]
    pub extern "C" fn jet_std_math_next_down(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_next_down(a0))
    }

    /// core.math.next_up
    #[no_mangle]
    pub extern "C" fn jet_std_math_next_up(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_next_up(a0))
    }

    /// core.math.combinatorics.permutations_count, core.math.combinatorics.npr, core.math.combinatorics.falling_factorial, core.math.perm
    #[no_mangle]
    pub extern "C" fn jet_std_math_perm(a0: i64, a1: i64, some: *mut i64) -> i64 {
        guard(|| match crate::jet_std_math_perm(a0, a1) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.math.pi
    #[no_mangle]
    pub extern "C" fn jet_std_math_pi() -> f64 {
        guard(|| crate::jet_std_math_pi())
    }

    /// core.math.pow
    #[no_mangle]
    pub extern "C" fn jet_std_math_pow(a0: f64, a1: f64) -> f64 {
        guard(|| crate::jet_std_math_pow(a0, a1))
    }

    /// core.math.powmod
    #[no_mangle]
    pub extern "C" fn jet_std_math_powmod(a0: i64, a1: i64, a2: i64, some: *mut i64) -> i64 {
        guard(|| match crate::jet_std_math_powmod(a0, a1, a2) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.math.prod
    #[no_mangle]
    pub extern "C" fn jet_std_math_prod_int(a0_0: *const u64, a0_1: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_math_prod_int(list_in(a0_0, a0_1, |w| w as i64))))
    }

    /// core.math.prod_int
    #[no_mangle]
    pub extern "C" fn jet_std_math_prod_int_ref(a0_0: *const u64, a0_1: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_math_prod_int_ref(&list_in(a0_0, a0_1, |w| w as i64))))
    }

    /// core.math.radians
    #[no_mangle]
    pub extern "C" fn jet_std_math_radians(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_radians(a0))
    }

    /// core.math.radix
    #[no_mangle]
    pub extern "C" fn jet_std_math_radix(a0: f64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_math_radix(a0)))
    }

    /// core.math.remainder
    #[no_mangle]
    pub extern "C" fn jet_std_math_remainder(a0: f64, a1: f64) -> f64 {
        guard(|| crate::jet_std_math_remainder(a0, a1))
    }

    /// core.math.combinatorics.rising_factorial
    #[no_mangle]
    pub extern "C" fn jet_std_math_rising_factorial(a0: i64, a1: i64, some: *mut i64) -> i64 {
        guard(|| match crate::jet_std_math_rising_factorial(a0, a1) {
            Some(value) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            None => 0,
        })
    }

    /// core.math.round
    #[no_mangle]
    pub extern "C" fn jet_std_math_round(a0: f64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_math_round(a0)))
    }

    /// core.math.saturating_add
    #[no_mangle]
    pub extern "C" fn jet_std_math_saturating_add(a0: i64, a1: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_math_saturating_add(a0, a1)))
    }

    /// core.math.saturating_mul
    #[no_mangle]
    pub extern "C" fn jet_std_math_saturating_mul(a0: i64, a1: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_math_saturating_mul(a0, a1)))
    }

    /// core.math.saturating_sub
    #[no_mangle]
    pub extern "C" fn jet_std_math_saturating_sub(a0: i64, a1: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_math_saturating_sub(a0, a1)))
    }

    /// core.math.sign_bit
    #[no_mangle]
    pub extern "C" fn jet_std_math_sign_bit(a0: f64) -> bool {
        guard(|| crate::jet_std_math_sign_bit(a0))
    }

    /// core.math.significand
    #[no_mangle]
    pub extern "C" fn jet_std_math_significand(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_significand(a0))
    }

    /// core.math.signum
    #[no_mangle]
    pub extern "C" fn jet_std_math_signum(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_signum(a0))
    }

    /// core.math.sin
    #[no_mangle]
    pub extern "C" fn jet_std_math_sin(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_sin(a0))
    }

    /// core.math.sin_cos
    #[no_mangle]
    pub extern "C" fn jet_std_math_sin_cos(a0: f64, item0: *mut f64, item1: *mut f64) {
        guard(|| {
            let (e0, e1) = crate::jet_std_math_sin_cos(a0);
            unsafe { item0.write(e0) };
            unsafe { item1.write(e1) };
        })
    }

    /// core.math.sinh
    #[no_mangle]
    pub extern "C" fn jet_std_math_sinh(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_sinh(a0))
    }

    /// core.math.sum_int
    #[no_mangle]
    pub extern "C" fn jet_std_math_sum_int(a0_0: *const u64, a0_1: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_math_sum_int(&list_in(a0_0, a0_1, |w| w as i64))))
    }

    /// core.math.sumprod
    #[no_mangle]
    pub extern "C" fn jet_std_math_sumprod(a0_0: *const u64, a0_1: i64, a1_0: *const u64, a1_1: i64) -> f64 {
        guard(|| crate::jet_std_math_sumprod(list_in(a0_0, a0_1, |w| f64::from_bits(w)), list_in(a1_0, a1_1, |w| f64::from_bits(w))))
    }

    /// core.math.tan
    #[no_mangle]
    pub extern "C" fn jet_std_math_tan(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_tan(a0))
    }

    /// core.math.tanh
    #[no_mangle]
    pub extern "C" fn jet_std_math_tanh(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_tanh(a0))
    }

    /// core.math.tau_const
    #[no_mangle]
    pub extern "C" fn jet_std_math_tau() -> f64 {
        guard(|| crate::jet_std_math_tau())
    }

    /// core.math.to_bits
    #[no_mangle]
    pub extern "C" fn jet_std_math_to_bits(a0: f64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_math_to_bits(a0)))
    }

    /// core.math.trailing_ones
    #[no_mangle]
    pub extern "C" fn jet_std_math_trailing_ones(a0: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_math_trailing_ones(a0)))
    }

    /// core.math.trunc, core.math.truncate
    #[no_mangle]
    pub extern "C" fn jet_std_math_trunc(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_trunc(a0))
    }

    /// core.math.ulp
    #[no_mangle]
    pub extern "C" fn jet_std_math_ulp(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_ulp(a0))
    }

    /// core.math.xor
    #[no_mangle]
    pub extern "C" fn jet_std_math_xor(a0: i64, a1: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_math_xor(a0, a1)))
    }

    /// core.math.zero
    #[no_mangle]
    pub extern "C" fn jet_std_math_zero() -> f64 {
        guard(|| crate::jet_std_math_zero())
    }

    /// core.math.imag
    #[no_mangle]
    pub extern "C" fn jet_std_math_zero_f64(a0: f64) -> f64 {
        guard(|| crate::jet_std_math_zero_f64(a0))
    }

    /// core.sys.arch
    #[no_mangle]
    pub extern "C" fn jet_std_os_arch() -> JetCString {
        guard(|| handle(crate::jet_std_os_arch()))
    }

    /// core.sys.close_fd
    #[no_mangle]
    pub extern "C" fn jet_std_os_close_fd(a0: i64) {
        guard(|| crate::jet_std_os_close_fd(a0))
    }

    /// core.sys.cpu_count
    #[no_mangle]
    pub extern "C" fn jet_std_os_cpu_count() -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_os_cpu_count()))
    }

    /// core.sys.executable
    #[no_mangle]
    pub extern "C" fn jet_std_os_executable() -> JetCString {
        guard(|| handle(crate::jet_std_os_executable()))
    }

    /// core.sys.exitcode
    #[no_mangle]
    pub extern "C" fn jet_std_os_exitcode(a0: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_os_exitcode(a0)))
    }

    /// core.sys.expand
    #[no_mangle]
    pub extern "C" fn jet_std_os_expand(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_std_os_expand(unsafe { &*a0 })))
    }

    /// core.sys.family
    #[no_mangle]
    pub extern "C" fn jet_std_os_family() -> JetCString {
        guard(|| handle(crate::jet_std_os_family()))
    }

    /// core.sys.fork
    #[no_mangle]
    pub extern "C" fn jet_std_os_fork(ok: *mut i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_os_fork() {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.sys.getegid
    #[no_mangle]
    pub extern "C" fn jet_std_os_getegid() -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_os_getegid()))
    }

    /// core.sys.geteuid
    #[no_mangle]
    pub extern "C" fn jet_std_os_geteuid() -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_os_geteuid()))
    }

    /// core.sys.getgid
    #[no_mangle]
    pub extern "C" fn jet_std_os_getgid() -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_os_getgid()))
    }

    /// core.sys.getgroups
    #[no_mangle]
    pub extern "C" fn jet_std_os_getgroups(data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_std_os_getgroups().into_iter().map(|e| e as u64).collect(), data))
    }

    /// core.sys.getpgid
    #[no_mangle]
    pub extern "C" fn jet_std_os_getpgid(a0: i64, ok: *mut i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_os_getpgid(a0) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.sys.getpgrp
    #[no_mangle]
    pub extern "C" fn jet_std_os_getpgrp() -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_os_getpgrp()))
    }

    /// core.sys.getppid
    #[no_mangle]
    pub extern "C" fn jet_std_os_getppid() -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_os_getppid()))
    }

    /// core.sys.getpriority
    #[no_mangle]
    pub extern "C" fn jet_std_os_getpriority(a0: i64, ok: *mut i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_os_getpriority(a0) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.sys.getsid
    #[no_mangle]
    pub extern "C" fn jet_std_os_getsid(a0: i64, ok: *mut i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_os_getsid(a0) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.sys.getuid
    #[no_mangle]
    pub extern "C" fn jet_std_os_getuid() -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_os_getuid()))
    }

    /// core.sys.hostname, core.net.gethostname
    #[no_mangle]
    pub extern "C" fn jet_std_os_hostname() -> JetCString {
        guard(|| handle(crate::jet_std_os_hostname()))
    }

    /// core.sys.initgroups
    #[no_mangle]
    pub extern "C" fn jet_std_os_initgroups(a0: JetCString, a1: i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_os_initgroups(unsafe { &*a0 }, a1) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.sys.kill
    #[no_mangle]
    pub extern "C" fn jet_std_os_kill(a0: i64, a1: i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_os_kill(a0, a1) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.sys.loadavg
    #[no_mangle]
    pub extern "C" fn jet_std_os_loadavg(data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_std_os_loadavg().into_iter().map(|e| e.to_bits()).collect(), data))
    }

    /// core.sys.mkfifo
    #[no_mangle]
    pub extern "C" fn jet_std_os_mkfifo(a0: JetCString, a1: i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_os_mkfifo(unsafe { &*a0 }, a1) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.sys.name
    #[no_mangle]
    pub extern "C" fn jet_std_os_name() -> JetCString {
        guard(|| handle(crate::jet_std_os_name()))
    }

    /// core.sys.pid, core.sys.getpid
    #[no_mangle]
    pub extern "C" fn jet_std_os_pid() -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_os_pid()))
    }

    /// core.sys.pipe
    #[no_mangle]
    pub extern "C" fn jet_std_os_pipe(ok: *mut *mut u64, ok_len: *mut i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_os_pipe() {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| e as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.sys.release
    #[no_mangle]
    pub extern "C" fn jet_std_os_release() -> JetCString {
        guard(|| handle(crate::jet_std_os_release()))
    }

    /// core.sys.set_current_dir
    #[no_mangle]
    pub extern "C" fn jet_std_os_set_current_dir(a0: JetCString, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_os_set_current_dir(unsafe { &*a0 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.sys.setgid
    #[no_mangle]
    pub extern "C" fn jet_std_os_setgid(a0: i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_os_setgid(a0) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.sys.setpgid
    #[no_mangle]
    pub extern "C" fn jet_std_os_setpgid(a0: i64, a1: i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_os_setpgid(a0, a1) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.sys.setpgrp
    #[no_mangle]
    pub extern "C" fn jet_std_os_setpgrp(err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_os_setpgrp() {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.sys.setpriority
    #[no_mangle]
    pub extern "C" fn jet_std_os_setpriority(a0: i64, a1: i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_os_setpriority(a0, a1) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.sys.setsid
    #[no_mangle]
    pub extern "C" fn jet_std_os_setsid(ok: *mut i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_os_setsid() {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.sys.setuid
    #[no_mangle]
    pub extern "C" fn jet_std_os_setuid(a0: i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_os_setuid(a0) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.sys.stop
    #[no_mangle]
    pub extern "C" fn jet_std_os_stop(a0: i64) {
        guard(|| crate::jet_std_os_stop(a0))
    }

    /// core.sys.sync
    #[no_mangle]
    pub extern "C" fn jet_std_os_sync() {
        guard(|| crate::jet_std_os_sync())
    }

    /// core.sys.temp_dir
    #[no_mangle]
    pub extern "C" fn jet_std_os_temp_dir() -> JetCString {
        guard(|| handle(crate::jet_std_os_temp_dir()))
    }

    /// core.sys.times
    #[no_mangle]
    pub extern "C" fn jet_std_os_times(data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_std_os_times().into_iter().map(|e| e.to_bits()).collect(), data))
    }

    /// core.sys.umask
    #[no_mangle]
    pub extern "C" fn jet_std_os_umask(a0: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_os_umask(a0)))
    }

    /// core.sys.uptime
    #[no_mangle]
    pub extern "C" fn jet_std_os_uptime() -> f64 {
        guard(|| crate::jet_std_os_uptime())
    }

    /// core.sys.username
    #[no_mangle]
    pub extern "C" fn jet_std_os_username() -> JetCString {
        guard(|| handle(crate::jet_std_os_username()))
    }

    /// core.sys.utime
    #[no_mangle]
    pub extern "C" fn jet_std_os_utime(a0: JetCString, a1: i64, a2: i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_os_utime(unsafe { &*a0 }, a1, a2) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.sys.version
    #[no_mangle]
    pub extern "C" fn jet_std_os_version() -> JetCString {
        guard(|| handle(crate::jet_std_os_version()))
    }

    /// core.sys.wait
    #[no_mangle]
    pub extern "C" fn jet_std_os_wait(ok: *mut i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_os_wait() {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.sys.waitpid
    #[no_mangle]
    pub extern "C" fn jet_std_os_waitpid(a0: i64, a1: i64, ok: *mut i64, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_std_os_waitpid(a0, a1) {
            Ok(value) => { unsafe { ok.write(crate::jet_std::jet_int_from_i64(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.process.cmd
    #[no_mangle]
    pub extern "C" fn jet_std_process_cmd(a0_0: *const u64, a0_1: i64) -> *mut crate::jet_std::ProcessSpec {
        guard(|| Box::into_raw(Box::new(crate::jet_std_process_cmd(&list_in(a0_0, a0_1, |w| view(w as JetCString).to_owned())))))
    }

    /// core.process.exit
    #[no_mangle]
    pub extern "C" fn jet_std_process_exit(a0: i64) {
        guard(|| crate::jet_std_process_exit(a0))
    }

    /// core.handle.process.spec.under
    #[no_mangle]
    pub extern "C" fn jet_std_process_spec_under(a0: *mut crate::jet_std::ProcessSpec, a1: *mut crate::JetAuthority) -> *mut crate::jet_std::ProcessSpec {
        guard(|| Box::into_raw(Box::new(crate::jet_std_process_spec_under(unsafe { &*a0 }.clone(), unsafe { &*a1 }))))
    }

    /// core.math.random
    #[no_mangle]
    pub extern "C" fn jet_std_random_float() -> f64 {
        guard(|| crate::jet_std_random_float())
    }

    /// core.math.random.getrandbits
    #[no_mangle]
    pub extern "C" fn jet_std_random_getrandbits(a0: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_std_random_getrandbits(a0)))
    }

    /// core.math.random.seed
    #[no_mangle]
    pub extern "C" fn jet_std_random_seed(a0: i64) {
        guard(|| crate::jet_std_random_seed(a0))
    }

    /// core.math.random.split
    #[no_mangle]
    pub extern "C" fn jet_std_random_split(a0: i64) -> *mut crate::jet_std::Rng {
        guard(|| Box::into_raw(Box::new(crate::jet_std_random_split(a0))))
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

    /// core.encoding.toml.parse, core.encoding.toml.load, core.encoding.toml.loads
    #[no_mangle]
    pub extern "C" fn jet_std_toml_parse(a0: JetCString, ok: *mut *mut crate::jet_std::DataTree, err: *mut *mut crate::jet_std::EncodingError) -> i64 {
        guard(|| match crate::jet_std_toml_parse(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
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

    /// core.encoding.xml.to_bytes
    #[no_mangle]
    pub extern "C" fn jet_std_xml_to_bytes(a0: *mut crate::jet_std::DataTree, a1: *mut crate::jet_std::XMLRenderOptions, ok: *mut *mut u64, ok_len: *mut i64, err: *mut *mut crate::jet_std::XMLError) -> i64 {
        guard(|| match crate::jet_std_xml_to_bytes(unsafe { &*a0 }, unsafe { &*a1 }.clone()) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| e as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.encoding.yaml.parse
    #[no_mangle]
    pub extern "C" fn jet_std_yaml_parse(a0: JetCString, ok: *mut *mut crate::jet_std::DataTree, err: *mut *mut crate::jet_std::EncodingError) -> i64 {
        guard(|| match crate::jet_std_yaml_parse(unsafe { &*a0 }) {
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

    /// core.handle.terminal.resize
    #[no_mangle]
    pub extern "C" fn jet_terminal_session_resize(a0: *mut crate::jet_std::TerminalSession, a1: *mut crate::jet_std::TerminalSize, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_terminal_session_resize(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
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

    /// core.testing.corpus
    #[no_mangle]
    pub extern "C" fn jet_testing_corpus(a0: JetCString, data: *mut *mut u64) -> i64 {
        guard(|| list_out(crate::jet_testing_corpus(unsafe { &*a0 }).into_iter().map(|e| handle(e) as u64).collect(), data))
    }

    /// core.testing.fake_data
    #[no_mangle]
    pub extern "C" fn jet_testing_fake_new(a0: i64) -> *mut crate::jet_std::Fake {
        guard(|| Box::into_raw(Box::new(crate::jet_testing_fake_new(a0))))
    }

    /// core.testing.fixture
    #[no_mangle]
    pub extern "C" fn jet_testing_fixture(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_testing_fixture(unsafe { &*a0 })))
    }

    /// core.testing.golden
    #[no_mangle]
    pub extern "C" fn jet_testing_golden(a0: JetCString, a1: JetCString) -> bool {
        guard(|| crate::jet_testing_golden(unsafe { &*a0 }, unsafe { &*a1 }))
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

    /// core.testing.snap
    #[no_mangle]
    pub extern "C" fn jet_testing_snap(a0: JetCString, a1: JetCString) -> bool {
        guard(|| crate::jet_testing_snap(unsafe { &*a0 }, unsafe { &*a1 }))
    }

    /// core.testing.status
    #[no_mangle]
    pub extern "C" fn jet_testing_status(a0: *mut crate::jet_std::JetTestComparison) -> JetCString {
        guard(|| handle(crate::jet_testing_status(unsafe { &*a0 })))
    }

    /// core.testing.temp_dir
    #[no_mangle]
    pub extern "C" fn jet_testing_temp_dir(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_testing_temp_dir(unsafe { &*a0 })))
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

    /// core.text.parse.parse_kv
    #[no_mangle]
    pub extern "C" fn jet_text_parse_kv(a0: JetCString, a1: JetCString, item0: *mut JetCString, item1: *mut bool, item2: *mut JetCString) {
        guard(|| {
            let (e0, e1, e2) = crate::jet_text_parse_kv(unsafe { &*a0 }, unsafe { &*a1 });
            unsafe { item0.write(handle(e0)) };
            unsafe { item1.write(e1) };
            unsafe { item2.write(handle(e2)) };
        })
    }

    /// core.text.parse.lstrip
    #[no_mangle]
    pub extern "C" fn jet_text_parse_lstrip(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_text_parse_lstrip(unsafe { &*a0 })))
    }

    /// core.text.parse.partition
    #[no_mangle]
    pub extern "C" fn jet_text_parse_partition(a0: JetCString, a1: JetCString, item0: *mut JetCString, item1: *mut JetCString, item2: *mut JetCString) {
        guard(|| {
            let (e0, e1, e2) = crate::jet_text_parse_partition(unsafe { &*a0 }, unsafe { &*a1 });
            unsafe { item0.write(handle(e0)) };
            unsafe { item1.write(handle(e1)) };
            unsafe { item2.write(handle(e2)) };
        })
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

    /// core.text.parse.rpartition
    #[no_mangle]
    pub extern "C" fn jet_text_parse_rpartition(a0: JetCString, a1: JetCString, item0: *mut JetCString, item1: *mut JetCString, item2: *mut JetCString) {
        guard(|| {
            let (e0, e1, e2) = crate::jet_text_parse_rpartition(unsafe { &*a0 }, unsafe { &*a1 });
            unsafe { item0.write(handle(e0)) };
            unsafe { item1.write(handle(e1)) };
            unsafe { item2.write(handle(e2)) };
        })
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

    /// core.text.parse.split_once
    #[no_mangle]
    pub extern "C" fn jet_text_parse_split_once(a0: JetCString, a1: JetCString, item0: *mut bool, item1: *mut JetCString, item2: *mut JetCString) {
        guard(|| {
            let (e0, e1, e2) = crate::jet_text_parse_split_once(unsafe { &*a0 }, unsafe { &*a1 });
            unsafe { item0.write(e0) };
            unsafe { item1.write(handle(e1)) };
            unsafe { item2.write(handle(e2)) };
        })
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

    /// core.time.from_unix_microseconds
    #[no_mangle]
    pub extern "C" fn jet_time_from_unix_microseconds(a0: i64) -> *mut crate::JetDateTime {
        guard(|| Box::into_raw(Box::new(crate::jet_time_from_unix_microseconds(a0))))
    }

    /// core.time.from_unix_ms
    #[no_mangle]
    pub extern "C" fn jet_time_from_unix_ms(a0: i64) -> *mut crate::JetDateTime {
        guard(|| Box::into_raw(Box::new(crate::jet_time_from_unix_ms(a0))))
    }

    /// core.time.from_unix_nanoseconds
    #[no_mangle]
    pub extern "C" fn jet_time_from_unix_nanoseconds(a0: i64) -> *mut crate::JetDateTime {
        guard(|| Box::into_raw(Box::new(crate::jet_time_from_unix_nanoseconds(a0))))
    }

    /// core.time.from_unix_seconds
    #[no_mangle]
    pub extern "C" fn jet_time_from_unix_seconds(a0: i64) -> *mut crate::JetDateTime {
        guard(|| Box::into_raw(Box::new(crate::jet_time_from_unix_seconds(a0))))
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

    /// core.handle.tls.config_with_alpn
    #[no_mangle]
    pub extern "C" fn jet_tls_client_config_with_alpn(a0: *mut crate::JetTLSClientConfig, a1_0: *const u64, a1_1: i64, ok: *mut *mut crate::JetTLSClientConfig, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_tls_client_config_with_alpn(unsafe { &*a0 }.clone(), &list_in(a1_0, a1_1, |w| view(w as JetCString).to_owned())) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tls.config_with_identity
    #[no_mangle]
    pub extern "C" fn jet_tls_client_config_with_client_identity(a0: *mut crate::JetTLSClientConfig, a1: *mut crate::JetTLSClientIdentity, ok: *mut *mut crate::JetTLSClientConfig, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_tls_client_config_with_client_identity(unsafe { &*a0 }.clone(), unsafe { &*a1 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tls.config_with_trust
    #[no_mangle]
    pub extern "C" fn jet_tls_client_config_with_trust(a0: *mut crate::JetTLSClientConfig, a1: *mut crate::JetTLSTrust, ok: *mut *mut crate::JetTLSClientConfig, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_tls_client_config_with_trust(unsafe { &*a0 }.clone(), unsafe { &*a1 }.clone()) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.tls.config_with_version_bounds
    #[no_mangle]
    pub extern "C" fn jet_tls_client_config_with_version_bounds(a0: *mut crate::JetTLSClientConfig, a1: i64, a2: i64, ok: *mut *mut crate::JetTLSClientConfig, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_tls_client_config_with_version_bounds(unsafe { &*a0 }.clone(), match a1 { 0 => crate::JetTLSVersion::TLS12, 1 => crate::JetTLSVersion::TLS13, _ => range_stop() }, match a2 { 0 => crate::JetTLSVersion::TLS12, 1 => crate::JetTLSVersion::TLS13, _ => range_stop() }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.tui.ascii
    #[no_mangle]
    pub extern "C" fn jet_tui_ascii(a0: JetCString) -> JetCString {
        guard(|| handle(crate::jet_tui_ascii(view(a0))))
    }

    /// core.tui.capabilities
    #[no_mangle]
    pub extern "C" fn jet_tui_capabilities() -> *mut crate::JetTuiCapabilities {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_capabilities())))
    }

    /// core.tui.close_event
    #[no_mangle]
    pub extern "C" fn jet_tui_close_event() -> *mut crate::JetTuiEvent {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_close_event())))
    }

    /// core.tui.color_ansi16
    #[no_mangle]
    pub extern "C" fn jet_tui_color_ansi16(a0: i64) -> *mut crate::JetTuiColor {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_color_ansi16(a0))))
    }

    /// core.tui.color_ansi256
    #[no_mangle]
    pub extern "C" fn jet_tui_color_ansi256(a0: i64) -> *mut crate::JetTuiColor {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_color_ansi256(a0))))
    }

    /// core.tui.color_rgb
    #[no_mangle]
    pub extern "C" fn jet_tui_color_rgb(a0: i64, a1: i64, a2: i64) -> *mut crate::JetTuiColor {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_color_rgb(a0, a1, a2))))
    }

    /// core.tui.display_width
    #[no_mangle]
    pub extern "C" fn jet_tui_display_width_int(a0: JetCString) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_tui_display_width_int(view(a0))))
    }

    /// core.tui.fill
    #[no_mangle]
    pub extern "C" fn jet_tui_fill(a0: f64) -> *mut crate::JetTuiConstraint {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_fill(a0))))
    }

    /// core.tui.focus_event
    #[no_mangle]
    pub extern "C" fn jet_tui_focus_event(a0: u8) -> *mut crate::JetTuiEvent {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_focus_event(a0 != 0))))
    }

    /// core.tui.horizontal
    #[no_mangle]
    pub extern "C" fn jet_tui_horizontal() -> *mut crate::JetTuiDirection {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_horizontal())))
    }

    /// core.tui.interrupt_event
    #[no_mangle]
    pub extern "C" fn jet_tui_interrupt_event() -> *mut crate::JetTuiEvent {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_interrupt_event())))
    }

    /// core.tui.io_event
    #[no_mangle]
    pub extern "C" fn jet_tui_io_event(a0: JetCString, a1_0: *const u64, a1_1: i64) -> *mut crate::JetTuiEvent {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_io_event(view(a0), list_in(a1_0, a1_1, |w| w as u8)))))
    }

    /// core.tui.key_event
    #[no_mangle]
    pub extern "C" fn jet_tui_key_event(a0: JetCString) -> *mut crate::JetTuiEvent {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_key_event(view(a0)))))
    }

    /// core.tui.key_event_modifiers
    #[no_mangle]
    pub extern "C" fn jet_tui_key_event_modifiers(a0: JetCString, a1: i64) -> *mut crate::JetTuiEvent {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_key_event_modifiers(view(a0), a1))))
    }

    /// core.tui.length
    #[no_mangle]
    pub extern "C" fn jet_tui_length(a0: f64) -> *mut crate::JetTuiConstraint {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_length(a0))))
    }

    /// core.tui.list
    #[no_mangle]
    pub extern "C" fn jet_tui_list(a0_0: *const u64, a0_1: i64) -> *mut crate::JetUiNode {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_list(list_in(a0_0, a0_1, |w| view(w as JetCString).to_owned())))))
    }

    /// core.tui.list_state
    #[no_mangle]
    pub extern "C" fn jet_tui_list_state() -> *mut crate::JetTuiListState {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_list_state())))
    }

    /// core.tui.list_state_offset
    #[no_mangle]
    pub extern "C" fn jet_tui_list_state_offset(a0: *mut crate::JetTuiListState, a1: i64) -> *mut crate::JetTuiListState {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_list_state_offset(unsafe { &*a0 }.clone(), a1))))
    }

    /// core.tui.list_state_select
    #[no_mangle]
    pub extern "C" fn jet_tui_list_state_select(a0: *mut crate::JetTuiListState, a1: i64) -> *mut crate::JetTuiListState {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_list_state_select(unsafe { &*a0 }.clone(), a1))))
    }

    /// core.tui.list_state_selected
    #[no_mangle]
    pub extern "C" fn jet_tui_list_state_selected(a0: *mut crate::JetTuiListState) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(crate::jet_tui_list_state_selected(unsafe { &*a0 }.clone())))
    }

    /// core.tui.max
    #[no_mangle]
    pub extern "C" fn jet_tui_max(a0: f64) -> *mut crate::JetTuiConstraint {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_max(a0))))
    }

    /// core.tui.min
    #[no_mangle]
    pub extern "C" fn jet_tui_min(a0: f64) -> *mut crate::JetTuiConstraint {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_min(a0))))
    }

    /// core.tui.percent
    #[no_mangle]
    pub extern "C" fn jet_tui_percent(a0: f64) -> *mut crate::JetTuiConstraint {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_percent(a0))))
    }

    /// core.tui.resize_event
    #[no_mangle]
    pub extern "C" fn jet_tui_resize_event(a0: f64, a1: f64) -> *mut crate::JetTuiEvent {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_resize_event(a0, a1))))
    }

    /// core.tui.style
    #[no_mangle]
    pub extern "C" fn jet_tui_style() -> *mut crate::JetTuiStyle {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_style())))
    }

    /// core.tui.style_background
    #[no_mangle]
    pub extern "C" fn jet_tui_style_background(a0: *mut crate::JetTuiStyle, a1: *mut crate::JetTuiColor) -> *mut crate::JetTuiStyle {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_style_background(unsafe { &*a0 }.clone(), unsafe { &*a1 }.clone()))))
    }

    /// core.tui.style_bold
    #[no_mangle]
    pub extern "C" fn jet_tui_style_bold(a0: *mut crate::JetTuiStyle, a1: u8) -> *mut crate::JetTuiStyle {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_style_bold(unsafe { &*a0 }.clone(), a1 != 0))))
    }

    /// core.tui.style_dim
    #[no_mangle]
    pub extern "C" fn jet_tui_style_dim(a0: *mut crate::JetTuiStyle, a1: u8) -> *mut crate::JetTuiStyle {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_style_dim(unsafe { &*a0 }.clone(), a1 != 0))))
    }

    /// core.tui.style_foreground
    #[no_mangle]
    pub extern "C" fn jet_tui_style_foreground(a0: *mut crate::JetTuiStyle, a1: *mut crate::JetTuiColor) -> *mut crate::JetTuiStyle {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_style_foreground(unsafe { &*a0 }.clone(), unsafe { &*a1 }.clone()))))
    }

    /// core.tui.style_text
    #[no_mangle]
    pub extern "C" fn jet_tui_style_text(a0: JetCString, a1: *mut crate::JetTuiStyle, a2: *mut crate::JetTuiCapabilities) -> JetCString {
        guard(|| handle(crate::jet_tui_style_text(view(a0), unsafe { &*a1 }.clone(), unsafe { &*a2 }.clone())))
    }

    /// core.tui.style_underline
    #[no_mangle]
    pub extern "C" fn jet_tui_style_underline(a0: *mut crate::JetTuiStyle, a1: u8) -> *mut crate::JetTuiStyle {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_style_underline(unsafe { &*a0 }.clone(), a1 != 0))))
    }

    /// core.tui.timer_event
    #[no_mangle]
    pub extern "C" fn jet_tui_timer_event(a0: JetCString, a1: i64) -> *mut crate::JetTuiEvent {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_timer_event(view(a0), a1))))
    }

    /// core.tui.vertical
    #[no_mangle]
    pub extern "C" fn jet_tui_vertical() -> *mut crate::JetTuiDirection {
        guard(|| Box::into_raw(Box::new(crate::jet_tui_vertical())))
    }

    /// core.ui.aria_role_button
    #[no_mangle]
    pub extern "C" fn jet_ui_aria_role_button() -> *mut crate::JetUiAriaRole {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_aria_role_button())))
    }

    /// core.ui.aria_role_container
    #[no_mangle]
    pub extern "C" fn jet_ui_aria_role_container() -> *mut crate::JetUiAriaRole {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_aria_role_container())))
    }

    /// core.ui.aria_role_label
    #[no_mangle]
    pub extern "C" fn jet_ui_aria_role_label() -> *mut crate::JetUiAriaRole {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_aria_role_label())))
    }

    /// core.ui.aria_role_text_input
    #[no_mangle]
    pub extern "C" fn jet_ui_aria_role_text_input() -> *mut crate::JetUiAriaRole {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_aria_role_text_input())))
    }

    /// core.ui.button
    #[no_mangle]
    pub extern "C" fn jet_ui_button(a0: JetCString) -> *mut crate::JetUiNode {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_button(view(a0)))))
    }

    /// core.ui.constraint
    #[no_mangle]
    pub extern "C" fn jet_ui_constraint(a0: f64, a1: f64, a2: f64, a3: f64) -> *mut crate::JetSizeConstraint {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_constraint(a0, a1, a2, a3))))
    }

    /// core.ui.desktop
    #[no_mangle]
    pub extern "C" fn jet_ui_desktop() -> *mut crate::JetUiPreviewViewport {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_desktop())))
    }

    /// core.ui.host.capabilities
    #[no_mangle]
    pub extern "C" fn jet_ui_host_capabilities() -> *mut crate::JetUiCapabilityFacts {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_host_capabilities())))
    }

    /// core.ui.host.file_filter_text
    #[no_mangle]
    pub extern "C" fn jet_ui_host_file_filter_text() -> *mut crate::JetUiFileFilter {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_host_file_filter_text())))
    }

    /// core.ui.host.fs_rights_read
    #[no_mangle]
    pub extern "C" fn jet_ui_host_fs_rights_read() -> *mut crate::JetUiFsRights {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_host_fs_rights_read())))
    }

    /// core.ui.host.fs_rights_read_write
    #[no_mangle]
    pub extern "C" fn jet_ui_host_fs_rights_read_write() -> *mut crate::JetUiFsRights {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_host_fs_rights_read_write())))
    }

    /// core.ui.host.fs_rights_write
    #[no_mangle]
    pub extern "C" fn jet_ui_host_fs_rights_write() -> *mut crate::JetUiFsRights {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_host_fs_rights_write())))
    }

    /// core.ui.host.open_request
    #[no_mangle]
    pub extern "C" fn jet_ui_host_open_request(a0: *mut crate::JetUiFsGrant) -> *mut crate::JetUiFileDialogRequest {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_host_open_request(unsafe { &*a0 }.clone()))))
    }

    /// core.ui.host.save_request
    #[no_mangle]
    pub extern "C" fn jet_ui_host_save_request(a0: *mut crate::JetUiFsGrant) -> *mut crate::JetUiFileDialogRequest {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_host_save_request(unsafe { &*a0 }.clone()))))
    }

    /// core.ui.key_event
    #[no_mangle]
    pub extern "C" fn jet_ui_key_event(a0: JetCString) -> *mut crate::JetInputEvent {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_key_event(view(a0)))))
    }

    /// core.ui.node
    #[no_mangle]
    pub extern "C" fn jet_ui_node(a0: JetCString, a1: f64, a2: f64) -> *mut crate::JetUiNode {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_node(view(a0), a1, a2))))
    }

    /// core.ui.node_accessibility
    #[no_mangle]
    pub extern "C" fn jet_ui_node_accessibility(a0: *mut crate::JetUiNode, a1: *mut crate::JetUiAccessibility) -> *mut crate::JetUiNode {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_node_accessibility(unsafe { &*a0 }.clone(), unsafe { &*a1 }.clone()))))
    }

    /// core.ui.node_color
    #[no_mangle]
    pub extern "C" fn jet_ui_node_color(a0: JetCString, a1: f64, a2: f64, a3: JetCString) -> *mut crate::JetUiNode {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_node_color(view(a0), a1, a2, view(a3)))))
    }

    /// core.ui.node_role
    #[no_mangle]
    pub extern "C" fn jet_ui_node_role(a0: JetCString, a1: f64, a2: f64, a3: i64) -> *mut crate::JetUiNode {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_node_role(view(a0), a1, a2, match a3 { 0 => crate::JetUiAriaRole::Button, 1 => crate::JetUiAriaRole::TextInput, 2 => crate::JetUiAriaRole::Label, 3 => crate::JetUiAriaRole::Container, _ => range_stop() }))))
    }

    /// core.ui.node_shortcut
    #[no_mangle]
    pub extern "C" fn jet_ui_node_shortcut(a0: *mut crate::JetUiNode, a1: *mut crate::JetUiShortcut) -> *mut crate::JetUiNode {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_node_shortcut(unsafe { &*a0 }.clone(), unsafe { &*a1 }.clone()))))
    }

    /// core.ui.null_backend
    #[no_mangle]
    pub extern "C" fn jet_ui_null() -> *mut crate::JetNullBackend {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_null())))
    }

    /// core.ui.phone
    #[no_mangle]
    pub extern "C" fn jet_ui_phone() -> *mut crate::JetUiPreviewViewport {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_phone())))
    }

    /// core.ui.point
    #[no_mangle]
    pub extern "C" fn jet_ui_point(a0: f64, a1: f64) -> *mut crate::JetPoint {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_point(a0, a1))))
    }

    /// core.ui.rect
    #[no_mangle]
    pub extern "C" fn jet_ui_rect(a0: f64, a1: f64, a2: f64, a3: f64) -> *mut crate::JetRect {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_rect(a0, a1, a2, a3))))
    }

    /// core.ui.resize_event
    #[no_mangle]
    pub extern "C" fn jet_ui_resize_event(a0: f64, a1: f64) -> *mut crate::JetInputEvent {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_resize_event(a0, a1))))
    }

    /// core.ui.size
    #[no_mangle]
    pub extern "C" fn jet_ui_size(a0: f64, a1: f64) -> *mut crate::JetSize {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_size(a0, a1))))
    }

    /// core.ui.tablet
    #[no_mangle]
    pub extern "C" fn jet_ui_tablet() -> *mut crate::JetUiPreviewViewport {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_tablet())))
    }

    /// core.ui.text
    #[no_mangle]
    pub extern "C" fn jet_ui_text(a0: JetCString) -> *mut crate::JetUiNode {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_text(view(a0)))))
    }

    /// core.ui.text_input
    #[no_mangle]
    pub extern "C" fn jet_ui_text_input(a0: JetCString, a1: i64) -> *mut crate::JetUiNode {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_text_input(view(a0), match a1 { 0 => crate::JetUiImeMode::Native, 1 => crate::JetUiImeMode::Disabled, _ => range_stop() }))))
    }

    /// core.ui.tui_backend
    #[no_mangle]
    pub extern "C" fn jet_ui_tui() -> *mut crate::JetTuiBackend {
        guard(|| Box::into_raw(Box::new(crate::jet_ui_tui())))
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

    /// core.net.url.split_fragment
    #[no_mangle]
    pub extern "C" fn jet_url_split_fragment(a0: JetCString, item0: *mut JetCString, item1: *mut JetCString) {
        guard(|| {
            let (e0, e1) = crate::jet_url_split_fragment(unsafe { &*a0 });
            unsafe { item0.write(handle(e0)) };
            unsafe { item1.write(handle(e1)) };
        })
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

    /// core.net.url.unquote_to_bytes
    #[no_mangle]
    pub extern "C" fn jet_url_unquote_to_bytes(a0: JetCString, ok: *mut *mut u64, ok_len: *mut i64, err: *mut JetCString) -> i64 {
        guard(|| match crate::jet_url_unquote_to_bytes(unsafe { &*a0 }) {
            Ok(value) => { let len = list_out(value.into_iter().map(|e| e as u64).collect(), ok); unsafe { ok_len.write(len) }; 1 }
            Err(error) => { unsafe { err.write(handle(error)) }; 0 }
        })
    }

    /// core.net.url.urldefrag
    #[no_mangle]
    pub extern "C" fn jet_url_urldefrag(a0: JetCString, item0: *mut JetCString, item1: *mut JetCString) {
        guard(|| {
            let (e0, e1) = crate::jet_url_urldefrag(unsafe { &*a0 });
            unsafe { item0.write(handle(e0)) };
            unsafe { item1.write(handle(e1)) };
        })
    }

    /// core.watcher.files
    #[no_mangle]
    pub extern "C" fn jet_watcher_files(a0: JetCString, ok: *mut *mut crate::jet_std::WatchHandle, err: *mut *mut crate::jet_std::IOError) -> i64 {
        guard(|| match crate::jet_watcher_files(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.watcher.port
    #[no_mangle]
    pub extern "C" fn jet_watcher_port(a0: JetCString, a1: i64) -> *mut crate::jet_std::WatchHandle {
        guard(|| Box::into_raw(Box::new(crate::jet_watcher_port(unsafe { &*a0 }, a1))))
    }

    /// core.watcher.process_pid
    #[no_mangle]
    pub extern "C" fn jet_watcher_process_pid(a0: i64) -> *mut crate::jet_std::WatchHandle {
        guard(|| Box::into_raw(Box::new(crate::jet_watcher_process_pid(a0))))
    }

    /// core.watcher.set
    #[no_mangle]
    pub extern "C" fn jet_watcher_set() -> *mut crate::jet_std::WatchSet {
        guard(|| Box::into_raw(Box::new(crate::jet_watcher_set())))
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

    /// core.handle.ws.close
    #[no_mangle]
    pub extern "C" fn jet_ws_close(a0: *mut crate::JetWsConn, a1: i64, a2: JetCString, err: *mut *mut crate::JetWsError) -> i64 {
        guard(|| match crate::jet_ws_close(unsafe { &*a0 }, a1, unsafe { &*a2 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.ws.connect
    #[no_mangle]
    pub extern "C" fn jet_ws_connect(a0: JetCString, ok: *mut *mut crate::JetWsConn, err: *mut *mut crate::JetWsError) -> i64 {
        guard(|| match crate::jet_ws_connect(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.ws.message_is_text
    #[no_mangle]
    pub extern "C" fn jet_ws_message_is_text(a0: *mut crate::JetWsMessage) -> bool {
        guard(|| crate::jet_ws_message_is_text(unsafe { &*a0 }))
    }

    /// core.handle.ws.message_text
    #[no_mangle]
    pub extern "C" fn jet_ws_message_text(a0: *mut crate::JetWsMessage, ok: *mut JetCString, err: *mut *mut crate::JetWsError) -> i64 {
        guard(|| match crate::jet_ws_message_text(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(handle(value)) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.ws.recv
    #[no_mangle]
    pub extern "C" fn jet_ws_recv(a0: *mut crate::JetWsConn, ok: *mut *mut crate::JetWsMessage, err: *mut *mut crate::JetWsError) -> i64 {
        guard(|| match crate::jet_ws_recv(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.handle.ws.send_text
    #[no_mangle]
    pub extern "C" fn jet_ws_send_text(a0: *mut crate::JetWsConn, a1: JetCString, err: *mut *mut crate::JetWsError) -> i64 {
        guard(|| match crate::jet_ws_send_text(unsafe { &*a0 }, unsafe { &*a1 }) {
            Ok(()) => 1,
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }

    /// core.net.ws.upgrade
    #[no_mangle]
    pub extern "C" fn jet_ws_upgrade(a0: *mut crate::JetHTTPRequest, ok: *mut *mut crate::JetWsConn, err: *mut *mut crate::JetWsError) -> i64 {
        guard(|| match crate::jet_ws_upgrade(unsafe { &*a0 }) {
            Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }
            Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }
        })
    }
}
// Skipped routes (no C carrier yet):
//   jet_app_sync (app.sync): no runtime function
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
//   jet_auth_verify_jwt_defaulted (core.auth.verify_jwt): parameter types  token: &String, key: &Vec<u8>, audience: &String, issuer: Option<&String>, clock_skew_ns: Option<i64>, 
//   jet_bag_add (core.builtin.bag_add): generic
//   jet_bag_count (core.builtin.bag_count): generic
//   jet_bag_has (core.builtin.bag_has): generic
//   jet_bag_remove (core.builtin.bag_remove): generic
//   jet_bitset_add (core.builtin.bitset_add): no runtime function
//   jet_bitset_remove (core.builtin.bitset_remove): no runtime function
//   jet_calendar_monthcalendar (core.time.calendar.monthcalendar): result type Vec<Vec<jet_foundation::Numeric::JetInt>>
//   jet_calendar_monthcalendar_start (core.time.calendar.monthcalendar_start): result type Vec<Vec<jet_foundation::Numeric::JetInt>>
//   jet_calendar_yearcalendar (core.time.calendar.yearcalendar): result type Vec<Vec<Vec<jet_foundation::Numeric::JetInt>>>
//   jet_coll_heappop (core.collections.heappop): result type ( Vec<jet_foundation::Numeric::JetInt>, JetOutcome<jet_foundation::Numeric::JetInt, JetAbsent>, )
//   jet_coll_heappushpop (core.collections.heappushpop): result type ( Vec<jet_foundation::Numeric::JetInt>, jet_foundation::Numeric::JetInt, )
//   jet_coll_heapreplace (core.collections.heapreplace): result type ( Vec<jet_foundation::Numeric::JetInt>, JetOutcome<jet_foundation::Numeric::JetInt, JetAbsent>, )
//   jet_comb_batched (core.math.combinatorics.batched): result type Vec<Vec<i64>>
//   jet_comb_cartesian (core.math.combinatorics.cartesian): result type Vec<Vec<i64>>
//   jet_comb_combinations (core.math.combinatorics.combinations): result type Vec<Vec<i64>>
//   jet_comb_combinations_with_replacement (core.math.combinatorics.combinations_with_replacement): result type Vec<Vec<i64>>
//   jet_comb_flatten (core.math.combinatorics.flatten): parameter types groups: Vec<Vec<i64>>
//   jet_comb_groupby (core.math.combinatorics.groupby): result type Vec<Vec<i64>>
//   jet_comb_pairwise (core.math.combinatorics.pairwise): result type Vec<Vec<i64>>
//   jet_comb_permutations (core.math.combinatorics.permutations): result type Vec<Vec<i64>>
//   jet_comb_powerset (core.math.combinatorics.powerset): result type Vec<Vec<i64>>
//   jet_comb_product (core.math.combinatorics.product): result type Vec<Vec<i64>>
//   jet_comb_starmap (core.math.combinatorics.starmap): parameter types rows: Vec<Vec<i64>>
//   jet_comb_tee (core.math.combinatorics.tee): result type Vec<Vec<i64>>
//   jet_comb_windows (core.math.combinatorics.windows): result type Vec<Vec<i64>>
//   jet_comb_zip_longest (core.math.combinatorics.zip_longest): result type Vec<Vec<i64>>
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
//   jet_data_arrow_import (core.data.arrow.import): generic
//   jet_data_bar_text_checked (core.data.bar_text): generic
//   jet_data_count (core.data.count): generic
//   jet_data_csv_reader (core.data.csv_reader): generic
//   jet_data_entries_to_map (core.collections.entries_to_map): generic
//   jet_data_json_decode (core.data.json): generic
//   jet_data_json_reader (core.data.json_reader): generic
//   jet_data_line_svg_plot_checked (core.data.line_svg): generic
//   jet_data_line_text_plot_checked (core.data.line_text): generic
//   jet_data_loader_bind (core.data.loader.bind): generic
//   jet_data_loader_bind_text (core.data.loader.bind_text): generic
//   jet_data_loader_cancel (core.data.loader.cancel): generic
//   jet_data_loader_database (core.data.database): generic
//   jet_data_loader_file (core.data.file): generic
//   jet_data_loader_file_member (core.data.file_member): generic
//   jet_data_loader_invalidate (core.data.loader.invalidate): generic
//   jet_data_loader_load (core.data.load): generic
//   jet_data_loader_load_default (core.data.load_default): generic
//   jet_data_loader_needs_refresh (core.data.loader.needs_refresh): generic
//   jet_data_loader_offline (core.data.loader.offline): generic
//   jet_data_loader_ready (core.data.loader.ready): generic
//   jet_data_loader_snapshot (core.data.snapshot): generic
//   jet_data_loader_source_identity (core.data.loader.source_identity): generic
//   jet_data_loader_status (core.data.loader.status): generic
//   jet_data_loader_stream (core.data.loader.stream): generic
//   jet_data_loader_url (core.data.url): generic
//   jet_data_loader_value (core.data.value): generic
//   jet_data_plot (core.data.plot.plot): generic
//   jet_data_plot_inspect (core.data.inspect): generic
//   jet_data_plot_inspect_json (core.data.inspect_json): generic
//   jet_data_plot_render (core.data.render): generic
//   jet_data_plot_show (core.data.show): generic
//   jet_data_plot_svg (core.data.svg): generic
//   jet_data_plot_text (core.data.text): generic
//   jet_data_query (core.data.query): generic
//   jet_data_query_arrow (core.data.arrow.query): generic
//   jet_data_status (core.data.status): result type Vec<jet_std::DataStatus>
//   jet_data_stream_collect (core.data.stream.collect): generic
//   jet_data_stream_next (core.data.stream.next): generic
//   jet_data_track (core.data.track): generic
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
//   jet_decimal_compare (core.precise.decimal_compare): result type __jet_Ordering
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
//   jet_devtools_publish (core.devtools.publish): generic
//   jet_duration_divide (core.handle.duration.divide): parameter types d: &jet_std::Duration, factor: &i64
//   jet_duration_round (core.handle.duration.round): parameter types  d: &jet_std::Duration, unit: &String, increment: &i64, mode: &String, 
//   jet_duration_scale (core.handle.duration.scale): parameter types d: &jet_std::Duration, factor: &i64
//   jet_email::envelope (core.email.envelope): parameter types from: &Address, recipients: &Vec<Address>
//   jet_email::Limits::safe (core.email.limits_safe): no runtime function
//   jet_email::message (core.email.message): parameter types  from: &Address, to: &Vec<Address>, bcc: &Vec<Address>, subject: &String, text: &String, html: &String, attachments: &Vec<Attachment>, 
//   jet_enc_cbor_reader (core.encoding.cbor.reader): parameter types input: JetFileReader, limits: jet_std::EncodingLimits
//   jet_enc_cbor_reader_next (core.handle.cbor_reader.next): result type Result<JetOutcome<jet_std::DataEvent, JetAbsent>, jet_std::EncodingError>
//   jet_enc_cbor_to_bytes (core.encoding.cbor.to_bytes): generic
//   jet_enc_cbor_to_bytes_canonical (core.encoding.cbor.to_bytes_canonical): generic
//   jet_enc_cbor_writer (core.encoding.cbor.writer): parameter types output: JetFileWriter, limits: jet_std::EncodingLimits
//   jet_enc_csv_decode (core.encoding.csv.decode): generic
//   jet_enc_csv_query (core.encoding.csv.query): generic
//   jet_enc_csv_reader (core.encoding.csv.reader): parameter types  input: JetFileReader, limits: jet_std::EncodingLimits, delimiter: String, header: bool, skip_blank: bool, 
//   jet_enc_csv_reader_next (core.handle.csv_reader.next): result type Result<JetOutcome<jet_std::CSVRow, JetAbsent>, jet_std::EncodingError>
//   jet_enc_csv_to_string (core.encoding.csv.to_string): generic
//   jet_enc_csv_writer (core.encoding.csv.writer): parameter types output: JetFileWriter, limits: jet_std::EncodingLimits
//   jet_enc_json_reader (core.encoding.json.reader): parameter types  input: JetFileReader, limits: jet_std::EncodingLimits, 
//   jet_enc_json_reader_next (core.handle.json_reader.next): result type Result<JetOutcome<jet_std::DataEvent, JetAbsent>, jet_std::EncodingError>
//   jet_enc_json_writer (core.encoding.json.writer): parameter types  output: JetFileWriter, limits: jet_std::EncodingLimits, canonical: bool, 
//   jet_enc_jsonl_reader (core.encoding.jsonl.reader): parameter types  input: JetFileReader, limits: jet_std::EncodingLimits, 
//   jet_enc_jsonl_reader_next (core.handle.jsonl_reader.next): result type Result<JetOutcome<jet_std::DataTree, JetAbsent>, jet_std::EncodingError>
//   jet_enc_jsonl_writer (core.encoding.jsonl.writer): parameter types  output: JetFileWriter, limits: jet_std::EncodingLimits, 
//   jet_enc_toml_to_string (core.encoding.toml.to_string): generic
//   jet_enc_xml_reader (core.encoding.xml.reader): parameter types  input: JetFileReader, limits: jet_std::EncodingLimits, xml: jet_std::XMLParseOptions, 
//   jet_enc_xml_reader_next (core.handle.xml_reader.next): result type Result<JetOutcome<jet_std::DataTree, JetAbsent>, jet_std::EncodingError>
//   jet_enc_yaml_to_string (core.encoding.yaml.to_string): generic
//   jet_expect_snapshot (core.test.expect_snapshot): no runtime function
//   jet_expiring_get (core.handle.expiring.get): generic
//   jet_ffi_callback_boundary (core.ffi.callback_boundary): generic
//   jet_ffi_callback_event_stop_unit (core.ffi.callback_event_stop): no runtime function
//   jet_fraction_from_decimal (core.precise.fraction_from_decimal): parameter types value: jet_std::JetDecimal
//   jet_fraction_from_owned_parts (core.precise.fraction_from_owned_parts): parameter types  numerator: &jet_foundation::Numeric::JetInt, denominator: &jet_foundation::Numeric::JetInt, 
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
//   jet_http_client_get (core.http.get): no runtime function
//   jet_http_client_new_impl (core.handle.http.client_new): no runtime function
//   jet_http_client_post (core.http.client.post): no runtime function
//   jet_http_mux_middleware (core.handle.http_mux.middleware): parameter types mux: &JetHTTPMux, middleware: JetHTTPMiddleware
//   jet_http_srv_json (core.http.server.json): generic
//   jet_http_srv_req_header (core.handle.http.request_header): result type JetOutcome<String, JetAbsent>
//   jet_http_srv_req_param (core.handle.http.request_param): result type JetOutcome<String, JetAbsent>
//   jet_http_srv_response (core.http.server.response): generic
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
//   jet_loadable_failed (core.reactive.loadable.failed): generic
//   jet_loadable_idle (core.reactive.loadable.idle): result type JetLoadable<(), ()>
//   jet_loadable_loaded (core.reactive.loadable.loaded): generic
//   jet_loadable_loading (core.reactive.loadable.loading): result type JetLoadable<(), ()>
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
//   jet_math_F64x4_from_array (core.math.F64x4_from_array): parameter types a: [f64; 4]
//   jet_math_F64x4_gather_lane (core.math.F64x4_gather_lane): generic
//   jet_math_F64x4_lane_const (core.math.F64x4_lane_const): generic
//   jet_math_F64x4_mul_lane_scale (core.math.F64x4_mul_lane_scale): generic
//   jet_math_F64x4_to_array (core.math.F64x4_to_array): result type [f64; 4]
//   jet_math_Mat3_from_array (core.math.Mat3_from_array): parameter types a: [f64; 9]
//   jet_math_Mat3_to_array (core.math.Mat3_to_array): result type [f64; 9]
//   jet_math_Mat4_from_array (core.math.Mat4_from_array): parameter types a: [f64; 16]
//   jet_math_Mat4_to_array (core.math.Mat4_to_array): result type [f64; 16]
//   jet_math_Vec2_from_array (core.math.Vec2_from_array): parameter types a: [f64; 2]
//   jet_math_Vec2_to_array (core.math.Vec2_to_array): result type [f64; 2]
//   jet_math_Vec3_from_array (core.math.Vec3_from_array): parameter types a: [f64; 3]
//   jet_math_Vec3_to_array (core.math.Vec3_to_array): result type [f64; 3]
//   jet_math_Vec4_from_array (core.math.Vec4_from_array): parameter types a: [f64; 4]
//   jet_math_Vec4_to_array (core.math.Vec4_to_array): result type [f64; 4]
//   jet_net_dns_a (core.net.dns_a): result type Result<Vec<JetIpAddr>, String>
//   jet_net_dns_a_at (core.net.dns_a_at): result type Result<Vec<JetIpAddr>, String>
//   jet_net_dns_aaaa (core.net.dns_aaaa): result type Result<Vec<JetIpAddr>, String>
//   jet_net_dns_aaaa_at (core.net.dns_aaaa_at): result type Result<Vec<JetIpAddr>, String>
//   jet_net_dns_srv (core.net.dns_srv): result type Result<Vec<JetDNSSrv>, String>
//   jet_net_dns_srv_at (core.net.dns_srv_at): result type Result<Vec<JetDNSSrv>, String>
//   jet_net_tcp_reply (core.net.tcp_reply): parameter types  mut stream: JetTCPStream, status: &String, body: &String, 
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
//   jet_parsed_flag (core.handle.parsed.flag): no runtime function
//   jet_parsed_option (core.handle.parsed.option): no runtime function
//   jet_parsed_option_float (core.handle.parsed.option_float): no runtime function
//   jet_parsed_option_int (core.handle.parsed.option_int): no runtime function
//   jet_parsed_options (core.handle.parsed.options): no runtime function
//   jet_parsed_positional (core.handle.parsed.positional): no runtime function
//   jet_parsed_subcommand (core.handle.parsed.subcommand): no runtime function
//   jet_path_extension (core.handle.path.extension): result type JetOutcome<String, JetAbsent>
//   jet_path_parent (core.handle.path.parent): result type JetOutcome<JetPath, JetAbsent>
//   jet_path_stem (core.handle.path.stem): result type JetOutcome<String, JetAbsent>
//   jet_path_walk (core.handle.path.walk): result type Vec<JetPath>
//   jet_priority_queue_from (core.builtin.priority_queue_from): generic
//   jet_priority_queue_peek (core.builtin.priority_queue_peek): generic
//   jet_priority_queue_pop_kernel (core.builtin.priority_queue_pop): generic
//   jet_priority_queue_remove_slot_canonical (core.builtin.priority_queue_remove_slot): no runtime function
//   jet_priority_queue_remove_value (core.builtin.priority_queue_remove_value): generic
//   jet_priority_queue_to_sorted_list (core.builtin.priority_queue_to_sorted_list): generic
//   jet_process_spec_abilities (core.handle.process.spec.abilities): result type std::collections::HashSet<String>
//   jet_process_stdin_close (core.handle.process.stdin_close): parameter types  handle: &std::rc::Rc<std::cell::RefCell<Option<jet_std::ProcessStdin>>>, 
//   jet_process_stdin_write (core.handle.process.stdin_write): platform-gated
//   jet_reader_read_f32_be (core.handle.reader.read_f32_be): result type Result<f32, String>
//   jet_reader_read_f32_le (core.handle.reader.read_f32_le): result type Result<f32, String>
//   jet_receipt_attach (receipt.attach): generic
//   jet_ring_csv_parse (core.encoding.csv.parse): result type Result<Vec<Vec<String>>, String>
//   jet_ring_log_debug_fields (core.log.debug_fields): parameter types msg: &String, fields: &Vec<jet_std::LogField>
//   jet_ring_log_error_fields (core.log.error_fields): parameter types msg: &String, fields: &Vec<jet_std::LogField>
//   jet_ring_log_info_fields (core.log.info_fields): parameter types msg: &String, fields: &Vec<jet_std::LogField>
//   jet_ring_log_warn_fields (core.log.warn_fields): parameter types msg: &String, fields: &Vec<jet_std::LogField>
//   jet_rng_pick (core.handle.rng.pick): generic
//   jet_rng_sample (core.handle.rng.sample): generic
//   jet_rng_shuffle (core.handle.rng.shuffle): generic
//   jet_rng_weighted_pick (core.handle.rng.weighted_pick): generic
//   jet_rt_callback (core.rt.callback): generic
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
//   jet_std_binary_iter_unpack (core.encoding.binary.iter_unpack): result type Result<Vec<Vec<i64>>, String>
//   jet_std_env_decode (core.sys.decode): generic
//   jet_std_file_reader_read_line (core.handle.file_reader.read_line): result type Result<Option<String>, jet_std::IOError>
//   jet_std_fs_list_dir (core.files.list_dir): result type Result<Vec<jet_std::DirEntry>, jet_std::IOError>
//   jet_std_fs_map_lines_view (core.handle.mapped_file.lines): generic
//   jet_std_fs_map_window_len_view (core.handle.mapped_file.window_len): result type Result<&[u8], jet_std::IOError>
//   jet_std_fs_map_window_view (core.handle.mapped_file.window): result type Result<&[u8], jet_std::IOError>
//   jet_std_fs_walk (core.files.walk): parameter types  path: &String, ignore_name: JetOutcome<String, JetAbsent>, 
//   jet_std_fs_walk_files (core.files.walk_files): parameter types  path: &String, ignore_name: JetOutcome<String, JetAbsent>, 
//   jet_std_fs_walk_parallel (core.files.walk_parallel): parameter types  path: &String, ignore_name: JetOutcome<String, JetAbsent>, 
//   jet_std_io_input (core.term.input): parameter types prompt: Option<&String>
//   jet_std_io_progress_iter (core.term.progress_iter): generic
//   jet_std_io_stdin_read_line (core.handle.stdin.read_line): result type Result<Option<String>, jet_std::IOError>
//   jet_std_jsonl_first (core.encoding.jsonl.first): result type Result<Option<jet_std::DataTree>, jet_std::EncodingError>
//   jet_std_jsonl_parse (core.encoding.jsonl.parse): result type Result<Vec<jet_std::DataTree>, jet_std::EncodingError>
//   jet_std_jsonl_render (core.encoding.jsonl.to_string): parameter types rows: &Vec<jet_std::DataTree>
//   jet_std_os_atexit (core.sys.atexit): generic
//   jet_std_process_pipeline (core.process.pipeline): parameter types  specs: &Vec<jet_std::ProcessSpec>, 
//   jet_std_xml_attribute (core.encoding.xml.attribute): result type Result<JetOutcome<String, JetAbsent>, jet_std::XMLError>
//   jet_std_xml_content (core.encoding.xml.content): result type Result<Vec<jet_std::DataTree>, jet_std::XMLError>
//   jet_std::after_value (core.tasks.after): generic
//   jet_std::FieldError::under (core.encoding.decode_under): no runtime function
//   jet_std::interval (core.tasks.interval): result type JetReceiver<i64>
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
//   jet_testing_compare (core.testing.compare): parameter types  cases: &Vec<jet_std::DataTree>, reference: Box<dyn Fn(jet_std::DataTree) -> jet_std::DataTree>, candidate: Box<dyn Fn(jet_std::DataTree) -> jet_std::DataTree>, relation: &String, 
//   jet_testing_histories (core.testing.histories): generic
//   jet_testing_world (core.testing.world): generic
//   jet_text_byte_views (core.text.byte_views): generic
//   jet_text_grapheme_views (core.text.grapheme_views): generic
//   jet_text_line_views (core.text.line_views): generic
//   jet_text_word_views (core.text.word_views): generic
//   jet_tls_client_identity_from_pem (core.handle.tls.client_identity_from_pem): parameter types  cert_chain: &Vec<u8>, private_key: &Vec<u8>, validate: fn(&Vec<u8>, &Vec<u8>) -> Result<(), String>, 
//   jet_tls_root_certificates_from_pem (core.handle.tls.root_certificates_from_pem): parameter types  pem: &Vec<u8>, validate: fn(&Vec<u8>) -> Result<(), String>, 
//   jet_tui_layout (core.tui.layout): parameter types  area: JetRect, direction: JetTuiDirection, constraints: Vec<JetTuiConstraint>, 
//   jet_tui_table (core.tui.table): parameter types headers: Vec<String>, rows: Vec<Vec<String>>
//   jet_ui_gtk (core.ui.gtk_backend): no runtime function
//   jet_ui_host_accessibility (core.ui.host.accessibility): result type JetUiServiceResult<JetUiAccessibility>
//   jet_ui_host_attach_accessibility (core.ui.host.accessibility.attach): result type JetUiServiceResult<JetUiNode>
//   jet_ui_host_clipboard_read_text (core.ui.host.clipboard.read_text): result type JetUiServiceResult<JetUiClipboardText>
//   jet_ui_host_clipboard_write_text (core.ui.host.clipboard.write_text): result type JetUiServiceResult<JetUiClipboardWrite>
//   jet_ui_host_drag_poll (core.ui.host.drag_drop.poll): result type JetUiServiceResult<Option<JetUiDragEvent>>
//   jet_ui_host_file_filter (core.ui.host.file_filter): result type JetUiServiceResult<JetUiFileFilter>
//   jet_ui_host_fs_grant (core.ui.host.fs_grant): result type JetUiServiceResult<JetUiFsGrant>
//   jet_ui_host_ime_poll (core.ui.host.ime.poll): result type JetUiServiceResult<Option<JetUiImeEvent>>
//   jet_ui_host_open_file (core.ui.host.open_file): result type JetUiServiceResult<JetUiFileDialogSelection>
//   jet_ui_host_project_accessibility (core.ui.host.accessibility.project): result type JetUiServiceResult<Option<JetUiAccessibilityProjection>>
//   jet_ui_host_save_file (core.ui.host.save_file): result type JetUiServiceResult<JetUiFileDialogSelection>
//   jet_ui_host_shortcut (core.ui.host.shortcut): result type JetUiServiceResult<JetUiShortcut>
//   jet_ui_host_shortcut_binding (core.ui.host.shortcuts.binding): result type JetUiServiceResult<JetUiShortcutBinding>
//   jet_ui_host_shortcuts_dispatch (core.ui.host.shortcuts.dispatch): result type JetUiServiceResult<JetUiShortcutDispatch>
//   jet_ui_host_shortcuts_register (core.ui.host.shortcuts.register): result type JetUiServiceResult<JetUiShortcutBinding>
//   jet_ui_playground_with_viewport (core.ui.playground): generic
//   jet_ui_playgrounds (core.ui.playgrounds): generic
//   jet_ui_preview_with_viewport (core.ui.preview): generic
//   jet_ui_previews (core.ui.previews): generic
//   jet_ui_reactive_render (core.ui.reactive_render): generic
//   jet_unicode_cut_last (core.builtin.string_cut_last): result type jet_foundation::Outcome::JetOutcome< (String, String), jet_foundation::Outcome::JetAbsent, >
//   jet_unicode_split_once (core.builtin.string_split_once): result type jet_foundation::Outcome::JetOutcome< (String, String), jet_foundation::Outcome::JetAbsent, >
//   jet_unicode_trim_view (core.builtin.string_trim_view): result type &str
//   jet_unit_conversion_rounded (core.units.conversion_rounded): result type Result<f64, &'static str>
//   jet_url_from_parts (core.net.url.from_parts): parameter types  scheme: &String, host: &String, path: &String, query: &Vec<Vec<String>>, fragment: &String, 
//   jet_url_parse_qsl (core.net.url.parse_qsl): result type Vec<Vec<String>>
//   jet_url_query (core.net.url.query): parameter types pairs: &Vec<Vec<String>>
//   jet_url_urlencode (core.net.url.urlencode): parameter types pairs: &Vec<Vec<String>>
//   jet_view_mut_new (core.builtin.view_mut_new): generic
//   jet_view_new (core.builtin.view_new): generic
//   jet_web_virtual_window_facts (core.web.virtual.facts_json): no runtime function
//   JetByteBuffer::capacity (core.builtin.byte_buffer_capacity): no runtime function
//   JetByteBuffer::from (core.builtin.byte_buffer_from): no runtime function
//   JetByteBuffer::to_bytes (core.builtin.byte_buffer_to_bytes): no runtime function
//   JetByteBuffer::with_capacity (core.builtin.byte_buffer_with_capacity): no runtime function
//   JetCountMinSketch::new (core.data.sketch.cms.new): no runtime function
//   JetDate::parse_iso_week_date (core.time.parse_iso_week_date): no runtime function
//   JetHyperLogLog::new (core.data.sketch.hll.new): no runtime function
//   JetLocalTime::new (core.time.time): no runtime function
//   JetReservoirSampler::new (core.data.sketch.reservoir.new): no runtime function
//   JetTDigest::new (core.data.sketch.tdigest.new): no runtime function
// END GENERATED C-ABI ROUTES
