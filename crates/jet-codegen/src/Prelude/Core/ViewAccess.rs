// D-VIEWACCESS1=A / I9: one checked element-access policy for every borrowed
// view. Tensor-specific window addressing remains in Compute.rs; this part
// owns the final logical index and get/set operation for both AOT and ambient.

fn jet_view_address(len: usize, index: i64) -> Result<usize, String> {
    let len = i64::try_from(len).unwrap_or(i64::MAX);
    if index < 0 || index >= len {
        return Err(format!(
            "the list has {} items, so position {} doesn't exist",
            len, index
        ));
    }
    Ok(index as usize)
}

fn jet_view_get_checked<T: Clone>(view: &[T], index: i64) -> Result<T, String> {
    let index = jet_view_address(view.len(), index)?;
    view.get(index)
        .cloned()
        .ok_or_else(|| "view index is outside storage".to_string())
}

fn jet_view_set_checked<T>(view: &mut [T], index: i64, value: T) -> Result<(), String> {
    let index = jet_view_address(view.len(), index)?;
    let Some(slot) = view.get_mut(index) else {
        return Err("view index is outside storage".to_string());
    };
    *slot = value;
    Ok(())
}

fn jet_view_set_f64_checked(view: &mut [f64], index: i64, value: f64) -> Result<(), String> {
    if !value.is_finite() {
        return Err("Tensor values must be finite".to_string());
    }
    jet_view_set_checked(view, index, value)
}

fn jet_view_get<T: Clone>(view: &[T], index: i64, file: &str, line: u32) -> T {
    match jet_view_get_checked(view, index) {
        Ok(value) => value,
        Err(error) => jet_panic(file, line, &error),
    }
}

fn jet_view_set<T>(view: &mut [T], index: i64, value: T, file: &str, line: u32) {
    if let Err(error) = jet_view_set_checked(view, index, value) {
        jet_panic(file, line, &error);
    }
}

fn jet_view_set_f64(view: &mut [f64], index: i64, value: f64, file: &str, line: u32) {
    if let Err(error) = jet_view_set_f64_checked(view, index, value) {
        jet_panic(file, line, &error);
    }
}

/// One shared setter seam for borrowed float storage and Tensor storage.
/// Compute.rs supplies the Tensor implementation; this always-present part
/// supplies the ordinary float-view implementation so the symbol remains
/// available even when a program does not import core.compute.
trait JetComputeSetTarget {
    type Error;

    fn jet_compute_set_target(
        &mut self,
        indices: &[i64],
        value: f64,
    ) -> Result<(), Self::Error>;
}

impl JetComputeSetTarget for [f64] {
    type Error = String;

    fn jet_compute_set_target(
        &mut self,
        indices: &[i64],
        value: f64,
    ) -> Result<(), Self::Error> {
        let [index] = indices else {
            return Err("a float view set expects one index".to_string());
        };
        jet_view_set_f64_checked(self, *index, value)
    }
}

fn jet_compute_set<T: JetComputeSetTarget + ?Sized>(
    target: &mut T,
    indices: &[i64],
    value: f64,
) -> Result<(), T::Error> {
    target.jet_compute_set_target(indices, value)
}
