/// Shared directory-queue kernel for filesystem walks.
///
/// Consumers provide only entry and error carriers. Traversal policy stays
/// here so AOT, JIT, and interpreter adapters cannot drift.
pub(crate) fn jet_fs_walk_parallel<T, E, MakeEntry, MakeError>(
    path: &str,
    shown: &str,
    make_entry: MakeEntry,
    make_error: MakeError,
) -> Result<Vec<T>, E>
where
    T: Send + 'static,
    E: Send + 'static,
    MakeEntry: Fn(String, String, bool, i64) -> T + Send + Sync + 'static,
    MakeError: Fn(&str, std::io::Error) -> E + Send + Sync + 'static,
{
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::{Arc, Condvar, Mutex};

    struct QueueState<E> {
        directories: VecDeque<(PathBuf, i64)>,
        active_workers: usize,
        error: Option<E>,
    }

    let root = PathBuf::from(path);
    let state = Arc::new((
        Mutex::new(QueueState {
            directories: VecDeque::from([(root.clone(), 0)]),
            active_workers: 0,
            error: None,
        }),
        Condvar::new(),
    ));
    let sink = Arc::new(Mutex::new(Vec::new()));
    let shown = Arc::new(shown.to_string());
    let make_entry = Arc::new(make_entry);
    let make_error = Arc::new(make_error);
    let workers = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1);
    let mut handles = Vec::with_capacity(workers);

    for _ in 0..workers {
        let state = Arc::clone(&state);
        let sink = Arc::clone(&sink);
        let shown = Arc::clone(&shown);
        let make_entry = Arc::clone(&make_entry);
        let make_error = Arc::clone(&make_error);
        let root = root.clone();
        handles.push(std::thread::spawn(move || loop {
            let (dir, depth) = {
                let (queue, wake) = &*state;
                let mut queue = queue
                    .lock()
                    .unwrap_or_else(|_| panic!("filesystem walk queue poisoned"));
                loop {
                    if queue.error.is_some() {
                        return;
                    }
                    if let Some(task) = queue.directories.pop_front() {
                        queue.active_workers += 1;
                        break task;
                    }
                    if queue.active_workers == 0 {
                        wake.notify_all();
                        return;
                    }
                    queue = wake
                        .wait(queue)
                        .unwrap_or_else(|_| panic!("filesystem walk queue poisoned"));
                }
            };

            let result = (|| {
                let mut entries = Vec::new();
                for entry in std::fs::read_dir(&dir).map_err(|error| make_error(&shown, error))? {
                    entries.push(entry.map_err(|error| make_error(&shown, error))?);
                }
                let mut batch = Vec::with_capacity(entries.len());
                let mut children = Vec::new();
                for entry in entries {
                    let child = entry.path();
                    let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
                    let relative = child
                        .strip_prefix(&root)
                        .unwrap_or(&child)
                        .to_string_lossy()
                        .to_string();
                    batch.push(make_entry(
                        child.to_string_lossy().to_string(),
                        relative,
                        is_dir,
                        depth,
                    ));
                    if is_dir {
                        children.push((child, depth + 1));
                    }
                }
                Ok((batch, children))
            })();

            let (queue, wake) = &*state;
            let mut queue = queue
                .lock()
                .unwrap_or_else(|_| panic!("filesystem walk queue poisoned"));
            queue.active_workers -= 1;
            match result {
                Ok((batch, children)) => {
                    queue.directories.extend(children);
                    drop(queue);
                    sink.lock()
                        .unwrap_or_else(|_| panic!("filesystem walk sink poisoned"))
                        .extend(batch);
                }
                Err(error) => queue.error = Some(error),
            }
            wake.notify_all();
        }));
    }

    for handle in handles {
        if handle.join().is_err() {
            let (queue, _) = &*state;
            let mut queue = queue
                .lock()
                .unwrap_or_else(|_| panic!("filesystem walk queue poisoned"));
            if queue.error.is_none() {
                queue.error = Some(make_error(
                    &shown,
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "parallel walk worker panicked",
                    ),
                ));
            }
        }
    }

    let (queue, _) = &*state;
    let mut queue = queue
        .lock()
        .unwrap_or_else(|_| panic!("filesystem walk queue poisoned"));
    if let Some(error) = queue.error.take() {
        return Err(error);
    }
    let sink = match Arc::try_unwrap(sink) {
        Ok(sink) => sink,
        Err(_) => panic!("filesystem walk sink still referenced"),
    };
    Ok(sink
        .into_inner()
        .unwrap_or_else(|_| panic!("filesystem walk sink poisoned")))
}
