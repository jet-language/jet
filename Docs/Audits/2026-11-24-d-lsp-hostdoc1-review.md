# Adversarial review: D-LSP-HOSTDOC1, how an editor hands the Jet regions of a host document to `jet self lsp`

Date: 2026-11-24
Reviewer: HostDocReview (Claude Opus, fresh agent)
Ballot reviewed: `$JET_SCRATCH_ROOT/tmp/orch/ballots/D-LSP-HOSTDOC1.json` (draft, author G2Editor, recommendation T1). The draft as received is kept as `D-LSP-HOSTDOC1.json.before-review` beside it, with a copy in this directory.
Compiler for probes: `$JET_SCRATCH_ROOT/tmp/orch/tip18-chain/s1/jet` (`jet a0.0.46-dev`)
Source read: dev worktree `Main-dev-collectall` at dc046ef3d1. During the review, dev moved to 6d87b75ed5. `git diff --stat dc046ef3d1..6d87b75ed5` touches nothing under Compiler/JetLSP or Tools/Editors, and `SourceDocuments.jet` is still not on dev.
Evidence: `review-probes/`, every probe run one at a time.
- `p1-offsets.mjs` gives byte, UTF-16 and line arithmetic for the inWild host. Output: `out-p1-p2.txt`.
- `p2-pront.jet` is the inWild module under `jet check` and `jet check --json`. Output: `out-p1-p2.txt`.
- `p3-lsp.mjs` drives today's `jet self lsp` over stdio with each wire form and edge case. It has one server process per scenario. Output: `out-p3.txt`.
- `p4-exit.mjs` is an incidental shutdown/exit check found while running p3. Output: `out-p4.txt`.
- `out-validate-before.txt` and `out-validate-after.txt` hold `Validate.jet` on the received draft and on the repaired ballot.
- `out-embed-region1.txt` is `node Tools/Tower/tower.mjs decision show D-EMBED-REGION1`, run read-only.

The `(server exit code 1 …)` lines in `out-p3.txt` come from p3's first version, which sent `"params": null` on shutdown and exit (see D3). p3 now omits them.

Verdict scale: **holds**, **holds within limits** (the limits are stated), **fails**.
Finding IDs: F1…F17 are round 0. R1…R3 came up when I re-reviewed my own repairs. D1…D3 are code defects. I repaired the draft directly, as the assignment allowed. Section 4 logs each repair and its re-review.

## 0. Context the ballot depends on

- **D-EMBED-REGION1=A** was ratified 2026-09-10. It was archived on 2026-10-29 by Tower's age-based retirement (`store.mjs` 804-818), with status still `ratified` and no `supersededBy`, so it still governs. It is a compiler contract and says nothing about the LSP wire. So this ballot is not redundant. The decision does constrain the ballot:
  - Option A technical: "Hosts supply complete generated Jet source and its unchanged source segments. They retain control of module scope and host syntax. Jet validates UTF-8 boundaries and mappings."
  - "Every result carries the external revision. The same checked maps serve diagnostics, completion, navigation, and edits. Edits across unmapped generated text are rejected."
  - "Generated-only locations stay labeled as generated. Cancellation never publishes a stale partial result. Editor conversion uses UTF-16 units, not byte offsets."
  - "Use one SourceDocument input for whole and mapped documents. Migrate existing callers to the same check operation; do not keep parallel parsers or compatibility entry points."
  - Detail: "The design pass removed host-format parsing from the compiler proposal." Option A loss: "Hosts must identify their embedded regions."
  - Its own Volar comparison says "Its string offsets are UTF-16 positions, unlike the proposed Jet byte ranges". So bytes are the compiler's unit, and UTF-16 is the editor's.
  - Owner comment: "You wrote the examples in rust not jet. Do not codify rust code, it must be jet."
- AGENTS.md Owner gates list public API as owner-only. `land/6903-c4c5-split.md` says c4e is "a ballot candidate, or Main's call within D-EMBED-REGION1". This is the first `jet/` protocol surface, so a ballot is legitimate.
- **The driver side is not on dev.** `Compiler/JetDriver/Source/Driver/SourceDocuments.jet` and its tests exist only in the unlanded `land/6903-c4ab.patch`, based on dc046ef3d1. That patch:
  - adds `JetDriverSourceDocument{path, source, host: JetDriverHostDocument?}`, `JetDriverHostDocument{identity, revision: Int, text, mappings: [JetDriverSourceMapping{host: Span, jet: Span}]}`, and the functions `jet_driver_source_document_problem`, `jet_driver_host_span`, `jet_driver_jet_offset`, `jet_driver_host_edit` and `jet_driver_utf16_offset`;
  - has a message (`6903-c4ab.msg`) reporting that 4 tests pass on tip18. I did not re-run them, because they need the patch applied to a tree.
- JetLSP on dev:
  - `Requests.jet` `lsp_handle_request` has no `jet/` method and no `workspace/executeCommand`; anything else gets -32601 `method not found: {method}`.
  - `lsp_handle_notification` handles exit, didOpen, didChange and didClose, and drops anything else at `else -> {}`.
  - `lsp_initialize_response` advertises `textDocumentSync: 2` and the providers. It has no `experimental`, no `executeCommandProvider` and no `positionEncoding`.
  - `Server.jet` `lsp_update_document` ignores languageId and drops a didChange whose version is `<= current`. `overlays()` passes every open document as `LSPOverlay{path, source}`.
  - `Check.jet` `LSPChecker` takes `[LSPOverlay]` in `check`, `stage` and `failure_marks`.
  - `Diagnostics.jet` gates the published version on `sha256(open.text) == origin.revision` (`lsp_document_group_json`). `lsp_close_document` clears every target URI that root published.
  - `Loop.jet` handles messages strictly in order and calls `lsp_flush_dirty` before each request, so publication after a didChange is driven by requests.
  - `Position.jet` `lsp_checked_offset_bytes` refuses a position inside a surrogate pair and clamps past a line's end. `lsp_utf16_length` is the server's UTF-16 counter.
- Tools/Editors:
  - VS Code `extension.js` uses vscode-languageclient ^9.0.1 with `documentSelector: [{scheme: "file", language: "jet"}]`.
  - `fetchReasoning` (line 543) sends `workspace/executeCommand` `jet.reasoning`. The middleware at line 808 intercepts that command on the client side.
  - Zed `WasmSrc/src/lib.rs` implements only `language_server_command`. There is no Neovim recipe in the repo (`git grep`).

## 1. Strongest existing solutions, stated fairly

| System | How the host's embedded code reaches the server | Best case | What it shows that the draft left out |
|---|---|---|---|
| LSP 3.17 notebook sync | `notebookDocument/didOpen\|didChange\|didSave\|didClose`. Cells are text documents with their own URIs. Cell text is synced through notebook messages "so that the cell and its text content arrives on the server using one open, change or close event". | **The standard's own answer**: atomic, versioned, several code parts per document. | Missing from the draft. It works only for real notebooks: vscode-languageclient's `notebook.ts` syncs `vscode.workspace.notebookDocuments` only, and Neovim's `make_client_capabilities` advertises no notebook support. Results land on cell URIs, so for a text host the client maps positions. Source: microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#notebookDocument_synchronization. |
| Volar (Vue, Astro) | Language plugins **inside the language server** find blocks and build `VirtualCode{id, languageId, snapshot, mappings: CodeMapping[]}`. Each mapping has `CodeInformation{verification, completion, semantic, navigation, structure, format}`. | One server-side mapping layer for every feature. | The draft called Volar "the T3 shape" (client-side). It is server-side: @vue/language-server and @astrojs/language-server depend on @volar/language-server, and the client sends plain didOpen of `.vue`. Volar is the E shape (the host format's own server finds regions), with A's one map. Source: raw.githubusercontent.com/volarjs/volar.js/master/packages/language-core/lib/types.ts. |
| Svelte | svelte-language-server runs `svelte2tsx` on `.svelte` and keeps a source map. | The host format's own server parses the host. | Like Volar, this is the E shape, and it is why E is the only shape that needs no client code. |
| VS Code "Embedded Programming Languages" guide | It shows two shapes: language services inside the server (whitespace-masked virtual documents), and request forwarding to `embedded-content://` virtual documents that the client maps. | It recommends language services. | Request forwarding is the true client-side shape (D). The guide's "Potential issues" warns that rename and formatting across regions are hard, and that escaping such as `</` matters. Source: code.visualstudio.com/api/language-extensions/embedded-languages. |
| otter.nvim (Quarto) | Hidden buffers such as `index.md.tmp.py` with every non-code line blanked, so line numbers match. | Neovim client-side mapping that works today. | Also the D shape. It shows the whitespace-mask identity mapping (p1: masked text is 56 bytes, mapping 25..46 → 25..46). Source: github.com/jmbuhr/otter.nvim README. |
| rust-analyzer | Capabilities go under `experimental`. Methods use the `experimental/` or `rust-analyzer/` prefix, including client→server notifications such as `rust-analyzer/runFlycheck`. | A server-prefixed custom method behind an `experimental` capability. | A's convention holds. Source: github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/lsp-extensions.md. |
| typescript-language-server | Client→server actions go through `workspace/executeCommand`, including the state push `_typescript.configurePlugin`. It also has a custom `$/typescriptVersion` server→client notification. | **C's real precedent**, missing from the draft. | Source: github.com/typescript-language-server/typescript-language-server README, "Workspace commands". |
| Client hooks | vscode-languageclient 9.0.1 has `middleware.didOpen/didChange(…, next)`, and `GeneralMiddleware.sendNotification(type, next, params)` (client.ts:327) sees every notification, didChange included (textSynchronization.ts:383). Neovim has the `LspNotify` autocmd after each notification. `Client:request` flushes pending didChange first, and `cmd` may be a function that returns a wrapped RPC client. eglot flushes didChange before requests (`eglot--request`) and has a private `eglot--send-changes-hook`. Zed's `zed_extension_api` `Extension` trait has no document events and cannot send LSP messages. Helix has no plugins. | | These decide which options each client can send (F4, F5). Sources: github.com/microsoft/vscode-languageserver-node (client/src/common/client.ts, textSynchronization.ts, notebook.ts at 9.0.1); neovim runtime/doc/lsp.txt and runtime/lua/vim/lsp/client.lua; eglot.el; zed crates/extension_api/src/extension_api.rs. |

The draft cited only Volar (mislabeled), Svelte and rust-analyzer. With the right Volar label, the prior art says two things. The host format's own server usually finds regions (E), which D-EMBED-REGION1 rules out for the compiler but not for a separate adapter. The standard atomic form is notebook sync, which the draft never weighed (F12, F13).

## 2. Claims and verdicts

### C1. Situation: "That decision fixed what the host supplies, not how the editor sends it … the VS Code extension's one custom call goes through `workspace/executeCommand`"
The first half holds (section 0). The second half is literally true as a call site, but the server answers that call with -32601 (p3 `executeCommand`: `response id=2 {"code":-32601,"message":"method not found: workspace/executeCommand"}`), so it is no precedent (F10, D1).
**Verdict: holds within limits.**

### C2. Detail, ratified ground
Every quote matches `out-embed-region1.txt`. The draft then read "Editor conversion uses UTF-16 units, not byte offsets" as compatible with UTF-8 byte offsets on the editor-facing wire, "the unit the decision's example uses". The example's bytes are the compiler's unit. The decision's Volar row contrasts them with UTF-16 positions, and the clause assigns UTF-16 to the editor side (F8).
**Verdict: holds within limits.**

### C3. "What is built. JetDriver gains `JetDriverSourceDocument` … Nothing in JetLSP accepts a host document yet."
`SourceDocuments.jet` is not on dev at dc046ef3d1 or 6d87b75ed5. It exists only in `land/6903-c4ab.patch`. The checkInstructions sent the owner to a file that does not exist (F2).
**Verdict: fails.**

### C4. Whole-mechanism inventory
The draft has no inventory. Under ballot-review.md, a missing site, a second store or an untreated sibling is a failed claim. Missing:
- the languageId gate in `lsp_update_document`;
- the one document store versus a second host store;
- `overlays()`/`LSPOverlay` and the three `LSPChecker` methods, which must take `JetDriverSourceDocument` per "one SourceDocument input";
- the version/digest gate in `lsp_document_group_json`, and target clearing in `lsp_close_document`;
- request-driven publication in `lsp_flush_dirty`/Loop.jet;
- every provider's position conversion, plus a **second UTF-16 counter**: `jet_driver_utf16_offset` beside Position.jet `lsp_utf16_length`;
- the request parking the E1 fix needs in Loop.jet, and its meeting with D-LSP-CANCEL1;
- the extension's selector, middleware and dead call;
- CLI `jet check` of host files (c4c), which the draft did not say it was keeping separate (F1).

**Verdict: fails.**

### C5. "What every option shares"
- Byte offsets on the wire: F8.
- One Jet module per host cannot carry Markdown's independent examples. 108 tracked `.md` files hold ```` ```jet ```` blocks, and one proposal holds 63. The host "retain[s] control of module scope" (F9).
- "A diagnostic that lies wholly in generated text" disagrees with the driver: `jet_driver_host_span` returns `Generated` for any span that reaches generated text or crosses regions. Navigation targets in generated text are untreated (F17).
- Module-URI rules are absent: collision with an open or on-disk `.jet`, non-`file:` hosts, and package resolution (F11).

**Verdict: fails.**

### C6. Beginner path: "The first-party extensions (Tools/Editors/Vscode, Tools/Editors/Zed, and the Neovim recipe) find the regions … and send the message"
- Zed's extension API cannot send any LSP notification or request (extension_api.rs).
- No Neovim recipe exists.
- Helix cannot send, and eglot's one server per buffer means jet lsp replaces the HTML server.

Under T1, T2 and T5, Zed and Helix get nothing, and the draft never said so (F4).
**Verdict: fails.**

### C7. T1 behavior and its E1–E4 answers
- **E1, "publishes nothing, so no result is ever shown for the wrong text"**: false. Publishing nothing is not withdrawing. vscode-languageclient `handleDiagnostics` (client.ts:1787-1798) and Neovim `on_publish_diagnostics` set diagnostics by URI and ignore `version`, so version 7's diagnostics stay on screen (F6).
- **E1, "host-URI requests answer null until the matching hostDocument arrives"**: this breaks completion in the common case. Neovim `Client:request` and eglot `eglot--request` flush didChange right before the request. A hostDocument sent after the didChange (from `LspNotify`, or VS Code's async middleware) can arrive after the completion request (F7).
- **Lifecycle gaps**: no didClose rule, no reopen with versions restarting, no bound on held messages, and no E6 for clients that never send (F11).
- **Publication timing**: the code shows a publish right after hostDocument, but today's server publishes only before a request (p3 `publishTiming`: `after didChange v2 (no request): nothing`; `after hover: publishDiagnostics … version=2`). The ballot must say which applies to hosts (F14).
- E2's refusal and E4's stale and newer rules are sound.

**Verdict: fails.**

### C8. T2, at its strongest, and its whyNot "Neovim and Zed offer no hook for extra didChange members, so clients would have to rebuild document sync"
- False for VS Code. `GeneralMiddleware.sendNotification` sees and can rewrite every didChange.
- False for Neovim. `cmd` as a function can wrap the RPC client.
- True for Zed and Helix.
- T2's real costs are absent: transport wrapping, and vscode-languageclient capturing the version before async middleware (textSynchronization.ts:370) so the member can describe newer text.
- Today's server ignores the member (p3 `t2Member`: the html host is still checked as Jet) (F5).

**Verdict: fails.**

### C9. T3 "(the Volar shape)", ruled out by D-EMBED-REGION1
The ruling holds: D-EMBED-REGION1 chose A over host-owned mapping (its B). The label is wrong, because Volar is server-side (F12).
**Verdict: holds within limits.**

### C10. T4, ruled out
The ruling holds for the compiler. T4 is not stated at its strongest. It is the only option that works in Zed, Helix and eglot with no client code, and it is how Svelte and Vue do it. A host adapter outside the compiler that speaks T1 keeps that gain without breaking the decision (F13).
**Verdict: fails as stated.**

### C11. T5: "as the VS Code extension's reasoning panel already calls the server"; "Editors list executeCommand commands in user-facing command menus"
- The precedent is dead (-32601, D1).
- VS Code shows a command in the palette only if an extension contributes it.
- Helix's `:lsp-workspace-command` picker does list server commands.
- The real precedent (`_typescript.configurePlugin`) and the request's one real gain (the client learns when modules are in place) are missing (F10).

**Verdict: fails.**

### C12. Option coverage
Notebook sync is missing (F13).
**Verdict: fails.**

### C13. Comparisons
Volar is mislabeled, there are no sources, and notebooks, the VS Code guide, otter.nvim and typescript-language-server are missing (F12).
**Verdict: fails.**

### C14. Recommendation gains, losses and tradeoff
- Gain 1 ("VS Code, Zed and Neovim … can send one extra notification without replacing their document sync") is false for Zed (F4).
- Loss 1's mitigation is false (F6).
- The hidden costs are not stated (F16):
  - every host keystroke resends all module text;
  - every HTML/Markdown file is synced to jet lsp once the selector grows;
  - a new extension against an old `jet` sends host documents that today's server checks as Jet (p3 `hostDidOpen`).

**Verdict: fails.**

### C15. Surface
- `trio.current` says "Today the server takes only Jet files. A Jet block inside an HTML page gets no checking". p3 `hostDidOpen` shows the opposite: `publishDiagnostics uri=file://$DIR/panel.html version=7 [[0,0,0,2,"E0001","The character `🙂` doesn't mean anything here (yet)"]]` (F3).
- Keys T1–T5 fail Validate.jet's new-ballot rule, and the draft had eight plain-language gaps (F15).

**Verdict: fails.**

### C16. inWild and story arithmetic
p1 checks the inWild numbers:
- region host bytes 25..46, UTF-16 23..44;
- `pront` at Jet 11..16, host 36..41, line 2 characters 11..16;
- after E1's `!`, the region is at 26..47.

p2 checks the story's fix: `jet check --json` gives E0102 at 11..16, with `fix_edits` `print` marked `behavior-preserving`, `applicability: safe`. That edit lies inside the mapping, so the story's "a quick fix must edit her page" holds through `jet_driver_host_edit`.
**Verdict: holds.**

### C17. checkInstructions
They point at absent files (F2).
**Verdict: fails.**

### Checklist (AGENTS.md "Adversarial design review" and ballot-review.md)

| Item | State in the draft |
|---|---|
| Every site of the mechanism named, one source of truth | Fails (C4; F1) |
| Second store or second maker | Fails: a host store and a second UTF-16 counter were unaddressed (F1) |
| Siblings (CLI host check, session tiers, existing diagnostic-target publication) | Fails: no reason given for keeping the CLI separate (F1) |
| Strongest prior art, sourced | Fails (F12) |
| Each option at its strongest | Fails for T2 and T4; notebook sync missing (F5, F13) |
| Ratified text quoted and applied correctly | Holds within limits (unit reading, F8) |
| Edge cases | Fails (F6, F7, F11) |
| Beginner and expert paths | Beginner path fails (F4); expert path holds |
| Built state stated truly | Fails (F2) |
| Examples in Jet, not Rust (owner comment) | Holds: wire examples are JSON, and the repaired A adds a Jet `JetDriverSourceDocument` example copied from the patch's own syntax |

## 3. Findings (round 0)

Each fix below is now in the ballot text (section 4).

- **F1 Inventory.** Add a cutover inventory naming each `file:symbol`:
  - Requests.jet `lsp_handle_notification`, `lsp_initialize_response`;
  - Server.jet `lsp_update_document` (the languageId gate; host documents join the one store as a host kind, not a second store), `overlays()`, `LSPOverlay`;
  - Check.jet `LSPChecker.check/stage/failure_marks`, which take `JetDriverSourceDocument`, the one input c4c moves callers to;
  - Diagnostics.jet `lsp_publish_document_diagnostics`, `lsp_document_group_json`, `lsp_close_document`;
  - every provider's conversion through the one driver map, with `jet_driver_utf16_offset` reusing Position.jet's UTF-16 count rather than adding a second counter;
  - Loop.jet parking, which meets D-LSP-CANCEL1;
  - extension.js.

  Also say why `jet check` of host files (c4c) stays outside: the CLI reads files, not editor versions.
- **F2 Built state.** Replace "What is built" with "What is proposed underneath": the unlanded `land/6903-c4ab.patch` (c4a, c4b) on dc046ef3d1. Point checkInstructions at the patch.
- **F3 Today's server.** Replace `trio.current` with the probed behavior: an `html` document is checked as Jet. State the languageId gate as a shared rule. Add a "Today's server, probed" paragraph.
- **F4 Client reach.**
  - Remove Zed from the gains.
  - Say the Neovim recipe does not exist yet, and that eglot's one server per buffer displaces the HTML server.
  - State that Zed and Helix get nothing under A, B, C and F.
  - Name a host adapter (a separate process that speaks A) as their path, unbuilt.
  - Add this as a recommendation loss.
- **F5 B at its strongest.**
  - Correct the technical text and whyNot: VS Code uses `middleware.sendNotification`, Neovim a `cmd` function, Emacs advice, and only Zed and Helix cannot.
  - State the real costs: transport wrapping, async-middleware version drift, and typed re-encoders that may drop the member.
- **F6 E1 display.** State that the last diagnostics stay on screen, with the client sources showing they ignore `version`. Note that plain Jet files behave the same today (p3 `publishTiming`).
- **F7 E1 requests.** Park host-URI requests until that version's modules arrive. Answer ContentModified (-32801) when a newer didChange or didClose supersedes them.
- **F8 Unit.** Make mappings LSP ranges in the session's position encoding (UTF-16 by default), converted by the server's existing checked code. Refuse an end past the line's end instead of clamping. State why bytes lose: every JavaScript client converts.
- **F9 Modules.** Make the wire carry `modules: [{uri, text, mappings}]`, zero or more. Mappings of different modules must not overlap, and zero modules withdraws the host's results. Update the story.
- **F10 C's precedent.** Say the VS Code call is dead (-32601, D1). Cite `_typescript.configurePlugin`. Correct the menu claim: Helix lists server commands, while VS Code shows only contributed ones. Add C's real gain, that the client learns when modules are in place.
- **F11 Lifecycle and URIs.** Add:
  - E5: close, and reopen with a fresh version sequence;
  - E6: a client that never sends;
  - one held notification per host URI;
  - didClose clears the host and module URIs;
  - module-URI rules: a `file:` URI naming no file on disk, no open Jet document and no other host's module; otherwise E2; its directory decides the package.
- **F12 Comparisons.**
  - Relabel Volar as server-side (Vue and Astro).
  - Add LSP notebooks, the VS Code embedded-languages guide, otter.nvim and typescript-language-server, with sources for every row.
  - Point D at request forwarding and otter.nvim instead of Volar.
- **F13 Options.**
  - Add notebook sync (F) at its strongest, with E1–E6 and a whyNot.
  - State E at its strongest: the only option needing no client code, as Svelte and Vue do it. Note that a host adapter speaking A keeps that gain without breaking the ratified decision.
- **F14 Publication.** For hosts, the server checks and publishes on hostDocument receipt. The E1 window rules (F6, F7) cover the gap.
- **F15 Validator.** Rekey the options as A–F with the recommendation first as A, and fix the eight plain-language gaps (`out-validate-before.txt`).
- **F16 Hidden costs.** State that every keystroke resends every module's full text, with the remap-only reuse when nothing read has changed. State that every HTML/Markdown file is synced once the selector grows. State that the middleware withholds host documents until the server advertises the capability, because today's server checks them as Jet.
- **F17 Generated text.** Say "a diagnostic that reaches generated text, or crosses regions", matching `jet_driver_host_span`. Drop navigation targets in generated text, because no editor can open them.

### Code defects found (not ballot text)

- **D1, VS Code reasoning panel is dead.** `Tools/Editors/Vscode/extension.js:543` (`fetchReasoning`) sends `workspace/executeCommand` `jet.reasoning`. Today's server answers -32601: the server side went with the Jet rewrite 590841dff5 (#6650), and `Tests/LSP/Protocol.jet:122` asserts -32601 for this method. `Tests/Cases/LSP/lsp_execute_command_impact_returns_report.case` expects an `executeCommandProvider` but is PENDING-GRAMMAR.
  - Reproduction: `node review-probes/p3-lsp.mjs $JET_SCRATCH_ROOT/tmp/orch/tip18-chain/s1/jet executeCommand` prints `response id=2 {"code":-32601,"message":"method not found: workspace/executeCommand"}`.
- **D2, a non-Jet document is checked as Jet.** `Server.jet` `lsp_update_document` ignores languageId. A client that opens an HTML page gets Jet diagnostics on it. Today's VS Code selector sends only `jet`, so this is latent there, but any generic client pointed at HTML shows it. c4e needs the gate anyway.
  - Reproduction: `node review-probes/p3-lsp.mjs <jet> hostDidOpen` prints `publishDiagnostics uri=file://$DIR/panel.html version=7 [[0,0,0,2,"E0001","The character `🙂` doesn't mean anything here (yet)"]]`.
- **D3, shutdown with `"params": null` hangs the client.** `shutdown` or `exit` sent with `"params": null` gets `{"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid Request"}}`. The id was readable, and JSON-RPC 2.0 reserves a null id for when the id cannot be detected, so the client's shutdown request never gets its answer. `exit` is refused too, and the server exits 1 at end of input. Sent without `params`, it answers `{"jsonrpc":"2.0","id":2,"result":null}` and exits 0. Rejecting `null` params is spec-permitted; the `id: null` is the defect.
  - Reproduction: `node review-probes/p4-exit.mjs <jet>`.

## 4. Re-review log

- 2026-11-24, round 0: findings F1–F17 and D1–D3, as above. `Validate.jet` on the received draft (`out-validate-before.txt`) reports 5 gaps: `posting payload must set draft:false`; `rec (new ballots list the recommended option first, as A)`; `reviewPasses.adversarial (need text)`; the prefix gap; and plain language (explainer, story, detail ×3, T1 ×2, T3, T4).
- 2026-11-24, round 1: my repairs, written to the ballot after copying it to `.before-review`. The draft's mtime was 02:11:12 before and was unchanged when I wrote. Every F1–F17 fix is in the text as listed in section 3, with T1→A, T2→B, T5→C, T3→D, T4→E and the new F. I then attacked the repaired A again:
  - **R1** (new). Parking a host-URI request until "that version's modules arrive" waits forever for a client that never sends modules (E6). Fixed: "Parking starts only once the host has had a hostDocument since it was opened. Before that, requests answer null, so a client that never sends modules never waits."
  - **R2** (new). "If every module text equals the last checked one, the server reuses that check" is wrong when a file the module imports changed. Fixed: "If the module texts and everything they read are unchanged since the last check". The recommendation loss's mitigation now says the same. The server already tracks readers (`checked_modules`, `lsp_reads_any`).
  - **R3** (new, sourcing). The typescript-language-server README does not say a leading `_` marks internal commands; that is VS Code's convention. Fixed: "Its leading `_` follows VS Code's habit of marking internal commands."
  - Rechecked against sources:
    - vscode-languageclient `handleDiagnostics` ignores `version` (client.ts:1787-1798);
    - Neovim `diagnostic.lua` has no version check;
    - Neovim `LspNotify` fires "after each successful notification sent to an LSP server" (lsp.txt:791-796);
    - `make_client_capabilities` (protocol.lua:356 on) has no notebook entry;
    - typescript-language-server README lists `_typescript.configurePlugin`;
    - D-EMBED-REGION1's text matches every quote.
  - The repaired A still beats B. B's atomicity is real in Neovim, but in VS Code the version is captured before async middleware, so the server must validate against held text anyway. B also needs a transport wrapper in every client, while A needs a post-sync hook, and A pairs version to version explicitly.
  - `Validate.jet` on the written ballot (`out-validate-after.txt`):

    ```text
    note: Tools/Agent/Ballots/Validate.jet ends its own process and reads files (-A hides this)
    $JET_SCRATCH_ROOT/tmp/orch/ballots/D-LSP-HOSTDOC1.json (D-LSP-HOSTDOC1): 1 gap
      - posting payload must set draft:false
    ```

    The one gap is the stored-draft flag, which posting through Tower clears. I did not touch Tower.

## 5. Overall verdict

Round 0: **fails** (17 findings). The recommended wire (a `jet/hostDocument` notification) was the right family. But the draft:
- presented an unlanded patch as built and had no cutover inventory;
- claimed Zed support that the Zed API cannot give;
- misstated T2's cost and T5's precedent, and mislabeled Volar;
- promised "no result for the wrong text" while clients keep old diagnostics, and answered requests with null in the window where completions land;
- put byte offsets on the editor wire against the ratified clause;
- allowed only one module per host, which fails Markdown;
- omitted notebook sync.

Round 1 (final, my repairs and three self-found fixes): **holds within limits**. Every claim holds or holds within these named limits:
- The driver side (`land/6903-c4ab.patch`, c4a/c4b) must land first. c4c moves `LSPOverlay` and the CLI callers to `JetDriverSourceDocument`.
- Request parking is new Loop.jet machinery and must agree with D-LSP-CANCEL1, which is still a draft.
- Zed and Helix get nothing in host files until a host adapter exists (unbuilt). Neovim needs a recipe that does not exist yet.
- D1–D3 are code defects outside the ballot text.

| Claim | Round 0 | Round 1 |
|---|---|---|
| C1 situation | holds within limits | holds |
| C2 ratified ground | holds within limits | holds |
| C3 built state | fails | holds |
| C4 inventory | fails | holds within limits (c4ab, c4c prerequisites) |
| C5 shared rules | fails | holds |
| C6 beginner path | fails | holds within limits (no Zed/Helix; Neovim recipe unwritten) |
| C7 A and E1–E6 | fails | holds within limits (parking machinery, D-LSP-CANCEL1) |
| C8 B | fails | holds |
| C9 D | holds within limits | holds |
| C10 E | fails as stated | holds |
| C11 C | fails | holds |
| C12 option coverage | fails | holds |
| C13 comparisons | fails | holds |
| C14 recommendation | fails | holds |
| C15 surface | fails | holds |
| C16 inWild and story | holds | holds |
| C17 checkInstructions | fails | holds |

`reviewPasses.adversarial` as written into the ballot. It passes `Validate.jet` with only the draft gap left. The validator allows one sentence after the four-sentence prefix here, because the prefix's last sentence counts as the first.

> Author model family: Claude Opus. Adversarial model family: Claude Opus. Fresh agent: HostDocReview. The adversarial review attacked the recommendation. Round 0 failed on a false built state, false Zed and VS Code claims, the request window and missing options; the repaired ballot holds.

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| F1 | no-action | archived: fixed in the ballot text before posting |
| F2 | no-action | archived: fixed in the ballot text before posting |
| F3 | no-action | archived: fixed in the ballot text before posting |
| F4 | no-action | archived: fixed in the ballot text before posting |
| F5 | no-action | archived: fixed in the ballot text before posting |
| F6 | no-action | archived: fixed in the ballot text before posting |
| F7 | no-action | archived: fixed in the ballot text before posting |
| F8 | no-action | archived: fixed in the ballot text before posting |
| F9 | no-action | archived: fixed in the ballot text before posting |
| F10 | no-action | archived: fixed in the ballot text before posting |
| F11 | no-action | archived: fixed in the ballot text before posting |
| F12 | no-action | archived: fixed in the ballot text before posting |
| F13 | no-action | archived: fixed in the ballot text before posting |
| F14 | no-action | archived: fixed in the ballot text before posting |
| F15 | no-action | archived: fixed in the ballot text before posting |
| F16 | no-action | archived: fixed in the ballot text before posting |
| F17 | no-action | archived: fixed in the ballot text before posting |
| R1 | no-action | archived: fixed in the ballot text before posting |
| R2 | no-action | archived: fixed in the ballot text before posting |
| R3 | no-action | archived: fixed in the ballot text before posting |
| D1 | card | #6903 |
| D2 | card | #6903 |
| D3 | card | #6903 |
<!-- /audit-dispositions -->
