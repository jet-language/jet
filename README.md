<p align="center"><img src="./Docs/assets/jetlang.png" width="120" alt="Jet" /></p>

<h1 align="center">Jet</h1>

<p align="center"><b>Write it like a script. Ship it as a native binary. Stay memory-safe the whole way.</b></p>

Jet is a compiled programming language for people who want Python's ease and
a systems language's speed, without C's pitfalls. A short program needs no
setup and no ceremony, mistakes are explained in plain words, and the same
source becomes a small static executable. Expert control is there when you
ask for it, and only where you write it.

> Jet is **pre-release** (`a0_NNN` builds, version 0.x). It changes in place,
> with no compatibility promise until 1.0.

---

## Thirty seconds of Jet

A complete command-line tool: it counts the words in a file and prints the
most common ones.

```jet
package: { name: "wordfreq", authority: { allow: [FS.Read] } }

use fs: core.files

#CLI
Args: struct {
    #Doc("text file to read") path: String
    #Doc("how many words to show") top: Int{3}
}

count_words: fn(text: String) -> [String:Int] {
    counts := [String:Int]{}
    loop word in text.lower().replace("\n", " ").split(" ") {
        if word != "" -> counts[word] = (counts.get(word) ?? 0) + 1
    }
    counts
}

#Test("repeated words are counted once each") {
    counts: count_words("the cat and the hat")
    assert_eq(counts.get("the") ?? 0, 2)
    assert_eq(counts.get("cat") ?? 0, 1)
}

run: fn(args: Args) {
    text: fs.read(args.path)
    rows := [(word: String, n: Int)]{}
    loop (word, n) in count_words(text) -> rows.push((word: word, n: n))
    rows.sort_by((a, b) -> (b.n <=> a.n).then(a.word <=> b.word))
    loop row in rows.take(args.top) -> print("{row.n}  {row.word}")
}
```

```console
$ jet run wordfreq.jet -- poem.txt
2  and
2  on
1  cat

$ jet test wordfreq.jet
repeated words are counted once each: pass
1 passed, 0 failed, 0 skipped

$ jet run wordfreq.jet -- --help
Usage: wordfreq path [options]

Arguments:
  path                   text file to read

Options:
  --path PATH              text file to read (required)
  --top TOP                how many words to show [default: 3]
  --help                   show this help
```

What that one file shows:

- **The struct is the command line.** `#CLI` turns `Args` into argument
  parsing, defaults and `--help`. There's no parser library to learn.
- **Programs ask before they touch your machine.** The `package:` line grants
  file reading and nothing else. Delete it and Jet refuses to read the file
  until you allow it, either in that line or for one run with
  `jet run --allow=FS.Read`.
- **One way to declare anything.** Types, functions and values are all written
  `name: value`, and `name := value` is the one way to make something
  changeable.
- **Tests live beside the code** and run with `jet test`.
- **Failures are values with a trail.** If the file is missing, the run stops
  with the reason and the exact line:

  ```console
  $ jet run wordfreq.jet -- missing.txt
  Error: not found during read `missing.txt`: No such file or directory (os error 2) (type: IOError)
    conversion: IOError -> Err
    failed here: run (wordfreq.jet:24)
        text: fs.read(args.path)
  ```

## Mistakes, explained

Every error says what happened, why the rule exists, and how to fix it:

```console
$ jet run oops.jet
Error [jet.ImmutableAssignment]: `total` was made with `:`, so it can't change
  --> oops.jet:3:23
    |
  3 |     loop n in 1..3 -> total = total + n
    |                       ^^^^^
 Why: Only `:=` bindings and parameters declared with the write mark `&` can be changed
 Fix: Declare it with `total := ...` instead
More: jet-lang.dev/e/E0111
```

Each code has a lesson behind it: `jet explain E0111`.

## From script to binary

`jet run` compiles and runs in one step, with no project and no build file.
When the program is ready, `jet build` makes a standalone executable:

```console
$ jet build wordfreq.jet          # 0.6 s from a cold cache
$ ls -l .jet/build/wordfreq       # 315 KB, statically linked
$ .jet/build/wordfreq poem.txt    # 7 ms
```

Those numbers are from the program above on an x86_64 Linux desktop with the
`a0_029` release. `jet run`, `jet build` and `jet build --release` all use the
same compiler and one shared runtime, so a program behaves identically however
you run it.

## Install

Jet runs on x86_64 Linux. Download the latest release, check it, and put it on
your `PATH`:

```sh
gh release download --repo jet-language/jet --pattern 'jet-*-linux-x64.tar.gz*'
sha256sum -c jet-*-linux-x64.tar.gz.sha256
mkdir -p ~/.local/jet && tar -xzf jet-*-linux-x64.tar.gz -C ~/.local/jet
export PATH="$HOME/.local/jet:$PATH"
```

> **For now, one extra step.** The release binary doesn't yet carry its core
> library inside itself, so it needs the matching source next to it. That is
> being fixed. Until then:
>
> ```sh
> git clone --depth 1 --branch "$(cat ~/.local/jet/RELEASE)" https://github.com/jet-language/jet ~/.local/jet-src
> export JET_TOOLCHAIN_ROOT="$HOME/.local/jet-src"
> ```

Then start a project:

```console
$ jet new hello && cd hello
$ jet run
hello, world
$ jet test
the greeting stays stable: pass
1 passed, 0 failed, 0 skipped
```

## What's in the box

One `jet` command does the whole job: `run`, `build`, `test`, `check`, `fmt`,
`doc`, `debug`, `repl`, `explain`, `fix`, dependency management through the
built-in package manager, and more (`jet help` lists them all).

The core library is written in Jet and covers files, processes, networking and
HTTP, JSON and other formats, time, text, collections, concurrency, terminal
interfaces and more. Interop with C and C++ goes through checked boundaries.
Low-level code that needs it can open an `#Unsafe("reason") { … }` block, which
tools list and audit; nothing outside one can corrupt memory or leak it.

## Where it's headed

Jet is built in the open, and its plan lives on
[Tower](Tools/tower/skills/tower/SKILL.md), the project board. The work in
flight:

- **A clean hand-off.** Anyone can clone Jet, fetch the latest release, and
  build, test and ship it with no private setup. That includes a release
  binary that carries its own library.
- **Self-hosting with no Rust left.** The compiler, package manager and tools
  are already written in Jet; the last tools and tests are moving over.
- **The language as a library.** The core library declares its own runtime
  functions instead of leaning on compiler tables, so any package can do what
  Core does.
- **Effects in types, memory safety proofs, and a complete core library.**
- **A better day-to-day:** the command line, error messages, editor support,
  and a first-class testing toolkit.
- **Packages and configuration** that replace Nix.
- **The best scripting language, and web end to end,** measured against the
  strongest alternatives.
- **Speed and size** that beat every peer language and match hand-written
  machine code on each measured workload.

The [philosophy](Docs/spec/philosophy.md) explains what Jet optimizes for and
how it breaks ties.

## Build Jet yourself

Jet compiles itself: each release is built by the one before it, and a new
compiler must rebuild its own source byte for byte before it becomes a release.
To build from a fresh clone, use the latest release as the seed:

```sh
TAG=$(gh release view --repo jet-language/jet --json tagName --jq .tagName)
git clone --branch "$TAG" https://github.com/jet-language/jet && cd jet
gh release download "$TAG" --repo jet-language/jet --pattern "jet-$TAG-linux-x64.tar.gz*" --dir ../release
(cd ../release && sha256sum -c "jet-$TAG-linux-x64.tar.gz.sha256")
mkdir ../seed && tar -xzf "../release/jet-$TAG-linux-x64.tar.gz" -C ../seed
../seed/jet build Compiler
../seed/jet run Tools/packaging/ReuseSeedRuntime.jet -- ../seed Compiler/JetCli/.jet/build/jet
export PATH="$PWD/Compiler/JetCli/.jet/build:$PATH"
```

[The bootstrap guide](Docs/spec/contributing/bootstrap.md) covers the full
self-host chain and older releases.

## Contribute

Start with [AGENTS.md](AGENTS.md), the project's working agreement: who
decides what, the rules every change keeps, and how a change is proven. Then
read [CONTRIBUTING.md](CONTRIBUTING.md). Report security issues through
[SECURITY.md](SECURITY.md).

| Folder | What's there |
|---|---|
| [`Compiler/`](Compiler/) | The compiler and its runtime |
| [`Core/`](Core/) | The core library |
| [`Jetpack/`](Jetpack/) | The package manager |
| [`Examples/`](Examples/README.md) | Runnable examples, each with checked output |
| [`tests/`](tests/) | Language, diagnostic and regression tests |
| [`Docs/`](Docs/README.md) | The spec, guides and the website |
| [`Tools/`](Tools/) | Contributor tooling and the project board |

## Learn more

- [First hour with Jet](Docs/spec/guides/first-hour.md)
- [Examples](Examples/README.md)
- [Fixing compiler errors](Docs/spec/guides/diagnostic-recovery.md)
- [Core library reference](Docs/spec/reference/core-library.md)
- [Language specification](Docs/spec/spec.md)

## License

[MIT](LICENSE)
