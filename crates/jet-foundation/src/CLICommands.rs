//! D-JET-VERBS1=A: shared command registry for CLI rendering and job-name checks.
pub const JOBS_COMMAND: &str = "jobs";
pub const JOB_INVOCATION_HELP: &str = "\nRun a project job: jet [jet flags] <name> [job args...]\nJet flags go before the job name; every word after it belongs to the job.\n";

pub fn is_command_name(name: &str) -> bool {
    COMMANDS.iter().any(|command| command.name == name)
}

/// One subcommand of `jet` — a bare leaf (`build`, `run`, …) or a namespaced
/// group (`registry`, …) that owns nested actions. #1659 criterion 1
/// (round 2): this is the ONE declaration dispatch, help, man, and completions
/// all render from — a group is a row here with non-empty `actions`, not a
/// second parallel table.
pub struct CommandSpec {
    pub name: &'static str,
    /// One-line summary (man + completion description).
    pub summary: &'static str,
    /// Whether this is one of the "commands that matter" shown when argv is
    /// flags-only with no subcommand (bare `jet` starts the REPL instead).
    pub headline: bool,
    /// Position in `jet help`'s default order, 1 = most used. Measured and
    /// written by `node Tools/cli-census/census.mjs`; never edit by hand.
    pub frequency_rank: u16,
    /// Nested actions when this is a namespaced group (`registry`, …); empty
    /// for a bare leaf command.
    pub actions: &'static [NestedCommandSpec],
    /// Only meaningful when `actions` is non-empty: true when every subword of
    /// this group is modeled by `actions` — the bare group form and `<group>
    /// help` print the CLI-owned summary, and an unmodeled subword is E2101.
    /// False for `os`/`gc`/`env` (D-CLI-SURFACE3=B): these front doors expose a
    /// wider downstream verb surface
    /// surface (`check`/`build`/`switch`/…) that stays opaque to this
    /// registry, so only the migrated verbs (`push`/`bridge`/`services`/
    /// `config`) are modeled and everything else — including bare `jet os` and
    /// `jet os help` — falls through to the real `jet os` dispatcher unchanged.
    pub exhaustive: bool,
    /// Positional argument shape for a flat command, e.g. `repl [<file.jet>]`
    /// — the same role `NestedCommandSpec::usage` plays for a nested action,
    /// so `command_usage` has one field to read instead of a second table
    /// (#2072). `None` keeps the generic `[args]` shape. Flags are NOT
    /// repeated here: `flags_for_command` already renders them from `FLAGS`.
    pub usage: Option<&'static str>,
}

impl CommandSpec {
    /// True when this row is a namespaced group with nested actions, rather
    /// than a bare leaf command.
    pub const fn is_group(&self) -> bool {
        !self.actions.is_empty()
    }
}

/// One canonical nested spelling and the real legacy dispatcher seam it reaches.
/// `HandlerKey::SharedStore` keeps the group because that handler consumes the
/// group word itself (`shared-store status`, …).
pub struct NestedCommandSpec {
    pub name: &'static str,
    /// Canonical spelling after the group name. Multiple forms use one line each.
    pub usage: &'static str,
    pub summary: &'static str,
    pub handler: HandlerKey,
    /// True when this spelling is also canonical as a top-level command.
    pub also_canonical_top_level: bool,
}
/// The nine canonical views under `jet inspect`.  The enum is the parser's
/// source of truth; the help, completion, and man rows below are generated
/// from the same values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectPlane {
    Types,
    Rights,
    Claims,
    Shapes,
    Accel,
    Decisions,
    Structure,
    Build,
    Gates,
}

impl InspectPlane {
    pub const ALL: [Self; 9] = [
        Self::Types,
        Self::Rights,
        Self::Claims,
        Self::Shapes,
        Self::Accel,
        Self::Decisions,
        Self::Structure,
        Self::Build,
        Self::Gates,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Types => "types",
            Self::Rights => "rights",
            Self::Claims => "claims",
            Self::Shapes => "shapes",
            Self::Accel => "accel",
            Self::Decisions => "decisions",
            Self::Structure => "structure",
            Self::Build => "build",
            Self::Gates => "gates",
        }
    }

    pub const fn usage(self) -> &'static str {
        match self {
            Self::Types => "types [TARGET] [--live PID | --replay ARTIFACT]",
            Self::Rights => "rights [TARGET] [--live PID | --replay ARTIFACT]",
            Self::Claims => "claims [TARGET] [--live PID | --replay ARTIFACT]",
            Self::Shapes => "shapes [TARGET] [--live PID | --replay ARTIFACT]",
            Self::Accel => "accel [TARGET] [--live PID | --replay ARTIFACT]",
            Self::Decisions => "decisions [TARGET] [--live PID | --replay ARTIFACT]",
            Self::Structure => "structure [TARGET] [--core] [--live PID | --replay ARTIFACT]",
            Self::Build => "build [TARGET] [--coverage] [--live PID | --replay ARTIFACT]",
            Self::Gates => "gates [TARGET] [--live PID | --replay ARTIFACT]",
        }
    }

    pub const fn summary(self) -> &'static str {
        match self {
            Self::Types => "Show registered type and operator facts",
            Self::Rights => "Show required and granted rights",
            Self::Claims => "Show receipt-backed claims",
            Self::Shapes => "Show semantic shapes and projections",
            Self::Accel => "Show vector proof and acceleration gate decisions",
            Self::Decisions => "Show compiler decisions and their evidence",
            Self::Structure => "Show structure and lifecycle facts",
            Self::Build => "Show build identity, provenance, and coverage",
            Self::Gates => "Show the complete compile-time gate ledger",
        }
    }
}
impl InspectPlane {
    fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|plane| plane.name() == name)
    }
}
pub fn inspect_plane(name: &str) -> Option<InspectPlane> {
    InspectPlane::parse(name)
}

/// Scope selected by the one inspect grammar.  `Target` accepts either a
/// source file or a package directory; the resolver decides which one it is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InspectScope {
    Package,
    Target(String),
    Live(u32),
    Replay(String),
}

/// Fully parsed `jet inspect <plane>` request.  `options` retains registered
/// plane flags for the projection backend without making the parser duplicate
/// each plane's option vocabulary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InspectRequest {
    pub plane: InspectPlane,
    pub scope: InspectScope,
    pub options: Vec<String>,
}

/// Parse the canonical inspect grammar from the unnormalized argv.
///
/// Plane names come from `INSPECT_ACTIONS`; scope selection is exclusive and
/// deterministic.  Other registered flags stay in `options` for the selected
/// projection and are validated by the shared CLI flag checker.
pub fn parse_inspect_args(args: &[String]) -> Result<InspectRequest, String> {
    if args.first().map(String::as_str) != Some("inspect") {
        return Err("inspect parser expects `jet inspect <plane>`".to_string());
    }
    let plane_name = args.get(1).map(String::as_str).ok_or_else(|| {
        "missing inspect plane; choose types, rights, claims, shapes, accel, decisions, structure, build, or gates"
            .to_string()
    })?;
    let plane = InspectPlane::parse(plane_name).ok_or_else(|| {
        format!(
            "unknown inspect plane `{plane_name}`; choose types, rights, claims, shapes, accel, decisions, structure, build, or gates"
        )
    })?;

    let mut target = None;
    let mut live = None;
    let mut replay = None;
    let mut options = Vec::new();
    let mut index = 2;
    while index < args.len() {
        let argument = args[index].as_str();
        if argument == "--live" {
            let value = args.get(index + 1).map(String::as_str).ok_or_else(|| {
                "`--live` needs a process id".to_string()
            })?;
            live = Some(parse_inspect_pid(value)?);
            options.push("--live".to_string());
            options.push(value.to_string());
            index += 2;
            continue;
        }
        if let Some(value) = argument.strip_prefix("--live=") {
            live = Some(parse_inspect_pid(value)?);
            options.push(argument.to_string());
            index += 1;
            continue;
        }
        if argument == "--replay" {
            let value = args.get(index + 1).map(String::as_str).ok_or_else(|| {
                "`--replay` needs a replay artifact path".to_string()
            })?;
            if value.starts_with('-') {
                return Err("`--replay` needs a replay artifact path".to_string());
            }
            replay = Some(value.to_string());
            options.push("--replay".to_string());
            options.push(value.to_string());
            index += 2;
            continue;
        }
        if let Some(value) = argument.strip_prefix("--replay=") {
            if value.is_empty() {
                return Err("`--replay` needs a replay artifact path".to_string());
            }
            replay = Some(value.to_string());
            options.push(argument.to_string());
            index += 1;
            continue;
        }
        if argument == "--" {
            return Err("inspect planes do not accept a `--` command tail".to_string());
        }
        if argument.starts_with('-') {
            options.push(argument.to_string());
            if inspect_flag_takes_value(argument) {
                if let Some(value) = args.get(index + 1) {
                    if !value.starts_with('-') {
                        options.push(value.clone());
                        index += 2;
                        continue;
                    }
                }
            }
            index += 1;
            continue;
        }
        if target.replace(argument.to_string()).is_some() {
            return Err("inspect accepts at most one file or package target".to_string());
        }
        index += 1;
    }

    if live.is_some() && replay.is_some() {
        return Err("inspect accepts at most one of `--live PID` or `--replay ARTIFACT`".to_string());
    }
    if target.is_some() && (live.is_some() || replay.is_some()) {
        return Err("inspect accepts a target, `--live PID`, or `--replay ARTIFACT` as one scope".to_string());
    }
    let scope = match (target, live, replay) {
        (Some(target), None, None) => InspectScope::Target(target),
        (None, Some(pid), None) => InspectScope::Live(pid),
        (None, None, Some(artifact)) => InspectScope::Replay(artifact),
        (None, None, None) => InspectScope::Package,
        _ => unreachable!("inspect scope exclusivity is checked above"),
    };
    Ok(InspectRequest {
        plane,
        scope,
        options,
    })
}

fn parse_inspect_pid(value: &str) -> Result<u32, String> {
    let pid = value
        .parse::<u32>()
        .map_err(|_| format!("`--live` expects a positive process id, got `{value}`"))?;
    if pid == 0 {
        return Err("`--live` expects a positive process id".to_string());
    }
    Ok(pid)
}

fn inspect_flag_takes_value(argument: &str) -> bool {
    let name = argument.split('=').next().unwrap_or(argument);
    matches!(
        name,
        "--before"
            | "--color"
            | "--facts"
            | "--gate"
            | "--kind"
            | "--output"
            | "--profile"
            | "--scope"
            | "--set"
            | "--target"
            | "--topic"
    )
}

const fn inspect_plane_action(plane: InspectPlane) -> NestedCommandSpec {
    NestedCommandSpec {
        name: plane.name(),
        usage: plane.usage(),
        summary: plane.summary(),
        handler: HandlerKey::InspectPlane,
        also_canonical_top_level: false,
    }
}


/// Closed set of real dispatcher seams reachable from nested commands.
/// Adding a handler requires updating this exhaustive mapping and its route test.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandlerKey {
    Publish,
    Yank,
    Keygen,
    Key,
    Vendor,
    Graph,
    Query,
    ExplainBuild,
    Compiler,
    Impact,
    Provenance,
    Digest,
    InspectPlane,
    Schema,
    Semindex,
    Output,
    Expand,
    Codemod,
    Audit,
    Sbom,
    Bind,
    Logs,
    Info,
    Outdated,
    GcReport,
    ProjectParts,
    Push,
    Bridge,
    Services,
    Config,
    Toolchain,
    SelfUpdate,
    Doctor,
    Completions,
    Man,
    Devtools,
    Lsp,
    Exec,
    Env,
    SharedStore,
    Cache,
    Perf,
    Reserved,
    Db,
}

impl HandlerKey {
    pub const fn dispatch_word(self) -> &'static str {
        match self {
            Self::Publish => "publish",
            Self::Yank => "yank",
            Self::Keygen => "keygen",
            Self::Key => "key",
            Self::Vendor => "vendor",
            Self::Graph => "graph",
            Self::Query => "query",
            Self::ExplainBuild => "explain-build",
            Self::Compiler => "compiler",
            Self::Impact => "impact",
            Self::Provenance => "provenance",
            Self::Digest => "digest",
            Self::Env => "env",
            Self::Semindex => "semindex",
            Self::Output => "output",
            Self::Expand => "expand",
            Self::InspectPlane => "inspect",
            Self::Schema => "schema",
            Self::Codemod => "codemod",
            Self::Audit => "audit",
            Self::Sbom => "sbom",
            Self::Bind => "bind",
            Self::Logs => "logs",
            Self::Info => "info",
            Self::Outdated => "outdated",
            Self::GcReport => "gc",
            Self::ProjectParts => "parts",
            Self::Push => "push",
            Self::Bridge => "bridge",
            Self::Services => "services",
            Self::Config => "config",
            Self::Toolchain => "toolchain",
            Self::SelfUpdate => "self-update",
            Self::Doctor => "doctor",
            Self::Completions => "completions",
            Self::Man => "man",
            Self::Devtools => "devtools",
            Self::Lsp => "lsp",
            Self::Exec => "exec",
            Self::SharedStore => "shared-store",
            Self::Cache => "cache",
            Self::Perf => "perf",
            Self::Reserved => "reserved",
            Self::Db => "db",
        }
    }

    pub const fn keeps_group(self) -> bool {
        matches!(
            self,
            Self::InspectPlane
                | Self::GcReport
                | Self::Perf
                | Self::Env
                | Self::SharedStore
                | Self::Cache
                | Self::Db
        )
    }
}

const REGISTRY_ACTIONS: &[NestedCommandSpec] = &[
    NestedCommandSpec {
        name: "publish",
        usage: "publish --to <registry> [--no-sign]",
        summary: "Publish the current package",
        handler: HandlerKey::Publish,
        also_canonical_top_level: false,
    },
    NestedCommandSpec {
        name: "yank",
        usage: "yank <version> [--message <reason>]",
        summary: "Stop new installs of a published version",
        handler: HandlerKey::Yank,
        also_canonical_top_level: false,
    },
    NestedCommandSpec {
        name: "keygen",
        usage: "keygen [--registry <name>] [--force]",
        summary: "Create a package-signing key",
        handler: HandlerKey::Keygen,
        also_canonical_top_level: false,
    },
    NestedCommandSpec {
        name: "key",
        usage: "key backup [<dest>] [--registry <name>]",
        summary: "Manage the package-signing key",
        handler: HandlerKey::Key,
        also_canonical_top_level: false,
    },
    NestedCommandSpec {
        name: "vendor",
        usage: "vendor [--vendor-dir <path>]",
        summary: "Copy dependencies into vendor/",
        handler: HandlerKey::Vendor,
        also_canonical_top_level: false,
    },
];
const DB_ACTIONS: &[NestedCommandSpec] = &[NestedCommandSpec {
    name: "migrate",
    usage: "migrate new <Name> --up <SQL> [--down <SQL>] [--lock shared|exclusive] [--risk <note>]\nmigrate preview --target <name> [--to <version>|--step <count>]\nmigrate status --target <name>\nmigrate target --target <name> [--database <relative-path>] [--lock shared|exclusive]\nmigrate apply --target <name> [--to <version>|--step <count>] [--lock exclusive]\nmigrate step --target <name> <count> [--lock exclusive]\nmigrate rollback --target <name> [<version>|--step <count>] [--lock exclusive]\nmigrate resume --target <name>",
    summary: "Create, preview, inspect, apply, resume, and roll back versioned database migrations",
    handler: HandlerKey::Db,
    also_canonical_top_level: false,
}];
const INSPECT_ACTIONS: &[NestedCommandSpec] = &[
    inspect_plane_action(InspectPlane::Types),
    inspect_plane_action(InspectPlane::Rights),
    inspect_plane_action(InspectPlane::Claims),
    inspect_plane_action(InspectPlane::Shapes),
    inspect_plane_action(InspectPlane::Accel),
    inspect_plane_action(InspectPlane::Decisions),
    inspect_plane_action(InspectPlane::Structure),
    inspect_plane_action(InspectPlane::Build),
    inspect_plane_action(InspectPlane::Gates),
    NestedCommandSpec { name: "graph", usage: "graph <file.jet>", summary: "Show the build graph", handler: HandlerKey::Graph, also_canonical_top_level: false },
    NestedCommandSpec { name: "query", usage: "query build <file.jet>", summary: "Search code and build information", handler: HandlerKey::Query, also_canonical_top_level: false },
    NestedCommandSpec { name: "explain-build", usage: "explain-build <target|action|file> <file.jet>", summary: "Explain why a target, action, or file is rebuilt", handler: HandlerKey::ExplainBuild, also_canonical_top_level: false },
    NestedCommandSpec { name: "compiler", usage: "compiler <lex|parse|check|source-map> <file>", summary: "Read compiler facts as versioned JSON", handler: HandlerKey::Compiler, also_canonical_top_level: false },
    NestedCommandSpec { name: "impact", usage: "impact <file.jet> <symbol>", summary: "Show code affected by a symbol", handler: HandlerKey::Impact, also_canonical_top_level: false },
    NestedCommandSpec { name: "provenance", usage: "provenance [--json] [<dependency>]", summary: "Read dependency provenance", handler: HandlerKey::Provenance, also_canonical_top_level: false },
    NestedCommandSpec { name: "digest", usage: "digest [--json] [--list-topics] [--topic <name>]", summary: "Write the one-file LLM surface digest", handler: HandlerKey::Digest, also_canonical_top_level: false },
    NestedCommandSpec { name: "env", usage: "env [--json] [<env.jet|config.jet>]", summary: "List typed environment reads in a config surface", handler: HandlerKey::Env, also_canonical_top_level: false },
    NestedCommandSpec { name: "semindex", usage: "semindex <file.jet>", summary: "Search the code index", handler: HandlerKey::Semindex, also_canonical_top_level: false },
    NestedCommandSpec { name: "output", usage: "output <file.jet> [<address>]", summary: "Inspect one selected Output", handler: HandlerKey::Output, also_canonical_top_level: false },
    NestedCommandSpec { name: "expand", usage: "expand [--facts <inline|memory|web|effects|layout|derive|templates|callable-signature>] [--json] <file.jet>", summary: "Show expanded meaning of Jet code (use --json for canonical facts)", handler: HandlerKey::Expand, also_canonical_top_level: false },
    NestedCommandSpec { name: "schema", usage: "schema status\nschema squash --before <version>", summary: "Inspect saved data schema versions", handler: HandlerKey::Schema, also_canonical_top_level: false },
    NestedCommandSpec { name: "codemod", usage: "codemod <plan.json> --dry-run\ncodemod apply <plan.json> [--yes]\ncodemod undo <log.json>", summary: "Preview or apply code changes", handler: HandlerKey::Codemod, also_canonical_top_level: false },
    NestedCommandSpec { name: "audit", usage: "audit copies [--json] [<entry.jet>]\naudit memory [--json]\naudit [--advisory-db <path>]", summary: "Inspect implicit copies, exercised memory witnesses, or dependencies", handler: HandlerKey::Audit, also_canonical_top_level: true },
    NestedCommandSpec { name: "sbom", usage: "sbom [--cyclonedx]", summary: "Create a software bill of materials", handler: HandlerKey::Sbom, also_canonical_top_level: false },
    NestedCommandSpec { name: "bind", usage: "bind <header.h> --pkg <lib>\nbind cpp <header.hpp> --target <triple> --clang <path> --ar <path>", summary: "Generate Jet bindings from a foreign header", handler: HandlerKey::Bind, also_canonical_top_level: false },
    NestedCommandSpec { name: "logs", usage: "logs <pkg>", summary: "Show recent package build logs", handler: HandlerKey::Logs, also_canonical_top_level: false },
    NestedCommandSpec { name: "info", usage: "info <source>.<package>", summary: "Show package details", handler: HandlerKey::Info, also_canonical_top_level: false },
    NestedCommandSpec { name: "outdated", usage: "outdated", summary: "List dependencies with available updates", handler: HandlerKey::Outdated, also_canonical_top_level: false },
    // #1659 criterion 5: reserved words and sigils, including the five
    // teaching-reserved words (copy/mut/take/const/unsafe) that reject valid
    // identifiers with a redirect to their current spelling.
    NestedCommandSpec { name: "reserved", usage: "reserved [--json]", summary: "List reserved words and sigils", handler: HandlerKey::Reserved, also_canonical_top_level: false },
];
const GC_ACTIONS: &[NestedCommandSpec] = &[NestedCommandSpec {
    name: "report",
    usage: "report",
    summary: "Show values moved into automatic memory management",
    handler: HandlerKey::GcReport,
    also_canonical_top_level: true,
}];
const PROJECT_ACTIONS: &[NestedCommandSpec] = &[NestedCommandSpec {
    name: "parts",
    usage: "parts",
    summary: "List loaded and skipped project modules",
    handler: HandlerKey::ProjectParts,
    also_canonical_top_level: false,
}];
const SELF_ACTIONS: &[NestedCommandSpec] = &[
    NestedCommandSpec { name: "toolchain", usage: "toolchain", summary: "Show the Jet version selected for this project", handler: HandlerKey::Toolchain, also_canonical_top_level: false },
    NestedCommandSpec { name: "update", usage: "update [--endpoint <url>] [--channel <name>] [--platform <target>] [--trust-key <file>] [--dry-run] [--apply] [--allow-unofficial]", summary: "Verify or install a signed Jet toolchain release", handler: HandlerKey::SelfUpdate, also_canonical_top_level: false },
    NestedCommandSpec { name: "doctor", usage: "doctor", summary: "Find and fix toolchain problems", handler: HandlerKey::Doctor, also_canonical_top_level: false },
    NestedCommandSpec { name: "completions", usage: "completions", summary: "Print shell completions", handler: HandlerKey::Completions, also_canonical_top_level: false },
    NestedCommandSpec { name: "man", usage: "man", summary: "Print the Jet manual", handler: HandlerKey::Man, also_canonical_top_level: false },
    NestedCommandSpec { name: "devtools", usage: "devtools", summary: "Run Jet maintenance tools", handler: HandlerKey::Devtools, also_canonical_top_level: false },
    NestedCommandSpec { name: "lsp", usage: "lsp", summary: "Start the language server", handler: HandlerKey::Lsp, also_canonical_top_level: false },
    NestedCommandSpec { name: "exec", usage: "exec --workspace <dir> [--exec <path>] [--read <path>] [--write <path>] -- <program> [args]", summary: "Execute one command in an authority-bound workspace", handler: HandlerKey::Exec, also_canonical_top_level: false },
];
const CACHE_ACTIONS: &[NestedCommandSpec] = &[
    NestedCommandSpec {
        name: "status",
        usage: "status",
        summary: "Show machine-wide artifact store usage",
        handler: HandlerKey::Cache,
        also_canonical_top_level: true,
    },
    NestedCommandSpec {
        name: "prune",
        usage: "prune --to <size>",
        summary: "Prune the artifact store to a target size",
        handler: HandlerKey::Cache,
        also_canonical_top_level: false,
    },
    NestedCommandSpec {
        name: "limit",
        usage: "limit --host <size>",
        summary: "Set the host-wide artifact store limit",
        handler: HandlerKey::Cache,
        also_canonical_top_level: false,
    },
];
// D-ENVHOOK1=A / D-ENV-FILES1=A / D-ENV-PROFILE1=C: these are the shipped
// `jetpack env` subverbs exposed through Jet's environment front door. `env`
// stays non-exhaustive because `export` is a private callback used by the shell
// hook and must continue downstream.
const ENV_ACTIONS: &[NestedCommandSpec] = &[
    NestedCommandSpec {
        name: "test",
        usage: "test [-- command]",
        summary: "Run environment checks in a clean environment",
        handler: HandlerKey::Env,
        also_canonical_top_level: true,
    },
    NestedCommandSpec {
        name: "hook",
        usage: "hook <bash|zsh|fish>",
        summary: "Print the shell auto-activation hook",
        handler: HandlerKey::Env,
        also_canonical_top_level: false,
    },
    NestedCommandSpec {
        name: "sync",
        usage: "sync",
        summary: "Apply typed managed environment files",
        handler: HandlerKey::Env,
        also_canonical_top_level: false,
    },
    NestedCommandSpec {
        name: "info",
        usage: "info",
        summary: "Show the typed environment plan",
        handler: HandlerKey::Env,
        also_canonical_top_level: false,
    },
];
const SHARED_STORE_ACTIONS: &[NestedCommandSpec] = &[
    NestedCommandSpec {
        name: "install",
        usage: "install",
        summary: "Install the optional shared package broker",
        handler: HandlerKey::SharedStore,
        also_canonical_top_level: false,
    },
    NestedCommandSpec {
        name: "enroll",
        usage: "enroll <uid> [--read-only]",
        summary: "Grant a user shared-store broker access",
        handler: HandlerKey::SharedStore,
        also_canonical_top_level: false,
    },
    NestedCommandSpec {
        name: "status",
        usage: "status",
        summary: "Show shared-store broker configuration",
        handler: HandlerKey::SharedStore,
        also_canonical_top_level: false,
    },
    NestedCommandSpec {
        name: "broker",
        usage: "broker [--fd <n>]",
        summary: "Serve one shared-store broker request",
        handler: HandlerKey::SharedStore,
        also_canonical_top_level: false,
    },
];
// D-CLI-SURFACE3=B: `push`/`bridge`/`services`/`config` move under `jet os`.
// This group is *not* exhaustive (see `CommandSpec::exhaustive`) — jetos's
// own native verbs (`check`/`build`/`switch`/…, D-JPK-OSVERB1) stay entirely
// owned by the `jet os` dispatcher and are not modeled here.
const OS_ACTIONS: &[NestedCommandSpec] = &[
    NestedCommandSpec {
        name: "push",
        usage: "push",
        summary: "Deploy one or more Jetos machines",
        handler: HandlerKey::Push,
        also_canonical_top_level: false,
    },
    NestedCommandSpec {
        name: "bridge",
        usage: "bridge",
        summary: "Convert an existing system configuration to Jet",
        handler: HandlerKey::Bridge,
        also_canonical_top_level: false,
    },
    NestedCommandSpec {
        name: "services",
        usage: "services",
        summary: "Manage development services",
        handler: HandlerKey::Services,
        also_canonical_top_level: false,
    },
    NestedCommandSpec {
        name: "config",
        usage: "config",
        summary: "Manage Jet settings and trust",
        handler: HandlerKey::Config,
        also_canonical_top_level: false,
    },
];

// D-PERFSESSION1=D: `jet perf` owns collection/view/compare/export. `run` and
// `test` stay canonical top-level intents and also live here so the group help
// lists the full family. Measurement remains a mode of the canonical test
// intent, not another perf-session action.
const PERF_ACTIONS: &[NestedCommandSpec] = &[
    NestedCommandSpec {
        name: "run",
        usage: "run",
        summary: "Run a program and write a .jettrace",
        handler: HandlerKey::Perf,
        also_canonical_top_level: true,
    },
    NestedCommandSpec {
        name: "test",
        usage: "test",
        summary: "Run tests and write a .jettrace",
        handler: HandlerKey::Perf,
        also_canonical_top_level: true,
    },
    NestedCommandSpec {
        name: "attach",
        usage: "attach",
        summary: "Attach to a running process and write a .jettrace",
        handler: HandlerKey::Perf,
        also_canonical_top_level: false,
    },
    NestedCommandSpec {
        name: "view",
        usage: "view",
        summary: "Show a .jettrace summary",
        handler: HandlerKey::Perf,
        also_canonical_top_level: false,
    },
    NestedCommandSpec {
        name: "compare",
        usage: "compare",
        summary: "Compare two .jettrace artifacts",
        handler: HandlerKey::Perf,
        also_canonical_top_level: false,
    },
    NestedCommandSpec {
        name: "export",
        usage: "export",
        summary: "Export a loss-declared projection of a .jettrace",
        handler: HandlerKey::Perf,
        also_canonical_top_level: false,
    },
];
/// Every built-in subcommand. Order here is the order shown in the man page and
/// completions. `jet help` lists the rows by `frequency_rank` (1 = most used),
/// which `node Tools/cli-census/census.mjs` measures and writes in place; run
/// it after adding a row, and `--check` proves the ranks are current. The
/// greeting shows the `headline` rows in that same order.
pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "registry",
        summary: "Publish and manage packages",
        headline: false,
        frequency_rank: 16,
        actions: REGISTRY_ACTIONS,
        exhaustive: true,
        usage: None,
    },
    CommandSpec {
        name: "db",
        summary: "Run a bounded SQL console and manage database migrations",
        headline: false,
        frequency_rank: 55,
        actions: DB_ACTIONS,
        exhaustive: false,
        usage: Some("db [PATH] [--query SQL | --script PATH]"),
    },
    CommandSpec {
        name: "inspect",
        summary: "Explore code, builds, packages, and bindings",
        headline: false,
        frequency_rank: 4,
        actions: INSPECT_ACTIONS,
        exhaustive: true,
        usage: None,
    },
    CommandSpec {
        name: "bind",
        summary: "Resolve and record a checked foreign binding plan",
        headline: false,
        frequency_rank: 39,
        actions: &[],
        exhaustive: false,
        usage: Some("bind <name> [--shape automatic|native] [--freeze]\nbind --policy automatic|frozen\nbind <name> --update --preview\nbind <name> --update --accept <candidate-digest>"),
    },
    CommandSpec {
        name: "project",
        summary: "Inspect project files and modules",
        headline: false,
        frequency_rank: 53,
        actions: PROJECT_ACTIONS,
        exhaustive: true,
        usage: None,
    },
    CommandSpec {
        name: "self",
        summary: "Manage the Jet installation and editor tools",
        headline: false,
        frequency_rank: 11,
        actions: SELF_ACTIONS,
        exhaustive: true,
        usage: None,
    },
    CommandSpec {
        name: "diff",
        summary: "Compare two Jet programs by meaning",
        headline: false,
        frequency_rank: 50,
        actions: &[],
        exhaustive: false,
        usage: None,
    },
    CommandSpec {
        name: "merge",
        summary: "Merge Jet programs without losing code structure",
        headline: false,
        frequency_rank: 52,
        actions: &[],
        exhaustive: false,
        usage: None,
    },
    CommandSpec {
        name: "review",
        summary: "Review meaning, authority, and proof changes",
        headline: false,
        frequency_rank: 45,
        actions: &[],
        exhaustive: false,
        usage: Some("review <base.jet> <head.jet>"),
    },
    CommandSpec {
        name: "run",
        summary: "Run a program or project",
        headline: true,
        frequency_rank: 1,
        actions: &[],
        exhaustive: false,
        usage: Some("run [<file.jet|dir>] [--no-prepare] [-- <args>]"),
    },
    CommandSpec {
        name: JOBS_COMMAND,
        summary: "List, inspect, or watch named project jobs",
        headline: false,
        frequency_rank: 25,
        actions: &[],
        exhaustive: false,
        usage: Some("jobs [--graph|--status|--explain|--watch[=<on|off>]] [<name>]"),
    },
    // D-DX-GENERATE1=A: generation is source-ordered, explicit, and receipt-backed;
    // build/check/test never call this command implicitly.
    CommandSpec {
        name: "generate",
        summary: "Run an explicit source generator with authority and a receipt",
        headline: false,
        frequency_rank: 59,
        actions: &[],
        exhaustive: false,
        usage: Some("generate <GeneratorJob> [--apply|--dry-run] [--entry <file.jet>] [--json]"),
    },
    CommandSpec {
        name: "check",
        summary: "Check code without creating a binary",
        headline: true,
        frequency_rank: 3,
        actions: &[],
        exhaustive: false,
        usage: Some("check [<file.jet|dir>]"),
    },
    CommandSpec {
        name: "fill",
        summary: "Propose checked code for typed goals",
        headline: false,
        frequency_rank: 58,
        actions: &[],
        exhaustive: false,
        usage: Some("fill <file.jet[:line]>"),
    },
    CommandSpec {
        name: "test",
        summary: "Run tests",
        headline: false,
        frequency_rank: 7,
        actions: &[],
        exhaustive: false,
        usage: Some("test [<file.jet|dir>] [<filter>] [--watch] [--fresh] [--docs] [--where=<expr>] [--capture=<failed|all|none>] [--browser=<chromium,firefox,webkit>] [--browser-retries=<n>] [--browser-reporter=<text|json|html>] [--browser-ui] [--browser-visual] [--browser-trace] [--browser-scaffold=<name>] [--grade=generated] [--iterations=<n>] [--time=<s>] [--seed=<n>] [--corpus=<dir>]"),
    },
    CommandSpec {
        name: "test-compare",
        summary: "Compare one recorded observation corpus against its relation",
        headline: false,
        frequency_rank: 54,
        actions: &[],
        exhaustive: false,
        usage: Some("test-compare <corpus.json> [--relation=<name>] [--json]"),
    },
    CommandSpec {
        name: "prove",
        summary: "Create a proof report for code and tests",
        headline: false,
        frequency_rank: 13,
        actions: &[],
        exhaustive: false,
        usage: None,
    },
    // D-DEVR-STATUS1=A: the one project-truth surface. The renderer reads
    // receipts and does not perform a check, test, build, or proof.
    CommandSpec {
        name: "status",
        summary: "Show what the project has proved",
        headline: false,
        frequency_rank: 34,
        actions: &[],
        exhaustive: false,
        usage: Some("status [<file.jet|dir>]"),
    },
    CommandSpec {
        name: "build",
        summary: "Create a native executable",
        headline: false,
        frequency_rank: 2,
        actions: &[],
        exhaustive: false,
        usage: Some("build [<file.jet|dir>] | build --verify <receipt-id>"),
    },
    CommandSpec {
        name: "package",
        summary: "Create a desktop or game application bundle",
        headline: false,
        frequency_rank: 28,
        actions: &[],
        exhaustive: false,
        usage: Some("package --kind <desktop|game> --target <linux-appimage|macos-app|windows-msix> [--executable <path>|<source.jet>] [--output <path>] [--profile <dev|release|name>] [--phase <build,cook,stage,package,export,deploy,run>] [--backend <aot>] [--renderer <headless|raylib>] [--cook-mode <fast|reproducible|scripts-only>] [--export-preset <default|store|headless>] [--deploy-to <path>] [--crash-reporter <off|on|opt-in>] [--crash-consent <not-requested|granted|denied>] [--dry-run] [--explain] [--resume|--cancel] [--clean|--scripts-only] [--run-now] [--package <id>] [--name <name>] [--version <version>] [--icon <path>] [--icon-format <png|icns|ico|svg>] [--icon-size <pixels>] [--publisher <name>] [--description <text>] [--update-channel <channel>] [--update-url <url>]"),
    },
    CommandSpec {
        name: "flash",
        summary: "Flash firmware to a target board",
        headline: false,
        frequency_rank: 30,
        actions: &[],
        exhaustive: false,
        usage: Some("flash --target <board.name> [--image <firmware.elf>] [--audit <target.json>] [--adapter <probe-rs|openocd|emulator>]"),
    },
    CommandSpec {
        name: "cc",
        summary: "Compile and link C with the pinned Jetpack toolchain",
        headline: false,
        frequency_rank: 29,
        actions: &[],
        exhaustive: false,
        usage: Some("cc [options] <sources>"),
    },
    CommandSpec {
        name: "c++",
        summary: "Compile and link C++ with the pinned Jetpack toolchain",
        headline: false,
        frequency_rank: 36,
        actions: &[],
        exhaustive: false,
        usage: Some("c++ [options] <sources>"),
    },
    CommandSpec {
        name: "dev",
        summary: "Watch and run a program; optionally open Canvas or a local app",
        headline: false,
        frequency_rank: 6,
        actions: &[],
        exhaustive: false,
        usage: Some("dev [<file.jet|dir>] [--canvas|--app <function>] [--share <loopback|lan>] [--token <token>] [-- <args>]"),
    },
    CommandSpec {
        name: "learn",
        summary: "Practice Jet with offline code exercises",
        headline: false,
        frequency_rank: 37,
        actions: &[],
        exhaustive: false,
        usage: Some("learn [--check] [--watch|--watch=off] [--json] [--quiet] [--color[=<mode>]]"),
    },
    CommandSpec {
        name: "try",
        summary: "Speculatively apply a plan and re-check its claims",
        headline: false,
        frequency_rank: 61,
        actions: &[],
        exhaustive: false,
        usage: Some("try <plan.json>"),
    },
    CommandSpec {
        name: "debug",
        summary: "Debug a program from Jet source",
        headline: false,
        frequency_rank: 10,
        actions: &[],
        exhaustive: false,
        usage: Some(
            "debug [<file.jet>] [--record=NAME|--replay=NAME] [--dap] [--raw-frames]",
        ),
    },
    CommandSpec {
        name: "repl",
        summary: "Try Jet code interactively",
        headline: false,
        frequency_rank: 24,
        actions: &[],
        exhaustive: false,
        usage: Some("repl [<file.jet>] [--project <dir>] [--console] [--sandbox data] [--console-ttl <milliseconds>] [--allow=<RIGHTS>] [--deny=<RIGHTS>]"),
    },
    CommandSpec {
        name: "notebook",
        summary: "Open a Jet notebook (.jetnb) or Jupyter adapter",
        headline: false,
        frequency_rank: 38,
        actions: &[],
        exhaustive: false,
        usage: None,
    },
    CommandSpec {
        name: "import",
        summary: "Convert supported source code into editable Jet",
        headline: false,
        frequency_rank: 33,
        actions: &[],
        exhaustive: false,
        usage: Some("import <language> <dir> [--dry-run|--update]"),
    },
    CommandSpec {
        name: "new",
        summary: "Create a Jet project or backend source scaffold",
        headline: true,
        frequency_rank: 15,
        actions: &[],
        exhaustive: false,
        usage: Some("new <name> [--template cli|ui|web|overrides] | new service|route|job|migration <name> [--path <path>] [--route <path>] [--model <name>] [--up|--sql <SQL>] [--down <SQL>] [--risk <note>] [--lock <shared|exclusive>] [--version <n>] [--preview|--apply|--remove]"),
    },
    CommandSpec {
        name: "fmt",
        summary: "Format Jet and configured project files",
        headline: false,
        frequency_rank: 8,
        actions: &[],
        exhaustive: false,
        usage: None,
    },
    CommandSpec {
        name: "fix",
        summary: "Apply safe automatic fixes, including `fix memory`",
        headline: false,
        frequency_rank: 9,
        actions: &[],
        exhaustive: false,
        usage: Some("fix <file.jet|dir>"),
    },
    CommandSpec {
        name: "audit",
        summary: "Inspect implicit copies, exercised memory witnesses, or dependencies",
        headline: false,
        frequency_rank: 27,
        actions: &[],
        exhaustive: false,
        usage: None,
    },
    CommandSpec {
        name: "lint",
        summary: "Run optional code-quality checks",
        headline: false,
        frequency_rank: 42,
        actions: &[],
        exhaustive: false,
        usage: Some("lint --a11y|--complexity|--cost <file.jet>"),
    },
    CommandSpec {
        name: "doc",
        summary: "Generate reference documentation",
        headline: false,
        frequency_rank: 48,
        actions: &[],
        exhaustive: false,
        usage: Some("doc [--json|--check] [<file.jet|dir>]"),
    },
    CommandSpec {
        name: "explain",
        summary: "Explain a diagnostic code, build fact, generic-module value, or typed cost",
        headline: false,
        frequency_rank: 5,
        actions: &[],
        exhaustive: false,
        usage: Some("explain <CODE|FACT> [file] | explain --cost <file.jet> | explain --reload <file.jet|dir>"),
    },
    CommandSpec {
        name: "env",
        summary: "Open the project development shell",
        headline: false,
        frequency_rank: 18,
        actions: ENV_ACTIONS,
        exhaustive: false,
        usage: None,
    },
    CommandSpec {
        name: "shared-store",
        summary: "Manage the optional shared package broker",
        headline: false,
        frequency_rank: 60,
        actions: SHARED_STORE_ACTIONS,
        exhaustive: true,
        usage: None,
    },
    CommandSpec {
        name: "cache",
        summary: "Manage the machine-wide artifact store",
        headline: false,
        frequency_rank: 47,
        actions: CACHE_ACTIONS,
        exhaustive: true,
        usage: None,
    },
    CommandSpec {
        name: "remote",
        summary: "Manage host-owned remote builders",
        headline: false,
        frequency_rank: 51,
        actions: &[],
        exhaustive: false,
        usage: None,
    },
    // #1659 criterion 1: `push`/`bridge`/`services` are `jet os` nested
    // actions (see OS_ACTIONS below) — declared once there, not here too.
    CommandSpec {
        name: "trust",
        summary: "Review or change trusted authority",
        headline: false,
        frequency_rank: 44,
        actions: &[],
        exhaustive: false,
        usage: None,
    },
    CommandSpec {
        name: "image",
        summary: "Build a declared container image",
        headline: false,
        frequency_rank: 21,
        actions: &[],
        exhaustive: false,
        usage: None,
    },
    CommandSpec {
        name: "os",
        summary: "Manage Jetos machines and images",
        headline: false,
        frequency_rank: 12,
        actions: OS_ACTIONS,
        exhaustive: false,
        usage: None,
    },
    CommandSpec {
        name: "add",
        summary: "Add and download a dependency",
        headline: false,
        frequency_rank: 43,
        actions: &[],
        exhaustive: false,
        usage: None,
    },
    CommandSpec {
        name: "remove",
        summary: "Remove a dependency",
        headline: false,
        frequency_rank: 57,
        actions: &[],
        exhaustive: false,
        usage: None,
    },
    CommandSpec {
        name: "fetch",
        summary: "Download locked dependencies",
        headline: false,
        frequency_rank: 14,
        actions: &[],
        exhaustive: false,
        usage: None,
    },
    CommandSpec {
        name: "search",
        summary: "Search the local package catalog",
        headline: false,
        frequency_rank: 46,
        actions: &[],
        exhaustive: false,
        usage: Some("search <query>"),
    },
    CommandSpec {
        name: "find",
        summary: "Find code by type, effect, or example",
        headline: false,
        frequency_rank: 56,
        actions: &[],
        exhaustive: false,
        usage: Some(
            "find [--effect <effect>] [--example <input -> output>] [<query>] [<file.jet|dir>]",
        ),
    },
    CommandSpec {
        name: "update",
        summary: "Update dependency or toolchain pins",
        headline: false,
        frequency_rank: 17,
        actions: &[],
        exhaustive: false,
        usage: None,
    },
    CommandSpec {
        name: "init",
        summary: "Create package settings in this directory",
        headline: false,
        frequency_rank: 23,
        actions: &[],
        exhaustive: false,
        usage: None,
    },
    CommandSpec {
        name: "split",
        summary: "Extract closed Package facts into Configs or members",
        headline: false,
        frequency_rank: 49,
        actions: &[],
        exhaustive: false,
        usage: None,
    },
    CommandSpec {
        name: "Fold",
        summary: "Reverse a recorded Package source transition",
        headline: false,
        frequency_rank: 62,
        actions: &[],
        exhaustive: false,
        usage: None,
    },
    // #1659 criterion 1: `config` is a `jet os` nested action (OS_ACTIONS).
    CommandSpec {
        name: "gc",
        summary: "Show values moved into automatic memory management",
        headline: false,
        frequency_rank: 32,
        actions: GC_ACTIONS,
        exhaustive: false,
        usage: None,
    },
    CommandSpec {
        name: "clean",
        summary: "Remove unused package-store data",
        headline: false,
        frequency_rank: 40,
        actions: &[],
        exhaustive: false,
        usage: None,
    },
    // #1659 criterion 1: `publish`/`yank`/`keygen`/`key`/`vendor` are `jet
    // registry` nested actions (REGISTRY_ACTIONS); `audit`/`sbom` are `jet
    // inspect` nested actions (INSPECT_ACTIONS) — declared once there.
    CommandSpec {
        name: "emit",
        summary: "Print generated build output",
        headline: false,
        frequency_rank: 41,
        actions: &[],
        exhaustive: false,
        usage: None,
    },
    CommandSpec {
        name: "eval",
        summary: "Evaluate pure Jet and print the value (`--json` for JSON)",
        headline: false,
        frequency_rank: 22,
        actions: &[],
        exhaustive: false,
        usage: Some("eval <file.jet|expression>"),
    },
    CommandSpec {
        name: "budget",
        summary: "Check performance limits or update baselines",
        headline: false,
        frequency_rank: 26,
        actions: &[],
        exhaustive: false,
        usage: None,
    },
    CommandSpec {
        name: "perf",
        summary: "Collect and inspect performance traces",
        headline: false,
        frequency_rank: 19,
        actions: PERF_ACTIONS,
        exhaustive: true,
        usage: None,
    },
    CommandSpec {
        name: "fuzz",
        summary: "Find failing inputs for property tests",
        headline: false,
        frequency_rank: 31,
        actions: &[],
        exhaustive: false,
        usage: Some("fuzz <file.jet> [<test>]"),
    },
    CommandSpec {
        name: "version",
        summary: "Show the Jet version",
        headline: false,
        frequency_rank: 35,
        actions: &[],
        exhaustive: false,
        usage: None,
    },
    CommandSpec {
        name: "help",
        summary: "Show command help",
        headline: false,
        frequency_rank: 20,
        actions: &[],
        exhaustive: false,
        usage: Some("help [<command>] [--sort frequency|az|za]"),
    },
];
