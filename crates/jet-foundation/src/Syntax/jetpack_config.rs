/// U13: env-namespace field name — `secrets: ["name", …]` under an
/// `env.<name>` role-module, the names this env expects to find in the
/// project's encrypted store (validated at env entry, E1263 if any is
/// missing).
pub const ENV_FIELD_SECRETS: &str = "secrets";

/// U19: `jetpack config <verb>` — today only `trust` pattern management.
pub const CONFIG_SUBCOMMAND: &str = "config";
pub const CONFIG_VERB_TRUST: &str = "trust";
/// U28 / D-JPK-NODAEMON1=A: sandbox fallback policy lives under `jetpack config
/// sandbox`; `require` hard-fails when unprivileged sandboxing is unavailable,
/// `allow` permits the explicit L0205 fallback warning.
pub const CONFIG_VERB_SANDBOX: &str = "sandbox";
pub const CONFIG_TRUST_VERB_ADD: &str = "add";
pub const CONFIG_TRUST_VERB_LIST: &str = "list";
pub const CONFIG_TRUST_VERB_REMOVE: &str = "remove";
pub const CONFIG_TRUST_VERBS: &[&str] = &[
    CONFIG_TRUST_VERB_ADD,
    CONFIG_TRUST_VERB_LIST,
    CONFIG_TRUST_VERB_REMOVE,
];
pub const CONFIG_SANDBOX_VERB_REQUIRE: &str = "require";
pub const CONFIG_SANDBOX_VERB_ALLOW: &str = "allow";
pub const CONFIG_SANDBOX_VERB_STATUS: &str = "status";
pub const CONFIG_SANDBOX_VERBS: &[&str] = &[
    CONFIG_SANDBOX_VERB_REQUIRE,
    CONFIG_SANDBOX_VERB_ALLOW,
    CONFIG_SANDBOX_VERB_STATUS,
];

/// U19: the one-shot bypass flag for the env/dev trust gate — never persists
/// a grant (unlike accepting the interactive prompt, which does).
pub const TRUST_BYPASS_FLAG: &str = "--trust";

/// U19: the env/dev trust store, `~/.jet/trust` (home-scoped: a user's trust
/// decisions follow them across projects, unlike the project-local `.jet/`
/// managed folder). Plain newline-separated `hash:`/`pattern:` lines, mirroring
/// the plain-text convention `Jetpack::Recipe`'s adapter trust marker already
/// uses. Lives under the same default dir as `~/.jet/config.jet`
/// (`CONFIG_DEFAULT_DIR`).
pub const TRUST_FILE: &str = "trust";

/// D-JPK-DISPATCH1=B (A1, card c9jetpackgates): `jet` execs the engine
/// binary (`jetpack`, later `jetos`) for every engine verb instead of linking
/// it in-process — git/kubectl-style dispatch by executable name. Before
/// exec-ing the real command, `jet` runs `<engine> --engine-protocol`, a
/// hidden handshake flag every engine binary answers with its own
/// `CARGO_PKG_VERSION` on stdout; a mismatch against `jet`'s own version is
/// E1227 (`engine-version-skew`). This is process-dispatch plumbing between
/// two binaries jet ships together, never a token a user writes in a `.jet`
/// file, so I7 (every user-typeable keyword lives here with a decision ID)
/// does not require it to gate a Tower ballot of its own — it is this
/// gate's own implementation surface.
pub const ENGINE_PROTOCOL_FLAG: &str = "--engine-protocol";

/// D-JPK14: the default visible prompt label inside a Jetpack shell.
pub const JETPACK_PROMPT_LABEL: &str = "jetpack";

/// D-JPK14: shell marker env var set inside a Jetpack shell.
pub const JETPACK_ENV_MARKER: &str = "JETPACK_ENV";

/// D-JPK14: env var carrying the realized package refs inside a Jetpack shell.
pub const JETPACK_REF_VAR: &str = "JETPACK_REF";

/// D-JPK3/17: the directive calls an `env.jet` author writes. `pkg.source`
/// takes one arg (default built-in source) or two (named source + upstream/pin,
/// D-JPK17). Packages reference named sources inline via `<name>:<package>`.
pub const PACK_DIRECTIVE_SOURCE: &str = "pkg.source";
pub const PACK_DIRECTIVE_PACKAGES: &str = "pkg.packages";
pub const PACK_DIRECTIVE_PROMPT: &str = "pkg.prompt";

// ──────────────────────────────────────────────
// Unified ecosystem (jet + jetpack + jetos) — user-typeable surface (I7).
// Owner-ratified design-of-record: docs/plans/epoch-5/unified-ecosystem.md
// (U1–U7, ratified 2026-06-16). These IDs start with `U`, enforced by
// tests/decisions.rs alongside the S/N decisions. Tokens are recorded here;
// behavior lands in the Jetpack/Jetos implementation chunks (no syntax beyond
// what is ratified). The S52 amendment names (U1/U2) live with the S52 block.
// ──────────────────────────────────────────────

/// U3 (ratified 2026-06-16): module declaration keyword — `module name { … }`.
pub const KW_MODULE: &str = "module";

/// D-GENMOD2=A (ratified 2026-06-28): generic module parameter list uses `<…>`.
/// Type params: `K: Hash` (name starts uppercase; bound is a trait).
/// Value params: `capacity: Int` (name starts lowercase; annotation is a concrete type).
/// Instantiation: `module alias = module_name<TypeArg, value_arg>`.
/// Reuses existing `<`/`>` angle-bracket tokens (no new sigil, I7 satisfied).
pub const GENMOD_OPEN: &str = "<"; // reuses OP_LT
pub const GENMOD_CLOSE: &str = ">"; // reuses OP_GT

/// U3 (ratified 2026-06-16): a leading underscore on a module name disables it
/// (`module _name { … }` is not discovered or merged). One char, reversible.
pub const MODULE_DISABLE_PREFIX: &str = "_";

/// S84 (ratified 2026-06-16): *dashed names* — the kebab-case naming rule for
/// package / module / system / image / env **names** (and `from: system.<name>`
/// references). The grammar is `ident (-ident)*`: a `-` joins two segments only
/// when it is span-adjacent to both (no surrounding whitespace), matching
/// nixpkgs/npm package-name convention (e.g. `image.halcyon-iso`,
/// `system.my-host`). No new sigil — this reuses the existing `-`/Minus token;
/// span adjacency is what keeps a spaced `a - b` as subtraction, so the rule
/// never bleeds into the expression grammar. No leading, trailing, or doubled
/// hyphen. Code identifiers (variables, fields, types, functions) stay plain
/// `ident`. Enforced in `parser.rs::expect_dashed_name`.
pub const NAME_SEGMENT_SEP: &str = "-";

/// U3 (ratified 2026-06-16): reserved namespaces any module may contribute to.
pub const NS_ENV: &str = "env";
pub const NS_SYSTEM: &str = "system";
pub const NS_IMAGE: &str = "image";

/// D-WORKSPACE2 (ratified 2026-06-25, option A): the monorepo index is the
/// reserved namespace `workspace` — `module workspace { members: … }` in
/// `workspace.jet` (D-WORKSPACE1=B; see WORKSPACE_FILE). Owner kept the
/// industry-standard term over the aviation menu (`fleet`/`wing`/…). Not yet wired
/// (resolver rides board card c156).
pub const NS_WORKSPACE: &str = "workspace";

/// D-PERFBUDGET-GRAMMAR1=A: reserved performance-policy role namespace.
/// `module perf.<role> { budgets: [Budget.{ ... }] }` is sole declaration
/// surface. Names are reserved before parser/runtime implementation. Full law:
/// docs/spec/performance-budget-decisions.md.
pub const NS_PERF: &str = "perf";
pub const PERF_FIELD_BUDGETS: &str = "budgets";
pub const TYPE_BUDGET: &str = "Budget";
pub const TYPE_BUDGET_APPLIES: &str = "BudgetApplies";

/// D-JOS-PRIORITY-SURFACE2=A: typed wrapper used only when one option
/// contribution needs explicit precedence. Plain values remain ordinary.
pub const TYPE_OPTION_VALUE: &str = "OptionValue";
pub const OPTION_PRIORITY_TIERS: &[&str] = &["Default", "Force", "Priority"];

/// D-PERFBUDGET-GRAMMAR1=A: closed typed Budget vocabulary. Leading-dot enum
/// cases use these exact spellings; no metric-key shorthand or aliases exist.
pub const PERF_BUDGET_SCOPES: &[&str] =
    &["Package", "Env", "Service", "Scene", "Bench", "Target"];
pub const PERF_BUDGET_PROVIDERS: &[&str] = &[
    "BuildArtifact", "CompilerFacts", "AllocationProbe", "BenchMeasurement",
    "ServiceProbe", "SceneProbe",
];
pub const PERF_BUDGET_METRICS: &[&str] = &[
    "BinarySize", "ArtifactSize", "GeneratedUnsafe", "PublicApiItems",
    "DependencyCount", "EffectCount", "AllocationCount", "AllocationBytes",
    "StartupTime", "FrameTime", "Latency", "Throughput", "MemoryHighWater",
    "BenchTime", "ServiceReadiness", "SceneAssetBytes", "DrawCalls",
];
pub const PERF_BUDGET_PERCENTILES: &[&str] = &["P50", "P90", "P95", "P99", "P999"];
pub const PERF_BUDGET_COMPARISONS: &[&str] = &["Absolute", "AbsoluteFrom", "RelativeTo"];
pub const PERF_BUDGET_LIMITS: &[&str] =
    &["AtMost", "AtLeast", "RegressionAtMost", "ImprovementAtLeast"];
pub const PERF_BUDGET_ENFORCEMENT: &[&str] = &["Fail", "Warn"];
pub const PERF_BUDGET_SELECTIONS: &[&str] = &["Current", "All", "Only"];
pub const PERF_BUDGET_TARGET_SELECTORS: &[&str] = &["Class", "Triple"];
pub const PERF_BUDGET_TARGET_CLASSES: &[&str] =
    &["Native", "Web", "Freestanding", "Plugin", "OsImage"];
pub const PERF_BUDGET_PROFILES: &[&str] =
    &["Dev", "Release", "Small", "Test", "Bench", "Named"];
pub const PERF_BUDGET_UNIT_SUFFIXES: &[&str] =
    &["ns", "us", "ms", "s", "B", "KiB", "MiB", "GiB", "pct"];

/// D-JPK-FLEET1=A (ratified 2026-07-02): a fleet is a map of named hosts to
/// `System` refs — `module fleet.<name> { hosts: { web1: system.<sys>.{ … } } }`.
/// Distinct from `workspace` (the monorepo index): a fleet is a deployment target.
/// Parse/capture/cross-check now; ssh realization rides single-host jetos (Phase D).
pub const NS_FLEET: &str = "fleet";

/// D-JOS-VMTEST1=A: a VM scenario is a checked test target over jetos systems.
/// `module vmtest.<name> { hosts: { node: system.<host> }, run: test { … } }`
/// is the canonical scenario declaration; the CLI and CI consume the same object.
pub const NS_VMTEST: &str = "vmtest";

/// U3 (ratified 2026-06-16): the type matching each reserved namespace.
pub const TYPE_ENV: &str = "Env";
/// D-FE-PROMPT-STRIP1: structured prompt config inside an `Env` contribution.
pub const TYPE_PROMPT: &str = "Prompt";
pub const TYPE_SYSTEM: &str = "System";
pub const TYPE_IMAGE: &str = "Image";
/// D-JPK-FLEET1: the type name of a `fleet.<name>` contribution record.
pub const TYPE_FLEET: &str = "Fleet";
/// D-JOS-VMTEST1: the type name of a `vmtest.<name>` contribution record.
pub const TYPE_VMTEST: &str = "VmTest";

/// D-JPK-FLEET1: a `Fleet`'s one required field — the `hosts:` map.
pub const FLEET_FIELD_HOSTS: &str = "hosts";
/// D-JOS-VMTEST1: a `VmTest`'s host map, same host shape as `Fleet`.
pub const VMTEST_FIELD_HOSTS: &str = "hosts";
/// D-JOS-VMASSERT1: a `VmTest`'s typed assertion body.
pub const VMTEST_FIELD_RUN: &str = "run";

/// D-JETOS-FREEZE1: frozen element type of a `System`'s `services:` map.
/// `Service` is not a top-level namespace (it never appears as `service.<name>:`);
/// it is the inferred type of each bare `{ … }` record written under `services:`.
pub const TYPE_SERVICE: &str = "Service";

/// D-JETOS-FREEZE1: frozen jetos sketch fields kept only for legacy
/// parser/evaluator coverage while `system.*` is outside current syntax law.
pub const SYSTEM_FIELD_TARGET: &str = "target";
pub const SYSTEM_FIELD_PACKAGES: &str = "packages";
pub const SYSTEM_FIELD_SERVICES: &str = "services";
pub const SYSTEM_FIELD_OPTIONS: &str = "options";

/// D-JPK-SERVICE1: the required first field of every `Service` record.
pub const SERVICE_FIELD_ENABLE: &str = "enable";

/// D-JPK-SERVICE1 (supervised-services slice, card c9jetpackgates):
/// the recognized fields of a **dev-supervised** `Service` (an entry under an
/// `env.<name>` role-module's `services:` map). `Service` stays the one
/// ratified open record either way (same grammar as `system.*.services`,
/// `SYSTEM_FIELD_SERVICES`/`SERVICE_FIELD_ENABLE` reused verbatim) — only the
/// dev-runtime tier (`Jetpack::Services`) interprets these particular keys,
/// to start/probe/stop the supervised process: `ports` (the `[Int]` TCP ports
/// it listens on), `init` (the shell command that starts it), `shutdown` (the
/// shell command that stops it, else a plain signal), `data_dir` (its
/// persisted-state directory, else `.jet/services/<name>/data`), and `ready`
/// (a shell command polled until it exits 0 — the readiness contract, else a
/// TCP probe on `ports[0]`, else a bare process-alive check).
pub const DEV_SERVICE_FIELD_PORTS: &str = "ports";
pub const DEV_SERVICE_FIELD_INIT: &str = "init";
pub const DEV_SERVICE_FIELD_SHUTDOWN: &str = "shutdown";
pub const DEV_SERVICE_FIELD_DATA_DIR: &str = "data_dir";
pub const DEV_SERVICE_FIELD_READY: &str = "ready";

/// D-JPK-PLATFORM1: the typed platform values a `System.target` (and a
/// cross-compile `Image.target`) may hold — `linux.x64` / `linux.arm64`. Written
/// as a dotted typed value (an OS namespace `.` an arch), never a quoted string.
pub const PLATFORM_OS_LINUX: &str = "linux";
pub const PLATFORM_ARCH_X64: &str = "x64";
pub const PLATFORM_ARCH_ARM64: &str = "arm64";

/// D-JPK-IMAGE1: an `Image`'s fields — required `from: system.<name>`
/// and optional `format:` (default `iso`). `target`/`packages`/`services`/
/// `options` are inherited from the referenced `System`, never restated (the lone
/// exception is an explicit cross-compile `target:`).
pub const IMAGE_FIELD_FROM: &str = "from";
pub const IMAGE_FIELD_FORMAT: &str = "format";

/// D-JPK-IMAGE1: the disk-image formats — `iso` (default) / `qcow` /
/// `raw`.
pub const IMAGE_FORMAT_ISO: &str = "iso";
pub const IMAGE_FORMAT_QCOW: &str = "qcow";
pub const IMAGE_FORMAT_RAW: &str = "raw";

/// D-JPK-IMAGE1 (=A, ratified 2026-07-01, c9jetpackgates): the keyword after
/// `from:` that selects the OCI-container referent (`from: packages.<name>`,
/// the sibling of the original `from: system.<name>`, `NS_SYSTEM`).
pub const IMAGE_FROM_PACKAGES: &str = "packages";

/// D-JPK-IMAGE1: an `Image`'s optional `kind:` — a leading-dot enum literal
/// (`.Oci`/`.Iso`, D-ENUMDOT2) picking which referent `from:` names. Omitted,
/// it infers from `from:` itself (`system.*` → Iso, `packages.*` → Oci); written,
/// it must agree with `from:` or name a real kind (E1266 `image-unknown-kind`).
pub const IMAGE_KIND_ISO: &str = "Iso";
pub const IMAGE_KIND_OCI: &str = "Oci";

/// D-JPK-IMAGE1: the `.Oci`-only fields — exposed TCP ports (`[Int]`), env vars
/// (a `[KEY: "value"]` map), extra files layered into the image (`[String]`
/// project-relative paths), and an optional base image escape hatch
/// (`base: oci("<ref>")`, unrealized — no registry-pull client yet, D-JPK-
/// RINGSHIP1/D-JPK-BUILDTOOL1 territory, honestly gated rather than faked).
pub const IMAGE_FIELD_KIND: &str = "kind";
pub const IMAGE_FIELD_EXPOSE: &str = "expose";
pub const IMAGE_FIELD_ENV_VARS: &str = "env_vars";
pub const IMAGE_FIELD_FILES: &str = "files";
pub const IMAGE_FIELD_BASE: &str = "base";
/// The `oci(...)` call name inside `base: oci("<ref>")`.
pub const IMAGE_BASE_FN: &str = "oci";

/// U3 (ratified 2026-06-16): project environment file (`env` namespace) and the
/// master jetos system config (`system`/`image` namespaces, default dir ~/.jet/).
pub const ENV_FILE: &str = "env.jet";
pub const CONFIG_FILE: &str = "config.jet";

/// D-WORKSPACE1 (B) + D-WORKSPACE2 (A), ratified 2026-06-25: the monorepo index
/// is a `module workspace { members: … }` written in `workspace.jet`, parallel to
/// `env.jet`/`config.jet` — retiring the root `jetpack.toml` index so the whole
/// project is one grammar (Jet). `members:` may run arbitrary `comptime`
/// (D-WORKSPACE1=B). Wired by the resolver (board card c156). `NS_WORKSPACE` is
/// declared with the other reserved namespaces near `NS_ENV`.
pub const WORKSPACE_FILE: &str = "workspace.jet";
/// D-JPK-OVERLAY1=A: reviewed workspace overlay blocks.
pub const WORKSPACE_OVERLAY: &str = "overlay";
/// D-WORKSPACE1=B: the `members:` field in a workspace module — the comptime
/// expression that evaluates to the list of member package paths.
pub const MODULE_FIELD_MEMBERS: &str = "members";

/// D-JPK-OSVERB1=A (ratified 2026-07-06): the public jetos CLI surface is
/// `jet os <verb>`. The engine still executes in the `jetpack` process via
/// D-JPK-DISPATCH1, but users type `jet os`, not `jetpack os`.
pub const OS_SUBCOMMAND: &str = "os";

/// D-JPK-OSVERB1=A: public jetos verbs.
pub const OS_VERB_CHECK: &str = "check";
pub const OS_VERB_INIT: &str = "init";
pub const OS_VERB_SWITCH: &str = "switch";
pub const OS_VERB_BUILD: &str = "build";
/// D-JOS-PROOFAPI1=B: read the exact checked plan without building.
pub const OS_VERB_PLAN: &str = "plan";
/// D-JOS-PROOFAPI1=B: read proof/provenance artifacts for the latest generation.
pub const OS_VERB_PROOF: &str = "proof";
pub const OS_VERB_ROLLBACK: &str = "rollback";
pub const OS_VERB_GENERATIONS: &str = "generations";
pub const OS_VERB_LIFT: &str = "lift";
/// D-JOS-NIXIMPORT1=C: import semantic NixOS/flake-parts/Home Manager facts.
pub const OS_VERB_IMPORT: &str = "import";
pub const OS_VERB_IMAGE: &str = "image";
/// D-JOS-VMCOMMAND1=A: `jet os vm prove` runs installer/reboot proof.
pub const OS_VERB_VM: &str = "vm";
/// D-JOS-NIXIMPORT1=C: select the NixOS host to import.
pub const OS_IMPORT_FLAG_HOST: &str = "--host";
/// D-JOS-NIXIMPORT1=C: select Home Manager users to import. Repeatable.
pub const OS_IMPORT_FLAG_USER: &str = "--user";
/// D-JOS-NIXIMPORT1=C: write generated JetOS config/audit files.
pub const OS_IMPORT_FLAG_WRITE: &str = "--write";
/// D-JOS-NIXIMPORT1=C: write generated files to this directory or config path.
pub const OS_IMPORT_FLAG_OUT: &str = "--out";
/// D-JOS-NIXIMPORT1=C: force audited scan mode when no semantic facts exist.
pub const OS_IMPORT_FLAG_FACTS_ONLY: &str = "--facts-only";
/// D-JOS-VMCOMMAND1=A: non-interactive VM install/reboot proof action.
pub const OS_VM_ACTION_PROVE: &str = "prove";
/// D-JOS-VMRUN1=A: interactive launch of a proved installed VM disk.
pub const OS_VM_ACTION_RUN: &str = "run";
/// D-JOS-VMTEST1=A: run a declared VM scenario and write proof artifacts.
pub const OS_VM_ACTION_TEST: &str = "test";
/// D-JOS-REALGUEST1=C: require real VM tools for replacement acceptance.
pub const OS_VM_FLAG_REAL: &str = "--real";
/// D-JOS-STUDIO-LAUNCH1=A / D-JOS-STUDIO-HOST1=A: `jetos studio`.
pub const STUDIO_SUBCOMMAND: &str = "studio";
/// D-JOS-USERAPPLY1=A: standalone user-profile management entrypoint.
pub const USER_SUBCOMMAND: &str = "user";
/// D-JOS-USERAPPLY1=A: standalone user-profile verbs.
pub const USER_VERBS: &[&str] = &["plan", "build", "switch", "rollback", "prove"];
/// D-JOS-STUDIO-HOST1=A: headless review mode over the same local protocol.
pub const STUDIO_FLAG_HEADLESS: &str = "--headless";
/// D-JOS-STUDIO-HOST1=A: serve browser fallback over local projection protocol.
pub const STUDIO_FLAG_SERVE: &str = "--serve";
/// D-JOS-STUDIO-HOST1=A: select the system host Studio projects/edits.
pub const STUDIO_FLAG_HOST: &str = "--host";
pub const OS_VERBS: &[&str] = &[
    OS_VERB_CHECK,
    OS_VERB_INIT,
    OS_VERB_PLAN,
    OS_VERB_PROOF,
    OS_VERB_BUILD,
    OS_VERB_SWITCH,
    OS_VERB_ROLLBACK,
    OS_VERB_GENERATIONS,
    OS_VERB_LIFT,
    OS_VERB_IMPORT,
    OS_VERB_IMAGE,
    OS_VERB_VM,
    STUDIO_SUBCOMMAND,
    USER_SUBCOMMAND,
];

/// c146 (D-PKGSIGN1, ratified): package-signing CLI verbs (I7). `jet registry keygen`
/// creates the Ed25519 author key; `jet registry key backup` copies the secret key out
/// for safekeeping. `jet registry publish` signs by default and takes `--no-sign`.
pub const KEYGEN_SUBCOMMAND: &str = "keygen";
pub const KEY_SUBCOMMAND: &str = "key";
pub const KEY_VERB_BACKUP: &str = "backup";
pub const KEY_VERBS: &[&str] = &[KEY_VERB_BACKUP];
pub const PUBLISH_FLAG_NO_SIGN: &str = "--no-sign";

/// D-JPK-OSHOST1=C: a bare host discovers `system.<host>` in the current repo;
/// `path@host` selects an exact external repo/config root.
pub const OS_HOST_SELECTOR: &str = "@";

/// D-JPK-OSHOST1=C: current-repo/external-root config filename.
pub const CONFIG_DEFAULT_DIR: &str = ".jet";

/// D-JPK-OSGEN1=C: switch may override the generated generation name.
pub const OS_FLAG_NAME: &str = "--name";

/// D-JPK-OSDISK1=C: installer/init accepts a manual disk path override.
pub const OS_FLAG_MANUAL_DISK: &str = "--manual";
/// D-JOS-VMCOMMAND1=A: VM proof target disk path.
pub const OS_FLAG_DISK: &str = "--disk";

/// D-FE-CLI1=D: consequence-scaled CLI output accepts both spellings for
/// bypassing mutation confirmation gates.
pub const CLI_FLAG_YES_SHORT: &str = "-y";
pub const CLI_FLAG_YES_LONG: &str = "--yes";

/// D-JPK-OSNS1=B: full-word option namespaces.
pub const OS_OPTION_NS_FILESYSTEM: &str = "filesystem";
pub const OS_OPTION_NS_NETWORK: &str = "network";
pub const OS_OPTION_NS_PACKAGES: &str = "packages";
/// D-JOS-SYSTEMTREE1=A: standard full-word jetos option namespaces.
pub const OS_OPTION_NS_SERVICES: &str = "services";
pub const OS_OPTION_NS_USERS: &str = "users";
pub const OS_OPTION_NS_GROUPS: &str = "groups";
pub const OS_OPTION_NS_SECRETS: &str = "secrets";
pub const OS_OPTION_NS_BOOT: &str = "boot";
pub const OS_OPTION_NS_KERNEL: &str = "kernel";
pub const OS_OPTION_NS_INIT: &str = "init";
pub const OS_OPTION_NS_HEALTH: &str = "health";
/// D-JOS-USERENV1=A: per-user environment declarations can appear as `user.*`
/// option projections while the role-module surface is being realized.
pub const OS_OPTION_NS_USER: &str = "user";
/// D-JOS-FLATPAK1=A: first-party foreign app ecosystem declarations.
pub const OS_OPTION_NS_APPS: &str = "apps";
/// D-JOS-KERNELTUNE1=A: performance and kernel-tuning profile declarations.
pub const OS_OPTION_NS_PERFORMANCE: &str = "performance";
/// D-JOS-DISK1=A: storage tree declarations consumed by installer and activation.
pub const OS_OPTION_NS_STORAGE: &str = "storage";
/// D-JOS-THEME1=A: reusable theme profile projection.
pub const OS_OPTION_NS_THEME: &str = "theme";
/// D-JOS-CONTAINER1=A: isolated workload declarations.
pub const OS_OPTION_NS_WORKLOAD: &str = "workload";
/// D-JOS-HARDWARE1=A: hardware scan/profile/specialisation declarations.
pub const OS_OPTION_NS_HARDWARE: &str = "hardware";
/// D-JOS-FLEETTARGET1=A / D-JOS-FLEETROLLOUT1=A: deploy target/rollout facts.
pub const OS_OPTION_NS_DEPLOY: &str = "deploy";
pub const OS_OPTION_NAMESPACES: &[&str] = &[
    OS_OPTION_NS_FILESYSTEM,
    OS_OPTION_NS_NETWORK,
    OS_OPTION_NS_PACKAGES,
    OS_OPTION_NS_SERVICES,
    OS_OPTION_NS_USERS,
    OS_OPTION_NS_GROUPS,
    OS_OPTION_NS_SECRETS,
    OS_OPTION_NS_BOOT,
    OS_OPTION_NS_KERNEL,
    OS_OPTION_NS_INIT,
    OS_OPTION_NS_HEALTH,
    OS_OPTION_NS_USER,
    OS_OPTION_NS_APPS,
    OS_OPTION_NS_PERFORMANCE,
    OS_OPTION_NS_STORAGE,
    OS_OPTION_NS_THEME,
    OS_OPTION_NS_WORKLOAD,
    OS_OPTION_NS_HARDWARE,
    OS_OPTION_NS_DEPLOY,
];

/// U4 (ratified 2026-06-16): import-tree discovery builtin — `find("./modules")`
/// auto-discovers and merges every `.jet` module in the tree.
pub const BUILTIN_FIND: &str = "find";

/// U8 (ratified 2026-06-16): `sources:` and `imports:` are module-body fields,
/// nested inside `module name { … }` as siblings of the typed contributions
/// (`env.dev: Env { … }`) — not file top-level fields. Amends U4. `sources:`
/// holds `name: provider@target` entries; `imports:` holds `find(…)` directives.
pub const MODULE_FIELD_SOURCES: &str = "sources";
pub const MODULE_FIELD_IMPORTS: &str = "imports";

/// U6/U8: the conventional name of the default source (`sources: { default: … }`)
/// that bare packages and `default.ripgrep` sugar resolve against. Not a
/// reserved keyword — just the well-known name `jetpack` falls back to.
pub const DEFAULT_SOURCE: &str = "default";

/// U3/U8: the `Env` contribution field carrying the shell prompt label.
pub const ENV_FIELD_PROMPT: &str = "prompt";
/// D-FE-PROMPT-STRIP1: `Prompt.{ label: "...", path: .Short, strip: .On }`.
pub const PROMPT_FIELD_LABEL: &str = "label";
pub const PROMPT_FIELD_PATH: &str = "path";
pub const PROMPT_FIELD_STRIP: &str = "strip";
pub const PROMPT_SETTING_PATH: &str = "prompt.path";
pub const PROMPT_SETTING_STRIP: &str = "prompt.strip";
pub const PROMPT_PATH_SHORT: &str = "Short";
pub const PROMPT_PATH_FULL: &str = "Full";
pub const PROMPT_STRIP_ON: &str = "On";
pub const PROMPT_STRIP_OFF: &str = "Off";

/// U6 (ratified 2026-06-16): package value type, and the `provider@target`
/// source-ref separator (`github@owner/repo/rev`, `path@../local`, `nixpkgs@…`).
/// Provider names reuse REF_SOURCE_* (github / path / nixpkgs).
pub const TYPE_PKG: &str = "Pkg";
pub const REF_PROVIDER_AT: &str = "@";

/// U10 (ratified 2026-06-16; amends U1) / D-JPK-FILES (ratified 2026-06-18;
/// amends U10): the package manifest is `pkg.jet` (D-JPK-FILES rename; prior
/// interim names retired, clean break, no alias). A payload is a collection
/// of packages; its identity block is `payload: { … }`.
pub const PAYLOAD_FILE: &str = "pkg.jet";

/// D-JPK-FILENAME2=B (A2, card c9jetpackgates, ratified 2026-07-02): retired
/// manifest filenames from earlier reshapes of this same file (U1 `jet.toml`
/// -> U10 `pack.jet` -> D-JPK-FILES `pkg.jet`). Finding one of these instead
/// of `PAYLOAD_FILE` is E1226, not a silent fallback — D-JPK-FILENAME2
/// reconfirmed `pkg.jet` as final, so these never come back as aliases.
/// `jetpack.toml` is a *different*, still-live file (D-JPK-FILES repo
/// metadata: `[repo]`/`[sources]`) and does not belong on this list.
pub const STALE_MANIFEST_NAMES: &[&str] = &["pack.jet", "payload.jet", "jet.toml"];

/// U10 (ratified 2026-06-16): manifest identity block keyword — `payload: { name,
/// version, … }` (was `package:`).
pub const MANIFEST_BLOCK_PAYLOAD: &str = "payload";

/// U10 (ratified 2026-06-16): the block listing a payload's packages —
/// `packages: { name: kind }`. Each `name` is a top-level `module` (the package),
/// discovered by name in the tree; the old `exports: [module …]` folds into this.
pub const MANIFEST_BLOCK_PACKAGES: &str = "packages";

/// D-TGT1/D-TGT2 (ratified 2026-06-21): a package's build targets, replacing the
/// removed `kind:` (U10). The six shipped targets — `library` is imported for its
/// code, `executable` installs a binary on PATH, `test`/`example` build their own
/// artifacts, `benchmark` (c80, D-TGT2) points `jet bench` at the package entry,
/// `plugin` (c81, D-PLUGIN1/D-DEP-WASM1) builds a sandboxed WASM Component Model
/// module. Written as a bare keyword (`deploy: executable`, D-TGT3) or inside a
/// `{ targets: [ … ] }` list.
pub const TARGET_LIBRARY: &str = "library";
pub const TARGET_EXECUTABLE: &str = "executable";
pub const TARGET_TEST: &str = "test";
pub const TARGET_EXAMPLE: &str = "example";
/// D-TGT2 / c80 (ratified 2026-06-21; backend shipped 2026-06-25): the manifest
/// target that routes `jet bench` at the package entry — same engine as `@Bench`/
/// `jet bench file.jet`, now addressable from a `packages:` declaration.
pub const TARGET_BENCHMARK: &str = "benchmark";
/// D-PLUGIN1=B / D-DEP-WASM1=A (ratified 2026-06-25; backend shipped c81): a
/// package built as `plugin` compiles to a sandboxed `wasm32` Component Model
/// module (wasmtime host, typed `.wit` contract) instead of a native binary.
/// Safe by default — no `@Unsafe` gate (I1 holds by construction: the sandbox
/// is the safety boundary). Its exported surface is named by the `export:`
/// target field (`TARGET_FIELD_EXPORT`, D-PLUGIN-EXPORT1).
pub const TARGET_PLUGIN: &str = "plugin";

/// D-TGT2 (ratified 2026-06-21): target keywords reserved for a future increment —
/// recognized but rejected (no backend yet) until their tooling lands.
/// `benchmark` shipped (c80); `plugin` shipped (c81). Empty until the next
/// reserved target is proposed.
pub const TARGET_RESERVED: &[&str] = &[];

/// D-TGT1 (ratified 2026-06-21): the per-package field listing build targets —
/// `app: { targets: [library, executable { entry: "src/cli.jet" }] }`. A bare
/// keyword value (`app: library`) is the single-target shorthand (D-TGT3). The
/// former `kind:` field is removed; using it is a teaching error (E1211).
pub const PACKAGE_FIELD_TARGETS: &str = "targets";

/// D-TGT1 (ratified 2026-06-21): the removed per-package kind field. Recognized
/// only to emit a migration teaching error pointing at `targets:`.
pub const PACKAGE_FIELD_KIND_REMOVED: &str = "kind";

/// D-TGT3/D-TGT4 (ratified 2026-06-21): fields a target block may carry —
/// `entry:` (D-TGT4 entry module), `name:` (output/bin name). Parsed when
/// present; behavior lands with the realize pipeline. `api:` (D-CAP4) is
/// retired by D-MEM1/S2 — a target block carrying it hits the ordinary
/// unknown-field error like any other typo'd key.
pub const TARGET_FIELD_ENTRY: &str = "entry";
pub const TARGET_FIELD_NAME: &str = "name";
/// D-PLUGIN-EXPORT1=A (ratified 2026-06-25): names a `plugin` target's exported
/// surface (the `.wit` world name). Only meaningful on `plugin { export: "…" }`;
/// defaults to the package name when omitted.
pub const TARGET_FIELD_EXPORT: &str = "export";

/// D-CAP1 (ratified 2026-06-21): the four-capability vocabulary —
/// `view`/`edit`/`take`/`share`. `view` and `take` are ratified ownership keywords
/// (S10); `edit` and `share` are reserved here. Parameter-position placement
/// (D-CAP3) and the copy/share call form (D-CAP2) are still open, so these are
/// reserved spellings only — not yet wired into the parser.
pub const CAPABILITY_EDIT: &str = "edit";
pub const CAPABILITY_SHARE: &str = "share";

/// D-REL3 (ratified 2026-06-16): the project compatibility marker —
/// `edition: "2026"` in the `payload: { … }` block of `pkg.jet`. An edition
/// opts a project into a specific era of Jet syntax; a toolchain advertises the
/// editions it supports and rejects a future edition it can't provide (E2001).
/// Single-file `jet run file.jet` has no edition marker and always uses the
/// newest stable edition (E2-V4). Not an `S`/`N`/`U` surface decision, so it is
/// not enforced by tests/decisions.rs; it is a release-policy key recorded here
/// per I7.
pub const MANIFEST_FIELD_EDITION: &str = "edition";

/// D-RINGLAYER1=A: optional package runtime-layer ceiling in `payload: { … }`.
pub const MANIFEST_FIELD_LAYER: &str = "layer";
