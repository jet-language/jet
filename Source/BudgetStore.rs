//! D-PERFBUDGET-REPORT1 durable report/baseline store.
//!
//! Linux mutation uses descriptor-relative, no-follow operations, advisory
//! locks, create-new immutable objects, atomic manifest replacement, and file
//! plus directory durability. Other platforms stay read-only until they gain
//! equivalent primitives.

use jet_foundation::PerformanceBudget::{stable_id, verify_budget_report, CanonicalJson};
use std::collections::{BTreeMap, BTreeSet};
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
    baseline: String,
    report_id: String,
    report_bytes: Vec<u8>,
    prior_manifest_id: Option<String>,
    prior_head_report_id: Option<String>,
    accepted_at: String,
    kind: UpdateKind,
}

impl UpdatePlan {
    pub fn baseline(&self)->&str{&self.baseline}
    pub fn report_id(&self)->&str{&self.report_id}
    pub fn prior_head_report_id(&self)->Option<&str>{self.prior_head_report_id.as_deref()}
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
    pub at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GcResult { pub removed: Vec<String>, pub retained: Vec<String> }

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
        self.validate_update_eligibility(baseline, &parsed, &kind, current.as_ref(), accepted_at)?;
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
        let _global = Lock::take(&locks, "global.lock")?;
        let _lock = Lock::take(&locks, &format!("{}.lock", plan.baseline.replace('/', "--")))?;
        let current = self.read_manifest_optional(&plan.baseline)?;
        if current.as_ref().map(manifest_id).transpose()? != plan.prior_manifest_id
            || current.as_ref().map(head_id).transpose()? != plan.prior_head_report_id {
            return Err("baseline changed after plan; re-plan against current head".into());
        }
        validate_update_evidence(&report,&plan.kind)?;
        self.validate_update_eligibility(&plan.baseline,&report,&plan.kind,current.as_ref(),&plan.accepted_at)?;

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
        let candidate = verify_budget_report(candidate).map_err(|e| format!("invalid candidate report: {e}"))?;
        let candidate_content = report_content(&candidate)?;
        let candidate_measurement = measurement(candidate_content, &query.budget_id)?;
        require_text(candidate_measurement, "budget_spec_sha256", &query.budget_spec_sha256)?;
        require_text(candidate_measurement, "context_key", &query.context_key)?;
        let maximum=history_window(candidate_measurement)?;
        let manifest = self.read_manifest(baseline)?;
        let objects = self.dir(&[".jet", "perf", "baselines", "objects"], false, 0o755)?;
        let mut selected = Vec::new();
        for generation in generations(&manifest)?.iter().rev() {
            let generation = object(generation, "generation")?;
            let id = text(generation.get("report_id"), "generation.report_id")?;
            let bytes = read_regular(&objects, &format!("{id}.json"))?;
            let old = verify_budget_report(&bytes).map_err(|e| format!("corrupt baseline object {id}: {e}"))?;
            if report_id(&old)? != id { return Err(format!("baseline object {id} contains a different report_id")); }
            let old_content = report_content(&old)?;
            let Ok(old_measurement) = measurement(old_content, &query.budget_id) else { continue };
            if compatible(candidate_content, candidate_measurement, old_content, old_measurement) {
                let audit=object(generation.get("audit").ok_or("generation audit is absent")?,"audit")?;
                let accepted=text(audit.get("accepted_at"),"accepted_at")?;
                let age=timestamp_seconds(&query.at)?.checked_sub(timestamp_seconds(accepted)?).ok_or("baseline generation is future-dated")?;
                if age>stale_after_seconds(candidate_measurement)?{continue}
                selected.push(id.into());
                if selected.len() == maximum { break; }
            }
        }
        Ok(selected)
    }

    pub fn gc(&self, at:&str)->Result<GcResult,String>{let now=timestamp_seconds(at)?;let locks=self.dir(&[".jet","perf","baselines","locks"],true,0o700)?;let _global=Lock::take(&locks,"global.lock")?;let names=self.dir(&[".jet","perf","baselines","names"],false,0o755)?;let mut referenced=BTreeSet::new();let mut retained=Vec::new();let mut reachability_complete=true;collect_manifest_refs(&names,"",&mut referenced,&mut retained,&mut reachability_complete)?;let objects=self.dir(&[".jet","perf","baselines","objects"],false,0o755)?;let mut removed=Vec::new();for entry in list_dir(&objects)?{if entry.is_dir||entry.is_symlink{retained.push(format!("objects/{}",entry.name));continue}let Some(id)=entry.name.strip_suffix(".json")else{retained.push(format!("objects/{}",entry.name));continue};if !is_hex64(id){retained.push(format!("objects/{}",entry.name));continue}let bytes=match read_regular(&objects,&entry.name){Ok(bytes)=>bytes,Err(error)=>{retained.push(format!("objects/{}: {error}",entry.name));continue}};let report=match verify_budget_report(&bytes){Ok(report)=>report,Err(error)=>{retained.push(format!("objects/{}: {error}",entry.name));continue}};if report_id(&report)?!=id{retained.push(format!("objects/{}: report_id mismatch",entry.name));continue}if !reachability_complete||referenced.contains(id)||now.saturating_sub(entry.modified)<86400{retained.push(format!("objects/{}",entry.name));continue}unlink_checked(&objects,&entry.name)?;removed.push(id.into())}removed.sort();retained.sort();Ok(GcResult{removed,retained})}

    fn validate_update_eligibility(&self,baseline:&str,report:&CanonicalJson,kind:&UpdateKind,current:Option<&CanonicalJson>,at:&str)->Result<(),String>{
        let content=report_content(report)?;let measurements=array(content.get("measurements"),"measurements")?;let mut budget_ids=BTreeSet::new();
        for value in measurements{let candidate=object(value,"measurement")?;let budget_id=text(candidate.get("budget_id"),"budget_id")?;if !budget_ids.insert(budget_id){return Err(format!("duplicate budget_id `{budget_id}` in update"))}let comparison=object(candidate.get("comparison").ok_or("comparison is absent")?,"comparison")?;let comparison_kind=text(comparison.get("kind"),"comparison.kind")?;
            if matches!(comparison_kind,"absolute_from"|"relative_to")&&text(comparison.get("baseline"),"comparison.baseline")?!=baseline{return Err(format!("measurement baseline does not match `{baseline}`"));}
            if comparison_kind=="absolute"&&current.is_some()&&!matches!(kind,UpdateKind::Bootstrap{..}){let history=self.compatible_generation(current.unwrap(),content,candidate,at,false)?;if history.is_none(){return Err("deterministic measurement has no compatible prior generation".into());}}
            if matches!(kind,UpdateKind::Bootstrap{..}){let evidence=text(object(candidate.get("decision").ok_or("measurement decision is absent")?,"decision")?.get("evidence"),"decision.evidence")?;match comparison_kind{"absolute"|"absolute_from" if evidence!="pass"=>return Err("bootstrap requires deterministic/absolute-from evidence to pass".into()),"relative_to" if !matches!(evidence,"pass"|"unavailable")=>return Err("relative bootstrap permits only pass or unavailable evidence".into()),_=>{}}}
        }
        if matches!(kind,UpdateKind::Bootstrap{..}){if let Some(manifest)=current{for value in measurements{let candidate=object(value,"measurement")?;match self.compatible_generation(manifest,content,candidate,at,true)?{Some(true)=>{},Some(false)=>return Err("bootstrap is allowed only when newest compatible history is stale".into()),None=>return Err("bootstrap cannot replace incompatible or corrupt history".into())}}}}
        Ok(())
    }

    fn compatible_generation(&self,manifest:&CanonicalJson,candidate_content:&BTreeMap<String,CanonicalJson>,candidate:&BTreeMap<String,CanonicalJson>,at:&str,include_stale:bool)->Result<Option<bool>,String>{let objects=self.dir(&[".jet","perf","baselines","objects"],false,0o755)?;for generation in generations(manifest)?.iter().rev(){let generation=object(generation,"generation")?;let id=text(generation.get("report_id"),"generation.report_id")?;let bytes=read_regular(&objects,&format!("{id}.json"))?;let old=verify_budget_report(&bytes).map_err(|e|format!("corrupt baseline object {id}: {e}"))?;if report_id(&old)?!=id{return Err(format!("baseline object {id} contains a different report_id"))}let old_content=report_content(&old)?;let Ok(old_measurement)=measurement(old_content,text(candidate.get("budget_id"),"budget_id")?)else{continue};if compatible(candidate_content,candidate,old_content,old_measurement){let audit=object(generation.get("audit").ok_or("generation audit is absent")?,"audit")?;let age=timestamp_seconds(at)?.checked_sub(timestamp_seconds(text(audit.get("accepted_at"),"accepted_at")?)?).ok_or("baseline generation is future-dated")?;let stale=age>stale_after_seconds(candidate)?;return if stale&&!include_stale{Ok(None)}else{Ok(Some(stale))}}}Ok(None)}

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

struct ListedEntry{name:String,is_dir:bool,is_symlink:bool,modified:u64}
#[cfg(target_os="linux")]
fn list_dir(dir:&File)->Result<Vec<ListedEntry>,String>{use std::os::fd::AsRawFd;use std::time::UNIX_EPOCH;let path=format!("/proc/self/fd/{}",dir.as_raw_fd());let mut out=Vec::new();for entry in std::fs::read_dir(path).map_err(|e|e.to_string())?{let entry=entry.map_err(|e|e.to_string())?;let name=entry.file_name().into_string().map_err(|_|"store entry name is not UTF-8")?;let kind=entry.file_type().map_err(|e|e.to_string())?;let modified=entry.metadata().ok().and_then(|m|m.modified().ok()).and_then(|t|t.duration_since(UNIX_EPOCH).ok()).map(|v|v.as_secs()).unwrap_or(u64::MAX);out.push(ListedEntry{name,is_dir:kind.is_dir(),is_symlink:kind.is_symlink(),modified})}out.sort_by(|a,b|a.name.cmp(&b.name));Ok(out)}
#[cfg(not(target_os="linux"))]fn list_dir(_: &File)->Result<Vec<ListedEntry>,String>{Err("secure baseline GC unavailable on this platform".into())}

fn collect_manifest_refs(dir:&File,prefix:&str,referenced:&mut BTreeSet<String>,retained:&mut Vec<String>,complete:&mut bool)->Result<(),String>{for entry in list_dir(dir)?{let logical=if prefix.is_empty(){entry.name.clone()}else{format!("{prefix}/{}",entry.name)};if entry.is_symlink{*complete=false;retained.push(format!("names/{logical}: symlink"));continue}if entry.is_dir{let child=open_child_dir(dir,&entry.name)?;collect_manifest_refs(&child,&logical,referenced,retained,complete)?;continue}if entry.name.starts_with(".tmp-"){retained.push(format!("names/{logical}: incomplete temporary manifest"));continue}let Some(stem)=entry.name.strip_suffix(".json")else{*complete=false;retained.push(format!("names/{logical}: unknown file"));continue};let baseline=if prefix.is_empty(){stem.into()}else{format!("{prefix}/{stem}")};if validate_baseline_name(&baseline).is_err(){*complete=false;retained.push(format!("names/{logical}: invalid baseline name"));continue}let bytes=match read_regular(dir,&entry.name){Ok(bytes)=>bytes,Err(error)=>{*complete=false;retained.push(format!("names/{logical}: {error}"));continue}};let manifest=match verify_manifest(&bytes,&baseline){Ok(manifest)=>manifest,Err(error)=>{*complete=false;retained.push(format!("names/{logical}: {error}"));continue}};for generation in generations(&manifest)?{let generation=object(generation,"generation")?;referenced.insert(text(generation.get("report_id"),"generation.report_id")?.into());}}Ok(())}

#[cfg(unix)]fn open_child_dir(dir:&File,name:&str)->Result<File,String>{use std::ffi::CString;use std::os::fd::{AsRawFd,FromRawFd};const O_RDONLY:i32=0;const O_CLOEXEC:i32=0o2000000;const O_DIRECTORY:i32=0o200000;const O_NOFOLLOW:i32=0o400000;extern "C"{fn openat(fd:i32,path:*const i8,flags:i32,mode:u32)->i32;}let name=CString::new(name).map_err(|_|"NUL in directory name")?;let fd=unsafe{openat(dir.as_raw_fd(),name.as_ptr(),O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC,0)};if fd<0{return Err(format!("cannot securely open store directory: {}",std::io::Error::last_os_error()))}Ok(unsafe{File::from_raw_fd(fd)})}
#[cfg(not(unix))]fn open_child_dir(_: &File,_:&str)->Result<File,String>{Err("secure directory traversal unavailable on this platform".into())}

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
    let mut prior_accepted:Option<&str>=None;
    for (index, generation) in generations.iter().enumerate() {
        let generation = exact_object(generation, "generation", &["audit", "report_id"])?;
        let report = text(generation.get("report_id"), "generation.report_id")?;
        if !is_hex64(report) { return Err("generation report_id is not lowercase Hex64".into()); }
        let audit = exact_object(generation.get("audit").unwrap(), "audit", &["accepted_at", "actor_label", "audit_id", "flags", "kind", "prior_head_report_id", "prior_state_id", "reason", "report_id"])?;
        require_text(audit, "actor_label", "local")?;
        require_text(audit, "report_id", report)?;
        let accepted_at=text(audit.get("accepted_at"), "accepted_at")?;validate_timestamp(accepted_at)?;if prior_accepted.is_some_and(|prior|prior>accepted_at){return Err("manifest generations are not ordered by accepted_at".into())}prior_accepted=Some(accepted_at);
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
            if kind!="bootstrap" { return Err("first generation is not a bootstrap audit".into()); }
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
fn normalize_reason(value: &str) -> Result<String, String> { if value.is_empty() || value.trim() != value || value.chars().count() > 512 || value.chars().any(char::is_control) { Err("acceptance reason must be trimmed, control-free, and 1..=512 scalars".into()) } else if nfc(value)!=value { Err("acceptance reason must be NFC".into()) } else { Ok(value.into()) } }
fn validate_update_evidence(report:&CanonicalJson,kind:&UpdateKind)->Result<(),String>{
    let measurements=array(report_content(report)?.get("measurements"),"measurements")?;
    if measurements.is_empty(){return Err("update report has no measurements".into())}
    let mut regressed=false;
    for measurement in measurements { let measurement=object(measurement,"measurement")?;let decision=object(measurement.get("decision").ok_or("measurement decision is absent")?,"decision")?;let evidence=text(decision.get("evidence"),"decision.evidence")?;match kind{UpdateKind::Pass if evidence!="pass"=>return Err("plain update requires every measurement to pass".into()),UpdateKind::Bootstrap{..} if !matches!(evidence,"pass"|"unavailable")=>return Err("bootstrap rejects regression and inconclusive evidence".into()),UpdateKind::AcceptRegression{..} if !matches!(evidence,"pass"|"regression"|"inconclusive")=>return Err("accept-regression rejects unavailable evidence".into()),_=>{}}if matches!(evidence,"regression"|"inconclusive"){regressed=true;}}
    if matches!(kind,UpdateKind::AcceptRegression{..})&&!regressed{return Err("accept-regression requires regression or inconclusive evidence".into())}Ok(())
}
fn validate_timestamp(value: &str) -> Result<(), String> { timestamp_parts(value).map(|_|()) }
fn timestamp_parts(value:&str)->Result<(i64,u32,u32,u32,u32,u32),String>{let b=value.as_bytes();let punctuation=[(4,b'-'),(7,b'-'),(10,b'T'),(13,b':'),(16,b':'),(19,b'.'),(29,b'Z')];if b.len()!=30||punctuation.iter().any(|(i,v)|b[*i]!=*v)||b.iter().enumerate().any(|(i,v)|!punctuation.iter().any(|(p,_)|*p==i)&&!v.is_ascii_digit()){return Err("accepted_at is not RFC3339UTC with nine fractional digits".into())}let number=|range:std::ops::Range<usize>|std::str::from_utf8(&b[range]).unwrap().parse::<u32>().unwrap();let year=number(0..4)as i64;let month=number(5..7);let day=number(8..10);let hour=number(11..13);let minute=number(14..16);let second=number(17..19);let leap=year%4==0&&(year%100!=0||year%400==0);let days=[31,if leap{29}else{28},31,30,31,30,31,31,30,31,30,31];if !(1..=12).contains(&month)||day==0||day>days[(month-1)as usize]||hour>23||minute>59||second>59{return Err("accepted_at contains an out-of-range UTC field".into())}Ok((year,month,day,hour,minute,second))}
fn timestamp_seconds(value:&str)->Result<u64,String>{let(year,month,day,hour,minute,second)=timestamp_parts(value)?;if year<1970{return Err("accepted_at predates Unix epoch".into())}let y=year-if month<=2{1}else{0};let era=y.div_euclid(400);let yoe=y-era*400;let m=month as i64+if month>2{-3}else{9};let doy=(153*m+2)/5+day as i64-1;let doe=yoe*365+yoe/4-yoe/100+doy;let days=era*146097+doe-719468;Ok((days as u64)*86400+(hour as u64)*3600+(minute as u64)*60+second as u64)}
fn stale_after_seconds(measurement:&BTreeMap<String,CanonicalJson>)->Result<u64,String>{match measurement.get("policy"){Some(CanonicalJson::Object(policy))=>integer(policy.get("stale_after_seconds"),"policy.stale_after_seconds")?.parse().map_err(|_|"stale_after_seconds exceeds u64".into()),Some(CanonicalJson::Null)=>Ok(2_592_000),_=>Err("measurement policy is invalid".into())}}
fn history_window(measurement:&BTreeMap<String,CanonicalJson>)->Result<usize,String>{match measurement.get("policy"){Some(CanonicalJson::Object(policy))=>integer(policy.get("baseline_generations"),"policy.baseline_generations")?.parse().map_err(|_|"baseline_generations exceeds usize".into()),Some(CanonicalJson::Null)=>Ok(5),_=>Err("measurement policy is invalid".into())}}
fn nfc(value:&str)->String{let mut decomposed=Vec::new();for ch in value.chars(){canonical_decompose(ch as u32,&mut decomposed)}let mut start=0usize;for index in 0..decomposed.len(){let class=ccc(decomposed[index]);if class==0{start=index+1}else{let mut at=index;while at>start&&ccc(decomposed[at-1])>class{decomposed.swap(at-1,at);at-=1}}}let mut output:Vec<u32>=Vec::new();let mut starter=0usize;let mut last_class=0u8;for code in decomposed{let class=ccc(code);let composed=output.get(starter).and_then(|left|compose(*left,code));if let Some(combined)=composed.filter(|_|last_class<class||last_class==0){output[starter]=combined}else{if class==0{starter=output.len()}output.push(code);last_class=class}}output.into_iter().filter_map(char::from_u32).collect()}
fn canonical_decompose(code:u32,out:&mut Vec<u32>){const SBASE:u32=0xAC00;const LBASE:u32=0x1100;const VBASE:u32=0x1161;const TBASE:u32=0x11A7;const LCOUNT:u32=19;const VCOUNT:u32=21;const TCOUNT:u32=28;const NCOUNT:u32=VCOUNT*TCOUNT;const SCOUNT:u32=LCOUNT*NCOUNT;if(SBASE..SBASE+SCOUNT).contains(&code){let index=code-SBASE;out.push(LBASE+index/NCOUNT);out.push(VBASE+(index%NCOUNT)/TCOUNT);if index%TCOUNT!=0{out.push(TBASE+index%TCOUNT)}return}use jet_foundation::generated::UnicodeTables::{UNICODE_DECOMP_INDEX,UNICODE_DECOMP_POOL};if let Ok(index)=UNICODE_DECOMP_INDEX.binary_search_by_key(&code,|entry|entry.0){let(_,offset,length,canonical)=UNICODE_DECOMP_INDEX[index];if canonical==1{for child in &UNICODE_DECOMP_POOL[offset as usize..(offset+length as u32)as usize]{canonical_decompose(*child,out)}return}}out.push(code)}
fn ccc(code:u32)->u8{use jet_foundation::generated::UnicodeTables::UNICODE_CCC;let mut low=0usize;let mut high=UNICODE_CCC.len();while low<high{let mid=(low+high)/2;let(start,end,class)=UNICODE_CCC[mid];if code<start{high=mid}else if code>end{low=mid+1}else{return class}}0}
fn compose(left:u32,right:u32)->Option<u32>{const SBASE:u32=0xAC00;const LBASE:u32=0x1100;const VBASE:u32=0x1161;const TBASE:u32=0x11A7;const LCOUNT:u32=19;const VCOUNT:u32=21;const TCOUNT:u32=28;const NCOUNT:u32=VCOUNT*TCOUNT;const SCOUNT:u32=LCOUNT*NCOUNT;if(LBASE..LBASE+LCOUNT).contains(&left)&&(VBASE..VBASE+VCOUNT).contains(&right){return Some(SBASE+(left-LBASE)*NCOUNT+(right-VBASE)*TCOUNT)}if(SBASE..SBASE+SCOUNT).contains(&left)&&(left-SBASE)%TCOUNT==0&&(TBASE+1..TBASE+TCOUNT).contains(&right){return Some(left+right-TBASE)}use jet_foundation::generated::UnicodeTables::UNICODE_COMPOSE_PAIRS;UNICODE_COMPOSE_PAIRS.binary_search_by_key(&(left,right),|entry|(entry.0,entry.1)).ok().map(|index|UNICODE_COMPOSE_PAIRS[index].2)}
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
#[cfg(unix)]fn unlink_checked(dir:&File,name:&str)->Result<(),String>{use std::ffi::CString;use std::os::fd::AsRawFd;extern "C"{fn unlinkat(fd:i32,path:*const i8,flags:i32)->i32;}let name=CString::new(name).map_err(|_|"NUL in artifact name")?;if unsafe{unlinkat(dir.as_raw_fd(),name.as_ptr(),0)}==0{dir.sync_all().map_err(|e|e.to_string())}else{Err(format!("cannot remove unreferenced object: {}",std::io::Error::last_os_error()))}}
#[cfg(not(unix))]fn unlink_checked(_: &File,_:&str)->Result<(),String>{Err("secure unlink unavailable on this platform".into())}

fn install_immutable(dir:&File,name:&str,bytes:&[u8],verify:impl Fn(&[u8])->Result<(),String>)->Result<bool,String>{match read_regular(dir,name){Ok(existing)=>{verify(&existing)?;if existing==bytes{return Ok(false)}return Err("immutable artifact differs from candidate".into())},Err(e)if !e.contains("No such file")=>return Err(e),Err(_)=>{}}let (file,tmp)=temp(dir,bytes)?;artifact_permissions(&file)?;#[cfg(unix)]{use std::ffi::CString;use std::os::fd::AsRawFd;extern "C"{fn linkat(oldfd:i32,old:*const i8,newfd:i32,new:*const i8,flags:i32)->i32;}let old=CString::new(tmp.as_str()).unwrap();let new=CString::new(name).unwrap();if unsafe{linkat(dir.as_raw_fd(),old.as_ptr(),dir.as_raw_fd(),new.as_ptr(),0)}!=0{let error=std::io::Error::last_os_error();unlink(dir,&tmp);if error.kind()==ErrorKind::AlreadyExists{let existing=read_regular(dir,name)?;verify(&existing)?;if existing==bytes{return Ok(false)}}return Err(format!("cannot atomically install artifact: {error}"));}unlink(dir,&tmp);dir.sync_all().map_err(|e|e.to_string())?;Ok(true)}#[cfg(not(unix))]{let _=tmp;Err("atomic no-replace unavailable on this platform".into())}}

fn replace_atomic(dir:&File,name:&str,bytes:&[u8])->Result<(),String>{let (file,tmp)=temp(dir,bytes)?;artifact_permissions(&file)?;#[cfg(unix)]{use std::ffi::CString;use std::os::fd::AsRawFd;extern "C"{fn renameat(oldfd:i32,old:*const i8,newfd:i32,new:*const i8)->i32;}let old=CString::new(tmp.as_str()).unwrap();let new=CString::new(name).unwrap();if unsafe{renameat(dir.as_raw_fd(),old.as_ptr(),dir.as_raw_fd(),new.as_ptr())}!=0{let error=std::io::Error::last_os_error();unlink(dir,&tmp);return Err(format!("cannot atomically replace manifest: {error}"));}dir.sync_all().map_err(|e|e.to_string())?;Ok(())}#[cfg(not(unix))]{let _=(dir,name,tmp);Err("atomic replacement unavailable on this platform".into())}}

#[cfg(target_os="linux")]fn artifact_permissions(file:&File)->Result<(),String>{use std::os::unix::fs::PermissionsExt;let status=std::fs::read_to_string("/proc/self/status").map_err(|e|format!("cannot read process umask: {e}"))?;let raw=status.lines().find_map(|line|line.strip_prefix("Umask:\t")).ok_or("process umask is unavailable")?;let mask=u32::from_str_radix(raw.trim(),8).map_err(|_|"process umask is invalid")?;file.set_permissions(std::fs::Permissions::from_mode(0o644&!mask)).map_err(|e|e.to_string())?;file.sync_all().map_err(|e|e.to_string())}
#[cfg(not(target_os="linux"))]fn artifact_permissions(_: &File)->Result<(),String>{Err("performance baseline mutation is enabled only on Linux until equivalent umask and durability guarantees are ratified".into())}

#[cfg(unix)]struct Lock{file:File}
#[cfg(unix)]impl Lock{fn take(dir:&File,name:&str)->Result<Self,String>{use std::ffi::CString;use std::os::fd::{AsRawFd,FromRawFd};use std::os::unix::fs::MetadataExt;const O_RDWR:i32=2;const O_CREAT:i32=0o100;const O_CLOEXEC:i32=0o2000000;const O_NOFOLLOW:i32=0o400000;const LOCK_EX:i32=2;extern "C"{fn openat(fd:i32,path:*const i8,flags:i32,mode:u32)->i32;fn flock(fd:i32,operation:i32)->i32;}let name=CString::new(name).map_err(|_|"NUL in lock")?;let fd=unsafe{openat(dir.as_raw_fd(),name.as_ptr(),O_RDWR|O_CREAT|O_NOFOLLOW|O_CLOEXEC,0o600)};if fd<0{return Err(format!("cannot securely open lock: {}",std::io::Error::last_os_error()));}let file=unsafe{File::from_raw_fd(fd)};let meta=file.metadata().map_err(|e|e.to_string())?;if !meta.is_file()||meta.nlink()!=1{return Err("baseline lock is linked or not regular".into())}if unsafe{flock(file.as_raw_fd(),LOCK_EX)}!=0{return Err(format!("cannot lock baseline: {}",std::io::Error::last_os_error()));}Ok(Self{file})}}
#[cfg(unix)]impl Drop for Lock{fn drop(&mut self){use std::os::fd::AsRawFd;const LOCK_UN:i32=8;extern "C"{fn flock(fd:i32,operation:i32)->i32;}unsafe{flock(self.file.as_raw_fd(),LOCK_UN)};}}
#[cfg(not(unix))]struct Lock;
#[cfg(not(unix))]impl Lock{fn take(_: &File,_:&str)->Result<Self,String>{Err("advisory lock unavailable on this platform".into())}}

#[cfg(test)]
mod tests {
    use super::*;
    use jet_foundation::PerformanceBudget::{Comparison,Direction,Enforcement,LimitDirection,Rational,evaluate};
    use jet_foundation::SHA256::sha256_hex;

    fn valid_report(marker:&str)->(Vec<u8>,String,String,String){
        let subject=CanonicalJson::object([("artifact".into(),CanonicalJson::Null),("measured_end".into(),CanonicalJson::String("2026-01-01T00:00:01.000000000Z".into())),("measured_start".into(),CanonicalJson::String("2026-01-01T00:00:00.000000000Z".into())),("member_sources".into(),CanonicalJson::Array(Vec::new())),("profile".into(),CanonicalJson::String("dev".into())),("target_class".into(),CanonicalJson::String("native".into())),("target_id".into(),CanonicalJson::String(format!("target-{marker}"))),("target_triple".into(),CanonicalJson::String("x86_64-unknown-linux-gnu".into()))]).unwrap();
        let tool_body=CanonicalJson::object([("compiler_build_id".into(),CanonicalJson::String("compiler".into())),("jet_version".into(),CanonicalJson::String("1".into())),("runner_id".into(),CanonicalJson::String("runner".into())),("stdlib_id".into(),CanonicalJson::String("stdlib".into()))]).unwrap();let mut tool=object(&tool_body,"tool").unwrap().clone();tool.insert("digest".into(),CanonicalJson::String(stable_id(&tool_body)));let tool=CanonicalJson::Object(tool);
        let provider_body=CanonicalJson::object([("cpu_arch".into(),CanonicalJson::String("x86_64".into())),("cpu_model".into(),CanonicalJson::String("test".into())),("identity".into(),CanonicalJson::String(String::new())),("isolation".into(),CanonicalJson::String("process".into())),("kernel".into(),CanonicalJson::String("test".into())),("kind".into(),CanonicalJson::String("CompilerFacts".into())),("logical_cpus".into(),CanonicalJson::Integer("1".into())),("memory_bytes".into(),CanonicalJson::Integer("1".into())),("os".into(),CanonicalJson::String("linux".into())),("power_governor".into(),CanonicalJson::String("fixed".into())),("version".into(),CanonicalJson::String("1".into()))]).unwrap();let mut provider=object(&provider_body,"provider").unwrap().clone();provider.insert("hardware_fingerprint".into(),CanonicalJson::String(stable_id(&provider_body)));let provider=CanonicalJson::Object(provider);
        let metric=CanonicalJson::object([("name".into(),CanonicalJson::String("BinarySize".into())),("percentile".into(),CanonicalJson::Null)]).unwrap();let comparison=CanonicalJson::object([("direction".into(),CanonicalJson::String("at_most".into())),("kind".into(),CanonicalJson::String("absolute".into())),("limit".into(),Rational::integer(10).to_json())]).unwrap();let budget_spec=CanonicalJson::object([("applies".into(),CanonicalJson::object([("profiles".into(),CanonicalJson::Array(vec![CanonicalJson::String("dev".into())])),("targets".into(),CanonicalJson::Array(vec![CanonicalJson::String(format!("target-{marker}"))]))]).unwrap()),("comparison".into(),comparison.clone()),("enforcement".into(),CanonicalJson::String("fail".into())),("metric".into(),metric.clone()),("name".into(),CanonicalJson::String(format!("size-{marker}"))),("package_id".into(),CanonicalJson::String("pkg".into())),("perf_role".into(),CanonicalJson::String("package".into())),("provider".into(),CanonicalJson::object([("identity".into(),CanonicalJson::String(String::new())),("kind".into(),CanonicalJson::String("CompilerFacts".into()))]).unwrap()),("scope".into(),CanonicalJson::String("Package".into()))]).unwrap();let spec=stable_id(&budget_spec);
        let mut context_input=b"jet-budget-context-v1\0".to_vec();let frame=|out:&mut Vec<u8>,value:&str|{out.extend_from_slice(&(value.len()as u64).to_be_bytes());out.extend_from_slice(value.as_bytes());};for value in [format!("target-{marker}"),"BinarySize".into(),String::new(),"native".into(),"x86_64-unknown-linux-gnu".into(),"dev".into(),"1".into(),"compiler".into(),"stdlib".into(),"runner".into(),text(object(&tool,"tool").unwrap().get("digest"),"digest").unwrap().into(),"CompilerFacts".into(),String::new(),"1".into(),"process".into(),"x86_64".into(),"test".into(),"1".into(),"1".into(),"linux".into(),"test".into(),"fixed".into(),text(object(&provider,"provider").unwrap().get("hardware_fingerprint"),"fingerprint").unwrap().into()]{frame(&mut context_input,&value)}let context=sha256_hex(&context_input);
        let base=CanonicalJson::object([("baseline".into(),CanonicalJson::Null),("budget_id".into(),CanonicalJson::String(format!("pkg:size-{marker}"))),("budget_spec".into(),budget_spec),("budget_spec_sha256".into(),CanonicalJson::String(spec.clone())),("comparison".into(),comparison.clone()),("context_key".into(),CanonicalJson::String(context.clone())),("decision".into(),CanonicalJson::Null),("direction".into(),CanonicalJson::String("lower_is_better".into())),("enforcement".into(),CanonicalJson::String("fail".into())),("history".into(),CanonicalJson::Null),("metric".into(),metric),("policy".into(),CanonicalJson::Null),("provider".into(),provider),("samples".into(),CanonicalJson::Array(vec![Rational::integer(1).to_json()])),("source".into(),CanonicalJson::String("main.jet:1".into())),("statistics".into(),CanonicalJson::Null),("target_class".into(),CanonicalJson::String("native".into())),("unit".into(),CanonicalJson::String("Bytes".into()))]).unwrap();let evidence=stable_id(&CanonicalJson::object([("measurements".into(),CanonicalJson::Array(vec![base.clone()])),("subject".into(),subject.clone()),("toolchain".into(),tool.clone())]).unwrap());let evaluation=evaluate(&evidence,&context,&[],&[Rational::integer(1)],&[],None,&Comparison::Absolute{limit:Rational::integer(10),direction:LimitDirection::AtMost},Direction::LowerIsBetter,Enforcement::Fail,None).unwrap();let trend=CanonicalJson::object([("estimators".into(),CanonicalJson::Array(Vec::new())),("label".into(),CanonicalJson::String("insufficient".into())),("report_ids".into(),CanonicalJson::Array(Vec::new())),("score".into(),CanonicalJson::Null)]).unwrap();let decision=CanonicalJson::object([("evidence".into(),CanonicalJson::String("pass".into())),("lower95".into(),CanonicalJson::Null),("point".into(),evaluation.point.to_json()),("policy_outcome".into(),CanonicalJson::String("pass".into())),("reason".into(),CanonicalJson::Null),("trend".into(),trend),("upper95".into(),CanonicalJson::Null)]).unwrap();let mut measurement=object(&base,"measurement").unwrap().clone();measurement.insert("decision".into(),decision);let privacy=CanonicalJson::object([("excluded".into(),CanonicalJson::Array(Vec::new())),("retained".into(),CanonicalJson::Array(Vec::new())),("schema".into(),CanonicalJson::Integer("1".into())),("workspace_paths_only".into(),CanonicalJson::Bool(true))]).unwrap();let content=CanonicalJson::object([("evidence_id".into(),CanonicalJson::String(evidence)),("measurements".into(),CanonicalJson::Array(vec![CanonicalJson::Object(measurement)])),("privacy".into(),privacy),("subject".into(),subject),("summary".into(),CanonicalJson::object([("fail".into(),CanonicalJson::Integer("0".into())),("outcome".into(),CanonicalJson::String("pass".into())),("pass".into(),CanonicalJson::Integer("1".into())),("warn".into(),CanonicalJson::Integer("0".into()))]).unwrap()),("toolchain".into(),tool)]).unwrap();let id=stable_id(&content);let report=CanonicalJson::object([("content".into(),content),("report_id".into(),CanonicalJson::String(id.clone())),("schema".into(),CanonicalJson::String("jet.budget-report".into())),("version".into(),CanonicalJson::Integer("1".into()))]).unwrap();let bytes=report.bytes();verify_budget_report(&bytes).unwrap();(bytes,id,spec,context)
    }
    #[test]
    fn baseline_names_and_reasons_are_closed() {
        assert!(validate_baseline_name("release/x86-linux").is_ok());
        for bad in ["", "/x", "x/", "x//y", "X", "x_", "x/../y", "x--y"] { assert!(validate_baseline_name(bad).is_err(), "{bad}"); }
        assert!(normalize_reason(" reviewed regression ").is_err());
        assert!(normalize_reason("reviewed regression").is_ok());
        assert!(normalize_reason("e\u{301}").is_err());assert!(normalize_reason("é").is_ok());
        assert!(validate_timestamp("2026-02-31T12:00:00.000000000Z").is_err());assert!(validate_timestamp("2024-02-29T12:00:00.000000000Z").is_ok());
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
        let invalid_first=UpdatePlan{kind:UpdateKind::Pass,..first.clone()};assert!(verify_manifest(&next_manifest(None,&invalid_first).unwrap().bytes(),"release/linux").unwrap_err().contains("bootstrap"));
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
        let status=std::fs::read_to_string("/proc/self/status").unwrap();let mask=u32::from_str_radix(status.lines().find_map(|line|line.strip_prefix("Umask:\t")).unwrap().trim(),8).unwrap();assert_eq!(std::fs::metadata(root.join("objects/a.json")).unwrap().permissions().mode()&0o777,0o644&!mask);
        assert!(!install_immutable(&dir,"a.json",b"one\n",|_|Ok(())).unwrap());
        assert!(install_immutable(&dir,"a.json",b"two\n",|_|Ok(())).is_err());
        symlink("/etc/passwd",root.join("objects/evil.json")).unwrap();
        assert!(read_regular(&dir,"evil.json").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn store_rejects_fresh_bootstrap_corrupt_links_and_collects_only_old_unreferenced(){
        let root=std::env::temp_dir().join(format!("jet-budget-store-full-{}-{}",std::process::id(),NONCE.fetch_add(1,Ordering::Relaxed)));std::fs::create_dir(&root).unwrap();let store=BudgetStore::new(&root);let(bytes,id,spec,context)=valid_report("one");let plan=store.plan_update("release/linux",&bytes,UpdateKind::Bootstrap{reason:"initial baseline".into()},"2026-01-01T00:00:00.000000000Z").unwrap();assert_eq!(store.apply_update(&plan).unwrap().report_id,id);assert!(store.plan_update("release/linux",&bytes,UpdateKind::Bootstrap{reason:"replace fresh".into()},"2026-01-02T00:00:00.000000000Z").unwrap_err().contains("stale"));let fresh=HistoryQuery{budget_id:"pkg:size-one".into(),budget_spec_sha256:spec.clone(),context_key:context.clone(),at:"2026-01-02T00:00:00.000000000Z".into()};assert_eq!(store.select_compatible_history("release/linux",&bytes,&fresh).unwrap(),vec![id.clone()]);let stale=HistoryQuery{at:"2026-03-02T00:00:00.000000000Z".into(),..fresh.clone()};assert!(store.select_compatible_history("release/linux",&bytes,&stale).unwrap().is_empty());
        let(other,other_id,_,_)=valid_report("other");let objects=store.dir(&[".jet","perf","baselines","objects"],false,0o755).unwrap();assert!(install_immutable(&objects,&format!("{other_id}.json"),&other,|bytes|verify_budget_report(bytes).map(|_|()).map_err(|e|e.to_string())).unwrap());use std::ffi::CString;#[repr(C)]struct Timespec{tv_sec:i64,tv_nsec:i64}extern "C"{fn utimensat(fd:i32,path:*const i8,times:*const Timespec,flags:i32)->i32;}let path=CString::new(root.join(format!(".jet/perf/baselines/objects/{other_id}.json")).to_str().unwrap()).unwrap();let times=[Timespec{tv_sec:0,tv_nsec:0},Timespec{tv_sec:0,tv_nsec:0}];assert_eq!(unsafe{utimensat(-100,path.as_ptr(),times.as_ptr(),0)},0);std::fs::write(root.join(".jet/perf/baselines/objects/unknown"),b"retain").unwrap();let result=store.gc("2026-03-02T00:00:00.000000000Z").unwrap();assert_eq!(result.removed,vec![other_id.clone()]);assert!(result.retained.iter().any(|item|item.contains("unknown")));
        std::fs::remove_file(root.join(format!(".jet/perf/baselines/objects/{id}.json"))).unwrap();std::fs::write(root.join(format!(".jet/perf/baselines/objects/{id}.json")),other).unwrap();assert!(store.select_compatible_history("release/linux",&bytes,&fresh).unwrap_err().contains("different report_id"));std::fs::remove_dir_all(root).unwrap();
    }
}
