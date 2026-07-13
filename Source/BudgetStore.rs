//! D-PERFBUDGET-REPORT1 durable report/baseline store.
//!
//! POSIX mutation uses descriptor-relative, no-follow operations, advisory
//! locks, create-new immutable objects, atomic manifest replacement, and file
//! plus directory durability. Other platforms stay read-only until they gain
//! equivalent primitives.

use jet_foundation::PerformanceBudget::{stable_id, verify_budget_report, CanonicalJson};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateKind {
    Pass,
    Bootstrap { reason: String },
    AcceptRegression { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdatePlan {
    pub baseline: String,
    pub report_id: String,
    pub report_bytes: Vec<u8>,
    pub prior_manifest_id: Option<String>,
    pub prior_head_report_id: Option<String>,
    pub accepted_at: String,
    pub kind: UpdateKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedUpdate {
    pub report_id: String,
    pub manifest_id: String,
    pub object_created: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryQuery {
    pub budget_id: String,
    pub budget_spec_sha256: String,
    pub context_key: String,
    pub maximum: usize,
}

pub struct BudgetStore {
    workspace: PathBuf,
}

impl BudgetStore {
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        Self { workspace: workspace.into() }
    }

    pub fn write_report(&self, bytes: &[u8]) -> Result<(String, bool), String> {
        let report = verify_budget_report(bytes).map_err(|e| format!("invalid budget report: {e}"))?;
        let id = report_id(&report)?;
        let dir = self.dir(&[".jet", "perf", "reports"], true, 0o755)?;
        let created = install_immutable(&dir, &format!("{id}.json"), bytes, |existing| {
            let value = verify_budget_report(existing).map_err(|e| format!("invalid existing report: {e}"))?;
            if report_id(&value)? != id { return Err("existing report filename/id mismatch".into()); }
            Ok(())
        })?;
        Ok((id, created))
    }

    pub fn plan_update(&self, baseline: &str, report: &[u8], kind: UpdateKind, accepted_at: &str) -> Result<UpdatePlan, String> {
        validate_baseline_name(baseline)?;
        validate_kind(&kind)?;
        validate_timestamp(accepted_at)?;
        let parsed = verify_budget_report(report).map_err(|e| format!("invalid budget report: {e}"))?;
        validate_update_evidence(&parsed, &kind)?;
        let current = self.read_manifest_optional(baseline)?;
        if current.is_none() && !matches!(kind, UpdateKind::Bootstrap { .. }) {
            return Err("baseline is absent; bootstrap with an explicit reason".into());
        }
        Ok(UpdatePlan {
            baseline: baseline.into(), report_id: report_id(&parsed)?, report_bytes: report.to_vec(),
            prior_manifest_id: current.as_ref().map(manifest_id).transpose()?,
            prior_head_report_id: current.as_ref().map(head_id).transpose()?,
            accepted_at: accepted_at.into(), kind,
        })
    }

    pub fn apply_update(&self, plan: &UpdatePlan) -> Result<AppliedUpdate, String> {
        validate_baseline_name(&plan.baseline)?;
        validate_kind(&plan.kind)?;
        validate_timestamp(&plan.accepted_at)?;
        let report = verify_budget_report(&plan.report_bytes).map_err(|e| format!("invalid budget report: {e}"))?;
        if report_id(&report)? != plan.report_id { return Err("planned report id changed".into()); }

        let _baseline_root = self.dir(&[".jet", "perf", "baselines"], true, 0o755)?;
        let locks = self.dir(&[".jet", "perf", "baselines", "locks"], true, 0o700)?;
        let _lock = Lock::take(&locks, &format!("{}.lock", plan.baseline.replace('/', "--")))?;
        let current = self.read_manifest_optional(&plan.baseline)?;
        if current.as_ref().map(manifest_id).transpose()? != plan.prior_manifest_id
            || current.as_ref().map(head_id).transpose()? != plan.prior_head_report_id {
            return Err("baseline changed after plan; re-plan against current head".into());
        }

        let objects = self.dir(&[".jet", "perf", "baselines", "objects"], true, 0o755)?;
        let object_created = install_immutable(&objects, &format!("{}.json", plan.report_id), &plan.report_bytes, |existing| {
            let value = verify_budget_report(existing).map_err(|e| format!("invalid existing object: {e}"))?;
            if report_id(&value)? != plan.report_id { return Err("existing object filename/id mismatch".into()); }
            Ok(())
        })?;
        let manifest = next_manifest(current.as_ref(), plan)?;
        let (parent, name) = self.manifest_parent(&plan.baseline, true)?;
        replace_atomic(&parent, &name, &manifest.bytes())?;
        Ok(AppliedUpdate { report_id: plan.report_id.clone(), manifest_id: manifest_id(&manifest)?, object_created })
    }

    pub fn select_compatible_history(&self, baseline: &str, candidate: &[u8], query: &HistoryQuery) -> Result<Vec<String>, String> {
        validate_baseline_name(baseline)?;
        if query.maximum == 0 { return Ok(Vec::new()); }
        let candidate = verify_budget_report(candidate).map_err(|e| format!("invalid candidate report: {e}"))?;
        let candidate_content = report_content(&candidate)?;
        let candidate_measurement = measurement(candidate_content, &query.budget_id)?;
        require_text(candidate_measurement, "budget_spec_sha256", &query.budget_spec_sha256)?;
        require_text(candidate_measurement, "context_key", &query.context_key)?;
        let manifest = self.read_manifest(baseline)?;
        let objects = self.dir(&[".jet", "perf", "baselines", "objects"], false, 0o755)?;
        let mut selected = Vec::new();
        for generation in generations(&manifest)?.iter().rev() {
            let generation = object(generation, "generation")?;
            let id = text(generation.get("report_id"), "generation.report_id")?;
            let bytes = read_regular(&objects, &format!("{id}.json"))?;
            let old = verify_budget_report(&bytes).map_err(|e| format!("corrupt baseline object {id}: {e}"))?;
            let old_content = report_content(&old)?;
            let Ok(old_measurement) = measurement(old_content, &query.budget_id) else { continue };
            if compatible(candidate_content, candidate_measurement, old_content, old_measurement) {
                selected.push(id.into());
                if selected.len() == query.maximum { break; }
            }
        }
        Ok(selected)
    }

    pub fn cache_identity(digests: &BTreeMap<String, String>) -> Result<String, String> {
        const REQUIRED: [&str; 9] = ["budget_spec", "source_inputs", "target", "profile", "toolchain", "provider", "workload", "policy", "privacy"];
        if digests.len() != REQUIRED.len() || REQUIRED.iter().any(|k| !digests.contains_key(*k)) {
            return Err(format!("cache identity requires exactly: {}", REQUIRED.join(", ")));
        }
        let fields = REQUIRED.into_iter().map(|key| {
            let value = &digests[key];
            if !is_hex64(value) { return Err(format!("cache digest {key} is not lowercase Hex64")); }
            Ok((key.into(), CanonicalJson::String(value.clone())))
        }).collect::<Result<Vec<_>, String>>()?;
        Ok(stable_id(&CanonicalJson::object(fields)?))
    }

    fn read_manifest(&self, baseline: &str) -> Result<CanonicalJson, String> {
        self.read_manifest_optional(baseline)?.ok_or_else(|| format!("baseline `{baseline}` is absent"))
    }

    fn read_manifest_optional(&self, baseline: &str) -> Result<Option<CanonicalJson>, String> {
        let (parent, name) = match self.manifest_parent(baseline, false) {
            Ok(value) => value,
            Err(error) if error.contains("No such file") => return Ok(None),
            Err(error) => return Err(error),
        };
        match read_regular(&parent, &name) {
            Ok(bytes) => Ok(Some(verify_manifest(&bytes, baseline)?)),
            Err(error) if error.contains("No such file") => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn manifest_parent(&self, baseline: &str, create: bool) -> Result<(File, String), String> {
        validate_baseline_name(baseline)?;
        let segments = baseline.split('/').collect::<Vec<_>>();
        let mut parts = vec![".jet", "perf", "baselines", "names"];
        parts.extend_from_slice(&segments[..segments.len() - 1]);
        Ok((self.dir(&parts, create, 0o755)?, format!("{}.json", segments.last().unwrap())))
    }

    fn dir(&self, parts: &[&str], create: bool, mode: u32) -> Result<File, String> {
        open_dir_chain(&self.workspace, parts, create, mode)
    }
}

fn compatible(a_content: &BTreeMap<String, CanonicalJson>, a: &BTreeMap<String, CanonicalJson>, b_content: &BTreeMap<String, CanonicalJson>, b: &BTreeMap<String, CanonicalJson>) -> bool {
    ["budget_spec_sha256", "metric", "unit", "direction", "comparison", "target_class", "provider", "context_key"].into_iter().all(|k| a.get(k) == b.get(k))
        && ["toolchain", "privacy"].into_iter().all(|k| a_content.get(k) == b_content.get(k))
        && ["target_triple", "target_class", "profile"].into_iter().all(|k| {
            let asubject = a_content.get("subject").and_then(|v| object(v, "subject").ok());
            let bsubject = b_content.get("subject").and_then(|v| object(v, "subject").ok());
            asubject.and_then(|v| v.get(k)) == bsubject.and_then(|v| v.get(k))
        })
}

fn next_manifest(current: Option<&CanonicalJson>, plan: &UpdatePlan) -> Result<CanonicalJson, String> {
    let (kind, reason, bootstrap, accept_regression) = match &plan.kind {
        UpdateKind::Pass => ("pass", CanonicalJson::Null, false, false),
        UpdateKind::Bootstrap { reason } => ("bootstrap", CanonicalJson::String(normalize_reason(reason)?), true, false),
        UpdateKind::AcceptRegression { reason } => ("exception", CanonicalJson::String(normalize_reason(reason)?), false, true),
    };
    let audit_body = CanonicalJson::object([
        ("accepted_at".into(), CanonicalJson::String(plan.accepted_at.clone())),
        ("actor_label".into(), CanonicalJson::String("local".into())),
        ("flags".into(), CanonicalJson::object([("accept_regression".into(), CanonicalJson::Bool(accept_regression)), ("bootstrap".into(), CanonicalJson::Bool(bootstrap))])?),
        ("kind".into(), CanonicalJson::String(kind.into())),
        ("prior_head_report_id".into(), plan.prior_head_report_id.clone().map(CanonicalJson::String).unwrap_or(CanonicalJson::Null)),
        ("prior_state_id".into(), plan.prior_manifest_id.clone().map(CanonicalJson::String).unwrap_or(CanonicalJson::Null)),
        ("reason".into(), reason),
        ("report_id".into(), CanonicalJson::String(plan.report_id.clone())),
    ])?;
    let mut audit = object(&audit_body, "audit")?.clone();
    audit.insert("audit_id".into(), CanonicalJson::String(stable_id(&audit_body)));
    let mut generations = current.map(generations).transpose()?.unwrap_or(&[]).to_vec();
    generations.push(CanonicalJson::object([("audit".into(), CanonicalJson::Object(audit)), ("report_id".into(), CanonicalJson::String(plan.report_id.clone()))])?);
    let content = CanonicalJson::object([
        ("generations".into(), CanonicalJson::Array(generations)),
        ("head_report_id".into(), CanonicalJson::String(plan.report_id.clone())),
        ("name".into(), CanonicalJson::String(plan.baseline.clone())),
    ])?;
    CanonicalJson::object([
        ("content".into(), content.clone()),
        ("manifest_id".into(), CanonicalJson::String(stable_id(&content))),
        ("schema".into(), CanonicalJson::String("jet.budget-manifest".into())),
        ("version".into(), CanonicalJson::Integer("1".into())),
    ])
}

fn verify_manifest(bytes: &[u8], expected_name: &str) -> Result<CanonicalJson, String> {
    let value = CanonicalJson::parse_canonical(bytes).map_err(|e| format!("invalid manifest: {e}"))?;
    let wrapper = exact_object(&value, "manifest", &["content", "manifest_id", "schema", "version"])?;
    require_text(wrapper, "schema", "jet.budget-manifest")?;
    if integer(wrapper.get("version"), "version")? != "1" { return Err("manifest version is not 1".into()); }
    let content_value = wrapper.get("content").unwrap();
    let content = exact_object(content_value, "manifest content", &["generations", "head_report_id", "name"])?;
    require_text(content, "name", expected_name)?;
    let id = text(wrapper.get("manifest_id"), "manifest_id")?;
    if !is_hex64(id) || stable_id(content_value) != id { return Err("manifest_id mismatch".into()); }
    let generations = array(content.get("generations"), "generations")?;
    if generations.is_empty() { return Err("manifest has no generations".into()); }
    for (index, generation) in generations.iter().enumerate() {
        let generation = exact_object(generation, "generation", &["audit", "report_id"])?;
        let report = text(generation.get("report_id"), "generation.report_id")?;
        if !is_hex64(report) { return Err("generation report_id is not lowercase Hex64".into()); }
        let audit = exact_object(generation.get("audit").unwrap(), "audit", &["accepted_at", "actor_label", "audit_id", "flags", "kind", "prior_head_report_id", "prior_state_id", "reason", "report_id"])?;
        require_text(audit, "actor_label", "local")?;
        require_text(audit, "report_id", report)?;
        validate_timestamp(text(audit.get("accepted_at"), "accepted_at")?)?;
        let flags = exact_object(audit.get("flags").unwrap(), "audit flags", &["accept_regression", "bootstrap"])?;
        let bootstrap = boolean(flags.get("bootstrap"), "flags.bootstrap")?;
        let accept = boolean(flags.get("accept_regression"), "flags.accept_regression")?;
        let kind = text(audit.get("kind"), "audit.kind")?;
        let reason = nullable_text(audit.get("reason"), "audit.reason")?;
        match (kind, bootstrap, accept, reason) {
            ("pass", false, false, None) => {}
            ("bootstrap", true, false, Some(reason)) | ("exception", false, true, Some(reason)) => { normalize_reason(reason)?; }
            _ => return Err("audit kind, flags, and reason disagree".into()),
        }
        let prior_state = nullable_text(audit.get("prior_state_id"), "prior_state_id")?;
        let prior_head = nullable_text(audit.get("prior_head_report_id"), "prior_head_report_id")?;
        if index == 0 {
            if prior_state.is_some() || prior_head.is_some() { return Err("first generation has prior CAS links".into()); }
        } else {
            let previous = object(&generations[index - 1], "generation")?;
            let expected_head = text(previous.get("report_id"), "report_id")?;
            let prefix_content = CanonicalJson::object([
                ("generations".into(), CanonicalJson::Array(generations[..index].to_vec())),
                ("head_report_id".into(), CanonicalJson::String(expected_head.into())),
                ("name".into(), CanonicalJson::String(expected_name.into())),
            ])?;
            let expected_state = stable_id(&prefix_content);
            if prior_state != Some(expected_state.as_str()) || prior_head != Some(expected_head) { return Err("manifest generation CAS chain is broken".into()); }
        }
        let mut body = audit.clone();
        let audit_id_value = body.remove("audit_id").ok_or("audit_id is absent")?;
        let audit_id = text(Some(&audit_id_value), "audit_id")?.to_owned();
        if !is_hex64(&audit_id) || stable_id(&CanonicalJson::Object(body)) != audit_id { return Err("audit_id mismatch".into()); }
    }
    if head_id(&value)? != text(object(generations.last().unwrap(), "generation")?.get("report_id"), "report_id")? { return Err("manifest head is not newest generation".into()); }
    Ok(value)
}

fn report_id(value: &CanonicalJson) -> Result<String, String> { Ok(text(object(value, "report")?.get("report_id"), "report_id")?.into()) }
fn report_content(value: &CanonicalJson) -> Result<&BTreeMap<String, CanonicalJson>, String> { object(object(value, "report")?.get("content").unwrap(), "report content") }
fn manifest_id(value: &CanonicalJson) -> Result<String, String> { Ok(text(object(value, "manifest")?.get("manifest_id"), "manifest_id")?.into()) }
fn manifest_content(value: &CanonicalJson) -> Result<&BTreeMap<String, CanonicalJson>, String> { object(object(value, "manifest")?.get("content").unwrap(), "manifest content") }
fn head_id(value: &CanonicalJson) -> Result<String, String> { Ok(text(manifest_content(value)?.get("head_report_id"), "head_report_id")?.into()) }
fn generations(value: &CanonicalJson) -> Result<&[CanonicalJson], String> { array(manifest_content(value)?.get("generations"), "generations") }

fn measurement<'a>(content: &'a BTreeMap<String, CanonicalJson>, budget_id: &str) -> Result<&'a BTreeMap<String, CanonicalJson>, String> {
    let mut found = None;
    for measurement in array(content.get("measurements"), "measurements")? {
        let measurement = object(measurement, "measurement")?;
        if text(measurement.get("budget_id"), "budget_id")? == budget_id {
            if found.replace(measurement).is_some() { return Err(format!("duplicate budget_id `{budget_id}`")); }
        }
    }
    found.ok_or_else(|| format!("measurement `{budget_id}` is absent"))
}

pub fn validate_baseline_name(value: &str) -> Result<(), String> {
    if value.is_empty() || value.starts_with('/') || value.ends_with('/') { return Err("invalid BaselineName".into()); }
    for segment in value.split('/') {
        if segment.is_empty() || segment.starts_with('-') || segment.ends_with('-') || segment.contains("--") || !segment.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') { return Err("invalid BaselineName".into()); }
    }
    Ok(())
}

fn validate_kind(kind: &UpdateKind) -> Result<(), String> { match kind { UpdateKind::Pass => Ok(()), UpdateKind::Bootstrap { reason } | UpdateKind::AcceptRegression { reason } => normalize_reason(reason).map(|_| ()) } }
fn normalize_reason(value: &str) -> Result<String, String> { if value.is_empty() || value.trim() != value || value.chars().count() > 512 || value.chars().any(char::is_control) { Err("acceptance reason must be trimmed, control-free, and 1..=512 scalars".into()) } else { Ok(value.into()) } }
fn validate_update_evidence(report:&CanonicalJson,kind:&UpdateKind)->Result<(),String>{
    let measurements=array(report_content(report)?.get("measurements"),"measurements")?;
    if measurements.is_empty(){return Err("update report has no measurements".into())}
    let mut regressed=false;
    for measurement in measurements { let measurement=object(measurement,"measurement")?;let decision=object(measurement.get("decision").ok_or("measurement decision is absent")?,"decision")?;let evidence=text(decision.get("evidence"),"decision.evidence")?;match kind{UpdateKind::Pass if evidence!="pass"=>return Err("plain update requires every measurement to pass".into()),UpdateKind::Bootstrap{..} if !matches!(evidence,"pass"|"unavailable")=>return Err("bootstrap rejects regression and inconclusive evidence".into()),UpdateKind::AcceptRegression{..} if !matches!(evidence,"pass"|"regression"|"inconclusive")=>return Err("accept-regression rejects unavailable evidence".into()),_=>{}}if matches!(evidence,"regression"|"inconclusive"){regressed=true;}}
    if matches!(kind,UpdateKind::AcceptRegression{..})&&!regressed{return Err("accept-regression requires regression or inconclusive evidence".into())}Ok(())
}
fn validate_timestamp(value: &str) -> Result<(), String> { let b=value.as_bytes();let punctuation=[(4,b'-'),(7,b'-'),(10,b'T'),(13,b':'),(16,b':'),(19,b'.'),(29,b'Z')];if b.len()!=30||punctuation.iter().any(|(i,v)|b[*i]!=*v)||b.iter().enumerate().any(|(i,v)|!punctuation.iter().any(|(p,_)|*p==i)&&!v.is_ascii_digit()){return Err("accepted_at is not RFC3339UTC with nine fractional digits".into())}let number=|range:std::ops::Range<usize>|std::str::from_utf8(&b[range]).unwrap().parse::<u32>().unwrap();let month=number(5..7);let day=number(8..10);let hour=number(11..13);let minute=number(14..16);let second=number(17..19);if !(1..=12).contains(&month)||!(1..=31).contains(&day)||hour>23||minute>59||second>59{return Err("accepted_at contains an out-of-range UTC field".into())}Ok(()) }
fn is_hex64(value:&str)->bool{value.len()==64&&value.bytes().all(|b|b.is_ascii_hexdigit()&&!b.is_ascii_uppercase())}
fn exact_object<'a>(value:&'a CanonicalJson,name:&str,keys:&[&str])->Result<&'a BTreeMap<String,CanonicalJson>,String>{let f=object(value,name)?;if f.len()!=keys.len()||keys.iter().any(|k|!f.contains_key(*k)){Err(format!("{name} has missing or unknown fields"))}else{Ok(f)}}
fn object<'a>(value:&'a CanonicalJson,name:&str)->Result<&'a BTreeMap<String,CanonicalJson>,String>{match value{CanonicalJson::Object(v)=>Ok(v),_=>Err(format!("{name} is not an object"))}}
fn array<'a>(value:Option<&'a CanonicalJson>,name:&str)->Result<&'a[CanonicalJson],String>{match value{Some(CanonicalJson::Array(v))=>Ok(v),_=>Err(format!("{name} is not an array"))}}
fn text<'a>(value:Option<&'a CanonicalJson>,name:&str)->Result<&'a str,String>{match value{Some(CanonicalJson::String(v))=>Ok(v),_=>Err(format!("{name} is not text"))}}
fn integer<'a>(value:Option<&'a CanonicalJson>,name:&str)->Result<&'a str,String>{match value{Some(CanonicalJson::Integer(v))=>Ok(v),_=>Err(format!("{name} is not integer"))}}
fn boolean(value:Option<&CanonicalJson>,name:&str)->Result<bool,String>{match value{Some(CanonicalJson::Bool(v))=>Ok(*v),_=>Err(format!("{name} is not boolean"))}}
fn nullable_text<'a>(value:Option<&'a CanonicalJson>,name:&str)->Result<Option<&'a str>,String>{match value{Some(CanonicalJson::Null)=>Ok(None),Some(CanonicalJson::String(v))=>Ok(Some(v)),_=>Err(format!("{name} is not text or null"))}}
fn require_text(fields:&BTreeMap<String,CanonicalJson>,key:&str,expected:&str)->Result<(),String>{if text(fields.get(key),key)?==expected{Ok(())}else{Err(format!("{key} mismatch"))}}

#[cfg(unix)]
fn open_dir_chain(root:&Path,parts:&[&str],create:bool,mode:u32)->Result<File,String>{
    use std::ffi::CString;use std::os::fd::{AsRawFd,FromRawFd};use std::os::unix::fs::OpenOptionsExt;
    const O_RDONLY:i32=0;const O_CLOEXEC:i32=0o2000000;const O_DIRECTORY:i32=0o200000;const O_NOFOLLOW:i32=0o400000;
    extern "C"{fn openat(fd:i32,path:*const i8,flags:i32,mode:u32)->i32;fn mkdirat(fd:i32,path:*const i8,mode:u32)->i32;}
    let mut dir=OpenOptions::new().read(true).custom_flags(O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC).open(root).map_err(|e|format!("cannot securely open workspace: {e}"))?;
    for part in parts{let name=CString::new(*part).map_err(|_|"NUL in store path")?;let mut fd=unsafe{openat(dir.as_raw_fd(),name.as_ptr(),O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC,0)};if fd<0&&create&&std::io::Error::last_os_error().kind()==ErrorKind::NotFound{if unsafe{mkdirat(dir.as_raw_fd(),name.as_ptr(),mode)}!=0&&std::io::Error::last_os_error().kind()!=ErrorKind::AlreadyExists{return Err(format!("cannot create store directory: {}",std::io::Error::last_os_error()));}fd=unsafe{openat(dir.as_raw_fd(),name.as_ptr(),O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC,0)};}if fd<0{return Err(format!("cannot securely open store directory: {}",std::io::Error::last_os_error()));}dir=unsafe{File::from_raw_fd(fd)};}Ok(dir)
}
#[cfg(not(unix))]fn open_dir_chain(_: &Path,_:&[&str],_:bool,_:u32)->Result<File,String>{Err("performance baseline mutation unavailable on this platform".into())}

#[cfg(unix)]
fn read_regular(dir:&File,name:&str)->Result<Vec<u8>,String>{use std::ffi::CString;use std::os::fd::{AsRawFd,FromRawFd};use std::os::unix::fs::MetadataExt;const O_RDONLY:i32=0;const O_CLOEXEC:i32=0o2000000;const O_NOFOLLOW:i32=0o400000;extern "C"{fn openat(fd:i32,path:*const i8,flags:i32,mode:u32)->i32;}let name=CString::new(name).map_err(|_|"NUL in artifact name")?;let fd=unsafe{openat(dir.as_raw_fd(),name.as_ptr(),O_RDONLY|O_NOFOLLOW|O_CLOEXEC,0)};if fd<0{return Err(format!("cannot open artifact: {}",std::io::Error::last_os_error()));}let mut file=unsafe{File::from_raw_fd(fd)};let meta=file.metadata().map_err(|e|e.to_string())?;if !meta.is_file()||meta.nlink()!=1{return Err("artifact is linked or not regular".into());}let mut out=Vec::new();file.read_to_end(&mut out).map_err(|e|e.to_string())?;Ok(out)}
#[cfg(not(unix))]fn read_regular(_: &File,_:&str)->Result<Vec<u8>,String>{Err("secure artifact read unavailable on this platform".into())}

static NONCE:AtomicU64=AtomicU64::new(0);
#[cfg(unix)]
fn temp(dir:&File,bytes:&[u8])->Result<(File,String),String>{use std::ffi::CString;use std::os::fd::{AsRawFd,FromRawFd};const O_WRONLY:i32=1;const O_CREAT:i32=0o100;const O_EXCL:i32=0o200;const O_CLOEXEC:i32=0o2000000;const O_NOFOLLOW:i32=0o400000;extern "C"{fn openat(fd:i32,path:*const i8,flags:i32,mode:u32)->i32;}for _ in 0..32{let name=format!(".tmp-{}-{}",std::process::id(),NONCE.fetch_add(1,Ordering::Relaxed));let cname=CString::new(name.as_str()).unwrap();let fd=unsafe{openat(dir.as_raw_fd(),cname.as_ptr(),O_WRONLY|O_CREAT|O_EXCL|O_NOFOLLOW|O_CLOEXEC,0o600)};if fd<0{if std::io::Error::last_os_error().kind()==ErrorKind::AlreadyExists{continue}return Err(format!("cannot create temp: {}",std::io::Error::last_os_error()));}let mut file=unsafe{File::from_raw_fd(fd)};file.write_all(bytes).map_err(|e|e.to_string())?;file.sync_all().map_err(|e|e.to_string())?;return Ok((file,name));}Err("cannot allocate temp".into())}
#[cfg(not(unix))]fn temp(_: &File,_:&[u8])->Result<(File,String),String>{Err("secure temp unavailable on this platform".into())}

#[cfg(unix)]fn unlink(dir:&File,name:&str){use std::ffi::CString;use std::os::fd::AsRawFd;extern "C"{fn unlinkat(fd:i32,path:*const i8,flags:i32)->i32;}if let Ok(name)=CString::new(name){unsafe{unlinkat(dir.as_raw_fd(),name.as_ptr(),0)};}}

fn install_immutable(dir:&File,name:&str,bytes:&[u8],verify:impl Fn(&[u8])->Result<(),String>)->Result<bool,String>{match read_regular(dir,name){Ok(existing)=>{verify(&existing)?;if existing==bytes{return Ok(false)}return Err("immutable artifact differs from candidate".into())},Err(e)if !e.contains("No such file")=>return Err(e),Err(_)=>{}}let (file,tmp)=temp(dir,bytes)?;artifact_permissions(&file)?;#[cfg(unix)]{use std::ffi::CString;use std::os::fd::AsRawFd;extern "C"{fn linkat(oldfd:i32,old:*const i8,newfd:i32,new:*const i8,flags:i32)->i32;}let old=CString::new(tmp.as_str()).unwrap();let new=CString::new(name).unwrap();if unsafe{linkat(dir.as_raw_fd(),old.as_ptr(),dir.as_raw_fd(),new.as_ptr(),0)}!=0{let error=std::io::Error::last_os_error();unlink(dir,&tmp);if error.kind()==ErrorKind::AlreadyExists{let existing=read_regular(dir,name)?;verify(&existing)?;if existing==bytes{return Ok(false)}}return Err(format!("cannot atomically install artifact: {error}"));}unlink(dir,&tmp);dir.sync_all().map_err(|e|e.to_string())?;Ok(true)}#[cfg(not(unix))]{let _=tmp;Err("atomic no-replace unavailable on this platform".into())}}

fn replace_atomic(dir:&File,name:&str,bytes:&[u8])->Result<(),String>{let (file,tmp)=temp(dir,bytes)?;artifact_permissions(&file)?;#[cfg(unix)]{use std::ffi::CString;use std::os::fd::AsRawFd;extern "C"{fn renameat(oldfd:i32,old:*const i8,newfd:i32,new:*const i8)->i32;}let old=CString::new(tmp.as_str()).unwrap();let new=CString::new(name).unwrap();if unsafe{renameat(dir.as_raw_fd(),old.as_ptr(),dir.as_raw_fd(),new.as_ptr())}!=0{let error=std::io::Error::last_os_error();unlink(dir,&tmp);return Err(format!("cannot atomically replace manifest: {error}"));}dir.sync_all().map_err(|e|e.to_string())?;Ok(())}#[cfg(not(unix))]{let _=(dir,name,tmp);Err("atomic replacement unavailable on this platform".into())}}

#[cfg(unix)]fn artifact_permissions(file:&File)->Result<(),String>{use std::os::unix::fs::PermissionsExt;file.set_permissions(std::fs::Permissions::from_mode(0o644)).map_err(|e|e.to_string())?;file.sync_all().map_err(|e|e.to_string())}
#[cfg(not(unix))]fn artifact_permissions(_: &File)->Result<(),String>{Err("artifact permissions unavailable on this platform".into())}

#[cfg(unix)]struct Lock{file:File}
#[cfg(unix)]impl Lock{fn take(dir:&File,name:&str)->Result<Self,String>{use std::ffi::CString;use std::os::fd::{AsRawFd,FromRawFd};use std::os::unix::fs::MetadataExt;const O_RDWR:i32=2;const O_CREAT:i32=0o100;const O_CLOEXEC:i32=0o2000000;const O_NOFOLLOW:i32=0o400000;const LOCK_EX:i32=2;extern "C"{fn openat(fd:i32,path:*const i8,flags:i32,mode:u32)->i32;fn flock(fd:i32,operation:i32)->i32;}let name=CString::new(name).map_err(|_|"NUL in lock")?;let fd=unsafe{openat(dir.as_raw_fd(),name.as_ptr(),O_RDWR|O_CREAT|O_NOFOLLOW|O_CLOEXEC,0o600)};if fd<0{return Err(format!("cannot securely open lock: {}",std::io::Error::last_os_error()));}let file=unsafe{File::from_raw_fd(fd)};let meta=file.metadata().map_err(|e|e.to_string())?;if !meta.is_file()||meta.nlink()!=1{return Err("baseline lock is linked or not regular".into())}if unsafe{flock(file.as_raw_fd(),LOCK_EX)}!=0{return Err(format!("cannot lock baseline: {}",std::io::Error::last_os_error()));}Ok(Self{file})}}
#[cfg(unix)]impl Drop for Lock{fn drop(&mut self){use std::os::fd::AsRawFd;const LOCK_UN:i32=8;extern "C"{fn flock(fd:i32,operation:i32)->i32;}unsafe{flock(self.file.as_raw_fd(),LOCK_UN)};}}
#[cfg(not(unix))]struct Lock;
#[cfg(not(unix))]impl Lock{fn take(_: &File,_:&str)->Result<Self,String>{Err("advisory lock unavailable on this platform".into())}}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn baseline_names_and_reasons_are_closed() {
        assert!(validate_baseline_name("release/x86-linux").is_ok());
        for bad in ["", "/x", "x/", "x//y", "X", "x_", "x/../y", "x--y"] { assert!(validate_baseline_name(bad).is_err(), "{bad}"); }
        assert!(normalize_reason(" reviewed regression ").is_err());
        assert!(normalize_reason("reviewed regression").is_ok());
    }

    #[test]
    fn cache_identity_covers_every_ratified_digest() {
        let keys = ["budget_spec", "source_inputs", "target", "profile", "toolchain", "provider", "workload", "policy", "privacy"];
        let mut values = BTreeMap::new();
        for (index, key) in keys.into_iter().enumerate() { values.insert(key.into(), format!("{index:064x}")); }
        let base = BudgetStore::cache_identity(&values).unwrap();
        for key in keys { let mut changed=values.clone();changed.insert(key.into(),"f".repeat(64));assert_ne!(BudgetStore::cache_identity(&changed).unwrap(),base,"{key}"); }
        values.remove("privacy");
        assert!(BudgetStore::cache_identity(&values).is_err());
    }

    #[test]
    fn manifests_hash_audits_and_enforce_cas_chain() {
        let first = UpdatePlan { baseline:"release/linux".into(),report_id:"1".repeat(64),report_bytes:vec![],prior_manifest_id:None,prior_head_report_id:None,accepted_at:"2026-07-13T12:00:00.000000000Z".into(),kind:UpdateKind::Bootstrap{reason:"initial baseline".into()} };
        let m1=next_manifest(None,&first).unwrap();
        verify_manifest(&m1.bytes(),"release/linux").unwrap();
        let second = UpdatePlan { baseline:first.baseline.clone(),report_id:"2".repeat(64),report_bytes:vec![],prior_manifest_id:Some(manifest_id(&m1).unwrap()),prior_head_report_id:Some(first.report_id.clone()),accepted_at:"2026-07-13T13:00:00.000000000Z".into(),kind:UpdateKind::Pass };
        let m2=next_manifest(Some(&m1),&second).unwrap();
        verify_manifest(&m2.bytes(),"release/linux").unwrap();
        let mut corrupt=m2.clone();
        let wrapper=match &mut corrupt{CanonicalJson::Object(v)=>v,_=>unreachable!()};
        let content=match wrapper.get_mut("content").unwrap(){CanonicalJson::Object(v)=>v,_=>unreachable!()};
        let generations=match content.get_mut("generations").unwrap(){CanonicalJson::Array(v)=>v,_=>unreachable!()};
        let second=match &mut generations[1]{CanonicalJson::Object(v)=>v,_=>unreachable!()};
        let audit=match second.get_mut("audit").unwrap(){CanonicalJson::Object(v)=>v,_=>unreachable!()};
        audit.insert("prior_state_id".into(),CanonicalJson::String("f".repeat(64)));
        let mut body=audit.clone();body.remove("audit_id");audit.insert("audit_id".into(),CanonicalJson::String(stable_id(&CanonicalJson::Object(body))));
        let new_content=CanonicalJson::Object(content.clone());wrapper.insert("manifest_id".into(),CanonicalJson::String(stable_id(&new_content)));
        assert!(verify_manifest(&corrupt.bytes(),"release/linux").unwrap_err().contains("CAS chain"));
    }

    #[test]
    fn update_kinds_reject_wrong_evidence_before_mutation() {
        let report=|evidence:&str|CanonicalJson::object([("content".into(),CanonicalJson::object([("measurements".into(),CanonicalJson::Array(vec![CanonicalJson::object([("decision".into(),CanonicalJson::object([("evidence".into(),CanonicalJson::String(evidence.into()))]).unwrap())]).unwrap()]))]).unwrap())]).unwrap();
        assert!(validate_update_evidence(&report("pass"),&UpdateKind::Pass).is_ok());
        assert!(validate_update_evidence(&report("regression"),&UpdateKind::Pass).is_err());
        assert!(validate_update_evidence(&report("regression"),&UpdateKind::AcceptRegression{reason:"reviewed".into()}).is_ok());
        assert!(validate_update_evidence(&report("pass"),&UpdateKind::AcceptRegression{reason:"reviewed".into()}).is_err());
        assert!(validate_update_evidence(&report("unavailable"),&UpdateKind::Bootstrap{reason:"initial".into()}).is_ok());
        assert!(validate_update_evidence(&report("unavailable"),&UpdateKind::AcceptRegression{reason:"reviewed".into()}).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn immutable_install_is_nofollow_noreplace_and_byte_exact() {
        use std::os::unix::fs::symlink;
        let root=std::env::temp_dir().join(format!("jet-budget-store-{}-{}",std::process::id(),NONCE.fetch_add(1,Ordering::Relaxed)));
        std::fs::create_dir(&root).unwrap();let dir=open_dir_chain(&root,&["objects"],true,0o755).unwrap();
        assert!(install_immutable(&dir,"a.json",b"one\n",|_|Ok(())).unwrap());
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(root.join("objects/a.json")).unwrap().permissions().mode()&0o777,0o644);
        assert!(!install_immutable(&dir,"a.json",b"one\n",|_|Ok(())).unwrap());
        assert!(install_immutable(&dir,"a.json",b"two\n",|_|Ok(())).is_err());
        symlink("/etc/passwd",root.join("objects/evil.json")).unwrap();
        assert!(read_regular(&dir,"evil.json").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
