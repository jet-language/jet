# Decision ballots — open owner queue

Every open decision, and **nothing else**. The instant a decision is submitted it
leaves this file: it is recorded in the decision log in
[`syntax-decisions.md`](syntax-decisions.md) and removed here. No "recently
ratified" section, no decided history — decided decisions never reappear.

**House rule for whoever edits this file:** a full decision card carries a worked,
user-story example for each option (what a real person types, sees, and hits as an
error) — not abstract option tables. Decisions not yet drafted to that bar are
listed below as one-liners with a recommendation; expand one into a full card
(with examples) when it's time to decide it.

---

## Open decisions

Every open decision, listed and available now — nothing parked or hidden. Decide
any directly when it has a full card; for the one-liners, ask the dashboard to
**expand into a full card** (options + worked examples) when you want to decide it.
Submitting a decision records it in `syntax-decisions.md` and removes it from here.

### Language surface

### S83 — External-definition connector for `derive` / `impl` / `fn` (rec C)

You want ONE `Type<sep>name` connector, used consistently and Type-first, to attach things to a type from outside its body — `fn` keeps its keyword, the connector just injects the type before the name:

```jet
fn Point<sep>dist(self) -> Int { … }   // external method
impl Point<sep>Drawable { … }          // trait impl
derive Point<sep>Serialize             // derive (S56)
```

The constraint: the connector must be a token that isn't already spent. **Taken:** `=>` (lambda arrow, S46), `->` (return type / match arm), `::` (binding, D-BIND1), `.` (module + field, D-MOD1), `:` (type annotation), `@` (attribute/label), `#`. **Free (unused by the lexer today):** the whole tilde family, `:>`, `<-`, `<~`, and `$`/`\`. Each option below shows the `fn` form (same shape applies to `impl` and `derive`).

- **Option A — `=>` (your first pick).** *Taken* — it's the lambda arrow. Parseable here (item position can't be a lambda), but it gives `=>` two meanings and sits oddly next to `->`: `fn Point=>dist(self) -> Int`.

    ```jet
    fn Point=>dist(self) -> Int { self.x + self.y }
    ```

- **Option B — `~` (single tilde).** *Free.* Minimal, quiet, Type-first. Reads "Point's dist."

    ```jet
    fn Point~dist(self) -> Int { … }   //  impl Point~Drawable   derive Point~Serialize
    ```

- **Option C — `~>` (tilde-arrow, recommended).** *Free.* Keeps the arrow feel you liked, but is unmistakably distinct from both `->` and `=>`. Reads "Point extends-to dist."

    ```jet
    fn Point~>dist(self) -> Int { … }   //  impl Point~>Drawable   derive Point~>Serialize
    ```

- **Option D — `~~` (double tilde).** *Free.* More visually weighted than `~`, clearly a "binder" not an operator.

    ```jet
    fn Point~~dist(self) -> Int { … }
    ```

- **Option E — `~~~` (triple tilde).** *Free.* Maximum visual distinctness; heavier to type, more "ceremony."

    ```jet
    fn Point~~~dist(self) -> Int { … }
    ```

- **Option F — `:>` (colon-arrow).** *Free.* Reads "Point provides dist." Risk: visually near `:` (type annotation) and `>`.

    ```jet
    fn Point:>dist(self) -> Int { … }
    ```

- **Option G — `<-` (left arrow).** *Free.* "Point receives dist." Direction reads backwards for `fn` (the member flows *into* the type).

    ```jet
    fn Point<-dist(self) -> Int { … }
    ```

- **Option H — `.` in definition position.** *Free in this position* (no expression here to confuse with field/module access). Familiar — looks like the call site `point.dist()`.

    ```jet
    fn Point.dist(self) -> Int { … }   //  impl Point.Drawable   derive Point.Serialize
    ```

- **Option I — `for` keyword (Rust-style).** Familiar, but **reverses order** (trait/name first, not Type-first) and reads awkwardly for free `fn`s.

    ```jet
    impl Drawable for Point { … }
    fn dist for Point(self) -> Int { … }   // awkward
    ```

- **Option J — `extend Type { … }` block.** No per-item connector at all — a keyword block groups out-of-body members. Clean and Type-first, but it's a block, not the inline `Type<sep>name` shape.

    ```jet
    extend Point {
        fn dist(self) -> Int { … }
    }
    ```

**Recommendation:** **C (`~>`)** — free, keeps the arrow you wanted, and can never be confused with `->` (return) or `=>` (lambda); applies identically to `fn` / `impl` / `derive` and stays Type-first. If you want it quieter, **B (`~`)** is the minimal version of the same idea. Whatever you pick here also fixes S56's derive spelling.

### D-TOOL-SPLIT — Split lsp/fmt/lint out of the `jet` binary (no rec)

Editor tooling — format, lint, language server — can live inside the one `jet` binary or ship separately. This shapes install size, release cadence, and how an editor finds the LSP.

- **Option A — One bundled binary.** Everything is a `jet` subcommand; one install, one version.

    ```jet
    // shell
    jet fmt src/
    jet lint src/
    ```

- **Option B — Separate binaries.** Ship `jet-fmt`, `jet-lint`, `jet-lsp` independently so each can release and update on its own.

    ```jet
    // shell
    jet-fmt src/
    jet-lsp --stdio
    ```

- **Option C — Plugin model.** `jet` loads tools as plugins discovered at runtime.

    ```jet
    // shell
    jet fmt src/      // dispatched to the loaded fmt plugin
    ```

**Recommendation:** none. Owner's call on packaging philosophy.

