// D-MEM-COPYSEM1=A: shared read-view materialization adapter for the web tier.
function jet_view_copy(view) {
  return view.slice();
}

function jet_string_view_copy(view) {
  return String(view);
}
