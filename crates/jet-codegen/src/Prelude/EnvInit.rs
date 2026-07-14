// D-ENV-MUTATE1=A: every generated executable snapshots the host environment
// before user code. Accessors live in the opt-in Core prelude; this mandatory
// foundation is emitted with every entrypoint, including test harnesses.
type JetEnvEntries = Vec<(std::ffi::OsString, std::ffi::OsString)>;

fn jet_env_table() -> &'static std::sync::RwLock<JetEnvEntries> {
    static TABLE: std::sync::OnceLock<std::sync::RwLock<JetEnvEntries>> =
        std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut entries: JetEnvEntries = Vec::new();
        for (name, value) in std::env::vars_os() {
            if let Some(old) = entries
                .iter()
                .position(|(candidate, _)| jet_env_key_eq(candidate.as_os_str(), name.as_os_str()))
            {
                entries.remove(old);
            }
            entries.push((name, value));
        }
        std::sync::RwLock::new(entries)
    })
}

fn jet_std_env_init() {
    let _ = jet_env_table();
    jet_observe_runtime_start();
}

fn jet_env_read() -> std::sync::RwLockReadGuard<'static, JetEnvEntries> {
    jet_env_table().read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn jet_env_write() -> std::sync::RwLockWriteGuard<'static, JetEnvEntries> {
    jet_env_table().write().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(unix)]
fn jet_env_key_cmp(left: &std::ffi::OsStr, right: &std::ffi::OsStr) -> std::cmp::Ordering {
    use std::os::unix::ffi::OsStrExt;
    left.as_bytes().cmp(right.as_bytes())
}

// JET_VETTED_UNSAFE_BEGIN: jet_env_windows
#[cfg(windows)]
fn jet_env_key_cmp(left: &std::ffi::OsStr, right: &std::ffi::OsStr) -> std::cmp::Ordering {
    use std::os::windows::ffi::OsStrExt;
    extern "system" {
        fn CompareStringOrdinal(left: *const u16, left_len: i32, right: *const u16, right_len: i32, ignore_case: i32) -> i32;
    }
    let left: Vec<u16> = left.encode_wide().collect();
    let right: Vec<u16> = right.encode_wide().collect();
    let (Ok(left_len), Ok(right_len)) = (i32::try_from(left.len()), i32::try_from(right.len())) else { return left.cmp(&right); };
    let result = unsafe { CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) };
    match result { 1 => std::cmp::Ordering::Less, 2 => std::cmp::Ordering::Equal, 3 => std::cmp::Ordering::Greater, _ => left.cmp(&right) }
}
// JET_VETTED_UNSAFE_END: jet_env_windows

#[cfg(not(any(unix, windows)))]
fn jet_env_key_cmp(left: &std::ffi::OsStr, right: &std::ffi::OsStr) -> std::cmp::Ordering {
    left.cmp(right)
}

fn jet_env_key_eq(left: &std::ffi::OsStr, right: &std::ffi::OsStr) -> bool {
    jet_env_key_cmp(left, right) == std::cmp::Ordering::Equal
}
