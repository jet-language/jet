# Jet, end to end — the vision in files you can read

Everything here is **illustrative and unratified**. The point is to *see and feel* Jet in
use, then react. Anything new goes through normal ratification before it's real.

## Syntax this document uses (corrected, owner-directed)

- **No semicolons** — the compiler inserts statement breaks; arms and statements are
  newline-separated.
- **Bindings:** `x @= v` (immutable), `x := v` (mutable), `x: T @= v` / `x: T := v`
  (typed), `x = v` (reassign), `comptime X = v` (compile-time).
- **`if` is also the switch.** Binary: `if cond { … } else { … }`. Many-way:
  ```
  if score == {
      100      -> "perfect"
      90 || 95 -> "great"
      else     -> "ok"
  }
  ```
  A bare arm value means `subject == value`. `if … == { … }` is an expression (yields a
  value) or a statement.
- **The dot rule (one spelling for "construct of this type").** `T.{ field: v }` builds a
  `T`; `.{ field: v }` builds the type expected by context. Enums match: `Color.Red` or,
  when the type is known, `.Red`. This is the single inferred-construction spelling — it
  replaces bare `{ … }` and `Type { … }`.
- **`loop` is the only loop:** `loop x in xs { … }` (each), `loop x < 10 { … }` (while),
  `loop { … }` (forever).
- **Rest:** `struct P { x: Int, y: Int }` · `enum E { A(Int) B }` · methods `v.m()`,
  receiver `self` · `trait T { … }`, `impl P: T { … }`, delegation `impl App: T using f` ·
  errors `-> T ? E`, `ok(v)`/`err(e)`, `?`/`??` · refs `^T`, `^x`, `.clone()` ·
  `use core.io as io`, `use pkg.{a, b}` · attrs `#[Codable]`, `#layout(columnar)`,
  `#Unsafe("reason")` · strings `"hi {name}"` · `comptime X = …`, `@embed("path")`.

## The one idea

**One file kind (`.jet`), one grammar.** A `.jet` file holds *code* and *typed surfaces* —
`Package`, `Env`, `Build`, `Workspace`, `System`, `Image` — all written with the same
`module name { contribution: Type.{ … } }` grammar, and **any field can be computed by Jet
running at compile time.** Hello-world and an operating system are the same language, the
same file type, the same model. You opt into surfaces as you need them.

Two promises hold at every level: **beginners get magic out of the box** (a bare file runs,
safe by default, no ceremony), and **experts get full control** (every default has an
explicit door to the machine underneath, without that door being a beginner footgun).

---

# Persona 1 — Mae, a CS student

Mae is learning Jet in a class. Her first program is one file, no project, no setup.

`hello.jet`
```jet
fn main() {
    print("hello, world")
}
```
`jet run hello.jet` prints `hello, world`. No manifest, no config, no build file.

Week 3, she writes a grade calculator — and meets enums, the `if` switch, and the dot rule:

`grades.jet`
```jet
enum Grade { A B C D F }

fn points(g: Grade) -> Int {
    if g == {
        .A -> 4
        .B -> 3
        .C -> 2
        .D -> 1
        else -> 0
    }
}

struct Student {
    name: String
    grade: Grade
}

fn main() {
    mae @= Student.{ name: "Mae", grade: .A }   // .A is inferred: field type is Grade
    print("{mae.name} earned {points(mae.grade)} points")
}
```

`.A` carries no type name because the context already knows it's a `Grade`. `Student.{ … }`
names the type because nothing else would. Same rule, two readings.

When she makes a mistake, the compiler teaches instead of scolding:

`if g == { .A -> 4 .B -> 3 }`  *(forgot the `else`)*
```
error[E0140]: this `if` match doesn't cover every case
  ┌─ grades.jet:5
  │  if g == {
  │     ^^ a `Grade` can also be .C, .D, .F
  │
  = why: a match must handle every value, so your program can't fall through a gap.
  = fix: add the missing arms, or an `else ->` to catch the rest.
```

Her final project spans two files — she learns modules with one keyword:

`stats.jet`
```jet
pub fn mean(xs: [Float]) -> Float {
    sum := 0.0
    loop x in xs { sum = sum + x }
    return sum / xs.len().to_float()
}
```
`main.jet`
```jet
use "stats.jet" as stats

fn main() {
    print(stats.mean([90.0, 82.5, 77.0]))
}
```

She never wrote a manifest, a build file, or a dependency line. **The simplest path stayed
the simplest path** — and the one new idea per week was genuinely one idea.

---

# Persona 2 — Devin, shipping his first CLI tool

Devin wants to publish a real command-line program. He scaffolds a package:

```
$ jet new weather
  created weather/ (package "weather", executable)
```
```
weather/
  pkg.jet
  main.jet
```
`pkg.jet`
```jet
payload: {
    name: "weather",
    version: "0.1.0",
}
packages: {
    weather: executable,
}
```

He needs an argument parser and HTTP. The standard library is free to reach for (you only
pay for what you `use`), so HTTP needs no dependency at all:

`main.jet`
```jet
use core.http
use core.io as io

fn main() -> Unit ? {
    args @= io.args()
    city @= args.get(1) ?? "London"

    resp @= http.get("https://wttr.in/{city}?format=3")?
    print(resp.body)
}
```

His CLI grows subcommands — the `if` switch handles dispatch cleanly:

```jet
fn run(cmd: String) -> Unit ? {
    if cmd == {
        "now"      -> show_current()?
        "forecast" -> show_forecast()?
        "help"     -> print(USAGE)
        else       -> return err("unknown command: {cmd}")
    }
}
```

When he wants a third-party library, he asks for it instead of hand-editing:

```
$ jet add csv --from github@jetpkgs/csv
  + csv  →  pkg.jet deps
```
`pkg.jet` (deps section now present)
```jet
deps: {
    csv: github@jetpkgs/csv,
}
```

Finally he takes control of the build — not with flags scattered across CI scripts, but as a
typed surface whose fields are real expressions. Profiles are **named and selected by flag**
(reproducible — not read from ambient environment):

`pkg.jet` (build section)
```jet
build: {
    release: Build.{ optimize: .small,  targets: [linux.x64, wasm32.web] },
    debug:   Build.{ optimize: .none },
}
```
```
$ jet build --release
  linux.x64    → build/weather        (small, 214 KB)
  wasm32.web   → build/weather.wasm   (small,  61 KB)
```

One file describes his package, its dependency, and how it builds — and a bare `jet run
main.jet` still works with none of it. **The manifest grew only as fast as he did.**

---

# Persona 3 — Aero Tools Inc., a small company with a monorepo

Aero has one repo holding a CLI tool plus a couple of internal libraries the tool shares.
Nothing is published to the world; it's all internal.

```
aero/
  jetpack.toml            ← repo index + shared sources (the ONE root manifest)
  env.jet                 ← shared dev shell for everyone on the team
  packages/
    cli/      pkg.jet      ← executable "aero" (the product)
    logging/  pkg.jet      ← library, internal
    config/   pkg.jet      ← library, internal
```

`jetpack.toml` — the repo's index. Members are discovered (`find . -name pkg.jet`), and
shared upstreams are named once so no one repeats a URL:
```toml
[repo]
name = "aero"
version = "0.4.0"

[sources]
stable = "github@NixOS/nixpkgs/nixos-24.05"

[packages]
cli     = "packages/cli/pkg.jet"
logging = "packages/logging/pkg.jet"
config  = "packages/config/pkg.jet"
```

The CLI depends on the two internal libraries **by name** — siblings in the same repo
resolve through the index, no URLs, no relative-path spaghetti:

`packages/cli/pkg.jet`
```jet
payload: {
    name: "aero",
    version: "0.4.0",
}
packages: {
    aero: executable,
}
deps: {
    logging: logging,     // in-repo sibling (resolved via the workspace index)
    config:  config,
}
```

An internal library marks how much of itself is public, so refactors inside don't break
teammates — `api: stable` is the promise, everything else is free to change:

`packages/logging/pkg.jet`
```jet
payload: {
    name: "logging",
    version: "0.4.0",
}
packages: {
    logging: library,
}
api: stable
```

Everyone shares one dev shell — the flake replacement — so a new hire is productive with one
command:

`env.jet`
```jet
module dev {
    sources: { stable: github@NixOS/nixpkgs/nixos-24.05 }
    env.dev: .{
        packages: [stable.[ripgrep, jq, postgresql]],
        prompt: "aero",
    }
}
```
```
$ jetpack enter
(aero) $ jet build --all
  packages/logging  → lib
  packages/config   → lib
  packages/cli      → build/aero
```

**Teams don't step on each other** because the boundaries are explicit: each package owns its
directory and its `pkg.jet`; `api: stable` pins the surface other packages may rely on;
private items simply don't escape (`pub` is opt-in). Two engineers editing `logging`'s
internals and `cli` in parallel never collide unless they touch the *stable surface* — and
then the build tells them, immediately.

---

# Persona 4 — Behemoth Corp., Google-scale

One repository, thousands of packages, hundreds of teams. The model from Persona 3 doesn't
change — it just has to hold at scale. Four things make that work.

**1. Ownership is where the file lives.** A team owns a directory subtree; the `pkg.jet`
files under it are theirs. There is no global registry of names to fight over inside the
repo — a package is addressed by *where it is*, indexed in `jetpack.toml`. Two teams can both
have a `logging` package; they never collide because each is reached through its own subtree.

```
behemoth/
  jetpack.toml
  search/    packages/ranker/pkg.jet      ranking/ index/ …      (Search team)
  ads/       packages/bidder/pkg.jet      billing/ …             (Ads team)
  infra/     packages/logging/pkg.jet     metrics/ tracing/ …    (Infra team)
```

**2. You depend on a package, never on the monorepo.** The Ads `bidder` wants Infra's
logging — it names exactly that, and resolution pulls *only* that package's subtree and its
transitive deps. The other 9,999 packages are not fetched, built, or considered:

`ads/packages/bidder/pkg.jet`
```jet
deps: {
    logging: infra/logging,     // one package out of the giant repo
    metrics: infra/metrics,
}
```

**3. Stability surfaces keep teams independent.** Infra publishes `api: stable` on `logging`;
everything behind it is private and refactorable without a company-wide breakage. A team that
needs to reach past the stable surface must opt in explicitly (`api: explicit`), and that
shows up in review as exactly what it is — a coupling, on the record.

**4. Builds are hermetic and reproducible.** This is where the compile-time model earns its
keep. Manifest computation (`find(...)`, profile selection) reads only **checked-in repo
state**, never the ambient machine — so the same commit produces the same build on any
laptop, any CI node, with no network. Profiles are selected by flag, not by reading
environment variables, precisely so two builders can't silently diverge.

### Pulling one library or executable out of the monorepo — into an OS image

Behemoth runs its own fleet on Jet's NixOS-equivalent. A server image needs exactly one
executable from the monorepo — the `ranker` — and nothing else from it. You name it, and the
resolver extracts just that package from the repo source:

`fleet/search-node.jet`
```jet
module search-node {
    sources: {
        stable: github@NixOS/nixpkgs/nixos-24.05,
        mono:   git@behemoth/monorepo,        // the whole monorepo, as a source
    }
    system.search-host: .{
        target: linux.x64,
        packages: [
            stable.nginx,        // a system package from nixpkgs
            mono.ranker,         // ONE executable from the monorepo — only this subtree
        ],
        services: {
            ranker: .{ enable: true, port: 8080 },
        },
        options: [
            net.hostName: search-1,
            users.svc.shell: stable.bash,
        ],
    }
}
```
```
$ jetpack os switch
  resolving search-host … mono.ranker (+ infra/logging, infra/metrics)
  building generation 137 …
  ✓ search-host @ generation 137
$ jetpack os rollback        # atomic, NixOS-style
```

No separate "ranker package repo" had to exist. A monorepo member is independently
addressable — `source.package` — so "give me just the one I want" is the **default**, not a
special case. The same addressing pulls a *library* into another project's `deps:` or an
*executable* into a dev shell, a build, or a whole machine. One mechanism, every scale.

**Native and polyglot dependencies** (a C library, BLAS, CUDA) come through the same `sources`
mechanism via the nixpkgs provider, so a team mixing Jet and C doesn't leave the model — they
add `stable.openblas` to `packages` and `use c.openblas` in code.

---

# What this replaces (the cohesion payoff)

Five jobs that are, today, **as many different formats**:

| Job | Today | In the vision |
|---|---|---|
| Package identity / deps / build | `pkg.jet` (`payload:`/`packages:`/`deps:`) | `Package` + `Build` surfaces |
| Dev shell (nix-shell / flake) | `env.jet` (`env.dev: Env`) | `Env` surface |
| Build options | CLI flags (`--small`, `--target`) | `Build` surface, computed |
| Monorepo index | **`jetpack.toml` (TOML!)** | `Workspace` surface (Jet) |
| OS config (nixos) | `config.jet` (future) | `System` / `Image` surfaces |

The odd one out is `jetpack.toml` — a second *language* (TOML) for the workspace index. In
the vision it becomes a Jet `Workspace` surface, so the whole project is **one grammar**,
discovered and computed the same way everywhere. `.jet/lock` stays generated (not authored,
so it isn't a format you learn). Net: from five authored formats to **one grammar with a
family of typed surfaces** — few concepts, one place to look, full reach when you need it.

`workspace.jet` (replacing the TOML)
```jet
module workspace {
    sources: { stable: github@NixOS/nixpkgs/nixos-24.05 }
    members: find("./packages")      // discovered, not hand-listed
}
```

---

# Where Jai's three powers land (the actual import)

- **Integrated build system** → the `Build` surface (above). The build is Jet, computed, no
  second DSL — Jai's win, in Jet's grammar.
- **Compile-time execution** → the `comptime` interpreter Jet already has. Compute tables,
  parse embedded files, generate config — baked into the binary. (`comptime palette =
  parse(@embed("palette.csv"))`.) Kept **pure** so builds stay reproducible; the manifest's
  small effect vocabulary (`find`, profile selection) reads only checked-in state.
- **Compiler-as-library** → two different things the original ask conflated. *Now:* factor the
  Rust compiler into clean internal libraries (serves tooling, the build driver, and a future
  self-host). *To user code, v1:* read-only reflection / derives (`#[Codable]`) — not live AST
  rewriting, which collides with "no user macros." The full Jai metaprogram is a post-self-host
  conversation.

---

# Experts get everything Jai's memory model gives — behind an opt-in door

Nothing in Jai's manual memory model is beyond Jet's reach; Jet just makes it an audited
opt-in instead of the default. Batteries-included by default, ownership-checked, no GC:

```jet
use core.mem.Arena

fn parse_doc(src: [U8]) -> Doc {
    arena @= Arena.new()        // bulk allocator, freed at scope exit; no per-node free
    return build_tree(arena, src)
}
```

Full manual control, explicit and audited, where you need it:

```jet
use core.mem

#Unsafe("reads through a raw pointer; addr must be a live, valid Int")
fn read_at(addr: Int) -> Int {
    p @= mem.Ptr<Int>.from_addr(addr)
    return mem.volatile_read(p)
}
```

Jet already has columnar/SOA layout (`#layout(columnar)`), distinct numeric types, and the
`#Unsafe` + `core.mem` tier. To make "full control" feel complete next to Jai, the pieces
worth adding (none of which weaken the safe default): an **implicit swappable allocator**
("context"), first-class **arena/temp-storage** patterns, and **scoped cleanup** (`defer`).

---

*Open decisions that shape all of the above live in the companion report
(`jai-import-report.md` §Decisions): the dot rule, killing the TOML, the pure-vs-effectful
compile-time line, and how packages address each other inside a monorepo.*
