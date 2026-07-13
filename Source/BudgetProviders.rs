//! D-PERFBUDGET-PROVIDER1: compiler-owned measurement-provider registry.
//!
//! Registry keys, executable paths, and response files are resolved by the
//! compiler. No provider lookup consults `PATH`. Every transport feeds the
//! same binary decoder and limit checker before evidence reaches evaluation.

use jet_foundation::PerformanceBudget::{CanonicalJson, Rational};
use jet_foundation::PerformanceBudget::{Comparison, Direction, Enforcement, Evaluation, MeasurementPolicy, Percentile};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub const MAX_SAMPLES: usize = 1_000_000;
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_SPECS: usize = 4_096;
pub const MAX_DETAIL_SCALARS: usize = 512;
const MAGIC: &[u8] = b"JETBUDGET1\n";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderSpec {
    pub budget_hash: String,
    pub metric: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderRequest {
    pub schema: String,
    pub version: u32,
    pub request_id: String,
    pub provider_hash: String,
    pub context_hash: String,
    pub specs: Vec<ProviderSpec>,
    pub workload: CanonicalJson,
    pub policy: CanonicalJson,
}

impl ProviderRequest {
    pub fn validate(&self) -> Result<(), ProviderFailure> {
        if self.schema != "jet.provider-request" || self.version != 1 {
            return Err(ProviderFailure::malformed("unsupported provider request schema/version"));
        }
        for (name, value) in [("request_id", &self.request_id), ("provider_hash", &self.provider_hash), ("context_hash", &self.context_hash)] {
            if !is_hex64(value) { return Err(ProviderFailure::malformed(format!("{name} is not lowercase Hex64"))); }
        }
        if self.specs.is_empty() || self.specs.len() > MAX_SPECS { return Err(ProviderFailure::malformed("provider request spec count is outside 1..=4096")); }
        let mut previous: Option<(&str, &str)> = None;
        for spec in &self.specs {
            if !is_hex64(&spec.budget_hash) || spec.metric.is_empty() { return Err(ProviderFailure::malformed("provider request has an invalid budget hash or empty metric")); }
            let key = (spec.metric.as_str(), spec.budget_hash.as_str());
            if previous.is_some_and(|prior| prior >= key) { return Err(ProviderFailure::malformed("provider request specs are not strictly ordered by metric then budget hash")); }
            previous = Some(key);
        }
        Ok(())
    }

    pub fn bytes(&self) -> Result<Vec<u8>, ProviderFailure> {
        self.validate()?;
        let specs = self.specs.iter().map(|spec| CanonicalJson::object([
            ("budget_hash".into(), CanonicalJson::String(spec.budget_hash.clone())),
            ("metric".into(), CanonicalJson::String(spec.metric.clone())),
        ]).expect("fixed keys")).collect();
        Ok(CanonicalJson::object([
            ("context_hash".into(), CanonicalJson::String(self.context_hash.clone())),
            ("policy".into(), self.policy.clone()),
            ("provider_hash".into(), CanonicalJson::String(self.provider_hash.clone())),
            ("request_id".into(), CanonicalJson::String(self.request_id.clone())),
            ("schema".into(), CanonicalJson::String(self.schema.clone())),
            ("specs".into(), CanonicalJson::Array(specs)),
            ("version".into(), CanonicalJson::Integer(self.version.to_string())),
            ("workload".into(), self.workload.clone()),
        ]).expect("fixed keys").bytes())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderEvent {
    Sample { spec: u32, metric: String, value: Rational },
    Unavailable { spec: u32, reason: String, details: Vec<(String, String)> },
    Complete { request_id: String, samples: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderEvidence { pub events: Vec<ProviderEvent> }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureClass { Unavailable, Malformed, Panic, Timeout, Execution, Incompatible, Unsupported, Unresolved }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderFailure { pub class: FailureClass, pub reason: String }
impl ProviderFailure {
    fn malformed(reason: impl Into<String>) -> Self { Self { class: FailureClass::Malformed, reason: reason.into() } }
    fn operation(class: FailureClass, reason: impl Into<String>) -> Self { Self { class, reason: reason.into() } }
    pub fn diagnostic(&self, budget: &str) -> ProviderDiagnostic {
        match self.class {
            FailureClass::Unavailable | FailureClass::Incompatible => ProviderDiagnostic { code: "E2906", what: format!("performance budget {budget} has no usable evidence"), why: self.reason.clone(), fix: "correct the provider evidence or bootstrap only when absent or stale evidence is eligible".into() },
            FailureClass::Unsupported => ProviderDiagnostic { code: "E2903", what: format!("performance budget {budget} is not valid"), why: self.reason.clone(), fix: "use one supported metric and provider pair".into() },
            FailureClass::Unresolved => ProviderDiagnostic { code: "E2905", what: format!("performance budget {budget} cannot resolve provider"), why: self.reason.clone(), fix: "name one registered provider identity".into() },
            _ => ProviderDiagnostic { code: "E2908", what: "performance budget operation failed".into(), why: format!("measurement provider refused the operation: {}", self.reason), fix: "correct the named provider failure and retry the operation".into() },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderDiagnostic { pub code: &'static str, pub what: String, pub why: String, pub fix: String }
impl ProviderDiagnostic { pub fn render(&self)->String{format!("# Error [{}]: {}\n\nWhat: {}\nWhy: {}\nFix: {}\n",self.code,self.what,self.what,self.why,self.fix)} }

type InProcessProvider = fn(&ProviderRequest) -> Result<Vec<ProviderEvent>, ProviderFailure>;
#[derive(Clone)]
enum Provider { InProcess(InProcessProvider), Subprocess(PathBuf), File(PathBuf) }

#[derive(Default)]
pub struct ProviderRegistry { providers: BTreeMap<String, Provider> }
impl ProviderRegistry {
    pub fn register_in_process(&mut self, identity: impl Into<String>, provider: InProcessProvider) -> Result<(), String> { self.insert(identity.into(), Provider::InProcess(provider)) }
    pub fn register_subprocess(&mut self, identity: impl Into<String>, executable: PathBuf) -> Result<(), String> {
        if !executable.is_absolute() { return Err("provider executable must be an absolute compiler-resolved path".into()); }
        self.insert(identity.into(), Provider::Subprocess(executable))
    }
    pub fn register_file(&mut self, identity: impl Into<String>, response: PathBuf) -> Result<(), String> {
        if !response.is_absolute() { return Err("provider response must be an absolute compiler-resolved path".into()); }
        self.insert(identity.into(), Provider::File(response))
    }
    fn insert(&mut self, identity: String, provider: Provider) -> Result<(), String> {
        if identity.is_empty() { return Err("provider identity is empty".into()); }
        if self.providers.insert(identity.clone(), provider).is_some() { return Err(format!("duplicate provider identity `{identity}`")); }
        Ok(())
    }
    pub fn collect(&self, identity: &str, request: &ProviderRequest, timeout: Duration) -> Result<ProviderEvidence, ProviderFailure> {
        request.validate()?;
        let provider = self.providers.get(identity).ok_or_else(|| ProviderFailure::operation(FailureClass::Unresolved, format!("provider `{identity}` is unresolved")))?;
        let events = match provider {
            Provider::InProcess(function) => run_in_process(*function, request, timeout, identity)?,
            Provider::Subprocess(path) => decode_stream(&run_subprocess(path, &request.bytes()?, timeout)?, request)?,
            Provider::File(path) => decode_stream(&read_bounded(path, timeout)?, request)?,
        };
        validate_events(events, request)
    }
}

fn run_in_process(function: InProcessProvider, request: &ProviderRequest, timeout: Duration, identity: &str) -> Result<Vec<ProviderEvent>, ProviderFailure> {
    let request = request.clone();
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(|| function(&request))
            .map_err(|_| ProviderFailure::operation(FailureClass::Panic, "in-process provider panicked"))
            .and_then(|value| value);
        let _ = tx.send(result);
    });
    rx.recv_timeout(timeout).map_err(|error| match error {
        mpsc::RecvTimeoutError::Timeout => ProviderFailure::operation(FailureClass::Timeout, format!("provider `{identity}` timed out")),
        mpsc::RecvTimeoutError::Disconnected => ProviderFailure::operation(FailureClass::Execution, format!("provider `{identity}` worker disconnected")),
    })?
}

fn read_bounded(path: &Path, timeout: Duration) -> Result<Vec<u8>, ProviderFailure> {
    let path = path.to_path_buf();
    let display = path.display().to_string();
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || { let _ = tx.send(read_regular_bounded(&path)); });
    rx.recv_timeout(timeout).map_err(|error| match error {
        mpsc::RecvTimeoutError::Timeout => ProviderFailure::operation(FailureClass::Timeout, format!("provider response {display} timed out")),
        mpsc::RecvTimeoutError::Disconnected => ProviderFailure::operation(FailureClass::Execution, format!("provider response {display} reader disconnected")),
    })?
}

fn read_regular_bounded(path: &Path) -> Result<Vec<u8>, ProviderFailure> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| ProviderFailure::operation(FailureClass::Execution, format!("cannot inspect provider response {}: {error}", path.display())))?;
    if !metadata.file_type().is_file() { return Err(ProviderFailure::operation(FailureClass::Execution, "provider response is not a regular file")); }
    #[cfg(unix)]
    let file = { use std::os::unix::fs::OpenOptionsExt; std::fs::OpenOptions::new().read(true).custom_flags(0o400000).open(path) };
    #[cfg(not(unix))]
    let file = std::fs::File::open(path);
    let file = file.map_err(|error| ProviderFailure::operation(FailureClass::Execution, format!("cannot open provider response {}: {error}", path.display())))?;
    if !file.metadata().map_err(|error| ProviderFailure::operation(FailureClass::Execution, format!("cannot inspect open provider response: {error}")))?.is_file() {
        return Err(ProviderFailure::operation(FailureClass::Execution, "open provider response is not a regular file"));
    }
    let mut bytes = Vec::new(); file.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes).map_err(|error| ProviderFailure::operation(FailureClass::Execution, format!("cannot read provider response: {error}")))?;
    if bytes.len() > MAX_BYTES { return Err(ProviderFailure::malformed("provider stream exceeds 16 MiB")); } Ok(bytes)
}

fn run_subprocess(path: &Path, request: &[u8], timeout: Duration) -> Result<Vec<u8>, ProviderFailure> {
    let mut command = Command::new(path);
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(unix)] { use std::os::unix::process::CommandExt; command.process_group(0); }
    let mut child = command.spawn().map_err(|error| ProviderFailure::operation(FailureClass::Execution, format!("cannot launch provider {}: {error}", path.display())))?;
    let mut stdin = child.stdin.take().ok_or_else(|| ProviderFailure::operation(FailureClass::Execution, "provider stdin was unavailable"))?; let request=request.to_vec();
    let (writer_tx,writer_rx)=mpsc::sync_channel(1);std::thread::spawn(move || {let _=writer_tx.send(stdin.write_all(&request));});let stdout=child.stdout.take().ok_or_else(|| ProviderFailure::operation(FailureClass::Execution, "provider stdout was unavailable"))?;
    let (tx,rx)=mpsc::channel();std::thread::spawn(move || { let mut bytes=Vec::new();let result=stdout.take((MAX_BYTES+1) as u64).read_to_end(&mut bytes).map(|_|bytes);let _=tx.send(result); });
    let deadline=Instant::now()+timeout;loop { match child.try_wait() { Ok(Some(status)) => { if !status.success(){terminate_group(&mut child);return Err(ProviderFailure::operation(FailureClass::Execution,format!("provider exited with {status}")));}let remaining=deadline.saturating_duration_since(Instant::now());let bytes=rx.recv_timeout(remaining).map_err(|_|{terminate_group(&mut child);ProviderFailure::operation(FailureClass::Timeout,"provider stdout did not close before deadline")})?.map_err(|e|ProviderFailure::operation(FailureClass::Execution,format!("cannot read provider stdout: {e}")))?;writer_rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).map_err(|_|{terminate_group(&mut child);ProviderFailure::operation(FailureClass::Timeout,"provider stdin did not close before deadline")})?.map_err(|e|ProviderFailure::operation(FailureClass::Execution,format!("cannot write provider stdin: {e}")))?;if bytes.len()>MAX_BYTES{return Err(ProviderFailure::malformed("provider stream exceeds 16 MiB"));}return Ok(bytes); },Ok(None) if Instant::now()<deadline=>std::thread::sleep(Duration::from_millis(2)),Ok(None)=>{terminate_group(&mut child);return Err(ProviderFailure::operation(FailureClass::Timeout,"provider timed out and was terminated"));},Err(error)=>{terminate_group(&mut child);return Err(ProviderFailure::operation(FailureClass::Execution,format!("cannot supervise provider: {error}")))} } }
}

fn terminate_group(child: &mut std::process::Child) {
    #[cfg(unix)] { unsafe { extern "C" { fn kill(pid: i32, signal: i32) -> i32; } let _ = kill(-(child.id() as i32), 9); } }
    let _ = child.kill();
    let _ = child.wait();
}

pub fn encode_stream(events: &[ProviderEvent]) -> Vec<u8> { let mut out=MAGIC.to_vec();for event in events{match event{ProviderEvent::Sample{spec,metric,value}=>{out.push(1);put_u32(&mut out,*spec);put_text(&mut out,metric);put_text(&mut out,&value.num.to_string());put_text(&mut out,&value.den.to_string());},ProviderEvent::Unavailable{spec,reason,details}=>{out.push(2);put_u32(&mut out,*spec);put_text(&mut out,reason);put_u32(&mut out,details.len() as u32);for(k,v)in details{put_text(&mut out,k);put_text(&mut out,v);}},ProviderEvent::Complete{request_id,samples}=>{out.push(3);put_text(&mut out,request_id);out.extend_from_slice(&samples.to_be_bytes());}}}out }
fn put_u32(out:&mut Vec<u8>,value:u32){out.extend_from_slice(&value.to_be_bytes())}fn put_text(out:&mut Vec<u8>,value:&str){put_u32(out,value.len() as u32);out.extend_from_slice(value.as_bytes())}

fn decode_stream(bytes:&[u8],request:&ProviderRequest)->Result<Vec<ProviderEvent>,ProviderFailure>{if bytes.len()>MAX_BYTES{return Err(ProviderFailure::malformed("provider stream exceeds 16 MiB"));}let mut r=Reader{bytes,at:0};if r.take(MAGIC.len())?!=MAGIC{return Err(ProviderFailure::malformed("provider stream has bad magic"));}let mut events=Vec::new();while r.at<bytes.len(){let tag=r.byte()?;let event=match tag{1=>ProviderEvent::Sample{spec:r.u32()?,metric:r.text()?,value:Rational::parse(&r.text()?,&r.text()?).map_err(ProviderFailure::malformed)?},2=>{let spec=r.u32()?;let reason=r.text()?;let count=r.u32()? as usize;if count>MAX_DETAIL_SCALARS{return Err(ProviderFailure::malformed("provider unavailable detail exceeds 512 scalars"));}let mut details=Vec::with_capacity(count);for _ in 0..count{details.push((r.text()?,r.text()?));}if detail_scalars(&reason,&details)>MAX_DETAIL_SCALARS{return Err(ProviderFailure::malformed("provider unavailable detail exceeds 512 scalars"));}ProviderEvent::Unavailable{spec,reason,details}},3=>ProviderEvent::Complete{request_id:r.text()?,samples:r.u64()?},_=>return Err(ProviderFailure::malformed("provider stream has unknown event tag"))};events.push(event);}validate_events(events,request).map(|v|v.events)}
struct Reader<'a>{bytes:&'a[u8],at:usize}impl<'a>Reader<'a>{fn take(&mut self,n:usize)->Result<&'a[u8],ProviderFailure>{let end=self.at.checked_add(n).ok_or_else(||ProviderFailure::malformed("provider frame length overflow"))?;let value=self.bytes.get(self.at..end).ok_or_else(||ProviderFailure::malformed("provider stream is truncated"))?;self.at=end;Ok(value)}fn byte(&mut self)->Result<u8,ProviderFailure>{Ok(self.take(1)?[0])}fn u32(&mut self)->Result<u32,ProviderFailure>{Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))}fn u64(&mut self)->Result<u64,ProviderFailure>{Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))}fn text(&mut self)->Result<String,ProviderFailure>{let n=self.u32()? as usize;String::from_utf8(self.take(n)?.to_vec()).map_err(|_|ProviderFailure::malformed("provider text is not UTF-8"))}}

fn validate_events(events:Vec<ProviderEvent>,request:&ProviderRequest)->Result<ProviderEvidence,ProviderFailure>{if events.is_empty(){return Err(ProviderFailure::malformed("provider stream is empty"));}let mut sample_count=0usize;let mut complete=false;let mut last_spec=0u32;let mut seen=false;for(index,event)in events.iter().enumerate(){if complete{return Err(ProviderFailure::malformed("event follows final Complete"));}match event{ProviderEvent::Sample{spec,metric,..}=>{if *spec as usize>=request.specs.len()||request.specs[*spec as usize].metric!=*metric{return Err(ProviderFailure::operation(FailureClass::Incompatible,"provider sample does not match requested spec/metric"));}if seen&&*spec<last_spec{return Err(ProviderFailure::malformed("provider events are not contiguous and ordered"));}seen=true;last_spec=*spec;sample_count+=1;if sample_count>MAX_SAMPLES{return Err(ProviderFailure::malformed("provider emitted more than 1000000 samples"));}},ProviderEvent::Unavailable{spec,reason,details}=>{if *spec as usize>=request.specs.len()||reason.is_empty(){return Err(ProviderFailure::malformed("provider Unavailable has invalid spec or empty reason"));}if detail_scalars(reason,details)>MAX_DETAIL_SCALARS{return Err(ProviderFailure::malformed("provider unavailable detail exceeds 512 scalars"));}if seen&&*spec<last_spec{return Err(ProviderFailure::malformed("provider events are not contiguous and ordered"));}seen=true;last_spec=*spec;},ProviderEvent::Complete{request_id,samples}=>{if index+1!=events.len()||request_id!=&request.request_id||*samples!=sample_count as u64{return Err(ProviderFailure::malformed("provider Complete request id/count/finality mismatch"));}complete=true;}}}if !complete{return Err(ProviderFailure::malformed("provider stream has no final Complete"));}Ok(ProviderEvidence{events})}
fn detail_scalars(reason:&str,details:&[(String,String)])->usize{reason.chars().count().saturating_add(details.iter().map(|(key,value)|key.chars().count().saturating_add(value.chars().count())).sum::<usize>())}
fn is_hex64(value:&str)->bool{value.len()==64&&value.bytes().all(|b|b.is_ascii_hexdigit()&&!b.is_ascii_uppercase())}

pub fn unavailable_if_too_few(budget:&str, evidence:&ProviderEvidence, minimum:usize)->Result<(),ProviderDiagnostic>{let count=evidence.events.iter().filter(|e|matches!(e,ProviderEvent::Sample{..})).count();if let Some(ProviderEvent::Unavailable{reason,..})=evidence.events.iter().find(|e|matches!(e,ProviderEvent::Unavailable{..})){return Err(ProviderFailure::operation(FailureClass::Unavailable,reason.clone()).diagnostic(budget));}if count<minimum{return Err(ProviderFailure::operation(FailureClass::Unavailable,format!("provider returned {count} samples; policy requires {minimum}")).diagnostic(budget));}Ok(())}

#[allow(clippy::too_many_arguments)]
pub fn evaluate_provider_evidence(budget:&str,evidence_id:&str,context_key:&str,baseline_report_ids:&[String],evidence:&ProviderEvidence,spec:u32,baseline:&[Rational],percentile:Option<Percentile>,comparison:&Comparison,direction:Direction,enforcement:Enforcement,policy:Option<&MeasurementPolicy>,minimum_samples:usize)->Result<Evaluation,ProviderDiagnostic>{
    let mut samples=Vec::new();for event in &evidence.events{match event{ProviderEvent::Sample{spec:event_spec,value,..}if *event_spec==spec=>samples.push(value.clone()),ProviderEvent::Unavailable{spec:event_spec,reason,..}if *event_spec==spec=>return Err(ProviderFailure::operation(FailureClass::Unavailable,reason.clone()).diagnostic(budget)),_=>{}}}
    if samples.len()<minimum_samples{return Err(ProviderFailure::operation(FailureClass::Unavailable,format!("provider returned {} samples; policy requires {minimum_samples}",samples.len())).diagnostic(budget));}
    jet_foundation::PerformanceBudget::evaluate(evidence_id,context_key,baseline_report_ids,&samples,baseline,percentile,comparison,direction,enforcement,policy).map_err(|reason|ProviderFailure::operation(FailureClass::Execution,format!("shared evaluator rejected provider evidence: {reason}")).diagnostic(budget))
}

pub fn evaluation_diagnostic(budget:&str,evaluation:&Evaluation,direction:Direction,baseline_report_ids:&[String])->Option<ProviderDiagnostic>{
    use jet_foundation::PerformanceBudget::Evidence;match evaluation.evidence{Evidence::Pass=>None,Evidence::Unavailable=>Some(ProviderFailure::operation(FailureClass::Unavailable,"the shared evaluator found no compatible nonzero baseline evidence").diagnostic(budget)),Evidence::Regression|Evidence::Inconclusive=>{let state=if evaluation.evidence==Evidence::Regression{"regressed"}else{"is inconclusive"};let rational=|v:&Rational|format!("{}/{}",v.num,v.den);let lower=evaluation.lower95.as_ref().map(&rational).unwrap_or_else(||"none".into());let upper=evaluation.upper95.as_ref().map(&rational).unwrap_or_else(||"none".into());Some(ProviderDiagnostic{code:"E2907",what:format!("performance budget {budget} {state}"),why:format!("estimator {} with confidence [{lower}, {upper}] in {} direction did not prove the limit; baseline reports [{}]",rational(&evaluation.point),match direction{Direction::LowerIsBetter=>"lower-is-better",Direction::HigherIsBetter=>"higher-is-better"},baseline_report_ids.join(",")),fix:"improve the measured behavior, inspect the named evidence, or record an explicit exception".into()})}}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn request() -> ProviderRequest {
        ProviderRequest { schema:"jet.provider-request".into(), version:1, request_id:"1".repeat(64), provider_hash:"2".repeat(64), context_hash:"3".repeat(64), specs:vec![ProviderSpec{budget_hash:"4".repeat(64),metric:"BenchTime".into()}], workload:CanonicalJson::Null, policy:CanonicalJson::Null }
    }
    fn valid_events(request:&ProviderRequest)->Vec<ProviderEvent>{vec![ProviderEvent::Sample{spec:0,metric:"BenchTime".into(),value:Rational::integer(42)},ProviderEvent::Complete{request_id:request.request_id.clone(),samples:1}]}
    fn panic_provider(_: &ProviderRequest)->Result<Vec<ProviderEvent>,ProviderFailure>{panic!("hostile provider panic")}
    fn unavailable_provider(request:&ProviderRequest)->Result<Vec<ProviderEvent>,ProviderFailure>{Ok(vec![ProviderEvent::Unavailable{spec:0,reason:"probe could not observe ready event".into(),details:vec![]},ProviderEvent::Complete{request_id:request.request_id.clone(),samples:0}])}
    fn temporary(name:&str)->PathBuf{static NEXT:AtomicU64=AtomicU64::new(0);std::env::temp_dir().join(format!("jet-budget-provider-{}-{name}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)))}

    #[test]
    fn file_transport_round_trips_and_rejects_hostile_frames(){
        let req=request();let path=temporary("response");std::fs::write(&path,encode_stream(&valid_events(&req))).unwrap();let mut registry=ProviderRegistry::default();registry.register_file("fixture",path.clone()).unwrap();let evidence=registry.collect("fixture",&req,Duration::from_secs(1)).unwrap();assert_eq!(evidence.events,valid_events(&req));
        for (name,bytes) in [("bad-magic",b"NOTBUDGET\n".to_vec()),("truncated",{let mut b=MAGIC.to_vec();b.extend_from_slice(&[1,0,0]);b}),("trailing",{let mut b=encode_stream(&valid_events(&req));b.push(99);b})] { let hostile=temporary(name);std::fs::write(&hostile,bytes).unwrap();let mut registry=ProviderRegistry::default();registry.register_file("hostile",hostile.clone()).unwrap();let error=registry.collect("hostile",&req,Duration::from_secs(1)).unwrap_err();assert_eq!(error.diagnostic("api").code,"E2908");let _=std::fs::remove_file(hostile); }
        let _=std::fs::remove_file(path);
    }

    #[test]
    fn in_process_panic_and_unavailable_are_separate_diagnostic_classes(){
        let req=request();let mut registry=ProviderRegistry::default();registry.register_in_process("panic",panic_provider).unwrap();let failure=registry.collect("panic",&req,Duration::from_secs(1)).unwrap_err();let diagnostic=failure.diagnostic("api-p99");assert_eq!((diagnostic.code,diagnostic.what.as_str()),("E2908","performance budget operation failed"));
        registry.register_in_process("unavailable",unavailable_provider).unwrap();let evidence=registry.collect("unavailable",&req,Duration::from_secs(1)).unwrap();let diagnostic=unavailable_if_too_few("api-p99",&evidence,20).unwrap_err();assert_eq!((diagnostic.code,diagnostic.what.as_str()),("E2906","performance budget api-p99 has no usable evidence"));assert!(diagnostic.why.contains("ready event"));
        fn slow(_: &ProviderRequest)->Result<Vec<ProviderEvent>,ProviderFailure>{std::thread::sleep(Duration::from_secs(2));Ok(Vec::new())}
        registry.register_in_process("slow",slow).unwrap();let started=Instant::now();let failure=registry.collect("slow",&req,Duration::from_millis(20)).unwrap_err();assert_eq!(failure.class,FailureClass::Timeout);assert!(started.elapsed()<Duration::from_secs(1));
    }

    #[cfg(unix)]
    #[test]
    fn subprocess_uses_exact_path_and_is_bounded_and_timed(){
        use std::os::unix::fs::PermissionsExt;
        let req=request();let response=temporary("subprocess-response");std::fs::write(&response,encode_stream(&valid_events(&req))).unwrap();let script=temporary("provider.sh");std::fs::write(&script,format!("#!/bin/sh\ncat '{}'\n",response.display())).unwrap();let mut permissions=std::fs::metadata(&script).unwrap().permissions();permissions.set_mode(0o700);std::fs::set_permissions(&script,permissions).unwrap();let mut registry=ProviderRegistry::default();registry.register_subprocess("process",script.clone()).unwrap();assert_eq!(registry.collect("process",&req,Duration::from_secs(2)).unwrap().events,valid_events(&req));
        let sleeper=temporary("sleep.sh");std::fs::write(&sleeper,"#!/bin/sh\nsleep 5\n").unwrap();let mut permissions=std::fs::metadata(&sleeper).unwrap().permissions();permissions.set_mode(0o700);std::fs::set_permissions(&sleeper,permissions).unwrap();let mut registry=ProviderRegistry::default();registry.register_subprocess("slow",sleeper.clone()).unwrap();let started=Instant::now();let failure=registry.collect("slow",&req,Duration::from_millis(30)).unwrap_err();assert_eq!(failure.class,FailureClass::Timeout);assert!(started.elapsed()<Duration::from_secs(2));
        let descendant=temporary("descendant.sh");std::fs::write(&descendant,"#!/bin/sh\nsleep 5 &\nexit 0\n").unwrap();let mut permissions=std::fs::metadata(&descendant).unwrap().permissions();permissions.set_mode(0o700);std::fs::set_permissions(&descendant,permissions).unwrap();let mut registry=ProviderRegistry::default();registry.register_subprocess("descendant",descendant.clone()).unwrap();let started=Instant::now();let failure=registry.collect("descendant",&req,Duration::from_millis(30)).unwrap_err();assert_eq!(failure.class,FailureClass::Timeout);assert!(started.elapsed()<Duration::from_secs(1));
        for path in [response,script,sleeper,descendant]{let _=std::fs::remove_file(path);}
    }

    #[test]
    fn file_transport_rejects_symlinks_and_scalar_overflow(){
        let req=request();let target=temporary("target");std::fs::write(&target,encode_stream(&valid_events(&req))).unwrap();
        #[cfg(unix)]{use std::os::unix::fs::symlink;let link=temporary("link");symlink(&target,&link).unwrap();let mut registry=ProviderRegistry::default();registry.register_file("link",link.clone()).unwrap();assert!(registry.collect("link",&req,Duration::from_secs(1)).is_err());let _=std::fs::remove_file(link);}
        let events=vec![ProviderEvent::Unavailable{spec:0,reason:"x".repeat(513),details:Vec::new()},ProviderEvent::Complete{request_id:req.request_id.clone(),samples:0}];let path=temporary("detail");std::fs::write(&path,encode_stream(&events)).unwrap();let mut registry=ProviderRegistry::default();registry.register_file("detail",path.clone()).unwrap();assert!(registry.collect("detail",&req,Duration::from_secs(1)).unwrap_err().reason.contains("512 scalars"));for path in [target,path]{let _=std::fs::remove_file(path);}
    }
}
