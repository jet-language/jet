//! `jet learn` — offline code exercises with authored answers and local checks.
//!
//! The four entries in `examples/learn/curriculum.json` are authored lessons.
//! They project canonical task identities; they are not a second census
//! denominator.  The runner records prediction, checked evidence, controlled
//! change, and structurally different transfer in one resumable progress record
//! per lesson.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::OutputAdapter::active_profile;
use crate::{OutputMode, report_problems};
use jet::ExitCodes;
use jet_foundation::DataTree::DataTree;
use jet_foundation::EncodingJson::parse_json;
use jet_foundation::Facts::{DerivationRecord, DerivationRef};
use jet_foundation::JSON::json_escape;
use jet_foundation::Report::StatusEnvelope;
use jet_foundation::Terminal::Theme;

const LEARN_DIR: &str = ".jet/learn/feedback";
const CURRICULUM_JSON: &str = include_str!("../examples/learn/curriculum.json");
const CURRICULUM_SCHEMA: &str = "jet-learning-curriculum-v1";
const CENSUS_RELATION: &str = "jet-learning-census-v1";
const CENSUS_RELATION_SOURCE: &str = "scripts/agent/learning-census.mjs";
const PROGRESS_SCHEMA: &str = "jet-learn-progress-v1";
const REVISION_RULE: &str = "source-bound observations are valid only when source_identity.digest matches; a source edit invalidates the observation until the dependent projection is regenerated and the task is re-run";

#[derive(Clone)]
struct Curriculum {
    relation: String,
    relation_source: String,
    revision_rule: String,
    tasks: Vec<LearningTask>,
    revision: String,
}

#[derive(Clone)]
struct LearningTask {
    id: String,
    capability_id: String,
    census_task_id: String,
    family: String,
    axis: String,
    concept: String,
    prerequisites: Vec<String>,
    prompt: String,
    wrong_model: String,
    source: Fixture,
    input_identity: InputIdentity,
    oracle: Oracle,
    reveal: Reveal,
    controlled_edit: Fixture,
    transfer: Transfer,
    accessibility: Accessibility,
}

#[derive(Clone)]
struct Fixture {
    path: String,
    solution_path: String,
    expected_output: String,
    failure: String,
}

#[derive(Clone)]
struct Transfer {
    fixture: Fixture,
    structural_difference: String,
    copy_guard: String,
}

#[derive(Clone)]
struct InputIdentity {
    path: String,
    role: String,
    revision: String,
}

#[derive(Clone)]
struct Oracle {
    id: String,
    kind: String,
    answer: String,
    expected_output: String,
    distinguishes: String,
}

#[derive(Clone)]
struct Reveal {
    policy: String,
    evidence: String,
    reason: String,
    counterexample: String,
}

#[derive(Clone)]
struct Accessibility {
    keyboard: bool,
    text_only: bool,
    reduced_motion: bool,
    no_auto_quiz: bool,
}

#[derive(Clone)]
struct Progress {
    schema: String,
    curriculum_revision: String,
    task_id: String,
    capability_id: String,
    family: String,
    source_path: String,
    source_fixture_revision: String,
    source_revision: String,
    oracle_id: String,
    oracle_revision: String,
    status: String,
    phase: String,
    prediction_initial: String,
    prediction: String,
    prediction_response: String,
    prediction_measured: bool,
    reveal_shown: bool,
    evidence_status: String,
    evidence_source_revision: String,
    derivation_ref: Option<String>,
    reason_completed: bool,
    source_completed: bool,
    controlled_completed: bool,
    controlled_revision: String,
    transfer_completed: bool,
    transfer_revision: String,
    transfer_evidence: bool,
    stale_reason: Option<String>,
}

impl Progress {
    fn new(
        task: &LearningTask,
        curriculum_revision: &str,
        source_fixture_revision: String,
        source_revision: String,
        oracle_revision: String,
    ) -> Self {
        Self {
            schema: PROGRESS_SCHEMA.to_string(),
            curriculum_revision: curriculum_revision.to_string(),
            task_id: task.id.clone(),
            capability_id: task.capability_id.clone(),
            family: task.family.clone(),
            source_path: task.source.path.clone(),
            source_fixture_revision,
            source_revision,
            oracle_id: task.oracle.id.clone(),
            oracle_revision,
            status: "in_progress".to_string(),
            phase: "prediction".to_string(),
            prediction_initial: "pending".to_string(),
            prediction: "pending".to_string(),
            prediction_response: String::new(),
            prediction_measured: false,
            reveal_shown: false,
            evidence_status: "unknown".to_string(),
            evidence_source_revision: String::new(),
            derivation_ref: None,
            reason_completed: false,
            source_completed: false,
            controlled_completed: false,
            controlled_revision: String::new(),
            transfer_completed: false,
            transfer_revision: String::new(),
            transfer_evidence: false,
            stale_reason: None,
        }
    }

    fn reset_for_stale(&mut self, reason: String, source_revision: String) {
        self.status = "stale".to_string();
        self.phase = "prediction".to_string();
        self.source_revision = source_revision;
        self.prediction_initial = "pending".to_string();
        self.prediction = "pending".to_string();
        self.prediction_response.clear();
        self.prediction_measured = false;
        self.reveal_shown = false;
        self.evidence_status = "stale".to_string();
        self.evidence_source_revision.clear();
        self.derivation_ref = None;
        self.reason_completed = false;
        self.source_completed = false;
        self.controlled_completed = false;
        self.controlled_revision.clear();
        self.transfer_completed = false;
        self.transfer_revision.clear();
        self.transfer_evidence = false;
        self.stale_reason = Some(reason);
    }

    fn update_phase(&mut self) {
        self.phase = if !self.source_completed {
            "source".to_string()
        } else if !self.prediction_measured {
            "prediction".to_string()
        } else if self.evidence_status != "current" {
            "evidence".to_string()
        } else if !self.reason_completed {
            "reason".to_string()
        } else if !self.controlled_completed {
            "controlled_change".to_string()
        } else if !self.transfer_completed || !self.transfer_evidence {
            "transfer".to_string()
        } else {
            "complete".to_string()
        };
        self.status = if self.phase == "complete"
            && self.prediction_measured
            && self.prediction != "unknown"
            && self.prediction != "unmeasured"
            && self.evidence_status == "current"
            && self.stale_reason.is_none()
            && self.transfer_evidence
        {
            "completed".to_string()
        } else if self.status == "cancelled" {
            "cancelled".to_string()
        } else if self.evidence_status == "unavailable" {
            "unavailable".to_string()
        } else if self.stale_reason.is_some() {
            "stale".to_string()
        } else {
            "in_progress".to_string()
        };
    }

    fn complete(&self) -> bool {
        self.status == "completed"
    }
}

#[derive(Clone)]
enum Evaluation {
    Complete {
        stdout: String,
        stderr: String,
        derivation: Option<DerivationRecord>,
    },
    Problems {
        diagnostics: Vec<jet::Diagnostics::Diagnostic>,
    },
    WrongOutput {
        stdout: String,
        stderr: String,
        derivation: Option<DerivationRecord>,
    },
}

impl Evaluation {
    fn complete(&self) -> bool {
        matches!(self, Self::Complete { .. })
    }

    fn derivation(&self) -> Option<&DerivationRecord> {
        match self {
            Self::Complete { derivation, .. } | Self::WrongOutput { derivation, .. } => {
                derivation.as_ref()
            }
            Self::Problems { .. } => None,
        }
    }
}

#[derive(Clone)]
enum Action {
    Predict(String),
    Unknown,
    Skip,
    Reveal,
    Reason,
    Cancel,
}

struct Actions {
    lines: Vec<String>,
    next: usize,
    interactive: bool,
}

impl Actions {
    fn from_stdin() -> Self {
        if io::stdin().is_terminal() {
            return Self {
                lines: Vec::new(),
                next: 0,
                interactive: true,
            };
        }
        let mut input = String::new();
        let _ = io::stdin().read_to_string(&mut input);
        Self {
            lines: input
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect(),
            next: 0,
            interactive: false,
        }
    }

    fn next(&mut self, mode: OutputMode, prompt: &str) -> Option<Action> {
        loop {
            let line = if let Some(line) = self.lines.get(self.next).cloned() {
                self.next += 1;
                line
            } else if self.interactive {
                if mode.json || mode.quiet {
                    return None;
                }
                print_action_prompt(mode, prompt);
                let _ = io::stdout().flush();
                let mut line = String::new();
                if io::stdin().read_line(&mut line).ok()? == 0 {
                    return None;
                }
                line.trim().to_string()
            } else {
                return None;
            };
            if let Some(action) = parse_action(&line) {
                return Some(action);
            }
            if !mode.json && !mode.quiet {
                println!("{}", action_help(mode));
            }
            if !self.interactive && self.next >= self.lines.len() {
                return None;
            }
        }
    }
}

pub(crate) fn run(args: &[String], mode: OutputMode) -> ! {
    let check = args.iter().any(|arg| arg == "--check");
    let once = args.iter().any(|arg| arg == "--watch=off");
    if let Some(arg) = args.iter().find(|arg| {
        !matches!(
            arg.as_str(),
            "--check"
                | "--watch"
                | "--watch=on"
                | "--watch=true"
                | "--watch=off"
                | "--json"
                | "--quiet"
                | "--color"
        ) && !arg.starts_with("--color=")
    }) {
        crate::cli_error!(
            @fix "E2104",
            format!("{arg} is not a jet learn argument"),
            "run jet learn, jet learn --watch=off, or jet learn --check"
        );
        std::process::exit(ExitCodes::USAGE);
    }

    let curriculum = match load_curriculum() {
        Ok(curriculum) => curriculum,
        Err(error) => {
            crate::cli_error!(
                @fix "E2105",
                format!("the learning curriculum is unavailable: {error}"),
                "restore examples/learn/curriculum.json and regenerate the census projection"
            );
            std::process::exit(ExitCodes::USER_ERROR);
        }
    };
    if check {
        exit_check_curriculum(&curriculum, mode);
    }

    let root = match std::env::current_dir() {
        Ok(root) => root.join(LEARN_DIR),
        Err(error) => {
            crate::cli_error!("E2105", "couldn't read the current directory: {error}");
            std::process::exit(ExitCodes::USER_ERROR);
        }
    };
    if let Err(error) = fs::create_dir_all(&root) {
        crate::cli_error!(
            @fix "E2105",
            format!("couldn't create the learn directory {}: {error}", root.display()),
            "run jet learn from a writable directory"
        );
        std::process::exit(ExitCodes::USER_ERROR);
    }

    let mut actions = Actions::from_stdin();
    for (index, task) in curriculum.tasks.iter().enumerate() {
        let status = run_task(&curriculum, task, &root, index, once, mode, &mut actions);
        if status != ExitCodes::OK {
            std::process::exit(status);
        }
    }
    print_complete(&curriculum, mode);
    std::process::exit(ExitCodes::OK);
}

fn load_curriculum() -> Result<Curriculum, String> {
    let value = parse_json(CURRICULUM_JSON, true)
        .map_err(|error| format!("invalid JSON at line {}: {}", error.line, error.message))?;
    let object = value
        .as_object()
        .map_err(|error| format!("curriculum root must be an object: {error}"))?;
    let schema = required_text(object, "schema", "curriculum")?;
    if schema != CURRICULUM_SCHEMA {
        return Err(format!("unsupported curriculum schema `{schema}`"));
    }
    let relation = required_text(object, "relation", "curriculum")?;
    if relation != CENSUS_RELATION {
        return Err(format!(
            "curriculum must project `{CENSUS_RELATION}`, got `{relation}`"
        ));
    }
    let relation_source = required_text(object, "relation_source", "curriculum")?;
    if relation_source != CENSUS_RELATION_SOURCE {
        return Err(format!(
            "curriculum must use `{CENSUS_RELATION_SOURCE}`, got `{relation_source}`"
        ));
    }
    let revision_rule = required_text(object, "revision_rule", "curriculum")?;
    if revision_rule != REVISION_RULE {
        return Err("curriculum revision rule does not match the census contract".to_string());
    }
    let task_values = object_field(object, "tasks")
        .ok_or_else(|| "curriculum is missing `tasks`".to_string())?
        .as_array()
        .map_err(|error| format!("curriculum tasks must be an array: {error}"))?;
    let mut tasks = Vec::with_capacity(task_values.len());
    let mut ids = BTreeSet::new();
    let mut capabilities = BTreeSet::new();
    for (index, value) in task_values.iter().enumerate() {
        let task = parse_task(value, index)?;
        if !ids.insert(task.id.clone()) {
            return Err(format!("duplicate curriculum task `{}`", task.id));
        }
        if !capabilities.insert(task.capability_id.clone()) {
            return Err(format!(
                "duplicate curriculum capability `{}`",
                task.capability_id
            ));
        }
        tasks.push(task);
    }
    if tasks.is_empty() {
        return Err("curriculum has no lessons".to_string());
    }
    Ok(Curriculum {
        relation,
        relation_source,
        revision_rule,
        tasks,
        revision: digest_text(CURRICULUM_JSON),
    })
}

fn parse_task(value: &DataTree, index: usize) -> Result<LearningTask, String> {
    let object = value
        .as_object()
        .map_err(|error| format!("task {index} must be an object: {error}"))?;
    let id = required_text(object, "id", "task")?;
    let capability_id = required_text(object, "capability_id", &id)?;
    let census_task_id = required_text(object, "census_task_id", &id)?;
    if !census_task_id.ends_with(".predict") {
        return Err(format!(
            "{id}.census_task_id must name the canonical predict task"
        ));
    }
    let family = required_text(object, "family", &id)?;
    let axis = required_text(object, "axis", &id)?;
    if axis != "predict" {
        return Err(format!("{id}.axis must be predict"));
    }
    let concept = required_text(object, "concept", &id)?;
    let prerequisites = text_array(object, "prerequisites", &id)?;
    let prompt = required_text(object, "prompt", &id)?;
    let wrong_model = required_text(object, "wrong_model", &id)?;
    let source = parse_fixture(
        object_field(object, "source").ok_or_else(|| format!("{id} is missing `source`"))?,
        &format!("{id}.source"),
    )?;
    let input_identity = parse_input_identity(
        object_field(object, "input_identity")
            .ok_or_else(|| format!("{id} is missing `input_identity`"))?,
        &id,
    )?;
    let oracle = parse_oracle(
        object_field(object, "oracle").ok_or_else(|| format!("{id} is missing `oracle`"))?,
        &id,
    )?;
    let reveal = parse_reveal(
        object_field(object, "reveal").ok_or_else(|| format!("{id} is missing `reveal`"))?,
        &id,
    )?;
    let controlled_edit = parse_fixture(
        object_field(object, "controlled_edit")
            .ok_or_else(|| format!("{id} is missing `controlled_edit`"))?,
        &format!("{id}.controlled_edit"),
    )?;
    let transfer_object = object_field(object, "transfer")
        .ok_or_else(|| format!("{id} is missing `transfer`"))?
        .as_object()
        .map_err(|error| format!("{id}.transfer must be an object: {error}"))?;
    let transfer = Transfer {
        fixture: parse_fixture(
            &DataTree::Object(transfer_object.to_vec()),
            &format!("{id}.transfer"),
        )?,
        structural_difference: required_text(transfer_object, "structural_difference", &id)?,
        copy_guard: required_text(transfer_object, "copy_guard", &id)?,
    };
    let accessibility = parse_accessibility(
        object_field(object, "accessibility")
            .ok_or_else(|| format!("{id} is missing `accessibility`"))?,
        &id,
    )?;
    for fixture in [&source, &controlled_edit, &transfer.fixture] {
        validate_fixture(fixture, &id)?;
    }
    if input_identity.path != source.path {
        return Err(format!("{id}.input_identity.path must match source.path"));
    }
    if oracle.expected_output != source.expected_output {
        return Err(format!(
            "{id}.oracle.expected_output must match source.expected_output"
        ));
    }
    if reveal.policy != "predict_then_reveal" {
        return Err(format!("{id}.reveal.policy must be predict_then_reveal"));
    }
    Ok(LearningTask {
        id,
        capability_id,
        census_task_id,
        family,
        axis,
        concept,
        prerequisites,
        prompt,
        wrong_model,
        source,
        input_identity,
        oracle,
        reveal,
        controlled_edit,
        transfer,
        accessibility,
    })
}

fn parse_fixture(value: &DataTree, label: &str) -> Result<Fixture, String> {
    let object = value
        .as_object()
        .map_err(|error| format!("{label} must be an object: {error}"))?;
    Ok(Fixture {
        path: required_text(object, "path", label)?,
        solution_path: required_text(object, "solution_path", label)?,
        expected_output: required_text(object, "expected_output", label)?,
        failure: required_text(object, "failure", label)?,
    })
}

fn parse_input_identity(value: &DataTree, label: &str) -> Result<InputIdentity, String> {
    let object = value
        .as_object()
        .map_err(|error| format!("{label}.input_identity must be an object: {error}"))?;
    Ok(InputIdentity {
        path: required_text(object, "path", label)?,
        role: required_text(object, "role", label)?,
        revision: required_text(object, "revision", label)?,
    })
}

fn parse_oracle(value: &DataTree, label: &str) -> Result<Oracle, String> {
    let object = value
        .as_object()
        .map_err(|error| format!("{label}.oracle must be an object: {error}"))?;
    Ok(Oracle {
        id: required_text(object, "id", label)?,
        kind: required_text(object, "kind", label)?,
        answer: required_text(object, "answer", label)?,
        expected_output: required_text(object, "expected_output", label)?,
        distinguishes: required_text(object, "distinguishes", label)?,
    })
}

fn parse_reveal(value: &DataTree, label: &str) -> Result<Reveal, String> {
    let object = value
        .as_object()
        .map_err(|error| format!("{label}.reveal must be an object: {error}"))?;
    Ok(Reveal {
        policy: required_text(object, "policy", label)?,
        evidence: required_text(object, "evidence", label)?,
        reason: required_text(object, "reason", label)?,
        counterexample: required_text(object, "counterexample", label)?,
    })
}

fn parse_accessibility(value: &DataTree, label: &str) -> Result<Accessibility, String> {
    let object = value
        .as_object()
        .map_err(|error| format!("{label}.accessibility must be an object: {error}"))?;
    Ok(Accessibility {
        keyboard: required_bool(object, "keyboard", label)?,
        text_only: required_bool(object, "text_only", label)?,
        reduced_motion: required_bool(object, "reduced_motion", label)?,
        no_auto_quiz: required_bool(object, "no_auto_quiz", label)?,
    })
}

fn validate_fixture(fixture: &Fixture, label: &str) -> Result<(), String> {
    for path in [&fixture.path, &fixture.solution_path] {
        if !safe_fixture_path(path) {
            return Err(format!(
                "{label} fixture path `{path}` is not a safe Jet filename"
            ));
        }
        if fixture_text(path).is_none() {
            return Err(format!("{label} fixture `{path}` is not packaged"));
        }
    }
    if !matches!(fixture.failure.as_str(), "diagnostic" | "output") {
        return Err(format!(
            "{label} fixture has unsupported failure `{}`",
            fixture.failure
        ));
    }
    Ok(())
}

fn object_field<'a>(object: &'a [(String, DataTree)], name: &str) -> Option<&'a DataTree> {
    object
        .iter()
        .find_map(|(key, value)| (key == name).then_some(value))
}

fn required_text(object: &[(String, DataTree)], name: &str, label: &str) -> Result<String, String> {
    object_field(object, name)
        .and_then(|value| value.as_str().ok())
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{label} is missing non-empty `{name}`"))
}

fn required_bool(object: &[(String, DataTree)], name: &str, label: &str) -> Result<bool, String> {
    match object_field(object, name) {
        Some(DataTree::Bool(value)) => Ok(*value),
        _ => Err(format!("{label} is missing boolean `{name}`")),
    }
}

fn text_array(
    object: &[(String, DataTree)],
    name: &str,
    label: &str,
) -> Result<Vec<String>, String> {
    let values = object_field(object, name)
        .ok_or_else(|| format!("{label} is missing `{name}`"))?
        .as_array()
        .map_err(|error| format!("{label}.{name} must be an array: {error}"))?;
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .map_err(|error| format!("{label}.{name} contains a non-string: {error}"))
        })
        .collect()
}

fn safe_fixture_path(path: &str) -> bool {
    let candidate = Path::new(path);
    !candidate.is_absolute()
        && candidate
            .extension()
            .is_some_and(|extension| extension == "jet")
        && candidate
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn fixture_text(path: &str) -> Option<&'static str> {
    Some(match path {
        "loop.jet" => include_str!("../examples/learn/feedback/loop.jet"),
        "loop.solution.jet" => include_str!("../examples/learn/feedback/loop.solution.jet"),
        "loop.change.jet" => include_str!("../examples/learn/feedback/loop.change.jet"),
        "loop.change.solution.jet" => {
            include_str!("../examples/learn/feedback/loop.change.solution.jet")
        }
        "loop.transfer.jet" => include_str!("../examples/learn/feedback/loop.transfer.jet"),
        "loop.transfer.solution.jet" => {
            include_str!("../examples/learn/feedback/loop.transfer.solution.jet")
        }
        "state.jet" => include_str!("../examples/learn/feedback/state.jet"),
        "state.solution.jet" => include_str!("../examples/learn/feedback/state.solution.jet"),
        "state.change.jet" => include_str!("../examples/learn/feedback/state.change.jet"),
        "state.change.solution.jet" => {
            include_str!("../examples/learn/feedback/state.change.solution.jet")
        }
        "state.transfer.jet" => include_str!("../examples/learn/feedback/state.transfer.jet"),
        "state.transfer.solution.jet" => {
            include_str!("../examples/learn/feedback/state.transfer.solution.jet")
        }
        "effects.jet" => include_str!("../examples/learn/feedback/effects.jet"),
        "effects.solution.jet" => {
            include_str!("../examples/learn/feedback/effects.solution.jet")
        }
        "effects.change.jet" => include_str!("../examples/learn/feedback/effects.change.jet"),
        "effects.change.solution.jet" => {
            include_str!("../examples/learn/feedback/effects.change.solution.jet")
        }
        "effects.transfer.jet" => {
            include_str!("../examples/learn/feedback/effects.transfer.jet")
        }
        "effects.transfer.solution.jet" => {
            include_str!("../examples/learn/feedback/effects.transfer.solution.jet")
        }
        "foreign.jet" => include_str!("../examples/learn/feedback/foreign.jet"),
        "foreign.solution.jet" => {
            include_str!("../examples/learn/feedback/foreign.solution.jet")
        }
        "foreign.change.jet" => include_str!("../examples/learn/feedback/foreign.change.jet"),
        "foreign.change.solution.jet" => {
            include_str!("../examples/learn/feedback/foreign.change.solution.jet")
        }
        "foreign.transfer.jet" => {
            include_str!("../examples/learn/feedback/foreign.transfer.jet")
        }
        "foreign.transfer.solution.jet" => {
            include_str!("../examples/learn/feedback/foreign.transfer.solution.jet")
        }
        _ => return None,
    })
}

fn run_task(
    curriculum: &Curriculum,
    task: &LearningTask,
    root: &Path,
    index: usize,
    once: bool,
    mode: OutputMode,
    actions: &mut Actions,
) -> i32 {
    let source_path = root.join(&task.source.path);
    if let Err(error) = materialize(&source_path, &task.source.path) {
        return learn_error(
            "E2105",
            format!("couldn't materialize {}: {error}", source_path.display()),
            "run jet learn from a writable directory",
        );
    }
    let source_revision = match digest_file(&source_path) {
        Ok(revision) => revision,
        Err(error) => return learn_error("E2105", error, "restore the learning source file"),
    };
    let source_fixture_revision = digest_text(fixture_text(&task.source.path).unwrap_or_default());
    let oracle_revision = oracle_revision(task);
    let progress_file = progress_path(root, task);
    let mut progress = match load_progress(&progress_file) {
        Ok(Some(progress)) => progress,
        Ok(None) => Progress::new(
            task,
            &curriculum.revision,
            source_fixture_revision.clone(),
            source_revision.clone(),
            oracle_revision.clone(),
        ),
        Err(error) => {
            let mut unavailable = Progress::new(
                task,
                &curriculum.revision,
                source_fixture_revision,
                source_revision,
                oracle_revision,
            );
            unavailable.status = "unavailable".to_string();
            unavailable.evidence_status = "unavailable".to_string();
            unavailable.stale_reason = Some(error.clone());
            let _ = save_progress(&progress_file, &unavailable);
            print_status(task, &unavailable, None, mode);
            return learn_error(
                "E2105",
                format!("learning progress is unavailable: {error}"),
                "remove the malformed progress record after preserving its source edits",
            );
        }
    };
    if progress.task_id != task.id
        || progress.curriculum_revision != curriculum.revision
        || progress.oracle_revision != oracle_revision
        || progress.source_fixture_revision != source_fixture_revision
        || progress.source_revision != source_revision
    {
        let reason = if progress.curriculum_revision != curriculum.revision {
            "lesson content changed".to_string()
        } else if progress.oracle_revision != oracle_revision {
            "lesson answer changed".to_string()
        } else if progress.source_fixture_revision != source_fixture_revision {
            "packaged lesson source changed".to_string()
        } else {
            format!("source changed: {}", task.source.path)
        };
        progress.curriculum_revision = curriculum.revision.clone();
        progress.oracle_revision = oracle_revision;
        progress.source_fixture_revision = source_fixture_revision;
        progress.reset_for_stale(reason, source_revision.clone());
    }
    if progress.status == "cancelled"
        && progress.stale_reason.as_deref() == Some("learner cancelled this checkpoint")
    {
        progress.status = "in_progress".to_string();
        progress.stale_reason = None;
    }
    progress.update_phase();
    let _ = save_progress(&progress_file, &progress);

    print_task_intro(
        task,
        &progress,
        index,
        curriculum.tasks.len(),
        &source_path,
        mode,
    );
    while !progress.prediction_measured {
        let Some(action) = actions.next(mode, "Your prediction (unknown, skip, cancel):") else {
            break;
        };
        match action {
            Action::Predict(answer) => apply_prediction(task, &mut progress, answer, mode),
            Action::Unknown => {
                progress.prediction_initial = "unknown".to_string();
                progress.prediction = "unknown".to_string();
                progress.prediction_response = "I do not know".to_string();
                progress.reveal_shown = true;
                show_reveal(task, mode);
            }
            Action::Skip => {
                progress.prediction_initial = "unmeasured".to_string();
                progress.prediction = "unmeasured".to_string();
                progress.prediction_response = "skip".to_string();
                show_skip(mode);
            }
            Action::Cancel => {
                progress.status = "cancelled".to_string();
                progress.stale_reason = Some("learner cancelled this checkpoint".to_string());
                let _ = save_progress(&progress_file, &progress);
                print_status(task, &progress, None, mode);
                return ExitCodes::USER_ERROR;
            }
            Action::Reveal | Action::Reason => {
                if !mode.json && !mode.quiet {
                    let style = LearnStyle::new(mode);
                    println!(
                        "{}",
                        status_line(
                            style,
                            "hint",
                            "Make a prediction before asking for the explanation."
                        )
                    );
                    println!("{}", action_help(mode));
                }
            }
        }
        let _ = save_progress(&progress_file, &progress);
        if progress.prediction == "unmeasured" {
            break;
        }
    }

    let primary = loop {
        let evaluation = evaluate(&source_path, &task.source.expected_output);
        if evaluation.complete() {
            break evaluation;
        }
        show_evaluation(task, &source_path, &evaluation, mode);
        progress.source_completed = false;
        progress.evidence_status = if matches!(evaluation, Evaluation::Problems { .. }) {
            "current".to_string()
        } else {
            "unknown".to_string()
        };
        progress.update_phase();
        let _ = save_progress(&progress_file, &progress);
        if once {
            print_status(task, &progress, evaluation.derivation(), mode);
            return ExitCodes::USER_ERROR;
        }
        if let Err(status) = watch_until_changed(&source_path, mode) {
            return status;
        }
    };

    progress.source_completed = true;
    progress.source_revision = digest_file(&source_path).unwrap_or_else(|_| String::new());
    progress.stale_reason = None;
    if let Some(derivation) = primary.derivation() {
        let derivation_ref: DerivationRef = derivation.reference();
        progress.evidence_status = "current".to_string();
        progress.evidence_source_revision = progress.source_revision.clone();
        progress.derivation_ref = Some(derivation_ref.id);
    } else {
        progress.evidence_status = "unavailable".to_string();
        progress.evidence_source_revision = progress.source_revision.clone();
        progress.derivation_ref = None;
    }
    show_execution_evidence(task, &primary, mode);
    progress.update_phase();
    let _ = save_progress(&progress_file, &progress);
    if progress.evidence_status != "current" {
        print_status(task, &progress, primary.derivation(), mode);
        return learn_error(
            "E2105",
            format!(
                "checked execution for {} has no current checked evidence",
                task.id
            ),
            "rerun with the toolchain that emits the canonical semindex derivation relation",
        );
    }

    while !progress.prediction_measured {
        let Some(action) = actions.next(mode, "Predict from the checked run (help, skip, cancel):")
        else {
            break;
        };
        match action {
            Action::Predict(answer) => apply_prediction(task, &mut progress, answer, mode),
            Action::Unknown => {
                progress.prediction_initial = "unknown".to_string();
                progress.prediction = "unknown".to_string();
                progress.prediction_response = "I do not know".to_string();
                progress.reveal_shown = true;
                show_reveal(task, mode);
            }
            Action::Skip => {
                progress.prediction_initial = "unmeasured".to_string();
                progress.prediction = "unmeasured".to_string();
                progress.prediction_response = "skip".to_string();
                show_skip(mode);
            }
            Action::Cancel => {
                progress.status = "cancelled".to_string();
                progress.stale_reason = Some("learner cancelled this checkpoint".to_string());
                let _ = save_progress(&progress_file, &progress);
                print_status(task, &progress, primary.derivation(), mode);
                return ExitCodes::USER_ERROR;
            }
            Action::Reveal | Action::Reason => show_reveal(task, mode),
        }
        let _ = save_progress(&progress_file, &progress);
        if progress.prediction == "unmeasured" {
            break;
        }
    }
    if !progress.prediction_measured {
        progress.update_phase();
        let _ = save_progress(&progress_file, &progress);
        print_status(task, &progress, primary.derivation(), mode);
        return ExitCodes::USER_ERROR;
    }

    if !progress.reason_completed {
        let Some(action) = actions.next(mode, "Explain the result (reason, help, cancel):") else {
            progress.update_phase();
            let _ = save_progress(&progress_file, &progress);
            print_status(task, &progress, primary.derivation(), mode);
            return ExitCodes::USER_ERROR;
        };
        match action {
            Action::Reason => {
                progress.reason_completed = true;
                progress.reveal_shown = true;
                show_reveal(task, mode);
            }
            Action::Reveal => {
                progress.reveal_shown = true;
                show_reveal(task, mode);
                progress.update_phase();
                let _ = save_progress(&progress_file, &progress);
                print_status(task, &progress, primary.derivation(), mode);
                return ExitCodes::USER_ERROR;
            }
            Action::Cancel => {
                progress.status = "cancelled".to_string();
                progress.stale_reason = Some("learner cancelled this checkpoint".to_string());
                let _ = save_progress(&progress_file, &progress);
                print_status(task, &progress, primary.derivation(), mode);
                return ExitCodes::USER_ERROR;
            }
            _ => {
                progress.update_phase();
                let _ = save_progress(&progress_file, &progress);
                print_status(task, &progress, primary.derivation(), mode);
                return ExitCodes::USER_ERROR;
            }
        }
    }
    let _ = save_progress(&progress_file, &progress);

    let controlled_path = root.join(&task.controlled_edit.path);
    if let Err(error) = materialize(&controlled_path, &task.controlled_edit.path) {
        return learn_error(
            "E2105",
            format!("couldn't materialize controlled-change lesson: {error}"),
            "run jet learn from a writable directory",
        );
    }
    if progress.controlled_completed {
        if let Ok(revision) = digest_file(&controlled_path) {
            if revision != progress.controlled_revision {
                progress.controlled_completed = false;
                progress.transfer_completed = false;
                progress.transfer_evidence = false;
                progress.status = "stale".to_string();
                progress.stale_reason = Some("controlled edit source changed".to_string());
            }
        }
    }
    let controlled = if progress.controlled_completed {
        None
    } else {
        Some(drive_fixture(
            &controlled_path,
            &task.controlled_edit,
            once,
            mode,
            task,
            "controlled change",
        ))
    };
    if let Some(result) = controlled {
        let _evaluation = match result {
            Ok(evaluation) => evaluation,
            Err(status) => return status,
        };
        progress.controlled_completed = true;
        progress.controlled_revision = digest_file(&controlled_path).unwrap_or_default();
        progress.stale_reason = None;
        if !mode.json && !mode.quiet {
            let style = LearnStyle::new(mode);
            println!(
                "{}",
                status_line(
                    style,
                    "success",
                    "The controlled change now passes the check."
                )
            );
            println!(
                "{}",
                frame(
                    style,
                    "CHANGE / passed",
                    &[
                        format!("Checked file: {}", controlled_path.display()),
                        "The edit changed the requested behavior and still passes the lesson check.".to_string(),
                    ],
                )
            );
        }
    }
    let _ = save_progress(&progress_file, &progress);

    let transfer_path = root.join(&task.transfer.fixture.path);
    if let Err(error) = materialize(&transfer_path, &task.transfer.fixture.path) {
        return learn_error(
            "E2105",
            format!("couldn't materialize transfer lesson: {error}"),
            "run jet learn from a writable directory",
        );
    }
    if progress.transfer_completed {
        if let Ok(revision) = digest_file(&transfer_path) {
            if revision != progress.transfer_revision {
                progress.transfer_completed = false;
                progress.transfer_evidence = false;
                progress.status = "stale".to_string();
                progress.stale_reason = Some("transfer source changed".to_string());
            }
        }
    }
    if !progress.transfer_completed {
        let result = match drive_fixture(
            &transfer_path,
            &task.transfer.fixture,
            once,
            mode,
            task,
            "structurally different transfer",
        ) {
            Ok(evaluation) => evaluation,
            Err(status) => return status,
        };
        progress.transfer_completed = true;
        progress.transfer_revision = digest_file(&transfer_path).unwrap_or_default();
        let primary_solution = fixture_text(&task.source.solution_path).unwrap_or_default();
        let transfer_solution =
            fixture_text(&task.transfer.fixture.solution_path).unwrap_or_default();
        progress.transfer_evidence = transfer_solution != primary_solution
            && transfer_solution != fixture_text(&task.source.path).unwrap_or_default()
            && result.complete();
        progress.stale_reason = None;
        if !mode.json && !mode.quiet {
            let style = LearnStyle::new(mode);
            println!(
                "{}",
                status_line(
                    style,
                    "success",
                    "The transfer check passes with a different source shape."
                )
            );
            println!(
                "{}",
                frame(
                    style,
                    "TRANSFER / passed",
                    &[
                        task.transfer.structural_difference.clone(),
                        format!("Checked file: {}", transfer_path.display()),
                    ],
                )
            );
        }
    }

    progress.update_phase();
    let _ = save_progress(&progress_file, &progress);
    print_status(task, &progress, primary.derivation(), mode);
    if progress.complete() {
        if !mode.json && !mode.quiet {
            let style = LearnStyle::new(mode);
            println!(
                "{}",
                frame(
                    style,
                    "LESSON COMPLETE",
                    &[
                        format!("{} is complete.", task.concept),
                        "You predicted, checked the source, made a controlled change, and solved a different example.".to_string(),
                    ],
                )
            );
        }
        ExitCodes::OK
    } else {
        ExitCodes::USER_ERROR
    }
}

fn drive_fixture(
    path: &Path,
    fixture: &Fixture,
    once: bool,
    mode: OutputMode,
    task: &LearningTask,
    label: &str,
) -> Result<Evaluation, i32> {
    loop {
        let evaluation = evaluate(path, &fixture.expected_output);
        if evaluation.complete() {
            return Ok(evaluation);
        }
        show_evaluation(task, path, &evaluation, mode);
        if once {
            return Err(ExitCodes::USER_ERROR);
        }
        if let Err(status) = watch_until_changed(path, mode) {
            return Err(status);
        }
        if !mode.json && !mode.quiet {
            let style = LearnStyle::new(mode);
            println!(
                "{}",
                status_line(style, "CHECK", &format!("Rechecking {label}."))
            );
        }
    }
}

fn evaluate(path: &Path, expected_output: &str) -> Evaluation {
    let file = path.to_string_lossy();
    let run = jet::Interpreter::dev_iteration_with_gates_profile_and_settings_with_lints(
        &file,
        false,
        false,
        jet::Policy::GateSet::default(),
        "dev",
        &std::collections::BTreeMap::new(),
    );
    let derivation = run.snapshot.as_ref().and_then(|snapshot| {
        let index = jet_semindex::from_checked(&snapshot.bundle, &snapshot.facts);
        index
            .derivations()
            .iter()
            .find(|record| record.disposition.is_current())
            .cloned()
    });
    match run.outcome {
        jet::Interpreter::RunOutcome::Problems(diagnostics) => Evaluation::Problems { diagnostics },
        jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } if exit_code == 0 && stdout == expected_output => Evaluation::Complete {
            stdout,
            stderr,
            derivation,
        },
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, .. } => Evaluation::WrongOutput {
            stdout,
            stderr,
            derivation,
        },
    }
}

#[derive(Clone, Copy)]
struct LearnStyle {
    theme: Theme,
    width: usize,
    unicode: bool,
}

impl LearnStyle {
    fn new(mode: OutputMode) -> Self {
        if let Some(profile) = active_profile() {
            Self {
                theme: Theme::new(profile.ansi_enabled()),
                width: profile.width().clamp(24, 96),
                unicode: profile.unicode_enabled(),
            }
        } else {
            Self {
                theme: Theme::new(mode.color_stderr()),
                width: 80,
                unicode: false,
            }
        }
    }

    fn horizontal(self) -> &'static str {
        if self.unicode { "─" } else { "-" }
    }

    fn vertical(self) -> &'static str {
        if self.unicode { "│" } else { "|" }
    }
}

fn visible_cols(text: &str) -> usize {
    let mut escaped = false;
    text.chars()
        .filter(|&character| {
            if escaped {
                if character == 'm' {
                    escaped = false;
                }
                false
            } else if character == '\x1b' {
                escaped = true;
                false
            } else {
                true
            }
        })
        .count()
}

fn wrap_plain(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    if text.is_empty() {
        return vec![String::new()];
    }
    let chars = text.chars().collect::<Vec<_>>();
    let mut rows = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let mut end = (start + width).min(chars.len());
        if end < chars.len() {
            let mut split = end;
            while split > start && !chars[split - 1].is_whitespace() {
                split -= 1;
            }
            if split > start {
                end = split;
            }
        }
        rows.push(chars[start..end].iter().collect::<String>());
        start = end;
    }
    rows
}

fn pad_plain(text: &str, width: usize) -> String {
    let columns = visible_cols(text);
    if columns >= width {
        text.to_string()
    } else {
        format!("{text}{}", " ".repeat(width - columns))
    }
}

fn frame(style: LearnStyle, title: &str, rows: &[String]) -> String {
    let width = style.width;
    let inner = width.saturating_sub(4).max(1);
    let (top_left, top_right, bottom_left, bottom_right) = if style.unicode {
        ("╭", "╮", "╰", "╯")
    } else {
        ("+", "+", "+", "+")
    };
    let prefix = format!("{top_left}{} ", style.horizontal());
    let fill = width.saturating_sub(visible_cols(&prefix) + visible_cols(title) + 2);
    let top_suffix = format!(" {}{top_right}", style.horizontal().repeat(fill));
    let mut output = String::new();
    output.push_str(&style.theme.border(&prefix));
    output.push_str(&style.theme.accent(title));
    output.push_str(&style.theme.border(&top_suffix));
    output.push('\n');
    for row in rows {
        for wrapped in wrap_plain(&row, inner) {
            output.push_str(&style.theme.border(style.vertical()));
            output.push(' ');
            output.push_str(&pad_plain(&wrapped, inner));
            output.push(' ');
            output.push_str(&style.theme.border(style.vertical()));
            output.push('\n');
        }
    }
    let bottom = format!(
        "{bottom_left}{}{bottom_right}",
        style.horizontal().repeat(width.saturating_sub(2))
    );
    output.push_str(&style.theme.border(&bottom));
    output
}

fn status_line(style: LearnStyle, kind: &str, message: &str) -> String {
    let badge = match kind {
        "success" => style.theme.success("PASS"),
        "error" => style.theme.error("CHECK"),
        "hint" => style.theme.warn("HELP"),
        "stale" => style.theme.warn("STALE"),
        "resume" => style.theme.accent("RESUME"),
        "paused" => style.theme.warn("PAUSED"),
        _ => style.theme.accent(kind),
    };
    format!("{badge} {message}")
}

fn action_help(mode: OutputMode) -> String {
    let style = LearnStyle::new(mode);
    format!(
        "{} {}",
        style.theme.dim("Commands:"),
        style
            .theme
            .dim("predict <answer>  unknown  skip  help  reason  cancel")
    )
}

fn print_action_prompt(mode: OutputMode, prompt: &str) {
    let style = LearnStyle::new(mode);
    print!(
        "{} {}",
        style.theme.accent("learn>"),
        style.theme.bold(prompt)
    );
}

fn step_name(progress: &Progress) -> &'static str {
    if !progress.prediction_measured {
        "PREDICT"
    } else if !progress.source_completed
        || progress.evidence_status != "current"
        || !progress.reason_completed
    {
        "CHECK"
    } else if !progress.controlled_completed {
        "CHANGE"
    } else {
        "TRANSFER"
    }
}

fn step_rail(style: LearnStyle, progress: &Progress) -> String {
    let current = step_name(progress);
    let steps = ["PREDICT", "CHECK", "CHANGE", "TRANSFER"];
    let current_index = steps.iter().position(|step| *step == current).unwrap_or(0);
    let mut rendered = vec![style.theme.dim("Steps")];
    for (index, step) in steps.iter().enumerate() {
        let label = if index == current_index {
            style.theme.invert(&format!("[{step}]"))
        } else if index < current_index {
            style.theme.success(&format!("[{step}]"))
        } else {
            style.theme.dim(&format!("[{step}]"))
        };
        rendered.push(label);
    }
    rendered.join("  ")
}

fn source_rows(path: &Path) -> Vec<String> {
    let source = fs::read_to_string(path).unwrap_or_else(|_| "(source is unavailable)".to_string());
    source
        .lines()
        .enumerate()
        .map(|(index, line)| format!("{:>3} | {}", index + 1, line))
        .collect()
}

fn bounded_output_rows(value: &str, unicode: bool) -> Vec<String> {
    const MAX_LINES: usize = 8;
    const MAX_COLUMNS: usize = 120;
    let mut rows = Vec::new();
    for (index, line) in value.lines().enumerate() {
        if index == MAX_LINES {
            rows.push("... more output omitted".to_string());
            break;
        }
        let mut display = line.chars().take(MAX_COLUMNS).collect::<String>();
        if line.chars().count() > MAX_COLUMNS {
            if unicode {
                display.push('…');
            } else {
                display.push_str("...");
            }
        }
        rows.push(display);
    }
    if rows.is_empty() {
        rows.push("(no output)".to_string());
    }
    rows
}

fn output_comparison(style: LearnStyle, expected: &str, actual: &str, stderr: &str) -> String {
    let mut rows = vec!["Expected output:".to_string()];
    rows.extend(bounded_output_rows(expected, style.unicode));
    rows.push(String::new());
    rows.push("Actual output:".to_string());
    rows.extend(bounded_output_rows(actual, style.unicode));
    if !stderr.is_empty() {
        rows.push(String::new());
        rows.push("Stderr:".to_string());
        rows.extend(bounded_output_rows(stderr, style.unicode));
    }
    frame(style, "output", &rows)
}

fn show_evaluation(task: &LearningTask, path: &Path, evaluation: &Evaluation, mode: OutputMode) {
    match evaluation {
        Evaluation::Problems { diagnostics } => {
            let source = fs::read_to_string(path).unwrap_or_default();
            let display = path.display().to_string();
            report_problems(mode, &display, &source, diagnostics);
            if mode.json || mode.quiet {
                return;
            }
            let style = LearnStyle::new(mode);
            println!(
                "{}",
                status_line(style, "error", "The check reported a problem.")
            );
            println!(
                "{}",
                frame(
                    style,
                    "CHECK / next",
                    &[
                        format!("Lesson source: {}", path.display()),
                        "Read the message above before editing.".to_string(),
                    ],
                )
            );
            println!("{}", frame(style, "source", &source_rows(path)));
            println!(
                "{}",
                style
                    .theme
                    .dim("Next: resolve the reported problem before continuing.")
            );
        }
        Evaluation::WrongOutput { stdout, stderr, .. } => {
            if mode.json || mode.quiet {
                return;
            }
            let style = LearnStyle::new(mode);
            println!(
                "{}",
                status_line(
                    style,
                    "error",
                    &format!("{} runs, but the output is not right yet.", task.family)
                )
            );
            println!(
                "{}",
                output_comparison(style, &task.oracle.expected_output, stdout, stderr)
            );
            println!("{}", frame(style, "source / edit", &source_rows(path)));
            println!(
                "{}",
                style
                    .theme
                    .dim("Next: edit the named target, save, and check the output again.")
            );
        }
        Evaluation::Complete { .. } => {}
    }
}

fn show_execution_evidence(task: &LearningTask, evaluation: &Evaluation, mode: OutputMode) {
    if mode.json || mode.quiet {
        return;
    }
    if let Evaluation::Complete {
        stdout,
        stderr,
        derivation,
    } = evaluation
    {
        let style = LearnStyle::new(mode);
        println!(
            "{}",
            status_line(
                style,
                "success",
                "The source passed the check and printed the expected output."
            )
        );
        println!(
            "{}",
            output_comparison(style, &task.oracle.expected_output, stdout, stderr)
        );
        let evidence = if let Some(derivation) = derivation {
            format!("Checked evidence: {} ({})", derivation.id, derivation.rule)
        } else {
            "Checked evidence is unavailable; this lesson cannot finish yet.".to_string()
        };
        println!(
            "{}",
            frame(
                style,
                "checked run",
                &[task.reveal.evidence.clone(), evidence],
            )
        );
        println!(
            "{}",
            style
                .theme
                .dim("Next: explain why the source behaves this way.")
        );
    }
}

fn apply_prediction(
    task: &LearningTask,
    progress: &mut Progress,
    answer: String,
    mode: OutputMode,
) {
    let correct = prediction_matches(task, &answer);
    if progress.prediction_initial == "pending" {
        progress.prediction_initial = if correct { "correct" } else { "incorrect" }.to_string();
    }
    progress.prediction = if correct { "correct" } else { "incorrect" }.to_string();
    progress.prediction_response = answer.clone();
    progress.prediction_measured = true;
    if !mode.json && !mode.quiet {
        let style = LearnStyle::new(mode);
        let (kind, message) = if correct {
            ("success", "Your answer matches.")
        } else {
            (
                "hint",
                "Prediction recorded. Compare it with the output below.",
            )
        };
        println!("{}", status_line(style, kind, message));
        println!(
            "{}",
            frame(
                style,
                "prediction",
                &[
                    format!("You wrote: {answer}"),
                    "Now check the source.".to_string()
                ],
            )
        );
    }
}

fn prediction_matches(task: &LearningTask, answer: &str) -> bool {
    answer
        .trim()
        .eq_ignore_ascii_case(task.oracle.answer.trim())
}

fn show_skip(mode: OutputMode) {
    if mode.json || mode.quiet {
        return;
    }
    let style = LearnStyle::new(mode);
    println!(
        "{}",
        status_line(
            style,
            "hint",
            "Prediction skipped. It stays unmeasured and cannot complete this lesson.",
        )
    );
}

fn show_reveal(task: &LearningTask, mode: OutputMode) {
    if mode.json || mode.quiet {
        return;
    }
    let style = LearnStyle::new(mode);
    println!(
        "{}",
        frame(
            style,
            "Help",
            &[
                format!("Why: {}", task.reveal.reason),
                format!("Try this: {}", task.reveal.counterexample),
                format!("Common mistake: {}", task.wrong_model),
            ],
        )
    );
}

fn print_task_intro(
    task: &LearningTask,
    progress: &Progress,
    index: usize,
    total: usize,
    path: &Path,
    mode: OutputMode,
) {
    if mode.json || mode.quiet {
        return;
    }
    let style = LearnStyle::new(mode);
    println!(
        "{}",
        frame(
            style,
            &format!("JET LEARN / {}/{}", index + 1, total),
            &[
                task.concept.clone(),
                format!("Before this lesson: {}", task.prerequisites.join(", ")),
            ],
        )
    );
    println!("{}", step_rail(style, progress));
    if progress.status == "stale" {
        if let Some(reason) = &progress.stale_reason {
            println!(
                "{}",
                status_line(
                    style,
                    "stale",
                    &format!("Saved evidence is stale because {reason}.")
                )
            );
        }
    } else if progress.prediction_measured
        || progress.source_completed
        || progress.controlled_completed
        || progress.transfer_completed
    {
        println!(
            "{}",
            status_line(
                style,
                "resume",
                &format!(
                    "Resuming at the {} step.",
                    step_name(progress).to_ascii_lowercase()
                ),
            )
        );
    }
    println!();
    let cwd = std::env::current_dir().unwrap_or_default();
    let display_path = path.strip_prefix(&cwd).unwrap_or(path);
    let mut source = vec![format!("Edit {}", display_path.display()), String::new()];
    source.extend(source_rows(path));
    println!("{}", frame(style, "Source", &source));
    println!();
    println!(
        "{}",
        frame(
            style,
            "Your prediction",
            &[
                task.prompt.clone(),
                String::new(),
                "Type predict <answer>, or unknown for an explanation.".to_string(),
            ],
        )
    );
    println!(
        "{}",
        style
            .theme
            .dim("unknown  Explanation    skip  Leave incomplete    cancel  Save and exit")
    );
}

fn watch_until_changed(path: &Path, mode: OutputMode) -> Result<(), i32> {
    let mut watch = match jet::DevServer::WatchSession::open(path) {
        Ok(session) => session,
        Err(diagnostic) => {
            let display = path.display().to_string();
            eprint!(
                "{}",
                jet::render_all_colored(&display, "", &[diagnostic], mode.color_stderr())
            );
            return Err(ExitCodes::USER_ERROR);
        }
    };
    if !mode.json && !mode.quiet {
        let style = LearnStyle::new(mode);
        println!(
            "{}",
            status_line(
                style,
                "WAIT",
                &format!(
                    "Watching {}. Save the file to run the check again.",
                    path.display()
                ),
            )
        );
        println!("Press Ctrl+C to exit. Run 'jet learn' to resume.");
    }
    loop {
        jet_jit::scheduler_sleep_ms(120);
        let Some(receipt) = watch.poll() else {
            continue;
        };
        if receipt.change_kinds.iter().all(|kind| *kind == "stale") {
            continue;
        }
        if let Err(diagnostic) = watch.acknowledge(&receipt) {
            let display = path.display().to_string();
            eprint!(
                "{}",
                jet::render_all_colored(&display, "", &[diagnostic], mode.color_stderr())
            );
            return Err(ExitCodes::USER_ERROR);
        }
        return Ok(());
    }
}

fn materialize(path: &Path, fixture_path: &str) -> std::io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    let source = fixture_text(fixture_path).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "packaged learning fixture is missing",
        )
    })?;
    fs::write(path, source)
}

fn digest_text(value: &str) -> String {
    jet::SHA256::sha256_hex(value.as_bytes())
}

fn digest_file(path: &Path) -> Result<String, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("couldn't read {}: {error}", path.display()))?;
    Ok(jet::SHA256::sha256_hex(&bytes))
}

fn oracle_revision(task: &LearningTask) -> String {
    digest_text(&format!(
        "{}\0{}\0{}\0{}\0{}",
        task.oracle.id,
        task.oracle.kind,
        task.oracle.answer,
        task.oracle.expected_output,
        task.oracle.distinguishes
    ))
}

fn progress_path(root: &Path, task: &LearningTask) -> PathBuf {
    let mut safe = String::with_capacity(task.id.len());
    for byte in task.id.bytes() {
        if byte.is_ascii_alphanumeric() {
            safe.push(byte as char);
        } else {
            safe.push('_');
        }
    }
    root.join("progress").join(format!("{safe}.json"))
}

fn load_progress(path: &Path) -> Result<Option<Progress>, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("couldn't read {}: {error}", path.display())),
    };
    let value = parse_json(&text, true)
        .map_err(|error| format!("progress JSON at line {}: {}", error.line, error.message))?;
    let object = value
        .as_object()
        .map_err(|error| format!("progress must be an object: {error}"))?;
    let schema = required_text(object, "schema", "progress")?;
    if schema != PROGRESS_SCHEMA {
        return Err(format!("unsupported progress schema `{schema}`"));
    }
    Ok(Some(Progress {
        schema,
        curriculum_revision: required_text(object, "curriculum_revision", "progress")?,
        task_id: required_text(object, "task_id", "progress")?,
        capability_id: required_text(object, "capability_id", "progress")?,
        family: required_text(object, "family", "progress")?,
        source_path: required_text(object, "source_path", "progress")?,
        source_fixture_revision: required_text(object, "source_fixture_revision", "progress")?,
        source_revision: required_text(object, "source_revision", "progress")?,
        oracle_id: required_text(object, "oracle_id", "progress")?,
        oracle_revision: required_text(object, "oracle_revision", "progress")?,
        status: required_text(object, "status", "progress")?,
        phase: required_text(object, "phase", "progress")?,
        prediction_initial: required_text(object, "prediction_initial", "progress")?,
        prediction: required_text(object, "prediction", "progress")?,
        prediction_response: optional_text(object, "prediction_response"),
        prediction_measured: required_bool(object, "prediction_measured", "progress")?,
        reveal_shown: required_bool(object, "reveal_shown", "progress")?,
        evidence_status: required_text(object, "evidence_status", "progress")?,
        evidence_source_revision: optional_text(object, "evidence_source_revision"),
        derivation_ref: optional_nullable_text(object, "derivation_ref"),
        reason_completed: required_bool(object, "reason_completed", "progress")?,
        source_completed: required_bool(object, "source_completed", "progress")?,
        controlled_completed: required_bool(object, "controlled_completed", "progress")?,
        controlled_revision: optional_text(object, "controlled_revision"),
        transfer_completed: required_bool(object, "transfer_completed", "progress")?,
        transfer_revision: optional_text(object, "transfer_revision"),
        transfer_evidence: required_bool(object, "transfer_evidence", "progress")?,
        stale_reason: optional_nullable_text(object, "stale_reason"),
    }))
}

fn optional_text(object: &[(String, DataTree)], name: &str) -> String {
    object_field(object, name)
        .and_then(|value| value.as_str().ok())
        .unwrap_or_default()
        .to_string()
}

fn optional_nullable_text(object: &[(String, DataTree)], name: &str) -> Option<String> {
    match object_field(object, name) {
        Some(DataTree::Null) | None => None,
        Some(value) => value.as_str().ok().map(str::to_string),
    }
}

fn save_progress(path: &Path, progress: &Progress) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("couldn't create {}: {error}", parent.display()))?;
    }
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, progress_json(progress))
        .map_err(|error| format!("couldn't write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, path)
        .map_err(|error| format!("couldn't commit {}: {error}", path.display()))
}

fn progress_json(progress: &Progress) -> String {
    let option = |value: &Option<String>| {
        value
            .as_deref()
            .map(|value| format!("\"{}\"", json_escape(value)))
            .unwrap_or_else(|| "null".to_string())
    };
    format!(
        "{{\"schema\":\"{}\",\"curriculum_revision\":\"{}\",\"task_id\":\"{}\",\"capability_id\":\"{}\",\"family\":\"{}\",\"source_path\":\"{}\",\"source_fixture_revision\":\"{}\",\"source_revision\":\"{}\",\"oracle_id\":\"{}\",\"oracle_revision\":\"{}\",\"status\":\"{}\",\"phase\":\"{}\",\"prediction_initial\":\"{}\",\"prediction\":\"{}\",\"prediction_response\":\"{}\",\"prediction_measured\":{},\"reveal_shown\":{},\"evidence_status\":\"{}\",\"evidence_source_revision\":\"{}\",\"derivation_ref\":{},\"reason_completed\":{},\"source_completed\":{},\"controlled_completed\":{},\"controlled_revision\":\"{}\",\"transfer_completed\":{},\"transfer_revision\":\"{}\",\"transfer_evidence\":{},\"stale_reason\":{}}}\n",
        json_escape(&progress.schema),
        json_escape(&progress.curriculum_revision),
        json_escape(&progress.task_id),
        json_escape(&progress.capability_id),
        json_escape(&progress.family),
        json_escape(&progress.source_path),
        json_escape(&progress.source_fixture_revision),
        json_escape(&progress.source_revision),
        json_escape(&progress.oracle_id),
        json_escape(&progress.oracle_revision),
        json_escape(&progress.status),
        json_escape(&progress.phase),
        json_escape(&progress.prediction_initial),
        json_escape(&progress.prediction),
        json_escape(&progress.prediction_response),
        progress.prediction_measured,
        progress.reveal_shown,
        json_escape(&progress.evidence_status),
        json_escape(&progress.evidence_source_revision),
        option(&progress.derivation_ref),
        progress.reason_completed,
        progress.source_completed,
        progress.controlled_completed,
        json_escape(&progress.controlled_revision),
        progress.transfer_completed,
        json_escape(&progress.transfer_revision),
        progress.transfer_evidence,
        option(&progress.stale_reason),
    )
}

fn print_status(
    task: &LearningTask,
    progress: &Progress,
    derivation: Option<&DerivationRecord>,
    mode: OutputMode,
) {
    if mode.json {
        let derivation_json = derivation
            .map(DerivationRecord::to_json)
            .unwrap_or_else(|| "null".to_string());
        let reference = progress
            .derivation_ref
            .as_deref()
            .map(|value| format!("\"{}\"", json_escape(value)))
            .unwrap_or_else(|| "null".to_string());
        println!(
            "{{\"schema\":\"jet.learn/v1\",\"status\":\"{}\",\"phase\":\"{}\",\"task_id\":\"{}\",\"census_task_id\":\"{}\",\"capability_id\":\"{}\",\"family\":\"{}\",\"axis\":\"{}\",\"concept\":\"{}\",\"curriculum_revision\":\"{}\",\"source_identity\":{{\"path\":\"{}\",\"digest\":\"{}\",\"packaged_digest\":\"{}\"}},\"input_identity\":{{\"path\":\"{}\",\"role\":\"{}\",\"revision\":\"{}\"}},\"oracle\":{{\"id\":\"{}\",\"revision\":\"{}\",\"kind\":\"{}\",\"answer\":\"{}\",\"expected_output\":\"{}\",\"distinguishes\":\"{}\"}},\"prediction\":{{\"initial\":\"{}\",\"result\":\"{}\",\"response\":\"{}\",\"measured\":{}}},\"reveal\":{{\"policy\":\"{}\",\"shown\":{},\"evidence\":\"{}\",\"reason\":\"{}\",\"counterexample\":\"{}\"}},\"evidence\":{{\"status\":\"{}\",\"source_digest\":\"{}\",\"derivation_ref\":{},\"derivation\":{}}},\"completion\":{{\"source\":{},\"controlled_change\":{},\"transfer\":{},\"transfer_evidence\":{}}},\"stale_reason\":{},\"accessibility\":{{\"keyboard\":{},\"text_only\":{},\"reduced_motion\":{},\"no_auto_quiz\":{}}}}}",
            json_escape(&progress.status),
            json_escape(&progress.phase),
            json_escape(&task.id),
            json_escape(&task.census_task_id),
            json_escape(&task.capability_id),
            json_escape(&task.family),
            json_escape(&task.axis),
            json_escape(&task.concept),
            json_escape(&progress.curriculum_revision),
            json_escape(&task.source.path),
            json_escape(&progress.source_revision),
            json_escape(&progress.source_fixture_revision),
            json_escape(&task.input_identity.path),
            json_escape(&task.input_identity.role),
            json_escape(&task.input_identity.revision),
            json_escape(&task.oracle.id),
            json_escape(&progress.oracle_revision),
            json_escape(&task.oracle.kind),
            json_escape(&task.oracle.answer),
            json_escape(&task.oracle.expected_output),
            json_escape(&task.oracle.distinguishes),
            json_escape(&progress.prediction_initial),
            json_escape(&progress.prediction),
            json_escape(&progress.prediction_response),
            progress.prediction_measured,
            json_escape(&task.reveal.policy),
            progress.reveal_shown,
            json_escape(&task.reveal.evidence),
            json_escape(&task.reveal.reason),
            json_escape(&task.reveal.counterexample),
            json_escape(&progress.evidence_status),
            json_escape(&progress.evidence_source_revision),
            reference,
            derivation_json,
            progress.source_completed,
            progress.controlled_completed,
            progress.transfer_completed,
            progress.transfer_evidence,
            progress
                .stale_reason
                .as_deref()
                .map(|value| format!("\"{}\"", json_escape(value)))
                .unwrap_or_else(|| "null".to_string()),
            task.accessibility.keyboard,
            task.accessibility.text_only,
            task.accessibility.reduced_motion,
            task.accessibility.no_auto_quiz,
        );
    } else if !mode.quiet {
        let style = LearnStyle::new(mode);
        if progress.status == "cancelled" {
            println!(
                "{}",
                status_line(
                    style,
                    "paused",
                    "Progress saved. Run `jet learn` to resume."
                )
            );
            println!("{}", step_rail(style, progress));
            return;
        }
        let kind = match progress.status.as_str() {
            "completed" => "success",
            "cancelled" => "paused",
            "stale" => "stale",
            "unavailable" => "error",
            _ => "STATUS",
        };
        println!(
            "{}",
            status_line(
                style,
                kind,
                &format!(
                    "{} / current step: {}",
                    progress.status,
                    step_name(progress).to_ascii_lowercase()
                ),
            )
        );
        let mut rows = vec![
            format!("Lesson: {}", task.concept),
            format!("Source: {}", progress.source_path),
            format!("Prediction: {}", progress.prediction),
            format!("Check: {}", progress.evidence_status),
            format!(
                "Change: {}",
                if progress.controlled_completed {
                    "passed"
                } else {
                    "pending"
                }
            ),
            format!(
                "Transfer: {}",
                if progress.transfer_evidence {
                    "passed"
                } else {
                    "pending"
                }
            ),
        ];
        if let Some(reason) = &progress.stale_reason {
            rows.push(format!("Note: {reason}"));
        }
        println!("{}", frame(style, "progress", &rows));
        println!("{}", step_rail(style, progress));
    }
}

fn parse_action(line: &str) -> Option<Action> {
    let normalized = line.trim().to_ascii_lowercase();
    if normalized == "i do not know" || normalized == "unknown" || normalized == "idk" {
        return Some(Action::Unknown);
    }
    if normalized == "skip" {
        return Some(Action::Skip);
    }
    if normalized == "reveal" || normalized == "help" {
        return Some(Action::Reveal);
    }
    if normalized == "reason" || normalized == "explain" {
        return Some(Action::Reason);
    }
    if normalized == "cancel" || normalized == "quit" {
        return Some(Action::Cancel);
    }
    normalized
        .strip_prefix("predict ")
        .map(|answer| Action::Predict(answer.trim().to_string()))
        .filter(|action| matches!(action, Action::Predict(answer) if !answer.is_empty()))
}

fn learn_error(code: &str, message: String, fix: &str) -> i32 {
    crate::cli_error!(@fix code, message, fix);
    ExitCodes::USER_ERROR
}

fn print_complete(curriculum: &Curriculum, mode: OutputMode) {
    if mode.json {
        println!(
            "{}",
            StatusEnvelope::new("learn", true)
                .with_field("curriculum", "feedback")
                .with_field("relation", curriculum.relation.as_str())
                .with_field("relation_source", curriculum.relation_source.as_str())
                .with_field("witnesses", curriculum.tasks.len())
                .with_field("revision_rule", curriculum.revision_rule.as_str())
                .json()
        );
    } else if !mode.quiet {
        let style = LearnStyle::new(mode);
        println!(
            "{}",
            status_line(
                style,
                "success",
                &format!("All {} lessons are complete.", curriculum.tasks.len()),
            )
        );
        println!(
            "{}",
            frame(
                style,
                "JET LEARN / DONE",
                &[
                    "Each lesson was predicted, checked, changed, and transferred.".to_string(),
                    "Run `jet learn --check` to check the packaged lessons offline.".to_string(),
                ],
            )
        );
    }
}

fn exit_check_curriculum(curriculum: &Curriculum, mode: OutputMode) -> ! {
    let root = match std::env::current_dir() {
        Ok(root) => root.join(LEARN_DIR).join(format!(
            ".check-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        )),
        Err(error) => {
            crate::cli_error!("E2105", "couldn't read the current directory: {error}");
            std::process::exit(ExitCodes::USER_ERROR);
        }
    };
    if let Err(error) = fs::create_dir_all(&root) {
        crate::cli_error!(
            "E2105",
            "couldn't create curriculum check directory: {error}"
        );
        std::process::exit(ExitCodes::USER_ERROR);
    }
    let result = validate_curriculum(curriculum, &root);
    let _ = fs::remove_dir_all(&root);
    match result {
        Ok(()) => {
            if mode.json {
                println!(
                    "{}",
                    StatusEnvelope::new("learn.check", true)
                        .with_field("curriculum", "feedback")
                        .with_field("relation", curriculum.relation.as_str())
                        .with_field("witnesses", curriculum.tasks.len())
                        .json()
                );
            } else if !mode.quiet {
                let style = LearnStyle::new(mode);
                println!(
                    "{}",
                    status_line(
                        style,
                        "success",
                        &format!(
                            "Packaged lessons checked: {} passed.",
                            curriculum.tasks.len()
                        ),
                    )
                );
                println!(
                    "{}",
                    style
                        .theme
                        .dim("No source files were changed by this check.")
                );
            }
            std::process::exit(ExitCodes::OK);
        }
        Err(error) => {
            crate::cli_error!(
                @fix "E2105",
                format!("the packaged lessons are not currently checkable: {error}"),
                "repair the packaged lesson source or provide the required checked runtime, then run jet learn --check again"
            );
            std::process::exit(ExitCodes::USER_ERROR);
        }
    }
}

fn validate_curriculum(curriculum: &Curriculum, root: &Path) -> Result<(), String> {
    for task in &curriculum.tasks {
        let fixtures = [
            (&task.source, "source"),
            (&task.controlled_edit, "controlled"),
            (&task.transfer.fixture, "transfer"),
        ];
        for (fixture, label) in fixtures {
            let broken = root.join(format!("{}-broken-{}", safe_id(&task.id), fixture.path));
            let solution = root.join(format!(
                "{}-solution-{}",
                safe_id(&task.id),
                fixture.solution_path
            ));
            fs::write(&broken, fixture_text(&fixture.path).unwrap_or_default())
                .map_err(|error| format!("{label} {} broken fixture: {error}", task.id))?;
            fs::write(
                &solution,
                fixture_text(&fixture.solution_path).unwrap_or_default(),
            )
            .map_err(|error| format!("{label} {} solution fixture: {error}", task.id))?;
            let broken_result = evaluate(&broken, &fixture.expected_output);
            match fixture.failure.as_str() {
                "diagnostic" if matches!(broken_result, Evaluation::Problems { .. }) => {}
                "output" if matches!(broken_result, Evaluation::WrongOutput { .. }) => {}
                expected => {
                    return Err(format!(
                        "{} {label} expected {expected} failure, got {:?}",
                        task.id,
                        evaluation_kind(&broken_result)
                    ));
                }
            }
            let solved = evaluate(&solution, &fixture.expected_output);
            match solved {
                Evaluation::Complete {
                    derivation: Some(_),
                    ..
                } => {}
                Evaluation::Complete {
                    derivation: None, ..
                } => {
                    return Err(format!(
                        "{} {label} solved without current checked evidence",
                        task.id
                    ));
                }
                other => {
                    return Err(format!(
                        "{} {label} solution failed: {}",
                        task.id,
                        evaluation_kind(&other)
                    ));
                }
            }
        }
    }
    Ok(())
}

fn evaluation_kind(evaluation: &Evaluation) -> &'static str {
    match evaluation {
        Evaluation::Complete { .. } => "complete",
        Evaluation::Problems { .. } => "diagnostic",
        Evaluation::WrongOutput { .. } => "wrong-output",
    }
}

fn safe_id(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() {
                byte as char
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{load_curriculum, prediction_matches};

    #[test]
    fn predictions_require_the_authored_code_answer() {
        let curriculum = load_curriculum().expect("packaged curriculum");
        let task = curriculum
            .tasks
            .iter()
            .find(|task| task.family == "foreign")
            .expect("foreign type lesson");
        assert!(prediction_matches(task, " String "));
        assert!(!prediction_matches(task, "not String"));
        assert!(!prediction_matches(task, "correct"));
    }
}
