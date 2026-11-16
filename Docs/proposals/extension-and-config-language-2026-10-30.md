# Jet as the language for extensions, mods and config (#6380)

Status: proposal, 2026-10-30. Card #6380 (c0edci7p). Owner choices: D-EXT-HOST1. Gap cards: §6.

## Plain summary

The owner wants Jet to be the default choice for extensible things: editor plugins (replacing Lua in Neovim), game mods, application plugins and configuration, in line with the plan to beat Nix.

Jet already has several things the incumbents lack, in a single language:

- Static types.
- Effects written in the signature, so a config function that touches no files provably touches no files.
- Correctly rounded math, so pure code gives the same bits on every platform.
- A sandboxed plugin loader with fuel, memory and time limits.
- Typed option priorities for system config.
- A ratified command that exports typed config as JSON, YAML or TOML.

Four gaps decide whether it wins:

1. A host program cannot yet run a Jet extension without building it ahead of time. Lua's whole appeal is `luaL_dofile`.
2. There is no Neovim bridge.
3. Evaluating a Jet file to a value does not work today (`jet eval` rejects both shapes).
4. Pure-code determinism is promised for math but not stated for the rest of the language.

The one owner choice is how a host runs Jet extensions. We recommend a small runtime that runs checked, portable Jet, with the full compiler kept out of the host.

## 1. What "best" means here

Every candidate below is judged on the seven axes the card names:

| Axis | Question |
|---|---|
| Embedding | Can a C/C++/Rust host load it in-process with a small API? |
| Sandboxing | Can the host deny files, network and processes, and bound CPU and memory? |
| Startup | How long from "load this file" to "first call returns"? |
| Size | What does the host binary gain? |
| Typing | Are mistakes caught before running, and can editors complete fields? |
| Determinism | Same input, same output, on every machine? |
| Authoring | Can a non-specialist write and read it? |

## 2. The field

| | Embedding | Sandboxing | Startup | Size | Typing | Determinism | Authoring |
|---|---|---|---|---|---|---|---|
| **Lua 5.4 / LuaJIT** | Best in class: `lua_State`, ~32k lines of C | By hand: strip `_ENV`, no CPU limit without hooks | Sub-millisecond | 293 KB interpreter, 484 KB library (lua.org/about) | Dynamic; LuaLS annotations are advisory | Not guaranteed (`pairs` order, float libm) | Easy; 1-based, `~=`, globals by default |
| **Nix** | None; you run `nix` | Pure evaluation and sandboxed builds | Slow at nixpkgs scale; lazy evaluation keeps it workable | Large toolchain | Dynamic; module system checks option types when evaluated | Strong: hermetic, content-addressed | Hard: lazy functional, error traces |
| **Starlark** (Bazel, Buck2) | Libraries in Go, Java, Rust | Hermetic by design: no I/O, no recursion in Bazel | Fast | Small | Dynamic; starlark-rust adds annotations | Deterministic; values frozen after load | Python-like, easy |
| **Dhall** | Libraries in Haskell, Rust, Go | Total: always terminates; imports pinned by hash | Moderate | Moderate | Static | Strong | Unfamiliar syntax |
| **CUE** | Go library | Not Turing complete | Fast | Go binary | Types are values; constraints unify | Strong | New model (unification) |
| **Nickel** | Rust library | Pure | Fast | Moderate | Gradual types plus contracts | Strong | Moderate; merging built in |
| **KCL** | Rust/Go, CNCF sandbox | Pure | Fast | Moderate | Schemas with checks | Strong | Python-like |
| **Pkl** (Apple, 2024) | JVM; native via GraalVM; codegen to Swift/Go/Java/Kotlin | Pure, with resource allow-lists | JVM-bound unless native | Large | Static classes, amends | Strong | Readable |
| **JSON / TOML / YAML** | Any parser | Nothing to sandbox | Instant | Tiny | None without a schema; YAML 1.1 reads `NO` as false | Trivial | Easiest; no abstraction at all |
| **WASM plugin hosts** (wasmtime, Extism; Zed, Zellij, Envoy, Typst plugins) | Good: one ABI for many languages | Strong: memory isolation, fuel | Fast after compile | Host gains a runtime (MBs) | Whatever the guest language has | Mostly, except float NaN bits and host calls | Needs a separate toolchain; the shipped artifact is not readable |

What wins where:

- Lua wins editors and games on embedding, size and startup.
- Nix wins environments on hermeticity and the size of nixpkgs.
- CUE, Dhall, Nickel, KCL and Pkl win typed config on totality and validation.
- WASM wins untrusted plugins on isolation.

No incumbent has all seven. Static types with an effect-checked sandbox and a small, fast host runtime is the empty square.

## 3. Where Jet stands today (dev 03c64ad0c4)

| Axis | Jet today | Evidence |
|---|---|---|
| Embedding (Jet exports to a foreign host) | Done in law, partial in code: one `#Export` surface lowered to a native C-ABI library or a WASM component (D-EMBED1=E, D-EMBED2=C). The port to the self-hosted toolchain is #6023. | `Docs/research/evidence-2026-09-29/3114-embedding-two-adapter-delivery.md` |
| Embedding (a host runs Jet scripts) | **Missing.** Every route needs an ahead-of-time `jet build`. Nothing loads `.jet` source or a portable bundle at run time inside a foreign host. | §4, gap 1 |
| Jet hosting plugins | `core.plugin.load` runs sandboxed components with authority preflight (D-CORE-PLUGINPATH1=A) and fuel, memory and time limits (D-CORE-PLUGINLIMITS1=A). wasmtime is being replaced by Jet's own translator (`Docs/proposals/plugin-wasm-engine-2026-10-29.md`). | ratified |
| Sandboxing | Effects are part of every signature (`-[FS]>`). A pure function cannot reach IO; the `jet eval --pure` walk reports E3401. D-PORTABLE1=A ratifies a verified portable form, an interpreter, and a local native cache. | D-PORTABLE1, D-PORTABLE2 |
| Startup | Measured with the prebuilt dev compiler at 03c64ad0c4: `jet run` on an 8-line config program took 286 ms cold (compile) and **8 ms** warm (cached). #5476 targets ≤ 2 ms for hello. | Appendix, probe 1 |
| Size | The `jet` toolchain binary is 81 MB. A host cannot carry it the way it carries Lua. | size of the prebuilt dev `jet` binary |
| Typing | Static types, inference, typed settings checked before serving (D-VERDICT-3086-1=A), typed data in package.jet. | ratified |
| Determinism | `core.math` returns correctly rounded bits on every platform and tier (D-MATH-BITS1=A). Compile-time evaluation runs under a step budget (D-COMPTIME-WORD1=A). Nothing yet states determinism for the rest of pure code: map order, hashing, float printing. | §4, gap 4 |
| Config output | `jet config export app.jet --format yaml` is ratified (D-CONFIG-EXPORT1=A) and planned as #6029. | ratified, not built |
| Config evaluation | **Broken today.** `jet eval cfg.jet` rejects a value-returning `fn run() -> Int` with E1321 ("an Executable … returns `()`"). A printing `run` is E3401 in eval mode. `jet eval '1 + 2'` is E2104 "not built yet". This is #6195. | Appendix, probes 1 and 2 |
| System and environment config | Option priorities `#Default`/`#Force`/`#Priority(n)` with `jet os explain` (D-JOS-PRIORITY1=A). Environments are typed fields of package.jet (D-MANIFEST-ONE1=A). Nixpkgs comes in without Nix (D-NIX-FREE1, open). Benchmark against Nix: #6071. Nix evaluator port: #3600. | ratified / open |
| Hooks | One typed hook mechanism, `#On(point)` (D-HOOK-ONE1=A). Extensible domain lists (D-EXTEND-ARCH1=A). | ratified |

## 4. What Jet needs to win

### Gap 1: a host can run Jet extensions (the Lua slot)

The incumbent API is three calls: create a state, load a file, call a function. Jet's equivalent should be just as short, and keep its advantages:

```c
jet_host *h = jet_host_new(&api);              // api = the host's typed functions
jet_module *m = jet_host_load(h, "init.jet");  // checked + cached, or a portable bundle
jet_call(m, "on_save", args, &result);         // effects limited to what `api` grants
```

On the Jet side the extension is ordinary Jet. Its effects show what it may touch:

```jet
use editor // the host's API, declared as a Jet trait

#On(editor.buffer_saved)
fn format_on_save(buf: editor.Buffer) -[editor.Edit]> () {
    buf.replace_all(fmt.format(buf.text()))
}
```

The owner choice is what the host carries (ballot D-EXT-HOST1):

- **A: a small runtime for checked, portable Jet.** The runtime is the D-PORTABLE1 interpreter plus the native cache. When `jet` is installed it checks and compiles changed sources, so editing `init.jet` just works.
- **B: the full compiler in the host.**
- **C: out-of-process only.**

### Gap 2: Neovim specifically

Neovim embeds LuaJIT in-process. Every other language talks to it over msgpack-RPC, using the API metadata from `nvim --api-info`. Jet can win there without patching Neovim:

- A `nvim` package generates typed Jet bindings from `nvim --api-info`, so every API call is checked and completed in the editor.
- A host process runs Jet plugins over RPC and lazy-loads them by event.
- The package measures startup against an equivalent Lua plugin and publishes the numbers.
- Once gap 1 exists, an in-process route (a small Lua shim that loads the Jet runtime through LuaJIT's FFI) removes the RPC hop. [INFERENCE: LuaJIT FFI can load a C library; this is untested with a Jet runtime.]

### Gap 3: a config file evaluates to a value

- D-CONFIG-EXPORT1=A says that one command evaluates a typed config value and encodes it. Today `jet eval` cannot produce a value at all (§3).
- The fix belongs to #6195 (eval/repl) and #6029 (export). They need one shared rule: a config file's value is a typed `::` constant or a pure function result, evaluated under the comptime budget.
- No new card is needed. #6195 and #6029 should cross-reference each other, and this proposal records the dependency.

### Gap 4: determinism for pure code, stated and tested

- Mods in lockstep multiplayer, reproducible config, and build caches all need "same input, same output" beyond math.
- The spec says nothing about map iteration order, hash seeds, float-to-text, sort stability or integer overflow across tiers. [Checked: no such statement in spec.md or core-library.md.]
- Card: state the guarantee for code with no effects, and test it across platforms and tiers.

### Gap 5: authoring path

- Each incumbent has a "from X" path: Lua's `init.lua` guides, `nix flake init`, `cue import`.
- Jet has `jetpack import` for flakes. It has no guide that takes an `init.lua`, a `flake.nix`/`devenv.nix` and a YAML file and shows the typed Jet result with `jet config export` output beside it.
- Card: write that guide with tested examples.

### What Jet already has that the field does not, and should be the pitch

1. **Effects in the type.** A config or mod declares what it may touch, and the compiler proves it. Lua needs a hand-built sandbox, Nix a build sandbox, WASM a VM boundary.
2. **One language from config to program.** Starlark, Dhall, CUE and Pkl stop at data. When config grows logic, Jet does not switch language.
3. **Bit-identical math** on every platform (D-MATH-BITS1=A). No incumbent promises this.
4. **Typed priorities and explanations** for merged config (`#Force`, `jet os explain`). NixOS has `mkForce`, but untyped and with poor explanations.

## 5. Risks

- **Size.** If the host runtime grows past a few megabytes, Jet loses the embedding argument to Lua outright. The card puts a size budget in its criteria.
- **Startup.** Warm 8 ms is good for config and slow for a Neovim plugin manager that loads 50 plugins. #5476's ≤ 2 ms target matters here.
- **Two front doors.** Native libraries, components, and now a script runtime must share one export and type table. D-EMBED2's single contract has to extend to the script route, not fork from it.

## 6. Cards and ballots

Ballot on #6380:

- **D-EXT-HOST1**: how a host program runs Jet extensions.

New cards, filed from this proposal:

1. Embeddable Jet script runtime for foreign hosts (gap 1).
2. Neovim bridge: typed API and Jet plugin host (gap 2).
3. Determinism guarantee for pure Jet code (gap 4).
4. "Jet for config and extensions" guide with migrations from Lua, Nix and YAML (gap 5).

Existing cards this proposal depends on, with no new card created: #6195 (eval), #6029 (config export), #6023 (libjet port), #5476 (2 ms startup), #4195 (portable format), #6071 (beat Nix), #3600 (Nix evaluator).

## Sources

- Lua: lua.org/about.html (sizes and line count); Ierusalimschy et al., "The Evolution of Lua", HOPL III (2007).
- Neovim: `:help api`, `:help remote-plugin`, `nvim --api-info` (neovim.io/doc/user/api.html).
- Starlark: github.com/bazelbuild/starlark (spec: deterministic, hermetic); Bazel docs on recursion.
- Dhall: dhall-lang.org (total, imports by hash).
- CUE: cuelang.org/docs (types as values, not Turing complete).
- Nickel: nickel-lang.org (gradual typing, contracts, merging).
- KCL: kcl-lang.io (CNCF sandbox).
- Pkl: pkl-lang.org (Apple, 2024; code generation).
- YAML 1.1 boolean forms: yaml.org/type/bool.html.
- WASM hosts: wasmtime.dev, extism.org, zed.dev/docs/extensions, zellij.dev/documentation/plugins, github.com/proxy-wasm.
- Jet: decisions and files cited inline; probe programs in the appendix.

## Appendix: probes

Run with the prebuilt compiler for dev 03c64ad0c4.

Probe 1, `cfg.jet`. `jet run` prints the two lines, 286 ms cold and 8 ms warm. `jet eval cfg.jet` gives E3401 (standard output is not permitted in eval mode).

```jet
struct Server {
    host: String
    port: Int
}

fn servers() -> [Server] {
    base :: 8080
    [Server{host: "a.local", port: base}, Server{host: "b.local", port: base + 1}]
}

fn run() {
    loop s in servers() -> print("{s.host}:{s.port}")
}
```

Probe 2, `val.jet`. `jet eval val.jet` gives E1321: an Executable's `run` must return `()`.

```jet
fn run() -> Int {
    40 + 2
}
```
