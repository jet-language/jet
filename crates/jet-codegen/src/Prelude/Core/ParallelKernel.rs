// D-PARCAPTURE1=D: the indexed chunk scheduler is shared by AOT and resident
// adapters. The AOT wrapper adds its failure rail; adapters only marshal the
// callback and values into this kernel.
const JET_PARA_CHUNK_ITEMS: usize = 64;

#[inline]
pub(crate) fn jet_list_para_chunks_serial_kernel<R, E, F>(
    len: usize,
    mut f: F,
) -> Vec<(usize, Result<R, E>)>
where
    F: FnMut(std::ops::Range<usize>) -> Result<R, E>,
{
    let chunk_count = len.div_ceil(JET_PARA_CHUNK_ITEMS);
    (0..chunk_count)
        .map(|chunk| {
            let start = chunk * JET_PARA_CHUNK_ITEMS;
            let end = (start + JET_PARA_CHUNK_ITEMS).min(len);
            (chunk, f(start..end))
        })
        .collect()
}

#[inline]
pub(crate) fn jet_list_para_merge_tree<T, E, F>(
    mut partials: Vec<T>,
    mut merge: F,
) -> Result<T, E>
where
    F: FnMut(T, T) -> Result<T, E>,
{
    if partials.is_empty() {
        panic!("parallel merge tree requires at least one partial");
    }
    while partials.len() > 1 {
        let mut next = Vec::with_capacity(partials.len().div_ceil(2));
        let mut iter = partials.into_iter();
        while let Some(left) = iter.next() {
            match iter.next() {
                Some(right) => next.push(merge(left, right)?),
                None => next.push(left),
            }
        }
        partials = next;
    }
    Ok(partials.pop().expect("non-empty para-merge lost its result"))
}

fn jet_list_para_chunks_kernel<R, E, F>(
    len: usize,
    worker_limit: usize,
    worker_cap: usize,
    f: F,
) -> Vec<(usize, Result<R, E>)>
where
    R: Send,
    E: Send,
    F: Fn(std::ops::Range<usize>) -> Result<R, E> + Sync,
{
    let chunk_count = len.div_ceil(JET_PARA_CHUNK_ITEMS);
    if chunk_count == 0 {
        return Vec::new();
    }
    let worker_count = worker_cap.min(worker_limit.max(1)).min(chunk_count);
    if worker_count == 1 {
        return jet_list_para_chunks_serial_kernel(len, f);
    }
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(worker_count);
        let f = &f;
        for worker in 0..worker_count {
            handles.push(scope.spawn(move || {
                let mut out = Vec::new();
                for chunk in (worker..chunk_count).step_by(worker_count) {
                    let start = chunk * JET_PARA_CHUNK_ITEMS;
                    let end = (start + JET_PARA_CHUNK_ITEMS).min(len);
                    out.push((chunk, f(start..end)));
                }
                out
            }));
        }
        let mut indexed = Vec::with_capacity(chunk_count);
        for handle in handles.into_iter().rev() {
            match handle.join() {
                Ok(results) => indexed.extend(results),
                Err(payload) => std::panic::resume_unwind(payload),
            }
        }
        indexed.sort_unstable_by_key(|(chunk, _)| *chunk);
        indexed
    })
}


/// D-FRED1=A: para-fold addition keeps the established 64-item stable chunks.
/// Each partial starts with the checked seed, uses the fixed eight-lane tree,
/// and final partial merges use the same fixed tree without adding a seed.
#[inline(always)]
fn jet_list_para_fold_add_fixed<T>(xs: Vec<T>, seed: T) -> T
where
    T: JetSimdScalar + Send + Sync,
{
    let worker_cap = std::thread::available_parallelism()
        .map(|workers| workers.get())
        .unwrap_or(1);
    let partials = jet_list_para_chunks_kernel(xs.len(), usize::MAX, worker_cap, |range| {
        let partial = jet_simd_reduce_fixed_iter(xs[range].iter().copied(), seed);
        Ok::<_, ()>(partial)
    });
    let partials = partials
        .into_iter()
        .map(|(_, partial)| partial.expect("fixed para-fold partial cannot fail"))
        .collect::<Vec<_>>();
    if partials.is_empty() {
        return seed;
    }
    jet_list_para_merge_tree(partials, |left, right| {
        let pair = [left, right];
        Ok::<_, ()>(jet_simd_reduce_fixed_iter(
            pair.into_iter(),
            T::simd_zero(),
        ))
    })
    .expect("non-empty fixed para-fold lost its result")
}

#[inline(always)]
pub(crate) fn jet_list_para_fold_add_fixed_f32(xs: Vec<f32>, seed: f32) -> f32 {
    jet_list_para_fold_add_fixed(xs, seed)
}

#[inline(always)]
pub(crate) fn jet_list_para_fold_add_fixed_f64(xs: Vec<f64>, seed: f64) -> f64 {
    jet_list_para_fold_add_fixed(xs, seed)
}
