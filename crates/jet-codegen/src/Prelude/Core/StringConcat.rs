/// The shared owned-text kernel for Jet's String `+` and `+=` operators.
///
/// The macro form lets AOT put this already-authorized kernel in a hot loop
/// without a call boundary. Resident tiers call the function below, which
/// expands the same source kernel; neither tier owns a second concatenation
/// rule.
#[macro_export]
macro_rules! jet_string_concat_hot {
    ($left:expr, $right:expr) => {{
        let __jet_left = $left;
        let __jet_right = $right;
        let mut __jet_result = String::with_capacity(__jet_left.len() + __jet_right.len());
        __jet_result.push_str(__jet_left);
        __jet_result.push_str(__jet_right);
        __jet_result
    }};
}

#[inline(always)]
pub fn jet_string_concat(left: &str, right: &str) -> String {
    jet_string_concat_hot!(left, right)
}
