/// D-FIXARR1 / I9: checked fixed-list indexing for unproven indexes.
///
/// The getter is an adapter. Dynamic bounds selection and error wording live
/// here; sema-proven fixed-list indexes use direct engine reads instead.
#[derive(Clone, Copy, Debug)]
pub struct JetFixedListIndexError {
    pub index: i64,
    pub len: usize,
}

impl JetFixedListIndexError {
    pub fn message(self) -> String {
        jet_list_bounds_message(self.len, self.index)
    }
}

pub fn jet_fixed_list_index<T, F>(len: usize, index: i64, get: F) -> Result<T, JetFixedListIndexError>
where
    F: FnOnce(usize) -> T,
{
    if index < 0 || (index as u128) >= len as u128 {
        return Err(JetFixedListIndexError { index, len });
    }
    Ok(get(index as usize))
}
