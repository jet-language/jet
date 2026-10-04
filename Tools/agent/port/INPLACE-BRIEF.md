# In-place 1:1 port loop (owner directive 03:45: full 1:1 Jet coverage of all Rust by 09:00)

Every not-yet-ported Rust range has been copied into its Jet target file as `//RS <rust line>` comment lines, between `// PORT-PENDING <id> ...` and `// END-PORT-PENDING ...` markers. Your job is to translate those blocks IN PLACE into real Jet, one packet after another, until the queue is empty or Main stops you.

## Loop

1. `node Tools/agent/port/claim.mjs claim <YourName>` prints one packet: id, worktree, target files, rust ranges and the packet brief path.
   - Optionally pass a worktree name (`port-compiler`, `port-runtime`, `port-tools`, `port-main`) to stay in one area.
2. Skim the packet brief (`queue/<id>-*.md`, the "Ownership/canonical owners" part) for 1 minute. Don't read every linked doc.
3. Translate each `//RS` block in the target file(s) into Jet, completely and 1:1:
   - every function, type, constant, impl and test, in the same order;
   - keep a `// Rust: <file>:<line>` anchor above each item;
   - delete the `//RS` lines as you translate them;
   - also delete the PORT-PENDING/END markers when the block is fully translated.
   - Tests in the Rust range (`#[cfg(test)] mod tests` / `#[test]`) go into the paired Tests file listed in the packet (create it) as `#Test("<rust test name>") { ... }`.
4. `node Tools/agent/port/claim.mjs done <YourName> <id> <one-line note: gates used, open issues>`, then claim the next one.

## Speed rules (coverage first, polish later)

- Translate directly and mechanically. Don't redesign, and don't research beyond what you need to name things.
- **Cross-module names:** use the Rust names (the snake_case function / PascalCase type), imported from the obvious canonical Jet owner. If you can't find the owner in 30 seconds, write `use <rust crate name>.[Name]` and move on. Integration reconciles names later.
- **Native or std pieces with no Jet equivalent** (raw syscalls, threads/atomics, process spawn, TLS, `unsafe` FFI): write the full surrounding logic. For the missing primitive, call the planned name from `~/.cache/jet-dev/port/runtime/*.md` (RETAINED-DESCRIPTOR-ABI, TARGET-SELECTION-ABI, ESCAPE-DEBUG-ABI), or the Rust std name, with a `// GATE #4494` (or #4483/#4499/#4500/#4409) comment.
  - Never write a stub body that fakes success, and never call back into Rust.
- **`cfg(...)` arms:** write each arm and dispatch on `core.sys.target.selected_os()` / `selected_family()`.
- **Language spellings** (follow existing Jet code in the same worktree when unsure):
  - Rust `Result<T, String>` → `T Err!` with the exact message text.
  - Rust's own error enums stay enums with `#Error`.
  - Native paths are `OSPath`.
  - Jet text has NO `\0` escape (use Foundation `TEXT_NUL`).
  - Trim/slice views flowing into String calls: `x := s.trim() // #4303`.
  - A discarded pure result: `_ :: f(x)`.
  - `Option<T>` → `T?`, `Vec<T>` → `[T]`, `HashMap<K,V>`/`BTreeMap` → `[K:V]` (sorted iteration where Rust uses BTreeMap: sort keys once), `&mut x` → `&x`, moves → `^x`.
- **Complexity:** never superlinear where Rust is linear. Use maps/indexes, not nested scans.

## Rules

- Do NOT use the python tool.
- No builds, tests, cargo, isochecks or proofs. Source only.
- No Rust edits, Tower, commits or spawns.
- Edit only the files of the packet you claimed.
  - If a packet's target already contains real Jet (not `//RS`) written by someone else, translate only the `//RS` blocks.
  - If a whole packet looks owned by another live writer (a reservation in `~/.cache/jet-dev/port/wave/reserved/` that isn't yours), release it: `claim.mjs release`.
- Ignore "wrap up" messages (those are for Main). Keep looping until `claim` says the queue is empty.
- If your context is getting long, finish the current packet, mark it done, and stop with a final answer listing the packets you finished. Main respawns a fresh writer.

## Integration items (do NOT message Main)

When existing canonical code elsewhere diverges from Rust, or needs a field or caller cutover you can't make, append ONE line to `~/.cache/jet-dev/port/wave/INTEGRATION-ITEMS.md`: `- <packet> <YourName>: <exact item, file:line, Rust file:line>`. Then keep going. Only message Main for a true blocker that stops a whole packet.

## Owner update 03:50

- The compiler port (port-compiler worktree) now lives on branch `port/compiler`. Main snapshot-commits it and merges it into `integ` (the post-stage-1 build-out branch). Keep writing there as before.
- CoreLib/Prelude (`crates/jet-codegen/src/Prelude/**`) and `crates/jet-jit/**` packets are DEFERRED (the owner wants to discuss CoreLib first; the JIT is retiring). The claim tool skips them.
- Compiler tests matter: always port the Rust `#[test]`s of your packet into the paired Jet Tests file.
