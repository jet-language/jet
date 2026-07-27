//! U27 (D-JPK-BUILDDBG1=A): failed-build logs, preserved scratch, explain.

use super::JSON::{self, JSONValue};
use std::path::{Path, PathBuf};

const LOG_ROOT: &str = "build-logs";
const SCRATCH_ROOT: &str = "failed-scratch";

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
        let dir = log_dir(hangar_dir, &self.package).join(&self.id);
        std::fs::create_dir_all(&dir)?;
        self.log_dir = dir.to_string_lossy().into_owned();
        for step in &self.steps {
            let stem = format!("{:02}-{}", step.index, safe_name(&step.name));
            std::fs::write(dir.join(format!("{stem}.stdout.log")), &step.stdout)?;
            std::fs::write(dir.join(format!("{stem}.stderr.log")), &step.stderr)?;
        }
        let json = self.to_json();
        std::fs::write(dir.join("attempt.json"), &json)?;
        std::fs::write(log_dir(hangar_dir, &self.package).join("latest.json"), json)?;
        Ok(())
    }

    pub fn preserve_scratch(&mut self, hangar_dir: &Path, source: &Path, output: &Path) {
        let dir = hangar_dir.join(SCRATCH_ROOT).join(&self.id);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let _ = copy_tree(source, &dir.join("source"));
        if output.exists() {
            let _ = copy_tree(output, &dir.join("output"));
        }
        self.scratch_dir = dir.to_string_lossy().into_owned();
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

pub fn latest(hangar_dir: &Path, package: &str) -> Result<Option<Attempt>, String> {
    let path = log_dir(hangar_dir, package).join("latest.json");
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    parse_attempt(&text).map(Some)
}

pub fn latest_json(hangar_dir: &Path, package: &str) -> Result<Option<String>, String> {
    let path = log_dir(hangar_dir, package).join("latest.json");
    if !path.exists() {
        return Ok(None);
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
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn log_dir(hangar_dir: &Path, package: &str) -> PathBuf {
    hangar_dir.join(LOG_ROOT).join(safe_name(package))
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
        Some(JSONValue::Num(n)) => *n as usize,
        _ => 0,
    }
}

fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
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
}
