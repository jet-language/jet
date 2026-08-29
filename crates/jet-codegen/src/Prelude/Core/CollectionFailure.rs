// D-FAIL-IMPLICIT: one failure-aware traversal kernel for collection
// callbacks. Native emit, the interpreter, and the resident JIT host include
// this source; their adapters only choose the input representation.

pub(crate) fn jet_collection_try_map<T, U, E, I, F>(items: I, f: F) -> Result<Vec<U>, E>
where
    I: IntoIterator<Item = T>,
    F: FnMut(T) -> Result<U, E>,
{
    items.into_iter().map(f).collect()
}

pub(crate) fn jet_collection_try_filter<T, E, I, F>(items: I, mut f: F) -> Result<Vec<T>, E>
where
    I: IntoIterator<Item = T>,
    F: FnMut(&T) -> Result<bool, E>,
{
    let mut out = Vec::new();
    for item in items {
        if f(&item)? {
            out.push(item);
        }
    }
    Ok(out)
}
