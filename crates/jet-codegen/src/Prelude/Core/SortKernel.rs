pub(crate) fn jet_list_try_sort_by_key_kernel<T, K, E, F, C>(
    xs: &mut [T],
    mut key: F,
    mut compare: C,
) -> Result<(), E>
where
    F: FnMut(&T) -> Result<K, E>,
    C: FnMut(&K, &K) -> std::cmp::Ordering,
{
    // Evaluate each key once; a failed key leaves the receiver untouched.
    let mut keyed = xs
        .iter()
        .enumerate()
        .map(|(index, item)| key(item).map(|key| (index, key)))
        .collect::<Result<Vec<_>, _>>()?;
    keyed.sort_by(|left, right| compare(&left.1, &right.1));

    // Follow previously applied swaps instead of copying the receiver.
    for target in 0..keyed.len() {
        let mut source = keyed[target].0;
        while source < target {
            source = keyed[source].0;
        }
        keyed[target].0 = source;
        if source != target {
            xs.swap(target, source);
        }
    }
    Ok(())
}

pub(crate) fn jet_list_sort_by_compare_kernel<T, F>(xs: &mut [T], compare: F)
where
    F: FnMut(&T, &T) -> std::cmp::Ordering,
{
    xs.sort_by(compare);
}

pub(crate) fn jet_list_try_partition_kernel<T, E, F, I>(
    xs: I,
    mut predicate: F,
) -> Result<(Vec<T>, Vec<T>), E>
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T) -> Result<bool, E>,
{
    let mut yes = Vec::new();
    let mut no = Vec::new();
    for item in xs {
        if predicate(&item)? {
            yes.push(item);
        } else {
            no.push(item);
        }
    }
    Ok((yes, no))
}
