/// D-HOLE1 / I9: one option-lift operation for every execution tier.
///
/// The carrier is an adapter. It lets the TIR evaluator and the resident JIT
/// pass their own representations through this same operation while the
/// emitted native and wasm programs use `JetOutcome` directly. The callable
/// factory is deliberately called only after both carriers report presence.
pub trait JetOptionValue {
    type Item;

    fn jet_option_is_present(&self) -> bool;
    fn jet_option_into_item(self) -> Self::Item;
}

impl<T> JetOptionValue for JetOutcome<T, JetAbsent> {
    type Item = T;

    fn jet_option_is_present(&self) -> bool {
        self.is_ok()
    }

    fn jet_option_into_item(self) -> Self::Item {
        match self {
            Ok(value) => value,
            Err(JetAbsent) => unreachable!("option payload requested from absence"),
        }
    }
}

/// A scalar/handle carrier used by execution adapters whose option payload is
/// already packed into one machine word. The adapter supplies the payload
/// bits; this type does not decide presence or invoke the callable.
#[derive(Clone, Copy)]
pub struct JetOptionPacked<T> {
    pub present: bool,
    pub value: T,
}

impl<T> JetOptionValue for JetOptionPacked<T> {
    type Item = T;

    fn jet_option_is_present(&self) -> bool {
        self.present
    }

    fn jet_option_into_item(self) -> Self::Item {
        debug_assert!(self.present, "option payload requested from absence");
        self.value
    }
}

/// Pack a scalar/handle option result into the resident word ABI.
///
/// The execution adapter owns only representation. Presence selection and
/// lazy callable evaluation remain in `jet_option_lift2`; every packed host
/// boundary uses this helper for the same `None = 0`, `Some(value) = value + 1`
/// representation.
pub fn jet_option_pack_i64(present: bool, value: i64) -> i64 {
    if present {
        value.wrapping_add(1)
    } else {
        0
    }
}

pub fn jet_option_lift2<A, B, F, R, O, Absent, Present, MakeF>(
    a: A,
    b: B,
    absent: Absent,
    present: Present,
    make_f: MakeF,
) -> O
where
    A: JetOptionValue,
    B: JetOptionValue,
    F: FnOnce(A::Item, B::Item) -> R,
    Absent: FnOnce() -> O,
    Present: FnOnce(R) -> O,
    MakeF: FnOnce() -> F,
{
    if !a.jet_option_is_present() || !b.jet_option_is_present() {
        return absent();
    }
    let f = make_f();
    let left = a.jet_option_into_item();
    let right = b.jet_option_into_item();
    present(f(left, right))
}
