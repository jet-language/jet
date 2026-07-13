/// D-TOOL2 (ratified 2026-06-17, E2-M11; PascalCase marker D-CASING1 follow-on
/// 2026-06-21): typed hole `#Todo` — compiles everywhere, panics at runtime with
/// file, line, and expected type. Bare lowercase `todo` (FOREIGN_TODO) is the
/// retired spelling → E0054 teaching error pointing at `#Todo`.
pub const KW_TODO: &str = "Todo";

/// S60 (ratified 2026-06-12; implemented E2-M16; PascalCase marker D-CASING1
/// follow-on 2026-06-21): the purity modifier, written as the marker `@Pure fn
/// name() { … }`. A `@Pure fn` may only call other `@Pure fn`s and pure
/// builtins; impure calls are a compile error (E3401) with the call-trace path.
/// Bare lowercase `pure` (FOREIGN_PURE) is the retired spelling → E0053 teaching
/// error pointing at `@Pure`.
///
/// D-EFF2 (ratified 2026-06-22): `@Pure` also rides the front of a callback
/// parameter type — `f: @Pure fn(T) -> U` demands a pure callback; passing one
/// with any effect is E0747. Sibling of the `#(E, …) fn(…)` bounded form.
pub const KW_PURE: &str = "Pure";

/// D-TAINT1 (ratified 2026-06-21, option A; gated on D-EFF1): the value-fact tag
/// that marks an untrusted value at its source — `#Tainted input`. The taint
/// **spreads** along dataflow (assignment, interpolation, field store, return,
/// arithmetic); a tainted value reaching a sink effect (`Db`/`Exec`/`Net`)
/// without passing through a `#Sanitizer fn` is E0721. A value fact, not a
/// declaration: it rides the value (D-QUAL1). PascalCase per D-CASING1 (the
/// ratified card's lowercase `#tainted` is normalized to the tag convention).
/// Static, erased in codegen (I3).
pub const KW_TAINTED: &str = "Tainted";

/// D-TAINT1: the `#Sanitizer fn name(…)` modifier — the one blessed way to strip
/// taint. A sanitizer's return value is untainted by contract, regardless of
/// whether its inputs were tainted (it is the audited cleaning step). A fn
/// modifier in the `@Pure`/`#Unsafe` family; PascalCase per D-CASING1. Erased in
/// codegen (I3). NOTE: the ratified card spells the modifier bare `sanitizer fn`;
/// the D-CASING1 marker convention (which moved `pure fn` → `@Pure fn`) makes
/// `#Sanitizer fn` the consistent default — a spelling fork queued as D-TAINT-SAN.
pub const KW_SANITIZER: &str = "Sanitizer";

/// D-STATE1 (ratified 2026-06-22, option A): the typestate **require-state** fn
/// modifier — `#State(Confirmed) fn check_in(self, …)`. Declares the method valid
/// only when its receiver is currently in state `Confirmed`. Calling it on a value
/// in any other state is E0150. The state is an ordinary `tag` (D-QUAL2); the
/// current state of a value is a compile-time fact threaded by forward dataflow,
/// erased in codegen (I3 — zero runtime cost). A paren-arg fn marker, parallel to
/// `#layout(c)` / `#UnitFamily(currency)`. The exact spelling is the implemented
/// default queued for owner confirmation as D-STATE-REQ.
pub const KW_STATE: &str = "State";

/// D-STATE1 (ratified 2026-06-22, option A): the typestate **transition** fn
/// modifier — `#Transition(Pending -> Confirmed) fn confirm(self) -> Reservation`.
/// Declares a function that consumes a value in state `Pending` and yields one in
/// state `Confirmed` (the ratified mechanism: "a fn takes the old state tag and
/// returns the next"). The from-state may be `_` for an **entry** transition (a
/// constructor that produces the initial state from nothing). Wrong from-state at a
/// call site is E0150; the call advances the receiver/result to the to-state. The
/// `->` inside reuses the return arrow. Tags erase (I3). Implemented default queued
/// for owner confirmation as D-STATE-TRANS.
pub const KW_TRANSITION: &str = "Transition";

/// D-STATE-DECL (ratified 2026-06-25, option B): the typestate **state-set
/// declaration** contextual keyword — `state TypeName { Pending, Confirmed, CheckedIn }`.
/// Declares the bounded set of states for a type, tied to the type by name. The set
/// erases at runtime (pure compile-time, no discriminant). A dead-end state (no
/// outgoing `#Transition`) is a warning (L0151). A state referenced in `#State(X)` or
/// `#Transition(A -> B)` that is not in the declared set is E0151. Contextual: the
/// word `state` stays usable as an ordinary identifier outside a top-level declaration
/// position (like `migration`). Declaration family sibling of `tag`/`struct`/`enum`.
pub const KW_STATE_DECL: &str = "state"; // D-STATE-DECL

/// D-PROTO1 / D-PROTO2 (ratified 2026-06-27, options A+A): the session/protocol
/// declaration contextual keyword — `protocol Name { client -> server: Msg(…) }`.
/// Declares an ordered request/response exchange once; sema expands it into
/// `#SingleUse` `.Client`/`.Server` handle types with typestate-checked send/recv
/// methods (out-of-order use = E0150). Contextual like `state`/`migration`.
pub const KW_PROTOCOL: &str = "protocol"; // D-PROTO1, D-PROTO2

/// D-PROTO2: endpoint labels in a protocol message line.
pub const PROTO_CLIENT: &str = "client"; // D-PROTO2
pub const PROTO_SERVER: &str = "server"; // D-PROTO2

/// D-STATE1: the entry-transition placeholder — `#Transition(_ -> Pending)` means
/// "from no prior state". Reuses the existing `_` wildcard glyph.
pub const STATE_ENTRY: &str = "_";

/// D-EFF1 / D-QUAL1 (ratified 2026-06-22): the effect-restriction region marker,
/// written `#Caps(Net, Db) { … }`. Inside the block, the body (and everything it
/// transitively calls) may use only the listed effects; an out-of-set effect is
/// E0741. PascalCase per D-CASING1. Erased in codegen (I3).
pub const KW_CAPS: &str = "Caps";

/// D-SCAP1 (ratified 2026-06-21): the scoped-capability grant marker, written
/// `#grant(Fs) { caps -> … }`. Grants (authorizes) the listed effect(s) inside
/// the block through the first-class handle bound after `{` (here `caps`), and
/// **revokes** the capability at scope end (RAII, S63) — the handle is bound only
/// for the block. The dual of `#Caps` (which restricts): an effect used inside
/// that the grant doesn't cover has no capability (E0712); letting the handle
/// escape is E0711. Erased in codegen (I3). PascalCase per D-MARKERCASE1=A.
pub const KW_GRANT: &str = "Grant";

/// D-SCAP1: the `->` token between the grant handle and the block body —
/// `#grant(Fs) { caps -> … }`.
pub const GRANT_ARROW: &str = "->";

/// D-SCAP1: the type of a capability handle bound by `#grant(…) { caps -> … }`.
/// An opaque sema-only handle (authority to perform the granted effects); erased
/// in codegen (I3). Mirrors `TXN_HANDLE_TYPE`.
pub const CAP_HANDLE_TYPE: &str = "Capability";

/// D-TASKSCOPE1=A / D-NURSERY1=A: the sema-only handle type bound by
/// `taskgroup g { … }`. Erased in codegen (I3); routes `g.task` / `g.all`.
pub const TYPE_TASKGROUP: &str = "TaskGroup";

/// D-TASKSCOPE1=A: scoped spawn method on a taskgroup handle — `g.task { … }`.
pub const TASKGROUP_SPAWN_METHOD: &str = "task";

/// D-NURSERY1=A: join every task handle in a list — `g.all([h1, h2])`.
pub const TASKGROUP_ALL_METHOD: &str = "all";

/// D-CONCCOMB1=A: first completed task wins — `g.race([h1, h2])`.
pub const TASKGROUP_RACE_METHOD: &str = "race";

/// D-CONCCOMB1=A: first completed result — `g.any([h1, h2])` (v1: same join race).
pub const TASKGROUP_ANY_METHOD: &str = "any";

/// D-CONCSELECT1=A: fluent scoped select — `g.select().recv(...).after(...).wait()?`.
pub const TASKGROUP_SELECT_METHOD: &str = "select";

/// D-CONCSELECT1=A: sema/codegen builder type for chained select arms.
pub const TYPE_SELECT_BUILDER: &str = "SelectBuilder";

/// D-CONCSELECT1=A: register a channel receive arm on a select builder.
pub const SELECT_RECV_METHOD: &str = "recv";

/// D-CONCSELECT1=A: register a timer arm — `.after(ms: N)`.
pub const SELECT_AFTER_METHOD: &str = "after";

/// D-CONCSELECT1=A: register a readable I/O arm — `.read(stream)`.
pub const SELECT_READ_METHOD: &str = "read";

/// D-CONCSELECT1=A: block until one arm wins — `.wait()`.
pub const SELECT_WAIT_METHOD: &str = "wait";

/// D-NURSERY1=A: wait for a task result (alias for `.join()` on `Task<T>`).
pub const METHOD_TASK_WAIT: &str = "wait";
/// D-COROUTINE1=A: mark a task paused in the control plane.
pub const METHOD_TASK_PAUSE: &str = "pause";
/// D-COROUTINE1=A: clear the paused marker in the control plane.
pub const METHOD_TASK_RESUME: &str = "resume";
/// D-COROUTINE1=A: request cancellation for a task in the control plane.
pub const METHOD_TASK_CANCEL: &str = "cancel";
/// D-COROUTINE1=A: inspect task control-plane state.
pub const METHOD_TASK_TRACE: &str = "trace";

/// D-TXN4 (ratified 2026-06-24): the transaction-block marker, written
/// `#Transact(order) { … }`. `order` binds a user-chosen transaction handle
/// (any lowercase ident, mirroring `region r { … }`). Inside the block an
/// irreversible effect (Net/Fs/Exec) is rejected (E0746, D-TXN2); the fix is to
/// move it after the block or register it on the handle via
/// `order.on_commit(() => { … })` (D-TXN3), which runs Drop-backed on a clean
/// commit. PascalCase per D-CASING1. Erased in codegen (I3).
pub const KW_TRANSACT: &str = "Transact";

/// D-TXN3 (ratified 2026-06-24): the post-commit hook method on a transaction
/// handle — `order.on_commit(() => { … })`. Drop-backed (D-DEFER1 model), runs
/// LIFO on a clean commit and is dropped (not run) on a `?`-failure/rollback.
/// NO new keyword (library form, I7 untouched).
pub const TXN_ON_COMMIT: &str = "on_commit";

/// D-TXN-ROLLBACK (ratified 2026-06-25, layer 3): the explicit rollback-hook
/// method on a transaction handle — `order.on_rollback(() => { … })`. The exact
/// mirror of `on_commit`: Drop-backed (D-DEFER1 model), runs LIFO on a
/// `?`-failure/rollback and is dropped (not run) on a clean commit. A value handled
/// by an explicit `on_rollback` is the author's to undo, so it is NOT auto-snapshot
/// (layer 1) — they took control and skip the perf cost. NO new keyword (library
/// form, I7 untouched).
pub const TXN_ON_ROLLBACK: &str = "on_rollback";

/// D-TXN-ROLLBACK (ratified 2026-06-25, layer 2): the trait a type may derive/impl
/// to customize how a mutated value is snapshotted and restored inside a `#Transact`
/// block (e.g. a cheap diff instead of a full deep copy). When a mutated value's
/// type implements `Rollback`, the auto-snapshot (layer 1) uses it instead of a
/// generic clone. A user-derivable trait name (I7).
pub const TRAIT_ROLLBACK: &str = "Rollback";

/// D-DISPLAYDBG1 / D-DISPLAY-SHAPE: user-facing string rendering for `{}` interpolation.
pub const TRAIT_DISPLAY: &str = "Display";
/// D-DISPLAYDBG1: developer-facing debug rendering for `{value@Debug}` interpolation.
pub const TRAIT_DEBUG: &str = "Debug";
/// D-ITER-HOOK: expert opt-in hook enabling zero-copy `for x in mytype`.
pub const TRAIT_ITERABLE: &str = "Iterable";
/// D-ITER-HOOK: cursor type for `Iterable::iter`.
pub const TRAIT_ITERATOR: &str = "Iterator";
/// D-INDEX-HOOK: expert opt-in hook enabling `mytype[key]` read syntax.
pub const TRAIT_INDEX: &str = "Index";
/// D-INDEX-HOOK: expert opt-in hook enabling `mytype[key] = v` write syntax.
pub const TRAIT_INDEX_MUT: &str = "IndexMut";
/// D-NETIO-CONTRACT2=B: nominal byte-stream read contract in `core.io`.
pub const TRAIT_IO_READER: &str = "Reader";
/// D-NETIO-CONTRACT2=B: nominal byte-stream write contract in `core.io`.
pub const TRAIT_IO_WRITER: &str = "Writer";
/// D-DISPLAYDBG2: closed interpolation selector spelling after `@`.
pub const INTERP_SELECTOR_DEBUG: &str = "Debug";
/// D-DEBUG-REDACT / D-MARKERMOVE1 (contract plane, `@Redact`): hide a field
/// from auto-derived Debug output.
pub const ATTR_REDACT: &str = "Redact";

/// D-TXN4: the type of a transaction handle bound by `#Transact(name)`. An
/// opaque sema-only handle; erased in codegen (I3).
pub const TXN_HANDLE_TYPE: &str = "Transaction";

/// S14 / D-CASING1 follow-on (2026-06-21): the retired lowercase spellings of
/// the three marker keywords, recognized only for teaching errors that point at
/// the `#Test` / `@Pure` / `#Todo` marker forms.
pub const FOREIGN_TEST: &str = "test";
pub const FOREIGN_PURE: &str = "pure";
pub const FOREIGN_TODO: &str = "todo";

/// D-TAINT-SAN (ratified 2026-06-25, option B): the taint-strip modifier is the
/// PascalCase marker `#Sanitizer fn`. Bare lowercase `sanitizer` in fn-modifier
/// position (`sanitizer fn …`) is the retired spelling, recognized only for the
/// teaching error E0059 that points at `#Sanitizer`. An ordinary identifier named
/// `sanitizer` elsewhere is unaffected.
pub const FOREIGN_SANITIZER: &str = "sanitizer";

/// D-LIN1-DROP (ratified 2026-06-25, option A): `drop(x)` is the deliberate
/// discard of a `#SingleUse` value. Legal ONLY inside an `#Unsafe("reason")`
/// region/fn — the `#Unsafe` reason IS the audit note (reuses D-UNSAFE2's audited
/// gate). It satisfies the single-use consume duty by moving the value to nowhere;
/// the value's Rust `Drop` runs. Outside an `#Unsafe` context it is E0143. Erased
/// to a plain `drop(x)` in codegen (I3 — no `unsafe` emitted). Shadowed by any
/// user-defined `drop` function or local.
pub const BUILTIN_DROP: &str = "drop";

/// D-TOOL4 (ratified 2026-06-16, E2-M11): snapshot testing builtin.
/// `expect(value).snapshot()` records or compares a golden snapshot.
pub const BUILTIN_EXPECT: &str = "expect";
pub const BUILTIN_SNAPSHOT: &str = "snapshot";

/// M4: synthetic name for a `switch` subject that isn't a plain identifier.
/// Foundational keyword predating the S-numbered decision log (card #447
/// KW_DECISION_ID_EXEMPT).
pub const KW_IT: &str = "it";

/// S42 (ratified M5): `as` recognized only for teaching error E0030.
pub const FOREIGN_AS: &str = "as";

/// S46 (M8): foreign anonymous-fn spellings for teaching error E0032.
pub const FOREIGN_LAMBDA: &str = "lambda";

/// S46 (M8): Rust pipe closures for teaching error E0033.
pub const FOREIGN_PIPE_CLOSURE: &str = "|";

/// S14 (M5): foreign collection spellings for teaching errors.
pub const FOREIGN_VEC: &str = "Vec";
pub const FOREIGN_DICT: &str = "dict";
pub const FOREIGN_APPEND: &str = "append";

/// S14 (M4): foreign error spellings for teaching errors.
pub const FOREIGN_THROW: &str = "throw";
pub const FOREIGN_RAISE: &str = "raise";
pub const FOREIGN_CATCH: &str = "catch";
pub const FOREIGN_EXCEPT: &str = "except";
pub const FOREIGN_UNWRAP: &str = "unwrap";
pub const FOREIGN_EXPECT: &str = "expect";

/// M10 teaching spellings for common Core/library habits.
pub const FOREIGN_EPRINTLN: &str = "eprintln";
pub const FOREIGN_OPEN: &str = "open";
pub const FOREIGN_GETENV: &str = "getenv";
pub const FOREIGN_OS: &str = "os";

/// M11 teaching spellings: async/await and mutex/lock.
pub const FOREIGN_ASYNC: &str = "async";
pub const FOREIGN_AWAIT: &str = "await";
pub const FOREIGN_MUTEX: &str = "Mutex";
pub const FOREIGN_LOCK: &str = "lock";

/// D-ATTR1 (ratified 2026-06-19): attribute prefix sigil — `#Marker` / `#[a, b]`.
/// Replaces S82's `@` spelling. Loop labels keep `@` (D-ATTR3 = B).
pub const ATTR_PREFIX: &str = "#";

/// S82 (ratified 2026-06-16): multi-attribute list delimiters after `@`.
pub const ATTR_LIST_OPEN: &str = "[";
pub const ATTR_LIST_CLOSE: &str = "]";

/// D-ATTR1: rejected old `@` attribute spelling (teaching error). S82 reversed.
pub const FOREIGN_AT_ATTR: &str = "@";

/// S80 (ratified 2026-06-16): cross-type `?` conversion trait (D-ERR2).
pub const TRAIT_FALLIBLE: &str = "Fallible";

/// S80 (ratified 2026-06-16): `Fallible` method returning default `Error`.
pub const FN_TO_ERROR: &str = "to_error";

// S52's `MANIFEST_FILE`/`LOCK_FILE` (`jet.toml`/`jet.lock`) were retired in the
// manifest reshape chunk (U1/U2): the manifest is now `PAYLOAD_FILE`
// (`pkg.jet`, D-JPK-FILES — prior filename iterations retired) and
// the lockfile is `UNIFIED_LOCK_FILE` (`.jet/lock`). Clean break — no alias.

/// S52 (ratified M12): package source root directory inside a project.
pub const SOURCE_ROOT_DIR: &str = ".jet";

/// S52 (ratified M12): dependency kind table suffixes.
pub const DEP_TABLE_JET: &str = "dependencies";
pub const DEP_TABLE_RUST: &str = "dependencies:rust";

/// S59 / D-CFFI2 (ratified): the native-C-library dependency provider name,
/// written as a `provider@target` ref inside the `deps: { … }` block —
/// `lib: c@system` (pkg-config, with a bare `-l <lib>` fallback) or
/// `lib: c@"vendor/path"` (local dir: `-L`/`-I`/`-l`). Replaces the retired
/// TOML `[dependencies:c]` table. A C dep is a link dep, not a Jet package: it
/// is never realized as source or written to the package lock.
pub const DEP_PROVIDER_C: &str = "c";

/// S59 / D-CFFI2 (ratified): the `c@<target>` system-library target —
/// `lib: c@system` resolves via `pkg-config <lib>`, falling back to a bare
/// `-l <lib>` when there is no `.pc` (e.g. libc). Any other target is a local
/// directory path.
pub const SYSTEM_LIB_TARGET: &str = "system";

// ──────────────────────────────────────────────
// Jetpack (Phase 1) — user-typeable surface (I7).
// All decisions ratified in docs/spec/syntax-decisions.md (D-JPK*).
// These IDs start with `D`, so tests/decisions.rs leaves them alone, but
// I7 still wants every typeable token to live here with its decision ID.
// ──────────────────────────────────────────────

/// D-JPK1/9: the Jetpack package-manager binary name.
pub const JETPACK_BINARY_NAME: &str = "jetpack";
/// D-JOS-STUDIO-LAUNCH1=A: direct jetos system-tool binary name.
pub const JETOS_BINARY_NAME: &str = "jetos";

/// U1 (D-JPK20) / U10 / D-JPK-FILES: the Jet **package manifest** is `pkg.jet`
/// (`PAYLOAD_FILE`; Cargo.toml analog, replaces `jet.toml`). Prior filenames
/// (pack.jet, the U10 interim name) were retired (clean break, no alias).
/// `PACK_LOCK_FILE` is superseded by `.jet/lock` (U2/S52).
pub const PACK_LOCK_FILE: &str = "pack.lock";

/// D-JPK7/15: the `<source>:<package/path>` ref separator. Users never type
/// Nix's `#` selector; Jetpack translates `:` to the provider's form.
pub const REF_SEPARATOR: &str = ":";

/// D-JPK7/15: recognized ref source prefixes.
pub const REF_SOURCE_NIXPKGS: &str = "nixpkgs";
pub const REF_SOURCE_GITHUB: &str = "github";
pub const REF_SOURCE_PATH: &str = "path";

/// D-JPK2/9: the Phase 1 verb set.
pub const JETPACK_VERBS: &[&str] = &[
    // Card #479: reuses D-DX2's existing `doctor` spelling for Jetpack health.
    "doctor",
    "run",
    "enter",
    "build",
    "test",
    "list",
    "hangar",
    "vendor",
    "audit",
    "clean",
    "add",
    "remove",
    "update",
    "outdated",
    "search",
    "info",
    "explain",
    "logs",
    "override",
    "push",
    TRUST_SUBCOMMAND,
    OS_SUBCOMMAND,
    DEV_SUBCOMMAND,
    CONFIG_SUBCOMMAND,
    BRIDGE_SUBCOMMAND,
    SERVICES_SUBCOMMAND,
    SECRETS_SUBCOMMAND,
    IMAGE_SUBCOMMAND,
    USER_SUBCOMMAND,
    TOOL_SUBCOMMAND,
];

/// U16 (card c9jetpackgates): `jet env -p <pkg>...` — ad-hoc nixpkgs packages
/// added to the shell without declaring them in any manifest. Repeatable;
/// realized once and dropped, same lifecycle as a manifest-declared ref.
///
/// D-JPK-SELECTOR1=C: on `jetpack build` / `test` / `run`, the same `-p`
/// spelling selects workspace members by exact name (cargo-style, repeatable).
pub const ENV_FLAG_PACKAGE: &str = "-p";

/// D-JPK-SELECTOR1=C: compute workspace members whose input hashes differ from
/// the recorded action-cache baseline, always including dependents.
pub const WS_FLAG_AFFECTED: &str = "--affected";

/// D-JPK-SELECTOR1=C: compute members changed since a git ref (plus dependents).
pub const WS_FLAG_AFFECTED_SINCE: &str = "--affected-since";

/// U16: force foreign-flake/devenv detection even when the project's own
/// manifest already declares `env.*` modules (which otherwise wins).
pub const ENV_FLAG_FLAKE: &str = "--flake";

/// U16: enter an isolated shell with no host environment leaking in —
/// threaded straight through to the underlying `nix` invocation.
pub const ENV_FLAG_PURE: &str = "--pure";

/// U27 (D-JPK-BUILDDBG1=A): preserve failed build scratch and open a shell in
/// the failing build environment.
pub const BUILD_FLAG_SHELL_ON_FAIL: &str = "--shell-on-fail";

/// U16: `jetpack bridge <verb>` — best-effort translators from a foreign
/// ecosystem descriptor into jetpack's own manifest form.
pub const BRIDGE_SUBCOMMAND: &str = "bridge";
pub const BRIDGE_VERB_FLAKE: &str = "flake";

/// U16: foreign dev-shell descriptor filenames `jet env`/`jet bridge flake`
/// look for. `jet env` only auto-detects one of these when the project's own
/// manifest declares no `env.*` module; `--flake` forces it either way.
pub const FOREIGN_FLAKE_FILE: &str = "flake.nix";
pub const FOREIGN_DEVENV_FILE: &str = "devenv.nix";

/// U19 (D-JPK-DEVCOMPOSE1=D, card c9jetpackgates): the project-level `jetpack
/// dev` engine verb — distinct from the already-shipped `jet dev <file.jet>`
/// interpreter/hot-reload loop (D-DEV4). Bare `jet dev` (no file argument)
/// dispatches here: realize `env(base + env.dev)`, gate on trust, wait for
/// services (U12 no-op today), then run the project's `fn dev()` or fall back
/// to `fn run()`.
pub const DEV_SUBCOMMAND: &str = "dev";

/// D-ENVHOOK1=A (ratified 2026-07-12): direnv-style opt-in env auto-activation.
/// `jet env hook <shell>` prints a one-line shell hook the user installs once;
/// after that, entering a directory whose tree carries an `env.jet` activates
/// that env (its first activation of an untrusted env prompts through the
/// D-JPK-GRANTCMD1 trust law), and leaving it deactivates. The hook is opt-in:
/// nothing runs on `cd` until the user adds it. These are engine subverbs of
/// `jet env`, so they route through `jetpack enter` (D-JPK-DISPATCH1) exactly
/// like the bare `jet env` shell-entry does.
pub const ENV_HOOK_VERB: &str = "hook";
/// D-ENVHOOK1=A: the hook's private per-prompt callback — realizes the nearest
/// `env.jet` and prints the shell statements the installed hook `eval`s to
/// activate/deactivate. Users never type this themselves; the installed hook
/// calls it (direnv's `direnv export` shape).
pub const ENV_EXPORT_VERB: &str = "export";
/// D-ENVHOOK1=A: the escape hatch — set to any non-empty value to suppress
/// auto-activation (and drop any active env) in the current shell.
/// Documented in docs/reference/environment.md.
pub const ENV_DISABLE_VAR: &str = "JET_ENV_DISABLE";
/// D-ENVHOOK1=A: the hook's activation state, exported into the shell so each
/// per-prompt `export` knows which `env.jet` directory is currently live (empty
/// = none). A change from it to the nearest `env.jet` root is what triggers a
/// load / unload.
pub const ENV_HOOK_ACTIVE_DIR_VAR: &str = "JETPACK_ENV_DIR";
/// D-ENVHOOK1=A: the pre-activation `PATH` saved on load, restored verbatim on
/// unload so leaving a project returns the shell to exactly its prior `PATH`.
pub const ENV_HOOK_OLD_PATH_VAR: &str = "JETPACK_ENV_OLD_PATH";
/// D-ENVHOOK1=A: the shells `jet env hook` can emit an auto-activation hook for.
pub const ENV_HOOK_SHELLS: &[&str] = &["bash", "zsh", "fish"];

/// U12 (card c9jetpackgates): `jetpack services <verb>` supervises the
/// project's dev `services:` processes under `.jet/services/<name>/` —
/// `up`/`down` start/stop the enabled set (or one named service), `health`
/// one-shot probes readiness, `logs` prints a service's captured
/// stdout/stderr. Distinct from the jetos `system.*.services` tier (Phase D,
/// untouched): this dev tier runs plain child processes via `std::process`,
/// never a system service manager.
pub const SERVICES_SUBCOMMAND: &str = "services";
/// D-JPK-IMAGE1 (=A, ratified 2026-07-01, c9jetpackgates): `jet image <name>`
/// builds the named `image.<name>` module contribution into a hangar OCI
/// layout (the `.Oci` kind only — `.Iso` rides the jetos installer tier,
/// Phase D, owner-gated, untouched). `--push <ref>` is honestly gated (E1268)
/// until TLS support lands for registry pushes.
pub const IMAGE_SUBCOMMAND: &str = "image";
pub const IMAGE_FLAG_PUSH: &str = "--push";

/// D-JPK-TOOLRUN1=A: unified `jetpack tool run|install|list|uninstall` noun —
/// ephemeral package-binary execution and persistent global PATH installs.
pub const TOOL_SUBCOMMAND: &str = "tool";
pub const TOOL_VERB_RUN: &str = "run";
pub const TOOL_VERB_INSTALL: &str = "install";
pub const TOOL_VERB_LIST: &str = "list";
pub const TOOL_VERB_UNINSTALL: &str = "uninstall";
pub const TOOL_VERBS: &[&str] = &[
    TOOL_VERB_RUN,
    TOOL_VERB_INSTALL,
    TOOL_VERB_LIST,
    TOOL_VERB_UNINSTALL,
];
/// Install under a different on-PATH bin name (avoids JPK-TOOL-COLLIDE / E1297).
pub const TOOL_FLAG_AS: &str = "--as";
/// Default profile name for `jetpack tool install` PATH projections.
pub const TOOL_PROFILE_NAME: &str = "tools";
/// On-PATH projection directory under `~/.jet/` (`bin/`).
pub const TOOL_BIN_DIR: &str = "bin";
/// Generation + metadata root under `~/.jet/` (`tools/`).
pub const TOOL_STATE_DIR: &str = "tools";
/// External tool-provider prefixes recognized but not yet realizable as
/// hangar providers — emit E1298 instead of silently skipping.
pub const TOOL_EXTERNAL_PROVIDERS: &[&str] =
    &["npm", "pypi", "cargo", "crates", "brew", "go", "gem"];
/// Diagnostic class JPK-TOOL-COLLIDE (E1297): install bin shadows a `#Task fn`.
pub const TOOL_DIAG_COLLIDE: &str = "E1297";
/// Diagnostic class JPK-TOOL-PROVIDER (E1298): external provider not available.
pub const TOOL_DIAG_PROVIDER: &str = "E1298";

/// D-JPK-GRANTCMD1=A: `jet trust <verb>` is the public grant graph command
/// family. The top-level `jet` binary dispatches it to Jetpack, which owns the
/// trust store.
pub const TRUST_SUBCOMMAND: &str = "trust";
pub const TRUST_VERB_GRANT: &str = "grant";
pub const TRUST_VERB_LIST: &str = "list";
pub const TRUST_VERB_EXPLAIN: &str = "explain";
pub const TRUST_VERB_REVOKE: &str = "revoke";
pub const TRUST_VERBS: &[&str] = &[
    TRUST_VERB_GRANT,
    TRUST_VERB_LIST,
    TRUST_VERB_EXPLAIN,
    TRUST_VERB_REVOKE,
];
pub const TRUST_FLAG_SCOPE: &str = "--scope";
pub const TRUST_SCOPE_USER: &str = "user";
pub const TRUST_SCOPE_REPO: &str = "repo";

pub const SERVICES_VERB_UP: &str = "up";
pub const SERVICES_VERB_DOWN: &str = "down";
pub const SERVICES_VERB_HEALTH: &str = "health";
pub const SERVICES_VERB_LOGS: &str = "logs";
pub const SERVICES_VERBS: &[&str] = &[
    SERVICES_VERB_UP,
    SERVICES_VERB_DOWN,
    SERVICES_VERB_HEALTH,
    SERVICES_VERB_LOGS,
];

/// U12: the per-project supervised-services state dir name, nested under the
/// project's `.jet/` managed folder — `.jet/services/<name>/{pid,stdout.log,
/// stderr.log,data/}`.
pub const SERVICES_STATE_DIR: &str = "services";

/// U13 (D-JPK-SECRETCRYPTO1, card c9jetpackgates): `jetpack secrets <verb>` —
/// the encrypted-repo-secrets engine (`.jet/secrets.age`, age-style crypto
/// bridge). `keygen` mints a local identity, `recipients add/list` manage the
/// committed recipients file, `set`/`get` upsert/read one entry (re-encrypting
/// the whole store each `set`).
pub const SECRETS_SUBCOMMAND: &str = "secrets";
pub const SECRETS_VERB_KEYGEN: &str = "keygen";
pub const SECRETS_VERB_SET: &str = "set";
pub const SECRETS_VERB_GET: &str = "get";
pub const SECRETS_VERB_RECIPIENTS: &str = "recipients";
pub const SECRETS_VERBS: &[&str] = &[
    SECRETS_VERB_KEYGEN,
    SECRETS_VERB_SET,
    SECRETS_VERB_GET,
    SECRETS_VERB_RECIPIENTS,
];
pub const SECRETS_RECIPIENTS_VERB_ADD: &str = "add";
pub const SECRETS_RECIPIENTS_VERB_LIST: &str = "list";
pub const SECRETS_RECIPIENTS_VERBS: &[&str] =
    &[SECRETS_RECIPIENTS_VERB_ADD, SECRETS_RECIPIENTS_VERB_LIST];
/// U13: the `--force` flag on `jetpack secrets keygen`, overwriting an
/// existing identity. Reuses the bare string rather than minting a new flag
/// constant family — mirrors `jet registry keygen --force`'s own flag spelling
/// (`Source/CLI.rs`), kept a plain literal there too.
pub const SECRETS_FLAG_FORCE: &str = "--force";
use super::{CONFIG_SUBCOMMAND, OS_SUBCOMMAND, USER_SUBCOMMAND};
