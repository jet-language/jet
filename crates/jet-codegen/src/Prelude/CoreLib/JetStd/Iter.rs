/// Shared zip-family policy. Engines only marshal values into this policy;
/// length selection, strict mismatch, and padding indexes stay identical.
fn jet_zip_row_count(lengths: &[usize], mode: u8) -> Option<usize> {
    if lengths.is_empty() {
        return Some(0);
    }
    if mode == 1 && lengths.iter().any(|length| *length != lengths[0]) {
        return None;
    }
    Some(match mode {
        2 => lengths.iter().copied().max().unwrap_or(0),
        _ => lengths.iter().copied().min().unwrap_or(0),
    })
}

fn jet_zip_column_index(row: usize, length: usize) -> Option<usize> {
    (row < length).then_some(row)
}

fn jet_zip_fill_at<T: Clone>(
    fill_mode: u8,
    common_fills: &[T],
    column_fills: &[T],
    default: T,
    column: usize,
) -> T {
    match fill_mode {
        1 => common_fills.get(column).cloned().unwrap_or(default),
        2 => column_fills.get(column).cloned().unwrap_or(default),
        _ => default,
    }
}

fn jet_zip_rows<T: Clone, Read, Fill>(
    lengths: &[usize],
    mode: u8,
    mut read: Read,
    mut fill: Fill,
) -> Option<Vec<Vec<T>>>
where
    Read: FnMut(usize, usize) -> Option<T>,
    Fill: FnMut(usize) -> T,
{
    let row_count = jet_zip_row_count(lengths, mode)?;
    Some(
        (0..row_count)
            .map(|row| {
                lengths
                    .iter()
                    .enumerate()
                    .map(|(column, length)| {
                        jet_zip_column_index(row, *length)
                            .and_then(|index| read(column, index))
                            .unwrap_or_else(|| fill(column))
                    })
                    .collect()
            })
            .collect(),
    )
}

fn jet_zip_strict_step<A, B>(left: Option<A>, right: Option<B>) -> Result<Option<(A, B)>, ()> {
    match (left, right) {
        (Some(left), Some(right)) => Ok(Some((left, right))),
        (None, None) => Ok(None),
        (None, Some(_)) | (Some(_), None) => Err(()),
    }
}

fn jet_zip_pad_step<A: Clone, B: Clone>(
    left: Option<A>,
    right: Option<B>,
    left_fill: A,
    right_fill: B,
) -> Option<(A, B)> {
    match (left, right) {
        (Some(left), Some(right)) => Some((left, right)),
        (Some(left), None) => Some((left, right_fill)),
        (None, Some(right)) => Some((left_fill, right)),
        (None, None) => None,
    }
}
