//! U27 (D-JPK-BUILDDBG1=A): failed-build logs, preserved scratch, explain.

use super::JSON::{self, JSONValue};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const LOG_ROOT: &str = "build-logs";
const SCRATCH_ROOT: &str = "failed-scratch";
static WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepLog {
    pub index: usize,
    pub total: usize,
    pub name: String,
    pub command: String,
    pub cwd: String,
    pub status: String,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub id: String,
    pub package: String,
    pub reference: String,
    pub provider: String,
    pub recipe_hash: String,
    pub source_hash: String,
    pub platform: String,
    pub status: String,
    pub failed_step: usize,
    pub scratch_dir: String,
    pub log_dir: String,
    pub steps: Vec<StepLog>,
}

impl Attempt {
    pub fn new(
        package: &str,
        reference: &str,
        provider: &str,
        recipe_hash: &str,
        source_hash: &str,
    ) -> Attempt {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        Attempt {
            id: format!("{}-{now}", safe_name(package)),
            package: package.to_string(),
            reference: reference.to_string(),
            provider: provider.to_string(),
            recipe_hash: recipe_hash.to_string(),
            source_hash: source_hash.to_string(),
            platform: super::Platform::host_key(),
            status: "running".to_string(),
            failed_step: 0,
            scratch_dir: String::new(),
            log_dir: String::new(),
            steps: Vec::new(),
        }
    }

    pub fn push_step(&mut self, step: StepLog) {
        if step.status != "ok" && self.failed_step == 0 {
            self.failed_step = step.index;
            self.status = "failed".to_string();
        }
        self.steps.push(step);
    }

    pub fn mark_ok(&mut self) {
        self.status = "ok".to_string();
    }

    pub fn persist(&mut self, hangar_dir: &Path) -> std::io::Result<()> {
        validate_id(&self.id)?;
        let dir = log_dir(hangar_dir, &self.package).join(&self.id);
        ensure_real_directory(&log_dir(hangar_dir, &self.package), "build log package")?;
        ensure_real_directory(&dir, "build attempt log")?;
        self.log_dir = dir.to_string_lossy().into_owned();
        for step in &self.steps {
            let stem = format!("{:02}-{}", step.index, safe_name(&step.name));
            write_atomic(&dir.join(format!("{stem}.stdout.log")), step.stdout.as_bytes())?;
            write_atomic(&dir.join(format!("{stem}.stderr.log")), step.stderr.as_bytes())?;
        }
        let json = self.to_json();
        write_atomic(&dir.join("attempt.json"), json.as_bytes())?;
        write_atomic(
            &log_dir(hangar_dir, &self.package).join("latest.json"),
            json.as_bytes(),
        )?;
        Ok(())
    }

    pub fn preserve_scratch(
        &mut self,
        hangar_dir: &Path,
        source: &Path,
        output: &Path,
    ) -> io::Result<()> {
        validate_id(&self.id)?;
        let root = hangar_dir.join(SCRATCH_ROOT);
        ensure_real_directory(&root, "failed-build scratch root")?;
        let sequence = WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let stage = root.join(format!(
            ".jet-build-stage-{}-{}-{}",
            self.id,
            std::process::id(),
            sequence
        ));
        let dir = root.join(&self.id);
        let result = (|| {
            ensure_absent(&stage, "failed-build scratch staging")?;
            std::fs::create_dir(&stage)?;
            copy_tree(source, &stage.join("source"))?;
            match std::fs::symlink_metadata(output) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "failed-build output is a symlink; scratch preservation stopped",
                    ));
                }
                Ok(_) => copy_tree(output, &stage.join("output"))?,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            ensure_absent(&dir, "failed-build scratch destination")?;
            std::fs::rename(&stage, &dir)?;
            self.scratch_dir = dir.to_string_lossy().into_owned();
            Ok(())
        })();
        if result.is_err() {
            let _ = remove_tree_without_following(&stage);
            self.scratch_dir.clear();
        }
        result
    }

    pub fn to_json(&self) -> String {
        let steps = self
            .steps
            .iter()
            .map(step_json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"id\":{},\"package\":{},\"reference\":{},\"provider\":{},\"recipe_hash\":{},\"source_hash\":{},\"platform\":{},\"status\":{},\"failed_step\":{},\"scratch_dir\":{},\"log_dir\":{},\"steps\":[{}]}}",
            JSON::quote(&self.id),
            JSON::quote(&self.package),
            JSON::quote(&self.reference),
            JSON::quote(&self.provider),
            JSON::quote(&self.recipe_hash),
            JSON::quote(&self.source_hash),
            JSON::quote(&self.platform),
            JSON::quote(&self.status),
            self.failed_step,
            JSON::quote(&self.scratch_dir),
            JSON::quote(&self.log_dir),
            steps
        )
    }
}

/// Remove only failed-build scratch staging directories left by a crashed
/// snapshot publication. Published scratch directories remain user-visible.
pub fn recover_scratch(hangar_dir: &Path) -> io::Result<usize> {
    let root = hangar_dir.join(SCRATCH_ROOT);
    let metadata = match std::fs::symlink_metadata(&root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "failed-build scratch root is not a real directory",
        ));
    }
    let mut recovered = 0;
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(".jet-build-stage-") {
            continue;
        }
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed-build scratch stage is a symlink: {}", path.display()),
            ));
        }
        remove_tree_without_following(&path)?;
        recovered += 1;
    }
    Ok(recovered)
}

pub fn latest(hangar_dir: &Path, package: &str) -> Result<Option<Attempt>, String> {
    let path = log_dir(hangar_dir, package).join("latest.json");
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("latest build log is not a regular file".to_string());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    parse_attempt(&text).map(Some)
}

pub fn latest_json(hangar_dir: &Path, package: &str) -> Result<Option<String>, String> {
    let path = log_dir(hangar_dir, package).join("latest.json");
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("latest build log is not a regular file".to_string());
    }
    std::fs::read_to_string(&path)
        .map(Some)
        .map_err(|e| e.to_string())
}

pub fn text_logs(attempt: &Attempt) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{}  {}  {}\n",
        attempt.package, attempt.status, attempt.reference
    ));
    for step in &attempt.steps {
        out.push_str(&format!(
            "[step {}/{} {}] {}\n",
            step.index, step.total, step.name, step.status
        ));
        if !step.stdout.trim().is_empty() {
            out.push_str(step.stdout.trim_end());
            out.push('\n');
        }
        if !step.stderr.trim().is_empty() {
            out.push_str(step.stderr.trim_end());
            out.push('\n');
        }
    }
    out
}

pub fn explain(attempt: &Attempt) -> String {
    let failed = attempt
        .steps
        .iter()
        .find(|s| s.index == attempt.failed_step)
        .map(|s| format!("step {}/{} ({})", s.index, s.total, s.command))
        .unwrap_or_else(|| "-".to_string());
    format!(
        "ref      {}\nprovider {}\nplatform {}\nrecipe   {}\nsource   {}\nstatus   {}\nfailed   {}\nscratch  {}\nlogs     jet logs {}\n",
        attempt.reference,
        attempt.provider,
        attempt.platform,
        attempt.recipe_hash,
        attempt.source_hash,
        attempt.status,
        failed,
        empty_dash(&attempt.scratch_dir),
        attempt.package
    )
}

pub fn safe_name(s: &str) -> String {
    let name: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() || matches!(name.as_str(), "." | "..") {
        "_".to_string()
    } else {
        name
    }
}

fn log_dir(hangar_dir: &Path, package: &str) -> PathBuf {
    hangar_dir.join(LOG_ROOT).join(safe_name(package))
}

fn validate_id(id: &str) -> io::Result<()> {
    if id.is_empty() || safe_name(id) != id || id == "." || id == ".." {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "build attempt id is not a safe Hangar path component",
        ));
    }
    Ok(())
}

fn step_json(step: &StepLog) -> String {
    format!(
        "{{\"index\":{},\"total\":{},\"name\":{},\"command\":{},\"cwd\":{},\"status\":{},\"stdout\":{},\"stderr\":{}}}",
        step.index,
        step.total,
        JSON::quote(&step.name),
        JSON::quote(&step.command),
        JSON::quote(&step.cwd),
        JSON::quote(&step.status),
        JSON::quote(&redact(&step.stdout)),
        JSON::quote(&redact(&step.stderr))
    )
}

fn parse_attempt(text: &str) -> Result<Attempt, String> {
    let json = JSON::parse(text)?;
    let obj = json.as_object()?;
    let mut attempt = Attempt {
        id: str_field(obj, "id")?,
        package: str_field(obj, "package")?,
        reference: str_field(obj, "reference")?,
        provider: str_field(obj, "provider")?,
        recipe_hash: str_field(obj, "recipe_hash")?,
        source_hash: str_field(obj, "source_hash")?,
        platform: str_field(obj, "platform")?,
        status: str_field(obj, "status")?,
        failed_step: num_field(obj, "failed_step"),
        scratch_dir: str_field(obj, "scratch_dir")?,
        log_dir: str_field(obj, "log_dir")?,
        steps: Vec::new(),
    };
    if let Some(JSONValue::Array(steps)) = obj.get("steps") {
        for item in steps {
            let o = item.as_object()?;
            attempt.steps.push(StepLog {
                index: num_field(o, "index"),
                total: num_field(o, "total"),
                name: str_field(o, "name")?,
                command: str_field(o, "command")?,
                cwd: str_field(o, "cwd")?,
                status: str_field(o, "status")?,
                stdout: str_field(o, "stdout")?,
                stderr: str_field(o, "stderr")?,
            });
        }
    }
    Ok(attempt)
}

fn str_field(obj: &std::collections::BTreeMap<String, JSONValue>, key: &str) -> Result<String, String> {
    obj.get(key)
        .ok_or_else(|| format!("missing key `{key}`"))?
        .as_str()
        .map(ToString::to_string)
}

fn num_field(obj: &std::collections::BTreeMap<String, JSONValue>, key: &str) -> usize {
    match obj.get(key) {
        Some(JSONValue::Number(n)) => *n as usize,
        Some(JSONValue::Flt(n)) => *n as usize,
        _ => 0,
    }
}

fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    let source_metadata = std::fs::symlink_metadata(src)?;
    if source_metadata.file_type().is_symlink() || !source_metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("build scratch source is not a real directory: {}", src.display()),
        ));
    }
    std::fs::create_dir(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let metadata = std::fs::symlink_metadata(&from)?;
        if metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("build scratch contains a symlink: {}", from.display()),
            ));
        }
        if metadata.is_dir() {
            copy_tree(&from, &to)?;
        } else if metadata.is_file() {
            std::fs::copy(&from, &to)?;
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("build scratch contains an unsupported entry: {}", from.display()),
            ));
        }
    }
    Ok(())
}

fn ensure_real_directory(path: &Path, label: &str) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{label} is not a real directory"),
            ))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            std::fs::create_dir_all(path)?;
            ensure_real_directory(path, label)
        }
        Err(error) => Err(error),
    }
}

fn ensure_absent(path: &Path, label: &str) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{label} already exists: {}", path.display()),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn remove_tree_without_following(path: &Path) -> io::Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return std::fs::remove_file(path);
    }
    for entry in std::fs::read_dir(path)? {
        remove_tree_without_following(&entry?.path())?;
    }
    std::fs::remove_dir(path)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("build log path has no parent"))?;
    ensure_real_directory(parent, "build log parent")?;
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "build log destination is a symlink",
            ));
        }
    }
    let sequence = WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::other("build log path has no file name"))?;
    let partial = parent.join(format!(".{name}.partial-{}-{sequence}", std::process::id()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&partial, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    result
}

fn redact(s: &str) -> String {
    s.replace("JET_SECRET=", "JET_SECRET=<redacted>")
        .replace("SECRET=", "SECRET=<redacted>")
}

fn empty_dash(s: &str) -> &str {
    if s.is_empty() {
        "-"
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attempt_json_round_trips() {
        let mut a = Attempt::new(
            "weirdctl",
            "adapt:weirdctl:./vendor/weirdctl",
            "adapter",
            "rh",
            "sh",
        );
        a.push_step(StepLog {
            index: 1,
            total: 1,
            name: "install".into(),
            command: "install missing bin/weirdctl".into(),
            cwd: "/tmp/nope".into(),
            status: "failed".into(),
            stdout: "ok".into(),
            stderr: "SECRET=value".into(),
        });
        let parsed = parse_attempt(&a.to_json()).unwrap();
        assert_eq!(parsed.package, "weirdctl");
        assert_eq!(parsed.failed_step, 1);
        assert!(parsed.to_json().contains("<redacted>"));
    }

    #[cfg(unix)]
    #[test]
    fn scratch_preservation_rejects_symlink_without_recording_a_path() {
        let root = std::env::temp_dir().join(format!(
            "jet-build-debug-{}-{}",
            std::process::id(),
            WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let source = root.join("source");
        let outside = root.join("outside");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("live"), "must survive").unwrap();
        std::os::unix::fs::symlink(&outside, source.join("escape")).unwrap();

        let mut attempt = Attempt::new("pkg", "pkg@1", "fixture", "recipe", "source");
        assert!(attempt
            .preserve_scratch(&root, &source, &root.join("missing-output"))
            .is_err());
        assert!(attempt.scratch_dir.is_empty());
        assert!(!root.join(SCRATCH_ROOT).join(&attempt.id).exists());
        assert_eq!(
            std::fs::read_to_string(outside.join("live")).unwrap(),
            "must survive"
        );
        let _ = remove_tree_without_following(&root);
    }

    #[test]
    fn recover_scratch_removes_only_unpublished_stages() {
        let root = std::env::temp_dir().join(format!(
            "jet-build-recover-{}-{}",
            std::process::id(),
            WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let hangar = root.join("hangar");
        let stage = hangar.join(SCRATCH_ROOT).join(".jet-build-stage-orphan");
        let published = hangar.join(SCRATCH_ROOT).join("pkg-1");
        std::fs::create_dir_all(&stage).unwrap();
        std::fs::create_dir_all(&published).unwrap();
        std::fs::write(stage.join("partial"), "partial").unwrap();
        std::fs::write(published.join("source"), "published").unwrap();

        assert_eq!(recover_scratch(&hangar).unwrap(), 1);
        assert!(!stage.exists());
        assert!(published.is_dir());
        let _ = remove_tree_without_following(&root);
    }
}
