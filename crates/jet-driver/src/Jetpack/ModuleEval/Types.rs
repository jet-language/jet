//! Captured plans (derive-only data) produced by module evaluation: the
//! per-module `EvaluatedModule`, the jetos-tier `SystemPlan`/`ServicePlan`/
//! `OptionPlan`/`ImagePlan`, and the runnable `EnvPlan`.

use crate::AST::Namespace;

use super::super::Merge::{self, EntryContribution};
use super::super::RefSpec::SourceTable;

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
    /// U13: the ordered option entries (`net.hostName: laptop`), in source order.
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
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DevServicePlan {
    /// The service name (the map key), e.g. `redis`.
    pub name: String,
    /// The required `enable` flag (U12) — `false` captures the service
    /// (so `jet services logs` etc still knows its name) without supervising it.
    pub enable: bool,
    /// `ports: [Int]` — TCP ports the service listens on. The first port is
    /// the default readiness probe target when no explicit `ready:` is given.
    pub ports: Vec<i64>,
    /// `init: "…"` — the shell command that starts the service. Falls back to
    /// the built-in catalog's command when the name matches a known service
    /// and this is unset.
    pub init: Option<String>,
    /// `shutdown: "…"` — the shell command that stops the service. Falls back
    /// to a plain `SIGTERM`/`SIGKILL` of the supervised pid when unset.
    pub shutdown: Option<String>,
    /// `data_dir: "…"` — override for the persisted-state directory, which
    /// otherwise defaults to `.jet/services/<name>/data`.
    pub data_dir: Option<String>,
    /// `ready: "…"` — a shell command polled until it exits 0; the readiness
    /// contract. Falls back to a TCP connect on `ports[0]` when unset and
    /// `ports` is non-empty, else to a bare process-alive check.
    pub ready: Option<String>,
    /// Any further field, captured verbatim as a display string (open record,
    /// U12) — checked against the known keys above at supervision time, not
    /// at field-check time (E1262).
    pub extra: Vec<(String, String)>,
}

/// U13: one captured `options:` entry — a dotted key path and its value, rendered
/// to a display string.
#[derive(Debug, Clone, PartialEq)]
pub struct OptionPlan {
    pub key: String,
    pub value: String,
}

/// U14: a field-checked `image.<name>: { … }` contribution, captured for the
/// jetos tier. `target`/`packages`/`services`/`options` are inherited from the
/// referenced `System` at realize time (gap #4), so they are not stored here.
#[derive(Debug, Clone, PartialEq)]
pub struct ImagePlan {
    /// The contribution path — the `<name>` in `image.<name>`.
    pub name: String,
    /// U14: the source system this image is built from (`from: system.<name>`).
    pub from: String,
    /// U14: the disk-image format (`iso` default / `qcow` / `raw`).
    pub format: String,
    /// U14: an explicit cross-compile target, if any (else inherited from system).
    pub target: Option<String>,
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
    /// The raw `.{ … }` copy-with-update override text, if written. Captured
    /// verbatim; not semantically applied until fleet realization (Phase D).
    pub overrides: Option<String>,
}

/// The runnable shape of a typed `env.jet`, ready for the CLI run/build path:
/// the named-source table, the package refs to realize (`<source>:<package>`),
/// and the prompt label. Only the `env` namespace is consulted — `system`/`image`
/// are the jetos tiers and have no meaning for `jetpack`.
#[derive(Debug)]
pub struct EnvPlan {
    pub table: SourceTable,
    pub package_refs: Vec<String>,
    pub prompt: Option<String>,
    /// U11: every captured `System` across all evaluated modules, in source order.
    /// The jetos tier (gap #4) realizes these; the dev-shell path ignores them.
    pub systems: Vec<SystemPlan>,
    /// U14: every captured `Image`, validated so each `from` names a known system.
    pub images: Vec<ImagePlan>,
    /// U15: every captured `Fleet`, validated so each host names a known system.
    pub fleets: Vec<FleetPlan>,
    /// U12: every captured dev-supervised `Service`, across all evaluated
    /// modules, in source order. `jetpack services <verb>`/`jetpack dev`'s
    /// health gate are the only consumers — the jetos tier never reads this.
    pub dev_services: Vec<DevServicePlan>,
    /// U13 (D-JPK-SECRETCRYPTO1): every declared `secrets:` name, across all
    /// evaluated modules, in source order. `jetpack enter`/`jetpack dev`
    /// validate every name exists in the encrypted store at env entry
    /// (E1263); the jetos tier never reads this.
    pub secrets: Vec<String>,
}
