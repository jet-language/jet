//! M5: list/map host shims for the Cranelift JIT (`JetArena` handles).

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.

mod set_semantics {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
}

mod range_semantics {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    use jet_foundation::StructuralDebug::jet_debug_range;
    include!("../../jet-codegen/src/Prelude/Core/RangeBounds.rs");
}

pub(crate) mod byte_buffer_semantics {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/Core/Bytes.rs");
}

mod disjoint_semantics {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/Core/Disjoint.rs");

    pub(super) fn split(len: usize, mid: i64) -> Result<((usize, usize), (usize, usize)), String> {
        jet_disjoint_split_bounds(len, mid)
    }

    pub(super) fn indexes(
        len: usize,
        indices: &[i64],
    ) -> Result<Vec<(usize, usize, usize)>, String> {
        jet_disjoint_index_bounds(len, indices)
    }
}

pub(crate) mod authority_semantics {
    include!("../../jet-codegen/src/Prelude/Core/Authority.rs");
}

mod float_ordering {
    include!("../../jet-codegen/src/Prelude/Core/FloatOrdering.rs");
}

mod process_args_kernel {
}

// The resident collection ABI owns only handle conversion.  The operation
// itself must stay in the same Prelude source embedded by AOT and evaluated by
// tier 0 (I9).  This small module supplies the Prelude's sibling types that
// are normally present in the flat emitted program, then exposes Vec-shaped
// adapters for JIT handles.
#[allow(non_camel_case_types, unused_imports)]
pub(crate) mod collection_semantics {
    use crate::fault_injection::jet_fault_should_fail_allocation;
    use crate::Encoding::json_rt::jet_int_to_string;
    use jet_foundation::Numeric::JetInt;
    use super::float_ordering::jet_float_sort_cmp;
    pub use jet_foundation::Outcome::*;

    use jet_foundation::StructuralDebug::jet_debug_map;
    use jet_foundation::StructuralDebug::jet_debug_optional;
    use jet_foundation::StructuralDebug::jet_debug_range;

    // These are emitted beside the Prelude in an AOT program. Keep the same
    // bridge and text-value fragments here so collection adapters do not grow
    // a second Display/Debug implementation.
    trait __jet_Display {
        fn display(&self) -> String;
    }
    trait __jet_Equatable: Sized {
        fn equal(&self, rhs: &Self) -> bool;
    }

    #[derive(Clone)]
    struct JetByteBuffer {
        bytes: Vec<u8>
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(crate) struct JetMap<K, V>(std::sync::Arc<std::collections::BTreeMap<K, V>>);

    impl<K, V> JetMap<K, V> {
        fn new() -> Self {
            Self(std::sync::Arc::new(std::collections::BTreeMap::new()))
        }
    }

    impl<K: Ord, V> std::iter::FromIterator<(K, V)> for JetMap<K, V> {
        fn from_iter<I: IntoIterator<Item = (K, V)>>(pairs: I) -> Self {
            Self(std::sync::Arc::new(pairs.into_iter().collect()))
        }
    }

    impl<K, V> std::ops::Deref for JetMap<K, V> {
        type Target = std::collections::BTreeMap<K, V>;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl<K: Ord + Clone, V: Clone> std::ops::DerefMut for JetMap<K, V> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            std::sync::Arc::make_mut(&mut self.0)
        }
    }

    /// By-value take of the map entries (`Prelude/Core.rs` spelling): the
    /// sole owner moves the storage out, a shared handle clones the entries.
    fn jet_map_into_entries<K: Ord + Clone, V: Clone>(m: JetMap<K, V>) -> Vec<(K, V)> {
        match std::sync::Arc::try_unwrap(m.0) {
            Ok(map) => map.into_iter().collect(),
            Err(shared) => shared
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        }
    }

    fn jet_panic(_file: &str, _line: u32, message: &str) -> ! {
        panic!("{message}");
    }

    fn jet_panic_rich_code(
        _code: &'static str,
        _file: &str,
        _line: u32,
        _fn_name: &str,
        _source_line: &str,
        _col: u32,
        _caret_len: u32,
        message: &str,
        _locals: &str,
    ) -> ! {
        panic!("{message}");
    }

    // Share the Prelude bounds kernel and the comptime stop transport.
    include!("../../jet-codegen/src/Prelude/Core/FixedList.rs");
    fn jet_arithmetic_stop(file: &str, line: u32, message: &str) -> ! {
        jet_panic_rich_code("E3010", file, line, "", "", 0, 0, message, "")
    }

    include!("../../jet-codegen/src/Prelude/Core/Loadable.rs");
    include!("../../jet-codegen/src/Prelude/Core/Values.rs");
    // D-DATAFRAME1 / card #2447: the JIT uses the shared typed table-plan
    // kernel; this module only converts resident handles at its ABI boundary.
    include!("../../jet-codegen/src/Prelude/Core/LazyTablePlan.rs");
    use jet_foundation::StructuralDebug::jet_text_list;
    include!("../../jet-codegen/src/Prelude/Core/TextValues.rs");
    include!("../../jet-codegen/src/Prelude/Core/RangeBounds.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/Iter.rs");
    include!("../../jet-codegen/src/Prelude/Memo.rs");
    include!("../../jet-codegen/src/Prelude/Core/CollectionFailure.rs");
    // The shared acceleration plan carries the D-ACCEL1/D-PARA-GATE1
    // constants and retained-first-chunk decision. These small host shims
    // supply the AOT failure/receipt siblings that the flat Prelude expects.
    mod jet_std {
        pub(crate) use crate::Encoding::json_rt::{render_dyn_json, Dyn};
    }
    struct JetParaFailure {
        index: usize,
        payload: Box<dyn std::any::Any + Send + 'static>
    }
    fn jet_para_call<R, F>(index: usize, f: F) -> Result<R, JetParaFailure>
    where
        F: FnOnce() -> R,
    {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
            .map_err(|payload| JetParaFailure { index, payload })
    }
    fn jet_para_raise_failure(failure: JetParaFailure) -> ! {
        std::panic::resume_unwind(failure.payload)
    }
    use crate::Receipt::kernel::jet_receipt_attach_encoded;
    include!("../../jet-codegen/src/Prelude/Core/ParallelPlan.rs");
    include!("../../jet-codegen/src/Prelude/Core/ParallelKernel.rs");
    include!("../../jet-codegen/src/Prelude/Core/SimdLanes.rs");
    include!("../../jet-codegen/src/Prelude/Core/SortKernel.rs");
    // The shared collection surfaces name the same vetted storage as AOT.
    mod jet_mem {
        include!("../../jet-codegen/src/Prelude/InlineList.rs");
    }
    include!("../../jet-codegen/src/Prelude/Core/Collections.rs");

    fn display<T: JetDisplay>(value: &T) -> String {
        value.jet_display()
    }

    fn debug<T: JetDebug>(value: &T) -> String {
        value.jet_debug()
    }

}

pub(crate) enum JetLoopItem {
    Int(i64),
    FloatBits(i64)
}

/// The one pairing `.unzip()` still has on both sides of a representation
/// boundary: `jit_zip_set_value` writes a column word, `jit_zip_record_field`
/// reads it back out of the row, and `jit_zip_read_value` reads it back out of
/// the republished column. Total over `JitZipValueKind`, so it is checkable as
/// a set — unlike the `TBuiltinOp` lowering halves, which need a live
/// `FunctionBuilder` (#2091).
#[cfg(test)]
mod unzip_column_round_trip {

}

thread_local! {
}

