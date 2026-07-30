// D-MEMDISJOINT1=A: canonical runtime proof for mutable partitions.
// AOT, Cranelift hosts, and TIR evaluation call these checks before creating
// any mutable view. Bounds are half-open after normalization.

fn jet_disjoint_split_bounds(
    len: usize,
    mid: i64,
) -> Result<((usize, usize), (usize, usize)), String> {
    let len_i64 = len as i64;
    if mid < 0 || mid > len_i64 {
        return Err(format!(
            "split point {} is outside 0..={} for {} items",
            mid, len_i64, len_i64
        ));
    }
    let mid = mid as usize;
    Ok(((0, mid), (mid, len)))
}

fn jet_disjoint_index_bounds(
    len: usize,
    indices: &[i64],
) -> Result<Vec<(usize, usize, usize)>, String> {
    let len_i64 = len as i64;
    let mut ordered = Vec::with_capacity(indices.len());
    for (position, &index) in indices.iter().enumerate() {
        if index < 0 || index >= len_i64 {
            return Err(format!(
                "index {} is outside 0..{} for {} items",
                index, len_i64, len_i64
            ));
        }
        ordered.push((index as usize, index as usize + 1, position));
    }
    ordered.sort_by_key(|&(start, end, _)| (start, end));
    if let Some(pair) = ordered.windows(2).find(|pair| pair[0].1 > pair[1].0) {
        return Err(format!("duplicate index {} overlaps itself", pair[1].0));
    }
    Ok(ordered)
}
