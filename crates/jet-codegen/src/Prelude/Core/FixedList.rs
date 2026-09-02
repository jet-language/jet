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

#[inline(always)]
pub fn jet_fixed_list_index<T, F>(len: usize, index: i64, get: F) -> Result<T, JetFixedListIndexError>
where
    F: FnOnce(usize) -> T,
{
    // Convert once on the native index width. The old u128 comparison forced
    // every hot read through a widened integer path; `try_from` preserves the
    // negative and usize-width rejection cases without a second conversion.
    let raw_index = index;
    let Ok(index) = usize::try_from(raw_index) else {
        return Err(JetFixedListIndexError {
            index: raw_index,
            len,
        });
    };
    if index >= len {
        return Err(JetFixedListIndexError {
            index: raw_index,
            len,
        });
    }
    Ok(get(index))
}

#[cfg(test)]
mod fixed_list_index_tests {
    use super::jet_fixed_list_index;

    #[test]
    fn checked_index_preserves_native_boundaries() {
        let in_range = jet_fixed_list_index(3, 2, |index| index).expect("last slot is valid");
        assert_eq!(in_range, 2);

        for index in [-1, 3, i64::MAX] {
            let error = jet_fixed_list_index(3, index, |value| value).expect_err("index must stop");
            assert_eq!(error.index, index);
            assert_eq!(error.len, 3);
        }
    }
}
