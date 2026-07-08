// ── Typed Path API (D-PATHFS1) ────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct JetPath {
    inner: std::path::PathBuf,
}
impl JetShow for JetPath {
    fn jet_show(&self) -> String {
        self.inner.to_string_lossy().to_string()
    }
}
fn jet_path_from(s: &String) -> JetPath {
    JetPath {
        inner: std::path::PathBuf::from(s),
    }
}
fn jet_path_join(p: &JetPath, other: &String) -> JetPath {
    JetPath {
        inner: p.inner.join(other.as_str()),
    }
}
fn jet_path_parent(p: &JetPath) -> Option<JetPath> {
    p.inner.parent().map(|par| JetPath {
        inner: par.to_path_buf(),
    })
}
fn jet_path_extension(p: &JetPath) -> Option<String> {
    p.inner.extension().map(|e| e.to_string_lossy().to_string())
}
fn jet_path_stem(p: &JetPath) -> Option<String> {
    p.inner.file_stem().map(|s| s.to_string_lossy().to_string())
}
fn jet_path_write_atomic(p: &JetPath, content: &Vec<u8>) -> Result<(), jet_std::IoError> {
    let path_s = p.inner.to_string_lossy();
    let dir = p.inner.parent().ok_or_else(|| {
        jet_std::io_error(
            &path_s,
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "path has no parent directory",
            ),
        )
    })?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!(".jet_tmp_{}", nanos));
    std::fs::write(&tmp, content)
        .map_err(|e| jet_std::io_error(tmp.to_string_lossy().as_ref(), e))?;
    std::fs::rename(&tmp, &p.inner).map_err(|e| jet_std::io_error(&path_s, e))
}
fn jet_path_walk(p: &JetPath) -> Vec<JetPath> {
    let mut result = Vec::new();
    let mut stack = vec![p.inner.clone()];
    let mut visited = std::collections::HashSet::new();
    while let Some(dir) = stack.pop() {
        let canonical = match std::fs::canonicalize(&dir) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !visited.insert(canonical) {
            continue; // symlink loop — skip
        }
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for entry in rd {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            result.push(JetPath {
                inner: path.clone(),
            });
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    result
}
// ─────────────────────────────────────────────────────────────────────────────

fn jet_std_files_open(path: &String) -> Result<JetFileReader, jet_std::IoError> {
    let f = std::fs::File::open(path).map_err(|e| jet_std::io_error(path, e))?;
    Ok(JetFileReader {
        inner: std::io::BufReader::new(f),
        path: path.clone(),
    })
}
fn jet_std_files_create(path: &String) -> Result<JetFileWriter, jet_std::IoError> {
    let f = std::fs::File::create(path).map_err(|e| jet_std::io_error(path, e))?;
    Ok(JetFileWriter {
        inner: std::io::BufWriter::new(f),
        path: path.clone(),
    })
}
fn jet_std_files_append(path: &String) -> Result<JetFileWriter, jet_std::IoError> {
    let f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| jet_std::io_error(path, e))?;
    Ok(JetFileWriter {
        inner: std::io::BufWriter::new(f),
        path: path.clone(),
    })
}
fn jet_std_file_reader_read_line(
    r: &mut JetFileReader,
) -> Result<Option<String>, jet_std::IoError> {
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
        Err(e) => Err(jet_std::io_error(&r.path, e)),
    }
}
fn jet_std_file_writer_write_line(
    w: &mut JetFileWriter,
    line: &String,
) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    w.inner
        .write_all(line.as_bytes())
        .and_then(|_| w.inner.write_all(b"\n"))
        .map_err(|e| jet_std::io_error(&w.path, e))
}
fn jet_std_file_writer_flush(w: &mut JetFileWriter) -> Result<(), jet_std::IoError> {
    use std::io::Write;
    w.inner.flush().map_err(|e| jet_std::io_error(&w.path, e))
}

