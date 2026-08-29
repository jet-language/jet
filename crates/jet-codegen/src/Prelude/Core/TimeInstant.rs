// D-TIME-INSTANT-SPLIT1=A: Instant is an optional Core time value. Keep its
// carrier and primitive operations in one shared Prelude fragment so AOT and
// JIT do not maintain separate time semantics across the cache boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct JetInstant {
    start_ns: i64,
}

impl JetInstant {
    pub(crate) fn now() -> Self {
        JetInstant {
            start_ns: jet_time_monotonic_now_ns(),
        }
    }

    pub(crate) fn elapsed_millis(&self) -> i64 {
        self.elapsed_nanos().saturating_div(1_000_000)
    }

    pub(crate) fn elapsed_nanos(&self) -> i64 {
        jet_time_instant_elapsed_ns(jet_time_monotonic_now_ns(), self.start_ns)
    }

    pub(crate) fn plus_duration_ns(&self, ns: i64) -> Self {
        Self {
            start_ns: jet_time_instant_add_duration_ns(self.start_ns, ns),
        }
    }

    pub(crate) fn minus_duration_ns(&self, ns: i64) -> Self {
        Self {
            start_ns: jet_time_instant_sub_duration_ns(self.start_ns, ns),
        }
    }

    pub(crate) fn difference_ns(&self, other: &Self) -> i64 {
        jet_time_instant_difference_ns(self.start_ns, other.start_ns)
    }

    pub(crate) fn compare_to(&self, other: &Self) -> i64 {
        jet_time_instant_compare(self.start_ns, other.start_ns)
    }

    pub(crate) fn to_string_fmt(&self) -> String {
        "Instant".to_string()
    }
}
