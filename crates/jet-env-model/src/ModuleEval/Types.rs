//! Captured plans (derive-only data) produced by module evaluation: the
//! per-module `EvaluatedModule`, the jetos-tier `SystemPlan`/`ServicePlan`/
//! `OptionPlan`/`ImagePlan`, and the runnable `EnvPlan`.

use crate::AST::Namespace;

use super::super::Merge::{self, EntryContribution};
use super::super::Recipe::BuildRecipe;
use super::super::RefSpec::SourceTable;
use super::Environment::{
    EnvironmentIntegration, EnvironmentLifecycle, IntegrationFactProjection, LanguageExpansion, LanguagePack,
    LanguageProjection, LanguageSpec, PackageProfileSpec, PresetSpec, ResolvedPreset,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptPathMode {
    Short,
    Full,
}

impl Default for PromptPathMode {
    fn default() -> PromptPathMode {
        PromptPathMode::Short
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptStripMode {
    Off,
    On,
}

impl Default for PromptStripMode {
    fn default() -> PromptStripMode {
        PromptStripMode::Off
    }
}

/// One module's contributions, keyed by `(namespace, path)` so `merge_all`
/// can combine same-keyed contributions from different modules.
#[derive(Debug)]
pub struct EvaluatedModule {
    pub name: String,
    pub entries: Vec<((Namespace, String), EntryContribution)>,
    /// U11: `system.<name>:` contributions, captured for the jetos tier (gap #4
    /// realizes them; gap #5 only field-checks + captures).
    pub systems: Vec<SystemPlan>,
    /// U14: `image.<name>:` contributions, captured for the jetos tier.
    pub images: Vec<ImagePlan>,
    /// U15: `fleet.<name>:` contributions, captured (parse/cross-check now;
    /// ssh realization rides single-host jetos, Phase D).
    pub fleets: Vec<FleetPlan>,
    /// D-JOS-VMTEST1: `vmtest.<name>:` contributions, captured as runnable VM
    /// scenarios over known systems.
    pub vmtests: Vec<VmTestPlan>,
    /// U12: dev-supervised `services:` entries captured from every `env.<name>`
    /// role-module in this module, in source order. Distinct from the jetos
    /// `system.<name>.services` capture (`ServicePlan` above) — a different
    /// Rust type and evaluator (`ModuleEval::DevService`), even though the
    /// `Service` grammar itself is the one ratified shape (U12).
    pub dev_services: Vec<DevServicePlan>,
    /// U13 (D-JPK-SECRETCRYPTO1): declared `secrets: ["name", …]` names from
    /// every `env.<name>` role-module in this module, in source order — the
    /// names this env expects to find in the project's encrypted store
    /// (`.jet/secrets.age`). Validated (every name present) at env entry;
    /// `Jetpack::Secrets` is the only consumer.
    pub secrets: Vec<String>,
    /// U20: `Pkg.adapt(...)` declarations captured from `packages:` fields.
    /// They ride alongside ordinary package refs and realize into normal
    /// hangar objects.
    pub adapters: Vec<AdapterPlan>,
    /// D-ENV-LIFECYCLE1: typed lifecycle facts captured from env fields.
    pub lifecycle: EnvironmentLifecycle,
    /// D-ENV-PROFILE1: named profile contributions.
    pub profiles: Vec<PresetSpec>,
    /// D-ENV-LANGPACK1: typed language-pack selections from this module.
    pub languages: Vec<LanguageSpec>,
    /// D-ENV-FILES1: managed environment-file declarations.
    pub files: Vec<super::Environment::ManagedFile>,
    /// D-ENV-INTEGRATIONS1: typed first-party integrations imported by this
    /// module and lowered into ordinary environment facts.
    pub integrations: Vec<EnvironmentIntegration>,
    pub environment_names: Vec<String>,
    /// D-JPK-PROFILE1=D: source-backed package profile declarations. These
    /// are not active environment profiles and never affect shell selection.
    pub package_profiles: Vec<PackageProfileSpec>,
}

/// U20: an ad-hoc adapter package declared with `Pkg.adapt(...)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterPlan {
    pub name: String,
    /// A source ref string such as `"./vendor/weirdctl"`.
    pub source: String,
    pub deps: Vec<Merge::Pkg>,
    pub recipe: AdapterRecipe,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterRecipe {
    /// `Recipe.copy(...)`: copy the staged source tree as-is.
    Copy,
    /// `Recipe.prebuilt(bin: "...", as: "...")`: install one executable under
    /// `bin/<as>`.
    Prebuilt { bin: String, as_name: String },
    /// `Recipe.build(steps: […])`: a finite, digestable executable action graph.
    Build(BuildRecipe),
}

/// U11: a field-checked `system.<name>: { … }` contribution, captured so the
/// jetos tier (gap #4) can realize it. Pure data — no realize logic here.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemPlan {
    /// The contribution path — the `<name>` in `system.<name>`.
    pub name: String,
    /// U13: the typed target platform, e.g. `linux.x64`.
    pub target: String,
    /// U6: the packages to install, as `Pkg`s (source-qualified).
    pub packages: Vec<Merge::Pkg>,
    /// U12: the enabled/typed services, in source order.
    pub services: Vec<ServicePlan>,
    /// U13: the ordered option entries (`network.hostName: laptop`), in source order.
    pub options: Vec<OptionPlan>,
}

/// U12: one captured `Service` record under a `System`'s `services:` map.
#[derive(Debug, Clone, PartialEq)]
pub struct ServicePlan {
    /// The service name (the map key), e.g. `openssh`.
    pub name: String,
    /// The required `enable` flag (U12).
    pub enable: bool,
    /// Any further open-record fields, rendered to display strings, in source
    /// order. (e.g. `ports: [22]`.)
    pub extra: Vec<(String, String)>,
}

/// U12: one captured dev-supervised `Service` record under an `env.<name>`
/// role-module's `services:` map (`ContribValue::Env`/`EnvLit::services`).
/// Distinct from `ServicePlan` (the jetos `system.*.services` capture): same
/// ratified `Service` grammar (open record, required `enable`), but jetpack's
/// dev-runtime tier (`Jetpack::Services`) owns and interprets every field
/// here — there is no downstream Nix-option consumer the way jetos services
/// have (Phase D), so an unrecognized `extra` key is a supervision-time
/// error (E1262), not silently-forwarded metadata.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DevServicePlan {
    /// The service name (the map key), e.g. `redis`.
    pub name: String,
    /// The required `enable` flag (U12) — `false` captures the service
    /// (so `jet services logs` etc still knows its name) without supervising it.
    pub enable: bool,
    /// `ports: [Int]` — TCP ports the service listens on. The first port is
    /// the default readiness probe target when no explicit `ready:` is given.
    pub ports: Vec<i64>,
    /// `run: ["program", "arg", …]` — the executable and argv that start the
    /// service. Falls back to the built-in catalog when the name matches a
    /// known service and this is unset.
    pub run: Option<Vec<String>>,
    /// `shutdown: .Term(...)`/`.Kill` — typed process shutdown policy. Falls
    /// back to a bounded TERM-then-KILL sequence when unset.
    pub shutdown: Option<ShutdownPolicy>,
    /// `data_dir: "…"` — override for the persisted-state directory, which
    /// otherwise defaults to `.jet/services/<name>/data`.
    pub data_dir: Option<String>,
    /// `ready: "…"` — a shell command polled until it exits 0; the readiness
    /// contract. Falls back to a TCP connect on `ports[0]` when unset and
    /// `ports` is non-empty, else to a bare process-alive check.
    pub ready: Option<String>,
    /// D-JPK-SERVICEDEPTH1: typed readiness without shell-command parsing.
    pub ready_probe: Option<ReadyProbe>,
    /// D-JPK-SERVICEDEPTH1: bounded restart behavior.
    pub restart: Option<RestartPolicy>,
    /// Files whose changes may trigger a bounded restart.
    pub watch: Vec<String>,
    /// Service names that must be healthy before this service starts.
    pub after: Vec<String>,
    /// Ordinary `#Task` names to run successfully immediately before start.
    pub before_start: Vec<String>,
    /// Named sockets reserved by the service.
    pub sockets: Vec<String>,
    /// Any further field, captured verbatim as a display string (open record,
    /// U12) — checked against the known keys above at supervision time, not
    /// at field-check time (E1262). This includes retired spellings such as
    /// `depends_on`; `after` is the only dependency field.
    pub extra: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadyProbe {
    Exec(String),
    Http { url: String, status: Option<u16> },
    Notify { path: String },
    Tcp { host: String, port: u16 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartPolicy {
    Never,
    OnFailure {
        max: u32,
        backoff_ms: u64,
        exponential: bool,
    },
    Always {
        max: u32,
        backoff_ms: u64,
        exponential: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShutdownPolicy {
    /// Send TERM, wait up to `grace_ms`, then use the bounded KILL fallback.
    Term { grace_ms: u64 },
    /// Skip the graceful signal and terminate the verified process group.
    Kill,
}

/// U13: one captured `options:` entry — a dotted key path and its value, rendered
/// to a display string.
#[derive(Debug, Clone, PartialEq)]
pub struct OptionPlan {
    pub key: String,
    pub value: String,
}

/// D-JPK-IMAGE1 (=A, ratified 2026-07-01): which referent an `Image`'s `from:`
/// names, and so which realize tier owns it. `Iso` is the original U14 shape
/// (disk images from a `System`, Phase D jetos installer tier, owner-gated,
/// untouched by this slice). `Oci` is the new container tier (built from a
/// `Package`, native — no jetos/Phase D gate).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    Iso,
    Oci,
}

/// U14/D-JPK-IMAGE1: a field-checked `image.<name>: { … }` contribution.
/// `Iso`: `target`/`packages`/`services`/`options` are inherited from the
/// referenced `System` at realize time (Phase D, gap #4), so they are not
/// stored here. `Oci`: built natively now (no jetos tier involved) from the
/// referenced `Package`'s realized binary plus `expose`/`env_vars`/`files`/`base`.
#[derive(Debug, Clone, PartialEq)]
pub struct ImagePlan {
    /// The contribution path — the `<name>` in `image.<name>`.
    pub name: String,
    /// Which referent `from:` names, and which tier realizes this image.
    pub kind: ImageKind,
    /// `Iso`: the source system's name (`from: system.<name>`).
    /// `Oci`: the source package's name (`from: packages.<name>`).
    pub from: String,
    /// D-ENV-IMAGE1: true when `from:` projects an `env.<name>` into an OCI
    /// shell image. The source kind stays explicit in the plan so realization
    /// cannot confuse an environment with an executable package.
    pub from_environment: bool,
    /// `Iso` only: the disk-image format (`iso` default / `qcow` / `raw`).
    /// Empty for `Oci`.
    pub format: String,
    /// An explicit target platform, if any. ISO realizes it through its
    /// referenced system; OCI records it for the image realization path.
    pub target: Option<String>,
    /// `Oci` only: `expose: [Int]` — TCP ports recorded as `ExposedPorts` in
    /// the OCI image config. Sorted + deduped for reproducibility.
    pub expose: Vec<i64>,
    /// `Oci` only: `env_vars: [KEY: "value"]` — baked into the OCI image
    /// config's `Env` list. Source order follows the map's key order
    /// (`CtValue::Map` is a `BTreeMap`, so this is already sorted by key).
    pub env_vars: Vec<(String, String)>,
    /// `Oci` only: `files: ["path", …]` — extra project-relative paths layered
    /// into the image alongside the package binary. Sorted before layering so
    /// the tar layer is byte-identical regardless of declaration order.
    pub files: Vec<String>,
    /// `Oci` only: `base: oci("<ref>")` — a base-image escape hatch (D-JPK-
    /// IMAGE1 option A). Local `file://` layouts are admitted by digest; remote
    /// refs stay explicit until a verified registry transport is configured.
    pub base: Option<String>,
    /// D-ENV-IMAGE1 expert projection fields. Empty/None uses the safe shell
    /// default for an environment image.
    pub services: Vec<String>,
    pub health: Option<String>,
    pub entrypoint: Option<String>,
    pub user: Option<u32>,
}

/// U15: a field-checked `fleet.<name>: { hosts: { … } }` contribution, captured
/// for the jetos deployment tier. Pure data — ssh/rollout realization lives in
/// Phase D (gated). Each host's `system` ref is cross-checked against the known
/// systems at plan-assembly time (E1242).
#[derive(Debug, Clone, PartialEq)]
pub struct FleetPlan {
    /// The contribution path — the `<name>` in `fleet.<name>`.
    pub name: String,
    /// The hosts this fleet deploys, in source order.
    pub hosts: Vec<HostPlan>,
}

/// U15: one `<host>: system.<name>.{ overrides }` entry in a fleet's `hosts:` map.
#[derive(Debug, Clone, PartialEq)]
pub struct HostPlan {
    /// The host name (the map key), e.g. `web1`.
    pub name: String,
    /// The referenced `System`'s role name (the `<name>` in `system.<name>`).
    pub system: String,
    /// The typed `.{ … }` copy-with-update override, if written. Every field
    /// is evaluated through the same pure comptime path as a `System` field.
    pub overrides: Option<HostOverride>,
    /// Exact source text retained as provenance for explain/round-trip output.
    pub override_source: Option<String>,
}

/// One host-local copy-with-update record. The value variants preserve the
/// closed System field shapes while still allowing future fleet-only fields
/// such as `region` to remain typed comptime values. `source` is provenance;
/// consumers use `fields`, never deferred source evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct HostOverride {
    pub fields: Vec<(String, HostOverrideValue)>,
    pub source: String,
    /// The dependency/purity facts used to produce each typed field. Consumers
    /// can explain why a host value changed without re-evaluating source text.
    pub provenance: Vec<HostOverrideProvenance>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostOverrideProvenance {
    pub field: String,
    pub dependencies: Vec<String>,
    pub pure: bool,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HostOverrideValue {
    Platform(String),
    Packages(Vec<Merge::Pkg>),
    Services(Vec<ServicePlan>),
    Options(Vec<OptionPlan>),
    Value(crate::Comptime::CtValue),
}

/// D-JOS-VMTEST1/D-JOS-VMASSERT1: a declarative VM test scenario.
#[derive(Debug, Clone, PartialEq)]
pub struct VmTestPlan {
    pub name: String,
    pub hosts: Vec<HostPlan>,
    pub run: String,
    pub assertions: Vec<String>,
}

/// The runnable shape of a typed `env.jet`, ready for the CLI run/build path:
/// the named-source table, the package refs to realize (`package@source`),
/// and the prompt label. Only the `env` namespace is consulted — `system`/`image`
/// are the jetos tiers and have no meaning for `jetpack`.
#[derive(Debug)]
pub struct EnvPlan {
    pub table: SourceTable,
    /// The source files that contributed to this graph, relative to the
    /// environment root and in deterministic discovery order.
    pub source_files: Vec<String>,
    pub package_refs: Vec<String>,
    /// U20 adapter packages declared in `packages:`. Kept separate from refs
    /// because they have inline build identity and no provider selector.
    pub adapters: Vec<AdapterPlan>,
    pub prompt: Option<String>,
    pub prompt_path: PromptPathMode,
    pub prompt_strip: PromptStripMode,
    /// U11: every captured `System` across all evaluated modules, in source order.
    /// The jetos tier (gap #4) realizes these; the dev-shell path ignores them.
    pub systems: Vec<SystemPlan>,
    /// U14: every captured `Image`, validated so each `from` names a known system.
    pub images: Vec<ImagePlan>,
    /// U15: every captured `Fleet`, validated so each host names a known system.
    pub fleets: Vec<FleetPlan>,
    /// D-JOS-VMTEST1: every captured VM scenario, validated so each host names a
    /// known system.
    pub vmtests: Vec<VmTestPlan>,
    /// U12: every captured dev-supervised `Service`, across all evaluated
    /// modules, in source order. `jetpack services <verb>`/`jetpack dev`'s
    /// health gate are the only consumers — the jetos tier never reads this.
    pub dev_services: Vec<DevServicePlan>,
    /// U13 (D-JPK-SECRETCRYPTO1): every declared `secrets:` name, across all
    /// evaluated modules, in source order. `jetpack enter`/`jetpack dev`
    /// validate every name exists in the encrypted store at env entry
    /// (E1263); the jetos tier never reads this.
    pub secrets: Vec<String>,
    /// Typed lifecycle facts for activation, checks, and reload.
    pub lifecycle: EnvironmentLifecycle,
    /// Named profiles before CLI/host selection.
    pub profiles: Vec<PresetSpec>,
    /// Typed language-pack selections before catalog expansion.
    pub languages: Vec<LanguageSpec>,
    /// The profile selected by the evaluator/runtime, if one was requested.
    pub selected_preset: Option<ResolvedPreset>,
    /// One evaluator-owned expansion shared by realization and trust. The
    /// `language_packs` field remains as a compatibility view for existing
    /// environment consumers; it is populated from this expansion.
    pub language_expansion: LanguageExpansion,
    /// Expanded language packs, kept in the plan for disclosure and hashing.
    pub language_packs: Vec<LanguagePack>,
    /// Exact typed language projection, including included/omitted tools and
    /// changed selection facts. Consumers must use this instead of reparsing
    /// the source or reconstructing pack defaults.
    pub language_projections: Vec<LanguageProjection>,
    /// Managed environment-file declarations before `jet env sync` applies them.
    pub files: Vec<super::Environment::ManagedFile>,
    /// D-ENV-INTEGRATIONS1: typed integrations before host realization.
    pub integrations: Vec<EnvironmentIntegration>,
    pub integration_facts: IntegrationFactProjection,
    /// D-JPK-PROFILE1=D: source-backed package-profile declarations. These
    /// remain separate from the selected shell environment profile.
    pub package_profiles: Vec<PackageProfileSpec>,
    pub environment_names: Vec<String>,
    /// The one environment profile whose packages/settings are active for this
    /// plan. `dev` is the deterministic beginner default when present.
    pub active_environment: Option<String>,
    /// Module names that contributed the selected environment profile, in source order.
    pub active_environment_provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EnvironmentFacts {
    pub environment_names: Vec<String>,
    pub active_environment: Option<String>,
    pub active_environment_provenance: Vec<String>,
    pub source_files: Vec<String>,
    pub dev_services: Vec<DevServicePlan>,
    pub lifecycle: EnvironmentLifecycle,
    pub profiles: Vec<PresetSpec>,
    pub languages: Vec<LanguageSpec>,
    pub selected_preset: Option<ResolvedPreset>,
    pub language_expansion: LanguageExpansion,
    pub language_packs: Vec<LanguagePack>,
    pub language_projections: Vec<LanguageProjection>,
    pub files: Vec<super::Environment::ManagedFile>,
    pub integrations: Vec<EnvironmentIntegration>,
    pub integration_facts: IntegrationFactProjection,
    pub package_profiles: Vec<PackageProfileSpec>,
}
