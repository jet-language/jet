// D-TYPE2-UNCERT1=A: one Float measurement arithmetic kernel for the JS tier.
function jet_measurement_kernel_new(value, uncertainty) {
  return { value: Number(value), uncertainty: Math.abs(Number(uncertainty)) };
}

function jet_measurement_kernel_from_relative(value, relative_uncertainty) {
  return jet_measurement_kernel_new(
    value,
    Math.abs(Number(value)) * Math.abs(Number(relative_uncertainty)),
  );
}

function jet_measurement_kernel_add(left, right) {
  return jet_measurement_kernel_new(
    left.value + right.value,
    Math.sqrt(left.uncertainty * left.uncertainty + right.uncertainty * right.uncertainty),
  );
}

function jet_measurement_kernel_sub(left, right) {
  return jet_measurement_kernel_new(
    left.value - right.value,
    Math.sqrt(left.uncertainty * left.uncertainty + right.uncertainty * right.uncertainty),
  );
}

function jet_measurement_kernel_mul(left, right) {
  return jet_measurement_kernel_new(
    left.value * right.value,
    Math.sqrt(
      (right.value * left.uncertainty) ** 2
        + (left.value * right.uncertainty) ** 2,
    ),
  );
}

function jet_measurement_kernel_div(left, right) {
  return jet_measurement_kernel_new(
    left.value / right.value,
    Math.sqrt(
      (left.uncertainty / right.value) ** 2
        + ((left.value * right.uncertainty) / (right.value * right.value)) ** 2,
    ),
  );
}

function jet_measurement_kernel_sqrt(value) {
  const root = Math.sqrt(value.value);
  const uncertainty = root === 0 && value.uncertainty === 0
    ? 0
    : value.uncertainty / (2 * root);
  return jet_measurement_kernel_new(root, uncertainty);
}

function jet_measurement_kernel_show(value) {
  return `${jet_float_display(value.value)} ± ${jet_float_display(value.uncertainty)}`;
}

function jet_measurement_new(value, uncertainty) {
  return jet_measurement_kernel_new(value, uncertainty);
}

function jet_measurement_value(value) {
  return value.value;
}

function jet_measurement_uncertainty(value) {
  return value.uncertainty;
}

function jet_measurement_add(left, right) {
  return jet_measurement_kernel_add(left, right);
}

function jet_measurement_sub(left, right) {
  return jet_measurement_kernel_sub(left, right);
}

function jet_measurement_mul(left, right) {
  return jet_measurement_kernel_mul(left, right);
}

function jet_measurement_div(left, right) {
  return jet_measurement_kernel_div(left, right);
}

function jet_measurement_sqrt(value) {
  return jet_measurement_kernel_sqrt(value);
}

function jet_measurement_show(value) {
  return jet_measurement_kernel_show(value);
}
