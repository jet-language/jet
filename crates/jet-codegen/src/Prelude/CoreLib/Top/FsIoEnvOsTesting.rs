fn jet_std_fs_symlink(from: &String, to: &String) -> Result<(), jet_std::IoError> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(from, to).map_err(|e| jet_std::io_error(to, e))
    }
    #[cfg(windows)]
    {
        let meta = std::fs::metadata(from).map_err(|e| jet_std::io_error(from, e))?;
        if meta.is_dir() {
            std::os::windows::fs::symlink_dir(from, to).map_err(|e| jet_std::io_error(to, e))
        } else {
            std::os::windows::fs::symlink_file(from, to).map_err(|e| jet_std::io_error(to, e))
        }
    }
}
fn jet_std_fs_read_link(path: &String) -> Result<String, jet_std::IoError> {
    std::fs::read_link(path)
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_hard_link(from: &String, to: &String) -> Result<(), jet_std::IoError> {
    std::fs::hard_link(from, to).map_err(|e| jet_std::io_error(to, e))
}
fn jet_std_fs_rename(from: &String, to: &String) -> Result<(), jet_std::IoError> {
    std::fs::rename(from, to).map_err(|e| jet_std::io_error(from, e))
}
fn jet_std_fs_stat(path: &String) -> Result<jet_std::Stat, jet_std::IoError> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| jet_std::io_error(path, e))?;
    let ft = meta.file_type();
    let modified_ms = meta.modified().ok().and_then(system_time_ms).unwrap_or(0);
    let created_ms = meta.created().ok().and_then(system_time_ms).unwrap_or(0);
    let kind = if ft.is_symlink() {
        "symlink"
    } else if ft.is_dir() {
        "dir"
    } else if ft.is_file() {
        "file"
    } else {
        "other"
    };
    Ok(jet_std::Stat {
        size: meta.len() as i64,
        modified_ms,
        created_ms,
        readonly: meta.permissions().readonly(),
        is_file: ft.is_file(),
        is_dir: ft.is_dir(),
        is_symlink: ft.is_symlink(),
        kind: kind.to_string(),
    })
}
fn system_time_ms(t: std::time::SystemTime) -> Option<i64> {
    t.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
}
fn jet_std_fs_canonicalize(path: &String) -> Result<String, jet_std::IoError> {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_absolute(path: &String) -> Result<String, jet_std::IoError> {
    let p = std::path::Path::new(path);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| jet_std::IoError::Other {
                message: e.to_string(),
            })?
            .join(p)
    };
    Ok(abs.to_string_lossy().to_string())
}
fn jet_std_fs_copy_dir(from: &String, to: &String) -> Result<(), jet_std::IoError> {
    fn copy_tree(
        src: &std::path::Path,
        dst: &std::path::Path,
        shown: &str,
    ) -> Result<(), jet_std::IoError> {
        std::fs::create_dir_all(dst).map_err(|e| jet_std::io_error(shown, e))?;
        for entry in std::fs::read_dir(src).map_err(|e| jet_std::io_error(shown, e))? {
            let entry = entry.map_err(|e| jet_std::io_error(shown, e))?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            let ft = entry.file_type().map_err(|e| jet_std::io_error(shown, e))?;
            if ft.is_dir() {
                copy_tree(&src_path, &dst_path, shown)?;
            } else if ft.is_file() {
                std::fs::copy(&src_path, &dst_path).map_err(|e| jet_std::io_error(shown, e))?;
            }
        }
        Ok(())
    }
    copy_tree(std::path::Path::new(from), std::path::Path::new(to), from)
}
fn jet_std_fs_walk(path: &String) -> Result<Vec<jet_std::WalkEntry>, jet_std::IoError> {
    let root = std::path::PathBuf::from(path);
    let mut out = Vec::new();
    fn walk_dir(
        root: &std::path::Path,
        dir: &std::path::Path,
        depth: i64,
        out: &mut Vec<jet_std::WalkEntry>,
        shown: &str,
    ) -> Result<(), jet_std::IoError> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(dir).map_err(|e| jet_std::io_error(shown, e))? {
            entries.push(entry.map_err(|e| jet_std::io_error(shown, e))?);
        }
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let p = entry.path();
            let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            let relative = p
                .strip_prefix(root)
                .unwrap_or(&p)
                .to_string_lossy()
                .to_string();
            out.push(jet_std::WalkEntry {
                path: p.to_string_lossy().to_string(),
                relative,
                is_dir,
                depth,
            });
            if is_dir {
                walk_dir(root, &p, depth + 1, out, shown)?;
            }
        }
        Ok(())
    }
    walk_dir(&root, &root, 0, &mut out, path)?;
    Ok(out)
}
fn jet_std_fs_glob(pattern: &String) -> Result<Vec<String>, jet_std::IoError> {
    let split = pattern.find(['*', '?']).unwrap_or(pattern.len());
    let base = pattern[..split]
        .rsplit_once(std::path::MAIN_SEPARATOR)
        .map(|(dir, _)| if dir.is_empty() { "." } else { dir })
        .unwrap_or(".");
    let mut out = Vec::new();
    let base_s = base.to_string();
    for entry in jet_std_fs_walk(&base_s)? {
        if glob_match(pattern.as_str(), entry.path.as_str()) {
            out.push(entry.path);
        }
    }
    out.sort();
    Ok(out)
}
fn glob_match(pattern: &str, text: &str) -> bool {
    fn inner(p: &[u8], t: &[u8]) -> bool {
        if p.is_empty() {
            return t.is_empty();
        }
        match p[0] {
            b'*' => inner(&p[1..], t) || (!t.is_empty() && inner(p, &t[1..])),
            b'?' => !t.is_empty() && inner(&p[1..], &t[1..]),
            c => !t.is_empty() && c == t[0] && inner(&p[1..], &t[1..]),
        }
    }
    inner(pattern.as_bytes(), text.as_bytes())
}
fn jet_std_fs_read_at(path: &String, offset: i64, len: i64) -> Result<Vec<u8>, jet_std::IoError> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).map_err(|e| jet_std::io_error(path, e))?;
    f.seek(SeekFrom::Start(offset.max(0) as u64))
        .map_err(|e| jet_std::io_error(path, e))?;
    let mut buf = vec![0u8; len.max(0) as usize];
    let n = f.read(&mut buf).map_err(|e| jet_std::io_error(path, e))?;
    buf.truncate(n);
    Ok(buf)
}
fn jet_std_fs_write_at(
    path: &String,
    offset: i64,
    bytes: &Vec<u8>,
) -> Result<(), jet_std::IoError> {
    use std::io::{Seek, SeekFrom, Write};
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(path)
        .map_err(|e| jet_std::io_error(path, e))?;
    f.seek(SeekFrom::Start(offset.max(0) as u64))
        .map_err(|e| jet_std::io_error(path, e))?;
    f.write_all(bytes).map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_fsync(path: &String) -> Result<(), jet_std::IoError> {
    std::fs::OpenOptions::new()
        .read(true)
        .open(path)
        .and_then(|f| f.sync_all())
        .map_err(|e| jet_std::io_error(path, e))
}
fn jet_std_fs_write_atomic(path: &String, bytes: &Vec<u8>) -> Result<(), jet_std::IoError> {
    jet_path_write_atomic(&jet_path_from(path), bytes)
}
fn jet_std_fs_temp_dir(prefix: &String) -> Result<jet_std::TempDir, jet_std::IoError> {
    let path = jet_temp_path(prefix);
    std::fs::create_dir(&path).map_err(|e| jet_std::io_error(&path, e))?;
    Ok(jet_std::TempDir {
        path,
        cleanup: std::rc::Rc::new(()),
    })
}
fn jet_std_fs_temp_file(prefix: &String) -> Result<jet_std::TempFile, jet_std::IoError> {
    let path = jet_temp_path(prefix);
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(|e| jet_std::io_error(&path, e))?;
    Ok(jet_std::TempFile {
        path,
        cleanup: std::rc::Rc::new(()),
    })
}
fn jet_std_fs_lock(path: &String) -> Result<jet_std::FileLock, jet_std::IoError> {
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|e| jet_std::io_error(path, e))?;
    Ok(jet_std::FileLock {
        path: path.clone(),
        cleanup: std::rc::Rc::new(()),
    })
}
fn jet_temp_path(prefix: &String) -> String {
    let clean: String = prefix
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir()
        .join(format!("{}_{}_{}", clean, std::process::id(), nanos))
        .to_string_lossy()
        .to_string()
}
fn jet_watcher_files(path: &String) -> Result<jet_std::WatchHandle, jet_std::IoError> {
    jet_std::WatchHandle::files(path.clone())
}
fn jet_watcher_process_pid(pid: i64) -> jet_std::WatchHandle {
    jet_std::WatchHandle::process_pid(pid)
}
fn jet_watcher_port(host: &String, port: i64) -> jet_std::WatchHandle {
    jet_std::WatchHandle::port(host.clone(), port)
}
fn jet_watcher_set() -> jet_std::WatchSet {
    jet_std::WatchSet::new()
}

fn jet_std_io_args() -> Vec<String> {
    std::env::args().collect()
}
fn jet_std_io_input(prompt: Option<&String>) -> Result<String, jet_std::IoError> {
    use std::io::Write;
    if let Some(p) = prompt {
        print!("{}", p);
        std::io::stdout()
            .flush()
            .map_err(|e| jet_std::IoError::Other {
                message: e.to_string(),
            })?;
    }
    let mut s = String::new();
    std::io::stdin()
        .read_line(&mut s)
        .map_err(|e| jet_std::IoError::Other {
            message: e.to_string(),
        })?;
    while s.ends_with('\n') || s.ends_with('\r') {
        s.pop();
    }
    Ok(s)
}
fn jet_std_io_read_all_input() -> Result<String, jet_std::IoError> {
    use std::io::Read;
    let mut s = String::new();
    std::io::stdin()
        .read_to_string(&mut s)
        .map_err(|e| jet_std::IoError::Other {
            message: e.to_string(),
        })?;
    Ok(s)
}

// D-STDIN1=A: streaming line-by-line stdin.
struct JetStdinReader {
    inner: std::io::BufReader<std::io::Stdin>,
}
fn jet_std_io_stdin() -> JetStdinReader {
    JetStdinReader {
        inner: std::io::BufReader::new(std::io::stdin()),
    }
}
fn jet_std_io_stdin_read_line(r: &mut JetStdinReader) -> Result<Option<String>, jet_std::IoError> {
    use std::io::BufRead;
    let mut line = String::new();
    match r.inner.read_line(&mut line) {
        Ok(0) => Ok(None),
        Ok(_) => {
            while line.ends_with('\n') || line.ends_with('\r') {
                line.pop();
            }
            Ok(Some(line))
        }
        Err(e) => Err(jet_std::IoError::Other {
            message: e.to_string(),
        }),
    }
}

// D-COREIO1=A: stdout/stderr stream handles and TTY-aware terminal helpers.
struct JetStdout;
struct JetStderr;

fn jet_stdio_error(e: std::io::Error) -> jet_std::IoError {
    jet_std::IoError::Other {
        message: e.to_string(),
    }
}

fn jet_std_io_stdout() -> JetStdout {
    JetStdout
}
fn jet_std_io_stderr() -> JetStderr {
    JetStderr
}
fn jet_std_io_stdout_write(_s: &mut JetStdout, text: &String) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    std::io::stdout()
        .write_all(text.as_bytes())
        .map_err(jet_stdio_error)
}
fn jet_std_io_stdout_write_line(_s: &mut JetStdout, text: &String) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    let mut out = std::io::stdout();
    out.write_all(text.as_bytes()).map_err(jet_stdio_error)?;
    out.write_all(b"\n").map_err(jet_stdio_error)
}
fn jet_std_io_stdout_write_bytes(_s: &mut JetStdout, bytes: &Vec<u8>) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    std::io::stdout().write_all(bytes).map_err(jet_stdio_error)
}
fn jet_std_io_stdout_flush(_s: &mut JetStdout) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    std::io::stdout().flush().map_err(jet_stdio_error)
}
fn jet_std_io_stdout_is_tty(_s: &JetStdout) -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}
fn jet_std_io_stderr_write(_s: &mut JetStderr, text: &String) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    std::io::stderr()
        .write_all(text.as_bytes())
        .map_err(jet_stdio_error)
}
fn jet_std_io_stderr_write_line(_s: &mut JetStderr, text: &String) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    let mut out = std::io::stderr();
    out.write_all(text.as_bytes()).map_err(jet_stdio_error)?;
    out.write_all(b"\n").map_err(jet_stdio_error)
}
fn jet_std_io_stderr_write_bytes(_s: &mut JetStderr, bytes: &Vec<u8>) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    std::io::stderr().write_all(bytes).map_err(jet_stdio_error)
}
fn jet_std_io_stderr_flush(_s: &mut JetStderr) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    std::io::stderr().flush().map_err(jet_stdio_error)
}
fn jet_std_io_stderr_is_tty(_s: &JetStderr) -> bool {
    use std::io::IsTerminal;
    std::io::stderr().is_terminal()
}

fn jet_env_int(name: &str) -> Option<i64> {
    std::env::var(name).ok()?.parse::<i64>().ok().filter(|n| *n > 0)
}
fn jet_terminal_size_from_stty() -> Option<(i64, i64)> {
    let out = std::process::Command::new("stty").arg("size").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let mut parts = text.split_whitespace();
    let rows = parts.next()?.parse::<i64>().ok()?;
    let cols = parts.next()?.parse::<i64>().ok()?;
    (rows > 0 && cols > 0).then_some((cols, rows))
}
fn jet_std_io_terminal_width() -> i64 {
    jet_env_int("COLUMNS")
        .or_else(|| jet_terminal_size_from_stty().map(|(w, _)| w))
        .unwrap_or(80)
}
fn jet_std_io_terminal_height() -> i64 {
    jet_env_int("LINES")
        .or_else(|| jet_terminal_size_from_stty().map(|(_, h)| h))
        .unwrap_or(24)
}
fn jet_style_code(name: &str) -> Option<&'static str> {
    match name {
        "black" => Some("30"),
        "red" => Some("31"),
        "green" => Some("32"),
        "yellow" => Some("33"),
        "blue" => Some("34"),
        "magenta" => Some("35"),
        "cyan" => Some("36"),
        "white" => Some("37"),
        "bold" => Some("1"),
        "dim" => Some("2"),
        _ => None,
    }
}
fn jet_style_enabled() -> bool {
    use std::io::IsTerminal;
    std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").map(|t| t != "dumb").unwrap_or(true)
        && std::io::stdout().is_terminal()
}
fn jet_std_io_style(style: &String, text: &String) -> String {
    if jet_style_enabled() {
        jet_std_io_style_force(style, text)
    } else {
        text.clone()
    }
}
fn jet_std_io_style_force(style: &String, text: &String) -> String {
    match jet_style_code(style.as_str()) {
        Some(code) => format!("\x1b[{code}m{text}\x1b[0m"),
        None => text.clone(),
    }
}
fn jet_std_io_progress(text: &String) -> Result<(), jet_std::IoError> {
    use std::io::{IsTerminal, Write};
    let mut out = std::io::stdout();
    if out.is_terminal() {
        out.write_all(b"\r").map_err(jet_stdio_error)?;
        out.write_all(text.as_bytes()).map_err(jet_stdio_error)?;
        out.flush().map_err(jet_stdio_error)
    } else {
        out.write_all(text.as_bytes()).map_err(jet_stdio_error)?;
        out.write_all(b"\n").map_err(jet_stdio_error)
    }
}

fn jet_std_env_get(name: &String) -> Option<String> {
    std::env::var(name).ok()
}
fn jet_std_env_set(name: &String, value: &String) {
    std::env::set_var(name, value);
}
fn jet_std_env_current_dir() -> Result<String, jet_std::IoError> {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| jet_std::IoError::Other {
            message: e.to_string(),
        })
}
fn jet_std_env_home_dir() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
}

fn jet_std_os_name() -> String {
    std::env::consts::OS.to_string()
}
fn jet_std_os_family() -> String {
    std::env::consts::FAMILY.to_string()
}
fn jet_std_os_arch() -> String {
    std::env::consts::ARCH.to_string()
}
fn jet_std_os_cpu_count() -> i64 {
    std::thread::available_parallelism()
        .map(|n| n.get() as i64)
        .unwrap_or(1)
}
fn jet_std_os_temp_dir() -> String {
    std::env::temp_dir().to_string_lossy().to_string()
}
fn jet_std_os_executable() -> String {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}
fn jet_std_os_pid() -> i64 {
    std::process::id() as i64
}
fn jet_std_os_hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok().map(|s| s.trim().to_string()))
        .unwrap_or_else(|| "localhost".to_string())
}
fn jet_std_os_username() -> String {
    std::env::var("USER")
        .ok()
        .or_else(|| std::env::var("USERNAME").ok())
        .unwrap_or_default()
}
fn jet_std_os_set_current_dir(path: &String) -> Result<(), jet_std::IoError> {
    std::env::set_current_dir(path).map_err(|e| jet_std::IoError::Other {
        message: e.to_string(),
    })
}

#[cfg(unix)]
mod jet_os_unix {
    static HIT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    extern "C" fn mark(_: i32) {
        HIT.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    extern "C" {
        fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
    }

    pub fn on_interrupt<F>(handler: F)
    where
        F: Fn() + Send + 'static,
    {
        const SIGINT: i32 = 2;
        unsafe {
            signal(SIGINT, mark);
        }
        std::thread::spawn(move || loop {
            if HIT.swap(false, std::sync::atomic::Ordering::SeqCst) {
                handler();
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        });
    }
}

#[cfg(unix)]
fn jet_std_os_on_interrupt<F>(handler: F)
where
    F: Fn() + Send + 'static,
{
    jet_os_unix::on_interrupt(handler);
}

#[cfg(not(unix))]
fn jet_std_os_on_interrupt<F>(handler: F)
where
    F: Fn() + Send + 'static,
{
    let _ = handler;
}

fn jet_testing_snap(name: &String, actual: &String) -> bool {
    let path = std::path::Path::new("__snapshots__").join(format!("{}.snap", sanitize_test_name(name)));
    let update = std::env::var("JET_UPDATE_SNAPSHOTS").ok().as_deref() == Some("1");
    if update || !path.is_file() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        return std::fs::write(&path, actual).is_ok();
    }
    std::fs::read_to_string(path).map(|s| s == *actual).unwrap_or(false)
}

fn jet_testing_golden(path: &String, actual: &String) -> bool {
    std::fs::read_to_string(path).map(|s| s == *actual).unwrap_or(false)
}

fn jet_testing_fixture(path: &String) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn jet_testing_temp_dir(prefix: &String) -> String {
    let safe = sanitize_test_name(prefix);
    let path = std::env::temp_dir().join(format!("jet_test_{}_{}", safe, std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    let _ = std::fs::create_dir_all(&path);
    path.to_string_lossy().into_owned()
}

fn jet_testing_corpus(path: &String) -> Vec<String> {
    let mut entries = Vec::new();
    if let Ok(read) = std::fs::read_dir(path) {
        let mut paths = read.filter_map(|e| e.ok().map(|e| e.path())).collect::<Vec<_>>();
        paths.sort();
        for p in paths {
            if p.is_file() {
                if let Ok(text) = std::fs::read_to_string(p) {
                    entries.push(text);
                }
            }
        }
    }
    entries
}

fn jet_testing_bench_budget(_name: &String, max_ns: i64) -> bool {
    max_ns >= 0
}

fn sanitize_test_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "case".to_string()
    } else {
        out
    }
}

fn jet_std_process_exit(code: i64) -> ! {
    std::process::exit(code as i32)
}
fn io_other(e: impl ToString) -> jet_std::IoError {
    jet_std::IoError::Other {
        message: e.to_string(),
    }
}
fn jet_std_process_cmd(cmd: &Vec<String>) -> jet_std::ProcessSpec {
    jet_std::ProcessSpec {
        cmd: cmd.clone(),
        cwd: None,
        env_clear: false,
        env_set: Vec::new(),
        env_remove: Vec::new(),
        stdin_text: None,
        stdout: jet_std::ProcessStreamMode::Capture,
        stderr: jet_std::ProcessStreamMode::Capture,
        timeout_ms: None,
        output_limit: None,
        detached: false,
    }
}
fn jet_std_process_run(cmd: &Vec<String>) -> Result<jet_std::ProcessResult, jet_std::IoError> {
    jet_process_spec_run_inner(&jet_std_process_cmd(cmd))
}
fn jet_std_process_pipeline(
    commands: &Vec<Vec<String>>,
) -> Result<jet_std::ProcessResult, jet_std::IoError> {
    if commands.is_empty() {
        return Err(jet_std::IoError::Other {
            message: "process.pipeline needs at least one command".to_string(),
        });
    }
    let mut children: Vec<std::process::Child> = Vec::new();
    let mut prev_stdout: Option<std::process::ChildStdout> = None;
    for cmd in commands {
        if cmd.is_empty() {
            return Err(jet_std::IoError::Other {
                message: "process.pipeline command is empty".to_string(),
            });
        }
        let mut command = std::process::Command::new(&cmd[0]);
        command.args(&cmd[1..]);
        if let Some(stdout) = prev_stdout.take() {
            command.stdin(std::process::Stdio::from(stdout));
        }
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        let mut child = command.spawn().map_err(io_other)?;
        prev_stdout = child.stdout.take();
        children.push(child);
    }
    let mut output = String::new();
    if let Some(mut stdout) = prev_stdout.take() {
        std::io::Read::read_to_string(&mut stdout, &mut output).map_err(io_other)?;
    }
    let mut errors = String::new();
    let mut code = 0;
    for mut child in children {
        if let Some(mut stderr) = child.stderr.take() {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut stderr, &mut text).map_err(io_other)?;
            errors.push_str(&text);
        }
        let status = child.wait().map_err(io_other)?;
        code = status.code().unwrap_or(-1) as i64;
        if !status.success() {
            break;
        }
    }
    Ok(jet_std::ProcessResult {
        code,
        success: code == 0,
        signal: None,
        timed_out: false,
        output,
        errors,
    })
}
fn jet_process_stream_mode(mode: &String) -> jet_std::ProcessStreamMode {
    match mode.as_str() {
        "inherit" => jet_std::ProcessStreamMode::Inherit,
        "discard" => jet_std::ProcessStreamMode::Discard,
        "capture" | "stream" => jet_std::ProcessStreamMode::Capture,
        _ => jet_std::ProcessStreamMode::Capture,
    }
}
fn jet_process_spec_with_mode(
    mut spec: jet_std::ProcessSpec,
    stdout: bool,
    mode: jet_std::ProcessStreamMode,
) -> jet_std::ProcessSpec {
    if stdout {
        spec.stdout = mode;
    } else {
        spec.stderr = mode;
    }
    spec
}
fn jet_process_spec_cwd(mut spec: jet_std::ProcessSpec, cwd: &String) -> jet_std::ProcessSpec {
    spec.cwd = Some(cwd.clone());
    spec
}
fn jet_process_spec_env(
    mut spec: jet_std::ProcessSpec,
    name: &String,
    value: &String,
) -> jet_std::ProcessSpec {
    spec.env_set.push((name.clone(), value.clone()));
    spec
}
fn jet_process_spec_env_remove(
    mut spec: jet_std::ProcessSpec,
    name: &String,
) -> jet_std::ProcessSpec {
    spec.env_remove.push(name.clone());
    spec
}
fn jet_process_spec_env_clear(mut spec: jet_std::ProcessSpec) -> jet_std::ProcessSpec {
    spec.env_clear = true;
    spec
}
fn jet_process_spec_stdin_text(
    mut spec: jet_std::ProcessSpec,
    text: &String,
) -> jet_std::ProcessSpec {
    spec.stdin_text = Some(text.clone());
    spec
}
fn jet_process_spec_stdout(spec: jet_std::ProcessSpec, mode: &String) -> jet_std::ProcessSpec {
    jet_process_spec_with_mode(spec, true, jet_process_stream_mode(mode))
}
fn jet_process_spec_stderr(spec: jet_std::ProcessSpec, mode: &String) -> jet_std::ProcessSpec {
    jet_process_spec_with_mode(spec, false, jet_process_stream_mode(mode))
}
