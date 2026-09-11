/// Shared directory-queue kernel for filesystem walks.
///
/// Consumers provide only entry and error carriers. Traversal policy stays
/// here so AOT, JIT, and interpreter adapters cannot drift.
fn jet_fs_validate_walk_root(path: &std::path::Path) -> std::io::Result<()> {
    let source = if path.as_os_str().is_empty() {
        std::path::Path::new(".")
    } else {
        path
    };
    for ancestor in source
        .ancestors()
        .filter(|ancestor| !ancestor.as_os_str().is_empty())
    {
        let metadata = std::fs::symlink_metadata(ancestor)?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("filesystem walk path contains symlink: {}", ancestor.display()),
            ));
        }
    }
    let metadata = std::fs::symlink_metadata(source)?;
    if !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotADirectory,
            format!("filesystem walk root is not a directory: {}", source.display()),
        ));
    }
    Ok(())
}

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
    jet_fs_walk_parallel_filtered(path, shown, make_entry, make_error, |_, _| true)
}

/// The same walk policy with an entry filter. The traversal still visits every
/// real directory when no ignore selector is supplied. With a selector, ignored
/// directories are pruned before they can be yielded or traversed.
pub(crate) fn jet_fs_walk_parallel_filtered<T, E, MakeEntry, MakeError, Keep>(
    path: &str,
    shown: &str,
    make_entry: MakeEntry,
    make_error: MakeError,
    keep: Keep,
) -> Result<Vec<T>, E>
where
    T: Send + 'static,
    E: Send + 'static,
    MakeEntry: Fn(String, String, bool, i64) -> T + Send + Sync + 'static,
    MakeError: Fn(&str, std::io::Error) -> E + Send + Sync + 'static,
    Keep: Fn(bool, bool) -> bool + Send + Sync + 'static,
{
    jet_fs_walk_parallel_filtered_with_ignore(
        path,
        shown,
        None,
        make_entry,
        make_error,
        move |_, is_dir, is_file| keep(is_dir, is_file),
    )
}

/// Shared walk policy with one optional ignore-file selector. The selector is
/// currently the typed `.gitignore` row; keeping it as a filename here lets the
/// traversal kernel stay independent of the surface enum and its marshalling.
pub(crate) fn jet_fs_walk_parallel_with_ignore<T, E, MakeEntry, MakeError>(
    path: &str,
    shown: &str,
    ignore_name: Option<&str>,
    make_entry: MakeEntry,
    make_error: MakeError,
) -> Result<Vec<T>, E>
where
    T: Send + 'static,
    E: Send + 'static,
    MakeEntry: Fn(String, String, bool, i64) -> T + Send + Sync + 'static,
    MakeError: Fn(&str, std::io::Error) -> E + Send + Sync + 'static,
{
    jet_fs_walk_parallel_filtered_with_ignore(
        path,
        shown,
        ignore_name,
        make_entry,
        make_error,
        |_, _, _| true,
    )
}

pub(crate) fn jet_fs_walk_parallel_filtered_with_ignore<
    T,
    E,
    MakeEntry,
    MakeError,
    Keep,
>(
    path: &str,
    shown: &str,
    ignore_name: Option<&str>,
    make_entry: MakeEntry,
    make_error: MakeError,
    keep: Keep,
) -> Result<Vec<T>, E>
where
    T: Send + 'static,
    E: Send + 'static,
    MakeEntry: Fn(String, String, bool, i64) -> T + Send + Sync + 'static,
    MakeError: Fn(&str, std::io::Error) -> E + Send + Sync + 'static,
    Keep: Fn(&str, bool, bool) -> bool + Send + Sync + 'static,
{
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::{Arc, Condvar, Mutex};

    struct QueueState<E> {
        directories: VecDeque<(PathBuf, i64, Option<JetFsIgnoreMatcher>)>,
        active_workers: usize,
        error: Option<E>,
        error_path: Option<PathBuf>,
    }

    let root = PathBuf::from(path);
    if let Err(error) = jet_fs_validate_walk_root(&root) {
        return Err(make_error(shown, error));
    }
    let ignore_name = ignore_name.map(str::to_owned);
    let initial_matcher = ignore_name
        .as_ref()
        .map(|_| JetFsIgnoreMatcher::new(&root));
    let state = Arc::new((
        Mutex::new(QueueState {
            directories: VecDeque::from([(root.clone(), 0, initial_matcher)]),
            active_workers: 0,
            error: None,
            error_path: None,
        }),
        Condvar::new(),
    ));
    let sink = Arc::new(Mutex::new(Vec::new()));
    let shown = Arc::new(shown.to_string());
    let make_entry = Arc::new(make_entry);
    let make_error = Arc::new(make_error);
    let keep = Arc::new(keep);
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
        let keep = Arc::clone(&keep);
        let ignore_name = ignore_name.clone();
        let root = root.clone();
        handles.push(std::thread::spawn(move || loop {
            let (dir, depth, inherited_matcher) = {
                let (queue, wake) = &*state;
                let mut queue = queue
                    .lock()
                    .unwrap_or_else(|_| panic!("filesystem walk queue poisoned"));
                loop {
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
                jet_fs_validate_walk_root(&dir)
                    .map_err(|error| make_error(&shown, error))?;
                let matcher = if let Some(filename) = ignore_name.as_deref() {
                    let inherited_matcher = inherited_matcher
                        .expect("ignore selector always carries a matcher");
                    let error_path = dir.to_string_lossy().into_owned();
                    Some(
                        inherited_matcher
                            .with_directory_rules(&dir, filename)
                            .map_err(|error| make_error(&error_path, error))?,
                    )
                } else {
                    None
                };
                let mut batch = Vec::with_capacity(64);
                let mut children = Vec::new();
                for entry in std::fs::read_dir(&dir)
                    .map_err(|error| make_error(&shown, error))?
                {
                    let entry = entry.map_err(|error| make_error(&shown, error))?;
                    let child = entry.path();
                    let file_type = entry.file_type();
                    let is_dir = file_type.as_ref().is_ok_and(std::fs::FileType::is_dir);
                    let is_file = file_type.as_ref().is_ok_and(std::fs::FileType::is_file);
                    let relative = child
                        .strip_prefix(&root)
                        .unwrap_or(&child)
                        .to_string_lossy()
                        .to_string();
                    let is_ignore_file = ignore_name.as_deref().is_some_and(|filename| {
                        entry.file_name() == std::ffi::OsStr::new(filename)
                    });
                    let ignored = matcher.as_ref().is_some_and(|matcher| {
                        is_ignore_file || matcher.is_ignored(&relative, is_dir)
                    });
                    if !ignored && keep(&relative, is_dir, is_file) {
                        batch.push(make_entry(
                            child.to_string_lossy().to_string(),
                            relative,
                            is_dir,
                            depth,
                        ));
                    }
                    if is_dir && !ignored {
                        children.push((child, depth + 1, matcher.clone()));
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
                Err(error) => {
                    let is_earlier = queue.error_path.as_ref().map_or(true, |current| {
                        dir.as_path() < current.as_path()
                    });
                    if is_earlier {
                        queue.error_path = Some(dir.clone());
                        queue.error = Some(error);
                    }
                }
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
                queue.error_path = Some(PathBuf::new());
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

pub(crate) fn jet_fs_walk_files_parallel<T, E, MakeEntry, MakeError>(
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
    jet_fs_walk_parallel_filtered(
        path,
        shown,
        make_entry,
        make_error,
        |is_dir, is_file| !is_dir && is_file,
    )
}

pub(crate) fn jet_fs_walk_files_parallel_with_ignore<T, E, MakeEntry, MakeError>(
    path: &str,
    shown: &str,
    ignore_name: Option<&str>,
    make_entry: MakeEntry,
    make_error: MakeError,
) -> Result<Vec<T>, E>
where
    T: Send + 'static,
    E: Send + 'static,
    MakeEntry: Fn(String, String, bool, i64) -> T + Send + Sync + 'static,
    MakeError: Fn(&str, std::io::Error) -> E + Send + Sync + 'static,
{
    jet_fs_walk_parallel_filtered_with_ignore(
        path,
        shown,
        ignore_name,
        make_entry,
        make_error,
        |_, is_dir, is_file| !is_dir && is_file,
    )
}
