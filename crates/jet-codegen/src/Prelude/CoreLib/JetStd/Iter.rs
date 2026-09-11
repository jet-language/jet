/// Shared zip-family policy. Engines only marshal values into this policy;
/// length selection, strict mismatch, and padding indexes stay identical.
fn jet_sequence_argument_message(method: &str, value: i64) -> Option<&'static str> {
    match method {
        "take" | "skip" if value < 0 => Some("sequence count must be nonnegative"),
        "step_by" if value <= 0 => Some("step_by requires a positive step"),
        "chunks" if value <= 0 => Some("chunks requires a positive size"),
        "windows" if value <= 0 => Some("windows requires a positive size"),
        _ => None,
    }
}

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

pub fn jet_zip_strict_step<A, B>(left: Option<A>, right: Option<B>) -> Result<Option<(A, B)>, ()> {
    match (left, right) {
        (Some(left), Some(right)) => Ok(Some((left, right))),
        (None, None) => Ok(None),
        (None, Some(_)) | (Some(_), None) => Err(()),
    }
}

pub fn jet_zip_short_step<A, B>(
    left: Option<A>,
    right: impl FnOnce() -> Option<B>,
) -> Option<(A, B)> {
    match left {
        Some(left) => right().map(|right| (left, right)),
        None => None,
    }
}

pub fn jet_zip_pad_step<A: Clone, B: Clone>(
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

pub fn jet_zip_length_mismatch_message() -> &'static str {
    "zip length mismatch"
}

/// Shared predicate count kernel for eager List adapters.
fn jet_list_count_where_kernel<T, F>(xs: &[T], mut predicate: F) -> i64
where
    F: FnMut(&T) -> bool,
{
    let mut count = 0_i64;
    for item in xs {
        if predicate(item) {
            count += 1;
        }
    }
    count
}

/// Shared first-match replacement kernel for eager List adapters.
fn jet_list_update_first_kernel<T, F>(
    xs: &mut Vec<T>,
    mut predicate: F,
    replacement: T,
) -> bool
where
    F: FnMut(&T) -> bool,
{
    for index in 0..xs.len() {
        if predicate(&xs[index]) {
            xs[index] = replacement;
            return true;
        }
    }
    false
}

/// Error-propagating form used by the reference evaluator's callback ABI.
fn jet_list_count_where_result_kernel<T, E, F>(
    xs: &[T],
    mut predicate: F,
) -> Result<i64, E>
where
    F: FnMut(&T) -> Result<bool, E>,
{
    let mut count = 0_i64;
    for item in xs {
        if predicate(item)? {
            count += 1;
        }
    }
    Ok(count)
}

/// Error-propagating first-match replacement kernel for the reference
/// evaluator. Replacement is moved only after a successful predicate.
fn jet_list_update_first_result_kernel<T, E, F>(
    xs: &mut Vec<T>,
    mut predicate: F,
    replacement: T,
) -> Result<bool, E>
where
    F: FnMut(&T) -> Result<bool, E>,
{
    for index in 0..xs.len() {
        if predicate(&xs[index])? {
            xs[index] = replacement;
            return Ok(true);
        }
    }
    Ok(false)
}
