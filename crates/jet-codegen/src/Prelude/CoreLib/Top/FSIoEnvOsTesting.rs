fn jet_std_fs_symlink(from: &String, to: &String) -> Result<(), jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(to.clone()),
            "fault injected: FS.Write",
        ));
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(from, to).map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Write, to, e))
    }
    #[cfg(windows)]
    {
        let meta = std::fs::metadata(from).map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Read, from, e))?;
        if meta.is_dir() {
            std::os::windows::fs::symlink_dir(from, to).map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Write, to, e))
        } else {
            std::os::windows::fs::symlink_file(from, to).map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Write, to, e))
        }
    }
}
fn jet_std_fs_read_link(path: &String) -> Result<String, jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(path.clone()),
            "fault injected: FS.Read",
        ));
    }
    std::fs::read_link(path)
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Read, path, e))
}
fn jet_std_fs_hard_link(from: &String, to: &String) -> Result<(), jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(to.clone()),
            "fault injected: FS.Write",
        ));
    }
    std::fs::hard_link(from, to).map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Write, to, e))
}
fn jet_std_fs_rename(from: &String, to: &String) -> Result<(), jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(to.clone()),
            "fault injected: FS.Write",
        ));
    }
    std::fs::rename(from, to).map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Write, from, e))
}
fn jet_std_fs_stat(path: &String) -> Result<jet_std::Stat, jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(path.clone()),
            "fault injected: FS.Read",
        ));
    }
    let meta = std::fs::symlink_metadata(path).map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Read, path, e))?;
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
fn jet_std_fs_canonicalize(path: &String) -> Result<String, jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(path.clone()),
            "fault injected: FS.Read",
        ));
    }
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Resolve, path, e))
}
fn jet_std_fs_absolute(path: &String) -> Result<String, jet_std::IOError> {
    let p = std::path::Path::new(path);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| jet_std::IOError::other(jet_std::IOOperation::Resolve, None, e))?
            .join(p)
    };
    Ok(abs.to_string_lossy().to_string())
}
fn jet_std_fs_copy_dir(from: &String, to: &String) -> Result<(), jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(from.clone()),
            "fault injected: FS.Read",
        ));
    }
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(to.clone()),
            "fault injected: FS.Write",
        ));
    }
    fn copy_tree(
        src: &std::path::Path,
        dst: &std::path::Path,
        shown: &str,
    ) -> Result<(), jet_std::IOError> {
        std::fs::create_dir_all(dst).map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Write, shown, e))?;
        for entry in std::fs::read_dir(src).map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Read, shown, e))? {
            let entry = entry.map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Read, shown, e))?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            let ft = entry.file_type().map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Read, shown, e))?;
            if ft.is_dir() {
                copy_tree(&src_path, &dst_path, shown)?;
            } else if ft.is_file() {
                std::fs::copy(&src_path, &dst_path).map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Write, shown, e))?;
            }
        }
        Ok(())
    }
    copy_tree(std::path::Path::new(from), std::path::Path::new(to), from)
}
fn jet_std_fs_walk(path: &String) -> Result<Vec<jet_std::WalkEntry>, jet_std::IOError> {
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(path.clone()),
            "fault injected: FS.Read",
        ));
    }
    let root = std::path::PathBuf::from(path);
    let mut out = Vec::new();
    fn walk_dir(
        root: &std::path::Path,
        dir: &std::path::Path,
        depth: i64,
        out: &mut Vec<jet_std::WalkEntry>,
        shown: &str,
    ) -> Result<(), jet_std::IOError> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(dir).map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Read, shown, e))? {
            entries.push(entry.map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Read, shown, e))?);
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
fn jet_std_fs_glob(pattern: &String) -> Result<Vec<String>, jet_std::IOError> {
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
fn jet_std_fs_read_at(path: &String, offset: i64, len: i64) -> Result<Vec<u8>, jet_std::IOError> {
    use std::io::{Read, Seek, SeekFrom};
    if jet_fault_should_fail("FS.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(path.clone()),
            "fault injected: FS.Read",
        ));
    }
    let mut f = std::fs::File::open(path).map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Read, path, e))?;
    f.seek(SeekFrom::Start(offset.max(0) as u64))
        .map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Read, path, e))?;
    let mut buf = vec![0u8; len.max(0) as usize];
    let n = f.read(&mut buf).map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Read, path, e))?;
    buf.truncate(n);
    Ok(buf)
}
fn jet_std_fs_write_at(
    path: &String,
    offset: i64,
    bytes: &Vec<u8>,
) -> Result<(), jet_std::IOError> {
    use std::io::{Seek, SeekFrom, Write};
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(path.clone()),
            "fault injected: FS.Write",
        ));
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(path)
        .map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Write, path, e))?;
    f.seek(SeekFrom::Start(offset.max(0) as u64))
        .map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Write, path, e))?;
    f.write_all(bytes).map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Write, path, e))
}
fn jet_std_fs_fsync(path: &String) -> Result<(), jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Flush,
            Some(path.clone()),
            "fault injected: FS.Write",
        ));
    }
    std::fs::OpenOptions::new()
        .read(true)
        .open(path)
        .and_then(|f| f.sync_all())
        .map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Flush, path, e))
}
fn jet_std_fs_write_atomic(path: &String, bytes: &Vec<u8>) -> Result<(), jet_std::IOError> {
    jet_path_write_atomic(&jet_path_from(path), bytes)
}
fn jet_std_fs_temp_dir(prefix: &String) -> Result<jet_std::TempDir, jet_std::IOError> {
    let path = jet_temp_path(prefix);
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(path.clone()),
            "fault injected: FS.Write",
        ));
    }
    std::fs::create_dir(&path).map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Write, &path, e))?;
    Ok(jet_std::TempDir {
        path,
        cleanup: std::rc::Rc::new(()),
    })
}
fn jet_std_fs_temp_file(prefix: &String) -> Result<jet_std::TempFile, jet_std::IOError> {
    let path = jet_temp_path(prefix);
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(path.clone()),
            "fault injected: FS.Write",
        ));
    }
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Write, &path, e))?;
    Ok(jet_std::TempFile {
        path,
        cleanup: std::rc::Rc::new(()),
    })
}
fn jet_std_fs_lock(path: &String) -> Result<jet_std::FileLock, jet_std::IOError> {
    if jet_fault_should_fail("FS.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some(path.clone()),
            "fault injected: FS.Write",
        ));
    }
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|e| jet_std::io_error_at(jet_std::IOOperation::Write, path, e))?;
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
fn jet_watcher_files(path: &String) -> Result<jet_std::WatchHandle, jet_std::IOError> {
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
// #1480: jet_std_io_input moved to IoLineStream.rs so the JIT host can
// `include!` the same Prelude source (I9); still in scope here via the
// shared crate-root closure both files get concatenated into for AOT.

// D-IO-PROMPT1=A: safe defaults and one terminal-owned secret-input path.
fn jet_std_io_confirm(prompt: &String) -> bool {
    jet_term_confirm_with_io(
        prompt,
        |text| {
            jet_term_write_stdout(text, true)
                .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stdout", e))
        },
        || jet_std_io_input(None),
    )
}

fn jet_std_io_choose(prompt: &String, items: &Vec<String>) -> Result<String, jet_std::IOError> {
    jet_term_choose_with_io(
        prompt,
        items,
        |text| {
            jet_term_write_stdout(text, true)
                .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stdout", e))
        },
        || jet_std_io_input(None),
        || {
            jet_std::IOError::InvalidInput(jet_std::IOContext::new(
                jet_std::IOOperation::Read,
                Some("stdin".to_string()),
                None,
                Some(jet_term_choose_empty_error().to_string()),
            ))
        },
    )
}

fn jet_std_io_secret_error(error: JetTermSecretError) -> jet_std::IOError {
    let projection = jet_term_secret_error_projection(&error);
    let message = error.message();
    let operation = match projection.operation {
        JetTermSecretErrorOperation::Read => jet_std::IOOperation::Read,
        JetTermSecretErrorOperation::Flush => jet_std::IOOperation::Flush,
    };
    let resource = Some(projection.resource.to_string());
    match projection.kind {
        JetTermSecretErrorKind::InvalidInput => jet_std::IOError::InvalidInput(
            jet_std::IOContext::new(operation, resource, None, Some(message)),
        ),
        JetTermSecretErrorKind::Other => jet_std::IOError::other(operation, resource, message),
    }
}

fn jet_std_io_input_secret(prompt: &String) -> Result<String, jet_std::IOError> {
    jet_term_input_secret(
        prompt,
1853:         |text| {
            if jet_fault_should_fail("IO.Write") {
                return Err("fault injected: IO.Write".to_string());
            }
            if jet_fault_should_fail("IO.Flush") {
                return Err("fault injected: IO.Flush".to_string());
            }
            jet_term_write_stdout(text, true).map_err(|error| error.to_string())
        },
1854:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stdout(text, false)
1855:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    let text = format!("{text}\n");
    jet_term_write_stdout(&text, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stdout", e))
1856:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stdout_bytes(bytes, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stdout", e))
1857:     if jet_fault_should_fail("IO.Flush") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Flush,
            Some("stdout".to_string()),
            "fault injected: IO.Flush",
        ));
    }
    jet_term_write_stdout("", true)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Flush, "stdout", e))
1858:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stderr(text, false)
1859:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    let text = format!("{text}\n");
    jet_term_write_stderr(&text, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stderr", e))
1860:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stderr_bytes(bytes, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stderr", e))
1861:     if jet_fault_should_fail("IO.Flush") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Flush,
            Some("stderr".to_string()),
            "fault injected: IO.Flush",
        ));
    }
    jet_term_write_stderr("", true)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Flush, "stderr", e))
1862:         if jet_fault_should_fail("IO.Write") {
            return Err(jet_std::IOError::other(
                jet_std::IOOperation::Write,
                Some("stdout".to_string()),
                "fault injected: IO.Write",
            ));
        }
        if jet_fault_should_fail("IO.Flush") {
            return Err(jet_std::IOError::other(
                jet_std::IOOperation::Flush,
                Some("stdout".to_string()),
                "fault injected: IO.Flush",
            ));
        }
        jet_term_write_stdout(p, true)
1863:     use super::term_prelude::jet_term_write_stdout;
    use crate::fault_injection::jet_fault_should_fail;
        || {
            if jet_fault_should_fail("IO.Read") {
                return Err("fault injected: IO.Read".to_string());
            }
            let mut secret = String::new();
            std::io::stdin()
                .read_line(&mut secret)
                .map(|_| secret)
                .map_err(|error| error.to_string())
        },
    )
    .map_err(jet_std_io_secret_error)
}

fn jet_std_io_read_all_input() -> Result<String, jet_std::IOError> {
    use std::io::Read;
    if jet_fault_should_fail("IO.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some("stdin".to_string()),
            "fault injected: IO.Read",
        ));
    }
    let mut s = String::new();
    std::io::stdin()
        .read_to_string(&mut s)
        .map_err(|e| jet_std::IOError::other(jet_std::IOOperation::Read, Some("stdin".to_string()), e))?;
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
fn jet_std_io_stdin_read_line(r: &mut JetStdinReader) -> Result<Option<String>, jet_std::IOError> {
    use std::io::BufRead;
    if jet_fault_should_fail("IO.Read") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some("stdin".to_string()),
            "fault injected: IO.Read",
        ));
    }
    let mut line = String::new();
    match r.inner.read_line(&mut line) {
        Ok(0) => Ok(None),
        Ok(_) => {
            while line.ends_with('\n') || line.ends_with('\r') {
                line.pop();
            }
            Ok(Some(line))
        }
        Err(e) => Err(jet_std::IOError::other(jet_std::IOOperation::Read, Some("stdin".to_string()), e)),
    }
}

// #1480: readline / read_until / take moved to
// IoLineStream.rs so the JIT host can `include!` the same Prelude source.

fn jet_std_io_buffered() -> JetStdinReader {
    jet_std_io_stdin()
}

fn jet_std_io_binread(path: &String) -> Result<Vec<u8>, jet_std::IOError> {
    jet_std_fs_read_bytes(path)
}

fn jet_std_io_binwrite(path: &String, bytes: &Vec<u8>) -> Result<(), jet_std::IOError> {
    jet_std_fs_write_atomic(path, bytes)
}

// D-COREIO1=A: stdout/stderr stream handles and TTY-aware terminal helpers.
struct JetStdout;
struct JetStderr;

fn jet_stdio_error(operation: jet_std::IOOperation, resource: &str, e: std::io::Error) -> jet_std::IOError {
    jet_std::IOError::other(operation, Some(resource.to_string()), e)
}

fn jet_std_io_stdout() -> JetStdout {
    JetStdout
}
fn jet_std_io_stderr() -> JetStderr {
    JetStderr
}
fn jet_std_io_stdout_write(_s: &mut JetStdout, text: &String) -> Result<(), jet_std::IOError> {
1853:         |text| {
            if jet_fault_should_fail("IO.Write") {
                return Err("fault injected: IO.Write".to_string());
            }
            if jet_fault_should_fail("IO.Flush") {
                return Err("fault injected: IO.Flush".to_string());
            }
            jet_term_write_stdout(text, true).map_err(|error| error.to_string())
        },
1854:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stdout(text, false)
1855:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    let text = format!("{text}\n");
    jet_term_write_stdout(&text, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stdout", e))
1856:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stdout_bytes(bytes, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stdout", e))
1857:     if jet_fault_should_fail("IO.Flush") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Flush,
            Some("stdout".to_string()),
            "fault injected: IO.Flush",
        ));
    }
    jet_term_write_stdout("", true)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Flush, "stdout", e))
1858:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stderr(text, false)
1859:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    let text = format!("{text}\n");
    jet_term_write_stderr(&text, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stderr", e))
1860:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stderr_bytes(bytes, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stderr", e))
1861:     if jet_fault_should_fail("IO.Flush") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Flush,
            Some("stderr".to_string()),
            "fault injected: IO.Flush",
        ));
    }
    jet_term_write_stderr("", true)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Flush, "stderr", e))
1862:         if jet_fault_should_fail("IO.Write") {
            return Err(jet_std::IOError::other(
                jet_std::IOOperation::Write,
                Some("stdout".to_string()),
                "fault injected: IO.Write",
            ));
        }
        if jet_fault_should_fail("IO.Flush") {
            return Err(jet_std::IOError::other(
                jet_std::IOOperation::Flush,
                Some("stdout".to_string()),
                "fault injected: IO.Flush",
            ));
        }
        jet_term_write_stdout(p, true)
1863:     use super::term_prelude::jet_term_write_stdout;
    use crate::fault_injection::jet_fault_should_fail;
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stdout", e))
}
fn jet_std_io_stdout_write_line(_s: &mut JetStdout, text: &String) -> Result<(), jet_std::IOError> {
1853:         |text| {
            if jet_fault_should_fail("IO.Write") {
                return Err("fault injected: IO.Write".to_string());
            }
            if jet_fault_should_fail("IO.Flush") {
                return Err("fault injected: IO.Flush".to_string());
            }
            jet_term_write_stdout(text, true).map_err(|error| error.to_string())
        },
1854:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stdout(text, false)
1855:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    let text = format!("{text}\n");
    jet_term_write_stdout(&text, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stdout", e))
1856:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stdout_bytes(bytes, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stdout", e))
1857:     if jet_fault_should_fail("IO.Flush") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Flush,
            Some("stdout".to_string()),
            "fault injected: IO.Flush",
        ));
    }
    jet_term_write_stdout("", true)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Flush, "stdout", e))
1858:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stderr(text, false)
1859:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    let text = format!("{text}\n");
    jet_term_write_stderr(&text, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stderr", e))
1860:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stderr_bytes(bytes, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stderr", e))
1861:     if jet_fault_should_fail("IO.Flush") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Flush,
            Some("stderr".to_string()),
            "fault injected: IO.Flush",
        ));
    }
    jet_term_write_stderr("", true)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Flush, "stderr", e))
1862:         if jet_fault_should_fail("IO.Write") {
            return Err(jet_std::IOError::other(
                jet_std::IOOperation::Write,
                Some("stdout".to_string()),
                "fault injected: IO.Write",
            ));
        }
        if jet_fault_should_fail("IO.Flush") {
            return Err(jet_std::IOError::other(
                jet_std::IOOperation::Flush,
                Some("stdout".to_string()),
                "fault injected: IO.Flush",
            ));
        }
        jet_term_write_stdout(p, true)
1863:     use super::term_prelude::jet_term_write_stdout;
    use crate::fault_injection::jet_fault_should_fail;
}
fn jet_std_io_stdout_write_bytes(_s: &mut JetStdout, bytes: &Vec<u8>) -> Result<(), jet_std::IOError> {
1853:         |text| {
            if jet_fault_should_fail("IO.Write") {
                return Err("fault injected: IO.Write".to_string());
            }
            if jet_fault_should_fail("IO.Flush") {
                return Err("fault injected: IO.Flush".to_string());
            }
            jet_term_write_stdout(text, true).map_err(|error| error.to_string())
        },
1854:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stdout(text, false)
1855:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    let text = format!("{text}\n");
    jet_term_write_stdout(&text, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stdout", e))
1856:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stdout_bytes(bytes, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stdout", e))
1857:     if jet_fault_should_fail("IO.Flush") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Flush,
            Some("stdout".to_string()),
            "fault injected: IO.Flush",
        ));
    }
    jet_term_write_stdout("", true)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Flush, "stdout", e))
1858:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stderr(text, false)
1859:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    let text = format!("{text}\n");
    jet_term_write_stderr(&text, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stderr", e))
1860:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stderr_bytes(bytes, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stderr", e))
1861:     if jet_fault_should_fail("IO.Flush") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Flush,
            Some("stderr".to_string()),
            "fault injected: IO.Flush",
        ));
    }
    jet_term_write_stderr("", true)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Flush, "stderr", e))
1862:         if jet_fault_should_fail("IO.Write") {
            return Err(jet_std::IOError::other(
                jet_std::IOOperation::Write,
                Some("stdout".to_string()),
                "fault injected: IO.Write",
            ));
        }
        if jet_fault_should_fail("IO.Flush") {
            return Err(jet_std::IOError::other(
                jet_std::IOOperation::Flush,
                Some("stdout".to_string()),
                "fault injected: IO.Flush",
            ));
        }
        jet_term_write_stdout(p, true)
1863:     use super::term_prelude::jet_term_write_stdout;
    use crate::fault_injection::jet_fault_should_fail;
}
fn jet_std_io_stdout_flush(_s: &mut JetStdout) -> Result<(), jet_std::IOError> {
1853:         |text| {
            if jet_fault_should_fail("IO.Write") {
                return Err("fault injected: IO.Write".to_string());
            }
            if jet_fault_should_fail("IO.Flush") {
                return Err("fault injected: IO.Flush".to_string());
            }
            jet_term_write_stdout(text, true).map_err(|error| error.to_string())
        },
1854:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stdout(text, false)
1855:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    let text = format!("{text}\n");
    jet_term_write_stdout(&text, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stdout", e))
1856:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stdout_bytes(bytes, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stdout", e))
1857:     if jet_fault_should_fail("IO.Flush") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Flush,
            Some("stdout".to_string()),
            "fault injected: IO.Flush",
        ));
    }
    jet_term_write_stdout("", true)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Flush, "stdout", e))
1858:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stderr(text, false)
1859:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    let text = format!("{text}\n");
    jet_term_write_stderr(&text, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stderr", e))
1860:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stderr_bytes(bytes, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stderr", e))
1861:     if jet_fault_should_fail("IO.Flush") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Flush,
            Some("stderr".to_string()),
            "fault injected: IO.Flush",
        ));
    }
    jet_term_write_stderr("", true)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Flush, "stderr", e))
1862:         if jet_fault_should_fail("IO.Write") {
            return Err(jet_std::IOError::other(
                jet_std::IOOperation::Write,
                Some("stdout".to_string()),
                "fault injected: IO.Write",
            ));
        }
        if jet_fault_should_fail("IO.Flush") {
            return Err(jet_std::IOError::other(
                jet_std::IOOperation::Flush,
                Some("stdout".to_string()),
                "fault injected: IO.Flush",
            ));
        }
        jet_term_write_stdout(p, true)
1863:     use super::term_prelude::jet_term_write_stdout;
    use crate::fault_injection::jet_fault_should_fail;
}
fn jet_std_io_stdout_is_tty(_s: &JetStdout) -> bool {
    jet_term_stdout_is_terminal()
}
fn jet_std_io_stderr_write(_s: &mut JetStderr, text: &String) -> Result<(), jet_std::IOError> {
1853:         |text| {
            if jet_fault_should_fail("IO.Write") {
                return Err("fault injected: IO.Write".to_string());
            }
            if jet_fault_should_fail("IO.Flush") {
                return Err("fault injected: IO.Flush".to_string());
            }
            jet_term_write_stdout(text, true).map_err(|error| error.to_string())
        },
1854:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stdout(text, false)
1855:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    let text = format!("{text}\n");
    jet_term_write_stdout(&text, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stdout", e))
1856:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stdout_bytes(bytes, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stdout", e))
1857:     if jet_fault_should_fail("IO.Flush") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Flush,
            Some("stdout".to_string()),
            "fault injected: IO.Flush",
        ));
    }
    jet_term_write_stdout("", true)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Flush, "stdout", e))
1858:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stderr(text, false)
1859:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    let text = format!("{text}\n");
    jet_term_write_stderr(&text, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stderr", e))
1860:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stderr_bytes(bytes, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stderr", e))
1861:     if jet_fault_should_fail("IO.Flush") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Flush,
            Some("stderr".to_string()),
            "fault injected: IO.Flush",
        ));
    }
    jet_term_write_stderr("", true)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Flush, "stderr", e))
1862:         if jet_fault_should_fail("IO.Write") {
            return Err(jet_std::IOError::other(
                jet_std::IOOperation::Write,
                Some("stdout".to_string()),
                "fault injected: IO.Write",
            ));
        }
        if jet_fault_should_fail("IO.Flush") {
            return Err(jet_std::IOError::other(
                jet_std::IOOperation::Flush,
                Some("stdout".to_string()),
                "fault injected: IO.Flush",
            ));
        }
        jet_term_write_stdout(p, true)
1863:     use super::term_prelude::jet_term_write_stdout;
    use crate::fault_injection::jet_fault_should_fail;
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stderr", e))
}
fn jet_std_io_stderr_write_line(_s: &mut JetStderr, text: &String) -> Result<(), jet_std::IOError> {
1853:         |text| {
            if jet_fault_should_fail("IO.Write") {
                return Err("fault injected: IO.Write".to_string());
            }
            if jet_fault_should_fail("IO.Flush") {
                return Err("fault injected: IO.Flush".to_string());
            }
            jet_term_write_stdout(text, true).map_err(|error| error.to_string())
        },
1854:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stdout(text, false)
1855:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    let text = format!("{text}\n");
    jet_term_write_stdout(&text, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stdout", e))
1856:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stdout_bytes(bytes, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stdout", e))
1857:     if jet_fault_should_fail("IO.Flush") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Flush,
            Some("stdout".to_string()),
            "fault injected: IO.Flush",
        ));
    }
    jet_term_write_stdout("", true)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Flush, "stdout", e))
1858:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stderr(text, false)
1859:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    let text = format!("{text}\n");
    jet_term_write_stderr(&text, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stderr", e))
1860:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stderr_bytes(bytes, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stderr", e))
1861:     if jet_fault_should_fail("IO.Flush") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Flush,
            Some("stderr".to_string()),
            "fault injected: IO.Flush",
        ));
    }
    jet_term_write_stderr("", true)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Flush, "stderr", e))
1862:         if jet_fault_should_fail("IO.Write") {
            return Err(jet_std::IOError::other(
                jet_std::IOOperation::Write,
                Some("stdout".to_string()),
                "fault injected: IO.Write",
            ));
        }
        if jet_fault_should_fail("IO.Flush") {
            return Err(jet_std::IOError::other(
                jet_std::IOOperation::Flush,
                Some("stdout".to_string()),
                "fault injected: IO.Flush",
            ));
        }
        jet_term_write_stdout(p, true)
1863:     use super::term_prelude::jet_term_write_stdout;
    use crate::fault_injection::jet_fault_should_fail;
}
fn jet_std_io_stderr_write_bytes(_s: &mut JetStderr, bytes: &Vec<u8>) -> Result<(), jet_std::IOError> {
1853:         |text| {
            if jet_fault_should_fail("IO.Write") {
                return Err("fault injected: IO.Write".to_string());
            }
            if jet_fault_should_fail("IO.Flush") {
                return Err("fault injected: IO.Flush".to_string());
            }
            jet_term_write_stdout(text, true).map_err(|error| error.to_string())
        },
1854:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stdout(text, false)
1855:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    let text = format!("{text}\n");
    jet_term_write_stdout(&text, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stdout", e))
1856:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stdout_bytes(bytes, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stdout", e))
1857:     if jet_fault_should_fail("IO.Flush") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Flush,
            Some("stdout".to_string()),
            "fault injected: IO.Flush",
        ));
    }
    jet_term_write_stdout("", true)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Flush, "stdout", e))
1858:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stderr(text, false)
1859:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    let text = format!("{text}\n");
    jet_term_write_stderr(&text, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stderr", e))
1860:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stderr_bytes(bytes, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stderr", e))
1861:     if jet_fault_should_fail("IO.Flush") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Flush,
            Some("stderr".to_string()),
            "fault injected: IO.Flush",
        ));
    }
    jet_term_write_stderr("", true)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Flush, "stderr", e))
1862:         if jet_fault_should_fail("IO.Write") {
            return Err(jet_std::IOError::other(
                jet_std::IOOperation::Write,
                Some("stdout".to_string()),
                "fault injected: IO.Write",
            ));
        }
        if jet_fault_should_fail("IO.Flush") {
            return Err(jet_std::IOError::other(
                jet_std::IOOperation::Flush,
                Some("stdout".to_string()),
                "fault injected: IO.Flush",
            ));
        }
        jet_term_write_stdout(p, true)
1863:     use super::term_prelude::jet_term_write_stdout;
    use crate::fault_injection::jet_fault_should_fail;
}
fn jet_std_io_stderr_flush(_s: &mut JetStderr) -> Result<(), jet_std::IOError> {
1853:         |text| {
            if jet_fault_should_fail("IO.Write") {
                return Err("fault injected: IO.Write".to_string());
            }
            if jet_fault_should_fail("IO.Flush") {
                return Err("fault injected: IO.Flush".to_string());
            }
            jet_term_write_stdout(text, true).map_err(|error| error.to_string())
        },
1854:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stdout(text, false)
1855:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    let text = format!("{text}\n");
    jet_term_write_stdout(&text, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stdout", e))
1856:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stdout".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stdout_bytes(bytes, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stdout", e))
1857:     if jet_fault_should_fail("IO.Flush") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Flush,
            Some("stdout".to_string()),
            "fault injected: IO.Flush",
        ));
    }
    jet_term_write_stdout("", true)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Flush, "stdout", e))
1858:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stderr(text, false)
1859:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    let text = format!("{text}\n");
    jet_term_write_stderr(&text, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stderr", e))
1860:     if jet_fault_should_fail("IO.Write") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("stderr".to_string()),
            "fault injected: IO.Write",
        ));
    }
    jet_term_write_stderr_bytes(bytes, false)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Write, "stderr", e))
1861:     if jet_fault_should_fail("IO.Flush") {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Flush,
            Some("stderr".to_string()),
            "fault injected: IO.Flush",
        ));
    }
    jet_term_write_stderr("", true)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Flush, "stderr", e))
1862:         if jet_fault_should_fail("IO.Write") {
            return Err(jet_std::IOError::other(
                jet_std::IOOperation::Write,
                Some("stdout".to_string()),
                "fault injected: IO.Write",
            ));
        }
        if jet_fault_should_fail("IO.Flush") {
            return Err(jet_std::IOError::other(
                jet_std::IOOperation::Flush,
                Some("stdout".to_string()),
                "fault injected: IO.Flush",
            ));
        }
        jet_term_write_stdout(p, true)
1863:     use super::term_prelude::jet_term_write_stdout;
    use crate::fault_injection::jet_fault_should_fail;
}
fn jet_std_io_stderr_is_tty(_s: &JetStderr) -> bool {
    jet_term_stderr_is_terminal()
}

fn jet_std_io_terminal_width() -> i64 {
    jet_term_width(|name| jet_std_env_get(&name.to_string()))
}
fn jet_std_io_terminal_height() -> i64 {
    jet_term_height(|name| jet_std_env_get(&name.to_string()))
}
fn jet_style_enabled() -> bool {
    jet_term_style_enabled(
        jet_env_value_raw("NO_COLOR").is_some(),
        jet_env_value_raw("TERM")
            .and_then(|term| term.into_string().ok())
            .is_some_and(|term| term == "dumb"),
        jet_term_stdout_is_terminal(),
    )
}
fn jet_std_io_style(style: &String, text: &String) -> String {
    jet_term_style(style, text, jet_style_enabled())
}
fn jet_std_io_style_force(style: &String, text: &String) -> String {
    jet_term_style_force(style, text)
}

struct JetProgressIter<T> {
    inner: Box<dyn Iterator<Item = T>>,
    total: Option<usize>,
    description: String,
    format: String,
    started: std::time::Instant,
    count: usize,
    displayed: bool,
    finished: bool,
}

impl<T> Iterator for JetProgressIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        let item = match self.inner.next() {
            Some(item) => item,
            None => {
                self.finish();
                return None;
            }
        };
        self.count += 1;
        let text = jet_progress_render(
            &self.description,
            &self.format,
            self.count,
            self.total,
            self.started.elapsed().as_secs_f64(),
            jet_env_value_raw("NO_COLOR").is_some(),
        );
        if let Err(error) = jet_std_io_progress_emit(&text) {
            jet_panic("<progress>", 0, &format!("{error:?}"));
        }
        self.displayed = true;
        Some(item)
    }
}

impl<T> JetProgressIter<T> {
    fn finish(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        if self.displayed {
            if let Err(error) = jet_std_io_progress_finish() {
                jet_panic("<progress>", 0, &format!("{error:?}"));
            }
        }
    }
}

impl<T> Drop for JetProgressIter<T> {
    fn drop(&mut self) {
        self.finish();
    }
}

fn jet_std_io_progress_iter<T: 'static>(
    it: JetIter<T>,
    description: &String,
    format: &String,
) -> JetIter<T> {
    let (lower, upper) = it.0.size_hint();
    let total = upper.filter(|upper| *upper == lower);
    jet_std_io_progress_iter_with_total(it, description, format, total)
}

fn jet_std_io_progress_iter_with_total<T: 'static>(
    it: JetIter<T>,
    description: &String,
    format: &String,
    total: Option<usize>,
) -> JetIter<T> {
    JetIter(Box::new(JetProgressIter {
        inner: it.0,
        total,
        description: description.clone(),
        format: format.clone(),
        started: std::time::Instant::now(),
        count: 0,
        displayed: false,
        finished: false,
    }))
}

fn jet_std_io_progress_list<T: 'static>(
    xs: Vec<T>,
    description: &String,
    format: &String,
) -> JetIter<T> {
    let total = xs.len();
    jet_std_io_progress_iter_with_total(
        jet_iter_from_vec(xs),
        description,
        format,
        Some(total),
    )
}

fn jet_std_io_progress_emit(text: &str) -> Result<(), jet_std::IOError> {
    let frame = jet_term_progress_frame(jet_term_stdout_is_terminal(), text);
    jet_term_write_stdout(&frame, true)
        .map_err(|e| jet_stdio_error(jet_std::IOOperation::Flush, "stdout", e))
}

fn jet_std_io_progress_finish() -> Result<(), jet_std::IOError> {
    let frame = jet_term_progress_finish(jet_term_stdout_is_terminal());
    if !frame.is_empty() {
        jet_term_write_stdout(frame, true)
            .map_err(|e| jet_stdio_error(jet_std::IOOperation::Flush, "stdout", e))?;
    }
    Ok(())
}

fn jet_std_io_progress(text: &String) -> Result<(), jet_std::IOError> {
    // Preserve the legacy one-string helper's terminal behavior. Iterable
    // progress owns its lifecycle through `JetProgressIter::finish`; the
    // string form has never added a prompt-terminating newline.
    jet_std_io_progress_emit(text)
}

// D-ENV-MUTATE1=A: Jet owns a raw, process-global logical environment. User
// mutation never calls libc `setenv` or changes the Windows environment block.
// One lock supplies get/set/unset/vars and atomic child snapshots.
fn jet_env_validate_name(name: &str) -> Result<(), jet_std::EnvError> {
    if name.is_empty() || name.contains('\0') || name.contains('=') {
        Err(jet_std::EnvError::InvalidName)
    } else {
        Ok(())
    }
}

fn jet_env_validate_value(value: &str) -> Result<(), jet_std::EnvError> {
    if value.contains('\0') {
        Err(jet_std::EnvError::InvalidValue)
    } else {
        Ok(())
    }
}

fn jet_env_value_raw(name: &str) -> Option<std::ffi::OsString> {
    let name = std::ffi::OsStr::new(name);
    jet_env_read()
        .iter()
        .find(|(candidate, _)| jet_env_key_eq(candidate.as_os_str(), name))
        .map(|(_, value)| value.clone())
}

fn jet_std_env_get(name: &String) -> Option<String> {
    jet_env_value_raw(name).and_then(|value| value.into_string().ok())
}

fn jet_std_env_set(name: &String, value: &String) -> Result<(), jet_std::EnvError> {
    jet_env_validate_name(name)?;
    jet_env_validate_value(value)?;
    let os_name = std::ffi::OsString::from(name);
    let mut entries = jet_env_write();
    if let Some(old) = entries
        .iter()
        .position(|(candidate, _)| jet_env_key_eq(candidate.as_os_str(), os_name.as_os_str()))
    {
        entries.remove(old);
    }
    entries.push((os_name, std::ffi::OsString::from(value)));
    Ok(())
}

fn jet_std_env_unset(name: &String) -> Result<bool, jet_std::EnvError> {
    jet_env_validate_name(name)?;
    let os_name = std::ffi::OsStr::new(name);
    let mut entries = jet_env_write();
    if let Some(old) = entries
        .iter()
        .position(|(candidate, _)| jet_env_key_eq(candidate.as_os_str(), os_name))
    {
        entries.remove(old);
        Ok(true)
    } else {
        Ok(false)
    }
}

fn jet_std_env_vars() -> Result<Vec<String>, jet_std::EnvError> {
    let entries = jet_env_read();
    let mut names = Vec::with_capacity(entries.len());
    for (name, value) in entries.iter() {
        let decoded_name = name.to_str().ok_or(jet_std::EnvError::NonUnicode)?;
        value.to_str().ok_or(jet_std::EnvError::NonUnicode)?;
        names.push((name.clone(), decoded_name.to_string()));
    }
    names.sort_by(|(left, _), (right, _)| {
        let folded = jet_env_key_cmp(left.as_os_str(), right.as_os_str());
        if folded != std::cmp::Ordering::Equal {
            return folded;
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            return left.encode_wide().cmp(right.encode_wide());
        }
        #[cfg(not(windows))]
        std::cmp::Ordering::Equal
    });
    Ok(names.into_iter().map(|(_, name)| name).collect())
}

fn jet_std_env_snapshot_raw() -> JetEnvEntries {
    jet_env_read().clone()
}
fn jet_std_env_current_dir() -> Result<String, jet_std::IOError> {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| jet_std::IOError::other(jet_std::IOOperation::Resolve, None, e))
}
fn jet_std_env_home_dir() -> Option<String> {
    jet_std_env_get(&"HOME".to_string())
        .or_else(|| jet_std_env_get(&"USERPROFILE".to_string()))
}

fn jet_std_os_set_current_dir(path: &String) -> Result<(), jet_std::IOError> {
    std::env::set_current_dir(path).map_err(|e| jet_std::IOError::other(jet_std::IOOperation::Resolve, Some(path.clone()), e))
}

mod jet_os_interrupt {
    use std::sync::{mpsc, Arc, OnceLock};

    static QUEUE: JetInterruptQueue = JetInterruptQueue::new();
    static DISPATCH: OnceLock<Result<mpsc::Sender<Command>, String>> = OnceLock::new();

    enum Command {
        Register(Arc<dyn Fn() + Send + Sync + 'static>, mpsc::SyncSender<()>),
    }

    struct PanicBoundary;

    impl PanicBoundary {
        fn enter() -> Self {
            super::jet_interrupt_handler_panic_enter();
            Self
        }
    }

    impl Drop for PanicBoundary {
        fn drop(&mut self) {
            super::jet_interrupt_handler_panic_leave();
        }
    }

    fn note_interrupt() {
        // OS callbacks do no allocation, locking, or user work.
        QUEUE.note();
    }

    #[cfg(unix)]
    extern "C" fn unix_mark(_: i32) {
        note_interrupt();
    }

    #[cfg(unix)]
    fn install_platform_handler() -> Result<(), String> {
        super::jet_interrupt_install_unix_handler(unix_mark)
    }

    #[cfg(windows)]
    unsafe extern "system" fn windows_mark(kind: u32) -> i32 {
        const CTRL_C_EVENT: u32 = 0;
        if kind == CTRL_C_EVENT {
            note_interrupt();
            1
        } else {
            0
        }
    }

    #[cfg(windows)]
    fn install_platform_handler() -> Result<(), String> {
        super::jet_interrupt_install_windows_handler(Some(windows_mark))
    }

    #[cfg(not(any(unix, windows)))]
    fn install_platform_handler() -> Result<(), String> {
        Err(super::jet_interrupt_unavailable_error().to_string())
    }

    fn dispatcher() -> Result<&'static mpsc::Sender<Command>, String> {
        match DISPATCH.get_or_init(|| {
            install_platform_handler()?;
            let (tx, rx) = mpsc::channel::<Command>();
            std::thread::Builder::new()
                .name("jet-interrupt".to_string())
                .spawn(move || {
                    let mut handlers: Vec<Arc<dyn Fn() + Send + Sync + 'static>> = Vec::new();
                    loop {
                        match rx.recv_timeout(super::jet_interrupt_poll_interval()) {
                            Ok(Command::Register(handler, ready)) => {
                                handlers.push(handler);
                                let _ = ready.send(());
                            }
                            Err(mpsc::RecvTimeoutError::Disconnected) => return,
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                        }
                        QUEUE.dispatch(&handlers, |handler| {
                            if let Err(payload) = std::panic::catch_unwind(
                                std::panic::AssertUnwindSafe(|| {
                                    let _boundary = PanicBoundary::enter();
                                    handler();
                                }),
                            ) {
                                super::jet_report_caught_unwind(payload);
                            }
                        });
                    }
                })
                .map_err(super::jet_interrupt_dispatcher_start_error)?;
            Ok(tx)
        }) {
            Ok(tx) => Ok(tx),
            Err(message) => Err(message.clone()),
        }
    }

    pub fn on_interrupt(handler: Arc<dyn Fn() + Send + Sync + 'static>) {
        let tx = dispatcher().unwrap_or_else(|message| {
            super::jet_panic(
                "<core.os>",
                0,
                &super::jet_interrupt_core_error(&message),
            )
        });
        let (ready_tx, ready_rx) = mpsc::sync_channel(0);
        tx.send(Command::Register(handler, ready_tx))
            .unwrap_or_else(|_| {
                super::jet_panic(
                    "<core.os>",
                    0,
                    &super::jet_interrupt_core_error(
                        super::jet_interrupt_dispatcher_stopped_error(),
                    ),
                )
            });
        ready_rx
            .recv()
            .unwrap_or_else(|_| {
                super::jet_panic(
                    "<core.os>",
                    0,
                    &super::jet_interrupt_core_error(
                        super::jet_interrupt_dispatcher_stopped_error(),
                    ),
                )
            });
    }
}

fn jet_std_os_on_interrupt(handler: std::sync::Arc<dyn Fn() + Send + Sync + 'static>) {
    jet_os_interrupt::on_interrupt(handler);
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
    match std::fs::read_to_string(path) {
        Ok(expected) if expected == *actual => true,
        Ok(expected) => {
            let detail = format!(
                "path: {}\n{}",
                path,
                jet_testing_unified_diff(path, &expected, actual)
            );
            jet_testing_record_failure(
                format!("golden file differs: {}", path),
                detail,
            );
            false
        }
        Err(error) => {
            let message = if error.kind() == std::io::ErrorKind::NotFound {
                format!("golden file is missing: {}", path)
            } else {
                format!("golden file cannot be read: {}", path)
            };
            jet_testing_record_failure(message, format!("path: {}", path));
            false
        }
    }
}

fn jet_testing_fixture(path: &String) -> String {
    match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) => {
            let message = if error.kind() == std::io::ErrorKind::NotFound {
                format!("fixture is missing: {}", path)
            } else {
                format!("fixture cannot be read: {}", path)
            };
            jet_testing_record_failure(message, format!("path: {}", path));
            String::new()
        }
    }
}

fn jet_testing_temp_dir(prefix: &String) -> String {
    jet_testing_temp_dir_path(prefix)
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
    jet_runtime_explicit_exit(code)
}
fn jet_std_process_run(cmd: &Vec<String>) -> Result<jet_std::ProcessResult, jet_std::IOError> {
    jet_process_spec_run(&jet_std_process_cmd(cmd))
}
fn jet_std_process_pipeline(
    specs: &Vec<jet_std::ProcessSpec>,
) -> Result<jet_std::ProcessResult, jet_std::IOError> {
    jet_process_spec_pipeline(specs)
}
