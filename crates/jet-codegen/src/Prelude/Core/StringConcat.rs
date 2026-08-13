/// The owned text result for Jet's String `+` and `+=` operators.
pub fn jet_string_concat(left: &str, right: &str) -> String {
    let mut result = String::with_capacity(left.len() + right.len());
    result.push_str(left);
    result.push_str(right);
    result
}
