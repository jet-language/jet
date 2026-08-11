/// D-FIXARR1 / I9: checked fixed-list indexing shared by every tier.
///
/// The getter is an adapter. Bounds selection and the error wording live here;
/// an execution tier supplies only its storage read after the check succeeds.
#[derive(Clone, Copy, Debug)]
pub struct JetFixedListIndexError {
    pub index: i64,
    pub len: usize,
}

impl JetFixedListIndexError {
    pub fn message(self) -> String {
        format!(
            "the list has {} items, so position {} doesn't exist",
            self.len, self.index
        )
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
