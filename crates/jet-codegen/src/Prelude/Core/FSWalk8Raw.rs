// OWNER #32 admitted native path/matcher identities for the Jet walk queue.
// No traversal or queue policy lives here. Replaced by L7; dies at cutover.
struct JetWalk8Slot {
    path: std::path::PathBuf,
    matcher: Option<JetFsIgnoreMatcher>,
    directory: Option<std::fs::ReadDir>,
}
struct JetWalk8Context {
    root: std::path::PathBuf,
    ignore: Option<String>,
    slots: Vec<JetWalk8Slot>,
}
fn jet_walk8_table() -> &'static std::sync::Mutex<std::collections::HashMap<i64, std::sync::Arc<std::sync::Mutex<JetWalk8Context>>>> {
    static TABLE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<i64, std::sync::Arc<std::sync::Mutex<JetWalk8Context>>>>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}
fn jet_walk8_context(key: i64) -> std::sync::Arc<std::sync::Mutex<JetWalk8Context>> {
    jet_walk8_table().lock().expect("walk context table poisoned").get(&key).expect("live walk context").clone()
}
fn jet_walk8_open(root: &String, ignore: &String, enabled: bool) -> i64 {
    static NEXT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);
    let key = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::path::PathBuf::from(root);
    let context = JetWalk8Context { root: path.clone(), ignore: enabled.then(|| ignore.clone()), slots: vec![JetWalk8Slot { path, matcher: None, directory: None }] };
    jet_walk8_table().lock().expect("walk context table poisoned").insert(key, std::sync::Arc::new(std::sync::Mutex::new(context)));
    key
}
fn jet_walk8_close(key: i64) {
    jet_walk8_table().lock().expect("walk context table poisoned").remove(&key);
}
fn jet_walk8_error(error: std::io::Error) -> (i64, Option<i64>, Option<String>) {
    let kind = match error.kind() {
        std::io::ErrorKind::InvalidInput | std::io::ErrorKind::InvalidData => 1,
        std::io::ErrorKind::NotFound => 2,
        std::io::ErrorKind::PermissionDenied => 3,
        std::io::ErrorKind::TimedOut => 4,
        std::io::ErrorKind::NotConnected | std::io::ErrorKind::BrokenPipe => 5,
        _ => 0,
    };
    (kind, error.raw_os_error().map(i64::from), Some(error.to_string()))
}
fn jet_walk8_validation_path(key: i64, slot: i64) -> i64 {
    let context = jet_walk8_context(key);
    let mut context = context.lock().expect("walk context poisoned");
    if !context.slots[slot as usize].path.as_os_str().is_empty() { return slot; }
    let index = context.slots.len();
    context.slots.push(JetWalk8Slot { path: std::path::PathBuf::from("."), matcher: None, directory: None });
    index as i64
}
fn jet_walk8_ancestors(key: i64, slot: i64) -> Vec<i64> {
    let context = jet_walk8_context(key);
    let mut context = context.lock().expect("walk context poisoned");
    let paths: Vec<_> = context.slots[slot as usize].path.ancestors().filter(|path| !path.as_os_str().is_empty()).map(std::path::Path::to_path_buf).collect();
    let mut keys = Vec::with_capacity(paths.len());
    for path in paths {
        keys.push(context.slots.len() as i64);
        context.slots.push(JetWalk8Slot { path, matcher: None, directory: None });
    }
    keys
}
fn jet_walk8_metadata(key: i64, slot: i64) -> (bool, bool, i64, Option<i64>, Option<String>) {
    let context = jet_walk8_context(key);
    let context = context.lock().expect("walk context poisoned");
    match std::fs::symlink_metadata(&context.slots[slot as usize].path) {
        Ok(metadata) => (metadata.is_dir(), metadata.file_type().is_symlink(), 0, None, None),
        Err(error) => { let (kind, code, cause) = jet_walk8_error(error); (false, false, kind, code, cause) }
    }
}
fn jet_walk8_shown(key: i64, slot: i64) -> String {
    let context = jet_walk8_context(key);
    let context = context.lock().expect("walk context poisoned");
    context.slots[slot as usize].path.to_string_lossy().into_owned()
}
fn jet_walk8_before(key: i64, left: i64, right: i64) -> bool {
    let context = jet_walk8_context(key);
    let context = context.lock().expect("walk context poisoned");
    context.slots[left as usize].path < context.slots[right as usize].path
}
fn jet_walk8_matcher_new(key: i64) {
    let context = jet_walk8_context(key);
    let mut context = context.lock().expect("walk context poisoned");
    context.slots[0].matcher = Some(JetFsIgnoreMatcher::new(&context.root));
}
fn jet_walk8_rules(key: i64, slot: i64, filename: &String) -> (i64, Option<i64>, Option<String>) {
    let context = jet_walk8_context(key);
    let mut context = context.lock().expect("walk context poisoned");
    let entry = &mut context.slots[slot as usize];
    match entry.matcher.as_ref().expect("inherited ignore matcher").with_directory_rules(&entry.path, filename) {
        Ok(matcher) => { entry.matcher = Some(matcher); (0, None, None) },
        Err(error) => jet_walk8_error(error),
    }
}
fn jet_walk8_ignored(key: i64, slot: i64, relative: &String, directory: bool) -> bool {
    let context = jet_walk8_context(key);
    let context = context.lock().expect("walk context poisoned");
    context.slots[slot as usize].matcher.as_ref().expect("inherited ignore matcher").is_ignored(relative, directory)
}
fn jet_walk8_inherit(key: i64, child: i64, parent: i64) {
    let context = jet_walk8_context(key);
    let mut context = context.lock().expect("walk context poisoned");
    context.slots[child as usize].matcher = context.slots[parent as usize].matcher.clone();
}
fn jet_walk8_read_dir(key: i64, slot: i64) -> (i64, Option<i64>, Option<String>) {
    let context = jet_walk8_context(key);
    let mut context = context.lock().expect("walk context poisoned");
    let entry = &mut context.slots[slot as usize];
    match jet_fs_read_directory_raw(&entry.path) {
        Ok(directory) => { entry.directory = Some(directory); (0, None, None) },
        Err(error) => jet_walk8_error(error),
    }
}
fn jet_walk8_next(key: i64, slot: i64) -> (bool, i64, bool, bool, String, String, bool, i64, Option<i64>, Option<String>) {
    let context = jet_walk8_context(key);
    let mut context = context.lock().expect("walk context poisoned");
    let entry = match context.slots[slot as usize].directory.as_mut().expect("open walk directory").next() {
        None => { context.slots[slot as usize].directory = None; return (true, 0, false, false, String::new(), String::new(), false, 0, None, None); },
        Some(Err(error)) => { let (kind, code, cause) = jet_walk8_error(error); return (false, 0, false, false, String::new(), String::new(), false, kind, code, cause); },
        Some(Ok(entry)) => entry,
    };
    let child = entry.path();
    let file_type = jet_fs_entry_type_raw(&entry);
    let directory = file_type.as_ref().is_ok_and(std::fs::FileType::is_dir);
    let file = file_type.as_ref().is_ok_and(std::fs::FileType::is_file);
    let shown = child.to_string_lossy().into_owned();
    let relative = child.strip_prefix(&context.root).unwrap_or(&child).to_string_lossy().into_owned();
    let ignore_file = context.ignore.as_deref().is_some_and(|filename| entry.file_name() == std::ffi::OsStr::new(filename));
    let index = context.slots.len() as i64;
    context.slots.push(JetWalk8Slot { path: child, matcher: None, directory: None });
    (false, index, directory, file, shown, relative, ignore_file, 0, None, None)
}
