# D-MIGRATE1 — Published-schema migrations: implementation plan

**Decision:** D-MIGRATE1 (Option A), ratified 2026-06-22. Compile-time enforcement of breaking data-shape changes on `#PublishedSchema` structs.
**Scope lock:** only `#PublishedSchema` + `migration Type { rename a -> b }`. The conversion-fn codegen (`from_vXXX`) belongs to Build-tier versioning library #11 and is a named follow-on. Other migration ops (add/drop/type-change) and `jet schema status`/`squash` are gated behind **D-MIGRATE2** (ratified-pending) — not implemented here.

## 0. Open decisions (resolve before/at implementation, do not guess)

1. **Diagnostic code number.** D-MIGRATE1 text said E0901, but E0901 is live ("method needs a generic bound", diagnostics.md:242) and the registry forbids reuse/renumber. **Resolved at ratification: E0910** (E0910–E0950 are entirely free — confirmed by grep). The whole plan below uses **E0910**.
2. **Sidecar durability: committed vs gitignored.** The snapshot is the *contract* — a fresh checkout must diff against it, so it must be durable and committed to the repo (not under the disposable `~/.cache/jet/build`). **Proposal: committed**, living under the project-local `.jet/` managed dir (`SOURCE_ROOT_DIR`, sibling of `.jet/lock`). Confirm.
3. **Sidecar file format.** I6 forbids serde. **Proposal: reuse the `.jet/lock` line-oriented round-trip pattern** (`Source/Lock.rs::write`/`parse`, std-only) rather than free-form JSON requiring a new parser. The decision text says "versioned JSON sidecar"; if literal JSON is required, the writer can follow `Source/DiagnosticsJSON.rs`/`Source/Publish/SBOM.rs` (hand-rolled emit) but a std-only *reader* must also be written. Recommend the lockfile-style format for zero new parser surface; flag JSON-vs-lockfile-format as an owner choice.
4. **Project scope of enforcement.** `#PublishedSchema` only has meaning in a project (it needs a published *version* from `pkg.jet`, and a release event). Single-file `jet run foo.jet` (R9) has no version. **Proposal: the marker is accepted but inert for single-file runs; the diff check only runs when a `pkg.jet` + a prior sidecar exist.** Confirm.
5. **What counts as "renamed" vs "removed+added".** The diff cannot distinguish a rename from a drop+add by shape alone — that's exactly why a `migration { rename a -> b }` is the user's declaration of intent. **Proposal: a removed field whose old type matches a new field with no migration = E0910 "removed"; with a matching `rename old -> new` declaration = unblocked.** Confirm the rename must type-match (old field's type == new field's type) to be accepted, else it is itself a type-change (D-MIGRATE2 territory) and stays blocked.

## 1. Attribute parsing — `#PublishedSchema`

The closest existing analog is the `#Numeric` marker on a distinct decl (`Source/Parser/Items.rs:339` `at_numeric_distinct_def` → `distinct_def`), and `#Pure`/`#Unsafe`/`#Test` markers (all `TokKind::Hash` + lookahead at the top-level item dispatch, `Source/Parser/Items.rs:289–342`).

- **`Source/Syntax.rs`**: add `pub const ATTR_PUBLISHED_SCHEMA: &str = "PublishedSchema";` near `ATTR_NUMERIC` (Syntax.rs:861), tagged `// D-MIGRATE1`. (I7: every user-typeable marker lives here with a decision ID.)
- **`Source/AST.rs`**: add `pub is_published_schema: bool` to `StructDef` (AST.rs:625), mirroring `Func::is_pure`/`is_unsafe` (AST.rs:607–609). R4: the marker's span should be retained if E0910 ever needs to point at the annotation — store `published_schema_span: Option<Span>`.
- **`Source/Parser/Items.rs`**: add a `TokKind::Hash if self.at_published_schema_struct()` arm in the top-level item `match` (alongside the existing `Hash` arms at 289–342), plus a `pub fn struct_def_published(...)` / extend `struct_def` to accept the flag. Detection helper mirrors `at_numeric_distinct_def` (Items.rs:1563): `Hash` then ident == `ATTR_PUBLISHED_SCHEMA` then `struct`/`pub struct`.
- Handle the `pub #PublishedSchema struct` / `#PublishedSchema pub struct` ordering — follow whatever order `#Numeric`/`#Pure` accept and keep it consistent.
- **Formatter**: `Source/Formatter/Items.rs` already prints `#Pure`/markers — add `#PublishedSchema` emission so round-trip/`jet fmt` is stable.

## 2. The `migration Type { rename a -> b }` block — lexing / parsing / AST

- **`Source/Syntax.rs`**: add `pub const KW_MIGRATION: &str = "migration";` and `pub const KW_RENAME: &str = "rename";`, tagged `// D-MIGRATE1`. The `->` arrow already exists (reuse the existing arrow token; confirm its `TokKind` via `Source/Lexer/Tokens.rs`). I7-compliant.
- **`Source/Lexer/Tokens.rs`**: these are contextual identifiers, not necessarily reserved keywords. **Proposal: parse `migration` as a top-level item only when an ident `migration` is followed by a TypeName + `{`** (contextual, like `module`/`derive` handling) to avoid stealing the identifier `migration` from user code. Confirm whether it should be a hard keyword (`KwMigration` token) — depends on whether the owner wants `migration` reserved. Recommend contextual.
- **`Source/AST.rs`**: new `Item::Migration(MigrationDecl)` variant on the `Item` enum (AST.rs:274), and:
  ```
  pub struct MigrationDecl {
      pub type_name: String,
      pub type_span: Span,
      pub ops: Vec<MigrationOp>,
      pub span: Span,
  }
  pub enum MigrationOp {
      Rename { from: String, from_span: Span, to: String, to_span: Span },
      // D-MIGRATE2 follow-on: Add/Drop/TypeChange — NOT parsed in this slice.
  }
  ```
  Only `Rename` is in scope. Parsing any other op keyword inside the block emits a "staged → D-MIGRATE2" teaching-style error (see diagnostics voice rule: "a future feature must never die as a generic error").
- **`Source/Parser/Items.rs`** + likely a small `Source/Parser/` addition: `fn migration_decl(&mut self) -> Result<MigrationDecl, Diagnostic>` — parse `migration <TypeName> { rename <ident> -> <ident> (; ...)* }`. R4: every node carries a span.

## 3. Shape-snapshot format + where/when written and read

**Location:** project-local `.jet/cache/schema/<TypeName>.snapshot` (or one `.jet/cache/published-schema.lock`). Add a constant in `Source/Syntax.rs`, e.g. `pub const SCHEMA_CACHE_SUBDIR: &str = "cache/schema";` under `SOURCE_ROOT_DIR` (`.jet`). This is **distinct from** `Source/BuildCache.rs::cache_dir()` (`~/.cache/jet/build`, disposable, content-hashed) — the snapshot is durable contract state, committed (decision 2).

**Format (decision 3 — recommend lockfile-style, std-only round-trip per `Source/Lock.rs`):** a versioned record per published struct:
```
schema_version = 1
type = UserRecord
published_version = 1.2.0
field name: String
field email: String
field age: Int
```
A new module **`Source/Publish/Schema.rs`** (joining `Diff.rs`/`API.rs`/`SemVer.rs` in the existing `Source/Publish/` tree) owns:
- `pub struct SchemaSnapshot { schema_version: u32, type_name, published_version, fields: Vec<(String /*name*/, String /*type, via Type::show()*/)> }`
- `fn write(&self) -> String` and `fn parse(raw: &str) -> Result<SchemaSnapshot, String>` (mirror `Source/Lock.rs:61/125`).
- `fn snapshot_from_struct(s: &StructDef, version: &str) -> SchemaSnapshot` — uses `Type::show()` for canonical field types, the same canonicalization `Source/Publish/API.rs::format_struct_sig` already relies on.

**When written (release time):** hook into the publish flow at `Source/CmdSupply.rs::run_publish` (after the gate passes, near line 79 where the SemVer diff currently no-ops). On a successful release, every `#PublishedSchema` struct in the entry bundle is snapshotted to `.jet/cache/schema/`. This is the net-new release-time step.

**When read (next build):** sema reads the sidecar (decision 4: only when `pkg.jet` + a prior snapshot exist) and diffs.

## 4. Sema diff pass + E0910 diagnostic (I3: all checking in sema)

- **`Source/Sema/`**: new pass (e.g. extend `Source/Sema/Registration.rs` where `Item::Struct` is already registered at 159/334/403, or a dedicated `Source/Sema/SchemaMigration.rs`). After struct registration:
  1. For each `#PublishedSchema` struct, load its prior `SchemaSnapshot` (if any) via `Publish::Schema::parse`.
  2. Compute current shape (`snapshot_from_struct`).
  3. Diff old vs new — reuse the *pattern* of `Source/Publish/Diff.rs::diff_public_api` (BTreeMap by name, detect removed / type-changed).
  4. Collect declared `migration <Type> { rename a -> b }` ops for that type (from `Item::Migration`).
  5. A field present in old, absent in new, **with no `rename old -> new` whose `new` exists and type-matches** → **E0910**. A type change without a (D-MIGRATE2) migration → also E0910 (named differently in the *why*), but note add/drop/type-change *operations* aren't author-declarable yet, so the only unblock available in this slice is `rename`.
- **I3/R1 compliance:** the check lives entirely in sema; codegen never sees migration state. Conversion-fn `from_vXXX` generation is explicitly **out of scope → library #11 follow-on**.
- **Diagnostic (E0910), in diagnostics.md voice** (what/why/fix, sentence case, plain words; uses backticked names; fix is imperative). Add to the registry table and a new section. Proposed copy:

  | Code | What | Why | Fix |
  |------|------|-----|-----|
  | E0910 | The published record `{Type}` dropped (or renamed) `{field}` since version `{version}`, with no migration to bridge it. | `#PublishedSchema` pins a record's saved shape at release. Old data already written with `{field}` could no longer be read, so a build that silently changes the shape would break readers of the published version. | Add `migration {Type} { rename {field} -> {new} }` if you renamed it; or bump the major version to publish a new shape; or mark the old field deprecated to keep reading it. |

  Render (pinned by snapshot):
  ```
  Error [E0910]: the published record `UserRecord` dropped `name` since version `1.2.0`, with no migration to bridge it
    --> examples/.../user.jet:3:5
      |
    3 |     display_name: String
      |     ^^^^^^^^^^^^
   Why: `#PublishedSchema` pins a record's saved shape at release; old data written with `name` could no longer be read
   Fix: add `migration UserRecord { rename name -> display_name }`, bump the major version, or deprecate the old field
  ```
- **`Source/Explain.rs`**: add an `E0910` arm so `jet explain E0910` works (other E09xx codes have entries; keep parity).
- **`Source/DiagnosticsJSON.rs`**: no change needed — E0910 flows through the generic serializer once it carries a span + what/why/fix.

## 5. tests/ui snapshot(s)

The `tests/ui.rs` harness compiles a **lone** `.jet` file with no project/cache (ui.rs:86–97), so it cannot supply the prior snapshot E0910 needs. **Plan: extend the ui-fixture convention** — an optional sibling `NAME.published.snapshot` that the harness, when present, installs into a temp `.jet/cache/schema/` (via a new env override mirroring `BuildCache`'s `JET_CACHE_DIR` pattern — add `JET_SCHEMA_CACHE_DIR` or reuse a project-root override) before compiling. This keeps the diagnostic text pinned in `tests/ui` (satisfies I4) while supplying the state.

Fixtures:
- `tests/ui/published_schema_breaking.jet` + `.published.snapshot` + `.stderr` — the E0910 error (field renamed without a migration).
- `tests/ui/published_schema_migrated.jet` + `.published.snapshot` + `.stderr` (empty / clean) — same change **with** `migration UserRecord { rename name -> display_name }`, compiles clean.
- `tests/ui/migration_unknown_op.jet` + `.stderr` — a non-`rename` op inside `migration { }` → staged-to-D-MIGRATE2 teaching error.

If extending the harness proves infeasible, the honest fallback is an integration test in `tests/pkg.rs` (which already builds temp projects under `std::env::temp_dir()`, pkg.rs:25) — but prefer the ui-harness extension since the decision asks for a ui snapshot.

## 6. Runnable example + golden output (I5)

Because enforcement is project-scoped (decision 4), the example is a **project**, not a lone file. Model on `examples/jetpack*`:
- `examples/features/migrations/` (or `examples/published-schema/`) with `pkg.jet` (version `1.2.0`), `src/main.jet` defining `#PublishedSchema struct UserRecord`, a checked-in `.jet/cache/schema/UserRecord.snapshot`, and the `migration UserRecord { rename name -> display_name }` block.
- The example **demonstrates both**: comment-documented, the without-migration state errors with E0910; the with-migration state compiles and runs, printing e.g. `display_name = ...`.
- Golden output `examples/features/expected/<n>_migrations.out`, enforced by `tests/golden.rs` (golden examples must front-end-pass, contain no `unsafe`, and print the expected `.out`).

## 7. Test coverage

- **ui snapshots** (§5): E0910 fires; migration unblocks; bad-op staged error.
- **golden** (§6): the migrated example builds + prints expected output.
- **Unit tests in `Source/Publish/Schema.rs`** (mirror `Source/Publish/mod.rs` tests): `write`/`parse` round-trip; `snapshot_from_struct` canonicalization; diff detects removed field; diff + matching `rename` → no error; rename with mismatched type → still blocked.
- **`tests/decisions.rs`**: D-MIGRATE1 is in the **Decision log** of `docs/spec/syntax-decisions.md`; if `tests/decisions.rs::ratified_decisions_enforced` keys off Syntax.rs IDs, the new `D-MIGRATE1` constants must be reflected (the ratification entry already exists). Verify the test passes after adding the Syntax.rs constants (I7 + ratification-drift test).
- **`docs/spec/spec.md`** + **`docs/spec/diagnostics.md`**: register E0910 in the code table and add the E0910 what/why/fix section + render example (I4: no snapshot/registry entry → the diagnostic doesn't exist).
- Run via the Nix shell: `nix develop -c cargo test`, bless ui/golden with `nix develop -c env UPDATE_EXPECT=1 cargo test`.

## Implementation order (R2: spec → parser → sema → codegen → tests)

1. diagnostics.md E0910 registry/section (spec first).
2. Syntax.rs constants (marker, keywords, cache subdir).
3. AST additions (`StructDef.is_published_schema`, `Item::Migration`, `MigrationDecl`/`MigrationOp`).
4. Parser (`#PublishedSchema` arm + detection; `migration` block).
5. `Source/Publish/Schema.rs` (snapshot type + round-trip + `snapshot_from_struct`).
6. Sema diff pass + E0910 (+ Explain.rs arm).
7. Release-time write hook in `Source/CmdSupply.rs::run_publish`.
8. ui-harness extension + fixtures; golden example; unit tests; bless snapshots.

## Out of scope (named gated follow-ons)

- **`from_vXXX` up/down conversion-fn codegen** → Build-tier versioning library #11.
- **add / drop / type-change migration ops; `jet schema status`; `jet schema squash`** → **D-MIGRATE2** (ratified-pending). The `migration { }` parser rejects these ops with a "staged → D-MIGRATE2" teaching error rather than a generic parse failure.

## Critical files for implementation
- `Source/Syntax.rs` (marker + keyword + cache-dir constants; I7)
- `Source/AST.rs` (`StructDef.is_published_schema`, `Item::Migration`, `MigrationDecl`/`MigrationOp`)
- `Source/Parser/Items.rs` (`#PublishedSchema` arm + `migration` block parsing)
- `Source/Publish/Schema.rs` (new — snapshot format + std-only round-trip, joining `Diff.rs`/`API.rs`)
- `Source/Sema/Registration.rs` (or new `Source/Sema/SchemaMigration.rs`) — the diff pass + E0910 (I3)
- Supporting: `Source/CmdSupply.rs` (release-time snapshot write), `docs/spec/diagnostics.md` + `docs/spec/syntax-decisions.md` (registry + ratification), `tests/ui.rs` (harness extension)
