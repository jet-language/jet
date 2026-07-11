use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use super::{fail, hash_bytes, json_escape};

pub struct Lock { path: PathBuf, _file: File }
impl Drop for Lock { fn drop(&mut self) { let _ = fs::remove_file(&self.path); } }

#[derive(Clone)]
pub struct Change { pub path: PathBuf, pub before: Vec<u8>, pub after: Vec<u8> }

pub fn lock(project: &Path) -> Lock {
    let dir = project.join(".jet/codemods");
    fs::create_dir_all(&dir).unwrap_or_else(|e| fail(&format!("could not create `{}`: {e}", dir.display())));
    let path = dir.join("codemod.lock");
    let mut file = OpenOptions::new().write(true).create_new(true).open(&path)
        .unwrap_or_else(|_| fail(&format!("another codemod holds `{}`", path.display())));
    writeln!(file, "pid={}", std::process::id()).unwrap_or_else(|e| fail(&format!("could not write codemod lock: {e}")));
    file.sync_all().unwrap_or_else(|e| fail(&format!("could not sync codemod lock: {e}")));
    Lock { path, _file: file }
}

pub fn recover(project: &Path) {
    let journal = project.join(".jet/codemods/transaction.journal");
    if !journal.exists() { return; }
    let raw = fs::read_to_string(&journal).unwrap_or_else(|e| fail(&format!("could not read recovery journal: {e}")));
    let records = parse_records(&raw);
    let mut conflict = Vec::new();
    for record in &records {
        let current = fs::read(&record.path).unwrap_or_default();
        if current == record.before { continue; }
        if current == record.after {
            atomic_restore(&record.path, &record.before);
        } else {
            conflict.push(format!("{} (current {}, before {}, after {})", record.path.display(), hash_bytes(&current), hash_bytes(&record.before), hash_bytes(&record.after)));
        }
    }
    if !conflict.is_empty() {
        fail(&format!("codemod recovery found concurrent drift; journal preserved:\n  {}", conflict.join("\n  ")));
    }
    for record in &records { let _ = fs::remove_file(&record.temp); }
    fs::remove_file(&journal).unwrap_or_else(|e| fail(&format!("could not remove recovered journal: {e}")));
    sync_dir(journal.parent().unwrap());
}

pub fn commit(project: &Path, changes: &[Change], log_path: &Path, log: &[u8]) {
    if changes.is_empty() { fail("codemod has no file edits"); }
    for c in changes {
        let current = fs::read(&c.path).unwrap_or_else(|e| fail(&format!("could not re-read `{}`: {e}", c.path.display())));
        if current != c.before { fail(&format!("observed drift for `{}` before commit; no files written", c.path.display())); }
    }
    let dir = project.join(".jet/codemods");
    let journal = dir.join("transaction.journal");
    let tx = format!("{}-{}", std::process::id(), now_nanos());
    let mut records = Vec::new();
    for (i, c) in changes.iter().enumerate() {
        let parent = c.path.parent().unwrap_or(project);
        let temp = parent.join(format!(".jet-codemod-{tx}-{i}.tmp"));
        write_sync(&temp, &c.after);
        records.push(Record { path: c.path.clone(), temp, before: c.before.clone(), after: c.after.clone() });
    }
    let journal_text = render_journal(&tx, &records);
    write_sync(&journal, journal_text.as_bytes());
    sync_dir(&dir);
    for (i, record) in records.iter().enumerate() {
        fs::rename(&record.temp, &record.path).unwrap_or_else(|e| {
            rollback(&records, &journal);
            fail(&format!("codemod rename failed for `{}`: {e}", record.path.display()))
        });
        sync_dir(record.path.parent().unwrap_or(project));
        if std::env::var("JET_CODEMOD_CRASH_AFTER_RENAME").ok().as_deref() == Some(&(i + 1).to_string()) {
            std::process::exit(86);
        }
    }
    write_sync(log_path, log);
    sync_dir(log_path.parent().unwrap_or(&dir));
    fs::remove_file(&journal).unwrap_or_else(|e| fail(&format!("could not remove transaction journal: {e}")));
    sync_dir(&dir);
}

fn rollback(records: &[Record], journal: &Path) {
    let mut conflict = false;
    for r in records {
        let current = fs::read(&r.path).unwrap_or_default();
        if current == r.after { atomic_restore(&r.path, &r.before); }
        else if current != r.before { conflict = true; }
        let _ = fs::remove_file(&r.temp);
    }
    if !conflict { let _ = fs::remove_file(journal); }
}

#[derive(Clone)] struct Record { path: PathBuf, temp: PathBuf, before: Vec<u8>, after: Vec<u8> }
fn render_journal(tx: &str, records: &[Record]) -> String {
    let rows = records.iter().map(|r| format!("{{\"path\":\"{}\",\"temp\":\"{}\",\"before\":\"{}\",\"after\":\"{}\"}}", json_escape(&r.path.display().to_string()), json_escape(&r.temp.display().to_string()), hex(&r.before), hex(&r.after))).collect::<Vec<_>>().join(",");
    format!("{{\"schema\":1,\"tx\":\"{}\",\"files\":[{}]}}\n", json_escape(tx), rows)
}
fn parse_records(raw: &str) -> Vec<Record> {
    let root = super::Json::parse(raw).and_then(|v| v.object()).unwrap_or_else(|e| fail(&format!("invalid recovery journal: {e}")));
    let files = match root.get("files") { Some(super::Json::Value::Array(v)) => v, _ => fail("invalid recovery journal files") };
    files.iter().map(|v| {
        let o = match v { super::Json::Value::Object(o) => o, _ => fail("invalid recovery journal record") };
        let s = |k| match o.get(k) { Some(super::Json::Value::String(s)) => s.clone(), _ => fail(&format!("recovery journal missing `{k}`")) };
        Record { path: PathBuf::from(s("path")), temp: PathBuf::from(s("temp")), before: unhex(&s("before")), after: unhex(&s("after")) }
    }).collect()
}
fn write_sync(path: &Path, bytes: &[u8]) { let mut f = File::create(path).unwrap_or_else(|e| fail(&format!("could not write `{}`: {e}", path.display()))); f.write_all(bytes).unwrap_or_else(|e| fail(&format!("could not write `{}`: {e}", path.display()))); f.sync_all().unwrap_or_else(|e| fail(&format!("could not sync `{}`: {e}", path.display()))); }
fn atomic_restore(path: &Path, bytes: &[u8]) { let tmp = path.with_extension(format!("recover-{}.tmp", std::process::id())); write_sync(&tmp, bytes); fs::rename(&tmp, path).unwrap_or_else(|e| fail(&format!("could not recover `{}`: {e}", path.display()))); sync_dir(path.parent().unwrap()); }
fn sync_dir(path: &Path) { File::open(path).and_then(|f| f.sync_all()).unwrap_or_else(|e| fail(&format!("could not sync directory `{}`: {e}", path.display()))); }
fn now_nanos() -> u128 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() }
fn hex(bytes: &[u8]) -> String { let mut out=String::with_capacity(bytes.len()*2); for b in bytes { out.push_str(&format!("{b:02x}")); } out }
fn unhex(s: &str) -> Vec<u8> { if s.len()%2!=0 { fail("invalid recovery journal byte encoding"); } (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i+2],16).unwrap_or_else(|_| fail("invalid recovery journal byte encoding"))).collect() }
