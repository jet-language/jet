// D-MEM-COPYSEM1=A: shared read-view materialization adapter for native and wasm.
fn jet_view_copy<T: Clone>(view: &[T]) -> Vec<T> {
    view.to_vec()
}

fn jet_string_view_copy(view: &str) -> String {
    view.to_string()
}
