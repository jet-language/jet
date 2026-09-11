//! D-SHAPE-CLI1: the checked command-schema projection shared by codegen and
//! inspection. The entry parameter type remains source truth; consumers never
//! reconstruct shell names, requiredness, defaults, or help independently.

use crate::Syntax;
use crate::AST::{
    CtFloat, CtValue, EveryArg, Expr, Field, Func, Item, JobCachePolicy, JobMetadata, JobScope,
    JobSkip, LoadedModule, Marker, Param, ParamZone, Program, ProgramBundle, StrPart, StructDef,
    Type,
};
use crate::Shape::{ShapeFieldNames, ShapeProjectionKind};
use std::collections::BTreeMap;


const RECORD_MAGIC: &[u8; 8] = b"JETCMD\0\0";
/// D-DX-JOBS-UX1 / D-JOB-SUBCMD1: bumps the record so checked job discovery
/// (scope, docs, schedule, graph, and complete execution metadata) is available
/// to tooling.
pub const RECORD_VERSION: u16 = 8;
pub const ELF_SECTION: &str = ".jet_command";
pub const PE_SECTION: &str = ".jetcmd";
pub const MACH_SECTION: &str = "__jetcmd";
pub const WASM_SECTION: &str = "jet.command";
const MAX_RECORD_BYTES: usize = 1024 * 1024;
const MAX_INPUTS: usize = 4096;
const MAX_JOBS: usize = 4096;
const MAX_STRING_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CLIValueKind {
    Bool,
    Int,
    Float,
    String,
    Path,
}

impl CLIValueKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CLIValueKind::Bool => "Bool",
            CLIValueKind::Int => "Int",
            CLIValueKind::Float => "Float",
            CLIValueKind::String => "String",
            CLIValueKind::Path => "Path",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CLIDefault {
    TypeDefault,
    Value(CtValue),
    /// Canonical display value recovered from an executable metadata record.
    Recorded(String),
}

impl CLIDefault {
    pub fn display(&self) -> String {
        match self {
            CLIDefault::TypeDefault => "type default".to_string(),
            CLIDefault::Value(value) => value.jet_show(),
            CLIDefault::Recorded(value) => value.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CLIInputShape {
    Flag,
    Value {
        kind: CLIValueKind,
        optional: bool,
        default: Option<CLIDefault>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CLIInputSchema {
    pub field: String,
    pub flag: String,
    /// D-CLI-FIELD-MARKERS1=A: one-letter alias from `#Short`.
    pub short: Option<String>,
    /// D-CLI-FIELD-MARKERS1=A: fallback variable from `#Env`.
    pub env: Option<String>,
    pub help: String,
    pub metavar: Option<String>,
    pub shape: CLIInputShape,
    /// D-CLI-POS1=A: `Some(order)` when this required value also fills from a
    /// bare argv slot (declaration order among positional fields). `None` for
    /// Bool flags, optional/defaulted values, and `#[Flag]` opt-outs.
    pub positional: Option<u16>,
}

impl CLIInputSchema {
    /// Help passed to the shared `core.args` builder. The builder adds the
    /// environment label; the typed layer adds its decoded default and the
    /// ratified precedence law.
    pub fn builder_help(&self) -> String {
        let mut help = self.help.clone();
        if let CLIInputShape::Value {
            default: Some(default),
            ..
        } = &self.shape
        {
            help.push_str(&format!(" [default: {}]", default.display()));
            if self.env.is_some() {
                help.push_str(" [precedence: flag > env > default]");
            }
        }
        help
    }

    pub fn required(&self) -> bool {
        matches!(
            self.shape,
            CLIInputShape::Value {
                optional: false,
                default: None,
                ..
            }
        )
    }

    pub fn value_kind(&self) -> CLIValueKind {
        match self.shape {
            CLIInputShape::Flag => CLIValueKind::Bool,
            CLIInputShape::Value { kind, .. } => kind,
        }
    }

    pub fn default_display(&self) -> Option<String> {
        match &self.shape {
            CLIInputShape::Value { default, .. } => default.as_ref().map(CLIDefault::display),
            CLIInputShape::Flag => Some("false".to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CLICommandSchema {
    pub entry_type: String,
    /// Optional #Doc text on the entry type. None keeps the old help shape.
    pub description: Option<String>,
    pub inputs: Vec<CLIInputSchema>,
    pub commands: Vec<CLISubcommandSchema>,
    /// Checked top-level `#Job` declarations for the entry namespace.
    pub jobs: Vec<JobFact>,
    /// D-CLI-GLOBAL1=E: `#CLI(Standard)` adds the standard root pack.
    pub standard: bool,
    /// The package version used by the standard `--version` flag.
    /// `None` is intentional for an unversioned schema projection.
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CLISubcommandSchema {
    pub name: String,
    /// Optional #Doc text on the callable member.
    pub description: Option<String>,
    pub inputs: Vec<CLIInputSchema>,
}

/// One argument in a checked `#Job` declaration.
///
/// Unlike `CLIInputSchema`, this keeps the full source type and call zone:
/// jobs may use ordinary typed parameters rather than a `#[CLI]` struct.
#[derive(Debug, Clone, PartialEq)]
pub struct JobArgumentSchema {
    pub name: String,
    pub label: String,
    pub ty: String,
    pub required: bool,
    pub default: Option<String>,
    pub variadic: bool,
    pub zone: ParamZone,
}

impl JobArgumentSchema {
    pub fn display(&self) -> String {
        let variadic = if self.variadic { "..." } else { "" };
        format!("{}: {variadic}{}", self.label, self.ty)
    }
}

/// One checked top-level `#Job` declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct JobFact {
    pub name: String,
    pub scope: JobScope,
    pub doc: Option<String>,
    pub arguments: Vec<JobArgumentSchema>,
    /// D-DX-JOBGRAPH1=A: checked predecessor names in declaration order.
    pub after: Vec<String>,
    /// Package capabilities granted to this job.
    pub packages: Vec<String>,
    /// Project-relative working directory for this job.
    pub cwd: Option<String>,
    /// Source-relative paths used to explain freshness decisions.
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    /// Explicit host skip rule, when one was declared.
    pub skip: Option<JobSkip>,
    /// Durable cache policy for the job.
    pub cache: JobCachePolicy,
    /// Effective graph admission bound. One is the checked default.
    pub parallel: usize,
    /// Per-job runtime limits.
    pub limits: BTreeMap<String, String>,
    /// Source spelling is a presentation projection. Sema still keeps the
    /// resolved `EverySchedule` on `Func::every` for runtime consumers.
    pub schedule: Option<String>,
}

impl JobFact {
    pub fn from_function(function: &Func) -> Self {
        let metadata = function.job_metadata.as_ref();
        let scope = metadata.map(|metadata| metadata.scope).unwrap_or_default();
        Self {
            name: function.name.clone(),
            scope,
            doc: marker(&function.markers, Syntax::MARKER_DOC).and_then(marker_string),
            arguments: function
                .params
                .iter()
                .filter(|param| param.name != Syntax::KW_SELF)
                .map(JobArgumentSchema::from_param)
                .collect(),
            after: metadata
                .map(|metadata| metadata.after.clone())
                .unwrap_or_default(),
            packages: metadata
                .map(|metadata| metadata.packages.clone())
                .unwrap_or_default(),
            cwd: metadata.and_then(|metadata| metadata.cwd.clone()),
            inputs: metadata
                .map(|metadata| metadata.inputs.clone())
                .unwrap_or_default(),
            outputs: metadata
                .map(|metadata| metadata.outputs.clone())
                .unwrap_or_default(),
            skip: metadata.and_then(|metadata| metadata.skip.clone()),
            cache: metadata.map(|metadata| metadata.cache).unwrap_or_default(),
            parallel: metadata
                .and_then(|metadata| metadata.parallel)
                .unwrap_or(1),
            limits: metadata
                .map(|metadata| metadata.limits.clone())
                .unwrap_or_default(),
            schedule: function.every.as_ref().and_then(schedule_text),
        }
    }

    /// Rehydrate the complete checked job policy for a host that only has the
    /// CLI projection (for example an embedded executable record).
    pub fn metadata(&self) -> JobMetadata {
        JobMetadata {
            scope: self.scope,
            after: self.after.clone(),
            packages: self.packages.clone(),
            cwd: self.cwd.clone(),
            inputs: self.inputs.clone(),
            outputs: self.outputs.clone(),
            skip: self.skip.clone(),
            cache: self.cache,
            parallel: Some(self.parallel),
            limits: self.limits.clone(),
        }
    }

    pub fn scope_name(&self) -> &'static str {
        match self.scope {
            JobScope::Dev => "dev",
            JobScope::Ship => "ship",
            JobScope::Internal => "internal",
        }
    }
}

impl JobArgumentSchema {
    fn from_param(param: &Param) -> Self {
        let optional = matches!(&param.ty, Type::Option(_));
        Self {
            name: param.name.clone(),
            label: param.call_label().to_string(),
            ty: param.ty.name(),
            required: !param.variadic
                && !optional
                && !matches!(&param.ty, Type::Bool)
                && param.default.is_none(),
            default: param
                .default
                .as_deref()
                .and_then(expr_default)
                .map(|value| value.display()),
            variadic: param.variadic,
            zone: param.zone,
        }
    }
}

/// The one deterministic checked job registry consumed by CLI, completion,
/// dossier, and devtools projections.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JobRegistry {
    jobs: Vec<JobFact>,
}

impl JobRegistry {
    pub fn from_items(items: &[Item]) -> Self {
        Self {
            jobs: items
                .iter()
                .filter_map(|item| match item {
                    Item::Func(function) if function.is_job => {
                        Some(JobFact::from_function(function))
                    }
                    _ => None,
                })
                .collect(),
        }
    }

    pub fn from_program(program: &Program) -> Self {
        Self::from_items(&program.items)
    }

    /// Discover all loaded modules in stable display/path order. The loader
    /// normally supplies this order already, but the registry owns the
    /// deterministic boundary for package/tooling callers too.
    pub fn from_modules(modules: &[LoadedModule]) -> Self {
        let mut ordered = modules.iter().collect::<Vec<_>>();
        ordered.sort_by(|left, right| {
            left.display
                .cmp(&right.display)
                .then_with(|| left.path.cmp(&right.path))
        });
        let mut registry = Self::default();
        for module in ordered {
            registry.append(Self::from_items(&module.items));
        }
        registry
    }

    pub fn from_bundle(bundle: &ProgramBundle) -> Self {
        Self::from_modules(&bundle.modules)
    }

    /// The entry module is the job namespace for `jet jobs` and executable
    /// completion. Imported helpers remain callable by code but do not become
    /// a second argv command namespace.
    pub fn for_entry(bundle: &ProgramBundle) -> Self {
        bundle
            .modules
            .get(bundle.entry)
            .map_or_else(Self::default, |module| Self::from_items(&module.items))
    }

    pub fn append(&mut self, other: Self) {
        self.jobs.extend(other.jobs);
    }

    pub fn into_jobs(self) -> Vec<JobFact> {
        self.jobs
    }

    pub fn jobs(&self) -> &[JobFact] {
        &self.jobs
    }

    pub fn visible_jobs(&self) -> Vec<&JobFact> {
        self.jobs
            .iter()
            .filter(|job| job.scope != JobScope::Internal)
            .collect()
    }

    pub fn find(&self, name: &str) -> Option<&JobFact> {
        self.jobs.iter().find(|job| job.name == name)
    }

    pub fn find_visible(&self, name: &str) -> Option<&JobFact> {
        self.jobs
            .iter()
            .find(|job| job.name == name && job.scope != JobScope::Internal)
    }
    /// Resolve one checked root to its transitive dependency closure. The
    /// returned rows are dependency-first and each declaration appears once;
    /// duplicate names, missing predecessors, zero bounds, and cycles fail
    /// closed instead of being silently re-planned by a host.
    pub fn dependency_closure(&self, root: &str) -> Result<Vec<&JobFact>, String> {
        fn visit<'a>(
            registry: &'a JobRegistry,
            name: &str,
            state: &mut BTreeMap<String, u8>,
            stack: &mut Vec<String>,
            order: &mut Vec<&'a JobFact>,
        ) -> Result<(), String> {
            match state.get(name).copied() {
                Some(2) => return Ok(()),
                Some(1) => {
                    let start = stack.iter().position(|candidate| candidate == name).unwrap_or(0);
                    let mut cycle = stack[start..].to_vec();
                    cycle.push(name.to_string());
                    return Err(format!(
                        "job graph contains a cycle: {}",
                        cycle.join(" -> ")
                    ));
                }
                _ => {}
            }
            let candidates = registry
                .jobs
                .iter()
                .filter(|job| job.name == name)
                .collect::<Vec<_>>();
            let job = match candidates.as_slice() {
                [] => return Err(format!("job graph references unknown job `{name}`")),
                [job] => *job,
                _ => return Err(format!("job `{name}` has an ambiguous declaration")),
            };
            if job.parallel == 0 {
                return Err(format!("job `{name}` declares a zero parallel bound"));
            }
            state.insert(name.to_string(), 1);
            stack.push(name.to_string());
            for dependency in &job.after {
                visit(registry, dependency, state, stack, order)?;
            }
            stack.pop();
            state.insert(name.to_string(), 2);
            order.push(job);
            Ok(())
        }

        let mut state = BTreeMap::new();
        let mut stack = Vec::new();
        let mut order = Vec::new();
        visit(self, root, &mut state, &mut stack, &mut order)?;
        Ok(order)
    }
    /// Validate every checked declaration before a host starts, inspects, or
    /// watches a graph. This keeps unknown predecessors, duplicate
    /// declarations, cycles, and invalid bounds on one closed path.
    pub fn validate_graph(&self) -> Result<(), String> {
        let mut names = self
            .jobs
            .iter()
            .map(|job| job.name.as_str())
            .collect::<Vec<_>>();
        names.sort_unstable();
        names.dedup();
        for name in names {
            self.dependency_closure(name)?;
        }
        Ok(())
    }



    pub fn completion_words(&self) -> Vec<String> {
        completion_words(&self.jobs)
    }
}

fn completion_words(jobs: &[JobFact]) -> Vec<String> {
    let mut words = Vec::new();
    for job in jobs {
        if job.scope == JobScope::Internal || words.iter().any(|word| word == &job.name) {
            continue;
        }
        words.push(job.name.clone());
    }
    words
}

fn schedule_text(marker: &crate::AST::EveryMarker) -> Option<String> {
    match &marker.arg {
        EveryArg::Duration {
            int, float, suffix, ..
        } => Some(format!(
            "{}{suffix}",
            int.as_ref()
                .map(|value| value.to_string())
                .or_else(|| float.as_ref().map(|value| value.to_string()))
                .unwrap_or_default()
        )),
        EveryArg::WallClock { text, .. } => Some(format!("day {text}")),
        EveryArg::Expression(_) => None,
    }
}

/// The checked callable selected by one CLI command.
///
/// This is the one target projection used by AOT emission, JIT planning, and
/// TIR evaluation. The schema owns command shape; this projection owns the
/// callable and its receiver/payload split.
#[derive(Debug, Clone)]
pub struct CLICommandTarget {
    pub function: Func,
    pub is_method: bool,
    pub is_binding: bool,
    pub bound_shared: bool,
    pub function_name: String,
}

fn command_payload_params(function: &Func, shared_type: &str, bound_shared: bool) -> Vec<Param> {
    function
        .params
        .iter()
        .filter(|param| {
            param.name != Syntax::KW_SELF
                && !(bound_shared
                    && matches!(&param.ty, Type::Named(name) if name.rsplit('.').next().unwrap_or(name) == shared_type))
        })
        .cloned()
        .collect()
}

impl CLICommandTarget {
    pub fn payload_params(&self, shared_type: &str) -> Vec<Param> {
        command_payload_params(&self.function, shared_type, self.bound_shared)
    }
}

/// Resolve a schema command to its canonical callable target.
///
/// Methods win over same-named bindings. A binding may target a free function
/// whose first parameter is the shared command struct; that parameter is the
/// command receiver and is not part of the decoded payload.
pub fn command_target(
    structure: &StructDef,
    command: &CLISubcommandSchema,
    items: &[Item],
    function_owner: &str,
) -> Option<CLICommandTarget> {
    let method = structure.methods.iter().find(|method| {
        method.name.to_lowercase() == command.name
            && !structure
                .fields
                .iter()
                .any(|field| field.name == method.name && field.computed.is_some())
    });

    let (function, is_binding) = if let Some(method) = method {
        (method.clone(), false)
    } else {
        let binding = structure
            .cli_bindings
            .iter()
            .find(|binding| binding.name.to_lowercase() == command.name)?;
        let target_name = binding_target_name(binding)?;
        let function = items.iter().find_map(|item| match item {
            Item::Func(function) if function.name == target_name => Some(function.clone()),
            _ => None,
        })?;
        (function, true)
    };

    let is_method = function
        .params
        .iter()
        .any(|param| param.name == Syntax::KW_SELF);
    let bound_shared = is_binding
        && function.params.first().is_some_and(|param| {
            matches!(&param.ty, Type::Named(name) if name.rsplit('.').next().unwrap_or(name) == structure.name)
        });
    let function_name = if is_method {
        format!("{function_owner}::{}", function.name)
    } else if let Some((module, _)) = function_owner.rsplit_once("::") {
        format!("{module}::{}", function.name)
    } else {
        function.name.clone()
    };

    Some(CLICommandTarget {
        function,
        is_method,
        is_binding,
        bound_shared,
        function_name,
    })
}

/// Checked command surface for an executable. Plain `fn run()` deliberately
/// produces an empty record so external completion can still register the
/// built-in `--help` surface without executing the program.
pub fn executable_schema(bundle: &ProgramBundle) -> CLICommandSchema {
    let mut schema = entry_schema_for_bundle(bundle).unwrap_or(CLICommandSchema {
        entry_type: String::new(),
        description: None,
        inputs: Vec::new(),
        commands: Vec::new(),
        jobs: Vec::new(),
        standard: false,
        version: None,
    });
    schema.jobs = JobRegistry::for_entry(bundle).into_jobs();
    schema
}

fn selected_entry_type(items: &[Item]) -> Option<&str> {
    items
        .iter()
        .find_map(|item| {
            let Item::Const(value) = item else {
                return None;
            };
            let output = value.resolved_output.as_ref()?;
            if !output.selected
                || output.kind != crate::AST::OutputKind::Executable
                || output.params.len() != 1
            {
                return None;
            }
            match &output.params[0].1 {
                Type::Named(name) => Some(name.as_str()),
                _ => None,
            }
        })
        .or_else(|| {
            items.iter().find_map(|item| {
                let Item::Func(function) = item else {
                    return None;
                };
                if function.name != "run" {
                    return None;
                }
                if function.params.len() == 1 {
                    if let Type::Named(name) = &function.params[0].ty {
                        let is_local_type = items
                            .iter()
                            .any(|item| matches!(item, Item::Struct(s) if s.name == name.as_str()));
                        if is_local_type || name.contains('.') || name.contains("::") {
                            return Some(name.as_str());
                        }
                        // An unqualified imported struct keeps its source leaf
                        // in the entry AST. It is typed CLI when the direct
                        // scalar path does not recognize the parameter.
                        if direct_run_function(items).is_none() {
                            return Some(name.as_str());
                        }
                    }
                }
                direct_run_function(items).is_some().then_some("run")
            })
        })
}

fn direct_run_function(items: &[Item]) -> Option<&Func> {
    items.iter().find_map(|item| {
        let Item::Func(function) = item else {
            return None;
        };
        if function.name != "run"
            || function.params.is_empty()
            || function_inputs(function).is_none()
        {
            return None;
        }
        if function.params.len() == 1 {
            if let Type::Named(name) = &function.params[0].ty {
                let is_local_type = items
                    .iter()
                    .any(|item| matches!(item, Item::Struct(s) if s.name == name.as_str()));
                if is_local_type || name.contains('.') {
                    return None;
                }
            }
        }
        Some(function)
    })
}

pub fn is_direct_run_entry(items: &[Item]) -> bool {
    let has_selected_executable = items.iter().any(|item| {
        matches!(
            item,
            Item::Const(value)
                if value.resolved_output.as_ref().is_some_and(|output| {
                    output.selected && output.kind == crate::AST::OutputKind::Executable
                })
        )
    });
    !has_selected_executable && direct_run_function(items).is_some()
}

fn selected_entry_type_source(bundle: &ProgramBundle) -> Option<(usize, &str)> {
    let entry = &bundle.modules[bundle.entry];
    if let Some(selected) = entry.items.iter().find_map(|item| {
        let Item::Const(value) = item else {
            return None;
        };
        let output = value.resolved_output.as_ref()?;
        if !output.selected
            || output.kind != crate::AST::OutputKind::Executable
            || output.params.len() != 1
        {
            return None;
        }
        match &output.params[0].1 {
            Type::Named(name) => Some((output.module, name.as_str())),
            _ => None,
        }
    }) {
        return Some(selected);
    }
    entry.items.iter().find_map(|item| {
        let Item::Func(function) = item else {
            return None;
        };
        if function.name != "run" {
            return None;
        }
        if function.params.len() == 1 {
            if let Type::Named(name) = &function.params[0].ty {
                let is_local_type = entry
                    .items
                    .iter()
                    .any(|item| matches!(item, Item::Struct(s) if s.name == name.as_str()));
                if is_local_type || name.contains('.') || name.contains("::") {
                    return Some((bundle.entry, name.as_str()));
                }
                if direct_run_function(&entry.items).is_none() {
                    return Some((bundle.entry, name.as_str()));
                }
            }
        }
        direct_run_function(&entry.items).map(|_| (bundle.entry, "run"))
    })
}

pub fn schema_for_type(items: &[Item], name: &str) -> Option<CLICommandSchema> {
    if let Some(structure) = items.iter().find_map(|item| match item {
        Item::Struct(structure) if structure.name == name => {
            command_schema_with_items(items, structure)
        }
        _ => None,
    }) {
        return Some(structure);
    }
    None
}

/// Module containing the entry parameter's checked CLI type. Local types win;
/// otherwise the type must be public in one directly imported file module.
pub fn entry_type_module(bundle: &ProgramBundle) -> Option<usize> {
    let (source, name) = selected_entry_type_source(bundle)?;
    if std::env::var_os("JET_DEBUG_CLI").is_some() {
        eprintln!(
            "cli-debug entry_type_module source={source} name={name} entry={}",
            bundle.entry
        );
    }
    let entry = &bundle.modules[source];
    if name == "run"
        && entry
            .items
            .iter()
            .any(|item| matches!(item, Item::Func(function) if function.name == "run"))
    {
        return Some(source);
    }
    if let Some(owner) = bundle.name_ledger.nominal_module(name) {
        let leaf = name.rsplit_once("::").map_or(name, |(_, leaf)| leaf);
        return bundle
            .name_ledger
            .visible(source, owner, leaf)
            .then_some(owner);
    }
    let (wanted_alias, leaf) = name
        .split_once('.')
        .map_or((None, name), |(alias, leaf)| (Some(alias), leaf));
    if wanted_alias.is_none()
        && entry.items.iter().any(|item| match item {
            Item::Struct(structure) => structure.name == leaf,
            _ => false,
        })
    {
        return Some(source);
    }
    let mut candidates = entry
        .imports
        .iter()
        .filter_map(|import| {
            if wanted_alias.is_some_and(|alias| import.import_alias() != alias) {
                return None;
            }
            let target = bundle.name_ledger.import_target(source, import.span)?;
            bundle
                .name_ledger
                .visible(source, target, leaf)
                .then_some(target)
        })
        .collect::<Vec<_>>();
    if std::env::var_os("JET_DEBUG_CLI").is_some() {
        eprintln!("cli-debug candidates={candidates:?}");
    }
    candidates.sort_unstable();
    candidates.dedup();
    match candidates.as_slice() {
        [target] => Some(*target),
        _ => None,
    }
}

/// Checked schema for a typed `fn run`, including a CLI type declared in a
/// directly imported module. Codegen, dossier, metadata, and completion share it.
pub fn entry_schema_for_bundle(bundle: &ProgramBundle) -> Option<CLICommandSchema> {
    let (source, name) = selected_entry_type_source(bundle)?;
    let leaf = name.rsplit('.').next().unwrap_or(name);
    let module = entry_type_module(bundle)?;
    let mut schema = if leaf == "run"
        && source == bundle.entry
        && is_direct_run_entry(&bundle.modules[source].items)
    {
        direct_run_function(&bundle.modules[source].items)
            .and_then(function_schema)
            .expect("direct run entry has a canonical function schema")
    } else {
        schema_for_type(&bundle.modules[module].items, leaf)?
    };
    if schema.standard {
        schema.version = Some(bundle.build_facts.package_version.clone());
    }
    schema.jobs = JobRegistry::for_entry(bundle).into_jobs();
    Some(schema)
}

/// Checked schema for a typed `fn run` in one module.
pub fn entry_schema(items: &[Item]) -> Option<CLICommandSchema> {
    let name = selected_entry_type(items)?;
    let leaf = name.rsplit('.').next().unwrap_or(name);
    let mut schema = if leaf == "run" {
        if is_direct_run_entry(items) {
            direct_run_function(items).and_then(function_schema)?
        } else {
            schema_for_type(items, leaf)?
        }
    } else {
        schema_for_type(items, leaf)?
    };
    schema.jobs = JobRegistry::from_items(items).into_jobs();
    Some(schema)
}

/// Canonical, versioned JetCommandSchema record. The digest makes corruption
/// fail closed; embedding these bytes before linking binds them into the
/// executable and therefore its cache/signing identity.
pub fn encode_record(schema: &CLICommandSchema) -> Vec<u8> {
    let mut payload = Vec::new();
    put_string(&mut payload, &schema.entry_type);
    put_optional_string(&mut payload, schema.description.as_deref());
    payload.push(u8::from(schema.standard));
    put_optional_string(&mut payload, schema.version.as_deref());
    put_u32(&mut payload, schema.inputs.len() as u32);
    for input in &schema.inputs {
        encode_input(&mut payload, input);
    }
    put_u32(&mut payload, schema.commands.len() as u32);
    for command in &schema.commands {
        put_string(&mut payload, &command.name);
        put_optional_string(&mut payload, command.description.as_deref());
        put_u32(&mut payload, command.inputs.len() as u32);
        for input in &command.inputs {
            encode_input(&mut payload, input);
        }
    }
    put_u32(&mut payload, schema.jobs.len() as u32);
    for job in &schema.jobs {
        encode_job(&mut payload, job);
    }
    finish_record(payload)
}

fn finish_record(payload: Vec<u8>) -> Vec<u8> {
    let mut record = Vec::with_capacity(46 + payload.len());
    record.extend_from_slice(RECORD_MAGIC);
    record.extend_from_slice(&RECORD_VERSION.to_le_bytes());
    put_u32(&mut record, payload.len() as u32);
    record.extend_from_slice(&crate::SHA256::sha256(&payload));
    record.extend_from_slice(&payload);
    record
}
/// Encode a command record directly from canonical MIR rows.
///
/// The executable record format predates MIR and intentionally stores only
/// the projection represented by `CLICommandSchema`. This entry point keeps
/// Web emission independent of AST and `CtValue`: every representable MIR
/// field is written with the same tags and ordering as `encode_record`.
pub fn encode_mir_record(
    entry_type: &str,
    description: Option<&str>,
    standard: bool,
    version: Option<&str>,
    inputs: &[crate::MIR::MirCliInput],
    commands: &[crate::MIR::MirCliCommand],
    jobs: &[crate::MIR::MirJob],
) -> Vec<u8> {
    let mut payload = Vec::new();
    put_string(&mut payload, entry_type);
    put_optional_string(&mut payload, description);
    payload.push(u8::from(standard));
    put_optional_string(&mut payload, version);
    put_u32(&mut payload, inputs.len() as u32);
    for input in inputs {
        encode_mir_input(&mut payload, input);
    }
    put_u32(&mut payload, commands.len() as u32);
    for command in commands {
        put_string(&mut payload, &command.name);
        put_optional_string(&mut payload, command.description.as_deref());
        put_u32(&mut payload, command.inputs.len() as u32);
        for input in &command.inputs {
            encode_mir_input(&mut payload, input);
        }
    }
    put_u32(&mut payload, jobs.len() as u32);
    for job in jobs {
        encode_mir_job(&mut payload, job);
    }
    finish_record(payload)
}

fn encode_mir_input(payload: &mut Vec<u8>, input: &crate::MIR::MirCliInput) {
    put_string(payload, &input.name);
    put_string(payload, &input.label);
    put_optional_string(payload, input.short.as_deref());
    put_optional_string(payload, input.env.as_deref());
    put_string(payload, &input.help);
    put_optional_string(payload, input.metavar.as_deref());
    match &input.shape {
        crate::MIR::MirCliInputShape::Flag => payload.push(0),
        crate::MIR::MirCliInputShape::Value {
            kind,
            optional,
            default,
        } => {
            payload.push(1);
            payload.push(match kind {
                crate::MIR::MirCliValueKind::Bool => 0,
                crate::MIR::MirCliValueKind::Int => 1,
                crate::MIR::MirCliValueKind::Float => 2,
                crate::MIR::MirCliValueKind::String => 3,
                crate::MIR::MirCliValueKind::Path => 4,
            });
            payload.push(u8::from(*optional));
            match default {
                None => payload.push(0),
                Some(crate::MIR::MirCliDefault::TypeDefault) => payload.push(1),
                Some(crate::MIR::MirCliDefault::Value(value)) => {
                    payload.push(2);
                    put_string(payload, &mir_constant_display(value));
                }
            }
        }
    }
    match input.positional {
        None => payload.push(0),
        Some(order) => {
            payload.push(1);
            put_u32(payload, u32::from(order));
        }
    }
}

fn encode_mir_job(payload: &mut Vec<u8>, job: &crate::MIR::MirJob) {
    put_string(payload, &job.name);
    payload.push(match job.scope {
        crate::MIR::MirJobScope::Dev => 0,
        crate::MIR::MirJobScope::Ship => 1,
        crate::MIR::MirJobScope::Internal => 2,
    });
    put_optional_string(payload, None);
    let schedule = job.schedule.as_ref().map(mir_job_schedule_display);
    put_optional_string(payload, schedule.as_deref());
    put_u32(payload, job.after.len() as u32);
    for dependency in &job.after {
        put_string(payload, dependency);
    }
    put_u32(payload, job.input_paths.len() as u32);
    for input in &job.input_paths {
        put_string(payload, input);
    }
    put_u32(payload, job.output_paths.len() as u32);
    for output in &job.output_paths {
        put_string(payload, output);
    }
    put_u32(payload, job.parallel as u32);
    put_u32(payload, job.inputs.len() as u32);
    for input in &job.inputs {
        put_string(payload, &input.name);
        put_string(payload, &input.label);
        put_string(payload, &input.ty.name());
        payload.push(u8::from(mir_cli_input_required(input)));
        match &input.shape {
            crate::MIR::MirCliInputShape::Value {
                default: Some(crate::MIR::MirCliDefault::TypeDefault),
                ..
            } => put_optional_string(payload, Some("type default")),
            crate::MIR::MirCliInputShape::Value {
                default: Some(crate::MIR::MirCliDefault::Value(value)),
                ..
            } => {
                let display = mir_constant_display(value);
                put_optional_string(payload, Some(&display));
            }
            crate::MIR::MirCliInputShape::Flag
            | crate::MIR::MirCliInputShape::Value { default: None, .. } => {
                put_optional_string(payload, None);
            }
        }
        payload.push(u8::from(input.variadic));
        payload.push(match input.zone {
            crate::MIR::MirParamZone::PositionalOnly => 0,
            crate::MIR::MirParamZone::Either => 1,
            crate::MIR::MirParamZone::LabelOnly => 2,
        });
    }
    put_u32(payload, job.packages.len() as u32);
    for package in &job.packages {
        put_string(payload, package);
    }
    put_optional_string(payload, job.working_directory.as_deref());
    encode_mir_job_skip(payload, job.skip.as_ref());
    payload.push(match job.cache {
        crate::MIR::MirJobCachePolicy::Uncached => 0,
        crate::MIR::MirJobCachePolicy::Local => 1,
        crate::MIR::MirJobCachePolicy::Shared => 2,
    });
    put_u32(payload, job.limits.len() as u32);
    for (name, value) in &job.limits {
        put_string(payload, name);
        put_string(payload, value);
    }
}

fn encode_mir_job_skip(
    payload: &mut Vec<u8>,
    skip: Option<&crate::MIR::MirJobSkip>,
) {
    match skip {
        None => payload.push(0),
        Some(crate::MIR::MirJobSkip::Always(reason)) => {
            payload.push(1);
            put_string(payload, reason);
        }
        Some(crate::MIR::MirJobSkip::UnlessPlatform(platform)) => {
            payload.push(2);
            put_string(payload, platform);
        }
    }
}

fn mir_cli_input_required(input: &crate::MIR::MirCliInput) -> bool {
    matches!(
        input.shape,
        crate::MIR::MirCliInputShape::Value {
            optional: false,
            default: None,
            ..
        }
    )
}

fn mir_job_schedule_display(schedule: &crate::MIR::MirJobSchedule) -> String {
    match schedule {
        crate::MIR::MirJobSchedule::Duration { nanos } => format!("{nanos}ns"),
        crate::MIR::MirJobSchedule::WallClockTime { hour, minute } => {
            format!("day {hour:02}:{minute:02}")
        }
    }
}

fn mir_constant_display(value: &crate::MIR::MirConstant) -> String {
    match value {
        crate::MIR::MirConstant::Int { value, .. } => value.to_string(),
        crate::MIR::MirConstant::Float { value, .. } => format!("{value:?}"),
        crate::MIR::MirConstant::Bool(value) => value.to_string(),
        crate::MIR::MirConstant::Char(value) => value.to_string(),
        crate::MIR::MirConstant::String(value) => value.clone(),
        crate::MIR::MirConstant::Bytes(values) => format!(
            "[{}]",
            values.iter().map(|value| value.to_string()).collect::<Vec<_>>().join(", ")
        ),
        crate::MIR::MirConstant::Unit => "()".to_string(),
        crate::MIR::MirConstant::BigInt(value) => value.clone(),
        crate::MIR::MirConstant::List(values) => format!(
            "[{}]",
            values.iter().map(mir_constant_display).collect::<Vec<_>>().join(", ")
        ),
        crate::MIR::MirConstant::Map(values) => format!(
            "[{}]",
            values
                .iter()
                .map(|(key, value)| format!(
                    "{}: {}",
                    mir_const_key_display(key),
                    mir_constant_display(value)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        crate::MIR::MirConstant::Struct { type_name, fields } => format!(
            "{} {{ {} }}",
            type_name,
            fields
                .iter()
                .map(|(name, value)| format!("{name}: {}", mir_constant_display(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        crate::MIR::MirConstant::Enum {
            type_name,
            variant,
            args,
        } => {
            let rendered = args
                .iter()
                .map(|(label, value)| match label {
                    Some(label) => format!("{label}: {}", mir_constant_display(value)),
                    None => mir_constant_display(value),
                })
                .collect::<Vec<_>>()
                .join(", ");
            if rendered.is_empty() {
                format!("{type_name}::{variant}")
            } else {
                format!("{type_name}::{variant}({rendered})")
            }
        }
        crate::MIR::MirConstant::Present(value) => mir_constant_display(value),
        crate::MIR::MirConstant::Failed(crate::MIR::MirConstReport::Clean(_)) => "null".to_string(),
        crate::MIR::MirConstant::Failed(crate::MIR::MirConstReport::Told(_)) => "err".to_string(),
    }
}

fn mir_const_key_display(value: &crate::MIR::MirConstKey) -> String {
    match value {
        crate::MIR::MirConstKey::Int(value) => value.to_string(),
        crate::MIR::MirConstKey::String(value) => value.clone(),
        crate::MIR::MirConstKey::Bool(value) => value.to_string(),
        crate::MIR::MirConstKey::Char(value) => value.to_string(),
        crate::MIR::MirConstKey::Tuple(fields) => format!(
            "({})",
            fields
                .iter()
                .map(|(name, value)| format!("{name}: {}", mir_const_key_display(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        crate::MIR::MirConstKey::Struct { type_name, fields } => format!(
            "{} {{ {} }}",
            type_name,
            fields
                .iter()
                .map(|(name, value)| format!("{name}: {}", mir_const_key_display(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        crate::MIR::MirConstKey::Enum {
            type_name,
            variant,
        } => format!("{type_name}::{variant}"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataError {
    UnknownFormat,
    Missing,
    Duplicate,
    Malformed(&'static str),
    UnsupportedVersion(u16),
}

impl std::fmt::Display for MetadataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MetadataError::UnknownFormat => {
                write!(f, "the file is not an ELF, PE, Mach-O, or Wasm executable")
            }
            MetadataError::Missing => write!(f, "the executable has no JetCommandSchema metadata"),
            MetadataError::Duplicate => write!(
                f,
                "the executable contains more than one JetCommandSchema record"
            ),
            MetadataError::Malformed(why) => {
                write!(f, "the JetCommandSchema record is malformed ({why})")
            }
            MetadataError::UnsupportedVersion(version) => {
                write!(f, "JetCommandSchema version {version} is not supported")
            }
        }
    }
}

pub fn read_executable(bytes: &[u8]) -> Result<CLICommandSchema, MetadataError> {
    let sections = if bytes.starts_with(b"\x7fELF") {
        elf_sections(bytes)?
    } else if bytes.starts_with(b"MZ") {
        pe_sections(bytes)?
    } else if bytes.starts_with(b"\0asm") {
        wasm_sections(bytes)?
    } else if is_mach(bytes) {
        mach_sections(bytes)?
    } else {
        return Err(MetadataError::UnknownFormat);
    };
    if sections.is_empty() {
        return Err(MetadataError::Missing);
    }
    if sections.len() != 1 {
        return Err(MetadataError::Duplicate);
    }
    decode_record(sections[0])
}

pub fn decode_record(record: &[u8]) -> Result<CLICommandSchema, MetadataError> {
    if record.len() < 46 || &record[..8] != RECORD_MAGIC {
        return Err(MetadataError::Malformed("bad record header"));
    }
    let version = u16::from_le_bytes([record[8], record[9]]);
    if version != RECORD_VERSION {
        return Err(MetadataError::UnsupportedVersion(version));
    }
    let len = u32::from_le_bytes(record[10..14].try_into().unwrap()) as usize;
    if len > MAX_RECORD_BYTES || 46usize.checked_add(len) != Some(record.len()) {
        return Err(MetadataError::Malformed("invalid record length"));
    }
    let payload = &record[46..];
    if crate::SHA256::sha256(payload) != record[14..46] {
        return Err(MetadataError::Malformed("digest mismatch"));
    }
    let mut cursor = Cursor::new(payload);
    let entry_type = cursor.string()?;
    let description = cursor.optional_string()?;
    let standard = match cursor.byte()? {
        0 => false,
        1 => true,
        _ => return Err(MetadataError::Malformed("invalid standard bit")),
    };
    let version = cursor.optional_string()?;
    let count = cursor.u32()? as usize;
    if count > MAX_INPUTS {
        return Err(MetadataError::Malformed("too many inputs"));
    }
    let mut inputs = Vec::with_capacity(count);
    for _ in 0..count {
        inputs.push(decode_input(&mut cursor)?);
    }
    let command_count = cursor.u32()? as usize;
    if command_count > MAX_INPUTS {
        return Err(MetadataError::Malformed("too many commands"));
    }
    let mut commands = Vec::with_capacity(command_count);
    for _ in 0..command_count {
        let name = cursor.string()?;
        let description = cursor.optional_string()?;
        let input_count = cursor.u32()? as usize;
        if input_count > MAX_INPUTS {
            return Err(MetadataError::Malformed("too many command inputs"));
        }
        let mut command_inputs = Vec::with_capacity(input_count);
        for _ in 0..input_count {
            command_inputs.push(decode_input(&mut cursor)?);
        }
        commands.push(CLISubcommandSchema {
            name,
            description,
            inputs: command_inputs,
        });
    }
    let job_count = cursor.u32()? as usize;
    if job_count > MAX_JOBS {
        return Err(MetadataError::Malformed("too many jobs"));
    }
    let mut jobs = Vec::with_capacity(job_count);
    for _ in 0..job_count {
        jobs.push(decode_job(&mut cursor)?);
    }
    if !cursor.done() {
        return Err(MetadataError::Malformed("trailing payload bytes"));
    }
    Ok(CLICommandSchema {
        entry_type,
        description,
        inputs,
        commands,
        jobs,
        standard,
        version,
    })
}

fn encode_job(payload: &mut Vec<u8>, job: &JobFact) {
    put_string(payload, &job.name);
    payload.push(match job.scope {
        JobScope::Dev => 0,
        JobScope::Ship => 1,
        JobScope::Internal => 2,
    });
    put_optional_string(payload, job.doc.as_deref());
    put_optional_string(payload, job.schedule.as_deref());
    put_u32(payload, job.after.len() as u32);
    for dependency in &job.after {
        put_string(payload, dependency);
    }
    put_u32(payload, job.inputs.len() as u32);
    for input in &job.inputs {
        put_string(payload, input);
    }
    put_u32(payload, job.outputs.len() as u32);
    for output in &job.outputs {
        put_string(payload, output);
    }
    put_u32(payload, job.parallel as u32);
    put_u32(payload, job.arguments.len() as u32);
    for argument in &job.arguments {
        put_string(payload, &argument.name);
        put_string(payload, &argument.label);
        put_string(payload, &argument.ty);
        payload.push(u8::from(argument.required));
        put_optional_string(payload, argument.default.as_deref());
        payload.push(u8::from(argument.variadic));
        payload.push(match argument.zone {
            ParamZone::PositionalOnly => 0,
            ParamZone::Either => 1,
            ParamZone::LabelOnly => 2,
        });
    }
    put_u32(payload, job.packages.len() as u32);
    for package in &job.packages {
        put_string(payload, package);
    }
    put_optional_string(payload, job.cwd.as_deref());
    encode_job_skip(payload, job.skip.as_ref());
    payload.push(match job.cache {
        JobCachePolicy::Uncached => 0,
        JobCachePolicy::Local => 1,
        JobCachePolicy::Shared => 2,
    });
    put_u32(payload, job.limits.len() as u32);
    for (name, value) in &job.limits {
        put_string(payload, name);
        put_string(payload, value);
    }
}

fn encode_job_skip(payload: &mut Vec<u8>, skip: Option<&JobSkip>) {
    match skip {
        None => payload.push(0),
        Some(JobSkip::Always(reason)) => {
            payload.push(1);
            put_string(payload, reason);
        }
        Some(JobSkip::UnlessPlatform { platform }) => {
            payload.push(2);
            put_string(payload, platform);
        }
    }
}

fn decode_job(cursor: &mut Cursor<'_>) -> Result<JobFact, MetadataError> {
    let name = cursor.string()?;
    let scope = match cursor.byte()? {
        0 => JobScope::Dev,
        1 => JobScope::Ship,
        2 => JobScope::Internal,
        _ => return Err(MetadataError::Malformed("unknown job scope")),
    };
    let doc = cursor.optional_string()?;
    let schedule = cursor.optional_string()?;
    let after_count = cursor.u32()? as usize;
    if after_count > MAX_JOBS {
        return Err(MetadataError::Malformed("too many job dependencies"));
    }
    let mut after = Vec::with_capacity(after_count);
    for _ in 0..after_count {
        after.push(cursor.string()?);
    }
    let input_count = cursor.u32()? as usize;
    if input_count > MAX_INPUTS {
        return Err(MetadataError::Malformed("too many job inputs"));
    }
    let mut inputs = Vec::with_capacity(input_count);
    for _ in 0..input_count {
        inputs.push(cursor.string()?);
    }
    let output_count = cursor.u32()? as usize;
    if output_count > MAX_INPUTS {
        return Err(MetadataError::Malformed("too many job outputs"));
    }
    let mut outputs = Vec::with_capacity(output_count);
    for _ in 0..output_count {
        outputs.push(cursor.string()?);
    }
    let parallel = cursor.u32()? as usize;
    if parallel == 0 {
        return Err(MetadataError::Malformed("job parallel bound must be positive"));
    }
    let argument_count = cursor.u32()? as usize;
    if argument_count > MAX_INPUTS {
        return Err(MetadataError::Malformed("too many job arguments"));
    }
    let mut arguments = Vec::with_capacity(argument_count);
    for _ in 0..argument_count {
        let name = cursor.string()?;
        let label = cursor.string()?;
        let ty = cursor.string()?;
        let required = match cursor.byte()? {
            0 => false,
            1 => true,
            _ => return Err(MetadataError::Malformed("invalid job argument required bit")),
        };
        let default = cursor.optional_string()?;
        let variadic = match cursor.byte()? {
            0 => false,
            1 => true,
            _ => return Err(MetadataError::Malformed("invalid job argument variadic bit")),
        };
        let zone = match cursor.byte()? {
            0 => ParamZone::PositionalOnly,
            1 => ParamZone::Either,
            2 => ParamZone::LabelOnly,
            _ => return Err(MetadataError::Malformed("unknown job argument zone")),
        };
        arguments.push(JobArgumentSchema {
            name,
            label,
            ty,
            required,
            default,
            variadic,
            zone,
        });
    }
    let package_count = cursor.u32()? as usize;
    if package_count > MAX_INPUTS {
        return Err(MetadataError::Malformed("too many job packages"));
    }
    let mut packages = Vec::with_capacity(package_count);
    for _ in 0..package_count {
        packages.push(cursor.string()?);
    }
    let cwd = cursor.optional_string()?;
    let skip = decode_job_skip(cursor)?;
    let cache = match cursor.byte()? {
        0 => JobCachePolicy::Uncached,
        1 => JobCachePolicy::Local,
        2 => JobCachePolicy::Shared,
        _ => return Err(MetadataError::Malformed("unknown job cache policy")),
    };
    let limit_count = cursor.u32()? as usize;
    if limit_count > MAX_INPUTS {
        return Err(MetadataError::Malformed("too many job limits"));
    }
    let mut limits = BTreeMap::new();
    for _ in 0..limit_count {
        limits.insert(cursor.string()?, cursor.string()?);
    }
    Ok(JobFact {
        name,
        scope,
        doc,
        arguments,
        after,
        packages,
        cwd,
        inputs,
        outputs,
        skip,
        cache,
        parallel,
        limits,
        schedule,
    })
}

fn decode_job_skip(cursor: &mut Cursor<'_>) -> Result<Option<JobSkip>, MetadataError> {
    match cursor.byte()? {
        0 => Ok(None),
        1 => Ok(Some(JobSkip::Always(cursor.string()?))),
        2 => Ok(Some(JobSkip::UnlessPlatform {
            platform: cursor.string()?,
        })),
        _ => Err(MetadataError::Malformed("unknown job skip rule")),
    }
}

fn encode_input(payload: &mut Vec<u8>, input: &CLIInputSchema) {
    put_string(payload, &input.field);
    put_string(payload, &input.flag);
    put_optional_string(payload, input.short.as_deref());
    put_optional_string(payload, input.env.as_deref());
    put_string(payload, &input.help);
    put_optional_string(payload, input.metavar.as_deref());
    match &input.shape {
        CLIInputShape::Flag => payload.push(0),
        CLIInputShape::Value {
            kind,
            optional,
            default,
        } => {
            payload.push(1);
            payload.push(match kind {
                CLIValueKind::Bool => 0,
                CLIValueKind::Int => 1,
                CLIValueKind::Float => 2,
                CLIValueKind::String => 3,
                CLIValueKind::Path => 4,
            });
            payload.push(u8::from(*optional));
            match default {
                None => payload.push(0),
                Some(CLIDefault::TypeDefault) => payload.push(1),
                Some(value) => {
                    payload.push(2);
                    put_string(payload, &value.display());
                }
            }
        }
    }
    // D-CLI-POS1 / RECORD_VERSION 2: optional positional order after shape.
    match input.positional {
        None => payload.push(0),
        Some(order) => {
            payload.push(1);
            put_u32(payload, order as u32);
        }
    }
}

fn decode_input(cursor: &mut Cursor<'_>) -> Result<CLIInputSchema, MetadataError> {
    let field = cursor.string()?;
    let flag = cursor.string()?;
    let short = cursor.optional_string()?;
    let env = cursor.optional_string()?;
    let help = cursor.string()?;
    let metavar = cursor.optional_string()?;
    let shape = match cursor.byte()? {
        0 => CLIInputShape::Flag,
        1 => {
            let kind = match cursor.byte()? {
                0 => CLIValueKind::Bool,
                1 => CLIValueKind::Int,
                2 => CLIValueKind::Float,
                3 => CLIValueKind::String,
                4 => CLIValueKind::Path,
                _ => return Err(MetadataError::Malformed("unknown input kind")),
            };
            let optional = match cursor.byte()? {
                0 => false,
                1 => true,
                _ => return Err(MetadataError::Malformed("invalid optional bit")),
            };
            let default = match cursor.byte()? {
                0 => None,
                1 => Some(CLIDefault::TypeDefault),
                2 => Some(CLIDefault::Recorded(cursor.string()?)),
                _ => return Err(MetadataError::Malformed("unknown default kind")),
            };
            CLIInputShape::Value {
                kind,
                optional,
                default,
            }
        }
        _ => return Err(MetadataError::Malformed("unknown input shape")),
    };
    let positional = match cursor.byte()? {
        0 => None,
        1 => {
            let order = cursor.u32()?;
            if order > u16::MAX as u32 {
                return Err(MetadataError::Malformed("positional order out of range"));
            }
            Some(order as u16)
        }
        _ => return Err(MetadataError::Malformed("invalid positional bit")),
    };
    Ok(CLIInputSchema {
        field,
        flag,
        short,
        env,
        help,
        metavar,
        shape,
        positional,
    })
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn put_string(out: &mut Vec<u8>, value: &str) {
    put_u32(out, value.len() as u32);
    out.extend_from_slice(value.as_bytes());
}
fn put_optional_string(out: &mut Vec<u8>, value: Option<&str>) {
    out.push(u8::from(value.is_some()));
    if let Some(value) = value {
        put_string(out, value);
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }
    fn byte(&mut self) -> Result<u8, MetadataError> {
        let value = *self
            .bytes
            .get(self.at)
            .ok_or(MetadataError::Malformed("truncated payload"))?;
        self.at += 1;
        Ok(value)
    }
    fn u32(&mut self) -> Result<u32, MetadataError> {
        let end = self
            .at
            .checked_add(4)
            .ok_or(MetadataError::Malformed("length overflow"))?;
        let bytes = self
            .bytes
            .get(self.at..end)
            .ok_or(MetadataError::Malformed("truncated payload"))?;
        self.at = end;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<String, MetadataError> {
        let len = self.u32()? as usize;
        if len > MAX_STRING_BYTES {
            return Err(MetadataError::Malformed("string too long"));
        }
        let end = self
            .at
            .checked_add(len)
            .ok_or(MetadataError::Malformed("length overflow"))?;
        let bytes = self
            .bytes
            .get(self.at..end)
            .ok_or(MetadataError::Malformed("truncated string"))?;
        self.at = end;
        String::from_utf8(bytes.to_vec())
            .map_err(|_| MetadataError::Malformed("string is not UTF-8"))
    }
    fn optional_string(&mut self) -> Result<Option<String>, MetadataError> {
        match self.byte()? {
            0 => Ok(None),
            1 => self.string().map(Some),
            _ => Err(MetadataError::Malformed("invalid optional string bit")),
        }
    }
    fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

fn bounds(bytes: &[u8], at: usize, len: usize) -> Result<&[u8], MetadataError> {
    let end = at
        .checked_add(len)
        .ok_or(MetadataError::Malformed("section range overflow"))?;
    bytes
        .get(at..end)
        .ok_or(MetadataError::Malformed("section outside file"))
}

fn read_num(bytes: &[u8], at: usize, width: usize, little: bool) -> Result<u64, MetadataError> {
    let value = bounds(bytes, at, width)?;
    Ok(match (width, little) {
        (2, true) => u16::from_le_bytes(value.try_into().unwrap()) as u64,
        (2, false) => u16::from_be_bytes(value.try_into().unwrap()) as u64,
        (4, true) => u32::from_le_bytes(value.try_into().unwrap()) as u64,
        (4, false) => u32::from_be_bytes(value.try_into().unwrap()) as u64,
        (8, true) => u64::from_le_bytes(value.try_into().unwrap()),
        (8, false) => u64::from_be_bytes(value.try_into().unwrap()),
        _ => return Err(MetadataError::Malformed("invalid integer width")),
    })
}

fn usize_num(value: u64) -> Result<usize, MetadataError> {
    usize::try_from(value).map_err(|_| MetadataError::Malformed("file offset overflow"))
}

fn c_name(bytes: &[u8]) -> Result<&str, MetadataError> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[..end])
        .map_err(|_| MetadataError::Malformed("section name is not UTF-8"))
}

fn elf_sections(bytes: &[u8]) -> Result<Vec<&[u8]>, MetadataError> {
    let class = *bytes
        .get(4)
        .ok_or(MetadataError::Malformed("truncated ELF header"))?;
    let little = match bytes.get(5) {
        Some(1) => true,
        Some(2) => false,
        _ => return Err(MetadataError::Malformed("invalid ELF byte order")),
    };
    let (
        header,
        shoff_at,
        shentsize_at,
        shnum_at,
        shstr_at,
        shoff_width,
        sh_offset_at,
        sh_size_at,
        value_width,
    ) = match class {
        1 => (52, 32, 46, 48, 50, 4, 16, 20, 4),
        2 => (64, 40, 58, 60, 62, 8, 24, 32, 8),
        _ => return Err(MetadataError::Malformed("unsupported ELF class")),
    };
    bounds(bytes, 0, header)?;
    let shoff = usize_num(read_num(bytes, shoff_at, shoff_width, little)?)?;
    let entsize = usize_num(read_num(bytes, shentsize_at, 2, little)?)?;
    let count = usize_num(read_num(bytes, shnum_at, 2, little)?)?;
    let names_index = usize_num(read_num(bytes, shstr_at, 2, little)?)?;
    if count == 0 || entsize < sh_size_at + value_width || names_index >= count {
        return Err(MetadataError::Malformed("invalid ELF section table"));
    }
    bounds(
        bytes,
        shoff,
        entsize
            .checked_mul(count)
            .ok_or(MetadataError::Malformed("ELF section count overflow"))?,
    )?;
    let section = |index: usize| -> Result<(usize, usize, usize), MetadataError> {
        let base = shoff
            .checked_add(
                index
                    .checked_mul(entsize)
                    .ok_or(MetadataError::Malformed("ELF section offset overflow"))?,
            )
            .ok_or(MetadataError::Malformed("ELF section offset overflow"))?;
        let name = usize_num(read_num(bytes, base, 4, little)?)?;
        let offset = usize_num(read_num(bytes, base + sh_offset_at, value_width, little)?)?;
        let size = usize_num(read_num(bytes, base + sh_size_at, value_width, little)?)?;
        Ok((name, offset, size))
    };
    let (_, names_at, names_len) = section(names_index)?;
    let names = bounds(bytes, names_at, names_len)?;
    let mut found = Vec::new();
    for index in 0..count {
        let (name_at, offset, size) = section(index)?;
        let tail = names.get(name_at..).ok_or(MetadataError::Malformed(
            "ELF section name outside string table",
        ))?;
        if c_name(tail)? == ELF_SECTION {
            found.push(bounds(bytes, offset, size)?);
        }
    }
    Ok(found)
}

fn pe_sections(bytes: &[u8]) -> Result<Vec<&[u8]>, MetadataError> {
    let pe = usize_num(read_num(bytes, 0x3c, 4, true)?)?;
    if bounds(bytes, pe, 4)? != b"PE\0\0" {
        return Err(MetadataError::Malformed("bad PE signature"));
    }
    let count = usize_num(read_num(bytes, pe + 6, 2, true)?)?;
    let optional = usize_num(read_num(bytes, pe + 20, 2, true)?)?;
    let table = pe
        .checked_add(24)
        .and_then(|v| v.checked_add(optional))
        .ok_or(MetadataError::Malformed("PE section table overflow"))?;
    bounds(
        bytes,
        table,
        count
            .checked_mul(40)
            .ok_or(MetadataError::Malformed("PE section count overflow"))?,
    )?;
    let mut found = Vec::new();
    for index in 0..count {
        let base = table + index * 40;
        if c_name(bounds(bytes, base, 8)?)? == PE_SECTION {
            let size = usize_num(read_num(bytes, base + 16, 4, true)?)?;
            let offset = usize_num(read_num(bytes, base + 20, 4, true)?)?;
            let raw = bounds(bytes, offset, size)?;
            // PE section raw sizes are file-aligned. The canonical record's
            // own length trims only zero alignment padding, then validates.
            if raw.len() < 14 {
                return Err(MetadataError::Malformed("truncated PE metadata section"));
            }
            let record_len = 46usize
                .checked_add(u32::from_le_bytes(raw[10..14].try_into().unwrap()) as usize)
                .ok_or(MetadataError::Malformed("record length overflow"))?;
            let record = bounds(raw, 0, record_len)?;
            let trailing = &raw[record_len..];
            if trailing.starts_with(RECORD_MAGIC) {
                return Err(MetadataError::Duplicate);
            }
            if trailing.iter().any(|byte| *byte != 0) {
                return Err(MetadataError::Malformed(
                    "nonzero bytes after PE metadata record",
                ));
            }
            found.push(record);
        }
    }
    Ok(found)
}

fn is_mach(bytes: &[u8]) -> bool {
    matches!(
        bytes.get(..4),
        Some([0xce, 0xfa, 0xed, 0xfe])
            | Some([0xcf, 0xfa, 0xed, 0xfe])
            | Some([0xca, 0xfe, 0xba, 0xbe])
            | Some([0xca, 0xfe, 0xba, 0xbf])
            | Some([0xbe, 0xba, 0xfe, 0xca])
            | Some([0xbf, 0xba, 0xfe, 0xca])
    )
}

fn is_fat_mach(bytes: &[u8]) -> bool {
    matches!(
        bytes.get(..4),
        Some([0xca, 0xfe, 0xba, 0xbe])
            | Some([0xca, 0xfe, 0xba, 0xbf])
            | Some([0xbe, 0xba, 0xfe, 0xca])
            | Some([0xbf, 0xba, 0xfe, 0xca])
    )
}

fn mach_sections(bytes: &[u8]) -> Result<Vec<&[u8]>, MetadataError> {
    if is_fat_mach(bytes) {
        return fat_mach_sections(bytes);
    }
    let is_64 = bytes.get(..4) == Some(&[0xcf, 0xfa, 0xed, 0xfe]);
    let header = if is_64 { 32 } else { 28 };
    bounds(bytes, 0, header)?;
    let count = usize_num(read_num(bytes, 16, 4, true)?)?;
    let mut command = header;
    let mut found = Vec::new();
    for _ in 0..count {
        let kind = read_num(bytes, command, 4, true)? as u32;
        let size = usize_num(read_num(bytes, command + 4, 4, true)?)?;
        if size < 8 {
            return Err(MetadataError::Malformed("invalid Mach-O load command size"));
        }
        bounds(bytes, command, size)?;
        let segment_kind = if is_64 { 0x19 } else { 0x1 };
        if kind == segment_kind {
            let (
                nsects_at,
                sections_at,
                section_size,
                file_offset_at,
                section_len_at,
                section_len_width,
            ): (usize, usize, usize, usize, usize, usize) = if is_64 {
                (64, 72, 80, 48, 40, 8)
            } else {
                (48, 56, 68, 40, 36, 4)
            };
            let nsects = usize_num(read_num(bytes, command + nsects_at, 4, true)?)?;
            let needed = sections_at
                .checked_add(
                    nsects
                        .checked_mul(section_size)
                        .ok_or(MetadataError::Malformed("Mach-O section count overflow"))?,
                )
                .ok_or(MetadataError::Malformed("Mach-O section count overflow"))?;
            if needed > size {
                return Err(MetadataError::Malformed(
                    "Mach-O sections outside load command",
                ));
            }
            for index in 0..nsects {
                let section = command + sections_at + index * section_size;
                if c_name(bounds(bytes, section, 16)?)? == MACH_SECTION {
                    let offset = usize_num(read_num(bytes, section + file_offset_at, 4, true)?)?;
                    let len = usize_num(read_num(
                        bytes,
                        section + section_len_at,
                        section_len_width,
                        true,
                    )?)?;
                    found.push(bounds(bytes, offset, len)?);
                }
            }
        }
        command = command
            .checked_add(size)
            .ok_or(MetadataError::Malformed("Mach-O load command overflow"))?;
    }
    Ok(found)
}

fn fat_mach_sections(bytes: &[u8]) -> Result<Vec<&[u8]>, MetadataError> {
    let magic = bounds(bytes, 0, 4)?;
    let little = matches!(magic, [0xbe, 0xba, 0xfe, 0xca] | [0xbf, 0xba, 0xfe, 0xca]);
    let is_64 = matches!(magic, [0xca, 0xfe, 0xba, 0xbf] | [0xbf, 0xba, 0xfe, 0xca]);
    let count = usize_num(read_num(bytes, 4, 4, little)?)?;
    if count == 0 || count > 64 {
        return Err(MetadataError::Malformed(
            "invalid universal Mach-O architecture count",
        ));
    }
    let entry_size = if is_64 { 32usize } else { 20usize };
    bounds(
        bytes,
        8,
        count
            .checked_mul(entry_size)
            .ok_or(MetadataError::Malformed("universal Mach-O table overflow"))?,
    )?;
    let mut canonical: Option<&[u8]> = None;
    for index in 0..count {
        let arch = 8 + index * entry_size;
        let width = if is_64 { 8 } else { 4 };
        let offset = usize_num(read_num(bytes, arch + 8, width, little)?)?;
        let size = usize_num(read_num(bytes, arch + 8 + width, width, little)?)?;
        let slice = bounds(bytes, offset, size)?;
        if is_fat_mach(slice) {
            return Err(MetadataError::Malformed("nested universal Mach-O slice"));
        }
        let sections = mach_sections(slice)?;
        if sections.len() != 1 {
            return if sections.is_empty() {
                Err(MetadataError::Missing)
            } else {
                Err(MetadataError::Duplicate)
            };
        }
        match canonical {
            None => canonical = Some(sections[0]),
            Some(record) if record == sections[0] => {}
            Some(_) => return Err(MetadataError::Malformed("universal Mach-O slices disagree")),
        }
    }
    Ok(vec![canonical.unwrap()])
}

fn wasm_leb(bytes: &[u8], at: &mut usize) -> Result<usize, MetadataError> {
    let mut value = 0usize;
    for shift in (0..35).step_by(7) {
        let byte = *bytes
            .get(*at)
            .ok_or(MetadataError::Malformed("truncated Wasm LEB"))?;
        *at += 1;
        value |= ((byte & 0x7f) as usize)
            .checked_shl(shift)
            .ok_or(MetadataError::Malformed("Wasm LEB overflow"))?;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(MetadataError::Malformed("Wasm LEB too long"))
}

fn wasm_sections(bytes: &[u8]) -> Result<Vec<&[u8]>, MetadataError> {
    if bounds(bytes, 0, 8)? != b"\0asm\x01\0\0\0" {
        return Err(MetadataError::Malformed("unsupported Wasm header"));
    }
    let mut at = 8;
    let mut found = Vec::new();
    while at < bytes.len() {
        let id = bytes[at];
        at += 1;
        let size = wasm_leb(bytes, &mut at)?;
        let payload = bounds(bytes, at, size)?;
        at += size;
        if id == 0 {
            let mut name_at = 0;
            let name_len = wasm_leb(payload, &mut name_at)?;
            let name = bounds(payload, name_at, name_len)?;
            name_at += name_len;
            if name == WASM_SECTION.as_bytes() {
                found.push(&payload[name_at..]);
            }
        }
    }
    Ok(found)
}

fn put_wasm_leb(out: &mut Vec<u8>, mut value: usize) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}

/// Add the canonical Wasm custom section before the artifact is published.
pub fn embed_wasm_record(wasm: &mut Vec<u8>, record: &[u8]) -> Result<(), MetadataError> {
    wasm_sections(wasm)?;
    let mut payload = Vec::new();
    put_wasm_leb(&mut payload, WASM_SECTION.len());
    payload.extend_from_slice(WASM_SECTION.as_bytes());
    payload.extend_from_slice(record);
    wasm.push(0);
    put_wasm_leb(wasm, payload.len());
    wasm.extend_from_slice(&payload);
    Ok(())
}

impl CLICommandSchema {
    pub fn standard_completion_words(&self) -> Vec<String> {
        if self.standard {
            let mut words = vec![
                "--verbose".to_string(),
                "-v".to_string(),
                "--quiet".to_string(),
                "-q".to_string(),
                "--color".to_string(),
            ];
            if self.version.is_some() {
                words.push("--version".to_string());
            }
            words
        } else {
            Vec::new()
        }
    }

    fn input_words(inputs: &[CLIInputSchema]) -> Vec<String> {
        let mut words = Vec::new();
        let mut positionals: Vec<&CLIInputSchema> = inputs
            .iter()
            .filter(|input| input.positional.is_some())
            .collect();
        positionals.sort_by_key(|input| input.positional.unwrap());
        for input in positionals {
            words.push(input.flag.clone());
        }
        words.extend(inputs.iter().map(|input| format!("--{}", input.flag)));
        words.extend(
            inputs
                .iter()
                .filter_map(|input| input.short.as_ref().map(|short| format!("-{short}"))),
        );
        words
    }

    /// Inputs declared by a callable member command. Shared root inputs stay
    /// at root completion scope and are not repeated here.
    pub fn command_inputs(&self, command: &CLISubcommandSchema) -> Vec<CLIInputSchema> {
        command.inputs.clone()
    }

    /// Candidates legal after a subcommand is selected.
    pub fn command_completion_words(&self, command: &CLISubcommandSchema) -> Vec<String> {
        let mut words = vec!["--help".to_string()];
        words.extend(Self::input_words(&self.command_inputs(command)));
        words
    }
    /// Job names legal after selecting the `jobs` command.
    pub fn job_completion_words(&self) -> Vec<String> {
        self.jobs
            .iter()
            .filter(|job| job.scope != JobScope::Internal)
            .map(|job| job.name.clone())
            .collect()
    }

    /// Find an argv-reachable job by name. Internal jobs are never exposed.
    pub fn job(&self, name: &str) -> Option<&JobFact> {
        self.jobs
            .iter()
            .find(|job| job.name == name && job.scope != JobScope::Internal)
    }


    /// Candidates legal at the root while a subcommand is still pending.
    pub fn root_completion_words(&self) -> Vec<String> {
        let mut words = vec!["--help".to_string()];
        words.extend(self.standard_completion_words());
        words.extend(Self::input_words(&self.inputs));
        words
    }

    /// Candidates legal before any subcommand is selected.
    pub fn completion_words(&self) -> Vec<String> {
        let mut words = self.root_completion_words();
        for command in &self.commands {
            words.push(command.name.clone());
        }
        words.extend(self.job_completion_words());
        words
    }
}

/// D-SHAPE-ONE1=A: resolve every source field's public spellings once. The
/// checked Shape fact, CLI schema, and later projection callers all consume
/// this same marker fold; no backend derives a second casing or rename rule.
pub fn shape_field_names(structure: &StructDef, field: &Field) -> ShapeFieldNames {
    let rename = marker(&field.serde_markers, Syntax::MARKER_RENAME);
    let style = shape_rename_all_style(structure);
    let shared = rename
        .and_then(|marker| shape_marker_string(marker, None))
        .unwrap_or_else(|| shape_rename_all_name(style, &field.name));
    let mut names = ShapeFieldNames::from_source(&shared);
    if let Some(rename) = rename {
        if let Some(value) = shape_marker_string(rename, Some("json")) {
            names = names.json(value);
        }
        if let Some(value) = shape_marker_string(rename, Some("cbor")) {
            names = names.cbor(value);
        }
        if let Some(value) = shape_marker_string(rename, Some("csv")) {
            names = names.csv(value);
        }
        if let Some(value) = shape_marker_string(rename, Some("toml")) {
            names = names.toml(value);
        }
        if let Some(value) = shape_marker_string(rename, Some("yaml")) {
            names = names.yaml(value);
        }
        if let Some(value) = shape_marker_string(rename, Some("xml")) {
            names = names.xml(value);
        }
        if let Some(value) = shape_marker_string(rename, Some("args")) {
            names = names.args(value);
        }
        if let Some(value) = shape_marker_string(rename, Some("env")) {
            names = names.env(value);
        }
        if let Some(value) = shape_marker_string(rename, Some("db")) {
            names = names.db(value);
        }
        if let Some(value) = shape_marker_string(rename, Some("layout")) {
            names = names.layout(value);
        }
    }
    if let Some(value) = field
        .serde_markers
        .iter()
        .find(|marker| marker.name == Syntax::MARKER_ENV)
        .and_then(|marker| shape_marker_string(marker, None))
    {
        names = names.env(value);
    }
    names
}

fn shape_marker_string(marker: &Marker, label: Option<&str>) -> Option<String> {
    let expression = match label {
        Some(label) => marker
            .arg_labels
            .iter()
            .enumerate()
            .find(|(_, value)| value.as_ref().is_some_and(|(name, _)| name == label))
            .and_then(|(index, _)| marker.args.get(index))
            .and_then(crate::AST::MarkerCallArg::as_expr),
        None => marker.expr_arg(0),
    }?;
    match expression {
        Expr::Str(parts, _) => parts.first().and_then(|part| match part {
            StrPart::Lit(value) => Some(value.clone()),
            StrPart::Interp(..) => None,
        }),
        _ => None,
    }
}

fn shape_rename_all_style(structure: &StructDef) -> Option<&str> {
    structure
        .serde_markers
        .iter()
        .find(|marker| marker.name == Syntax::MARKER_RENAME_ALL)
        .and_then(|marker| marker.expr_arg(0))
        .and_then(|expression| match expression {
            Expr::Ident(name, _) => Some(name.as_str()),
            _ => None,
        })
}

fn shape_rename_all_name(style: Option<&str>, name: &str) -> String {
    match style {
        Some("camel") => Syntax::to_camel_acronym(name),
        Some("kebab") => Syntax::to_snake_acronym(name).replace('_', "-"),
        Some("screaming") => Syntax::to_shouty_acronym(name),
        Some("pascal") => Syntax::to_pascal_acronym(name),
        Some("snake") => Syntax::to_snake_acronym(name),
        _ => name.to_string(),
    }
}

pub fn command_schema(structure: &StructDef) -> Option<CLICommandSchema> {
    command_schema_with_items(&[], structure)
}

/// D-CLI-GLOBAL1=E: project a program struct and its local bound functions
/// into one command schema. The root helper projection passes no item list;
/// it still includes methods, while bundle-facing callers resolve bindings.
pub fn command_schema_with_items(
    items: &[Item],
    structure: &StructDef,
) -> Option<CLICommandSchema> {
    if !structure
        .derives
        .iter()
        .any(|(name, _)| name == Syntax::MARKER_CLI)
    {
        return None;
    }

    let mut positional_order: u16 = 0;
    let inputs = structure
        .fields
        .iter()
        .filter(|field| field.computed.is_none())
        .map(|field| {
            let names = shape_field_names(structure, field);
            let flag = names
                .name_for(ShapeProjectionKind::Args)
                .expect("checked CLI field is missing its Args shape name")
                .to_owned();
            let short = marker(&field.serde_markers, Syntax::MARKER_SHORT).and_then(marker_string);
            let env = marker(&field.serde_markers, Syntax::MARKER_ENV).and_then(marker_string);
            let help = marker(&field.serde_markers, Syntax::MARKER_DOC)
                .and_then(marker_string)
                .unwrap_or_else(|| format!("value for --{flag}"));
            let metavar = flag.replace('-', "_").to_uppercase();
            let flag_only = marker(&field.serde_markers, Syntax::MARKER_FLAG).is_some();
            let shape = match &field.ty {
                Type::Bool => CLIInputShape::Flag,
                Type::Option(inner) => CLIInputShape::Value {
                    kind: scalar_kind(inner)
                        .expect("sema permits only scalar Option fields on a CLI struct"),
                    optional: true,
                    default: None,
                },
                ty => CLIInputShape::Value {
                    kind: scalar_kind(ty).expect("sema permits only scalar fields on a CLI struct"),
                    optional: false,
                    default: field_default(field),
                },
            };
            // D-CLI-POS1=A: required value fields (no default) fill positionally
            // unless #[Flag] opts them out. Bool / optional / defaulted stay flags.
            let positional = match &shape {
                CLIInputShape::Value {
                    optional: false,
                    default: None,
                    ..
                } if !flag_only => {
                    let order = positional_order;
                    positional_order = positional_order.saturating_add(1);
                    Some(order)
                }
                _ => None,
            };
            CLIInputSchema {
                field: field.name.clone(),
                flag,
                short,
                env,
                help,
                metavar: (!matches!(&shape, CLIInputShape::Flag)).then_some(metavar),
                shape,
                positional,
            }
        })
        .collect();

    let computed: std::collections::HashSet<&str> = structure
        .fields
        .iter()
        .filter(|field| field.computed.is_some())
        .map(|field| field.name.as_str())
        .collect();
    let mut commands = structure
        .methods
        .iter()
        .filter(|function| !computed.contains(function.name.as_str()))
        .filter_map(|function| {
            let inputs = function_inputs(function)?;
            Some(CLISubcommandSchema {
                name: function.name.to_lowercase(),
                description: marker(&function.markers, Syntax::MARKER_DOC).and_then(marker_string),
                inputs,
            })
        })
        .collect::<Vec<_>>();
    for binding in &structure.cli_bindings {
        let Some(target) = binding_target_name(binding) else {
            continue;
        };
        let Some(function) = items.iter().find_map(|item| match item {
            Item::Func(function) if function.name == target => Some(function),
            _ => None,
        }) else {
            continue;
        };
        let Some(inputs) = function_inputs_for_binding(function, &structure.name) else {
            continue;
        };
        commands.push(CLISubcommandSchema {
            name: binding.name.to_lowercase(),
            description: marker(&binding.markers, Syntax::MARKER_DOC).and_then(marker_string),
            inputs,
        });
    }

    Some(CLICommandSchema {
        entry_type: structure.name.clone(),
        description: marker(&structure.type_markers, Syntax::MARKER_DOC).and_then(marker_string),
        inputs,
        commands,
        jobs: JobRegistry::from_items(items).into_jobs(),
        standard: cli_standard(&structure.type_markers),
        version: None,
    })
}

fn binding_target_name(binding: &crate::AST::CLICommandBinding) -> Option<&str> {
    match binding.target.without_parens() {
        Expr::Ident(name, _) => Some(name.as_str()),
        _ => None,
    }
}

fn function_inputs_for_binding(function: &Func, shared_type: &str) -> Option<Vec<CLIInputSchema>> {
    let mut command = function.clone();
    command.params = command_payload_params(function, shared_type, true);
    function_inputs(&command)
}

fn function_schema(function: &Func) -> Option<CLICommandSchema> {
    Some(CLICommandSchema {
        entry_type: "run".to_string(),
        description: marker(&function.markers, Syntax::MARKER_DOC).and_then(marker_string),
        inputs: function_inputs(function)?,
        commands: Vec::new(),
        jobs: Vec::new(),
        standard: cli_standard(&function.markers),
        version: None,
    })
}

fn function_inputs(function: &Func) -> Option<Vec<CLIInputSchema>> {
    let mut positional_order = 0u16;
    function
        .params
        .iter()
        .filter(|param| param.name != Syntax::KW_SELF)
        .map(|param| {
            let flag = param.call_label().replace('_', "-");
            let shape = match &param.ty {
                Type::Bool => CLIInputShape::Flag,
                Type::Option(inner) => CLIInputShape::Value {
                    kind: scalar_kind(inner)?,
                    optional: true,
                    default: None,
                },
                ty => {
                    let default = param.default.as_deref().and_then(expr_default);
                    if param.default.is_some() && default.is_none() {
                        return None;
                    }
                    CLIInputShape::Value {
                        kind: scalar_kind(ty)?,
                        optional: false,
                        default,
                    }
                }
            };
            let positional = match &shape {
                CLIInputShape::Value {
                    optional: false,
                    default: None,
                    ..
                } => {
                    let order = positional_order;
                    positional_order = positional_order.saturating_add(1);
                    Some(order)
                }
                _ => None,
            };
            Some(CLIInputSchema {
                field: param.name.clone(),
                flag,
                short: None,
                env: None,
                help: format!("value for --{}", param.call_label().replace('_', "-")),
                metavar: (!matches!(&shape, CLIInputShape::Flag))
                    .then(|| param.call_label().replace('-', "_").to_uppercase()),
                shape,
                positional,
            })
        })
        .collect()
}

fn expr_default(expr: &Expr) -> Option<CLIDefault> {
    match expr.without_parens() {
        Expr::Int(value, _, _, _) => Some(CLIDefault::Value(CtValue::Int(*value))),
        Expr::Float(value, _, is_f32, _) => Some(CLIDefault::Value(CtValue::Float(if *is_f32 {
            CtFloat::f32(*value as f32)
        } else {
            CtFloat::f64(*value)
        }))),
        Expr::Bool(value, _) => Some(CLIDefault::Value(CtValue::Bool(*value))),
        Expr::Str(parts, _) if parts.len() == 1 => match &parts[0] {
            StrPart::Lit(value) => Some(CLIDefault::Value(CtValue::Str(value.clone()))),
            _ => None,
        },
        _ => None,
    }
}

fn cli_standard(markers: &[Marker]) -> bool {
    marker(markers, Syntax::MARKER_CLI).is_some_and(|marker| {
        marker
            .expr_args()
            .any(|arg| matches!(arg, Expr::Ident(name, _) if name == "Standard"))
    })
}

fn scalar_kind(ty: &Type) -> Option<CLIValueKind> {
    match ty {
        Type::Bool => Some(CLIValueKind::Bool),
        Type::Int | Type::InlineRange { .. } => Some(CLIValueKind::Int),
        Type::Float => Some(CLIValueKind::Float),
        Type::String => Some(CLIValueKind::String),
        Type::Named(name) if name == "Path" => Some(CLIValueKind::Path),
        _ => None,
    }
}

fn marker<'a>(markers: &'a [Marker], name: &str) -> Option<&'a Marker> {
    markers.iter().find(|marker| marker.name == name)
}

fn marker_string(marker: &Marker) -> Option<String> {
    match marker.expr_arg(0) {
        Some(Expr::Str(parts, _)) if parts.len() == 1 => match &parts[0] {
            StrPart::Lit(value) => Some(value.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn field_default(field: &Field) -> Option<CLIDefault> {
    if let Some(value) = &field.default_ct {
        return Some(CLIDefault::Value(value.clone()));
    }
    if field.default.is_some() {
        return Some(CLIDefault::TypeDefault);
    }
    let marker = marker(&field.serde_markers, Syntax::MARKER_DEFAULT)?;
    Some(match (&marker.args[..], &marker.ct) {
        ([_, ..], Some(value)) => CLIDefault::Value(value.clone()),
        _ => CLIDefault::TypeDefault,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn schema() -> CLICommandSchema {
        CLICommandSchema {
            entry_type: "Options".to_string(),
            description: None,
            inputs: vec![CLIInputSchema {
                field: "output_file".to_string(),
                flag: "output-file".to_string(),
                short: Some("o".to_string()),
                env: Some("OUTPUT_FILE".to_string()),
                help: "destination".to_string(),
                metavar: Some("OUTPUT_FILE".to_string()),
                shape: CLIInputShape::Value {
                    kind: CLIValueKind::Path,
                    optional: false,
                    default: None,
                },
                positional: Some(0),
            }],
            commands: Vec::new(),
            jobs: Vec::new(),
            standard: false,
            version: None,
        }
    }

    fn elf(record: &[u8]) -> Vec<u8> {
        let names = b"\0.shstrtab\0.jet_command\0";
        let names_at = 64usize;
        let record_at = names_at + names.len();
        let sections_at = record_at + record.len();
        let mut bytes = vec![0u8; sections_at + 3 * 64];
        bytes[..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[40..48].copy_from_slice(&(sections_at as u64).to_le_bytes());
        bytes[58..60].copy_from_slice(&64u16.to_le_bytes());
        bytes[60..62].copy_from_slice(&3u16.to_le_bytes());
        bytes[62..64].copy_from_slice(&1u16.to_le_bytes());
        bytes[names_at..record_at].copy_from_slice(names);
        bytes[record_at..sections_at].copy_from_slice(record);
        let strings = sections_at + 64;
        bytes[strings..strings + 4].copy_from_slice(&1u32.to_le_bytes());
        bytes[strings + 24..strings + 32].copy_from_slice(&(names_at as u64).to_le_bytes());
        bytes[strings + 32..strings + 40].copy_from_slice(&(names.len() as u64).to_le_bytes());
        let metadata = sections_at + 128;
        bytes[metadata..metadata + 4].copy_from_slice(&11u32.to_le_bytes());
        bytes[metadata + 24..metadata + 32].copy_from_slice(&(record_at as u64).to_le_bytes());
        bytes[metadata + 32..metadata + 40].copy_from_slice(&(record.len() as u64).to_le_bytes());
        bytes
    }

    fn pe(record: &[u8], duplicate: bool) -> Vec<u8> {
        let pe = 0x80usize;
        let count = if duplicate { 2usize } else { 1usize };
        let table = pe + 24;
        let raw_at = table + count * 40;
        let mut bytes = vec![0u8; raw_at + count * record.len()];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&(pe as u32).to_le_bytes());
        bytes[pe..pe + 4].copy_from_slice(b"PE\0\0");
        bytes[pe + 6..pe + 8].copy_from_slice(&(count as u16).to_le_bytes());
        for index in 0..count {
            let section = table + index * 40;
            bytes[section..section + 7].copy_from_slice(b".jetcmd");
            bytes[section + 16..section + 20].copy_from_slice(&(record.len() as u32).to_le_bytes());
            let offset = raw_at + index * record.len();
            bytes[section + 20..section + 24].copy_from_slice(&(offset as u32).to_le_bytes());
            bytes[offset..offset + record.len()].copy_from_slice(record);
        }
        bytes
    }

    fn pe_with_trailing(record: &[u8], trailing: &[u8]) -> Vec<u8> {
        let mut bytes = pe(record, false);
        let table = 0x80 + 24;
        bytes[table + 16..table + 20]
            .copy_from_slice(&((record.len() + trailing.len()) as u32).to_le_bytes());
        bytes.extend_from_slice(trailing);
        bytes
    }

    fn mach(record: &[u8]) -> Vec<u8> {
        let command_size = 72 + 80;
        let record_at = 32 + command_size;
        let mut bytes = vec![0u8; record_at + record.len()];
        bytes[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&(command_size as u32).to_le_bytes());
        bytes[32..36].copy_from_slice(&0x19u32.to_le_bytes());
        bytes[36..40].copy_from_slice(&(command_size as u32).to_le_bytes());
        bytes[96..100].copy_from_slice(&1u32.to_le_bytes());
        let section = 104usize;
        bytes[section..section + 8].copy_from_slice(b"__jetcmd");
        bytes[section + 40..section + 48].copy_from_slice(&(record.len() as u64).to_le_bytes());
        bytes[section + 48..section + 52].copy_from_slice(&(record_at as u32).to_le_bytes());
        bytes[record_at..].copy_from_slice(record);
        bytes
    }

    fn fat_mach(record: &[u8]) -> Vec<u8> {
        let first = mach(record);
        let second = mach(record);
        let table_end = 8 + 2 * 20;
        let second_at = table_end + first.len();
        let mut bytes = vec![0u8; second_at + second.len()];
        bytes[..4].copy_from_slice(&[0xca, 0xfe, 0xba, 0xbe]);
        bytes[4..8].copy_from_slice(&2u32.to_be_bytes());
        bytes[16..20].copy_from_slice(&(table_end as u32).to_be_bytes());
        bytes[20..24].copy_from_slice(&(first.len() as u32).to_be_bytes());
        bytes[36..40].copy_from_slice(&(second_at as u32).to_be_bytes());
        bytes[40..44].copy_from_slice(&(second.len() as u32).to_be_bytes());
        bytes[table_end..second_at].copy_from_slice(&first);
        bytes[second_at..].copy_from_slice(&second);
        bytes
    }

    fn nested_fat_mach(record: &[u8]) -> Vec<u8> {
        let inner = fat_mach(record);
        let table_end = 8 + 20;
        let mut bytes = vec![0u8; table_end + inner.len()];
        bytes[..4].copy_from_slice(&[0xca, 0xfe, 0xba, 0xbe]);
        bytes[4..8].copy_from_slice(&1u32.to_be_bytes());
        bytes[16..20].copy_from_slice(&(table_end as u32).to_be_bytes());
        bytes[20..24].copy_from_slice(&(inner.len() as u32).to_be_bytes());
        bytes[table_end..].copy_from_slice(&inner);
        bytes
    }

    #[test]
    fn canonical_record_round_trips() {
        let schema = schema();
        assert_eq!(decode_record(&encode_record(&schema)).unwrap(), schema);
    }

    #[test]
    fn reads_cross_format_section_fixtures() {
        let schema = schema();
        let record = encode_record(&schema);
        assert_eq!(read_executable(&elf(&record)).unwrap(), schema);
        assert_eq!(read_executable(&pe(&record, false)).unwrap(), schema);
        assert_eq!(read_executable(&mach(&record)).unwrap(), schema);
        assert_eq!(read_executable(&fat_mach(&record)).unwrap(), schema);
        let mut wasm = b"\0asm\x01\0\0\0".to_vec();
        embed_wasm_record(&mut wasm, &record).unwrap();
        assert_eq!(read_executable(&wasm).unwrap(), schema);
    }

    #[test]
    fn hostile_records_fail_closed() {
        let record = encode_record(&schema());
        assert_eq!(
            read_executable(b"not executable"),
            Err(MetadataError::UnknownFormat)
        );
        assert_eq!(
            read_executable(b"\0asm\x01\0\0\0"),
            Err(MetadataError::Missing)
        );
        assert_eq!(
            read_executable(&pe(&record, true)),
            Err(MetadataError::Duplicate)
        );
        assert_eq!(
            read_executable(&pe_with_trailing(&record, &record)),
            Err(MetadataError::Duplicate)
        );
        assert_eq!(
            read_executable(&pe_with_trailing(&record, &[0, 7])),
            Err(MetadataError::Malformed(
                "nonzero bytes after PE metadata record"
            ))
        );
        assert_eq!(
            read_executable(&pe_with_trailing(&record, &[0; 16])).unwrap(),
            schema()
        );
        assert_eq!(
            read_executable(&nested_fat_mach(&record)),
            Err(MetadataError::Malformed("nested universal Mach-O slice"))
        );

        let mut unsupported = record.clone();
        unsupported[8..10].copy_from_slice(&6u16.to_le_bytes());
        assert_eq!(
            decode_record(&unsupported),
            Err(MetadataError::UnsupportedVersion(6))
        );
        let mut corrupt = record;
        *corrupt.last_mut().unwrap() ^= 1;
        assert_eq!(
            decode_record(&corrupt),
            Err(MetadataError::Malformed("digest mismatch"))
        );
    }
}
