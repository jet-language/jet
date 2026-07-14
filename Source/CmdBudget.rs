//! D-PERFBUDGET-OUTPUT1 command projection over the shared evaluator/provider/store.

use jet::AST::Item;
use jet::BudgetProviders::{terminate_group, ProviderCancellation, ProviderEvent, ProviderFailure, ProviderRegistry, ProviderRequest, ProviderSpec};
use jet::BudgetStore::{validate_baseline_name, BudgetStore, HistoryQuery, UpdateKind};
use jet::Sema::{BudgetAxis, BudgetQuantity, BudgetSpec, LocatedBudgetSpec};
use jet_foundation::PerformanceBudget::{
    estimator, evaluate, stable_id, statistics, trend as evaluate_trend, verify_budget_report, BigInt, CanonicalJson, Comparison, Direction, Enforcement, Evidence,
    Evaluation, LimitDirection, MeasurementPolicy, Percentile, PolicyOutcome, Rational, RelativeGoal, TrendLabel,
};
use jet_foundation::SHA256::sha256_hex;
use std::path::{Path, PathBuf};
use std::io::{IsTerminal, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Annotations { Auto, None, Github }

struct Options {
    command: &'static str,
    baseline: Option<String>,
    bootstrap: bool,
    accept_regression: bool,
    reason: Option<String>,
    yes: bool,
    json: bool,
    verbose: bool,
    annotations: Annotations,
}

pub(crate) fn run(raw: &[String]) -> i32 {
    let options = match parse(raw) {
        Ok(options) => options,
        Err(message) => { eprintln!("Error [E2102]: {message}\n Fix: {USAGE}"); return 2; }
    };
    let cwd = match std::env::current_dir() {
        Ok(path) => path,
        Err(error) => return tool_failure(&options, &format!("cannot resolve current workspace: {error}")),
    };
    let root = jet::Loader::find_manifest_root(&cwd).unwrap_or(cwd);
    let entry = project_entry(&root);
    if !entry.is_file() {
        return tool_failure(&options, "no project entry exists; add src/main.jet inside a project");
    }

    let measured_start = timestamp_now();
    // Preflight owns no artifact write: parse/load/sema must finish first.
    let entry_text = entry.to_string_lossy();
    let (diagnostics, bundle, effect_facts) = jet::Driver::check_file_with_effect_facts(&entry_text, None, false);
    if diagnostics.iter().any(|d| d.severity == jet::Diagnostics::Severity::Error) {
        return compiler_failure(&options, &entry, &diagnostics);
    }
    let Some(bundle) = bundle else { return tool_failure(&options, "front-end produced no checked program"); };
    let specs = match jet::Sema::collect_located_budget_specs_bundle(&bundle) {
        Ok(specs) => specs,
        Err(diagnostics) => return compiler_failure(&options, &entry, &diagnostics),
    };
    let active: Vec<_> = specs.into_iter().filter(|located| applicable(&located.spec, "native", "dev")).collect();
    let store = BudgetStore::new(&root);
    let built = match build_report(&root, &store, &bundle, &effect_facts, &active, &measured_start, "native", "dev", None, None, None) {
        Ok(report) => report,
        Err(error) => return tool_failure(&options, &error),
    };
    let report_id = text_field(&built.value, "report_id").expect("verified report id").to_string();
    let report_path = format!(".jet/perf/reports/{report_id}.json");
    let failed = built.fail > 0;

    if options.command == "check" {
        let created = match store.write_report(&built.bytes) {
            Ok((_, created)) => created,
            Err(error) => return tool_failure(&options, &format!("report write refused: {error}")),
        };
        emit_check(&options, &built, &report_id, &report_path, created);
        return if failed { 1 } else { 0 };
    }
    if failed && !options.accept_regression && !options.bootstrap {
        let created = match store.write_report(&built.bytes) {
            Ok((_, created)) => created,
            Err(error) => return tool_failure(&options, &format!("report write refused: {error}")),
        };
        emit_check(&options, &built, &report_id, &report_path, created);
        return 1;
    }
    let baseline = options.baseline.as_deref().expect("validated update baseline");
    let kind = if options.bootstrap {
        UpdateKind::Bootstrap { reason: options.reason.clone().unwrap() }
    } else if options.accept_regression {
        UpdateKind::AcceptRegression { reason: options.reason.clone().unwrap() }
    } else { UpdateKind::Pass };
    let plan = match store.plan_update(baseline, &built.bytes, kind, &timestamp_now()) {
        Ok(plan) => plan,
        Err(error) => return tool_failure_with_report(&options, &report_id, &format!("baseline plan refused: {error}")),
    };
    let old = plan.prior_head_report_id().map(str::to_string);
    let created = match store.inspect_report(&built.bytes) {
        Ok((_, exists)) => !exists,
        Err(error) => return tool_failure_with_report(&options, &report_id, &format!("report inspection refused: {error}")),
    };
    if options.json {
        if options.yes {
            if let Err(error) = store.write_report(&built.bytes) {
                return tool_failure_with_report(&options, &report_id, &format!("report write refused: {error}"));
            }
            if let Err(error) = store.apply_update(&plan) {
                return tool_failure_with_report(&options, &report_id, &format!("baseline apply refused: {error}"));
            }
        }
        emit_json(&options, &built, &report_path, Some((baseline, old.as_deref(), created)), options.yes, 0, None);
    } else {
        eprintln!("{} report {}", if created { "+" } else { "~" }, report_id);
        eprintln!("~ baseline {} {} -> {}", baseline, old.as_deref().unwrap_or("none"), report_id);
        let interactive = std::io::stdin().is_terminal();
        let apply = options.yes || (interactive && confirm_update());
        if apply {
            if let Err(error) = store.write_report(&built.bytes) {
                return tool_failure_with_report(&options, &report_id, &format!("report write refused: {error}"));
            }
            match store.apply_update(&plan) {
                Ok(_) => {}
                Err(error) => return tool_failure_with_report(&options, &report_id, &format!("baseline apply refused: {error}")),
            }
        } else if interactive {
            eprintln!("plan cancelled; no baseline changed");
        } else {
            eprintln!("plan only; pass -y or --yes to apply in a non-interactive shell");
        }
    }
    0
}

/// D-PERFBUDGET-INTEGRATION1: `jet build` owns deterministic Fail gates.
/// It reuses one verified canonical report while every relevant identity
/// remains exact, and otherwise refreshes through the same evaluator used by
/// `jet budget check`. The already-built artifact is measured directly; this
/// path never shells back into `jet build`.
pub(crate) fn run_build_gates(entry:&str, artifact_path:&Path, target:&str, profile:&str)->i32 {
    let entry_path=Path::new(entry);
    let entry=if entry_path.is_absolute(){entry_path.to_path_buf()}else{match std::env::current_dir(){Ok(cwd)=>cwd.join(entry_path),Err(error)=>return build_gate_tool_failure(&format!("cannot resolve build workspace: {error}"))}};
    let entry=entry.as_path();
    let root=jet::Loader::find_manifest_root(entry.parent().unwrap_or(Path::new("."))).unwrap_or_else(||entry.parent().unwrap_or(Path::new(".")).to_path_buf());
    let entry_text=entry.to_string_lossy();
    let(diagnostics,bundle,effect_facts)=jet::Driver::check_file_with_effect_facts(&entry_text,None,false);
    if diagnostics.iter().any(|diagnostic|diagnostic.severity==jet::Diagnostics::Severity::Error){
        eprint!("{}",jet::render_all_colored(&entry_text,&std::fs::read_to_string(entry).unwrap_or_default(),&diagnostics,false));
        return 1;
    }
    let Some(bundle)=bundle else{return build_gate_tool_failure("front-end produced no checked program")};
    let specs=match jet::Sema::collect_located_budget_specs_bundle(&bundle){Ok(specs)=>specs,Err(diagnostics)=>{eprint!("{}",jet::render_all_colored(&entry_text,&std::fs::read_to_string(entry).unwrap_or_default(),&diagnostics,false));return 1}};
    let active=specs.into_iter().filter(|located|{
        let spec=&located.spec;
        applicable(spec,target,profile)&&spec.enforcement=="Fail"&&spec.comparison_fact.kind=="Absolute"&&matches!(provider_kind(&spec.provider),"CompilerFacts"|"BuildArtifact")
    }).collect::<Vec<_>>();
    if active.is_empty(){return 0}
    let needs_artifact=active.iter().any(|located|provider_kind(&located.spec.provider)=="BuildArtifact");
    let artifact=if needs_artifact{match artifact_identity(artifact_path){Ok(artifact)=>Some(artifact),Err(error)=>return build_gate_tool_failure(&error)}}else{None};
    let store=BudgetStore::new(&root);
    match compatible_report(&root,&bundle,&active,target,profile,artifact.as_ref()){
        Ok(Some(stored))=>return emit_stored_build_gates(&stored),
        Ok(None)=>{}
        Err(error)=>return build_gate_tool_failure(&error),
    }
    let started=timestamp_now();
    let built=match build_report(&root,&store,&bundle,&effect_facts,&active,&started,target,profile,artifact,None,None){Ok(report)=>report,Err(error)=>return build_gate_tool_failure(&error)};
    let report_id=text_field(&built.value,"report_id").expect("verified report id").to_string();
    let path=format!(".jet/perf/reports/{report_id}.json");
    let created=match store.write_report(&built.bytes){Ok((_,created))=>created,Err(error)=>return build_gate_tool_failure(&format!("report write refused: {error}"))};
    let options=Options{command:"check",baseline:None,bootstrap:false,accept_regression:false,reason:None,yes:false,json:false,verbose:false,annotations:Annotations::None};
    emit_check(&options,&built,&report_id,&path,created);
    if built.fail>0{1}else{0}
}

#[derive(Clone)]
struct StoredBuildFact{name:String,source:String,evidence:String,outcome:String,reason:String}
struct StoredBuildReport{report_id:String,facts:Vec<StoredBuildFact>}

fn artifact_identity(path:&Path)->Result<ArtifactIdentity,String>{let path=if path.is_absolute(){path.to_path_buf()}else{std::env::current_dir().map_err(|error|format!("cannot resolve build output directory: {error}"))?.join(path)};let metadata=std::fs::symlink_metadata(&path).map_err(|error|format!("built artifact is unavailable: {error}"))?;if metadata.file_type().is_symlink()||!metadata.is_file(){return Err("built artifact is not a regular file".into())}let digest=jet::SHA256::sha256_file_hex(&path).map_err(|error|format!("cannot hash built artifact: {error}"))?;Ok((path,metadata.len(),digest))}

fn compatible_report(root:&Path,bundle:&jet::AST::ProgramBundle,specs:&[LocatedBudgetSpec],target:&str,profile:&str,artifact:Option<&ArtifactIdentity>)->Result<Option<StoredBuildReport>,String>{
    let needs_artifact=specs.iter().any(|located|provider_kind(&located.spec.provider)=="BuildArtifact");
    let expected_subject=subject(root,bundle,"identity","identity",target,profile,if needs_artifact{artifact}else{None})?;
    let expected_subject=as_object(&expected_subject)?;
    let expected_tool_value=toolchain()?;let expected_tool=as_object(&expected_tool_value)?;
    let expected_ids=specs.iter().map(|located|format!("{}:{}",located.spec.role,located.spec.name)).collect::<std::collections::BTreeSet<_>>();
    let mut expected=std::collections::BTreeMap::new();
    for located in specs { let provider=provider(provider_kind(&located.spec.provider),provider_identity(&located.spec.provider))?;let(base,hash,context)=measurement_base_truthful(root,bundle,located,&CanonicalJson::Object(expected_subject.clone()),&expected_tool_value,&provider,target,profile)?;let _=base;expected.insert(format!("{}:{}",located.spec.role,located.spec.name),(hash,context,provider)); }
    let dir=root.join(".jet/perf/reports");let Ok(entries)=std::fs::read_dir(dir)else{return Ok(None)};
    let mut paths=entries.filter_map(Result::ok).map(|entry|entry.path()).filter(|path|path.extension().and_then(|ext|ext.to_str())==Some("json")).collect::<Vec<_>>();paths.sort();paths.reverse();
    for path in paths{
        let Ok(bytes)=std::fs::read(path)else{continue};let Ok(report)=verify_budget_report(&bytes)else{continue};let Ok(wrapper)=as_object(&report)else{continue};let Ok(report_id)=text_map(wrapper,"report_id")else{continue};let Some(content)=wrapper.get("content")else{continue};let Ok(content)=as_object(content)else{continue};let Some(actual_subject)=content.get("subject")else{continue};let Ok(actual_subject)=as_object(actual_subject)else{continue};let Some(actual_tool)=content.get("toolchain")else{continue};let Ok(actual_tool)=as_object(actual_tool)else{continue};
        if actual_subject.get("member_sources")!=expected_subject.get("member_sources")||actual_subject.get("artifact")!=expected_subject.get("artifact")||actual_subject.get("target_class")!=expected_subject.get("target_class")||actual_subject.get("target_triple")!=expected_subject.get("target_triple")||actual_subject.get("profile")!=expected_subject.get("profile")||actual_tool.get("digest")!=expected_tool.get("digest"){continue}
        let Some(CanonicalJson::Array(measurements))=content.get("measurements")else{continue};let mut facts=Vec::new();let mut ids=std::collections::BTreeSet::new();let mut valid=true;
        for measurement in measurements{let Ok(measurement)=as_object(measurement)else{valid=false;break};let Ok(budget_id)=text_map(measurement,"budget_id")else{valid=false;break};let Some((hash,context,expected_provider))=expected.get(budget_id)else{valid=false;break};if measurement.get("budget_spec_sha256")!=Some(&CanonicalJson::String(hash.clone()))||measurement.get("context_key")!=Some(&CanonicalJson::String(context.clone()))||measurement.get("provider")!=Some(expected_provider){valid=false;break}let Some(decision)=measurement.get("decision")else{valid=false;break};let Ok(decision)=as_object(decision)else{valid=false;break};let(Ok(evidence),Ok(outcome),Ok(reason),Ok(source))=(text_map(decision,"evidence"),text_map(decision,"policy_outcome"),text_map(decision,"reason"),text_map(measurement,"source"))else{valid=false;break};if !expected_ids.contains(budget_id){valid=false;break}ids.insert(budget_id.to_string());let name=measurement.get("budget_spec").and_then(|value|as_object(value).ok()).and_then(|spec|text_map(spec,"name").ok()).unwrap_or_else(||budget_id.rsplit_once(':').map(|(_,name)|name).unwrap_or(budget_id));facts.push(StoredBuildFact{name:name.into(),source:source.into(),evidence:evidence.into(),outcome:outcome.into(),reason:reason.into()});}
        if valid&&ids==expected_ids{return Ok(Some(StoredBuildReport{report_id:report_id.into(),facts}))}
    }
    Ok(None)
}

fn emit_stored_build_gates(stored:&StoredBuildReport)->i32{let mut failed=0;for fact in &stored.facts{if fact.outcome=="fail"{failed+=1;let code=if fact.evidence=="unavailable"{"E2906"}else{"E2907"};let state=if fact.evidence=="unavailable"{"has no usable evidence"}else if fact.evidence=="inconclusive"{"is inconclusive"}else{"regressed"};eprintln!("Error [{code}]: performance budget {} {state}\n --> {}\n Why: {}\n Fix: {}",fact.name,fact.source,fact.reason,if code=="E2906"{"correct the provider evidence or bootstrap only when absent or stale evidence is eligible"}else{"improve the measured behavior, inspect `jet budget check --verbose`, or record an explicit exception"});}}let short=&stored.report_id[..12];if failed>0{eprintln!("budgets failed: {} · report {short}",count(failed,"budget failed","budgets failed"));1}else{eprintln!("budgets: {} passed · report {short}",count(stored.facts.len(),"budget","budgets"));0}}
fn build_gate_tool_failure(why:&str)->i32{eprintln!("Error [E2908]: performance budget operation failed\n Why: {why}\n Fix: correct the named failure and retry");1}

/// `jet bench` owns BenchMeasurement and Bench-scoped AllocationProbe refresh.
/// Both come from the same harness invocation; no second workload pass exists.
pub(crate) fn reuse_bench_report(entry:&str)->Option<i32>{
    let entry=match std::path::Path::new(entry).canonicalize(){Ok(path)=>path,Err(error)=>return Some(build_gate_tool_failure(&format!("cannot resolve benchmark entry: {error}")))};
    let root=jet::Loader::find_manifest_root(entry.parent().unwrap_or(Path::new("."))).unwrap_or_else(||entry.parent().unwrap_or(Path::new(".")).to_path_buf());
    let entry_text=entry.to_string_lossy();
    let(diagnostics,bundle,_)=jet::Driver::check_file_with_effect_facts(&entry_text,None,false);
    if diagnostics.iter().any(|diagnostic|diagnostic.severity==jet::Diagnostics::Severity::Error){return None}
    let bundle=bundle?;
    let specs=jet::Sema::collect_located_budget_specs_bundle(&bundle).ok()?;
    let active=specs.into_iter().filter(|located|bench_owned(&located.spec)&&applicable(&located.spec,"native","bench")).collect::<Vec<_>>();
    if active.is_empty(){return None}
    match compatible_report(&root,&bundle,&active,"native","bench",None){Ok(Some(stored))=>Some(emit_stored_build_gates(&stored)),Ok(None)=>None,Err(error)=>Some(build_gate_tool_failure(&error))}
}

pub(crate) fn run_bench_refresh(entry:&str,evidence:&[crate::CmdDevTools::BenchEvidence])->i32{
    let entry=match std::path::Path::new(entry).canonicalize(){Ok(path)=>path,Err(error)=>return build_gate_tool_failure(&format!("cannot resolve benchmark entry: {error}"))};
    let root=jet::Loader::find_manifest_root(entry.parent().unwrap_or(Path::new("."))).unwrap_or_else(||entry.parent().unwrap_or(Path::new(".")).to_path_buf());
    let entry_text=entry.to_string_lossy();
    let(diagnostics,bundle,effect_facts)=jet::Driver::check_file_with_effect_facts(&entry_text,None,false);
    if diagnostics.iter().any(|diagnostic|diagnostic.severity==jet::Diagnostics::Severity::Error){eprint!("{}",jet::render_all_colored(&entry_text,&std::fs::read_to_string(&entry).unwrap_or_default(),&diagnostics,false));return 1}
    let Some(bundle)=bundle else{return build_gate_tool_failure("front-end produced no checked benchmark program")};
    let specs=match jet::Sema::collect_located_budget_specs_bundle(&bundle){Ok(specs)=>specs,Err(diagnostics)=>{eprint!("{}",jet::render_all_colored(&entry_text,&std::fs::read_to_string(&entry).unwrap_or_default(),&diagnostics,false));return 1}};
    let active=specs.into_iter().filter(|located|bench_owned(&located.spec)&&applicable(&located.spec,"native","bench")).collect::<Vec<_>>();
    if active.is_empty(){return 0}
    match compatible_report(&root,&bundle,&active,"native","bench",None){Ok(Some(stored))=>return emit_stored_build_gates(&stored),Ok(None)=>{},Err(error)=>return build_gate_tool_failure(&error)}
    let store=BudgetStore::new(&root);let started=timestamp_now();
    let built=match build_report(&root,&store,&bundle,&effect_facts,&active,&started,"native","bench",None,Some(evidence),None){Ok(report)=>report,Err(error)=>return build_gate_tool_failure(&error)};
    let report_id=text_field(&built.value,"report_id").expect("verified report id").to_string();let path=format!(".jet/perf/reports/{report_id}.json");
    let created=match store.write_report(&built.bytes){Ok((_,created))=>created,Err(error)=>return build_gate_tool_failure(&format!("report write refused: {error}"))};
    let options=Options{command:"check",baseline:None,bootstrap:false,accept_regression:false,reason:None,yes:false,json:false,verbose:false,annotations:Annotations::None};emit_check(&options,&built,&report_id,&path,created);
    if built.fail>0{1}else{0}
}

fn confirm_update() -> bool {
    eprint!("Apply? [y/N] ");
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() { return false; }
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

const USAGE: &str = "jet budget check [--json] [--verbose] [--annotations auto|none|github] | jet budget update --baseline <name> [--bootstrap|--accept-regression] [--reason <text>] [-y|--yes] [--json] [--verbose] [--annotations auto|none|github]";

fn parse(raw: &[String]) -> Result<Options, String> {
    let command = match raw.get(1).map(String::as_str) {
        Some("check") => "check", Some("update") => "update",
        Some(other) => return Err(format!("unknown `jet budget` subcommand `{other}`")),
        None => return Err("`jet budget` needs `check` or `update`".into()),
    };
    let mut out = Options { command, baseline:None, bootstrap:false, accept_regression:false, reason:None, yes:false, json:false, verbose:false, annotations:Annotations::Auto };
    let mut seen = std::collections::BTreeSet::new();
    let mut index = 2;
    while index < raw.len() {
        let flag = raw[index].as_str();
        let key = match flag { "-y" => "--yes", value => value };
        if !seen.insert(key.to_string()) { return Err(format!("flag `{flag}` is repeated")); }
        match flag {
            "--json" => out.json = true,
            "--verbose" => out.verbose = true,
            "-y" | "--yes" => out.yes = true,
            "--bootstrap" => out.bootstrap = true,
            "--accept-regression" => out.accept_regression = true,
            "--baseline" | "--reason" | "--annotations" => {
                index += 1;
                let value = raw.get(index).filter(|v| !v.starts_with('-')).ok_or_else(|| format!("flag `{flag}` needs a value"))?;
                match flag {
                    "--baseline" => out.baseline = Some(value.clone()),
                    "--reason" => out.reason = Some(value.clone()),
                    _ => out.annotations = match value.as_str() { "auto"=>Annotations::Auto,"none"=>Annotations::None,"github"=>Annotations::Github,_=>return Err("`--annotations` accepts auto, none, or github".into()) },
                }
            }
            other => return Err(format!("unknown flag or argument `{other}`")),
        }
        index += 1;
    }
    if command == "check" && (out.baseline.is_some() || out.bootstrap || out.accept_regression || out.reason.is_some() || out.yes) {
        return Err("update-only flags cannot be used with `budget check`".into());
    }
    if command == "update" && out.baseline.is_none() { return Err("`budget update` requires `--baseline <name>`".into()); }
    if let Some(baseline)=out.baseline.as_deref(){validate_baseline_name(baseline).map_err(|error|format!("invalid `--baseline` value: {error}"))?;}
    if out.bootstrap && out.accept_regression { return Err("`--bootstrap` and `--accept-regression` are mutually exclusive".into()); }
    if (out.bootstrap || out.accept_regression) != out.reason.is_some() { return Err("`--reason` is required with, and only accepted with, bootstrap or accept-regression".into()); }
    if out.json { out.annotations = Annotations::None; }
    Ok(out)
}

struct BuiltReport { value: CanonicalJson, bytes: Vec<u8>, results: Vec<ResultRow>, fail:usize }
struct ResultRow { id:String, name:String, source:String, line:usize, column:usize, metric:CanonicalJson, metric_label:String, unit:String, direction:String, comparison:CanonicalJson, sample:Rational, lower95:Option<Rational>, upper95:Option<Rational>, trend:CanonicalJson, baseline_report_ids:Vec<String>, stale:bool, outcome:PolicyOutcome, evidence:Evidence, enforcement:Enforcement, reason:String }

type ArtifactIdentity = (PathBuf, u64, String);

fn build_report(root:&Path, store:&BudgetStore, bundle:&jet::AST::ProgramBundle, effect_facts:&jet::Sema::SemIndexEffectFacts, specs:&[LocatedBudgetSpec], at:&str, target:&str, profile:&str, supplied_artifact:Option<ArtifactIdentity>, bench_evidence:Option<&[crate::CmdDevTools::BenchEvidence]>, dev_evidence:Option<&DevEvidence>)->Result<BuiltReport,String>{
    for located in specs { let spec=&located.spec; if !matches!(provider_kind(&spec.provider),"CompilerFacts"|"BuildArtifact"|"BenchMeasurement"|"AllocationProbe"|"ServiceProbe"|"SceneProbe") { return Err(format!("provider `{}` for budget `{}` has no command-owned measurement implementation",spec.provider,spec.name)); } }
    let mut ordered=specs.iter().collect::<Vec<_>>();ordered.sort_by(|a,b|a.spec.name.cmp(&b.spec.name));
    let needs_artifact=ordered.iter().any(|located|provider_kind(&located.spec.provider)=="BuildArtifact");
    let artifact=if needs_artifact{Some(match supplied_artifact{Some(artifact)=>artifact,None=>build_selected_artifact(root,&project_entry(root))?})}else{None};
    let context_subject=subject(root,bundle,at,at,target,profile,artifact.as_ref())?;let tool=toolchain()?;
    let providers=ordered.iter().map(|located|provider(provider_kind(&located.spec.provider),provider_identity(&located.spec.provider))).collect::<Result<Vec<_>,_>>()?;
    let mut bases=Vec::new();
    for (located,provider) in ordered.iter().zip(&providers) { bases.push(measurement_base_truthful(root,bundle,located,&context_subject,&tool,provider,target,profile)?); }
    let mut groups=std::collections::BTreeMap::<String,Vec<usize>>::new();
    for (index,located) in ordered.iter().enumerate(){groups.entry(located.spec.provider.clone()).or_default().push(index);}
    let mut registry=ProviderRegistry::with_builtins();
    registry.register_in_process("BenchMeasurement",bench_measurement_provider).map_err(|error|format!("cannot register BenchMeasurement: {error}"))?;
    registry.register_in_process("AllocationProbe",allocation_probe_provider).map_err(|error|format!("cannot register AllocationProbe: {error}"))?;
    registry.register_in_process("ServiceProbe",service_probe_provider).map_err(|error|format!("cannot register ServiceProbe: {error}"))?;
    registry.register_in_process("SceneProbe",scene_probe_provider).map_err(|error|format!("cannot register SceneProbe: {error}"))?;
    let mut samples=vec![Vec::new();ordered.len()];
    for (provider_name,indices) in groups {
        let kind=provider_kind(&provider_name);
        if matches!(kind,"BenchMeasurement"|"AllocationProbe") {
            if let Some(evidence)=bench_evidence {
                let name=provider_identity(&provider_name);
                let bench=evidence.iter().find(|bench|bench.name==name).ok_or_else(||format!("#Bench `{name}` was not emitted by its selected benchmark target"))?;
                for index in indices {
                    if kind=="BenchMeasurement" {
                        if !ordered[index].spec.metric.starts_with("BenchTime("){return Err(format!("BenchMeasurement does not support metric `{}`",ordered[index].spec.metric))}
                        samples[index]=bench.samples.iter().map(|(elapsed,iters)|Rational::parse(&elapsed.to_string(),&iters.to_string())).collect::<Result<Vec<_>,_>>()?;
                    } else {
                        let metric=ordered[index].spec.metric.as_str();
                        if !matches!(metric,"AllocationCount"|"AllocationBytes"){return Err(format!("AllocationProbe does not support metric `{metric}`"))}
                        if bench.allocation_samples.len()!=20{return Err(format!("AllocationProbe `{name}` returned {} samples; policy requires 20",bench.allocation_samples.len()))}
                        samples[index]=bench.allocation_samples.iter().map(|(count,bytes,iters)|{
                            let numerator=if metric=="AllocationCount"{count}else{bytes};
                            Rational::parse(&numerator.to_string(),&iters.to_string())
                        }).collect::<Result<Vec<_>,_>>()?;
                    }
                }
                continue;
            }
        }
        if kind=="ServiceProbe" {
            if let Some(evidence)=dev_evidence {
                let name=provider_identity(&provider_name);
                let svc=evidence.service.iter().find(|s|s.name==name).ok_or_else(||format!("ServiceProbe `{name}` was not collected; run `jet dev <file>` to refresh service evidence"))?;
                if svc.samples_ns.len()!=20{return Err(format!("ServiceProbe `{name}` returned {} samples; policy requires 20",svc.samples_ns.len()))}
                for index in indices {
                    if ordered[index].spec.metric!="ServiceReadiness"{return Err(format!("ServiceProbe does not support metric `{}`",ordered[index].spec.metric))}
                    samples[index]=svc.samples_ns.iter().map(|ns|Rational::parse(&ns.to_string(),"1")).collect::<Result<Vec<_>,_>>()?;
                }
                continue;
            }
        }
        if kind=="SceneProbe" {
            if let Some(evidence)=dev_evidence {
                let name=provider_identity(&provider_name);
                let scene=evidence.scene.iter().find(|s|s.name==name).ok_or_else(||format!("SceneProbe `{name}` was not collected; run `jet dev <file>` to refresh scene evidence"))?;
                for index in &indices {
                    let metric=ordered[*index].spec.metric.split('(').next().unwrap_or(&ordered[*index].spec.metric);
                    let values=match metric{
                        "FrameTime"=>&scene.frame_ns,
                        "DrawCalls"=>&scene.draw_calls,
                        "SceneAssetBytes"=>&scene.asset_bytes,
                        "MemoryHighWater"=>&scene.rss_hwm,
                        other=>return Err(format!("SceneProbe does not support metric `{other}`")),
                    };
                    if values.len()!=20{return Err(format!("SceneProbe `{name}` returned {} samples for `{metric}`; policy requires 20",values.len()))}
                    samples[*index]=values.iter().map(|v|Rational::parse(&v.to_string(),"1")).collect::<Result<Vec<_>,_>>()?;
                }
                continue;
            }
        }
        let provider=&providers[indices[0]];
        let workload=if kind=="BuildArtifact"{
            CanonicalJson::object([("path".into(),CanonicalJson::String(artifact.as_ref().expect("built artifact").0.to_string_lossy().into_owned()))])?
        }else if matches!(kind,"BenchMeasurement"|"AllocationProbe"){
            CanonicalJson::object([("name".into(),CanonicalJson::String(provider_identity(&provider_name).into())),("path".into(),CanonicalJson::String(project_entry(root).to_string_lossy().into_owned()))])?
        }else if matches!(kind,"ServiceProbe"|"SceneProbe"){
            CanonicalJson::object([("name".into(),CanonicalJson::String(provider_identity(&provider_name).into())),("path".into(),CanonicalJson::String(project_entry(root).to_string_lossy().into_owned()))])?
        }else{
            CanonicalJson::Array(indices.iter().map(|index|compiler_fact(bundle,effect_facts,&ordered[*index].spec).map(|value|CanonicalJson::Integer(value.to_string()))).collect::<Result<Vec<_>,_>>()?)
        };
        let requests=indices.iter().map(|index|ProviderSpec{budget_hash:bases[*index].1.clone(),metric:ordered[*index].spec.metric.clone()}).collect();
        let request=ProviderRequest{schema:"jet.provider-request".into(),version:1,request_id:stable_id(&workload),provider_hash:stable_id(provider),context_hash:stable_id(&context_subject),specs:requests,workload,policy:CanonicalJson::Null};
        let evidence=registry.collect(kind,&request,Duration::from_secs(30)).map_err(|e|e.reason)?;
        for event in evidence.events {
            match event {
                ProviderEvent::Sample{spec,value,..}=>{
                    let global=*indices.get(spec as usize).ok_or("provider sample index escaped its measurement group")?;
                    samples[global].push(value);
                }
                ProviderEvent::Unavailable{spec,reason,..}=>{
                    let global=*indices.get(spec as usize).ok_or("provider unavailable index escaped its measurement group")?;
                    return Err(format!("provider `{provider_name}` has no evidence for budget `{}`: {reason}",ordered[global].spec.name));
                }
                ProviderEvent::Complete{..}=>{}
            }
        }
    }
    for(index,values)in samples.iter().enumerate(){let statistical=ordered[index].spec.comparison_fact.kind!="Absolute";if values.is_empty()||(!statistical&&values.len()!=1)||statistical&&values.len()<20{return Err(format!("provider `{}` returned {} samples for budget `{}`; policy requires {}",ordered[index].spec.provider,values.len(),ordered[index].spec.name,if statistical{20}else{1}));}}
    let subject=subject(root,bundle,at,&timestamp_now(),target,profile,artifact.as_ref())?;
    for (index, provider_samples) in samples.iter().enumerate() {
        let metric_name=ordered[index].spec.metric.split('(').next().unwrap_or(&ordered[index].spec.metric);
        let context=context_key(&subject,&tool,&providers[index],metric_name,metric_percentile(&ordered[index].spec.metric))?;
        let mut base = as_object(&bases[index].0)?.clone();
        base.insert("samples".into(), CanonicalJson::Array(provider_samples.iter().map(Rational::to_json).collect()));
        if ordered[index].spec.comparison_fact.kind!="Absolute"{base.insert("statistics".into(),statistics_json(&provider_samples)?);}
        base.insert("context_key".into(), CanonicalJson::String(context.clone()));
        bases[index].0 = CanonicalJson::Object(base);
        bases[index].2 = context;
    }
    let skeletons=bases.iter().map(|(base,_,_)|base.clone()).collect::<Vec<_>>();
    let evidence_id=stable_id(&CanonicalJson::object([("measurements".into(),CanonicalJson::Array(skeletons)),("subject".into(),subject.clone()),("toolchain".into(),tool.clone())])?);
    let privacy=privacy_json()?;
    let provisional=report_wrapper(&evidence_id,bases.iter().map(|base|base.0.clone()).collect(),subject.clone(),tool.clone(),privacy.clone(),0,0,0)?;
    let provisional_bytes=provisional.bytes();
    let mut measurements=Vec::new();let mut results=Vec::new();let(mut pass,mut warn,mut fail)=(0,0,0);
    for (index, spec) in ordered.iter().enumerate() {
        let provider_samples=&samples[index];
        let comparison = comparison_model(&spec)?;
        let improvement = if spec.metric.starts_with("Throughput") { Direction::HigherIsBetter } else { Direction::LowerIsBetter };
        let enforcement = if spec.enforcement == "Warn" { Enforcement::Warn } else { Enforcement::Fail };
        let mut history_ids=Vec::new();let mut history_samples=Vec::new();let mut history_state=None;let mut stale=false;let mut stale_age=None;let mut stale_after=None;
        if let Some(baseline)=spec.comparison_fact.baseline.as_deref(){
            let query=HistoryQuery{budget_id:format!("{}:{}",spec.role,spec.name),budget_spec_sha256:bases[index].1.clone(),context_key:bases[index].2.clone(),at:at.into()};
            match store.select_compatible_history(baseline,&provisional_bytes,&query){
                Ok(selection)=>{history_ids=selection.report_ids;if !history_ids.is_empty(){history_state=Some(selection.state_id);stale=selection.stale;stale_age=selection.newest_age_seconds;stale_after=Some(selection.stale_after_seconds);history_samples=store.load_history_samples(&history_ids,&query.budget_id)?;}}
                Err(error)if error==format!("baseline `{baseline}` is absent")=>{}
                Err(error)=>return Err(error),
            }
        }
        let pooled=if stale{Vec::new()}else{history_samples.iter().flatten().cloned().collect::<Vec<_>>()};
        let policy=evaluation_policy();
        let evaluation = if matches!(comparison,Comparison::RelativeTo{..})&&pooled.is_empty(){let point=estimator(provider_samples,percentile(&spec.metric))?;Evaluation{point,lower95:None,upper95:None,evidence:Evidence::Unavailable,outcome:if enforcement==Enforcement::Warn{PolicyOutcome::Warn}else{PolicyOutcome::Fail},bootstrap:Vec::new()}}else{evaluate(&evidence_id, &bases[index].2, &history_ids, provider_samples, &pooled, percentile(&spec.metric), &comparison, improvement, enforcement,if spec.comparison_fact.kind=="Absolute"{None}else{Some(&policy)})?};
        match evaluation.outcome { PolicyOutcome::Pass => pass += 1, PolicyOutcome::Warn => warn += 1, PolicyOutcome::Fail => fail += 1 }
        let evidence_name = match evaluation.evidence { Evidence::Pass => "pass", Evidence::Regression => "regression", Evidence::Inconclusive => "inconclusive", Evidence::Unavailable => "unavailable" };
        let outcome = match evaluation.outcome { PolicyOutcome::Pass => "pass", PolicyOutcome::Warn => "warn", PolicyOutcome::Fail => "fail" };
        let reason = if stale{format!("named baseline compatible history is stale: newest generation is {} seconds old; policy limit is {} seconds",stale_age.expect("stale selection age"),stale_after.expect("stale selection policy"))}else if evaluation.evidence==Evidence::Unavailable{"named baseline has no compatible history".into()}else{format!("measured estimator {} under {} policy",evaluation.point.num,spec.comparison_fact.kind)};
        let trend=trend_json(&history_ids,&history_samples,percentile(&spec.metric),improvement)?;
        let decision = CanonicalJson::object([("evidence".into(), CanonicalJson::String(evidence_name.into())), ("lower95".into(), evaluation.lower95.as_ref().map(Rational::to_json).unwrap_or(CanonicalJson::Null)), ("point".into(), evaluation.point.to_json()), ("policy_outcome".into(), CanonicalJson::String(outcome.into())), ("reason".into(), CanonicalJson::String(reason.clone())), ("trend".into(), trend.clone()), ("upper95".into(), evaluation.upper95.as_ref().map(Rational::to_json).unwrap_or(CanonicalJson::Null))])?;
        let mut object = as_object(&bases[index].0)?.clone();
        if let Some(state)=history_state{let history=CanonicalJson::object([("report_ids".into(),CanonicalJson::Array(history_ids.iter().cloned().map(CanonicalJson::String).collect())),("state_id".into(),CanonicalJson::String(state))])?;object.insert("history".into(),history.clone());if matches!(comparison,Comparison::RelativeTo{..})&&!pooled.is_empty(){object.insert("baseline".into(),CanonicalJson::object([("history".into(),history),("pooled_samples".into(),CanonicalJson::Array(pooled.iter().map(Rational::to_json).collect())),("policy".into(),policy_json()),("statistics".into(),statistics_json(&pooled)?)])?);}}
        object.insert("decision".into(), decision);
        measurements.push(CanonicalJson::Object(object));
        let (source, line, column) = source_location(root, bundle, spec);
        let comparison_json = comparison_json(&spec)?;
        let metric=CanonicalJson::object([("name".into(),CanonicalJson::String(spec.metric.split('(').next().unwrap_or(&spec.metric).into())),("percentile".into(),metric_percentile(&spec.metric).map(|value|CanonicalJson::String(value.into())).unwrap_or(CanonicalJson::Null))])?;
        results.push(ResultRow { id:format!("{}:{}",spec.role,spec.name), name:spec.name.clone(), source, line, column, metric, metric_label:spec.metric.clone(), unit:metric_unit(&spec.metric).into(), direction:if improvement==Direction::LowerIsBetter{"lower_is_better".into()}else{"higher_is_better".into()}, comparison:comparison_json, sample:evaluation.point.clone(), lower95:evaluation.lower95.clone(), upper95:evaluation.upper95.clone(), trend, baseline_report_ids:history_ids, stale, outcome:evaluation.outcome, evidence:evaluation.evidence, enforcement, reason });
    }
    results.sort_by(|a,b|result_rank(a).cmp(&result_rank(b)).then_with(||a.id.cmp(&b.id)));
    let value=report_wrapper(&evidence_id,measurements,subject,tool,privacy,pass,warn,fail)?;let bytes=value.bytes();jet_foundation::PerformanceBudget::verify_budget_report(&bytes)?;Ok(BuiltReport{value,bytes,results,fail})
}

/// ServiceProbe evidence: 20 startup-latency samples (nanoseconds) for one
/// named service. Produced by `jetpack::Services::measure_readiness`.
#[derive(Clone, Debug)]
pub(crate) struct ServiceEvidence {
    pub(crate) name: String,
    /// Exactly 20 nanosecond durations — one per down→up→ready trial.
    pub(crate) samples_ns: Vec<u64>,
}

/// SceneEvidence: per-frame samples for a named game scene produced by
/// `jet_game_run` in probe mode (`JET_SCENE_PROBE=<name>`).
#[derive(Clone, Debug)]
pub(crate) struct SceneEvidence {
    pub(crate) name: String,
    /// Exactly 20 frame-callback elapsed-nanosecond values.
    pub(crate) frame_ns: Vec<u64>,
    /// Exactly 20 draw-call counts (callbacks per frame).
    pub(crate) draw_calls: Vec<u64>,
    /// Exactly 20 SceneAssetBytes values (constant across frames; real file sizes).
    pub(crate) asset_bytes: Vec<u64>,
    /// Exactly 20 MemoryHighWater (VmHWM) values (bytes).
    pub(crate) rss_hwm: Vec<u64>,
}

/// All dev-owned evidence: one ServiceEvidence per ServiceProbe + one
/// SceneEvidence per SceneProbe.
struct DevEvidence {
    service: Vec<ServiceEvidence>,
    scene: Vec<SceneEvidence>,
}

/// D-PERFBUDGET-INTEGRATION1: `jet dev` owns ServiceProbe + SceneProbe
/// refresh. Mirrors `run_bench_refresh` exactly: reuses a compatible cached
/// report when digests are unchanged; otherwise measures and writes a new one.
pub(crate) fn run_dev_refresh(entry: &str, service_evidence: &[ServiceEvidence], scene_evidence: &[SceneEvidence]) -> i32 {
    let entry = match std::path::Path::new(entry).canonicalize() {
        Ok(path) => path,
        Err(error) => return build_gate_tool_failure(&format!("cannot resolve dev entry: {error}")),
    };
    let root = jet::Loader::find_manifest_root(entry.parent().unwrap_or(Path::new(".")))
        .unwrap_or_else(|| entry.parent().unwrap_or(Path::new(".")).to_path_buf());
    let entry_text = entry.to_string_lossy();
    let (diagnostics, bundle, effect_facts) = jet::Driver::check_file_with_effect_facts(&entry_text, None, false);
    if diagnostics.iter().any(|d| d.severity == jet::Diagnostics::Severity::Error) {
        eprint!("{}", jet::render_all_colored(&entry_text, &std::fs::read_to_string(&entry).unwrap_or_default(), &diagnostics, false));
        return 1;
    }
    let Some(bundle) = bundle else { return build_gate_tool_failure("front-end produced no checked dev program") };
    let specs = match jet::Sema::collect_located_budget_specs_bundle(&bundle) {
        Ok(specs) => specs,
        Err(diagnostics) => {
            eprint!("{}", jet::render_all_colored(&entry_text, &std::fs::read_to_string(&entry).unwrap_or_default(), &diagnostics, false));
            return 1;
        }
    };
    let active = specs.into_iter()
        .filter(|located| dev_owned(&located.spec) && applicable(&located.spec, "native", "dev"))
        .collect::<Vec<_>>();
    if active.is_empty() { return 0; }
    match compatible_report(&root, &bundle, &active, "native", "dev", None) {
        Ok(Some(stored)) => return emit_stored_build_gates(&stored),
        Ok(None) => {}
        Err(error) => return build_gate_tool_failure(&error),
    }
    let evidence = DevEvidence { service: service_evidence.to_vec(), scene: scene_evidence.to_vec() };
    let store = BudgetStore::new(&root);
    let started = timestamp_now();
    let built = match build_report(&root, &store, &bundle, &effect_facts, &active, &started, "native", "dev", None, None, Some(&evidence)) {
        Ok(report) => report,
        Err(error) => return build_gate_tool_failure(&error),
    };
    let report_id = text_field(&built.value, "report_id").expect("verified report id").to_string();
    let path = format!(".jet/perf/reports/{report_id}.json");
    let created = match store.write_report(&built.bytes) {
        Ok((_, created)) => created,
        Err(error) => return build_gate_tool_failure(&format!("report write refused: {error}")),
    };
    let options = Options { command: "check", baseline: None, bootstrap: false, accept_regression: false, reason: None, yes: false, json: false, verbose: false, annotations: Annotations::None };
    emit_check(&options, &built, &report_id, &path, created);
    if built.fail > 0 { 1 } else { 0 }
}

fn dev_owned(spec: &BudgetSpec) -> bool {
    matches!(provider_kind(&spec.provider), "ServiceProbe" | "SceneProbe")
        && (spec.scope.starts_with("Service(") || spec.scope.starts_with("Scene("))
}

fn bench_measurement_provider(request:&ProviderRequest,_:&ProviderCancellation)->Result<Vec<ProviderEvent>,ProviderFailure>{
    let CanonicalJson::Object(workload)=&request.workload else{return Err(ProviderFailure::malformed("BenchMeasurement workload is not an object"))};
    let Some(CanonicalJson::String(path))=workload.get("path") else{return Err(ProviderFailure::malformed("BenchMeasurement workload has no source path"))};
    let Some(CanonicalJson::String(name))=workload.get("name") else{return Err(ProviderFailure::malformed("BenchMeasurement workload has no benchmark name"))};
    let source=std::fs::read_to_string(path).map_err(|error|ProviderFailure::malformed(format!("cannot read benchmark source: {error}")))?;
    let mode=crate::OutputMode{json:false,color:jet::Diagnostics::ColorChoice::Never};
    let evidence=crate::CmdDevTools::collect_bench_evidence(path,&source,mode,false);
    let bench=evidence.into_iter().find(|bench|bench.name==*name).ok_or_else(||ProviderFailure::malformed(format!("#Bench `{name}` was not emitted by its selected benchmark target")))?;
    let mut events=Vec::new();
    for(index,spec)in request.specs.iter().enumerate(){if !spec.metric.starts_with("BenchTime("){return Err(ProviderFailure::malformed(format!("BenchMeasurement does not support metric `{}`",spec.metric)))}for(elapsed,iters)in &bench.samples{let value=Rational::parse(&elapsed.to_string(),&iters.to_string()).map_err(ProviderFailure::malformed)?;events.push(ProviderEvent::Sample{spec:index as u32,metric:spec.metric.clone(),value});}}
    let samples=events.len()as u64;events.push(ProviderEvent::Complete{request_id:request.request_id.clone(),samples});Ok(events)
}

fn allocation_probe_provider(request:&ProviderRequest,_:&ProviderCancellation)->Result<Vec<ProviderEvent>,ProviderFailure>{
    let CanonicalJson::Object(workload)=&request.workload else{return Err(ProviderFailure::malformed("AllocationProbe workload is not an object"))};
    let Some(CanonicalJson::String(path))=workload.get("path") else{return Err(ProviderFailure::malformed("AllocationProbe workload has no source path"))};
    let Some(CanonicalJson::String(name))=workload.get("name") else{return Err(ProviderFailure::malformed("AllocationProbe workload has no benchmark name"))};
    let source=std::fs::read_to_string(path).map_err(|error|ProviderFailure::malformed(format!("cannot read allocation workload source: {error}")))?;
    let mode=crate::OutputMode{json:false,color:jet::Diagnostics::ColorChoice::Never};
    let evidence=crate::CmdDevTools::collect_bench_evidence(path,&source,mode,false);
    let bench=evidence.into_iter().find(|bench|bench.name==*name).ok_or_else(||ProviderFailure::malformed(format!("#Bench `{name}` was not emitted by its selected allocation workload")))?;
    if bench.allocation_samples.len()!=20{return Err(ProviderFailure::malformed(format!("AllocationProbe `{name}` emitted {} trials; policy requires 20",bench.allocation_samples.len())))}
    let mut events=Vec::new();
    for(index,spec)in request.specs.iter().enumerate(){
        if !matches!(spec.metric.as_str(),"AllocationCount"|"AllocationBytes"){return Err(ProviderFailure::malformed(format!("AllocationProbe does not support metric `{}`",spec.metric)))}
        for(count,bytes,iters)in &bench.allocation_samples{
            let numerator=if spec.metric=="AllocationCount"{count}else{bytes};
            let value=Rational::parse(&numerator.to_string(),&iters.to_string()).map_err(ProviderFailure::malformed)?;
            events.push(ProviderEvent::Sample{spec:index as u32,metric:spec.metric.clone(),value});
        }
    }
    let samples=events.len()as u64;events.push(ProviderEvent::Complete{request_id:request.request_id.clone(),samples});Ok(events)
}

fn service_probe_provider(request:&ProviderRequest,_:&ProviderCancellation)->Result<Vec<ProviderEvent>,ProviderFailure>{
    // ServiceProbe evidence is injected via run_dev_refresh → build_report's
    // dev_evidence path; this in-process stub is reached only when no evidence
    // was injected (bare `jet budget check` with a ServiceProbe budget).
    // In that case we surface E2906 (unavailable) for every spec.
    let mut events=Vec::new();
    for(index,spec)in request.specs.iter().enumerate(){
        if spec.metric!="ServiceReadiness"{return Err(ProviderFailure::malformed(format!("ServiceProbe does not support metric `{}`",spec.metric)))}
             events.push(ProviderEvent::Unavailable{spec:index as u32,reason:"ServiceProbe requires `jet dev` to collect readiness evidence; run `jet dev <file>` to refresh".into(),details:Vec::new()});
    }
    let samples=0u64;events.push(ProviderEvent::Complete{request_id:request.request_id.clone(),samples});Ok(events)
}

fn scene_probe_provider(request:&ProviderRequest,_:&ProviderCancellation)->Result<Vec<ProviderEvent>,ProviderFailure>{
    // SceneProbe evidence is injected via run_dev_refresh → build_report's
    // dev_evidence path; this in-process stub is reached only when no evidence
    // was injected (bare `jet budget check` with a SceneProbe budget).
    let mut events=Vec::new();
    for(index,spec)in request.specs.iter().enumerate(){
        if !matches!(spec.metric.split('(').next().unwrap_or(&spec.metric),"FrameTime"|"DrawCalls"|"SceneAssetBytes"|"MemoryHighWater"){return Err(ProviderFailure::malformed(format!("SceneProbe does not support metric `{}`",spec.metric)))}
             events.push(ProviderEvent::Unavailable{spec:index as u32,reason:"SceneProbe requires `jet dev` to collect scene evidence; run `jet dev <file>` to refresh".into(),details:Vec::new()});
    }
    let samples=0u64;events.push(ProviderEvent::Complete{request_id:request.request_id.clone(),samples});Ok(events)
}

fn measurement_base(root:&Path,bundle:&jet::AST::ProgramBundle,spec:&BudgetSpec,_subject:&CanonicalJson,tool:&CanonicalJson,provider:&CanonicalJson,target:&str,profile:&str)->Result<(CanonicalJson,String,String),String>{let metric_name=spec.metric.split('(').next().unwrap_or(&spec.metric);let percentile=metric_percentile(&spec.metric).map(|value|CanonicalJson::String(value.into())).unwrap_or(CanonicalJson::Null);let metric=CanonicalJson::object([("name".into(),CanonicalJson::String(metric_name.into())),("percentile".into(),percentile.clone())])?;let comparison=comparison_json(spec)?;let direction=if metric_name=="Throughput"{"higher_is_better"}else{"lower_is_better"};let applies=CanonicalJson::object([("profiles".into(),CanonicalJson::Array(axis(&spec.applicability.profiles,profile))), ("targets".into(),CanonicalJson::Array(axis(&spec.applicability.targets,target)))])?;let kind=provider_kind(&spec.provider);let identity=provider_identity(&spec.provider);let budget_spec=CanonicalJson::object([("applies".into(),applies),("comparison".into(),comparison.clone()),("enforcement".into(),CanonicalJson::String(spec.enforcement.to_ascii_lowercase())),("metric".into(),metric.clone()),("name".into(),CanonicalJson::String(spec.name.clone())),("package_id".into(),CanonicalJson::String(package_name(root))),("perf_role".into(),CanonicalJson::String(spec.role.clone())),("provider".into(),CanonicalJson::object([("identity".into(),CanonicalJson::String(identity.into())),("kind".into(),CanonicalJson::String(kind.into()))])?),("scope".into(),CanonicalJson::String(spec.scope.clone()))])?;let hash=stable_id(&budget_spec);let fingerprint=text_field(provider,"hardware_fingerprint")?;let mut framed=b"jet-budget-context-v1\0".to_vec();for value in ["package",metric_name,metric_percentile(&spec.metric).unwrap_or(""),target,std::env::consts::ARCH,profile,env!("CARGO_PKG_VERSION"),text_field(tool,"compiler_build_id")?,text_field(tool,"stdlib_id")?,text_field(tool,"runner_id")?,text_field(tool,"digest")?,kind,identity,text_field(provider,"version")?,text_field(provider,"isolation")?,text_field(provider,"cpu_arch")?,text_field(provider,"cpu_model")?,wire_map(as_object(provider)?,"logical_cpus")?,wire_map(as_object(provider)?,"memory_bytes")?,text_field(provider,"os")?,text_field(provider,"kernel")?,text_field(provider,"power_governor")?,fingerprint]{frame(&mut framed,value)}let context=sha256_hex(&framed);let(source,line,_)=source_location(root,bundle,spec);Ok((CanonicalJson::object([("baseline".into(),CanonicalJson::Null),("budget_id".into(),CanonicalJson::String(format!("{}:{}",spec.role,spec.name))),("budget_spec".into(),budget_spec),("budget_spec_sha256".into(),CanonicalJson::String(hash.clone())),("comparison".into(),comparison),("context_key".into(),CanonicalJson::String(context.clone())),("decision".into(),CanonicalJson::Null),("direction".into(),CanonicalJson::String(direction.into())),("enforcement".into(),CanonicalJson::String(spec.enforcement.to_ascii_lowercase())),("history".into(),CanonicalJson::Null),("metric".into(),metric),("policy".into(),if spec.comparison_fact.kind=="Absolute"{CanonicalJson::Null}else{policy_json()}),("provider".into(),provider.clone()),("samples".into(),CanonicalJson::Array(vec![CanonicalJson::Integer("0".into())])),("source".into(),CanonicalJson::String(format!("{source}:{line}"))),("statistics".into(),CanonicalJson::Null),("target_class".into(),CanonicalJson::String(target.into())),("unit".into(),CanonicalJson::String(metric_unit(&spec.metric).into()))])?,hash,context))}

fn measurement_base_truthful(root:&Path,bundle:&jet::AST::ProgramBundle,located:&LocatedBudgetSpec,subject:&CanonicalJson,tool:&CanonicalJson,provider:&CanonicalJson,target:&str,profile:&str)->Result<(CanonicalJson,String,String),String>{
    let spec=&located.spec;
    let (base, hash, _) = measurement_base(root,bundle,spec,subject,tool,provider,target,profile)?;
    let metric_name=spec.metric.split('(').next().unwrap_or(&spec.metric);
    let context=context_key(subject,tool,provider,metric_name,metric_percentile(&spec.metric))?;
    let mut object=as_object(&base)?.clone();
    object.insert("context_key".into(),CanonicalJson::String(context.clone()));
    let(source,line,_)=source_location(root,bundle,located);
    object.insert("source".into(),CanonicalJson::String(format!("{source}:{line}")));
    Ok((CanonicalJson::Object(object),hash,context))
}

fn context_key(subject:&CanonicalJson,tool:&CanonicalJson,provider:&CanonicalJson,metric:&str,percentile:Option<&str>)->Result<String,String>{
    let subject=as_object(subject)?;let tool=as_object(tool)?;let provider=as_object(provider)?;
    let mut input=b"jet-budget-context-v1\0".to_vec();
    for value in [text_map(subject,"target_id")?,metric,percentile.unwrap_or(""),text_map(subject,"target_class")?,text_map(subject,"target_triple")?,text_map(subject,"profile")?]{frame(&mut input,value)}
    for key in ["jet_version","compiler_build_id","stdlib_id","runner_id","digest"]{frame(&mut input,text_map(tool,key)?)}
    for key in ["kind","identity","version","isolation","cpu_arch","cpu_model","logical_cpus","memory_bytes","os","kernel","power_governor","hardware_fingerprint"]{frame(&mut input,wire_map(provider,key)?)}
    Ok(sha256_hex(&input))
}

fn compiler_fact(bundle:&jet::AST::ProgramBundle,effect_facts:&jet::Sema::SemIndexEffectFacts,spec:&BudgetSpec)->Result<u128,String>{match spec.metric.as_str(){"PublicApiItems"=>Ok(bundle.modules.iter().flat_map(|m|&m.items).filter(|item|match item{Item::Func(v)=>v.is_pub,Item::Struct(v)=>v.is_pub,Item::Enum(v)=>v.is_pub,Item::Trait(v)=>v.is_pub,Item::CodeModule(v)=>v.is_pub,_=>false}).count()as u128),"DependencyCount"=>Ok(bundle.dep_roots.len()as u128),"EffectCount"=>{let effects=effect_facts.solved.values().flat_map(|set|set.iter().cloned()).collect::<std::collections::BTreeSet<_>>();Ok(effects.len()as u128)},"GeneratedUnsafe"=>Err("CompilerFacts metric `GeneratedUnsafe` has no exact checked front-end fact; refusing proxy measurement".into()),other=>Err(format!("CompilerFacts metric `{other}` is not implemented"))}}
fn provider_kind(provider:&str)->&str{provider.split_once('(').map(|(kind,_)|kind).unwrap_or(provider)}
fn provider_identity(provider:&str)->&str{provider.split_once('(').and_then(|(_,rest)|rest.strip_suffix(')')).unwrap_or("")}
fn bench_owned(spec:&BudgetSpec)->bool{matches!(provider_kind(&spec.provider),"BenchMeasurement"|"AllocationProbe")&&spec.scope.starts_with("Bench(")}
fn metric_unit(metric:&str)->&'static str{match metric.split('(').next().unwrap_or(metric){"BinarySize"|"ArtifactSize"=>"Bytes","StartupTime"|"FrameTime"|"Latency"|"BenchTime"|"ServiceReadiness"=>"Duration","MemoryHighWater"|"AllocationBytes"|"SceneAssetBytes"=>"Bytes","Throughput"=>"Rate",_=>"Count"}}
fn build_selected_artifact(root:&Path,entry:&Path)->Result<(PathBuf,u64,String),String>{
    let executable=running_executable().map_err(|e|format!("cannot identify compiler for BuildArtifact: {e}"))?;
    let mut command=Command::new(executable);command.arg("build").arg(entry).current_dir(root).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(unix)]{use std::os::unix::process::CommandExt;command.process_group(0);}
    let mut child=command.spawn().map_err(|e|format!("cannot start selected artifact build: {e}"))?;
    // Building prepares provider input; it is not provider measurement. Give
    // a real native build its own bounded deadline while BuildArtifact
    // collection below keeps the tighter production 30-second deadline.
    let deadline=Instant::now()+Duration::from_secs(120);
    loop{match child.try_wait(){Ok(Some(status))if status.success()=>break,Ok(Some(status))=>return Err(format!("selected artifact build exited with {status}")),Ok(None)if Instant::now()<deadline=>std::thread::sleep(Duration::from_millis(5)),Ok(None)=>{terminate_group(&mut child);return Err("selected artifact build exceeded 120 second build deadline".into())},Err(e)=>{terminate_group(&mut child);return Err(format!("cannot supervise selected artifact build: {e}"))}}}
    let stem=entry.file_stem().and_then(|v|v.to_str()).ok_or("selected artifact entry has no UTF-8 stem")?;let artifact=root.join("build").join(if cfg!(windows){format!("{stem}.exe")}else{stem.into()});let metadata=std::fs::symlink_metadata(&artifact).map_err(|e|format!("selected artifact was not produced: {e}"))?;if metadata.file_type().is_symlink()||!metadata.is_file(){return Err("selected artifact is not a regular file".into())}let digest=jet::SHA256::sha256_file_hex(&artifact).map_err(|e|format!("cannot hash selected artifact: {e}"))?;Ok((artifact,metadata.len(),digest))
}
fn quantity(value:&BudgetQuantity)->Result<Rational,String>{match value{BudgetQuantity::Count(v)|BudgetQuantity::Bytes(v)|BudgetQuantity::DurationNs(v)=>Rational::parse(&v.to_string(),"1"),BudgetQuantity::Rate{numerator,denominator_ns}=>Rational::parse(&numerator.to_string(),&denominator_ns.to_string()),BudgetQuantity::PercentBasisPoints(_)=>Err("absolute budget cannot use a relative percent limit".into())}}
fn metric_percentile(metric:&str)->Option<&'static str>{let raw=metric.split_once('(')?.1.strip_suffix(')')?;match raw{"P50"=>Some("p50"),"P90"=>Some("p90"),"P95"=>Some("p95"),"P99"=>Some("p99"),"P999"=>Some("p999"),_=>None}}
fn percentile(metric:&str)->Option<Percentile>{match metric_percentile(metric){Some("p50")=>Some(Percentile::P50),Some("p90")=>Some(Percentile::P90),Some("p95")=>Some(Percentile::P95),Some("p99")=>Some(Percentile::P99),Some("p999")=>Some(Percentile::P999),_=>None}}
fn comparison_json(spec:&BudgetSpec)->Result<CanonicalJson,String>{let direction=if spec.limit_fact.kind=="AtLeast"{"at_least"}else{"at_most"};match spec.comparison_fact.kind.as_str(){"Absolute"=>CanonicalJson::object([("direction".into(),CanonicalJson::String(direction.into())),("kind".into(),CanonicalJson::String("absolute".into())),("limit".into(),quantity(&spec.limit_fact.quantity)?.to_json())]),"AbsoluteFrom"=>CanonicalJson::object([("baseline".into(),CanonicalJson::String(spec.comparison_fact.baseline.clone().ok_or("AbsoluteFrom baseline is absent")?)),("direction".into(),CanonicalJson::String(direction.into())),("kind".into(),CanonicalJson::String("absolute_from".into())),("limit".into(),quantity(&spec.limit_fact.quantity)?.to_json())]),"RelativeTo"=>{let BudgetQuantity::PercentBasisPoints(value)=&spec.limit_fact.quantity else{return Err("RelativeTo requires a basis-point limit".into())};let improvement=spec.limit_fact.kind=="ImprovementAtLeast";CanonicalJson::object([("baseline".into(),CanonicalJson::String(spec.comparison_fact.baseline.clone().ok_or("RelativeTo baseline is absent")?)),("direction".into(),CanonicalJson::String(if spec.metric.starts_with("Throughput"){"higher_is_better".into()}else{"lower_is_better".into()})),("goal".into(),CanonicalJson::String(if improvement{"improvement_at_least".into()}else{"regression_at_most".into()})),("kind".into(),CanonicalJson::String("relative_to".into())),("limit_basis_points".into(),CanonicalJson::Integer(value.to_string()))])},other=>Err(format!("unknown budget comparison `{other}`"))}}
fn comparison_model(spec:&BudgetSpec)->Result<Comparison,String>{let direction=if spec.limit_fact.kind=="AtLeast"{LimitDirection::AtLeast}else{LimitDirection::AtMost};match spec.comparison_fact.kind.as_str(){"Absolute"=>Ok(Comparison::Absolute{limit:quantity(&spec.limit_fact.quantity)?,direction}),"AbsoluteFrom"=>Ok(Comparison::AbsoluteFrom{limit:quantity(&spec.limit_fact.quantity)?,direction}),"RelativeTo"=>{let BudgetQuantity::PercentBasisPoints(value)=&spec.limit_fact.quantity else{return Err("RelativeTo requires a basis-point limit".into())};let value=i128::try_from(*value).map_err(|_|"relative basis-point limit exceeds evaluator range")?;Ok(Comparison::RelativeTo{limit_basis_points:BigInt::from_i128(value),goal:if spec.limit_fact.kind=="ImprovementAtLeast"{RelativeGoal::ImprovementAtLeast}else{RelativeGoal::RegressionAtMost}})},other=>Err(format!("unknown budget comparison `{other}`"))}}
fn policy_json()->CanonicalJson{CanonicalJson::object([("baseline_generations".into(),CanonicalJson::Integer("5".into())),("bootstrap_resamples".into(),CanonicalJson::Integer("10000".into())),("lower_rank".into(),CanonicalJson::Integer("250".into())),("min_baseline_samples".into(),CanonicalJson::Integer("20".into())),("min_candidate_samples".into(),CanonicalJson::Integer("20".into())),("stale_after_seconds".into(),CanonicalJson::Integer("2592000".into())),("trend_generations".into(),CanonicalJson::Integer("5".into())),("upper_rank".into(),CanonicalJson::Integer("9750".into()))]).unwrap()}
fn evaluation_policy()->MeasurementPolicy{MeasurementPolicy{bootstrap_resamples:10000,lower_rank:250,upper_rank:9750}}
fn statistics_json(samples:&[Rational])->Result<CanonicalJson,String>{let mut sorted=samples.to_vec();sorted.sort();let value=statistics(samples)?;CanonicalJson::object([("count".into(),CanonicalJson::Integer(samples.len().to_string())),("mad".into(),value.mad.to_json()),("mean".into(),value.mean.to_json()),("p50".into(),value.p50.to_json()),("p90".into(),value.p90.to_json()),("p95".into(),value.p95.to_json()),("p99".into(),value.p99.to_json()),("p999".into(),value.p999.to_json()),("sorted_samples".into(),CanonicalJson::Array(sorted.iter().map(Rational::to_json).collect()))])}
fn privacy_json()->Result<CanonicalJson,String>{CanonicalJson::object([("excluded".into(),CanonicalJson::Array(Vec::new())),("retained".into(),CanonicalJson::Array(vec![CanonicalJson::String("typed measurements".into()),CanonicalJson::String("workspace source hashes".into())])),("schema".into(),CanonicalJson::Integer("1".into())),("workspace_paths_only".into(),CanonicalJson::Bool(true))])}
fn report_wrapper(evidence_id:&str,measurements:Vec<CanonicalJson>,subject:CanonicalJson,tool:CanonicalJson,privacy:CanonicalJson,pass:usize,warn:usize,fail:usize)->Result<CanonicalJson,String>{let overall=if fail>0{"fail"}else if warn>0{"warn"}else{"pass"};let content=CanonicalJson::object([("evidence_id".into(),CanonicalJson::String(evidence_id.into())),("measurements".into(),CanonicalJson::Array(measurements)),("privacy".into(),privacy),("subject".into(),subject),("summary".into(),CanonicalJson::object([("fail".into(),CanonicalJson::Integer(fail.to_string())),("outcome".into(),CanonicalJson::String(overall.into())),("pass".into(),CanonicalJson::Integer(pass.to_string())),("warn".into(),CanonicalJson::Integer(warn.to_string()))])?),("toolchain".into(),tool)])?;let id=stable_id(&content);CanonicalJson::object([("content".into(),content),("report_id".into(),CanonicalJson::String(id)),("schema".into(),CanonicalJson::String("jet.budget-report".into())),("version".into(),CanonicalJson::Integer("1".into()))])}
fn trend_json(ids:&[String],samples:&[Vec<Rational>],percentile:Option<Percentile>,direction:Direction)->Result<CanonicalJson,String>{let mut pairs=ids.iter().cloned().zip(samples.iter()).map(|(id,samples)|Ok((id,estimator(samples,percentile)?))).collect::<Result<Vec<_>,String>>()?;pairs.reverse();let ids=pairs.iter().map(|pair|pair.0.clone()).collect::<Vec<_>>();let estimators=pairs.iter().map(|pair|pair.1.clone()).collect::<Vec<_>>();let value=evaluate_trend(&ids,&estimators,direction)?;let label=match value.label{TrendLabel::Improving=>"improving",TrendLabel::Stable=>"stable",TrendLabel::Regressing=>"regressing",TrendLabel::Insufficient=>"insufficient"};CanonicalJson::object([("estimators".into(),CanonicalJson::Array(value.estimators.iter().map(Rational::to_json).collect())),("label".into(),CanonicalJson::String(label.into())),("report_ids".into(),CanonicalJson::Array(value.report_ids.into_iter().map(CanonicalJson::String).collect())),("score".into(),value.score.as_ref().map(Rational::to_json).unwrap_or(CanonicalJson::Null))])}
fn applicable(spec:&BudgetSpec,target:&str,profile:&str)->bool{fn one(axis:&BudgetAxis,current:&str)->bool{match axis{BudgetAxis::Current|BudgetAxis::All=>true,BudgetAxis::Only(values)=>values.iter().any(|v|v.eq_ignore_ascii_case(current)||v.contains(&title(current)))}}one(&spec.applicability.targets,target)&&one(&spec.applicability.profiles,profile)}
fn axis(axis:&BudgetAxis,current:&str)->Vec<CanonicalJson>{match axis{BudgetAxis::Current=>vec![CanonicalJson::String(current.into())],BudgetAxis::All=>vec![CanonicalJson::String("all".into())],BudgetAxis::Only(v)=>v.iter().cloned().map(CanonicalJson::String).collect()}}
fn title(s:&str)->String{let mut c=s.chars();c.next().map(|x|x.to_ascii_uppercase().to_string()+c.as_str()).unwrap_or_default()}
fn project_entry(root:&Path)->PathBuf{let main=root.join("src/main.jet");if main.is_file(){return main}if let Some(Ok(manifest))=jet::PackageManifest::PackManifest::load(root){let named=root.join(format!("{}.jet",manifest.package.name));if named.is_file(){return named}}main}
fn package_name(root:&Path)->String{jet::PackageManifest::PackManifest::load(root).and_then(Result::ok).map(|m|m.package.name).unwrap_or_else(||"package".into())}
trait BudgetSource {
    fn spec(&self) -> &BudgetSpec;
    fn module_index(&self, bundle: &jet::AST::ProgramBundle) -> usize;
}
impl BudgetSource for BudgetSpec {
    fn spec(&self) -> &BudgetSpec { self }
    fn module_index(&self, bundle: &jet::AST::ProgramBundle) -> usize { bundle.entry }
}
impl BudgetSource for LocatedBudgetSpec {
    fn spec(&self) -> &BudgetSpec { &self.spec }
    fn module_index(&self, _bundle: &jet::AST::ProgramBundle) -> usize { self.module_index }
}
impl<T: BudgetSource + ?Sized> BudgetSource for &T {
    fn spec(&self) -> &BudgetSpec { (*self).spec() }
    fn module_index(&self, bundle: &jet::AST::ProgramBundle) -> usize { (*self).module_index(bundle) }
}
fn source_location<T:BudgetSource+?Sized>(root:&Path,bundle:&jet::AST::ProgramBundle,located:&T)->(String,usize,usize){let spec=located.spec();let module=&bundle.modules[located.module_index(bundle)];let(line,column)=jet::Diagnostics::span_line_col(&module.source,spec.span.start.min(module.source.len()));let path=module.path.strip_prefix(root).unwrap_or(&module.path).to_string_lossy().replace('\\',"/");(path,line,column)}
fn subject(root:&Path,bundle:&jet::AST::ProgramBundle,start:&str,end:&str,target:&str,profile:&str,artifact:Option<&(PathBuf,u64,String)>)->Result<CanonicalJson,String>{let mut sources=Vec::new();for module in &bundle.modules{let path=module.path.strip_prefix(root).unwrap_or(&module.path).to_string_lossy().replace('\\',"/");sources.push((path,sha256_hex(module.source.as_bytes())))}sources.sort();let artifact=artifact.map(|(_,bytes,sha256)|CanonicalJson::object([("bytes".into(),CanonicalJson::Integer(bytes.to_string())),("sha256".into(),CanonicalJson::String(sha256.clone()))]).unwrap()).unwrap_or(CanonicalJson::Null);CanonicalJson::object([("artifact".into(),artifact),("measured_end".into(),CanonicalJson::String(end.into())),("measured_start".into(),CanonicalJson::String(start.into())),("member_sources".into(),CanonicalJson::Array(sources.into_iter().map(|(path,hash)|CanonicalJson::object([("path".into(),CanonicalJson::String(path)),("sha256".into(),CanonicalJson::String(hash))]).unwrap()).collect())),("profile".into(),CanonicalJson::String(profile.into())),("target_class".into(),CanonicalJson::String(target.into())),("target_id".into(),CanonicalJson::String(package_name(root))),("target_triple".into(),CanonicalJson::String(host_triple()?))])}
fn toolchain()->Result<CanonicalJson,String>{let executable=running_executable().map_err(|e|format!("cannot identify running compiler executable: {e}"))?;let build=executable_digest(&executable)?;let body=CanonicalJson::object([("compiler_build_id".into(),CanonicalJson::String(build.clone())),("jet_version".into(),CanonicalJson::String(env!("CARGO_PKG_VERSION").into())),("runner_id".into(),CanonicalJson::String(build.clone())),("stdlib_id".into(),CanonicalJson::String(build))])?;let mut map=as_object(&body)?.clone();map.insert("digest".into(),CanonicalJson::String(stable_id(&body)));Ok(CanonicalJson::Object(map))}
fn provider(kind:&str,identity:&str)->Result<CanonicalJson,String>{let triple=host_triple()?;let cpu_arch=triple.split('-').next().filter(|v|!v.is_empty()).ok_or("host triple has no architecture")?;let cpuinfo=std::fs::read_to_string("/proc/cpuinfo").map_err(|e|format!("cannot read CPU identity: {e}"))?;let cpu_model=proc_value(&cpuinfo,"model name").or_else(||proc_value(&cpuinfo,"Hardware")).ok_or("CPU model is unavailable")?;let meminfo=std::fs::read_to_string("/proc/meminfo").map_err(|e|format!("cannot read memory identity: {e}"))?;let memory_kib=proc_value(&meminfo,"MemTotal").and_then(|v|v.split_whitespace().next()).ok_or("memory total is unavailable")?.parse::<u128>().map_err(|_|"memory total is malformed")?;let memory_bytes=memory_kib.checked_mul(1024).ok_or("memory total overflow")?;let kernel=read_trimmed("/proc/sys/kernel/osrelease","kernel identity")?;let power_governor=read_trimmed("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor","CPU power governor")?;let logical_cpus=std::thread::available_parallelism().map_err(|e|format!("logical CPU count unavailable: {e}"))?.get();let(version,isolation)=if kind=="AllocationProbe"{("jet-arena-events-v1-warmup-auto-trials-20","benchmark-process-counter-reset-per-trial")}else if kind=="ServiceProbe"{("jet-service-readiness-v1-trials-20","service-process-group-down-up-per-trial")}else if kind=="SceneProbe"{("jet-scene-headless-v1-warmup-5-measure-20","scene-in-process-frame-loop")}else{("1","process")};let body=CanonicalJson::object([("cpu_arch".into(),CanonicalJson::String(cpu_arch.into())),("cpu_model".into(),CanonicalJson::String(cpu_model.into())),("identity".into(),CanonicalJson::String(identity.into())),("isolation".into(),CanonicalJson::String(isolation.into())),("kernel".into(),CanonicalJson::String(kernel)),("kind".into(),CanonicalJson::String(kind.into())),("logical_cpus".into(),CanonicalJson::Integer(logical_cpus.to_string())),("memory_bytes".into(),CanonicalJson::Integer(memory_bytes.to_string())),("os".into(),CanonicalJson::String(std::env::consts::OS.into())),("power_governor".into(),CanonicalJson::String(power_governor)),("version".into(),CanonicalJson::String(version.into()))])?;let mut map=as_object(&body)?.clone();map.insert("hardware_fingerprint".into(),CanonicalJson::String(stable_id(&body)));Ok(CanonicalJson::Object(map))}
fn executable_digest(path:&Path)->Result<String,String>{jet::SHA256::sha256_file_hex(path).map_err(|e|format!("cannot hash running compiler executable: {e}"))}
/// Stable handle to this process's executable bytes. Cargo may atomically
/// replace the pathname returned by `current_exe` while parallel test targets
/// are still running. Linux keeps the executing inode available here even
/// after that pathname is unlinked or points at the next build.
fn running_executable()->std::io::Result<PathBuf>{
    #[cfg(target_os="linux")]
    {
        let current=std::env::current_exe()?;
        // Preserve fail-closed behavior for an executable whose source path
        // exists but cannot be read; only a readable or concurrently unlinked
        // path may use the kernel's stable executing-inode handle.
        match std::fs::File::open(&current){
            Ok(_)=>Ok(PathBuf::from("/proc/self/exe")),
            Err(error) if error.kind()==std::io::ErrorKind::NotFound=>Ok(PathBuf::from("/proc/self/exe")),
            Err(_)=>Ok(current),
        }
    }
    #[cfg(not(target_os="linux"))]
    {std::env::current_exe()}
}
fn host_triple()->Result<String,String>{let target=env!("JET_BUILD_TARGET");if target.split('-').count()<3{return Err("compiler build omitted canonical target triple".into())}Ok(target.into())}
fn proc_value<'a>(text:&'a str,key:&str)->Option<&'a str>{text.lines().find_map(|line|{let(left,right)=line.split_once(':')?;(left.trim()==key).then(||right.trim())})}
fn read_trimmed(path:&str,label:&str)->Result<String,String>{let value=std::fs::read_to_string(path).map_err(|e|format!("cannot read {label}: {e}"))?;let value=value.trim();if value.is_empty(){Err(format!("{label} is empty"))}else{Ok(value.into())}}
fn text_map<'a>(map:&'a std::collections::BTreeMap<String,CanonicalJson>,key:&str)->Result<&'a str,String>{match map.get(key){Some(CanonicalJson::String(value))=>Ok(value),_=>Err(format!("{key} is not text"))}}
fn wire_map<'a>(map:&'a std::collections::BTreeMap<String,CanonicalJson>,key:&str)->Result<&'a str,String>{match map.get(key){Some(CanonicalJson::String(value))=>Ok(value),Some(CanonicalJson::Integer(value))=>Ok(value),_=>Err(format!("{key} is not wire text"))}}
fn as_object(value:&CanonicalJson)->Result<&std::collections::BTreeMap<String,CanonicalJson>,String>{if let CanonicalJson::Object(value)=value{Ok(value)}else{Err("expected canonical object".into())}}
fn text_field<'a>(value:&'a CanonicalJson,key:&str)->Result<&'a str,String>{match as_object(value)?.get(key){Some(CanonicalJson::String(value))=>Ok(value),_=>Err(format!("missing {key}"))}}
fn frame(out:&mut Vec<u8>,value:&str){out.extend_from_slice(&(value.len()as u64).to_be_bytes());out.extend_from_slice(value.as_bytes())}

fn result_rank(row:&ResultRow)->u8{match(row.outcome,row.evidence,row.stale){(PolicyOutcome::Fail,Evidence::Regression|Evidence::Inconclusive,_)=>1,(PolicyOutcome::Fail,Evidence::Unavailable,false)=>2,(PolicyOutcome::Fail,_,true)=>3,(PolicyOutcome::Warn,_,_)=>4,_=>5}}
fn output_counts(built:&BuiltReport)->(usize,usize,usize,usize,usize){let mut counts=(0,0,0,0,0);for row in &built.results{match result_rank(row){1=>counts.0+=1,2=>counts.1+=1,3=>counts.2+=1,4=>counts.3+=1,_=>counts.4+=1}}counts}
fn diagnostic_parts(row:&ResultRow)->(&'static str,String,String){if row.evidence==Evidence::Unavailable{("E2906",format!("performance budget {} has no usable evidence",row.name),"correct the provider evidence or bootstrap only when absent or stale evidence is eligible".into())}else{("E2907",format!("performance budget {} {}",row.name,if row.evidence==Evidence::Inconclusive{"is inconclusive"}else{"regressed"}),"improve the measured behavior, inspect `jet budget check --verbose`, or record an explicit exception".into())}}
fn emit_check(options:&Options,built:&BuiltReport,id:&str,path:&str,created:bool){if options.json{emit_json(options,built,path,None,false,if built.fail>0{1}else{0},None);return}for row in &built.results{if row.outcome==PolicyOutcome::Pass{if options.verbose{emit_verbose_row(row)}continue}let(code,message,fix)=diagnostic_parts(row);let severity=if row.outcome==PolicyOutcome::Warn{"Warning"}else{"Error"};eprintln!("{severity} [{code}]: {message}\n --> {}:{}:{}\n Why: {}\n Fix: {fix}",row.source,row.line,row.column,row.reason);emit_annotation(options,severity,code,row,&message,&fix);if options.verbose{emit_verbose_row(row)}}if options.verbose{eprintln!("{} report {} {}{}",if created{"+"}else{"~"},id,path,if created{""}else{" (verified reuse)"})}let(short,(failed,unavailable,stale,warn,passed))=(&id[..12],output_counts(built));if failed>0{eprintln!("budgets failed: {}{} · report {}",count(failed,"budget failed","budgets failed"),if warn>0{format!(" · {}",count(warn,"warning","warnings"))}else{String::new()},short)}else if unavailable>0{eprintln!("budgets unavailable: {}{} · report {}",count(unavailable,"result unavailable","results unavailable"),if warn>0{format!(" · {}",count(warn,"warning","warnings"))}else{String::new()},short)}else if stale>0{eprintln!("budgets stale: {}{} · report {}",count(stale,"baseline stale","baselines stale"),if warn>0{format!(" · {}",count(warn,"warning","warnings"))}else{String::new()},short)}else if warn>0{eprintln!("budgets: {}{} · report {}",if passed>0{format!("{} passed · ",count(passed,"budget","budgets"))}else{String::new()},count(warn,"warning","warnings"),short)}else{eprintln!("budgets: {} passed · report {}",count(passed,"budget","budgets"),short)}}
fn emit_verbose_row(row:&ResultRow){let ids=if row.baseline_report_ids.is_empty(){"none".into()}else{row.baseline_report_ids.join(",")};let bound=|value:&Option<Rational>|value.as_ref().map(|value|format!("{}/{}",value.num,value.den)).unwrap_or_else(||"none".into());eprintln!("{} {}: point {}/{} · bounds [{}, {}] · baseline [{}] · {} · {}",if row.outcome==PolicyOutcome::Pass{"pass"}else if row.outcome==PolicyOutcome::Warn{"warn"}else{"fail"},row.id,row.sample.num,row.sample.den,bound(&row.lower95),bound(&row.upper95),ids,row.metric_label,row.reason)}
fn emit_annotation(options:&Options,severity:&str,code:&str,row:&ResultRow,message:&str,fix:&str){let enabled=options.annotations==Annotations::Github||(options.annotations==Annotations::Auto&&std::env::var("GITHUB_ACTIONS").ok().as_deref()==Some("true"));if !enabled{return}let level=if severity=="Warning"{"warning"}else{"error"};let property=|v:&str|v.replace('%',"%25").replace('\r',"%0D").replace('\n',"%0A").replace(':',"%3A").replace(',',"%2C");let message=format!("{message}\nWhy: {}\nFix: {fix}",row.reason).replace('%',"%25").replace('\r',"%0D").replace('\n',"%0A");eprintln!("::{level} file={},line={},col={},title={}::{message}",property(&row.source),row.line,row.column,property(&format!("Jet {code}")))}
fn emit_json(options:&Options,built:&BuiltReport,path:&str,plan:Option<(&str,Option<&str>,bool)>,applied:bool,exit:i32,diagnostic:Option<CanonicalJson>){let(failed,unavailable,stale,warn,_)=output_counts(built);let(status,failure_kind)=if failed>0{("fail",Some("budget"))}else if unavailable>0{("unavailable",Some("evidence"))}else if stale>0{("stale",Some("evidence"))}else if warn>0{("warn",None)}else{("pass",None)};let results=built.results.iter().map(result_json).collect();let plan=plan.map(|(baseline,old,created)|CanonicalJson::object([("baseline".into(),CanonicalJson::String(baseline.into())),("requires_confirmation".into(),CanonicalJson::Bool(false)),("rows".into(),CanonicalJson::Array(vec![plan_row(if created{"create"}else{"reuse"},"report",path,None,text_field(&built.value,"report_id").unwrap()),plan_row("advance","baseline",&format!(".jet/perf/baselines/names/{baseline}.json"),old,text_field(&built.value,"report_id").unwrap())]))]).unwrap()).unwrap_or(CanonicalJson::Null);let value=CanonicalJson::object([("applied".into(),CanonicalJson::Bool(applied)),("command".into(),CanonicalJson::String(options.command.into())),("diagnostics".into(),CanonicalJson::Array(diagnostic.into_iter().collect())),("exit_code".into(),CanonicalJson::Integer(exit.to_string())),("failure_kind".into(),failure_kind.map(|value|CanonicalJson::String(value.into())).unwrap_or(CanonicalJson::Null)),("plan".into(),plan),("report".into(),built.value.clone()),("report_path".into(),CanonicalJson::String(path.into())),("results".into(),CanonicalJson::Array(results)),("schema".into(),CanonicalJson::String("jet.budget-command".into())),("status".into(),CanonicalJson::String(status.into())),("version".into(),CanonicalJson::Integer("1".into()))]).unwrap();print!("{}",String::from_utf8(value.bytes()).unwrap())}
fn result_json(row:&ResultRow)->CanonicalJson{let evidence=match row.evidence{Evidence::Pass=>"pass",Evidence::Regression=>"regression",Evidence::Inconclusive=>"inconclusive",Evidence::Unavailable=>"unavailable"};let status=if row.stale&&row.outcome==PolicyOutcome::Fail{"stale"}else if row.evidence==Evidence::Unavailable&&row.outcome==PolicyOutcome::Fail{"unavailable"}else{match row.outcome{PolicyOutcome::Pass=>"pass",PolicyOutcome::Warn=>"warn",PolicyOutcome::Fail=>"fail"}};CanonicalJson::object([("baseline_report_ids".into(),CanonicalJson::Array(row.baseline_report_ids.iter().cloned().map(CanonicalJson::String).collect())),("budget_id".into(),CanonicalJson::String(row.id.clone())),("comparison".into(),row.comparison.clone()),("diagnostic_code".into(),if row.outcome==PolicyOutcome::Pass{CanonicalJson::Null}else{CanonicalJson::String(if row.evidence==Evidence::Unavailable{"E2906".into()}else{"E2907".into()})}),("direction".into(),CanonicalJson::String(row.direction.clone())),("enforcement".into(),CanonicalJson::String(if row.enforcement==Enforcement::Warn{"warn".into()}else{"fail".into()})),("evidence".into(),CanonicalJson::String(evidence.into())),("lower95".into(),row.lower95.as_ref().map(Rational::to_json).unwrap_or(CanonicalJson::Null)),("metric".into(),row.metric.clone()),("point".into(),row.sample.to_json()),("reason".into(),CanonicalJson::String(row.reason.clone())),("source".into(),CanonicalJson::object([("column".into(),CanonicalJson::Integer(row.column.to_string())),("line".into(),CanonicalJson::Integer(row.line.to_string())),("path".into(),CanonicalJson::String(row.source.clone()))]).unwrap()),("stale".into(),CanonicalJson::Bool(row.stale)),("status".into(),CanonicalJson::String(status.into())),("trend".into(),row.trend.clone()),("unit".into(),CanonicalJson::String(row.unit.clone())),("upper95".into(),row.upper95.as_ref().map(Rational::to_json).unwrap_or(CanonicalJson::Null))]).unwrap()}
fn plan_row(operation:&str,artifact:&str,path:&str,from:Option<&str>,id:&str)->CanonicalJson{CanonicalJson::object([("artifact".into(),CanonicalJson::String(artifact.into())),("from_id".into(),from.map(|v|CanonicalJson::String(v.into())).unwrap_or(CanonicalJson::Null)),("id".into(),CanonicalJson::String(id.into())),("operation".into(),CanonicalJson::String(operation.into())),("path".into(),CanonicalJson::String(path.into())),("to_id".into(),CanonicalJson::String(id.into()))]).unwrap()}
fn count(n:usize,one:&str,many:&str)->String{format!("{} {}",n,if n==1{one}else{many})}
fn compiler_failure(options: &Options, entry: &Path, diags: &[jet::Diagnostics::Diagnostic]) -> i32 {
    let src = std::fs::read_to_string(entry).unwrap_or_default();
    if options.json {
        let path = entry
            .parent()
            .and_then(Path::parent)
            .and_then(|root| entry.strip_prefix(root).ok())
            .unwrap_or(entry)
            .to_string_lossy()
            .replace('\\', "/");
        let diagnostics = diags.iter().map(|diagnostic| {
            let source = diagnostic.span.map(|span| {
                let (line, column) = jet::Diagnostics::span_line_col(&src, span.start.min(src.len()));
                let (end_line, end_column) = jet::Diagnostics::span_line_col(&src, span.end.min(src.len()));
                CanonicalJson::object([
                    ("column".into(), CanonicalJson::Integer(column.to_string())),
                    ("end_column".into(), CanonicalJson::Integer(end_column.to_string())),
                    ("end_line".into(), CanonicalJson::Integer(end_line.to_string())),
                    ("line".into(), CanonicalJson::Integer(line.to_string())),
                    ("path".into(), CanonicalJson::String(path.clone())),
                ]).unwrap()
            }).unwrap_or(CanonicalJson::Null);
            let why = diagnostic.detail.as_deref().filter(|detail| !detail.is_empty())
                .map(|detail| format!("{}\n{}", diagnostic.why, detail))
                .unwrap_or_else(|| diagnostic.why.clone());
            CanonicalJson::object([
                ("code".into(), CanonicalJson::String(diagnostic.code.clone())),
                ("fix".into(), CanonicalJson::String(diagnostic.fix.clone())),
                ("message".into(), CanonicalJson::String(diagnostic.what.clone())),
                ("phase".into(), CanonicalJson::String("compiler".into())),
                ("severity".into(), CanonicalJson::String(if diagnostic.severity == jet::Diagnostics::Severity::Lint { "warning".into() } else { "error".into() })),
                ("source".into(), source),
                ("why".into(), CanonicalJson::String(why)),
            ]).unwrap()
        }).collect();
        let value = CanonicalJson::object([
            ("applied".into(), CanonicalJson::Bool(false)),
            ("command".into(), CanonicalJson::String(options.command.into())),
            ("diagnostics".into(), CanonicalJson::Array(diagnostics)),
            ("exit_code".into(), CanonicalJson::Integer("1".into())),
            ("failure_kind".into(), CanonicalJson::String("compiler".into())),
            ("plan".into(), CanonicalJson::Null),
            ("report".into(), CanonicalJson::Null),
            ("report_path".into(), CanonicalJson::Null),
            ("results".into(), CanonicalJson::Array(Vec::new())),
            ("schema".into(), CanonicalJson::String("jet.budget-command".into())),
            ("status".into(), CanonicalJson::String("fail".into())),
            ("version".into(), CanonicalJson::Integer("1".into())),
        ]).unwrap();
        print!("{}", String::from_utf8(value.bytes()).unwrap())
    } else {
        eprint!("{}", jet::Diagnostics::render_all(&entry.to_string_lossy(), &src, diags))
    }
    1
}
fn tool_failure(options:&Options,why:&str)->i32{if options.json{let diagnostic=CanonicalJson::object([("code".into(),CanonicalJson::String("E2908".into())),("fix".into(),CanonicalJson::String("correct the named failure and retry".into())),("message".into(),CanonicalJson::String("performance budget operation failed".into())),("phase".into(),CanonicalJson::String("tool".into())),("severity".into(),CanonicalJson::String("error".into())),("source".into(),CanonicalJson::Null),("why".into(),CanonicalJson::String(why.into()))]).unwrap();let value=CanonicalJson::object([("applied".into(),CanonicalJson::Bool(false)),("command".into(),CanonicalJson::String(options.command.into())),("diagnostics".into(),CanonicalJson::Array(vec![diagnostic])),("exit_code".into(),CanonicalJson::Integer("1".into())),("failure_kind".into(),CanonicalJson::String("tool".into())),("plan".into(),CanonicalJson::Null),("report".into(),CanonicalJson::Null),("report_path".into(),CanonicalJson::Null),("results".into(),CanonicalJson::Array(Vec::new())),("schema".into(),CanonicalJson::String("jet.budget-command".into())),("status".into(),CanonicalJson::String("fail".into())),("version".into(),CanonicalJson::Integer("1".into()))]).unwrap();print!("{}",String::from_utf8(value.bytes()).unwrap())}else{eprintln!("Error [E2908]: performance budget operation failed\n Why: {why}\n Fix: correct the named failure and retry\nbudget command failed before a valid report was produced")}1}
fn tool_failure_with_report(options:&Options,id:&str,why:&str)->i32{if options.json{return tool_failure(options,why)}eprintln!("Error [E2908]: performance budget operation failed\n Why: {why}\n Fix: correct the named failure and retry\nbudget command failed · report {id} was not accepted");1}

fn timestamp_now()->String{let elapsed=SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();let seconds=elapsed.as_secs()as i64;let(days,second)=(seconds.div_euclid(86400),seconds.rem_euclid(86400));let z=days+719468;let era=if z>=0{z}else{z-146096}/146097;let doe=z-era*146097;let yoe=(doe-doe/1460+doe/36524-doe/146096)/365;let mut year=yoe+era*400;let doy=doe-(365*yoe+yoe/4-yoe/100);let mp=(5*doy+2)/153;let day=doy-(153*mp+2)/5+1;let month=mp+if mp<10{3}else{-9};year+=if month<=2{1}else{0};format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:09}Z",second/3600,(second%3600)/60,second%60,elapsed.subsec_nanos())}
