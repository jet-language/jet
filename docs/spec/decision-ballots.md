# Decision ballots — open owner queue

Every decision waiting on the owner, and **nothing else**. The instant a
decision is ratified it leaves this file: delete the row, implement it, and
build it into its destination doc/code. No "recently ratified" section, no
tables of decided history — that clutter is what this file exists to avoid.
The ratified record lives in the decision log in
[`syntax-decisions.md`](syntax-decisions.md).

**House rule for whoever edits this file:** every decision below carries a
worked, user-story example for each option. The owner decides from concrete
artifacts — what a real person types, sees, and hits as an error — not from
abstract option tables. A bare ballot is not ready to show him. If you add a
decision, add its examples in the same edit.

### Glossary (terms used below)

- **option** — one declared jetos setting: a path, a type, a default, a doc.
- **module** — one `.jet` file that declares and/or sets options.
- **host** — a machine; `hosts/laptop.jet` sets that machine's options.
- **priority** — the tie-break ladder when two modules set the same scalar:
  `default` < normal < `force`.
- **lift** — copy one module file from someone else's repo into yours.
- **RAII** — a handle that cleans itself up when it goes out of scope.
- **shrinking** — a property-test feature: minimize a failing input to the
  smallest case that still fails.
- **freestanding** — a build with no OS underneath (embedded / microcontroller).
- **module boundary** — what unit of code constitutes a single module (file, directory, explicit block).
- **re-export** — making an item you imported visible to your own importers.

---

## 0. Jet language module system (blocks multi-file projects)

Four ballots that define how Jet's language-level module system works. The JetOS
`module { … }` declaration is a separate, already-scoped concept for config
namespaces — these ballots concern how Jet *code* is organized across files.

**Current state:** `use "path/to/file.jet" as alias` exists and works; it gives
you `alias.FnName(…)` access. What's missing is: no named paths, no visibility
control within files, no way to group or re-export, no inline submodules.

Cast: **Alex** (experienced developer, building a multi-file CLI tool), **Maya**
(beginner writing her first project bigger than one file).

---

### D-MOD1 — What *is* a module? · No rec — owner's call

The most fundamental question. What unit of code constitutes one module?

**Community context — Rust vs Go, what developers actually say:**

*Rust (`mod foo;` explicit tree):*
- **Praised for:** explicitness — you can always find every module by following the
  declaration tree from the crate root; large codebases stay navigable; compiler
  errors on forgotten declarations prevent orphaned files.
- **Criticized for:** ceremony — every new file requires a `mod foo;` line somewhere;
  the `mod.rs` vs directory layout (old `foo/mod.rs` vs new `foo.rs` + `foo/`) confused
  beginners for years and Rust only resolved it in 2018 (edition change); directory
  modules still need a `mod.jet` re-export file which adds friction.
- **Community verdict:** experienced Rust devs consider it the right call for large
  codebases; it's consistently cited as one of Rust's steeper beginner onramps.

*Go (directory = package, no declaration):*
- **Praised for:** zero ceremony — put a file in a folder, it's in the package; no
  orphaned files, no forgotten declarations; the "one directory = one package" rule
  is simple enough to explain in one sentence.
- **Criticized for:** capitalization-as-visibility forces naming decisions; you cannot
  have two packages in one directory (splits require full directory restructure);
  no inline submodules; adding any `.go` file to a directory silently adds it to the
  public module surface.
- **Community verdict:** Go's package system is almost universally praised as its
  simplest, most beginner-friendly feature. The main complaint is that it doesn't
  scale to monorepos elegantly.

**Practical tradeoff summary:**

| | Rust explicit `mod` | Go directory |
|---|---|---|
| New file ceremony | add `mod foo;` in parent | none |
| Orphan protection | compiler error | silent |
| Inline submodules | yes | no |
| Two modules, one directory | yes (inline `mod`) | no |
| Directory restructure needed to split | no | yes |
| Beginner friction | medium-high | low |
| Large codebase navigability | excellent | good |

**A — Rust: explicit `mod` declaration in a parent**

Every module must be declared. `mod math;` finds `math.jet` or `math/mod.jet`.
Inline `mod math { … }` also works. Compiler errors on undeclared files.

```
main.jet
math.jet
text/
  mod.jet   ← re-exports text module's public surface
  wrap.jet
  parse.jet
```

`main.jet`:
```jet
mod math;
mod text;

fn main() {
    val clamped = math.clamp(200, 0, 100);
    val line    = text.wrap("a long sentence", 40);
    print("{clamped}");
    print(line);
}
```

`text/mod.jet`:
```jet
mod wrap;
mod parse;
pub use wrap.wrap;
```

Forget `mod wrap;` → compiler error pointing at the missing declaration.

---

**B — Go: directory = module, no declaration needed**

All `.jet` files in the same directory are one module automatically. No `mod`
declarations anywhere. Import by directory path.

```
main.jet
math/
  clamp.jet
  stats.jet
text/
  wrap.jet
  parse.jet
```

`main.jet`:
```jet
use "./math" as math;
use "./text" as text;

fn main() {
    val clamped = math.clamp(200, 0, 100);
    val line    = text.wrap("a long sentence", 40);
}
```

`math/clamp.jet` — no declaration; being in `math/` is enough:
```jet
pub fn clamp(n: Int, lo: Int, hi: Int) -> Int { … }
```

---

**C — TypeScript/ES: every file is its own module, import by path**

Every `.jet` file is a module; directories are organizational only. Import any
file by relative path. No declaration ceremony anywhere.

```
main.jet
math.jet
text/wrap.jet
text/parse.jet
```

`main.jet`:
```jet
use "./math"      as math;
use "./text/wrap" as text;

fn main() {
    val clamped = math.clamp(200, 0, 100);
    val line    = text.wrap("a long sentence", 40);
}
```

`math.jet` — just write code:
```jet
pub fn clamp(n: Int, lo: Int, hi: Int) -> Int { … }
pub fn lerp(a: Float, b: Float, t: Float) -> Float { … }
```

---

### D-MOD2 — Import access: qualified vs unqualified · Updated per owner direction

Owner specified: **Rust-style two-step, but dot notation + path-based aliases
instead of `::`, and no wildcard imports.** Option A below reflects that.
Confirm A or pick an alternative.

**A — Owner's proposed: two-step with dot notation, path aliases, no wildcard**

Step 1: import the module with a path alias (existing Jet syntax, extended).
Step 2 (optional): bring specific items into unqualified scope with `use alias.Item`.
No `use alias.*` — wildcard is not supported.

```jet
use "./math" as math;       // step 1: import with alias

use math.clamp;             // step 2: clamp is now unqualified
use math.Stats;             // step 2: Stats is now unqualified
use math.{lerp, mix};       // step 2: group import

fn main() {
    val s = Stats.from([1, 2, 3, 4]);    // unqualified (step-2 imported)
    print(clamp(s.mean, 0, 100));         // unqualified
    print(math.lerp(0.0, 1.0, 0.5));     // qualified — lerp was in the group import above
    // (fix: lerp IS in the group above — use math.mix2 for unimported example)
    print(math.mix2(0.3, 0.7, 0.5));     // qualified — not step-2 imported
}
```

Maya's first program — she never needs step 2:
```jet
use "./math" as math;

fn main() {
    print(math.clamp(5, 0, 3));   // qualified, simple, one concept
}
```

No wildcard. `use math.*` is a compile error.

---

**B — Qualified-only (always use the module prefix)**

One concept only: import with alias, always prefix. No step-2 unqualified imports.

```jet
use "./math" as math;
use "./text" as text;

fn main() {
    val clamped = math.clamp(200, 0, 100);
    val line    = text.wrap("a long sentence", 40);
    val stats   = math.Stats.from([1, 2, 3, 4]);
}

// Short alias for heavily-used modules:
use "./math" as m;
print(m.clamp(200, 0, 100));
```

---

**C — Single-step `from … use` (Python-style)**

One keyword form that skips the alias and goes straight to unqualified:

```jet
from "./math" use clamp, Stats, lerp;

fn main() {
    val s = Stats.from([1, 2, 3, 4]);
    print(clamp(s.mean, 0, 100));
    print(lerp(0.0, 1.0, 0.5));
    // No module-prefix form available — origin invisible at call site
}
```

---

### D-MOD3 — Visibility defaults · Leaning A — owner's call to confirm

Owner selected **A** (private by default, `pub` to export) with a follow-up:
the LSP should detect when a function exists in a module but hasn't been
imported, and surface a hint. That is a **toolchain feature**, not a syntax
decision — it goes in the LSP/diagnostics roadmap regardless of which option is
chosen here. No syntax changes required to support it.

**A — Private by default, `pub` to export** *(owner's selection)*

Nothing leaves a file unless marked `pub`. Safest model, industry standard for
compiled languages.

```jet
// math.jet
pub fn clamp(n: Int, lo: Int, hi: Int) -> Int { … }   // exported
pub struct Stats { pub mean: Float; pub count: Int; } // exported + fields exported

fn validate(lo: Int, hi: Int) { … }   // private — callers cannot reach this
```

Maya writes `fn helper()` and tries to use it from `main.jet`:
```
Warning [L????]: `helper` in math.jet is private
  → add `pub` to export it, or is this intentional?
  Fix: pub fn helper(…) in math.jet
```
(A warning with a fix, not a hard error on first encounter — per owner's LSP note.)

LSP follow-up (separate roadmap item): when a file uses a name that exists in an
imported module but wasn't brought into scope, the LSP offers an auto-import
quick-fix rather than a bare "not found" error.

---

**B — Go: Capitalize = exported, lowercase = private** *(for comparison)*

No keyword; name casing is the visibility marker.

```jet
fn Clamp(n: Int, lo: Int, hi: Int) -> Int { … }  // exported
fn validate(lo: Int, hi: Int) { … }               // private

// Collision with Jet's existing PascalCase-for-types:
// ALL type names would look exported even if they aren't.
```

---

**C — Public by default, `priv` to hide** *(for comparison)*

Everything exported unless marked `priv`.

```jet
fn clamp(n: Int, lo: Int, hi: Int) -> Int { … }   // exported automatically
priv fn validate(lo: Int, hi: Int) { … }           // explicitly hidden
```

---

## 1. Core language — unify loops under one keyword? (amends S19)

Jet today has three loop keywords: `loop { }` (infinite), `while cond { }`, and
`for x in <range> { }`. The question: collapse all three into a single `loop`
keyword whose header decides the mode. S19 (ratified 2026-06-11) currently keeps
them separate, so this would amend it.

Nix has no imperative loops to copy (it's functional — `map`/`fold`), so the
reference here is mainstream imperative languages (Rust uses three keywords too:
`loop`/`while`/`for`). The same three programs, side by side:

**A — one keyword, header picks the mode:**
```jet
// infinite — no header
loop {
    val line = read_line();
    if line == "quit" { break }
    print(line);
}

// while — a boolean header
var n = 10;
loop n > 0 {
    print(n);
    n = n - 1;
}

// for — an `x in <range>` header
loop i in 1..5 {
    print(i);
}
```

**B — keep the three keywords (status quo, S19):**
```jet
loop { … }              // infinite
while n > 0 { … }       // conditional
for i in 1..5 { … }     // iteration
```

What A buys: one word to learn; `break`/`continue` read identically in every
form; the empty/boolean/`in` header is the only thing that varies. What A costs:
`loop n > 0` reads slightly worse than `while n > 0` for the conditional case
("loop n greater than zero" is less natural English than "while …"), and a reader
must look past `loop` to the header to know which kind it is — `while`/`for`
announce the kind in the first word.

A middle option **C** exists if A's conditional reads badly: fold only the
infinite + conditional cases into `loop` (`loop {}` = infinite, `loop cond {}` =
while) and keep `for x in …` separate, since iteration is the most distinct case.

No rec yet — this is a feel call, which is why all three loop kinds are shown.

---

## 2. jetos config surface — syntax (blocks the module system)

Five ballots that pick how jetos config files are written. Nothing in the jetos
module system is implemented until they land. Plan:
[`jetos-design.md`](../plans/jetpack-jetos/jetos-design.md).

Each ballot shows **how NixOS does it today** first — that's the proven baseline
we're improving on, not reinventing — then the Jet options. Recurring cast:
**Maya**, a beginner moving from Mint, configuring her laptop and lifting a
stranger's `firefox.jet` module into her repo.

### D-OS2 — How you declare a new option · Rec: A

Maya's `zellij.jet` module exposes two new settings other modules can read.

**NixOS today** — every option is an `mkOption` block with a `types.*` and a
description, nested under `options`:
```nix
options.programs.zellij = {
  layout = mkOption {
    type = types.str;
    default = "default";
    description = "Which built-in Zellij layout to load at startup.";
  };
  theme = mkOption {
    type = types.str;
    default = "gruvbox-dark";
    description = "Color theme name.";
  };
};
```
Powerful, but heavy: four lines and a `mkOption`/`types.` ceremony per setting.

**A — `option` one-liners** (the same information, one line each):
```jet
// modules/apps/zellij.jet
option apps.zellij.layout: string = "default"  "Built-in layout at startup"
option apps.zellij.theme:  string = "gruvbox-dark"  "Color theme name"
```

**B — one `options: { … }` record per file:**
```jet
options: {
    apps.zellij.layout: string = "default"      "Built-in layout at startup",
    apps.zellij.theme:  string = "gruvbox-dark" "Color theme name",
}
```

A and B both kill Nix's `mkOption`/`types.` boilerplate. A reads top-to-bottom
with each option self-contained and liftable; B groups them under one header but
adds a nesting level and trailing commas.

### D-OS3 — Guard keyword · Rec: A

The firefox module should contribute its package and prefs *only when enabled* —
evaluated against the final merged config, not as a runtime branch.

**NixOS today** — a module's `config` is wrapped in `mkIf`, reading a `cfg` alias:
```nix
config = mkIf cfg.enable {
  environment.systemPackages = [ pkgs.firefox ];
  environment.etc."firefox/policies/policies.json".text =
    builtins.toJSON cfg.policies;
};
```

**A — `when` (a distinct, declarative keyword in place of `mkIf`):**
```jet
when apps.firefox.enable {
    sys.pkgs += [firefox]
    sys.files["/etc/firefox/policies/policies.json"] = json(cfg.apps.firefox.policies)
}
```

**B — reuse `if`:**
```jet
if apps.firefox.enable {
    sys.pkgs += [firefox]
}
```

NixOS deliberately uses `mkIf`, *not* a bare conditional, precisely because this
guard is declarative (resolved against merged config), not a runtime branch. `when`
keeps that distinction with a friendlier word; B (`if`) throws it away and gives
one keyword two meanings — exactly what confuses Maya.

### D-OS4 — Priorities · Rec: A

A lifted module *suggests* Firefox as the default browser; Maya's host wants
qutebrowser, no argument.

**NixOS today** — priority is a wrapper function around the value
(`mkDefault`/`mkForce`, plus numeric `mkOverride 50 …`):
```nix
programs.firefox.defaultBrowser = mkDefault "firefox";   # a suggestion
programs.firefox.defaultBrowser = mkForce "qutebrowser"; # end of discussion
```

**A — prefix keywords** (the priority leads the line, no wrapper):
```jet
default sys.desktop.default_browser = "firefox"     // a polite suggestion
force   sys.desktop.default_browser = "qutebrowser" // end of discussion
```

**B — call-style markers** (closest to Nix's `mkDefault`):
```jet
sys.desktop.default_browser = default("firefox")
sys.desktop.default_browser = force("qutebrowser")
```

Either is shorter than `mkDefault`/`mkForce`. A reads as plain English with the
priority first; B mirrors Nix but makes the value look like a function result,
hiding that it's a priority marker. (jetos drops Nix's numeric `mkOverride`
levels entirely — three priorities only, per OS-I4.)

### D-OS5 — Per-app enable flags · Rec: A

Maya lifts `modules/apps/steam.jet` and wants to turn it on for her desktop.

**NixOS today** — every module hand-declares its toggle with `mkEnableOption`:
```nix
options.programs.steam.enable = mkEnableOption "Steam";
# …and the user writes:
programs.steam.enable = true;
```

**A — automatic:** a file at `modules/apps/steam.jet` gets `apps.steam.enable:
bool = false` for free; Maya just flips it. No `mkEnableOption` line anywhere:
```jet
// hosts/desktop.jet
apps.steam.enable = true        // steam.jet declares no enable at all
```

**B — each module hand-declares its enable** (the NixOS habit, in Jet syntax):
```jet
option apps.steam.enable: bool = false  "Enable Steam"
```

NixOS already standardized on the `<prog>.enable = true` convention — A just
removes the `mkEnableOption` boilerplate that every Nix module repeats, making it
automatic. B keeps the boilerplate; a misspelled path silently makes a dead toggle.

### D-OS6 — User scope · Rec: A

Maya's laptop is single-user today; next year she adds her partner's account.

**NixOS today** — system users and per-user (home-manager) settings live in two
separate namespaces:
```nix
users.users.maya = { isNormalUser = true; extraGroups = [ "wheel" ]; };
home-manager.users.maya.home.file.".config/git/config".text = ''
  [user]
    name = Maya
'';
```

**A — `user.<name>.*` with a `user.me` alias** (one namespace, multi-user ready):
```jet
user.me.files["~/.config/git/config"] = gitconfig   // primary user, ergonomic
user.alex.packages += [inkscape]                     // a second account, no rework
```

**B — single-user `home.*`** (closest to standalone home-manager):
```jet
home.files["~/.config/git/config"] = gitconfig
// adding a second user later means restructuring every module
```

A unifies NixOS's two namespaces and keeps the common single-user path short
(`user.me`) while scaling to multiple users for free. B is marginally simpler
today and repaints everything later.

---

## 3. jetos platform & product (post-v1)

Direction calls for jetos itself. Post-v1: they depend on M16 (pure eval +
sandbox + signed cache). Confirming them now locks the direction. Cast:
**Maya** (new user) and **Diego** (experienced, migrating from NixOS).

### D-NX1 — Bootstrapping ~10k packages · Rec: tap the nix cache for the tail

Maya's first `jetos switch` needs Firefox, ffmpeg, and ~4,000 transitive deps —
far more than will have native Jet builds for years.

**Recommended:** read-only tap of `cache.nixos.org` as a compatibility provider;
hand-write native Jet builds only for the spine; measure the migration over time.
```
$ jetos switch
  resolving 4,212 packages …
  3,981 from nixos cache ↓     (compatibility provider, read-only)
    231 native jet builds ⚒
  generation 1 ready
```
The cost of *not* doing this: no usable distro until 10k builds are hand-ported.

### D-NX2 — What `jetos add` edits · Rec: the host file

Maya runs `jetos add ripgrep`.

**NixOS today** — there is no `add` command. Maya hand-edits the right list in
`configuration.nix`, then rebuilds:
```nix
environment.systemPackages = with pkgs; [ git vim ripgrep ];  # added by hand
```
```
$ sudo nixos-rebuild switch
```

**Recommended:** edit the smallest declarative file (the host) for her,
auto-commit, escape hatch to skip:
```
$ jetos add ripgrep
  + ripgrep → hosts/desktop.jet
  committed: "add ripgrep"   (use --no-commit to stage only)
```
This is the headline safety property: `add/set/remove` only ever edit
declarative config — never a hidden second install database that can drift.

### D-NX3 — Ephemeral `jetos try` · Rec: shell-scoped, nothing recorded

Maya needs `httpie` once to debug, not forever.

**NixOS today** — `nix shell` drops her into a subshell with the package, gone on
exit:
```
$ nix shell nixpkgs#httpie
```
jetos keeps that exact behavior under a clearer verb:
```
$ jetos try httpie
  shell with httpie ready (nothing written to your config)
  $ http GET example.com/health
  $ exit
  # httpie is gone; config untouched
```
The honest answer to "I just want it for a minute" without polluting the system.

### D-NX4 — NixOS migration path · Rec: reporter, not auto-converter

Diego has a working `configuration.nix` and wants to move.

**Recommended:** a checklist/reporter that says what maps cleanly and what needs
hand-porting — not a silent converter:
```
$ jetos migrate-report ./configuration.nix
  ✓ 12 packages map directly
  ✓ users.users.diego → user.diego
  ⚠ services.nginx — port by hand (3 custom location blocks)
  ⚠ overlay `myPkgs` — no equivalent; see docs/migration#overlays
```
An auto-converter that silently mistranslates would erode trust on day one.

### D-NX5 — v0 reference image · Rec: one polished image

The first downloadable jetos. Desktop is **already decided: GNOME default**, with
KDE Plasma (#2) and Cinnamon (#3) shipping later as alternate desktop modules.
The open call is breadth:

**Recommended — one image, done well:**
```
$ jetos download
  jetos-v0-x86_64-gnome.iso       (the single reference target)
```

**Alternative — a matrix up front** (x86_64 + aarch64 × GNOME/KDE/Cinnamon):
six ISOs to test and support before the core is proven.

One image lets v0 be excellent instead of six-ways mediocre.

### D-NX6 — Option-schema bootstrap · Rec: hand-write the spine, JSON for tails

Unlike packages, there is no existing option *schema* to tap. Someone hand-writes
the spine option modules (boot, networking, users, desktop). For vendor config
Jet doesn't model yet, a `JSON` pass-through keeps users unblocked:
```jet
// no typed option for this knob yet — pass raw config straight through:
sys.services.nginx.extraConfig = json({ worker_connections: 1024 })
```
This is the one place a typed-everything rule bends, on purpose, so a missing
schema never hard-blocks a real machine.

---

## 4. Epoch 2 milestone ballots (open)

Detail lives in each `docs/plans/epoch-2/m*.md`. Cast: **Tess** (writing tests),
**Diego** (library author), **Maya** (app author).

### D-REF2 (M5) — Ship arena allocators this milestone? · Rec: only if forced

The tier-2 references milestone includes a parser example that builds an AST of
many cross-linked nodes.

**If the parser example needs them**, ship arena surface:
```jet
val arena = Arena.new();
val node  = arena.alloc(Node { kind: Call, children: [lhs, rhs] });
```
**Otherwise defer** — the simplicity ratchet (I8) says don't ship speculative
allocator surface. The ballot: build arenas now, or wait until an example
actually demands them? Rec: wait.

### D-LIB1 (M6) — S61 labels/defaults + S62 delegation together? · Rec: both in M6

Diego writes a `connect` function and a logging wrapper.

**Both in M6:**
```jet
fn connect(host: String, port: Int = 5432, tls: Bool = true) -> Conn ? { … }
connect("db.local", tls: false);        // S61: labels catch transposed args

struct Logged { inner: Service; }
impl Service using inner;                // S62: forward Service methods to inner
```
They reinforce each other for library ergonomics; splitting them across
milestones ships half a story. Rec: both together.

### D-LIB2 (M6) — How far generics v1 goes · Rec: assoc. types + default bodies

Diego writes a reusable `Store` trait.

**Recommended — associated types + default method bodies:**
```jet
trait Store {
    type Key;
    type Value;
    fn get(self, k: Key) -> Value?;
    fn get_or(self, k: Key, fallback: Value) -> Value {   // default body
        self.get(k) ?? fallback
    }
}
```
Covers the bulk of library needs without higher-kinded complexity. The ballot:
is that the right ceiling for v1, or narrower/wider?

### D-JSON1 (M6) — JSON decode strictness baseline · No rec — owner's call

Maya decodes an API response into `struct Server { port: Int }`, but the JSON has
`"port": "8080"` (a string).

**Strict baseline — reject the mismatch:**
```
error[Exxxx]: expected Int at .port, found string "8080"
  fix: the API returns a string here; decode into `port: String`
       or map it explicitly
```

**Lenient baseline — coerce where unambiguous** (`"8080"` → `8080`), only error
on truly impossible conversions.

This sets the default posture: predictable-and-safe (strict) vs
forgiving-of-messy-APIs (lenient). Genuinely a values call — no rec.

### D-IO2 (M7) — Cleanup surface · Rec: RAII handles (confirm)

Maya copies a file; if a write fails halfway, both handles must close.

**RAII handles (S63):** handles close on every exit path, including a `?` early
return — no keyword, no manual cleanup:
```jet
fn copy(src: String, dst: String) -> Unit ? {
    val input  = files.open(src)?;
    val output = files.create(dst)?;
    for line in input.lines() {
        output.write_line(line)?;     // if this fails, BOTH handles still close
    }
    ok(unit)
}
```
The alternative is an explicit `defer`/`transact` keyword the user must remember
to write. Rec: RAII — cleanup is automatic and invisible. This ballot just
confirms RAII over a cleanup keyword.

### D-PKGS4 (M8) — Yank / immutability rules · Rec: immutable + yank-hides (discuss)

Diego publishes `json 1.2.0`, then finds it panics on empty input.

**Recommended:** published releases are immutable (1.2.0 stays byte-identical
forever, so everyone's locked builds keep working); `yank` only hides it from
*new* resolutions:
```
$ jet yank json@1.2.0 --reason "panics on empty input"
  1.2.0 hidden from new solves; existing lockfiles unaffected
```
The tension to weigh: true deletion would break every downstream locked build;
immutability + yank protects them while still steering new users away. Owner
asked for a brief discussion before locking this.

### D-TEST1 (M11) — Property testing · Rec: in, if a small shrinking design exists

Tess tests that reversing twice is identity.
```jet
test "reverse is its own inverse" {
    forall(xs: List<Int>) {
        check reverse(reverse(xs)) == xs;
    }
    // on failure, Jet shrinks to the smallest case, e.g. [0, 1]
}
```
The part users love is **shrinking** (the minimized failing case). Ballot: ship
property testing *only if* a small shrinking design exists this milestone, else
defer. Rec: in-if-small.

### D-TOOL2 (M11) — `todo` typed-hole expression · Rec: defer unless small

Diego stubs a function he hasn't written yet.
```jet
fn parse(input: String) -> Ast {
    todo        // compiles now; at runtime: "todo at parse.jet:3, expected Ast"
}
```
Lets a program type-check with holes. Ballot: ship now, or defer? Rec: defer
unless the design turns out small.

### D-TOOL5 (M11) — Per-build capability summary · No rec — owner's call

Maya wants to know what a built binary can actually do before she runs it.

**Human summary at build:**
```
$ jet build service.jet
  capabilities: network (listen :8080) · filesystem (read ./config)
```
**Or a machine-readable manifest** (`--json`) for CI/policy gates.

Open: do we emit a capability summary at all, and in which form (human, machine,
both)? No rec — needs the owner's shape.

### D-CROSS2 (M15) — Freestanding panic strategy · Rec: abort by default

Maya builds for a microcontroller and a panic fires.

**Abort by default** — no unwinding machinery, smallest binary:
```
$ jet build --target thumbv7em-none-eabi --freestanding
  panic strategy: abort   (no unwind tables → smaller image)
```
On embedded, unwinding usually isn't worth the size. Ballot confirms abort as the
freestanding default.

### D-CROSS3 (M15) — Embedded smoke test · Rec: documented emulator harness

Proving the freestanding build runs on metal without requiring real hardware in CI.

**Recommended:** a documented local/emulator harness with a stated minimum:
```
$ jet test --target thumbv7em-none-eabi --emulator qemu
  blink example ran 100 cycles in qemu ✓
```
Ballot: emulator-harness minimum vs requiring physical hardware. Rec: emulator.

---

## 5. REPL refinements (M18 — not yet decision IDs)

Small calls left from the REPL design. Pick a default for each; promote to an ID
only if it touches syntax or a public CLI contract. Cast: a **student** in a
class demo and **Diego** at a quick prompt.

### Fuel / timeout · Rec: yes, cap like comptime fuel

A student types an accidental infinite loop:
```
jet> while true { }
  ⚠ stopped after 10M steps (possible infinite loop)
     use `:run` to allow an unbounded evaluation
```
Without a cap, the demo hangs. Rec: cap per-input steps.

### Startup banner · open

**With banner** (points beginners at help):
```
$ jet repl
Jet 0.9.2  ·  type :help
jet>
```
**Silent** (minimal noise for repeat users):
```
$ jet repl
jet>
```
Beginner-first vs minimalist. Pick a default.

### Color · Rec: respect NO_COLOR / CLICOLOR

Same convention as every other `jet` command:
```
$ NO_COLOR=1 jet repl     # plain bytes, no ANSI
```
Rec: yes, for consistency.

### Std preload · open

**Implicit `use std.io`** — common things just work:
```
jet> print("hi")
hi
```
**Explicit** — teaches imports, matches file semantics:
```
jet> print("hi")
error: `print` not found — add `use std.io;`
jet> use std.io;
jet> print("hi")
hi
```
Magic-but-inconsistent-with-files vs teaches-the-real-model. Pick a default.
