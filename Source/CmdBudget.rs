//! D-PERFBUDGET-OUTPUT1 command projection over the shared evaluator/provider/store.

use jet::AST::Item;
use jet::BudgetProviders::{terminate_group, ProviderEvent, ProviderRegistry, ProviderRequest, ProviderSpec};
use jet::BudgetStore::{BudgetStore, UpdateKind};
use jet::Sema::{BudgetAxis, BudgetQuantity, BudgetSpec, LocatedBudgetSpec};
use jet_foundation::PerformanceBudget::{
    evaluate, stable_id, CanonicalJson, Comparison, Direction, Enforcement, Evidence,
    LimitDirection, PolicyOutcome, Rational,
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
    let built = match build_report(&root, &bundle, &effect_facts, &active, &measured_start) {
        Ok(report) => report,
        Err(error) => return tool_failure(&options, &error),
    };
    let store = BudgetStore::new(&root);
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
    if failed && !options.accept_regression {
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
    if out.bootstrap && out.accept_regression { return Err("`--bootstrap` and `--accept-regression` are mutually exclusive".into()); }
    if (out.bootstrap || out.accept_regression) != out.reason.is_some() { return Err("`--reason` is required with, and only accepted with, bootstrap or accept-regression".into()); }
    if out.json { out.annotations = Annotations::None; }
    Ok(out)
}

struct BuiltReport { value: CanonicalJson, bytes: Vec<u8>, results: Vec<ResultRow>, pass:usize, warn:usize, fail:usize }
struct ResultRow { id:String, name:String, source:String, line:usize, column:usize, metric:String, unit:String, direction:String, comparison:CanonicalJson, sample:Rational, outcome:PolicyOutcome, evidence:Evidence, enforcement:Enforcement, reason:String }

fn build_report(root:&Path, bundle:&jet::AST::ProgramBundle, effect_facts:&jet::Sema::SemIndexEffectFacts, specs:&[LocatedBudgetSpec], at:&str)->Result<BuiltReport,String>{
    for located in specs { let spec=&located.spec; if (!matches!(provider_kind(&spec.provider),"CompilerFacts"|"BuildArtifact")) || spec.comparison_fact.kind != "Absolute" { return Err(format!("provider `{}` for budget `{}` has no command-owned deterministic measurement implementation",spec.provider,spec.name)); } }
    let provider_kinds=specs.iter().map(|located|provider_kind(&located.spec.provider)).collect::<std::collections::BTreeSet<_>>();
    if provider_kinds.len()>1{return Err("one report cannot yet collect CompilerFacts and BuildArtifact together".into())}
    let artifact=if provider_kinds.contains("BuildArtifact"){Some(build_selected_artifact(root,&project_entry(root))?)}else{None};
    let context_subject=subject(root,bundle,at,at,artifact.as_ref())?;let tool=toolchain()?;let first_provider=specs.first().map(|v|v.spec.provider.as_str()).unwrap_or("CompilerFacts");let provider=provider(provider_kind(first_provider),provider_identity(first_provider))?;
    let mut ordered=specs.iter().collect::<Vec<_>>();ordered.sort_by(|a,b|a.spec.name.cmp(&b.spec.name));
    let facts=ordered.iter().map(|located|if provider_kind(&located.spec.provider)=="BuildArtifact"{Ok(artifact.as_ref().expect("built artifact").1 as u128)}else{compiler_fact(bundle,effect_facts,&located.spec)}).collect::<Result<Vec<_>,_>>()?;
    let mut bases=Vec::new();let mut requests=Vec::new();
    for located in &ordered { let spec=&located.spec;let (base,hash,context)=measurement_base_truthful(root,bundle,located,&context_subject,&tool,&provider)?;bases.push((base,hash.clone(),context));requests.push(ProviderSpec{budget_hash:hash,metric:spec.metric.clone()}); }
    let workload=if provider_kind(first_provider)=="BuildArtifact"{CanonicalJson::object([("path".into(),CanonicalJson::String(artifact.as_ref().expect("built artifact").0.to_string_lossy().into_owned()))])?}else{CanonicalJson::Array(facts.iter().map(|v|CanonicalJson::Integer(v.to_string())).collect())};
    let request=ProviderRequest{schema:"jet.provider-request".into(),version:1,request_id:stable_id(&workload),provider_hash:stable_id(&provider),context_hash:stable_id(&context_subject),specs:requests,workload,policy:CanonicalJson::Null};
    let evidence=ProviderRegistry::with_builtins().collect(provider_kind(first_provider),&request,Duration::from_secs(30)).map_err(|e|e.reason)?;
    let samples=evidence.events.iter().filter_map(|event|if let ProviderEvent::Sample{value,..}=event{Some(value.clone())}else{None}).collect::<Vec<_>>();
    if samples.len()!=ordered.len(){return Err("CompilerFacts provider returned incomplete evidence".into())}
    let subject=subject(root,bundle,at,&timestamp_now(),artifact.as_ref())?;
    for (index, sample) in samples.iter().enumerate() {
        let metric_name=ordered[index].spec.metric.split('(').next().unwrap_or(&ordered[index].spec.metric);
        let context=context_key(&subject,&tool,&provider,metric_name,None)?;
        let mut base = as_object(&bases[index].0)?.clone();
        base.insert("samples".into(), CanonicalJson::Array(vec![sample.to_json()]));
        base.insert("context_key".into(), CanonicalJson::String(context.clone()));
        bases[index].0 = CanonicalJson::Object(base);
        bases[index].2 = context;
    }
    let skeletons=bases.iter().map(|(base,_,_)|base.clone()).collect::<Vec<_>>();
    let evidence_id=stable_id(&CanonicalJson::object([("measurements".into(),CanonicalJson::Array(skeletons)),("subject".into(),subject.clone()),("toolchain".into(),tool.clone())])?);
    let mut measurements=Vec::new();let mut results=Vec::new();let(mut pass,mut warn,mut fail)=(0,0,0);
    for (index, spec) in ordered.iter().enumerate() {
        let limit = quantity(&spec.limit_fact.quantity)?;
        let direction = if spec.limit_fact.kind == "AtLeast" { LimitDirection::AtLeast } else { LimitDirection::AtMost };
        let comparison = Comparison::Absolute { limit: limit.clone(), direction };
        let improvement = if direction == LimitDirection::AtMost { Direction::LowerIsBetter } else { Direction::HigherIsBetter };
        let enforcement = if spec.enforcement == "Warn" { Enforcement::Warn } else { Enforcement::Fail };
        let evaluation = evaluate(&evidence_id, &bases[index].2, &[], &[samples[index].clone()], &[], None, &comparison, improvement, enforcement, None)?;
        match evaluation.outcome { PolicyOutcome::Pass => pass += 1, PolicyOutcome::Warn => warn += 1, PolicyOutcome::Fail => fail += 1 }
        let evidence_name = match evaluation.evidence { Evidence::Pass => "pass", Evidence::Regression => "regression", Evidence::Inconclusive => "inconclusive", Evidence::Unavailable => "unavailable" };
        let outcome = match evaluation.outcome { PolicyOutcome::Pass => "pass", PolicyOutcome::Warn => "warn", PolicyOutcome::Fail => "fail" };
        let reason = format!("measured {} against {} {}", samples[index].num, spec.limit_fact.kind, limit.num);
        let decision = CanonicalJson::object([("evidence".into(), CanonicalJson::String(evidence_name.into())), ("lower95".into(), CanonicalJson::Null), ("point".into(), evaluation.point.to_json()), ("policy_outcome".into(), CanonicalJson::String(outcome.into())), ("reason".into(), CanonicalJson::String(reason.clone())), ("trend".into(), trend()), ("upper95".into(), CanonicalJson::Null)])?;
        let mut object = as_object(&bases[index].0)?.clone();
        object.insert("decision".into(), decision);
        measurements.push(CanonicalJson::Object(object));
        let (source, line, column) = source_location(root, bundle, spec);
        let comparison_json = CanonicalJson::object([("direction".into(), CanonicalJson::String(spec.limit_fact.kind.clone())), ("kind".into(), CanonicalJson::String("absolute".into())), ("limit".into(), limit.to_json())])?;
        results.push(ResultRow { id:format!("{}:{}",spec.role,spec.name), name:spec.name.clone(), source, line, column, metric:spec.metric.clone(), unit:metric_unit(&spec.metric).into(), direction:if direction==LimitDirection::AtMost{"lower_is_better".into()}else{"higher_is_better".into()}, comparison:comparison_json, sample:samples[index].clone(), outcome:evaluation.outcome, evidence:evaluation.evidence, enforcement, reason });
    }
    let overall=if fail>0{"fail"}else if warn>0{"warn"}else{"pass"};let privacy=CanonicalJson::object([("excluded".into(),CanonicalJson::Array(Vec::new())),("retained".into(),CanonicalJson::Array(vec![CanonicalJson::String("typed measurements".into()),CanonicalJson::String("workspace source hashes".into())])),("schema".into(),CanonicalJson::Integer("1".into())),("workspace_paths_only".into(),CanonicalJson::Bool(true))])?;let content=CanonicalJson::object([("evidence_id".into(),CanonicalJson::String(evidence_id)),("measurements".into(),CanonicalJson::Array(measurements)),("privacy".into(),privacy),("subject".into(),subject),("summary".into(),CanonicalJson::object([("fail".into(),CanonicalJson::Integer(fail.to_string())),("outcome".into(),CanonicalJson::String(overall.into())),("pass".into(),CanonicalJson::Integer(pass.to_string())),("warn".into(),CanonicalJson::Integer(warn.to_string()))])?),("toolchain".into(),tool)])?;let id=stable_id(&content);let value=CanonicalJson::object([("content".into(),content),("report_id".into(),CanonicalJson::String(id)),("schema".into(),CanonicalJson::String("jet.budget-report".into())),("version".into(),CanonicalJson::Integer("1".into()))])?;let bytes=value.bytes();jet_foundation::PerformanceBudget::verify_budget_report(&bytes)?;Ok(BuiltReport{value,bytes,results,pass,warn,fail})
}

fn measurement_base(root:&Path,bundle:&jet::AST::ProgramBundle,spec:&BudgetSpec,_subject:&CanonicalJson,tool:&CanonicalJson,provider:&CanonicalJson)->Result<(CanonicalJson,String,String),String>{let metric_name=spec.metric.split('(').next().unwrap_or(&spec.metric);let metric=CanonicalJson::object([("name".into(),CanonicalJson::String(metric_name.into())),("percentile".into(),CanonicalJson::Null)])?;let limit=quantity(&spec.limit_fact.quantity)?;let direction=if spec.limit_fact.kind=="AtLeast"{"at_least"}else{"at_most"};let comparison=CanonicalJson::object([("direction".into(),CanonicalJson::String(direction.into())),("kind".into(),CanonicalJson::String("absolute".into())),("limit".into(),limit.to_json())])?;let applies=CanonicalJson::object([("profiles".into(),CanonicalJson::Array(axis(&spec.applicability.profiles,"dev"))), ("targets".into(),CanonicalJson::Array(axis(&spec.applicability.targets,"native")))])?;let kind=provider_kind(&spec.provider);let identity=provider_identity(&spec.provider);let budget_spec=CanonicalJson::object([("applies".into(),applies),("comparison".into(),comparison.clone()),("enforcement".into(),CanonicalJson::String(spec.enforcement.to_ascii_lowercase())),("metric".into(),metric.clone()),("name".into(),CanonicalJson::String(spec.name.clone())),("package_id".into(),CanonicalJson::String(package_name(root))),("perf_role".into(),CanonicalJson::String(spec.role.clone())),("provider".into(),CanonicalJson::object([("identity".into(),CanonicalJson::String(identity.into())),("kind".into(),CanonicalJson::String(kind.into()))])?),("scope".into(),CanonicalJson::String(spec.scope.clone()))])?;let hash=stable_id(&budget_spec);let fingerprint=text_field(provider,"hardware_fingerprint")?;let mut framed=b"jet-budget-context-v1\0".to_vec();for value in ["package",metric_name,"","native",std::env::consts::ARCH,"dev",env!("CARGO_PKG_VERSION"),text_field(tool,"compiler_build_id")?,text_field(tool,"stdlib_id")?,text_field(tool,"runner_id")?,text_field(tool,"digest")?,kind,identity,text_field(provider,"version")?,text_field(provider,"isolation")?,text_field(provider,"cpu_arch")?,text_field(provider,"cpu_model")?,wire_map(as_object(provider)?,"logical_cpus")?,wire_map(as_object(provider)?,"memory_bytes")?,text_field(provider,"os")?,text_field(provider,"kernel")?,text_field(provider,"power_governor")?,fingerprint]{frame(&mut framed,value)}let context=sha256_hex(&framed);let(source,line,_)=source_location(root,bundle,spec);Ok((CanonicalJson::object([("baseline".into(),CanonicalJson::Null),("budget_id".into(),CanonicalJson::String(format!("{}:{}",spec.role,spec.name))),("budget_spec".into(),budget_spec),("budget_spec_sha256".into(),CanonicalJson::String(hash.clone())),("comparison".into(),comparison),("context_key".into(),CanonicalJson::String(context.clone())),("decision".into(),CanonicalJson::Null),("direction".into(),CanonicalJson::String(if direction=="at_most"{"lower_is_better".into()}else{"higher_is_better".into()})),("enforcement".into(),CanonicalJson::String(spec.enforcement.to_ascii_lowercase())),("history".into(),CanonicalJson::Null),("metric".into(),metric),("policy".into(),CanonicalJson::Null),("provider".into(),provider.clone()),("samples".into(),CanonicalJson::Array(vec![CanonicalJson::Integer("0".into())])),("source".into(),CanonicalJson::String(format!("{source}:{line}"))),("statistics".into(),CanonicalJson::Null),("target_class".into(),CanonicalJson::String("native".into())),("unit".into(),CanonicalJson::String(metric_unit(&spec.metric).into()))])?,hash,context))}

fn measurement_base_truthful(root:&Path,bundle:&jet::AST::ProgramBundle,located:&LocatedBudgetSpec,subject:&CanonicalJson,tool:&CanonicalJson,provider:&CanonicalJson)->Result<(CanonicalJson,String,String),String>{
    let spec=&located.spec;
    let (base, hash, _) = measurement_base(root,bundle,spec,subject,tool,provider)?;
    let metric_name=spec.metric.split('(').next().unwrap_or(&spec.metric);
    let context=context_key(subject,tool,provider,metric_name,None)?;
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
fn metric_unit(metric:&str)->&'static str{match metric.split('(').next().unwrap_or(metric){"BinarySize"|"ArtifactSize"=>"Bytes","StartupTime"|"FrameTime"|"Latency"|"BenchTime"|"ServiceReadiness"=>"Duration","MemoryHighWater"|"AllocationBytes"|"SceneAssetBytes"=>"Bytes","Throughput"=>"Rate",_=>"Count"}}
fn build_selected_artifact(root:&Path,entry:&Path)->Result<(PathBuf,u64,String),String>{
    let executable=std::env::current_exe().map_err(|e|format!("cannot identify compiler for BuildArtifact: {e}"))?;
    let mut command=Command::new(executable);command.arg("build").arg(entry).current_dir(root).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(unix)]{use std::os::unix::process::CommandExt;command.process_group(0);}
    let mut child=command.spawn().map_err(|e|format!("cannot start selected artifact build: {e}"))?;let deadline=Instant::now()+Duration::from_secs(30);
    loop{match child.try_wait(){Ok(Some(status))if status.success()=>break,Ok(Some(status))=>return Err(format!("selected artifact build exited with {status}")),Ok(None)if Instant::now()<deadline=>std::thread::sleep(Duration::from_millis(5)),Ok(None)=>{terminate_group(&mut child);return Err("selected artifact build exceeded 30 second provider deadline".into())},Err(e)=>{terminate_group(&mut child);return Err(format!("cannot supervise selected artifact build: {e}"))}}}
    let stem=entry.file_stem().and_then(|v|v.to_str()).ok_or("selected artifact entry has no UTF-8 stem")?;let artifact=root.join("build").join(if cfg!(windows){format!("{stem}.exe")}else{stem.into()});let metadata=std::fs::symlink_metadata(&artifact).map_err(|e|format!("selected artifact was not produced: {e}"))?;if metadata.file_type().is_symlink()||!metadata.is_file(){return Err("selected artifact is not a regular file".into())}let digest=jet::SHA256::sha256_file_hex(&artifact).map_err(|e|format!("cannot hash selected artifact: {e}"))?;Ok((artifact,metadata.len(),digest))
}
fn quantity(value:&BudgetQuantity)->Result<Rational,String>{match value{BudgetQuantity::Count(v)|BudgetQuantity::Bytes(v)|BudgetQuantity::DurationNs(v)=>Rational::parse(&v.to_string(),"1"),BudgetQuantity::Rate{numerator,denominator_ns}=>Rational::parse(&numerator.to_string(),&denominator_ns.to_string()),BudgetQuantity::PercentBasisPoints(_)=>Err("absolute budget cannot use a relative percent limit".into())}}
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
fn subject(root:&Path,bundle:&jet::AST::ProgramBundle,start:&str,end:&str,artifact:Option<&(PathBuf,u64,String)>)->Result<CanonicalJson,String>{let mut sources=Vec::new();for module in &bundle.modules{let path=module.path.strip_prefix(root).unwrap_or(&module.path).to_string_lossy().replace('\\',"/");sources.push((path,sha256_hex(module.source.as_bytes())))}sources.sort();let artifact=artifact.map(|(_,bytes,sha256)|CanonicalJson::object([("bytes".into(),CanonicalJson::Integer(bytes.to_string())),("sha256".into(),CanonicalJson::String(sha256.clone()))]).unwrap()).unwrap_or(CanonicalJson::Null);CanonicalJson::object([("artifact".into(),artifact),("measured_end".into(),CanonicalJson::String(end.into())),("measured_start".into(),CanonicalJson::String(start.into())),("member_sources".into(),CanonicalJson::Array(sources.into_iter().map(|(path,hash)|CanonicalJson::object([("path".into(),CanonicalJson::String(path)),("sha256".into(),CanonicalJson::String(hash))]).unwrap()).collect())),("profile".into(),CanonicalJson::String("dev".into())),("target_class".into(),CanonicalJson::String("native".into())),("target_id".into(),CanonicalJson::String(package_name(root))),("target_triple".into(),CanonicalJson::String(host_triple()?))])}
fn toolchain()->Result<CanonicalJson,String>{let executable=std::env::current_exe().map_err(|e|format!("cannot identify running compiler executable: {e}"))?;let build=executable_digest(&executable)?;let body=CanonicalJson::object([("compiler_build_id".into(),CanonicalJson::String(build.clone())),("jet_version".into(),CanonicalJson::String(env!("CARGO_PKG_VERSION").into())),("runner_id".into(),CanonicalJson::String(build.clone())),("stdlib_id".into(),CanonicalJson::String(build))])?;let mut map=as_object(&body)?.clone();map.insert("digest".into(),CanonicalJson::String(stable_id(&body)));Ok(CanonicalJson::Object(map))}
fn provider(kind:&str,identity:&str)->Result<CanonicalJson,String>{let triple=host_triple()?;let cpu_arch=triple.split('-').next().filter(|v|!v.is_empty()).ok_or("host triple has no architecture")?;let cpuinfo=std::fs::read_to_string("/proc/cpuinfo").map_err(|e|format!("cannot read CPU identity: {e}"))?;let cpu_model=proc_value(&cpuinfo,"model name").or_else(||proc_value(&cpuinfo,"Hardware")).ok_or("CPU model is unavailable")?;let meminfo=std::fs::read_to_string("/proc/meminfo").map_err(|e|format!("cannot read memory identity: {e}"))?;let memory_kib=proc_value(&meminfo,"MemTotal").and_then(|v|v.split_whitespace().next()).ok_or("memory total is unavailable")?.parse::<u128>().map_err(|_|"memory total is malformed")?;let memory_bytes=memory_kib.checked_mul(1024).ok_or("memory total overflow")?;let kernel=read_trimmed("/proc/sys/kernel/osrelease","kernel identity")?;let power_governor=read_trimmed("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor","CPU power governor")?;let logical_cpus=std::thread::available_parallelism().map_err(|e|format!("logical CPU count unavailable: {e}"))?.get();let body=CanonicalJson::object([("cpu_arch".into(),CanonicalJson::String(cpu_arch.into())),("cpu_model".into(),CanonicalJson::String(cpu_model.into())),("identity".into(),CanonicalJson::String(identity.into())),("isolation".into(),CanonicalJson::String("process".into())),("kernel".into(),CanonicalJson::String(kernel)),("kind".into(),CanonicalJson::String(kind.into())),("logical_cpus".into(),CanonicalJson::Integer(logical_cpus.to_string())),("memory_bytes".into(),CanonicalJson::Integer(memory_bytes.to_string())),("os".into(),CanonicalJson::String(std::env::consts::OS.into())),("power_governor".into(),CanonicalJson::String(power_governor)),("version".into(),CanonicalJson::String("1".into()))])?;let mut map=as_object(&body)?.clone();map.insert("hardware_fingerprint".into(),CanonicalJson::String(stable_id(&body)));Ok(CanonicalJson::Object(map))}
fn executable_digest(path:&Path)->Result<String,String>{jet::SHA256::sha256_file_hex(path).map_err(|e|format!("cannot hash running compiler executable: {e}"))}
fn host_triple()->Result<String,String>{let target=env!("JET_BUILD_TARGET");if target.split('-').count()<3{return Err("compiler build omitted canonical target triple".into())}Ok(target.into())}
fn proc_value<'a>(text:&'a str,key:&str)->Option<&'a str>{text.lines().find_map(|line|{let(left,right)=line.split_once(':')?;(left.trim()==key).then(||right.trim())})}
fn read_trimmed(path:&str,label:&str)->Result<String,String>{let value=std::fs::read_to_string(path).map_err(|e|format!("cannot read {label}: {e}"))?;let value=value.trim();if value.is_empty(){Err(format!("{label} is empty"))}else{Ok(value.into())}}
fn text_map<'a>(map:&'a std::collections::BTreeMap<String,CanonicalJson>,key:&str)->Result<&'a str,String>{match map.get(key){Some(CanonicalJson::String(value))=>Ok(value),_=>Err(format!("{key} is not text"))}}
fn wire_map<'a>(map:&'a std::collections::BTreeMap<String,CanonicalJson>,key:&str)->Result<&'a str,String>{match map.get(key){Some(CanonicalJson::String(value))=>Ok(value),Some(CanonicalJson::Integer(value))=>Ok(value),_=>Err(format!("{key} is not wire text"))}}
fn trend()->CanonicalJson{CanonicalJson::object([("estimators".into(),CanonicalJson::Array(Vec::new())),("label".into(),CanonicalJson::String("insufficient".into())),("report_ids".into(),CanonicalJson::Array(Vec::new())),("score".into(),CanonicalJson::Null)]).unwrap()}
fn as_object(value:&CanonicalJson)->Result<&std::collections::BTreeMap<String,CanonicalJson>,String>{if let CanonicalJson::Object(value)=value{Ok(value)}else{Err("expected canonical object".into())}}
fn text_field<'a>(value:&'a CanonicalJson,key:&str)->Result<&'a str,String>{match as_object(value)?.get(key){Some(CanonicalJson::String(value))=>Ok(value),_=>Err(format!("missing {key}"))}}
fn frame(out:&mut Vec<u8>,value:&str){out.extend_from_slice(&(value.len()as u64).to_be_bytes());out.extend_from_slice(value.as_bytes())}

fn emit_check(options:&Options,built:&BuiltReport,id:&str,path:&str,created:bool){if options.json{emit_json(options,built,path,None,false,if built.fail>0{1}else{0},None);return}for row in &built.results{if row.outcome==PolicyOutcome::Pass{if options.verbose{eprintln!("pass {}: {}",row.name,row.reason)}continue}let(code,state)=if row.evidence==Evidence::Unavailable{("E2906","has no usable evidence")}else if row.evidence==Evidence::Inconclusive{("E2907","is inconclusive")}else{("E2907","regressed")};let severity=if row.outcome==PolicyOutcome::Warn{"Warning"}else{"Error"};eprintln!("{severity} [{code}]: performance budget {} {state}\n --> {}:{}:{}\n Why: {}\n Fix: improve the measured behavior, inspect `jet budget check --verbose`, or update the named baseline explicitly",row.name,row.source,row.line,row.column,row.reason);emit_annotation(options,severity,code,row,state)}if options.verbose{eprintln!("{} report {} {}{}",if created{"+"}else{"~"},id,path,if created{""}else{" (verified reuse)"})}let short=&id[..12];if built.fail>0{eprintln!("budgets failed: {} · report {}",count(built.fail,"budget failed","budgets failed"),short)}else if built.warn>0{eprintln!("budgets: {}{}warning{} · report {}",if built.pass>0{format!("{} passed · ",count(built.pass,"budget","budgets"))}else{String::new()},built.warn,if built.warn==1{""}else{"s"},short)}else{eprintln!("budgets: {} passed · report {}",count(built.pass,"budget","budgets"),short)}}
fn emit_annotation(options:&Options,severity:&str,code:&str,row:&ResultRow,state:&str){let enabled=options.annotations==Annotations::Github||(options.annotations==Annotations::Auto&&std::env::var("GITHUB_ACTIONS").ok().as_deref()==Some("true"));if !enabled{return}let level=if severity=="Warning"{"warning"}else{"error"};let property=|v:&str|v.replace('%',"%25").replace('\r',"%0D").replace('\n',"%0A").replace(':',"%3A").replace(',',"%2C");let message=format!("performance budget {} {}\nWhy: {}\nFix: inspect jet budget check --verbose",row.name,state,row.reason).replace('%',"%25").replace('\r',"%0D").replace('\n',"%0A");eprintln!("::{level} file={},line={},col={},title={}::{message}",property(&row.source),row.line,row.column,property(&format!("Jet {code}")))}
fn emit_json(options:&Options,built:&BuiltReport,path:&str,plan:Option<(&str,Option<&str>,bool)>,applied:bool,exit:i32,diagnostic:Option<CanonicalJson>){let status=if built.fail>0{"fail"}else if built.warn>0{"warn"}else{"pass"};let results=built.results.iter().map(result_json).collect();let plan=plan.map(|(baseline,old,created)|CanonicalJson::object([("baseline".into(),CanonicalJson::String(baseline.into())),("requires_confirmation".into(),CanonicalJson::Bool(false)),("rows".into(),CanonicalJson::Array(vec![plan_row(if created{"create"}else{"reuse"},"report",path,None,text_field(&built.value,"report_id").unwrap()),plan_row("advance","baseline",&format!(".jet/perf/baselines/names/{baseline}.json"),old,text_field(&built.value,"report_id").unwrap())]))]).unwrap()).unwrap_or(CanonicalJson::Null);let value=CanonicalJson::object([("applied".into(),CanonicalJson::Bool(applied)),("command".into(),CanonicalJson::String(options.command.into())),("diagnostics".into(),CanonicalJson::Array(diagnostic.into_iter().collect())),("exit_code".into(),CanonicalJson::Integer(exit.to_string())),("failure_kind".into(),if built.fail>0{CanonicalJson::String("budget".into())}else{CanonicalJson::Null}),("plan".into(),plan),("report".into(),built.value.clone()),("report_path".into(),CanonicalJson::String(path.into())),("results".into(),CanonicalJson::Array(results)),("schema".into(),CanonicalJson::String("jet.budget-command".into())),("status".into(),CanonicalJson::String(status.into())),("version".into(),CanonicalJson::Integer("1".into()))]).unwrap();print!("{}",String::from_utf8(value.bytes()).unwrap())}
fn result_json(row:&ResultRow)->CanonicalJson{let evidence=match row.evidence{Evidence::Pass=>"pass",Evidence::Regression=>"regression",Evidence::Inconclusive=>"inconclusive",Evidence::Unavailable=>"unavailable"};let status=match row.outcome{PolicyOutcome::Pass=>"pass",PolicyOutcome::Warn=>"warn",PolicyOutcome::Fail=>"fail"};CanonicalJson::object([("baseline_report_ids".into(),CanonicalJson::Array(Vec::new())),("budget_id".into(),CanonicalJson::String(row.id.clone())),("comparison".into(),row.comparison.clone()),("diagnostic_code".into(),if row.outcome==PolicyOutcome::Pass{CanonicalJson::Null}else{CanonicalJson::String(if row.evidence==Evidence::Unavailable{"E2906".into()}else{"E2907".into()})}),("direction".into(),CanonicalJson::String(row.direction.clone())),("enforcement".into(),CanonicalJson::String(if row.enforcement==Enforcement::Warn{"warn".into()}else{"fail".into()})),("evidence".into(),CanonicalJson::String(evidence.into())),("lower95".into(),CanonicalJson::Null),("metric".into(),CanonicalJson::object([("name".into(),CanonicalJson::String(row.metric.clone())),("percentile".into(),CanonicalJson::Null)]).unwrap()),("point".into(),row.sample.to_json()),("reason".into(),CanonicalJson::String(row.reason.clone())),("source".into(),CanonicalJson::object([("column".into(),CanonicalJson::Integer(row.column.to_string())),("line".into(),CanonicalJson::Integer(row.line.to_string())),("path".into(),CanonicalJson::String(row.source.clone()))]).unwrap()),("stale".into(),CanonicalJson::Bool(false)),("status".into(),CanonicalJson::String(status.into())),("trend".into(),trend()),("unit".into(),CanonicalJson::String(row.unit.clone())),("upper95".into(),CanonicalJson::Null)]).unwrap()}
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
