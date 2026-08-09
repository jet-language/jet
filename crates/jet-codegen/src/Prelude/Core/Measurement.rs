// D-HONESTNUM1=A: one Float measurement arithmetic kernel for every tier.

pub(crate) fn jet_measurement_kernel_new(value: f64, uncertainty: f64) -> (f64, f64) {
    (value, uncertainty)
}

pub(crate) fn jet_measurement_kernel_add(left: (f64, f64), right: (f64, f64)) -> (f64, f64) {
    (
        left.0 + right.0,
        (left.1 * left.1 + right.1 * right.1).sqrt(),
    )
}

pub(crate) fn jet_measurement_kernel_sub(left: (f64, f64), right: (f64, f64)) -> (f64, f64) {
    (
        left.0 - right.0,
        (left.1 * left.1 + right.1 * right.1).sqrt(),
    )
}

pub(crate) fn jet_measurement_kernel_mul(left: (f64, f64), right: (f64, f64)) -> (f64, f64) {
    (
        left.0 * right.0,
        ((right.0 * left.1).powi(2) + (left.0 * right.1).powi(2)).sqrt(),
    )
}

pub(crate) fn jet_measurement_kernel_div(left: (f64, f64), right: (f64, f64)) -> (f64, f64) {
    (
        left.0 / right.0,
        ((left.1 / right.0).powi(2) + (left.0 * right.1 / (right.0 * right.0)).powi(2))
            .sqrt(),
    )
}

pub(crate) fn jet_measurement_kernel_show(value: (f64, f64)) -> String {
    format!("{:?} ± {:?}", value.0, value.1)
}
