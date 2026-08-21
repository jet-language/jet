/// D-BENCH-KEEP1=A: the one measurement sink. It preserves the value while
/// preventing an optimizing build from erasing the measured work.
pub fn jet_keep<T>(value: T) -> T {
    std::hint::black_box(value)
}
